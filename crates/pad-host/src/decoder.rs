//! What the pad sends, turned into a standard gamepad: the part of
//! pad-view's `View` that is not the screen.
//!
//! The same steps as `View`, in the same order (so the two read a pad the
//! same way): a descriptor resets the pad and chooses the layout again;
//! a report chooses the layout if it is still unknown, is decoded, and is
//! shown to the Switch start-up talk; a control message is the battery
//! answer or a handshake for the start-up talk; a lost link resets both.

use pad_proto::buf::Text;
use pad_view::battery::{self, Ask};
use pad_view::decode::{self, Hints, Layout, Pad, Trigger};
use pad_view::descriptor::Descriptor;
use pad_view::startup::{self, Output, Startup};

/// The longest report kept for choosing the layout again later (a new
/// name or descriptor). Reports are not kept for anything else.
const KEEP: usize = 64;

pub(crate) struct Decoder {
    pub desc: Descriptor,
    pub pad: Pad,
    pub layout: Layout,
    pub reason: &'static str,
    pub startup: Startup,
    pub battery_ask: Ask,
    battery_out: bool,
    pub name: Text<32>,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    /// A report came on this link.
    pub live: bool,
    now: Option<u32>,
    last: [u8; KEEP],
    last_len: usize,
}

impl Decoder {
    pub const fn new() -> Self {
        Self {
            desc: Descriptor::empty(),
            pad: Pad::new(),
            layout: Layout::Unknown,
            reason: "",
            startup: Startup::new(),
            battery_ask: Ask::NotAsked,
            battery_out: false,
            name: Text::new(),
            vendor_id: None,
            product_id: None,
            live: false,
            now: None,
            last: [0; KEEP],
            last_len: 0,
        }
    }

    pub fn set_time(&mut self, now_ms: u32) {
        self.now = Some(now_ms);
        self.startup.set_time(now_ms);
        if let Ask::Waiting { since: Some(t) } = self.battery_ask
            && now_ms.wrapping_sub(t) > battery::WAIT_MS
        {
            self.battery_ask = Ask::NoAnswer;
        }
    }

    /// Who the pad is. A new name or id chooses the layout again.
    pub fn device(&mut self, name: &str, vendor_id: Option<u16>, product_id: Option<u16>) {
        let new = name != self.name.as_str()
            || vendor_id != self.vendor_id
            || product_id != self.product_id;
        self.name = Text::from_device_bytes(name.as_bytes());
        self.vendor_id = vendor_id;
        self.product_id = product_id;
        if new {
            self.rechoose();
        }
    }

    pub fn descriptor(&mut self, bytes: &[u8]) {
        self.pad = Pad::new();
        self.desc.parse_in_place(bytes);
        self.rechoose();
    }

    /// One input report (without the 0xA1 byte). True when the decoder
    /// understood it.
    pub fn report(&mut self, bytes: &[u8]) -> bool {
        if bytes.is_empty() {
            return false;
        }
        self.live = true;
        let n = bytes.len().min(KEEP);
        self.last[..n].copy_from_slice(&bytes[..n]);
        self.last_len = n;
        if self.layout == Layout::Unknown {
            self.layout_from(Some(bytes));
        }
        let ok = decode::decode(self.layout, &self.desc, bytes, &mut self.pad);
        self.startup.on_report(bytes);
        ok
    }

    /// One message from the HID control channel, header byte first.
    pub fn control(&mut self, msg: &[u8]) {
        let Some(&h) = msg.first() else { return };
        let waiting = matches!(self.battery_ask, Ask::Waiting { .. });
        match h >> 4 {
            0x0 if waiting => self.battery_ask = Ask::Refused(h & 0x0F),
            0x0 => self.startup.on_handshake(h & 0x0F),
            0xA => {
                if waiting
                    && msg.get(1) == Some(&battery::REPORT_ID)
                    && let Some(&b) = msg.get(2)
                {
                    self.pad.battery = Some(b);
                    self.battery_ask = Ask::Answered;
                }
                self.startup.on_report(&msg[1..]);
            }
            _ => {}
        }
    }

    /// The next output report to send, as `View::next_output`.
    pub fn next_output(&mut self, buf: &mut [u8]) -> Option<Output> {
        self.battery_out = false;
        if let Some(out) = self.startup.next_output(buf) {
            return Some(out);
        }
        let due = self.layout == Layout::Xbox && self.live && self.battery_ask == Ask::NotAsked;
        if due && !buf.is_empty() {
            buf[0] = battery::REPORT_ID;
            self.battery_ask = Ask::Waiting { since: self.now };
            self.battery_out = true;
            return Some(Output {
                len: 1,
                channel: startup::Channel::GetReport,
            });
        }
        None
    }

    /// The packet `next_output` gave could not go out.
    pub fn output_not_sent(&mut self) {
        if self.battery_out {
            self.battery_out = false;
            self.battery_ask = Ask::NotAsked;
        } else {
            self.startup.not_sent();
        }
    }

    /// The link is gone. The pad's identity stays (the same pad usually
    /// comes back); what it pressed does not.
    pub fn disconnected(&mut self) {
        self.live = false;
        self.startup.reset();
        self.battery_ask = Ask::NotAsked;
        self.battery_out = false;
        self.pad = Pad::new();
        self.last_len = 0;
        self.rechoose();
    }

    fn rechoose(&mut self) {
        if self.last_len > 0 {
            let copy = self.last;
            self.layout_from(Some(&copy[..self.last_len]));
        } else {
            self.layout_from(None);
        }
    }

    fn layout_from(&mut self, report: Option<&[u8]>) {
        let hints = Hints {
            name: self.name.as_str(),
            vendor_id: self.vendor_id,
            product_id: self.product_id,
        };
        let (layout, reason) = decode::choose(&hints, &self.desc, report);
        self.layout = layout;
        self.reason = reason;
        if decode::digital_triggers(layout, &self.desc) {
            for t in [&mut self.pad.lt, &mut self.pad.rt] {
                if !t.present {
                    *t = Trigger {
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

/// A guess of which of the GuliKit KingKong 2 Pro's four modes this is,
/// the same as pad-view's `View::mode_guess`.
pub(crate) fn mode_of(layout: Layout, name: &str) -> Mode {
    let has = |s: &str| {
        name.as_bytes()
            .windows(s.len())
            .any(|w| w.eq_ignore_ascii_case(s.as_bytes()))
    };
    match layout {
        Layout::SwitchPro => Mode::Switch,
        _ if has("XW") => Mode::XInput,
        _ if has("Xbox Wireless") => Mode::Android,
        Layout::Xbox => Mode::XInput,
        Layout::Unknown => Mode::Unknown,
        _ if has("GuliKit") => Mode::DInput,
        _ => Mode::Other,
    }
}

/// Which way the pad talks (the GuliKit KingKong 2 Pro has four modes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Switch mode ("Pro Controller"): buttons, sticks, digital triggers,
    /// motion, battery.
    Switch,
    /// XInput (Windows) mode ("GuliKit Controller XW"): analog triggers;
    /// battery only if the pad answers the question.
    XInput,
    /// Android/iOS mode ("Xbox Wireless Controller"): as XInput.
    Android,
    /// DInput mode ("GuliKit Controller DW"): decoded from the descriptor.
    DInput,
    /// Another pad: decoded from its descriptor (a guess).
    Other,
    /// Nothing decoded yet.
    Unknown,
}

impl Mode {
    pub fn word(self) -> &'static str {
        match self {
            Mode::Switch => "switch",
            Mode::XInput => "xinput",
            Mode::Android => "android",
            Mode::DInput => "dinput",
            Mode::Other => "other",
            Mode::Unknown => "unknown",
        }
    }
}
