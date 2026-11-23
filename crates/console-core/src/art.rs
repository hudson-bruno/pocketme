//! Tile pictures ("splash art"): one small picture per game, in the
//! game's card folder as `ART.BIN`, drawn on its home tile instead of
//! the coloured box with a letter.
//!
//! The file (version 1), every number little-endian:
//!
//! ```text
//! offset  size  what
//! 0       3     "ART"
//! 3       1     format version: 1
//! 4       1     width  (must be theme::ART_W, 36, today)
//! 5       1     height (must be theme::ART_H, 36, today)
//! 6       1     how many colours, n: 1 to 16
//! 7       1     bits per pixel: 4
//! 8       2n    the colours, RGB565
//! 8+2n    ...   the pixels, row by row from the top, ceil(width / 2)
//!               bytes a row, two pixels a byte, the left one in the high
//!               4 bits; each a colour number below n
//! ```
//!
//! So 36 x 36 with 16 colours is 8 + 32 + 648 = 688 bytes. The file must
//! be exactly as long as its header says. Anything else (no file, a
//! short or long file, another version or size, a colour number past n,
//! a read error) gives [`ArtError`], and the tile stays the letter tile.
//! The size is checked before anything is read, and every read is into
//! a fixed buffer: a damaged or hostile file cannot make the console
//! read more, wait longer, or crash. `games/art/make_art.py` makes the
//! file from a text picture.
//!
//! Memory: the pictures are kept in the RAM the board lends while no game
//! is loaded (see `lent`): [`ART_SLOTS`] of [`STORED_BYTES`] each (4,080
//! bytes with 6). The console itself keeps only [`Art`]: which game is in
//! which slot, and one bit per game for "has no usable picture" (12
//! bytes). Which pictures are kept, and when the card is read, is home's
//! choice (see `home`).

use board_api::{Files, FsError};
use console_video::{Frame, Palette};

use crate::lent;
use crate::theme::{ART_COLOURS, ART_EDGE, ART_FIRST, ART_H, ART_SLOTS, ART_W, LINE};

/// The file's name in a game's card folder.
pub const FILE: &str = "ART.BIN";
/// The first three bytes.
pub const MAGIC: [u8; 3] = *b"ART";
/// The format version this console reads.
pub const VERSION: u8 = 1;
/// Header bytes before the colours.
pub const HEADER: usize = 8;
/// Bits per pixel (version 1 has only 4).
pub const BITS: u8 = 4;
const W: usize = ART_W as usize;
const H: usize = ART_H as usize;
/// Bytes of one pixel row.
pub const ROW_BYTES: usize = W.div_ceil(2);
/// Bytes of all pixels.
pub const PIXEL_BYTES: usize = ROW_BYTES * H;
/// The longest file read: 16 colours.
pub const FILE_MAX: usize = HEADER + 2 * ART_COLOURS + PIXEL_BYTES;
/// Bytes of one picture as kept in the lent RAM: the 16 colours (RGB565,
/// little-endian; the ones past the file's colours are 0), then the
/// packed pixels as in the file.
pub const STORED_BYTES: usize = 2 * ART_COLOURS + PIXEL_BYTES;

/// Why a tile picture was not used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtError {
    /// No ART.BIN: a normal case (the letter tile is shown).
    Missing,
    /// The card gave an error.
    Unreadable,
    /// The file ended before its header or its pixels.
    Short,
    /// It does not start with "ART".
    NotArt,
    /// Another format version.
    Version,
    /// Another width or height than this console's tiles.
    Size,
    /// No colours, or more than 16.
    Colours,
    /// Not 4 bits per pixel.
    Depth,
    /// The file is not exactly as long as its header says.
    Length,
    /// A pixel names a colour past the file's colours.
    Pixel,
}

impl ArtError {
    /// A short word for the log ("home: picture refused: ... (short)").
    pub fn word(self) -> &'static str {
        match self {
            ArtError::Missing => "missing",
            ArtError::Unreadable => "card error",
            ArtError::Short => "short",
            ArtError::NotArt => "not ART",
            ArtError::Version => "version",
            ArtError::Size => "size",
            ArtError::Colours => "colours",
            ArtError::Depth => "depth",
            ArtError::Length => "length",
            ArtError::Pixel => "pixel",
        }
    }
}

/// One tile picture in memory: its colours and its packed pixels.
#[derive(Clone, Copy)]
pub struct Picture {
    /// RGB565; the ones past the file's colours are 0.
    pub colours: [u16; ART_COLOURS],
    pixels: [u8; PIXEL_BYTES],
}

impl Default for Picture {
    fn default() -> Self {
        Self::new()
    }
}

impl Picture {
    pub const fn new() -> Self {
        Picture {
            colours: [0; ART_COLOURS],
            pixels: [0; PIXEL_BYTES],
        }
    }

    /// The colour number of pixel `(x, y)` (0 to 15).
    pub fn at(&self, x: usize, y: usize) -> u8 {
        nibble(&self.pixels, x, y)
    }

    /// The picture from its kept bytes ([`STORED_BYTES`]).
    pub fn from_stored(s: &[u8; STORED_BYTES]) -> Self {
        let mut pic = Picture::new();
        take_colours(&s[..2 * ART_COLOURS], &mut pic);
        pic.pixels.copy_from_slice(&s[2 * ART_COLOURS..]);
        pic
    }
}

/// Colour number of pixel `(x, y)` in packed pixels.
fn nibble(pixels: &[u8], x: usize, y: usize) -> u8 {
    let b = pixels[y * ROW_BYTES + x / 2];
    if x.is_multiple_of(2) {
        b >> 4
    } else {
        b & 0x0F
    }
}

/// Colour number of pixel `(x, y)` of a kept picture.
fn stored_at(s: &[u8; STORED_BYTES], x: usize, y: usize) -> u8 {
    nibble(&s[2 * ART_COLOURS..], x, y)
}

/// Colour `c` (0 to 15) of a kept picture.
fn stored_colour(s: &[u8; STORED_BYTES], c: usize) -> u16 {
    u16::from_le_bytes([s[2 * c], s[2 * c + 1]])
}

/// Checks a header against the file's size. Returns the number of
/// colours.
pub fn check_header(h: &[u8; HEADER], file_size: u32) -> Result<usize, ArtError> {
    if h[..3] != MAGIC {
        return Err(ArtError::NotArt);
    }
    if h[3] != VERSION {
        return Err(ArtError::Version);
    }
    if usize::from(h[4]) != W || usize::from(h[5]) != H {
        return Err(ArtError::Size);
    }
    let n = usize::from(h[6]);
    if n == 0 || n > ART_COLOURS {
        return Err(ArtError::Colours);
    }
    if h[7] != BITS {
        return Err(ArtError::Depth);
    }
    if file_size as usize != HEADER + 2 * n + PIXEL_BYTES {
        return Err(ArtError::Length);
    }
    Ok(n)
}

/// Reads and checks the picture at `path` into `pic` (tests, tools; home
/// reads straight into the lent RAM with [`read_stored`]).
pub fn read<F: Files>(files: &mut F, path: &str, pic: &mut Picture) -> Result<(), ArtError> {
    let mut s = [0u8; STORED_BYTES];
    let got = read_stored(files, path, &mut s);
    *pic = Picture::from_stored(&s);
    got
}

/// Reads and checks the picture at `path` into `out`, kept as
/// [`STORED_BYTES`] bytes. Reads at most [`FILE_MAX`] bytes in all, and
/// only after the size was checked. On an error `out` holds rubbish (the
/// caller does not use it).
pub fn read_stored<F: Files>(
    files: &mut F,
    path: &str,
    out: &mut [u8; STORED_BYTES],
) -> Result<(), ArtError> {
    let size = match files.size(path) {
        Ok(s) => s,
        Err(FsError::NotFound) => return Err(ArtError::Missing),
        Err(_) => return Err(ArtError::Unreadable),
    };
    if (size as usize) < HEADER {
        return Err(ArtError::Short);
    }
    let mut head = [0u8; HEADER];
    read_full(files, path, 0, &mut head)?;
    let n = check_header(&head, size)?;
    let (colours, pixels) = out.split_at_mut(2 * ART_COLOURS);
    colours.fill(0);
    read_full(files, path, HEADER as u32, &mut colours[..2 * n])?;
    read_full(files, path, (HEADER + 2 * n) as u32, pixels)?;
    check_packed(pixels, n)
}

/// The same checks on a whole file in memory (tests, tools).
pub fn parse(bytes: &[u8], pic: &mut Picture) -> Result<(), ArtError> {
    let head: &[u8; HEADER] = bytes.first_chunk().ok_or(ArtError::Short)?;
    let size = u32::try_from(bytes.len()).map_err(|_| ArtError::Length)?;
    let n = check_header(head, size)?;
    take_colours(&bytes[HEADER..HEADER + 2 * n], pic);
    pic.pixels
        .copy_from_slice(&bytes[HEADER + 2 * n..HEADER + 2 * n + PIXEL_BYTES]);
    check_pixels(pic, n)
}

fn take_colours(raw: &[u8], pic: &mut Picture) {
    pic.colours = [0; ART_COLOURS];
    for (c, b) in pic.colours.iter_mut().zip(raw.as_chunks::<2>().0) {
        *c = u16::from_le_bytes(*b);
    }
}

fn check_pixels(pic: &Picture, n: usize) -> Result<(), ArtError> {
    check_packed(&pic.pixels, n)
}

fn check_packed(pixels: &[u8], n: usize) -> Result<(), ArtError> {
    for y in 0..H {
        for x in 0..W {
            if usize::from(nibble(pixels, x, y)) >= n {
                return Err(ArtError::Pixel);
            }
        }
    }
    Ok(())
}

/// Fills `buf` from byte `offset` of the file. A read error, or the file
/// ending early, is an error. At most `buf.len()` calls (each brings at
/// least one byte).
fn read_full<F: Files>(
    files: &mut F,
    path: &str,
    offset: u32,
    buf: &mut [u8],
) -> Result<(), ArtError> {
    let mut got = 0;
    while got < buf.len() {
        let n = files
            .read_at(path, offset + got as u32, &mut buf[got..])
            .map_err(|_| ArtError::Unreadable)?;
        if n == 0 {
            return Err(ArtError::Short);
        }
        got += n.min(buf.len() - got);
    }
    Ok(())
}

/// Bytes one kept picture costs in the lent RAM.
pub const SLOT_BYTES: usize = STORED_BYTES;

/// Which pictures home keeps (the pictures themselves are in the lent
/// RAM, see `lent`), and which games have none. 12 bytes.
pub struct Art {
    /// Per slot: the game's place in home's list, plus one; 0: the slot
    /// is free.
    slots: [u8; ART_SLOTS],
    /// Bit `i`: game `i`'s picture was tried and is missing or bad. It
    /// is not tried again until the list is read again.
    none: u32,
}

impl Default for Art {
    fn default() -> Self {
        Self::new()
    }
}

impl Art {
    pub const fn new() -> Self {
        Art {
            slots: [0; ART_SLOTS],
            none: 0,
        }
    }

    /// Forgets every picture (the game list changed, or the lent RAM
    /// went to a game).
    pub fn forget(&mut self) {
        self.slots = [0; ART_SLOTS];
        self.none = 0;
    }

    /// The slot that keeps game `game`'s picture, if one does.
    pub fn slot(&self, game: usize) -> Option<usize> {
        self.slots.iter().position(|&s| usize::from(s) == game + 1)
    }

    /// Whether game `game` was tried and has no usable picture.
    pub fn has_none(&self, game: usize) -> bool {
        game < 32 && self.none & (1 << game) != 0
    }

    /// Whether nothing more is to be done for `game`: its picture is
    /// kept, or it has none.
    pub fn known(&self, game: usize) -> bool {
        self.has_none(game) || self.slot(game).is_some()
    }

    /// Reads game `game`'s picture from `path` into a free slot of the
    /// lent RAM `ram`, or into the slot of a game not in `keep`. Puts its
    /// colours in the palette. Returns what the read gave (`None`: no
    /// slot could be freed, or the RAM is too short; the card was not
    /// read).
    pub fn load<F: Files>(
        &mut self,
        files: &mut F,
        path: &str,
        game: usize,
        keep: &[usize],
        p: &mut Palette,
        ram: &mut [u8],
    ) -> Option<Result<(), ArtError>> {
        if game >= 32 {
            return None;
        }
        let k = self.slots.iter().position(|&s| s == 0).or_else(|| {
            self.slots
                .iter()
                .position(|&s| !keep.contains(&(usize::from(s) - 1)))
        })?;
        let stored = lent::picture_mut(ram, k)?;
        self.slots[k] = 0;
        let got = read_stored(files, path, stored);
        match got {
            Ok(()) => {
                self.slots[k] = game as u8 + 1;
                Self::slot_colours(k, stored, p);
            }
            Err(_) => self.none |= 1 << game,
        }
        Some(got)
    }

    /// Puts every kept picture's colours in the palette (after a game,
    /// which owned these numbers, ended).
    pub fn set_colours(&self, p: &mut Palette, ram: &[u8]) {
        for (k, &s) in self.slots.iter().enumerate() {
            if s != 0
                && let Some(stored) = lent::picture(ram, k)
            {
                Self::slot_colours(k, stored, p);
            }
        }
    }

    fn slot_colours(k: usize, stored: &[u8; STORED_BYTES], p: &mut Palette) {
        let first = usize::from(ART_FIRST) + k * ART_COLOURS;
        for c in 0..ART_COLOURS {
            p.set((first + c) as u8, stored_colour(stored, c));
        }
    }
}

/// Draws the picture kept in slot `k` (its bytes `stored`), with its
/// top-left at `(x, y)` (cut at the picture's edges).
pub fn draw(f: &mut Frame, k: usize, stored: &[u8; STORED_BYTES], x: i16, y: i16) {
    let first = usize::from(ART_FIRST) + k * ART_COLOURS;
    for py in 0..H {
        for px in 0..W {
            let c = (first + usize::from(stored_at(stored, px, py))) as u8;
            f.pixel(x + px as i16, y + py as i16, c);
        }
    }
    if ART_EDGE {
        f.outline(x, y, ART_W, ART_H, LINE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A good file: `n` colours, pixel (x, y) = (x + y) % n.
    fn good(n: usize) -> [u8; FILE_MAX] {
        let mut b = [0u8; FILE_MAX];
        b[..3].copy_from_slice(&MAGIC);
        b[3..8].copy_from_slice(&[VERSION, W as u8, H as u8, n as u8, BITS]);
        for c in 0..n {
            b[HEADER + 2 * c..HEADER + 2 * c + 2]
                .copy_from_slice(&(0x1000 * c as u16).to_le_bytes());
        }
        let px = HEADER + 2 * n;
        for y in 0..H {
            for x in 0..W {
                let v = ((x + y) % n) as u8;
                let i = px + y * ROW_BYTES + x / 2;
                b[i] |= if x % 2 == 0 { v << 4 } else { v };
            }
        }
        b
    }

    #[test]
    fn a_good_file_is_read() {
        let b = good(16);
        let mut pic = Picture::new();
        assert_eq!(parse(&b, &mut pic), Ok(()));
        assert_eq!(pic.at(3, 4), 7);
        assert_eq!(pic.colours[2], 0x2000);
        let short = good(3);
        let len = HEADER + 6 + PIXEL_BYTES;
        assert_eq!(parse(&short[..len], &mut pic), Ok(()));
        assert_eq!(pic.colours[3], 0, "colours past n are black");
    }

    #[test]
    fn header_faults_are_named() {
        let b = good(16);
        let mut pic = Picture::new();
        let cases: [(usize, u8, ArtError); 7] = [
            (0, b'X', ArtError::NotArt),
            (3, 2, ArtError::Version),
            (4, 35, ArtError::Size),
            (5, 72, ArtError::Size),
            (6, 0, ArtError::Colours),
            (6, 17, ArtError::Colours),
            (7, 8, ArtError::Depth),
        ];
        for (at, v, want) in cases {
            let mut c = b;
            c[at] = v;
            assert_eq!(parse(&c, &mut pic), Err(want), "byte {at} = {v}");
        }
        // Fewer colours named than the file holds: the length is wrong.
        let mut c = b;
        c[6] = 15;
        assert_eq!(parse(&c, &mut pic), Err(ArtError::Length));
        let mut c = good(3);
        c[HEADER + 6] = 0x30; // colour 3 of 3
        assert_eq!(
            parse(&c[..HEADER + 6 + PIXEL_BYTES], &mut pic),
            Err(ArtError::Pixel)
        );
    }

    #[test]
    fn a_slot_costs_what_the_notes_say() {
        assert_eq!(PIXEL_BYTES, 648);
        assert_eq!(FILE_MAX, 688);
        assert_eq!(SLOT_BYTES, 680);
        assert_eq!(size_of::<Art>(), 12);
    }

    #[test]
    fn a_kept_picture_is_the_file() {
        let b = good(16);
        let mut pic = Picture::new();
        parse(&b, &mut pic).unwrap();
        let mut s = [0u8; STORED_BYTES];
        for (c, rgb) in pic.colours.iter().enumerate() {
            s[2 * c..2 * c + 2].copy_from_slice(&rgb.to_le_bytes());
        }
        s[2 * ART_COLOURS..].copy_from_slice(&b[HEADER + 32..]);
        let back = Picture::from_stored(&s);
        assert_eq!(back.colours, pic.colours);
        for y in 0..H {
            for x in 0..W {
                assert_eq!(stored_at(&s, x, y), pic.at(x, y));
            }
        }
    }
}
