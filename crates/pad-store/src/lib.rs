//! Saves gamepad pairings in flash, so a gamepad paired once can come
//! back after the board restarts or gets a new firmware.
//!
//! A pairing is the gamepad's address (6 bytes), the link key (16 bytes,
//! a secret both sides keep after pairing) and the key's type (1 byte).
//!
//! No heap and no operating system. The flash is reached through the
//! small [`Flash`] trait, so the tests on the PC use a fake flash that
//! behaves like the real one (see `fake.rs`).
//!
//! # How flash behaves (NOR flash, as on the ESP32 boards)
//!
//! - Erasing works on whole sectors of 4096 bytes and sets every bit to 1
//!   (every byte reads 0xFF). It is slow: about 45 ms, sometimes more.
//! - Writing can only turn bits from 1 into 0. To turn a 0 back into a 1,
//!   the whole sector must be erased.
//! - A power cut can stop a write or an erase half way.
//!
//! # The layout
//!
//! Only the first two sectors of the partition are used. Each one is an
//! "area" of 4096 bytes: a 32-byte header, then 127 slots of 32 bytes.
//! One area is active; the other one is blank and waits for the next
//! clean-up.
//!
//! - Header: `"PADS"`, version, a generation counter, a CRC-32 check sum.
//!   The valid header with the highest generation marks the active area.
//! - Record: a magic number, the kind (key / forget one / forget all),
//!   the key type, the address, the key, and a CRC-32. A record with a
//!   wrong check sum is ignored: a write cut by a power loss can never
//!   give a wrong key.
//!
//! While the radio runs, the store only appends one 32-byte record (about
//! 1 ms) or overwrites old records with zeros (also about 1 ms each).
//! It never erases then. Erasing happens only in [`Store::open`], at
//! start-up, before the radio is switched on. See `store.rs` for the
//! power-loss reasoning.
//!
//! # Three ways in
//!
//! - [`Store::open`] (the test firmware): writes at every start, and may
//!   erase, repair, clean up and drop pads. With [`Store::save`], a fifth
//!   pad pushes the oldest out.
//! - [`Store::attach`] (the console, which writes only when the user
//!   pairs or forgets): reads only, refuses what it cannot append to
//!   safely, never repairs. With [`Store::save_new`], a fifth pad is
//!   refused ([`SaveError::Full`]) and nothing is ever erased.
//! - [`read_pairings`]: reads through a trait that has no write at all.

#![cfg_attr(not(test), no_std)]

pub mod crc;
mod read;
pub mod region;
mod store;

#[cfg(test)]
mod fake;
#[cfg(test)]
mod tests;

pub use read::{ReadFlash, ReadReport, read_pairings};
pub use region::{
    Entry, FIRST_FREE, FindError, MAX_ENTRIES, Region, RegionError, SECTOR, Span, USED,
    check_region, find_nvs, label,
};
pub use store::{
    AREA, AttachError, Attached, COMPACT_BELOW, Error, Flash, MAX_PADS, Opened, Pairing, RECORD,
    SLOTS, SaveError, Saved, Store,
};
