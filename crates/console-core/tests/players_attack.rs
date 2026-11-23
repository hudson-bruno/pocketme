//! Hostile tests for the four players (the checker's, 2026-11-18).
//!
//! * Isolation: what player `n` sees is the same, call for call, whether
//!   the other three pads are idle or all mash at random (with pads
//!   coming and going in the middle of presses).
//! * Edges: in every call, for every player, `pressed` and `released`
//!   agree with how `held` changed, and a short press the board noted is
//!   seen once, never lost and never twice.
//! * Pause and leave while players 2 to 4 hold buttons.

mod common;

use std::cell::RefCell;

use board_api::{PadState, Stick, btn};
use common::{DOWN, Ev, FOUR, HOME, SOUTH, Sim};
use console_core::Page;
use console_core::game_api::{Input, PlayerInput, button, player_flag};
use console_core::gamein::GAME_BUTTONS;
use game_kit::{Ctx, Flow, Game};

thread_local! {
    static SEEN: RefCell<Vec<Input>> = const { RefCell::new(Vec::new()) };
}

fn seen() -> Vec<Input> {
    SEEN.with(|s| s.borrow().clone())
}

mod watcher {
    use super::*;

    pub struct Watcher;

    impl Game for Watcher {
        fn start(_: &mut Ctx) -> Self {
            Watcher
        }

        fn update(&mut self, ctx: &mut Ctx) -> Flow {
            // A game that asks for players that do not exist gets
            // nothing, and does not stop.
            for n in [4, 7, 255, usize::MAX] {
                assert_eq!(ctx.player(n), game_kit::Player::default());
                assert!(!ctx.has_pad(n));
            }
            SEEN.with(|s| s.borrow_mut().push(*ctx.input()));
            Flow::Run
        }
    }

    game_kit::game!(Watcher);
}

static WATCHER: &[console_core::game_api::LinkedGame] = &[
    // SAFETY: the kit's entry keeps the contract.
    unsafe { console_core::game_api::LinkedGame::new("BLOCKS", watcher::game_entry) },
];

/// A console on the home screen, BLOCKS (the watcher) selected.
fn home() -> Sim {
    SEEN.with(|s| s.borrow_mut().clear());
    let mut sim = Sim::with_games(Some(FOUR), WATCHER);
    sim.boot();
    sim.idle(2);
    sim
}

fn start() -> Sim {
    let mut sim = home();
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Game);
    sim.idle(2);
    sim
}

/// Pad `btn` bits as game `button` bits (the console's one table).
fn game_bits(pad: u32) -> u32 {
    GAME_BUTTONS
        .iter()
        .filter(|(p, _)| pad & p != 0)
        .fold(0, |acc, (_, g)| acc | g)
}

/// A small xorshift: the same numbers every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }

    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
}

/// The buttons a random script may use on pad `n`. Pad 0 never uses
/// Home (that would open the console's pause menu, not the game's
/// input); the others do, and it must change nothing.
fn script_buttons(n: u8) -> u32 {
    let all = btn::UP
        | btn::DOWN
        | btn::LEFT
        | btn::RIGHT
        | btn::SOUTH
        | btn::EAST
        | btn::WEST
        | btn::NORTH
        | btn::L
        | btn::R
        | btn::ZL
        | btn::ZR
        | btn::PLUS
        | btn::MINUS
        | btn::CAPTURE;
    if n == 0 { all } else { all | btn::HOME }
}

/// One tick of pad `n`'s random script, applied straight to the fake
/// board. Pad 0 stays connected (it drives the console); pads 1 to 3
/// come and go, sometimes in the middle of a press.
fn script_tick(sim: &mut Sim, n: u8, rng: &mut Rng) {
    let mask = script_buttons(n);
    let r = rng.below(16);
    let pad = &mut sim.board.pad;
    let (state, pressed): (&mut PadState, &mut u32) = if n == 0 {
        (&mut pad.state, &mut pad.pressed)
    } else {
        let p = pad.other(n);
        (&mut p.state, &mut p.pressed)
    };
    match r {
        0..=3 => state.buttons |= rng.next() & mask,
        4..=6 => state.buttons &= !(rng.next() & mask),
        7..=8 => *pressed |= rng.next() & mask,
        9 => {
            state.left = Stick {
                x: rng.next() as i16,
                y: rng.next() as i16,
            }
        }
        10 => state.lt = (rng.next() % 1024) as u16,
        11 if n != 0 => {
            // Gone, or back (holding whatever it held, or nothing).
            if state.connected {
                state.connected = false;
            } else {
                *state = PadState {
                    connected: true,
                    buttons: if rng.below(2) == 0 {
                        rng.next() & mask
                    } else {
                        0
                    },
                    ..PadState::NONE
                };
            }
        }
        _ => {}
    }
}

/// Runs `ticks` ticks of random scripts on the pads in `active` (seed
/// per pad, so pad `n` does the same in every run), and returns what
/// the game saw.
fn mash(active: &[u8], ticks: u32, check: bool) -> Vec<Input> {
    let mut sim = start();
    // Pads 1 to 3 that take part start plugged in, nothing held.
    for &n in active {
        if n != 0 {
            sim.input(Ev::PadN(n, true));
        }
    }
    let mut rngs: Vec<Rng> = (0..4u64)
        .map(|n| Rng(0x1234_5678_9ABC + n * 7919))
        .collect();
    let from = seen().len();
    let mut last_len = from;
    for _ in 0..ticks {
        let mut quick = [0u32; 4];
        for &n in active {
            let before = if n == 0 {
                sim.board.pad.pressed
            } else {
                sim.board.pad.other(n).pressed
            };
            script_tick(&mut sim, n, &mut rngs[usize::from(n)]);
            let after = if n == 0 {
                sim.board.pad.pressed
            } else {
                sim.board.pad.other(n).pressed
            };
            quick[usize::from(n)] = after & !before;
        }
        sim.input(Ev::Lift); // marks the input as changed
        sim.step_tick();
        assert_eq!(sim.console.page(), Page::Game, "the game still runs");
        assert!(!sim.console.game().paused(), "nobody paused it");
        let all = seen();
        assert_eq!(all.len(), last_len + 1, "one call per tick at 30 fps");
        last_len = all.len();
        if check {
            let now = all[all.len() - 1];
            let was = all[all.len() - 2];
            for n in 0..4u8 {
                let state = if n == 0 {
                    sim.board.pad.state
                } else {
                    sim.board.pad.other(n).state
                };
                let p = now.players[usize::from(n)];
                let w = was.players[usize::from(n)];
                check_edges(n, &w, &p);
                if state.connected {
                    assert_eq!(p.flags & player_flag::PAD, player_flag::PAD);
                    assert_eq!(p.held, game_bits(state.buttons), "player {} held", n + 1);
                    let q = game_bits(quick[usize::from(n)]);
                    assert_eq!(p.pressed & q, q, "player {}: a short press is seen", n + 1);
                } else {
                    assert_eq!(p.flags, 0);
                    assert_eq!(p.held, 0);
                    assert_eq!(p.pressed, 0);
                    assert_eq!(p.released, w.held, "a pad that goes lets go of all");
                }
            }
        }
    }
    seen()[from..].to_vec()
}

/// `pressed` and `released` agree with how `held` changed: a press alone
/// means up before and down now, a release alone the other way, neither
/// means no change. Both together (a short press) may end either way.
fn check_edges(n: u8, was: &PlayerInput, now: &PlayerInput) {
    for bit in 0..12 {
        let b = 1u32 << bit;
        let (p, r) = (now.pressed & b != 0, now.released & b != 0);
        let (h0, h1) = (was.held & b != 0, now.held & b != 0);
        match (p, r) {
            (true, false) => assert!(!h0 && h1, "player {} bit {bit}: press", n + 1),
            (false, true) => assert!(h0 && !h1, "player {} bit {bit}: release", n + 1),
            (false, false) => assert_eq!(h0, h1, "player {} bit {bit}: no edge", n + 1),
            (true, true) => {}
        }
    }
    assert_eq!(now.held & !button::ALL, 0);
    assert_eq!(now.pressed & !button::ALL, 0);
    assert_eq!(now.released & !button::ALL, 0);
}

#[test]
fn four_players_mashing_at_once_never_leak_into_each_other() {
    const TICKS: u32 = 600;
    let all = mash(&[0, 1, 2, 3], TICKS, true);
    for n in 0..4u8 {
        let alone = mash(&[n], TICKS, false);
        assert_eq!(alone.len(), all.len());
        for (t, (a, b)) in alone.iter().zip(&all).enumerate() {
            assert_eq!(
                a.players[usize::from(n)],
                b.players[usize::from(n)],
                "player {} differs at call {t} when the others mash",
                n + 1
            );
        }
        // And something did happen to this player.
        assert!(alone.iter().any(|i| i.players[usize::from(n)].pressed != 0));
    }
}

#[test]
fn leaving_and_resuming_while_players_2_to_4_hold_buttons() {
    let mut sim = start();
    for n in 1..=3 {
        sim.input(Ev::PadN(n, true));
    }
    sim.idle(1);
    // Players 2 to 4 hold buttons; player 1 pauses.
    sim.input(Ev::PressN(1, btn::SOUTH));
    sim.input(Ev::PressN(2, btn::PLUS | btn::LEFT));
    sim.input(Ev::PressN(3, btn::EAST));
    sim.idle(1);
    sim.tap_button(HOME);
    assert!(sim.console.game().paused());
    // While paused they let go and press other things, and short presses
    // come in: none of it is the game's.
    sim.input(Ev::ReleaseN(2, u32::MAX));
    sim.input(Ev::PressN(2, btn::NORTH));
    sim.input(Ev::QuickN(3, btn::WEST));
    sim.idle(3);
    // Resume (player 1's confirm on RESUME).
    let n = seen().len();
    sim.tap_button(SOUTH);
    assert!(!sim.console.game().paused());
    sim.idle(1);
    let after = &seen()[n..];
    assert!(!after.is_empty());
    for i in after {
        assert_eq!(i.players[1].held, 0, "held on resume: hidden");
        assert_eq!(i.players[2].held, 0, "pressed while paused: hidden");
        assert_eq!(i.players[2].pressed, 0);
        assert_eq!(i.players[3].held, 0);
        assert_eq!(
            i.players[3].pressed, 0,
            "a short press while paused is not the game's"
        );
    }
    // A hidden button let go and pressed again counts.
    sim.input(Ev::ReleaseN(2, u32::MAX));
    sim.idle(1);
    assert_eq!(seen().last().unwrap().players[2].released, 0);
    sim.input(Ev::PressN(2, btn::NORTH));
    sim.idle(1);
    assert_eq!(seen().last().unwrap().players[2].pressed, button::Y);

    // Now leave while they all hold: pause, down twice, confirm.
    sim.input(Ev::PressN(1, btn::R));
    sim.input(Ev::PressN(3, btn::PLUS));
    sim.idle(1);
    sim.tap_button(HOME);
    sim.tap_button(DOWN);
    sim.tap_button(DOWN);
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Home);
    // Player 2 to 4's buttons did not move home's selection or start
    // anything while home is read again.
    sim.input(Ev::QuickN(1, btn::SOUTH | btn::RIGHT));
    sim.idle(5);
    assert_eq!(sim.console.page(), Page::Home);
    // Start again with them still holding: hidden at the start.
    let n = seen().len();
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Game);
    sim.idle(2);
    for i in &seen()[n..] {
        for p in 1..4 {
            assert_eq!(i.players[p].held, 0, "player {} at start", p + 1);
            assert_eq!(i.players[p].pressed, 0, "player {} at start", p + 1);
        }
    }
}

#[test]
fn a_pad_that_blinks_out_and_back_between_two_steps_is_not_a_new_press() {
    let mut sim = start();
    sim.input(Ev::PadN(2, true));
    sim.input(Ev::PressN(2, btn::SOUTH));
    sim.idle(1);
    assert_eq!(seen().last().unwrap().players[2].pressed, button::A);
    // Gone and back before the console looked, still holding A: the
    // console never saw it go, so nothing changes for the game.
    sim.input(Ev::PadN(2, false));
    sim.input(Ev::PadN(2, true));
    sim.input(Ev::PressN(2, btn::SOUTH));
    sim.idle(1);
    let p = seen().last().unwrap().players[2];
    assert_eq!((p.held, p.pressed, p.released), (button::A, 0, 0));
    // Gone for one step: released, then a new press when it is back.
    sim.input(Ev::PadN(2, false));
    sim.idle(1);
    assert_eq!(seen().last().unwrap().players[2].released, button::A);
    sim.input(Ev::PadN(2, true));
    sim.input(Ev::PressN(2, btn::SOUTH));
    sim.idle(1);
    let p = seen().last().unwrap().players[2];
    assert_eq!((p.held, p.pressed, p.released), (button::A, button::A, 0));
}

#[test]
fn short_presses_on_absent_pads_are_dropped_and_do_not_pile_up() {
    let mut sim = start();
    // Pad 3 is not there: its noted presses are taken (and dropped) every
    // step, so they cannot all arrive at once when it connects.
    for _ in 0..10 {
        sim.board.pad.other(3).pressed |= btn::SOUTH;
        sim.idle(1);
    }
    assert!(seen().iter().all(|i| i.players[3] == PlayerInput::NONE));
    sim.input(Ev::PadN(3, true));
    sim.idle(1);
    assert_eq!(seen().last().unwrap().players[3].pressed, 0);
}
