//! A HID report descriptor parser.
//!
//! A HID device describes its own reports with a "report descriptor": a
//! string of short items, each one or more bytes, defined in the USB HID
//! specification 1.11, section 6.2.2. Bluetooth HID uses the same format.
//!
//! An item's first byte holds three things:
//!
//! * bits 0-1: how many data bytes follow (0, 1, 2 or 4);
//! * bits 2-3: the type: 0 main, 1 global, 2 local;
//! * bits 4-7: the tag (what the item means).
//!
//! "Global" items (usage page, logical range, report size, report count,
//! report id) stay in force until changed. "Local" items (usages) apply to
//! the next main item only. A "main" item (Input, Output, Feature) creates
//! fields in a report, using the current global and local state.
//!
//! This parser turns the descriptor into a flat table of fields. It never
//! panics and always ends, whatever the bytes are: every loop moves forward
//! through the input or is bounded by a fixed table size. When a table is
//! full the rest is counted in [`Descriptor::dropped`] and ignored.

/// Most fields kept. A plain gamepad declares 20 to 40; a DualShock 4,
/// with its many vendor reports, 97.
pub const MAX_FIELDS: usize = 128;
/// Most reports (input, output and feature together) kept.
pub const MAX_REPORTS: usize = 40;
/// Most usages remembered for one main item.
const MAX_USAGES: usize = 24;
/// Depth of the Push/Pop stack for global items.
const MAX_STACK: usize = 4;

/// Which way a report travels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// From the device to the host (buttons, sticks).
    Input,
    /// From the host to the device (rumble, lights).
    Output,
    /// Read or written on request (settings).
    Feature,
}

/// Field flags, from the main item's data bits (HID 1.11, 6.2.2.5).
pub mod flag {
    /// Bit 0: constant (padding), not data.
    pub const CONSTANT: u16 = 1 << 0;
    /// Bit 1: variable (one value per usage). Clear means an array.
    pub const VARIABLE: u16 = 1 << 1;
    /// Bit 2: relative (a change) rather than absolute.
    pub const RELATIVE: u16 = 1 << 2;
    /// Bit 6: has a "null" state (a value outside the logical range means
    /// "nothing", used by hat switches for "centred").
    pub const NULL_STATE: u16 = 1 << 6;
}

/// One field of a report: where its bits are and what it means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    pub kind: Kind,
    /// The report id, or 0 when the descriptor uses no report ids.
    pub report_id: u8,
    /// Position of the first bit, counted from the first byte after the
    /// report id (bit 0 is the lowest bit of that byte).
    pub bit_offset: u16,
    /// Size of one value in bits (1 to 32 for values this crate reads).
    pub bit_size: u8,
    /// How many values of this size follow each other. Usually 1. More for
    /// arrays (one slot each) and for blocks of bytes with one usage.
    pub count: u16,
    /// See [`flag`].
    pub flags: u16,
    pub logical_min: i32,
    pub logical_max: i32,
    pub physical_min: i32,
    pub physical_max: i32,
    pub usage_page: u16,
    /// The usage, or the first usage of an array.
    pub usage: u16,
    /// The last usage of an array (same as `usage` for a variable).
    pub usage_max: u16,
}

impl Field {
    pub fn is_constant(&self) -> bool {
        self.flags & flag::CONSTANT != 0
    }
    pub fn is_variable(&self) -> bool {
        self.flags & flag::VARIABLE != 0
    }
    /// The value is signed when the logical range goes below zero.
    pub fn is_signed(&self) -> bool {
        self.logical_min < 0
    }
    /// Read value number `index` (0 for most fields) out of a report body
    /// (the bytes after the report id). None when the report is too short
    /// or the field is wider than 32 bits.
    pub fn value(&self, body: &[u8], index: u16) -> Option<i32> {
        if index >= self.count || self.bit_size == 0 || self.bit_size > 32 {
            return None;
        }
        let start = self.bit_offset as u32 + index as u32 * self.bit_size as u32;
        read_bits(body, start, self.bit_size as u32, self.is_signed())
    }
}

/// One report: its id, direction and size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    pub kind: Kind,
    pub id: u8,
    /// Size in bits, not counting the report id byte.
    pub bits: u32,
}

impl Report {
    /// Size in bytes as it travels, report id byte included when used.
    pub fn bytes(&self, uses_ids: bool) -> u32 {
        self.bits.div_ceil(8) + uses_ids as u32
    }
}

/// The parsed table.
#[derive(Clone, Debug)]
pub struct Descriptor {
    fields: [Field; MAX_FIELDS],
    n_fields: usize,
    reports: [Report; MAX_REPORTS],
    n_reports: usize,
    /// True when at least one Report ID item was seen.
    pub uses_report_ids: bool,
    /// Fields or reports that did not fit in the tables.
    pub dropped: u16,
    /// True when the bytes ended in the middle of an item, or a collection
    /// was left open, or something else did not add up. The table still
    /// holds everything read up to that point.
    pub damaged: bool,
    /// Number of collections opened.
    pub collections: u16,
}

const EMPTY_FIELD: Field = Field {
    kind: Kind::Input,
    report_id: 0,
    bit_offset: 0,
    bit_size: 0,
    count: 0,
    flags: 0,
    logical_min: 0,
    logical_max: 0,
    physical_min: 0,
    physical_max: 0,
    usage_page: 0,
    usage: 0,
    usage_max: 0,
};

const EMPTY_REPORT: Report = Report {
    kind: Kind::Input,
    id: 0,
    bits: 0,
};

/// The global state (HID 1.11, 6.2.2.7).
#[derive(Clone, Copy)]
struct Globals {
    usage_page: u16,
    logical_min: i32,
    logical_max: i32,
    /// The raw bytes of Logical Maximum, to fix a common mistake (see below).
    logical_max_raw: u32,
    logical_max_len: u8,
    physical_min: i32,
    physical_max: i32,
    report_size: u32,
    report_count: u32,
    report_id: u8,
}

const GLOBALS0: Globals = Globals {
    usage_page: 0,
    logical_min: 0,
    logical_max: 0,
    logical_max_raw: 0,
    logical_max_len: 0,
    physical_min: 0,
    physical_max: 0,
    report_size: 0,
    report_count: 0,
    report_id: 0,
};

/// One local usage entry: a single usage or a range, with its page.
#[derive(Clone, Copy)]
struct UsageRange {
    page: u16,
    min: u16,
    max: u16,
    /// True when the item gave only the usage number (1 or 2 bytes), so
    /// the page came from the Usage Page in force.
    short: bool,
}

struct Locals {
    list: [UsageRange; MAX_USAGES],
    n: usize,
    /// A Usage Minimum waiting for its Usage Maximum.
    pending_min: Option<(u16, u16, bool)>,
}

impl Locals {
    const fn new() -> Self {
        Locals {
            list: [UsageRange {
                page: 0,
                min: 0,
                max: 0,
                short: true,
            }; MAX_USAGES],
            n: 0,
            pending_min: None,
        }
    }
    fn clear(&mut self) {
        self.n = 0;
        self.pending_min = None;
    }
    fn push(&mut self, r: UsageRange) {
        if self.n < MAX_USAGES {
            self.list[self.n] = r;
            self.n += 1;
        }
    }
    /// Called at a main item. A short usage takes the Usage Page in force
    /// when it was declared. Some descriptors put the Usage Page item after
    /// the usages it is meant for; so the usages at the end of the list
    /// that do not match the current page are moved to it, stopping at the
    /// first one that already matches. (This is how widely used hosts read
    /// such descriptors.)
    fn settle_pages(&mut self, page: u16) {
        for r in self.list[..self.n].iter_mut().rev() {
            if !r.short {
                continue;
            }
            if r.page == page {
                break;
            }
            r.page = page;
        }
    }
    /// How many single usages the list holds (ranges counted in full).
    fn total(&self) -> u32 {
        let mut t = 0u32;
        for r in &self.list[..self.n] {
            t = t.saturating_add(r.max.saturating_sub(r.min) as u32 + 1);
        }
        t
    }
    /// Usage number `i` of the list, as (page, usage). Past the end, the
    /// last usage repeats, as the HID specification says.
    fn get(&self, i: u32, default_page: u16) -> Option<(u16, u16)> {
        if self.n == 0 {
            return None;
        }
        let mut i = i;
        for r in &self.list[..self.n] {
            let len = r.max.saturating_sub(r.min) as u32 + 1;
            if i < len {
                let page = if r.page != 0 { r.page } else { default_page };
                return Some((page, r.min + i as u16));
            }
            i -= len;
        }
        let r = self.list[self.n - 1];
        let page = if r.page != 0 { r.page } else { default_page };
        Some((page, r.max))
    }
}

/// Read `size` bits starting at bit `start` (little-endian bit order, as
/// HID uses). None when the bits run past the end of `data`.
pub fn read_bits(data: &[u8], start: u32, size: u32, signed: bool) -> Option<i32> {
    if size == 0 || size > 32 {
        return None;
    }
    let end = start.checked_add(size)?;
    if end.div_ceil(8) as usize > data.len() {
        return None;
    }
    let mut v: u64 = 0;
    let first = (start / 8) as usize;
    let last = ((end - 1) / 8) as usize;
    for (k, b) in data[first..=last].iter().enumerate() {
        v |= (*b as u64) << (8 * k);
    }
    v >>= start % 8;
    let mask = if size == 32 {
        u32::MAX as u64
    } else {
        (1u64 << size) - 1
    };
    v &= mask;
    if signed && size < 32 && v & (1 << (size - 1)) != 0 {
        v |= !mask;
    }
    Some(v as u32 as i32)
}

/// Item data as an unsigned number.
fn unsigned(data: &[u8]) -> u32 {
    let mut v = 0u32;
    for (k, b) in data.iter().enumerate().take(4) {
        v |= (*b as u32) << (8 * k);
    }
    v
}

/// Item data as a signed number (the top bit of the last byte is the sign).
fn signed(data: &[u8]) -> i32 {
    match data.len() {
        0 => 0,
        1 => data[0] as i8 as i32,
        2 => i16::from_le_bytes([data[0], data[1]]) as i32,
        _ => unsigned(data) as i32,
    }
}

impl Default for Descriptor {
    fn default() -> Self {
        Self::empty()
    }
}

impl Descriptor {
    pub const fn empty() -> Self {
        Descriptor {
            fields: [EMPTY_FIELD; MAX_FIELDS],
            n_fields: 0,
            reports: [EMPTY_REPORT; MAX_REPORTS],
            n_reports: 0,
            uses_report_ids: false,
            dropped: 0,
            damaged: false,
            collections: 0,
        }
    }

    /// All fields, in the order the descriptor declares them.
    pub fn fields(&self) -> &[Field] {
        &self.fields[..self.n_fields]
    }

    /// All reports, in the order first seen.
    pub fn reports(&self) -> &[Report] {
        &self.reports[..self.n_reports]
    }

    pub fn is_empty(&self) -> bool {
        self.n_fields == 0
    }

    /// The input report with this id, if declared.
    pub fn input_report(&self, id: u8) -> Option<&Report> {
        self.reports()
            .iter()
            .find(|r| r.kind == Kind::Input && r.id == id)
    }

    /// The input fields of one report.
    pub fn input_fields(&self, id: u8) -> impl Iterator<Item = &Field> {
        self.fields()
            .iter()
            .filter(move |f| f.kind == Kind::Input && f.report_id == id)
    }

    /// True if some input field has this usage.
    pub fn has_input_usage(&self, page: u16, usage: u16) -> bool {
        self.fields().iter().any(|f| {
            f.kind == Kind::Input
                && !f.is_constant()
                && f.usage_page == page
                && f.usage <= usage
                && usage <= f.usage_max
        })
    }

    /// Parse a report descriptor. Never panics; see the module text.
    pub fn parse(bytes: &[u8]) -> Self {
        let mut d = Descriptor::empty();
        d.parse_into(bytes);
        d
    }

    /// Parse into this table, replacing what it held. Same as
    /// [`Descriptor::parse`], but builds no second 4 KB table on the stack
    /// (which matters on a small microcontroller stack).
    pub fn parse_in_place(&mut self, bytes: &[u8]) {
        self.n_fields = 0;
        self.n_reports = 0;
        self.uses_report_ids = false;
        self.dropped = 0;
        self.damaged = false;
        self.collections = 0;
        self.parse_into(bytes);
    }

    fn parse_into(&mut self, bytes: &[u8]) {
        let mut g = GLOBALS0;
        let mut stack = [GLOBALS0; MAX_STACK];
        let mut depth = 0usize;
        let mut locals = Locals::new();
        let mut open: i32 = 0;
        let mut i = 0usize;
        while i < bytes.len() {
            let prefix = bytes[i];
            i += 1;
            if prefix == 0xFE {
                // A long item: next byte is the data size, then a tag byte.
                // No HID 1.11 long items are defined, so skip it.
                let Some(&size) = bytes.get(i) else {
                    self.damaged = true;
                    break;
                };
                i = i.saturating_add(2 + size as usize);
                if i > bytes.len() {
                    self.damaged = true;
                }
                continue;
            }
            let size = match prefix & 3 {
                3 => 4,
                s => s as usize,
            };
            if i + size > bytes.len() {
                self.damaged = true;
                break;
            }
            let data = &bytes[i..i + size];
            i += size;
            let tag = prefix >> 4;
            match (prefix >> 2) & 3 {
                // Main items.
                0 => {
                    match tag {
                        0x8 | 0x9 | 0xB => {
                            let kind = match tag {
                                0x8 => Kind::Input,
                                0x9 => Kind::Output,
                                _ => Kind::Feature,
                            };
                            locals.settle_pages(g.usage_page);
                            self.main_item(kind, &g, &locals, unsigned(data));
                        }
                        0xA => {
                            open += 1;
                            self.collections = self.collections.saturating_add(1);
                        }
                        0xC => {
                            open -= 1;
                            if open < 0 {
                                self.damaged = true;
                                open = 0;
                            }
                        }
                        _ => {}
                    }
                    locals.clear();
                }
                // Global items.
                1 => match tag {
                    0x0 => g.usage_page = unsigned(data) as u16,
                    0x1 => g.logical_min = signed(data),
                    0x2 => {
                        g.logical_max = signed(data);
                        g.logical_max_raw = unsigned(data);
                        g.logical_max_len = size as u8;
                    }
                    0x3 => g.physical_min = signed(data),
                    0x4 => g.physical_max = signed(data),
                    // 0x5 unit exponent and 0x6 unit: not needed here.
                    0x7 => g.report_size = unsigned(data),
                    0x8 => {
                        g.report_id = unsigned(data) as u8;
                        self.uses_report_ids = true;
                    }
                    0x9 => g.report_count = unsigned(data),
                    0xA => {
                        if depth < MAX_STACK {
                            stack[depth] = g;
                            depth += 1;
                        } else {
                            self.damaged = true;
                        }
                    }
                    0xB => {
                        if depth > 0 {
                            depth -= 1;
                            g = stack[depth];
                        } else {
                            self.damaged = true;
                        }
                    }
                    _ => {}
                },
                // Local items.
                2 => {
                    // A 4-byte usage carries its own page in the high half.
                    let short = size < 4;
                    let (page, usage) = if short {
                        (g.usage_page, unsigned(data) as u16)
                    } else {
                        let v = unsigned(data);
                        ((v >> 16) as u16, v as u16)
                    };
                    match tag {
                        0x0 => locals.push(UsageRange {
                            page,
                            min: usage,
                            max: usage,
                            short,
                        }),
                        0x1 => locals.pending_min = Some((page, usage, short)),
                        0x2 => {
                            let (p, min, sh) =
                                locals.pending_min.take().unwrap_or((page, usage, short));
                            let (min, max) = if usage >= min {
                                (min, usage)
                            } else {
                                (usage, min)
                            };
                            locals.push(UsageRange {
                                page: p,
                                min,
                                max,
                                short: sh,
                            });
                        }
                        _ => {}
                    }
                }
                _ => {} // Reserved type: ignore.
            }
        }
        if open != 0 {
            self.damaged = true;
        }
    }

    /// The report entry for (kind, id), created if new.
    fn report_mut(&mut self, kind: Kind, id: u8) -> Option<&mut Report> {
        let pos = self.reports[..self.n_reports]
            .iter()
            .position(|r| r.kind == kind && r.id == id);
        match pos {
            Some(p) => Some(&mut self.reports[p]),
            None if self.n_reports < MAX_REPORTS => {
                self.reports[self.n_reports] = Report { kind, id, bits: 0 };
                self.n_reports += 1;
                Some(&mut self.reports[self.n_reports - 1])
            }
            None => {
                self.dropped = self.dropped.saturating_add(1);
                None
            }
        }
    }

    fn push_field(&mut self, f: Field) {
        if self.n_fields < MAX_FIELDS {
            self.fields[self.n_fields] = f;
            self.n_fields += 1;
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    fn main_item(&mut self, kind: Kind, g: &Globals, locals: &Locals, data: u32) {
        let flags = (data & 0x1FF) as u16;
        let size = g.report_size;
        let count = g.report_count;
        let total_bits = size.saturating_mul(count);
        let Some(report) = self.report_mut(kind, g.report_id) else {
            return;
        };
        let start = report.bits;
        report.bits = report.bits.saturating_add(total_bits);
        if size == 0 || count == 0 {
            return;
        }
        // Positions past 64 Kibit or values wider than 255 bits are not
        // something a gamepad sends: keep the size count, skip the fields.
        if start > u16::MAX as u32 || size > 255 {
            self.damaged = true;
            return;
        }
        // A common descriptor mistake: Logical Minimum 0 with Logical
        // Maximum written as 0xFF in one byte (meaning 255, but read as -1
        // because item data is signed). Read it as unsigned in that case.
        let mut lmax = g.logical_max;
        if g.logical_min >= 0 && lmax < g.logical_min && g.logical_max_len > 0 {
            lmax = g.logical_max_raw as i32;
        }
        let mut f = Field {
            kind,
            report_id: g.report_id,
            bit_offset: start as u16,
            bit_size: size as u8,
            count: 1,
            flags,
            logical_min: g.logical_min,
            logical_max: lmax,
            physical_min: g.physical_min,
            physical_max: g.physical_max,
            usage_page: g.usage_page,
            usage: 0,
            usage_max: 0,
        };
        let constant = flags & flag::CONSTANT != 0;
        let variable = flags & flag::VARIABLE != 0;
        if constant || locals.n == 0 {
            // Padding, or data with no usage: one field for the whole item.
            f.count = count.min(u16::MAX as u32) as u16;
            self.push_field(f);
            return;
        }
        if !variable {
            // An array: `count` slots, each holding the index of a usage
            // that is "on" (for example the keys held on a keyboard).
            let (page, min) = locals.get(0, g.usage_page).unwrap_or((g.usage_page, 0));
            let last = locals.total().saturating_sub(1);
            let (_, max) = locals.get(last, g.usage_page).unwrap_or((page, min));
            f.usage_page = page;
            f.usage = min;
            f.usage_max = max;
            f.count = count.min(u16::MAX as u32) as u16;
            self.push_field(f);
            return;
        }
        // Variables: one value per usage. When there are more values than
        // usages, the last usage repeats; those extra values are kept as
        // one block field so that 48 vendor bytes take one table row.
        let distinct = locals.total().min(count);
        for k in 0..distinct {
            let (page, usage) = locals.get(k, g.usage_page).unwrap_or((g.usage_page, 0));
            let off = start as u64 + k as u64 * size as u64;
            if off > u16::MAX as u64 {
                self.damaged = true;
                return;
            }
            let mut one = f;
            one.bit_offset = off as u16;
            one.usage_page = page;
            one.usage = usage;
            one.usage_max = usage;
            self.push_field(one);
            if self.n_fields >= MAX_FIELDS {
                // Stop early: the rest would only be dropped one by one.
                self.dropped = self
                    .dropped
                    .saturating_add((distinct - k - 1).min(u16::MAX as u32) as u16);
                return;
            }
        }
        if count > distinct {
            let (page, usage) = locals
                .get(distinct.saturating_sub(1), g.usage_page)
                .unwrap_or((g.usage_page, 0));
            let off = start as u64 + distinct as u64 * size as u64;
            if off > u16::MAX as u64 {
                self.damaged = true;
                return;
            }
            let mut rest = f;
            rest.bit_offset = off as u16;
            rest.usage_page = page;
            rest.usage = usage;
            rest.usage_max = usage;
            rest.count = (count - distinct).min(u16::MAX as u32) as u16;
            self.push_field(rest);
        }
    }

    /// Counts for the About page: (buttons, axes, hats) declared as input.
    pub fn summary(&self) -> (u16, u16, u16) {
        let (mut buttons, mut axes, mut hats) = (0u16, 0u16, 0u16);
        for f in self.fields() {
            if f.kind != Kind::Input || f.is_constant() {
                continue;
            }
            match (f.usage_page, f.usage) {
                (0x09, _) => {
                    let n = if f.is_variable() {
                        f.count
                    } else {
                        f.usage_max.saturating_sub(f.usage).saturating_add(1)
                    };
                    buttons = buttons.saturating_add(n);
                }
                (0x01, 0x39) => hats = hats.saturating_add(1),
                (0x01, 0x30..=0x38) | (0x02, 0xBA..=0xC8) => axes = axes.saturating_add(f.count),
                _ => {}
            }
        }
        (buttons, axes, hats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits() {
        let d = [0b1010_1100, 0xFF, 0x01];
        assert_eq!(read_bits(&d, 2, 3, false), Some(0b011));
        assert_eq!(read_bits(&d, 4, 4, false), Some(0b1010));
        assert_eq!(read_bits(&d, 4, 4, true), Some(-6));
        assert_eq!(read_bits(&d, 8, 16, false), Some(0x01FF));
        assert_eq!(read_bits(&d, 20, 8, false), None);
        assert_eq!(read_bits(&[0xFF; 5], 3, 32, false), Some(-1));
    }
}
