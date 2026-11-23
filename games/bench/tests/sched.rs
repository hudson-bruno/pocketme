//! Added by the check: the ramp against the console's real way of
//! calling a game, with work that changes from call to call.
//!
//! `tests/ramp.rs` feeds the ramp a cost that depends on the load only.
//! A scene's cost moves with the picture (how many pixels the near balls
//! cover), and the console keeps a frame grid: a late call is followed by
//! an early one when the next has room (console-core `next_wake`: the
//! next frame is one period after the one that was due, unless that is
//! already past). This model copies that rule. Each call's work is a
//! made-up cost of what the call really drew (triangles, pixels,
//! objects), so the scenes' unsteadiness reaches the ramp as it would on
//! the board.
//!
//! The truth: for each step's load, a long run (three scene cycles) under
//! the same rule, judged by the ramp's own rule. The ramp's FIT must be
//! within one step of it.
//!
//! SAFE is a straight line through two over loads, stretched down to 4/5
//! of a frame. A scene's cost per object is not constant (more objects,
//! more of them small and far), so the line is only a guide: at 30 its
//! points lie close to the answer and the test holds it within 10 %; at
//! 60 it reaches further down and the error is only printed (up to about
//! half in this model when the fixed part is near the whole budget).
//! When the line has fewer than two over loads, SAFE is missing
//! ("SAFE -"), which is allowed (not seen in these models).
//!
//! About 40 s in a release build and several minutes in a debug one, so
//! a debug build skips it: `cargo test --release --test sched`.

mod common;

use bench::app::Mode;
use bench::load::{Kind, TESTS, Work};
use bench::ramp::{Meter, load_of, period};
use common::Driver;
use game_kit::{Frame, button};

#[derive(Clone, Copy, Debug)]
struct Cost {
    fixed_us: u64,
    tri_ns: u64,
    px_ns: u64,
    obj_ns: u64,
}

impl Cost {
    fn work(&self, c: &bench::load::Counts) -> u64 {
        self.fixed_us * 1000
            + u64::from(c.tris) * self.tri_ns
            + u64::from(c.px) * self.px_ns
            + u64::from(c.objs) * self.obj_ns
    }
}

/// The console's grid (console-core `next_wake`, fixed rate): returns the
/// time of the next call.
struct Grid {
    t: u64,
    next: u64,
}

impl Grid {
    fn after(&mut self, work_ns: u64, period: u64) -> u64 {
        let now = self.t;
        if now >= self.next {
            self.next += period;
            if self.next <= now {
                self.next = now + period;
            }
        }
        let t2 = (now + work_ns / 1000).max(self.next);
        let d = t2 - now;
        self.t = t2;
        d
    }
}

/// A long run at one load: the ramp's rule over three cycles.
fn long_run(kind: Kind, load: u32, rate60: bool, cost: Cost) -> (bool, u32) {
    let p = u64::from(period(rate60));
    let mut w = Work::new();
    let mut f = Frame::new();
    let mut g = Grid { t: 0, next: 0 };
    let mut m = Meter::default();
    m.restart(8);
    // The ramp's advance: 2 ticks of 1/60 s a call at both rates.
    for tick in 0..(278 * 3) {
        let c = w.run(&mut f, kind, load, 2 * tick, tick == 0);
        let d = g.after(cost.work(&c), p);
        m.add(d as u32, p as u32, 0);
    }
    (m.fits(p as u32), m.avg())
}

/// The mean work of one call at a load, over a whole cycle (µs).
fn mean_work(kind: Kind, load: u32, cost: Cost) -> u64 {
    let mut w = Work::new();
    let mut f = Frame::new();
    let mut sum = 0u64;
    for tick in 0..278 {
        let c = w.run(&mut f, kind, load, 2 * tick, tick == 0);
        sum += cost.work(&c);
    }
    sum / 278 / 1000
}

/// The load whose mean work is 4/5 of the frame (searched in whole
/// loads between two steps).
fn true_safe(kind: Kind, top: u32, rate60: bool, cost: Cost) -> u32 {
    let budget = u64::from(period(rate60)) * 4 / 5;
    let (mut lo, mut hi) = (0u32, top);
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if mid == 0 || mean_work(kind, mid, cost) <= budget {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

fn ramp(i: usize, cost: Cost) -> ([u32; 2], [Option<u32>; 2]) {
    let mut d = Driver::new();
    d.pick(i, 1);
    d.call(button::A);
    assert_eq!(d.app.mode, Mode::Ramp);
    let mut g = Grid { t: 0, next: 0 };
    let mut work = 0u64;
    let mut n = 0;
    while d.app.mode == Mode::Ramp && n < 200_000 {
        let p = u64::from(period(d.rate60));
        let delta = g.after(work, p);
        d.call_full(0, delta as u32, false);
        work = cost.work(&d.app.last);
        n += 1;
    }
    assert_eq!(d.app.mode, Mode::Result);
    println!("          over points {:?}", d.app.ramp.overs());
    let r = d.app.ramp.results;
    ([r[0].fit, r[1].fit], [r[0].safe, r[1].safe])
}

#[test]
#[cfg_attr(debug_assertions, ignore = "slow in debug: run with --release")]
fn the_ramp_finds_the_long_run_fit_on_a_moving_scene() {
    let costs = [
        // Triangle set-up heavy.
        Cost {
            fixed_us: 2_000,
            tri_ns: 9_000,
            px_ns: 20,
            obj_ns: 30_000,
        },
        // Pixel heavy (slow fill).
        Cost {
            fixed_us: 3_000,
            tri_ns: 3_000,
            px_ns: 120,
            obj_ns: 20_000,
        },
        // A one-core-like fixed part: the send inside the loop.
        Cost {
            fixed_us: 12_000,
            tri_ns: 6_000,
            px_ns: 40,
            obj_ns: 25_000,
        },
    ];
    let mut worst = 0;
    let mut safe_err = [0; 2];
    for (i, t) in TESTS.iter().enumerate() {
        let Kind::Scene(_) = t.kind else { continue };
        for cost in costs {
            let (got, safe) = ramp(i, cost);
            for (r, rate60) in [true, false].into_iter().enumerate() {
                // The truth: the largest step whose long run fits.
                let mut best = 0;
                for k in 0..=t.kmax {
                    let load = load_of(t.base, k);
                    if long_run(t.kind, load, rate60, cost).0 {
                        best = best.max(load);
                    }
                }
                let ks = |l: u32| (0..=t.kmax).position(|k| load_of(t.base, k) >= l).unwrap();
                let off = ks(got[r]) as i32 - ks(best) as i32;
                println!(
                    "{:<9} {cost:?} at {}: ramp {} long run {} ({off:+} steps)",
                    t.name,
                    if rate60 { 60 } else { 30 },
                    got[r],
                    best
                );
                worst = worst.max(off.abs());
                // SAFE: from the cost line of two over steps.
                let want = true_safe(t.kind, load_of(t.base, t.kmax), rate60, cost).min(got[r]);
                let Some(s) = safe[r] else {
                    println!("          safe missing, true {want}");
                    continue;
                };
                let err = (i64::from(s) - i64::from(want)).abs() * 100 / i64::from(want.max(1));
                println!("          safe {s}, true {want} ({err} %)");
                safe_err[r] = safe_err[r].max(err);
            }
        }
    }
    assert!(
        worst <= 1,
        "the ramp is {worst} steps from the long-run fit"
    );
    println!(
        "largest SAFE error: at 60 {} %, at 30 {} %",
        safe_err[0], safe_err[1]
    );
    assert!(safe_err[1] <= 10, "SAFE at 30 is {} % off", safe_err[1]);
}
