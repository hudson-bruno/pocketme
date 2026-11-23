//! The console's settings in flash: the brightness step and the boot
//! animation now, with room for more small values later.
//!
//! Two sectors of a [`Store`] (8 KB) hold small records, 16 bytes each,
//! written one after the other. A change adds one record; nothing is
//! erased until a sector is full. Then the other sector is erased and
//! used. The newest record (the highest sequence number with a good
//! check sum) is the value.
//!
//! Safe against a power cut at any moment:
//!
//! - A record is written into a blank slot only, never over one; a cut
//!   leaves a record with a bad check sum, which is skipped. The record
//!   before it is still there: the old value.
//! - Only the sector that does NOT hold the newest record is ever
//!   erased. A cut during that erase leaves the newest record as it was.
//! - The next save writes after the last slot that is not blank, so a
//!   damaged slot is never used again, and never makes the store stuck.
//!
//! Record (16 bytes, little-endian):
//!
//! ```text
//! 0..4    sequence number, 1, 2, 3, ... (counts over both sectors)
//! 4       kind: 1 = settings. Other kinds (from a newer firmware) are
//!         skipped, but their sequence numbers count.
//! 5..12   seven one-byte values; 0xFF = not set (the default).
//!         5: brightness step. 6: boot animation (0 full, 1 short,
//!         2 off; 0xFF or another value reads as full). 7..12: not used
//!         yet, kept as they are.
//! 12..16  CRC-32 of bytes 0..12, xor [`SALT`]
//! ```
//!
//! A rule for later firmware: the first record written into a freshly
//! erased sector is a kind-1 record (this crate does that), so the
//! newest settings are never only in the sector that gets erased next.
//!
//! No std, no heap, no unsafe. It only calls the [`Store`] trait.

#![no_std]
#![forbid(unsafe_code)]

use board_api::link::SECTOR;
use board_api::{Store, StoreError};

pub mod area;
pub mod policy;

/// Bytes the settings use in the store: two sectors.
pub const AREA: u32 = 2 * SECTOR;
/// One record.
pub const RECORD: usize = 16;
/// Records in one sector.
pub const SLOTS: u16 = (SECTOR as usize / RECORD) as u16;
/// The kind of a settings record.
pub const KIND_SETTINGS: u8 = 1;
/// Mixed into every check sum, so other data that happens to carry a
/// good CRC-32 is not taken for a record.
pub const SALT: u32 = 0x5E77_1A65;
/// The brightest step, and the default (as the console's
/// `theme::BRIGHT_STEPS - 1`). Larger stored values read as this.
pub const BRIGHTNESS_MAX: u8 = 4;
/// The boot animation's values ([`Settings::boot_anim`]), numbered as
/// console-core's `boot::Anim` (feature `boot-gather`) numbers them;
/// kept in step by hand. Full is also what a blank or unknown value
/// reads as.
pub const BOOT_ANIM_FULL: u8 = 0;
pub const BOOT_ANIM_SHORT: u8 = 1;
pub const BOOT_ANIM_OFF: u8 = 2;
/// The highest boot animation value; larger stored values read as
/// [`BOOT_ANIM_FULL`].
pub const BOOT_ANIM_MAX: u8 = BOOT_ANIM_OFF;
/// Value bytes in a record.
pub const VALUES: usize = 7;
/// Tries of one save: a slot that does not read back as written (a worn
/// cell) is left, and the next slot tried.
pub const TRIES: usize = 3;
/// How long the settings should be left alone before they are saved
/// (see [`SaveTimer`]): a finger sliding on the brightness bars makes
/// one record, not ten.
pub const QUIET_US: u64 = 2_000_000;

/// Bytes read from the store at once while scanning (8 records).
const CHUNK: usize = 128;

/// The settings the console keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Screen brightness step, 0 (dimmest) to [`BRIGHTNESS_MAX`].
    pub brightness: u8,
    /// Value bytes this firmware does not use (a newer one may): kept
    /// as they were read, and written back with each new record.
    others: [u8; VALUES - 1],
}

impl Settings {
    /// What a console with nothing stored uses.
    pub const DEFAULT: Settings = Settings {
        brightness: BRIGHTNESS_MAX,
        others: [0xFF; VALUES - 1],
    };

    /// These settings with another brightness step (above the brightest
    /// counts as the brightest).
    pub const fn with_brightness(mut self, step: u8) -> Settings {
        self.brightness = if step > BRIGHTNESS_MAX {
            BRIGHTNESS_MAX
        } else {
            step
        };
        self
    }

    /// The boot animation: [`BOOT_ANIM_FULL`], [`BOOT_ANIM_SHORT`] or
    /// [`BOOT_ANIM_OFF`]. Nothing stored (a record from before this
    /// value, or none) or a value this firmware does not know reads as
    /// full.
    pub const fn boot_anim(&self) -> u8 {
        match self.others[0] {
            v if v <= BOOT_ANIM_MAX => v,
            _ => BOOT_ANIM_FULL,
        }
    }

    /// These settings with another boot animation (above
    /// [`BOOT_ANIM_MAX`] counts as full). The same value as now changes
    /// nothing, so a blank value stays blank (and a save writes no new
    /// record for it).
    pub const fn with_boot_anim(mut self, anim: u8) -> Settings {
        let anim = if anim > BOOT_ANIM_MAX {
            BOOT_ANIM_FULL
        } else {
            anim
        };
        if anim != self.boot_anim() {
            self.others[0] = anim;
        }
        self
    }

    fn from_values(v: &[u8; VALUES]) -> Settings {
        let mut others = [0xFF; VALUES - 1];
        others.copy_from_slice(&v[1..]);
        Settings {
            brightness: match v[0] {
                0xFF => Settings::DEFAULT.brightness,
                b => b.min(BRIGHTNESS_MAX),
            },
            others,
        }
    }

    fn values(&self) -> [u8; VALUES] {
        let mut v = [0xFF; VALUES];
        v[0] = self.brightness.min(BRIGHTNESS_MAX);
        v[1..].copy_from_slice(&self.others);
        v
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings::DEFAULT
    }
}

/// What went wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The store said no (out of range, or a flash error).
    Store(StoreError),
    /// The area does not fit the store, or does not start on a sector.
    Area,
    /// [`TRIES`] slots in a row did not read back as written.
    Verify,
}

impl From<StoreError> for Error {
    fn from(e: StoreError) -> Self {
        Error::Store(e)
    }
}

/// What a save did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Saved {
    /// The store already held these settings: nothing written.
    Unchanged,
    /// One record written. `erased`: a sector was erased first.
    Written { erased: bool },
}

/// The settings in two sectors of a [`Store`], from `base`. About 24
/// bytes. Call [`SettingsStore::load`] once at start, then
/// [`SettingsStore::save`] when they changed (best after
/// [`QUIET_US`] of quiet, see [`SaveTimer`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingsStore {
    /// Where the two sectors start in the store (a multiple of
    /// `SECTOR`).
    base: u32,
    /// What the store holds now: the newest settings record, or the
    /// defaults when there is none.
    current: Settings,
    /// A good settings record was found (or written).
    found: bool,
    /// The sector the newest record is in (0 or 1), the next slot to
    /// write there (after the last slot that is not blank; [`SLOTS`]
    /// when full), and the next sequence number.
    active: u8,
    next_slot: u16,
    next_seq: u32,
    /// The fields above match the flash. False before the first scan
    /// and after an error: the next save scans again.
    ready: bool,
}

impl SettingsStore {
    /// The settings area at `base` in the store. Nothing is read yet.
    pub const fn new(base: u32) -> Self {
        SettingsStore {
            base,
            current: Settings::DEFAULT,
            found: false,
            active: 0,
            next_slot: 0,
            next_seq: 1,
            ready: false,
        }
    }

    /// Reads the settings. Never fails: with nothing stored, a blank or
    /// damaged area, or a store that cannot be read, it gives the
    /// defaults (see [`SettingsStore::found`]).
    pub fn load<S: Store>(&mut self, store: &mut S) -> Settings {
        if self.scan(store).is_err() {
            self.current = Settings::DEFAULT;
            self.found = false;
            self.ready = false;
        }
        self.current
    }

    /// Whether the last load (or save) found good settings in the store.
    pub fn found(&self) -> bool {
        self.found
    }

    /// What the store holds now (the defaults when nothing).
    pub fn current(&self) -> Settings {
        self.current
    }

    /// Records written in the sector in use, of [`SLOTS`] (for a log).
    pub fn used(&self) -> u16 {
        self.next_slot
    }

    /// Saves `new` if it differs from what the store holds. One record
    /// of 16 bytes; when the sector in use is full, the other one is
    /// erased first (4 KB, about 45 ms on the CYD's flash, up to 0.4 s).
    pub fn save<S: Store>(&mut self, store: &mut S, new: &Settings) -> Result<Saved, Error> {
        if !self.ready {
            self.scan(store)?;
        }
        // (With nothing stored, `current` is the defaults: saving the
        // defaults writes nothing either.)
        if *new == self.current {
            return Ok(Saved::Unchanged);
        }
        let record = encode(self.next_seq, KIND_SETTINGS, &new.values());
        let result = self.append(store, &record);
        match result {
            Ok(erased) => {
                self.current = *new;
                self.found = true;
                self.next_seq = self.next_seq.saturating_add(1);
                Ok(Saved::Written { erased })
            }
            Err(e) => {
                // A store error: read the flash again before the next
                // save. Slots that did not read back (`Verify`) are left
                // behind: the next save goes on after them (a worn cell
                // that stays blank would otherwise be tried for ever).
                if e != Error::Verify {
                    self.ready = false;
                }
                Err(e)
            }
        }
    }

    /// Erases the spare sector now if the one in use is at least half
    /// full and the spare is not blank, so that no save in the next
    /// [`SLOTS`]` / 2` needs an erase. For start-up, before the radio
    /// runs. Returns whether it erased.
    pub fn prepare<S: Store>(&mut self, store: &mut S) -> Result<bool, Error> {
        if !self.ready {
            self.scan(store)?;
        }
        if self.next_slot < SLOTS / 2 {
            return Ok(false);
        }
        let spare = self.sector_at(1 - self.active);
        if self.blank(store, spare)? {
            return Ok(false);
        }
        store
            .erase_sector(spare)
            .inspect_err(|_| self.ready = false)?;
        Ok(true)
    }

    /// Where sector `i` (0 or 1) of the area starts in the store.
    fn sector_at(&self, i: u8) -> u32 {
        self.base + u32::from(i) * SECTOR
    }

    /// Writes one record into the next blank slot, switching sectors
    /// when full. Returns whether it erased.
    fn append<S: Store>(&mut self, store: &mut S, record: &[u8; RECORD]) -> Result<bool, Error> {
        let mut erased = false;
        for _ in 0..TRIES {
            if self.next_slot >= SLOTS {
                // Full: the other sector holds only older records.
                let other = 1 - self.active;
                let at = self.sector_at(other);
                if !self.blank(store, at)? {
                    store.erase_sector(at)?;
                    erased = true;
                }
                self.active = other;
                self.next_slot = 0;
            }
            let at = self.sector_at(self.active) + u32::from(self.next_slot) * RECORD as u32;
            self.next_slot += 1;
            store.write(at, record)?;
            let mut back = Aligned([0; RECORD]);
            store.read(at, &mut back.0)?;
            if back.0 == *record {
                return Ok(erased);
            }
        }
        Err(Error::Verify)
    }

    /// Whether the sector at `at` is all 0xFF.
    fn blank<S: Store>(&self, store: &mut S, at: u32) -> Result<bool, Error> {
        let mut buf = Aligned([0; CHUNK]);
        for k in 0..SECTOR / CHUNK as u32 {
            store.read(at + k * CHUNK as u32, &mut buf.0)?;
            if buf.0.iter().any(|&b| b != 0xFF) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Reads both sectors: the newest settings, the newest record of
    /// any kind (it decides the sector in use and the next number), and
    /// the last slot that is not blank in each sector.
    fn scan<S: Store>(&mut self, store: &mut S) -> Result<(), Error> {
        if !self.base.is_multiple_of(SECTOR)
            || self
                .base
                .checked_add(AREA)
                .is_none_or(|end| end > store.size())
        {
            return Err(Error::Area);
        }
        let mut settings: Option<(u32, Settings)> = None;
        let mut newest: Option<(u32, u8)> = None;
        let mut used = [0u16; 2];
        let mut buf = Aligned([0; CHUNK]);
        for sector in 0..2u8 {
            let start = self.sector_at(sector);
            for k in 0..SECTOR / CHUNK as u32 {
                store.read(start + k * CHUNK as u32, &mut buf.0)?;
                for (j, rec) in buf.0.as_chunks::<RECORD>().0.iter().enumerate() {
                    let slot = (k as usize * (CHUNK / RECORD) + j) as u16;
                    if rec.iter().all(|&b| b == 0xFF) {
                        continue;
                    }
                    used[sector as usize] = slot + 1;
                    let Some((seq, kind, values)) = decode(rec) else {
                        continue;
                    };
                    if newest.is_none_or(|(s, _)| seq >= s) {
                        newest = Some((seq, sector));
                    }
                    if kind == KIND_SETTINGS && settings.is_none_or(|(s, _)| seq >= s) {
                        settings = Some((seq, Settings::from_values(&values)));
                    }
                }
            }
        }
        let (seq, active) = newest.unwrap_or((0, 0));
        self.active = active;
        self.next_slot = used[active as usize];
        self.next_seq = seq.saturating_add(1);
        self.found = settings.is_some();
        self.current = settings.map_or(Settings::DEFAULT, |(_, s)| s);
        self.ready = true;
        Ok(())
    }
}

/// A buffer the flash can read into and write from directly (the
/// ESP32's flash calls want 4-byte aligned buffers).
#[repr(align(4))]
struct Aligned<const N: usize>([u8; N]);

/// One record, as written.
fn encode(seq: u32, kind: u8, values: &[u8; VALUES]) -> [u8; RECORD] {
    let mut r = [0xFF; RECORD];
    r[0..4].copy_from_slice(&seq.to_le_bytes());
    r[4] = kind;
    r[5..12].copy_from_slice(values);
    let sum = crc32(&r[..12]) ^ SALT;
    r[12..16].copy_from_slice(&sum.to_le_bytes());
    r
}

/// A record's sequence number, kind and values, if its check sum is
/// good (and its number is not 0 or 0xFFFF_FFFF, which no writer uses).
fn decode(r: &[u8]) -> Option<(u32, u8, [u8; VALUES])> {
    let word = |i: usize| u32::from_le_bytes([r[i], r[i + 1], r[i + 2], r[i + 3]]);
    if r.len() != RECORD || word(12) != crc32(&r[..12]) ^ SALT {
        return None;
    }
    let seq = word(0);
    if seq == 0 || seq == u32::MAX {
        return None;
    }
    let mut values = [0; VALUES];
    values.copy_from_slice(&r[5..12]);
    Some((seq, r[4], values))
}

/// CRC-32, the common kind (zip, Ethernet), four bits at a time with a
/// 64-byte table: small, and quick enough for 512 records at start.
pub fn crc32(data: &[u8]) -> u32 {
    crc32_more(0, data)
}

/// CRC-32 of a piece read in parts: `crc32_more(crc32(a), b)` is the
/// CRC-32 of `a` then `b` (start with 0).
pub fn crc32_more(crc: u32, data: &[u8]) -> u32 {
    const T: [u32; 16] = {
        let mut t = [0u32; 16];
        let mut i = 0;
        while i < 16 {
            let mut c = i as u32;
            let mut k = 0;
            while k < 4 {
                c = if c & 1 != 0 {
                    (c >> 1) ^ 0xEDB8_8320
                } else {
                    c >> 1
                };
                k += 1;
            }
            t[i] = c;
            i += 1;
        }
        t
    };
    let mut c = !crc;
    for &b in data {
        c ^= u32::from(b);
        c = (c >> 4) ^ T[(c & 15) as usize];
        c = (c >> 4) ^ T[(c & 15) as usize];
    }
    !c
}

/// When to save: [`QUIET_US`] after the last change, so a burst of
/// changes makes one record. 16 bytes. The board calls
/// [`SaveTimer::changed`] when the console says the settings changed,
/// and saves when [`SaveTimer::due`] says so.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaveTimer {
    at: Option<u64>,
}

impl SaveTimer {
    pub const fn new() -> Self {
        SaveTimer { at: None }
    }

    /// The settings changed at `now_us`: save [`QUIET_US`] later (a
    /// later change moves the time on).
    pub fn changed(&mut self, now_us: u64) {
        self.at = Some(now_us.saturating_add(QUIET_US));
    }

    /// When the save is due, if one waits. The board wakes by then.
    pub fn deadline(&self) -> Option<u64> {
        self.at
    }

    /// True once, when the save is due at `now_us`; the board saves
    /// then. (If that save fails, call [`SaveTimer::changed`] again to
    /// try later.)
    pub fn due(&mut self, now_us: u64) -> bool {
        if self.at.is_some_and(|t| now_us >= t) {
            self.at = None;
            return true;
        }
        false
    }

    /// Whether a save waits (for a board that saves before power off).
    pub fn pending(&self) -> bool {
        self.at.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_known_value() {
        // The standard check value for this CRC.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
        // In parts, at every split.
        let d = b"123456789";
        for i in 0..=d.len() {
            assert_eq!(crc32_more(crc32(&d[..i]), &d[i..]), 0xCBF4_3926, "{i}");
        }
    }

    #[test]
    fn a_record_reads_back() {
        let v = [3, 0xFF, 7, 0xFF, 0xFF, 0xFF, 1];
        let r = encode(42, KIND_SETTINGS, &v);
        assert_eq!(decode(&r), Some((42, KIND_SETTINGS, v)));
        // One bit off anywhere: not a record.
        for i in 0..RECORD * 8 {
            let mut bad = r;
            bad[i / 8] ^= 1 << (i % 8);
            assert_eq!(decode(&bad), None, "bit {i}");
        }
        assert_eq!(decode(&[0xFF; RECORD]), None);
        assert_eq!(decode(&[0; RECORD]), None);
    }

    #[test]
    fn sizes() {
        assert_eq!(SLOTS, 256);
        assert!(size_of::<SettingsStore>() <= 24);
        assert_eq!(size_of::<SaveTimer>(), 16);
    }
}
