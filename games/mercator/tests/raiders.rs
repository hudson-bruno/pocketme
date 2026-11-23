//! Step M1, the raiders: they fire from as far as the player's laser,
//! with guns in three levels by the system's danger; their aim suffers
//! when the player turns; a raider flees only after it has traded fire
//! and comes back when it is not chased; packs grow with the danger and
//! never pile up; a TAG's worth follows the raider's level.
//!
//! The scripted pilots here measure rules; they do not stand for how
//! hard the game is for a person. `cargo test --release --test raiders
//! -- --ignored --nocapture` prints the step's tables (duels, patrols,
//! the cost of a big pack's pictures).

mod common;

use common::*;
use mercator::app::seed_of;
use mercator::fixed::{Mat3, ONE14, V3, length, norm, scale14, sub};
use mercator::galaxy;
use mercator::scene::Detail;
use mercator::world::{
    self, Career, Event, HEAT_MAX, KM, Kind, M, MAX_SPEED, Mode, PLAYER, Pad, State, World,
};

// ---- Set-up -----------------------------------------------------------------------

/// The gun level of ship `i` (0..=2). Step M1's field.
fn set_gun(w: &mut World, i: usize, gun: u8) {
    if let Some(s) = w.ships[i].as_mut() {
        s.gun = gun;
    }
}

/// A system of `danger` in galaxy 1 (not the start system), the player
/// launched and put 20 km out on the station's axis, nothing else in
/// space; gear `laser`, `shield`; `day` varies the dice.
fn world_for(danger: u8, laser: u8, shield: u8, day: u16) -> Box<World> {
    let seed = seed_of(1);
    let start = galaxy::start_system(seed);
    let mut c = Career::new(seed);
    c.system = (0..galaxy::SYSTEMS as u8)
        .find(|&i| i != start && galaxy::system(seed, i).danger == danger)
        .unwrap();
    c.laser = laser;
    c.shield = shield;
    c.day = day;
    let mut w = Box::new(World::empty());
    w.begin(c);
    w.launch();
    let b = w.bodies;
    w.me.pos = world::add(b.station_pos, scale14(b.station_rot.col(2), 20 * KM));
    w.me.rot = b.station_rot;
    w.me.shield = w.shield_max();
    w.ships = [None; world::SHIPS];
    // Different dice per day: tick the world a little.
    for _ in 0..u32::from(day % 64) {
        w.tick(&Pad::default());
        w.ships = [None; world::SHIPS];
    }
    while w.take_event().is_some() {}
    w
}

fn dist_m(a: V3, b: V3) -> i32 {
    length(sub(a, b)) / M
}

// ---- Pilots -----------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pilot {
    /// Aims well and flies straight at the raider at full speed, firing
    /// whenever a shot would hit.
    Straight,
    /// Aims as well, but jinks between shots ([`jink`]): a fifth of a
    /// second at full stick every 0.8 s, then it aims and fires.
    Dodger,
    /// Aims poorly: at where the raider was 0.67 s ago plus a 400 m
    /// wobble, trigger held, 3/4 speed.
    Poor,
    /// Flies straight on at full speed and never fires.
    Passive,
    /// Never fires either, but jinks all the time at full speed: full
    /// stick, a new way (right, up, left, down) every 20 ticks.
    Jinker,
}

const PILOTS: [Pilot; 5] = [
    Pilot::Straight,
    Pilot::Dodger,
    Pilot::Poor,
    Pilot::Passive,
    Pilot::Jinker,
];

/// The pad of `pilot` against a raider at `target` (its past places in
/// `hist`), at tick `t`.
fn pilot_pad(w: &World, pilot: Pilot, target: V3, hist: &[V3], t: u32) -> Pad {
    // The two good aimers never let the laser pass 80 % heat.
    let insight = w.foe_in_sights() && w.me.heat < HEAT_MAX * 8 / 10;
    match pilot {
        Pilot::Straight => Pad {
            steer: steer_to(w, target),
            throttle: 256,
            fire: insight,
            ..Pad::default()
        },
        Pilot::Dodger => Pad {
            steer: jink(t).unwrap_or_else(|| steer_to(w, target)),
            throttle: 256,
            fire: insight,
            ..Pad::default()
        },
        Pilot::Poor => {
            let lag = hist[hist.len().saturating_sub(40)];
            let ph = (t % 180) as i32;
            let tri = if ph < 90 { ph - 45 } else { 135 - ph };
            let wob = scale14(w.me.rot.col(0), tri * 400 * M / 45);
            Pad {
                steer: steer_to(w, world::add(lag, wob)),
                throttle: if w.me.speed > MAX_SPEED * 3 / 4 {
                    -256
                } else {
                    64
                },
                fire: true,
                ..Pad::default()
            }
        }
        Pilot::Passive => Pad {
            throttle: 256,
            ..Pad::default()
        },
        Pilot::Jinker => Pad {
            steer: [(256, 0), (0, 256), (-256, 0), (0, -256)][(t / 20 % 4) as usize],
            throttle: 256,
            ..Pad::default()
        },
    }
}

/// The dodger's stick while it jinks: 12 ticks (0.2 s) of every 48 at
/// full stick, right, up, left and down in turn; None while it aims.
fn jink(t: u32) -> Option<(i32, i32)> {
    (t % 48 < 12).then(|| [(256, 0), (0, 256), (-256, 0), (0, -256)][(t / 48 % 4) as usize])
}

// ---- One raider against one pilot ---------------------------------------------------

#[derive(Default, Debug, Clone, Copy)]
struct Duel {
    /// Metres at the raider's first shot.
    first_shot: Option<i32>,
    shots: u32,
    hits: u32,
    /// Shield and hull points the player lost.
    dmg: i32,
    /// Shots it had fired when it turned to flee, and the metres then.
    fled: Option<(u32, i32)>,
    /// Ticks to the kill.
    killed: Option<u32>,
    /// It got away (fled out of the world's reach).
    escaped: bool,
    died: bool,
}

/// One raider of gun level `gun` (None: the world's own pick) 4 km dead
/// ahead (0 to 240 m to the side by `trial`), `secs` at most.
fn duel(danger: u8, gun: Option<u8>, laser: u8, pilot: Pilot, trial: u32, secs: u32) -> Duel {
    let mut w = world_for(danger, laser, 0, trial as u16);
    w.me.speed = MAX_SPEED;
    w.me.throttle = MAX_SPEED;
    let nose = w.me.rot.col(2);
    let side = scale14(w.me.rot.col(0), (trial % 5) as i32 * 60 * M);
    let p = world::add(world::add(w.me.pos, scale14(nose, 4 * KM)), side);
    let k = w.add_ship(Kind::Raider, p, Mode::Hunt).unwrap();
    if let Some(g) = gun {
        set_gun(&mut w, k, g);
    }
    let mut r = Duel::default();
    let mut hist = Vec::new();
    let mut fled = false;
    for t in 0..secs * 60 {
        let Some(s) = w.ships[k].filter(|s| s.kind == Kind::Raider) else {
            break;
        };
        hist.push(s.pos);
        let d = dist_m(s.pos, w.me.pos);
        let pad = pilot_pad(&w, pilot, s.pos, &hist, t);
        let before = w.me.shield + i32::from(w.car.hull);
        w.tick(&pad);
        r.dmg += (before - w.me.shield - i32::from(w.car.hull)).max(0);
        if w.beams.iter().any(|b| b.foe && b.age == 4) {
            r.shots += 1;
            r.first_shot.get_or_insert(d);
        }
        while let Some(e) = w.take_event() {
            match e {
                Event::Hit => r.hits += 1,
                Event::Killed { kind: Kind::Raider } => r.killed = Some(t),
                _ => {}
            }
        }
        // Keep the duel alone (no spawns, no traffic; the TAG may stay).
        for i in 0..world::SHIPS {
            if i != k && w.ships[i].is_some_and(|s| s.kind != Kind::Canister) {
                w.ships[i] = None;
            }
        }
        match w.ships[k].filter(|s| s.kind == Kind::Raider) {
            Some(s) if s.mode == Mode::Flee && !fled => {
                fled = true;
                r.fled = Some((r.shots, dist_m(s.pos, w.me.pos)));
            }
            Some(_) => {}
            None if r.killed.is_none() => r.escaped = true,
            None => {}
        }
        if w.state == State::Dead {
            r.died = true;
            break;
        }
    }
    r
}

fn mean(v: impl Iterator<Item = f64>) -> f64 {
    let v: Vec<f64> = v.collect();
    if v.is_empty() {
        f64::NAN
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

/// Mean damage the player takes from `trials` duels.
fn duel_damage(danger: u8, gun: u8, laser: u8, pilot: Pilot, trials: u32) -> f64 {
    mean((0..trials).map(|t| f64::from(duel(danger, Some(gun), laser, pilot, t, 40).dmg)))
}

// ---- Rules ------------------------------------------------------------------------

/// A raider opens fire from about the laser's reach (2.4 km), at every
/// danger: before step M1 it held fire until 1.1 km and had turned away
/// by then.
#[test]
fn a_raider_shoots_back_from_the_lasers_reach() {
    for (danger, gun) in [(0, 0), (1, 1), (3, 2)] {
        for t in 0..4 {
            let r = duel(danger, Some(gun), 0, Pilot::Straight, t, 30);
            let at = r.first_shot.expect("the raider fired");
            assert!(at >= 2000, "danger {danger}: first shot at {at} m");
            assert!(r.shots >= 2, "danger {danger}: {} shots", r.shots);
        }
    }
}

/// A level-3 raider is a real threat to a pilot who flies straight at it
/// and fires; jinking pays: a pilot who jinks between shots is hit
/// clearly less often, and one who jinks all the time takes clearly less
/// than one who flies straight on; a level-1 raider in a SECURE system
/// stays gentle.
#[test]
fn a_level_3_raider_hurts_and_jinking_pays() {
    let rate = |pilot| {
        let rs: Vec<Duel> = (0..40).map(|t| duel(3, Some(2), 0, pilot, t, 40)).collect();
        let shots: u32 = rs.iter().map(|r| r.shots).sum();
        let hits: u32 = rs.iter().map(|r| r.hits).sum();
        f64::from(hits) / f64::from(shots.max(1))
    };
    let straight = duel_damage(3, 2, 0, Pilot::Straight, 40);
    let dodger = duel_damage(3, 2, 0, Pilot::Dodger, 40);
    let gentle = duel_damage(0, 0, 0, Pilot::Straight, 40);
    let (hit_s, hit_d) = (rate(Pilot::Straight), rate(Pilot::Dodger));
    // The two that never fire: hit rates, since both may die.
    let (hit_p, hit_j) = (rate(Pilot::Passive), rate(Pilot::Jinker));
    println!(
        "level 3, MK1: straight {straight:.1} (hit {hit_s:.2}), jinking between shots {dodger:.1} \
         (hit {hit_d:.2}); never firing: straight on hit {hit_p:.2}, jinking all the time hit \
         {hit_j:.2}; level 1 at danger 0: {gentle:.1}"
    );
    assert!(
        straight >= 35.0,
        "a level-3 raider costs a straight pilot {straight:.1}"
    );
    assert!(
        dodger <= straight * 0.75,
        "jinking between shots: {dodger:.1} against {straight:.1}"
    );
    assert!(
        hit_d <= hit_s * 0.8,
        "jinking between shots: hit {hit_d:.2} against {hit_s:.2}"
    );
    assert!(
        hit_j <= hit_p * 0.6,
        "jinking: hit {hit_j:.2} against {hit_p:.2}"
    );
    assert!(gentle <= 15.0, "a SECURE raider costs {gentle:.1}");
}

/// A raider flees only after it has fired at least `FLEE_AFTER` shots,
/// never before its first; and a raider the player never hurts never
/// flees.
#[test]
fn a_raider_flees_only_after_trading_fire() {
    let mut fled = 0;
    for danger in 0..4u8 {
        for pilot in PILOTS {
            for t in 0..3 {
                let r = duel(danger, None, 0, pilot, t, 40);
                if let Some((shots, _)) = r.fled {
                    fled += 1;
                    assert!(
                        shots >= u32::from(world::FLEE_AFTER),
                        "{pilot:?} d{danger}: {r:?}"
                    );
                }
                if pilot == Pilot::Passive {
                    assert!(r.fled.is_none(), "an unhurt raider fled: {r:?}");
                }
            }
        }
    }
    println!("{fled} duels with a raider that fled");
}

/// Badly hurt after trading fire, a raider runs flat out (faster than
/// its top speed), and after FLEE_TICKS comes back if the player is not
/// near; a player who stays on its tail keeps it running.
#[test]
fn a_fleeing_raider_runs_and_comes_back_unless_chased() {
    for chase in [false, true] {
        let mut w = world_for(2, 0, 0, 1);
        let p = world::add(w.me.pos, scale14(w.me.rot.col(2), 1500 * M));
        let k = w.add_ship(Kind::Raider, p, Mode::Hunt).unwrap();
        if let Some(s) = w.ships[k].as_mut() {
            s.hp = 10;
            s.shots = world::FLEE_AFTER;
        }
        let mut top = 0;
        let mut back = None;
        for t in 0..60 * 20 {
            // Sitting still, or kept 2 km behind it.
            if chase && let Some(s) = w.ships[k] {
                let behind = norm(sub(w.me.pos, s.pos), 2 * KM);
                w.me.pos = world::add(s.pos, behind);
            }
            w.me.speed = 0;
            w.me.throttle = 0;
            w.tick(&Pad::default());
            w.me.shield = w.shield_max();
            w.car.hull = 100;
            while w.take_event().is_some() {}
            let Some(s) = w.ships[k].filter(|s| s.kind == Kind::Raider) else {
                panic!("the raider left (chase {chase}, tick {t})");
            };
            if t == 0 {
                assert_eq!(s.mode, Mode::Flee, "it flees at once");
            }
            top = top.max(s.speed);
            if back.is_none() && s.mode == Mode::Hunt {
                back = Some(t);
            }
        }
        assert!(top > 1000, "it runs faster than its top speed: {top}");
        if chase {
            assert_eq!(back, None, "a chased raider came back");
        } else {
            let t = back.expect("it came back");
            assert!(
                t >= u32::from(world::FLEE_TICKS) - 2,
                "back after {t} ticks"
            );
        }
    }
}

/// Pack sizes follow the danger (`PACKS`); SECURE systems, the start
/// system among them, send one raider at a time.
#[test]
fn packs_grow_with_the_danger_and_the_start_system_sends_one() {
    let start = galaxy::start_system(seed_of(1));
    for (danger, home) in [(0u8, true), (0, false), (1, false), (2, false), (3, false)] {
        let mut w = world_for(danger, 0, 0, 3);
        if home {
            w.car.system = start;
            w.sys = galaxy::system(w.car.seed, start);
        }
        let (least, more) = world::PACKS[usize::from(danger)];
        let mut sizes = Vec::new();
        for _ in 0..60 * 60 * 120 {
            let before = count(&w, Kind::Raider);
            w.tick(&Pad::default());
            w.me.shield = w.shield_max();
            w.car.hull = 100;
            let mut pack = false;
            while let Some(e) = w.take_event() {
                pack |= e == Event::Raiders;
            }
            if pack {
                sizes.push(count(&w, Kind::Raider) - before);
            }
            // Nothing stays to fight: the next roll is free.
            w.ships = [None; world::SHIPS];
            if sizes.len() >= 40 {
                break;
            }
        }
        println!("danger {danger} (start system {home}): packs {sizes:?}");
        assert!(sizes.len() >= 10, "danger {danger}: {} packs", sizes.len());
        for &n in &sizes {
            if home {
                assert_eq!(n, 1);
            } else {
                assert!(
                    (least..=least + more).contains(&(n as u32)),
                    "danger {danger}: {n}"
                );
            }
        }
        if !home && more > 0 {
            assert!(
                sizes.iter().any(|&n| n as u32 == least + more),
                "never the biggest pack"
            );
        }
    }
}

/// No new pack comes while a raider still presses an attack within 9 km;
/// one that flees does not hold the next pack back.
#[test]
fn no_new_pack_while_one_hunts() {
    for flees in [false, true] {
        let mut w = world_for(3, 0, 0, 2);
        let p = world::add(w.me.pos, scale14(w.me.rot.col(0), 5 * KM));
        let k = w.add_ship(Kind::Raider, p, Mode::Hunt).unwrap();
        w.me.speed = 0;
        let mut packs = 0;
        for _ in 0..60 * 60 * 5 {
            // The one raider stays put, alive, 5 km off; the rest leave.
            let mut keep = w.ships[k].unwrap();
            keep.pos = world::add(w.me.pos, scale14(w.me.rot.col(0), 5 * KM));
            keep.hp = 100;
            keep.mode = if flees { Mode::Flee } else { Mode::Hunt };
            keep.timer = 100;
            keep.cool = 100;
            w.ships = [None; world::SHIPS];
            w.ships[k] = Some(keep);
            w.tick(&Pad::default());
            w.me.shield = w.shield_max();
            w.car.hull = 100;
            while let Some(e) = w.take_event() {
                packs += usize::from(e == Event::Raiders);
            }
        }
        println!("a raider 5 km off that flees: {flees}; packs in 5 min: {packs}");
        if flees {
            assert!(packs >= 3, "{packs} packs");
        } else {
            assert_eq!(packs, 0);
        }
    }
}

/// Gun levels follow `GUN_ODDS`; a TAG is worth the raider's level plus
/// half the danger (in TAG_VALUEs), inside the old 25 to 100.
#[test]
fn guns_follow_the_danger_and_the_tag_follows_the_gun() {
    for danger in 0..4u8 {
        let mut w = world_for(danger, 0, 0, 0);
        let mut seen = [0u32; 3];
        for t in 0..400 {
            let i = w.add_ship(Kind::Raider, w.me.pos, Mode::Hunt).unwrap();
            let g = w.ships[i].unwrap().gun;
            assert!(world::GUN_ODDS[usize::from(danger)].contains(&g));
            seen[usize::from(g)] += 1;
            w.ships[i] = None;
            if t % 3 == 0 {
                w.tick(&Pad::default());
                w.ships = [None; world::SHIPS];
            }
        }
        println!("danger {danger}: gun levels 1/2/3 {seen:?}");
        for g in 0..3u8 {
            let want = world::GUN_ODDS[usize::from(danger)].contains(&g);
            assert_eq!(
                seen[usize::from(g)] > 0,
                want,
                "danger {danger} level {}",
                g + 1
            );
        }
    }
    let worth: Vec<Vec<u8>> = (0..4)
        .map(|d| (0..3).map(|g| world::tag_worth(g, d)).collect())
        .collect();
    assert_eq!(
        worth,
        [[1, 2, 3], [1, 2, 3], [2, 3, 4], [2, 3, 4]].map(|r| r.to_vec())
    );
    // The TAG a level-3 raider leaves in a LAWLESS system: 100 credits.
    let mut w = world_for(3, 2, 0, 0);
    let p = world::add(
        w.me.pos,
        scale14(w.me.rot.col(2), 300 * M * world::FIGHTER_X),
    ); // Step M2: was 300 m.
    let i = w.add_ship(Kind::Raider, p, Mode::Hunt).unwrap();
    set_gun(&mut w, i, 2);
    w.me.speed = 0;
    w.me.throttle = 0;
    for _ in 0..600 {
        if w.ships[i].is_some_and(|s| s.kind != Kind::Raider) {
            break;
        }
        let pad = Pad {
            steer: steer_to(&w, p),
            fire: w.foe_in_sights() && w.me.heat < HEAT_MAX * 8 / 10,
            ..Pad::default()
        };
        w.tick(&pad);
        w.me.shield = w.shield_max();
        w.car.hull = 100;
    }
    let tag = w.ships.iter().flatten().find(|s| s.load.0 == world::TAG);
    assert_eq!(tag.map(|s| s.load.1), Some(4));
}

// ---- Tables (measuring only) --------------------------------------------------------

/// The gun levels a danger uses (all three when `gun` is None in the
/// tables of the code before step M1).
fn levels(danger: u8) -> Vec<Option<u8>> {
    let mut v: Vec<u8> = world::GUN_ODDS[usize::from(danger)].to_vec();
    v.dedup();
    v.into_iter().map(Some).collect()
}

fn duel_table() {
    const N: u32 = 40;
    println!("## Duels: one raider 4 km ahead, {N} trials each, 60 s at most");
    println!(
        "| danger | gun | laser | pilot | raider 1st shot m | shots | hits | damage taken | fled (after shots, at m) | escaped | killed (s) | player died |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
    for danger in 0..4u8 {
        for gun in levels(danger) {
            for laser in [0u8, 2] {
                for pilot in PILOTS {
                    if laser == 2 && matches!(pilot, Pilot::Passive | Pilot::Jinker) {
                        continue;
                    }
                    let rs: Vec<Duel> = (0..N)
                        .map(|t| duel(danger, gun, laser, pilot, t, 60))
                        .collect();
                    let first = mean(rs.iter().filter_map(|r| r.first_shot.map(f64::from)));
                    let fled: Vec<_> = rs.iter().filter_map(|r| r.fled).collect();
                    let killed: Vec<f64> = rs
                        .iter()
                        .filter_map(|r| r.killed.map(|t| f64::from(t) / 60.0))
                        .collect();
                    println!(
                        "| {danger} | {} | MK{} | {pilot:?} | {first:.0} | {:.1} | {:.1} | {:.1} | {} ({:.1}, {:.0}) | {} | {} ({:.1}) | {} |",
                        gun.map_or("-".to_string(), |g| format!("L{}", g + 1)),
                        laser + 1,
                        mean(rs.iter().map(|r| f64::from(r.shots))),
                        mean(rs.iter().map(|r| f64::from(r.hits))),
                        mean(rs.iter().map(|r| f64::from(r.dmg))),
                        fled.len(),
                        mean(fled.iter().map(|f| f64::from(f.0))),
                        mean(fled.iter().map(|f| f64::from(f.1))),
                        rs.iter().filter(|r| r.escaped).count(),
                        killed.len(),
                        mean(killed.iter().copied()),
                        rs.iter().filter(|r| r.died).count(),
                    );
                }
            }
        }
    }
}

/// Five minutes 20 km out in a system of `danger` (the start system when
/// `home`), the spawner and the traffic live: the pilot fights the nearest
/// raider hunting it within 6 km, else flies a slow circle.
/// Returns (died, damage, kills, most hunters within 6 km, packs, biggest
/// pack, seconds flown).
fn patrol(danger: u8, home: bool, laser: u8, pilot: Pilot, trial: u32) -> [u32; 7] {
    let mut w = world_for(danger, laser, laser, trial as u16);
    if home {
        let s = galaxy::start_system(w.car.seed);
        w.car.system = s;
        w.sys = galaxy::system(w.car.seed, s);
    }
    w.me.speed = MAX_SPEED / 2;
    w.me.throttle = MAX_SPEED / 2;
    let (mut dmg, mut kills, mut most, mut packs, mut biggest) = (0, 0, 0, 0, 0);
    let mut hist = Vec::new();
    for t in 0..300 * 60u32 {
        let mut best = None;
        let mut bd = i64::MAX;
        let mut n = 0;
        for (i, s) in w.ships.iter().enumerate() {
            let Some(s) = s else { continue };
            let hunts = s.target == PLAYER && s.mode != Mode::Travel && s.mode != Mode::Flee;
            if !hunts || !world::within(s.pos, w.me.pos, 6 * KM) {
                continue;
            }
            n += 1;
            let e = sub(s.pos, w.me.pos);
            let dd = mercator::fixed::dot(e, e);
            if dd < bd {
                bd = dd;
                best = Some(i);
            }
        }
        most = most.max(n);
        let pad = match best {
            Some(i) => {
                let s = w.ships[i].unwrap();
                hist.push(s.pos);
                // The two good aimers fly as the crate's fighting pilot
                // (half speed, never past 80 % heat); the dodger jinks
                // 20 ticks of every 50 on top.
                match pilot {
                    Pilot::Straight => fight_pad(&w, i),
                    Pilot::Dodger => {
                        let mut p = fight_pad(&w, i);
                        if let Some(j) = jink(t) {
                            p.steer = j;
                            p.fire = false;
                        }
                        p
                    }
                    _ => pilot_pad(&w, pilot, s.pos, &hist, t),
                }
            }
            None => {
                hist.clear();
                Pad {
                    steer: (40, 0),
                    throttle: if w.me.speed > MAX_SPEED / 2 { -256 } else { 64 },
                    ..Pad::default()
                }
            }
        };
        let raiders = count(&w, Kind::Raider);
        let before = w.me.shield + i32::from(w.car.hull);
        w.tick(&pad);
        dmg += (before - w.me.shield - i32::from(w.car.hull)).max(0) as u32;
        while let Some(e) = w.take_event() {
            match e {
                Event::Killed { kind: Kind::Raider } => kills += 1,
                Event::Raiders => {
                    packs += 1;
                    biggest = biggest.max(count(&w, Kind::Raider).saturating_sub(raiders));
                }
                _ => {}
            }
        }
        if w.state == State::Dead {
            return [1, dmg, kills, most as u32, packs, biggest as u32, t / 60];
        }
    }
    [0, dmg, kills, most as u32, packs, biggest as u32, 300]
}

fn patrol_table() {
    const N: u32 = 30;
    println!();
    println!("## Patrols: 5 minutes 20 km out, spawner and traffic live, {N} runs each");
    println!(
        "| danger | gear | pilot | deaths | mean s lived | damage taken | raider kills | most hunters at once (mean/max) | packs | biggest pack |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|");
    for (danger, home) in [(0u8, true), (0, false), (1, false), (2, false), (3, false)] {
        for laser in [0u8, 2] {
            for pilot in [Pilot::Straight, Pilot::Dodger, Pilot::Poor] {
                let rs: Vec<[u32; 7]> = (0..N)
                    .map(|t| patrol(danger, home, laser, pilot, t))
                    .collect();
                let col = |k: usize| mean(rs.iter().map(|r| f64::from(r[k])));
                println!(
                    "| {danger}{} | MK{}/SH{} | {pilot:?} | {}/{N} | {:.0} | {:.0} | {:.1} | {:.1}/{} | {:.1} | {} |",
                    if home { " start" } else { "" },
                    laser + 1,
                    laser + 1,
                    rs.iter().filter(|r| r[0] == 1).count(),
                    col(6),
                    col(1),
                    col(2),
                    col(3),
                    rs.iter().map(|r| r[3]).max().unwrap_or(0),
                    col(4),
                    rs.iter().map(|r| r[5]).max().unwrap_or(0),
                );
            }
        }
    }
}

/// The pictures of a fight with six raiders in LAWLESS space, placed as
/// the spawner places a pack (one more than `PACKS` sends today, the
/// most its two-V layout holds): the worst picture over 30 s at
/// each detail (MEASURED ON THE PC: counts, not time).
fn pack_cost_table() {
    println!();
    println!("## A six-raider pack: worst picture over 30 s");
    println!("| detail | objects | faces | tris | clipped | pixels | dropped |");
    println!("|---|---|---|---|---|---|---|");
    for detail in Detail::ALL {
        let mut d = Driver::new();
        d.start();
        let mut c = Career::new(seed_of(1));
        c.system = (0..galaxy::SYSTEMS as u8)
            .find(|&i| galaxy::system(c.seed, i).danger == 3)
            .unwrap();
        c.target = c.system;
        d.app.world.begin(c);
        d.app.last_dock = c;
        d.app.opts.detail = detail;
        d.idle(2);
        d.app.sel = 0;
        d.tap(game_kit::button::A);
        {
            let w = &mut d.app.world;
            let b = w.bodies;
            w.me.pos = world::add(b.station_pos, scale14(b.station_rot.col(2), 20 * KM));
            w.me.rot = Mat3::looking(b.station_rot.col(0), [0, ONE14, 0]);
            w.ships = [None; world::SHIPS];
            let ahead = w.me.rot.col(2);
            for (k, back) in [(0, 0), (1, 0), (2, 0), (0, 3), (1, 3), (2, 3)] {
                let at = world::add(w.me.pos, scale14(ahead, (4 + back) * KM));
                w.add_ship(
                    Kind::Raider,
                    world::add(at, [k * 300 * M, 0, 0]),
                    Mode::Hunt,
                );
            }
        }
        let mut most = game_kit_stats_default();
        for _ in 0..300 {
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
                w.me.shield = w.shield_max();
                w.car.hull = 100;
                while w.take_event().is_some() {}
            }
            d.idle(1);
            let s = d.app.render.stats;
            if s.pixels + 40 * s.tris > most.pixels + 40 * most.tris {
                most = s;
            }
        }
        println!(
            "| {detail:?} | {} | {} | {} | {} | {} | {} |",
            most.objects, most.faces, most.tris, most.clipped, most.pixels, most.dropped
        );
    }
}

/// Pictures of a five-raider LAWLESS pack as the spawner places it (three
/// ahead at 7 km, two 3 km behind them), High detail, into the folder
/// `RAIDER_SHOTS` names: the approach, the player's first hit taken (a
/// raider's shot from its gun's reach), and every 4 s of the fight.
fn pack_pictures() {
    let Some(dir) = std::env::var_os("RAIDER_SHOTS") else {
        println!("RAIDER_SHOTS not set: no pictures");
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut d = Driver::new();
    d.start();
    let mut c = Career::new(seed_of(1));
    c.system = (0..galaxy::SYSTEMS as u8)
        .find(|&i| galaxy::system(c.seed, i).danger == 3)
        .unwrap();
    c.target = c.system;
    d.app.world.begin(c);
    d.app.last_dock = c;
    d.app.opts.detail = Detail::High;
    d.idle(2);
    d.app.sel = 0;
    d.tap(game_kit::button::A);
    {
        let w = &mut d.app.world;
        let b = w.bodies;
        w.me.pos = world::add(b.station_pos, scale14(b.station_rot.col(2), 20 * KM));
        w.me.rot = Mat3::looking(b.station_rot.col(0), [0, ONE14, 0]);
        w.ships = [None; world::SHIPS];
        let ahead = w.me.rot.col(2);
        for (k, back) in [(0, 0), (1, 0), (2, 0), (0, 3), (1, 3)] {
            let at = world::add(w.me.pos, scale14(ahead, (7 + back) * KM));
            if let Some(i) = w.add_ship(
                Kind::Raider,
                world::add(at, [k * 300 * M, 0, 0]),
                Mode::Hunt,
            ) {
                set_gun(w, i, 2);
            }
        }
    }
    let save = |d: &mut Driver, name: &str| {
        let c = d.app.colours().to_vec();
        save_frame(&dir.join(format!("{name}.png")), &d.frame, &c, 3);
    };
    d.idle(1);
    save(&mut d, "m1-pack-approach-7km");
    let mut hit = false;
    for f in 1..=900u32 {
        for _ in 0..2 {
            let w = &mut d.app.world;
            if w.state != State::Flying {
                break;
            }
            let pad = match hunter(w, 20 * KM) {
                Some(i) => fight_pad(w, i),
                None => Pad::default(),
            };
            w.tick(&pad);
            w.me.shield = w.shield_max();
            w.car.hull = 100;
            while let Some(e) = w.take_event() {
                hit |= e == Event::Hit;
            }
        }
        d.idle(1);
        if hit {
            hit = false;
            let w = &d.app.world;
            let near = w
                .ships
                .iter()
                .flatten()
                .filter(|s| s.kind == Kind::Raider)
                .map(|s| length(sub(s.pos, w.me.pos)) / M)
                .min()
                .unwrap_or(0);
            let name = format!("m1-hit-taken-{f:03}-nearest-{near}m");
            if !dir.join("first-hit").exists() {
                std::fs::write(dir.join("first-hit"), &name).unwrap();
                save(&mut d, &name);
            }
        }
        if f % 120 == 0 {
            save(&mut d, &format!("m1-fight-{:02}s", f / 30));
        }
    }
    let _ = std::fs::remove_file(dir.join("first-hit"));
}

fn game_kit_stats_default() -> mercator::poly::Stats {
    mercator::poly::Stats::default()
}

#[test]
#[ignore = "measuring only: prints the step's tables (best with --release)"]
fn tables() {
    let what = std::env::var("RAIDER_TABLES").unwrap_or_default();
    if what.is_empty() || what.contains("duel") {
        duel_table();
    }
    if what.is_empty() || what.contains("patrol") {
        patrol_table();
    }
    if what.is_empty() || what.contains("cost") {
        pack_cost_table();
    }
    if what.contains("pictures") {
        pack_pictures();
    }
}
