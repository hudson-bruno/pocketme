//! Step F1: the courier as big as the others (the cockpit view stays as
//! it was), haulers that take a beating and pay.
//!
//! The pictures (measuring only): `F1_SHOTS=<folder> F1_TAG=after cargo
//! test --release --test f1 -- --ignored --nocapture` writes
//! `<tag>-<name>.png` there and prints a hash of each picture. The scenes
//! use only what the game had before F1, so the same scenes drew the
//! "before" pictures from a copy of the old game.

mod common;

use common::*;
use game_kit::button;
use mercator::app::Scene;
use mercator::fixed::{Mat3, ONE14, V3, length, scale14, sub};
use mercator::scene::{Detail, Eye};
use mercator::world::{self, Event, HAUL_HP, HEAT_MAX, KM, Kind, M, MAX_SPEED, Mode, Pad, World};

/// First hull, middle size step, no markings.
const LOOK: u8 = 0b0111;

/// Flying in the start system of galaxy 1, still, 20 km out on the
/// station's axis, looking across it, nothing else in space.
fn out_far(eye: Eye) -> Driver {
    let mut d = Driver::new();
    d.start();
    d.app.opts.detail = Detail::High;
    d.app.opts.eye = eye;
    d.idle(2);
    assert_eq!(d.app.scene, Scene::Docked);
    d.app.sel = 0;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Flight);
    let w = &mut d.app.world;
    let b = w.bodies;
    w.me.pos = world::add(b.station_pos, scale14(b.station_rot.col(2), 20 * KM));
    w.me.rot = Mat3::looking(b.station_rot.col(0), [0, ONE14, 0]);
    w.me.speed = 0;
    w.me.throttle = 0;
    w.ships = [None; world::SHIPS];
    d
}

fn put(w: &mut world::World, kind: Kind, p: V3, nose: V3) -> usize {
    let i = w.add_ship(kind, p, Mode::Travel).unwrap();
    let s = w.ships[i].as_mut().unwrap();
    s.rot = Mat3::looking(nose, [0, ONE14, 0]);
    s.speed = 0;
    s.target = world::NOBODY;
    s.look = LOOK;
    i
}

fn save(dir: &std::path::Path, tag: &str, d: &mut Driver, name: &str) {
    let c = d.app.colours().to_vec();
    save_frame(&dir.join(format!("{tag}-{name}.png")), &d.frame, &c, 3);
    println!("{tag}-{name} {:016x}", d.picture_hash());
}

/// A raider `dist` m ahead, a little up and right, three quarters on.
fn raider_ahead(eye: Eye, dist: i32) -> Driver {
    let mut d = out_far(eye);
    let w = &mut d.app.world;
    let (pos, ahead, right, up) = (w.me.pos, w.me.rot.col(2), w.me.rot.col(0), w.me.rot.col(1));
    let p = world::add(
        world::add(pos, scale14(ahead, dist * M)),
        world::add(scale14(right, dist * M / 12), scale14(up, dist * M / 20)),
    );
    let nose = world::add(
        scale14(sub(pos, p), ONE14 / 2),
        scale14(right, dist * M / 2),
    );
    put(w, Kind::Raider, p, nose);
    d.idle(1);
    d
}

/// A hauler side on, 600 m ahead and to the right.
fn hauler_beside(eye: Eye) -> Driver {
    let mut d = out_far(eye);
    let w = &mut d.app.world;
    let (pos, ahead, right) = (w.me.pos, w.me.rot.col(2), w.me.rot.col(0));
    let p = world::add(
        world::add(pos, scale14(ahead, 600 * M)),
        scale14(right, 250 * M),
    );
    put(w, Kind::Hauler, p, ahead);
    d.idle(1);
    d
}

/// Out of the mouth after a launch, and in through it on the auto-dock.
fn station(dir: &std::path::Path, tag: &str) {
    let mut d = Driver::new();
    d.start();
    d.app.opts.detail = Detail::High;
    d.app.opts.eye = Eye::Chase;
    d.idle(2);
    d.app.sel = 0;
    d.tap(button::A);
    d.app.world.ships = [None; world::SHIPS];
    d.idle(2);
    save(dir, tag, &mut d, "chase-launch-0s");
    d.idle(60);
    save(dir, tag, &mut d, "chase-launch-1s");
    d.idle(120);
    save(dir, tag, &mut d, "chase-launch-3s");
    // In: from 2.5 km out on the axis, facing in, on the auto-dock.
    let w = &mut d.app.world;
    let b = w.bodies;
    w.ships = [None; world::SHIPS];
    w.me.pos = world::add(
        world::add(b.station_pos, scale14(b.station_rot.col(2), 2500 * M)),
        scale14(b.station_rot.col(0), 60 * M),
    );
    w.me.rot = Mat3::looking(sub(b.station_pos, w.me.pos), b.station_rot.col(1));
    d.tap(button::Y);
    let mut k = 0;
    let mut marks = [1500, 1000, 700, 560];
    while d.app.scene == Scene::Flight && k < 60 * 60 {
        d.idle(1);
        k += 1;
        let dist = d.app.world.station_dist() / M;
        if let Some(m) = marks.iter_mut().find(|m| **m > 0 && dist < **m) {
            let name = format!("chase-dock-{}m", *m);
            *m = 0;
            if d.app.scene == Scene::Flight {
                d.app.world.ships = [None; world::SHIPS];
                save(dir, tag, &mut d, &name);
            }
        }
    }
    println!(
        "{tag} docked after {:.1} s: {:?}",
        f64::from(k) / 60.0,
        d.app.scene
    );
}

/// A hauler shot down 500 m ahead (its hull set to one hit), then its
/// wreck and cargo after 1 s and 4 s.
fn wreck(dir: &std::path::Path, tag: &str, eye: Eye) {
    let mut d = out_far(eye);
    let w = &mut d.app.world;
    let (pos, ahead, right) = (w.me.pos, w.me.rot.col(2), w.me.rot.col(0));
    let p = world::add(pos, scale14(ahead, 900 * M));
    let i = put(w, Kind::Hauler, p, right);
    w.ships[i].as_mut().unwrap().hp = 1;
    w.me.cool = 0;
    d.hold(button::A);
    assert!(d.app.world.ships[i].is_none_or(|s| s.kind != Kind::Hauler));
    d.idle(60);
    let e = if eye == Eye::Chase {
        "chase"
    } else {
        "cockpit"
    };
    save(dir, tag, &mut d, &format!("{e}-hauler-wreck-1s"));
    d.idle(180);
    save(dir, tag, &mut d, &format!("{e}-hauler-wreck-4s"));
    let cans: Vec<(u8, u8)> = d
        .app
        .world
        .ships
        .iter()
        .flatten()
        .filter(|s| s.kind == Kind::Canister)
        .map(|s| s.load)
        .collect();
    println!("{tag} {e} wreck canisters {cans:?}");
}

/// The cockpit pictures before step F1 (hashes printed by the picture
/// run on the game before it: step J1 took them again in the merged
/// tree, with steps M3 and M4, before F1 went in): the courier grew,
/// the cockpit view did not change. Step F2 renewed the 1200 m one on
/// purpose (in the merged tree it was 0xbe67e6f6bc925565): that
/// raider's middle is 117 m off the nose line, outside its smaller hit
/// size (72 m plus 30 m of help), so the crosshair no longer locks on
/// it. Step G1 renewed all five on purpose: the landing site's beacon
/// (and, near it, the ring's lights) is drawn in flight now; with that
/// call taken out the pictures hash as before (checked by hand).
const COCKPIT_BEFORE: [(i32, u64); 4] = [
    (300, 0x949d02ff49b762d1),
    (600, 0x1ff17524d34b848c),
    (1200, 0xfad53c378d5b4266),
    (2400, 0x4d6e7387564ee359),
];
const COCKPIT_HAULER_BEFORE: u64 = 0x3c82e54982ee5469;

#[test]
fn the_cockpit_view_is_as_before() {
    for (dist, before) in COCKPIT_BEFORE {
        let d = raider_ahead(Eye::Cockpit, dist);
        assert_eq!(d.picture_hash(), before, "raider at {dist} m");
    }
    assert_eq!(
        hauler_beside(Eye::Cockpit).picture_hash(),
        COCKPIT_HAULER_BEFORE
    );
}

/// Canisters of goods (not TAGs or rocks), by slot.
fn goods_cans(w: &World) -> [Option<(u8, u8)>; world::SHIPS] {
    w.ships.map(|s| {
        s.filter(|s| s.kind == Kind::Canister && s.load.0 < world::TAG)
            .map(|s| s.load)
    })
}

/// What one chase of a hauler came to.
struct Chase {
    /// Laser hits it took.
    hits: u32,
    /// Ticks from the first shot to its end.
    ticks: u32,
    /// Canisters shed while it still flew (hull points after the hit).
    shed: Vec<(i32, (u8, u8))>,
    /// Its load: the canisters that came when it went.
    load: Vec<(u8, u8)>,
}

/// A hauler of size step `step`, 900 m ahead and side on, chased and
/// shot with laser level `laser` by a pilot who turns to it, keeps
/// within 700 m and fires while it is in the sights and the laser is
/// below 80 % heat. Other ships are taken out each tick (the dice still
/// roll as in play).
fn chase(laser: u8, step: u8) -> (Chase, Driver) {
    let mut d = out_far(Eye::Cockpit);
    let w = &mut d.app.world;
    w.car.laser = laser;
    let (pos, ahead, right) = (w.me.pos, w.me.rot.col(2), w.me.rot.col(0));
    let h = put(
        w,
        Kind::Hauler,
        world::add(pos, scale14(ahead, 900 * M)),
        right,
    );
    let s = w.ships[h].as_mut().unwrap();
    s.look = (s.look & !0b1100) | (step << 2);
    w.me.cool = 0;
    let mut c = Chase {
        hits: 0,
        ticks: 0,
        shed: Vec::new(),
        load: Vec::new(),
    };
    let mut hp = HAUL_HP;
    let mut started = false;
    for _ in 0..60 * 120 {
        let Some(s) = w.ships[h].filter(|s| s.kind == Kind::Hauler) else {
            unreachable!()
        };
        let gap = length(sub(s.pos, w.me.pos));
        let pad = Pad {
            steer: steer_to(w, s.pos),
            throttle: if gap > 700 * M || w.me.speed < s.speed {
                256
            } else {
                -256
            },
            fire: w.first_hit().is_some_and(|(i, _)| i == h) && w.me.heat < HEAT_MAX * 8 / 10,
            ..Pad::default()
        };
        let before = goods_cans(w);
        w.tick(&pad);
        while w.take_event().is_some() {}
        for s in w.ships.iter_mut() {
            if s.is_some_and(|s| matches!(s.kind, Kind::Raider | Kind::Warden)) {
                *s = None;
            }
        }
        started |= pad.fire;
        c.ticks += u32::from(started);
        let after = goods_cans(w);
        let new: Vec<(u8, u8)> = (0..world::SHIPS)
            .filter(|&i| before[i].is_none() && after[i].is_some())
            .map(|i| after[i].unwrap())
            .collect();
        match w.ships[h] {
            Some(s) if s.kind == Kind::Hauler => {
                if s.hp < hp {
                    c.hits += 1;
                    hp = s.hp;
                }
                c.shed.extend(new.iter().map(|&l| (hp, l)));
            }
            _ => {
                c.hits += 1;
                c.load = new;
                return (c, d);
            }
        }
    }
    panic!("laser {laser}: the hauler got away ({} hits)", c.hits);
}

#[test]
fn a_hauler_takes_many_hits_sheds_once_and_pays() {
    // Laser damage 8, 14, 22 (world::LASER): ceil(540 / damage) hits.
    for (laser, step, hits) in [(0, 0, 68), (1, 1, 39), (2, 2, 25), (2, 3, 25)] {
        let (c, _) = chase(laser, step);
        let total: u32 = c.load.iter().map(|l| u32::from(l.1)).sum();
        println!(
            "laser {laser} step {step}: {} hits in {:.1} s, shed {:?}, load {:?} ({total} t)",
            c.hits,
            f64::from(c.ticks) / 60.0,
            c.shed,
            c.load
        );
        assert_eq!(c.hits, hits, "laser {laser}");
        // One canister of 3 t, shed by the hit that took it below half.
        assert_eq!(c.shed.len(), 1, "laser {laser}: {:?}", c.shed);
        let (hp, (_, n)) = c.shed[0];
        assert!(hp < HAUL_HP / 2 && hp > 0 && n == 3, "{:?}", c.shed);
        // Its load: 3 or 4 canisters, 10 + 2 * step tonnes, at most two
        // goods (one of them the shed one's).
        assert_eq!(c.load.len(), 3 + usize::from(step / 2));
        assert_eq!(total, 10 + 2 * u32::from(step));
        let mut kinds: Vec<u8> = c.load.iter().map(|l| l.0).collect();
        kinds.push(c.shed[0].1.0);
        kinds.sort_unstable();
        kinds.dedup();
        assert!(kinds.len() <= 2, "{kinds:?}");
        // The slowest laser needs a long chase, but wins it.
        assert!(c.ticks < 60 * 40, "laser {laser}: {} ticks", c.ticks);
    }
}

/// After downing the biggest hauler (4 canisters and the shed one), a
/// pilot who flies to the nearest canister each time (common's
/// `fetch_pad`) scoops all five well within their 90 s life, into the
/// starting 24 t hold.
#[test]
fn the_whole_load_can_be_scooped_in_time() {
    let (_, mut d) = chase(2, 3);
    let w = &mut d.app.world;
    let start = w.car.hold.total();
    let room = w.car.room();
    let mut t = 0u32;
    loop {
        let near = (0..world::SHIPS)
            .filter(|&i| goods_cans(w)[i].is_some())
            .min_by_key(|&i| length(sub(w.ships[i].unwrap().pos, w.me.pos)));
        let Some(i) = near else { break };
        w.tick(&fetch_pad(w, i));
        while w.take_event().is_some() {}
        for s in w.ships.iter_mut() {
            if s.is_some_and(|s| matches!(s.kind, Kind::Raider | Kind::Warden)) {
                *s = None;
            }
        }
        t += 1;
        assert!(t < u32::from(world::CARGO_LIFE), "not all scooped in time");
    }
    let got = w.car.hold.total() - start;
    println!(
        "scooped {got} t in {:.1} s (hold room was {room} t)",
        f64::from(t) / 60.0
    );
    assert_eq!(got, 16 + 3);
}

/// A convoy's hauler (traffic slot) with a raider of gun level `gun` on
/// its tail (traffic slot: it goes for the hauler, not the player), the
/// player kept 3 km off to the side: the ticks until the hauler goes and
/// the canisters that came (shed and load), or None after 5 minutes.
fn raided(gun: u8) -> Option<(u32, Vec<(u8, u8)>)> {
    let mut d = out_far(Eye::Cockpit);
    let w = &mut d.app.world;
    let (pos, ahead, right) = (w.me.pos, w.me.rot.col(2), w.me.rot.col(0));
    let h = put(
        w,
        Kind::Hauler,
        world::add(pos, scale14(ahead, 3 * KM)),
        right,
    );
    let r = put(
        w,
        Kind::Raider,
        world::add(pos, scale14(ahead, 2 * KM)),
        right,
    );
    w.ships[r].as_mut().unwrap().gun = gun;
    let (h2, r2) = (world::FIGHT, world::FIGHT + 1);
    w.ships[h2] = w.ships[h].take();
    w.ships[r2] = w.ships[r].take();
    let mut cans = Vec::new();
    for t in 0..60 * 300 {
        let Some(l) = w.ships[h2].filter(|s| s.kind == Kind::Hauler) else {
            return Some((t, cans));
        };
        w.me.pos = world::add(l.pos, scale14(l.rot.col(1), 3 * KM));
        w.me.speed = 0;
        w.car.hull = world::HULL_MAX as u8;
        let before = goods_cans(w);
        w.tick(&Pad::default());
        while w.take_event().is_some() {}
        let after = goods_cans(w);
        cans.extend(
            (0..world::SHIPS)
                .filter(|&i| before[i].is_none() && after[i].is_some())
                .map(|i| after[i].unwrap()),
        );
        // Only the two of them (the fights' own raiders are taken out).
        for s in w.ships[..world::FIGHT].iter_mut() {
            if s.is_some_and(|s| s.kind != Kind::Canister) {
                *s = None;
            }
        }
    }
    None
}

#[test]
fn a_convoy_raider_downs_a_hauler_slowly_and_its_load_floats() {
    for gun in 0..3 {
        let r = raided(gun);
        println!("raider gun {gun}: {r:?}");
        let (t, cans) = r.expect("the raider never downed the hauler");
        // At least 36 hits of 15 (540 hull points), one about every
        // 1.5 s at best: well over half a minute.
        assert!(t > 60 * 30, "gun {gun}: {t} ticks");
        // Not shot by the player: no canister shed at half, the whole
        // load (3 or 4 canisters of 10 to 16 t) floats free.
        assert!((3..=4).contains(&cans.len()), "{cans:?}");
        let total: u32 = cans.iter().map(|l| u32::from(l.1)).sum();
        assert!((10..=16).contains(&total), "{cans:?}");
    }
}

/// Out through the mouth at full throttle, back in on the auto-dock
/// from off the axis, with no crash either way (the courier is ME_X times
/// as big; its door and dock depth are unchanged).
#[test]
fn the_big_courier_leaves_and_docks_without_touching_the_station() {
    let mut d = Driver::new();
    d.start();
    d.idle(2);
    d.app.sel = 0;
    d.tap(button::A);
    let w = &mut d.app.world;
    // One tick, nothing else in space; the crashes in it.
    let tick = |w: &mut World, pad: &Pad| {
        w.tick(pad);
        let mut crashes = 0;
        while let Some(e) = w.take_event() {
            crashes += u32::from(e == Event::Crash);
        }
        w.ships = [None; world::SHIPS];
        crashes
    };
    let mut crashes = 0;
    for _ in 0..60 * 6 {
        crashes += tick(
            w,
            &Pad {
                throttle: 256,
                ..Pad::default()
            },
        );
    }
    assert!(w.station_dist() > 2 * KM, "{}", w.station_dist() / M);
    for (out, side) in [(5 * KM, 2 * KM), (3 * KM, -KM), (2500 * M, 60 * M)] {
        let b = w.bodies;
        w.me.pos = world::add(
            world::add(b.station_pos, scale14(b.station_rot.col(2), out)),
            scale14(b.station_rot.col(0), side),
        );
        w.me.rot = Mat3::looking(sub(b.station_pos, w.me.pos), [0, ONE14, 0]);
        w.me.speed = MAX_SPEED / 2;
        let mut t = 0;
        while w.state != world::State::Docked {
            let pad = Pad {
                auto: !w.me.auto,
                ..Pad::default()
            };
            crashes += tick(w, &pad);
            t += 1;
            assert!(t < 60 * 120, "from {} m: no dock after 2 minutes", out / M);
        }
        println!(
            "from {} m out, {} m aside: docked after {:.1} s",
            out / M,
            side / M,
            f64::from(t) / 60.0
        );
        assert_eq!(crashes, 0, "from {} m out, {} m aside", out / M, side / M);
        w.launch();
        assert_eq!(w.state, world::State::Flying);
    }
}

#[test]
#[ignore = "measuring only: pictures into F1_SHOTS (best with --release)"]
fn f1_pictures() {
    let Some(dir) = std::env::var_os("F1_SHOTS") else {
        println!("F1_SHOTS not set: no pictures");
        return;
    };
    let tag = std::env::var("F1_TAG").unwrap_or_else(|_| "x".into());
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (eye, e) in [(Eye::Chase, "chase"), (Eye::Cockpit, "cockpit")] {
        for dist in [300, 600, 1200, 2400] {
            let mut d = raider_ahead(eye, dist);
            save(&dir, &tag, &mut d, &format!("{e}-raider-{dist}m"));
        }
        let mut d = hauler_beside(eye);
        save(&dir, &tag, &mut d, &format!("{e}-hauler-600m"));
        wreck(&dir, &tag, eye);
    }
    station(&dir, &tag);
    let _ = KM;
}
