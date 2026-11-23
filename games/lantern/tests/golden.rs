//! The golden record: LANTERN driven through its `game_entry` by the
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
    c.mark("lantern", name);
}

fn script() -> TestConsole {
    // SAFETY: the game's own entry function, made by `game_kit::game!`.
    let mut c = unsafe { TestConsole::new(lantern::game_entry) };
    c.run_ms(1000);
    m(&mut c, "title");
    c.tap(button::START);
    c.run_ms(1500);
    m(&mut c, "level-1");
    // Walk, turn, look: near and far walls, a door ahead.
    c.hold(button::UP);
    c.run_ms(1200);
    m(&mut c, "walk");
    c.hold(button::RIGHT);
    c.run_ms(700);
    m(&mut c, "turn");
    c.hold(button::UP | button::LEFT);
    c.run_ms(1500);
    c.hold(0);
    c.tap(button::A);
    c.tap(button::B);
    c.run_ms(300);
    m(&mut c, "use-and-flash");
    // Details: from HEAVY (the start) to LIGHT, NORMAL, back to HEAVY.
    c.tap(button::X);
    c.right_stick(0, 20_000, 0);
    c.run_ms(800);
    m(&mut c, "light");
    c.tap(button::X);
    c.right_stick(0, -26_000, 0);
    c.left_stick(0, 0, -30_000);
    c.run_ms(1500);
    m(&mut c, "normal");
    c.right_stick(0, 0, 0);
    c.left_stick(0, 12_000, 18_000);
    c.triggers(0, 0, 30_000);
    c.run_ms(1000);
    c.left_stick(0, 0, 0);
    c.triggers(0, 0, 0);
    m(&mut c, "normal-back");
    // The map, then back.
    c.tap(button::SELECT);
    c.run_ms(800);
    m(&mut c, "map");
    c.tap(button::SELECT);
    c.tap(button::X);
    m(&mut c, "heavy");
    // Touch: the top third walks, the middle third turns.
    c.finger(Some((80, 15)));
    c.run_ms(1000);
    c.finger(Some((140, 50)));
    c.run_ms(800);
    c.finger(None);
    c.run_ms(300);
    m(&mut c, "touch-walk");
    // A long walk around the level, turning at walls.
    for k in 0..6 {
        c.hold(button::UP);
        c.run_ms(900);
        c.hold(if k % 2 == 0 {
            button::LEFT
        } else {
            button::RIGHT
        });
        c.run_ms(450);
        c.hold(0);
        c.tap(button::A);
    }
    m(&mut c, "explore");
    // The game's pause menu: RESTART LEVEL.
    c.tap(button::START);
    c.run_ms(400);
    m(&mut c, "game-pause");
    c.tap(button::DOWN);
    c.tap(button::A);
    c.run_ms(800);
    m(&mut c, "restarted");
    // The console's pause.
    c.pause_ms(3000);
    c.hold(button::DOWN);
    c.run_ms(700);
    c.hold(0);
    // TITLE from the pause menu; LEVEL 3, LIGHT, 30 a second (the
    // record so far ran at the start's HEAVY and 60).
    c.tap(button::START);
    c.tap(button::DOWN);
    c.tap(button::DOWN);
    c.tap(button::A);
    c.run_ms(500);
    m(&mut c, "title-again");
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.tap(button::RIGHT);
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    m(&mut c, "title-options");
    c.tap(button::START);
    c.run_ms(1000);
    m(&mut c, "level-3");
    for k in 0..5 {
        c.hold(button::UP | if k % 2 == 0 { button::RIGHT } else { 0 });
        c.run_ms(700);
        c.hold(button::L);
        c.run_ms(300);
    }
    c.hold(0);
    c.tap(button::Y);
    c.run_ms(500);
    m(&mut c, "level-3-walk");
    // TITLE again, then LEAVE.
    c.tap(button::START);
    c.tap(button::DOWN);
    c.tap(button::DOWN);
    c.tap(button::A);
    c.run_ms(300);
    c.tap(button::UP);
    c.tap(button::A);
    c.run_ms(300);
    assert!(!c.running(), "LEAVE ends LANTERN");
    c
}

#[test]
fn pictures_and_log_match_the_stored_record() {
    let c = script();
    c.require(60, 20_000);
    c.check_golden(
        GOLDEN,
        "LANTERN golden record (tests/golden.rs). Lines: call, game ms, what.",
    );
}

#[test]
fn two_runs_give_the_same_record() {
    assert_eq!(script().record(), script().record());
}
