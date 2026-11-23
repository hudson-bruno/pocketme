//! Every press counts once: seeded random runs that count the presses a
//! board makes against the presses the console and a game see.
//!
//! The board here behaves like a real one that notes presses: at every
//! press (a button going down) it sets its note, and the console reads
//! the note and the buttons held now together, once per frame. Presses
//! come at random: some last many frames, some start and end between two
//! frames, some let go and press again between two frames. A button is
//! pressed at most once between two frames (two presses that close
//! together count as one: the note is one bit per button).
//!
//! Written by the second reader of step 3.

mod common;

use std::cell::RefCell;

use common::{Ev, FOUR, NINE, SOUTH, Sim};
use console_core::Page;
use console_core::game_api::{Input, LinkedGame, input_flag};
use console_core::gamein::GAME_BUTTONS;
use game_kit::{Ctx, Flow, Game, rate};

// ---- A game that writes down every input it gets -------------------------

thread_local! {
    static SEEN: RefCell<Vec<Input>> = const { RefCell::new(Vec::new()) };
    static RATE: RefCell<u16> = const { RefCell::new(rate::FPS30) };
}

mod counter {
    use super::*;

    pub struct Counter;

    impl Game for Counter {
        fn start(_ctx: &mut Ctx) -> Self {
            Counter
        }

        fn update(&mut self, ctx: &mut Ctx) -> Flow {
            if ctx.started() {
                let _ = ctx.set_rate(RATE.with(|r| *r.borrow()));
            }
            SEEN.with(|s| s.borrow_mut().push(*ctx.input()));
            Flow::Run
        }
    }

    game_kit::game!(Counter);
}

// SAFETY: made by `game_kit::game!`.
static COUNTER: &[LinkedGame] = &[unsafe { LinkedGame::new("BLOCKS", counter::game_entry) }];

fn seen() -> Vec<Input> {
    SEEN.with(|s| s.borrow().clone())
}

/// A small fixed random number maker (xorshift).
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x % n
    }
}

/// What a button does between two frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Move {
    Nothing,
    /// Goes down and stays down.
    Down,
    /// Goes up and stays up.
    Up,
    /// Down and up again, both before the next frame.
    DownUp,
    /// Up and down again, both before the next frame.
    UpDown,
}

/// A random move for a button that is `down` now. `short`: whether moves
/// shorter than a frame are allowed.
fn pick(rng: &mut Rng, down: bool, short: bool) -> Move {
    match (down, rng.below(10)) {
        (false, 7) => Move::Down,
        (false, 8 | 9) if short => Move::DownUp,
        (false, 8 | 9) => Move::Down,
        (true, 7 | 8) => Move::Up,
        (true, 9) if short => Move::UpDown,
        (true, 9) => Move::Up,
        _ => Move::Nothing,
    }
}

/// Counts of edges the board made.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
struct Made {
    downs: u32,
    ups: u32,
}

/// The board makes one move of pad button `b`, as a real board that
/// notes presses would show it. `notes`: false for a board that only
/// knows "held now".
fn apply(sim: &mut Sim, b: u32, m: Move, notes: bool, made: &mut Made) {
    let note = |sim: &mut Sim| {
        if notes {
            sim.input(Ev::Quick(b));
        }
    };
    match m {
        Move::Nothing => {}
        Move::Down => {
            sim.input(Ev::Press(b));
            note(sim);
            made.downs += 1;
        }
        Move::Up => {
            sim.input(Ev::Release(b));
            made.ups += 1;
        }
        Move::DownUp => {
            note(sim);
            made.downs += 1;
            made.ups += 1;
        }
        Move::UpDown => {
            sim.input(Ev::Release(b));
            sim.input(Ev::Press(b));
            note(sim);
            made.downs += 1;
            made.ups += 1;
        }
    }
}

/// Boots, starts the counter game with a short press, and lets go.
fn start_game(game_rate: u16) -> Sim {
    SEEN.with(|s| s.borrow_mut().clear());
    RATE.with(|r| *r.borrow_mut() = game_rate);
    let mut sim = Sim::with_games(Some(FOUR), COUNTER);
    sim.boot();
    sim.idle(2);
    sim.input(Ev::Quick(SOUTH));
    sim.step_tick();
    assert_eq!(sim.console.page(), Page::Game);
    sim.idle(3);
    SEEN.with(|s| s.borrow_mut().clear());
    sim
}

fn count(inputs: &[Input], bit: u32) -> (u32, u32) {
    let pressed = inputs
        .iter()
        .filter(|i| i.players[0].pressed & bit != 0)
        .count() as u32;
    let released = inputs
        .iter()
        .filter(|i| i.players[0].released & bit != 0)
        .count() as u32;
    (pressed, released)
}

fn touch_count(inputs: &[Input]) -> (u32, u32) {
    let s = inputs
        .iter()
        .filter(|i| i.flags & input_flag::TOUCH_START != 0)
        .count() as u32;
    let e = inputs
        .iter()
        .filter(|i| i.flags & input_flag::TOUCH_END != 0)
        .count() as u32;
    (s, e)
}

/// Every pad press and every touch reaches the game exactly once, and
/// every release too, at 30 and 60 frames a second and on demand.
#[test]
fn every_press_reaches_a_game_exactly_once() {
    for (seed, game_rate) in [
        (1u64, rate::FPS30),
        (2, rate::FPS60),
        (3, rate::ON_DEMAND),
        (4, rate::FPS30),
        (5, rate::ON_DEMAND),
    ] {
        let mut sim = start_game(game_rate);
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ seed);
        let mut made = [Made::default(); 12];
        let mut down = [false; 12];
        let mut touch_made = Made::default();
        let mut touching = false;
        for _ in 0..800 {
            for (k, &(pad_bit, _)) in GAME_BUTTONS.iter().enumerate() {
                let m = pick(&mut rng, down[k], true);
                apply(&mut sim, pad_bit, m, true, &mut made[k]);
                down[k] = matches!(m, Move::Down | Move::UpDown)
                    || (down[k] && matches!(m, Move::Nothing));
            }
            // The touch screen, at one place, the same way.
            let m = pick(&mut rng, touching, true);
            match m {
                Move::Nothing => {}
                Move::Down => {
                    sim.input(Ev::Touch(50, 60));
                    sim.input(Ev::QuickTouch(50, 60));
                }
                Move::Up => sim.input(Ev::Lift),
                Move::DownUp => sim.input(Ev::QuickTouch(50, 60)),
                Move::UpDown => {
                    sim.input(Ev::Lift);
                    sim.input(Ev::Touch(50, 60));
                    sim.input(Ev::QuickTouch(50, 60));
                }
            }
            match m {
                Move::Down => touch_made.downs += 1,
                Move::Up => touch_made.ups += 1,
                Move::DownUp | Move::UpDown => {
                    touch_made.downs += 1;
                    touch_made.ups += 1;
                }
                Move::Nothing => {}
            }
            touching = matches!(m, Move::Down | Move::UpDown) || (touching && m == Move::Nothing);
            sim.step_tick();
            assert_eq!(sim.console.page(), Page::Game, "seed {seed}");
            assert!(!sim.console.game().paused(), "seed {seed}");
        }
        // Let everything go, so every release is seen too.
        for (k, &(pad_bit, _)) in GAME_BUTTONS.iter().enumerate() {
            if down[k] {
                apply(&mut sim, pad_bit, Move::Up, true, &mut made[k]);
            }
        }
        if touching {
            sim.input(Ev::Lift);
            touch_made.ups += 1;
        }
        sim.idle(3);
        let inputs = seen();
        for (k, &(_, game_bit)) in GAME_BUTTONS.iter().enumerate() {
            assert!(made[k].downs > 50, "the script presses enough");
            assert_eq!(
                count(&inputs, game_bit),
                (made[k].downs, made[k].ups),
                "seed {seed}, rate {game_rate}, game button {game_bit:#x}"
            );
        }
        assert_eq!(
            touch_count(&inputs),
            (touch_made.downs, touch_made.ups),
            "seed {seed}, rate {game_rate}: touches"
        );
    }
}

/// A board that only knows "held now" and one that also notes presses
/// behave exactly the same when every press and every release lasts at
/// least a frame: same game inputs, same pictures. (So the notes never
/// add a second press, and a board without notes works as before.)
#[test]
fn notes_change_nothing_when_presses_are_long() {
    for game_rate in [rate::FPS30, rate::ON_DEMAND] {
        let mut runs = Vec::new();
        for notes in [false, true] {
            let mut sim = start_game(game_rate);
            let mut rng = Rng(0xD1CE_5EED);
            let mut down = [false; 12];
            let mut made = [Made::default(); 12];
            for _ in 0..600 {
                for (k, &(pad_bit, _)) in GAME_BUTTONS.iter().enumerate() {
                    let m = pick(&mut rng, down[k], false);
                    apply(&mut sim, pad_bit, m, notes, &mut made[k]);
                    down[k] = m == Move::Down || (down[k] && m == Move::Nothing);
                }
                sim.step_tick();
            }
            let prints: Vec<u64> = sim.ticks.iter().map(|t| t.print).collect();
            runs.push((seen(), prints));
        }
        assert!(runs[0].0.len() > 100);
        assert_eq!(runs[0], runs[1], "rate {game_rate}");
    }
}

/// On the home screen every press of right and left moves the choice
/// exactly once, short or long, and a press during the boot screen does
/// not reach home at all.
#[test]
fn every_press_moves_the_home_choice_exactly_once() {
    use board_api::btn;
    for seed in 1..=4u64 {
        let mut sim = Sim::new(Some(NINE));
        let mut rng = Rng(0xA5A5_0000_1234 ^ seed);
        // A press during the boot screen ends it, and is not home's.
        sim.idle(3);
        let m = if seed % 2 == 0 {
            Move::DownUp
        } else {
            Move::Down
        };
        apply(&mut sim, btn::RIGHT, m, true, &mut Made::default());
        sim.step_tick();
        assert_eq!(sim.console.page(), Page::Home);
        if m == Move::Down {
            sim.input(Ev::Release(btn::RIGHT));
        }
        sim.idle(3);
        assert_eq!(sim.console.home().selected(), 0, "seed {seed}: boot press");

        let mut want: usize = 0;
        for _ in 0..120 {
            // One of the two arrows, held 0 (shorter than a frame) to 8
            // frames: under the key-repeat delay (12 frames).
            let (b, step): (u32, i32) = if rng.below(2) == 0 {
                (btn::RIGHT, 1)
            } else {
                (btn::LEFT, -1)
            };
            let frames = rng.below(9);
            if frames == 0 {
                apply(&mut sim, b, Move::DownUp, true, &mut Made::default());
                sim.step_tick();
            } else {
                apply(&mut sim, b, Move::Down, true, &mut Made::default());
                sim.idle(frames);
                sim.input(Ev::Release(b));
                sim.step_tick();
            }
            want = (want as i32 + step).clamp(0, 8) as usize;
            assert_eq!(sim.console.home().selected(), want, "seed {seed}");
            sim.idle(rng.below(3));
        }
    }
}

/// In the pause menu every press of up and down moves the choice
/// exactly once; none of them reaches the game; and the board's BOOT
/// button opens and closes the menu once per press.
#[test]
fn pause_menu_presses_count_once_and_never_reach_the_game() {
    use board_api::btn;
    for seed in 1..=3u64 {
        let mut sim = start_game(rate::FPS30);
        let mut rng = Rng(0x7777_0000 ^ seed);
        let mut pauses = 0;
        for _ in 0..40 {
            // Open the menu with BOOT: short, or held a few frames.
            let n_before = seen().len();
            if rng.below(2) == 0 {
                sim.input(Ev::QuickBoot);
                sim.step_tick();
            } else {
                sim.input(Ev::Boot(true));
                sim.input(Ev::QuickBoot);
                sim.idle(1 + rng.below(4));
                sim.input(Ev::Boot(false));
                sim.step_tick();
            }
            assert!(sim.console.game().paused(), "seed {seed}");
            pauses += 1;
            let mut want = 0u8;
            for _ in 0..rng.below(8) {
                let (b, up) = if rng.below(2) == 0 {
                    (btn::DOWN, false)
                } else {
                    (btn::UP, true)
                };
                let frames = rng.below(6);
                if frames == 0 {
                    apply(&mut sim, b, Move::DownUp, true, &mut Made::default());
                    sim.step_tick();
                } else {
                    apply(&mut sim, b, Move::Down, true, &mut Made::default());
                    sim.idle(frames);
                    sim.input(Ev::Release(b));
                    sim.step_tick();
                }
                want = if up {
                    want.saturating_sub(1)
                } else {
                    (want + 1).min(2)
                };
                assert_eq!(sim.console.game().menu_item(), want, "seed {seed}");
            }
            // Back to RESUME, then BOOT (it confirms in the menu) closes
            // it. (BOOT on BRIGHTNESS would change the step instead.)
            for _ in 0..want {
                apply(&mut sim, btn::UP, Move::DownUp, true, &mut Made::default());
                sim.step_tick();
            }
            assert_eq!(
                seen().len(),
                n_before,
                "the game is not called while paused"
            );
            sim.input(Ev::QuickBoot);
            sim.step_tick();
            assert!(
                !sim.console.game().paused(),
                "seed {seed}: one BOOT press resumes"
            );
            sim.idle(1 + rng.below(3));
        }
        assert_eq!(pauses, 40);
        assert_eq!(sim.console.page(), Page::Game);
        assert!(
            seen().iter().all(|i| i.players[0].pressed == 0
                && i.players[0].released == 0
                && i.players[0].held == 0),
            "seed {seed}: no menu press reached the game"
        );
    }
}
