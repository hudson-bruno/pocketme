//! What the game costs the screen: changed pixels per frame over a
//! scripted minute, measured with the console's own rule
//! (`console_video::diff`, at most 16 areas). Writes a CSV and a
//! send-plan trace (panel pixels, 30 calls a second) to
//! `.local/four-players/hockey/pixels/`.

mod common;

use std::fmt::Write as _;

use common::{CALL_US, Driver, evidence};
use console_video::MAX_AREAS;
use game_kit::{H, W, button};
use tundra::app::Scene;

struct Cost {
    avg: u32,
    worst: u32,
    worst_areas: usize,
    frames: usize,
}

/// One minute of a match from the drop of the puck, `players` pads, the
/// test's player 1 and fixed patterns for the others.
fn minute(name: &str, players: usize, marks: bool) -> Cost {
    let mut pads = [false; 4];
    for p in pads.iter_mut().take(players) {
        *p = true;
    }
    let mut d = Driver::new(pads);
    // The RATE 30 setting, chosen here: the calls below are 30 a second.
    d.app.opts.fast = false;
    d.app.opts.marks = marks;
    d.start();
    let from = d.sent.len();
    for k in 0..30 * 60u32 {
        let mut held = [d.bot_held(), 0, 0, 0];
        for (p, h) in held.iter_mut().enumerate().skip(1) {
            let dirs = [button::LEFT, button::UP, button::RIGHT, button::DOWN];
            *h = dirs[(k as usize / 45 + p) % 4] | if k % 40 < 12 { button::A } else { 0 };
        }
        assert!(d.call(held));
        assert_eq!(d.app.scene, Scene::Play);
    }
    // The first frame after the title repaints everything: left out of
    // the average, reported as the start.
    let sent = &d.sent[from + 1..];
    let total: u64 = sent.iter().map(|(px, _)| u64::from(*px)).sum();
    let worst = sent.iter().map(|(px, _)| *px).max().unwrap_or(0);
    let worst_areas = sent.iter().map(|(_, a)| a.len()).max().unwrap_or(0);
    assert!(worst_areas <= MAX_AREAS);

    let dir = evidence("pixels");
    let mut csv = String::from("frame,pixels,areas\n");
    let mut trace = format!("# send-plan trace 1\n# game TUNDRA\n# case {name}\n# rate 30\n");
    for (k, (px, areas)) in sent.iter().enumerate() {
        let _ = writeln!(csv, "{k},{px},{}", areas.len());
        let _ = write!(trace, "S {} {}", k as u64 * u64::from(CALL_US), areas.len());
        for a in areas.as_slice() {
            let _ = write!(trace, " {},{},{},{}", a.x * 2, a.y * 2, a.w * 2, a.h * 2);
        }
        trace.push('\n');
    }
    std::fs::write(dir.join(format!("{name}.csv")), csv).unwrap();
    std::fs::write(dir.join(format!("{name}.trace")), trace).unwrap();
    Cost {
        avg: (total / sent.len() as u64) as u32,
        worst,
        worst_areas,
        frames: sent.len(),
    }
}

#[test]
fn a_minute_of_play_changes_a_small_part_of_the_picture() {
    let full = (W * H) as u32;
    let mut report = String::new();
    for (name, players, marks) in [
        ("one-player", 1, true),
        ("four-players", 4, true),
        ("one-player-no-marks", 1, false),
    ] {
        let c = minute(name, players, marks);
        let line = format!(
            "{name}: {} frames, changed pixels a frame: average {} ({}%), worst {} ({}%), most areas {}",
            c.frames,
            c.avg,
            c.avg * 100 / full,
            c.worst,
            c.worst * 100 / full,
            c.worst_areas
        );
        println!("{line}");
        report.push_str(&line);
        report.push('\n');
        // Small changes: the average under a sixth of the picture.
        assert!(c.avg < full / 6, "{line}");
    }
    std::fs::write(evidence("pixels").join("summary.txt"), report).unwrap();
}
