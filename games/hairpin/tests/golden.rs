//! The golden record: HAIRPIN driven through its `game_entry` by the
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
    c.mark("hairpin", name);
}

fn script() -> TestConsole {
    // SAFETY: the game's own entry function, made by `game_kit::game!`.
    let mut c = unsafe { TestConsole::new(hairpin::game_entry) };
    c.run_ms(1000);
    m(&mut c, "title");
    c.tap(button::START);
    c.run_ms(1200);
    m(&mut c, "countdown");
    c.run_ms(2500);
    // Race: throttle, steer with the stick through the first bends.
    c.hold(button::A);
    c.run_ms(2000);
    m(&mut c, "race");
    for k in 0..6i16 {
        c.left_stick(0, if k % 2 == 0 { 14_000 } else { -9_000 }, 0);
        c.run_ms(700);
    }
    c.left_stick(0, 0, 0);
    m(&mut c, "steer");
    // Boost, then brake hard.
    c.hold(button::A | button::X);
    c.run_ms(1500);
    m(&mut c, "boost");
    c.hold(button::B);
    c.run_ms(600);
    c.hold(button::A);
    // NOSE view, then the details (the race starts in HEAVY).
    c.tap(button::Y);
    c.hold(button::A | button::RIGHT);
    c.run_ms(1000);
    m(&mut c, "nose");
    c.hold(button::A);
    c.tap(button::SELECT);
    c.hold(button::A);
    c.run_ms(1000);
    m(&mut c, "light");
    c.tap(button::SELECT);
    c.hold(button::A);
    c.run_ms(1000);
    m(&mut c, "normal");
    c.tap(button::Y);
    c.hold(button::A | button::LEFT);
    c.run_ms(1000);
    m(&mut c, "normal-chase");
    c.hold(0);
    // A finger drives: full throttle, steering by its place.
    c.finger(Some((120, 70)));
    c.run_ms(1200);
    c.finger(Some((40, 70)));
    c.run_ms(1200);
    c.finger(None);
    m(&mut c, "touch-drive");
    // Triggers.
    c.triggers(0, 0, 32_767);
    c.right_stick(0, 20_000, 0);
    c.run_ms(1500);
    c.triggers(0, 25_000, 0);
    c.right_stick(0, 0, 0);
    c.run_ms(500);
    c.triggers(0, 0, 0);
    m(&mut c, "triggers");
    // The game's pause menu, the console's pause.
    c.tap(button::START);
    c.run_ms(300);
    m(&mut c, "game-pause");
    c.tap(button::START);
    c.pause_ms(2500);
    c.hold(button::A);
    c.run_ms(1500);
    c.hold(0);
    m(&mut c, "after-pause");
    // TITLE from the pause menu (the last line).
    c.tap(button::START);
    c.tap(button::UP);
    c.tap(button::A);
    c.run_ms(500);
    m(&mut c, "title-again");
    // SUMMIT, LIGHT, 30, NOSE (detail NORMAL -> HEAVY -> LIGHT, rate
    // 60 -> 30).
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.tap(button::RIGHT);
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.tap(button::RIGHT);
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    m(&mut c, "title-options");
    c.tap(button::START);
    c.run_ms(3800);
    c.hold(button::A);
    c.run_ms(2500);
    m(&mut c, "summit");
    c.hold(button::A | button::LEFT);
    c.run_ms(1200);
    c.hold(button::A | button::RIGHT);
    c.run_ms(1200);
    c.hold(0);
    m(&mut c, "summit-bends");
    c.tap(button::START);
    c.tap(button::UP);
    c.tap(button::A);
    c.run_ms(300);
    c.tap(button::UP);
    c.tap(button::A);
    c.run_ms(300);
    assert!(!c.running(), "LEAVE ends HAIRPIN");
    c
}

#[test]
fn pictures_and_log_match_the_stored_record() {
    let c = script();
    c.require(60, 20_000);
    c.check_golden(
        GOLDEN,
        "HAIRPIN golden record (tests/golden.rs). Lines: call, game ms, what.",
    );
}

#[test]
fn two_runs_give_the_same_record() {
    assert_eq!(script().record(), script().record());
}
