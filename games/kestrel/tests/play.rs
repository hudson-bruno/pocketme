//! Every mission can be finished: an autopilot flies each one with the
//! left stick and the A and B buttons only, through the same
//! `App::update` the console calls, in every detail and view (the
//! detail and the view change only the pictures, never the flight).

mod common;

use common::{Driver, autopilot, evidence};
use game_kit::button;
use kestrel::app::Scene;
use kestrel::course::MISSIONS;
use kestrel::scene::{Detail, Eye};

/// Calls allowed for one mission (30 a second): 3 minutes.
const MAX_CALLS: usize = 30 * 180;

fn fly(n: u8, detail: Detail, eye: Eye) -> (Driver, common::Trip) {
    let mut d = Driver::new();
    d.app.opts.mission = n;
    d.app.opts.detail = detail;
    d.app.opts.eye = eye;
    d.play_from_title();
    let trip = autopilot(&mut d, MAX_CALLS);
    (d, trip)
}

#[test]
fn every_mission_is_delivered_by_the_autopilot() {
    let mut report = String::new();
    for n in 0..MISSIONS.len() as u8 {
        let (d, trip) = fly(n, Detail::Low, Eye::Chase);
        let w = &d.app.world;
        let t = w.tally;
        let line = format!(
            "mission {} {}: {:?} in {} calls ({} s flown), gates {}/{} missed {}, cells {}, hits {}, hull {}, time left {}.{} s, score {}",
            n + 1,
            MISSIONS[usize::from(n)].name,
            w.state,
            trip.calls,
            w.ticks / 60,
            t.passed,
            w.m().gates,
            t.missed,
            t.cells,
            t.hits,
            w.hull,
            w.clock_s10() / 10,
            w.clock_s10() % 10,
            w.score,
        );
        println!("{line}");
        report.push_str(&line);
        report.push('\n');
        assert!(trip.delivered, "mission {} not delivered: {line}", n + 1);
        assert_eq!(d.app.scene, Scene::Result);
    }
    std::fs::write(evidence("").join("autopilot.txt"), report).unwrap();
}

#[test]
fn detail_and_view_never_change_the_flight() {
    let (base, _) = fly(1, Detail::Low, Eye::Chase);
    for detail in Detail::ALL {
        for eye in Eye::ALL {
            let (d, _) = fly(1, detail, eye);
            assert_eq!(d.app.world.ship, base.app.world.ship, "{detail:?} {eye:?}");
            assert_eq!(d.app.world.score, base.app.world.score);
            assert_eq!(d.app.world.state, base.app.world.state);
        }
    }
}

#[test]
fn all_three_missions_in_a_row_reach_the_end() {
    let mut d = Driver::new();
    d.play_from_title();
    for n in 0..MISSIONS.len() {
        assert_eq!(usize::from(d.app.world.mission), n);
        let trip = autopilot(&mut d, MAX_CALLS);
        assert!(trip.delivered, "mission {}", n + 1);
        d.idle(45);
        d.tap(button::A);
        if n + 1 < MISSIONS.len() {
            d.launch();
        }
    }
    assert_eq!(d.app.scene, Scene::End);
    assert!(d.app.run.score > 0);
    assert_eq!(
        d.app.run.gates_total,
        MISSIONS.iter().map(|m| u32::from(m.gates)).sum()
    );
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Title);
}
