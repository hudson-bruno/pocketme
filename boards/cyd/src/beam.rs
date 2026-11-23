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

use send_plan::beam::{Counter, DEFAULT_MAX_INNER_US, anchor_try, calibration};

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
/// The survey's and the anchor's decisions (which reads are trusted,
/// the line time, the checks before a survey is used, the time stamp)
/// are plain logic in `send_plan::beam`, tested on the PC; this file
/// keeps the reads and the waits.
pub use send_plan::beam::{Refused, Survey};

// send_plan::beam counts in this board's cycles and masks with this
// board's counter bits.
const _: () = assert!(send_plan::beam::CPU_MHZ == CPU_MHZ);
const _: () = assert!(send_plan::beam::SCANLINE_MASK == SCANLINE_MASK);

/// Anchor tries when a recent anchor can stand in, and when not.
const ANCHOR_TRIES: u32 = 2;
const ANCHOR_TRIES_COLD: u32 = 6;
/// Wait between two tries (about 6 lines).
const ANCHOR_RETRY_US: u32 = 300;
/// An anchor at most this old may be used again when no try works.
const ANCHOR_REUSE_MS: u32 = 60;

// No libm in no_std: the few float helpers needed.

pub fn floor(x: f32) -> f32 {
    let t = x as i64 as f32;
    if t > x { t - 1.0 } else { t }
}

pub fn spin_us(us: u32) {
    let start = cycles();
    while cycles().wrapping_sub(start) < us * CPU_MHZ {}
}

/// A time in whole microseconds for the log, or `none`.
pub struct LogUs(pub Option<u32>);

impl core::fmt::Display for LogUs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Some(us) => write!(f, "{us}"),
            None => f.write_str("none"),
        }
    }
}

/// Reads 45h back to back for `ms` milliseconds (`send_plan::beam::survey`).
pub fn survey(lcd: &mut Lcd, ms: u32) -> Survey {
    send_plan::beam::survey(|| lcd.scanline_stamped_raw().ok(), cycles, ms)
}

/// The refresh, as a straight line in time, fixed once a frame.
pub struct Beam {
    /// Cycles per counter step (per gate line).
    pub line_cycles: f32,
    /// Lines per refresh (not a whole number: the counter holds a value
    /// through part of the porch).
    pub lines: f32,
    /// Where the counter can be trusted.
    counter: Counter,
    /// Cycles one whole 45h read costs (with the clock switches).
    pub read_cycles: u32,
    pub calibrated: bool,
    /// The longest timed part an anchor read may have, in cycles.
    inner_limit: u32,
    anchor_cycles: u32,
    anchor_line: f32,
    anchored: bool,
    /// Anchor reads that were not used, since start: too slow (over
    /// the limit), bits outside the counter, a value too near the
    /// ends of the range or the held one, the read failed.
    pub misses: [u32; 4],
}

impl Beam {
    pub const fn new() -> Self {
        Self {
            line_cycles: DEFAULT_LINE_US * CPU_MHZ as f32,
            lines: DEFAULT_LINES,
            counter: Counter {
                line_min: DEFAULT_LINE_MIN,
                line_max: DEFAULT_LINE_MAX,
                held_line: u16::MAX,
            },
            read_cycles: 30 * CPU_MHZ,
            calibrated: false,
            inner_limit: DEFAULT_MAX_INNER_US * CPU_MHZ,
            anchor_cycles: 0,
            anchor_line: 0.0,
            anchored: false,
            misses: [0; 4],
        }
    }

    /// Takes line time, lines per refresh and counter range from a
    /// survey, if it looks sane (`send_plan::beam::calibration`);
    /// otherwise says why not and keeps what it had.
    pub fn calibrate(&mut self, s: &Survey) -> Result<(), Refused> {
        let c = calibration(s)?;
        self.inner_limit = c.inner_limit;
        self.line_cycles = c.line_cycles;
        self.lines = c.lines;
        self.counter = c.counter;
        self.read_cycles = c.read_cycles;
        self.calibrated = true;
        self.anchored = false;
        Ok(())
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
    /// the first read's time goes with the second read's answer
    /// (`send_plan::beam::anchor_try` judges the pair). One throwaway
    /// read first brings the read's code back into the flash cache, so
    /// the timed reads are not stretched by cache misses.
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
                self.misses[3] += 1;
                continue;
            };
            let Ok(second) = lcd.scanline_stamped_raw() else {
                self.misses[3] += 1;
                continue;
            };
            match anchor_try((la, lb), second, self.inner_limit, &self.counter) {
                Ok(a) => {
                    self.anchor_cycles = a.cycles;
                    self.anchor_line = f32::from(a.line);
                    self.anchored = true;
                    return true;
                }
                Err(m) => self.misses[m] += 1,
            }
        }
        recent
    }
}
