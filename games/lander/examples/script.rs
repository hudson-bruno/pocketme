//! Writes a simulator script that flies level 1 with the test autopilot,
//! so the same landing can be watched (and checked) in the simulator.
//!
//! `cargo run --offline --example script -- START_FRAME > lander.txt`
//!
//! START_FRAME is the simulator frame of the game's first call (the
//! frame that starts LANDER from the home screen). The script first
//! steps right five times on the home screen, from BENCH to LANDER, in
//! the 30 frames before START_FRAME. At `--fps 60` the boot screen ends
//! by itself at frame 90 (a press before that only ends it early), so
//! START_FRAME must be 125 or more. Call `k` of the game
//! is then frame START_FRAME + k: the game starts at 60 pictures a
//! second, and the simulator run with `--fps 60` calls it once a frame
//! and adds 1/60 s each time, as the tests' loop does at RATE 60. The
//! script ends with a comment saying on which frame the craft lands and
//! with how many points; the simulator's log line "lander: landed on
//! level 1, N points" must say the same.

#[path = "../tests/common/mod.rs"]
mod common;

use common::{BOARD_TRIGGER, Pilot, Run, pilot};
use game_kit::button;
use lander::app::{Input, Scene};

/// Title calls before PLAY, and calls on the "LEVEL 1" card.
const ON_TITLE: u64 = 20;
const ON_CARD: u64 = 15;

fn main() {
    let start: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(130);
    assert!(start >= 125, "START_FRAME must be 125 or more");
    let mut r = Run::new();
    // The game's own start: RATE 60 (`Run::new` sets 30 for the tests).
    r.app.opts.fast = true;
    let mut out = Vec::new();
    let f = |k: u64| start + k;

    out.push(String::from(
        "# LANDER, level 1, flown by the autopilot of games/lander/tests/common/mod.rs",
    ));
    out.push(format!(
        "# (made by: cargo run --example script -- {start}); the game starts at frame {start}"
    ));
    // LANDER is the sixth tile: five steps right from BENCH, then one
    // press of south on the home screen.
    for k in 0..5u64 {
        out.push(format!("{} press right", start - 30 + 6 * k));
        out.push(format!("{} release right", start - 28 + 6 * k));
    }
    out.push(format!("{start} press south"));
    out.push(format!("{} release south", start + 1));
    r.idle(ON_TITLE as u32);
    out.push(format!("{} shot   # title", f(r.calls) - 1));
    // PLAY: A for one call.
    out.push(format!("{} press south", f(r.calls)));
    out.push(format!("{} release south", f(r.calls) + 1));
    r.tap(button::A);
    assert_eq!(r.app.scene, Scene::Ready);
    r.idle(ON_CARD as u32 - 2);
    out.push(format!("{} shot   # the LEVEL 1 card", f(r.calls) - 1));
    // Skip the rest of the card with A: the flight starts on this call.
    out.push(format!("{} press south", f(r.calls)));
    out.push(format!("{} release south", f(r.calls) + 1));
    r.call(&Input {
        held: button::A,
        pressed: button::A,
        ..Input::default()
    });
    assert_eq!(r.app.scene, Scene::Fly);
    // The release call: the pilot's first input, without A.
    let mut last = Pilot::default();
    let mut first = true;
    let mut shots = 0;
    while r.app.scene == Scene::Fly && r.calls < 5000 {
        let p = pilot(&r.app);
        let k = r.calls;
        if first || p.stick_x != last.stick_x {
            out.push(format!("{} stick left {} 0", f(k), p.stick_x));
        }
        if first || p.trigger != last.trigger {
            out.push(format!(
                "{} trigger right {}",
                f(k),
                p.trigger.min(BOARD_TRIGGER as u16)
            ));
        }
        if !first && p.level != last.level {
            out.push(format!(
                "{} {} east",
                f(k),
                if p.level { "press" } else { "release" }
            ));
        }
        if k.is_multiple_of(150) && shots < 4 {
            out.push(format!("{} shot   # in flight", f(k)));
            shots += 1;
        }
        first = false;
        last = p;
        r.call(&p.input());
    }
    assert_eq!(r.app.scene, Scene::Landed, "{:?}", r.app.ending);
    let landed = f(r.calls) - 1;
    out.push(format!("{} stick left 0 0", f(r.calls)));
    out.push(format!("{} trigger right 0", f(r.calls)));
    if last.level {
        out.push(format!("{} release east", f(r.calls)));
    }
    out.push(format!(
        "# landed on frame {landed}: {} points, score {}",
        r.app.tally.points, r.app.score
    ));
    out.push(format!("{} shot   # the LANDED card", landed + 20));
    for line in out {
        println!("{line}");
    }
}
