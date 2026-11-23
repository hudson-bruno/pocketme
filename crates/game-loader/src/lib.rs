//! The game loader: everything about loading a game file, except touching
//! real memory.
//!
//! A game file (`GAME.BIN` on the card) is a header of nine numbers, then
//! the game's code, then its data (see `game_api::GameHeader`). A game
//! with a flash part ("GAMF") has eight more numbers between the header
//! and the code (`game_api::FlashPartHeader`): the part itself is in the
//! firmware, and the board is asked whether it holds it
//! ([`GameSlots::flash_part`]) before anything is written. The
//! loader reads it from the card in small pieces ([`PIECE`] bytes at a
//! time), checks it, and hands each piece to the board through
//! [`GameSlots`]. The board puts the bytes into the two slots. The loader
//! never writes memory itself, so it needs no `unsafe` and runs the same
//! on the PC, where the tests give it a pretend pair of slots.
//!
//! Use [`load`]. It returns a [`Loaded`] only when every check passed;
//! that is the one way to learn where to start the game. When it returns
//! an error, the slots may hold rubbish, and nothing may be started.
//!
//! Memory: no heap, nothing kept between calls. One load uses about
//! [`PIECE`] bytes of stack for the piece being read, plus a few hundred
//! more (the crate README has the numbers measured for the chip).
//!
//! The order of work, and why it is safe when a file turns out to be
//! bad half way, is written at [`load`].

#![no_std]
#![forbid(unsafe_code)]

mod error;
mod load;
mod slots;

pub use error::{LoadError, NOTICE_MAX};
pub use load::{LoadStats, Loaded, check_file, check_header, load};
pub use slots::{FlashHeld, GameSlots, Slots};

/// Bytes read from the card at a time. The loader keeps one buffer of
/// this size on the stack, so it is kept small: the stack is precious.
///
/// 512 is one block of the SD card. The loader also lines its reads up
/// with the blocks: after the 36-byte header (and the 32-byte flash
/// part extension of a "GAMF" file), every read starts at a
/// multiple of 512 in the file, so the card never has to read one block
/// twice for two pieces.
///
/// Must be a multiple of 4 (code goes in as whole 32-bit words) and at
/// least the header's 36 bytes. Checked while compiling.
pub const PIECE: usize = 512;

const _: () = assert!(PIECE.is_multiple_of(4) && PIECE >= game_api::GameHeader::SIZE);
