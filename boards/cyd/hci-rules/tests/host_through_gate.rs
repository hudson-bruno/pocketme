//! Every packet the real gamepad host (crates/pad-host) sends, in whole
//! scripted sessions, passes the gate that knows only the saved pad: the
//! gate is not in the host's way, and the host never needs anything the
//! gate refuses. The controller and the pad are pad-host's own test
//! scripts (taken in by path), with made-up addresses and keys.

#[path = "../../../../crates/pad-host/tests/common/mod.rs"]
mod common;

use common::fixtures::{self, Session};
use common::*;
use hci_rules::{Gate, Refusal};
use pad_host::Mode;
use pad_host::inject::{self, SwitchInput};

fn gate_for_saved(s: &Sim) -> Gate {
    let mut g = Gate::new();
    let mut i = 0;
    while let Some(a) = s.host.saved_address(i) {
        assert!(g.allow(a));
        i += 1;
    }
    g
}

/// Each packet sent so far through the gate; returns how many.
fn all_pass(s: &Sim, g: &Gate) -> usize {
    for (n, p) in s.radio.out.iter().enumerate() {
        assert_eq!(
            g.check(p),
            Ok(()),
            "packet {n}: {:02x?}",
            &p[..p.len().min(4)]
        );
    }
    s.radio.out.len()
}

fn session(f: &Session) -> usize {
    let mut s = Sim::new(&[(PAD, KEY)]);
    let g = gate_for_saved(&s);
    assert_eq!(g.saved(), 1);
    s.until_waiting();
    let (_, intr) = s.reconnect(f);
    if f.name == fixtures::SWITCH_WORKS.name {
        let mut pad = SwitchPad::new(f);
        s.switch_runs(&mut pad, intr, 3_000);
    } else {
        for r in f.reports {
            s.report(intr, &hex(r));
            s.advance(10);
        }
    }
    assert!(s.host.connected(), "{}", s.tail_log());
    s.feed(&ev_disconnected(H, 0x13));
    s.advance(1_100);
    assert!(s.host.waiting());
    all_pass(&s, &g)
}

#[test]
fn whole_reconnects_in_every_mode_pass_the_gate() {
    for f in [
        &fixtures::SWITCH_WORKS,
        &fixtures::XINPUT,
        &fixtures::ANDROID,
        &fixtures::DINPUT,
    ] {
        let n = session(f);
        assert!(n > 20, "{n} packets");
    }
}

/// The unhappy paths: a failed link, a stranger, a pairing attempt, a
/// silent pad, then a good reconnect after all of it.
#[test]
fn refusals_failures_and_timeouts_pass_the_gate() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    let g = gate_for_saved(&s);
    s.until_waiting();
    s.reconnect(&fixtures::SWITCH_FAILS);
    s.advance(200);
    s.feed(&ev_disconnected(
        H,
        fixtures::SWITCH_FAILS.end_reason.unwrap(),
    ));
    s.advance(1_100);
    assert!(!s.pad_calls(STRANGER, PAD_COD));
    s.feed(&ev_conn_request(STRANGER, 0x5A020C));
    s.advance(2_000);
    assert!(s.pad_calls(PAD, PAD_COD));
    s.feed(&ev_addr(0x17, PAD));
    s.feed(&ev_addr(0x31, PAD));
    s.feed(&ev_addr(0x16, PAD));
    s.feed(&ev_link_key(PAD, [0x77; 16]));
    s.advance(1_100);
    assert!(s.host.waiting(), "{}", s.tail_log());
    assert!(s.pad_calls(PAD, PAD_COD));
    s.advance(60_000);
    assert!(s.host.waiting(), "{}", s.tail_log());
    let (_, intr) = s.reconnect(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected());
    all_pass(&s, &g);
}

/// The same packets through a gate that knows NO pad: the accept and the
/// key reply are exactly what it stops, so the board's gate list matters.
#[test]
fn without_the_saved_address_the_gate_stops_the_accept_and_the_key() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    s.reconnect(&fixtures::SWITCH_WORKS);
    let g = Gate::new();
    let mut stopped = Vec::new();
    for p in &s.radio.out {
        if let Err(e) = g.check(p) {
            stopped.push(e);
        }
    }
    assert!(stopped.contains(&Refusal::NotSaved(hci_rules::op::ACCEPT_CONNECTION_REQUEST)));
    assert!(stopped.contains(&Refusal::NotSaved(hci_rules::op::LINK_KEY_REQUEST_REPLY)));
    assert!(
        stopped.iter().all(|e| matches!(e, Refusal::NotSaved(_))),
        "{stopped:?}"
    );
}

/// Injected input sends nothing at all to the radio.
#[test]
fn injection_sends_nothing() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    let before = s.radio.out.len();
    assert!(s.host.inject_begin(s.now, Mode::Switch, &mut s.radio));
    let r = inject::switch_report(&SwitchInput::default());
    for _ in 0..50 {
        assert!(s.host.inject_report(s.now, &r, &mut s.radio));
        s.advance(10);
    }
    s.host.inject_end(s.now, 0x13, &mut s.radio);
    s.advance(100);
    assert_eq!(s.radio.out.len(), before);
}

/// Every packet, both ways, in order: the gate sees what the controller
/// says (`observe`, as the board's loop does) and checks what the host
/// sends. Returns (packets in, packets out).
fn replay_wire(s: &Sim, g: &mut Gate) -> (usize, usize) {
    let (mut ins, mut outs) = (0, 0);
    for (n, (out, p)) in s.wire.iter().enumerate() {
        if *out {
            assert_eq!(
                g.check(p),
                Ok(()),
                "packet {n}: {:02x?}",
                &p[..p.len().min(8)]
            );
            outs += 1;
        } else {
            g.observe(p);
            ins += 1;
        }
    }
    (ins, outs)
}

/// Round 6: the pad switched off and on. The supervision timeout after
/// the role change, the silence guard's Disconnect, a new call on a new
/// handle while the old link is still closing, its own supervision
/// timeout, the late end of the old link, and the pad calling again while
/// linked: all of it passes the gate that sees the controller's events,
/// and the timeout goes to the saved pad's handle only.
#[test]
fn a_pad_switched_off_and_on_passes_the_gate() {
    use pad_proto::hci::op;
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.confirm_disconnect = false;
    s.until_waiting();
    assert!(s.pad_calls(PAD, fixtures::SWITCH_WORKS.cod));
    s.feed(&ev_role(0, PAD, 0));
    s.pad_uses_saved_key(PAD);
    let (_, intr) = s.pad_connects_hid(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected());
    // Switched off: the guard drops the link.
    s.advance(1_100);
    assert!(s.host.waiting(), "{}", s.tail_log());
    // Switched on: a new handle, the old link still closing.
    s.handle = 0x0081;
    let (_, intr) = s.reconnect(&fixtures::SWITCH_WORKS);
    s.feed(&ev_role(0, PAD, 0));
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected(), "{}", s.tail_log());
    s.feed(&ev_disconnected(H, 0x08));
    // The pad calls again while linked: the old link is replaced.
    s.feed(&ev_conn_request(PAD, PAD_COD));
    s.feed(&ev_disconnected(0x0081, 0x16));
    s.handle = 0x0082;
    s.feed(&ev_conn_complete(0, 0x0082, PAD));
    s.feed(&ev_role(0, PAD, 0));
    s.advance(100);
    let lsto = lsto_written(&s);
    assert_eq!(
        lsto,
        vec![(H, 3200), (0x0081, 3200), (0x0082, 3200)],
        "{}",
        s.tail_log()
    );
    let mut g = gate_for_saved(&s);
    let (ins, outs) = replay_wire(&s, &mut g);
    assert!(ins > 100 && outs > 50, "{ins} in, {outs} out");
    assert_eq!(g.linked(), 1, "only the newest link is still up");
    // The same packets, but the gate never saw the controller's events:
    // exactly the three supervision timeouts are stopped, nothing else.
    let g = gate_for_saved(&s);
    let stopped: Vec<_> = s
        .wire
        .iter()
        .filter(|(out, _)| *out)
        .filter_map(|(_, p)| {
            g.check(p)
                .err()
                .map(|e| (u16::from_le_bytes([p[1], p[2]]), e))
        })
        .collect();
    let w = op::WRITE_LINK_SUPERVISION_TIMEOUT;
    assert_eq!(stopped, vec![(w, Refusal::NotSaved(w)); 3]);
}
