//! The point where the screen plugs into the Bluetooth host.
//!
//! Every event meant for the screen goes through [`UiSink::event`]. The
//! sink in use, [`ViewSink`], prints a line for most events and hands
//! each one to pad-view's `View` (the picture of the gamepad and all its
//! data). The drawing itself happens elsewhere (`screen.rs`), on a timer.
//!
//! Rules for a sink:
//!
//! - `event` is called on the radio loop, from inside the Bluetooth host,
//!   while a packet is being handled. It must return quickly: no drawing,
//!   no SPI, no waiting. Giving a report to the `View` only decodes it
//!   into a few numbers (a few microseconds, measured: `event_us_max` in
//!   the PAD-UI line); drawing happens later.
//! - The borrowed data (names, descriptor, report) is valid only during
//!   the call.
//! - `next_output` is asked after every `Device`, `Descriptor`, `Report`
//!   and `Control` event, and every 20 ms, but only once both HID
//!   channels to the gamepad are open, until it says `None` (at most 4
//!   times in a row). Each report it gives is sent on the channel it
//!   names: the HID interrupt channel (the host puts 0xA2 in front) or
//!   the control channel as a SET_REPORT (the host puts 0x52 in front).
//!   Then `output_sent` is called, or, if the host could not send it
//!   after all, `output_not_sent`: the report is given back and asked
//!   for again later, and it does not count as a try.
//! - The Switch start-up conversation (pad-view's prober) leaves notes;
//!   each is logged as `PAD-STATE ui switch start-up: <note>` right
//!   after the event or send that caused it.
//!
//! No queue of reports: each report is decoded into the `View` at once,
//! so the screen always draws the newest state, every report is counted,
//! and nothing piles up when the screen is slower than the gamepad.
//!
//! The button trace: pad-view's `Tracer` picks the reports worth a line
//! (every digital change, an identical report or an analog move after a
//! silence, everything during a full trace), at most 40 lines a second.
//! Each one is logged here as `PAD-HID change|same|analog|trace n= id=
//! len= gap= hex= held= unmapped=`. Lines the cap skipped are logged as
//! `PAD-HID trace skipped=N` once their second is over. The serial key
//! "t" starts a full trace of 10 s ([`start_full_trace`]).

use core::cell::{Cell, RefCell};
use core::fmt;

use pad_proto::UiEvent;
use pad_proto::buf::{Hex, Printable};
use pad_view::View;
use pad_view::startup::{self, Output};
use pad_view::trace::{self, Tracer};

pub trait UiSink {
    /// One event for the screen. See the module rules: be quick.
    fn event(&mut self, ev: &UiEvent<'_>);

    /// An output report for the gamepad (without the 0xA2 byte), written
    /// into `buf`; returns its length. `None` when there is nothing to
    /// send.
    fn next_output(&mut self, buf: &mut [u8]) -> Option<Output> {
        let _ = buf;
        None
    }

    /// The last report from `next_output` was queued by the host.
    fn output_sent(&mut self) {}

    /// The last report from `next_output` did not reach the gamepad.
    fn output_not_sent(&mut self) {}

    /// The host just printed a PAD-SUMMARY line; add the screen's side.
    fn summary(&mut self) {}
}

/// What the radio loop and the screen loop share. Both run in the same
/// task, one after the other, never at the same time: a borrow of the
/// `View` is never held across a wait, so the `RefCell` can never be
/// found borrowed.
pub struct Shared {
    pub view: RefCell<View>,
    /// Reports given to the view since start.
    pub reports: Cell<u32>,
    /// Longest time one event (status, device, descriptor, report) took
    /// inside the sink, in microseconds.
    pub event_us_max: Cell<u32>,
    /// Longest time between two returns of the radio loop's wait, in
    /// microseconds: since the last PAD-UI line, and since start. Gaps
    /// during a screenshot are kept apart (`gap_shot_us_max`).
    pub gap_us_max: Cell<u32>,
    pub gap_us_max_all: Cell<u32>,
    pub gap_shot_us_max: Cell<u32>,
    /// Gaps longer than 50 ms (outside screenshots).
    pub gaps_over_50ms: Cell<u32>,
    /// How late the radio loop's 20 ms timer fired, at most, in
    /// microseconds (since the last PAD-UI line).
    pub tick_late_us_max: Cell<u32>,
    /// A screenshot is going out now; and how many have started.
    pub shot_busy: Cell<bool>,
    pub shots: Cell<u32>,
    /// The host's phase name and whether a gamepad is linked, for the
    /// debug link's stats.
    pub phase: Cell<&'static str>,
    pub linked: Cell<bool>,
    /// The serial key "t" asked for a full trace; the sink starts it.
    pub full_trace_asked: Cell<bool>,
    /// The serial key "f" or a 5 s BOOT hold asked to forget every
    /// pairing; the radio loop does it.
    pub forget_all_asked: Cell<bool>,
}

impl Shared {
    pub const fn new() -> Self {
        Self {
            view: RefCell::new(View::new()),
            reports: Cell::new(0),
            event_us_max: Cell::new(0),
            gap_us_max: Cell::new(0),
            gap_us_max_all: Cell::new(0),
            gap_shot_us_max: Cell::new(0),
            gaps_over_50ms: Cell::new(0),
            tick_late_us_max: Cell::new(0),
            shot_busy: Cell::new(false),
            shots: Cell::new(0),
            phase: Cell::new("init"),
            linked: Cell::new(false),
            full_trace_asked: Cell::new(false),
            forget_all_asked: Cell::new(false),
        }
    }
}

fn now_us() -> u64 {
    esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_micros()
}

fn now_ms32() -> u32 {
    (now_us() / 1000) as u32
}

/// Starts a full button trace: every report gets a line for the next
/// 10 s (still at most 40 a second). Called from the serial key "t".
pub fn start_full_trace(s: &Shared) {
    s.full_trace_asked.set(true);
    crate::out::line(
        "PAD-HID",
        format_args!(
            "trace start: every report for {} s, at most {} lines a second",
            trace::FULL_MS / 1000,
            trace::MAX_LINES_PER_S
        ),
    );
}

/// The longest report part of a trace line (the rest is cut).
const TRACE_HEX: usize = 64;

/// The sink in use: logs, and feeds pad-view's `View`.
pub struct ViewSink {
    s: &'static Shared,
    /// A link was lost since the last Device event: if the next gamepad
    /// is a different one, the view forgets the old one.
    lost: bool,
    have: bool,
    name: heapless::String<40>,
    ids: (Option<u16>, Option<u16>),
    /// Which reports get a trace line.
    tracer: Tracer,
}

impl ViewSink {
    pub fn new(s: &'static Shared) -> Self {
        Self {
            s,
            lost: false,
            have: false,
            name: heapless::String::new(),
            ids: (None, None),
            tracer: Tracer::new(),
        }
    }

    /// The trace line for the report just given to the view, if the
    /// tracer wants one.
    fn trace(&mut self, v: &View, r: &[u8], now: u32) {
        if self.s.full_trace_asked.take() {
            self.tracer.start_full(now);
        }
        let c = v.last_change();
        let d = self.tracer.report(now, &c);
        if d.skipped > 0 {
            skipped_line(d.skipped);
        }
        if d.full_ended {
            crate::out::line("PAD-HID", format_args!("trace end: the full trace is over"));
        }
        let Some(kind) = d.kind else { return };
        let mut held: heapless::String<96> = heapless::String::new();
        v.write_held(&mut CutString(&mut held));
        let mut un: heapless::String<160> = heapless::String::new();
        v.write_unmapped(&mut CutString(&mut un), "|");
        let cut = r.len() > TRACE_HEX;
        crate::out::line(
            "PAD-HID",
            format_args!(
                "{} n={} id={:02x} len={} gap={} hex={}{} held={} unmapped=\"{}\"",
                kind.word(),
                self.s.reports.get(),
                c.id,
                c.len,
                Gap(d.gap_ms),
                HexMasked(&r[..r.len().min(TRACE_HEX)], startup::private_bytes(r)),
                if cut { ".." } else { "" },
                held,
                un
            ),
        );
    }

    /// Logs the Switch start-up notes the view left since the last look.
    fn log_notes(&mut self, v: &mut View) {
        while let Some(n) = v.take_startup_note() {
            line(format_args!("ui switch start-up: {}", n));
        }
    }

    fn device(&mut self, v: &mut View, d: &pad_proto::DeviceInfo<'_>) {
        // A different gamepad after a loss: start from a clean view, but
        // keep the status line.
        let other_name =
            !d.name.is_empty() && !self.name.is_empty() && d.name != self.name.as_str();
        let other_ids = d.vendor_id.is_some()
            && self.ids.0.is_some()
            && (d.vendor_id, d.product_id) != self.ids;
        if self.have && self.lost && (other_name || other_ids) {
            line(format_args!(
                "ui a different gamepad: the view starts afresh"
            ));
            v.reset();
            v.status("Another gamepad");
        }
        self.have = true;
        self.lost = false;
        if !d.name.is_empty() {
            self.name.clear();
            // Cut at 40 bytes, on a character boundary.
            for c in d.name.chars() {
                if self.name.push(c).is_err() {
                    break;
                }
            }
        }
        if d.vendor_id.is_some() {
            self.ids = (d.vendor_id, d.product_id);
        }
        // An incoming call may come with no name; keep the one we know.
        let name = if d.name.is_empty() {
            self.name.as_str()
        } else {
            d.name
        };
        v.device(&pad_view::Device {
            name,
            class_of_device: d.class_of_device,
            vendor_id: d.vendor_id,
            product_id: d.product_id,
            version: d.version,
            vendor_id_source: d.vendor_id_source,
        });
    }
}

impl UiSink for ViewSink {
    fn event(&mut self, ev: &UiEvent<'_>) {
        // The log first (as the earlier log-only sink did).
        match ev {
            UiEvent::Status(s) => line(format_args!("ui status=\"{}\"", s)),
            UiEvent::Device(d) => line(format_args!(
                "ui device name=\"{}\" class={:#08x} vendor={:04x?} product={:04x?} version={:04x?} vendor_source={:?}",
                Printable(d.name.as_bytes()),
                d.class_of_device,
                d.vendor_id,
                d.product_id,
                d.version,
                d.vendor_id_source
            )),
            UiEvent::Descriptor(b) => line(format_args!(
                "ui descriptor bytes={} first={}",
                b.len(),
                Hex(&b[..b.len().min(16)])
            )),
            // The host logs reports and control messages itself.
            UiEvent::Report(_) | UiEvent::Control(_) => {}
            UiEvent::Disconnected => line(format_args!(
                "ui disconnected after {} reports",
                self.s.reports.get()
            )),
        }

        let started = now_us();
        let Ok(mut v) = self.s.view.try_borrow_mut() else {
            // Cannot happen (see `Shared`); counted as a lost event.
            line(format_args!("ui view busy, event lost"));
            return;
        };
        match ev {
            UiEvent::Status(s) => v.status(s),
            UiEvent::Device(d) => self.device(&mut v, d),
            UiEvent::Descriptor(b) => {
                v.descriptor(b);
                let (layout, why) = v.layout();
                line(format_args!(
                    "ui layout={:?} because {} mode_guess=\"{}\"",
                    layout,
                    why,
                    v.mode_guess()
                ));
            }
            UiEvent::Report(r) => {
                self.s.reports.set(self.s.reports.get().wrapping_add(1));
                let now = now_ms32();
                v.set_time_ms(now);
                v.report(r);
                if self.s.reports.get() == 1 {
                    let (layout, why) = v.layout();
                    line(format_args!(
                        "ui first report: layout={:?} because {} mode_guess=\"{}\"",
                        layout,
                        why,
                        v.mode_guess()
                    ));
                }
                self.trace(&v, r, now);
            }
            UiEvent::Control(m) => v.control(m),
            UiEvent::Disconnected => {
                self.lost = true;
                v.disconnected();
                let skipped = self.tracer.link_lost();
                if skipped > 0 {
                    skipped_line(skipped);
                }
            }
        }
        self.log_notes(&mut v);
        drop(v);
        let took = (now_us() - started) as u32;
        if took > self.s.event_us_max.get() {
            self.s.event_us_max.set(took);
        }
    }

    fn next_output(&mut self, buf: &mut [u8]) -> Option<Output> {
        let mut v = self.s.view.try_borrow_mut().ok()?;
        v.set_time_ms(now_ms32());
        let out = v.next_output(buf);
        // With a packet, its notes wait until the host took it (or not):
        // a packet given back must not leave a "sent" line.
        if out.is_none() {
            self.log_notes(&mut v);
        }
        out
    }

    fn output_sent(&mut self) {
        if let Ok(mut v) = self.s.view.try_borrow_mut() {
            self.log_notes(&mut v);
        }
    }

    fn output_not_sent(&mut self) {
        if let Ok(mut v) = self.s.view.try_borrow_mut() {
            v.output_not_sent();
            self.log_notes(&mut v);
            line(format_args!(
                "ui an output report could not be sent (HID not open); it will be asked for again"
            ));
        }
    }

    fn summary(&mut self) {
        let Ok(v) = self.s.view.try_borrow() else {
            return;
        };
        let (layout, why) = v.layout();
        let st = v.startup();
        // The pad's device info (never its address) and the last HID
        // handshake on the control channel, if any.
        let mut info: heapless::String<32> = heapless::String::new();
        match st.device_info {
            Some(d) => {
                let _ = fmt::Write::write_fmt(
                    &mut info,
                    format_args!("fw{}.{:02}/type{}", d.firmware.0, d.firmware.1, d.kind),
                );
            }
            None => {
                let _ = info.push_str("none");
            }
        }
        crate::out::line(
            "PAD-SUMMARY",
            format_args!(
                "view layout={:?} because {} mode_guess=\"{}\" switch_startup={} switch_info={} switch_handshake={} outputs_sent={} reports_seen={} triggers={} unmapped_screen_changes={} unmapped_report_changes={} trace_lines={} trace_skipped={} battery={:?} battery_byte={:?}",
                layout,
                why,
                v.mode_guess(),
                st.outcome(),
                info,
                Handshake(st.handshake),
                st.sent(),
                self.s.reports.get(),
                if v.triggers_on_off() {
                    "on/off"
                } else {
                    "analog-or-unknown"
                },
                v.unmapped_changes().0,
                v.unmapped_changes().1,
                self.tracer.total_lines,
                self.tracer.total_skipped,
                v.battery_ask(),
                v.pad().battery
            ),
        );
    }
}

fn line(args: fmt::Arguments<'_>) {
    crate::out::line("PAD-STATE", args);
}

fn skipped_line(n: u32) {
    crate::out::line(
        "PAD-HID",
        format_args!(
            "trace skipped={} (more than {} lines in one second)",
            n,
            trace::MAX_LINES_PER_S
        ),
    );
}

/// The gap in ms, or "-" for the first report.
struct Gap(Option<u32>);

impl fmt::Display for Gap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(g) => write!(f, "{g}"),
            None => f.write_str("-"),
        }
    }
}

/// Writes into a fixed string and stops quietly when it is full (the end
/// of the text is lost, the start is kept).
struct CutString<'a, const N: usize>(&'a mut heapless::String<N>);

impl<const N: usize> fmt::Write for CutString<'_, N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            if self.0.push(c).is_err() {
                break;
            }
        }
        Ok(())
    }
}

/// The last HID handshake result for the summary: `none`, or its code.
struct Handshake(Option<u8>);

impl fmt::Display for Handshake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            None => f.write_str("none"),
            Some(r) => write!(f, "{:#x}", r),
        }
    }
}

/// Bytes as hex, with the bytes in the range (the pad's own address in
/// its answer to device info) written as "xx". Never logs the address.
struct HexMasked<'a>(&'a [u8], Option<core::ops::Range<usize>>);

impl fmt::Display for HexMasked<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, b) in self.0.iter().enumerate() {
            if self.1.as_ref().is_some_and(|r| r.contains(&i)) {
                f.write_str("xx")?;
            } else {
                write!(f, "{:02x}", b)?;
            }
        }
        Ok(())
    }
}
