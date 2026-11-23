//! Whole conversations replayed against the state machine, on the PC.
//!
//! The test plays two parts at once: the radio controller (it answers
//! every command, and gives buffers back after each data packet) and the
//! gamepad (its events and L2CAP messages are written out by hand from
//! the Bluetooth Core and HID specifications). Time only moves when the
//! test says so.
//!
//! All addresses and keys here are made up.

use std::collections::VecDeque;
use std::fmt;

use pad_proto::acl;
use pad_proto::hci::LinkKey;
use pad_proto::hci::{BdAddr, op};
use pad_proto::l2cap::{self, code, conf_result, conn_result, conn_status};
use pad_proto::sdp::{self, Builder};
use pad_proto::{Config, ForgetReason, Host, Io, KeyChange, Purpose, Tag, UiEvent};

const PAD: BdAddr = BdAddr([0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
const PHONE: BdAddr = BdAddr([0x21, 0x22, 0x23, 0x24, 0x25, 0x26]);
const FAKE: BdAddr = BdAddr([0x00, 0x00, 0x00, 0xAD, 0xDE, 0x02]);
/// Peripheral (0x05), minor 0x08 = gamepad; "limited discoverable" bit.
const PAD_COD: u32 = 0x002508;
/// Phone (major 0x02), smartphone.
const PHONE_COD: u32 = 0x5A020C;
const H: u16 = 0x000B;
const KEY: [u8; 16] = [
    0xC0, 0xFF, 0xEE, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD,
];
/// A second made-up key: an older pairing the gamepad may have dropped.
const OLD_KEY: [u8; 16] = [
    0x0D, 0x1E, 0x2F, 0x3A, 0x4B, 0x5C, 0x6D, 0x7E, 0x8F, 0x9A, 0xAB, 0xBC, 0xCD, 0xDE, 0xEF, 0xFA,
];

// ---------------------------------------------------------------- recorder

#[derive(Clone, Debug, PartialEq)]
enum Ui {
    Status(String),
    Device {
        name: String,
        cod: u32,
        vid: Option<u16>,
        pid: Option<u16>,
    },
    Descriptor(Vec<u8>),
    Report(Vec<u8>),
    Control(Vec<u8>),
    Disconnected,
}

#[derive(Default)]
struct Rec {
    out: Vec<Vec<u8>>,
    ui: Vec<Ui>,
    logs: Vec<String>,
    /// What the host told the caller to save or erase.
    changes: Vec<KeyChange>,
}

impl Io for Rec {
    fn send(&mut self, p: &[u8]) {
        assert!(
            p.len() <= 259,
            "packet longer than the radio driver takes: {}",
            p.len()
        );
        self.out.push(p.to_vec());
    }
    fn ui(&mut self, ev: UiEvent<'_>) {
        self.ui.push(match ev {
            UiEvent::Status(s) => Ui::Status(s.to_string()),
            UiEvent::Device(d) => Ui::Device {
                name: d.name.to_string(),
                cod: d.class_of_device,
                vid: d.vendor_id,
                pid: d.product_id,
            },
            UiEvent::Descriptor(d) => Ui::Descriptor(d.to_vec()),
            UiEvent::Report(r) => Ui::Report(r.to_vec()),
            UiEvent::Control(m) => Ui::Control(m.to_vec()),
            UiEvent::Disconnected => Ui::Disconnected,
        });
    }
    fn log(&mut self, tag: Tag, args: fmt::Arguments<'_>) {
        let line = format!("{} {}", tag.name(), args);
        if std::env::var_os("PAD_TRACE").is_some() {
            println!("{line}");
        }
        self.logs.push(line);
    }
    fn key_change(&mut self, change: KeyChange) {
        self.changes.push(change);
    }
}

// --------------------------------------------------------------- simulator

/// One signalling command we sent: (code, id, data).
type Sig = (u8, u8, Vec<u8>);

struct Sim {
    host: Host,
    io: Rec,
    now: u64,
    seen: usize,
    cmds: VecDeque<(u16, Vec<u8>)>,
    all_cmds: Vec<u16>,
    sigs: VecDeque<Sig>,
    /// Frames we sent on channels other than signalling: (their cid, payload).
    data: VecDeque<(u16, Vec<u8>)>,
    reasm: acl::Reassembler<2048>,
    pad_id: u8,
}

impl Sim {
    fn new() -> Self {
        Self::with_config(Config::default())
    }

    fn with_config(cfg: Config) -> Self {
        Self::with_saved(cfg, &[])
    }

    /// A host that starts with pairings saved earlier (oldest first), as
    /// after a restart of the board.
    fn with_saved(cfg: Config, saved: &[(BdAddr, [u8; 16])]) -> Self {
        let mut host = Host::new(cfg);
        for (addr, key) in saved {
            host.load_key(*addr, LinkKey(*key), 0x04);
        }
        let mut s = Self {
            host,
            io: Rec::default(),
            now: 1_000,
            seen: 0,
            cmds: VecDeque::new(),
            all_cmds: Vec::new(),
            sigs: VecDeque::new(),
            data: VecDeque::new(),
            reasm: acl::Reassembler::new(),
            pad_id: 0x80,
        };
        s.host.start(s.now, &mut s.io);
        s.settle();
        assert!(
            s.io.ui.contains(&Ui::Status("Radio ready".into())),
            "start-up did not finish"
        );
        s.cmds.clear();
        s
    }

    fn feed(&mut self, p: &[u8]) {
        self.host.on_packet(self.now, p, &mut self.io);
        self.settle();
    }

    /// Moves time on in steps of 20 ms, like the firmware's loop.
    fn advance(&mut self, ms: u64) {
        let end = self.now + ms;
        while self.now < end {
            self.now = (self.now + 20).min(end);
            self.host.tick(self.now, &mut self.io);
            self.settle();
        }
    }

    /// Plays the controller: answers commands, returns data buffers.
    fn settle(&mut self) {
        loop {
            if self.seen == self.io.out.len() {
                return;
            }
            let p = self.io.out[self.seen].clone();
            self.seen += 1;
            match p[0] {
                0x01 => {
                    let opcode = u16::from_le_bytes([p[1], p[2]]);
                    assert_eq!(usize::from(p[3]), p.len() - 4, "command length byte");
                    let params = p[4..].to_vec();
                    self.cmds.push_back((opcode, params.clone()));
                    self.all_cmds.push(opcode);
                    self.controller_answer(opcode, &params);
                }
                0x02 => {
                    let (h, d) = acl::parse(&p[1..]).expect("acl header");
                    assert!(d.len() <= acl::MAX_FRAGMENT);
                    if let Some(f) = self.reasm.push(h.pb, d).expect("our frames join up") {
                        let cid = u16::from_le_bytes([f[2], f[3]]);
                        let payload = f[4..].to_vec();
                        assert_eq!(usize::from(u16::from_le_bytes([f[0], f[1]])), payload.len());
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
                    // Number Of Completed Packets: one buffer back.
                    let mut e = vec![1];
                    e.extend_from_slice(&h.handle.to_le_bytes());
                    e.extend_from_slice(&1_u16.to_le_bytes());
                    self.host.on_packet(self.now, &evt(0x13, &e), &mut self.io);
                }
                k => panic!("packet of kind {k:#04x} sent to the controller"),
            }
        }
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
            let mut ret = vec![0];
            match opcode {
                op::READ_BUFFER_SIZE => ret.extend_from_slice(&[0xFD, 0x03, 0, 9, 0, 0, 0]),
                op::READ_BD_ADDR => ret.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]),
                op::READ_LOCAL_VERSION => ret.extend_from_slice(&[8, 0, 0, 8, 0x60, 0, 0, 0]),
                op::LINK_KEY_REQUEST_REPLY
                | op::LINK_KEY_REQUEST_NEGATIVE_REPLY
                | op::IO_CAPABILITY_REQUEST_REPLY
                | op::IO_CAPABILITY_REQUEST_NEGATIVE_REPLY
                | op::USER_CONFIRMATION_REQUEST_REPLY
                | op::USER_CONFIRMATION_REQUEST_NEGATIVE_REPLY
                | op::PIN_CODE_REQUEST_NEGATIVE_REPLY
                | op::CREATE_CONNECTION_CANCEL => ret.extend_from_slice(&params[..6]),
                _ => {}
            }
            let o = opcode.to_le_bytes();
            let mut e = vec![1, o[0], o[1]];
            e.extend_from_slice(&ret);
            evt(0x0E, &e)
        };
        self.host.on_packet(self.now, &ev, &mut self.io);
    }

    /// The next command of this kind (earlier other commands are skipped).
    fn take_cmd(&mut self, opcode: u16) -> Vec<u8> {
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

    fn sent(&self, opcode: u16) -> bool {
        self.all_cmds.contains(&opcode)
    }

    fn take_sig(&mut self, code: u8) -> (u8, Vec<u8>) {
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

    /// Our next configuration request for the gamepad's channel `pad_cid`.
    fn take_config_request(&mut self, pad_cid: u16) -> (u8, Vec<u8>) {
        let i = self
            .sigs
            .iter()
            .position(|s| s.0 == code::CONFIGURE_REQUEST && u16le(&s.2, 0) == pad_cid)
            .unwrap_or_else(|| {
                panic!(
                    "no configure request for {pad_cid:#06x}; queue: {:?}\nlog:\n{}",
                    self.sigs,
                    self.tail_log()
                )
            });
        let (_, id, d) = self.sigs.remove(i).unwrap();
        (id, d)
    }

    fn take_data(&mut self, cid: u16) -> Vec<u8> {
        let i = self
            .data
            .iter()
            .position(|d| d.0 == cid)
            .unwrap_or_else(|| panic!("nothing sent to cid {cid:#06x}\nlog:\n{}", self.tail_log()));
        self.data.remove(i).unwrap().1
    }

    fn tail_log(&self) -> String {
        let n = self.io.logs.len();
        self.io.logs[n.saturating_sub(40)..].join("\n")
    }

    /// One L2CAP frame from the gamepad, cut into ACL pieces at `cuts`.
    fn pad_frame(&mut self, handle: u16, cid: u16, payload: &[u8], cuts: &[usize]) {
        let f = l2frame(cid, payload);
        let mut start = 0;
        let mut ends: Vec<usize> = cuts
            .iter()
            .copied()
            .filter(|&c| c > 0 && c < f.len())
            .collect();
        ends.push(f.len());
        for (k, end) in ends.into_iter().enumerate() {
            let pb = if k == 0 {
                acl::PB_START
            } else {
                acl::PB_CONTINUE
            };
            self.feed(acl::packet(handle, pb, &f[start..end]).as_bytes());
            start = end;
        }
    }

    fn pad_sig(&mut self, code: u8, id: u8, data: &[u8]) {
        let mut p = vec![code, id];
        p.extend_from_slice(&(data.len() as u16).to_le_bytes());
        p.extend_from_slice(data);
        self.pad_frame(H, l2cap::CID_SIGNALLING, &p, &[]);
    }

    fn next_pad_id(&mut self) -> u8 {
        self.pad_id = self.pad_id.wrapping_add(1);
        self.pad_id
    }

    fn statuses(&self) -> Vec<String> {
        self.io
            .ui
            .iter()
            .filter_map(|u| {
                if let Ui::Status(s) = u {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    fn reports(&self) -> Vec<Vec<u8>> {
        self.io
            .ui
            .iter()
            .filter_map(|u| {
                if let Ui::Report(r) = u {
                    Some(r.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    fn descriptor(&self) -> Option<Vec<u8>> {
        self.io.ui.iter().rev().find_map(|u| {
            if let Ui::Descriptor(d) = u {
                Some(d.clone())
            } else {
                None
            }
        })
    }

    /// The gamepad accepts a channel we asked for and configures it
    /// plainly. Returns our local channel id.
    fn pad_accepts(&mut self, psm: u16, pad_cid: u16) -> u16 {
        let (id, d) = self.take_sig(code::CONNECTION_REQUEST);
        assert_eq!(u16le(&d, 0), psm, "connection request for the wrong psm");
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

    /// The gamepad sends its configuration request (MTU 672) for our
    /// channel `ours`; we must accept it.
    fn pad_configures(&mut self, ours: u16) {
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
        let (rid, d) = self.take_sig(code::CONFIGURE_RESPONSE);
        assert_eq!(rid, id);
        assert_eq!(
            u16le(&d, 4),
            conf_result::SUCCESS,
            "our answer to a plain configuration"
        );
    }

    /// The gamepad opens a channel itself. Returns our local channel id
    /// and the result we gave (it may be "pending").
    fn pad_opens(&mut self, psm: u16, pad_cid: u16) -> (u16, u16, u16) {
        let id = self.next_pad_id();
        self.pad_sig(
            code::CONNECTION_REQUEST,
            id,
            &cat(&[&psm.to_le_bytes(), &pad_cid.to_le_bytes()]),
        );
        let (rid, d) = self.take_sig(code::CONNECTION_RESPONSE);
        assert_eq!(rid, id);
        assert_eq!(u16le(&d, 2), pad_cid);
        (u16le(&d, 0), u16le(&d, 4), u16le(&d, 6))
    }

    /// Finishes configuring a channel the gamepad opened, after our
    /// success answer: our configuration request, its configuration request.
    fn pad_finishes_config(&mut self, ours: u16, pad_cid: u16) {
        let (cid_id, _) = self.take_config_request(pad_cid);
        self.pad_sig(
            code::CONFIGURE_RESPONSE,
            cid_id,
            &cat(&[&ours.to_le_bytes(), &[0, 0, 0, 0]]),
        );
        self.pad_configures(ours);
    }

    /// The gamepad answers our SDP queries (HID, then Device ID) in one
    /// piece each, then accepts the close of the SDP channel.
    fn pad_serves_sdp(&mut self, ours: u16, pad_cid: u16, desc: &[u8]) {
        let req = self.take_data(pad_cid);
        assert_eq!(req[0], sdp::pdu::SERVICE_SEARCH_ATTRIBUTE_REQUEST);
        assert_eq!(
            &req[5..10],
            &[0x35, 3, 0x19, 0x11, 0x24],
            "first query is for HID"
        );
        let lists = hid_lists(desc);
        self.pad_frame(H, ours, &sdp_rsp(be16(&req, 1), &lists, &[]), &[]);
        let req = self.take_data(pad_cid);
        assert_eq!(
            &req[5..10],
            &[0x35, 3, 0x19, 0x12, 0x00],
            "second query is for Device ID"
        );
        self.pad_frame(H, ours, &sdp_rsp(be16(&req, 1), &pnp_lists(), &[]), &[]);
        self.pad_closes_ok(ours, pad_cid);
    }

    /// We close a channel; the gamepad agrees.
    fn pad_closes_ok(&mut self, ours: u16, pad_cid: u16) {
        let (id, d) = self.take_sig(code::DISCONNECTION_REQUEST);
        assert_eq!((u16le(&d, 0), u16le(&d, 2)), (pad_cid, ours));
        self.pad_sig(code::DISCONNECTION_RESPONSE, id, &d);
    }

    fn assert_key_never_printed(&self) {
        for key in [KEY, OLD_KEY] {
            let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
            let hex_up = hex.to_uppercase();
            let first4 = &hex[..8];
            let dec = format!("{}, {}, {}", key[0], key[1], key[2]);
            for l in &self.io.logs {
                assert!(
                    !l.contains(&hex) && !l.contains(&hex_up) && !l.contains(first4),
                    "link key in the log: {l}"
                );
                assert!(!l.contains(&dec), "link key in the log: {l}");
            }
        }
    }
}

// ------------------------------------------------------------- byte makers

fn evt(code: u8, params: &[u8]) -> Vec<u8> {
    let mut p = vec![0x04, code, params.len() as u8];
    p.extend_from_slice(params);
    p
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn u16le(d: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([d[at], d[at + 1]])
}

fn be16(d: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([d[at], d[at + 1]])
}

fn cod3(cod: u32) -> [u8; 3] {
    let b = cod.to_le_bytes();
    [b[0], b[1], b[2]]
}

fn l2frame(cid: u16, payload: &[u8]) -> Vec<u8> {
    cat(&[
        &(payload.len() as u16).to_le_bytes(),
        &cid.to_le_bytes(),
        payload,
    ])
}

fn ev_ext_inquiry(addr: BdAddr, cod: u32, name: &[u8]) -> Vec<u8> {
    let mut e = vec![1];
    e.extend_from_slice(&addr.0);
    e.extend_from_slice(&[0x01, 0x00]);
    e.extend_from_slice(&cod3(cod));
    e.extend_from_slice(&0x1234_u16.to_le_bytes());
    e.push(0xC4); // -60 dBm
    let mut eir = vec![(name.len() + 1) as u8, 0x09];
    eir.extend_from_slice(name);
    eir.resize(240, 0);
    e.extend_from_slice(&eir);
    evt(0x2F, &e)
}

fn ev_conn_complete(status: u8, handle: u16, addr: BdAddr) -> Vec<u8> {
    evt(
        0x03,
        &cat(&[&[status], &handle.to_le_bytes(), &addr.0, &[0x01, 0x00]]),
    )
}

fn ev_conn_request(addr: BdAddr, cod: u32) -> Vec<u8> {
    evt(0x04, &cat(&[&addr.0, &cod3(cod), &[0x01]]))
}

fn ev_disconnected(handle: u16, reason: u8) -> Vec<u8> {
    evt(0x05, &cat(&[&[0], &handle.to_le_bytes(), &[reason]]))
}

fn ev_auth(status: u8, handle: u16) -> Vec<u8> {
    evt(0x06, &cat(&[&[status], &handle.to_le_bytes()]))
}

fn ev_encrypted(handle: u16) -> Vec<u8> {
    evt(0x08, &cat(&[&[0], &handle.to_le_bytes(), &[1]]))
}

fn ev_addr(code: u8, addr: BdAddr) -> Vec<u8> {
    evt(code, &addr.0)
}

fn ev_io_cap_response(addr: BdAddr) -> Vec<u8> {
    evt(0x32, &cat(&[&addr.0, &[0x03, 0x00, 0x00]]))
}

fn ev_user_confirmation(addr: BdAddr) -> Vec<u8> {
    evt(0x33, &cat(&[&addr.0, &123_456_u32.to_le_bytes()]))
}

fn ev_ssp_complete(addr: BdAddr) -> Vec<u8> {
    evt(0x36, &cat(&[&[0], &addr.0]))
}

fn ev_link_key(addr: BdAddr) -> Vec<u8> {
    evt(0x18, &cat(&[&addr.0, &KEY, &[0x04]]))
}

/// A made-up gamepad report descriptor of `len` bytes (a real header,
/// then filler).
fn descriptor(len: usize) -> Vec<u8> {
    let mut d = vec![0x05, 0x01, 0x09, 0x05, 0xA1, 0x01, 0x85, 0x30];
    while d.len() < len - 1 {
        d.push((d.len() % 251) as u8);
    }
    d.push(0xC0);
    d
}

/// The attribute lists of a HID record holding `desc`.
fn hid_lists(desc: &[u8]) -> Vec<u8> {
    let mut b: Builder<2048> = Builder::new();
    let outer = b.seq();
    let rec = b.seq();
    b.u16(0x0000).raw(&[0x0A, 0x00, 0x01, 0x00, 0x00]);
    b.u16(0x0001);
    let cls = b.seq();
    b.uuid16(0x1124);
    b.end(cls);
    b.u16(0x0100).text(b"Wireless Gamepad");
    b.u16(0x0201).u16(0x0111);
    b.u16(0x0202).u8(0x08);
    b.u16(0x0203).u8(0x21);
    b.u16(0x0204).boolean(true);
    b.u16(0x0205).boolean(true);
    b.u16(0x0206);
    let list = b.seq();
    let one = b.seq();
    b.u8(0x22).text(desc);
    b.end(one);
    b.end(list);
    b.u16(0x020D).boolean(false);
    b.u16(0x020E).boolean(false);
    b.end(rec);
    b.end(outer);
    b.bytes().to_vec()
}

fn pnp_lists() -> Vec<u8> {
    let mut b: Builder<256> = Builder::new();
    let outer = b.seq();
    let rec = b.seq();
    b.u16(0x0001);
    let cls = b.seq();
    b.uuid16(0x1200);
    b.end(cls);
    b.u16(0x0200).u16(0x0103);
    b.u16(0x0201).u16(0x057E);
    b.u16(0x0202).u16(0x2009);
    b.u16(0x0203).u16(0x0001);
    b.u16(0x0204).boolean(true);
    b.u16(0x0205).u16(0x0002);
    b.end(rec);
    b.end(outer);
    b.bytes().to_vec()
}

/// A Service Search Attribute Response carrying `piece` of the lists.
fn sdp_rsp(tid: u16, piece: &[u8], cont: &[u8]) -> Vec<u8> {
    let plen = 2 + piece.len() + 1 + cont.len();
    cat(&[
        &[sdp::pdu::SERVICE_SEARCH_ATTRIBUTE_RESPONSE],
        &tid.to_be_bytes(),
        &(plen as u16).to_be_bytes(),
        &(piece.len() as u16).to_be_bytes(),
        piece,
        &[cont.len() as u8],
        cont,
    ])
}

// ------------------------------------------------------------ shared steps

/// Search until the gamepad answers; we must call it.
fn find_and_call_pad(s: &mut Sim) {
    s.advance(40);
    let inq = s.take_cmd(op::INQUIRY);
    assert_eq!(
        &inq[..3],
        &[0x33, 0x8B, 0x9E],
        "general inquiry access code"
    );
    // A phone answers first: logged, never called.
    s.feed(&ev_ext_inquiry(PHONE, PHONE_COD, b"Someone's phone"));
    assert!(!s.sent(op::INQUIRY_CANCEL));
    assert!(
        !s.io.logs.iter().any(|l| l.contains("Someone")),
        "a stranger's name was logged"
    );
    s.feed(&ev_ext_inquiry(PAD, PAD_COD, b"Pro Controller"));
    s.take_cmd(op::INQUIRY_CANCEL);
    let cc = s.take_cmd(op::CREATE_CONNECTION);
    assert_eq!(&cc[..6], &PAD.0);
    assert_eq!(cc[8], 0x01, "page scan repetition mode from the answer");
    assert_eq!(
        u16le(&cc, 10),
        0x1234 | 0x8000,
        "clock offset from the answer, marked valid"
    );
    assert!(
        !s.io
            .out
            .iter()
            .any(|p| p[0] == 1 && p[4..].starts_with(&PHONE.0)),
        "a command named the phone"
    );
}

/// Outgoing link, first time: Secure Simple Pairing "just works".
fn pair_outgoing(s: &mut Sim) {
    find_and_call_pad(s);
    s.feed(&ev_conn_complete(0, H, PAD));
    s.take_cmd(op::READ_REMOTE_SUPPORTED_FEATURES);
    s.take_cmd(op::READ_REMOTE_VERSION_INFORMATION);
    assert_eq!(s.take_cmd(op::AUTHENTICATION_REQUESTED), H.to_le_bytes());
    let (info_id, d) = s.take_sig(code::INFORMATION_REQUEST);
    assert_eq!(d, [2, 0]);
    s.pad_sig(
        code::INFORMATION_RESPONSE,
        info_id,
        &[2, 0, 0, 0, 0x80, 0, 0, 0],
    );
    assert!(
        !s.sent(op::REMOTE_NAME_REQUEST),
        "the name is known from the search"
    );

    s.feed(&ev_addr(0x17, PAD));
    assert_eq!(s.take_cmd(op::LINK_KEY_REQUEST_NEGATIVE_REPLY), PAD.0);
    s.feed(&ev_addr(0x31, PAD));
    let r = s.take_cmd(op::IO_CAPABILITY_REQUEST_REPLY);
    assert_eq!(&r[..6], &PAD.0);
    assert_eq!(
        &r[6..],
        &[0x03, 0x00, 0x04],
        "NoInputNoOutput, no OOB data, general bonding without MITM"
    );
    s.feed(&ev_io_cap_response(PAD));
    s.feed(&ev_user_confirmation(PAD));
    assert_eq!(s.take_cmd(op::USER_CONFIRMATION_REQUEST_REPLY), PAD.0);
    s.feed(&ev_ssp_complete(PAD));
    s.feed(&ev_link_key(PAD));
    s.feed(&ev_auth(0, H));
    assert_eq!(
        s.take_cmd(op::SET_CONNECTION_ENCRYPTION),
        [0x0B, 0x00, 0x01]
    );
    s.feed(&ev_encrypted(H));
    s.assert_key_never_printed();
}

/// After encryption on an outgoing link: SDP, then both HID channels.
/// Returns our interrupt channel id.
fn sdp_and_hid_outgoing(s: &mut Sim, desc: &[u8]) -> (u16, u16) {
    let sdp_ours = s.pad_accepts(l2cap::PSM_SDP, 0x0070);
    s.pad_serves_sdp(sdp_ours, 0x0070, desc);
    s.advance(40);
    let ctrl = s.pad_accepts(l2cap::PSM_HID_CONTROL, 0x0071);
    s.advance(40);
    let intr = s.pad_accepts(l2cap::PSM_HID_INTERRUPT, 0x0072);
    assert_eq!(s.statuses().last().map(String::as_str), Some("Connected"));
    (ctrl, intr)
}

// ------------------------------------------------------------------ tests

/// Scenario 1: we find the gamepad, call it, pair, read its description
/// and open HID; reports reach the screen; an output report goes out.
#[test]
fn outgoing_connection_with_pairing() {
    let mut s = Sim::new();
    pair_outgoing(&mut s);
    let desc = descriptor(120);
    let (_ctrl, intr) = sdp_and_hid_outgoing(&mut s, &desc);

    assert_eq!(s.descriptor().as_deref(), Some(&desc[..]));
    let dev = s.io.ui.iter().rev().find_map(|u| {
        if let Ui::Device {
            name,
            cod,
            vid,
            pid,
        } = u
        {
            Some((name.clone(), *cod, *vid, *pid))
        } else {
            None
        }
    });
    assert_eq!(
        dev,
        Some(("Pro Controller".into(), PAD_COD, Some(0x057E), Some(0x2009)))
    );

    // Input reports on the interrupt channel reach the screen without 0xA1.
    for k in 0..25_u8 {
        s.pad_frame(H, intr, &[0xA1, 0x30, k, 0x80, 0x00, 0x08], &[]);
        s.advance(16);
    }
    let r = s.reports();
    assert_eq!(r.len(), 25);
    assert_eq!(r[3], vec![0x30, 3, 0x80, 0x00, 0x08]);
    // The first 20 are logged in full, then about one a second.
    let full =
        s.io.logs
            .iter()
            .filter(|l| l.starts_with("PAD-HID report n="))
            .count();
    assert!((20..=22).contains(&full), "{full} reports logged");

    // An output report goes out on the interrupt channel with 0xA2.
    assert!(
        s.host
            .send_output_report(s.now, &[0x01, 0x00, 0x10], &mut s.io)
    );
    s.settle();
    assert_eq!(s.take_data(0x0072), vec![0xA2, 0x01, 0x00, 0x10]);

    // The pad switches off: back to searching.
    s.feed(&ev_disconnected(H, 0x13));
    assert!(s.io.ui.contains(&Ui::Disconnected));
    s.cmds.clear();
    s.advance(1_100);
    s.take_cmd(op::INQUIRY);
    assert_eq!(
        s.host.stats().errors,
        0,
        "errors:\n{}",
        s.io.logs
            .iter()
            .filter(|l| l.starts_with("PAD-ERR"))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Scenario 2: the gamepad, paired earlier in this run, calls us back.
/// It opens HID control before the link is encrypted: we answer
/// "pending", authenticate with the stored key, then say yes.
#[test]
fn incoming_reconnect_with_stored_key() {
    let mut s = Sim::new();
    pair_outgoing(&mut s);
    sdp_and_hid_outgoing(&mut s, &descriptor(60));
    s.feed(&ev_disconnected(H, 0x08));
    s.advance(1_100);
    s.take_cmd(op::INQUIRY);
    s.cmds.clear();
    s.sigs.clear();
    s.data.clear();
    let before = s.all_cmds.len();

    // It calls during our inquiry.
    s.feed(&ev_conn_request(PAD, PAD_COD));
    let a = s.take_cmd(op::ACCEPT_CONNECTION_REQUEST);
    assert_eq!(a, cat(&[&PAD.0, &[0x01]]), "accept and stay peripheral");
    s.take_cmd(op::INQUIRY_CANCEL);
    s.feed(&ev_conn_complete(0, H, PAD));
    // The name is unknown on an incoming link: we ask for it.
    let n = s.take_cmd(op::REMOTE_NAME_REQUEST);
    assert_eq!(&n[..6], &PAD.0);
    let mut name = cat(&[&[0], &PAD.0, b"Pro Controller"]);
    name.resize(255, 0);
    s.feed(&evt(0x07, &name));

    let (ctrl, result, status) = s.pad_opens(l2cap::PSM_HID_CONTROL, 0x0050);
    assert_eq!(
        (result, status),
        (conn_result::PENDING, conn_status::AUTHENTICATION_PENDING)
    );
    s.take_cmd(op::AUTHENTICATION_REQUESTED);
    s.feed(&ev_addr(0x17, PAD));
    let r = s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    assert_eq!(&r[..6], &PAD.0);
    assert_eq!(&r[6..], &KEY, "the key from the first pairing");
    s.feed(&ev_auth(0, H));
    s.take_cmd(op::SET_CONNECTION_ENCRYPTION);
    s.feed(&ev_encrypted(H));
    // Now the held-back answer: success, same identifier as the request.
    let (_, d) = s.take_sig(code::CONNECTION_RESPONSE);
    assert_eq!(
        (u16le(&d, 0), u16le(&d, 2), u16le(&d, 4)),
        (ctrl, 0x0050, conn_result::SUCCESS)
    );
    s.pad_finishes_config(ctrl, 0x0050);
    let (intr, result, _) = s.pad_opens(l2cap::PSM_HID_INTERRUPT, 0x0051);
    assert_eq!(result, conn_result::SUCCESS);
    s.pad_finishes_config(intr, 0x0051);
    assert_eq!(s.statuses().last().map(String::as_str), Some("Connected"));

    // SDP runs alongside.
    let sdp_ours = s.pad_accepts(l2cap::PSM_SDP, 0x0052);
    s.pad_serves_sdp(sdp_ours, 0x0052, &descriptor(60));
    s.advance(3_000);
    assert!(
        !s.all_cmds[before..].contains(&op::IO_CAPABILITY_REQUEST_REPLY),
        "no new pairing"
    );
    assert!(
        !s.sigs
            .iter()
            .any(|(c, _, d)| *c == code::CONNECTION_REQUEST && u16le(d, 0) != l2cap::PSM_SDP),
        "we must not open HID channels the pad opened"
    );

    s.pad_frame(H, intr, &[0xA1, 0x3F, 0x00, 0x00, 0x08], &[3]);
    assert_eq!(s.reports().last(), Some(&vec![0x3F, 0x00, 0x00, 0x08]));
    s.assert_key_never_printed();
}

/// Scenario 3: a gamepad we never paired with calls us and starts the
/// pairing itself; it also opens HID itself, before encryption.
#[test]
fn incoming_connection_without_key() {
    let mut s = Sim::new();
    s.advance(40);
    s.take_cmd(op::INQUIRY);
    s.feed(&ev_conn_request(PAD, PAD_COD));
    s.take_cmd(op::ACCEPT_CONNECTION_REQUEST);
    s.feed(&ev_conn_complete(0, H, PAD));
    s.advance(100);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_NEGATIVE_REPLY);
    s.advance(100);
    s.feed(&ev_addr(0x31, PAD));
    s.take_cmd(op::IO_CAPABILITY_REQUEST_REPLY);
    s.feed(&ev_io_cap_response(PAD));
    s.advance(300);
    s.feed(&ev_user_confirmation(PAD));
    s.take_cmd(op::USER_CONFIRMATION_REQUEST_REPLY);
    s.advance(200);
    s.feed(&ev_ssp_complete(PAD));
    s.feed(&ev_link_key(PAD));
    // HID control comes before encryption: "pending", and no second
    // authentication from us since the pad's pairing is under way.
    let (ctrl, result, _) = s.pad_opens(l2cap::PSM_HID_CONTROL, 0x0060);
    assert_eq!(result, conn_result::PENDING);
    s.advance(300);
    s.feed(&ev_encrypted(H));
    assert!(
        !s.sent(op::AUTHENTICATION_REQUESTED),
        "the pad did the security itself"
    );
    let (_, d) = s.take_sig(code::CONNECTION_RESPONSE);
    assert_eq!(u16le(&d, 4), conn_result::SUCCESS);
    s.pad_finishes_config(ctrl, 0x0060);
    let (intr, _, _) = s.pad_opens(l2cap::PSM_HID_INTERRUPT, 0x0061);
    s.pad_finishes_config(intr, 0x0061);
    assert_eq!(s.statuses().last().map(String::as_str), Some("Connected"));
    let sdp_ours = s.pad_accepts(l2cap::PSM_SDP, 0x0062);
    s.pad_serves_sdp(sdp_ours, 0x0062, &descriptor(40));
    s.pad_frame(H, intr, &[0xA1, 0x01, 0x02], &[]);
    assert_eq!(s.reports(), vec![vec![0x01, 0x02]]);
    s.assert_key_never_printed();
    assert_eq!(s.host.stats().errors, 0);
}

/// A phone (or anything that is not a peripheral) calling is refused.
#[test]
fn calls_from_non_peripherals_are_refused() {
    let mut s = Sim::new();
    s.feed(&ev_conn_request(PHONE, PHONE_COD));
    let r = s.take_cmd(op::REJECT_CONNECTION_REQUEST);
    assert_eq!(r, cat(&[&PHONE.0, &[0x0F]]));
    assert!(!s.sent(op::ACCEPT_CONNECTION_REQUEST));
    assert!(
        !s.io.logs.iter().any(|l| l.contains("26:25:24")),
        "a stranger's address was logged"
    );
}

/// Scenario 4: the gamepad does not like our configuration (it does not
/// know the MTU option), and sends options we must refuse: an unknown
/// one, then a retransmission mode we do not do.
#[test]
fn gamepad_rejects_a_configuration_option() {
    let mut s = Sim::new();
    pair_outgoing(&mut s);
    let sdp_ours = s.pad_accepts(l2cap::PSM_SDP, 0x0070);
    s.pad_serves_sdp(sdp_ours, 0x0070, &descriptor(50));
    s.advance(40);

    let (id, d) = s.take_sig(code::CONNECTION_REQUEST);
    assert_eq!(u16le(&d, 0), l2cap::PSM_HID_CONTROL);
    let ours = u16le(&d, 2);
    s.pad_sig(
        code::CONNECTION_RESPONSE,
        id,
        &cat(&[
            &0x0071_u16.to_le_bytes(),
            &ours.to_le_bytes(),
            &[0, 0, 0, 0],
        ]),
    );
    let (cid_id, d) = s.take_sig(code::CONFIGURE_REQUEST);
    assert_eq!(&d[4..], &[0x01, 2, 0xA0, 0x02], "we ask for MTU 672");
    // "Unknown options: MTU".
    s.pad_sig(
        code::CONFIGURE_RESPONSE,
        cid_id,
        &cat(&[
            &ours.to_le_bytes(),
            &[0, 0],
            &conf_result::UNKNOWN_OPTIONS.to_le_bytes(),
            &[0x01],
        ]),
    );
    let (cid_id2, d) = s.take_sig(code::CONFIGURE_REQUEST);
    assert_ne!(cid_id2, cid_id);
    assert_eq!(d.len(), 4, "second try without options");
    s.pad_sig(
        code::CONFIGURE_RESPONSE,
        cid_id2,
        &cat(&[&ours.to_le_bytes(), &[0, 0, 0, 0]]),
    );

    // Its request carries an unknown option 0x09 (not a hint).
    let pid = s.next_pad_id();
    s.pad_sig(
        code::CONFIGURE_REQUEST,
        pid,
        &cat(&[
            &ours.to_le_bytes(),
            &[0, 0],
            &[0x01, 2, 0xA0, 0x02],
            &[0x09, 2, 0xAA, 0xBB],
        ]),
    );
    let (_, d) = s.take_sig(code::CONFIGURE_RESPONSE);
    assert_eq!(u16le(&d, 4), conf_result::UNKNOWN_OPTIONS);
    assert_eq!(&d[6..], &[0x09], "the unknown option's type is listed");
    // A hint we do not know (0x80 | 0x0A) is fine to ignore.
    s.pad_configures_with(ours, &[0x8A, 1, 0x00], conf_result::SUCCESS);

    // The interrupt channel: the pad asks for enhanced retransmission mode.
    s.advance(40);
    let (id, d) = s.take_sig(code::CONNECTION_REQUEST);
    let intr = u16le(&d, 2);
    s.pad_sig(
        code::CONNECTION_RESPONSE,
        id,
        &cat(&[
            &0x0072_u16.to_le_bytes(),
            &intr.to_le_bytes(),
            &[0, 0, 0, 0],
        ]),
    );
    let (cid_id, _) = s.take_sig(code::CONFIGURE_REQUEST);
    s.pad_sig(
        code::CONFIGURE_RESPONSE,
        cid_id,
        &cat(&[&intr.to_le_bytes(), &[0, 0, 0, 0]]),
    );
    let pid = s.next_pad_id();
    s.pad_sig(
        code::CONFIGURE_REQUEST,
        pid,
        &cat(&[
            &intr.to_le_bytes(),
            &[0, 0],
            &[0x04, 9, 0x03, 8, 3, 0xD0, 0x07, 0xE0, 0x2E, 0xA0, 0x02],
        ]),
    );
    let (_, d) = s.take_sig(code::CONFIGURE_RESPONSE);
    assert_eq!(u16le(&d, 4), conf_result::UNACCEPTABLE);
    assert_eq!(&d[6..8], &[0x04, 9]);
    assert_eq!(d[8], 0x00, "we propose basic mode");
    s.pad_configures_with(
        intr,
        &[0x04, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        conf_result::SUCCESS,
    );
    assert_eq!(s.statuses().last().map(String::as_str), Some("Connected"));
    // A malformed option list is rejected.
    let pid = s.next_pad_id();
    s.pad_sig(
        code::CONFIGURE_REQUEST,
        pid,
        &cat(&[&intr.to_le_bytes(), &[0, 0], &[0x01, 5, 0xA0]]),
    );
    let (_, d) = s.take_sig(code::CONFIGURE_RESPONSE);
    assert_eq!(u16le(&d, 4), conf_result::REJECTED);
}

impl Sim {
    fn pad_configures_with(&mut self, ours: u16, options: &[u8], want: u16) {
        let id = self.next_pad_id();
        self.pad_sig(
            code::CONFIGURE_REQUEST,
            id,
            &cat(&[&ours.to_le_bytes(), &[0, 0], options]),
        );
        let (rid, d) = self.take_sig(code::CONFIGURE_RESPONSE);
        assert_eq!(rid, id);
        assert_eq!(u16le(&d, 4), want);
    }
}

/// Scenario 5: the HID record comes in three SDP pieces joined by
/// "continuation state", and each SDP answer is itself cut into several
/// ACL packets.
#[test]
fn fragmented_sdp_answer_with_continuation() {
    let mut s = Sim::new();
    pair_outgoing(&mut s);
    let ours = s.pad_accepts(l2cap::PSM_SDP, 0x0070);
    let desc = descriptor(400);
    let lists = hid_lists(&desc);
    assert!(lists.len() > 400);

    let req = s.take_data(0x0070);
    assert_eq!(be16(&req, 10), 600, "most bytes per answer");
    assert_eq!(*req.last().unwrap(), 0, "no continuation at first");
    s.pad_frame(
        H,
        ours,
        &sdp_rsp(be16(&req, 1), &lists[..150], &[0x02, 0xAB, 0x01]),
        &[7, 60],
    );
    let req = s.take_data(0x0070);
    assert_eq!(
        &req[req.len() - 4..],
        &[3, 0x02, 0xAB, 0x01],
        "continuation sent back"
    );
    assert_eq!(&req[5..10], &[0x35, 3, 0x19, 0x11, 0x24], "same query");
    s.pad_frame(
        H,
        ours,
        &sdp_rsp(be16(&req, 1), &lists[150..300], &[0x02, 0xAB, 0x02]),
        &[1, 2, 3, 100],
    );
    let req = s.take_data(0x0070);
    assert_eq!(&req[req.len() - 4..], &[3, 0x02, 0xAB, 0x02]);
    s.pad_frame(H, ours, &sdp_rsp(be16(&req, 1), &lists[300..], &[]), &[50]);
    assert_eq!(
        s.descriptor().as_deref(),
        Some(&desc[..]),
        "descriptor joined from three pieces"
    );

    // The Device ID query starts fresh (no continuation).
    let req = s.take_data(0x0070);
    assert_eq!(&req[5..10], &[0x35, 3, 0x19, 0x12, 0x00]);
    assert_eq!(*req.last().unwrap(), 0);
    s.pad_frame(H, ours, &sdp_rsp(be16(&req, 1), &pnp_lists(), &[]), &[]);
    s.pad_closes_ok(ours, 0x0070);
    s.advance(40);
    s.pad_accepts(l2cap::PSM_HID_CONTROL, 0x0071);
    s.advance(40);
    s.pad_accepts(l2cap::PSM_HID_INTERRUPT, 0x0072);
    assert_eq!(s.statuses().last().map(String::as_str), Some("Connected"));
    assert_eq!(s.host.stats().errors, 0, "{}", s.tail_log());
}

/// Both sides open HID control at the same moment: the extra channel
/// (ours) is closed, the gamepad's is kept.
#[test]
fn both_sides_open_hid_control() {
    let mut s = Sim::new();
    pair_outgoing(&mut s);
    let sdp_ours = s.pad_accepts(l2cap::PSM_SDP, 0x0070);
    s.pad_serves_sdp(sdp_ours, 0x0070, &descriptor(50));
    s.advance(40);
    // Our request is out; the pad's crosses it.
    let (pad_ctrl_ours, result, _) = s.pad_opens(l2cap::PSM_HID_CONTROL, 0x0090);
    assert_eq!(result, conn_result::SUCCESS);
    let our_ctrl = s.pad_accepts(l2cap::PSM_HID_CONTROL, 0x0071);
    s.pad_finishes_config(pad_ctrl_ours, 0x0090);
    // The extra one, ours, is closed.
    s.pad_closes_ok(our_ctrl, 0x0071);
    let (intr, _, _) = s.pad_opens(l2cap::PSM_HID_INTERRUPT, 0x0091);
    s.pad_finishes_config(intr, 0x0091);
    assert_eq!(s.statuses().last().map(String::as_str), Some("Connected"));
    s.advance(8_000);
    assert!(
        !s.sigs
            .iter()
            .any(|(c, _, _)| *c == code::CONNECTION_REQUEST),
        "no further channel opened"
    );
    assert!(!s.sent(op::DISCONNECT));
}

/// A stored key the gamepad no longer has: authentication fails, the key
/// is dropped and we pair again on the same link.
#[test]
fn stale_key_is_dropped_and_pairing_repeats() {
    let mut s = Sim::new();
    pair_outgoing(&mut s);
    sdp_and_hid_outgoing(&mut s, &descriptor(50));
    s.feed(&ev_disconnected(H, 0x08));
    s.advance(1_100);
    s.cmds.clear();
    s.sigs.clear();
    s.feed(&ev_conn_request(PAD, PAD_COD));
    s.feed(&ev_conn_complete(0, H, PAD));
    s.advance(700);
    s.take_cmd(op::AUTHENTICATION_REQUESTED);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    s.feed(&ev_auth(0x06, H)); // "PIN or key missing"
    s.take_cmd(op::AUTHENTICATION_REQUESTED);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_NEGATIVE_REPLY);
    s.feed(&ev_addr(0x31, PAD));
    s.take_cmd(op::IO_CAPABILITY_REQUEST_REPLY);
    s.feed(&ev_user_confirmation(PAD));
    s.take_cmd(op::USER_CONFIRMATION_REQUEST_REPLY);
    s.feed(&ev_link_key(PAD));
    s.feed(&ev_auth(0, H));
    s.take_cmd(op::SET_CONNECTION_ENCRYPTION);
}

/// A call to an address nobody answers fails cleanly, and the search
/// starts again.
#[test]
fn page_to_made_up_address_fails_cleanly() {
    let mut s = Sim::new();
    s.host.request_page(FAKE, Purpose::Gamepad);
    s.advance(40);
    let cc = s.take_cmd(op::CREATE_CONNECTION);
    assert_eq!(&cc[..6], &FAKE.0);
    s.advance(5_120);
    s.feed(&ev_conn_complete(0x04, 0, FAKE)); // page timeout
    assert!(
        s.statuses()
            .iter()
            .any(|t| t == "Connect failed: page-timeout" || t.starts_with("Connect failed"))
    );
    assert!(!s.host.linked());
    s.advance(1_600);
    s.take_cmd(op::INQUIRY);
    assert!(!s.io.out.iter().any(|p| p[0] == 0x02), "no data was sent");
}

/// The SDP-only probe of a device that is not a gamepad (the owner's
/// phone on the board): no pairing at all, an SDP query in small pieces,
/// then everything is closed.
#[test]
fn sdp_probe_never_pairs() {
    let mut s = Sim::new();
    s.host.request_page(
        PHONE,
        Purpose::SdpProbe {
            uuid: 0x0100,
            max_bytes: 48,
        },
    );
    s.advance(40);
    s.take_cmd(op::CREATE_CONNECTION);
    s.feed(&ev_conn_complete(0, H, PHONE));
    assert!(!s.sent(op::AUTHENTICATION_REQUESTED));
    // The phone asks for a key and for IO capabilities: always no.
    s.feed(&ev_addr(0x17, PHONE));
    s.take_cmd(op::LINK_KEY_REQUEST_NEGATIVE_REPLY);
    s.feed(&ev_addr(0x31, PHONE));
    s.take_cmd(op::IO_CAPABILITY_REQUEST_NEGATIVE_REPLY);
    s.feed(&ev_user_confirmation(PHONE));
    s.take_cmd(op::USER_CONFIRMATION_REQUEST_NEGATIVE_REPLY);
    // It tries HID: refused.
    let (_, result, _) = s.pad_opens(l2cap::PSM_HID_CONTROL, 0x0099);
    assert_eq!(result, conn_result::SECURITY_BLOCK);

    let ours = s.pad_accepts(l2cap::PSM_SDP, 0x0040);
    let req = s.take_data(0x0040);
    assert_eq!(&req[5..10], &[0x35, 3, 0x19, 0x01, 0x00]);
    assert_eq!(be16(&req, 10), 48);
    let mut b: Builder<256> = Builder::new();
    let outer = b.seq();
    for _ in 0..3 {
        let r = b.seq();
        b.u16(0x0001);
        let c = b.seq();
        b.uuid16(0x110A);
        b.end(c);
        b.u16(0x0100).text(b"Some service");
        b.end(r);
    }
    b.end(outer);
    let lists = b.bytes().to_vec();
    let mut at = 0;
    let mut k = 0;
    while at < lists.len() {
        let end = (at + 48).min(lists.len());
        let cont: Vec<u8> = if end < lists.len() {
            vec![1, k]
        } else {
            vec![]
        };
        s.pad_frame(
            H,
            ours,
            &sdp_rsp(be16(&req, 1), &lists[at..end], &cont),
            &[],
        );
        at = end;
        k += 1;
        if !cont.is_empty() {
            let r = s.take_data(0x0040);
            assert_eq!(&r[r.len() - 3..], &[2, 1, k - 1]);
        }
    }
    assert!(k >= 2);
    assert!(
        s.io.logs.iter().any(|l| l.contains("done records=3")),
        "{}",
        s.tail_log()
    );
    s.pad_closes_ok(ours, 0x0040);
    s.advance(600);
    assert_eq!(s.take_cmd(op::DISCONNECT), [0x0B, 0x00, 0x13]);
    assert!(
        !s.sent(op::AUTHENTICATION_REQUESTED)
            && !s.sent(op::IO_CAPABILITY_REQUEST_REPLY)
            && !s.sent(op::USER_CONFIRMATION_REQUEST_REPLY)
    );
    s.feed(&ev_disconnected(H, 0x16));
    s.advance(1_100);
    s.take_cmd(op::INQUIRY);
}

/// A virtual cable unplug from the gamepad: the key is forgotten and
/// the link closed.
#[test]
fn virtual_cable_unplug_forgets_the_key() {
    let mut s = Sim::new();
    pair_outgoing(&mut s);
    let (ctrl, _) = sdp_and_hid_outgoing(&mut s, &descriptor(50));
    s.pad_frame(H, ctrl, &[0x15], &[]);
    assert_eq!(s.take_cmd(op::DISCONNECT), [0x0B, 0x00, 0x13]);
    // The caller is told to erase it too.
    assert_eq!(
        s.io.changes.last(),
        Some(&KeyChange::Forget {
            addr: PAD,
            why: ForgetReason::Unplugged
        })
    );
    s.feed(&ev_disconnected(H, 0x16));
    s.advance(1_100);
    s.feed(&ev_conn_request(PAD, PAD_COD));
    s.feed(&ev_conn_complete(0, H, PAD));
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_NEGATIVE_REPLY);
}

/// Unknown events and odd packets are logged, never fatal.
#[test]
fn odd_input_is_survived() {
    let mut s = Sim::new();
    s.feed(&evt(0x77, &[1, 2, 3]));
    s.feed(&[0x04, 0x0E]);
    s.feed(&[0x03, 0, 0]);
    s.feed(acl::packet(0x0B, acl::PB_CONTINUE, &[1, 2, 3]).as_bytes());
    assert!(s.io.logs.iter().any(|l| l.contains("code=0x77")));
    s.advance(40);
    s.take_cmd(op::INQUIRY);
}

/// Both sides start a security step at the same moment ("collision"):
/// authentication, then encryption, are each tried once more instead of
/// dropping the link. The gamepad is in pairing mode only 30 to 40 s, so
/// a lost link here would cost the owner another try.
#[test]
fn security_collisions_are_retried() {
    let mut s = Sim::new();
    find_and_call_pad(&mut s);
    s.feed(&ev_conn_complete(0, H, PAD));
    s.take_cmd(op::AUTHENTICATION_REQUESTED);
    // LMP transaction collision.
    s.feed(&ev_auth(0x23, H));
    assert!(!s.sent(op::DISCONNECT), "a collision must not end the link");
    s.advance(600);
    assert_eq!(
        s.take_cmd(op::AUTHENTICATION_REQUESTED),
        H.to_le_bytes(),
        "a second try"
    );

    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_NEGATIVE_REPLY);
    s.feed(&ev_addr(0x31, PAD));
    s.take_cmd(op::IO_CAPABILITY_REQUEST_REPLY);
    s.feed(&ev_io_cap_response(PAD));
    s.feed(&ev_user_confirmation(PAD));
    s.take_cmd(op::USER_CONFIRMATION_REQUEST_REPLY);
    s.feed(&ev_ssp_complete(PAD));
    s.feed(&ev_link_key(PAD));
    s.feed(&ev_auth(0, H));
    s.take_cmd(op::SET_CONNECTION_ENCRYPTION);
    // Different transaction collision on encryption.
    s.feed(&evt(0x08, &cat(&[&[0x2A], &H.to_le_bytes(), &[0]])));
    assert!(!s.sent(op::DISCONNECT));
    s.advance(600);
    assert_eq!(
        s.take_cmd(op::SET_CONNECTION_ENCRYPTION),
        [0x0B, 0x00, 0x01]
    );
    s.feed(&ev_encrypted(H));
    // On to the description and HID as usual.
    let (_ctrl, intr) = sdp_and_hid_outgoing(&mut s, &descriptor(50));
    s.pad_frame(H, intr, &[0xA1, 0x01, 0x7F], &[]);
    assert_eq!(s.reports(), vec![vec![0x01, 0x7F]]);
    assert!(!s.sent(op::DISCONNECT));
    s.assert_key_never_printed();
}

/// A second collision on the same step is not retried for ever: the link
/// is closed and the search goes on.
#[test]
fn repeated_collision_gives_up() {
    let mut s = Sim::new();
    find_and_call_pad(&mut s);
    s.feed(&ev_conn_complete(0, H, PAD));
    s.take_cmd(op::AUTHENTICATION_REQUESTED);
    s.feed(&ev_auth(0x23, H));
    s.advance(600);
    s.take_cmd(op::AUTHENTICATION_REQUESTED);
    s.feed(&ev_auth(0x23, H));
    let d = s.take_cmd(op::DISCONNECT);
    assert_eq!(d, [0x0B, 0x00, 0x05]);
    s.feed(&ev_disconnected(H, 0x16));
    s.cmds.clear();
    s.advance(1_100);
    s.take_cmd(op::INQUIRY);
}

// ------------------------------------------- after the review (pad-fix)

const OTHER: BdAddr = BdAddr([0x31, 0x32, 0x33, 0x34, 0x35, 0x36]);

fn summaries(s: &Sim) -> Vec<String> {
    s.io.logs
        .iter()
        .filter(|l| l.starts_with("PAD-SUMMARY"))
        .cloned()
        .collect()
}

/// Review finding 1: output reports can only go out once both HID
/// channels are open; `hid_ready` says when.
#[test]
fn hid_ready_only_when_both_hid_channels_are_open() {
    let mut s = Sim::new();
    assert!(!s.host.hid_ready());
    pair_outgoing(&mut s);
    assert!(!s.host.hid_ready(), "encrypted is not enough");
    assert!(
        !s.host
            .send_output_report(s.now, &[0x01, 0x00, 0x10], &mut s.io),
        "nothing to send on yet"
    );
    let sdp_ours = s.pad_accepts(l2cap::PSM_SDP, 0x0070);
    s.pad_serves_sdp(sdp_ours, 0x0070, &descriptor(50));
    let _ctrl = s.pad_accepts(l2cap::PSM_HID_CONTROL, 0x0071);
    assert!(!s.host.hid_ready(), "control alone is not enough");
    s.advance(40);
    let _intr = s.pad_accepts(l2cap::PSM_HID_INTERRUPT, 0x0072);
    assert!(s.host.hid_ready());
    assert!(
        s.host
            .send_output_report(s.now, &[0x01, 0x00, 0x10], &mut s.io)
    );
    s.feed(&ev_disconnected(H, 0x13));
    assert!(!s.host.hid_ready(), "link gone");
}

/// Review finding 2: a configuration request in two parts (continuation
/// flag): the MTU of the first part counts.
#[test]
fn configuration_in_two_parts_keeps_the_mtu() {
    let mut s = Sim::new();
    pair_outgoing(&mut s);
    let sdp_ours = s.pad_accepts(l2cap::PSM_SDP, 0x0070);
    s.pad_serves_sdp(sdp_ours, 0x0070, &descriptor(50));
    let _ctrl = s.pad_accepts(l2cap::PSM_HID_CONTROL, 0x0071);
    s.advance(40);
    let (id, d) = s.take_sig(code::CONNECTION_REQUEST);
    assert_eq!(u16le(&d, 0), l2cap::PSM_HID_INTERRUPT);
    let intr = u16le(&d, 2);
    s.pad_sig(
        code::CONNECTION_RESPONSE,
        id,
        &cat(&[
            &0x0072_u16.to_le_bytes(),
            &intr.to_le_bytes(),
            &[0, 0, 0, 0],
        ]),
    );
    let (cid_id, _) = s.take_config_request(0x0072);
    s.pad_sig(
        code::CONFIGURE_RESPONSE,
        cid_id,
        &cat(&[&intr.to_le_bytes(), &[0, 0, 0, 0]]),
    );
    // Part 1: "more follows", MTU 100.
    let p1 = s.next_pad_id();
    s.pad_sig(
        code::CONFIGURE_REQUEST,
        p1,
        &cat(&[
            &intr.to_le_bytes(),
            &[0x01, 0x00],
            &[0x01, 2],
            &100_u16.to_le_bytes(),
        ]),
    );
    let (rid, d) = s.take_sig(code::CONFIGURE_RESPONSE);
    assert_eq!(rid, p1);
    assert_eq!(
        u16le(&d, 2) & 1,
        1,
        "the answer to a part carries the flag too"
    );
    assert_eq!(u16le(&d, 4), conf_result::SUCCESS);
    assert!(!s.host.hid_ready(), "not open before the last part");
    // Part 2: the last, with no options.
    let p2 = s.next_pad_id();
    s.pad_sig(
        code::CONFIGURE_REQUEST,
        p2,
        &cat(&[&intr.to_le_bytes(), &[0x00, 0x00]]),
    );
    let (_, d) = s.take_sig(code::CONFIGURE_RESPONSE);
    assert_eq!(u16le(&d, 2) & 1, 0);
    assert_eq!(u16le(&d, 4), conf_result::SUCCESS);
    assert!(s.host.hid_ready());
    assert!(
        s.io.logs.iter().any(|l| l.contains("remote_mtu=100")),
        "log:\n{}",
        s.tail_log()
    );
    assert!(
        s.host.send_output_report(s.now, &[0u8; 99], &mut s.io),
        "99 bytes + 0xA2 fit in 100"
    );
    assert!(
        !s.host.send_output_report(s.now, &[0u8; 100], &mut s.io),
        "101 bytes do not fit in 100"
    );
}

/// Review finding 3: with an allow-list, a gamepad that does not match is
/// neither called nor logged by address or name.
#[test]
fn allow_list_skips_other_gamepads_quietly() {
    let mut s = Sim::with_config(Config {
        only: Some("GuliKit, Pro Controller"),
        ..Config::default()
    });
    s.advance(40);
    s.take_cmd(op::INQUIRY);
    s.feed(&ev_ext_inquiry(OTHER, PAD_COD, b"Someone's Joypad"));
    assert!(!s.sent(op::INQUIRY_CANCEL), "not called");
    let other = OTHER.to_string();
    assert!(
        !s.io
            .logs
            .iter()
            .any(|l| l.contains("Someone") || l.contains(&other) || l.contains("31:32:33")),
        "stranger in the log:\n{}",
        s.tail_log()
    );
    assert!(
        s.io.logs
            .iter()
            .any(|l| l.contains("found gamepad not on the allow-list cod=0x002508"))
    );
    assert_eq!(
        (s.host.stats().found_skipped, s.host.stats().found_gamepads),
        (1, 0)
    );
    // The owner's pad matches by a piece of its name, in any case.
    s.feed(&ev_ext_inquiry(PAD, PAD_COD, b"pro controller"));
    s.take_cmd(op::INQUIRY_CANCEL);
    let cc = s.take_cmd(op::CREATE_CONNECTION);
    assert_eq!(&cc[..6], &PAD.0);
}

/// An allow-list with an address also decides who may call us.
#[test]
fn allow_list_by_address_checks_calls_too() {
    // PAD is written the usual way, last byte first.
    let mut s = Sim::with_config(Config {
        only: Some("06:05:04:03:02:01"),
        ..Config::default()
    });
    s.advance(40);
    s.feed(&ev_conn_request(OTHER, PAD_COD));
    let r = s.take_cmd(op::REJECT_CONNECTION_REQUEST);
    assert_eq!((&r[..6], r[6]), (&OTHER.0[..], 0x0F));
    assert!(
        !s.io.logs.iter().any(|l| l.contains(&OTHER.to_string())),
        "stranger's address in the log"
    );
    s.feed(&ev_conn_request(PAD, PAD_COD));
    s.take_cmd(op::ACCEPT_CONNECTION_REQUEST);
    s.feed(&ev_conn_complete(0, H, PAD));
    s.feed(&ev_disconnected(H, 0x13));
    let sums = summaries(&s);
    assert_eq!(sums.len(), 1, "{sums:?}");
    assert!(sums[0].starts_with("PAD-SUMMARY end "), "{}", sums[0]);
    for w in [
        "link=it-called",
        "reached=linked",
        "pairing=none",
        "reports=0",
        "ids=-",
        "reason=0x13",
    ] {
        assert!(sums[0].contains(w), "{w} missing in {}", sums[0]);
    }
    // The plain function, for the record.
    assert!(pad_proto::host::allowed(None, OTHER, b"x"));
    assert!(
        pad_proto::host::allowed(Some(" , "), OTHER, b"x"),
        "an empty list is no list"
    );
    assert!(pad_proto::host::allowed(
        Some("gulikit"),
        OTHER,
        b"GuliKit Controller XW"
    ));
    assert!(!pad_proto::host::allowed(
        Some("GuliKit"),
        OTHER,
        b"Xbox Wireless Controller"
    ));
    assert!(
        pad_proto::host::allowed_call(Some("GuliKit"), OTHER),
        "names cannot be checked on a call"
    );
}

/// PAD-SUMMARY: one line when reports flow, one when the link ends; no
/// address in either.
#[test]
fn summary_lines_when_reports_flow_and_when_the_link_ends() {
    let mut s = Sim::new();
    pair_outgoing(&mut s);
    let (_ctrl, intr) = sdp_and_hid_outgoing(&mut s, &descriptor(120));
    // No report for 3 s: the log says what it waits for.
    s.advance(3_100);
    assert!(
        s.io.logs
            .iter()
            .any(|l| l.contains("waiting: the HID channels have been open")
                && l.contains("awaited a report")),
        "log:\n{}",
        s.tail_log()
    );
    for k in 0..30_u8 {
        s.pad_frame(H, intr, &[0xA1, 0x30, k, 0, 0], &[]);
        s.advance(20);
    }
    s.pad_frame(H, intr, &[0xA1, 0x21, 1, 2, 3, 4, 5, 6], &[]);
    assert!(summaries(&s).is_empty(), "too early for the flowing line");
    s.advance(1_500);
    let sums = summaries(&s);
    assert_eq!(sums.len(), 1, "{sums:?}");
    let f = &sums[0];
    assert!(
        f.starts_with(
            "PAD-SUMMARY flowing name=\"Pro Controller\" class=0x002508 vendor=057e product=2009"
        ),
        "{f}"
    );
    for w in [
        "descriptor_bytes=120",
        "pairing=new",
        "link=we-called",
        "reached=reports",
        "reports=31",
        "ids=30:4x30,21:7x1",
        "found_to_link_ms=",
        "hid_to_first_report_ms=31",
    ] {
        assert!(f.contains(w), "{w} missing in {f}");
    }
    assert!(!f.contains("rate=-"), "{f}");
    s.feed(&ev_disconnected(H, 0x08));
    let sums = summaries(&s);
    assert_eq!(sums.len(), 2);
    assert!(
        sums[1].starts_with("PAD-SUMMARY end ")
            && sums[1].ends_with("reason=0x08(connection-timeout)"),
        "{}",
        sums[1]
    );
    let addr = PAD.to_string();
    assert!(
        sums.iter().all(|l| !l.contains(&addr)),
        "address in a summary"
    );
}

/// A timeout says what it waited for, so the log alone shows where a
/// try stopped.
#[test]
fn timeouts_say_what_was_awaited() {
    let mut s = Sim::new();
    pair_outgoing(&mut s);
    let _sdp_ours = s.pad_accepts(l2cap::PSM_SDP, 0x0070);
    // The pad never answers the SDP request.
    s.advance(8_100);
    assert!(
        s.io.logs
            .iter()
            .any(|l| l.starts_with("PAD-ERR timeout: SDP")
                && l.contains("awaited the answer to SDP request tid=1")),
        "log:\n{}",
        s.tail_log()
    );
    // We go on with HID; the pad never answers that either.
    let (_, d) = s.take_sig(code::CONNECTION_REQUEST);
    assert_eq!(u16le(&d, 0), l2cap::PSM_HID_CONTROL);
    s.advance(5_100);
    assert!(
        s.io.logs
            .iter()
            .any(|l| l.starts_with("PAD-ERR timeout: channel")
                && l.contains("awaited their connection-response to our request id=")),
        "log:\n{}",
        s.tail_log()
    );
}

// ------------------------------------------------- output reports on the air

/// The SDP, HID control and HID interrupt channel ids the GuliKit gave in
/// its Switch-mode session of the 2026-11-16 live test.
const LIVE_SDP: u16 = 0x0041;
const LIVE_CTRL: u16 = 0x0042;
const LIVE_INTR: u16 = 0x0043;

/// The first start-up packet of the live test, as the view built it:
/// report 0x01, counter 0, neutral rumble, subcommand 0x03 argument 0x30,
/// zeros up to 49 bytes.
fn live_packet() -> Vec<u8> {
    let mut r = vec![0u8; 49];
    r[..12].copy_from_slice(&[
        0x01, 0x00, 0x00, 0x01, 0x40, 0x40, 0x00, 0x01, 0x40, 0x40, 0x03, 0x30,
    ]);
    r
}

/// Pairs and opens SDP and both HID channels with the live test's channel
/// ids. Returns our (control, interrupt) channel ids.
fn connect_like_live(s: &mut Sim) -> (u16, u16) {
    pair_outgoing(s);
    let sdp_ours = s.pad_accepts(l2cap::PSM_SDP, LIVE_SDP);
    s.pad_serves_sdp(sdp_ours, LIVE_SDP, &descriptor(60));
    s.advance(40);
    let ctrl = s.pad_accepts(l2cap::PSM_HID_CONTROL, LIVE_CTRL);
    s.advance(40);
    let intr = s.pad_accepts(l2cap::PSM_HID_INTERRUPT, LIVE_INTR);
    assert!(s.host.hid_ready());
    (ctrl, intr)
}

/// The ACL packets (type byte first) the host handed to the controller
/// from index `from` of everything it sent.
fn acl_packets(s: &Sim, from: usize) -> Vec<Vec<u8>> {
    s.io.out[from..]
        .iter()
        .filter(|p| p[0] == 0x02)
        .cloned()
        .collect()
}

/// Task 1 of the Switch start-up work: are our output reports well-formed
/// on the air? Every header of the exact bytes the host hands to the
/// radio for the live test's first start-up packet, and the same flags as
/// the signalling and SDP packets that worked in the same session.
#[test]
fn output_report_bytes_are_well_formed() {
    let mut s = Sim::new();
    let start = s.io.out.len();
    connect_like_live(&mut s);
    let before = s.io.out.len();
    let report = live_packet();
    assert!(s.host.send_output_report(s.now, &report, &mut s.io));
    s.settle();
    let sent = acl_packets(&s, before);
    assert_eq!(sent.len(), 1, "one ACL packet for one 49-byte report");
    let p = &sent[0];
    // HCI: type 0x02 (ACL data), then handle + flags, then length.
    assert_eq!(p[0], 0x02, "HCI packet type ACL data");
    let hw = u16le(p, 1);
    assert_eq!(hw & 0x0FFF, H, "connection handle");
    assert_eq!(
        (hw >> 12) & 0b11,
        u16::from(acl::PB_START),
        "packet boundary: first automatically-flushable packet"
    );
    assert_eq!(hw >> 14, 0, "broadcast flag: point-to-point");
    assert_eq!(
        usize::from(u16le(p, 3)),
        p.len() - 5,
        "ACL length = the rest of the packet"
    );
    assert_eq!(
        u16le(p, 3),
        4 + 1 + 49,
        "ACL length: L2CAP header 4 + 0xA2 + 49"
    );
    // L2CAP basic header: length, then the gamepad's (remote) channel id.
    assert_eq!(u16le(p, 5), 1 + 49, "L2CAP length: 0xA2 + 49");
    assert_eq!(
        u16le(p, 7),
        LIVE_INTR,
        "the gamepad's interrupt channel id, as in the live log"
    );
    // HIDP header and the report itself.
    assert_eq!(p[9], 0xA2, "HIDP DATA + Output");
    assert_eq!(&p[10..], &report[..], "the 49 report bytes, untouched");
    assert_eq!(p.len(), 5 + 4 + 1 + 49);

    // The same flags as every signalling and SDP packet of this session,
    // which the gamepad clearly understood (it answered them).
    let earlier = acl_packets(&s, start);
    let earlier = &earlier[..earlier.len() - 1];
    assert!(
        earlier.len() >= 10,
        "signalling and SDP packets: {}",
        earlier.len()
    );
    for e in earlier {
        assert_eq!(u16le(e, 1), hw, "same handle and flags as {e:02x?}");
        assert_eq!(usize::from(u16le(e, 3)), e.len() - 5);
        assert_eq!(
            usize::from(u16le(e, 5)),
            e.len() - 9,
            "L2CAP length of {e:02x?}"
        );
    }
    // The controller gave the buffer back: the log says the output left.
    assert!(
        s.io.logs
            .iter()
            .any(|l| l.starts_with("PAD-HCI completed output n=1 ")),
        "log:\n{}",
        s.tail_log()
    );
    let out_line =
        s.io.logs
            .iter()
            .find(|l| l.starts_with("PAD-HID output n=1 "))
            .expect("output line");
    assert!(
        out_line.contains("cid=0x0043") && out_line.contains("len=49"),
        "{out_line}"
    );
}

/// Part B: a GET_REPORT for input report 04 (the battery of an Xbox-mode
/// pad) goes on the control channel with header 0x41. The answer, DATA
/// Input with the report, is logged with its time and reaches the screen
/// hook. A refusal (HANDSHAKE) is logged as the answer to the GET_REPORT,
/// not as one to a SET_REPORT.
#[test]
fn get_report_for_the_battery_and_its_answers() {
    let mut s = Sim::new();
    let (ctrl, _intr) = connect_like_live(&mut s);
    assert!(s.host.send_get_report(s.now, 0x04, &mut s.io));
    s.settle();
    let msg = s.take_data(LIVE_CTRL);
    assert_eq!(msg, vec![0x41, 0x04], "GET_REPORT, Input, report 04");
    assert!(
        s.io.logs
            .iter()
            .any(|l| l.starts_with("PAD-HID get-report n=1 len=1 cid=0x0042")),
        "log:\n{}",
        s.tail_log()
    );

    s.advance(15);
    s.pad_frame(H, ctrl, &[0xA1, 0x04, 0x13], &[]);
    assert!(
        s.io.logs
            .iter()
            .any(|l| l == "PAD-HID get-report answer id=0x04 len=2 in 15 ms"),
        "log:\n{}",
        s.tail_log()
    );
    assert!(s.io.ui.contains(&Ui::Control(vec![0xA1, 0x04, 0x13])));

    // Asked again; this time the pad refuses.
    assert!(s.host.send_get_report(s.now, 0x04, &mut s.io));
    s.settle();
    s.take_data(LIVE_CTRL);
    s.advance(5);
    s.pad_frame(H, ctrl, &[0x02], &[]);
    assert!(
        s.io.logs.iter().any(|l| l
            == "PAD-HID handshake result=invalid-report-id(0x2) after get-report id=0x04 in 5 ms (the pad did not send the report)"),
        "log:\n{}",
        s.tail_log()
    );
    assert!(
        !s.io
            .logs
            .iter()
            .any(|l| l.contains("no set-report of ours")),
        "a refused GET_REPORT is not an unasked handshake"
    );
    assert!(s.io.ui.contains(&Ui::Control(vec![0x02])));
}

/// A SET_REPORT goes on the control channel with header 0x52; the
/// gamepad's HANDSHAKE comes back on the control channel, is logged with
/// its result, and reaches the screen hook. Other control messages too.
#[test]
fn set_report_on_the_control_channel_and_its_handshake() {
    let mut s = Sim::new();
    let (ctrl, _intr) = connect_like_live(&mut s);
    let report = live_packet();
    assert!(s.host.send_set_report(s.now, &report, &mut s.io));
    s.settle();
    let msg = s.take_data(LIVE_CTRL);
    assert_eq!(msg[0], 0x52, "SET_REPORT, report type Output");
    assert_eq!(&msg[1..], &report[..]);
    assert!(
        s.io.logs
            .iter()
            .any(|l| l.starts_with("PAD-HID set-report n=1 len=49 cid=0x0042")),
        "log:\n{}",
        s.tail_log()
    );
    assert!(
        s.io.logs
            .iter()
            .any(|l| l.starts_with("PAD-HCI completed set-report n=1 "))
    );

    s.advance(20);
    s.pad_frame(H, ctrl, &[0x00], &[]);
    assert!(
        s.io.logs
            .iter()
            .any(|l| l == "PAD-HID handshake result=successful(0x0) after set-report n=1 in 20 ms"),
        "log:\n{}",
        s.tail_log()
    );
    assert!(s.io.ui.contains(&Ui::Control(vec![0x00])));

    // A second one, refused by the gamepad.
    assert!(s.host.send_set_report(s.now, &report, &mut s.io));
    s.settle();
    s.take_data(LIVE_CTRL);
    s.pad_frame(H, ctrl, &[0x03], &[]);
    assert!(
        s.io.logs.iter().any(|l| l.starts_with(
            "PAD-HID handshake result=unsupported-request(0x3) after set-report n=2 "
        )),
        "log:\n{}",
        s.tail_log()
    );
    // A handshake nobody asked for, and data on the control channel.
    s.pad_frame(H, ctrl, &[0x01], &[]);
    assert!(s.io.logs.iter().any(|l| l.starts_with(
        "PAD-HID handshake result=not-ready(0x1) with no set-report of ours waiting"
    )));
    s.pad_frame(H, ctrl, &[0xA3, 0x02, 0x11], &[]);
    assert!(
        s.io.logs
            .iter()
            .any(|l| l == "PAD-HID control data-feature len=3 hex=a30211"),
        "log:\n{}",
        s.tail_log()
    );
    assert!(s.io.ui.contains(&Ui::Control(vec![0xA3, 0x02, 0x11])));

    // Mixed with an interrupt output: both counted, in order.
    assert!(s.host.send_output_report(s.now, &report, &mut s.io));
    s.settle();
    assert_eq!(s.take_data(LIVE_INTR)[0], 0xA2);
    s.feed(&ev_disconnected(H, 0x13));
    let end = summaries(&s)
        .into_iter()
        .find(|l| l.starts_with("PAD-SUMMARY end "))
        .expect("end summary");
    assert!(
        end.contains(" outputs=3 set_reports=2 handshakes=3 "),
        "{end}"
    );
    assert!(end.contains(" acl_completed="), "{end}");
    assert_eq!(s.host.stats().errors, 0);
}

/// Without a HID control channel there is no SET_REPORT.
#[test]
fn set_report_needs_the_control_channel() {
    let mut s = Sim::new();
    assert!(!s.host.send_set_report(s.now, &[0x01, 0x00], &mut s.io));
    pair_outgoing(&mut s);
    assert!(
        !s.host.send_set_report(s.now, &[0x01, 0x00], &mut s.io),
        "encrypted but no HID channel"
    );
}

/// A Switch pad's answer to "device info" carries its own address
/// (forwards); HCI carries it backwards. Neither reaches the report log.
#[test]
fn the_pad_address_inside_a_report_is_not_logged() {
    let mut s = Sim::new();
    let (_ctrl, intr) = connect_like_live(&mut s);
    let mut r = vec![
        0xA1, 0x21, 0x05, 0x8E, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x0B, 0x82, 0x02, 0x03, 0x48, 0x03, 0x02,
    ];
    r.extend(PAD.0.iter().rev());
    r.extend_from_slice(&[0x01, 0x01]);
    s.pad_frame(H, intr, &r, &[]);
    let mut r2 = vec![0xA1, 0x3F];
    r2.extend(PAD.0.iter());
    s.pad_frame(H, intr, &r2, &[]);
    let fwd: String = PAD.0.iter().rev().map(|b| format!("{b:02x}")).collect();
    let back: String = PAD.0.iter().map(|b| format!("{b:02x}")).collect();
    let lines: Vec<&String> =
        s.io.logs
            .iter()
            .filter(|l| l.starts_with("PAD-HID report"))
            .collect();
    assert_eq!(lines.len(), 2);
    for l in &lines {
        assert!(!l.contains(&fwd) && !l.contains(&back), "address in {l}");
        assert!(l.contains("xxxxxxxxxxxx"), "{l}");
    }
    assert!(
        lines[0].contains(
            "hex=21058e000000000000000000 0b820203480302 xxxxxxxxxxxx 0101"
                .replace(' ', "")
                .as_str()
        ),
        "{}",
        lines[0]
    );
    // The screen still gets the real bytes.
    assert!(
        s.reports()
            .iter()
            .any(|x| x.len() == 27 && x[19..25] == r[20..26])
    );
}

/// A Switch device-info answer carries the pad's own address, which may
/// differ from the link address: the report log hides it by position, on
/// the interrupt channel and on the control channel.
#[test]
fn device_info_address_is_hidden() {
    let mut s = Sim::new();
    pair_outgoing(&mut s);
    let (ctrl, intr) = sdp_and_hid_outgoing(&mut s, &descriptor(50));
    let mut r = vec![0u8; 49];
    r[0] = 0x21;
    r[13] = 0x82;
    r[14] = 0x02;
    r[15] = 0x03;
    r[16] = 0x8B;
    r[17] = 0x03;
    r[18] = 0x02;
    r[19..25].copy_from_slice(&[0x31, 0x32, 0x33, 0x34, 0x35, 0x36]);
    r[25] = 0x01;
    let mut on_intr = vec![0xA1];
    on_intr.extend_from_slice(&r);
    s.pad_frame(H, intr, &on_intr, &[]);
    s.pad_frame(H, ctrl, &on_intr, &[]);
    let hidden = "8202038b0302xxxxxxxxxxxx01";
    let lines: Vec<&String> = s.io.logs.iter().filter(|l| l.contains("hex=")).collect();
    assert!(
        lines.iter().filter(|l| l.contains(hidden)).count() >= 2,
        "log:\n{}",
        s.tail_log()
    );
    assert!(
        !s.io.logs.iter().any(|l| l.contains("313233343536")),
        "log:\n{}",
        s.tail_log()
    );
}

/// While the controller takes no packets, output reports fill the send
/// queue. Once it is full, a report is refused ("not sent", so the caller
/// keeps it) instead of accepted and then dropped; nothing is logged as
/// dropped. When the controller takes packets again, sending works.
#[test]
fn full_send_queue_refuses_instead_of_dropping() {
    let mut s = Sim::new();
    connect_like_live(&mut s);
    let report = live_packet();
    let mut accepted = 0;
    let mut refused = 0;
    for _ in 0..60 {
        // No settle: the controller gives no buffers back meanwhile.
        if s.host.send_output_report(s.now, &report, &mut s.io) {
            accepted += 1;
        } else {
            refused += 1;
        }
    }
    assert!(accepted >= 16, "{accepted}");
    assert!(refused > 0);
    assert!(
        !s.io.logs.iter().any(|l| l.contains("data packets dropped")),
        "log:\n{}",
        s.tail_log()
    );
    s.settle();
    assert!(s.host.send_output_report(s.now, &report, &mut s.io));
}

// ------------------------------------- saved pairings (pad3: keys in flash)

/// A second and third made-up gamepad address.
const PAD_B: BdAddr = BdAddr([0x41, 0x42, 0x43, 0x44, 0x45, 0x46]);
const PAD_C: BdAddr = BdAddr([0x51, 0x52, 0x53, 0x54, 0x55, 0x56]);

fn ev_link_key_of(addr: BdAddr, key: [u8; 16]) -> Vec<u8> {
    evt(0x18, &cat(&[&addr.0, &key, &[0x04]]))
}

fn saved(addr: BdAddr, key: [u8; 16]) -> KeyChange {
    KeyChange::Save {
        addr,
        key: LinkKey(key),
        key_type: 0x04,
    }
}

/// The gamepad calls (we are searching) and the link comes up.
fn pad_calls(s: &mut Sim, addr: BdAddr) {
    s.feed(&ev_conn_request(addr, PAD_COD));
    s.take_cmd(op::ACCEPT_CONNECTION_REQUEST);
    s.feed(&ev_conn_complete(0, H, addr));
    s.advance(100);
}

/// After encryption on a gamepad's call, as in the live logs: the pad
/// opens HID control and interrupt, we read SDP, a report flows.
fn pad_opens_hid_and_reports(s: &mut Sim) {
    let (ctrl, result, _) = s.pad_opens(l2cap::PSM_HID_CONTROL, 0x0050);
    assert_eq!(result, conn_result::SUCCESS);
    s.pad_finishes_config(ctrl, 0x0050);
    let (intr, result, _) = s.pad_opens(l2cap::PSM_HID_INTERRUPT, 0x0051);
    assert_eq!(result, conn_result::SUCCESS);
    s.pad_finishes_config(intr, 0x0051);
    let sdp_ours = s.pad_accepts(l2cap::PSM_SDP, 0x0052);
    s.pad_serves_sdp(sdp_ours, 0x0052, &descriptor(60));
    assert_eq!(s.statuses().last().map(String::as_str), Some("Connected"));
    for k in 0..3 {
        s.pad_frame(H, intr, &[0xA1, 0x01, k], &[]);
        s.advance(20);
    }
    assert_eq!(s.reports().len(), 3);
}

/// The gamepad pairs on its own call (Secure Simple Pairing, "just
/// works"), with `key`, then hangs up. If we offer a key the pad no
/// longer has (it was put in pairing mode), its chip goes straight on to
/// pairing.
fn pad_calls_and_pairs(s: &mut Sim, addr: BdAddr, key: [u8; 16]) {
    pad_calls(s, addr);
    s.feed(&ev_addr(0x17, addr));
    if s.cmds.iter().any(|(o, _)| *o == op::LINK_KEY_REQUEST_REPLY) {
        s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    } else {
        s.take_cmd(op::LINK_KEY_REQUEST_NEGATIVE_REPLY);
    }
    s.feed(&ev_addr(0x31, addr));
    s.take_cmd(op::IO_CAPABILITY_REQUEST_REPLY);
    s.feed(&ev_io_cap_response(addr));
    s.feed(&ev_user_confirmation(addr));
    s.take_cmd(op::USER_CONFIRMATION_REQUEST_REPLY);
    s.feed(&ev_ssp_complete(addr));
    s.feed(&ev_link_key_of(addr, key));
    s.feed(&ev_encrypted(H));
    s.feed(&ev_disconnected(H, 0x13));
    s.advance(1_100);
    s.cmds.clear();
    s.sigs.clear();
    s.data.clear();
}

/// Analysis test 1. After a restart the saved key is loaded. The pad
/// calls and starts authentication itself: link-key-request comes early,
/// we answer with the saved key, and encryption comes on with NO
/// authentication-complete. Everything then works, and the summary says
/// the stored key was used.
#[test]
fn saved_key_after_restart_pad_calls_and_reports_flow() {
    let mut s = Sim::with_saved(Config::default(), &[(PAD, KEY)]);
    assert_eq!(s.host.key_count(), 1);
    s.advance(40);
    s.take_cmd(op::INQUIRY);
    pad_calls(&mut s, PAD);
    s.feed(&ev_addr(0x17, PAD));
    let r = s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    assert_eq!((&r[..6], &r[6..]), (&PAD.0[..], &KEY[..]), "the saved key");
    s.advance(200);
    s.feed(&ev_encrypted(H));
    pad_opens_hid_and_reports(&mut s);
    s.advance(2_000);
    assert!(
        !s.sent(op::AUTHENTICATION_REQUESTED),
        "the pad did the security itself"
    );
    assert!(!s.sent(op::IO_CAPABILITY_REQUEST_REPLY), "no new pairing");
    s.feed(&ev_disconnected(H, 0x13));
    let sums = summaries(&s);
    assert!(
        sums.iter().all(|l| l.contains(" pairing=stored-key ")),
        "{sums:?}"
    );
    assert!(sums.last().unwrap().contains("reached=reports"));
    assert!(s.io.changes.is_empty(), "nothing new to save");
    // A plain hang-up after a good session: no call back.
    s.cmds.clear();
    s.advance(1_100);
    s.take_cmd(op::INQUIRY);
    assert!(!s.sent(op::CREATE_CONNECTION));
    s.assert_key_never_printed();
    assert_eq!(s.host.stats().errors, 0);
}

/// Analysis test 2. The pad calls and we have no key: we say so, the pad
/// hangs up. Nothing is saved, and the search goes on.
#[test]
fn pad_calls_without_a_saved_key_and_hangs_up() {
    let mut s = Sim::new();
    s.advance(40);
    s.take_cmd(op::INQUIRY);
    pad_calls(&mut s, PAD);
    s.feed(&ev_addr(0x17, PAD));
    assert_eq!(s.take_cmd(op::LINK_KEY_REQUEST_NEGATIVE_REPLY), PAD.0);
    s.feed(&ev_disconnected(H, 0x13));
    assert_eq!(s.host.key_count(), 0);
    assert!(s.io.changes.is_empty());
    let sums = summaries(&s);
    let last = sums.last().unwrap();
    assert!(
        last.contains(" pairing=none ") && last.contains(" reached=linked "),
        "{last}"
    );
    s.cmds.clear();
    s.advance(1_100);
    s.take_cmd(op::INQUIRY);
    assert!(!s.sent(op::CREATE_CONNECTION));
}

/// The first part of analysis test 3: the pad calls, we offer the saved
/// key, the pad hangs up before encryption. The key is kept, and the next
/// step is our own call to that pad, not a search.
fn offered_key_then_hang_up(s: &mut Sim) {
    s.advance(40);
    s.take_cmd(op::INQUIRY);
    pad_calls(s, PAD);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    s.feed(&ev_disconnected(H, 0x13));
    assert!(
        summaries(s)
            .last()
            .unwrap()
            .contains(" pairing=stored-key-not-accepted "),
    );
    assert_eq!(s.host.key_count(), 1, "never erase on a plain hang-up");
    assert!(s.io.changes.is_empty());
    s.cmds.clear();
    let from = s.all_cmds.len();
    s.advance(1_100);
    let cc = s.take_cmd(op::CREATE_CONNECTION);
    assert_eq!(&cc[..6], &PAD.0, "we call the same pad back");
    assert!(
        !s.all_cmds[from..].contains(&op::INQUIRY),
        "no search before the call back"
    );
}

/// Analysis test 3, and the call back finds the key good.
#[test]
fn refused_saved_key_on_a_pad_call_is_kept_and_checked_by_calling_back() {
    let mut s = Sim::with_saved(Config::default(), &[(PAD, KEY)]);
    offered_key_then_hang_up(&mut s);
    s.feed(&ev_conn_complete(0, H, PAD));
    s.take_cmd(op::AUTHENTICATION_REQUESTED);
    s.feed(&ev_addr(0x17, PAD));
    assert_eq!(&s.take_cmd(op::LINK_KEY_REQUEST_REPLY)[6..], &KEY);
    s.feed(&ev_auth(0, H));
    s.take_cmd(op::SET_CONNECTION_ENCRYPTION);
    s.feed(&ev_encrypted(H));
    assert_eq!(s.host.key_count(), 1);
    assert!(s.io.changes.is_empty(), "the key was good: nothing changes");
    s.feed(&ev_disconnected(H, 0x13));
    assert!(
        summaries(&s)
            .last()
            .unwrap()
            .contains(" pairing=stored-key ")
    );
    s.assert_key_never_printed();
}

/// Analysis test 3 and 9 (the stale key, as the real pad does it): the
/// pad hangs up on our key, our call back gets "key missing", the key is
/// erased and the pad pairs again on that same link.
#[test]
fn stale_saved_key_is_erased_after_the_call_back_and_pairing_repeats() {
    let mut s = Sim::with_saved(Config::default(), &[(PAD, OLD_KEY)]);
    offered_key_then_hang_up(&mut s);
    s.feed(&ev_conn_complete(0, H, PAD));
    s.take_cmd(op::AUTHENTICATION_REQUESTED);
    s.feed(&ev_addr(0x17, PAD));
    assert_eq!(&s.take_cmd(op::LINK_KEY_REQUEST_REPLY)[6..], &OLD_KEY);
    s.feed(&ev_auth(0x06, H)); // "PIN or key missing"
    assert_eq!(
        s.io.changes,
        vec![KeyChange::Forget {
            addr: PAD,
            why: ForgetReason::AuthFailed
        }]
    );
    s.take_cmd(op::AUTHENTICATION_REQUESTED);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_NEGATIVE_REPLY);
    s.feed(&ev_addr(0x31, PAD));
    s.take_cmd(op::IO_CAPABILITY_REQUEST_REPLY);
    s.feed(&ev_user_confirmation(PAD));
    s.take_cmd(op::USER_CONFIRMATION_REQUEST_REPLY);
    s.feed(&ev_link_key_of(PAD, KEY));
    s.feed(&ev_auth(0, H));
    s.take_cmd(op::SET_CONNECTION_ENCRYPTION);
    s.feed(&ev_encrypted(H));
    assert_eq!(s.io.changes.last(), Some(&saved(PAD, KEY)));
    assert_eq!(s.host.key_count(), 1);
    s.assert_key_never_printed();
}

/// The call back happens once: if the pad does not answer it and then
/// calls and hangs up again, we go back to searching.
#[test]
fn the_call_back_happens_only_once() {
    let mut s = Sim::with_saved(Config::default(), &[(PAD, KEY)]);
    offered_key_then_hang_up(&mut s);
    s.advance(5_120);
    s.feed(&ev_conn_complete(0x04, 0, PAD)); // page timeout: switched off
    s.cmds.clear();
    s.advance(1_600);
    s.take_cmd(op::INQUIRY);
    pad_calls(&mut s, PAD);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    s.feed(&ev_disconnected(H, 0x13));
    s.cmds.clear();
    s.advance(1_100);
    s.take_cmd(op::INQUIRY);
    assert!(!s.sent_after_inquiry(op::CREATE_CONNECTION));
    assert_eq!(s.host.key_count(), 1, "still kept");
}

impl Sim {
    /// True if `opcode` was sent after the last INQUIRY.
    fn sent_after_inquiry(&self, opcode: u16) -> bool {
        let from = self
            .all_cmds
            .iter()
            .rposition(|o| *o == op::INQUIRY)
            .unwrap_or(0);
        self.all_cmds[from..].contains(&opcode)
    }
}

/// Analysis test 4. We call with a saved key, and the pad's chip goes
/// straight to pairing ("key missing" without a failure event): the new
/// key replaces the old one, reported once.
#[test]
fn our_call_with_a_saved_key_that_the_pad_lost_pairs_anew() {
    let mut s = Sim::with_saved(Config::default(), &[(PAD, OLD_KEY)]);
    find_and_call_pad(&mut s);
    s.feed(&ev_conn_complete(0, H, PAD));
    s.take_cmd(op::AUTHENTICATION_REQUESTED);
    s.feed(&ev_addr(0x17, PAD));
    assert_eq!(&s.take_cmd(op::LINK_KEY_REQUEST_REPLY)[6..], &OLD_KEY);
    s.feed(&ev_addr(0x31, PAD));
    s.take_cmd(op::IO_CAPABILITY_REQUEST_REPLY);
    s.feed(&ev_io_cap_response(PAD));
    s.feed(&ev_user_confirmation(PAD));
    s.take_cmd(op::USER_CONFIRMATION_REQUEST_REPLY);
    s.feed(&ev_ssp_complete(PAD));
    s.feed(&ev_link_key_of(PAD, KEY));
    s.feed(&ev_auth(0, H));
    s.take_cmd(op::SET_CONNECTION_ENCRYPTION);
    s.feed(&ev_encrypted(H));
    assert_eq!(s.io.changes, vec![saved(PAD, KEY)]);
    assert_eq!(s.host.key_count(), 1);
    s.feed(&ev_disconnected(H, 0x13));
    assert!(summaries(&s).last().unwrap().contains(" pairing=new "));
    s.assert_key_never_printed();
}

/// Analysis test 5. We call, offer the key, authentication fails with
/// 0x05: the key is erased (reported) and pairing repeats on the link.
#[test]
fn our_call_with_a_refused_key_erases_it_and_pairs_again() {
    let mut s = Sim::with_saved(Config::default(), &[(PAD, OLD_KEY)]);
    find_and_call_pad(&mut s);
    s.feed(&ev_conn_complete(0, H, PAD));
    s.take_cmd(op::AUTHENTICATION_REQUESTED);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    s.feed(&ev_auth(0x05, H));
    s.take_cmd(op::AUTHENTICATION_REQUESTED);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_NEGATIVE_REPLY);
    s.feed(&ev_addr(0x31, PAD));
    s.take_cmd(op::IO_CAPABILITY_REQUEST_REPLY);
    s.feed(&ev_user_confirmation(PAD));
    s.take_cmd(op::USER_CONFIRMATION_REQUEST_REPLY);
    s.feed(&ev_link_key_of(PAD, KEY));
    s.feed(&ev_auth(0, H));
    assert_eq!(
        s.io.changes,
        vec![
            KeyChange::Forget {
                addr: PAD,
                why: ForgetReason::AuthFailed
            },
            saved(PAD, KEY)
        ]
    );
    s.assert_key_never_printed();
}

/// Analysis test 6. Two pad modes at the same address share one place;
/// other addresses get their own; a fifth address pushes out the oldest,
/// and the caller hears about it before the new one is saved.
#[test]
fn one_place_per_address_and_the_oldest_makes_room() {
    let mut s = Sim::new();
    s.advance(40);
    s.take_cmd(op::INQUIRY);
    pad_calls_and_pairs(&mut s, PAD, OLD_KEY); // one mode
    pad_calls_and_pairs(&mut s, PAD, KEY); // another mode, same address
    assert_eq!(s.host.key_count(), 1);
    pad_calls_and_pairs(&mut s, PAD_B, KEY);
    pad_calls_and_pairs(&mut s, PAD_C, KEY);
    pad_calls_and_pairs(&mut s, OTHER, KEY);
    assert_eq!(s.host.key_count(), 4);
    assert!(
        !s.io
            .changes
            .iter()
            .any(|c| matches!(c, KeyChange::Forget { .. }))
    );
    pad_calls_and_pairs(&mut s, PHONE, KEY);
    assert_eq!(s.host.key_count(), 4);
    let n = s.io.changes.len();
    assert_eq!(
        s.io.changes[n - 2..],
        [
            KeyChange::Forget {
                addr: PAD,
                why: ForgetReason::Evicted
            },
            saved(PHONE, KEY)
        ]
    );
    // The oldest is really gone: its call gets "no key".
    pad_calls(&mut s, PAD);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_NEGATIVE_REPLY);
    s.assert_key_never_printed();
}

/// Analysis test 8. A key that comes on an SDP-only probe link is never
/// kept or handed to the store.
#[test]
fn a_key_on_a_probe_link_never_reaches_the_store() {
    let mut s = Sim::new();
    s.host.request_page(
        PHONE,
        Purpose::SdpProbe {
            uuid: 0x0100,
            max_bytes: 48,
        },
    );
    s.advance(40);
    s.take_cmd(op::CREATE_CONNECTION);
    s.feed(&ev_conn_complete(0, H, PHONE));
    s.feed(&ev_link_key(PHONE));
    assert!(s.io.changes.is_empty());
    assert_eq!(s.host.key_count(), 0);
    s.assert_key_never_printed();
}

/// Loading saved keys: oldest first, at most four, and the newest load of
/// one address wins.
#[test]
fn loading_saved_keys_keeps_the_newest_four() {
    let mut s = Sim::with_saved(
        Config::default(),
        &[
            (PAD, OLD_KEY),
            (PAD_B, KEY),
            (PAD_C, KEY),
            (OTHER, KEY),
            (PHONE, KEY),
            (PAD_C, OLD_KEY),
        ],
    );
    assert_eq!(s.host.key_count(), 4);
    assert!(s.io.changes.is_empty(), "loading reports nothing");
    s.advance(40);
    pad_calls(&mut s, PAD);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_NEGATIVE_REPLY);
    s.feed(&ev_disconnected(H, 0x13));
    s.advance(1_100);
    pad_calls(&mut s, PAD_C);
    s.feed(&ev_addr(0x17, PAD_C));
    assert_eq!(&s.take_cmd(op::LINK_KEY_REQUEST_REPLY)[6..], &OLD_KEY);
    s.assert_key_never_printed();
}

/// "Forget all pairings" from the user: the keys go, a connected pad is
/// let go, and its next call gets "no key".
#[test]
fn forget_all_lets_the_pad_go_and_drops_every_key() {
    let mut s = Sim::with_saved(Config::default(), &[(PAD, KEY), (PAD_B, KEY)]);
    s.advance(40);
    pad_calls(&mut s, PAD);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    s.feed(&ev_encrypted(H));
    let n = s.host.forget_all_keys(&mut s.io);
    s.advance(20);
    assert_eq!(n, 2);
    assert_eq!(s.host.key_count(), 0);
    assert_eq!(s.take_cmd(op::DISCONNECT), [0x0B, 0x00, 0x13]);
    assert!(
        s.io.changes.is_empty(),
        "the caller asked; it erases itself"
    );
    s.feed(&ev_disconnected(H, 0x16));
    s.advance(1_100);
    pad_calls(&mut s, PAD);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_NEGATIVE_REPLY);
    assert!(
        s.io.logs
            .iter()
            .any(|l| l.contains("forgot all pairings: 2")),
        "{}",
        s.tail_log()
    );
}

/// Second reader, pad3: events that carry a link key but are cut short
/// (or are of a kind we do not read, like "return link keys", 0x15) fall
/// through to the "unknown event" line. That line must not print their
/// bytes: they hold the key. Here the link-key event lacks its last byte
/// (the key type), so it cannot be read as a link-key event.
#[test]
fn key_bearing_events_we_cannot_read_never_print_their_bytes() {
    let mut s = Sim::new();
    // Link key notification, 22 bytes instead of 23.
    s.feed(&evt(0x18, &cat(&[&PAD.0, &KEY])));
    // Return link keys: one entry (address + key).
    s.feed(&evt(0x15, &cat(&[&[1], &PAD.0, &OLD_KEY])));
    assert!(
        s.io.logs.iter().any(|l| l.contains("code=0x18")),
        "the event is still noted: {}",
        s.tail_log()
    );
    assert!(s.io.logs.iter().any(|l| l.contains("code=0x15")));
    assert!(s.io.changes.is_empty(), "nothing to save from a cut event");
    s.assert_key_never_printed();
}

// ------------------------------------------------- reconnect-only (round 6)

fn reconnect_only() -> Config {
    Config {
        reconnect_only: true,
        ..Config::default()
    }
}

/// The opcodes that must never leave in reconnect-only mode.
const NEVER_IN_RECONNECT_ONLY: [u16; 5] = [
    op::INQUIRY,
    op::CREATE_CONNECTION,
    op::IO_CAPABILITY_REQUEST_REPLY,
    op::USER_CONFIRMATION_REQUEST_REPLY,
    op::PIN_CODE_REQUEST_REPLY,
];

fn assert_nothing_forbidden(s: &Sim) {
    for o in NEVER_IN_RECONNECT_ONLY {
        assert!(!s.sent(o), "{} sent in reconnect-only mode", op::name(o));
    }
    assert!(
        s.io.changes.is_empty(),
        "key table changed: {:?}",
        s.io.changes
    );
}

/// The high four bytes of an address, in every way a log could spell
/// them. The last two bytes may show; these must not.
fn assert_no_full_address(s: &Sim, a: BdAddr) {
    let hi = &a.0[2..6];
    let fwd: String = hi.iter().map(|b| format!("{b:02x}")).collect();
    let back: String = hi.iter().rev().map(|b| format!("{b:02x}")).collect();
    let colon: String = hi
        .iter()
        .rev()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":");
    for l in &s.io.logs {
        let low = l.to_lowercase();
        assert!(
            !low.contains(&fwd) && !low.contains(&back) && !l.contains(&colon),
            "full address in the log: {l}"
        );
    }
}

/// Their bytes shown as hex (an SDP request or a stray PDU on the SDP
/// channel the pad opened, L2CAP configure options, information data)
/// never show the pad's address, in either byte order. Found by
/// boards/cyd/hci-rules/tests/hostile_radio.rs: a Switch device-info
/// reply on the SDP channel was logged whole. Fails with plain hex.
#[test]
fn hex_of_their_bytes_hides_the_pad_address() {
    let mut s = Sim::with_saved(reconnect_only(), &[(PAD, KEY)]);
    s.advance(2_000);
    pad_calls(&mut s, PAD);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    s.feed(&ev_encrypted(H));
    let fwd = PAD.0;
    let mut back = PAD.0;
    back.reverse();
    let (sdp, result, _) = s.pad_opens(l2cap::PSM_SDP, 0x0060);
    assert_eq!(result, conn_result::SUCCESS);
    // A stray PDU (an answer nobody asked for), then a request.
    s.pad_frame(H, sdp, &cat(&[&[0x03, 0, 1, 0, 12], &fwd, &back]), &[]);
    s.pad_frame(H, sdp, &cat(&[&[0x02, 0, 2, 0, 12], &back, &fwd]), &[]);
    let id = s.next_pad_id();
    s.pad_sig(
        code::CONFIGURE_REQUEST,
        id,
        &cat(&[&sdp.to_le_bytes(), &[0, 0], &fwd]),
    );
    let id = s.next_pad_id();
    s.pad_sig(
        code::INFORMATION_RESPONSE,
        id,
        &cat(&[&[2, 0, 0, 0], &back]),
    );
    for what in [
        "unexpected pdu",
        "asks us",
        "configure-request",
        "information-response",
    ] {
        assert!(
            s.io.logs.iter().any(|l| l.contains(what)),
            "no {what} line\n{}",
            s.tail_log()
        );
    }
    assert_no_full_address(&s, PAD);
}

/// Reconnect-only: no inquiry ever, no call of our own (a requested
/// page is ignored), "waiting" after start-up. A stranger's call is
/// refused with "unacceptable address" and its address is not logged.
#[test]
fn reconnect_only_never_searches_or_calls_and_refuses_strangers() {
    let mut s = Sim::with_saved(reconnect_only(), &[(PAD, KEY)]);
    s.host.request_page(FAKE, Purpose::Gamepad);
    s.advance(30_000);
    assert!(s.host.waiting(), "phase {}", s.host.phase_name());
    assert_eq!(s.host.phase_name(), "waiting");
    assert!(s.statuses().iter().any(|t| t == "Waiting for a saved pad"));
    // A peripheral without a saved key (a stranger's gamepad), a phone.
    s.feed(&ev_conn_request(OTHER, PAD_COD));
    assert_eq!(
        s.take_cmd(op::REJECT_CONNECTION_REQUEST),
        cat(&[&OTHER.0, &[0x0F]])
    );
    s.feed(&ev_conn_request(PHONE, PHONE_COD));
    assert_eq!(
        s.take_cmd(op::REJECT_CONNECTION_REQUEST),
        cat(&[&PHONE.0, &[0x0F]])
    );
    assert!(!s.sent(op::ACCEPT_CONNECTION_REQUEST));
    assert!(
        s.io.logs
            .iter()
            .any(|l| l.contains("refused: no saved key")),
        "{}",
        s.tail_log()
    );
    s.advance(5_000);
    assert!(s.host.waiting());
    assert_nothing_forbidden(&s);
    for a in [OTHER, PHONE, FAKE, PAD] {
        assert_no_full_address(&s, a);
    }
}

/// Reconnect-only: a saved pad calls, but then starts a new pairing
/// (it lost our key). The pairing is refused (IO capability negative
/// reply, reason 0x18 "pairing not allowed"), the link is ended, the
/// saved key stays, and the host is back to "waiting".
#[test]
fn reconnect_only_refuses_pairing_and_keeps_the_key() {
    let mut s = Sim::with_saved(reconnect_only(), &[(PAD, KEY)]);
    s.advance(2_000);
    pad_calls(&mut s, PAD);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    s.feed(&ev_addr(0x31, PAD));
    assert_eq!(
        s.take_cmd(op::IO_CAPABILITY_REQUEST_NEGATIVE_REPLY),
        cat(&[&PAD.0, &[0x18]])
    );
    assert_eq!(s.take_cmd(op::DISCONNECT), [0x0B, 0x00, 0x05]);
    // A user confirmation or a new key that comes anyway changes nothing.
    s.feed(&ev_user_confirmation(PAD));
    s.take_cmd(op::USER_CONFIRMATION_REQUEST_NEGATIVE_REPLY);
    s.feed(&ev_link_key(PAD));
    s.feed(&evt(0x16, &PAD.0)); // PIN code request
    s.take_cmd(op::PIN_CODE_REQUEST_NEGATIVE_REPLY);
    s.feed(&ev_disconnected(H, 0x16));
    assert_eq!(s.host.key_count(), 1, "the saved key stays");
    s.advance(1_100);
    assert!(s.host.waiting());
    assert_nothing_forbidden(&s);
    s.assert_key_never_printed();
    assert_no_full_address(&s, PAD);
}

/// Reconnect-only: the pad refuses our saved key (authentication failure
/// 0x05). No second try and no new pairing: the link ends, the key
/// stays, the host waits for the next call.
#[test]
fn reconnect_only_auth_failure_keeps_the_key_and_waits() {
    let mut s = Sim::with_saved(reconnect_only(), &[(PAD, KEY)]);
    s.advance(2_000);
    pad_calls(&mut s, PAD);
    s.advance(600);
    s.take_cmd(op::AUTHENTICATION_REQUESTED);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    s.feed(&ev_auth(0x05, H));
    assert_eq!(s.take_cmd(op::DISCONNECT), [0x0B, 0x00, 0x05]);
    assert!(
        !s.cmds
            .iter()
            .any(|(o, _)| *o == op::AUTHENTICATION_REQUESTED),
        "no second try"
    );
    s.feed(&ev_disconnected(H, 0x05));
    assert_eq!(s.host.key_count(), 1);
    s.advance(1_100);
    assert!(s.host.waiting());
    // The next call is taken again.
    s.cmds.clear();
    pad_calls(&mut s, PAD);
    assert!(s.host.linked());
    assert_nothing_forbidden(&s);
}

/// Reconnect-only: the pad called with its saved key and hung up before
/// encryption (as in the live log of round 5). Other modes call it back
/// once; this one never calls: it waits for the pad's next call.
#[test]
fn reconnect_only_never_calls_back() {
    let mut s = Sim::with_saved(reconnect_only(), &[(PAD, KEY)]);
    s.advance(2_000);
    pad_calls(&mut s, PAD);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    s.feed(&ev_disconnected(H, 0x13));
    s.advance(10_000);
    assert!(s.host.waiting());
    assert_nothing_forbidden(&s);
    assert_eq!(s.host.key_count(), 1);
}

/// A virtual cable unplug in reconnect-only mode ends the link but keeps
/// the key (the table is read-only there).
#[test]
fn reconnect_only_unplug_keeps_the_key() {
    let mut s = Sim::with_saved(reconnect_only(), &[(PAD, KEY)]);
    s.advance(2_000);
    pad_calls(&mut s, PAD);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    s.feed(&ev_encrypted(H));
    let (ctrl, _, _) = s.pad_opens(l2cap::PSM_HID_CONTROL, 0x0050);
    s.pad_finishes_config(ctrl, 0x0050);
    s.pad_frame(H, ctrl, &[0x15], &[]);
    assert_eq!(s.take_cmd(op::DISCONNECT), [0x0B, 0x00, 0x13]);
    assert_eq!(s.host.key_count(), 1);
    assert_nothing_forbidden(&s);
}

/// The controller never confirms our Disconnect (no Disconnection
/// Complete). The host must not wait for it for ever: after a few
/// seconds it drops the link itself and waits for the next call, which
/// is accepted. Fails without the guard (the host stays linked).
#[test]
fn a_disconnect_never_confirmed_is_dropped_after_a_while() {
    let mut s = Sim::with_saved(reconnect_only(), &[(PAD, KEY)]);
    s.advance(2_000);
    pad_calls(&mut s, PAD);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    s.feed(&ev_addr(0x31, PAD));
    assert_eq!(s.take_cmd(op::DISCONNECT), [0x0B, 0x00, 0x05]);
    s.advance(4_000);
    assert!(s.host.linked(), "still waiting for the controller");
    s.advance(2_000);
    assert!(!s.host.linked(), "{}", s.tail_log());
    assert!(
        s.io.logs
            .iter()
            .any(|l| l.contains("no disconnection-complete"))
    );
    s.advance(1_100);
    assert!(s.host.waiting(), "phase {}", s.host.phase_name());
    // A late confirmation changes nothing; the next call is taken.
    s.feed(&ev_disconnected(H, 0x16));
    assert!(s.host.waiting());
    s.cmds.clear();
    pad_calls(&mut s, PAD);
    assert!(s.host.linked());
    assert_nothing_forbidden(&s);
}

/// Every mode: logs show at most the last two bytes of any address,
/// the pad's, the console's own (READ_BD_ADDR) and a stranger's, in
/// commands, their answers and events. Fails with the old full-address
/// printing.
#[test]
fn logs_never_show_a_whole_address() {
    let mut s = Sim::new();
    pair_outgoing(&mut s);
    sdp_and_hid_outgoing(&mut s, &descriptor(60));
    s.feed(&evt(0x77, &cat(&[&PAD.0, &[1]])));
    // A stranger with bytes that appear nowhere else in this test.
    let stranger = BdAddr([0x9A, 0xE7, 0x5C, 0xB1, 0xD3, 0x4F]);
    s.feed(&ev_conn_request(stranger, PAD_COD));
    s.feed(&ev_disconnected(H, 0x13));
    s.advance(2_000);
    let own = BdAddr([0xFF, 0xEE, 0xDD, 0xCC, 0xBB, 0xAA]);
    for a in [PAD, stranger, own] {
        assert_no_full_address(&s, a);
    }
    // The last two bytes still tell devices apart.
    assert!(s.io.logs.iter().any(|l| l.contains("xx:xx:xx:xx:02:01")));
}

// ------------------------------------------------- a pad switched off (round 6)

fn ev_role(status: u8, addr: BdAddr, role: u8) -> Vec<u8> {
    evt(0x12, &cat(&[&[status], &addr.0, &[role]]))
}

fn lsto_cmds(s: &Sim) -> usize {
    s.all_cmds
        .iter()
        .filter(|o| **o == op::WRITE_LINK_SUPERVISION_TIMEOUT)
        .count()
}

/// The supervision timeout: the pad's SDP wish kept between 1 and 5 s,
/// 2 s without one, and always at least three sniff intervals.
#[test]
fn supervision_slots_keeps_the_wish_in_range_and_above_sniff() {
    use pad_proto::host::{LSTO_DEFAULT, LSTO_MAX, LSTO_MIN, supervision_slots};
    assert_eq!((LSTO_MIN, LSTO_DEFAULT, LSTO_MAX), (1600, 3200, 8000));
    assert_eq!(supervision_slots(None, 0), LSTO_DEFAULT);
    assert_eq!(supervision_slots(Some(0), 0), LSTO_DEFAULT);
    assert_eq!(supervision_slots(Some(0x0C80), 0), 0x0C80);
    assert_eq!(supervision_slots(Some(100), 0), LSTO_MIN);
    assert_eq!(supervision_slots(Some(0x7D00), 0), LSTO_MAX);
    assert_eq!(supervision_slots(Some(2000), 1000), 3000);
    assert_eq!(supervision_slots(Some(0x7D00), 0x8000), 0xFFFF);
}

/// Reconnect-only: once the role change makes us central, the timeout
/// goes to the controller for our handle, once; it is never sent while
/// we are the peripheral.
#[test]
fn the_host_sets_the_supervision_timeout_once_it_is_central() {
    let mut s = Sim::with_saved(reconnect_only(), &[(PAD, KEY)]);
    s.advance(2_000);
    pad_calls(&mut s, PAD);
    assert!(!s.sent(op::WRITE_LINK_SUPERVISION_TIMEOUT));
    s.feed(&ev_role(0, PAD, 1));
    assert!(!s.sent(op::WRITE_LINK_SUPERVISION_TIMEOUT));
    s.feed(&ev_role(0, PAD, 0));
    assert_eq!(
        s.take_cmd(op::WRITE_LINK_SUPERVISION_TIMEOUT),
        [0x0B, 0x00, 0x80, 0x0C]
    );
    s.feed(&ev_role(0, PAD, 0));
    s.advance(100);
    assert_eq!(lsto_cmds(&s), 1);
    assert_nothing_forbidden(&s);
}

/// The supervision timeout waits behind a command still unanswered and
/// the link ends meanwhile: it is not sent for the handle that is gone
/// (the board's gate refuses such a handle and nothing would answer it,
/// holding every later command for the 3 s time-out). With the link
/// still up it goes out once the other command is answered. Found by
/// boards/cyd/hci-rules/tests/hostile_radio.rs.
#[test]
fn a_queued_supervision_timeout_is_not_sent_after_its_link_ended() {
    for ends in [false, true] {
        let mut s = Sim::with_saved(reconnect_only(), &[(PAD, KEY)]);
        s.advance(2_000);
        pad_calls(&mut s, PAD);
        let now = s.now;
        // Unsettled: the key reply waits for its answer, the timeout
        // queues behind it.
        s.host.on_packet(now, &ev_addr(0x17, PAD), &mut s.io);
        s.host.on_packet(now, &ev_role(0, PAD, 0), &mut s.io);
        if ends {
            s.host.on_packet(now, &ev_disconnected(H, 0x08), &mut s.io);
        }
        s.settle();
        s.advance(100);
        assert_eq!(lsto_cmds(&s), usize::from(!ends), "ends {ends}");
        if ends {
            assert!(!s.host.linked());
            // The next call is answered at once, not after a time-out.
            s.cmds.clear();
            s.feed(&ev_conn_request(PAD, PAD_COD));
            assert_eq!(&s.take_cmd(op::ACCEPT_CONNECTION_REQUEST)[..6], &PAD.0);
        }
        assert_nothing_forbidden(&s);
    }
}

/// The caller drops the link (the pad went silent): a Disconnect goes
/// out, the UI hears the pad gone, the host waits for the next call at
/// once. The old link's late data and its late Disconnection Complete
/// change nothing and raise no error.
#[test]
fn a_dropped_link_waits_at_once_and_its_late_end_is_quiet() {
    let mut s = Sim::with_saved(reconnect_only(), &[(PAD, KEY)]);
    s.advance(2_000);
    pad_calls(&mut s, PAD);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    s.feed(&ev_encrypted(H));
    s.io.ui.clear();
    let now = s.now;
    assert!(s.host.drop_link(now, 0x08, &mut s.io));
    s.settle();
    assert_eq!(s.take_cmd(op::DISCONNECT), [0x0B, 0x00, 0x13]);
    assert!(s.io.ui.contains(&Ui::Disconnected));
    assert!(s.host.waiting(), "phase {}", s.host.phase_name());
    assert!(!s.host.linked());
    let errs = |s: &Sim| {
        s.io.logs
            .iter()
            .filter(|l| l.starts_with("PAD-ERR"))
            .count()
    };
    let before = errs(&s);
    s.pad_frame(H, 0x0041, &[0xA1, 0x30], &[]);
    s.feed(&ev_disconnected(H, 0x16));
    s.advance(20_000);
    assert_eq!(errs(&s), before, "{}", s.tail_log());
    assert!(
        s.io.logs
            .iter()
            .any(|l| l.contains("old link closed by the controller"))
    );
    assert!(
        !s.host.drop_link(s.now, 0x08, &mut Rec::default()),
        "no link"
    );
    pad_calls(&mut s, PAD);
    assert!(s.host.linked());
    assert_nothing_forbidden(&s);
}

/// The pad we are linked to calls again: the old link is ended and the
/// new call is accepted, never refused as "busy".
#[test]
fn the_linked_pad_calling_again_is_accepted() {
    let mut s = Sim::with_saved(reconnect_only(), &[(PAD, KEY)]);
    s.advance(2_000);
    pad_calls(&mut s, PAD);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    s.feed(&ev_encrypted(H));
    s.cmds.clear();
    s.feed(&ev_conn_request(PAD, PAD_COD));
    assert_eq!(s.take_cmd(op::DISCONNECT), [0x0B, 0x00, 0x13]);
    assert_eq!(&s.take_cmd(op::ACCEPT_CONNECTION_REQUEST)[..6], &PAD.0);
    assert!(!s.sent(op::REJECT_CONNECTION_REQUEST));
    // A stranger's call while linked is still refused.
    let mut s = Sim::with_saved(reconnect_only(), &[(PAD, KEY)]);
    s.advance(2_000);
    pad_calls(&mut s, PAD);
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    s.feed(&ev_encrypted(H));
    s.cmds.clear();
    s.feed(&ev_conn_request(PHONE, PAD_COD));
    assert!(!s.sent(op::DISCONNECT));
    assert!(
        s.cmds
            .iter()
            .all(|(o, _)| *o != op::ACCEPT_CONNECTION_REQUEST)
    );
    assert!(
        s.cmds
            .iter()
            .any(|(o, _)| *o == op::REJECT_CONNECTION_REQUEST)
    );
    assert!(s.host.linked());
}
