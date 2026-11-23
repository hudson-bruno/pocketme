//! The golden record: CAMBER driven through its `game_entry` by the
//! kit's stand-in console (`game_kit::testing`) with a fixed script.
//! Every different picture (pixels and palette, as a hash) and every
//! log line is compared with `tests/data/golden.txt`.
//!
//! When the game's look changes on purpose, look at the pictures first
//! (`GOLDEN_SHOTS=<folder>` writes one PNG at each mark), then write the
//! new record with `UPDATE_GOLDEN=1 cargo test --test golden`.

use game_kit::button;
use game_kit::testing::TestConsole;

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/golden.txt");

fn m(c: &mut TestConsole, name: &str) {
    c.mark("camber", name);
}

fn script() -> TestConsole {
    // SAFETY: the game's own entry function, made by `game_kit::game!`.
    let mut c = unsafe { TestConsole::new(camber::game_entry) };
    // The title runs at 30 a second (its made-up race behind the menu).
    // PLAY when the game has drawn 113 pictures: the race's seed and the
    // blinking of its marks (the picture count, by 7 and by 16) are then
    // the same as when the title stood still and drew one, so the race
    // below is the same race, picture for picture.
    c.run_ms(1500);
    m(&mut c, "title");
    c.run_ms(2200);
    m(&mut c, "title-later");
    assert_eq!(c.calls() % 112, 1, "PLAY on the 113th picture");
    c.tap(button::A);
    c.run_ms(1500);
    m(&mut c, "grid");
    c.run_ms(2500);
    // Race: thrust, steer about the track (walls, near track pieces cut
    // at the screen edge and the near plane).
    c.hold(button::A);
    c.run_ms(2500);
    m(&mut c, "race");
    for (x, ms) in [(20_000, 800), (-28_000, 900), (32_767, 600), (-10_000, 900)] {
        c.left_stick(0, x, 0);
        c.run_ms(ms);
        m(&mut c, "steer");
    }
    // A carve (L, stick in the middle: left), held on by R too, then
    // the brake (B), which lets go of it.
    c.left_stick(0, 0, 0);
    c.hold(button::A | button::L);
    c.run_ms(400);
    m(&mut c, "carve");
    c.run_ms(200);
    c.hold(button::A | button::R);
    c.run_ms(600);
    c.hold(button::B);
    c.run_ms(500);
    m(&mut c, "brake");
    // Views FAR and NOSE, details HIGH and LOW, 60 a second.
    c.hold(button::A);
    c.tap(button::Y);
    c.hold(button::A | button::LEFT);
    c.run_ms(1000);
    m(&mut c, "far");
    c.tap(button::Y);
    c.hold(button::A | button::RIGHT);
    c.run_ms(1000);
    m(&mut c, "nose");
    c.tap(button::X);
    c.hold(button::A);
    c.run_ms(1000);
    m(&mut c, "nose-high");
    c.tap(button::Y);
    c.tap(button::X);
    c.hold(button::A);
    c.run_ms(1000);
    m(&mut c, "chase-low");
    c.tap(button::SELECT);
    c.hold(button::A);
    c.triggers(0, 0, 32_767);
    c.run_ms(1500);
    m(&mut c, "chase-low-60");
    c.triggers(0, 0, 0);
    c.hold(0);
    // A finger, then the pause menu and the console's pause.
    c.finger(Some((20, 80)));
    c.run_ms(800);
    c.finger(None);
    c.tap(button::START);
    c.run_ms(300);
    m(&mut c, "game-pause");
    c.tap(button::A);
    c.pause_ms(2000);
    c.hold(button::A);
    c.run_ms(6000);
    m(&mut c, "race-on");
    c.hold(0);
    // TITLE (two up from RESUME), then GLACIER, WILD, TIME TRIAL, and on
    // the OPTIONS page HIGH and NOSE.
    c.tap(button::START);
    c.tap(button::UP);
    c.tap(button::UP);
    c.tap(button::A);
    c.run_ms(800);
    m(&mut c, "title-again");
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.tap(button::RIGHT);
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.run_ms(500);
    m(&mut c, "title-glacier");
    c.tap(button::DOWN);
    c.tap(button::A);
    c.tap(button::RIGHT);
    c.tap(button::DOWN);
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.tap(button::RIGHT);
    c.run_ms(500);
    m(&mut c, "title-options");
    // B back to the main page (on OPTIONS), up four to PLAY.
    c.tap(button::B);
    for _ in 0..4 {
        c.tap(button::UP);
    }
    c.tap(button::A);
    c.run_ms(4000);
    c.hold(button::A);
    c.run_ms(3000);
    m(&mut c, "glacier");
    for x in [-24_000, 24_000, -12_000] {
        c.left_stick(0, x, 0);
        c.run_ms(700);
    }
    c.left_stick(0, 0, 0);
    c.hold(0);
    m(&mut c, "glacier-steer");
    // TITLE from the pause menu (two up from RESUME); on the OPTIONS
    // page the BACKDROP line to SHIP: the player's ship turning.
    c.tap(button::START);
    c.tap(button::UP);
    c.tap(button::UP);
    c.tap(button::A);
    for _ in 0..4 {
        c.tap(button::DOWN);
    }
    c.tap(button::A);
    for _ in 0..4 {
        c.tap(button::DOWN);
    }
    c.tap(button::RIGHT);
    c.run_ms(1000);
    m(&mut c, "title-ship");
    c.run_ms(2000);
    m(&mut c, "title-ship-later");
    // B back to the main page (on OPTIONS), down one to LEAVE.
    c.tap(button::B);
    c.tap(button::DOWN);
    c.tap(button::A);
    c.run_ms(300);
    assert!(!c.running(), "LEAVE ends CAMBER");
    c
}

#[test]
fn pictures_and_log_match_the_stored_record() {
    let c = script();
    c.require(60, 20_000);
    c.check_golden(
        GOLDEN,
        "CAMBER golden record (tests/golden.rs). Lines: call, game ms, what.",
    );
}

#[test]
fn two_runs_give_the_same_record() {
    assert_eq!(script().record(), script().record());
}
