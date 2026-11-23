//! The whole app driven through the send model of `common/pace.rs`
//! (ESTIMATED: a model, not the board): the times between calls are the
//! ones the board would give for the pictures the app draws. Written so it
//! builds against the BENCH before the ramp-60 change too (a copy kept
//! outside the repository): the same file run there shows the
//! old ramp's numbers. Run with `--nocapture` for the lines;
//! `PACE_OUT=<file>` also writes them to a file.

mod common;
#[path = "common/pace.rs"]
mod pace;

use bench::load::{Kind, TESTS};
use bench::ramp::period;
use common::Driver;
use game_kit::button;
use pace::{Cost, Pace, box_of, city_cost, city_tris};

fn city() -> usize {
    TESTS.iter().position(|t| t.kind == Kind::City).unwrap()
}

/// One call through the model; returns the call's `delta_us`.
fn step(d: &mut Driver, p: &mut Pace, cost: &Cost) -> u32 {
    let (start, delta) = p.begin(period(d.rate60));
    let shown = Box::new(d.frame.as_ref().clone());
    // The load this call draws (the ramp's may move on in the call).
    let load = if d.app.mode == bench::app::Mode::Ramp {
        d.app.ramp.load()
    } else {
        d.app.load()
    };
    d.call_full(0, delta, false);
    let draw = cost.us(load, d.app.last.tris);
    let mut areas = console_video::Areas::new();
    let b = box_of(&shown, &d.frame, &mut areas);
    p.end(start, draw, b, true);
    delta
}

fn out(lines: &[String]) {
    for l in lines {
        println!("{l}");
    }
    if let Ok(path) = std::env::var("PACE_OUT") {
        let mut s = String::new();
        if let Ok(old) = std::fs::read_to_string(&path) {
            s = old;
        }
        for l in lines {
            s.push_str(l);
            s.push('\n');
        }
        std::fs::write(path, s).unwrap();
    }
}

#[test]
fn model_fixed_city() {
    let tris = city_tris();
    let cost = Cost::city(&tris);
    let mut lines = vec![format!(
        "model: CITY draw time {:.0} us + {:.0} us a building + {:.2} us a triangle (fitted to the board's CITY at 30)",
        cost.fixed_us, cost.load_us, cost.tri_us
    )];
    for (load, board, model) in cost.residuals(&tris) {
        lines.push(format!(
            "model: fit at load {load}: board step {board:.0} us, model {model:.0} us ({:+.0})",
            model - board
        ));
    }
    for (lv, &load) in TESTS[city()].loads.iter().enumerate() {
        for rate60 in [true, false] {
            let mut d = Driver::new();
            d.pick(city(), lv);
            if rate60 {
                d.call(button::X);
            }
            d.idle(30);
            let mut p = Pace::new();
            let calls = if rate60 { 1560 } else { 780 };
            let mut sum = 0u64;
            for i in 0..calls {
                let delta = step(&mut d, &mut p, &cost);
                if i >= calls / 10 {
                    sum += u64::from(delta);
                }
            }
            let n = calls - calls / 10;
            let avg = sum as f64 / f64::from(n);
            lines.push(format!(
                "model: fixed CITY {load} at {}: {:.0} us a call ({:.2} a second), full-width boxes {} of {}",
                if rate60 { 60 } else { 30 },
                avg,
                1e6 / avg,
                p.full,
                p.sent
            ));
            // The board (MEASURED, board3): CITY 8 and 32 at 60 held
            // 59.99 and 60.01 a second; the model must agree.
            if rate60 && load <= 32 {
                assert!(1e6 / avg > 59.5, "{load}: {avg}");
            }
        }
    }
    out(&lines);
}

#[test]
fn model_city_ramp() {
    let cost = city_cost();
    let mut d = Driver::new();
    d.pick(city(), 1);
    d.call(button::A);
    let mut p = Pace::new();
    let from = d.logs.len();
    let mut n = 0;
    let mut us = 0u64;
    while d.app.mode == bench::app::Mode::Ramp && n < 400_000 {
        us += u64::from(step(&mut d, &mut p, &cost));
        n += 1;
    }
    assert_eq!(d.app.mode, bench::app::Mode::Result);
    let mut lines = vec![format!(
        "model: CITY ramp, {n} calls, {:.1} s",
        us as f64 / 1e6
    )];
    lines.extend(
        d.logs[from..]
            .iter()
            .filter(|l| l.starts_with("bench: "))
            .cloned(),
    );
    out(&lines);
}
