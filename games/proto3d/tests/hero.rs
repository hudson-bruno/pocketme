//! The hero's rules (PROTOTYPE-SPEC.md, section 1, tests).

mod common;

use common::*;
use game_kit::button;
use proto3d::hero::{Hero, Pose};
use proto3d::island::{
    BLOCKS, Block, FLOAT, ISLAND, ISLET, MAIN, RAMP, STEP, TERRACE, body_fits, top_under,
};
use proto3d::maths::cm;
use proto3d::play::{Detail, Play};
use proto3d::step::Pad;
use proto3d::tune::*;

/// One big flat top, top at 0, 20 m square.
static FLAT: [Block; 1] = [flat([-1000, -50, -1000], [1000, 0, 1000])];

#[test]
fn a_full_jump_peaks_at_exactly_4950_subs_and_lands_after_19_steps() {
    let mut h = hero_at(0, 0, 0, 0);
    let mut peak = 0;
    let mut steps = 0;
    h.step(&jump_pad((0, 0), 0), 0, &FLAT);
    steps += 1;
    while h.ground.is_none() {
        peak = peak.max(h.pos[1]);
        h.step(&pad((0, 0), button::A), 0, &FLAT);
        steps += 1;
        assert!(steps < 100);
    }
    assert_eq!(peak, 4_950, "peak in subs (1.21 m)");
    assert_eq!(steps, 19, "steps in the air, the landing step counted");
    assert_eq!(h.pos[1], 0);
    assert_eq!(h.pose(), Pose::Squash);
}

#[test]
fn a_tap_of_a_gives_at_most_0_4_m() {
    let mut h = hero_at(0, 0, 0, 0);
    h.step(&jump_pad((0, 0), 0), 0, &FLAT);
    let peak = run(&mut h, &pad((0, 0), 0), 40, &FLAT).max(1_006);
    // A held for the press step only: 1006, then cut to 400: +286,
    // +172, +58 (arithmetic, the spec's rule order).
    assert_eq!(peak, 1_522, "0.37 m");
    assert!(peak <= cm(40));
    assert_eq!(h.ground, Some(0));
}

/// Two tops along z with a gap of `gap` cm between z = 0 and z = gap.
fn gap_blocks(gap: i16) -> &'static [Block] {
    Box::leak(Box::new([
        flat([-500, -50, -1000], [500, 0, 0]),
        flat([-500, -50, gap], [500, 0, gap + 1000]),
    ]))
}

/// Runs from 2 m before the edge at full speed and presses A (held) in
/// the first step that starts with the middle at least `at` subs past the
/// edge (negative: before it). Gives whether it landed on the far top,
/// and the hero after.
fn try_gap(gap: i16, at: i32) -> (bool, Hero) {
    let blocks = gap_blocks(gap);
    let mut h = hero_at(0, 0, -200, 0);
    let mut pressed = false;
    for _ in 0..200 {
        let p = if !pressed && h.pos[2] >= at {
            pressed = true;
            jump_pad(FWD, 0)
        } else {
            pad(FWD, button::A)
        };
        h.step(&p, 0, blocks);
        if h.ground == Some(1) {
            return (true, h);
        }
        if h.frozen > 0 {
            return (false, h);
        }
    }
    (false, h)
}

#[test]
fn from_a_2_m_run_up_the_island_gap_is_cleared() {
    // The real island: start 2 m before the main island's edge (z = 7 m),
    // jump with the middle at the edge.
    let mut h = hero_at(0, 0, 500, MAIN);
    let mut pressed = false;
    let mut n = 0;
    while h.ground != Some(ISLET) {
        let p = if !pressed && h.pos[2] >= cm(700) {
            pressed = true;
            jump_pad(FWD, 0)
        } else {
            pad(FWD, button::A)
        };
        h.step(&p, 0, &BLOCKS);
        assert_eq!(h.frozen, 0, "fell into the gap");
        n += 1;
        assert!(n < 100);
    }
    assert!(pressed);
    assert_eq!(h.pos[1], 0);
}

#[test]
fn a_3_5_m_gap_is_cleared_with_the_press_at_the_edge() {
    // The spec expected 3.5 m not to be cleared. With its own numbers it
    // is: 19 steps of 683 subs (3.17 m) plus the body's half width at the
    // far side (0.25 m), up to one step's run past the edge before the
    // press (0.17 m) and the step-up onto the far top's edge in the
    // 20th step. See `the_widest_gap_cleared_is_measured`.
    assert!(try_gap(350, 0).0);
}

#[test]
fn a_4_5_m_gap_is_not_cleared_and_the_hero_comes_back_at_safe() {
    // 4.5 m is past the widest gap any press point clears (4.17 m).
    let (ok, mut h) = try_gap(450, 0);
    assert!(!ok);
    assert!(h.frozen > 0);
    assert_eq!(h.falls, 1);
    let safe = h.safe;
    for _ in 0..RESPAWN_STEPS {
        h.step(&Pad::NONE, 0, gap_blocks(450));
    }
    assert_eq!(h.frozen, 0);
    assert_eq!(h.pos, safe);
    assert_eq!(h.vel, [0; 3]);
    assert_eq!(h.ground, Some(0));
    // The safe spot is on the near top, at least SAFE_IN inside its edge.
    assert!(safe[2] <= -SAFE_IN, "safe {safe:?}");
}

/// The widest gap (cm) cleared with the press at `at` (subs from the
/// edge), searching every whole cm.
fn widest(at: i32) -> i16 {
    let mut best = 0;
    for gap in 200..=500 {
        if try_gap(gap, at).0 {
            best = gap;
        }
    }
    best
}

#[test]
fn the_widest_gap_cleared_is_measured() {
    // Press with the middle at the edge (the spec's 3.17 m of airtime
    // plus the body's half width at the far side): MEASURED, PC.
    let at_edge = widest(0);
    // The latest press that still jumps: coyote steps after the square
    // left the top. Search the press point over every 4 cm.
    let mut best = (0, 0);
    for at in (-cm(100)..=cm(150)).step_by(164) {
        let w = widest(at);
        if w > best.0 {
            best = (w, at);
        }
    }
    println!("widest gap cleared, press with the middle at the edge: {at_edge} cm");
    println!(
        "widest gap cleared, best press point ({} cm past the edge): {} cm",
        best.1 * 25 / 1024,
        best.0
    );
    assert_eq!(at_edge, GAP_AT_EDGE);
    assert_eq!(best.0, GAP_BEST);
}

/// MEASURED (PC, `the_widest_gap_cleared_is_measured`).
const GAP_AT_EDGE: i16 = 366;
const GAP_BEST: i16 = 417;

#[test]
fn the_step_cannot_be_walked_onto_but_can_be_jumped_onto() {
    // Walk at the step (x 1 to 3 m, z -1 to 1 m) from x = -1 m.
    let mut h = hero_at(-100, 0, 0, MAIN);
    let right = (32_767, 0);
    for _ in 0..60 {
        h.step(&pad(right, 0), 0, &BLOCKS);
        assert_ne!(h.ground, Some(STEP));
        assert_eq!(h.pos[1], 0);
    }
    assert!(h.pos[0] <= cm(100) - HALF, "pushed back at the side");
    // Now jump.
    h.step(&jump_pad(right, 0), 0, &BLOCKS);
    for _ in 0..30 {
        if h.ground == Some(STEP) {
            break;
        }
        h.step(&pad(right, button::A), 0, &BLOCKS);
    }
    assert_eq!(h.ground, Some(STEP));
    assert_eq!(h.pos[1], cm(50));
}

#[test]
fn the_ramp_can_be_walked_up_to_the_terrace() {
    let mut h = hero_at(-350, 0, -250, MAIN);
    let mut on_ramp = false;
    for _ in 0..120 {
        h.step(&pad(FWD, 0), 0, &BLOCKS);
        assert_eq!(h.jumps, 0);
        on_ramp |= h.ground == Some(RAMP);
        if h.ground == Some(TERRACE) {
            break;
        }
    }
    assert!(on_ramp);
    assert_eq!(h.ground, Some(TERRACE));
    assert_eq!(h.pos[1], cm(100));
}

#[test]
fn the_ramp_can_be_walked_down_without_leaving_it() {
    let mut h = hero_at(-350, 100, 300, TERRACE);
    let back = (0, 32_767);
    let mut air = 0;
    for _ in 0..80 {
        h.step(&pad(back, 0), 0, &BLOCKS);
        if h.ground.is_none() {
            air += 1;
        }
        if h.pos[2] < cm(-200) {
            break;
        }
    }
    assert_eq!(h.ground, Some(MAIN));
    assert_eq!(air, 0, "the snap keeps the feet on the slope");
}

#[test]
fn the_floating_block_is_reached_from_the_step() {
    // On the step's middle, run along +z and jump at z = 0.75 m.
    let mut h = hero_at(280, 50, -50, STEP);
    let mut pressed = false;
    for _ in 0..80 {
        let p = if !pressed && h.pos[2] >= cm(75) {
            pressed = true;
            jump_pad(FWD, 0)
        } else {
            pad(FWD, button::A)
        };
        // Stop running once over the block.
        let p = if h.pos[2] > cm(380) {
            pad((0, 0), button::A)
        } else {
            p
        };
        h.step(&p, 0, &BLOCKS);
        if h.ground == Some(FLOAT) {
            break;
        }
    }
    assert_eq!(h.ground, Some(FLOAT));
    assert_eq!(h.pos[1], cm(150));
}

#[test]
fn the_floating_block_cannot_be_reached_from_the_ground() {
    // A full jump peaks at 1.21 m, the block's top is 1.5 m.
    for x in [210, 280, 350] {
        let mut h = hero_at(x, 0, 150, MAIN);
        h.step(&jump_pad(FWD, 0), 0, &BLOCKS);
        for _ in 0..60 {
            h.step(&pad(FWD, button::A), 0, &BLOCKS);
            assert_ne!(h.ground, Some(FLOAT));
        }
    }
}

#[test]
fn nobody_gets_under_the_floating_block() {
    // From every side, walking at it on the ground pushes the hero back:
    // its underside (1.1 m) is lower than the head (1.26 m).
    let b = BLOCKS[usize::from(FLOAT)].bounds();
    for (from, s) in [
        ([280, 0, 150], FWD),
        ([280, 0, 600], (0, 32_767)),
        ([100, 0, 380], (32_767, 0)),
        ([500, 0, 380], (-32_767, 0)),
    ] {
        let mut h = hero_at(from[0], from[1], from[2], MAIN);
        for _ in 0..60 {
            h.step(&pad(s, 0), 0, &BLOCKS);
            assert!(
                !b.overlaps_square(h.pos[0], h.pos[2], HALF) || h.pos[1] >= b.hi[1],
                "under the block at {:?}",
                h.pos
            );
        }
    }
}

#[test]
fn a_jump_under_a_low_ceiling_bumps_the_head() {
    // The island's block cannot be stood under (above), so a test world:
    // a ceiling 1.6 m up over the ground.
    static ROOM: [Block; 2] = [
        flat([-500, -50, -500], [500, 0, 500]),
        flat([-100, 160, -100], [100, 200, 100]),
    ];
    let mut h = hero_at(0, 0, 0, 0);
    let ev = h.step(&jump_pad((0, 0), 0), 0, &ROOM);
    assert!(ev.jumped);
    let mut bumped = 0;
    for _ in 0..30 {
        let ev = h.step(&pad((0, 0), button::A), 0, &ROOM);
        if ev.bumped {
            bumped += 1;
            assert_eq!(h.head(), cm(160));
            assert_eq!(h.vel[1], 0);
        }
        assert!(h.head() <= cm(160));
    }
    assert_eq!(bumped, 1);
    assert_eq!(h.ground, Some(0));
}

/// Walks off the +z edge of a lone top and presses A `k` steps after the
/// last step on the top. Gives the jumps made.
fn coyote_after(k: u8) -> u16 {
    static LEDGE: [Block; 1] = [flat([-500, -50, -500], [500, 0, 0])];
    let mut h = hero_at(0, 0, -100, 0);
    let slow = (0, -12_000);
    while h.ground.is_some() {
        h.step(&pad(slow, 0), 0, &LEDGE);
    }
    // Now air_steps is 1: the first step after the last one on the top.
    for _ in 1..k {
        h.step(&pad(slow, 0), 0, &LEDGE);
    }
    h.step(&jump_pad(slow, 0), 0, &LEDGE);
    // A second press in the air never jumps again.
    for _ in 0..3 {
        h.step(&pad(slow, button::A), 0, &LEDGE);
    }
    h.step(&jump_pad(slow, 0), 0, &LEDGE);
    h.jumps
}

#[test]
fn coyote_steps_work_once_and_only_once() {
    for k in 1..=COYOTE {
        assert_eq!(coyote_after(k), 1, "press {k} steps after leaving");
    }
    assert_eq!(coyote_after(COYOTE + 1), 0);
}

/// Drops the hero from 2 m and presses A `k` steps before the landing
/// step (0: in it). Gives the jumps made and the step after the landing
/// that it came in (0: none).
fn buffer_before(k: u32) -> (u16, u32) {
    // When does it land?
    let fall = |press: Option<u32>| {
        let mut h = hero_at(0, 200, 0, 0);
        h.ground = None;
        h.air_steps = COYOTE + 1;
        let mut landed_at = None;
        let mut jumped_at = 0;
        for n in 0..80u32 {
            let p = if Some(n) == press {
                jump_pad((0, 0), 0)
            } else {
                pad((0, 0), button::A)
            };
            let ev = h.step(&p, 0, &FLAT);
            if ev.landed.is_some() && landed_at.is_none() {
                landed_at = Some(n);
            }
            if ev.jumped && jumped_at == 0 {
                jumped_at = n - landed_at.unwrap_or(n);
            }
        }
        (landed_at.unwrap(), h.jumps, jumped_at)
    };
    let (land, _, _) = fall(None);
    let (_, jumps, after) = fall(Some(land - k));
    (jumps, after)
}

#[test]
fn the_jump_buffer_works_once_and_only_once() {
    // A press is kept in the step it comes in and the BUFFER after (as
    // coyote gives BUFFER steps after leaving a top); the landing step's
    // jump test runs before the landing, so a press up to BUFFER - 1
    // steps before the landing step jumps in the step after it.
    for k in 0..=u32::from(BUFFER) - 1 {
        assert_eq!(buffer_before(k), (1, 1), "press {k} steps before landing");
    }
    assert_eq!(buffer_before(u32::from(BUFFER)).0, 0);
    assert_eq!(buffer_before(u32::from(BUFFER) + 3).0, 0);
}

#[test]
fn no_shadow_and_a_ring_over_the_gap_a_shadow_and_no_ring_on_every_top() {
    // On every top, standing in its middle.
    for (i, b) in BLOCKS.iter().enumerate() {
        let bb = b.bounds();
        let (x, z) = ((bb.lo[0] + bb.hi[0]) / 2, (bb.lo[2] + bb.hi[2]) / 2);
        let (top, y) = top_under(&BLOCKS, x, z, i32::MAX).unwrap();
        if usize::from(top) != i {
            continue; // the main island's middle is not under the terrace
        }
        let h = Hero::new([x, y, z], top);
        let s = h.shadow(&BLOCKS).expect("a shadow on every top");
        assert_eq!(s.top, top);
        assert_eq!(s.r, SH_MAX);
        assert_eq!(h.ring(&Pad::NONE, 0, &BLOCKS), None);
    }
    // In the jump across the gap: while the middle is over the gap, no
    // shadow and a ring.
    let mut h = hero_at(0, 0, 500, MAIN);
    let mut pressed = false;
    let mut over_gap = 0;
    for _ in 0..60 {
        let p = if !pressed && h.pos[2] >= cm(700) {
            pressed = true;
            jump_pad(FWD, 0)
        } else {
            pad(FWD, button::A)
        };
        h.step(&p, 0, &BLOCKS);
        // In the air with the middle over the gap (the landing step can
        // end with the middle still over it, the square on the islet).
        if h.ground.is_none() && h.pos[2] > cm(700) && h.pos[2] < cm(950) {
            over_gap += 1;
            assert_eq!(h.shadow(&BLOCKS), None);
            let r = h.ring(&pad(FWD, button::A), 0, &BLOCKS).expect("a ring");
            assert_eq!(r.top, ISLET);
        }
        if h.ground == Some(ISLET) {
            break;
        }
    }
    assert!(over_gap > 10);
    assert_eq!(h.ground, Some(ISLET));
}

#[test]
fn the_shadow_shrinks_with_height_down_to_its_smallest() {
    let mut h = hero_at(0, 0, 0, 0);
    h.pos[1] = cm(100);
    h.ground = None;
    let s = h.shadow(&FLAT).unwrap();
    assert_eq!(s.r, SH_MAX - cm(100) * 14 / 150);
    h.pos[1] = cm(1000);
    assert_eq!(h.shadow(&FLAT).unwrap().r, SH_MIN);
}

#[test]
fn the_ring_matches_the_real_landing_on_ten_jumps() {
    // Ten jumps across the gap with the full rules (camera included):
    // different sides, angles, speeds and A held or let go.
    let cases: [(i16, f64, f64, u32); 10] = [
        (0, 0.0, 1.0, 30),
        (-120, 0.0, 1.0, 30),
        (120, 0.0, 1.0, 30),
        (-150, 0.25, 1.0, 30),
        (150, -0.25, 1.0, 30),
        (50, 0.1, 0.95, 30),
        (-50, -0.1, 0.9, 30),
        (0, 0.0, 0.85, 30),
        (100, -0.15, 1.0, 7),
        (-100, 0.15, 1.0, 6),
    ];
    for (n, &(x, side, speed, hold)) in cases.iter().enumerate() {
        let mut play = Play::on(&ISLAND, Detail::Mid);
        play.hero = hero_at(x, 0, 400, MAIN);
        play.cam.snap(&play.hero, &BLOCKS, ISLAND.props, 1);
        let mut since: Option<u32> = None;
        let mut first_ring = None;
        let mut landed = None;
        for _ in 0..120 {
            let s = stick_towards(side, 1.0, play.cam.yaw, speed);
            let p = match since {
                None if play.hero.pos[2] >= cm(690) => {
                    since = Some(0);
                    jump_pad(s, 0)
                }
                Some(k) if k < hold => pad(s, button::A),
                _ => pad(s, 0),
            };
            if let Some(k) = since.as_mut() {
                *k += 1;
            }
            play.step(&p);
            if first_ring.is_none() {
                first_ring = play.ring;
            }
            if since.is_some()
                && let Some(g) = play.hero.ground
            {
                landed = Some((g, play.hero.pos));
                break;
            }
            assert_eq!(play.hero.frozen, 0, "jump {n} fell");
        }
        let r = first_ring.unwrap_or_else(|| panic!("jump {n}: no ring"));
        let (top, at) = landed.unwrap_or_else(|| panic!("jump {n}: no landing"));
        assert_eq!(r.top, top, "jump {n}");
        let d = proto3d::maths::len2(r.spot[0] - at[0], r.spot[2] - at[2]);
        assert!(d <= cm(25), "jump {n}: ring {:?} landing {at:?}", r.spot);
    }
}

#[test]
fn the_same_inputs_give_the_same_state_after_600_steps() {
    let go = |seed| {
        let mut play = Play::new();
        let mut rng = Lcg(seed);
        let mut last = Pad::NONE;
        for _ in 0..600 {
            last = rng.pad(&last);
            play.step(&last);
        }
        play.fingerprint()
    };
    assert_eq!(go(7), go(7));
    assert_ne!(go(7), go(8));
}

#[test]
fn ten_minutes_of_scripted_play_do_not_panic() {
    // 18,000 steps (10 minutes at 30 a second), overflow checks on (test
    // builds keep them), every detail setting.
    let mut rng = Lcg(42);
    for d in [Detail::Low, Detail::Mid, Detail::High] {
        let mut play = Play::on(&ISLAND, d);
        let mut last = Pad::NONE;
        for _ in 0..6_000 {
            last = rng.pad(&last);
            play.step(&last);
            assert!(body_fits(&BLOCKS, play.hero.pos) || play.hero.frozen > 0);
        }
    }
}

/// Rules review F6, H2: a jump towards a ledge just over the head (top
/// 1.30 m, the hero 1.26 m tall, the jump's peak 1.21 m) lands on it, and
/// the body is never inside the ledge on the way: under the top by more
/// than a step-up it is pushed out of the face, within a step-up while
/// rising it is lifted onto the top.
#[test]
fn a_jump_towards_a_ledge_just_over_the_head_lands_on_it() {
    static LEDGE: [Block; 2] = [
        flat([-1000, -50, -1000], [1000, 0, 1000]),
        flat([-300, 0, 100], [300, 130, 400]),
    ];
    let ledge = LEDGE[1].bounds();
    // From 15 cm before the face: the body meets it while still low.
    let mut h = hero_at(0, 0, 60, 0);
    h.step(&jump_pad(FWD, 0), 0, &LEDGE);
    let mut steps = 1;
    while h.ground.is_none() {
        let inside = ledge.overlaps_square(h.pos[0], h.pos[2], HALF) && h.pos[1] < ledge.hi[1];
        assert!(!inside, "step {steps}: body inside the ledge: {h:?}");
        h.step(&pad(FWD, button::A), 0, &LEDGE);
        steps += 1;
        assert!(steps < 60, "never landed: {h:?}");
    }
    assert_eq!(h.ground, Some(1), "landed on {:?}", h.ground);
    assert_eq!(h.pos[1], cm(130));
}
