//! Step G4: the drones, the particle pool, the props and the scan
//! (SPEC 3.6, 3.10, 3.13 items 9 and 10): the telegraph's timing, a side
//! step or a jump in its last instant, a bolt's damage by danger and
//! suit level, a boulder crushing a drone, the leash, the pool never
//! running over, the props kept off the things, the scan's marks, the
//! heaviest picture's budget at the three levels, and pictures.
//!
//! The scripted pilots here measure the RULES, never a human's skill.

mod common;

use common::below::below_in;
use common::{Driver, save_frame, shots};
use game_kit::button;
use mercator::app::seed_of;
use mercator::fixed::V3;
use mercator::galaxy::{self, SYSTEMS, System};
use mercator::planet::drones::{self, AIM, BURST, COOL, Drone, GONE, PATROL};
use mercator::planet::lander::{In, Phase, TO_WORLD};
use mercator::planet::loose::{self, ROCK_REST, ROCK_ROLL};
use mercator::planet::sparks::{self, Pool};
use mercator::planet::tune::{
    BOLT_BASE, BOLT_PER, DRONE_AIM, DRONE_HP, DRONE_HP_PER, DRONE_LEASH, DRONE_LOCK, DRONE_UP,
    HATCH_OPEN, PROP_CLEAR, PROP_NEAR_THING, SCAN_COST, SCAN_GROW, SCAN_R, SEAMS, SPARKS, SUIT_MAX,
};
use mercator::planet::{self, Below, Outcome, place, props, site, walker};
use mercator::scene::Detail;
use mercator::world::{CRAFTS, M};

/// The first system of galaxies 1.. with a surface and `danger`.
fn system(danger: u8, skip: usize) -> System {
    let mut n = 0;
    for g in 1..=60u8 {
        let seed = seed_of(g);
        for i in 0..SYSTEMS as u8 {
            let s = galaxy::system(seed, i);
            if s.look != 4 && s.danger == danger {
                if n == skip {
                    return s;
                }
                n += 1;
            }
        }
    }
    panic!("no system with danger {danger}");
}

/// The ship landed on the site of `sys` (as foot.rs makes it), the
/// pilot out of the hatch.
fn out_at(sys: &System, shield: u8) -> Below {
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
        shield,
        ..Below::EMPTY
    };
    planet::populate(&mut b, 0);
    let hull = CRAFTS[0].hull;
    planet::ready(&mut b, 0, 0, 600, (hull, hull), 10);
    b.phase = Phase::Landed;
    b.phase_t = 0;
    b.p = [0, b.site.pad << TO_WORLD, 0];
    b.v = [0; 3];
    for _ in 0..=HATCH_OPEN {
        assert_eq!(planet::tick(&mut b, &In::default()), Outcome::Stay);
    }
    assert_eq!(b.phase, Phase::Foot);
    b
}

/// Puts the pilot at (x, z) on the ground, still, facing `yaw`.
fn put_pilot(b: &mut Below, x: i32, z: i32, yaw: u16) {
    b.pilot.p = [
        x << TO_WORLD,
        walker::ground(b, x << TO_WORLD, z << TO_WORLD),
        z << TO_WORLD,
    ];
    b.pilot.v = [0; 3];
    b.pilot.ground = true;
    b.pilot.yaw = yaw;
}

/// A spot 14 m from seam 0 toward the pad (where a guard sees a pilot).
fn near_seam(b: &Below) -> [i32; 2] {
    let s = b.places.seams[0];
    let d = walker::apart(s, [0, 0]).max(1);
    [s[0] - s[0] * 14 * M / d, s[1] - s[1] * 14 * M / d]
}

/// Only drone 0 is left, `r` from the pilot along `bearing`, 3 m over
/// the ground, its aim just begun (as `drones::tick` starts it).
fn lone_drone(b: &mut Below, r: i32, bearing: u16) {
    for d in b.drones.iter_mut() {
        *d = Drone::NONE;
    }
    let me = b.pilot.at();
    let (s, c) = (mercator::fixed::sin(bearing), mercator::fixed::cos(bearing));
    let x = me[0] + ((s * (r >> 4)) >> 10);
    let z = me[2] + ((c * (r >> 4)) >> 10);
    let p = [x, site::surface(&b.site, x, z) + DRONE_UP, z];
    b.drones[0] = Drone {
        p,
        yaw: planet::yaw_to(me[0] - x, me[2] - z),
        mode: AIM,
        home: 0,
        hp: drones::points(b.danger),
        lock: [me[0], me[1] + M * 9 / 10, me[2]],
        ..Drone::NONE
    };
}

fn input(held: u32, pressed: u32, push: [i32; 2]) -> In {
    In {
        held,
        pressed,
        push,
        ..In::default()
    }
}

/// The push (camera frame) that walks along world direction (dx, dz).
fn push_to(b: &Below, dx: i64, dz: i64) -> [i32; 2] {
    let d = ((dx * dx + dz * dz) as f64).sqrt().max(1.0);
    let (ux, uz) = (dx as f64 / d, dz as f64 / d);
    let (s, c) = (
        f64::from(mercator::fixed::sin(b.cam_yaw)) / 16384.0,
        f64::from(mercator::fixed::cos(b.cam_yaw)) / 16384.0,
    );
    [
        ((ux * c - uz * s) * 256.0).round() as i32,
        ((ux * s + uz * c) * 256.0).round() as i32,
    ]
}

/// What the pilot does in the drone's last instant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dodge {
    Stand,
    Side,
    Jump,
}

/// How a shot ended.
#[derive(Clone, Copy, Debug)]
struct Shot {
    /// The bolt hit the pilot.
    hit: bool,
    /// The bolt ended in the ground short of him (a hill between).
    ground: bool,
    /// How far he was off the locked point when the bolt ended (cm).
    off: i32,
}

/// One aim of a lone drone at `r` along `bearing`; the pilot does
/// `dodge` from the first tick he can see the lock (the picture after
/// the first locked tick) on.
fn one_shot(base: &Below, r: i32, bearing: u16, dodge: Dodge) -> Shot {
    let mut b = *base;
    lone_drone(&mut b, r, bearing);
    let suit = b.pilot.suit;
    let mut fired = false;
    let mut last = [0; 3];
    for _ in 0..240 {
        let d = b.drones[0];
        let (held, pressed, push) = if d.locked() || fired {
            match dodge {
                Dodge::Stand => (0, 0, [0, 0]),
                Dodge::Side => {
                    let me = b.pilot.at();
                    let (ex, ez) = (i64::from(d.p[0] - me[0]), i64::from(d.p[2] - me[2]));
                    (0, 0, push_to(&b, -ez, ex))
                }
                Dodge::Jump => {
                    let first = d.mode == AIM && d.t == DRONE_AIM - DRONE_LOCK + 1;
                    (button::A, if first { button::A } else { 0 }, [0, 0])
                }
            }
        } else {
            (0, 0, [0, 0])
        };
        if fired {
            last = b.drones[0].bolt;
        }
        planet::tick(&mut b, &input(held, pressed, push));
        if b.drones[0].bolt_t > 0 {
            fired = true;
        }
        if fired && b.drones[0].bolt_t == 0 {
            break;
        }
    }
    assert!(fired, "the drone fired");
    let me = b.pilot.at();
    let lock = b.drones[0].lock;
    let hit = b.pilot.suit < suit;
    Shot {
        hit,
        ground: !hit && walker::apart([last[0], last[2]], [me[0], me[2]]) > 2 * M,
        off: walker::apart([lock[0], lock[2]], [me[0], me[2]]) * 100 / M,
    }
}

/// SPEC 3.6: the telegraph lasts `DRONE_AIM` ticks (1 s); the line
/// follows the pilot until its last `DRONE_LOCK` ticks (0.15 s), then
/// stands still; the bolt leaves on the last tick and the drone cools.
#[test]
fn the_telegraph_lasts_a_second_and_locks_its_last_nine_ticks() {
    let sys = system(1, 0);
    let mut b = out_at(&sys, 0);
    let at = near_seam(&b);
    put_pilot(&mut b, at[0], at[1], 0);
    lone_drone(&mut b, 12 * M, 0x4000);
    let mut locks = Vec::new();
    let mut fired_at = None;
    for t in 1..=DRONE_AIM + 2 {
        // The pilot walks slowly across the line all along.
        let d = b.drones[0];
        let me = b.pilot.at();
        let (ex, ez) = (i64::from(d.p[0] - me[0]), i64::from(d.p[2] - me[2]));
        let p = push_to(&b, -ez, ex).map(|v| v / 3);
        planet::tick(&mut b, &input(0, 0, p));
        let d = b.drones[0];
        locks.push((t, d.lock, d.locked()));
        if fired_at.is_none() && d.bolt_t > 0 {
            fired_at = Some(t);
            assert_eq!(d.mode, COOL);
        }
        if t < DRONE_AIM {
            assert_eq!(d.mode, AIM, "still aiming at tick {t}");
        }
    }
    assert_eq!(
        fired_at,
        Some(DRONE_AIM),
        "the bolt leaves on the aim's last tick"
    );
    let free = DRONE_AIM - DRONE_LOCK;
    for w in locks.windows(2) {
        let ((t0, l0, _), (t1, l1, k1)) = (w[0], w[1]);
        if t1 <= free {
            assert_ne!(l0, l1, "the line follows him at tick {t1}");
            assert!(!k1);
        } else if t1 <= DRONE_AIM {
            assert_eq!(l0, l1, "the line stands still at tick {t1} (from {t0})");
            assert!(k1 || t1 == DRONE_AIM, "locked at tick {t1}");
        }
    }
}

/// SPEC 3.13 item 9: a pilot who side-steps (full stick across the
/// line, from the first picture that shows the lock) is never hit from
/// 6 m on; one who stands still always is, unless a hill takes the
/// bolt. Over 5 sites, 7 ranges from 4 to 24 m and 8 bearings. From
/// 4 m the bolt arrives 0.13 s after it leaves: the step has had 0.27 s
/// in all, about 0.7 m, so there it may or may not miss (counted, not
/// failed; the report says so).
#[test]
fn a_side_step_in_the_last_instant_escapes_and_standing_still_never_does() {
    let mut rows = String::new();
    let (mut side_hits, mut stand_miss, mut ground, mut n) = (0, 0, 0, 0);
    let mut close = [0u32; 2];
    let mut least_off = i32::MAX;
    for skip in 0..5 {
        let sys = system((skip % 4) as u8, skip);
        let mut b = out_at(&sys, 0);
        let at = near_seam(&b);
        put_pilot(&mut b, at[0], at[1], 0);
        for r in [4, 6, 8, 12, 16, 20, 24] {
            for k in 0..8u16 {
                let bearing = k * 0x2000 + 0x0800;
                let stood = one_shot(&b, r * M, bearing, Dodge::Stand);
                let side = one_shot(&b, r * M, bearing, Dodge::Side);
                n += 1;
                if stood.ground {
                    ground += 1;
                    rows += &format!(
                        "site {skip} range {r} m bearing {bearing:#06x}: a hill took the bolt\n"
                    );
                    continue;
                }
                if r == 4 {
                    close[usize::from(side.hit)] += 1;
                    continue;
                }
                least_off = least_off.min(side.off);
                stand_miss += u32::from(!stood.hit);
                side_hits += u32::from(side.hit);
                if !stood.hit || side.hit {
                    rows += &format!(
                        "site {skip} range {r} m bearing {bearing:#06x}: stood hit {}, side step hit {} (off {} cm)\n",
                        stood.hit, side.hit, side.off
                    );
                }
            }
        }
    }
    let text = format!(
        "{n} shots each way; hills took {ground}; from 6 m on: standing missed {stand_miss}, \
         side step hit {side_hits}, the step was at least {least_off} cm off the lock; \
         at 4 m: side step missed {} hit {}\n{rows}",
        close[0], close[1]
    );
    println!("{text}");
    let dir = shots().join("planet-g4");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("dodge.txt"), &text).unwrap();
    assert_eq!(stand_miss, 0, "standing still is always hit");
    assert_eq!(side_hits, 0, "a side step is never hit");
    assert!(ground * 10 <= n, "few shots end in a hill");
}

/// SPEC 3.6: a jump (A in the lock's first tick) takes the pilot over
/// the bolt at 8 to 10 m (closer, under a strong pull, it may not: the
/// side step is the sure way).
#[test]
fn a_jump_in_the_last_instant_escapes_at_8_to_10_m() {
    let mut out = String::new();
    for skip in 0..4 {
        let sys = system(skip as u8, skip);
        let mut b = out_at(&sys, 0);
        let at = near_seam(&b);
        put_pilot(&mut b, at[0], at[1], 0);
        for r in [8, 9, 10] {
            for k in 0..4u16 {
                let hit = one_shot(&b, r * M, k * 0x4000 + 0x1000, Dodge::Jump).hit;
                out += &format!("site {skip} pull {} range {r} m: hit {hit}\n", b.g);
                assert!(!hit, "a jump at {r} m escapes (site {skip})");
            }
        }
    }
    println!("{out}");
}

/// SPEC 3.12: a bolt takes 8 + 4 x danger suit points; a drone has
/// 40 + 10 x danger points. Bolts to PILOT DOWN by danger and suit
/// level (SUIT_MAX), standing still.
#[test]
fn a_bolts_damage_by_danger_and_suit_level() {
    let mut table =
        String::from("danger  dmg  drone pts  bolts to PILOT DOWN by suit level 0 1 2\n");
    for danger in 0..4u8 {
        let want = BOLT_BASE + BOLT_PER * i32::from(danger);
        assert_eq!(drones::damage(danger), want);
        assert_eq!(
            drones::points(danger),
            (DRONE_HP + DRONE_HP_PER * i32::from(danger)) * 60
        );
        let mut counts = Vec::new();
        for shield in 0..3u8 {
            let sys = system(danger, 0);
            let mut b = out_at(&sys, shield);
            assert_eq!(b.pilot.suit, i32::from(SUIT_MAX[usize::from(shield)]) * 60);
            let at = near_seam(&b);
            put_pilot(&mut b, at[0], at[1], 0);
            let mut bolts = 0;
            loop {
                let before = b.pilot.suit;
                lone_drone(&mut b, 8 * M, 0x3000);
                let mut lost = false;
                for _ in 0..120 {
                    if planet::tick(&mut b, &In::default()) == Outcome::Lost {
                        lost = true;
                        break;
                    }
                    if b.pilot.suit < before {
                        break;
                    }
                }
                bolts += 1;
                if lost {
                    break;
                }
                assert_eq!(before - b.pilot.suit, want * 60, "one bolt's damage");
                assert!(bolts < 40);
            }
            let need = (i32::from(SUIT_MAX[usize::from(shield)]) + want - 1) / want;
            assert_eq!(bolts, need, "danger {danger} suit level {shield}");
            counts.push(bolts);
        }
        table += &format!(
            "{danger:6} {want:4} {:10}  {:?}\n",
            drones::points(danger) / 60,
            counts
        );
    }
    println!("{table}");
    let dir = shots().join("planet-g4");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("damage.txt"), &table).unwrap();
}

/// The cutter takes a drone down in `points / CUT_DPS` seconds; it
/// bursts (DRONE DOWN, a chunk of SPARES, three parts in the pool) and
/// stays down the rest of the day.
#[test]
fn the_cutter_downs_a_drone_and_it_stays_down_today() {
    let sys = system(2, 0);
    let mut b = out_at(&sys, 0);
    let at = near_seam(&b);
    put_pilot(&mut b, at[0], at[1], 0);
    lone_drone(&mut b, 8 * M, 0);
    b.drones[0].mode = PATROL;
    b.pilot.yaw = 0;
    let mut ticks = 0;
    while b.drones[0].alive() && ticks < 600 {
        planet::tick(&mut b, &input(button::X, 0, [0, 0]));
        ticks += 1;
    }
    assert_eq!(b.drones[0].mode, BURST);
    assert_eq!(b.kills, 1);
    assert_eq!(b.downed & 1, 1);
    assert_eq!(b.note, Some("DRONE DOWN"));
    assert!(b.sparks.live() >= 3);
    let spares = b
        .chunks
        .iter()
        .filter(|c| c.state != loose::CHUNK_NONE && c.good == mercator::planet::cutter::SPARES)
        .count();
    assert!(spares >= 1, "a chunk of SPARES");
    let want = drones::points(2) / mercator::planet::tune::CUT_DPS[0];
    println!("down in {ticks} ticks (points / CUT_DPS: {want})");
    assert!(ticks <= want + 2);
    for _ in 0..10 {
        planet::tick(&mut b, &In::default());
    }
    assert_eq!(b.drones[0].mode, GONE);
    // A new visit the same day keeps it down.
    let mut again = b;
    planet::populate(&mut again, b.downed);
    assert!(!again.drones[0].alive());
}

/// SPEC 3.13 item 9: a rolling boulder crushes a drone in its path (a
/// kill for the pilot).
#[test]
fn a_rolling_boulder_crushes_a_drone() {
    let mut done = 0;
    for skip in 0..12 {
        let sys = system((skip % 4) as u8, skip);
        let mut b = out_at(&sys, 0);
        let Some(k) = b.rocks.iter().position(|r| r.state == ROCK_REST) else {
            continue;
        };
        let r = b.rocks[k].at();
        lone_drone(&mut b, 2 * M, 0);
        b.drones[0].mode = PATROL;
        b.drones[0].p = [r[0], site::surface(&b.site, r[0], r[2]) + DRONE_UP, r[2]];
        b.rocks[k].state = ROCK_ROLL;
        b.rocks[k].v = [1 << 10, 0];
        // The pilot well away.
        let far = near_seam(&b);
        put_pilot(&mut b, far[0], far[1], 0);
        loose::rocks(&mut b);
        assert_eq!(b.drones[0].mode, BURST, "site {skip}: crushed");
        assert_eq!(b.kills, 1);
        done += 1;
    }
    assert!(done >= 6, "{done} sites had a boulder");
    // A drone well over the boulder is not crushed.
    let sys = system(0, 0);
    let mut b = out_at(&sys, 0);
    let k = b.rocks.iter().position(|r| r.state == ROCK_REST).unwrap();
    let r = b.rocks[k].at();
    lone_drone(&mut b, 2 * M, 0);
    b.drones[0].mode = PATROL;
    b.drones[0].p = [r[0], r[1] + 3 * M, r[2]];
    b.rocks[k].state = ROCK_ROLL;
    drones::crush(&mut b, r);
    assert_eq!(b.drones[0].mode, PATROL);
}

/// SPEC 3.13 item 9: a drone never leaves 40 m from its seam, whatever
/// the pilot does (he runs round, comes close, goes far), over 20 sites
/// and 2 minutes each.
#[test]
fn a_drone_never_leaves_40_m_from_its_seam() {
    let mut worst = 0;
    for skip in 0..20 {
        let sys = system((skip % 4) as u8, skip / 4);
        let mut b = out_at(&sys, 2);
        let s = b.places.seams[0];
        for t in 0..7200u32 {
            // Toward seam 0 for 40 s, round it, then away.
            let me = b.pilot.at();
            let (dx, dz) = match t / 2400 {
                0 => (i64::from(s[0] - me[0]), i64::from(s[1] - me[2])),
                1 => (-i64::from(s[1] - me[2]), i64::from(s[0] - me[0])),
                _ => (-i64::from(s[0] - me[0]), -i64::from(s[1] - me[2])),
            };
            let jump = if t % 97 == 0 { button::A } else { 0 };
            let push = push_to(&b, dx, dz);
            let out = planet::tick(&mut b, &input(jump, jump, push));
            for d in &b.drones {
                if d.mode == PATROL || d.mode == AIM || d.mode == COOL {
                    let home = b.places.seams[usize::from(d.home) % SEAMS];
                    let far = walker::apart([d.p[0], d.p[2]], home);
                    worst = worst.max(far);
                    assert!(far <= DRONE_LEASH, "site {skip} tick {t}: {far}");
                }
            }
            if out != Outcome::Stay {
                break;
            }
            b.pilot.suit = i32::from(b.pilot.suit_max) * 60;
        }
    }
    println!("farthest from its seam: {} cm", worst * 100 / M);
}

/// SPEC 3.10: the pool never runs over: 10,000 sparks of every kind put
/// in while it ticks; never more than 64 alive, every place in range.
#[test]
fn the_spark_pool_never_runs_over() {
    let sys = system(3, 0);
    let mut b = out_at(&sys, 0);
    let me = b.pilot.at();
    for n in 0..10_000u32 {
        let kind = (n % 14) as u8;
        let at: V3 = [
            me[0] + (n as i32 % 300 - 150) * M / 8,
            me[1] + (n as i32 % 7) * M,
            me[2] + (n as i32 % 211 - 105) * M / 8,
        ];
        sparks::add(&mut b.sparks, at, kind, (n % 16) as u8);
        if n % 3 == 0 {
            planet::tick(&mut b, &In::default());
        }
        assert!(b.sparks.live() <= SPARKS);
        assert!(usize::from(b.sparks.next) < SPARKS);
    }
    // Far outside the site's range: kept at the edge, no overflow.
    let mut p = Pool::EMPTY;
    for k in 0..200 {
        sparks::add(&mut p, [i32::MAX / 2, i32::MIN / 2, k], sparks::PART, 3);
    }
    assert_eq!(p.live(), SPARKS);
}

/// SPEC 3.10: no prop on the pad, by a seam, the wreck or a boulder's
/// place, or in water; one in `ROCK_EVERY` is a rock; the walk goes
/// round the rocks; jets lay the plants down for 3 s.
#[test]
fn the_props_keep_off_the_things_and_lie_down_under_jets() {
    let mut total = [0u32; 2];
    for skip in 0..12 {
        let sys = system((skip % 4) as u8, skip);
        let b = out_at(&sys, 0);
        for cz in -20..20 {
            for cx in -20..20 {
                let Some(p) = props::at_cell(&b, cx, cz) else {
                    continue;
                };
                let xz = [p.at[0], p.at[2]];
                assert!(walker::apart(xz, [0, 0]) >= PROP_CLEAR - M);
                for s in &b.places.seams {
                    assert!(walker::apart(xz, *s) >= PROP_NEAR_THING - M);
                }
                assert!(walker::apart(xz, b.places.wreck) >= PROP_NEAR_THING - M);
                assert!(p.at[1] >= b.site.wet);
                total[usize::from(p.rock)] += 1;
            }
        }
    }
    println!("plants {} rocks {}", total[0], total[1]);
    assert!(total[0] > 0 && total[1] > 0);
    // A plant blown on lies down, then gets up.
    let sys = system(0, 3);
    let mut b = out_at(&sys, 0);
    let (cx, cz, p) = (-20..20)
        .flat_map(|z| (-20..20).map(move |x| (x, z)))
        .find_map(|(x, z)| {
            props::at_cell(&b, x, z)
                .filter(|p| !p.rock)
                .map(|p| (x, z, p))
        })
        .expect("a plant");
    props::blow(&mut b, p.at, 0);
    assert_eq!(props::laid(&b, cx, cz), 256);
    for _ in 0..200 {
        props::tick(&mut b);
    }
    assert_eq!(props::laid(&b, cx, cz), 0);
    assert_eq!(props::flats_on(&b), 0);
}

/// SPEC 3.6: Y on foot spends 20 jet energy; the ring grows to 60 m in
/// a second and marks what it passes; without the energy, a note.
#[test]
fn the_scan_marks_what_its_ring_passes() {
    let sys = system(2, 1);
    let mut b = out_at(&sys, 0);
    let e = b.pilot.energy;
    planet::tick(&mut b, &input(button::Y, button::Y, [0, 0]));
    assert!(
        b.pilot.energy <= e - SCAN_COST + 60,
        "the scan took its energy"
    );
    assert!(b.scan_t <= 1);
    for _ in 0..SCAN_GROW {
        planet::tick(&mut b, &In::default());
    }
    assert_eq!(planet::scan_r(&b), SCAN_R);
    let me = b.pilot.at();
    for (k, s) in b.places.seams.iter().enumerate() {
        let near = walker::apart(*s, [b.scan_at[0], b.scan_at[2]]) <= SCAN_R;
        let marked = b.marks & (1u16 << k) != 0;
        assert_eq!(near, marked, "seam {k}");
    }
    let _ = me;
    // Away from the hatch (where the jets refill).
    let at = near_seam(&b);
    put_pilot(&mut b, at[0], at[1], 0);
    b.pilot.energy = 0;
    b.note = None;
    planet::tick(&mut b, &input(button::Y, button::Y, [0, 0]));
    assert_eq!(b.note, Some("JETS LOW: NO SCAN"));
}

// ---- The picture's cost -------------------------------------------------------------

/// The scene for the budget: the pilot beside the ship, the camera
/// turned `turn` and tilted by `tilt`, two drones bursting in front of
/// him, a third one aiming with its bolt in flight, the scan's ring out,
/// sparks everywhere.
fn heavy(base: &Below, turn: u16, tilt: i32) -> Below {
    let mut b = *base;
    let h = walker::hatch(&b);
    let yaw = b.yaw.wrapping_add(0x4000).wrapping_add(turn);
    put_pilot(&mut b, h[0], h[1], yaw);
    b.pilot.tilt = tilt;
    b.cam_goal = yaw;
    b.cam_yaw = yaw;
    planet::camera::foot_follow(&mut b, true);
    let me = b.pilot.at();
    let ground = b.site;
    let ahead = |r: i32, side: i32| {
        let (s, c) = (mercator::fixed::sin(yaw), mercator::fixed::cos(yaw));
        let x = me[0] + ((s * (r >> 4) + c * (side >> 4)) >> 10);
        let z = me[2] + ((c * (r >> 4) - s * (side >> 4)) >> 10);
        [x, site::surface(&ground, x, z) + DRONE_UP, z]
    };
    for (k, (r, side)) in [
        (7 * M, -2 * M),
        (9 * M, 3 * M),
        (14 * M, 0),
        (20 * M, -5 * M),
    ]
    .into_iter()
    .enumerate()
    {
        b.drones[k] = Drone {
            p: ahead(r, side),
            mode: PATROL,
            hp: drones::points(b.danger),
            yaw: planet::yaw_to(me[0] - ahead(r, side)[0], me[2] - ahead(r, side)[2]),
            ..Drone::NONE
        };
    }
    drones::burst(&mut b, 0);
    drones::burst(&mut b, 1);
    b.drones[2].mode = AIM;
    b.drones[2].t = 55;
    b.drones[2].lock = [me[0], me[1] + M, me[2]];
    b.drones[3].mode = COOL;
    b.drones[3].bolt = ahead(10 * M, -2 * M);
    b.drones[3].step = [40, -20, -80];
    b.drones[3].bolt_t = 20;
    b.scan_t = SCAN_GROW / 2;
    b.scan_at = me;
    b.marks = 0x1FF;
    for n in 0..30u8 {
        sparks::add(
            &mut b.sparks,
            ahead(3 * M + i32::from(n) * M / 4, 0),
            sparks::JET,
            n % 16,
        );
    }
    b
}

/// SPEC 3.13 item 10: the heaviest picture below (beside the ship,
/// tilted down, two drones bursting, a bolt, the scan, the pool full)
/// stays within LOW 300, MID 620, HIGH 960 triangles with no triangle
/// lost, over 16 camera turns, both tilts, every look. Rows to
/// `planet-g4/cost.txt`.
#[test]
fn the_heaviest_picture_keeps_its_budget() {
    const BELOW_TRIS: [u32; 3] = [300, 620, 960];
    let dir = shots().join("planet-g4");
    std::fs::create_dir_all(&dir).unwrap();
    let mut rows = String::from("look     detail tilt turn   tris  pixels objects\n");
    let mut worst: [(u32, u32, String); 3] = Default::default();
    for &look in &[0u8, 1, 2, 3, 5, 6, 7] {
        let mut d: Driver = below_in(look);
        common::below::land(&mut d, 8000);
        for _ in 0..=HATCH_OPEN + 2 {
            d.idle(1);
        }
        let base = d.app.below;
        let name = mercator::art::LOOK_NAMES[usize::from(look)].to_ascii_lowercase();
        for tilt in [0, mercator::planet::tune::FOOT_TILT[1]] {
            for k in 0..16u16 {
                let b = heavy(&base, k * 0x1000, tilt);
                for det in Detail::ALL {
                    d.app.render.begin();
                    planet::draw(&mut d.frame, &mut d.app.render, &b, det);
                    planet::panel(&mut d.frame, &b);
                    let st = d.app.render.stats;
                    let lv = planet::level(det);
                    assert_eq!(
                        st.dropped,
                        0,
                        "{name} {} turn {k}: lost triangles",
                        det.name()
                    );
                    assert!(
                        st.tris <= BELOW_TRIS[lv],
                        "{name} {} tilt {tilt} turn {k}: {} triangles",
                        det.name(),
                        st.tris
                    );
                    if st.tris > worst[lv].0 {
                        worst[lv] = (st.tris, st.pixels, format!("{name} tilt {tilt} turn {k}"));
                    }
                    if k % 4 == 0 {
                        rows += &format!(
                            "{name:8} {:6} {:4} {k:4} {:6} {:7} {:7}\n",
                            det.name(),
                            tilt / M,
                            st.tris,
                            st.pixels,
                            st.objects
                        );
                    }
                }
                if k == 2 && tilt != 0 {
                    let c = d.app.colours().to_vec();
                    save_frame(&dir.join(format!("{name}-heavy.png")), &d.frame, &c, 3);
                }
            }
        }
    }
    for (lv, w) in worst.iter().enumerate() {
        rows += &format!(
            "most at level {lv}: {} triangles, {} pixels ({})\n",
            w.0, w.1, w.2
        );
    }
    std::fs::write(dir.join("cost.txt"), &rows).unwrap();
    println!("{rows}");
}

/// BOARD-G3 item 7: the 30 s warning comes while the clock runs; when
/// the storm starts the note says what to do, never a countdown that is
/// over (also when the clock jumps straight past the warning).
#[test]
fn the_storm_note_says_what_to_do() {
    use mercator::planet::clock;
    let sys = system(0, 2);
    let mut base = out_at(&sys, 0);
    // No drones here: this is the clock's test.
    base.drones = [Drone::NONE; drones::N];
    let warn = i32::from(mercator::planet::tune::STORM_WARN) * 60;
    let mut b = base;
    b.note = None;
    b.since = (clock::total(&b) - warn - 1) as u16;
    let at = near_seam(&b);
    put_pilot(&mut b, at[0], at[1], 0);
    let mut notes = Vec::new();
    for _ in 0..warn + 10 {
        planet::tick(&mut b, &In::default());
        if let Some(n) = b.note.take() {
            notes.push((clock::left(&b), n));
        }
    }
    println!("{notes:?}");
    assert!(notes.iter().any(|(l, n)| *l > 0 && n.ends_with("IN 30 S")));
    assert!(notes.iter().all(|(l, n)| *l > 0 || !n.contains(" IN ")));
    assert!(notes.contains(&(0, clock::STORM_FOOT)));
    // Straight past the warning: only the storm's note.
    let mut b = base;
    b.note = None;
    b.since = clock::total(&b) as u16;
    put_pilot(&mut b, at[0], at[1], 0);
    planet::tick(&mut b, &In::default());
    assert_eq!(b.note, Some(clock::STORM_FOOT));
}

/// Step G4: the octant arctangent that turns a drone (and the pilot at
/// the start) keeps within 0.1 degree (19 of 65536) of the true angle,
/// with the nose of yaw `a` along (sin a, cos a).
#[test]
fn yaw_to_keeps_within_a_tenth_of_a_degree() {
    let mut worst = 0.0f64;
    let mut seed = 0x1234_5678u32;
    for k in 0..20_000 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let r = if k % 2 == 0 { 3_000_000 } else { 3_000 };
        let x = (seed >> 8) as i32 % r;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let z = (seed >> 8) as i32 % r;
        if x == 0 && z == 0 {
            continue;
        }
        let want = f64::from(x).atan2(f64::from(z)) / std::f64::consts::TAU * 65536.0;
        let got = f64::from(planet::yaw_to(x, z));
        let d = (got - want).rem_euclid(65536.0);
        worst = worst.max(d.min(65536.0 - d));
    }
    for (x, z, a) in [(0, 5, 0u16), (5, 0, 16384), (0, -5, 32768), (-5, 0, 49152)] {
        assert_eq!(planet::yaw_to(x, z), a, "({x}, {z})");
    }
    assert!(worst <= 19.0, "worst {worst:.1} of 65536");
    println!("yaw_to: worst {worst:.1} of 65536");
}

/// Step G4 (BOARD-G3): the picture's cost by detail level in every
/// planet scene, the most over 16 headings and every look: the 120 m
/// view and the descent at 40 m over the pad (the chase camera), the
/// take-off at 10 m, on foot at the hatch, and the drone fight (the
/// heavy scene above, both tilts). The air scenes are drawn twice, with
/// the site's drones and with none, to show that G4's drones, props and
/// sparks add nothing up there. Rows to `planet-g4/scenes.txt`.
#[test]
fn the_planet_scenes_cost_by_detail() {
    let dir = shots().join("planet-g4");
    std::fs::create_dir_all(&dir).unwrap();
    // (scene, level) -> (most triangles, most pixels, most objects)
    let mut most = std::collections::BTreeMap::<(usize, usize), (u32, u32, u32)>::new();
    let names = [
        "120 m view",
        "120 m, no drones",
        "descent 40 m",
        "40 m, no drones",
        "take-off 10 m",
        "on foot, hatch",
        "drone fight",
    ];
    for &look in &[0u8, 1, 2, 3, 5, 6, 7] {
        let mut d: Driver = below_in(look);
        common::below::land(&mut d, 8000);
        for _ in 0..=HATCH_OPEN + 2 {
            d.idle(1);
        }
        let foot = d.app.below;
        assert!(planet::on_foot(&foot));
        for k in 0..16u16 {
            let yaw = k * 0x1000;
            let mut shots: Vec<(usize, Below, Option<mercator::poly::Camera>)> = Vec::new();
            for (n, up, none) in [
                (0, 120, false),
                (1, 120, true),
                (2, 40, false),
                (3, 40, true),
                (4, 10, false),
            ] {
                let mut b = foot;
                b.phase = Phase::Fly;
                b.p = [0, (b.site.pad + up * M) << TO_WORLD, 0];
                b.yaw = yaw;
                if none {
                    b.drones = [Drone::NONE; drones::N];
                }
                let cam = planet::camera::chase_at(&b, planet::ship_pos(&b), yaw);
                shots.push((n, b, Some(cam)));
            }
            let mut hatch = foot;
            hatch.cam_goal = yaw;
            hatch.cam_yaw = yaw;
            hatch.pilot.yaw = yaw;
            planet::camera::foot_follow(&mut hatch, true);
            shots.push((5, hatch, None));
            for tilt in [0, mercator::planet::tune::FOOT_TILT[1]] {
                shots.push((6, heavy(&foot, yaw, tilt), None));
            }
            for (n, b, cam) in &shots {
                for det in Detail::ALL {
                    d.app.render.begin();
                    match cam {
                        Some(c) => planet::draw_from(&mut d.frame, &mut d.app.render, b, det, c),
                        None => planet::draw(&mut d.frame, &mut d.app.render, b, det),
                    }
                    planet::panel(&mut d.frame, b);
                    let st = d.app.render.stats;
                    assert_eq!(st.dropped, 0);
                    let e = most.entry((*n, planet::level(det))).or_default();
                    *e = (e.0.max(st.tris), e.1.max(st.pixels), e.2.max(st.objects));
                }
            }
        }
    }
    let mut rows =
        String::from("scene              LOW tris/objects  MID tris/objects  HIGH tris/objects\n");
    for (n, name) in names.iter().enumerate() {
        rows += &format!("{name:18}");
        for lv in 0..3 {
            let (t, _, o) = most[&(n, lv)];
            rows += &format!(" {t:9} / {o:4}  ");
        }
        rows += "\n";
    }
    for lv in 0..3 {
        assert_eq!(
            most[&(0, lv)],
            most[&(1, lv)],
            "drones cost nothing at 120 m"
        );
        assert_eq!(
            most[&(2, lv)],
            most[&(3, lv)],
            "drones cost nothing at 40 m"
        );
    }
    std::fs::write(dir.join("scenes.txt"), &rows).unwrap();
    println!("{rows}");
}

/// Step G4's pictures (JUNGLE, MID, pictures only, nothing asserted but
/// that they draw): a drone aiming (the red line), locked (white), its
/// bolt in flight, a burst, the scan's ring with its marks, plants laid
/// down by the hop jets, and the wreck's smoke column seen from the
/// descent. To `planet-g4/g4-*.png`.
#[test]
fn pictures_of_the_drones_the_scan_and_the_props() {
    let dir = shots().join("planet-g4");
    std::fs::create_dir_all(&dir).unwrap();
    let mut d: Driver = below_in(5);
    common::below::land(&mut d, 8000);
    for _ in 0..=HATCH_OPEN + 2 {
        d.idle(1);
    }
    let foot = d.app.below;
    let det = Detail::ALL[1];
    let shoot = |d: &mut Driver, b: &Below, name: &str| {
        d.app.render.begin();
        planet::draw(&mut d.frame, &mut d.app.render, b, det);
        planet::panel(&mut d.frame, b);
        let c = d.app.colours().to_vec();
        save_frame(&dir.join(format!("g4-{name}.png")), &d.frame, &c, 3);
    };
    // A lone drone 12 m in front of the pilot (14 m from its seam), a
    // little to the right.
    let mut b = foot;
    let yaw = b.pilot.yaw;
    let at = near_seam(&b);
    put_pilot(&mut b, at[0], at[1], yaw);
    b.cam_goal = yaw;
    b.cam_yaw = yaw;
    planet::camera::foot_follow(&mut b, true);
    lone_drone(&mut b, 12 * M, yaw.wrapping_add(0x0600));
    for _ in 0..20 {
        planet::tick(&mut b, &In::default());
    }
    assert_eq!(b.drones[0].mode, AIM);
    shoot(&mut d, &b, "aim");
    while !b.drones[0].locked() {
        planet::tick(&mut b, &In::default());
    }
    planet::tick(&mut b, &In::default());
    shoot(&mut d, &b, "lock");
    while b.drones[0].bolt_t == 0 {
        planet::tick(&mut b, &In::default());
    }
    planet::tick(&mut b, &In::default());
    shoot(&mut d, &b, "bolt");
    drones::burst(&mut b, 0);
    for _ in 0..3 {
        planet::tick(&mut b, &In::default());
    }
    shoot(&mut d, &b, "burst");
    for _ in 0..30 {
        planet::tick(&mut b, &In::default());
    }
    shoot(&mut d, &b, "burst-parts");
    // The scan, half grown, from 14 m short of seam 0, facing it (so
    // the marks over the seam and its guards are in view).
    let mut b = foot;
    let s0 = b.places.seams[0];
    let face = planet::yaw_to(s0[0] - at[0], s0[1] - at[1]);
    put_pilot(&mut b, at[0], at[1], face);
    b.cam_goal = face;
    b.cam_yaw = face;
    planet::camera::foot_follow(&mut b, true);
    planet::tick(&mut b, &input(button::Y, button::Y, [0, 0]));
    for _ in 0..SCAN_GROW / 2 {
        planet::tick(&mut b, &In::default());
    }
    shoot(&mut d, &b, "scan");
    for _ in 0..SCAN_GROW {
        planet::tick(&mut b, &In::default());
    }
    shoot(&mut d, &b, "scan-marks");
    // Plants laid down 8 to 24 m ahead of the pilot (as under jets), at
    // the same spot (the pad's own 20 m are kept clear of plants).
    let mut b = foot;
    put_pilot(&mut b, at[0], at[1], face);
    b.cam_goal = face;
    b.cam_yaw = face;
    planet::camera::foot_follow(&mut b, true);
    shoot(&mut d, &b, "plants");
    let me = b.pilot.at();
    let ahead = [
        me[0] + ((mercator::fixed::sin(face) * 8) >> 6),
        me[1],
        me[2] + ((mercator::fixed::cos(face) * 8) >> 6),
    ];
    props::blow(&mut b, ahead, 12 * M);
    assert!(props::flats_on(&b) > 0, "no plant laid in the picture");
    shoot(&mut d, &b, "plants-laid");
    // The wreck's smoke from the descent, 40 m over the pad, facing it.
    let mut b = foot;
    b.phase = Phase::Fly;
    b.p = [0, (b.site.pad + 40 * M) << TO_WORLD, 0];
    let w = b.places.wreck;
    b.yaw = planet::yaw_to(w[0], w[1]);
    let cam = planet::camera::chase_at(&b, planet::ship_pos(&b), b.yaw);
    for _ in 0..200 {
        planet::tick(&mut b, &In::default());
        b.p = [0, (b.site.pad + 40 * M) << TO_WORLD, 0];
        b.v = [0; 3];
    }
    d.app.render.begin();
    planet::draw_from(&mut d.frame, &mut d.app.render, &b, det, &cam);
    planet::panel(&mut d.frame, &b);
    let c = d.app.colours().to_vec();
    save_frame(&dir.join("g4-smoke.png"), &d.frame, &c, 3);
}
