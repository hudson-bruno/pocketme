//! Screen brightness (step 4): five steps, the console menu on the home
//! screen and over a game, and the backlight calls the board gets. The
//! fake board, the fake clock, no window.
//!
//! Positions are picture pixels (the test board doubles them for the
//! panel).

mod common;

use std::cell::Cell;

use board_api::Wake;
use common::{DOWN, EAST, Ev, FOUR, HOME, LEFT, RIGHT, SOUTH, Sim, UP};
use console_core::game_api::LinkedGame;
use console_core::pause::{self, BAR_W, Kind, bar_x, item_y};
use console_core::theme::{
    BACKLIGHT, BRIGHT_STEPS, GEAR_X, GEAR_Y, GOLD, MENU_X, MUTE, NOTE_Y, PANEL, TILE_Y,
};
use console_core::{Console, Page};
use game_kit::{Ctx, Flow, Game};

/// The middle of the gear in the bar.
const GEAR: (u16, u16) = ((GEAR_X + 4) as u16, (GEAR_Y + 4) as u16);
/// A picture row on the tiles.
const ROW: u16 = (TILE_Y + 10) as u16;

/// The backlight levels the board was asked for, in order.
fn levels(sim: &Sim) -> Vec<u8> {
    let s = &sim.board.screen;
    s.backlight_log[..s.backlight_calls.min(32)]
        .iter()
        .map(|&(l, _)| l)
        .collect()
}

/// The middle of the brightness bar for `step` in a menu.
fn bar(kind: Kind, step: u8) -> (u16, u16) {
    let i = kind.index_of(pause::Item::Bright).unwrap();
    (
        (bar_x(step) + BAR_W as i16 / 2) as u16,
        (item_y(kind, i) + 8) as u16,
    )
}

/// Where CLOSE is in the home menu (after BOOT ANIM with the feature
/// `boot-gather`).
fn close() -> usize {
    Kind::Home.index_of(pause::Item::Close).unwrap()
}

/// The middle of item `i` of a menu.
fn item(kind: Kind, i: usize) -> (u16, u16) {
    (80, (item_y(kind, i) + 4) as u16)
}

fn home() -> Sim {
    let mut sim = Sim::new(Some(FOUR));
    sim.boot();
    sim.idle(2);
    sim
}

fn boot_press(sim: &mut Sim) {
    sim.input(Ev::Boot(true));
    sim.step_tick();
    sim.input(Ev::Boot(false));
    sim.step_tick();
}

// ---- The steps and the backlight -------------------------------------------

#[test]
fn the_steps_are_five_never_off_and_evenly_spaced_to_the_eye() {
    assert_eq!(BRIGHT_STEPS, 5);
    assert_eq!(BACKLIGHT[4], 255, "the brightest is full");
    // Lightness (CIE L*) of each step, from its share of full light.
    let lightness = |v: u8| {
        let y = f64::from(v) / 255.0;
        116.0 * y.cbrt() - 16.0
    };
    let l: Vec<f64> = BACKLIGHT.iter().map(|&v| lightness(v)).collect();
    for w in l.windows(2) {
        assert!(
            (w[1] - w[0] - 17.5).abs() < 1.0,
            "lightness steps {l:?} are not even"
        );
    }
    assert!(BACKLIGHT.iter().all(|&v| v > 0));
}

#[test]
fn a_new_console_is_at_full_and_has_lit_nothing() {
    let c = Console::new();
    assert_eq!(c.brightness(), BRIGHT_STEPS - 1);
    assert_eq!(c.backlight(), 0);
    assert!(!c.menu_open());
}

#[test]
fn the_backlight_is_set_once_and_only_after_the_first_picture() {
    let mut sim = Sim::new(Some(FOUR));
    assert_eq!(sim.board.screen.backlight_calls, 0);
    sim.step_tick();
    let s = &sim.board.screen;
    assert_eq!(s.backlight_calls, 1);
    assert_eq!(s.backlight_log[0], (255, 1), "after the first present");
    assert!(s.image.iter().any(|&p| p != 0), "a picture was sent first");
    // Boot, home, a minute of idling: no other call.
    sim.boot();
    sim.idle(60 * 30);
    assert_eq!(sim.board.screen.backlight_calls, 1);
    assert_eq!(sim.console.backlight(), 255);
}

#[test]
fn a_board_sets_the_saved_step_at_start() {
    let mut sim = Sim::new(Some(FOUR));
    sim.console.set_brightness(1);
    assert_eq!(sim.console.backlight(), 0, "nothing lit before a picture");
    sim.step_tick();
    assert_eq!(levels(&sim), [BACKLIGHT[1]]);
    assert_eq!(sim.board.screen.backlight_log[0].1, 1);
    // Not a change by the player.
    assert!(!sim.console.take_settings_changed());
    // Too big is full.
    sim.console.set_brightness(200);
    assert_eq!(sim.console.brightness(), 4);
    sim.step_tick();
    assert_eq!(levels(&sim), [BACKLIGHT[1], 255]);
}

// ---- The menu on the home screen -----------------------------------------------

#[test]
fn home_opens_and_closes_the_menu_by_pad() {
    let mut sim = home();
    let sel = sim.console.home().selected();
    sim.tap_button(HOME);
    assert!(sim.console.menu_open());
    assert_eq!(sim.console.menu_item(), 0, "BRIGHTNESS is chosen");
    assert!(sim.board.log.lines().iter().any(|l| l == "console: menu"));
    sim.tap_button(EAST); // back
    assert!(!sim.console.menu_open());
    sim.tap_button(HOME);
    assert!(sim.console.menu_open());
    sim.tap_button(HOME); // Home again
    assert!(!sim.console.menu_open());
    sim.tap_button(HOME);
    for _ in 0..close() {
        sim.tap_button(DOWN);
    }
    assert_eq!(usize::from(sim.console.menu_item()), close());
    sim.tap_button(SOUTH); // CLOSE
    assert!(!sim.console.menu_open());
    // Nothing started, the row did not move.
    assert_eq!(sim.console.page(), Page::Home);
    assert_eq!(sim.console.home().selected(), sel);
    assert_eq!(sim.board.games.started, 0);
    assert_eq!(sim.console.brightness(), 4);
}

#[test]
fn the_gear_opens_and_closes_the_menu_by_touch() {
    let mut sim = home();
    // The gear is drawn in the bar, quiet when closed.
    assert_eq!(sim.pixel(GEAR.0, GEAR.1 - 4), MUTE);
    sim.tap_at(GEAR.0, GEAR.1);
    assert!(sim.console.menu_open());
    assert_eq!(sim.pixel(GEAR.0, GEAR.1 - 4), GOLD, "gold while open");
    // The box shows.
    assert_eq!(
        sim.pixel(MENU_X as u16 + 1, pause::box_y(Kind::Home) as u16 + 1),
        PANEL
    );
    // A touch on the gear closes it.
    sim.tap_at(GEAR.0, GEAR.1);
    assert!(!sim.console.menu_open());
    assert_eq!(sim.pixel(GEAR.0, GEAR.1 - 4), MUTE);
    // CLOSE closes it; its lift does not tap the row under it.
    sim.tap_at(GEAR.0, GEAR.1);
    assert!(sim.console.menu_open());
    let (x, y) = item(Kind::Home, close());
    sim.tap_at(x, y);
    assert!(!sim.console.menu_open());
    sim.idle(3);
    assert_eq!(sim.console.page(), Page::Home);
    assert_eq!(sim.board.games.started, 0);
    assert_eq!(sim.console.home().selected(), 0);
    assert_eq!(sim.console.brightness(), 4);
}

#[test]
fn a_tap_on_the_row_still_works_and_misses_the_gear() {
    let mut sim = home();
    sim.tap_at(80, ROW); // the second tile
    assert_eq!(sim.console.home().selected(), 1);
    assert!(!sim.console.menu_open());
    // The bar left of the gear's area is not the gear.
    sim.tap_at(120, GEAR.1);
    assert!(!sim.console.menu_open());
}

#[test]
fn a_finger_that_opens_the_menu_taps_nothing_else() {
    let mut sim = home();
    // The finger goes down on the gear and is held: nothing yet (the
    // home screen acts on the lift).
    sim.input(Ev::Touch(GEAR.0, GEAR.1));
    sim.step_tick();
    assert!(!sim.console.menu_open());
    sim.input(Ev::Lift);
    sim.step_tick();
    // The lift is believed one grace frame later.
    assert!(!sim.console.menu_open());
    sim.idle(u64::from(console_core::theme::TOUCH_GRACE));
    assert!(sim.console.menu_open());
}

#[test]
fn left_and_right_change_the_step_at_once() {
    let mut sim = home();
    sim.tap_button(HOME);
    sim.tap_button(LEFT);
    assert_eq!(sim.console.brightness(), 3);
    assert_eq!(levels(&sim), [255, BACKLIGHT[3]]);
    assert_eq!(sim.console.backlight(), BACKLIGHT[3]);
    assert!(sim.console.take_settings_changed());
    assert!(!sim.console.take_settings_changed(), "once");
    // The bars show it: the brightest bar is empty now.
    let (x, y) = bar(Kind::Home, 4);
    assert_ne!(sim.pixel(x, y), sim.pixel(bar(Kind::Home, 0).0, y));
    for _ in 0..6 {
        sim.tap_button(LEFT);
    }
    assert_eq!(sim.console.brightness(), 0);
    sim.tap_button(RIGHT);
    assert_eq!(sim.console.brightness(), 1);
    // One call per change, never 0.
    assert_eq!(
        levels(&sim),
        [
            255,
            BACKLIGHT[3],
            BACKLIGHT[2],
            BACKLIGHT[1],
            BACKLIGHT[0],
            BACKLIGHT[1]
        ]
    );
    assert!(
        sim.board
            .log
            .lines()
            .iter()
            .any(|l| l == "console: brightness 1")
    );
    // The row did not move: left and right were the menu's.
    assert_eq!(sim.console.home().selected(), 0);
    // On CLOSE, left and right change nothing.
    sim.tap_button(DOWN);
    sim.tap_button(RIGHT);
    assert_eq!(sim.console.brightness(), 1);
    assert_eq!(sim.board.screen.backlight_calls, 6);
}

#[test]
fn boot_on_the_brightness_line_goes_up_and_round() {
    let mut sim = home();
    sim.tap_at(GEAR.0, GEAR.1);
    let mut seen = Vec::new();
    for _ in 0..5 {
        boot_press(&mut sim);
        seen.push(sim.console.brightness());
    }
    assert_eq!(seen, [0, 1, 2, 3, 4]);
    assert!(sim.console.menu_open(), "BOOT does not close it here");
    assert_eq!(sim.board.games.started, 0);
    // A short BOOT press the board only noted counts too.
    sim.input(Ev::QuickBoot);
    sim.step_tick();
    assert_eq!(sim.console.brightness(), 0);
}

#[test]
fn a_touch_on_a_bar_sets_its_step() {
    let mut sim = home();
    sim.tap_at(GEAR.0, GEAR.1);
    for step in [0, 3, 1, 4, 2] {
        let (x, y) = bar(Kind::Home, step);
        sim.tap_at(x, y);
        assert_eq!(sim.console.brightness(), step);
        assert_eq!(sim.console.backlight(), BACKLIGHT[usize::from(step)]);
    }
    // The same bar again: no call.
    let n = sim.board.screen.backlight_calls;
    let (x, y) = bar(Kind::Home, 2);
    sim.tap_at(x, y);
    assert_eq!(sim.board.screen.backlight_calls, n);
    assert_eq!(n, 6);
    // The bars are over the row: their lifts started nothing.
    assert!(sim.console.menu_open());
    assert_eq!(sim.board.games.started, 0);
    assert_eq!(sim.console.page(), Page::Home);
}

#[test]
fn a_finger_sliding_on_the_bars_follows() {
    let mut sim = home();
    sim.tap_button(HOME);
    let (x0, y) = bar(Kind::Home, 0);
    sim.input(Ev::Touch(x0, y));
    sim.step_tick();
    assert_eq!(sim.console.brightness(), 0);
    for step in 1..5 {
        sim.input(Ev::Touch(bar(Kind::Home, step).0, y));
        sim.step_tick();
        assert_eq!(sim.console.brightness(), step);
    }
    sim.input(Ev::Lift);
    sim.idle(3);
    assert_eq!(sim.console.brightness(), 4);
    assert_eq!(sim.board.games.started, 0);
}

#[test]
fn the_open_menu_sleeps_and_sends_nothing() {
    let mut sim = home();
    sim.tap_button(HOME);
    sim.idle(10);
    let before = sim.ticks.len();
    sim.idle(30);
    assert!(sim.ticks[before..].iter().all(|t| t.presents == 0));
    // The last wake waits for input (or the bar's next minute).
    let w = sim.console.step(&mut *sim.board);
    assert!(matches!(w, Wake::InputOr(_)), "{w:?}");
}

#[test]
fn notices_behave_as_before_with_the_menu_open() {
    let mut sim = home();
    sim.tap_button(HOME);
    sim.input(Ev::Pad(false));
    sim.step_tick();
    sim.idle(8);
    assert_eq!(sim.console.notices().current(), Some("PAD DISCONNECTED"));
    // Drawn on top of everything, at its usual place.
    assert_ne!(sim.pixel(80, NOTE_Y as u16 + 2), sim.pixel(80, 1));
    assert!(sim.console.menu_open());
    // It leaves by itself, as always.
    sim.idle(4 * 30);
    assert_eq!(sim.console.notices().current(), None);
    assert!(sim.console.menu_open());
    // A notice from outside too.
    assert!(sim.notify("HELLO"));
    sim.idle(8);
    assert_eq!(sim.console.notices().current(), Some("HELLO"));
}

#[test]
fn the_console_writes_no_setting_anywhere() {
    let mut sim = home();
    sim.tap_button(HOME);
    sim.tap_button(LEFT);
    sim.tap_button(LEFT);
    assert_eq!(sim.console.brightness(), 2);
    assert!(sim.board.store.bytes.iter().all(|&b| b == 0xFF));
}

// ---- The menu over a game -------------------------------------------------------

thread_local! {
    static CALLS: Cell<u32> = const { Cell::new(0) };
}

mod counter {
    use super::*;

    /// Counts its calls; runs for ever.
    pub struct Counter;

    impl Game for Counter {
        fn start(_ctx: &mut Ctx) -> Self {
            Counter
        }

        fn update(&mut self, _ctx: &mut Ctx) -> Flow {
            CALLS.with(|c| c.set(c.get() + 1));
            Flow::Run
        }
    }

    game_kit::game!(Counter);
}

// SAFETY: made by `game_kit::game!`.
static COUNTER: &[LinkedGame] = &[unsafe { LinkedGame::new("BLOCKS", counter::game_entry) }];

fn in_game() -> Sim {
    CALLS.with(|c| c.set(0));
    let mut sim = Sim::with_games(Some(FOUR), COUNTER);
    sim.boot();
    sim.idle(2);
    sim.tap_button(SOUTH);
    sim.idle(2);
    assert_eq!(sim.console.page(), Page::Game);
    sim
}

fn calls() -> u32 {
    CALLS.with(|c| c.get())
}

#[test]
fn boot_alone_still_opens_and_resumes() {
    let mut sim = in_game();
    boot_press(&mut sim);
    assert!(sim.console.game().paused());
    assert_eq!(sim.console.game().menu_item(), 0, "RESUME is chosen");
    boot_press(&mut sim);
    assert!(!sim.console.game().paused());
    assert_eq!(sim.console.brightness(), 4);
    assert_eq!(sim.board.screen.backlight_calls, 1);
}

#[test]
fn boot_and_a_touch_change_the_step_in_a_game() {
    let mut sim = in_game();
    boot_press(&mut sim);
    let n = calls();
    let (x, y) = bar(Kind::Game, 1);
    sim.tap_at(x, y);
    assert_eq!(sim.console.brightness(), 1);
    assert_eq!(levels(&sim), [255, BACKLIGHT[1]]);
    assert!(sim.console.game().paused(), "the menu stays");
    assert_eq!(sim.console.game().menu_item(), 1);
    sim.idle(30);
    assert_eq!(calls(), n, "the game is not called while the menu shows");
    // BOOT now steps the level (the BRIGHTNESS line is chosen)...
    boot_press(&mut sim);
    assert_eq!(sim.console.brightness(), 2);
    // ...and a touch on RESUME goes back.
    let (x, y) = item(Kind::Game, 0);
    sim.tap_at(x, y);
    assert!(!sim.console.game().paused());
    sim.idle(3);
    assert!(calls() > n);
    assert!(sim.console.take_settings_changed());
}

#[test]
fn the_pad_changes_the_step_in_a_game() {
    let mut sim = in_game();
    sim.tap_button(HOME);
    sim.tap_button(LEFT);
    assert_eq!(sim.console.brightness(), 4, "RESUME is chosen");
    sim.tap_button(DOWN);
    sim.tap_button(LEFT);
    sim.tap_button(LEFT);
    assert_eq!(sim.console.brightness(), 2);
    assert_eq!(levels(&sim), [255, BACKLIGHT[3], BACKLIGHT[2]]);
    sim.tap_button(UP);
    sim.tap_button(SOUTH);
    assert!(!sim.console.game().paused());
    // The step stays after the game.
    sim.tap_button(HOME);
    sim.tap_button(DOWN);
    sim.tap_button(DOWN);
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Home);
    sim.idle(10);
    assert_eq!(sim.console.brightness(), 2);
    assert_eq!(sim.board.screen.backlight_calls, 3);
}

#[test]
fn a_finger_sliding_on_the_bars_is_followed_every_frame_in_a_game() {
    let mut sim = in_game();
    boot_press(&mut sim);
    let (x0, y) = bar(Kind::Game, 4);
    sim.input(Ev::Touch(x0, y));
    sim.step_tick();
    // The finger moves with no new board wake: the console looks again
    // by itself while the finger is live.
    let (x1, _) = bar(Kind::Game, 1);
    sim.board.touch.at = Some(board_api::TouchPoint {
        x: x1 * 2 + 1,
        y: y * 2 + 1,
    });
    sim.step_tick();
    sim.step_tick();
    assert_eq!(sim.console.brightness(), 1);
    sim.input(Ev::Lift);
    sim.idle(2);
    assert!(sim.console.game().paused());
}

#[test]
fn the_home_menu_is_closed_in_a_game_and_after_it() {
    let mut sim = in_game();
    assert!(!sim.console.menu_open());
    sim.tap_button(HOME);
    assert!(sim.console.game().paused());
    assert!(!sim.console.menu_open(), "the game has its own menu");
    sim.tap_button(DOWN);
    sim.tap_button(DOWN);
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Home);
    assert!(!sim.console.menu_open());
}
