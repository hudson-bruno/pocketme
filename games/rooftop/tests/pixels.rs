//! What each frame costs the screen: every room played by its script
//! through the whole game, each picture compared with the one before by
//! the console's own rule (`console_video::diff`, at most 16 areas).
//! The tables go to .local/games/platformer/pixels/.
//!
//! The heavy case is on purpose and under the player's control: the
//! wide room with SCROLL SMOOTH moves the whole view every frame the
//! runner moves; the default, SCROLL STEP, moves it half a screen at a
//! time. RATE 60 doubles the frames.

mod common;

use std::fmt::Write as _;

use common::*;
use console_video::{Areas, MAX_AREAS, diff};
use game_kit::{Frame, H, W, button};
use rooftop::sim::Follow;
use rooftop::{Keys, ROOM_COUNT, Scene};

/// Per call: (area pixels sent, areas, pixels that really changed).
type Costs = Vec<(u32, usize, u32)>;

/// Plays room `room`'s script from the title (practice from that room),
/// and returns the cost of every call from the first call in the room
/// to the one that reaches the door. `per_call` is 1 at 30 calls a
/// second, 2 at 60.
fn costs(room: usize, follow: Follow, per_call: u32) -> Costs {
    let mut d = Driver::new();
    d.game.from = room;
    d.game.follow = follow;
    d.delta_us = CALL_US / per_call;
    d.call(Keys {
        held: button::A,
        pressed: button::A,
        delta_us: d.delta_us,
        ..Keys::default()
    });
    assert_eq!(d.game.scene, Scene::Play);
    let script: Vec<Hold> = solution(room)
        .iter()
        .map(|h| Hold {
            calls: h.calls * per_call,
            held: h.held,
        })
        .collect();
    let mut shown: Box<Frame> = d.frame.clone();
    let mut out = Vec::new();
    let mut areas = Areas::new();
    for (held, pressed) in calls(&script) {
        d.call(keys_at(held, pressed, d.delta_us));
        diff(&shown, &d.frame, &mut areas);
        assert!(areas.len() <= MAX_AREAS);
        let changed = shown
            .0
            .iter()
            .zip(d.frame.0.iter())
            .filter(|(a, b)| a != b)
            .count() as u32;
        out.push((areas.pixels(), areas.len(), changed));
        shown.0.copy_from_slice(&d.frame.0);
        if d.game.run.room != room || d.game.scene != Scene::Play {
            break;
        }
    }
    // The last call shows the next room (or the end screen): a whole
    // new picture, counted apart.
    out
}

fn keys_at(held: u32, pressed: u32, delta_us: u32) -> Keys {
    Keys {
        held,
        pressed,
        delta_us,
        ..Keys::default()
    }
}

struct Stats {
    frames: usize,
    mean: u32,
    median: u32,
    p95: u32,
    max: u32,
    max_areas: usize,
    /// Frames sending more than half the picture.
    big: usize,
    /// Pixels sent per second of play.
    per_s: u32,
}

fn stats(c: &[(u32, usize, u32)], calls_per_s: u32) -> Stats {
    let mut px: Vec<u32> = c.iter().map(|x| x.0).collect();
    px.sort_unstable();
    let n = px.len().max(1);
    let sum: u64 = px.iter().map(|&p| u64::from(p)).sum();
    Stats {
        frames: px.len(),
        mean: (sum / n as u64) as u32,
        median: px[px.len() / 2],
        p95: px[(px.len() * 95 / 100).min(px.len() - 1)],
        max: *px.last().unwrap(),
        max_areas: c.iter().map(|x| x.1).max().unwrap(),
        big: px.iter().filter(|&&p| p > (W * H / 2) as u32).count(),
        per_s: (sum * u64::from(calls_per_s) / n as u64) as u32,
    }
}

#[test]
fn pixels_sent_per_frame() {
    let dir = evidence("pixels");
    let mut table = String::new();
    writeln!(
        table,
        "room, mode, frames, mean px, median px, p95 px, max px, max areas, frames > half, px per s"
    )
    .unwrap();
    let mut one_screen_p95 = 0;
    let mut wide = Vec::new();
    for room in 0..ROOM_COUNT {
        let wide_room = rooftop::rooms::ROOMS[room].scrolls();
        let modes: &[(&str, Follow, u32)] = if wide_room {
            &[
                ("step 30", Follow::Step, 1),
                ("smooth 30", Follow::Smooth, 1),
                ("step 60", Follow::Step, 2),
                ("smooth 60", Follow::Smooth, 2),
            ]
        } else {
            &[("30", Follow::Step, 1), ("60", Follow::Step, 2)]
        };
        for &(name, follow, per_call) in modes {
            let mut c = costs(room, follow, per_call);
            // Drop the last call (the next room's first picture).
            c.pop();
            let s = stats(&c, 30 * per_call);
            let mut csv = String::from("call, area px, areas, changed px\n");
            for (i, x) in c.iter().enumerate() {
                writeln!(csv, "{i}, {}, {}, {}", x.0, x.1, x.2).unwrap();
            }
            std::fs::write(
                dir.join(format!("room{}-{}.csv", room + 1, name.replace(' ', "-"))),
                csv,
            )
            .unwrap();
            writeln!(
                table,
                "{}, {name}, {}, {}, {}, {}, {}, {}, {}, {}",
                room + 1,
                s.frames,
                s.mean,
                s.median,
                s.p95,
                s.max,
                s.max_areas,
                s.big,
                s.per_s
            )
            .unwrap();
            if wide_room {
                wide.push((name, s));
            } else {
                one_screen_p95 = one_screen_p95.max(s.p95);
                // A one-screen room never sends the whole picture.
                assert_eq!(s.big, 0, "room {} {name}", room + 1);
            }
        }
    }
    print!("{table}");
    std::fs::write(dir.join("summary.csv"), &table).unwrap();

    // Typical play on one screen: a few hundred pixels a frame.
    assert!(one_screen_p95 < 1_500, "one-screen p95 {one_screen_p95}");
    let get = |n: &str| &wide.iter().find(|w| w.0 == n).unwrap().1;
    // The smooth camera is the heavy case: most frames resend a large
    // part of the view (the sky's rows and the repeating bricks do not
    // change, so not all of it).
    let smooth = get("smooth 30");
    assert!(smooth.median > 4_000, "smooth median {}", smooth.median);
    // The stepped camera sends the view only when it steps.
    let step = get("step 30");
    assert!(step.median < 500, "step median {}", step.median);
    assert!(step.big <= 8, "{} big frames", step.big);
    assert!(step.per_s * 5 < smooth.per_s);
}

#[test]
fn the_title_and_end_screens_stand_still() {
    let mut d = Driver::new();
    let first: Box<Frame> = d.frame.clone();
    d.idle(30);
    assert_eq!(first.0, d.frame.0, "the title does not move by itself");
    // The cursor moving changes two lines only.
    let before: Box<Frame> = d.frame.clone();
    d.tap(button::DOWN);
    let mut a = Areas::new();
    diff(&before, &d.frame, &mut a);
    println!("menu step: {} px in {} areas", a.pixels(), a.len());
    assert!(a.pixels() < 2_000);
}
