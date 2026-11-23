//! The sense of speed in the two views (2026-11-23). The space dust
//! (`scene::dust`) is what shows the ship's own speed: specks fixed in
//! space that rush past and streak along the way the ship goes. The
//! same flight is measured from the cockpit eye and from the chase eye:
//! how many specks are in the view, how far a speck moves across the
//! picture from one step to the next, and how long its streak is. The
//! chase view must stay within a band of the cockpit's (see [`BAND`]).
//!
//! `cargo test --release --test speed -- --nocapture` prints the table.

mod common;

use std::collections::HashMap;

use common::*;
use game_kit::button;
use mercator::app::Scene;
use mercator::fixed::{Mat3, ONE14, V3, scale14};
use mercator::scene::{self, Detail, Eye, VIEW};
use mercator::world::{self, CRUISE_MAX, KM, M, MAX_SPEED, OD_TOP};

/// Chase over cockpit, as a fraction (low, high) that each measure must
/// stay inside. A third either way: the old chase view (before
/// 2026-11-23) showed about twice the cockpit's specks, moving at about
/// a sixth of its pace (x2.0 to x2.3 and x0.15 to x0.17 in this table),
/// far outside; two views of the same flight drawn from different
/// places never sample the very same specks, so they cannot match
/// exactly (the table shows how close they come).
const BAND: (f64, f64) = (0.75, 4.0 / 3.0);

/// Steps flown at each speed (one step is one tick, a picture at 60 a
/// second): at least this many, and enough to fly 6 km.
const STEPS: u32 = 600;

/// The speeds flown (world units a tick; 256 = 1 m): still, slow, half
/// and full top speed, the LARK's overboost, and cruise at 3 and
/// 15 km/s.
const SPEEDS: [(&str, i32); 7] = [
    ("still", 0),
    ("75 m/s", MAX_SPEED / 4),
    ("150 m/s", MAX_SPEED / 2),
    ("300 m/s (top)", MAX_SPEED),
    ("525 m/s (overboost)", OD_TOP),
    ("3 km/s (cruise)", 12_800),
    ("15 km/s (cruise top)", CRUISE_MAX),
];

/// In flight in the start system, 20 km out on the station's axis,
/// nothing else in space, the nose and the wings askew to the dust's
/// grid (so a straight flight crosses its cells at every offset).
fn out_far(detail: Detail) -> Driver {
    let mut d = Driver::new();
    d.start();
    d.app.opts.detail = detail;
    d.idle(2);
    d.app.sel = 0;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Flight);
    let w = &mut d.app.world;
    let b = w.bodies;
    w.me.pos = world::add(b.station_pos, scale14(b.station_rot.col(2), 20 * KM));
    w.me.rot = Mat3::looking([5_000, 3_000, 7_000], [1_000, ONE14, 2_000]);
    w.ships = [None; world::SHIPS];
    d
}

#[derive(Default, Clone, Copy)]
struct Sense {
    /// Specks whose head is in the view, a step on average.
    specks: f64,
    /// Pixels a speck's head moves from one step to the next (specks in
    /// the view at both steps), on average.
    moved: f64,
    /// How many such pairs there were.
    pairs: u32,
    /// A streak's length in pixels (specks in the view), on average.
    streak: f64,
}

fn inside(p: [i32; 2]) -> bool {
    let a = VIEW.area;
    (a.x0..a.x1).contains(&p[0]) && (a.y0..a.y1).contains(&p[1])
}

fn dist(a: [i32; 2], b: [i32; 2]) -> f64 {
    f64::from(a[0] - b[0]).hypot(f64::from(a[1] - b[1]))
}

/// One flight straight ahead at `speed`, seen from both eyes at every
/// step: (cockpit, chase).
fn fly(detail: Detail, speed: i32) -> (Sense, Sense) {
    let mut d = out_far(detail);
    let w = &mut d.app.world;
    w.me.speed = speed;
    w.me.throttle = speed.min(MAX_SPEED);
    let ahead = w.me.rot.col(2);
    let side = w.me.rot.col(0);
    let steps = if speed == 0 {
        STEPS
    } else {
        STEPS.max((6 * KM / speed) as u32)
    };
    let mut last: [HashMap<V3, [i32; 2]>; 2] = [HashMap::new(), HashMap::new()];
    let mut sum = [(0u64, 0f64, 0u32, 0f64, 0u64); 2];
    for _ in 0..steps {
        // The ship moves as `World::tick` moves it (only the dust is
        // measured, so nothing else needs to run). Still, it hops 97 m
        // sideways each step instead, so the count of specks is taken
        // in many places, not in one (nothing moves then: `last` is
        // cleared).
        if speed == 0 {
            w.me.pos = world::add(w.me.pos, scale14(side, 97 * M));
            last = [HashMap::new(), HashMap::new()];
        } else {
            w.me.pos = world::add(w.me.pos, scale14(ahead, speed));
        }
        for (k, eye) in [Eye::Cockpit, Eye::Chase].into_iter().enumerate() {
            let cam = scene::camera(w, eye);
            let mut now = HashMap::new();
            let s = &mut sum[k];
            scene::dust_specks(&VIEW, &cam, w, detail, |p, head, tail| {
                if inside(head) {
                    s.0 += 1;
                    s.3 += dist(head, tail);
                    if let Some(&was) = last[k].get(&p) {
                        s.1 += dist(head, was);
                        s.2 += 1;
                    }
                    now.insert(p, head);
                }
            });
            s.4 += 1;
            last[k] = now;
        }
    }
    let sense = |s: (u64, f64, u32, f64, u64)| Sense {
        specks: s.0 as f64 / s.4 as f64,
        moved: s.1 / f64::from(s.2.max(1)),
        pairs: s.2,
        streak: s.3 / (s.0.max(1)) as f64,
    };
    (sense(sum[0]), sense(sum[1]))
}

#[test]
fn the_chase_view_shows_speed_as_the_cockpit_does() {
    let mut bad = Vec::new();
    println!(
        "{:<6} {:<22} {:>22} {:>22} {:>22}",
        "detail", "speed", "specks (cock / chase)", "moved px", "streak px"
    );
    // LOW draws no dust in either view.
    for detail in [Detail::Mid, Detail::High] {
        for (name, speed) in SPEEDS {
            let (c, e) = fly(detail, speed);
            println!(
                "{:<6} {:<22} {:>10.1} / {:<10.1} {:>10.2} / {:<10.2} {:>10.2} / {:<10.2}",
                detail.name(),
                name,
                c.specks,
                e.specks,
                c.moved,
                e.moved,
                c.streak,
                e.streak
            );
            let mut check = |what: &str, a: f64, b: f64| {
                let r = b / a;
                if !(BAND.0..=BAND.1).contains(&r) {
                    bad.push(format!(
                        "{} {name} {what}: chase {b:.2}, cockpit {a:.2} (x{r:.2})",
                        detail.name()
                    ));
                }
            };
            check("specks", c.specks, e.specks);
            if speed == 0 {
                // Still: nothing moves and nothing streaks, in both.
                assert_eq!((c.moved, c.streak), (0.0, 0.0), "{name}: cockpit");
                assert_eq!((e.moved, e.streak), (0.0, 0.0), "{name}: chase");
                continue;
            }
            check("streak", c.streak, e.streak);
            // Above about 64 m a step a speck is in the view for one
            // picture at most, in both views: no movement to compare.
            if c.pairs >= 200 && e.pairs >= 200 {
                check("moved", c.moved, e.moved);
            } else {
                println!("       (too few specks seen twice to compare their movement)");
            }
        }
    }
    assert!(bad.is_empty(), "outside the band:\n{}", bad.join("\n"));
}
