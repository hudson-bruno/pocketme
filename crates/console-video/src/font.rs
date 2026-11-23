//! A small pixel font: capital letters, digits and a few signs, 5 pixels
//! tall. It is the font of the design page, letter for letter.
//!
//! Letters have their own width (1 to 5 pixels; "I" is 3, "M" is 5), with
//! one empty column after each. Small letters are drawn as capitals. A
//! sign the font lacks is drawn as "?".
//!
//! Memory: 64 letters x 4 bytes = 256 bytes, in flash on the chip.

/// Height of every letter in pixels.
pub const FONT_H: u16 = 5;
/// Suggested distance from one line of text to the next (one empty row).
pub const LINE_H: u16 = 6;

/// One letter packed in 32 bits: row `j` in bits `5j..5j+5` (bit `i` is
/// column `i`, from the left), the width in bits 28..32.
pub type Glyph = u32;

/// Packs a letter written as rows of `#` (ink) and `.` (empty),
/// separated by `/`, as on the design page. Runs while compiling.
const fn glyph(s: &str) -> Glyph {
    let b = s.as_bytes();
    let (mut row, mut col, mut width, mut bits) = (0u32, 0u32, 0u32, 0u32);
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' => {
                row += 1;
                col = 0;
            }
            b'#' => {
                bits |= 1 << (row * 5 + col);
                col += 1;
            }
            b'.' => col += 1,
            _ => panic!("font: only # . and / are allowed"),
        }
        if row == 0 {
            width = col;
        }
        i += 1;
    }
    assert!(row == 4, "font: a letter has 5 rows");
    assert!(width >= 1 && width <= 5, "font: 1 to 5 pixels wide");
    bits | (width << 28)
}

/// The letters for ASCII 32 (space) to 95 (`_`). 0 means "not in the
/// font".
const TABLE: [Glyph; 64] = {
    const FONT: [(u8, &str); 55] = [
        (b'A', ".##./#..#/####/#..#/#..#"),
        (b'B', "###./#..#/###./#..#/###."),
        (b'C', ".###/#.../#.../#.../.###"),
        (b'D', "###./#..#/#..#/#..#/###."),
        (b'E', "####/#.../###./#.../####"),
        (b'F', "####/#.../###./#.../#..."),
        (b'G', ".###/#.../#.##/#..#/.###"),
        (b'H', "#..#/#..#/####/#..#/#..#"),
        (b'I', "###/.#./.#./.#./###"),
        (b'J', "..##/...#/...#/#..#/.##."),
        (b'K', "#..#/#.#./##../#.#./#..#"),
        (b'L', "#.../#.../#.../#.../####"),
        (b'M', "#...#/##.##/#.#.#/#...#/#...#"),
        (b'N', "#..#/##.#/#.##/#..#/#..#"),
        (b'O', ".##./#..#/#..#/#..#/.##."),
        (b'P', "###./#..#/###./#.../#..."),
        (b'Q', ".##./#..#/#..#/#.#./.#.#"),
        (b'R', "###./#..#/###./#.#./#..#"),
        (b'S', ".###/#.../.##./...#/###."),
        (b'T', "###/.#./.#./.#./.#."),
        (b'U', "#..#/#..#/#..#/#..#/.##."),
        (b'V', "#...#/#...#/.#.#./.#.#./..#.."),
        (b'W', "#...#/#...#/#.#.#/##.##/#...#"),
        (b'X', "#..#/#..#/.##./#..#/#..#"),
        (b'Y', "#.#/#.#/.#./.#./.#."),
        (b'Z', "####/...#/.##./#.../####"),
        (b'0', "###/#.#/#.#/#.#/###"),
        (b'1', ".#./##./.#./.#./###"),
        (b'2', "###/..#/###/#../###"),
        (b'3', "###/..#/###/..#/###"),
        (b'4', "#.#/#.#/###/..#/..#"),
        (b'5', "###/#../###/..#/###"),
        (b'6', "###/#../###/#.#/###"),
        (b'7', "###/..#/..#/..#/..#"),
        (b'8', "###/#.#/###/#.#/###"),
        (b'9', "###/#.#/###/..#/###"),
        (b' ', "../../../../.."),
        // The design page writes "./././././#": six rows, so its loop of
        // five rows never draws the dot. Fixed here.
        (b'.', "././././#"),
        (b',', "../../../.#/#."),
        (b':', "./#/./#/."),
        (b'!', "#/#/#/./#"),
        (b'?', "###/..#/.#./.../.#."),
        (b'-', ".../.../###/.../..."),
        (b'+', ".../.#./###/.#./..."),
        (b'/', "..#/..#/.#./#../#.."),
        (b'%', "#.#/..#/.#./#../#.#"),
        (b'(', ".#/#./#./#./.#"),
        (b')', "#./.#/.#/.#/#."),
        (b'\'', "#/#/././."),
        (b'>', "#../.#./..#/.#./#.."),
        (b'<', "..#/.#./#../.#./..#"),
        // Not on the design page; added in the same style.
        (b'=', ".../###/.../###/..."),
        (b'_', ".../.../.../.../###"),
        (b'*', ".../#.#/.#./#.#/..."),
        (b'#', ".#.#./#####/.#.#./#####/.#.#."),
    ];
    let mut t = [0; 64];
    let mut i = 0;
    while i < FONT.len() {
        t[(FONT[i].0 - 32) as usize] = glyph(FONT[i].1);
        i += 1;
    }
    t
};

/// The letter for `c`: small letters as capitals, missing signs as `?`.
pub(crate) fn glyph_of(c: char) -> Glyph {
    // Every sign the font has is ASCII; any other one is a missing sign.
    glyph_for(if c.is_ascii() { c as u8 } else { 0 })
}

/// The font's letter for the byte `c`, as [`crate::Frame::text`] draws
/// it: small letters as capitals, missing signs as `?`. With
/// [`glyph_w`] and [`glyph_ink`], for code that builds its own tables
/// from a text while compiling (console-core's boot animation does, so
/// it needs no copy of the font).
pub const fn glyph_for(c: u8) -> Glyph {
    let c = c.to_ascii_uppercase();
    let g = if c >= b' ' && c <= b'_' {
        TABLE[(c - 32) as usize]
    } else {
        0
    };
    if g == 0 {
        TABLE[(b'?' - 32) as usize]
    } else {
        g
    }
}

/// Width of a letter in pixels, without the empty column after it.
pub const fn glyph_w(g: Glyph) -> u16 {
    (g >> 28) as u16
}

/// Is pixel `(i, j)` of the letter ink?
#[inline(always)]
pub const fn glyph_ink(g: Glyph, i: u16, j: u16) -> bool {
    g >> (j * 5 + i) & 1 != 0
}

/// Width of `text` in pixels at size 1: every letter plus one empty
/// column between letters (none after the last).
pub fn text_width(text: &str) -> u16 {
    // Saturating: a very long text gives u16::MAX instead of a panic
    // (debug builds) or a wrong, wrapped width (release builds).
    let w = text
        .chars()
        .fold(0u16, |w, c| w.saturating_add(glyph_w(glyph_of(c)) + 1));
    w.saturating_sub(1)
}
