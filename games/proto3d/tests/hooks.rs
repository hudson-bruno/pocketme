//! The hooks the test areas use (AREAS-SPEC.md, section 2): the hero's
//! tuning ([`Body`]: weight, glide, kill height, respawn wait), riding a
//! moving top, and switched-off blocks ([`flag::HIDE`]).

mod common;

use common::{Lcg, flat, hero_at, jump_pad, pad, world};
use game_kit::{Frame, button};
use proto3d::area::Area;
use proto3d::draw::Painter;
use proto3d::hero::{Body, Hero};
use proto3d::island::{BLOCKS, Block, body_fits, first_hit, flag, inside_any, top_under};
use proto3d::maths::cm;
use proto3d::pause::Menu;
use proto3d::play::{Detail, Play};
use proto3d::step::Pad;
use proto3d::tune::*;

/// A flat ground, top at 0, x and z from -10 m to 10 m.
const GROUND: Block = flat([-1000, -60, -1000], [1000, 0, 1000]);

const FWD: (i16, i16) = common::FWD;

/// The glide's tuning (SAIL's).
const GLIDE: Body = Body {
    glide: true,
    ..Body::NORMAL
};

/// The heavy tuning (MACHINES, carrying the weight).
const HEAVY: Body = Body {
    run: HEAVY_RUN,
    jump: HEAVY_JUMP,
    ..Body::NORMAL
};

#[test]
fn body_normal_is_today() {
    assert_eq!(
        Body::NORMAL,
        Body {
            run: RUN,
            jump: JUMP,
            glide: false,
            kill_y: KILL_Y,
            respawn: RESPAWN_STEPS,
        }
    );
    // The hero: `step` and `step_with(NORMAL)` agree in every field.
    let mut rng = Lcg(17);
    let start = Play::new().hero;
    let (mut a, mut b) = (start, start);
    let mut last = Pad::NONE;
    for n in 0..2_000 {
        last = rng.pad(&last);
        let yaw = (n as u16).wrapping_mul(97);
        let ea = a.step(&last, yaw, &BLOCKS);
        let eb = b.step_with(&last, yaw, &BLOCKS, &Body::NORMAL);
        assert_eq!((a, ea), (b, eb), "step {n}");
        assert_eq!(
            a.ring(&last, yaw, &BLOCKS),
            b.ring_with(&last, yaw, &BLOCKS, &Body::NORMAL)
        );
    }
    // A whole play: ISLAND started from the area menu is the same as the
    // first prototype's play, fingerprint for fingerprint.
    let mut rng = Lcg(19);
    let (mut p, mut q) = (Play::new(), Play::start(Area::Island, Detail::START));
    let mut last = Pad::NONE;
    for n in 0..2_000 {
        last = rng.pad(&last);
        p.step(&last);
        q.step(&last);
        assert_eq!(p.fingerprint(), q.fingerprint(), "step {n}");
    }
}

/// The highest the feet get in a held jump from flat ground (subs).
fn peak(body: &Body) -> i32 {
    let blocks = [GROUND];
    let mut h = hero_at(0, 0, 0, 0);
    h.step_with(&jump_pad((0, 0), 0), 0, &blocks, body);
    let mut top = h.pos[1];
    for _ in 0..60 {
        h.step_with(&pad((0, 0), button::A), 0, &blocks, body);
        top = top.max(h.pos[1]);
    }
    top
}

#[test]
fn a_weight_changes_the_jump_and_the_run() {
    // MACHINES' numbers (AREAS-SPEC.md, section 3): the heavy jump's peak
    // is 2,286 subs (0.56 m), the light one 4,950 (1.21 m).
    assert_eq!(peak(&HEAVY), 2_286);
    assert_eq!(peak(&Body::NORMAL), 4_950);
    // The top running speed is the body's.
    let blocks = [GROUND];
    for (body, top) in [(HEAVY, HEAVY_RUN), (Body::NORMAL, RUN)] {
        let mut h = hero_at(0, 0, -800, 0);
        for _ in 0..40 {
            h.step_with(&pad(FWD, 0), 0, &blocks, &body);
        }
        assert_eq!(h.vel[2], top);
    }
}

/// A hero in the air, `y` cm over the ground, still.
fn in_air(y: i16) -> Hero {
    let mut h = hero_at(0, y, 0, 0);
    h.ground = None;
    h.air_steps = 10;
    h
}

#[test]
fn the_glide_limits_the_fall() {
    let blocks = [GROUND];
    let b = pad((0, 0), button::B);
    // From still: the fall speed grows to -GLIDE_FALL and stays there.
    let mut h = in_air(2_000);
    for _ in 0..10 {
        h.step_with(&b, 0, &blocks, &GLIDE);
        assert!(h.gliding);
        assert!(h.vel[1] >= -GLIDE_FALL);
    }
    assert_eq!(h.vel[1], -GLIDE_FALL);
    // From the fastest fall: braked by FALL_BRAKE a step, 6 steps.
    let mut h = in_air(2_000);
    h.vel[1] = -FALL_MAX;
    let mut n = 0;
    while h.vel[1] != -GLIDE_FALL {
        h.step_with(&b, 0, &blocks, &GLIDE);
        n += 1;
        assert!(n < 20);
    }
    assert_eq!(n, 6);
    // The glide runs out after GLIDE_STEPS steps; then a normal fall.
    let mut h = in_air(4_000);
    let mut glided = 0;
    for _ in 0..100 {
        h.step_with(&b, 0, &blocks, &GLIDE);
        glided += u32::from(h.gliding);
    }
    assert_eq!(glided, u32::from(GLIDE_STEPS));
    assert_eq!(h.glide_left, 0);
    assert!(h.vel[1] < -GLIDE_FALL);
    // Landing gives it back.
    for _ in 0..200 {
        h.step_with(&b, 0, &blocks, &GLIDE);
        if h.ground.is_some() {
            break;
        }
    }
    assert_eq!(h.ground, Some(0));
    assert_eq!(h.glide_left, GLIDE_STEPS);
    // Running while gliding: the target is GLIDE_RUN.
    let mut h = in_air(4_000);
    for _ in 0..40 {
        h.step_with(&pad(FWD, button::B), 0, &blocks, &GLIDE);
    }
    assert_eq!(h.vel[2], GLIDE_RUN);
}

#[test]
fn the_glide_needs_b_the_air_and_the_body() {
    let blocks = [GROUND];
    // A jumps but never glides (A alone, held, all the way down).
    let mut h = hero_at(0, 0, 0, 0);
    h.step_with(&jump_pad((0, 0), 0), 0, &blocks, &GLIDE);
    for _ in 0..60 {
        h.step_with(&pad((0, 0), button::A), 0, &blocks, &GLIDE);
        assert!(!h.gliding);
    }
    // B on the ground does nothing.
    let (mut a, mut b) = (hero_at(0, 0, 0, 0), hero_at(0, 0, 0, 0));
    for _ in 0..30 {
        a.step_with(&pad(FWD, button::B), 0, &blocks, &GLIDE);
        b.step_with(&pad(FWD, 0), 0, &blocks, &GLIDE);
        assert!(!a.gliding);
    }
    assert_eq!(a.pos, b.pos);
    // Without the body's glide, B in the air does nothing.
    let (mut a, mut b) = (in_air(1_000), in_air(1_000));
    for _ in 0..30 {
        a.step_with(&pad(FWD, button::B), 0, &blocks, &Body::NORMAL);
        b.step_with(&pad(FWD, 0), 0, &blocks, &Body::NORMAL);
    }
    assert_eq!(a.pos, b.pos);
    assert!(!a.gliding);
}

#[test]
fn the_ring_follows_the_glide() {
    // Over a gap at 1.5 m, running at the glide's speed, a landing top
    // from z 1.5 m on.
    let blocks = [flat([-500, -60, 150], [500, 0, 2000])];
    let p = pad(FWD, button::B);
    let mut h0 = in_air(150);
    h0.vel[2] = GLIDE_RUN;
    let ring = h0.ring_with(&p, 0, &blocks, &GLIDE).expect("a ring");
    let plain = h0.ring_with(&p, 0, &blocks, &Body::NORMAL).expect("a ring");
    let mut h = h0;
    for _ in 0..RING_LOOK {
        h.step_with(&p, 0, &blocks, &GLIDE);
        if h.ground.is_some() {
            break;
        }
    }
    assert_eq!(h.ground, Some(0));
    // The ring is where the glide lands, far past the plain fall's.
    assert_eq!(ring.spot, h.pos);
    assert!(ring.spot[2] > plain.spot[2] + cm(100));
}

#[test]
fn a_rider_moves_with_its_top() {
    // A lift 2 m square going up 5 cm a step to 2 m, then down again.
    let mut cur = [GROUND, flat([-100, 0, -100], [100, 50, 100])];
    let mut h = hero_at(0, 50, 0, 1);
    for n in 0..60 {
        h.step_with(&Pad::NONE, 0, &cur, &Body::NORMAL);
        let mut next = cur;
        next[1].hi[1] += if n < 30 { 5 } else { -5 };
        h.ride(&cur, &next);
        cur = next;
        assert_eq!(h.ground, Some(1), "step {n}");
        assert_eq!(h.pos[1], cm(cur[1].hi[1]), "step {n}");
    }
    // A top switched off under the feet drops the hero.
    let mut next = cur;
    next[1].flags |= flag::HIDE;
    h.ride(&cur, &next);
    assert_eq!(h.ground, None);
    h.step_with(&Pad::NONE, 0, &next, &Body::NORMAL);
    assert!(h.vel[1] < 0);
}

#[test]
fn the_body_sets_the_fall_height_and_the_wait() {
    let blocks = [GROUND];
    let body = Body {
        kill_y: cm(-100),
        respawn: 4,
        ..Body::NORMAL
    };
    let mut h = in_air(-50);
    h.safe = [0, 0, 0];
    let mut n = 0;
    while h.frozen == 0 {
        h.step_with(&Pad::NONE, 0, &blocks, &body);
        n += 1;
        assert!(n < 30);
    }
    assert_eq!(h.frozen, 4);
    let mut back = 0;
    while h.frozen > 0 {
        h.step_with(&Pad::NONE, 0, &blocks, &body);
        back += 1;
    }
    assert_eq!(back, 4);
}

static OPEN: [Block; 1] = [GROUND];
static WALLED: [Block; 2] = [GROUND, {
    let mut w = flat([-300, 0, 100], [300, 200, 150]);
    w.flags |= flag::HIDE;
    w
}];

#[test]
fn hide_blocks_are_neither_solid_nor_drawn() {
    let wall = &WALLED[1];
    let inside = [0, cm(100), cm(125)];
    assert!(body_fits(&WALLED, [0, 0, cm(125)]));
    assert_eq!(inside_any(&WALLED, inside), None);
    assert_eq!(
        top_under(&WALLED, 0, cm(125), cm(300)).map(|t| t.0),
        Some(0)
    );
    assert_eq!(
        first_hit(&WALLED, [0, cm(100), 0], [0, cm(100), cm(300)]),
        None
    );
    assert!(!wall.solid());
    // Running through where the wall is, the same as with no wall, and
    // the same pictures.
    let (a, b) = (world(&OPEN, [0, 0, -200]), world(&WALLED, [0, 0, -200]));
    let (mut p, mut q) = (Play::on(a, Detail::Mid), Play::on(b, Detail::Mid));
    let mut paint = Box::new(Painter::new());
    let (mut f, mut g) = (Box::new(Frame::new()), Box::new(Frame::new()));
    for n in 0..60 {
        let pd = pad(FWD, 0);
        p.step(&pd);
        q.step(&pd);
        assert_eq!(p.hero, q.hero, "step {n}");
        assert_eq!(p.cam.eye, q.cam.eye, "step {n}");
        paint.picture(&mut f, &p, &Menu::new(), false, 1);
        paint.picture(&mut g, &q, &Menu::new(), false, 1);
        assert!(f.0 == g.0, "step {n}: the pictures differ");
    }
    assert!(p.hero.pos[2] > cm(150), "the hero ran past the wall");
}
