//! Rules review (2026-11-22): shortcuts that skip a stage of an area.
//!
//! Each test plays through [`Proto::control`] (one step a call, the
//! same input a pad gives) and searches a small grid of human-like
//! choices (where to jump, how hard to push the stick). It prints every
//! try that works, with its steps, so a person can repeat it in the
//! simulator. A test FAILS when the shortcut works: the area's rule
//! then lets the player skip what the area is meant to teach.
//!
//! Units: subs (1/4096 m), cm in the records, steps (1/30 s).

mod common;

use common::stick_towards;
use game_kit::button;
use proto3d::area::machines::{self as mc, Machines};
use proto3d::area::stone::{self as st, Held, Stone};
use proto3d::area::{Area, Kit};
use proto3d::hero::Hero;
use proto3d::maths::cm;
use proto3d::play::Play;
use proto3d::step::Clock;
use proto3d::{Input, Note, Proto};

struct Bot {
    p: Box<Proto>,
    done: Option<Note>,
    calls: u32,
}

impl Bot {
    fn new() -> Bot {
        Bot {
            p: Box::new(Proto::new()),
            done: None,
            calls: 0,
        }
    }

    fn call(&mut self, held: u32, pressed: u32, left: (i16, i16)) {
        let held = held | pressed;
        let n = self.p.control(
            &Input {
                held,
                pressed,
                left,
                delta_us: 33_333,
                ..Input::default()
            },
            |_| {},
        );
        self.calls += 1;
        if let Note::Done(..) = n {
            self.done = Some(n);
        }
    }

    fn press(&mut self, b: u32) {
        self.call(b, b, (0, 0));
        self.call(0, 0, (0, 0));
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

    /// Walks to (x, z) cm and stops within `near` cm, standing.
    fn walk(&mut self, to: [i16; 2], near: i16) {
        for _ in 0..400 {
            let h = self.hero();
            let dx = i64::from(h.pos[0] - cm(to[0]));
            let dz = i64::from(h.pos[2] - cm(to[1]));
            let n = i64::from(cm(near));
            if dx * dx + dz * dz <= n * n && h.ground.is_some() && h.frozen == 0 {
                return;
            }
            let s = self.towards(to, 0.0);
            self.call(0, 0, s);
        }
        panic!("walk to {to:?} failed: hero {:?}", self.hero());
    }

    fn settle(&mut self) {
        for _ in 0..20 {
            let h = self.hero();
            if h.ground.is_some() && h.vel == [0; 3] {
                return;
            }
            self.call(0, 0, (0, 0));
        }
    }

    /// Runs towards `to` (stick `speed`), pressing A once when `jump`
    /// says so and holding it in the air, until standing again (or
    /// `limit` calls).
    fn jump_to(&mut self, to: [i16; 2], speed: f64, jump: impl Fn(&Hero) -> bool, limit: u32) {
        let mut jumped = false;
        for _ in 0..limit {
            if self.p.play.done {
                return;
            }
            let h = self.hero();
            let s = self.towards(to, speed);
            if !jumped && h.ground.is_some() && h.frozen == 0 && jump(&h) {
                jumped = true;
                self.call(button::A, button::A, s);
            } else if jumped && h.ground.is_none() {
                self.call(button::A, 0, s);
            } else {
                self.call(0, 0, s);
            }
            let h = self.hero();
            if jumped && h.ground.is_some() {
                return;
            }
        }
    }

    /// Saves what a later try starts from (each call runs exactly one
    /// step, so no presses wait in the game between calls).
    fn save(&self) -> (Play, Clock) {
        (self.p.play, self.p.clock)
    }

    fn load(&mut self, s: &(Play, Clock)) {
        self.p.play = s.0;
        self.p.clock = s.1;
        self.done = None;
    }

    /// From the start's area menu: DOWN `n` times, A.
    fn enter(&mut self, n: usize) {
        for _ in 0..n {
            self.press(button::DOWN);
        }
        self.press(button::A);
    }
}

// ---- MACHINES: done without the bob, the plate or the bridge ----------------------

/// AREAS-SPEC.md section 3, "Why it cannot be skipped": the raised
/// right end "needs L > R: the bob (2) on the left half and the light
/// hero (1) on the right". But the hero alone on the left half (1 > 0)
/// tips the plank too, the plank keeps its position while nothing
/// stands on it, and a hero in the air loads nothing. So: jump the 3 m
/// gap light (the bridge is not needed), walk up the plank to its left
/// half (it tips: the right end goes up to 1.25 m), jump from the left
/// half onto the raised right part, and jump again at once to D.
///
/// D's top is 2.20 m, but `Hero::sides` lifts a hero whose feet are up
/// to `STEP_UP` (25 cm) under a top, so the feet need only reach
/// 1.95 m. From the right part (about 1.0..1.2 m after one swing step)
/// a jump peaks at 2.2..2.4 m.
#[test]
fn rv_rules_machines_done_without_the_bob_plate_or_bridge() {
    let mut bot = Bot::new();
    bot.enter(1);
    assert_eq!(bot.p.play.area, Area::Machines);
    // 1. The 3 m gap from A to C, light, at the bridge's place (x 0).
    bot.walk([0, -150], 15);
    bot.settle();
    bot.jump_to([0, 450], 1.0, |h| h.pos[2] >= cm(-25), 120);
    assert_eq!(bot.hero().ground, Some(mc::C), "gap: {:?}", bot.hero());
    // 2. Up the plank's low right end to its left half. Before the fix
    // it tipped left under the hero alone (1 > 0); now a half must carry
    // at least the bob's weight (2), so it stays with its right end low.
    // The tries below still run, from wherever the hero now stands.
    bot.walk([265, 430], 15);
    bot.walk([265, 550], 10);
    bot.walk([-150, 550], 12);
    for _ in 0..10 {
        bot.call(0, 0, (0, 0));
    }
    let m = bot.machines();
    assert!(!m.left_down && m.right == mc::END_LOW, "{m:?}");
    assert!(!m.carry && !m.latched && m.rest == mc::A, "{m:?}");
    // 3. Back to the left end (a run-up), then the tries.
    bot.walk([-220, 550], 10);
    bot.settle();
    let start = bot.save();
    let mut wins = Vec::new();
    let mut misses = Vec::new();
    let mut tries = 0;
    for jump_x in [-150i16, -100, -60, -30, -10] {
        for aim_x in [150i16, 180, 200, 220, 240] {
            for speed in [0.6, 0.8, 1.0] {
                tries += 1;
                bot.load(&start);
                let c0 = bot.calls;
                // The first jump: from the left half towards the right part.
                bot.jump_to([aim_x, 550], speed, |h| h.pos[0] >= cm(jump_x), 120);
                let h = bot.hero();
                if bot.p.play.done || h.ground != Some(mc::PLANK) {
                    misses.push(format!(
                        "jump {jump_x} aim {aim_x} stick {speed}: first jump ended on {:?} \
                         at x {} cm",
                        h.ground,
                        h.pos[0] * 25 / 1024
                    ));
                    continue;
                }
                let land_x = h.pos[0];
                let land_y = h.pos[1];
                // The second jump at once, towards D.
                bot.jump_to([430, 550], 1.0, |_| true, 120);
                let m = bot.machines();
                // Proto's own "finish seen" flag is not restored by `load`,
                // so the finish is read from the play itself.
                if bot.p.play.done && bot.hero().ground == Some(mc::D) {
                    let t = m.steps;
                    assert!(!m.latched && m.rest == mc::A && !m.carry);
                    wins.push(format!(
                        "jump at x {jump_x} cm, aim x {aim_x} cm, stick {speed}: landed on \
                         the plank at x {} cm, feet {} cm; done after {} calls, clock {t} \
                         steps ({:.1} s); the bob never moved, the plate never latched",
                        h.pos[0] * 25 / 1024,
                        land_y * 25 / 1024,
                        bot.calls - c0,
                        f64::from(t) / 30.0
                    ));
                    let _ = land_x;
                } else {
                    misses.push(format!(
                        "jump {jump_x} aim {aim_x} stick {speed}: landed on the plank at x {} \
                         cm feet {} cm; then ended on {:?} at x {} cm",
                        h.pos[0] * 25 / 1024,
                        land_y * 25 / 1024,
                        bot.hero().ground,
                        bot.hero().pos[0] * 25 / 1024
                    ));
                }
            }
        }
    }
    if std::env::var_os("RV_MISSES").is_some() {
        for m in &misses {
            println!("  miss: {m}");
        }
    }
    println!(
        "{} of {tries} tries finish MACHINES without the bob:",
        wins.len()
    );
    for w in &wins {
        println!("  {w}");
    }
    assert!(
        wins.is_empty(),
        "MACHINES can be finished without the bob, the plate or the bridge ({} of {tries} tries)",
        wins.len()
    );
}

// ---- STONE: the gap crossed with a raise on K, no air place -------------------------

/// AREAS-SPEC.md section 5: the gap stage is meant for the air place
/// ("Gap: jump only, 3.67 m + coyote 0.67 m = 4.3 m < 5.0 m"). The spec
/// does not count a ground place on K plus a raise: the hero then
/// jumps from 2.75 m over K and has about 27 steps in the air.
#[test]
fn rv_rules_stone_gap_crossed_without_an_air_place() {
    let mut bot = Bot::new();
    bot.enter(3);
    assert_eq!(bot.p.play.area, Area::Stone);
    // Stage 1 as the spec says: a ground place, a raise, onto K.
    bot.walk([0, 50], 8);
    bot.settle();
    bot.press(button::B);
    assert!(bot.stone().placed());
    bot.walk([0, 150], 10);
    bot.settle();
    assert_eq!(bot.hero().ground, Some(st::STONE));
    raise(&mut bot);
    bot.jump_to([0, 330], 1.0, |_| true, 120);
    assert_eq!(bot.hero().ground, Some(st::K));
    // Call it back, then a ground place on K near its far edge.
    bot.settle();
    bot.press(button::X);
    for _ in 0..20 {
        bot.call(0, 0, (0, 0));
    }
    assert_eq!(bot.stone().held, Held::InHand);
    // To K's back half, then a few steps forward so that the hero
    // faces +z (the place goes 1 m ahead of him).
    bot.walk([0, 205], 6);
    bot.settle();
    for _ in 0..12 {
        let yaw = bot.hero().yaw;
        if !(1_000..64_536).contains(&yaw) && bot.hero().pos[2] >= cm(240) {
            break;
        }
        let s = stick_towards(0.0, 1.0, bot.p.play.cam.yaw, 0.15);
        bot.call(0, 0, s);
    }
    bot.settle();
    println!(
        "before the place: hero at z {} cm",
        bot.hero().pos[2] * 25 / 1024
    );
    let yaw = bot.hero().yaw;
    assert!(!(4_000..61_536).contains(&yaw), "facing +z: yaw {yaw}");
    // Before the fix the place was taken, and a full raise on it (top
    // 5.75 m) let the hero jump the gap to T with no air place (11 of 11
    // tries in the review). Now a ground place on K is refused with
    // NO ROOM, and the stone stays in hand: only the air place bridges
    // the gap.
    bot.press(button::B);
    let s = bot.stone();
    assert!(
        !s.placed() && s.held == Held::InHand,
        "ground place on K must be refused: {s:?} hero {:?}",
        bot.hero()
    );
    assert!(s.no_room > 0, "NO ROOM must show: {s:?}");
    // The air place from K still works (the route the course is built
    // round): a jump off K's top, B at the top of the jump.
    let mut placed = false;
    bot.call(button::A, button::A, (0, 0));
    for _ in 0..40 {
        if bot.hero().vel[1] <= 0 {
            bot.call(button::B, button::B, (0, 0));
            placed = bot.stone().placed();
            break;
        }
        bot.call(button::A, 0, (0, 0));
    }
    assert!(placed, "an air place over K: {:?}", bot.stone());
}

/// A fresh press of B on the stone, then B held to the top.
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

/// STONE's goal stage, the same way: a ground place on T near its far
/// edge and a full raise put the feet at 5.75 m; G's top is 7.0 m, and
/// `Hero::sides` lifts the hero onto it from 6.75 m. The horizontal
/// distance from T's far edge (z 13.0) to G (z 16.0) is 3 m. This test
/// only reports; it passes either way (the spec's route uses an air
/// place here, and the margin is the point).
#[test]
fn rv_rules_stone_goal_from_a_ground_place_on_t_is_reported() {
    let mut bot = Bot::new();
    bot.enter(3);
    // The hero on T at z 11.4 m facing +z, as after stage 2.
    let mut h = Hero::new([0, cm(300), cm(1140)], st::T);
    h.yaw = 0;
    bot.p.play.hero = h;
    if let Kit::Stone(k) = &mut bot.p.play.kit {
        k.safe = h.safe;
        k.safe_top = st::T;
    }
    bot.settle();
    bot.press(button::B);
    let s = bot.stone();
    assert!(s.placed(), "ground place on T: {s:?} {:?}", bot.hero());
    bot.walk([s.at[0], s.at[2]], 10);
    bot.settle();
    assert_eq!(bot.hero().ground, Some(st::STONE));
    raise(&mut bot);
    let s = bot.stone();
    bot.walk([s.at[0], s.at[2] - 35], 6);
    bot.settle();
    let start = bot.save();
    let mut best: Option<(i32, i32)> = None;
    let mut wins = 0;
    for jump_dz in (30i16..=100).step_by(5) {
        bot.load(&start);
        let jz = s.at[2] + jump_dz;
        let mut jumped = false;
        for _ in 0..120 {
            let h = bot.hero();
            let sk = bot.towards([0, 1725], 1.0);
            if !jumped && h.pos[2] >= cm(jz) {
                jumped = true;
                bot.call(button::A, button::A, sk);
            } else if jumped {
                bot.call(button::A, 0, sk);
            } else {
                bot.call(0, 0, sk);
            }
            let h = bot.hero();
            // The feet's height when the body first reaches G's z
            // (middle at 15.75 m).
            if h.pos[2] >= cm(1575) && best.is_none_or(|(_, y)| h.pos[1] > y) {
                best = Some((jz.into(), h.pos[1]));
            }
            if (jumped && h.ground.is_some() && h.ground != Some(st::STONE)) || h.frozen > 0 {
                break;
            }
        }
        if bot.hero().ground == Some(st::G) {
            wins += 1;
        }
    }
    println!(
        "goal from a ground place on T (stone top {} cm, middle z {} cm): {wins} tries reach G; \
         highest feet seen with the middle past z 15.75 m: {:?} cm (G lifts from 675 cm)",
        s.at[1],
        s.at[2],
        best.map(|(jz, y)| (jz, y * 25 / 1024))
    );
}
