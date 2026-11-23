//! A fake flash for the tests, made to behave like the real one:
//!
//! - writing can only clear bits (new = old AND data);
//! - erasing works on whole 4096-byte sectors only;
//! - offsets and lengths of writes must be multiples of 4;
//! - a "power loss" can stop any write after any byte, and any erase
//!   after any 256-byte piece. The byte or piece being worked on when the
//!   power goes is left half done. After that every call fails until the
//!   test "switches the power on again" ([`Fake::revive`]).
//!
//! Every write and erase is recorded, so tests can check that nothing
//! outside the store's two sectors was ever touched.

use crate::store::Flash;

/// The fake chip: the first 64 KB of a 4 MB flash.
pub const SIZE: usize = 0x1_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FakeError {
    PowerLost,
    Dead,
    NotAligned,
    OutsideChip,
    Refused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Write,
    Erase,
}

#[derive(Clone)]
pub struct Fake {
    pub mem: Vec<u8>,
    /// Steps left before the power goes (None: never). A step is one
    /// byte written or one 256-byte piece erased.
    pub budget: Option<u64>,
    /// Steps taken so far.
    pub steps: u64,
    pub dead: bool,
    /// Erase the pieces of a sector from the end backwards (real chips
    /// do not promise an order).
    pub erase_backwards: bool,
    /// Every write and erase: (kind, absolute offset, length).
    pub ops: Vec<(Op, u32, u32)>,
    /// Refuse the write call with this number (counted from 0 in
    /// `ops`), changing nothing: a flash error without a power cut.
    pub refuse_write: Option<usize>,
}

impl Fake {
    /// A chip whose bootloader, table and program areas hold a pattern
    /// and whose data partitions (0x9000 to 0x10000) are blank.
    pub fn new() -> Self {
        let mut mem = vec![0xA5; SIZE];
        mem[0x9000..0x1_0000].fill(0xFF);
        Self {
            mem,
            budget: None,
            steps: 0,
            dead: false,
            erase_backwards: false,
            ops: Vec::new(),
            refuse_write: None,
        }
    }

    /// The power comes back: no more cuts, calls work again.
    pub fn revive(&mut self) {
        self.dead = false;
        self.budget = None;
    }

    fn step(&mut self) -> bool {
        match self.budget {
            Some(0) => false,
            Some(n) => {
                self.budget = Some(n - 1);
                self.steps += 1;
                true
            }
            None => {
                self.steps += 1;
                true
            }
        }
    }

    pub fn writes(&self) -> usize {
        self.ops.iter().filter(|o| o.0 == Op::Write).count()
    }

    pub fn erases(&self) -> usize {
        self.ops.iter().filter(|o| o.0 == Op::Erase).count()
    }
}

impl Flash for Fake {
    type Error = FakeError;

    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), FakeError> {
        if self.dead {
            return Err(FakeError::Dead);
        }
        let o = offset as usize;
        let src = self
            .mem
            .get(o..o + buf.len())
            .ok_or(FakeError::OutsideChip)?;
        buf.copy_from_slice(src);
        Ok(())
    }

    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), FakeError> {
        if self.dead {
            return Err(FakeError::Dead);
        }
        if !offset.is_multiple_of(4)
            || !data.len().is_multiple_of(4)
            || !(data.as_ptr() as usize).is_multiple_of(4)
        {
            return Err(FakeError::NotAligned);
        }
        let o = offset as usize;
        if o + data.len() > self.mem.len() {
            return Err(FakeError::OutsideChip);
        }
        if self.refuse_write == Some(self.writes()) {
            self.refuse_write = None;
            return Err(FakeError::Refused);
        }
        self.ops.push((Op::Write, offset, data.len() as u32));
        for (i, &d) in data.iter().enumerate() {
            if !self.step() {
                // Half a byte: only some of its bits were cleared.
                self.mem[o + i] &= d | 0xF0;
                self.dead = true;
                return Err(FakeError::PowerLost);
            }
            self.mem[o + i] &= d;
        }
        Ok(())
    }

    fn erase_sector(&mut self, offset: u32) -> Result<(), FakeError> {
        if self.dead {
            return Err(FakeError::Dead);
        }
        if !offset.is_multiple_of(4096) {
            return Err(FakeError::NotAligned);
        }
        let o = offset as usize;
        if o + 4096 > self.mem.len() {
            return Err(FakeError::OutsideChip);
        }
        self.ops.push((Op::Erase, offset, 4096));
        for k in 0..16 {
            let piece = if self.erase_backwards { 15 - k } else { k };
            let p = o + piece * 256;
            if !self.step() {
                // Half erased: some bits went back to 1, others not.
                for b in &mut self.mem[p..p + 256] {
                    *b |= 0x0F;
                }
                self.dead = true;
                return Err(FakeError::PowerLost);
            }
            self.mem[p..p + 256].fill(0xFF);
        }
        Ok(())
    }
}
