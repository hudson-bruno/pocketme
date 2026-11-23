//! Turning one input report into a "standard gamepad".
//!
//! Every gamepad has more or less the same controls: four face buttons,
//! two bumpers, two triggers, a d-pad, two sticks that can be clicked, and
//! a few small buttons in the middle. [`Pad`] holds those.
//!
//! Some gamepads are known and decoded by hand (their bytes are documented
//! elsewhere, see MODES.md). Every other gamepad is decoded from the usages
//! its HID report descriptor declares, and then the screen says that the
//! mapping is a guess.

use crate::descriptor::{Descriptor, Field};
use crate::motion::Motion;

/// The buttons of a standard gamepad, as bit numbers in [`Pad::buttons`].
///
/// The four face buttons are named by where they sit (south is the one
/// nearest to you), because the letters differ: an Xbox pad has A at the
/// bottom, a Nintendo pad has A on the right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Btn {
    South = 0,
    East,
    West,
    North,
    /// Left bumper (LB, L, L1).
    L,
    /// Right bumper (RB, R, R1).
    R,
    /// Left trigger as a button (LT, ZL, L2).
    ZL,
    /// Right trigger as a button (RT, ZR, R2).
    ZR,
    /// View, Back, Select, Share on PlayStation, "-" on Nintendo.
    Minus,
    /// Menu, Start, Options, "+".
    Plus,
    /// Xbox, Home, PS button.
    Home,
    /// Capture (Nintendo), Share (Xbox Series), touchpad click (PlayStation).
    Capture,
    /// Left stick pressed in.
    LStick,
    /// Right stick pressed in.
    RStick,
    Up,
    Down,
    Left,
    Right,
}

pub const BTN_COUNT: usize = 18;

impl Btn {
    pub const ALL: [Btn; BTN_COUNT] = [
        Btn::South,
        Btn::East,
        Btn::West,
        Btn::North,
        Btn::L,
        Btn::R,
        Btn::ZL,
        Btn::ZR,
        Btn::Minus,
        Btn::Plus,
        Btn::Home,
        Btn::Capture,
        Btn::LStick,
        Btn::RStick,
        Btn::Up,
        Btn::Down,
        Btn::Left,
        Btn::Right,
    ];
    pub const fn bit(self) -> u32 {
        1 << self as u8
    }
    /// A short name that does not depend on the brand.
    pub fn name(self, face: Face) -> &'static str {
        match self {
            Btn::South | Btn::East | Btn::West | Btn::North => face.letter(self),
            Btn::L => face.pick("LB", "L", "L1"),
            Btn::R => face.pick("RB", "R", "R1"),
            Btn::ZL => face.pick("LT", "ZL", "L2"),
            Btn::ZR => face.pick("RT", "ZR", "R2"),
            Btn::Minus => face.pick("View", "-", "Share"),
            Btn::Plus => face.pick("Menu", "+", "Opt"),
            Btn::Home => face.pick("Xbox", "Home", "PS"),
            Btn::Capture => face.pick("Share", "Capt", "Pad"),
            Btn::LStick => face.pick("LS", "LS", "L3"),
            Btn::RStick => face.pick("RS", "RS", "R3"),
            Btn::Up => "Up",
            Btn::Down => "Down",
            Btn::Left => "Left",
            Btn::Right => "Right",
        }
    }
}

/// Which letters the face buttons carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// A south, B east, X west, Y north.
    Xbox,
    /// B south, A east, Y west, X north (the Switch layout).
    Nintendo,
    /// Cross south, circle east, square west, triangle north.
    PlayStation,
}

impl Face {
    fn pick(self, x: &'static str, n: &'static str, p: &'static str) -> &'static str {
        match self {
            Face::Xbox => x,
            Face::Nintendo => n,
            Face::PlayStation => p,
        }
    }
    /// The letter of a face button.
    pub fn letter(self, b: Btn) -> &'static str {
        match (self, b) {
            (Face::Xbox, Btn::South) => "A",
            (Face::Xbox, Btn::East) => "B",
            (Face::Xbox, Btn::West) => "X",
            (Face::Xbox, Btn::North) => "Y",
            (Face::Nintendo, Btn::South) => "B",
            (Face::Nintendo, Btn::East) => "A",
            (Face::Nintendo, Btn::West) => "Y",
            (Face::Nintendo, Btn::North) => "X",
            // The font has no shapes, so the PlayStation symbols get letters.
            (Face::PlayStation, Btn::South) => "x",
            (Face::PlayStation, Btn::East) => "o",
            (Face::PlayStation, Btn::West) => "#",
            (Face::PlayStation, Btn::North) => "^",
            _ => "?",
        }
    }
}

/// One stick axis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Axis {
    /// The number as it came in the report.
    pub raw: i32,
    /// -1.0 (left or up) to +1.0 (right or down). Screen direction: y grows
    /// downwards, as in HID. Pads that send "up = bigger" are turned round.
    pub norm: f32,
    /// False when this pad has never sent this axis.
    pub present: bool,
}

/// One trigger.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Trigger {
    pub raw: i32,
    /// 0 (released) to 1023 (fully pressed).
    pub value: u16,
    /// True for a trigger that reports how far it is pressed; false for
    /// one that is only pressed or not (then value is 0 or 1023).
    pub analog: bool,
    pub present: bool,
}

/// The state of a standard gamepad.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pad {
    /// Bits from [`Btn::bit`].
    pub buttons: u32,
    /// Buttons that do not map to any standard control: bit n is button
    /// usage n+1 of the descriptor (generic mapping only).
    pub extra: u32,
    pub lx: Axis,
    pub ly: Axis,
    pub rx: Axis,
    pub ry: Axis,
    pub lt: Trigger,
    pub rt: Trigger,
    /// The d-pad as a direction: 0 up, 1 up-right, ... 7 up-left (clockwise),
    /// or None when centred.
    pub hat: Option<u8>,
    /// Battery level as the pad reports it (meaning depends on the pad).
    pub battery: Option<u8>,
    /// The motion samples of the last full report, if the pad sends real
    /// ones (Switch layout only; all zeros counts as none). See
    /// [`crate::motion`].
    pub motion: Option<Motion>,
    /// True once an Xbox pad sent its Xbox button in a report of its own
    /// (report 2). From then on report 1 is not asked about that button,
    /// so the two reports do not fight over it.
    pub guide_report: bool,
}

impl Pad {
    /// Nothing pressed, nothing known.
    pub const fn new() -> Self {
        Pad {
            buttons: 0,
            extra: 0,
            lx: Axis {
                raw: 0,
                norm: 0.0,
                present: false,
            },
            ly: Axis {
                raw: 0,
                norm: 0.0,
                present: false,
            },
            rx: Axis {
                raw: 0,
                norm: 0.0,
                present: false,
            },
            ry: Axis {
                raw: 0,
                norm: 0.0,
                present: false,
            },
            lt: Trigger {
                raw: 0,
                value: 0,
                analog: false,
                present: false,
            },
            rt: Trigger {
                raw: 0,
                value: 0,
                analog: false,
                present: false,
            },
            hat: None,
            battery: None,
            motion: None,
            guide_report: false,
        }
    }
    pub fn pressed(&self, b: Btn) -> bool {
        self.buttons & b.bit() != 0
    }
    fn set(&mut self, b: Btn, on: bool) {
        if on {
            self.buttons |= b.bit();
        } else {
            self.buttons &= !b.bit();
        }
    }
    /// Set the four d-pad buttons from a hat direction.
    fn set_hat(&mut self, hat: Option<u8>) {
        self.hat = hat;
        let (up, right, down, left) = match hat {
            Some(0) => (true, false, false, false),
            Some(1) => (true, true, false, false),
            Some(2) => (false, true, false, false),
            Some(3) => (false, true, true, false),
            Some(4) => (false, false, true, false),
            Some(5) => (false, false, true, true),
            Some(6) => (false, false, false, true),
            Some(7) => (true, false, false, true),
            _ => (false, false, false, false),
        };
        self.set(Btn::Up, up);
        self.set(Btn::Right, right);
        self.set(Btn::Down, down);
        self.set(Btn::Left, left);
    }
    /// Hat direction from the four d-pad buttons.
    fn hat_from_buttons(&mut self) {
        let u = self.pressed(Btn::Up);
        let d = self.pressed(Btn::Down);
        let l = self.pressed(Btn::Left);
        let r = self.pressed(Btn::Right);
        self.hat = match (u, r, d, l) {
            (true, false, false, false) => Some(0),
            (true, true, false, false) => Some(1),
            (false, true, false, false) => Some(2),
            (false, true, true, false) => Some(3),
            (false, false, true, false) => Some(4),
            (false, false, true, true) => Some(5),
            (false, false, false, true) => Some(6),
            (true, false, false, true) => Some(7),
            _ => None,
        };
    }
}

/// Which decoding is used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// Nothing known yet: reports are only shown raw.
    Unknown,
    /// Nintendo Switch Pro Controller (and pads that pretend to be one):
    /// reports 0x3F, 0x30, 0x21, decoded by hand.
    SwitchPro,
    /// Xbox One S / Series over classic Bluetooth: report 0x01, decoded by
    /// hand. Two button orders exist, told apart by the report length.
    Xbox,
    /// Generic from the descriptor, with Android's button numbering
    /// (1 A, 2 B, 4 X, 5 Y, 7 L1, 8 R1, ..., 13 Home, 14 L3, 15 R3).
    Android,
    /// Generic from the descriptor, PlayStation button numbering
    /// (1 square, 2 cross, 3 circle, 4 triangle, ...).
    PlayStation,
    /// Generic from the descriptor, buttons in plain order (a guess).
    Generic,
}

impl Layout {
    pub fn name(self) -> &'static str {
        match self {
            Layout::Unknown => "unknown",
            Layout::SwitchPro => "Switch Pro",
            Layout::Xbox => "Xbox BT",
            Layout::Android => "Android HID",
            Layout::PlayStation => "PlayStation",
            Layout::Generic => "generic HID",
        }
    }
    /// True when buttons are mapped by guessing from the descriptor.
    pub fn is_guess(self) -> bool {
        matches!(self, Layout::Generic | Layout::Unknown)
    }
    pub fn face(self) -> Face {
        match self {
            Layout::SwitchPro => Face::Nintendo,
            Layout::PlayStation => Face::PlayStation,
            _ => Face::Xbox,
        }
    }
}

/// What is known about the device, for choosing a layout.
#[derive(Clone, Copy, Debug, Default)]
pub struct Hints<'a> {
    pub name: &'a str,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
}

fn contains(hay: &str, needle: &str) -> bool {
    hay.as_bytes()
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Choose a layout, and say why in a few words.
///
/// `first_report` is the first input report seen (with its report id at
/// index 0 when the pad uses ids), if any.
pub fn choose(
    h: &Hints<'_>,
    d: &Descriptor,
    first_report: Option<&[u8]>,
) -> (Layout, &'static str) {
    let vid = h.vendor_id;
    // A vendor id or a name is only trusted when the descriptor (if one
    // was read) agrees: a pad may borrow a name and still send something
    // else. Then the descriptor's own shape decides (step 3).
    let switch_ok =
        d.is_empty() || d.input_report(0x30).is_some() || d.input_report(0x3F).is_some();
    let xbox_ok = d.is_empty()
        || looks_like_xbox(d)
        || d.input_report(0x01).is_some_and(|r| r.bits >= 15 * 8);
    // 1. Known by vendor id.
    if vid == Some(0x057E) && switch_ok {
        return (Layout::SwitchPro, "vendor id 057E (Nintendo)");
    }
    if vid == Some(0x045E) && xbox_ok {
        return (Layout::Xbox, "vendor id 045E (Microsoft)");
    }
    // 2. Known by name.
    if (contains(h.name, "Pro Controller") || contains(h.name, "Joy-Con")) && switch_ok {
        return (Layout::SwitchPro, "name says Pro Controller");
    }
    if (contains(h.name, "Xbox") || contains(h.name, "Controller XW")) && xbox_ok {
        return (Layout::Xbox, "name says Xbox / XW");
    }
    // 3. Known by the shape of the descriptor.
    if !d.is_empty() {
        let has = |id: u8| d.input_report(id).is_some();
        if has(0x3F) && has(0x21) && has(0x30) {
            return (Layout::SwitchPro, "reports 3F, 21 and 30");
        }
        if looks_like_xbox(d) {
            return (Layout::Xbox, "report 1 shaped like Xbox");
        }
        if vid == Some(0x054C) {
            return (Layout::PlayStation, "vendor id 054C (Sony)");
        }
        // The GuliKit KK2 Pro's wired DInput ids. Its buttons are numbered
        // the Android way, with Home on 16 (SDL's gamepad database). The
        // Bluetooth DInput mode may use the same ids: not known.
        if vid == Some(0x0079) && h.product_id == Some(0x181C) {
            return (Layout::Android, "0079:181C, GuliKit DInput");
        }
        if looks_android(d) {
            return (Layout::Android, "button usages as Android");
        }
        return (Layout::Generic, "from descriptor usages");
    }
    // 4. Known by the first report alone (no descriptor read).
    if let Some(r) = first_report {
        match (r.first(), r.len()) {
            (Some(0x3F), 12) | (Some(0x30), 13..) | (Some(0x21), 13..) => {
                return (Layout::SwitchPro, "report id 3F/30/21");
            }
            (Some(0x01), 16..=20) => return (Layout::Xbox, "report 1, 16-20 bytes"),
            _ => {}
        }
    }
    (Layout::Unknown, "no descriptor, unknown")
}

/// Xbox One S / Series classic Bluetooth: report 1 starts with 16-bit X and
/// Y, and has a hat with logical range 1..8.
fn looks_like_xbox(d: &Descriptor) -> bool {
    let mut f = d.input_fields(0x01).filter(|f| !f.is_constant());
    let (Some(a), Some(b)) = (f.next(), f.next()) else {
        return false;
    };
    let xy = a.usage_page == 1
        && a.usage == 0x30
        && a.bit_size == 16
        && b.usage == 0x31
        && b.bit_size == 16;
    let hat = d
        .input_fields(0x01)
        .any(|f| f.usage_page == 1 && f.usage == 0x39 && f.logical_min == 1 && f.logical_max == 8);
    xy && hat && d.input_report(0x01).map(|r| r.bits).unwrap_or(0) >= 15 * 8
}

/// Android's numbering leaves out buttons 3 and 6 (C and Z) and uses 13-15.
fn looks_android(d: &Descriptor) -> bool {
    let has = |n: u16| d.has_input_usage(0x09, n);
    !has(3) && !has(6) && has(1) && has(2) && has(4) && has(5) && (has(13) || has(14))
}

/// Decode one input report into `pad`. `report` starts with the report id
/// when the pad uses ids. Returns false when this report says nothing the
/// layout understands (it is still shown raw).
pub fn decode(layout: Layout, d: &Descriptor, report: &[u8], pad: &mut Pad) -> bool {
    match layout {
        Layout::SwitchPro => {
            decode_switch(report, pad) || decode_generic(Layout::Generic, d, report, pad)
        }
        Layout::Xbox => decode_xbox(report, pad) || decode_generic(Layout::Generic, d, report, pad),
        Layout::Unknown => false,
        l => decode_generic(l, d, report, pad),
    }
}

/// Map a value to -1.0..=1.0 given its centre and half range, clamped.
fn norm(v: i32, centre: f32, half: f32) -> f32 {
    if half <= 0.0 {
        return 0.0;
    }
    let n = (v as f32 - centre) / half;
    n.clamp(-1.0, 1.0)
}

fn axis(raw: i32, centre: f32, half: f32) -> Axis {
    Axis {
        raw,
        norm: norm(raw, centre, half),
        present: true,
    }
}

// ---------------------------------------------------------------------
// Switch Pro Controller. Sources: dekuNukem's Nintendo_Switch_Reverse_
// Engineering notes (bluetooth_hid_notes.md) and SDL's
// SDL_hidapi_switch.c (read to learn the layout; no code copied).

/// Half the travel of a Switch stick in 12-bit units. The real range
/// comes from calibration data stored in the pad; this is a round guess
/// so that a full push reads close to 1.0 on most pads.
const SWITCH_HALF: f32 = 1600.0;

fn decode_switch(r: &[u8], pad: &mut Pad) -> bool {
    match r.first() {
        Some(0x3F) if r.len() >= 12 => {
            // "Simple" report, sent before the host asks for full mode.
            let b1 = r[1];
            let b2 = r[2];
            pad.set(Btn::South, b1 & 0x01 != 0); // B
            pad.set(Btn::East, b1 & 0x02 != 0); // A
            pad.set(Btn::West, b1 & 0x04 != 0); // Y
            pad.set(Btn::North, b1 & 0x08 != 0); // X
            pad.set(Btn::L, b1 & 0x10 != 0);
            pad.set(Btn::R, b1 & 0x20 != 0);
            pad.set(Btn::ZL, b1 & 0x40 != 0);
            // Some pads put ZR in the second byte instead (SDL reads both).
            pad.set(Btn::ZR, b1 & 0x80 != 0 || b2 & 0x80 != 0);
            pad.set(Btn::Minus, b2 & 0x01 != 0);
            pad.set(Btn::Plus, b2 & 0x02 != 0);
            pad.set(Btn::LStick, b2 & 0x04 != 0);
            pad.set(Btn::RStick, b2 & 0x08 != 0);
            pad.set(Btn::Home, b2 & 0x10 != 0);
            pad.set(Btn::Capture, b2 & 0x20 != 0);
            pad.set_hat(if r[3] < 8 { Some(r[3]) } else { None });
            let w = |i: usize| u16::from_le_bytes([r[i], r[i + 1]]) as i32;
            pad.lx = axis(w(4), 32768.0, 32768.0);
            pad.ly = axis(w(6), 32768.0, 32768.0);
            pad.rx = axis(w(8), 32768.0, 32768.0);
            pad.ry = axis(w(10), 32768.0, 32768.0);
            set_digital_triggers(pad);
            // The simple report has no motion samples.
            pad.motion = None;
            true
        }
        Some(0x30 | 0x21 | 0x31) if r.len() >= 13 => {
            // Full report: timer, battery, 3 button bytes, 2 sticks of
            // 3 bytes (two 12-bit numbers each), vibration byte.
            let right = r[3];
            let mid = r[4];
            let left = r[5];
            pad.set(Btn::West, right & 0x01 != 0); // Y
            pad.set(Btn::North, right & 0x02 != 0); // X
            pad.set(Btn::South, right & 0x04 != 0); // B
            pad.set(Btn::East, right & 0x08 != 0); // A
            pad.set(Btn::R, right & 0x40 != 0);
            pad.set(Btn::ZR, right & 0x80 != 0);
            pad.set(Btn::Minus, mid & 0x01 != 0);
            pad.set(Btn::Plus, mid & 0x02 != 0);
            pad.set(Btn::RStick, mid & 0x04 != 0);
            pad.set(Btn::LStick, mid & 0x08 != 0);
            pad.set(Btn::Home, mid & 0x10 != 0);
            pad.set(Btn::Capture, mid & 0x20 != 0);
            pad.set(Btn::Down, left & 0x01 != 0);
            pad.set(Btn::Up, left & 0x02 != 0);
            pad.set(Btn::Right, left & 0x04 != 0);
            pad.set(Btn::Left, left & 0x08 != 0);
            pad.set(Btn::L, left & 0x40 != 0);
            pad.set(Btn::ZL, left & 0x80 != 0);
            pad.hat_from_buttons();
            let (lx, ly) = twelve(&r[6..9]);
            let (rx, ry) = twelve(&r[9..12]);
            // Nintendo sticks send "up" as a bigger number: turn y round.
            pad.lx = axis(lx, 2048.0, SWITCH_HALF);
            pad.ly = Axis {
                raw: ly,
                norm: -norm(ly, 2048.0, SWITCH_HALF),
                present: true,
            };
            pad.rx = axis(rx, 2048.0, SWITCH_HALF);
            pad.ry = Axis {
                raw: ry,
                norm: -norm(ry, 2048.0, SWITCH_HALF),
                present: true,
            };
            pad.battery = Some(r[2] >> 4);
            set_digital_triggers(pad);
            // Report 0x21 (an answer to a subcommand) carries the answer
            // where 0x30 carries motion: it leaves the motion as it was.
            if r[0] != 0x21 {
                pad.motion = Motion::from_switch_report(r);
            }
            true
        }
        _ => false,
    }
}

/// Two 12-bit numbers packed in three bytes.
pub fn twelve(b: &[u8]) -> (i32, i32) {
    let x = b[0] as i32 | ((b[1] as i32 & 0x0F) << 8);
    let y = (b[1] as i32 >> 4) | ((b[2] as i32) << 4);
    (x, y)
}

fn set_digital_triggers(pad: &mut Pad) {
    let zl = pad.pressed(Btn::ZL);
    let zr = pad.pressed(Btn::ZR);
    pad.lt = Trigger {
        raw: zl as i32,
        value: if zl { 1023 } else { 0 },
        analog: false,
        present: true,
    };
    pad.rt = Trigger {
        raw: zr as i32,
        value: if zr { 1023 } else { 0 },
        analog: false,
        present: true,
    };
}

// ---------------------------------------------------------------------
// Xbox One S / Series on classic Bluetooth. Source: SDL's
// SDL_hidapi_xboxone.c (HIDAPI_DriverXboxOneBluetooth_*), read to learn
// the layout; and the descriptors published by xpadneo and Android CTS.

fn decode_xbox(r: &[u8], pad: &mut Pad) -> bool {
    match r.first() {
        Some(0x01) if r.len() >= 16 => {
            let w = |i: usize| u16::from_le_bytes([r[i], r[i + 1]]) as i32;
            pad.lx = axis(w(1), 32768.0, 32768.0);
            pad.ly = axis(w(3), 32768.0, 32768.0);
            pad.rx = axis(w(5), 32768.0, 32768.0);
            pad.ry = axis(w(7), 32768.0, 32768.0);
            let lt = w(9) & 0x3FF;
            let rt = w(11) & 0x3FF;
            pad.lt = Trigger {
                raw: lt,
                value: lt as u16,
                analog: true,
                present: true,
            };
            pad.rt = Trigger {
                raw: rt,
                value: rt as u16,
                analog: true,
                present: true,
            };
            pad.set(Btn::ZL, lt > 512);
            pad.set(Btn::ZR, rt > 512);
            // Hat 1..8 clockwise from up; 0 is centred.
            pad.set_hat(match r[13] {
                1..=8 => Some(r[13] - 1),
                _ => None,
            });
            let a = r[14];
            let b = r[15];
            if r.len() == 16 {
                // Older firmware (and the GuliKit in XInput and Android
                // modes, by its descriptor): ten buttons in a row.
                pad.set(Btn::South, a & 0x01 != 0);
                pad.set(Btn::East, a & 0x02 != 0);
                pad.set(Btn::West, a & 0x04 != 0);
                pad.set(Btn::North, a & 0x08 != 0);
                pad.set(Btn::L, a & 0x10 != 0);
                pad.set(Btn::R, a & 0x20 != 0);
                pad.set(Btn::Minus, a & 0x40 != 0);
                pad.set(Btn::Plus, a & 0x80 != 0);
                pad.set(Btn::LStick, b & 0x01 != 0);
                pad.set(Btn::RStick, b & 0x02 != 0);
            } else {
                // Newer firmware: fifteen buttons with gaps (Android order).
                pad.set(Btn::South, a & 0x01 != 0);
                pad.set(Btn::East, a & 0x02 != 0);
                pad.set(Btn::West, a & 0x08 != 0);
                pad.set(Btn::North, a & 0x10 != 0);
                pad.set(Btn::L, a & 0x40 != 0);
                pad.set(Btn::R, a & 0x80 != 0);
                let back = b & 0x04 != 0 || r.get(16).is_some_and(|c| c & 0x01 != 0);
                pad.set(Btn::Minus, back);
                pad.set(Btn::Plus, b & 0x08 != 0);
                if !pad.guide_report {
                    pad.set(Btn::Home, b & 0x10 != 0);
                }
                pad.set(Btn::LStick, b & 0x20 != 0);
                pad.set(Btn::RStick, b & 0x40 != 0);
            }
            true
        }
        // The Xbox button alone, in its own report (older firmware).
        Some(0x02) if r.len() >= 2 => {
            pad.guide_report = true;
            pad.set(Btn::Home, r[1] & 0x01 != 0);
            true
        }
        // Battery.
        Some(0x04) if r.len() >= 2 => {
            pad.battery = Some(r[1]);
            true
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------
// Generic decoding from the descriptor's usages.

/// Which standard button a Button-page usage number means.
fn button_for(layout: Layout, n: u16) -> Option<Btn> {
    use Btn::*;
    match layout {
        Layout::Android => match n {
            1 => Some(South),
            2 => Some(East),
            4 => Some(West),
            5 => Some(North),
            7 => Some(L),
            8 => Some(R),
            9 => Some(ZL),
            10 => Some(ZR),
            11 => Some(Minus),
            12 => Some(Plus),
            13 => Some(Home),
            14 => Some(LStick),
            15 => Some(RStick),
            // Not Android's, but the GuliKit KK2 Pro's wired DInput mode
            // puts Home here (SDL's gamepad database, 0079:181C).
            16 => Some(Home),
            _ => None,
        },
        Layout::PlayStation => match n {
            1 => Some(West),
            2 => Some(South),
            3 => Some(East),
            4 => Some(North),
            5 => Some(L),
            6 => Some(R),
            7 => Some(ZL),
            8 => Some(ZR),
            9 => Some(Minus),
            10 => Some(Plus),
            11 => Some(LStick),
            12 => Some(RStick),
            13 => Some(Home),
            14 => Some(Capture),
            _ => None,
        },
        // A guess: the order most "DInput" pads use for the first ten,
        // then the usual extras.
        _ => match n {
            1 => Some(South),
            2 => Some(East),
            3 => Some(West),
            4 => Some(North),
            5 => Some(L),
            6 => Some(R),
            7 => Some(ZL),
            8 => Some(ZR),
            9 => Some(Minus),
            10 => Some(Plus),
            11 => Some(LStick),
            12 => Some(RStick),
            13 => Some(Home),
            14 => Some(Capture),
            _ => None,
        },
    }
}

/// Where each axis goes, decided from the order of the axis usages.
#[derive(Clone, Copy)]
pub(crate) struct AxisPlan {
    /// The usage pair for the right stick: (Z, Rz) or (Rx, Ry).
    right: (u16, u16),
    /// The usages for the triggers (page, usage) each.
    lt: (u16, u16),
    rt: (u16, u16),
}

pub(crate) fn axis_plan(d: &Descriptor, id: u8) -> AxisPlan {
    let mut z_at = None;
    let mut rx_at = None;
    for (i, f) in d.input_fields(id).enumerate() {
        if f.usage_page == 1 && f.usage == 0x32 && z_at.is_none() {
            z_at = Some(i);
        }
        if f.usage_page == 1 && f.usage == 0x33 && rx_at.is_none() {
            rx_at = Some(i);
        }
    }
    let has_brake_gas = d.has_input_usage(0x02, 0xC5) || d.has_input_usage(0x02, 0xC4);
    if has_brake_gas {
        let right = if z_at.is_some() {
            (0x32, 0x35)
        } else {
            (0x33, 0x34)
        };
        return AxisPlan {
            right,
            lt: (0x02, 0xC5),
            rt: (0x02, 0xC4),
        };
    }
    match (z_at, rx_at) {
        // Rx/Ry come first (Xbox style): they are the stick, Z/Rz triggers.
        (Some(z), Some(rx)) if rx < z => AxisPlan {
            right: (0x33, 0x34),
            lt: (1, 0x32),
            rt: (1, 0x35),
        },
        // Z/Rz first (PlayStation style): Rx/Ry are the triggers.
        (Some(_), Some(_)) => AxisPlan {
            right: (0x32, 0x35),
            lt: (1, 0x33),
            rt: (1, 0x34),
        },
        (Some(_), None) => AxisPlan {
            right: (0x32, 0x35),
            lt: (0, 0),
            rt: (0, 0),
        },
        _ => AxisPlan {
            right: (0x33, 0x34),
            lt: (0, 0),
            rt: (0, 0),
        },
    }
}

/// Centre and half range of a field, from its logical range (or from its
/// bit size when the range is missing).
fn range(f: &Field) -> (f32, f32) {
    let (lo, hi) = if f.logical_max > f.logical_min {
        (f.logical_min as f32, f.logical_max as f32)
    } else {
        let bits = f.bit_size.clamp(1, 32) as u32;
        let full = ((1u64 << bits) - 1) as f32;
        if f.is_signed() || bits == 32 {
            (-(full / 2.0) - 0.5, full / 2.0 - 0.5)
        } else {
            (0.0, full)
        }
    };
    ((lo + hi) / 2.0, (hi - lo) / 2.0)
}

/// Consumer "AC Back" is the "-" (View, Back, Select) button only when
/// that is certain: in Android's layout (Android reads AC Back as its Back
/// key, and the Android CTS files show pads that send it for that
/// button), or when no Button-page usage of the descriptor is "-" already.
/// The GuliKit's DInput mode has both AC Back and a button 9 ("-" in the
/// plain order), so there AC Back stays unmapped and is shown by its name.
fn ac_back_is_minus(layout: Layout, d: &Descriptor) -> bool {
    if layout == Layout::Android {
        return true;
    }
    !d.fields().iter().any(|f| {
        f.kind == crate::descriptor::Kind::Input
            && !f.is_constant()
            && f.usage_page == 0x09
            && (f.usage..=f.usage_max.min(f.usage.saturating_add(40)))
                .any(|n| button_for(layout, n) == Some(Btn::Minus))
    })
}

/// True when [`decode_generic`] reads field `f` for the standard gamepad
/// (with `layout`'s button order and `plan` for the axes of its report).
/// Must match the `match` in [`decode_generic`]: the unmapped-input
/// tracker (crate::unmapped) shows every field this says false for.
pub(crate) fn generic_uses(layout: Layout, d: &Descriptor, plan: &AxisPlan, f: &Field) -> bool {
    if f.is_constant() {
        return false;
    }
    if !f.is_variable() {
        // Arrays: only arrays of buttons are read (all of their slots).
        return f.usage_page == 0x09;
    }
    match (f.usage_page, f.usage) {
        (0x09, n) => button_for(layout, n).is_some(),
        (0x01, 0x39 | 0x90..=0x93 | 0x85 | 0x30 | 0x31) => true,
        (0x0C, 0x223 | 0x40 | 0xB2) => true,
        (0x0C, 0x224) => ac_back_is_minus(layout, d),
        (0x06, 0x20) => true,
        (1, u) if u == plan.right.0 || u == plan.right.1 => true,
        (page, u) => (page, u) == plan.lt || (page, u) == plan.rt,
    }
}

/// True when this descriptor gives the triggers only as buttons: some
/// input report has Button-page usages that this layout reads as LT and
/// RT, and no input report has a field the decoder reads as an analog
/// trigger. Then the screen shows the trigger bars as on/off from the
/// start. Decided from the descriptor alone, never from the pad's name.
pub fn digital_triggers(layout: Layout, d: &Descriptor) -> bool {
    if d.is_empty() || matches!(layout, Layout::Unknown | Layout::SwitchPro | Layout::Xbox) {
        return false;
    }
    let mut ids = [0u8; crate::descriptor::MAX_REPORTS];
    let mut n = 0;
    for r in d
        .reports()
        .iter()
        .filter(|r| r.kind == crate::descriptor::Kind::Input)
    {
        if n < ids.len() {
            ids[n] = r.id;
            n += 1;
        }
    }
    let mut zl = false;
    let mut zr = false;
    for &id in &ids[..n] {
        let plan = axis_plan(d, id);
        for f in d.input_fields(id).filter(|f| !f.is_constant()) {
            let (page, u) = (f.usage_page, f.usage);
            if (page, u) == plan.lt || (page, u) == plan.rt {
                return false;
            }
            if page == 0x09 {
                for k in u..=f.usage_max.min(u.saturating_add(40)) {
                    match button_for(layout, k) {
                        Some(Btn::ZL) => zl = true,
                        Some(Btn::ZR) => zr = true,
                        _ => {}
                    }
                }
            }
        }
    }
    zl && zr
}

fn decode_generic(layout: Layout, d: &Descriptor, report: &[u8], pad: &mut Pad) -> bool {
    if d.is_empty() {
        return false;
    }
    let (id, body) = if d.uses_report_ids {
        match report.split_first() {
            Some((id, body)) => (*id, body),
            None => return false,
        }
    } else {
        (0, report)
    };
    if d.input_report(id).is_none() {
        return false;
    }
    let plan = axis_plan(d, id);
    let mut any = false;
    // Two masks: the buttons this report declares ("touched") and those it
    // says are down ("down"). Two fields may mean the same button (a pad
    // can send Home as button 13 and as Consumer "AC Home"): the button is
    // down if either says so. Buttons from other reports keep their value.
    let mut down = 0u32;
    let mut touched = 0u32;
    // Analog triggers past half way, for pads with no separate button.
    let mut trig_down = 0u32;
    let mut trig_seen = 0u32;
    let mut extra = 0u32;
    let mut hat_seen = false;
    let mut hat = None;
    for f in d.input_fields(id) {
        if f.is_constant() {
            continue;
        }
        if !f.is_variable() {
            // An array of buttons: each slot holds the index of a pressed one.
            if f.usage_page == 0x09 {
                // Every button the array can name is "touched": one not
                // listed is up.
                for n in f.usage..=f.usage_max.min(f.usage.saturating_add(40)) {
                    if let Some(b) = button_for(layout, n) {
                        touched |= b.bit();
                    }
                }
                for k in 0..f.count.min(16) {
                    let Some(v) = f.value(body, k) else { continue };
                    any = true;
                    // Done in 64 bits: a hostile descriptor can give ranges
                    // near the ends of i32, and 32-bit sums would overflow.
                    let n = v as i64 - f.logical_min as i64 + f.usage as i64;
                    if v >= f.logical_min
                        && v <= f.logical_max
                        && (1..=u16::MAX as i64).contains(&n)
                    {
                        match button_for(layout, n as u16) {
                            Some(b) => down |= b.bit(),
                            None if n <= 32 => extra |= 1 << (n - 1),
                            None => {}
                        }
                    }
                }
            }
            continue;
        }
        let Some(v) = f.value(body, 0) else { continue };
        any = true;
        let on = v != 0;
        match (f.usage_page, f.usage) {
            (0x09, n) => match button_for(layout, n) {
                Some(b) => set_btn(&mut down, &mut touched, b, on),
                None if (1..=32).contains(&n) && on => extra |= 1 << (n - 1),
                None => {}
            },
            (0x01, 0x39) => {
                hat_seen = true;
                let span = f.logical_max as i64 - f.logical_min as i64;
                let pos = v as i64 - f.logical_min as i64;
                hat = if pos >= 0 && pos <= span && span >= 3 {
                    // A 4-way hat (range 0..3) counts in quarter turns.
                    Some(if span == 3 {
                        (pos * 2) as u8
                    } else {
                        pos as u8 & 7
                    })
                } else {
                    None
                };
            }
            (0x01, 0x90) => set_btn(&mut down, &mut touched, Btn::Up, on),
            (0x01, 0x91) => set_btn(&mut down, &mut touched, Btn::Down, on),
            (0x01, 0x92) => set_btn(&mut down, &mut touched, Btn::Right, on),
            (0x01, 0x93) => set_btn(&mut down, &mut touched, Btn::Left, on),
            (0x01, 0x85) => set_btn(&mut down, &mut touched, Btn::Home, on),
            (0x0C, 0x223) => set_btn(&mut down, &mut touched, Btn::Home, on),
            (0x0C, 0x224) if ac_back_is_minus(layout, d) => {
                set_btn(&mut down, &mut touched, Btn::Minus, on)
            }
            (0x0C, 0x40) => set_btn(&mut down, &mut touched, Btn::Plus, on),
            (0x0C, 0xB2) => set_btn(&mut down, &mut touched, Btn::Capture, on),
            (0x06, 0x20) => pad.battery = Some(v.clamp(0, 255) as u8),
            (0x01, 0x30) => {
                let (c, h) = range(f);
                pad.lx = axis(v, c, h);
            }
            (0x01, 0x31) => {
                let (c, h) = range(f);
                pad.ly = axis(v, c, h);
            }
            (page, u) if page == 1 && u == plan.right.0 => {
                let (c, h) = range(f);
                pad.rx = axis(v, c, h);
            }
            (page, u) if page == 1 && u == plan.right.1 => {
                let (c, h) = range(f);
                pad.ry = axis(v, c, h);
            }
            (page, u) if (page, u) == plan.lt || (page, u) == plan.rt => {
                let lo = f.logical_min.max(0) as i64;
                let hi = if f.logical_max as i64 > lo {
                    f.logical_max as i64
                } else {
                    (1i64 << f.bit_size.min(31)) - 1
                };
                let value =
                    (((v as i64 - lo).max(0) * 1023) / (hi - lo).max(1)).clamp(0, 1023) as u16;
                let t = Trigger {
                    raw: v,
                    value,
                    analog: true,
                    present: true,
                };
                let (which, b) = if (page, u) == plan.lt {
                    (&mut pad.lt, Btn::ZL)
                } else {
                    (&mut pad.rt, Btn::ZR)
                };
                *which = t;
                // An analog trigger also counts as pressed past half way,
                // unless the pad has a separate button for it.
                trig_seen |= b.bit();
                if value > 512 {
                    trig_down |= b.bit();
                }
            }
            _ => {}
        }
    }
    let trig_only = trig_seen & !touched;
    touched |= trig_only;
    down |= trig_down & trig_only;
    pad.buttons = (pad.buttons & !touched) | (down & touched);
    pad.extra = extra;
    if hat_seen {
        pad.set_hat(hat);
    } else if touched & (Btn::Up.bit() | Btn::Down.bit() | Btn::Left.bit() | Btn::Right.bit()) != 0
    {
        pad.hat_from_buttons();
    }
    // Triggers that are only buttons.
    if !pad.lt.analog && touched & Btn::ZL.bit() != 0 {
        let on = pad.pressed(Btn::ZL);
        pad.lt = Trigger {
            raw: on as i32,
            value: if on { 1023 } else { 0 },
            analog: false,
            present: true,
        };
    }
    if !pad.rt.analog && touched & Btn::ZR.bit() != 0 {
        let on = pad.pressed(Btn::ZR);
        pad.rt = Trigger {
            raw: on as i32,
            value: if on { 1023 } else { 0 },
            analog: false,
            present: true,
        };
    }
    any
}

/// Note that this report says something about button `b`.
fn set_btn(down: &mut u32, touched: &mut u32, b: Btn, on: bool) {
    *touched |= b.bit();
    if on {
        *down |= b.bit();
    }
}
