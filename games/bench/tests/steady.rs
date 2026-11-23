//! Added by the check: are the moving levels steady over the windows the
//! board is read in? A scene repeats every `CYCLE / SPEED` calls (277
//! at 30, 555 at 60: the scenes move by game time, ticks of 1/60 s);
//! a reading over a window shorter than that sees only part of it. These
//! tests measure how much the triangles and pixels per call change from
//! one window to the next, and fail when a level the run plan reads is
//! not steady enough. Run with `--nocapture` for the table.

mod common;

use bench::load::{Kind, TESTS, Work};
use bench::ramp::load_of;
use game_kit::Frame;

/// Per-call (tris, px) of a scene for one whole cycle and a bit more,
/// `step` ticks of 1/60 s a call (2 at 30 and in the ramp, 1 at 60).
fn trace(kind: Kind, load: u32, calls: u32, step: u32) -> Vec<(u32, u32)> {
    let mut w = Work::new();
    let mut f = Frame::new();
    (0..calls)
        .map(|t| {
            let c = w.run(&mut f, kind, load, t * step, t == 0);
            (c.tris, c.px)
        })
        .collect()
}

/// Smallest and largest average over every window of `n` calls, as a
/// per mille spread round the mean of the whole cycle.
fn spread(v: &[u32], n: usize, cycle: usize) -> (u32, u32, u32) {
    let mean = v[..cycle].iter().map(|&x| u64::from(x)).sum::<u64>() / cycle as u64;
    let (mut lo, mut hi) = (u64::MAX, 0u64);
    for s in 0..cycle {
        let a = v[s..s + n].iter().map(|&x| u64::from(x)).sum::<u64>() / n as u64;
        lo = lo.min(a);
        hi = hi.max(a);
    }
    let pm = |x: u64| ((x as i64 - mean as i64).unsigned_abs() * 1000 / mean.max(1)) as u32;
    (mean as u32, pm(lo), pm(hi))
}

const CYCLE: usize = 278; // ceil(26 * 256 / 24): the scene repeats at 30
const CYCLE60: usize = 555; // ceil(26 * 256 / 12): the same at 60

#[test]
fn fixed_scene_levels_are_steady_over_a_six_second_reading() {
    println!("level        load  rate  window  tris-mean  lo‰  hi‰   px-mean  lo‰  hi‰");
    for t in TESTS.iter().filter(|t| matches!(t.kind, Kind::Scene(_))) {
        for &load in t.loads {
            for (rate, n) in [(30, 180usize), (60, 360usize), (30, 30), (60, 60)] {
                let (step, cycle) = if rate == 60 { (1, CYCLE60) } else { (2, CYCLE) };
                let v = trace(t.kind, load, (cycle * 4) as u32, step);
                let tris: Vec<u32> = v.iter().map(|x| x.0).collect();
                let px: Vec<u32> = v.iter().map(|x| x.1).collect();
                let (tm, tlo, thi) = spread(&tris, n, cycle);
                let (pm, plo, phi) = spread(&px, n, cycle);
                println!(
                    "{:<10} {load:>5}  {rate:>4}  {n:>6}  {tm:>9}  {tlo:>3}  {thi:>3}  {pm:>8}  {plo:>3}  {phi:>3}",
                    t.name
                );
                // A 6 s reading (180 or 360 calls) of a level with at least
                // 8 objects: within 5 % of the cycle's mean.
                if n >= 180 && load >= 8 {
                    assert!(
                        tlo <= 50 && thi <= 50,
                        "{} {load}: tris spread {tlo}/{thi} per mille",
                        t.name
                    );
                }
            }
        }
    }
}

/// The ramp measures each step over about half a second: 15 calls at 30,
/// 30 at 60, 2 ticks a call at both rates (the ramp does not follow game
/// time). For the loads a ramp ends near, the triangles drawn must not
/// swing so much that a step is judged by where in the cycle it fell.
#[test]
fn ramp_scene_steps_see_a_steady_load() {
    println!("level        k  load  rate  tris-mean  lo‰  hi‰");
    for t in TESTS.iter().filter(|t| matches!(t.kind, Kind::Scene(_))) {
        for k in (0..=t.kmax).step_by(2) {
            let load = load_of(t.base, k);
            let v = trace(t.kind, load, (CYCLE * 2) as u32, 2);
            let tris: Vec<u32> = v.iter().map(|x| x.0).collect();
            for (rate, n) in [(60, 30usize), (30, 15usize)] {
                let (m, lo, hi) = spread(&tris, n, CYCLE);
                println!(
                    "{:<10} {k:>3} {load:>5}  {rate:>4}  {m:>9}  {lo:>3}  {hi:>3}",
                    t.name
                );
            }
        }
    }
}
