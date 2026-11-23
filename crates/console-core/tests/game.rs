//! A game running on the console, with no window: fake games linked
//! into the fake board, driven through the test runner.
//!
//! Most fake games are written with the game kit, as real ones are. A few
//! are written by hand against the raw tables, to send what the kit
//! never would (an unknown video mode, older and newer tables).

mod common;

use std::cell::RefCell;

use board_api::{Motion, btn};
use common::{DOWN, EAST, Ev, FOUR, HOME, SOUTH, Sim, UP};
use console_core::game_api::{
    self, ConsoleApi, GameCallbacks, GameFrame, Input, LinkedGame, answer, button, event,
    input_flag, player_flag, request, status,
};
use console_core::{FRAME_US, Page, msg, pause, theme};
use game_kit::{Ctx, Exit, Flow, Game, VideoSettings, rate};

// ---- The probe: a kit game that writes down what it sees -------------------

/// What the probe saw in one call.
#[derive(Clone, Copy, Debug)]
struct Seen {
    frame: u32,
    time_us: u64,
    delta_us: u32,
    started: bool,
    resumed: bool,
    redraw: bool,
    input: Input,
    motion: Option<game_kit::Motion>,
    answer: u16,
    video: VideoSettings,
}

/// What the probe does in its next call.
#[derive(Clone, Copy, Debug)]
enum Cmd {
    Rate(u16),
    Colour(u8, u16),
    WakeAfter(u64),
    Log(&'static str),
    Done,
    Panic,
}

thread_local! {
    static SEEN: RefCell<Vec<Seen>> = const { RefCell::new(Vec::new()) };
    static NEXT: RefCell<Vec<Cmd>> = const { RefCell::new(Vec::new()) };
    static EXITS: RefCell<Vec<Exit>> = const { RefCell::new(Vec::new()) };
}

fn reset() {
    SEEN.with(|s| s.borrow_mut().clear());
    NEXT.with(|s| s.borrow_mut().clear());
    EXITS.with(|s| s.borrow_mut().clear());
}

fn seen() -> Vec<Seen> {
    SEEN.with(|s| s.borrow().clone())
}

fn calls() -> usize {
    SEEN.with(|s| s.borrow().len())
}

fn last() -> Seen {
    *SEEN
        .with(|s| s.borrow().last().copied())
        .as_ref()
        .expect("a call")
}

fn next(cmd: Cmd) {
    NEXT.with(|s| s.borrow_mut().push(cmd));
}

/// Colours the probe draws with.
const BACK: u8 = 10;
const TOP: u8 = 20;

mod probe {
    use super::*;

    pub struct Probe;

    impl Game for Probe {
        fn start(ctx: &mut Ctx) -> Self {
            // A still picture, drawn once: a band at the top, the rest
            // plain.
            let s = ctx.screen();
            s.clear(BACK);
            s.rect(0, 0, 160, 20, TOP);
            Probe
        }

        fn update(&mut self, ctx: &mut Ctx) -> Flow {
            SEEN.with(|s| {
                s.borrow_mut().push(Seen {
                    frame: ctx.frame(),
                    time_us: ctx.time_us(),
                    delta_us: ctx.delta_us(),
                    started: ctx.started(),
                    resumed: ctx.resumed(),
                    redraw: ctx.must_redraw(),
                    input: *ctx.input(),
                    motion: ctx.motion(),
                    answer: ctx.last_answer(),
                    video: ctx.video(),
                })
            });
            let cmds: Vec<Cmd> = NEXT.with(|s| s.borrow_mut().drain(..).collect());
            for c in cmds {
                match c {
                    Cmd::Rate(r) => {
                        let _ = ctx.set_rate(r);
                    }
                    Cmd::Colour(i, c) => {
                        ctx.set_colour(i, c);
                    }
                    Cmd::WakeAfter(us) => ctx.wake_after(us),
                    Cmd::Log(t) => ctx.log(t),
                    Cmd::Done => return Flow::Done,
                    Cmd::Panic => panic!("the probe was told to"),
                }
            }
            Flow::Run
        }

        fn exit(&mut self, ctx: &mut Ctx, why: Exit) {
            ctx.log("probe exits");
            EXITS.with(|s| s.borrow_mut().push(why));
        }
    }

    game_kit::game!(Probe);
}

/// The first game of the FOUR card (sorted by name) is BLOCKS.
// SAFETY: made by `game_kit::game!`.
static PROBE: &[LinkedGame] = &[unsafe { LinkedGame::new("BLOCKS", probe::game_entry) }];

/// Boots to home and starts the first game with a short press of the
/// confirm button.
fn start(games: &'static [LinkedGame]) -> Sim {
    reset();
    let mut sim = Sim::with_games(Some(FOUR), games);
    sim.boot();
    sim.idle(2);
    sim.tap_button(SOUTH);
    sim
}

fn log_has(sim: &Sim, text: &str) -> bool {
    sim.board.log.lines().iter().any(|l| l.contains(text))
}

// ---- Start and draw --------------------------------------------------------

#[test]
fn a_game_starts_and_its_picture_is_shown() {
    let sim = start(PROBE);
    assert_eq!(sim.console.page(), Page::Game);
    assert_eq!(sim.console.game().folder(), "BLOCKS");
    assert_eq!(sim.board.games.started, 1);
    assert!(log_has(&sim, "console: game BLOCKS"));

    let first = seen()[0];
    assert!(first.started && first.redraw);
    assert_eq!((first.frame, first.time_us, first.delta_us), (0, 0, 0));
    assert_eq!(first.video, VideoSettings::default());

    // The game's picture, and the start colours.
    assert_eq!(sim.pixel(80, 5), TOP);
    assert_eq!(sim.pixel(80, 100), BACK);
    let pal = sim.console.video().palette();
    for i in [0u8, 7, 100, 239] {
        assert_eq!(pal.get(i), game_api::picture::start_colour(i));
    }
    // On the panel too (2x): the start colour of TOP.
    let panel = sim.board.screen.image[(10 * 320) + 160];
    assert_eq!(panel, game_api::picture::start_colour(TOP));
    // No bottom bar over the game.
    assert_eq!(sim.pixel(80, 115), BACK);
}

#[test]
fn a_still_game_at_30_fps_is_called_every_frame_but_sends_nothing() {
    let mut sim = start(PROBE);
    let n = calls();
    sim.idle(30);
    assert_eq!(calls() - n, 30);
    assert!(
        sim.ticks[sim.ticks.len() - 25..]
            .iter()
            .all(|t| t.pixels == 0)
    );
    let s = seen();
    let deltas: Vec<u32> = s[n..].iter().map(|c| c.delta_us).collect();
    assert!(deltas.iter().all(|&d| d == FRAME_US as u32), "{deltas:?}");
}

#[test]
fn palette_changes_and_log_lines_reach_the_console() {
    let mut sim = start(PROBE);
    next(Cmd::Colour(TOP, 0xF800));
    next(Cmd::Colour(245, 0x1234)); // the console's: refused by the kit
    next(Cmd::Log("hello from the game"));
    sim.idle(1);
    let pal = sim.console.video().palette();
    assert_eq!(pal.get(TOP), 0xF800);
    assert_eq!(pal.get(theme::OK), theme::rgb(theme::UI_COLOURS[5]));
    assert_eq!(sim.board.screen.image[(10 * 320) + 160], 0xF800);
    assert!(log_has(&sim, "game: hello from the game"));
}

// ---- Input -------------------------------------------------------------------

#[test]
fn every_pad_button_reaches_the_game_and_home_never_does() {
    let mut sim = start(PROBE);
    let pairs = [
        (btn::UP, button::UP),
        (btn::DOWN, button::DOWN),
        (btn::LEFT, button::LEFT),
        (btn::RIGHT, button::RIGHT),
        (btn::SOUTH, button::A),
        (btn::EAST, button::B),
        (btn::WEST, button::X),
        (btn::NORTH, button::Y),
        (btn::L, button::L),
        (btn::R, button::R),
        (btn::PLUS, button::START),
        (btn::MINUS, button::SELECT),
    ];
    for (pad, game) in pairs {
        sim.input(Ev::Press(pad));
        sim.idle(1);
        let i = last().input;
        assert_eq!(
            (i.players[0].held, i.players[0].pressed),
            (game, game),
            "pad bit {pad:#x}"
        );
        sim.input(Ev::Release(pad));
        sim.idle(1);
        let i = last().input;
        assert_eq!((i.players[0].held, i.players[0].released), (0, game));
    }
    // Capture and the stick clicks are not game buttons.
    sim.input(Ev::Press(btn::CAPTURE | btn::LSTICK | btn::RSTICK));
    sim.idle(1);
    assert_eq!(last().input.players[0].held, 0);
}

#[test]
fn the_button_that_started_the_game_is_hidden_until_let_go() {
    reset();
    let mut sim = Sim::with_games(Some(FOUR), PROBE);
    sim.boot();
    sim.idle(2);
    sim.input(Ev::Press(SOUTH));
    sim.idle(3);
    assert_eq!(sim.console.page(), Page::Game);
    assert!(seen().iter().all(|c| c.input.players[0].held == 0));
    sim.input(Ev::Release(SOUTH));
    sim.idle(1);
    sim.input(Ev::Press(SOUTH));
    sim.idle(1);
    assert_eq!(last().input.players[0].pressed, button::A);
}

#[test]
fn sticks_triggers_touch_and_motion_reach_the_game() {
    let mut sim = start(PROBE);
    sim.board.pad.state.left.x = -32768;
    sim.board.pad.state.left.y = 100;
    sim.board.pad.state.right.x = 32767;
    sim.board.pad.state.lt = 1023;
    sim.board.pad.state.buttons |= btn::ZR;
    sim.input(Ev::Touch(150, 110));
    sim.idle(1);
    let i = last().input;
    assert_eq!(
        (
            i.players[0].left_x,
            i.players[0].left_y,
            i.players[0].right_x,
            i.players[0].right_y
        ),
        (-32767, 100, 32767, 0)
    );
    assert_eq!(
        (i.players[0].left_trigger, i.players[0].right_trigger),
        (32767, 32767)
    );
    assert_eq!((i.touch_x, i.touch_y), (150, 110));
    assert_eq!(i.flags, input_flag::TOUCH | input_flag::TOUCH_START);
    assert_eq!(i.players[0].flags, player_flag::PAD);
    assert_eq!(last().motion, None, "no motion from this pad");

    sim.input(Ev::Lift);
    sim.board.pad.motion = Some(Motion {
        accel: [0, 0, 4096],
        gyro: [16, -16, 0],
    });
    sim.idle(1);
    let i = last().input;
    assert_eq!(i.flags, input_flag::TOUCH_END);
    assert_eq!(i.players[0].flags, player_flag::PAD | player_flag::MOTION);
    assert_eq!((i.touch_x, i.touch_y), (150, 110), "where it was last");
    let m = last().motion.unwrap();
    assert_eq!((m.accel, m.gyro), ([0, 0, 4096], [16, -16, 0]));

    // Motion off again (the pad changed mode).
    sim.board.pad.motion = None;
    sim.idle(1);
    assert_eq!(last().input.players[0].flags, player_flag::PAD);
    assert_eq!(last().input.players[0].accel, [0, 0, 0]);

    // No pad: no pad flag, no motion even if the board had some.
    sim.board.pad.motion = Some(Motion::default());
    sim.input(Ev::Pad(false));
    sim.idle(1);
    assert_eq!(last().input.players[0].flags, 0);
    assert_eq!(last().input.players[0].accel, [0, 0, 0]);
}

// ---- Frame rates ---------------------------------------------------------------

#[test]
fn at_60_fps_the_game_is_called_twice_a_tick() {
    let mut sim = start(PROBE);
    next(Cmd::Rate(rate::FPS60));
    sim.idle(2);
    assert_eq!(last().answer, answer::OK);
    assert_eq!(last().video.rate, rate::FPS60);
    assert_eq!(sim.console.game().rate(), rate::FPS60);
    let n = calls();
    sim.idle(30);
    let made = calls() - n;
    assert!((59..=61).contains(&made), "{made} calls in 30 ticks");
}

#[test]
fn on_demand_calls_only_on_input_or_the_timer() {
    let mut sim = start(PROBE);
    next(Cmd::Rate(rate::ON_DEMAND));
    sim.idle(2);
    let n = calls();
    sim.idle(30);
    assert_eq!(calls(), n, "idle: not called");
    // The console sleeps: no step at all in these ticks.
    assert!(
        sim.ticks[sim.ticks.len() - 25..]
            .iter()
            .all(|t| t.steps == 0)
    );
    assert!(
        sim.ticks[sim.ticks.len() - 30..]
            .iter()
            .all(|t| t.presents == 0 || t.pixels == 0)
    );

    // Motion alone does not wake it.
    sim.board.pad.motion = Some(Motion::default());
    sim.input(Ev::Stick(0, 0)); // the runner's "something changed"
    sim.idle(3);
    assert_eq!(calls(), n);

    // A button does.
    sim.input(Ev::Press(EAST));
    sim.idle(3);
    assert_eq!(calls(), n + 1);
    assert_eq!(last().input.players[0].pressed, button::B);
    sim.input(Ev::Release(EAST));
    sim.idle(3);
    assert_eq!(calls(), n + 2);

    // The timer: half a second of game time later.
    next(Cmd::WakeAfter(500_000));
    sim.input(Ev::Press(EAST));
    sim.idle(1);
    let t0 = last().time_us;
    let at = calls();
    sim.input(Ev::Release(EAST));
    sim.idle(1);
    assert_eq!(calls(), at + 1, "the release");
    // A timer lasts one call: the game sets it again each time.
    next(Cmd::WakeAfter(500_000));
    sim.input(Ev::Press(EAST));
    sim.idle(1);
    let t1 = last().time_us;
    let at = calls();
    sim.idle(20);
    assert_eq!(calls(), at + 1, "woken once by its timer");
    let woke = last().time_us;
    assert!(
        woke >= t1 + 500_000 && woke < t1 + 500_000 + FRAME_US,
        "{woke} vs {t1}"
    );
    assert!(t0 < t1);
}

#[test]
fn unknown_rates_are_refused_by_the_kit_and_nothing_changes() {
    let mut sim = start(PROBE);
    next(Cmd::Rate(9));
    sim.idle(2);
    assert_eq!(last().answer, answer::NONE, "the kit never sent it");
    assert_eq!(sim.console.game().rate(), rate::FPS30);
}

// ---- Pause menu --------------------------------------------------------------

#[test]
fn home_pauses_the_game_and_resume_goes_on_where_it_was() {
    let mut sim = start(PROBE);
    sim.idle(10);
    let before = last();
    sim.tap_button(HOME);
    assert!(sim.console.game().paused());
    let n = calls();
    sim.idle(60);
    assert_eq!(calls(), n, "not called while paused");
    // The menu shows: its box is drawn in the console's colours (left of
    // the chosen item's gold line).
    let panel = theme::rgb(theme::UI_COLOURS[1]);
    let menu_px = sim.board.screen.image[(2 * 46) * 320 + 2 * 42];
    assert_eq!(menu_px, panel);
    // ...but the game's picture under it is kept.
    assert_eq!(sim.pixel(50, 46), BACK);
    // Home never reached the game.
    assert!(seen().iter().all(|c| c.input.players[0].held == 0));

    sim.tap_button(EAST); // back: resume
    assert!(!sim.console.game().paused());
    let after = seen()[n];
    assert!(after.resumed);
    // Game time did not move while paused: one or two frames only.
    let gap = after.time_us - before.time_us;
    assert!(gap <= 3 * FRAME_US, "game time moved {gap} us while paused");
    // The menu is gone from the panel.
    let back = game_api::picture::start_colour(BACK);
    assert_eq!(sim.board.screen.image[(2 * 46) * 320 + 2 * 50], back);
}

#[test]
fn the_boot_button_opens_the_menu_and_a_tap_chooses() {
    let mut sim = start(PROBE);
    sim.input(Ev::Boot(true));
    sim.idle(1);
    sim.input(Ev::Boot(false));
    sim.idle(1);
    assert!(sim.console.game().paused());
    // "BACK TO HOME" is the third item.
    let y = pause::item_y(pause::Kind::Game, 2) as u16 + 2;
    sim.tap_at(80, y);
    assert_eq!(sim.console.page(), Page::Home);
    assert_eq!(EXITS.with(|e| e.borrow().clone()), vec![Exit::Player]);
}

#[test]
fn back_to_home_calls_exit_and_ends_the_game() {
    let mut sim = start(PROBE);
    sim.tap_button(HOME);
    sim.tap_button(DOWN);
    assert_eq!(sim.console.game().menu_item(), 1);
    sim.tap_button(DOWN);
    assert_eq!(sim.console.game().menu_item(), 2);
    sim.tap_button(UP);
    sim.tap_button(DOWN);
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Home);
    assert!(!sim.console.game().running());
    assert_eq!(EXITS.with(|e| e.borrow().clone()), vec![Exit::Player]);
    assert!(log_has(&sim, "game: probe exits"));
    assert_eq!(sim.board.games.ended, 1);
    // The home screen is back with its tile colours.
    let tile = sim.console.home_games(&mut *sim.board)[0].tile;
    assert_eq!(sim.console.video().palette().get(theme::TILE_FIRST), tile);
    // And the same game starts again.
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Game);
    assert_eq!(sim.board.games.started, 2);
}

// ---- Notices over the game ---------------------------------------------------

#[test]
fn a_notice_shows_over_the_game_and_the_picture_comes_back() {
    let mut sim = start(PROBE);
    next(Cmd::Rate(rate::ON_DEMAND));
    sim.idle(2);
    assert!(sim.notify("HELLO"));
    sim.idle(10);
    // On the panel: the notice (its gold fill) in the middle of the top.
    let gold = theme::rgb(theme::UI_COLOURS[3]);
    let shown = (0..32).any(|y| sim.board.screen.image[y * 320 + 160] == gold);
    assert!(shown, "the notice is on the panel");
    // In the picture the game draws into: its own pixels.
    for y in 0..20 {
        assert_eq!(sim.pixel(80, y), TOP, "row {y}");
    }
    // Once the notice is gone, the panel shows the game again.
    sim.idle(120);
    assert!(sim.console.notices().is_empty());
    let top = game_api::picture::start_colour(TOP);
    for y in 0..40 {
        assert_eq!(sim.board.screen.image[y * 320 + 160], top, "panel row {y}");
    }
    // An on-demand game was never called for any of that.
    assert_eq!(last().frame, 2);
}

// ---- Ending ----------------------------------------------------------------

#[test]
fn a_game_that_ends_goes_home_with_a_notice() {
    let mut sim = start(PROBE);
    next(Cmd::Done);
    sim.idle(1);
    assert_eq!(sim.console.page(), Page::Home);
    assert_eq!(sim.console.notices().current(), Some(msg::GAME_ENDED));
    assert!(
        EXITS.with(|e| e.borrow().is_empty()),
        "exit is not called after done"
    );
    assert_eq!(sim.board.games.ended, 1);
    sim.idle(5);
    assert_eq!(sim.console.page(), Page::Home);
}

#[test]
fn a_game_that_panics_goes_home_with_a_warning_and_the_console_goes_on() {
    let mut sim = start(PROBE);
    next(Cmd::Panic);
    sim.idle(1);
    assert_eq!(sim.console.page(), Page::Home);
    assert_eq!(sim.console.notices().current(), Some(msg::GAME_ABORTED));
    assert!(log_has(&sim, "game: panic: the probe was told to"));
    // Everything still works: start it again.
    sim.idle(3);
    let n = calls();
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Game);
    assert!(seen()[n].started);
}

/// The game's colours (palette 0 to 239) must not stay behind: home
/// after a game looks exactly as home before it, palette included.
#[test]
fn home_after_a_game_is_the_same_as_before_it() {
    reset();
    let mut sim = Sim::with_games(Some(FOUR), PROBE);
    sim.boot();
    sim.idle(30);
    let palette = *sim.console.video().palette().colours();
    let picture = sim.console.video().frame().0;
    let panel = sim.board.screen.image.to_vec();

    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Game);
    next(Cmd::Colour(3, 0xF800));
    sim.idle(3);
    // Back to home through the pause menu (no notice on that way).
    sim.tap_button(HOME);
    sim.tap_button(DOWN);
    sim.tap_button(DOWN); // past BRIGHTNESS to BACK TO HOME
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Home);
    sim.idle(30);

    let after = sim.console.video().palette().colours();
    let changed: Vec<usize> = (0..256).filter(|&i| after[i] != palette[i]).collect();
    assert!(changed.is_empty(), "palette numbers changed: {changed:?}");
    assert!(sim.console.video().frame().0 == picture, "the picture");
    assert!(sim.board.screen.image[..] == panel[..], "the panel");
}

#[test]
fn a_board_without_games_says_they_cannot_start() {
    let mut sim = start(&[]);
    assert_eq!(sim.console.page(), Page::Home);
    assert_eq!(sim.console.notices().current(), Some(msg::NO_START));
    sim.idle(1);
}

#[test]
fn a_missing_game_gives_a_clear_notice() {
    // SAFETY: made by `game_kit::game!`.
    static OTHER: &[LinkedGame] = &[unsafe { LinkedGame::new("COMET", probe::game_entry) }];
    let sim = start(OTHER);
    assert_eq!(sim.console.page(), Page::Home);
    assert_eq!(sim.console.notices().current(), Some(msg::GAME_NOT_FOUND));
}

#[test]
fn a_file_the_board_refuses_shows_the_boards_notice() {
    // The chip's loader refuses a file with its own short text (a game
    // built for another code slot, here); the console shows it as it is
    // and stays home.
    reset();
    let mut sim = Sim::with_games(Some(FOUR), PROBE);
    sim.board.games.refuse = Some("GAME FOR OTHER CODE SLOT");
    sim.boot();
    sim.idle(2);
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Home);
    assert_eq!(
        sim.console.notices().current(),
        Some("GAME FOR OTHER CODE SLOT")
    );
    assert!(log_has(&sim, "home: the board refused the game file"));
    assert_eq!(sim.board.games.started, 0);
    // The next start works once the board takes the file.
    sim.board.games.refuse = None;
    sim.idle(120);
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Game);
}

// ---- Raw games: what the kit would never send -------------------------------

thread_local! {
    static RAW_CALLS: RefCell<Vec<GameFrame>> = const { RefCell::new(Vec::new()) };
    static RAW_INPUT_SIZE: RefCell<u32> = const { RefCell::new(0) };
}

/// Asks for video mode 7 (there is none) in its first call, then rate 9,
/// then a hint and a colour the console does not know.
extern "C" fn raw_update(f: *mut GameFrame) -> u32 {
    // SAFETY: the console hands its frame for this call.
    let f = unsafe { &mut *f };
    RAW_CALLS.with(|c| c.borrow_mut().push(*f));
    // SAFETY: the same, for the input.
    RAW_INPUT_SIZE.with(|s| *s.borrow_mut() = unsafe { (*f.input).size });
    let (m, r, h, c) = match f.frame {
        0 => (7, 0, 0, 0),
        1 => (0, 9, 0, 0),
        2 => (0, 0, 31, 0),
        3 => (0, 0, 0, 40),
        _ => (0, 1, 1, 1),
    };
    f.ask_mode = m;
    f.ask_rate = r;
    f.ask_hint = h;
    f.ask_colour = c;
    f.requests = request::VIDEO;
    // Try to change a console colour without asking: it must not stick.
    // SAFETY: the palette has 256 entries.
    unsafe { f.palette.add(241).write(0xFFFF) };
    status::RUN
}

extern "C" fn raw_exit(_f: *mut GameFrame, _reason: u32) {
    EXITS.with(|e| e.borrow_mut().push(Exit::Other(99)));
}

static RAW_NOW: GameCallbacks = GameCallbacks {
    size: size_of::<GameCallbacks>() as u32,
    version: game_api::API_VERSION,
    // SAFETY: the raw functions touch only the frame and what it points at.
    update: unsafe { game_api::UpdateFn::new(raw_update) },
    exit: unsafe { game_api::ExitFn::new(raw_exit) },
};

/// An older game: its table ends after `update`.
static RAW_OLDER: GameCallbacks = GameCallbacks {
    size: core::mem::offset_of!(GameCallbacks, exit) as u32,
    ..RAW_NOW
};

/// A newer game: a bigger table and a later version.
#[repr(C)]
struct NewerTable {
    now: GameCallbacks,
    later: [u32; 4],
}
static RAW_NEWER: NewerTable = NewerTable {
    now: GameCallbacks {
        size: size_of::<NewerTable>() as u32,
        version: game_api::API_VERSION + 1,
        ..RAW_NOW
    },
    later: [1, 2, 3, 4],
};

static RAW_V1: GameCallbacks = GameCallbacks {
    version: 1,
    ..RAW_NOW
};

extern "C" fn entry_now(api: *const ConsoleApi) -> *const GameCallbacks {
    assert!(!api.is_null());
    &RAW_NOW
}
extern "C" fn entry_older(_: *const ConsoleApi) -> *const GameCallbacks {
    &RAW_OLDER
}
extern "C" fn entry_newer(_: *const ConsoleApi) -> *const GameCallbacks {
    &RAW_NEWER.now
}
extern "C" fn entry_v1(_: *const ConsoleApi) -> *const GameCallbacks {
    &RAW_V1
}
extern "C" fn entry_null(_: *const ConsoleApi) -> *const GameCallbacks {
    core::ptr::null()
}

/// `entry` must keep the contract (the raw games here do).
fn blocks(entry: game_api::GameEntry) -> &'static [LinkedGame] {
    // SAFETY: see above; every caller passes a raw game of this file.
    Box::leak(Box::new([unsafe { LinkedGame::new("BLOCKS", entry) }]))
}

fn raw_calls() -> Vec<GameFrame> {
    RAW_CALLS.with(|c| c.borrow().clone())
}

#[test]
fn unknown_video_settings_get_a_clean_no() {
    RAW_CALLS.with(|c| c.borrow_mut().clear());
    let mut sim = start(blocks(entry_now));
    sim.idle(5);
    let c = raw_calls();
    let answers: Vec<u16> = c.iter().map(|f| f.answer).collect();
    assert_eq!(
        &answers[..5],
        &[
            answer::NONE,
            answer::NO_MODE,
            answer::NO_RATE,
            answer::NO_HINT,
            answer::NO_COLOUR
        ]
    );
    // Nothing changed until a request that is all offered.
    assert!(c[..5].iter().all(|f| f.rate == rate::FPS30 && f.hint == 0));
    assert_eq!(c[5].answer, answer::OK);
    assert_eq!((c[5].rate, c[5].hint, c[5].colour), (1, 1, 1));
    // The console's colour came back.
    assert_eq!(
        sim.console.video().palette().get(theme::PANEL),
        theme::rgb(theme::UI_COLOURS[1])
    );
}

#[test]
fn the_console_hands_its_own_table_sizes() {
    RAW_CALLS.with(|c| c.borrow_mut().clear());
    let _sim = start(blocks(entry_now));
    let f = raw_calls()[0];
    assert_eq!(f.size as usize, size_of::<GameFrame>());
    assert_eq!(f.version, game_api::API_VERSION);
    assert_eq!(RAW_INPUT_SIZE.with(|s| *s.borrow()), 176);
    assert_eq!(
        (f.width, f.height, f.stride, f.game_colours),
        (160, 120, 160, 240)
    );
    assert_eq!(f.events, event::START | event::REDRAW);
    assert_eq!(f.offered_modes, 1);
    assert_eq!(f.offered_rates, 0b111);
    assert!(f.log_cap > 0);
}

#[test]
fn an_older_game_table_runs_and_has_no_exit() {
    RAW_CALLS.with(|c| c.borrow_mut().clear());
    let mut sim = start(blocks(entry_older));
    assert_eq!(sim.console.page(), Page::Game);
    sim.tap_button(HOME);
    sim.tap_button(DOWN);
    sim.tap_button(DOWN); // past BRIGHTNESS to BACK TO HOME
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Home);
    assert!(
        EXITS.with(|e| e.borrow().is_empty()),
        "its exit is not read"
    );
}

#[test]
fn a_newer_game_table_runs_with_what_this_console_knows() {
    RAW_CALLS.with(|c| c.borrow_mut().clear());
    let mut sim = start(blocks(entry_newer));
    assert_eq!(sim.console.page(), Page::Game);
    sim.idle(2);
    assert!(raw_calls().len() >= 3);
    sim.tap_button(HOME);
    sim.tap_button(DOWN);
    sim.tap_button(DOWN); // past BRIGHTNESS to BACK TO HOME
    sim.tap_button(SOUTH);
    assert_eq!(EXITS.with(|e| e.borrow().clone()), vec![Exit::Other(99)]);
}

#[test]
fn refused_tables_give_clear_notices() {
    let sim = start(blocks(entry_v1));
    assert_eq!(sim.console.page(), Page::Home);
    assert_eq!(sim.console.notices().current(), Some(msg::GAME_TOO_OLD));
    let sim = start(blocks(entry_null));
    assert_eq!(sim.console.notices().current(), Some(msg::GAME_REFUSED));
}

// ---- Short presses (step 3) ------------------------------------------------
//
// A press or a tap that starts and ends between two frames: the board
// only noted it (`take_pressed`, `take_tap`). These failed before the
// console read those notes: the game never heard of the press.

#[test]
fn a_press_shorter_than_a_frame_reaches_the_game_once() {
    let mut sim = start(PROBE);
    sim.idle(2);
    sim.input(Ev::Quick(SOUTH));
    sim.idle(1);
    let i = last().input;
    // Pressed and let go since the last call; not held now.
    assert_eq!(
        (
            i.players[0].held,
            i.players[0].pressed,
            i.players[0].released
        ),
        (0, button::A, button::A)
    );
    sim.idle(1);
    let i = last().input;
    assert_eq!(
        (
            i.players[0].held,
            i.players[0].pressed,
            i.players[0].released
        ),
        (0, 0, 0),
        "only once"
    );

    // Held at the last call, then let go and pressed again in between:
    // released and pressed, and held now.
    sim.input(Ev::Press(EAST));
    sim.idle(1);
    assert_eq!(last().input.players[0].pressed, button::B);
    sim.input(Ev::Quick(EAST));
    sim.idle(1);
    let i = last().input;
    assert_eq!(
        (
            i.players[0].held,
            i.players[0].pressed,
            i.players[0].released
        ),
        (button::B, button::B, button::B)
    );
}

#[test]
fn a_tap_shorter_than_a_frame_reaches_the_game_once() {
    let mut sim = start(PROBE);
    sim.idle(2);
    sim.input(Ev::QuickTouch(30, 40));
    sim.idle(1);
    let i = last().input;
    assert_eq!(i.flags, input_flag::TOUCH_START | input_flag::TOUCH_END);
    assert_eq!(i.players[0].flags, player_flag::PAD);
    assert_eq!((i.touch_x, i.touch_y), (30, 40), "where it started");
    sim.idle(1);
    assert_eq!(last().input.flags, 0, "only once");
}

#[test]
fn a_short_press_wakes_an_on_demand_game_at_once() {
    let mut sim = start(PROBE);
    next(Cmd::Rate(rate::ON_DEMAND));
    sim.idle(3);
    let n = calls();
    sim.idle(10);
    assert_eq!(calls(), n);
    sim.input(Ev::Quick(btn::WEST));
    sim.step_tick();
    assert_eq!(calls(), n + 1);
    assert_eq!(last().input.players[0].pressed, button::X);
    sim.input(Ev::QuickTouch(5, 6));
    sim.step_tick();
    assert_eq!(calls(), n + 2);
    assert!(last().input.flags & input_flag::TOUCH_START != 0);
    sim.idle(10);
    assert_eq!(calls(), n + 2, "and then it sleeps again");
}

#[test]
fn short_presses_open_and_close_the_pause_menu_and_never_reach_the_game() {
    let mut sim = start(PROBE);
    sim.idle(2);
    sim.input(Ev::Quick(HOME));
    sim.step_tick();
    assert!(sim.console.game().paused(), "a short Home pauses");
    let n = calls();
    // A short press of the confirm button picks RESUME.
    sim.input(Ev::Quick(SOUTH));
    sim.step_tick();
    assert!(!sim.console.game().paused());
    assert_eq!(calls(), n + 1);
    assert!(last().resumed);
    assert_eq!(
        last().input.players[0].pressed,
        0,
        "the press that resumed is the menu's"
    );
    // The board's BOOT button, pressed briefly, pauses too.
    sim.input(Ev::QuickBoot);
    sim.step_tick();
    assert!(sim.console.game().paused());
    sim.input(Ev::Quick(EAST));
    sim.step_tick();
    assert!(!sim.console.game().paused());
    assert!(
        seen()
            .iter()
            .all(|c| c.input.players[0].pressed == 0 && c.input.players[0].held == 0)
    );
}

#[test]
fn the_short_press_that_started_the_game_is_not_the_games() {
    reset();
    let mut sim = Sim::with_games(Some(FOUR), PROBE);
    sim.boot();
    sim.idle(2);
    sim.input(Ev::Quick(SOUTH));
    sim.step_tick();
    assert_eq!(sim.console.page(), Page::Game);
    sim.idle(2);
    assert!(
        seen()
            .iter()
            .all(|c| c.input.players[0].pressed == 0 && c.input.players[0].released == 0)
    );
    // A tap on the chosen tile that started it: not the game's either.
    reset();
    let mut sim = Sim::with_games(Some(FOUR), PROBE);
    sim.boot();
    sim.idle(2);
    sim.input(Ev::QuickTouch(20, 30));
    sim.step_tick();
    assert_eq!(sim.console.page(), Page::Game);
    sim.idle(2);
    assert!(
        seen()
            .iter()
            .all(|c| c.input.flags & (input_flag::TOUCH_START | input_flag::TOUCH_END) == 0)
    );
}
