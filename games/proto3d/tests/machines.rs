//! MACHINES (AREAS-SPEC.md, section 3, and the tests of section 6): the
//! weight's rules with their sums, the plate, the bridge, the plank, the
//! route through the game's own controls, the pictures and their budget.
//!
//! With `PROTO3D_PICTURES=<folder>` the golden moments are written there
//! as PNG files (4x). With `UPDATE_GOLDEN=1` `tests/golden/machines.txt`
//! is written again.

mod common;

use std::path::Path;

use common::{hero_at, pad, stick_towards};
use game_3d::Stats;
use game_kit::{Frame, H, W, button};
use proto3d::area::machines::{
    self as mc, A, BOB, BRIDGE, C, D, Machines, PIVOT, PLANK, PLATE, bob_rec, plank_rec,
};
use proto3d::area::{Area, Extra, Hud, Kit, MAX_EXTRAS};
use proto3d::draw::{PALETTE, Painter, col};
use proto3d::hero::{Body, Hero, HeroEvents};
use proto3d::island::{Block, body_fits, flag};
use proto3d::maths::cm;
use proto3d::models::{self, mat};
use proto3d::pause::Menu;
use proto3d::play::{Detail, Play};
use proto3d::step::Pad;
use proto3d::tune::{G, HEAVY_JUMP, HEAVY_RUN, JUMP, M, RUN, STEP_UP};
use proto3d::{Input, Note, Proto};

/// The budgets (triangles a picture), LOW, MID, HIGH and MAX (MAX: the most
/// triangles of the swept ISLAND views at MAX plus 5 %, as in
/// tests/max_level.rs).
const BUDGET: [u32; 4] = [160, 430, 940, 1324];

// ---- Helpers -----------------------------------------------------------------------

/// The area's blocks as they are at the start.
fn blocks() -> Vec<Block> {
    mc::WORLD.blocks.to_vec()
}

/// The blocks with the movers written from a state.
fn live(m: &Machines) -> Vec<Block> {
    let mut b = blocks();
    let n = b.len();
    m.movers(&mut b[n - mc::MOVERS..]);
    b
}

/// A pad with X pressed (and held), the stick still.
fn x_pad() -> Pad {
    Pad {
        held: button::X,
        pressed: button::X,
        ..Pad::NONE
    }
}

/// Runs a hero on its own with a body and a pad, camera yaw 0; the
/// highest feet seen.
fn run_body(h: &mut Hero, p: &Pad, body: &Body, n: usize, b: &[Block]) -> i32 {
    let mut top = h.pos[1];
    for _ in 0..n {
        h.step_with(p, 0, b, body);
        top = top.max(h.pos[1]);
    }
    top
}

/// One step of the area's own rules around a hand-set hero (what
/// `Play::step` does, without the camera).
fn area_step(m: &mut Machines, h: &mut Hero, p: &Pad, b: &mut [Block]) -> (HeroEvents, bool) {
    m.before(p, h, b);
    let body = m.body(h);
    let ev = h.step_with(p, 0, b, &body);
    let old = b.to_vec();
    let done = m.after(h, &ev, &old);
    let n = b.len();
    m.movers(&mut b[n - mc::MOVERS..]);
    h.ride(&old, b);
    (ev, done)
}

/// The yaw that faces world direction (dx, dz) (0 = +z, a quarter turn
/// = +x).
fn yaw_to(dx: i32, dz: i32) -> u16 {
    proto3d::maths::yaw_of(dx, dz).unwrap()
}

/// The area's state inside a play.
fn kit(p: &Play) -> &Machines {
    match &p.kit {
        Kit::Machines(m) => m,
        _ => panic!("not MACHINES"),
    }
}

fn kit_mut(p: &mut Play) -> &mut Machines {
    match &mut p.kit {
        Kit::Machines(m) => m,
        _ => panic!("not MACHINES"),
    }
}

fn hud(m: &Machines) -> Hud {
    let mut h = Hud::default();
    m.hud(&mut h);
    h
}

// ---- Weight --------------------------------------------------------------------------

#[test]
fn heavy_and_light_jump_peaks_are_the_sums_of_the_step_rule() {
    // The step rule takes G off the speed before each move, so the hero
    // rises (v-G) + (v-2G) + ... while that is above 0.
    // Heavy: 780 - 6*114 = 96 > 0, 780 - 7*114 < 0: six rises,
    // 6*780 - 114*(1+2+...+6) = 4680 - 114*21 = 4680 - 2394 = 2286 subs
    // (0.56 m).
    // Light: 1120 - 9*114 = 94 > 0: nine rises,
    // 9*1120 - 114*45 = 10080 - 5130 = 4950 subs (1.21 m).
    let flat = [common::flat([-2000, -60, -2000], [2000, 0, 2000])];
    for (jump, want) in [(HEAVY_JUMP, 2_286), (JUMP, 4_950)] {
        assert_eq!(
            want,
            if jump == HEAVY_JUMP {
                6 * 780 - 114 * 21
            } else {
                9 * 1120 - 114 * 45
            }
        );
        let body = Body {
            jump,
            ..Body::NORMAL
        };
        let mut h = hero_at(0, 0, 0, 0);
        let mut top = run_body(&mut h, &common::jump_pad((0, 0), 0), &body, 1, &flat);
        top = top.max(run_body(&mut h, &pad((0, 0), button::A), &body, 40, &flat));
        assert_eq!(top, want, "jump {jump}");
    }
    assert_eq!(G, 114);
}

#[test]
fn carrying_makes_the_hero_slow_and_the_body_heavy() {
    let mut m = Machines::new();
    let h = hero_at(0, 0, -100, A);
    let light = m.body(&h);
    assert_eq!((light.run, light.jump), (RUN, JUMP));
    m.carry = true;
    let heavy = m.body(&h);
    assert_eq!((heavy.run, heavy.jump), (HEAVY_RUN, HEAVY_JUMP));
    // 478 subs a step = 478*30/4096 = 3.50 m/s.
    assert_eq!(HEAVY_RUN * 30 * 100 / 4096, 350);
    // Fall room: the safe spot's height - 6 m.
    assert_eq!(heavy.kill_y, h.safe[1] - 6 * M);
    let flat = [common::flat([-3000, -60, -3000], [3000, 0, 3000])];
    let mut h = hero_at(0, 0, 0, 0);
    run_body(&mut h, &pad(common::FWD, 0), &heavy, 60, &flat);
    assert_eq!(h.vel[2], HEAVY_RUN, "top speed carrying");
}

#[test]
fn a_heavy_jump_does_not_cross_the_three_metre_gap_and_a_light_one_does() {
    // From A's far edge (z 0) towards C (z 3.0 m), running full speed,
    // jumping at the last moment. The bridge is still down (hidden).
    for (carry, crosses) in [(true, false), (false, true)] {
        let mut m = Machines::new();
        m.carry = carry;
        m.bob = [-150, 0, -250];
        let mut b = live(&m);
        let mut h = hero_at(-200, 0, -300, A);
        let fwd = pad(common::FWD, 0);
        let mut jumped = false;
        let mut reached = false;
        for _ in 0..200 {
            let p = if !jumped && h.pos[2] > cm(-25) {
                jumped = true;
                common::jump_pad(common::FWD, 0)
            } else if jumped {
                pad(common::FWD, button::A)
            } else {
                fwd
            };
            area_step(&mut m, &mut h, &p, &mut b);
            if h.ground == Some(C) {
                reached = true;
                break;
            }
            if h.frozen > 0 {
                break;
            }
        }
        assert!(jumped);
        assert_eq!(reached, crosses, "carrying {carry}");
    }
}

// ---- Pick up and set down ------------------------------------------------------------

#[test]
fn the_pick_up_reach_is_0_9_m_between_the_middles() {
    // The bob's middle is at x -150; the hero 89 cm to its right picks it
    // up (89 cm = 3645 subs <= 3686), at 91 cm (3727 subs) not.
    assert_eq!(cm(89), 3_645);
    assert_eq!(cm(91), 3_727);
    for (dx, yes) in [(89, true), (91, false)] {
        let mut m = Machines::new();
        let b = live(&m);
        let mut h = hero_at(-150 + dx, 0, -250, A);
        h.yaw = yaw_to(-1, 0);
        assert_eq!(m.can_pick(&h), yes, "{dx} cm");
        m.before(&x_pad(), &h, &b);
        assert_eq!(m.carry, yes);
    }
    // In the air, or standing on the bob itself: no.
    let m = Machines::new();
    let mut h = hero_at(-100, 0, -250, A);
    h.ground = None;
    assert!(!m.can_pick(&h));
    let h = hero_at(-150, 50, -250, BOB);
    assert!(!m.can_pick(&h));
    // The bob's top 0.3 m under the feet is the lowest, 0.8 m over them
    // the highest (subs: cm(30) = 1228, cm(80) = 3276). The bob on the
    // ground has its top at cm(50) = 2048: feet at 2048 + 1228 = 3276
    // yes, 3277 no; feet at 2048 - 3276 = -1228 yes, -1229 no.
    assert_eq!((mc::PICK_BELOW, mc::PICK_ABOVE), (1_228, 3_276));
    for (feet, yes) in [
        (3_276, true),
        (3_277, false),
        (-1_228, true),
        (-1_229, false),
    ] {
        let mut h = hero_at(-100, 0, -250, A);
        h.pos[1] = feet;
        assert_eq!(Machines::new().can_pick(&h), yes, "feet {feet}");
    }
}

#[test]
fn setting_down_puts_the_bob_on_the_spot_or_says_no_room() {
    // On A, facing -z from (0, -100): the spot is 75 cm ahead, (0, -175).
    let mut m = Machines::new();
    m.carry = true;
    let b = live(&m);
    let mut h = hero_at(0, 0, -100, A);
    h.yaw = yaw_to(0, -1);
    let s = m.spot(&h, &b);
    assert_eq!(s.at, [0, 0, -175]);
    assert_eq!(s.on, Some(A));
    assert!(s.ok);
    // The marker lies 1 cm over the top.
    assert_eq!(s.mark, [0, cm(1), cm(-175)]);
    m.before(&x_pad(), &h, &b);
    assert!(!m.carry);
    assert_eq!((m.bob, m.rest), ([0, 0, -175], A));
    let b = live(&m);
    assert_eq!(b[usize::from(BOB)].lo, [-25, 0, -200]);
    assert_eq!(b[usize::from(BOB)].hi, [25, 50, -150]);

    // Next to D's wall (x 3.1 m), facing +x from (260, 550) on C: the
    // spot (335, 550) is inside D: NO ROOM for 20 steps, still carrying.
    let mut m = Machines::new();
    m.carry = true;
    let b = live(&m);
    let mut h = hero_at(280, 0, 400, C);
    h.yaw = yaw_to(1, 0);
    let s = m.spot(&h, &b);
    assert!(!s.ok);
    m.before(&x_pad(), &h, &b);
    assert!(m.carry);
    assert_eq!(m.no_room, mc::NO_ROOM_STEPS);
    assert_eq!(hud(&m).msg, Some("NO ROOM"));
    for _ in 0..19 {
        m.before(&Pad::NONE, &h, &b);
    }
    assert_eq!(hud(&m).msg, Some("NO ROOM"), "shown for 20 pictures");
    m.before(&Pad::NONE, &h, &b);
    assert_eq!(hud(&m).msg, None);

    // Over the edge of A (z 0) facing +z from (0, -40): the spot's far
    // corners are over the gap: no.
    let mut m = Machines::new();
    m.carry = true;
    let b = live(&m);
    let mut h = hero_at(0, 0, -40, A);
    h.yaw = 0;
    assert!(!m.spot(&h, &b).ok);
    // A top more than 0.3 m under the feet: the hero on the bob (top 50)
    // facing -x, the spot on A (top 0): 50 > 30, no.
    let mut m = Machines::new();
    m.carry = true;
    m.bob = [-150, 0, -250];
    let mut b = live(&m);
    // The bob is resting there in this test, the hero standing on it.
    b[usize::from(BOB)] = bob_rec([-150, 0, -250], false);
    let mut h = hero_at(-150, 50, -250, BOB);
    h.yaw = yaw_to(-1, 0);
    let s = m.spot(&h, &b);
    assert_eq!(s.on, Some(A));
    assert!(!s.ok, "50 cm down");
}

#[test]
fn the_marker_is_gold_where_the_bob_can_go_and_dark_where_not() {
    let mut p = Play::start(Area::Machines, Detail::Mid);
    kit_mut(&mut p).carry = true;
    p.hero = hero_at(0, 0, -100, A);
    p.hero.yaw = yaw_to(0, -1);
    let mut out = [Extra::NONE; MAX_EXTRAS];
    let n = p.kit.extras(&p, &mut out);
    assert_eq!(n, 3, "beacon, carried bob, marker");
    assert!(core::ptr::eq(out[0].model, &models::BEACON));
    assert!(core::ptr::eq(out[1].model, &models::BOB));
    assert!(core::ptr::eq(out[2].model, &models::MARKER_OK));
    // The carried bob: over the head, 10 cm ahead, its bottom 1.30 m
    // over the feet.
    assert_eq!(out[1].pos, [0, cm(130), cm(-100) - cm(10)]);
    p.hero = hero_at(280, 0, 400, C);
    p.hero.yaw = yaw_to(1, 0);
    let n = p.kit.extras(&p, &mut out);
    assert_eq!(n, 3);
    assert!(core::ptr::eq(out[2].model, &models::MARKER_NO));
    // In the air: no marker; not carrying: only the beacon.
    p.hero.ground = None;
    assert_eq!(p.kit.extras(&p, &mut out), 2);
    kit_mut(&mut p).carry = false;
    assert_eq!(p.kit.extras(&p, &mut out), 1);
}

// ---- The plate and the bridge ----------------------------------------------------------

/// A state with the bob resting on the plate's middle.
fn bob_on_plate() -> Machines {
    let mut m = Machines::new();
    m.bob = [210, mc::PLATE_UP, -90];
    m.rest = PLATE;
    m
}

#[test]
fn the_plate_lights_one_pip_for_the_hero_two_for_the_bob_three_for_both() {
    // The hero alone on the plate: 1.
    let mut m = Machines::new();
    let mut b = live(&m);
    let mut h = hero_at(210, mc::PLATE_UP, -90, PLATE);
    area_step(&mut m, &mut h, &Pad::NONE, &mut b);
    assert_eq!(m.plate_load, 1);
    let u = hud(&m);
    assert_eq!((u.pips, u.lit), (3, 1));
    // Loaded, its top is 3 cm instead of 6.
    assert_eq!(b[usize::from(PLATE)].hi[1], 3);
    assert_eq!(h.pos[1], cm(3), "the hero rode it down");
    // The bob alone: 2.
    let mut m = bob_on_plate();
    let mut b = live(&m);
    let mut h = hero_at(0, 0, -300, A);
    area_step(&mut m, &mut h, &Pad::NONE, &mut b);
    assert_eq!(m.plate_load, 2);
    assert_eq!(hud(&m).lit, 2);
    assert_eq!(m.bob[1], 3, "the bob rides the plate down");
    // The hero carrying the bob: 3.
    let mut m = Machines::new();
    m.carry = true;
    let mut b = live(&m);
    let mut h = hero_at(210, mc::PLATE_UP, -90, PLATE);
    area_step(&mut m, &mut h, &Pad::NONE, &mut b);
    assert_eq!(m.plate_load, 3);
    assert_eq!(hud(&m).lit, 3);
    // The hero standing on the bob that rests on the plate: 2 + 1 = 3.
    let mut m = bob_on_plate();
    let mut b = live(&m);
    let mut h = hero_at(210, mc::PLATE_UP + 50, -90, BOB);
    area_step(&mut m, &mut h, &Pad::NONE, &mut b);
    assert_eq!(m.plate_load, 3);
    // Nobody: no pips.
    let mut m = Machines::new();
    let mut b = live(&m);
    let mut h = hero_at(0, 0, -300, A);
    area_step(&mut m, &mut h, &Pad::NONE, &mut b);
    assert_eq!(hud(&m).pips, 0);
}

#[test]
fn three_pips_for_15_steps_latch_the_plate_and_the_bridge_rises_in_20() {
    let mut m = Machines::new();
    m.carry = true;
    let mut b = live(&m);
    let mut h = hero_at(210, mc::PLATE_UP, -90, PLATE);
    // 14 steps: not yet.
    for _ in 0..14 {
        area_step(&mut m, &mut h, &Pad::NONE, &mut b);
    }
    assert!(!m.latched);
    assert_eq!(m.held3, 14);
    assert!(!b[usize::from(BRIDGE)].solid());
    // The 15th: latched, the bridge shown at -1.0 m.
    area_step(&mut m, &mut h, &Pad::NONE, &mut b);
    assert!(m.latched);
    assert_eq!(hud(&m).text[1].as_str(), "OPEN");
    assert!(b[usize::from(BRIDGE)].solid());
    assert_eq!(b[usize::from(BRIDGE)].hi[1], -100);
    assert_eq!(b[usize::from(BRIDGE)].lo[1], -160);
    // 20 more steps of 5 cm: -100 + 20*5 = 0.
    for k in 1..=20 {
        area_step(&mut m, &mut h, &Pad::NONE, &mut b);
        assert_eq!(m.bridge, -100 + 5 * k);
    }
    assert_eq!(b[usize::from(BRIDGE)].hi[1], 0);
    // Stepping off: it stays open.
    let mut h = hero_at(0, 0, -300, A);
    area_step(&mut m, &mut h, &Pad::NONE, &mut b);
    assert!(m.latched);
    // A break in the 15 resets the count: 10 steps, one off, 14 more.
    let mut m = Machines::new();
    m.carry = true;
    let mut b = live(&m);
    let mut on = hero_at(210, mc::PLATE_UP, -90, PLATE);
    let mut off = hero_at(0, 0, -300, A);
    for _ in 0..10 {
        area_step(&mut m, &mut on, &Pad::NONE, &mut b);
    }
    area_step(&mut m, &mut off, &Pad::NONE, &mut b);
    assert_eq!(m.held3, 0);
    for _ in 0..14 {
        area_step(&mut m, &mut on, &Pad::NONE, &mut b);
    }
    assert!(!m.latched);
}

/// Rules review F6, M2: the bob alone on the plate (two pips) never
/// latches it, however long it rests there.
#[test]
fn the_bob_alone_on_the_plate_never_latches_it() {
    let mut m = bob_on_plate();
    let mut b = live(&m);
    let mut h = hero_at(0, 0, -300, A);
    for _ in 0..(10 * usize::from(mc::LATCH)) {
        area_step(&mut m, &mut h, &Pad::NONE, &mut b);
        assert_eq!(m.plate_load, 2);
        assert_eq!(m.held3, 0);
        assert!(!m.latched);
    }
    assert!(!b[usize::from(BRIDGE)].solid());
}

#[test]
fn a_hero_in_the_gap_is_lifted_by_the_rising_bridge() {
    // Latched, the bridge at -60; a hero standing on nothing at -58 over
    // it: the next rise to -55 passes his feet and lifts him (3 cm < 5).
    let mut m = Machines::new();
    m.latched = true;
    m.bridge = -60;
    let b = live(&m);
    let mut h = hero_at(0, -58, 150, BRIDGE);
    h.ground = None;
    h.vel = [0, 0, 0];
    let ev = HeroEvents::default();
    m.after(&mut h, &ev, &b);
    assert_eq!(m.bridge, -55);
    assert_eq!(h.pos[1], cm(-55));
    assert_eq!(h.ground, Some(BRIDGE));
}

// ---- The plank ------------------------------------------------------------------------

#[test]
fn the_plank_swings_in_five_steps_and_never_lies_flat() {
    let mut m = Machines::new();
    m.bob = [-175, 40, 550];
    m.rest = PLANK;
    let mut b = live(&m);
    // RightDown at the start: right end 25, left 125, DOWN_X.
    let p = plank_rec(25);
    assert_eq!(
        (p.lo[1], p.hi[1], p.ramp),
        (25, 125, proto3d::island::ramp::DOWN_X)
    );
    // The bob (2) alone on the left: LeftDown, 20 cm a step.
    m.bob[1] = mc::to_cm(b[usize::from(PLANK)].surface(cm(-175), cm(550)));
    let mut h = hero_at(0, 0, 400, C);
    let mut seen = vec![m.right];
    for _ in 0..6 {
        area_step(&mut m, &mut h, &Pad::NONE, &mut b);
        seen.push(m.right);
    }
    assert_eq!(seen, [25, 45, 65, 85, 105, 125, 125]);
    assert!(!seen.contains(&75), "no flat record");
    assert!(m.left_down);
    let p = b[usize::from(PLANK)];
    assert_eq!(
        (p.lo[1], p.hi[1], p.ramp),
        (25, 125, proto3d::island::ramp::UP_X)
    );
    // The bob rode down: its bottom is the plank's top under its middle,
    // 25 + (250-175)*100/500 = 40 cm.
    assert_eq!(m.bob[1], 40);
}

#[test]
fn the_loads_count_on_each_half() {
    let mut m = Machines::new();
    // The light hero on the right half (x 1.0 m): R 1.
    let h = hero_at(100, 45, 550, PLANK);
    let l = m.loads(&h);
    assert_eq!((l.left, l.right, l.plate), (0, 1, 0));
    // Carrying on the left: L 3.
    m.carry = true;
    let h = hero_at(-100, 105, 550, PLANK);
    assert_eq!(m.loads(&h).left, 3);
    // The bob on the left (2), the light hero on the right (1): L > R.
    m.carry = false;
    m.bob = [-175, 40, 550];
    m.rest = PLANK;
    let h = hero_at(200, 115, 550, PLANK);
    let l = m.loads(&h);
    assert_eq!((l.left, l.right), (2, 1));
    // The hero on the bob counts at the bob's x: 2 + 1 on the left.
    let h = hero_at(-175, 90, 550, BOB);
    let l = m.loads(&h);
    assert_eq!((l.left, l.right), (3, 0));
}

#[test]
fn the_hero_rides_the_swing() {
    // RightDown; the carrying hero walks onto the left half at x -1.0 m:
    // the plank swings left down and the hero's feet follow the top.
    let mut m = Machines::new();
    m.carry = true;
    let mut b = live(&m);
    let p = b[usize::from(PLANK)];
    let y = p.surface(cm(-100), cm(550));
    let mut h = Hero::new([cm(-100), y, cm(550)], PLANK);
    for _ in 0..6 {
        area_step(&mut m, &mut h, &Pad::NONE, &mut b);
        let s = b[usize::from(PLANK)].surface(h.pos[0], h.pos[2]);
        assert_eq!(h.pos[1], s, "feet on the top");
        assert_eq!(h.ground, Some(PLANK));
    }
    assert_eq!(m.right, 125);
    // At x -1.0 m: 25 + 150*100/500 = 55 cm.
    assert_eq!(h.pos[1], cm(55));
}

#[test]
fn the_swing_holds_when_a_rider_would_hit_a_block() {
    // No block of the area hangs over the plank (D starts at x 3.1 m,
    // the plank ends at 2.5 m), so a lid placed by hand stands in for
    // one: 1.9 m up over x 2.0..2.5. The hero (1.26 m tall) stands at
    // x 2.3 m on the right end; the bob (2) on the left half asks for
    // LeftDown, so the right end wants to rise.
    let mut m = Machines::new();
    m.bob = [-175, 40, 550];
    m.rest = PLANK;
    let mut b = live(&m);
    m.bob[1] = mc::to_cm(b[usize::from(PLANK)].surface(cm(-175), cm(550)));
    // The lid replaces the pivot (not walkable, solid).
    b[usize::from(PIVOT)] = Block::flat([200, 190, 500], [250, 250, 600], mat::ROCK, mat::ROCK, 0);
    let y = b[usize::from(PLANK)].surface(cm(230), cm(550));
    let mut h = Hero::new([cm(230), y, cm(550)], PLANK);
    assert!(body_fits(&b, h.pos));
    // The feet at x 2.3 m, 480 cm from the left end:
    // right 25 (left 125): 125 - 100*480/500 = 29 cm, head 155;
    // right 45 (left 105): 105 - 60*480/500 = 47.4 cm, head 173.4;
    // right 65 (left 85): 85 - 20*480/500 = 65.8 cm, head 191.8 > 190.
    // So it swings once and holds at 45.
    let lid = b[usize::from(PIVOT)];
    for _ in 0..8 {
        let before = m.right;
        m.before(&Pad::NONE, &h, &b);
        let ev = h.step_with(&Pad::NONE, 0, &b, &m.body(&h));
        let old = b.clone();
        m.after(&mut h, &ev, &old);
        let n = b.len();
        m.movers(&mut b[n - mc::MOVERS..]);
        b[usize::from(PIVOT)] = lid;
        h.ride(&old, &b);
        assert!(body_fits(&b, h.pos), "the rider never overlaps the lid");
        assert!(m.right >= before);
    }
    // The goal is LeftDown (L 2 > R 1), but the end stopped short.
    assert!(m.left_down);
    assert_eq!(m.right, 45);
}

// ---- The map ------------------------------------------------------------------------

#[test]
fn no_block_leaves_a_low_gap_over_a_top() {
    // For every pair of solid blocks one over the other, the gap between
    // the lower one's top and the upper one's bottom is at most 25 cm or
    // at least 1.5 m (no head bump, no crawl space). The plank's wedge counts
    // down to its lower end. Checked at the start, with the bridge up and
    // with the plank at each position.
    // In the middle of a swing (1/6 s) the plank's low end is lifted to
    // 45 or 65 cm: lower than the hero (1.26 m) and the bob (0.5 m +
    // 2 cm), so nothing is under it; a swing that would come down on
    // either holds (`swing_fits`). Those records only need a gap under
    // the hero's height.
    let mut states = vec![Machines::new()];
    for r in [25, 45, 65, 85, 105, 125] {
        let mut m = Machines::new();
        m.latched = true;
        m.bridge = 0;
        m.right = r;
        states.push(m);
    }
    for m in &states {
        let b = live(m);
        for (i, lo) in b.iter().enumerate() {
            for (j, up) in b.iter().enumerate() {
                if i == j || !lo.solid() || !up.solid() || lo.flags & flag::WALK == 0 {
                    continue;
                }
                let x0 = lo.lo[0].max(up.lo[0]);
                let x1 = lo.hi[0].min(up.hi[0]);
                let z0 = lo.lo[2].max(up.lo[2]);
                let z1 = lo.hi[2].min(up.hi[2]);
                if x0 >= x1 || z0 >= z1 || up.lo[1] < lo.hi[1] {
                    continue;
                }
                let gap = up.lo[1] - lo.hi[1];
                // A gap of at most 25 cm (STEP_UP) is a step, not a
                // space: the plank's low end over C is one.
                let resting = m.right == 25 || m.right == 125;
                let bad = if resting { 26..150 } else { 126..150 };
                assert!(!bad.contains(&gap), "{i} under {j}: gap {gap} cm ({m:?})");
            }
        }
    }
}

#[test]
fn d_is_reached_only_from_the_raised_end_of_the_plank() {
    // Light peak 4950 subs = 1.21 m. D's top is 2.45 m, and a top at
    // most STEP_UP (25 cm) over the feet is stepped onto: the feet must
    // reach 2.20 m.
    let peak = 4_950;
    let d = cm(mc::D_TOP) - STEP_UP;
    assert_eq!(mc::D_TOP, 245);
    // From the ground: 0 + 1.21 < 2.20.
    assert!(peak < d);
    // From the bob on the ground: 0.5 + 1.21 = 1.71 < 2.20.
    assert!(cm(50) + peak < d);
    // From the bob on the plank's low right end (its top at most
    // 25 + 20 + 50 = 95 cm where the bob can rest): 0.95 + 1.21 = 2.16
    // < 2.20. (With D at 2.20 m the feet needed only 1.95 m: the bob on
    // the low end was a way up without the plank's tip; review F1.)
    assert!(cm(95) + peak < d);
    // From the plank's low end (25 cm): 1.46 < 2.20.
    assert!(cm(25) + peak < d);
    // From the raised end (125 cm): 2.46 >= 2.20, margin 0.26 m.
    assert!(cm(125) + peak >= d);
    // cm(125) = 5120, cm(245) = 250880/25 = 10035 (rounded down),
    // STEP_UP 1024: 5120 + 4950 - 9011 = 1059 subs (0.26 m).
    assert_eq!(cm(125) + peak - d, 1_059);
    // The same by play: from the ground right under D's wall, jumping at
    // it, the hero bumps and falls back.
    let m = Machines::new();
    let b = live(&m);
    let mut h = hero_at(280, 0, 550, C);
    let top = run_body(
        &mut h,
        &common::jump_pad((32_767, 0), 0),
        &Body::NORMAL,
        1,
        &b,
    );
    let top = top.max(run_body(
        &mut h,
        &pad((32_767, 0), button::A),
        &Body::NORMAL,
        60,
        &b,
    ));
    assert!(top < d);
    assert_ne!(h.ground, Some(D));
}

// ---- The route --------------------------------------------------------------------------

/// The game driven through [`Proto::control`], one step a call.
struct Bot {
    p: Box<Proto>,
    calls: usize,
    done: Option<Note>,
    /// The play after each step (for checks that look at every step).
    each: fn(&Play),
    /// Every call's input (held, pressed, left stick), for the simulator
    /// script.
    log: Vec<(u32, u32, (i16, i16))>,
    /// Calls after which the simulator script takes a picture, named.
    shots: Vec<(usize, &'static str)>,
}

impl Bot {
    fn new() -> Bot {
        Bot {
            p: Box::new(Proto::new()),
            calls: 0,
            done: None,
            each: |_| {},
            log: Vec::new(),
            shots: Vec::new(),
        }
    }

    /// Marks the last call for a picture in the simulator script.
    fn shot(&mut self, name: &'static str) {
        self.shots.push((self.calls - 1, name));
    }

    fn call(&mut self, held: u32, pressed: u32, left: (i16, i16)) {
        let each = self.each;
        self.log.push((held, pressed, left));
        let n = self.p.control(
            &Input {
                held,
                pressed,
                left,
                delta_us: 33_333,
                ..Input::default()
            },
            each,
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

    fn wait(&mut self, n: usize) {
        for _ in 0..n {
            self.call(0, 0, (0, 0));
        }
    }

    /// Runs towards (x, z) cm until the hero's middle is within `near`
    /// cm and he stands (or `limit` calls).
    fn go(&mut self, to: [i16; 2], near: f64) {
        self.go_jump(to, near, None);
    }

    /// The same, pressing A (then holding it 20 steps) once when `jump`
    /// says so.
    fn go_jump(&mut self, to: [i16; 2], near: f64, jump: Option<fn(&Hero) -> bool>) {
        let mut jumped = false;
        let mut hold_a = 0;
        for _ in 0..400 {
            if self.done.is_some() {
                return;
            }
            let h = self.p.play.hero;
            let dx = f64::from(cm(to[0]) - h.pos[0]);
            let dz = f64::from(cm(to[1]) - h.pos[2]);
            let d = (dx * dx + dz * dz).sqrt() * 25.0 / 1024.0;
            if d < near && h.ground.is_some() && h.frozen == 0 && !(jumped && hold_a > 0) {
                return;
            }
            let speed = (d / 100.0).clamp(0.3, 1.0);
            let s = stick_towards(dx, dz, self.p.play.cam.yaw, speed);
            if let Some(j) = jump
                && !jumped
                && h.ground.is_some()
                && j(&h)
            {
                jumped = true;
                hold_a = 20;
                self.call(button::A, button::A, s);
                self.shots.push((self.calls + 6, "jump"));
            } else if hold_a > 0 {
                hold_a -= 1;
                self.call(button::A, 0, s);
            } else {
                self.call(0, 0, s);
            }
        }
        panic!(
            "not at {to:?} in 400 steps: hero {:?} kit {:?}",
            self.p.play.hero,
            kit(&self.p.play)
        );
    }

    fn kit(&self) -> &Machines {
        kit(&self.p.play)
    }
}

/// The route of the spec, from the area menu to done.
fn route(detail: Detail) -> Bot {
    let mut bot = Bot::new();
    bot.each = |p| {
        // No bump into anything along the way and never under 0.
        assert!(!p.events.hero.bumped, "bumped at {:?}", p.hero);
        assert_eq!(p.hero.falls, 0, "fell at {:?}", p.hero);
    };
    // The area menu opens at the start: DOWN, A gives MACHINES.
    bot.press(button::DOWN);
    bot.press(button::A);
    assert_eq!(bot.p.play.area, Area::Machines);
    if detail != Detail::START {
        // START opens the pause menu on the start's line (HIGH); UP or
        // DOWN to the wanted line, A chooses it.
        bot.press(button::START);
        let (from, to) = (Detail::START.level(), detail.level());
        let key = if to < from { button::UP } else { button::DOWN };
        for _ in 0..from.abs_diff(to) {
            bot.press(key);
        }
        bot.press(button::A);
    }
    assert_eq!(bot.p.play.detail, detail);
    bot.shot("start");
    // 1. To the bob, X picks it up.
    bot.go([-80, -250], 12.0);
    bot.press(button::X);
    assert!(bot.kit().carry, "picked up");
    bot.shot("picked-up");
    let mut u = Hud::default();
    bot.p.play.kit.hud(&mut u);
    assert_eq!(u.text[0].as_str(), "CARRY");
    // 2. Onto the plate: three pips, the latch, the bridge up.
    bot.go([210, -90], 15.0);
    assert_eq!(bot.p.play.hero.ground, Some(PLATE));
    bot.shot("on-plate");
    for _ in 0..60 {
        if bot.kit().bridge == 0 {
            break;
        }
        bot.wait(1);
    }
    assert!(bot.kit().latched);
    assert_eq!(bot.kit().bridge, 0);
    bot.shot("bridge-up");
    // 3. Over the bridge onto C.
    bot.go([0, -20], 20.0);
    bot.go([0, 330], 20.0);
    assert_eq!(bot.p.play.hero.ground, Some(C));
    bot.shot("on-c");
    // 4. Along C to the plank's low right end, then up it to the left
    // half: the plank tips left.
    bot.go([265, 430], 15.0);
    bot.go([265, 550], 10.0);
    bot.go([-40, 550], 12.0);
    bot.go([-100, 550], 8.0);
    assert_eq!(bot.p.play.hero.ground, Some(PLANK));
    bot.wait(8);
    assert!(bot.kit().left_down);
    assert_eq!(bot.kit().right, 125);
    bot.shot("plank-tipped");
    // 5. Facing -x, X sets the bob down on the left half.
    bot.press(button::X);
    assert!(!bot.kit().carry, "set down: {:?}", bot.kit());
    assert_eq!(bot.kit().rest, PLANK);
    bot.shot("set-down");
    // 6. Light, up to the raised right end and over to D.
    bot.go_jump(
        [420, 550],
        30.0,
        Some(|h: &Hero| h.pos[0] >= cm(185) && h.ground == Some(PLANK)),
    );
    bot
}

#[test]
fn machines_route_reaches_done() {
    for d in [Detail::Mid, Detail::Low, Detail::Max] {
        let bot = route(d);
        let Some(Note::Done(Area::Machines, Some(t))) = bot.done else {
            panic!("{d:?}: not done: hero {:?}", bot.p.play.hero);
        };
        println!("{d:?}: done in {t} steps ({} calls)", bot.calls);
        assert!(bot.calls < 3_000);
        if d == Detail::Mid
            && let Ok(path) = std::env::var("MACHINES_SCRIPT")
        {
            write_script(&bot, t, Path::new(&path));
        }
        assert_eq!(bot.p.play.hero.ground, Some(D));
        assert!(bot.p.play.done);
        assert_eq!(bot.p.best[Area::Machines.index()], t);
    }
}

#[test]
fn a_fall_keeps_the_carry_and_the_machines_and_restart_resets_all() {
    let mut bot = Bot::new();
    bot.press(button::DOWN);
    bot.press(button::A);
    assert_eq!(bot.p.play.area, Area::Machines);
    let start = bot.p.play.hero.pos;
    // A state from later in the route: carrying, the plate latched, the
    // bridge up, the plank tipped left.
    {
        let m = kit_mut(&mut bot.p.play);
        m.carry = true;
        m.latched = true;
        m.bridge = 0;
        m.left_down = true;
        m.right = 125;
    }
    let before = *bot.kit();
    // Backwards (DOWN, the camera behind the hero facing +z) runs off
    // A's back edge: z -320 to -400 cm is 80 cm, then the fall of
    // FALL_ROOM 6 m, then the respawn.
    let mut respawned = false;
    for _ in 0..200 {
        bot.call(button::DOWN, 0, (0, 0));
        if bot.p.play.events.hero.respawned {
            respawned = true;
            break;
        }
    }
    assert!(respawned, "no respawn: hero {:?}", bot.p.play.hero);
    bot.wait(20);
    assert_eq!(bot.p.play.hero.falls, 1);
    assert_eq!(bot.p.play.hero.ground, Some(A), "back on the start pad");
    let after = *bot.kit();
    assert!(after.carry, "still carrying");
    assert!(after.latched && after.bridge == 0, "the bridge stays up");
    assert!(
        after.left_down && after.right == 125,
        "the plank keeps its tip"
    );
    assert_eq!(after.rest, before.rest);
    assert!(!bot.p.play.done);
    // RESTART: START opens the pause menu on HIGH (line 2, the start);
    // two downs reach RESTART; A resets the area, the hero and the clock.
    bot.call(button::START, button::START, (0, 0));
    bot.call(0, 0, (0, 0));
    bot.press(button::DOWN);
    bot.press(button::DOWN);
    bot.call(button::A, button::A, (0, 0));
    assert_eq!(bot.p.play.steps, 0);
    assert_eq!(*bot.kit(), Machines::new());
    assert_eq!(bot.p.play.hero.pos, start);
    assert_eq!(bot.p.play.hero.falls, 0);
}

/// The simulator's frame of the route's first call: the home tiles and
/// the PROTO3D start as in the area study (kept outside the repository).
const SCRIPT_FIRST: usize = 225;

/// Writes the MID route as a simulator script (`--script`): the home
/// prelude, then one game step a frame, the buttons as presses and
/// releases (A south, X west, START plus, UP and DOWN the d-pad), the
/// left stick as `stick left`, and the marked pictures.
fn write_script(bot: &Bot, t: u16, path: &Path) {
    use std::fmt::Write as _;
    let mut s = String::new();
    s.push_str("# MACHINES: the route of tests/machines.rs (machines_route_reaches_done, MID),\n");
    s.push_str("# written by that test with MACHINES_SCRIPT=<this file>. Do not edit by hand.\n");
    let _ = writeln!(
        s,
        "# The test finishes in {t} steps; the simulator log must say the same."
    );
    s.push_str("# home: to the 10th tile and start PROTO3D\n");
    for i in 0..9 {
        let f = 150 + 6 * i;
        let _ = writeln!(s, "{f} press right\n{} release right", f + 1);
    }
    s.push_str("200 press south\n201 release south\n");
    s.push_str("# the area menu: DOWN, A (south) takes MACHINES; then the route\n");
    let names = [
        (button::UP, "up"),
        (button::DOWN, "down"),
        (button::A, "south"),
        (button::X, "west"),
        (button::START, "plus"),
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

// ---- Pictures ---------------------------------------------------------------------------

/// The moments: the start, carrying on the plate with the bridge
/// rising, carrying on the plank's left half (the marker shown), and
/// light on the raised right end facing D.
const MOMENTS: [&str; 4] = ["start", "a-plate", "b-plank", "c-ledge"];

/// A play at a moment, the camera snapped behind the hero and turned
/// `turn` more.
fn moment(name: &str, d: Detail, turn: u16) -> Play {
    let mut p = Play::start(Area::Machines, d);
    let mut m = Machines::new();
    let (at, top, yaw) = match name {
        "a-plate" => {
            m.carry = true;
            m.latched = true;
            m.held3 = 15;
            m.plate_load = 3;
            m.bridge = -40;
            ([210, -90], PLATE, yaw_to(-1, 1))
        }
        "b-plank" => {
            m.carry = true;
            m.latched = true;
            m.bridge = 0;
            m.left_down = true;
            m.right = 125;
            ([-100, 550], PLANK, yaw_to(-1, 0))
        }
        "c-ledge" => {
            m.latched = true;
            m.bridge = 0;
            m.left_down = true;
            m.right = 125;
            m.bob = [-175, 40, 550];
            m.rest = PLANK;
            ([200, 550], PLANK, yaw_to(1, 0))
        }
        _ => ([mc::START[0], mc::START[2]], A, 0),
    };
    let n = usize::from(p.live.n);
    m.movers(&mut p.live.blocks[n - mc::MOVERS..n]);
    p.kit = Kit::Machines(m);
    let b: Vec<Block> = p.blocks().to_vec();
    let (x, z) = (cm(at[0]), cm(at[1]));
    let y = b[usize::from(top)].surface(x, z);
    p.hero = Hero::new([x, y, z], top);
    p.hero.yaw = yaw;
    p.cam.yaw = yaw.wrapping_add(turn);
    p.cam.snap(&p.hero, &b, p.world.props, d.level());
    p.shadow = p.hero.shadow(&b);
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
            "MACHINES {} sweep: 64 pictures, most tris {most} at {most_at:?} (budget {}), cracks worst {worst} at {worst_at:?}",
            d.name(),
            BUDGET[usize::from(d.level())]
        );
        assert!(most <= BUDGET[usize::from(d.level())], "{d:?}: {most}");
        assert!(worst <= 9, "{d:?}: {worst} crack pixels");
    }
}

#[test]
fn the_moments_show_the_hero_the_bob_and_the_goal() {
    for d in Detail::ALL {
        for name in MOMENTS {
            let (f, _) = picture(&moment(name, d, 0));
            // The hero is never hidden behind what he stands on or
            // carries: his crimson and his cap show.
            assert!(shows(&f, mat::HERO) > 20, "{d:?} {name}: crimson");
            assert!(shows(&f, mat::CAP) > 3, "{d:?} {name}: cap");
            // The bob (brass) shows, carried or resting ahead (at
            // c-ledge it rests behind the camera).
            if name != "c-ledge" {
                assert!(shows(&f, mat::BRASS) > 20, "{d:?} {name}: brass");
            }
        }
        // The marker is gold on the plank's left half.
        let (f, _) = picture(&moment("b-plank", d, 0));
        assert!(shows(&f, mat::GOLD) > 0, "{d:?}: marker");
        // Facing D: the beacon's gold.
        let (f, _) = picture(&moment("c-ledge", d, 0));
        assert!(shows(&f, mat::GOLD) > 0, "{d:?}: beacon");
        // On the plate after the latch: OPEN in place of the pips.
        let p = moment("a-plate", d, 0);
        let mut u = Hud::default();
        p.kit.hud(&mut u);
        assert_eq!((u.pips, u.text[1].as_str()), (0, "OPEN"));
    }
}

/// Golden pictures: a hash of each of the three moments of the route at
/// each detail, kept in `tests/golden/machines.txt` (beside ISLAND's
/// `pictures.txt`, which its own test compares whole).
#[test]
fn golden_pictures_of_three_moments() {
    let dir = std::env::var_os("PROTO3D_PICTURES");
    let mut text =
        String::from("# PROTO3D MACHINES: FNV-1a hashes of three moments at each detail\n");
    for d in Detail::ALL {
        for name in &MOMENTS[1..] {
            let (f, st) = picture(&moment(name, d, 0));
            let det = d.name().to_lowercase();
            println!(
                "MACHINES {det} {name}: tris {} cracks {}",
                st.tris,
                cracks(&f)
            );
            text += &format!("{det} {name} {:016x}\n", hash(&f));
            if let Some(dir) = &dir {
                std::fs::create_dir_all(dir).unwrap();
                let file = Path::new(dir).join(format!("machines-{det}-{name}.png"));
                std::fs::write(file, png(&f, 4)).unwrap();
            }
        }
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/machines.txt");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let want = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        text, want,
        "pictures changed: look at them, then UPDATE_GOLDEN=1"
    );
}
