//! The golden record: TUNDRA driven through its `game_entry` by the
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
    c.mark("tundra", name);
}

fn script() -> TestConsole {
    // SAFETY: the game's own entry function, made by `game_kit::game!`.
    let mut c = unsafe { TestConsole::new(tundra::game_entry) };
    c.run_ms(1500);
    m(&mut c, "title");
    // MATCH one step back.
    for _ in 0..3 {
        c.tap(button::DOWN);
    }
    c.tap(button::LEFT);
    c.run_ms(300);
    m(&mut c, "title-set");
    for _ in 0..3 {
        c.tap(button::UP);
    }
    c.tap(button::A);
    c.run_ms(2500);
    m(&mut c, "face-off");
    // Pad 1 skates, passes, shoots, checks, takes the other skater.
    for (x, y, b) in [
        (32_767, 0, button::A),
        (32_767, -20_000, button::B),
        (0, 32_767, button::X),
        (32_767, 20_000, button::A),
        (-32_767, 0, button::B),
        (20_000, -32_767, button::Y),
        (32_767, 32_767, button::A),
        (-20_000, 20_000, button::B),
    ] {
        c.left_stick(0, x, y);
        c.run_ms(700);
        c.tap(b);
        c.run_ms(700);
        m(&mut c, "play");
    }
    // A wound-up shot: hold A, let go.
    c.left_stick(0, 32_767, 0);
    c.hold(button::A);
    c.run_ms(900);
    c.hold(0);
    c.run_ms(600);
    c.left_stick(0, 0, 0);
    m(&mut c, "shot");
    // A finger on the ice takes player 2; a tap shoots or checks.
    for (x, y) in [(100, 60), (80, 40), (60, 90), (90, 60), (70, 70)] {
        c.finger(Some((x, y)));
        c.run_ms(500);
    }
    c.finger(None);
    c.touch(50, 60);
    c.run_ms(500);
    m(&mut c, "touch-p2");
    c.run_ms(2000);
    // Pause: the computer level, then RESTART.
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
    // TITLE; PLAYER 2 COMPUTER, ICE MARKS kept; pad 1 drops out and the
    // computer plays everyone to the end.
    c.tap(button::START);
    for _ in 0..4 {
        c.tap(button::DOWN);
    }
    c.tap(button::A);
    c.run_ms(500);
    m(&mut c, "title-back");
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.tap(button::UP);
    c.run_ms(300);
    m(&mut c, "title-cpu");
    c.tap(button::A);
    c.run_ms(500);
    c.set_pad(0, false);
    // Until the final (at most 8 minutes).
    for _ in 0..48 {
        c.run_ms(10_000);
        m(&mut c, "cpu-match");
        if c.log_lines().iter().any(|l| l.contains("final")) {
            break;
        }
    }
    c.set_pad(0, true);
    c.run_ms(1000);
    m(&mut c, "end");
    // REMATCH with four pads for a while; then LEAVE.
    c.set_pad(1, true);
    c.set_pad(2, true);
    c.set_pad(3, true);
    c.tap(button::A);
    c.run_ms(3000);
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
        "TUNDRA golden record (tests/golden.rs). Lines: call, game ms, what.",
    );
}

#[test]
fn two_runs_give_the_same_record() {
    assert_eq!(script().record(), script().record());
}
