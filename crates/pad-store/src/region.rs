//! Which piece of flash the store may use, checked before the first
//! write.
//!
//! The firmware finds the "nvs" data partition in the partition table
//! and hands every entry of the table to [`check_region`]. Only if every
//! check passes does it get a [`Region`], and only a `Region` can be
//! turned into a store. So the bootloader, the partition table and the
//! program can never be written to by mistake.

/// One flash sector: the smallest piece that can be erased.
pub const SECTOR: u32 = 4096;

/// Below this address live the bootloader (from 0x1000) and the
/// partition table (0x8000 to 0x8C00, rounded up to a whole sector).
pub const FIRST_FREE: u32 = 0x9000;

/// How much of the partition the store uses: its first two sectors.
pub const USED: u32 = 2 * SECTOR;

/// A piece of flash: where it starts and how long it is, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub offset: u32,
    pub len: u32,
}

impl Span {
    /// The first address after the span, or None if it would not fit in
    /// 32 bits (a broken table entry).
    pub fn end(&self) -> Option<u32> {
        self.offset.checked_add(self.len)
    }

    fn overlaps(&self, other: &Span) -> bool {
        match (self.end(), other.end()) {
            (Some(a_end), Some(b_end)) => self.offset < b_end && other.offset < a_end,
            // A span past the end of 32 bits: treat it as touching all.
            _ => true,
        }
    }
}

/// Why a partition was not taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionError {
    /// The partition does not start on a sector boundary.
    NotAligned,
    /// Smaller than the two sectors the store needs.
    TooSmall,
    /// Starts below 0x9000, where the bootloader and the table live.
    BelowTable,
    /// The part the store would use overlaps another partition.
    Overlaps,
    /// The part the store would use goes past the end of the flash chip.
    PastEnd,
}

impl RegionError {
    /// A short word for the log and the screen.
    pub fn word(self) -> &'static str {
        match self {
            RegionError::NotAligned => "not-sector-aligned",
            RegionError::TooSmall => "too-small",
            RegionError::BelowTable => "below-partition-table",
            RegionError::Overlaps => "overlaps-another-partition",
            RegionError::PastEnd => "past-end-of-flash",
        }
    }
}

/// A checked piece of flash: the first [`USED`] bytes of a partition.
/// It can only be made by [`check_region`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    base: u32,
}

impl Region {
    /// The first address the store may touch.
    pub fn base(&self) -> u32 {
        self.base
    }

    /// How many bytes the store may touch, from `base`.
    pub fn len(&self) -> u32 {
        USED
    }

    /// Never empty; here because clippy asks for it next to `len`.
    pub fn is_empty(&self) -> bool {
        false
    }
}

/// Checks the partition `part` against the other entries of the
/// partition table (`others`, without `part` itself) and the size of the
/// flash chip.
pub fn check_region(part: Span, others: &[Span], capacity: u32) -> Result<Region, RegionError> {
    if !part.offset.is_multiple_of(SECTOR) {
        return Err(RegionError::NotAligned);
    }
    if part.len < USED {
        return Err(RegionError::TooSmall);
    }
    if part.offset < FIRST_FREE {
        return Err(RegionError::BelowTable);
    }
    let used = Span {
        offset: part.offset,
        len: USED,
    };
    match used.end() {
        Some(end) if end <= capacity => {}
        _ => return Err(RegionError::PastEnd),
    }
    if others.iter().any(|o| o.len > 0 && used.overlaps(o)) {
        return Err(RegionError::Overlaps);
    }
    Ok(Region { base: part.offset })
}

/// One entry of the partition table, as the firmware reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// 0 = program, 1 = data.
    pub kind: u8,
    /// For data: 2 = NVS.
    pub subtype: u8,
    /// The name, as the 16 raw bytes of the table. It ends at the first
    /// 0x00 or 0xFF byte (as ESP-IDF writes it).
    pub label: [u8; 16],
    pub offset: u32,
    pub len: u32,
    pub read_only: bool,
    pub encrypted: bool,
}

/// Most table entries looked at. A table with more is not guessed at.
pub const MAX_ENTRIES: usize = 16;

/// Why no piece of flash was picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindError {
    /// No data partition of sub-type NVS named "nvs".
    NoPartition,
    /// The "nvs" partition is marked read-only.
    ReadOnly,
    /// The "nvs" partition is marked encrypted.
    Encrypted,
    /// More than [`MAX_ENTRIES`] entries: not all could be checked.
    TooManyEntries,
    /// Found, but [`check_region`] said no.
    Region(RegionError),
}

/// Picks the store's piece of flash from the partition table: the first
/// data partition of sub-type NVS (0x01, 0x02) whose name is exactly
/// "nvs". Every other entry (the program too) goes to [`check_region`],
/// so the two sectors used never overlap any of them.
pub fn find_nvs(
    entries: impl IntoIterator<Item = Entry>,
    capacity: u32,
) -> Result<Region, FindError> {
    let mut nvs = None;
    let mut others = [Span { offset: 0, len: 0 }; MAX_ENTRIES];
    let mut n = 0;
    for e in entries {
        let span = Span {
            offset: e.offset,
            len: e.len,
        };
        if nvs.is_none() && e.kind == 0x01 && e.subtype == 0x02 && name(&e.label) == b"nvs" {
            if e.read_only {
                return Err(FindError::ReadOnly);
            }
            if e.encrypted {
                return Err(FindError::Encrypted);
            }
            nvs = Some(span);
        } else if n < MAX_ENTRIES {
            others[n] = span;
            n += 1;
        } else {
            return Err(FindError::TooManyEntries);
        }
    }
    let nvs = nvs.ok_or(FindError::NoPartition)?;
    check_region(nvs, &others[..n], capacity).map_err(FindError::Region)
}

/// A table name up to its first 0x00 or 0xFF byte.
fn name(label: &[u8; 16]) -> &[u8] {
    let end = label
        .iter()
        .position(|&b| b == 0 || b == 0xFF)
        .unwrap_or(label.len());
    &label[..end]
}

/// A 16-byte table name from text (cut at 16 bytes, padded with zeros).
pub fn label(text: &str) -> [u8; 16] {
    let mut l = [0u8; 16];
    for (d, s) in l.iter_mut().zip(text.bytes()) {
        *d = s;
    }
    l
}
