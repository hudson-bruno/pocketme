//! The start-up conversation: output reports some gamepads need before
//! they send their full reports.
//!
//! Only the Switch Pro Controller (and pads that pretend to be one, like
//! the GuliKit in Switch mode) needs one. Out of the box it sends a small
//! report 0x3F. After the host sends subcommand 0x03 with argument 0x30,
//! it sends the full report 0x30 about 60 to 120 times a second, with
//! 12-bit sticks and motion data.
//!
//! A subcommand travels in output report 0x01:
//!
//! ```text
//! byte 0     0x01 (report id: rumble and subcommand)
//! byte 1     packet counter, 0 to 15, one more for each packet
//! bytes 2-9  rumble data for the two motors: 00 01 40 40 00 01 40 40
//!            means "no rumble" (SDL notes that the KingKong 2 Pro does
//!            not start properly with all zeros here); Linux and
//!            Bluepad32 send all zeros
//! byte 10    subcommand id
//! byte 11..  subcommand arguments; SDL pads the packet with zeros to
//!            49 bytes, Linux and Bluepad32 send only the bytes used
//! ```
//!
//! The pad answers each subcommand with input report 0x21: byte 13 is
//! the acknowledgement (top bit set means "done"), byte 14 repeats the
//! subcommand id, the data starts at byte 15. Sources: dekuNukem's
//! Nintendo_Switch_Reverse_Engineering notes, SDL's SDL_hidapi_switch.c,
//! Bluepad32 and the Linux hid-nintendo driver (read to learn; no code
//! copied).
//!
//! # The prober
//!
//! The GuliKit KingKong 2 Pro in Switch mode ignored our first way of
//! talking (49-byte packets with subcommand 0x03 on the interrupt
//! channel, sent at once). Hosts that work with it differ in small
//! things, and it is not known which one matters. So the conversation
//! starts by trying ways of talking ([`VARIANTS`]), in order, each a few
//! times, until the pad answers one:
//!
//! 1. Nothing is sent before the first input report plus
//!    [`SETTLE_MS`] (BlueRetro waits 300 ms after the channel opens), or
//!    [`NO_REPORT_START_MS`] after the HID channels opened if no report
//!    comes at all.
//! 2. Each variant sends subcommand 0x02 (device info, which every host
//!    sends first) its own way: 49 bytes or short, neutral or zero
//!    rumble, right after an input report, or as a HID SET_REPORT on the
//!    control channel. The last variant sends 0x03 0x30 directly.
//!    Each try waits [`TRY_WAIT_MS`] for an answer; after [`PROBE_TRIES`]
//!    tries the next variant starts.
//! 3. The first variant the pad answers (a 0x21 naming its subcommand, or
//!    0x30 reports starting after a 0x03) wins. An answer is credited to
//!    the newest variant that sent that subcommand, so an answer that
//!    arrives after its wait is over still counts.
//! 4. After a win, the steps [`STEPS`] go out the winner's way, in the
//!    order SDL and Bluepad32 use: full report mode (03 30), motion
//!    sensors on (40 01), player 1 light (30 01). Each step is tried up
//!    to [`STEP_TRIES`] times.
//! 5. If no variant is answered, the conversation ends there and the
//!    pad's 0x3F reports are shown as before.
//!
//! The 0x3F reports are decoded and shown the whole time.
//!
//! Worst case: 0.3 s + 7 variants x 3 tries x 0.3 s + 3 steps x 3 tries
//! x 0.3 s = 9.3 s from the first report to the end.
//!
//! Every send, answer, win and failure is queued as a [`Note`] for the
//! firmware's log ([`Startup::take_note`]); [`Startup::outcome`] is the
//! one-word result for the summary line.

use core::fmt;

/// Length of one full subcommand packet over Bluetooth (without the 0xA2
/// header byte that the host adds in front). The buffer given to
/// [`Startup::next_output`] must hold at least this many bytes.
pub const SWITCH_PACKET_LEN: usize = 49;

/// Neutral rumble: both motors still.
const NEUTRAL_RUMBLE: [u8; 8] = [0x00, 0x01, 0x40, 0x40, 0x00, 0x01, 0x40, 0x40];

/// Wait this long after the first input report before the first try.
pub const SETTLE_MS: u32 = 300;
/// With no input report at all, start this long after the first call to
/// [`Startup::next_output`] (the HID channels are open by then).
pub const NO_REPORT_START_MS: u32 = 1000;
/// Wait this long for an answer before the next try.
pub const TRY_WAIT_MS: u32 = 300;
/// A variant sent "right after an input report" sends anyway when no
/// report came for this long.
pub const QUIET_MS: u32 = 300;
/// Tries for each variant while probing.
pub const PROBE_TRIES: u8 = 3;
/// Tries for each step after a win.
pub const STEP_TRIES: u8 = 3;
/// Without a clock ([`Startup::set_time`] never called), each input
/// report counts as this many milliseconds.
const MS_PER_REPORT: u32 = 10;

/// Which HID channel an output report goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// The interrupt channel, as a DATA Output message (header 0xA2).
    Interrupt,
    /// The control channel, as a SET_REPORT Output request (header 0x52).
    /// The pad answers with a HANDSHAKE ([`Startup::on_handshake`]).
    Control,
    /// The control channel, as a GET_REPORT Input request (header 0x41):
    /// "send me input report N now". The packet is one byte, the report
    /// id. The pad answers with a DATA message (the report) or a
    /// HANDSHAKE (an error code). Only [`crate::View`] asks for this, for
    /// the battery report of an Xbox-mode pad.
    GetReport,
}

impl Channel {
    pub fn word(self) -> &'static str {
        match self {
            Channel::Interrupt => "interrupt",
            Channel::Control => "control",
            Channel::GetReport => "get-report",
        }
    }
}

/// One packet from [`Startup::next_output`]: its length in the buffer
/// and the channel to send it on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Output {
    pub len: usize,
    pub channel: Channel,
}

/// How a packet is built and sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Way {
    /// 49 bytes (SDL) or only the bytes used (Linux, Bluepad32).
    pub full_len: bool,
    /// Rumble bytes 00 01 40 40 (SDL) or all zero (Linux, Bluepad32).
    pub neutral: bool,
    pub channel: Channel,
    /// Send only right after an input report (Linux sends 4 ms after
    /// one), or after [`QUIET_MS`] without one.
    pub after_report: bool,
}

/// One way of talking that the prober tries.
#[derive(Clone, Copy, Debug)]
pub struct Variant {
    /// The name in the log and on the About page.
    pub name: &'static str,
    /// The subcommand it sends, and its argument.
    pub sub: u8,
    pub arg: u8,
    pub way: Way,
}

const fn way(full_len: bool, neutral: bool, channel: Channel, after_report: bool) -> Way {
    Way {
        full_len,
        neutral,
        channel,
        after_report,
    }
}

/// The ways of talking, in the order they are tried.
pub const VARIANTS: [Variant; 7] = [
    // SDL's packet, device info first.
    Variant {
        name: "info-49",
        sub: 0x02,
        arg: 0,
        way: way(true, true, Channel::Interrupt, false),
    },
    // Linux's and Bluepad32's packet: short, zero rumble.
    Variant {
        name: "info-short-zero",
        sub: 0x02,
        arg: 0,
        way: way(false, false, Channel::Interrupt, false),
    },
    Variant {
        name: "info-short",
        sub: 0x02,
        arg: 0,
        way: way(false, true, Channel::Interrupt, false),
    },
    Variant {
        name: "info-49-zero",
        sub: 0x02,
        arg: 0,
        way: way(true, false, Channel::Interrupt, false),
    },
    // Linux's timing: right after an input report.
    Variant {
        name: "info-after-report",
        sub: 0x02,
        arg: 0,
        way: way(true, true, Channel::Interrupt, true),
    },
    // HID SET_REPORT on the control channel.
    Variant {
        name: "info-setreport",
        sub: 0x02,
        arg: 0,
        way: way(true, true, Channel::Control, false),
    },
    // Our first way: full report mode directly (only after the settle).
    Variant {
        name: "mode-49",
        sub: 0x03,
        arg: 0x30,
        way: way(true, true, Channel::Interrupt, false),
    },
];

/// The steps after a win, in order: (subcommand id, argument, what it
/// does). This is the order of SDL and Bluepad32.
pub const STEPS: [(u8, u8, &str); 3] = [
    (0x03, 0x30, "full report mode"),
    (0x40, 0x01, "motion sensors on"),
    (0x30, 0x01, "player 1 light"),
];

/// What a subcommand id is for, for the log.
pub fn sub_name(sub: u8) -> &'static str {
    match sub {
        0x02 => "device info",
        _ => match STEPS.iter().find(|s| s.0 == sub) {
            Some(s) => s.2,
            None => "?",
        },
    }
}

/// Number of argument bytes of a subcommand we send.
fn args_len(sub: u8) -> usize {
    if sub == 0x02 { 0 } else { 1 }
}

/// Bytes of an input report that must never be logged: the pad's own
/// Bluetooth address in its answer to device info (0x21, subcommand
/// 0x02, bytes 19 to 24). `None` for any other report.
pub fn private_bytes(r: &[u8]) -> Option<core::ops::Range<usize>> {
    if r.len() >= 25 && r[0] == 0x21 && r[14] == 0x02 {
        Some(19..25)
    } else {
        None
    }
}

/// What happened to one step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepState {
    NotSent,
    /// Sent this many times, no answer yet.
    Sent(u8),
    /// Answered, with this acknowledgement byte.
    Answered(u8),
    /// Full mode only: no 0x21 answer, but 0x30 reports arrive.
    ByReports,
    /// Sent [`STEP_TRIES`] times, never answered.
    NoAnswer,
}

impl StepState {
    /// The word for the summary line.
    pub fn word(self) -> &'static str {
        match self {
            StepState::NotSent => "not-sent",
            StepState::Sent(_) => "waiting",
            StepState::Answered(_) | StepState::ByReports => "ok",
            StepState::NoAnswer => "no-answer",
        }
    }

    pub fn done_ok(self) -> bool {
        matches!(self, StepState::Answered(_) | StepState::ByReports)
    }
}

/// Where the conversation is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Waiting for the first report (plus [`SETTLE_MS`]).
    Waiting,
    /// Trying the variant with this index.
    Probing(usize),
    /// A variant won; sending the step with this index of [`STEPS`].
    Steps(usize),
    /// Every step answered or given up.
    Finished,
    /// No variant was answered.
    AllFailed,
}

/// The pad's answer to device info (never its address).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub firmware: (u8, u8),
    /// 1 Joy-Con left, 2 Joy-Con right, 3 Pro Controller.
    pub kind: u8,
}

/// One thing worth a log line. The firmware prints each as
/// `PAD-STATE ui switch start-up: <note>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// The probe starts. `report_ms`: time since the first input report,
    /// or None when no report came.
    Start {
        report_ms: Option<u32>,
    },
    Sent {
        /// Index in [`VARIANTS`] (the winner's index for a step).
        variant: u8,
        /// Index in [`STEPS`], or None while probing.
        step: Option<u8>,
        sub: u8,
        arg: u8,
        len: u8,
        counter: u8,
        try_k: u8,
        try_n: u8,
        /// For a variant sent after a report: Some(true) when one had
        /// just come, Some(false) when none came for [`QUIET_MS`].
        after_report: Option<bool>,
    },
    /// A 0x21 answer we were waiting for (or a late one).
    Answer {
        sub: u8,
        ack: u8,
        after_ms: u32,
        /// The variant credited with it.
        variant: u8,
        late: bool,
    },
    /// A 0x21 answer for a subcommand we never sent.
    OtherAnswer {
        sub: u8,
        ack: u8,
    },
    DeviceInfo(DeviceInfo),
    /// The pad answered this variant; it is used from now on.
    /// `alt`: an older variant that sent the same subcommand shortly
    /// before, so the answer may be for it; it takes over if no step
    /// is answered the winner's way.
    Won {
        variant: u8,
        probe_ms: u32,
        alt: Option<(u8, u32)>,
    },
    /// No step was answered the winner's way: the other candidate of an
    /// unclear win takes over and the steps start again.
    Switched {
        from: u8,
        to: u8,
    },
    VariantFailed {
        variant: u8,
    },
    AllFailed,
    /// 0x30 reports started; `asked`: after our 0x03 (Some(variant)).
    FullReports {
        asked: Option<u8>,
    },
    StepNoAnswer {
        step: u8,
    },
    Handshake {
        result: u8,
        variant: Option<u8>,
    },
    Finished,
    /// The note queue was full; this many notes were lost.
    Lost(u16),
}

/// Holds the [`Startup`] for writing its one-line result
/// ([`Startup::outcome`]).
pub struct Outcome<'a>(&'a Startup);

impl fmt::Display for Outcome<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.0;
        if !s.active {
            return f.write_str("not-needed");
        }
        match s.core.phase {
            Phase::Waiting => f.write_str("waiting-to-start"),
            Phase::Probing(v) => write!(
                f,
                "probing:{}(try {} of {})",
                VARIANTS[v].name, s.core.tries, PROBE_TRIES
            ),
            Phase::Steps(_) | Phase::Finished => {
                let v = s.core.won.unwrap_or(0);
                write!(f, "won:{}", VARIANTS[v].name)?;
                for (k, (sub, _, _)) in STEPS.iter().enumerate() {
                    write!(f, ",{:02x}:{}", sub, s.core.steps[k].word())?;
                }
                Ok(())
            }
            Phase::AllFailed => {
                f.write_str("all-variants-failed(")?;
                for (i, v) in VARIANTS.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    f.write_str(v.name)?;
                }
                f.write_str(")")?;
                if s.core.steps[0] == StepState::ByReports {
                    f.write_str(",03:ok-unasked")?;
                }
                Ok(())
            }
        }
    }
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = |v: u8| VARIANTS[usize::from(v).min(VARIANTS.len() - 1)].name;
        match *self {
            Note::Start {
                report_ms: Some(ms),
            } => write!(
                f,
                "probe starts, {} ms after the first input report; {} ways to try",
                ms,
                VARIANTS.len()
            ),
            Note::Start { report_ms: None } => write!(
                f,
                "probe starts with no input report seen, {} ms after the HID channels opened; {} ways to try",
                NO_REPORT_START_MS,
                VARIANTS.len()
            ),
            Note::Sent {
                variant,
                step,
                sub,
                arg,
                len,
                counter,
                try_k,
                try_n,
                after_report,
            } => {
                let w = VARIANTS[usize::from(variant)].way;
                write!(f, "variant={} sent subcommand {:#04x}", name(variant), sub)?;
                if args_len(sub) > 0 {
                    write!(f, "/{:#04x}", arg)?;
                }
                write!(
                    f,
                    " ({}) len={} rumble={} channel={} counter={}",
                    sub_name(sub),
                    len,
                    if w.neutral { "neutral" } else { "zero" },
                    w.channel.word(),
                    counter
                )?;
                match after_report {
                    Some(true) => f.write_str(" right after an input report")?,
                    Some(false) => {
                        write!(f, " (no input report for {} ms; sent anyway)", QUIET_MS)?
                    }
                    None => {}
                }
                match step {
                    Some(k) => write!(f, ", step {} of {}", k + 1, STEPS.len())?,
                    None => write!(f, ", way {} of {}", variant + 1, VARIANTS.len())?,
                }
                write!(f, ", try {} of {}", try_k, try_n)
            }
            Note::Answer {
                sub,
                ack,
                after_ms,
                variant,
                late,
            } => write!(
                f,
                "{}answer to subcommand {:#04x} ({}) ack={:#04x} {} ms after the last send, credited to variant={}",
                if late { "late " } else { "" },
                sub,
                sub_name(sub),
                ack,
                after_ms,
                name(variant)
            ),
            Note::OtherAnswer { sub, ack } => write!(
                f,
                "answer to subcommand {:#04x} ack={:#04x}, which we did not send; ignored",
                sub, ack
            ),
            Note::DeviceInfo(d) => write!(
                f,
                "device info: firmware {}.{:02} type={}{}",
                d.firmware.0,
                d.firmware.1,
                d.kind,
                match d.kind {
                    1 => " (Joy-Con left)",
                    2 => " (Joy-Con right)",
                    3 => " (Pro Controller)",
                    _ => "",
                }
            ),
            Note::Won {
                variant,
                probe_ms,
                alt,
            } => {
                let w = VARIANTS[usize::from(variant)].way;
                write!(
                    f,
                    "variant={} WON after {} ms of probing; the rest goes the same way (len={} rumble={} channel={}{})",
                    name(variant),
                    probe_ms,
                    if w.full_len { "49" } else { "short" },
                    if w.neutral { "neutral" } else { "zero" },
                    w.channel.word(),
                    if w.after_report {
                        ", after a report"
                    } else {
                        ""
                    }
                )?;
                if let Some((a, ms)) = alt {
                    write!(
                        f,
                        "; variant={} sent the same subcommand {} ms before the answer, so it may be for it: it takes over if no step is answered",
                        name(a),
                        ms
                    )?;
                }
                Ok(())
            }
            Note::Switched { from, to } => write!(
                f,
                "no step answered the way of variant={}; variant={} takes over and the steps start again",
                name(from),
                name(to)
            ),
            Note::VariantFailed { variant } => write!(
                f,
                "variant={} had no answer after {} tries",
                name(variant),
                PROBE_TRIES
            ),
            Note::AllFailed => {
                write!(f, "all {} variants failed (", VARIANTS.len())?;
                for (i, v) in VARIANTS.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    f.write_str(v.name)?;
                }
                f.write_str("); carrying on with the simple 0x3F reports")
            }
            Note::FullReports { asked: Some(v) } => write!(
                f,
                "full reports (0x30) started, after subcommand 0x03 of variant={}",
                name(v)
            ),
            Note::FullReports { asked: None } => {
                f.write_str("full reports (0x30) arrived before we asked for them")
            }
            Note::StepNoAnswer { step } => {
                let (sub, arg, what) = STEPS[usize::from(step).min(STEPS.len() - 1)];
                write!(
                    f,
                    "no answer to {:#04x}/{:#04x} ({}) after {} tries; going on without it",
                    sub, arg, what, STEP_TRIES
                )
            }
            Note::Handshake { result, variant } => {
                write!(f, "HID handshake result={:#x}", result)?;
                match variant {
                    Some(v) => write!(f, " during variant={}", name(v)),
                    None => f.write_str(" (no SET_REPORT variant running)"),
                }
            }
            Note::Finished => f.write_str("conversation finished"),
            Note::Lost(n) => write!(f, "{} notes lost (queue full)", n),
        }
    }
}

const NOTES: usize = 24;

/// Everything that a packet given and then handed back must undo.
#[derive(Clone, Copy, Debug)]
struct Core {
    phase: Phase,
    /// Tries of the current variant or step.
    tries: u8,
    /// A try is out and its wait is not over.
    waiting: bool,
    /// When the last packet went out.
    sent_at: u32,
    /// Since when the next try is due (for the after-report pacing).
    due_since: u32,
    counter: u8,
    /// When each variant last sent, None if never.
    variant_sent_at: [Option<u32>; VARIANTS.len()],
    won: Option<usize>,
    steps: [StepState; STEPS.len()],
    probe_started: u32,
    /// The other candidate of an unclear win (see [`Note::Won`]).
    alt: Option<usize>,
    sent: u16,
    /// A report arrived since the last call to next_output.
    fresh: bool,
}

impl Core {
    const fn new() -> Self {
        Core {
            phase: Phase::Waiting,
            tries: 0,
            waiting: false,
            sent_at: 0,
            due_since: 0,
            counter: 0,
            variant_sent_at: [None; VARIANTS.len()],
            won: None,
            steps: [StepState::NotSent; STEPS.len()],
            probe_started: 0,
            alt: None,
            sent: 0,
            fresh: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Startup {
    /// True once the pad looks like a Switch Pro Controller.
    pub active: bool,
    core: Core,
    /// The state before the last packet given, for [`Startup::not_sent`].
    before: Option<(Core, u32)>,
    now: u32,
    has_time: bool,
    first_call: Option<u32>,
    first_report: Option<u32>,
    last_report: Option<u32>,
    full_reports: bool,
    /// The pad's device info, once answered.
    pub device_info: Option<DeviceInfo>,
    /// The last HID handshake result on the control channel.
    pub handshake: Option<u8>,
    notes: [Note; NOTES],
    note_start: usize,
    note_len: usize,
    notes_pushed: u32,
    notes_lost: u16,
}

impl Default for Startup {
    fn default() -> Self {
        Self::new()
    }
}

impl Startup {
    pub const fn new() -> Self {
        Startup {
            active: false,
            core: Core::new(),
            before: None,
            now: 0,
            has_time: false,
            first_call: None,
            first_report: None,
            last_report: None,
            full_reports: false,
            device_info: None,
            handshake: None,
            notes: [Note::Finished; NOTES],
            note_start: 0,
            note_len: 0,
            notes_pushed: 0,
            notes_lost: 0,
        }
    }

    /// Forget everything (a new connection). Notes not yet taken are
    /// dropped too.
    pub fn reset(&mut self) {
        let active = self.active;
        *self = Startup::new();
        self.active = active;
    }

    pub fn set_time(&mut self, now_ms: u32) {
        self.now = now_ms;
        self.has_time = true;
    }

    pub fn phase(&self) -> Phase {
        self.core.phase
    }

    /// True when the conversation is over (every step answered or given
    /// up, or no variant answered).
    pub fn finished(&self) -> bool {
        matches!(self.core.phase, Phase::Finished | Phase::AllFailed)
    }

    /// The steps after a win, in the order of [`STEPS`].
    pub fn steps(&self) -> [StepState; STEPS.len()] {
        self.core.steps
    }

    /// The state of the step with this subcommand id.
    pub fn step(&self, sub: u8) -> StepState {
        match STEPS.iter().position(|s| s.0 == sub) {
            Some(k) => self.core.steps[k],
            None => StepState::NotSent,
        }
    }

    /// The winning variant, if any.
    pub fn won(&self) -> Option<&'static Variant> {
        self.core.won.map(|v| &VARIANTS[v])
    }

    /// While probing: (variant, try).
    pub fn probing(&self) -> Option<(&'static Variant, u8)> {
        match self.core.phase {
            Phase::Probing(v) => Some((&VARIANTS[v], self.core.tries)),
            _ => None,
        }
    }

    /// Number of output reports given and not handed back.
    pub fn sent(&self) -> u16 {
        self.core.sent
    }

    /// The one-line result, for example `won:info-49,03:ok,40:ok,30:ok`.
    pub fn outcome(&self) -> Outcome<'_> {
        Outcome(self)
    }

    /// The oldest note not yet taken.
    pub fn take_note(&mut self) -> Option<Note> {
        if self.notes_lost > 0 {
            let n = self.notes_lost;
            self.notes_lost = 0;
            return Some(Note::Lost(n));
        }
        if self.note_len == 0 {
            return None;
        }
        let n = self.notes[self.note_start];
        self.note_start = (self.note_start + 1) % NOTES;
        self.note_len -= 1;
        Some(n)
    }

    fn note(&mut self, n: Note) {
        self.notes_pushed = self.notes_pushed.wrapping_add(1);
        if self.note_len == NOTES {
            self.notes_lost = self.notes_lost.saturating_add(1);
            return;
        }
        self.notes[(self.note_start + self.note_len) % NOTES] = n;
        self.note_len += 1;
    }

    fn since(&self, t: u32) -> u32 {
        self.now.wrapping_sub(t)
    }

    /// Look at an input report (report id at index 0), from the
    /// interrupt channel or a DATA message on the control channel.
    pub fn on_report(&mut self, r: &[u8]) {
        if !self.has_time {
            self.now = self.now.wrapping_add(MS_PER_REPORT);
        }
        if !self.active || r.is_empty() {
            return;
        }
        // The pad said something: what the last packet changed stays.
        self.before = None;
        self.first_report.get_or_insert(self.now);
        self.last_report = Some(self.now);
        self.core.fresh = true;
        match r[0] {
            0x21 if r.len() >= 15 => {
                let (ack, sub) = (r[13], r[14]);
                if sub == 0x02 && ack & 0x80 != 0 && r.len() >= 18 && self.device_info.is_none() {
                    let d = DeviceInfo {
                        firmware: (r[15], r[16]),
                        kind: r[17],
                    };
                    self.device_info = Some(d);
                    self.note(Note::DeviceInfo(d));
                }
                self.on_answer(sub, ack);
            }
            0x30 | 0x31 if !self.full_reports => {
                self.full_reports = true;
                self.on_full_reports();
            }
            _ => {}
        }
    }

    /// A HID HANDSHAKE on the control channel (its result code, 0 =
    /// successful). Logged; it does not win a variant by itself.
    pub fn on_handshake(&mut self, result: u8) {
        if !self.active {
            return;
        }
        self.handshake = Some(result);
        let variant = match self.core.phase {
            Phase::Probing(v) if VARIANTS[v].way.channel == Channel::Control => Some(v as u8),
            Phase::Steps(_) => self
                .core
                .won
                .filter(|v| VARIANTS[*v].way.channel == Channel::Control)
                .map(|v| v as u8),
            _ => None,
        };
        self.note(Note::Handshake { result, variant });
    }

    /// The newest variant that sent subcommand `sub`. Variants are tried
    /// in order, so that is the last one in the list that sent it.
    fn credit(&self, sub: u8) -> Option<usize> {
        (0..VARIANTS.len())
            .rev()
            .find(|&i| VARIANTS[i].sub == sub && self.core.variant_sent_at[i].is_some())
    }

    fn on_answer(&mut self, sub: u8, ack: u8) {
        let after_ms = self.since(self.core.sent_at);
        match self.core.phase {
            Phase::Waiting | Phase::Probing(_) | Phase::AllFailed => {
                let Some(v) = self.credit(sub) else {
                    self.note(Note::OtherAnswer { sub, ack });
                    return;
                };
                let late = self.core.phase != Phase::Probing(v) || !self.core.waiting;
                self.note(Note::Answer {
                    sub,
                    ack,
                    after_ms,
                    variant: v as u8,
                    late,
                });
                self.win(v);
                if let Some(k) = STEPS.iter().position(|s| s.0 == sub) {
                    self.core.steps[k] = StepState::Answered(ack);
                }
                self.next_step();
            }
            Phase::Steps(_) | Phase::Finished => {
                let won = self.core.won.unwrap_or(0) as u8;
                let Some(k) = STEPS.iter().position(|s| s.0 == sub) else {
                    if sub == VARIANTS[usize::from(won)].sub {
                        // One more answer to the winning probe: fine.
                        self.note(Note::Answer {
                            sub,
                            ack,
                            after_ms,
                            variant: won,
                            late: true,
                        });
                    } else {
                        self.note(Note::OtherAnswer { sub, ack });
                    }
                    return;
                };
                match self.core.steps[k] {
                    StepState::Sent(_) | StepState::NoAnswer => {
                        let current = self.core.phase == Phase::Steps(k) && self.core.waiting;
                        self.note(Note::Answer {
                            sub,
                            ack,
                            after_ms,
                            variant: won,
                            late: !current,
                        });
                        self.core.steps[k] = StepState::Answered(ack);
                        self.core.alt = None;
                        if self.core.phase == Phase::Steps(k) {
                            self.next_step();
                        }
                    }
                    StepState::ByReports => {
                        self.core.steps[k] = StepState::Answered(ack);
                    }
                    StepState::NotSent | StepState::Answered(_) => {}
                }
            }
        }
    }

    fn on_full_reports(&mut self) {
        match self.core.phase {
            Phase::Waiting | Phase::Probing(_) | Phase::AllFailed => match self.credit(0x03) {
                Some(v) => {
                    self.note(Note::FullReports {
                        asked: Some(v as u8),
                    });
                    self.win(v);
                    self.core.steps[0] = StepState::ByReports;
                    self.next_step();
                }
                None => {
                    // Already in full mode: still probe, for a way to send
                    // the other steps.
                    self.note(Note::FullReports { asked: None });
                    self.core.steps[0] = StepState::ByReports;
                }
            },
            Phase::Steps(k) => {
                if !self.core.steps[0].done_ok() {
                    let asked = self.core.steps[0] != StepState::NotSent;
                    self.note(Note::FullReports {
                        asked: if asked {
                            self.core.won.map(|v| v as u8)
                        } else {
                            None
                        },
                    });
                    self.core.steps[0] = StepState::ByReports;
                    if asked {
                        // Our 0x03 went through the winner's way.
                        self.core.alt = None;
                    }
                    if k == 0 {
                        self.next_step();
                    }
                }
            }
            Phase::Finished => {
                if !self.core.steps[0].done_ok() {
                    self.note(Note::FullReports {
                        asked: self.core.won.map(|v| v as u8),
                    });
                    self.core.steps[0] = StepState::ByReports;
                }
            }
        }
    }

    fn win(&mut self, v: usize) {
        let probe_ms = self.since(self.core.probe_started);
        let sub = VARIANTS[v].sub;
        let alt = (0..v).rev().find(|&j| {
            VARIANTS[j].sub == sub
                && self.core.variant_sent_at[j].is_some_and(|t| self.since(t) < 2 * TRY_WAIT_MS)
        });
        self.core.won = Some(v);
        self.core.alt = alt;
        self.note(Note::Won {
            variant: v as u8,
            probe_ms,
            alt: alt.map(|a| {
                let at = self.core.variant_sent_at[a].unwrap_or(self.now);
                (a as u8, self.since(at))
            }),
        });
        self.core.phase = Phase::Steps(0);
        self.core.tries = 0;
        self.core.waiting = false;
        self.core.due_since = self.now;
    }

    /// Moves to the first step not yet done, or to the end.
    fn next_step(&mut self) {
        let from = match self.core.phase {
            Phase::Steps(k) => k,
            _ => return,
        };
        self.core.tries = 0;
        self.core.waiting = false;
        self.core.due_since = self.now;
        let mut k = from;
        while k < STEPS.len() && self.core.steps[k].done_ok() {
            k += 1;
        }
        if k >= STEPS.len() {
            self.core.phase = Phase::Finished;
            self.note(Note::Finished);
        } else {
            self.core.phase = Phase::Steps(k);
        }
    }

    /// The packet [`Startup::next_output`] just gave did not go out (for
    /// example: the HID channel was not open yet). It does not count as a
    /// try: the next call gives it again, with the same packet counter,
    /// and its "sent" note is taken back if it is still queued. Does
    /// nothing if the pad said something since.
    pub fn not_sent(&mut self) {
        let Some((core, pushed)) = self.before.take() else {
            return;
        };
        self.core = core;
        // The fresh report the packet was waiting for is still fresh.
        self.core.fresh = true;
        let new = self.notes_pushed.wrapping_sub(pushed) as usize;
        self.note_len -= new.min(self.note_len);
        self.notes_pushed = pushed;
    }

    /// The next packet to send, if any: written into `buf` (at least
    /// [`SWITCH_PACKET_LEN`] bytes, else nothing is given), with its
    /// length and channel.
    pub fn next_output(&mut self, buf: &mut [u8]) -> Option<Output> {
        if !self.active || buf.len() < SWITCH_PACKET_LEN {
            return None;
        }
        let snapshot = (self.core, self.notes_pushed);
        self.first_call.get_or_insert(self.now);
        let fresh = core::mem::take(&mut self.core.fresh);
        let out = self.next_inner(buf, fresh);
        self.before = if out.is_some() { Some(snapshot) } else { None };
        out
    }

    fn next_inner(&mut self, buf: &mut [u8], fresh: bool) -> Option<Output> {
        loop {
            match self.core.phase {
                Phase::Finished | Phase::AllFailed => return None,
                Phase::Waiting => {
                    let report_ms = self.first_report.map(|t| self.since(t));
                    let due = match report_ms {
                        Some(ms) => ms >= SETTLE_MS,
                        None => {
                            self.since(self.first_call.unwrap_or(self.now)) >= NO_REPORT_START_MS
                        }
                    };
                    if !due {
                        return None;
                    }
                    self.note(Note::Start { report_ms });
                    self.core.probe_started = self.now;
                    self.core.phase = Phase::Probing(0);
                    self.core.tries = 0;
                    self.core.waiting = false;
                    self.core.due_since = self.now;
                }
                Phase::Probing(v) => {
                    if self.core.waiting {
                        if self.since(self.core.sent_at) < TRY_WAIT_MS {
                            return None;
                        }
                        self.core.waiting = false;
                        self.core.due_since = self.now;
                        if self.core.tries >= PROBE_TRIES {
                            self.note(Note::VariantFailed { variant: v as u8 });
                            if v + 1 >= VARIANTS.len() {
                                self.core.phase = Phase::AllFailed;
                                self.note(Note::AllFailed);
                                return None;
                            }
                            self.core.phase = Phase::Probing(v + 1);
                            self.core.tries = 0;
                            continue;
                        }
                    }
                    let var = VARIANTS[v];
                    let after = self.pace(var.way, fresh)?;
                    self.core.tries += 1;
                    self.core.variant_sent_at[v] = Some(self.now);
                    return Some(self.send(buf, v, None, var.sub, var.arg, PROBE_TRIES, after));
                }
                Phase::Steps(k) => {
                    let v = self.core.won.unwrap_or(0);
                    if self.core.waiting {
                        if self.since(self.core.sent_at) < TRY_WAIT_MS {
                            return None;
                        }
                        self.core.waiting = false;
                        self.core.due_since = self.now;
                        if self.core.tries >= STEP_TRIES {
                            self.core.steps[k] = StepState::NoAnswer;
                            self.note(Note::StepNoAnswer { step: k as u8 });
                            let none_answered = !self
                                .core
                                .steps
                                .iter()
                                .any(|s| matches!(s, StepState::Answered(_)));
                            if let (Some(alt), true) = (self.core.alt.take(), none_answered) {
                                self.note(Note::Switched {
                                    from: v as u8,
                                    to: alt as u8,
                                });
                                self.core.won = Some(alt);
                                for st in self.core.steps.iter_mut() {
                                    if *st == StepState::NoAnswer {
                                        *st = StepState::NotSent;
                                    }
                                }
                                self.core.phase = Phase::Steps(0);
                            } else {
                                self.core.phase = Phase::Steps(k + 1);
                            }
                            self.next_step();
                            continue;
                        }
                    }
                    let (sub, arg, _) = STEPS[k];
                    let after = self.pace(VARIANTS[v].way, fresh)?;
                    self.core.tries += 1;
                    self.core.steps[k] = StepState::Sent(self.core.tries);
                    return Some(self.send(buf, v, Some(k), sub, arg, STEP_TRIES, after));
                }
            }
        }
    }

    /// For a way sent after a report: Some(Some(true)) right after one,
    /// Some(Some(false)) after [`QUIET_MS`] without one, None to wait.
    /// For other ways: Some(None).
    fn pace(&self, w: Way, fresh: bool) -> Option<Option<bool>> {
        if !w.after_report {
            return Some(None);
        }
        if fresh {
            Some(Some(true))
        } else if self.since(self.core.due_since) >= QUIET_MS {
            Some(Some(false))
        } else {
            None
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn send(
        &mut self,
        buf: &mut [u8],
        variant: usize,
        step: Option<usize>,
        sub: u8,
        arg: u8,
        try_n: u8,
        after_report: Option<bool>,
    ) -> Output {
        let w = VARIANTS[variant].way;
        let len = if w.full_len {
            SWITCH_PACKET_LEN
        } else {
            11 + args_len(sub)
        };
        let counter = self.core.counter;
        buf[..len].fill(0);
        buf[0] = 0x01;
        buf[1] = counter;
        if w.neutral {
            buf[2..10].copy_from_slice(&NEUTRAL_RUMBLE);
        }
        buf[10] = sub;
        if args_len(sub) > 0 {
            buf[11] = arg;
        }
        self.core.counter = (counter + 1) & 0x0F;
        self.core.waiting = true;
        self.core.sent_at = self.now;
        self.core.sent = self.core.sent.saturating_add(1);
        self.note(Note::Sent {
            variant: variant as u8,
            step: step.map(|k| k as u8),
            sub,
            arg,
            len: len as u8,
            counter,
            try_k: self.core.tries,
            try_n,
            after_report,
        });
        Output {
            len,
            channel: w.channel,
        }
    }
}
