//! Where on the chip the settings live, and a [`Store`] that cannot
//! touch anything else.
//!
//! The board reads the partition table and hands every entry to
//! [`find_area`]. Only when every check passes does it get an [`Area`]:
//! a data partition named "settings", exactly [`AREA`] bytes, at the
//! address the board expects, writable, not encrypted, above the
//! partition table, inside the chip and clear of every other partition
//! (the saved gamepad pairing in "nvs" and the program included).
//!
//! Only an `Area` makes an [`AreaStore`], and the `AreaStore` checks
//! every read, write and erase against it before the flash is called:
//! offsets count from the start of the area, and anything past its end,
//! or not on a 4-byte boundary, is refused with
//! [`StoreError::OutOfRange`] and nothing reaches the flash.

use board_api::link::SECTOR;
use board_api::{Store, StoreError};

use crate::AREA;

/// The partition's name in the table.
pub const LABEL: &[u8] = b"settings";

/// Data partitions have type 1.
pub const DATA: u8 = 0x01;

/// The data sub-type the settings partition uses: "undefined" (0x06),
/// the one ESP-IDF keeps for the user. (espflash 4.7 refuses numbers
/// it has no name for.)
pub const SUBTYPE: u8 = 0x06;

/// Below this address live the bootloader (from 0x1000) and the
/// partition table (0x8000 to 0x8C00, rounded up to a whole sector).
pub const FIRST_FREE: u32 = 0x9000;

/// Most table entries looked at. A table with more is not guessed at.
pub const MAX_ENTRIES: usize = 16;

/// Where the partition table starts on the chip (ESP-IDF's default).
pub const TABLE: u32 = 0x8000;
/// One table entry, bytes.
pub const ENTRY_LEN: usize = 32;
/// The first two bytes of every entry (little-endian 0x50AA). The table
/// ends at the first line without it: its MD5 line (0xEBEB), or blank
/// flash (0xFFFF).
pub const ENTRY_MAGIC: [u8; 2] = [0xAA, 0x50];

impl Entry {
    /// One raw 32-byte line of the table, as ESP-IDF writes it: magic,
    /// type, sub-type, offset, size (little-endian), the 16-byte name,
    /// flags (bit 0 encrypted, bit 1 read-only, as ESP-IDF and espflash's
    /// esp-idf-part 0.6 write them; esp-bootloader-esp-idf 0.6.0 reads
    /// the two the other way round). `None`: not an entry, the table
    /// ends here.
    ///
    /// The board reads the table one line at a time with this, so no
    /// 3 KB copy of the whole table is needed. (The bootloader checked
    /// the table's MD5 line before it started the program.)
    pub fn parse(raw: &[u8; ENTRY_LEN]) -> Option<Entry> {
        if raw[..2] != ENTRY_MAGIC {
            return None;
        }
        let word = |at: usize| u32::from_le_bytes([raw[at], raw[at + 1], raw[at + 2], raw[at + 3]]);
        let mut label = [0u8; 16];
        label.copy_from_slice(&raw[12..28]);
        let flags = word(28);
        Some(Entry {
            kind: raw[2],
            subtype: raw[3],
            label,
            offset: word(4),
            len: word(8),
            read_only: flags & 0b10 != 0,
            encrypted: flags & 0b01 != 0,
        })
    }
}

/// One entry of the partition table, as the firmware reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// 0 = program, 1 = data.
    pub kind: u8,
    pub subtype: u8,
    /// The name, as the 16 raw bytes of the table. It ends at the first
    /// 0x00 or 0xFF byte (as ESP-IDF writes it).
    pub label: [u8; 16],
    pub offset: u32,
    pub len: u32,
    pub read_only: bool,
    pub encrypted: bool,
}

/// Why the settings area was not taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindError {
    /// No data partition of sub-type "undefined" named "settings" (an
    /// older table, flashed without `boards/cyd/partitions.csv`).
    NoPartition,
    /// It is not where the board expects it.
    Moved,
    /// It is not exactly [`AREA`] bytes.
    WrongSize,
    /// Marked read-only.
    ReadOnly,
    /// Marked encrypted.
    Encrypted,
    /// Starts below 0x9000, where the bootloader and the table live.
    BelowTable,
    /// Does not start on a sector.
    NotAligned,
    /// Goes past the end of the flash chip.
    PastEnd,
    /// Overlaps another partition.
    Overlaps,
    /// More than [`MAX_ENTRIES`] entries: not all could be checked.
    TooManyEntries,
}

impl FindError {
    /// A short word for the log.
    pub fn word(self) -> &'static str {
        match self {
            FindError::NoPartition => "no-settings-partition",
            FindError::Moved => "not-at-the-expected-address",
            FindError::WrongSize => "wrong-size",
            FindError::ReadOnly => "read-only",
            FindError::Encrypted => "encrypted",
            FindError::BelowTable => "below-partition-table",
            FindError::NotAligned => "not-sector-aligned",
            FindError::PastEnd => "past-end-of-flash",
            FindError::Overlaps => "overlaps-another-partition",
            FindError::TooManyEntries => "too-many-entries",
        }
    }
}

/// A checked piece of flash: [`AREA`] bytes from `base`. Only
/// [`find_area`] makes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    base: u32,
}

impl Area {
    /// The first chip address the store may touch.
    pub fn base(&self) -> u32 {
        self.base
    }

    /// The first chip address after the area.
    pub fn end(&self) -> u32 {
        self.base + AREA
    }
}

/// Finds the settings partition in the table (`entries`) and checks it
/// against every other entry, the flash size (`capacity`) and the
/// address the board expects (`expected`).
pub fn find_area(
    entries: impl IntoIterator<Item = Entry>,
    capacity: u32,
    expected: u32,
) -> Result<Area, FindError> {
    let mut found: Option<Entry> = None;
    let mut others = [(0u32, 0u32); MAX_ENTRIES];
    let mut n = 0;
    for e in entries {
        if found.is_none() && e.kind == DATA && e.subtype == SUBTYPE && name(&e.label) == LABEL {
            found = Some(e);
        } else if n < MAX_ENTRIES {
            others[n] = (e.offset, e.len);
            n += 1;
        } else {
            return Err(FindError::TooManyEntries);
        }
    }
    let e = found.ok_or(FindError::NoPartition)?;
    if e.read_only {
        return Err(FindError::ReadOnly);
    }
    if e.encrypted {
        return Err(FindError::Encrypted);
    }
    if e.len != AREA {
        return Err(FindError::WrongSize);
    }
    if !e.offset.is_multiple_of(SECTOR) {
        return Err(FindError::NotAligned);
    }
    if e.offset < FIRST_FREE {
        return Err(FindError::BelowTable);
    }
    let end = match e.offset.checked_add(AREA) {
        Some(end) if end <= capacity => end,
        _ => return Err(FindError::PastEnd),
    };
    for &(o, l) in &others[..n] {
        // An entry past the end of 32 bits touches everything.
        let o_end = o.saturating_add(l);
        if l > 0 && e.offset < o_end && o < end {
            return Err(FindError::Overlaps);
        }
    }
    if e.offset != expected {
        return Err(FindError::Moved);
    }
    Ok(Area { base: e.offset })
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

/// The flash chip, by chip address. The board's version calls the
/// chip's flash functions; a write never erases first.
pub trait Flash {
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), StoreError>;
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), StoreError>;
    /// Erases the 4 KB sector that starts at `addr`.
    fn erase_sector(&mut self, addr: u32) -> Result<(), StoreError>;
}

/// The settings area as a [`Store`] of [`AREA`] bytes. Every call is
/// checked against the area before the flash is called.
pub struct AreaStore<F> {
    flash: F,
    area: Area,
}

impl<F: Flash> AreaStore<F> {
    pub fn new(flash: F, area: Area) -> Self {
        AreaStore { flash, area }
    }

    pub fn area(&self) -> Area {
        self.area
    }

    /// The flash, to read its counters.
    pub fn flash(&self) -> &F {
        &self.flash
    }

    /// The chip address of `len` bytes at `offset` in the area, if they
    /// are inside it and on 4-byte boundaries.
    pub fn span(&self, offset: u32, len: usize) -> Result<u32, StoreError> {
        let len = u32::try_from(len).map_err(|_| StoreError::OutOfRange)?;
        if !offset.is_multiple_of(4) || !len.is_multiple_of(4) {
            return Err(StoreError::OutOfRange);
        }
        match offset.checked_add(len) {
            Some(end) if end <= AREA => Ok(self.area.base + offset),
            _ => Err(StoreError::OutOfRange),
        }
    }
}

impl<F: Flash> Store for AreaStore<F> {
    fn size(&self) -> u32 {
        AREA
    }

    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), StoreError> {
        let at = self.span(offset, buf.len())?;
        self.flash.read(at, buf)
    }

    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), StoreError> {
        let at = self.span(offset, data.len())?;
        self.flash.write(at, data)
    }

    fn erase_sector(&mut self, offset: u32) -> Result<(), StoreError> {
        if !offset.is_multiple_of(SECTOR) {
            return Err(StoreError::OutOfRange);
        }
        let at = self.span(offset, SECTOR as usize)?;
        self.flash.erase_sector(at)
    }
}
