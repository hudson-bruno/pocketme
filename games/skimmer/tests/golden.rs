//! The golden record: SKIMMER driven through its `game_entry` by the
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
    c.mark("skimmer", name);
}

fn script() -> TestConsole {
    // SAFETY: the game's own entry function, made by `game_kit::game!`.
    let mut c = unsafe { TestConsole::new(skimmer::game_entry) };
    c.run_ms(1500);
    m(&mut c, "title");
    // MATCH: FIRST TO 5; TABLE: WILD.
    for _ in 0..5 {
        c.tap(button::DOWN);
    }
    c.tap(button::LEFT);
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.run_ms(300);
    m(&mut c, "title-set");
    for _ in 0..6 {
        c.tap(button::UP);
    }
    c.tap(button::A);
    c.run_ms(1500);
    m(&mut c, "serve");
    // Pad 1 plays CORAL against the computer: stick moves and dashes.
    for (x, y) in [
        (32_767, 0),
        (32_767, -20_000),
        (0, 32_767),
        (32_767, 20_000),
        (-32_767, 0),
        (20_000, -32_767),
        (32_767, 32_767),
        (-20_000, 20_000),
    ] {
        c.left_stick(0, x, y);
        c.run_ms(700);
        c.tap(button::A);
        c.run_ms(700);
        m(&mut c, "play");
    }
    c.left_stick(0, 0, 0);
    // A finger on the right half takes TEAL (no pad 2).
    for (x, y) in [(120, 60), (110, 40), (130, 90), (100, 60), (125, 70)] {
        c.finger(Some((x, y)));
        c.run_ms(500);
    }
    m(&mut c, "touch-teal");
    c.finger(None);
    c.run_ms(2000);
    // Pause: COMPUTER HARD, then RESTART.
    c.tap(button::START);
    c.run_ms(200);
    m(&mut c, "pause");
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.tap(button::DOWN);
    c.tap(button::DOWN);
    c.tap(button::A);
    c.run_ms(1500);
    m(&mut c, "restart");
    c.hold(button::RIGHT | button::DOWN);
    c.run_ms(3000);
    c.hold(button::UP);
    c.run_ms(2000);
    c.hold(0);
    m(&mut c, "dpad");
    // TITLE from the pause menu; then CORAL COMPUTER, RATE 30, and the
    // computer plays both sides to the end.
    c.tap(button::START);
    for _ in 0..4 {
        c.tap(button::DOWN);
    }
    c.tap(button::A);
    c.run_ms(500);
    m(&mut c, "title-back");
    c.tap(button::DOWN);
    c.tap(button::LEFT);
    for _ in 0..6 {
        c.tap(button::DOWN);
    }
    c.tap(button::RIGHT);
    c.run_ms(300);
    m(&mut c, "title-cpu");
    for _ in 0..7 {
        c.tap(button::UP);
    }
    c.tap(button::A);
    // Until the final (at most 5 minutes).
    for _ in 0..30 {
        c.run_ms(10_000);
        m(&mut c, "cpu-match");
        if c.log_lines().iter().any(|l| l.contains("final")) {
            break;
        }
    }
    c.run_ms(1000);
    m(&mut c, "end");
    // REMATCH is the first line; TITLE the second.
    c.tap(button::DOWN);
    c.tap(button::A);
    c.run_ms(500);
    m(&mut c, "title-end");
    // 2 AGAINST 2 with four pads, for a while; then LEAVE.
    c.set_pad(1, true);
    c.set_pad(2, true);
    c.set_pad(3, true);
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.tap(button::DOWN);
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    for _ in 0..3 {
        c.tap(button::UP);
    }
    c.run_ms(300);
    m(&mut c, "title-four");
    c.tap(button::A);
    c.run_ms(1000);
    for p in 0..4 {
        c.left_stick(p, if p % 2 == 0 { 32_767 } else { -32_767 }, 10_000);
    }
    c.run_ms(2500);
    m(&mut c, "four");
    for p in 0..4 {
        c.left_stick(p, 0, -32_767);
    }
    c.run_ms(2500);
    m(&mut c, "four-up");
    c.tap(button::START);
    c.tap(button::UP);
    c.tap(button::A);
    c
}

#[test]
fn pictures_and_log_match_the_stored_record() {
    let c = script();
    c.require(30, 20_000);
    c.check_golden(
        GOLDEN,
        "SKIMMER golden record (tests/golden.rs). Lines: call, game ms, what.",
    );
}

#[test]
fn two_runs_give_the_same_record() {
    assert_eq!(script().record(), script().record());
}
