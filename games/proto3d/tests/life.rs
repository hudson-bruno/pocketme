//! The creatures of ISLAND (src/life.rs): the burrs' patrols stay on
//! their tops, a knock-back never throws the hero through a wall or off
//! the island, the squash works from above only, RESTART brings every
//! creature back, the pause menu freezes them, no fixed list runs over,
//! and the creatures' rules are the same at every detail level. Also: a
//! play with the creatures differs from one without them only once the
//! hero has met one.

mod common;

use common::Lcg;
use game_kit::{Frame, button};
use proto3d::area::{Area, MAX_EXTRAS};
use proto3d::draw::{MAX_ITEMS, Painter};
use proto3d::hero::Hero;
use proto3d::island::{Block, ISLAND, World, body_fits, flag, inside_any, top_under};
use proto3d::life::{
    self, BOUNCE, BURR_H, BURR_R, Critter, FLAT, Life, MAX_CRITTERS, STUN, TUFT_H, TUFT_R, kind,
};
use proto3d::maths::cm;
use proto3d::pause::Menu;
use proto3d::play::{Detail, Play};
use proto3d::step::Pad;
use proto3d::tune::{HALF, PITCH};
use proto3d::{Input, Note, Proto};

fn recs() -> &'static [Critter] {
    ISLAND.critters
}

fn burrs() -> impl Iterator<Item = (usize, &'static Critter)> {
    recs()
        .iter()
        .enumerate()
        .filter(|(_, c)| c.kind == kind::BURR)
}

/// The top (block, height) right under (x, z) at `y` or below.
fn top(x: i32, z: i32, y: i32) -> Option<(u8, i32)> {
    top_under(ISLAND.blocks, x, z, y)
}

/// The creature's box (`r` half wide, `h` high, feet at `p`) stands
/// wholly on one flat top at `p[1]` and cuts no block.
fn stands_clear(p: [i32; 3], r: i32, h: i32) -> Result<(), String> {
    let mut first = None;
    for (dx, dz) in [
        (0, 0),
        (r, r),
        (-r, r),
        (r, -r),
        (-r, -r),
        (r, 0),
        (-r, 0),
        (0, r),
        (0, -r),
    ] {
        let (x, z) = (p[0] + dx, p[2] + dz);
        match top(x, z, p[1] + cm(1)) {
            Some((b, y)) if y == p[1] => {
                first.get_or_insert(b);
            }
            t => return Err(format!("corner ({dx}, {dz}): top {t:?}, feet {}", p[1])),
        }
        for y in [cm(2), h / 2, h - cm(1)] {
            if let Some(b) = inside_any(ISLAND.blocks, [x, p[1] + y, z]) {
                return Err(format!("corner ({dx}, {dz}) at {y}: inside block {b}"));
            }
        }
    }
    Ok(())
}

#[test]
fn every_patrol_stays_on_its_island_top() {
    let mut n = 0;
    for (i, c) in burrs() {
        for clock in 0..life::cycle(c) {
            let (p, _, _) = life::patrol(c, clock as u16);
            stands_clear(p, BURR_R, BURR_H)
                .unwrap_or_else(|e| panic!("burr {i} clock {clock}: {e}"));
        }
        n += 1;
    }
    assert!((3..=5).contains(&n), "3 to 5 burrs: {n}");
    let mut tufts = 0;
    for (i, c) in recs().iter().enumerate() {
        if c.kind == kind::TUFT {
            stands_clear(c.at, TUFT_R, TUFT_H).unwrap_or_else(|e| panic!("tuftling {i}: {e}"));
            tufts += 1;
        }
    }
    assert!((2..=3).contains(&tufts), "2 or 3 tuftlings: {tufts}");
    // A whole cycle comes back to the start, and the play's burrs follow
    // the patrol step by step.
    let mut p = Play::start(Area::Island, Detail::Mid);
    p.hero = Hero::new([cm(600), 0, cm(-600)], 0);
    p.hero.pos[1] = cm(2_000);
    p.hero.ground = None;
    for _ in 0..400 {
        p.hero.pos = [cm(600), cm(2_000), cm(-600)];
        p.hero.vel = [0; 3];
        p.step(&Pad::NONE);
        for (i, c) in burrs() {
            assert_eq!(
                p.life.pos(recs(), i, p.steps),
                life::patrol(c, Life::clock(c, p.steps)).0
            );
        }
    }
}

/// The hero standing at (x, z) subs on the top at `y`, if he fits there.
fn standing(x: i32, y: i32, z: i32) -> Option<Hero> {
    let (b, fy) = top(x, z, y + cm(1))?;
    (fy == y && body_fits(ISLAND.blocks, [x, y, z])).then(|| Hero::new([x, y, z], b))
}

/// Every place round a burr at many moments of its patrol: the hero
/// touches its side, is knocked, and in the next three seconds (no input)
/// never falls off the island, never stands inside a block, and is never
/// thrown more than a short way.
#[test]
fn a_knock_back_never_throws_the_hero_through_a_wall_or_off_the_island() {
    let (mut cases, mut sideways) = (0, 0);
    let mut far = 0;
    for (i, c) in burrs() {
        let cyc = life::cycle(c);
        for clock in (0..cyc).step_by(3) {
            let (p, _, _) = life::patrol(c, ((clock + 1) % cyc) as u16);
            for k in 0..16u16 {
                let d = k * 4_096;
                let reach = (BURR_R + HALF) * 3 / 4;
                let x = p[0] + game_3d::sin_fine(d) * reach / game_3d::ONE14;
                let z = p[2] + game_3d::cos_fine(d) * reach / game_3d::ONE14;
                let Some(hero) = standing(x, p[1], z) else {
                    continue;
                };
                let mut play = Play::start(Area::Island, Detail::Mid);
                play.steps = (clock - i32::from(c.phase)).rem_euclid(cyc) as u32;
                play.hero = hero;
                play.step(&Pad::NONE);
                // Another creature may be nearer; only this burr's knock counts.
                if play.events.life.hit() != Some(i as u8) {
                    continue;
                }
                assert_eq!(play.hero.stun, STUN);
                cases += 1;
                if play.hero.vel[0] != 0 || play.hero.vel[2] != 0 {
                    sideways += 1;
                }
                let start = play.hero.pos;
                for s in 0..90 {
                    play.step(&Pad::NONE);
                    let h = &play.hero;
                    assert!(
                        !play.events.hero.fell && h.frozen == 0,
                        "burr {i} clock {clock} dir {k}: fell at step {s}"
                    );
                    assert!(
                        body_fits(play.blocks(), h.pos),
                        "burr {i} clock {clock} dir {k}: inside a block at step {s}"
                    );
                    let moved = (h.pos[0] - start[0]).abs().max((h.pos[2] - start[2]).abs());
                    far = far.max(moved);
                }
            }
        }
    }
    println!(
        "knocks {cases}, sideways {sideways}, farthest {} cm",
        far * 25 / 1024
    );
    assert!(cases > 200, "{cases}");
    assert!(
        sideways * 2 > cases,
        "most knocks throw sideways: {sideways} of {cases}"
    );
}

/// A fresh ISLAND at MID with the first burr's next place `p` (after one
/// step), and that burr's number.
fn first_burr() -> (Play, usize, [i32; 3]) {
    let (i, c) = burrs().next().unwrap();
    let play = Play::start(Area::Island, Detail::Mid);
    let (p, _, _) = life::patrol(c, Life::clock(c, play.steps + 1));
    (play, i, p)
}

#[test]
fn the_squash_works_from_above_only() {
    // From above, falling: squashed, the hero bounces, the burr is gone
    // after FLAT steps and touches nothing any more.
    let (mut play, i, p) = first_burr();
    play.hero = Hero::new([p[0], p[1] + BURR_H + cm(5), p[2]], 0);
    play.hero.ground = None;
    play.hero.air_steps = 10;
    play.hero.vel = [0, -300, 0];
    play.step(&Pad::NONE);
    assert_eq!(play.events.life.squashed(), Some(i as u8));
    assert_eq!(play.events.life.hit(), None);
    assert_eq!(play.hero.vel[1], BOUNCE);
    assert_eq!(play.hero.stun, 0);
    for _ in 1..FLAT {
        assert!(play.life.shown(recs(), i));
        play.step(&Pad::NONE);
    }
    assert!(!play.life.shown(recs(), i));
    let gone = play.life;
    play.hero = standing(p[0], p[1], p[2]).unwrap();
    for _ in 0..60 {
        play.step(&Pad::NONE);
        assert_eq!(play.events.life.hit(), None);
        assert_eq!(play.events.life.squashed(), None);
    }
    assert_eq!(play.life.c[i].t, gone.c[i].t);

    // From the side, on the ground: knocked, never squashed.
    let (mut play, i, p) = first_burr();
    play.hero = standing(p[0] - BURR_R - HALF + cm(4), p[1], p[2]).unwrap();
    play.step(&Pad::NONE);
    assert_eq!(play.events.life.hit(), Some(i as u8));
    assert_eq!(play.events.life.squashed(), None);
    assert!(play.life.shown(recs(), i));

    // Rising through it (a jump from beside): knocked, not squashed.
    let (mut play, i, p) = first_burr();
    play.hero = Hero::new([p[0] + cm(10), p[1] + cm(10), p[2]], 0);
    play.hero.ground = None;
    play.hero.air_steps = 10;
    play.hero.vel = [0, 400, 0];
    play.step(&Pad::NONE);
    assert_eq!(play.events.life.squashed(), None);
    assert_eq!(play.events.life.hit(), Some(i as u8));

    // Falling, but the feet were already below its top (beside it):
    // knocked, not squashed.
    let (mut play, i, p) = first_burr();
    play.hero = Hero::new([p[0] + cm(20), p[1] + BURR_H / 2, p[2]], 0);
    play.hero.ground = None;
    play.hero.air_steps = 10;
    play.hero.vel = [0, -100, 0];
    play.step(&Pad::NONE);
    assert_eq!(play.events.life.squashed(), None);
    assert_eq!(play.events.life.hit(), Some(i as u8));
}

/// One call of one step with these buttons held and `pressed` new.
fn call(p: &mut Proto, held: u32, pressed: u32) -> Note {
    p.control(
        &Input {
            held,
            pressed,
            delta_us: 33_333,
            ..Input::default()
        },
        |_| {},
    )
}

/// The game on ISLAND (the area menu's first choice), after a few steps.
fn on_island() -> Box<Proto> {
    let mut p = Box::new(Proto::new());
    call(&mut p, button::A, button::A);
    call(&mut p, 0, 0);
    assert!(!p.areas.open && !p.menu.open);
    assert_eq!(p.play.area, Area::Island);
    p
}

#[test]
fn restart_brings_every_creature_back() {
    let mut p = on_island();
    // Squash the first burr and let the time run on: every creature has
    // moved or changed.
    let (i, c) = burrs().next().unwrap();
    let (at, _, _) = life::patrol(c, Life::clock(c, p.play.steps + 1));
    p.play.hero = Hero::new([at[0], at[1] + BURR_H + cm(5), at[2]], 0);
    p.play.hero.ground = None;
    p.play.hero.air_steps = 10;
    p.play.hero.vel = [0, -300, 0];
    call(&mut p, 0, 0);
    for _ in 0..100 {
        call(&mut p, 0, 0);
    }
    assert!(!p.play.life.shown(recs(), i));
    assert_ne!(p.play.life, Life::new(recs()));
    // START (the menu opens on HIGH, the start), two downs, A: RESTART.
    call(&mut p, button::START, button::START);
    assert!(p.menu.open);
    for _ in 0..2 {
        call(&mut p, button::DOWN, button::DOWN);
    }
    assert_eq!(
        call(&mut p, button::A, button::A),
        Note::Restart(Area::Island)
    );
    assert_eq!(p.play.life, Life::new(recs()));
    assert_eq!(p.play.hero.stun, 0);
    for j in 0..recs().len() {
        assert!(p.play.life.shown(recs(), j));
    }
    assert_eq!(
        p.play.fingerprint(),
        Play::start(Area::Island, p.play.detail).fingerprint()
    );
}

#[test]
fn the_pause_menu_freezes_them() {
    let mut p = on_island();
    // A knock under way: the stun must wait too.
    let (_, c) = burrs().next().unwrap();
    let (at, _, _) = life::patrol(c, Life::clock(c, p.play.steps + 1));
    p.play.hero = standing(at[0] - BURR_R - HALF + cm(4), at[1], at[2]).unwrap();
    call(&mut p, 0, 0);
    assert!(p.play.hero.stun > 0);
    call(&mut p, button::START, button::START);
    assert!(p.menu.open);
    let (life, hero, steps) = (p.play.life, p.play.hero, p.play.steps);
    for k in 0..300u32 {
        let b = [0, button::LEFT, button::RIGHT, button::UP, 0][(k % 5) as usize];
        call(&mut p, b, 0);
        assert!(p.menu.open);
        assert_eq!(p.play.life, life);
        assert_eq!(p.play.hero, hero);
        assert_eq!(p.play.steps, steps);
    }
    // Closed again, they move on.
    call(&mut p, button::START, button::START);
    assert!(!p.menu.open);
    for _ in 0..3 {
        call(&mut p, 0, 0);
    }
    assert_ne!(p.play.life, life);
}

#[test]
fn no_fixed_list_ever_overflows() {
    assert!(ISLAND.critters.len() <= MAX_CRITTERS);
    // Layer 2's list: every raised block, every tree, bush and tuft patch,
    // every coin, every creature and every extra together fit.
    for a in Area::ALL {
        let p = Play::start(a, Detail::Max);
        let w: &World = p.world;
        let raised = p
            .blocks()
            .iter()
            .filter(|b: &&Block| b.hi[1] > 0 || b.flags & flag::UP != 0)
            .count();
        let props = w.props.iter().filter(|q| q.what() <= 2).count();
        let n = raised + props + w.coins.len() + w.critters.len() + MAX_EXTRAS;
        println!("{}: at most {n} of {MAX_ITEMS} items", a.name());
        assert!(n <= MAX_ITEMS, "{a:?}: {n}");
        assert!(w.critters.len() <= MAX_CRITTERS);
    }
    // The renderer's list: a sweep of ISLAND at every level, the creatures
    // at several moments, never drops a triangle.
    let mut paint = Box::new(Painter::new());
    let mut f = Box::new(Frame::new());
    let mut views = 0;
    for d in Detail::ALL {
        for t in [0u32, 37, 91, 160] {
            let mut base = Play::start(Area::Island, d);
            for _ in 0..t {
                base.hero.pos = [cm(0), cm(3_000), cm(-900)];
                base.hero.vel = [0; 3];
                base.hero.ground = None;
                base.step(&Pad::NONE);
            }
            for x in (-600..=600).step_by(150) {
                for z in (-600..=1_300).step_by(150) {
                    let Some((b, y)) = top(cm(x), cm(z), cm(400)) else {
                        continue;
                    };
                    for k in 0..8u16 {
                        let mut p = base;
                        p.hero = Hero::new([cm(x), y, cm(z)], b);
                        p.hero.yaw = k * 8_192;
                        p.cam.yaw = k * 8_192;
                        p.cam.pitch = PITCH;
                        let props = p.world.props;
                        let blocks = p.blocks().to_vec();
                        p.cam.snap(&p.hero, &blocks, props, d.level());
                        paint.picture(&mut f, &p, &Menu::new(), false, 1);
                        assert_eq!(paint.r.stats.dropped, 0, "{d:?} t{t} x{x} z{z} k{k}");
                        views += 1;
                    }
                }
            }
        }
    }
    assert!(views > 1_000, "{views}");
}

/// The hero and creature state that must not depend on the detail.
fn rules_state(p: &Play) -> ([i32; 3], [i32; 3], u8, Option<u8>, Life) {
    (p.hero.pos, p.hero.vel, p.hero.stun, p.hero.ground, p.life)
}

/// Random play round the creatures (the hero put beside each in turn),
/// the same pads at every detail: the hero and the creatures stay the
/// same, step for step, and the creatures' events happen.
#[test]
fn the_creatures_are_the_same_at_every_detail() {
    let mut plays: Vec<Play> = Detail::ALL
        .iter()
        .map(|&d| Play::start(Area::Island, d))
        .collect();
    let mut rng = Lcg(0x05EE_D1F3);
    let mut last = Pad::NONE;
    let (mut hits, mut squashes, mut hops) = (0, 0, 0);
    for n in 0..9_000u32 {
        if n % 300 == 0 {
            // Next creature: the hero dropped beside or above it.
            let i = (n / 300) as usize % recs().len();
            let above = (n / 300) % 3 == 1;
            for p in plays.iter_mut() {
                let at = p.life.pos(recs(), i, p.steps);
                let off = if above { 0 } else { cm(80) };
                p.hero = Hero::new(
                    [at[0] + off, at[1] + cm(if above { 120 } else { 30 }), at[2]],
                    0,
                );
                p.hero.ground = None;
                p.hero.air_steps = 10;
            }
        }
        last = rng.pad(&last);
        last.held &= !(button::START | button::SELECT);
        last.pressed &= !(button::START | button::SELECT);
        for p in plays.iter_mut() {
            p.step(&last);
        }
        let first = rules_state(&plays[0]);
        for (k, p) in plays.iter().enumerate().skip(1) {
            assert_eq!(rules_state(p), first, "step {n}: {:?}", Detail::ALL[k]);
            assert_eq!(p.events.life, plays[0].events.life, "step {n}");
        }
        let e = plays[0].events.life;
        hits += usize::from(e.hit().is_some());
        squashes += usize::from(e.squashed().is_some());
        hops += usize::from(e.hopped().is_some());
    }
    println!("hits {hits}, squashes {squashes}, hops {hops}");
    assert!(hits > 0 && squashes > 0 && hops > 0);
}

/// The same seeded play with and without the creatures: the hero is the
/// same until the first step he touches one.
#[test]
fn a_play_without_creatures_differs_only_once_the_hero_meets_one() {
    let bare: &'static World = Box::leak(Box::new(World {
        critters: &[],
        ..ISLAND
    }));
    for seed in 1..=6u64 {
        let mut with = Play::start(Area::Island, Detail::Mid);
        let mut without = Play::on(bare, Detail::Mid);
        let mut rng = Lcg(seed * 7_919);
        let mut last = Pad::NONE;
        let mut met = None;
        for n in 0..6_000u32 {
            last = rng.pad(&last);
            with.step(&last);
            without.step(&last);
            if with.hero == without.hero {
                continue;
            }
            // The first difference: the hero touches a creature now.
            let h = without.hero;
            let near = recs().iter().enumerate().any(|(i, c)| {
                let p = with.life.pos(recs(), i, with.steps);
                let (r, tall) = if c.kind == kind::BURR {
                    (BURR_R, BURR_H)
                } else {
                    (TUFT_R, TUFT_H)
                };
                (h.pos[0] - p[0]).abs() < r + HALF + cm(2)
                    && (h.pos[2] - p[2]).abs() < r + HALF + cm(2)
                    && h.pos[1] < p[1] + tall + cm(2)
                    && h.pos[1] + proto3d::tune::HERO_H > p[1]
            });
            assert!(
                near,
                "seed {seed}: the hero changed at step {n} away from every creature"
            );
            met = Some(n);
            break;
        }
        println!("seed {seed}: first meeting at step {met:?}");
    }
}

/// The records: known kinds, and every burr walks at least a metre.
#[test]
fn the_creature_records_are_whole_and_in_range() {
    for c in recs() {
        assert!(c.kind == kind::BURR || c.kind == kind::TUFT);
        if c.kind == kind::BURR {
            assert!(life::cycle(c) < i32::from(u16::MAX));
            // A real patrol: at least a metre each way, along x or z.
            assert!(c.turn < 4, "{c:?}");
            let far = life::patrol(c, u16::from(c.walk)).0;
            let d = (far[0] - c.at[0]).abs() + (far[2] - c.at[2]).abs();
            assert!(d >= cm(100), "{c:?}");
            assert_eq!(far[1], c.at[1], "{c:?}");
        }
    }
}
