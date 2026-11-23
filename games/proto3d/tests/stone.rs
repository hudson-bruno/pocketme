//! STONE (AREAS-SPEC.md, sections 5 and 6): the stone's rules (ground
//! place, air place, raise, call back, the return after a fall), why
//! each stage needs the stone, the HUD, the route from the area menu to
//! "done", and pictures.
//!
//! Hand sums are written beside the expected values. Units: subs
//! (1/4096 m; cm(c) = c * 1024 / 25, cut toward 0), steps (1/30 s), cm
//! in the records.

mod common;

use std::path::Path;

use common::{FWD, hero_at, pad, stick_towards};
use game_3d::Stats;
use game_kit::{Frame, H, W, button};
use proto3d::area::stone::{
    self as st, AHEAD, FALL_ROOM, G, HINT_STEPS, Held, K, NO_ROOM_STEPS, RAISE_MAX, RETURN, S,
    STONE, Stone, T, air_spot, ground_spot, stone_record,
};
use proto3d::area::{Area, Extra, Hud, Kit};
use proto3d::draw::{PALETTE, Painter, col};
use proto3d::hero::{Body, Hero, HeroEvents};
use proto3d::island::{Block, flag, ramp};
use proto3d::maths::cm;
use proto3d::models::{self, mat};
use proto3d::pause::Menu;
use proto3d::play::{Detail, Live, Play};
use proto3d::step::Pad;
use proto3d::tune::{G as GRAVITY, HERO_H, JUMP, M, RESPAWN_STEPS, RUN, STEP_UP};
use proto3d::{Input, Note, Proto};

/// The triangle budgets, LOW, MID, HIGH and MAX (MAX: the most
/// triangles of the swept ISLAND views at MAX plus 5 %, as in
/// tests/max_level.rs).
const BUDGET: [u32; 4] = [160, 430, 940, 1324];

fn blocks() -> Vec<Block> {
    st::WORLD.blocks.to_vec()
}

fn kit(p: &Play) -> &Stone {
    match &p.kit {
        Kit::Stone(s) => s,
        k => panic!("not STONE: {k:?}"),
    }
}

fn kit_mut(p: &mut Play) -> &mut Stone {
    match &mut p.kit {
        Kit::Stone(s) => s,
        k => panic!("not STONE: {k:?}"),
    }
}

/// A pad with `pressed` pressed (and held) and `held` held.
fn press_pad(s: (i16, i16), pressed: u32, held: u32) -> Pad {
    Pad {
        held: held | pressed,
        pressed,
        released: 0,
        left: s,
        right: (0, 0),
    }
}

/// STONE at MID with the hero put at `at` (cm) on `top` (None: in the
/// air, 10 steps after take-off), facing `yaw`.
fn play_at(at: [i16; 3], top: Option<u8>, yaw: u16) -> Play {
    let mut p = Play::start(Area::Stone, Detail::Mid);
    let mut h = hero_at(at[0], at[1], at[2], top.unwrap_or(S));
    h.yaw = yaw;
    if top.is_none() {
        h.ground = None;
        h.air_steps = 10;
    }
    p.hero = h;
    kit_mut(&mut p).safe = h.safe;
    kit_mut(&mut p).safe_top = S;
    p
}

/// Writes the kit's stone record into the live blocks (as a step does).
fn sync(p: &mut Play) {
    let n = usize::from(p.live.n);
    let k = p.kit;
    k.movers(&mut p.live.blocks[n - 1..n]);
}

/// The stone's record as it is now.
fn record(p: &Play) -> Block {
    p.blocks()[usize::from(STONE)]
}

/// The HUD as it is now.
fn hud(p: &Play) -> Hud {
    let mut h = Hud::default();
    p.kit.hud(&mut h);
    h
}

// ---- The records --------------------------------------------------------------------------

#[test]
fn the_records_body_and_state_are_the_specs() {
    let b = blocks();
    let wr = flag::WALK | flag::RIM;
    let boxes: Vec<_> = b.iter().map(|b| (b.lo, b.hi, b.flags)).collect();
    assert_eq!(
        boxes[..4],
        [
            // S runs on under K and K starts on S's top (draw review F1:
            // K's buried foot was painted over S's grass).
            ([-300, -60, -400], [300, 0, 400], wr),
            ([-300, 0, 200], [300, 300, 400], wr),
            ([-250, 240, 900], [250, 300, 1300], wr),
            ([-125, 640, 1600], [125, 700, 1850], wr | flag::FADE),
        ]
    );
    assert_eq!(b[usize::from(K)].side, mat::ROCK);
    assert_eq!((S, K, T, G, STONE), (0, 1, 2, 3, 4));
    assert_eq!(st::MOVERS, 1);
    // The stone starts in hand: switched off, FADE, slate with brass
    // sides, 100 x 25 x 100.
    let s = b[usize::from(STONE)];
    assert!(!s.solid());
    assert_eq!(s.flags, wr | flag::FADE | flag::HIDE);
    assert_eq!((s.top, s.side), (mat::SLATE, mat::BRASS));
    assert_eq!([0, 1, 2].map(|k| s.hi[k] - s.lo[k]), [100, 25, 100]);
    // The gaps: K's far edge (z 4.0) to T (z 9.0) is 5.0 m; T's far edge
    // (z 13.0) to G (z 16.0) 3.0 m; G's top 4.0 m over T's.
    assert_eq!(b[2].lo[2] - b[1].hi[2], 500);
    assert_eq!(b[3].lo[2] - b[2].hi[2], 300);
    assert_eq!(b[3].hi[1] - b[2].hi[1], 400);
    // The start: (0, 0, -3 m) on S, facing +z.
    let p = Play::start(Area::Stone, Detail::Mid);
    assert_eq!(p.hero.pos, [0, 0, -12_288]); // -300 * 1024 / 25
    assert_eq!(p.hero.ground, Some(S));
    assert_eq!(p.hero.yaw, 0);
    // The body: kill_y = safe.y - 6 m = 0 - 6 * 4096 = -24576; 15 steps.
    assert_eq!(FALL_ROOM, 24_576);
    assert_eq!(
        Stone::new().body(&p.hero),
        Body {
            run: RUN,
            jump: JUMP,
            glide: false,
            kill_y: -24_576,
            respawn: 15,
        }
    );
    assert_eq!(RESPAWN_STEPS, 15);
    assert!(std::mem::size_of::<Stone>() <= 32);
    let k = kit(&p);
    assert_eq!(k.held, Held::InHand);
    assert!(k.air_ok);
    assert_eq!(p.kit.time(), Some(0));
}

// ---- Ground place -------------------------------------------------------------------------

#[test]
fn a_ground_place_puts_the_stone_one_metre_ahead() {
    let mut p = Play::start(Area::Stone, Detail::Mid);
    p.step(&press_pad((0, 0), button::B, 0));
    // Middle = (0, -3 m) + forward(0) * 4096 / 16384 * ... = z -12288 +
    // 4096 = -8192 subs = -200 cm; the top of S (0) + 25 cm.
    assert_eq!(AHEAD, M);
    assert_eq!(kit(&p).held, Held::Placed { base: 25 });
    assert_eq!(kit(&p).at, [0, 25, -200]);
    let r = record(&p);
    assert_eq!((r.lo, r.hi), ([-50, 0, -250], [50, 25, -150]));
    assert!(r.solid());
    assert_eq!(r.flags, flag::WALK | flag::RIM | flag::FADE);
    // The hero was not moved, and stands on S still.
    assert_eq!(p.hero.ground, Some(S));
    // Facing +x (yaw 16384): (0 + 100 cm, -300).
    let mut p = play_at([0, 0, -300], Some(S), 16_384);
    p.step(&press_pad((0, 0), button::B, 0));
    assert_eq!(kit(&p).at, [100, 25, -300]);
    // A second B while placed (off the stone) does nothing.
    p.step(&Pad::NONE);
    p.step(&press_pad((0, 0), button::B, 0));
    assert_eq!(kit(&p).at, [100, 25, -300]);
    assert_eq!(kit(&p).no_room, 0);
}

/// Rules review F6, T6: a ground place across two flat tops more than
/// 2 cm apart is NO ROOM; 2 cm apart it fits.
#[test]
fn a_ground_place_across_tops_more_than_2_cm_apart_has_no_room() {
    let flat = |lo: [i16; 3], hi: [i16; 3]| Block {
        lo,
        hi,
        top: mat::GRASS,
        side: mat::DIRT,
        flags: flag::WALK,
        ramp: ramp::FLAT,
    };
    // Block 1 is K's number (no ground place on it): a block far away.
    let two = |dy: i16| {
        vec![
            flat([-300, -60, 0], [300, 0, 300]),
            flat([2_000, -60, 2_000], [2_100, 0, 2_100]),
            flat([-300, -60, -300], [300, -dy, 0]),
        ]
    };
    // Facing +z from z -1 m: the stone's middle on the seam (z 0, top 0),
    // its corners at z -0.5 m (top -dy) and +0.5 m (top 0).
    let h = |dy: i16| hero_at(0, -dy, -100, 2);
    assert_eq!(ground_spot(&two(1), &h(1)), Some([0, 25, 0]));
    // 2 cm down is -82 subs (cm() rounds down) against FLAT's 81: in
    // practice the tops must be under 2 cm apart.
    for dy in [2, 3, 5, 10, 20] {
        assert_eq!(ground_spot(&two(dy), &h(dy)), None, "tops {dy} cm apart");
    }
}

#[test]
fn no_room_on_an_edge_into_the_cliff_on_a_ramp_or_over_the_hero() {
    let b = blocks();
    // Near S's right edge (x 2.5 m) facing +x: the middle at x 3.5 m is
    // past S's edge (3.0 m).
    let h = {
        let mut h = hero_at(250, 0, -300, S);
        h.yaw = 16_384;
        h
    };
    assert_eq!(ground_spot(&b, &h), None);
    // 1.5 m before the cliff (z 0.5 m): the stone fits (z 1.0..2.0 m,
    // touching the face); 0.5 m nearer it would cut into K.
    assert_eq!(ground_spot(&b, &hero_at(0, 0, 50, S)), Some([0, 25, 150]));
    assert_eq!(ground_spot(&b, &hero_at(0, 0, 100, S)), None);
    // On a ramp (1 m up over 4 m along z): the corners 1 m apart along
    // the slope are 25 cm apart in height, over 2 cm.
    let r = vec![Block {
        lo: [-300, 0, 0],
        hi: [300, 100, 400],
        top: mat::GRASS,
        side: mat::DIRT,
        flags: flag::WALK,
        ramp: ramp::UP_Z,
    }];
    assert_eq!(ground_spot(&r, &hero_at(0, 25, 100, 0)), None);
    // Along the ramp's side (facing +x) the corners differ too.
    let mut h = hero_at(-200, 50, 200, 0);
    h.yaw = 16_384;
    assert_eq!(ground_spot(&r, &h), None);
    // At 45 degrees the square stone 1 m ahead overlaps the hero's own
    // body: 100 cm * 0.707 = 71 cm, the stone from 21 cm, the body to
    // 25 cm on both x and z. 5.5 degrees off (yaw 7192) it fits.
    let mut h = hero_at(0, 0, -300, S);
    h.yaw = 8_192;
    assert_eq!(ground_spot(&b, &h), None);
    h.yaw = 7_192;
    assert!(ground_spot(&b, &h).is_some());
    // In play: NO ROOM for 20 steps, the stone stays in hand.
    let mut p = play_at([250, 0, -300], Some(S), 16_384);
    p.step(&press_pad((0, 0), button::B, 0));
    assert_eq!(kit(&p).held, Held::InHand);
    let mut shown = 0;
    for _ in 0..40 {
        if hud(&p).msg == Some("NO ROOM") {
            shown += 1;
        }
        p.step(&Pad::NONE);
    }
    assert_eq!(NO_ROOM_STEPS, 20);
    assert_eq!(shown, 20);
}

// ---- Air place ----------------------------------------------------------------------------

#[test]
fn an_air_place_puts_the_top_at_the_feet_and_the_hero_lands_on_it() {
    // Still in the air at 4950 subs (a jump's top, 1.21 m).
    let mut p = play_at([0, 0, -300], None, 0);
    p.hero.pos[1] = 4_950;
    p.step(&press_pad((0, 0), button::B, 0));
    // The move first: 4950 - 114 = 4836. Its cm, rounded down:
    // 4836 * 25 / 1024 = 118.07 -> 118; cm(118) = 120832 / 25 = 4833
    // <= 4836.
    assert_eq!(p.hero.pos[1], 4_836);
    assert_eq!(kit(&p).at, [0, 118, -300]);
    assert_eq!(kit(&p).held, Held::Placed { base: 118 });
    assert!(!kit(&p).air_ok);
    let r = record(&p);
    assert_eq!((r.lo, r.hi), ([-50, 93, -350], [50, 118, -250]));
    // Next step: 4836 - 228 = 4608 passes 4833: he lands on it.
    p.step(&Pad::NONE);
    assert_eq!(p.events.hero.landed, Some(STONE));
    assert_eq!(p.hero.pos[1], 4_833);
    // Under 0: the feet at -100 subs: -100 * 25 / 1024 = -2.44, down to
    // -3 cm; cm(-3) = -3072 / 25 = -122 <= -100.
    let mut h = hero_at(1_000, 0, 0, S);
    h.ground = None;
    h.pos[1] = -100;
    assert_eq!(air_spot(&[], &h), Some([1_000, -3, 0]));
    // Not into a block: just over S (feet 10 cm up), the stone would
    // reach 15 cm into it.
    let b = blocks();
    let mut h = hero_at(0, 10, -300, S);
    h.ground = None;
    assert_eq!(air_spot(&b, &h), None);
    // 25 cm up it just fits (its underside on S's top).
    h.pos[1] = cm(25);
    assert_eq!(air_spot(&b, &h), Some([0, 25, -300]));
}

#[test]
fn one_air_place_per_take_off_from_real_ground() {
    let mut p = Play::start(Area::Stone, Detail::Mid);
    p.step(&Pad::NONE);
    // Jump; at the top, B: placed in the air.
    p.step(&press_pad((0, 0), button::A, 0));
    while p.hero.vel[1] > 0 {
        p.step(&pad((0, 0), button::A));
    }
    p.step(&press_pad((0, 0), button::B, 0));
    assert!(matches!(kit(&p).held, Held::Placed { .. }));
    assert!(!kit(&p).air_ok);
    p.step(&Pad::NONE);
    assert_eq!(p.hero.ground, Some(STONE));
    // Standing on the stone does not give the air place back.
    for _ in 0..5 {
        p.step(&Pad::NONE);
    }
    assert!(!kit(&p).air_ok);
    // Jump off it and call it back at once: in hand 15 steps later,
    // still in the air (a jump from 1.2 m up lasts over 20 steps).
    p.step(&press_pad((0, 0), button::A, 0));
    p.step(&press_pad((0, 0), button::X, button::A));
    for _ in 0..RETURN {
        p.step(&pad((0, 0), button::A));
    }
    assert_eq!(kit(&p).held, Held::InHand);
    assert!(p.hero.ground.is_none());
    // No outline under the feet, and B does nothing (no NO ROOM either).
    let mut ex = [Extra::NONE; 6];
    assert_eq!(p.kit.extras(&p, &mut ex), 1); // the beacon only
    p.step(&press_pad((0, 0), button::B, 0));
    assert_eq!(kit(&p).held, Held::InHand);
    assert_eq!(kit(&p).no_room, 0);
    // Down on S: allowed again.
    while p.hero.ground.is_none() {
        p.step(&Pad::NONE);
    }
    assert_eq!(p.hero.ground, Some(S));
    assert!(kit(&p).air_ok);
}

#[test]
fn the_outline_shows_under_the_feet_while_an_air_place_works() {
    let mut p = play_at([0, 0, -300], None, 0);
    p.hero.pos[1] = cm(100);
    let mut ex = [Extra::NONE; 6];
    assert_eq!(p.kit.extras(&p, &mut ex), 2);
    assert!(std::ptr::eq(ex[1].model, &models::STONE_OUTLINE));
    // 25 cm under the feet: 4096 - 1024 = 3072.
    assert_eq!(ex[1].pos, [0, 3_072, -12_288]);
    // Standing: none.
    let p = Play::start(Area::Stone, Detail::Mid);
    assert_eq!(p.kit.extras(&p, &mut ex), 1);
    // Too low (the stone would cut into S): none.
    let mut p = play_at([0, 0, -300], None, 0);
    p.hero.pos[1] = cm(10);
    assert_eq!(p.kit.extras(&p, &mut ex), 1);
}

// ---- Raise ---------------------------------------------------------------------------------

/// STONE with the stone placed on S 1 m ahead of the start and the hero
/// walked onto it and standing still.
fn on_placed_stone() -> Play {
    let mut p = Play::start(Area::Stone, Detail::Mid);
    p.step(&press_pad((0, 0), button::B, 0));
    assert_eq!(kit(&p).at, [0, 25, -200]);
    let slow = stick_towards(0.0, 1.0, p.cam.yaw, 0.3);
    for _ in 0..30 {
        if p.hero.ground == Some(STONE) {
            break;
        }
        p.step(&pad(slow, 0));
    }
    for _ in 0..4 {
        p.step(&Pad::NONE);
    }
    assert_eq!(p.hero.ground, Some(STONE));
    assert_eq!(p.hero.vel, [0; 3]);
    assert_eq!(p.hero.pos[1], 1_024); // cm(25)
    p
}

#[test]
fn the_raise_goes_4_cm_a_step_to_250_cm_and_never_sinks() {
    let mut p = on_placed_stone();
    // The walk onto it did not arm it; holding B now does nothing.
    for _ in 0..5 {
        p.step(&pad((0, 0), button::B));
    }
    assert_eq!(kit(&p).at[1], 25);
    p.step(&Pad::NONE);
    // A fresh press arms it and raises in the same step: 25 + 4 = 29.
    p.step(&press_pad((0, 0), button::B, 0));
    assert_eq!(kit(&p).at[1], 29);
    assert_eq!(p.hero.pos[1], cm(29));
    let mut steps = 1;
    while kit(&p).at[1] < 275 {
        p.step(&pad((0, 0), button::B));
        steps += 1;
        assert!(steps <= 63);
        assert_eq!(p.hero.ground, Some(STONE));
        assert_eq!(p.hero.pos[1], cm(kit(&p).at[1]));
    }
    // 25 + 4 * 62 = 273, then 275 (the cap 25 + 250): 63 steps, 2.1 s.
    assert_eq!(steps, 63);
    assert_eq!(RAISE_MAX, 250);
    assert_eq!(p.hero.pos[1], 11_264); // cm(275)
    p.step(&pad((0, 0), button::B));
    assert_eq!(kit(&p).at[1], 275);
    // Let go: it stays; it never sinks.
    for _ in 0..30 {
        p.step(&Pad::NONE);
    }
    assert_eq!(kit(&p).at[1], 275);
    // The safe spot never moved onto the stone (on it for 100 steps).
    assert_eq!(p.hero.safe_top, S);
    assert_eq!(p.hero.safe, [0, 0, -12_288]);
}

#[test]
fn the_raise_stops_when_the_body_would_hit_a_block_above() {
    // A ceiling 2.0..2.6 m up over the stone on S.
    let hide = Block::flat([0; 3], [0; 3], 0, 0, flag::HIDE);
    let mut b = vec![
        blocks()[0],
        Block::flat(
            [-300, 200, -400],
            [300, 260, 200],
            mat::STONE,
            mat::ROCK,
            flag::WALK,
        ),
        hide,
        hide,
        stone_record([0, 25, -200], true),
    ];
    let mut s = Stone {
        held: Held::Placed { base: 25 },
        at: [0, 25, -200],
        ..Stone::new()
    };
    let mut h = hero_at(0, 25, -200, STONE);
    let ev = HeroEvents::default();
    for k in 0..80 {
        let p = if k == 0 {
            press_pad((0, 0), button::B, 0)
        } else {
            pad((0, 0), button::B)
        };
        s.before(&p, &h, &b);
        s.after(&mut h, &ev, &b);
        s.movers(&mut b[4..5]);
        h.pos[1] = cm(s.at[1]);
    }
    // The head at feet + 5161 must stay at or under 2.0 m (8192 subs):
    // the top 25 + 4 k: k = 12 gives 73 (cm(73) = 2990, + 5161 = 8151);
    // 77 would give 3153 + 5161 = 8314 > 8192.
    assert_eq!(HERO_H, 5_161);
    assert_eq!(s.at[1], 73);
}

#[test]
fn the_placing_press_never_raises() {
    // An air place with B held on through the landing: no raise.
    let mut p = play_at([0, 0, -300], None, 0);
    p.hero.pos[1] = cm(150);
    p.step(&press_pad((0, 0), button::B, 0));
    let top = kit(&p).at[1];
    for _ in 0..20 {
        p.step(&pad((0, 0), button::B));
    }
    assert_eq!(p.hero.ground, Some(STONE));
    assert_eq!(kit(&p).at[1], top);
    // Let go and press again: it rises.
    p.step(&Pad::NONE);
    p.step(&press_pad((0, 0), button::B, 0));
    assert_eq!(kit(&p).at[1], top + 4);
    // A ground place with B held on, walking onto it with B held: no
    // raise either (see also the raise test).
    let mut p = Play::start(Area::Stone, Detail::Mid);
    p.step(&press_pad((0, 0), button::B, 0));
    let slow = stick_towards(0.0, 1.0, p.cam.yaw, 0.3);
    for _ in 0..30 {
        p.step(&pad(slow, button::B));
    }
    assert_eq!(p.hero.ground, Some(STONE));
    assert_eq!(kit(&p).at[1], 25);
}

// ---- Call back and fall -------------------------------------------------------------------

#[test]
fn a_call_back_takes_15_steps_and_drops_a_rider() {
    let mut p = on_placed_stone();
    p.step(&press_pad((0, 0), button::B, 0));
    for _ in 0..20 {
        p.step(&pad((0, 0), button::B));
    }
    // 25 + 4 * 21 = 109 cm up.
    assert_eq!(kit(&p).at[1], 109);
    p.step(&press_pad((0, 0), button::X, 0));
    // Switched off at once: the hero falls.
    assert_eq!(kit(&p).held, Held::Returning(RETURN));
    assert!(!record(&p).solid());
    assert_eq!(p.hero.ground, None);
    let mut n = 0;
    let mut icons = Vec::new();
    while kit(&p).held != Held::InHand {
        icons.push(hud(&p).icon);
        p.step(&Pad::NONE);
        n += 1;
        assert!(n <= 20);
    }
    assert_eq!(n, 15);
    // The icon blinks while it returns (n & 4: 15, 14, 13, 12 filled,
    // 11..8 outline, 7..4 filled, 3..1 outline).
    assert!(icons.contains(&Some(true)) && icons.contains(&Some(false)));
    assert_eq!(hud(&p).icon, Some(true));
    // He landed back on S.
    for _ in 0..30 {
        p.step(&Pad::NONE);
    }
    assert_eq!(p.hero.ground, Some(S));
    // X in hand does nothing.
    p.step(&press_pad((0, 0), button::X, 0));
    assert_eq!(kit(&p).held, Held::InHand);
}

#[test]
fn a_fall_returns_the_stone_and_the_air_place() {
    let mut p = Play::start(Area::Stone, Detail::Mid);
    p.step(&press_pad((0, 0), button::B, 0));
    assert!(matches!(kit(&p).held, Held::Placed { .. }));
    kit_mut(&mut p).air_ok = false;
    // Off S's left side and down: 6 m under the safe spot is off.
    let left = stick_towards(-1.0, 0.0, p.cam.yaw, 1.0);
    let mut fell = None;
    for n in 0..300 {
        p.step(&pad(left, 0));
        if p.events.hero.fell {
            fell = Some(n);
            break;
        }
    }
    assert!(fell.is_some());
    assert!(p.hero.pos[1] < p.hero.safe[1] - FALL_ROOM);
    // Still placed while frozen; 15 steps later he is back, the stone in
    // hand and the air place allowed.
    assert!(matches!(kit(&p).held, Held::Placed { .. }));
    let mut n = 0;
    while !p.events.hero.respawned {
        p.step(&Pad::NONE);
        n += 1;
        assert!(n <= 20);
    }
    assert_eq!(n, 15);
    assert_eq!(kit(&p).held, Held::InHand);
    assert!(kit(&p).air_ok);
    assert_eq!(p.hero.ground, Some(S));
    assert!(!record(&p).solid());
}

// ---- Why each stage needs the stone ------------------------------------------------------

/// A jump's height (subs): 1006 + 892 + .. + 94 (9 steps, 1120 less
/// 114 a step) = (1006 + 94) * 9 / 2 = 4950 (1.21 m).
const PEAK: i32 = 4_950;

#[test]
fn each_stage_fails_without_the_stone() {
    assert_eq!((JUMP, GRAVITY), (1_120, 114));
    assert_eq!((1..=9).map(|k| JUMP - GRAVITY * k).sum::<i32>(), PEAK);
    // Cliff (3.0 m = 12288 subs): a jump reaches 4950; from the stone on
    // the ground 1024 + 4950 = 5974; one air place and a jump from it
    // 2 * 4950 = 9900 (and a second air place is refused); all short.
    // After the raise (2.75 m = 11264) K's top is 1024 = STEP_UP over
    // the feet: a step, and a jump would reach 11264 + 4950 = 16214.
    const { assert!(PEAK < 12_288 && 1_024 + PEAK < 12_288 && 2 * PEAK < 12_288) };
    assert_eq!(cm(300) - cm(275), STEP_UP);
    // In play: run at the face and jump, again and again: never on K.
    let mut p = play_at([0, 0, 0], Some(S), 0);
    for n in 0..120 {
        let a = if n % 25 == 0 { button::A } else { 0 };
        p.step(&press_pad(FWD, a, button::A));
        assert_ne!(p.hero.ground, Some(K));
    }
    // Gap: run off K's far edge at full speed and jump at any point up to
    // the last coyote step: never on T (5 m away).
    for late in (0..=30).step_by(3) {
        let mut p = play_at([0, 300, 300], Some(K), 0);
        p.hero.vel[2] = RUN;
        let mut jumped = false;
        let mut landed = None;
        for _ in 0..120 {
            let go = !jumped && p.hero.pos[2] >= cm(370 + late);
            let a = if go { button::A } else { 0 };
            p.step(&press_pad(FWD, a, if jumped { button::A } else { 0 }));
            jumped |= go;
            if let Some(l) = p.events.hero.landed {
                landed = Some(l);
                break;
            }
            if p.events.hero.fell {
                break;
            }
        }
        assert_ne!(landed, Some(T), "jump at z {} cm", 370 + late);
    }
    // Goal (4 m over T): a jump from T reaches T + 1.21 m; from an
    // air-placed stone (at most T + 1.21 m) T + 2.42 m; only a raise of
    // 2.5 m more makes T + 1.21 + 2.5 + 1.21 = T + 4.92 >= T + 4.0.
    assert!(2 * PEAK < cm(400));
    assert!(PEAK + cm(250) + PEAK >= cm(400));
    let mut p = play_at([0, 300, 1250], Some(T), 0);
    for n in 0..150 {
        let a = if n % 25 == 0 { button::A } else { 0 };
        p.step(&press_pad(FWD, a, button::A));
        assert_ne!(p.hero.ground, Some(G));
    }
}

// ---- The HUD -------------------------------------------------------------------------------

#[test]
fn the_hud_shows_the_name_clock_icon_hint_and_goal() {
    let mut p = Play::start(Area::Stone, Detail::Mid);
    let u = hud(&p);
    assert_eq!(u.text[0].as_str(), "STONE");
    assert_eq!(u.text[1].as_str(), "0.0");
    assert_eq!(u.icon, Some(true));
    assert_eq!(u.msg, Some("B STONE  X CALL"));
    assert_eq!(u.label.map(|l| l.1), Some("GOAL"));
    // The hint for the first 150 steps only.
    assert_eq!(HINT_STEPS, 150);
    for _ in 0..149 {
        p.step(&Pad::NONE);
    }
    assert_eq!(hud(&p).msg, Some("B STONE  X CALL"));
    p.step(&Pad::NONE);
    assert_eq!(hud(&p).msg, None);
    // 150 steps: 150 / 3 = 50 tenths, "5.0".
    assert_eq!(hud(&p).text[1].as_str(), "5.0");
    // Placed: the outline icon.
    p.step(&press_pad((0, 0), button::B, 0));
    assert_eq!(hud(&p).icon, Some(false));
}

// ---- The route -----------------------------------------------------------------------------

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
}

impl Bot {
    fn new() -> Bot {
        Bot {
            p: Box::new(Proto::new()),
            calls: 0,
            done: None,
            log: Vec::new(),
            shots: Vec::new(),
        }
    }

    fn shot(&mut self, name: &'static str) {
        self.shots.push((self.calls - 1, name));
    }

    fn call(&mut self, held: u32, pressed: u32, left: (i16, i16)) {
        let held = held | pressed;
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
        if std::env::var_os("STONE_TRACE").is_some() && matches!(self.p.play.kit, Kit::Stone(_)) {
            let (h, k) = (self.hero(), self.stone());
            println!(
                "{} h{held:x} p{pressed:x} pos {:?} vel {:?} g {:?} | {:?} at {:?} ok {}",
                self.calls, h.pos, h.vel, h.ground, k.held, k.at, k.air_ok
            );
        }
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

    fn stone(&self) -> Stone {
        *kit(&self.p.play)
    }

    /// The stick towards (x, z) cm at `speed` (0: slowing down near it).
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

    /// Runs towards `to` until `end` (after a step); A is pressed once
    /// when `jump` says so (standing) and held after; B is pressed once
    /// when `place` says so (in the air). Panics after 300 steps.
    fn leg(
        &mut self,
        to: [i16; 2],
        speed: f64,
        jump: fn(&Hero, &Stone) -> bool,
        place: fn(&Hero, &Stone) -> bool,
        end: fn(&Hero, &Stone) -> bool,
    ) {
        let (mut jumped, mut placed) = (false, false);
        for _ in 0..300 {
            if self.done.is_some() {
                return;
            }
            let (h, s) = (self.hero(), self.stone());
            let stick = self.towards(to, speed);
            let mut pressed = 0;
            if !jumped && h.ground.is_some() && h.frozen == 0 && jump(&h, &s) {
                jumped = true;
                pressed |= button::A;
            } else if jumped && !placed && h.ground.is_none() && place(&h, &s) {
                placed = true;
                pressed |= button::B;
            }
            let held = if jumped && h.ground.is_none() || pressed & button::A != 0 {
                button::A
            } else {
                0
            };
            self.call(held, pressed, stick);
            if end(&self.hero(), &self.stone()) {
                return;
            }
        }
        panic!(
            "leg to {to:?} not ended in 300 steps: hero {:?} stone {:?}",
            self.hero(),
            self.stone()
        );
    }

    /// Stops (no input) until the hero stands still.
    fn settle(&mut self) {
        for _ in 0..10 {
            let h = self.hero();
            if h.ground.is_some() && h.vel == [0; 3] {
                return;
            }
            self.call(0, 0, (0, 0));
        }
    }

    /// Calls the stone back (X) and waits, standing, until it is in hand.
    fn fetch(&mut self) {
        self.settle();
        self.press(button::X);
        for _ in 0..20 {
            if self.stone().held == Held::InHand {
                return;
            }
            self.call(0, 0, (0, 0));
        }
        panic!("the stone did not come back: {:?}", self.stone());
    }

    /// Walks back along z to `z` cm (keeping x 0) and stops there.
    fn back_to(&mut self, z: i16) {
        self.leg([0, z], 0.0, never, never, move_end);
        self.settle();
    }

    /// A fresh press of B, then B held until the stone is all the way
    /// up (or `n` steps).
    fn raise(&mut self, n: usize) {
        self.call(button::B, button::B, (0, 0));
        for _ in 1..n {
            let s = self.stone();
            if let Held::Placed { base } = s.held
                && s.at[1] >= base + RAISE_MAX
            {
                break;
            }
            self.call(button::B, 0, (0, 0));
        }
        self.call(0, 0, (0, 0));
    }

    /// Opens STONE from the area menu (DOWN three times, A), then sets
    /// the detail in the pause menu unless it is the start's.
    fn enter(&mut self, detail: Detail) {
        for _ in 0..3 {
            self.press(button::DOWN);
        }
        self.press(button::A);
        assert_eq!(self.p.play.area, Area::Stone);
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

/// The end of a walk back: within 15 cm of the target (read from the
/// hero's z only: the targets are z 2.4 m and 11.0 m, both behind).
fn move_end(h: &Hero, _: &Stone) -> bool {
    let z = h.pos[2];
    (z - cm(240)).abs() < cm(15) || (z - cm(1100)).abs() < cm(15)
}

fn never(_: &Hero, _: &Stone) -> bool {
    false
}

/// The jump's top: rising no more.
fn at_top(h: &Hero, _: &Stone) -> bool {
    h.vel[1] <= 0
}

/// The route of the spec, from the area menu to done.
fn route(detail: Detail) -> Bot {
    let mut bot = Bot::new();
    bot.enter(detail);
    bot.shot("start");
    // 1. Cliff: to 1.5 m before the face, place the stone, step on it,
    // raise it all the way, and jump onto K.
    bot.leg([0, 60], 0.0, never, never, |h, _| h.pos[2] >= cm(45));
    bot.settle();
    bot.press(button::B);
    // The hero stops near z 0.46 m: the stone's middle 1 m ahead, on S.
    let s = bot.stone();
    assert_eq!(s.held, Held::Placed { base: 25 }, "{:?}", bot.hero());
    assert!((140..=150).contains(&s.at[2]), "{s:?}");
    bot.leg([0, 150], 0.3, never, never, |h, _| {
        h.ground == Some(STONE) && h.pos[2] >= cm(130)
    });
    bot.settle();
    bot.shot("on-stone");
    bot.raise(70);
    assert_eq!(bot.stone().at[1], 275);
    bot.shot("raised");
    bot.leg(
        [0, 330],
        1.0,
        |_, _| true,
        never,
        |h, _| h.ground == Some(K),
    );
    // 2. Gap: call the stone back, run off K's far edge, jump, place it
    // at the top, land on it, run across and jump again to T.
    bot.fetch();
    bot.shot("on-k");
    bot.back_to(240);
    bot.leg(
        [0, 1100],
        1.0,
        |h, s| h.pos[2] >= cm(385) && s.held == Held::InHand,
        at_top,
        |h, _| h.ground == Some(STONE),
    );
    bot.shot("gap-stone");
    bot.leg(
        [0, 1100],
        1.0,
        |h, s| h.ground == Some(STONE) && h.pos[2] >= cm(s.at[2] + 40),
        never,
        |h, _| h.ground == Some(T),
    );
    // 3. Goal: call the stone back, jump off T's far edge, place it at
    // the top, stand still on it, raise it and jump to G.
    bot.fetch();
    bot.shot("on-t");
    bot.back_to(1100);
    bot.leg(
        [0, 1500],
        1.0,
        |h, s| h.pos[2] >= cm(1285) && s.held == Held::InHand,
        at_top,
        |h, _| h.ground == Some(STONE),
    );
    let s = bot.stone();
    bot.leg([s.at[0], s.at[2]], 0.0, never, never, |h, s| {
        (h.pos[2] - cm(s.at[2])).abs() < cm(15)
    });
    bot.settle();
    assert_eq!(bot.hero().ground, Some(STONE));
    bot.shot("goal-stone");
    bot.raise(70);
    bot.shot("goal-raised");
    bot.leg(
        [0, 1725],
        1.0,
        |_, _| true,
        never,
        |h, _| h.ground == Some(G),
    );
    bot
}

#[test]
fn stone_route_reaches_done() {
    for d in [Detail::Mid, Detail::Low, Detail::Max] {
        let bot = route(d);
        let Some(Note::Done(Area::Stone, Some(t))) = bot.done else {
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
        assert!(bot.calls < 3_000);
        assert_eq!(bot.p.play.hero.falls, 0);
        assert_eq!(bot.p.play.hero.ground, Some(G));
        assert!(bot.p.play.done);
        assert_eq!(bot.p.best[Area::Stone.index()], t);
        // The end's scene (without the shared done panel over it): the
        // hero on G is in view, not hidden by the raised stone.
        let mut paint = Box::new(Painter::new());
        let mut f = Box::new(Frame::new());
        paint.scene(&mut f, &bot.p.play);
        println!(
            "{d:?}: at done, hero crimson {} px, cap {} px, slate {} px",
            shows(&f, mat::HERO),
            shows(&f, mat::CAP),
            shows(&f, mat::SLATE)
        );
        assert!(shows(&f, mat::HERO) > 20 && shows(&f, mat::CAP) > 3);
        if let Some(dir) = std::env::var_os("PROTO3D_PICTURES") {
            let det = d.name().to_lowercase();
            let file = Path::new(&dir).join(format!("stone-{det}-done-scene.png"));
            std::fs::write(file, png(&f, 4)).unwrap();
        }
        if d == Detail::Mid
            && let Ok(path) = std::env::var("STONE_SCRIPT")
        {
            write_script(&bot, t, Path::new(&path));
        }
    }
}

// ---- The simulator script ----------------------------------------------------------------

/// The simulator's frame of the route's first call (the home tiles and
/// the PROTO3D start, as in `tests/sail.rs`).
const SCRIPT_FIRST: usize = 225;

/// Writes the MID route as a simulator script (`--script`): the home
/// prelude, then one game step a frame, the buttons as presses and
/// releases (A south, B east, X west, START plus, UP and DOWN the
/// d-pad), the left stick as `stick left`, and the marked pictures.
fn write_script(bot: &Bot, t: u16, path: &Path) {
    use std::fmt::Write as _;
    let mut s = String::new();
    s.push_str("# STONE: the route of tests/stone.rs (stone_route_reaches_done, MID),\n");
    s.push_str("# written by that test with STONE_SCRIPT=<this file>. Do not edit by hand.\n");
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
    s.push_str("# the area menu: DOWN three times, A (south) takes STONE; then the route\n");
    let names = [
        (button::UP, "up"),
        (button::DOWN, "down"),
        (button::A, "south"),
        (button::B, "east"),
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

// ---- Pictures ------------------------------------------------------------------------------

/// The moments: the start on S facing the cliff; on the raised stone at
/// the cliff's face; on the air-placed stone over the gap with T ahead;
/// on the raised stone before G (the busiest view, all five blocks).
const MOMENTS: [&str; 4] = ["start", "cliff", "gap", "goal"];

/// A play at a moment, the camera snapped behind the hero and turned
/// `turn` more.
fn moment(name: &str, d: Detail, turn: u16) -> Play {
    let mut p = Play::start(Area::Stone, d);
    // (hero feet cm, the stone's top middle cm, its base, clock)
    let (at, stone, base, clock): ([i16; 3], Option<[i16; 3]>, i16, u16) = match name {
        "cliff" => ([0, 275, 146], Some([0, 275, 146]), 25, 120),
        // As in the route: placed at 4.17 m (the step after the jump's
        // top), at z 5.80 m and 14.81 m; raised to 6.67 m before G.
        "gap" => ([0, 417, 590], Some([0, 417, 580]), 417, 200),
        "goal" => ([0, 667, 1495], Some([0, 667, 1481]), 417, 330),
        _ => (st::START, None, 0, 0),
    };
    let s = kit_mut(&mut p);
    s.steps = clock;
    if let Some(a) = stone {
        s.held = Held::Placed { base };
        s.at = a;
        s.air_ok = false;
    }
    sync(&mut p);
    let b: Vec<Block> = p.blocks().to_vec();
    let top = if stone.is_some() { STONE } else { S };
    let h = Hero::new([cm(at[0]), cm(at[1]), cm(at[2])], top);
    p.hero = h;
    p.cam.yaw = turn;
    p.cam.snap(&p.hero, &b, p.world.props, d.level());
    p.shadow = p.hero.shadow(&b);
    p.ring = p
        .hero
        .ring_with(&Pad::NONE, p.cam.yaw, &b, &p.kit.body(&p.hero));
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
            "STONE {} sweep: 64 pictures, most tris {most} at {most_at:?} (budget {}), cracks worst {worst} at {worst_at:?}",
            d.name(),
            BUDGET[usize::from(d.level())]
        );
        assert!(most <= BUDGET[usize::from(d.level())], "{d:?}: {most}");
        assert!(worst <= 9, "{d:?}: {worst} crack pixels");
    }
}

#[test]
fn the_moments_show_the_hero_and_the_stone() {
    for d in Detail::ALL {
        for name in MOMENTS {
            let p = moment(name, d, 0);
            let (f, _) = picture(&p);
            // The hero is never hidden: his crimson and his cap show.
            assert!(shows(&f, mat::HERO) > 20, "{d:?} {name}: crimson");
            assert!(shows(&f, mat::CAP) > 3, "{d:?} {name}: cap");
            // The stone's slate shows when it is placed.
            if name != "start" {
                assert!(shows(&f, mat::SLATE) > 20, "{d:?} {name}: slate");
            } else {
                assert_eq!(shows(&f, mat::SLATE), 0, "{d:?} {name}: no slate");
            }
        }
        // Before G: the beacon's gold.
        let (f, _) = picture(&moment("goal", d, 0));
        assert!(shows(&f, mat::GOLD) > 0, "{d:?} goal: beacon");
    }
}

/// Golden pictures: a hash of three moments at each detail, kept in
/// `tests/golden/stone.txt` (as MACHINES and SAIL keep their own).
#[test]
fn golden_pictures_of_three_moments() {
    let dir = std::env::var_os("PROTO3D_PICTURES");
    let mut text = String::from("# PROTO3D STONE: FNV-1a hashes of three moments at each detail\n");
    for d in Detail::ALL {
        for name in &MOMENTS[1..] {
            let (f, st) = picture(&moment(name, d, 0));
            let det = d.name().to_lowercase();
            println!("STONE {det} {name}: tris {} cracks {}", st.tris, cracks(&f));
            text += &format!("{det} {name} {:016x}\n", hash(&f));
            if let Some(dir) = &dir {
                std::fs::create_dir_all(dir).unwrap();
                let file = Path::new(dir).join(format!("stone-{det}-{name}.png"));
                std::fs::write(file, png(&f, 4)).unwrap();
            }
        }
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/stone.txt");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let want = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        text, want,
        "pictures changed: look at them, then UPDATE_GOLDEN=1"
    );
}

/// The live list keeps its order: the stone is the last record.
#[test]
fn the_stone_is_the_last_live_record() {
    let p = Play::start(Area::Stone, Detail::Mid);
    assert_eq!(p.live, Live::of(&st::WORLD));
    assert_eq!(usize::from(p.live.n), 5);
}
