//! Pictures of every screen and of real play, saved for a person to look
//! at (in `.local/next/ship/shots/`, git-ignored), and checks that hold
//! for any picture: only the game's palette numbers.

mod common;

use common::{Driver, autopilot, check_palette, evidence, plan, save_frame, stick_of};
use game_kit::button;
use kestrel::app::Scene;
use kestrel::course::Kind;
use kestrel::scene::{Detail, Eye};
use kestrel::world::State;

fn shot(d: &Driver, name: &str) {
    check_palette(&d.frame, name);
    save_frame(&evidence("shots").join(format!("{name}.png")), &d.frame, 3);
}

/// Flies with the autopilot for `calls` calls (or until the mission
/// ends).
fn fly(d: &mut Driver, calls: usize) {
    for _ in 0..calls {
        if d.app.scene != Scene::Play {
            return;
        }
        let p = plan(&d.app.world);
        d.hold_stick(common::buttons_of(&p), stick_of(p.steer));
    }
}

#[test]
fn every_screen_and_real_play() {
    let mut d = Driver::new();
    d.idle(20);
    shot(&d, "01-title");
    d.tap(button::A);
    d.idle(25);
    shot(&d, "02-brief");

    // Real play in every detail and view: mission 2 at the same moment.
    for detail in Detail::ALL {
        for eye in Eye::ALL {
            let mut d = Driver::new();
            d.app.opts.mission = 1;
            d.app.opts.detail = detail;
            d.app.opts.eye = eye;
            d.play_from_title();
            fly(&mut d, 30 * 9);
            shot(
                &d,
                &format!(
                    "03-play-m2-{}-{}",
                    detail.name().to_lowercase(),
                    eye.name().to_lowercase()
                ),
            );
        }
    }

    // A wreck close by, a pod, a gate passed, in MID chase on mission 3.
    for (what, want) in [("wreck", Kind::Wreck), ("pod", Kind::Pod)] {
        let mut d = Driver::new();
        d.app.opts.mission = if want == Kind::Wreck { 1 } else { 2 };
        d.app.opts.detail = Detail::Mid;
        d.play_from_title();
        let mut found = false;
        for _ in 0..30 * 60 {
            fly(&mut d, 1);
            let w = &d.app.world;
            let p = w.ship.pos();
            let m = w.m();
            let mut list = kestrel::course::Things::new();
            let c = kestrel::course::Mission::cell_of(p[2] + 14 * 256);
            m.things(c, w.ticks, &mut list);
            if list.as_slice().iter().any(|t| t.kind == want) {
                found = true;
                break;
            }
        }
        assert!(found, "no {what} came near");
        shot(&d, &format!("04-near-{what}"));
    }

    // The pause menu over play.
    let mut d = Driver::new();
    d.app.opts.mission = 2;
    d.play_from_title();
    fly(&mut d, 30 * 12);
    d.tap(button::START);
    assert!(d.app.pause.is_some());
    shot(&d, "05-pause");
    d.tap(button::START);

    // The dock and the station.
    let mut d = Driver::new();
    d.play_from_title();
    while d.app.scene == Scene::Play && d.app.world.next_gate < d.app.world.m().gates {
        fly(&mut d, 1);
    }
    fly(&mut d, 30 * 2);
    shot(&d, "06-dock-ahead");
    assert_eq!(d.app.scene, Scene::Play);
    let trip = autopilot(&mut d, 30 * 60);
    assert!(trip.delivered);
    d.idle(45);
    shot(&d, "07-delivered");

    // A crash into the first rock wall: hold the stick hard down-left
    // and boost until the hull breaks or time runs out.
    let mut d = Driver::new();
    d.app.opts.mission = 2;
    d.play_from_title();
    let mut hit_shot = false;
    for _ in 0..30 * 90 {
        if d.app.scene != Scene::Play {
            break;
        }
        let hurt = d.app.world.ship.hurt;
        d.hold_stick(button::A, (0, 0));
        if !hit_shot && hurt == 0 && d.app.world.ship.hurt > 0 {
            shot(&d, "08-hit");
            hit_shot = true;
        }
    }
    d.idle(45);
    assert_ne!(d.app.world.state, State::Delivered);
    shot(&d, "09-failed");

    // The end of the run.
    let mut d = Driver::new();
    d.play_from_title();
    for n in 0..3 {
        assert!(autopilot(&mut d, 30 * 180).delivered);
        d.idle(45);
        d.tap(button::A);
        if n < 2 {
            d.launch();
        }
    }
    assert_eq!(d.app.scene, Scene::End);
    shot(&d, "10-end");
}
