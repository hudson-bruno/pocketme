//! SAIL (AREAS-SPEC.md, sections 4 and 6): the glide's numbers, the
//! reach of each gap with and without it, the clock, the best time, the
//! instant retry, the route from the area menu to "done", and pictures.
//!
//! Hand sums are written beside the expected values. Units: subs
//! (1/4096 m), steps (1/30 s), cm in the records.

mod common;

use std::path::Path;

use common::{hero_at, pad, stick_towards};
use game_3d::Stats;
use game_kit::{Frame, H, W, button};
use proto3d::area::sail::{
    self as sa, BAR_MAX, FALL_ROOM, P0, P1, P2, P3, P4, RESPAWN, S1, S2, Sail, bar_of, on_p0,
};
use proto3d::area::{Area, Hud, Kit};
use proto3d::draw::{PALETTE, Painter, col};
use proto3d::hero::{Body, Hero, HeroEvents, Pose};
use proto3d::island::{Block, flag};
use proto3d::maths::cm;
use proto3d::models::{self, mat};
use proto3d::pause::Menu;
use proto3d::play::{Detail, Play};
use proto3d::step::Pad;
use proto3d::tune::{
    FALL_BRAKE, FALL_MAX, G, GLIDE_FALL, GLIDE_RUN, GLIDE_STEPS, GLIDE_TURN, JUMP, M, RING_LOOK,
    RUN,
};
use proto3d::{Input, Note, Proto};

/// The triangle budgets, LOW, MID, HIGH and MAX (MAX: the most
/// triangles of the swept ISLAND views at MAX plus 5 %, as in
/// tests/max_level.rs).
const BUDGET: [u32; 4] = [160, 430, 940, 1324];

fn blocks() -> Vec<Block> {
    sa::WORLD.blocks.to_vec()
}

/// The area's body for a hero.
fn body(h: &Hero) -> Body {
    Sail::new().body(h)
}

/// The same body without the glide (a jump alone).
fn no_glide(h: &Hero) -> Body {
    Body {
        glide: false,
        ..body(h)
    }
}

/// A hero in the air at `y` cm over a far-away floor, still.
fn in_air(y: i16) -> (Hero, Vec<Block>) {
    let b = vec![Block::flat(
        [-500, -2_060, -500],
        [500, -2_000, 500],
        mat::GRASS,
        mat::DIRT,
        flag::WALK,
    )];
    let mut h = hero_at(0, -2_000, 0, 0);
    h.pos[1] = cm(y);
    h.ground = None;
    h.air_steps = 10;
    (h, b)
}

fn kit(p: &Play) -> &Sail {
    match &p.kit {
        Kit::Sail(s) => s,
        k => panic!("not SAIL: {k:?}"),
    }
}

fn b_pad() -> Pad {
    pad((0, 0), button::B)
}

// ---- The glide's numbers ---------------------------------------------------------------

#[test]
fn the_area_body_glides_and_retries_in_four_steps() {
    let h = hero_at(0, 0, -500, P0);
    let b = body(&h);
    // kill_y = safe.y - 4 m = 0 - 4 * 4096 = -16384.
    assert_eq!(FALL_ROOM, 16_384);
    assert_eq!(
        b,
        Body {
            run: RUN,
            jump: JUMP,
            glide: true,
            kill_y: -16_384,
            respawn: 4,
        }
    );
    // On P3 (top -0.6 m = -60 * 1024 / 25 = -2457.6 -> -2458 subs):
    // -2458 - 16384 = -18842.
    let h = hero_at(2000, -60, 1900, P3);
    assert_eq!(h.safe[1], -2_458);
    assert_eq!(body(&h).kill_y, -18_842);
    // The tuning the spec names: 1.5 m/s, 5.5 m/s, 2.0 s, a brake of 228.
    // 205 * 30 / 4096 = 1.50 m/s; 751 * 30 / 4096 = 5.50 m/s; 60 / 30 = 2 s.
    assert_eq!(
        (GLIDE_FALL, GLIDE_RUN, GLIDE_STEPS, FALL_BRAKE),
        (205, 751, 60, 228)
    );
    assert_eq!(GLIDE_FALL * 30 * 100 / 4_096, 150);
    assert_eq!(GLIDE_RUN * 30 * 100 / 4_096, 550);
}

#[test]
fn the_fall_speed_caps_at_205() {
    let (mut h, b) = in_air(300);
    let body = body(&h);
    // From still: 0 - 114 = -114; -114 - 114 = -228, held to -205; then
    // -205 - 114 = -319, held to -205 again, and so on.
    let mut seen = Vec::new();
    for _ in 0..5 {
        h.step_with(&b_pad(), 0, &b, &body);
        seen.push(h.vel[1]);
        assert!(h.gliding);
    }
    assert_eq!(seen, [-114, -205, -205, -205, -205]);
    assert_eq!(G, 114);
}

/// Rules review F6, H5: B held while rising neither glides nor uses up
/// the glide; the glide starts only once the hero falls.
#[test]
fn b_held_while_rising_neither_glides_nor_uses_the_glide() {
    let (mut h, b) = in_air(300);
    let body = body(&h);
    h.vel[1] = 1_000;
    let mut rising = 0;
    while h.vel[1] > 0 {
        h.step_with(&b_pad(), 0, &b, &body);
        if h.vel[1] > 0 {
            rising += 1;
            assert!(!h.gliding, "glides while rising at {}", h.vel[1]);
            assert_eq!(h.glide_left, GLIDE_STEPS);
        }
        assert!(rising < 20);
    }
    // A is not held, so the rise is cut short; still some steps of it.
    assert!(rising >= 3, "{rising}");
    for _ in 0..3 {
        h.step_with(&b_pad(), 0, &b, &body);
    }
    assert!(h.gliding);
    assert!(h.glide_left < GLIDE_STEPS);
}

#[test]
fn the_brake_from_1365_takes_six_steps() {
    let (mut h, b) = in_air(1_500);
    h.vel[1] = -FALL_MAX;
    assert_eq!(FALL_MAX, 1_365);
    let body = body(&h);
    // -1365 + 228 = -1137; -909; -681; -453; -225; -225 + 228 = 3, held
    // to -205: six steps.
    let mut seen = Vec::new();
    while h.vel[1] != -GLIDE_FALL {
        h.step_with(&b_pad(), 0, &b, &body);
        seen.push(h.vel[1]);
        assert!(seen.len() <= 6);
    }
    assert_eq!(seen, [-1_137, -909, -681, -453, -225, -205]);
}

#[test]
fn the_glide_stops_after_60_steps_and_a_landing_refills_it() {
    // 60 steps at 1.5 m/s fall about 3 m (60 * 205 = 12300 subs, less
    // the first step's 91): start 20 m up so the floor is far.
    let (mut h, b) = in_air(1_500);
    let body = body(&h);
    let mut glided = 0;
    for _ in 0..80 {
        h.step_with(&b_pad(), 0, &b, &body);
        glided += u32::from(h.gliding);
    }
    assert_eq!(glided, 60);
    assert_eq!(h.glide_left, 0);
    // After it ran out, a plain fall: faster than -205 (-205 - 114 = -319
    // in the first step after).
    assert!(h.vel[1] < -GLIDE_FALL);
    assert!(!h.gliding);
    // Letting go of B and pressing again uses what is left: 30 steps,
    // let go, then 30 more and no more.
    let (mut h, b) = in_air(1_500);
    for _ in 0..30 {
        h.step_with(&b_pad(), 0, &b, &body);
    }
    assert_eq!(h.glide_left, 30);
    for _ in 0..5 {
        h.step_with(&Pad::NONE, 0, &b, &body);
    }
    assert_eq!(h.glide_left, 30);
    let mut more = 0;
    for _ in 0..40 {
        h.step_with(&b_pad(), 0, &b, &body);
        more += u32::from(h.gliding);
    }
    assert_eq!(more, 30);
    // Down on the floor: full again.
    for _ in 0..400 {
        h.step_with(&b_pad(), 0, &b, &body);
        if h.ground.is_some() {
            break;
        }
    }
    assert_eq!(h.ground, Some(0));
    assert_eq!(h.glide_left, 60);
    // The HUD's bar: hidden while full, glide_left / 2 px else.
    assert_eq!(BAR_MAX, 30);
    assert_eq!(bar_of(60), None);
    assert_eq!(bar_of(59), Some(29)); // 59 / 2 = 29
    assert_eq!(bar_of(30), Some(15));
    assert_eq!(bar_of(1), Some(0));
    assert_eq!(bar_of(0), Some(0));
}

#[test]
fn a_jumps_but_never_glides_and_b_on_the_ground_does_nothing() {
    let b = blocks();
    // A held from the press to the landing: a full jump, no glide.
    let mut h = hero_at(0, 0, -300, P0);
    let body = body(&h);
    h.step_with(&common::jump_pad((0, 0), 0), 0, &b, &body);
    let mut steps = 1;
    while h.ground.is_none() {
        h.step_with(&pad((0, 0), button::A), 0, &b, &body);
        assert!(!h.gliding);
        steps += 1;
        assert!(steps < 60);
    }
    // Up 1120 and down by 114 a step: after n steps the feet are
    // 1120 n - 114 (1 + .. + n) = 1120 n - 57 n (n + 1) up; n = 18:
    // 20160 - 19494 = 666 > 0, n = 19: 21280 - 21660 = -380, so the
    // landing is in step 19.
    assert_eq!(steps, 19);
    // B on the ground: the same as no button, step for step.
    let mut a = hero_at(0, 0, -300, P0);
    let mut c = a;
    // (15 steps cover about 2.1 m of the 2.75 m to the edge.)
    for _ in 0..15 {
        let ea = a.step_with(&pad(common::FWD, button::B), 0, &b, &body);
        let ec = c.step_with(&pad(common::FWD, 0), 0, &b, &body);
        assert_eq!((a, ea), (c, ec));
        assert!(!a.gliding);
        assert_eq!(a.ground, Some(P0));
    }
    // B with A pressed in the same step jumps (the jump wins).
    let mut h = hero_at(0, 0, -300, P0);
    let ev = h.step_with(&common::jump_pad((0, 0), button::B), 0, &b, &body);
    assert!(ev.jumped);
    assert_eq!(h.vel[1], JUMP - G); // 1120 - 114 = 1006
    assert!(!h.gliding);
}

#[test]
fn the_ring_follows_the_glide() {
    // A full jump off P0's far edge at run speed; at the top of it,
    // holding B: the ring is on P1 with the glide, and there is none
    // without it (the plain fall lands on nothing within the look-ahead).
    let b = blocks();
    let mut h = hero_at(0, 0, -25, P0);
    h.vel[2] = RUN;
    h.step_with(&common::jump_pad(common::FWD, 0), 0, &b, &body(&h));
    while h.vel[1] > 0 {
        h.step_with(&pad(common::FWD, button::A), 0, &b, &body(&h));
    }
    let p = pad(common::FWD, button::B);
    let ring = h
        .ring_with(&p, 0, &b, &body(&h))
        .expect("a ring with the glide");
    assert_eq!(ring.top, P1);
    assert!(h.ring_with(&p, 0, &b, &no_glide(&h)).is_none());
    // The ring is where the glide lands.
    let mut g = h;
    for _ in 0..RING_LOOK {
        g.step_with(&p, 0, &b, &body(&h));
        if g.ground.is_some() {
            break;
        }
    }
    assert_eq!(g.ground, Some(P1));
    assert_eq!(ring.spot, g.pos);
}

// ---- The reach of each gap --------------------------------------------------------------

/// One take-off: the hero stands at `from` (cm) on `on`, runs towards
/// `to` (cm, x and z; through `via` first, until he is within 60 cm of
/// it), jumps when `jump_at` says, holds A to the top and B from the
/// top on (when `glide`). Returns the block he lands on first after the
/// jump (None: he fell off), and the horizontal distance from the
/// take-off to the landing (cm).
fn take_off(
    from: [i16; 3],
    on: u8,
    via: Option<[i16; 2]>,
    to: [i16; 2],
    jump_at: fn(&Hero) -> bool,
    glide: bool,
) -> (Option<u8>, i32) {
    let b = blocks();
    let mut h = hero_at(from[0], from[1], from[2], on);
    let mut via = via;
    let mut jumped: Option<[i32; 3]> = None;
    for _ in 0..400 {
        let goal = via.unwrap_or(to);
        let (dx, dz) = (
            f64::from(cm(goal[0]) - h.pos[0]),
            f64::from(cm(goal[1]) - h.pos[2]),
        );
        if via.is_some() && (dx * dx + dz * dz).sqrt() < f64::from(cm(60)) {
            via = None;
        }
        let s = stick_towards(dx, dz, 0, 1.0);
        let bd = if glide { body(&h) } else { no_glide(&h) };
        let p = if jumped.is_none() && h.ground.is_some() && jump_at(&h) {
            jumped = Some(h.pos);
            common::jump_pad(s, 0)
        } else if jumped.is_some() {
            pad(s, button::A | button::B)
        } else {
            pad(s, 0)
        };
        let ev = h.step_with(&p, 0, &b, &bd);
        if let (Some(j), Some(t)) = (jumped, ev.landed) {
            let d = (f64::from(h.pos[0] - j[0]).hypot(f64::from(h.pos[2] - j[2])) * 25.0 / 1024.0)
                as i32;
            return (Some(t), d);
        }
        if ev.fell {
            return (None, 0);
        }
    }
    panic!("no landing and no fall: {h:?}");
}

/// The four gaps' take-offs: (name, from, on, via, to, jump rule,
/// landing block).
type Gap = (
    &'static str,
    [i16; 3],
    u8,
    Option<[i16; 2]>,
    [i16; 2],
    fn(&Hero) -> bool,
    u8,
);

const GAPS: [Gap; 4] = [
    // 1: P0 to P1, 4.5 m, level. From 3 m before the edge.
    (
        "1 P0-P1",
        [0, 0, -300],
        P0,
        None,
        [0, 800],
        |h| h.pos[2] >= cm(-25),
        P1,
    ),
    // 2: P1 to P2, 6.5 m, 0.6 m down, drifting 1 m right.
    (
        "2 P1-P2",
        [0, 0, 600],
        P1,
        Some([0, 1000]),
        [500, 1900],
        |h| h.pos[2] >= cm(1025),
        P2,
    ),
    // 3: S2 to P3, 8.5 m, 1.0 m down, a 2 m run-up on S2.
    (
        "3 S2-P3",
        [660, 40, 1900],
        S2,
        None,
        [2000, 1900],
        |h| h.pos[0] >= cm(825),
        P3,
    ),
    // 4: P3 to P4, 10.5 m, 1.5 m down, onto the 2.5 m goal pad.
    (
        "4 P3-P4",
        [1750, -60, 1900],
        P3,
        None,
        [3475, 1900],
        |h| h.pos[0] >= cm(2275),
        P4,
    ),
];

#[test]
fn every_gap_needs_the_glide_and_the_glide_crosses_it() {
    for (name, from, on, via, to, jump_at, land) in GAPS {
        let (with, d) = take_off(from, on, via, to, jump_at, true);
        let (without, d0) = take_off(from, on, via, to, jump_at, false);
        println!(
            "gap {name}: glide lands on {with:?} {d} cm from the take-off; jump only {without:?} {d0} cm"
        );
        assert_eq!(with, Some(land), "gap {name} with the glide");
        assert_ne!(without, Some(land), "gap {name} without the glide");
    }
}

// ---- The clock, the best, the retry ------------------------------------------------------

/// The game driven through [`Proto::control`], one step a call.
struct Bot {
    p: Box<Proto>,
    calls: usize,
    done: Option<Note>,
    /// Every call's input (held, pressed, left stick), for the simulator
    /// script.
    log: Vec<(u32, u32, (i16, i16))>,
    /// Calls after which the simulator script takes a picture, named.
    shots: Vec<(usize, &'static str)>,
    /// The highest number of steps between a fall and the respawn.
    frozen_most: u32,
    frozen_now: u32,
    /// When set: the first call of a leg in which it says yes taps L and
    /// R together (the camera turns behind the hero), as a player does
    /// after a turn the camera does not follow by itself.
    behind: Option<fn(&Hero) -> bool>,
    /// The camera's yaw after every call.
    yaws: Vec<u16>,
}

impl Bot {
    fn new() -> Bot {
        Bot {
            p: Box::new(Proto::new()),
            calls: 0,
            done: None,
            log: Vec::new(),
            shots: Vec::new(),
            frozen_most: 0,
            frozen_now: 0,
            behind: None,
            yaws: Vec::new(),
        }
    }

    /// Marks the last call for a picture in the simulator script.
    fn shot(&mut self, name: &'static str) {
        self.shots.push((self.calls - 1, name));
    }

    fn call(&mut self, held: u32, pressed: u32, left: (i16, i16)) {
        self.log.push((held, pressed, left));
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
        self.yaws.push(self.p.play.cam.yaw);
        if let Note::Done(..) = n {
            self.done = Some(n);
        }
        if self.p.play.hero.frozen > 0 {
            self.frozen_now += 1;
            self.frozen_most = self.frozen_most.max(self.frozen_now);
        } else {
            self.frozen_now = 0;
        }
    }

    fn press(&mut self, b: u32) {
        self.call(b, b, (0, 0));
        self.call(0, 0, (0, 0));
    }

    fn wait(&mut self, n: usize) {
        for _ in 0..n {
            self.call(0, 0, (0, 0));
        }
    }

    fn ev(&self) -> HeroEvents {
        self.p.play.events.hero
    }

    /// Runs towards (x, z) cm at `speed`, pressing A once when `jump`
    /// says so (standing), then holding A, and B too when `glide`, until
    /// `end` says so (after a step). Panics after 400 steps.
    fn leg(
        &mut self,
        to: [i16; 2],
        speed: f64,
        jump: fn(&Hero) -> bool,
        glide: bool,
        end: fn(&Hero, &HeroEvents) -> bool,
    ) {
        let mut jumped = false;
        for _ in 0..400 {
            if self.done.is_some() {
                return;
            }
            let h = self.p.play.hero;
            let dx = f64::from(cm(to[0]) - h.pos[0]);
            let dz = f64::from(cm(to[1]) - h.pos[2]);
            let d = (dx * dx + dz * dz).sqrt() * 25.0 / 1024.0;
            let sp = if speed > 0.0 {
                speed
            } else {
                (d / 100.0).clamp(0.3, 1.0)
            };
            let s = stick_towards(dx, dz, self.p.play.cam.yaw, sp);
            // In the air: A held after this leg's jump (a full jump), B
            // held on a gliding leg (the glide starts at the top).
            let air = if jumped { button::A } else { 0 } | if glide { button::B } else { 0 };
            let tap = match self.behind {
                Some(f) if f(&h) => {
                    self.behind = None;
                    button::L | button::R
                }
                _ => 0,
            };
            if !jumped && h.ground.is_some() && h.frozen == 0 && jump(&h) {
                jumped = true;
                self.call(button::A | tap, button::A | tap, s);
            } else if h.ground.is_none() && h.frozen == 0 && air != 0 {
                self.call(air | tap, tap, s);
            } else {
                self.call(tap, tap, s);
            }
            let ev = self.ev();
            if end(&self.p.play.hero, &ev) {
                return;
            }
        }
        panic!(
            "leg to {to:?} not ended in 400 steps: hero {:?} kit {:?}",
            self.p.play.hero,
            kit(&self.p.play)
        );
    }

    /// Opens SAIL from the area menu (DOWN, DOWN, A), then sets the
    /// detail in the pause menu unless it is the start's.
    fn enter(&mut self, detail: Detail) {
        self.press(button::DOWN);
        self.press(button::DOWN);
        self.press(button::A);
        assert_eq!(self.p.play.area, Area::Sail);
        if detail != Detail::START {
            // START opens the pause menu on the start's line (HIGH); UP or
            // DOWN to the wanted line, A chooses it.
            self.press(button::START);
            let (from, to) = (Detail::START.level(), detail.level());
            let key = if to < from { button::UP } else { button::DOWN };
            for _ in 0..from.abs_diff(to) {
                self.press(key);
            }
            self.press(button::A);
        }
        assert_eq!(self.p.play.detail, detail);
    }
}

fn never(_: &Hero) -> bool {
    false
}

/// The route of the spec, from the area menu to done: four glides and
/// two hops, no fall.
fn route(detail: Detail) -> Bot {
    let mut bot = Bot::new();
    bot.enter(detail);
    bot.shot("start");
    // Gap 1: run to P0's edge, jump, glide to P1.
    bot.leg(
        [0, 800],
        1.0,
        |h| h.ground == Some(P0) && h.pos[2] >= cm(-25),
        true,
        |h, _| h.gliding && h.pos[2] >= cm(150),
    );
    bot.shot("glide-gap-1");
    bot.leg([0, 800], 1.0, never, true, |_, e| e.landed == Some(P1));
    // Gap 2: on along P1, then towards P2's middle, 1 m to the right.
    bot.leg([0, 1100], 1.0, never, false, |h, _| h.pos[2] >= cm(940));
    bot.leg(
        [500, 1900],
        1.0,
        |h| h.ground == Some(P1) && h.pos[2] >= cm(1025),
        true,
        |_, e| e.landed == Some(P2),
    );
    bot.shot("landed-p2");
    // The turn of 90°: to P2's left end, slowing down, then face +x.
    bot.leg([320, 1900], 0.0, never, false, |h, _| {
        h.ground == Some(P2) && (h.pos[0] - cm(320)).abs() < cm(25)
    });
    // Up the steps: a hop onto S1 (or straight onto S2). No camera
    // input: the glide over gap 3 brings the camera round behind him.
    bot.leg(
        [2000, 1900],
        1.0,
        |h| h.ground == Some(P2) && h.pos[0] >= cm(390),
        false,
        |_, e| matches!(e.landed, Some(S1) | Some(S2)),
    );
    if bot.p.play.hero.ground == Some(S1) {
        bot.leg(
            [2000, 1900],
            1.0,
            |h| h.ground == Some(S1) && h.pos[0] >= cm(560),
            false,
            |_, e| e.landed == Some(S2),
        );
    }
    bot.shot("on-s2");
    // Gap 3: off S2's end, glide to P3.
    bot.leg(
        [2000, 1900],
        1.0,
        |h| h.ground == Some(S2) && h.pos[0] >= cm(825),
        true,
        |_, e| e.landed == Some(P3),
    );
    bot.shot("landed-p3");
    // Gap 4: along P3 and off its end, glide to P4: done.
    bot.leg(
        [3475, 1900],
        1.0,
        |h| h.ground == Some(P3) && h.pos[0] >= cm(2275),
        true,
        |h, _| h.gliding && h.pos[0] >= cm(2900),
    );
    bot.shot("glide-gap-4");
    bot.leg([3475, 1900], 1.0, never, true, |_, e| e.landed == Some(P4));
    bot
}

/// Builder issue c (draw review F6): after the 90° turn on P2 the camera
/// comes round behind the gliding hero by itself, as one gentle turn: no
/// step turns it more than the glide follow's most (2.5°), and it is
/// within 30° of the hero's run (+x) on landing on P3.
#[test]
fn the_camera_comes_round_behind_the_glide_gently() {
    let bot = route(Detail::Mid);
    assert!(bot.done.is_some());
    let most = bot
        .yaws
        .windows(2)
        .map(|w| (i32::from(w[1]) - i32::from(w[0])).rem_euclid(65_536))
        .map(|d| d.min(65_536 - d))
        .max()
        .unwrap();
    println!("largest camera turn in one step: {most} (of 65536)");
    assert!(most <= GLIDE_TURN, "a camera jump of {most}");
    let at = bot.shots.iter().find(|s| s.1 == "landed-p3").unwrap().0;
    let off = (i32::from(bot.yaws[at]) - 16_384).abs();
    assert!(off < 5_461, "camera {off} off +x on P3");
    // On S2, before the glide, the camera still looked along +z.
    let s2 = bot.shots.iter().find(|s| s.1 == "on-s2").unwrap().0;
    let off = i32::from(bot.yaws[s2]).min(65_536 - i32::from(bot.yaws[s2]));
    assert!(off < 5_461, "camera {off} already turned on S2");
}

#[test]
fn sail_route_reaches_done() {
    for d in [Detail::Mid, Detail::Low, Detail::Max] {
        let bot = route(d);
        let Some(Note::Done(Area::Sail, Some(t))) = bot.done else {
            panic!("{d:?}: not done: hero {:?}", bot.p.play.hero);
        };
        let tenths = u32::from(t) * 10 / 30;
        println!(
            "{d:?}: done in {t} steps on the clock ({}.{} s), {} calls, {} steps in all",
            tenths / 10,
            tenths % 10,
            bot.calls,
            bot.p.play.steps
        );
        assert!(bot.calls < 1_200);
        // No camera input on the whole route (builder issue c): the glide
        // follow brought the camera behind the hero (within 30°).
        assert!(
            bot.log
                .iter()
                .all(|&(h, p, _)| (h | p) & (button::L | button::R) == 0),
            "{d:?}: L or R used"
        );
        let off = (i32::from(bot.p.play.cam.yaw) - i32::from(bot.p.play.hero.yaw)).abs();
        assert!(
            off.min(65536 - off) < 5461,
            "{d:?}: camera {off} off the hero's back"
        );
        assert_eq!(bot.p.play.hero.falls, 0);
        assert_eq!(bot.p.play.hero.ground, Some(P4));
        assert!(bot.p.play.done);
        assert_eq!(bot.p.best[Area::Sail.index()], t);
        assert!(bot.p.new_best);
        // The clock counted from leaving P0: fewer steps than the play.
        assert!(u32::from(t) < bot.p.play.steps);
        if d == Detail::Mid
            && let Ok(path) = std::env::var("SAIL_SCRIPT")
        {
            write_script(&bot, t, Path::new(&path));
        }
    }
}

#[test]
fn the_clock_starts_on_leaving_p0_and_stops_on_p4() {
    let mut p = Play::start(Area::Sail, Detail::Mid);
    assert_eq!(p.kit.time(), Some(0));
    // Standing still: no clock.
    for _ in 0..30 {
        p.step(&Pad::NONE);
    }
    assert_eq!(p.kit.time(), Some(0));
    assert!(!kit(&p).started);
    // Forward at full speed from z -5 m: the step that takes the middle
    // past z 0 (P0's far edge) is the clock's first.
    let mut first = None;
    for n in 0..200 {
        p.step(&pad(common::FWD, 0));
        if kit(&p).started {
            first = Some(n);
            assert!(!on_p0(p.hero.pos[0], p.hero.pos[2]));
            assert_eq!(p.kit.time(), Some(1));
            break;
        }
        assert!(on_p0(p.hero.pos[0], p.hero.pos[2]));
    }
    assert!(first.is_some());
    // The edges count as on P0.
    assert!(on_p0(cm(200), cm(0)) && on_p0(cm(-200), cm(-600)));
    assert!(!on_p0(cm(200) + 1, 0) && !on_p0(0, 1));
    // Landing on P4 is done; the area's after() says so only then.
    let mut s = Sail::new();
    let mut h = hero_at(3475, -210, 1900, P4);
    let b = blocks();
    let ev = HeroEvents {
        landed: Some(P3),
        ..HeroEvents::default()
    };
    assert!(!s.after(&mut h, &ev, &b));
    let ev = HeroEvents {
        landed: Some(P4),
        ..HeroEvents::default()
    };
    assert!(s.after(&mut h, &ev, &b));
    assert_eq!(s.time(), Some(2)); // started on the first call, 2 calls
}

#[test]
fn a_fall_puts_the_hero_back_within_four_steps_and_the_clock_runs_on() {
    let mut bot = Bot::new();
    bot.enter(Detail::Mid);
    // Stand still 12 steps: the start becomes the safe spot (10 steps).
    bot.wait(12);
    // Run off P0's left side (x -2 m) and fall: 4 m under the safe spot
    // is off.
    // (The safe spot follows him while he runs on P0, kept 0.5 m inside
    // its edges.)
    let mut fell_at = None;
    for _ in 0..200 {
        let s = stick_towards(-1.0, 0.0, bot.p.play.cam.yaw, 1.0);
        bot.call(0, 0, s);
        if bot.ev().fell {
            fell_at = Some((bot.calls, kit(&bot.p.play).steps));
            break;
        }
    }
    let (fell_call, clock) = fell_at.expect("a fall");
    let safe = bot.p.play.hero.safe;
    // x -2 m + 0.5 m = -1.5 m = -150 * 1024 / 25 = -6144 subs.
    assert_eq!(safe[0], -6_144);
    assert!(bot.p.play.hero.pos[1] < safe[1] - FALL_ROOM);
    assert!(clock > 0, "the clock started on leaving P0");
    assert_eq!(bot.p.play.hero.frozen, RESPAWN);
    // 4 more steps and he is back: frozen 3, 2, 1, then put back.
    let mut back = None;
    for _ in 0..10 {
        bot.wait(1);
        if bot.ev().respawned {
            back = Some(bot.calls - fell_call);
            break;
        }
    }
    assert_eq!(back, Some(4));
    assert_eq!(bot.frozen_most, 4);
    assert_eq!(bot.p.play.hero.pos, safe);
    assert_eq!(bot.p.play.hero.falls, 1);
    // The clock ran on through the frozen steps: +4.
    assert_eq!(kit(&bot.p.play).steps, clock + 4);
    assert!(!bot.p.play.done);
}

#[test]
fn the_best_is_kept_over_restart_and_areas() {
    let mut bot = route(Detail::Mid);
    let Some(Note::Done(Area::Sail, Some(t))) = bot.done else {
        panic!("not done");
    };
    assert_eq!(bot.p.best[Area::Sail.index()], t);
    // A on the done panel starts again (after the half second it ignores
    // A): the clock at 0, the best kept.
    bot.done = None;
    bot.press(button::A);
    assert!(bot.p.play.done, "A at once must not skip the DONE panel");
    bot.wait(15);
    bot.press(button::A);
    assert!(!bot.p.play.done);
    assert_eq!(*kit(&bot.p.play), Sail::new());
    assert_eq!(bot.p.best[Area::Sail.index()], t);
    // RESTART from the pause menu (START, three downs, A): the same.
    bot.wait(3);
    bot.call(button::START, button::START, (0, 0));
    bot.call(0, 0, (0, 0));
    bot.press(button::DOWN);
    bot.press(button::DOWN);
    bot.press(button::DOWN);
    bot.call(button::A, button::A, (0, 0));
    assert_eq!(bot.p.play.steps, 0);
    assert_eq!(*kit(&bot.p.play), Sail::new());
    assert_eq!(bot.p.best[Area::Sail.index()], t);
    // Another area and back: kept.
    bot.p.best[Area::Machines.index()] = 0;
    let mut q = Proto::new();
    q.best = bot.p.best;
    assert_eq!(q.best[Area::Sail.index()], t);
    // A slower finish does not replace it; a faster one does.
    let mut s = Sail::new();
    s.started = true;
    s.steps = t + 30;
    assert!(s.time().unwrap() > t);
}

#[test]
fn the_hud_shows_the_clock_the_best_line_and_the_sail_bar() {
    let mut s = Sail::new();
    let mut u = Hud::default();
    s.hud(&mut u);
    assert_eq!(u.text[0].as_str(), "SAIL");
    assert_eq!(u.text[1].as_str(), "0.0");
    assert!(u.best_line);
    assert_eq!(u.low_bar, None);
    assert_eq!(u.label.map(|l| l.1), Some("GOAL"));
    // 372 steps: 372 * 10 / 30 = 124 tenths, "12.4"; 21 glide steps left:
    // 21 / 2 = 10 px.
    s.steps = 372;
    s.glide_left = 21;
    let mut u = Hud::default();
    s.hud(&mut u);
    assert_eq!(u.text[1].as_str(), "12.4");
    assert_eq!(u.low_bar, Some(10));
}

// ---- The simulator script ----------------------------------------------------------------

/// The simulator's frame of the route's first call (the home tiles and
/// the PROTO3D start, as in `tests/machines.rs`).
const SCRIPT_FIRST: usize = 225;

/// Writes the MID route as a simulator script (`--script`): the home
/// prelude, then one game step a frame, the buttons as presses and
/// releases (A south, B east, START plus, UP and DOWN the d-pad), the
/// left stick as `stick left`, and the marked pictures.
fn write_script(bot: &Bot, t: u16, path: &Path) {
    use std::fmt::Write as _;
    let mut s = String::new();
    s.push_str("# SAIL: the route of tests/sail.rs (sail_route_reaches_done, MID),\n");
    s.push_str("# written by that test with SAIL_SCRIPT=<this file>. Do not edit by hand.\n");
    let _ = writeln!(
        s,
        "# The test finishes with {t} steps on the clock; the simulator log must say the same."
    );
    s.push_str("# home: to the 10th tile and start PROTO3D\n");
    for i in 0..9 {
        let f = 150 + 6 * i;
        let _ = writeln!(s, "{f} press right\n{} release right", f + 1);
    }
    s.push_str("200 press south\n201 release south\n");
    s.push_str("# the area menu: DOWN, DOWN, A (south) takes SAIL; then the route\n");
    let names = [
        (button::UP, "up"),
        (button::DOWN, "down"),
        (button::A, "south"),
        (button::B, "east"),
        (button::START, "plus"),
        (button::L, "l"),
        (button::R, "r"),
    ];
    let mut held = 0u32;
    let mut stick = (0i16, 0i16);
    for (i, &(h, p, l)) in bot.log.iter().enumerate() {
        let f = SCRIPT_FIRST + i;
        for &(bit, name) in &names {
            if (held & bit != 0) && (h & bit == 0) {
                let _ = writeln!(s, "{f} release {name}");
            }
        }
        for &(bit, name) in &names {
            if (held & bit == 0 && h & bit != 0) || (p & bit != 0 && held & bit != 0) {
                let _ = writeln!(s, "{f} press {name}");
            }
        }
        held = h;
        if l != stick {
            let _ = writeln!(s, "{f} stick left {} {}", l.0, l.1);
            stick = l;
        }
        for &(at, name) in &bot.shots {
            if at == i {
                let _ = writeln!(s, "{f} shot  # {name}");
            }
        }
    }
    let end = SCRIPT_FIRST + bot.log.len();
    if stick != (0, 0) {
        let _ = writeln!(s, "{end} stick left 0 0");
    }
    for &(bit, name) in &names {
        if held & bit != 0 {
            let _ = writeln!(s, "{end} release {name}");
        }
    }
    let _ = writeln!(s, "{} shot  # done", end + 5);
    let _ = writeln!(s, "{} shot  # done, later", end + 45);
    std::fs::write(path, s).expect("write the script");
    println!("script: {} ({} frames)", path.display(), end + 60);
}

// ---- Pictures ------------------------------------------------------------------------------

/// The moments: the start on P0; gliding over gap 2 towards P2; standing
/// on S2 facing +x with P3 and P4 ahead (the busiest view); gliding over
/// gap 4 towards the goal.
const MOMENTS: [&str; 4] = ["start", "glide-2", "on-s2", "glide-4"];

/// A play at a moment, the camera snapped behind the hero and turned
/// `turn` more.
fn moment(name: &str, d: Detail, turn: u16) -> Play {
    let mut p = Play::start(Area::Sail, d);
    let mut s = Sail::new();
    let quarter = 16_384u16;
    // (feet cm, block under, yaw, in the air, clock)
    let (at, top, yaw, air, clock): ([i16; 3], u8, u16, bool, u16) = match name {
        "glide-2" => ([200, 40, 1450], P2, 6_000, true, 150),
        "on-s2" => ([700, 40, 1900], S2, quarter, false, 260),
        "glide-4" => ([2900, 0, 1900], P4, quarter, true, 420),
        _ => (sa::START, P0, 0, false, 0),
    };
    let b: Vec<Block> = p.blocks().to_vec();
    let (x, z) = (cm(at[0]), cm(at[2]));
    let mut h = Hero::new([x, cm(at[1]), z], top);
    h.yaw = yaw;
    let held = if air {
        h.ground = None;
        h.air_steps = 30;
        h.vel = [
            GLIDE_RUN * game_3d::sin_fine(yaw) / 16_384,
            -GLIDE_FALL,
            GLIDE_RUN * game_3d::cos_fine(yaw) / 16_384,
        ];
        h.glide_left = 25;
        h.gliding = true;
        button::B
    } else {
        let y = b[usize::from(top)].surface(x, z);
        h.pos[1] = y;
        0
    };
    if clock > 0 {
        s.started = true;
        s.steps = clock;
    }
    s.glide_left = h.glide_left;
    s.gliding = h.gliding;
    p.kit = Kit::Sail(s);
    p.hero = h;
    p.cam.yaw = yaw.wrapping_add(turn);
    p.cam.snap(&p.hero, &b, p.world.props, d.level());
    p.shadow = p.hero.shadow(&b);
    let fwd = pad((0, -32_767), held);
    p.ring = p.hero.ring_with(&fwd, p.cam.yaw, &b, &p.kit.body(&p.hero));
    p
}

fn picture(p: &Play) -> (Box<Frame>, Stats) {
    let mut paint = Box::new(Painter::new());
    let mut f = Box::new(Frame::new());
    paint.picture(&mut f, p, &Menu::new(), false, 1);
    (f, paint.r.stats)
}

/// A PNG (8-bit RGB, stored zlib), each pixel `k` times `k` (as in
/// `tests/draw.rs`).
fn png(f: &Frame, k: usize) -> Vec<u8> {
    let (w, h) = (W * k, H * k);
    let mut raw = Vec::with_capacity((w * 3 + 1) * h);
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            let c = PALETTE
                .get(usize::from(f.0[(y / k) * W + x / k]))
                .copied()
                .unwrap_or(0);
            let (r, g, b) = ((c >> 11) & 31, (c >> 5) & 63, c & 31);
            raw.extend([
                (r * 255 / 31) as u8,
                (g * 255 / 63) as u8,
                (b * 255 / 31) as u8,
            ]);
        }
    }
    let mut z = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, ch) in chunks.iter().enumerate() {
        z.push(u8::from(i + 1 == chunks.len()));
        let n = ch.len() as u16;
        z.extend(n.to_le_bytes());
        z.extend((!n).to_le_bytes());
        z.extend_from_slice(ch);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend(((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend((w as u32).to_be_bytes());
    ihdr.extend((h as u32).to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    for (kind, data) in [(b"IHDR", ihdr), (b"IDAT", z), (b"IEND", Vec::new())] {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(&data);
        out.extend(&body);
        out.extend(crc32(&body).to_be_bytes());
    }
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

fn is_sky(c: u8) -> bool {
    (col::SKY0..=col::HAZE).contains(&c)
}

/// The surface a palette entry belongs to (as in `tests/draw.rs`).
fn surface(c: u8) -> Option<u8> {
    let m = match c {
        col::DARK_GRASS => mat::GRASS,
        col::DARK_STONE => mat::STONE,
        col::DARK_SAND => mat::SAND,
        col::DARK_BLOCK => mat::BLOCK_TOP,
        c if c < 96 => c % models::MATS,
        _ => return None,
    };
    if m == mat::LEAF || m == mat::LEAF2 {
        return None;
    }
    Some(match m {
        mat::GRASS2 | mat::RIM_GRASS => mat::GRASS,
        mat::RIM_STONE => mat::STONE,
        mat::RIM_SAND => mat::SAND,
        mat::RIM_BLOCK => mat::BLOCK_TOP,
        m => m,
    })
}

/// Crack pixels: sky in a single pixel between two pixels of the same
/// surface (as in `tests/draw.rs`).
fn cracks(f: &Frame) -> u32 {
    let at = |x: usize, y: usize| f.0[y * W + x];
    let same = |a: u8, b: u8| surface(a).is_some() && surface(a) == surface(b);
    let mut n = 0;
    for y in 1..H - 1 {
        for x in 1..W - 1 {
            if !is_sky(at(x, y)) {
                continue;
            }
            if same(at(x - 1, y), at(x + 1, y)) || same(at(x, y - 1), at(x, y + 1)) {
                n += 1;
            }
        }
    }
    n
}

fn hash(f: &Frame) -> u64 {
    f.0.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}

/// How many pixels show material `m` (lit, any shade).
fn shows(f: &Frame, m: u8) -> usize {
    f.0.iter()
        .filter(|&&c| c < 96 && c % models::MATS == m)
        .count()
}

#[test]
fn a_camera_sweep_stays_in_budget_with_few_cracks() {
    for d in Detail::ALL {
        let (mut most, mut most_at, mut worst, mut worst_at) = (0, ("", 0), 0, ("", 0));
        for name in MOMENTS {
            for k in 0..16u16 {
                let p = moment(name, d, k * 4096);
                let (f, st) = picture(&p);
                assert_eq!(st.dropped, 0, "{d:?} {name} {k}");
                if st.tris > most {
                    most = st.tris;
                    most_at = (name, k);
                }
                let c = cracks(&f);
                if c > worst {
                    worst = c;
                    worst_at = (name, k);
                }
            }
        }
        println!(
            "SAIL {} sweep: 64 pictures, most tris {most} at {most_at:?} (budget {}), cracks worst {worst} at {worst_at:?}",
            d.name(),
            BUDGET[usize::from(d.level())]
        );
        assert!(most <= BUDGET[usize::from(d.level())], "{d:?}: {most}");
        assert!(worst <= 9, "{d:?}: {worst} crack pixels");
    }
}

#[test]
fn the_moments_show_the_hero_the_sail_and_the_goal() {
    for d in Detail::ALL {
        for name in MOMENTS {
            let p = moment(name, d, 0);
            let (f, _) = picture(&p);
            // The hero is never hidden: his crimson and his cap show.
            assert!(shows(&f, mat::HERO) > 20, "{d:?} {name}: crimson");
            assert!(shows(&f, mat::CAP) > 3, "{d:?} {name}: cap");
            // The sail shows while gliding, and only then.
            let gliding = name.starts_with("glide");
            assert_eq!(p.pose() == Pose::Glide, gliding, "{d:?} {name}: pose");
            if gliding {
                assert!(shows(&f, mat::SAIL) > 20, "{d:?} {name}: sail");
            } else {
                assert_eq!(shows(&f, mat::SAIL), 0, "{d:?} {name}: no sail");
            }
        }
        // Facing P4 over the last gap: the beacon's gold (from S2 it is
        // 27 m away and past the far cut at some details).
        let (f, _) = picture(&moment("glide-4", d, 0));
        assert!(shows(&f, mat::GOLD) > 0, "{d:?} glide-4: beacon");
        // The ring is on the top the glide lands on.
        assert_eq!(moment("glide-2", d, 0).ring.map(|r| r.top), Some(P2));
        assert_eq!(moment("glide-4", d, 0).ring.map(|r| r.top), Some(P4));
    }
}

/// Golden pictures: a hash of three moments at each detail, kept in
/// `tests/golden/sail.txt` (as MACHINES keeps its own).
#[test]
fn golden_pictures_of_three_moments() {
    let dir = std::env::var_os("PROTO3D_PICTURES");
    let mut text = String::from("# PROTO3D SAIL: FNV-1a hashes of three moments at each detail\n");
    for d in Detail::ALL {
        for name in &MOMENTS[1..] {
            let (f, st) = picture(&moment(name, d, 0));
            let det = d.name().to_lowercase();
            println!("SAIL {det} {name}: tris {} cracks {}", st.tris, cracks(&f));
            text += &format!("{det} {name} {:016x}\n", hash(&f));
            if let Some(dir) = &dir {
                std::fs::create_dir_all(dir).unwrap();
                let file = Path::new(dir).join(format!("sail-{det}-{name}.png"));
                std::fs::write(file, png(&f, 4)).unwrap();
            }
        }
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/sail.txt");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let want = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        text, want,
        "pictures changed: look at them, then UPDATE_GOLDEN=1"
    );
}

/// The steps keep their order: S1 is drawn with the raised blocks, and
/// every step's side starts on the top under it.
#[test]
fn the_steps_stand_on_each_other() {
    let b = blocks();
    let (p2, s1, s2) = (b[usize::from(P2)], b[usize::from(S1)], b[usize::from(S2)]);
    assert_eq!(s1.lo[1], p2.hi[1]); // -60
    assert_eq!(s2.lo[1], s1.hi[1]); // -10
    assert!(s1.flags & flag::UP != 0);
    // The tops one walks on are the spec's: P2 to x 5 m, S1 5..6.5 m,
    // S2 6.5..8.5 m; each 0.5 m over the one before.
    assert_eq!((p2.hi[1], s1.hi[1], s2.hi[1]), (-60, -10, 40));
    assert_eq!((s1.lo[0], s2.lo[0], s2.hi[0]), (500, 650, 850));
    // The gaps' lengths and drops (cm): 450 / 0, 650 / 60, 850 / 100,
    // 1050 / 150.
    let gap = |a: usize, c: usize, k: usize| b[c].lo[k] - b[a].hi[k];
    assert_eq!(gap(0, 1, 2), 450);
    assert_eq!(gap(1, 2, 2), 650);
    assert_eq!(b[2].lo[0] - b[1].hi[0], 100); // P2 starts 1 m to the right
    assert_eq!(gap(4, 5, 0), 850);
    assert_eq!(gap(5, 6, 0), 1050);
    let top = |i: usize| b[i].hi[1];
    assert_eq!(
        (top(1) - top(2), top(4) - top(5), top(5) - top(6)),
        (60, 100, 150)
    );
    // The retry height: 4 m (16384 subs = 4 * M).
    assert_eq!(FALL_ROOM, 4 * M);
}
