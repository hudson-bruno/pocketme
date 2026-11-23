//! Pretend hardware for the loader tests: a pair of slots that enforces
//! the chip's word rule, a card with one file that can lie, fail or
//! change, a small random number maker, and a reader for the linked game
//! (ELF) files. Each test file uses only some of it.
#![allow(dead_code)]

use std::path::PathBuf;

use board_api::{DirEntry, Files, FsError};
use game_api::{Crc32, FlashPartHeader, GameHeader, MAGIC, MAGIC_FLASH, layout};
use game_loader::{FlashHeld, GameSlots, Slots};

/// What a byte of a slot holds before any load: rubbish, so that a load
/// that forgets to write something is caught.
pub const RUBBISH: u8 = 0xA5;

/// How the fake maps the spill (code offsets past the code slot) onto
/// the data slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mirror {
    /// As `game_api::layout` says the ESP32 does (seen on the CYD,
    /// 2026-11-22): the first spill word is the data slot's LAST word, and so
    /// on backwards, word by word; the bytes inside a word keep their
    /// order.
    Reversed,
    /// A wrong board: the first spill word is the data slot's FIRST word.
    /// Then code past the slot and data meet at once.
    Straight,
}

/// Where a code offset lives in the fake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// In the code slot, at this byte.
    Code(usize),
    /// In the spill, which is this byte of the data slot.
    Spill(usize),
}

/// A pair of slots in plain PC memory.
///
/// The code slot behaves like the chip's: only whole, aligned 32-bit
/// words. Any other access panics, as the chip would stop. Every access
/// outside a slot panics too.
///
/// Code offsets from `code_size` on are the spill. As on the chip they
/// are stored IN the data slot, at the mirrored place ([`Mirror`]), so a
/// test sees real overlap between code past the slot and data.
pub struct FakeSlots {
    pub slots: Slots,
    pub code: Vec<u8>,
    pub data: Vec<u8>,
    /// Calls of each kind, to see what a refused load touched.
    pub code_writes: usize,
    pub code_reads: usize,
    pub data_writes: usize,
    pub zero_calls: usize,
    /// When set, the word written at this code offset reads back wrong
    /// (a broken slot).
    pub broken_at: Option<u32>,
    /// How the spill lies in the data slot.
    pub mirror: Mirror,
    /// When set (the default), data or zeroes written over a byte that
    /// this load's code put into the data slot (through the spill) panic:
    /// the chip would run broken code. A load starts with the code word
    /// at offset 0, which forgets the last load's code.
    pub strict: bool,
    /// When set, the spill word at this code offset is changed right
    /// after the zeroing (something wrote over the code at the end of a
    /// load).
    pub spoil_after_zero: Option<u32>,
    /// One flag per data byte: the code side wrote it in this load.
    code_in_data: Vec<bool>,
    /// The flash part this pretend firmware holds: where it says it is
    /// (only the addresses and sizes are used) and its two halves' bytes.
    /// `None`: a firmware without flash parts.
    pub holds: Option<(FlashPartHeader, Vec<u8>, Vec<u8>)>,
    /// Calls of `flash_part`.
    pub flash_asks: usize,
}

impl FakeSlots {
    pub fn new(slots: Slots) -> Self {
        FakeSlots {
            slots,
            code: vec![RUBBISH; slots.code_size as usize],
            data: vec![RUBBISH; slots.data_size as usize],
            code_writes: 0,
            code_reads: 0,
            data_writes: 0,
            zero_calls: 0,
            broken_at: None,
            mirror: Mirror::Reversed,
            strict: true,
            spoil_after_zero: None,
            code_in_data: vec![false; slots.data_size as usize],
            holds: None,
            flash_asks: 0,
        }
    }

    pub fn console() -> Self {
        Self::new(Slots::CONSOLE)
    }

    /// Calls that changed memory.
    pub fn writes(&self) -> usize {
        self.code_writes + self.data_writes + self.zero_calls
    }

    /// Calls of any kind.
    pub fn touches(&self) -> usize {
        self.writes() + self.code_reads
    }

    /// Bytes of the data slot that this load's code wrote (through the
    /// spill).
    pub fn code_bytes_in_data(&self) -> usize {
        self.code_in_data.iter().filter(|&&c| c).count()
    }

    /// The spill bytes the fake can hold: the spill, but never more than
    /// the data slot (it is the data slot).
    fn spill_room(&self) -> u32 {
        self.slots.spill_size.min(self.slots.data_size)
    }

    /// The byte at a chip address, as a test sees it (not the loader).
    /// An address in the spill reads the data slot through the mirror.
    pub fn byte_at(&self, addr: u32) -> Option<u8> {
        let s = self.slots;
        if let Some(o) = addr.checked_sub(s.code_addr).filter(|&o| o < s.code_size) {
            return Some(self.code[o as usize]);
        }
        if let Some(o) = addr
            .checked_sub(s.code_addr)
            .filter(|&o| o >= s.code_size && o - s.code_size < self.spill_room())
        {
            let Place::Spill(at) = self.place(o & !3) else {
                unreachable!()
            };
            return Some(self.data[at + (o & 3) as usize]);
        }
        if let Some(o) = addr.checked_sub(s.data_addr).filter(|&o| o < s.data_size) {
            return Some(self.data[o as usize]);
        }
        None
    }

    /// Where the code word at `offset` lives. Panics as the chip would
    /// stop, or as a loader bug deserves.
    fn place(&self, offset: u32) -> Place {
        assert!(
            offset.is_multiple_of(4),
            "byte access to the code slot at offset {offset}: the chip would stop"
        );
        let code_size = self.slots.code_size;
        if u64::from(offset) + 4 <= u64::from(code_size) {
            return Place::Code(offset as usize);
        }
        let k = offset.wrapping_sub(code_size);
        assert!(
            offset >= code_size && u64::from(k) + 4 <= u64::from(self.spill_room()),
            "code word at offset {offset} is outside the code slot and the spill"
        );
        Place::Spill(match self.mirror {
            Mirror::Reversed => (self.slots.data_size - 4 - k) as usize,
            Mirror::Straight => k as usize,
        })
    }

    fn data_range(&self, offset: u32, len: usize) -> std::ops::Range<usize> {
        let at = offset as usize;
        assert!(
            at.checked_add(len)
                .is_some_and(|end| end <= self.data.len()),
            "data access at offset {offset}, {len} bytes, is outside the data slot"
        );
        let r = at..at + len;
        if self.strict
            && let Some(i) = self.code_in_data[r.clone()].iter().position(|&c| c)
        {
            panic!(
                "data over code at data offset {}: the chip would run broken code",
                at + i
            );
        }
        r
    }
}

impl GameSlots for FakeSlots {
    fn write_code_word(&mut self, offset: u32, word: u32) {
        let place = self.place(offset);
        if offset == 0 {
            // A new load: the last load's code is gone.
            self.code_in_data.fill(false);
        }
        self.code_writes += 1;
        let word = if self.broken_at == Some(offset) {
            word ^ 0x10
        } else {
            word
        };
        match place {
            Place::Code(at) => self.code[at..at + 4].copy_from_slice(&word.to_le_bytes()),
            Place::Spill(at) => {
                self.data[at..at + 4].copy_from_slice(&word.to_le_bytes());
                self.code_in_data[at..at + 4].fill(true);
            }
        }
    }

    fn read_code_word(&mut self, offset: u32) -> u32 {
        let place = self.place(offset);
        self.code_reads += 1;
        let (mem, at) = match place {
            Place::Code(at) => (&self.code, at),
            Place::Spill(at) => (&self.data, at),
        };
        u32::from_le_bytes(mem[at..at + 4].try_into().unwrap())
    }

    fn write_data(&mut self, offset: u32, bytes: &[u8]) {
        let r = self.data_range(offset, bytes.len());
        self.data_writes += 1;
        self.data[r].copy_from_slice(bytes);
    }

    fn zero_data(&mut self, offset: u32, len: u32) {
        let r = self.data_range(offset, len as usize);
        self.zero_calls += 1;
        self.data[r].fill(0);
        if let Some(o) = self.spoil_after_zero {
            let Place::Spill(at) = self.place(o) else {
                panic!("spoil_after_zero must name a spill offset, not {o}")
            };
            self.data[at] ^= 0x01;
        }
    }

    /// As the CYD does it: the part at exactly these addresses and sizes,
    /// then the CRC-32s of the bytes it holds.
    fn flash_part(&mut self, part: &FlashPartHeader) -> FlashHeld {
        self.flash_asks += 1;
        let Some((at, code, ro)) = &self.holds else {
            return FlashHeld::Missing;
        };
        let same_place = at.code_addr == part.code_addr
            && code.len() as u32 == part.code_size
            && at.ro_addr == part.ro_addr
            && ro.len() as u32 == part.ro_size;
        if !same_place {
            return FlashHeld::Missing;
        }
        let (got_code, got_ro) = (crc_of(code), if ro.is_empty() { 0 } else { crc_of(ro) });
        if got_code == part.code_crc32 && got_ro == part.ro_crc32 {
            FlashHeld::Held
        } else {
            FlashHeld::Differs { got_code, got_ro }
        }
    }
}

/// CRC-32 of some bytes.
pub fn crc_of(bytes: &[u8]) -> u32 {
    let mut crc = Crc32::new();
    crc.update(bytes);
    crc.value()
}

/// The extension that names these two halves at the start of the
/// windows, as games/chip/pack.py writes it.
pub fn flash_ext(code: &[u8], ro: &[u8]) -> FlashPartHeader {
    FlashPartHeader {
        ext_size: FlashPartHeader::SIZE as u32,
        code_addr: layout::FLASH_CODE_ADDR,
        code_size: code.len() as u32,
        code_crc32: crc_of(code),
        ro_addr: layout::FLASH_RO_ADDR,
        ro_size: ro.len() as u32,
        ro_crc32: if ro.is_empty() { 0 } else { crc_of(ro) },
        reserved: 0,
    }
}

/// A whole "GAMF" file: the header (magic "GAMF") with the checksum over
/// bytes 0..32, the extension and the payload.
pub fn pack_flash(mut head: GameHeader, ext: &FlashPartHeader, payload: &[u8]) -> Vec<u8> {
    head.magic = MAGIC_FLASH;
    let mut crc = Crc32::new();
    crc.update(&head.to_bytes()[..GameHeader::CHECKED]);
    crc.update(&ext.to_bytes());
    crc.update(payload);
    head.crc32 = crc.value();
    let mut file = head.to_bytes().to_vec();
    file.extend_from_slice(&ext.to_bytes());
    file.extend_from_slice(payload);
    file
}

/// The flash halves of [`small_flash_game`].
pub fn small_flash_halves() -> (Vec<u8>, Vec<u8>) {
    (pattern(600, 9), pattern(200, 11))
}

/// [`small_game`]'s RAM part with a flash part of 600 code bytes and
/// 200 data bytes.
pub fn small_flash_game() -> Vec<u8> {
    let (code, ro) = small_flash_halves();
    pack_flash(
        header(1000, 300, 64),
        &flash_ext(&code, &ro),
        &pattern(1300, 3),
    )
}

/// Slots whose firmware holds [`small_flash_game`]'s part.
pub fn holding_slots() -> FakeSlots {
    let (code, ro) = small_flash_halves();
    let mut fake = FakeSlots::console();
    fake.holds = Some((flash_ext(&code, &ro), code, ro));
    fake
}

/// What a [`Card`] call was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    Size,
    Read {
        offset: u32,
        len: usize,
        gave: usize,
    },
}

/// A card with one file on it, which can misbehave on purpose.
pub struct Card {
    pub path: &'static str,
    pub bytes: Vec<u8>,
    /// When set, `size` says this instead of the real size.
    pub size_says: Option<u32>,
    /// When set, the call with this number (0 = the first, counting
    /// `size` and `read_at`) fails with `FsError::Io`.
    pub fail_call: Option<usize>,
    /// When set, after this many `read_at` calls the file becomes the
    /// other bytes (it grew, shrank, or was swapped).
    pub change: Option<(usize, Vec<u8>)>,
    /// When set, each read gives at most this many bytes (the contract
    /// allows less only at the end; the loader copes anyway).
    pub max_read: Option<usize>,
    /// When set, `read_at` claims to have read one byte more than asked
    /// (a broken board).
    pub overclaim: bool,
    pub calls: Vec<Call>,
}

pub const PATH: &str = "/CONSOLE/GAMES/TEST/GAME.BIN";

impl Card {
    pub fn new(bytes: Vec<u8>) -> Self {
        Card {
            path: PATH,
            bytes,
            size_says: None,
            fail_call: None,
            change: None,
            max_read: None,
            overclaim: false,
            calls: Vec::new(),
        }
    }

    pub fn reads(&self) -> usize {
        self.calls
            .iter()
            .filter(|c| matches!(c, Call::Read { .. }))
            .count()
    }

    pub fn bytes_read(&self) -> usize {
        self.calls
            .iter()
            .map(|c| match c {
                Call::Read { gave, .. } => *gave,
                Call::Size => 0,
            })
            .sum()
    }

    fn call(&mut self) -> Result<(), FsError> {
        let n = self.calls.len();
        if self.fail_call == Some(n) {
            // Still counted, so that "fail call n" means one call.
            self.calls.push(Call::Size);
            return Err(FsError::Io);
        }
        Ok(())
    }
}

impl Files for Card {
    fn list<F: FnMut(&DirEntry<'_>) -> bool>(
        &mut self,
        _dir: &str,
        _each: F,
    ) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }

    fn size(&mut self, path: &str) -> Result<u32, FsError> {
        self.call()?;
        self.calls.push(Call::Size);
        if path != self.path {
            return Err(FsError::NotFound);
        }
        Ok(self.size_says.unwrap_or(self.bytes.len() as u32))
    }

    fn read_at(&mut self, path: &str, offset: u32, buf: &mut [u8]) -> Result<usize, FsError> {
        self.call()?;
        if path != self.path {
            return Err(FsError::NotFound);
        }
        if let Some((after, _)) = &self.change
            && self.reads() == *after
        {
            let (_, new) = self.change.take().unwrap();
            self.bytes = new;
        }
        let start = (offset as usize).min(self.bytes.len());
        let mut n = buf.len().min(self.bytes.len() - start);
        if let Some(max) = self.max_read {
            n = n.min(max);
        }
        buf[..n].copy_from_slice(&self.bytes[start..start + n]);
        let gave = if self.overclaim { n + 1 } else { n };
        self.calls.push(Call::Read {
            offset,
            len: buf.len(),
            gave,
        });
        Ok(gave)
    }
}

/// A small, fixed random number maker (xorshift), so every run of the
/// tests damages files the same way.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    /// A number from 0 to `n - 1`.
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// A header for the console's slots with the given sizes. The checksum
/// is left at 0: [`pack`] fills it in.
pub fn header(code: u32, data: u32, bss: u32) -> GameHeader {
    GameHeader {
        magic: MAGIC,
        api_version: game_api::API_VERSION,
        code_addr: layout::CODE_SLOT_ADDR,
        code_size: code,
        data_addr: layout::DATA_SLOT_ADDR,
        data_size: data,
        bss_size: bss,
        entry: layout::CODE_SLOT_ADDR + 4,
        crc32: 0,
    }
}

/// A whole game file, as games/chip/pack.py writes it: the header with
/// the right checksum, then the payload. `payload` must be
/// `code_size + data_size` bytes.
pub fn pack(mut head: GameHeader, payload: &[u8]) -> Vec<u8> {
    let mut crc = Crc32::new();
    crc.update(&head.to_bytes()[..GameHeader::CHECKED]);
    crc.update(payload);
    head.crc32 = crc.value();
    let mut file = head.to_bytes().to_vec();
    file.extend_from_slice(payload);
    file
}

/// Bytes that differ from each other and from [`RUBBISH`] in most
/// places, so a byte in the wrong place is noticed.
pub fn pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed) ^ (i >> 8) as u8)
        .collect()
}

/// A small but complete game file: 1,000 bytes of code (not a multiple
/// of 512, so pieces fall across the code-to-data boundary), 300 of
/// data, 64 zeroed.
pub fn small_game() -> Vec<u8> {
    let head = header(1000, 300, 64);
    pack(head, &pattern(1300, 3))
}

/// The folder with the files games/build-chip.sh made: GAME_FILES_DIR
/// (check.sh sets it), or this crate's own build folder.
pub fn game_files_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("GAME_FILES_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/chip/chip-games/files")
}

/// The real game files, read from [`game_files_dir`]: (name, file, ELF).
/// Panics with what to do when they are missing.
pub fn real_games() -> Vec<(&'static str, Vec<u8>, Vec<u8>)> {
    let dir = game_files_dir();
    ["PADTEST", "STARDUST"]
        .into_iter()
        .map(|name| {
            let read = |ext: &str| {
                let path = dir.join(format!("{name}.{ext}"));
                std::fs::read(&path).unwrap_or_else(|e| {
                    panic!(
                        "{}: {e}. Build the game files first: \
                         GAMES_TARGET=$PWD/target/chip ../../games/build-chip.sh \
                         (in crates/game-loader), or run boards/desktop/check.sh",
                        path.display()
                    )
                })
            };
            (name, read("BIN"), read("elf"))
        })
        .collect()
}

/// One part of a linked game that takes memory: where it goes, and its
/// bytes (`None`: it is set to zero, not stored in the file).
pub struct Section {
    pub name: String,
    pub addr: u32,
    pub size: u32,
    pub bytes: Option<Vec<u8>>,
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(b[at..at + 2].try_into().unwrap())
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// The sections of a 32-bit little-endian ELF file that take memory, as
/// games/chip/check_load.py reads them.
pub fn elf_sections(elf: &[u8]) -> Vec<Section> {
    assert_eq!(&elf[..4], b"\x7fELF", "not an ELF file");
    assert_eq!((elf[4], elf[5]), (1, 1), "not 32-bit little-endian");
    const SHF_ALLOC: u32 = 2;
    const SHT_NOBITS: u32 = 8;
    let shoff = u32_at(elf, 0x20) as usize;
    let shentsize = u16_at(elf, 0x2E) as usize;
    let shnum = u16_at(elf, 0x30) as usize;
    let shstrndx = u16_at(elf, 0x32) as usize;
    let head = |i: usize| {
        let at = shoff + i * shentsize;
        (0..10).map(|k| u32_at(elf, at + 4 * k)).collect::<Vec<_>>()
    };
    let names = head(shstrndx)[4] as usize;
    (0..shnum)
        .map(head)
        .filter(|h| h[2] & SHF_ALLOC != 0 && h[5] != 0)
        .map(|h| {
            let name_at = names + h[0] as usize;
            let name_end = name_at + elf[name_at..].iter().position(|&b| b == 0).unwrap();
            let (off, size) = (h[4] as usize, h[5]);
            Section {
                name: String::from_utf8_lossy(&elf[name_at..name_end]).into_owned(),
                addr: h[3],
                size,
                bytes: (h[1] != SHT_NOBITS).then(|| elf[off..off + size as usize].to_vec()),
            }
        })
        .collect()
}

/// The ELF file's entry point (where `ENTRY(game_entry)` points).
pub fn elf_entry(elf: &[u8]) -> u32 {
    u32_at(elf, 0x18)
}
