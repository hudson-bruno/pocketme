//! The handling, measured on a flat test floor: jump heights for each
//! hold, the running jump, the dash, coyote time and the jump buffer.
//! The numbers are the README's table; a change to a constant in
//! src/sim.rs shows up here first.

mod common;

use common::*;
use rooftop::sim::*;

fn world() -> World {
    World::new(&FLAT, Follow::Smooth)
}

fn pad(run: i32, jump_held: bool, jump: bool) -> Pad {
    Pad {
        run,
        dx: run.signum() as i8,
        dy: 0,
        jump_held,
        jump,
        dash: false,
    }
}

/// Holds jump for `hold` ticks from standing; returns (height, air
/// ticks).
fn standing_jump(hold: u32) -> (i32, u32) {
    let mut w = world();
    let y0 = w.runner.y;
    let mut top = y0;
    let mut t = 0;
    loop {
        t += 1;
        w.tick(&pad(0, t <= hold, t == 1));
        top = top.min(w.runner.y);
        if t > 2 && w.runner.on_ground {
            return (y0 - top, t);
        }
        assert!(t < 200);
    }
}

#[test]
fn a_longer_press_jumps_higher() {
    let got: Vec<(u32, (i32, u32))> = [1, 4, 8, 12, 100]
        .into_iter()
        .map(|h| (h, standing_jump(h)))
        .collect();
    println!("{got:?}");
    assert_eq!(
        got,
        vec![
            (1, (7, 18)),
            (4, (11, 23)),
            (8, (16, 27)),
            (12, (19, 30)),
            (100, (20, 36)),
        ]
    );
}

#[test]
fn a_running_jump_crosses_four_tiles() {
    let mut w = world();
    for _ in 0..30 {
        w.tick(&pad(RUN_MAX, false, false));
    }
    let x0 = w.runner.x;
    let mut t = 0;
    loop {
        t += 1;
        w.tick(&pad(RUN_MAX, true, t == 1));
        if t > 2 && w.runner.on_ground {
            break;
        }
    }
    assert_eq!((w.runner.x - x0, t), (36, 36));
}

fn dash_pad(dx: i8, dy: i8, dash: bool) -> Pad {
    Pad {
        run: 0,
        dx,
        dy,
        jump_held: false,
        jump: false,
        dash,
    }
}

#[test]
fn the_dash_freezes_then_goes_straight_and_is_used_up() {
    let mut w = world();
    let (x0, y0) = (w.runner.x, w.runner.y);
    w.tick(&dash_pad(1, 0, true));
    // The short freeze: the runner stands still, white, for a moment.
    assert_eq!(w.runner.dashes, 0, "the dash is used at once");
    assert!(w.runner.freeze > 0);
    assert_eq!(w.runner.pose(), Pose::Dash);
    for _ in 0..DASH_FREEZE {
        assert_eq!((w.runner.x, w.runner.y), (x0, y0));
        w.tick(&dash_pad(1, 0, false));
    }
    for _ in 0..40 {
        w.tick(&dash_pad(0, 0, false));
    }
    assert_eq!(w.runner.x - x0, 29, "ground dash length");
    assert_eq!(w.runner.y, y0);
    // On the ground again, so it is back (with the flash).
    assert_eq!(w.runner.dashes, 1);
}

#[test]
fn an_up_dash_and_a_jump_and_up_dash() {
    let mut w = world();
    let y0 = w.runner.y;
    let mut top = y0;
    for t in 0..40 {
        w.tick(&dash_pad(0, -1, t == 0));
        top = top.min(w.runner.y);
    }
    assert_eq!(y0 - top, 28);

    let mut w = world();
    let mut top = y0;
    for t in 0..60 {
        w.tick(&Pad {
            run: 0,
            dx: 0,
            dy: if t >= 18 { -1 } else { 0 },
            jump_held: true,
            jump: t == 0,
            dash: t == 18,
        });
        top = top.min(w.runner.y);
    }
    assert_eq!(y0 - top, 48);
}

#[test]
fn the_dash_is_not_given_back_in_the_air() {
    let mut w = world();
    w.tick(&pad(0, true, true));
    for t in 0..12 {
        w.tick(&dash_pad(0, -1, t == 0));
        assert_eq!(w.runner.dashes, 0);
    }
    // A second press in the air does nothing.
    let before = w.runner;
    w.tick(&dash_pad(1, 0, true));
    assert_eq!(w.runner.freeze, 0);
    assert_eq!(w.runner.dashes, 0);
    assert_ne!(w.runner.y, before.y, "still moving, no freeze");
}

#[test]
fn coyote_time_allows_a_late_jump_off_a_ledge() {
    // Walk off the end of a ledge: the first tick in the air with the
    // floor gone, then press jump a few ticks late.
    let mut jumped = Vec::new();
    for late in 0..10u32 {
        let mut w = World::new(&LEDGE, Follow::Smooth);
        // Run right until the floor is gone.
        let mut t = 0;
        while w.runner.on_ground || t == 0 {
            w.tick(&pad(RUN_MAX, false, false));
            t += 1;
            assert!(t < 200);
        }
        for _ in 0..late {
            w.tick(&pad(RUN_MAX, false, false));
        }
        w.tick(&pad(RUN_MAX, true, true));
        jumped.push(w.runner.vy < 0);
    }
    println!("late jumps: {jumped:?}");
    // The first tick in the air and `ok` more: COYOTE - 1 ticks of grace
    // after the last tick on the floor.
    let ok = jumped.iter().take_while(|&&j| j).count();
    assert_eq!(ok as u8 + 1, COYOTE - 1);
    assert!(jumped[ok..].iter().all(|&j| !j));
}

#[test]
fn a_jump_pressed_just_before_landing_still_happens() {
    let mut got = Vec::new();
    for early in 1..10u32 {
        let mut w = world();
        // A full jump, then press again `early` ticks before landing.
        let (_, air) = standing_jump(100);
        w.tick(&pad(0, true, true));
        let press_at = air - early;
        let mut t = 1;
        while t < air {
            w.tick(&pad(0, true, t == press_at));
            t += 1;
        }
        // A few ticks on: did a second jump start?
        let mut rose = false;
        for _ in 0..4 {
            w.tick(&pad(0, true, false));
            rose |= w.runner.vy < 0;
        }
        got.push(rose);
    }
    println!("buffered jumps: {got:?}");
    let ok = got.iter().take_while(|&&j| j).count();
    assert!(ok as u8 >= JUMP_BUFFER - 1, "{got:?}");
    assert!(!got[got.len() - 1], "a press far too early is not kept");
}
