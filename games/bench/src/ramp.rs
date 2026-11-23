//! The ramp: finds the largest load that fits a frame, by trying.
//!
//! A game cannot read a clock inside its own call: the console reads the
//! time once per step and hands the game `delta_us`, the game time since
//! the last call. While the work fits the frame, `delta_us` is the frame
//! (16,667 or 33,333 µs) and says nothing about the work. When the work
//! is too much for the frame, the console calls again at once, so
//! `delta_us` becomes the time of one whole loop: the game's work plus
//! the console's own (input, sending the picture, the log). The ramp
//! uses both facts:
//!
//! * A load **fits** when the average time between calls stays at the
//!   frame (at most 1/64 over) and few calls are late (more than 1/8
//!   over): at most one in 16, or one.
//! * A load is **over** when the average is more than 5/4 of the frame.
//!   Then the average is the loop's real time. Two over loads give a
//!   straight line: the cost of one more unit (the slope) and the fixed
//!   cost of a loop (where the line meets load 0). An over loop's time
//!   does not depend on the rate (the console calls again at once), so
//!   the over loads of both rates lie on the same line: the line at 30
//!   uses the two largest over loads seen at either rate. Without this,
//!   the 30 ramp often reached its top step before two loads were over
//!   at 30 and had no line at all.
//! * **SAFE** is where that line reaches 4/5 of the frame: the load that
//!   leaves a fifth of the frame free (the stated margin). It is never
//!   more than the measured FIT.
//!
//! Loads go up in steps of a quarter of a doubling: `load(k) = base *
//! 2^(k/4) * MANT[k % 4] / 64` (each step about 19 % more). The search:
//!
//! 1. coarse: k = 0, 4, 8, ... (doubling) until a load does not fit;
//! 2. fine: from the last load that fit, one step at a time, until one
//!    does not fit (steps with the same whole-number load are skipped);
//! 3. probe: when fewer than two over loads were seen at this rate, two
//!    steps above the highest so far, until two are seen (or a loop takes
//!    more than three frames, or the top step is reached). At the top
//!    step, while the line still has fewer than two points, the untried
//!    steps just below the top, downwards (added by the check: the
//!    coarse search jumps four steps, so it can reach the top with one
//!    over step and the untried ones between it and the fine search).
//!
//! When even the top step fits, FIT is only a floor: the result says so
//! (`capped`, shown as "512+").
//!
//! First at 60 a second from k = 0, then at 30 from the load that fit
//! at 60. Each step: [`WARM`] calls not counted (the load and the rate
//! settle), then about half a second ([`STEP_US`]) and at least
//! [`MIN_FRAMES`] calls measured. CITY's steps are longer (see
//! [`Ramp::frame_n`] and `crate::city::RAMP_STRIDE`): each measures one
//! whole cycle of the street, so FIT and its count are the cycle's.
//!
//! **SAFE "-"**: the cost line needs two over steps. When even the top
//! step's loop stays under 5/4 of the frame, there is no line and no
//! SAFE (shown as "-"); FIT is then the measured answer.
//!
//! **Near panel pace** (CITY only, [`Ramp::screen_rule`]): a label,
//! never a verdict. At 60 a step can miss the frame because the *screen*
//! holds the calls back, not the drawing: when pictures change across the
//! whole width of the screen, the console sends them in step with the
//! panel's own refresh, which on the CYD is a little slower than 60 a
//! second (58.7 times, one sweep [`SWEEP_US`]). Then the time between
//! calls sits at about one sweep, 17.0 ms, whatever the load. But a
//! drawing that takes about 17.0 ms a call looks exactly the same from
//! inside the game (BENCH only gets `delta_us`, never a clock inside a
//! call), so BENCH cannot tell the two apart. A step that does not fit,
//! averages more than the frame's limit but at most [`SCREEN_MAX_US`] and
//! has no more late calls than a fit may have is therefore only
//! **labelled** "near panel pace": it still counts as over, the search
//! goes on exactly as without the label, and FIT stays the strict answer
//! (the largest load that kept the full 60). The result says how many
//! steps had the label, the last one's average and the lowest load that
//! had it. The console's own `perf` reading taken during such steps tells
//! the panel from the drawing (`fps` about 58.8 and a long
//! `present_avg_us`: the console waited for the screen).

/// The four steps inside one doubling, in 1/64.
pub const MANT: [u32; 4] = [64, 76, 91, 108];
/// Calls not counted at the start of each step.
pub const WARM: u32 = 4;
/// Game time measured per step (µs).
pub const STEP_US: u32 = 500_000;
/// Calls measured per step, at least.
pub const MIN_FRAMES: u32 = 4;
/// The console's frames (game_api: 1,000,000 / 60 and / 30, rounded).
pub const PERIOD_60: u32 = 16_667;
pub const PERIOD_30: u32 = 33_333;
/// The CYD panel's sweep (µs): boot line `lines=343.45 line_us=49.550`,
/// MEASURED ON THE BOARD (round 3).
pub const SWEEP_US: u32 = 17_018;
/// The longest average a step "near panel pace" may have (µs): the sweep
/// plus 1/64 of it, the same margin a fit has over the frame. Round 3's
/// CITY steps held by the panel averaged 16,942 to 17,165 µs on the board,
/// some with one dropped sweep (a call of about 34,1xx µs) or up to 3
/// late calls.
pub const SCREEN_MAX_US: u32 = SWEEP_US + SWEEP_US / 64;

/// The frame at 60 or 30 calls a second.
pub const fn period(rate60: bool) -> u32 {
    if rate60 { PERIOD_60 } else { PERIOD_30 }
}

/// The load of step `k`.
pub fn load_of(base: u32, k: u8) -> u32 {
    let k = u32::from(k);
    let m = MANT[(k % 4) as usize];
    base.saturating_mul(1u32 << (k / 4).min(24))
        .saturating_mul(m)
        / 64
}

/// A call is late when it comes more than 1/8 of a frame after its time.
pub const fn late_limit(period: u32) -> u32 {
    period + period / 8
}

/// Times between calls, added up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Meter {
    /// Calls counted.
    pub frames: u32,
    /// Their times added (µs).
    pub sum: u32,
    /// The longest (µs).
    pub worst: u32,
    /// Calls that came late ([`late_limit`]).
    pub late: u32,
    /// A count the workload reports per call (triangles, pixels),
    /// added up.
    pub extra: u32,
    /// Calls still to skip before counting.
    pub skip: u32,
}

impl Meter {
    /// Starts again, the first `skip` calls not counted.
    pub fn restart(&mut self, skip: u32) {
        *self = Meter {
            skip,
            ..Meter::default()
        };
    }

    /// One call: `delta` µs since the last one.
    pub fn add(&mut self, delta: u32, period: u32, extra: u32) {
        if self.skip > 0 {
            self.skip -= 1;
            return;
        }
        self.frames = self.frames.saturating_add(1);
        self.sum = self.sum.saturating_add(delta);
        self.worst = self.worst.max(delta);
        if delta > late_limit(period) {
            self.late = self.late.saturating_add(1);
        }
        self.extra = self.extra.saturating_add(extra);
    }

    /// The average time between calls (0 before the first).
    pub fn avg(&self) -> u32 {
        self.sum.checked_div(self.frames).unwrap_or(0)
    }

    /// The average of the workload's count.
    pub fn extra_avg(&self) -> u32 {
        self.extra.checked_div(self.frames).unwrap_or(0)
    }

    /// Whether the calls kept to the frame.
    pub fn fits(&self, period: u32) -> bool {
        self.frames > 0
            && self.avg() <= period + period / 64
            && self.late <= (self.frames / 16).max(1)
    }

    /// Whether the calls kept to about one panel sweep instead (see the
    /// module's "Near panel pace"): over the frame's limit, at most
    /// [`SCREEN_MAX_US`] on average, and no more late calls than a fit
    /// may have. Only a label: such a step still counts as over.
    pub fn screen_pace(&self, period: u32) -> bool {
        self.frames > 0
            && self.avg() > period + period / 64
            && self.avg() <= SCREEN_MAX_US
            && self.late <= (self.frames / 16).max(1)
    }
}

/// One measured step, for the log and the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Step {
    pub rate60: bool,
    pub k: u8,
    pub load: u32,
    pub avg: u32,
    pub worst: u32,
    pub late: u32,
    pub frames: u32,
    pub extra: u32,
    pub fits: bool,
    /// Did not fit, and the calls kept to about one panel sweep (CITY
    /// only): a label, the step still counts as over.
    pub screen: bool,
}

/// What the ramp found at one rate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RateResult {
    /// The largest load that fit (0: not even the first step).
    pub fit: u32,
    /// The workload's count at that load (triangles for a scene).
    pub fit_extra: u32,
    /// The largest load that leaves a fifth of the frame free (from the
    /// cost line, at most `fit`). None: no line (fewer than two over
    /// loads).
    pub safe: Option<u32>,
    /// The cost line: ns per unit of load ...
    pub unit_ns: Option<u32>,
    /// ... and µs per loop at load 0 (negative only from noise).
    pub fixed_us: Option<i32>,
    /// Steps measured at this rate.
    pub steps: u8,
    /// The top step fit: the real FIT is larger than `fit`.
    pub capped: bool,
    /// Steps labelled "near panel pace" (counted as over).
    pub screen: u8,
    /// The average of the last of them (µs).
    pub screen_us: u32,
    /// The lowest load that had the label (0: none).
    pub screen_load: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Coarse,
    Fine,
    Probe,
}

/// The ramp's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ramp {
    pub base: u32,
    pub kmax: u8,
    /// The rate being measured: 60 first, then 30.
    pub rate60: bool,
    /// The step being measured.
    pub k: u8,
    phase: Phase,
    best: Option<u8>,
    best_extra: u32,
    /// The first step that did not fit (above `kmax`: none yet).
    fail: u8,
    /// The highest step measured at this rate.
    top: u8,
    /// The two over steps with the largest loads, at either rate:
    /// (load, average).
    overs: [(u32, u32); 2],
    n_over: u8,
    /// Over steps seen at this rate.
    n_here: u8,
    /// The lowest step tried by the probe downwards from the top
    /// (`u8::MAX`: none yet).
    down: u8,
    /// The longest average at this rate.
    peak: u32,
    steps: u8,
    pub meter: Meter,
    /// At 60, at 30.
    pub results: [RateResult; 2],
    pub done: bool,
    /// Whether a step may be labelled "near panel pace" (CITY's ramp).
    pub screen_rule: bool,
    screen_n: u8,
    screen_us: u32,
    screen_load: u32,
}

impl Ramp {
    pub const fn new(base: u32, kmax: u8) -> Ramp {
        Ramp {
            base,
            kmax,
            rate60: true,
            k: 0,
            phase: Phase::Coarse,
            best: None,
            best_extra: 0,
            fail: u8::MAX,
            top: 0,
            overs: [(0, 0); 2],
            n_over: 0,
            n_here: 0,
            down: u8::MAX,
            peak: 0,
            steps: 0,
            meter: Meter {
                frames: 0,
                sum: 0,
                worst: 0,
                late: 0,
                extra: 0,
                skip: WARM,
            },
            results: [RateResult {
                fit: 0,
                fit_extra: 0,
                safe: None,
                unit_ns: None,
                fixed_us: None,
                steps: 0,
                capped: false,
                screen: 0,
                screen_us: 0,
                screen_load: 0,
            }; 2],
            done: false,
            screen_rule: false,
            screen_n: 0,
            screen_us: 0,
            screen_load: 0,
        }
    }

    /// The load to run now.
    pub fn load(&self) -> u32 {
        load_of(self.base, self.k)
    }

    /// The frame being measured against.
    pub fn period(&self) -> u32 {
        period(self.rate60)
    }

    /// The step's warm-up starts again (a pause, a rate not yet taken).
    pub fn interrupt(&mut self) {
        self.meter.restart(WARM);
    }

    /// One call: `delta` µs since the last one, `extra` the workload's
    /// count. `ok`: the console runs at the ramp's rate now (otherwise
    /// the call is not counted). Returns the step when one ends.
    pub fn frame(&mut self, delta: u32, extra: u32, ok: bool) -> Option<Step> {
        self.frame_n(delta, extra, ok, MIN_FRAMES)
    }

    /// [`Ramp::frame`] with another least number of calls per step: a
    /// step ends when both [`STEP_US`] and `min_frames` calls are
    /// measured (CITY: one whole cycle, see `app`).
    pub fn frame_n(&mut self, delta: u32, extra: u32, ok: bool, min_frames: u32) -> Option<Step> {
        if self.done {
            return None;
        }
        if !ok {
            self.interrupt();
            return None;
        }
        let p = self.period();
        self.meter.add(delta, p, extra);
        let m = self.meter;
        if m.sum < STEP_US || m.frames < min_frames {
            return None;
        }
        let fits = m.fits(p);
        let s = Step {
            rate60: self.rate60,
            k: self.k,
            load: self.load(),
            avg: m.avg(),
            worst: m.worst,
            late: m.late,
            frames: m.frames,
            extra: m.extra_avg(),
            fits,
            screen: self.screen_rule && !fits && m.screen_pace(p),
        };
        self.meter.restart(WARM);
        self.end_step(&s);
        Some(s)
    }

    fn end_step(&mut self, s: &Step) {
        let p = self.period();
        self.steps = self.steps.saturating_add(1);
        self.peak = self.peak.max(s.avg);
        self.top = self.top.max(s.k);
        if s.avg > p + p / 4 {
            self.note_over(s.load, s.avg);
        }
        if s.screen {
            // Only a label: the search below takes the step as over.
            self.screen_n = self.screen_n.saturating_add(1);
            self.screen_us = s.avg;
            if self.screen_load == 0 || s.load < self.screen_load {
                self.screen_load = s.load;
            }
        }
        let fits = s.fits;
        match self.phase {
            Phase::Coarse => {
                if fits {
                    self.best = Some(s.k);
                    self.best_extra = s.extra;
                    if s.k >= self.kmax {
                        self.probe();
                    } else {
                        self.k = s.k.saturating_add(4).min(self.kmax);
                    }
                } else {
                    self.fail = s.k;
                    self.fine();
                }
            }
            Phase::Fine => {
                if fits {
                    self.best = Some(s.k);
                    self.best_extra = s.extra;
                    self.fine();
                } else {
                    self.fail = s.k;
                    self.probe();
                }
            }
            Phase::Probe => self.probe(),
        }
    }

    /// The next step above `k` with a larger load (at most `kmax`).
    fn next_above(&self, k: u8) -> u8 {
        let l = load_of(self.base, k);
        let mut n = k.saturating_add(1).min(self.kmax);
        while n < self.kmax && load_of(self.base, n) <= l {
            n += 1;
        }
        n
    }

    fn fine(&mut self) {
        self.phase = Phase::Fine;
        let Some(b) = self.best else {
            return self.probe();
        };
        let n = self.next_above(b);
        if n >= self.fail || load_of(self.base, n) <= load_of(self.base, b) {
            self.probe();
        } else {
            self.k = n;
        }
    }

    fn probe(&mut self) {
        self.phase = Phase::Probe;
        let p = self.period();
        if self.n_here >= 2 || self.peak > p.saturating_mul(3) {
            return self.finish();
        }
        if self.top >= self.kmax {
            // Nothing above to try. When the line still lacks a point, try
            // the untried steps just below the top (between the first
            // step that did not fit and the top), from the top down.
            let d = if self.down == u8::MAX {
                self.kmax.saturating_sub(1)
            } else {
                self.down.saturating_sub(1)
            };
            let untried = self.fail != u8::MAX && d > self.fail;
            if self.n_over < 2 && untried && load_of(self.base, d) > load_of(self.base, self.fail) {
                self.down = d;
                self.k = d;
            } else {
                self.finish();
            }
            return;
        }
        let n = self
            .top
            .saturating_add(2)
            .min(self.kmax)
            .max(self.next_above(self.top));
        if load_of(self.base, n) <= load_of(self.base, self.top) {
            self.finish();
        } else {
            self.k = n;
        }
    }

    /// The over steps the cost line is drawn through: (load, average),
    /// smaller load first, and how many there are (for tests).
    pub fn overs(&self) -> ([(u32, u32); 2], u8) {
        (self.overs, self.n_over)
    }

    /// Keeps the two over steps with the largest loads, smaller first.
    fn note_over(&mut self, load: u32, avg: u32) {
        self.n_here = self.n_here.saturating_add(1);
        if self
            .overs
            .iter()
            .take(usize::from(self.n_over))
            .any(|o| o.0 == load)
        {
            // The same load over at both rates: keep the later reading.
            for o in self.overs.iter_mut().filter(|o| o.0 == load) {
                o.1 = avg;
            }
            return;
        }
        if self.n_over < 2 {
            self.overs[usize::from(self.n_over)] = (load, avg);
            self.n_over += 1;
        } else if load > self.overs[0].0 {
            self.overs[0] = (load, avg);
        }
        if self.n_over == 2 && self.overs[0].0 > self.overs[1].0 {
            self.overs.swap(0, 1);
        }
    }

    fn finish(&mut self) {
        let p = self.period();
        let fit = self.best.map_or(0, |b| load_of(self.base, b));
        let mut r = RateResult {
            fit,
            fit_extra: if self.best.is_some() {
                self.best_extra
            } else {
                0
            },
            steps: self.steps,
            capped: self.best == Some(self.kmax),
            screen: self.screen_n,
            screen_us: self.screen_us,
            screen_load: self.screen_load,
            ..RateResult::default()
        };
        let [(l1, a1), (l2, a2)] = self.overs;
        if self.n_over == 2 && l2 > l1 && a2 > a1 {
            let ns = u64::from(a2 - a1) * 1000 / u64::from(l2 - l1);
            if ns > 0 {
                let ns = ns.min(u64::from(u32::MAX));
                let fixed = i64::from(a1) - (u64::from(l1) * ns / 1000) as i64;
                let budget = i64::from(p * 4 / 5) - fixed;
                let safe = if budget <= 0 {
                    0
                } else {
                    (budget as u64 * 1000 / ns).min(u64::from(fit)) as u32
                };
                r.unit_ns = Some(ns as u32);
                r.fixed_us = Some(fixed.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32);
                r.safe = Some(safe);
            }
        }
        if self.rate60 {
            self.results[0] = r;
            let start = self.best.unwrap_or(0);
            *self = Ramp {
                rate60: false,
                k: start,
                results: self.results,
                overs: self.overs,
                n_over: self.n_over,
                ..Ramp::new(self.base, self.kmax)
            };
        } else {
            self.results[1] = r;
            self.done = true;
        }
    }
}
