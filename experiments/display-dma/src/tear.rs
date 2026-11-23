//! Sending without tearing: vertical slices, in the order the panel
//! refreshes, timed from the scanline.
//!
//! Why rows always tear. In landscape the panel refreshes one picture
//! COLUMN at a time, sweeping sideways (about 50 us a column, 15.8 ms for
//! all 320), then rests for about 1 ms (the porches). We send ROWS, top
//! to bottom: every row touches all 320 columns. A tear-free write must
//! not meet the sweep, so a full frame in rows would have to fit inside
//! the 1 ms rest. It takes 11.7 to 31 ms. So every refresh that overlaps
//! a write shows part old and part new picture, split along a slanted
//! line: the tear the owner saw.
//!
//! What this module does instead. The frame is cut into slices of
//! [`SLICE_LW`] logical columns (32 panel columns), each sent as its own
//! window, rows top to bottom inside it, in the order the sweep visits
//! them. A slice is started only after the sweep of refresh k has left
//! it, and must end before the sweep of refresh k+1 comes back to it.
//! Then refresh k shows the whole old picture and refresh k+1 the whole
//! new one. At 80 MHz a slice takes about 1.6 ms, and the sweep crosses
//! 32 columns in about 1.6 ms too, so the writer simply follows the
//! sweep. Changed rectangles are cut and scheduled the same way.
//!
//! Four ways of sending, to compare ([`Way`]):
//!
//! * `rows`: today's way, free running (the old sender, unchanged);
//! * `slices_free`: slices in sweep order, but sent at once with no
//!   timing: shows how much the order alone helps;
//! * `slices_locked`: slices timed from the scanline: the new way;
//! * `columns`: the picture sent transposed under an MV=0 MADCTL value,
//!   each changed rectangle as ONE window whose write runs along the
//!   sweep (`cols.rs`), timed from the scanline with its own rule
//!   ([`cols_window`]). Not yet run on the board.
//!
//! Every way is counted the same way ([`Stats`]):
//!
//! * predicted: each piece's start and end times, put on the model of
//!   the sweep (`beam.rs`), say whether the sweep passed over the
//!   piece's columns while it was being written (a "crossing"). Free:
//!   arithmetic only. A frame is torn if any piece crossed, or if its
//!   pieces would first be shown in different refreshes ("split");
//! * measured (RIGHT button): a real scanline read just before and just
//!   after each piece. Only the mapping from counter value to column is
//!   assumed. Costs one or two reads (24 to 83 us each) per piece. For
//!   the columns way a split counts here too.
//!
//! For the columns way both counters take the writer from its first
//! pixel byte to the end of its last transfer (`Timing::first_byte`,
//! `last_byte`), not the call's start and end ([`cols_crossing`]).

use log::{info, warn};

use crate::{
    beam::{self, Beam, Dec, abs, ceil, floor, round},
    cols,
    lcd::{CPU_MHZ, Depth, Lcd, LcdError, Send, W, cycles},
    pipeline::{self, Frame, LH, LW, Palette, Rect},
};

// ---- guesses: tune these on the board ---------------------------------

/// Slice width in logical pixels (twice that in panel columns). Narrower
/// slices give a wider start window but cost one more piece's overhead
/// ([`JOB_OVERHEAD_US`], measured 118 us) each. 16 makes a full slice exactly one strip buffer.
/// Must be a multiple of 4 (12-bit colour sends 4 logical pixels at a
/// time).
pub const SLICE_LW: u16 = 16;

/// Strip size for slices, in full-width logical rows (see
/// `pipeline::rows_per_strip`): 6 makes a 16-wide slice two strips of 60
/// rows, so the second is filled while the first is on the wire.
pub const SLICE_STRIP_ROWS: usize = 6;

/// Most pieces in one frame. Changed rectangles that would need more
/// slices than this are sent as a whole sliced frame instead (10 pieces).
pub const MAX_JOBS: usize = 24;

/// Time of one piece beyond its bytes on the wire: window commands
/// (about 20 us), filling the first strip before DMA can start, starting
/// and ending the transfers. Measured on the board: 118 us
/// (`per_job_overhead_us` in `bench=tear_slices`, 80 MHz 16-bit).
pub const JOB_OVERHEAD_US: f32 = 118.0;

/// The wire runs a little slower than bytes x 8 / MHz (gaps between
/// transfers): full frames measured 1.4% over at 80 MHz.
pub const WIRE_SLOWDOWN: f32 = 1.015;

/// Lines kept between the sweep and a piece, for errors of the model:
/// the anchor is rounded to a whole line, the line time is off by about
/// 0.1%, the panel may fetch a line a little before it shows it.
pub const GUARD_LINES: f32 = 4.0;

/// More lines kept on both sides because `beam::FIRST_VISIBLE_LINE` is a
/// guess (one line = one panel column = about 50 us). Once the eye test
/// confirms it, this can go down to 0.
pub const SLACK_LINES: f32 = 16.0;

/// Longest a piece waits for the sweep, in refreshes. Only a model error
/// can make the plan ask for more; then the piece goes anyway.
const MAX_WAIT_REFRESHES: f32 = 2.0;

/// Frames sent per way in the start-up check (back to back, the test
/// card).
const SELFTEST_FRAMES: u32 = 30;

/// Lines kept between the sweep and the columns way's writer (see
/// [`cols_window`]). Its own number, not [`GUARD_LINES`] +
/// [`SLACK_LINES`]: the anchor rounded to a whole line (1), the line
/// time off by about 0.1% over up to 3 refreshes (1), the writer's speed
/// ([`WIRE_SLOWDOWN`] is good to about 0.5%: 1.6 lines over a full
/// frame), the start (window commands and the first strip, an interrupt:
/// 1), the panel fetching a line a little before showing it (1), and the
/// first visible line, which the counter confirmed (1 + back porch = 13)
/// but no eye has (1 or 2). About 7 in all. The window is wide (about
/// 320 of 344 lines for a full frame at 80/16), so more costs little.
pub const COLS_GUARD_LINES: f32 = 8.0;

/// Columns way: time from the call to the first pixel byte on the wire:
/// MADCTL check, CASET, RASET, RAMWR through the FIFO (about 18 to 20 us,
/// `bench=rect`), filling the first strip (one logical column, about
/// 9 us) and starting the DMA. A guess; `bench=tear_cols` prints the
/// window commands' real time (`cols_window_us`) and `bench=tear` the
/// longest real lead (`cols_lead_us_max`). Used only to plan: the
/// counters take the real first and last byte from `Timing`.
pub const COLS_LEAD_US: f32 = 35.0;

/// Columns way: time after the last byte until the call returns (the
/// last DMA wait, CS high). A guess (`cols_tail_us_max` in `bench=tear`).
pub const COLS_TAIL_US: f32 = 10.0;

// ---- ways of sending ----------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Way {
    Rows,
    SlicesFree,
    SlicesLocked,
    Columns,
}

/// How many ways there are.
const WAYS: usize = 4;

impl Way {
    /// The UP button's order: slices_locked (at start) -> rows ->
    /// slices_free -> columns -> slices_locked. The first three as before
    /// the columns way existed (one press still gives rows, so earlier
    /// sessions can be run again); columns is three presses
    /// (`board-session.sh ... up*3`).
    pub fn next(self) -> Way {
        match self {
            Way::SlicesLocked => Way::Rows,
            Way::Rows => Way::SlicesFree,
            Way::SlicesFree => Way::Columns,
            Way::Columns => Way::SlicesLocked,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Way::Rows => "rows",
            Way::SlicesFree => "slices_free",
            Way::SlicesLocked => "slices_locked",
            Way::Columns => "columns",
        }
    }

    /// The names in the UP button's order, for the log.
    pub const CYCLE: &'static str = "slices_locked,rows,slices_free,columns";

    fn index(self) -> usize {
        self as usize
    }
}

/// One piece to send: a window, the gate lines it covers, and how many
/// lines the sweep moves while it is written.
#[derive(Clone, Copy, Default)]
struct Job {
    rect: Rect,
    g0: f32,
    g1: f32,
    lines: f32,
    /// Columns way only (0 for the others): line times the wire needs
    /// per gate line, and from the end of the wait to the first pixel
    /// byte.
    per_gate: f32,
    lead: f32,
}

impl Job {
    fn new(rect: Rect, beam: &Beam, depth: Depth, mhz: u32, extra_us: f32) -> Job {
        let (x0, _, x1, _) = rect.panel_window();
        let (a, b) = (beam.gate(x0), beam.gate(x1));
        let pixels = 4 * usize::from(rect.w) * usize::from(rect.h);
        let us = depth.bytes(pixels) as f32 * 8.0 / mhz as f32 * WIRE_SLOWDOWN
            + JOB_OVERHEAD_US
            + extra_us;
        Job {
            rect,
            g0: a.min(b),
            g1: a.max(b),
            lines: us * CPU_MHZ as f32 / beam.line_cycles,
            per_gate: 0.0,
            lead: 0.0,
        }
    }

    /// A columns-way window. `read_us` is one bracket read in measure
    /// mode (0 otherwise): one before the window (it delays the first
    /// byte) and one after.
    fn cols(rect: Rect, beam: &Beam, depth: Depth, mhz: u32, read_us: f32) -> Job {
        let rect = cols::written(rect, depth);
        let (x0, _, x1, _) = rect.panel_window();
        let (a, b) = (beam.gate(x0), beam.gate(x1));
        let to_lines = CPU_MHZ as f32 / beam.line_cycles;
        // The per-strip gaps are inside WIRE_SLOWDOWN (measured with
        // strips of 10 KB; these are up to 15 KB).
        let gate_us = pipeline::col_bytes(depth, usize::from(rect.h)) as f32 * 8.0 / mhz as f32
            * WIRE_SLOWDOWN;
        let gates = 2.0 * f32::from(rect.w);
        let lead = (COLS_LEAD_US + read_us) * to_lines;
        let per_gate = gate_us * to_lines;
        let lines = lead + gates * per_gate + (COLS_TAIL_US + read_us) * to_lines;
        Job {
            rect,
            g0: a.min(b),
            g1: a.max(b),
            lines,
            per_gate,
            lead,
        }
    }
}

/// The start window of a columns-way window, in virtual lines of pass 0:
/// the first pixel byte may go out at any `v0` with `lo < v0 < hi`
/// (add `k * lines` for pass k). Then pass k shows the whole old
/// picture there and pass k+1 the whole new one.
///
/// Derivation. The window covers gate lines g0..=g1, N = g1 - g0 (N + 1
/// lines), written in that order (`cols.rs` sends them along the sweep in
/// either direction). The writer takes `w` line times a gate line
/// (`per_gate`), so it writes gate line g0 + i from v0 + i*w to
/// v0 + (i+1)*w. Sweep k shows that line from k*L + g0 + i to one line
/// later. Line i is clean if sweep k has left it before the writer
/// starts it, and the writer is done before sweep k+1 comes:
///
///   k*L + g0 + i + 1 + guard  <=  v0 + i*w
///   v0 + (i+1)*w + guard       <=  (k+1)*L + g0 + i
///
/// that is, for every i in 0..=N:
///
///   v0 >= k*L + g0 + 1 + guard + i*(1 - w)
///   v0 <= (k+1)*L + g0 - w - guard + i*(1 - w)
///
/// i*(1 - w) is largest at i = N when w < 1 (the writer is faster than
/// the sweep and catches up with sweep k) and at i = 0 otherwise; it is
/// smallest the other way round (a slower writer is caught by sweep
/// k+1). So:
///
///   k*L + g0 + 1 + guard + max(0, N*(1 - w))  <=  v0
///   v0  <=  (k+1)*L + g0 - w - guard + min(0, N*(1 - w))
///
/// The brief's form without the two one-line terms (+1: the sweep needs
/// one line time to leave a line; -w: the writer needs one to finish
/// one). At 80 MHz 16-bit a full frame has w = 0.98 and N = 319: start
/// from about 14 to 335 lines (0.7 to 16.6 ms) after the sweep passes
/// gate line 0.
fn cols_window(job: &Job, lines: f32) -> (f32, f32) {
    let guard = COLS_GUARD_LINES;
    let slope = (job.g1 - job.g0) * (1.0 - job.per_gate);
    let lo = job.g0 + 1.0 + guard + slope.max(0.0);
    let hi = lines + job.g0 - job.per_gate - guard + slope.min(0.0);
    (lo, hi)
}

struct Jobs {
    list: [Job; MAX_JOBS],
    n: usize,
}

impl Jobs {
    fn push(&mut self, job: Job) -> bool {
        if self.n == MAX_JOBS {
            return false;
        }
        self.list[self.n] = job;
        self.n += 1;
        true
    }

    fn as_slice(&self) -> &[Job] {
        &self.list[..self.n]
    }

    /// Cuts `rect` at every multiple of [`SLICE_LW`].
    fn cut(&mut self, rect: Rect, beam: &Beam, depth: Depth, mhz: u32, extra_us: f32) -> bool {
        let (mut x, end) = (rect.x, rect.x + rect.w);
        while x < end {
            let x1 = ((x / SLICE_LW + 1) * SLICE_LW).min(end);
            let piece = Rect {
                x,
                y: rect.y,
                w: x1 - x,
                h: rect.h,
            };
            if !self.push(Job::new(piece, beam, depth, mhz, extra_us)) {
                return false;
            }
            x = x1;
        }
        true
    }
}

/// The pieces for `rects`: as they are for `rows`, else cut into slices
/// and sorted in sweep order.
fn build(rects: &[Rect], way: Way, beam: &Beam, depth: Depth, mhz: u32, extra_us: f32) -> Jobs {
    let mut jobs = Jobs {
        list: [Job::default(); MAX_JOBS],
        n: 0,
    };
    if way == Way::Rows {
        for r in rects.iter().take(MAX_JOBS) {
            // The same rectangle present_rect sends (12-bit widens it),
            // so the counted columns are the written ones.
            let r = if depth == Depth::Rgb444 {
                r.to_quads()
            } else {
                *r
            };
            jobs.push(Job::new(r, beam, depth, mhz, extra_us));
        }
        return jobs;
    }
    let mut fits = true;
    for r in rects {
        let r = if depth == Depth::Rgb444 {
            r.to_quads()
        } else {
            *r
        };
        if !jobs.cut(r, beam, depth, mhz, extra_us) {
            fits = false;
            break;
        }
    }
    if !fits {
        jobs.n = 0;
        jobs.cut(Rect::FULL, beam, depth, mhz, extra_us);
    }
    // Insertion sort by first gate line: at most MAX_JOBS pieces.
    for i in 1..jobs.n {
        let mut j = i;
        while j > 0 && jobs.list[j - 1].g0 > jobs.list[j].g0 {
            jobs.list.swap(j - 1, j);
            j -= 1;
        }
    }
    jobs
}

/// The columns way's windows for `rects`: one per rectangle (as many as
/// [`MAX_JOBS`], else one full frame), sorted by first gate line.
fn build_cols(rects: &[Rect], beam: &Beam, depth: Depth, mhz: u32, read_us: f32) -> Jobs {
    let mut jobs = Jobs {
        list: [Job::default(); MAX_JOBS],
        n: 0,
    };
    if rects.len() > MAX_JOBS {
        jobs.push(Job::cols(Rect::FULL, beam, depth, mhz, read_us));
        return jobs;
    }
    for r in rects {
        jobs.push(Job::cols(*r, beam, depth, mhz, read_us));
    }
    for i in 1..jobs.n {
        let mut j = i;
        while j > 0 && jobs.list[j - 1].g0 > jobs.list[j].g0 {
            jobs.list.swap(j - 1, j);
            j -= 1;
        }
    }
    jobs
}

/// The sibling of [`plan`] for the columns way: the pass `k` such that
/// every window, sent one after the other from `v_now` in order of first
/// gate line (each waiting if needed), starts its first pixel byte
/// inside its start window ([`cols_window`]) of pass k. All in one pass,
/// so all are first shown by pass k+1 (no split). Earliest pass first.
/// Returns `k` and the predicted end; `None` if no pass works.
fn plan_cols(jobs: &[Job], v_now: f32, lines: f32) -> Option<(f32, f32)> {
    let k0 = floor(v_now / lines);
    for dk in [-1.0, 0.0, 1.0, 2.0] {
        let k = k0 + dk;
        let mut t = v_now;
        let mut ok = true;
        for j in jobs {
            let (lo, hi) = cols_window(j, lines);
            let call = t.max(k * lines + lo - j.lead);
            if call + j.lead > k * lines + hi {
                ok = false;
                break;
            }
            t = call + j.lines;
        }
        if ok {
            return Some((k, t));
        }
    }
    None
}

/// What [`cols_crossing`] found for one window.
#[derive(Clone, Copy)]
struct ColsCross {
    crossed: bool,
    /// The writer ran into a sweep that was ahead of it (it started too
    /// soon after the sweep, or is faster); else, when crossed, a sweep
    /// ran into the writer (it started too late, or is slower).
    writer_first: bool,
    /// When not crossed: how close a sweep came, in lines.
    gap: f32,
    /// When not crossed: the first pass that shows the whole window.
    shown: i32,
}

/// Did a sweep and the writer meet anywhere in a columns-way window?
/// The writer is taken to move evenly from virtual line `vs` (the first
/// pixel byte, `Timing::first_byte`) to `ve` (the end of the last
/// transfer, `Timing::last_byte`) over gate lines g0..=g1. The strip
/// gaps make it not quite even (a few us at each strip start); that is
/// assumed away.
///
/// With w = (ve - vs) / (N + 1), the writer writes line i from vs + i*w
/// to vs + (i+1)*w and sweep j shows it from j*L + g0 + i to one line
/// later; they meet if D = vs + i*w - (j*L + g0 + i) is in (-w, 1). D is
/// a straight line in i, so its ends (i = 0 and i = N) say whether it
/// enters that range.
///
/// `early` widens the range on the writer-ahead side, for real reads: a
/// counter value c means the sweep is somewhere in line c..c+1, so the
/// virtual line taken from the reads is up to one line behind the true
/// one, and the true D is D + (0 to 1). Then they may have met if D is in
/// (-w - 1, 1): `early` = 1 for the measured counter, 0 for the model.
fn cols_crossing(vs: f32, ve: f32, g0: f32, g1: f32, lines: f32, early: f32) -> ColsCross {
    let n = g1 - g0;
    let w = (ve - vs) / (n + 1.0);
    let k = floor(vs / lines);
    let (low, high) = (-w - early, 1.0);
    let mut out = ColsCross {
        crossed: false,
        writer_first: false,
        gap: f32::MAX,
        shown: 0,
    };
    // Sweeps k-1 to k+4: the window is at most 3.5 refreshes long here.
    let mut j = k - 1.0;
    while j <= k + 4.0 {
        let d0 = vs - (j * lines + g0);
        let d1 = d0 + n * (w - 1.0);
        let (lo, hi) = (d0.min(d1), d0.max(d1));
        if lo < high && hi > low {
            out.crossed = true;
            // Which side of the start window was missed: the writer was
            // behind sweep j at the start (d0 >= high) and caught it, or
            // ahead (d0 <= low) and was caught; a start on the sweep's
            // line counts to the nearer side.
            out.writer_first = d0 >= high || (d0 > low && high - d0 < d0 - low);
            return out;
        }
        out.gap = out.gap.min(if lo >= high { lo - high } else { low - hi });
        j += 1.0;
    }
    // The first sweep to reach line 0 after the writer finished it; with
    // no crossing every line shares it. With `early`, no sweep has D in
    // (-w - 1, -w] here, so the reads' offset of 0 to 1 line cannot move
    // it to another pass.
    out.shown = ceil((vs + w - g0) / lines) as i32;
    out
}

/// Picks the refresh pass `k` such that every piece, sent one after the
/// other from `v_now` (each waiting if needed), starts after pass k's
/// sweep has left it and ends before pass k+1's sweep reaches it.
/// Earliest pass first. Returns `k` and the predicted end, in virtual
/// lines; `None` if no pass works (the frame is longer than a refresh
/// allows).
fn plan(jobs: &[Job], v_now: f32, lines: f32) -> Option<(f32, f32)> {
    let guard = GUARD_LINES + SLACK_LINES;
    let k0 = floor(v_now / lines);
    for dk in [-1.0, 0.0, 1.0, 2.0] {
        let k = k0 + dk;
        let mut t = v_now;
        let mut ok = true;
        for j in jobs {
            let release = k * lines + j.g1 + 1.0 + guard;
            let deadline = (k + 1.0) * lines + j.g0 - guard;
            t = t.max(release);
            if t + j.lines > deadline {
                ok = false;
                break;
            }
            t += j.lines;
        }
        if ok {
            return Some((k, t));
        }
    }
    None
}

/// Did the sweep pass over gate lines `g0..=g1` while it went from
/// virtual line `a` to `b`? If not, how close it came, in lines.
fn crossing(a: f32, b: f32, g0: f32, g1: f32, lines: f32) -> (bool, f32) {
    let k = floor(a / lines);
    let (a, b) = (a - k * lines, b - k * lines);
    let mut gap = f32::MAX;
    // Copies of the span in pass -1, 0, 1, ... up to past `b`.
    let mut j = -1.0;
    while j <= 4.0 {
        let (lo, hi) = (g0 + j * lines, g1 + 1.0 + j * lines);
        if a < hi && b > lo {
            return (true, 0.0);
        }
        gap = gap.min(if b <= lo { lo - b } else { a - hi });
        if lo > b {
            break;
        }
        j += 1.0;
    }
    (false, gap)
}

// ---- counters -------------------------------------------------------------

/// Counters for one reporting period.
#[derive(Clone, Copy)]
pub struct Counts {
    pub frames: u32,
    pieces: u32,
    crossings: u32,
    torn: u32,
    split: u32,
    meas_frames: u32,
    meas_pieces: u32,
    meas_crossings: u32,
    meas_torn: u32,
    bracket_fails: u32,
    no_window: u32,
    late: u32,
    wait_cycles: u64,
    wait_max: u32,
    min_gap: f32,
    anchor_fails: u32,
    anchor_retries: u32,
    anchor_err_max: f32,
    bracket_err_max: f32,
    bracket_err_over3: u32,
    // Why anchor tries were rejected (see `beam::Anchor`).
    anchor_rej_read: u32,
    anchor_rej_inner: u32,
    anchor_rej_value: u32,
    anchor_first_inner_long: u32,
    anchor_inner_max: u32,
    anchor_warm_long: u32,
    anchor_warm_inner_max: u32,
    // The anchor error: how many, sum of |error|, above 1 and 3 lines.
    anchor_err_n: u32,
    anchor_err_abs_sum: f32,
    anchor_err_over1: u32,
    anchor_err_over3: u32,
    /// Frames that used a recent anchor again.
    anchor_reused: u32,
    /// Locked frames sent untimed: no usable anchor, or no plan fits.
    /// Together they are `no_window`.
    no_anchor: u32,
    no_fit: u32,
    // Bracket errors by side: the read before a piece, the one after.
    bracket_before_max: f32,
    bracket_before_over3: u32,
    bracket_after_max: f32,
    bracket_after_over3: u32,
    /// Frames with no usable anchor: the model is too old to say whether
    /// they tore, so they are not in `torn` (nor their pieces in
    /// `crossings`, `min_gap` or the bracket errors).
    unscored: u32,
    /// Time spent in `Beam::anchor`, in cycles (all tries and the
    /// throwaway read).
    anchor_cycles: u64,
    /// Columns way: frames sent as slices_locked instead, because the
    /// start-up check did not confirm the mapping for this direction.
    cols_refused: u32,
    /// Columns way, predicted: windows where the writer ran into the
    /// sweep ahead of it, and where the sweep ran into the writer
    /// (together they are the way's `crossings`)...
    cross_writer: u32,
    cross_sweep: u32,
    /// ...and the same from the real reads (together `meas_crossings`).
    meas_cross_writer: u32,
    meas_cross_sweep: u32,
    /// Pieces whose bracket reads were at an untrusted counter value (an
    /// end of its range or the held value): still scored, as before.
    meas_untrusted: u32,
    /// MADCTL (36h) commands sent while sending frames.
    madctl_sends: u32,
    /// Anchor reads with a bit above the 10 that carry the line set, and
    /// the tries rejected for it.
    anchor_bad_bits: u32,
    anchor_rej_bits: u32,
    /// Columns way, measured: frames whose windows were first shown by
    /// different refreshes (in `meas_torn` too).
    meas_split: u32,
    /// Columns way: longest time from the call to the first pixel byte,
    /// and from the end of the last transfer to the return, in cycles
    /// (what `COLS_LEAD_US` and `COLS_TAIL_US` guess).
    cols_lead_max: u32,
    cols_tail_max: u32,
}

impl Counts {
    const fn new() -> Self {
        Self {
            frames: 0,
            pieces: 0,
            crossings: 0,
            torn: 0,
            split: 0,
            meas_frames: 0,
            meas_pieces: 0,
            meas_crossings: 0,
            meas_torn: 0,
            bracket_fails: 0,
            no_window: 0,
            late: 0,
            wait_cycles: 0,
            wait_max: 0,
            min_gap: f32::MAX,
            anchor_fails: 0,
            anchor_retries: 0,
            anchor_err_max: 0.0,
            bracket_err_max: 0.0,
            bracket_err_over3: 0,
            anchor_rej_read: 0,
            anchor_rej_inner: 0,
            anchor_rej_value: 0,
            anchor_first_inner_long: 0,
            anchor_inner_max: 0,
            anchor_warm_long: 0,
            anchor_warm_inner_max: 0,
            anchor_err_n: 0,
            anchor_err_abs_sum: 0.0,
            anchor_err_over1: 0,
            anchor_err_over3: 0,
            anchor_reused: 0,
            no_anchor: 0,
            no_fit: 0,
            bracket_before_max: 0.0,
            bracket_before_over3: 0,
            bracket_after_max: 0.0,
            bracket_after_over3: 0,
            unscored: 0,
            anchor_cycles: 0,
            cols_refused: 0,
            cross_writer: 0,
            cross_sweep: 0,
            meas_cross_writer: 0,
            meas_cross_sweep: 0,
            meas_untrusted: 0,
            madctl_sends: 0,
            anchor_bad_bits: 0,
            anchor_rej_bits: 0,
            meas_split: 0,
            cols_lead_max: 0,
            cols_tail_max: 0,
        }
    }

    /// Counts what one anchor did.
    fn add_anchor(&mut self, a: &beam::Anchor) {
        self.anchor_retries += a.tries.saturating_sub(1);
        if !a.ok {
            self.anchor_fails += 1;
        }
        self.anchor_reused += u32::from(a.reused);
        self.anchor_rej_read += a.rej_read;
        self.anchor_rej_inner += a.rej_inner;
        self.anchor_rej_value += a.rej_value;
        self.anchor_rej_bits += a.rej_bits;
        self.anchor_bad_bits += a.bad_bits;
        self.anchor_first_inner_long += u32::from(a.first_inner_long);
        self.anchor_inner_max = self.anchor_inner_max.max(a.inner_max);
        self.anchor_warm_long += u32::from(a.warm_long());
        self.anchor_warm_inner_max = self.anchor_warm_inner_max.max(a.warm_inner.unwrap_or(0));
        if let Some(e) = a.err_lines {
            let e = abs(e);
            self.anchor_err_max = self.anchor_err_max.max(e);
            self.anchor_err_n += 1;
            self.anchor_err_abs_sum += e;
            self.anchor_err_over1 += u32::from(e > 1.0);
            self.anchor_err_over3 += u32::from(e > 3.0);
        }
    }
}

/// Totals since start, per way the frames were really sent: frames, torn
/// (predicted), frames measured, torn (measured). `cols_refused` (columns
/// only): frames asked for as columns but sent as slices_locked (they
/// are in slices_locked's totals).
#[derive(Clone, Copy, Default)]
struct Total {
    frames: u32,
    torn: u32,
    meas_frames: u32,
    meas_torn: u32,
    cols_refused: u32,
}

pub struct Stats {
    pub now: Counts,
    totals: [Total; WAYS],
}

impl Stats {
    pub const fn new() -> Self {
        Self {
            now: Counts::new(),
            totals: [Total {
                frames: 0,
                torn: 0,
                meas_frames: 0,
                meas_torn: 0,
                cols_refused: 0,
            }; WAYS],
        }
    }

    /// Starts a new reporting period (the totals are kept).
    pub fn restart(&mut self) {
        self.now = Counts::new();
    }

    /// One `bench=tear` line for the period, then a new period. `ms` is
    /// its length, for the frame rate.
    #[allow(clippy::too_many_arguments)]
    pub fn report(
        &mut self,
        way: Way,
        scene: &str,
        beam: &Beam,
        measure: bool,
        mhz: u32,
        depth: Depth,
        ms: u64,
    ) {
        let c = &self.now;
        let t = &self.totals[way.index()];
        let ms = ms.max(1);
        let frames = c.frames.max(1);
        let pct = |n: u32, of: u32| (n * 100).checked_div(of).unwrap_or(0);
        info!(
            "bench=tear way={} scene={scene} dir={} measure={measure} mhz={mhz} depth={} frames={} seconds={}.{:03} fps={}.{} pieces={} crossings={} crossings_per_frame={} torn_frames={} torn_pct={} split_frames={} meas_frames={} meas_crossings={} meas_torn_frames={} meas_torn_pct={} bracket_fails={} no_window={} late_pieces={} wait_us_avg={} wait_us_max={} min_gap_lines={} anchor_err_max_lines={} anchor_retries={} anchor_fails={} bracket_err_max_lines={} bracket_err_over3={} total_frames={} total_torn={} total_meas_frames={} total_meas_torn={} anchor_rej_read={} anchor_rej_inner={} anchor_rej_value={} anchor_first_inner_long={} anchor_inner_us_max={} anchor_warm={} anchor_warm_long={} anchor_warm_inner_us_max={} anchor_err_n={} anchor_err_mean_abs_lines={} anchor_err_over1={} anchor_err_over3={} anchor_reused={} no_anchor={} no_fit={} bracket_before_over3={} bracket_before_max_lines={} bracket_after_over3={} bracket_after_max_lines={} unscored_frames={} anchor_us_avg={} cols_refused={} cross_writer={} cross_sweep={} meas_cross_writer={} meas_cross_sweep={} meas_untrusted={} madctl_sends={} anchor_bad_bits={} anchor_rej_bits={} meas_split_frames={} cols_lead_us_max={} cols_tail_us_max={} total_cols_refused={}",
            way.name(),
            if beam.reversed { "B" } else { "A" },
            depth.bits(),
            c.frames,
            ms / 1000,
            ms % 1000,
            u64::from(c.frames) * 1000 / ms,
            u64::from(c.frames) * 10_000 / ms % 10,
            c.pieces,
            c.crossings,
            Dec(c.crossings as f32 / frames as f32),
            c.torn,
            pct(c.torn, c.frames),
            c.split,
            c.meas_frames,
            c.meas_crossings,
            c.meas_torn,
            pct(c.meas_torn, c.meas_frames),
            c.bracket_fails,
            c.no_window,
            c.late,
            c.wait_cycles / u64::from(frames) / u64::from(CPU_MHZ),
            c.wait_max / CPU_MHZ,
            Dec(if c.min_gap == f32::MAX {
                -1.0
            } else {
                c.min_gap
            }),
            Dec(c.anchor_err_max),
            c.anchor_retries,
            c.anchor_fails,
            Dec(c.bracket_err_max),
            c.bracket_err_over3,
            t.frames,
            t.torn,
            t.meas_frames,
            t.meas_torn,
            c.anchor_rej_read,
            c.anchor_rej_inner,
            c.anchor_rej_value,
            c.anchor_first_inner_long,
            Dec(c.anchor_inner_max as f32 / CPU_MHZ as f32),
            beam::ANCHOR_WARM_UP,
            c.anchor_warm_long,
            Dec(c.anchor_warm_inner_max as f32 / CPU_MHZ as f32),
            c.anchor_err_n,
            Dec(c.anchor_err_abs_sum / c.anchor_err_n.max(1) as f32),
            c.anchor_err_over1,
            c.anchor_err_over3,
            c.anchor_reused,
            c.no_anchor,
            c.no_fit,
            c.bracket_before_over3,
            Dec(c.bracket_before_max),
            c.bracket_after_over3,
            Dec(c.bracket_after_max),
            c.unscored,
            c.anchor_cycles / u64::from(frames) / u64::from(CPU_MHZ),
            c.cols_refused,
            c.cross_writer,
            c.cross_sweep,
            c.meas_cross_writer,
            c.meas_cross_sweep,
            c.meas_untrusted,
            c.madctl_sends,
            c.anchor_bad_bits,
            c.anchor_rej_bits,
            c.meas_split,
            Dec(c.cols_lead_max as f32 / CPU_MHZ as f32),
            Dec(c.cols_tail_max as f32 / CPU_MHZ as f32),
            t.cols_refused,
        );
        self.restart();
    }
}

// ---- sending --------------------------------------------------------------

/// What one frame did, for the counters.
#[derive(Default)]
struct FrameTally {
    crossed: bool,
    split: bool,
    shown: Option<i32>,
    meas_crossed: bool,
    meas_ok: bool,
    /// Columns way only: the same split test from the real reads, with
    /// every window's pass put on the first measured window's numbering
    /// (`meas_base`: its before-read's virtual line and time stamp).
    meas_split: bool,
    meas_shown: Option<i32>,
    meas_base: Option<(f32, u32)>,
}

/// A piece that did not cross was first shown by pass `pass`: a split if
/// another piece of the frame was first shown by another pass.
fn note_shown(seen: &mut Option<i32>, split: &mut bool, pass: i32) {
    match *seen {
        None => *seen = Some(pass),
        Some(other) if other != pass => *split = true,
        Some(_) => {}
    }
}

impl FrameTally {
    /// A piece that did not cross was first shown by pass `shown`.
    fn shown(&mut self, shown: i32) {
        note_shown(&mut self.shown, &mut self.split, shown);
    }

    /// The same for the measured counter.
    fn meas_shown(&mut self, shown: i32) {
        note_shown(&mut self.meas_shown, &mut self.meas_split, shown);
    }
}

/// Virtual line at cycle count `t` from two real reads: the sweep's
/// path between them (`Beam::measured_path`) taken as a straight line in
/// time.
fn measured_v(b: (u16, u32), a: (u16, u32), path: (f32, f32), t: u32) -> f32 {
    let span = a.1.wrapping_sub(b.1) as i32 as f32;
    let at = t.wrapping_sub(b.1) as i32 as f32;
    if span <= 0.0 {
        path.0
    } else {
        path.0 + (path.1 - path.0) * at / span
    }
}

/// Sends `rects` of the logical screen the given way, and counts.
///
/// `strip_rows` is the strip size used for `rows` (today's sender);
/// slices use [`SLICE_STRIP_ROWS`]. With `measure`, each piece is
/// bracketed by two real scanline reads. Returns the pixel bytes sent.
#[allow(clippy::too_many_arguments)]
pub fn present(
    lcd: &mut Lcd,
    fb: &Frame,
    pal: &Palette,
    rects: &[Rect],
    way: Way,
    beam: &mut Beam,
    measure: bool,
    stats: &mut Stats,
    strip_rows: usize,
) -> Result<u32, LcdError> {
    // One read fixes the model for this frame (all ways: the counters
    // need it). If it fails, a recent anchor stands in.
    let t = cycles();
    let anchor = beam.anchor(lcd);
    let c = &mut stats.now;
    c.anchor_cycles += u64::from(cycles().wrapping_sub(t));
    c.add_anchor(&anchor);
    let madctl_sends = lcd.madctl_sends;

    // The columns way runs only on a mapping the start-up check
    // confirmed (for the direction in use); else this frame goes as
    // slices_locked, counted, and said once.
    let config = cols::Config::for_direction(beam.reversed);
    let mut send_way = way;
    if way == Way::Columns && !beam.orient.confirms(config) {
        send_way = Way::SlicesLocked;
        c.cols_refused += 1;
        if !beam.orient.refusal_logged {
            beam.orient.refusal_logged = true;
            warn!(
                "tear: columns way refused: MADCTL {:02x}h mirror_c={} mirror_r={} not confirmed by bench=tear_orient (checked={}); sending slices_locked instead (counted in cols_refused)",
                config.madctl, config.mirror_c, config.mirror_r, beam.orient.checked,
            );
        }
    }
    let columns = send_way == Way::Columns;

    let (depth, mhz) = (lcd.depth(), lcd.write_mhz());
    // In measure mode the plan must allow for the bracket reads.
    let read_us = beam.read_cycles as f32 / CPU_MHZ as f32;
    let extra_us = if measure { 2.0 * read_us } else { 0.0 };
    let jobs = if columns {
        build_cols(rects, beam, depth, mhz, if measure { read_us } else { 0.0 })
    } else {
        build(rects, send_way, beam, depth, mhz, extra_us)
    };
    let strip_rows = if send_way == Way::Rows {
        strip_rows
    } else {
        SLICE_STRIP_ROWS
    };
    let lines = beam.lines;
    let guard = GUARD_LINES + SLACK_LINES;

    let mut pass = None;
    if send_way == Way::SlicesLocked || columns {
        // With this frame's own anchor or a recent one (at most
        // `beam::ANCHOR_REUSE_MS` old); an older one has drifted. With
        // neither, or when no pass fits, the frame goes untimed.
        if anchor.usable() {
            let v_now = beam.v_at(cycles());
            pass = if columns {
                plan_cols(jobs.as_slice(), v_now, lines)
            } else {
                plan(jobs.as_slice(), v_now, lines)
            }
            .map(|p| p.0);
            c.no_fit += u32::from(pass.is_none());
        } else {
            c.no_anchor += 1;
        }
        if pass.is_none() {
            c.no_window += 1;
        }
    }

    let max_wait = (MAX_WAIT_REFRESHES * lines * beam.line_cycles) as u32;
    let mut frame = FrameTally {
        meas_ok: measure,
        ..Default::default()
    };
    let mut sent = 0;
    let mut result = Ok(());
    for job in jobs.as_slice() {
        if let Some(k) = pass {
            // Wait (spinning on the cycle counter, no bus traffic) until
            // pass k's sweep has left this piece (columns: until the
            // first byte would go out at the start of its window).
            let window = if columns {
                cols_window(job, lines)
            } else {
                (0.0, 0.0)
            };
            let release = if columns {
                k * lines + window.0 - job.lead
            } else {
                k * lines + job.g1 + 1.0 + guard
            };
            let target = beam.cycles_at(release);
            let w0 = cycles();
            while (cycles().wrapping_sub(target) as i32) < 0 && cycles().wrapping_sub(w0) < max_wait
            {
            }
            let waited = cycles().wrapping_sub(w0);
            c.wait_cycles += u64::from(waited);
            c.wait_max = c.wait_max.max(waited);
            // Late: it will not end before pass k+1's sweep arrives
            // (columns: the first byte would go out after the window), or
            // the wait gave up before the sweep had left (model error).
            let v = beam.v_at(cycles());
            let late = if columns {
                v + job.lead > k * lines + window.1
            } else {
                v + job.lines > (k + 1.0) * lines + job.g0 - guard
            };
            if waited >= max_wait || late {
                c.late += 1;
            }
        }
        let before = if measure { Beam::bracket(lcd) } else { None };
        let s = cycles();
        let r = if columns {
            cols::present_rect(lcd, fb, pal, job.rect, config, Send::Overlap)
        } else {
            pipeline::present_rect(lcd, fb, pal, job.rect, strip_rows, Send::Overlap)
        };
        let e = cycles();
        let after = if measure { Beam::bracket(lcd) } else { None };
        let timing = match r {
            Ok(t) => t,
            Err(err) => {
                result = Err(err);
                break;
            }
        };
        sent += timing.bytes;
        // Columns: the writer's path runs from the first pixel byte to the
        // end of the last transfer, not from the call's start and end
        // (those add the window commands and the first strip before, the
        // last wait after).
        let (fs, fe) = (
            timing.first_byte.unwrap_or(s),
            timing.last_byte.unwrap_or(e),
        );
        if columns {
            c.cols_lead_max = c.cols_lead_max.max(fs.wrapping_sub(s));
            c.cols_tail_max = c.cols_tail_max.max(e.wrapping_sub(fe));
        }

        // Predicted (only with a usable anchor: an older one says nothing).
        c.pieces += 1;
        if anchor.usable() {
            let (vs, ve) = (beam.v_at(s), beam.v_at(e));
            if columns {
                let x = cols_crossing(beam.v_at(fs), beam.v_at(fe), job.g0, job.g1, lines, 0.0);
                if x.crossed {
                    c.crossings += 1;
                    frame.crossed = true;
                    if x.writer_first {
                        c.cross_writer += 1;
                    } else {
                        c.cross_sweep += 1;
                    }
                } else {
                    c.min_gap = c.min_gap.min(x.gap);
                    frame.shown(x.shown);
                }
            } else {
                let (crossed, gap) = crossing(vs, ve, job.g0, job.g1, lines);
                if crossed {
                    c.crossings += 1;
                    frame.crossed = true;
                } else {
                    c.min_gap = c.min_gap.min(gap);
                    // The first pass whose sweep reaches the piece after it ends.
                    frame.shown(ceil((ve - job.g0) / lines) as i32);
                }
            }
        }

        // Measured.
        if measure {
            match (before, after) {
                (Some(b), Some(a)) => {
                    c.meas_pieces += 1;
                    c.meas_untrusted += u32::from(!beam.trusted(b.0) || !beam.trusted(a.0));
                    let (vb, va) = beam.measured_path(b, a);
                    let crossed = if columns {
                        // One pass numbering for the whole frame: the
                        // first measured window's, carried to this one by
                        // the time between their before-reads (as
                        // `measured_path` does inside a window).
                        let shift = match frame.meas_base {
                            None => {
                                frame.meas_base = Some((vb, b.1));
                                0.0
                            }
                            Some((v0, t0)) => {
                                let elapsed = b.1.wrapping_sub(t0) as i32 as f32 / beam.line_cycles;
                                lines * round((v0 + elapsed - vb) / lines)
                            }
                        };
                        let path = (vb + shift, va + shift);
                        // The writer's first and last byte put on the
                        // sweep's real path between the two reads.
                        let (vs, ve) = (measured_v(b, a, path, fs), measured_v(b, a, path, fe));
                        let x = cols_crossing(vs, ve, job.g0, job.g1, lines, 1.0);
                        if x.crossed {
                            if x.writer_first {
                                c.meas_cross_writer += 1;
                            } else {
                                c.meas_cross_sweep += 1;
                            }
                        } else {
                            frame.meas_shown(x.shown);
                        }
                        x.crossed
                    } else {
                        // A counter value c means the sweep is somewhere in
                        // line c..c+1: the end may be up to one line further.
                        crossing(vb, va + 1.0, job.g0, job.g1, lines).0
                    };
                    if crossed {
                        c.meas_crossings += 1;
                        frame.meas_crossed = true;
                    }
                    // How far the model was from the real reads (only where
                    // the counter moves with time, and with a usable anchor).
                    for (line, v, t, after) in [(b.0, vb, b.1, false), (a.0, va, a.1, true)] {
                        if !anchor.usable() || !beam.trusted(line) {
                            continue;
                        }
                        let err = abs(beam::wrap_half(v - beam.v_at(t), lines));
                        c.bracket_err_max = c.bracket_err_max.max(err);
                        let over3 = u32::from(err > 3.0);
                        c.bracket_err_over3 += over3;
                        if after {
                            c.bracket_after_max = c.bracket_after_max.max(err);
                            c.bracket_after_over3 += over3;
                        } else {
                            c.bracket_before_max = c.bracket_before_max.max(err);
                            c.bracket_before_over3 += over3;
                        }
                    }
                }
                _ => {
                    c.bracket_fails += 1;
                    frame.meas_ok = false;
                }
            }
        }
    }

    c.madctl_sends += lcd.madctl_sends.wrapping_sub(madctl_sends);
    c.frames += 1;
    c.unscored += u32::from(!anchor.usable());
    let torn = frame.crossed || frame.split;
    c.torn += u32::from(torn);
    c.split += u32::from(frame.split);
    // Totals go to the way the frame was really sent: a refused columns
    // frame is a slices_locked one, and only counted as refused under
    // columns.
    let total = &mut stats.totals[send_way.index()];
    total.frames += 1;
    total.torn += u32::from(torn);
    if frame.meas_ok {
        // A split (columns only) is a tear too: some refresh showed part
        // of the frame old and part new.
        let meas_torn = frame.meas_crossed || frame.meas_split;
        c.meas_frames += 1;
        c.meas_torn += u32::from(meas_torn);
        c.meas_split += u32::from(frame.meas_split);
        total.meas_frames += 1;
        total.meas_torn += u32::from(meas_torn);
    }
    if send_way != way {
        stats.totals[way.index()].cols_refused += 1;
    }
    result.map(|()| sent)
}

// ---- start-up checks --------------------------------------------------------

/// Predicted cost of a full sliced frame at `mhz` and `depth`, over every
/// possible start position of the sweep: how often it can start at once
/// (no waiting at all), and the average and longest wait and frame time.
fn predict(beam: &Beam, mhz: u32, depth: Depth) {
    let jobs = build(&[Rect::FULL], Way::SlicesLocked, beam, depth, mhz, 0.0);
    let busy: f32 = jobs.as_slice().iter().map(|j| j.lines).sum();
    let steps = beam.lines as u32;
    let (mut fits, mut at_once, mut idle_sum, mut idle_max, mut span_sum) =
        (0u32, 0u32, 0.0f32, 0.0f32, 0.0f32);
    for i in 0..steps {
        let v = i as f32;
        if let Some((_, end)) = plan(jobs.as_slice(), v, beam.lines) {
            let idle = end - v - busy;
            fits += 1;
            if idle < 1.0 {
                at_once += 1;
            }
            idle_sum += idle;
            idle_max = idle_max.max(idle);
            span_sum += end - v;
        }
    }
    let to_us = beam.line_cycles / CPU_MHZ as f32;
    let n = fits.max(1) as f32;
    info!(
        "bench=tear_window mhz={mhz} depth={} pieces={} piece_us={} busy_us={} fits_pct={} no_wait_pct={} wait_us_avg={} wait_us_max={} frame_us_avg={} refresh_us={}",
        depth.bits(),
        jobs.n,
        Dec(jobs.list[0].lines * to_us),
        Dec(busy * to_us),
        fits * 100 / steps.max(1),
        at_once * 100 / steps.max(1),
        Dec(idle_sum / n * to_us),
        Dec(idle_max * to_us),
        Dec(span_sum / n * to_us),
        Dec(beam.period_us()),
    );
}

/// The columns way's sibling of [`predict`]: one full-frame window at
/// `mhz` and `depth`, over every start position of the sweep. `lo_us` and
/// `hi_us` are the start window measured from the sweep passing the
/// window's first gate line (`cols_window`); the `sprite_` pair is the
/// same for a 16x16 logical sprite. Lines into microseconds with the
/// measured line time.
fn predict_cols(beam: &Beam, mhz: u32, depth: Depth) {
    let jobs = build_cols(&[Rect::FULL], beam, depth, mhz, 0.0);
    let job = jobs.list[0];
    let (lo, hi) = cols_window(&job, beam.lines);
    let sprite = Job::cols(
        Rect {
            x: 72,
            y: 52,
            w: 16,
            h: 16,
        },
        beam,
        depth,
        mhz,
        0.0,
    );
    let (slo, shi) = cols_window(&sprite, beam.lines);
    let steps = beam.lines as u32;
    let (mut fits, mut at_once, mut idle_sum, mut idle_max, mut span_sum) =
        (0u32, 0u32, 0.0f32, 0.0f32, 0.0f32);
    for i in 0..steps {
        let v = i as f32;
        if let Some((_, end)) = plan_cols(jobs.as_slice(), v, beam.lines) {
            let idle = end - v - job.lines;
            fits += 1;
            if idle < 1.0 {
                at_once += 1;
            }
            idle_sum += idle;
            idle_max = idle_max.max(idle);
            span_sum += end - v;
        }
    }
    let to_us = beam.line_cycles / CPU_MHZ as f32;
    let n = fits.max(1) as f32;
    info!(
        "bench=tear_window_cols mhz={mhz} depth={} per_gate={} gates={} lo_us={} hi_us={} window_us={} lead_us={} busy_us={} fits_pct={} no_wait_pct={} wait_us_avg={} wait_us_max={} frame_us_avg={} sprite_per_gate={} sprite_lo_us={} sprite_hi_us={} guard_lines={} refresh_us={}",
        depth.bits(),
        Dec(job.per_gate),
        Dec(job.g1 - job.g0 + 1.0),
        Dec((lo - job.g0) * to_us),
        Dec((hi - job.g0) * to_us),
        Dec((hi - lo) * to_us),
        Dec(job.lead * to_us),
        Dec(job.lines * to_us),
        fits * 100 / steps.max(1),
        at_once * 100 / steps.max(1),
        Dec(idle_sum / n * to_us),
        Dec(idle_max * to_us),
        Dec(span_sum / n * to_us),
        Dec(sprite.per_gate),
        Dec((slo - sprite.g0) * to_us),
        Dec((shi - sprite.g0) * to_us),
        Dec(COLS_GUARD_LINES),
        Dec(beam.period_us()),
    );
}

/// The sender's start-up checks: the predicted start windows, what
/// slicing really costs, and a short run of each way with both counters
/// on. Uses the framebuffer as it is (the test card at start-up).
pub fn characterise(lcd: &mut Lcd, fb: &Frame, pal: &Palette, beam: &mut Beam) {
    for (mhz, depth) in [
        (80, Depth::Rgb565),
        (80, Depth::Rgb444),
        (40, Depth::Rgb565),
        (40, Depth::Rgb444),
    ] {
        predict(beam, mhz, depth);
    }
    for (mhz, depth) in [
        (80, Depth::Rgb565),
        (80, Depth::Rgb444),
        (40, Depth::Rgb565),
        (40, Depth::Rgb444),
    ] {
        predict_cols(beam, mhz, depth);
    }

    // Real cost: today's full frame against the sliced one, sent free.
    let _ = lcd.set_write_mhz(80);
    let _ = lcd.set_depth(Depth::Rgb565);
    const REPS: u32 = 10;
    let mut stats = Stats::new();
    let (mut rows, mut slices, mut slices_max, mut rows_fill) = (0u64, 0u64, 0u32, 0u64);
    for _ in 0..REPS {
        let t = cycles();
        let r = pipeline::present_full(lcd, fb, pal, 8, Send::Overlap);
        rows += u64::from(cycles().wrapping_sub(t));
        if let Ok(timing) = r {
            rows_fill += u64::from(timing.fill);
        }
        let t = cycles();
        let _ = present(
            lcd,
            fb,
            pal,
            &[Rect::FULL],
            Way::SlicesFree,
            beam,
            false,
            &mut stats,
            8,
        );
        let c = cycles().wrapping_sub(t);
        slices += u64::from(c);
        slices_max = slices_max.max(c);
    }
    let n = (W / 2) as u64 / u64::from(SLICE_LW);
    let rows_us = rows / u64::from(REPS) / u64::from(CPU_MHZ);
    let slices_us = slices / u64::from(REPS) / u64::from(CPU_MHZ);
    // Wire floor of a frame: 153,600 bytes at 80 MHz.
    let wire = (LW * LH * 4 * 2 * 8 / 80) as u64;
    let read_us = u64::from(beam.read_cycles / CPU_MHZ);
    // The anchor as timed in present(): the throwaway read, the tries and
    // their waits (the first frame after calibrate() may need several).
    let anchor_us = stats.now.anchor_cycles / u64::from(REPS) / u64::from(CPU_MHZ);
    info!(
        "bench=tear_slices mhz=80 depth=16 pieces={n} rows_us={rows_us} slices_us={slices_us} slices_us_max={} extra_us={} wire_floor_us={wire} per_job_overhead_us={} anchor_read_us={read_us} anchor_us_avg={anchor_us}",
        u64::from(slices_max) / u64::from(CPU_MHZ),
        slices_us.saturating_sub(rows_us),
        // Beyond the wire as the plan counts it (WIRE_SLOWDOWN included)
        // and beyond the anchor, so the number can go straight into
        // JOB_OVERHEAD_US.
        slices_us.saturating_sub((wire as f32 * WIRE_SLOWDOWN) as u64 + anchor_us) / n,
    );

    // The columns way's cost: a full frame sent free (no anchor, no
    // wait), against the rows above. Only on a confirmed mapping.
    let config = cols::Config::for_direction(beam.reversed);
    if beam.orient.confirms(config) {
        let sends = lcd.madctl_sends;
        let (mut total, mut total_max, mut fill, mut window, mut errors) =
            (0u64, 0u32, 0u64, 0u64, 0u32);
        for _ in 0..REPS {
            let t = cycles();
            let r = cols::present_rect(lcd, fb, pal, Rect::FULL, config, Send::Overlap);
            let c = cycles().wrapping_sub(t);
            total += u64::from(c);
            total_max = total_max.max(c);
            match r {
                Ok(timing) => {
                    fill += u64::from(timing.fill);
                    window += u64::from(timing.window);
                }
                Err(_) => errors += 1,
            }
        }
        let per = u64::from(REPS) * u64::from(CPU_MHZ);
        let cols_us = total / per;
        info!(
            "bench=tear_cols mhz=80 depth=16 madctl=0x{:02x} mirror_r={} rows_us={rows_us} slices_us={slices_us} cols_us={cols_us} cols_us_max={} wire_floor_us={wire} cols_extra_us={} rows_fill_us={} cols_fill_us={} cols_window_us={} madctl_sends={} errors={errors}",
            config.madctl,
            config.mirror_r,
            u64::from(total_max) / u64::from(CPU_MHZ),
            cols_us as i64 - rows_us as i64,
            rows_fill / per,
            fill / per,
            Dec(window as f32 / per as f32),
            lcd.madctl_sends.wrapping_sub(sends),
        );
    } else {
        info!(
            "bench=tear_cols mhz=80 depth=16 refused=1 rows_us={rows_us} slices_us={slices_us} rows_fill_us={}",
            rows_fill / u64::from(REPS) / u64::from(CPU_MHZ)
        );
    }

    // Each way for a few frames, both counters on. Today's way should
    // count every frame as torn, the locked ways none.
    for way in [Way::Rows, Way::SlicesFree, Way::SlicesLocked, Way::Columns] {
        let mut stats = Stats::new();
        let start = cycles();
        for _ in 0..SELFTEST_FRAMES {
            let _ = present(lcd, fb, pal, &[Rect::FULL], way, beam, true, &mut stats, 8);
        }
        let ms = u64::from(cycles().wrapping_sub(start) / CPU_MHZ / 1000);
        stats.report(way, "selftest", beam, true, 80, Depth::Rgb565, ms);
    }
}
