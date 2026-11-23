//! Reading the saved pairings without ever writing.
//!
//! [`Store::open`](crate::Store::open) always writes (at least the header
//! again, with the same bytes) and may erase, repair or compact. A
//! firmware that must leave the pairing area exactly as it is uses
//! [`read_pairings`] instead: it reaches the flash only through
//! [`ReadFlash`], a trait with a read and nothing else, so no write or
//! erase can happen however the code below goes.
//!
//! It finds the same pairings as `Store::open` followed by
//! [`Store::load`](crate::Store::load), in the same order (the tests
//! check this against random histories). What `open` would repair or
//! clean up is left as it is and only counted.

use core::fmt;

use crate::crc::crc32_more;
use crate::region::{Region, USED};
use crate::store::{Block, Error, MAX_PADS, Pairing, Rec, SLOTS, at};

/// Flash that can only be read. Offsets are absolute (from the start of
/// the chip), like [`Flash::read`](crate::Flash::read).
pub trait ReadFlash {
    type Error: fmt::Debug;
    /// Reads `buf.len()` bytes at `offset`.
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), Self::Error>;
}

impl<T: ReadFlash + ?Sized> ReadFlash for &mut T {
    type Error = T::Error;
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), Self::Error> {
        (**self).read(offset, buf)
    }
}

/// What [`read_pairings`] found. Holds no address and no key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadReport {
    /// A valid store header was found.
    pub found: bool,
    /// Both sectors are blank (never used). Then `found` is false too.
    pub blank: bool,
    /// The active area (0 or 1), when `found`.
    pub area: u8,
    /// The active area's generation, when `found`.
    pub generation: u32,
    /// Pairings handed to the caller.
    pub count: usize,
    /// Records ignored because their check sum did not match.
    pub damaged: u32,
    /// Older gamepads left out because more than [`MAX_PADS`] were found.
    pub dropped: u32,
    /// CRC-32 (zip kind) of the whole 8 KB, both sectors: proof that the
    /// area did not change, without showing a byte of it.
    pub crc: u32,
}

/// The absolute address of `rel`, if it lies inside the region.
fn inside<E>(region: Region, rel: u32, n: usize) -> Result<u32, Error<E>> {
    let n = u32::try_from(n).map_err(|_| Error::OutOfBounds)?;
    match rel.checked_add(n) {
        Some(end) if end <= USED => region.base().checked_add(rel).ok_or(Error::OutOfBounds),
        _ => Err(Error::OutOfBounds),
    }
}

fn read_at<R: ReadFlash>(f: &mut R, region: Region, rel: u32) -> Result<Block, Error<R::Error>> {
    let mut b = Block::BLANK;
    let a = inside(region, rel, b.0.len())?;
    f.read(a, &mut b.0).map_err(Error::Flash)?;
    Ok(b)
}

/// Reads the saved pairings and hands each to `f`, the one saved
/// longest ago first (the order pad-proto's `Host::load_key` wants).
/// Only reads: the flash is reached through [`ReadFlash`] alone.
///
/// About 260 reads of 32 bytes (each block of both sectors once for the
/// check sum, then the active area's header and live records again).
/// Uses about 200 bytes of stack besides `f`; each key exists only in a
/// local copy for the call.
pub fn read_pairings<R: ReadFlash>(
    flash: &mut R,
    region: Region,
    mut f: impl FnMut(&Pairing),
) -> Result<ReadReport, Error<R::Error>> {
    let mut rep = ReadReport::default();
    // The check sum, and whether each area is blank.
    let mut crc = 0;
    let mut blank = [true; 2];
    for area in 0..2_u32 {
        for slot in 0..=SLOTS {
            let b = read_at(flash, region, at(area, slot))?;
            crc = crc32_more(crc, &b.0);
            blank[area as usize] &= b.is_blank();
        }
    }
    rep.crc = crc;
    let g0 = read_at(flash, region, at(0, 0))?.generation();
    let g1 = read_at(flash, region, at(1, 0))?.generation();
    // The same choice as Store::open.
    let (active, generation) = match (g0, g1) {
        (Some(a), Some(c)) if c > a => (1, c),
        (Some(a), _) => (0, a),
        (None, Some(c)) => (1, c),
        (None, None) => {
            rep.blank = blank[0] && blank[1];
            return Ok(rep);
        }
    };
    rep.found = true;
    rep.area = active as u8;
    rep.generation = generation;
    // The same scan as Store::open: which slot holds each live pairing.
    let mut live = [([0_u8; 6], 0_u32); MAX_PADS];
    let mut n = 0;
    for slot in 1..=SLOTS {
        let blk = read_at(flash, region, at(active, slot))?;
        if blk.is_blank() {
            continue;
        }
        match blk.parse() {
            Some(Rec::Key(p)) => {
                if let Some(i) = live[..n].iter().position(|l| l.0 == p.addr) {
                    live.copy_within(i + 1..n, i);
                    n -= 1;
                }
                if n == MAX_PADS {
                    live.copy_within(1..n, 0);
                    n -= 1;
                    rep.dropped += 1;
                }
                live[n] = (p.addr, slot);
                n += 1;
            }
            Some(Rec::Forget(a)) => {
                if let Some(i) = live[..n].iter().position(|l| l.0 == a) {
                    live.copy_within(i + 1..n, i);
                    n -= 1;
                }
            }
            Some(Rec::ForgetAll) => n = 0,
            None if blk.is_zero() => {}
            None => rep.damaged += 1,
        }
    }
    for &(_, slot) in &live[..n] {
        if let Some(Rec::Key(p)) = read_at(flash, region, at(active, slot))?.parse() {
            f(&p);
            rep.count += 1;
        }
    }
    Ok(rep)
}
