//! One picture per test (at its start load), a ramp under way and a
//! result screen, four times as large, into `.local/bench/shots/` (next
//! to this crate). Only the game's own colours are used.

mod common;

use bench::app::Mode;
use bench::load::TESTS;
use common::{CostModel, Driver, check_palette, save_frame, shots};
use game_kit::button;

fn file_name(i: usize, name: &str) -> String {
    format!(
        "{:02}-{}.png",
        i + 1,
        name.to_ascii_lowercase().replace(' ', "-")
    )
}

#[test]
fn a_picture_of_every_test() {
    let dir = shots();
    for (i, t) in TESTS.iter().enumerate() {
        let mut d = Driver::new();
        d.pick(i, 1);
        d.idle(40);
        check_palette(&d.frame, t.name);
        save_frame(&dir.join(file_name(i, t.name)), &d.frame, 4);
    }
}

#[test]
fn a_picture_of_a_ramp_and_its_result() {
    let dir = shots();
    // SCENE 32 against a made-up console: 2 ms a loop and 0.4 ms an
    // object.
    let m = CostModel {
        fixed_us: 2_000,
        unit_ns: 400_000,
    };
    let mut d = Driver::new();
    d.pick(6, 1);
    d.call(button::A);
    let mut before = d.app.ramp.load();
    let mut saved = false;
    let mut calls = 0;
    while d.app.mode == Mode::Ramp && calls < 50_000 {
        let load = d.app.ramp.load();
        let delta = m.delta(before, bench::ramp::period(d.rate60));
        d.call_full(0, delta, false);
        before = load;
        calls += 1;
        if !saved && d.app.last_step.is_some_and(|s| s.k >= 8) {
            check_palette(&d.frame, "ramp");
            save_frame(&dir.join("11-ramp.png"), &d.frame, 4);
            saved = true;
        }
    }
    assert!(saved);
    d.idle(2);
    assert_eq!(d.app.mode, Mode::Result);
    check_palette(&d.frame, "result");
    save_frame(&dir.join("12-result.png"), &d.frame, 4);
    // The fixed screen at 60.
    let mut d = Driver::new();
    d.pick(7, 1);
    d.call(button::X);
    d.idle(80);
    save_frame(&dir.join("13-scene-96-at-60.png"), &d.frame, 4);
    // Added by the check: a result whose top step fit at both rates.
    let m = CostModel {
        fixed_us: 100,
        unit_ns: 1,
    };
    let mut d = Driver::new();
    d.pick(6, 1);
    d.call(button::A);
    let mut before = d.app.ramp.load();
    let mut calls = 0;
    while d.app.mode == Mode::Ramp && calls < 50_000 {
        let load = d.app.ramp.load();
        d.call_full(0, m.delta(before, bench::ramp::period(d.rate60)), false);
        before = load;
        calls += 1;
    }
    d.idle(2);
    assert!(d.app.ramp.results.iter().all(|r| r.capped));
    check_palette(&d.frame, "capped result");
    save_frame(&dir.join("14-result-capped.png"), &d.frame, 4);
}
