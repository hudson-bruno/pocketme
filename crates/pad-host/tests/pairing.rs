//! The pairing window through the pad host (crate feature `pair`), with
//! the scripted controller and gamepad of `common`: search, the found
//! list, the pick, "just works" pairing, the new key handed to the board
//! once, then the pad's HID start as a saved pad's; and every way a
//! window can end early. All addresses, names and keys are made up.
#![cfg(feature = "pair")]

mod common;

use board_api::{FoundPad, SavedPad};
use common::fixtures;
use common::*;
use pad_host::{PadAddr, PadHost, PairEnd, PairError, PairPhase, PairRefused};
use pad_proto::hci::{BdAddr, op};
use pad_proto::l2cap::{self, code};

/// A new pad, never saved before.
const NEW: BdAddr = BdAddr([0x16, 0x25, 0x34, 0x43, 0x52, 0x61]);
/// Two more saved pads, to fill the table.
const OLD_B: BdAddr = BdAddr([0x2B, 0x3B, 0x4B, 0x5B, 0x6B, 0x7B]);
const OLD_C: BdAddr = BdAddr([0x2C, 0x3C, 0x4C, 0x5C, 0x6C, 0x7C]);
const OLD_D: BdAddr = BdAddr([0x2D, 0x3D, 0x4D, 0x5D, 0x6D, 0x7D]);
/// A phone heard in the search: never listed.
const PHONE: BdAddr = BdAddr([0x81, 0x92, 0xA3, 0xB4, 0xC5, 0xD6]);
const PHONE_COD: u32 = 0x5A020C;
/// The key a new pairing gives, and another for re-pairing.
const NEW_KEY: [u8; 16] = [
    0xC3, 0x1F, 0x86, 0x4A, 0xE9, 0x27, 0x5D, 0xB0, 0x73, 0x0E, 0xA8, 0x64, 0x3B, 0xF2, 0x99, 0x15,
];
const KEY_B: [u8; 16] = [
    0x5A, 0x6B, 0x7C, 0x8D, 0x9E, 0xAF, 0xB0, 0xC1, 0xD2, 0xE3, 0xF4, 0x05, 0x16, 0x27, 0x38, 0x49,
];
/// Names of other people's pads: never in a log line.
const SECRET_NAME: &[u8] = b"Quokka Gamepad";
const OTHER_NAME: &[u8] = b"Wombat Pad";
/// The pad's own name in the search (cut to 24 bytes in the list).
const LONG_NAME: &[u8] = b"Pro Controller with a very long name";

fn tail(a: BdAddr) -> [u8; 2] {
    [a.0[1], a.0[0]]
}

fn sim(saved: &[(BdAddr, [u8; 16])]) -> Sim {
    let mut s = Sim::new(saved);
    s.until_waiting();
    s.cmds.clear();
    s
}

fn begin(s: &mut Sim, for_ms: u64) {
    s.host
        .pair_begin(s.now, for_ms, &mut s.radio)
        .unwrap_or_else(|e| panic!("window refused: {e:?}\n{}", s.tail_log()));
    s.settle();
    assert_eq!(s.host.pair_phase(), PairPhase::Searching);
    s.take_cmd(op::INQUIRY);
}

fn heard(s: &mut Sim, addr: BdAddr, cod: u32, name: &[u8]) {
    s.feed(&ev_ext_inquiry(addr, cod, name));
}

/// Picks found pad `i` as the board does: the address for the gate
/// first (with the tail the user saw), then the choice.
fn choose(s: &mut Sim, i: usize, for_ms: u64) -> Result<PadAddr, PairRefused> {
    let t = s.host.pair_found(i).map(|f| f.tail).unwrap_or([0; 2]);
    let a = s.host.pair_target(i, t).ok_or(PairRefused::NoSuchPad)?;
    s.host.pair_choose(i, s.now, for_ms, &mut s.radio)?;
    s.settle();
    Ok(a)
}

/// The search hears a phone, the new pad (twice) and another gamepad.
fn search_hears_three(s: &mut Sim) {
    heard(s, PHONE, PHONE_COD, b"Someone's phone");
    heard(s, NEW, PAD_COD, SECRET_NAME);
    heard(s, NEW, PAD_COD, SECRET_NAME);
    heard(s, STRANGER, PAD_COD, OTHER_NAME);
}

/// The link to `addr` comes up and pairs, up to the key notification.
fn link_to_key(s: &mut Sim, addr: BdAddr, key: [u8; 16]) {
    s.take_cmd(op::CREATE_CONNECTION);
    s.feed(&ev_conn_complete(0, H, addr));
    s.take_cmd(op::AUTHENTICATION_REQUESTED);
    s.feed(&ev_addr(0x17, addr));
    assert_eq!(s.take_cmd(op::LINK_KEY_REQUEST_NEGATIVE_REPLY), addr.0);
    s.feed(&ev_addr(0x31, addr));
    let r = s.take_cmd(op::IO_CAPABILITY_REQUEST_REPLY);
    assert_eq!(&r[..], &cat(&[&addr.0, &[0x03, 0x00, 0x04]])[..]);
    s.feed(&ev_io_cap_response(addr));
    s.feed(&ev_user_confirmation(addr));
    assert_eq!(s.take_cmd(op::USER_CONFIRMATION_REQUEST_REPLY), addr.0);
    s.feed(&ev_ssp_complete(addr));
    s.feed(&ev_link_key(addr, key));
}

/// From the key notification to encryption on: the window ends saved.
fn key_to_encryption(s: &mut Sim) {
    assert!(
        s.host.take_pending_save().is_none(),
        "nothing before encryption"
    );
    s.feed(&ev_auth(0, H));
    s.take_cmd(op::SET_CONNECTION_ENCRYPTION);
    s.feed(&ev_encrypted(H));
}

/// After a pairing, our side opens SDP (HID record, then Device ID),
/// then HID control and interrupt. Returns our interrupt channel.
fn hid_up_after_pairing(s: &mut Sim, f: &fixtures::Session) -> u16 {
    s.advance(20);
    s.pad_answers_info();
    let sdp_ours = s.pad_accepts(l2cap::PSM_SDP, PAD_SDP);
    s.pad_answers_sdp(sdp_ours, f.sdp_hid, 0x1124);
    s.pad_answers_sdp(sdp_ours, f.sdp_pnp, 0x1200);
    let (id, d) = s.take_sig(code::DISCONNECTION_REQUEST);
    s.pad_sig(code::DISCONNECTION_RESPONSE, id, &d);
    s.advance(40);
    s.pad_accepts(l2cap::PSM_HID_CONTROL, PAD_CTRL);
    s.advance(40);
    s.pad_accepts(l2cap::PSM_HID_INTERRUPT, PAD_INTR)
}

/// Nothing a window must never send went out, ever: no PIN reply, no
/// call or pairing step for anyone but `target`, inquiry scan never on.
fn assert_only_target(s: &Sim, target: Option<BdAddr>) {
    for (o, p) in &s.history {
        match *o {
            op::PIN_CODE_REQUEST_REPLY => panic!("a PIN reply went out"),
            op::CREATE_CONNECTION
            | op::IO_CAPABILITY_REQUEST_REPLY
            | op::USER_CONFIRMATION_REQUEST_REPLY => {
                assert_eq!(
                    Some(&p[..6]),
                    target.as_ref().map(|t| &t.0[..]),
                    "{}",
                    op::name(*o)
                );
            }
            op::WRITE_SCAN_ENABLE => assert_eq!(p[0] & 1, 0, "inquiry scan on"),
            _ => {}
        }
    }
}

/// No log line holds a name heard in the search, or the new keys.
fn assert_private(s: &Sim) {
    s.assert_key_never_printed();
    s.assert_no_full_address();
    for l in &s.radio.logs {
        for name in ["Quokka", "Wombat", "Someone's"] {
            assert!(!l.contains(name), "a found name in the log: {l}");
        }
        assert_no_bytes(l, &NEW_KEY);
        assert_no_bytes(l, &KEY_B);
        for a in [NEW, OLD_B, PHONE] {
            let whole: String =
                a.0.iter()
                    .rev()
                    .take(4)
                    .map(|b| format!("{b:02x}"))
                    .collect();
            assert!(
                !l.to_lowercase().replace(':', "").contains(&whole),
                "address in: {l}"
            );
        }
    }
}

fn assert_no_bytes(text: &str, key: &[u8; 16]) {
    let low = text.to_lowercase();
    let hexs: String = key.iter().map(|b| format!("{b:02x}")).collect();
    assert!(!low.contains(&hexs[..8]), "key bytes in: {text}");
    assert!(!low.contains(&hexs[24..]), "key bytes in: {text}");
    let dec = format!("{}, {}, {}", key[0], key[1], key[2]);
    assert!(!text.contains(&dec), "key bytes in: {text}");
}

fn no_key_change_ignored(s: &Sim) {
    assert!(!s.log_has("key change '"), "{}", s.tail_log());
}

// ---------------------------------------------------------------- tests

/// The whole way: search (a phone is not listed, a pad heard twice is
/// listed once), pick with the tail the user saw, "just works", the key
/// once encryption is on, taken once; then HID opens and the pad plays,
/// counted as one call that got ready; later it calls back with the new
/// key and is taken.
#[test]
fn a_whole_pairing_hands_over_one_key_and_the_pad_plays() {
    let mut s = sim(&[]);
    assert_eq!(s.host.pair_phase(), PairPhase::Off);
    assert_eq!(s.host.pair_outcome(), None);
    begin(&mut s, 60_000);
    assert_eq!(s.host.pair_deadline(), Some(s.now + 60_000));
    s.wakes.clear();
    search_hears_three(&mut s);
    heard(&mut s, PAD, PAD_COD, LONG_NAME);
    s.tick();
    assert!(!s.wakes.is_empty(), "a growing list wakes the console");
    assert_eq!(s.host.pair_found_count(), 3);
    assert_eq!(
        s.host.pair_found(0),
        Some(FoundPad::new(tail(NEW), SECRET_NAME))
    );
    assert_eq!(
        s.host.pair_found(1),
        Some(FoundPad::new(tail(STRANGER), OTHER_NAME))
    );
    let long = s.host.pair_found(2).unwrap();
    assert_eq!(long.name(), &LONG_NAME[..24], "cut to 24 bytes");
    assert_eq!(s.host.pair_found(3), None);
    assert!(!s.sent(op::CREATE_CONNECTION), "nobody is called by itself");

    // The board checks the tail the user saw, and sends nothing for it.
    let before = s.all_cmds.len();
    assert_eq!(s.host.pair_target(0, tail(STRANGER)), None, "wrong tail");
    assert_eq!(s.host.pair_target(7, tail(NEW)), None, "no pad 7");
    let t = s.host.pair_target(0, tail(NEW)).unwrap();
    assert_eq!(s.all_cmds.len(), before);
    assert_eq!(t.whole_address(), NEW.0);
    assert_eq!(t.tail(), [0x25, 0x16]);
    assert_eq!(format!("{t}"), "xx:xx:xx:xx:25:16");
    assert_eq!(format!("{t:?}"), "PadAddr(xx:xx:xx:xx:25:16)");

    assert_eq!(choose(&mut s, 0, 30_000), Ok(t));
    assert_eq!(s.host.pair_phase(), PairPhase::Pairing);
    assert_eq!(
        s.host.pair_target(0, tail(NEW)),
        None,
        "only while searching"
    );
    s.take_cmd(op::INQUIRY_CANCEL);
    link_to_key(&mut s, NEW, NEW_KEY);
    key_to_encryption(&mut s);
    assert_eq!(s.host.pair_phase(), PairPhase::Off);
    assert_eq!(s.host.pair_outcome(), Some(PairEnd::Saved));
    assert_eq!(s.host.saved_pads(), 1);
    assert_eq!(s.host.saved_tail(0), Some(tail(NEW)));

    // The board refuses a new window until it has the key.
    assert_eq!(
        s.host.pair_begin(s.now, 60_000, &mut s.radio),
        Err(PairRefused::SaveWaiting)
    );
    let p = s.host.take_pending_save().expect("the new pairing");
    assert_eq!(p.addr(), t);
    assert_eq!(p.key(), &NEW_KEY);
    assert_eq!(p.key_type(), 0x04);
    let shown = format!("{p:?}");
    assert_no_bytes(&shown, &NEW_KEY);
    assert!(shown.contains("xx:xx:xx:xx:25:16"), "{shown}");
    assert!(s.host.take_pending_save().is_none(), "once");
    drop(p);

    // HID, as a saved pad's call from here on.
    s.pad_name = Some(fixtures::SWITCH_WORKS.name.to_string());
    let intr = hid_up_after_pairing(&mut s, &fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected(), "{}", s.tail_log());
    assert_eq!(s.host.connected_place(), Some(0));
    assert_eq!(
        s.host.saved_pad(0),
        Some(SavedPad {
            tail: tail(NEW),
            connected: true
        })
    );
    let c = s.host.counters();
    assert_eq!((c.attempts, c.ready, c.failed), (1, 1, [0; 4]), "{c:?}");
    pad.buttons = [0x08, 0, 0];
    s.switch_runs(&mut pad, intr, 30);
    assert_ne!(s.host.take_pressed(), 0);

    // It goes, and calls back later with its new key.
    s.feed(&ev_disconnected(H, 0x13));
    s.advance(1_100);
    assert!(s.host.waiting());
    assert_eq!(s.host.connected_place(), None);
    assert!(s.pad_calls(NEW, PAD_COD), "{}", s.tail_log());
    s.feed(&ev_addr(0x17, NEW));
    let r = s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    assert_eq!(&r[..6], &NEW.0);
    assert_eq!(&r[6..22], &NEW_KEY);
    assert_only_target(&s, Some(NEW));
    no_key_change_ignored(&s);
    assert_private(&s);

    // Nothing else shows a key or a name either.
    let mut texts = vec![
        format!("{:?}", s.host.counters()),
        format!("{:?}", s.host.stats()),
        format!("{:?}", s.host.pair_found(0)),
        format!("{:?}", s.host.saved_pad(0)),
        format!("{:?}", s.host.pair_outcome()),
    ];
    for e in s.take_events() {
        texts.push(format!("{e:?}"));
    }
    for t in &texts {
        assert_no_bytes(t, &NEW_KEY);
        assert!(!t.contains("Quokka"), "{t}");
        assert_no_address(t);
    }
}

/// The window's phases are not calls: a search and a pairing that is
/// cancelled count nothing.
#[test]
fn a_window_counts_no_call() {
    let mut s = sim(&[(PAD, KEY)]);
    begin(&mut s, 60_000);
    heard(&mut s, NEW, PAD_COD, SECRET_NAME);
    choose(&mut s, 0, 30_000).unwrap();
    s.take_cmd(op::INQUIRY_CANCEL);
    link_to_key(&mut s, NEW, NEW_KEY);
    assert!(s.host.pair_cancel(s.now, &mut s.radio));
    s.settle();
    s.advance(2_000);
    assert!(s.host.waiting());
    assert_eq!(s.host.counters().attempts, 0, "{:?}", s.host.counters());
    assert_eq!(s.host.counters().failed, [0; 4]);
}

/// Cancel at every stage: searching, calling, linked and pairing, key
/// held before encryption, and after the save but before the board took
/// it. Each time: nothing to take, the table as the flash has it, the
/// host waits again, and nobody but the target was ever called.
#[test]
fn cancel_at_each_stage() {
    // Searching: the search stops.
    let mut s = sim(&[(PAD, KEY)]);
    begin(&mut s, 60_000);
    heard(&mut s, NEW, PAD_COD, SECRET_NAME);
    assert!(s.host.pair_cancel(s.now, &mut s.radio));
    s.settle();
    s.take_cmd(op::INQUIRY_CANCEL);
    assert_eq!(s.host.pair_outcome(), Some(PairEnd::Cancelled));
    assert_eq!(s.host.pair_found_count(), 0, "the list is emptied");
    assert!(!s.host.pair_cancel(s.now, &mut s.radio), "nothing left");
    s.advance(1_000);
    assert!(s.host.waiting());
    assert_only_target(&s, None);

    // Calling: the call is taken back.
    let mut s = sim(&[(PAD, KEY)]);
    begin(&mut s, 60_000);
    heard(&mut s, NEW, PAD_COD, SECRET_NAME);
    choose(&mut s, 0, 30_000).unwrap();
    s.take_cmd(op::INQUIRY_CANCEL);
    s.take_cmd(op::CREATE_CONNECTION);
    s.host.pair_cancel(s.now, &mut s.radio);
    s.settle();
    assert_eq!(s.take_cmd(op::CREATE_CONNECTION_CANCEL), NEW.0);
    s.feed(&ev_conn_complete(0x02, H, NEW));
    s.advance(1_200);
    assert!(s.host.waiting(), "{}", s.host.phase());

    // Linked, pairing: the pad is hung up.
    let mut s = sim(&[(PAD, KEY)]);
    begin(&mut s, 60_000);
    heard(&mut s, NEW, PAD_COD, SECRET_NAME);
    choose(&mut s, 0, 30_000).unwrap();
    s.take_cmd(op::CREATE_CONNECTION);
    s.feed(&ev_conn_complete(0, H, NEW));
    s.feed(&ev_addr(0x31, NEW));
    s.take_cmd(op::IO_CAPABILITY_REQUEST_REPLY);
    s.host.pair_cancel(s.now, &mut s.radio);
    s.settle();
    s.take_cmd(op::DISCONNECT);
    s.advance(1_200);
    assert!(s.host.waiting(), "{}", s.host.phase());
    assert_eq!(s.host.saved_pads(), 1);

    // Key held, encryption not on yet: the key is dropped.
    let mut s = sim(&[(PAD, KEY)]);
    begin(&mut s, 60_000);
    heard(&mut s, NEW, PAD_COD, SECRET_NAME);
    choose(&mut s, 0, 30_000).unwrap();
    link_to_key(&mut s, NEW, NEW_KEY);
    assert_eq!(s.host.saved_pads(), 2, "held in the table until the end");
    s.host.pair_cancel(s.now, &mut s.radio);
    s.settle();
    s.take_cmd(op::DISCONNECT);
    assert_eq!(s.host.saved_pads(), 1);
    assert_eq!(s.host.saved_tail(0), Some(tail(PAD)));
    assert!(s.host.take_pending_save().is_none());
    s.advance(1_200);
    assert!(s.host.waiting());

    // Saved, not taken yet: the pairing is zeroed, the key leaves the
    // table (the flash never got it) and the pad is let go.
    let mut s = sim(&[(PAD, KEY)]);
    begin(&mut s, 60_000);
    heard(&mut s, NEW, PAD_COD, SECRET_NAME);
    choose(&mut s, 0, 30_000).unwrap();
    link_to_key(&mut s, NEW, NEW_KEY);
    key_to_encryption(&mut s);
    assert_eq!(s.host.pair_outcome(), Some(PairEnd::Saved));
    assert!(s.host.pair_cancel(s.now, &mut s.radio));
    s.settle();
    assert!(s.host.take_pending_save().is_none(), "zeroed, gone");
    assert_eq!(s.host.saved_pads(), 1);
    assert_eq!(s.host.saved_tail(0), Some(tail(PAD)));
    s.take_cmd(op::DISCONNECT);
    s.advance(1_200);
    assert!(s.host.waiting(), "{}", s.host.phase());
    // It calls back: refused, it has no key here.
    s.cmds.clear();
    assert!(!s.pad_calls(NEW, PAD_COD));
    assert_only_target(&s, Some(NEW));
    no_key_change_ignored(&s);
    assert_private(&s);
}

/// The deadline closes the window by itself, searching and calling.
#[test]
fn the_deadline_closes_the_window() {
    let mut s = sim(&[(PAD, KEY)]);
    begin(&mut s, 3_000);
    heard(&mut s, NEW, PAD_COD, SECRET_NAME);
    s.advance(3_000);
    assert_eq!(s.host.pair_phase(), PairPhase::Off);
    assert_eq!(s.host.pair_outcome(), Some(PairEnd::Timeout));
    s.take_cmd(op::INQUIRY_CANCEL);
    assert_eq!(s.host.pair_target(0, tail(NEW)), None);
    assert_eq!(
        s.host.pair_choose(0, s.now, 30_000, &mut s.radio),
        Err(PairRefused::Host(PairError::NotScanning))
    );
    s.advance(500);
    assert!(s.host.waiting());

    let mut s = sim(&[(PAD, KEY)]);
    begin(&mut s, 60_000);
    heard(&mut s, NEW, PAD_COD, SECRET_NAME);
    choose(&mut s, 0, 4_000).unwrap();
    assert_eq!(s.host.pair_deadline(), Some(s.now + 4_000));
    s.take_cmd(op::CREATE_CONNECTION);
    s.advance(4_000);
    assert_eq!(s.host.pair_outcome(), Some(PairEnd::Timeout));
    assert_eq!(s.take_cmd(op::CREATE_CONNECTION_CANCEL), NEW.0);
    s.advance(1_200);
    assert!(s.host.waiting());
    assert!(s.host.take_pending_save().is_none());
    assert_eq!(s.host.counters().attempts, 0);
    assert_only_target(&s, Some(NEW));
}

/// A pad that asks for an old PIN is refused and let go.
#[test]
fn an_old_pin_ends_the_window() {
    let mut s = sim(&[]);
    begin(&mut s, 60_000);
    heard(&mut s, NEW, PAD_COD, SECRET_NAME);
    choose(&mut s, 0, 30_000).unwrap();
    s.take_cmd(op::CREATE_CONNECTION);
    s.feed(&ev_conn_complete(0, H, NEW));
    s.feed(&ev_addr(0x16, NEW));
    assert_eq!(s.take_cmd(op::PIN_CODE_REQUEST_NEGATIVE_REPLY), NEW.0);
    assert_eq!(s.host.pair_outcome(), Some(PairEnd::OldPin));
    s.take_cmd(op::DISCONNECT);
    s.advance(1_200);
    assert!(s.host.waiting());
    assert!(s.host.take_pending_save().is_none());
    assert_eq!(s.host.saved_pads(), 0);
    assert_only_target(&s, Some(NEW));
}

/// Four pads saved: a new one cannot be picked (nothing is called); a
/// saved one can (pairing again).
#[test]
fn a_full_table_refuses_a_new_pad_only() {
    let mut s = sim(&[(PAD, KEY), (OLD_B, KEY_B), (OLD_C, KEY_B), (OLD_D, KEY_B)]);
    assert_eq!(s.host.saved_pads(), 4);
    begin(&mut s, 60_000);
    heard(&mut s, NEW, PAD_COD, SECRET_NAME);
    heard(&mut s, OLD_C, PAD_COD, OTHER_NAME);
    assert_eq!(
        choose(&mut s, 0, 30_000),
        Err(PairRefused::Host(PairError::Full))
    );
    assert!(!s.sent(op::CREATE_CONNECTION));
    assert_eq!(s.host.pair_phase(), PairPhase::Searching, "still searching");
    assert_eq!(
        s.host.pair_choose(5, s.now, 30_000, &mut s.radio),
        Err(PairRefused::NoSuchPad)
    );
    let t = choose(&mut s, 1, 30_000).unwrap();
    assert_eq!(t.whole_address(), OLD_C.0);
    link_to_key(&mut s, OLD_C, NEW_KEY);
    key_to_encryption(&mut s);
    let p = s.host.take_pending_save().unwrap();
    assert_eq!(p.addr().whole_address(), OLD_C.0);
    assert_eq!(s.host.saved_pads(), 4);
    assert_eq!(s.host.saved_tail(2), Some(tail(OLD_C)), "same place");
    assert_only_target(&s, Some(OLD_C));
}

/// The saved pad paired again: its new key replaces the old one, here
/// and (through the pending save) in the flash; it calls back with it.
#[test]
fn a_saved_pad_paired_again_gets_its_new_key() {
    let mut s = sim(&[(PAD, KEY)]);
    begin(&mut s, 60_000);
    heard(&mut s, PAD, PAD_COD, b"Pro Controller");
    let t = choose(&mut s, 0, 30_000).unwrap();
    assert_eq!(t.whole_address(), PAD.0);
    link_to_key(&mut s, PAD, NEW_KEY);
    key_to_encryption(&mut s);
    let p = s.host.take_pending_save().unwrap();
    assert_eq!((p.addr(), p.key()), (t, &NEW_KEY));
    assert_eq!(s.host.saved_pads(), 1);
    s.feed(&ev_disconnected(H, 0x13));
    s.advance(1_100);
    assert!(s.pad_calls(PAD, PAD_COD));
    s.feed(&ev_addr(0x17, PAD));
    let r = s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    assert_eq!(&r[6..22], &NEW_KEY, "the new key, not the old one");
    assert_only_target(&s, Some(PAD));
    assert_private(&s);
}

/// A saved pad that calls during the search is refused ("no room now")
/// and not counted; after the window it is taken as always.
#[test]
fn a_saved_pad_calling_during_the_search_waits_for_the_end() {
    let mut s = sim(&[(PAD, KEY)]);
    begin(&mut s, 60_000);
    s.cmds.clear();
    assert!(!s.pad_calls(PAD, PAD_COD));
    assert_eq!(
        s.take_cmd(op::REJECT_CONNECTION_REQUEST),
        cat(&[&PAD.0, &[0x0D]])
    );
    assert!(!s.sent(op::ACCEPT_CONNECTION_REQUEST));
    assert_eq!(s.host.counters().attempts, 0);
    assert!(s.host.pair_cancel(s.now, &mut s.radio));
    s.settle();
    s.advance(500);
    assert!(s.host.waiting());
    assert!(s.pad_calls(PAD, PAD_COD), "{}", s.tail_log());
    s.pad_uses_saved_key(PAD);
    assert_eq!(s.host.counters().attempts, 1);
    assert_only_target(&s, None);
}

/// Opening a window lets a linked pad go (one call, counted as lost) and
/// ends an injected pad; no injection starts while a window is open.
#[test]
fn a_window_lets_the_linked_pad_and_injection_go() {
    let mut s = sim(&[(PAD, KEY)]);
    let (_, intr) = s.reconnect(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected());
    assert!(s.host.pair_begin(s.now, 60_000, &mut s.radio).is_ok());
    s.settle();
    assert!(!s.host.connected());
    assert!(s.sent(op::DISCONNECT));
    s.advance(20);
    assert!(
        s.sent(op::INQUIRY),
        "the search starts once the hang-up is answered"
    );
    assert_eq!(s.host.counters().attempts, 1);
    assert!(
        !s.host
            .inject_begin(s.now, pad_host::Mode::Switch, &mut s.radio)
    );
    s.host.pair_cancel(s.now, &mut s.radio);
    s.settle();
    s.advance(1_200);
    assert!(
        s.host
            .inject_begin(s.now, pad_host::Mode::Switch, &mut s.radio)
    );
    assert!(s.host.pair_begin(s.now, 60_000, &mut s.radio).is_ok());
    assert!(!s.host.injecting(), "the window ends the injection");
}

/// Forget a saved pad by place: its address comes back for the board's
/// store and gate, the key leaves the table, a linked pad is hung up, and
/// the places after it move down.
#[test]
fn forget_drops_the_key_and_hangs_up() {
    let mut s = sim(&[(OLD_B, KEY_B), (PAD, KEY)]);
    assert!(s.pad_calls(PAD, PAD_COD));
    s.pad_uses_saved_key(PAD);
    assert_eq!(s.host.connected_place(), Some(1));
    assert_eq!(s.host.saved_pad(0).map(|p| p.connected), Some(false));
    assert_eq!(s.host.forget(4, s.now, &mut s.radio), None);
    s.cmds.clear();
    let a = s.host.forget(1, s.now, &mut s.radio).unwrap();
    s.settle();
    assert_eq!(a.whole_address(), PAD.0);
    assert_eq!(
        s.take_cmd(op::DISCONNECT),
        cat(&[&H.to_le_bytes(), &[0x13]])
    );
    assert_eq!(s.host.saved_pads(), 1);
    assert_eq!(s.host.saved_tail(0), Some(tail(OLD_B)));
    assert_eq!(s.host.saved_tail(1), None);
    s.advance(1_200);
    assert!(s.host.waiting());
    s.cmds.clear();
    assert!(!s.pad_calls(PAD, PAD_COD), "forgotten: refused");
    no_key_change_ignored(&s);
    assert_private(&s);
}

/// The value grows little with the window (it lives on a small heap).
#[test]
fn the_host_stays_small_and_const() {
    static FRESH: PadHost = PadHost::new();
    assert_eq!(FRESH.pair_phase(), PairPhase::Off);
    assert_eq!(FRESH.pair_found(0), None);
    let size = core::mem::size_of::<PadHost>();
    println!("size_of::<PadHost>() with pair = {size}");
}
