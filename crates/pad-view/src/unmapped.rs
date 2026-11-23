//! Input the decoder does not map to the standard gamepad.
//!
//! The decoder turns a report into a [`crate::decode::Pad`]: buttons,
//! sticks, triggers, d-pad. Whatever else the report carries (a vendor
//! bit, a consumer key such as Play/Pause, a button with no standard
//! name, padding that should never change) would be invisible. This
//! module keeps it visible.
//!
//! For each report id it keeps a mask of the bits the decoder reads
//! ("used"), the first report of that id ("base") and the newest one
//! ("last"). An unmapped bit is "on" while it differs from its base value.
//! The base is the first report, except that a declared one-bit field
//! (a button, a consumer key, a vendor flag) is off at 0: a pad sends a
//! consumer report only when such a key is pressed, so its first report
//! already has a bit on. Bytes the descriptor does not declare at all
//! (past the declared size of a report, or every byte of a report id it
//! does not declare) are off at 0 too, so a pad that sends more than its
//! descriptor says shows it even in its first report. Padding inside a
//! declared report and multi-bit values are on while they differ from the
//! first report, so a byte that idles at 0x80 is not on until it changes.
//!
//! A multi-bit value is one thing for the trace: it changes the "on" set
//! only when it leaves its first value or comes back to it, not at every
//! step, so a noisy unmapped byte does not turn every report into a
//! "change" line.
//!
//! Which bits the decoder reads:
//! - layouts decoded by hand (Xbox, Switch): the bits [`hand_used`] lists,
//!   which are exactly the bits decode.rs reads for those report ids;
//! - layouts decoded from the descriptor: every field
//!   [`crate::decode::generic_uses`] says the generic decoder reads.
//!
//! Every other bit is unmapped: fields the descriptor declares that no
//! standard control takes, padding the descriptor declares constant, and
//! bytes the descriptor does not declare at all.

use core::fmt::Write;

use crate::decode::{self, Layout};
use crate::descriptor::{Descriptor, Field, read_bits};

/// Bytes tracked per report (longer reports: only the first ones).
pub const TRACK_BYTES: usize = 64;
/// Report ids tracked; reports of further ids are not tracked.
pub const TRACK_IDS: usize = 4;

/// One tracked report id.
#[derive(Clone, Copy)]
struct Slot {
    /// The report id as the View keys it (0 when the pad uses no ids).
    key: u8,
    /// The real length of the last report (may be more than TRACK_BYTES).
    len: u16,
    /// 1 = the decoder reads this bit.
    used: [u8; TRACK_BYTES],
    base: [u8; TRACK_BYTES],
    last: [u8; TRACK_BYTES],
    /// Unmapped bits that were on at some moment since the last snapshot.
    latched: [u8; TRACK_BYTES],
}

const SLOT0: Slot = Slot {
    key: 0,
    len: 0,
    used: [0; TRACK_BYTES],
    base: [0; TRACK_BYTES],
    last: [0; TRACK_BYTES],
    latched: [0; TRACK_BYTES],
};

/// What one report changed, for the serial trace.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    /// Byte for byte the same as the last report of this id.
    pub same: bool,
    /// The first report of this id (since the tracker was cleared).
    pub first: bool,
    /// The set of unmapped things that are on changed.
    pub on_changed: bool,
}

/// What an [`Item`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    /// A field the descriptor declares, which no standard control takes.
    Field,
    /// Bits the descriptor declares constant (padding).
    Padding,
    /// Bits the descriptor does not declare (or there is no descriptor).
    Undeclared,
}

/// One unmapped thing that is on: a single bit, or one multi-bit value of
/// a declared field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    /// The report id byte (0 when the pad uses no ids).
    pub id: u8,
    /// Position in the report as it arrived (byte 0 is the report id when
    /// the pad uses ids), as in the hex dumps.
    pub byte: u8,
    pub bit: u8,
    /// 1 for a bit, more for a value.
    pub bits: u8,
    /// The value now.
    pub value: u32,
    pub how: How,
    pub page: u16,
    pub usage: u16,
}

impl Item {
    /// "07 b8.0 vendor FF00:1", "02 b1.0 Play/Pause", "07 b5 usage 0=18".
    pub fn label<W: Write>(&self, w: &mut W) {
        let _ = write!(w, "{:02X} b{}", self.id, self.byte);
        if self.bits == 1 || self.bit != 0 || !self.bits.is_multiple_of(8) {
            let _ = write!(w, ".{}", self.bit);
        }
        let _ = w.write_char(' ');
        self.name(w);
        if self.bits > 1 {
            let _ = write!(w, "={}", self.value);
        }
    }

    /// The name alone: the usage when the descriptor gives one.
    pub fn name<W: Write>(&self, w: &mut W) {
        match self.how {
            How::Padding => {
                let _ = w.write_str("padding");
            }
            How::Undeclared => {
                let _ = w.write_str("undeclared");
            }
            How::Field => usage_long_name(w, self.page, self.usage),
        }
    }
}

/// A usage in words, for unmapped items (longer than
/// [`crate::text::usage_name`]).
pub fn usage_long_name<W: Write>(w: &mut W, page: u16, usage: u16) {
    let s = match (page, usage) {
        (0x01, 0x00) => "usage 0",
        (0x01, 0x30) => "X axis",
        (0x01, 0x31) => "Y axis",
        (0x01, 0x32) => "Z axis",
        (0x01, 0x33) => "Rx axis",
        (0x01, 0x34) => "Ry axis",
        (0x01, 0x35) => "Rz axis",
        (0x01, 0x36) => "Slider",
        (0x01, 0x37) => "Dial",
        (0x01, 0x38) => "Wheel",
        (0x01, 0x39) => "Hat",
        (0x01, 0x85) => "System Menu",
        (0x0C, 0x40) => "Menu",
        (0x0C, 0xB2) => "Record",
        (0x0C, 0xB5) => "Scan Next",
        (0x0C, 0xB6) => "Scan Prev",
        (0x0C, 0xB7) => "Stop",
        (0x0C, 0xB8) => "Eject",
        (0x0C, 0xCD) => "Play/Pause",
        (0x0C, 0xE2) => "Mute",
        (0x0C, 0xE9) => "Vol Up",
        (0x0C, 0xEA) => "Vol Down",
        (0x0C, 0x223) => "AC Home",
        (0x0C, 0x224) => "AC Back",
        (0x09, n) => {
            let _ = write!(w, "button {}", n);
            return;
        }
        (p, n) if p >= 0xFF00 => {
            let _ = write!(w, "vendor {:04X}:{:X}", p, n);
            return;
        }
        (p, n) => {
            let _ = write!(w, "usage {:X}:{:X}", p, n);
            return;
        }
    };
    let _ = w.write_str(s);
}

/// The tracker. About 1 KB.
pub struct Tracker {
    slots: [Slot; TRACK_IDS],
    n: usize,
    /// Reports whose unmapped "on" set changed (since cleared).
    pub changes: u32,
    /// Reports of an id that did not fit in the table.
    pub untracked: u32,
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new()
    }
}

impl Tracker {
    pub const fn new() -> Self {
        Tracker {
            slots: [SLOT0; TRACK_IDS],
            n: 0,
            changes: 0,
            untracked: 0,
        }
    }

    /// Forget everything (new layout, new descriptor, new connection).
    pub fn clear(&mut self) {
        self.n = 0;
        self.changes = 0;
        self.untracked = 0;
    }

    /// Take one report. `key` is its report id as the View keys it.
    pub fn report(&mut self, layout: Layout, d: &Descriptor, key: u8, r: &[u8]) -> Seen {
        let n = r.len().min(TRACK_BYTES);
        let i = match self.slots[..self.n].iter().position(|s| s.key == key) {
            Some(i) => i,
            None if self.n < TRACK_IDS => {
                self.n += 1;
                let s = &mut self.slots[self.n - 1];
                s.key = key;
                start(s, layout, d, r);
                let on = s.latched.iter().any(|b| *b != 0);
                if on {
                    self.changes = self.changes.wrapping_add(1);
                }
                return Seen {
                    same: false,
                    first: true,
                    on_changed: on,
                };
            }
            None => {
                self.untracked = self.untracked.wrapping_add(1);
                return Seen::default();
            }
        };
        let s = &mut self.slots[i];
        let same = s.len as usize == r.len() && s.last[..n] == r[..n];
        if s.len as usize != r.len() {
            // Another length: another meaning (the Xbox layout tells two
            // button orders apart by length). Start this id again.
            start(s, layout, d, r);
            let on = s.latched.iter().any(|b| *b != 0);
            return Seen {
                same: false,
                first: false,
                on_changed: on,
            };
        }
        // A multi-bit value counts as one thing: it changes the "on" set
        // only when it leaves its base value or comes back to it. A value
        // that moves while it is away from its base (a noisy byte) changes
        // what is shown, not what is on, so it does not make every report
        // a "change" line in the trace.
        let mut values = [0u8; TRACK_BYTES];
        let mut on_changed = false;
        value_spans(layout, d, r, |from, len| {
            mark(&mut values, from, len);
            let was = any_on(&s.last[..n], &s.base, &s.used, from, len);
            let is = any_on(&r[..n], &s.base, &s.used, from, len);
            on_changed |= was != is;
        });
        for k in 0..n {
            let old_on = (s.last[k] ^ s.base[k]) & !s.used[k];
            let new_on = (r[k] ^ s.base[k]) & !s.used[k];
            on_changed |= (old_on ^ new_on) & !values[k] != 0;
            s.latched[k] |= new_on;
            s.last[k] = r[k];
        }
        if on_changed {
            self.changes = self.changes.wrapping_add(1);
        }
        Seen {
            same,
            first: false,
            on_changed,
        }
    }

    /// Unmapped things on now (`latched` false), or on at some moment since
    /// the last [`Tracker::relatch`] (`latched` true). Writes as many as fit
    /// into `out` and returns how many there are in all.
    pub fn items(&self, d: &Descriptor, latched: bool, out: &mut [Item]) -> usize {
        let mut total = 0usize;
        for s in &self.slots[..self.n] {
            let n = (s.len as usize).min(TRACK_BYTES);
            let mut mask = [0u8; TRACK_BYTES];
            for k in 0..n {
                let on = (s.last[k] ^ s.base[k]) & !s.used[k];
                mask[k] = if latched { s.latched[k] | on } else { on };
            }
            slot_items(s, d, &mask[..n], &mut |it| {
                if total < out.len() {
                    out[total] = it;
                }
                total += 1;
            });
        }
        total
    }

    /// A snapshot was taken: from now on only what is on now, or turns on
    /// later, counts as latched.
    pub fn relatch(&mut self) {
        for s in &mut self.slots[..self.n] {
            for k in 0..TRACK_BYTES {
                s.latched[k] = (s.last[k] ^ s.base[k]) & !s.used[k];
            }
        }
    }

    /// The last report of this id, if tracked (at most TRACK_BYTES bytes).
    pub fn last(&self, key: u8) -> Option<&[u8]> {
        let s = self.slots[..self.n].iter().find(|s| s.key == key)?;
        Some(&s.last[..(s.len as usize).min(TRACK_BYTES)])
    }
}

/// Start an id afresh: this report is the base.
fn start(s: &mut Slot, layout: Layout, d: &Descriptor, r: &[u8]) {
    let n = r.len().min(TRACK_BYTES);
    s.len = r.len().min(u16::MAX as usize) as u16;
    s.base = [0; TRACK_BYTES];
    s.base[..n].copy_from_slice(&r[..n]);
    s.last = s.base;
    s.latched = [0; TRACK_BYTES];
    s.used = [0; TRACK_BYTES];
    used_bits(layout, d, r, &mut s.used);
    // A declared one-bit field is off at 0, whatever the first report
    // says: a pad sends its consumer report (0x02 on the GuliKit) only
    // when such a key is pressed, so the first one has a bit on.
    if layout != Layout::Unknown && !d.is_empty() {
        let key = if d.uses_report_ids {
            r.first().copied().unwrap_or(0)
        } else {
            0
        };
        let body = body_bits(d);
        for f in d.input_fields(key) {
            if f.bit_size == 1 && !f.is_constant() {
                let from = body + f.bit_offset as u32;
                let end = from
                    .saturating_add(f.count as u32)
                    .min(TRACK_BYTES as u32 * 8);
                for b in from..end {
                    s.base[(b / 8) as usize] &= !(1 << (b % 8));
                }
            }
        }
    }
    // Bytes the descriptor does not declare (past the declared size, or
    // all of a report id it does not declare) are off at 0 too: a pad
    // that sends more than its descriptor says, with something already
    // on in its first report, would otherwise never show it.
    if layout != Layout::Unknown && !d.is_empty() {
        let key = if d.uses_report_ids {
            r.first().copied().unwrap_or(0)
        } else {
            0
        };
        let declared = match d.input_report(key) {
            Some(rep) => rep.bytes(d.uses_report_ids) as usize,
            None => d.uses_report_ids as usize,
        };
        for k in declared.min(n)..n {
            s.base[k] &= s.used[k];
        }
    }
    for k in 0..TRACK_BYTES {
        s.latched[k] = (s.last[k] ^ s.base[k]) & !s.used[k];
    }
}

/// Each multi-bit value of a declared, non-constant input field of this
/// report: `f(first bit in the whole report, bits)`.
fn value_spans(layout: Layout, d: &Descriptor, r: &[u8], mut f: impl FnMut(u32, u32)) {
    if layout == Layout::Unknown || d.is_empty() {
        return;
    }
    let key = if d.uses_report_ids {
        r.first().copied().unwrap_or(0)
    } else {
        0
    };
    let body = body_bits(d);
    for fl in d.input_fields(key) {
        if fl.is_constant() || fl.bit_size <= 1 || fl.bit_size > 32 {
            continue;
        }
        for k in 0..fl.count as u32 {
            let from = body + fl.bit_offset as u32 + k * fl.bit_size as u32;
            if from >= TRACK_BYTES as u32 * 8 {
                break;
            }
            f(from, fl.bit_size as u32);
        }
    }
}

/// True when some unused bit in `from..from+len` of `r` differs from the
/// base. Bits past the end of `r` count as equal.
fn any_on(
    r: &[u8],
    base: &[u8; TRACK_BYTES],
    used: &[u8; TRACK_BYTES],
    from: u32,
    len: u32,
) -> bool {
    let end = from.saturating_add(len).min(r.len() as u32 * 8);
    (from..end).any(|b| {
        let (k, m) = ((b / 8) as usize, 1u8 << (b % 8));
        (r[k] ^ base[k]) & !used[k] & m != 0
    })
}

/// Mark bits `from..from+len` (bit numbers in the whole report) as used.
fn mark(used: &mut [u8; TRACK_BYTES], from: u32, len: u32) {
    let end = from.saturating_add(len).min(TRACK_BYTES as u32 * 8);
    for b in from..end {
        used[(b / 8) as usize] |= 1 << (b % 8);
    }
}

/// Where the report body starts, in bits: after the report id byte when
/// there is one. A hand-decoded pad without a descriptor always has one.
fn body_bits(d: &Descriptor) -> u32 {
    if d.is_empty() || d.uses_report_ids {
        8
    } else {
        0
    }
}

/// Which bits of this report the decoder reads.
fn used_bits(layout: Layout, d: &Descriptor, r: &[u8], used: &mut [u8; TRACK_BYTES]) {
    if layout == Layout::Unknown {
        // Nothing is decoded: the report is only shown raw. Tracking every
        // bit as unmapped would only be noise.
        *used = [0xFF; TRACK_BYTES];
        return;
    }
    let body = body_bits(d);
    if body == 8 {
        used[0] = 0xFF;
    }
    if let Some(bytes) = hand_used(layout, r) {
        for (k, m) in bytes.iter().enumerate().take(TRACK_BYTES) {
            used[k] |= *m;
        }
        return;
    }
    // Decoded from the descriptor. The hand layouts fall back to the
    // generic decoder with the plain button order (see decode::decode).
    let gl = match layout {
        Layout::SwitchPro | Layout::Xbox => Layout::Generic,
        l => l,
    };
    if d.is_empty() {
        return;
    }
    let key = if d.uses_report_ids {
        r.first().copied().unwrap_or(0)
    } else {
        0
    };
    let plan = decode::axis_plan(d, key);
    for f in d.input_fields(key) {
        if decode::generic_uses(gl, d, &plan, f) {
            mark(
                used,
                body + f.bit_offset as u32,
                f.bit_size as u32 * f.count as u32,
            );
        }
    }
}

/// The bits decode.rs reads by hand, per byte of the whole report, or
/// None when this report is not decoded by hand. Must match decode.rs.
fn hand_used(layout: Layout, r: &[u8]) -> Option<[u8; TRACK_BYTES]> {
    let mut m = [0u8; TRACK_BYTES];
    let all = |m: &mut [u8; TRACK_BYTES], from: usize, to: usize| {
        for b in m.iter_mut().take(to.min(TRACK_BYTES)).skip(from) {
            *b = 0xFF;
        }
    };
    match (layout, r.first()) {
        (Layout::SwitchPro, Some(0x3F)) if r.len() >= 12 => {
            all(&mut m, 0, 12);
            // Byte 2: -, +, LS, RS, Home, Capture, and ZR on some pads.
            m[2] = 0xBF;
        }
        (Layout::SwitchPro, Some(0x30 | 0x21 | 0x31)) if r.len() >= 13 => {
            // Timer, battery and connection, sticks, vibration, and from
            // byte 13 the motion samples or the subcommand answer: all
            // known, none of them a control the pad could add.
            all(&mut m, 0, TRACK_BYTES);
            // Right: Y X B A, (SR SL of a Joy-Con), R ZR.
            m[3] = 0xCF;
            // Middle: - + RS LS Home Capture, (bit 6 unused, bit 7 grip).
            m[4] = 0x3F;
            // Left: down up right left, (SR SL), L ZL.
            m[5] = 0xCF;
        }
        (Layout::Xbox, Some(0x01)) if r.len() >= 16 => {
            all(&mut m, 0, 10);
            m[10] = 0x03; // left trigger: 10 bits
            m[11] = 0xFF;
            m[12] = 0x03; // right trigger: 10 bits
            m[13] = 0xFF; // hat, read as a whole byte
            if r.len() == 16 {
                m[14] = 0xFF;
                m[15] = 0x03;
            } else {
                m[14] = 0xDB;
                m[15] = 0x7C;
                m[16] = 0x01;
            }
        }
        (Layout::Xbox, Some(0x02)) if r.len() >= 2 => {
            m[0] = 0xFF;
            m[1] = 0x01;
        }
        (Layout::Xbox, Some(0x04)) if r.len() >= 2 => {
            all(&mut m, 0, 2);
        }
        _ => return None,
    }
    Some(m)
}

/// The items of one slot whose bits are set in `mask`.
fn slot_items(s: &Slot, d: &Descriptor, mask: &[u8], put: &mut dyn FnMut(Item)) {
    let body = body_bits(d);
    let id = if body == 8 { s.key } else { 0 };
    let fkey = if d.uses_report_ids { s.key } else { 0 };
    let total = mask.len() as u32 * 8;
    let mut b = 0u32;
    while b < total {
        if mask[(b / 8) as usize] & (1 << (b % 8)) == 0 {
            b += 1;
            continue;
        }
        let field = if b >= body {
            find_field(d, fkey, b - body)
        } else {
            None
        };
        let bit_of = |at: u32| (s.last[(at / 8) as usize] >> (at % 8)) & 1;
        let mut it = Item {
            id,
            byte: (b / 8) as u8,
            bit: (b % 8) as u8,
            bits: 1,
            value: bit_of(b) as u32,
            how: How::Undeclared,
            page: 0,
            usage: 0,
        };
        match field {
            Some(f) if f.is_constant() => it.how = How::Padding,
            Some(f) => {
                it.how = How::Field;
                it.page = f.usage_page;
                it.usage = f.usage;
                if f.bit_size > 1 && f.bit_size <= 32 {
                    // A value: one item for the whole value, from its start.
                    let k = (b - body - f.bit_offset as u32) / f.bit_size as u32;
                    let at = f.bit_offset as u32 + k * f.bit_size as u32;
                    let whole = body + at;
                    it.byte = (whole / 8) as u8;
                    it.bit = (whole % 8) as u8;
                    it.bits = f.bit_size;
                    let n = (s.len as usize).min(TRACK_BYTES);
                    let body_bytes = &s.last[(body / 8) as usize..n];
                    it.value =
                        read_bits(body_bytes, at, f.bit_size as u32, false).unwrap_or(0) as u32;
                    put(it);
                    b = whole + f.bit_size as u32;
                    continue;
                }
            }
            None => {}
        }
        put(it);
        b += 1;
    }
}

/// The input field of report `key` that holds body bit `bit`.
fn find_field(d: &Descriptor, key: u8, bit: u32) -> Option<&Field> {
    d.input_fields(key).find(|f| {
        let from = f.bit_offset as u32;
        let len = f.bit_size as u32 * f.count as u32;
        bit >= from && bit < from + len
    })
}
