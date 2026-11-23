//! The ramp fed with made-up times: a straight-line cost model
//! (`fixed + load * unit`) stands in for the console. The ramp must find
//! the step a search over every step finds, and the cost line.

mod common;

use bench::app::Mode;
use bench::ramp::{
    MIN_FRAMES, Meter, PERIOD_30, PERIOD_60, Ramp, RateResult, STEP_US, Step, WARM, late_limit,
    load_of, period,
};
use common::{CostModel, Driver};
use game_kit::button;

/// Runs a ramp against the model; returns it and its steps.
fn run(base: u32, kmax: u8, m: CostModel) -> (Ramp, Vec<Step>) {
    let mut r = Ramp::new(base, kmax);
    let mut steps = Vec::new();
    // The time between calls shows the work of the call before.
    let mut before = r.load();
    for _ in 0..1_000_000 {
        if r.done {
            break;
        }
        let d = m.delta(before, r.period());
        before = r.load();
        if let Some(s) = r.frame(d, 7, true) {
            steps.push(s);
        }
    }
    assert!(r.done, "the ramp ends");
    (r, steps)
}

/// The largest step whose loop fits the frame (by trying every step).
fn best_by_search(base: u32, kmax: u8, m: CostModel, period: u32) -> u32 {
    (0..=kmax)
        .map(|k| load_of(base, k))
        .filter(|&l| m.delta(l, period) <= period + period / 64)
        .max()
        .unwrap_or(0)
}

/// What the cost line should give for SAFE.
fn safe_by_line(m: CostModel, period: u32, fit: u32) -> u32 {
    let budget = i64::from(period * 4 / 5) - i64::from(m.fixed_us);
    if budget <= 0 {
        return 0;
    }
    ((budget as u64 * 1000 / u64::from(m.unit_ns)) as u32).min(fit)
}

fn check(base: u32, kmax: u8, m: CostModel) -> [RateResult; 2] {
    let (r, steps) = run(base, kmax, m);
    for (i, p) in [PERIOD_60, PERIOD_30].into_iter().enumerate() {
        let res = r.results[i];
        let want = best_by_search(base, kmax, m, p);
        assert_eq!(res.fit, want, "rate {i}: {m:?}, steps {steps:#?}");
        if let Some(safe) = res.safe {
            let line = safe_by_line(m, p, res.fit);
            let slack = (line / 50).max(1);
            assert!(
                safe.abs_diff(line) <= slack,
                "rate {i}: safe {safe}, the line says {line} ({m:?})"
            );
            let ns = res.unit_ns.unwrap();
            assert!(
                ns.abs_diff(m.unit_ns) <= m.unit_ns / 100 + 2,
                "unit {ns} ns, made-up {}",
                m.unit_ns
            );
            let fx = res.fixed_us.unwrap();
            assert!(
                (fx - m.fixed_us as i32).unsigned_abs() <= m.fixed_us / 50 + 20,
                "fixed {fx}, made-up {}",
                m.fixed_us
            );
        }
        assert!(res.steps <= 24, "rate {i}: {} steps", res.steps);
    }
    r.results
}

#[test]
fn small_triangles_like_costs_find_the_searched_step() {
    let r = check(
        16,
        44,
        CostModel {
            fixed_us: 1_500,
            unit_ns: 3_300,
        },
    );
    assert!(r[0].fit > 3_000 && r[1].fit > 2 * r[0].fit - 1_000, "{r:?}");
    assert!(r[0].safe.is_some() && r[1].safe.is_some());
}

#[test]
fn fill_like_costs_find_the_searched_step() {
    let r = check(
        1,
        44,
        CostModel {
            fixed_us: 800,
            unit_ns: 40_000,
        },
    );
    // 16,000 bytes in 40 µs: 400 bytes a µs.
    assert_eq!(16_000 * 1000 / r[0].unit_ns.unwrap(), 400);
}

#[test]
fn scene_like_costs_find_the_searched_step() {
    for unit in [60_000, 230_000, 700_000, 1_900_000] {
        for fixed in [0, 2_000, 9_000] {
            check(
                1,
                32,
                CostModel {
                    fixed_us: fixed,
                    unit_ns: unit,
                },
            );
        }
    }
}

#[test]
fn many_made_up_costs_find_the_searched_step() {
    let mut seed = 12_345u32;
    let mut next = |n: u32| {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) % n
    };
    for _ in 0..300 {
        let base = 1 + next(32);
        let kmax = 8 + next(37) as u8;
        let m = CostModel {
            fixed_us: next(20_000),
            unit_ns: 50 + next(400_000),
        };
        check(base, kmax, m);
    }
}

#[test]
fn when_nothing_fits_the_result_says_zero() {
    let (r, _) = run(
        1,
        44,
        CostModel {
            fixed_us: 40_000,
            unit_ns: 10_000,
        },
    );
    assert_eq!(r.results[0].fit, 0);
    assert_eq!(r.results[1].fit, 0);
    assert_eq!(r.results[0].safe, Some(0));
}

#[test]
fn when_everything_fits_there_is_no_cost_line() {
    let (r, steps) = run(
        1,
        44,
        CostModel {
            fixed_us: 100,
            unit_ns: 1,
        },
    );
    assert_eq!(r.results[0].fit, load_of(1, 44));
    assert_eq!(r.results[1].fit, load_of(1, 44));
    assert_eq!(r.results[0].safe, None);
    // The top step fit: FIT is only a floor, and says so.
    assert!(r.results[0].capped && r.results[1].capped);
    assert!(steps.iter().all(|s| s.fits));
    // 0, 4, ..., 44 at 60; 44 at 30.
    assert_eq!(steps.len(), 12 + 1);
}

/// Added by the check. Over loops take the same time at both rates, so
/// when 30 reaches its top step with fewer than two over loads of its
/// own, the line from 60's over loads still gives SAFE at 30.
#[test]
fn thirty_reaching_the_top_still_gets_a_cost_line() {
    // 16 * 2^(36/4) = 8192 at the top. At 60, 3,456 fit and 5,824 and
    // 8,192 are over; at 30 everything up to the top fits.
    let m = CostModel {
        fixed_us: 2_000,
        unit_ns: 3_600,
    };
    let (r, _) = run(16, 36, m);
    let [r60, r30] = r.results;
    assert!(!r60.capped && r30.capped, "{r60:?} {r30:?}");
    assert_eq!(r30.fit, load_of(16, 36));
    let safe = r30.safe.expect("a line at 30");
    let line = safe_by_line(m, PERIOD_30, r30.fit);
    assert!(
        safe.abs_diff(line) <= line / 50 + 1,
        "safe {safe}, line {line}"
    );
    assert!(r60.safe.is_some());
}

#[test]
fn thirty_starts_where_sixty_ended() {
    let (_, steps) = run(
        8,
        44,
        CostModel {
            fixed_us: 1_000,
            unit_ns: 5_000,
        },
    );
    let last60 = steps
        .iter()
        .filter(|s| s.rate60 && s.fits)
        .map(|s| s.k)
        .max();
    let first30 = steps.iter().find(|s| !s.rate60).unwrap();
    assert_eq!(Some(first30.k), last60);
}

#[test]
fn each_step_warms_up_then_measures_half_a_second() {
    let mut r = Ramp::new(1, 44);
    for i in 0..WARM {
        assert_eq!(r.frame(PERIOD_60, 0, true), None, "warm call {i}");
        assert_eq!(r.meter.frames, 0);
    }
    let mut n = 0;
    let s = loop {
        n += 1;
        if let Some(s) = r.frame(PERIOD_60, 0, true) {
            break s;
        }
    };
    assert_eq!(n, STEP_US.div_ceil(PERIOD_60));
    assert_eq!(s.frames, n);
    assert!(s.fits);
    assert_eq!(r.k, 4, "the coarse search doubles the load");
    // A slow loop still measures at least MIN_FRAMES calls.
    let mut r = Ramp::new(1, 44);
    let mut calls = 0;
    while r.frame(400_000, 0, true).is_none() {
        calls += 1;
    }
    assert_eq!(calls + 1, WARM + MIN_FRAMES);
}

#[test]
fn a_rate_not_yet_taken_or_a_pause_starts_the_warm_up_again() {
    let mut r = Ramp::new(1, 44);
    for _ in 0..10 {
        r.frame(PERIOD_60, 0, true);
    }
    assert!(r.meter.frames > 0);
    r.frame(PERIOD_60, 0, false);
    assert_eq!(r.meter.frames, 0);
    assert_eq!(r.meter.skip, WARM);
    for _ in 0..10 {
        r.frame(PERIOD_60, 0, true);
    }
    r.interrupt();
    assert_eq!(r.meter.frames, 0);
}

#[test]
fn a_few_late_calls_still_fit_but_one_in_ten_does_not() {
    let p = PERIOD_30;
    let late = late_limit(p) + 1;
    // Late calls come early afterwards: the average stays the frame.
    let pattern = |every: u32| {
        let mut m = Meter::default();
        for i in 0..64 {
            let d = match i % every {
                0 => late,
                1 => 2 * p - late,
                _ => p,
            };
            m.add(d, p, 0);
        }
        m
    };
    assert!(pattern(32).fits(p), "2 late in 64");
    assert!(pattern(16).fits(p), "4 late in 64");
    assert!(!pattern(10).fits(p), "7 late in 64");
    // A steady 2 % over the frame does not fit.
    let mut m = Meter::default();
    for _ in 0..30 {
        m.add(p + p / 50, p, 0);
    }
    assert!(!m.fits(p));
}

#[test]
fn the_steps_grow_by_quarter_doublings_without_repeats_in_the_fine_search() {
    assert_eq!(
        (0..9).map(|k| load_of(100, k)).collect::<Vec<_>>(),
        [100, 118, 142, 168, 200, 237, 284, 337, 400]
    );
    // Small bases repeat loads; the ramp never measures one twice at a
    // rate.
    let (_, steps) = run(
        1,
        44,
        CostModel {
            fixed_us: 2_000,
            unit_ns: 1_000_000,
        },
    );
    for rate60 in [true, false] {
        let mut loads: Vec<u32> = steps
            .iter()
            .filter(|s| s.rate60 == rate60)
            .map(|s| s.load)
            .collect();
        let n = loads.len();
        loads.sort_unstable();
        loads.dedup();
        assert_eq!(loads.len(), n, "{steps:#?}");
    }
}

#[test]
fn the_app_runs_the_ramp_and_shows_the_result() {
    let m = CostModel {
        fixed_us: 800,
        unit_ns: 40_000,
    };
    let mut d = Driver::new();
    d.pick(0, 1);
    d.call(button::A);
    assert_eq!(d.app.mode, Mode::Ramp);
    assert!(d.rate60, "the ramp asks for 60 first");
    let mut before = d.app.ramp.load();
    let mut calls = 0;
    while d.app.mode == Mode::Ramp && calls < 100_000 {
        let load = d.app.ramp.load();
        let delta = m.delta(before, period(d.rate60));
        d.call_full(0, delta, false);
        before = load;
        calls += 1;
    }
    assert_eq!(d.app.mode, Mode::Result);
    let (r, _) = run(1, 44, m);
    assert_eq!(
        d.app.ramp.results.map(|x| (x.fit, x.safe, x.unit_ns)),
        r.results.map(|x| (x.fit, x.safe, x.unit_ns))
    );
    assert!(!d.rate60, "back to the chosen rate");
    let result = d
        .logs
        .iter()
        .find(|l| l.starts_with("bench: result FILL"))
        .unwrap();
    assert!(result.contains("; 60: fit "), "{result}");
    assert!(
        d.logs
            .iter()
            .any(|l| l.starts_with("bench: ramp 60 k 0 load 1: avg "))
    );
    // The result is logged again every 2 s while shown.
    let n = d.logs.len();
    d.idle(61);
    assert_eq!(d.logs.len(), n + 1);
    // B goes back to the fixed level.
    d.call(button::B);
    assert_eq!(d.app.mode, Mode::Fixed);
}

#[test]
fn b_stops_the_ramp() {
    let mut d = Driver::new();
    d.pick(5, 0);
    d.call(button::A);
    d.idle(10);
    d.call(button::B);
    assert_eq!(d.app.mode, Mode::Fixed);
    assert!(d.logs.iter().any(|l| l == "bench: ramp stopped"));
    d.idle(2);
    assert!(!d.rate60);
}

#[test]
fn the_send_tests_have_no_ramp() {
    let mut d = Driver::new();
    d.pick(8, 2);
    d.call(button::A);
    assert_eq!(d.app.mode, Mode::Fixed);
}

/// How long a ramp takes in game time (seconds), for the README.
#[test]
fn a_ramp_takes_well_under_a_minute() {
    let mut longest = 0u64;
    for (base, kmax, m) in [
        (
            1,
            44,
            CostModel {
                fixed_us: 800,
                unit_ns: 40_000,
            },
        ),
        (
            16,
            44,
            CostModel {
                fixed_us: 1_500,
                unit_ns: 3_300,
            },
        ),
        (
            1,
            32,
            CostModel {
                fixed_us: 2_000,
                unit_ns: 400_000,
            },
        ),
        (
            1,
            28,
            CostModel {
                fixed_us: 9_000,
                unit_ns: 1_900_000,
            },
        ),
        (
            1,
            44,
            CostModel {
                fixed_us: 40_000,
                unit_ns: 10_000,
            },
        ),
        (
            1,
            44,
            CostModel {
                fixed_us: 100,
                unit_ns: 1,
            },
        ),
    ] {
        let mut r = Ramp::new(base, kmax);
        let mut before = r.load();
        let mut t = 0u64;
        while !r.done {
            let d = m.delta(before, r.period());
            t += u64::from(d);
            before = r.load();
            r.frame(d, 0, true);
        }
        println!("{m:?}: {:.1} s", t as f64 / 1e6);
        longest = longest.max(t);
    }
    assert!(longest < 60_000_000, "{longest} µs");
}
