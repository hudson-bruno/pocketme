//! The golden record: MERCATOR driven through its `game_entry` by the
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
    c.mark("mercator", name);
}

fn script() -> TestConsole {
    // SAFETY: the game's own entry function, made by `game_kit::game!`.
    let mut c = unsafe { TestConsole::new(mercator::game_entry) };
    c.run_ms(1500);
    m(&mut c, "title");
    // PLAY: docked at the first station.
    c.tap(button::A);
    c.run_ms(800);
    m(&mut c, "docked");
    // MARKET: buy a few tonnes, sell one.
    c.tap(button::DOWN);
    c.tap(button::A);
    c.run_ms(400);
    m(&mut c, "market");
    c.tap(button::A);
    c.tap(button::A);
    c.tap(button::DOWN);
    c.hold(button::A);
    c.run_ms(1200);
    c.hold(0);
    c.tap(button::X);
    c.run_ms(300);
    m(&mut c, "market-bought");
    c.tap(button::B);
    // OUTFIT and CHART, looked at.
    c.tap(button::DOWN);
    c.tap(button::A);
    c.run_ms(400);
    m(&mut c, "outfit");
    c.tap(button::B);
    c.tap(button::DOWN);
    c.tap(button::A);
    c.run_ms(400);
    m(&mut c, "chart");
    c.tap(button::X);
    c.run_ms(400);
    m(&mut c, "chart-whole");
    c.tap(button::X);
    c.tap(button::RIGHT);
    c.tap(button::A);
    c.run_ms(300);
    c.tap(button::B);
    // LAUNCH: the station close behind, the planet and sun far.
    c.tap(button::UP);
    c.tap(button::UP);
    c.tap(button::UP);
    c.tap(button::A);
    c.run_ms(2000);
    m(&mut c, "launch");
    // Fly about: pitch, yaw, roll; faster and slower.
    for (x, y, ms) in [
        (32_767, 0, 1200),
        (0, -32_767, 900),
        (-20_000, 20_000, 1000),
    ] {
        c.left_stick(0, x, y);
        c.run_ms(ms);
        m(&mut c, "turn");
    }
    c.left_stick(0, 0, 0);
    c.right_stick(0, 30_000, 0);
    c.run_ms(800);
    c.right_stick(0, 0, 0);
    c.triggers(0, 0, 32_767);
    c.run_ms(1500);
    c.triggers(0, 0, 0);
    m(&mut c, "faster");
    // Fire the laser; the gunner on pad 2 too.
    c.set_pad(1, true);
    c.hold(button::A);
    c.hold_player(1, button::A);
    c.run_ms(800);
    c.hold(0);
    c.hold_player(1, 0);
    m(&mut c, "fire");
    // Up to here the starting settings: CHASE view, HIGH detail. The
    // pause menu turns them to COCKPIT and MID (DETAIL steps HIGH, LOW,
    // MID; VIEW steps to COCKPIT), then auto-dock.
    c.tap(button::START);
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.tap(button::RIGHT);
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.run_ms(300);
    m(&mut c, "game-pause");
    c.tap(button::START);
    c.run_ms(1000);
    m(&mut c, "cockpit-mid");
    c.tap(button::Y);
    c.run_ms(8000);
    m(&mut c, "auto-dock");
    c.pause_ms(2000);
    c.run_ms(8000);
    m(&mut c, "auto-dock-later");
    // The chart in flight; A jumps when a course is set.
    c.tap(button::SELECT);
    c.run_ms(400);
    m(&mut c, "flight-chart");
    c.tap(button::RIGHT);
    c.tap(button::A);
    c.run_ms(3000);
    m(&mut c, "after-jump");
    c.tap(button::Y);
    c.left_stick(0, 0, 32_767);
    c.run_ms(1500);
    c.left_stick(0, 0, 0);
    m(&mut c, "new-system");
    // TITLE from the pause menu (last line), then 60 a second.
    c.tap(button::START);
    c.tap(button::UP);
    c.tap(button::A);
    c.run_ms(800);
    m(&mut c, "title-again");
    c.tap(button::A);
    c.run_ms(500);
    c.tap(button::A);
    c.run_ms(1500);
    c.tap(button::SELECT);
    c.tap(button::B);
    c.left_stick(0, 32_767, 32_767);
    c.run_ms(1500);
    c.left_stick(0, 0, 0);
    m(&mut c, "second-launch");
    c.quit();
    c
}

#[test]
fn pictures_and_log_match_the_stored_record() {
    let c = script();
    c.require(60, 20_000);
    c.check_golden(
        GOLDEN,
        "MERCATOR golden record (tests/golden.rs). Lines: call, game ms, what.",
    );
}

#[test]
fn two_runs_give_the_same_record() {
    assert_eq!(script().record(), script().record());
}
