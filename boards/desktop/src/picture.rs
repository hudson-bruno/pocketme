//! Pictures: save as PNG, a short fingerprint, and a difference picture.
//! No outside library: the PNG writer is about 40 lines.

use std::io;
use std::path::Path;

/// RGB565 to 8-bit red, green, blue. The top bits are copied into the
/// empty low bits, so full white stays 255 and black stays 0.
pub fn to_rgb888(c: u16) -> [u8; 3] {
    let r = (c >> 11) as u8 & 0x1F;
    let g = (c >> 5) as u8 & 0x3F;
    let b = c as u8 & 0x1F;
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}

/// Turns an RGB565 picture into the bytes of a PNG file.
///
/// A PNG file is: a fixed start, a header with the size, the pixels
/// packed with "zlib", and an end mark. Here the pixels are stored but
/// not made smaller ("stored" blocks), which keeps the code short and
/// exact. A 320x240 picture is about 230 KB.
pub fn png_bytes(pixels: &[u16], w: usize, h: usize) -> Vec<u8> {
    assert_eq!(pixels.len(), w * h);
    let rgb: Vec<u8> = pixels.iter().flat_map(|&c| to_rgb888(c)).collect();
    png_bytes_rgb888(&rgb, w, h)
}

/// The same, from 8-bit red, green, blue bytes (3 per pixel, row by row).
/// Used for what the window shows, which is already 8-bit.
pub fn png_bytes_rgb888(rgb: &[u8], w: usize, h: usize) -> Vec<u8> {
    assert_eq!(rgb.len(), w * h * 3);
    // Each row starts with a 0 ("no filter"), then red, green, blue.
    let mut raw = Vec::with_capacity(h * (1 + w * 3));
    for row in rgb.chunks_exact(w * 3) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01]; // zlib header
    let mut blocks = raw.chunks(65_535).peekable();
    while let Some(block) = blocks.next() {
        z.push(u8::from(blocks.peek().is_none())); // 1 on the last block
        let len = block.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32); // Adler-32 checksum of the raw rows
    for &x in &raw {
        a = (a + x as u32) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend_from_slice(&(w as u32).to_be_bytes());
    header.extend_from_slice(&(h as u32).to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]); // 8 bits per colour, RGB
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Writes an RGB565 picture as a PNG file, making its folder if needed.
pub fn save_png(path: &Path, pixels: &[u16], w: usize, h: usize) -> io::Result<()> {
    write_file(path, &png_bytes(pixels, w, h))
}

/// Writes 8-bit red, green, blue pixels as a PNG file.
pub fn save_png_rgb888(path: &Path, rgb: &[u8], w: usize, h: usize) -> io::Result<()> {
    write_file(path, &png_bytes_rgb888(rgb, w, h))
}

fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, bytes)
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// The common CRC-32 (the one PNG, zip and game files use).
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &byte in data {
        c ^= byte as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// A short fingerprint of a picture (FNV-1a, 64 bits). Two pictures with
/// the same fingerprint are, in practice, the same picture. Tests keep
/// fingerprints instead of whole pictures.
pub fn fingerprint(pixels: &[u16]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &p in pixels {
        for byte in p.to_le_bytes() {
            h ^= byte as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    }
    h
}

/// Compares two pictures. Returns how many pixels differ, and a picture
/// that is the first one darkened where they match, and bright magenta
/// where they differ.
pub fn difference(a: &[u16], b: &[u16]) -> (usize, Vec<u16>) {
    let mut count = 0;
    let picture = a
        .iter()
        .zip(b)
        .map(|(&x, &y)| {
            if x == y {
                (x >> 2) & 0x39E7 // a quarter of the brightness
            } else {
                count += 1;
                0xF81F
            }
        })
        .collect();
    (count, picture)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_widen_to_full_range() {
        assert_eq!(to_rgb888(0xFFFF), [255, 255, 255]);
        assert_eq!(to_rgb888(0x0000), [0, 0, 0]);
        assert_eq!(to_rgb888(0xF800), [255, 0, 0]);
        assert_eq!(to_rgb888(0x07E0), [0, 255, 0]);
    }

    #[test]
    fn png_has_its_parts() {
        let png = png_bytes(&[0xF800; 4], 2, 2);
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(&png[12..16], b"IHDR");
        assert!(png.ends_with(&[b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]));
        // The CRC of "IEND" is a known number, so the CRC code is right.
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }

    #[test]
    fn difference_counts_pixels() {
        let (n, img) = difference(&[1, 2, 3], &[1, 0, 3]);
        assert_eq!(n, 1);
        assert_eq!(img[1], 0xF81F);
    }
}
