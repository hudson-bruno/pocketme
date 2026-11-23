//! Every level can be finished: an autopilot plays each one from its
//! map, with the d-pad and the A and B buttons only, through the same
//! `App::update` the console calls. Its button presses are saved as
//! scripts for the simulator.

mod common;

use common::{Driver, autopilot, evidence, sim_script};
use lantern::app::Scene;
use lantern::level::LEVELS;

/// Calls allowed for one level (30 a second): 6 minutes.
const MAX_CALLS: usize = 30 * 360;

fn play_level(n: u8, greedy: bool) -> (Driver, common::Trip) {
    let mut d = Driver::new();
    d.app.opts.level = n;
    d.play_from_title();
    d.record.clear();
    let trip = autopilot(&mut d, greedy, MAX_CALLS);
    (d, trip)
}

#[test]
fn every_level_is_finished_by_the_autopilot_taking_the_crank_only() {
    for n in 0..LEVELS.len() as u8 {
        let (d, trip) = play_level(n, false);
        let w = &d.app.world;
        println!(
            "level {}: cleared {} in {} calls ({} s), faints {}, flashes {}, hearts {}",
            n + 1,
            trip.cleared,
            trip.calls,
            trip.calls / 30,
            trip.faints,
            trip.flashes,
            w.life
        );
        assert!(trip.cleared, "level {} not cleared", n + 1);
        assert_eq!(d.app.scene, Scene::Clear);
    }
}

#[test]
fn every_level_is_finished_collecting_every_item() {
    let mut report = String::new();
    for n in 0..LEVELS.len() as u8 {
        let (d, trip) = play_level(n, true);
        let t = d.app.world.tally;
        let line = format!(
            "level {} {}: cleared {} in {} calls ({} s at 30 calls/s), crystals {}/{}, faints {}, flashes {}, moths sent {}, stuns {}, hurts {}",
            n + 1,
            LEVELS[usize::from(n)].name,
            trip.cleared,
            trip.calls,
            trip.calls / 30,
            t.crystals,
            t.crystals_total,
            trip.faints,
            trip.flashes,
            t.moths_sent,
            t.stuns,
            t.hurts,
        );
        println!("{line}");
        report.push_str(&line);
        report.push('\n');
        assert!(trip.cleared, "level {} not cleared", n + 1);
        if trip.faints == 0 {
            assert_eq!(t.crystals, t.crystals_total, "level {}", n + 1);
        }
        let dir = evidence("scripts");
        std::fs::write(
            dir.join(format!("level{}.txt", n + 1)),
            sim_script(&d.record),
        )
        .unwrap();
    }
    std::fs::write(evidence("").join("autopilot.txt"), report).unwrap();
}
