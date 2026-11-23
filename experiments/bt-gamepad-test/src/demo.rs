//! The demo build (Cargo feature `demo`): made-up gamepad data, so the
//! controller picture can be checked on the real screen without a
//! gamepad. Never leave this build on the board.
//!
//! The made-up events go through the same path as real ones: the screen
//! hook (`FwIo::ui`, then `ViewSink`, then pad-view's `View`), from the
//! radio loop. The radio still searches as usual meanwhile, so the radio
//! loop's gaps are measured under a real load.
//!
//! What it sends, over and over in a 40 s cycle:
//! - after 1.5 s: a device ("DEMO canned pad", gamepad class, Microsoft
//!   Xbox ids 045E:02E0, as the owner's GuliKit reports in its Xbox mode),
//!   and the HID descriptor the GuliKit really sent (306 bytes, copied
//!   from a board log; it holds no address);
//! - then a 16-byte input report (report id 1) every 8 ms (125 a second,
//!   faster than a real pad): the left stick turns in a circle once every
//!   2 s, the right one the other way every 3 s, the triggers ramp up and
//!   down, the d-pad turns, and the ten buttons light one after another;
//! - every 2 s a battery report (report id 4);
//! - at 30 s: "link lost" (the view greys out), and at 34 s a device
//!   comes back: the next kind each time, so the Xbox pad, a Switch pad
//!   and a DInput pad take turns.
//!
//! The Switch pad ("DEMO Switch pad", Nintendo ids 057E:2009, no
//! descriptor) sends a 49-byte full report 0x30 every 8 ms: the sticks
//! and buttons move as above, and the motion sensor rocks the pad from
//! side to side (40 degrees each way, every 4 s) and nose up and down (25
//! degrees, every 6 s), with the matching gyroscope readings, three
//! samples 5 ms apart in each report. The demo sends nothing to the pad,
//! so its start-up steps stay "not sent yet" on the About page.
//!
//! The DInput pad (a made-up name, ids 1949:0402) gives the
//! descriptor the GuliKit really sent in its DInput mode (148 bytes, from
//! the 2026-11-16 board log) and sends its 9-byte report 0x07 every 8 ms:
//! sticks as above, the hat turning, and one thing pressed at a time, 400
//! ms each: buttons 1 to 12 (LT is button 7, RT button 8: on/off
//! triggers), then vendor bit b8.0 (unmapped: the "?" lamp lights), then
//! AC Back in the consumer report 0x02 (unmapped, by its name), then AC
//! Home (the Home button), then nothing. Report 0x02 goes out only when
//! it changes, as a pad sends it.

use pad_proto::{DeviceInfo, Io, UiEvent};

/// Time between two reports.
pub const REPORT_MS: u64 = 8;
const START_MS: u64 = 1500;
const CYCLE_MS: u64 = 40_000;
const LOSE_AT_MS: u64 = 30_000;
const BACK_AT_MS: u64 = 34_000;

/// Reports made up at most per call, when the loop was late.
const CATCH_UP: u32 = 4;

/// The GuliKit KingKong 2 Pro's HID report descriptor in its Xbox mode,
/// as read over SDP on the board.
pub const DESCRIPTOR: [u8; 306] = [
    0x05, 0x01, 0x09, 0x05, 0xA1, 0x01, 0x85, 0x01, 0x09, 0x01, 0xA1, 0x00, 0x09, 0x30, 0x09, 0x31,
    0x15, 0x00, 0x27, 0xFF, 0xFF, 0x00, 0x00, 0x95, 0x02, 0x75, 0x10, 0x81, 0x02, 0xC0, 0x09, 0x01,
    0xA1, 0x00, 0x09, 0x33, 0x09, 0x34, 0x15, 0x00, 0x27, 0xFF, 0xFF, 0x00, 0x00, 0x95, 0x02, 0x75,
    0x10, 0x81, 0x02, 0xC0, 0x05, 0x01, 0x09, 0x32, 0x15, 0x00, 0x26, 0xFF, 0x03, 0x95, 0x01, 0x75,
    0x0A, 0x81, 0x02, 0x15, 0x00, 0x25, 0x00, 0x75, 0x06, 0x95, 0x01, 0x81, 0x03, 0x05, 0x01, 0x09,
    0x35, 0x15, 0x00, 0x26, 0xFF, 0x03, 0x95, 0x01, 0x75, 0x0A, 0x81, 0x02, 0x15, 0x00, 0x25, 0x00,
    0x75, 0x06, 0x95, 0x01, 0x81, 0x03, 0x05, 0x01, 0x09, 0x39, 0x15, 0x01, 0x25, 0x08, 0x35, 0x00,
    0x46, 0x3B, 0x01, 0x66, 0x14, 0x00, 0x75, 0x04, 0x95, 0x01, 0x81, 0x42, 0x75, 0x04, 0x95, 0x01,
    0x15, 0x00, 0x25, 0x00, 0x35, 0x00, 0x45, 0x00, 0x65, 0x00, 0x81, 0x03, 0x05, 0x09, 0x19, 0x01,
    0x29, 0x0A, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x0A, 0x81, 0x02, 0x15, 0x00, 0x25, 0x00,
    0x75, 0x06, 0x95, 0x01, 0x81, 0x03, 0x05, 0x01, 0x09, 0x80, 0x85, 0x02, 0xA1, 0x00, 0x09, 0x85,
    0x15, 0x00, 0x25, 0x01, 0x95, 0x01, 0x75, 0x01, 0x81, 0x02, 0x15, 0x00, 0x25, 0x00, 0x75, 0x07,
    0x95, 0x01, 0x81, 0x03, 0xC0, 0x05, 0x0F, 0x09, 0x21, 0x85, 0x03, 0xA1, 0x02, 0x09, 0x97, 0x15,
    0x00, 0x25, 0x01, 0x75, 0x04, 0x95, 0x01, 0x91, 0x02, 0x15, 0x00, 0x25, 0x00, 0x75, 0x04, 0x95,
    0x01, 0x91, 0x03, 0x09, 0x70, 0x15, 0x00, 0x25, 0x64, 0x75, 0x08, 0x95, 0x04, 0x91, 0x02, 0x09,
    0x50, 0x66, 0x01, 0x10, 0x55, 0x0E, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x01, 0x91,
    0x02, 0x09, 0xA7, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x01, 0x91, 0x02, 0x65, 0x00,
    0x55, 0x00, 0x09, 0x7C, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x01, 0x91, 0x02, 0xC0,
    0x85, 0x04, 0x05, 0x06, 0x09, 0x20, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x01, 0x81,
    0x02, 0xC0,
];

/// The GuliKit KingKong 2 Pro's HID report descriptor in its DInput mode,
/// as read over SDP on the board (148 bytes; it holds no address).
pub const DINPUT_DESCRIPTOR: [u8; 148] = [
    0x05, 0x0c, 0x09, 0x01, 0xa1, 0x01, 0x85, 0x02, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x08,
    0x09, 0xcd, 0x09, 0xb6, 0x09, 0xb5, 0x09, 0xea, 0x09, 0xe9, 0x09, 0xb8, 0x0a, 0x24, 0x02, 0x0a,
    0x23, 0x02, 0x81, 0x02, 0xc0, 0x05, 0x01, 0x09, 0x04, 0xa1, 0x01, 0x85, 0x07, 0xa1, 0x02, 0x75,
    0x08, 0x95, 0x05, 0x15, 0x00, 0x26, 0xff, 0x00, 0x35, 0x00, 0x46, 0xff, 0x00, 0x09, 0x32, 0x09,
    0x35, 0x09, 0x30, 0x09, 0x31, 0x09, 0x00, 0x81, 0x02, 0x75, 0x04, 0x95, 0x01, 0x25, 0x07, 0x46,
    0x3b, 0x01, 0x65, 0x14, 0x09, 0x39, 0x81, 0x42, 0x65, 0x00, 0x75, 0x01, 0x95, 0x0c, 0x25, 0x01,
    0x45, 0x01, 0x05, 0x09, 0x19, 0x01, 0x29, 0x0c, 0x81, 0x02, 0x06, 0x00, 0xff, 0x75, 0x01, 0x95,
    0x08, 0x25, 0x01, 0x45, 0x01, 0x09, 0x01, 0x81, 0x02, 0xc0, 0xa1, 0x02, 0x75, 0x08, 0x95, 0x08,
    0x46, 0xff, 0x00, 0x26, 0xff, 0x00, 0x09, 0x02, 0x91, 0x02, 0x75, 0x08, 0x95, 0x04, 0x09, 0x02,
    0xb1, 0x02, 0xc0, 0xc0,
];

/// The pads the demo takes turns with.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Xbox,
    Switch,
    DInput,
}

impl Kind {
    fn next(self) -> Self {
        match self {
            Kind::Xbox => Kind::Switch,
            Kind::Switch => Kind::DInput,
            Kind::DInput => Kind::Xbox,
        }
    }
}

/// sin(2 pi k / 32) times 30000.
const SINE: [i32; 32] = [
    0, 5853, 11481, 16667, 21213, 24944, 27716, 29424, 30000, 29424, 27716, 24944, 21213, 16667,
    11481, 5853, 0, -5853, -11481, -16667, -21213, -24944, -27716, -29424, -30000, -29424, -27716,
    -24944, -21213, -16667, -11481, -5853,
];

/// sin of `phase` (0..65536 is one turn), times 30000, from the table
/// with a straight line between its points.
fn sine(phase: u32) -> i32 {
    let p = phase & 0xFFFF;
    let k = (p >> 11) as usize;
    let frac = (p & 0x7FF) as i32;
    let a = SINE[k];
    let b = SINE[(k + 1) % 32];
    a + (b - a) * frac / 2048
}

fn cosine(phase: u32) -> i32 {
    sine(phase.wrapping_add(0x4000))
}

/// A stick value: centre 0x8000, `v` from -30000 to 30000.
fn axis(v: i32) -> [u8; 2] {
    ((0x8000 + v).clamp(0, 0xFFFF) as u16).to_le_bytes()
}

pub struct Demo {
    /// When the next report is due, in ms since start.
    next: u64,
    /// The made-up device is "connected" now.
    shown: bool,
    /// The device now.
    kind: Kind,
    last_battery: u64,
    /// The DInput pad's consumer bits as last sent (report 0x02).
    consumer: u8,
}

impl Demo {
    pub const fn new() -> Self {
        // Starts with the Xbox pad: `announce` moves on to the next first.
        Self {
            next: START_MS,
            shown: false,
            kind: Kind::DInput,
            last_battery: 0,
            consumer: 0,
        }
    }

    /// When the radio loop should wake for the demo next.
    pub fn due(&self) -> u64 {
        self.next
    }

    fn announce(io: &mut impl Io, kind: Kind) {
        if kind == Kind::DInput {
            io.ui(UiEvent::Status("DEMO: canned DInput reports, no gamepad"));
            let d = DeviceInfo {
                name: "DEMO GuliKit Controller DW",
                class_of_device: 0x00_2508,
                vendor_id: Some(0x1949),
                product_id: Some(0x0402),
                version: Some(0x011B),
                vendor_id_source: Some(2),
            };
            io.ui(UiEvent::Device(d));
            io.ui(UiEvent::Descriptor(&DINPUT_DESCRIPTOR));
            return;
        }
        if kind == Kind::Switch {
            io.ui(UiEvent::Status("DEMO: canned Switch reports, no gamepad"));
            let mut d = DeviceInfo {
                name: "DEMO Switch pad",
                class_of_device: 0x00_2508,
                vendor_id: None,
                product_id: None,
                version: None,
                vendor_id_source: None,
            };
            io.ui(UiEvent::Device(d));
            d.vendor_id = Some(0x057E);
            d.product_id = Some(0x2009);
            d.version = Some(0x0001);
            d.vendor_id_source = Some(2);
            io.ui(UiEvent::Device(d));
            return;
        }
        io.ui(UiEvent::Status("DEMO: canned reports, no gamepad"));
        let mut d = DeviceInfo {
            name: "DEMO canned pad",
            class_of_device: 0x00_0508,
            vendor_id: None,
            product_id: None,
            version: None,
            vendor_id_source: None,
        };
        io.ui(UiEvent::Device(d));
        d.vendor_id = Some(0x045E);
        d.product_id = Some(0x02E0);
        d.version = Some(0x0903);
        d.vendor_id_source = Some(2);
        io.ui(UiEvent::Device(d));
        io.ui(UiEvent::Descriptor(&DESCRIPTOR));
    }

    /// Makes the events due by `now` (ms since start).
    pub fn pump(&mut self, now: u64, io: &mut impl Io) {
        let mut made = 0;
        while now >= self.next && made < CATCH_UP {
            let t = self.next;
            let in_cycle = t.saturating_sub(START_MS) % CYCLE_MS;
            let live = !(LOSE_AT_MS..BACK_AT_MS).contains(&in_cycle);
            if live && !self.shown {
                self.kind = self.kind.next();
                Self::announce(io, self.kind);
                self.shown = true;
                self.consumer = 0;
            }
            if !live && self.shown {
                io.ui(UiEvent::Disconnected);
                io.ui(UiEvent::Status("DEMO: link lost (made up)"));
                self.shown = false;
            }
            if live && self.kind == Kind::Switch {
                io.ui(UiEvent::Report(&switch_report(t)));
            } else if live && self.kind == Kind::DInput {
                let (r, consumer) = dinput_report(t);
                io.ui(UiEvent::Report(&r));
                if consumer != self.consumer {
                    self.consumer = consumer;
                    io.ui(UiEvent::Report(&[0x02, consumer]));
                }
            } else if live {
                io.ui(UiEvent::Report(&report(t)));
                if t >= self.last_battery + 2000 {
                    self.last_battery = t;
                    let level = 100 - ((t / 2000) % 5) as u8 * 20;
                    io.ui(UiEvent::Report(&[0x04, level]));
                }
            }
            self.next += REPORT_MS;
            made += 1;
        }
        if now >= self.next {
            // Far behind (the loop was held up): skip, do not pile up.
            self.next = now + REPORT_MS;
        }
    }
}

/// The made-up report for time `t` (ms): report id 1, 16 bytes, in the
/// layout of the GuliKit's descriptor.
fn report(t: u64) -> [u8; 16] {
    let t32 = t as u32;
    // Left stick: one turn every 2 s. Right: the other way, every 3 s.
    let pl = (t32 % 2000) * 65536 / 2000;
    let pr = 65536 - (t32 % 3000) * 65536 / 3000;
    // Triggers: up over 1.5 s, then down; the right one half a cycle on.
    let ramp = |t: u32| {
        let k = t % 3000;
        (if k < 1500 { k } else { 3000 - k }) * 1023 / 1500
    };
    let lt = ramp(t32) as u16;
    let rt = ramp(t32 + 1500) as u16;
    // D-pad: 1..8 clockwise from up, 0 centred; a step every 400 ms.
    let hat = ((t32 / 400) % 9) as u8;
    // Buttons: one at a time, 300 ms each, then all off for a moment.
    let k = (t32 / 300) % 12;
    let buttons: u16 = if k < 10 { 1 << k } else { 0 };
    let mut r = [0u8; 16];
    r[0] = 0x01;
    r[1..3].copy_from_slice(&axis(cosine(pl)));
    r[3..5].copy_from_slice(&axis(sine(pl)));
    r[5..7].copy_from_slice(&axis(cosine(pr)));
    r[7..9].copy_from_slice(&axis(sine(pr)));
    r[9..11].copy_from_slice(&lt.to_le_bytes());
    r[11..13].copy_from_slice(&rt.to_le_bytes());
    r[13] = hat;
    r[14..16].copy_from_slice(&buttons.to_le_bytes());
    r
}

/// A DInput stick value: one byte, centre 0x80, `v` from -30000 to 30000.
fn byte_axis(v: i32) -> u8 {
    (0x80 + v * 127 / 30000).clamp(0, 255) as u8
}

/// The made-up DInput report 0x07 for time `t` (ms), 9 bytes, in the
/// layout of the GuliKit's DInput descriptor: Z, Rz (right stick), X, Y
/// (left stick), a byte with usage 0 (always 0 on the real pad), the hat
/// (low 4 bits, 0x0F centred) with buttons 1 to 4, buttons 5 to 12, 8
/// vendor bits. Also the consumer bits for report 0x02 at that time.
fn dinput_report(t: u64) -> ([u8; 9], u8) {
    let t32 = t as u32;
    let pl = (t32 % 2000) * 65536 / 2000;
    let pr = 65536 - (t32 % 3000) * 65536 / 3000;
    // Hat: 0..7 clockwise from up, then centred; a step every 400 ms.
    let k = (t32 / 400) % 9;
    let hat = if k < 8 { k as u8 } else { 0x0F };
    // One thing at a time, 400 ms each: buttons 1..12, the vendor bit,
    // AC Back, AC Home, nothing.
    let step = (t32 / 400) % 16;
    let buttons: u16 = if step < 12 { 1 << step } else { 0 };
    let vendor = if step == 12 { 0x01 } else { 0x00 };
    let consumer = match step {
        13 => 0x40,
        14 => 0x80,
        _ => 0x00,
    };
    let r = [
        0x07,
        byte_axis(cosine(pr)),
        byte_axis(sine(pr)),
        byte_axis(cosine(pl)),
        byte_axis(sine(pl)),
        0x00,
        hat | ((buttons as u8 & 0x0F) << 4),
        (buttons >> 4) as u8,
        vendor,
    ];
    (r, consumer)
}

/// A Switch stick: three bytes holding two 12-bit numbers (x, then y),
/// centre 2048; `x` and `y` from -30000 to 30000.
fn switch_stick(x: i32, y: i32) -> [u8; 3] {
    let x = (2048 + x * 1400 / 30000) as u16;
    // Switch sticks count up when pushed up; the screen's y goes down.
    let y = (2048 - y * 1400 / 30000) as u16;
    [
        x as u8,
        ((x >> 8) & 0x0F) as u8 | ((y & 0x0F) << 4) as u8,
        (y >> 4) as u8,
    ]
}

/// One motion sample at time `t` (ms): accelerometer x, y, z, gyroscope
/// x, y, z, in the pad's counts (4096 per g, 0.070 degrees a second per
/// count). Roll (around the forward axis) swings 40 degrees each way
/// every 4 s, pitch (nose up) 25 degrees every 6 s, and the pad turns
/// flat a little (yaw) every 3 s.
fn motion_sample(t: u32) -> [i16; 6] {
    // One turn is 65536; one degree is 65536 / 360.
    let p4 = (t % 4000) * 65536 / 4000;
    let p6 = (t % 6000) * 65536 / 6000;
    let p3 = (t % 3000) * 65536 / 3000;
    let roll = (sine(p4) as i64 * 40 * 65536 / 360 / 30000) as i32;
    let pitch = (sine(p6) as i64 * 25 * 65536 / 360 / 30000) as i32;
    let (sr, cr) = (sine(roll as u32) as i64, cosine(roll as u32) as i64);
    let (sp, cp) = (sine(pitch as u32) as i64, cosine(pitch as u32) as i64);
    // Gravity seen from the pad (pointing up when flat): x = sin pitch,
    // y = sin roll cos pitch, z = cos roll cos pitch.
    let g = 4096i64;
    let ax = g * sp / 30000;
    let ay = g * sr * cp / 30000 / 30000;
    let az = g * cr * cp / 30000 / 30000;
    // Turning speeds: the roll changes by 40 * 2 pi / 4 = 62.8 degrees a
    // second at most, the pitch by 25 * 2 pi / 6 = 26.2, yaw 30; in counts
    // (/ 0.070): 898, 374, 429.
    // Signs: the axes (x forward, y left, z up) turn by the right-hand
    // rule. Turning about +x lifts the left side (y towards z): the roll
    // grows, so gx follows the roll's rate. Turning about +y lowers the
    // nose (x towards -z), so a nose going up reads a negative gy.
    let gx = cosine(p4) * 898 / 30000;
    let gy = -cosine(p6) * 374 / 30000;
    let gz = sine(p3) * 429 / 30000;
    [
        ax as i16, ay as i16, az as i16, gx as i16, gy as i16, gz as i16,
    ]
}

/// The made-up Switch full report 0x30 for time `t` (ms), 49 bytes: the
/// sticks, triggers and buttons move as in [`report`], and bytes 13 to 48
/// hold three motion samples, 5 ms apart, the newest last.
fn switch_report(t: u64) -> [u8; 49] {
    let t32 = t as u32;
    let pl = (t32 % 2000) * 65536 / 2000;
    let pr = 65536 - (t32 % 3000) * 65536 / 3000;
    // Right, shared and left button bytes: one button at a time, 300 ms
    // each (Y X B A R ZR, - + RS LS Home Capture, down up right left L ZL).
    const ORDER: [(usize, u8); 18] = [
        (3, 0x01),
        (3, 0x02),
        (3, 0x04),
        (3, 0x08),
        (3, 0x40),
        (3, 0x80),
        (4, 0x01),
        (4, 0x02),
        (4, 0x04),
        (4, 0x08),
        (4, 0x10),
        (4, 0x20),
        (5, 0x01),
        (5, 0x02),
        (5, 0x04),
        (5, 0x08),
        (5, 0x40),
        (5, 0x80),
    ];
    let mut r = [0u8; 49];
    r[0] = 0x30;
    r[1] = (t32 / REPORT_MS as u32) as u8;
    // Battery full (8), on battery, connection type E (Pro Controller).
    r[2] = 0x8E;
    let k = (t32 / 300) as usize % (ORDER.len() + 2);
    if let Some((byte, bit)) = ORDER.get(k) {
        r[*byte] = *bit;
    }
    r[6..9].copy_from_slice(&switch_stick(cosine(pl), sine(pl)));
    r[9..12].copy_from_slice(&switch_stick(cosine(pr), sine(pr)));
    r[12] = 0x0B;
    for (n, back) in [10u32, 5, 0].into_iter().enumerate() {
        let s = motion_sample(t32.wrapping_sub(back));
        for (i, v) in s.iter().enumerate() {
            let at = 13 + n * 12 + i * 2;
            r[at..at + 2].copy_from_slice(&v.to_le_bytes());
        }
    }
    r
}
