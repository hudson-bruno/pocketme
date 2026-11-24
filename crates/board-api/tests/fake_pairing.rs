//! The fake pad's pairing radio: every case a page needs, played by
//! calls alone. A "turn" is one `pair_state` call (see `fake_radio.rs`).

use board_api::fake::{
    FAKE_NAME_A, FAKE_NAME_B, FAKE_SAVED_TAILS, FAKE_SEARCH_TURNS, FAKE_TAIL_A, FAKE_TAIL_B,
    FakePad, FakeRadio, PairScript,
};
use board_api::{
    Absent, FoundPad, PAD_NAME_MAX, PAIR_HOLD_MS, Pad, PairFail, PairReq, PairState, SavedPad,
};

fn pad(radio: FakeRadio) -> FakePad {
    FakePad {
        radio,
        ..FakePad::default()
    }
}

/// Calls `pair_state` `n` times and returns the last answer.
fn turns(p: &mut FakePad, n: u32) -> PairState {
    let mut s = p.pair_state();
    for _ in 1..n {
        s = p.pair_state();
    }
    s
}

/// Begin, served, then the search started (one turn in Starting).
fn begin(p: &mut FakePad) {
    p.pair_request(PairReq::Begin);
    assert_eq!(p.pair_state(), PairState::Starting);
    assert_eq!(p.pair_state(), PairState::Searching);
}

fn choose(p: &mut FakePad, i: u8) -> PairState {
    let tail = p.pair_found(i).expect("found").tail;
    p.pair_request(PairReq::Choose { i, tail });
    p.pair_state()
}

fn saved_count(p: &FakePad) -> usize {
    (0..8).filter(|&i| p.saved_pad(i).is_some()).count()
}

#[test]
fn the_defaults_and_the_default_fake_cannot_pair() {
    let mut a = Absent;
    let mut f = FakePad::default();
    assert!(!a.pairing() && !f.pairing());
    assert_eq!(a.pair_slots(), None);
    assert_eq!(f.pair_slots(), None);
    for p in [&mut a as &mut dyn Pad, &mut f] {
        for req in [PairReq::Begin, PairReq::Cancel, PairReq::Ack] {
            p.pair_request(req);
            assert_eq!(p.pair_state(), PairState::Off);
        }
        assert_eq!(p.saved_pad(0), None);
        assert_eq!(p.pair_found(0), None);
    }
    assert_eq!(f.radio.requests, 3, "counted");
    assert_eq!(f.radio.waiting(), None, "and dropped");
    assert_eq!(f.radio.last_request, Some(PairReq::Ack));
}

#[test]
fn a_pad_found_after_n_turns_is_saved_m_turns_after_yes() {
    let (n, m) = (5, 7);
    let mut p = pad(FakeRadio::new(PairScript::found_and_saved(n, m)).with_slots(10));
    assert!(p.pairing());
    assert_eq!(p.pair_state(), PairState::Off, "nothing before Begin");
    begin(&mut p);
    assert_eq!(turns(&mut p, u32::from(n) - 1), PairState::Searching);
    assert_eq!(p.pair_found(0), None, "not yet");
    assert_eq!(p.pair_state(), PairState::Searching);
    let found = p.pair_found(0).expect("found after n turns");
    assert_eq!(found.tail, FAKE_TAIL_A);
    assert_eq!(found.name(), FAKE_NAME_A);
    assert_eq!(p.pair_found(1), None);

    assert_eq!(choose(&mut p, 0), PairState::Calling);
    assert_eq!(turns(&mut p, u32::from(m) - 1), PairState::Calling);
    assert_eq!(p.pair_state(), PairState::Saved(0));
    assert_eq!(
        p.saved_pad(0),
        Some(SavedPad {
            tail: FAKE_TAIL_A,
            connected: true
        })
    );
    assert_eq!(p.pair_slots(), Some(9), "one record used");
    // The ending stays until Ack.
    assert_eq!(turns(&mut p, 500), PairState::Saved(0));
    p.pair_request(PairReq::Ack);
    assert_eq!(p.pair_state(), PairState::Off);
    assert_eq!(p.saved_pad(0).map(|s| s.tail), Some(FAKE_TAIL_A));
}

#[test]
fn two_pads_are_found_and_the_list_only_grows() {
    let mut p = pad(FakeRadio::new(PairScript::two_found(3, 10)));
    begin(&mut p);
    turns(&mut p, 3);
    assert_eq!(p.pair_found(0).map(|f| f.tail), Some(FAKE_TAIL_A));
    assert_eq!(p.pair_found(1), None);
    turns(&mut p, 7);
    assert_eq!(
        p.pair_found(0).map(|f| f.tail),
        Some(FAKE_TAIL_A),
        "same number, same pad"
    );
    let b = p.pair_found(1).expect("second pad");
    assert_eq!((b.tail, b.name()), (FAKE_TAIL_B, FAKE_NAME_B));
    turns(&mut p, 100);
    assert_eq!(p.radio.found_count(), 2);
    assert_eq!(choose(&mut p, 1), PairState::Calling);
    assert_eq!(turns(&mut p, 200), PairState::Saved(0));
    assert_eq!(p.saved_pad(0).map(|s| s.tail), Some(FAKE_TAIL_B));
    assert_eq!(
        p.pair_found(1).map(|f| f.tail),
        Some(FAKE_TAIL_B),
        "kept after the end"
    );
}

#[test]
fn nothing_found_ends_with_timeout_when_the_window_ends() {
    let mut p = pad(FakeRadio::new(PairScript::nothing_found(40)));
    begin(&mut p);
    assert_eq!(turns(&mut p, 39), PairState::Searching);
    assert_eq!(p.pair_state(), PairState::Failed(PairFail::Timeout));
    assert_eq!(turns(&mut p, 100), PairState::Failed(PairFail::Timeout));
    assert_eq!(p.pair_found(0), None);
    // The default script searches for the full window.
    let mut q = pad(FakeRadio::new(PairScript::new()));
    begin(&mut q);
    assert_eq!(
        turns(&mut q, u32::from(FAKE_SEARCH_TURNS) - 1),
        PairState::Searching
    );
    assert_eq!(q.pair_state(), PairState::Failed(PairFail::Timeout));
}

#[test]
fn a_found_pad_not_chosen_in_time_also_ends_with_timeout() {
    let mut p = pad(FakeRadio::new(
        PairScript::found_and_saved(2, 5).searching(20),
    ));
    begin(&mut p);
    assert_eq!(turns(&mut p, 20), PairState::Failed(PairFail::Timeout));
    // A late YES changes nothing.
    p.pair_request(PairReq::Choose {
        i: 0,
        tail: FAKE_TAIL_A,
    });
    assert_eq!(turns(&mut p, 10), PairState::Failed(PairFail::Timeout));
    assert_eq!(p.saved_pad(0), None);
}

#[test]
fn call_failures_save_nothing() {
    for (script, fail) in [
        (PairScript::old_pin(1, 4), PairFail::OldPin),
        (PairScript::link_lost(1, 4), PairFail::Lost),
        (PairScript::store_fails(1, 4), PairFail::Store),
        (
            PairScript::found_and_saved(1, 4).call_fails(PairFail::Refused),
            PairFail::Refused,
        ),
    ] {
        let mut p = pad(FakeRadio::new(script).with_slots(3));
        begin(&mut p);
        p.pair_state();
        assert_eq!(choose(&mut p, 0), PairState::Calling);
        assert_eq!(turns(&mut p, 3), PairState::Calling);
        assert_eq!(p.pair_state(), PairState::Failed(fail));
        assert_eq!(turns(&mut p, 50), PairState::Failed(fail), "stays");
        assert_eq!(saved_count(&p), 0);
        assert_eq!(p.pair_slots(), Some(3));
    }
}

#[test]
fn a_full_list_refuses_a_new_pad_but_pairs_a_saved_one_again() {
    let mut p = pad(FakeRadio::full(PairScript::found_and_saved(1, 2)));
    assert_eq!(saved_count(&p), 4);
    begin(&mut p);
    p.pair_state();
    assert_eq!(choose(&mut p, 0), PairState::Failed(PairFail::Full));
    assert_eq!(saved_count(&p), 4);
    assert!((0..4).all(|i| p.saved_pad(i).unwrap().tail == FAKE_SAVED_TAILS[usize::from(i)]));

    // The third saved pad is found again: its new key replaces the old.
    let again = PairScript::new()
        .find(1, FAKE_SAVED_TAILS[2], b"")
        .calling(2);
    let mut p = pad(FakeRadio::full(again));
    begin(&mut p);
    p.pair_state();
    assert_eq!(p.pair_found(0).unwrap().name(), b"", "no name given");
    assert_eq!(choose(&mut p, 0), PairState::Calling);
    assert_eq!(turns(&mut p, 2), PairState::Saved(2));
    assert_eq!(saved_count(&p), 4);
    assert!(p.saved_pad(2).unwrap().connected);
}

#[test]
fn no_room_refuses_begin() {
    let mut p = pad(FakeRadio::new(PairScript::found_and_saved(1, 1)).with_slots(0));
    assert_eq!(p.pair_slots(), Some(0));
    p.pair_request(PairReq::Begin);
    assert_eq!(p.pair_state(), PairState::Failed(PairFail::NoRoom));
    assert_eq!(turns(&mut p, 10), PairState::Failed(PairFail::NoRoom));
    // The last free slot is used by a save; the next Begin is refused.
    let mut p = pad(FakeRadio::new(PairScript::found_and_saved(1, 1)).with_slots(1));
    begin(&mut p);
    p.pair_state();
    choose(&mut p, 0);
    assert_eq!(p.pair_state(), PairState::Saved(0));
    assert_eq!(p.pair_slots(), Some(0));
    p.pair_request(PairReq::Begin);
    assert_eq!(p.pair_state(), PairState::Failed(PairFail::NoRoom));
}

#[test]
fn begin_and_forget_are_refused_when_boot_was_not_held() {
    let mut p = pad(FakeRadio::new(PairScript::not_held()).with_saved(1, FAKE_TAIL_B, false));
    p.pair_request(PairReq::Begin);
    assert_eq!(p.pair_state(), PairState::Failed(PairFail::NotHeld));
    p.pair_request(PairReq::Forget {
        place: 1,
        tail: FAKE_TAIL_B,
    });
    assert_eq!(p.pair_state(), PairState::Failed(PairFail::NotHeld));
    assert!(p.saved_pad(1).is_some(), "still saved");
    // Also with a wrong tail: the pin is checked first.
    p.pair_request(PairReq::Forget {
        place: 1,
        tail: FAKE_TAIL_A,
    });
    assert_eq!(p.pair_state(), PairState::Failed(PairFail::NotHeld));
}

#[test]
fn a_linked_pad_is_hung_up_when_the_search_starts() {
    let mut p = pad(FakeRadio::new(PairScript::found_and_saved(1, 1))
        .with_saved(0, FAKE_SAVED_TAILS[0], true)
        .with_saved(3, FAKE_SAVED_TAILS[3], false));
    assert!(p.saved_pad(0).unwrap().connected);
    begin(&mut p);
    assert!(!p.saved_pad(0).unwrap().connected, "hung up");
    p.pair_state();
    choose(&mut p, 0);
    assert_eq!(p.pair_state(), PairState::Saved(1), "first empty place");
    assert!(p.saved_pad(1).unwrap().connected);
    assert!(!p.saved_pad(0).unwrap().connected && !p.saved_pad(3).unwrap().connected);

    // A board that refuses instead of hanging up.
    let mut q = pad(
        FakeRadio::new(PairScript::new().begin_fails(PairFail::Linked)).with_saved(
            0,
            FAKE_SAVED_TAILS[0],
            true,
        ),
    );
    q.pair_request(PairReq::Begin);
    assert_eq!(q.pair_state(), PairState::Failed(PairFail::Linked));
    assert!(q.saved_pad(0).unwrap().connected, "left linked");
}

#[test]
fn other_begin_refusals_come_from_the_script() {
    for fail in [PairFail::Busy, PairFail::Damaged] {
        let mut p = pad(FakeRadio::new(PairScript::new().begin_fails(fail)));
        p.pair_request(PairReq::Begin);
        assert_eq!(p.pair_state(), PairState::Failed(fail));
    }
}

#[test]
fn forget_works_fails_and_refuses_a_wrong_tail() {
    let radio = FakeRadio::full(PairScript::new())
        .with_saved(2, FAKE_SAVED_TAILS[2], true)
        .with_slots(5);
    let mut p = pad(radio);
    // Wrong tail: refused, nothing changed.
    p.pair_request(PairReq::Forget {
        place: 2,
        tail: FAKE_SAVED_TAILS[1],
    });
    assert_eq!(p.pair_state(), PairState::Failed(PairFail::Busy));
    // An empty or missing place: refused too.
    p.pair_request(PairReq::Forget {
        place: 7,
        tail: FAKE_SAVED_TAILS[1],
    });
    assert_eq!(p.pair_state(), PairState::Failed(PairFail::Busy));
    assert_eq!(saved_count(&p), 4);
    // Right tail: forgotten (and hung up), the place is empty.
    p.pair_request(PairReq::Forget {
        place: 2,
        tail: FAKE_SAVED_TAILS[2],
    });
    assert_eq!(p.pair_state(), PairState::Forgotten(2));
    assert_eq!(p.saved_pad(2), None);
    assert_eq!(saved_count(&p), 3);
    assert_eq!(p.pair_slots(), Some(4));
    assert_eq!(turns(&mut p, 30), PairState::Forgotten(2), "stays");
    p.pair_request(PairReq::Ack);
    assert_eq!(p.pair_state(), PairState::Off);

    // The flash write fails: the pad stays saved.
    let mut p = pad(FakeRadio::full(
        PairScript::new().forget_fails(PairFail::Store),
    ));
    p.pair_request(PairReq::Forget {
        place: 0,
        tail: FAKE_SAVED_TAILS[0],
    });
    assert_eq!(p.pair_state(), PairState::Failed(PairFail::Store));
    assert_eq!(saved_count(&p), 4);
}

#[test]
fn forget_is_ignored_while_a_window_is_open() {
    let mut p = pad(FakeRadio::full(PairScript::found_and_saved(1, 1)));
    begin(&mut p);
    p.pair_request(PairReq::Forget {
        place: 0,
        tail: FAKE_SAVED_TAILS[0],
    });
    assert_eq!(p.pair_state(), PairState::Searching);
    assert_eq!(saved_count(&p), 4);
}

#[test]
fn choose_is_refused_with_a_wrong_tail_or_number_and_ignored_out_of_search() {
    let mut p = pad(FakeRadio::new(PairScript::two_found(1, 1)));
    p.pair_request(PairReq::Begin);
    assert_eq!(p.pair_state(), PairState::Starting);
    // Not searching yet: ignored.
    p.pair_request(PairReq::Choose {
        i: 0,
        tail: FAKE_TAIL_A,
    });
    assert_eq!(p.pair_state(), PairState::Starting);
    assert_eq!(p.pair_state(), PairState::Searching);
    p.pair_state();
    assert_eq!(p.radio.found_count(), 2);
    // Number 0 shown with pad B's tail: the list changed under the press.
    p.pair_request(PairReq::Choose {
        i: 0,
        tail: FAKE_TAIL_B,
    });
    assert_eq!(p.pair_state(), PairState::Failed(PairFail::Busy));
    assert_eq!(saved_count(&p), 0);

    let mut p = pad(FakeRadio::new(PairScript::found_and_saved(1, 3)));
    begin(&mut p);
    p.pair_state();
    p.pair_request(PairReq::Choose {
        i: 1,
        tail: FAKE_TAIL_A,
    });
    assert_eq!(
        p.pair_state(),
        PairState::Failed(PairFail::Busy),
        "no pad 1"
    );

    // A second YES while calling changes nothing.
    let mut p = pad(FakeRadio::new(PairScript::found_and_saved(1, 3)));
    begin(&mut p);
    p.pair_state();
    assert_eq!(choose(&mut p, 0), PairState::Calling);
    assert_eq!(choose(&mut p, 0), PairState::Calling);
    assert_eq!(turns(&mut p, 3), PairState::Saved(0));
}

#[test]
fn cancel_beats_everything() {
    let mut p = pad(FakeRadio::new(PairScript::found_and_saved(1, 50)));
    // A Cancel in the box is never replaced.
    p.pair_request(PairReq::Cancel);
    p.pair_request(PairReq::Begin);
    assert_eq!(p.radio.waiting(), Some(PairReq::Cancel));
    assert_eq!(p.pair_state(), PairState::Off);
    // The newest wins otherwise.
    p.pair_request(PairReq::Ack);
    p.pair_request(PairReq::Begin);
    assert_eq!(p.radio.waiting(), Some(PairReq::Begin));
    p.pair_request(PairReq::Cancel);
    p.pair_request(PairReq::Ack);
    assert_eq!(p.radio.waiting(), Some(PairReq::Cancel));
    assert_eq!(p.pair_state(), PairState::Off);

    // Cancel stops a search and a call; nothing is saved.
    begin(&mut p);
    p.pair_request(PairReq::Cancel);
    assert_eq!(p.pair_state(), PairState::Off);
    begin(&mut p);
    p.pair_state();
    assert_eq!(choose(&mut p, 0), PairState::Calling);
    p.pair_request(PairReq::Cancel);
    assert_eq!(p.pair_state(), PairState::Off);
    assert_eq!(turns(&mut p, 100), PairState::Off);
    assert_eq!(saved_count(&p), 0);
    // And clears an ending.
    let mut q = pad(FakeRadio::new(PairScript::not_held()));
    q.pair_request(PairReq::Begin);
    assert_eq!(q.pair_state(), PairState::Failed(PairFail::NotHeld));
    q.pair_request(PairReq::Cancel);
    assert_eq!(q.pair_state(), PairState::Off);
}

#[test]
fn endings_stay_until_ack_or_the_next_begin() {
    let mut p = pad(FakeRadio::new(PairScript::two_found(1, 1).searching(5)));
    begin(&mut p);
    assert_eq!(turns(&mut p, 5), PairState::Failed(PairFail::Timeout));
    assert_eq!(p.radio.found_count(), 2);
    // Begin while a window is open is ignored; an ending is no window.
    p.pair_request(PairReq::Begin);
    assert_eq!(p.pair_state(), PairState::Starting);
    assert_eq!(p.pair_found(0), None, "a new search starts empty");
    p.pair_request(PairReq::Begin);
    assert_eq!(p.pair_state(), PairState::Starting, "ignored while open");
    assert_eq!(p.pair_state(), PairState::Searching);
    // Ack outside an ending is ignored.
    p.pair_request(PairReq::Ack);
    assert_eq!(p.pair_state(), PairState::Searching);
}

#[test]
fn manual_turns_and_a_board_that_starts_at_once() {
    let mut radio = FakeRadio::new(PairScript::found_and_saved(2, 2).starting(0));
    radio.manual = true;
    let mut p = pad(radio);
    p.pair_request(PairReq::Begin);
    assert_eq!(
        p.pair_state(),
        PairState::Off,
        "manual: reading moves nothing"
    );
    assert_eq!(p.pair_state(), PairState::Off);
    p.radio.turn();
    assert_eq!(p.pair_state(), PairState::Searching, "start 0: at once");
    p.radio.turn();
    assert_eq!(p.pair_found(0), None);
    p.radio.turn();
    assert!(p.pair_found(0).is_some());
    assert_eq!(p.radio.turns, 3);

    let mut slow = pad(FakeRadio::new(PairScript::new().starting(4)));
    slow.pair_request(PairReq::Begin);
    assert_eq!(turns(&mut slow, 4), PairState::Starting);
    assert_eq!(slow.pair_state(), PairState::Searching);
}

#[test]
fn the_same_calls_give_the_same_states() {
    fn trace() -> Vec<PairState> {
        let mut p = pad(FakeRadio::full(PairScript::two_found(4, 9).calling(10)).with_slots(20));
        let mut out = Vec::new();
        for step in 0..120u32 {
            match step {
                3 => p.pair_request(PairReq::Begin),
                20 => p.pair_request(PairReq::Choose {
                    i: 0,
                    tail: FAKE_TAIL_A,
                }),
                30 => p.pair_request(PairReq::Ack),
                40 => p.pair_request(PairReq::Forget {
                    place: 1,
                    tail: FAKE_SAVED_TAILS[1],
                }),
                45 => p.pair_request(PairReq::Ack),
                50 => p.pair_request(PairReq::Begin),
                65 => p.pair_request(PairReq::Choose {
                    i: 1,
                    tail: FAKE_TAIL_B,
                }),
                _ => {}
            }
            out.push(p.pair_state());
        }
        out
    }
    let a = trace();
    assert_eq!(a, trace());
    assert!(a.contains(&PairState::Failed(PairFail::Full)));
    assert!(a.contains(&PairState::Forgotten(1)));
    assert_eq!(a.last(), Some(&PairState::Saved(1)), "into the freed place");
}

#[test]
fn found_names_are_cut_and_never_printed() {
    let long = b"A MADE UP NAME LONGER THAN THE LIMIT";
    let f = FoundPad::new(FAKE_TAIL_A, long);
    assert_eq!(usize::from(f.name_len), PAD_NAME_MAX);
    assert_eq!(f.name(), &long[..PAD_NAME_MAX]);
    let shown = format!("{f:?}");
    assert!(!shown.contains("MADE"), "{shown}");
    assert!(shown.contains("name_len"));
    let mut p = pad(FakeRadio::new(PairScript::found_and_saved(1, 1)));
    begin(&mut p);
    p.pair_state();
    let shown = format!("{:?} {:?}", p.radio, p.radio.script);
    assert!(!shown.contains("TEST PAD"), "{shown}");
    // A name_len past the array never reads past it.
    let bad = FoundPad { name_len: 200, ..f };
    assert_eq!(bad.name().len(), PAD_NAME_MAX);
}

#[test]
fn small_facts() {
    assert_eq!(PAIR_HOLD_MS, 2000);
    assert_eq!(PairState::default(), PairState::Off);
    assert!(PairState::Calling.is_open() && !PairState::Calling.is_end());
    assert!(PairState::Failed(PairFail::Busy).is_end());
    assert!(!PairState::Off.is_open() && !PairState::Off.is_end());
    assert_eq!(core::mem::size_of::<PairState>(), 2);
}
