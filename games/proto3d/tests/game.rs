//! The whole game through game-kit's stand-in console: the fixed step,
//! the pause menu, and runs that repeat.

use game_kit::button;
use game_kit::testing::TestConsole;
use proto3d::step::Clock;
use proto3d::tune::{MAX_STEPS, STEP_US};

fn console() -> TestConsole {
    // SAFETY: `game_entry` is made by `game_kit::game!`.
    let mut c = unsafe { TestConsole::new(proto3d::game_entry) };
    // The area menu opens at the start: A takes ISLAND.
    c.tap(button::A);
    c
}

/// The last log line (the record shows it quoted).
fn last_log(c: &TestConsole) -> String {
    c.log_lines()
        .last()
        .map_or(String::new(), |l| l.to_string())
}

#[test]
fn the_clock_runs_one_step_per_33333_us_and_at_most_two_per_update() {
    // The store starts half a step full (so a little jitter round 33,333
    // us still gives one step a call).
    let mut c = Clock::new();
    assert_eq!(c.store, STEP_US / 2);
    assert_eq!(c.add(0), 0);
    assert_eq!(c.add(STEP_US), 1);
    assert_eq!(c.add(STEP_US / 2), 0);
    assert_eq!(c.add(1), 1);
    assert_eq!(c.store, 0);
    // 60 calls a second: a step every other call.
    let n: u32 = (0..60).map(|_| u32::from(c.add(16_667))).sum();
    assert_eq!(n, 30);
    // A long stall: two steps, and at most two steps' time kept.
    let mut c = Clock::new();
    assert_eq!(c.add(1_000_000), MAX_STEPS);
    assert_eq!(c.store, 2 * STEP_US);
    assert_eq!(c.add(0), 2);
    assert_eq!(c.add(0), 0);
}

#[test]
fn the_game_starts_and_draws() {
    let mut c = console();
    c.run_ms(1_000);
    assert!(c.running());
    let logs = c.log_lines();
    assert_eq!(logs.len(), 1);
    assert!(logs[0].contains("proto3d: started"));
    assert!(c.pictures() >= 1);
}

#[test]
fn start_opens_the_detail_menu_and_it_changes_the_setting() {
    let mut c = console();
    c.run_ms(200);
    // It opens on HIGH (the start); DOWN and A: MAX.
    c.tap(button::START);
    c.tap(button::DOWN);
    c.tap(button::A);
    assert!(last_log(&c).contains("proto3d: detail MAX"));
    // UP twice and B: closed, no change.
    c.tap(button::START);
    c.tap(button::UP);
    c.tap(button::UP);
    c.tap(button::B);
    assert!(last_log(&c).contains("proto3d: detail MAX"));
    // UP three times and START: LOW.
    c.tap(button::START);
    c.tap(button::UP);
    c.tap(button::UP);
    c.tap(button::UP);
    c.tap(button::START);
    assert!(last_log(&c).contains("proto3d: detail LOW"));
}

#[test]
fn nothing_moves_while_the_menu_is_open() {
    let mut c = console();
    c.run_ms(300);
    c.tap(button::START);
    let before = c.png();
    // The stick sideways and X (neither moves the menu's choice).
    c.left_stick(0, 32_767, 0);
    c.hold(button::X);
    c.run_ms(1_000);
    assert_eq!(c.png(), before);
    c.hold(0);
    c.left_stick(0, 0, 0);
    c.tap(button::B);
    c.left_stick(0, 32_767, 0);
    c.run_ms(500);
    assert_ne!(c.png(), before);
}

/// A fixed script: run, jump, turn the camera, the menu, SELECT.
fn script(c: &mut TestConsole) {
    c.run_ms(300);
    c.left_stick(0, 0, -32_767);
    c.run_ms(700);
    c.hold(button::A);
    c.run_ms(400);
    c.hold(0);
    c.right_stick(0, 20_000, 0);
    c.left_stick(0, 20_000, -20_000);
    c.run_ms(1_000);
    c.right_stick(0, 0, 0);
    c.tap(button::SELECT);
    c.tap(button::L);
    c.tap(button::START);
    c.tap(button::DOWN);
    c.tap(button::A);
    c.left_stick(0, -32_767, 0);
    c.run_ms(2_000);
}

#[test]
fn the_same_script_gives_the_same_record() {
    let mut a = console();
    script(&mut a);
    let ra = a.record().to_vec();
    drop(a);
    let mut b = console();
    script(&mut b);
    assert_eq!(ra, b.record());
    assert!(b.pictures() > 20);
}

#[test]
fn two_minutes_of_random_input_do_not_panic() {
    let mut c = console();
    let mut r: u64 = 99;
    let mut next = || {
        r = r
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (r >> 33) as u32
    };
    for _ in 0..600 {
        let v = next();
        let s = |v: u32| ((v % 65_535) as i32 - 32_767) as i16;
        c.left_stick(0, s(next()), s(next()));
        c.right_stick(0, if v.is_multiple_of(3) { s(next()) } else { 0 }, 0);
        let all = [
            button::A,
            button::B,
            button::L,
            button::R,
            button::UP,
            button::DOWN,
            button::START,
            button::SELECT,
        ];
        let held = all
            .iter()
            .fold(0, |h, &b| if next().is_multiple_of(6) { h | b } else { h });
        c.hold(held);
        c.run_ms(200);
    }
    assert!(c.running());
}
