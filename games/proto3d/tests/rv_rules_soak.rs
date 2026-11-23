//! Rules review (2026-11-22): a long soak with random input through
//! [`Proto::control`], then RESTART and each area's route of the spec.
//!
//! The soak:
//!
//! * a fixed seed, so a failure can be run again;
//! * all 12 buttons and both sticks, long holds and quick taps, touch
//!   taps on and off the menu boxes, odd call lengths (0 to 3 steps);
//! * the pause menu, RESTART and AREAS at random, and area changes in
//!   the middle of anything (a glide, a carry, a raise, a fall, the
//!   done panel);
//! * at least `RV_SOAK_STEPS` (default 200,000) steps of play in each of
//!   the four areas;
//! * after every call: the hero's position is inside a box round the
//!   area, and (counted, printed) the hero's middle is not inside a
//!   solid block;
//! * every 4,000 calls a full picture is drawn (HUD, menus, done panel)
//!   so that the drawing sees the odd states too.
//!
//! At the end, for each area: RESTART from the pause menu, the state is
//! the same as a fresh start, and a scripted route of the spec (written
//! here, not copied from the area tests) reaches "done".
//!
//! Units: subs (1/4096 m), cm in the records, steps (1/30 s).

mod common;

use std::time::Instant;

use common::stick_towards;
use game_kit::{Frame, button};
use proto3d::area::machines::{self as mc, Machines};
use proto3d::area::sail::{self as sl};
use proto3d::area::stone::{self as st, Held, Stone};
use proto3d::area::{Area, Kit};
use proto3d::draw::Shown;
use proto3d::hero::Hero;
use proto3d::island::inside_any;
use proto3d::maths::cm;
use proto3d::pause::{AREAS, RESTART, Tap};
use proto3d::play::{Detail, Play};
use proto3d::tune::{FALL_MAX, HERO_H, KILL_Y, M};
use proto3d::{Input, Note, Proto};

/// A small fixed-seed generator (xorshift64*), the review's own.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, one_in: u64) -> bool {
        self.below(one_in) == 0
    }
    fn stick(&mut self) -> (i16, i16) {
        match self.below(6) {
            0 => (0, 0),
            // Full pushes in the eight directions.
            1 => {
                let d = [-32_767, 0, 32_767];
                (
                    d[self.below(3) as usize] as i16,
                    d[self.below(3) as usize] as i16,
                )
            }
            // Small pushes round the dead zone.
            2 => (
                (self.below(16_001) as i32 - 8_000) as i16,
                (self.below(16_001) as i32 - 8_000) as i16,
            ),
            _ => (
                (self.below(65_535) as i32 - 32_767) as i16,
                (self.below(65_535) as i32 - 32_767) as i16,
            ),
        }
    }
}

/// All twelve buttons.
const BUTTONS: [u32; 12] = [
    button::UP,
    button::DOWN,
    button::LEFT,
    button::RIGHT,
    button::A,
    button::B,
    button::X,
    button::Y,
    button::L,
    button::R,
    button::START,
    button::SELECT,
];

/// One random "hand": what is held, for how long, and the sticks.
struct Hand {
    held: u32,
    left: (i16, i16),
    right: (i16, i16),
    left_for: u32,
    right_for: u32,
    /// Buttons with a time left before they are let go.
    hold_left: [u32; 12],
}

impl Hand {
    fn new() -> Hand {
        Hand {
            held: 0,
            left: (0, 0),
            right: (0, 0),
            left_for: 0,
            right_for: 0,
            hold_left: [0; 12],
        }
    }

    /// The next call's buttons (held, pressed, released) and sticks.
    fn next(&mut self, r: &mut Rng) -> (u32, u32, u32) {
        let last = self.held;
        let mut held = 0;
        for (k, &b) in BUTTONS.iter().enumerate() {
            if self.hold_left[k] > 0 {
                self.hold_left[k] -= 1;
            } else {
                // START is rarer, or the menus would hide most of play.
                let one_in = match b {
                    button::START => 120,
                    button::SELECT => 400,
                    button::A | button::B => 12,
                    button::X => 25,
                    _ => 30,
                };
                if r.chance(one_in) {
                    self.hold_left[k] = match r.below(4) {
                        // A quick tap: one call.
                        0 | 1 => 1,
                        // A short hold.
                        2 => 2 + r.below(10) as u32,
                        // A long hold (up to 6 s).
                        _ => 10 + r.below(170) as u32,
                    };
                }
            }
            if self.hold_left[k] > 0 {
                held |= b;
            }
        }
        if self.left_for == 0 {
            self.left = r.stick();
            self.left_for = 1 + r.below(90) as u32;
        }
        self.left_for -= 1;
        if self.right_for == 0 {
            self.right = if r.chance(3) { r.stick() } else { (0, 0) };
            self.right_for = 1 + r.below(60) as u32;
        }
        self.right_for -= 1;
        self.held = held;
        (held, held & !last, last & !held)
    }
}

/// A box round an area's blocks (subs): lo and hi corners.
fn area_box(area: Area) -> ([i32; 3], [i32; 3]) {
    let mut lo = [i32::MAX; 3];
    let mut hi = [i32::MIN; 3];
    for b in area.world().blocks {
        let bb = b.bounds();
        for k in 0..3 {
            lo[k] = lo[k].min(bb.lo[k]);
            hi[k] = hi[k].max(bb.hi[k]);
        }
    }
    (lo, hi)
}

/// Whether the hero is in sane bounds: within 15 m sideways of the
/// area's blocks, at most 12 m under the lowest block's bottom (the
/// furthest fall room is 6 m under a safe spot; ISLAND falls to its
/// fixed `KILL_Y`, -20 m) and 8 m over the highest top.
fn sane(play: &Play) -> bool {
    let (lo, hi) = area_box(play.area);
    let p = play.hero.pos;
    let side = 15 * M;
    p[0] >= lo[0] - side
        && p[0] <= hi[0] + side
        && p[2] >= lo[2] - side
        && p[2] <= hi[2] + side
        && p[1] >= (lo[1] - 12 * M).min(KILL_Y - 2 * FALL_MAX)
        && p[1] <= hi[1] + 8 * M
}

/// The game driven through [`Proto::control`].
struct Bot {
    p: Box<Proto>,
    done: Option<Note>,
    calls: u64,
}

impl Bot {
    fn new() -> Bot {
        Bot {
            p: Box::new(Proto::new()),
            done: None,
            calls: 0,
        }
    }

    fn input(&mut self, i: &Input) -> Note {
        let n = self.p.control(i, |_| {});
        self.calls += 1;
        if let Note::Done(..) = n {
            self.done = Some(n);
        }
        n
    }

    fn call(&mut self, held: u32, pressed: u32, left: (i16, i16)) -> Note {
        let held = held | pressed;
        self.input(&Input {
            held,
            pressed,
            left,
            delta_us: 33_333,
            ..Input::default()
        })
    }

    fn press(&mut self, b: u32) -> Note {
        let n = self.call(b, b, (0, 0));
        self.call(0, 0, (0, 0));
        n
    }

    fn hero(&self) -> Hero {
        self.p.play.hero
    }

    fn machines(&self) -> Machines {
        match self.p.play.kit {
            Kit::Machines(m) => m,
            k => panic!("not MACHINES: {k:?}"),
        }
    }

    fn stone(&self) -> Stone {
        match self.p.play.kit {
            Kit::Stone(s) => s,
            k => panic!("not STONE: {k:?}"),
        }
    }

    /// The stick towards (x, z) cm; speed 0: slowing down near it.
    fn towards(&self, to: [i16; 2], speed: f64) -> (i16, i16) {
        let h = self.hero();
        let dx = f64::from(cm(to[0]) - h.pos[0]);
        let dz = f64::from(cm(to[1]) - h.pos[2]);
        let d = (dx * dx + dz * dz).sqrt() * 25.0 / 1024.0;
        let sp = if speed > 0.0 {
            speed
        } else {
            (d / 100.0).clamp(0.3, 1.0)
        };
        stick_towards(dx, dz, self.p.play.cam.yaw, sp)
    }

    /// Runs towards `to` until `end` says so. A is pressed once when
    /// `jump` says so (standing), then held in the air; with `sail`, B is
    /// held in the air after the jump too; with `place`, B is pressed
    /// once in the air when it says so. Panics after `limit` calls.
    #[allow(clippy::too_many_arguments)]
    fn leg(
        &mut self,
        to: [i16; 2],
        speed: f64,
        jump: fn(&Proto) -> bool,
        sail: bool,
        place: fn(&Proto) -> bool,
        end: fn(&Proto) -> bool,
        limit: usize,
    ) {
        let (mut jumped, mut placed) = (false, false);
        for _ in 0..limit {
            if self.done.is_some() {
                return;
            }
            let h = self.hero();
            let stick = self.towards(to, speed);
            let mut pressed = 0;
            if !jumped && h.ground.is_some() && h.frozen == 0 && jump(&self.p) {
                jumped = true;
                pressed |= button::A;
            } else if jumped && !placed && h.ground.is_none() && place(&self.p) {
                placed = true;
                pressed |= button::B;
            }
            let mut held = 0;
            if jumped && h.ground.is_none() || pressed & button::A != 0 {
                held |= button::A;
                if sail {
                    held |= button::B;
                }
            }
            self.call(held, pressed, stick);
            if end(&self.p) {
                return;
            }
        }
        panic!(
            "leg to {to:?} not ended in {limit} calls: hero {:?} kit {:?}",
            self.hero(),
            self.p.play.kit
        );
    }

    /// Walks to (x, z) cm and stops within `near` cm, standing.
    fn walk(&mut self, to: [i16; 2], near: i32) {
        for _ in 0..400 {
            let h = self.hero();
            let dx = i64::from(h.pos[0] - cm(to[0]));
            let dz = i64::from(h.pos[2] - cm(to[1]));
            let d2 = dx * dx + dz * dz;
            let n = i64::from(cm(near as i16));
            if d2 <= n * n && h.ground.is_some() && h.frozen == 0 {
                return;
            }
            let s = self.towards(to, 0.0);
            self.call(0, 0, s);
        }
        panic!("walk to {to:?} failed: hero {:?}", self.hero());
    }

    /// Lets go of everything until the hero stands still.
    fn settle(&mut self) {
        for _ in 0..20 {
            let h = self.hero();
            if h.ground.is_some() && h.vel == [0; 3] {
                return;
            }
            self.call(0, 0, (0, 0));
        }
    }
}

fn never(_: &Proto) -> bool {
    false
}

// ---- The routes of the spec (written from AREAS-SPEC.md) -------------------------

/// MACHINES: the bob, the plate, the bridge, the plank, D.
fn machines_route(bot: &mut Bot) {
    // 1. To the bob (at x -1.5, z -2.5), X picks it up.
    bot.walk([-80, -250], 12);
    bot.settle();
    bot.press(button::X);
    assert!(bot.machines().carry, "picked up: {:?}", bot.hero());
    // 2. Onto the plate with it: the latch, the bridge up.
    bot.walk([210, -90], 12);
    assert_eq!(bot.hero().ground, Some(mc::PLATE));
    for _ in 0..80 {
        if bot.machines().bridge == 0 {
            break;
        }
        bot.call(0, 0, (0, 0));
    }
    assert!(bot.machines().latched && bot.machines().bridge == 0);
    // 3. Over the bridge to C.
    bot.walk([0, -20], 20);
    bot.walk([0, 330], 20);
    assert_eq!(bot.hero().ground, Some(mc::C));
    // 4. Up the plank's low right end to its left half: it tips left.
    bot.walk([265, 430], 15);
    bot.walk([265, 550], 10);
    bot.walk([-40, 550], 12);
    bot.walk([-100, 550], 8);
    for _ in 0..8 {
        bot.call(0, 0, (0, 0));
    }
    assert!(bot.machines().left_down);
    // 5. X sets the bob down ahead (facing -x), on the left half.
    bot.press(button::X);
    assert!(!bot.machines().carry, "set down: {:?}", bot.machines());
    // 6. Light, up to the raised right end and the jump to D.
    bot.leg(
        [420, 550],
        1.0,
        |p| p.play.hero.pos[0] >= cm(185) && p.play.hero.ground == Some(mc::PLANK),
        false,
        never,
        never,
        300,
    );
}

/// SAIL: four glides.
fn sail_route(bot: &mut Bot) {
    // Gap 1.
    bot.leg(
        [0, 800],
        1.0,
        |p| p.play.hero.ground == Some(sl::P0) && p.play.hero.pos[2] >= cm(-25),
        true,
        never,
        |p| p.play.events.hero.landed == Some(sl::P1),
        300,
    );
    // Gap 2: along P1, then towards P2's middle.
    bot.leg(
        [0, 1100],
        1.0,
        never,
        false,
        never,
        |p| p.play.hero.pos[2] >= cm(940),
        300,
    );
    bot.leg(
        [500, 1900],
        1.0,
        |p| p.play.hero.ground == Some(sl::P1) && p.play.hero.pos[2] >= cm(1025),
        true,
        never,
        |p| p.play.events.hero.landed == Some(sl::P2),
        300,
    );
    // To P2's left end, then up the steps towards +x.
    bot.walk([320, 1900], 20);
    bot.settle();
    bot.leg(
        [2000, 1900],
        1.0,
        |p| p.play.hero.ground == Some(sl::P2) && p.play.hero.pos[0] >= cm(390),
        false,
        never,
        |p| matches!(p.play.events.hero.landed, Some(sl::S1) | Some(sl::S2)),
        300,
    );
    if bot.hero().ground == Some(sl::S1) {
        bot.leg(
            [2000, 1900],
            1.0,
            |p| p.play.hero.ground == Some(sl::S1) && p.play.hero.pos[0] >= cm(560),
            false,
            never,
            |p| p.play.events.hero.landed == Some(sl::S2),
            300,
        );
    }
    // Gap 3: off S2's end to P3.
    bot.leg(
        [2000, 1900],
        1.0,
        |p| p.play.hero.ground == Some(sl::S2) && p.play.hero.pos[0] >= cm(825),
        true,
        never,
        |p| p.play.events.hero.landed == Some(sl::P3),
        300,
    );
    // Gap 4: off P3's end to P4.
    bot.leg(
        [3475, 1900],
        1.0,
        |p| p.play.hero.ground == Some(sl::P3) && p.play.hero.pos[0] >= cm(2275),
        true,
        never,
        never,
        300,
    );
}

/// STONE: a ground place and a raise, an air place, an air place and a
/// raise.
fn stone_route(bot: &mut Bot) {
    fn fetch(bot: &mut Bot) {
        bot.settle();
        bot.press(button::X);
        for _ in 0..20 {
            if bot.stone().held == Held::InHand {
                return;
            }
            bot.call(0, 0, (0, 0));
        }
        panic!("the stone did not come back: {:?}", bot.stone());
    }
    fn raise(bot: &mut Bot) {
        bot.call(button::B, button::B, (0, 0));
        for _ in 0..80 {
            let s = bot.stone();
            if let Held::Placed { base } = s.held
                && s.at[1] >= base + st::RAISE_MAX
            {
                break;
            }
            bot.call(button::B, 0, (0, 0));
        }
        bot.call(0, 0, (0, 0));
    }
    // 1. Cliff.
    bot.walk([0, 50], 8);
    bot.settle();
    bot.press(button::B);
    assert!(bot.stone().placed(), "ground place: {:?}", bot.hero());
    bot.walk([0, 150], 10);
    bot.settle();
    assert_eq!(bot.hero().ground, Some(st::STONE));
    raise(bot);
    bot.leg(
        [0, 330],
        1.0,
        |_| true,
        false,
        never,
        |p| p.play.hero.ground == Some(st::K),
        300,
    );
    // 2. Gap: back to K's back, then off the far edge with an air place.
    fetch(bot);
    bot.walk([0, 240], 15);
    bot.settle();
    bot.leg(
        [0, 1100],
        1.0,
        |p| {
            p.play.hero.pos[2] >= cm(385)
                && matches!(p.play.kit, Kit::Stone(s) if s.held == Held::InHand)
        },
        false,
        |p| p.play.hero.vel[1] <= 0,
        |p| p.play.hero.ground == Some(st::STONE),
        300,
    );
    bot.leg(
        [0, 1100],
        1.0,
        |p| match p.play.kit {
            Kit::Stone(s) => {
                p.play.hero.ground == Some(st::STONE) && p.play.hero.pos[2] >= cm(s.at[2] + 40)
            }
            _ => false,
        },
        false,
        never,
        |p| p.play.hero.ground == Some(st::T),
        300,
    );
    // 3. Goal.
    fetch(bot);
    bot.walk([0, 1100], 15);
    bot.settle();
    bot.leg(
        [0, 1500],
        1.0,
        |p| {
            p.play.hero.pos[2] >= cm(1285)
                && matches!(p.play.kit, Kit::Stone(s) if s.held == Held::InHand)
        },
        false,
        |p| p.play.hero.vel[1] <= 0,
        |p| p.play.hero.ground == Some(st::STONE),
        300,
    );
    let s = bot.stone();
    bot.walk([s.at[0], s.at[2]], 12);
    bot.settle();
    assert_eq!(bot.hero().ground, Some(st::STONE));
    raise(bot);
    bot.leg([0, 1725], 1.0, |_| true, false, never, never, 300);
}

/// ISLAND keeps no time and has no route of its own in AREAS-SPEC.md;
/// after RESTART it is checked by the fresh-start comparison only.
fn run_route(bot: &mut Bot, area: Area) {
    match area {
        Area::Island => {}
        Area::Machines => machines_route(bot),
        Area::Sail => sail_route(bot),
        Area::Stone => stone_route(bot),
    }
}

/// Opens the pause menu and chooses line `sel` with the d-pad.
fn menu_choose(bot: &mut Bot, sel: u8) -> Note {
    bot.press(button::START);
    assert!(bot.p.menu.open);
    while bot.p.menu.sel > sel {
        bot.press(button::UP);
    }
    while bot.p.menu.sel < sel {
        bot.press(button::DOWN);
    }
    // One call only: the next call would already run a step.
    bot.call(button::A, button::A, (0, 0))
}

/// From the open area menu, choose `a` with the d-pad.
fn area_choose(bot: &mut Bot, a: Area) {
    assert!(bot.p.areas.open);
    while bot.p.areas.sel > a as u8 {
        bot.press(button::UP);
    }
    while bot.p.areas.sel < a as u8 {
        bot.press(button::DOWN);
    }
    bot.press(button::A);
    assert_eq!(bot.p.play.area, a);
}

/// The parts of a Play that a fresh start fixes.
fn same_as_fresh(play: &Play) -> bool {
    let f = Play::start(play.area, play.detail);
    play.hero == f.hero
        && play.kit == f.kit
        && play.live == f.live
        && play.cam == f.cam
        && play.done == f.done
        && play.done_steps == f.done_steps
        && play.steps == f.steps
        && play.fingerprint() == f.fingerprint()
}

#[test]
fn rv_rules_soak_all_areas_then_restart_and_finish() {
    let target: u64 = std::env::var("RV_SOAK_STEPS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200_000);
    let t0 = Instant::now();
    let mut r = Rng(0x005E_ED0F_2026_1122);
    let mut hand = Hand::new();
    let mut bot = Bot::new();
    let mut steps = [0u64; 4];
    let mut inside = [0u64; 4];
    let mut first_inside: Vec<String> = Vec::new();
    let mut notes = [0u64; 5];
    let mut seen = [0u64; 10];
    let mut pictures = 0u32;
    let mut frame = Box::new(Frame::new());
    let mut touch_down = false;
    let mut calls = 0u64;
    while steps.iter().any(|&s| s < target) {
        calls += 1;
        // Steer the area menu now and then towards the area with the
        // fewest steps, so every area gets its share.
        if bot.p.areas.open && r.chance(2) {
            let want = (0..4).min_by_key(|&k| steps[k]).unwrap() as u8;
            let s = bot.p.areas.sel;
            let b = if s < want {
                button::DOWN
            } else if s > want {
                button::UP
            } else {
                button::A
            };
            bot.call(b, b, (0, 0));
            bot.call(0, 0, (0, 0));
            continue;
        }
        // Now and then a straight trip through the pause menu: RESTART,
        // AREAS, or a detail.
        if !bot.p.menu.open && !bot.p.areas.open && r.chance(3_000) {
            let sel = r.below(u64::from(AREAS) + 1) as u8;
            let n = menu_choose(&mut bot, sel);
            if sel == RESTART {
                assert_eq!(n, Note::Restart(bot.p.play.area));
                assert!(same_as_fresh(&bot.p.play), "RESTART is not a fresh start");
            }
            continue;
        }
        let (held, pressed, released) = hand.next(&mut r);
        // Touch: a finger down for a few calls at a random point, or on
        // a menu line, then up.
        let touch = if touch_down {
            touch_down = !r.chance(3);
            let x = r.below(160) as i16;
            let y = r.below(120) as i16;
            Some(Tap {
                x,
                y,
                ended: !touch_down,
            })
        } else if r.chance(150) {
            touch_down = true;
            let (x, y) = if r.chance(2) {
                (40 + r.below(80) as i16, 30 + r.below(62) as i16)
            } else {
                (r.below(160) as i16, r.below(120) as i16)
            };
            Some(Tap { x, y, ended: false })
        } else {
            None
        };
        let delta_us = match r.below(40) {
            0 => 0,
            1 => 16_667,
            2 => 66_666,
            3 => 250_000,
            4 => 33_000 + r.below(700) as u32,
            _ => 33_333,
        };
        let area = bot.p.play.area;
        let n = bot.input(&Input {
            held,
            pressed,
            released,
            left: hand.left,
            right: hand.right,
            touch,
            delta_us,
        });
        notes[match n {
            Note::None => 0,
            Note::Area(_) => 1,
            Note::Detail(_) => 2,
            Note::Restart(_) => 3,
            Note::Done(..) => 4,
        }] += 1;
        if bot.p.play.area == area {
            steps[area.index()] += u64::from(bot.p.steps_last);
        }
        let play = &bot.p.play;
        assert!(
            sane(play),
            "call {calls}: {:?} hero out of bounds: {:?}",
            play.area,
            play.hero
        );
        let h = play.hero;
        // What the random hands reached (calls in each state).
        if bot.p.steps_last > 0 {
            match play.kit {
                Kit::Machines(m) => {
                    seen[0] += u64::from(m.carry);
                    seen[1] += u64::from(m.latched);
                    seen[2] += u64::from(m.left_down);
                    seen[3] += u64::from(m.rest == mc::PLANK);
                }
                Kit::Sail(_) => seen[4] += u64::from(h.gliding),
                Kit::Stone(s) => {
                    seen[5] += u64::from(s.placed());
                    seen[6] += u64::from(matches!(s.held, Held::Placed { base } if s.at[1] > base));
                    seen[7] += u64::from(h.ground == Some(st::STONE));
                }
                Kit::None => {}
            }
            seen[8] += u64::from(h.frozen > 0);
            seen[9] += u64::from(play.done);
        }
        if h.frozen == 0 {
            let mid = [h.pos[0], h.pos[1] + HERO_H / 2, h.pos[2]];
            if let Some(b) = inside_any(play.blocks(), mid) {
                inside[play.area.index()] += 1;
                if first_inside.len() < 8 {
                    first_inside.push(format!(
                        "call {calls} {:?}: middle inside block {b}: {:?} kit {:?}",
                        play.area, h, play.kit
                    ));
                }
            }
        }
        if calls.is_multiple_of(4_000) {
            let shown = Shown {
                menu: &bot.p.menu,
                areas: &bot.p.areas,
                line: bot.p.line,
                steps: bot.p.steps_last,
                best: bot.p.best[bot.p.play.area.index()],
                new_best: bot.p.new_best,
            };
            let play = bot.p.play;
            bot.p.paint.show(&mut frame, &play, &shown);
            pictures += 1;
        }
    }
    let soak = t0.elapsed();
    println!(
        "soak: {calls} calls, steps per area {steps:?} (ISLAND, MACHINES, SAIL, STONE), \
         notes [none, area, detail, restart, done] {notes:?}, {pictures} pictures, \
         {:.1} s (MEASURED, debug build on the PC)",
        soak.as_secs_f64()
    );
    println!(
        "calls in a state: carry {}, plate latched {}, plank left down {}, bob on the plank {}, \
         gliding {}, stone placed {}, stone raised {}, on the stone {}, frozen {}, done {}",
        seen[0], seen[1], seen[2], seen[3], seen[4], seen[5], seen[6], seen[7], seen[8], seen[9]
    );
    println!("hero's middle inside a solid block, calls per area: {inside:?}");
    for l in &first_inside {
        println!("  {l}");
    }
    println!("best times after the soak: {:?}", bot.p.best);

    // Close whatever is open, then for each area: AREAS to it, RESTART,
    // compare with a fresh start, and the route.
    let t1 = Instant::now();
    if bot.p.areas.open && !bot.p.areas.back_ok {
        area_choose(&mut bot, Area::Island);
    }
    if bot.p.areas.open {
        bot.press(button::B);
    }
    if bot.p.menu.open {
        bot.press(button::B);
    }
    assert!(!bot.p.menu.open && !bot.p.areas.open);
    for a in [Area::Machines, Area::Sail, Area::Stone, Area::Island] {
        menu_choose(&mut bot, AREAS);
        area_choose(&mut bot, a);
        // Play a little, then RESTART.
        for _ in 0..30 {
            bot.call(0, 0, (0, -32_767));
        }
        let n = menu_choose(&mut bot, RESTART);
        assert_eq!(n, Note::Restart(a));
        assert!(
            same_as_fresh(&bot.p.play),
            "{a:?}: RESTART is not a fresh start"
        );
        // MID detail for the routes.
        if bot.p.play.detail != Detail::Mid {
            menu_choose(&mut bot, 1);
        }
        bot.done = None;
        run_route(&mut bot, a);
        if a != Area::Island {
            let Some(Note::Done(da, Some(t))) = bot.done else {
                panic!(
                    "{a:?}: the route after the soak did not finish: {:?}",
                    bot.hero()
                );
            };
            assert_eq!(da, a);
            println!("{a:?}: route after RESTART done in {t} steps");
            assert!(bot.p.best[a.index()] <= t && bot.p.best[a.index()] > 0);
            // A again on the done panel restarts, once the panel has
            // been up half a second (rules review F5).
            for _ in 0..15 {
                bot.call(0, 0, (0, 0));
            }
            assert_eq!(bot.call(button::A, button::A, (0, 0)), Note::Restart(a));
            assert!(same_as_fresh(&bot.p.play));
        }
    }
    println!(
        "routes after the soak: {:.1} s; whole test {:.1} s (MEASURED)",
        t1.elapsed().as_secs_f64(),
        t0.elapsed().as_secs_f64()
    );
}
