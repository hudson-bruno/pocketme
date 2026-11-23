//! The golden record: KESTREL driven through its `game_entry` by the
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
    c.mark("kestrel", name);
}

fn script() -> TestConsole {
    // SAFETY: the game's own entry function, made by `game_kit::game!`.
    let mut c = unsafe { TestConsole::new(kestrel::game_entry) };
    c.run_ms(1000);
    m(&mut c, "title");
    c.tap(button::START);
    c.run_ms(1500);
    m(&mut c, "briefing");
    c.tap(button::A);
    c.run_ms(1500);
    m(&mut c, "fly-chase");
    // Steer about the rock belt: rocks come near, pass the screen
    // edges and the near plane.
    for (x, y, ms) in [
        (20_000, 0, 900),
        (-26_000, 8_000, 900),
        (0, -20_000, 700),
        (12_000, 20_000, 900),
        (-32_000, -32_000, 600),
    ] {
        c.left_stick(0, x, y);
        c.run_ms(ms);
        m(&mut c, "steer");
    }
    c.left_stick(0, 0, 0);
    // Barrel rolls, boost and brake.
    c.tap(button::L);
    c.run_ms(500);
    c.tap(button::R);
    c.run_ms(300);
    m(&mut c, "roll");
    c.hold(button::A);
    c.run_ms(1200);
    m(&mut c, "boost");
    c.hold(button::B);
    c.run_ms(800);
    c.hold(0);
    // WIDE and NOSE views, then the details LOW and MID (the flight
    // starts in HIGH).
    c.tap(button::Y);
    c.hold(button::RIGHT | button::UP);
    c.run_ms(1000);
    m(&mut c, "wide");
    c.hold(0);
    c.tap(button::Y);
    c.hold(button::LEFT);
    c.run_ms(1000);
    m(&mut c, "nose");
    c.hold(0);
    c.tap(button::X);
    c.left_stick(0, 18_000, -6_000);
    c.run_ms(1000);
    m(&mut c, "nose-low");
    c.tap(button::Y);
    c.tap(button::X);
    c.left_stick(0, -18_000, 10_000);
    c.run_ms(1200);
    m(&mut c, "chase-mid");
    c.left_stick(0, 0, 0);
    // A finger on the view, the right stick and the triggers.
    c.finger(Some((30, 30)));
    c.run_ms(800);
    c.finger(None);
    c.right_stick(0, 0, -30_000);
    c.run_ms(800);
    c.right_stick(0, 30_000, 0);
    c.run_ms(200);
    c.right_stick(0, 0, 0);
    c.triggers(0, 30_000, 0);
    c.run_ms(500);
    c.triggers(0, 0, 0);
    m(&mut c, "touch-and-sticks");
    // The game's pause menu, the console's pause.
    c.tap(button::START);
    c.run_ms(300);
    m(&mut c, "game-pause");
    c.tap(button::START);
    c.pause_ms(2000);
    // Fly straight on until the mission ends.
    c.run_ms(9000);
    m(&mut c, "straight-on");
    c.run_ms(6000);
    m(&mut c, "mission-end");
    c.tap(button::A);
    c.run_ms(1000);
    m(&mut c, "after-card");
    c.tap(button::A);
    c.run_ms(1000);
    // Back to the title (pause menu, last line), mission 3, 30 a second.
    c.tap(button::START);
    c.tap(button::UP);
    c.tap(button::A);
    c.run_ms(500);
    m(&mut c, "title-again");
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.tap(button::RIGHT);
    c.tap(button::DOWN);
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    m(&mut c, "title-options");
    c.tap(button::START);
    c.run_ms(1500);
    c.tap(button::A);
    c.run_ms(1000);
    m(&mut c, "mission-3");
    for k in 0..6 {
        c.left_stick(0, if k % 2 == 0 { 24_000 } else { -24_000 }, 9_000);
        c.run_ms(600);
    }
    c.left_stick(0, 0, 0);
    c.tap(button::SELECT);
    c.run_ms(1500);
    m(&mut c, "mission-3-60");
    // TITLE, then LEAVE.
    c.tap(button::START);
    c.tap(button::UP);
    c.tap(button::A);
    c.run_ms(300);
    c.tap(button::UP);
    c.tap(button::A);
    c.run_ms(300);
    assert!(!c.running(), "LEAVE ends KESTREL");
    c
}

#[test]
fn pictures_and_log_match_the_stored_record() {
    let c = script();
    c.require(60, 20_000);
    c.check_golden(
        GOLDEN,
        "KESTREL golden record (tests/golden.rs). Lines: call, game ms, what.",
    );
}

#[test]
fn two_runs_give_the_same_record() {
    assert_eq!(script().record(), script().record());
}
