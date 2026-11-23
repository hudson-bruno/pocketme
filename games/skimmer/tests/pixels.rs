//! What the game costs the screen: changed pixels per frame over a
//! minute of play, measured with the console's own rule
//! (`console_video::diff`, at most 16 areas). Writes a CSV, a
//! send-plan trace (panel pixels) and a summary to
//! `.local/airhockey/pixels/`.

mod common;

use std::fmt::Write as _;

use common::{Driver, evidence};
use console_video::MAX_AREAS;
use game_kit::{H, W};
use skimmer::app::{Scene, Who};
use skimmer::table::Length;

/// A case: its name, the pads, the settings, the frame rate.
type Case<'a> = (&'a str, [bool; 4], &'a dyn Fn(&mut Driver), u32);

struct Cost {
    frames: usize,
    avg: u32,
    p99: u32,
    worst: u32,
    most_areas: usize,
}

/// One minute of a timed match from the first serve, at `fps` calls a
/// second, with the test players on every pad and touch mallet.
fn minute(name: &str, pads: [bool; 4], set: impl Fn(&mut Driver), fps: u32) -> Cost {
    let mut d = Driver::new(pads);
    d.app.opts.length = Length::Timed;
    d.app.opts.fast = fps == 60;
    set(&mut d);
    d.delta_us = 1_000_000 / fps + u32::from(fps == 30);
    d.start();
    let from = d.sent.len();
    for _ in 0..fps * 60 {
        assert!(d.play_call());
        assert_eq!(d.app.scene, Scene::Play);
    }
    // The first frame after the title repaints everything: left out.
    let sent = &d.sent[from + 1..];
    let mut px: Vec<u32> = sent.iter().map(|(p, _)| *p).collect();
    let total: u64 = px.iter().map(|&p| u64::from(p)).sum();
    let most_areas = sent.iter().map(|(_, a)| a.len()).max().unwrap_or(0);
    assert!(most_areas <= MAX_AREAS);

    let dir = evidence("pixels");
    let mut csv = String::from("frame,pixels,areas\n");
    let mut trace = format!("# send-plan trace 1\n# game SKIMMER\n# case {name}\n# rate {fps}\n");
    for (k, (p, areas)) in sent.iter().enumerate() {
        let _ = writeln!(csv, "{k},{p},{}", areas.len());
        let _ = write!(
            trace,
            "S {} {}",
            k as u64 * u64::from(d.delta_us),
            areas.len()
        );
        for a in areas.as_slice() {
            let _ = write!(trace, " {},{},{},{}", a.x * 2, a.y * 2, a.w * 2, a.h * 2);
        }
        trace.push('\n');
    }
    std::fs::write(dir.join(format!("{name}.csv")), csv).unwrap();
    std::fs::write(dir.join(format!("{name}.trace")), trace).unwrap();

    px.sort_unstable();
    Cost {
        frames: px.len(),
        avg: (total / px.len() as u64) as u32,
        p99: px[px.len() * 99 / 100],
        worst: *px.last().unwrap(),
        most_areas,
    }
}

#[test]
fn a_minute_of_play_changes_a_small_part_of_the_picture() {
    let full = (W * H) as u32;
    let one = [true, false, false, false];
    let two = [true, true, false, false];
    let four = [true; 4];
    let plain = |_: &mut Driver| {};
    let touch = |d: &mut Driver| d.app.opts.side = [Who::Pad1, Who::Touch];
    let both = |d: &mut Driver| d.app.opts.four = true;
    let wild = |d: &mut Driver| {
        d.app.opts.four = true;
        d.app.opts.wild = true;
    };
    let cases: [Case; 8] = [
        ("one-player-60", one, &plain, 60),
        ("one-player-30", one, &plain, 30),
        ("pad-against-pad-60", two, &plain, 60),
        ("pad-against-touch-60", one, &touch, 60),
        ("two-against-two-60", four, &both, 60),
        ("two-against-two-30", four, &both, 30),
        ("wild-two-against-two-60", four, &wild, 60),
        ("wild-two-against-two-30", four, &wild, 30),
    ];
    let mut report = String::new();
    for (name, pads, set, fps) in cases {
        let c = minute(name, pads, set, fps);
        let line = format!(
            "{name}: {} frames, changed pixels a frame: average {} ({:.1}%), 99th percentile {} ({:.1}%), worst {} ({:.1}%), most areas {}",
            c.frames,
            c.avg,
            f64::from(c.avg) * 100.0 / f64::from(full),
            c.p99,
            f64::from(c.p99) * 100.0 / f64::from(full),
            c.worst,
            f64::from(c.worst) * 100.0 / f64::from(full),
            c.most_areas
        );
        println!("{line}");
        report.push_str(&line);
        report.push('\n');
        // Small changes: the average under a tenth of the picture.
        assert!(c.avg < full / 10, "{line}");
    }
    std::fs::write(evidence("pixels").join("summary.txt"), report).unwrap();
}
