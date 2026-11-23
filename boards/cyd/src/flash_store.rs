//! The console's settings in the chip's flash (feature `store`).
//!
//! Where: the "settings" partition of `partitions.csv`, the last 8 KB of
//! the 4 MB chip (0x3FE000..0x400000), far from the saved gamepad
//! pairing (the first 8 KB of "nvs", 0x9000..0xB000) and from the
//! program. The partition is looked up in the table at start and checked
//! by `settings_store::area::find_area` (tested on the PC): only then is
//! there a store at all. A board flashed with the old table has none, and
//! keeps the brightness for the session only.
//!
//! Every read, write and erase goes through `AreaStore`, which refuses
//! anything outside the area, and then through [`EspFlash`], which checks
//! the chip address against [`BASE`] once more before the flash is
//! called. Nothing else in this firmware writes flash.
//!
//! The pairing area is only ever read, at start: its CRC-32 goes to the
//! log (never its bytes), so a log shows it never changed.
//!
//! A flash write or erase runs with every interrupt off, and the loop
//! stops for it; [`EspFlash`] times each one (`boardctl stats`, the
//! "settings" line). When saves happen: `settings_store::policy`.

use board_api::{Store, StoreError};
use esp_hal::time::Instant;
use esp_storage::FlashStorage;
use log::{info, warn};
use settings_store::area::{AreaStore, ENTRY_LEN, Entry, Flash, TABLE, find_area};
use settings_store::policy::{Moment, SavePolicy};
use settings_store::{AREA, Saved, Settings, SettingsStore};

use crate::pairing::PAIRING;
use crate::stats::Sum;

/// Where the settings partition must start (partitions.csv).
pub const BASE: u32 = 0x3F_E000;
/// The chip's size (the CYD's flash).
const CHIP: u32 = 4 * 1024 * 1024;

// The area is above the program's start, on a sector, inside the chip,
// and nowhere near the pairing.
const _: () = assert!(BASE >= 0x1_0000 && BASE.is_multiple_of(4096));
const _: () = assert!(BASE + AREA <= CHIP && BASE >= PAIRING.end);

/// A buffer the flash functions can use directly (no 4 KB copy on the
/// stack: esp-storage makes one for buffers not on a word boundary).
#[repr(align(4))]
struct Aligned<const N: usize>([u8; N]);

/// Reads and writes go through a word-aligned buffer this big, so the
/// caller's buffer (a 16-byte record anywhere on the stack) never makes
/// esp-storage copy through 4 KB of stack. 128: settings-store scans in
/// 128-byte reads, so a scan is still one flash call per read (32 made
/// four, and the start 1.4 ms slower).
const CHUNK: usize = 128;

/// Lines in the partition table's 3 KB (0x8000..0x8C00).
const TABLE_LINES: u32 = 0xC00 / ENTRY_LEN as u32;

/// The chip's flash, for the settings area only. Counts and times every
/// call.
pub struct EspFlash {
    fs: FlashStorage<'static>,
    pub reads: Sum,
    pub writes: Sum,
    pub erases: Sum,
}

/// The last line of defence: a chip address range inside the settings
/// area, or no.
fn inside(addr: u32, len: usize) -> Result<(), StoreError> {
    let end = u32::try_from(len)
        .ok()
        .and_then(|l| addr.checked_add(l))
        .ok_or(StoreError::OutOfRange)?;
    if addr >= BASE && end <= BASE + AREA {
        Ok(())
    } else {
        Err(StoreError::OutOfRange)
    }
}

impl Flash for EspFlash {
    // `#[inline(never)]`: esp-storage keeps room for a 4 KB copy in its
    // frame; this keeps it out of the loop's frame (the experiments saw
    // 9.6 KB more stack without it).
    #[inline(never)]
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), StoreError> {
        let _held = enter(addr, buf.len())?;
        let t = Instant::now();
        let mut at = addr;
        let mut r = Ok(());
        for part in buf.chunks_mut(CHUNK) {
            let mut a = Aligned([0u8; CHUNK]);
            r = self.fs.read_nor(at, &mut a.0[..part.len()]);
            if r.is_err() {
                break;
            }
            part.copy_from_slice(&a.0[..part.len()]);
            at += part.len() as u32;
        }
        self.reads.add(t.elapsed().as_micros());
        r.map_err(|_| StoreError::Io)
    }

    /// Writes without erasing (never `FlashStorage::write`: that one
    /// erases the whole sector first).
    #[inline(never)]
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), StoreError> {
        let _held = enter(addr, data.len())?;
        let t = Instant::now();
        let mut at = addr;
        let mut r = Ok(());
        for part in data.chunks(CHUNK) {
            let mut a = Aligned([0u8; CHUNK]);
            a.0[..part.len()].copy_from_slice(part);
            r = self.fs.write_nor(at, &a.0[..part.len()]);
            if r.is_err() {
                break;
            }
            at += part.len() as u32;
        }
        self.writes.add(t.elapsed().as_micros());
        r.map_err(|_| StoreError::Io)
    }

    /// One 4 KB sector (never a 64 KB block).
    #[inline(never)]
    fn erase_sector(&mut self, addr: u32) -> Result<(), StoreError> {
        let _held = enter(addr, 4096)?;
        let t = Instant::now();
        let r = self.fs.erase(addr, addr + 4096);
        self.erases.add(t.elapsed().as_micros());
        r.map_err(|_| StoreError::Io)
    }
}

/// The board's store: the settings area, or none.
pub struct CydStore {
    inner: Option<AreaStore<EspFlash>>,
}

impl CydStore {
    pub fn flash(&self) -> Option<&EspFlash> {
        self.inner.as_ref().map(|s| s.flash())
    }
}

impl Store for CydStore {
    fn size(&self) -> u32 {
        if self.inner.is_some() { AREA } else { 0 }
    }

    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), StoreError> {
        self.inner
            .as_mut()
            .ok_or(StoreError::OutOfRange)?
            .read(offset, buf)
    }

    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), StoreError> {
        self.inner
            .as_mut()
            .ok_or(StoreError::OutOfRange)?
            .write(offset, data)
    }

    fn erase_sector(&mut self, offset: u32) -> Result<(), StoreError> {
        self.inner
            .as_mut()
            .ok_or(StoreError::OutOfRange)?
            .erase_sector(offset)
    }
}

/// The boot animation's part of a settings log line (feature
/// `boot-gather`: ", boot anim " and its word), or nothing.
fn boot_note(s: &Settings) -> (&'static str, &'static str) {
    #[cfg(feature = "boot-gather")]
    {
        let anim = console_core::boot::Anim::from_byte(s.boot_anim());
        (", boot anim ", anim.word())
    }
    #[cfg(not(feature = "boot-gather"))]
    {
        let _ = s;
        ("", "")
    }
}

/// The settings as the board keeps them: what the flash holds, when to
/// save, and what saving cost.
pub struct Saver {
    pub store: SettingsStore,
    pub policy: SavePolicy,
    /// Records written, saves that wrote nothing, saves that failed.
    pub saves: u32,
    pub unchanged: u32,
    pub fails: u32,
    /// One whole save (write, read back, an erase if one was due).
    pub save_us: Sum,
    /// The load and `prepare` at start.
    pub load_us: u64,
}

impl Saver {
    /// The settings read at start (the defaults when none).
    pub fn settings(&self) -> Settings {
        self.store.current()
    }

    /// After a console step: saves `new` (the settings the console has
    /// now) if the rules say so.
    pub fn after_step(&mut self, store: &mut CydStore, now_us: u64, m: Moment, new: Settings) {
        if !self.policy.after_step(now_us, m) {
            return;
        }
        let t = Instant::now();
        let r = self.store.save(store, &new);
        let us = t.elapsed().as_micros();
        let (boot, anim) = boot_note(&new);
        match r {
            Ok(Saved::Unchanged) => {
                self.unchanged += 1;
                info!(
                    "settings: brightness {}{boot}{anim} already in flash, nothing written",
                    new.brightness
                );
            }
            Ok(Saved::Written { erased }) => {
                self.saves += 1;
                self.save_us.add(us);
                info!(
                    "settings: saved brightness {}{boot}{anim} (record {} of {}) in {us} us{}",
                    new.brightness,
                    self.store.used(),
                    settings_store::SLOTS,
                    if erased {
                        ", erased a sector first"
                    } else {
                        ""
                    }
                );
            }
            Err(e) => {
                self.fails += 1;
                self.policy.failed(now_us);
                warn!("settings: not saved ({e:?}) after {us} us; tried again later");
            }
        }
    }

    /// One short line (for `boardctl stats`, whose frame holds 1 KB, and
    /// the log's report): on (an area was found), found (good settings
    /// were read or written), b (brightness held), n (saves written /
    /// unchanged / failed), rec (records in the sector in use), pend (a
    /// save waits), save, wr (one flash write), er (erases: count,
    /// average, largest) in us, load (load and prepare at start, us).
    pub fn write_short(&self, store: &CydStore, out: &mut impl core::fmt::Write) {
        let (w, e) = store
            .flash()
            .map_or((Sum::default(), Sum::default()), |f| (f.writes, f.erases));
        let _ = writeln!(
            out,
            "settings on={} found={} b={} n={}/{}/{} rec={} pend={} save={}/{} wr={}/{} er={}/{}/{} load={}",
            u8::from(store.size() > 0),
            u8::from(self.store.found()),
            self.store.current().brightness,
            self.saves,
            self.unchanged,
            self.fails,
            self.store.used(),
            u8::from(self.policy.pending()),
            self.save_us.avg(),
            self.save_us.max,
            w.avg(),
            w.max,
            e.n,
            e.avg(),
            e.max,
            self.load_us,
        );
    }
}

/// Opens the flash, logs the pairing's CRC-32, finds the settings area,
/// reads the settings and erases the spare sector if it will be needed
/// soon. Call once, early (before the radio: an erase turns interrupts
/// off for tens of milliseconds).
#[inline(never)]
pub fn start(flash: esp_hal::peripherals::FLASH<'static>) -> (CydStore, Saver) {
    let t0 = Instant::now();
    let mut fs = storage(flash);
    crate::pairing::log_crc(&mut fs);
    let mut saver = Saver {
        store: SettingsStore::new(0),
        policy: SavePolicy::new(),
        saves: 0,
        unchanged: 0,
        fails: 0,
        save_us: Sum::default(),
        load_us: 0,
    };
    let area = if esp_storage::flash_encryption() {
        Err("flash-encryption-on")
    } else {
        find(&mut fs)
    };
    let mut store = match area {
        Ok(area) => {
            info!(
                "settings: area {:#x}..{:#x} (partition \"settings\")",
                area.base(),
                area.end()
            );
            CydStore {
                inner: Some(AreaStore::new(
                    EspFlash {
                        fs,
                        reads: Sum::default(),
                        writes: Sum::default(),
                        erases: Sum::default(),
                    },
                    area,
                )),
            }
        }
        Err(why) => {
            warn!("settings: no store ({why}): the brightness is kept until power off only");
            CydStore { inner: None }
        }
    };
    let settings = saver.store.load(&mut store);
    let prepared = saver.store.prepare(&mut store);
    saver.load_us = t0.elapsed().as_micros();
    info!(
        "settings: brightness {} ({}), {} of {} records used, spare sector {}, {} us",
        settings.brightness,
        if saver.store.found() {
            "saved"
        } else {
            "default"
        },
        saver.store.used(),
        settings_store::SLOTS,
        match prepared {
            Ok(true) => "erased now",
            Ok(false) => "ready",
            Err(_) => "not checked",
        },
        saver.load_us,
    );
    (store, saver)
}

/// Reads the table and checks the settings partition. The table is read
/// one 32-byte line at a time (`Entry::parse`), up to its first line
/// that is not an entry, so no 3 KB copy of it is made. Only the
/// unencrypted table is read (start() refuses flash encryption first).
#[inline(never)]
fn find(fs: &mut FlashStorage<'static>) -> Result<settings_store::area::Area, &'static str> {
    let capacity = fs.capacity() as u32;
    let mut failed = false;
    let mut line = 0u32;
    let entries = core::iter::from_fn(|| {
        // The table's whole 3 KB (0xC00) at most: a longer table than
        // find_area checks is then refused ("too many"), never cut short.
        if failed || line >= TABLE_LINES {
            return None;
        }
        let mut raw = Aligned([0u8; ENTRY_LEN]);
        if fs
            .read_nor(TABLE + line * ENTRY_LEN as u32, &mut raw.0)
            .is_err()
        {
            failed = true;
            return None;
        }
        line += 1;
        Entry::parse(&raw.0)
    });
    let found = find_area(entries, capacity.min(CHIP), BASE);
    if failed {
        return Err("no-table");
    }
    found.map_err(|e| e.word())
}

// Feature `core2`. These two helpers sit at the end of the file on
// purpose: the calls above keep their lines, so a build without `core2`
// is byte-for-byte the build before it (line numbers end up in the code).
//
// `enter` checks the range and, with `core2`, first parks the second
// core in RAM after its current send (`core2::Held`), so it holds no
// lock and runs no flash code while this core reads, writes or erases.
// The guard lets it go again when the call returns.
#[cfg(not(feature = "core2"))]
#[inline(always)]
fn enter(addr: u32, len: usize) -> Result<Unheld, StoreError> {
    inside(addr, len).map(|()| Unheld)
}

/// Without `core2` there is nothing to hold: an empty guard.
#[cfg(not(feature = "core2"))]
struct Unheld;

#[cfg(feature = "core2")]
#[inline(always)]
fn enter(addr: u32, len: usize) -> Result<crate::core2::Held, StoreError> {
    inside(addr, len)?;
    crate::core2::Held::park().ok_or(StoreError::Io)
}

// esp-storage refuses every write and erase while the second core runs,
// unless told what to do. With `core2` it stalls that core for each one
// (`CpuControl::park_core`); by then the core already waits in RAM
// (`enter` above), so the stall only adds a second safety net.
#[cfg(not(feature = "core2"))]
#[inline(always)]
fn storage(flash: esp_hal::peripherals::FLASH<'static>) -> FlashStorage<'static> {
    FlashStorage::new(flash)
}

#[cfg(feature = "core2")]
#[inline(always)]
fn storage(flash: esp_hal::peripherals::FLASH<'static>) -> FlashStorage<'static> {
    FlashStorage::new(flash).multicore_auto_park()
}
