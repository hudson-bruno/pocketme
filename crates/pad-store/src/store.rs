//! The store itself.
//!
//! # Why a power cut never loses a saved pairing, and never gives a wrong key
//!
//! - Every header and record carries a CRC-32. A write cut half way
//!   leaves bytes that do not match their sum, so they are ignored.
//! - Saving appends a new record. The older record of the same address
//!   is wiped (overwritten with zeros) only after the new one is written
//!   and read back. A cut before that leaves the old one, a cut after it
//!   leaves the new one.
//! - Forgetting appends a "forget" record, then wipes the old records.
//!   A cut before the marker leaves the pairing (it was not forgotten
//!   yet, but it is right); a cut after it forgets it.
//! - The clean-up (only at start-up) copies the live records into the
//!   blank area, writes that area's header LAST with a higher
//!   generation, and only then erases the old area. Until the new header
//!   is complete, the old area is the active one.
//! - A partition that holds somebody else's data is never touched: if
//!   neither area has our header, both must be blank (or area 0 must hold
//!   nothing but the start of our own first header, cut by a power loss).
//!
//! # Two ways in: `open` (tidies) and `attach` (never tidies)
//!
//! [`Store::open`] is the test firmware's way in: it writes at every
//! start and may erase, repair, compact and wipe pads dropped for room.
//! [`Store::attach`] is the console's way in, for the rule "the pairing
//! area is written only by pair and forget": it only reads. It refuses
//! (with [`AttachError`]) what cannot be appended to safely, and leaves
//! everything else exactly as it is, only reporting it ([`Attached`]):
//!
//! | The area holds | `attach` |
//! |---|---|
//! | both sectors blank (a board flashed from zero) | takes it, `fresh`; the first [`Store::save_new`] writes the header, then the record |
//! | area 0: only part of our first header (a cut first pair or first `open`), area 1 blank | takes it, `fresh` and `cut_header`; the first save writes the whole header over the part (same bytes, bits only go 1 to 0), reads it back, then the record |
//! | anything else without a valid header | refuses: `Foreign` (nothing written, ever) |
//! | a valid header, damaged records (a cut write or wipe) | takes it; they are ignored and counted, new records go after the last used slot, as with `open` |
//! | a valid header, the other area not blank (a clean-up cut by a power loss, or other bytes) | takes it, `spare_dirty`; the other area is never written or erased |
//! | a valid header, no free slot | takes it (load and forget work); `save_new` says `NoRoom` |
//! | a valid header, a pad that `open` would wipe because more than [`MAX_PADS`] were saved (a cut while `save` pushed one out) | refuses: `TooMany`, since the hidden pad would come back after the next forget; `open` (test firmware) clears it |
//!
//! [`Store::save_new`] never drops a pad, never erases and never
//! compacts: a fifth address is [`SaveError::Full`], no free slot is
//! [`SaveError::NoRoom`], both before anything is written.

use core::fmt;

use crate::crc::crc32;
use crate::region::{Region, SECTOR, USED};

/// The most gamepads kept. [`Store::save`] of one more drops the one
/// saved longest ago; [`Store::save_new`] refuses it instead. The
/// GuliKit pad uses three addresses for its four modes.
pub const MAX_PADS: usize = 4;
/// One area: one flash sector.
pub const AREA: u32 = SECTOR;
/// One header or record, in bytes.
pub const RECORD: usize = 32;
/// Record slots in one area (the first 32 bytes hold the header).
pub const SLOTS: u32 = AREA / RECORD as u32 - 1;
/// At start-up, clean up when fewer slots than this are free.
pub const COMPACT_BELOW: u32 = 32;

const HEADER_MAGIC: [u8; 4] = *b"PADS";
const VERSION: u8 = 1;
const RECORD_MAGIC: [u8; 2] = [0x50, 0x4B];
const KIND_KEY: u8 = 1;
const KIND_FORGET: u8 = 2;
const KIND_FORGET_ALL: u8 = 3;
/// How many slots a save tries when the read-back does not match.
const WRITE_TRIES: u32 = 3;

/// The flash, as the store needs it. Offsets are absolute (from the
/// start of the chip). Only the store's own bounds check (`Bounded`)
/// calls `write` and `erase_sector`.
pub trait Flash {
    type Error: fmt::Debug;
    /// Reads `buf.len()` bytes at `offset`.
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), Self::Error>;
    /// Writes `data` at `offset`. Bits can only go from 1 to 0. The
    /// offset and the length are multiples of 4; `data` is 4-byte
    /// aligned in memory.
    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), Self::Error>;
    /// Erases the one sector (4096 bytes) that starts at `offset`.
    fn erase_sector(&mut self, offset: u32) -> Result<(), Self::Error>;
}

impl<T: Flash> Flash for &mut T {
    type Error = T::Error;
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), Self::Error> {
        (**self).read(offset, buf)
    }
    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), Self::Error> {
        (**self).write(offset, data)
    }
    fn erase_sector(&mut self, offset: u32) -> Result<(), Self::Error> {
        (**self).erase_sector(offset)
    }
}

/// One saved pairing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Pairing {
    /// The gamepad's Bluetooth address, in the byte order of HCI packets.
    pub addr: [u8; 6],
    /// The link key. A secret: never print it.
    pub key: [u8; 16],
    /// The key's type as the radio reported it (0x04: made by "just works"
    /// pairing).
    pub key_type: u8,
}

impl fmt::Debug for Pairing {
    // Neither the key nor the address goes into a log or a test failure.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Pairing(key_type={:#04x}, key hidden)", self.key_type)
    }
}

/// What went wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error<E> {
    /// The flash itself said no.
    Flash(E),
    /// A read, write or erase would have gone outside the two sectors.
    /// Refused before the flash was touched.
    OutOfBounds,
    /// No free slot left. The pairing stays in RAM only until the next
    /// start-up, which cleans up.
    Full,
    /// What was written did not read back the same, several times.
    Verify,
    /// The partition holds data that is not ours. Nothing was written.
    Foreign,
}

impl<E> Error<E> {
    /// A short word for the log and the screen.
    pub fn word(&self) -> &'static str {
        match self {
            Error::Flash(_) => "flash-error",
            Error::OutOfBounds => "out-of-bounds",
            Error::Full => "full",
            Error::Verify => "read-back-differs",
            Error::Foreign => "partition-holds-other-data",
        }
    }
}

/// What [`Store::save`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Saved {
    /// A gamepad not saved before. If the store was full, the one saved
    /// longest ago was dropped (its address is here, for the caller).
    New { evicted: Option<[u8; 6]> },
    /// A new key for a gamepad saved before.
    Replaced,
    /// The very same pairing was saved already: nothing written.
    Unchanged,
}

/// Why [`Store::attach`] refused. Nothing was written or erased.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachError<E> {
    /// The flash itself said no to a read.
    Flash(E),
    /// A read would have gone outside the two sectors.
    OutOfBounds,
    /// The partition holds data that is not ours (no valid header, and
    /// neither blank nor the start of our own first header).
    Foreign,
    /// More than [`MAX_PADS`] pads are saved: an older one, left out
    /// today, would come back after the next forget. Only
    /// [`Store::open`] clears this (it wipes that pad).
    TooMany,
}

impl<E> AttachError<E> {
    /// A short word for the log and the screen.
    pub fn word(&self) -> &'static str {
        match self {
            AttachError::Flash(_) => "flash-error",
            AttachError::OutOfBounds => "out-of-bounds",
            AttachError::Foreign => "partition-holds-other-data",
            AttachError::TooMany => "more-pads-than-the-limit",
        }
    }
}

/// Every error of a read is a flash error or out of bounds.
fn read_err<E>(e: Error<E>) -> AttachError<E> {
    match e {
        Error::Flash(e) => AttachError::Flash(e),
        _ => AttachError::OutOfBounds,
    }
}

/// Why [`Store::save_new`] did not save.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveError<E> {
    /// A new address while [`MAX_PADS`] pads are saved. Nothing written:
    /// one must be forgotten first.
    Full,
    /// No free record slot left. Nothing written. (Only a clean-up,
    /// which erases, makes room; the console never runs one.)
    NoRoom,
    /// The flash itself said no.
    Flash(E),
    /// A write would have gone outside the two sectors.
    OutOfBounds,
    /// What was written did not read back the same, several times.
    Verify,
}

impl<E> SaveError<E> {
    /// A short word for the log and the screen.
    pub fn word(&self) -> &'static str {
        match self {
            SaveError::Full => "four-pads-saved",
            SaveError::NoRoom => "no-free-slot",
            SaveError::Flash(_) => "flash-error",
            SaveError::OutOfBounds => "out-of-bounds",
            SaveError::Verify => "read-back-differs",
        }
    }
}

impl<E> From<Error<E>> for SaveError<E> {
    fn from(e: Error<E>) -> Self {
        match e {
            Error::Flash(e) => SaveError::Flash(e),
            // Only `append` says Full: no slot left.
            Error::Full => SaveError::NoRoom,
            Error::Verify => SaveError::Verify,
            // `Foreign` comes only from `open`; a save never makes it.
            Error::OutOfBounds | Error::Foreign => SaveError::OutOfBounds,
        }
    }
}

/// What [`Store::attach`] found. It wrote nothing and erased nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Attached {
    /// No header yet (both sectors blank, or `cut_header`): the first
    /// save writes the header, then the record.
    pub fresh: bool,
    /// Area 0 holds only the start of our first header (a power cut
    /// during the very first save). The first save writes the whole
    /// header over it; `open` would erase and set it up again.
    pub cut_header: bool,
    /// The other area is not blank (a clean-up cut by a power loss, or
    /// other bytes). Left alone; `open` would erase it.
    pub spare_dirty: bool,
    /// Records ignored because their check sum did not match.
    pub damaged: u32,
    /// The active area's generation (1 when `fresh`).
    pub generation: u32,
}

/// What [`Store::open`] found and did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Opened {
    /// The partition was blank: a new store was set up.
    pub created: bool,
    /// Area 0 held only a cut first header (a power loss during the very
    /// first start): it was erased and set up again.
    pub repaired: bool,
    /// The spare area held old data (from a clean-up cut by a power
    /// loss): it was erased.
    pub cleaned_spare: bool,
    /// Few free slots were left: the live records were copied into the
    /// spare area.
    pub compacted: bool,
    /// Records ignored because their check sum did not match.
    pub damaged: u32,
    /// Gamepads dropped because more than [`MAX_PADS`] were found.
    pub dropped: u32,
    /// The active area's generation (counts clean-ups).
    pub generation: u32,
}

/// A 32-byte block, 4-byte aligned (the ESP32's flash routines need
/// aligned memory, or they copy through a 4 KB buffer on the stack).
#[repr(C, align(4))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Block(pub(crate) [u8; RECORD]);

impl Block {
    pub(crate) const BLANK: Block = Block([0xFF; RECORD]);
    const ZERO: Block = Block([0x00; RECORD]);

    fn header(generation: u32) -> Block {
        let mut b = Block::BLANK;
        b.0[0..4].copy_from_slice(&HEADER_MAGIC);
        b.0[4] = VERSION;
        b.0[8..12].copy_from_slice(&generation.to_le_bytes());
        let crc = crc32(&b.0[..28]);
        b.0[28..32].copy_from_slice(&crc.to_le_bytes());
        b
    }

    fn record(kind: u8, key_type: u8, addr: [u8; 6], key: [u8; 16]) -> Block {
        let mut b = Block::BLANK;
        b.0[0..2].copy_from_slice(&RECORD_MAGIC);
        b.0[2] = VERSION;
        b.0[3] = kind;
        b.0[4] = RECORD as u8;
        b.0[5] = key_type;
        b.0[6..12].copy_from_slice(&addr);
        b.0[12..28].copy_from_slice(&key);
        let crc = crc32(&b.0[..28]);
        b.0[28..32].copy_from_slice(&crc.to_le_bytes());
        b
    }

    fn crc_ok(&self) -> bool {
        crc32(&self.0[..28]).to_le_bytes() == self.0[28..32]
    }

    pub(crate) fn generation(&self) -> Option<u32> {
        (self.0[0..4] == HEADER_MAGIC && self.0[4] == VERSION && self.crc_ok())
            .then(|| u32::from_le_bytes([self.0[8], self.0[9], self.0[10], self.0[11]]))
    }

    pub(crate) fn parse(&self) -> Option<Rec> {
        if self.0[0..2] != RECORD_MAGIC
            || self.0[2] != VERSION
            || usize::from(self.0[4]) != RECORD
            || !self.crc_ok()
        {
            return None;
        }
        let mut addr = [0; 6];
        addr.copy_from_slice(&self.0[6..12]);
        match self.0[3] {
            KIND_KEY => {
                let mut key = [0; 16];
                key.copy_from_slice(&self.0[12..28]);
                Some(Rec::Key(Pairing {
                    addr,
                    key,
                    key_type: self.0[5],
                }))
            }
            KIND_FORGET => Some(Rec::Forget(addr)),
            KIND_FORGET_ALL => Some(Rec::ForgetAll),
            _ => None,
        }
    }

    pub(crate) fn is_blank(&self) -> bool {
        self.0.iter().all(|&b| b == 0xFF)
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.0.iter().all(|&b| b == 0)
    }
}

pub(crate) enum Rec {
    Key(Pairing),
    Forget([u8; 6]),
    ForgetAll,
}

/// The only code that writes or erases. Every call is checked against
/// the two sectors of the region before the flash is touched.
pub(crate) struct Bounded<F> {
    flash: F,
    base: u32,
    len: u32,
}

impl<F: Flash> Bounded<F> {
    pub(crate) fn new(flash: F, region: Region) -> Self {
        Self {
            flash,
            base: region.base(),
            len: USED,
        }
    }

    /// The absolute address of `rel`, if `rel..rel+n` lies inside.
    fn inside(&self, rel: u32, n: usize) -> Result<u32, Error<F::Error>> {
        let n = u32::try_from(n).map_err(|_| Error::OutOfBounds)?;
        match rel.checked_add(n) {
            Some(end) if end <= self.len => Ok(self.base + rel),
            _ => Err(Error::OutOfBounds),
        }
    }

    pub(crate) fn read(&mut self, rel: u32, buf: &mut [u8]) -> Result<(), Error<F::Error>> {
        let at = self.inside(rel, buf.len())?;
        self.flash.read(at, buf).map_err(Error::Flash)
    }

    pub(crate) fn write(&mut self, rel: u32, data: &[u8]) -> Result<(), Error<F::Error>> {
        if !rel.is_multiple_of(4) || !data.len().is_multiple_of(4) {
            return Err(Error::OutOfBounds);
        }
        let at = self.inside(rel, data.len())?;
        self.flash.write(at, data).map_err(Error::Flash)
    }

    pub(crate) fn erase(&mut self, rel: u32) -> Result<(), Error<F::Error>> {
        if !rel.is_multiple_of(SECTOR) {
            return Err(Error::OutOfBounds);
        }
        let at = self.inside(rel, SECTOR as usize)?;
        self.flash.erase_sector(at).map_err(Error::Flash)
    }

    pub(crate) fn into_inner(self) -> F {
        self.flash
    }
}

/// Where slot `slot` of area `area` starts (slot 0 is the header).
pub(crate) fn at(area: u32, slot: u32) -> u32 {
    area * AREA + slot * RECORD as u32
}

#[derive(Clone, Copy)]
struct Live {
    addr: [u8; 6],
    slot: u32,
}

const NO_LIVE: Live = Live {
    addr: [0; 6],
    slot: 0,
};

/// The pairing store. Holds no keys in RAM: only which slot holds the
/// newest record of each gamepad.
pub struct Store<F> {
    b: Bounded<F>,
    /// The active area: 0 or 1.
    active: u32,
    generation: u32,
    /// The next free slot (1 to SLOTS; SLOTS + 1 when full).
    next: u32,
    /// Saved gamepads, the one saved longest ago first.
    live: [Live; MAX_PADS],
    n: usize,
    /// No header in flash yet (only from `attach`): the first append
    /// writes it first.
    fresh: bool,
}

/// What a read-only scan of the active area found.
struct Scanned {
    damaged: u32,
    dropped: u32,
    /// The first [`MAX_PADS`] addresses dropped for room.
    dropped_addrs: [Option<[u8; 6]>; MAX_PADS],
}

impl<F: Flash> Store<F> {
    /// Opens the store at start-up. This is the only place that erases,
    /// so call it before the radio starts. It sets the store up on a
    /// blank partition, repairs what a power cut left, and cleans up when
    /// few slots are free. It always writes once (the header again, with
    /// the same bytes), so the flash chip's one-time unlock happens now
    /// and not later while the radio runs.
    pub fn open(flash: F, region: Region) -> Result<(Self, Opened), Error<F::Error>> {
        let mut b = Bounded::new(flash, region);
        let mut rep = Opened::default();
        let g0 = read_block(&mut b, at(0, 0))?.generation();
        let g1 = read_block(&mut b, at(1, 0))?.generation();
        let (active, generation) = match (g0, g1) {
            (Some(a), Some(c)) if c > a => (1, c),
            (Some(a), _) => (0, a),
            (None, Some(c)) => (1, c),
            (None, None) => {
                // Not ours yet. Only a blank partition (or our own cut
                // first header) is taken; anything else is left alone.
                if !area_blank(&mut b, 1)? {
                    return Err(Error::Foreign);
                }
                if !area_blank(&mut b, 0)? {
                    if !cut_first_header(&mut b)? {
                        return Err(Error::Foreign);
                    }
                    b.erase(at(0, 0))?;
                    rep.repaired = true;
                }
                write_verified(&mut b, at(0, 0), &Block::header(1))?;
                rep.created = true;
                (0, 1)
            }
        };
        let mut s = Store {
            b,
            active,
            generation,
            next: 1,
            live: [NO_LIVE; MAX_PADS],
            n: 0,
            fresh: false,
        };
        s.scan(&mut rep)?;
        let spare = 1 - s.active;
        if !area_blank(&mut s.b, spare)? {
            s.b.erase(at(spare, 0))?;
            rep.cleaned_spare = true;
        }
        if s.free_slots() < COMPACT_BELOW {
            s.compact()?;
            rep.compacted = true;
        } else {
            // The same header bytes again: changes nothing, but makes the
            // flash chip's one-time unlock (about 10 ms) happen now.
            let h = Block::header(s.generation);
            s.b.write(at(s.active, 0), &h.0)?;
        }
        rep.generation = s.generation;
        Ok((s, rep))
    }

    /// Takes the store as it is, for a firmware that may write only when
    /// the user pairs or forgets a pad. Only reads: no header rewrite, no
    /// repair, no clean-up, no wipe of pads dropped for room, no erase.
    /// Every later write comes from [`Store::save_new`], [`Store::forget`]
    /// (or the other writing calls the caller makes).
    ///
    /// Refuses, with nothing written, when the partition is not ours
    /// ([`AttachError::Foreign`]) or a pad left out for room would come
    /// back after a forget ([`AttachError::TooMany`]). A blank partition,
    /// or one holding only the start of our first header, is taken as
    /// `fresh`: the first save writes the header, then the record. The
    /// module text of `store.rs` has the table of every case.
    ///
    /// About 260 reads of 32 bytes; finds the same pads, in the same
    /// order, as [`read_pairings`](crate::read_pairings).
    pub fn attach(flash: F, region: Region) -> Result<(Self, Attached), AttachError<F::Error>> {
        let mut b = Bounded::new(flash, region);
        let mut rep = Attached::default();
        let g0 = read_block(&mut b, at(0, 0)).map_err(read_err)?.generation();
        let g1 = read_block(&mut b, at(1, 0)).map_err(read_err)?.generation();
        let (active, generation) = match (g0, g1) {
            (Some(a), Some(c)) if c > a => (1, c),
            (Some(a), _) => (0, a),
            (None, Some(c)) => (1, c),
            (None, None) => {
                // The same test as `open`, but nothing is repaired here.
                if !area_blank(&mut b, 1).map_err(read_err)? {
                    return Err(AttachError::Foreign);
                }
                if !area_blank(&mut b, 0).map_err(read_err)? {
                    if !cut_first_header(&mut b).map_err(read_err)? {
                        return Err(AttachError::Foreign);
                    }
                    rep.cut_header = true;
                }
                rep.fresh = true;
                (0, 1)
            }
        };
        let mut s = Store {
            b,
            active,
            generation,
            next: 1,
            live: [NO_LIVE; MAX_PADS],
            n: 0,
            fresh: rep.fresh,
        };
        if !s.fresh {
            let sc = s.read_live().map_err(read_err)?;
            rep.damaged = sc.damaged;
            if sc.dropped as usize > MAX_PADS {
                // More dropped than listed: cannot check them all.
                return Err(AttachError::TooMany);
            }
            for d in sc.dropped_addrs.into_iter().flatten() {
                if s.find(d).is_none() && s.alive_without_limit(d).map_err(read_err)? {
                    return Err(AttachError::TooMany);
                }
            }
            rep.spare_dirty = !area_blank(&mut s.b, 1 - s.active).map_err(read_err)?;
        }
        rep.generation = s.generation;
        Ok((s, rep))
    }

    /// Saves a pairing without ever pushing another pad out, erasing or
    /// cleaning up.
    ///
    /// - The same pairing (address, key and key type) saved already:
    ///   [`Saved::Unchanged`], nothing written.
    /// - A new key for a saved address: the new record is written and
    ///   read back, then the old one is zeroed ([`Saved::Replaced`], two
    ///   writes). Allowed with [`MAX_PADS`] saved: it adds no pad.
    /// - A new address: [`SaveError::Full`] when [`MAX_PADS`] are saved,
    ///   else one record ([`Saved::New`] with `evicted: None`, always).
    /// - No free slot: [`SaveError::NoRoom`]. Both refusals come before
    ///   any write.
    /// - On a `fresh` store the header is written (and read back) first.
    ///
    /// A power cut at any moment leaves the old state or the new one,
    /// and never touches another pad.
    pub fn save_new(
        &mut self,
        addr: [u8; 6],
        key: [u8; 16],
        key_type: u8,
    ) -> Result<Saved, SaveError<F::Error>> {
        let known = self.find(addr);
        if let Some(i) = known {
            let slot = self.live[i].slot;
            if let Some(Rec::Key(old)) = read_block(&mut self.b, at(self.active, slot))?.parse()
                && old.key == key
                && old.key_type == key_type
            {
                return Ok(Saved::Unchanged);
            }
        } else if self.n >= MAX_PADS {
            return Err(SaveError::Full);
        }
        if self.free_slots() == 0 {
            return Err(SaveError::NoRoom);
        }
        let slot = self.append(&Block::record(KIND_KEY, key_type, addr, key))?;
        if let Some(i) = known {
            self.remove_at(i);
        }
        self.live[self.n] = Live { addr, slot };
        self.n += 1;
        // The new record is safe in flash: now the old one may go.
        self.wipe(Some(addr), slot)?;
        Ok(match known {
            Some(_) => Saved::Replaced,
            None => Saved::New { evicted: None },
        })
    }

    /// How many gamepads are saved.
    pub fn count(&self) -> usize {
        self.n
    }

    /// Free record slots in the active area.
    pub fn free_slots(&self) -> u32 {
        SLOTS + 1 - self.next
    }

    /// Hands every saved pairing to `f`, the one saved longest ago first.
    /// The key is read from flash into a local copy only for the call.
    pub fn load(&mut self, mut f: impl FnMut(&Pairing)) -> Result<(), Error<F::Error>> {
        for i in 0..self.n {
            let slot = self.live[i].slot;
            if let Some(Rec::Key(p)) = read_block(&mut self.b, at(self.active, slot))?.parse() {
                f(&p);
            }
        }
        Ok(())
    }

    /// Saves a pairing. Appends one record and wipes the older record of
    /// the same address. Never erases.
    pub fn save(&mut self, p: &Pairing) -> Result<Saved, Error<F::Error>> {
        let known = self.find(p.addr);
        if let Some(i) = known {
            let slot = self.live[i].slot;
            if let Some(Rec::Key(old)) = read_block(&mut self.b, at(self.active, slot))?.parse()
                && old == *p
            {
                return Ok(Saved::Unchanged);
            }
        }
        let slot = self.append(&Block::record(KIND_KEY, p.key_type, p.addr, p.key))?;
        let mut evicted = None;
        if let Some(i) = known {
            self.remove_at(i);
        } else if self.n == MAX_PADS {
            evicted = Some(self.live[0].addr);
            self.remove_at(0);
        }
        self.live[self.n] = Live { addr: p.addr, slot };
        self.n += 1;
        // The new record is safe in flash: now the old one may go.
        self.wipe(Some(p.addr), slot)?;
        if let Some(e) = evicted {
            self.forget_in_flash(e, KIND_FORGET)?;
        }
        Ok(match known {
            Some(_) => Saved::Replaced,
            None => Saved::New { evicted },
        })
    }

    /// Forgets one gamepad: a "forget" record, then its old records are
    /// wiped. False if it was not saved (nothing written).
    pub fn forget(&mut self, addr: [u8; 6]) -> Result<bool, Error<F::Error>> {
        let Some(i) = self.find(addr) else {
            return Ok(false);
        };
        self.remove_at(i);
        self.forget_in_flash(addr, KIND_FORGET)?;
        Ok(true)
    }

    /// Forgets every gamepad. Returns how many were saved.
    pub fn forget_all(&mut self) -> Result<usize, Error<F::Error>> {
        let had = self.n;
        if had == 0 {
            return Ok(0);
        }
        self.n = 0;
        self.forget_in_flash([0; 6], KIND_FORGET_ALL)?;
        Ok(had)
    }

    /// Gives the flash back (tests, or a firmware that stops using it).
    pub fn into_flash(self) -> F {
        self.b.into_inner()
    }

    // ------------------------------------------------------------ inside

    fn find(&self, addr: [u8; 6]) -> Option<usize> {
        self.live[..self.n].iter().position(|l| l.addr == addr)
    }

    fn remove_at(&mut self, i: usize) {
        self.live.copy_within(i + 1..self.n, i);
        self.n -= 1;
    }

    /// The marker first (if there is room: the wipe alone also forgets),
    /// then the wipe of every older key record it covers. The wipe runs
    /// even when the marker could not be written: the caller has already
    /// told the owner the pairing is gone, so it must not come back at
    /// the next start. The marker's error is still reported.
    fn forget_in_flash(&mut self, addr: [u8; 6], kind: u8) -> Result<(), Error<F::Error>> {
        let marker = match self.append(&Block::record(kind, 0, addr, [0; 16])) {
            Ok(_) | Err(Error::Full) => Ok(()),
            Err(e) => Err(e),
        };
        let all = kind == KIND_FORGET_ALL;
        self.wipe(if all { None } else { Some(addr) }, self.next)?;
        marker
    }

    /// Overwrites with zeros every valid key record before slot `before`
    /// of `addr` (or of every address). A zeroed record has no magic and
    /// no check sum: it is ignored from then on. A wipe cut half way
    /// leaves a record whose check sum fails: also ignored.
    fn wipe(&mut self, addr: Option<[u8; 6]>, before: u32) -> Result<(), Error<F::Error>> {
        // A local block, so the zeros sit at an aligned address.
        let zero = Block::ZERO;
        for slot in 1..before.min(SLOTS + 1) {
            let r = at(self.active, slot);
            if let Some(Rec::Key(p)) = read_block(&mut self.b, r)?.parse()
                && addr.is_none_or(|a| a == p.addr)
            {
                self.b.write(r, &zero.0)?;
            }
        }
        Ok(())
    }

    /// Writes `blk` into the next free slot and reads it back. On a
    /// mismatch the next slot is tried (a few times).
    /// On a `fresh` store (only `attach` makes one) the header is written
    /// and read back first. Over a cut first header this writes the same
    /// bytes again: every 0 bit already there is a 0 bit of the header
    /// (`cut_first_header` checked it), so the result is the header. A
    /// cut here leaves a cut first header again: nothing saved, nothing
    /// lost.
    fn append(&mut self, blk: &Block) -> Result<u32, Error<F::Error>> {
        if self.next > SLOTS {
            return Err(Error::Full);
        }
        if self.fresh {
            write_verified(
                &mut self.b,
                at(self.active, 0),
                &Block::header(self.generation),
            )?;
            self.fresh = false;
        }
        let mut tries = 0;
        while self.next <= SLOTS {
            let slot = self.next;
            self.next += 1;
            let r = at(self.active, slot);
            if !read_block(&mut self.b, r)?.is_blank() {
                continue;
            }
            self.b.write(r, &blk.0)?;
            if read_block(&mut self.b, r)? == *blk {
                return Ok(slot);
            }
            tries += 1;
            if tries >= WRITE_TRIES {
                return Err(Error::Verify);
            }
        }
        Err(Error::Full)
    }

    /// Reads the active area: which gamepads are saved, and where the
    /// free slots start.
    fn scan(&mut self, rep: &mut Opened) -> Result<(), Error<F::Error>> {
        let sc = self.read_live()?;
        rep.damaged += sc.damaged;
        rep.dropped += sc.dropped;
        // A gamepad dropped for room must not come back later (when a
        // newer one is forgotten): wipe its records now. Start-up only.
        for d in sc.dropped_addrs.into_iter().flatten() {
            if self.find(d).is_none() {
                self.wipe(Some(d), self.next)?;
            }
        }
        Ok(())
    }

    /// The reading part of `scan`: fills `live`, `n` and `next`. Never
    /// writes.
    fn read_live(&mut self) -> Result<Scanned, Error<F::Error>> {
        let mut sc = Scanned {
            damaged: 0,
            dropped: 0,
            dropped_addrs: [None; MAX_PADS],
        };
        let mut last_used = 0;
        for slot in 1..=SLOTS {
            let blk = read_block(&mut self.b, at(self.active, slot))?;
            if blk.is_blank() {
                continue;
            }
            last_used = slot;
            match blk.parse() {
                Some(Rec::Key(p)) => {
                    if let Some(i) = self.find(p.addr) {
                        self.remove_at(i);
                    }
                    if self.n == MAX_PADS {
                        let old = self.live[0].addr;
                        self.remove_at(0);
                        sc.dropped += 1;
                        if let Some(d) = sc.dropped_addrs.iter_mut().find(|d| d.is_none()) {
                            *d = Some(old);
                        }
                    }
                    self.live[self.n] = Live { addr: p.addr, slot };
                    self.n += 1;
                }
                Some(Rec::Forget(a)) => {
                    if let Some(i) = self.find(a) {
                        self.remove_at(i);
                    }
                }
                Some(Rec::ForgetAll) => self.n = 0,
                None if blk.is_zero() => {}
                None => sc.damaged += 1,
            }
        }
        self.next = last_used + 1;
        Ok(sc)
    }

    /// Whether `addr` would be saved if there were no limit of
    /// [`MAX_PADS`]: its last key record is not followed by a forget of
    /// it or a forget-all. Only reads.
    fn alive_without_limit(&mut self, addr: [u8; 6]) -> Result<bool, Error<F::Error>> {
        let mut alive = false;
        for slot in 1..=SLOTS {
            match read_block(&mut self.b, at(self.active, slot))?.parse() {
                Some(Rec::Key(p)) if p.addr == addr => alive = true,
                Some(Rec::Forget(a)) if a == addr => alive = false,
                Some(Rec::ForgetAll) => alive = false,
                _ => {}
            }
        }
        Ok(alive)
    }

    /// Copies the live records into the spare area, writes its header
    /// last, then erases the old area. Start-up only.
    fn compact(&mut self) -> Result<(), Error<F::Error>> {
        let from = self.active;
        let to = 1 - from;
        self.b.erase(at(to, 0))?;
        let mut slot = 1;
        let mut kept = 0;
        for i in 0..self.n {
            let blk = read_block(&mut self.b, at(from, self.live[i].slot))?;
            if !matches!(blk.parse(), Some(Rec::Key(_))) {
                continue;
            }
            write_verified(&mut self.b, at(to, slot), &blk)?;
            self.live[kept] = Live {
                addr: self.live[i].addr,
                slot,
            };
            kept += 1;
            slot += 1;
        }
        self.n = kept;
        let generation = self.generation.wrapping_add(1);
        write_verified(&mut self.b, at(to, 0), &Block::header(generation))?;
        self.active = to;
        self.generation = generation;
        self.next = slot;
        self.b.erase(at(from, 0))
    }
}

fn read_block<F: Flash>(b: &mut Bounded<F>, r: u32) -> Result<Block, Error<F::Error>> {
    let mut blk = Block::BLANK;
    b.read(r, &mut blk.0)?;
    Ok(blk)
}

fn write_verified<F: Flash>(
    b: &mut Bounded<F>,
    r: u32,
    blk: &Block,
) -> Result<(), Error<F::Error>> {
    b.write(r, &blk.0)?;
    if read_block(b, r)? == *blk {
        Ok(())
    } else {
        Err(Error::Verify)
    }
}

fn area_blank<F: Flash>(b: &mut Bounded<F>, area: u32) -> Result<bool, Error<F::Error>> {
    for slot in 0..=SLOTS {
        if !read_block(b, at(area, slot))?.is_blank() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Area 0 holds nothing but the start of our very first header, cut by a
/// power loss: every 0 bit in it is also 0 in that header, and the rest
/// of the area is blank.
fn cut_first_header<F: Flash>(b: &mut Bounded<F>) -> Result<bool, Error<F::Error>> {
    let h = Block::header(1);
    let got = read_block(b, at(0, 0))?;
    if got.0.iter().zip(h.0.iter()).any(|(g, w)| !g & w != 0) {
        return Ok(false);
    }
    for slot in 1..=SLOTS {
        if !read_block(b, at(0, slot))?.is_blank() {
            return Ok(false);
        }
    }
    Ok(true)
}
