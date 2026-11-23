//! Where the panel is refreshing, without the TE pin.
//!
//! The panel copies its memory to the glass one gate line at a time,
//! 320 gate lines, about 59 times a second. On this board the TE pin
//! (which would pulse at each refresh) is not wired, but the panel
//! answers Get Scanline (45h) with the number of the line it is at.
//!
//! In landscape (MADCTL 60h) one gate line is one COLUMN of our picture
//! (240 pixels tall), so the refresh sweeps sideways across the picture
//! while we send rows from top to bottom. That is why sending rows
//! always tears: see `tear.rs`.
//!
//! This module:
//!
//! * [`survey`]: reads 45h back to back for a while and measures the
//!   refresh period, its jitter, the time of one line, how many lines a
//!   refresh has, which counter value is held longest (a porch the
//!   counter does not count), and what one read costs;
//! * [`Beam`]: a model built from a survey. One read per frame (the
//!   "anchor") fixes where the refresh is; from there the cycle counter
//!   predicts it for the rest of the frame;
//! * [`characterise`]: the start-up run that prints all of this as
//!   `bench=tear_*` lines, then the sender's own checks (`tear.rs`).
//!
//! Positions are counted in "virtual lines" `v`: `v = 0` is the moment
//! the refresh is at gate line 0 in some pass, `v = 319` at the last
//! gate line, and the pass after starts at `v = lines` (about 334 or
//! 344, porch included). Pass k shows gate line g at `v = k*lines + g`.

use core::fmt;

use log::{info, warn};

use crate::{
    bench, cols,
    lcd::{self, CPU_MHZ, Depth, H, Lcd, SCANLINE_MASK, STRIP_BYTES, Send, Timing, W, cmd, cycles},
    pipeline::{self, Frame, LH, LW, Palette, Rect},
};

// ---- guesses: tune these on the board ---------------------------------

/// The 45h counter value at which the panel refreshes its first gate
/// line. The data sheet says line 0 is the first line of the vertical
/// sync, then come the back porch lines (BPA = 12 after reset), so the
/// guess is 1 + 12 = 13. Not measured: nothing on the board shows which
/// line is on the glass. `tear::SLACK_LINES` covers an error of up to 16
/// lines. Tune: if locked mode still tears only near one edge of the
/// screen, move this by 10 and look again.
pub const FIRST_VISIBLE_LINE: f32 = 13.0;

/// Which way the refresh sweeps across the landscape picture. `false`
/// ("A"): gate line 0 is the left edge (x = 0), the sweep goes left to
/// right. `true` ("B"): right to left. The data sheet suggests A (gate
/// scan 0 to 319, and MADCTL 60h maps x to the gate lines), but the
/// mirror bit's exact effect is in a figure, so it needs one look: the
/// LEFT button flips it at run time; in the right direction the tear
/// pattern stays clean in locked mode.
pub const REVERSED_DEFAULT: bool = false;

/// Line time and lines per refresh used before the first survey: the
/// data sheet's formula (10 MHz / ((320 + 12 + 12) x 490)) with the
/// oscillator 1% slow, which matches the measured 17.01 ms.
const DEFAULT_LINE_US: f32 = 49.45;
const DEFAULT_LINES: f32 = 344.0;
/// Counter range seen on this board (1 dummy bit), used before a survey.
const DEFAULT_LINE_MIN: u16 = 1;
const DEFAULT_LINE_MAX: u16 = 333;

/// How long each survey reads the counter. 200 ms is 11 refreshes at
/// 59 Hz. Longer gives a finer line time.
pub const SURVEY_MS: u32 = 200;
/// Counter values used to fit the line time (first and last value seen
/// in this range in each refresh). Kept away from both ends, where the
/// counter may stop during the porches.
const FIT_LO: u16 = 10;
const FIT_HI: u16 = 320;
/// A read whose inner part (command to last answer byte, 4 bytes at
/// 4 MHz = 8 us) took longer than this was stretched by an interrupt:
/// its time stamp is not trusted. The 8 us are only the bits: two
/// esp-hal transfers and, once a frame, flash cache misses come on top,
/// so 15 us could reject every read. 25 us still puts the stamp (the
/// middle of the read) within 12.5 us = 0.25 line of the truth, well
/// inside `tear::GUARD_LINES`. Check `inner_us_max` and `rejected` in
/// `bench=tear_scan`.
const MAX_INNER_US: u32 = 25;
/// The per-frame anchor read is tried this many times when a try is
/// rejected (stretched, failed, or the counter at an untrusted value)
/// and a recent anchor can stand in (see [`ANCHOR_REUSE_MS`])...
const ANCHOR_TRIES: u32 = 2;
/// ...and this many when there is no recent one (the first frame after
/// `calibrate`, or after a pause). 6 tries 300 us apart span 1.6 ms,
/// longer than the about 1.3 ms the counter spends at untrusted values
/// (held at 1 for about 23.5 lines, plus the edge values).
const ANCHOR_TRIES_COLD: u32 = 6;
/// Wait between two tries (about 6 lines).
const ANCHOR_RETRY_US: u32 = 300;
/// When no try gives a trusted read, an anchor at most this old (in
/// ms, on the cycle counter) is used again instead. The model then runs
/// from it for up to 3.5 refreshes; its line time is off by about 0.1%,
/// a few lines over that span, inside `tear::GUARD_LINES` +
/// `tear::SLACK_LINES`.
const ANCHOR_REUSE_MS: u32 = 60;
/// One throwaway 45h read just before the first try of each anchor.
/// Why: the read's code (`Lcd::scanline_stamped`, `Lcd::command_bytes`,
/// esp-hal's `SpiDma::read`/`write`, 1.7 KB each) runs from flash
/// through the cache; only `pipeline::fill16`/`fill12` are in RAM. In
/// the demo loop other code runs between frames and pushes the read's
/// code out of the cache, so the first read of a frame pays cache misses
/// (a few us each) inside its timed part and goes over
/// [`MAX_INNER_US`]; back to back (the start-up selftest) it stays
/// cached. The throwaway read brings it back. Costs one read, about
/// 22 us a frame. `anchor_first_inner_long` and `anchor_warm_long` in
/// `bench=tear` show whether it works; false switches it off.
pub const ANCHOR_WARM_UP: bool = true;
/// Every scanline value is taken as two 45h reads back to back, and the
/// first read's time goes with the second read's answer. Why: measured
/// on this panel (`bench=tear_reads`, read=single), a 45h read answers
/// with the line at the time of the PREVIOUS 45h read, not its own.
/// After 1.7 ms of pixel writes the single read was 34 lines behind
/// (1.7 ms is 34 lines); after a full frame it was a whole refresh
/// behind; reads one after the other only lag by one read (0.4 line),
/// which is why the surveys looked right. The data sheet does not say
/// this. false gives the old single reads.
pub const READ_TWICE: bool = true;
/// Anchor values must be at least this far from the lowest and highest
/// counter value seen in the survey.
const ANCHOR_EDGE_LINES: u16 = 2;
/// A value the counter holds for more than this many line times is
/// "held": a stretch of lines (a porch) the counter does not count.
const HELD_LINES: f32 = 3.0;
/// Wait after changing a panel setting before measuring: two refreshes
/// at the slowest setting used here.
const SETTLE_MS: u32 = 60;

// ---- small helpers (no libm in no_std) --------------------------------

pub fn floor(x: f32) -> f32 {
    let t = x as i64 as f32;
    if t > x { t - 1.0 } else { t }
}

pub fn round(x: f32) -> f32 {
    floor(x + 0.5)
}

pub fn ceil(x: f32) -> f32 {
    -floor(-x)
}

pub fn abs(x: f32) -> f32 {
    if x < 0.0 { -x } else { x }
}

/// `x` moved by whole `n` into -n/2..n/2.
pub fn wrap_half(x: f32, n: f32) -> f32 {
    x - n * round(x / n)
}

/// A number printed with two decimals, for the log lines.
pub struct Dec(pub f32);

impl fmt::Display for Dec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let v = (self.0 * 100.0) as i64;
        let sign = if v < 0 { "-" } else { "" };
        let a = v.unsigned_abs();
        write!(f, "{sign}{}.{:02}", a / 100, a % 100)
    }
}

fn spin_us(us: u32) {
    let start = cycles();
    while cycles().wrapping_sub(start) < us * CPU_MHZ {}
}

fn wait_ms(ms: u32) {
    spin_us(ms * 1000);
}

fn us(c: u64) -> u64 {
    c / u64::from(CPU_MHZ)
}

// ---- the survey ---------------------------------------------------------

/// What reading the counter back to back for a while showed.
#[derive(Default)]
pub struct Survey {
    pub samples: u32,
    /// Reads that failed or were stretched by an interrupt.
    pub rejected: u32,
    outer_sum: u64,
    outer_min: u32,
    outer_max: u32,
    inner_sum: u64,
    inner_max: u32,
    pub line_min: u16,
    pub line_max: u16,
    pub wraps: u32,
    first_wrap: u32,
    last_wrap: u32,
    gap_min: u32,
    gap_max: u32,
    fit_dt: u64,
    fit_dl: u32,
    fit_refreshes: u32,
    values_sum: u32,
    /// The value held longest, and for how long (cycles, a lower bound:
    /// first to last sample that saw it).
    pub held_line: u16,
    held_cycles: u32,
    /// How many times any value was held more than [`HELD_LINES`].
    long_holds: u32,
}

impl Survey {
    /// Average time from one return to the top to the next, in cycles.
    pub fn period_cycles(&self) -> Option<f32> {
        (self.wraps > 1)
            .then(|| self.last_wrap.wrapping_sub(self.first_wrap) as f32 / (self.wraps - 1) as f32)
    }

    /// Time of one counter step, in cycles, from the fit.
    pub fn line_cycles(&self) -> Option<f32> {
        (self.fit_dl > 0).then(|| self.fit_dt as f32 / self.fit_dl as f32)
    }

    pub fn read_cycles(&self) -> u32 {
        if self.samples == 0 {
            0
        } else {
            (self.outer_sum / u64::from(self.samples)) as u32
        }
    }
}

/// Reads 45h back to back for `ms` milliseconds.
pub fn survey(lcd: &mut Lcd, ms: u32) -> Survey {
    let span = ms * 1000 * CPU_MHZ;
    let mut s = Survey {
        outer_min: u32::MAX,
        gap_min: u32::MAX,
        line_min: u16::MAX,
        held_line: u16::MAX,
        ..Default::default()
    };
    let long = (HELD_LINES * DEFAULT_LINE_US * CPU_MHZ as f32) as u32;
    let mut prev: Option<(u16, u32)> = None;
    // A refresh is measured only from its start (a wrap) to its end.
    let mut in_refresh = false;
    // (line, time stamp) of the first and last sample used for the fit.
    let (mut first, mut last) = (None::<(u16, u32)>, None::<(u16, u32)>);
    let mut changes = 0u32;
    let (mut run_line, mut run_start) = (u16::MAX, 0u32);
    let start = cycles();
    while cycles().wrapping_sub(start) < span {
        let t0 = cycles();
        let Ok((line, a, b)) = lcd.scanline_stamped() else {
            s.rejected += 1;
            continue;
        };
        let outer = cycles().wrapping_sub(t0);
        let inner = b.wrapping_sub(a);
        if inner > MAX_INNER_US * CPU_MHZ {
            s.rejected += 1;
            continue;
        }
        let stamp = a.wrapping_add(inner / 2);
        s.samples += 1;
        s.outer_sum += u64::from(outer);
        s.outer_min = s.outer_min.min(outer);
        s.outer_max = s.outer_max.max(outer);
        s.inner_sum += u64::from(inner);
        s.inner_max = s.inner_max.max(inner);
        s.line_min = s.line_min.min(line);
        s.line_max = s.line_max.max(line);
        if let Some((pl, pt)) = prev {
            if line + 100 < pl {
                // Back at the top.
                if s.wraps == 0 {
                    s.first_wrap = stamp;
                } else {
                    let gap = stamp.wrapping_sub(s.last_wrap);
                    s.gap_min = s.gap_min.min(gap);
                    s.gap_max = s.gap_max.max(gap);
                }
                s.last_wrap = stamp;
                s.wraps += 1;
                if in_refresh
                    && let (Some(f), Some(l)) = (first, last)
                    && l.0 > f.0
                {
                    s.fit_dt += u64::from(l.1.wrapping_sub(f.1));
                    s.fit_dl += u32::from(l.0 - f.0);
                    s.fit_refreshes += 1;
                    s.values_sum += changes + 1;
                }
                in_refresh = true;
                first = None;
                last = None;
                changes = 0;
            } else if line != pl {
                changes += 1;
            }
            if line != run_line {
                let held = pt.wrapping_sub(run_start);
                if held > s.held_cycles {
                    s.held_cycles = held;
                    s.held_line = run_line;
                }
                if held > long {
                    s.long_holds += 1;
                }
                run_line = line;
                run_start = stamp;
            }
        } else {
            run_line = line;
            run_start = stamp;
        }
        if in_refresh && (FIT_LO..=FIT_HI).contains(&line) {
            if first.is_none() {
                first = Some((line, stamp));
            }
            last = Some((line, stamp));
        }
        prev = Some((line, stamp));
    }
    s
}

/// One `bench=tear_scan` line.
fn log_survey(what: &str, frctrl2: u8, bpa: u8, fpa: u8, s: &Survey) {
    let n = u64::from(s.samples.max(1));
    let period = s.period_cycles().unwrap_or(0.0);
    let line = s.line_cycles().unwrap_or(0.0);
    let cpu = CPU_MHZ as f32;
    info!(
        "bench={what} frctrl2=0x{frctrl2:02x} bpa={bpa} fpa={fpa} ms={SURVEY_MS} samples={} rejected={} read_us_avg={} read_us_min={} read_us_max={} inner_us_avg={} inner_us_max={} line_min={} line_max={} wraps={} period_us={} period_min_us={} period_max_us={} jitter_us={} hz={} line_us={} lines_per_refresh={} values_per_refresh={} held_line={} held_us={} held_lines={} long_holds={}",
        s.samples,
        s.rejected,
        us(s.outer_sum / n),
        us(u64::from(if s.samples == 0 { 0 } else { s.outer_min })),
        us(u64::from(s.outer_max)),
        Dec(s.inner_sum as f32 / n as f32 / cpu),
        Dec(s.inner_max as f32 / cpu),
        s.line_min,
        s.line_max,
        s.wraps,
        Dec(period / cpu),
        if s.wraps > 2 {
            us(u64::from(s.gap_min))
        } else {
            0
        },
        if s.wraps > 2 {
            us(u64::from(s.gap_max))
        } else {
            0
        },
        if s.wraps > 2 {
            us(u64::from(s.gap_max - s.gap_min))
        } else {
            0
        },
        Dec(if period > 0.0 {
            1e6 * cpu / period
        } else {
            0.0
        }),
        Dec(line / cpu),
        Dec(if line > 0.0 { period / line } else { 0.0 }),
        s.values_sum.checked_div(s.fit_refreshes).unwrap_or(0),
        s.held_line,
        Dec(s.held_cycles as f32 / cpu),
        Dec(if line > 0.0 {
            s.held_cycles as f32 / line
        } else {
            0.0
        }),
        s.long_holds,
    );
}

// ---- the model ------------------------------------------------------------

/// Result of one anchor read.
#[derive(Default, Clone, Copy)]
pub struct Anchor {
    /// A try gave a trusted read: the model is fixed from it.
    pub ok: bool,
    /// No try did, but a recent anchor is used again (see
    /// [`ANCHOR_REUSE_MS`]).
    pub reused: bool,
    pub tries: u32,
    /// How far the old anchor's prediction was from this read, in lines
    /// (only when the old anchor is at most [`ANCHOR_REUSE_MS`] old: one
    /// to three frames, not a pause).
    pub err_lines: Option<f32>,
    /// Tries rejected because the read failed on the bus...
    pub rej_read: u32,
    /// ...because its timed part took longer than [`MAX_INNER_US`]...
    pub rej_inner: u32,
    /// ...or because the counter was at an untrusted value (an end of
    /// its range or its held value). A try both long and untrusted is
    /// counted as long...
    pub rej_value: u32,
    /// ...or because a bit above the 10 that carry the line was set (a
    /// misread; checked before the value).
    pub rej_bits: u32,
    /// Reads of this anchor (the throwaway one and the tries) with a bit
    /// above the 10 that carry the line set.
    pub bad_bits: u32,
    /// The first try was rejected for a long timed part.
    pub first_inner_long: bool,
    /// Longest timed part of the tries, in cycles.
    pub inner_max: u32,
    /// Timed part of the throwaway read ([`ANCHOR_WARM_UP`]), in cycles.
    pub warm_inner: Option<u32>,
}

impl Anchor {
    /// The model is usable for this frame (own read or a recent one).
    pub fn usable(&self) -> bool {
        self.ok || self.reused
    }

    /// The throwaway read itself would have been rejected as too long.
    pub fn warm_long(&self) -> bool {
        self.warm_inner.is_some_and(|w| w > MAX_INNER_US * CPU_MHZ)
    }
}

/// The refresh, as a straight line in time, fixed once a frame.
pub struct Beam {
    /// Cycles per counter step (per gate line).
    pub line_cycles: f32,
    /// Lines per refresh: period / line time. Not a whole number when
    /// the counter holds a value for a while.
    pub lines: f32,
    /// See [`FIRST_VISIBLE_LINE`].
    pub first_visible: f32,
    /// See [`REVERSED_DEFAULT`].
    pub reversed: bool,
    /// Counter range and held value from the survey: anchor reads at
    /// these values are not trusted.
    line_min: u16,
    line_max: u16,
    held_line: u16,
    /// Cycles one whole 45h read costs (with the clock switches).
    pub read_cycles: u32,
    /// Whether a survey has set the numbers above.
    pub calibrated: bool,
    /// What the start-up check of the columns way's mapping found
    /// (`cols::check`).
    pub orient: cols::Orient,
    anchor_cycles: u32,
    anchor_line: f32,
    anchored: bool,
}

impl Beam {
    pub const fn new() -> Self {
        Self {
            line_cycles: DEFAULT_LINE_US * CPU_MHZ as f32,
            lines: DEFAULT_LINES,
            first_visible: FIRST_VISIBLE_LINE,
            reversed: REVERSED_DEFAULT,
            line_min: DEFAULT_LINE_MIN,
            line_max: DEFAULT_LINE_MAX,
            held_line: u16::MAX,
            read_cycles: 30 * CPU_MHZ,
            calibrated: false,
            orient: cols::Orient::new(),
            anchor_cycles: 0,
            anchor_line: 0.0,
            anchored: false,
        }
    }

    /// Takes line time, lines per refresh and counter range from a
    /// survey, if it looks sane. Returns whether it did.
    pub fn calibrate(&mut self, s: &Survey) -> bool {
        let (Some(period), Some(line)) = (s.period_cycles(), s.line_cycles()) else {
            return false;
        };
        let lines = period / line;
        // 320 gate lines plus 2 to 254 porch lines.
        if !(322.0..=574.0).contains(&lines) {
            return false;
        }
        self.line_cycles = line;
        self.lines = lines;
        self.line_min = s.line_min;
        self.line_max = s.line_max;
        self.held_line = if s.held_cycles as f32 > HELD_LINES * line {
            s.held_line
        } else {
            u16::MAX
        };
        self.read_cycles = s.read_cycles();
        self.calibrated = true;
        self.anchored = false;
        true
    }

    pub fn period_us(&self) -> f32 {
        self.lines * self.line_cycles / CPU_MHZ as f32
    }

    /// The gate line that shows panel column `x` (0..319, landscape).
    pub fn gate(&self, x: u16) -> f32 {
        if self.reversed {
            f32::from(W as u16 - 1 - x)
        } else {
            f32::from(x)
        }
    }

    /// Virtual line (see the module notes) at cycle count `t`, from the
    /// last anchor. `t` may be a little before the anchor.
    pub fn v_at(&self, t: u32) -> f32 {
        self.anchor_line - self.first_visible
            + t.wrapping_sub(self.anchor_cycles) as i32 as f32 / self.line_cycles
    }

    /// The cycle count at which the refresh reaches virtual line `v`.
    pub fn cycles_at(&self, v: f32) -> u32 {
        let lines = v - (self.anchor_line - self.first_visible);
        self.anchor_cycles
            .wrapping_add((lines * self.line_cycles) as i32 as u32)
    }

    /// Whether the counter moves with time at this value (not at an end
    /// of its range, not the value it holds through a porch).
    pub fn trusted(&self, line: u16) -> bool {
        line >= self.line_min + ANCHOR_EDGE_LINES
            && line + ANCHOR_EDGE_LINES <= self.line_max
            && (self.held_line == u16::MAX || line.abs_diff(self.held_line) > 1)
    }

    /// Whether the last anchor is at most [`ANCHOR_REUSE_MS`] old at
    /// cycle count `t`.
    fn recent(&self, t: u32) -> bool {
        self.anchored && t.wrapping_sub(self.anchor_cycles) <= ANCHOR_REUSE_MS * 1000 * CPU_MHZ
    }

    /// Drops the last anchor, so it is not used again. For a pause long
    /// enough for the cycle counter to wrap (17.9 s): then an old anchor
    /// could look recent.
    pub fn forget(&mut self) {
        self.anchored = false;
    }

    /// Cycles from the last anchor to `t`.
    pub fn since_anchor(&self, t: u32) -> u32 {
        t.wrapping_sub(self.anchor_cycles)
    }

    /// One timed read that fixes the model to the panel. Called once a
    /// frame, before sending. A rejected read (failed, stretched, or at
    /// an untrusted counter value: an end of the range or the held
    /// value, where the counter is not moving with time) is tried again
    /// a little later. If no try works, a recent anchor is kept: waiting
    /// out the whole blanking (1.2 ms) for a new one would cost more.
    pub fn anchor(&mut self, lcd: &mut Lcd) -> Anchor {
        let mut out = Anchor::default();
        // The age is taken once, here: the tries take time, and an anchor
        // that is recent now but not after the tries must not lose both
        // the cold tries and the reuse.
        let recent = self.recent(cycles());
        let tries = if recent {
            ANCHOR_TRIES
        } else {
            ANCHOR_TRIES_COLD
        };
        if ANCHOR_WARM_UP && let Ok((raw, a, b)) = lcd.scanline_stamped_raw() {
            out.warm_inner = Some(b.wrapping_sub(a));
            out.bad_bits += u32::from(raw & !SCANLINE_MASK != 0);
        }
        for i in 0..tries {
            if i > 0 {
                spin_us(ANCHOR_RETRY_US);
            }
            out.tries += 1;
            // Two reads back to back (see [`READ_TWICE`]): the answer to
            // the second is the line at the time of the first.
            let latch = if READ_TWICE {
                let Ok((_, a, b)) = lcd.scanline_stamped_raw() else {
                    out.rej_read += 1;
                    continue;
                };
                Some((a, b.wrapping_sub(a)))
            } else {
                None
            };
            let Ok((raw, a, b)) = lcd.scanline_stamped_raw() else {
                out.rej_read += 1;
                continue;
            };
            let line = raw & SCANLINE_MASK;
            let own = b.wrapping_sub(a);
            let (a, inner) = match latch {
                Some((la, li)) => (la, li.max(own)),
                None => (a, own),
            };
            out.inner_max = out.inner_max.max(inner);
            if inner > MAX_INNER_US * CPU_MHZ {
                out.rej_inner += 1;
                out.first_inner_long |= i == 0;
                out.bad_bits += u32::from(raw != line);
                continue;
            }
            if raw != line {
                out.bad_bits += 1;
                out.rej_bits += 1;
                continue;
            }
            if !self.trusted(line) {
                out.rej_value += 1;
                continue;
            }
            let stamp = a.wrapping_add(inner.min(own) / 2);
            if self.recent(stamp) {
                let predicted = self.v_at(stamp) + self.first_visible;
                out.err_lines = Some(wrap_half(f32::from(line) - predicted, self.lines));
            }
            self.anchor_cycles = stamp;
            self.anchor_line = f32::from(line);
            self.anchored = true;
            out.ok = true;
            return out;
        }
        out.reused = recent;
        out
    }

    /// One read for the measured counter (`tear.rs`): the line and its
    /// time stamp, or `None` if the read failed or was stretched.
    pub fn bracket(lcd: &mut Lcd) -> Option<(u16, u32)> {
        Self::bracket_raw(lcd).map(|(raw, t)| (raw & SCANLINE_MASK, t))
    }

    /// The same with the whole 16-bit answer (see
    /// `Lcd::scanline_stamped_raw`).
    pub fn bracket_raw(lcd: &mut Lcd) -> Option<(u16, u32)> {
        if !READ_TWICE {
            return Self::single_raw(lcd);
        }
        // The first read makes the panel latch its line; the second
        // read's answer is that line, so the time stamp is the first's.
        let (_, t) = Self::single_raw(lcd)?;
        let (raw, _) = Self::single_raw(lcd)?;
        Some((raw, t))
    }

    /// One 45h read alone. Its answer is the line at the time of the
    /// 45h read before it (see [`READ_TWICE`]), however long ago.
    pub fn single_raw(lcd: &mut Lcd) -> Option<(u16, u32)> {
        let (raw, a, b) = lcd.scanline_stamped_raw().ok()?;
        let inner = b.wrapping_sub(a);
        (inner <= MAX_INNER_US * CPU_MHZ).then(|| (raw, a.wrapping_add(inner / 2)))
    }

    /// The path of the refresh between two real reads, in virtual lines
    /// (`from` <= `to`). Only the number of whole refreshes in between
    /// is taken from the model (from the time between the reads).
    pub fn measured_path(&self, before: (u16, u32), after: (u16, u32)) -> (f32, f32) {
        let vb = f32::from(before.0) - self.first_visible;
        let va = f32::from(after.0) - self.first_visible;
        let elapsed = after.1.wrapping_sub(before.1) as f32 / self.line_cycles;
        let n = round((vb + elapsed - va) / self.lines);
        (vb, (va + n * self.lines).max(vb))
    }
}

// ---- the start-up run ------------------------------------------------------

/// Is a memory write still right when it is paused for a scanline read
/// and resumed with Write Memory Continue (3Ch)? The panel is covered
/// with one flat colour, then the framebuffer is sent as one memory
/// write cut after every strip, read back and compared.
fn test_continue(lcd: &mut Lcd, fb: &Frame, pal: &Palette, scratch: &mut [u8]) {
    const ROWS: usize = 8;
    let _ = lcd.set_write_mhz(80);
    let _ = lcd.set_depth(Depth::Rgb565);
    let frame_bytes = W * H * 2;
    let _ = lcd.draw(
        (0, 0, W as u16 - 1, H as u16 - 1),
        frame_bytes.div_ceil(STRIP_BYTES),
        Send::Overlap,
        |k, buf| {
            let len = buf.len().min(frame_bytes - k * STRIP_BYTES);
            buf[..len].fill(0xA5);
            len
        },
    );
    let strips = LH / ROWS;
    let mut lines = [0u16; LH / ROWS];
    let mut timing = Timing::default();
    let (mut pause, mut pauses, mut failed) = (0u32, 0u32, false);
    let start = cycles();
    if lcd.begin(0, 0, W as u16 - 1, H as u16 - 1).is_err() {
        failed = true;
    }
    for (k, seen) in lines.iter_mut().enumerate().take(strips) {
        if failed {
            break;
        }
        if k > 0 {
            let t = cycles();
            lcd.end();
            *seen = lcd.scanline_stamped().map_or(0xFFFF, |r| r.0);
            failed |= lcd.continue_write().is_err();
            pause = pause.wrapping_add(cycles().wrapping_sub(t));
            pauses += 1;
        }
        failed |= lcd
            .stream(1, Send::Overlap, &mut timing, |_, buf| {
                pipeline::fill(Depth::Rgb565, fb, pal, k * ROWS, ROWS, 0, LW, buf)
            })
            .is_err();
    }
    lcd.end();
    let total = cycles().wrapping_sub(start);
    let (wrong, first) = bench::verify_fb(lcd, fb, pal, Depth::Rgb565, scratch);
    info!(
        "bench=tear_ramwrc mhz=80 depth=16 strips={strips} reads_between={pauses} send_failed={failed} pixels=76800 wrong={wrong} first_wrong={first:?} pause_us_avg={} frame_us={} lines_read={lines:?}",
        pause.checked_div(pauses).map_or(0, |p| us(u64::from(p))),
        us(u64::from(total)),
    );
}

// ---- is a read right, and does the model hold? -----------------------------

/// What happens between the anchor and the checked read.
#[derive(Clone, Copy)]
enum Activity {
    /// Nothing on the bus for 0.2 to 18 ms.
    Idle,
    /// One full-height slice written, read at once.
    AfterSlice,
    /// One full frame in rows (today's sender), read at once.
    AfterRows,
    /// One slice, then [`READ_TEST_PAUSE_US`] of nothing.
    AfterSliceWait,
}

/// The conditions of [`test_reads`] and their rounds. Fewer rounds for
/// the slow ones (idle about 9 ms a round, rows about 16 ms), so the
/// whole test takes about 5 s and the start-up log still ends inside
/// `board-session.sh`'s listening time.
const READ_TESTS: [(&str, Activity, u32); 4] = [
    ("idle", Activity::Idle, 200),
    ("after_slice", Activity::AfterSlice, 300),
    ("after_rows", Activity::AfterRows, 100),
    ("after_slice_wait", Activity::AfterSliceWait, 300),
];
const READ_TEST_PAUSE_US: u32 = 200;
/// Idle waits are drawn from this range (us).
const READ_TEST_IDLE_US: (u32, u32) = (200, 18_000);
/// Raw samples printed per condition: the first half and the worst half.
const READ_SAMPLES: usize = 8;

/// A small pseudo-random generator (xorshift32): no wanted pattern
/// between the waits or slice positions and the refresh.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
}

#[derive(Clone, Copy, Default)]
struct ReadSample {
    line: u16,
    model: f32,
    us: u32,
    err: f32,
}

/// Counters for one condition. No sample list: only sums, bins and the
/// few raw samples printed.
#[derive(Default)]
struct ReadStats {
    n: u32,
    skipped: u32,
    failed: u32,
    no_anchor: u32,
    /// Reads with a bit above the 10 that carry the line set: a misread,
    /// not compared (not in `skipped`).
    bad_bits: u32,
    /// The same in the anchor calls before each read (throwaway reads and
    /// tries), and the tries rejected for it: when misreads make every
    /// anchor fail, `no_anchor` counts the rounds and these say why.
    anchor_bad_bits: u32,
    anchor_rej_bits: u32,
    err_sum: f32,
    abs_sum: f32,
    abs_max: f32,
    /// |error| < 1, < 2, < 3, < 6, < 12, < 24, >= 24 lines.
    bins: [u32; 7],
    us_sum: u64,
    us_max: u32,
    first: [ReadSample; READ_SAMPLES / 2],
    worst: [ReadSample; READ_SAMPLES / 2],
}

impl ReadStats {
    fn add(&mut self, s: ReadSample) {
        let e = abs(s.err);
        self.err_sum += s.err;
        self.abs_sum += e;
        self.abs_max = self.abs_max.max(e);
        let bin = [1.0, 2.0, 3.0, 6.0, 12.0, 24.0]
            .iter()
            .position(|&b| e < b)
            .unwrap_or(6);
        self.bins[bin] += 1;
        self.us_sum += u64::from(s.us);
        self.us_max = self.us_max.max(s.us);
        let k = self.n as usize;
        self.n += 1;
        if k < self.first.len() {
            self.first[k] = s;
            return;
        }
        // The worst ones after the first few, largest first.
        let filled = (k - self.first.len()).min(self.worst.len());
        let mut at = filled;
        while at > 0 && abs(self.worst[at - 1].err) < e {
            if at < self.worst.len() {
                self.worst[at] = self.worst[at - 1];
            }
            at -= 1;
        }
        if at < self.worst.len() {
            self.worst[at] = s;
        }
    }
}

/// Is a 45h read right when taken just after pixel data, and how well
/// does the straight-line model hold over one frame? For each condition:
/// anchor (again until a fresh read works), the condition's activity,
/// one read, and its counter value against the model's prediction at
/// the read's time stamp. Reads at untrusted counter values are skipped.
/// Prints `bench=tear_reads` and `bench=tear_read_sample` lines. Sends
/// the framebuffer as it is (the test card: nothing on the panel
/// changes). About 5 s.
fn test_reads(lcd: &mut Lcd, fb: &Frame, pal: &Palette, beam: &mut Beam) {
    let _ = lcd.set_write_mhz(80);
    let _ = lcd.set_depth(Depth::Rgb565);
    let slices = (LW as u32) / u32::from(crate::tear::SLICE_LW);
    let mut rng = Rng(0x2545_F491);
    for (name, activity, rounds) in READ_TESTS {
        let mut st = ReadStats::default();
        let mut st2 = ReadStats::default();
        let start = cycles();
        for _ in 0..rounds {
            // A fresh anchor, not a reused one.
            let mut fresh = false;
            for _ in 0..4 {
                let a = beam.anchor(lcd);
                st.anchor_bad_bits += a.bad_bits;
                st.anchor_rej_bits += a.rej_bits;
                if a.ok {
                    fresh = true;
                    break;
                }
            }
            if !fresh {
                st.no_anchor += 1;
                continue;
            }
            let slice = Rect {
                x: (rng.next() % slices) as u16 * crate::tear::SLICE_LW,
                y: 0,
                w: crate::tear::SLICE_LW,
                h: LH as u16,
            };
            match activity {
                Activity::Idle => {
                    let (lo, hi) = READ_TEST_IDLE_US;
                    spin_us(lo + rng.next() % (hi - lo));
                }
                Activity::AfterSlice | Activity::AfterSliceWait => {
                    let _ = pipeline::present_rect(
                        lcd,
                        fb,
                        pal,
                        slice,
                        crate::tear::SLICE_STRIP_ROWS,
                        Send::Overlap,
                    );
                    if matches!(activity, Activity::AfterSliceWait) {
                        spin_us(READ_TEST_PAUSE_US);
                    }
                }
                Activity::AfterRows => {
                    let _ = pipeline::present_full(lcd, fb, pal, 8, Send::Overlap);
                }
            }
            // First one read alone, then a pair: the pair's answer
            // belongs to the time of the single read just before it.
            let single = Beam::single_raw(lcd);
            let pair = Beam::single_raw(lcd)
                .zip(single)
                .map(|((raw, _), (_, t))| (raw, t));
            for (st, read) in [(&mut st, single), (&mut st2, pair)] {
                let Some((raw, stamp)) = read else {
                    st.failed += 1;
                    continue;
                };
                let line = raw & SCANLINE_MASK;
                if raw != line {
                    st.bad_bits += 1;
                    continue;
                }
                if !beam.trusted(line) {
                    st.skipped += 1;
                    continue;
                }
                let predicted = beam.v_at(stamp) + beam.first_visible;
                st.add(ReadSample {
                    line,
                    model: predicted - beam.lines * floor(predicted / beam.lines),
                    us: beam.since_anchor(stamp) / CPU_MHZ,
                    err: wrap_half(f32::from(line) - predicted, beam.lines),
                });
            }
        }
        let ms = us(u64::from(cycles().wrapping_sub(start))) / 1000;
        st2.no_anchor = st.no_anchor;
        for (read, st) in [("single", &st), ("pair", &st2)] {
            let n = st.n.max(1) as f32;
            let b = &st.bins;
            info!(
                "bench=tear_reads cond={name} read={read} rounds={rounds} n={} skipped={} failed={} no_anchor={} err_mean_lines={} err_abs_mean_lines={} err_abs_max_lines={} lt1={} lt2={} lt3={} lt6={} lt12={} lt24={} ge24={} us_since_anchor_avg={} us_since_anchor_max={} ms={ms} bad_bits={} anchor_bad_bits={} anchor_rej_bits={}",
                st.n,
                st.skipped,
                st.failed,
                st.no_anchor,
                Dec(st.err_sum / n),
                Dec(st.abs_sum / n),
                Dec(st.abs_max),
                b[0],
                b[1],
                b[2],
                b[3],
                b[4],
                b[5],
                b[6],
                Dec(st.us_sum as f32 / n),
                st.us_max,
                st.bad_bits,
                st.anchor_bad_bits,
                st.anchor_rej_bits,
            );
            let kept_first = (st.n as usize).min(st.first.len());
            let kept_worst = (st.n as usize)
                .saturating_sub(st.first.len())
                .min(st.worst.len());
            let first = st.first[..kept_first].iter().map(|s| ("first", s));
            let worst = st.worst[..kept_worst].iter().map(|s| ("worst", s));
            for (kind, s) in first.chain(worst) {
                info!(
                    "bench=tear_read_sample cond={name} read={read} line={} model={} us_since_anchor={} err_lines={} kind={kind}",
                    s.line,
                    Dec(s.model),
                    s.us,
                    Dec(s.err),
                );
            }
        }
    }
}

/// The start-up run: the refresh at several settings, the porches, the
/// 3Ch test, then the model used by the sender, the columns way's
/// mapping check, the sender's own checks and the read test. Leaves
/// FRCTRL2 at 0Fh, the porches at their reset values, MADCTL at 60h, and
/// the clock and colour depth as they were. About 10 s.
pub fn characterise(lcd: &mut Lcd, fb: &Frame, pal: &Palette, scratch: &mut [u8], beam: &mut Beam) {
    let (mhz, depth) = (lcd.write_mhz(), lcd.depth());
    info!("tear: characterise start");
    let start = cycles();

    // 1. Refresh period, jitter, line time, lines per refresh, read cost,
    // at the reset rate, the fastest and a slow one. 0Fh last: the
    // model is taken from it.
    let _ = lcd.set_porch(lcd::PORCH_DEFAULT[0], lcd::PORCH_DEFAULT[1]);
    for setting in [0x00u8, 0x1E, 0x0F] {
        if lcd.command(cmd::FRCTRL2, &[setting]).is_err() {
            warn!("tear: FRCTRL2 {setting:02x}h could not be sent");
            continue;
        }
        wait_ms(SETTLE_MS);
        let s = survey(lcd, SURVEY_MS);
        log_survey("tear_scan", setting, 12, 12, &s);
        if setting == 0x0F && !beam.calibrate(&s) {
            warn!(
                "tear: the survey at 0Fh did not give a usable model; keeping the data sheet numbers"
            );
        }
    }

    // 2. Porches: which counter values belong to which porch. Each
    // added porch line should add one line time to the period; whether
    // the counter range grows says whether the counter counts it.
    for (bpa, fpa) in [(0x20u8, 0x0Cu8), (0x0C, 0x20)] {
        if lcd.set_porch(bpa, fpa).is_err() {
            warn!("tear: PORCTRL could not be sent");
            continue;
        }
        wait_ms(SETTLE_MS);
        let s = survey(lcd, SURVEY_MS);
        log_survey("tear_porch", 0x0F, bpa, fpa, &s);
    }
    let _ = lcd.set_porch(lcd::PORCH_DEFAULT[0], lcd::PORCH_DEFAULT[1]);
    wait_ms(SETTLE_MS);

    // 3. A scanline read in the middle of a memory write.
    test_continue(lcd, fb, pal, scratch);

    // 4. The model the sender will use.
    info!(
        "bench=tear_model calibrated={} line_us={} lines_per_refresh={} period_us={} porch_lines={} first_visible_line={} direction={} direction_known=no_needs_eye read_us={}",
        beam.calibrated,
        Dec(beam.line_cycles / CPU_MHZ as f32),
        Dec(beam.lines),
        Dec(beam.period_us()),
        Dec(beam.lines - W as f32),
        Dec(beam.first_visible),
        if beam.reversed {
            "B_right_to_left"
        } else {
            "A_left_to_right"
        },
        us(u64::from(beam.read_cycles)),
    );

    // 5. Which MV=0 MADCTL value the columns way needs (read back, no
    // eyes needed).
    beam.orient = cols::check(lcd, scratch);

    // 6. The sender: predicted waits, real cost of slicing and of the
    // columns way, and every way of sending counted for a few frames.
    crate::tear::characterise(lcd, fb, pal, beam);

    // 7. Is a read right just after pixel data, and does the model hold
    // over one frame?
    test_reads(lcd, fb, pal, beam);

    let _ = lcd.set_write_mhz(mhz);
    let _ = lcd.set_depth(depth);
    info!(
        "tear: characterise done in {} ms",
        us(u64::from(cycles().wrapping_sub(start))) / 1000
    );
}
