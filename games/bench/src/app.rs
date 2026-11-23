//! The bench's screens and keys, with no console in sight: [`App::update`]
//! takes the keys pressed, the time since the last call and the rate the
//! console runs at, draws, and says which rate it wants.
//!
//! Three modes:
//!
//! * **Fixed**: one test at one load, for ever. LEFT / RIGHT pick the
//!   test, UP / DOWN the load, X switches 30 / 60 calls a second, A
//!   starts the ramp (not for the send tests), START leaves.
//! * **Ramp**: the load goes up by itself ([`crate::ramp`]), first at 60
//!   then at 30. B stops it.
//! * **Result**: what the ramp found, in large digits, until B (back) or
//!   A (once more). Logged every 2 s while shown.
//!
//! The text band (rows 0 to 19) is drawn again only when its text
//! changes: the numbers in it change at most once a second.
//!
//! **Game time.** The console's `delta_us` is measured: the board's
//! clock (read once per step, pauses taken out) minus the clock at the
//! last call. The calls come on a fixed grid (33,333 or 16,667 µs), so
//! each delta wobbles with the wake-up but their sum stays on the grid.
//! The app turns the deltas into ticks of 1/60 s ([`advance`]), counted
//! in half µs with a tick of exactly half the console's frame
//! ([`tick_half_us`]): each call takes the nearest whole number of ticks
//! and keeps all the rest, so a steady 30 gives exactly 2 ticks a call
//! and a steady 60 exactly 1, wobble of up to about 8 ms changes nothing,
//! a late call catches up (at most [`MAX_TICKS`] at once), and when the
//! steps are longer than the frame game time still runs at real speed.
//! The moving tests in the fixed mode take this time; the ramp keeps a
//! fixed advance per call (its steps must see the same pictures however
//! late the calls come). CITY's ramp at 60 moves one tick a call in
//! short runs spread over its cycle ([`crate::city::ramp60_tick`]).

use game_kit::{Frame, W, button};

use crate::art::{BAD, BAND, BAND2, DIM, GOOD, SKY, TEXT};
use crate::city;
use crate::load::{BAND_H, Counts, Kind, TESTS, TICK_US, WORK, WORK_BYTES, Work};
use crate::poly::{Rect, rect_fill};
use crate::ramp::{MIN_FRAMES, Meter, PERIOD_30, PERIOD_60, Ramp, RateResult, Step, WARM, period};
use crate::text::Txt;

/// Tests in the menu.
pub const NTESTS: usize = TESTS.len();
/// Game time per measured window in the fixed mode (µs).
pub const WINDOW_US: u32 = 1_000_000;
/// The level each test starts at (an index into its loads).
const START_LEVEL: u8 = 1;
/// The most ticks one call moves on (250 ms; a longer gap is a stall,
/// not motion to catch up).
pub const MAX_TICKS: u32 = 15;
/// One tick in half µs at the console's rate: exactly half the
/// console's frame, so a steady rate gives whole ticks with nothing
/// left over. At 30 the frame is 33,333 µs, so a tick is 33,333 half µs
/// (16,666.5 µs); at 60 the frame is 16,667 µs, a tick 33,334 half µs.
/// (With one tick of 16,667 µs for both, 30 would leave 1 µs a call
/// behind, and the rest would have to be cut to stay small: that cut
/// lost or added time whenever the steps were longer than the frame.)
pub const fn tick_half_us(rate60: bool) -> u32 {
    if rate60 { 2 * PERIOD_60 } else { PERIOD_30 }
}

/// Turns a call's measured time into ticks of 1/60 s:
/// `rest + 2 * delta` (half µs) rounded to the nearest tick of the
/// console's rate now ([`tick_half_us`]); all of the remainder is kept
/// for the next call (within half a tick by the rounding), so game time
/// keeps with real time whatever the calls' lengths. At most
/// [`MAX_TICKS`] at once (then the rest is dropped: a stall). Returns
/// the ticks and the new rest (half µs).
pub const fn advance(rest: i32, delta_us: u32, rate60: bool) -> (u32, i32) {
    let tick = tick_half_us(rate60) as i32;
    let cap = (MAX_TICKS + 1) * TICK_US;
    let dd = if delta_us > cap { cap } else { delta_us };
    // 2 * dd <= 533,344; the rest is within a tick when it comes from
    // here (saturating for any other value).
    let d = rest.saturating_add(2 * dd as i32);
    if d > MAX_TICKS as i32 * tick {
        return (MAX_TICKS, 0);
    }
    let n = if d <= 0 { 0 } else { (d + tick / 2) / tick };
    (n as u32, d - n * tick)
}

/// What the console hands the app for one call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    /// Buttons pressed since the last call (`game_kit::button`).
    pub pressed: u32,
    /// Game time since the last call (µs).
    pub delta_us: u32,
    /// The first call, the first after a pause, or the picture was lost.
    pub fresh: bool,
    /// The console calls 60 times a second now (else 30).
    pub rate60_now: bool,
}

/// What the app wants after a call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Out {
    pub rate60: bool,
    pub quit: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Fixed,
    Ramp,
    Result,
}

/// The numbers on the screen in the fixed mode, from the last whole
/// window: times between calls, and the workload's counts (averages).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    pub valid: bool,
    pub avg: u32,
    pub worst: u32,
    pub late: u32,
    pub frames: u32,
    pub counts: Counts,
}

pub struct App {
    pub test: usize,
    /// Each test's level, two to a byte (low half: the even test), so
    /// the game value stays as small as it was with ten tests.
    levels: [u8; NTESTS.div_ceil(2)],
    /// The rate chosen for the fixed mode (X).
    pub rate60: bool,
    pub mode: Mode,
    pub ramp: Ramp,
    pub meter: Meter,
    /// The workload's counts added up over the window, and the calls.
    acc: Counts,
    acc_n: u32,
    pub shown: Shown,
    /// Calls since the test (or mode) started.
    pub tick: u32,
    /// Game time since the test (or mode) started, in ticks of 1/60 s.
    pub gt: u32,
    /// The measured time not yet turned into ticks (half µs, see
    /// [`advance`]).
    rest: i32,
    /// The clock has had its first call (that one does not move it).
    clock: bool,
    /// The next call is the first of a test or mode (set by a change or
    /// a lost picture; cleared by the call that drew the background).
    first: bool,
    /// The band as last drawn, and its background (0: not drawn, draw
    /// it again).
    band: [Txt; 3],
    band_bg: u8,
    /// Windows finished in the fixed mode.
    pub windows: u32,
    /// Game time since the result was last logged.
    since_log: u32,
    pub last_step: Option<Step>,
    /// This call's counts (for the tests).
    pub last: Counts,
    /// Lines for the console's log, split by a new line (empty: none).
    pub log: Txt,
    /// CITY's ramp at 60: calls since its step started.
    rc: u32,
    /// The "near panel pace" result line is owed on the next call (it
    /// does not fit in one call's log beside the result line).
    screen_line: bool,
}

impl Default for App {
    fn default() -> Self {
        App::new()
    }
}

impl App {
    pub const fn new() -> App {
        App {
            test: 0,
            levels: [START_LEVEL | START_LEVEL << 4; NTESTS.div_ceil(2)],
            rate60: false,
            mode: Mode::Fixed,
            ramp: Ramp::new(1, 0),
            meter: Meter {
                frames: 0,
                sum: 0,
                worst: 0,
                late: 0,
                extra: 0,
                skip: WARM,
            },
            acc: ZERO,
            acc_n: 0,
            shown: Shown {
                valid: false,
                avg: 0,
                worst: 0,
                late: 0,
                frames: 0,
                counts: ZERO,
            },
            tick: 0,
            gt: 0,
            rest: 0,
            clock: false,
            first: true,
            band: [Txt::new(), Txt::new(), Txt::new()],
            band_bg: 0,
            windows: 0,
            since_log: 0,
            last_step: None,
            last: ZERO,
            log: Txt::new(),
            rc: 0,
            screen_line: false,
        }
    }

    /// The level of test `t` (an index into its loads).
    pub fn level(&self, t: usize) -> u8 {
        (self.levels[t / 2 % self.levels.len()] >> (4 * (t % 2))) & 15
    }

    fn set_level(&mut self, t: usize, lv: u8) {
        let sh = 4 * (t % 2);
        let b = &mut self.levels[t / 2 % NTESTS.div_ceil(2)];
        *b = (*b & !(15 << sh)) | ((lv & 15) << sh);
    }

    /// The load of the fixed level now.
    pub fn load(&self) -> u32 {
        let t = &TESTS[self.test];
        let i = usize::from(self.level(self.test)).min(t.loads.len() - 1);
        t.loads[i]
    }

    /// One call. `w` is the workloads' memory, kept beside the app (not
    /// in it) so that the start value needs no 8 KB constant.
    pub fn update(&mut self, inp: &Input, f: &mut Frame, w: &mut Work) -> Out {
        self.log.clear();
        if self.clock {
            let (n, r) = advance(self.rest, inp.delta_us, inp.rate60_now);
            self.gt = self.gt.wrapping_add(n);
            self.rest = r;
        }
        self.clock = true;
        if inp.fresh {
            self.band_bg = 0;
            self.first = true;
            self.meter.restart(WARM);
            self.ramp.interrupt();
            self.rc = 0;
        }
        let k = inp.pressed;
        if k & button::START != 0 {
            return Out {
                rate60: self.rate60,
                quit: true,
            };
        }
        match self.mode {
            Mode::Fixed => self.fixed(k, inp, f, w),
            Mode::Ramp => self.ramping(k, inp, f, w),
            Mode::Result => self.result(k, inp, f),
        }
        self.draw_band(f, inp.rate60_now);
        self.tick = self.tick.wrapping_add(1);
        Out {
            rate60: if self.mode == Mode::Ramp {
                self.ramp.rate60
            } else {
                self.rate60
            },
            quit: false,
        }
    }

    /// A new test or mode: calls and game time start at 0 (this call
    /// draws time 0).
    fn restart(&mut self) {
        self.tick = 0;
        self.gt = 0;
        self.rest = 0;
        self.first = true;
        self.meter.restart(WARM);
        self.acc = ZERO;
        self.acc_n = 0;
        self.shown.valid = false;
    }

    fn fixed(&mut self, k: u32, inp: &Input, f: &mut Frame, w: &mut Work) {
        let n = TESTS.len();
        let before = (self.test, self.level(self.test), self.rate60);
        if k & button::LEFT != 0 {
            self.test = (self.test + n - 1) % n;
        }
        if k & button::RIGHT != 0 {
            self.test = (self.test + 1) % n;
        }
        let top = (TESTS[self.test].loads.len() - 1) as u8;
        let mut lv = self.level(self.test);
        if k & button::UP != 0 {
            lv = (lv + 1).min(top);
        }
        if k & button::DOWN != 0 {
            lv = lv.saturating_sub(1);
        }
        self.set_level(self.test, lv);
        if k & button::X != 0 {
            self.rate60 = !self.rate60;
        }
        let t = &TESTS[self.test];
        if k & button::A != 0 && t.kmax > 0 {
            self.start_ramp();
            return;
        }
        if (self.test, self.level(self.test), self.rate60) != before {
            self.restart();
            self.log_level();
        }
        let p = period(inp.rate60_now);
        if inp.rate60_now == self.rate60 {
            self.meter.add(inp.delta_us, p, 0);
        } else {
            self.meter.restart(WARM);
        }
        let time = if t.kind.moves() { self.gt } else { self.tick };
        let c = w.run(f, t.kind, self.load(), time, self.first);
        self.first = false;
        self.last = c;
        self.add(c);
        if self.meter.sum >= WINDOW_US && self.meter.frames > 0 {
            let m = self.meter;
            self.shown = Shown {
                valid: true,
                avg: m.avg(),
                worst: m.worst,
                late: m.late,
                frames: m.frames,
                counts: self.average(),
            };
            self.meter.restart(0);
            self.acc = ZERO;
            self.acc_n = 0;
            self.windows = self.windows.wrapping_add(1);
            if self.windows % 4 == 1 {
                self.log_window();
            }
        }
    }

    /// The ramp of the test shown starts (A).
    fn start_ramp(&mut self) {
        let t = &TESTS[self.test];
        self.mode = Mode::Ramp;
        self.ramp = Ramp::new(t.base, t.kmax);
        self.ramp.screen_rule = t.kind == Kind::City;
        self.rc = 0;
        self.restart();
        self.log.s("bench: ramp ").s(t.name).s(" starts");
    }

    fn ramping(&mut self, k: u32, inp: &Input, f: &mut Frame, w: &mut Work) {
        let t = &TESTS[self.test];
        if k & button::B != 0 {
            self.mode = Mode::Fixed;
            self.restart();
            self.log.s("bench: ramp stopped");
            return;
        }
        // A fixed advance per call, not game time, so a step sees the same
        // pictures however late the calls are. CITY at 30: each step one
        // whole cycle, every 16th tick (defect C); at 60 short runs at real
        // speed spread over the cycle, their first calls not counted
        // (`city::ramp60_tick`). The other moving tests 2 ticks a call
        // (their pictures at 30).
        let runs60 = t.kind == Kind::City && self.ramp.rate60;
        let (tick, min_frames) = match t.kind {
            Kind::City if runs60 => {
                let (tick, counted) = city::ramp60_tick(self.rc);
                self.ramp.meter.skip = u32::from(!counted);
                (tick, city::RAMP_CALLS)
            }
            Kind::City => (self.tick.wrapping_mul(city::RAMP_STRIDE), city::RAMP_CALLS),
            k if k.moves() => (self.tick.wrapping_mul(2), MIN_FRAMES),
            _ => (self.tick, MIN_FRAMES),
        };
        let c = w.run(f, t.kind, self.ramp.load(), tick, self.first);
        self.first = false;
        self.last = c;
        let ok = inp.rate60_now == self.ramp.rate60;
        let extra = extra_of(t.kind, &c);
        let step = self.ramp.frame_n(inp.delta_us, extra, ok, min_frames);
        // A new step, or the step started again (a rate not yet taken).
        self.rc = if step.is_some() || !ok {
            0
        } else {
            self.rc.saturating_add(1)
        };
        if let Some(s) = step {
            self.last_step = Some(s);
            self.log_step(&s);
        }
        if self.ramp.done {
            self.mode = Mode::Result;
            self.restart();
            self.since_log = 0;
            // After the last step's line: a second line.
            if !self.log.is_empty() {
                self.log.s("\n");
            }
            self.log_result();
        }
    }

    fn result(&mut self, k: u32, inp: &Input, f: &mut Frame) {
        if k & button::B != 0 {
            self.mode = Mode::Fixed;
            self.restart();
            self.log_level();
            return;
        }
        if k & button::A != 0 {
            self.start_ramp();
            return;
        }
        if self.first {
            self.draw_result(f);
            self.first = false;
        }
        if self.screen_line {
            self.screen_line = false;
            self.log_screen();
        }
        self.since_log = self.since_log.saturating_add(inp.delta_us);
        if self.since_log >= 2 * WINDOW_US {
            self.since_log = 0;
            self.log_result();
        }
    }

    fn add(&mut self, c: Counts) {
        let a = &mut self.acc;
        a.tris = a.tris.saturating_add(c.tris);
        a.px = a.px.saturating_add(c.px);
        a.objs = a.objs.saturating_add(c.objs);
        a.verts = a.verts.saturating_add(c.verts);
        a.check = c.check;
        self.acc_n = self.acc_n.saturating_add(1);
    }

    fn average(&self) -> Counts {
        let n = self.acc_n.max(1);
        Counts {
            tris: self.acc.tris / n,
            px: self.acc.px / n,
            objs: self.acc.objs / n,
            verts: self.acc.verts / n,
            check: self.acc.check,
            ..Counts::default()
        }
    }

    // ---- The text band -----------------------------------------------------

    /// The band's three lines.
    pub fn band_lines(&self, rate60_now: bool) -> [Txt; 3] {
        let t = &TESTS[self.test];
        let mut l = [Txt::new(), Txt::new(), Txt::new()];
        match self.mode {
            Mode::Fixed => {
                l[0].s(t.name).s("  ");
                if t.kind == Kind::Island {
                    let lv = self.load().clamp(1, 5) as usize - 1;
                    l[0].s(crate::island::LEVEL_NAMES[lv]);
                } else {
                    l[0].num(self.load()).s(" ").s(t.unit);
                }
                l[0].s("  AT ").s(if rate60_now { "60" } else { "30" });
                if rate60_now != self.rate60 {
                    l[0].s(" WAIT");
                }
                let s = &self.shown;
                if s.valid {
                    l[1].s("FRAME ")
                        .num(s.avg)
                        .s(" MAX ")
                        .num(s.worst)
                        .s(" LATE ")
                        .num(s.late);
                    counts_line(&mut l[2], t.kind, &s.counts);
                } else {
                    l[1].s("FRAME ...");
                }
            }
            Mode::Ramp => {
                l[0].s("RAMP ")
                    .s(t.name)
                    .s("  AT ")
                    .s(if self.ramp.rate60 { "60" } else { "30" });
                l[1].s("LOAD ").num(self.ramp.load()).s(" ").s(t.unit);
                if let Some(s) = self.last_step {
                    l[2].s("LAST ")
                        .num(s.load)
                        .s(": ")
                        .num(s.avg)
                        .s(" US ")
                        .s(verdict(&s, "FITS", "OVER, NEAR PANEL", "OVER"));
                }
            }
            Mode::Result => {
                l[0].s("RESULT ").s(t.name);
                l[1].s("B BACK  A AGAIN");
            }
        }
        l
    }

    fn draw_band(&mut self, f: &mut Frame, rate60_now: bool) {
        let lines = self.band_lines(rate60_now);
        // The 100 % send level changes the band too, every call.
        let t = &TESTS[self.test];
        let bg = if self.mode == Mode::Fixed
            && t.kind == Kind::Rows
            && self.load() >= 100
            && self.tick & 1 == 1
        {
            BAND2
        } else {
            BAND
        };
        // `bg` is never 0: a band marked as not drawn is drawn again.
        if lines == self.band && bg == self.band_bg {
            return;
        }
        rect_fill(f, &Rect::new(0, 0, W as i32, BAND_H), bg);
        for (i, l) in lines.iter().enumerate() {
            l.at(f, 2, 1 + 6 * i as i16, if i == 0 { TEXT } else { DIM }, 1);
        }
        self.band = lines;
        self.band_bg = bg;
    }

    // ---- The result screen ---------------------------------------------------

    fn draw_result(&self, f: &mut Frame) {
        rect_fill(f, &WORK, SKY);
        let t = &TESTS[self.test];
        for (i, r) in self.ramp.results.iter().enumerate() {
            let y = 22 + 32 * i as i16;
            let mut h = Txt::new();
            h.s(if i == 0 { "AT 60:" } else { "AT 30:" });
            h.at(f, 2, y, DIM, 1);
            let mut a = Txt::new();
            a.s("FIT ").num(r.fit);
            if r.capped {
                a.s("+");
            }
            a.at(f, 2, y + 7, TEXT, 2);
            let mut b = Txt::new();
            b.s("SAFE ");
            match r.safe {
                Some(s) => b.num(s),
                None => b.s("-"),
            };
            b.at(f, 2, y + 19, GOOD, 2);
            let mut c = Txt::new();
            if let (Some(ns), Some(fx)) = (r.unit_ns, r.fixed_us) {
                c.hundredths(ns / 10).s(" US EACH");
                c.at(f, 100, y + 8, DIM, 1);
                let mut d = Txt::new();
                d.s("+").int(fx).s(" US");
                d.at(f, 100, y + 14, DIM, 1);
            }
            if matches!(t.kind, Kind::Scene(_) | Kind::City | Kind::Island) && r.fit > 0 {
                let mut e = Txt::new();
                e.num(r.fit_extra).s(" TRIS");
                e.at(f, 100, y + 22, TEXT, 1);
            }
            if t.kind == Kind::Fill
                && let Some(ns) = r.unit_ns
            {
                let mut e = Txt::new();
                e.num(WORK_BYTES * 1000 / ns.max(1)).s(" BYTES/US");
                e.at(f, 100, y + 22, TEXT, 1);
            }
        }
        let r = &self.ramp.results[0];
        if r.screen > 0 {
            // Plain words: these steps ran at about one panel sweep. That
            // is the panel or a drawing of about 17.0 ms; BENCH cannot
            // tell which, so they counted as over (FIT is strict).
            let mut a = Txt::new();
            a.s("60: ")
                .num(u32::from(r.screen))
                .s(" NEAR PANEL PACE, ")
                .num(r.screen_us)
                .s(" US");
            a.at(f, 2, 89, TEXT, 1);
            let mut b = Txt::new();
            b.s("PANEL OR DRAWING? COUNTED OVER");
            b.at(f, 2, 95, DIM, 1);
            let mut c = Txt::new();
            c.s("FIRST AT LOAD ").num(r.screen_load);
            c.at(f, 2, 101, DIM, 1);
        }
        let mut n = Txt::new();
        n.s("UNIT: ").s(t.unit).s(". SAFE LEAVES 1/5 FREE");
        n.at(
            f,
            2,
            114,
            if self.ramp.results[0].fit == 0 {
                BAD
            } else {
                DIM
            },
            1,
        );
    }

    // ---- The log --------------------------------------------------------------

    fn log_level(&mut self) {
        let t = &TESTS[self.test];
        let load = self.load();
        self.log
            .s("bench: ")
            .s(t.name)
            .s(" load ")
            .num(load)
            .s(" at ")
            .s(if self.rate60 { "60" } else { "30" });
    }

    fn log_window(&mut self) {
        let t = &TESTS[self.test];
        let s = self.shown;
        let load = self.load();
        self.log
            .s("bench: ")
            .s(t.name)
            .s(" load ")
            .num(load)
            .s(" at ")
            .s(if self.rate60 { "60" } else { "30" })
            .s(": frame avg ")
            .num(s.avg)
            .s(" max ")
            .num(s.worst)
            .s(" late ")
            .num(s.late)
            .s(" of ")
            .num(s.frames)
            .s("; tris ")
            .num(s.counts.tris)
            .s(" px ")
            .num(s.counts.px)
            .s(" objs ")
            .num(s.counts.objs);
    }

    fn log_step(&mut self, s: &Step) {
        self.log
            .s("bench: ramp ")
            .s(if s.rate60 { "60" } else { "30" })
            .s(" k ")
            .num(u32::from(s.k))
            .s(" load ")
            .num(s.load)
            .s(": avg ")
            .num(s.avg)
            .s(" max ")
            .num(s.worst)
            .s(" late ")
            .num(s.late)
            .s(" of ")
            .num(s.frames)
            .s(" n ")
            .num(s.extra)
            .s(verdict(s, " fits", " over, near panel pace", " over"));
    }

    fn log_result(&mut self) {
        let t = &TESTS[self.test];
        self.log.s("bench: result ").s(t.name);
        for (i, r) in self.ramp.results.iter().enumerate() {
            let r: &RateResult = r;
            self.log
                .s(if i == 0 { "; 60: fit " } else { "; 30: fit " })
                .num(r.fit)
                .s(if r.capped { "+ n " } else { " n " })
                .num(r.fit_extra)
                .s(" safe ");
            match r.safe {
                Some(s) => self.log.num(s),
                None => self.log.s("-"),
            };
            if let (Some(ns), Some(fx)) = (r.unit_ns, r.fixed_us) {
                self.log.s(" ns ").num(ns).s(" fixed ").int(fx);
            }
        }
        // Only when steps were near panel pace: a line of its own on the
        // next call (the console cuts a game's log line at 96 letters,
        // and this call's log is nearly full).
        self.screen_line = self.ramp.results[0].screen > 0;
    }

    /// CITY's 60 half in plain words when some steps were near panel
    /// pace (counted as over; the panel or the drawing, BENCH cannot tell).
    fn log_screen(&mut self) {
        let t = &TESTS[self.test];
        let r = &self.ramp.results[0];
        self.log
            .s("bench: result ")
            .s(t.name)
            .s(" 60: ")
            .num(u32::from(r.screen))
            .s(" near panel pace (counted over), ")
            .num(r.screen_us)
            .s(" us; first at load ")
            .num(r.screen_load);
    }
}

/// A step's word: fits, over near panel pace (over, at about one panel
/// sweep a call: the panel or the drawing) or over.
fn verdict<'a>(s: &Step, fits: &'a str, screen: &'a str, over: &'a str) -> &'a str {
    if s.fits {
        fits
    } else if s.screen {
        screen
    } else {
        over
    }
}

/// What the ramp keeps per call besides the time: triangles for a
/// scene, pixels changed for the rest.
fn extra_of(kind: Kind, c: &Counts) -> u32 {
    match kind {
        Kind::Scene(_) | Kind::Tri(_) | Kind::Spin | Kind::City | Kind::Island => c.tris,
        Kind::Vert => c.verts,
        _ => c.px,
    }
}

/// The band's third line in the fixed mode.
fn counts_line(l: &mut Txt, kind: Kind, c: &Counts) {
    match kind {
        Kind::Fill => {
            l.s("BYTES ").num(c.px);
        }
        Kind::Tri(_) | Kind::Spin => {
            l.s("TRIS ").num(c.tris).s(" PX ").num(c.px);
        }
        Kind::Vert => {
            l.s("VERTS ").num(c.verts).s(" CHECK ").num(c.check);
        }
        Kind::Scene(_) | Kind::City | Kind::Island => {
            l.s("OBJ ")
                .num(c.objs)
                .s(" TRIS ")
                .num(c.tris)
                .s(" PX ")
                .num(c.px);
        }
        Kind::Rows | Kind::Dots => {
            l.s("CHANGED ").num(c.px).s(" PX");
        }
    }
}

const ZERO: Counts = Counts {
    tris: 0,
    px: 0,
    objs: 0,
    verts: 0,
    faces: 0,
    front: 0,
    clipped: 0,
    dropped: 0,
    check: 0,
};
