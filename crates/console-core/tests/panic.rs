//! A game that panics, on the PC: everything the PC can prove.
//!
//! On the PC the game kit catches the panic, drops the game's state and
//! reports "aborted". These tests check what the console does then: the
//! notice, its own state, the game's memory, and that a second game
//! starts as if the first had never run.
//!
//! (On the chip a panic cannot be caught this way. What the chip's board
//! must do so that a panic there also comes back home is written down in
//! `crates/console-core/README.md`, "A game that panics".)

mod common;

use std::cell::{Cell, RefCell};

use common::{DOWN, FOUR, HOME, SOUTH, Sim};
use console_core::game_api::{LinkedGame, picture};
use console_core::{Page, msg};
use game_kit::{Ctx, Flow, Game, rate};

/// Where the game should panic.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Boom {
    Never,
    InStart,
    InUpdate,
    InExit,
}

/// What one call saw: (run, calls of this state, frame, time, started).
type Call = (u32, u32, u32, u64, bool);

thread_local! {
    static BOOM: Cell<Boom> = const { Cell::new(Boom::Never) };
    /// How many states were made and dropped.
    static MADE: Cell<u32> = const { Cell::new(0) };
    static DROPPED: Cell<u32> = const { Cell::new(0) };
    static CALLS: RefCell<Vec<Call>> = const { RefCell::new(Vec::new()) };
}

fn reset() {
    BOOM.with(|b| b.set(Boom::Never));
    MADE.with(|m| m.set(0));
    DROPPED.with(|d| d.set(0));
    CALLS.with(|c| c.borrow_mut().clear());
}

fn boom(at: Boom) {
    BOOM.with(|b| b.set(at));
}

fn made() -> u32 {
    MADE.with(Cell::get)
}

fn dropped() -> u32 {
    DROPPED.with(Cell::get)
}

fn calls() -> Vec<Call> {
    CALLS.with(|c| c.borrow().clone())
}

mod bomb {
    use super::*;

    /// A game with state: which run it is, and how often it was called.
    /// A second run that went on with the first run's state would show
    /// a count that does not start at 1.
    pub struct Bomb {
        run: u32,
        calls: u32,
        /// Some memory, as a real game has.
        _junk: [u8; 64],
    }

    impl Drop for Bomb {
        fn drop(&mut self) {
            DROPPED.with(|d| d.set(d.get() + 1));
        }
    }

    impl Game for Bomb {
        fn start(ctx: &mut Ctx) -> Self {
            if BOOM.with(Cell::get) == Boom::InStart {
                boom(Boom::Never);
                panic!("boom in start");
            }
            MADE.with(|m| m.set(m.get() + 1));
            // Its own colours and rate, which must not outlive it.
            for i in 0..picture::GAME_COLOURS as u8 {
                ctx.set_colour(i, 0xF81F);
            }
            ctx.screen().clear(7);
            // Only the first run asks for 60 a second; a later run
            // must then get the default (30) again.
            if MADE.with(Cell::get) == 1 {
                let _ = ctx.set_rate(rate::FPS60);
            }
            Bomb {
                run: MADE.with(Cell::get),
                calls: 0,
                _junk: [0xAA; 64],
            }
        }

        fn update(&mut self, ctx: &mut Ctx) -> Flow {
            self.calls += 1;
            CALLS.with(|c| {
                c.borrow_mut().push((
                    self.run,
                    self.calls,
                    ctx.frame(),
                    ctx.time_us(),
                    ctx.started(),
                ))
            });
            if BOOM.with(Cell::get) == Boom::InUpdate {
                boom(Boom::Never);
                panic!("boom in update");
            }
            Flow::Run
        }

        fn exit(&mut self, _ctx: &mut Ctx, _why: game_kit::Exit) {
            if BOOM.with(Cell::get) == Boom::InExit {
                boom(Boom::Never);
                panic!("boom in exit");
            }
        }
    }

    game_kit::game!(Bomb);
}

// SAFETY: made by `game_kit::game!`.
static BOMB: &[LinkedGame] = &[unsafe { LinkedGame::new("BLOCKS", bomb::game_entry) }];

/// Home, settled, with the bomb on the card.
fn home() -> Sim {
    reset();
    let mut sim = Sim::with_games(Some(FOUR), BOMB);
    sim.boot();
    sim.idle(30);
    sim
}

fn log_has(sim: &Sim, text: &str) -> bool {
    sim.board.log.lines().iter().any(|l| l.contains(text))
}

/// What a fresh start must look like: a new state (count 1, the newest
/// run), frame 0, time 0, "started". `n`: calls seen before the start.
fn assert_fresh_start(sim: &Sim, run: u32, n: usize) {
    assert_eq!(sim.console.page(), Page::Game);
    let first = calls()[n];
    assert_eq!(
        first,
        (run, 1, 0, 0, true),
        "(run, calls, frame, time, started)"
    );
    // The start colours again, not what the last run left.
    let pal = sim.console.video().palette();
    for i in [0u8, 7, 100, 239] {
        assert_eq!(pal.get(i), 0xF81F, "set by the new run's start");
    }
    assert_eq!(sim.pixel(80, 60), 7);
}

#[test]
fn a_panic_in_update_goes_home_with_a_notice_and_drops_the_game() {
    let mut sim = home();
    let palette = *sim.console.video().palette().colours();
    sim.tap_button(SOUTH);
    let n = calls().len();
    sim.idle(5);
    assert_eq!(made(), 1);
    assert_eq!(calls().len() - n, 10, "the first run runs at 60 a second");
    boom(Boom::InUpdate);
    sim.idle(1);

    // The notice and the log.
    assert_eq!(sim.console.page(), Page::Home);
    assert_eq!(sim.console.notices().current(), Some(msg::GAME_ABORTED));
    assert!(log_has(&sim, "game: panic: boom in update"));
    assert!(log_has(&sim, "console: game stopped with an error"));
    // The console's state: no game, nothing paused, the board told.
    assert!(!sim.console.game().running());
    assert!(!sim.console.game().paused());
    assert_eq!(sim.board.games.ended, 1);
    // The game's memory is gone: its state was dropped, once.
    assert_eq!((made(), dropped()), (1, 1));
    // Home reads its list again (the game had its RAM): nothing is
    // drawn in this step and the next; then home's own colours are back
    // (the notice uses the console's).
    assert!(sim.console.home_pending());
    sim.idle(2);
    assert!(!sim.console.home_pending());
    let after = sim.console.video().palette().colours();
    assert!(after[..240] == palette[..240], "the game's colours stayed");

    // Home still works, and the same game starts as new.
    sim.idle(60);
    let n = calls().len();
    sim.tap_button(SOUTH);
    assert_fresh_start(&sim, 2, n);
    assert_eq!(sim.board.games.started, 2);
    // At the default rate (the first run had asked for 60 a second).
    let n = calls().len();
    sim.idle(30);
    assert_eq!(calls().len() - n, 30, "30 calls in 30 frames");
}

#[test]
fn a_panic_in_the_first_call_goes_home_and_leaves_nothing() {
    let mut sim = home();
    boom(Boom::InStart);
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Home);
    assert_eq!(sim.console.notices().current(), Some(msg::GAME_ABORTED));
    assert!(log_has(&sim, "game: panic: boom in start"));
    assert_eq!((made(), dropped()), (0, 0), "no state was ever made");
    assert_eq!(sim.board.games.ended, 1);
    sim.idle(60);
    let n = calls().len();
    sim.tap_button(SOUTH);
    assert_fresh_start(&sim, 1, n);
}

#[test]
fn a_panic_in_exit_still_goes_home_and_the_next_start_is_clean() {
    let mut sim = home();
    sim.tap_button(SOUTH);
    sim.idle(2);
    boom(Boom::InExit);
    sim.tap_button(HOME);
    sim.tap_button(DOWN);
    sim.tap_button(DOWN); // past BRIGHTNESS
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Home);
    assert!(log_has(&sim, "game: panic: boom in exit"));
    // The state was taken out before `exit` ran, and dropped once.
    assert_eq!((made(), dropped()), (1, 1));
    assert_eq!(sim.board.games.ended, 1);
    sim.idle(30);
    let n = calls().len();
    sim.tap_button(SOUTH);
    assert_fresh_start(&sim, 2, n);
}
