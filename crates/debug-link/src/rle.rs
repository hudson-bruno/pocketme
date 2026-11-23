//! Run-length packing of pixels.
//!
//! Pixel art is mostly flat areas, so runs of the same pixel shrink it
//! well, and nothing cleverer is needed. A pixel ("element") is one byte
//! for the indexed framebuffer and two for RGB565.
//!
//! The packed form is a row of tokens. Each starts with a control byte:
//!
//! * `0x00..=0x7F`: the next `control + 1` pixels follow as they are.
//! * `0x80..=0xFF`: the one pixel that follows is repeated
//!   `control - 0x80 + 2` times (2 to 129).

/// Longest run of equal pixels in one token.
const MAX_RUN: usize = 129;
/// Most pixels in one "as they are" token.
const MAX_LITERAL: usize = 128;

/// How many pixels starting at `at` are equal to the one at `at`, up to
/// `MAX_RUN`.
fn run_length(elem: usize, input: &[u8], at: usize) -> usize {
    let first = &input[at * elem..(at + 1) * elem];
    let pixels = input.len() / elem;
    let mut run = 1;
    while run < MAX_RUN && at + run < pixels && &input[(at + run) * elem..(at + run + 1) * elem] == first {
        run += 1;
    }
    run
}

/// Packs as many pixels of `input` as fit into `out`.
///
/// `elem` is the pixel size in bytes (1 or 2) and `input` a whole number
/// of pixels. Returns `(pixels packed, bytes written)`. When fewer pixels
/// were packed than given, `out` is full: send it and call again with the
/// rest. The output of each call can be unpacked by itself.
pub fn encode(elem: usize, input: &[u8], out: &mut [u8]) -> (usize, usize) {
    debug_assert!(elem == 1 || elem == 2);
    debug_assert!(input.len() % elem == 0);
    let pixels = input.len() / elem;
    // A run of two saves a byte with 2-byte pixels, and nothing with
    // 1-byte pixels.
    let worth_a_run = if elem == 1 { 3 } else { 2 };
    let (mut at, mut written) = (0, 0);

    while at < pixels {
        let run = run_length(elem, input, at);
        if run >= worth_a_run {
            if out.len() - written < 1 + elem {
                break;
            }
            out[written] = 0x80 + (run - 2) as u8;
            out[written + 1..written + 1 + elem].copy_from_slice(&input[at * elem..(at + 1) * elem]);
            written += 1 + elem;
            at += run;
            continue;
        }

        // Pixels as they are, up to where the next run worth having
        // begins.
        let mut count = 1;
        while count < MAX_LITERAL && at + count < pixels && run_length(elem, input, at + count) < worth_a_run {
            count += 1;
        }
        let room = (out.len() - written).saturating_sub(1) / elem;
        if room == 0 {
            break;
        }
        let count = count.min(room);
        out[written] = (count - 1) as u8;
        out[written + 1..written + 1 + count * elem].copy_from_slice(&input[at * elem..(at + count) * elem]);
        written += 1 + count * elem;
        at += count;
    }
    (at, written)
}

/// The packed bytes do not describe pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// A token is cut off.
    Truncated,
    /// More pixels than the output has room for.
    Overflow,
}

/// Unpacks `input` into `out` and returns the number of bytes written.
pub fn decode(elem: usize, input: &[u8], out: &mut [u8]) -> Result<usize, DecodeError> {
    let (mut at, mut written) = (0, 0);
    while at < input.len() {
        let control = input[at];
        at += 1;
        if control < 0x80 {
            let bytes = (usize::from(control) + 1) * elem;
            let pixels = input.get(at..at + bytes).ok_or(DecodeError::Truncated)?;
            out.get_mut(written..written + bytes).ok_or(DecodeError::Overflow)?.copy_from_slice(pixels);
            at += bytes;
            written += bytes;
        } else {
            let count = usize::from(control - 0x80) + 2;
            let pixel = input.get(at..at + elem).ok_or(DecodeError::Truncated)?;
            let room = out.get_mut(written..written + count * elem).ok_or(DecodeError::Overflow)?;
            for copy in room.chunks_exact_mut(elem) {
                copy.copy_from_slice(pixel);
            }
            at += elem;
            written += count * elem;
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::{vec, vec::Vec};

    use super::*;

    /// Packs all of `input` through an output buffer of `chunk` bytes and
    /// unpacks each piece again.
    fn round_trip(elem: usize, input: &[u8], chunk: usize) -> (Vec<u8>, usize) {
        let mut rest = input;
        let mut unpacked = Vec::new();
        let mut packed_total = 0;
        while !rest.is_empty() {
            let mut out = vec![0u8; chunk];
            let (pixels, written) = encode(elem, rest, &mut out);
            assert!(pixels > 0, "no progress with {chunk} bytes of room");
            packed_total += written;
            let mut piece = vec![0u8; pixels * elem];
            assert_eq!(decode(elem, &out[..written], &mut piece), Ok(pixels * elem));
            unpacked.extend(piece);
            rest = &rest[pixels * elem..];
        }
        (unpacked, packed_total)
    }

    #[test]
    fn flat_row_becomes_tiny() {
        // 320 pixels of one RGB565 colour: three tokens of three bytes.
        let row: Vec<u8> = [0x1F, 0xF8].repeat(320);
        let (back, packed) = round_trip(2, &row, 1024);
        assert_eq!(back, row);
        assert_eq!(packed, 9);
    }

    #[test]
    fn noise_grows_by_under_one_percent() {
        let mut state = 99u32;
        let row: Vec<u8> = (0..640)
            .map(|_| {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                (state >> 16) as u8
            })
            .collect();
        for elem in [1, 2] {
            let (back, packed) = round_trip(elem, &row, 1024);
            assert_eq!(back, row);
            assert!(packed <= row.len() + row.len() / 100 + 2, "{packed}");
        }
    }

    #[test]
    fn mixed_runs_and_single_pixels() {
        let row = [1, 1, 1, 1, 2, 3, 3, 4, 5, 5, 5, 6, 6, 6, 6, 6, 6, 7];
        for elem in [1, 2] {
            for chunk in [3, 4, 5, 7, 64] {
                assert_eq!(round_trip(elem, &row, chunk).0, row, "elem {elem} chunk {chunk}");
            }
        }
    }

    #[test]
    fn runs_longer_than_one_token() {
        for len in [1, 2, 3, 128, 129, 130, 131, 258, 259, 1000] {
            let row = vec![0xABu8; len];
            assert_eq!(round_trip(1, &row, 64).0, row, "len {len}");
            if len % 2 == 0 {
                assert_eq!(round_trip(2, &row, 64).0, row, "len {len}");
            }
        }
    }

    #[test]
    fn output_too_small_for_anything_packs_nothing() {
        assert_eq!(encode(2, &[1, 2, 3, 4], &mut [0u8; 2]), (0, 0));
        assert_eq!(encode(1, &[1, 2], &mut [0u8; 1]), (0, 0));
        assert_eq!(encode(1, &[], &mut [0u8; 8]), (0, 0));
    }

    #[test]
    fn bad_input_is_refused() {
        let mut out = [0u8; 8];
        // Says three pixels follow, has one.
        assert_eq!(decode(1, &[0x02, 0x55], &mut out), Err(DecodeError::Truncated));
        // A run with its pixel missing.
        assert_eq!(decode(2, &[0x80, 0x55], &mut out), Err(DecodeError::Truncated));
        // A run of 129 into 8 bytes.
        assert_eq!(decode(1, &[0xFF, 0x55], &mut out), Err(DecodeError::Overflow));
    }
}
