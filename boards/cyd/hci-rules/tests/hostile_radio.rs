//! Hostile radio input against the whole path the board uses: the real
//! gamepad host (crates/pad-host, reconnect-only) fed by a reacting
//! controller and pad of this file (not pad-host's scripted ones), every
//! packet the host sends put through the board's gate that knows only the
//! saved pad.
//!
//! Each run (a seed) sets the controller up, lets the saved pad call and
//! fails it in random ways: lying lengths, fragments that never end,
//! floods of channels and calls, calls from strangers, pairing attempts,
//! failed keys and encryption, wrong-mode and short reports, an endless
//! search answer, a virtual cable unplug, answers that never come, random
//! events and random bytes. Throughout, the host must not panic, must
//! send only what the gate lets through (an accept, a key, a name request
//! only to the saved pad; never a search, a call, a "yes" to pairing,
//! inquiry scan), must keep its sends and log lines per call bounded, and
//! must never log a key or a whole address (no three bytes of either in a
//! row). After the chaos the pad goes quiet and every link drops: within
//! 12 s the host is back to waiting with nothing pressed. Then the saved
//! pad calls cleanly and must be a working Switch pad again (buttons and
//! motion). The pairing area is only ever read, its CRC the same.
//!
//! Scale: HOSTILE_SEEDS (default 24) runs of HOSTILE_SECS (default 20)
//! simulated seconds of chaos each. The addresses and the key are made
//! up.

use std::collections::VecDeque;
use std::fmt::{self, Write as _};

use hci_rules::Gate;
use pad_host::inject::{SwitchInput, switch_report};
use pad_host::{Mode, PadHost, Radio, TX_MAX};
use pad_store::{Flash, Pairing, ReadFlash, Span, Store, check_region};

// Bytes no random input of this test ever contains (see Rng::byte), so
// three of them in a row in a log line can only come from a secret.
const SECRET_BYTES: [u8; 10] = [0xA7, 0xB3, 0xC9, 0xD1, 0xE5, 0x5B, 0x6D, 0x8F, 0x9A, 0x4E];
/// The saved pad (as HCI sends it, low byte first).
const PAD: [u8; 6] = [0xA7, 0xB3, 0xC9, 0xD1, 0xE5, 0x5B];
/// The console's own address (Read BD_ADDR).
const OWN: [u8; 6] = [0x6D, 0x8F, 0x9A, 0x4E, 0xA7, 0xC9];
/// Strangers.
const STRANGERS: [[u8; 6]; 3] = [
    [0xD1, 0x4E, 0xB3, 0x6D, 0xE5, 0x9A],
    [0x5B, 0xC9, 0x8F, 0xA7, 0xD1, 0xB3],
    // Differs from the pad in its first byte only.
    [0xB3, 0xB3, 0xC9, 0xD1, 0xE5, 0x5B],
];
const KEY: [u8; 16] = [
    0x8F, 0xA7, 0x4E, 0xD1, 0x9A, 0xC9, 0x5B, 0xE5, 0xB3, 0x6D, 0xA7, 0xD1, 0x4E, 0x8F, 0xC9, 0x9A,
];
const WRONG_KEY: [u8; 16] = [0x11; 16];
const PAD_COD: u32 = 0x00_2508;

const NVS: Span = Span {
    offset: 0x9000,
    len: 0x6000,
};

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

// ------------------------------------------------------------ random

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
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
    fn chance(&mut self, per_mille: u64) -> bool {
        self.below(1000) < per_mille
    }
    /// A random byte that is never one of the secret bytes.
    fn byte(&mut self) -> u8 {
        let mut b = self.next() as u8;
        while SECRET_BYTES.contains(&b) {
            b = b.wrapping_add(1);
        }
        b
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.byte()).collect()
    }
    /// Fewer than `max` random bytes.
    fn some_bytes(&mut self, max: u64) -> Vec<u8> {
        let n = self.below(max) as usize;
        self.bytes(n)
    }
    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u64) as usize]
    }
}

// ------------------------------------------------------------ flash

/// 64 KB of flash. Written only while the test builds it (pad-store's
/// writing path, as the pairing firmware did); the host gets a reading
/// view.
struct Mem(Vec<u8>);

impl Flash for Mem {
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

struct ReadView<'a>(&'a [u8]);

impl ReadFlash for ReadView<'_> {
    type Error = ();
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), ()> {
        let o = offset as usize;
        buf.copy_from_slice(self.0.get(o..o + buf.len()).ok_or(())?);
        Ok(())
    }
}

fn flash_with_pad() -> Vec<u8> {
    let mut mem = Mem(vec![0xFF; 0x1_0000]);
    let region = check_region(NVS, &[], 0x40_0000).unwrap();
    {
        let (mut store, _) = Store::open(&mut mem, region).unwrap();
        store
            .save(&Pairing {
                addr: PAD,
                key: KEY,
                key_type: 0x04,
            })
            .unwrap();
    }
    mem.0
}

// ------------------------------------------------------------ the radio

fn hex3(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Every three-byte run of the key and of each address, both ways
/// round, as lower-case hex.
fn secret_windows() -> Vec<(String, &'static str)> {
    let mut v = Vec::new();
    let mut add = |bytes: &[u8], what: &'static str| {
        for w in bytes.windows(3) {
            v.push((hex3(w), what));
            let r: Vec<u8> = w.iter().rev().copied().collect();
            v.push((hex3(&r), what));
        }
    };
    add(&KEY, "key");
    add(&PAD, "pad address");
    add(&OWN, "own address");
    for s in &STRANGERS {
        add(s, "stranger address");
    }
    v
}

/// Whether the hex text `w` stands in `n` on a byte boundary: at an even
/// place of a run of hex digits (or of a run that a masked "xx" starts).
fn shows(n: &str, w: &str) -> bool {
    let b = n.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_hexdigit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_hexdigit() {
            i += 1;
        }
        let run = &n[start..i];
        // A run right after "x" digits (a masked byte) is byte-aligned
        // from its start too.
        if (0..run.len()).step_by(2).any(|k| run[k..].starts_with(w)) {
            return true;
        }
    }
    false
}

fn normalise(line: &str) -> String {
    line.to_ascii_lowercase()
        .replace("0x", "")
        .chars()
        .filter(|c| !matches!(c, ':' | ' ' | '-' | ',' | '[' | ']' | '_' | '.'))
        .collect()
}

#[derive(Default)]
struct Totals {
    sends: u64,
    lines: u64,
    longest_line: usize,
    most_sends_per_call: usize,
    most_lines_per_call: usize,
    refused: u64,
    chaos_steps: u64,
    links: u64,
    reports: u64,
    sdp_answers: u64,
    raw: u64,
    back_ms_max: u64,
}

struct Wire {
    gate: Gate,
    /// Sent by the host, not yet taken by the controller.
    out: VecDeque<Vec<u8>>,
    lines_this_call: usize,
    sends_this_call: usize,
    windows: Vec<(String, &'static str)>,
    tail: VecDeque<String>,
    t: Totals,
    seed: u64,
}

impl Wire {
    fn new(seed: u64) -> Self {
        let mut gate = Gate::new();
        assert!(gate.allow(PAD));
        Wire {
            gate,
            out: VecDeque::new(),
            lines_this_call: 0,
            sends_this_call: 0,
            windows: secret_windows(),
            tail: VecDeque::new(),
            t: Totals::default(),
            seed,
        }
    }

    fn begin_call(&mut self) {
        self.lines_this_call = 0;
        self.sends_this_call = 0;
    }

    fn end_call(&mut self) {
        self.t.most_sends_per_call = self.t.most_sends_per_call.max(self.sends_this_call);
        self.t.most_lines_per_call = self.t.most_lines_per_call.max(self.lines_this_call);
        assert!(
            self.sends_this_call <= 64,
            "seed {}: {} sends in one call",
            self.seed,
            self.sends_this_call
        );
        assert!(
            self.lines_this_call <= 64,
            "seed {}: {} log lines in one call\n{}",
            self.seed,
            self.lines_this_call,
            self.tail()
        );
    }

    fn tail(&self) -> String {
        self.tail.iter().cloned().collect::<Vec<_>>().join("\n")
    }
}

impl Radio for Wire {
    fn send(&mut self, packet: &[u8]) {
        self.sends_this_call += 1;
        self.t.sends += 1;
        let seed = self.seed;
        assert!(
            packet.len() <= TX_MAX,
            "seed {seed}: send of {}",
            packet.len()
        );
        // What the host itself may never send, gate or not.
        if packet.first() == Some(&0x01) && packet.len() >= 4 {
            let opcode = u16::from_le_bytes([packet[1], packet[2]]);
            let p = &packet[4..];
            match opcode {
                0x0401 | 0x0405 | 0x040D | 0x042B | 0x042C => {
                    panic!("seed {seed}: forbidden command {opcode:#06x}")
                }
                0x0C1A => assert!(
                    p == [0x02] || p == [0x00],
                    "seed {seed}: scan enable {p:02x?}"
                ),
                0x0409 | 0x040B | 0x0419 => {
                    assert_eq!(&p[..6], &PAD, "seed {seed}: {opcode:#06x} to a stranger")
                }
                _ => {}
            }
        }
        if let Err(e) = self.gate.check(packet) {
            self.t.refused += 1;
            panic!(
                "seed {seed}: the gate refused {e:?}: {:02x?}\n{}",
                &packet[..packet.len().min(4)],
                self.tail()
            );
        }
        self.out.push_back(packet.to_vec());
    }

    fn log(&mut self, tag: &'static str, args: fmt::Arguments<'_>) {
        let mut line = String::new();
        let _ = write!(line, "{tag}: {args}");
        self.lines_this_call += 1;
        self.t.lines += 1;
        self.t.longest_line = self.t.longest_line.max(line.len());
        let n = normalise(&line);
        for (w, what) in &self.windows {
            assert!(
                !shows(&n, w),
                "seed {}: a log line shows three bytes of the {what} ({w}):\n{line}",
                self.seed
            );
        }
        if self.tail.len() == 40 {
            self.tail.pop_front();
        }
        self.tail.push_back(line);
    }
}

// ------------------------------------------------------------ packets

fn event(code: u8, params: &[u8]) -> Vec<u8> {
    let mut v = vec![0x04, code, params.len() as u8];
    v.extend_from_slice(params);
    v
}

fn complete(opcode: u16, ret: &[u8]) -> Vec<u8> {
    let mut p = vec![1];
    p.extend_from_slice(&opcode.to_le_bytes());
    p.extend_from_slice(ret);
    event(0x0E, &p)
}

fn status(opcode: u16, st: u8) -> Vec<u8> {
    let mut p = vec![st, 1];
    p.extend_from_slice(&opcode.to_le_bytes());
    event(0x0F, &p)
}

fn with_addr(addr: &[u8; 6], rest: &[u8]) -> Vec<u8> {
    let mut p = addr.to_vec();
    p.extend_from_slice(rest);
    p
}

fn conn_request(addr: &[u8; 6], cod: u32) -> Vec<u8> {
    let c = cod.to_le_bytes();
    event(0x04, &with_addr(addr, &[c[0], c[1], c[2], 0x01]))
}

fn conn_complete(st: u8, h: u16, addr: &[u8; 6]) -> Vec<u8> {
    let mut p = vec![st];
    p.extend_from_slice(&h.to_le_bytes());
    p.extend_from_slice(addr);
    p.extend_from_slice(&[0x01, 0x00]);
    event(0x03, &p)
}

fn disconnected(h: u16, reason: u8) -> Vec<u8> {
    let hb = h.to_le_bytes();
    event(0x05, &[0, hb[0], hb[1], reason])
}

/// ACL packet(s) for one L2CAP frame on `cid`, cut into fragments of at
/// most `frag` bytes.
fn acl_frames(h: u16, cid: u16, payload: &[u8], frag: usize) -> Vec<Vec<u8>> {
    let mut l2 = Vec::new();
    l2.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    l2.extend_from_slice(&cid.to_le_bytes());
    l2.extend_from_slice(payload);
    l2.chunks(frag.max(1))
        .enumerate()
        .map(|(i, c)| acl_raw(h, if i == 0 { 0b10 } else { 0b01 }, c))
        .collect()
}

fn acl_raw(h: u16, pb: u16, data: &[u8]) -> Vec<u8> {
    let hdr = (h & 0x0FFF) | (pb << 12);
    let mut v = vec![0x02];
    v.extend_from_slice(&hdr.to_le_bytes());
    v.extend_from_slice(&(data.len() as u16).to_le_bytes());
    v.extend_from_slice(data);
    v
}

fn signal(code: u8, id: u8, data: &[u8]) -> Vec<u8> {
    let mut v = vec![code, id];
    v.extend_from_slice(&(data.len() as u16).to_le_bytes());
    v.extend_from_slice(data);
    v
}

fn u16s(xs: &[u16]) -> Vec<u8> {
    xs.iter().flat_map(|x| x.to_le_bytes()).collect()
}

// ------------------------------------------------------------ the pad

const PSM_CONTROL: u16 = 0x11;
const PSM_INTERRUPT: u16 = 0x13;

#[derive(Clone, Copy, Debug)]
struct Chan {
    psm: u16,
    ours: u16,
    theirs: Option<u16>,
    their_conf_done: bool,
    our_conf_done: bool,
}

impl Chan {
    fn open(&self) -> bool {
        self.theirs.is_some() && self.their_conf_done && self.our_conf_done
    }
}

/// The controller and the pad behind it, reacting to what the host sends.
struct World {
    rng: Rng,
    now: u64,
    /// Packets for the host, with when they arrive.
    to_host: Vec<(u64, Vec<u8>)>,
    /// The pad wants to be connected.
    pad_on: bool,
    /// The pad's link, if any.
    handle: Option<u16>,
    next_handle: u16,
    /// Handles the controller thinks are open (the pad's and strangers').
    live: Vec<u16>,
    pad_key: [u8; 16],
    encrypted: bool,
    chans: Vec<Chan>,
    next_cid: u16,
    sig_id: u8,
    next_call: u64,
    /// Switch start-up: full reports on, motion on.
    full: bool,
    motion: bool,
    next_report: u64,
    timer: u8,
    buttons_phase: u32,
    // Faults currently switched on.
    chaos: bool,
    drop_cmd_answers: u32,
    late: u64,
    endless_sdp: bool,
    silent_pad: bool,
    wrong_mode: bool,
    sdp_answers: u32,
    // The controller's link supervision: its timeout (ms; 20 s until the
    // host writes one), and the pad's link the controller still holds
    // after the pad was switched off without a word (no detach), with
    // since when nothing came from it.
    lsto_ms: u64,
    lsto_writes: u32,
    dead_link: Option<(u16, u64)>,
    /// The pad was switched on again while the controller still holds
    /// its dead link: pass its calls to the host (true) or not (false,
    /// what the owner's recording suggests).
    forward_calls_while_linked: bool,
    /// The pad sends only simple reports (0x3F), each `simple_every` ms:
    /// a mode the silence guard must not watch.
    simple_only: bool,
    // What happened (to show the run did what it says).
    chaos_steps: u64,
    links: u64,
    reports: u64,
}

impl World {
    fn new(seed: u64) -> Self {
        World {
            rng: Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1),
            now: 0,
            to_host: Vec::new(),
            pad_on: false,
            handle: None,
            next_handle: 0x0001,
            live: Vec::new(),
            pad_key: KEY,
            encrypted: false,
            chans: Vec::new(),
            next_cid: 0x0050,
            sig_id: 1,
            next_call: 0,
            full: false,
            motion: false,
            next_report: 0,
            timer: 0,
            buttons_phase: 0,
            chaos: false,
            drop_cmd_answers: 0,
            late: 0,
            endless_sdp: false,
            silent_pad: false,
            wrong_mode: false,
            sdp_answers: 0,
            lsto_ms: 20_000,
            lsto_writes: 0,
            dead_link: None,
            forward_calls_while_linked: false,
            simple_only: false,
            chaos_steps: 0,
            links: 0,
            reports: 0,
        }
    }

    fn put(&mut self, delay: u64, p: Vec<u8>) {
        let jitter = if self.late > 0 {
            self.rng.below(self.late)
        } else {
            0
        };
        self.to_host.push((self.now + delay + jitter, p));
    }

    fn handle_alloc(&mut self) -> u16 {
        let h = self.next_handle;
        self.next_handle = if h >= 0x0EFE { 1 } else { h + 1 };
        self.live.push(h);
        h
    }

    fn link_dropped(&mut self, h: u16) {
        self.live.retain(|&x| x != h);
        if self.handle == Some(h) {
            self.handle = None;
            self.encrypted = false;
            self.chans.clear();
            self.full = false;
            self.motion = false;
            self.next_call = self.now + 1_500;
            self.lsto_ms = 20_000;
        }
        if self.dead_link.is_some_and(|(d, _)| d == h) {
            self.dead_link = None;
        }
    }

    /// The pad is switched off: it says nothing (no detach); the
    /// controller keeps the link until its supervision timeout.
    fn switch_off(&mut self) {
        self.pad_on = false;
        if let Some(h) = self.handle {
            self.dead_link = Some((h, self.now));
        }
    }

    /// The controller's own timers each millisecond: a dead link ends at
    /// its supervision timeout (reason 0x08).
    fn controller_step(&mut self) {
        if let Some((h, since)) = self.dead_link
            && self.now >= since + self.lsto_ms
        {
            self.put(0, disconnected(h, 0x08));
            self.link_dropped(h);
        }
    }

    fn pad_send(&mut self, cid: u16, payload: &[u8]) {
        let Some(h) = self.handle else { return };
        let frag = if self.rng.chance(200) {
            1 + self.rng.below(40) as usize
        } else {
            1021
        };
        for f in acl_frames(h, cid, payload, frag) {
            self.put(2, f);
        }
    }

    fn pad_signal(&mut self, code: u8, data: &[u8]) {
        let id = self.sig_id;
        self.sig_id = self.sig_id.wrapping_add(1).max(1);
        self.pad_send(0x0001, &signal(code, id, data));
    }

    fn pad_open(&mut self, psm: u16) {
        let ours = self.next_cid;
        self.next_cid = self.next_cid.wrapping_add(1).max(0x40);
        self.chans.push(Chan {
            psm,
            ours,
            theirs: None,
            their_conf_done: false,
            our_conf_done: false,
        });
        self.pad_signal(0x02, &u16s(&[psm, ours]));
    }

    fn chan_open(&self, psm: u16) -> Option<Chan> {
        self.chans
            .iter()
            .copied()
            .find(|c| c.psm == psm && c.open())
    }

    // ---- what the host sent

    fn take(&mut self, wire: &mut Wire) {
        while let Some(p) = wire.out.pop_front() {
            match p[0] {
                0x01 => self.command(&p),
                0x02 => self.acl_from_host(&p),
                _ => unreachable!(),
            }
        }
    }

    fn command(&mut self, p: &[u8]) {
        let opcode = u16::from_le_bytes([p[1], p[2]]);
        let a = &p[4..];
        if self.drop_cmd_answers > 0 {
            // An answer that never comes.
            self.drop_cmd_answers -= 1;
            return;
        }
        let addr = |a: &[u8]| -> [u8; 6] { a[..6].try_into().unwrap() };
        // A command naming a link the controller does not have: "Unknown
        // Connection Identifier", as a real controller answers.
        if matches!(
            opcode,
            0x0406 | 0x0411 | 0x0413 | 0x041B | 0x041C | 0x041D | 0x1408
        ) {
            let h = u16::from_le_bytes([a[0], a[1]]);
            if !self.live.contains(&h) {
                if opcode == 0x1408 {
                    self.put(1, complete(opcode, &[0x02, a[0], a[1], 0]));
                } else {
                    self.put(1, status(opcode, 0x02));
                }
                return;
            }
        }
        match opcode {
            0x0409 => {
                // Accept: the gate already made sure it is the pad.
                self.put(1, status(opcode, 0));
                if self.handle.is_none() && self.pad_on {
                    let h = self.handle_alloc();
                    self.handle = Some(h);
                    self.links += 1;
                    self.put(20, conn_complete(0, h, &PAD));
                    // The pad starts security: the controller asks for
                    // the key.
                    self.put(40, event(0x17, &PAD));
                    // And switches roles: the console becomes central
                    // (as every call in the owner's recording).
                    let mut rc = vec![0];
                    rc.extend_from_slice(&PAD);
                    rc.push(0);
                    self.put(45, event(0x12, &rc));
                } else if self.dead_link.is_some() {
                    // The controller still holds the pad's old link:
                    // "connection already exists".
                    self.put(20, conn_complete(0x0B, 0, &addr(a)));
                } else {
                    self.put(20, conn_complete(0x02, 0, &addr(a)));
                }
            }
            0x040A => {
                self.put(1, status(opcode, 0));
                self.put(5, conn_complete(a[6], 0, &addr(a)));
            }
            0x0406 => {
                let h = u16::from_le_bytes([a[0], a[1]]);
                self.put(1, status(opcode, 0));
                if self.dead_link.is_some_and(|(d, _)| d == h) {
                    // A dead pad never acknowledges the detach: the link
                    // ends at the supervision timeout (controller_step).
                } else if self.live.contains(&h) {
                    self.put(30, disconnected(h, 0x16));
                    self.link_dropped(h);
                }
            }
            0x040B => {
                let mut r = vec![0];
                r.extend_from_slice(&a[..6]);
                self.put(1, complete(opcode, &r));
                let key: [u8; 16] = a[6..22].try_into().unwrap();
                if let Some(h) = self.handle {
                    let hb = h.to_le_bytes();
                    if key == self.pad_key {
                        self.put(20, event(0x06, &[0, hb[0], hb[1]]));
                        self.put(40, event(0x08, &[0, hb[0], hb[1], 1]));
                        self.encrypted = true;
                        self.next_call = self.now + 60;
                    } else {
                        self.put(20, event(0x06, &[0x05, hb[0], hb[1]]));
                        self.put(40, disconnected(h, 0x05));
                        self.link_dropped(h);
                    }
                }
            }
            0x040C | 0x040E | 0x042D | 0x042F | 0x0434 => {
                let mut r = vec![0];
                r.extend_from_slice(&a[..6]);
                self.put(1, complete(opcode, &r));
            }
            0x0411 => {
                self.put(1, status(opcode, 0));
                if self.handle.is_some() {
                    self.put(20, event(0x17, &PAD));
                }
            }
            0x0413 => {
                self.put(1, status(opcode, 0));
                if let Some(h) = self.handle {
                    let hb = h.to_le_bytes();
                    self.put(30, event(0x08, &[0, hb[0], hb[1], a[2]]));
                    self.encrypted = a[2] != 0;
                    self.next_call = self.now + 60;
                }
            }
            0x0419 => {
                self.put(1, status(opcode, 0));
                let mut r = vec![0];
                r.extend_from_slice(&PAD);
                let mut name = b"Pro Controller".to_vec();
                name.resize(248, 0);
                r.extend_from_slice(&name);
                self.put(50, event(0x07, &r));
            }
            0x041B..=0x041D => {
                self.put(1, status(opcode, 0x0C));
            }
            0x1408 => {
                self.put(1, complete(opcode, &[0, a[0], a[1], 16]));
            }
            0x0C37 => {
                let h = u16::from_le_bytes([a[0], a[1]]);
                if self.live.contains(&h) {
                    if self.handle == Some(h) {
                        self.lsto_ms = u64::from(u16::from_le_bytes([a[2], a[3]])) * 625 / 1000;
                        self.lsto_writes += 1;
                    }
                    self.put(1, complete(opcode, &[0, a[0], a[1]]));
                } else {
                    self.put(1, complete(opcode, &[0x02, a[0], a[1]]));
                }
            }
            0x1005 => self.put(1, complete(opcode, &[0, 0xFD, 0x03, 0, 9, 0, 0, 0])),
            0x1009 => {
                let mut r = vec![0];
                r.extend_from_slice(&OWN);
                self.put(1, complete(opcode, &r));
            }
            0x1001 => self.put(1, complete(opcode, &[0, 8, 0, 0, 8, 0x60, 0, 0, 0])),
            0x1003 => self.put(
                1,
                complete(opcode, &[0, 0xFF, 0xFF, 0x8F, 0xFE, 0xDB, 0xFF, 0x7B, 0x87]),
            ),
            _ => self.put(1, complete(opcode, &[0])),
        }
    }

    fn acl_from_host(&mut self, p: &[u8]) {
        let hdr = u16::from_le_bytes([p[1], p[2]]);
        let h = hdr & 0x0FFF;
        let hb = h.to_le_bytes();
        // Buffers back.
        self.put(3, event(0x13, &[1, hb[0], hb[1], 1, 0]));
        if Some(h) != self.handle || self.silent_pad {
            return;
        }
        // The host's frames fit in one fragment (its MTU is small).
        let data = &p[5..];
        if data.len() < 4 {
            return;
        }
        let cid = u16::from_le_bytes([data[2], data[3]]);
        let body = data[4..].to_vec();
        if cid == 0x0001 {
            self.signals(&body);
        } else if let Some(c) = self.chans.iter().copied().find(|c| c.ours == cid) {
            match c.psm {
                0x0001 => self.sdp(c, &body),
                _ => self.hid_output(c, &body),
            }
        }
    }

    fn signals(&mut self, mut b: &[u8]) {
        while b.len() >= 4 {
            let code = b[0];
            let id = b[1];
            let n = usize::from(u16::from_le_bytes([b[2], b[3]]));
            if b.len() < 4 + n {
                return;
            }
            let d = &b[4..4 + n];
            let w = |i: usize| u16::from_le_bytes([d[i], d[i + 1]]);
            match code {
                0x02 if n >= 4 => {
                    // The host opens a channel (the search).
                    let (psm, theirs) = (w(0), w(2));
                    let ours = self.next_cid;
                    self.next_cid = self.next_cid.wrapping_add(1).max(0x40);
                    self.chans.push(Chan {
                        psm,
                        ours,
                        theirs: Some(theirs),
                        their_conf_done: false,
                        our_conf_done: false,
                    });
                    let rsp = signal(0x03, id, &u16s(&[ours, theirs, 0, 0]));
                    self.pad_send(0x0001, &rsp);
                    self.pad_signal(0x04, &u16s(&[theirs, 0]));
                }
                0x03 if n >= 8 => {
                    let (theirs, ours, result) = (w(0), w(2), w(4));
                    if let Some(c) = self.chans.iter_mut().find(|c| c.ours == ours) {
                        if result == 0 {
                            c.theirs = Some(theirs);
                            self.pad_signal(0x04, &u16s(&[theirs, 0]));
                        } else if result != 1 {
                            self.chans.retain(|c| c.ours != ours);
                        }
                    }
                }
                0x04 if n >= 4 => {
                    let ours = w(0);
                    let theirs = self
                        .chans
                        .iter_mut()
                        .find(|c| c.ours == ours)
                        .and_then(|c| {
                            c.their_conf_done = true;
                            c.theirs
                        })
                        .unwrap_or(0);
                    let rsp = signal(0x05, id, &u16s(&[theirs, 0, 0]));
                    self.pad_send(0x0001, &rsp);
                }
                0x05 if n >= 6 => {
                    let ours = w(0);
                    if let Some(c) = self.chans.iter_mut().find(|c| c.ours == ours) {
                        c.our_conf_done = true;
                    }
                }
                0x06 if n >= 4 => {
                    let (ours, theirs) = (w(0), w(2));
                    self.chans.retain(|c| c.ours != ours);
                    let rsp = signal(0x07, id, &u16s(&[ours, theirs]));
                    self.pad_send(0x0001, &rsp);
                }
                0x07 if n >= 4 => {
                    let theirs = w(2);
                    self.chans.retain(|c| c.ours != theirs);
                }
                0x0A if n >= 2 => {
                    let rsp = signal(0x0B, id, &u16s(&[w(0), 1]));
                    self.pad_send(0x0001, &rsp);
                }
                0x08 => {
                    let rsp = signal(0x09, id, d);
                    self.pad_send(0x0001, &rsp);
                }
                _ => {}
            }
            b = &b[4 + n..];
        }
    }

    fn sdp(&mut self, c: Chan, req: &[u8]) {
        let Some(theirs) = c.theirs else { return };
        if req.len() < 5 {
            return;
        }
        self.sdp_answers += 1;
        let pdu = req[0] + 1;
        let mut params = vec![0x00, 0x02, 0x35, 0x00];
        if self.endless_sdp {
            // A continuation that never ends (16 bytes, the most).
            params.push(16);
            params.extend(self.rng.bytes(16));
        } else {
            params.push(0);
        }
        let mut rsp = vec![pdu, req[1], req[2]];
        rsp.extend_from_slice(&(params.len() as u16).to_be_bytes());
        rsp.extend_from_slice(&params);
        self.pad_send(theirs, &rsp);
    }

    fn hid_output(&mut self, c: Chan, b: &[u8]) {
        // An output report (interrupt 0xA2, or SET_REPORT 0x52 on the
        // control channel) carrying report 0x01: a subcommand.
        if b.len() < 12 || !matches!(b[0], 0xA2 | 0x52) || b[1] != 0x01 {
            return;
        }
        if b[0] == 0x52
            && let Some(t) = c.theirs
        {
            self.pad_send(t, &[0x00]);
        }
        let sub = b[11];
        let arg = b.get(12).copied().unwrap_or(0);
        match (sub, arg) {
            (0x03, 0x30) => self.full = true,
            (0x40, 0x01) => self.motion = true,
            _ => {}
        }
        if self.wrong_mode {
            return;
        }
        let mut r = vec![0u8; 49];
        r[0] = 0x21;
        r[1] = self.timer;
        r[2] = 0x8E;
        r[13] = if sub == 0x02 { 0x82 } else { 0x80 };
        r[14] = sub;
        if sub == 0x02 {
            // Device info, with the pad's own address (high byte first):
            // the host may log the answer, never this address.
            r[15] = 0x04;
            r[16] = 0x21;
            r[17] = 0x03;
            r[18] = 0x02;
            for i in 0..6 {
                r[19 + i] = PAD[5 - i];
            }
            r[25] = 0x01;
        }
        let mut msg = vec![0xA1];
        msg.extend_from_slice(&r);
        if let Some(i) = self.chan_open(PSM_INTERRUPT)
            && let Some(t) = i.theirs
        {
            self.pad_send(t, &msg);
        }
    }

    /// The pad's own doings each millisecond step.
    fn pad_step(&mut self) {
        if !self.pad_on || self.silent_pad {
            return;
        }
        if self.dead_link.is_some() {
            // Switched on again; the controller still holds its old link.
            if self.forward_calls_while_linked && self.now >= self.next_call {
                self.put(1, conn_request(&PAD, PAD_COD));
                self.next_call = self.now + 1_280;
            }
            return;
        }
        if self.handle.is_none() {
            if self.now >= self.next_call {
                self.put(1, conn_request(&PAD, PAD_COD));
                self.next_call = self.now + 2_000;
            }
            return;
        }
        if !self.encrypted || self.now < self.next_call {
            return;
        }
        let ctl = self.chans.iter().find(|c| c.psm == PSM_CONTROL).copied();
        let intr = self.chans.iter().find(|c| c.psm == PSM_INTERRUPT).copied();
        match (ctl, intr) {
            (None, _) => {
                self.pad_open(PSM_CONTROL);
                self.next_call = self.now + 300;
            }
            (Some(c), None) if c.open() => {
                self.pad_open(PSM_INTERRUPT);
                self.next_call = self.now + 300;
            }
            (Some(c), Some(i)) if c.open() && i.open() && self.now >= self.next_report => {
                self.next_report = self.now + if self.simple_only { 500 } else { 15 };
                self.timer = self.timer.wrapping_add(1);
                let t = i.theirs.unwrap();
                let msg = if self.wrong_mode {
                    // An Xbox-like report on a Switch pad.
                    let mut m = vec![0xA1, 0x01];
                    m.extend(self.rng.bytes(15));
                    m
                } else if self.full && !self.simple_only {
                    self.buttons_phase = self.buttons_phase.wrapping_add(1);
                    let s = SwitchInput {
                        buttons: if self.buttons_phase % 8 < 2 {
                            1 << 0
                        } else {
                            0
                        },
                        left: (1000, -1000),
                        right: (0, 0),
                        accel: if self.motion { [0, 0, 4096] } else { [0; 3] },
                        gyro: if self.motion { [16, 0, 0] } else { [0; 3] },
                        battery: 3,
                        charging: false,
                        timer: self.timer,
                    };
                    let mut m = vec![0xA1];
                    m.extend_from_slice(&switch_report(&s));
                    m
                } else {
                    let mut m = vec![0xA1, 0x3F, 0, 0, 0x08];
                    m.extend_from_slice(&[0x00, 0x80, 0x00, 0x80, 0x00, 0x80, 0x00, 0x80]);
                    m
                };
                self.reports += 1;
                self.pad_send(t, &msg);
            }
            _ => {}
        }
    }

    // ---- faults

    fn chaos_step(&mut self) {
        self.chaos_steps += 1;
        let r = self.rng.below(1000);
        let h = self.handle.unwrap_or(0x0001);
        let hb = h.to_le_bytes();
        let pick_addr = |rng: &mut Rng| -> [u8; 6] {
            if rng.chance(500) {
                PAD
            } else {
                STRANGERS[rng.below(3) as usize]
            }
        };
        match r {
            // Calls from strangers and from the pad, sometimes a flood.
            0..=59 => {
                let n = if self.rng.chance(200) { 40 } else { 1 };
                for _ in 0..n {
                    let a = pick_addr(&mut self.rng);
                    let cod = self.rng.pick(&[PAD_COD, 0x5A_020C, 0x24_0404, 0]);
                    let d = self.rng.below(5);
                    self.put(d, conn_request(&a, cod));
                }
            }
            // Pairing: every question of it, from the pad or a stranger.
            60..=119 => {
                let a = pick_addr(&mut self.rng);
                let ev = match self.rng.below(7) {
                    0 => event(0x31, &a),
                    1 => event(0x32, &with_addr(&a, &[0x03, 0, 0x00])),
                    2 => event(0x33, &with_addr(&a, &[1, 2, 3, 4])),
                    3 => event(0x16, &a),
                    4 => event(0x34, &a),
                    5 => {
                        let mut k = vec![0u8; 16];
                        k.copy_from_slice(&WRONG_KEY);
                        k.push(0x04);
                        event(0x18, &with_addr(&a, &k))
                    }
                    _ => event(0x17, &a),
                };
                self.put(1, ev);
            }
            // Authentication and encryption fail.
            120..=149 => {
                let ev = match self.rng.below(4) {
                    0 => event(0x06, &[0x05, hb[0], hb[1]]),
                    1 => event(0x08, &[0x06, hb[0], hb[1], 0]),
                    2 => event(0x08, &[0x00, hb[0], hb[1], 0]),
                    _ => event(0x30, &[0x00, hb[0], hb[1]]),
                };
                self.put(1, ev);
            }
            // The next key is wrong (the pad "lost" its pairing).
            150..=159 => self.pad_key = WRONG_KEY,
            // A link drops, a wrong handle drops, a stranger's link
            // completes out of nowhere.
            160..=189 => {
                let ev = match self.rng.below(4) {
                    0 => {
                        if let Some(h) = self.handle {
                            self.link_dropped(h);
                        }
                        disconnected(h, 0x08)
                    }
                    1 => disconnected(self.rng.below(0x1000) as u16, 0x13),
                    2 => {
                        let a = STRANGERS[self.rng.below(3) as usize];
                        let nh = self.handle_alloc();
                        conn_complete(0, nh, &a)
                    }
                    _ => conn_complete(0, h, &PAD),
                };
                self.put(1, ev);
            }
            // Random well-framed events, some with a live handle or the
            // pad's address where one is read.
            190..=389 => {
                let code = if self.rng.chance(700) {
                    self.rng.pick(&[
                        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x0B, 0x0C, 0x0E, 0x0F,
                        0x10, 0x12, 0x13, 0x14, 0x16, 0x17, 0x18, 0x1A, 0x1B, 0x20, 0x22, 0x23,
                        0x2F, 0x30, 0x31, 0x32, 0x33, 0x34, 0x36, 0x38, 0x3D, 0x3E, 0xFF,
                    ])
                } else {
                    self.rng.byte()
                };
                let n = self
                    .rng
                    .pick(&[0usize, 1, 2, 3, 4, 6, 7, 10, 11, 14, 15, 255]);
                let mut p = self.rng.bytes(n);
                if n >= 3 && self.rng.chance(500) {
                    p[1] = hb[0];
                    p[2] = hb[1] & 0x0F;
                }
                // The pad's address where the event carries an address
                // (only there: elsewhere the same bytes are just data).
                let at = match code {
                    0x04 | 0x16 | 0x17 | 0x18 | 0x20 | 0x31 | 0x32 | 0x33 | 0x34 | 0x3D => Some(0),
                    0x07 | 0x12 | 0x36 => Some(1),
                    0x03 => Some(3),
                    _ => None,
                };
                if let Some(at) = at
                    && n >= at + 6
                    && self.rng.chance(300)
                {
                    p[at..at + 6].copy_from_slice(&PAD);
                }
                // Credits are never 0 here: a controller that hands out
                // none stops every command (tested on its own below).
                if (code == 0x0E || code == 0x0F) && n >= 2 {
                    let at = if code == 0x0E { 0 } else { 1 };
                    if p[at] == 0 {
                        p[at] = 1;
                    }
                }
                self.put(1, event(code, &p));
            }
            // ACL chaos.
            390..=589 => {
                let which = self.rng.below(8);
                let mut out: Vec<Vec<u8>> = Vec::new();
                let h2 = if self.rng.chance(800) {
                    h
                } else {
                    self.rng.below(0x1000) as u16
                };
                match which {
                    // L2CAP length that lies (longer than sent).
                    0 => {
                        let n = self.rng.below(30) as usize;
                        let mut d = (self.rng.below(2000) as u16).to_le_bytes().to_vec();
                        d.extend_from_slice(&u16s(&[self.rng.pick(&[1, 0x40, 0x41, 0x50])]));
                        d.extend(self.rng.bytes(n));
                        out.push(acl_raw(h2, 0b10, &d));
                    }
                    // A start that never ends, then more starts.
                    1 => {
                        let mut d = 1000u16.to_le_bytes().to_vec();
                        d.extend_from_slice(&u16s(&[0x0001]));
                        d.extend(self.rng.bytes(10));
                        out.push(acl_raw(h2, 0b10, &d));
                    }
                    // Continuations with no start, a long run of them.
                    2 => {
                        for _ in 0..self.rng.below(200) {
                            let n = self.rng.below(60) as usize;
                            out.push(acl_raw(h2, 0b01, &self.rng.bytes(n)));
                        }
                    }
                    // A flood of channel requests from the pad.
                    3 => {
                        for i in 0..self.rng.below(60) {
                            let psm = self.rng.pick(&[0x11, 0x13, 0x01, 0x03, 0x1001, 0]);
                            let s = signal(0x02, i as u8, &u16s(&[psm, 0x40 + i as u16]));
                            out.extend(acl_frames(h2, 1, &s, 1021));
                        }
                    }
                    // Configuration with odd options: MTU 0, 65535, an
                    // unknown option, an option whose length lies.
                    4 => {
                        let c = self
                            .chans
                            .first()
                            .map_or(0x40, |c| c.theirs.unwrap_or(0x40));
                        let opts = self.rng.pick(&[
                            &[0x01u8, 0x02, 0x00, 0x00][..],
                            &[0x01, 0x02, 0xFF, 0xFF],
                            &[0x7F, 0x03, 1, 2, 3],
                            &[0x01, 0xFE, 0x30],
                            &[0x02, 0x02],
                        ]);
                        let mut d = u16s(&[c, 0]);
                        d.extend_from_slice(opts);
                        out.extend(acl_frames(h2, 1, &signal(0x04, 0x77, &d), 1021));
                        let mut r = u16s(&[c, 0, self.rng.pick(&[0, 1, 2, 3, 4])]);
                        r.extend_from_slice(opts);
                        out.extend(acl_frames(h2, 1, &signal(0x05, 0x78, &r), 1021));
                    }
                    // Signals with lying inner lengths, unknown codes.
                    5 => {
                        let mut s = vec![self.rng.byte(), self.rng.byte()];
                        s.extend_from_slice(&(self.rng.below(4000) as u16).to_le_bytes());
                        s.extend(self.rng.some_bytes(20));
                        out.extend(acl_frames(h2, 1, &s, 1021));
                    }
                    // HID: wrong-mode and short reports, a virtual cable
                    // unplug, empty frames, on the open channels.
                    6 => {
                        let intr = self.chan_open(PSM_INTERRUPT).and_then(|c| c.theirs);
                        let ctl = self.chan_open(PSM_CONTROL).and_then(|c| c.theirs);
                        let cid = if self.rng.chance(500) { intr } else { ctl };
                        let cid = cid.unwrap_or(0x41);
                        let m: Vec<u8> = match self.rng.below(8) {
                            0 => vec![],
                            1 => vec![0xA1],
                            2 => vec![0xA1, 0x30],
                            3 => vec![0xA1, 0x30, 1, 2, 3],
                            4 => {
                                let mut m = vec![0xA1, 0x21];
                                m.extend(self.rng.bytes(14));
                                m[14] = 0x82;
                                m
                            }
                            5 => vec![0x15],
                            6 => {
                                let mut m = vec![0xA1, 0x01];
                                m.extend(self.rng.some_bytes(70));
                                m
                            }
                            _ => self.rng.some_bytes(100),
                        };
                        out.extend(acl_frames(h2, cid, &m, 1021));
                    }
                    // A whole random ACL frame, sometimes as long as the
                    // board lets through.
                    _ => {
                        let n = if self.rng.chance(100) {
                            1021
                        } else {
                            self.rng.below(80) as usize
                        };
                        let pb = self.rng.below(4) as u16;
                        out.push(acl_raw(h2, pb, &self.rng.bytes(n)));
                    }
                }
                for p in out {
                    self.put(1, p);
                }
            }
            // Modes that last a while.
            590..=619 => self.drop_cmd_answers = 1 + self.rng.below(3) as u32,
            620..=649 => self.late = if self.late == 0 { 300 } else { 0 },
            650..=669 => self.endless_sdp = !self.endless_sdp,
            670..=689 => self.silent_pad = !self.silent_pad,
            690..=709 => self.wrong_mode = !self.wrong_mode,
            710..=729 => self.pad_on = !self.pad_on,
            _ => {}
        }
    }

    fn calm(&mut self) {
        self.chaos = false;
        self.drop_cmd_answers = 0;
        self.late = 0;
        self.endless_sdp = false;
        self.silent_pad = false;
        self.wrong_mode = false;
        self.pad_key = KEY;
    }
}

// ------------------------------------------------------------ the run

struct Run {
    host: Box<PadHost>,
    wire: Wire,
    w: World,
    flash: Vec<u8>,
    crc_before: u32,
    raw_rng: Rng,
    /// Faults per thousand milliseconds (HOSTILE_RATE).
    rate: u64,
    /// The longest time from "every link dropped" to waiting.
    back_ms_max: u64,
}

impl Run {
    fn new(seed: u64) -> Self {
        let flash = flash_with_pad();
        let crc_before = pad_store::crc::crc32(&flash[0x9000..0xF000]);
        let mut host = Box::new(PadHost::new());
        let mut wire = Wire::new(seed);
        let region = check_region(NVS, &[], 0x40_0000).unwrap();
        let loaded = host
            .load_pairings(&mut ReadView(&flash), region, &mut wire)
            .unwrap();
        assert_eq!(loaded.keys, 1);
        wire.begin_call();
        host.start(0, &mut wire);
        wire.end_call();
        Run {
            host,
            wire,
            w: World::new(seed),
            flash,
            crc_before,
            raw_rng: Rng(seed ^ 0x5555_AAAA_1234_5678 | 1),
            rate: env("HOSTILE_RATE", 4),
            back_ms_max: 0,
        }
    }

    /// One millisecond.
    fn step(&mut self) {
        self.w.now += 1;
        let now = self.w.now;
        self.w.take(&mut self.wire);
        self.w.controller_step();
        // Deliver what is due, in order of time (equal times keep their
        // order).
        self.w.to_host.sort_by_key(|(t, _)| *t);
        let due = self.w.to_host.partition_point(|(t, _)| *t <= now);
        let batch: Vec<Vec<u8>> = self.w.to_host.drain(..due).map(|(_, p)| p).collect();
        for p in batch {
            // The board drops a packet longer than 1027 bytes or one
            // whose header does not match it (boards/cyd/src/hci.rs).
            if p.len() >= hci_rules::MAX_PACKET || hci_rules::packet_len(&p) != Some(p.len()) {
                continue;
            }
            // The board's poll: the gate notes a saved pad's link first
            // (boards/cyd/src/pad_radio.rs), then the host reads it.
            self.wire.gate.observe(&p);
            self.wire.begin_call();
            self.host.packet(now, &p, &mut self.wire);
            self.wire.end_call();
            self.w.take(&mut self.wire);
        }
        if self.w.chaos {
            if self.w.rng.chance(self.rate) {
                self.w.chaos_step();
            }
            // Bytes that are not even packets, straight to the host.
            if self.raw_rng.chance(self.rate.div_ceil(2)) {
                self.wire.t.raw += 1;
                let n = self.raw_rng.below(1100) as usize;
                let mut b = self.raw_rng.bytes(n);
                if let Some(k) = b.first_mut() {
                    *k = self.raw_rng.pick(&[0x02, 0x04, 0x01, 0x03, 0x00]);
                }
                self.wire.begin_call();
                self.host.packet(now, &b, &mut self.wire);
                self.wire.end_call();
            }
        }
        self.w.pad_step();
        if now.is_multiple_of(15) {
            self.wire.begin_call();
            self.host.tick(now, &mut self.wire);
            self.wire.end_call();
            let _ = self.host.read();
            let _ = self.host.motion();
            let _ = self.host.battery();
            while self.host.take_event().is_some() {}
        }
    }

    fn run_ms(&mut self, ms: u64) {
        for _ in 0..ms {
            self.step();
        }
    }

    fn until(&mut self, limit_ms: u64, f: impl Fn(&PadHost) -> bool) -> bool {
        for _ in 0..limit_ms {
            if f(&self.host) {
                return true;
            }
            self.step();
        }
        f(&self.host)
    }

    fn crc_same(&self) {
        assert_eq!(
            pad_store::crc::crc32(&self.flash[0x9000..0xF000]),
            self.crc_before
        );
    }
}

fn clean_reconnect(r: &mut Run, seed: u64) {
    r.w.calm();
    r.w.pad_on = true;
    r.w.next_call = r.w.now;
    let ok = r.until(20_000, |h| {
        h.connected() && h.mode() == Mode::Switch && h.motion().is_some()
    });
    assert!(
        ok,
        "seed {seed}: no clean reconnect (state {}, phase {})\n{}",
        r.host.state(),
        r.host.phase(),
        r.wire.tail()
    );
    // Buttons come through.
    let mut seen = 0;
    for _ in 0..300 {
        r.step();
        seen |= r.host.take_pressed();
    }
    assert_ne!(seen, 0, "seed {seed}: no press after the reconnect");
}

/// The longest a host may take to be back to waiting after every link
/// dropped: its longest own timer (security, 30 s) plus the disconnect
/// guard (5 s) and some.
const BACK_LIMIT_MS: u64 = 40_000;

fn drop_everything(r: &mut Run, seed: u64) {
    r.w.calm();
    r.w.pad_on = false;
    let live = std::mem::take(&mut r.w.live);
    for h in live {
        r.w.put(1, disconnected(h, 0x08));
    }
    r.w.handle = None;
    r.w.chans.clear();
    r.w.encrypted = false;
    r.run_ms(200);
    let _ = r.host.take_pressed();
    let from = r.w.now;
    let back = r.until(BACK_LIMIT_MS, |h| {
        h.waiting() && !h.connected() && h.state() == "waiting"
    });
    let took = r.w.now - from;
    r.back_ms_max = r.back_ms_max.max(took);
    // And it stays there with nothing calling.
    r.run_ms(2_000);
    assert!(
        back && r.host.waiting() && r.host.state() == "waiting",
        "seed {seed}: not back to waiting (state {}, phase {})\n{}",
        r.host.state(),
        r.host.phase(),
        r.wire.tail()
    );
    assert_eq!(r.host.read(), board_api::PadState::NONE, "seed {seed}");
    assert_eq!(r.host.take_pressed(), 0, "seed {seed}");
    assert!(r.host.motion().is_none(), "seed {seed}");
}

fn one_seed(seed: u64, chaos_secs: u64) -> Totals {
    let mut r = Run::new(seed);
    assert!(
        r.until(3_000, |h| h.waiting()),
        "seed {seed}: no set-up\n{}",
        r.wire.tail()
    );
    // A good start first, so the chaos hits every phase.
    clean_reconnect(&mut r, seed);
    r.w.chaos = true;
    for _ in 0..chaos_secs {
        r.run_ms(1_000);
        // The pad keeps calling now and then, so links come and go.
        if !r.w.pad_on && r.w.rng.chance(300) {
            r.w.pad_on = true;
        }
    }
    drop_everything(&mut r, seed);
    clean_reconnect(&mut r, seed);
    r.crc_same();
    let mut t = r.wire.t;
    t.chaos_steps = r.w.chaos_steps;
    t.links = r.w.links;
    t.reports = r.w.reports;
    t.sdp_answers = u64::from(r.w.sdp_answers);
    t.back_ms_max = r.back_ms_max;
    t
}

#[test]
fn hostile_radio_never_panics_leaks_or_sticks() {
    let seeds = env("HOSTILE_SEEDS", 24);
    let secs = env("HOSTILE_SECS", 20);
    let first = env("HOSTILE_FIRST", 1);
    let mut all = Totals::default();
    for seed in first..first + seeds {
        let t = one_seed(seed, secs);
        all.sends += t.sends;
        all.lines += t.lines;
        all.longest_line = all.longest_line.max(t.longest_line);
        all.most_sends_per_call = all.most_sends_per_call.max(t.most_sends_per_call);
        all.most_lines_per_call = all.most_lines_per_call.max(t.most_lines_per_call);
        all.refused += t.refused;
        all.chaos_steps += t.chaos_steps;
        all.links += t.links;
        all.reports += t.reports;
        all.sdp_answers += t.sdp_answers;
        all.raw += t.raw;
        all.back_ms_max = all.back_ms_max.max(t.back_ms_max);
    }
    println!(
        "hostile radio: {seeds} seeds x {secs} s chaos: {} sends, {} log lines, longest line {} bytes, most sends in one call {}, most log lines in one call {}, gate refusals {}, PadHost {} bytes; {} faults, {} raw byte runs, {} links accepted, {} reports, {} search answers; longest way back to waiting {} ms",
        all.sends,
        all.lines,
        all.longest_line,
        all.most_sends_per_call,
        all.most_lines_per_call,
        all.refused,
        std::mem::size_of::<PadHost>(),
        all.chaos_steps,
        all.raw,
        all.links,
        all.reports,
        all.sdp_answers,
        all.back_ms_max
    );
    assert!(all.links >= 2 * seeds && all.chaos_steps > 0 && all.raw > 0);
    assert!(all.longest_line < 600);
}

/// A controller that answers with zero command credits and never gives
/// any back (a broken controller, not a remote device): the host must
/// not panic; this test records what it does.
#[test]
fn zero_credit_controller_does_not_panic() {
    let mut r = Run::new(9_999);
    assert!(r.until(3_000, |h| h.waiting()));
    r.w.put(1, event(0x0E, &[0x00, 0x00, 0x00]));
    r.w.put(2, event(0x0F, &[0x00, 0x00, 0x00, 0x00]));
    r.run_ms(100);
    r.w.pad_on = true;
    r.w.next_call = r.w.now;
    let connected = r.until(40_000, |h| h.connected());
    println!(
        "zero credits: connected={connected} state={} phase={}",
        r.host.state(),
        r.host.phase()
    );
}

/// Every chaos case on its own a few hundred times, from a connected pad.
#[test]
fn each_fault_alone_comes_back() {
    let reps = env("HOSTILE_REPS", 3);
    for case in 0..73u64 {
        for rep in 0..reps {
            let seed = 10_000 + case * 100 + rep;
            let mut r = Run::new(seed);
            assert!(r.until(3_000, |h| h.waiting()));
            clean_reconnect(&mut r, seed);
            // Force the case: chaos_step picks by r in [case*10, case*10+9].
            for _ in 0..20 {
                let saved = r.w.rng.0;
                // Find a draw that lands on this case.
                let mut tries = 0;
                loop {
                    let probe = Rng(r.w.rng.0).next() % 1000;
                    if probe / 10 == case {
                        break;
                    }
                    r.w.rng.next();
                    tries += 1;
                    if tries > 100_000 {
                        r.w.rng.0 = saved;
                        break;
                    }
                }
                r.w.chaos_step();
                r.run_ms(250);
            }
            drop_everything(&mut r, seed);
            clean_reconnect(&mut r, seed);
            r.crc_same();
        }
    }
}

// ------------------------------------------------- the owner's case (round 6)

/// What one switch-off and switch-on took, in milliseconds.
struct OffOn {
    /// From switch-off until the host no longer shows the pad.
    gone: u64,
    /// From switch-off until the controller let the old link go.
    freed: u64,
    /// From switch-on until the pad is connected again.
    back: u64,
}

/// The owner's case: a linked pad is switched off without a word (no
/// detach, as the owner's recording shows: reason 0x08), then on again
/// `on_after` ms later. The controller holds the dead link until its
/// supervision timeout (20 s until the host writes one) and, unless
/// `forward`, does not pass the pad's calls on meanwhile. `simple`: the
/// pad sends only simple reports (0x3F, twice a second), a mode the
/// silence guard leaves alone.
fn owner_off_on(seed: u64, simple: bool, forward: bool, on_after: u64) -> OffOn {
    let mut r = Run::new(seed);
    assert!(r.until(3_000, |h| h.waiting()));
    clean_reconnect(&mut r, seed);
    r.w.simple_only = simple;
    r.w.forward_calls_while_linked = forward;
    r.run_ms(1_500);
    assert!(
        r.host.connected(),
        "seed {seed}: lost before the switch-off"
    );
    assert_eq!(r.w.lsto_writes, 1, "seed {seed}: the timeout written once");
    assert_eq!(r.w.lsto_ms, 2_000, "seed {seed}: 2 s supervision timeout");
    let off = r.w.now;
    r.w.switch_off();
    let mut gone = None;
    let mut freed = None;
    let mut on_at = None;
    let mut back = None;
    while r.w.now < off + 30_000 && back.is_none() {
        r.step();
        let t = r.w.now - off;
        if gone.is_none() && !r.host.connected() {
            gone = Some(t);
        }
        if freed.is_none() && r.w.dead_link.is_none() {
            freed = Some(t);
        }
        if on_at.is_none() && t >= on_after {
            r.w.pad_on = true;
            r.w.next_call = r.w.now;
            on_at = Some(r.w.now);
        }
        if let Some(at) = on_at
            && gone.is_some()
            && r.host.connected()
        {
            back = Some(r.w.now - at);
        }
    }
    let (Some(gone), Some(freed), Some(back)) = (gone, freed, back) else {
        panic!(
            "seed {seed} simple {simple} forward {forward} on after {on_after} ms: gone {gone:?} freed {freed:?} back {back:?} (state {}, phase {})\n{}",
            r.host.state(),
            r.host.phase(),
            r.wire.tail()
        );
    };
    r.crc_same();
    OffOn { gone, freed, back }
}

/// The owner's report: the pad switched off showed as connected for
/// about 20 s, and a switch-on before that never connected. Now: a
/// streaming pad is shown gone about 1 s after the switch-off, the
/// controller lets the dead link go at the 2 s timeout the host wrote,
/// and a switch-on at any time is connected within a few seconds of
/// that, never after 20 s. A host that writes no supervision timeout
/// fails its first check (the controller would keep 20 s).
#[test]
fn a_pad_switched_off_and_on_again_comes_back_quickly() {
    for (simple, forward) in [(false, false), (false, true), (true, false), (true, true)] {
        for on_after in [300, 800, 1_500, 2_500, 5_000] {
            let mut worst = (0, 0, 0);
            for seed in 0..4 {
                let o = owner_off_on(40_000 + seed, simple, forward, on_after);
                let gone_max = if simple { 2_100 } else { 1_100 };
                assert!(
                    o.gone <= gone_max,
                    "simple {simple}: shown gone after {} ms",
                    o.gone
                );
                assert!(o.freed <= 2_100, "old link freed after {} ms", o.freed);
                // Connected again at most 4 s after both the switch-on
                // and the controller's release of the old link.
                let limit = o.freed.saturating_sub(on_after) + 4_000;
                assert!(
                    o.back <= limit,
                    "simple {simple} forward {forward} on after {on_after}: back after {} ms",
                    o.back
                );
                worst = (
                    worst.0.max(o.gone),
                    worst.1.max(o.freed),
                    worst.2.max(o.back),
                );
            }
            println!(
                "owner off/on: simple {simple} forward {forward} on after {on_after} ms: shown gone {} ms, old link freed {} ms, connected {} ms after the switch-on (worst of 4)",
                worst.0, worst.1, worst.2
            );
        }
    }
}
