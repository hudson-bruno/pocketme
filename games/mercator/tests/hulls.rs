//! Step L3: a second hull per role, sizes and markings per ship, and
//! each system's own trader and pirate colours. Pictures go to
//! `.local/mercator/shots/` (names `60-hulls-*`).

mod common;

use common::*;
use game_kit::button;
use mercator::app::{Scene, seed_of};
use mercator::fixed::{Mat3, ONE14, scale14, sub};
use mercator::galaxy;
use mercator::scene::{self, Detail};
use mercator::world::{self, Career, KM, Kind, M, Mode};

/// Flying out of system `sys` of galaxy 1, still, 20 km out.
fn out_at(sys: u8) -> Driver {
    let mut d = Driver::new();
    d.start();
    let mut c = Career::new(seed_of(1));
    c.system = sys;
    c.target = sys;
    d.app.world.begin(c);
    d.app.last_dock = c;
    d.idle(2);
    assert_eq!(d.app.scene, Scene::Docked);
    d.app.sel = 0;
    d.tap(button::A);
    d.app.opts.detail = Detail::High;
    d
}

/// Ships of `line` (kind, look) in a row `dist` metres ahead and
/// below, `2 * gap` metres apart, turned three quarters towards the eye; one picture.
fn line_up(
    d: &mut Driver,
    line: &[(Kind, u8)],
    (dist, gap): (i32, i32),
    name: &str,
) -> mercator::poly::Stats {
    let w = &mut d.app.world;
    let b = w.bodies;
    let pos = world::add(b.station_pos, scale14(b.station_rot.col(2), 20 * KM));
    let ahead = world::add(pos, scale14(b.station_rot.col(0), dist * M));
    w.me.pos = pos;
    w.me.rot = Mat3::looking(sub(ahead, pos), [0, ONE14, 0]);
    w.me.speed = 0;
    w.me.throttle = 0;
    w.ships = [None; world::SHIPS];
    let right = w.me.rot.col(0);
    let below = scale14(w.me.rot.col(1), -dist / 7 * M);
    let n = line.len() as i32;
    for (k, &(kind, look)) in line.iter().enumerate() {
        let p = world::add(ahead, scale14(right, (2 * k as i32 - n + 1) * gap * M));
        let p = world::add(p, below);
        let i = w.add_ship(kind, p, Mode::Travel).unwrap();
        let s = w.ships[i].as_mut().unwrap();
        s.rot = Mat3::looking(
            world::add(sub(pos, p), scale14(right, dist / 3 * M)),
            [0, ONE14, 0],
        );
        s.speed = 0;
        s.target = world::NOBODY;
        s.look = look;
    }
    d.idle(1);
    shot(d, name);
    d.app.render.stats
}

/// Bits 0-1 of `look`: 0 always gives the second hull, 3 never.
const ALT: u8 = 0;
const FIRST: u8 = 3;

#[test]
fn each_role_has_two_hulls() {
    let mut d = out_at(0);
    let all = [
        (Kind::Raider, FIRST),
        (Kind::Raider, ALT),
        (Kind::Hauler, FIRST),
        (Kind::Hauler, ALT),
        (Kind::Warden, FIRST),
        (Kind::Warden, ALT),
    ];
    let mut most = 0;
    for (k, pair) in all.chunks(2).enumerate() {
        let s = line_up(&mut d, pair, (230, 70), &format!("60-hulls-{k}"));
        println!("a pair of hulls: {} triangles, {} pixels", s.tris, s.pixels);
        most = most.max(s.tris);
    }
    let s = line_up(&mut d, &all, (900, 60), "60-hulls-all");
    println!("six hulls: {} triangles, {} pixels", s.tris, s.pixels);
    let h = d.app.world.sys.h;
    for &(kind, _) in &all[..] {
        let mut a = d
            .app
            .world
            .ships
            .iter()
            .flatten()
            .filter(|s| s.kind == kind);
        let (one, two) = (a.next().unwrap(), a.next().unwrap());
        assert!(!core::ptr::eq(
            scene::model_of(one, h),
            scene::model_of(two, h)
        ));
    }
}

#[test]
fn sizes_and_markings_vary() {
    let mut d = out_at(0);
    // Size steps 0..3 (bits 2-3), markings 0..3 (bits 4-5).
    let line: Vec<(Kind, u8)> = (0..4u8)
        .map(|k| (Kind::Hauler, FIRST | k << 2 | k << 4))
        .chain((0..2u8).map(|k| (Kind::Raider, FIRST | (3 * k) << 2 | (2 + k) << 4)))
        .collect();
    let s = line_up(&mut d, &line, (650, 70), "61-hulls-sizes-marks");
    println!("sizes and marks: {} triangles, {} pixels", s.tris, s.pixels);
}

#[test]
fn systems_wear_their_own_colours() {
    // A system for each of the four trader liveries.
    let seed = seed_of(1);
    let mut seen = [false; 4];
    for i in 0..galaxy::SYSTEMS as u8 {
        let k = ((galaxy::system(seed, i).h >> 4) % 4) as usize;
        if seen[k] {
            continue;
        }
        seen[k] = true;
        let mut d = out_at(i);
        let line = [
            (Kind::Hauler, FIRST),
            (Kind::Hauler, ALT),
            (Kind::Raider, FIRST),
            (Kind::Raider, ALT),
        ];
        line_up(&mut d, &line, (420, 70), &format!("62-hulls-livery-{k}"));
    }
    assert!(seen.iter().all(|&s| s), "{seen:?}");
}

/// Over a whole system, about the system's share of ships take the
/// second hull: never none, never all (with a few dozen ships).
#[test]
fn the_second_hull_share_follows_the_system() {
    let seed = seed_of(1);
    for i in 0..8u8 {
        let h = galaxy::system(seed, i).h;
        let share = (h >> 8) % 3 + 1;
        let alt = (0..=255u8)
            .filter(|&look| {
                let mut s = world::Ship {
                    kind: Kind::Raider,
                    pos: [0; 3],
                    rot: Mat3::IDENTITY,
                    speed: 0,
                    hp: 1,
                    mode: Mode::Travel,
                    timer: 0,
                    target: world::NOBODY,
                    cool: 0,
                    load: (0, 0),
                    angry: false,
                    flash: 0,
                    lead: world::NOBODY,
                    wing: 0,
                    look: 0,
                    gun: 0,
                    shots: 0,
                };
                s.look = look;
                core::ptr::eq(scene::model_of(&s, h), &mercator::models::HOOK)
            })
            .count() as u32;
        assert_eq!(alt, 64 * share, "system {i}");
    }
}
