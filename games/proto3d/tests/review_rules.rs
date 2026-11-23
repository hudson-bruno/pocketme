//! A reviewer's tests of the rules (PROTOTYPE-SPEC.md, sections 0 to 4):
//! the cases the builders' tests do not cover. Each test says what the
//! spec asks. A failing test here is a finding (see the review's
//! FINDINGS.md); it is kept failing on purpose until the code or the spec
//! changes.

mod common;

use common::*;
use game_kit::button;
use proto3d::hero::Hero;
use proto3d::island::{
    BLOCKS, Block, FLOAT, ISLAND, ISLET, MAIN, RAMP, STEP, TERRACE, body_fits, first_hit,
    inside_any, top_under,
};
use proto3d::maths::{angle_diff, cm, len2, sub};
use proto3d::play::{Detail, Play};
use proto3d::step::{Clock, Pad};
use proto3d::tune::*;

/// One big flat top, top at 0, 40 m square.
static BIG: [Block; 1] = [flat([-2000, -50, -2000], [2000, 0, 2000])];

/// A 2 m square top at 0 on its own.
static PAD_BOX: [Block; 1] = [flat([-100, -50, -100], [100, 0, 100])];

/// A hero in the air at `pos` (subs), not rising, past its coyote steps.
fn falling_at(pos: [i32; 3]) -> Hero {
    let mut h = Hero::new(pos, 0);
    h.ground = None;
    h.air_steps = COYOTE + 1;
    h
}

/// Play standing still on the main island, the camera behind.
fn still() -> Play {
    let mut play = Play::new();
    play.hero = hero_at(0, 0, -300, MAIN);
    play.cam.snap(&play.hero, &BLOCKS, ISLAND.props, 1);
    play
}

/// Checks the safe spot: on its top (not over open sky), with the body
/// fitting, and at least SAFE_IN inside the top's edges (the tops here are
/// all wider than 2 * SAFE_IN).
fn check_safe(h: &Hero, when: &str) {
    let s = h.safe;
    assert_eq!(
        top_under(&BLOCKS, s[0], s[2], s[1]),
        Some((h.safe_top, s[1])),
        "{when}: safe {s:?} not on its top {}",
        h.safe_top
    );
    assert!(body_fits(&BLOCKS, s), "{when}: body does not fit at {s:?}");
    let b = BLOCKS[usize::from(h.safe_top)].bounds();
    for k in [0, 2] {
        assert!(
            s[k] - b.lo[k] >= SAFE_IN && b.hi[k] - s[k] >= SAFE_IN,
            "{when}: safe {s:?} within 0.5 m of an edge of top {}",
            h.safe_top
        );
    }
}

// ---- Section 1: the jump's numbers -------------------------------------------

#[test]
fn review_a_full_jump_at_full_run_covers_19_steps_of_683_subs() {
    // Sums (the code's order: jump sets 1120, gravity, then position):
    // the feet after n steps are 1120 n - 57 n (n + 1); n = 18 gives 666,
    // n = 19 gives -380: the landing is in step 19. Across: 19 * 683 =
    // 12,977 subs (3.17 m).
    let mut h = hero_at(0, 0, -1500, 0);
    for _ in 0..10 {
        h.step(&pad(FWD, 0), 0, &BIG);
    }
    assert_eq!(h.vel[2], RUN, "full speed after 10 steps");
    let z0 = h.pos[2];
    h.step(&jump_pad(FWD, 0), 0, &BIG);
    let mut steps = 1;
    while h.ground.is_none() {
        h.step(&pad(FWD, button::A), 0, &BIG);
        steps += 1;
        assert!(steps < 40);
    }
    assert_eq!(steps, 19);
    assert_eq!(h.pos[2] - z0, 19 * RUN, "12,977 subs = 3.17 m");
}

#[test]
fn review_full_speed_is_the_same_at_any_camera_yaw() {
    // The stick turned by the camera's yaw (whole numbers): full speed
    // should stay within a few subs of RUN at any yaw.
    for yaw in (0..65_536u32).step_by(4_096 + 512) {
        let mut h = hero_at(0, 0, 0, 0);
        for _ in 0..12 {
            h.step(&pad(FWD, 0), yaw as u16, &BIG);
        }
        let v = len2(h.vel[0], h.vel[2]);
        assert!((RUN - v).abs() <= 3, "yaw {yaw}: speed {v}");
    }
}

// ---- Section 1: landing on an edge, and the rim ------------------------------

#[test]
fn review_landing_exactly_on_an_edge() {
    // The square touching the edge does not hold the hero; one sub over
    // it does. All four sides and a corner of a 2 m top.
    let e = cm(100);
    for (pos, lands) in [
        ([e + HALF - 1, cm(50), 0], true),
        ([e + HALF, cm(50), 0], false),
        ([-e - HALF + 1, cm(50), 0], true),
        ([-e - HALF, cm(50), 0], false),
        ([0, cm(50), e + HALF - 1], true),
        ([0, cm(50), e + HALF], false),
        ([0, cm(50), -e - HALF + 1], true),
        ([0, cm(50), -e - HALF], false),
        ([e + HALF - 1, cm(50), e + HALF - 1], true),
        ([e + HALF, cm(50), e + HALF - 1], false),
    ] {
        let mut h = falling_at(pos);
        for _ in 0..120 {
            h.step(&Pad::NONE, 0, &PAD_BOX);
            if h.ground.is_some() || h.frozen > 0 {
                break;
            }
        }
        assert_eq!(h.ground.is_some(), lands, "{pos:?}");
        if lands {
            assert_eq!(h.pos[1], 0);
            assert_eq!(h.pos[0], pos[0], "no sideways move on landing");
        }
    }
}

#[test]
fn review_a_hero_standing_on_a_rim_has_a_shadow() {
    // Spec 1, tests: "a shadow and no ring on every top". The hero stands
    // with the middle past the edge, the square still on the top.
    let mut h = falling_at([cm(100) + HALF - 1, cm(50), 0]);
    for _ in 0..30 {
        h.step(&Pad::NONE, 0, &PAD_BOX);
    }
    assert_eq!(h.ground, Some(0), "standing on the rim");
    assert!(
        h.shadow(&PAD_BOX).is_some(),
        "standing on a top with no shadow (and no ring)"
    );
}

#[test]
fn review_no_sky_tilt_while_standing_on_a_rim() {
    // Spec 2: SKY_TILT only "with no top under the hero".
    let mut play = still();
    // The main island's +x rim: middle 0.24 m past the edge.
    play.hero = Hero::new([cm(700) + HALF - 40, 0, 0], MAIN);
    play.cam.snap(&play.hero, &BLOCKS, ISLAND.props, 1);
    for _ in 0..20 {
        play.step(&Pad::NONE);
    }
    assert_eq!(play.hero.ground, Some(MAIN));
    assert_eq!(play.cam.tilt, 0, "tilted as if over open sky");
}

#[test]
fn review_a_jump_from_the_very_rim_and_each_coyote_step_crosses_the_gap() {
    for k in 0..=COYOTE {
        let mut h = hero_at(0, 0, 450, MAIN);
        // Run to the last step still on the top (the rim).
        let mut n = 0;
        loop {
            let mut t = h;
            t.step(&pad(FWD, 0), 0, &BLOCKS);
            if t.ground.is_none() {
                break;
            }
            h = t;
            n += 1;
            assert!(n < 100);
        }
        assert!(h.pos[2] > cm(700), "the middle is past the edge");
        for _ in 0..k {
            h.step(&pad(FWD, 0), 0, &BLOCKS);
        }
        assert_eq!(h.air_steps, k);
        let ev = h.step(&jump_pad(FWD, 0), 0, &BLOCKS);
        assert!(ev.jumped, "k {k}");
        let mut n = 0;
        while h.ground.is_none() && h.frozen == 0 {
            h.step(&pad(FWD, button::A), 0, &BLOCKS);
            n += 1;
            assert!(n < 60);
        }
        assert_eq!(h.ground, Some(ISLET), "k {k}: {:?}", h.pos);
        assert_eq!(h.jumps, 1);
    }
}

// ---- Section 1: jump buffer --------------------------------------------------

/// Drops the hero from 2 m and presses A `k` steps before the landing
/// step (0: in it). Gives the jumps made.
fn buffer_jumps(k: u32) -> u16 {
    let fall = |press: Option<u32>| {
        let mut h = falling_at([0, cm(200), 0]);
        let mut landed = None;
        for n in 0..60u32 {
            let p = if Some(n) == press {
                jump_pad((0, 0), 0)
            } else {
                pad((0, 0), button::A)
            };
            let ev = h.step(&p, 0, &BIG);
            if ev.landed.is_some() && landed.is_none() {
                landed = Some(n);
            }
        }
        (landed.unwrap(), h.jumps)
    };
    let (land, _) = fall(None);
    fall(Some(land - k)).1
}

#[test]
fn review_the_jump_buffer_keeps_a_press_for_buffer_steps() {
    // Spec: BUFFER 4 steps (0.13 s), the same as COYOTE (4 steps, which
    // the code gives in full). A press 3 steps before the landing step
    // is within 4 steps of the landing.
    for k in 0..=u32::from(BUFFER) - 1 {
        assert_eq!(
            buffer_jumps(k),
            1,
            "press {k} steps before the landing step"
        );
    }
    assert_eq!(buffer_jumps(u32::from(BUFFER) + 1), 0);
}

// ---- Section 1: sides, corners, the ramp and the floating block ---------------

#[test]
fn review_jumping_while_running_up_the_ramp_does_not_throw_the_hero() {
    // On the ramp's middle line (x = -3.5 m), running up it (+z), then a
    // jump. In the jump step the hero should move by its own speed only
    // (the speed itself changes by at most ACC in a step).
    let mut bad = Vec::new();
    for jump_at in [-130, -100, -50, 0, 50, 100] {
        let mut h = hero_at(-350, 0, -250, MAIN);
        for _ in 0..60 {
            if h.ground == Some(RAMP) && h.pos[2] >= cm(jump_at) {
                break;
            }
            h.step(&pad(FWD, 0), 0, &BLOCKS);
        }
        assert_eq!(h.ground, Some(RAMP), "jump at {jump_at}");
        let before = h;
        let ev = h.step(&jump_pad(FWD, 0), 0, &BLOCKS);
        assert!(ev.jumped);
        let dx = h.pos[0] - before.pos[0] - before.vel[0];
        let dz = h.pos[2] - before.pos[2] - before.vel[2];
        let off = len2(dx, dz);
        println!(
            "jump at z {jump_at} cm (ramp {} cm high): moved off its path by x {} cm, z {} cm",
            before.pos[1] * 25 / 1024,
            dx * 25 / 1024,
            dz * 25 / 1024
        );
        if off > ACC {
            bad.push((jump_at, dx * 25 / 1024, dz * 25 / 1024));
        }
    }
    assert!(bad.is_empty(), "thrown (jump z cm, x cm, z cm): {bad:?}");
}

#[test]
fn review_the_hero_is_pushed_out_of_box_corners() {
    // Diagonal runs into every corner of the step, the floating block and
    // the terrace's outer corner, from the main top. The body never ends a
    // step inside a box and never climbs the 0.5 m step.
    let d = 23_170;
    let corners: [([i16; 2], (i16, i16)); 9] = [
        // Step: x 1 to 3, z -1 to 1. Start 1 m out along the diagonal.
        ([0, -200], (d, -d)),
        ([400, -200], (-d, -d)),
        ([0, 200], (d, d)),
        ([400, 200], (-d, d)),
        // Floating block: x 2 to 3.6, z 3 to 4.6.
        ([100, 200], (d, -d)),
        ([460, 200], (-d, -d)),
        ([100, 560], (d, d)),
        ([460, 560], (-d, d)),
        // Terrace's outer corner (x -1.5, z 1.5), from +x and -z.
        ([-50, 50], (-d, -d)),
    ];
    for (from, s) in corners {
        let mut h = hero_at(from[0], 0, from[1], MAIN);
        // 25 steps: into the corner and a little along a face, never as
        // far as the island's edge.
        for n in 0..25 {
            h.step(&pad(s, 0), 0, &BLOCKS);
            assert!(
                body_fits(&BLOCKS, h.pos),
                "from {from:?}, step {n}: inside a box at {:?}",
                h.pos
            );
            assert_eq!(h.pos[1], 0, "from {from:?}: climbed");
        }
    }
}

#[test]
fn review_running_into_the_ramps_side() {
    // From either side along x, at every 20 cm of the ramp's length. Where
    // the ramp's side is at most STEP_UP high the hero steps on (and
    // crosses it); where it is higher it is pushed back.
    for z in (-140..=140).step_by(20) {
        let side = (z + 150) * 100 / 300; // ramp height there, cm
        for (x0, s) in [(-150i16, (-32_767i16, 0i16)), (-550, (32_767, 0))] {
            let mut h = hero_at(x0, 0, z, MAIN);
            let mut on = false;
            for _ in 0..22 {
                h.step(&pad(s, 0), 0, &BLOCKS);
                on |= h.ground == Some(RAMP);
                assert!(body_fits(&BLOCKS, h.pos), "z {z}, from {x0}: {:?}", h.pos);
            }
            if cm(side) <= STEP_UP {
                assert!(on, "z {z} cm ({side} cm high), from {x0}: not stepped on");
            } else {
                assert!(!on, "z {z} cm ({side} cm high), from {x0}: climbed");
                let near = if x0 > 0 || x0 == -150 {
                    h.pos[0] >= cm(-250) + HALF
                } else {
                    h.pos[0] <= cm(-450) - HALF
                };
                assert!(near, "z {z}, from {x0}: {:?}", h.pos);
            }
        }
    }
}

#[test]
fn review_nobody_jumps_under_the_floating_blocks_edge() {
    // Run and jump at the block from all four sides, from several
    // distances, A held: the body never overlaps it, nothing stands on it,
    // no head bump happens (its underside is below the head).
    let b = BLOCKS[usize::from(FLOAT)].bounds();
    for (from, s) in [
        ([280, 0, 150], FWD),
        ([280, 0, 600], (0, 32_767)),
        ([100, 0, 380], (32_767, 0)),
        ([500, 0, 380], (-32_767, 0)),
    ] {
        for jump_after in 0..8 {
            let mut h = hero_at(from[0], from[1], from[2], MAIN);
            for n in 0..50 {
                let p = if n == jump_after {
                    jump_pad(s, 0)
                } else {
                    pad(s, button::A)
                };
                let ev = h.step(&p, 0, &BLOCKS);
                assert!(!ev.bumped);
                assert_ne!(h.ground, Some(FLOAT));
                let over = b.overlaps_square(h.pos[0], h.pos[2], HALF);
                assert!(
                    !over || h.pos[1] >= b.hi[1] || h.head() <= b.lo[1],
                    "{from:?} jump {jump_after}: body in the block at {:?}",
                    h.pos
                );
            }
        }
    }
}

// ---- Section 1: falling off, the safe spot, respawn ----------------------------

#[test]
fn review_falls_off_every_side_come_back_at_a_safe_spot() {
    // Walk off each edge of the main island and of the islet (into the gap
    // from both sides, and off the outer edges).
    let back = (0, 32_767);
    let right = (32_767, 0);
    let left = (-32_767, 0);
    let cases: [([i16; 3], u8, (i16, i16)); 8] = [
        ([0, 0, 400], MAIN, FWD),
        ([0, 0, -400], MAIN, back),
        ([400, 0, -400], MAIN, right),
        ([-400, 0, -400], MAIN, left),
        ([0, 0, 1150], ISLET, back),
        ([0, 0, 1150], ISLET, FWD),
        ([0, 0, 1150], ISLET, right),
        ([0, 0, 1150], ISLET, left),
    ];
    for (from, top, s) in cases {
        let mut h = hero_at(from[0], from[1], from[2], top);
        let mut n = 0;
        while h.frozen == 0 {
            h.step(&pad(s, 0), 0, &BLOCKS);
            assert!(body_fits(&BLOCKS, h.pos), "{from:?}: inside at {:?}", h.pos);
            n += 1;
            assert!(n < 200, "{from:?}: never fell, at {:?}", h.pos);
        }
        assert_eq!(h.falls, 1);
        assert_eq!(h.safe_top, top, "{from:?}");
        check_safe(&h, "after the fall");
        let safe = h.safe;
        for _ in 0..RESPAWN_STEPS {
            h.step(&pad(s, 0), 0, &BLOCKS);
        }
        assert_eq!(h.frozen, 0);
        assert_eq!(h.pos, safe);
        assert_eq!(h.vel, [0; 3]);
        assert_eq!(h.ground, Some(top));
    }
}

#[test]
fn review_a_short_hop_into_the_gap_never_ends_inside_the_islet() {
    // Hops of every length (A held 1 to 12 steps) from the rim at full
    // speed: each lands on the islet or falls, never ends a step inside a
    // box (the islet's side is in the way of the short ones).
    for hold in 1..=12u32 {
        let mut h = hero_at(0, 0, 500, MAIN);
        let mut pressed = None;
        for _ in 0..150 {
            let p = match pressed {
                None if h.pos[2] >= cm(700) => {
                    pressed = Some(0u32);
                    jump_pad(FWD, 0)
                }
                Some(k) if k < hold => pad(FWD, button::A),
                _ => pad(FWD, 0),
            };
            if let Some(k) = pressed.as_mut() {
                *k += 1;
            }
            h.step(&p, 0, &BLOCKS);
            assert!(
                body_fits(&BLOCKS, h.pos),
                "hold {hold}: inside at {:?}",
                h.pos
            );
            if h.frozen > 0 || h.ground == Some(ISLET) {
                break;
            }
        }
        assert!(h.frozen > 0 || h.ground == Some(ISLET), "hold {hold}");
    }
}

#[test]
fn review_the_safe_spot_is_never_over_open_sky_nor_near_an_edge() {
    // The scripted route over every top, then a long random run.
    let mut play = Play::on(&ISLAND, Detail::High);
    follow(&mut play, &ROUTE, 400, |p| check_safe(&p.hero, "route"));
    let mut rng = Lcg(11);
    let mut last = Pad::NONE;
    for n in 0..12_000 {
        last = rng.pad(&last);
        play.step(&last);
        check_safe(&play.hero, &format!("random step {n}"));
    }
}

#[test]
fn review_respawn_after_a_fall_during_a_jump_buffer_does_not_jump() {
    let mut h = hero_at(0, 0, -600, MAIN);
    let back = (0, 32_767);
    // Run off the -z edge; press A in the last step before the fall line.
    let mut n = 0;
    loop {
        let mut t = h;
        t.step(&pad(back, 0), 0, &BLOCKS);
        if t.frozen > 0 {
            break;
        }
        h = t;
        n += 1;
        assert!(n < 200);
    }
    let ev = h.step(&jump_pad(back, 0), 0, &BLOCKS);
    assert!(ev.fell && !ev.jumped);
    // Presses while frozen too (one of them in the respawn step).
    for k in 0..RESPAWN_STEPS {
        let p = if k % 4 == 0 || k == RESPAWN_STEPS - 1 {
            jump_pad((0, 0), 0)
        } else {
            pad((0, 0), button::A)
        };
        let ev = h.step(&p, 0, &BLOCKS);
        assert!(!ev.jumped);
    }
    assert_eq!(h.frozen, 0);
    let safe = h.safe;
    assert_eq!(h.pos, safe);
    for _ in 0..10 {
        let ev = h.step(&pad((0, 0), button::A), 0, &BLOCKS);
        assert!(!ev.jumped, "a jump from a press before the fall");
        assert_eq!(h.pos, safe);
    }
    assert_eq!(h.jumps, 0);
}

// ---- Section 2: the camera ----------------------------------------------------

#[test]
fn review_the_eye_is_never_inside_a_box_on_a_walk_round_the_whole_island() {
    let check = |p: &Play, when: &str| {
        let eye = p.eye();
        assert_eq!(inside_any(&BLOCKS, eye), None, "{when}: eye {eye:?}");
        assert!(p.cam.dist() >= i64::from(sub(DIST_MIN)), "{when}");
    };
    // The route over every top.
    let mut play = Play::on(&ISLAND, Detail::Mid);
    follow(&mut play, &ROUTE, 400, |p| check(p, "route"));
    // Round the main island near its edges, then the same with the right
    // stick turning the camera all the way.
    for turn in [0i16, 20_000, -20_000] {
        let mut play = Play::on(&ISLAND, Detail::Mid);
        let ring: [[i16; 2]; 6] = [
            [600, -600],
            [600, 620],
            [-100, 620],
            [-100, -200],
            [-640, -200],
            [-640, -640],
        ];
        for to in ring {
            for _ in 0..200 {
                let h = &play.hero;
                let dx = f64::from(cm(to[0]) - h.pos[0]);
                let dz = f64::from(cm(to[1]) - h.pos[2]);
                if (dx * dx + dz * dz).sqrt() < f64::from(cm(30)) {
                    break;
                }
                let mut p = pad(stick_towards(dx, dz, play.cam.yaw, 0.8), 0);
                p.right = (turn, 0);
                play.step(&p);
                check(&play, &format!("ring {to:?} turn {turn}"));
                assert_eq!(play.hero.falls, 0, "fell at {to:?}");
            }
        }
    }
}

/// Steps with these held buttons, one pad a step, then `rest` empty steps.
fn shoulders(play: &mut Play, held: &[u32], rest: usize) {
    for &b in held {
        play.step(&pad((0, 0), b));
    }
    for _ in 0..rest {
        play.step(&Pad::NONE);
    }
}

#[test]
fn review_l_and_r_held_together_in_any_order_put_the_camera_behind() {
    let (l, r) = (button::L, button::R);
    for seq in [
        &[l, l | r, l][..],
        &[l, l | r, r],
        &[r, l | r, l],
        &[r, l | r, r],
        &[l | r],
        &[l, l, l | r, l | r, l],
    ] {
        let mut play = still();
        play.hero.yaw = 20_000;
        shoulders(&mut play, seq, 10);
        assert_eq!(play.cam.yaw, 20_000, "{seq:?}");
    }
}

#[test]
fn review_two_quick_l_taps_turn_90_degrees() {
    // Each release of L turns 45 degrees. The second tap comes while the
    // first turn (6 steps) is still going.
    let mut play = still();
    let y0 = play.cam.yaw;
    shoulders(&mut play, &[button::L, 0, 0, button::L], 12);
    assert_eq!(
        angle_diff(y0, play.cam.yaw),
        -2 * i32::from(SHOULDER_TURN),
        "part of the first turn was lost"
    );
}

#[test]
fn review_l_then_r_during_the_turn_comes_back() {
    let mut play = still();
    let y0 = play.cam.yaw;
    shoulders(&mut play, &[button::L, 0, 0, button::R], 12);
    assert_eq!(angle_diff(y0, play.cam.yaw), 0);
}

#[test]
fn review_an_l_tap_between_two_steps_still_turns() {
    // An update that runs no step keeps the press and the release for the
    // next step (lib.rs, step.rs): the step sees L pressed and released,
    // not held. Spec 2: L turns 45 degrees on release.
    let mut play = still();
    let y0 = play.cam.yaw;
    play.step(&Pad {
        pressed: button::L,
        released: button::L,
        ..Pad::NONE
    });
    shoulders(&mut play, &[], 10);
    assert_eq!(angle_diff(y0, play.cam.yaw), -i32::from(SHOULDER_TURN));
}

#[test]
fn review_holding_the_stick_sideways_turns_the_camera_round_and_round() {
    // Not a spec break: what the spec's auto turn did. MEASURED (PC): the
    // camera turned 2 degrees a step after AUTO_WAIT, the hero ran in a
    // circle of about 5 / 1.05 = 4.8 m radius (ESTIMATED, arithmetic).
    // FINAL (changed after the review): AUTO_SPAN is now 30 degrees, so a
    // run a quarter turn off the view no longer turns the camera; the
    // assert now pins that (it asked for more than half a turn before).
    let w = world(&BIG, [0, 0, 0]);
    let mut play = Play::on(w, Detail::Mid);
    let y0 = play.cam.yaw;
    let mut turned = 0i32;
    let mut last = y0;
    for _ in 0..150 {
        play.step(&pad((32_767, 0), 0));
        turned += angle_diff(last, play.cam.yaw);
        last = play.cam.yaw;
    }
    println!(
        "camera turned {} degrees in 5 s holding the stick right",
        turned * 360 / 65_536
    );
    assert_eq!(turned, 0, "turned {turned}");
}

#[test]
#[ignore = "a measurement for the report: prints, asserts nothing (rules review F8); run with --ignored"]
fn review_the_line_of_sight_is_counted() {
    // How often a box sits between the aim and the eye after the wall ray
    // (the eye pulled in no nearer than DIST_MIN, then lifted out of boxes).
    // Printed, MEASURED (PC); spec allows it where a wall is nearer than
    // DIST_MIN + 0.3 m.
    let mut rng = Lcg(5);
    let mut play = Play::on(&ISLAND, Detail::Mid);
    let mut last = Pad::NONE;
    let mut blocked = 0;
    let n = 9_000;
    for _ in 0..n {
        last = rng.pad(&last);
        play.step(&last);
        if play.hero.frozen == 0 && first_hit(&BLOCKS, play.aim(), play.eye()).is_some() {
            blocked += 1;
        }
    }
    println!("line of sight blocked in {blocked} of {n} steps");
}

// ---- Section 0: the fixed step ------------------------------------------------

#[test]
fn review_odd_deltas_give_at_most_two_steps_and_lose_time_only_past_the_cap() {
    let seqs: [&[u32]; 6] = [
        &[0; 50],
        &[1; 50],
        &[33_333; 50],
        &[100_000; 50],
        &[0, 1, 33_333, 100_000, 1, 0, 100_000, 33_333, 33_333, 1],
        &[33_332, 33_334, 1, 66_665, 99_999, 0, 0, 0],
    ];
    for seq in seqs {
        let mut c = Clock::new();
        // FINAL (changed after the review): the clock now starts half a
        // step full (this file's jitter test), so that start counts as fed.
        let (mut fed, mut ran, mut lost) = (u64::from(c.store), 0u64, 0u64);
        for &d in seq {
            let before = u64::from(c.store) + u64::from(d);
            let n = c.add(d);
            assert!(n <= MAX_STEPS);
            fed += u64::from(d);
            ran += u64::from(n) * u64::from(STEP_US);
            let left = before - u64::from(n) * u64::from(STEP_US);
            let cap = u64::from(STEP_US) * u64::from(MAX_STEPS);
            assert_eq!(u64::from(c.store), left.min(cap), "{seq:?}");
            lost += left.saturating_sub(cap);
            // A step is run whenever a whole step is stored.
            assert!(n == MAX_STEPS || c.store < STEP_US);
        }
        assert_eq!(fed, ran + u64::from(c.store) + lost, "{seq:?}");
        if seq.iter().all(|&d| d <= STEP_US) {
            assert_eq!(lost, 0, "{seq:?}");
        }
    }
}

#[test]
fn review_a_steady_30_a_second_with_small_jitter_runs_one_step_each_update() {
    // The console calls the game about every 33,333 us; on the board a call
    // can come a little early or late (ESTIMATED: tens of us). Spec 0 and
    // 6: S is 1 or 2, 30 pictures a second. With the store starting at 0,
    // a call 2 us early runs no step and the next one runs two.
    let mut c = Clock::new();
    let mut counts = [0u32; 3];
    for k in 0..300 {
        let d = if k % 2 == 0 { 33_331 } else { 33_335 };
        counts[usize::from(c.add(d))] += 1;
    }
    assert_eq!(counts, [0, 300, 0], "updates with 0, 1 and 2 steps");
}

// ---- Determinism and long runs -------------------------------------------------

/// A random run; gives the play at the end.
fn random_run(seed: u64, steps: u32) -> Play {
    let mut play = Play::new();
    let mut rng = Lcg(seed);
    let mut last = Pad::NONE;
    for _ in 0..steps {
        last = rng.pad(&last);
        play.step(&last);
    }
    play
}

#[test]
fn review_the_same_inputs_give_the_same_whole_state() {
    for seed in [1, 99, 2026] {
        let (a, b) = (random_run(seed, 3_000), random_run(seed, 3_000));
        assert_eq!(a.hero, b.hero);
        assert_eq!(a.cam, b.cam);
        assert_eq!(a.coins, b.coins);
        assert_eq!(a.fingerprint(), b.fingerprint());
    }
}

#[test]
fn review_the_fingerprint_sees_the_whole_rule_state() {
    // Spec 1: "a fingerprint of the state". A difference in state that
    // shows later (the safe spot, a shoulder turn under way, the squash)
    // should change it now.
    let a = random_run(4, 600);
    let mut b = a;
    b.hero.safe[0] += 1;
    assert_ne!(a.fingerprint(), b.fingerprint(), "safe spot");
    let mut b = a;
    b.cam.turn_left += 100;
    b.cam.turn_n = 3;
    assert_ne!(a.fingerprint(), b.fingerprint(), "shoulder turn");
    let mut b = a;
    b.hero.squash = 3;
    assert_ne!(a.fingerprint(), b.fingerprint(), "squash");
}

#[test]
fn review_twenty_minutes_of_random_input_with_overflow_checks() {
    // 36,000 steps (20 minutes at 30 a second), fixed seed, the detail
    // changed every 4 minutes (LOW, MID, HIGH, MAX, LOW). Overflow checks
    // are on in test builds.
    let checked = std::panic::catch_unwind(|| std::hint::black_box(i32::MAX) + 1).is_err();
    assert!(checked, "overflow checks are off: run without --release");
    let mut play = Play::on(&ISLAND, Detail::Mid);
    let mut rng = Lcg(20_261_122);
    let mut last = Pad::NONE;
    let mut far = 0i32;
    for n in 0..36_000u32 {
        if n % 7_200 == 0 {
            play.set_detail(Detail::ALL[(n / 7_200) as usize % 4]);
        }
        last = rng.pad(&last);
        play.step(&last);
        let h = &play.hero;
        if h.frozen == 0 {
            assert!(body_fits(&BLOCKS, h.pos), "step {n}: {:?}", h.pos);
            // Before the fall line, never far from the island: 13.5 m plus
            // what a fall of 20 m at full run can cover (ESTIMATED 12 m).
            far = far.max(h.pos[0].abs()).max(h.pos[2].abs());
            assert!(h.pos[0].abs() < cm(2_600) && h.pos[2].abs() < cm(2_600));
            assert!(h.pos[1] >= KILL_Y - FALL_MAX && h.pos[1] <= cm(300));
            assert_eq!(inside_any(&BLOCKS, play.eye()), None, "step {n}");
        }
        let (taken, total) = play.count();
        assert!(taken <= total);
    }
    println!(
        "farthest from the middle before the fall line: {} cm; falls {}; jumps {}",
        far * 25 / 1024,
        play.hero.falls,
        play.hero.jumps
    );
    // The detail menu's other tops: every top was not needed, but the
    // run must have moved and fallen at least once.
    assert!(play.hero.jumps > 0);
    let _ = (STEP, TERRACE);
}
