//! The replay model (feature `model`, PC only): plays the firmware's
//! `CydScreen::present` and the console's frame loop over recorded frames,
//! in simulated time, and counts what the board counts.
//!
//! What it copies, from the code (boards/cyd/src/screen.rs, beam.rs,
//! app.rs, crates/console-core/src/lib.rs):
//!
//! - the anchor (`Beam::anchor`): one throwaway 45h read, then up to 2
//!   tries (6 when the last good anchor is older than 60 ms) of two reads
//!   each, 300 us apart; a try whose counter value is near the ends of
//!   the counter's range or the held value is not used; with no good try
//!   a recent anchor is used again. The anchor fixes where the code
//!   *believes* the refresh is: the counter value read, a whole number,
//!   so the belief is up to one line behind the truth;
//! - the windows, sorted by first column, one refresh pass for all of
//!   them when [`crate::plan`] finds one, else each window its own
//!   earliest pass (this is [`Baseline`], one [`Planner`]);
//! - the spin to each window's start moment, at most
//!   [`crate::MAX_WAIT_REFRESHES`] refreshes; `late` and `crossed` by the
//!   code's own rules, on the believed refresh position;
//! - each window takes `Job::lines` line times (lead, wire, tail: the
//!   code's own timing model), plus [`BoardTimes::send_extra_us`];
//! - the console's frame loop at a fixed rate: the next step is due one
//!   period after the one before was due, or one period after this
//!   step's start when it is already late (`Console::next_wake`); a late
//!   step starts as soon as the one before is done (app.rs). No frame is
//!   ever skipped: every step runs the game and sends what changed.
//!
//! What it does not know (ESTIMATES, see [`BoardTimes`]): the CPU time
//! of the plan, of the loop between windows and of the board's loop
//! between steps; the exact counter values in the porch; whether the
//! real wire time of a window differs from the code's model.

use std::string::String;
use std::vec::Vec;

use crate::choose::{Rules, Shape, choose_with};
use crate::{Job, MAX_WAIT_REFRESHES, Panel, crossed, plan};

/// A changed area, in panel pixels, as the console hands it to the board.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Area {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

/// The board's numbers. `Default` is the CYD as measured (see each
/// field: MEASURED or ESTIMATE).
#[derive(Clone, Copy, Debug)]
pub struct BoardTimes {
    pub panel: Panel,
    /// MEASURED on the board (boot log `panel:` line, 2026-11-17):
    /// 49.571 us a line, 343.41 lines a refresh (17.02 ms).
    pub line_us: f64,
    pub lines: f32,
    /// The write clock, MHz (80 on the CYD).
    pub wire_mhz: u32,
    /// One whole 45h read with its clock switches, us. MEASURED: the
    /// fastest 17 us (boot log), 22 us on average in display-dma's
    /// survey; 20 is taken.
    pub read_us: f64,
    /// The 45h counter: value at gate line 0 (13), smallest and largest
    /// value seen (1, 333), the value held through the porch (1) and for
    /// how many lines (MEASURED in display-dma at 58.76 Hz: held 23.89
    /// lines at value 1, 321 values a refresh).
    pub first_visible: f32,
    pub line_min: u16,
    pub line_max: u16,
    pub held_line: u16,
    pub held_lines: f32,
    /// CPU time to build and sort the jobs and run the plan, once a
    /// present (ESTIMATE).
    pub plan_us: f64,
    /// CPU time per window between windows (the loop, the late check, the
    /// fallback's own one-window plan) (ESTIMATE).
    pub per_window_cpu_us: f64,
    /// Real window time minus the code's `Job::lines` (0: the code's own
    /// model is the truth; ESTIMATE).
    pub send_extra_us: f64,
    /// Real lead (call to first pixel byte) minus the code's `LEAD_US`
    /// (ESTIMATE; display-dma saw up to 120-165 us more at worst, the
    /// average is not known). Moves the first byte later and adds to the
    /// window's time.
    pub lead_extra_us: f64,
}

impl Default for BoardTimes {
    fn default() -> Self {
        BoardTimes {
            panel: Panel { w: 320, h: 240 },
            line_us: 49.571,
            lines: 343.41,
            wire_mhz: 80,
            read_us: 20.0,
            first_visible: 13.0,
            line_min: 1,
            line_max: 333,
            held_line: 1,
            held_lines: 23.89,
            plan_us: 20.0,
            per_window_cpu_us: 3.0,
            send_extra_us: 0.0,
            lead_extra_us: 0.0,
        }
    }
}

/// Anchor rules (beam.rs).
const ANCHOR_TRIES: u32 = 2;
const ANCHOR_TRIES_COLD: u32 = 6;
const ANCHOR_RETRY_US: f64 = 300.0;
const ANCHOR_REUSE_US: f64 = 60_000.0;
const ANCHOR_EDGE_LINES: u16 = 2;
/// An anchor older than this is forgotten (screen.rs).
const FORGET_US: f64 = 1_000_000.0;

/// What a planner gets to know.
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub panel: Panel,
    /// Lines a refresh, and lines per microsecond.
    pub lines: f32,
    pub lines_per_us: f32,
    pub wire_mhz: u32,
}

impl Ctx {
    /// The job for an area, as the firmware makes it.
    pub fn job(&self, a: Area) -> Job {
        Job::new(
            (a.x, a.y, a.w, a.h),
            self.panel,
            |x| self.panel.gate(x),
            self.lines_per_us,
            self.wire_mhz,
        )
    }
}

/// When to send one window.
#[derive(Clone, Copy, Debug, Default)]
pub struct Slot {
    /// The virtual line (as the code believes it) at which the window's
    /// commands should start; `None`: send at once, untimed.
    pub call_at: Option<f32>,
    /// Latest call that is still in time (for the `late` counter).
    pub latest: Option<f32>,
}

/// A way to send a frame's areas. [`Baseline`] is today's firmware.
///
/// `begin` gets the areas (panel pixels, inside the panel, in the
/// console's order) and the believed refresh position `v_now` right
/// after the anchor (`None`: no usable anchor), and writes the windows
/// to send, in order, into `jobs`. Then, before each window, `slot` says
/// when to start it: a planner that knows all start times at `begin`
/// just hands them out; one that decides as it goes (today's fallback)
/// sees the position then.
pub trait Planner {
    fn name(&self) -> String;
    fn begin(&mut self, areas: &[Area], v_now: Option<f32>, ctx: &Ctx, jobs: &mut Vec<Job>);
    fn slot(&mut self, i: usize, jobs: &[Job], v_now: Option<f32>, ctx: &Ctx) -> Slot;
}

/// Today's firmware (screen.rs `present`): windows sorted by first
/// column; one pass for all if [`plan`] finds one, else each window
/// its own earliest pass, decided just before it.
#[derive(Default)]
pub struct Baseline {
    pass: Option<f32>,
    /// Presents where no single pass fitted.
    pub no_plan: u32,
}

pub use crate::MAX_JOBS;

impl Planner for Baseline {
    fn name(&self) -> String {
        "baseline".into()
    }

    fn begin(&mut self, areas: &[Area], v_now: Option<f32>, ctx: &Ctx, jobs: &mut Vec<Job>) {
        jobs.clear();
        if areas.len() > MAX_JOBS {
            jobs.push(ctx.job(Area {
                x: 0,
                y: 0,
                w: ctx.panel.w,
                h: ctx.panel.h,
            }));
        } else {
            for &a in areas {
                if a.w == 0 || a.h == 0 {
                    continue;
                }
                jobs.push(ctx.job(a));
            }
        }
        jobs.sort_unstable_by_key(|j| j.x0);
        self.pass = v_now.and_then(|v| plan(jobs, v, ctx.lines));
        if v_now.is_some() && self.pass.is_none() {
            self.no_plan += 1;
        }
    }

    fn slot(&mut self, i: usize, jobs: &[Job], v_now: Option<f32>, ctx: &Ctx) -> Slot {
        let job = &jobs[i];
        let k = match (self.pass, v_now) {
            (Some(k), _) => Some(k),
            (None, Some(v)) => plan(core::slice::from_ref(job), v, ctx.lines),
            (None, None) => None,
        };
        match k {
            Some(k) => {
                let (lo, hi) = job.window(ctx.lines);
                Slot {
                    call_at: Some(k * ctx.lines + lo - job.lead),
                    latest: Some(k * ctx.lines + hi - job.lead),
                }
            }
            None => Slot::default(),
        }
    }
}

/// The new rule ([`crate::choose::choose_with`]): today's windows,
/// column groups or one box, whichever the code's model says is done
/// soonest; then one pass for all if there is one, else today's
/// fallback (each window its own pass, decided just before it).
pub struct NewRule {
    pub rules: Rules,
    pass: Option<f32>,
    out: Vec<Job>,
    /// Presents sent as windows / groups / box, and with no one pass.
    pub shapes: [u32; 3],
    pub no_plan: u32,
}

impl NewRule {
    pub fn new(rules: Rules) -> Self {
        NewRule {
            rules,
            pass: None,
            out: std::vec![Job::default(); MAX_JOBS],
            shapes: [0; 3],
            no_plan: 0,
        }
    }
}

impl Planner for NewRule {
    fn name(&self) -> String {
        std::format!("{:?}", self.rules)
    }

    fn begin(&mut self, areas: &[Area], v_now: Option<f32>, ctx: &Ctx, jobs: &mut Vec<Job>) {
        let mut base = Baseline::default();
        base.begin(areas, None, ctx, jobs);
        self.pass = None;
        let Some(v) = v_now else {
            return;
        };
        let c = choose_with(
            jobs,
            |(x, y, w, h)| ctx.job(Area { x, y, w, h }),
            v,
            ctx.lines,
            &mut self.out,
            self.rules,
        );
        self.shapes[c.shape as usize] += 1;
        if c.shape != Shape::Windows {
            jobs.clear();
            jobs.extend_from_slice(&self.out[..c.n]);
        }
        // Planned once more, as the firmware does just before sending.
        self.pass = plan(jobs, v, ctx.lines);
        if self.pass.is_none() {
            self.no_plan += 1;
        }
    }

    fn slot(&mut self, i: usize, jobs: &[Job], v_now: Option<f32>, ctx: &Ctx) -> Slot {
        let job = &jobs[i];
        let k = match (self.pass, v_now) {
            (Some(k), _) => Some(k),
            (None, Some(v)) => plan(core::slice::from_ref(job), v, ctx.lines),
            (None, None) => None,
        };
        match k {
            Some(k) => {
                let (lo, hi) = job.window(ctx.lines);
                Slot {
                    call_at: Some(k * ctx.lines + lo - job.lead),
                    latest: Some(k * ctx.lines + hi - job.lead),
                }
            }
            None => Slot::default(),
        }
    }
}

/// The console's frame loop.
#[derive(Clone, Copy, Debug)]
pub struct LoopTimes {
    /// The game's period: 33,333 or 16,667 us.
    pub period_us: f64,
    /// Step time besides the present (input, the game's update and
    /// drawing, the diff, the overlays), us. From the board's logs:
    /// (step sum - present sum) / frames.
    pub draw_us: f64,
    /// The board loop's own work between two steps (log, settings,
    /// timer), us (ESTIMATE).
    pub loop_us: f64,
}

/// Counters of one run (the board's names where it has them).
#[derive(Clone, Debug, Default)]
pub struct Counters {
    pub frames: u32,
    pub presents: u32,
    pub windows: u32,
    pub untimed: u32,
    pub no_anchor: u32,
    pub anchor_miss: u32,
    pub no_plan: u32,
    pub late: u32,
    pub crossed: u32,
    /// Windows the refresh met by the TRUE position (the code's belief is
    /// up to a line behind; `crossed` uses the belief, as the board).
    pub crossed_true: u32,
    /// Spins cut short by the two-refresh limit.
    pub capped: u32,
    /// Step and present times, us: sums and worst.
    pub step_sum: f64,
    pub step_max: f64,
    pub present_sum: f64,
    pub present_max: f64,
    /// The present's time split, us (sums): the anchor; the plan and the
    /// loop's CPU between windows; waiting (spins); the window commands,
    /// first strip and end (lead + tail); bytes on the wire.
    pub anchor_us: f64,
    pub cpu_us: f64,
    pub spin_us: f64,
    pub overhead_us: f64,
    pub wire_us: f64,
    /// Steps that started later than they were due, and the sum of gaps
    /// between step starts.
    pub late_steps: u32,
    pub gap_sum: f64,
    pub gaps: u32,
    /// Panel pixels sent (after widening to whole picture pixels).
    pub pixels: u64,
}

impl Counters {
    pub fn fps(&self) -> f64 {
        if self.gap_sum > 0.0 {
            f64::from(self.gaps) * 1e6 / self.gap_sum
        } else {
            0.0
        }
    }
    pub fn step_avg_ms(&self) -> f64 {
        self.step_sum / f64::from(self.frames.max(1)) / 1000.0
    }
    pub fn present_avg_ms(&self) -> f64 {
        self.present_sum / f64::from(self.presents.max(1)) / 1000.0
    }
    fn per_present_ms(&self, us: f64) -> f64 {
        us / f64::from(self.presents.max(1)) / 1000.0
    }
    pub fn anchor_ms(&self) -> f64 {
        self.per_present_ms(self.anchor_us)
    }
    pub fn cpu_ms(&self) -> f64 {
        self.per_present_ms(self.cpu_us)
    }
    pub fn spin_ms(&self) -> f64 {
        self.per_present_ms(self.spin_us)
    }
    pub fn overhead_ms(&self) -> f64 {
        self.per_present_ms(self.overhead_us)
    }
    pub fn wire_ms(&self) -> f64 {
        self.per_present_ms(self.wire_us)
    }
    pub fn no_plan_share(&self) -> f64 {
        f64::from(self.no_plan) / f64::from(self.presents.max(1))
    }
    pub fn windows_per_present(&self) -> f64 {
        f64::from(self.windows) / f64::from(self.presents.max(1))
    }
}

/// The refresh as the code believes it after an anchor: the counter
/// value it read (`line`, a whole number) at true time `at_us`.
#[derive(Clone, Copy, Debug)]
struct Anchor {
    at_us: f64,
    line: f32,
    /// How far the truth is ahead of the belief, in lines (0 to 1).
    err: f64,
}

/// One whole run: the board's screen and the console's loop, in
/// simulated time. Feed it one step at a time with [`Sim::step`].
pub struct Sim<P: Planner> {
    pub board: BoardTimes,
    pub frame: LoopTimes,
    pub planner: P,
    /// True refresh position at time 0, in lines (the random phase).
    pub phase: f64,
    pub c: Counters,
    ctx: Ctx,
    /// Simulated time: when the CPU is free again, and the console's
    /// next due time (`Console::next_us`).
    free_us: f64,
    next_us: f64,
    started: bool,
    last_step: Option<f64>,
    last_present: Option<f64>,
    anchor: Option<Anchor>,
    jobs: Vec<Job>,
    /// The last step's start, end and present time.
    pub last: StepTimes,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StepTimes {
    pub start_us: f64,
    pub end_us: f64,
    pub present_us: f64,
}

impl<P: Planner> Sim<P> {
    pub fn new(board: BoardTimes, frame: LoopTimes, planner: P, phase: f64) -> Self {
        let ctx = Ctx {
            panel: board.panel,
            lines: board.lines,
            // As the firmware: CPU_MHZ / line_cycles, in f32.
            lines_per_us: (1.0 / board.line_us) as f32,
            wire_mhz: board.wire_mhz,
        };
        Sim {
            board,
            frame,
            planner,
            phase,
            c: Counters::default(),
            ctx,
            free_us: 0.0,
            next_us: 0.0,
            started: false,
            last_step: None,
            last_present: None,
            anchor: None,
            jobs: Vec::new(),
            last: StepTimes::default(),
        }
    }

    pub fn ctx(&self) -> Ctx {
        self.ctx
    }

    /// When the next step starts (the console's clock for it).
    pub fn next_start(&self) -> f64 {
        if self.started {
            self.free_us.max(self.next_us)
        } else {
            0.0
        }
    }

    /// The console's next due time after the last step.
    pub fn due_us(&self) -> f64 {
        self.next_us
    }

    /// The true refresh position (lines since refresh 0) at time `t`.
    fn true_v(&self, t: f64) -> f64 {
        self.phase + t / self.board.line_us
    }

    /// The 45h answer at time `t` (the value read by a try's first read).
    fn counter(&self, t: f64) -> u16 {
        let lines = f64::from(self.board.lines);
        let m = self.true_v(t).rem_euclid(lines);
        if m >= lines - f64::from(self.board.held_lines) {
            self.board.held_line
        } else {
            (m + f64::from(self.board.first_visible)) as u16
        }
    }

    fn trusted(&self, line: u16) -> bool {
        let b = &self.board;
        line >= b.line_min + ANCHOR_EDGE_LINES
            && line + ANCHOR_EDGE_LINES <= b.line_max
            && line.abs_diff(b.held_line) > 1
    }

    /// The believed virtual line at time `t` (beam.rs `v_at`).
    fn v_at(&self, a: &Anchor, t: f64) -> f32 {
        a.line - self.board.first_visible + ((t - a.at_us) / self.board.line_us) as f32
    }

    /// The time at which the believed position reaches `v` (beam.rs
    /// `cycles_at`).
    fn time_at(&self, a: &Anchor, v: f32) -> f64 {
        a.at_us + f64::from(v - (a.line - self.board.first_visible)) * self.board.line_us
    }

    /// Beam::anchor from time `t`; returns the time after it.
    fn do_anchor(&mut self, mut t: f64) -> (f64, bool) {
        let recent = self.anchor.is_some_and(|a| t - a.at_us <= ANCHOR_REUSE_US);
        let tries = if recent {
            ANCHOR_TRIES
        } else {
            ANCHOR_TRIES_COLD
        };
        t += self.board.read_us; // the throwaway read
        for i in 0..tries {
            if i > 0 {
                t += ANCHOR_RETRY_US;
            }
            // The second read answers with the line at the first read;
            // the anchor time is the middle of the first read (beam.rs).
            let at = t + self.board.read_us / 2.0;
            t += 2.0 * self.board.read_us;
            let line = self.counter(at);
            if !self.trusted(line) {
                self.c.anchor_miss += 1;
                continue;
            }
            let lines = f64::from(self.board.lines);
            let m = self.true_v(at).rem_euclid(lines);
            let err = m + f64::from(self.board.first_visible) - f64::from(line);
            self.anchor = Some(Anchor {
                at_us: at,
                line: f32::from(line),
                err,
            });
            return (t, true);
        }
        (t, recent)
    }

    /// One present starting at `t`; returns its end.
    fn present(&mut self, areas: &[Area], t0: f64) -> f64 {
        if self.last_present.is_none_or(|p| t0 - p > FORGET_US) {
            self.anchor = None;
        }
        self.last_present = Some(t0);
        let mut t = t0;
        let (t1, anchored) = self.do_anchor(t);
        self.c.anchor_us += t1 - t;
        t = t1;
        if !anchored {
            self.c.no_anchor += 1;
        }
        let anchor = if anchored { self.anchor } else { None };
        let v_now = anchor.map(|a| self.v_at(&a, t));
        let mut jobs = std::mem::take(&mut self.jobs);
        let ctx = self.ctx;
        self.planner.begin(areas, v_now, &ctx, &mut jobs);
        t += self.board.plan_us;
        self.c.cpu_us += self.board.plan_us;
        let max_wait = f64::from(MAX_WAIT_REFRESHES * self.board.lines) * self.board.line_us;
        self.c.presents += 1;
        for i in 0..jobs.len() {
            let job = jobs[i];
            if job.w == 0 || job.h == 0 {
                continue;
            }
            let v = anchor.map(|a| self.v_at(&a, t));
            let slot = self.planner.slot(i, &jobs, v, &ctx);
            t += self.board.per_window_cpu_us;
            self.c.cpu_us += self.board.per_window_cpu_us;
            let timed = match (slot.call_at, anchor) {
                (Some(target), Some(a)) => {
                    let w0 = t;
                    let want = self.time_at(&a, target);
                    if want - w0 > max_wait {
                        self.c.capped += 1;
                    }
                    t = t.max(want.min(w0 + max_wait));
                    self.c.spin_us += t - w0;
                    if let Some(latest) = slot.latest
                        && self.v_at(&a, t) > latest
                    {
                        self.c.late += 1;
                    }
                    true
                }
                _ => {
                    self.c.untimed += 1;
                    false
                }
            };
            // The window: lead, wire, tail (the code's model) + extra.
            let lu = self.board.line_us;
            let lead = f64::from(job.lead) * lu + self.board.lead_extra_us;
            let wire = f64::from(f32::from(job.w) * job.per_gate) * lu;
            let total =
                f64::from(job.lines) * lu + self.board.send_extra_us + self.board.lead_extra_us;
            let first = t + lead;
            let last = first + wire;
            self.c.wire_us += wire;
            self.c.overhead_us += total - wire;
            self.c.windows += 1;
            self.c.pixels += u64::from(job.w) * u64::from(job.h);
            if timed && let Some(a) = anchor {
                let (vs, ve) = (self.v_at(&a, first), self.v_at(&a, last));
                if crossed(vs, ve, job.g0, job.g1, self.board.lines) {
                    self.c.crossed += 1;
                }
                let e = a.err as f32;
                if crossed(vs + e, ve + e, job.g0, job.g1, self.board.lines) {
                    self.c.crossed_true += 1;
                }
            }
            t += total;
        }
        self.jobs = jobs;
        t
    }

    /// One console step with these changed areas (empty: nothing to
    /// send). It starts at [`Sim::next_start`]. Returns its times.
    pub fn step(&mut self, areas: &[Area]) -> StepTimes {
        let start = self.next_start();
        if self.started && start > self.next_us + 1.0 {
            self.c.late_steps += 1;
        }
        self.started = true;
        // Console::next_wake, from this step's clock.
        let period = self.frame.period_us;
        if start >= self.next_us {
            self.next_us += period;
            if self.next_us <= start {
                self.next_us = start + period;
            }
        }
        if let Some(prev) = self.last_step {
            self.c.gap_sum += start - prev;
            self.c.gaps += 1;
        }
        self.last_step = Some(start);
        let mut t = start + self.frame.draw_us;
        let mut present_us = 0.0;
        if !areas.is_empty() {
            let end = self.present(areas, t);
            present_us = end - t;
            self.c.present_sum += present_us;
            self.c.present_max = self.c.present_max.max(present_us);
            t = end;
        }
        let step_us = t - start;
        self.c.frames += 1;
        self.c.step_sum += step_us;
        self.c.step_max = self.c.step_max.max(step_us);
        self.free_us = t + self.frame.loop_us;
        self.last = StepTimes {
            start_us: start,
            end_us: t,
            present_us,
        };
        self.last
    }

    /// The counters, with the planner's own (no_plan) filled in by the
    /// caller if it keeps one.
    pub fn counters(&self) -> &Counters {
        &self.c
    }
}

/// One recorded console step: its clock when recorded, and the areas it
/// sent (empty: none).
#[derive(Clone, Debug, Default)]
pub struct Step {
    pub t_us: u64,
    pub areas: Vec<Area>,
}

/// A recorded run (see the trace format in [`parse_trace`]).
#[derive(Clone, Debug, Default)]
pub struct Trace {
    /// `# key value` header lines, in order.
    pub meta: Vec<(String, String)>,
    pub steps: Vec<Step>,
}

impl Trace {
    pub fn meta(&self, key: &str) -> Option<&str> {
        self.meta
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Reads a trace file:
///
/// ```text
/// # send-plan trace 1
/// # game LANTERN            (any "# key value" lines: kept as meta)
/// S <t_us> <n> x,y,w,h x,y,w,h ...
/// ```
///
/// One `S` line per console step, in order: its clock in microseconds
/// when recorded, then the number of areas and each area in panel pixels
/// (`n` is 0 for a step that sent nothing). Other lines starting with
/// `#` are comments.
pub fn parse_trace(text: &str) -> Result<Trace, String> {
    let mut tr = Trace::default();
    for (no, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            let rest = rest.trim();
            if let Some((k, v)) = rest.split_once(' ') {
                tr.meta.push((k.into(), v.trim().into()));
            }
            continue;
        }
        let bad = |what: &str| std::format!("trace line {}: {what}: `{line}`", no + 1);
        let mut it = line.split_whitespace();
        if it.next() != Some("S") {
            return Err(bad("expected `S`"));
        }
        let t_us = it
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| bad("time"))?;
        let n: usize = it
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| bad("count"))?;
        let mut areas = Vec::with_capacity(n);
        for part in it {
            let v: Vec<u16> = part.split(',').filter_map(|s| s.parse().ok()).collect();
            if v.len() != 4 {
                return Err(bad("area"));
            }
            areas.push(Area {
                x: v[0],
                y: v[1],
                w: v[2],
                h: v[3],
            });
        }
        if areas.len() != n {
            return Err(bad("area count"));
        }
        tr.steps.push(Step { t_us, areas });
    }
    Ok(tr)
}

/// One `S` line of a trace.
pub fn trace_line(t_us: u64, areas: &[Area]) -> String {
    use std::fmt::Write;
    let mut s = std::format!("S {t_us} {}", areas.len());
    for a in areas {
        let _ = write!(s, " {},{},{},{}", a.x, a.y, a.w, a.h);
    }
    s
}

/// Replays a trace's steps in order (open loop: the recorded content,
/// the model's own timing) with a fresh planner, at refresh phase
/// `phase` (lines). Returns the counters.
pub fn replay<P: Planner>(
    steps: &[Step],
    board: BoardTimes,
    frame: LoopTimes,
    planner: P,
    phase: f64,
) -> Sim<P> {
    let mut sim = Sim::new(board, frame, planner, phase);
    for s in steps {
        sim.step(&s.areas);
    }
    sim
}

/// A small fixed random sequence (xorshift), so runs repeat exactly.
pub struct Rng(pub u64);

impl Rng {
    pub fn next_f64(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}
