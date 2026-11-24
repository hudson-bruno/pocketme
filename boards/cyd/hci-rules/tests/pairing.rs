//! The gate's pairing window (feature `pair`): what each phase lets out,
//! byte for byte; that it closes by itself (deadline, cap, every
//! fall-back event); that a gate never armed is today's gate; and that no
//! address or key ever prints. Every address and key here is made up.

#![cfg(feature = "pair")]

use hci_rules::{
    FOUND_MAX, Gate, INQUIRY_UNITS_MAX, PAIR_WINDOW_MAX_MS, PairPhase, PairRefusal, Refusal, op,
};

// ---- made-up devices ----------------------------------------------------

/// The pad being paired (written A1:B2:C3:D4:E5:F6), HCI order.
const PAD: [u8; 6] = [0xF6, 0xE5, 0xD4, 0xC3, 0xB2, 0xA1];
/// A pad saved before.
const SAVED_PAD: [u8; 6] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
/// Some other device.
const OTHER: [u8; 6] = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
/// A made-up link key.
const KEY: [u8; 16] = [
    0x7E, 0x3D, 0x91, 0x0B, 0xC4, 0x58, 0xA6, 0x2F, 0xE3, 0x17, 0x6C, 0xD0, 0x49, 0xB5, 0x82, 0x1A,
];

/// Class of device of a gamepad (peripheral, gamepad), a keyboard
/// (peripheral), a phone and a laptop.
const COD_GAMEPAD: u32 = 0x00_25_08;
const COD_KEYBOARD: u32 = 0x00_05_40;
const COD_PHONE: u32 = 0x5A_02_0C;
const COD_LAPTOP: u32 = 0x10_01_0C;

const H: u16 = 0x0080;
const H_SAVED: u16 = 0x0081;

const SEARCH_MS: u64 = 60_000;
const BOUND_MS: u64 = 30_000;

// ---- packets --------------------------------------------------------------

fn command(opcode: u16, params: &[u8]) -> Vec<u8> {
    let [lo, hi] = opcode.to_le_bytes();
    let mut p = vec![0x01, lo, hi, params.len() as u8];
    p.extend_from_slice(params);
    p
}

fn event(code: u8, params: &[u8]) -> Vec<u8> {
    let mut p = vec![0x04, code, params.len() as u8];
    p.extend_from_slice(params);
    p
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn cod3(cod: u32) -> [u8; 3] {
    let b = cod.to_le_bytes();
    [b[0], b[1], b[2]]
}

/// pad-proto's inquiry: GIAC, `units` x 1.28 s, no limit on answers.
fn inquiry(units: u8) -> Vec<u8> {
    command(op::INQUIRY, &[0x33, 0x8B, 0x9E, units, 0x00])
}

fn inquiry_cancel() -> Vec<u8> {
    command(op::INQUIRY_CANCEL, &[])
}

fn create_connection(addr: [u8; 6]) -> Vec<u8> {
    // Packet types 18 CC, page scan mode 1, reserved 0, clock offset
    // valid, role switch allowed.
    command(
        op::CREATE_CONNECTION,
        &cat(&[&addr, &[0x18, 0xCC, 0x01, 0x00, 0x34, 0x92, 0x01]]),
    )
}

fn create_connection_cancel(addr: [u8; 6]) -> Vec<u8> {
    command(op::CREATE_CONNECTION_CANCEL, &addr)
}

fn accept(addr: [u8; 6]) -> Vec<u8> {
    command(op::ACCEPT_CONNECTION_REQUEST, &cat(&[&addr, &[0x01]]))
}

fn name_request(addr: [u8; 6]) -> Vec<u8> {
    command(
        op::REMOTE_NAME_REQUEST,
        &cat(&[&addr, &[0x01, 0x00, 0x00, 0x00]]),
    )
}

fn io_reply(addr: [u8; 6]) -> Vec<u8> {
    command(
        op::IO_CAPABILITY_REQUEST_REPLY,
        &cat(&[&addr, &[0x03, 0x00, 0x04]]),
    )
}

fn confirm(addr: [u8; 6]) -> Vec<u8> {
    command(op::USER_CONFIRMATION_REQUEST_REPLY, &addr)
}

fn key_reply(addr: [u8; 6]) -> Vec<u8> {
    command(op::LINK_KEY_REQUEST_REPLY, &cat(&[&addr, &KEY]))
}

fn key_negative(addr: [u8; 6]) -> Vec<u8> {
    command(op::LINK_KEY_REQUEST_NEGATIVE_REPLY, &addr)
}

fn pin_reply(addr: [u8; 6]) -> Vec<u8> {
    command(
        op::PIN_CODE_REQUEST_REPLY,
        &cat(&[&addr, &[4], b"0000", &[0; 12]]),
    )
}

fn lsto(handle: u16, slots: u16) -> Vec<u8> {
    command(
        op::WRITE_LINK_SUPERVISION_TIMEOUT,
        &cat(&[&handle.to_le_bytes(), &slots.to_le_bytes()]),
    )
}

/// Inquiry Result (0x02), one or more answers, each whole.
fn inquiry_result(answers: &[([u8; 6], u32)]) -> Vec<u8> {
    let mut p = vec![answers.len() as u8];
    for (a, cod) in answers {
        p.extend_from_slice(a);
        p.extend_from_slice(&[0x01, 0x00, 0x00]); // page scan mode, 2 reserved
        p.extend_from_slice(&cod3(*cod));
        p.extend_from_slice(&[0x34, 0x12]); // clock offset
    }
    event(0x02, &p)
}

/// Inquiry Result with RSSI (0x22), one or more answers, each whole.
fn inquiry_result_rssi(answers: &[([u8; 6], u32)]) -> Vec<u8> {
    let mut p = vec![answers.len() as u8];
    for (a, cod) in answers {
        p.extend_from_slice(a);
        p.extend_from_slice(&[0x01, 0x00]); // page scan mode, 1 reserved
        p.extend_from_slice(&cod3(*cod));
        p.extend_from_slice(&[0x34, 0x12, 0xC8]); // clock offset, RSSI
    }
    event(0x22, &p)
}

/// Extended Inquiry Result (0x2F): one answer and 240 bytes of EIR.
fn extended_result(addr: [u8; 6], cod: u32) -> Vec<u8> {
    let mut p = vec![0x01];
    p.extend_from_slice(&addr);
    p.extend_from_slice(&[0x01, 0x00]);
    p.extend_from_slice(&cod3(cod));
    p.extend_from_slice(&[0x34, 0x12, 0xC8]);
    let mut eir = [0u8; 240];
    eir[..16].copy_from_slice(b"\x0F\x09Pro Controller");
    p.extend_from_slice(&eir);
    event(0x2F, &p)
}

fn conn_complete(status: u8, handle: u16, addr: [u8; 6]) -> Vec<u8> {
    event(
        0x03,
        &cat(&[&[status], &handle.to_le_bytes(), &addr, &[0x01, 0x00]]),
    )
}

fn disconnected(status: u8, handle: u16) -> Vec<u8> {
    event(0x05, &cat(&[&[status], &handle.to_le_bytes(), &[0x13]]))
}

fn key_notification(addr: [u8; 6], key: [u8; 16]) -> Vec<u8> {
    event(0x18, &cat(&[&addr, &key, &[0x04]]))
}

fn pin_request(addr: [u8; 6]) -> Vec<u8> {
    event(0x16, &addr)
}

fn pairing_complete(status: u8, addr: [u8; 6]) -> Vec<u8> {
    event(0x36, &cat(&[&[status], &addr]))
}

fn hardware_error() -> Vec<u8> {
    event(0x10, &[0x01])
}

// ---- gates in each phase -------------------------------------------------

fn saved_gate() -> Gate {
    let mut g = Gate::new();
    assert!(g.allow(SAVED_PAD));
    g.observe(&conn_complete(0, H_SAVED, SAVED_PAD));
    g
}

/// Armed at 0, the pad found at 1 s.
fn searching() -> Gate {
    let mut g = saved_gate();
    g.arm(0, SEARCH_MS).unwrap();
    g.observe_at(&extended_result(PAD, COD_GAMEPAD), 1_000);
    assert_eq!(g.phase(), PairPhase::Searching);
    g
}

/// Bound to the pad at 2 s.
fn bound() -> Gate {
    let mut g = searching();
    g.bind(PAD, 2_000, BOUND_MS).unwrap();
    assert_eq!(g.phase(), PairPhase::Bound);
    g
}

/// Bound, and the pad's link is up (handle H) at 3 s.
fn bound_linked() -> Gate {
    let mut g = bound();
    g.observe_at(&conn_complete(0, H, PAD), 3_000);
    assert_eq!(g.phase(), PairPhase::Bound);
    g
}

/// Its key came at 4 s.
fn paired() -> Gate {
    let mut g = bound_linked();
    g.observe_at(&key_notification(PAD, KEY), 4_000);
    assert_eq!(g.phase(), PairPhase::Paired);
    g
}

/// Each phase, with a time inside it.
fn every_phase() -> Vec<(&'static str, Gate, u64)> {
    vec![
        ("never armed", saved_gate(), 5_000),
        ("searching", searching(), 5_000),
        ("bound", bound(), 5_000),
        ("bound, linked", bound_linked(), 5_000),
        ("paired", paired(), 5_000),
    ]
}

const NOT_ALLOWED: fn(u16) -> Result<(), Refusal> = |o| Err(Refusal::NotAllowed(o));
const BAD: fn(u16) -> Result<(), Refusal> = |o| Err(Refusal::BadParams(o));
const NOT_BOUND: fn(u16) -> Result<(), Refusal> = |o| Err(Refusal::NotBound(o));

// ---- a generated set of packets ------------------------------------------

/// A small fixed-seed generator (xorshift), so every run sees the same
/// packets.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn byte(&mut self) -> u8 {
        self.next() as u8
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Every opcode the gate knows of, the window's, and some it never
/// allows.
const OPCODES: [u16; 50] = [
    0x0401, 0x0402, 0x0403, 0x0405, 0x0406, 0x0408, 0x0409, 0x040A, 0x040B, 0x040C, 0x040D, 0x040E,
    0x0411, 0x0413, 0x0419, 0x041B, 0x041C, 0x041D, 0x042B, 0x042C, 0x042D, 0x042F, 0x0434, 0x080F,
    0x0C01, 0x0C03, 0x0C05, 0x0C11, 0x0C12, 0x0C13, 0x0C18, 0x0C1A, 0x0C1E, 0x0C20, 0x0C24, 0x0C37,
    0x0C3A, 0x0C45, 0x0C52, 0x0C56, 0x1001, 0x1003, 0x1005, 0x1009, 0x1408, 0x200A, 0x200C, 0x200D,
    0x0000, 0xFFFF,
];

/// Commands of every known opcode with lengths around the right ones,
/// parameters built from the made-up addresses, handles and the window's
/// pinned bytes, and random ones; ACL data; events; broken packets.
fn generated() -> Vec<Vec<u8>> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut out = Vec::new();
    let addrs = [PAD, SAVED_PAD, OTHER];
    let tails: [&[u8]; 8] = [
        &[],
        &[0x33, 0x8B, 0x9E, 3, 0],
        &[0x18, 0xCC, 0x01, 0x00, 0x34, 0x92, 0x01],
        &[0x03, 0x00, 0x04],
        &[0x01],
        &[0x00, 0x00, 0x00, 0x00],
        &KEY,
        &[0x80, 0x0C],
    ];
    for &opcode in &OPCODES {
        for len in [
            0usize, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 13, 14, 22, 23, 248,
        ] {
            for variant in 0..6 {
                let mut p: Vec<u8> = match variant {
                    0 => vec![0; len],
                    1 => (0..len).map(|_| rng.byte()).collect(),
                    2..=4 => {
                        let mut v = addrs[variant - 2].to_vec();
                        v.extend_from_slice(tails[rng.below(tails.len())]);
                        v
                    }
                    _ => {
                        let h = [H, H_SAVED, 0x0099][rng.below(3)];
                        let mut v = h.to_le_bytes().to_vec();
                        v.extend_from_slice(&[0x80, 0x0C, 0x01]);
                        v
                    }
                };
                p.resize(len, 0);
                out.push(command(opcode, &p));
            }
        }
    }
    for _ in 0..2_000 {
        let n = rng.below(40);
        let mut p: Vec<u8> = (0..n).map(|_| rng.byte()).collect();
        if let Some(k) = p.first_mut() {
            *k = [0x01, 0x02, 0x03, 0x04, 0x00][rng.below(5)];
        }
        // Half of them with a matching length byte.
        if rng.below(2) == 0 && p.len() >= 4 && p[0] == 0x01 {
            p[3] = (p.len() - 4) as u8;
        }
        out.push(p);
    }
    out.push(vec![0x02, 0x80, 0x20, 0x01, 0x00, 0xAA]);
    out.push(inquiry_result(&[(PAD, COD_GAMEPAD)]));
    out.push(conn_complete(0, H, PAD));
    out.push(vec![]);
    out
}

/// A whole command (its length byte right) with the opcode of one of the
/// two cancels: the only packets on which a never-armed `check_at` and
/// `check` differ.
fn is_cancel(p: &[u8]) -> bool {
    p.len() >= 4
        && p[0] == 0x01
        && usize::from(p[3]) == p.len() - 4
        && matches!(
            u16::from_le_bytes([p[1], p[2]]),
            op::INQUIRY_CANCEL | op::CREATE_CONNECTION_CANCEL
        )
}

/// What `check_at` says to a cancel: out when its length is right,
/// otherwise wrong parameters.
fn cancel_verdict(p: &[u8]) -> Result<(), Refusal> {
    let opcode = u16::from_le_bytes([p[1], p[2]]);
    let want = if opcode == op::INQUIRY_CANCEL { 0 } else { 6 };
    if p.len() - 4 == want {
        Ok(())
    } else {
        BAD(opcode)
    }
}

const TIMES: [u64; 6] = [0, 1, 59_999, 60_000, PAIR_WINDOW_MAX_MS, u64::MAX];

/// For each generated packet and every time: `check_at` == `check`, but
/// for the two cancels (which `check` refuses as not on its list).
fn assert_todays(g: &Gate, what: &str) {
    let mut cancels = 0;
    for p in generated() {
        let today = g.check(&p);
        for t in TIMES {
            let at = g.check_at(&p, t);
            if is_cancel(&p) {
                assert_eq!(at, cancel_verdict(&p), "{what}: {p:02x?}");
                assert!(
                    matches!(today, Err(Refusal::NotAllowed(0x0402 | 0x0408))),
                    "{what}: {p:02x?}"
                );
                cancels += usize::from(at.is_ok());
            } else {
                assert_eq!(at, today, "{what} at {t}: {p:02x?}");
            }
        }
    }
    assert!(cancels > 0, "the set holds the cancels");
}

// ---- 1. a gate never armed is today's gate ---------------------------------

#[test]
fn a_never_armed_gate_is_todays_gate() {
    assert_todays(&Gate::new(), "empty");
    assert_todays(&saved_gate(), "one saved, linked");
    let mut full = Gate::new();
    for a in [PAD, SAVED_PAD, OTHER, [9; 6]] {
        assert!(full.allow(a));
    }
    assert_todays(&full, "four saved");
}

#[test]
fn a_never_armed_gate_observes_as_today() {
    // The same packets through `observe` and `observe_at` leave equal
    // gates (links noted and dropped the same way).
    let mut rng = Rng(0x0123_4567_89AB_CDEF);
    let events = [
        conn_complete(0, H, PAD),
        conn_complete(0, H_SAVED, SAVED_PAD),
        conn_complete(0x04, H_SAVED, SAVED_PAD),
        disconnected(0, H_SAVED),
        disconnected(0x0C, H),
        key_notification(SAVED_PAD, KEY),
        pin_request(PAD),
        pairing_complete(0x05, PAD),
        inquiry_result(&[(PAD, COD_GAMEPAD)]),
        hardware_error(),
        lsto(H, 0x0C80),
    ];
    let mut a = saved_gate();
    let mut b = saved_gate();
    for i in 0..500u64 {
        let p = &events[rng.below(events.len())];
        a.observe(p);
        b.observe_at(p, i * 997);
        assert_eq!(a, b, "after {p:02x?}");
        assert_eq!(b.phase(), PairPhase::Off);
    }
}

// ---- 2. the window closes by itself ---------------------------------------

#[test]
fn the_window_closes_by_itself() {
    let mut g = Gate::new();
    g.arm(0, SEARCH_MS).unwrap();
    assert_eq!(g.check_at(&inquiry(3), 59_999), Ok(()));
    assert_eq!(g.check_at(&inquiry(3), 60_000), NOT_ALLOWED(op::INQUIRY));
    assert_eq!(g.check_at(&inquiry(3), u64::MAX), NOT_ALLOWED(op::INQUIRY));
    // Nobody called anything: the stored phase still says searching until
    // the time comes in.
    assert_eq!(g.phase(), PairPhase::Searching);
    g.expire(59_999);
    assert_eq!(g.phase(), PairPhase::Searching);
    g.expire(60_000);
    assert_eq!(g.phase(), PairPhase::Off);
}

#[test]
fn each_phase_ends_at_its_deadline_with_nothing_but_time() {
    // Search: until 60 s.
    let g = searching();
    assert_eq!(g.check_at(&inquiry(3), SEARCH_MS - 1), Ok(()));
    assert_eq!(g.check_at(&inquiry(3), SEARCH_MS), NOT_ALLOWED(op::INQUIRY));
    // Bound at 2 s for 30 s: until 32 s.
    let g = bound_linked();
    let end = 2_000 + BOUND_MS;
    for p in [
        create_connection(PAD),
        io_reply(PAD),
        confirm(PAD),
        name_request(PAD),
    ] {
        assert_eq!(g.check_at(&p, end - 1), Ok(()), "{p:02x?}");
        assert!(g.check_at(&p, end).is_err(), "{p:02x?}");
    }
    // Paired keeps Bound's deadline.
    let g = paired();
    assert_eq!(g.check_at(&key_reply(PAD), end - 1), Ok(()));
    assert_eq!(
        g.check_at(&key_reply(PAD), end),
        Err(Refusal::NotSaved(op::LINK_KEY_REQUEST_REPLY))
    );
    // A window past its deadline is today's gate (with the cancels), and
    // observe_at closes it at the first packet.
    for (what, mut g, _) in every_phase() {
        let late = 200_000;
        let fresh = g;
        g.observe_at(&disconnected(0x0C, 0x0999), late);
        assert_eq!(g.phase(), PairPhase::Off, "{what}");
        for p in generated().iter().step_by(7) {
            if !is_cancel(p) {
                assert_eq!(fresh.check_at(p, late), fresh.check(p), "{what}: {p:02x?}");
            }
        }
    }
}

// ---- 3. the cap -------------------------------------------------------------

#[test]
fn no_window_is_longer_than_the_cap() {
    let mut g = Gate::new();
    g.arm(1_000, u64::MAX).unwrap();
    let cap = 1_000 + PAIR_WINDOW_MAX_MS;
    assert_eq!(g.check_at(&inquiry(3), cap - 1), Ok(()));
    assert_eq!(g.check_at(&inquiry(3), cap), NOT_ALLOWED(op::INQUIRY));
    // Arming again while open is refused (no stretching); after the end
    // it is allowed.
    assert_eq!(g.arm(cap - 1, SEARCH_MS), Err(PairRefusal::Open));
    assert_eq!(g.arm(cap, SEARCH_MS), Ok(()));
    // No time at all is refused.
    let mut g = Gate::new();
    assert_eq!(g.arm(0, 0), Err(PairRefusal::NoTime));
    assert_eq!(g.phase(), PairPhase::Off);
    // A clock near its end does not wrap round.
    let mut g = Gate::new();
    g.arm(u64::MAX - 10, SEARCH_MS).unwrap();
    assert_eq!(g.check_at(&inquiry(3), u64::MAX - 1), Ok(()));
}

#[test]
fn bind_cannot_stretch_past_the_cap() {
    let mut g = Gate::new();
    g.arm(0, 100_000).unwrap();
    g.observe_at(&inquiry_result(&[(PAD, COD_GAMEPAD)]), 89_000);
    // Bound at 90 s for 60 s: still ends at 120 s from the arming.
    g.bind(PAD, 90_000, 60_000).unwrap();
    assert_eq!(g.check_at(&create_connection(PAD), 119_999), Ok(()));
    assert_eq!(
        g.check_at(&create_connection(PAD), PAIR_WINDOW_MAX_MS),
        NOT_ALLOWED(op::CREATE_CONNECTION)
    );
    // Paired keeps the capped deadline too.
    g.observe_at(&conn_complete(0, H, PAD), 100_000);
    g.observe_at(&key_notification(PAD, KEY), 101_000);
    assert_eq!(g.phase(), PairPhase::Paired);
    assert_eq!(g.check_at(&key_reply(PAD), 119_999), Ok(()));
    assert!(g.check_at(&key_reply(PAD), PAIR_WINDOW_MAX_MS).is_err());
    // A bind shorter than what is left ends at its own time.
    let mut g = searching();
    g.bind(PAD, 2_000, 5_000).unwrap();
    assert_eq!(g.check_at(&create_connection(PAD), 6_999), Ok(()));
    assert!(g.check_at(&create_connection(PAD), 7_000).is_err());
}

// ---- 4. bind takes only a pad found in this window ------------------------

#[test]
fn bind_takes_only_an_address_found_in_this_window() {
    // Not searching.
    let mut g = Gate::new();
    assert_eq!(g.bind(PAD, 0, BOUND_MS), Err(PairRefusal::NotSearching));
    // Never seen.
    g.arm(0, SEARCH_MS).unwrap();
    assert_eq!(g.bind(PAD, 1, BOUND_MS), Err(PairRefusal::NotFound));
    // Seen, but not as a peripheral.
    g.observe_at(&inquiry_result(&[(PAD, COD_PHONE)]), 2);
    assert_eq!(g.bind(PAD, 3, BOUND_MS), Err(PairRefusal::NotFound));
    // Seen in an earlier window.
    g.observe_at(&inquiry_result(&[(PAD, COD_GAMEPAD)]), 4);
    g.disarm();
    g.arm(10, SEARCH_MS).unwrap();
    assert_eq!(g.bind(PAD, 11, BOUND_MS), Err(PairRefusal::NotFound));
    // Seen in a window that ran out before the bind.
    g.observe_at(&inquiry_result(&[(PAD, COD_GAMEPAD)]), 12);
    assert_eq!(
        g.bind(PAD, 10 + SEARCH_MS, BOUND_MS),
        Err(PairRefusal::NotSearching)
    );
    // An address seen outside a search is not noted.
    let mut g = Gate::new();
    g.observe_at(&inquiry_result(&[(PAD, COD_GAMEPAD)]), 0);
    g.arm(1, SEARCH_MS).unwrap();
    assert_eq!(g.bind(PAD, 2, BOUND_MS), Err(PairRefusal::NotFound));
    // No time at all.
    g.observe_at(&inquiry_result(&[(PAD, COD_GAMEPAD)]), 3);
    assert_eq!(g.bind(PAD, 4, 0), Err(PairRefusal::NoTime));
    // Each of the three answer forms counts; a keyboard is a peripheral.
    for (what, answer) in [
        ("0x02", inquiry_result(&[(PAD, COD_KEYBOARD)])),
        ("0x22", inquiry_result_rssi(&[(PAD, COD_GAMEPAD)])),
        ("0x2F", extended_result(PAD, COD_GAMEPAD)),
    ] {
        let mut g = Gate::new();
        g.arm(0, SEARCH_MS).unwrap();
        g.observe_at(&answer, 1);
        assert_eq!(g.bind(PAD, 2, BOUND_MS), Ok(()), "{what}");
        // Bound once: a second bind is refused (no search).
        assert_eq!(g.bind(PAD, 3, BOUND_MS), Err(PairRefusal::NotSearching));
    }
    // Answers cut short or with a wrong count are not read.
    for broken in [
        event(
            0x02,
            &cat(&[&[2], &PAD, &[1, 0, 0], &cod3(COD_GAMEPAD), &[0, 0]]),
        ),
        event(0x22, &cat(&[&[1], &PAD, &[1, 0], &cod3(COD_GAMEPAD)])),
        event(0x2F, &cat(&[&[1], &PAD, &[1, 0], &cod3(COD_GAMEPAD)])),
        // A header that does not match the length.
        {
            let mut p = inquiry_result(&[(PAD, COD_GAMEPAD)]);
            p[2] += 1;
            p
        },
    ] {
        let mut g = Gate::new();
        g.arm(0, SEARCH_MS).unwrap();
        g.observe_at(&broken, 1);
        assert_eq!(g.bind(PAD, 2, BOUND_MS), Err(PairRefusal::NotFound));
    }
}

#[test]
fn several_answers_in_one_event_are_each_read() {
    let a = [0xA0, 1, 2, 3, 4, 5];
    let b = [0xB0, 1, 2, 3, 4, 5];
    for ev in [
        inquiry_result(&[(a, COD_PHONE), (PAD, COD_GAMEPAD), (b, COD_KEYBOARD)]),
        inquiry_result_rssi(&[(a, COD_PHONE), (PAD, COD_GAMEPAD), (b, COD_KEYBOARD)]),
    ] {
        for (addr, want) in [(PAD, Ok(())), (b, Ok(())), (a, Err(PairRefusal::NotFound))] {
            let mut g = Gate::new();
            g.arm(0, SEARCH_MS).unwrap();
            g.observe_at(&ev, 1);
            assert_eq!(g.bind(addr, 2, BOUND_MS), want);
        }
    }
}

#[test]
fn a_flood_of_thirty_non_pads_then_the_pad() {
    let mut g = Gate::new();
    g.arm(0, SEARCH_MS).unwrap();
    for i in 0..30u8 {
        let a = [i, 0x10, 0x20, 0x30, 0x40, 0x50];
        let cod = [COD_PHONE, COD_LAPTOP, 0x24_04_04][usize::from(i % 3)];
        let ev = match i % 3 {
            0 => inquiry_result(&[(a, cod)]),
            1 => inquiry_result_rssi(&[(a, cod)]),
            _ => extended_result(a, cod),
        };
        g.observe_at(&ev, 100 + u64::from(i));
        assert_eq!(g.bind(a, 200, BOUND_MS), Err(PairRefusal::NotFound));
    }
    g.observe_at(&extended_result(PAD, COD_GAMEPAD), 1_000);
    assert_eq!(g.bind(PAD, 1_001, BOUND_MS), Ok(()));
}

#[test]
fn the_found_list_keeps_the_newest_eight_once_each() {
    let periph = |i: u8| [i, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE];
    let fresh = |seen: &[[u8; 6]]| {
        let mut g = Gate::new();
        g.arm(0, SEARCH_MS).unwrap();
        for (t, a) in seen.iter().enumerate() {
            g.observe_at(&inquiry_result(&[(*a, COD_KEYBOARD)]), t as u64 + 1);
        }
        g
    };
    // Nine peripherals: the first is pushed out.
    let nine: Vec<_> = (0..=FOUND_MAX as u8).map(periph).collect();
    let mut g = fresh(&nine);
    assert_eq!(g.bind(periph(0), 100, BOUND_MS), Err(PairRefusal::NotFound));
    for i in 1..=FOUND_MAX as u8 {
        let mut g2 = fresh(&nine);
        assert_eq!(g2.bind(periph(i), 100, BOUND_MS), Ok(()), "{i}");
    }
    assert_eq!(g.bind(periph(1), 100, BOUND_MS), Ok(()));
    // The same address many times takes one place, and seeing it again
    // makes it the newest: the pad seen first, then seven others, the
    // pad again, then one more: the pad stays.
    let mut seen = vec![PAD];
    seen.extend((1..8).map(periph));
    seen.push(PAD);
    seen.push(PAD);
    seen.push(periph(9));
    let mut g = fresh(&seen);
    assert_eq!(g.bind(PAD, 100, BOUND_MS), Ok(()));
    // Without seeing it again it would have gone.
    let mut seen: Vec<_> = vec![PAD];
    seen.extend((1..=8).map(periph));
    let mut g = fresh(&seen);
    assert_eq!(g.bind(PAD, 100, BOUND_MS), Err(PairRefusal::NotFound));
}

// ---- 5. bound commands go to the bound pad only ---------------------------

#[test]
fn bound_commands_go_to_the_bound_address_only() {
    let g = bound();
    let t = 5_000;
    for addr in [OTHER, SAVED_PAD] {
        assert_eq!(
            g.check_at(&create_connection(addr), t),
            NOT_BOUND(op::CREATE_CONNECTION)
        );
        assert_eq!(
            g.check_at(&io_reply(addr), t),
            NOT_BOUND(op::IO_CAPABILITY_REQUEST_REPLY)
        );
        assert_eq!(
            g.check_at(&confirm(addr), t),
            NOT_BOUND(op::USER_CONFIRMATION_REQUEST_REPLY)
        );
    }
    assert_eq!(
        g.check_at(&name_request(OTHER), t),
        NOT_BOUND(op::REMOTE_NAME_REQUEST)
    );
    // A saved pad keeps its name request (today's rule).
    assert_eq!(g.check_at(&name_request(SAVED_PAD), t), Ok(()));
    for p in [
        create_connection(PAD),
        io_reply(PAD),
        confirm(PAD),
        name_request(PAD),
        key_negative(PAD),
        inquiry_cancel(),
        create_connection_cancel(PAD),
    ] {
        assert_eq!(g.check_at(&p, t), Ok(()), "{p:02x?}");
    }
    // Searching ended with the bind.
    assert_eq!(g.check_at(&inquiry(3), t), NOT_ALLOWED(op::INQUIRY));
    // Before the bind none of it passes.
    let s = searching();
    for (p, o) in [
        (create_connection(PAD), op::CREATE_CONNECTION),
        (io_reply(PAD), op::IO_CAPABILITY_REQUEST_REPLY),
        (confirm(PAD), op::USER_CONFIRMATION_REQUEST_REPLY),
    ] {
        assert_eq!(s.check_at(&p, t), NOT_ALLOWED(o));
    }
    assert_eq!(
        s.check_at(&name_request(PAD), t),
        Err(Refusal::NotSaved(op::REMOTE_NAME_REQUEST)),
        "a name request in a search would page the device"
    );
}

#[test]
fn no_accept_while_a_window_is_open() {
    for (what, g, t) in every_phase() {
        let open = matches!(what, "searching" | "bound" | "bound, linked");
        for addr in [PAD, SAVED_PAD, OTHER] {
            let got = g.check_at(&accept(addr), t);
            if open {
                assert_eq!(got, NOT_ALLOWED(op::ACCEPT_CONNECTION_REQUEST), "{what}");
            } else {
                assert_eq!(got, g.check(&accept(addr)), "{what}");
            }
        }
    }
    // After the window, the saved pad is accepted again.
    let g = searching();
    assert_eq!(g.check_at(&accept(SAVED_PAD), SEARCH_MS), Ok(()));
}

#[test]
fn a_stale_key_is_never_offered_to_the_pad_being_paired() {
    // The pad is saved already and is paired again.
    let mut g = Gate::new();
    assert!(g.allow(PAD));
    g.arm(0, SEARCH_MS).unwrap();
    g.observe_at(&inquiry_result(&[(PAD, COD_GAMEPAD)]), 1);
    g.bind(PAD, 2, BOUND_MS).unwrap();
    assert_eq!(
        g.check_at(&key_reply(PAD), 3),
        NOT_ALLOWED(op::LINK_KEY_REQUEST_REPLY)
    );
    assert_eq!(g.check_at(&key_negative(PAD), 3), Ok(()));
    // Its new key came: its key may be given again.
    g.observe_at(&conn_complete(0, H, PAD), 4);
    g.observe_at(&key_notification(PAD, KEY), 5);
    assert_eq!(g.check_at(&key_reply(PAD), 6), Ok(()));
}

// ---- every rule's bytes ---------------------------------------------------

#[test]
fn inquiry_parameters() {
    let g = searching();
    let t = 5_000;
    for units in 1..=INQUIRY_UNITS_MAX {
        assert_eq!(g.check_at(&inquiry(units), t), Ok(()), "{units}");
    }
    for p in [
        vec![0x33, 0x8B, 0x9E, 0, 0],                     // no time
        vec![0x33, 0x8B, 0x9E, INQUIRY_UNITS_MAX + 1, 0], // too long
        vec![0x33, 0x8B, 0x9E, 3, 1],                     // a count
        vec![0x33, 0x8B, 0x9E, 3, 0xFF],
        vec![0x00, 0x8B, 0x9E, 3, 0], // LIAC
        vec![0x33, 0x8B, 0x9F, 3, 0],
        vec![0x33, 0x8B, 0x9E, 3],
        vec![0x33, 0x8B, 0x9E, 3, 0, 0],
        vec![],
    ] {
        assert_eq!(
            g.check_at(&command(op::INQUIRY, &p), t),
            BAD(op::INQUIRY),
            "{p:02x?}"
        );
    }
}

#[test]
fn create_connection_parameters() {
    let g = bound();
    let t = 5_000;
    let good = create_connection(PAD);
    assert_eq!(g.check_at(&good, t), Ok(()));
    // Any page scan mode and clock offset; role switch 0 or 1.
    for (at, v) in [(8, 0x00), (8, 0x02), (10, 0x00), (11, 0x00), (12, 0x00)] {
        let mut p = good.clone();
        p[4 + at] = v;
        assert_eq!(g.check_at(&p, t), Ok(()), "byte {at} = {v:#04x}");
    }
    for (at, v) in [(6, 0x08), (7, 0xCD), (9, 0x01), (12, 0x02), (12, 0xFF)] {
        let mut p = good.clone();
        p[4 + at] = v;
        assert_eq!(
            g.check_at(&p, t),
            BAD(op::CREATE_CONNECTION),
            "byte {at} = {v:#04x}"
        );
    }
    let p = &good[4..];
    assert_eq!(
        g.check_at(&command(op::CREATE_CONNECTION, &p[..12]), t),
        BAD(op::CREATE_CONNECTION)
    );
    assert_eq!(
        g.check_at(&command(op::CREATE_CONNECTION, &cat(&[p, &[0]])), t),
        BAD(op::CREATE_CONNECTION)
    );
}

#[test]
fn io_capability_and_confirmation_parameters() {
    let g = bound();
    let t = 5_000;
    for (io, oob, auth) in [
        (0x01, 0x00, 0x04), // display yes/no
        (0x00, 0x00, 0x04),
        (0x03, 0x01, 0x04), // OOB data
        (0x03, 0x00, 0x00), // no bonding
        (0x03, 0x00, 0x05), // MITM
        (0x03, 0x00, 0x02),
    ] {
        let p = command(
            op::IO_CAPABILITY_REQUEST_REPLY,
            &cat(&[&PAD, &[io, oob, auth]]),
        );
        assert_eq!(
            g.check_at(&p, t),
            BAD(op::IO_CAPABILITY_REQUEST_REPLY),
            "{io} {oob} {auth}"
        );
    }
    for p in [
        command(op::IO_CAPABILITY_REQUEST_REPLY, &cat(&[&PAD, &[3, 0]])),
        command(
            op::IO_CAPABILITY_REQUEST_REPLY,
            &cat(&[&PAD, &[3, 0, 4, 0]]),
        ),
    ] {
        assert_eq!(g.check_at(&p, t), BAD(op::IO_CAPABILITY_REQUEST_REPLY));
    }
    for p in [
        command(op::USER_CONFIRMATION_REQUEST_REPLY, &PAD[..5]),
        command(op::USER_CONFIRMATION_REQUEST_REPLY, &cat(&[&PAD, &[0]])),
    ] {
        assert_eq!(g.check_at(&p, t), BAD(op::USER_CONFIRMATION_REQUEST_REPLY));
    }
    // Name request and key reply lengths.
    for p in [
        command(op::REMOTE_NAME_REQUEST, &cat(&[&PAD, &[1, 0, 0]])),
        command(op::REMOTE_NAME_REQUEST, &cat(&[&PAD, &[1, 0, 0, 0, 0]])),
    ] {
        assert_eq!(g.check_at(&p, t), BAD(op::REMOTE_NAME_REQUEST));
    }
    let pg = paired();
    for p in [
        command(op::LINK_KEY_REQUEST_REPLY, &cat(&[&PAD, &KEY[..15]])),
        command(op::LINK_KEY_REQUEST_REPLY, &cat(&[&PAD, &KEY, &[0]])),
    ] {
        assert_eq!(pg.check_at(&p, t), BAD(op::LINK_KEY_REQUEST_REPLY));
    }
    assert_eq!(
        pg.check_at(&key_reply(OTHER), t),
        NOT_BOUND(op::LINK_KEY_REQUEST_REPLY)
    );
    assert_eq!(pg.check_at(&key_reply(SAVED_PAD), t), Ok(()));
}

#[test]
fn the_cancels_pass_in_every_state_with_their_lengths() {
    for (what, g, t) in every_phase() {
        for t in [t, 200_000] {
            assert_eq!(g.check_at(&inquiry_cancel(), t), Ok(()), "{what}");
            for a in [PAD, OTHER] {
                assert_eq!(
                    g.check_at(&create_connection_cancel(a), t),
                    Ok(()),
                    "{what}"
                );
            }
            assert_eq!(
                g.check_at(&command(op::INQUIRY_CANCEL, &[0]), t),
                BAD(op::INQUIRY_CANCEL)
            );
            for n in [5, 7] {
                assert_eq!(
                    g.check_at(&command(op::CREATE_CONNECTION_CANCEL, &vec![0; n]), t),
                    BAD(op::CREATE_CONNECTION_CANCEL)
                );
            }
        }
        // `check` stays today's: never.
        assert_eq!(g.check(&inquiry_cancel()), NOT_ALLOWED(op::INQUIRY_CANCEL));
        assert_eq!(
            g.check(&create_connection_cancel(PAD)),
            NOT_ALLOWED(op::CREATE_CONNECTION_CANCEL)
        );
    }
}

// ---- 7. never, in any phase -----------------------------------------------

#[test]
fn never_in_any_phase() {
    for (what, g, t) in every_phase() {
        for addr in [PAD, SAVED_PAD, OTHER] {
            assert_eq!(
                g.check_at(&pin_reply(addr), t),
                NOT_ALLOWED(op::PIN_CODE_REQUEST_REPLY),
                "{what}"
            );
        }
        for v in [0x01, 0x03, 0x04, 0x80, 0xFF] {
            assert_eq!(
                g.check_at(&command(op::WRITE_SCAN_ENABLE, &[v]), t),
                BAD(op::WRITE_SCAN_ENABLE),
                "{what}: {v:#04x}"
            );
        }
        for v in [0x00, 0x02] {
            assert_eq!(
                g.check_at(&command(op::WRITE_SCAN_ENABLE, &[v]), t),
                Ok(()),
                "{what}"
            );
        }
        let store_key = cat(&[&[1], &PAD, &KEY]);
        let delete_key = cat(&[&PAD, &[0]]);
        let never: [(u16, &[u8]); 12] = [
            (0x0403, &[0; 9]),                // Periodic Inquiry Mode
            (0x0C05, &[0; 1]),                // Set Event Filter
            (0x0C11, &store_key),             // Write Stored Link Key
            (0x0C12, &delete_key),            // Delete Stored Link Key
            (0x0C20, &[1]),                   // Write Authentication Enable
            (0x0C1E, &[0; 4]),                // Write Inquiry Scan Activity
            (0x0C3A, &[1, 0x33, 0x8B, 0x9E]), // Write Current IAC LAP
            (0x0C52, &[0; 241]),              // Write Extended Inquiry Response
            (0x200C, &[1, 0]),                // LE Set Scan Enable
            (0x200A, &[1]),                   // LE Set Advertising Enable
            (0x200D, &[0; 25]),               // LE Create Connection
            (0x0C7A, &[1]),                   // Write Secure Connections Host Support
        ];
        for (opcode, params) in never {
            assert_eq!(
                g.check_at(&command(opcode, params), t),
                NOT_ALLOWED(opcode),
                "{what}: {opcode:#06x}"
            );
        }
    }
}

// ---- 6. each fall-back -------------------------------------------------------

/// The gate is closed: no window command passes, and it judges as a
/// never-armed gate with the same saved pads and links.
fn assert_closed(g: &Gate, t: u64, what: &str) {
    assert_eq!(g.phase(), PairPhase::Off, "{what}");
    for p in [
        inquiry(3),
        create_connection(PAD),
        io_reply(PAD),
        confirm(PAD),
    ] {
        assert!(
            matches!(g.check_at(&p, t), Err(Refusal::NotAllowed(_))),
            "{what}: {p:02x?}"
        );
    }
    for p in generated().iter().step_by(5) {
        if !is_cancel(p) {
            assert_eq!(g.check_at(p, t), g.check(p), "{what}: {p:02x?}");
        }
    }
}

#[test]
fn a_hardware_error_closes_any_window() {
    for (what, mut g, t) in every_phase() {
        g.observe_at(&hardware_error(), t);
        assert_closed(&g, t, what);
    }
}

#[test]
fn a_pin_request_from_the_bound_pad_closes_the_window() {
    for mut g in [bound(), bound_linked(), paired()] {
        let before = g;
        g.observe_at(&pin_request(OTHER), 5_000);
        assert_eq!(g, before, "another device's PIN request");
        g.observe_at(&pin_request(PAD), 5_000);
        assert_closed(&g, 5_000, "pin request");
    }
    // In a search it changes nothing.
    let mut g = searching();
    g.observe_at(&pin_request(PAD), 5_000);
    assert_eq!(g.phase(), PairPhase::Searching);
}

#[test]
fn a_failed_simple_pairing_closes_the_window() {
    for mut g in [bound_linked(), paired()] {
        let phase = g.phase();
        g.observe_at(&pairing_complete(0x05, OTHER), 5_000);
        g.observe_at(&pairing_complete(0x00, PAD), 5_000);
        g.observe_at(&event(0x36, &[0x05, 0xF6, 0xE5]), 5_000); // cut short
        assert_eq!(g.phase(), phase);
        g.observe_at(&pairing_complete(0x05, PAD), 5_000);
        assert_closed(&g, 5_000, "failed pairing");
    }
}

#[test]
fn a_failed_call_closes_the_window() {
    let mut g = bound();
    g.observe_at(&conn_complete(0x04, H, OTHER), 3_000);
    assert_eq!(g.phase(), PairPhase::Bound);
    g.observe_at(&conn_complete(0x04, H, PAD), 3_000);
    assert_closed(&g, 3_000, "page timeout");
    assert_eq!(
        g.check_at(&lsto(H, 0x0C80), 3_000),
        Err(Refusal::NotSaved(op::WRITE_LINK_SUPERVISION_TIMEOUT))
    );
}

#[test]
fn the_bound_pads_link_ending_closes_the_window() {
    for mut g in [bound_linked(), paired()] {
        let phase = g.phase();
        g.observe_at(&disconnected(0, 0x0099), 5_000);
        g.observe_at(&disconnected(0x0C, H), 5_000); // the disconnect failed
        g.observe_at(&disconnected(0, H_SAVED), 5_000); // the saved pad's link
        assert_eq!(g.phase(), phase);
        g.observe_at(&disconnected(0, H), 5_000);
        assert_closed(&g, 5_000, "link lost");
        assert_eq!(
            g.check_at(&lsto(H, 0x0C80), 5_000),
            Err(Refusal::NotSaved(op::WRITE_LINK_SUPERVISION_TIMEOUT))
        );
    }
}

#[test]
fn the_key_turns_bound_into_paired_and_saving_it_closes_the_window() {
    let mut g = bound_linked();
    g.observe_at(&key_notification(OTHER, KEY), 4_000);
    assert_eq!(g.phase(), PairPhase::Bound, "another device's key");
    g.observe_at(&event(0x18, &cat(&[&PAD, &KEY[..8]])), 4_000);
    assert_eq!(g.phase(), PairPhase::Bound, "a notification cut short");
    g.observe_at(&key_notification(PAD, KEY), 4_000);
    assert_eq!(g.phase(), PairPhase::Paired);
    // Paired: the pairing "yes" answers are over.
    for p in [io_reply(PAD), confirm(PAD), create_connection(PAD)] {
        assert!(
            matches!(g.check_at(&p, 5_000), Err(Refusal::NotAllowed(_))),
            "{p:02x?}"
        );
    }
    assert_eq!(g.check_at(&key_reply(PAD), 5_000), Ok(()));
    assert_eq!(g.check_at(&name_request(PAD), 5_000), Ok(()));
    assert_eq!(
        g.check_at(&accept(PAD), 5_000),
        Err(Refusal::NotSaved(op::ACCEPT_CONNECTION_REQUEST))
    );
    // The key is saved: allow ends the window; the pad is a saved pad.
    assert!(g.allow(SAVED_PAD), "another address changes nothing");
    assert_eq!(g.phase(), PairPhase::Paired);
    assert!(g.allow(PAD));
    assert_eq!(g.phase(), PairPhase::Off);
    assert_eq!(g.check_at(&key_reply(PAD), 5_000), Ok(()));
    assert_eq!(g.check_at(&accept(PAD), 5_000), Ok(()));
    assert_eq!(g.saved(), 2);
}

#[test]
fn a_window_is_never_reopened_by_events() {
    // Once closed, nothing the controller says opens it again.
    let mut g = paired();
    g.disarm();
    assert_eq!(g.phase(), PairPhase::Off);
    for p in [
        inquiry_result(&[(PAD, COD_GAMEPAD)]),
        conn_complete(0, H, PAD),
        key_notification(PAD, KEY),
    ] {
        g.observe_at(&p, 5_000);
        assert_eq!(g.phase(), PairPhase::Off);
    }
    assert_closed(&g, 5_000, "disarmed");
}

// ---- 9. four saved ----------------------------------------------------------

#[test]
fn four_saved_may_search_and_pair_one_of_them_again_only() {
    let mut g = Gate::new();
    let four = [SAVED_PAD, [0x21; 6], [0x22; 6], [0x23; 6]];
    for a in four {
        assert!(g.allow(a));
    }
    assert_eq!(g.arm(0, SEARCH_MS), Ok(()), "not refused at four");
    g.observe_at(
        &inquiry_result(&[(PAD, COD_GAMEPAD), (SAVED_PAD, COD_GAMEPAD)]),
        1,
    );
    assert_eq!(g.bind(PAD, 2, BOUND_MS), Err(PairRefusal::Full));
    assert_eq!(g.phase(), PairPhase::Searching, "still searching");
    assert_eq!(g.bind(SAVED_PAD, 2, BOUND_MS), Ok(()));
    // After forgetting one, a new pad may be bound.
    let mut g = Gate::new();
    for a in four {
        assert!(g.allow(a));
    }
    g.arm(0, SEARCH_MS).unwrap();
    g.observe_at(&inquiry_result(&[(PAD, COD_GAMEPAD)]), 1);
    assert!(g.forget([0x23; 6]));
    assert_eq!(g.saved(), 3);
    assert_eq!(g.bind(PAD, 2, BOUND_MS), Ok(()));
}

// ---- 10. the new pad's link -----------------------------------------------

#[test]
fn the_new_pads_link_gets_its_supervision_timeout() {
    let nope = Err(Refusal::NotSaved(op::WRITE_LINK_SUPERVISION_TIMEOUT));
    let mut g = bound();
    assert_eq!(g.check_at(&lsto(H, 0x0C80), 2_500), nope, "no link yet");
    // A voice link to the pad is no ACL link.
    let mut voice = g;
    voice.observe_at(
        &event(0x03, &cat(&[&[0], &H.to_le_bytes(), &PAD, &[0x00, 0x00]])),
        2_600,
    );
    assert_eq!(voice.check_at(&lsto(H, 0x0C80), 2_600), nope);
    // Its link comes up (flag bits in the handle are dropped).
    g.observe_at(&conn_complete(0, H | 0x2000, PAD), 3_000);
    assert_eq!(g.linked(), 2);
    assert_eq!(g.check_at(&lsto(H, 0x0C80), 3_000), Ok(()));
    assert_eq!(g.check_at(&lsto(0x0099, 0x0C80), 3_000), nope);
    assert_eq!(
        g.check_at(&lsto(H, 0x063F), 3_000),
        BAD(op::WRITE_LINK_SUPERVISION_TIMEOUT),
        "under 1 s"
    );
    assert_eq!(
        g.check_at(&lsto(H, 0), 3_000),
        BAD(op::WRITE_LINK_SUPERVISION_TIMEOUT)
    );
    // After its key, after saving, and after the window: still allowed.
    g.observe_at(&key_notification(PAD, KEY), 4_000);
    assert_eq!(g.check_at(&lsto(H, 0x0C80), 4_000), Ok(()));
    let mut late = g;
    late.expire(100_000);
    assert_eq!(late.phase(), PairPhase::Off);
    assert_eq!(late.check_at(&lsto(H, 0x0C80), 100_000), Ok(()));
    assert_eq!(late.check(&lsto(H, 0x0C80)), Ok(()));
    assert!(g.allow(PAD));
    assert_eq!(g.check_at(&lsto(H, 0x0C80), 5_000), Ok(()));
    // Until its link ends.
    g.observe_at(&disconnected(0, H), 6_000);
    assert_eq!(g.check_at(&lsto(H, 0x0C80), 6_000), nope);
    assert_eq!(g.check_at(&lsto(H_SAVED, 0x0C80), 6_000), Ok(()));
}

// ---- forget -------------------------------------------------------------------

#[test]
fn forget_drops_the_address_and_its_link() {
    let nope = Err(Refusal::NotSaved(op::WRITE_LINK_SUPERVISION_TIMEOUT));
    let mut g = saved_gate();
    assert!(g.allow(PAD));
    g.observe(&conn_complete(0, H, PAD));
    assert_eq!(g.linked(), 2);
    assert!(!g.forget(OTHER), "never saved");
    assert_eq!(g.saved(), 2);
    assert!(g.forget(SAVED_PAD));
    assert!(!g.forget(SAVED_PAD), "gone already");
    assert_eq!(g.saved(), 1);
    assert_eq!(g.linked(), 1);
    assert_eq!(g.check(&lsto(H_SAVED, 0x0C80)), nope);
    assert_eq!(
        g.check(&lsto(H, 0x0C80)),
        Ok(()),
        "the other pad's link stays"
    );
    for p in [
        accept(SAVED_PAD),
        key_reply(SAVED_PAD),
        name_request(SAVED_PAD),
    ] {
        assert!(matches!(g.check(&p), Err(Refusal::NotSaved(_))));
    }
    assert_eq!(g.check(&accept(PAD)), Ok(()));
    // Its place is free again.
    assert!(g.allow(SAVED_PAD));
    assert_eq!(g.saved(), 2);
    // Forgetting the pad a window is bound to closes the window.
    for mut g in [bound_linked(), paired()] {
        assert!(!g.forget(PAD), "not saved yet");
        assert_eq!(g.phase(), PairPhase::Off);
        assert_eq!(g.check_at(&lsto(H, 0x0C80), 5_000), nope);
    }
    // Forgetting another one leaves the window as it is.
    let mut g = bound_linked();
    assert!(g.forget(SAVED_PAD));
    assert_eq!(g.phase(), PairPhase::Bound);
}

// ---- the window against pad-proto's own bytes ------------------------------

/// Every command pad-proto builds for a pairing, in the order it sends
/// them (map: review-radio section 1), through a gate opened and bound
/// with the same clock, with the controller's events fed to `observe_at`
/// between them: no refusal.
#[test]
fn pad_protos_pairing_commands_pass_the_window() {
    use pad_proto::hci::{BdAddr, command as pp_command, params};
    let pad = BdAddr(PAD);
    let pp = |opcode: u16, p: &[u8]| pp_command(opcode, p).as_bytes().to_vec();
    let mut g = saved_gate();
    let mut t = 10_000;
    let send = |g: &Gate, t: u64, p: Vec<u8>| {
        assert_eq!(g.check_at(&p, t), Ok(()), "at {t}: {p:02x?}");
    };
    g.arm(t, SEARCH_MS).unwrap();
    send(&g, t, pp(op::INQUIRY, &params::inquiry(3)));
    t += 900;
    g.observe_at(&extended_result(PAD, COD_GAMEPAD), t);
    // The user says yes: the host cancels the inquiry, the gate is bound.
    t += 2_000;
    g.bind(PAD, t, BOUND_MS).unwrap();
    send(&g, t, pp(op::INQUIRY_CANCEL, &[]));
    send(
        &g,
        t,
        pp(
            op::CREATE_CONNECTION,
            &params::create_connection(pad, 0x01, Some(0x1234)),
        ),
    );
    send(
        &g,
        t,
        pp(
            op::CREATE_CONNECTION,
            &params::create_connection(pad, 0x01, None),
        ),
    );
    t += 300;
    g.observe_at(&conn_complete(0, H, PAD), t);
    send(
        &g,
        t,
        pp(
            op::WRITE_LINK_SUPERVISION_TIMEOUT,
            &params::handle_u16(H, 0x0C80),
        ),
    );
    send(
        &g,
        t,
        pp(op::READ_REMOTE_SUPPORTED_FEATURES, &params::handle(H)),
    );
    send(
        &g,
        t,
        pp(op::READ_REMOTE_VERSION_INFORMATION, &params::handle(H)),
    );
    send(
        &g,
        t,
        pp(op::READ_REMOTE_EXTENDED_FEATURES, &params::handle_u8(H, 1)),
    );
    send(
        &g,
        t,
        pp(
            op::REMOTE_NAME_REQUEST,
            &params::remote_name(pad, 0x01, Some(0x1234)),
        ),
    );
    send(&g, t, pp(op::AUTHENTICATION_REQUESTED, &params::handle(H)));
    send(
        &g,
        t,
        pp(op::LINK_KEY_REQUEST_NEGATIVE_REPLY, &params::addr(pad)),
    );
    send(
        &g,
        t,
        pp(
            op::IO_CAPABILITY_REQUEST_REPLY,
            &params::io_capability(pad, 0x03, 0x04),
        ),
    );
    send(
        &g,
        t,
        pp(op::USER_CONFIRMATION_REQUEST_REPLY, &params::addr(pad)),
    );
    // Simple Pairing Complete, then the key (either order closes the
    // pairing "yes" answers the same way).
    t += 500;
    g.observe_at(&pairing_complete(0, PAD), t);
    g.observe_at(&key_notification(PAD, KEY), t);
    assert_eq!(g.phase(), PairPhase::Paired);
    send(
        &g,
        t,
        pp(op::SET_CONNECTION_ENCRYPTION, &params::handle_u8(H, 1)),
    );
    send(&g, t, pp(op::AUTHENTICATION_REQUESTED, &params::handle(H)));
    send(
        &g,
        t,
        pp(
            op::WRITE_LINK_SUPERVISION_TIMEOUT,
            &params::handle_u16(H, 0x1F40),
        ),
    );
    send(
        &g,
        t,
        pp(
            op::LINK_KEY_REQUEST_REPLY,
            &params::link_key_reply(pad, &KEY),
        ),
    );
    // Saved: the normal rules carry it.
    assert!(g.allow(PAD));
    send(
        &g,
        t,
        pp(
            op::LINK_KEY_REQUEST_REPLY,
            &params::link_key_reply(pad, &KEY),
        ),
    );
    send(&g, t, pp(op::DISCONNECT, &params::handle_u8(H, 0x13)));
    // A late clean-up after the window: the cancels still pass.
    let late = t + PAIR_WINDOW_MAX_MS;
    send(&g, late, pp(op::INQUIRY_CANCEL, &[]));
    send(
        &g,
        late,
        pp(op::CREATE_CONNECTION_CANCEL, &params::addr(pad)),
    );
}

// ---- printing ----------------------------------------------------------------

#[test]
fn the_window_prints_only_its_phase_and_never_an_address_or_key() {
    assert_eq!(format!("{:?}", saved_gate()), "Gate(1 saved, 1 linked)");
    assert_eq!(
        format!("{:?}", searching()),
        "Gate(1 saved, 1 linked, searching)"
    );
    assert_eq!(format!("{:?}", bound()), "Gate(1 saved, 1 linked, bound)");
    assert_eq!(
        format!("{:?}", bound_linked()),
        "Gate(1 saved, 2 linked, bound)"
    );
    assert_eq!(format!("{:?}", paired()), "Gate(1 saved, 2 linked, paired)");
    for (p, w) in [
        (PairPhase::Off, "off"),
        (PairPhase::Searching, "searching"),
        (PairPhase::Bound, "bound"),
        (PairPhase::Paired, "paired"),
    ] {
        assert_eq!(p.word(), w);
        assert_eq!(format!("{p}"), w);
        assert_eq!(format!("{p:?}"), w);
    }
    // The gate keeps no key: two gates told different keys are equal.
    let mut a = bound_linked();
    let mut b = bound_linked();
    a.observe_at(&key_notification(PAD, KEY), 4_000);
    b.observe_at(&key_notification(PAD, [0x55; 16]), 4_000);
    assert_eq!(a, b);
    // Nothing printed holds an address byte pattern or the key.
    let outputs = [
        format!("{a:?}"),
        format!("{:?}", a.phase()),
        format!("{:?}", PairRefusal::NotFound),
        format!("{:?}", Refusal::NotBound(op::CREATE_CONNECTION)),
    ];
    for o in outputs {
        for needle in ["f6", "e5", "d4", "c3", "b2", "a1", "7e", "3d", "F6", "A1"] {
            assert!(!o.contains(needle), "{o}");
        }
    }
}

// ---- size ------------------------------------------------------------------------

#[test]
fn the_gate_stays_small_and_const() {
    const G: Gate = Gate::new();
    assert_eq!(G.phase(), PairPhase::Off);
    assert!(
        core::mem::size_of::<Gate>() <= 192,
        "{}",
        core::mem::size_of::<Gate>()
    );
}
