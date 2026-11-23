//! The second reader's cases for step 4 (drag scrolling, the brightness
//! setting and the console menu): things the first tests did not try.
//! The fake board, the fake clock, no window.
//!
//! Positions are picture pixels (the test board doubles them for the
//! panel). Tile `i` starts at 14 + 44 i - scroll; the row band is
//! y 19..61.

mod common;

use std::cell::Cell;

use board_api::Wake;
use common::{DOWN, EAST, Ev, FOUR, HOME, LEFT, NINE, RIGHT, SOUTH, Sim, UP};
use console_core::game_api::LinkedGame;
use console_core::home::STRIDE;
use console_core::pause::{self, BAR_W, Kind, bar_x, item_y};
use console_core::theme::{
    BACKLIGHT, BRIGHT_STEPS, DRAG_START, GEAR_X, GEAR_Y, TAP_MAX_US, TILE, TILE_LEFT, TILE_MARGIN,
    TILE_Y, TOUCH_GRACE,
};
use console_core::{FRAME_US, Page};
use game_kit::{Ctx, Flow, Game};

/// A picture row on the tiles.
const ROW: u16 = (TILE_Y + 10) as u16;
/// The middle of the gear in the bar.
const GEAR: (u16, u16) = ((GEAR_X + 4) as u16, (GEAR_Y + 4) as u16);
/// The frame round a tile (gap and width).
const EDGE: i16 = 3;

fn scroll(sim: &Sim) -> i16 {
    sim.console.home().scroll()
}

/// Where tile `i` is on screen now (left edge).
fn tile_x(sim: &Sim, i: usize) -> i16 {
    TILE_LEFT + i as i16 * STRIDE - scroll(sim)
}

/// A resting place: a tile at the left margin, or the far end.
fn is_rest(sim: &Sim) -> bool {
    let s = scroll(sim);
    (s - (TILE_LEFT - TILE_MARGIN)) % STRIDE == 0 || s == sim.console.home().max_scroll()
}

/// The selected tile, with its frame, is fully on screen.
fn selected_shown(sim: &Sim) -> bool {
    let x = tile_x(sim, sim.console.home().selected());
    x - EDGE >= 0 && x + TILE as i16 + EDGE <= 160
}

/// The finger at `x`, `y`: down, or moved there. One tick.
fn finger(sim: &mut Sim, x: u16, y: u16) {
    sim.input(Ev::Touch(x, y));
    sim.step_tick();
}

/// The finger lifts; runs the grace frames too.
fn lift(sim: &mut Sim) {
    sim.input(Ev::Lift);
    sim.step_tick();
    sim.idle(u64::from(TOUCH_GRACE));
}

/// Runs ticks until the row rests.
fn settle(sim: &mut Sim) {
    let mut n = 0;
    while sim.console.home().moving() || sim.console.home().settling() {
        sim.step_tick();
        n += 1;
        assert!(n < 60, "the row never comes to rest");
    }
}

/// A card: the game folders and their files.
type Card = &'static [(&'static str, &'static [u8])];

fn home(card: Card) -> Sim {
    let mut sim = Sim::new(Some(card));
    sim.boot();
    sim.idle(10);
    sim
}

/// Steps in the last `n` ticks.
fn steps(sim: &Sim, n: usize) -> u32 {
    sim.ticks[sim.ticks.len() - n..]
        .iter()
        .map(|t| t.steps)
        .sum()
}

/// How many times home tried to start a game (no game is linked into the
/// test board, so each try logs this line).
fn tries(sim: &Sim) -> usize {
    sim.board
        .log
        .lines()
        .iter()
        .filter(|l| l.as_str() == "home: games cannot start yet")
        .count()
}

/// A card with `n` games, GAME00 to GAMEnn.
fn card_of(n: usize) -> Card {
    let files: Vec<(&'static str, &'static [u8])> = (0..n)
        .map(|i| {
            let path: &'static str =
                Box::leak(format!("/CONSOLE/GAMES/GAME{i:02}/GAME.BIN").into_boxed_str());
            (path, &b"GAME"[..])
        })
        .collect();
    Box::leak(files.into_boxed_slice())
}

// ---- Faults found (each test failed before its fix) -------------------------

/// A finger goes down on a tile that is NOT selected while the row still
/// scrolls after a button press. By the lift the row has moved on, and the
/// selected tile is now under the point where the finger went down. That
/// tap must not start the selected game: the finger never touched it.
#[test]
fn a_tap_on_a_tile_the_row_moved_away_starts_nothing() {
    let mut sim = home(NINE);
    for _ in 0..3 {
        sim.tap_button(RIGHT);
    }
    settle(&mut sim);
    assert_eq!(sim.console.home().selected(), 3);
    assert_eq!(scroll(&sim), 36);
    // RIGHT: tile 4 is chosen and the row starts to scroll (36 -> 80).
    sim.input(Ev::Press(RIGHT));
    sim.step_tick();
    sim.input(Ev::Release(RIGHT));
    assert_eq!(sim.console.home().selected(), 4);
    let shown = scroll(&sim);
    assert!(shown < 70, "still scrolling: {shown}");
    // The finger goes down on tile 3, as the screen shows it now.
    let x = 120;
    let left3 = tile_x(&sim, 3);
    assert!(
        (left3..left3 + TILE as i16).contains(&(x as i16)),
        "x {x} is on tile 3 ({left3})"
    );
    finger(&mut sim, x, ROW);
    sim.idle(2);
    // By now tile 4 is under that point.
    let left4 = tile_x(&sim, 4);
    assert!(
        (left4 - EDGE..left4 + TILE as i16 + EDGE).contains(&(x as i16)),
        "tile 4 ({left4}) moved under the finger"
    );
    lift(&mut sim);
    sim.idle(2);
    assert_eq!(tries(&sim), 0, "a game started from a tap on another tile");
    assert_eq!(
        sim.console.home().selected(),
        3,
        "the tap chose the tile the finger went down on"
    );
}

/// The console menu opens (pad Home) while a finger drags the row. The
/// menu takes the finger away, so no release ever comes. The row must
/// still come to a resting place, as after any drag.
#[test]
fn the_row_rests_when_the_menu_takes_the_finger_mid_drag() {
    let mut sim = home(NINE);
    finger(&mut sim, 140, ROW);
    finger(&mut sim, 130, ROW);
    finger(&mut sim, 120, ROW);
    assert_eq!(scroll(&sim), 20);
    // Home on the pad, the finger still down and still.
    sim.tap_button(HOME);
    assert!(sim.console.menu_open());
    lift(&mut sim);
    sim.idle(20);
    sim.tap_button(EAST);
    assert!(!sim.console.menu_open());
    settle(&mut sim);
    assert!(
        is_rest(&sim),
        "the row stopped half way: scroll {}",
        scroll(&sim)
    );
    assert!(selected_shown(&sim));
    assert_eq!(tries(&sim), 0);
}

// ---- Random gestures --------------------------------------------------------

/// A small seeded random source (xorshift64*), so every run is the same.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// True `per_mille` times in a thousand.
    fn chance(&mut self, per_mille: u64) -> bool {
        self.below(1000) < per_mille
    }
}

const PLUS: u32 = board_api::btn::PLUS;
const BUTTONS: [u32; 8] = [LEFT, RIGHT, SOUTH, HOME, EAST, UP, DOWN, PLUS];
/// The buttons that confirm on home (start the selected game).
const CONFIRM: u32 = SOUTH | PLUS;

/// One touch as the console read it: where it went down, how far it went
/// from there (the larger axis), the row and selection at that time,
/// whether the row was gliding then, when it went down and when a read
/// last missed it (its lift), and how many reads in a row it has been
/// missing.
#[derive(Clone, Copy, Debug)]
struct Contact {
    down: (u16, u16),
    far: u16,
    scroll: i16,
    sel: usize,
    glide: bool,
    down_us: u64,
    up_us: u64,
    lost: u8,
}

/// The tile under picture column `x` with the row at `scroll` (frame
/// included).
fn tile_under(x: u16, scroll: i16, count: usize) -> Option<usize> {
    let x = x as i16;
    (0..count).find(|&i| {
        let left = TILE_LEFT + i as i16 * STRIDE - scroll;
        x >= left - EDGE && x < left + TILE as i16 + EDGE
    })
}

fn on_row(y: u16) -> bool {
    let y = y as i16;
    y >= TILE_Y - EDGE && y < TILE_Y + TILE as i16 + EDGE
}

/// A random point; half of them on the tiles' band.
fn random_point(rng: &mut Rng) -> (u16, u16) {
    let x = rng.below(160) as u16;
    let y = if rng.chance(500) {
        (TILE_Y - 4 + rng.below(TILE as u64 + 8) as i16) as u16
    } else {
        rng.below(120) as u16
    };
    (x, y)
}

/// Random fingers and buttons for `ticks` frames, the console stepped at
/// every frame (so the test knows what it read). After every frame:
/// - the scroll stays inside the row and the selection inside the list;
/// - once the row rests and no finger is down, the selected tile is
///   fully on screen;
/// - a game is tried only from home with its menu closed, and only for a
///   confirm press or a real tap: a touch that moved less than
///   `DRAG_START`, was held at most `TAP_MAX_US`, went down on the row
///   while it was not gliding, on the tile that was selected then and
///   is selected now, and ended `TOUCH_GRACE` reads after the finger
///   was last seen (a finger back within the grace is the same touch).
///
/// Then everything is let go: after the notices are gone, the console
/// sleeps (few steps a second), also with a finger resting on the screen.
fn fuzz(seed: u64, card: Option<Card>, ticks: u64) {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut sim = Sim::new(card);
    let ctx = |t: u64| format!("seed {seed}, tick {t}");
    let mut finger: Option<(u16, u16)> = None;
    // Reads with the finger missing (noise): it comes back after them.
    let mut back: Option<((u16, u16), u8)> = None;
    // Frames a finger stays still before it lifts (0: it moves about).
    let mut still: u32 = 0;
    let mut held: u32 = 0;
    // What the console read last: the touch, the pad buttons, BOOT.
    let mut seen_buttons: u32 = 0;
    let mut seen_boot = false;
    let mut contact: Option<Contact> = None;
    let mut boot_down = false;
    for t in 0..ticks {
        // ---- This frame's input.
        if let Some((p, gap)) = back.take() {
            if gap == 0 {
                finger = Some(p);
                sim.input(Ev::Touch(p.0, p.1));
            } else {
                back = Some((p, gap - 1));
                if rng.chance(150) {
                    // The board saw it touch again in between.
                    sim.input(Ev::QuickTouch(p.0, p.1));
                }
            }
        } else if let Some((x, y)) = finger {
            let resting = still > 0;
            still = still.saturating_sub(1);
            if (!resting && rng.chance(100)) || (resting && still == 0) {
                finger = None;
                sim.input(Ev::Lift);
            } else if rng.chance(40) {
                finger = None;
                // Mostly one read missing; sometimes one more than the
                // grace (then it is a lift and a new touch).
                let gap = if rng.chance(800) { 0 } else { TOUCH_GRACE };
                back = Some(((x, y), gap));
                sim.input(Ev::Lift);
            } else if !resting && rng.chance(700) {
                let r: i32 = if rng.chance(150) { 40 } else { 6 };
                let dx = rng.below(2 * r as u64 + 1) as i32 - r;
                let dy = rng.below(5) as i32 - 2;
                let p = (
                    (x as i32 + dx).clamp(0, 159) as u16,
                    (y as i32 + dy).clamp(0, 119) as u16,
                );
                finger = Some(p);
                sim.input(Ev::Touch(p.0, p.1));
            }
        } else if rng.chance(100) {
            let p = random_point(&mut rng);
            finger = Some(p);
            sim.input(Ev::Touch(p.0, p.1));
            // Some fingers rest, about as long as a tap may last.
            if rng.chance(250) {
                still = 8 + rng.below(20) as u32;
            }
        } else if rng.chance(30) {
            let p = random_point(&mut rng);
            sim.input(Ev::QuickTouch(p.0, p.1));
        }
        if rng.chance(60) {
            let b = BUTTONS[rng.below(8) as usize];
            held |= b;
            sim.input(Ev::Press(b));
        }
        if held != 0 && rng.chance(300) {
            sim.input(Ev::Release(held));
            held = 0;
        }
        if rng.chance(15) {
            sim.input(Ev::Quick(BUTTONS[rng.below(8) as usize]));
        }
        if rng.chance(6) {
            sim.input(Ev::QuickBoot);
        }
        if boot_down || rng.chance(4) {
            boot_down = !boot_down;
            sim.input(Ev::Boot(boot_down));
        }
        if rng.chance(8) {
            sim.console.notify("HELLO", 0);
        }
        if rng.chance(3) {
            let on = !sim.board.files.inserted;
            sim.input(Ev::Card(on));
        }
        if rng.chance(2) {
            let on = !sim.board.pad.state.connected;
            sim.input(Ev::Pad(on));
            held = 0;
        }

        // ---- What the console will read (if it reads).
        let connected = sim.board.pad.state.connected;
        let buttons = if connected {
            sim.board.pad.state.buttons
        } else {
            0
        };
        let noted_buttons = if connected { sim.board.pad.pressed } else { 0 };
        let boot = sim.board.keys.held & board_api::key::BOOT != 0;
        let boot_noted = sim.board.keys.pressed & board_api::key::BOOT != 0;
        let noted_tap = sim.board.touch.tap.map(|p| ((p.x - 1) / 2, (p.y - 1) / 2));
        let now = finger;
        let before = (
            sim.console.page(),
            sim.console.menu_open(),
            scroll(&sim),
            sim.console.home().selected(),
            tries(&sim),
        );
        let gliding = sim.console.home().settling();

        // ---- One step, at this frame's time.
        let now_us = t * FRAME_US;
        sim.board.clock.now_us = now_us;
        let frames = sim.console.frames();
        sim.console.step(&mut *sim.board);
        if sim.console.frames() == frames {
            continue; // not due: nothing read
        }
        let confirm = ((buttons & !seen_buttons) | noted_buttons) & CONFIRM != 0
            || (boot && !seen_boot)
            || boot_noted;
        seen_buttons = buttons;
        seen_boot = boot;

        // ---- The touch as the console read it.
        let fresh = |q: (u16, u16)| Contact {
            down: q,
            far: 0,
            scroll: before.2,
            sel: before.3,
            glide: gliding,
            down_us: now_us,
            up_us: now_us,
            lost: 0,
        };
        let mut ended: Option<Contact> = None;
        match (&mut contact, now) {
            (None, Some(p)) => contact = Some(fresh(p)),
            (Some(c), Some(p)) => {
                // Still down, or back within the grace: the same touch.
                let far = p.0.abs_diff(c.down.0).max(p.1.abs_diff(c.down.1));
                c.far = c.far.max(far);
                c.lost = 0;
            }
            (Some(c), None) => {
                if noted_tap.is_some() {
                    // Missing, but the board saw it touch again.
                    c.lost = 0;
                } else {
                    if c.lost == 0 {
                        c.up_us = now_us; // the lift, if it stays away
                    }
                    c.lost += 1;
                    if c.lost > TOUCH_GRACE {
                        ended = contact.take();
                    }
                }
            }
            (None, None) => ended = noted_tap.map(fresh),
        }

        // ---- The rules.
        let h = sim.console.home();
        if sim.console.page() != Page::Home {
            continue;
        }
        let count = h.count();
        if count > 0 {
            assert!(h.selected() < count, "{}", ctx(t));
            assert!(
                (0..=h.max_scroll()).contains(&h.scroll()),
                "{}: scroll {} outside 0..={}",
                ctx(t),
                h.scroll(),
                h.max_scroll()
            );
            if !h.moving()
                && !h.settling()
                && finger.is_none()
                && back.is_none()
                && contact.is_none()
            {
                assert!(
                    selected_shown(&sim),
                    "{}: tile {} left off screen at scroll {}",
                    ctx(t),
                    h.selected(),
                    h.scroll()
                );
            }
        }
        if tries(&sim) > before.4 {
            assert_eq!(
                before.0,
                Page::Home,
                "{}: a game from the boot screen",
                ctx(t)
            );
            assert!(!before.1, "{}: a game started from the open menu", ctx(t));
            if !confirm {
                let c = ended.unwrap_or_else(|| panic!("{}: a game with no press, no tap", ctx(t)));
                let sel = h.selected();
                assert!(
                    c.far < DRAG_START,
                    "{}: a drag started a game: {c:?}",
                    ctx(t)
                );
                assert!(
                    c.up_us - c.down_us <= TAP_MAX_US,
                    "{}: a press held too long started a game: {c:?}",
                    ctx(t)
                );
                assert!(
                    !c.glide,
                    "{}: the touch that stopped a glide started a game: {c:?}",
                    ctx(t)
                );
                assert!(on_row(c.down.1), "{}: a tap off the row: {c:?}", ctx(t));
                assert_eq!(
                    tile_under(c.down.0, c.scroll, count),
                    Some(sel),
                    "{}: the finger went down on another tile: {c:?}",
                    ctx(t)
                );
                assert!(
                    c.sel == sel && before.3 == sel,
                    "{}: tile {sel} was not selected first: {c:?}",
                    ctx(t)
                );
            }
        }
    }

    // ---- Everything let go: the console goes back to sleep.
    sim.tick = ticks;
    sim.input(Ev::Lift);
    sim.input(Ev::Release(u32::MAX));
    sim.input(Ev::Boot(false));
    sim.idle(450); // 15 s: every notice is gone
    sim.idle(90);
    assert!(
        steps(&sim, 90) <= 4,
        "seed {seed}: {} steps in 3 s with nothing going on",
        steps(&sim, 90)
    );
    // A finger resting on the screen.
    sim.input(Ev::Touch(80, ROW));
    sim.idle(30);
    sim.idle(90);
    assert!(
        steps(&sim, 90) <= 4,
        "seed {seed}: {} steps in 3 s with a resting finger",
        steps(&sim, 90)
    );
}

#[test]
fn random_gestures_keep_the_rules() {
    let cards: [Option<Card>; 7] = [
        None,
        Some(common::EMPTY),
        Some(card_of(1)),
        Some(card_of(2)),
        Some(card_of(7)),
        Some(card_of(32)),
        Some(NINE),
    ];
    let runs: u64 = std::env::var("FUZZ_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        // Two runs keep the PC check quick. For a long hunt:
        // FUZZ_RUNS=1500 cargo test --release --test attack_step4 random
        .unwrap_or(2);
    for seed in 0..runs {
        for (k, card) in cards.iter().enumerate() {
            fuzz(seed * 16 + k as u64, *card, 900);
        }
    }
}

// ---- A test game ------------------------------------------------------------

thread_local! {
    /// The game's time at its last call, and its calls.
    static GAME_TIME: Cell<u64> = const { Cell::new(0) };
    static GAME_CALLS: Cell<u32> = const { Cell::new(0) };
}

mod clocked {
    use super::*;

    /// Runs for ever; notes its time and its calls.
    pub struct Clocked;

    impl Game for Clocked {
        fn start(_ctx: &mut Ctx) -> Self {
            Clocked
        }

        fn update(&mut self, ctx: &mut Ctx) -> Flow {
            GAME_TIME.with(|t| t.set(ctx.time_us()));
            GAME_CALLS.with(|c| c.set(c.get() + 1));
            Flow::Run
        }
    }

    game_kit::game!(Clocked);
}

// SAFETY: made by `game_kit::game!`.
static CLOCKED: &[LinkedGame] = &[unsafe { LinkedGame::new("BLOCKS", clocked::game_entry) }];

/// FOUR's games, BLOCKS (the first tile) linked in, home showing.
fn home_with_game() -> Sim {
    GAME_CALLS.with(|c| c.set(0));
    let mut sim = Sim::with_games(Some(FOUR), CLOCKED);
    sim.boot();
    sim.idle(10);
    sim
}

/// BLOCKS running.
fn in_game() -> Sim {
    let mut sim = home_with_game();
    sim.tap_button(SOUTH);
    sim.idle(2);
    assert_eq!(sim.console.page(), Page::Game);
    sim
}

fn count_lines(sim: &Sim, line: &str) -> usize {
    sim.board.log.lines().iter().filter(|l| *l == line).count()
}

/// Where CLOSE is in the home menu (after BOOT ANIM with the feature
/// `boot-gather`).
fn close() -> usize {
    Kind::Home.index_of(pause::Item::Close).unwrap()
}

/// Moves the home menu's choice from BRIGHTNESS down to CLOSE.
fn to_close(s: &mut Sim) {
    for _ in 0..close() {
        s.tap_button(DOWN);
    }
}

/// The middle of item `i` of a menu.
fn item(kind: Kind, i: usize) -> (u16, u16) {
    (80, (item_y(kind, i) + 4) as u16)
}

/// The middle of the brightness bar for `step` in a menu.
fn bar(kind: Kind, step: u8) -> (u16, u16) {
    let i = kind.index_of(pause::Item::Bright).unwrap();
    (
        (bar_x(step) + BAR_W as i16 / 2) as u16,
        (item_y(kind, i) + 8) as u16,
    )
}

/// The backlight values the board got, in order.
fn levels(sim: &Sim) -> Vec<u8> {
    let s = &sim.board.screen;
    assert!(s.backlight_calls <= 32, "the log keeps 32");
    s.backlight_log[..s.backlight_calls]
        .iter()
        .map(|&(l, _)| l)
        .collect()
}

// ---- Taps everywhere else: exactly once -------------------------------------

#[test]
fn a_tap_note_on_the_gear_opens_and_closes_the_menu_once() {
    let mut sim = home(FOUR);
    sim.input(Ev::QuickTouch(GEAR.0, GEAR.1));
    sim.step_tick();
    assert!(sim.console.menu_open());
    sim.idle(5);
    assert!(sim.console.menu_open());
    sim.input(Ev::QuickTouch(GEAR.0, GEAR.1));
    sim.step_tick();
    sim.idle(5);
    assert!(!sim.console.menu_open());
    assert_eq!(count_lines(&sim, "console: menu"), 1);
    assert_eq!(count_lines(&sim, "console: menu closed"), 1);
    assert_eq!(tries(&sim), 0);
}

#[test]
fn a_gear_tap_and_home_in_the_same_frame_open_the_menu_once() {
    let mut sim = home(FOUR);
    sim.input(Ev::QuickTouch(GEAR.0, GEAR.1));
    sim.input(Ev::Press(HOME));
    sim.step_tick();
    sim.input(Ev::Release(HOME));
    sim.idle(5);
    assert!(sim.console.menu_open(), "not opened and closed again");
    assert_eq!(count_lines(&sim, "console: menu"), 1);
    // The same with a real finger: down, Home, lift on the gear.
    sim.tap_button(EAST);
    assert!(!sim.console.menu_open());
    sim.input(Ev::Touch(GEAR.0, GEAR.1));
    sim.step_tick();
    sim.input(Ev::Press(HOME));
    sim.input(Ev::Lift);
    sim.step_tick();
    sim.input(Ev::Release(HOME));
    sim.idle(5);
    assert!(sim.console.menu_open());
    assert_eq!(count_lines(&sim, "console: menu"), 2);
}

#[test]
fn tap_notes_in_the_home_menu_act_once() {
    let mut sim = home(FOUR);
    sim.tap_button(HOME);
    // A bar, only noted: one step, one backlight call.
    let (x, y) = bar(Kind::Home, 1);
    sim.input(Ev::QuickTouch(x, y));
    sim.step_tick();
    sim.idle(3);
    assert_eq!(sim.console.brightness(), 1);
    assert_eq!(levels(&sim), [255, BACKLIGHT[1]]);
    // CLOSE, only noted, with RIGHT in the same frame: closes once, and
    // neither the tap nor RIGHT reaches the row.
    let (x, y) = item(Kind::Home, close());
    sim.input(Ev::QuickTouch(x, y));
    sim.input(Ev::Quick(RIGHT));
    sim.step_tick();
    sim.idle(5);
    assert!(!sim.console.menu_open());
    assert_eq!(count_lines(&sim, "console: menu closed"), 1);
    assert_eq!(sim.console.home().selected(), 0);
    assert_eq!(tries(&sim), 0);
}

#[test]
fn tap_notes_in_the_menu_over_a_game_act_once() {
    let mut sim = in_game();
    sim.tap_button(HOME);
    assert!(sim.console.game().paused());
    // RESUME, only noted.
    let (x, y) = item(Kind::Game, 0);
    sim.input(Ev::QuickTouch(x, y));
    sim.step_tick();
    sim.idle(3);
    assert!(!sim.console.game().paused());
    assert_eq!(count_lines(&sim, "console: game resumed"), 1);
    // BACK TO HOME, only noted, with BOOT noted in the same frame.
    sim.tap_button(HOME);
    let (x, y) = item(Kind::Game, 2);
    sim.input(Ev::QuickTouch(x, y));
    sim.input(Ev::QuickBoot);
    sim.step_tick();
    sim.idle(5);
    assert_eq!(sim.console.page(), Page::Home);
    assert_eq!(sim.board.games.ended, 1);
    assert_eq!(count_lines(&sim, "console: back to home"), 1);
    assert_eq!(sim.board.games.started, 1, "nothing started again");
}

#[test]
fn a_tap_and_a_button_in_the_same_frame_both_count_on_home() {
    let mut sim = home(NINE);
    let x = (tile_x(&sim, 2) + 10) as u16;
    sim.input(Ev::QuickTouch(x, ROW));
    sim.input(Ev::Quick(RIGHT));
    sim.step_tick();
    sim.idle(3);
    assert_eq!(sim.console.home().selected(), 2, "the tap wins over RIGHT");
    assert_eq!(tries(&sim), 0, "tile 2 was not chosen before");
    // On the chosen tile with OK in the same frame: one try, not two.
    sim.input(Ev::QuickTouch(x, ROW));
    sim.input(Ev::Quick(SOUTH));
    sim.step_tick();
    sim.idle(3);
    assert_eq!(tries(&sim), 1);
}

// ---- Drag state must not leak -----------------------------------------------

#[test]
fn a_game_started_mid_drag_leaves_a_sane_row() {
    let mut sim = home_with_game();
    finger(&mut sim, 140, ROW);
    finger(&mut sim, 130, ROW);
    // A drag move and OK in the same frame: the game starts.
    sim.input(Ev::Touch(120, ROW));
    sim.input(Ev::Press(SOUTH));
    sim.step_tick();
    sim.input(Ev::Release(SOUTH));
    sim.idle(2);
    assert_eq!(sim.console.page(), Page::Game);
    assert_eq!(sim.board.games.started, 1);
    // The finger keeps moving in the game, then BACK TO HOME by pad.
    finger(&mut sim, 60, ROW);
    sim.tap_button(HOME);
    sim.tap_button(DOWN);
    sim.tap_button(DOWN);
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Home);
    // The row ends the glide the OK started (it was cut short when the
    // game started). The finger is still down; moved on home: nothing.
    settle(&mut sim);
    let at = scroll(&sim);
    finger(&mut sim, 20, ROW);
    finger(&mut sim, 150, ROW);
    assert_eq!(scroll(&sim), at, "the old finger does not drag home");
    lift(&mut sim);
    settle(&mut sim);
    sim.idle(5);
    assert!(is_rest(&sim) && selected_shown(&sim));
    assert_eq!(sim.board.games.started, 1);
    // Taps work again.
    sim.tap_at((tile_x(&sim, 1) + 10) as u16, ROW);
    assert_eq!(sim.console.home().selected(), 1);
}

#[test]
fn the_card_pulled_and_put_back_mid_drag() {
    // Since the tile pictures (2026-11-17): the card is never read while
    // the row moves or a finger is on it, so home sees a pulled card only
    // once the row is still again.
    let mut sim = home(NINE);
    finger(&mut sim, 150, ROW);
    finger(&mut sim, 140, ROW);
    sim.input(Ev::Card(false));
    // The finger drags on for over 2 s: no card check meanwhile.
    let mut x = 140;
    for _ in 0..70 {
        x = if x > 20 { x - 2 } else { 140 };
        finger(&mut sim, x, ROW);
    }
    assert_eq!(
        sim.console.home().count(),
        9,
        "not looked at while dragging"
    );
    lift(&mut sim);
    settle(&mut sim);
    sim.idle(3);
    assert_eq!(
        sim.console.home().count(),
        0,
        "NO CARD once the row is still"
    );
    assert_eq!(scroll(&sim), 0);
    sim.input(Ev::Card(true));
    // No games: the drag starts on nothing and the next check finds them.
    for _ in 0..70 {
        x = if x > 20 { x - 2 } else { 140 };
        finger(&mut sim, x, ROW);
    }
    lift(&mut sim);
    sim.idle(70);
    assert_eq!(sim.console.home().count(), 9, "the card is back");
    // Drag again with the games there, and let go.
    for x in [150, 140, 120, 100] {
        finger(&mut sim, x, ROW);
    }
    lift(&mut sim);
    settle(&mut sim);
    assert!(is_rest(&sim) && selected_shown(&sim));
    assert!(
        (0..=sim.console.home().max_scroll()).contains(&scroll(&sim)),
        "scroll {}",
        scroll(&sim)
    );
    assert_eq!(tries(&sim), 0);
}

#[test]
fn the_menu_opened_mid_glide_and_a_notice_mid_drag() {
    let mut sim = home(NINE);
    for x in [140, 125, 110, 95] {
        finger(&mut sim, x, ROW);
    }
    lift(&mut sim); // a flick: the row glides
    assert!(sim.console.home().settling());
    sim.tap_button(HOME);
    assert!(sim.console.menu_open());
    sim.notify("HELLO");
    sim.idle(30);
    assert!(is_rest(&sim) && selected_shown(&sim));
    sim.tap_button(EAST);
    settle(&mut sim);
    assert!(is_rest(&sim) && selected_shown(&sim));
    assert_eq!(tries(&sim), 0);
}

// ---- Brightness ---------------------------------------------------------------

#[test]
fn any_saved_step_gives_a_lit_screen() {
    for step in 0..=u8::MAX {
        let mut sim = Sim::new(Some(FOUR));
        sim.console.set_brightness(step);
        assert!(sim.console.brightness() < BRIGHT_STEPS);
        sim.step_tick();
        let b = sim.console.backlight();
        assert_ne!(b, 0, "step {step}");
        assert_eq!(b, BACKLIGHT[usize::from(step.min(BRIGHT_STEPS - 1))]);
        assert_eq!(sim.board.screen.backlight_calls, 1);
        assert!(
            !sim.console.take_settings_changed(),
            "not the player's change"
        );
    }
}

#[test]
fn random_menu_use_gives_one_call_per_change_and_never_0() {
    let mut rng = Rng(77);
    let mut sim = home(FOUR);
    sim.tap_button(HOME);
    let mut changes = 0;
    for _ in 0..400 {
        let was = sim.console.brightness();
        match rng.below(6) {
            0 => sim.tap_button(LEFT),
            1 => sim.tap_button(RIGHT),
            2 => sim.tap_button(SOUTH),
            3 => {
                let (x, y) = bar(Kind::Home, rng.below(5) as u8);
                sim.input(Ev::QuickTouch(x, y));
                sim.step_tick();
            }
            4 => {
                let (x, y) = bar(Kind::Home, rng.below(5) as u8);
                sim.tap_at(x, y);
            }
            _ => boot_press(&mut sim),
        }
        assert!(sim.console.menu_open());
        assert_eq!(sim.console.menu_item(), 0, "BRIGHTNESS stays chosen");
        if sim.console.brightness() != was {
            changes += 1;
            assert!(sim.console.take_settings_changed());
        } else {
            assert!(!sim.console.take_settings_changed());
        }
        if changes >= 30 {
            break;
        }
    }
    assert!(changes >= 30, "the menu changed the step {changes} times");
    let l = levels(&sim);
    assert_eq!(
        l.len(),
        1 + changes,
        "one call at start, then one per change"
    );
    assert!(l.iter().all(|v| BACKLIGHT.contains(v)), "{l:?}");
    assert!(
        l.windows(2).all(|w| w[0] != w[1]),
        "a call with no change: {l:?}"
    );
    assert_eq!(*l.last().unwrap(), sim.console.backlight());
}

fn boot_press(sim: &mut Sim) {
    sim.input(Ev::Boot(true));
    sim.step_tick();
    sim.input(Ev::Boot(false));
    sim.step_tick();
}

#[test]
fn the_ends_neither_wrap_nor_call() {
    let mut sim = home(FOUR);
    sim.tap_button(HOME);
    for _ in 0..3 {
        sim.tap_button(RIGHT);
    }
    assert_eq!(sim.console.brightness(), BRIGHT_STEPS - 1);
    assert_eq!(sim.board.screen.backlight_calls, 1);
    assert!(!sim.console.take_settings_changed());
    for _ in 0..10 {
        sim.tap_button(LEFT);
    }
    assert_eq!(sim.console.brightness(), 0);
    let calls = sim.board.screen.backlight_calls;
    assert_eq!(calls, 1 + usize::from(BRIGHT_STEPS - 1));
    sim.tap_button(LEFT);
    sim.tap_button(LEFT);
    assert_eq!(sim.console.brightness(), 0, "no wrap at the dimmest");
    assert_eq!(sim.board.screen.backlight_calls, calls);
}

#[test]
fn the_step_survives_games_and_menus() {
    let mut sim = home_with_game();
    sim.tap_button(HOME);
    sim.tap_button(LEFT);
    sim.tap_button(LEFT);
    sim.tap_button(LEFT);
    sim.tap_button(EAST);
    assert_eq!(sim.console.brightness(), 1);
    let calls = sim.board.screen.backlight_calls;
    // Into a game, its menu, out by BACK TO HOME, and again by pad.
    sim.tap_button(SOUTH);
    sim.idle(5);
    assert_eq!(sim.console.page(), Page::Game);
    sim.tap_button(HOME);
    sim.idle(5);
    sim.tap_button(HOME);
    sim.idle(5);
    sim.tap_button(HOME);
    sim.tap_button(DOWN);
    sim.tap_button(DOWN);
    sim.tap_button(SOUTH);
    sim.idle(5);
    assert_eq!(sim.console.page(), Page::Home);
    sim.tap_button(HOME);
    sim.tap_button(EAST);
    sim.idle(30);
    assert_eq!(sim.console.brightness(), 1);
    assert_eq!(sim.console.backlight(), BACKLIGHT[1]);
    assert_eq!(sim.board.screen.backlight_calls, calls, "no extra call");
}

#[test]
fn closing_the_home_menu_never_starts_a_game() {
    // Every way to close it, while BLOCKS (the first tile) is chosen.
    type Close = fn(&mut Sim);
    let closes: [(&str, Close); 9] = [
        ("B", |s| s.tap_button(EAST)),
        ("Home", |s| s.tap_button(HOME)),
        ("OK on CLOSE", |s| {
            to_close(s);
            s.tap_button(SOUTH);
        }),
        ("OK held on CLOSE", |s| {
            to_close(s);
            s.input(Ev::Press(SOUTH));
            s.idle(40);
            s.input(Ev::Release(SOUTH));
            s.step_tick();
        }),
        ("BOOT held on CLOSE", |s| {
            to_close(s);
            s.input(Ev::Boot(true));
            s.idle(40);
            s.input(Ev::Boot(false));
            s.step_tick();
        }),
        ("BOOT noted on CLOSE", |s| {
            to_close(s);
            s.input(Ev::QuickBoot);
            s.step_tick();
        }),
        ("a finger on CLOSE", |s| {
            let (x, y) = item(Kind::Home, close());
            s.input(Ev::Touch(x, y));
            s.idle(10);
            s.input(Ev::Lift);
            s.step_tick();
        }),
        ("a noted tap on CLOSE", |s| {
            let (x, y) = item(Kind::Home, close());
            s.input(Ev::QuickTouch(x, y));
            s.step_tick();
        }),
        ("the gear, then a lift on the tile", |s| {
            s.input(Ev::Touch(GEAR.0, GEAR.1));
            s.step_tick();
            // The finger slides up onto the chosen tile and lifts there.
            s.input(Ev::Touch(30, ROW));
            s.step_tick();
            s.input(Ev::Lift);
            s.step_tick();
        }),
    ];
    for (name, close) in closes {
        let mut sim = home_with_game();
        sim.tap_button(HOME);
        assert!(sim.console.menu_open(), "{name}");
        close(&mut sim);
        sim.idle(10);
        assert!(!sim.console.menu_open(), "{name}: still open");
        assert_eq!(sim.console.page(), Page::Home, "{name}: a game started");
        assert_eq!(sim.board.games.started, 0, "{name}");
    }
}

#[test]
fn the_game_clock_stops_while_its_menu_shows() {
    let mut sim = in_game();
    sim.idle(10);
    sim.tap_button(HOME);
    assert!(sim.console.game().paused());
    let (t, n) = (GAME_TIME.with(|t| t.get()), GAME_CALLS.with(|c| c.get()));
    // 3 s with the menu open, the step changed on the way.
    sim.tap_button(DOWN);
    sim.tap_button(LEFT);
    sim.idle(90);
    assert_eq!(GAME_CALLS.with(|c| c.get()), n, "not called while paused");
    sim.tap_button(UP);
    sim.tap_button(SOUTH);
    sim.idle(1);
    let after = GAME_TIME.with(|t| t.get());
    assert!(
        after > t && after - t < 10 * FRAME_US,
        "game time jumped {} us over the pause",
        after - t
    );
}

// ---- Sleeping -----------------------------------------------------------------

#[test]
fn a_finger_resting_in_the_open_menu_lets_the_console_sleep() {
    let mut sim = home(FOUR);
    sim.tap_button(HOME);
    let (x, y) = bar(Kind::Home, 2);
    finger(&mut sim, x, y);
    finger(&mut sim, x + 12, y);
    sim.idle(30);
    sim.idle(90);
    assert!(steps(&sim, 90) <= 3, "{} steps", steps(&sim, 90));
    let w = sim.console.step(&mut *sim.board);
    assert!(matches!(w, Wake::InputOr(_)));
    lift(&mut sim);
    sim.idle(30);
    sim.idle(90);
    assert!(steps(&sim, 90) <= 3, "{} steps", steps(&sim, 90));
}

// ---- More faults --------------------------------------------------------------

/// A finger already on the game when Home opens the menu over it. The
/// bars are drawn where the finger is; when it slides, the step must not
/// follow it: the finger went down on the game, not on the bars. (The
/// home screen's menu already forgets such a finger when it opens.)
#[test]
fn a_finger_down_before_the_game_menu_opened_does_not_slide_the_bars() {
    let mut sim = in_game();
    let (x0, y) = bar(Kind::Game, 0);
    finger(&mut sim, x0, y);
    sim.tap_button(HOME);
    assert!(sim.console.game().paused());
    let (x3, _) = bar(Kind::Game, 3);
    for x in (x0..=x3).step_by(3) {
        finger(&mut sim, x, y);
    }
    lift(&mut sim);
    sim.idle(3);
    assert_eq!(
        sim.console.brightness(),
        BRIGHT_STEPS - 1,
        "the finger from the game set the step"
    );
    assert_eq!(sim.board.screen.backlight_calls, 1);
    // A new finger on the bars still works.
    let (x1, _) = bar(Kind::Game, 1);
    sim.tap_at(x1, y);
    assert_eq!(sim.console.brightness(), 1);
}

// ---- Owner decisions D1, D4, D5 (2026-11-17) ----------------------------------

/// D1. One finger held still on a tile that is not selected; the noisy
/// panel loses ONE read in the middle. Before D1 that read counted as a
/// lift: a tap (the tile was chosen), then the finger "came back" as a
/// new touch, and its real lift was a second tap on the now chosen tile:
/// the game started. Now a press gets the drag's grace (`TOUCH_GRACE`).
#[test]
fn one_lost_read_during_a_press_does_not_start_a_game() {
    let mut sim = home(NINE);
    let x = (tile_x(&sim, 1) + 10) as u16;
    finger(&mut sim, x, ROW);
    sim.idle(3);
    sim.input(Ev::Lift); // one read with no finger
    sim.step_tick();
    finger(&mut sim, x, ROW); // the same finger, still there
    sim.idle(3);
    lift(&mut sim);
    sim.idle(3);
    assert_eq!(tries(&sim), 0, "one held finger started a game");
    assert_eq!(sim.console.home().selected(), 1, "it was one tap");
}

/// D1. The finger that closed the home menu (a `down` on the gear) is
/// taken by the console. One lost read must not end that: before D1 the
/// finger "came back" as a new touch, and its lift tapped the gear and
/// opened the menu again.
#[test]
fn the_finger_that_closed_the_menu_does_not_reopen_it_after_a_gap() {
    let mut sim = home(FOUR);
    sim.tap_button(HOME);
    assert!(sim.console.menu_open());
    finger(&mut sim, GEAR.0, GEAR.1);
    assert!(!sim.console.menu_open(), "a down on the gear closes it");
    for _ in 0..TOUCH_GRACE {
        sim.input(Ev::Lift); // a read with no finger
        sim.step_tick();
    }
    finger(&mut sim, GEAR.0, GEAR.1); // the same finger, still there
    sim.idle(3);
    lift(&mut sim);
    sim.idle(5);
    assert!(
        !sim.console.menu_open(),
        "the closing finger opened it again"
    );
    assert_eq!(count_lines(&sim, "console: menu"), 1);
}

/// D1. A real lift: exactly one tap, `TOUCH_GRACE` frames after it, even
/// after the console slept under a resting finger. A tap the board only
/// noted still acts at once.
#[test]
fn a_real_lift_gives_one_tap_one_grace_later() {
    let mut sim = home(NINE);
    let x = (tile_x(&sim, 1) + 10) as u16;
    finger(&mut sim, x, ROW);
    sim.idle(10); // resting: the console sleeps (not held too long)
    assert!(steps(&sim, 5) <= 1, "{} steps", steps(&sim, 5));
    sim.input(Ev::Lift);
    sim.step_tick();
    for _ in 1..=TOUCH_GRACE {
        assert_eq!(sim.console.home().selected(), 0, "not yet: grace");
        sim.step_tick();
    }
    assert_eq!(sim.console.home().selected(), 1, "the tap, one grace late");
    sim.idle(10);
    assert_eq!(tries(&sim), 0, "one tap only");
    // The same lift again on the now chosen tile: one try, no more.
    sim.tap_at(x, ROW);
    sim.idle(10);
    assert_eq!(tries(&sim), 1);
    // A noted tap (down and up inside one frame) acts in its frame.
    let x2 = (tile_x(&sim, 2) + 10) as u16;
    sim.input(Ev::QuickTouch(x2, ROW));
    sim.step_tick();
    assert_eq!(sim.console.home().selected(), 2, "a noted tap acts at once");
}

/// A flick on NINE: the row glides afterwards.
fn glide(sim: &mut Sim) {
    for x in [140, 125, 110, 95] {
        finger(sim, x, ROW);
    }
    lift(sim);
    assert!(sim.console.home().settling(), "a glide");
    sim.step_tick();
    assert!(sim.console.home().settling(), "still gliding");
}

/// D4. A finger on the row while it glides stops it where it is, and
/// that touch is no tap: lifted, it neither selects nor starts, and the
/// row settles on the nearest resting place. Before D4 the tap chose
/// (or started) the tile under it.
#[test]
fn a_finger_on_a_gliding_row_stops_it_and_taps_nothing() {
    for on_selected in [false, true] {
        let mut sim = home(NINE);
        glide(&mut sim);
        let sel = sim.console.home().selected();
        // On the selected tile (a tap there would start it), or on the
        // next one (a tap there would choose it).
        let i = if on_selected { sel } else { sel + 1 };
        let x = (tile_x(&sim, i) + 10) as u16;
        finger(&mut sim, x, ROW);
        let stopped = scroll(&sim);
        sim.idle(20);
        assert_eq!(scroll(&sim), stopped, "the row stays where it stopped");
        assert!(!sim.console.home().moving());
        assert!(steps(&sim, 10) <= 1, "nothing moves: the console sleeps");
        lift(&mut sim);
        settle(&mut sim);
        sim.idle(5);
        assert_eq!(tries(&sim), 0, "the stopping touch started a game");
        assert!(is_rest(&sim) && selected_shown(&sim));
        assert!(
            (scroll(&sim) - stopped).abs() <= STRIDE / 2 + 1,
            "the nearest resting place: {stopped} -> {}",
            scroll(&sim)
        );
        if !on_selected {
            assert_ne!(sim.console.home().selected(), i, "it chose a tile");
        }
    }
}

/// D4. The same with a short touch (also one the board only noted, and
/// one shorter than a frame): no tap, the glide ends on a resting place.
#[test]
fn a_short_touch_on_a_gliding_row_is_no_tap_either() {
    for noted in [false, true] {
        let mut sim = home(NINE);
        glide(&mut sim);
        let sel = sim.console.home().selected();
        let x = (tile_x(&sim, sel) + 10) as u16;
        if noted {
            sim.input(Ev::QuickTouch(x, ROW));
            sim.step_tick();
        } else {
            sim.tap_at(x, ROW);
        }
        settle(&mut sim);
        sim.idle(5);
        assert_eq!(tries(&sim), 0, "noted {noted}: a game started");
        assert!(is_rest(&sim) && selected_shown(&sim));
    }
}

/// D4. Stopped at any frame of a glide (either way), the row stays
/// exactly where it stopped while the finger rests, even when the
/// selected tile is then near an edge.
#[test]
fn a_glide_stopped_at_any_frame_stays_still() {
    for back in [false, true] {
        for k in 0..8 {
            let mut sim = home(NINE);
            if back {
                for _ in 0..8 {
                    sim.tap_button(RIGHT);
                }
                settle(&mut sim);
                for x in [20, 35, 50, 65] {
                    finger(&mut sim, x, ROW);
                }
                lift(&mut sim);
            } else {
                // A fast flick: a long glide, the selection pulled along.
                for x in [150, 110, 70, 30] {
                    finger(&mut sim, x, ROW);
                }
                lift(&mut sim);
            }
            sim.idle(k);
            if !sim.console.home().settling() {
                continue;
            }
            finger(&mut sim, 80, ROW);
            let stopped = scroll(&sim);
            sim.idle(10);
            assert_eq!(scroll(&sim), stopped, "back {back}, frame {k}: it moved");
            lift(&mut sim);
            settle(&mut sim);
            assert!(
                is_rest(&sim) && selected_shown(&sim),
                "back {back}, frame {k}"
            );
            assert_eq!(tries(&sim), 0);
        }
    }
}

/// D4. Dragging after stopping a glide moves the row from where it
/// stopped, pixel for pixel.
#[test]
fn a_drag_takes_a_stopped_glide_from_where_it_stopped() {
    let mut sim = home(NINE);
    glide(&mut sim);
    let x = 100;
    finger(&mut sim, x, ROW);
    let stopped = scroll(&sim);
    sim.idle(2);
    finger(&mut sim, x - 10, ROW);
    assert_eq!(scroll(&sim), stopped + 10);
    finger(&mut sim, x - 14, ROW);
    assert_eq!(scroll(&sim), stopped + 14);
    sim.idle(5); // rest: no flick
    lift(&mut sim);
    settle(&mut sim);
    assert!(is_rest(&sim) && selected_shown(&sim));
    assert_eq!(tries(&sim), 0);
}

/// D4. A button while a finger keeps a glide stopped: the button wins
/// as during a drag (the row shows the new choice), and the lift after
/// it does nothing.
#[test]
fn a_button_while_a_finger_keeps_a_glide_stopped() {
    let mut sim = home(NINE);
    glide(&mut sim);
    let x = (tile_x(&sim, sim.console.home().selected()) + 10) as u16;
    finger(&mut sim, x, ROW);
    sim.idle(2);
    let sel = sim.console.home().selected();
    sim.tap_button(RIGHT);
    assert_eq!(sim.console.home().selected(), sel + 1);
    settle(&mut sim);
    assert!(is_rest(&sim) && selected_shown(&sim));
    let rest = scroll(&sim);
    lift(&mut sim);
    sim.idle(10);
    assert_eq!(sim.console.home().selected(), sel + 1);
    assert_eq!(scroll(&sim), rest, "the lift moved the row");
    assert_eq!(tries(&sim), 0);
}

/// D4 is for glides only: a finger off the row during a glide changes
/// nothing, and a row scrolling for a button is not a glide (see
/// `a_tap_on_a_tile_the_row_moved_away_starts_nothing`).
#[test]
fn a_finger_off_the_row_does_not_stop_a_glide() {
    let mut sim = home(NINE);
    glide(&mut sim);
    let s = scroll(&sim);
    finger(&mut sim, 80, 100);
    assert_ne!(scroll(&sim), s, "the glide goes on");
    lift(&mut sim);
    settle(&mut sim);
    assert!(is_rest(&sim) && selected_shown(&sim));
}

/// Frames a finger must be held to pass `TAP_MAX_US`.
const LONG: u64 = TAP_MAX_US / FRAME_US + 2;

/// D5. A press held longer than `TAP_MAX_US` is no tap: on a tile it
/// selects nothing, on the selected tile it starts nothing, on the gear
/// it opens nothing. Just under the limit it still is a tap.
#[test]
fn a_long_press_does_nothing() {
    let mut sim = home(NINE);
    let x1 = (tile_x(&sim, 1) + 10) as u16;
    let x0 = (tile_x(&sim, 0) + 10) as u16;
    for (x, y) in [(x1, ROW), (x0, ROW), GEAR] {
        finger(&mut sim, x, y);
        sim.idle(LONG);
        lift(&mut sim);
        sim.idle(3);
        assert_eq!(sim.console.home().selected(), 0, "at {x},{y}");
        assert_eq!(tries(&sim), 0, "at {x},{y}");
        assert!(!sim.console.menu_open(), "at {x},{y}");
    }
    // Held a little under the limit: a tap.
    finger(&mut sim, x1, ROW);
    sim.idle(TAP_MAX_US / FRAME_US - 2);
    lift(&mut sim);
    assert_eq!(sim.console.home().selected(), 1);
}

/// D5 leaves the menus alone: they act when the finger goes DOWN, so a
/// finger held long on a brightness bar or on CLOSE still works.
#[test]
fn a_long_press_still_works_in_the_menu() {
    let mut sim = home(FOUR);
    sim.tap_button(HOME);
    let (x, y) = bar(Kind::Home, 1);
    finger(&mut sim, x, y);
    sim.idle(LONG);
    lift(&mut sim);
    assert_eq!(sim.console.brightness(), 1);
    let (x, y) = item(Kind::Home, close());
    finger(&mut sim, x, y);
    assert!(!sim.console.menu_open(), "CLOSE acts on the down");
    sim.idle(LONG);
    lift(&mut sim);
    sim.idle(3);
    assert!(!sim.console.menu_open());
    assert_eq!(tries(&sim), 0);
}
