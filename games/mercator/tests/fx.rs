//! The view's effects (step L1): pictures of each (into
//! `.local/mercator/shots/`, names `40-fx-*`) and what they cost.

mod common;

use common::*;
use mercator::app::seed_of;
use mercator::art;
use mercator::fixed::{Mat3, ONE14, V3, norm, scale14, sub};
use mercator::galaxy;
use mercator::scene::{Detail, Eye};
use mercator::world::{self, Career, KM, Kind, M, Mode, Pad, State};

fn docked_in(danger: u8) -> Driver {
    let mut d = Driver::new();
    d.start();
    let mut c = Career::new(seed_of(1));
    c.system = (0..galaxy::SYSTEMS as u8)
        .find(|&i| galaxy::system(c.seed, i).danger == danger)
        .unwrap();
    c.target = c.system;
    d.app.world.begin(c);
    d.app.last_dock = c;
    d.idle(2);
    d.app.sel = 0;
    d.tap(game_kit::button::A);
    d
}

fn place(d: &mut Driver, pos: V3, at: V3, speed: i32) {
    let w = &mut d.app.world;
    w.me.pos = pos;
    w.me.rot = Mat3::looking(sub(at, pos), [0, ONE14, 0]);
    w.me.speed = speed;
    w.me.throttle = speed;
    w.ships = [None; world::SHIPS];
}

/// Pixels of the frame in any of `cols`.
fn count(d: &Driver, cols: &[u8]) -> usize {
    d.frame.0.iter().filter(|p| cols.contains(p)).count()
}

#[test]
fn the_effects_show() {
    let mut d = docked_in(1);
    d.app.opts.detail = Detail::High;
    let b = d.app.world.bodies;
    let axis = b.station_rot.col(2);
    // The sun ahead, seen from near the station: corona, rays, flare.
    let pos = world::add(b.station_pos, scale14(axis, 3 * KM));
    place(&mut d, pos, b.sun_pos, 0);
    // Turn a little so the flare ghosts spread across the view.
    d.app.world.me.rot = d.app.world.me.rot.turned(1200, 1800, 0);
    d.idle(1);
    let corona = count(&d, &[art::CORONA1, art::CORONA2]);
    shot(&mut d, "40-fx-sun");
    assert!(corona > 20, "no corona: {corona}");
    // The planet's limb with the sun beside it: its rim of air.
    let up = norm(b.sun_pos, ONE14);
    let side = norm(b.station_rot.col(0), ONE14);
    let pos = world::add(scale14(up, b.planet_r * 2), scale14(side, b.planet_r));
    place(&mut d, pos, [0; 3], 0);
    d.idle(1);
    let air = count(&d, &[art::ATMOS1, art::ATMOS2]);
    shot(&mut d, "41-fx-air");
    assert!(air > 20, "no air: {air}");
    // The nebula: look round until the most of it shows.
    let mut best = (0, Mat3::IDENTITY);
    for k in 0..64u32 {
        let rot = Mat3::rotation((k * 4096) as u16, ((k / 16) * 6000) as u16, 0);
        d.app.world.me.rot = rot;
        d.idle(1);
        let n = count(&d, &[art::NEB1, art::NEB2, art::NEB3]);
        if n > best.0 {
            best = (n, rot);
        }
    }
    d.app.world.me.rot = best.1;
    d.idle(1);
    shot(&mut d, "42-fx-nebula");
    println!("nebula: most pixels shown {}", best.0);
    assert!(best.0 > 200, "no nebula");
    // Warp: the stars stream after a jump.
    d.app.world.fx.warp = 40;
    d.idle(1);
    shot(&mut d, "43-fx-warp");
    d.app.world.fx.warp = 0;
    // The station's lights and the guide lights before its mouth.
    let pos = world::add(b.station_pos, scale14(axis, 1500 * M));
    let pos = world::add(pos, scale14(b.station_rot.col(1), 200 * M));
    place(&mut d, pos, b.station_pos, 0);
    let mut lit = 0;
    for _ in 0..40 {
        d.idle(1);
        lit = lit.max(count(&d, &[art::mat::LIGHT_G]));
    }
    shot(&mut d, "44-fx-guide");
    assert!(lit > 4, "no guide lights");
}

#[test]
fn a_wreck_flies_apart_and_hits_spark() {
    let mut d = docked_in(3);
    d.app.opts.detail = Detail::High;
    d.app.opts.eye = Eye::Chase;
    let b = d.app.world.bodies;
    let at = world::add(b.station_pos, scale14(b.station_rot.col(2), 14 * KM));
    let ahead = world::add(at, scale14(b.station_rot.col(0), 350 * M));
    place(&mut d, at, ahead, 0);
    let w = &mut d.app.world;
    let i = w.add_ship(Kind::Raider, ahead, Mode::Travel).unwrap();
    let s = w.ships[i].as_mut().unwrap();
    s.rot = Mat3::looking(b.station_rot.col(1), [0, 0, ONE14]);
    s.speed = 600;
    s.hp = 1;
    // One shot: the raider goes up.
    let fire = Pad {
        fire: true,
        ..Pad::default()
    };
    for _ in 0..60 {
        if d.app.world.fx.bits.iter().any(|b| b.age < 100) {
            break;
        }
        // Kept dead ahead until the shot lands.
        let w = &mut d.app.world;
        if let Some(s) = w.ships[i].as_mut() {
            s.pos = ahead;
        }
        w.tick(&fire);
    }
    assert!(d.app.world.ships[i].is_none_or(|s| s.kind == Kind::Canister));
    let mut worst = (0u32, 0u32);
    for (k, ticks) in [3u32, 9, 30].into_iter().enumerate() {
        let youngest = |w: &world::World| w.fx.bits.iter().map(|b| b.age).min().unwrap();
        while youngest(&d.app.world) < ticks as u8 {
            d.app.world.tick(&Pad::default());
        }
        d.idle(1);
        let s = d.app.render.stats;
        worst = (worst.0.max(s.tris), worst.1.max(s.pixels));
        shot(&mut d, &format!("45-fx-wreck-{k}"));
    }
    println!("wreck: worst {} triangles, {} pixels", worst.0, worst.1);
    assert!(d.app.world.fx.bits.iter().filter(|b| b.age < 100).count() >= 4);
    // A hit on the player: the shield's ripple, then the red edge.
    d.app.opts.eye = Eye::Cockpit;
    d.app.world.fx.jolt = 8;
    d.idle(1);
    let ripple = count(&d, &[art::SHIELD]);
    shot(&mut d, "46-fx-shield");
    assert!(ripple > 20, "no ripple: {ripple}");
    d.app.world.me.shield = 0;
    d.app.world.fx.jolt = 7;
    d.idle(1);
    shot(&mut d, "47-fx-hull");
}

#[test]
fn trails_follow_speed() {
    let mut d = docked_in(1);
    d.app.opts.detail = Detail::High;
    d.app.opts.eye = Eye::Chase;
    let b = d.app.world.bodies;
    let at = world::add(b.station_pos, scale14(b.station_rot.col(2), 10 * KM));
    let ahead = world::add(at, scale14(b.station_rot.col(0), 2 * KM));
    place(&mut d, at, ahead, 900);
    let w = &mut d.app.world;
    let nose = w.me.rot.col(2);
    let right = w.me.rot.col(0);
    for (k, kind) in [Kind::Hauler, Kind::Raider, Kind::Warden]
        .into_iter()
        .enumerate()
    {
        let p = world::add(at, scale14(nose, (500 + 250 * k as i32) * M));
        let p = world::add(p, scale14(right, (k as i32 - 1) * 140 * M));
        let i = w.add_ship(kind, p, Mode::Travel).unwrap();
        let s = w.ships[i].as_mut().unwrap();
        s.rot = Mat3::looking(world::add(nose, scale14(right, 4000)), [0, ONE14, 0]);
        s.speed = 900;
    }
    d.idle(1);
    let n = count(
        &d,
        &[
            art::MATS + art::mat::ENGINE,
            3 * art::MATS + art::mat::ENGINE,
        ],
    );
    shot(&mut d, "48-fx-trails");
    assert!(n > 10, "no trails: {n}");
    assert_eq!(d.app.world.state, State::Flying);
}

#[test]
fn every_colour_is_one_the_console_takes() {
    // The console sets only palette numbers below 240 for a game.
    const { assert!(art::COLOURS_LEN <= 240) };
    for c in [
        art::FAR_SUN,
        art::FAR_PLANET,
        art::HALO,
        art::NEB1,
        art::NEB2,
        art::NEB3,
        art::ATMOS1,
        art::ATMOS2,
        art::CORONA1,
        art::CORONA2,
        art::TUNNEL2,
    ] {
        assert!(usize::from(c) < art::COLOURS_LEN, "colour {c}");
    }
    // The spare shades are those of glowing materials only.
    for c in [art::FAR_PLANET, art::NEB1, art::CORONA1, art::CORONA2] {
        let m = c % art::MATS;
        assert!(art::GLOW & (1 << m) != 0 && c >= art::MATS, "colour {c}");
    }
}

/// Pixels of the 3D view (above the panel) that differ between two
/// pictures.
fn differ(a: &game_kit::Frame, b: &game_kit::Frame) -> usize {
    let view = 96 * game_kit::W;
    a.0[..view]
        .iter()
        .zip(b.0[..view].iter())
        .filter(|(x, y)| x != y)
        .count()
}

/// The picture drawn now, without a tick (the world stands still).
fn drawn(d: &mut Driver) -> Box<game_kit::Frame> {
    d.app.draw(&mut d.frame);
    d.frame.clone()
}

/// Step M3: damaged ships smoke (light below half the hull, heavy with
/// fire below 3/10), the player's own in the chase view, wisps in the
/// cockpit; and each gun level has its bolt colour.
#[test]
fn damaged_ships_smoke_and_bolts_show_their_level() {
    use mercator::fx::{PUFFS, smoke_of};
    use mercator::scene::BEAM_LV;
    // The levels by hull share.
    assert_eq!(smoke_of(100, 100), 0);
    assert_eq!(smoke_of(50, 100), 0);
    assert_eq!(smoke_of(49, 100), 1);
    assert_eq!(smoke_of(30, 100), 1);
    assert_eq!(smoke_of(29, 100), 2);
    assert_eq!(smoke_of(0, 100), 2);
    assert_eq!(smoke_of(-5, 100), 2);
    assert_eq!(PUFFS[0], 0);
    let mut d = docked_in(3);
    d.app.opts.detail = Detail::High;
    d.app.opts.eye = Eye::Chase;
    let b = d.app.world.bodies;
    let at = world::add(b.station_pos, scale14(b.station_rot.col(2), 14 * KM));
    let ahead = world::add(at, scale14(b.station_rot.col(0), 2 * KM));
    place(&mut d, at, ahead, 600);
    let w = &mut d.app.world;
    let (nose, right, up) = (w.me.rot.col(2), w.me.rot.col(0), w.me.rot.col(1));
    let mut ids = Vec::new();
    for (k, dist) in [450, 800, 1500].into_iter().enumerate() {
        let p = world::add(at, scale14(nose, dist * M));
        let p = world::add(p, scale14(right, (k as i32 - 1) * 160 * M));
        let p = world::add(p, scale14(up, 40 * M));
        let i = w.add_ship(Kind::Raider, p, Mode::Travel).unwrap();
        let s = w.ships[i].as_mut().unwrap();
        // Crossing the view, tail to the left.
        s.rot = Mat3::looking(world::add(right, scale14(nose, -3000)), [0, ONE14, 0]);
        s.speed = 600;
        ids.push(i);
    }
    let full = drawn(&mut d);
    let hull = Kind::Raider.hull(d.app.world.sys.danger);
    let mut most = 0;
    for (name, share) in [("light", 40), ("heavy", 20)] {
        for &i in &ids {
            d.app.world.ships[i].as_mut().unwrap().hp = hull * share / 100;
        }
        let mut px = 0;
        for k in 0..4 {
            d.app.world.ticks = d.app.world.ticks.wrapping_add(7);
            let f = drawn(&mut d);
            px = px.max(differ(&full, &f));
            if k == 3 {
                shot(&mut d, &format!("49-fx-smoke-{name}"));
            }
            d.app.world.ticks = d.app.world.ticks.wrapping_sub(7);
        }
        println!("smoke {name}: 3 raiders, up to {px} pixels changed");
        assert!(px > 12, "no {name} smoke: {px}");
        most = most.max(px);
    }
    // The player's own ship, in the chase view.
    d.app.world.ships = [None; world::SHIPS];
    let clean = drawn(&mut d);
    d.app.world.car.hull = 20;
    let own = differ(&clean, &drawn(&mut d));
    shot(&mut d, "49-fx-smoke-own-chase");
    println!("smoke: own ship in the chase view, {own} pixels changed");
    assert!(own > 6, "no smoke behind the player: {own}");
    // The cockpit: wisps.
    d.app.opts.eye = Eye::Cockpit;
    d.app.world.car.hull = 100;
    let clean = drawn(&mut d);
    d.app.world.car.hull = 20;
    let wisps = differ(&clean, &drawn(&mut d));
    shot(&mut d, "49-fx-smoke-cockpit");
    println!("smoke: cockpit wisps, {wisps} pixels changed");
    assert!((6..=24).contains(&wisps), "wisps: {wisps}");
    d.app.world.car.hull = 100;
    // Bolts of each level side by side, and none in the mint of a
    // TAG's blink or a hit flash.
    for c in BEAM_LV {
        assert!(c != art::BEAM_ME && c != art::mat::FLASH && c != art::TAG_MARK);
    }
    let w = &mut d.app.world;
    for (k, lv) in [0u8, 1, 2].into_iter().enumerate() {
        // Fanning out ahead, just fired (the first part of the way).
        let side = (k as i32 - 1) * 40 * M;
        let from = world::add(at, scale14(right, side));
        let from = world::add(from, scale14(up, -15 * M));
        let to = world::add(from, scale14(nose, 600 * M));
        w.beams[k] = world::Beam {
            from,
            to: world::add(to, scale14(right, side * 3)),
            foe: false,
            age: 4,
            lv,
        };
    }
    drawn(&mut d);
    shot(&mut d, "49-fx-bolts-by-level");
    for c in BEAM_LV {
        assert!(count(&d, &[c]) > 4, "bolt colour {c} missing");
    }
    assert_eq!(count(&d, &[art::BEAM_ME]), 0);
}

/// Step M3: the particles of the heaviest fight (the cost test's fight
/// in a LAWLESS system, HIGH detail): sparks, wreck pieces, bolts,
/// blasts and smoke puffs alive in one picture, the most over 20 s
/// (MEASURED ON THE PC: counts).
#[test]
fn the_heaviest_fight_counts_its_particles() {
    use mercator::fx::{BIT_LIFE, PUFFS, smoke_of};
    use mercator::scene::SHIP_SEEN;
    let mut d = docked_in(3);
    d.app.opts.detail = Detail::High;
    let w = &mut d.app.world;
    let out = w.bodies.station_rot.col(2);
    let at = world::add(w.bodies.station_pos, scale14(out, 14 * KM));
    let side = w.bodies.station_rot.col(0);
    w.me.pos = at;
    w.me.rot = Mat3::looking(side, [0, ONE14, 0]);
    w.ships = [None; world::SHIPS];
    for k in 0..3 {
        let p = world::add(at, scale14(side, 900 * M + k * 500 * M));
        let p = world::add(p, [0, (k - 1) * 120 * M, (k - 1) * 200 * M]);
        w.add_ship(Kind::Raider, p, Mode::Hunt);
    }
    let mut most = (0, [0; 5]);
    let mut each = [0; 5];
    for _ in 0..200 {
        for _ in 0..6 {
            let w = &mut d.app.world;
            if w.state != State::Flying {
                break;
            }
            let pad = match hunter(w, 20 * KM) {
                Some(i) => fight_pad(w, i),
                None => Pad::default(),
            };
            w.tick(&pad);
            while w.take_event().is_some() {}
        }
        let w = &d.app.world;
        let near = |p| world::within(p, w.me.pos, SHIP_SEEN);
        let sparks =
            w.fx.sparks
                .iter()
                .filter(|s| s.age < s.life && near(s.at))
                .count();
        let bits =
            w.fx.bits
                .iter()
                .filter(|b| b.age < BIT_LIFE && near(b.at))
                .count();
        let bolts = w.beams.iter().filter(|b| b.age > 0).count();
        let blasts = w
            .blasts
            .iter()
            .filter(|b| b.age < world::BLAST_TICKS)
            .count();
        let puffs: u32 = w
            .ships
            .iter()
            .flatten()
            .filter(|s| s.kind != Kind::Canister && near(s.pos))
            .map(|s| PUFFS[usize::from(smoke_of(s.hp, s.kind.hull(w.sys.danger)))])
            .sum::<u32>()
            + PUFFS[usize::from(smoke_of(i32::from(w.car.hull), world::HULL_MAX))];
        let all = [sparks, bits, bolts, blasts, puffs as usize];
        let n: usize = all.iter().sum();
        for (e, a) in each.iter_mut().zip(all) {
            *e = (*e).max(a);
        }
        if n > most.0 {
            most = (n, all);
        }
    }
    let [s, b, l, x, p] = most.1;
    println!(
        "heaviest fight: {} particles at once (sparks {s}, pieces {b}, bolts {l}, blasts {x}, smoke puffs {p})",
        most.0
    );
    println!(
        "heaviest fight, most of each kind at any time: sparks {}, pieces {}, bolts {}, blasts {}, puffs {}",
        each[0], each[1], each[2], each[3], each[4]
    );
    assert!(most.0 > 0);
}
