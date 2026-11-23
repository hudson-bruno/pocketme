//! The touch filter, fed with scripted readings (the shapes seen in the
//! owner's finger test, round 4: README.md, "Tuning").

use board_api::TouchPoint;
use touch_filter::{
    CONFIRM_READS, DEAD_PX, Fed, HOLD_READS, Kind, Reading, TouchCounters, TouchFilter, TouchLife,
    Z_OFF, Z_ON,
};

/// About the CYD's reading rate while a finger is down (16 to 24 ms).
const READ_US: u64 = 20_000;

fn pt(x: u16, y: u16) -> TouchPoint {
    TouchPoint { x, y }
}
fn firm(x: u16, y: u16) -> Reading {
    Reading::Firm(pt(x, y), 1000)
}

/// A filter, its counters and a clock.
struct Rig {
    f: TouchFilter,
    c: TouchCounters,
    now: u64,
    /// The point after every reading (what the console would see).
    seen: Vec<Option<TouchPoint>>,
    ended: Option<(TouchLife, Kind)>,
}

impl Rig {
    fn new() -> Self {
        Rig {
            f: TouchFilter::default(),
            c: TouchCounters::default(),
            now: 1_000_000,
            seen: vec![],
            ended: None,
        }
    }

    fn feed(&mut self, r: Reading) -> Fed {
        self.now += READ_US;
        let fed = self.f.feed(self.now, r, false, &mut self.c);
        self.seen.push(self.f.point());
        if fed.ended.is_some() {
            self.ended = fed.ended;
        }
        fed
    }

    fn feed_all(&mut self, rs: &[Reading]) {
        for &r in rs {
            self.feed(r);
        }
    }

    fn kind(&self) -> Kind {
        self.ended.expect("the touch ended").1
    }
}

// ---- the lone wild reading (CONFIRM_READS) ---------------------------------

#[test]
fn one_wild_reading_at_rest_is_left_out_and_the_tap_stays_a_tap() {
    let mut r = Rig::new();
    // As in the owner's log: a 3- or 4-reading tap whose one jump of
    // 7 to 15 px made a drag of it.
    r.feed_all(&[
        firm(100, 100),
        firm(101, 100),
        firm(112, 100),
        firm(100, 101),
    ]);
    assert!(
        r.seen.iter().all(|&p| p == Some(pt(100, 100))),
        "{:?}",
        r.seen
    );
    r.feed(Reading::None);
    assert_eq!(r.kind(), Kind::Tap);
    assert_eq!(r.c.spikes, 1);
    assert_eq!(r.c.breakouts, 0);
    assert_eq!(r.c.taps, 1);
    assert_eq!(r.c.drags, 0);
}

#[test]
fn a_wild_reading_just_before_the_lift_is_left_out() {
    let mut r = Rig::new();
    r.feed_all(&[firm(100, 100), firm(100, 100), firm(114, 96)]);
    assert_eq!(r.f.point(), Some(pt(100, 100)));
    r.feed(Reading::None);
    let (life, kind) = r.ended.unwrap();
    assert_eq!(kind, Kind::Tap);
    assert_eq!(life.spikes, 1);
    assert_eq!(life.travel, 0);
    // The rebound check uses where the console last saw the finger.
    let fed = r.feed(firm(102, 100));
    assert!(fed.started);
    assert_eq!(r.c.rebounds, 1);
}

#[test]
fn a_drag_from_rest_starts_one_reading_later_then_follows_every_reading() {
    let mut r = Rig::new();
    r.feed_all(&[firm(100, 100), firm(100, 100)]);
    // The first reading outside the band: not shown yet.
    r.feed(firm(106, 100));
    assert_eq!(r.f.point(), Some(pt(100, 100)));
    // The second: shown at once, where the finger is now (no distance
    // lost), and from then on every reading.
    for x in [112, 118, 124, 130, 136] {
        r.feed(firm(x, 100));
        assert_eq!(r.f.point(), Some(pt(x, 100)));
    }
    assert_eq!(r.c.breakouts, 1);
    assert_eq!(r.c.spikes, 0);
    r.feed(Reading::None);
    assert!(matches!(r.kind(), Kind::Drag | Kind::Flick));
}

#[test]
fn a_weak_reading_between_two_outside_does_not_undo_the_pair() {
    let mut r = Rig::new();
    r.feed_all(&[
        firm(100, 100),
        firm(110, 100),
        Reading::Weak,
        firm(111, 100),
    ]);
    assert_eq!(r.f.point(), Some(pt(111, 100)));
    assert_eq!(r.c.breakouts, 1);
    assert_eq!(r.c.spikes, 0);
}

#[test]
fn a_fast_flick_is_still_a_flick() {
    let mut r = Rig::new();
    r.feed(firm(60, 120));
    for i in 1..=6u16 {
        r.feed(firm(60 + 20 * i, 120));
    }
    r.feed(Reading::None);
    assert_eq!(r.kind(), Kind::Flick);
}

/// The price, written down: a swipe so short that the panel was read
/// only twice (start, one point away, lift) is a tap now. The owner's
/// log had 6 such touches in 278; 5 of them moved 7 to 14 px (a tap
/// with one bad reading) and 1 moved 47 px.
#[test]
fn the_price_a_two_reading_swipe_is_a_tap() {
    let mut r = Rig::new();
    r.feed_all(&[firm(100, 100), firm(147, 100), Reading::None]);
    assert_eq!(r.kind(), Kind::Tap);
    assert_eq!(CONFIRM_READS, 2);
}

#[test]
fn wild_readings_in_a_long_rest_never_move_the_point() {
    let mut r = Rig::new();
    r.feed(firm(200, 50));
    for i in 0..200u16 {
        // Every 7th reading wild, by up to 20 px, in turn on each side.
        let p = if i % 7 == 3 {
            let d = 8 + i % 13;
            if i % 2 == 0 {
                firm(200 + d, 50)
            } else {
                firm(200 - d, 50 + d)
            }
        } else {
            firm(200 + i % 2, 50)
        };
        r.feed(p);
    }
    assert!(r.seen.iter().all(|&p| p == Some(pt(200, 50))));
    assert_eq!(r.c.breakouts, 0);
    assert!(r.c.spikes >= 28);
}

// ---- what the log line tells (for the next finger test) -------------------

#[test]
fn the_largest_jump_says_which_reading_made_it() {
    let mut r = Rig::new();
    // Reading 4 jumps 9 px (at the lift), reading 2 only 2 px.
    r.feed_all(&[
        firm(50, 50),
        firm(52, 50),
        firm(52, 51),
        firm(61, 51),
        Reading::None,
    ]);
    let (life, _) = r.ended.unwrap();
    assert_eq!(life.max_jump, 9);
    assert_eq!(life.max_jump_at, 4);
    assert_eq!(life.reads, 4);
}

#[test]
fn a_start_says_how_long_after_the_last_lift() {
    let mut r = Rig::new();
    let fed = r.feed(firm(10, 10));
    assert!(fed.started);
    assert_eq!(fed.after_lift_us, None);
    r.feed(Reading::None);
    r.now += 15_000;
    let fed = r.feed(firm(12, 10));
    assert!(fed.started);
    assert_eq!(fed.after_lift_us, Some(15_000 + READ_US));
}

// ---- the rules kept from round 4 ------------------------------------------

#[test]
fn pressure_limits_have_hysteresis() {
    let mut r = Rig::new();
    assert_eq!(r.f.z_limit(), Z_ON);
    r.feed(firm(10, 10));
    assert_eq!(r.f.z_limit(), Z_OFF);
    r.feed(Reading::None);
    assert_eq!(r.f.z_limit(), Z_ON);
}

#[test]
fn weak_readings_are_held_then_end_the_touch() {
    let mut r = Rig::new();
    r.feed(firm(30, 30));
    for _ in 0..HOLD_READS {
        r.feed(Reading::Weak);
        assert_eq!(r.f.point(), Some(pt(30, 30)));
    }
    r.feed(firm(30, 31));
    for _ in 0..HOLD_READS {
        r.feed(Reading::Weak);
    }
    assert!(r.f.down());
    r.feed(Reading::Weak);
    assert!(!r.f.down());
    assert_eq!(r.c.dropout_ends, 1);
    assert_eq!(r.c.dropouts, 2 * u32::from(HOLD_READS));
}

#[test]
fn the_line_going_high_ends_the_touch_at_once() {
    let mut r = Rig::new();
    r.feed_all(&[firm(30, 30), firm(30, 30)]);
    r.feed(Reading::None);
    assert!(!r.f.down());
    assert_eq!(r.f.point(), None);
}

/// The owner's resting finger: about 1 px from its average, at most 3
/// (DEAD_PX), and about 2 % weak readings, never more than 2 in a row.
#[test]
fn the_measured_rest_noise_shows_no_movement_and_never_ends_the_touch() {
    let mut r = Rig::new();
    let mut rng: u32 = 0x2545_F491;
    let mut next = || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        rng
    };
    r.feed(firm(160, 120));
    for _ in 0..3000 {
        let v = next();
        if v % 50 == 0 {
            r.feed(Reading::Weak);
            continue;
        }
        let span = u32::from(DEAD_PX);
        let dx = (v >> 8) % (2 * span + 1);
        let dy = (v >> 16) % (2 * span + 1);
        // Most readings within 1 px, now and then up to DEAD_PX.
        let (dx, dy) = if v % 5 == 0 {
            (dx, dy)
        } else {
            (dx % 3 + 2, dy % 3 + 2)
        };
        r.feed(firm(157 + dx as u16, 117 + dy as u16));
    }
    assert!(r.f.down());
    assert!(r.seen.iter().all(|&p| p == Some(pt(160, 120))));
    assert_eq!(r.c.breakouts, 0);
    assert_eq!(r.c.dropout_ends, 0);
}
