//! pad-view: understands what a Bluetooth gamepad sends and draws it.
//!
//! The firmware hands the [`View`] everything it learns: what it is doing
//! ([`View::status`]), who the gamepad is ([`View::device`]), the gamepad's
//! own description of its reports ([`View::descriptor`]) and every report
//! ([`View::report`]). [`View::draw`] then draws what changed on any
//! `embedded-graphics` target of 320x240 pixels in RGB565.
//!
//! Some gamepads only send full reports after the host asks;
//! [`View::next_output`] gives the packets to send for that.
//!
//! Nothing the pad sends stays invisible: bits and fields the decoder does
//! not map to the standard gamepad are tracked ([`unmapped`]) and shown on
//! the Pad and Data pages, and [`View::last_change`] with a
//! [`trace::Tracer`] tells a firmware which reports to log.
//!
//! Nothing here allocates memory and nothing panics on bad input. See
//! README.md for how a firmware should call it, and MODES.md for what the
//! GuliKit KingKong 2 Pro sends in each of its four modes.

#![no_std]

pub mod battery;
pub mod decode;
pub mod descriptor;
mod draw;
pub mod motion;
pub mod startup;
pub mod text;
pub mod trace;
pub mod unmapped;

use core::fmt::Write;
use decode::{Hints, Layout, Pad};
use descriptor::Descriptor;

use startup::Startup;
use text::Line;
use unmapped::{Item, Tracker};

pub use draw::{HEIGHT, WIDTH};

/// What the firmware knows about the gamepad.
#[derive(Clone, Copy, Debug, Default)]
pub struct Device<'a> {
    /// From the inquiry answer or a name request; may be empty.
    pub name: &'a str,
    /// The 24-bit Bluetooth class of device.
    pub class_of_device: u32,
    /// From the SDP Device ID record, if any.
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub version: Option<u16>,
    /// 1 = Bluetooth SIG, 2 = USB Implementers Forum.
    pub vendor_id_source: Option<u16>,
}

/// Longest report kept whole. Longer reports are counted with their real
/// length, but only this many bytes are kept and shown.
pub const REPORT_KEEP: usize = 64;
/// How many different report ids are remembered for the Data page.
const IDS_KEPT: usize = 6;
/// Bytes kept per report id for the Data page.
const ID_BYTES: usize = 24;
/// Bytes of the descriptor kept to show on the About page.
const DESC_HEAD: usize = 32;
/// Shortest time between two motion changes on the Data page.
const DATA_MOTION_GAP_MS: u32 = 250;
/// Shortest time between two changes of the unmapped input shown on the
/// Pad and Data pages. A noisy unmapped value then redraws at most 4
/// times a second; a press shorter than this still shows, for one period
/// (see [`unmapped::Tracker::relatch`]).
const UNMAPPED_GAP_MS: u32 = 250;
/// Unmapped things kept for the screen.
pub const UNMAPPED_SHOWN: usize = 8;

const ITEM0: Item = Item {
    id: 0,
    byte: 0,
    bit: 0,
    bits: 1,
    value: 0,
    how: unmapped::How::Undeclared,
    page: 0,
    usage: 0,
};

/// The last report seen with one report id.
#[derive(Clone, Copy)]
struct IdRaw {
    id: u8,
    len: u16,
    count: u32,
    bytes: [u8; ID_BYTES],
}

const ID_RAW0: IdRaw = IdRaw {
    id: 0,
    len: 0,
    count: 0,
    bytes: [0; ID_BYTES],
};

/// The pages, switched by [`View::next_page`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Controller,
    Data,
    About,
}

/// Everything the screen shows. About 7 KB: keep it in a `static` (for
/// example in a `static_cell::StaticCell`) rather than on a small stack.
pub struct View {
    status: Line<40>,
    /// A message shown instead of the status until `notice_until` (in
    /// the time of [`View::set_time_ms`]).
    notice: Line<40>,
    notice_until: Option<u32>,
    /// One line about saved pairings for the About page (the firmware
    /// writes it; empty: no line).
    pairings: Line<54>,
    name: Line<40>,
    class_of_device: u32,
    vendor_id: Option<u16>,
    product_id: Option<u16>,
    version: Option<u16>,
    vendor_id_source: Option<u16>,
    have_device: bool,

    desc: Descriptor,
    desc_len: usize,
    desc_head: [u8; DESC_HEAD],

    layout: Layout,
    reason: &'static str,
    pad: Pad,
    /// True between the first report and disconnected().
    live: bool,
    reports: u32,
    decoded: u32,
    last: [u8; REPORT_KEEP],
    last_len: usize,
    ids: [IdRaw; IDS_KEPT],
    ids_n: usize,

    now_ms: Option<u32>,
    rate_start_ms: Option<u32>,
    rate_count: u32,
    /// Reports per second times ten.
    rate_x10: Option<u32>,
    last_report_ms: Option<u32>,

    startup: Startup,
    /// The battery question to an Xbox-mode pad (see [`battery`]).
    battery_ask: battery::Ask,
    /// The packet last given by next_output was the battery question.
    battery_out: bool,
    /// The motion values on the Pad and About pages, changed at most 10
    /// times a second.
    motion_gate: motion::Gate,
    /// The motion values on the Data page, changed at most 4 times a
    /// second: one change there redraws about 100 letters (about 7 ms on
    /// the board), so 10 a second would take 7% of the time.
    motion_gate_data: motion::Gate,

    /// Input the decoder does not map, and what the screen shows of it
    /// (changed at most every UNMAPPED_GAP_MS).
    unmapped: Tracker,
    un_shown: [Item; UNMAPPED_SHOWN],
    un_shown_n: usize,
    un_shown_total: usize,
    un_at_ms: Option<u32>,
    un_hash: u32,
    /// How many times the shown unmapped list changed.
    un_changes: u32,
    /// What the last report changed (for the serial trace).
    change: trace::Change,

    page: Page,
    screen: draw::Screen,
}

impl Default for View {
    fn default() -> Self {
        Self::new()
    }
}

impl View {
    pub const fn new() -> Self {
        View {
            status: Line::new(),
            notice: Line::new(),
            notice_until: None,
            pairings: Line::new(),
            name: Line::new(),
            class_of_device: 0,
            vendor_id: None,
            product_id: None,
            version: None,
            vendor_id_source: None,
            have_device: false,
            desc: Descriptor::empty(),
            desc_len: 0,
            desc_head: [0; DESC_HEAD],
            layout: Layout::Unknown,
            reason: "nothing known yet",
            pad: Pad::new(),
            live: false,
            reports: 0,
            decoded: 0,
            last: [0; REPORT_KEEP],
            last_len: 0,
            ids: [ID_RAW0; IDS_KEPT],
            ids_n: 0,
            now_ms: None,
            rate_start_ms: None,
            rate_count: 0,
            rate_x10: None,
            last_report_ms: None,
            startup: Startup::new(),
            battery_ask: battery::Ask::NotAsked,
            battery_out: false,
            motion_gate: motion::Gate::new(),
            motion_gate_data: motion::Gate::with_gap(DATA_MOTION_GAP_MS),
            unmapped: Tracker::new(),
            un_shown: [ITEM0; UNMAPPED_SHOWN],
            un_shown_n: 0,
            un_shown_total: 0,
            un_at_ms: None,
            un_hash: 0,
            un_changes: 0,
            change: trace::Change {
                id: 0,
                len: 0,
                same: false,
                first: false,
                digital: false,
                analog: false,
            },
            page: Page::Controller,
            screen: draw::Screen::new(),
        }
    }

    /// One short line about what the console is doing ("Searching",
    /// "Pairing", ...). Cut to 40 characters.
    pub fn status(&mut self, line: &str) {
        self.status.set(line);
    }

    /// A message that stays in place of the status line for `for_ms`
    /// milliseconds, even when the status changes meanwhile (for example
    /// "All pads forgotten"). Needs [`View::set_time_ms`]; without a time
    /// it is never shown. Cut to 40 characters.
    pub fn notice(&mut self, line: &str, for_ms: u32) {
        self.notice.set(line);
        self.notice_until = self.now_ms.map(|t| t.wrapping_add(for_ms));
    }

    /// The text shown in the status place now: the notice while it lasts,
    /// otherwise the status.
    pub fn shown_status(&self) -> &str {
        match (self.notice_until, self.now_ms) {
            // Wrapping-safe "now is before until".
            (Some(until), Some(now)) if (until.wrapping_sub(now) as i32) > 0 => {
                self.notice.as_str()
            }
            _ => self.status.as_str(),
        }
    }

    /// The line about saved pairings on the About page, such as "Known
    /// pads: 2 (saved)". Cut to 54 characters. Kept by [`View::reset`].
    pub fn pairings(&mut self, line: &str) {
        self.pairings.set(line);
    }

    /// Who the gamepad is. Calling it again (a new gamepad) is fine.
    pub fn device(&mut self, d: &Device<'_>) {
        let new_name = d.name != self.name.as_str()
            || d.vendor_id != self.vendor_id
            || d.product_id != self.product_id;
        self.name.set(d.name);
        self.class_of_device = d.class_of_device & 0xFF_FFFF;
        self.vendor_id = d.vendor_id;
        self.product_id = d.product_id;
        self.version = d.version;
        self.vendor_id_source = d.vendor_id_source;
        if !self.have_device {
            self.have_device = true;
            self.screen.invalidate();
        }
        if new_name {
            self.rechoose();
        }
    }

    /// The HID report descriptor read over SDP.
    pub fn descriptor(&mut self, bytes: &[u8]) {
        // A new description: nothing decoded with the old one stays (an
        // analog trigger from another mode would keep its bar analog).
        self.pad = Pad::new();
        self.unmapped.clear();
        self.desc.parse_in_place(bytes);
        self.desc_len = bytes.len();
        let n = bytes.len().min(DESC_HEAD);
        self.desc_head[..n].copy_from_slice(&bytes[..n]);
        self.rechoose();
    }

    /// One input report as it arrived on the interrupt channel, without
    /// the leading 0xA1 byte.
    pub fn report(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if !self.live {
            self.live = true;
            self.screen.invalidate_live();
        }
        self.reports = self.reports.wrapping_add(1);
        let n = bytes.len().min(REPORT_KEEP);
        self.last[..n].copy_from_slice(&bytes[..n]);
        self.last_len = bytes.len();
        self.remember_id(bytes);
        if self.layout == Layout::Unknown {
            self.layout_from(Some(bytes));
        }
        let before = (self.pad.buttons, self.pad.extra, self.pad.hat);
        if decode::decode(self.layout, &self.desc, bytes, &mut self.pad) {
            self.decoded = self.decoded.wrapping_add(1);
        }
        let key = self.key_of(bytes);
        let seen = self.unmapped.report(self.layout, &self.desc, key, bytes);
        let digital = before != (self.pad.buttons, self.pad.extra, self.pad.hat) || seen.on_changed;
        self.change = trace::Change {
            id: bytes[0],
            len: bytes.len(),
            same: seen.same,
            first: seen.first,
            digital,
            analog: !seen.same && !seen.first && !digital,
        };
        self.startup.on_report(bytes);
        self.offer_motion();
        // Rate: count reports over windows of about a second.
        if let Some(now) = self.now_ms {
            self.last_report_ms = Some(now);
            match self.rate_start_ms {
                None => {
                    self.rate_start_ms = Some(now);
                    self.rate_count = 0;
                }
                Some(start) => {
                    self.rate_count += 1;
                    let span = now.wrapping_sub(start);
                    if span >= 1000 {
                        self.rate_x10 =
                            Some((self.rate_count as u64 * 10_000 / span as u64) as u32);
                        self.rate_start_ms = Some(now);
                        self.rate_count = 0;
                    }
                }
            }
        }
    }

    /// The link is gone: keep the device text, grey out the live values.
    pub fn disconnected(&mut self) {
        self.live = false;
        self.rate_start_ms = None;
        self.rate_x10 = None;
        self.startup.reset();
        // Ask again on the next link. A level already read stays shown.
        self.battery_ask = battery::Ask::NotAsked;
        self.battery_out = false;
        self.unmapped.clear();
        self.rechoose();
        self.screen.invalidate_live();
    }

    /// Forget everything (a different gamepad is coming). The next draw()
    /// redraws the whole screen. Not needed when the same gamepad comes
    /// back: then just call device(), descriptor() and report() again.
    pub fn reset(&mut self) {
        let pairings = self.pairings.clone();
        let notice = self.notice.clone();
        let notice_until = self.notice_until;
        *self = View::new();
        self.pairings = pairings;
        self.notice = notice;
        self.notice_until = notice_until;
    }

    /// Show the next page (Controller, Data, About, then round again).
    pub fn next_page(&mut self) {
        self.page = match self.page {
            Page::Controller => Page::Data,
            Page::Data => Page::About,
            Page::About => Page::Controller,
        };
        self.screen.invalidate();
    }

    /// The page on screen.
    pub fn page(&self) -> Page {
        self.page
    }

    /// Tell the view the time, in milliseconds from any starting point.
    /// Optional: without it the report rate shows "--" and the start-up
    /// conversation retries only by counting reports. Call it before
    /// [`View::report`] and [`View::draw`], for example with
    /// `embassy_time::Instant::now().as_millis() as u32`.
    pub fn set_time_ms(&mut self, now_ms: u32) {
        self.now_ms = Some(now_ms);
        self.startup.set_time(now_ms);
        // No report for two seconds: the rate is zero.
        let quiet = self
            .last_report_ms
            .is_some_and(|t| now_ms.wrapping_sub(t) > 2000);
        if quiet && self.live && self.rate_x10 != Some(0) {
            self.rate_x10 = Some(0);
            self.rate_start_ms = None;
        }
        // The battery question had no answer in time.
        if let battery::Ask::Waiting { since: Some(t) } = self.battery_ask
            && now_ms.wrapping_sub(t) > battery::WAIT_MS
        {
            self.battery_ask = battery::Ask::NoAnswer;
        }
    }

    /// An output report to send now, without the HID header byte,
    /// written into `buf` (give it 49 bytes or more; a shorter buffer
    /// gets nothing). Returns its length and the channel to send it on:
    /// the interrupt channel (header 0xA2) or the control channel as a
    /// SET_REPORT (header 0x52). None when there is nothing to send.
    ///
    /// For an Xbox-mode pad it also gives, once per link after the first
    /// report, the battery question: one byte (the report id 04) on
    /// [`startup::Channel::GetReport`].
    pub fn next_output(&mut self, buf: &mut [u8]) -> Option<startup::Output> {
        self.battery_out = false;
        if let Some(out) = self.startup.next_output(buf) {
            return Some(out);
        }
        let due =
            self.layout == Layout::Xbox && self.live && self.battery_ask == battery::Ask::NotAsked;
        if due && !buf.is_empty() {
            buf[0] = battery::REPORT_ID;
            self.battery_ask = battery::Ask::Waiting { since: self.now_ms };
            self.battery_out = true;
            return Some(startup::Output {
                len: 1,
                channel: startup::Channel::GetReport,
            });
        }
        None
    }

    /// The output report last given by [`View::next_output`] could not be
    /// sent (no open HID channel yet). It is not counted as a try; the
    /// next call to [`View::next_output`] gives it again.
    pub fn output_not_sent(&mut self) {
        if self.battery_out {
            self.battery_out = false;
            self.battery_ask = battery::Ask::NotAsked;
        } else {
            self.startup.not_sent();
        }
    }

    /// One message from the HID control channel, header byte first: a
    /// HANDSHAKE (the answer to a SET_REPORT, or an error code for a
    /// GET_REPORT) or a DATA message. The start-up conversation and the
    /// battery question look at it.
    pub fn control(&mut self, msg: &[u8]) {
        let Some(&h) = msg.first() else { return };
        let waiting = matches!(self.battery_ask, battery::Ask::Waiting { .. });
        match h >> 4 {
            // While the battery question waits, a HANDSHAKE is its answer:
            // an Xbox-mode pad gets no SET_REPORT from the start-up
            // conversation.
            0x0 if waiting => self.battery_ask = battery::Ask::Refused(h & 0x0F),
            0x0 => self.startup.on_handshake(h & 0x0F),
            0xA => {
                if waiting
                    && msg.get(1) == Some(&battery::REPORT_ID)
                    && let Some(&b) = msg.get(2)
                {
                    self.pad.battery = Some(b);
                    self.battery_ask = battery::Ask::Answered;
                }
                self.startup.on_report(&msg[1..]);
            }
            _ => {}
        }
    }

    /// Where the battery question stands (for logs and tests).
    pub fn battery_ask(&self) -> battery::Ask {
        self.battery_ask
    }

    /// The oldest start-up note not yet taken, for the log (see
    /// [`startup::Note`]).
    pub fn take_startup_note(&mut self) -> Option<startup::Note> {
        self.startup.take_note()
    }

    /// The start-up conversation's progress (Switch mode), for logs.
    pub fn startup(&self) -> &Startup {
        &self.startup
    }

    /// The decoded standard gamepad state (for logs and tests).
    pub fn pad(&self) -> &Pad {
        &self.pad
    }

    /// The motion values the screen shows: the newest ones, but changed
    /// at most 10 times a second and only when they moved (see
    /// [`motion::Gate`]). None when there is no motion data.
    pub fn motion_shown(&self) -> Option<&motion::Motion> {
        self.motion_gate.shown()
    }

    /// How many times the shown motion values changed (for tests and
    /// logs).
    pub fn motion_changes(&self) -> u32 {
        self.motion_gate.changes
    }

    /// The motion values the Data page shows (changed at most 4 times a
    /// second), and how many times they changed.
    pub fn motion_shown_data(&self) -> (Option<&motion::Motion>, u32) {
        (self.motion_gate_data.shown(), self.motion_gate_data.changes)
    }

    /// Hand the newest motion values (Switch layout only) to both gates.
    fn offer_motion(&mut self) {
        let latest = if self.layout == Layout::SwitchPro {
            self.pad.motion
        } else {
            None
        };
        self.motion_gate.offer(self.now_ms, latest.as_ref());
        self.motion_gate_data.offer(self.now_ms, latest.as_ref());
    }

    /// What the last report changed, for the serial trace (see
    /// [`trace::Tracer`]).
    pub fn last_change(&self) -> trace::Change {
        self.change
    }

    /// The held buttons by name ("A,LB,Up,B13"), or "-" when none.
    pub fn write_held<W: Write>(&self, w: &mut W) {
        let face = self.layout.face();
        let mut any = false;
        for b in decode::Btn::ALL {
            if self.pad.pressed(b) {
                let _ = write!(w, "{}{}", if any { "," } else { "" }, b.name(face));
                any = true;
            }
        }
        for n in 0..32 {
            if self.pad.extra & (1 << n) != 0 {
                let _ = write!(w, "{}B{}", if any { "," } else { "" }, n + 1);
                any = true;
            }
        }
        if !any {
            let _ = w.write_char('-');
        }
    }

    /// The unmapped things on now (not limited in time, unlike the
    /// screen), labels joined by `sep`, or nothing when none.
    pub fn write_unmapped<W: Write>(&self, w: &mut W, sep: &str) {
        let mut items = [ITEM0; UNMAPPED_SHOWN];
        let total = self.unmapped.items(&self.desc, false, &mut items);
        for (k, it) in items[..total.min(UNMAPPED_SHOWN)].iter().enumerate() {
            if k > 0 {
                let _ = w.write_str(sep);
            }
            it.label(w);
        }
        if total > UNMAPPED_SHOWN {
            let _ = write!(w, "{}+{} more", sep, total - UNMAPPED_SHOWN);
        }
    }

    /// The unmapped things the screen shows now, and how many there are
    /// in all (more than the slice when some did not fit).
    pub fn unmapped_shown(&self) -> (&[Item], usize) {
        (&self.un_shown[..self.un_shown_n], self.un_shown_total)
    }

    /// How many times the unmapped list on screen changed, and how many
    /// reports changed what is unmapped and on.
    pub fn unmapped_changes(&self) -> (u32, u32) {
        (self.un_changes, self.unmapped.changes)
    }

    /// True when both triggers are known to be only on or off (Switch
    /// layout, or a descriptor with trigger buttons and no trigger axes).
    pub fn triggers_on_off(&self) -> bool {
        let t = |t: &decode::Trigger| t.present && !t.analog;
        t(&self.pad.lt) && t(&self.pad.rt)
    }

    /// Take a new snapshot of the unmapped input for the screen, at most
    /// every UNMAPPED_GAP_MS (always without a clock).
    fn offer_unmapped(&mut self) {
        if let (Some(now), Some(at)) = (self.now_ms, self.un_at_ms)
            && now.wrapping_sub(at) < UNMAPPED_GAP_MS
        {
            return;
        }
        let mut items = [ITEM0; UNMAPPED_SHOWN];
        let total = self.unmapped.items(&self.desc, true, &mut items);
        let n = total.min(UNMAPPED_SHOWN);
        let mut h = text::fnv(&(total as u32).to_le_bytes());
        for it in &items[..n] {
            let parts = [it.id, it.byte, it.bit, it.bits];
            // A single bit shows no value: one that was on for a moment
            // and is off again is the same picture, and must not count as
            // a change (that would restart the gate and keep it up longer).
            let value = if it.bits > 1 { it.value } else { 0 };
            h = h.rotate_left(5) ^ text::fnv(&parts) ^ value;
        }
        self.unmapped.relatch();
        if h == self.un_hash && total == self.un_shown_total {
            return;
        }
        self.un_at_ms = self.now_ms;
        self.un_hash = h;
        self.un_shown = items;
        self.un_shown_n = n;
        self.un_shown_total = total;
        self.un_changes = self.un_changes.wrapping_add(1);
    }

    /// The layout in use and why it was chosen.
    pub fn layout(&self) -> (Layout, &'static str) {
        (self.layout, self.reason)
    }

    /// The parsed descriptor (empty when none was given).
    pub fn parsed_descriptor(&self) -> &Descriptor {
        &self.desc
    }

    /// A guess of which of the GuliKit KingKong 2 Pro's four modes this is.
    pub fn mode_guess(&self) -> &'static str {
        let name = self.name.as_str();
        let has = |s: &str| {
            name.as_bytes()
                .windows(s.len())
                .any(|w| w.eq_ignore_ascii_case(s.as_bytes()))
        };
        match self.layout {
            Layout::SwitchPro => "Switch mode",
            _ if has("XW") => "XInput (Windows) mode",
            _ if has("Xbox Wireless") => "Android/iOS mode (or a real Xbox)",
            Layout::Xbox => "XInput or Android/iOS mode",
            Layout::Unknown => "not known yet",
            _ if has("GuliKit") => "DInput mode (guess)",
            _ => "not a GuliKit mode I know",
        }
    }

    /// The report id of a report: its first byte, unless the descriptor
    /// says the pad uses no ids (then 0).
    fn key_of(&self, bytes: &[u8]) -> u8 {
        if !self.desc.is_empty() && !self.desc.uses_report_ids {
            0
        } else {
            bytes.first().copied().unwrap_or(0)
        }
    }

    fn remember_id(&mut self, bytes: &[u8]) {
        let id = self.key_of(bytes);
        let pos = self.ids[..self.ids_n].iter().position(|r| r.id == id);
        let i = match pos {
            Some(i) => i,
            None if self.ids_n < IDS_KEPT => {
                self.ids_n += 1;
                self.ids_n - 1
            }
            // Full: reuse the last slot.
            None => IDS_KEPT - 1,
        };
        let e = &mut self.ids[i];
        if e.id != id {
            e.count = 0;
        }
        e.id = id;
        e.len = bytes.len().min(u16::MAX as usize) as u16;
        e.count = e.count.wrapping_add(1);
        let n = bytes.len().min(ID_BYTES);
        e.bytes[..n].copy_from_slice(&bytes[..n]);
    }

    fn rechoose(&mut self) {
        let first = if self.last_len > 0 {
            Some(self.last_len.min(REPORT_KEEP))
        } else {
            None
        };
        match first {
            Some(n) => {
                let copy = self.last;
                self.layout_from(Some(&copy[..n]));
            }
            None => self.layout_from(None),
        }
    }

    fn layout_from(&mut self, report: Option<&[u8]>) {
        let hints = Hints {
            name: self.name.as_str(),
            vendor_id: self.vendor_id,
            product_id: self.product_id,
        };
        let (layout, reason) = decode::choose(&hints, &self.desc, report);
        if layout != self.layout {
            self.screen.invalidate();
            self.unmapped.clear();
        }
        self.layout = layout;
        self.reason = reason;
        // Triggers that are only buttons: the bars show on/off from the
        // start, before the first press.
        if decode::digital_triggers(layout, &self.desc) {
            for t in [&mut self.pad.lt, &mut self.pad.rt] {
                if !t.present {
                    *t = decode::Trigger {
                        raw: 0,
                        value: 0,
                        analog: false,
                        present: true,
                    };
                }
            }
        }
        self.startup.active = layout == Layout::SwitchPro;
    }
}
