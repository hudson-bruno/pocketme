//! The contract between the console and a native game.
//!
//! A game is a separate program. It is compiled on the PC for two fixed
//! places in the console's memory (the "slots" in [`layout`]), packed into
//! one file ([`GameHeader`] followed by the bytes for each slot), and copied
//! into those places by the console at run time. Nothing is patched while
//! loading: the game's code already contains the right addresses because it
//! was linked for exactly these slots.
//!
//! The two programs know nothing about each other's insides. Everything
//! that crosses the border is in this crate:
//!
//! * [`ConsoleApi`]: functions the console offers to the game,
//! * [`GameCallbacks`]: functions the game offers to the console,
//! * [`GameEntry`]: the one function in the game the console finds through
//!   the file header, which swaps the two tables.
//!
//! Only `repr(C)` structs, plain integers, raw pointers and `extern "C"`
//! functions may appear here. Rust's own types (`&str`, slices, `Option`,
//! ...) have no fixed layout, and the two sides may be built by different
//! compiler versions.

#![no_std]

/// Where a game lives in memory. Both linker scripts are generated from
/// these numbers (see each crate's `build.rs`), so this is the only place
/// they are written down.
pub mod layout {
    /// Start of the code slot.
    ///
    /// This is the chip's executable RAM (SRAM0, 0x4008_0000 to
    /// 0x4009_FFFF). The console's own RAM code sits at the bottom of it;
    /// the slot is the top 64 KB. Memory at these addresses can only be
    /// read and written as whole, aligned 32-bit words (measured on the
    /// board: a byte read stops the program), so a game may keep only
    /// machine code and its literal pools here. Strings, tables and
    /// variables go to the data slot.
    pub const CODE_SLOT_ADDR: u32 = 0x4009_0000;
    /// Size of the code slot in bytes.
    pub const CODE_SLOT_SIZE: u32 = 0x1_0000;

    /// Start of the data slot: ordinary RAM (the top of SRAM1), readable
    /// and writable a byte at a time. It holds the game's constants
    /// (`.rodata`), its initialised variables (`.data`) and its zeroed
    /// variables (`.bss`), in that order.
    ///
    /// The console's heap (which the radio needs) fills the same memory
    /// block from the bottom up to here, so this slot is paid for with
    /// heap.
    pub const DATA_SLOT_ADDR: u32 = 0x3FFF_4000;
    /// Size of the data slot in bytes.
    pub const DATA_SLOT_SIZE: u32 = 0xC000;

    /// The memory block the data slot is cut from, as esp-hal 1.2.2 names
    /// it (`dram2_seg` in its `memory.x`). What is left below the slot is
    /// the console's heap.
    pub const DRAM2_ADDR: u32 = 0x3FFE_7E30;
    /// End of that block (one past the last byte).
    pub const DRAM2_END: u32 = 0x4000_0000;
    /// Bytes left for the console's heap.
    pub const CONSOLE_HEAP_SIZE: u32 = DATA_SLOT_ADDR - DRAM2_ADDR;
}

/// Version of [`ConsoleApi`] and [`GameCallbacks`]. A game file carries the
/// version it was built for, and the console refuses any other.
pub const API_VERSION: u32 = 1;

/// Why a game asked the console to stop it. Passed to [`ConsoleApi::abort`].
pub mod abort_code {
    /// A Rust panic inside the game (the second argument is the source
    /// line, or 0 if unknown).
    pub const PANIC: u32 = 1;
    /// The game found the console's table too old or too small.
    pub const BAD_API: u32 = 2;
}

/// The functions the console offers. The console owns this table and hands
/// the game a pointer to it.
///
/// `size` and `version` come first and never move. New functions are only
/// ever added at the end, so a newer console can still run an older game:
/// the game checks that `size` covers every field it knows about.
#[repr(C)]
pub struct ConsoleApi {
    /// `size_of::<ConsoleApi>()` as the console was built.
    pub size: u32,
    /// [`API_VERSION`] as the console was built.
    pub version: u32,
    /// Prints `len` bytes of UTF-8 text at `text` as one log line.
    pub log: extern "C" fn(text: *const u8, len: u32),
    /// Hands the console a numbered result. The test game uses it to prove
    /// what it computed without needing number formatting of its own.
    pub report: extern "C" fn(id: u32, value: u32),
    /// Stops the game for good. Never returns: the console restarts the
    /// chip and remembers what happened. `code` is one of [`abort_code`].
    pub abort: extern "C" fn(code: u32, detail: u32) -> !,
    /// Returns `a + b`. It exists only to prove that arguments and a
    /// return value cross the border intact.
    pub echo_sum: extern "C" fn(a: u32, b: u32) -> u32,
    /// Microseconds since the console started (a 64-bit return value).
    pub time_us: extern "C" fn() -> u64,
}

/// The functions the game offers. The game owns this table (it lives in the
/// data slot) and returns a pointer to it from its entry function.
///
/// The console calls these from its own main loop, on its own stack. A game
/// never loops for ever: it does one frame's work in `update` and returns,
/// which is what lets the console keep the radio, the timing and the menu
/// alive in between.
#[repr(C)]
pub struct GameCallbacks {
    /// `size_of::<GameCallbacks>()` as the game was built.
    pub size: u32,
    /// Called once before the first frame. Returns 0 if the game can run.
    pub init: extern "C" fn() -> u32,
    /// Called once per frame with the frame number, starting at 0.
    pub update: extern "C" fn(frame: u32) -> u32,
    /// Called once when the console ends the game normally.
    pub exit: extern "C" fn(),
}

/// The game's entry function, found through [`GameHeader::entry`].
///
/// It receives the console's table and returns the game's, or null when
/// the game cannot work with this console.
pub type GameEntry = extern "C" fn(api: *const ConsoleApi) -> *const GameCallbacks;

/// The first four bytes of every game file: "GAME" in ASCII.
pub const MAGIC: u32 = u32::from_le_bytes(*b"GAME");

/// The start of a game file. Nine little-endian 32-bit numbers.
///
/// After it come `code_size` bytes for the code slot, then `data_size`
/// bytes for the data slot. `bss_size` bytes after those in the data slot
/// are not in the file: the console sets them to zero.
///
/// The addresses are in the file so that a console whose slots have moved
/// can tell that the game was built for another layout and refuse it,
/// instead of running code that points at the wrong places.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameHeader {
    /// [`MAGIC`].
    pub magic: u32,
    /// The [`API_VERSION`] the game was built against.
    pub api_version: u32,
    /// Where the code must be loaded. Must equal the console's code slot.
    pub code_addr: u32,
    /// Bytes of code in the file. Always a multiple of 4.
    pub code_size: u32,
    /// Where the data must be loaded. Must equal the console's data slot.
    pub data_addr: u32,
    /// Bytes of constants and initialised variables in the file. Always a
    /// multiple of 4.
    pub data_size: u32,
    /// Bytes to zero right after the data. Always a multiple of 4.
    pub bss_size: u32,
    /// Address of the [`GameEntry`] function, inside the code.
    pub entry: u32,
    /// CRC-32 of the eight numbers above followed by everything after the
    /// header (see [`Crc32`]).
    pub crc32: u32,
}

/// Why a header was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderError {
    /// Not a game file at all.
    BadMagic,
    /// Built for another version of the tables.
    WrongApiVersion,
    /// Built for a code slot at another address.
    WrongCodeAddress,
    /// The code is larger than the slot, or not a whole number of words.
    BadCodeSize,
    /// Built for a data slot at another address.
    WrongDataAddress,
    /// Data and zeroed memory together are larger than the slot, or not a
    /// whole number of words.
    BadDataSize,
    /// The entry function is not inside the code.
    EntryOutsideCode,
}

impl GameHeader {
    /// Bytes the header takes in a file.
    pub const SIZE: usize = 36;
    /// Bytes of the header that the checksum covers (all but the checksum
    /// itself).
    pub const CHECKED: usize = 32;

    /// Reads a header from the first bytes of a file.
    pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> Self {
        let word = |index: usize| {
            let at = index * 4;
            u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        Self {
            magic: word(0),
            api_version: word(1),
            code_addr: word(2),
            code_size: word(3),
            data_addr: word(4),
            data_size: word(5),
            bss_size: word(6),
            entry: word(7),
            crc32: word(8),
        }
    }

    /// The header as it is written to a file.
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let words = [
            self.magic,
            self.api_version,
            self.code_addr,
            self.code_size,
            self.data_addr,
            self.data_size,
            self.bss_size,
            self.entry,
            self.crc32,
        ];
        let mut bytes = [0; Self::SIZE];
        let mut index = 0;
        while index < words.len() {
            let [a, b, c, d] = words[index].to_le_bytes();
            bytes[index * 4] = a;
            bytes[index * 4 + 1] = b;
            bytes[index * 4 + 2] = c;
            bytes[index * 4 + 3] = d;
            index += 1;
        }
        bytes
    }

    /// Bytes that follow the header in the file.
    pub fn payload_size(&self) -> u32 {
        self.code_size.wrapping_add(self.data_size)
    }

    /// Checks everything that can be checked without the rest of the file:
    /// that this is a game, for this version, built for these slots, and
    /// that it fits. The console calls this before it writes anything.
    pub fn validate(&self) -> Result<(), HeaderError> {
        use layout::{CODE_SLOT_ADDR, CODE_SLOT_SIZE, DATA_SLOT_ADDR, DATA_SLOT_SIZE};

        if self.magic != MAGIC {
            return Err(HeaderError::BadMagic);
        }
        if self.api_version != API_VERSION {
            return Err(HeaderError::WrongApiVersion);
        }
        if self.code_addr != CODE_SLOT_ADDR {
            return Err(HeaderError::WrongCodeAddress);
        }
        if self.code_size == 0 || self.code_size % 4 != 0 || self.code_size > CODE_SLOT_SIZE {
            return Err(HeaderError::BadCodeSize);
        }
        if self.data_addr != DATA_SLOT_ADDR {
            return Err(HeaderError::WrongDataAddress);
        }
        let data_fits = self.data_size % 4 == 0
            && self.bss_size % 4 == 0
            && self.data_size <= DATA_SLOT_SIZE
            && self.bss_size <= DATA_SLOT_SIZE - self.data_size;
        if !data_fits {
            return Err(HeaderError::BadDataSize);
        }
        if self.entry < self.code_addr || self.entry - self.code_addr >= self.code_size {
            return Err(HeaderError::EntryOutsideCode);
        }
        Ok(())
    }
}

/// The checksum of a game file: the common CRC-32 (the one zip, PNG and
/// Ethernet use), fed a piece at a time so the console can check a file
/// while it streams it from the card.
///
/// It catches a damaged or cut-off file. It is not a signature: anyone can
/// compute it for any file.
#[derive(Clone, Copy)]
pub struct Crc32(u32);

impl Crc32 {
    /// One table entry per half byte: 64 bytes of table instead of the
    /// usual 1 KB, at about half the speed.
    const TABLE: [u32; 16] = [
        0x0000_0000,
        0x1DB7_1064,
        0x3B6E_20C8,
        0x26D9_30AC,
        0x76DC_4190,
        0x6B6B_51F4,
        0x4DB2_6158,
        0x5005_713C,
        0xEDB8_8320,
        0xF00F_9344,
        0xD6D6_A3E8,
        0xCB61_B38C,
        0x9B64_C2B0,
        0x86D3_D2D4,
        0xA00A_E278,
        0xBDBD_F21C,
    ];

    /// A checksum over nothing yet.
    pub const fn new() -> Self {
        Self(!0)
    }

    /// Adds bytes to the checksum.
    pub fn update(&mut self, bytes: &[u8]) {
        let mut crc = self.0;
        for &byte in bytes {
            crc ^= byte as u32;
            crc = (crc >> 4) ^ Self::TABLE[(crc & 15) as usize];
            crc = (crc >> 4) ^ Self::TABLE[(crc & 15) as usize];
        }
        self.0 = crc;
    }

    /// The checksum of everything added so far.
    pub const fn value(&self) -> u32 {
        !self.0
    }
}

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_standard_check_value() {
        // Every description of CRC-32 gives this value for "123456789".
        let mut crc = Crc32::new();
        crc.update(b"1234");
        crc.update(b"56789");
        assert_eq!(crc.value(), 0xCBF4_3926);
    }

    #[test]
    fn header_survives_a_round_trip() {
        let header = GameHeader {
            magic: MAGIC,
            api_version: API_VERSION,
            code_addr: layout::CODE_SLOT_ADDR,
            code_size: 64,
            data_addr: layout::DATA_SLOT_ADDR,
            data_size: 32,
            bss_size: 16,
            entry: layout::CODE_SLOT_ADDR + 8,
            crc32: 0x1234_5678,
        };
        assert_eq!(GameHeader::from_bytes(&header.to_bytes()), header);
        assert_eq!(header.validate(), Ok(()));
    }

    #[test]
    fn header_for_another_layout_is_refused() {
        let good = GameHeader {
            magic: MAGIC,
            api_version: API_VERSION,
            code_addr: layout::CODE_SLOT_ADDR,
            code_size: 64,
            data_addr: layout::DATA_SLOT_ADDR,
            data_size: 32,
            bss_size: 16,
            entry: layout::CODE_SLOT_ADDR,
            crc32: 0,
        };
        let moved = GameHeader { code_addr: 0x4008_8000, ..good };
        assert_eq!(moved.validate(), Err(HeaderError::WrongCodeAddress));
        let odd = GameHeader { code_size: 62, ..good };
        assert_eq!(odd.validate(), Err(HeaderError::BadCodeSize));
        let fat = GameHeader { bss_size: layout::DATA_SLOT_SIZE, ..good };
        assert_eq!(fat.validate(), Err(HeaderError::BadDataSize));
        let lost = GameHeader { entry: layout::CODE_SLOT_ADDR + 64, ..good };
        assert_eq!(lost.validate(), Err(HeaderError::EntryOutsideCode));
    }

    #[test]
    fn the_heap_and_the_slot_share_the_block_exactly() {
        assert_eq!(layout::DATA_SLOT_ADDR + layout::DATA_SLOT_SIZE, layout::DRAM2_END);
        assert_eq!(layout::CONSOLE_HEAP_SIZE, 49_616);
    }
}
