//! The saved gamepad pairings (features `store` and `pad`): only ever
//! read, at start. Nothing in this firmware writes or erases them.
//!
//! - [`log_crc`]: the area's CRC-32 to the log, never the bytes (link
//!   keys are secrets); the CRC staying the same at every start shows the
//!   area was never written.
//! - [`load`] (feature `pad`): the pairings handed to the gamepad host,
//!   through a reader that can only read, and only inside the area; and
//!   the saved pads' addresses given to the radio's gate, so an accept or
//!   a key reply can only ever go to one of them.
//!
//! Why nothing writes the area: the firmware has exactly two ways to the
//! flash. The settings store (src/flash_store.rs) writes and erases, and
//! every one of its writes and erases is checked to lie inside the
//! "settings" partition (0x3FE000..0x400000) before it reaches the chip
//! (`inside`, and settings-store's own bounds check). The pad path is
//! [`ReadOnly`] here, which has no write or erase at all, and
//! `pad_store::ReadFlash`, which pad-host takes, has only `read`.
//! pad-host never gets the flash for anything else.

use esp_storage::FlashStorage;
use log::{info, warn};
use settings_store::crc32_more;

/// The saved pairing: the first 8 KB of "nvs".
pub const PAIRING: core::ops::Range<u32> = 0x9000..0xB000;

/// A buffer the flash functions can use directly (on a word boundary).
#[repr(align(4))]
struct Aligned([u8; 256]);

/// CRC-32 of the pairing area, read 256 bytes at a time. Only the CRC
/// leaves this function; the buffer is cleared before it returns.
#[inline(never)]
fn crc(fs: &mut FlashStorage<'_>) -> Result<u32, ()> {
    let mut buf = Aligned([0u8; 256]);
    let mut crc = 0;
    let mut at = PAIRING.start;
    let mut result = Ok(());
    while at < PAIRING.end {
        if fs.read_nor(at, &mut buf.0).is_err() {
            result = Err(());
            break;
        }
        crc = crc32_more(crc, &buf.0);
        at += buf.0.len() as u32;
    }
    buf.0.fill(0);
    // The clearing must not be left out as a dead store.
    core::hint::black_box(&buf);
    result.map(|()| crc)
}

/// Logs the pairing area's CRC-32 (or that it could not be read).
pub fn log_crc(fs: &mut FlashStorage<'_>) {
    match crc(fs) {
        Ok(crc) => info!(
            "pairing area crc32={crc:#010x} ({:#x}..{:#x}, read only)",
            PAIRING.start, PAIRING.end
        ),
        Err(()) => warn!("pairing area: could not be read"),
    }
}

#[cfg(feature = "pad")]
pub use pad::load;

#[cfg(feature = "pad")]
mod pad {
    use esp_storage::FlashStorage;
    use hci_rules::Gate;
    use log::{info, warn};
    use pad_host::{PadHost, Radio};
    use pad_store::{Entry, ReadFlash};
    use settings_store::area::{ENTRY_LEN, TABLE};

    use super::PAIRING;

    /// Lines of the partition table looked at, at most (its 3 KB).
    const TABLE_LINES: u32 = 0xC00 / ENTRY_LEN as u32;
    /// The chip's size (4 MB), whatever the flash says.
    const CHIP: u32 = 4 * 1024 * 1024;

    #[repr(align(4))]
    struct Line([u8; ENTRY_LEN]);

    /// Why a read was not done.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ReadError {
        /// Outside the pairing area: never read through here.
        Outside,
        /// The flash said no.
        Flash,
    }

    /// The pairing area, read only: no write, no erase, and no read
    /// outside 0x9000..0xB000. What it reads lands in the caller's buffer
    /// only; its own aligned buffer is cleared after each read.
    pub struct ReadOnly<'a, 'd> {
        fs: &'a mut FlashStorage<'d>,
        pub reads: u32,
    }

    impl<'a, 'd> ReadOnly<'a, 'd> {
        pub fn new(fs: &'a mut FlashStorage<'d>) -> Self {
            ReadOnly { fs, reads: 0 }
        }
    }

    impl ReadFlash for ReadOnly<'_, '_> {
        type Error = ReadError;
        fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), ReadError> {
            let end = offset
                .checked_add(buf.len() as u32)
                .ok_or(ReadError::Outside)?;
            if offset < PAIRING.start || end > PAIRING.end {
                return Err(ReadError::Outside);
            }
            self.reads += 1;
            // Word-aligned pieces of at most 32 bytes (pad-store reads
            // 32-byte blocks at 32-byte offsets).
            let mut a = Line([0u8; ENTRY_LEN]);
            let mut done = 0;
            let mut result = Ok(());
            while done < buf.len() {
                let at = offset + done as u32;
                let base = at & !3;
                let skip = (at - base) as usize;
                let take = (ENTRY_LEN - skip).min(buf.len() - done);
                let whole = (skip + take).next_multiple_of(4);
                if self.fs.read_nor(base, &mut a.0[..whole]).is_err() {
                    result = Err(ReadError::Flash);
                    break;
                }
                buf[done..done + take].copy_from_slice(&a.0[skip..skip + take]);
                done += take;
            }
            a.0.fill(0);
            core::hint::black_box(&a);
            result
        }
    }

    /// The "nvs" partition as pad-store checks it (data, sub-type NVS,
    /// named "nvs", not read-only, not encrypted, overlapping nothing),
    /// read from the partition table one 32-byte line at a time.
    fn region(fs: &mut FlashStorage<'_>) -> Result<pad_store::Region, &'static str> {
        let capacity = (fs.capacity() as u32).min(CHIP);
        let mut failed = false;
        let mut line = 0u32;
        let entries = core::iter::from_fn(|| {
            if failed || line >= TABLE_LINES {
                return None;
            }
            let mut raw = Line([0u8; ENTRY_LEN]);
            if fs
                .read_nor(TABLE + line * ENTRY_LEN as u32, &mut raw.0)
                .is_err()
            {
                failed = true;
                return None;
            }
            line += 1;
            settings_store::area::Entry::parse(&raw.0).map(|e| Entry {
                kind: e.kind,
                subtype: e.subtype,
                label: e.label,
                offset: e.offset,
                len: e.len,
                read_only: e.read_only,
                encrypted: e.encrypted,
            })
        });
        let found = pad_store::find_nvs(entries, capacity);
        if failed {
            return Err("no-table");
        }
        let r = found.map_err(|e| match e {
            pad_store::FindError::NoPartition => "no-nvs-partition",
            pad_store::FindError::ReadOnly => "nvs-read-only",
            pad_store::FindError::Encrypted => "nvs-encrypted",
            pad_store::FindError::TooManyEntries => "too-many-entries",
            pad_store::FindError::Region(r) => r.word(),
        })?;
        // The pairing has always been here (espflash's default table);
        // anywhere else is not ours to read.
        if r.base() != PAIRING.start || r.base() + r.len() != PAIRING.end {
            return Err("nvs-moved");
        }
        Ok(r)
    }

    /// Reads the saved pairings (read only) into the gamepad host and
    /// returns the gate that lets the radio accept and answer only those
    /// pads. Logs one line: how many, the reads, the area's CRC-32; never
    /// an address or a key. On any problem nothing is loaded and the
    /// gate knows no pad: the console then simply waits for nobody.
    #[inline(never)]
    pub fn load(fs: &mut FlashStorage<'_>, pads: &mut PadHost, log: &mut impl Radio) -> Gate {
        let mut gate = Gate::new();
        if esp_storage::flash_encryption() {
            warn!("pad: pairings not read (flash encryption is on); no pad can connect");
            return gate;
        }
        let t = crate::board::now_us();
        let region = match region(fs) {
            Ok(r) => r,
            Err(why) => {
                warn!("pad: pairings not read ({why}); no pad can connect");
                return gate;
            }
        };
        let mut ro = ReadOnly::new(fs);
        let loaded = pads.load_pairings(&mut ro, region, log);
        let reads = ro.reads;
        let mut allowed = 0;
        for i in 0..hci_rules::SAVED {
            if let Some(a) = pads.saved_address(i)
                && gate.allow(a)
            {
                allowed += 1;
            }
        }
        match loaded {
            Ok(l) => info!(
                "pad: pairings loaded (read only): saved pads={} gate allows={allowed} reads={reads} crc32={:#010x} in {} us",
                l.keys,
                l.report.crc,
                crate::board::now_us() - t
            ),
            Err(e) => warn!("pad: pairings not read: {}", e.word()),
        }
        gate
    }
}
