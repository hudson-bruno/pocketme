//! Round 3: the moving tests follow game time (ticks of 1/60 s), so they
//! move at the same real speed at 30 and at 60 calls a second.
//!
//! * [`advance`] turns measured deltas into ticks without stutter when
//!   the deltas wobble round the console's grid, catches up after a late
//!   call, and keeps game time with real time when every step is longer
//!   than the frame (the over-loaded tests): same speed at 30 and at 60.
//! * CAMBER's rule (a floor accumulator, kept here as a model; the game
//!   itself is not changed) does stutter under the same wobble, how
//!   often depending on how late the call that started its clock was.
//! * The picture at 60 after `2k` calls is the picture at 30 after `k`
//!   calls, and the calls in between show positions in between.
//! * CITY's cycle is 25.6 s at both rates.

mod common;

use bench::app::{MAX_TICKS, advance, tick_half_us};
use bench::city::{self, CYCLE};
use bench::load::{Kind, TESTS, TICK_US, spin_angle};
use common::Driver;
use game_kit::{Frame, W, button};

const P30: u32 = 33_333;
const P60: u32 = 16_667;

/// A small fixed random sequence.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, n: u32) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % u64::from(n)) as u32
    }
}

/// The console's calls on its grid: the call that starts the clock
/// comes `first_late` µs after a grid point, every later one `0..=
/// late_max` µs after its grid point (never early), and `delta_us` is
/// the measured time between two calls. The first call's delta is 0
/// (BENCH's first call does not move its clock). Deltas of `calls`
/// calls after it, with the first call's 0 in front.
fn grid_from(period: u32, first_late: u32, late_max: u32, calls: usize, seed: u64) -> Vec<u32> {
    let mut r = Lcg(seed);
    let mut last = u64::from(first_late);
    let mut v = vec![0];
    v.extend((1..=calls as u64).map(|k| {
        let at = k * u64::from(period) + u64::from(r.next(late_max + 1));
        let d = (at - last) as u32;
        last = at;
        d
    }));
    v
}

/// As [`grid_from`] with the first call as late as any other (random).
fn grid_deltas(period: u32, late_max: u32, calls: usize, seed: u64) -> Vec<u32> {
    let first = Lcg(seed ^ 0x5A5A).next(late_max + 1);
    grid_from(period, first, late_max, calls, seed)
}

/// Ticks per call under our rule at one rate.
fn ours(deltas: &[u32], rate60: bool) -> Vec<u32> {
    let mut rest = 0;
    deltas
        .iter()
        .map(|&d| {
            let (n, r) = advance(rest, d, rate60);
            rest = r;
            n
        })
        .collect()
}

/// CAMBER's rule (games/camber/src/app.rs), as a model: add the delta,
/// cap at 6 ticks, take every whole tick. Its clock starts at 0 on the
/// call that started the race (the first delta here).
fn camber(deltas: &[u32]) -> Vec<u32> {
    let mut acc = 0u32;
    deltas
        .iter()
        .map(|&d| {
            acc = acc.saturating_add(d).min(TICK_US * 6);
            let mut n = 0;
            while acc >= TICK_US {
                acc -= TICK_US;
                n += 1;
            }
            n
        })
        .collect()
}

#[test]
fn the_tick_is_half_the_consoles_frame() {
    assert_eq!(tick_half_us(false), P30, "30: 2 ticks of 16,666.5 µs");
    assert_eq!(tick_half_us(true), 2 * P60, "60: 1 tick of 16,667 µs");
    assert_eq!(bench::ramp::period(false), P30);
    assert_eq!(bench::ramp::period(true), P60);
}

#[test]
fn steady_rates_give_whole_ticks() {
    let n30 = ours(&vec![P30; 100_000], false);
    assert!(n30.iter().all(|&n| n == 2), "30: two ticks every call");
    let n60 = ours(&vec![P60; 100_000], true);
    assert!(n60.iter().all(|&n| n == 1), "60: one tick every call");
    // The first call of a game has a delta of about 0: no tick (the
    // rest is in half µs).
    assert_eq!(advance(0, 0, false), (0, 0));
    assert_eq!(advance(0, 300, false), (0, 600));
    assert_eq!(advance(0, 300, true), (0, 600));
}

#[test]
fn wobble_does_not_stutter() {
    println!("rate  wobble   ours: calls off");
    for (period, want, rate60) in [(P30, 2, false), (P60, 1, true)] {
        for late_max in [2, 1_000, 2_000, 4_000, 8_000] {
            for seed in 1..=4 {
                let d = grid_deltas(period, late_max, 20_000, seed);
                let a = ours(&d, rate60);
                assert_eq!(a[0], 0, "the first call draws time 0");
                let off = a[1..].iter().filter(|&&n| n != want).count();
                if seed == 1 {
                    println!(
                        "{:>4}  {late_max:>6}   {off:>15}",
                        if rate60 { 60 } else { 30 }
                    );
                }
                assert_eq!(off, 0, "period {period}, wobble {late_max}, seed {seed}");
                // Game time is real time to the nearest tick.
                let ticks: u64 = a.iter().map(|&n| u64::from(n)).sum();
                let half: u64 = d.iter().map(|&x| 2 * u64::from(x)).sum();
                let t = u64::from(tick_half_us(rate60));
                let exact = (half + t / 2) / t;
                assert!(ticks.abs_diff(exact) <= 1, "{ticks} {exact}");
            }
        }
    }
}

#[test]
fn camber_rule_stutters_depending_on_the_launch_offset() {
    // CAMBER's clock starts on the call that started the race; how late
    // that call came (its offset from the grid) decides at 60 whether the
    // floor rule stutters: on the grid never, late then whenever a later
    // call is less late than it. At 30 it slips anyway (1 µs a call).
    println!("calls of 20,000 off the steady count, CAMBER's rule (ours: 0 in every case)");
    println!("rate  wobble   launch offset 0     J/4     J/2       J   random");
    for (period, want, rate60) in [(P30, 2, false), (P60, 1, true)] {
        for late_max in [1_000u32, 2_000, 4_000] {
            let mut row = Vec::new();
            for first in [0, late_max / 4, late_max / 2, late_max, u32::MAX] {
                let d = if first == u32::MAX {
                    grid_deltas(period, late_max, 20_000, 1)
                } else {
                    grid_from(period, first, late_max, 20_000, 1)
                };
                let cam = camber(&d);
                row.push(cam[1..].iter().filter(|&&n| n != want).count());
                assert!(ours(&d, rate60)[1..].iter().all(|&n| n == want));
            }
            println!(
                "{:>4}  {late_max:>6}   {:>15} {:>7} {:>7} {:>7} {:>8}",
                if rate60 { 60 } else { 30 },
                row[0],
                row[1],
                row[2],
                row[3],
                row[4]
            );
            if rate60 {
                assert_eq!(row[0], 0, "60, first call on the grid: no stutter");
                assert!(row[3] > 0, "60, a late first call: stutters");
            } else {
                assert!(row[0] > 0, "30 slips whatever the offset");
            }
        }
    }
}

#[test]
fn camber_rule_slips_even_without_wobble() {
    // At 30 the console's period is 33,333, two ticks of 16,667 are
    // 33,334: CAMBER's floor rule gives 1 tick on the first call after
    // the start, then one call of 1 tick every 16,667 calls (9.3 min)
    // and 3 on the next.
    let mut d = vec![0];
    d.extend(vec![P30; 40_000]);
    let c = camber(&d);
    let ones: Vec<usize> = (1..c.len()).filter(|&i| c[i] == 1).collect();
    assert_eq!(ones, [1, 16_668, 33_335]);
    // Ours does not.
    assert!(ours(&d, false)[1..].iter().all(|&n| n == 2));
}

#[test]
fn a_late_call_catches_up() {
    // At 30, one call 16.7 ms late, the next one back on the grid.
    let d = [P30, P30, P30 + 16_667, P30 - 16_667, P30, P30];
    assert_eq!(ours(&d, false), [2, 2, 3, 1, 2, 2]);
    // A whole frame missed (the console starts its grid again).
    let d = [P30, 2 * P30, P30, P30];
    assert_eq!(ours(&d, false), [2, 4, 2, 2]);
    // At 60, a call 50 ms late.
    let d = [P60, P60 + 50_000, P60];
    assert_eq!(ours(&d, true), [1, 4, 1]);
    // A stall (a card read, a pause the console did not take out): at
    // most MAX_TICKS, then the rest is dropped.
    assert_eq!(advance(0, 2_000_000, false), (MAX_TICKS, 0));
    assert_eq!(advance(16_000, u32::MAX, true), (MAX_TICKS, 0));
    assert_eq!(advance(i32::MAX, u32::MAX, true), (MAX_TICKS, 0));
    assert_eq!(MAX_TICKS * TICK_US, 250_005);
}

#[test]
fn advance_stays_in_its_bounds() {
    // Every rest it can hand back, every delta (in steps), both rates:
    // the new rest is within half a tick, the ticks at most MAX_TICKS,
    // and below a stall nothing is lost (overflow checks are on here).
    for rate60 in [false, true] {
        let t = i64::from(tick_half_us(rate60));
        for rest in -(t / 2 + 1)..=t / 2 + 1 {
            let mut d = 0u32;
            while d < 300_000 {
                let (n, r) = advance(rest as i32, d, rate60);
                assert!(n <= MAX_TICKS);
                if n < MAX_TICKS {
                    assert!(i64::from(r).abs() <= t / 2 + 1, "{rest} {d}: {n} {r}");
                    assert_eq!(rest + 2 * i64::from(d), i64::from(n) * t + i64::from(r));
                }
                d += 7_919;
            }
        }
    }
}

/// The console's grid (console-core `next_wake`, the board's wake-up):
/// a step at `now` moves the due time one frame on, or to `now + frame`
/// when that has passed already; the board wakes `late` µs after a due
/// time it slept for, and a step that ends after its due time is
/// followed at once. Returns each call's delta (the first 0) and time.
fn console_calls(
    period: u32,
    calls: usize,
    mut work: impl FnMut(usize) -> u64,
    mut late: impl FnMut() -> u64,
) -> (Vec<u32>, Vec<u64>) {
    let p = u64::from(period);
    let (mut now, mut next, mut last) = (1_000_000u64, 0u64, 0u64);
    let mut deltas = Vec::with_capacity(calls);
    let mut at = Vec::with_capacity(calls);
    for i in 0..calls {
        deltas.push(if i == 0 { 0 } else { (now - last) as u32 });
        at.push(now);
        last = now;
        if now >= next {
            next += p;
            if next <= now {
                next = now + p;
            }
        }
        let end = now + work(i);
        now = if end >= next { end } else { next + late() };
    }
    (deltas, at)
}

/// Game time over real time for steps of `work` µs at one rate.
fn speed(
    rate60: bool,
    calls: usize,
    work: impl FnMut(usize) -> u64,
    late: impl FnMut() -> u64,
) -> f64 {
    let (d, at) = console_calls(bench::ramp::period(rate60), calls, work, late);
    let ticks: u64 = ours(&d, rate60).iter().map(|&n| u64::from(n)).sum();
    let tick_us = f64::from(tick_half_us(rate60)) / 2.0;
    ticks as f64 * tick_us / (at[calls - 1] - at[0]) as f64
}

#[test]
fn overloaded_steps_keep_real_speed_at_both_rates() {
    // Every step `w` µs long (longer than the frame: the over-loaded
    // tests), 8 to 90 ms in 250 µs steps. Round 3's first rule cut the
    // rest to a quarter tick and ran at 0.80 to 1.11 of real speed here.
    let (mut lo, mut hi) = (9.0f64, 0.0f64);
    for rate60 in [false, true] {
        let mut w = 8_000u64;
        while w <= 90_000 {
            let s = speed(rate60, 20_000, |_| w, || 0);
            lo = lo.min(s);
            hi = hi.max(s);
            assert!(
                (s - 1.0).abs() < 1e-3,
                "rate60 {rate60}, step {w}: speed {s}"
            );
            w += 250;
        }
    }
    println!("steps of 8 to 90 ms, both rates: game speed {lo:.5} to {hi:.5}");
    // The loads of the board round (round 2's board loop for CITY 128
    // at 30, MEASURED ON THE BOARD; the others ESTIMATED), with 0 to 1.5
    // ms of jitter on every step.
    for (what, rate60, w) in [
        ("CITY 128 at 30 (board loop 36,760)", false, 36_760u64),
        ("CITY 64 at 60 (ESTIMATED loop 21,000)", true, 21_000),
        ("ISLAND XL at 60 (ESTIMATED loop 24,000)", true, 24_000),
        ("ISLAND XXL at 30 (ESTIMATED loop 50,300)", false, 50_300),
        ("ISLAND XXL at 60 (ESTIMATED loop 35,100)", true, 35_100),
    ] {
        let mut r = Lcg(3);
        let s = speed(rate60, 6_000, |_| w + u64::from(r.next(1_500)), || 0);
        println!("  {what}: game speed {s:.4}");
        assert!((s - 1.0).abs() < 1e-3, "{what}: {s}");
    }
}

#[test]
fn a_test_moves_at_the_same_speed_at_30_and_60_however_loaded() {
    // The same step length at both rates: the same game time after the
    // same real time (to a tick), so an over-loaded test is not slower
    // or faster at one rate than at the other.
    for w in [10_000u64, 20_750, 30_000, 36_000, 46_000] {
        let a = speed(false, 6_000, |_| w, || 0);
        let b = speed(true, 6_000, |_| w, || 0);
        assert!((a - b).abs() < 2e-3, "step {w}: 30 {a}, 60 {b}");
    }
}

#[test]
fn the_app_keeps_real_speed_under_overload() {
    // Through the app itself: CITY at 60 with every step 20.75 ms (the
    // first rule's worst case, 0.803): after the calls of 25.6 s the
    // game time is one cycle.
    let i = TESTS.iter().position(|t| t.kind == Kind::City).unwrap();
    let mut d = at(i, 0, true);
    let (deltas, times) = console_calls(P60, 1_300, |_| 20_750, || 0);
    for &dt in &deltas[1..] {
        d.call_full(0, dt, false);
    }
    let real = times[1_299] - times[0];
    let want = (2 * real + 33_334 / 2) / 33_334;
    assert!(
        u64::from(d.app.gt).abs_diff(want) <= 1,
        "{} {want}",
        d.app.gt
    );
}

/// The app at test `i`, level `lv`, at a rate, its clock started on the
/// call that switched to that rate: after this call `gt` is 0.
fn at(i: usize, lv: usize, rate60: bool) -> Driver {
    let mut d = Driver::new();
    d.pick(i, lv);
    d.call(button::X); // to 60, restart: this call draws time 0
    if !rate60 {
        d.call(button::X); // back to 30, restart again
    }
    assert_eq!(d.app.gt, 0);
    d
}

fn work_area(f: &Frame) -> &[u8] {
    &f.0[20 * W..120 * W]
}

#[test]
fn sixty_shows_the_same_scene_at_the_same_time() {
    for (i, t) in TESTS.iter().enumerate() {
        let calls30 = if t.kind == Kind::City { 24 } else { 40 };
        for lv in 0..t.loads.len() {
            let mut a = at(i, lv, false);
            let mut b = at(i, lv, true);
            let mut in_between = 0;
            for k in 1..=calls30 {
                a.call(0);
                b.call(0);
                let odd = b.frame.0;
                b.call(0);
                assert_eq!(a.app.gt, 2 * k);
                assert_eq!(b.app.gt, 2 * k);
                if t.kind.moves() {
                    assert_eq!(
                        work_area(&a.frame),
                        work_area(&b.frame),
                        "{} {lv}: 60 call {} is 30 call {k}",
                        t.name,
                        2 * k
                    );
                    in_between += u32::from(odd[20 * W..120 * W] != *work_area(&b.frame));
                }
            }
            if t.kind.moves() {
                // The pictures in between are new pictures, not repeats.
                assert!(
                    in_between * 10 >= calls30 * 9,
                    "{} {lv}: {in_between}",
                    t.name
                );
            }
        }
    }
}

#[test]
fn the_in_between_ticks_are_in_between() {
    // CITY: the eye and every car move half a step on an odd tick.
    for t in 0..CYCLE - 2 {
        if t % 2 == 1 {
            continue;
        }
        let (a, m, b) = (city::eye_z(t), city::eye_z(t + 1), city::eye_z(t + 2));
        assert!(a < m && m < b, "{t}: {a} {m} {b}");
        for lane in 0..4 {
            let (a, m, b) = (
                city::lane_car_z(lane, 0, t),
                city::lane_car_z(lane, 0, t + 1),
                city::lane_car_z(lane, 0, t + 2),
            );
            if a.abs_diff(b) < 1000 {
                assert!(m.abs_diff(a) <= a.abs_diff(b) && m.abs_diff(b) <= a.abs_diff(b));
            }
        }
    }
    // TRI SPIN: half of a call's turn.
    for i in 0..16 {
        for t in [0u32, 2, 100, 1000] {
            let a = spin_angle(i, t);
            let m = spin_angle(i, t + 1);
            let b = spin_angle(i, t + 2);
            assert_eq!(m.wrapping_sub(a), b.wrapping_sub(m), "{i} {t}");
            assert_eq!(m.wrapping_sub(a).min(a.wrapping_sub(m)), 512);
        }
    }
}

#[test]
fn the_city_cycle_is_25_6_seconds_at_both_rates() {
    let i = TESTS.iter().position(|t| t.kind == Kind::City).unwrap();
    for (rate60, calls) in [(false, 768u32), (true, 1536u32)] {
        let mut d = at(i, 0, rate60);
        let start = work_area(&d.frame).to_vec();
        let mut half = Vec::new();
        for c in 1..=calls {
            d.call(0);
            if c == calls / 2 {
                half = work_area(&d.frame).to_vec();
            }
        }
        // 768 calls of 33,333 µs or 1,536 of 16,667 µs: 25.6 s.
        assert_eq!(d.app.gt, CYCLE);
        assert_eq!(work_area(&d.frame), &start[..], "the cycle closes");
        assert_ne!(half, start);
    }
    assert_eq!(u64::from(CYCLE) * u64::from(TICK_US), 25_600_512);
}

#[test]
fn ramps_advance_per_call_not_by_time() {
    // The ramp's pictures are the same at both rates and with late
    // calls: CITY steps RAMP_STRIDE ticks a call, the others 2.
    let i = TESTS.iter().position(|t| t.kind == Kind::City).unwrap();
    let pics = |late: u32| {
        let mut d = Driver::new();
        d.pick(i, 0);
        d.call(button::A);
        let mut v = Vec::new();
        for _ in 0..20 {
            let p = bench::ramp::period(d.rate60);
            d.call_full(0, p + late, false);
            v.push(common::hash(&d.frame));
        }
        v
    };
    assert_eq!(pics(0), pics(30_000));
    assert_eq!(city::RAMP_CALLS * city::RAMP_STRIDE, CYCLE);
    assert_eq!(city::RAMP_CALLS, 96);
}
