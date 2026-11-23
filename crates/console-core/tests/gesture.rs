//! The touch recogniser on its own (`input::Gesture`): down, tap on
//! lift, drag, release with speed. Any page can use it; these tests say
//! what it promises. Picture pixels, times in microseconds.

use console_core::FRAME_US;
use console_core::input::{Drag, Gesture, Release, Touches};
use console_core::theme::{DRAG_START, TAP_MAX_US, TOUCH_GRACE, TOUCH_STILL_US};

/// Feeds one frame per entry (`None`: no finger), one frame apart.
fn run(g: &mut Gesture, frames: &[Option<(u16, u16)>]) -> Vec<Touches> {
    frames
        .iter()
        .enumerate()
        .map(|(i, &at)| g.update(i as u64 * FRAME_US, at, None))
        .collect()
}

/// A touch going on, and nothing else this frame.
const HELD: Touches = Touches {
    down: None,
    tap: None,
    drag: None,
    release: None,
    touching: true,
};

#[test]
fn down_then_lift_is_a_tap_one_grace_after_the_lift() {
    let mut g = Gesture::new();
    let mut frames = vec![Some((50, 40)), Some((51, 41))];
    frames.extend([None; TOUCH_GRACE as usize + 1]);
    let t = run(&mut g, &frames);
    assert_eq!(t[0].down, Some((50, 40)));
    assert_eq!(t[0].tap, None, "not yet");
    assert!(t[0].touching);
    assert_eq!(t[1], HELD);
    // The lift frame and the grace: nothing yet, the touch goes on.
    for (i, t) in t[2..2 + TOUCH_GRACE as usize].iter().enumerate() {
        assert_eq!(*t, HELD, "grace frame {i}");
    }
    // Exactly one tap, where it went down, one grace after the lift.
    let last = t.last().unwrap();
    assert_eq!(last.tap, Some((50, 40)), "where it went down");
    assert_eq!(last.release, None);
    assert!(!last.touching);
    assert_eq!(t.iter().filter(|t| t.tap.is_some()).count(), 1);
}

#[test]
fn one_missing_read_does_not_end_a_press() {
    let mut g = Gesture::new();
    // Down, missing for the grace, back, then a real lift.
    let mut frames = vec![Some((50, 40))];
    frames.extend([None; TOUCH_GRACE as usize]);
    frames.push(Some((50, 40)));
    frames.extend([None; TOUCH_GRACE as usize + 1]);
    let t = run(&mut g, &frames);
    assert_eq!(t.iter().filter(|t| t.down.is_some()).count(), 1, "{t:?}");
    let taps: Vec<_> = t.iter().map(|t| t.tap.is_some()).collect();
    assert_eq!(taps.iter().filter(|&&x| x).count(), 1, "{t:?}");
    assert!(*taps.last().unwrap(), "the one tap comes at the end");
}

#[test]
fn a_bounce_the_board_noted_in_the_grace_is_the_same_press() {
    let mut g = Gesture::new();
    g.update(0, Some((50, 40)), None);
    // Missing, but the board saw it touch again since the last frame.
    for i in 1..=4 {
        let t = g.update(i * FRAME_US, None, Some((50, 40)));
        assert_eq!(t, HELD, "frame {i}");
    }
    // A real lift: one tap, after the grace.
    let mut t = Touches::default();
    for i in 0..=u64::from(TOUCH_GRACE) {
        t = g.update((5 + i) * FRAME_US, None, None);
    }
    assert_eq!(t.tap, Some((50, 40)));
}

#[test]
fn a_press_held_too_long_is_no_tap() {
    // Down at 0, the first read with no finger at `up`.
    let hold = |up: u64| {
        let mut g = Gesture::new();
        g.update(0, Some((50, 40)), None);
        let mut taps = 0;
        for i in 0..=u64::from(TOUCH_GRACE) {
            let t = g.update(up + i * FRAME_US, None, None);
            taps += usize::from(t.tap.is_some());
            if i == u64::from(TOUCH_GRACE) {
                assert!(!t.touching && t.release.is_none() && t.drag.is_none());
            }
        }
        assert!(!g.touching());
        taps
    };
    assert_eq!(hold(TAP_MAX_US), 1, "held just the limit: a tap");
    assert_eq!(hold(TAP_MAX_US + 1), 0, "held longer: no tap");
    assert_eq!(hold(10 * TAP_MAX_US), 0);
    // The console slept under the resting finger: the last read that saw
    // it was early, but the lift came late. No tap.
    let mut g = Gesture::new();
    g.update(0, Some((50, 40)), None);
    g.update(TOUCH_STILL_US, Some((50, 40)), None);
    let mut t = g.update(TAP_MAX_US + FRAME_US, None, None);
    for i in 1..=u64::from(TOUCH_GRACE) {
        t = g.update(TAP_MAX_US + (1 + i) * FRAME_US, None, None);
    }
    assert_eq!(t.tap, None, "held past the limit while the console slept");
    // The late frame that believes the lift does not count.
    let mut g = Gesture::new();
    g.update(0, Some((50, 40)), None);
    g.update(TAP_MAX_US, None, None);
    let t = g.update(TAP_MAX_US + 9 * FRAME_US, None, None);
    if TOUCH_GRACE == 1 {
        assert_eq!(t.tap, Some((50, 40)));
    }
}

#[test]
fn a_drag_starts_at_drag_start_and_reports_every_move() {
    let mut g = Gesture::new();
    let d = DRAG_START;
    let t = run(
        &mut g,
        &[
            Some((100, 40)),
            Some((100 - (d - 1), 40)),
            Some((100 - d, 40)),
            Some((90, 41)),
        ],
    );
    assert_eq!(t[1].drag, None, "under the threshold");
    let drag = |dx: i16, dy: i16| {
        Some(Drag {
            from: (100, 40),
            dx,
            dy,
        })
    };
    // The first drag frame brings all the way since the finger went down.
    assert_eq!(t[2].drag, drag(-(d as i16), 0));
    assert_eq!(t[3].drag, drag(90 - (100 - d as i16), 1));
    // Up and down count too (a page that scrolls the other way).
    let mut g = Gesture::new();
    let t = run(&mut g, &[Some((10, 10)), Some((10, 10 + d))]);
    assert!(t[1].drag.is_some());
}

#[test]
fn a_drag_is_never_a_tap_again() {
    let mut g = Gesture::new();
    let t = run(
        &mut g,
        &[Some((50, 40)), Some((60, 40)), Some((50, 40)), None, None],
    );
    let last = TOUCH_GRACE as usize + 3;
    assert!(t.iter().all(|t| t.tap.is_none()));
    assert!(t[last].release.is_some());
}

#[test]
fn the_release_has_the_speed_unless_the_finger_rested() {
    // 6 pixels a frame to the right: 180 pixels a second.
    let frames: Vec<_> = (0..6).map(|i| Some((20 + 6 * i, 40))).collect();
    let mut g = Gesture::new();
    let mut all = frames.clone();
    all.extend([None; TOUCH_GRACE as usize + 1]);
    let t = run(&mut g, &all);
    let r = t.last().unwrap().release.expect("a release");
    assert!(r.vx > 120 && r.vx <= 180, "vx {}", r.vx);
    assert_eq!(r.vy, 0);
    // The same, then resting before the lift: no speed.
    let mut g = Gesture::new();
    let mut all = frames;
    let rest = TOUCH_STILL_US.div_ceil(FRAME_US) as usize + 1;
    all.extend(vec![Some((50, 40)); rest]);
    all.extend([None; TOUCH_GRACE as usize + 1]);
    let t = run(&mut g, &all);
    assert_eq!(
        t.last().unwrap().release,
        Some(Release {
            from: (20, 40),
            vx: 0,
            vy: 0
        })
    );
}

#[test]
fn live_while_moving_and_while_the_finger_is_missing() {
    let mut g = Gesture::new();
    assert!(!g.live(0));
    g.update(0, Some((10, 10)), None);
    assert!(g.live(0), "just went down");
    assert!(!g.live(TOUCH_STILL_US), "resting");
    g.update(FRAME_US, Some((30, 10)), None);
    assert!(g.dragging());
    let t = g.update(2 * FRAME_US, None, None);
    if TOUCH_GRACE > 0 {
        assert!(t.drag.is_some() && t.release.is_none());
        assert!(g.live(10 * TOUCH_STILL_US), "waiting to see it back");
    }
}

#[test]
fn cancel_forgets_the_touch_until_lifted() {
    let mut g = Gesture::new();
    g.update(0, Some((10, 10)), None);
    g.cancel();
    assert!(!g.touching());
    let mut f = 1;
    assert_eq!(
        g.update(f * FRAME_US, Some((40, 10)), None),
        Touches::default()
    );
    for _ in 0..=TOUCH_GRACE {
        f += 1;
        assert_eq!(g.update(f * FRAME_US, None, None), Touches::default());
    }
    // The next touch counts.
    f += 1;
    assert_eq!(
        g.update(f * FRAME_US, Some((5, 5)), None).down,
        Some((5, 5))
    );
}

#[test]
fn a_taken_touch_stays_taken_through_a_missing_read() {
    let mut g = Gesture::new();
    g.update(0, Some((10, 10)), None);
    g.cancel();
    let mut f = 1;
    for _ in 0..TOUCH_GRACE {
        assert_eq!(g.update(f * FRAME_US, None, None), Touches::default());
        // The console comes back next frame to see whether it lifted.
        assert!(g.live(f * FRAME_US + 10 * TOUCH_STILL_US));
        f += 1;
    }
    // Back: still the same, taken touch. No down, no tap, ever.
    let mut all = vec![g.update(f * FRAME_US, Some((10, 10)), None)];
    for _ in 0..=TOUCH_GRACE {
        f += 1;
        all.push(g.update(f * FRAME_US, None, None));
    }
    assert!(all.iter().all(|t| *t == Touches::default()), "{all:?}");
    assert!(!g.live(f * FRAME_US), "lifted: the console may sleep");
}

#[test]
fn live_through_the_grace_of_a_resting_press() {
    let mut g = Gesture::new();
    g.update(0, Some((10, 10)), None);
    let later = 10 * TOUCH_STILL_US;
    g.update(later, Some((10, 10)), None);
    assert!(
        !g.live(later + 1),
        "a resting finger lets the console sleep"
    );
    g.update(later + FRAME_US, None, None);
    if TOUCH_GRACE > 0 {
        assert!(g.live(later + FRAME_US), "back next frame for the tap");
    }
}
