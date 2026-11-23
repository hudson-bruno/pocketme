//! The golden record: STARDUST driven through its `game_entry` by the
//! kit's stand-in console with a fixed script, every different picture
//! (with the palette) and every log line compared with
//! `tests/data/golden.txt`.
//!
//! When the game's look changes on purpose, look at the pictures first
//! (`GOLDEN_SHOTS=<folder>` writes one PNG at each mark), then write the
//! new record with `UPDATE_GOLDEN=1 cargo test --test golden`.

use game_kit::button;
use game_kit::testing::TestConsole;

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/golden.txt");

fn script() -> TestConsole {
    // SAFETY: the game's own entry function, made by `game_kit::game!`.
    let mut c = unsafe { TestConsole::new(stardust::game_entry) };
    let m = |c: &mut TestConsole, name: &str| c.mark("stardust", name);
    c.run_ms(1000);
    m(&mut c, "start");
    for (b, ms) in [
        (button::RIGHT, 1500),
        (button::DOWN, 1000),
        (button::LEFT, 2200),
        (button::UP, 900),
        (button::RIGHT | button::DOWN, 1200),
    ] {
        c.hold(b);
        c.run_ms(ms);
        m(&mut c, "dpad");
    }
    c.hold(0);
    c.left_stick(0, -20_000, -30_000);
    c.run_ms(1500);
    c.left_stick(0, 32_767, 9_000);
    c.run_ms(1500);
    c.left_stick(0, 0, 0);
    m(&mut c, "stick");
    // A finger on three of the first stars: the comet flies to each.
    for (x, y) in [(105, 47), (147, 36), (15, 47)] {
        c.finger(Some((x, y)));
        c.run_ms(1500);
        m(&mut c, "finger");
    }
    c.finger(None);
    c.run_ms(200);
    c.pause_ms(5000);
    c.run_ms(3000);
    m(&mut c, "after-pause");
    c.hold(button::LEFT | button::UP);
    c.run_ms(2500);
    c.hold(0);
    c.run_ms(2000);
    m(&mut c, "idle");
    c.quit();
    // A second run, ended by the game itself.
    c.begin();
    c.run_ms(1500);
    m(&mut c, "second-run");
    c.tap(button::START);
    assert!(!c.running(), "START ends STARDUST");
    c
}

#[test]
fn pictures_and_log_match_the_stored_record() {
    let c = script();
    c.require(30, 20_000);
    c.check_golden(
        GOLDEN,
        "STARDUST golden record (tests/golden.rs). Lines: call, game ms, what.",
    );
}

#[test]
fn two_runs_give_the_same_record() {
    assert_eq!(script().record(), script().record());
}
