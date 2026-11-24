//! A scripted radio controller and gamepad for the pad host, on the PC.
//!
//! The test plays the controller (it answers every command, gives
//! buffers back after each data packet, and confirms a Disconnect) and
//! the gamepad (its events and L2CAP messages, written from the specs,
//! and its SDP answers and reports, taken from the owner's live logs:
//! see `fixtures.rs`). Time only moves when the test says so.
//!
//! All addresses and keys here are made up.
#![allow(dead_code)]

pub mod fixtures;

use std::collections::VecDeque;
use std::fmt;

use pad_host::{PadEvent, PadHost, Radio, TX_MAX};
use pad_proto::acl;
use pad_proto::hci::{BdAddr, op};
use pad_proto::l2cap::{self, code, conn_result};
use pad_proto::sdp;
use pad_store::{Flash, Pairing, ReadFlash, Region, Span, Store, check_region};

/// Every address and key in this file is made up for the tests.
pub const PAD: BdAddr = BdAddr([0xA6, 0xA5, 0xA4, 0xA3, 0xA2, 0xA1]);
/// Someone else's device: never welcome.
pub const STRANGER: BdAddr = BdAddr([0x9A, 0xE7, 0x5C, 0xB1, 0xD3, 0x4F]);
/// Peripheral, gamepad (as the live logs: Switch and XW modes differ a
/// little; the fixtures carry their own).
pub const PAD_COD: u32 = 0x002508;
pub const H: u16 = 0x0080;
/// A made-up link key: bytes without a pattern, so that a test can
/// search every output for them.
pub const KEY: [u8; 16] = [
    0x5E, 0xC7, 0x3E, 0x7A, 0x91, 0x0B, 0x42, 0xD8, 0x6F, 0x13, 0xA0, 0x2C, 0x88, 0x4D, 0xE1, 0x57,
];
/// The pad's channel ids (as in the live log).
pub const PAD_CTRL: u16 = 0x0044;
pub const PAD_INTR: u16 = 0x0046;
pub const PAD_SDP: u16 = 0x0045;

// ------------------------------------------------------------ the flash

pub const NVS: Span = Span {
    offset: 0x9000,
    len: 0x6000,
};

pub fn region() -> Region {
    check_region(NVS, &[], 0x40_0000).unwrap()
}

/// 64 KB of flash in RAM.
#[derive(Clone)]
pub struct Ram(pub Vec<u8>);

impl Ram {
    pub fn blank() -> Self {
        Ram(vec![0xFF; 0x1_0000])
    }
    /// A flash with these pairings saved (oldest first), written by
    /// pad-store's normal (writing) path, as the pairing firmware did.
    pub fn with(saved: &[(BdAddr, [u8; 16])]) -> Self {
        let mut ram = Ram::blank();
        {
            let (mut store, _) = Store::open(&mut ram, region()).unwrap();
            for (a, k) in saved {
                store
                    .save(&Pairing {
                        addr: a.0,
                        key: *k,
                        key_type: 0x04,
                    })
                    .unwrap();
            }
        }
        ram
    }
}

impl Flash for Ram {
    type Error = ();
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), ()> {
        let o = offset as usize;
        buf.copy_from_slice(self.0.get(o..o + buf.len()).ok_or(())?);
        Ok(())
    }
    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), ()> {
        let o = offset as usize;
        for (m, d) in self
            .0
            .get_mut(o..o + data.len())
            .ok_or(())?
            .iter_mut()
            .zip(data)
        {
            *m &= d;
        }
        Ok(())
    }
    fn erase_sector(&mut self, offset: u32) -> Result<(), ()> {
        let o = offset as usize;
        self.0.get_mut(o..o + 4096).ok_or(())?.fill(0xFF);
        Ok(())
    }
}

/// The view the pad host gets: reads only, and counted.
pub struct ReadOnly<'a> {
    pub ram: &'a Ram,
    pub reads: u32,
}

impl ReadFlash for ReadOnly<'_> {
    type Error = ();
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), ()> {
        self.reads += 1;
        let o = offset as usize;
        buf.copy_from_slice(self.ram.0.get(o..o + buf.len()).ok_or(())?);
        Ok(())
    }
}

/// CRC-32 of the 8 KB pairing area, as the firmware logs it.
pub fn area_crc(ram: &Ram) -> u32 {
    let b = NVS.offset as usize;
    pad_store::crc::crc32(&ram.0[b..b + 8192])
}

// -------------------------------------------------------------- the radio

#[derive(Default)]
pub struct Rec {
    pub out: Vec<Vec<u8>>,
    pub logs: Vec<String>,
}

impl Radio for Rec {
    fn send(&mut self, p: &[u8]) {
        assert!(p.len() <= TX_MAX, "packet of {} bytes", p.len());
        self.out.push(p.to_vec());
    }
    fn log(&mut self, tag: &'static str, args: fmt::Arguments<'_>) {
        let line = format!("{tag} {args}");
        if std::env::var_os("PAD_TRACE").is_some() {
            println!("{line}");
        }
        self.logs.push(line);
    }
}

// ------------------------------------------------------------- simulator

pub type Sig = (u8, u8, Vec<u8>);

pub struct Sim {
    pub host: PadHost,
    pub radio: Rec,
    pub ram: Ram,
    pub now: u64,
    seen: usize,
    pub cmds: VecDeque<(u16, Vec<u8>)>,
    pub all_cmds: Vec<u16>,
    /// Every command with its parameters, in order.
    pub history: Vec<(u16, Vec<u8>)>,
    pub sigs: VecDeque<Sig>,
    /// Frames we sent on channels other than signalling: (their cid,
    /// payload).
    pub data: VecDeque<(u16, Vec<u8>)>,
    reasm: acl::Reassembler<2048>,
    pad_id: u8,
    /// Times `tick` said "wake".
    pub wakes: Vec<u64>,
    pub events: Vec<PadEvent>,
    /// Every packet handed to the host, in order (for the fuzz tests).
    pub fed: Vec<(u64, Vec<u8>)>,
    /// The controller confirms a Disconnect with Disconnection Complete
    /// (reason 0x16). False: it never does (a controller fault).
    pub confirm_disconnect: bool,
    /// The controller answers commands. False: it is dead.
    pub answer_commands: bool,
    /// The name the pad gives when asked (Remote Name Request).
    pub pad_name: Option<String>,
    /// Most packets and log lines one call produced.
    pub most_out_per_call: usize,
    pub most_logs_per_call: usize,
    /// How long each packet from the controller waits before the host
    /// sees it (ms): 0 as the test firmware (read at once); a frame's time
    /// as the console's loop, which polls the radio between frames.
    pub feed_delay_ms: u64,
    /// The handle the controller gives the next link, and the pad's
    /// frames use ([`H`] unless a test changes it).
    pub handle: u16,
    /// Commands the controller answers with this status instead of 0.
    pub refuse: Option<(u16, u8)>,
    /// Everything on the wire in order: (to the controller?, packet).
    pub wire: Vec<(bool, Vec<u8>)>,
}

impl Sim {
    pub fn new(saved: &[(BdAddr, [u8; 16])]) -> Self {
        let ram = Ram::with(saved);
        Self::with_flash(ram)
    }

    pub fn with_flash(ram: Ram) -> Self {
        let mut s = Self {
            host: PadHost::new(),
            radio: Rec::default(),
            ram,
            now: 1_000,
            seen: 0,
            cmds: VecDeque::new(),
            all_cmds: Vec::new(),
            history: Vec::new(),
            sigs: VecDeque::new(),
            data: VecDeque::new(),
            reasm: acl::Reassembler::new(),
            pad_id: 0x80,
            wakes: Vec::new(),
            events: Vec::new(),
            fed: Vec::new(),
            confirm_disconnect: true,
            answer_commands: true,
            pad_name: None,
            most_out_per_call: 0,
            most_logs_per_call: 0,
            feed_delay_ms: 0,
            handle: H,
            refuse: None,
            wire: Vec::new(),
        };
        let before = area_crc(&s.ram);
        let mut view = ReadOnly {
            ram: &s.ram,
            reads: 0,
        };
        let loaded = s.host.load_pairings(&mut view, region(), &mut s.radio);
        assert!(loaded.is_ok());
        assert_eq!(area_crc(&s.ram), before);
        s.host.start(s.now, &mut s.radio);
        s.settle();
        s.cmds.clear();
        s
    }

    /// Runs the host until it waits for a saved pad (start-up done).
    pub fn until_waiting(&mut self) {
        for _ in 0..500 {
            if self.host.waiting() {
                return;
            }
            self.advance(10);
        }
        panic!("never waiting; log:\n{}", self.tail_log());
    }

    pub fn feed(&mut self, p: &[u8]) {
        if self.feed_delay_ms > 0 {
            self.advance(self.feed_delay_ms);
        }
        self.fed.push((self.now, p.to_vec()));
        self.wire.push((false, p.to_vec()));
        let (o, l) = (self.radio.out.len(), self.radio.logs.len());
        self.host.packet(self.now, p, &mut self.radio);
        self.note_work(o, l);
        self.settle();
    }

    fn note_work(&mut self, o: usize, l: usize) {
        for p in &self.radio.out[o..] {
            self.wire.push((true, p.clone()));
        }
        self.most_out_per_call = self.most_out_per_call.max(self.radio.out.len() - o);
        self.most_logs_per_call = self.most_logs_per_call.max(self.radio.logs.len() - l);
    }

    /// One tick at the current time.
    pub fn tick(&mut self) -> bool {
        let (o, l) = (self.radio.out.len(), self.radio.logs.len());
        let wake = self.host.tick(self.now, &mut self.radio);
        self.note_work(o, l);
        if wake {
            self.wakes.push(self.now);
        }
        while let Some(e) = self.host.take_event() {
            self.events.push(e);
        }
        self.settle();
        wake
    }

    /// Moves time on in steps of 10 ms, ticking like the firmware's loop.
    pub fn advance(&mut self, ms: u64) {
        let end = self.now + ms;
        while self.now < end {
            self.now = (self.now + 10).min(end);
            self.tick();
        }
    }

    /// Plays the controller: answers commands, gives data buffers back.
    pub fn settle(&mut self) {
        while self.seen < self.radio.out.len() {
            let p = self.radio.out[self.seen].clone();
            self.seen += 1;
            match p[0] {
                0x01 => {
                    let opcode = u16::from_le_bytes([p[1], p[2]]);
                    assert_eq!(usize::from(p[3]), p.len() - 4, "command length byte");
                    let params = p[4..].to_vec();
                    self.cmds.push_back((opcode, params.clone()));
                    self.all_cmds.push(opcode);
                    self.history.push((opcode, params.clone()));
                    if self.answer_commands {
                        self.controller_answer(opcode, &params);
                    }
                }
                0x02 => {
                    let (h, d) = acl::parse(&p[1..]).expect("acl header");
                    assert!(d.len() <= acl::MAX_FRAGMENT);
                    if let Some(f) = self.reasm.push(h.pb, d).expect("our frames join up") {
                        let cid = u16::from_le_bytes([f[2], f[3]]);
                        let payload = f[4..].to_vec();
                        if cid == l2cap::CID_SIGNALLING {
                            let mut q = &payload[..];
                            while q.len() >= 4 {
                                let len = usize::from(u16::from_le_bytes([q[2], q[3]]));
                                self.sigs.push_back((q[0], q[1], q[4..4 + len].to_vec()));
                                q = &q[4 + len..];
                            }
                        } else {
                            self.data.push_back((cid, payload));
                        }
                    }
                    let mut e = vec![1];
                    e.extend_from_slice(&h.handle.to_le_bytes());
                    e.extend_from_slice(&1_u16.to_le_bytes());
                    self.inner_feed(&evt(0x13, &e));
                }
                k => panic!("packet of kind {k:#04x} sent to the controller"),
            }
        }
    }

    fn inner_feed(&mut self, p: &[u8]) {
        self.fed.push((self.now, p.to_vec()));
        self.wire.push((false, p.to_vec()));
        let (o, l) = (self.radio.out.len(), self.radio.logs.len());
        self.host.packet(self.now, p, &mut self.radio);
        self.note_work(o, l);
    }

    fn controller_answer(&mut self, opcode: u16, params: &[u8]) {
        let status_only = [
            op::INQUIRY,
            op::CREATE_CONNECTION,
            op::DISCONNECT,
            op::ACCEPT_CONNECTION_REQUEST,
            op::REJECT_CONNECTION_REQUEST,
            op::AUTHENTICATION_REQUESTED,
            op::SET_CONNECTION_ENCRYPTION,
            op::REMOTE_NAME_REQUEST,
            op::READ_REMOTE_SUPPORTED_FEATURES,
            op::READ_REMOTE_EXTENDED_FEATURES,
            op::READ_REMOTE_VERSION_INFORMATION,
        ];
        let ev = if status_only.contains(&opcode) {
            let o = opcode.to_le_bytes();
            evt(0x0F, &[0, 1, o[0], o[1]])
        } else {
            let mut ret = vec![match self.refuse {
                Some((o, st)) if o == opcode => st,
                _ => 0,
            }];
            match opcode {
                op::READ_BUFFER_SIZE => ret.extend_from_slice(&[0xFD, 0x03, 0xFF, 9, 0, 4, 0]),
                op::READ_BD_ADDR => ret.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]),
                op::READ_LOCAL_VERSION => ret.extend_from_slice(&[8, 0x0E, 3, 8, 0x60, 0, 0x0E, 3]),
                op::LINK_KEY_REQUEST_REPLY
                | op::LINK_KEY_REQUEST_NEGATIVE_REPLY
                | op::IO_CAPABILITY_REQUEST_REPLY
                | op::IO_CAPABILITY_REQUEST_NEGATIVE_REPLY
                | op::USER_CONFIRMATION_REQUEST_REPLY
                | op::USER_CONFIRMATION_REQUEST_NEGATIVE_REPLY
                | op::PIN_CODE_REQUEST_NEGATIVE_REPLY
                | op::CREATE_CONNECTION_CANCEL => {
                    ret.extend_from_slice(params.get(..6).unwrap_or(&[0; 6]))
                }
                _ => {}
            }
            let o = opcode.to_le_bytes();
            let mut e = vec![1, o[0], o[1]];
            e.extend_from_slice(&ret);
            evt(0x0E, &e)
        };
        self.inner_feed(&ev);
        if opcode == op::REMOTE_NAME_REQUEST
            && params.len() >= 6
            && let Some(n) = self.pad_name.clone()
        {
            let mut e = cat(&[&[0], &params[..6], n.as_bytes()]);
            e.resize(255, 0);
            self.inner_feed(&evt(0x07, &e));
        }
        if opcode == op::DISCONNECT && self.confirm_disconnect && params.len() >= 2 {
            let h = u16::from_le_bytes([params[0], params[1]]);
            self.inner_feed(&ev_disconnected(h, 0x16));
        }
    }

    // -------------------------------------------------- the host's output

    pub fn take_cmd(&mut self, opcode: u16) -> Vec<u8> {
        let i = self
            .cmds
            .iter()
            .position(|(o, _)| *o == opcode)
            .unwrap_or_else(|| {
                panic!(
                    "no command {} sent; queue: {:?}\nlog:\n{}",
                    op::name(opcode),
                    self.cmds
                        .iter()
                        .map(|(o, _)| op::name(*o))
                        .collect::<Vec<_>>(),
                    self.tail_log()
                )
            });
        let (_, p) = self.cmds.remove(i).unwrap();
        self.cmds.drain(..i);
        p
    }

    pub fn sent(&self, opcode: u16) -> bool {
        self.all_cmds.contains(&opcode)
    }

    pub fn take_sig(&mut self, code: u8) -> (u8, Vec<u8>) {
        let i = self
            .sigs
            .iter()
            .position(|s| s.0 == code)
            .unwrap_or_else(|| {
                panic!(
                    "no signal {} sent; queue: {:?}\nlog:\n{}",
                    l2cap::code_name(code),
                    self.sigs,
                    self.tail_log()
                )
            });
        let (_, id, d) = self.sigs.remove(i).unwrap();
        (id, d)
    }

    pub fn take_config_request(&mut self, pad_cid: u16) -> (u8, Vec<u8>) {
        let i = self
            .sigs
            .iter()
            .position(|s| s.0 == code::CONFIGURE_REQUEST && u16le(&s.2, 0) == pad_cid)
            .unwrap_or_else(|| {
                panic!(
                    "no configure request for {pad_cid:#06x}\nlog:\n{}",
                    self.tail_log()
                )
            });
        let (_, id, d) = self.sigs.remove(i).unwrap();
        (id, d)
    }

    pub fn take_data(&mut self, cid: u16) -> Vec<u8> {
        let i = self
            .data
            .iter()
            .position(|d| d.0 == cid)
            .unwrap_or_else(|| panic!("nothing sent to cid {cid:#06x}\nlog:\n{}", self.tail_log()));
        self.data.remove(i).unwrap().1
    }

    /// Everything sent to the pad's channel `cid` so far, taken.
    pub fn drain_data(&mut self, cid: u16) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while let Some(i) = self.data.iter().position(|d| d.0 == cid) {
            out.push(self.data.remove(i).unwrap().1);
        }
        out
    }

    pub fn tail_log(&self) -> String {
        let n = self.radio.logs.len();
        self.radio.logs[n.saturating_sub(60)..].join("\n")
    }

    pub fn log_has(&self, piece: &str) -> bool {
        self.radio.logs.iter().any(|l| l.contains(piece))
    }

    // ------------------------------------------------------- the gamepad

    pub fn pad_frame(&mut self, handle: u16, cid: u16, payload: &[u8]) {
        let f = l2frame(cid, payload);
        // Cut like the controller would (pieces of at most 1021 bytes).
        let mut start = 0;
        let mut first = true;
        while start < f.len() {
            let end = (start + 1021).min(f.len());
            let pb = if first {
                acl::PB_START
            } else {
                acl::PB_CONTINUE
            };
            self.feed(&acl_in(handle, pb, &f[start..end]));
            start = end;
            first = false;
        }
    }

    pub fn pad_sig(&mut self, code: u8, id: u8, data: &[u8]) {
        let mut p = vec![code, id];
        p.extend_from_slice(&(data.len() as u16).to_le_bytes());
        p.extend_from_slice(data);
        self.pad_frame(self.handle, l2cap::CID_SIGNALLING, &p);
    }

    fn next_pad_id(&mut self) -> u8 {
        self.pad_id = self.pad_id.wrapping_add(1);
        self.pad_id
    }

    /// The pad calls. True when the host accepted.
    pub fn pad_calls(&mut self, addr: BdAddr, cod: u32) -> bool {
        self.cmds.clear();
        self.feed(&ev_conn_request(addr, cod));
        if !self
            .cmds
            .iter()
            .any(|c| c.0 == op::ACCEPT_CONNECTION_REQUEST)
        {
            return false;
        }
        self.take_cmd(op::ACCEPT_CONNECTION_REQUEST);
        self.feed(&ev_conn_complete(0, self.handle, addr));
        self.advance(20);
        self.pad_answers_info();
        true
    }

    /// Answers our L2CAP information requests (extended features: none
    /// special), as a pad does.
    pub fn pad_answers_info(&mut self) {
        while let Some(i) = self
            .sigs
            .iter()
            .position(|s| s.0 == code::INFORMATION_REQUEST)
        {
            let (_, id, d) = self.sigs.remove(i).unwrap();
            let kind = u16le(&d, 0);
            let mut r = cat(&[&kind.to_le_bytes(), &[0, 0]]);
            r.extend_from_slice(if kind == 2 { &[0x80, 0, 0, 0] } else { &[0; 8] });
            self.pad_sig(code::INFORMATION_RESPONSE, id, &r);
        }
    }

    /// The pad asks for the key (as the live logs: early, by itself); we
    /// must answer with the saved one; it turns encryption on.
    pub fn pad_uses_saved_key(&mut self, addr: BdAddr) {
        self.feed(&ev_addr(0x17, addr));
        let r = self.take_cmd(op::LINK_KEY_REQUEST_REPLY);
        assert_eq!(&r[..6], &addr.0);
        assert_eq!(&r[6..22], &KEY, "the saved key");
        self.advance(30);
        self.feed(&ev_encrypted(self.handle));
    }

    /// The pad opens a channel itself. Returns our channel id.
    pub fn pad_opens(&mut self, psm: u16, pad_cid: u16) -> u16 {
        let id = self.next_pad_id();
        self.pad_sig(
            code::CONNECTION_REQUEST,
            id,
            &cat(&[&psm.to_le_bytes(), &pad_cid.to_le_bytes()]),
        );
        let (rid, d) = self.take_sig(code::CONNECTION_RESPONSE);
        assert_eq!(rid, id);
        assert_eq!(u16le(&d, 4), conn_result::SUCCESS, "{}", self.tail_log());
        let ours = u16le(&d, 0);
        let (cid_id, _) = self.take_config_request(pad_cid);
        self.pad_sig(
            code::CONFIGURE_RESPONSE,
            cid_id,
            &cat(&[&ours.to_le_bytes(), &[0, 0, 0, 0]]),
        );
        self.pad_configures(ours);
        ours
    }

    pub fn pad_configures(&mut self, ours: u16) {
        let id = self.next_pad_id();
        self.pad_sig(
            code::CONFIGURE_REQUEST,
            id,
            &cat(&[
                &ours.to_le_bytes(),
                &[0, 0, 0x01, 2],
                &672_u16.to_le_bytes(),
            ]),
        );
        let (rid, _) = self.take_sig(code::CONFIGURE_RESPONSE);
        assert_eq!(rid, id);
    }

    /// The pad accepts a channel we asked for.
    pub fn pad_accepts(&mut self, psm: u16, pad_cid: u16) -> u16 {
        let (id, d) = self.take_sig(code::CONNECTION_REQUEST);
        assert_eq!(u16le(&d, 0), psm);
        let ours = u16le(&d, 2);
        self.pad_sig(
            code::CONNECTION_RESPONSE,
            id,
            &cat(&[&pad_cid.to_le_bytes(), &ours.to_le_bytes(), &[0, 0, 0, 0]]),
        );
        let (cid_id, _) = self.take_config_request(pad_cid);
        self.pad_sig(
            code::CONFIGURE_RESPONSE,
            cid_id,
            &cat(&[&ours.to_le_bytes(), &[0, 0, 0, 0]]),
        );
        self.pad_configures(ours);
        ours
    }

    /// The pad answers our SDP query with this logged PDU (its transaction
    /// id set to the one we asked with).
    pub fn pad_answers_sdp(&mut self, sdp_ours: u16, pdu_hex: &str, uuid: u16) {
        let req = self.take_data(PAD_SDP);
        assert_eq!(req[0], sdp::pdu::SERVICE_SEARCH_ATTRIBUTE_REQUEST);
        assert_eq!(&req[5..10], &[0x35, 3, 0x19, (uuid >> 8) as u8, uuid as u8]);
        let mut pdu = hex(pdu_hex);
        pdu[1] = req[1];
        pdu[2] = req[2];
        self.pad_frame(self.handle, sdp_ours, &pdu);
    }

    /// As the live log of a pad's call: after encryption we open SDP, the
    /// pad opens HID control, SDP is answered (HID record), the pad opens
    /// HID interrupt, SDP is answered (Device ID), we close SDP.
    /// Returns (our control channel, our interrupt channel).
    pub fn pad_connects_hid(&mut self, s: &fixtures::Session) -> (u16, u16) {
        let ctrl = self.pad_opens(l2cap::PSM_HID_CONTROL, PAD_CTRL);
        let sdp_ours = self.pad_accepts(l2cap::PSM_SDP, PAD_SDP);
        self.pad_answers_sdp(sdp_ours, s.sdp_hid, 0x1124);
        let intr = self.pad_opens(l2cap::PSM_HID_INTERRUPT, PAD_INTR);
        self.pad_answers_sdp(sdp_ours, s.sdp_pnp, 0x1200);
        let (id, d) = self.take_sig(code::DISCONNECTION_REQUEST);
        self.pad_sig(code::DISCONNECTION_RESPONSE, id, &d);
        (ctrl, intr)
    }

    /// One input report on the interrupt channel.
    pub fn report(&mut self, intr: u16, r: &[u8]) {
        let mut p = vec![0xA1];
        p.extend_from_slice(r);
        self.pad_frame(self.handle, intr, &p);
    }

    /// One message on the control channel (header byte first).
    pub fn control(&mut self, ctrl: u16, m: &[u8]) {
        self.pad_frame(self.handle, ctrl, m);
    }

    /// A whole reconnect of a saved pad, from "waiting" to "HID open".
    pub fn reconnect(&mut self, s: &fixtures::Session) -> (u16, u16) {
        self.pad_name = Some(s.name.to_string());
        assert!(self.pad_calls(PAD, s.cod), "{}", self.tail_log());
        self.pad_uses_saved_key(PAD);
        self.pad_connects_hid(s)
    }

    pub fn take_events(&mut self) -> Vec<PadEvent> {
        std::mem::take(&mut self.events)
    }

    /// No log line holds the key, in any form.
    pub fn assert_key_never_printed(&self) {
        for l in &self.radio.logs {
            assert_no_key(l);
        }
    }

    /// No log line holds a whole address.
    pub fn assert_no_full_address(&self) {
        for l in &self.radio.logs {
            assert_no_address(l);
        }
    }
}

pub fn assert_no_key(text: &str) {
    let hexl: String = KEY.iter().map(|b| format!("{b:02x}")).collect();
    let low = text.to_lowercase();
    assert!(!low.contains(&hexl[..8]), "key bytes in: {text}");
    assert!(!low.contains(&hexl[24..]), "key bytes in: {text}");
    let rev: String = KEY.iter().rev().map(|b| format!("{b:02x}")).collect();
    assert!(!low.contains(&rev[..8]), "key bytes (backwards) in: {text}");
    let dec = format!("{}, {}, {}", KEY[0], KEY[1], KEY[2]);
    assert!(!text.contains(&dec), "key bytes in: {text}");
    let dec_hex = format!("{:#x}, {:#x}", KEY[0], KEY[1]);
    assert!(!low.contains(&dec_hex), "key bytes in: {text}");
}

pub fn assert_no_address(text: &str) {
    let low = text.to_lowercase().replace(':', "");
    for a in [PAD, STRANGER] {
        // The first four bytes as printed (high bytes first) and as sent
        // (low bytes first).
        let fwd: String =
            a.0.iter()
                .rev()
                .take(4)
                .map(|b| format!("{b:02x}"))
                .collect();
        let back: String = a.0.iter().take(4).map(|b| format!("{b:02x}")).collect();
        assert!(!low.contains(&fwd), "address in: {text}");
        assert!(!low.contains(&back), "address in: {text}");
    }
}

// ------------------------------------------------------------ byte makers

pub fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

pub fn evt(code: u8, params: &[u8]) -> Vec<u8> {
    let mut p = vec![0x04, code, params.len() as u8];
    p.extend_from_slice(params);
    p
}

pub fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

pub fn u16le(d: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([d[at], d[at + 1]])
}

pub fn cod3(cod: u32) -> [u8; 3] {
    let b = cod.to_le_bytes();
    [b[0], b[1], b[2]]
}

/// An ACL packet from the controller (any length up to 1021 data bytes,
/// as the ESP32 controller gives them).
pub fn acl_in(handle: u16, pb: u8, data: &[u8]) -> Vec<u8> {
    let hw = (handle & 0x0FFF) | u16::from(pb & 3) << 12;
    cat(&[
        &[0x02],
        &hw.to_le_bytes(),
        &(data.len() as u16).to_le_bytes(),
        data,
    ])
}

pub fn l2frame(cid: u16, payload: &[u8]) -> Vec<u8> {
    cat(&[
        &(payload.len() as u16).to_le_bytes(),
        &cid.to_le_bytes(),
        payload,
    ])
}

pub fn ev_conn_complete(status: u8, handle: u16, addr: BdAddr) -> Vec<u8> {
    evt(
        0x03,
        &cat(&[&[status], &handle.to_le_bytes(), &addr.0, &[0x01, 0x00]]),
    )
}

pub fn ev_conn_request(addr: BdAddr, cod: u32) -> Vec<u8> {
    evt(0x04, &cat(&[&addr.0, &cod3(cod), &[0x01]]))
}

/// Role Change: `role` 0 = we are central now, 1 = peripheral.
pub fn ev_role(status: u8, addr: BdAddr, role: u8) -> Vec<u8> {
    evt(0x12, &cat(&[&[status], &addr.0, &[role]]))
}

/// Mode Change: `mode` 0 active, 2 sniff; `interval` in slots.
pub fn ev_mode(handle: u16, mode: u8, interval: u16) -> Vec<u8> {
    evt(
        0x14,
        &cat(&[
            &[0],
            &handle.to_le_bytes(),
            &[mode],
            &interval.to_le_bytes(),
        ]),
    )
}

/// The supervision timeouts written so far: (handle, slots).
pub fn lsto_written(s: &Sim) -> Vec<(u16, u16)> {
    s.history
        .iter()
        .filter(|(o, _)| *o == op::WRITE_LINK_SUPERVISION_TIMEOUT)
        .map(|(_, p)| (u16le(p, 0), u16le(p, 2)))
        .collect()
}

pub fn ev_disconnected(handle: u16, reason: u8) -> Vec<u8> {
    evt(0x05, &cat(&[&[0], &handle.to_le_bytes(), &[reason]]))
}

pub fn ev_auth(status: u8, handle: u16) -> Vec<u8> {
    evt(0x06, &cat(&[&[status], &handle.to_le_bytes()]))
}

pub fn ev_encrypted(handle: u16) -> Vec<u8> {
    evt(0x08, &cat(&[&[0], &handle.to_le_bytes(), &[1]]))
}

pub fn ev_addr(code: u8, addr: BdAddr) -> Vec<u8> {
    evt(code, &addr.0)
}

pub fn ev_link_key(addr: BdAddr, key: [u8; 16]) -> Vec<u8> {
    evt(0x18, &cat(&[&addr.0, &key, &[0x04]]))
}

// ------------------------------------- what a pairing window hears (bytes)

/// Link Key Notification with any key type.
pub fn ev_link_key_typed(addr: BdAddr, key: [u8; 16], key_type: u8) -> Vec<u8> {
    evt(0x18, &cat(&[&addr.0, &key, &[key_type]]))
}

/// Extended Inquiry Result: one device heard in a search, with its
/// class, a clock offset of 0x1234, -60 dBm and its name in the EIR.
pub fn ev_ext_inquiry(addr: BdAddr, cod: u32, name: &[u8]) -> Vec<u8> {
    let mut e = vec![1];
    e.extend_from_slice(&addr.0);
    e.extend_from_slice(&[0x01, 0x00]);
    e.extend_from_slice(&cod3(cod));
    e.extend_from_slice(&0x1234_u16.to_le_bytes());
    e.push(0xC4);
    let mut eir = vec![(name.len() + 1) as u8, 0x09];
    eir.extend_from_slice(name);
    eir.resize(240, 0);
    e.extend_from_slice(&eir);
    evt(0x2F, &e)
}

/// IO Capability Response: the pad has no input and no output.
pub fn ev_io_cap_response(addr: BdAddr) -> Vec<u8> {
    evt(0x32, &cat(&[&addr.0, &[0x03, 0x00, 0x00]]))
}

/// User Confirmation Request (the number is never shown: just works).
pub fn ev_user_confirmation(addr: BdAddr) -> Vec<u8> {
    evt(0x33, &cat(&[&addr.0, &123_456_u32.to_le_bytes()]))
}

/// Simple Pairing Complete, status 0.
pub fn ev_ssp_complete(addr: BdAddr) -> Vec<u8> {
    evt(0x36, &cat(&[&[0], &addr.0]))
}

// ------------------------------------------------- a Switch-mode gamepad

/// A Switch-mode pad that plays its part of the start-up talk from the
/// logged bytes: 0x3F reports at first; each output report is answered
/// with the logged 0x21 answer to the same subcommand; after "full report
/// mode" (03 30) the logged 0x30 reports without motion, after "motion
/// on" (40 01) the logged 0x30 reports with motion.
pub struct SwitchPad {
    simple: Vec<Vec<u8>>,
    plain: Vec<Vec<u8>>,
    moving: Vec<Vec<u8>>,
    answers: Vec<Vec<u8>>,
    pub full: bool,
    pub motion: bool,
    pub n: usize,
    /// Every output report the host sent on the interrupt channel (without
    /// the 0xA2 header).
    pub outputs: Vec<Vec<u8>>,
    /// Bytes 3, 4, 5 (buttons) of the reports from now on.
    pub buttons: [u8; 3],
    /// Answer outputs (false: a pad that ignores them).
    pub answers_outputs: bool,
}

impl SwitchPad {
    pub fn new(s: &fixtures::Session) -> Self {
        let all: Vec<Vec<u8>> = s.reports.iter().map(|r| hex(r)).collect();
        let pick = |f: &dyn Fn(&Vec<u8>) -> bool| -> Vec<Vec<u8>> {
            all.iter().filter(|r| f(r)).cloned().collect()
        };
        let still = |r: &Vec<u8>| r.len() >= 49 && r[13..49].iter().all(|b| *b == 0);
        Self {
            simple: pick(&|r| r[0] == 0x3F),
            plain: pick(&|r| r[0] == 0x30 && still(r)),
            moving: pick(&|r| r[0] == 0x30 && !still(r)),
            answers: pick(&|r| r[0] == 0x21 && r.len() >= 15),
            full: false,
            motion: false,
            n: 0,
            outputs: Vec::new(),
            buttons: [0; 3],
            answers_outputs: true,
        }
    }

    /// The logged answer to subcommand `sub`.
    pub fn answer(&self, sub: u8) -> Option<Vec<u8>> {
        self.answers.iter().find(|r| r[14] == sub).cloned()
    }

    /// The report the pad sends next.
    pub fn next_report(&mut self) -> Vec<u8> {
        self.n += 1;
        let list = if self.motion && !self.moving.is_empty() {
            &self.moving
        } else if self.full && !self.plain.is_empty() {
            &self.plain
        } else {
            &self.simple
        };
        let mut r = list[self.n % list.len()].clone();
        if r[0] == 0x30 {
            r[3] |= self.buttons[0];
            r[4] |= self.buttons[1];
            r[5] |= self.buttons[2];
        }
        r
    }
}

impl Sim {
    /// The pad answers what the host sent on the interrupt channel.
    pub fn switch_answers(&mut self, pad: &mut SwitchPad, intr: u16) {
        for p in self.drain_data(PAD_INTR) {
            assert_eq!(p[0], 0xA2, "an output report");
            let out = p[1..].to_vec();
            pad.outputs.push(out.clone());
            if !pad.answers_outputs || out.len() < 12 || out[0] != 0x01 {
                continue;
            }
            let (sub, arg) = (out[10], out[11]);
            if let Some(a) = pad.answer(sub) {
                self.report(intr, &a);
            }
            match (sub, arg) {
                (0x03, 0x30) => pad.full = true,
                (0x40, 0x01) => pad.motion = true,
                _ => {}
            }
        }
    }

    /// `ms` of a Switch pad sending a report every 10 ms (about 100 a
    /// second, as logged) and answering the host's outputs.
    pub fn switch_runs(&mut self, pad: &mut SwitchPad, intr: u16, ms: u64) {
        let end = self.now + ms;
        while self.now < end {
            let r = pad.next_report();
            self.report(intr, &r);
            self.advance(10);
            self.switch_answers(pad, intr);
        }
    }
}

/// Nothing the round forbids went out: no search, no call of our own, no
/// pairing step, never visible (scan enable at most "page scan").
pub fn assert_nothing_forbidden(s: &Sim) {
    for o in [
        op::INQUIRY,
        op::CREATE_CONNECTION,
        op::IO_CAPABILITY_REQUEST_REPLY,
        op::USER_CONFIRMATION_REQUEST_REPLY,
        op::PIN_CODE_REQUEST_REPLY,
    ] {
        assert!(!s.sent(o), "{} sent", op::name(o));
    }
    for (o, p) in &s.history {
        if *o == op::WRITE_SCAN_ENABLE {
            assert!(p[0] & 0x01 == 0, "inquiry scan turned on: {p:?}");
        }
        if *o == op::ACCEPT_CONNECTION_REQUEST {
            assert_eq!(&p[..6], &PAD.0, "only the saved pad is accepted");
        }
        if *o == op::LINK_KEY_REQUEST_REPLY {
            assert_eq!(&p[..6], &PAD.0, "the key goes only to its own pad");
        }
    }
}
