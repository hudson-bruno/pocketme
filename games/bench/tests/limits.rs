//! The largest loads and odd times: no overflow, no panic. Tests build
//! with overflow checks on, like the chip build.

mod common;

use bench::app::Mode;
use bench::load::{Kind, TESTS, Work};
use bench::ramp::{Meter, Ramp, load_of};
use bench::text::Txt;
use common::Driver;
use game_kit::{Frame, button};

#[test]
fn every_test_at_its_largest_ramp_load_runs() {
    let mut w = Work::new();
    let mut f = Frame::new();
    for t in &TESTS {
        let mut loads = vec![*t.loads.last().unwrap()];
        if t.kmax > 0 {
            loads.push(load_of(t.base, t.kmax));
        }
        for load in loads {
            if matches!(t.kind, Kind::Tri(_) | Kind::Vert | Kind::Fill) && load > 20_000 {
                // A few calls only: they are slow in a debug build.
                for tick in [0, 1, u32::MAX] {
                    w.run(&mut f, t.kind, load, tick, tick == 0);
                }
                continue;
            }
            for tick in (0..300).chain(u32::MAX - 5..=u32::MAX) {
                let c = w.run(&mut f, t.kind, load, tick, tick == 0);
                assert_eq!(c.dropped, 0, "{} load {load}", t.name);
            }
        }
    }
}

#[test]
fn ramp_loads_stay_in_range() {
    for t in &TESTS {
        if t.kmax == 0 {
            continue;
        }
        let top = load_of(t.base, t.kmax);
        assert!(top >= *t.loads.last().unwrap() / 2, "{}", t.name);
        if let Kind::Scene(_) = t.kind {
            assert!(top <= 512, "{}: {top} objects, 512 kept", t.name);
        }
    }
    assert_eq!(
        load_of(u32::MAX, 255),
        u32::MAX / 64,
        "saturates, no overflow"
    );
    assert_eq!(load_of(0, 44), 0);
}

#[test]
fn the_meter_takes_any_time() {
    let mut m = Meter::default();
    for _ in 0..100_000 {
        m.add(u32::MAX, 16_667, u32::MAX);
    }
    assert_eq!(m.worst, u32::MAX);
    let _ = (m.avg(), m.extra_avg(), m.fits(16_667));
    let mut r = Ramp::new(1, 44);
    for _ in 0..1_000 {
        let _ = r.frame(u32::MAX, u32::MAX, true);
        let _ = r.frame(0, 0, true);
    }
    assert!(r.done, "steps of any length end");
}

#[test]
fn every_mode_takes_odd_times_and_keys() {
    for (i, t) in TESTS.iter().enumerate() {
        let mut d = Driver::new();
        d.pick(i, t.loads.len().min(3) - 1);
        for delta in [0, 1, u32::MAX, u32::MAX - 1, 16_667] {
            d.call_full(0, delta, false);
            d.call_full(0, delta, true);
        }
        if t.kmax > 0 {
            d.call(button::A);
            assert_eq!(d.app.mode, Mode::Ramp);
            let mut n = 0;
            while d.app.mode == Mode::Ramp && n < 2_000 {
                // Very slow calls: every step is over.
                d.call_full(0, u32::MAX, false);
                n += 1;
            }
            assert_eq!(d.app.mode, Mode::Result, "{}", t.name);
            d.call_full(0, u32::MAX, false);
            d.call_full(0, u32::MAX, true);
            d.call(button::B);
        }
        // Every key at once.
        d.call(!button::START);
        d.call(0);
    }
}

#[test]
fn the_ticks_wrap() {
    let mut d = Driver::new();
    d.pick(5, 2);
    d.app.tick = u32::MAX - 3;
    d.idle(8);
    assert_eq!(d.app.tick, 4);
}

#[test]
fn texts_take_the_largest_numbers_and_stop_at_their_end() {
    let mut t = Txt::new();
    t.num(u32::MAX)
        .s(" ")
        .int(i32::MIN)
        .s(" ")
        .hundredths(u32::MAX);
    assert_eq!(t.as_str(), "4294967295 -2147483648 42949672.95");
    for _ in 0..100 {
        t.num(u32::MAX);
    }
    assert_eq!(t.bytes().len(), bench::text::CAP);
}

#[test]
fn the_longest_result_line_fits_the_log() {
    let mut d = Driver::new();
    d.pick(5, 0);
    d.call(button::A);
    for r in d.app.ramp.results.iter_mut() {
        r.fit = u32::MAX;
        r.fit_extra = u32::MAX;
        r.safe = Some(u32::MAX);
        r.unit_ns = Some(u32::MAX);
        r.fixed_us = Some(i32::MIN);
    }
    d.app.ramp.done = true;
    d.call_full(0, u32::MAX, false);
    assert_eq!(d.app.mode, Mode::Result);
    let line = d.logs.last().unwrap();
    assert!(line.starts_with("bench: result SCENE 12; 60: fit 4294967295"));
    assert!(line.ends_with("fixed -2147483648"), "{line}");
    assert!(line.len() < 255);
    // And the result screen draws them.
    d.idle(2);
    common::check_palette(&d.frame, "result");
}
