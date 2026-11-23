//! The home screen's data in the RAM the board lends
//! (`board_api::Board::parts_and_lent`; on the chip, the empty game data
//! slot). The rules of the loan are in `board_api::lent`.
//!
//! What is kept there, as plain bytes (the console has no `unsafe`, so it
//! cannot lay a typed value over borrowed RAM; and any byte is a valid
//! `u8`, so rubbish can never crash it):
//!
//! ```text
//! offset  bytes          what
//! 0       4              the stamp (see below), little-endian
//! 4       32 x 41        the game list (MAX_GAMES entries, GAME_BYTES each)
//! 1,316   6 x 680        the tile pictures (ART_SLOTS, art::STORED_BYTES)
//! 5,396                  = LENT_BYTES, all the home needs
//! ```
//!
//! One game entry (41 bytes): folder (8 bytes, then its length), name
//! (12, length), tag (16, length), tile colour (RGB565, little-endian).
//! A length past its field is cut to the field; a byte that is not a
//! printable letter reads as `?`. So rubbish reads as wrong text, never
//! as a crash or a read outside the RAM.
//!
//! The stamp: each time home reads the list from the card, it writes a
//! new number at offset 0 and keeps the same number in the `Console`.
//! Home reads the lent RAM only when it has a list (no game was started
//! since), and the stamp in the RAM is its own. A board that broke the
//! loan's rules (changed the bytes, or lent other RAM) is caught by the
//! stamp, and home reads the card again instead of showing rubbish.

use crate::art;
use crate::gfx::Text;
use crate::home::{Game, MAX_GAMES, NAME_LEN, TAG_LEN};
use crate::theme::ART_SLOTS;

/// Bytes of one game entry.
pub const GAME_BYTES: usize = (8 + 1) + (NAME_LEN + 1) + (TAG_LEN + 1) + 2;
/// Where the game list starts.
pub const LIST_AT: usize = 4;
/// Bytes of the game list.
pub const LIST_BYTES: usize = MAX_GAMES * GAME_BYTES;
/// Where the tile pictures start.
pub const PICTURES_AT: usize = LIST_AT + LIST_BYTES;
/// Bytes of the tile pictures.
pub const PICTURES_BYTES: usize = ART_SLOTS * art::STORED_BYTES;
/// All the lent RAM home uses. A shorter loan is not used.
pub const LENT_BYTES: usize = PICTURES_AT + PICTURES_BYTES;

/// The stamp in the lent RAM.
pub fn stamp(ram: &[u8]) -> u32 {
    match ram.first_chunk::<4>() {
        Some(b) => u32::from_le_bytes(*b),
        None => 0,
    }
}

/// Writes the stamp.
pub fn set_stamp(ram: &mut [u8], stamp: u32) {
    if let Some(b) = ram.first_chunk_mut::<4>() {
        *b = stamp.to_le_bytes();
    }
}

/// Entry `i` of the list (`i` below [`MAX_GAMES`]; the RAM at least
/// [`LENT_BYTES`]).
fn entry(ram: &[u8], i: usize) -> &[u8; GAME_BYTES] {
    let at = LIST_AT + i * GAME_BYTES;
    // The sizes are checked by the caller; a short slice gives a blank
    // entry instead of a panic.
    ram.get(at..at + GAME_BYTES)
        .and_then(|s| s.first_chunk::<GAME_BYTES>())
        .unwrap_or(&[0; GAME_BYTES])
}

fn entry_mut(ram: &mut [u8], i: usize) -> Option<&mut [u8; GAME_BYTES]> {
    let at = LIST_AT + i * GAME_BYTES;
    ram.get_mut(at..at + GAME_BYTES)
        .and_then(|s| s.first_chunk_mut::<GAME_BYTES>())
}

/// Game `i` of the list, as written by [`put_game`] (or rubbish, read
/// safely).
pub fn game(ram: &[u8], i: usize) -> Game {
    let e = entry(ram, i);
    let (folder, rest) = e.split_at(9);
    let (name, rest) = rest.split_at(NAME_LEN + 1);
    let (tag, rest) = rest.split_at(TAG_LEN + 1);
    Game {
        folder: text(folder),
        name: text(name),
        tag: text(tag),
        tile: u16::from_le_bytes([rest[0], rest[1]]),
    }
}

/// Only the tile colour of game `i` (for the palette).
pub fn tile(ram: &[u8], i: usize) -> u16 {
    let e = entry(ram, i);
    u16::from_le_bytes([e[GAME_BYTES - 2], e[GAME_BYTES - 1]])
}

/// Writes game `i` of the list.
pub fn put_game(ram: &mut [u8], i: usize, g: &Game) {
    let Some(e) = entry_mut(ram, i) else {
        return;
    };
    let (folder, rest) = e.split_at_mut(9);
    let (name, rest) = rest.split_at_mut(NAME_LEN + 1);
    let (tag, rest) = rest.split_at_mut(TAG_LEN + 1);
    put_text(folder, g.folder.as_str());
    put_text(name, g.name.as_str());
    put_text(tag, g.tag.as_str());
    rest.copy_from_slice(&g.tile.to_le_bytes());
}

/// Swaps games `a` and `b` of the list.
pub fn swap_games(ram: &mut [u8], a: usize, b: usize) {
    let (ga, gb) = (game(ram, a), game(ram, b));
    put_game(ram, a, &gb);
    put_game(ram, b, &ga);
}

/// The bytes of picture slot `k` (`k` below [`ART_SLOTS`]).
pub fn picture(ram: &[u8], k: usize) -> Option<&[u8; art::STORED_BYTES]> {
    let at = PICTURES_AT + k * art::STORED_BYTES;
    ram.get(at..at + art::STORED_BYTES)?.first_chunk()
}

/// The same, to write.
pub fn picture_mut(ram: &mut [u8], k: usize) -> Option<&mut [u8; art::STORED_BYTES]> {
    let at = PICTURES_AT + k * art::STORED_BYTES;
    ram.get_mut(at..at + art::STORED_BYTES)?.first_chunk_mut()
}

/// A text field: the bytes, then their count.
fn text<const N: usize>(field: &[u8]) -> Text<N> {
    let (bytes, len) = field.split_at(field.len() - 1);
    let n = usize::from(len[0]).min(N).min(bytes.len());
    Text::of_ascii(&bytes[..n])
}

fn put_text(field: &mut [u8], s: &str) {
    let (bytes, len) = field.split_at_mut(field.len() - 1);
    bytes.fill(0);
    let n = s.len().min(bytes.len());
    bytes[..n].copy_from_slice(&s.as_bytes()[..n]);
    len[0] = n as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_is_as_the_notes_say() {
        assert_eq!(GAME_BYTES, 41);
        assert_eq!(PICTURES_AT, 1316);
        assert_eq!(art::STORED_BYTES, 680);
        assert_eq!(LENT_BYTES, 5396);
    }

    #[test]
    fn a_game_comes_back_as_written() {
        let mut ram = [0xA5u8; LENT_BYTES];
        let g = Game {
            folder: Text::of("LANDER"),
            name: Text::of("MOON LANDER"),
            tag: Text::of("GENTLY DOWN"),
            tile: 0x1234,
        };
        put_game(&mut ram, 31, &g);
        assert_eq!(game(&ram, 31), g);
        assert_eq!(tile(&ram, 31), 0x1234);
        swap_games(&mut ram, 0, 31);
        assert_eq!(game(&ram, 0), g);
    }

    #[test]
    fn rubbish_reads_as_text_and_never_panics() {
        let mut ram = [0u8; LENT_BYTES];
        let mut x = 7u32;
        for b in ram.iter_mut() {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            *b = (x >> 16) as u8;
        }
        for i in 0..MAX_GAMES {
            let g = game(&ram, i);
            assert!(g.folder.as_str().len() <= 8);
            assert!(g.name.as_str().len() <= NAME_LEN);
            assert!(g.tag.as_str().len() <= TAG_LEN);
        }
        // A short loan gives blanks, not a panic.
        assert_eq!(game(&ram[..10], 3), game(&[0; GAME_BYTES + 4], 0));
        assert!(picture(&ram[..100], 0).is_none());
    }
}
