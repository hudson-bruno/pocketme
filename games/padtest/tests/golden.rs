//! The golden record: PADTEST driven through its `game_entry` by the
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
    let mut c = unsafe { TestConsole::new(padtest::game_entry) };
    let m = |c: &mut TestConsole, name: &str| c.mark("padtest", name);
    c.run_ms(500);
    m(&mut c, "start");
    // Every button alone, then a few together.
    for b in 0..12 {
        c.hold(1 << b);
        c.run_ms(300);
        if b % 4 == 3 {
            m(&mut c, "button");
        }
    }
    c.hold(button::A | button::L | button::UP | button::START);
    c.run_ms(500);
    m(&mut c, "buttons");
    c.hold(0);
    // Sticks and triggers, in steps.
    for k in -4i16..=4 {
        let v = k * 8_000;
        c.left_stick(0, v, -v / 2);
        c.right_stick(0, -v, v);
        c.triggers(0, (k + 4) as u16 * 4_000, (4 - k) as u16 * 4_000);
        c.run_ms(200);
    }
    m(&mut c, "sticks");
    c.left_stick(0, 0, 0);
    c.right_stick(0, 0, 0);
    c.triggers(0, 0, 0);
    // The touch: a finger moving over the screen, then a tap at a corner.
    for k in 0..8i16 {
        c.finger(Some((10 + k * 18, 15 + k * 12)));
        c.run_ms(150);
    }
    m(&mut c, "finger");
    c.finger(None);
    c.touch(155, 2);
    // Motion: level, tilted, then gone.
    c.motion(0, Some(([0, 0, 4096], [0, 0, 0])));
    c.run_ms(600);
    c.motion(0, Some(([2048, -1500, 3500], [160, -320, 48])));
    c.run_ms(600);
    m(&mut c, "motion");
    c.motion(0, None);
    c.run_ms(300);
    // Other players: pads come and go, player 3 holds buttons.
    c.set_pad(2, true);
    c.hold_player(2, button::B | button::RIGHT);
    c.run_ms(500);
    c.set_pad(3, true);
    c.run_ms(500);
    m(&mut c, "players");
    c.set_pad(2, false);
    c.set_pad(3, false);
    c.set_pad(0, false);
    c.run_ms(800);
    m(&mut c, "no-pad");
    c.set_pad(0, true);
    // Still: it asks for the on-demand rate and sleeps; motion alone is
    // seen only by its timer; a button wakes it.
    c.run_ms(3000);
    m(&mut c, "sleeping");
    c.motion(0, Some(([-3000, 0, 2800], [0, 0, 0])));
    c.run_ms(1500);
    m(&mut c, "motion-while-asleep");
    c.motion(0, None);
    c.run_ms(1500);
    c.tap(button::X);
    c.run_ms(400);
    m(&mut c, "woken");
    c.pause_ms(4000);
    c.run_ms(1000);
    c.hold(button::R | button::DOWN);
    c.run_ms(1000);
    c.hold(0);
    c.run_ms(3500);
    m(&mut c, "asleep-again");
    c.quit();
    c
}

#[test]
fn pictures_and_log_match_the_stored_record() {
    let c = script();
    c.require(30, 20_000);
    c.check_golden(
        GOLDEN,
        "PADTEST golden record (tests/golden.rs). Lines: call, game ms, what.",
    );
}

#[test]
fn two_runs_give_the_same_record() {
    assert_eq!(script().record(), script().record());
}
