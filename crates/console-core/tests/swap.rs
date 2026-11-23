//! The whole console with the feature `swap` and a slow sender (a model
//! of the board's second core): `Console::step_with_sender` against
//! `Console::step`, in lockstep, with the same input.
//!
//! The run plays a game that draws only what moves and sometimes changes
//! a colour, opens the pause menu (whose brightness line changes the
//! backlight), gets notices drawn over the game, goes back home and
//! starts the game again. The slow sender holds each picture until the
//! next present (or until the test waits for it), reads it in pieces in
//! between, and checks every byte it reads against the picture as it was
//! handed over. The test fails when:
//!
//! - a byte the sender reads differs from the picture at hand-over time
//!   (someone wrote the picture it holds);
//! - the console hands over a picture while the sender still holds one;
//! - the picture the console (or its game) draws into is the held one;
//! - after a wait, the sender's panel differs from the panel of the
//!   console that sends at once;
//! - the screenshot picture (`video().frame()`) or the palette differs
//!   between the two consoles at any tick;
//! - the backlight is set while the sender still holds a picture, or the
//!   two consoles set different backlight levels;
//! - right after a hand-over, the picture the console goes on drawing in
//!   differs from the one handed over (no catch-up);
//! - after a wait, the panel (built from the changed areas only) differs
//!   from a fresh, whole render of the last picture handed over.
//!
//! With the feature on, both consoles use the swap `Video` (the direct
//! one through `Inline`), so comparing them alone would miss a fault both
//! share; the two checks above do not lean on the other console.

#![cfg(feature = "swap")]

mod common;

use board_api::fake::FakeScreen;
use board_api::{Rect, Screen, SendReport, btn};
use common::{EAST, Ev, FOUR, HOME, SOUTH, Sim, fingerprint};
use console_core::Page;
use console_core::console_video::{Frame, Handed, Palette, Sender, Source, View};
use console_core::game_api::LinkedGame;

/// A small, fixed random number maker (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ---- Two copies of one game (each copy keeps its own state) -----------

macro_rules! dot_game {
    ($name:ident) => {
        mod $name {
            use game_kit::{Ctx, Flow, Game};

            /// A dot that moves with the stick and the d-pad (through the
            /// notice band and under the pause menu too), a trail that
            /// stays, and now and then a new colour or a full redraw.
            pub struct Dot {
                x: i16,
                y: i16,
            }

            impl Game for Dot {
                fn start(ctx: &mut Ctx) -> Self {
                    ctx.screen().clear(30);
                    Dot { x: 80, y: 10 }
                }

                fn update(&mut self, ctx: &mut Ctx) -> Flow {
                    let f = ctx.frame();
                    if f % 97 == 50 {
                        ctx.screen().clear((f % 7) as u8 + 30);
                    }
                    if f % 41 == 7 {
                        ctx.set_colour(
                            (f % 16) as u8,
                            (f.wrapping_mul(2_654_435_761) >> 16) as u16,
                        );
                    }
                    let (lx, ly) = ctx.left_stick();
                    self.x = (self.x + lx / 8192).rem_euclid(157);
                    self.y = (self.y + ly / 8192 + 1).rem_euclid(117);
                    if ctx.held(game_kit::api::button::LEFT) {
                        self.x = (self.x + 150) % 157;
                    }
                    let c = (f % 200) as u8 + 1;
                    ctx.screen().rect(self.x, self.y, 3, 3, c);
                    Flow::Run
                }
            }

            game_kit::game!(Dot);
        }
    };
}

dot_game!(dot_a);
dot_game!(dot_b);

/// The first game of the FOUR card (sorted by name) is BLOCKS.
// SAFETY: games made for these tests, keeping the contract.
static GAME_A: &[LinkedGame] = &[unsafe { LinkedGame::new("BLOCKS", dot_a::game_entry) }];
static GAME_B: &[LinkedGame] = &[unsafe { LinkedGame::new("BLOCKS", dot_b::game_entry) }];

// ---- The slow sender --------------------------------------------------

/// What the sender keeps of one hand-over (see console-video's
/// tests/swap.rs for the same model, commented).
struct Held {
    at: *const Frame,
    snap: Box<Frame>,
    palette: Palette,
    view: View,
    border: u16,
    areas: Vec<Rect>,
    sent: usize,
    checked: usize,
}

struct Slow {
    held: Option<Held>,
    /// The palette, view and border of the last hand-over (for the fresh
    /// render).
    last: Option<(Box<Palette>, View, u16)>,
    /// The last picture handed over, as it was (kept after the wait too:
    /// the console may wait within the same step, before the backlight).
    last_snap: Option<Box<Frame>>,
    screen: Box<FakeScreen>,
    hand_overs: u32,
    bytes_read: u64,
}

impl Slow {
    fn new() -> Self {
        Slow {
            held: None,
            last: None,
            last_snap: None,
            screen: Box::new(FakeScreen::new()),
            hand_overs: 0,
            bytes_read: 0,
        }
    }

    fn check(h: &mut Held, n: usize, read: &mut u64) {
        // SAFETY (model of the second core): the picture lives in the
        // boxed console of the test, never moved or dropped while held;
        // only read.
        let now: &Frame = unsafe { &*h.at };
        for _ in 0..n {
            let i = h.checked % now.0.len();
            assert_eq!(
                now.0[i], h.snap.0[i],
                "byte {i} of the picture held by the sender changed after hand-over"
            );
            h.checked += 1;
        }
        *read += n as u64;
    }

    fn send_one(h: &mut Held, screen: &mut FakeScreen) {
        if h.sent < h.areas.len() {
            // SAFETY: as in `check`.
            let frame: &Frame = unsafe { &*h.at };
            let src = Source::new(frame, &h.palette, h.view, h.border);
            screen.present(&src, &h.areas[h.sent..h.sent + 1]);
            h.sent += 1;
        }
    }

    fn pump(&mut self, rng: &mut Rng) {
        if let Some(h) = self.held.as_mut() {
            Self::check(h, rng.below(8000) as usize, &mut self.bytes_read);
            if rng.below(2) == 0 {
                Self::send_one(h, &mut self.screen);
            }
        }
    }
}

impl Sender for Slow {
    fn wait_free(&mut self) {
        if let Some(mut h) = self.held.take() {
            while h.sent < h.areas.len() {
                Self::send_one(&mut h, &mut self.screen);
            }
            h.checked = 0;
            let n = h.snap.0.len();
            Self::check(&mut h, n, &mut self.bytes_read);
        }
    }

    fn hand_over(&mut self, h: &Handed<'_>) -> SendReport {
        assert!(
            self.held.is_none(),
            "a picture was handed over while the sender still held one"
        );
        self.hand_overs += 1;
        self.last = Some((Box::new(h.palette.clone()), h.view, h.border));
        self.last_snap = Some(Box::new(h.frame.clone()));
        self.held = Some(Held {
            at: h.frame,
            snap: Box::new(h.frame.clone()),
            palette: h.palette.clone(),
            view: h.view,
            border: h.border,
            areas: h.areas.to_vec(),
            sent: 0,
            checked: 0,
        });
        SendReport {
            areas: h.areas.len() as u16,
            pixels: h.areas.iter().map(|a| a.area()).sum(),
            send_us: 0,
            wait_us: 0,
        }
    }
}

/// The bytes `put_back` may write right after a present in a game: the
/// notice band (the top rows) and the pause menu's box.
fn restored_after_present(i: usize) -> bool {
    use console_core::theme::{MENU_H, MENU_W, MENU_X, MENU_Y, NOTE_H, NOTE_Y};
    let (x, y) = (i % 160, i / 160);
    let band = y < (NOTE_Y as usize + NOTE_H as usize);
    let menu = (MENU_X as usize..MENU_X as usize + MENU_W as usize).contains(&x)
        && (MENU_Y as usize..MENU_Y as usize + MENU_H as usize).contains(&y);
    band || menu
}

/// The whole panel drawn at once from a picture and a palette, on a
/// fresh fake screen (no areas, no history).
fn fresh_render(frame: &Frame, palette: &Palette, view: View, border: u16) -> u64 {
    let mut s = Box::new(FakeScreen::new());
    let all = Rect::new(0, 0, FakeScreen::PANEL_W, FakeScreen::PANEL_H);
    s.present(&Source::new(frame, palette, view, border), &[all]);
    fingerprint(&s.image)
}

fn frame_print(f: &Frame) -> u64 {
    let wide: Vec<u16> = f.0.iter().map(|&b| u16::from(b)).collect();
    fingerprint(&wide)
}

/// One random input event, the same for both consoles.
fn random_event(rng: &mut Rng, sim: &Sim) -> Option<Ev> {
    let held = sim.board.pad.state.buttons;
    let toggle = |b: u32| {
        Some(if held & b != 0 {
            Ev::Release(b)
        } else {
            Ev::Press(b)
        })
    };
    match rng.below(100) {
        0..=29 => {
            let x = rng.below(65_535) as i16;
            Some(Ev::Stick(x, x.wrapping_mul(3)))
        }
        30..=39 => toggle(btn::LEFT),
        // The pause menu, its lines, and choosing.
        40..=43 => toggle(HOME),
        44..=49 => toggle(btn::DOWN),
        50..=53 => toggle(btn::UP),
        54..=59 => toggle(btn::RIGHT),
        60..=63 => toggle(EAST),
        64..=66 => toggle(SOUTH),
        _ => None,
    }
}

#[test]
fn a_slow_sender_and_the_direct_way_show_the_same_and_nothing_held_is_written() {
    let mut total_hand_overs = 0;
    let mut lights = 0;
    let mut games = 0;
    for seed in 1..=3u64 {
        let mut rng = Rng(seed * 104_729);
        let mut a = Sim::with_games(Some(FOUR), GAME_A);
        let mut b = Sim::with_games(Some(FOUR), GAME_B);
        let mut slow = Slow::new();
        for tick in 0..1000u64 {
            let at = format!("seed {seed} tick {tick}");
            for _ in 0..rng.below(3) {
                if let Some(ev) = random_event(&mut rng, &a) {
                    a.input(ev);
                    b.input(ev);
                }
            }
            if rng.below(60) == 0 {
                assert_eq!(a.notify("HELLO"), b.notify("HELLO"));
            }
            // On the home screen, start the game again now and then.
            if a.console.page() == Page::Home && rng.below(8) == 0 {
                let ev = if a.board.pad.state.buttons & SOUTH != 0 {
                    Ev::Release(SOUTH)
                } else {
                    Ev::Press(SOUTH)
                };
                a.input(ev);
                b.input(ev);
            }
            let lights_before = b.board.screen.backlight_calls;
            let hand_overs_before = slow.hand_overs;
            a.step_tick();
            // The "second core" works while the console steps (the
            // console's writes in between are caught by the full check
            // at the next wait).
            slow.pump(&mut rng);
            // The backlight is never set while a picture is held: checked
            // after each step (a tick may hold more than one step, for
            // example in a boot animation faster than the tick).
            b.step_tick_by(|console, board| {
                let calls = board.screen.backlight_calls;
                let wake = console.step_with_sender(board, &mut slow);
                assert!(
                    board.screen.backlight_calls == calls || slow.held.is_none(),
                    "{at}: the backlight was set while the sender still held a picture"
                );
                wake
            });
            if slow.hand_overs != hand_overs_before {
                let snap = slow.last_snap.as_ref().expect("a picture was handed over");
                let game = b.console.page() == Page::Game;
                let now = b.console.video().frame();
                for (i, (&x, &y)) in now.0.iter().zip(snap.0.iter()).enumerate() {
                    // In a game, the console puts the game's own pixels
                    // back under the notice band and the pause menu right
                    // after the present: those two places may differ.
                    if !(game && restored_after_present(i)) {
                        assert_eq!(
                            x, y,
                            "{at}: byte {i}: the picture the console goes on in was not caught up"
                        );
                    }
                }
            }
            slow.pump(&mut rng);
            if a.console.page() == Page::Game && a.console.game().frames() == 1 {
                games += 1;
            }

            // The drawn picture is never the one held.
            let drawn = b.console.video().frame() as *const Frame;
            if let Some(h) = &slow.held {
                assert_ne!(drawn, h.at, "{at}: the console draws into the held picture");
            }
            // Screenshots and palettes: the same at every tick.
            assert_eq!(a.console.page(), b.console.page(), "{at}: page");
            assert_eq!(
                frame_print(a.console.video().frame()),
                frame_print(b.console.video().frame()),
                "{at}: the console's picture (screenshot) differs"
            );
            assert_eq!(
                a.console.video().palette().colours(),
                b.console.video().palette().colours(),
                "{at}: palette"
            );
            // The backlight: same levels; never while a picture is held.
            assert_eq!(a.board.screen.backlight, b.board.screen.backlight, "{at}");
            if b.board.screen.backlight_calls != lights_before {
                lights += 1;
            }
            // Now and then the board waits for the sender: its panel must
            // then be the direct console's panel.
            if rng.below(4) == 0 {
                slow.wait_free();
                assert_eq!(
                    fingerprint(&slow.screen.image),
                    a.ticks.last().unwrap().print,
                    "{at}: panel differs"
                );
                if let (Some((palette, view, border)), Some(snap)) = (&slow.last, &slow.last_snap) {
                    assert_eq!(
                        fingerprint(&slow.screen.image),
                        fresh_render(snap, palette, *view, *border),
                        "{at}: the panel is not the last picture handed over, drawn afresh"
                    );
                }
            }
        }
        slow.wait_free();
        assert_eq!(
            fingerprint(&slow.screen.image),
            a.ticks.last().unwrap().print
        );
        println!(
            "seed {seed}: {} hand-overs, {} bytes read and checked",
            slow.hand_overs, slow.bytes_read
        );
        total_hand_overs += slow.hand_overs;
    }
    println!("{total_hand_overs} hand-overs, {lights} backlight changes, {games} game starts");
    assert!(total_hand_overs > 1500, "enough pictures were sent");
    assert!(
        lights > 3,
        "the backlight changed during the runs ({lights})"
    );
    assert!(games > 3, "the game was started again and again ({games})");
}
