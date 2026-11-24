//! The settings app (feature `settings`): placement E (GAMEPADS first,
//! SETTINGS last, plus quick-menu shortcuts), the way back leaves it,
//! the brightness on its SETTINGS page is the console's own, and
//! pairing and forgetting run through the board's pad as the pairing
//! rules say, played by board-api's scripted fake radio. The fake
//! board, the fake clock, no window.
//!
//! Positions are picture pixels (the test board doubles them for the
//! panel). The fake radio moves one turn each time the console reads the
//! pairing state, which it does once per step on the GAMEPADS pages.

#![cfg(feature = "settings")]

mod common;

use common::{DOWN, EAST, Ev, FOUR, HOME, LEFT, RIGHT, SOUTH, Sim, UP};
use console_core::Page;
use console_core::board_api::fake::{
    FAKE_NAME_A, FAKE_SAVED_TAILS, FAKE_TAIL_A, FAKE_TAIL_B, FakeRadio, PairScript,
};
use console_core::board_api::{PairFail, PairReq, PairState, SavedPad};
use console_core::home::Slot;
use console_core::pause::{self, Kind, item_y};
use console_core::settings::{
    ABOUT_LINE, AppPage, BACK_BOX, BACK_HIT, Entry, PLACES, bright_bar_x, button_box, found_row_y,
    main_row_y, pads_row_y,
};
use console_core::theme::{GEAR_X, GEAR_Y, TILE, TILE_GAP, TILE_LEFT, TILE_Y};

/// The middle of the BACK button.
const BACK: (u16, u16) = (
    (BACK_BOX.0 + BACK_BOX.2 as i16 / 2) as u16,
    (BACK_BOX.1 + BACK_BOX.3 as i16 / 2) as u16,
);
/// The middle of today's small gear in the bar.
const GEAR: (u16, u16) = ((GEAR_X + 4) as u16, (GEAR_Y + 4) as u16);

/// Turns into the search when pad A is found, and turns its call takes.
const FIND: u16 = 30;
const CALL: u16 = 30;

/// A board that can pair, with no saved pads yet; the search finds
/// pad A, which then pairs.
fn empty_radio() -> FakeRadio {
    FakeRadio::new(PairScript::found_and_saved(FIND, CALL))
}

/// A board that can pair, one pad saved (and connected) at place 0.
fn one_saved() -> FakeRadio {
    empty_radio().with_saved(0, FAKE_SAVED_TAILS[0], true)
}

fn home(entry: Entry, radio: FakeRadio) -> Sim {
    let mut sim = Sim::new(Some(FOUR));
    sim.board.pad.radio = radio;
    sim.console.set_settings_entry(entry);
    sim.boot();
    sim.idle(2);
    sim
}

fn home_no_card(entry: Entry, radio: FakeRadio) -> Sim {
    let mut sim = Sim::new(None);
    sim.board.pad.radio = radio;
    sim.console.set_settings_entry(entry);
    sim.boot();
    sim.idle(2);
    sim
}

fn open(sim: &Sim) -> bool {
    sim.console.settings().is_open()
}

fn page(sim: &Sim) -> AppPage {
    sim.console.settings().page()
}

fn sel(sim: &Sim) -> u8 {
    sim.console.settings().sel()
}

fn fake(sim: &Sim) -> &FakeRadio {
    &sim.board.pad.radio
}

/// Runs ticks until the app shows `want` (at most `max` ticks).
fn wait_for(sim: &mut Sim, want: AppPage, max: u64) {
    for _ in 0..max {
        if page(sim) == want {
            return;
        }
        sim.step_tick();
    }
    assert_eq!(page(sim), want, "after {max} ticks");
}

/// Opens an item from the home quick menu (placement E), with the pad.
fn open_by_menu(sim: &mut Sim, item: pause::Item) {
    sim.tap_button(HOME);
    assert!(sim.console.menu_open());
    let i = Kind::HomeSettings.index_of(item).unwrap();
    for _ in 0..i {
        sim.tap_button(DOWN);
    }
    sim.tap_button(SOUTH);
    assert!(open(sim));
}

/// Opens the SETTINGS page via the home menu.
fn open_settings_by_menu(sim: &mut Sim) {
    open_by_menu(sim, pause::Item::Settings);
    assert_eq!(page(sim), AppPage::Main);
}

/// Opens GAMEPADS directly (from the GAMEPADS tile at slot 0).
fn open_gamepads_by_tile(sim: &mut Sim) {
    assert_eq!(sim.console.home().on_system(), Some(0));
    sim.tap_button(SOUTH);
    assert!(open(sim));
    assert_eq!(page(sim), AppPage::Pads);
}

/// From home (empty card or 0 saved) to `+ PAIR PLAYER 1` on GAMEPADS.
fn to_pair(sim: &mut Sim) {
    open_gamepads_by_tile(sim);
    assert_eq!(sel(sim), PLACES, "+ PAIR PLAYER 1");
}

/// From home to SEARCHING with pad A in the list (chosen), by pad.
fn to_found_list(sim: &mut Sim) {
    to_pair(sim);
    sim.tap_button(SOUTH);
    assert_eq!(page(sim), AppPage::Searching);
    sim.idle(u64::from(FIND) + 5);
    assert_eq!(fake(sim).found_count(), 1);
    assert_eq!(page(sim), AppPage::Searching);
}

/// From SEARCHING with pad A chosen: YES, then CALLING.
fn call_it(sim: &mut Sim) {
    sim.tap_button(SOUTH);
    assert_eq!(page(sim), AppPage::Found);
    sim.tap_button(RIGHT);
    sim.tap_button(SOUTH);
    assert_eq!(page(sim), AppPage::Calling);
}

/// Pairs from home (0 saved) with `radio` until an ending shows; YES when
/// a pad is found.
fn pair_to_end(radio: FakeRadio) -> Sim {
    let mut sim = home(Entry::Tiles, radio);
    to_pair(&mut sim);
    sim.tap_button(SOUTH);
    for _ in 0..200 {
        match page(&sim) {
            AppPage::End => break,
            AppPage::Searching if sim.board.pad.radio.found_count() > 0 => call_it(&mut sim),
            _ => {
                sim.step_tick();
            }
        }
    }
    assert_eq!(page(&sim), AppPage::End);
    sim
}

fn fail_of(sim: &Sim) -> PairFail {
    match sim.console.settings().state() {
        PairState::Failed(why) => why,
        other => panic!("not a failure: {other:?}"),
    }
}

// ---- Placement E: home tiles and quick menu --------------------------------

#[test]
fn e_puts_gamepads_first_settings_last_and_wraps_with_games() {
    let mut sim = home(Entry::Tiles, one_saved());
    let h = sim.console.home();
    assert_eq!((h.count(), h.slots(), h.lead()), (4, 6, 1));
    assert_eq!(h.slot(0), Slot::System(0));
    assert_eq!(h.slot(1), Slot::Game(0));
    assert_eq!(h.slot(4), Slot::Game(3));
    assert_eq!(h.slot(5), Slot::System(1));
    assert_eq!(h.selected(), 0);
    assert_eq!(h.on_system(), Some(0));

    // LEFT from GAMEPADS wraps to SETTINGS; RIGHT from SETTINGS wraps to GAMEPADS.
    sim.tap_button(LEFT);
    assert_eq!(sim.console.home().selected(), 5);
    assert_eq!(sim.console.home().on_system(), Some(1));

    sim.tap_button(RIGHT);
    assert_eq!(sim.console.home().selected(), 0);
    assert_eq!(sim.console.home().on_system(), Some(0));

    // Stepping RIGHT across all games reaches SETTINGS at slot 5.
    for g in 0..4 {
        sim.tap_button(RIGHT);
        assert_eq!(sim.console.home().selected(), g + 1);
        assert_eq!(sim.console.home().on_system(), None);
        assert_eq!(sim.console.home().slot(g + 1), Slot::Game(g));
    }
    sim.tap_button(RIGHT);
    assert_eq!(sim.console.home().selected(), 5);
    assert_eq!(sim.console.home().on_system(), Some(1));
}

#[test]
fn e_skips_empty_slot_when_there_is_no_card() {
    let mut sim = home_no_card(Entry::Tiles, empty_radio());
    let h = sim.console.home();
    assert_eq!((h.count(), h.slots(), h.lead()), (0, 3, 1));
    assert_eq!(h.slot(0), Slot::System(0));
    assert_eq!(h.slot(1), Slot::Empty);
    assert_eq!(h.slot(2), Slot::System(1));
    assert_eq!(h.selected(), 0);

    sim.tap_button(RIGHT);
    assert_eq!(sim.console.home().selected(), 2);
    assert_eq!(sim.console.home().on_system(), Some(1));

    sim.tap_button(RIGHT);
    assert_eq!(sim.console.home().selected(), 0);
    assert_eq!(sim.console.home().on_system(), Some(0));

    sim.tap_button(LEFT);
    assert_eq!(sim.console.home().selected(), 2);
    assert_eq!(sim.console.home().on_system(), Some(1));
}

#[test]
fn e_tiles_open_gamepads_and_settings_by_pad_and_touch() {
    let mut sim = home(Entry::Tiles, one_saved());
    sim.idle(20);
    // Slot 0: GAMEPADS opens directly; BACK/B returns to Home.
    sim.tap_button(SOUTH);
    assert!(open(&sim));
    assert_eq!(page(&sim), AppPage::Pads);
    assert_eq!(sim.console.page(), Page::Home);
    sim.tap_button(EAST);
    assert!(!open(&sim));
    assert_eq!(sim.console.home().on_system(), Some(0));

    // Touch on chosen GAMEPADS tile opens GAMEPADS.
    let scroll = sim.console.home().scroll();
    let x0 = TILE_LEFT - scroll + TILE as i16 / 2;
    sim.tap_at(x0 as u16, (TILE_Y + 10) as u16);
    assert!(open(&sim));
    assert_eq!(page(&sim), AppPage::Pads);
    sim.tap_at(BACK.0, BACK.1);
    assert!(!open(&sim));

    // Wrap LEFT onto SETTINGS (last slot), settle, open by A, BOOT, and touch.
    sim.tap_button(LEFT);
    assert_eq!(sim.console.home().on_system(), Some(1));
    sim.idle(20);
    sim.tap_button(SOUTH);
    assert!(open(&sim));
    assert_eq!(page(&sim), AppPage::Main);
    sim.tap_button(EAST);
    assert!(!open(&sim));

    sim.input(Ev::QuickBoot);
    sim.idle(2);
    assert!(open(&sim));
    assert_eq!(page(&sim), AppPage::Main);
    sim.tap_at(BACK.0, BACK.1);
    assert!(!open(&sim));

    let scroll = sim.console.home().scroll();
    let x5 = TILE_LEFT + 5 * (TILE + TILE_GAP) as i16 - scroll + TILE as i16 / 2;
    sim.tap_at(x5 as u16, (TILE_Y + 10) as u16);
    assert!(open(&sim));
    assert_eq!(page(&sim), AppPage::Main);
}

#[test]
fn e_quick_menu_has_shortcuts_and_no_tiles_under_off() {
    // Under Entry::Off, HOME shows plain Kind::Home without GAMEPADS.
    let mut plain = home(Entry::Off, one_saved());
    assert_eq!(plain.console.home().slots(), 4);
    plain.tap_button(HOME);
    assert!(plain.console.menu_open());
    assert_eq!(Kind::Home.index_of(pause::Item::Gamepads), None);
    assert_eq!(Kind::Home.index_of(pause::Item::Settings), None);

    // Under Entry::Tiles, HomeSettings has 4 items: Bright, Gamepads, Settings, Close.
    assert_eq!(
        Kind::HomeSettings.items(),
        &[
            pause::Item::Bright,
            pause::Item::Gamepads,
            pause::Item::Settings,
            pause::Item::Close,
        ]
    );
    let mut sim = home(Entry::Tiles, one_saved());
    open_by_menu(&mut sim, pause::Item::Gamepads);
    assert_eq!(page(&sim), AppPage::Pads);
    assert!(sim.console.menu_open());
    sim.tap_button(EAST);
    assert!(!open(&sim));
    assert!(!sim.console.menu_open());

    // Touch via small gear -> GAMEPADS row.
    sim.tap_at(GEAR.0, GEAR.1);
    assert!(sim.console.menu_open());
    let gi = Kind::HomeSettings.index_of(pause::Item::Gamepads).unwrap();
    sim.tap_at(80, (item_y(Kind::HomeSettings, gi) + 4) as u16);
    assert!(open(&sim));
    assert_eq!(page(&sim), AppPage::Pads);
    sim.tap_at(BACK.0, BACK.1);
    assert!(!open(&sim));

    // Touch via small gear -> SETTINGS row.
    sim.tap_at(GEAR.0, GEAR.1);
    assert!(sim.console.menu_open());
    let si = Kind::HomeSettings.index_of(pause::Item::Settings).unwrap();
    sim.tap_at(80, (item_y(Kind::HomeSettings, si) + 4) as u16);
    assert!(open(&sim));
    assert_eq!(page(&sim), AppPage::Main);
}

#[test]
fn the_brightness_line_changes_the_real_brightness() {
    let mut sim = home(Entry::Tiles, one_saved());
    assert_eq!(sim.console.brightness(), 4);
    let _ = sim.console.take_settings_changed();
    open_settings_by_menu(&mut sim);
    sim.tap_button(LEFT);
    sim.tap_button(LEFT);
    assert_eq!(sim.console.brightness(), 2);
    assert!(sim.console.take_settings_changed());
    sim.tap_button(RIGHT);
    assert_eq!(sim.console.brightness(), 3);
    // A tap on the first bar: the dimmest step.
    let y = (main_row_y(0) + 8) as u16;
    sim.tap_at((bright_bar_x(0) + 4) as u16, y);
    assert_eq!(sim.console.brightness(), 0);
    sim.idle(2);
    assert_eq!(sim.console.backlight(), console_core::theme::BACKLIGHT[0]);
    // Still set after the app closes.
    sim.tap_button(HOME);
    assert!(!open(&sim));
    assert_eq!(sim.console.brightness(), 0);
}

// ---- Pairing and forgetting (Player 1 MVP, auto-search) --------------------

#[test]
fn opening_gamepads_with_connected_pad_keeps_link_and_blocks_pairing() {
    let mut sim = home(Entry::Tiles, one_saved());
    open_gamepads_by_tile(&mut sim);
    sim.idle(10);
    // No Begin sent; connected pad remains connected; PAIR row is disabled.
    assert_eq!(sel(&sim), 0);
    assert_eq!(fake(&sim).requests, 0);
    assert_eq!(
        fake(&sim).saved_pad(0),
        Some(SavedPad {
            tail: FAKE_SAVED_TAILS[0],
            connected: true,
        })
    );
    assert!(!sim.console.pairing_page());
    sim.tap_button(DOWN);
    assert_eq!(sel(&sim), 0);
    sim.tap_at(80, (pads_row_y(1) + 6) as u16);
    assert_eq!(page(&sim), AppPage::Pads);
    assert_eq!(fake(&sim).requests, 0);
}

#[test]
fn pair_player_1_starts_searching_at_once_then_calls_and_saves() {
    let mut sim = home(Entry::Tiles, empty_radio());
    to_pair(&mut sim);
    // No BOOT hold: SOUTH sends Begin and opens SEARCHING at once.
    sim.tap_button(SOUTH);
    assert_eq!(page(&sim), AppPage::Searching);
    assert_eq!(fake(&sim).last_request, Some(PairReq::Begin));
    assert_eq!(fake(&sim).requests, 1);
    assert!(sim.console.pairing_page());
    sim.idle(5);
    assert_eq!(fake(&sim).state(), PairState::Searching);
    // Pad A shows up in the list; the selection is on it.
    sim.idle(u64::from(FIND) + 5);
    assert_eq!(page(&sim), AppPage::Searching);
    assert_eq!(sel(&sim), 0);
    // Chosen: CALL IT AND PAIR? with NO chosen first; nothing is sent.
    sim.tap_button(SOUTH);
    assert_eq!(page(&sim), AppPage::Found);
    assert_eq!(sel(&sim), 0, "NO first");
    assert_eq!(fake(&sim).requests, 1);
    // YES sends Choose with the tail the player saw.
    sim.tap_button(RIGHT);
    sim.tap_button(SOUTH);
    assert_eq!(page(&sim), AppPage::Calling);
    assert_eq!(
        fake(&sim).last_request,
        Some(PairReq::Choose {
            i: 0,
            tail: FAKE_TAIL_A
        })
    );
    wait_for(&mut sim, AppPage::End, u64::from(CALL) + 20);
    assert_eq!(sim.console.settings().state(), PairState::Saved(0));
    assert_eq!(
        fake(&sim).saved_pad(0),
        Some(SavedPad {
            tail: FAKE_TAIL_A,
            connected: true
        })
    );
    assert!(sim.console.pairing_page());
    // OK: back on the list, on PLAYER 1; the ending is acknowledged.
    sim.tap_button(SOUTH);
    assert_eq!(page(&sim), AppPage::Pads);
    assert_eq!(sel(&sim), 0);
    assert_eq!(fake(&sim).last_request, Some(PairReq::Ack));
    assert!(!sim.console.pairing_page());
    sim.tap_button(UP);
    assert_eq!(fake(&sim).state(), PairState::Off);
}

#[test]
fn no_on_the_found_page_goes_back_to_the_open_search() {
    let mut sim = home(Entry::Tiles, empty_radio());
    to_found_list(&mut sim);
    sim.tap_button(SOUTH);
    assert_eq!(page(&sim), AppPage::Found);
    let before = fake(&sim).requests;
    // NO: the list again, the search still open.
    sim.tap_button(SOUTH);
    assert_eq!(page(&sim), AppPage::Searching);
    assert_eq!(fake(&sim).requests, before);
    assert_eq!(fake(&sim).state(), PairState::Searching);
    // B on the found page does the same.
    sim.tap_button(SOUTH);
    sim.tap_button(EAST);
    assert_eq!(page(&sim), AppPage::Searching);
    assert_eq!(fake(&sim).requests, before);
}

#[test]
fn the_list_shows_every_pad_found_and_the_choice_names_its_tail() {
    let mut sim = home(Entry::Tiles, FakeRadio::new(PairScript::two_found(10, 20)));
    to_pair(&mut sim);
    sim.tap_button(SOUTH);
    sim.idle(30);
    assert_eq!(fake(&sim).found_count(), 2);
    // DOWN: pad B; DOWN again: STOP; UP: pad B again.
    sim.tap_button(DOWN);
    assert_eq!(sel(&sim), 1);
    sim.tap_button(DOWN);
    assert!(sel(&sim) >= 2, "STOP");
    sim.tap_button(UP);
    assert_eq!(sel(&sim), 1);
    // A tap on pad B's row chooses it.
    sim.tap_at(80, (found_row_y(1) + 5) as u16);
    assert_eq!(page(&sim), AppPage::Found);
    sim.tap_button(RIGHT);
    sim.tap_button(SOUTH);
    assert_eq!(
        fake(&sim).last_request,
        Some(PairReq::Choose {
            i: 1,
            tail: FAKE_TAIL_B
        })
    );
    wait_for(&mut sim, AppPage::End, 200);
    assert_eq!(sim.console.settings().state(), PairState::Saved(0));
}

#[test]
fn try_again_from_end_starts_searching_at_once() {
    let mut sim = pair_to_end(FakeRadio::new(PairScript::nothing_found(30)));
    assert_eq!(fail_of(&sim), PairFail::Timeout);
    assert_eq!(sim.console.settings().buttons(), &["TRY AGAIN", "BACK"]);
    let before = fake(&sim).requests;
    sim.tap_button(SOUTH);
    // TRY AGAIN must send Begin (and leave_request must NOT overwrite it with Ack).
    assert_eq!(page(&sim), AppPage::Searching);
    assert_eq!(fake(&sim).requests, before + 1);
    assert_eq!(fake(&sim).last_request, Some(PairReq::Begin));
    assert_eq!(fake(&sim).state(), PairState::Starting);
    sim.idle(2);
    assert_eq!(fake(&sim).state(), PairState::Searching);
}

#[test]
fn every_failure_of_the_board_has_its_ending() {
    let s = PairScript::found_and_saved(5, 5);
    let cases = [
        (
            FakeRadio::new(s.call_fails(PairFail::OldPin)),
            PairFail::OldPin,
            false,
        ),
        (
            FakeRadio::new(s.call_fails(PairFail::Lost)),
            PairFail::Lost,
            true,
        ),
        (
            FakeRadio::new(s.call_fails(PairFail::Store)),
            PairFail::Store,
            false,
        ),
        (
            FakeRadio::new(s.call_fails(PairFail::Refused)),
            PairFail::Refused,
            false,
        ),
        (
            FakeRadio::new(s.call_fails(PairFail::Timeout)),
            PairFail::Timeout,
            true,
        ),
        (
            FakeRadio::new(s.begin_fails(PairFail::Linked)),
            PairFail::Linked,
            false,
        ),
        (
            FakeRadio::new(s.begin_fails(PairFail::Full)),
            PairFail::Full,
            false,
        ),
        (
            FakeRadio::new(s.begin_fails(PairFail::Damaged)),
            PairFail::Damaged,
            false,
        ),
        (
            FakeRadio::new(s.begin_fails(PairFail::Busy)),
            PairFail::Busy,
            false,
        ),
        (
            FakeRadio::new(s.begin_fails(PairFail::NotHeld)),
            PairFail::NotHeld,
            true,
        ),
        (
            FakeRadio::new(PairScript::nothing_found(60)),
            PairFail::Timeout,
            true,
        ),
    ];
    for (radio, why, again) in cases {
        let mut sim = pair_to_end(radio);
        assert_eq!(fail_of(&sim), why);
        let words = sim.console.settings().buttons();
        assert_eq!(words.len(), if again { 2 } else { 1 }, "{why:?}: {words:?}");
        // Nothing was saved.
        assert_eq!(
            sim.board.pad.radio.saved, [None; PLACES as usize],
            "{why:?}"
        );
        // OK (or BACK): the list, on PAIR PLAYER 1; the ending acknowledged.
        if again {
            sim.tap_button(RIGHT);
        }
        sim.tap_button(SOUTH);
        assert_eq!(page(&sim), AppPage::Pads, "{why:?}");
        assert_eq!(sel(&sim), PLACES, "{why:?}");
        assert_eq!(
            sim.board.pad.radio.last_request,
            Some(PairReq::Ack),
            "{why:?}"
        );
        sim.tap_button(UP);
        assert_eq!(sim.board.pad.radio.state(), PairState::Off, "{why:?}");
    }
}

#[test]
fn no_room_left_is_refused_before_anything_is_sent() {
    let radio = empty_radio().with_slots(0);
    let mut sim = home(Entry::Tiles, radio);
    to_pair(&mut sim);
    sim.tap_button(SOUTH);
    assert_eq!(page(&sim), AppPage::End);
    assert_eq!(fail_of(&sim), PairFail::NoRoom);
    assert_eq!(sim.console.settings().buttons(), &["OK"]);
    assert_eq!(fake(&sim).requests, 0);
    sim.tap_button(SOUTH);
    assert_eq!(page(&sim), AppPage::Pads);
    // The ending was the console's own; its Ack changes nothing.
    assert_eq!(fake(&sim).state(), PairState::Off);
}

#[test]
fn leaving_an_open_search_cancels_it() {
    // B on the list.
    let mut sim = home(Entry::Tiles, empty_radio());
    to_found_list(&mut sim);
    sim.tap_button(EAST);
    assert_eq!(page(&sim), AppPage::Pads);
    assert_eq!(fake(&sim).last_request, Some(PairReq::Cancel));
    sim.tap_button(UP);
    assert_eq!(fake(&sim).state(), PairState::Off);

    // STOP (the selection moved past the pads).
    let mut sim = home(Entry::Tiles, empty_radio());
    to_found_list(&mut sim);
    sim.tap_button(DOWN);
    sim.tap_button(SOUTH);
    assert_eq!(page(&sim), AppPage::Pads);
    assert_eq!(fake(&sim).last_request, Some(PairReq::Cancel));

    // BACK tap on SEARCHING.
    let mut sim = home(Entry::Tiles, empty_radio());
    to_found_list(&mut sim);
    sim.tap_at(BACK.0, BACK.1);
    assert_eq!(page(&sim), AppPage::Pads);
    assert_eq!(fake(&sim).last_request, Some(PairReq::Cancel));

    // Home while the pad is called: the app closes, the call stops.
    let mut sim = home(Entry::Tiles, empty_radio());
    to_found_list(&mut sim);
    call_it(&mut sim);
    sim.idle(3);
    assert_eq!(fake(&sim).state(), PairState::Calling);
    sim.tap_button(HOME);
    assert!(!open(&sim));
    assert!(!sim.console.pairing_page());
    assert_eq!(fake(&sim).last_request, Some(PairReq::Cancel));
    assert_eq!(fake(&sim).waiting(), Some(PairReq::Cancel));
    assert_eq!(fake(&sim).saved_pad(0), None);
}

#[test]
fn a_search_stopped_by_the_board_goes_back_to_the_list() {
    let mut sim = home(Entry::Tiles, empty_radio());
    to_found_list(&mut sim);
    // The board closes the window with no ending (as after a Cancel of
    // its own).
    sim.board.pad.radio.request(PairReq::Cancel);
    sim.idle(3);
    assert_eq!(page(&sim), AppPage::Pads);
    assert_eq!(sel(&sim), PLACES);
}

#[test]
fn forgetting_player_1_runs_without_boot_hold() {
    let mut sim = home(Entry::Tiles, one_saved());
    open_gamepads_by_tile(&mut sim);
    // Row 0 (Player 1) by touch opens Forget confirmation.
    sim.tap_at(60, (pads_row_y(0) + 6) as u16);
    assert_eq!(page(&sim), AppPage::Forget);
    // A at once (KEEP) keeps it.
    sim.tap_button(SOUTH);
    assert_eq!(page(&sim), AppPage::Pads);
    assert_eq!(sel(&sim), 0);
    assert_eq!(fake(&sim).requests, 0);
    // FORGET button by touch sends Forget at once (no BOOT hold!).
    sim.tap_at(60, (pads_row_y(0) + 6) as u16);
    assert_eq!(page(&sim), AppPage::Forget);
    let (x, y, w, h) = button_box(2, 1);
    sim.board.pad.radio.manual = true;
    sim.tap_at((x + w as i16 / 2) as u16, (y + h as i16 / 2) as u16);
    assert_eq!(page(&sim), AppPage::Forgetting);
    assert!(sim.console.pairing_page());
    assert_eq!(
        fake(&sim).last_request,
        Some(PairReq::Forget {
            place: 0,
            tail: FAKE_SAVED_TAILS[0]
        })
    );
    sim.board.pad.radio.manual = false;
    wait_for(&mut sim, AppPage::End, 10);
    assert_eq!(sim.console.settings().state(), PairState::Forgotten(0));
    assert_eq!(fake(&sim).saved_pad(0), None);
    sim.tap_button(SOUTH);
    assert_eq!(page(&sim), AppPage::Pads);
    assert_eq!(sel(&sim), PLACES);
    assert_eq!(fake(&sim).last_request, Some(PairReq::Ack));
    // Back once from directly-opened GAMEPADS goes home.
    sim.tap_button(EAST);
    assert!(!open(&sim));
}

#[test]
fn a_forget_checks_the_tail_the_player_saw() {
    // Open Forget page for place 0, change tail underneath, tap FORGET:
    // the board answers BUSY and keeps the pad there now.
    let mut sim = home(Entry::Tiles, one_saved());
    open_gamepads_by_tile(&mut sim);
    assert_eq!(sel(&sim), 0);
    sim.tap_button(SOUTH);
    assert_eq!(page(&sim), AppPage::Forget);
    sim.board.pad.radio.saved[0] = Some(SavedPad {
        tail: FAKE_TAIL_B,
        connected: false,
    });
    sim.tap_button(RIGHT);
    sim.tap_button(SOUTH);
    wait_for(&mut sim, AppPage::End, 10);
    assert_eq!(fail_of(&sim), PairFail::Busy);
    assert_eq!(fake(&sim).saved_pad(0).map(|s| s.tail), Some(FAKE_TAIL_B));
    // OK: the list, on the place it was about.
    sim.tap_button(SOUTH);
    assert_eq!((page(&sim), sel(&sim)), (AppPage::Pads, 0));
}

#[test]
fn a_forget_can_fail() {
    for why in [PairFail::NotHeld, PairFail::Store, PairFail::Damaged] {
        let mut radio = one_saved();
        radio.script = radio.script.forget_fails(why);
        let mut sim = home(Entry::Tiles, radio);
        open_gamepads_by_tile(&mut sim);
        sim.tap_button(SOUTH);
        sim.tap_button(RIGHT);
        sim.tap_button(SOUTH);
        wait_for(&mut sim, AppPage::End, 10);
        assert_eq!(fail_of(&sim), why);
        // No TRY AGAIN for a forget.
        assert_eq!(sim.console.settings().buttons(), &["OK"]);
        assert!(fake(&sim).saved_pad(0).is_some(), "{why:?}");
    }
}

#[test]
fn legacy_multiple_saved_pads_show_list_and_allow_forgetting_each() {
    let radio = FakeRadio::full(PairScript::found_and_saved(5, 5));
    let mut sim = home(Entry::Tiles, radio);
    open_gamepads_by_tile(&mut sim);
    assert_eq!(sel(&sim), 0);
    // Cannot move down onto row 4 ("FORGET ALL") or tap it.
    for _ in 0..4 {
        sim.tap_button(DOWN);
    }
    assert_eq!(sel(&sim), 3, "last saved pad, not PAIR row");
    sim.tap_at(80, (pads_row_y(PLACES) + 6) as u16);
    assert_eq!(page(&sim), AppPage::Pads);
    assert_eq!(fake(&sim).requests, 0);

    // Forget pad 4 (place 3).
    sim.tap_button(SOUTH);
    assert_eq!(page(&sim), AppPage::Forget);
    sim.tap_button(RIGHT);
    sim.tap_button(SOUTH);
    wait_for(&mut sim, AppPage::End, 10);
    assert_eq!(sim.console.settings().state(), PairState::Forgotten(3));
    assert_eq!(fake(&sim).saved_pad(3), None);
    sim.tap_button(SOUTH);
    assert_eq!(page(&sim), AppPage::Pads);
}

#[test]
fn a_board_that_cannot_pair_says_so_and_sends_nothing() {
    // The fake's default: no pairing, no saved pads (as the trait's
    // default bodies).
    let mut sim = home(Entry::Tiles, FakeRadio::default());
    open_gamepads_by_tile(&mut sim);
    assert_eq!(sel(&sim), PLACES);
    sim.tap_button(SOUTH);
    sim.input(Ev::QuickBoot);
    sim.idle(2);
    sim.tap_at(80, (pads_row_y(1) + 7) as u16);
    assert_eq!(page(&sim), AppPage::Pads);
    assert!(!sim.console.pairing_page());
    // A board that cannot pair but lists a saved pad: no FORGET either.
    let mut radio = FakeRadio::default();
    radio.saved[0] = Some(SavedPad {
        tail: FAKE_SAVED_TAILS[0],
        connected: true,
    });
    let mut sim = home(Entry::Tiles, radio);
    open_gamepads_by_tile(&mut sim);
    assert_eq!(sel(&sim), 0);
    sim.tap_button(SOUTH);
    assert_eq!(page(&sim), AppPage::Pads);
    assert_eq!(fake(&sim).requests, 0);
}

#[test]
fn the_open_app_counts_as_an_open_menu() {
    let mut sim = home(Entry::Tiles, one_saved());
    assert!(!sim.console.menu_open());
    open_settings_by_menu(&mut sim);
    assert!(sim.console.menu_open());
    assert!(!sim.console.pairing_page());
    sim.tap_button(EAST);
    assert!(!sim.console.menu_open());
}

#[test]
fn the_names_shown_come_from_the_board() {
    let mut sim = home(Entry::Tiles, empty_radio());
    to_found_list(&mut sim);
    let found = fake(&sim).found(0).unwrap();
    assert_eq!(found.name(), FAKE_NAME_A);
    assert_eq!(found.tail, FAKE_TAIL_A);
}

/// The page `want`, reached by pad from home.
fn reach(want: AppPage) -> Sim {
    let mut sim = match want {
        AppPage::Pads | AppPage::Forget | AppPage::Forgetting => home(Entry::Tiles, one_saved()),
        _ => home(Entry::Tiles, empty_radio()),
    };
    match want {
        AppPage::Main => open_settings_by_menu(&mut sim),
        AppPage::About => {
            open_settings_by_menu(&mut sim);
            for _ in 0..ABOUT_LINE {
                sim.tap_button(DOWN);
            }
            sim.tap_button(SOUTH);
        }
        AppPage::Pads => open_gamepads_by_tile(&mut sim),
        AppPage::Forget => {
            open_gamepads_by_tile(&mut sim);
            sim.tap_button(SOUTH);
        }
        AppPage::Forgetting => {
            open_gamepads_by_tile(&mut sim);
            sim.tap_button(SOUTH);
            sim.board.pad.radio.manual = true;
            sim.tap_button(RIGHT);
            sim.tap_button(SOUTH);
        }
        AppPage::Searching => {
            to_pair(&mut sim);
            sim.tap_button(SOUTH);
        }
        AppPage::Found | AppPage::Calling | AppPage::End => {
            to_pair(&mut sim);
            sim.tap_button(SOUTH);
            sim.idle(u64::from(FIND) + 5);
            sim.tap_button(SOUTH);
            if want != AppPage::Found {
                sim.tap_button(RIGHT);
                sim.tap_button(SOUTH);
            }
            if want == AppPage::End {
                wait_for(&mut sim, AppPage::End, u64::from(CALL) + 20);
            }
        }
    }
    assert_eq!(page(&sim), want);
    sim
}

const ALL: [AppPage; 9] = [
    AppPage::Main,
    AppPage::Pads,
    AppPage::About,
    AppPage::Searching,
    AppPage::Found,
    AppPage::Calling,
    AppPage::End,
    AppPage::Forget,
    AppPage::Forgetting,
];

#[test]
fn every_page_has_a_way_back_by_touch() {
    // One page up for each; from Main or directly-opened Pads, home.
    // The tap area is bigger than the drawn button: its far corners count too.
    let (x, y, w, h) = BACK_HIT;
    let corners = [
        BACK,
        (x as u16, y as u16),
        ((x + w as i16 - 1) as u16, (y + h as i16 - 1) as u16),
    ];
    for (i, from) in ALL.into_iter().enumerate() {
        let to = match from {
            AppPage::Main | AppPage::Pads => None,
            AppPage::About => Some(AppPage::Main),
            AppPage::Found => Some(AppPage::Searching),
            _ => Some(AppPage::Pads),
        };
        let mut sim = reach(from);
        let (tx, ty) = corners[i % corners.len()];
        sim.tap_at(tx, ty);
        match to {
            Some(p) => assert_eq!(page(&sim), p, "BACK from {from:?}"),
            None => assert!(!open(&sim), "BACK from {from:?}"),
        }
        if matches!(
            from,
            AppPage::Searching | AppPage::Calling | AppPage::Forgetting
        ) {
            assert_eq!(fake(&sim).last_request, Some(PairReq::Cancel), "{from:?}");
        }
    }
    // Just outside the tap area: nothing.
    let mut sim = reach(AppPage::About);
    sim.tap_at((x + w as i16) as u16, 4);
    assert_eq!(page(&sim), AppPage::About);
}

#[test]
fn every_page_has_a_way_back_by_pad() {
    for from in ALL.into_iter().skip(1) {
        let mut sim = reach(from);
        sim.tap_button(EAST);
        if from == AppPage::Pads {
            assert!(!open(&sim), "B on direct GAMEPADS leaves app");
        } else {
            assert!(open(&sim));
            assert_ne!(page(&sim), from, "B on {from:?}");
        }
        // Home leaves at once; an open pairing or Forgetting is cancelled,
        // an ending acknowledged.
        let mut sim = reach(from);
        let before = fake(&sim).requests;
        sim.tap_button(HOME);
        assert!(!open(&sim), "Home on {from:?}");
        let want = match from {
            AppPage::End => Some(PairReq::Ack),
            p if p.window() || p == AppPage::Forgetting => Some(PairReq::Cancel),
            _ => None,
        };
        if let Some(r) = want {
            assert_eq!(fake(&sim).last_request, Some(r), "Home on {from:?}");
        } else {
            assert_eq!(fake(&sim).requests, before, "Home on {from:?}");
        }
    }
}

/// Golden fingerprints of the app's pages: every page reached by pad,
/// every ending of the board, a board that cannot pair, GAMEPADS states,
/// and home with placement E (with card, no card, quick menu). Stored in
/// `tests/data/golden_settings.txt`, and with `boot-gather` in
/// `tests/data/golden_settings_gather.txt`. When the look changes on
/// purpose, look at the new pictures in the simulator first, then:
///
/// ```text
/// UPDATE_GOLDEN=1 cargo test --features settings --test settings golden
/// UPDATE_GOLDEN=1 cargo test --features settings,boot-gather --test settings golden
/// ```
mod golden {
    use super::*;
    use common::fingerprint;

    #[cfg(not(feature = "boot-gather"))]
    const FILE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/golden_settings.txt"
    );
    #[cfg(feature = "boot-gather")]
    const FILE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/golden_settings_gather.txt"
    );

    /// Lets the picture settle, then names its fingerprint.
    fn shot(out: &mut Vec<String>, sim: &mut Sim, name: &str) {
        sim.idle(3);
        out.push(format!(
            "{name} {:016x}",
            fingerprint(&sim.board.screen.image)
        ));
    }

    fn tour() -> Vec<String> {
        let mut out = Vec::new();
        for want in ALL {
            let mut sim = reach(want);
            shot(&mut out, &mut sim, &format!("{want:?}").to_lowercase());
        }
        let s = PairScript::found_and_saved(5, 5);
        let ends = [
            ("old-pin", FakeRadio::new(s.call_fails(PairFail::OldPin))),
            ("lost", FakeRadio::new(s.call_fails(PairFail::Lost))),
            ("store", FakeRadio::new(s.call_fails(PairFail::Store))),
            ("refused", FakeRadio::new(s.call_fails(PairFail::Refused))),
            ("timeout", FakeRadio::new(s.call_fails(PairFail::Timeout))),
            ("linked", FakeRadio::new(s.begin_fails(PairFail::Linked))),
            ("full", FakeRadio::new(s.begin_fails(PairFail::Full))),
            ("not-held", FakeRadio::new(s.begin_fails(PairFail::NotHeld))),
            ("damaged", FakeRadio::new(s.begin_fails(PairFail::Damaged))),
            ("busy", FakeRadio::new(s.begin_fails(PairFail::Busy))),
            (
                "nothing-found",
                FakeRadio::new(PairScript::nothing_found(60)),
            ),
        ];
        for (name, radio) in ends {
            let mut sim = pair_to_end(radio);
            shot(&mut out, &mut sim, &format!("end-{name}"));
        }
        let mut sim = home(Entry::Tiles, FakeRadio::default());
        open_gamepads_by_tile(&mut sim);
        shot(&mut out, &mut sim, "pads-cannot-pair");

        let mut sim = home(Entry::Tiles, one_saved());
        open_gamepads_by_tile(&mut sim);
        shot(&mut out, &mut sim, "pads-one-connected");

        let mut sim = home(Entry::Tiles, empty_radio());
        open_gamepads_by_tile(&mut sim);
        shot(&mut out, &mut sim, "pads-empty");

        let mut sim = home(Entry::Tiles, FakeRadio::full(s));
        open_gamepads_by_tile(&mut sim);
        shot(&mut out, &mut sim, "pads-legacy-full");

        let mut sim = home(Entry::Tiles, one_saved());
        shot(&mut out, &mut sim, "home-tiles");

        let mut sim = home_no_card(Entry::Tiles, empty_radio());
        shot(&mut out, &mut sim, "home-tiles-no-card");

        let mut sim = home(Entry::Tiles, one_saved());
        sim.tap_button(HOME);
        shot(&mut out, &mut sim, "home-menu-settings");
        out
    }

    #[test]
    fn golden_the_same_pictures_every_run() {
        assert_eq!(tour(), tour());
    }

    #[test]
    fn golden_pages_match_the_stored_ones() {
        let now = tour().join("\n") + "\n";
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::write(FILE, &now).unwrap();
            eprintln!("wrote {FILE}");
            return;
        }
        let stored =
            std::fs::read_to_string(FILE).expect("no golden file: run with UPDATE_GOLDEN=1 once");
        assert_eq!(
            now, stored,
            "the pictures changed; if on purpose, run with UPDATE_GOLDEN=1"
        );
    }
}
