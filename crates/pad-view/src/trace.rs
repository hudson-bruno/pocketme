//! Which input reports get a line in the serial log.
//!
//! The firmware logs only about one raw report a second, so a short press
//! of a button the screen does not show leaves no trace. The [`Tracer`]
//! picks the reports worth a line:
//!
//! - **change**: something digital changed (a standard button, the hat,
//!   a button with no standard name, or the set of unmapped things that
//!   are on, see [`crate::unmapped`]), or it is the first report of its id;
//! - **same**: byte for byte the same as the last report of its id, after
//!   at least [`SILENCE_MS`] with no report at all. In the Xbox modes the
//!   GuliKit sends only when something changes, so an identical report
//!   after a silence means the pad had something to say that the report
//!   has no place for (a button without a slot);
//! - **analog**: only analog values changed, after such a silence (the
//!   first move of a stick, not every step of it);
//! - **trace**: every report, for [`FULL_MS`] after the owner asks
//!   ([`Tracer::start_full`]).
//!
//! Stick wobble alone never makes a line: the reports come every few
//! milliseconds, never after a silence. All kinds together are capped at
//! [`MAX_LINES_PER_S`] lines a second; what is skipped is counted and
//! handed back once the second is over, so the firmware can log the
//! count. The cap keeps the serial line (about 11 KB a second) and the
//! log pipe from filling, and the radio loop from spending its time on
//! formatting.
//!
//! The tracer only decides. The firmware writes the lines (see the
//! bt-gamepad-test README).

/// A report after this long with no report counts as "after a silence".
pub const SILENCE_MS: u32 = 150;
/// Most trace lines in one second.
pub const MAX_LINES_PER_S: u32 = 40;
/// How long a full trace lasts.
pub const FULL_MS: u32 = 10_000;

/// What one report changed, from [`crate::View::last_change`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Change {
    /// The report id byte (0 when the pad uses no ids).
    pub id: u8,
    /// The real length of the report.
    pub len: usize,
    /// Byte for byte the same as the last report of this id.
    pub same: bool,
    /// The first report of this id since the connection (or since the
    /// layout or descriptor changed).
    pub first: bool,
    /// A standard button, the hat, an extra button or an unmapped thing
    /// changed.
    pub digital: bool,
    /// Something else changed (sticks, analog triggers, counters).
    pub analog: bool,
}

/// The kind of a trace line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Change,
    Same,
    Analog,
    Full,
}

impl Kind {
    /// The word after "PAD-HID" in the log.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Change => "change",
            Kind::Same => "same",
            Kind::Analog => "analog",
            Kind::Full => "trace",
        }
    }
}

/// What to log for one report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Decision {
    /// The line to write, if any.
    pub kind: Option<Kind>,
    /// Time since the report before (None for the first one).
    pub gap_ms: Option<u32>,
    /// Lines skipped by the cap in the second that just ended: log this
    /// count first when it is not zero.
    pub skipped: u32,
    /// A full trace was on and is over now.
    pub full_ended: bool,
}

/// See the module text.
#[derive(Clone, Debug, Default)]
pub struct Tracer {
    last_ms: Option<u32>,
    window_ms: Option<u32>,
    lines: u32,
    skipped: u32,
    full_until: Option<u32>,
    /// Lines given and skipped since start.
    pub total_lines: u32,
    pub total_skipped: u32,
}

impl Tracer {
    pub const fn new() -> Self {
        Tracer {
            last_ms: None,
            window_ms: None,
            lines: 0,
            skipped: 0,
            full_until: None,
            total_lines: 0,
            total_skipped: 0,
        }
    }

    /// Log every report for the next [`FULL_MS`] (again from now when it
    /// is already on).
    pub fn start_full(&mut self, now_ms: u32) {
        self.full_until = Some(now_ms.wrapping_add(FULL_MS));
    }

    /// True while a full trace is on.
    pub fn full_active(&self, now_ms: u32) -> bool {
        self.full_until
            .is_some_and(|u| (u.wrapping_sub(now_ms) as i32) > 0)
    }

    /// The link is gone: the next report is not "after a silence" of the
    /// old link. Returns the lines skipped in the unfinished second.
    pub fn link_lost(&mut self) -> u32 {
        self.last_ms = None;
        self.window_ms = None;
        self.lines = 0;
        core::mem::take(&mut self.skipped)
    }

    /// Decide about one report that arrived at `now_ms`.
    pub fn report(&mut self, now_ms: u32, c: &Change) -> Decision {
        let gap = self.last_ms.map(|t| now_ms.wrapping_sub(t));
        self.last_ms = Some(now_ms);
        let silent = gap.is_none_or(|g| g >= SILENCE_MS);
        let mut d = Decision {
            gap_ms: gap,
            ..Decision::default()
        };
        let full = match self.full_until {
            Some(u) if (u.wrapping_sub(now_ms) as i32) > 0 => true,
            Some(_) => {
                self.full_until = None;
                d.full_ended = true;
                false
            }
            None => false,
        };
        let kind = if c.digital || c.first {
            Some(Kind::Change)
        } else if silent && c.same {
            Some(Kind::Same)
        } else if full {
            Some(Kind::Full)
        } else if silent {
            Some(Kind::Analog)
        } else {
            None
        };
        // The one-second window of the cap.
        match self.window_ms {
            Some(w) if now_ms.wrapping_sub(w) < 1000 => {}
            _ => {
                d.skipped = core::mem::take(&mut self.skipped);
                self.lines = 0;
                self.window_ms = Some(now_ms);
            }
        }
        if kind.is_some() {
            if self.lines >= MAX_LINES_PER_S {
                self.skipped += 1;
                self.total_skipped = self.total_skipped.wrapping_add(1);
                return d;
            }
            self.lines += 1;
            self.total_lines = self.total_lines.wrapping_add(1);
        }
        d.kind = kind;
        d
    }
}
