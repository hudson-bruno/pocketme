//! Pictures of every screen, saved for a person to look at (in
//! `.local/games/3d/look/`, git-ignored), and checks that hold for any
//! picture: only the game's palette numbers, the status line in place.

mod common;

use common::{Driver, autopilot, evidence, save_frame};
use game_kit::button;
use lantern::app::Scene;
use lantern::art;
use lantern::render::Detail;
use lantern::world::Kind;

fn check_palette(d: &Driver, what: &str) {
    let max = art::COLOURS.len() as u8;
    for (i, &p) in d.frame.0.iter().enumerate() {
        assert!(p < max, "{what}: pixel {i} has palette number {p}");
    }
}

fn shot(d: &Driver, name: &str) {
    check_palette(d, name);
    save_frame(&evidence("look").join(format!("{name}.png")), &d.frame, 3);
}

#[test]
fn every_screen_is_drawn_with_the_games_colours_only() {
    let d = Driver::new();
    shot(&d, "01-title");

    for (k, detail) in Detail::ALL.iter().enumerate() {
        let mut d = Driver::new();
        d.app.opts.detail = *detail;
        d.app.opts.level = 1;
        d.play_from_title();
        // Walk a little into the level.
        for _ in 0..20 {
            d.hold(button::UP);
        }
        for _ in 0..6 {
            d.hold(button::RIGHT);
        }
        d.hold(0);
        shot(
            &d,
            &format!("02-play-{}-{}", k, detail.name().to_lowercase()),
        );
    }

    // A creature: play level 3 until a crawler or digger is drawn close.
    let mut d = Driver::new();
    d.app.opts.level = 2;
    d.app.opts.detail = Detail::Normal;
    d.play_from_title();
    let mut found = false;
    for _ in 0..400 {
        autopilot(&mut d, true, 1);
        let w = &d.app.world;
        let near = w.things().iter().any(|t| {
            t.alive
                && matches!(t.kind, Kind::Crawler | Kind::Digger)
                && (t.x - w.x).abs() < 3 * 4096
                && (t.y - w.y).abs() < 3 * 4096
        });
        if near && d.app.render.cost.sprite_px > 300 {
            found = true;
            break;
        }
    }
    shot(&d, "03-creature");
    assert!(found, "no creature came into view");

    // The map, the pause menu.
    d.tap(button::SELECT);
    assert_eq!(d.app.scene, Scene::Map);
    shot(&d, "04-map");
    d.tap(button::SELECT);
    d.tap(button::START);
    assert!(d.app.pause.is_some());
    shot(&d, "05-pause");
    d.tap(button::START);

    // Level 1 to the end, then the card and the last screen.
    let mut d = Driver::new();
    d.play_from_title();
    let trip = autopilot(&mut d, true, 30 * 300);
    assert!(trip.cleared);
    d.idle(50);
    shot(&d, "06-clear");

    let mut d = Driver::new();
    d.app.opts.level = 3;
    d.play_from_title();
    assert!(autopilot(&mut d, false, 30 * 300).cleared);
    d.idle(50);
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::End);
    shot(&d, "07-end");
}
