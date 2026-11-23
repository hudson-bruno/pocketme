//! The golden record: ROOFTOP driven through its `game_entry` by the
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
    c.mark("rooftop", name);
}

fn script() -> TestConsole {
    // SAFETY: the game's own entry function, made by `game_kit::game!`.
    let mut c = unsafe { TestConsole::new(rooftop::game_entry) };
    // A room's solution script (tests/data/roomN.txt: "N KEYS", one
    // call each at 30 calls a second), played through the console at
    // the game's starting rate, 60 a second: two calls for each.
    fn play(c: &mut TestConsole, text: &str) {
        for line in text.lines() {
            let line = line.split('#').next().unwrap().trim();
            if line.is_empty() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let calls: u32 = parts.next().unwrap().parse().unwrap();
            let held = parts.next().unwrap_or("-").chars().fold(0, |a, k| {
                a | match k {
                    'L' => button::LEFT,
                    'R' => button::RIGHT,
                    'U' => button::UP,
                    'D' => button::DOWN,
                    'J' => button::A,
                    'X' => button::X,
                    'S' => button::START,
                    'E' => button::SELECT,
                    'B' => button::B,
                    _ => 0,
                }
            });
            c.hold(held);
            for _ in 0..calls * 2 {
                c.step();
            }
        }
        c.hold(0);
    }
    c.run_ms(1000);
    m(&mut c, "title");
    // ROOM 2 then back to 1, SCROLL changed twice: the menu's values.
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    m(&mut c, "title-room-2");
    c.tap(button::LEFT);
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.tap(button::RIGHT);
    c.tap(button::UP);
    c.tap(button::UP);
    c.tap(button::A);
    m(&mut c, "room-1");
    play(&mut c, include_str!("data/room1.txt"));
    m(&mut c, "room-1-done");
    play(&mut c, include_str!("data/room2.txt"));
    m(&mut c, "room-2-done");
    play(&mut c, include_str!("data/room3.txt"));
    m(&mut c, "room-3-done");
    // Room 4: run into the gap (a fall), then restart with START.
    c.hold(button::RIGHT);
    c.run_ms(2500);
    m(&mut c, "room-4-run");
    c.hold(0);
    c.tap(button::START);
    c.pause_ms(2000);
    c.run_ms(500);
    play(&mut c, include_str!("data/room4.txt"));
    m(&mut c, "room-4-done");
    // SELECT: back to the title.
    c.tap(button::SELECT);
    c.run_ms(500);
    m(&mut c, "title-again");
    // The wide room 7 with the smooth view, then room 8 to the end.
    c.tap(button::DOWN);
    for _ in 0..6 {
        c.tap(button::RIGHT);
    }
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    m(&mut c, "title-room-7-smooth");
    c.tap(button::UP);
    c.tap(button::UP);
    c.tap(button::START);
    play(&mut c, include_str!("data/room7.txt"));
    m(&mut c, "room-7-done");
    play(&mut c, include_str!("data/room8.txt"));
    c.run_ms(1500);
    m(&mut c, "end-screen");
    c.tap(button::A);
    c.run_ms(500);
    m(&mut c, "title-last");
    // A touch on the LEAVE line would leave; B leaves.
    c.tap(button::B);
    assert!(!c.running(), "B on the title leaves ROOFTOP");
    c
}

#[test]
fn pictures_and_log_match_the_stored_record() {
    let c = script();
    c.require(30, 20_000);
    c.check_golden(
        GOLDEN,
        "ROOFTOP golden record (tests/golden.rs). Lines: call, game ms, what.",
    );
}

#[test]
fn two_runs_give_the_same_record() {
    assert_eq!(script().record(), script().record());
}
