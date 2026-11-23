//! The rules of the race, one at a time, on a [`Race`] set up by hand:
//! the countdown, the top speed in bends, the brake and the boost
//! battery, the grass, crashes, laps, places, the top three going on,
//! the courses getting harder, and a long race that never ends.

mod common;

use common::Driver;
use game_kit::button;
use hairpin::app::{CARD_WAIT, Scene};
use hairpin::race::{
    BOOST_COST, CHARGE_MAX, CHARGE_START, COUNTDOWN, GRIP, IN_WATER, KARTS, LAPS, OFF_ROAD, Pad,
    Race, RaceEvent, SUB, V_BOOST, V_MAX, X_LIMIT, X_ONE,
};
use hairpin::track::{COURSES, SEG};

const GAS: Pad = Pad {
    throttle: 256,
    brake: 0,
    steer: 0,
    boost: false,
};

/// Course `c` set up, past the countdown.
fn going(c: u8) -> Box<Race> {
    let mut r = Box::new(Race::empty());
    r.reset(c);
    r.tick = COUNTDOWN + 1;
    while r.take_event().is_some() {}
    r
}

/// The player's kart at segment `seg`, `x` across, speed `ratio` (in
/// 1/1024 of the top speed).
fn place_at(r: &mut Race, seg: u32, x: i32, ratio: i32) {
    r.karts[0].prog = seg as i32 * SEG * SUB;
    r.karts[0].x = x;
    r.karts[0].v = ratio * V_MAX / 1024;
}

fn events(r: &mut Race) -> Vec<RaceEvent> {
    let mut v = Vec::new();
    while let Some(e) = r.take_event() {
        v.push(e);
    }
    v
}

#[test]
fn nothing_moves_before_the_lights_go_green() {
    let mut r = Box::new(Race::empty());
    r.reset(0);
    let start = r.karts;
    for _ in 0..COUNTDOWN - 1 {
        r.tick(&GAS);
    }
    assert_eq!(r.karts, start);
    assert!(r.counting());
    r.tick(&GAS);
    assert_eq!(events(&mut r), vec![RaceEvent::Go]);
    r.tick(&GAS);
    assert!(r.karts[0].prog > start[0].prog);
    // The player starts last, behind the four rivals.
    assert_eq!(r.place_now(), KARTS as u8);
    assert_eq!(r.charge, CHARGE_START);
}

#[test]
fn a_kart_gets_up_to_top_speed_on_the_straight() {
    let mut r = going(0);
    place_at(&mut r, 0, 0, 0);
    for _ in 0..240 {
        r.karts[0].x = 0;
        r.tick(&GAS);
    }
    assert!(r.karts[0].v >= V_MAX * 95 / 100, "{}", r.karts[0].v);
    assert!(r.karts[0].v <= V_MAX);
}

/// The SUMMIT's first hairpin (a right bend), in its sharpest part.
fn in_the_hairpin() -> (Box<Race>, u32, i32) {
    let r = going(2);
    let c = &COURSES[2];
    let seg = (0..c.segs)
        .find(|&i| c.seg(i).curve16 == 15 * 16 && c.seg(i + 4).curve16 == 15 * 16)
        .unwrap();
    (r, seg, c.seg(seg).curve16)
}

#[test]
fn a_bend_has_a_top_speed_and_steering_holds_the_kart_below_it() {
    let (mut r, seg, curve) = in_the_hairpin();
    // The fastest the kart can take this bend: GRIP / bend.
    let top = GRIP / curve;
    assert!(top < 1024, "a hairpin needs the brake: {top}");
    let full_right = Pad {
        throttle: 0,
        brake: 0,
        steer: 256,
        boost: false,
    };
    // Below it: steering into the bend pulls the kart inwards.
    place_at(&mut r, seg, 0, top - 80);
    for _ in 0..3 {
        r.tick(&full_right);
    }
    assert!(r.karts[0].x > 0, "held the bend: {}", r.karts[0].x);
    // Above it: even full steering cannot stop the slide to the outside.
    let (mut r, seg, _) = in_the_hairpin();
    place_at(&mut r, seg, 0, top + 80);
    for _ in 0..3 {
        r.tick(&full_right);
    }
    assert!(r.karts[0].x < 0, "slid out: {}", r.karts[0].x);
    // And with no steering at all, out much faster.
    let (mut r, seg, _) = in_the_hairpin();
    place_at(&mut r, seg, 0, top + 80);
    for _ in 0..3 {
        r.tick(&Pad::default());
    }
    assert!(r.karts[0].x < -1000, "{}", r.karts[0].x);
}

#[test]
fn braking_and_rolling_charge_the_boost_battery() {
    let mut r = going(0);
    place_at(&mut r, 1, 0, 1024);
    r.charge = 0;
    let brake = Pad {
        brake: 256,
        ..Pad::default()
    };
    r.tick(&brake);
    r.tick(&brake);
    assert_eq!(r.charge, 4, "2 a tick while braking");
    assert!(r.karts[0].v < V_MAX);
    assert!(r.karts[0].braking);
    // Rolling (no throttle, no brake) above a third of top speed: 1 a tick.
    place_at(&mut r, 1, 0, 1024);
    r.tick(&Pad::default());
    assert_eq!(r.charge, 5);
    // Not when slow.
    place_at(&mut r, 1, 0, 200);
    r.tick(&Pad::default());
    assert_eq!(r.charge, 5);
    // Never above full.
    r.charge = CHARGE_MAX;
    place_at(&mut r, 1, 0, 1024);
    r.tick(&brake);
    assert_eq!(r.charge, CHARGE_MAX);
}

#[test]
fn the_boost_goes_past_top_speed_and_spends_the_battery() {
    let mut r = going(0);
    let boost = Pad { boost: true, ..GAS };
    place_at(&mut r, 0, 0, 1024);
    r.charge = CHARGE_MAX;
    for _ in 0..40 {
        r.karts[0].x = 0;
        r.tick(&boost);
    }
    assert!(r.karts[0].v > V_MAX, "{}", r.karts[0].v);
    assert!(r.karts[0].v <= V_BOOST);
    assert_eq!(r.charge, CHARGE_MAX - 40 * BOOST_COST);
    assert!(r.boosting);
    // The brake stops the boost.
    r.tick(&Pad {
        brake: 256,
        ..boost
    });
    assert!(!r.boosting);
    // An empty battery gives no boost, and the kart falls back to top
    // speed (from the start straight, so no hill helps it).
    r.charge = 0;
    r.karts[0].prog = 0;
    for _ in 0..200 {
        r.karts[0].x = 0;
        r.tick(&boost);
    }
    assert!(!r.boosting);
    assert_eq!(r.charge, 0);
    assert!(r.karts[0].v <= V_MAX);
}

#[test]
fn the_grass_is_slow() {
    let mut r = going(0);
    place_at(&mut r, 0, 0, 1024);
    let x = OFF_ROAD + X_ONE / 8;
    let mut worst = 0;
    for t in 0..200 {
        r.karts[0].x = x;
        r.tick(&GAS);
        assert!(r.off_road);
        if t >= 100 {
            worst = worst.max(r.karts[0].v);
        }
    }
    // Under half of top speed (45 %, give or take a hill).
    assert!(worst < V_MAX / 2, "{worst}");
    // Back on the road: fast again.
    for _ in 0..200 {
        r.karts[0].x = 0;
        r.tick(&GAS);
    }
    assert!(!r.off_road);
    assert!(r.karts[0].v > V_MAX * 9 / 10);
}

#[test]
fn running_into_a_post_stops_the_kart_short() {
    let mut r = going(0);
    let c = &COURSES[0];
    // A post on the left of the road.
    let (seg, post) = (1..c.segs)
        .find_map(|i| {
            c.things(i)
                .into_iter()
                .flatten()
                .find(|t| t.kind == hairpin::track::Kind::Post && t.x < 0)
                .map(|t| (i, t))
        })
        .unwrap();
    let x = i32::from(post.x) * (X_ONE / 1024);
    // Just before it, at half speed, right on its line.
    r.karts[0].prog = seg as i32 * SEG * SUB - 100;
    r.karts[0].x = x;
    r.karts[0].v = V_MAX / 2;
    r.tick(&GAS);
    assert!(events(&mut r).contains(&RaceEvent::Crash));
    assert!(r.karts[0].v < V_MAX / 4);
    assert!(r.shake > 0);
    // On the road nothing is hit.
    let mut r = going(0);
    r.karts[0].prog = seg as i32 * SEG * SUB - 100;
    r.karts[0].v = V_MAX / 2;
    r.tick(&GAS);
    assert!(!events(&mut r).contains(&RaceEvent::Crash));
}

#[test]
fn the_lake_cannot_be_driven_on() {
    // Coming from far off the road in the woods before the lake, steering
    // left the whole time: the railing's posts are passed outside them,
    // so only the water itself can stop the kart.
    let c = &COURSES[0];
    let lake = (0..c.segs)
        .filter(|&i| c.shoulder(i) == hairpin::track::Shoulder::WaterLeft)
        .collect::<Vec<_>>();
    assert!(!lake.is_empty());
    for x in [-X_LIMIT, -X_ONE * 5 / 2, -X_ONE * 2] {
        let mut r = going(0);
        place_at(&mut r, lake[0] - 60, x, 512);
        let mut crashes = 0;
        for _ in 0..600 {
            r.tick(&Pad { steer: -256, ..GAS });
            crashes += events(&mut r)
                .iter()
                .filter(|e| **e == RaceEvent::Crash)
                .count();
            let seg = r.seg_of(0) % c.segs;
            assert!(
                !(lake.contains(&seg) && r.karts[0].x < -IN_WATER),
                "kart in the lake at segment {seg}, x {}",
                r.karts[0].x
            );
        }
        assert!(crashes > 0, "start x {x}: the water's edge was hit");
    }
}

#[test]
fn three_laps_make_a_race_and_each_lap_is_timed() {
    let mut r = going(0);
    let lap = r.lap_prog();
    for done in 1..=LAPS {
        r.tick = COUNTDOWN + 100 * done;
        r.karts[0].prog = done as i32 * lap - 1000;
        r.karts[0].x = 0;
        r.karts[0].v = V_MAX;
        r.tick(&GAS);
        let ev = events(&mut r);
        assert!(
            ev.iter()
                .any(|e| matches!(e, RaceEvent::Lap { done: d, .. } if u32::from(*d) == done)),
            "lap {done}: {ev:?}"
        );
        assert_eq!(r.finished, done == LAPS);
    }
    assert!(r.laps.iter().all(|&t| t > 0));
    assert_eq!(r.finish_place, 1, "the rivals were still on the grid");
    assert!(r.finish_ticks > 0);
    // After the line the player's kart drives itself.
    let p = r.karts[0].prog;
    r.tick(&Pad::default());
    assert!(r.karts[0].prog > p);
}

#[test]
fn the_place_follows_who_is_ahead() {
    let mut r = going(0);
    for (k, kart) in r.karts.iter_mut().enumerate() {
        kart.prog = 1_000_000 - k as i32 * 10_000;
    }
    assert_eq!(r.place_now(), 1);
    r.karts[3].prog = 2_000_000;
    assert_eq!(r.place_now(), 2);
    r.karts[0].prog = 3_000_000;
    assert_eq!(r.place_now(), 1);
    // A rival that finished first stays ahead whatever its count.
    r.karts[2].finish = 10;
    r.karts[2].prog = 0;
    assert_eq!(r.place_now(), 2);
}

#[test]
fn only_the_top_three_go_on_to_the_next_course() {
    for (place, next) in [(1u8, 1u8), (3, 1), (4, 0), (5, 0)] {
        let mut d = Driver::new();
        d.race_course(0);
        d.app.race.finish_place = place;
        d.app.race.finish_ticks = 3000;
        d.app.scene = Scene::Finish;
        d.app.scene_t = CARD_WAIT;
        d.tap(button::A);
        assert_eq!(d.app.scene, Scene::Race, "place {place}");
        assert_eq!(d.app.race.course, next, "place {place}");
    }
    // After the last course: the cup.
    let mut d = Driver::new();
    d.race_course(2);
    d.app.race.finish_place = 2;
    d.app.scene = Scene::Finish;
    d.app.scene_t = CARD_WAIT;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Cup);
}

/// The steepest climb or fall of a course, in world units per segment.
fn steepest(c: usize) -> i32 {
    let c = &COURSES[c];
    (0..c.segs).map(|i| c.slope(i).abs()).max().unwrap()
}

/// The sharpest bend of a course (curve16).
fn sharpest(c: usize) -> i32 {
    let c = &COURSES[c];
    (0..c.segs).map(|i| c.seg(i).curve16.abs()).max().unwrap()
}

#[test]
fn every_course_is_a_closed_loop_and_each_is_harder_than_the_last() {
    for (n, c) in COURSES.iter().enumerate() {
        // The road ends where it began, at the same height, and the line
        // is on a flat straight.
        assert_eq!(c.y_at(0), c.y_at(c.lap_z()), "{}", c.name);
        assert_eq!(c.seg(0).curve16, 0);
        assert_eq!(c.slope(0), 0);
        // Heights change smoothly: no step bigger than a steep climb.
        for i in 0..c.segs {
            assert!(c.slope(i).abs() < 120, "{} seg {i}", c.name);
        }
        let mean_skill: i32 = c.skills.iter().sum::<i32>() / 4;
        println!(
            "course {} {}: {} segments ({} world units), sharpest bend {} (top speed there {} of 1024), \
             steepest {} a segment, rivals {:?} (mean {})",
            n + 1,
            c.name,
            c.segs,
            c.lap_z(),
            sharpest(n),
            (GRIP / sharpest(n).max(1)).min(1024),
            steepest(n),
            c.skills,
            mean_skill
        );
    }
    for n in 1..COURSES.len() {
        assert!(sharpest(n) > sharpest(n - 1), "bends");
        assert!(steepest(n) > steepest(n - 1), "hills");
        let skill = |c: usize| COURSES[c].skills.iter().sum::<i32>();
        assert!(skill(n) > skill(n - 1), "rivals");
    }
    // Course 1 can be driven flat out; the last needs the brake.
    assert!(GRIP / sharpest(0) >= 1024);
    assert!(GRIP / sharpest(2) < 800);
}

#[test]
fn a_race_left_running_for_half_an_hour_never_overflows() {
    // The player never drives; the rivals lap on and on. (The tests run
    // with overflow checks, like the chip build.)
    for c in 0..COURSES.len() as u8 {
        let mut r = going(c);
        let end = LAPS as i32 * r.lap_prog();
        for _ in 0..60 * 60 * 30 {
            r.tick(&Pad::default());
            while r.take_event().is_some() {}
        }
        for k in &r.karts[1..] {
            assert!(k.finish > 0);
            assert!(k.prog < end + r.lap_prog(), "kept in range");
        }
        assert!(!r.finished);
        assert_eq!(r.place_now(), KARTS as u8);
    }
}
