//! A model of the console's two-core send (ESTIMATED: a model, not the
//! board), to drive the app with the times between calls the board would
//! give. Ported from the round-3 checker's model (`lock_sim.py`, kept
//! outside the repository): arithmetic from `boards/cyd/send-plan` (Job::new,
//! Job::window), the order of the steps from `boards/cyd/src/{screen.rs,
//! beam.rs,core2.rs}` and console-core's `next_wake`. Times in
//! microseconds; panel numbers from `board3/boot.txt` (lines=343.45,
//! line_us=49.550).
//!
//! Core 1: a step starts at max(grid, end of the last step); the game
//! draws (`Cost`); when something changed it waits until core 2 is free
//! and hands over (HAND); the rest of the step (AFTER). Grid: next += P,
//! or now + P when already past. Core 2: wakes, finds the panel's line
//! (an "anchor": a throwaway read, then tries of two reads; a try misses
//! when the panel is in its porch and is tried again 300 us later, at most
//! 2 tries), plans, waits for the first sweep the box fits behind, sends
//! one box around everything that changed.
//!
//! What it leaves out: the send plan's choice of separate windows (the
//! board sent boxes in the probes), the radio, flash cache misses, the
//! touch and log work, the variation of the draw time inside a picture.
//! What it can show: whether a run of pictures, by their changed boxes
//! alone, holds the calls to the panel's sweep (about 17,018 us) instead
//! of the 16,667 us frame. What it cannot show: the exact board numbers.
#![allow(dead_code)]

use bench::city::CYCLE;
use bench::load::{Kind, Work};
use console_video::{Areas, Frame as VFrame, diff};

pub const LINE_US: f64 = 49.550;
pub const LINES: f64 = 343.45;
/// The panel's own refresh (one sweep): about 17,018 us.
pub const REFRESH_US: f64 = LINES * LINE_US;
const GUARD: f64 = 8.0;
const LEAD_US: f64 = 35.0;
const TAIL_US: f64 = 10.0;
const SLOW: f64 = 1.015;
const HAND: f64 = 50.0;
const AFTER: f64 = 80.0;
const WAKE: f64 = 30.0;
const READ: f64 = 20.0;
const RETRY: f64 = 300.0;
const PLAN: f64 = 40.0;
const UNTRUST_LO: f64 = 318.0;
const UNTRUST_HI: f64 = 333.45;

/// The changed box of one picture, in screen pixels (twice the game's):
/// `w` along the panel's sweep (screen x), `h` across it, `x0` its left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SendBox {
    pub w: f64,
    pub h: f64,
    pub x0: f64,
}

impl SendBox {
    /// At least 316 of the 320 columns: the width the checker's model
    /// found to lock the pace to the sweep.
    pub fn full_width(&self) -> bool {
        self.w >= 316.0
    }
}

/// The box around every changed area (none: nothing changed).
pub fn box_of(shown: &VFrame, cur: &VFrame, areas: &mut Areas) -> Option<SendBox> {
    diff(shown, cur, areas);
    let a = areas.as_slice();
    if a.is_empty() {
        return None;
    }
    let x0 = a.iter().map(|r| u32::from(r.x)).min().unwrap();
    let x1 = a
        .iter()
        .map(|r| u32::from(r.x) + u32::from(r.w))
        .max()
        .unwrap();
    let y0 = a.iter().map(|r| u32::from(r.y)).min().unwrap();
    let y1 = a
        .iter()
        .map(|r| u32::from(r.y) + u32::from(r.h))
        .max()
        .unwrap();
    Some(SendBox {
        w: f64::from((x1 - x0) * 2),
        h: f64::from((y1 - y0) * 2),
        x0: f64::from(x0 * 2),
    })
}

struct Job {
    lo: f64,
    hi: f64,
    lead: f64,
    lines: f64,
}

fn job(b: SendBox) -> Job {
    let pg = b.h * 2.0 * 8.0 / 80.0 * SLOW / LINE_US;
    let lead = LEAD_US / LINE_US;
    let (g0, g1) = (b.x0, b.x0 + b.w - 1.0);
    let lines = lead + b.w * pg + TAIL_US / LINE_US;
    let slope = (g1 - g0) * (1.0 - pg);
    Job {
        lo: g0 + 1.0 + GUARD + slope.max(0.0),
        hi: LINES + g0 - pg - GUARD + slope.min(0.0),
        lead,
        lines,
    }
}

fn start_v(v_now: f64, j: &Job) -> f64 {
    let k0 = (v_now / LINES).floor();
    for dk in [-1.0, 0.0, 1.0, 2.0] {
        let k = k0 + dk;
        let call = v_now.max(k * LINES + j.lo - j.lead);
        if call + j.lead <= k * LINES + j.hi {
            return call;
        }
    }
    // A box that fits behind no sweep (not met by these pictures).
    v_now
}

/// Core 1's draw time for one call (ESTIMATED): `fixed_us + load *
/// load_us + tris * tri_us`, fitted to the board's CITY steps at 30
/// ([`Cost::city`]): a part per building (whether seen or not) and a
/// part per triangle drawn, so pictures of one load differ as their
/// triangle counts do.
#[derive(Clone, Copy, Debug)]
pub struct Cost {
    pub fixed_us: f64,
    pub load_us: f64,
    pub tri_us: f64,
}

/// MEASURED on the board (round 3, `board3/city-<load>-30.txt`, perf
/// `step_avg_us`, two cores, BENCH at opt-level s): CITY's load and core
/// 1's average step at 30.
pub const BOARD_CITY_30: [(u32, f64); 5] = [
    (8, 5355.0),
    (32, 10723.0),
    (64, 19614.0),
    (96, 28074.0),
    (128, 35100.0),
];

impl Cost {
    pub fn us(&self, load: u32, tris: u32) -> f64 {
        self.fixed_us + f64::from(load) * self.load_us + f64::from(tris) * self.tri_us
    }

    /// Least squares through the board's steps (less the model's own
    /// HAND + AFTER) against the load and the PC's average triangle count
    /// of the same load over a whole cycle (`avg_tris[i]` for
    /// `BOARD_CITY_30[i]`).
    // Row and column indices of a 3 x 4 matrix (Gauss-Jordan): clearer
    // with indices than with iterators.
    #[allow(clippy::needless_range_loop)]
    pub fn city(avg_tris: &[f64; 5]) -> Cost {
        // Normal equations for y = a + b * load + c * tris.
        let mut m = [[0.0f64; 4]; 3];
        for (i, &(load, step)) in BOARD_CITY_30.iter().enumerate() {
            let row = [1.0, f64::from(load), avg_tris[i]];
            let y = step - HAND - AFTER;
            for r in 0..3 {
                for c in 0..3 {
                    m[r][c] += row[r] * row[c];
                }
                m[r][3] += row[r] * y;
            }
        }
        for col in 0..3 {
            let piv = (col..3)
                .max_by(|&a, &b| m[a][col].abs().total_cmp(&m[b][col].abs()))
                .unwrap();
            m.swap(col, piv);
            for r in 0..3 {
                if r != col {
                    let k = m[r][col] / m[col][col];
                    for c in col..4 {
                        m[r][c] -= k * m[col][c];
                    }
                }
            }
        }
        Cost {
            fixed_us: m[0][3] / m[0][0],
            load_us: m[1][3] / m[1][1],
            tri_us: m[2][3] / m[2][2],
        }
    }

    /// The fit against the board's steps: (load, board, model) per load,
    /// both with HAND + AFTER.
    pub fn residuals(&self, avg_tris: &[f64; 5]) -> Vec<(u32, f64, f64)> {
        BOARD_CITY_30
            .iter()
            .enumerate()
            .map(|(i, &(load, step))| {
                let model = self.fixed_us
                    + f64::from(load) * self.load_us
                    + avg_tris[i] * self.tri_us
                    + HAND
                    + AFTER;
                (load, step, model)
            })
            .collect()
    }
}

/// The console and its second core, call by call.
pub struct Pace {
    t: f64,
    next: f64,
    free_at: f64,
    last_start: f64,
    started: bool,
    /// Calls whose box was at least 316 columns wide, and all calls with
    /// a box.
    pub full: u32,
    pub sent: u32,
}

impl Pace {
    pub fn new() -> Pace {
        Pace {
            t: 0.0,
            next: 0.0,
            free_at: 0.0,
            last_start: 0.0,
            started: false,
            full: 0,
            sent: 0,
        }
    }

    /// The next call's start and the time since the last one (the
    /// call's `delta_us`), on the grid of `period`.
    pub fn begin(&mut self, period: u32) -> (f64, u32) {
        let p = f64::from(period);
        let start = self.next.max(self.t);
        if start >= self.next {
            self.next += p;
            if self.next <= start {
                self.next = start + p;
            }
        }
        let delta = if self.started {
            (start - self.last_start).round() as u32
        } else {
            period
        };
        self.started = true;
        self.last_start = start;
        (start, delta)
    }

    /// The call that started at `start` drew for `draw` us and changed
    /// `b` (None: nothing to send).
    pub fn end(&mut self, start: f64, draw: f64, b: Option<SendBox>, misses: bool) {
        let ready = start + draw;
        let Some(b) = b else {
            self.t = ready + AFTER;
            return;
        };
        self.sent += 1;
        if b.full_width() {
            self.full += 1;
        }
        let j = job(b);
        let hand = ready.max(self.free_at) + HAND;
        let mut c = hand + WAKE + READ;
        let mut tries = 0;
        loop {
            c += 2.0 * READ;
            tries += 1;
            let ph = (c / LINE_US) % LINES;
            let bad = misses && UNTRUST_LO < ph && ph < UNTRUST_HI;
            if !bad || tries >= 2 {
                break;
            }
            c += RETRY;
        }
        c += PLAN;
        let sv = start_v(c / LINE_US, &j);
        self.free_at = (sv + j.lines) * LINE_US;
        self.t = hand + AFTER;
    }
}

/// CITY's average triangle count over a whole cycle at `load`.
pub fn avg_tris(load: u32) -> f64 {
    let mut w = Work::new();
    let mut f = Box::new(VFrame::new());
    let mut sum = 0u64;
    for t in 0..CYCLE {
        sum += u64::from(w.run(&mut f, Kind::City, load, t, t == 0).tris);
    }
    sum as f64 / f64::from(CYCLE)
}

/// The PC's whole-cycle triangle averages of the board's loads.
pub fn city_tris() -> [f64; 5] {
    let mut x = [0.0; 5];
    for (i, &(load, _)) in BOARD_CITY_30.iter().enumerate() {
        x[i] = avg_tris(load);
    }
    x
}

/// [`Cost::city`] with the PC's counts.
pub fn city_cost() -> Cost {
    Cost::city(&city_tris())
}
