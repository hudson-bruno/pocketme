//! Four players: each board pad is one game player, with its own held,
//! pressed and released bits. Nothing leaks from one player to another,
//! a pad that goes away lets go of its buttons, and only pad 0 (player
//! 1) drives the console's menus.

mod common;

use std::cell::RefCell;

use board_api::{Motion, btn};
use common::{Ev, FOUR, HOME, SOUTH, Sim};
use console_core::game_api::{Input, PlayerInput, button, player_flag};
use console_core::{Page, msg};
use game_kit::{Ctx, Flow, Game, rate};

thread_local! {
    static SEEN: RefCell<Vec<Input>> = const { RefCell::new(Vec::new()) };
    static ON_DEMAND: RefCell<bool> = const { RefCell::new(false) };
}

fn seen() -> Vec<Input> {
    SEEN.with(|s| s.borrow().clone())
}

fn last() -> Input {
    *seen().last().expect("a call")
}

fn calls() -> usize {
    SEEN.with(|s| s.borrow().len())
}

mod watcher {
    use super::*;

    pub struct Watcher;

    impl Game for Watcher {
        fn start(ctx: &mut Ctx) -> Self {
            if ON_DEMAND.with(|o| *o.borrow()) {
                let _ = ctx.set_rate(rate::ON_DEMAND);
            }
            Watcher
        }

        fn update(&mut self, ctx: &mut Ctx) -> Flow {
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

fn start(on_demand: bool) -> Sim {
    SEEN.with(|s| s.borrow_mut().clear());
    ON_DEMAND.with(|o| *o.borrow_mut() = on_demand);
    let mut sim = Sim::with_games(Some(FOUR), WATCHER);
    sim.boot();
    sim.idle(2);
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Game);
    sim.idle(2);
    sim
}

/// Board pad `n` (1 to 3) plugged in, nothing held.
fn plug(sim: &mut Sim, n: u8) {
    sim.input(Ev::PadN(n, true));
}

fn connected(i: &Input) -> [bool; 4] {
    i.players.map(|p| p.flags & player_flag::PAD != 0)
}

#[test]
fn a_press_on_pad_3_reaches_player_3_only() {
    let mut sim = start(false);
    for n in 1..=3 {
        plug(&mut sim, n);
    }
    sim.idle(1);
    assert_eq!(connected(&last()), [true; 4]);

    // Board pad 2 is player 3 on screen.
    sim.input(Ev::PressN(2, btn::SOUTH | btn::LEFT));
    sim.input(Ev::StickN(2, -100, 0));
    sim.idle(1);
    let i = last();
    assert_eq!(i.players[2].held, button::A | button::LEFT);
    assert_eq!(i.players[2].pressed, button::A | button::LEFT);
    assert_eq!(i.players[2].left_x, -100);
    for n in [0, 1, 3] {
        let p = i.players[n];
        assert_eq!(
            (p.held, p.pressed, p.released, p.left_x),
            (0, 0, 0, 0),
            "player {} sees nothing",
            n + 1
        );
    }

    // Let go: released on player 3 only.
    sim.input(Ev::ReleaseN(2, u32::MAX));
    sim.idle(1);
    let i = last();
    assert_eq!(i.players[2].released, button::A | button::LEFT);
    assert!(
        [0, 1, 3]
            .iter()
            .all(|&n| i.players[n].released == 0 && i.players[n].pressed == 0)
    );
}

#[test]
fn a_short_press_on_one_pad_is_noted_for_that_player_only() {
    let mut sim = start(false);
    plug(&mut sim, 3);
    sim.idle(1);
    // Down and up between two frames, on the fourth pad.
    sim.input(Ev::QuickN(3, btn::NORTH));
    sim.idle(1);
    let i = last();
    assert_eq!(
        (
            i.players[3].held,
            i.players[3].pressed,
            i.players[3].released
        ),
        (0, button::Y, button::Y)
    );
    assert!(i.players[..3].iter().all(|p| p.pressed == 0));
    sim.idle(1);
    assert_eq!(last().players[3].pressed, 0, "only once");
}

#[test]
fn the_same_button_on_two_pads_stays_two_presses() {
    let mut sim = start(false);
    plug(&mut sim, 1);
    sim.idle(1);
    sim.input(Ev::Press(btn::EAST));
    sim.idle(1);
    assert_eq!(last().players[0].pressed, button::B);
    // Player 2 presses the same button while player 1 still holds it:
    // a new press for player 2, nothing new for player 1.
    sim.input(Ev::PressN(1, btn::EAST));
    sim.idle(1);
    let i = last();
    assert_eq!((i.players[0].held, i.players[0].pressed), (button::B, 0));
    assert_eq!(
        (i.players[1].held, i.players[1].pressed),
        (button::B, button::B)
    );
    // Player 1 lets go: player 2 still holds.
    sim.input(Ev::Release(btn::EAST));
    sim.idle(1);
    let i = last();
    assert_eq!(i.players[0].released, button::B);
    assert_eq!((i.players[1].held, i.players[1].released), (button::B, 0));
}

#[test]
fn a_pad_that_goes_away_lets_go_of_its_buttons() {
    let mut sim = start(false);
    plug(&mut sim, 1);
    sim.input(Ev::PressN(1, btn::SOUTH | btn::R));
    sim.board.pad.other(1).state.right.y = 500;
    sim.board.pad.other(1).motion = Some(Motion {
        accel: [0, 0, 4096],
        gyro: [0; 3],
    });
    sim.idle(1);
    let i = last();
    assert_eq!(i.players[1].held, button::A | button::R);
    assert_eq!(i.players[1].flags, player_flag::PAD | player_flag::MOTION);
    // The pad goes away while the buttons are still down on it.
    sim.input(Ev::PadN(1, false));
    sim.idle(1);
    let i = last();
    assert_eq!(
        i.players[1],
        PlayerInput {
            released: button::A | button::R,
            ..PlayerInput::NONE
        }
    );
    assert_eq!(i.players[0].released, 0, "player 1 lost nothing");
    sim.idle(1);
    assert_eq!(last().players[1], PlayerInput::NONE, "and then nothing");
    // The notice comes after the one that said it came.
    for _ in 0..300 {
        if sim.console.notices().current() != Some(msg::PADS_ON[1]) {
            break;
        }
        sim.idle(1);
    }
    assert_eq!(sim.console.notices().current(), Some("PAD 2 DISCONNECTED"));
}

#[test]
fn buttons_held_on_any_pad_at_start_are_hidden_until_let_go() {
    SEEN.with(|s| s.borrow_mut().clear());
    ON_DEMAND.with(|o| *o.borrow_mut() = false);
    let mut sim = Sim::with_games(Some(FOUR), WATCHER);
    sim.boot();
    sim.idle(2);
    plug(&mut sim, 2);
    sim.input(Ev::PressN(2, btn::WEST));
    sim.idle(1);
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Game);
    sim.idle(2);
    assert!(seen().iter().all(|i| i.players[2].held == 0));
    sim.input(Ev::ReleaseN(2, u32::MAX));
    sim.idle(1);
    assert_eq!(last().players[2].released, 0, "never seen, never let go");
    sim.input(Ev::PressN(2, btn::WEST));
    sim.idle(1);
    assert_eq!(last().players[2].pressed, button::X);
}

#[test]
fn only_player_1_opens_and_runs_the_pause_menu() {
    let mut sim = start(false);
    plug(&mut sim, 1);
    sim.idle(1);
    // Home on pad 2: not the game's (Home never is), and no pause.
    sim.input(Ev::PressN(1, btn::HOME));
    sim.idle(2);
    assert!(!sim.console.game().paused());
    assert_eq!(last().players[1].held, 0);
    sim.input(Ev::ReleaseN(1, u32::MAX));
    sim.idle(1);
    // Player 1 pauses; player 2's confirm does not pick anything.
    sim.tap_button(HOME);
    assert!(sim.console.game().paused());
    let n = calls();
    sim.input(Ev::PressN(1, btn::SOUTH));
    sim.idle(2);
    sim.input(Ev::ReleaseN(1, u32::MAX));
    sim.idle(2);
    assert!(sim.console.game().paused(), "player 2 cannot resume");
    assert_eq!(calls(), n, "the game is not called while paused");
    sim.tap_button(SOUTH);
    assert!(!sim.console.game().paused());
}

#[test]
fn player_2_cannot_start_a_game_from_home() {
    SEEN.with(|s| s.borrow_mut().clear());
    let mut sim = Sim::with_games(Some(FOUR), WATCHER);
    sim.boot();
    sim.idle(2);
    plug(&mut sim, 1);
    sim.idle(1);
    sim.input(Ev::PressN(1, btn::SOUTH));
    sim.idle(2);
    sim.input(Ev::ReleaseN(1, u32::MAX));
    sim.input(Ev::QuickN(1, btn::SOUTH));
    sim.idle(2);
    assert_eq!(sim.console.page(), Page::Home);
}

#[test]
fn a_press_on_any_pad_wakes_an_on_demand_game() {
    let mut sim = start(true);
    sim.idle(3);
    plug(&mut sim, 3);
    sim.idle(2);
    let n = calls();
    sim.idle(10);
    assert_eq!(calls(), n, "asleep");
    sim.input(Ev::QuickN(3, btn::PLUS));
    sim.step_tick();
    assert_eq!(calls(), n + 1);
    assert_eq!(last().players[3].pressed, button::START);
    // A stick on pad 2 (not plugged in) changes nothing.
    sim.input(Ev::StickN(2, 30_000, 0));
    sim.idle(5);
    assert_eq!(calls(), n + 1);
    // A stick on the plugged-in pad wakes it.
    sim.input(Ev::StickN(3, 30_000, 0));
    sim.idle(1);
    assert_eq!(calls(), n + 2);
    assert_eq!(last().players[3].left_x, 30_000);
}

#[test]
fn pads_coming_and_going_have_their_own_notices() {
    let mut sim = Sim::with_games(Some(FOUR), WATCHER);
    sim.boot();
    sim.idle(2);
    plug(&mut sim, 3);
    sim.step_tick();
    assert_eq!(sim.console.notices().current(), Some(msg::PADS_ON[3]));
    assert_eq!(msg::PADS_ON[3], "PAD 4 CONNECTED");
    assert_eq!(msg::PADS_ON[0], msg::PAD_ON);
    assert_eq!(msg::PADS_OFF[0], msg::PAD_OFF);
}
