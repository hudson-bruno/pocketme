//! Step G3: MERCATOR out of the ship (SPEC 3.6 without drones and the
//! scan): the walk over many seeds, the cutter's stages and heat, the
//! chunks' tonnes and the hold, a boulder cut loose on a slope, the
//! clock and the storm by danger, the suit, the write-back on take-off,
//! PILOT DOWN, what was cut across a jump, the cost of a picture on foot
//! and a scripted visit over many sites.
//!
//! The scripted pilots here measure the RULES (what a straight-line
//! walker with a fixed plan gets), never a human's skill or difficulty.
//!
//! `FOOT_SEEDS` sets the number of sites the walk and visit tests use
//! (default 100).

mod common;

use common::below::{below_in, land, system_with_look};
use common::{Driver, save_frame, shots};
use game_kit::button;
use mercator::app::{Event, Scene, seed_of};
use mercator::fixed::{cos, sin};
use mercator::galaxy::{self, DANGER_NAMES, SYSTEMS, System};
use mercator::market::{self, GOODS};
use mercator::planet::cutter::{self, ALL_TAKEN, Lock, WRECK_BIT};
use mercator::planet::lander::{self, In, MPS, Phase, TO_WORLD};
use mercator::planet::loose::{self, CHUNK_NONE, ROCK_REST, ROCK_ROLL, ROCK_STOPPED};
use mercator::planet::tune::{
    BOARD_TICKS, BOULDER_LIFE, BOULDER_LOOSE, CUT_HEAT_DOWN, CUT_HEAT_MAX, CUT_HEAT_UP, CUT_RESUME,
    DRONE_TONNES, HATCH_OPEN, HOP_ENERGY, LIFT_FUEL, LIFT_HOLD, PILOT_R, SEAM_STAGES, SEAM_TONNES,
    SEAMS, SHIP_SOLID, SITE_TIME, STAGE_TIME, STORM_CLOSE, STORM_DRAIN, STORM_STEPS, STORM_WARN,
    SUIT_MAX, SUIT_REFILL, TAKEOFF_TICKS, WALK_EDGE, WRECK_STAGES, WRECK_TONNES,
};
use mercator::planet::{self, Below, Outcome, clock, drones, place, site, space, walker};
use mercator::scene::Detail;
use mercator::world::{CRAFTS, M};

/// The looks with a surface (all but the giant).
const SURFACES: [u8; 7] = [0, 1, 2, 3, 5, 6, 7];

fn seeds() -> usize {
    std::env::var("FOOT_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100)
}

/// The first `n` systems with a surface, galaxy by galaxy (real sites:
/// their look, danger and seed are the game's).
fn sites(n: usize) -> Vec<(u32, System)> {
    let mut out = Vec::new();
    for g in 1..=60u8 {
        let seed = seed_of(g);
        for i in 0..SYSTEMS as u8 {
            let s = galaxy::system(seed, i);
            if s.look != 4 {
                out.push((seed, s));
                if out.len() == n {
                    return out;
                }
            }
        }
    }
    out
}

/// A site of system `sys` and the ship landed on its pad, as `begin`
/// makes it (without the world): ship 0, empty, gravity `gc`, laser and
/// shield levels.
fn landed(sys: &System, gc: i32, laser: u8, shield: u8) -> Below {
    let mut s = site::make(sys.h, sys.look).expect("a site");
    let places = place::place(&mut s, sys.danger);
    let mut b = Below {
        on: true,
        site: s,
        places,
        axes: [[16384, 0, 0], [0, 16384, 0], [0, 0, 16384]],
        sun: [0, 11_585, 11_585],
        station: [0, 16384, 0],
        star: sys.star,
        room: u16::from(CRAFTS[0].hold),
        danger: sys.danger,
        laser,
        shield,
        ..Below::EMPTY
    };
    planet::populate(&mut b, 0);
    let hull = CRAFTS[0].hull;
    planet::ready(&mut b, 0, 0, gc, (hull, hull), 10);
    b.phase = Phase::Landed;
    b.phase_t = 0;
    b.p = [0, b.site.pad << TO_WORLD, 0];
    b.v = [0; 3];
    b
}

/// Waits on the ground for the hatch: the pilot steps out.
fn step_out(b: &mut Below) {
    for _ in 0..=HATCH_OPEN {
        assert_eq!(planet::tick(b, &In::default()), Outcome::Stay);
    }
    assert_eq!(b.phase, Phase::Foot, "the hatch opened");
}

/// The stick (camera frame) that walks toward (dx, dz) at `k`/256.
fn stick(b: &Below, dx: i64, dz: i64, k: i32) -> [i32; 2] {
    let d = ((dx * dx + dz * dz) as f64).sqrt();
    if d < 1.0 {
        return [0, 0];
    }
    let (ux, uz) = (dx as f64 / d, dz as f64 / d);
    let (s, c) = (
        f64::from(sin(b.cam_yaw)) / 16384.0,
        f64::from(cos(b.cam_yaw)) / 16384.0,
    );
    let k = f64::from(k);
    [
        ((ux * c - uz * s) * k).round() as i32,
        ((ux * s + uz * c) * k).round() as i32,
    ]
}

fn input(held: u32, pressed: u32, push: [i32; 2]) -> In {
    In {
        held,
        pressed,
        push,
        ..In::default()
    }
}

/// The rules every tick on foot keeps: never under the ground, never
/// past the walk's edge, never inside the ship, the hold never over.
fn keeps_the_rules(b: &Below, what: &str) {
    if !planet::on_foot(b) {
        return;
    }
    assert!(
        walker::under(b) <= 0,
        "{what}: the pilot sank {}",
        walker::under(b)
    );
    let p = b.pilot.at();
    let out = walker::apart([p[0], p[2]], [0, 0]);
    assert!(out <= WALK_EDGE + 1, "{what}: {} m out", out / M);
    let ship = lander::at(b);
    let gap = walker::apart([p[0], p[2]], [ship[0], ship[2]]);
    assert!(
        gap >= SHIP_SOLID + PILOT_R - 2,
        "{what}: inside the ship ({gap})"
    );
    assert!(
        b.cargo.total() <= u32::from(b.room),
        "{what}: the hold overflowed"
    );
    // Step G4: every drone stays on its leash.
    for d in &b.drones {
        if d.alive() {
            let home = b.places.seams[usize::from(d.home) % SEAMS];
            let far = walker::apart([d.p[0], d.p[2]], home);
            assert!(
                far <= mercator::planet::tune::DRONE_LEASH,
                "{what}: a drone {far} from its seam"
            );
        }
    }
}

// ---- The walk ----------------------------------------------------------------------

/// A random walker (the stick turned every second, jumps, jets held,
/// runs at the walk's edge) never leaves the site and never sinks into
/// the ground, over many sites of every look.
#[test]
fn walking_never_leaves_the_site_and_never_sinks() {
    let list = sites(seeds());
    let mut far = 0;
    let mut hops = 0u32;
    for (n, (_, sys)) in list.iter().enumerate() {
        let mut b = landed(sys, 400 + (n as i32 * 37) % 501, 0, 2);
        step_out(&mut b);
        let mut push = [0, 0];
        let mut held = 0;
        for t in 0..2400u32 {
            let h = mercator::fixed::hash(t / 60, n as u32);
            if t % 60 == 0 {
                // Mostly outward (to test the edge), sometimes anywhere.
                let p = b.pilot.at();
                push = if h.is_multiple_of(3) {
                    stick(&b, i64::from(p[0]), i64::from(p[2]), 256)
                } else {
                    [(h & 0x1FF) as i32 - 256, ((h >> 9) & 0x1FF) as i32 - 256]
                };
            }
            let was = held;
            held = match h >> 28 {
                0..=1 => button::A,
                2 if t % 60 < 3 => button::A,
                _ => held & !button::A,
            };
            if held & !was & button::A != 0 {
                hops += 1;
            }
            // A long run with the storm far away: the suit is the test's.
            b.since = 0;
            b.pilot.suit = i32::from(b.pilot.suit_max) * 60;
            let out = planet::tick(&mut b, &input(held, held & !was, push));
            assert_eq!(out, Outcome::Stay, "site {n}");
            keeps_the_rules(&b, &format!("site {n} tick {t}"));
            let p = b.pilot.at();
            far = far.max(walker::apart([p[0], p[2]], [0, 0]));
        }
        // Then straight out from 170 m for 20 s, hopping now and then:
        // the walk's edge holds him.
        let p = b.pilot.at();
        let d = (walker::apart([p[0], p[2]], [0, 0]) / M).max(1);
        let (x, z) = (p[0] * 170 / d, p[2] * 170 / d);
        b.pilot.p = [
            x << TO_WORLD,
            walker::ground(&b, x << TO_WORLD, z << TO_WORLD),
            z << TO_WORLD,
        ];
        b.pilot.v = [0; 3];
        b.pilot.ground = true;
        for t in 0..1200u32 {
            let p = b.pilot.at();
            let push = stick(&b, i64::from(p[0]), i64::from(p[2]), 256);
            let a = if t % 90 < 50 { button::A } else { 0 };
            let pressed = if t % 90 == 0 { button::A } else { 0 };
            b.since = 0;
            b.pilot.suit = i32::from(b.pilot.suit_max) * 60;
            let out = planet::tick(&mut b, &input(a, pressed, push));
            assert_eq!(out, Outcome::Stay, "site {n}");
            keeps_the_rules(&b, &format!("site {n} out tick {t}"));
            let p = b.pilot.at();
            far = far.max(walker::apart([p[0], p[2]], [0, 0]));
        }
    }
    println!(
        "{} sites x 2,400 ticks random and 1,200 straight out from 170 m: farthest {} m (edge {} m), {} jumps",
        list.len(),
        far / M,
        WALK_EDGE / M,
        hops
    );
    assert!(far > WALK_EDGE - 5 * M, "the walkers reached the edge");
}

/// A fall over 12 m/s costs 4 points a m/s over it, and the jets lift
/// him while their energy lasts, and fill at the hatch.
#[test]
fn falls_cost_suit_and_the_jets_run_dry() {
    let (_, sys) = sites(1)[0];
    let mut b = landed(&sys, 650, 0, 0);
    step_out(&mut b);
    let full = b.pilot.suit;
    // 30 m up, straight down.
    b.pilot.p[1] += (30 * M) << TO_WORLD;
    b.pilot.ground = false;
    let mut v = 0;
    for _ in 0..600 {
        v = b.pilot.v[1];
        planet::tick(&mut b, &In::default());
        if b.pilot.ground {
            break;
        }
    }
    assert!(b.pilot.ground);
    let impact = f64::from(-v) / f64::from(MPS);
    let lost = f64::from(full - b.pilot.suit) / 60.0;
    let want = (impact - 12.0) * 4.0;
    assert!(
        (lost - want).abs() < 2.0,
        "a fall at {impact:.1} m/s cost {lost:.1}, rule {want:.1}"
    );
    // The jets (away from the hatch, which fills them): A pressed jumps,
    // held on it climbs, then runs dry.
    b.pilot.suit = full;
    let away = b.pilot.p;
    let x = away[0] + ((15 * M) << TO_WORLD);
    b.pilot.p = [x, walker::ground(&b, x, away[2]), away[2]];
    b.pilot.v = [0; 3];
    let ground_y = b.pilot.p[1];
    let mut top = 0;
    let mut dry = None;
    for t in 0..600u32 {
        let pressed = if t == 0 { button::A } else { 0 };
        planet::tick(&mut b, &input(button::A, pressed, [0, 0]));
        top = top.max(b.pilot.p[1] - ground_y);
        if b.pilot.energy == 0 && dry.is_none() {
            dry = Some(t);
        }
    }
    let dry = dry.expect("the jets ran dry");
    println!(
        "fall {impact:.1} m/s cost {lost:.1} points; jets dry after {dry} ticks, highest {:.1} m",
        f64::from(top) / f64::from(lander::FINE)
    );
    assert!(top > 3 * lander::FINE, "the jets climb over a 3 m ledge");
    // At the hatch they fill again.
    let h = walker::hatch(&b);
    b.pilot.p = [
        h[0] << TO_WORLD,
        walker::ground(&b, h[0] << TO_WORLD, h[1] << TO_WORLD),
        h[1] << TO_WORLD,
    ];
    b.pilot.ground = true;
    b.pilot.v = [0; 3];
    planet::tick(&mut b, &In::default());
    assert_eq!(b.pilot.energy, HOP_ENERGY);
}

// ---- The cutter --------------------------------------------------------------------

/// The pilot `back` m from (x, z), on the pad's side, facing it.
fn stand_before(b: &mut Below, at: [i32; 2], back: i32) {
    let d = walker::apart(at, [0, 0]).max(1);
    let x = at[0] - at[0] * back / d;
    let z = at[1] - at[1] * back / d;
    b.pilot.p = [
        x << TO_WORLD,
        walker::ground(b, x << TO_WORLD, z << TO_WORLD),
        z << TO_WORLD,
    ];
    b.pilot.v = [0; 3];
    b.pilot.ground = true;
    b.pilot.yaw = planet::yaw_to(at[0] - x, at[1] - z);
}

/// A seam cracks in three stages of `STAGE_TIME[laser]` ticks and
/// bursts into `SEAM_TONNES[danger]` tonnes; the wreck in two stages into 2 t of
/// SPARES; a boulder's base in `BOULDER_LOOSE` ticks.
#[test]
fn the_cutter_cracks_seams_and_the_wreck_by_stages() {
    for (n, (_, sys)) in sites(14).iter().enumerate() {
        let laser = (n % 3) as u8;
        let mut b = landed(sys, 650, laser, 0);
        step_out(&mut b);
        let seam = b.places.seams[0];
        stand_before(&mut b, seam, 3 * M);
        assert_eq!(cutter::find(&b), Lock::Seam(0), "site {n}");
        let mut at = Vec::new();
        for t in 1..=600u32 {
            let was = b.seams[0];
            cutter::cut(&mut b, true);
            if b.seams[0] != was {
                at.push(t);
            }
            if b.seams[0] >= SEAM_STAGES {
                break;
            }
        }
        let st = u32::from(STAGE_TIME[usize::from(laser)]);
        assert_eq!(at, vec![st, 2 * st, 3 * st], "site {n} laser {laser}");
        assert!(!b.cutter.hot, "a seam cracks before the cutter runs hot");
        assert_eq!(b.taken & cutter::seam_bit(0), cutter::seam_bit(0));
        assert_eq!(b.note, Some("SEAM CRACKED"));
        let tonnes = loose::loose_tonnes(&b);
        assert_eq!(
            tonnes,
            u32::from(SEAM_TONNES[usize::from(sys.danger.min(3))])
        );
        let good = cutter::seam_good(sys.look);
        assert!(
            b.chunks
                .iter()
                .filter(|c| c.state != CHUNK_NONE)
                .all(|c| c.good == good)
        );
        // The wreck.
        let w = b.places.wreck;
        stand_before(&mut b, w, 5 * M);
        b.cutter.heat = 0;
        assert_eq!(cutter::find(&b), Lock::Wreck, "site {n}");
        let mut ticks = 0;
        while b.wreck < WRECK_STAGES {
            cutter::cut(&mut b, true);
            ticks += 1;
            assert!(ticks <= 3 * st, "site {n}: the wreck held");
        }
        assert_eq!(ticks, u32::from(WRECK_STAGES) * st);
        assert_eq!(b.taken & WRECK_BIT, WRECK_BIT);
        assert_eq!(b.note, Some("WRECK STRIPPED"));
        assert_eq!(loose::loose_tonnes(&b), tonnes + u32::from(WRECK_TONNES));
        // A boulder's base, when the site has one.
        if let Some(k) = b.rocks.iter().position(|r| r.state == ROCK_REST) {
            let r = b.rocks[k].at();
            stand_before(&mut b, [r[0], r[2]], 5 * M);
            b.cutter.heat = 0;
            if cutter::find(&b) == Lock::Boulder(k as u8) {
                for _ in 0..BOULDER_LOOSE {
                    cutter::cut(&mut b, true);
                }
                assert_eq!(
                    b.rocks[k].state, ROCK_ROLL,
                    "site {n}: the boulder came loose"
                );
            }
        }
    }
}

/// X held runs the cutter hot in `CUT_HEAT_MAX / CUT_HEAT_UP` ticks; it
/// cuts out (CUTTER HOT) and comes back when it has cooled to
/// `CUT_RESUME`.
#[test]
fn the_cutter_runs_hot_and_cools() {
    let (_, sys) = sites(1)[0];
    let mut b = landed(&sys, 650, 0, 0);
    step_out(&mut b);
    // Facing out, away from everything.
    let p = b.pilot.at();
    b.pilot.yaw = planet::yaw_to(p[0], p[2]);
    let mut hot_at = None;
    for t in 1..=400u32 {
        cutter::cut(&mut b, true);
        if b.cutter.hot {
            hot_at = Some(t);
            break;
        }
        assert!(b.cutter.on);
    }
    let up = (CUT_HEAT_MAX + CUT_HEAT_UP - 1) / CUT_HEAT_UP;
    assert_eq!(hot_at, Some(up as u32));
    assert_eq!(b.note, Some("CUTTER HOT"));
    assert!(!b.cutter.on, "no beam while hot");
    let mut back = 0;
    loop {
        cutter::cut(&mut b, true);
        back += 1;
        if !b.cutter.hot {
            break;
        }
        assert!(!b.cutter.on, "no beam while hot");
    }
    let down = (CUT_HEAT_MAX - CUT_RESUME + CUT_HEAT_DOWN - 1) / CUT_HEAT_DOWN;
    assert_eq!(back, down);
    assert!(b.cutter.on, "back on with X still held");
    println!(
        "hot after {up} ticks ({:.1} s), back after {down} ({:.1} s)",
        f64::from(up) / 60.0,
        f64::from(down) / 60.0
    );
}

// ---- Chunks and boulders ------------------------------------------------------------

/// Burst chunks keep their tonnes (in the hold or on the ground, never
/// lost, never made), never go into the ground, and the hold never
/// takes more than its room: HOLD FULL is said.
#[test]
fn chunks_keep_their_tonnes_and_never_overfill_the_hold() {
    for (n, (_, sys)) in sites(seeds().min(40)).iter().enumerate() {
        let mut b = landed(sys, 400 + (n as i32 * 53) % 501, 0, 0);
        step_out(&mut b);
        let room = 1 + (n as u16 % 4);
        b.room = room;
        let s = b.places.seams[n % SEAMS];
        let at = [s[0], site::surface(&b.site, s[0], s[1]), s[1]];
        let made = u32::from(loose::burst(&mut b, at, 2, 5));
        assert_eq!(made, 5);
        b.pilot.p = [(s[0] + 40 * M) << TO_WORLD, b.pilot.p[1], s[1] << TO_WORLD];
        let mut full = false;
        for t in 0..900u32 {
            loose::chunks(&mut b);
            if b.note == Some("HOLD FULL") {
                full = true;
            }
            assert_eq!(
                b.cargo.total() + loose::loose_tonnes(&b),
                made,
                "site {n} tick {t}"
            );
            assert!(b.cargo.total() <= u32::from(room));
            for c in b.chunks.iter().filter(|c| c.state != CHUNK_NONE) {
                let g = walker::ground(&b, c.p[0], c.p[2]);
                assert!(c.p[1] >= g, "site {n} tick {t}: a chunk in the ground");
            }
            // From tick 200 the pilot walks onto the nearest chunk.
            if t >= 200
                && t.is_multiple_of(10)
                && let Some(c) = b
                    .chunks
                    .iter()
                    .filter(|c| c.state != CHUNK_NONE && !c.full)
                    .min_by_key(|c| c.p[0].abs())
            {
                b.pilot.p = [c.p[0], walker::ground(&b, c.p[0], c.p[2]), c.p[2]];
            }
        }
        assert_eq!(b.cargo.total(), u32::from(room).min(made), "site {n}");
        assert_eq!(b.haul as u32, b.cargo.total());
        assert_eq!(full, u32::from(room) < made, "site {n}: HOLD FULL");
    }
}

/// A placed boulder (on the uphill side of a guarded seam, on 15
/// degrees or more) cut loose rolls downhill and stops (on flat ground,
/// at a wall, the ship, the wreck or the edge, or after its life), and
/// never goes into the ground.
#[test]
fn a_boulder_cut_loose_on_a_slope_rolls_and_stops() {
    let (mut n_rocks, mut rolled, mut longest, mut far) = (0, 0, 0u32, 0i32);
    for (n, (_, sys)) in sites(seeds()).iter().enumerate() {
        let mut b = landed(sys, 400 + (n as i32 * 53) % 501, 0, 0);
        step_out(&mut b);
        // The pilot far away (the hit has its own test).
        b.pilot.p[0] += (300 * M) << TO_WORLD;
        for k in 0..b.rocks.len() {
            if b.rocks[k].state != ROCK_REST {
                continue;
            }
            n_rocks += 1;
            let from = b.rocks[k].at();
            b.rocks[k].state = ROCK_ROLL;
            let mut t = 0u32;
            while b.rocks[k].state == ROCK_ROLL {
                loose::rocks(&mut b);
                t += 1;
                let r = b.rocks[k];
                let g = walker::ground(&b, r.p[0], r.p[2]);
                assert!(r.p[1] >= g, "site {n} rock {k}: in the ground");
                assert!(
                    t <= u32::from(BOULDER_LIFE) + 1,
                    "site {n} rock {k}: never stopped"
                );
            }
            assert_eq!(b.rocks[k].state, ROCK_STOPPED);
            let to = b.rocks[k].at();
            let d = walker::apart([from[0], from[2]], [to[0], to[2]]);
            if d >= 2 * M {
                rolled += 1;
            }
            assert!(
                to[1] < from[1] || d < 2 * M,
                "site {n} rock {k}: it rolled uphill"
            );
            far = far.max(d);
            longest = longest.max(t);
        }
    }
    println!(
        "{n_rocks} boulders cut loose: {rolled} rolled 2 m or more; farthest {} m, longest {:.1} s",
        far / M,
        f64::from(longest) / 60.0
    );
    assert!(n_rocks > 0);
    assert!(rolled * 10 >= n_rocks * 9, "nine in ten roll");
}

/// A rolling boulder that meets the pilot costs 30 points and knocks him
/// away, once.
#[test]
fn a_rolling_boulder_knocks_the_pilot() {
    for (n, (_, sys)) in sites(30).iter().enumerate() {
        let mut b = landed(sys, 650, 0, 2);
        step_out(&mut b);
        let Some(k) = b.rocks.iter().position(|r| r.state == ROCK_REST) else {
            continue;
        };
        b.rocks[k].state = ROCK_ROLL;
        // Run it a second, then stand in its way.
        for _ in 0..30 {
            loose::rocks(&mut b);
        }
        if b.rocks[k].state != ROCK_ROLL {
            continue;
        }
        let r = b.rocks[k];
        let ahead = [
            r.at()[0] + ((r.v[0] >> TO_WORLD) * 20),
            r.at()[2] + ((r.v[1] >> TO_WORLD) * 20),
        ];
        b.pilot.p = [
            ahead[0] << TO_WORLD,
            walker::ground(&b, ahead[0] << TO_WORLD, ahead[1] << TO_WORLD),
            ahead[1] << TO_WORLD,
        ];
        b.pilot.ground = true;
        b.pilot.v = [0; 3];
        let full = b.pilot.suit;
        for _ in 0..120 {
            loose::rocks(&mut b);
        }
        if b.pilot.suit < full {
            assert_eq!(full - b.pilot.suit, 30 * 60, "site {n}: one hit, 30 points");
            assert!(!b.pilot.ground, "knocked off his feet");
            println!("site {n}: the boulder hit the pilot");
            return;
        }
    }
    panic!("no boulder met the pilot");
}

// ---- The clock, the storm, the suit ------------------------------------------------

/// The clock by danger: the warning (per look) 30 s before the end, the
/// storm closing the fog to 24 m over 20 s in `STORM_STEPS` palette
/// steps, the air's colour sliding.
#[test]
fn the_clock_and_the_storm_by_danger() {
    let mut rows = String::new();
    for danger in 0..4u8 {
        for &look in SURFACES.iter() {
            let (g, id) = system_with_look(look);
            let mut sys = galaxy::system(seed_of(g), id);
            sys.danger = danger;
            let mut b = landed(&sys, 650, 0, 2);
            step_out(&mut b);
            // The clock counts from the touchdown (the hatch's second).
            let total = i32::from(SITE_TIME[usize::from(danger)]) * 60;
            assert_eq!(clock::total(&b), total);
            assert_eq!(i32::from(b.since), i32::from(HATCH_OPEN) + 1);
            b.pilot.suit = i32::MAX / 2;
            let mut warn_at = None;
            let mut steps = Vec::new();
            while i32::from(b.since) < total + i32::from(STORM_CLOSE) + 60 {
                b.note = None;
                clock::tick(&mut b);
                if let Some(t) = b.note {
                    if t == clock::STORM_FOOT {
                        // Step G4: the storm's own note, once it is on.
                        assert!(i32::from(b.since) >= total, "{t}");
                    } else {
                        assert!(t.ends_with("IN 30 S"), "{t}");
                        warn_at.get_or_insert((b.since, t));
                    }
                }
                let s = clock::step(&b);
                if steps.last() != Some(&s) {
                    steps.push(s);
                }
            }
            let (at, text) = warn_at.expect("the warning");
            assert_eq!(i32::from(at), total - i32::from(STORM_WARN) * 60);
            assert_eq!(steps, (0..=STORM_STEPS).collect::<Vec<_>>());
            assert_eq!(clock::closing(&b), 256, "the storm is closed");
            for lv in 0..3 {
                let near = clock::fog(&b, lv);
                assert!(near < 256 / 2, "the fog closed: {near}/256 at level {lv}");
            }
            if danger == 0 {
                rows += &format!("{look}: {text}\n");
            }
            // The palette's air slides.
            let mut calm = [0u16; mercator::art::COLOURS_LEN];
            let mut storm = calm;
            planet::palette::below(&mut calm, look, sys.star, sys.h, 0);
            planet::palette::below(&mut storm, look, sys.star, sys.h, STORM_STEPS);
            assert_ne!(calm, storm, "look {look}: the storm changes the colours");
        }
        rows += &format!(
            "{}: {} s, warning at {} s\n",
            DANGER_NAMES[usize::from(danger)],
            SITE_TIME[usize::from(danger)],
            SITE_TIME[usize::from(danger)] - STORM_WARN
        );
    }
    println!("{rows}");
}

/// The shield's level sizes the suit; the storm drains it away from the
/// hatch (not at it); the hatch refills it; SUIT LOW at a quarter; at 0
/// PILOT DOWN is a loss.
#[test]
fn the_suit_drains_in_the_storm_and_refills_at_the_hatch() {
    let (_, sys) = sites(1)[0];
    for shield in 0..3u8 {
        let mut b = landed(&sys, 650, 0, shield);
        step_out(&mut b);
        assert_eq!(b.pilot.suit, i32::from(SUIT_MAX[usize::from(shield)]) * 60);
        assert_eq!(b.pilot.suit_max, SUIT_MAX[usize::from(shield)]);
    }
    let mut b = landed(&sys, 650, 0, 0);
    step_out(&mut b);
    let full = b.pilot.suit;
    b.since = clock::total(&b) as u16;
    // At the hatch: no drain.
    for _ in 0..60 {
        planet::tick(&mut b, &In::default());
    }
    assert_eq!(b.pilot.suit, full, "no drain at the hatch");
    // Away from it: STORM_DRAIN a tick.
    let p = b.pilot.p;
    b.pilot.p = [p[0] + ((10 * M) << TO_WORLD), p[1], p[2]];
    b.pilot.p[1] = walker::ground(&b, b.pilot.p[0], b.pilot.p[2]);
    let before = b.pilot.suit;
    for _ in 0..60 {
        planet::tick(&mut b, &In::default());
    }
    assert_eq!(before - b.pilot.suit, 60 * STORM_DRAIN);
    // Back at the hatch: SUIT_REFILL a tick, up to full.
    b.pilot.suit = full / 5;
    b.note = None;
    planet::tick(&mut b, &In::default());
    assert_eq!(b.note, Some("SUIT LOW"));
    b.pilot.p = p;
    let low = b.pilot.suit;
    planet::tick(&mut b, &In::default());
    assert_eq!(b.pilot.suit, low + SUIT_REFILL);
    for _ in 0..(full / SUIT_REFILL + 2) {
        planet::tick(&mut b, &In::default());
    }
    assert_eq!(b.pilot.suit, full);
    // At 0, away: PILOT DOWN.
    b.pilot.p = [p[0] + ((10 * M) << TO_WORLD), p[1], p[2]];
    b.pilot.p[1] = walker::ground(&b, b.pilot.p[0], b.pilot.p[2]);
    b.pilot.suit = STORM_DRAIN;
    let mut out = Outcome::Stay;
    for _ in 0..3 {
        out = planet::tick(&mut b, &In::default());
        if out != Outcome::Stay {
            break;
        }
    }
    assert_eq!(out, Outcome::Lost);
    assert_eq!(b.note, Some("PILOT DOWN"));
}

// ---- The game's own way: write-back, PILOT DOWN, cut today -------------------------

/// From the dock to the pilot on foot at `look`, through the real game.
fn on_foot_in(look: u8) -> Driver {
    let mut d = below_in(look);
    land(&mut d, 8000);
    for _ in 0..=HATCH_OPEN + 2 {
        d.idle(1);
    }
    assert_eq!(d.app.below.phase, Phase::Foot);
    d
}

/// Chunks picked up go into the hold; B held at the hatch boards and
/// lifts; space gets the hold, the hull and the fuel as the visit left
/// them.
#[test]
fn the_take_off_writes_back_hold_hull_and_fuel() {
    let mut d = on_foot_in(1);
    let before = d.app.world.car;
    let b = &mut d.app.below;
    let at = b.pilot.at();
    assert_eq!(loose::burst(b, at, 2, 3), 3);
    for c in b.chunks.iter_mut().filter(|c| c.state != CHUNK_NONE) {
        c.p = b.pilot.p;
        c.state = loose::CHUNK_REST;
    }
    b.hull = b.hull.saturating_sub(1);
    let hull = b.hull;
    d.idle(2);
    assert_eq!(d.app.below.haul, 3);
    for _ in 0..LIFT_HOLD + 1 {
        d.hold(button::B);
    }
    assert_eq!(d.app.below.phase, Phase::Board);
    d.idle(u32::from(BOARD_TICKS + TAKEOFF_TICKS) + 4);
    assert_eq!(d.app.scene, Scene::Flight);
    let car = d.app.world.car;
    assert_eq!(car.hold.0[2], before.hold.0[2] + 3);
    assert_eq!(car.hold.total(), before.hold.total() + 3);
    assert_eq!(car.hull, hull);
    assert_eq!(car.fuel, before.fuel - LIFT_FUEL);
}

/// The suit gone ends as a loss in space ends (PILOT DOWN, the LOST
/// card, back at the last station without the cargo).
#[test]
fn pilot_down_is_a_loss() {
    let mut d = on_foot_in(5);
    let b = &mut d.app.below;
    let p = b.pilot.p;
    b.pilot.p = [p[0] + ((12 * M) << TO_WORLD), p[1], p[2]];
    b.pilot.p[1] = walker::ground(b, b.pilot.p[0], b.pilot.p[2]);
    b.since = clock::total(b) as u16;
    b.pilot.suit = 30;
    let mut lost = false;
    for _ in 0..60 {
        d.idle(1);
        if d.app.scene == Scene::Lost {
            lost = true;
            break;
        }
    }
    assert!(lost, "PILOT DOWN loses the ship");
    assert!(d.events.iter().any(|e| matches!(e, Event::Lost { .. })));
    d.wait_card();
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Docked);
}

/// What was cut stays cut until the next day (a jump is a day): the
/// docked line says CUT TODAY when everything was; a new day or another
/// system has its site whole.
#[test]
fn what_was_cut_stays_cut_until_the_next_day() {
    let mut d = on_foot_in(7);
    // Cut seam 0 and the wreck the game's way.
    let seam = d.app.below.places.seams[0];
    stand_before(&mut d.app.below, seam, 3 * M);
    for _ in 0..600 {
        d.hold(button::X);
        if d.app.below.seams[0] >= SEAM_STAGES {
            break;
        }
    }
    assert_eq!(d.app.below.taken, cutter::seam_bit(0));
    let mut again = d.app.below;
    assert!(planet::begin(&mut again, &d.app.world));
    assert_eq!(again.seams[0], SEAM_STAGES, "the same day: still cut");
    assert_eq!(again.seams[1], 0);
    assert!(!space::cut_today(&again, &d.app.world));
    // Everything cut: CUT TODAY on the docked line.
    again.taken = ALL_TAKEN;
    assert!(space::cut_today(&again, &d.app.world));
    let mut t = mercator::hud::Txt::new();
    space::dock_line(&mut t, &d.app.world, true);
    assert_eq!(t.as_str(), "SITE  CUT TODAY");
    let mut whole = again;
    assert!(planet::begin(&mut whole, &d.app.world));
    assert!(whole.seams.iter().all(|&s| s == SEAM_STAGES));
    assert_eq!(whole.wreck, WRECK_STAGES);
    // The next day (what a jump does to the day), the same system.
    d.app.world.car.day += 1;
    let mut fresh = whole;
    assert!(planet::begin(&mut fresh, &d.app.world));
    assert_eq!(fresh.taken, 0);
    assert!(fresh.seams.iter().all(|&s| s == 0));
    assert!(!space::cut_today(&whole, &d.app.world));
    // The App's new career forgets it.
    d.app.below.taken_day = Some((d.app.world.car.system, d.app.world.car.day));
    d.app.below.taken = ALL_TAKEN;
    d.app.new_game();
    assert_eq!(d.app.below.taken_day, None);
}

// ---- The picture's cost on foot ----------------------------------------------------

/// Triangles and pixels of the pictures on foot, at every level, for
/// every look: out of the hatch (looking away from the ship), at seam 0
/// cutting with its chunks out, and walking back to the ship (the ship,
/// the pad and the wreck's side in view). Rows to `planet-g3/cost.txt`.
#[test]
fn the_cost_of_a_picture_on_foot() {
    let dir = shots().join("planet-g3");
    std::fs::create_dir_all(&dir).unwrap();
    let mut rows = String::from("look     detail scene   tris  pixels objects\n");
    let mut worst = [0u32; 3];
    for &look in SURFACES.iter() {
        let mut d = on_foot_in(look);
        let name = mercator::art::LOOK_NAMES[usize::from(look)].to_ascii_lowercase();
        let hatch = d.app.below;
        let mut seam = hatch;
        let s = seam.places.seams[0];
        stand_before(&mut seam, s, 4 * M);
        let at = [s[0], site::surface(&seam.site, s[0], s[1]), s[1]];
        loose::burst(&mut seam, at, 2, 5);
        for _ in 0..20 {
            loose::chunks(&mut seam);
        }
        seam.cutter.on = true;
        seam.cutter.lock = Lock::Seam(0);
        seam.cutter.end = cutter::aim(&seam, Lock::Seam(0)).unwrap();
        seam.cam_goal = seam.pilot.yaw;
        planet::camera::foot_follow(&mut seam, true);
        let mut back = hatch;
        let out = [hatch.places.wreck[0] / 3, hatch.places.wreck[1] / 3];
        stand_before(&mut back, out, 0);
        back.pilot.yaw = planet::yaw_to(-out[0], -out[1]);
        back.cam_goal = back.pilot.yaw;
        planet::camera::foot_follow(&mut back, true);
        for det in Detail::ALL {
            for (scene, b) in [("hatch", hatch), ("seam", seam), ("back", back)] {
                d.app.render.begin();
                planet::draw(&mut d.frame, &mut d.app.render, &b, det);
                planet::panel(&mut d.frame, &b);
                let st = d.app.render.stats;
                rows += &format!(
                    "{:8} {:6} {:6} {:5} {:7} {:7}\n",
                    name,
                    det.name(),
                    scene,
                    st.tris,
                    st.pixels,
                    st.objects
                );
                let lv = planet::level(det);
                worst[lv] = worst[lv].max(st.tris);
            }
        }
    }
    rows += &format!(
        "most triangles on foot: LOW {} MID {} HIGH {}\n",
        worst[0], worst[1], worst[2]
    );
    std::fs::write(dir.join("cost.txt"), &rows).unwrap();
    println!("{rows}");
}

// ---- The scripted visit -------------------------------------------------------------

#[derive(Default)]
struct Visit {
    got: [u32; GOODS],
    made: u32,
    seams: u32,
    wreck: bool,
    aboard: bool,
    lost: bool,
    ticks: u32,
    full: bool,
    /// Step G4: drones downed, bolts that hit.
    kills: u32,
    hits: u32,
}

/// What the scripted pilot is after.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Goal {
    Seam(usize),
    Wreck,
    Chunk(usize),
    Home,
}

/// A scripted visit (it measures the RULES, not a human): out of the
/// hatch, to the nearest seam or the wreck, cut it, pick up its chunks,
/// on to the next; home when the clock (with the walk back at 3 m/s and
/// 20 s spare), the hold or the suit says so; B held at the hatch. When
/// it makes no headway for 1.5 s it hops (A held a second); after four
/// hops it gives the thing up.
fn visit(b: &mut Below) -> Visit {
    step_out(b);
    let start = b.cargo;
    let mut v = Visit::default();
    let mut skip = [false; SEAMS + 1 + 24];
    let mut goal: Option<Goal> = None;
    let (mut best, mut idle, mut hop, mut tries) = (i32::MAX, 0u32, 0u32, 0u32);
    let mut home = false;
    let mut last = 0;
    let end = clock::total(b) as u32 + 90 * 60;
    for t in 0..end {
        let me = b.pilot.at();
        let me2 = [me[0], me[2]];
        let hatch = walker::hatch(b);
        let to_hatch = walker::apart(me2, hatch);
        let room = u32::from(b.room);
        if !home
            && (clock::left(b) < to_hatch / M * 60 / 3 + 20 * 60
                || b.cargo.total() >= room
                || walker::suit_points(b) < 15)
        {
            home = true;
            goal = Some(Goal::Home);
        }
        // The goal still there?
        let alive = |b: &Below, g: Goal| match g {
            Goal::Seam(k) => b.seams[k] < SEAM_STAGES && !skip[k],
            Goal::Wreck => b.wreck < WRECK_STAGES && !skip[SEAMS],
            Goal::Chunk(k) => {
                b.chunks[k].state != CHUNK_NONE && !b.chunks[k].full && !skip[SEAMS + 1 + k]
            }
            Goal::Home => true,
        };
        if goal.is_some_and(|g| !alive(b, g)) {
            goal = None;
        }
        if goal.is_none() || matches!(goal, Some(Goal::Seam(_) | Goal::Wreck)) {
            // Chunks first (nearest), then the nearest seam or wreck.
            let mut pick: Option<(i32, Goal)> = None;
            for (k, c) in b.chunks.iter().enumerate() {
                if alive(b, Goal::Chunk(k)) {
                    let a = c.at();
                    let d = walker::apart(me2, [a[0], a[2]]);
                    if pick.is_none_or(|p| d < p.0) {
                        pick = Some((d, Goal::Chunk(k)));
                    }
                }
            }
            if pick.is_none() && goal.is_none() {
                for k in 0..SEAMS {
                    if alive(b, Goal::Seam(k)) {
                        let d = walker::apart(me2, b.places.seams[k]);
                        if pick.is_none_or(|p| d < p.0) {
                            pick = Some((d, Goal::Seam(k)));
                        }
                    }
                }
                if alive(b, Goal::Wreck) {
                    let d = walker::apart(me2, b.places.wreck);
                    if pick.is_none_or(|p| d < p.0) {
                        pick = Some((d, Goal::Wreck));
                    }
                }
            }
            if let Some((_, g)) = pick {
                if goal != Some(g) {
                    goal = Some(g);
                    best = i32::MAX;
                    idle = 0;
                    tries = 0;
                }
            } else if goal.is_none() {
                home = true;
                goal = Some(Goal::Home);
            }
        }
        let g = goal.unwrap();
        let (to, cut_at) = match g {
            Goal::Seam(k) => (b.places.seams[k], 4 * M),
            Goal::Wreck => (b.places.wreck, 6 * M),
            Goal::Chunk(k) => {
                let a = b.chunks[k].at();
                ([a[0], a[2]], 0)
            }
            Goal::Home => (hatch, 0),
        };
        let d = walker::apart(me2, to);
        let (dx, dz) = (i64::from(to[0] - me[0]), i64::from(to[1] - me[2]));
        let mut held = 0;
        let mut push;
        if g == Goal::Home && walker::at_hatch(b) {
            held = button::B;
            push = [0, 0];
        } else if cut_at > 0 && d <= cut_at + 2 * M {
            held = button::X;
            push = stick(b, dx, dz, 40);
        } else {
            push = stick(b, dx, dz, 256);
        }
        // No headway: hop; after four hops, give the thing up.
        let cutting = held == button::X;
        if !cutting && held != button::B {
            if d < best - M / 2 {
                best = d;
                idle = 0;
            } else {
                idle += 1;
            }
            if idle > 90 {
                idle = 0;
                best = d;
                hop = 60;
                tries += 1;
                if tries > 4 && g != Goal::Home {
                    let i = match g {
                        Goal::Seam(k) => k,
                        Goal::Wreck => SEAMS,
                        Goal::Chunk(k) => SEAMS + 1 + k,
                        Goal::Home => unreachable!(),
                    };
                    skip[i] = true;
                    goal = None;
                    tries = 0;
                }
            }
        }
        // Step G4: the drones. Any locked one (or its bolt in flight)
        // within reach first: a side step across its line at full
        // speed; else one that aims or cools within reach: face it and
        // cut it (it bursts long before the cutter is hot at laser 0).
        if g != Goal::Home || !walker::at_hatch(b) {
            let reach = |d: &drones::Drone| walker::apart(me2, [d.p[0], d.p[2]]);
            let locked = b
                .drones
                .iter()
                .filter(|d| (d.locked() || d.bolt_t > 0) && reach(d) < 30 * M)
                .min_by_key(|d| reach(d))
                .copied();
            let near = b
                .drones
                .iter()
                .filter(|d| d.alive() && d.mode != drones::PATROL && reach(d) < 28 * M)
                .min_by_key(|d| reach(d))
                .copied();
            if let Some(dr) = locked {
                let (ex, ez) = (i64::from(dr.p[0] - me[0]), i64::from(dr.p[2] - me[2]));
                // Across its line.
                push = stick(b, -ez, ex, 256);
                held &= !button::X;
            } else if let Some(dr) = near {
                let (ex, ez) = (i64::from(dr.p[0] - me[0]), i64::from(dr.p[2] - me[2]));
                push = stick(b, ex, ez, 40);
                held = button::X;
            }
        }
        let mut pressed = held & !last;
        if hop > 0 {
            held |= button::A;
            if hop == 60 {
                pressed |= button::A;
            }
            hop -= 1;
        }
        last = held;
        let suit = b.pilot.suit;
        let out = planet::tick(b, &input(held, pressed, push));
        keeps_the_rules(b, &format!("visit tick {t}"));
        if suit - b.pilot.suit >= 8 * 60 {
            v.hits += 1;
        }
        if b.note == Some("HOLD FULL") {
            v.full = true;
        }
        v.ticks = t;
        match out {
            Outcome::Lifted => {
                v.aboard = true;
                break;
            }
            Outcome::Lost => {
                v.lost = true;
                break;
            }
            Outcome::Stay => {}
        }
    }
    for g in 0..GOODS {
        v.got[g] = u32::from(b.cargo.0[g] - start.0[g]);
    }
    v.seams = (0..SEAMS).filter(|&k| b.seams[k] >= SEAM_STAGES).count() as u32;
    v.wreck = b.wreck >= WRECK_STAGES;
    v.kills = u32::from(b.kills);
    v.made = v.seams * u32::from(SEAM_TONNES[usize::from(b.danger.min(3))])
        + if v.wreck { u32::from(WRECK_TONNES) } else { 0 }
        + v.kills * u32::from(DRONE_TONNES);
    assert_eq!(
        u32::from(b.haul) + loose::loose_tonnes(b),
        v.made,
        "the tonnes are kept"
    );
    v
}

/// The scripted visit on many real sites (ship 0, empty, laser and
/// shield level 0): earnings by look and by danger, the tonnes valued at
/// the best day-0 sale price in the galaxy (where a trader would take
/// them). SPEC 3.11's EST: 330 CR SECURE to 760 CR LAWLESS with drones;
/// a careful trade jump about 530 CR. Rows to `planet-g4/visit.txt`.
#[test]
fn a_scripted_visit_over_many_sites() {
    let list = sites(seeds());
    let mut by_look: Vec<Vec<u32>> = vec![Vec::new(); 8];
    let mut by_danger: Vec<Vec<u32>> = vec![Vec::new(); 4];
    let mut rows = String::new();
    let (mut aboard, mut lost, mut full) = (0, 0, 0);
    let (mut seams, mut wrecks, mut tonnes) = (0, 0, 0);
    let (mut kills, mut met, mut hits) = (0, 0, 0);
    let mut secs = Vec::new();
    for (n, (seed, sys)) in list.iter().enumerate() {
        let mut b = landed(sys, 400 + (n as i32 * 37) % 501, 0, 0);
        let v = visit(&mut b);
        let best = |g: usize| {
            (0..SYSTEMS as u8)
                .map(|i| market::sell_price(&galaxy::system(*seed, i), g, 0))
                .max()
                .unwrap()
        };
        let cr: u32 = (0..GOODS).map(|g| v.got[g] * best(g)).sum();
        by_look[usize::from(sys.look)].push(cr);
        by_danger[usize::from(sys.danger.min(3))].push(cr);
        aboard += u32::from(v.aboard);
        lost += u32::from(v.lost);
        full += u32::from(v.full);
        seams += v.seams;
        wrecks += u32::from(v.wreck);
        tonnes += v.got.iter().sum::<u32>();
        kills += v.kills;
        met += u32::from(b.met);
        hits += v.hits;
        secs.push(v.ticks / 60);
        rows += &format!(
            "site {n:3} look {} danger {} seams {} wreck {} drones {}/{} hits {} tonnes {:2} of {:2} cr {cr:4} {} {} s\n",
            sys.look,
            sys.danger,
            v.seams,
            u8::from(v.wreck),
            v.kills,
            b.met,
            v.hits,
            v.got.iter().sum::<u32>(),
            v.made,
            if v.aboard {
                "aboard"
            } else if v.lost {
                "LOST"
            } else {
                "out"
            },
            v.ticks / 60
        );
        assert!(v.aboard || v.lost, "site {n}: the visit ended");
    }
    let stat = |v: &Vec<u32>| {
        if v.is_empty() {
            return String::from("-");
        }
        let mut s = v.clone();
        s.sort_unstable();
        let avg = s.iter().sum::<u32>() / s.len() as u32;
        format!(
            "{:3} visits: average {avg:4} CR, median {:4}, {}..{}",
            s.len(),
            s[s.len() / 2],
            s[0],
            s[s.len() - 1]
        )
    };
    let mut head = format!(
        "SCRIPTED PILOT (measures the rules, not a human): {} sites, ship 0 (hold {} t), laser and shield 0\n\
         back aboard {aboard}, PILOT DOWN {lost}, hold full {full}; seams cracked {seams}, wrecks stripped {wrecks}, tonnes home {tonnes}\n\
         drones downed {kills} of {met}, bolts that hit {hits}\n",
        list.len(),
        CRAFTS[0].hold
    );
    secs.sort_unstable();
    head += &format!(
        "time below (s): median {}, longest {}\n",
        secs[secs.len() / 2],
        secs[secs.len() - 1]
    );
    for (l, v) in by_look.iter().enumerate() {
        if !v.is_empty() {
            head += &format!("{:7} {}\n", mercator::art::LOOK_NAMES[l], stat(v));
        }
    }
    for (k, v) in by_danger.iter().enumerate() {
        head += &format!("{:7} {}\n", DANGER_NAMES[k], stat(v));
    }
    let dir = shots().join("planet-g4");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("visit.txt"), format!("{head}\n{rows}")).unwrap();
    println!("{head}");
    assert_eq!(lost, 0, "the scripted pilot never runs out of suit");
    // Against trading (step G4): a trade jump pays about 530 CR (SPEC
    // 3.11; `balance`'s SECURE-only trader made 49,700 CR in 94.4 jumps
    // after step M5, about 527 a jump, ships bought). A SECURE visit pays at
    // least half of that, and the dangerous half of the sites (RISKY,
    // LAWLESS) pays more on average than the safe half.
    let avg = |ks: &[usize]| {
        let v: Vec<u32> = ks.iter().flat_map(|&k| by_danger[k].clone()).collect();
        v.iter().sum::<u32>() / v.len().max(1) as u32
    };
    assert!(avg(&[0]) >= 265, "a SECURE visit pays {} CR", avg(&[0]));
    assert!(
        avg(&[2, 3]) > avg(&[0, 1]),
        "danger pays: {} against {}",
        avg(&[2, 3]),
        avg(&[0, 1])
    );
}

// ---- Pictures ----------------------------------------------------------------------

/// Pictures on foot for four looks (git-ignored, `planet-g3/` in the
/// shots folder): out of the hatch, a seam at each stage, the chunks
/// rolling, a boulder rolling, the clock at 0:30 and 0:05, the storm,
/// boarding and the report card.
#[test]
fn pictures_on_foot() {
    let dir = shots().join("planet-g3");
    std::fs::create_dir_all(&dir).unwrap();
    for look in [1u8, 2, 5, 7] {
        let name = mercator::art::LOOK_NAMES[usize::from(look)].to_ascii_lowercase();
        let mut d = on_foot_in(look);
        let save = |d: &mut Driver, what: &str| {
            let c = d.app.colours().to_vec();
            save_frame(&dir.join(format!("{name}-{what}.png")), &d.frame, &c, 3);
        };
        d.idle(20);
        save(&mut d, "01-hatch");
        // Walk out a little (the stick ahead).
        for _ in 0..90 {
            let k = mercator::app::Keys {
                left: (0, -32_000),
                ..Default::default()
            };
            d.call_keys(k);
        }
        save(&mut d, "02-walking");
        // A seam, stage by stage.
        let seam = d.app.below.places.seams[0];
        stand_before(&mut d.app.below, seam, 4 * M);
        d.app.below.cam_goal = d.app.below.pilot.yaw;
        planet::camera::foot_follow(&mut d.app.below, true);
        let mut stage = 0;
        for _ in 0..600 {
            d.hold(button::X);
            let s = d.app.below.seams[0];
            if s != stage {
                stage = s;
                if s < SEAM_STAGES {
                    save(&mut d, &format!("03-seam-stage{s}"));
                } else {
                    save(&mut d, "04-seam-burst");
                    break;
                }
            }
        }
        d.idle(25);
        save(&mut d, "05-chunks-rolling");
        // A boulder: cut its base from its side, then watch it roll.
        if let Some(k) = d.app.below.rocks.iter().position(|r| r.state == ROCK_REST) {
            let r = d.app.below.rocks[k].at();
            stand_before(&mut d.app.below, [r[0], r[2]], 7 * M);
            d.app.below.cam_goal = d.app.below.pilot.yaw;
            planet::camera::foot_follow(&mut d.app.below, true);
            d.app.below.cutter.heat = 0;
            for _ in 0..BOULDER_LOOSE + 2 {
                d.hold(button::X);
            }
            d.idle(40);
            save(&mut d, "06-boulder-rolling");
        }
        // The clock: 0:30 (the warning), 0:05, and 20 s into the storm.
        let total = clock::total(&d.app.below);
        d.app.below.since = (total - 30 * 60) as u16;
        d.idle(3);
        save(&mut d, "07-clock-0-30");
        d.app.below.since = (total - 5 * 60) as u16;
        d.idle(3);
        save(&mut d, "08-clock-0-05");
        d.app.below.since = (total + 20 * 60) as u16;
        d.app.below.pilot.suit = i32::from(d.app.below.pilot.suit_max) * 60;
        d.idle(3);
        save(&mut d, "09-storm");
        // Home: at the hatch, B held, boarding, the report.
        let h = walker::hatch(&d.app.below);
        let b = &mut d.app.below;
        b.pilot.p = [
            h[0] << TO_WORLD,
            walker::ground(b, h[0] << TO_WORLD, h[1] << TO_WORLD),
            h[1] << TO_WORLD,
        ];
        b.since = 60 * 60;
        b.pilot.yaw = b.yaw.wrapping_add(0xC000);
        b.cam_goal = b.pilot.yaw;
        planet::camera::foot_follow(b, true);
        d.idle(2);
        save(&mut d, "10-at-hatch");
        for _ in 0..LIFT_HOLD + 4 {
            d.hold(button::B);
        }
        save(&mut d, "11-boarding");
        d.idle(40);
        save(&mut d, "12-report");
    }
}
