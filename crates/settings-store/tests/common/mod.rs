//! A pretend flash for the tests: flash rules (erase sets 0xFF, a write
//! only clears bits), and a power cut after any byte of a write or any
//! word of an erase, if asked. Plain std: tests only.

#![allow(dead_code)]

use board_api::link::SECTOR;
use board_api::{Store, StoreError};

/// The store the tests use: 4 sectors, the settings in the middle two,
/// so a write outside the area would show.
pub const STORE_SIZE: u32 = 4 * SECTOR;
pub const BASE: u32 = SECTOR;

/// A small seeded random source (xorshift64*), so every run is the same.
#[derive(Clone)]
pub struct Rng(pub u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    pub fn chance(&mut self, per_mille: u64) -> bool {
        self.below(1000) < per_mille
    }
}

pub struct CutFlash {
    pub bytes: Vec<u8>,
    /// Bytes (of writes) and words (of erases) that may still complete
    /// before the power goes. `None`: the power stays.
    pub budget: Option<u64>,
    /// The power went: every call fails until [`CutFlash::power_on`].
    pub dead: bool,
    /// Bytes and words done so far.
    pub units: u64,
    /// Erases started, per sector of the store.
    pub erases: Vec<u32>,
    /// Calls to write.
    pub writes: u64,
    /// At the cut, the byte or word being changed is left half done
    /// (some of its bits changed, others not), from this random source.
    pub partial: Option<Rng>,
    /// Where writes and erases may go; anything else is a test failure.
    pub allowed: core::ops::Range<u32>,
}

impl CutFlash {
    pub fn blank() -> CutFlash {
        CutFlash::from(vec![0xFF; STORE_SIZE as usize])
    }

    pub fn from(bytes: Vec<u8>) -> CutFlash {
        let sectors = bytes.len() / SECTOR as usize;
        CutFlash {
            bytes,
            budget: None,
            dead: false,
            units: 0,
            erases: vec![0; sectors],
            writes: 0,
            partial: None,
            allowed: BASE..BASE + settings_store::AREA,
        }
    }

    /// The power comes back.
    pub fn power_on(&mut self) {
        self.dead = false;
        self.budget = None;
    }

    /// Whether one more unit may complete; if not, the power goes now.
    fn tick(&mut self) -> bool {
        match &mut self.budget {
            Some(0) => {
                self.dead = true;
                false
            }
            Some(n) => {
                *n -= 1;
                self.units += 1;
                true
            }
            None => {
                self.units += 1;
                true
            }
        }
    }

    fn check(&self, offset: u32, len: usize) -> Result<core::ops::Range<usize>, StoreError> {
        let end = (offset as usize)
            .checked_add(len)
            .ok_or(StoreError::OutOfRange)?;
        if end > self.bytes.len() {
            return Err(StoreError::OutOfRange);
        }
        Ok(offset as usize..end)
    }
}

impl Store for CutFlash {
    fn size(&self) -> u32 {
        self.bytes.len() as u32
    }

    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), StoreError> {
        if self.dead {
            return Err(StoreError::Io);
        }
        let r = self.check(offset, buf.len())?;
        buf.copy_from_slice(&self.bytes[r]);
        Ok(())
    }

    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), StoreError> {
        if self.dead {
            return Err(StoreError::Io);
        }
        if !offset.is_multiple_of(4) || !data.len().is_multiple_of(4) {
            return Err(StoreError::OutOfRange);
        }
        let r = self.check(offset, data.len())?;
        assert!(
            self.allowed.contains(&offset) && self.allowed.contains(&(r.end as u32 - 1)),
            "a write outside the settings area at {offset:#x}"
        );
        self.writes += 1;
        for (i, &d) in r.zip(data) {
            if !self.tick() {
                if let Some(rng) = &mut self.partial {
                    // Some of the bits to clear were cleared.
                    self.bytes[i] &= d | rng.next() as u8;
                }
                return Err(StoreError::Io);
            }
            self.bytes[i] &= d;
        }
        Ok(())
    }

    fn erase_sector(&mut self, offset: u32) -> Result<(), StoreError> {
        if self.dead {
            return Err(StoreError::Io);
        }
        if !offset.is_multiple_of(SECTOR) {
            return Err(StoreError::OutOfRange);
        }
        let r = self.check(offset, SECTOR as usize)?;
        assert!(
            self.allowed.contains(&offset),
            "an erase outside the settings area at {offset:#x}"
        );
        self.erases[offset as usize / SECTOR as usize] += 1;
        for w in r.step_by(4) {
            if !self.tick() {
                if let Some(rng) = &mut self.partial {
                    // Some bits of this word were set again.
                    let m = rng.next().to_le_bytes();
                    for (b, k) in self.bytes[w..w + 4].iter_mut().zip(m) {
                        *b |= k;
                    }
                }
                return Err(StoreError::Io);
            }
            self.bytes[w..w + 4].fill(0xFF);
        }
        Ok(())
    }
}
