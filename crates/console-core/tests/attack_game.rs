//! The second reader's attacks on a running game (step 2). No window.
//!
//! Each test tries to break one promise of the game page: a timer far in
//! the future, a noisy stick at the "only when I say so" rate, every
//! input reaching the game exactly once, the picture after a notice or
//! the pause menu, many starts and ends, games that stop with an error at
//! every point, and a raw game that writes nonsense into every field.
//!
//! The two long runs are short by default (a few seconds). Set
//! `ATTACK_LONG=1` for the full length (about 30 s in a debug build):
//! `ATTACK_LONG=1 cargo test --test attack_game`.

mod common;

use std::cell::{Cell, RefCell};

use board_api::{Wake, btn};

/// `short` normally, `long` with `ATTACK_LONG` set.
fn length(short: u32, long: u32) -> u32 {
    if std::env::var_os("ATTACK_LONG").is_some() {
        long
    } else {
        short
    }
}
use common::{EAST, Ev, FOUR, HOME, SOUTH, Sim};
use console_core::game_api::{
    self, ConsoleApi, GameCallbacks, GameFrame, Input, LinkedGame, button, event, picture, status,
};
use console_core::{Console, FRAME_US, Page, theme};
use game_kit::{Ctx, Flow, Game, rate};

// ---- A small seeded random source (the same numbers on every run) ----------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ---- The toy: a kit game that does what the test asks ----------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Do {
    Nothing,
    Rate(u16),
    WakeAt(u64),
    Done,
    PanicInUpdate,
}

thread_local! {
    /// Every call: the input and the events the toy saw.
    static CALLS: RefCell<Vec<(Input, u32, u64)>> = const { RefCell::new(Vec::new()) };
    /// What the toy does in its next call.
    static TODO: Cell<Do> = const { Cell::new(Do::Nothing) };
    /// Panic in `start` / in `exit` (for the next run).
    static PANIC_START: Cell<bool> = const { Cell::new(false) };
    static PANIC_EXIT: Cell<bool> = const { Cell::new(false) };
    /// Whether it draws a dot that follows the input (or nothing at all).
    static DRAWS: Cell<bool> = const { Cell::new(false) };
    /// Problems the toy found in what the console gave it at a start.
    static START_FAULTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn reset() {
    CALLS.with(|c| c.borrow_mut().clear());
    TODO.with(|t| t.set(Do::Nothing));
    PANIC_START.with(|p| p.set(false));
    PANIC_EXIT.with(|p| p.set(false));
    DRAWS.with(|d| d.set(false));
    START_FAULTS.with(|s| s.borrow_mut().clear());
}

fn calls() -> Vec<(Input, u32, u64)> {
    CALLS.with(|c| c.borrow().clone())
}

mod toy {
    use super::*;

    pub struct Toy {
        x: i16,
        y: i16,
    }

    impl Game for Toy {
        fn start(ctx: &mut Ctx) -> Self {
            // What every start must give: a picture cleared to 0, the
            // start colours, frame 0, time 0, START and REDRAW.
            let mut faults = Vec::new();
            if ctx.screen().0.iter().any(|&p| p != 0) {
                faults.push("picture not cleared".to_string());
            }
            for i in 0..240u16 {
                let i = i as u8;
                if ctx.colour(i) != picture::start_colour(i) {
                    faults.push(format!("colour {i} not the start colour"));
                    break;
                }
            }
            if ctx.frame() != 0 || ctx.time_us() != 0 || ctx.delta_us() != 0 {
                faults.push(format!(
                    "frame {} time {} delta {}",
                    ctx.frame(),
                    ctx.time_us(),
                    ctx.delta_us()
                ));
            }
            if !(ctx.started() && ctx.must_redraw()) || ctx.resumed() {
                faults.push("events".to_string());
            }
            if ctx.video().rate != rate::FPS30 {
                faults.push("rate not reset".to_string());
            }
            START_FAULTS.with(|s| s.borrow_mut().extend(faults));
            if PANIC_START.with(|p| p.get()) {
                panic!("toy panics in start");
            }
            ctx.screen().clear(30);
            Toy { x: 80, y: 60 }
        }

        fn update(&mut self, ctx: &mut Ctx) -> Flow {
            let events = (ctx.started() as u32)
                | ((ctx.resumed() as u32) << 1)
                | ((ctx.must_redraw() as u32) << 2);
            CALLS.with(|c| c.borrow_mut().push((*ctx.input(), events, ctx.time_us())));
            if DRAWS.with(|d| d.get()) {
                // A dot that moves with the pad, through the notice band
                // and the pause menu's box too.
                let s = ctx.screen();
                s.rect(self.x, self.y, 3, 3, 30);
                let (lx, ly) = ctx.left_stick();
                self.x = (self.x + lx / 8192).rem_euclid(158);
                self.y = (self.y + ly / 8192).rem_euclid(118);
                if ctx.held(button::UP) {
                    self.y = (self.y + 117) % 118;
                }
                if ctx.held(button::LEFT) {
                    self.x = (self.x + 157) % 158;
                }
                let c = (ctx.frame() % 200) as u8 + 1;
                ctx.screen().rect(self.x, self.y, 3, 3, c);
            }
            match TODO.with(|t| t.replace(Do::Nothing)) {
                Do::Nothing => {}
                Do::Rate(r) => {
                    let _ = ctx.set_rate(r);
                }
                Do::WakeAt(t) => ctx.wake_at(t),
                Do::Done => return Flow::Done,
                Do::PanicInUpdate => panic!("toy panics in update"),
            }
            Flow::Run
        }

        fn exit(&mut self, _ctx: &mut Ctx, _why: game_kit::Exit) {
            if PANIC_EXIT.with(|p| p.get()) {
                panic!("toy panics in exit");
            }
        }
    }

    game_kit::game!(Toy);
}

/// The first game of the FOUR card (sorted by name) is BLOCKS.
// SAFETY: a game made for these tests, keeping the contract.
static TOY: &[LinkedGame] = &[unsafe { LinkedGame::new("BLOCKS", toy::game_entry) }];

fn start_toy() -> Sim {
    reset();
    let mut sim = Sim::with_games(Some(FOUR), TOY);
    sim.boot();
    sim.idle(2);
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Game);
    sim
}

/// Picks "BACK TO HOME" in the pause menu.
fn back_to_home(sim: &mut Sim) {
    sim.tap_button(HOME);
    sim.tap_button(common::DOWN);
    sim.tap_button(common::DOWN);
    sim.tap_button(SOUTH);
}

/// The panel as the picture and palette say it should be (2x, no
/// border on the fake 320 x 240 panel), compared pixel by pixel. Pixels
/// under a console overlay that is on show now are skipped.
fn stale_pixels(sim: &Sim) -> usize {
    let v = sim.console.video();
    let notices = !sim.console.notices().is_empty();
    let paused = sim.console.page() == Page::Game && sim.console.game().paused();
    let band = (theme::NOTE_Y as u16 + theme::NOTE_H) as usize;
    let (mx, my) = (theme::MENU_X as usize, theme::MENU_Y as usize);
    let (mw, mh) = (theme::MENU_W as usize, theme::MENU_H as usize);
    let mut bad = 0;
    for y in 0..120usize {
        for x in 0..160usize {
            if sim.console.page() == Page::Game {
                if notices && y < band {
                    continue;
                }
                if paused && (mx..mx + mw).contains(&x) && (my..my + mh).contains(&y) {
                    continue;
                }
            } else {
                // Home draws its overlays into the picture itself.
            }
            let want = v.palette().get(v.frame().get(x as u16, y as u16));
            if sim.board.screen.image[(2 * y) * 320 + 2 * x] != want {
                bad += 1;
            }
        }
    }
    bad
}

// ---- 1. A timer far in the future -----------------------------------------

#[test]
fn a_timer_at_the_end_of_time_does_not_crash_or_wake_the_console() {
    let mut sim = start_toy();
    TODO.with(|t| t.set(Do::Rate(rate::ON_DEMAND)));
    sim.idle(2);
    assert_eq!(sim.console.game().rate(), rate::ON_DEMAND);
    // The kit's own `wake_after(u64::MAX)` gives this too (it saturates).
    TODO.with(|t| t.set(Do::WakeAt(u64::MAX)));
    sim.input(Ev::Press(EAST));
    sim.idle(1);
    sim.input(Ev::Release(EAST));
    TODO.with(|t| t.set(Do::WakeAt(u64::MAX)));
    sim.idle(1);
    let n = calls().len();
    sim.idle(30);
    assert_eq!(calls().len(), n, "not woken by a timer that never comes");
    let quiet = sim.ticks[sim.ticks.len() - 25..]
        .iter()
        .all(|t| t.steps == 0);
    assert!(quiet, "the console sleeps");
}

// ---- 2. A noisy stick at the on-demand rate --------------------------------

/// A board that wakes the console at every pad report (as the CYD's loop
/// does: `deadline = now` when input waits). A stick that shakes by one
/// unit every millisecond must not make the game run faster than the
/// fastest rate the console offers (60 calls a second).
#[test]
fn an_on_demand_game_is_never_called_more_than_60_times_a_second() {
    let mut sim = start_toy();
    TODO.with(|t| t.set(Do::Rate(rate::ON_DEMAND)));
    sim.idle(3);
    assert_eq!(sim.console.game().rate(), rate::ON_DEMAND);
    let n = calls().len();
    let presents = sim.board.screen.presents;
    let t0 = sim.board.clock.now_us;
    // One second of pad reports, one per millisecond, each a little
    // different (stick noise), each waking the console at once.
    for ms in 1..=1000u64 {
        sim.board.pad.state.left.x = if ms % 2 == 0 { 300 } else { 301 };
        sim.board.clock.now_us = t0 + ms * 1000;
        let _ = sim.console.step(&mut *sim.board);
    }
    let made = calls().len() - n;
    let sent = sim.board.screen.presents - presents;
    println!("{made} calls and {sent} presents in one second of stick noise");
    assert!(made <= 61, "{made} game calls in one second");
    // The last change still reaches the game soon after.
    let last = sim.board.pad.state.left.x;
    for ms in 1001..=1040u64 {
        sim.board.clock.now_us = t0 + ms * 1000;
        let _ = sim.console.step(&mut *sim.board);
    }
    assert_eq!(calls().last().unwrap().0.players[0].left_x, last);
}

/// The same board, the stick held still: one wake gives at most one
/// call, and an input change made early is not lost but comes at the
/// next allowed time, even when no more input arrives.
#[test]
fn an_early_input_change_waits_and_is_then_delivered() {
    let mut sim = start_toy();
    TODO.with(|t| t.set(Do::Rate(rate::ON_DEMAND)));
    sim.idle(3);
    let t0 = sim.board.clock.now_us;
    // A press, then a release 2 ms later, each waking the console at once.
    sim.board.pad.state.buttons = btn::EAST;
    sim.board.clock.now_us = t0 + 1000;
    let _ = sim.console.step(&mut *sim.board);
    let n = calls().len();
    sim.board.pad.state.buttons = 0;
    sim.board.clock.now_us = t0 + 3000;
    let w = sim.console.step(&mut *sim.board);
    // Follow the console's own wake times from here, with no input.
    let mut w: Wake = w;
    for _ in 0..10 {
        let at = w.deadline().max(sim.board.clock.now_us);
        if at == u64::MAX {
            break;
        }
        sim.board.clock.now_us = at;
        w = sim.console.step(&mut *sim.board);
    }
    let c = calls();
    assert!(c.len() > n, "the release reached the game");
    let (last, _, _) = c.last().unwrap();
    assert_eq!(last.players[0].held, 0);
    assert_eq!(last.players[0].released, button::B);
}

// ---- 3. Every input reaches the game exactly once --------------------------

/// The game buttons a pad state gives (the console's table).
fn mapped(pad_buttons: u32) -> u32 {
    let mut b = 0;
    for &(p, g) in &console_core::gamein::GAME_BUTTONS {
        if pad_buttons & p != 0 {
            b |= g;
        }
    }
    b
}

/// Random play: buttons, Home, BOOT, the pause menu, back to home and
/// starting again. After every call the input the game saw is checked
/// against a model of the promise: "held" is what is held, minus the
/// buttons held at the start or the resume until they are let go;
/// "pressed" and "released" are the changes since the last call; nothing
/// outside the 12 buttons. And every new press of a button the game may
/// see appears as "pressed" exactly once.
fn input_run(seed: u64, ticks: u64) -> (u32, u32) {
    let mut sim = start_toy();
    DRAWS.with(|d| d.set(true));
    let mut rng = Rng(seed);
    let pads = [
        btn::UP,
        btn::DOWN,
        btn::LEFT,
        btn::RIGHT,
        btn::SOUTH,
        btn::EAST,
        btn::WEST,
        btn::NORTH,
        btn::L,
        btn::R,
        btn::PLUS,
        btn::MINUS,
        btn::HOME,
        btn::CAPTURE,
        btn::ZL,
        btn::ZR,
    ];
    let mut hide = 0u32;
    let mut prev_held = 0u32;
    let mut prev_raw = mapped(sim.board.pad.state.buttons);
    let mut presses_made = 0u32;
    let mut presses_seen = 0u32;
    for _ in 0..ticks {
        // Change one or two things.
        for _ in 0..1 + rng.below(2) {
            let r = rng.below(100);
            let b = pads[rng.below(pads.len() as u64) as usize];
            if r < 70 {
                if sim.board.pad.state.buttons & b != 0 {
                    sim.input(Ev::Release(b));
                } else {
                    sim.input(Ev::Press(b));
                }
            } else if r < 75 {
                let down = sim.board.keys.held == 0;
                sim.input(Ev::Boot(down));
            } else if r < 85 {
                let x = rng.below(65535) as i16;
                sim.input(Ev::Stick(x, -x));
            }
        }
        let raw_now = mapped(sim.board.pad.state.buttons);
        let page_before = sim.console.page();
        let paused_before = sim.console.page() == Page::Game && sim.console.game().paused();
        let n = calls().len();
        sim.step_tick();
        let c = calls();
        // Presses the game may see: new ones while it runs unpaused.
        let new_presses = raw_now & !prev_raw;
        for (input, events, _) in &c[n..] {
            let started = events & 1 != 0;
            let resumed = events & 2 != 0;
            if started {
                hide = raw_now;
                prev_held = 0;
            } else if resumed {
                hide = raw_now;
            } else {
                hide &= raw_now;
                // A button pressed since the last call is new: count it.
                if page_before == Page::Game && !paused_before {
                    presses_made += (new_presses & !hide).count_ones();
                }
            }
            let want = raw_now & !hide;
            assert_eq!(input.players[0].held, want, "seed {seed} tick {}", sim.tick);
            assert_eq!(input.players[0].held & !button::ALL, 0);
            assert_eq!(
                input.players[0].pressed,
                input.players[0].held & !prev_held,
                "seed {seed} tick {}",
                sim.tick
            );
            assert_eq!(
                input.players[0].released,
                prev_held & !input.players[0].held
            );
            presses_seen += input.players[0].pressed.count_ones();
            prev_held = input.players[0].held;
        }
        if c.len() == n && sim.console.page() == Page::Game && !sim.console.game().paused() {
            panic!(
                "seed {seed} tick {}: a 30 fps game was not called",
                sim.tick
            );
        }
        if sim.console.page() == Page::Game {
            // Any game call since a pause is checked through `resumed`.
        }
        // The whole panel, every tick. While home is read again after a
        // game, nothing is sent and the panel keeps the last picture.
        let bad = if sim.console.home_pending() {
            assert_eq!(sim.console.last_report().areas, 0);
            0
        } else {
            stale_pixels(&sim)
        };
        assert_eq!(
            bad, 0,
            "seed {seed} tick {}: {bad} stale panel pixels",
            sim.tick
        );
        prev_raw = raw_now;
        // Back home: start again with a confirm press (as an event of
        // the next tick, so the model sees the start).
        if sim.console.page() == Page::Home && rng.below(4) == 0 {
            if sim.board.pad.state.buttons & SOUTH != 0 {
                sim.input(Ev::Release(SOUTH));
            } else {
                sim.input(Ev::Press(SOUTH));
            }
        }
    }
    (presses_made, presses_seen)
}

#[test]
fn random_play_every_input_reaches_the_game_once_and_the_panel_is_never_stale() {
    let mut made = 0;
    let mut seen = 0;
    let ticks = length(400, 1500);
    for seed in 1..=4u64 {
        let (m, s) = input_run(seed * 7919, ticks.into());
        made += m;
        seen += s;
    }
    println!("{made} presses made while a game ran, {seen} seen by the game");
    // About 0.2 presses reach a running game per tick.
    assert!(made > ticks / 2, "the runs pressed enough buttons ({made})");
    assert_eq!(made, seen);
}

// ---- 4. The picture after a notice and the pause menu ---------------------

#[test]
fn after_a_notice_and_the_menu_the_panel_is_exactly_the_game_picture() {
    for on_demand in [false, true] {
        let mut sim = start_toy();
        if on_demand {
            TODO.with(|t| t.set(Do::Rate(rate::ON_DEMAND)));
        }
        sim.idle(3);
        let clean = sim.ticks.last().unwrap().print;
        // A notice; then the menu while the notice still shows; the
        // notice leaves while paused; resume.
        assert!(sim.notify("HELLO THERE"));
        sim.idle(20);
        sim.tap_button(HOME);
        assert!(sim.console.game().paused());
        sim.idle(120);
        assert!(sim.console.notices().is_empty());
        assert!(sim.notify("ONE MORE"));
        sim.idle(5);
        sim.tap_button(EAST);
        assert!(!sim.console.game().paused());
        sim.idle(120);
        assert!(sim.console.notices().is_empty());
        let after = sim.ticks.last().unwrap().print;
        assert_eq!(after, clean, "on demand {on_demand}: the panel differs");
        assert_eq!(stale_pixels(&sim), 0);
    }
}

// ---- 5. Many starts and ends, every way ------------------------------------

#[test]
fn many_starts_and_ends_every_way_leave_no_trace() {
    let mut sim = start_toy();
    let home_colours: Vec<u16> = Vec::new();
    let mut home_colours = Some(home_colours);
    let mut rng = Rng(42);
    let mut ways = [0u32; 6];
    let rounds = length(90, 300);
    for round in 0..rounds {
        if sim.console.page() != Page::Game {
            sim.tap_button(SOUTH);
        }
        if sim.console.page() != Page::Game {
            // Started and stopped at once (a panic in start): again.
            assert!(PANIC_START.with(|p| p.get()), "round {round}");
            PANIC_START.with(|p| p.set(false));
            sim.idle(1);
            sim.tap_button(SOUTH);
            assert_eq!(sim.console.page(), Page::Game, "round {round}");
        }
        PANIC_START.with(|p| p.set(false));
        PANIC_EXIT.with(|p| p.set(false));
        // Play a little, sometimes at another rate.
        if rng.below(3) == 0 {
            TODO.with(|t| t.set(Do::Rate(rng.below(3) as u16)));
        }
        sim.idle(rng.below(4));
        let way = rng.below(6) as usize;
        ways[way] += 1;
        match way {
            0 => {
                TODO.with(|t| t.set(Do::Done));
                sim.input(Ev::Press(EAST));
                sim.idle(1);
                sim.input(Ev::Release(EAST));
            }
            1 => {
                TODO.with(|t| t.set(Do::PanicInUpdate));
                sim.input(Ev::Press(EAST));
                sim.idle(1);
                sim.input(Ev::Release(EAST));
            }
            2 => back_to_home(&mut sim),
            3 => {
                PANIC_EXIT.with(|p| p.set(true));
                back_to_home(&mut sim);
            }
            4 => {
                // The next start panics in `start`.
                back_to_home(&mut sim);
                PANIC_START.with(|p| p.set(true));
            }
            _ => {
                // Pause and resume, then end with Done.
                sim.tap_button(HOME);
                sim.tap_button(EAST);
                TODO.with(|t| t.set(Do::Done));
                sim.input(Ev::Press(EAST));
                sim.idle(1);
                sim.input(Ev::Release(EAST));
            }
        }
        sim.idle(1);
        assert_eq!(sim.console.page(), Page::Home, "round {round} way {way}");
        assert!(!sim.console.game().running());
        // The console's colours are the same after every game.
        let pal: Vec<u16> = sim.console.video().palette().colours()[240..].to_vec();
        match &home_colours {
            Some(h) if h.is_empty() => home_colours = Some(pal),
            Some(h) => assert_eq!(&pal, h, "round {round}"),
            None => {}
        }
    }
    let faults = START_FAULTS.with(|s| s.borrow().clone());
    assert!(faults.is_empty(), "{faults:?}");
    assert!(ways.iter().all(|&w| w > rounds / 15), "{ways:?}");
    assert_eq!(sim.board.games.started, sim.board.games.ended);
}

// ---- 6. A raw game that writes nonsense into every field -------------------

thread_local! {
    static WILD_CALLS: Cell<u32> = const { Cell::new(0) };
}

extern "C" fn wild_update(f: *mut GameFrame) -> u32 {
    WILD_CALLS.with(|c| c.set(c.get() + 1));
    // SAFETY: the console hands its frame for this call.
    let f = unsafe { &mut *f };
    // Every console colour, and the log length far past the buffer.
    for i in 0..256 {
        // SAFETY: the palette has 256 entries.
        unsafe { f.palette.add(i).write(0x1234) };
    }
    // SAFETY: `log_cap` bytes are the game's to write.
    unsafe { core::ptr::write_bytes(f.log, 0xC3, usize::from(f.log_cap)) };
    f.log_len = u16::MAX;
    f.wake_at_us = u64::MAX;
    f.requests = u32::MAX;
    f.ask_mode = u16::MAX;
    f.ask_rate = u16::MAX;
    f.ask_hint = u16::MAX;
    f.ask_colour = u16::MAX;
    // Fields only the console writes: changed too, must not matter.
    f.size = 0;
    f.version = 0;
    f.events = u32::MAX;
    f.frame = u32::MAX;
    f.time_us = u64::MAX;
    f.pixels = core::ptr::null_mut();
    if WILD_CALLS.with(|c| c.get()) > 40 {
        // An unknown status: counts as "aborted".
        return 77;
    }
    status::RUN
}

extern "C" fn wild_exit(_f: *mut GameFrame, _reason: u32) {}

static WILD: GameCallbacks = GameCallbacks {
    size: size_of::<GameCallbacks>() as u32,
    version: game_api::API_VERSION,
    // SAFETY: the wild functions write only inside the frame's buffers.
    update: unsafe { game_api::UpdateFn::new(wild_update) },
    exit: unsafe { game_api::ExitFn::new(wild_exit) },
};

extern "C" fn wild_entry(_: *const ConsoleApi) -> *const GameCallbacks {
    &WILD
}

// SAFETY: a game made for these tests, keeping the contract.
static WILD_GAME: &[LinkedGame] = &[unsafe { LinkedGame::new("BLOCKS", wild_entry) }];

#[test]
fn a_raw_game_writing_nonsense_everywhere_cannot_hurt_the_console() {
    reset();
    WILD_CALLS.with(|c| c.set(0));
    let mut sim = Sim::with_games(Some(FOUR), WILD_GAME);
    sim.boot();
    sim.idle(2);
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Game);
    sim.notify("STILL HERE");
    sim.idle(10);
    // The console's colours are its own on the panel: the notice's gold.
    let gold = theme::rgb(theme::UI_COLOURS[3]);
    assert!((0..32).any(|y| sim.board.screen.image[y * 320 + 160] == gold));
    // Settings: nothing taken.
    assert_eq!(sim.console.game().rate(), rate::FPS30);
    // The log: at most the buffer, as '?'.
    let lines: Vec<&String> = sim
        .board
        .log
        .lines()
        .iter()
        .filter(|l| l.starts_with("game: "))
        .collect();
    assert!(!lines.is_empty());
    assert!(
        lines
            .iter()
            .all(|l| l.len() <= 6 + console_core::game::LOG_LINE)
    );
    // The frame counter and events still go as the console says.
    sim.idle(40);
    assert_eq!(sim.console.page(), Page::Home, "status 77 ends the game");
    assert!(
        sim.board
            .log
            .lines()
            .iter()
            .any(|l| l == "console: game stopped with an error")
    );
}

// ---- 7. Statics in a game -------------------------------------------------

/// A game's `static` lives on in the simulator from one run to the next,
/// while the chip loads the game afresh (its statics start again). The
/// kit cannot stop a game from using one (an atomic is safe code), so
/// the kit's README warns about it. This test pins down what the
/// simulator does, so a change is noticed.
mod keeper {
    use core::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    pub static RUNS: AtomicU32 = AtomicU32::new(0);
    pub static SEEN_AT_START: AtomicU32 = AtomicU32::new(0);

    pub struct Keeper;

    impl Game for Keeper {
        fn start(_ctx: &mut Ctx) -> Self {
            SEEN_AT_START.store(RUNS.fetch_add(1, Ordering::Relaxed), Ordering::Relaxed);
            Keeper
        }
        fn update(&mut self, _ctx: &mut Ctx) -> Flow {
            Flow::Done
        }
    }

    game_kit::game!(Keeper);
}

// SAFETY: a game made for these tests, keeping the contract.
static KEEPER: &[LinkedGame] = &[unsafe { LinkedGame::new("BLOCKS", keeper::game_entry) }];

#[test]
fn a_games_static_survives_a_restart_in_the_simulator_only() {
    use core::sync::atomic::Ordering;
    reset();
    let mut sim = Sim::with_games(Some(FOUR), KEEPER);
    sim.boot();
    sim.idle(2);
    sim.tap_button(SOUTH);
    sim.idle(2);
    sim.tap_button(SOUTH);
    // On the chip this would be 0 again (the data slot is loaded anew).
    assert_eq!(keeper::SEEN_AT_START.load(Ordering::Relaxed), 1);
}

// ---- 8. Pause and the game clock ------------------------------------------

/// Pause stops the game clock (game time does not move while the menu
/// shows). The clock the console gives is the only one a game has, so an
/// idle game cannot tell how long it was paused.
#[test]
fn the_game_clock_stops_while_paused() {
    let mut sim = start_toy();
    sim.idle(5);
    let before = calls().last().unwrap().2;
    sim.tap_button(HOME);
    sim.idle(300); // 10 s paused
    sim.tap_button(EAST);
    let after = calls().iter().find(|c| c.1 & 2 != 0).unwrap().2;
    assert!(after - before < 4 * FRAME_US, "{before} -> {after}");
}

// ---- 9. The console on its own still works ---------------------------------

#[test]
fn console_new_step_notify_still_work_with_no_games() {
    let mut board = board_api::fake::FakeBoard::new();
    let mut c = Box::new(Console::new());
    for i in 0..200u64 {
        board.clock.now_us = i * FRAME_US;
        let _ = c.step(&mut board);
        if i == 100 {
            assert!(c.notify("HI", i * FRAME_US));
        }
    }
    assert_ne!(c.page(), Page::Boot);
    let _ = event::START;
}
