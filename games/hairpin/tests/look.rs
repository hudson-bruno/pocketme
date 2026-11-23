//! Pictures of real play, saved for a person to look at (in
//! `.local/next/racing/shots/`, git-ignored), and checks that hold for
//! any picture: only the game's own palette numbers.

mod common;

use common::{Driver, autopilot, evidence, save_frame};
use game_kit::button;
use hairpin::app::{CARD_WAIT, Scene};
use hairpin::art;
use hairpin::hud::MENU_Y;
use hairpin::render::{Cam, Detail, TOP};

fn check_palette(d: &Driver, what: &str) {
    let max = art::COLOURS.len() as u8;
    for (i, &p) in d.frame.0.iter().enumerate() {
        assert!(p < max, "{what}: pixel {i} has palette number {p}");
    }
}

fn shot(d: &Driver, name: &str) {
    check_palette(d, name);
    save_frame(&evidence("shots").join(format!("{name}.png")), &d.frame, 3);
}

/// Races course `c` with the autopilot for `calls` calls.
fn drive(c: u8, detail: Detail, cam: Cam, calls: usize) -> Driver {
    let mut d = Driver::new();
    d.app.opts.detail = detail;
    d.app.opts.cam = cam;
    d.race_course(c);
    autopilot(&mut d, calls);
    d
}

#[test]
fn every_screen_is_drawn_with_the_games_colours_only() {
    let d = Driver::new();
    shot(&d, "01-title");

    // The grid in the countdown.
    let mut d = Driver::new();
    d.play_from_title();
    d.idle(40);
    shot(&d, "02-countdown");

    // Each course in each detail, some way into the race.
    for c in 0..3u8 {
        for detail in Detail::ALL {
            let d = drive(c, detail, Cam::Chase, 90 + 160 * usize::from(c) + 230);
            shot(
                &d,
                &format!("03-course{}-{}", c + 1, detail.name().to_lowercase()),
            );
        }
    }
    // A sharp bend on the summit course, and the nose view.
    let d = drive(2, Detail::Light, Cam::Chase, 90 + 330);
    shot(&d, "04-summit-bend");
    let d = drive(1, Detail::Light, Cam::Nose, 90 + 300);
    shot(&d, "05-nose-light");
    let d = drive(1, Detail::Heavy, Cam::Nose, 90 + 300);
    shot(&d, "05-nose-heavy");

    // The pause menu.
    let mut d = drive(0, Detail::Light, Cam::Chase, 200);
    d.tap(button::START);
    assert!(d.app.pause.is_some());
    shot(&d, "06-pause");

    // The finish card, then the end of the cup.
    let mut d = Driver::new();
    d.race_course(2);
    let trip = autopilot(&mut d, 30 * 360);
    assert!(trip.finished);
    d.idle(CARD_WAIT);
    shot(&d, "07-finish");
    d.app.cup_places = [1, 2, 1];
    d.app.cup_times = [3800, 4400, 5100];
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Cup);
    shot(&d, "08-cup");
}

/// The rows a note changes in the picture: drawn once without it and
/// once with it, the same moment.
fn note_rows(d: &mut Driver) -> (i32, i32) {
    let mut without = Box::new(game_kit::Frame::new());
    let mut with = Box::new(game_kit::Frame::new());
    d.app.note = None;
    d.app.draw(&mut without);
    d.app.note = Some(("FINAL LAP", 75));
    d.app.draw(&mut with);
    let rows = (0..120i32)
        .filter(|&y| {
            let r = (y * 160) as usize..((y + 1) * 160) as usize;
            without.0[r.clone()] != with.0[r]
        })
        .collect::<Vec<_>>();
    assert!(!rows.is_empty(), "the note is drawn");
    (rows[0], *rows.last().unwrap())
}

#[test]
fn a_note_covers_neither_the_lap_times_nor_a_menu_or_card() {
    for detail in Detail::ALL {
        let v = detail.view();
        // Racing: inside the view (in LIGHT the lap times and the lap
        // strip sit above it), and above the countdown's box.
        let mut d = drive(0, detail, Cam::Chase, 20);
        let (top, bottom) = note_rows(&mut d);
        assert!(
            top >= v.y0 && bottom < (v.y0 + 18).max(46),
            "{detail:?} racing: {top}..={bottom}"
        );
        // Paused: between the top bar and the pause panel.
        d.tap(button::START);
        assert!(d.app.pause.is_some());
        let (top, bottom) = note_rows(&mut d);
        assert!(
            top >= TOP && bottom < i32::from(MENU_Y) - 15,
            "{detail:?} paused: {top}..={bottom}"
        );
    }
    // The finish card (its panel starts at row 22).
    let mut d = Driver::new();
    d.race_course(0);
    assert!(autopilot(&mut d, 30 * 360).finished);
    assert_eq!(d.app.scene, Scene::Finish);
    let (top, bottom) = note_rows(&mut d);
    assert!(top >= TOP && bottom < 22, "finish card: {top}..={bottom}");
}
