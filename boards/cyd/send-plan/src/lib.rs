//! The CYD's screen timing (boards/cyd/src/screen.rs), as plain logic:
//! no pins, no esp crates. The firmware uses it for every changed area it
//! sends; the PC tests and the replay model (feature `model`) use the
//! very same code.
//!
//! The panel refreshes one panel column ("gate line") after the other,
//! left to right, about 49.5 us each, about 343 lines a refresh (320
//! visible). A window is sent column by column too. A window must start
//! when the refresh has just left its first column, and the writer must
//! neither overtake the refresh nor be caught by the next one. The
//! numbers it needs from the board are plain values:
//!
//! - the panel size ([`Panel`]),
//! - the line time, as lines per microsecond (`lines_per_us`; the
//!   firmware gives `CPU_MHZ / line_cycles` from its survey),
//! - the lines per refresh (`lines`, from the survey, not a whole
//!   number),
//! - the write clock in MHz (`wire_mhz`, 80 on the CYD).
//!
//! "Virtual line" v: lines since the first visible line of refresh 0,
//! counting on through later refreshes (v = k * lines + gate line).
//!
//! Moved here from screen.rs without a change in behaviour (same
//! arithmetic, same order of float operations). The new choice between
//! today's windows, column groups and one box is [`choose`].
//!
//! Where `lines_per_us` and `lines` come from (the start-up survey of the
//! panel's counter, its checks, and the anchor read before each present)
//! is [`beam`], moved here from the firmware's beam.rs the same way.
//!
//! No std (except the `model` feature, PC only), no heap, no unsafe.

#![no_std]
#![forbid(unsafe_code)]

#[cfg(feature = "model")]
extern crate std;

pub mod beam;
pub mod choose;

#[cfg(feature = "model")]
pub mod model;

/// Most windows sent apart in one present; more are sent as one full
/// frame. console-video hands at most 16.
pub const MAX_JOBS: usize = 16;

/// Lines kept between the refresh and the writer (display-dma's
/// `COLS_GUARD_LINES`, reasoned there line by line: about 7 needed).
pub const GUARD_LINES: f32 = 8.0;
/// Time from the start of a window to its first pixel byte (window
/// commands, filling the first strip, starting the DMA), and after the
/// last byte. display-dma's guesses, checked on the board there.
pub const LEAD_US: f32 = 35.0;
pub const TAIL_US: f32 = 10.0;
/// The wire is this much slower than its clock says (gaps between
/// bytes and between strips), measured in display-dma.
pub const WIRE_SLOWDOWN: f32 = 1.015;
/// Longest wait for the right moment, in refreshes.
pub const MAX_WAIT_REFRESHES: f32 = 2.0;

/// The panel as the console uses it, in panel pixels (320 x 240 on the
/// CYD).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panel {
    pub w: u16,
    pub h: u16,
}

impl Panel {
    /// The gate line that shows panel column `x` (the sweep goes left to
    /// right on the CYD: direction "A"). Same as `Beam::gate`.
    #[inline]
    pub fn gate(&self, x: u16) -> f32 {
        f32::from(x.min(self.w - 1))
    }
}

/// floor() for f32 (no libm in no_std). Same as `beam::floor`.
#[inline]
pub fn floor(x: f32) -> f32 {
    let t = x as i64 as f32;
    if t > x { t - 1.0 } else { t }
}

/// One window to send: panel columns `x0..x0 + w` (even), rows
/// `y0..y0 + h` (even), and its timing.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Job {
    pub x0: u16,
    pub w: u16,
    pub y0: u16,
    pub h: u16,
    /// First and last gate line.
    pub g0: f32,
    pub g1: f32,
    /// Line times the wire needs per gate line, and from the start of the
    /// window to its first pixel byte.
    pub per_gate: f32,
    pub lead: f32,
    /// Line times for the whole window.
    pub lines: f32,
}

impl Job {
    /// The window for the area `x, y, w, h` (panel pixels, already cut to
    /// the panel). `gate` gives the gate line of a panel column (the
    /// firmware passes `Beam::gate`, the model [`Panel::gate`]).
    #[inline]
    pub fn new(
        (x, y, w, h): (u16, u16, u16, u16),
        panel: Panel,
        gate: impl Fn(u16) -> f32,
        lines_per_us: f32,
        wire_mhz: u32,
    ) -> Job {
        // Widen to whole picture pixels (2x2 panel pixels).
        let x0 = x & !1;
        let y0 = y & !1;
        let x1 = (x + w + 1).min(panel.w) & !1;
        let y1 = (y + h + 1).min(panel.h) & !1;
        let (w, h) = (x1 - x0, y1 - y0);
        let to_lines = lines_per_us;
        let gate_us = f32::from(h) * 2.0 * 8.0 / wire_mhz as f32 * WIRE_SLOWDOWN;
        let per_gate = gate_us * to_lines;
        let lead = LEAD_US * to_lines;
        Job {
            x0,
            w,
            y0,
            h,
            g0: gate(x0),
            g1: gate(x1.saturating_sub(1)),
            per_gate,
            lead,
            lines: lead + f32::from(w) * per_gate + TAIL_US * to_lines,
        }
    }

    /// The start window of this job in virtual lines of pass 0: the first
    /// pixel byte may go out at any v with lo < v < hi (add k * lines for
    /// pass k). Derived in display-dma/src/tear.rs (`cols_window`): the
    /// refresh must have left each column before the writer starts it,
    /// and the writer must be done with it before the next refresh comes.
    #[inline]
    pub fn window(&self, lines: f32) -> (f32, f32) {
        let slope = (self.g1 - self.g0) * (1.0 - self.per_gate);
        let lo = self.g0 + 1.0 + GUARD_LINES + slope.max(0.0);
        let hi = lines + self.g0 - self.per_gate - GUARD_LINES + slope.min(0.0);
        (lo, hi)
    }
}

/// The refresh pass k such that every window, sent one after the other
/// from `v_now` (each waiting if needed), starts inside its start window
/// of pass k. Earliest first; `None` if none works.
pub fn plan(jobs: &[Job], v_now: f32, lines: f32) -> Option<f32> {
    let k0 = floor(v_now / lines);
    for dk in [-1.0, 0.0, 1.0, 2.0] {
        let k = k0 + dk;
        let mut t = v_now;
        let mut ok = true;
        for j in jobs {
            let (lo, hi) = j.window(lines);
            let call = t.max(k * lines + lo - j.lead);
            if call + j.lead > k * lines + hi {
                ok = false;
                break;
            }
            t = call + j.lines;
        }
        if ok {
            return Some(k);
        }
    }
    None
}

/// Did the refresh and the writer meet in a window, by the model? The
/// writer moves evenly from virtual line `vs` (first pixel byte) to `ve`
/// (last) over gate lines g0..=g1 (display-dma `cols_crossing`).
pub fn crossed(vs: f32, ve: f32, g0: f32, g1: f32, lines: f32) -> bool {
    let n = g1 - g0;
    let w = (ve - vs) / (n + 1.0);
    let k = floor(vs / lines);
    let mut j = k - 1.0;
    while j <= k + 4.0 {
        let d0 = vs - (j * lines + g0);
        let d1 = d0 + n * (w - 1.0);
        let (lo, hi) = (d0.min(d1), d0.max(d1));
        if lo < 1.0 && hi > -w {
            return true;
        }
        j += 1.0;
    }
    false
}
