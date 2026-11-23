//! The A (and B) that takes a START menu line counts for nothing until
//! it is let go once: held on, it neither drives the car nor starts the
//! run clock. Without that, choosing a level or RESTART with A and
//! keeping the thumb on it set the car off at once.

use game_kit::button;
use game_kit::testing::TestConsole;

fn console() -> TestConsole {
    // SAFETY: `game_entry` is made by `game_kit::game!`.
    unsafe { TestConsole::new(drive3d::game_entry) }
}

fn pixels(c: &TestConsole) -> Vec<u8> {
    c.picture().0.to_vec()
}

/// From the start: open the menu, move `down` lines, take the line with
/// `take` and keep holding `keep` for `ms`.
fn take_and_hold(down: u32, take: u32, keep: u32, ms: u64) -> TestConsole {
    let mut c = console();
    c.run_ms(100);
    c.tap(button::START);
    for _ in 0..down {
        c.tap(button::DOWN);
    }
    c.hold(take);
    c.run_ms(100);
    c.hold(keep);
    c.run_ms(ms);
    c
}

#[test]
fn a_held_from_a_level_choice_neither_drives_nor_starts_the_clock() {
    // The menu opens on HIGH (the start); one line down is MAX.
    let held = take_and_hold(1, button::A, button::A, 2_000);
    let let_go = take_and_hold(1, button::A, 0, 2_000);
    assert_eq!(pixels(&held), pixels(&let_go), "A held on after the choice");
    // Let go once and press again: now it drives.
    let mut c = held;
    c.hold(0);
    c.run_ms(100);
    c.hold(button::A);
    c.run_ms(2_000);
    assert_ne!(pixels(&c), pixels(&let_go), "a new A press drives");
}

#[test]
fn a_and_b_held_from_restart_count_for_nothing() {
    // HIGH, MAX, RESTART: two lines down.
    let both = button::A | button::B;
    let let_go = take_and_hold(2, both, 0, 2_000);
    // Each of the two held on alone: without the rule, A alone drives
    // the car off and B alone backs it up, so both pictures would
    // differ from the let-go one. (A and B held on together would not
    // show it: the car stands still with both held anyway.)
    for (keep, what) in [(button::A, "A"), (button::B, "B")] {
        let held = take_and_hold(2, both, keep, 2_000);
        assert_eq!(
            pixels(&held),
            pixels(&let_go),
            "{what} held on after RESTART"
        );
    }
    let held = take_and_hold(2, both, both, 2_000);
    assert_eq!(
        pixels(&held),
        pixels(&let_go),
        "A and B held on after RESTART"
    );
    // B let go and pressed again (A still held, still ignored): B
    // counts again, A does not until it is let go too.
    let mut c = take_and_hold(2, both, button::A, 1_000);
    c.hold(button::A | button::B);
    c.run_ms(1_000);
    let mut d = take_and_hold(2, both, 0, 1_000);
    d.hold(button::B);
    d.run_ms(1_000);
    assert_eq!(pixels(&c), pixels(&d), "A still ignored, B counts again");
}

#[test]
fn a_held_without_a_menu_choice_still_drives() {
    // No menu taken: A held drives as before (the ignore is only for a
    // menu choice).
    let mut c = console();
    c.run_ms(100);
    let still = pixels(&c);
    c.hold(button::A);
    c.run_ms(2_000);
    assert_ne!(pixels(&c), still);
}
