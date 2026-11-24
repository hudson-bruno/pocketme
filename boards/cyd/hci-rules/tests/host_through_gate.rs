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

#[cfg(feature = "pair")]
mod pair_through_gate {
    use super::*;
    use hci_rules::op;
    use pad_host::PairEnd;
    use pad_proto::hci::BdAddr;

    const NEW: BdAddr = BdAddr([0x16, 0x25, 0x34, 0x43, 0x52, 0x61]);
    const NEW_KEY: [u8; 16] = [
        0xC3, 0x1F, 0x86, 0x4A, 0xE9, 0x27, 0x5D, 0xB0, 0x73, 0x0E, 0xA8, 0x64, 0x3B, 0xF2, 0x99,
        0x15,
    ];

    struct TimedGate {
        gate: Gate,
        seen: usize,
        checked: usize,
        observed: usize,
    }

    impl TimedGate {
        fn new(s: &Sim) -> Self {
            Self {
                gate: gate_for_saved(s),
                seen: s.wire.len(),
                checked: 0,
                observed: 0,
            }
        }

        fn sync(&mut self, s: &Sim) {
            while self.seen < s.wire.len() {
                let (out, p) = &s.wire[self.seen];
                self.seen += 1;
                if *out {
                    assert_eq!(
                        self.gate.check_at(p, s.now),
                        Ok(()),
                        "packet {} at {} ms: {:02x?}",
                        self.checked,
                        s.now,
                        &p[..p.len().min(8)]
                    );
                    self.checked += 1;
                } else {
                    self.gate.observe_at(p, s.now);
                    self.observed += 1;
                }
            }
        }

        fn feed(&mut self, s: &mut Sim, p: &[u8]) {
            s.feed(p);
            self.sync(s);
        }

        fn tick(&mut self, s: &mut Sim) {
            s.tick();
            self.sync(s);
        }

        fn advance(&mut self, s: &mut Sim, ms: u64) {
            let end = s.now + ms;
            while s.now < end {
                s.now = (s.now + 10).min(end);
                self.tick(s);
            }
        }
    }

    #[test]
    fn happy_pairing_passes_gate_with_zero_refusals() {
        let mut s = Sim::new(&[]);
        s.until_waiting();
        let mut tg = TimedGate::new(&s);
        tg.gate.arm(s.now, 66_000).unwrap();
        s.host.pair_begin(s.now, 60_000, &mut s.radio).unwrap();
        tg.sync(&s);
        tg.advance(&mut s, 100);

        tg.feed(&mut s, &ev_ext_inquiry(NEW, PAD_COD, b"Test Gamepad"));
        tg.tick(&mut s);
        let tail = s.host.pair_found(0).expect("found pad").tail;
        let addr = s.host.pair_target(0, tail).expect("target");
        tg.gate.bind(addr.whole_address(), s.now, 36_000).unwrap();
        s.host.pair_choose(0, s.now, 30_000, &mut s.radio).unwrap();
        tg.sync(&s);
        tg.advance(&mut s, 50);

        tg.feed(&mut s, &ev_conn_complete(0, H, NEW));
        tg.advance(&mut s, 20);
        tg.feed(&mut s, &ev_addr(0x17, NEW));
        tg.feed(&mut s, &ev_addr(0x31, NEW));
        tg.feed(&mut s, &ev_io_cap_response(NEW));
        tg.feed(&mut s, &ev_user_confirmation(NEW));
        tg.feed(&mut s, &ev_ssp_complete(NEW));
        tg.feed(&mut s, &ev_link_key(NEW, NEW_KEY));
        tg.feed(&mut s, &ev_auth(0, H));
        tg.feed(&mut s, &ev_encrypted(H));
        tg.advance(&mut s, 50);

        assert_eq!(s.host.pair_outcome(), Some(PairEnd::Saved));
        let pending = s.host.take_pending_save().expect("pending save");
        assert!(tg.gate.allow(pending.addr().whole_address()));
        assert!(tg.checked > 5 && tg.observed > 5);
    }

    #[test]
    fn user_cancel_passes_gate_with_zero_refusals() {
        let mut s = Sim::new(&[]);
        s.until_waiting();
        let mut tg = TimedGate::new(&s);
        tg.gate.arm(s.now, 66_000).unwrap();
        s.host.pair_begin(s.now, 60_000, &mut s.radio).unwrap();
        tg.sync(&s);
        tg.advance(&mut s, 200);

        assert!(s.host.pair_cancel(s.now, &mut s.radio));
        tg.gate.disarm();
        tg.sync(&s);
        tg.advance(&mut s, 100);
        assert_eq!(s.host.pair_outcome(), Some(PairEnd::Cancelled));
    }

    #[test]
    fn search_timeout_within_gate_margin_passes_with_zero_refusals() {
        let mut s = Sim::new(&[]);
        s.until_waiting();
        let mut tg = TimedGate::new(&s);
        tg.gate.arm(s.now, 66_000).unwrap();
        s.host.pair_begin(s.now, 60_000, &mut s.radio).unwrap();
        tg.sync(&s);
        tg.advance(&mut s, 60_500);
        assert_eq!(s.host.pair_outcome(), Some(PairEnd::Timeout));
        tg.gate.disarm();
    }

    #[test]
    fn old_pin_request_passes_gate_with_zero_refusals() {
        let mut s = Sim::new(&[]);
        s.until_waiting();
        let mut tg = TimedGate::new(&s);
        tg.gate.arm(s.now, 66_000).unwrap();
        s.host.pair_begin(s.now, 60_000, &mut s.radio).unwrap();
        tg.sync(&s);
        tg.advance(&mut s, 50);

        tg.feed(&mut s, &ev_ext_inquiry(NEW, PAD_COD, b"Old PIN Pad"));
        tg.tick(&mut s);
        let tail = s.host.pair_found(0).expect("found").tail;
        let addr = s.host.pair_target(0, tail).unwrap();
        tg.gate.bind(addr.whole_address(), s.now, 36_000).unwrap();
        s.host.pair_choose(0, s.now, 30_000, &mut s.radio).unwrap();
        tg.sync(&s);
        tg.advance(&mut s, 50);

        tg.feed(&mut s, &ev_conn_complete(0, H, NEW));
        tg.advance(&mut s, 20);
        tg.feed(&mut s, &ev_addr(0x17, NEW));
        tg.feed(&mut s, &ev_addr(0x16, NEW));
        tg.advance(&mut s, 100);
        assert_eq!(s.host.pair_outcome(), Some(PairEnd::OldPin));
    }

    #[test]
    fn forget_removes_pad_from_gate() {
        let mut s = Sim::new(&[(PAD, KEY)]);
        s.until_waiting();
        let mut tg = TimedGate::new(&s);
        assert_eq!(tg.gate.saved(), 1);
        let addr = s.host.forget(0, s.now, &mut s.radio).expect("forgot");
        assert!(tg.gate.forget(addr.whole_address()));
        tg.sync(&s);
        tg.advance(&mut s, 100);
        assert_eq!(tg.gate.saved(), 0);

        // Accept and Link Key Reply for the forgotten pad are refused again.
        let mut accept = vec![0x01];
        accept.extend_from_slice(&op::ACCEPT_CONNECTION_REQUEST.to_le_bytes());
        accept.push(7);
        accept.extend_from_slice(&PAD.0);
        accept.push(0x01);
        assert_eq!(
            tg.gate.check_at(&accept, s.now),
            Err(Refusal::NotSaved(op::ACCEPT_CONNECTION_REQUEST))
        );

        let mut reply = vec![0x01];
        reply.extend_from_slice(&op::LINK_KEY_REQUEST_REPLY.to_le_bytes());
        reply.push(22);
        reply.extend_from_slice(&PAD.0);
        reply.extend_from_slice(&KEY);
        assert_eq!(
            tg.gate.check_at(&reply, s.now),
            Err(Refusal::NotSaved(op::LINK_KEY_REQUEST_REPLY))
        );
    }
}
