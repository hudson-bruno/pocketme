//! pad-host: the console's gamepad host, in one value.
//!
//! In: the HCI packets the radio controller sends, the time, and the
//! saved pairings (read through a read-only view of the flash). Out: HCI
//! packets for the controller, the pad's state as a game sees it
//! (`board_api::PadState`, `Motion`, pressed buttons), connection events,
//! the battery, and log lines.
//!
//! It is reconnect-only, always: it never searches, is never visible,
//! never calls anyone, accepts a call only from a pad it has a saved key
//! for, refuses every pairing and never changes the saved keys (see
//! pad-proto's `Config::reconnect_only`). Every failure ends the link and
//! comes back to "waiting".
//!
//! # How the board uses it
//!
//! The board only moves bytes. Once:
//!
//! 1. `PadHost::new()` (about 19.7 KB on the ESP32; `const`, so keep it in
//!    a static, for example a `ConstStaticCell`, never on a small stack;
//!    never move it in the loop).
//! 2. `pads.load_pairings(&mut flash, region)`: reads the saved pairings
//!    (read only) and logs how many and the area's CRC-32.
//! 3. `pads.start(now_ms, &mut radio)`: the controller's set-up commands
//!    go out through `radio.send`.
//!
//! Then, in the radio loop:
//!
//! * every packet from the controller, whole, packet type byte first
//!   (0x04 event or 0x02 ACL): `pads.packet(now_ms, bytes, &mut radio)`.
//!   Buffer: [`RX_MAX`] bytes covers any packet the ESP32 controller sends
//!   (an ACL packet of 1021 data bytes plus 5 header bytes). A longer
//!   packet is a controller fault: drop it.
//! * every 10 to 20 ms, and after a batch of packets:
//!   `let wake = pads.tick(now_ms, &mut radio);`. True when something a
//!   game cares about changed since the last tick (see [`map::changed`]):
//!   then the console should run a frame.
//!
//! What goes out through [`Radio::send`] is one whole HCI packet (0x01
//! command or 0x02 ACL), at most [`TX_MAX`] bytes. The radio must send
//! each one in order; if it cannot, it should drop it and count it (the
//! host's timeouts recover). The console's frame reads [`PadHost::read`],
//! [`PadHost::motion`], [`PadHost::take_pressed`] and the events
//! ([`PadHost::take_event`]).
//!
//! The time is milliseconds from any start, never going back.
//!
//! Nothing here allocates, panics on bad input, or uses `unsafe`.

#![no_std]
#![forbid(unsafe_code)]

mod decoder;
pub mod inject;
pub mod map;

use core::fmt::{self, Write as _};

use board_api::{Motion, PadState};
use pad_proto::buf::Text;
use pad_proto::hci::{self, BdAddr, LinkKey};
use pad_proto::{Config, Host, Io, KeyChange, Tag, UiEvent};
use pad_store::{ReadFlash, ReadReport, Region};
use pad_view::decode::Layout;
use pad_view::startup::{Channel, SWITCH_PACKET_LEN};

pub use decoder::Mode;
pub use map::Battery;

/// The longest packet [`PadHost::packet`] needs to take: an ACL packet
/// with the ESP32 controller's 1021 data bytes, its 4 header bytes and
/// the type byte. Give the radio's receive buffer at least this much.
pub const RX_MAX: usize = 1 + 4 + 1021;
/// The longest packet [`Radio::send`] is given: a command of 255
/// parameter bytes (type, opcode, length: 4 bytes in front). ACL packets
/// we send are shorter (at most 4 + 254 data bytes after the type byte).
pub const TX_MAX: usize = 4 + 255;
/// Output reports handed to the radio per [`PadHost::tick`] at most.
const OUTPUTS_PER_TICK: usize = 4;
/// Connection events kept until [`PadHost::take_event`]; older ones are
/// dropped first when nobody takes them.
const EVENTS: usize = 4;
/// The silence guard: a ready pad that streams (Switch mode with full
/// reports 0x30, about 85 to 120 reports a second also at rest) and sent
/// nothing for this long is gone (switched off: it sends no "detach").
/// The longest gap between two reports seen with the real pad was
/// 366 ms; 1 s is almost three times that.
pub const SILENCE_MS: u64 = 1_000;
/// Ticks further apart than this mean the board's loop was busy: the
/// silence is counted again from that tick (a late look is not a silent
/// pad).
pub const STALL_MS: u64 = 250;
/// The reason given to the console for a pad the silence guard ended:
/// "connection timeout", as the controller's own timeout says.
pub const REASON_SILENT: u8 = 0x08;

/// The board's radio, as the pad host sees it.
pub trait Radio {
    /// One whole HCI packet for the controller, packet type byte first.
    fn send(&mut self, packet: &[u8]);
    /// One log line. `tag` is the kind ("PAD-STATE", "PAD-HCI",
    /// "PAD-HOST", ...): print it in front. Holds no link key and at most
    /// the last two bytes of any address. Default: nothing.
    fn log(&mut self, tag: &'static str, args: fmt::Arguments<'_>) {
        let _ = (tag, args);
    }
}

/// Something the console may want to show or react to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadEvent {
    /// The pad is here: its first report was understood.
    Connected { mode: Mode, name: Name },
    /// The pad is gone (switched off, out of range, or a failure).
    /// `reason` is the Bluetooth reason code: 0x13 the pad ended it (for
    /// example switched off), 0x08 out of range (supervision timeout),
    /// 0x16 we ended it.
    Disconnected { reason: u8 },
    /// The battery level or charging state changed (or is first known).
    Battery(Battery),
}

/// A pad's name, as it gave it (printable ASCII only, at most 32 bytes).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Name {
    len: u8,
    bytes: [u8; 32],
}

impl Name {
    fn from(s: &str) -> Self {
        let mut n = Name {
            len: 0,
            bytes: [0; 32],
        };
        let b = s.as_bytes();
        let k = b.len().min(32);
        n.bytes[..k].copy_from_slice(&b[..k]);
        n.len = k as u8;
        n
    }
    pub fn as_str(&self) -> &str {
        // Only printable ASCII is ever stored (it comes from a `Text`).
        core::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("?")
    }
}

impl fmt::Debug for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What happened so far, for the board's stats line. Counted since the
/// start (or [`PadHost::reset_counters`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    /// Calls the host took up: it left "waiting" for a link (a saved pad
    /// called; an unknown device is refused while still waiting and is
    /// not counted).
    pub attempts: u32,
    /// Attempts that came as far as a report the decoder understood.
    pub ready: u32,
    /// Attempts that ended before that, by the furthest stage reached:
    /// 0 accepting the call, 1 security (key, encryption), 2 opening the
    /// HID channels, 3 waiting for the first understood report.
    pub failed: [u32; 4],
    /// Links that ended after they were ready (switched off, out of
    /// range, a failure later on).
    pub lost: u32,
    /// Input reports taken (real and injected), and those of them the
    /// decoder did not understand.
    pub reports: u32,
    pub undecoded: u32,
    /// Reports injected for testing ([`PadHost::inject_report`]).
    pub injected: u32,
    /// Longest time between two reports while a pad was ready, in ms.
    /// (A Switch-mode pad sends about every 15 ms; the Xbox modes only
    /// when something changes, so long gaps are normal there.)
    pub gap_max_ms: u32,
    /// Connection events dropped because nobody took them.
    pub events_lost: u32,
    /// Links the silence guard ended ([`SILENCE_MS`]).
    pub silent: u32,
}

/// The stages of a call, in order, for [`Counters::failed`].
const STAGE_ACCEPT: u8 = 0;
const STAGE_SECURITY: u8 = 1;
const STAGE_OPEN: u8 = 2;
const STAGE_REPORT: u8 = 3;

/// What [`PadHost::load_pairings`] found. No address, no key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loaded {
    /// What the read-only scan of the pairing area found (with its
    /// CRC-32: the proof it did not change).
    pub report: ReadReport,
    /// Pairings given to the host (at most four are kept).
    pub keys: usize,
}

/// The gamepad host. See the crate notes for the calls and their order.
pub struct PadHost {
    host: Host,
    st: State,
}

/// Everything except pad-proto's host, so both can be borrowed at once.
struct State {
    dec: decoder::Decoder,
    /// Connected and a report understood on this link.
    connected: bool,
    announced: bool,
    /// The game's view at the last change (for "wake").
    seen: PadState,
    wake: bool,
    /// Buttons that went down since the last `take_pressed`.
    pressed: u32,
    last_buttons: u32,
    battery: Option<Battery>,
    events: [Option<PadEvent>; EVENTS],
    events_lost: u32,
    /// The reason of the last disconnection-complete for our link.
    reason: Option<u8>,
    link_handle: Option<u16>,
    reports: u32,
    undecoded: u32,
    motion_seen: bool,
    now_ms: u64,
    /// Reports are being injected (no radio link).
    injecting: bool,
    /// The call being taken up: the furthest stage reached; and whether
    /// it came as far as an understood report.
    attempt: Option<u8>,
    attempt_ready: bool,
    /// When the last report came while ready (for the longest gap).
    last_report_ms: Option<u64>,
    /// The silence guard: armed once a real (not injected) Switch-mode
    /// pad sends full reports (0x30), which it then sends all the time.
    streaming: bool,
    /// Since when nothing came (the last report, or a late tick).
    quiet_since: u64,
    /// The last tick (to see a busy loop: [`STALL_MS`]).
    last_tick_ms: Option<u64>,
    /// pad-proto's host said the link ended (counted in `follow`).
    link_ended: bool,
    counters: Counters,
}

impl Default for PadHost {
    fn default() -> Self {
        Self::new()
    }
}

impl PadHost {
    /// A host with no saved pad yet. Then [`PadHost::load_pairings`].
    /// `const`: keep it in a static (a `ConstStaticCell`, or a `static`
    /// behind the board's lock) so it is never built on the stack.
    pub const fn new() -> Self {
        let cfg = Config {
            reconnect_only: true,
            ..Config::DEFAULT
        };
        Self {
            host: Host::new(cfg),
            st: State {
                dec: decoder::Decoder::new(),
                connected: false,
                announced: false,
                seen: PadState::NONE,
                wake: false,
                pressed: 0,
                last_buttons: 0,
                battery: None,
                events: [None; EVENTS],
                events_lost: 0,
                reason: None,
                link_handle: None,
                reports: 0,
                undecoded: 0,
                motion_seen: false,
                now_ms: 0,
                injecting: false,
                attempt: None,
                attempt_ready: false,
                last_report_ms: None,
                streaming: false,
                quiet_since: 0,
                last_tick_ms: None,
                link_ended: false,
                counters: Counters {
                    attempts: 0,
                    ready: 0,
                    failed: [0; 4],
                    lost: 0,
                    reports: 0,
                    undecoded: 0,
                    injected: 0,
                    gap_max_ms: 0,
                    events_lost: 0,
                    silent: 0,
                },
            },
        }
    }

    /// Reads the saved pairings through `flash` (which can only read) and
    /// hands them to the host, the oldest first. Call before
    /// [`PadHost::start`]. Logs the count and the area's CRC-32, never an
    /// address or a key. On a flash error nothing is loaded; the console
    /// then simply waits for nobody.
    pub fn load_pairings<F: ReadFlash>(
        &mut self,
        flash: &mut F,
        region: Region,
        radio: &mut impl Radio,
    ) -> Result<Loaded, pad_store::Error<F::Error>> {
        let host = &mut self.host;
        let r = pad_store::read_pairings(flash, region, |p| {
            host.load_key(BdAddr(p.addr), LinkKey(p.key), p.key_type);
        });
        match r {
            Ok(report) => {
                let keys = self.host.key_count();
                radio.log(
                    "PAD-HOST",
                    format_args!(
                        "pairings read (read only): found={} blank={} area={} generation={} count={} damaged={} dropped={} kept={} crc32={:#010x}",
                        u8::from(report.found),
                        u8::from(report.blank),
                        report.area,
                        report.generation,
                        report.count,
                        report.damaged,
                        report.dropped,
                        keys,
                        report.crc
                    ),
                );
                Ok(Loaded { report, keys })
            }
            Err(e) => {
                radio.log(
                    "PAD-HOST",
                    format_args!("pairings not read: {}; no pad can connect", e.word()),
                );
                Err(e)
            }
        }
    }

    /// Sends the controller's set-up commands. Call once, after
    /// [`PadHost::load_pairings`].
    pub fn start(&mut self, now_ms: u64, radio: &mut impl Radio) {
        self.st.now_ms = now_ms;
        let mut io = Glue {
            radio,
            st: &mut self.st,
        };
        self.host.start(now_ms, &mut io);
    }

    /// One packet from the controller, whole, type byte first.
    pub fn packet(&mut self, now_ms: u64, bytes: &[u8], radio: &mut impl Radio) {
        self.st.set_time(now_ms);
        self.st.peek(bytes, self.host.linked());
        let mut io = Glue {
            radio,
            st: &mut self.st,
        };
        self.host.on_packet(now_ms, bytes, &mut io);
        if !self.host.linked() {
            self.st.link_handle = None;
        }
        self.follow(radio);
    }

    /// Timers, and the outputs the pad is waiting for. Call every 10 to
    /// 20 ms. True when the pad changed in a way a game cares about since
    /// the last call (see [`map::changed`]), or an event is waiting.
    pub fn tick(&mut self, now_ms: u64, radio: &mut impl Radio) -> bool {
        self.st.set_time(now_ms);
        self.silence_guard(now_ms, radio);
        let mut io = Glue {
            radio,
            st: &mut self.st,
        };
        self.host.tick(now_ms, &mut io);
        if !self.host.linked() {
            self.st.link_handle = None;
        }
        self.follow(radio);
        self.outputs(now_ms, radio);
        self.st.notes(radio);
        self.st.latch();
        core::mem::take(&mut self.st.wake)
    }

    /// The pad now, as a game sees it. [`PadState::NONE`] when no pad is
    /// connected (or nothing it sent was understood yet).
    pub fn read(&self) -> PadState {
        if self.st.connected {
            map::pad_state(&self.st.dec.pad)
        } else {
            PadState::NONE
        }
    }

    /// The motion sensor now, in the console's units and axes: only in
    /// Switch mode, once the pad sends motion (after the start-up talk
    /// turned it on). None otherwise.
    pub fn motion(&self) -> Option<Motion> {
        if !self.st.connected || self.st.dec.layout != Layout::SwitchPro {
            return None;
        }
        self.st.dec.pad.motion.as_ref().map(map::motion)
    }

    /// The buttons that went down since the last call (board-api `btn`
    /// bits), even if they are up again. Reading clears them. A press
    /// shorter than a frame is seen once. 0 when no pad is connected.
    pub fn take_pressed(&mut self) -> u32 {
        // Buttons pressed in reports since the last tick count too.
        self.st.latch();
        core::mem::take(&mut self.st.pressed)
    }

    /// The battery, when the pad's mode reports it (Switch mode in every
    /// full report; the Xbox modes only if the pad answers the question).
    pub fn battery(&self) -> Option<Battery> {
        if self.st.connected {
            self.st.battery
        } else {
            None
        }
    }

    /// The oldest connection event not taken yet.
    pub fn take_event(&mut self) -> Option<PadEvent> {
        let e = self.st.events[0].take();
        self.st.events.rotate_left(1);
        e
    }

    /// True when a pad is connected and understood.
    pub fn connected(&self) -> bool {
        self.st.connected
    }

    /// The pad's mode (the GuliKit's four, or another pad).
    pub fn mode(&self) -> Mode {
        decoder::mode_of(self.st.dec.layout, self.st.dec.name.as_str())
    }

    /// The pad's name as it gave it ("" before it is known).
    pub fn name(&self) -> &str {
        self.st.dec.name.as_str()
    }

    /// True while no pad is linked and none is calling: the state every
    /// failure comes back to.
    pub fn waiting(&self) -> bool {
        self.host.waiting()
    }

    /// The host's phase, for logs ("waiting", "accepting", "pairing",
    /// "opening", "connected", ...).
    pub fn phase(&self) -> &'static str {
        self.host.phase_name()
    }

    /// Saved pads the host knows.
    pub fn saved_pads(&self) -> usize {
        self.host.key_count()
    }

    /// pad-proto's counters, for a periodic log line.
    pub fn stats(&self) -> pad_proto::Stats {
        self.host.stats()
    }

    /// This host's own counters (calls, failures by stage, reports, the
    /// longest gap between reports).
    pub fn counters(&self) -> Counters {
        let mut c = self.st.counters;
        c.events_lost = self.st.events_lost;
        c
    }

    /// The counters back to zero (a new measurement).
    pub fn reset_counters(&mut self) {
        self.st.counters = Counters::default();
        self.st.events_lost = 0;
    }

    /// One word for where the pad is: "off" (the controller is being set
    /// up), "waiting" (for a saved pad to call), "connecting" (a saved
    /// pad called: accepting it, checking its key, encryption),
    /// "starting" (HID channels opening, or open and no understood
    /// report yet), "ready" (a game sees it).
    pub fn state(&self) -> &'static str {
        if self.st.connected {
            return "ready";
        }
        match self.host.phase_name() {
            "init" => "off",
            "waiting" | "pause" => "waiting",
            "opening" | "connected" => "starting",
            _ => "connecting",
        }
    }

    /// The address of saved pad `i` (0 to 3), for the board's own check
    /// of what it sends (an accept or a key reply only ever to a saved
    /// pad). Personal: never log it whole.
    pub fn saved_address(&self, i: usize) -> Option<[u8; 6]> {
        self.host.key_addr(i).map(|a| a.0)
    }

    // ---- injected input (testing without a pad) -------------------------

    /// Starts injecting reports as if a pad in `mode` had connected
    /// (Switch, XInput or Android mode; the others need a descriptor and
    /// are refused). Only while no real pad is linked or calling: false
    /// otherwise. A pad already injected is let go first. Nothing goes
    /// to the radio: the reports go straight to the decoder, and from
    /// there the same way as a real pad's (presses, wake, motion,
    /// battery, events).
    pub fn inject_begin(&mut self, now_ms: u64, mode: Mode, radio: &mut impl Radio) -> bool {
        let (name, vid, pid) = match mode {
            Mode::Switch => ("Pro Controller", 0x057E, 0x2009),
            Mode::XInput => ("GuliKit Controller XW", 0x045E, 0x02E0),
            Mode::Android => ("Xbox Wireless Controller", 0x045E, 0x02E0),
            _ => return false,
        };
        if self.host.linked() || self.host.phase_name() == "accepting" {
            return false;
        }
        if self.st.injecting {
            self.inject_end(now_ms, 0x13, radio);
        }
        self.st.set_time(now_ms);
        self.st.dec.device(name, Some(vid), Some(pid));
        self.st.injecting = true;
        radio.log(
            "PAD-HOST",
            format_args!("injection on: mode={} (no radio link)", mode.word()),
        );
        true
    }

    /// One injected input report (without the 0xA1 byte). False when no
    /// injection runs.
    pub fn inject_report(&mut self, now_ms: u64, bytes: &[u8], radio: &mut impl Radio) -> bool {
        if !self.st.injecting {
            return false;
        }
        self.st.set_time(now_ms);
        self.st.counters.injected = self.st.counters.injected.wrapping_add(1);
        self.st.on_report(bytes, radio);
        true
    }

    /// The injected pad goes, with this reason (0x13: switched off).
    pub fn inject_end(&mut self, now_ms: u64, reason: u8, radio: &mut impl Radio) {
        if !self.st.injecting {
            return;
        }
        self.st.set_time(now_ms);
        self.st.injecting = false;
        self.st.reason = Some(reason);
        self.st.on_gone(radio);
        radio.log("PAD-HOST", format_args!("injection off"));
    }

    /// True while reports are being injected.
    pub fn injecting(&self) -> bool {
        self.st.injecting
    }

    /// The silence guard (see [`SILENCE_MS`]): a streaming pad that went
    /// quiet is gone at once. The console sees it gone (state none, the
    /// "gone" event, buttons released); the host drops the link (a
    /// Disconnect goes to the controller) and waits for the next call.
    /// Never for an injected pad, a mode that sends only on change, or
    /// before the full reports flow.
    fn silence_guard(&mut self, now_ms: u64, radio: &mut impl Radio) {
        let st = &mut self.st;
        if let Some(t) = st.last_tick_ms
            && now_ms.saturating_sub(t) > STALL_MS
        {
            // The loop was busy: what waited was handed over before this
            // tick; count the silence from here.
            st.quiet_since = st.quiet_since.max(now_ms);
        }
        st.last_tick_ms = Some(now_ms);
        // An injected pad never arms the guard (see `on_report`) and has
        // no radio link.
        if !st.streaming || !st.connected || !self.host.linked() {
            return;
        }
        let quiet = now_ms.saturating_sub(st.quiet_since);
        if quiet < SILENCE_MS {
            return;
        }
        radio.log(
            "PAD-HOST",
            format_args!(
                "silent for {} ms (a Switch-mode pad sends about every 10 ms): it is gone; dropping the link",
                quiet
            ),
        );
        st.counters.silent = st.counters.silent.wrapping_add(1);
        st.reason = Some(REASON_SILENT);
        let mut io = Glue {
            radio,
            st: &mut self.st,
        };
        self.host.drop_link(now_ms, REASON_SILENT, &mut io);
        if !self.host.linked() {
            self.st.link_handle = None;
        }
        self.follow(radio);
    }

    /// After every call into pad-proto's host: a real pad's call ends an
    /// injection, and each call's furthest stage is counted.
    fn follow(&mut self, radio: &mut impl Radio) {
        let phase = self.host.phase_name();
        let stage = match phase {
            "init" | "waiting" | "pause" => None,
            "accepting" | "paging" | "cancelling" | "inquiry" => Some(STAGE_ACCEPT),
            "opening" => Some(STAGE_OPEN),
            "connected" => Some(STAGE_REPORT),
            _ => Some(STAGE_SECURITY),
        };
        if self.st.injecting && stage.is_some() {
            radio.log(
                "PAD-HOST",
                format_args!("a saved pad calls: injection ends"),
            );
            self.inject_end(self.st.now_ms, 0x16, radio);
        }
        let c = &mut self.st.counters;
        // A link ended and a new call came within the same packet (the
        // linked pad called again): count the end first.
        if core::mem::take(&mut self.st.link_ended)
            && stage.is_some()
            && let Some(was) = self.st.attempt.take()
        {
            if self.st.attempt_ready {
                c.lost = c.lost.wrapping_add(1);
            } else {
                let k = usize::from(was).min(c.failed.len() - 1);
                c.failed[k] = c.failed[k].wrapping_add(1);
            }
        }
        match (stage, self.st.attempt) {
            (Some(s), None) => {
                c.attempts = c.attempts.wrapping_add(1);
                self.st.attempt = Some(s);
                self.st.attempt_ready = false;
            }
            (Some(s), Some(was)) => self.st.attempt = Some(was.max(s)),
            (None, Some(was)) => {
                self.st.attempt = None;
                if self.st.attempt_ready {
                    c.lost = c.lost.wrapping_add(1);
                } else {
                    let k = usize::from(was).min(c.failed.len() - 1);
                    c.failed[k] = c.failed[k].wrapping_add(1);
                }
            }
            (None, None) => {}
        }
    }

    /// Hand the outputs the decoder wants (the Switch start-up talk, the
    /// Xbox battery question) to the host while the HID channels are open.
    fn outputs(&mut self, now_ms: u64, radio: &mut impl Radio) {
        let mut buf = [0_u8; SWITCH_PACKET_LEN];
        for _ in 0..OUTPUTS_PER_TICK {
            if !self.host.hid_ready() {
                return;
            }
            let Some(out) = self.st.dec.next_output(&mut buf) else {
                return;
            };
            let data = &buf[..out.len.min(buf.len())];
            let mut io = Glue {
                radio: &mut *radio,
                st: &mut self.st,
            };
            let sent = match out.channel {
                Channel::Interrupt => self.host.send_output_report(now_ms, data, &mut io),
                Channel::Control => self.host.send_set_report(now_ms, data, &mut io),
                Channel::GetReport => match data.first() {
                    Some(&id) => self.host.send_get_report(now_ms, id, &mut io),
                    None => false,
                },
            };
            if !sent {
                self.st.dec.output_not_sent();
                return;
            }
        }
    }
}

impl State {
    fn set_time(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        self.dec.set_time(now_ms as u32);
    }

    /// Looks at an event before pad-proto does, for what its UI events do
    /// not carry: the handle of our link and why it ended. `linked`: the
    /// host has a link already (then a new link is not ours).
    fn peek(&mut self, p: &[u8], linked: bool) {
        if p.first() != Some(&0x04) {
            return;
        }
        match hci::parse_event(&p[1..]) {
            Some(hci::Event::ConnectionComplete {
                status: 0, handle, ..
            }) if !linked => {
                self.link_handle = Some(handle);
                self.reason = None;
            }
            Some(hci::Event::DisconnectionComplete {
                status: 0,
                handle,
                reason,
            }) if self.link_handle == Some(handle) => self.reason = Some(reason),
            _ => {}
        }
    }

    fn push(&mut self, e: PadEvent) {
        if self.events[EVENTS - 1].is_some() {
            self.events.rotate_left(1);
            self.events[EVENTS - 1] = None;
            self.events_lost = self.events_lost.wrapping_add(1);
        }
        if let Some(slot) = self.events.iter_mut().find(|s| s.is_none()) {
            *slot = Some(e);
        }
        self.wake = true;
    }

    /// New presses since the last look, into the latch.
    fn latch(&mut self) {
        if !self.connected {
            return;
        }
        let b = self.dec.pad.buttons & board_api::btn::ALL;
        self.pressed |= b & !self.last_buttons;
        self.last_buttons = b;
    }

    /// After each report: presses, change, battery, the first report.
    fn on_report(&mut self, bytes: &[u8], radio: &mut impl Radio) {
        let ok = self.dec.report(bytes);
        self.quiet_since = self.now_ms;
        self.reports = self.reports.wrapping_add(1);
        let c = &mut self.counters;
        c.reports = c.reports.wrapping_add(1);
        if !ok {
            self.undecoded = self.undecoded.wrapping_add(1);
            c.undecoded = c.undecoded.wrapping_add(1);
            return;
        }
        if let Some(t) = self.last_report_ms {
            let gap = self.now_ms.saturating_sub(t).min(u64::from(u32::MAX)) as u32;
            c.gap_max_ms = c.gap_max_ms.max(gap);
        }
        self.last_report_ms = Some(self.now_ms);
        if !self.connected {
            self.connected = true;
            self.last_buttons = 0;
            if !self.injecting && !self.attempt_ready {
                self.attempt_ready = true;
                c.ready = c.ready.wrapping_add(1);
            }
        }
        if !self.announced {
            self.announced = true;
            let mode = decoder::mode_of(self.dec.layout, self.dec.name.as_str());
            let name = Name::from(self.dec.name.as_str());
            radio.log(
                "PAD-HOST",
                format_args!(
                    "connected mode={} layout={} because {} name=\"{}\" report_id={:#04x} len={}",
                    mode.word(),
                    self.dec.layout.name(),
                    self.dec.reason,
                    name,
                    bytes[0],
                    bytes.len()
                ),
            );
            self.push(PadEvent::Connected { mode, name });
        }
        // Every report: a short press (down and up between two ticks) is
        // caught here.
        self.latch();
        if !self.streaming
            && !self.injecting
            && self.dec.layout == Layout::SwitchPro
            && bytes.first() == Some(&0x30)
        {
            self.streaming = true;
            radio.log(
                "PAD-HOST",
                format_args!("full reports flow: silence guard on ({} ms)", SILENCE_MS),
            );
        } else if self.streaming && bytes.first() == Some(&0x3f) {
            // Back to the simple reports, sent only on change: quiet is
            // no longer a sign of a pad gone.
            self.streaming = false;
            radio.log(
                "PAD-HOST",
                format_args!("simple reports again: silence guard off"),
            );
        }
        if !self.motion_seen
            && self.dec.layout == Layout::SwitchPro
            && self.dec.pad.motion.is_some()
        {
            self.motion_seen = true;
            radio.log(
                "PAD-HOST",
                format_args!("motion on (report n={})", self.reports),
            );
        }
        let now = map::pad_state(&self.dec.pad);
        if map::changed(&self.seen, &now) {
            self.seen = now;
            self.wake = true;
        }
        self.battery_check(radio);
    }

    /// A message on the HID control channel (the Xbox battery answer, the
    /// Switch start-up handshakes).
    fn on_control(&mut self, msg: &[u8], radio: &mut impl Radio) {
        self.dec.control(msg);
        if self.connected {
            self.battery_check(radio);
        }
    }

    /// A new battery level or charging state: log line and event.
    fn battery_check(&mut self, radio: &mut impl Radio) {
        let bat = map::battery(self.dec.layout, self.dec.pad.battery);
        if bat.is_some() && bat != self.battery {
            self.battery = bat;
            if let Some(b) = bat {
                radio.log(
                    "PAD-HOST",
                    format_args!(
                        "battery percent={} level={} charging={}",
                        b.percent,
                        b.level,
                        u8::from(b.charging)
                    ),
                );
                self.push(PadEvent::Battery(b));
            }
        }
    }

    fn on_gone(&mut self, radio: &mut impl Radio) {
        let reason = self.reason.take().unwrap_or(0x16);
        self.link_handle = None;
        radio.log(
            "PAD-HOST",
            format_args!(
                "gone reason={:#04x}({}) connected={} reports={} undecoded={}",
                reason,
                hci::status_text(reason),
                u8::from(self.connected),
                self.reports,
                self.undecoded
            ),
        );
        if self.announced {
            self.push(PadEvent::Disconnected { reason });
        }
        self.dec.disconnected();
        self.connected = false;
        self.announced = false;
        self.motion_seen = false;
        self.pressed = 0;
        self.last_buttons = 0;
        self.battery = None;
        self.reports = 0;
        self.undecoded = 0;
        self.last_report_ms = None;
        self.streaming = false;
        self.link_ended = true;
        if self.seen != PadState::NONE {
            self.seen = PadState::NONE;
            self.wake = true;
        }
    }

    /// The Switch start-up talk's notes, into the log.
    fn notes(&mut self, radio: &mut impl Radio) {
        // At most a few per tick; the talk makes about one per packet.
        for _ in 0..8 {
            let Some(n) = self.dec.startup.take_note() else {
                return;
            };
            radio.log("PAD-HOST", format_args!("switch start-up: {}", n));
        }
    }
}

/// pad-proto's way out, joined to the radio and the decoder.
struct Glue<'a, R: Radio> {
    radio: &'a mut R,
    st: &'a mut State,
}

impl<R: Radio> Io for Glue<'_, R> {
    fn send(&mut self, packet: &[u8]) {
        self.radio.send(packet);
    }

    fn ui(&mut self, event: UiEvent<'_>) {
        match event {
            UiEvent::Status(s) => self.radio.log("PAD-HOST", format_args!("stage: {}", s)),
            UiEvent::Device(d) => self.st.dec.device(d.name, d.vendor_id, d.product_id),
            UiEvent::Descriptor(d) => self.st.dec.descriptor(d),
            UiEvent::Report(r) => self.st.on_report(r, self.radio),
            UiEvent::Control(m) => self.st.on_control(m, self.radio),
            UiEvent::Disconnected => self.st.on_gone(self.radio),
        }
    }

    fn log(&mut self, tag: Tag, args: fmt::Arguments<'_>) {
        self.radio.log(tag.name(), args);
    }

    fn key_change(&mut self, change: KeyChange) {
        // Reconnect-only never changes keys; if it ever did, nothing is
        // stored, and the log says so without the key or the address.
        let what = match change {
            KeyChange::Save { .. } => "save",
            KeyChange::Forget { .. } => "forget",
        };
        let mut t: Text<8> = Text::new();
        let _ = t.write_str(what);
        self.radio.log(
            "PAD-ERR",
            format_args!(
                "key change '{}' ignored (reconnect-only, flash is read only)",
                t.as_str()
            ),
        );
    }
}
