//! The game file and where a game lives on the chip.
//!
//! Copied from the loader experiment (experiments/native-loader/game-api,
//! version 1), which measured these numbers on the board. Only the
//! version check changed: a file must be built for version
//! [`MIN_GAME_VERSION`] (3) or later.
//!
//! On the PC a game is linked into the simulator instead, so nothing in
//! this file is used there, except by the tool that packs game files.

use crate::MIN_GAME_VERSION;

/// Where a game lives in memory. This is the only place the numbers are
/// written down. Everything else is made from them:
///
/// - the console's linker script on the chip (boards/cyd/build.rs calls
///   [`write_linker_symbols`](layout::write_linker_symbols));
/// - the games' linker script (games/build-chip.sh runs this crate's
///   example `slots_x`, which calls the same function);
/// - the game loader's `Slots::CONSOLE` and the header check below;
/// - games/chip/check_load.py, which reads them from the generated
///   slots.x.
///
/// The CYD also compares its linked slots with these numbers at start-up,
/// and refuses to start games when they differ.
pub mod layout {
    /// Start of the chip's executable RAM (SRAM0, the instruction bus).
    pub const IRAM_ADDR: u32 = 0x4008_0000;
    /// End of the chip's executable RAM (one past the last byte).
    pub const IRAM_END: u32 = 0x400A_0000;

    /// Start of the code slot.
    ///
    /// This is the top of the chip's executable RAM ([`IRAM_ADDR`] to
    /// [`IRAM_END`]). The console's own RAM code (and the Bluetooth
    /// radio's) sits at the bottom of it, below the slot. Memory at these
    /// addresses can only be read and written as whole, aligned 32-bit
    /// words (measured on the board: a byte read stops the program), so a
    /// game may keep only machine code and its literal pools here.
    /// Strings, tables and variables go to the data slot.
    ///
    /// 56 KB since 2026-11-17 (owner's decision; it was 64 KB at
    /// 0x4009_0000): the dual-mode radio needs 69,756 bytes of RAM code
    /// below the slot (measured), and the flash driver about 1,156 more.
    pub const CODE_SLOT_ADDR: u32 = 0x4009_2000;
    /// Size of the code slot in bytes (56 KB). The slot ends at
    /// [`IRAM_END`].
    pub const CODE_SLOT_SIZE: u32 = 0xE000;
    /// Executable RAM left below the code slot for the console (and the
    /// radio): 73,728 bytes.
    pub const CONSOLE_CODE_RAM: u32 = CODE_SLOT_ADDR - IRAM_ADDR;

    /// Start of the data slot: ordinary RAM (the top of SRAM1), readable
    /// and writable a byte at a time. It holds the game's constants
    /// (`.rodata`), its initialised variables (`.data`) and its zeroed
    /// variables (`.bss`), in that order.
    ///
    /// The console's heap (which the radio needs) fills the same memory
    /// block from the bottom up to here, so this slot is paid for with
    /// heap.
    ///
    /// Its top may also hold game code: the spill ([`CODE_SPILL_ADDR`]).
    /// That only works while the slot ends at the end of SRAM1
    /// ([`DRAM2_END`]), so the slot is fixed at 48 KB ending there
    /// (owner's decision, 2026-11-19); shrinking it for more heap would
    /// take room from the biggest games.
    pub const DATA_SLOT_ADDR: u32 = 0x3FFF_4000;
    /// Size of the data slot in bytes.
    pub const DATA_SLOT_SIZE: u32 = 0xC000;

    /// The memory block the data slot is cut from, as esp-hal 1.2.2 names
    /// it (`dram2_seg` in its `memory.x`). What is left below the slot is
    /// the console's heap.
    pub const DRAM2_ADDR: u32 = 0x3FFE_7E30;
    /// End of that block (one past the last byte). It is also the end of
    /// SRAM1 on the data bus (SRAM1 is 0x3FFE_0000..0x4000_0000 there).
    pub const DRAM2_END: u32 = 0x4000_0000;
    /// Bytes left for the console's heap.
    pub const CONSOLE_HEAP_SIZE: u32 = DATA_SLOT_ADDR - DRAM2_ADDR;

    /// Start of SRAM1 on the instruction bus. The same 128 KB of RAM is
    /// 0x3FFE_0000..0x4000_0000 on the data bus, in reverse word order:
    /// see [`sram1_iram_to_dram`].
    pub const SRAM1_IRAM_ADDR: u32 = 0x400A_0000;
    /// End of SRAM1 on the instruction bus (one past the last byte).
    pub const SRAM1_IRAM_END: u32 = 0x400C_0000;

    /// Start of "the spill": game code that does not fit in the code slot
    /// runs on past its end, here. The spill is the TOP of the data slot,
    /// seen from the instruction bus (stage 1, owner's go-ahead
    /// 2026-11-19). Code fills it from the top of the data slot down; the
    /// game's data fills the slot from the bottom up. [`fits`] is the rule
    /// that keeps them apart.
    ///
    /// Like the code slot, the spill can only be read and written as
    /// whole, aligned 32-bit words from the instruction bus.
    pub const CODE_SPILL_ADDR: u32 = CODE_SLOT_ADDR + CODE_SLOT_SIZE;
    /// The most code that may spill: the whole data slot. What a game may
    /// really use is less, because its data shares the room: see [`fits`].
    pub const CODE_SPILL_MAX: u32 = DATA_SLOT_SIZE;
    /// The most code a game file may have: the code slot and the whole
    /// spill (106,496 bytes).
    pub const CODE_MAX: u32 = CODE_SLOT_SIZE + CODE_SPILL_MAX;

    /// The data-bus address of the SRAM1 word at instruction-bus address
    /// `i` (word aligned, `SRAM1_IRAM_ADDR <= i < SRAM1_IRAM_END`).
    ///
    /// The order is reversed word by word: the first word on the
    /// instruction bus is the last word on the data bus. The bytes inside
    /// a word are assumed to keep their order. This is quoted from
    /// ESP-IDF (`SOC_DIRAM_INVERTED`, `esp_ptr_diram_dram_to_iram`) and
    /// seen to hold on the CYD (2026-11-22): the firmware tests it at
    /// every start-up, reading the instruction side only, and refuses
    /// every game with a spill when it does not hold.
    pub const fn sram1_iram_to_dram(i: u32) -> u32 {
        DRAM2_END - 4 - (i - SRAM1_IRAM_ADDR)
    }

    /// Bytes of the data slot (taken from its top) that `code_size` bytes
    /// of code use: what is past the code slot.
    pub const fn spill_of(code_size: u32) -> u32 {
        code_size.saturating_sub(CODE_SLOT_SIZE)
    }

    /// The one rule for a game's size: its code fits in the code slot and
    /// the spill, and the code past the slot, the constants and starting
    /// values (`data_size`) and the zeroed variables (`bss_size`) fit in
    /// the data slot together. Summed in 64 bits, so huge sizes cannot
    /// wrap round to small ones. Whole words are not checked here.
    pub const fn fits(code_size: u32, data_size: u32, bss_size: u32) -> bool {
        let shared = spill_of(code_size) as u64 + data_size as u64 + bss_size as u64;
        code_size <= CODE_MAX && shared <= DATA_SLOT_SIZE as u64
    }

    // The slots must stay inside their memories, on whole words.
    const _: () =
        assert!(CODE_SLOT_ADDR >= IRAM_ADDR && CODE_SLOT_SIZE <= IRAM_END - CODE_SLOT_ADDR);
    const _: () =
        assert!(DATA_SLOT_ADDR >= DRAM2_ADDR && DATA_SLOT_SIZE <= DRAM2_END - DATA_SLOT_ADDR);
    const _: () = assert!(CODE_SLOT_ADDR.is_multiple_of(4) && CODE_SLOT_SIZE.is_multiple_of(4));
    const _: () = assert!(DATA_SLOT_ADDR.is_multiple_of(4) && DATA_SLOT_SIZE.is_multiple_of(4));

    // The spill follows the code slot directly, at the start of SRAM1 on
    // the instruction bus.
    const _: () = assert!(CODE_SPILL_ADDR == IRAM_END && IRAM_END == SRAM1_IRAM_ADDR);
    // The spill only meets the data slot if the slot is SRAM1's top.
    const _: () = assert!(DATA_SLOT_ADDR + DATA_SLOT_SIZE == DRAM2_END);
    const _: () = assert!(CODE_SPILL_MAX <= DATA_SLOT_SIZE);
    // Its first word is the data slot's last, its last word the slot's
    // first, and it stays inside SRAM1.
    const _: () = assert!(sram1_iram_to_dram(CODE_SPILL_ADDR) == DRAM2_END - 4);
    const _: () =
        assert!(sram1_iram_to_dram(CODE_SPILL_ADDR + CODE_SPILL_MAX - 4) == DATA_SLOT_ADDR);
    const _: () = assert!(CODE_SPILL_ADDR + CODE_SPILL_MAX <= SRAM1_IRAM_END);

    /// Writes the slot numbers as linker-script symbols, one per line:
    ///
    /// ```text
    /// GAME_CODE_ORIGIN = 0x40092000;
    /// GAME_CODE_LENGTH = 0xe000;
    /// GAME_DATA_ORIGIN = 0x3fff4000;
    /// GAME_DATA_LENGTH = 0xc000;
    /// GAME_SPILL_ORIGIN = 0x400a0000;
    /// GAME_SPILL_LENGTH = 0xc000;
    /// GAME_SPILL_FIRST_DATA_WORD = 0x3ffffffc;
    /// GAME_SPILL_LAST_DATA_WORD = 0x3fff4000;
    /// ```
    ///
    /// The console's linker script (boards/cyd/game-slots.x) and the
    /// games' (games/chip/game.x) both read these names, so both are made
    /// from this one function. `GAME_SPILL_ORIGIN` and `_LENGTH` are the
    /// spill ([`CODE_SPILL_ADDR`], [`CODE_SPILL_MAX`]). The last two are
    /// the data words that the spill's first and last word are, by
    /// [`sram1_iram_to_dram`]: games/chip/check_load.py, which has its own
    /// copy of that rule, checks its copy against them.
    pub fn write_linker_symbols(out: &mut impl core::fmt::Write) -> core::fmt::Result {
        write!(
            out,
            "GAME_CODE_ORIGIN = {CODE_SLOT_ADDR:#010x};\n\
             GAME_CODE_LENGTH = {CODE_SLOT_SIZE:#x};\n\
             GAME_DATA_ORIGIN = {DATA_SLOT_ADDR:#010x};\n\
             GAME_DATA_LENGTH = {DATA_SLOT_SIZE:#x};\n\
             GAME_SPILL_ORIGIN = {CODE_SPILL_ADDR:#010x};\n\
             GAME_SPILL_LENGTH = {CODE_SPILL_MAX:#x};\n\
             GAME_SPILL_FIRST_DATA_WORD = {:#010x};\n\
             GAME_SPILL_LAST_DATA_WORD = {:#010x};\n",
            sram1_iram_to_dram(CODE_SPILL_ADDR),
            sram1_iram_to_dram(CODE_SPILL_ADDR + CODE_SPILL_MAX - 4),
        )
    }

    /// Start of the flash code window: room at the very start of the
    /// firmware's flash code, reserved by the CYD's `game-flash` feature,
    /// where games' flash parts (code that runs straight from flash) lie.
    ///
    /// This is an esp-hal 1.2.2 fact copied here: the origin of its
    /// `irom_seg` (0x400D_0000 plus the 0x20 image header). The firmware's
    /// linker script checks that the window really lands here and stops
    /// the link if not. Like the code slot, it can only be read as whole,
    /// aligned 32-bit words.
    pub const FLASH_CODE_ADDR: u32 = 0x400D_0020;
    /// Size of the flash code window (128 KB, a multiple of the cache's
    /// 16 KB set period, so the firmware's own flash code keeps its cache
    /// sets).
    pub const FLASH_CODE_SIZE: u32 = 0x2_0000;
    /// Start of the flash read-only data window: esp-hal 1.2.2's
    /// `drom_seg` origin (0x3F40_0020) plus its 0x100-byte app
    /// descriptor. Byte reads are fine here.
    pub const FLASH_RO_ADDR: u32 = 0x3F40_0120;
    /// Size of the flash read-only data window (64 KB, a multiple of
    /// 16 KB).
    pub const FLASH_RO_SIZE: u32 = 0x1_0000;

    /// Where one game's flash part lies inside the two windows. Moving a
    /// place means building that game and the firmware again.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct FlashPlace {
        /// The game's card folder name (capitals, at most 8 letters).
        pub game: &'static str,
        /// Offset of its code inside the code window.
        pub code_off: u32,
        /// The most code it may have there.
        pub code_max: u32,
        /// Offset of its read-only data inside the data window.
        pub ro_off: u32,
        /// The most read-only data it may have there.
        pub ro_max: u32,
    }

    impl FlashPlace {
        /// Address of the code half's place.
        pub const fn code_addr(&self) -> u32 {
            FLASH_CODE_ADDR + self.code_off
        }
        /// Address of the data half's place.
        pub const fn ro_addr(&self) -> u32 {
            FLASH_RO_ADDR + self.ro_off
        }
    }

    /// Every game that has a flash part, and its place. Offsets and
    /// maxima are multiples of 32 (cache lines), and each place ends at
    /// least 32 bytes before its window's end (the firmware writes an end
    /// mark into the window's last word).
    ///
    /// MERCATOR's place was raised on 2026-11-23 (step G1, the planet
    /// landing) from 32 KB of code and 16 KB of constants to 80 KB and
    /// 24 KB, so the planned planet part fits without moving it again;
    /// its constants were raised again to 32 KB in step G4 (the drones,
    /// the effects and the props).
    pub const FLASH_PLACES: &[FlashPlace] = &[FlashPlace {
        game: "MERCATOR",
        code_off: 0,
        code_max: 0x1_4000,
        ro_off: 0,
        ro_max: 0x8000,
    }];

    /// The place of the game with this card folder name, if it has one.
    pub fn flash_place(game: &str) -> Option<&'static FlashPlace> {
        FLASH_PLACES.iter().find(|p| p.game == game)
    }

    const fn is_folder_name(name: &str) -> bool {
        let b = name.as_bytes();
        if b.is_empty() || b.len() > 8 {
            return false;
        }
        let mut i = 0;
        while i < b.len() {
            if !(b[i].is_ascii_uppercase() || b[i].is_ascii_digit() || b[i] == b'_') {
                return false;
            }
            i += 1;
        }
        true
    }

    const fn same_name(a: &str, b: &str) -> bool {
        let (a, b) = (a.as_bytes(), b.as_bytes());
        if a.len() != b.len() {
            return false;
        }
        let mut i = 0;
        while i < a.len() {
            if a[i] != b[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    const fn places_are_sound() -> bool {
        let mut i = 0;
        while i < FLASH_PLACES.len() {
            let p = &FLASH_PLACES[i];
            let lines = p.code_off.is_multiple_of(32)
                && p.code_max.is_multiple_of(32)
                && p.ro_off.is_multiple_of(32)
                && p.ro_max.is_multiple_of(32);
            // Summed in 64 bits; the last 32 bytes stay free for the mark.
            let inside = p.code_max > 0
                && p.code_off as u64 + p.code_max as u64 + 32 <= FLASH_CODE_SIZE as u64
                && p.ro_off as u64 + p.ro_max as u64 + 32 <= FLASH_RO_SIZE as u64;
            if !lines || !inside || !is_folder_name(p.game) {
                return false;
            }
            let mut j = i + 1;
            while j < FLASH_PLACES.len() {
                let q = &FLASH_PLACES[j];
                let code_apart =
                    p.code_off + p.code_max <= q.code_off || q.code_off + q.code_max <= p.code_off;
                let ro_apart = p.ro_max == 0
                    || q.ro_max == 0
                    || p.ro_off + p.ro_max <= q.ro_off
                    || q.ro_off + q.ro_max <= p.ro_off;
                if !code_apart || !ro_apart || same_name(p.game, q.game) {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    // The windows: on whole cache sets, apart from the slots.
    const _: () = assert!(FLASH_CODE_ADDR.is_multiple_of(32) && FLASH_RO_ADDR.is_multiple_of(32));
    const _: () = assert!(
        FLASH_CODE_SIZE.is_multiple_of(16 * 1024) && FLASH_RO_SIZE.is_multiple_of(16 * 1024)
    );
    // Places: inside their windows, on cache lines, apart, named well.
    const _: () = assert!(places_are_sound());

    /// Writes one game's place as linker-script symbols (the
    /// `flash-place.x` its second link includes):
    ///
    /// ```text
    /// GAME_FLASH_CODE_ORIGIN = 0x400d0020;
    /// GAME_FLASH_CODE_LENGTH = 0x14000;
    /// GAME_FLASH_RO_ORIGIN = 0x3f400120;
    /// GAME_FLASH_RO_LENGTH = 0x8000;
    /// ```
    ///
    /// `None` when the game has no place.
    pub fn write_flash_place_symbols(
        game: &str,
        out: &mut impl core::fmt::Write,
    ) -> Option<core::fmt::Result> {
        let p = flash_place(game)?;
        Some(write!(
            out,
            "GAME_FLASH_CODE_ORIGIN = {:#010x};\n\
             GAME_FLASH_CODE_LENGTH = {:#x};\n\
             GAME_FLASH_RO_ORIGIN = {:#010x};\n\
             GAME_FLASH_RO_LENGTH = {:#x};\n",
            p.code_addr(),
            p.code_max,
            p.ro_addr(),
            p.ro_max,
        ))
    }

    /// Writes the two windows as linker-script symbols, for the
    /// firmware's `game-flash.x` (boards/cyd/build.rs):
    ///
    /// ```text
    /// GAME_FLASH_CODE_WINDOW_ORIGIN = 0x400d0020;
    /// GAME_FLASH_CODE_WINDOW_LENGTH = 0x20000;
    /// GAME_FLASH_RO_WINDOW_ORIGIN = 0x3f400120;
    /// GAME_FLASH_RO_WINDOW_LENGTH = 0x10000;
    /// ```
    pub fn write_flash_window_symbols(out: &mut impl core::fmt::Write) -> core::fmt::Result {
        write!(
            out,
            "GAME_FLASH_CODE_WINDOW_ORIGIN = {FLASH_CODE_ADDR:#010x};\n\
             GAME_FLASH_CODE_WINDOW_LENGTH = {FLASH_CODE_SIZE:#x};\n\
             GAME_FLASH_RO_WINDOW_ORIGIN = {FLASH_RO_ADDR:#010x};\n\
             GAME_FLASH_RO_WINDOW_LENGTH = {FLASH_RO_SIZE:#x};\n",
        )
    }
}

/// The first four bytes of every game file: "GAME" in ASCII.
pub const MAGIC: u32 = u32::from_le_bytes(*b"GAME");

/// The first four bytes of a game file with a flash part: "GAMF". The
/// [`GameHeader`] is followed by a [`FlashPartHeader`]; the payload starts
/// after it. A new magic and not a new version, so that a console without
/// flash parts refuses the file (it lets later versions through).
pub const MAGIC_FLASH: u32 = u32::from_le_bytes(*b"GAMF");

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
    /// The [`crate::API_VERSION`] the game was built against. Version 3 or
    /// later; a later one is let through, and the game itself then checks
    /// the console's tables (see the crate notes).
    pub api_version: u32,
    /// Where the code must be loaded. Must equal the console's code slot.
    pub code_addr: u32,
    /// Bytes of code in the file. Always a multiple of 4. More than the
    /// code slot means the rest runs on into the spill
    /// ([`layout::CODE_SPILL_ADDR`]); nothing else in the header changes.
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
    /// header (see [`Crc32`]). In a "GAMF" file that is the
    /// [`FlashPartHeader`] and then the payload.
    pub crc32: u32,
}

/// Why a header was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderError {
    /// Not a game file at all.
    BadMagic,
    /// Built for a version below [`MIN_GAME_VERSION`]: 1 (the loader
    /// experiment) or 2 (one player), or not built at all.
    WrongApiVersion,
    /// Built for a code slot at another address.
    WrongCodeAddress,
    /// The code is larger than the code slot and the whole spill
    /// ([`layout::CODE_MAX`]), or empty, or not a whole number of words.
    BadCodeSize,
    /// Built for a data slot at another address.
    WrongDataAddress,
    /// Data and zeroed memory together are larger than the slot, or not a
    /// whole number of words.
    BadDataSize,
    /// The data would fit alone, but the code past the code slot and the
    /// data meet in the data slot ([`layout::fits`] says no).
    CodeMeetsData,
    /// The entry function is not inside the code.
    EntryOutsideCode,
    /// Fewer bytes than the header (and, for "GAMF", its extension).
    Short,
    /// A "GAMF" file whose [`FlashPartHeader`] does not pass its check.
    BadFlashPart,
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
        use layout::{CODE_MAX, CODE_SLOT_ADDR, DATA_SLOT_ADDR, DATA_SLOT_SIZE};

        if self.magic != MAGIC {
            return Err(HeaderError::BadMagic);
        }
        if self.api_version < MIN_GAME_VERSION {
            return Err(HeaderError::WrongApiVersion);
        }
        if self.code_addr != CODE_SLOT_ADDR {
            return Err(HeaderError::WrongCodeAddress);
        }
        if self.code_size == 0 || !self.code_size.is_multiple_of(4) || self.code_size > CODE_MAX {
            return Err(HeaderError::BadCodeSize);
        }
        if self.data_addr != DATA_SLOT_ADDR {
            return Err(HeaderError::WrongDataAddress);
        }
        let data_fits = self.data_size.is_multiple_of(4)
            && self.bss_size.is_multiple_of(4)
            && self.data_size <= DATA_SLOT_SIZE
            && self.bss_size <= DATA_SLOT_SIZE - self.data_size;
        if !data_fits {
            return Err(HeaderError::BadDataSize);
        }
        // Code past the code slot takes the top of the data slot.
        if !layout::fits(self.code_size, self.data_size, self.bss_size) {
            return Err(HeaderError::CodeMeetsData);
        }
        if self.entry < self.code_addr || self.entry - self.code_addr >= self.code_size {
            return Err(HeaderError::EntryOutsideCode);
        }
        Ok(())
    }
}

/// The extension of a "GAMF" file ([`MAGIC_FLASH`]): eight little-endian
/// 32-bit numbers right after the [`GameHeader`], covered by its CRC.
///
/// It names the game's flash part: code and read-only data that are not
/// in the file but in the console's firmware, at fixed places inside the
/// windows of [`layout`]. A console runs the game only when its firmware
/// holds exactly these bytes (same addresses, sizes and CRCs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlashPartHeader {
    /// Bytes of the extension: [`FlashPartHeader::SIZE`] (32).
    pub ext_size: u32,
    /// Where the code half lies (inside the flash code window).
    pub code_addr: u32,
    /// Bytes of the code half. A multiple of 4, not 0.
    pub code_size: u32,
    /// CRC-32 of the code half as it lies in flash.
    pub code_crc32: u32,
    /// Where the read-only data half lies (inside the data window).
    pub ro_addr: u32,
    /// Bytes of the data half. A multiple of 4, may be 0.
    pub ro_size: u32,
    /// CRC-32 of the data half (0 when it is empty).
    pub ro_crc32: u32,
    /// Always 0.
    pub reserved: u32,
}

impl FlashPartHeader {
    /// Bytes the extension takes in a file.
    pub const SIZE: usize = 32;
    /// Where the payload of a "GAMF" file starts.
    pub const FILE_HEAD: usize = GameHeader::SIZE + Self::SIZE;

    /// Reads the extension (the 32 bytes after the [`GameHeader`]).
    pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> Self {
        let word = |index: usize| {
            let at = index * 4;
            u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        Self {
            ext_size: word(0),
            code_addr: word(1),
            code_size: word(2),
            code_crc32: word(3),
            ro_addr: word(4),
            ro_size: word(5),
            ro_crc32: word(6),
            reserved: word(7),
        }
    }

    /// The extension as it is written to a file.
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let words = [
            self.ext_size,
            self.code_addr,
            self.code_size,
            self.code_crc32,
            self.ro_addr,
            self.ro_size,
            self.ro_crc32,
            self.reserved,
        ];
        let mut bytes = [0; Self::SIZE];
        let mut index = 0;
        while index < words.len() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&words[index].to_le_bytes());
            index += 1;
        }
        bytes
    }

    /// Checks the extension on its own: its size, both halves inside
    /// their windows on whole words, a code half that is not empty, and
    /// the reserved word 0. Whether the firmware holds the part is the
    /// board's question, not this one.
    pub fn validate(&self) -> Result<(), HeaderError> {
        use layout::{FLASH_CODE_ADDR, FLASH_CODE_SIZE, FLASH_RO_ADDR, FLASH_RO_SIZE};
        let inside = |addr: u32, size: u32, start: u32, len: u32| {
            addr >= start
                && (addr as u64 + size as u64) <= start as u64 + len as u64
                && addr.is_multiple_of(4)
                && size.is_multiple_of(4)
        };
        let good = self.ext_size == Self::SIZE as u32
            && self.reserved == 0
            && self.code_size > 0
            && inside(
                self.code_addr,
                self.code_size,
                FLASH_CODE_ADDR,
                FLASH_CODE_SIZE,
            )
            && inside(self.ro_addr, self.ro_size, FLASH_RO_ADDR, FLASH_RO_SIZE)
            && (self.ro_size > 0 || self.ro_crc32 == 0);
        if good {
            Ok(())
        } else {
            Err(HeaderError::BadFlashPart)
        }
    }
}

/// Reads and checks the head of a game file of either kind: the
/// [`GameHeader`] and, for "GAMF", the [`FlashPartHeader`]. The payload
/// starts at [`GameHeader::SIZE`] or [`FlashPartHeader::FILE_HEAD`]. The
/// CRC is not checked here (it needs the whole file).
pub fn read_game_header(
    bytes: &[u8],
) -> Result<(GameHeader, Option<FlashPartHeader>), HeaderError> {
    let Some(head) = bytes.first_chunk::<{ GameHeader::SIZE }>() else {
        return Err(HeaderError::Short);
    };
    let header = GameHeader::from_bytes(head);
    match header.magic {
        MAGIC => header.validate().map(|()| (header, None)),
        MAGIC_FLASH => {
            // The same rules as a plain file for the RAM part.
            GameHeader {
                magic: MAGIC,
                ..header
            }
            .validate()?;
            let Some(ext) = bytes
                .get(GameHeader::SIZE..)
                .and_then(|rest| rest.first_chunk::<{ FlashPartHeader::SIZE }>())
            else {
                return Err(HeaderError::Short);
            };
            let ext = FlashPartHeader::from_bytes(ext);
            ext.validate()?;
            Ok((header, Some(ext)))
        }
        _ => Err(HeaderError::BadMagic),
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
    use crate::API_VERSION;

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
        let moved = GameHeader {
            code_addr: 0x4008_8000,
            ..good
        };
        assert_eq!(moved.validate(), Err(HeaderError::WrongCodeAddress));
        let odd = GameHeader {
            code_size: 62,
            ..good
        };
        assert_eq!(odd.validate(), Err(HeaderError::BadCodeSize));
        let fat = GameHeader {
            bss_size: layout::DATA_SLOT_SIZE,
            ..good
        };
        assert_eq!(fat.validate(), Err(HeaderError::BadDataSize));
        let lost = GameHeader {
            entry: layout::CODE_SLOT_ADDR + 64,
            ..good
        };
        assert_eq!(lost.validate(), Err(HeaderError::EntryOutsideCode));
    }

    #[test]
    fn the_linker_symbols_say_the_layout() {
        extern crate std;
        let mut text = std::string::String::new();
        layout::write_linker_symbols(&mut text).unwrap();
        assert_eq!(
            text,
            "GAME_CODE_ORIGIN = 0x40092000;\n\
             GAME_CODE_LENGTH = 0xe000;\n\
             GAME_DATA_ORIGIN = 0x3fff4000;\n\
             GAME_DATA_LENGTH = 0xc000;\n\
             GAME_SPILL_ORIGIN = 0x400a0000;\n\
             GAME_SPILL_LENGTH = 0xc000;\n\
             GAME_SPILL_FIRST_DATA_WORD = 0x3ffffffc;\n\
             GAME_SPILL_LAST_DATA_WORD = 0x3fff4000;\n"
        );
        // The code slot is the top of the executable RAM, and leaves room
        // below it for the console and the radio (69,756 bytes measured
        // with the radio, 2026-11-17).
        assert_eq!(
            layout::CODE_SLOT_ADDR + layout::CODE_SLOT_SIZE,
            layout::IRAM_END
        );
        assert_eq!(layout::CONSOLE_CODE_RAM, 73_728);
    }

    #[test]
    fn the_heap_and_the_slot_share_the_block_exactly() {
        assert_eq!(
            layout::DATA_SLOT_ADDR + layout::DATA_SLOT_SIZE,
            layout::DRAM2_END
        );
        assert_eq!(layout::CONSOLE_HEAP_SIZE, 49_616);
    }

    #[test]
    fn the_spill_is_the_data_slot_backwards() {
        extern crate std;
        use layout::*;
        assert_eq!(CODE_SPILL_ADDR, 0x400A_0000);
        assert_eq!(CODE_MAX, 106_496);
        assert_eq!(sram1_iram_to_dram(0x400A_0000), 0x3FFF_FFFC);
        assert_eq!(sram1_iram_to_dram(0x400A_BFFC), 0x3FFF_4000);
        // The whole of SRAM1: its last instruction word is the first word
        // of SRAM1 on the data bus.
        assert_eq!(sram1_iram_to_dram(SRAM1_IRAM_END - 4), 0x3FFE_0000);
        // One to one over the whole spill, and every word lands inside the
        // data slot, on a word.
        let mut seen = std::vec![false; (DATA_SLOT_SIZE / 4) as usize];
        let mut i = CODE_SPILL_ADDR;
        while i < CODE_SPILL_ADDR + CODE_SPILL_MAX {
            let d = sram1_iram_to_dram(i);
            assert!((DATA_SLOT_ADDR..DATA_SLOT_ADDR + DATA_SLOT_SIZE).contains(&d));
            assert!(d.is_multiple_of(4));
            let k = ((d - DATA_SLOT_ADDR) / 4) as usize;
            assert!(!seen[k], "{i:#x} and another word share {d:#x}");
            seen[k] = true;
            i += 4;
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn fits_is_the_shared_rule() {
        use layout::*;
        let slot = CODE_SLOT_SIZE;
        let room = DATA_SLOT_SIZE;
        for spill in [0, 4, 24 * 1024, 48 * 1024] {
            let code = slot + spill;
            // Exactly full, split between data and zeroed variables.
            assert!(fits(code, room - spill, 0), "spill {spill}");
            assert!(fits(code, 0, room - spill), "spill {spill}");
            if spill < room {
                assert!(fits(code, 4, room - spill - 4), "spill {spill}");
            }
            // One word more does not.
            assert!(!fits(code, room - spill + 4, 0), "spill {spill}");
            assert!(!fits(code, 0, room - spill + 4), "spill {spill}");
        }
        // No spill: the data slot alone, as before.
        assert!(fits(4, room, 0));
        assert!(fits(slot, room - 8, 8));
        // Code past the slot and the spill.
        assert!(!fits(CODE_MAX + 4, 0, 0));
        // Huge sizes do not wrap round.
        assert!(!fits(4, 0xFFFF_FFFC, 8));
        assert!(!fits(4, 0x8000_0000, 0x8000_0000));
        assert!(!fits(0xFFFF_FFFC, 0, 0));
        assert!(!fits(slot + 4, 0xFFFF_FFFC, 0));
        assert_eq!(spill_of(0), 0);
        assert_eq!(spill_of(slot), 0);
        assert_eq!(spill_of(slot + 4), 4);
    }

    #[test]
    fn a_header_with_code_past_the_slot() {
        use layout::*;
        let good = GameHeader {
            magic: MAGIC,
            api_version: API_VERSION,
            code_addr: CODE_SLOT_ADDR,
            code_size: CODE_SLOT_SIZE + 4,
            data_addr: DATA_SLOT_ADDR,
            data_size: 32,
            bss_size: 16,
            entry: CODE_SLOT_ADDR,
            crc32: 0,
        };
        assert_eq!(good.code_size, 57_348);
        assert_eq!(good.validate(), Ok(()));
        // The entry may be in the spill, up to the last code word.
        for entry in [CODE_SPILL_ADDR, CODE_SPILL_ADDR + 3] {
            assert_eq!(GameHeader { entry, ..good }.validate(), Ok(()));
        }
        assert_eq!(
            GameHeader {
                entry: CODE_SPILL_ADDR + 4,
                ..good
            }
            .validate(),
            Err(HeaderError::EntryOutsideCode)
        );
        // More code than the slot and the whole spill.
        assert_eq!(
            GameHeader {
                code_size: 106_500,
                ..good
            }
            .validate(),
            Err(HeaderError::BadCodeSize)
        );
        // Code past the slot and data that meet by one word.
        let spill = 24 * 1024;
        let full = GameHeader {
            code_size: CODE_SLOT_SIZE + spill,
            data_size: DATA_SLOT_SIZE - spill - 64,
            bss_size: 64,
            ..good
        };
        assert_eq!(full.validate(), Ok(()));
        assert_eq!(
            GameHeader {
                bss_size: 68,
                ..full
            }
            .validate(),
            Err(HeaderError::CodeMeetsData)
        );
        // Data that is too big alone stays BadDataSize, spill or not.
        assert_eq!(
            GameHeader {
                bss_size: DATA_SLOT_SIZE,
                ..full
            }
            .validate(),
            Err(HeaderError::BadDataSize)
        );
        // The whole spill and no data at all.
        let max = GameHeader {
            code_size: CODE_MAX,
            data_size: 0,
            bss_size: 0,
            ..good
        };
        assert_eq!(max.validate(), Ok(()));
        assert_eq!(
            GameHeader { bss_size: 4, ..max }.validate(),
            Err(HeaderError::CodeMeetsData)
        );
    }

    #[test]
    fn version_1_and_2_files_are_refused() {
        let v1 = GameHeader {
            magic: MAGIC,
            api_version: 1,
            code_addr: layout::CODE_SLOT_ADDR,
            code_size: 64,
            data_addr: layout::DATA_SLOT_ADDR,
            data_size: 32,
            bss_size: 16,
            entry: layout::CODE_SLOT_ADDR,
            crc32: 0,
        };
        assert_eq!(v1.validate(), Err(HeaderError::WrongApiVersion));
        // Version 2 (one player) is refused too: its input is laid out
        // differently.
        let v2 = GameHeader {
            api_version: 2,
            ..v1
        };
        assert_eq!(v2.validate(), Err(HeaderError::WrongApiVersion));
        let now = GameHeader {
            api_version: API_VERSION,
            ..v1
        };
        assert_eq!(now.validate(), Ok(()));
        let later = GameHeader {
            api_version: API_VERSION + 1,
            ..v1
        };
        assert_eq!(later.validate(), Ok(()));
    }

    fn flash_ext() -> FlashPartHeader {
        FlashPartHeader {
            ext_size: 32,
            code_addr: layout::FLASH_CODE_ADDR,
            code_size: 19_224,
            code_crc32: 0x1111_2222,
            ro_addr: layout::FLASH_RO_ADDR,
            ro_size: 10_524,
            ro_crc32: 0x3333_4444,
            reserved: 0,
        }
    }

    fn plain_header() -> GameHeader {
        GameHeader {
            magic: MAGIC,
            api_version: API_VERSION,
            code_addr: layout::CODE_SLOT_ADDR,
            code_size: 64,
            data_addr: layout::DATA_SLOT_ADDR,
            data_size: 32,
            bss_size: 16,
            entry: layout::CODE_SLOT_ADDR,
            crc32: 0,
        }
    }

    #[test]
    fn the_flash_extension_survives_a_round_trip() {
        let ext = flash_ext();
        assert_eq!(FlashPartHeader::from_bytes(&ext.to_bytes()), ext);
        assert_eq!(ext.validate(), Ok(()));
        assert_eq!(&ext.to_bytes()[..8], &[32, 0, 0, 0, 0x20, 0, 0x0D, 0x40]);
        assert_eq!(MAGIC_FLASH.to_le_bytes(), *b"GAMF");
        assert_eq!(FlashPartHeader::FILE_HEAD, 68);
    }

    #[test]
    fn a_bad_flash_extension_is_refused() {
        use layout::*;
        let good = flash_ext();
        let bad = [
            FlashPartHeader {
                ext_size: 36,
                ..good
            },
            FlashPartHeader {
                reserved: 1,
                ..good
            },
            FlashPartHeader {
                code_size: 0,
                ..good
            },
            FlashPartHeader {
                code_size: 6,
                ..good
            },
            FlashPartHeader { ro_size: 2, ..good },
            FlashPartHeader {
                code_addr: FLASH_CODE_ADDR - 4,
                ..good
            },
            FlashPartHeader {
                code_addr: FLASH_CODE_ADDR + 2,
                ..good
            },
            FlashPartHeader {
                code_addr: FLASH_CODE_ADDR + FLASH_CODE_SIZE - 16,
                code_size: 20,
                ..good
            },
            FlashPartHeader {
                code_addr: 0xFFFF_FFFC,
                ..good
            },
            FlashPartHeader {
                code_addr: layout::CODE_SLOT_ADDR,
                ..good
            },
            FlashPartHeader {
                ro_addr: FLASH_RO_ADDR + FLASH_RO_SIZE,
                ..good
            },
            FlashPartHeader {
                ro_addr: FLASH_CODE_ADDR,
                ..good
            },
            FlashPartHeader { ro_size: 0, ..good },
        ];
        for (i, ext) in bad.iter().enumerate() {
            assert_eq!(ext.validate(), Err(HeaderError::BadFlashPart), "case {i}");
        }
        // An empty data half with a zero CRC, and halves up to the end.
        let edge = FlashPartHeader {
            ro_size: 0,
            ro_crc32: 0,
            code_addr: FLASH_CODE_ADDR + FLASH_CODE_SIZE - 4,
            code_size: 4,
            ..good
        };
        assert_eq!(edge.validate(), Ok(()));
    }

    #[test]
    fn read_game_header_reads_both_kinds() {
        extern crate std;
        let plain = plain_header();
        let mut file = std::vec::Vec::from(plain.to_bytes());
        file.extend_from_slice(&[0; 96]);
        assert_eq!(read_game_header(&file), Ok((plain, None)));
        assert_eq!(read_game_header(&file[..35]), Err(HeaderError::Short));

        let flash = GameHeader {
            magic: MAGIC_FLASH,
            ..plain
        };
        let mut file = std::vec::Vec::from(flash.to_bytes());
        file.extend_from_slice(&flash_ext().to_bytes());
        file.extend_from_slice(&[0; 96]);
        assert_eq!(read_game_header(&file), Ok((flash, Some(flash_ext()))));
        assert_eq!(read_game_header(&file[..67]), Err(HeaderError::Short));
        // The RAM part keeps the plain rules.
        let mut lost = file.clone();
        lost[28..32].copy_from_slice(&(layout::CODE_SLOT_ADDR + 64).to_le_bytes());
        assert_eq!(read_game_header(&lost), Err(HeaderError::EntryOutsideCode));
        // A bad extension.
        let mut odd = file.clone();
        odd[64] = 1;
        assert_eq!(read_game_header(&odd), Err(HeaderError::BadFlashPart));
        // Other magics, and a plain check of a "GAMF" header, say no.
        let mut other = file.clone();
        other[3] = b'X';
        assert_eq!(read_game_header(&other), Err(HeaderError::BadMagic));
        assert_eq!(flash.validate(), Err(HeaderError::BadMagic));
    }

    #[test]
    fn the_flash_places_sit_in_their_windows() {
        use layout::*;
        assert!(!FLASH_PLACES.is_empty());
        for p in FLASH_PLACES {
            assert!(p.code_addr() >= FLASH_CODE_ADDR);
            assert!(p.code_addr() + p.code_max + 32 <= FLASH_CODE_ADDR + FLASH_CODE_SIZE);
            assert!(p.ro_addr() + p.ro_max + 32 <= FLASH_RO_ADDR + FLASH_RO_SIZE);
            assert_eq!(flash_place(p.game), Some(p));
        }
        assert_eq!(flash_place("LANDER"), None);
        assert_eq!(flash_place("mercator"), None);
        // Whole 16 KB cache periods, so the firmware's own lines keep
        // their sets when the windows push them up.
        assert_eq!(FLASH_CODE_SIZE % (16 * 1024), 0);
        assert_eq!(FLASH_RO_SIZE % (16 * 1024), 0);
    }

    #[test]
    fn the_flash_symbols_say_the_places() {
        extern crate std;
        let mut text = std::string::String::new();
        layout::write_flash_place_symbols("MERCATOR", &mut text)
            .unwrap()
            .unwrap();
        assert_eq!(
            text,
            "GAME_FLASH_CODE_ORIGIN = 0x400d0020;\n\
             GAME_FLASH_CODE_LENGTH = 0x14000;\n\
             GAME_FLASH_RO_ORIGIN = 0x3f400120;\n\
             GAME_FLASH_RO_LENGTH = 0x8000;\n"
        );
        let mut none = std::string::String::new();
        assert!(layout::write_flash_place_symbols("NOPE", &mut none).is_none());
        assert!(none.is_empty());
        let mut win = std::string::String::new();
        layout::write_flash_window_symbols(&mut win).unwrap();
        assert_eq!(
            win,
            "GAME_FLASH_CODE_WINDOW_ORIGIN = 0x400d0020;\n\
             GAME_FLASH_CODE_WINDOW_LENGTH = 0x20000;\n\
             GAME_FLASH_RO_WINDOW_ORIGIN = 0x3f400120;\n\
             GAME_FLASH_RO_WINDOW_LENGTH = 0x10000;\n"
        );
    }
}
