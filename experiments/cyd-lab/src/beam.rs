//! Where the panel's refresh is, at any moment: the model behind the
//! tear-free timing.
//!
//! The panel redraws itself from its own memory 58.8 times a second, one
//! panel column ("gate line") after the other, left to right, about
//! 49.5 us each, about 344 lines a refresh (320 visible). Get Scanline
//! (45h) tells which line it is at. Here the refresh is a straight line
//! in time, fixed once a frame by one timed read (the "anchor").
//!
//! A trimmed copy of experiments/display-dma/src/beam.rs. Every number
//! below was measured on this board there (logs in that folder). The
//! surveys at other refresh rates, the porch tests and the read tests
//! were left out.
//!
//! "Virtual line" v: lines since the first visible line of refresh 0,
//! counting on through later refreshes (v = k * lines + gate line).

use crate::lcd::{CPU_MHZ, Lcd, SCANLINE_MASK, W, cycles};

/// The 45h counter value at which the panel refreshes its first visible
/// gate line (1 + the 12 back porch lines after reset).
pub const FIRST_VISIBLE_LINE: f32 = 13.0;

/// Line time and lines per refresh before the first survey (data sheet
/// formula, oscillator 1% slow; matched the measured 17.01 ms).
const DEFAULT_LINE_US: f32 = 49.45;
const DEFAULT_LINES: f32 = 344.0;
/// Counter range seen on this board, used before a survey.
const DEFAULT_LINE_MIN: u16 = 1;
const DEFAULT_LINE_MAX: u16 = 333;

/// How long the survey at start reads the counter (11 refreshes).
pub const SURVEY_MS: u32 = 200;
/// Counter values used to fit the line time, away from both ends.
const FIT_LO: u16 = 10;
const FIT_HI: u16 = 320;
/// A read whose timed part took longer than this was stretched by an
/// interrupt or a cache miss: its time stamp is not trusted.
const MAX_INNER_US: u32 = 25;
/// Anchor tries when a recent anchor can stand in, and when not.
const ANCHOR_TRIES: u32 = 2;
const ANCHOR_TRIES_COLD: u32 = 6;
/// Wait between two tries (about 6 lines).
const ANCHOR_RETRY_US: u32 = 300;
/// An anchor at most this old may be used again when no try works.
const ANCHOR_REUSE_MS: u32 = 60;
/// Anchor values must be at least this far from the ends of the range.
const ANCHOR_EDGE_LINES: u16 = 2;
/// A value the counter holds for longer than this many lines is "held"
/// (a porch the counter does not count).
const HELD_LINES: f32 = 3.0;

// No libm in no_std: the few float helpers needed.

pub fn floor(x: f32) -> f32 {
    let t = x as i64 as f32;
    if t > x { t - 1.0 } else { t }
}

pub fn spin_us(us: u32) {
    let start = cycles();
    while cycles().wrapping_sub(start) < us * CPU_MHZ {}
}

/// What reading the counter back to back for a while showed.
#[derive(Default)]
pub struct Survey {
    pub samples: u32,
    pub rejected: u32,
    /// Why reads were rejected: the read failed, it took too long, or
    /// the answer had bits outside the counter's 10.
    pub rejected_error: u32,
    pub rejected_slow: u32,
    pub rejected_bits: u32,
    /// The shortest timed part of a read, in cycles.
    pub inner_min: u32,
    pub line_min: u16,
    pub line_max: u16,
    pub wraps: u32,
    first_wrap: u32,
    last_wrap: u32,
    fit_dt: u64,
    fit_dl: u32,
    outer_sum: u64,
    held_line: u16,
    held_cycles: u32,
}

impl Survey {
    /// Average time of one refresh, in cycles.
    pub fn period_cycles(&self) -> Option<f32> {
        (self.wraps > 1)
            .then(|| self.last_wrap.wrapping_sub(self.first_wrap) as f32 / (self.wraps - 1) as f32)
    }

    /// Time of one counter step, in cycles.
    pub fn line_cycles(&self) -> Option<f32> {
        (self.fit_dl > 0).then(|| self.fit_dt as f32 / self.fit_dl as f32)
    }

    fn read_cycles(&self) -> u32 {
        (self.outer_sum / u64::from(self.samples.max(1))) as u32
    }
}

/// Reads 45h back to back for `ms` milliseconds.
pub fn survey(lcd: &mut Lcd, ms: u32) -> Survey {
    let span = ms * 1000 * CPU_MHZ;
    let mut s = Survey {
        line_min: u16::MAX,
        held_line: u16::MAX,
        inner_min: u32::MAX,
        ..Default::default()
    };
    let mut prev: Option<(u16, u32)> = None;
    let mut in_refresh = false;
    let (mut first, mut last) = (None::<(u16, u32)>, None::<(u16, u32)>);
    let (mut run_line, mut run_start) = (u16::MAX, 0u32);
    let start = cycles();
    while cycles().wrapping_sub(start) < span {
        let t0 = cycles();
        let Ok((raw, a, b)) = lcd.scanline_stamped_raw() else {
            s.rejected += 1;
            s.rejected_error += 1;
            continue;
        };
        let line = raw & SCANLINE_MASK;
        let outer = cycles().wrapping_sub(t0);
        let inner = b.wrapping_sub(a);
        s.inner_min = s.inner_min.min(inner);
        if inner > MAX_INNER_US * CPU_MHZ || raw != line {
            s.rejected += 1;
            if raw != line {
                s.rejected_bits += 1;
            } else {
                s.rejected_slow += 1;
            }
            continue;
        }
        let stamp = a.wrapping_add(inner / 2);
        s.samples += 1;
        s.outer_sum += u64::from(outer);
        s.line_min = s.line_min.min(line);
        s.line_max = s.line_max.max(line);
        if let Some((pl, pt)) = prev {
            if line + 100 < pl {
                // Back at the top.
                if s.wraps == 0 {
                    s.first_wrap = stamp;
                }
                s.last_wrap = stamp;
                s.wraps += 1;
                if in_refresh
                    && let (Some(f), Some(l)) = (first, last)
                    && l.0 > f.0
                {
                    s.fit_dt += u64::from(l.1.wrapping_sub(f.1));
                    s.fit_dl += u32::from(l.0 - f.0);
                }
                in_refresh = true;
                first = None;
                last = None;
            }
            if line != run_line {
                let held = pt.wrapping_sub(run_start);
                if held > s.held_cycles {
                    s.held_cycles = held;
                    s.held_line = run_line;
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

/// The refresh, as a straight line in time, fixed once a frame.
pub struct Beam {
    /// Cycles per counter step (per gate line).
    pub line_cycles: f32,
    /// Lines per refresh (not a whole number: the counter holds a value
    /// through part of the porch).
    pub lines: f32,
    line_min: u16,
    line_max: u16,
    held_line: u16,
    /// Cycles one whole 45h read costs (with the clock switches).
    pub read_cycles: u32,
    pub calibrated: bool,
    anchor_cycles: u32,
    anchor_line: f32,
    anchored: bool,
}

impl Beam {
    pub const fn new() -> Self {
        Self {
            line_cycles: DEFAULT_LINE_US * CPU_MHZ as f32,
            lines: DEFAULT_LINES,
            line_min: DEFAULT_LINE_MIN,
            line_max: DEFAULT_LINE_MAX,
            held_line: u16::MAX,
            read_cycles: 30 * CPU_MHZ,
            calibrated: false,
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

    /// The gate line that shows panel column `x` (the sweep goes left to
    /// right on this board: direction "A").
    pub fn gate(&self, x: u16) -> f32 {
        f32::from(x.min(W as u16 - 1))
    }

    /// Virtual line at cycle count `t`, from the last anchor.
    pub fn v_at(&self, t: u32) -> f32 {
        self.anchor_line - FIRST_VISIBLE_LINE
            + t.wrapping_sub(self.anchor_cycles) as i32 as f32 / self.line_cycles
    }

    /// The cycle count at which the refresh reaches virtual line `v`.
    pub fn cycles_at(&self, v: f32) -> u32 {
        let lines = v - (self.anchor_line - FIRST_VISIBLE_LINE);
        self.anchor_cycles
            .wrapping_add((lines * self.line_cycles) as i32 as u32)
    }

    /// Whether the counter moves with time at this value.
    fn trusted(&self, line: u16) -> bool {
        line >= self.line_min + ANCHOR_EDGE_LINES
            && line + ANCHOR_EDGE_LINES <= self.line_max
            && (self.held_line == u16::MAX || line.abs_diff(self.held_line) > 1)
    }

    fn recent(&self, t: u32) -> bool {
        self.anchored && t.wrapping_sub(self.anchor_cycles) <= ANCHOR_REUSE_MS * 1000 * CPU_MHZ
    }

    /// Drops the last anchor (after a pause long enough for the cycle
    /// counter to wrap, 17.9 s: an old anchor could then look recent).
    pub fn forget(&mut self) {
        self.anchored = false;
    }

    /// One timed read that fixes the model to the panel, once a frame.
    /// Returns whether the model is usable for this frame (own read, or a
    /// recent anchor used again).
    ///
    /// Each try is two reads back to back: this panel answers a 45h read
    /// with the line at the time of the PREVIOUS 45h read (measured), so
    /// the first read's time goes with the second read's answer. One
    /// throwaway read first brings the read's code back into the flash
    /// cache, so the timed reads are not stretched by cache misses.
    pub fn anchor(&mut self, lcd: &mut Lcd) -> bool {
        let recent = self.recent(cycles());
        let tries = if recent {
            ANCHOR_TRIES
        } else {
            ANCHOR_TRIES_COLD
        };
        let _ = lcd.scanline_stamped_raw();
        for i in 0..tries {
            if i > 0 {
                spin_us(ANCHOR_RETRY_US);
            }
            let Ok((_, la, lb)) = lcd.scanline_stamped_raw() else {
                continue;
            };
            let Ok((raw, a, b)) = lcd.scanline_stamped_raw() else {
                continue;
            };
            let line = raw & SCANLINE_MASK;
            let own = b.wrapping_sub(a);
            let inner = lb.wrapping_sub(la).max(own);
            if inner > MAX_INNER_US * CPU_MHZ || raw != line || !self.trusted(line) {
                continue;
            }
            self.anchor_cycles = la.wrapping_add(inner.min(own) / 2);
            self.anchor_line = f32::from(line);
            self.anchored = true;
            return true;
        }
        recent
    }
}
