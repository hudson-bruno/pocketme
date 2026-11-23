//! The whole game through game-kit's stand-in console: start, the
//! controls (pad buttons, triggers, stick), the logs, and runs
//! that repeat, the START menu and the line of numbers.

use drive3d::draw::{LINE_Y, col};
use game_kit::button;
use game_kit::testing::TestConsole;

fn console() -> TestConsole {
    // SAFETY: `game_entry` is made by `game_kit::game!`.
    unsafe { TestConsole::new(drive3d::game_entry) }
}

fn logs(c: &TestConsole) -> Vec<String> {
    c.log_lines().iter().map(|l| l.to_string()).collect()
}

fn pixels(c: &TestConsole) -> Vec<u8> {
    c.picture().0.to_vec()
}

#[test]
fn the_game_starts_draws_and_logs() {
    let mut c = console();
    c.run_ms(500);
    assert!(c.running());
    let l = logs(&c);
    assert_eq!(l.len(), 1);
    assert!(l[0].contains("drive3d: started"));
    assert!(c.pictures() >= 1);
    // The car, seen from behind, covers the picture's (80, 70): a lit
    // model colour, not land or sky.
    let p = c.picture();
    for (x, y) in [(80, 66), (80, 70), (76, 72), (84, 72)] {
        assert!(p.get(x, y) < col::LAND, "({x}, {y}) is {}", p.get(x, y));
    }
    // The speed's text in the bottom left ("0 KMH") and the level's
    // name in the top right ("HIGH", the start).
    let p = pixels(&c);
    assert!(p[105 * 160..118 * 160].contains(&col::TEXT));
    assert!((1..8).any(|y| p[y * 160 + 130..y * 160 + 160].contains(&col::TEXT)));
}

/// The picture after `ms` of holding `buttons`, from the start.
fn after_hold(buttons: u32, ms: u64) -> Vec<u8> {
    let mut c = console();
    c.run_ms(100);
    c.hold(buttons);
    c.run_ms(ms);
    pixels(&c)
}

#[test]
fn a_and_the_right_trigger_drive_and_the_same_inputs_give_the_same_picture() {
    let still = after_hold(0, 2_000);
    let a = after_hold(button::A, 2_000);
    assert_ne!(still, a, "A held drives");
    assert_eq!(a, after_hold(button::A, 2_000), "the same run twice");
    // The right trigger fully down is the same throttle as A.
    let mut c = console();
    c.run_ms(100);
    c.triggers(0, 0, 32_767);
    c.run_ms(2_000);
    assert_eq!(pixels(&c), a);
}

#[test]
fn the_stick_and_the_d_pad_steer_alike_at_full_lock() {
    let pad = after_hold(button::A | button::RIGHT, 2_000);
    let mut c = console();
    c.run_ms(100);
    c.hold(button::A);
    c.left_stick(0, 32_767, 0);
    c.run_ms(2_000);
    assert_eq!(pixels(&c), pad);
    assert_ne!(pad, after_hold(button::A, 2_000));
}

#[test]
fn a_finger_does_nothing() {
    // Gamepad only (owner's rule, 2026-11-22): a finger anywhere is the
    // same as nothing pressed, and A held with a finger on the brake
    // rows is the same as A held.
    let none = after_hold(0, 2_000);
    for at in [(80, 40), (80, 110), (5, 60)] {
        let mut c = console();
        c.run_ms(100);
        c.finger(Some(at));
        c.run_ms(2_000);
        assert_eq!(pixels(&c), none, "finger at {at:?}");
    }
    let a = after_hold(button::A, 2_000);
    let mut c = console();
    c.run_ms(100);
    c.hold(button::A);
    c.finger(Some((80, 110)));
    c.run_ms(2_000);
    assert_eq!(pixels(&c), a);
}

#[test]
fn x_changes_the_level_and_y_puts_back() {
    let mut c = console();
    c.run_ms(100);
    // From HIGH, the start, round past MAX and LOW back to HIGH.
    c.tap(button::X);
    assert!(logs(&c).last().unwrap().contains("drive3d: detail MAX"));
    c.tap(button::X);
    assert!(logs(&c).last().unwrap().contains("drive3d: detail LOW"));
    c.tap(button::X);
    assert!(logs(&c).last().unwrap().contains("drive3d: detail MID"));
    c.tap(button::X);
    assert!(logs(&c).last().unwrap().contains("drive3d: detail HIGH"));
    c.tap(button::Y);
    c.run_ms(1_600);
    assert!(
        logs(&c)
            .last()
            .unwrap()
            .contains("drive3d: back on the road at point 0")
    );
}

#[test]
fn start_opens_the_menu_which_holds_the_run_still() {
    let mut c = console();
    c.run_ms(100);
    c.hold(button::A);
    c.run_ms(1_000);
    c.hold(0);
    c.tap(button::START);
    let paused = pixels(&c);
    // The menu's box, in the middle of the picture.
    assert_eq!(paused[78 * 160 + 50], col::BOX);
    // Nothing moves while it is open, even with A held (A takes a line
    // only when newly pressed).
    c.run_ms(1_000);
    assert_eq!(pixels(&c), paused);
    // B closes it and the run goes on from where it stood.
    c.tap(button::B);
    assert_ne!(pixels(&c)[78 * 160 + 50], col::BOX);
    assert!(logs(&c).iter().all(|l| !l.contains("from the start")));
}

#[test]
fn the_menu_picks_a_level_and_restarts() {
    let mut c = console();
    c.run_ms(100);
    // It opens on HIGH (the level in use, the start); UP then A takes
    // MID.
    c.tap(button::START);
    c.tap(button::UP);
    c.tap(button::A);
    assert!(logs(&c).last().unwrap().contains("drive3d: detail MID"));
    // It opens on MID now; DOWN then A takes HIGH.
    c.tap(button::START);
    c.tap(button::DOWN);
    c.tap(button::A);
    assert!(logs(&c).last().unwrap().contains("drive3d: detail HIGH"));
    // It opens on HIGH now; DOWN twice (past MAX) then A restarts.
    c.hold(button::A);
    c.run_ms(1_000);
    c.hold(0);
    c.tap(button::START);
    c.tap(button::DOWN);
    c.tap(button::DOWN);
    c.tap(button::A);
    assert!(logs(&c).last().unwrap().contains("drive3d: from the start"));
    let n = logs(&c).len();
    // UP three times from HIGH goes round past the top to RESTART, and
    // START takes it too.
    c.tap(button::START);
    c.tap(button::UP);
    c.tap(button::UP);
    c.tap(button::UP);
    c.tap(button::START);
    assert_eq!(logs(&c).len(), n + 1);
    assert!(logs(&c).last().unwrap().contains("drive3d: from the start"));
    // A restart keeps the level (HIGH: X goes on to MAX).
    c.tap(button::START);
    c.tap(button::B);
    c.tap(button::X);
    assert!(logs(&c).last().unwrap().contains("drive3d: detail MAX"));
    // The menu's MAX line: it opens on MAX, DOWN then A restarts; UP
    // from MAX then A takes HIGH.
    c.tap(button::START);
    c.tap(button::UP);
    c.tap(button::A);
    assert!(logs(&c).last().unwrap().contains("drive3d: detail HIGH"));
    c.tap(button::START);
    c.tap(button::DOWN);
    c.tap(button::A);
    assert!(logs(&c).last().unwrap().contains("drive3d: detail MAX"));
}

#[test]
fn select_shows_the_line_and_changes_nothing_else() {
    let mut c = console();
    c.run_ms(500);
    let without = pixels(&c);
    c.tap(button::SELECT);
    let with = pixels(&c);
    assert_ne!(with, without);
    // Only the line's rows (its text and shadow) differ.
    let rows = LINE_Y as usize..LINE_Y as usize + 8;
    for (i, (a, b)) in with.iter().zip(&without).enumerate() {
        if a != b {
            let y = i / 160;
            assert!(rows.contains(&y), "row {y}");
        }
    }
    c.tap(button::SELECT);
    assert_eq!(pixels(&c), without);
}
