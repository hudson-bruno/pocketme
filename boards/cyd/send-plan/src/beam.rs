//! The beam model's decisions (boards/cyd/src/beam.rs), as plain logic:
//! which Get Scanline (45h) reads are trusted, the time stamp of a read,
//! the start-up survey with its line time and refresh period, the checks
//! before a survey is used, and the judgement of one anchor try.
//!
//! The firmware hands in what its reads gave: the 16-bit answer and the
//! CPU's cycle counter just before the command byte (`a`) and just after
//! the last answer byte (`b`). The reads themselves, the cycle counter and
//! the waits stay in the firmware (boards/cyd/src/beam.rs).
//!
//! Moved here from boards/cyd/src/beam.rs without a change in behaviour
//! (same decisions for the same reads), so that the PC can test it.
//! Two changes came with the move, both on purpose and named in the
//! README: [`MAX_FLOOR_US`] went from 60 to 40, and a survey that missed
//! a refresh's wrap is no longer used ([`Refused::MissedWrap`]).
//!
//! Two refusals came later (2026-11-19). One board start printed
//! lines=325.19 line_us=47.978 calibrated=true, figures no panel has
//! (BOARD, 1 start in 552). Worked out on the PC: one wrong answer of
//! the 45h read (about half or double the true line), or a run of them,
//! was taken for a wrap. Now a survey is not used when two close reads
//! ([`CLOSE_US`]) show a step the counter cannot make
//! ([`Refused::OddStep`]), or when two wraps seen by close reads come
//! much too soon after each other ([`Refused::UnevenWraps`]). Nothing
//! can check the survey's first kept answer (it is the line at the read
//! before, usually the last floor read, which the survey does not
//! stamp), so a drop right after it is not counted as a wrap.
//!
//! Guard v3 (2026-11-19): those two are checked FIRST in
//! [`calibration`]. On the board one start (BOARD, boot 169 of a run of
//! boots) had a survey with an odd step that was refused as
//! [`Refused::Lines`] (checked first then), so it was not surveyed
//! again. On the PC one wrong answer does that when it is taken for one
//! more wrap with only one odd step: 20 or less in the middle of a
//! refresh (256 to 265 with bit 8 lost), or above 140 while the counter
//! shows 1 to 20 (1 with bit 8 or 9 set). The average refresh is then
//! about one in eleven short: about 310 to 322 lines (MEASURED ON THE
//! PC). The order only picks the reason; the same surveys are refused.
//! The firmware surveys again for the reasons [`Refused::retry`] names,
//! up to [`MAX_SURVEYS`] surveys in all (boards/cyd/src/main.rs).
//!
//! What these two do NOT catch (all on the PC; the code before them did
//! not catch these either):
//! - a run of wrong answers that starts while the counter shows 20 or
//!   less (there it holds or jumps anyway) and lasts to the end of the
//!   survey: no step in it is one the counter cannot make;
//! - a wrong answer at the first kept read after a pause, or after
//!   failed or slow reads, longer than [`CLOSE_US`]: the pairs on both
//!   sides of that read are not close, so its answer is never checked;
//! - a wrong answer of 20 or less at the survey's very last read (it
//!   looks like a wrap and no later read can show otherwise);
//! - on a panel with a long porch, a wrong answer while the counter
//!   holds its value in the first refresh;
//! - wrong figures from pauses (an interrupt or a stall between reads):
//!   the reads on both sides of a pause are not close, so neither check
//!   looks there. A pause near a wrap or at a refresh's first fitted
//!   read can still give figures several percent off that pass every
//!   check, as before;
//! - a wrong first answer still moves `line_min` or `line_max` (the
//!   anchor's range), as before.
//!
//! Part of every function here runs before each present (the anchor), so
//! they are all `#[inline]`: the firmware's hot-block guard
//! (boards/cyd/hot-check.py) fails if one of them lands as a function of
//! its own outside the hot block.

/// The rate of the cycle counter: the CPU clock, 240 MHz on the CYD
/// (boards/cyd's `lcd::CPU_MHZ`, checked against this one when the
/// firmware compiles).
pub const CPU_MHZ: u32 = 240;
/// The bits of a 45h answer that carry the line (`lcd::SCANLINE_MASK`,
/// checked the same way).
pub const SCANLINE_MASK: u16 = 0x3FF;

/// Counter values used to fit the line time, away from both ends.
pub const FIT_LO: u16 = 10;
pub const FIT_HI: u16 = 320;
/// A read whose timed part took this much longer than the fastest read
/// was stretched by an interrupt: its time stamp is not trusted.
///
/// The limit is counted from the fastest read of this start, not from a
/// fixed time. Part of the read (esp-hal's `SpiDma::read` and `write`
/// and the clock functions they call) runs from flash. Before the hot
/// block (boards/cyd/hot-block.list, README "The timed paths and the
/// flash cache") where that code landed in a build decided whether three
/// of its hot places shared one line set of the two-way flash cache. Then
/// EVERY read missed the cache once or twice, by the same amount each
/// time (MEASURED ON THE BOARD: 25, 29 and 37 us in some builds against
/// 17 us in the others). With a fixed limit of 25 us such a build kept
/// no read at all and lost its tear-free timing. The block now keeps
/// those functions apart in the cache (17 us in every layout measured
/// since), but read-only data still lands where the linker puts it, so
/// the limit stays relative. A read that is slow by the same amount
/// every time is as good a clock as a fast one: the line time comes
/// from differences, and the time stamp (the middle of the read) moves
/// by half the extra time.
pub const INNER_SLACK_US: u32 = 8;
/// Reads at the start of a survey that only look for the fastest read.
pub const FLOOR_READS: u32 = 64;
/// The fastest read may not be slower than this, or the survey is not
/// used ([`Refused::SlowReads`]).
///
/// Every board start since the hot block read 17 us (MEASURED ON THE
/// BOARD, five two-core layouts and one one-core layout, one run each);
/// 40 is about 2.3 times that. When the extra time of a slow read comes
/// before the panel takes its line, the time stamp is late by half of
/// it: at 40 us that is (40 - 17) / 2 = 11.5 us, plus at most half the
/// slack, 4 us, together about 15.5 us or 0.3 lines of 49.5 us
/// (ESTIMATE), against the one spare line of send-plan's `GUARD_LINES`
/// (8, about 7 needed). The limit was 60 before (about 0.6 lines); it
/// is a proposal, not measured: no start above 25 us has been seen
/// since the hot block.
/// Before the hot block one layout read 41 us or more (an sd-card build
/// with the SPI driver in flash, boards/cyd/README.md): 60 let such a
/// start calibrate, 40 refuses it, and the firmware then keeps its
/// defaults (25 us anchor limit), so its anchors miss and the pictures
/// go out untimed until the next start.
pub const MAX_FLOOR_US: u32 = 40;
/// The limit before a survey (fastest read 17 us, measured, plus slack).
pub const DEFAULT_MAX_INNER_US: u32 = 25;
/// Anchor values must be at least this far from the ends of the range.
pub const ANCHOR_EDGE_LINES: u16 = 2;
/// A value the counter holds for longer than this many lines is "held"
/// (a porch the counter does not count).
pub const HELD_LINES: f32 = 3.0;
/// A survey whose longest time between two wraps is more than this many
/// times its average refresh missed a wrap ([`Refused::MissedWrap`]).
pub const MAX_WRAP_GAP: f32 = 1.5;
/// Two kept reads are "close" when each is at most this long after the
/// kept read before it (the first kept read of a survey is never
/// close). The panel answers a read with the line at the PREVIOUS 45h
/// read, kept or not, so the answers of two close reads were taken
/// between the kept read before the first of them and the second one:
/// at most two times this apart (a pause just under this right before
/// the first of them, then rejected or failed reads up to the second),
/// usually one read. Back-to-back reads are about 24 us apart (8403
/// kept in 200 ms, BOARD); a longer pause between two reads makes the
/// pairs on both sides of it not close.
///
/// In 2 x 200 us the counter moves 8.1 lines at this board's 49.5 us
/// (under [`MAX_STEP_LINES`]), but 15.9 lines at the fastest line time
/// measured on this panel, 25.2 us (frame rate setting 0,
/// experiments/display-dma/logs/tear-boot.log line 86). So with a line
/// time under 33.3 us (above about 87 Hz with 344 lines) a good survey
/// with such a pause and slow reads after it can be refused as
/// [`Refused::OddStep`] (MEASURED ON THE PC at 25.23 us: a pause of 150
/// to 180 us, then 4 slow reads, refused at 114 to 175 of 200 places;
/// none at 33.3 or 49.48 us; how often the board does that is not
/// known). The firmware then surveys again, then keeps its defaults.
pub const CLOSE_US: u32 = 200;
/// The counter's bottom: on this panel it holds 1 through the porch,
/// then jumps to the first visible line (BOARD,
/// experiments/display-dma/logs/tear-boot.log line 88: held_line=1 for
/// 23.87 lines, values_per_refresh=321, line_max=333, so it jumps from 1
/// to about 14). A wrap lands here, and the porch jump starts here.
pub const BOTTOM_LINE: u16 = 20;
/// The porch jump lands at or below this line (back porch 127 lines at
/// most by the data sheet: line 129, plus 11 lines of movement; 8.1
/// lines move in 2 x [`CLOSE_US`] at 49.5 us).
pub const PORCH_TOP_LINE: u16 = 140;
/// Between two close reads the counter moves forward at most this many
/// lines, apart from the porch jump ([`Refused::OddStep`]).
pub const MAX_STEP_LINES: u16 = 12;
/// Between two close reads the counter never goes back, apart from a
/// wrap (more than 100 lines); one or two lines are let through.
pub const MAX_BACK_LINES: u16 = 2;
/// Two wraps both seen by close reads may not follow each other in less
/// than this many average refreshes ([`Refused::UnevenWraps`]). A wrap
/// that is not one (an answer that jumped back more than 100 lines in the
/// middle of a refresh) splits a refresh in two: its shorter part is at
/// most half a refresh, and the average gets about 8 percent short (one
/// more wrap among twelve), so at most 0.55 average refreshes. A wrap
/// seen by close reads is stamped at most 2 x [`CLOSE_US`] after it
/// happened, so on a good survey the time between two of them is a
/// refresh give or take 2 x [`CLOSE_US`] (2.4 percent of a 17 ms
/// refresh). A wrap seen late (after failed reads or a pause) is not
/// seen by close reads and is left out here. Wraps lost in a pattern
/// (every third one, say) make the average long and are refused here
/// too.
pub const MIN_WRAP_GAP: f32 = 0.75;

/// The time stamp of a read that started at cycle `a` and ended at `b`:
/// its middle.
#[inline]
pub fn stamp(a: u32, b: u32) -> u32 {
    a.wrapping_add(b.wrapping_sub(a) / 2)
}

/// Which of the spread buckets (at most 1, 2, 4, 8 us over the fastest
/// read, or more) a read `over_us` microseconds over it goes to.
#[inline]
pub fn spread_bucket(over_us: u32) -> usize {
    match over_us {
        0..=1 => 0,
        2 => 1,
        3..=4 => 2,
        5..=8 => 3,
        _ => 4,
    }
}

/// What reading the counter back to back for a while showed.
pub struct Survey {
    pub samples: u32,
    pub rejected: u32,
    /// Why reads were rejected: the read failed, it took too long, or
    /// the answer had bits outside the counter's 10.
    pub rejected_error: u32,
    pub rejected_slow: u32,
    pub rejected_bits: u32,
    /// The shortest timed part of a read, in cycles (`u32::MAX` when no
    /// read worked).
    pub inner_min: u32,
    /// How many reads took at most 1, 2, 4 and 8 us longer than the
    /// fastest read known at that moment, and how many took longer still.
    pub spread: [u32; 5],
    pub line_min: u16,
    pub line_max: u16,
    pub wraps: u32,
    first_wrap: u32,
    last_wrap: u32,
    /// The longest time between two wraps that followed each other, in
    /// cycles.
    max_wrap_gap: u32,
    /// The shortest time between two wraps that followed each other,
    /// both seen by close reads ([`CLOSE_US`]), in cycles (`u32::MAX`:
    /// none).
    min_wrap_gap: u32,
    /// Whether the last wrap was seen by close reads.
    last_wrap_close: bool,
    /// Whether the last kept read was at most [`CLOSE_US`] after the
    /// kept read before it (false for the first kept read: it has none).
    prev_close: bool,
    /// Steps between close reads that the counter cannot make: a wrap
    /// that lands above [`BOTTOM_LINE`], a step back that is no wrap, or
    /// a step forward of more than [`MAX_STEP_LINES`] that is no porch
    /// jump (from at most [`BOTTOM_LINE`] to at most [`PORCH_TOP_LINE`]).
    pub odd_steps: u32,
    /// The first of those steps, for the log: the answer before it, the
    /// answer after it, and the cycles between the two reads.
    pub first_odd: Option<(u16, u16, u32)>,
    fit_dt: u64,
    fit_dl: u32,
    outer_sum: u64,
    held_line: u16,
    held_cycles: u32,
    // The state of the survey's run (the loop's own variables before the
    // move): the last kept read, whether a whole refresh has begun, the
    // first and last read of this refresh inside FIT_LO..=FIT_HI, and the
    // value the counter shows now with the time it began.
    prev: Option<(u16, u32)>,
    in_refresh: bool,
    first: Option<(u16, u32)>,
    last: Option<(u16, u32)>,
    run_line: u16,
    run_start: u32,
}

impl Default for Survey {
    fn default() -> Self {
        Self::new()
    }
}

impl Survey {
    /// A survey that has seen nothing yet.
    pub const fn new() -> Self {
        Self {
            samples: 0,
            rejected: 0,
            rejected_error: 0,
            rejected_slow: 0,
            rejected_bits: 0,
            inner_min: u32::MAX,
            spread: [0; 5],
            line_min: u16::MAX,
            line_max: 0,
            wraps: 0,
            first_wrap: 0,
            last_wrap: 0,
            max_wrap_gap: 0,
            min_wrap_gap: u32::MAX,
            last_wrap_close: false,
            // No kept read before the first one, so the first pair of
            // kept reads is never close. (Its answers can lie across a
            // pause before the first survey read: the first one answers
            // with the line at the last floor read, which is not timed.)
            prev_close: false,
            odd_steps: 0,
            first_odd: None,
            fit_dt: 0,
            fit_dl: 0,
            outer_sum: 0,
            held_line: u16::MAX,
            held_cycles: 0,
            prev: None,
            in_refresh: false,
            first: None,
            last: None,
            run_line: u16::MAX,
            run_start: 0,
        }
    }

    /// A read of the first [`FLOOR_READS`]: it only lowers the fastest
    /// read.
    #[inline]
    pub fn floor_read(&mut self, a: u32, b: u32) {
        self.inner_min = self.inner_min.min(b.wrapping_sub(a));
    }

    /// A read that failed (an SPI error).
    #[inline]
    pub fn read_failed(&mut self) {
        self.rejected += 1;
        self.rejected_error += 1;
    }

    /// One read of the survey: answer `raw`, timed part `a..b`, `outer`
    /// cycles for the whole read with its clock switches. Returns
    /// whether the read was kept.
    #[inline]
    pub fn push(&mut self, raw: u16, a: u32, b: u32, outer: u32) -> bool {
        let line = raw & SCANLINE_MASK;
        let inner = b.wrapping_sub(a);
        self.inner_min = self.inner_min.min(inner);
        let over = (inner - self.inner_min) / CPU_MHZ;
        self.spread[spread_bucket(over)] += 1;
        if inner > self.inner_limit() || raw != line {
            self.rejected += 1;
            if raw != line {
                self.rejected_bits += 1;
            } else {
                self.rejected_slow += 1;
            }
            return false;
        }
        let stamp = stamp(a, b);
        self.samples += 1;
        self.outer_sum += u64::from(outer);
        self.line_min = self.line_min.min(line);
        self.line_max = self.line_max.max(line);
        if let Some((pl, pt)) = self.prev {
            let near = stamp.wrapping_sub(pt) <= CLOSE_US * CPU_MHZ;
            let close = near && self.prev_close;
            self.prev_close = near;
            // Nothing checks the first kept answer (the first pair is
            // never close), so a drop right after it is no wrap: a wrong
            // first answer would give a false first wrap and wrong
            // figures. A real wrap there is only lost; the next one
            // starts the count.
            let wrap = line + 100 < pl && self.samples > 2;
            let odd = if wrap {
                line > BOTTOM_LINE
            } else if line < pl {
                line + MAX_BACK_LINES < pl
            } else {
                line > pl + MAX_STEP_LINES && (pl > BOTTOM_LINE || line > PORCH_TOP_LINE)
            };
            if close && odd {
                if self.odd_steps == 0 {
                    self.first_odd = Some((pl, line, stamp.wrapping_sub(pt)));
                }
                self.odd_steps += 1;
            }
            if wrap {
                // Back at the top.
                if self.wraps == 0 {
                    self.first_wrap = stamp;
                } else {
                    let gap = stamp.wrapping_sub(self.last_wrap);
                    self.max_wrap_gap = self.max_wrap_gap.max(gap);
                    if close && self.last_wrap_close {
                        self.min_wrap_gap = self.min_wrap_gap.min(gap);
                    }
                }
                self.last_wrap = stamp;
                self.last_wrap_close = close;
                self.wraps += 1;
                if self.in_refresh
                    && let (Some(f), Some(l)) = (self.first, self.last)
                    && l.0 > f.0
                {
                    self.fit_dt += u64::from(l.1.wrapping_sub(f.1));
                    self.fit_dl += u32::from(l.0 - f.0);
                }
                self.in_refresh = true;
                self.first = None;
                self.last = None;
            }
            if line != self.run_line {
                let held = pt.wrapping_sub(self.run_start);
                if held > self.held_cycles {
                    self.held_cycles = held;
                    self.held_line = self.run_line;
                }
                self.run_line = line;
                self.run_start = stamp;
            }
        } else {
            self.run_line = line;
            self.run_start = stamp;
        }
        if self.in_refresh && (FIT_LO..=FIT_HI).contains(&line) {
            if self.first.is_none() {
                self.first = Some((line, stamp));
            }
            self.last = Some((line, stamp));
        }
        self.prev = Some((line, stamp));
        true
    }

    /// Average time of one refresh, in cycles.
    #[inline]
    pub fn period_cycles(&self) -> Option<f32> {
        (self.wraps > 1)
            .then(|| self.last_wrap.wrapping_sub(self.first_wrap) as f32 / (self.wraps - 1) as f32)
    }

    /// Time of one counter step, in cycles.
    #[inline]
    pub fn line_cycles(&self) -> Option<f32> {
        (self.fit_dl > 0).then(|| self.fit_dt as f32 / self.fit_dl as f32)
    }

    /// Average cycles of one whole read (with its clock switches).
    #[inline]
    pub fn read_cycles(&self) -> u32 {
        (self.outer_sum / u64::from(self.samples.max(1))) as u32
    }

    /// The longest timed part a read may have to be trusted, in cycles.
    #[inline]
    pub fn inner_limit(&self) -> u32 {
        self.inner_min.saturating_add(INNER_SLACK_US * CPU_MHZ)
    }

    /// The fastest read in whole microseconds, `None` when no read
    /// worked.
    #[inline]
    pub fn fastest_read_us(&self) -> Option<u32> {
        (self.inner_min != u32::MAX).then_some(self.inner_min / CPU_MHZ)
    }

    /// The longest time between two wraps that followed each other, in
    /// cycles (0 with fewer than two wraps).
    #[inline]
    pub fn max_wrap_gap(&self) -> u32 {
        self.max_wrap_gap
    }

    /// The shortest time between two wraps that followed each other,
    /// both seen by close reads, in cycles (`u32::MAX`: none).
    #[inline]
    pub fn min_wrap_gap(&self) -> u32 {
        self.min_wrap_gap
    }
}

/// Reads the counter back to back for `ms` milliseconds: first
/// [`FLOOR_READS`] reads that only look for the fastest read, so that the
/// reads after them are judged against it from the first one on (a
/// faster read later still lowers it), then reads until the time is up.
///
/// `read` is one timed 45h read (`(answer, a, b)`, `None` when it
/// failed), `now` the cycle counter.
#[inline]
pub fn survey(
    mut read: impl FnMut() -> Option<(u16, u32, u32)>,
    mut now: impl FnMut() -> u32,
    ms: u32,
) -> Survey {
    let span = ms * 1000 * CPU_MHZ;
    let mut s = Survey::new();
    for _ in 0..FLOOR_READS {
        if let Some((_, a, b)) = read() {
            s.floor_read(a, b);
        }
    }
    let start = now();
    while now().wrapping_sub(start) < span {
        let t0 = now();
        let Some((raw, a, b)) = read() else {
            s.read_failed();
            continue;
        };
        let outer = now().wrapping_sub(t0);
        s.push(raw, a, b, outer);
    }
    s
}

/// Where the counter can be trusted to move with time: its range, and
/// the value it holds through part of the porch (`u16::MAX`: none).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counter {
    pub line_min: u16,
    pub line_max: u16,
    pub held_line: u16,
}

impl Counter {
    /// Whether the counter moves with time at this value.
    #[inline]
    pub fn trusted(&self, line: u16) -> bool {
        line >= self.line_min + ANCHOR_EDGE_LINES
            && line + ANCHOR_EDGE_LINES <= self.line_max
            && (self.held_line == u16::MAX || line.abs_diff(self.held_line) > 1)
    }
}

/// What a survey gave, when it can be used.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Calibration {
    /// Cycles per counter step (per gate line).
    pub line_cycles: f32,
    /// Lines per refresh (not a whole number).
    pub lines: f32,
    pub counter: Counter,
    /// Average cycles of one whole read.
    pub read_cycles: u32,
    /// The longest timed part an anchor read may have, in cycles.
    pub inner_limit: u32,
}

/// Why a survey was not used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Fewer than two wraps, or no line time (no refresh with two kept
    /// reads inside FIT_LO..=FIT_HI).
    NoFit,
    /// The lines per refresh are not 320 gate lines plus 2 to 254 porch
    /// lines.
    Lines,
    /// The fastest read was slower than [`MAX_FLOOR_US`].
    SlowReads,
    /// The time between two wraps was more than [`MAX_WRAP_GAP`] times
    /// the average: no read was kept around one wrap, so the average
    /// refresh is too long (one missed wrap in ten makes it about 11
    /// percent long, and that still passes [`Refused::Lines`]).
    MissedWrap,
    /// Between two close reads the counter made a step it cannot make
    /// ([`Survey::odd_steps`]): an answer was wrong. One board start
    /// printed lines=325.19 line_us=47.978 calibrated=true (BOARD; the
    /// log has only these figures, not the answers). On the PC, one
    /// wrong answer taken for a wrap (111 for 223, one bit late), or a
    /// run of them, gives figures within about 0.6 lines and 0.06 us of
    /// these (MEASURED ON THE PC): the average refresh 8 percent short
    /// and the line time 3 percent short, which still pass
    /// [`Refused::Lines`].
    OddStep,
    /// Two wraps seen by close reads were less than [`MIN_WRAP_GAP`]
    /// average refreshes apart: a wrong answer was taken for a wrap, or
    /// wraps were lost in a pattern that makes the average long.
    UnevenWraps,
}

impl Refused {
    /// A short name for the log.
    pub fn as_str(self) -> &'static str {
        match self {
            Refused::NoFit => "no_fit",
            Refused::Lines => "lines_out_of_range",
            Refused::SlowReads => "reads_too_slow",
            Refused::MissedWrap => "missed_wrap",
            Refused::OddStep => "odd_step",
            Refused::UnevenWraps => "uneven_wraps",
        }
    }

    /// Whether the firmware surveys again after this: only for a wrong
    /// answer of the 45h read ([`Refused::OddStep`],
    /// [`Refused::UnevenWraps`]), a one-off (on the board 6 of 225
    /// surveys were refused, never two in a row: BOARD).
    /// [`Refused::NoFit`], [`Refused::Lines`] (with no wrong answer
    /// seen) and [`Refused::SlowReads`] mean another panel, a counter
    /// that does not count or a build whose reads are slow: a new
    /// survey gives the same. [`Refused::MissedWrap`] (reads failed or
    /// paused around a wrap) might pass a new survey, but no board
    /// start has shown it, so it is left out (see [`calibration`] for
    /// the order).
    #[inline]
    pub fn retry(self) -> bool {
        matches!(self, Refused::OddStep | Refused::UnevenWraps)
    }
}

/// How many surveys the firmware makes at most at start-up: one, and
/// two more after refusals that [`Refused::retry`] names (each about
/// 200 ms). At 6 refused in 225 (BOARD), three refused in a row would be
/// about 1 start in 50,000 if they are independent (DERIVED).
pub const MAX_SURVEYS: u32 = 3;

/// Line time, lines per refresh and counter range from a survey, if it
/// looks sane.
///
/// The checks come in this order, and the first one that fails is the
/// reason (they refuse the same surveys in any order; the order only
/// picks the reason, and the firmware surveys again only for the two
/// that [`Refused::retry`] names):
/// 1. [`Refused::OddStep`] and 2. [`Refused::UnevenWraps`]: the survey
///    holds a wrong answer (or wraps lost in a pattern). A wrong answer
///    taken for a wrap can push the figures out of range too (one more
///    wrap: the average refresh about 8 percent short, below 322 lines),
///    so these come before every check on the figures, or such a survey
///    would be refused as [`Refused::Lines`] and not done again.
///    UnevenWraps needs the average refresh; with fewer than two wraps
///    it cannot be judged and is skipped. A survey that lost wraps to
///    failed reads and also shows uneven wraps is named UnevenWraps now
///    (MissedWrap before) and so is done again (14 of 300 streams in
///    tests/beam_same.rs, MEASURED ON THE PC).
/// 3. [`Refused::NoFit`]: fewer than two wraps or no line time. A wrong
///    answer only adds wraps, it never takes one away, so this is a
///    panel that does not count or reads that do not work.
/// 4. [`Refused::Lines`]: with no wrong answer seen, figures out of
///    range mean another panel or porch setting.
/// 5. [`Refused::SlowReads`]: the time of the reads, which no answer
///    changes; it comes from where the build put the read's code.
/// 6. [`Refused::MissedWrap`]: no read kept around a wrap (reads failed
///    or a pause), which no single wrong answer does.
#[inline]
pub fn calibration(s: &Survey) -> Result<Calibration, Refused> {
    if s.odd_steps > 0 {
        return Err(Refused::OddStep);
    }
    let period = s.period_cycles();
    if let Some(period) = period
        && (s.min_wrap_gap as f32) < MIN_WRAP_GAP * period
    {
        return Err(Refused::UnevenWraps);
    }
    let (Some(period), Some(line)) = (period, s.line_cycles()) else {
        return Err(Refused::NoFit);
    };
    let lines = period / line;
    // 320 gate lines plus 2 to 254 porch lines.
    if !(322.0..=574.0).contains(&lines) {
        return Err(Refused::Lines);
    }
    if s.inner_min > MAX_FLOOR_US * CPU_MHZ {
        return Err(Refused::SlowReads);
    }
    if s.max_wrap_gap as f32 > MAX_WRAP_GAP * period {
        return Err(Refused::MissedWrap);
    }
    Ok(Calibration {
        line_cycles: line,
        lines,
        counter: Counter {
            line_min: s.line_min,
            line_max: s.line_max,
            held_line: if s.held_cycles as f32 > HELD_LINES * line {
                s.held_line
            } else {
                u16::MAX
            },
        },
        read_cycles: s.read_cycles(),
        inner_limit: s.inner_limit(),
    })
}

/// Why an anchor try was not used; the number is the firmware's index
/// into `Beam::misses` (3 is a failed read, counted there).
pub const MISS_SLOW: usize = 0;
pub const MISS_BITS: usize = 1;
pub const MISS_RANGE: usize = 2;

/// A usable anchor: at cycle `cycles` the counter showed `line`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub cycles: u32,
    pub line: u16,
}

/// One anchor try: two reads back to back. This panel answers a 45h read
/// with the line at the time of the PREVIOUS 45h read (measured), so the
/// first read's time (`la..lb`) goes with the second read's answer
/// `raw` (its own time `a..b`).
///
/// Both reads must be within `inner_limit`, the answer must have no bits
/// outside the counter, and the value must be one the counter moves at.
/// Otherwise the error is the miss's index ([`MISS_SLOW`],
/// [`MISS_BITS`], [`MISS_RANGE`], checked in that order).
///
/// The time stamp is the first read's start plus half the SECOND read's
/// length (the code always did this; before the move it was written
/// `inner.min(own) / 2`, which is `own / 2` because `inner` is at least
/// `own`). Both reads passed the limit, so their lengths differ by at
/// most the slack, and the stamp is off the first read's middle by at
/// most half of that (4 us, about 0.08 lines).
#[inline]
pub fn anchor_try(
    (la, lb): (u32, u32),
    (raw, a, b): (u16, u32, u32),
    inner_limit: u32,
    counter: &Counter,
) -> Result<Anchor, usize> {
    let line = raw & SCANLINE_MASK;
    let own = b.wrapping_sub(a);
    let inner = lb.wrapping_sub(la).max(own);
    if inner > inner_limit {
        Err(MISS_SLOW)
    } else if raw != line {
        Err(MISS_BITS)
    } else if !counter.trusted(line) {
        Err(MISS_RANGE)
    } else {
        Ok(Anchor {
            cycles: la.wrapping_add(own / 2),
            line,
        })
    }
}
