//! Saved pairings: the glue between the Bluetooth host (crates/pad-proto)
//! and the flash store (crates/pad-store).
//!
//! A pairing is a gamepad's address plus the secret link key both sides
//! agreed on the first time. With it saved, a gamepad paired once
//! connects again after a restart or a new firmware, without pairing
//! mode.
//!
//! When things happen:
//!
//! - At start-up, before the radio starts: find the "nvs" partition in
//!   the partition table, check it, open the store (the only moment the
//!   store may erase flash: an erase stops the radio for about 45 ms),
//!   and hand every saved pairing to the host.
//! - While the radio runs: the host reports each change (new key, key to
//!   forget). The radio loop applies it right after the host call that
//!   caused it. A save writes one 32-byte record (interrupts, the radio's
//!   too, are off for about 1 ms) and never erases.
//! - "Forget all" (serial key "f", or BOOT held 5 s): the host drops its
//!   keys and the store wipes them.
//!
//! If the store cannot be used (no partition, a check failed, somebody
//! else's data in it, a flash error at start), everything works as
//! before: keys live in RAM only and are lost at the next restart. The
//! About page and the log say so.
//!
//! Log lines start with PAD-STORE. They never hold a key, and never an
//! address either (the host's own lines already name the gamepad).
//!
//! Limits (an experiment): the keys are plain bytes in flash. Anyone with
//! the board and a USB cable can read them out.

use core::fmt::Write as _;

use esp_bootloader_esp_idf::partitions::{self, PARTITION_TABLE_MAX_LEN};
use esp_storage::{FlashStorage, FlashStorageError};
use pad_proto::hci::{BdAddr, LinkKey};
use pad_proto::{Host, KeyChange};
use pad_store::{
    Entry, Error, FindError, Opened, Pairing, Region, RegionError, Saved, Store, find_nvs,
};

use crate::out;

macro_rules! say {
    ($($a:tt)*) => {
        out::line("PAD-STORE", format_args!($($a)*))
    };
}

/// The flash chip, as pad-store wants it. Only these three calls reach
/// the chip, and pad-store calls them only inside its checked region.
///
/// Stack: esp-storage's read and write keep room for a 4 KB copy buffer
/// in their stack frame (used only for unaligned data, which the store
/// never passes). `#[inline(never)]` here and on the `Pairings` methods
/// keeps that room out of the frames of `main` and of the radio loop,
/// which stay on the stack the whole time. It is then taken only for the
/// moment of a flash call. (Without it, `main`'s frame grew by 9.6 KB.)
pub struct EspFlash(FlashStorage<'static>);

impl pad_store::Flash for EspFlash {
    type Error = FlashStorageError;

    #[inline(never)]
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), FlashStorageError> {
        self.0.read_nor(offset, buf)
    }

    /// Writes without erasing: bits can only go from 1 to 0. (Never
    /// `FlashStorage::write`: that one erases the whole sector first.)
    #[inline(never)]
    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), FlashStorageError> {
        self.0.write_nor(offset, data)
    }

    /// One 4 KB sector. (A range of 64 KB would become a slower block
    /// erase; one sector never does.)
    #[inline(never)]
    fn erase_sector(&mut self, offset: u32) -> Result<(), FlashStorageError> {
        self.0.erase(offset, offset + 4096)
    }
}

/// Why the store is not used: a short text for the log and the screen.
#[derive(Clone, Copy)]
pub enum Off {
    Table,
    NoPartition,
    ReadOnly,
    Encrypted,
    Region(RegionError),
    Foreign,
    Flash,
}

impl Off {
    fn word(self) -> &'static str {
        match self {
            Off::Table => "partition table unreadable",
            Off::NoPartition => "no nvs partition",
            Off::ReadOnly => "nvs is read-only",
            Off::Encrypted => "flash encryption is on",
            Off::Region(e) => e.word(),
            Off::Foreign => "nvs holds other data",
            Off::Flash => "flash error",
        }
    }
}

pub struct Pairings {
    store: Option<Store<EspFlash>>,
    off: Option<Off>,
    /// The pairing the board self-test saved last time, if any: 0 or 1
    /// for its two made-up keys.
    #[cfg(feature = "store-selftest")]
    selftest_variant: Option<u8>,
}

/// The self-test's made-up gamepad: a "locally administered" address
/// (no real device has one from a maker) and two made-up keys.
#[cfg(feature = "store-selftest")]
mod selftest {
    pub const ADDR: [u8; 6] = [0x5E, 0x1F, 0x7E, 0x57, 0x00, 0x02];
    pub fn key(variant: u8) -> [u8; 16] {
        core::array::from_fn(|i| (i as u8).wrapping_mul(0x11) ^ 0xA5 ^ variant)
    }
}

impl Pairings {
    /// Finds and opens the store. Call before the radio starts: this may
    /// erase a sector (about 45 ms with every interrupt off).
    #[inline(never)]
    pub fn open(flash: esp_hal::peripherals::FLASH<'static>) -> Self {
        let mut flash = FlashStorage::new(flash);
        let t0 = esp_hal::time::Instant::now();
        let region = match find_region(&mut flash) {
            Ok(r) => r,
            Err(off) => return Self::off(off),
        };
        say!(
            "partition nvs ok: using {:#x}..{:#x} ({} bytes, two areas of 4 KB)",
            region.base(),
            region.base() + region.len(),
            region.len()
        );
        match Store::open(EspFlash(flash), region) {
            Ok((store, rep)) => {
                log_opened(&rep, t0.elapsed().as_micros());
                Self {
                    store: Some(store),
                    off: None,
                    #[cfg(feature = "store-selftest")]
                    selftest_variant: None,
                }
            }
            Err(Error::Foreign) => Self::off(Off::Foreign),
            Err(e) => {
                say!("open failed: {}", e.word());
                Self::off(Off::Flash)
            }
        }
    }

    fn off(off: Off) -> Self {
        say!(
            "store not used ({}): pairings are kept in RAM only and are lost at restart",
            off.word()
        );
        Self {
            store: None,
            off: Some(off),
            #[cfg(feature = "store-selftest")]
            selftest_variant: None,
        }
    }

    /// Hands every saved pairing to the host (oldest first). Before
    /// `Host::start`.
    #[inline(never)]
    pub fn load_into(&mut self, host: &mut Host) {
        let Some(store) = self.store.as_mut() else {
            return;
        };
        #[cfg(feature = "store-selftest")]
        let mut variant = None;
        let mut n = 0;
        let r = store.load(|p| {
            #[cfg(feature = "store-selftest")]
            if p.addr == selftest::ADDR {
                variant = Some(u8::from(p.key == selftest::key(1)));
            }
            host.load_key(BdAddr(p.addr), LinkKey(p.key), p.key_type);
            n += 1;
        });
        #[cfg(feature = "store-selftest")]
        {
            self.selftest_variant = variant;
        }
        match r {
            Ok(()) => say!("loaded {} saved pairing(s) into the host", n),
            Err(e) => say!("load failed after {}: {}", n, e.word()),
        }
    }

    /// Applies one change the host reported. Writes one record at most,
    /// never erases.
    #[inline(never)]
    pub fn apply(&mut self, change: KeyChange) {
        let Some(store) = self.store.as_mut() else {
            match change {
                KeyChange::Save { .. } => say!("new pairing kept in RAM only (no store)"),
                KeyChange::Forget { why, .. } => {
                    say!("pairing dropped from RAM ({}; no store)", why.word())
                }
            }
            return;
        };
        let t0 = esp_hal::time::Instant::now();
        match change {
            KeyChange::Save {
                addr,
                key,
                key_type,
            } => {
                let p = Pairing {
                    addr: addr.0,
                    key: key.0,
                    key_type,
                };
                let r = store.save(&p);
                let us = t0.elapsed().as_micros();
                match r {
                    Ok(Saved::New { evicted }) => say!(
                        "saved new pairing{} us={} pads={} free_slots={}",
                        if evicted.is_some() {
                            " (the oldest made room)"
                        } else {
                            ""
                        },
                        us,
                        store.count(),
                        store.free_slots()
                    ),
                    Ok(Saved::Replaced) => say!(
                        "saved new key for a known pad (old key wiped) us={} pads={} free_slots={}",
                        us,
                        store.count(),
                        store.free_slots()
                    ),
                    Ok(Saved::Unchanged) => {
                        say!("pairing unchanged, nothing written pads={}", store.count())
                    }
                    Err(Error::Full) => say!(
                        "store full: this pairing stays in RAM until a restart (the next start cleans up)"
                    ),
                    Err(e) => say!("save failed: {} us={}", e.word(), us),
                }
            }
            KeyChange::Forget { addr, why } => {
                let r = store.forget(addr.0);
                let us = t0.elapsed().as_micros();
                match r {
                    Ok(true) => say!(
                        "forgot one pairing ({}), key bytes wiped us={} pads={}",
                        why.word(),
                        us,
                        store.count()
                    ),
                    Ok(false) => say!("forget ({}): it was not saved", why.word()),
                    Err(e) => say!("forget failed: {} us={}", e.word(), us),
                }
            }
        }
    }

    /// Wipes every saved pairing (the user asked).
    #[inline(never)]
    pub fn forget_all(&mut self) {
        let Some(store) = self.store.as_mut() else {
            say!("forget all: RAM only, nothing saved to wipe");
            return;
        };
        let t0 = esp_hal::time::Instant::now();
        match store.forget_all() {
            Ok(n) => say!(
                "forgot all pairings: {} wiped us={} pads={}",
                n,
                t0.elapsed().as_micros(),
                store.count()
            ),
            Err(e) => say!("forget all failed: {}", e.word()),
        }
    }

    /// The About page line, from the host's count (the two agree when the
    /// store works; without a store the host's RAM table is all there is).
    pub fn about_line(&self, host_count: usize) -> heapless::String<54> {
        let mut l = heapless::String::new();
        let _ = match self.off {
            None => write!(l, "Known pads: {} (saved)", host_count),
            Some(off) => write!(l, "Known pads: {} (RAM only: {})", host_count, off.word()),
        };
        l
    }

    /// Board self-test: saves the made-up pairing (the other made-up key
    /// than last time, so every start really writes). Only in builds with
    /// the `store-selftest` feature.
    #[cfg(feature = "store-selftest")]
    pub fn selftest_save(&mut self, host: &mut Host) {
        let variant = match self.selftest_variant {
            Some(v) => 1 - v,
            None => 0,
        };
        say!(
            "self-test: saving a made-up pairing (made-up address, made-up key {}) while the radio searches",
            variant
        );
        let key = selftest::key(variant);
        host.load_key(BdAddr(selftest::ADDR), LinkKey(key), 0x04);
        self.apply(KeyChange::Save {
            addr: BdAddr(selftest::ADDR),
            key: LinkKey(key),
            key_type: 0x04,
        });
        self.selftest_variant = Some(variant);
    }
}

fn log_opened(r: &Opened, us: u64) {
    say!(
        "opened us={} created={} repaired={} cleaned_spare={} compacted={} damaged={} dropped={} generation={}",
        us,
        u8::from(r.created),
        u8::from(r.repaired),
        u8::from(r.cleaned_spare),
        u8::from(r.compacted),
        r.damaged,
        r.dropped,
        r.generation
    );
}

/// Reads the partition table and checks the "nvs" partition: the right
/// type and name, writable, not encrypted, and the two sectors the store
/// uses above the partition table, inside the chip and clear of every
/// other partition (the program included). The rules live in
/// `pad_store::find_nvs`, where they are tested on the PC.
/// The entry's 16 raw name bytes.
fn label_bytes(e: &partitions::PartitionEntry) -> [u8; 16] {
    let mut l = [0u8; 16];
    l.copy_from_slice(e.label());
    l
}

#[inline(never)]
fn find_region(flash: &mut FlashStorage<'static>) -> Result<Region, Off> {
    if esp_storage::flash_encryption() {
        return Err(Off::Encrypted);
    }
    let capacity = flash.capacity() as u32;
    let mut buf = [0u8; PARTITION_TABLE_MAX_LEN];
    let table = partitions::read_partition_table(flash, &mut buf).map_err(|_| Off::Table)?;
    // The raw type bytes, not `partition_type()`: that one stops the
    // program on a sub-type it does not know.
    let entries = table.iter().map(|e| Entry {
        kind: e.raw_type(),
        subtype: e.raw_subtype(),
        label: label_bytes(&e),
        offset: e.offset(),
        len: e.len(),
        read_only: e.is_read_only(),
        encrypted: e.is_encrypted(),
    });
    find_nvs(entries, capacity).map_err(|e| match e {
        FindError::NoPartition => Off::NoPartition,
        FindError::ReadOnly => Off::ReadOnly,
        FindError::Encrypted => Off::Encrypted,
        FindError::TooManyEntries => Off::Table,
        FindError::Region(r) => Off::Region(r),
    })
}
