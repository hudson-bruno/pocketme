//! The two game slots: where they are, and how the loader hands bytes to
//! the board.

use game_api::{FlashPartHeader, layout};

/// Where the console's two game slots are and how big they are.
///
/// The loader checks every game against this, so nothing else in the
/// crate knows a slot address or size. The console uses
/// [`Slots::CONSOLE`], which comes from the one place the layout is
/// written down (`game_api::layout`). Tests make other ones, to show
/// that a game built for other slots is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slots {
    /// Start of the code slot (an address on the chip).
    pub code_addr: u32,
    /// Bytes in the code slot.
    pub code_size: u32,
    /// Bytes of code space right after the code slot that are the top of
    /// the data slot, seen from the instruction bus in reverse word order
    /// ("the spill", `game_api::layout::CODE_SPILL_ADDR`). Code past the
    /// slot goes there, and shares the data slot with the game's data.
    /// 0 = no spill: an older console, or a board whose start-up mirror
    /// check failed. Only up to `data_size` of it is ever used.
    pub spill_size: u32,
    /// Start of the data slot.
    pub data_addr: u32,
    /// Bytes in the data slot.
    pub data_size: u32,
}

impl Slots {
    /// The console's slots, from `game_api::layout`.
    pub const CONSOLE: Slots = Slots {
        code_addr: layout::CODE_SLOT_ADDR,
        code_size: layout::CODE_SLOT_SIZE,
        spill_size: layout::CODE_SPILL_MAX,
        data_addr: layout::DATA_SLOT_ADDR,
        data_size: layout::DATA_SLOT_SIZE,
    };

    /// Bytes of code a game may have: the code slot and the spill, the
    /// spill never more than the data slot (it is the data slot's top).
    /// A 64-bit number, so that made-up huge slots cannot wrap round.
    pub const fn code_room(&self) -> u64 {
        let spill = if self.spill_size < self.data_size {
            self.spill_size
        } else {
            self.data_size
        };
        self.code_size as u64 + spill as u64
    }

    /// Bytes of the data slot, from its start, that data may use when the
    /// game's code ends at code offset `code_end`: all of it, less the top
    /// that the code took through the spill. 0 when the code took it all.
    pub const fn data_room(&self, code_end: u32) -> u32 {
        let spill_used = code_end.saturating_sub(self.code_size);
        self.data_size.saturating_sub(spill_used)
    }
}

/// What the board does with the bytes of a game. The loader calls these
/// in order, and only with places inside the slot sizes it was given
/// ([`Slots`]): an offset is counted in bytes from the start of the slot.
///
/// The code slot on the chip can only be read and written as whole,
/// aligned 32-bit words. A byte access there stops the program (measured
/// on the board). So code comes as words, and every code offset is a
/// multiple of 4. The data slot is ordinary memory and takes bytes.
///
/// Code offsets go from 0 up to `code_size + spill_size`. Offsets from
/// `code_size` on are the spill: on the chip the same RAM as the top of
/// the data slot, and on the chip its instruction address is simply
/// `code_addr + offset` (the spill follows the slot directly). The
/// loader never hands `write_data` or `zero_data` a range that overlaps
/// the spill of the game it is loading.
pub trait GameSlots {
    /// Puts one 32-bit word into the code slot (or the spill) at
    /// `offset` (a multiple of 4). On the chip: one volatile 32-bit
    /// write.
    fn write_code_word(&mut self, offset: u32, word: u32);

    /// Reads one 32-bit word back from the code slot (or the spill) at
    /// `offset` (a multiple of 4). The loader reads every word back right
    /// after writing it, so a slot that does not keep what it is given is
    /// found at once; spill words are read once more at the end of the
    /// load. On the chip: one volatile 32-bit read.
    fn read_code_word(&mut self, offset: u32) -> u32;

    /// Copies `bytes` into the data slot, starting at `offset`.
    fn write_data(&mut self, offset: u32, bytes: &[u8]);

    /// Sets `len` bytes of the data slot to zero, starting at `offset`.
    /// Called once, at the very end of a good load, for the game's
    /// variables that start at zero.
    fn zero_data(&mut self, offset: u32, len: u32);

    /// Only for a game with a flash part ("GAMF"): does this firmware hold
    /// exactly the part `part` names (same addresses, sizes and CRC-32s of
    /// the bytes as they lie in flash)? Asked once, before any slot is
    /// written; anything but [`FlashHeld::Held`] refuses the game.
    ///
    /// The flash bytes are never copied: the board checks them where they
    /// are. The default says [`FlashHeld::Missing`], so a board (or a
    /// fake) that knows nothing of flash parts refuses every such game.
    fn flash_part(&mut self, part: &FlashPartHeader) -> FlashHeld {
        let _ = part;
        FlashHeld::Missing
    }
}

/// The board's answer to [`GameSlots::flash_part`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlashHeld {
    /// The firmware holds exactly this part: the game may load.
    Held,
    /// No part at these addresses and sizes (or no flash parts at all).
    Missing,
    /// A part at these addresses and sizes, but other bytes: the CRC-32s
    /// the board found for the code and the data halves.
    Differs {
        /// CRC-32 of the code half the board holds.
        got_code: u32,
        /// CRC-32 of the data half the board holds.
        got_ro: u32,
    },
}
