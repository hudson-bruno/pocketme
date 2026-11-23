//! Drag scrolling on the home row, and taps that act when the finger
//! lifts (step 4). The fake board, the fake clock, no window.
//!
//! Positions are picture pixels (the test board doubles them for the
//! panel). The row band is y 19..61; tile `i` starts at
//! 14 + 44 i - scroll.

mod common;

use std::cell::Cell;

use board_api::{TouchPoint, Wake};
use common::{Ev, FOUR, NINE, RIGHT, Sim};
use console_core::game_api::LinkedGame;
use console_core::home::STRIDE;
use console_core::theme::{
    DRAG_START, FLICK_MAX, TILE, TILE_LEFT, TILE_MARGIN, TILE_Y, TOUCH_GRACE, TOUCH_STILL_US,
};
use console_core::{FRAME_US, Page, msg};
use game_kit::{Ctx, Flow, Game};

/// A picture row on the tiles.
const ROW: u16 = (TILE_Y + 10) as u16;

/// Where tile `i` is on screen now (left edge).
fn tile_x(sim: &Sim, i: usize) -> i16 {
    TILE_LEFT + i as i16 * STRIDE - sim.console.home().scroll()
}

fn scroll(sim: &Sim) -> i16 {
    sim.console.home().scroll()
}

/// A resting place: a tile at the left margin, or the far end.
fn is_rest(sim: &Sim) -> bool {
    let s = scroll(sim);
    (s - (TILE_LEFT - TILE_MARGIN)) % STRIDE == 0 || s == sim.console.home().max_scroll()
}

/// The selected tile is inside the margins (where the buttons keep it).
fn selected_in_margins(sim: &Sim) -> bool {
    let x = tile_x(sim, sim.console.home().selected());
    x >= TILE_MARGIN && x + TILE as i16 + TILE_MARGIN <= 160
}

/// The finger at `x`, `y`: down, or moved there if already down. One
/// tick.
fn finger(sim: &mut Sim, x: u16, y: u16) {
    sim.input(Ev::Touch(x, y));
    sim.step_tick();
}

/// The finger lifts. A tap acts at once; a drag ends after the grace
/// frames ([`TOUCH_GRACE`]), so this runs those too.
fn lift(sim: &mut Sim) {
    sim.input(Ev::Lift);
    sim.step_tick();
    sim.idle(u64::from(TOUCH_GRACE));
}

/// Runs ticks until the row rests; returns how many it took.
fn settle(sim: &mut Sim) -> u64 {
    let mut n = 0;
    while sim.console.home().moving() || sim.console.home().settling() {
        sim.step_tick();
        n += 1;
        assert!(n < 60, "the row never comes to rest");
    }
    n
}

/// Steps in the last `n` ticks.
fn steps(sim: &Sim, n: usize) -> u32 {
    sim.ticks[sim.ticks.len() - n..]
        .iter()
        .map(|t| t.steps)
        .sum()
}

/// Booted, home showing, nothing moving.
fn home(card: &'static [(&'static str, &'static [u8])]) -> Sim {
    let mut sim = Sim::new(Some(card));
    sim.boot();
    sim.idle(10);
    sim
}

/// A card with `n` games, GAME00 to GAMEnn.
fn card_of(n: usize) -> &'static [(&'static str, &'static [u8])] {
    let files: Vec<(&'static str, &'static [u8])> = (0..n)
        .map(|i| {
            let path: &'static str =
                Box::leak(format!("/CONSOLE/GAMES/GAME{i:02}/GAME.BIN").into_boxed_str());
            (path, &b"GAME"[..])
        })
        .collect();
    Box::leak(files.into_boxed_slice())
}

// ---- Dragging --------------------------------------------------------------

#[test]
fn the_row_follows_the_finger_one_to_one() {
    let mut sim = home(NINE);
    assert_eq!(scroll(&sim), 0);
    finger(&mut sim, 100, ROW);
    // Under the threshold: nothing moves yet.
    finger(&mut sim, 100 - (DRAG_START - 1), ROW);
    assert_eq!(scroll(&sim), 0);
    let under = tile_x(&sim, 2);
    // Past it: the row catches up with the finger at once...
    finger(&mut sim, 95, ROW);
    assert_eq!(scroll(&sim), 5);
    assert_eq!(
        tile_x(&sim, 2),
        under - 5,
        "the tile stays under the finger"
    );
    // ...and follows it, both ways.
    for x in [80, 60, 70, 41] {
        finger(&mut sim, x, ROW);
        assert_eq!(scroll(&sim), 100 - x as i16);
        assert!(!sim.console.home().moving());
    }
}

#[test]
fn the_drag_stops_at_both_ends() {
    let mut sim = home(NINE);
    let max = sim.console.home().max_scroll();
    assert_eq!(
        max,
        8 * STRIDE + TILE_LEFT + TILE as i16 + TILE_MARGIN - 160
    );
    // To the right at the start: nothing to show before the first tile.
    finger(&mut sim, 20, ROW);
    finger(&mut sim, 60, ROW);
    finger(&mut sim, 150, ROW);
    assert_eq!(scroll(&sim), 0);
    // Back to the left goes at once (no slack from the clamp).
    finger(&mut sim, 140, ROW);
    assert_eq!(scroll(&sim), 10);
    lift(&mut sim);
    settle(&mut sim);
    // Far to the left, in several strokes: stops at the far end.
    for _ in 0..4 {
        sim.idle(5);
        finger(&mut sim, 150, ROW);
        finger(&mut sim, 140, ROW);
        finger(&mut sim, 10, ROW);
        sim.idle(5); // rest, so no flick
        lift(&mut sim);
        settle(&mut sim);
    }
    assert_eq!(scroll(&sim), max);
    // The row pushed the selection along: the nearest tile shown, the
    // first of the last three.
    assert_eq!(sim.console.home().selected(), 6);
    assert!(selected_in_margins(&sim));
}

#[test]
fn let_go_the_row_rests_in_a_tidy_place() {
    for end in [96, 90, 80, 75, 70, 52] {
        let mut sim = home(NINE);
        finger(&mut sim, 100, ROW);
        finger(&mut sim, 98, ROW);
        finger(&mut sim, end, ROW);
        // Resting before lifting: no flick, the nearest resting place.
        sim.idle(ticks(TOUCH_STILL_US) + 1);
        lift(&mut sim);
        let frames = settle(&mut sim);
        assert!(frames <= 8, "a short glide: {frames} frames");
        let moved = 100 - end as i16;
        let want = if moved <= STRIDE / 2 { 0 } else { STRIDE };
        assert_eq!(scroll(&sim), want, "dragged {moved}");
        assert!(is_rest(&sim));
        assert!(selected_in_margins(&sim));
        // The buttons then find nothing to fix.
        assert!(!sim.console.home().moving());
    }
}

/// Ticks for `us` microseconds.
fn ticks(us: u64) -> u64 {
    us.div_ceil(FRAME_US)
}

#[test]
fn a_flick_carries_the_row_further() {
    // The same drag, let go while still moving fast, or after resting.
    let run = |flick: bool| {
        let mut sim = home(NINE);
        for x in [130, 118, 106, 94] {
            finger(&mut sim, x, ROW);
        }
        if !flick {
            sim.idle(ticks(TOUCH_STILL_US) + 1);
        }
        let from = scroll(&sim);
        lift(&mut sim);
        let frames = settle(&mut sim);
        assert!(frames <= 9, "a short glide: {frames} frames");
        assert!(is_rest(&sim) && selected_in_margins(&sim));
        (from, scroll(&sim))
    };
    let (from, rested) = run(false);
    assert_eq!((from, rested), (36, 44), "the nearest resting place");
    let (from, flicked) = run(true);
    assert_eq!(from, 36);
    assert!(flicked > rested, "a flick goes further: {flicked}");
}

#[test]
fn a_fast_flick_goes_at_most_flick_max() {
    let mut sim = home(card_of(32));
    finger(&mut sim, 150, ROW);
    finger(&mut sim, 140, ROW);
    finger(&mut sim, 10, ROW); // 130 pixels in one frame
    let from = scroll(&sim);
    lift(&mut sim);
    let frames = settle(&mut sim);
    assert!(frames <= 10, "a short glide: {frames} frames");
    let went = (scroll(&sim) - from) as i32;
    assert!(went > STRIDE as i32, "it went on: {went}");
    // At most the carry plus the way to the next resting place.
    assert!(went <= FLICK_MAX + STRIDE as i32, "{went}");
    assert!(is_rest(&sim) && selected_in_margins(&sim));
}

#[test]
fn thirty_two_games_drag_to_the_end_and_back() {
    let mut sim = home(card_of(32));
    assert_eq!(sim.console.home().count(), 32);
    let max = sim.console.home().max_scroll();
    for _ in 0..40 {
        finger(&mut sim, 150, ROW);
        finger(&mut sim, 140, ROW);
        finger(&mut sim, 10, ROW);
        lift(&mut sim);
        settle(&mut sim);
        assert!(is_rest(&sim) && selected_in_margins(&sim));
    }
    assert_eq!(scroll(&sim), max);
    assert_eq!(sim.console.home().selected(), 29);
    for _ in 0..40 {
        finger(&mut sim, 10, ROW);
        finger(&mut sim, 20, ROW);
        finger(&mut sim, 150, ROW);
        lift(&mut sim);
        settle(&mut sim);
    }
    assert_eq!(scroll(&sim), 0);
    assert_eq!(sim.console.home().selected(), 2, "the nearest shown tile");
    // Buttons still work from here.
    sim.tap_button(RIGHT);
    assert_eq!(sim.console.home().selected(), 3);
    settle(&mut sim);
    assert!(selected_in_margins(&sim));
}

#[test]
fn a_drag_that_starts_off_the_row_does_not_scroll() {
    let mut sim = home(NINE);
    for y in [(TILE_Y - 8) as u16, 70, 100] {
        finger(&mut sim, 140, y);
        finger(&mut sim, 100, ROW); // into the row band: still not
        finger(&mut sim, 20, ROW);
        lift(&mut sim);
        sim.idle(10);
        assert_eq!(scroll(&sim), 0, "started at y {y}");
        assert_eq!(sim.console.home().selected(), 0);
        assert!(sim.console.notices().is_empty());
    }
}

#[test]
fn a_short_row_does_not_scroll_and_taps_still_work() {
    for n in [1, 2, 3] {
        let mut sim = home(card_of(n));
        assert_eq!(sim.console.home().max_scroll(), 0);
        finger(&mut sim, 30, ROW);
        finger(&mut sim, 10, ROW);
        finger(&mut sim, 100, ROW);
        lift(&mut sim);
        sim.idle(10);
        assert_eq!(scroll(&sim), 0);
        assert!(sim.console.notices().is_empty(), "a drag is not a tap");
        // A tap on the last tile selects it, a second one starts it.
        let x = (tile_x(&sim, n - 1) + 10) as u16;
        sim.tap_at(x, ROW);
        assert_eq!(sim.console.home().selected(), n - 1);
        if n > 1 {
            assert!(sim.console.notices().is_empty());
        }
        sim.idle(2);
        sim.tap_at(x, ROW);
        assert_eq!(sim.console.notices().current(), Some(msg::NO_START));
    }
}

// ---- The selection ----------------------------------------------------------

#[test]
fn the_selection_never_leaves_the_screen() {
    let mut sim = home(NINE);
    assert_eq!(sim.console.home().selected(), 0);
    finger(&mut sim, 150, ROW);
    // A small drag keeps the selection (tile 0 is still fully shown).
    finger(&mut sim, 140, ROW);
    assert_eq!(sim.console.home().selected(), 0);
    // Pushed off screen: the nearest tile fully shown is chosen.
    for x in (20..140).rev().step_by(7) {
        finger(&mut sim, x, ROW);
        let sel = sim.console.home().selected();
        let at = tile_x(&sim, sel);
        assert!(
            at >= 3 && at + TILE as i16 + 3 <= 160,
            "x {at} at finger {x}"
        );
    }
    assert!(sim.console.home().selected() >= 2);
    lift(&mut sim);
    settle(&mut sim);
    assert!(selected_in_margins(&sim));
}

#[test]
fn a_button_takes_the_row_from_the_finger() {
    let mut sim = home(NINE);
    finger(&mut sim, 140, ROW);
    finger(&mut sim, 120, ROW);
    finger(&mut sim, 101, ROW); // scroll 39
    let sel = sim.console.home().selected();
    // A finger move and a button in the same frame: the button wins.
    sim.input(Ev::Press(RIGHT));
    finger(&mut sim, 90, ROW);
    sim.input(Ev::Release(RIGHT));
    assert_eq!(sim.console.home().selected(), sel + 1);
    // The finger no longer moves the row; lifting gives no glide.
    let frames = settle(&mut sim);
    assert!(frames <= 8, "{frames}");
    let rested = scroll(&sim);
    finger(&mut sim, 20, ROW);
    lift(&mut sim);
    sim.idle(10);
    assert_eq!(scroll(&sim), rested);
    assert_eq!(sim.console.home().selected(), sel + 1);
    assert!(selected_in_margins(&sim));
    assert!(sim.console.notices().is_empty());
}

#[test]
fn a_button_ends_the_glide() {
    // A flick, then RIGHT in the next frame, or no button.
    let run = |button: bool| {
        let mut sim = home(card_of(32));
        finger(&mut sim, 150, ROW);
        finger(&mut sim, 140, ROW);
        finger(&mut sim, 10, ROW);
        lift(&mut sim);
        assert!(sim.console.home().settling());
        let sel = sim.console.home().selected();
        if button {
            sim.tap_button(RIGHT);
            assert_eq!(sim.console.home().selected(), sel + 1);
        }
        settle(&mut sim);
        assert!(selected_in_margins(&sim));
        (sim.console.home().selected(), scroll(&sim))
    };
    let (_, glided) = run(false);
    let (_, stopped) = run(true);
    assert!(
        stopped < glided,
        "the glide was cut short: {stopped} vs {glided}"
    );
}

// ---- Taps --------------------------------------------------------------------

#[test]
fn a_tap_acts_when_the_finger_lifts() {
    let mut sim = home(NINE);
    let x = (tile_x(&sim, 2) + 10) as u16;
    finger(&mut sim, x, ROW);
    sim.idle(5);
    assert_eq!(
        sim.console.home().selected(),
        0,
        "not while the finger is down"
    );
    lift(&mut sim);
    assert_eq!(sim.console.home().selected(), 2);
    // A wobble under the threshold is still a tap; it starts the game.
    finger(&mut sim, x, ROW);
    finger(&mut sim, x + DRAG_START - 1, ROW + 2);
    assert!(sim.console.notices().is_empty());
    lift(&mut sim);
    assert_eq!(sim.console.notices().current(), Some(msg::NO_START));
}

#[test]
fn a_drag_on_the_selected_tile_does_not_start_it() {
    let mut sim = home(FOUR);
    let x = (tile_x(&sim, 0) + 15) as u16;
    finger(&mut sim, x, ROW);
    finger(&mut sim, x - 10, ROW);
    finger(&mut sim, x - 20, ROW);
    lift(&mut sim);
    sim.idle(10);
    assert!(sim.console.notices().is_empty(), "no game was started");
    assert_eq!(sim.console.page(), Page::Home);
}

#[test]
fn a_drag_back_to_where_it_began_is_not_a_tap() {
    let mut sim = home(NINE);
    let x = (tile_x(&sim, 0) + 15) as u16; // the selected tile
    finger(&mut sim, x, ROW);
    finger(&mut sim, x - 8, ROW);
    finger(&mut sim, x, ROW);
    lift(&mut sim);
    sim.idle(10);
    assert!(
        sim.console.notices().is_empty(),
        "the largest distance counts"
    );
    assert_eq!(scroll(&sim), 0);
    assert_eq!(sim.console.home().selected(), 0);
}

#[test]
fn a_tap_starts_only_a_tile_selected_when_the_finger_went_down() {
    let mut sim = home(NINE);
    let x = (tile_x(&sim, 1) + 10) as u16;
    finger(&mut sim, x, ROW);
    // A button moves the choice onto that tile while the finger is down.
    sim.tap_button(RIGHT);
    assert_eq!(sim.console.home().selected(), 1);
    lift(&mut sim);
    sim.idle(2);
    assert!(sim.console.notices().is_empty(), "only chosen, not started");
    sim.tap_at(x, ROW);
    sim.idle(2);
    assert_eq!(sim.console.notices().current(), Some(msg::NO_START));
}

#[test]
fn the_touch_that_skips_the_boot_screen_does_not_tap_home() {
    let mut sim = Sim::new(Some(FOUR));
    sim.idle(3);
    // On the first tile, which is the selected one on home.
    sim.input(Ev::Touch(30, ROW));
    sim.step_tick();
    assert_eq!(sim.console.page(), Page::Home);
    sim.idle(3);
    lift(&mut sim);
    sim.idle(10);
    assert!(sim.console.notices().is_empty(), "no game was started");
    // The next touch counts again.
    sim.tap_at(30, ROW);
    assert_eq!(sim.console.notices().current(), Some(msg::NO_START));
}

// ---- Waking -------------------------------------------------------------------

#[test]
fn a_drag_draws_every_frame_and_a_resting_finger_sleeps() {
    let mut sim = home(NINE);
    finger(&mut sim, 140, ROW);
    for x in (40..135).rev().step_by(5) {
        sim.input(Ev::Touch(x, ROW));
        let t = sim.step_tick();
        assert_eq!(t.steps, 1);
        assert!(matches!(sim.console.step(&mut *sim.board), Wake::At(_)));
    }
    // Moving: one frame drawn per tick, even with no new input.
    sim.idle(2);
    assert_eq!(steps(&sim, 2), 2);
    // The finger rests: the console sleeps, though it is still down.
    sim.idle(60);
    assert!(steps(&sim, 55) <= 2, "{} steps", steps(&sim, 55));
    assert!(sim.console.step(&mut *sim.board).on_input());
    assert!(sim.board.touch.at.is_some());
    // Moving again wakes it; lifting settles the row, then it sleeps.
    finger(&mut sim, 30, ROW);
    lift(&mut sim);
    settle(&mut sim);
    sim.idle(90);
    assert!(steps(&sim, 90) <= 2, "{} steps", steps(&sim, 90));
    assert!(sim.console.step(&mut *sim.board).on_input());
}

#[test]
fn a_tap_is_seen_even_when_the_board_wakes_nobody_at_the_lift() {
    // A board that shows a touch for one read and then none, with no
    // input change in between (the CYD's taps from the debug link).
    let mut sim = home(NINE);
    let x = (tile_x(&sim, 1) + 10) as u16;
    sim.input(Ev::Touch(x, ROW));
    sim.step_tick();
    sim.board.touch.at = None; // gone, and nobody says so
    sim.idle(2);
    assert_eq!(sim.console.home().selected(), 1, "seen at the next frame");
}

// ---- A noisy panel ------------------------------------------------------------

#[test]
fn a_finger_missing_for_one_frame_does_not_end_the_drag() {
    let mut sim = home(NINE);
    finger(&mut sim, 140, ROW);
    finger(&mut sim, 130, ROW);
    finger(&mut sim, 120, ROW);
    assert_eq!(scroll(&sim), 20);
    // One read with no finger: nothing happens.
    sim.input(Ev::Lift);
    sim.step_tick();
    assert_eq!(scroll(&sim), 20);
    assert!(!sim.console.home().settling(), "the drag goes on");
    // It comes back (the board also notes a new touch): same drag.
    sim.input(Ev::Touch(110, ROW));
    sim.input(Ev::QuickTouch(110, ROW));
    sim.step_tick();
    assert_eq!(scroll(&sim), 30);
    finger(&mut sim, 100, ROW);
    assert_eq!(scroll(&sim), 40);
    // Missing for two frames: the drag ended.
    sim.input(Ev::Lift);
    sim.step_tick();
    assert!(!sim.console.home().settling());
    sim.step_tick();
    assert!(sim.console.home().settling() || is_rest(&sim));
    settle(&mut sim);
    assert!(is_rest(&sim));
    assert!(sim.console.notices().is_empty());
}

#[test]
fn a_tap_shorter_than_a_frame_still_taps() {
    let mut sim = home(NINE);
    let x = (tile_x(&sim, 0) + 10) as u16;
    sim.input(Ev::QuickTouch(x, ROW));
    sim.step_tick();
    assert_eq!(sim.console.notices().current(), Some(msg::NO_START));
}

// ---- With other things going on -----------------------------------------------

#[test]
fn a_drag_while_a_notice_slides_in() {
    let mut sim = home(NINE);
    assert!(sim.notify("HELLO"));
    finger(&mut sim, 140, ROW);
    for x in [130, 120, 110, 100] {
        finger(&mut sim, x, ROW);
        assert_eq!(scroll(&sim), 140 - x as i16);
    }
    assert_eq!(sim.console.notices().current(), Some("HELLO"));
    lift(&mut sim);
    settle(&mut sim);
    assert!(is_rest(&sim));
}

// ---- Leaving home with a finger down -----------------------------------------

thread_local! {
    static QUIT: Cell<bool> = const { Cell::new(false) };
}

mod quitter {
    use super::*;

    /// Runs until told to quit.
    pub struct Quitter;

    impl Game for Quitter {
        fn start(_ctx: &mut Ctx) -> Self {
            Quitter
        }

        fn update(&mut self, _ctx: &mut Ctx) -> Flow {
            if QUIT.with(|q| q.get()) {
                Flow::Done
            } else {
                Flow::Run
            }
        }
    }

    game_kit::game!(Quitter);
}

// SAFETY: made by `game_kit::game!`.
static QUITTER: &[LinkedGame] = &[unsafe { LinkedGame::new("BLOCKS", quitter::game_entry) }];

#[test]
fn a_finger_held_when_a_game_ends_does_not_tap_home() {
    QUIT.with(|q| q.set(false));
    let mut sim = Sim::with_games(Some(FOUR), QUITTER);
    sim.boot();
    sim.idle(2);
    // Start BLOCKS (the selected first tile) with a tap.
    sim.tap_at(30, ROW);
    sim.idle(1);
    assert_eq!(sim.console.page(), Page::Game);
    // A finger on the selected tile's place; the game ends under it.
    finger(&mut sim, 30, ROW);
    QUIT.with(|q| q.set(true));
    sim.idle(3);
    assert_eq!(sim.console.page(), Page::Home);
    QUIT.with(|q| q.set(false));
    lift(&mut sim);
    sim.idle(3);
    assert_eq!(sim.console.page(), Page::Home, "the lift started nothing");
}

#[test]
fn the_pause_menu_still_acts_when_the_finger_goes_down() {
    QUIT.with(|q| q.set(false));
    let mut sim = Sim::with_games(Some(FOUR), QUITTER);
    sim.boot();
    sim.idle(2);
    sim.tap_at(30, ROW);
    sim.idle(1);
    assert_eq!(sim.console.page(), Page::Game);
    sim.tap_button(common::HOME);
    assert!(sim.console.game().paused());
    // "BACK TO HOME", touched and still held.
    use console_core::pause::{Kind, item_y};
    let y = item_y(Kind::Game, 2) as u16 + 2;
    sim.input(Ev::Touch(80, y));
    sim.step_tick();
    assert_eq!(sim.console.page(), Page::Home);
    // Lifting there (on home now) does nothing.
    lift(&mut sim);
    sim.idle(3);
    assert_eq!(sim.console.page(), Page::Home);
    assert_eq!(sim.board.touch.at, None::<TouchPoint>);
}
