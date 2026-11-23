//! Step F2 measures (ignored, print only; run with `--ignored
//! --nocapture` in release): where a scripted trader's trip time goes,
//! and how the scripted pilots fare against raiders. The same file runs
//! on the game before step M2, after M2, after F1 and after F2, so it
//! uses only what all four have. Scripted pilots are test harnesses,
//! not how hard the game is for a person.

mod common;

use common::*;
use mercator::app::seed_of;
use mercator::galaxy;
use mercator::world::{self, Career, KM, Kind, M, Mode, Pad, State, World};

/// Ticks of one trip, by phase.
#[derive(Default, Clone, Copy)]
struct Phases {
    /// Launch to jump (no fight, no fetch).
    out: u64,
    /// Out to jump, within 6 km of the station.
    out_near: u64,
    /// After the jump, 6 km or more from the station.
    far: u64,
    /// After the jump, 3 to 6 km from the station.
    mid: u64,
    /// After the jump, within 3 km of the station.
    near: u64,
    /// Fighting or fetching a TAG.
    fight: u64,
    trips: u32,
    deaths: u32,
    kills: u32,
    /// Crashes into the station (auto-dock or launch).
    crashes: u32,
    /// Fastest speed within 1.2 km of the station's middle after the
    /// jump, and the speed at the dock (units a tick).
    top_near: i32,
    dock_speed: i32,
}

/// `common::trip`'s pilot, with its ticks sorted into phases.
fn trip_phases(w: &mut World, to: u8, p: &mut Phases) {
    assert_eq!(w.state, State::Docked);
    w.launch();
    let kills0 = u32::from(w.car.kills);
    let mut jumped = to == w.car.system;
    let mut fetch_left = 60 * 120;
    for t in 0..60 * 60 * 10 {
        if w.state == State::Dead {
            p.deaths += 1;
            return;
        }
        if w.state == State::Docked {
            p.trips += 1;
            p.kills += u32::from(w.car.kills) - kills0;
            return;
        }
        let tag =
            tag_near(w, 3 * world::KM).filter(|_| w.car.tags < world::TAGS_MAX && fetch_left > 0);
        let d = w.station_dist();
        let pad = if let Some(i) = hunter(w, 6 * world::KM) {
            p.fight += 1;
            fight_pad(w, i)
        } else if let Some(i) = tag {
            p.fight += 1;
            fetch_left -= 1;
            fetch_pad(w, i)
        } else if !jumped {
            p.out += 1;
            if d < 6 * KM {
                p.out_near += 1;
            }
            if w.can_jump(to).is_ok() {
                w.jump(to);
                jumped = true;
                Pad::default()
            } else {
                Pad {
                    throttle: 256,
                    cruise: !w.me.cruise && w.me.throttle >= world::MAX_SPEED && w.room() > 0,
                    ..Pad::default()
                }
            }
        } else {
            if d >= 6 * KM {
                p.far += 1;
            } else if d >= 3 * KM {
                p.mid += 1;
            } else {
                p.near += 1;
            }
            Pad {
                auto: !w.me.auto && t % 30 == 0,
                ..Pad::default()
            }
        };
        let v = w.me.speed;
        if jumped && d < 1200 * M {
            p.top_near = p.top_near.max(v);
        }
        w.tick(&pad);
        while let Some(e) = w.take_event() {
            match e {
                world::Event::Crash => p.crashes += 1,
                world::Event::Docked => p.dock_speed = p.dock_speed.max(v),
                _ => {}
            }
        }
    }
    panic!("a trip to {to} never ended");
}

fn secs(t: u64, n: u32) -> f64 {
    t as f64 / 60.0 / f64::from(n.max(1))
}

#[test]
#[ignore]
fn f2_trip_phases() {
    let mut all = Phases::default();
    for galaxy in 1..=8u8 {
        let mut w = Box::new(World::empty());
        w.begin(Career::new(seed_of(galaxy)));
        let mut p = Phases::default();
        for _ in 0..12 {
            sell_all(&mut w);
            // Step M5: the cargo ship in place of the big bay.
            if w.car.ship == world::LARK
                && w.ship_sold_here(usize::from(world::DRAY))
                && i64::from(w.car.credits)
                    > 1_100 + i64::from(w.car.ship_cost(usize::from(world::DRAY)))
            {
                w.buy_ship(usize::from(world::DRAY)).unwrap();
            }
            let (to, g, profit) = plan_any(&w, 1);
            if profit > 0 {
                buy_max(&mut w, g, 0);
            }
            let last = w.car;
            trip_phases(&mut w, to, &mut p);
            if w.state != State::Docked {
                w.begin(last.wrecked());
            }
        }
        println!(
            "galaxy {galaxy}: {} trips, {} lost | s a trip: out {:.1} (within 6 km {:.1}) | in: far {:.1}, 3-6 km {:.1}, <3 km {:.1} | fights {:.1} | total {:.1}",
            p.trips,
            p.deaths,
            secs(p.out, p.trips),
            secs(p.out_near, p.trips),
            secs(p.far, p.trips),
            secs(p.mid, p.trips),
            secs(p.near, p.trips),
            secs(p.fight, p.trips),
            secs(p.out + p.far + p.mid + p.near + p.fight, p.trips)
        );
        all.out += p.out;
        all.out_near += p.out_near;
        all.far += p.far;
        all.mid += p.mid;
        all.near += p.near;
        all.fight += p.fight;
        all.trips += p.trips;
        all.deaths += p.deaths;
        all.kills += p.kills;
        all.crashes += p.crashes;
        all.top_near = all.top_near.max(p.top_near);
        all.dock_speed = all.dock_speed.max(p.dock_speed);
    }
    let n = all.trips;
    println!(
        "ALL: {n} trips, {} lost, {} kills, {} crashes, top speed within 1.2 km {}, at the dock at most {} | s a trip: out {:.1} (within 6 km {:.1}) | in: far {:.1}, 3-6 km {:.1}, <3 km {:.1} | fights {:.1} | total {:.1} | round the station (out within 6 km + in within 6 km) {:.1}",
        all.deaths,
        all.kills,
        all.crashes,
        all.top_near,
        all.dock_speed,
        secs(all.out, n),
        secs(all.out_near, n),
        secs(all.far, n),
        secs(all.mid, n),
        secs(all.near, n),
        secs(all.fight, n),
        secs(all.out + all.far + all.mid + all.near + all.fight, n),
        secs(all.out_near + all.mid + all.near, n)
    );
}

/// Docked at the first system of `danger` in galaxy `galaxy`.
fn world_in(galaxy: u8, danger: u8) -> Box<World> {
    let mut c = Career::new(seed_of(galaxy));
    c.system = (0..galaxy::SYSTEMS as u8)
        .find(|&i| galaxy::system(c.seed, i).danger == danger)
        .unwrap();
    let mut w = Box::new(World::empty());
    w.begin(c);
    w
}

/// A raider pack 2 km and more ahead, 12 km out of the station
/// (`play::ambush`'s set-up).
fn ambush(w: &mut World, raiders: usize) {
    w.launch();
    w.me.pos = world::add(w.bodies.station_pos, [0, 0, 0]);
    let out = w.bodies.station_rot.col(2);
    w.me.pos = world::add(w.me.pos, mercator::fixed::scale14(out, 12 * KM));
    for k in 0..raiders {
        let p = world::add(
            w.me.pos,
            mercator::fixed::scale14(w.me.rot.col(2), (2 + k as i32) * KM),
        );
        w.add_ship(
            Kind::Raider,
            world::add(p, [k as i32 * 300 * M, 0, 0]),
            Mode::Hunt,
        );
    }
}

#[derive(Default)]
struct Score {
    fights: u32,
    won: u32,
    lost: u32,
    kills: u32,
    ticks: u64,
    shots: u32,
    hits: u32,
    /// Shots and hits at a raider nearer than 1 km.
    near_shots: u32,
    near_hits: u32,
}

/// One fight. `k` 0: the pilot fires on the game's own lock
/// (`foe_in_sights`, the scripted pilots' rule). `k` > 0: an unsteady
/// pilot, whose aim circles round the raider's middle at 1/`wob` of
/// the distance (as a hand on a stick might), fires whenever that
/// middle is within 1/`k` of the distance of its nose line, lock or
/// not: its share of hits shows how big the raider is to the laser
/// (96 px focal length: 1/20 is about 5 px, 1/12 about 8 px).
fn fight(galaxy: u8, danger: u8, raiders: usize, laser: u8, k: i64, wob: f64, s: &mut Score) {
    let mut w = world_in(galaxy, danger);
    w.car.laser = laser;
    ambush(&mut w, raiders);
    s.fights += 1;
    for _ in 0..60 * 90 {
        if w.state == State::Dead {
            s.lost += 1;
            return;
        }
        let Some(i) = hunter(&w, 20 * KM) else {
            s.won += 1;
            return;
        };
        let mut pad = fight_pad(&w, i);
        if wob > 0.0 {
            let t = w.ships[i].unwrap();
            let e = mercator::fixed::sub(t.pos, w.me.pos);
            let d = (mercator::fixed::dot(e, e) as f64).sqrt() / wob;
            let a = s.ticks as f64 / 60.0 * 4.7;
            let (x, y) = ((d * a.cos()) as i32, (d * (a * 1.37).sin()) as i32);
            let off = world::add(
                mercator::fixed::scale14(w.me.rot.col(0), x),
                mercator::fixed::scale14(w.me.rot.col(1), y),
            );
            pad.steer = steer_to(&w, world::add(t.pos, off));
        }
        if k > 0 {
            let t = w.ships[i].unwrap();
            let l = w.me.rot.apply_t(mercator::fixed::sub(t.pos, w.me.pos));
            let side = i64::from(l[0]).pow(2) + i64::from(l[1]).pow(2);
            let cone = i64::from(l[2]) / k;
            pad.fire = l[2] > 0
                && w.in_reach(t.pos)
                && side <= cone * cone
                && w.me.heat < world::HEAT_MAX * 8 / 10;
        }
        let near = world::within(w.ships[i].unwrap().pos, w.me.pos, KM);
        let before: Vec<Option<(Kind, i32)>> =
            w.ships.iter().map(|o| o.map(|x| (x.kind, x.hp))).collect();
        w.tick(&pad);
        s.ticks += 1;
        if w.me.firing == 4 {
            s.shots += 1;
            let hit = before.iter().zip(w.ships.iter()).any(|(b, a)| match b {
                Some((Kind::Raider, hp)) => match a {
                    Some(x) if x.kind == Kind::Raider => x.hp < *hp,
                    _ => true,
                },
                _ => false,
            });
            if hit {
                s.hits += 1;
            }
            if near {
                s.near_shots += 1;
                s.near_hits += u32::from(hit);
            }
        }
        while let Some(e) = w.take_event() {
            if e == (world::Event::Killed { kind: Kind::Raider }) {
                s.kills += 1;
            }
        }
    }
}

#[test]
#[ignore]
fn f2_fights() {
    for (name, k, wob) in [
        ("lock pilot (fires on the game's lock)", 0i64, 0.0),
        (
            "unsteady pilot A (aim circles 5 px off, fires within 10 px)",
            10,
            20.0,
        ),
        (
            "unsteady pilot B (aim circles 8 px off, fires within 16 px)",
            6,
            12.0,
        ),
    ] {
        for laser in [0u8, 2] {
            let mut s = Score::default();
            for galaxy in 1..=8u8 {
                for danger in 1..=3u8 {
                    for raiders in 1..=3usize {
                        fight(galaxy, danger, raiders, laser, k, wob, &mut s);
                    }
                }
            }
            println!(
                "{name}, laser {laser}: {} fights, won {} lost {} out of time {}, {} kills, {:.2} kills a fight, {:.1} s a kill, {} shots {} hits, {:.1} % hit ({:.1} % of {} nearer than 1 km), {:.1} s fought",
                s.fights,
                s.won,
                s.lost,
                s.fights - s.won - s.lost,
                s.kills,
                f64::from(s.kills) / f64::from(s.fights),
                s.ticks as f64 / 60.0 / f64::from(s.kills.max(1)),
                s.shots,
                s.hits,
                100.0 * f64::from(s.hits) / f64::from(s.shots.max(1)),
                100.0 * f64::from(s.near_hits) / f64::from(s.near_shots.max(1)),
                s.near_shots,
                s.ticks as f64 / 60.0
            );
        }
    }
}
