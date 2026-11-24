//! Hostile input: random and mutated HCI events, ACL data, L2CAP
//! signalling, SDP answers and HID reports, fed to the whole pad host
//! (pad-proto, pad-view and the mapping) with checks after every packet.
//!
//! Checks: no panic (debug and release with overflow checks), every
//! packet we send is well formed and at most `TX_MAX` bytes, the work per
//! packet is bounded (packets sent, log lines), nothing forbidden goes out
//! (no search, no call, no pairing step, accept and key only for the
//! saved pad), the game's view stays in range, the key never shows, and
//! at the end, once every link is gone, the host waits again.
//!
//! `PAD_FUZZ=N` runs N times as many cases (for long runs:
//! `PAD_FUZZ=200 cargo test --release --test fuzz`). `PAD_FUZZ_SEED`
//! picks another start.

mod common;

use std::collections::BTreeSet;
use std::fmt;

use board_api::{PadState, btn};
use common::fixtures;
use common::*;
use pad_host::{PadEvent, PadHost, Radio, TX_MAX};
use pad_proto::acl;
use pad_proto::hci::{self, op};

// ------------------------------------------------------------- random

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
    fn byte(&mut self) -> u8 {
        self.next() as u8
    }
    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.byte()).collect()
    }
    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len())]
    }
}

fn scale() -> usize {
    std::env::var("PAD_FUZZ")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
}

fn seed() -> u64 {
    std::env::var("PAD_FUZZ_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
}

/// One of the values, chosen at random (each may itself use `r`).
macro_rules! pick {
    ($r:expr, $($e:expr),+ $(,)?) => {{
        let xs = [$($e),+];
        let i = $r.below(xs.len());
        xs[i]
    }};
}

// ------------------------------------------------------- the checked run

/// The most packets and log lines one call may give (measured: see the
/// test output; these are generous bounds, not targets).
const MAX_SENT_PER_CALL: usize = 24;
const MAX_LOGS_PER_CALL: usize = 64;

#[derive(Default)]
struct Out {
    sent: Vec<Vec<u8>>,
    logs: usize,
    last_logs: Vec<String>,
}

impl Radio for Out {
    fn send(&mut self, p: &[u8]) {
        self.sent.push(p.to_vec());
    }
    fn log(&mut self, tag: &'static str, args: fmt::Arguments<'_>) {
        let line = format!("{tag} {args}");
        assert_no_key(&line);
        self.logs += 1;
        if self.last_logs.len() >= 40 {
            self.last_logs.remove(0);
        }
        self.last_logs.push(line);
    }
}

/// A pad host with a checker around it, and (optionally) a controller
/// that answers every command.
struct Run {
    host: PadHost,
    out: Out,
    now: u64,
    answer: bool,
    handles: BTreeSet<u16>,
    most_sent: usize,
    most_logs: usize,
    calls: u64,
}

impl Run {
    fn new() -> Self {
        let ram = Ram::with(&[(PAD, KEY)]);
        let mut r = Run {
            host: PadHost::new(),
            out: Out::default(),
            now: 1_000,
            answer: false,
            handles: BTreeSet::new(),
            most_sent: 0,
            most_logs: 0,
            calls: 0,
        };
        let mut view = ReadOnly {
            ram: &ram,
            reads: 0,
        };
        r.host
            .load_pairings(&mut view, region(), &mut r.out)
            .unwrap();
        r.host.start(r.now, &mut r.out);
        r.check_call();
        r
    }

    fn context(&self) -> String {
        self.out.last_logs.join("\n")
    }

    fn check_call(&mut self) {
        self.calls += 1;
        let sent = std::mem::take(&mut self.out.sent);
        let logs = std::mem::take(&mut self.out.logs);
        self.most_sent = self.most_sent.max(sent.len());
        self.most_logs = self.most_logs.max(logs);
        assert!(
            sent.len() <= MAX_SENT_PER_CALL,
            "{} packets from one call\n{}",
            sent.len(),
            self.context()
        );
        assert!(
            logs <= MAX_LOGS_PER_CALL,
            "{logs} log lines from one call\n{}",
            self.context()
        );
        for p in &sent {
            check_sent(p);
        }
        self.check_view();
        if self.answer {
            for p in &sent {
                if p[0] == 0x01 {
                    let opcode = u16::from_le_bytes([p[1], p[2]]);
                    self.packet_unchecked(&answer(opcode, &p[4..]));
                }
            }
        }
    }

    fn check_view(&mut self) {
        let st = self.host.read();
        if !self.host.connected() {
            assert_eq!(st, PadState::NONE);
            assert_eq!(self.host.motion(), None);
            assert_eq!(self.host.battery(), None);
        }
        assert_eq!(st.buttons & !btn::ALL, 0);
        for v in [st.left.x, st.left.y, st.right.x, st.right.y] {
            assert!(v > i16::MIN, "{st:?}");
        }
        assert!(st.lt <= 1023 && st.rt <= 1023, "{st:?}");
        if let Some(b) = self.host.battery() {
            assert!(b.percent <= 100, "{b:?}");
        }
        let mut n = 0;
        while let Some(e) = self.host.take_event() {
            n += 1;
            let t = format!("{e:?}");
            assert_no_key(&t);
            if let PadEvent::Connected { name, .. } = e {
                assert!(name.as_str().len() <= 32);
            }
        }
        assert!(n <= 4, "{n} events at once");
        let _ = self.host.take_pressed();
    }

    fn packet_unchecked(&mut self, p: &[u8]) {
        self.note_handles(p);
        self.host.packet(self.now, p, &mut self.out);
        self.check_call();
    }

    fn packet(&mut self, p: &[u8]) {
        self.packet_unchecked(p);
    }

    fn tick(&mut self) {
        let _ = self.host.tick(self.now, &mut self.out);
        self.check_call();
    }

    /// Time moves on to `t`, with a tick every 10 ms.
    fn until(&mut self, t: u64) {
        while self.now + 10 <= t {
            self.now += 10;
            self.tick();
        }
        self.now = self.now.max(t);
    }

    fn note_handles(&mut self, p: &[u8]) {
        if p.first() == Some(&0x04)
            && let Some(hci::Event::ConnectionComplete { handle, .. }) = hci::parse_event(&p[1..])
        {
            self.handles.insert(handle);
        }
    }

    /// Every link the controller ever announced goes, then a responsive
    /// controller and two minutes: the host must wait for a call again.
    fn finish(&mut self) {
        self.answer = true;
        for h in self.handles.clone() {
            self.packet(&ev_disconnected(h, 0x13));
        }
        let end = self.now + 130_000;
        self.until(end);
        assert!(
            self.host.waiting(),
            "phase {} after the end\n{}",
            self.host.phase(),
            self.context()
        );
        assert!(!self.host.connected());
        assert_eq!(self.host.read(), PadState::NONE);
        if self.host.phase() == "init" {
            INIT_ENDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// Cases that ended with the radio start-up never finished (a broken
/// controller answer during start-up). Printed, for the report.
static INIT_ENDS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// One packet we sent: well formed, small enough, allowed.
fn check_sent(p: &[u8]) {
    assert!(!p.is_empty() && p.len() <= TX_MAX, "len {}", p.len());
    match p[0] {
        0x01 => {
            assert!(p.len() >= 4);
            assert_eq!(usize::from(p[3]), p.len() - 4, "command length byte");
            let opcode = u16::from_le_bytes([p[1], p[2]]);
            let params = &p[4..];
            assert!(
                ![
                    op::INQUIRY,
                    op::CREATE_CONNECTION,
                    op::IO_CAPABILITY_REQUEST_REPLY,
                    op::USER_CONFIRMATION_REQUEST_REPLY,
                    op::PIN_CODE_REQUEST_REPLY,
                ]
                .contains(&opcode),
                "{} sent",
                op::name(opcode)
            );
            match opcode {
                op::ACCEPT_CONNECTION_REQUEST | op::LINK_KEY_REQUEST_REPLY => {
                    assert_eq!(
                        &params[..6],
                        &PAD.0,
                        "{} for another address",
                        op::name(opcode)
                    );
                }
                op::WRITE_SCAN_ENABLE => assert_eq!(params[0] & 1, 0, "inquiry scan on"),
                _ => {}
            }
        }
        0x02 => {
            let (h, d) = acl::parse(&p[1..]).expect("acl header");
            assert_eq!(usize::from(h.len), d.len());
            assert_eq!(p.len(), 5 + d.len(), "acl length");
        }
        k => panic!("packet kind {k:#04x}"),
    }
}

/// The controller's answer to a command (status 0).
fn answer(opcode: u16, params: &[u8]) -> Vec<u8> {
    let o = opcode.to_le_bytes();
    let status_only = [
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
    if status_only.contains(&opcode) {
        return evt(0x0F, &[0, 1, o[0], o[1]]);
    }
    let mut e = vec![1, o[0], o[1], 0];
    match opcode {
        op::READ_BUFFER_SIZE => e.extend_from_slice(&[0xFD, 0x03, 0xFF, 9, 0, 4, 0]),
        op::READ_BD_ADDR => e.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]),
        _ => e.extend_from_slice(params.get(..6).unwrap_or(&[])),
    }
    evt(0x0E, &e)
}

// --------------------------------------------------------- random input

const CODES: [u8; 30] = [
    0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x0B, 0x0C, 0x0E, 0x0F, 0x10, 0x13, 0x16, 0x17, 0x18, 0x1B,
    0x22, 0x2F, 0x31, 0x32, 0x33, 0x34, 0x36, 0x38, 0x3D, 0x1A, 0x30, 0x12, 0xFF, 0x00,
];

fn addr(r: &mut Rng) -> [u8; 6] {
    match r.below(4) {
        0 | 1 => PAD.0,
        2 => STRANGER.0,
        _ => {
            let b = r.bytes(6);
            [b[0], b[1], b[2], b[3], b[4], b[5]]
        }
    }
}

fn handle(r: &mut Rng) -> u16 {
    if r.chance(80) {
        H
    } else {
        r.next() as u16 & 0x0FFF
    }
}

/// A random HCI event: often one the host knows, with fields that make
/// sense (our pad, our handle), and a wrong length now and then.
fn rand_event(r: &mut Rng) -> Vec<u8> {
    let code = if r.chance(90) {
        r.pick(&CODES)
    } else {
        r.byte()
    };
    let mut params = match code {
        0x03 => cat(&[
            &[pick!(r, 0, 0, 0, 4, 0x0B)],
            &handle(r).to_le_bytes(),
            &addr(r),
            &[pick!(r, 1, 0, 2), r.below(2) as u8],
        ]),
        0x04 => cat(&[
            &addr(r),
            &cod3(pick!(r, PAD_COD, 0x002504, 0x5A020C, 0, r.next() as u32)),
            &[pick!(r, 1, 0)],
        ]),
        0x05 => cat(&[
            &[pick!(r, 0, 0, 1)],
            &handle(r).to_le_bytes(),
            &[pick!(r, 0x13, 0x08, 0x16, 0x05, r.byte())],
        ]),
        0x06 | 0x08 => cat(&[
            &[pick!(r, 0, 0, 5, 6)],
            &handle(r).to_le_bytes(),
            &[r.byte()],
        ]),
        0x07 => {
            let mut v = cat(&[&[0], &addr(r)]);
            let n = r.below(260);
            v.extend(r.bytes(n));
            v
        }
        0x0E => {
            let o = pick!(
                r,
                op::RESET,
                op::READ_BUFFER_SIZE,
                op::READ_BD_ADDR,
                op::WRITE_SCAN_ENABLE,
                op::LINK_KEY_REQUEST_REPLY,
                r.next() as u16
            )
            .to_le_bytes();
            let n = r.below(12);
            cat(&[
                &[r.byte(), o[0], o[1], pick!(r, 0, 0, 1, 0x0C)],
                &r.bytes(n),
            ])
        }
        0x0F => {
            let o = pick!(
                r,
                op::ACCEPT_CONNECTION_REQUEST,
                op::DISCONNECT,
                op::REMOTE_NAME_REQUEST,
                r.next() as u16
            )
            .to_le_bytes();
            vec![pick!(r, 0, 0, 0x0C, 0x02), 1, o[0], o[1]]
        }
        0x13 => {
            let n = r.below(4) as u8;
            let mut v = vec![n];
            for _ in 0..n {
                v.extend_from_slice(&handle(r).to_le_bytes());
                v.extend_from_slice(&(r.below(20) as u16).to_le_bytes());
            }
            v
        }
        0x16 | 0x17 | 0x31 | 0x32 | 0x33 | 0x34 | 0x36 => {
            let n = r.below(10);
            cat(&[&addr(r), &r.bytes(n)])
        }
        0x18 => cat(&[&addr(r), &r.bytes(16), &[r.byte()]]),
        _ => {
            let n = r.below(40);
            r.bytes(n)
        }
    };
    if r.chance(10) {
        let n = r.below(256);
        params = r.bytes(n);
    }
    params.truncate(255);
    let mut len = params.len() as u8;
    if r.chance(8) {
        len = r.byte();
    }
    let mut p = vec![0x04, code, len];
    p.extend_from_slice(&params);
    if r.chance(5) {
        p.truncate(r.below(p.len() + 1));
    }
    p
}

/// Our channel ids are handed out from 0x40 up; the pad's are in the
/// fixtures' range.
fn cid(r: &mut Rng) -> u16 {
    match r.below(10) {
        0..=3 => 1,
        4..=7 => 0x40 + r.below(6) as u16,
        8 => pick!(r, PAD_CTRL, PAD_INTR, PAD_SDP, 2, 3, 0),
        _ => r.next() as u16,
    }
}

fn rand_signal(r: &mut Rng) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..1 + r.below(3) {
        let code = if r.chance(85) {
            1 + r.below(0x13) as u8
        } else {
            r.byte()
        };
        let id = r.byte();
        let data = match code {
            // connection request: psm, their cid
            0x02 => cat(&[
                &pick!(r, 1_u16, 0x11, 0x13, 3, r.next() as u16).to_le_bytes(),
                &pick!(r, PAD_CTRL, PAD_INTR, PAD_SDP, 0, r.next() as u16).to_le_bytes(),
            ]),
            // connection response: dcid, scid, result, status
            0x03 => cat(&[
                &cid(r).to_le_bytes(),
                &cid(r).to_le_bytes(),
                &pick!(r, 0_u16, 1, 2, 4, r.next() as u16).to_le_bytes(),
                &[0, 0],
            ]),
            // configure request / response: cid, flags, options
            0x04 | 0x05 => {
                let mut v = cat(&[&cid(r).to_le_bytes(), &[pick!(r, 0, 0, 1), 0]]);
                if code == 0x05 {
                    v.extend_from_slice(&pick!(r, 0_u16, 0, 1, 2, 3, 4).to_le_bytes());
                }
                for _ in 0..r.below(4) {
                    let t = pick!(r, 1, 2, 3, 4, 5, 6, 0x81, r.byte());
                    let l = if r.chance(80) {
                        pick!(r, 2, 2, 9, 11, 1, 0)
                    } else {
                        r.byte()
                    };
                    v.extend_from_slice(&[t, l]);
                    let n = usize::from(l).min(r.below(12));
                    v.extend(r.bytes(n));
                }
                v
            }
            0x06 | 0x07 => cat(&[&cid(r).to_le_bytes(), &cid(r).to_le_bytes()]),
            0x0A | 0x0B => {
                let n = r.below(10);
                cat(&[
                    &pick!(r, 1_u16, 2, 3, r.next() as u16).to_le_bytes(),
                    &r.bytes(n),
                ])
            }
            _ => {
                let n = r.below(20);
                r.bytes(n)
            }
        };
        let mut len = data.len() as u16;
        if r.chance(8) {
            len = r.next() as u16;
        }
        out.extend_from_slice(&[code, id]);
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&data);
    }
    out
}

/// An SDP answer: a real one from the logs with damage, or random.
fn rand_sdp(r: &mut Rng) -> Vec<u8> {
    let base = pick!(
        r,
        fixtures::SWITCH_WORKS.sdp_hid,
        fixtures::SWITCH_WORKS.sdp_pnp,
        fixtures::XINPUT.sdp_hid,
        fixtures::DINPUT.sdp_hid,
        fixtures::DINPUT.sdp_pnp,
    );
    let mut v = hex(base);
    if r.chance(15) {
        let n = r.below(200);
        v = cat(&[&[pick!(r, 7, 1, 3, 5, r.byte())], &r.bytes(n)]);
    } else {
        v[1] = 0;
        v[2] = r.below(4) as u8;
        let k = 1 + r.below(4);
        mutate(r, &mut v, k);
    }
    v
}

/// A HID message: an input report (logged ones, damaged, or random), a
/// control message.
fn rand_hid(r: &mut Rng) -> Vec<u8> {
    let s = pick!(
        r,
        &fixtures::SWITCH_WORKS,
        &fixtures::XINPUT,
        &fixtures::ANDROID,
        &fixtures::DINPUT
    );
    let mut v = if !s.reports.is_empty() && r.chance(70) {
        let mut v = vec![0xA1];
        v.extend(hex(r.pick(s.reports)));
        if r.chance(50) {
            let k = 1 + r.below(3);
            mutate(r, &mut v, k);
        }
        v
    } else {
        let n = r.below(80);
        cat(&[
            &[
                pick!(
                    r,
                    0xA1,
                    0xA1,
                    0xA2,
                    0x00,
                    0x01,
                    0x03,
                    0x0B,
                    0x15,
                    0x41,
                    0x90,
                    r.byte()
                ),
                pick!(r, 0x30, 0x21, 0x3F, 0x01, 0x04, 0x02, r.byte()),
            ],
            &r.bytes(n),
        ])
    };
    if r.chance(5) {
        v.truncate(r.below(3));
    }
    v
}

/// One ACL packet from the controller.
fn rand_acl(r: &mut Rng) -> Vec<u8> {
    let c = cid(r);
    let payload = match c {
        1 => rand_signal(r),
        0x40 if r.chance(50) => rand_sdp(r),
        _ if r.chance(70) => rand_hid(r),
        _ => rand_sdp(r),
    };
    let mut l2len = payload.len() as u16;
    if r.chance(8) {
        l2len = r.next() as u16;
    }
    let frame = cat(&[&l2len.to_le_bytes(), &c.to_le_bytes(), &payload]);
    let pb = if r.chance(85) {
        acl::PB_START
    } else {
        r.below(4) as u8
    };
    let mut frag = frame.clone();
    if r.chance(10) {
        // A piece of a longer frame, or a continuation.
        frag.truncate(r.below(frame.len() + 1));
    }
    frag.truncate(1021);
    let mut p = acl_in(handle(r), pb, &frag);
    if r.chance(5) {
        // A wrong ACL length.
        p[3] = r.byte();
        p[4] = r.below(5) as u8;
    }
    p
}

fn mutate(r: &mut Rng, v: &mut Vec<u8>, times: usize) {
    for _ in 0..times {
        if v.is_empty() {
            v.push(r.byte());
            continue;
        }
        let i = r.below(v.len());
        match r.below(7) {
            0 | 1 => v[i] ^= 1 << r.below(8),
            2 => v[i] = pick!(r, 0, 0xFF, 0x7F, 0x80, 1, r.byte()),
            3 => v.truncate(i),
            4 => {
                let n = r.below(16);
                let b = r.bytes(n);
                v.splice(i..i, b);
            }
            5 => {
                v.remove(i);
            }
            _ => {
                let n = r.below(300);
                v.extend(r.bytes(n));
            }
        }
    }
}

// ------------------------------------------------------- recorded runs

/// The packets a whole session gives the host (controller answers and
/// the pad's side), with their times, from the scripted tests' sim.
fn recording(f: &fixtures::Session, switch: bool) -> Vec<(u64, Vec<u8>)> {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    let (ctrl, intr) = s.reconnect(f);
    if switch {
        let mut pad = SwitchPad::new(f);
        s.switch_runs(&mut pad, intr, 2_500);
    } else {
        for r in f.reports {
            s.report(intr, &hex(r));
            s.advance(10);
        }
        s.control(ctrl, &[0xA1, 0x04, 0x13]);
    }
    s.feed(&ev_disconnected(H, 0x08));
    s.advance(1_100);
    // The sim's own start-up is in `fed` too (it starts after `start`).
    s.fed
}

// --------------------------------------------------------------- tests

/// Random packets of every kind, from a fresh start and from deep inside
/// recorded sessions (a random cut of a real reconnect, then noise).
#[test]
fn random_packets_never_break_the_host() {
    let mut r = Rng(seed());
    let recs = [
        recording(&fixtures::SWITCH_WORKS, true),
        recording(&fixtures::XINPUT, false),
        recording(&fixtures::DINPUT, false),
    ];
    let cases = 150 * scale();
    let (mut most_sent, mut most_logs, mut packets) = (0, 0, 0_u64);
    for case in 0..cases {
        let mut run = Run::new();
        run.answer = r.chance(50);
        if case % 3 != 0 {
            // Replay a real session up to a random point.
            let rec = &recs[r.below(recs.len())];
            let cut = r.below(rec.len());
            for (t, p) in &rec[..cut] {
                run.until(*t);
                run.packet(p);
            }
        }
        for _ in 0..200 {
            let p = match r.below(10) {
                0..=3 => rand_event(&mut r),
                _ => rand_acl(&mut r),
            };
            run.packet(&p);
            packets += 1;
            if r.chance(20) {
                let t = run.now + pick!(r, 1, 10, 100, 1_000, 5_000, 31_000);
                run.until(t);
            }
        }
        run.finish();
        most_sent = most_sent.max(run.most_sent);
        most_logs = most_logs.max(run.most_logs);
    }
    println!(
        "random: cases={cases} packets={packets} most_sent_per_call={most_sent} most_logs_per_call={most_logs} init_ends_so_far={}",
        INIT_ENDS.load(std::sync::atomic::Ordering::Relaxed)
    );
}

/// Real sessions, damaged: bytes flipped, packets cut, lengthened,
/// dropped, doubled or swapped, times shifted.
#[test]
fn damaged_recordings_never_break_the_host() {
    let mut r = Rng(seed() ^ 0x5555);
    let recs = [
        recording(&fixtures::SWITCH_WORKS, true),
        recording(&fixtures::XINPUT, false),
        recording(&fixtures::ANDROID, false),
        recording(&fixtures::DINPUT, false),
    ];
    let cases = 120 * scale();
    let (mut most_sent, mut most_logs, mut packets) = (0, 0, 0_u64);
    let mut connected_cases = 0;
    for case in 0..cases {
        let mut rec = recs[case % recs.len()].clone();
        let damage = 1 + r.below(12);
        for _ in 0..damage {
            if rec.is_empty() {
                break;
            }
            let i = r.below(rec.len());
            match r.below(6) {
                0..=2 => {
                    let k = 1 + r.below(3);
                    mutate(&mut r, &mut rec[i].1, k);
                }
                3 => {
                    rec.remove(i);
                }
                4 => {
                    let p = rec[i].clone();
                    rec.insert(i, p);
                }
                _ => {
                    let j = r.below(rec.len());
                    let (a, b) = (rec[i].1.clone(), rec[j].1.clone());
                    rec[i].1 = b;
                    rec[j].1 = a;
                }
            }
        }
        let mut run = Run::new();
        let mut seen_connected = false;
        for (t, p) in &rec {
            run.until(*t);
            run.packet(p);
            packets += 1;
            seen_connected |= run.host.connected();
        }
        connected_cases += usize::from(seen_connected);
        run.finish();
        most_sent = most_sent.max(run.most_sent);
        most_logs = most_logs.max(run.most_logs);
    }
    println!(
        "damaged: cases={cases} packets={packets} reached_connected={connected_cases} most_sent_per_call={most_sent} most_logs_per_call={most_logs}"
    );
    assert!(
        connected_cases > cases / 4,
        "the damage is not so heavy that nothing connects"
    );
}

/// The recordings themselves, undamaged, through the checked runner:
/// they connect (so the checks above see deep states).
#[test]
fn undamaged_recordings_connect() {
    for (f, sw) in [
        (&fixtures::SWITCH_WORKS, true),
        (&fixtures::XINPUT, false),
        (&fixtures::ANDROID, false),
        (&fixtures::DINPUT, false),
    ] {
        let rec = recording(f, sw);
        let mut run = Run::new();
        let mut connected = false;
        for (t, p) in &rec {
            run.until(*t);
            run.packet(p);
            connected |= run.host.connected();
        }
        assert!(connected, "{}\n{}", f.name, run.context());
        run.finish();
        assert!(run.host.waiting());
    }
}

// ------------------------------------------------------ pairing windows

/// Pairing windows opened, picked, cancelled and left to their deadline
/// at random, between random packets, with a pad that sometimes pairs
/// properly. A test-side oracle keeps what must hold (window open, the
/// pad picked, the deadline, the saved pads, the new pairing not taken
/// yet) and checks every packet sent and every state after each call.
#[cfg(feature = "pair")]
mod window {
    use super::*;
    use pad_host::{PAIR_WINDOW_MAX, PairEnd, PairPhase, PairRefused};
    use pad_proto::hci::BdAddr;

    /// Made-up pads: one new, one more for the list.
    const NEW: BdAddr = BdAddr([0x16, 0x25, 0x34, 0x43, 0x52, 0x61]);
    const OTHER: BdAddr = BdAddr([0x1E, 0x2E, 0x3E, 0x4E, 0x5E, 0x6E]);
    const POOL: [BdAddr; 4] = [PAD, NEW, STRANGER, OTHER];
    /// The key a proper pairing gives here (made up).
    const FUZZ_KEY: [u8; 16] = [
        0xC3, 0x1F, 0x86, 0x4A, 0xE9, 0x27, 0x5D, 0xB0, 0x73, 0x0E, 0xA8, 0x64, 0x3B, 0xF2, 0x99,
        0x15,
    ];
    /// Names heard: never in a log line.
    const NAMES: [&[u8]; 3] = [
        b"Quokka Gamepad",
        b"",
        b"Numbat Controller with a long name",
    ];

    /// The log, checked line by line: no key, no name, no ignored key
    /// change; the lines that say a new pairing was held are counted.
    #[derive(Default)]
    struct WOut {
        sent: Vec<Vec<u8>>,
        logs: usize,
        held: u32,
        last_logs: Vec<String>,
    }

    impl Radio for WOut {
        fn send(&mut self, p: &[u8]) {
            self.sent.push(p.to_vec());
        }
        fn log(&mut self, tag: &'static str, args: fmt::Arguments<'_>) {
            let line = format!("{tag} {args}");
            assert_no_key(&line);
            let low = line.to_lowercase();
            let k: String = FUZZ_KEY.iter().map(|b| format!("{b:02x}")).collect();
            assert!(
                !low.contains(&k[..8]) && !low.contains(&k[24..]),
                "key in: {line}"
            );
            assert!(
                !line.contains("Quokka") && !line.contains("Numbat"),
                "name in: {line}"
            );
            assert!(!line.contains("key change '"), "{line}");
            if line.contains("new pairing held for the store") {
                self.held += 1;
            }
            self.logs += 1;
            if self.last_logs.len() >= 60 {
                self.last_logs.remove(0);
            }
            self.last_logs.push(line);
        }
    }

    /// What must hold, kept by the test alone.
    #[derive(Default)]
    struct Oracle {
        open: bool,
        opened: u64,
        target: Option<[u8; 6]>,
        deadline: u64,
        /// The saved pads as the flash (and so the host) should have them.
        saved: Vec<[u8; 6]>,
        /// A new pairing the host must hand over once.
        pending: Option<[u8; 6]>,
        /// "Held" log lines when this window opened.
        held_at_open: u32,
    }

    #[derive(Default)]
    struct Tally {
        windows: u64,
        saved: u64,
        takes: u64,
        ends: [u64; 6],
        packets: u64,
    }

    struct WRun {
        host: PadHost,
        out: WOut,
        now: u64,
        o: Oracle,
        handles: BTreeSet<u16>,
        /// The next step of the picked pad's proper pairing.
        step: usize,
        key_type: u8,
        confirm_disconnect: bool,
        rng: Rng,
    }

    impl WRun {
        fn new(seed: u64) -> Self {
            let ram = Ram::with(&[(PAD, KEY)]);
            let mut r = WRun {
                host: PadHost::new(),
                out: WOut::default(),
                now: 1_000,
                o: Oracle {
                    saved: vec![PAD.0],
                    ..Oracle::default()
                },
                handles: BTreeSet::new(),
                step: 0,
                key_type: 0x04,
                confirm_disconnect: true,
                rng: Rng(seed),
            };
            let mut view = ReadOnly {
                ram: &ram,
                reads: 0,
            };
            r.host
                .load_pairings(&mut view, region(), &mut r.out)
                .unwrap();
            let (was_open, was_target) = r.before();
            r.host.start(r.now, &mut r.out);
            r.after(was_open, was_target, false);
            r
        }

        fn context(&self) -> String {
            self.out.last_logs.join("\n")
        }

        fn before(&self) -> (bool, Option<[u8; 6]>) {
            (self.o.open, self.o.target)
        }

        /// Checks after one call: every packet sent, then the window.
        fn after(&mut self, was_open: bool, was_target: Option<[u8; 6]>, ticked: bool) {
            let sent = std::mem::take(&mut self.out.sent);
            let logs = std::mem::take(&mut self.out.logs);
            assert!(sent.len() <= MAX_SENT_PER_CALL, "{}", self.context());
            assert!(logs <= MAX_LOGS_PER_CALL, "{}", self.context());
            let open_now = self.host.pair_phase() != PairPhase::Off;
            for p in &sent {
                self.check_packet(p, was_open, was_target, open_now);
            }
            self.follow_window(open_now, ticked);
            // The answers of a controller that answers everything.
            for p in &sent {
                if p[0] == 0x01 {
                    let opcode = u16::from_le_bytes([p[1], p[2]]);
                    for a in self.answers(opcode, &p[4..]) {
                        self.packet(&a);
                    }
                }
            }
        }

        fn check_packet(
            &self,
            p: &[u8],
            was_open: bool,
            was_target: Option<[u8; 6]>,
            open_now: bool,
        ) {
            if !was_open && !open_now {
                // Outside a window: the reconnect-only rules, as always
                // (accept and key only for a saved pad).
                check_sent_saved(p, &self.o.saved);
                return;
            }
            assert!(!p.is_empty() && p.len() <= TX_MAX);
            if p[0] != 0x01 {
                return;
            }
            let opcode = u16::from_le_bytes([p[1], p[2]]);
            let a = p.get(4..10);
            let target_now = self.o.target;
            let is_target = |a: Option<&[u8]>| {
                a.is_some()
                    && (a == was_target.as_ref().map(|t| &t[..])
                        || a == target_now.as_ref().map(|t| &t[..]))
            };
            match opcode {
                op::PIN_CODE_REQUEST_REPLY => panic!("PIN reply\n{}", self.context()),
                op::INQUIRY => assert!(
                    (was_open && was_target.is_none()) || (open_now && target_now.is_none()),
                    "a search outside a searching window\n{}",
                    self.context()
                ),
                op::CREATE_CONNECTION
                | op::IO_CAPABILITY_REQUEST_REPLY
                | op::USER_CONFIRMATION_REQUEST_REPLY => assert!(
                    is_target(a),
                    "{} for a pad not picked\n{}",
                    op::name(opcode),
                    self.context()
                ),
                op::ACCEPT_CONNECTION_REQUEST => assert!(
                    !(was_open && open_now)
                        && a.is_some_and(|a| self.o.saved.iter().any(|s| s == a)),
                    "a call taken during a window\n{}",
                    self.context()
                ),
                op::LINK_KEY_REQUEST_REPLY => {
                    // The pad being paired may be asked again for the key
                    // this window made (the controller does so), never for
                    // an older one.
                    if (was_open || open_now) && is_target(a) {
                        assert_eq!(
                            p.get(10..26),
                            Some(&FUZZ_KEY[..]),
                            "an old key offered to the pad being paired\n{}",
                            self.context()
                        );
                        return;
                    }
                    assert!(a.is_some_and(|a| self.o.saved.iter().any(|s| s == a)));
                }
                op::WRITE_SCAN_ENABLE => assert_eq!(p[4] & 1, 0, "inquiry scan on"),
                _ => {}
            }
        }

        /// The window's state against the oracle.
        fn follow_window(&mut self, open_now: bool, ticked: bool) {
            if self.o.open && !open_now {
                let end = self
                    .host
                    .pair_outcome()
                    .expect("an ended window has an outcome");
                let held = self.out.held - self.o.held_at_open;
                if end == PairEnd::Saved {
                    let t = self.o.target.expect("saved only after a pick");
                    assert_eq!(held, 1, "one new pairing per saved window");
                    assert!(self.o.pending.is_none());
                    self.o.pending = Some(t);
                    if !self.o.saved.contains(&t) {
                        self.o.saved.push(t);
                    }
                } else {
                    assert_eq!(held, 0, "no new pairing from a window that ended {end:?}");
                    assert!(self.host.take_pending_save().is_none());
                }
                self.o.open = false;
                self.o.target = None;
                self.step = 0;
            }
            if open_now {
                assert!(self.o.open, "a window nobody opened\n{}", self.context());
                if let Some(d) = self.host.pair_deadline() {
                    assert!(
                        d <= self.o.deadline,
                        "deadline {d} after {}",
                        self.o.deadline
                    );
                }
                if ticked {
                    assert!(
                        self.now < self.o.deadline,
                        "open after its deadline\n{}",
                        self.context()
                    );
                }
            } else {
                // The table as the flash has it.
                let mut have: Vec<[u8; 6]> =
                    (0..4).filter_map(|i| self.host.saved_address(i)).collect();
                let mut want = self.o.saved.clone();
                have.sort();
                want.sort();
                assert_eq!(have, want, "saved pads\n{}", self.context());
            }
        }

        fn answers(&mut self, opcode: u16, params: &[u8]) -> Vec<Vec<u8>> {
            let o = opcode.to_le_bytes();
            let mut v = Vec::new();
            match opcode {
                op::INQUIRY | op::CREATE_CONNECTION => v.push(evt(0x0F, &[0, 1, o[0], o[1]])),
                _ => v.push(answer(opcode, params)),
            }
            if opcode == op::DISCONNECT && self.confirm_disconnect && params.len() >= 2 {
                let h = u16::from_le_bytes([params[0], params[1]]);
                v.push(ev_disconnected(h, 0x16));
            }
            v
        }

        fn note_handles(&mut self, p: &[u8]) {
            if p.first() == Some(&0x04)
                && let Some(hci::Event::ConnectionComplete { handle, .. }) =
                    hci::parse_event(&p[1..])
            {
                self.handles.insert(handle);
            }
        }

        fn packet(&mut self, p: &[u8]) {
            let (wo, wt) = self.before();
            self.note_handles(p);
            let attempts = self.host.counters().attempts;
            self.host.packet(self.now, p, &mut self.out);
            if wo && self.host.pair_phase() != PairPhase::Off {
                assert_eq!(
                    self.host.counters().attempts,
                    attempts,
                    "a window counted as a call"
                );
            }
            self.after(wo, wt, false);
        }

        fn tick(&mut self) {
            let (wo, wt) = self.before();
            let _ = self.host.tick(self.now, &mut self.out);
            self.after(wo, wt, true);
            while self.host.take_event().is_some() {}
        }

        fn until(&mut self, t: u64) {
            while self.now + 10 <= t {
                self.now += 10;
                self.tick();
            }
            self.now = self.now.max(t);
        }

        fn begin(&mut self) {
            let for_ms = pick!(self.rng, 2_000, 10_000, 60_000, 200_000);
            let (wo, wt) = self.before();
            let held = self.out.held;
            let r = self.host.pair_begin(self.now, for_ms, &mut self.out);
            match r {
                Ok(()) => {
                    assert!(!wo, "a second window opened");
                    assert!(self.o.pending.is_none(), "opened over a pairing not taken");
                    self.o = Oracle {
                        open: true,
                        opened: self.now,
                        target: None,
                        deadline: self.now + for_ms.min(PAIR_WINDOW_MAX),
                        saved: std::mem::take(&mut self.o.saved),
                        pending: None,
                        held_at_open: held,
                    };
                    self.step = 0;
                }
                Err(PairRefused::SaveWaiting) => assert!(self.o.pending.is_some()),
                Err(_) => {}
            }
            self.after(wo, wt, false);
        }

        fn choose(&mut self) {
            let n = self.host.pair_found_count();
            let i = self.rng.below(n + 1);
            let seen = self.host.pair_found(i).map(|f| f.tail);
            let tail = if self.rng.chance(90) {
                seen.unwrap_or([0; 2])
            } else {
                [self.rng.byte(), self.rng.byte()]
            };
            let Some(a) = self.host.pair_target(i, tail) else {
                assert!(
                    seen != Some(tail) || self.host.pair_phase() != PairPhase::Searching,
                    "the right tail refused"
                );
                return;
            };
            assert_eq!(Some(a.tail()), seen);
            let for_ms = pick!(self.rng, 1_000, 5_000, 30_000, 200_000);
            let (wo, wt) = self.before();
            if self
                .host
                .pair_choose(i, self.now, for_ms, &mut self.out)
                .is_ok()
            {
                self.o.target = Some(a.whole_address());
                self.o.deadline = (self.now + for_ms).min(self.o.opened + PAIR_WINDOW_MAX);
                self.step = 0;
                self.key_type = pick!(self.rng, 0x04, 0x04, 0x04, 0x07, 0x03, 0x05);
                assert_eq!(self.host.pair_phase(), PairPhase::Pairing);
            }
            self.after(wo, wt, false);
        }

        fn cancel(&mut self) {
            let (wo, wt) = self.before();
            if let Some(a) = self.o.pending.take() {
                self.o.saved.retain(|s| *s != a);
            }
            self.host.pair_cancel(self.now, &mut self.out);
            assert_eq!(self.host.pair_phase(), PairPhase::Off);
            assert!(self.host.take_pending_save().is_none());
            self.after(wo, wt, false);
        }

        fn take(&mut self) {
            let got = self.host.take_pending_save();
            assert_eq!(
                got.as_ref().map(|p| p.addr().whole_address()),
                self.o.pending.take(),
                "the new pairing, once"
            );
            if let Some(p) = got {
                assert!(matches!(p.key_type(), 0x04 | 0x07));
                assert_no_key(&format!("{p:?}"));
                TAKES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }

        fn forget(&mut self) {
            let place = self.rng.below(5);
            let a = self.host.saved_address(place);
            let (wo, wt) = self.before();
            let got = self.host.forget(place, self.now, &mut self.out);
            assert_eq!(got.map(|g| g.whole_address()), a);
            if let Some(a) = a {
                self.o.saved.retain(|s| *s != a);
                if self.o.pending == Some(a) {
                    self.o.pending = None;
                }
            }
            self.after(wo, wt, false);
        }

        /// The picked pad's next step of a proper pairing.
        fn pad_step(&mut self) {
            let Some(t) = self.o.target else { return };
            let t = BdAddr(t);
            let p = match self.step {
                0 => ev_conn_complete(0, H, t),
                1 => ev_addr(0x17, t),
                2 => ev_addr(0x31, t),
                3 => ev_io_cap_response(t),
                4 => ev_user_confirmation(t),
                5 => ev_ssp_complete(t),
                6 => ev_link_key_typed(t, FUZZ_KEY, self.key_type),
                7 => ev_auth(0, H),
                8 => ev_encrypted(H),
                _ => return,
            };
            self.step += 1;
            self.packet(&p);
        }

        fn finish(&mut self) {
            self.cancel();
            self.take();
            self.confirm_disconnect = true;
            for h in self.handles.clone() {
                self.packet(&ev_disconnected(h, 0x13));
            }
            let end = self.now + 130_000;
            self.until(end);
            assert!(
                self.host.waiting(),
                "phase {}\n{}",
                self.host.phase(),
                self.context()
            );
            assert_eq!(self.host.read(), PadState::NONE);
        }
    }

    /// `check_sent` with the saved pads of this run in place of PAD alone.
    fn check_sent_saved(p: &[u8], saved: &[[u8; 6]]) {
        if p.first() == Some(&0x01) && p.len() >= 10 {
            let opcode = u16::from_le_bytes([p[1], p[2]]);
            if matches!(
                opcode,
                op::ACCEPT_CONNECTION_REQUEST | op::LINK_KEY_REQUEST_REPLY
            ) {
                assert!(
                    saved.iter().any(|s| s[..] == p[4..10]),
                    "{} for another address",
                    op::name(opcode)
                );
                let mut q = p.to_vec();
                q[4..10].copy_from_slice(&PAD.0);
                check_sent(&q);
                return;
            }
        }
        check_sent(p);
    }

    static TAKES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    #[test]
    fn pairing_windows_call_only_the_target_and_save_once() {
        let mut r = Rng(seed() ^ 0xA11CE);
        let cases = 60 * scale();
        let mut t = Tally::default();
        for _ in 0..cases {
            let mut run = WRun::new(r.next());
            run.confirm_disconnect = r.chance(85);
            run.until(run.now + 1_000);
            let held_before = run.out.held;
            for _ in 0..300 {
                let was_open = run.o.open;
                // A picked pad mostly goes on with its pairing, so that
                // many windows get as far as a new key.
                let roll = if run.o.target.is_some() && run.rng.chance(50) {
                    30
                } else {
                    run.rng.below(100)
                };
                match roll {
                    0..=7 => run.begin(),
                    8..=15 => {
                        let a = run.rng.pick(&POOL);
                        let cod = pick!(run.rng, PAD_COD, PAD_COD, 0x5A020C);
                        // Outside a window pad-proto's own log line for an
                        // unasked search result carries the name (not this
                        // crate's to change), so names are heard only in one.
                        let name = if run.o.open {
                            run.rng.pick(&NAMES)
                        } else {
                            &b""[..]
                        };
                        run.packet(&ev_ext_inquiry(a, cod, name));
                    }
                    16..=23 => run.choose(),
                    24..=49 => run.pad_step(),
                    50..=52 => run.cancel(),
                    53..=57 => run.take(),
                    58 => run.forget(),
                    59..=61 => run.packet(&ev_conn_request(PAD, PAD_COD)),
                    62..=64 => {
                        // The pad being paired asks for a PIN, fails, or drops.
                        let p = match run.rng.below(3) {
                            0 => ev_addr(0x16, BdAddr(run.o.target.unwrap_or(NEW.0))),
                            1 => ev_auth(5, H),
                            _ => ev_disconnected(H, 0x13),
                        };
                        run.packet(&p);
                    }
                    65..=79 => {
                        let p = match run.rng.below(10) {
                            0..=3 => rand_event(&mut run.rng),
                            _ => rand_acl(&mut run.rng),
                        };
                        run.packet(&p);
                        t.packets += 1;
                    }
                    _ => {
                        let step = pick!(run.rng, 1, 10, 10, 100, 1_000, 5_000, 31_000);
                        run.until(run.now + step);
                    }
                }
                if was_open && !run.o.open {
                    t.windows += 1;
                    if let Some(e) = run.host.pair_outcome() {
                        let k = match e {
                            PairEnd::Saved => 0,
                            PairEnd::Timeout => 1,
                            PairEnd::OldPin => 2,
                            PairEnd::Refused => 3,
                            PairEnd::Lost => 4,
                            PairEnd::Cancelled => 5,
                        };
                        t.ends[k] += 1;
                    }
                }
            }
            run.finish();
            t.saved += u64::from(run.out.held - held_before);
        }
        t.takes = TAKES.load(std::sync::atomic::Ordering::Relaxed);
        println!(
            "windows: cases={cases} windows={} ends(saved,timeout,old-pin,refused,lost,cancelled)={:?} held={} taken={} noise_packets={}",
            t.windows, t.ends, t.saved, t.takes, t.packets
        );
        assert!(t.ends[0] > 0, "some windows pair");
        assert!(t.takes <= t.saved);
    }
}
