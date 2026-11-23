//! The golden record: LANDER driven through its `game_entry` by the
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
    c.mark("lander", name);
}

fn script() -> TestConsole {
    // SAFETY: the game's own entry function, made by `game_kit::game!`.
    let mut c = unsafe { TestConsole::new(lander::game_entry) };
    // Title: look, then LEVEL 2 and VIEW FOLLOW.
    c.run_ms(1500);
    m(&mut c, "title");
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    m(&mut c, "title-options");
    c.tap(button::START);
    c.run_ms(600);
    m(&mut c, "ready-card");
    c.tap(button::A);
    // Flight 1: half power (the left trigger), upright, slow turns.
    c.triggers(0, 30_000, 0);
    c.hold(button::B);
    c.run_ms(1500);
    m(&mut c, "fly-hover");
    c.hold(button::R);
    c.run_ms(400);
    c.hold(button::L);
    c.run_ms(400);
    c.hold(button::B);
    c.triggers(0, 0, 22_000);
    c.run_ms(2500);
    m(&mut c, "fly-down");
    c.tap(button::SELECT);
    c.run_ms(500);
    m(&mut c, "view-still");
    c.hold(button::B);
    c.triggers(0, 0, 0);
    c.run_ms(6000);
    m(&mut c, "after-flight-1");
    c.hold(0);
    // The game's own pause menu: RESUME.
    c.tap(button::A);
    c.run_ms(800);
    c.tap(button::A);
    c.run_ms(500);
    c.tap(button::START);
    c.run_ms(300);
    m(&mut c, "game-pause");
    c.tap(button::A);
    // Flight 2: a finger pulls the craft about.
    c.finger(Some((30, 30)));
    c.run_ms(1500);
    m(&mut c, "finger-fly");
    c.finger(Some((140, 60)));
    c.run_ms(1500);
    c.finger(None);
    c.left_stick(0, 20_000, -25_000);
    c.run_ms(1500);
    c.left_stick(0, 0, 0);
    c.run_ms(6000);
    m(&mut c, "after-flight-2");
    // The console's own pause: time stands still.
    c.pause_ms(3000);
    c.run_ms(600);
    // Flights 3 and on: fall until the game is over.
    for _ in 0..4 {
        c.tap(button::A);
        c.run_ms(5000);
    }
    m(&mut c, "game-over");
    c.tap(button::A);
    c.run_ms(800);
    m(&mut c, "title-again");
    // RATE 30 (the game starts at 60), then play a little at 30 calls
    // a second.
    for _ in 0..4 {
        c.tap(button::DOWN);
    }
    c.tap(button::RIGHT);
    m(&mut c, "rate-30");
    c.tap(button::START);
    c.run_ms(500);
    c.tap(button::A);
    c.triggers(0, 0, 32_767);
    c.hold(button::LEFT);
    c.run_ms(700);
    c.hold(0);
    c.triggers(0, 0, 0);
    c.run_ms(1500);
    m(&mut c, "fly-30");
    // Pause menu: TITLE, then END.
    c.tap(button::START);
    c.tap(button::DOWN);
    c.tap(button::DOWN);
    c.tap(button::A);
    c.run_ms(500);
    m(&mut c, "title-from-pause");
    c.tap(button::UP);
    c.tap(button::A);
    assert!(!c.running(), "END leaves LANDER");
    c
}

#[test]
fn pictures_and_log_match_the_stored_record() {
    let c = script();
    c.require(30, 20_000);
    c.check_golden(
        GOLDEN,
        "LANDER golden record (tests/golden.rs). Lines: call, game ms, what.",
    );
}

#[test]
fn two_runs_give_the_same_record() {
    assert_eq!(script().record(), script().record());
}
