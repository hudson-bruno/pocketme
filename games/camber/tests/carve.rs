//! The carve (L / R) and the sling: how a carve starts and which way,
//! that the stick never turns it back, that heat comes only from the
//! path's real turn (wiggling on a straight charges no faster than a
//! real carve, and a straight alone never reaches the first step), that
//! letting go pays by the heat's step (top speed and length) and settles
//! the slip without losing speed, that a tap costs nothing and pays
//! nothing, that a hard wall hit, the air and a crawl end it but a brush
//! does not, that its grip is off in the air, that B still brakes, and
//! the touch strip. And the reward: each step stronger and longer, the
//! stronger reward the owner chose (row B of 2026-11-22, stronger than
//! the reward of 2026-11-21 at every step), the heat steps where they
//! were, and the air brake stops the sling's push.

use camber::app::{Keys, TOUCH_STRIP, control};
use camber::fixed::turn;
use camber::race::{
    CARVE_AIR_TICKS, CARVE_END_SPEED, CARVE_SETTLE, CARVE_SETTLE_MIN, CARVE_START_SPEED,
    CARVE_WALL_SPEED, COUNT, Carve, Class, Ctl, HEAT_1, HEAT_2, HEAT_3, HOVER, Mode, Phase, REWARD,
    Race, SHIP_HW, Ship, heat_step, mps,
};
use camber::track::{FM, SEG_BITS, TRACKS, Track};
use game_kit::{Touch, button};

fn ctl(steer: i32, thrust: bool, carve: Carve) -> Ctl {
    Ctl {
        steer,
        thrust,
        brake_l: false,
        brake_r: false,
        carve,
    }
}

/// A time trial on `track` past the count.
fn race(track: u8) -> Race {
    let mut r = Race::new();
    r.setup(track, Class::Brisk, Mode::Trial, 0);
    for _ in 0..=COUNT {
        r.step(Some(Ctl::default()));
    }
    assert_eq!(r.phase, Phase::Run);
    r
}

/// The ship put in the middle of segment `seg`, `x` across, going
/// straight down the road at `v` m/s.
fn place(r: &mut Race, seg: usize, x: i32, v: i32) {
    let tr = r.tr();
    let sh = &mut r.ships[0];
    sh.s = ((seg as i32) << SEG_BITS) + (1 << (SEG_BITS - 1));
    sh.x = x;
    sh.h = HOVER;
    sh.vh = 0;
    sh.psi = 0;
    sh.rate = 0;
    sh.vl = 0;
    sh.vf = mps(v);
    sh.pitch = tr.at(sh.s).pitch;
    sh.lose_carve();
    sh.sling = 0;
    sh.sling_step = 0;
    sh.boost = 0;
    sh.ctl = Ctl::default();
}

/// How far the road turns over the `n` segments from `i` (right
/// positive).
fn bend(tr: &Track, i: usize, n: usize) -> i32 {
    turn(tr.seg(i).yaw, tr.seg(i + n).yaw)
}

/// The start of the longest stretch of road on `tr` that turns less than
/// a degree a segment, with no change of climb (no crest) on it.
fn straight(tr: &Track) -> usize {
    let n = (tr.len() >> SEG_BITS) as usize;
    let calm = |i: usize| {
        bend(tr, i, 1).abs() < 250
            && (tr.seg(i).pitch - tr.seg(i + 1).pitch).abs() < 100
            && tr.seg(i).kind & camber::track::GAP == 0
    };
    let mut best = (0, 0);
    for i in 0..n {
        let mut k = 0;
        while k < 30 && calm(i + k) {
            k += 1;
        }
        if k > best.1 {
            best = (i, k);
        }
    }
    assert!(
        best.1 >= 8,
        "{}: no straight ({} segments)",
        tr.name,
        best.1
    );
    best.0
}

/// The segment that starts the sharpest right-hand corner (over 8
/// segments) on `tr`.
fn corner(tr: &Track) -> usize {
    let n = (tr.len() >> SEG_BITS) as usize;
    (0..n).max_by_key(|&i| bend(tr, i, 8)).unwrap()
}

/// The way the ship really travels, angle units (a turn is 65,536),
/// right positive: the road's direction, the nose's from it, the slip's.
fn path(tr: &Track, sh: &Ship) -> f64 {
    let yaw = f64::from(tr.at(sh.s).yaw);
    let slip = f64::from(sh.vl).atan2(f64::from(sh.vf)) * 65_536.0 / std::f64::consts::TAU;
    yaw + f64::from(sh.psi) + slip
}

/// `b - a` in angle units, the short way round.
fn diff(a: f64, b: f64) -> f64 {
    let d = (b - a).rem_euclid(65_536.0);
    if d > 32_768.0 { d - 65_536.0 } else { d }
}

#[test]
fn a_press_at_speed_starts_a_carve_and_a_slow_one_does_not() {
    let mut r = race(0);
    let st = straight(r.tr());
    place(&mut r, st, 0, 50);
    r.step(Some(ctl(0, true, Carve::Right)));
    let sh = &r.ships[0];
    assert_eq!(sh.carve, 1);
    assert!(sh.heat < HEAT_1 / 20, "heat {}", sh.heat);
    // No kick: the nose starts turning smoothly (under a degree in the
    // first tick), so a press alone throws no speed away.
    assert!(sh.psi > 0 && sh.psi < 182, "psi {}", sh.psi);
    // Too slow: nothing, and holding on while speeding up does not start
    // one either (a carve needs a fresh press).
    let slow = CARVE_START_SPEED * 60 / FM - 5;
    place(&mut r, st, 0, slow);
    for _ in 0..90 {
        r.step(Some(ctl(0, true, Carve::Right)));
        assert_eq!(r.ships[0].carve, 0);
    }
    assert!(r.ships[0].vf > CARVE_START_SPEED, "it sped up");
    r.step(Some(ctl(0, true, Carve::Off)));
    r.step(Some(ctl(0, true, Carve::Right)));
    assert_eq!(r.ships[0].carve, 1, "a fresh press at speed starts it");
}

#[test]
fn the_way_comes_from_the_stick_then_from_the_button() {
    let mut r = race(0);
    let st = straight(r.tr());
    // (stick, buttons) -> way
    let cases = [
        (0, Carve::Left, -1),
        (0, Carve::Right, 1),
        (30, Carve::Left, -1),
        (-30, Carve::Right, 1),
        (256, Carve::Left, 1),
        (-256, Carve::Right, -1),
        (-120, Carve::Left, -1),
        (120, Carve::Both, 1),
        (-256, Carve::Both, -1),
        (0, Carve::Both, 0),
        (-256, Carve::Keep, 0),
        (256, Carve::Keep, 0),
    ];
    for (steer, c, way) in cases {
        place(&mut r, st, 0, 50);
        r.step(Some(ctl(steer, true, c)));
        assert_eq!(i32::from(r.ships[0].carve), way, "stick {steer} {c:?}");
    }
}

#[test]
fn one_carve_at_a_time_and_the_stick_never_turns_it_back() {
    let mut r = race(0);
    let st = straight(r.tr());
    let half = r.tr().at(((st as i32) + 2) << SEG_BITS).half * 256;
    // From the right side, carving left with the stick in the middle.
    place(&mut r, st, half - SHIP_HW - FM, 55);
    r.step(Some(ctl(0, true, Carve::Left)));
    assert_eq!(r.ships[0].carve, -1);
    let mut psi = r.ships[0].psi;
    for k in 0..30 {
        // The other button too, and the stick full the other way.
        let c = if k % 2 == 0 { Carve::Both } else { Carve::Left };
        r.step(Some(ctl(256, true, c)));
        let sh = &r.ships[0];
        if sh.carve == 0 {
            break;
        }
        assert_eq!(sh.carve, -1, "the other button starts nothing new");
        assert!(sh.rate <= 0, "the nose never turns back: rate {}", sh.rate);
        // Relative to the road the nose turns left or stays (the road
        // itself turns a little: allow it).
        assert!(sh.psi <= psi + 40, "tick {k}: psi {} after {psi}", sh.psi);
        psi = sh.psi;
    }
}

/// Heat is real turning: at every tick it is no more than how far the
/// way the ship travels has turned since its lowest point in the carve
/// (heat cannot go below nothing, so a turn the wrong way first is not
/// owed back) plus how far the road under it turned the carve's way (a
/// ship sliding wide through a corner still rides the corner), whatever
/// the stick does; and
/// wiggling the stick charges no faster than holding it full into the
/// carve. On a straight no pattern gets past the first step before the
/// wall or the road's edge would (the path can only point so far across
/// the road; crossing the whole road at full lock from one edge can just
/// reach step 1, which the lap tables show does not pay).
#[test]
fn heat_needs_real_turning_and_wiggling_does_not_charge_faster() {
    type Pattern = fn(u32) -> i32;
    let patterns: [(&str, Pattern); 6] = [
        ("middle", |_| 0),
        ("full in", |_| 256),
        ("full out", |_| -256),
        ("wiggle 4", |k| {
            if (k / 4).is_multiple_of(2) { 256 } else { -256 }
        }),
        ("wiggle 1", |k| if k.is_multiple_of(2) { 256 } else { -256 }),
        ("wiggle 9", |k| {
            if (k / 9).is_multiple_of(2) { 256 } else { -256 }
        }),
    ];
    for t in 0..TRACKS.len() as u8 {
        let tr = &TRACKS[usize::from(t)];
        let st = straight(tr);
        let co = corner(tr);
        // Heat a tick, for each pattern, on the straight (from the left
        // edge, carving right across the road) and in the corner.
        let mut full_in: Vec<i32> = Vec::new();
        for (where_, seg) in [("straight", st), ("corner", co)] {
            for (name, p) in patterns {
                let mut r = race(t);
                let half = tr.at(((seg as i32) + 1) << SEG_BITS).half * 256;
                let x = if where_ == "straight" {
                    -half + SHIP_HW + FM
                } else {
                    0
                };
                place(&mut r, seg, x, 55);
                let start = path(tr, &r.ships[0]);
                let yaw0 = f64::from(tr.at(r.ships[0].s).yaw);
                let mut best = 0;
                let mut low = 0.0f64;
                let mut heats = Vec::new();
                for k in 0..120u32 {
                    let c = if k == 0 { 0 } else { p(k) };
                    r.step(Some(ctl(c, true, Carve::Right)));
                    let sh = &r.ships[0];
                    if sh.carve == 0 {
                        break;
                    }
                    let turned = diff(start, path(tr, sh));
                    let road = diff(yaw0, f64::from(tr.at(sh.s).yaw)).max(0.0);
                    assert!(
                        f64::from(sh.heat) <= turned - low + road + 60.0,
                        "{} {where_} {name} tick {k}: heat {} but the path turned {:.0} (road {road:.0})",
                        tr.name,
                        sh.heat,
                        turned - low
                    );
                    low = low.min(turned);
                    heats.push(sh.heat);
                    best = best.max(sh.heat);
                }
                if where_ == "straight" {
                    assert!(
                        best < HEAT_2,
                        "{} straight {name}: heat {best} reached step {}",
                        tr.name,
                        heat_step(best)
                    );
                }
                if name == "full in" {
                    full_in = heats.clone();
                } else if name.starts_with("wiggle") {
                    for (k, h) in heats.iter().enumerate() {
                        if let Some(f) = full_in.get(k) {
                            assert!(
                                *h <= f + 60,
                                "{} {where_} {name} tick {k}: heat {h} above full in {f}",
                                tr.name
                            );
                        }
                    }
                }
                eprintln!(
                    "{:8} {where_:8} {name:9} top heat {best:5} (step {}) over {} ticks",
                    tr.name,
                    heat_step(best),
                    heats.len()
                );
            }
        }
    }
}

#[test]
fn letting_go_pays_a_sling_by_the_step_and_nothing_below_the_first() {
    let mut r = race(0);
    let st = straight(r.tr());
    let cases = [
        (HEAT_1 - 1, 0u8),
        (0, 0),
        (HEAT_1, 1),
        (HEAT_2 - 1, 1),
        (HEAT_2, 2),
        (HEAT_3, 3),
    ];
    for (heat, step) in cases {
        place(&mut r, st, 0, 60);
        r.step(Some(ctl(0, true, Carve::Right)));
        r.ships[0].heat = heat;
        r.step(Some(ctl(0, true, Carve::Off)));
        let sh = &r.ships[0];
        assert_eq!(sh.carve, 0);
        assert_eq!(sh.heat, 0);
        if step == 0 {
            assert_eq!(sh.sling, 0, "heat {heat}: no sling");
        } else {
            // One tick of it is already used.
            assert_eq!(sh.sling_step, step);
            assert_eq!(sh.sling, REWARD.ticks[usize::from(step - 1)] - 1);
        }
    }
    // The sling pushes above top speed; a longer one goes further.
    let mut gone = Vec::new();
    let mut tops = Vec::new();
    for step in 0..=3u8 {
        place(&mut r, st, 0, 72);
        let s0 = r.ships[0].s;
        if step > 0 {
            r.ships[0].sling = REWARD.ticks[usize::from(step - 1)];
            r.ships[0].sling_step = step;
        }
        let mut top = 0;
        for _ in 0..90 {
            r.step(Some(ctl(0, true, Carve::Off)));
            top = top.max(r.ships[0].vf);
        }
        let d = r.ships[0].s - s0;
        eprintln!(
            "sling step {step}: top {} m/s x100, 90 ticks went {} cm",
            top * 6000 / FM,
            d * 100 / FM
        );
        if step == 0 {
            assert!(
                top <= Class::Brisk.top() + FM / 100,
                "no sling: no more than top"
            );
        } else {
            assert!(top > Class::Brisk.top() * 21 / 20, "step {step}: top {top}");
        }
        gone.push(d);
        tops.push(top);
    }
    assert!(
        gone[0] < gone[1] && gone[1] < gone[2] && gone[2] < gone[3],
        "{gone:?}"
    );
    // Each step is stronger, not only longer: a higher top speed and more
    // ticks (a sling as long as the first one at each step's own strength
    // still goes further at the higher steps).
    assert!(
        tops[1] < tops[2] && tops[2] < tops[3],
        "tops by step {tops:?}"
    );
    let rw = REWARD;
    assert!(rw.ticks[0] < rw.ticks[1] && rw.ticks[1] < rw.ticks[2]);
    assert!(rw.top[0] < rw.top[1] && rw.top[1] < rw.top[2]);
    let mut short = Vec::new();
    for step in 1..=3u8 {
        place(&mut r, st, 0, 72);
        let s0 = r.ships[0].s;
        r.ships[0].sling = REWARD.ticks[0];
        r.ships[0].sling_step = step;
        for _ in 0..90 {
            r.step(Some(ctl(0, true, Carve::Off)));
        }
        short.push(r.ships[0].s - s0);
    }
    assert!(short[0] < short[1] && short[1] < short[2], "{short:?}");
    // And the sling pushes even without A.
    place(&mut r, st, 0, 60);
    r.ships[0].sling = REWARD.ticks[0];
    let v0 = r.ships[0].vf;
    for _ in 0..10 {
        r.step(Some(ctl(0, false, Carve::Off)));
    }
    assert!(r.ships[0].vf > v0);
}

/// Carving right at 60 m/s next to the right-hand wall of a straight,
/// the nose (and path) turned `deg` tenths of a degree into it: the
/// carve's state after the first tick that touches the wall, and how fast
/// it came across.
fn into_the_wall(tenths: i32) -> (i8, i32, i32) {
    let mut r = race(0);
    let st = straight(r.tr());
    let half = r.tr().at(((st as i32) + 1) << SEG_BITS).half * 256;
    place(&mut r, st, half - SHIP_HW - FM / 10, 60);
    r.step(Some(ctl(0, true, Carve::Right)));
    assert_eq!(r.ships[0].carve, 1);
    let sh = &mut r.ships[0];
    sh.x = half - SHIP_HW - FM / 20;
    sh.psi = tenths * 65_536 / 3600;
    sh.rate = 0;
    sh.vl = 0;
    sh.vf = mps(60);
    sh.heat = HEAT_2;
    let shield = sh.shield;
    let across = (i64::from(mps(60)) * i64::from(camber::fixed::sin(sh.psi as u16))) >> 14;
    for _ in 0..30 {
        // Full out of the carve: the nose turns the least into the wall.
        r.step(Some(ctl(-256, true, Carve::Right)));
        if r.ships[0].shield < shield {
            let sh = &r.ships[0];
            return (sh.carve, sh.heat, across as i32);
        }
    }
    panic!("{tenths}: never touched the wall");
}

#[test]
fn a_hard_wall_hit_ends_the_carve_and_a_brush_keeps_it() {
    // Hard: 20 degrees into the wall, about 20 m/s across.
    let (carve, heat, across) = into_the_wall(200);
    assert!(across > CARVE_WALL_SPEED * 3, "across {across}");
    assert_eq!(carve, 0, "a hard hit ends it");
    assert_eq!(heat, 0, "and its heat is lost");
    // A brush: 2 degrees, about 2 m/s across: the carve goes on, with
    // most of its heat (the wall's push takes a little away).
    let (carve, heat, across) = into_the_wall(20);
    assert!(across < CARVE_WALL_SPEED, "across {across}");
    assert_eq!(carve, 1, "a brush keeps it");
    assert!(heat > HEAT_2 * 3 / 4, "heat {heat} after the brush");
    // A hard hit, then letting go: nothing to pay.
    let mut r = race(0);
    let st = straight(r.tr());
    let half = r.tr().at(((st as i32) + 1) << SEG_BITS).half * 256;
    place(&mut r, st, half - SHIP_HW - FM / 20, 60);
    r.step(Some(ctl(0, true, Carve::Right)));
    r.ships[0].psi = 200 * 65_536 / 3600;
    r.ships[0].heat = HEAT_2;
    for _ in 0..5 {
        r.step(Some(ctl(0, true, Carve::Right)));
    }
    assert_eq!(r.ships[0].carve, 0);
    assert!(
        r.ships[0].shield < camber::race::SHIELD_MAX,
        "it was the wall"
    );
    r.step(Some(ctl(0, true, Carve::Off)));
    assert_eq!(r.ships[0].sling, 0);
}

/// The ship's speed along its path (not the nose).
fn speed(sh: &Ship) -> f64 {
    f64::from(sh.vf).hypot(f64::from(sh.vl))
}

/// Letting go of a carve costs no more speed than coasting: the slip it
/// leaves is turned into the path (the settle), not scrubbed off.
#[test]
fn letting_go_of_a_carve_costs_no_speed() {
    for t in 0..TRACKS.len() as u8 {
        let mut r = race(t);
        let tr = r.tr();
        let st = straight(tr);
        let half = tr.at(((st as i32) + 1) << SEG_BITS).half * 256;
        place(&mut r, st, -half + SHIP_HW + FM, 60);
        // A carve right held for 24 ticks with the stick in the middle,
        // then let go and coast (no thrust, stick in the middle).
        for _ in 0..24 {
            r.step(Some(ctl(0, true, Carve::Right)));
        }
        let sh = &r.ships[0];
        assert_eq!(sh.carve, 1);
        assert!(
            sh.vl < -mps(1),
            "{}: the carve left slip {}",
            tr.name,
            sh.vl
        );
        r.step(Some(ctl(0, false, Carve::Off)));
        assert_eq!(r.ships[0].settle, CARVE_SETTLE - 1);
        let v0 = speed(&r.ships[0]);
        for _ in 0..u32::from(CARVE_SETTLE) {
            r.step(Some(ctl(0, false, Carve::Off)));
        }
        let sh = &r.ships[0];
        let v1 = speed(sh);
        // Coasting alone loses 1/512 a tick.
        let coast = v0 * (1.0 - 1.0 / 512.0f64).powi(i32::from(CARVE_SETTLE));
        eprintln!(
            "{}: let go at {:.1} m/s, {} ticks later {:.1} (coasting {:.1}), slip left {:.2} m/s",
            tr.name,
            v0 * 60.0 / f64::from(FM),
            CARVE_SETTLE,
            v1 * 60.0 / f64::from(FM),
            coast * 60.0 / f64::from(FM),
            f64::from(sh.vl) * 60.0 / f64::from(FM)
        );
        assert!(
            v1 > coast * 0.995,
            "{}: {v1} against coasting {coast}",
            tr.name
        );
        assert!(sh.vl.abs() < mps(1), "{}: slip {} left", tr.name, sh.vl);
    }
}

/// Tapping L or R (with the stick in the middle) pays nothing and costs
/// little speed at any rhythm: each tap turns the nose a little, like a
/// touch of the stick, and only that turn's slip is scrubbed. (With the
/// old kick, tapping every other tick fell from 259 to about 40 km/h.)
#[test]
fn a_tap_costs_no_speed_and_pays_nothing() {
    let mut r = race(0);
    let st = straight(r.tr());
    place(&mut r, st, 0, 72);
    for _ in 0..30 {
        r.step(Some(ctl(0, true, Carve::Off)));
    }
    let clean = speed(&r.ships[0]);
    for every in 2..=8u32 {
        for len in 1..every {
            place(&mut r, st, 0, 72);
            for k in 0..30u32 {
                let c = if k % every < len {
                    Carve::Right
                } else {
                    Carve::Off
                };
                r.step(Some(ctl(0, true, c)));
                assert_eq!(r.ships[0].sling, 0, "a tap pays no sling");
            }
            let v = speed(&r.ships[0]);
            assert!(
                v > clean * 0.96,
                "tap {len} of every {every}: {v} against {clean}"
            );
        }
    }
    // A press shorter than CARVE_SETTLE_MIN leaves no settle.
    place(&mut r, st, 0, 72);
    for _ in 0..CARVE_SETTLE_MIN - 1 {
        r.step(Some(ctl(0, true, Carve::Right)));
    }
    r.step(Some(ctl(0, true, Carve::Off)));
    assert_eq!(r.ships[0].settle, 0);
}

/// Off the ground the carve keeps going (a short hop) but its grip is the
/// air's: the slip is not turned back towards the nose.
#[test]
fn the_carve_has_no_grip_in_the_air() {
    let mut r = race(0);
    let st = straight(r.tr());
    place(&mut r, st, 0, 60);
    r.step(Some(ctl(0, true, Carve::Right)));
    let sh = &mut r.ships[0];
    sh.h = HOVER + FM * 2;
    sh.vh = FM / 20;
    sh.rate = 0;
    sh.vl = -mps(6);
    let vl0 = sh.vl;
    r.step(Some(ctl(-256, true, Carve::Right)));
    let sh = &r.ships[0];
    assert_eq!(sh.carve, 1);
    // The air's grip takes 1/200; the carve's would take 1/20.
    assert!(sh.vl < vl0 * 99 / 100, "slip {} from {vl0}", sh.vl);
}

#[test]
fn the_air_ends_the_carve_but_a_short_hop_does_not() {
    let mut r = race(0);
    let st = straight(r.tr());
    let mut seen = [false, false];
    // Thrown up harder and harder (as off a crest): the carve ends exactly
    // when the ship has been off the ground more than CARVE_AIR_TICKS
    // ticks in a row, and then its heat is lost.
    for up in 0..40 {
        place(&mut r, st, -FM * 3, 60);
        r.step(Some(ctl(0, true, Carve::Right)));
        r.ships[0].heat = HEAT_1;
        r.ships[0].vh = up * FM / 400;
        let mut off = 0u8;
        let mut most = 0u8;
        let mut gone = false;
        for _ in 0..40 {
            let grounded = r.ships[0].h < HOVER + FM * 3 / 10;
            off = if grounded { 0 } else { off + 1 };
            most = most.max(off);
            // Full out of the carve: the widest arc, no wall in reach.
            r.step(Some(ctl(-256, true, Carve::Right)));
            if r.ships[0].carve == 0 {
                gone = true;
                break;
            }
        }
        assert_eq!(
            gone,
            most > CARVE_AIR_TICKS,
            "thrown up {up}: {most} ticks off"
        );
        seen[usize::from(gone)] = true;
        if gone {
            assert_eq!(r.ships[0].heat, 0);
            r.step(Some(ctl(0, true, Carve::Off)));
            assert_eq!(r.ships[0].sling, 0, "nothing to pay after the air");
        } else {
            assert!(r.ships[0].heat >= HEAT_1);
        }
    }
    assert!(
        seen[0] && seen[1],
        "both a short hop and a long one were tried"
    );
    // A press in the air starts nothing.
    place(&mut r, st, 0, 60);
    r.ships[0].h = HOVER + FM * 2;
    r.step(Some(ctl(0, true, Carve::Right)));
    assert_eq!(r.ships[0].carve, 0);
}

#[test]
fn a_crawl_ends_the_carve() {
    let mut r = race(0);
    let st = straight(r.tr());
    place(&mut r, st, 0, 40);
    r.step(Some(ctl(0, true, Carve::Right)));
    r.ships[0].heat = HEAT_1;
    let mut ticks = 0;
    // Both brakes, no thrust.
    while r.ships[0].carve != 0 && ticks < 300 {
        let mut c = ctl(0, false, Carve::Right);
        c.brake_l = true;
        c.brake_r = true;
        r.step(Some(c));
        ticks += 1;
    }
    assert_eq!(r.ships[0].carve, 0);
    assert!(r.ships[0].vf < CARVE_END_SPEED);
    assert_eq!(r.ships[0].heat, 0);
}

#[test]
fn b_and_the_left_trigger_brake_on_both_sides_and_l_r_do_not() {
    let keys = |held: u32, trig: (u16, u16)| Keys {
        held,
        trig,
        ..Keys::default()
    };
    for k in [keys(button::B, (0, 0)), keys(0, (1023, 0))] {
        let c = control(&k);
        assert!(c.brake_l && c.brake_r && c.carve == Carve::Off);
    }
    for (b, want) in [
        (button::L, Carve::Left),
        (button::R, Carve::Right),
        (button::L | button::R, Carve::Both),
    ] {
        let c = control(&keys(b, (0, 0)));
        assert!(!c.brake_l && !c.brake_r, "{want:?} is no brake");
        assert_eq!(c.carve, want);
    }
    // In the race: B slows the ship much more than coasting; holding a
    // carve keeps nearly all the speed.
    let mut r = race(0);
    let st = straight(r.tr());
    // The speed (along the path, not the nose).
    let run = |r: &mut Race, c: Ctl| {
        place(r, st, 0, 60);
        for _ in 0..30 {
            r.step(Some(c));
        }
        let sh = &r.ships[0];
        f64::from(sh.vf).hypot(f64::from(sh.vl)) as i32
    };
    let coast = run(&mut r, ctl(0, false, Carve::Off));
    let mut b = ctl(0, false, Carve::Off);
    b.brake_l = true;
    b.brake_r = true;
    let braked = run(&mut r, b);
    let carved = run(&mut r, ctl(0, false, Carve::Right));
    assert_eq!(r.ships[0].carve, 1);
    assert!(braked < coast * 3 / 4, "braked {braked} coast {coast}");
    // (Half a second of carving costs a few hundredths: the drag.)
    assert!(carved > coast * 94 / 100, "carved {carved} coast {coast}");
}

fn finger(x: i16, y: i16) -> Keys {
    Keys {
        touch: Some(Touch {
            x,
            y,
            down: true,
            started: false,
            ended: false,
        }),
        ..Keys::default()
    }
}

#[test]
fn the_touch_strip_starts_holds_and_lets_go_of_a_carve() {
    let low = TOUCH_STRIP + 5;
    // Far left / far right in the strip: a carve that way; the middle of
    // the strip only keeps one going; above the strip: none.
    assert_eq!(control(&finger(10, low)).carve, Carve::Left);
    assert_eq!(control(&finger(150, low)).carve, Carve::Right);
    assert_eq!(control(&finger(80, low)).carve, Carve::Keep);
    assert_eq!(control(&finger(10, 60)).carve, Carve::Off);
    assert_eq!(control(&finger(150, TOUCH_STRIP)).carve, Carve::Off);
    // A finger is always thrust and steers to itself; it never brakes.
    let c = control(&finger(10, low));
    assert!(c.thrust && c.steer == -256 && !c.brake_l && !c.brake_r);
    // In the race: the middle of the strip starts nothing; far right
    // starts it; sliding back to the middle keeps it; lifting the finger
    // pays the sling.
    let mut r = race(0);
    let st = straight(r.tr());
    place(&mut r, st, 0, 60);
    r.step(Some(control(&finger(80, low))));
    assert_eq!(r.ships[0].carve, 0);
    r.step(Some(control(&finger(150, low))));
    assert_eq!(r.ships[0].carve, 1);
    for _ in 0..10 {
        r.step(Some(control(&finger(80, low))));
        assert_eq!(r.ships[0].carve, 1);
    }
    r.ships[0].heat = HEAT_1;
    r.step(Some(control(&Keys::default())));
    assert_eq!(r.ships[0].carve, 0);
    assert_eq!(r.ships[0].sling_step, 1);
    // Moving up out of the strip lets go too.
    place(&mut r, st, 0, 60);
    r.step(Some(control(&finger(10, low))));
    assert_eq!(r.ships[0].carve, -1);
    r.step(Some(control(&finger(10, 60))));
    assert_eq!(r.ships[0].carve, 0);
}

#[test]
fn the_reward_grows_by_the_step_and_is_stronger_than_last_round() {
    let rw = REWARD;
    assert!(rw.top[0] > 1024, "the first step pays");
    assert!(
        rw.top[0] < rw.top[1] && rw.top[1] < rw.top[2],
        "tops {:?}",
        rw.top
    );
    assert!(
        rw.ticks[0] < rw.ticks[1] && rw.ticks[1] < rw.ticks[2],
        "ticks {:?}",
        rw.ticks
    );
    assert!(rw.pull > 0);
    // The owner's choice of 2026-11-22 (row B): 123 / 134 / 146 % for
    // 1.5 / 2.25 / 3.25 s, 1/30 of the gap a tick.
    assert_eq!(rw.top, [1_260, 1_370, 1_500]);
    assert_eq!(rw.ticks, [90, 135, 195]);
    assert_eq!(rw.pull, 30);
    // Stronger at every step than the reward of 2026-11-21 (110 / 115 /
    // 121 % for 30 / 45 / 65 ticks): a higher top and a longer push.
    let last_round = ([1_130, 1_180, 1_240], [30u8, 45, 65]);
    for k in 0..3 {
        assert!(
            rw.top[k] > last_round.0[k] && rw.ticks[k] > last_round.1[k],
            "step {}",
            k + 1
        );
    }
    // The heat steps stay at 30, 55 and 85 degrees of real turn: the
    // third step is no easier than before.
    assert_eq!(
        [HEAT_1, HEAT_2, HEAT_3],
        [30, 55, 85].map(|d| d * 65_536 / 360)
    );
}

/// The air brake beats the sling at every step: while B is held a sling
/// does not push (its ticks run on), so a ship that brakes in a sling
/// slows exactly as one without a sling, and the sling still ends on
/// time. Before this rule the reward (1/30 of the gap a tick) out-pulled
/// the brake (1/90 of the speed) and threw the ship into the next wall
/// (review 2026-11-22, play fault 3).
#[test]
fn the_air_brake_beats_the_sling_at_every_step() {
    let mut r = race(0);
    let st = straight(r.tr());
    let brake = Ctl {
        thrust: true,
        brake_l: true,
        brake_r: true,
        ..Ctl::default()
    };
    let rw = REWARD;
    for step in 1..=3u8 {
        let mut speeds = [0; 2];
        for (k, slinging) in [false, true].into_iter().enumerate() {
            place(&mut r, st, 0, 72);
            if slinging {
                r.ships[0].sling = rw.ticks[usize::from(step - 1)];
                r.ships[0].sling_step = step;
            }
            for _ in 0..30 {
                r.step(Some(brake));
            }
            speeds[k] = r.ships[0].vf;
        }
        assert_eq!(
            speeds[0], speeds[1],
            "step {step}: braking in a sling slows as without one"
        );
        assert!(speeds[1] < mps(72), "step {step}: the brake works");
        // Its ticks ran on: 30 are gone.
        assert_eq!(
            r.ships[0].sling,
            rw.ticks[usize::from(step - 1)] - 30,
            "step {step}"
        );
        // Let go of B: the rest of the sling pushes again.
        let v = r.ships[0].vf;
        for _ in 0..10 {
            r.step(Some(ctl(0, true, Carve::Off)));
        }
        assert!(r.ships[0].vf > v, "step {step}: pushes after B");
    }
}
