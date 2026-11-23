//! What the scanner has heard: one entry per advertiser, and the counters.
//!
//! Everything here treats what arrives as hostile input: lengths are
//! checked before anything is read, nothing is trusted to be text, and the
//! table has a fixed size and simply stops taking new devices when full.

use core::fmt::{self, Write as _};

use heapless::Vec;

use crate::beacon;

/// How many different advertisers are remembered. Phones change their
/// address every few minutes, so a busy place fills this in time; what
/// does not fit is counted in `Counters::not_stored`.
pub const DEVICES_MAX: usize = 256;

/// HID over GATT, the service every Low Energy keyboard, mouse and gamepad
/// has.
pub const UUID_HID: u16 = 0x1812;
/// Appearance values (a 16-bit "what am I" number): gamepad and joystick.
pub const APPEARANCE_GAMEPAD: u16 = 0x03C4;
pub const APPEARANCE_JOYSTICK: u16 = 0x03C3;

/// Why a device is taken for a game controller. Bits, so that several can
/// be shown.
pub mod why {
    pub const HID_SERVICE: u8 = 1;
    pub const GAMEPAD_APPEARANCE: u8 = 2;
    pub const JOYSTICK_APPEARANCE: u8 = 4;
    /// The name says so. Only a hint: a name can say anything.
    pub const NAME_HINT: u8 = 8;
}

/// One advertiser.
#[derive(Clone)]
pub struct Device {
    /// As it came from the radio: least significant byte first.
    pub addr: [u8; 6],
    /// 0 public, 1 random (2 and 3: the same two, resolved by the radio).
    pub addr_kind: u8,
    pub rssi: i8,
    pub rssi_best: i8,
    /// Which kinds of packet were heard from it, one bit per kind
    /// (bit 0 ADV_IND ... bit 4 SCAN_RSP).
    pub kinds: u8,
    pub flags: Option<u8>,
    pub appearance: Option<u16>,
    /// Company number of its manufacturer data, if it sent any.
    pub company: Option<u16>,
    pub uuids16: Vec<u16, 6>,
    /// 16-bit UUID of service data, if it sent any (first one only).
    pub service_data: Option<u16>,
    pub has_uuid128: bool,
    /// The name as bytes: it need not be valid text.
    pub name: Vec<u8, 29>,
    pub reports: u32,
    pub controller: u8,
    /// Something about it is new and not yet printed.
    pub news: bool,
    pub printed: bool,
}

impl Device {
    const fn empty() -> Self {
        Self {
            addr: [0; 6],
            addr_kind: 0,
            rssi: 0,
            rssi_best: i8::MIN,
            kinds: 0,
            flags: None,
            appearance: None,
            company: None,
            uuids16: Vec::new(),
            service_data: None,
            has_uuid128: false,
            name: Vec::new(),
            reports: 0,
            controller: 0,
            news: false,
            printed: false,
        }
    }

    /// Public, or which of the three kinds of random address. The top two
    /// bits of a random address say which.
    pub fn addr_type(&self) -> AddrType {
        if self.addr_kind & 1 == 0 {
            return AddrType::Public;
        }
        match self.addr[5] >> 6 {
            0b11 => AddrType::RandomStatic,
            0b01 => AddrType::RandomResolvable,
            0b00 => AddrType::RandomNonResolvable,
            _ => AddrType::RandomReserved,
        }
    }

    /// Takes in one advertisement's data. Returns true when it taught us
    /// something about the device that we did not know.
    fn learn(&mut self, data: &[u8]) -> bool {
        let mut new = false;
        for (kind, body) in Fields(data) {
            match kind {
                // Flags
                0x01 => {
                    if let Some(&flags) = body.first() {
                        new |= self.flags.replace(flags) != Some(flags);
                    }
                }
                // Lists of 16-bit service UUIDs, incomplete and complete
                0x02 | 0x03 => {
                    for pair in body.chunks_exact(2) {
                        let uuid = u16::from_le_bytes([pair[0], pair[1]]);
                        if !self.uuids16.contains(&uuid) && self.uuids16.push(uuid).is_ok() {
                            new = true;
                        }
                    }
                }
                // Lists of 128-bit service UUIDs
                0x06 | 0x07 => {
                    if body.len() >= 16 && !self.has_uuid128 {
                        self.has_uuid128 = true;
                        new = true;
                    }
                }
                // Shortened and complete local name. The complete one wins.
                0x08 | 0x09 => {
                    if (self.name.is_empty() || kind == 0x09) && self.name.as_slice() != body {
                        let fits = body.len().min(self.name.capacity());
                        self.name.clear();
                        // Cannot fail: `fits` is at most the capacity.
                        let _ = self.name.extend_from_slice(&body[..fits]);
                        new = true;
                    }
                }
                // Service data with a 16-bit UUID
                0x16 => {
                    if let [a, b, ..] = *body
                        && self.service_data.is_none()
                    {
                        self.service_data = Some(u16::from_le_bytes([a, b]));
                        new = true;
                    }
                }
                // Appearance
                0x19 => {
                    if let [a, b, ..] = *body {
                        let appearance = u16::from_le_bytes([a, b]);
                        new |= self.appearance.replace(appearance) != Some(appearance);
                    }
                }
                // Manufacturer data: a company number, then anything
                0xFF => {
                    if let [a, b, ..] = *body
                        && self.company.is_none()
                    {
                        self.company = Some(u16::from_le_bytes([a, b]));
                        new = true;
                    }
                }
                _ => {}
            }
        }

        let mut controller = 0;
        if self.uuids16.contains(&UUID_HID) {
            controller |= why::HID_SERVICE;
        }
        match self.appearance {
            Some(APPEARANCE_GAMEPAD) => controller |= why::GAMEPAD_APPEARANCE,
            Some(APPEARANCE_JOYSTICK) => controller |= why::JOYSTICK_APPEARANCE,
            _ => {}
        }
        if name_hints_controller(&self.name) {
            controller |= why::NAME_HINT;
        }
        new |= controller != self.controller;
        self.controller = controller;
        new
    }
}

/// Words that pads put in their names. Compared without regard to case.
fn name_hints_controller(name: &[u8]) -> bool {
    const WORDS: [&[u8]; 6] = [b"controller", b"gamepad", b"xbox", b"joystick", b"gulikit", b"joy-con"];
    WORDS.iter().any(|word| name.windows(word.len()).any(|part| part.eq_ignore_ascii_case(word)))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AddrType {
    Public,
    RandomStatic,
    RandomResolvable,
    RandomNonResolvable,
    RandomReserved,
}

impl AddrType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::RandomStatic => "random-static",
            Self::RandomResolvable => "random-resolvable",
            Self::RandomNonResolvable => "random-nonresolvable",
            Self::RandomReserved => "random-reserved",
        }
    }
}

/// Walks the fields of an advertisement: each is a length byte, a type
/// byte and `length - 1` bytes of content. Stops at the first field that
/// does not fit, and at a zero length, which is padding.
struct Fields<'a>(&'a [u8]);

impl<'a> Iterator for Fields<'a> {
    type Item = (u8, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let (&len, rest) = self.0.split_first()?;
        let len = usize::from(len);
        if len == 0 || len > rest.len() {
            self.0 = &[];
            return None;
        }
        let (field, rest) = rest.split_at(len);
        self.0 = rest;
        Some((field[0], &field[1..]))
    }
}

/// Finds our own kind of beacon in an advertisement.
fn passby_payload(data: &[u8]) -> Option<&[u8; beacon::PAYLOAD_LEN]> {
    Fields(data).find_map(|(kind, body)| {
        let (company, payload) = body.split_first_chunk::<2>()?;
        (kind == 0xFF && u16::from_le_bytes(*company) == beacon::COMPANY).then_some(())?;
        let payload: &[u8; beacon::PAYLOAD_LEN] = payload.try_into().ok()?;
        (payload[0] == beacon::VERSION).then_some(payload)
    })
}

/// Counts. `adv_*` count every advertisement the radio reported, repeats
/// included: that is the figure the listening share is judged by.
#[derive(Clone, Copy, Default)]
pub struct Counters {
    pub adv_total: u32,
    pub adv_phase: u32,
    pub adv_second: u32,
    /// By kind of packet: ADV_IND, ADV_DIRECT_IND, ADV_SCAN_IND,
    /// ADV_NONCONN_IND, SCAN_RSP.
    pub by_kind: [u32; 5],
    /// Reports the library could not take apart.
    pub broken: u32,
    /// New devices that found the table full.
    pub not_stored: u32,
    pub new_phase: u32,
    /// Beacons of our own format heard (from another console).
    pub passby: u32,
}

pub struct Seen {
    devices: Vec<Device, DEVICES_MAX>,
    pub count: Counters,
    /// The rotating number of the last pass-by beacon heard, to echo.
    pub last_passby: Option<beacon::Heard>,
    pub passby_news: bool,
}

impl Seen {
    pub const fn new() -> Self {
        Self { devices: Vec::new(), count: Counters { adv_total: 0, adv_phase: 0, adv_second: 0, by_kind: [0; 5], broken: 0, not_stored: 0, new_phase: 0, passby: 0 }, last_passby: None, passby_news: false }
    }

    pub fn devices(&self) -> &[Device] {
        &self.devices
    }

    /// One advertisement, as the radio reported it.
    pub fn heard(&mut self, kind: u8, addr_kind: u8, addr: &[u8; 6], data: &[u8], rssi: i8) {
        self.count.adv_total = self.count.adv_total.wrapping_add(1);
        self.count.adv_phase = self.count.adv_phase.wrapping_add(1);
        self.count.adv_second = self.count.adv_second.wrapping_add(1);
        if let Some(n) = self.count.by_kind.get_mut(usize::from(kind)) {
            *n = n.wrapping_add(1);
        }

        if let Some(payload) = passby_payload(data) {
            self.count.passby = self.count.passby.wrapping_add(1);
            let heard = beacon::Heard::from_payload(payload, rssi);
            if self.last_passby.as_ref().map(|last| last.id) != Some(heard.id) {
                self.passby_news = true;
            }
            self.last_passby = Some(heard);
        }

        let at = match self.devices.iter().position(|d| d.addr == *addr && d.addr_kind == addr_kind) {
            Some(at) => at,
            None => {
                let mut device = Device::empty();
                device.addr = *addr;
                device.addr_kind = addr_kind;
                device.news = true;
                if self.devices.push(device).is_err() {
                    self.count.not_stored = self.count.not_stored.wrapping_add(1);
                    return;
                }
                self.count.new_phase += 1;
                self.devices.len() - 1
            }
        };
        let device = &mut self.devices[at];
        device.reports = device.reports.wrapping_add(1);
        device.rssi = rssi;
        device.rssi_best = device.rssi_best.max(rssi);
        device.kinds |= 1 << kind.min(7);
        if device.learn(data) {
            device.news = true;
        }
    }

    /// Hands out a copy of one device that has news, and marks it as told.
    /// A copy, so that the caller can take its time printing while the
    /// table goes on being filled.
    pub fn take_news(&mut self) -> Option<(usize, bool, Device)> {
        let at = self.devices.iter().position(|d| d.news)?;
        let device = &mut self.devices[at];
        let first = !device.printed;
        device.news = false;
        device.printed = true;
        Some((at + 1, first, device.clone()))
    }

    /// Counts by category, with nothing in them that points at a device.
    pub fn summary(&self) -> Summary {
        let mut s = Summary { devices: self.devices.len(), ..Summary::default() };
        for d in &self.devices {
            match d.addr_type() {
                AddrType::Public => s.public += 1,
                AddrType::RandomStatic => s.random_static += 1,
                AddrType::RandomResolvable => s.resolvable += 1,
                AddrType::RandomNonResolvable | AddrType::RandomReserved => s.non_resolvable += 1,
            }
            s.named += usize::from(!d.name.is_empty());
            s.with_flags += usize::from(d.flags.is_some());
            s.with_uuid16 += usize::from(!d.uuids16.is_empty());
            s.with_uuid128 += usize::from(d.has_uuid128);
            s.with_service_data += usize::from(d.service_data.is_some());
            s.with_company += usize::from(d.company.is_some());
            s.with_appearance += usize::from(d.appearance.is_some());
            s.connectable += usize::from(d.kinds & 0b11 != 0);
            s.hid += usize::from(d.controller & why::HID_SERVICE != 0);
            s.pad_appearance += usize::from(d.controller & (why::GAMEPAD_APPEARANCE | why::JOYSTICK_APPEARANCE) != 0);
            s.controller_like += usize::from(d.controller & !why::NAME_HINT != 0);
            s.name_hint_only += usize::from(d.controller == why::NAME_HINT);
        }
        s
    }
}

#[derive(Default)]
pub struct Summary {
    pub devices: usize,
    pub public: usize,
    pub random_static: usize,
    pub resolvable: usize,
    pub non_resolvable: usize,
    pub named: usize,
    pub with_flags: usize,
    pub with_uuid16: usize,
    pub with_uuid128: usize,
    pub with_service_data: usize,
    pub with_company: usize,
    pub with_appearance: usize,
    pub connectable: usize,
    pub hid: usize,
    pub pad_appearance: usize,
    pub controller_like: usize,
    pub name_hint_only: usize,
}

/// An address the way people write it: most significant byte first.
pub struct Addr<'a>(pub &'a [u8; 6]);

impl fmt::Display for Addr<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [a, b, c, d, e, g] = self.0;
        write!(f, "{g:02x}:{e:02x}:{d:02x}:{c:02x}:{b:02x}:{a:02x}")
    }
}

/// A name that may hold anything, printed so that it cannot break the log
/// line: printable ASCII as it is, everything else as \xNN.
pub struct Name<'a>(pub &'a [u8]);

impl fmt::Display for Name<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_char('"')?;
        for &byte in self.0 {
            match byte {
                b'"' | b'\\' => write!(f, "\\x{byte:02x}")?,
                0x20..=0x7E => f.write_char(char::from(byte))?,
                _ => write!(f, "\\x{byte:02x}")?,
            }
        }
        f.write_char('"')
    }
}

/// The reasons a device counts as a controller, as words.
pub struct Why(pub u8);

impl fmt::Display for Why {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for (bit, word) in [
            (why::HID_SERVICE, "hid-service-0x1812"),
            (why::GAMEPAD_APPEARANCE, "appearance-gamepad-0x03c4"),
            (why::JOYSTICK_APPEARANCE, "appearance-joystick-0x03c3"),
            (why::NAME_HINT, "name-hint"),
        ] {
            if self.0 & bit != 0 {
                if !first {
                    f.write_char('+')?;
                }
                f.write_str(word)?;
                first = false;
            }
        }
        if first { f.write_str("no") } else { Ok(()) }
    }
}

/// The kinds of packet heard from a device, as words.
pub struct Kinds(pub u8);

impl fmt::Display for Kinds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for (bit, word) in ["ADV_IND", "ADV_DIRECT_IND", "ADV_SCAN_IND", "ADV_NONCONN_IND", "SCAN_RSP"].iter().enumerate() {
            if self.0 & (1 << bit) != 0 {
                if !first {
                    f.write_char('+')?;
                }
                f.write_str(word)?;
                first = false;
            }
        }
        Ok(())
    }
}
