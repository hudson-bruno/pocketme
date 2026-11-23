//! Pictures on the PC side: PNG files in and out, and comparing two of
//! them pixel by pixel.
//!
//! Everything is compared as RGB565, the 16-bit colour the panel is
//! sent: 5 bits of red, 6 of green, 5 of blue. A PNG stores 8 bits of
//! each, so a 565 colour is widened when written (the top bits are
//! repeated in the bottom, so full red is 255 and not 248) and narrowed
//! again when read. That round trip loses nothing.

use std::{fs::File, io::BufWriter, path::Path};

use anyhow::{Context, Result, bail};
use debug_link::{image::Image, msg::PixelFormat};

/// A picture as plain RGB565 values, row by row from the top left.
#[derive(Clone)]
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u16>,
}

pub fn rgb565_to_888(colour: u16) -> [u8; 3] {
    let (r, g, b) = ((colour >> 11) as u8, ((colour >> 5) & 0x3F) as u8, (colour & 0x1F) as u8);
    [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)]
}

pub fn rgb888_to_565(rgb: [u8; 3]) -> u16 {
    (u16::from(rgb[0] >> 3) << 11) | (u16::from(rgb[1] >> 2) << 5) | u16::from(rgb[2] >> 3)
}

impl Picture {
    pub fn from_image(image: &Image) -> Self {
        let (width, height) = (usize::from(image.width), usize::from(image.height));
        let pixels = (0..height).flat_map(|y| (0..width).map(move |x| image.rgb565(x, y))).collect();
        Self { width, height, pixels }
    }

    /// The test picture the firmware draws straight onto the panel.
    pub fn panel_card() -> Self {
        let mut pixels = vec![0u16; test_card::PANEL_W * test_card::PANEL_H];
        // Drawing into memory cannot fail.
        let _ = test_card::draw_panel_card(&mut test_card::PanelBuf::new(&mut pixels));
        Self { width: test_card::PANEL_W, height: test_card::PANEL_H, pixels }
    }

    /// Every pixel repeated `factor` times each way, as the firmware does
    /// when it shows its small framebuffer on the big panel.
    pub fn scaled(&self, factor: usize) -> Self {
        let (width, height) = (self.width * factor, self.height * factor);
        let pixels =
            (0..height).flat_map(|y| (0..width).map(move |x| self.pixels[(y / factor) * self.width + x / factor])).collect();
        Self { width, height, pixels }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let mut encoder = png::Encoder::new(create(path)?, self.width as u32, self.height as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let bytes: Vec<u8> = self.pixels.iter().flat_map(|&colour| rgb565_to_888(colour)).collect();
        encoder.write_header()?.write_image_data(&bytes)?;
        Ok(())
    }

    /// Reads any PNG and narrows it to RGB565.
    pub fn load(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
        // Palette pictures and odd bit depths come out as plain 8-bit.
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().with_context(|| format!("{} is not a PNG", path.display()))?;
        let Some(size) = reader.output_buffer_size() else { bail!("{} is too large", path.display()) };
        let mut bytes = vec![0u8; size];
        let info = reader.next_frame(&mut bytes)?;
        let step = match info.color_type {
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            png::ColorType::Grayscale => 1,
            png::ColorType::GrayscaleAlpha => 2,
            png::ColorType::Indexed => bail!("{}: the palette was not expanded", path.display()),
        };
        let pixels = bytes[..info.buffer_size()]
            .chunks_exact(step)
            .map(|pixel| rgb888_to_565(if step >= 3 { [pixel[0], pixel[1], pixel[2]] } else { [pixel[0]; 3] }))
            .collect();
        Ok(Self { width: info.width as usize, height: info.height as usize, pixels })
    }
}

fn create(path: &Path) -> Result<BufWriter<File>> {
    Ok(BufWriter::new(File::create(path).with_context(|| format!("cannot create {}", path.display()))?))
}

/// Saves a screenshot. A framebuffer screenshot keeps its palette: the
/// PNG holds the same index for every pixel that the firmware had, so
/// nothing about it is lost.
pub fn save_image(image: &Image, path: &Path) -> Result<()> {
    match (image.format, &image.palette) {
        (PixelFormat::Indexed8, Some(palette)) => save_indexed(image.width.into(), image.height.into(), &image.pixels, palette, path),
        _ => Picture::from_image(image).save(path),
    }
}

pub fn save_indexed(width: usize, height: usize, indices: &[u8], palette: &[u16], path: &Path) -> Result<()> {
    let mut encoder = png::Encoder::new(create(path)?, width as u32, height as u32);
    encoder.set_color(png::ColorType::Indexed);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_palette(palette.iter().flat_map(|&colour| rgb565_to_888(colour)).collect::<Vec<u8>>());
    encoder.write_header()?.write_image_data(indices)?;
    Ok(())
}

/// The firmware's framebuffer test picture, as indices.
pub fn fb_card() -> Vec<u8> {
    let mut indices = vec![0u8; test_card::FB_W * test_card::FB_H];
    let _ = test_card::draw_fb_card(&mut test_card::Fb::new(&mut indices));
    indices
}

/// What comparing two pictures found.
pub struct Difference {
    pub total: usize,
    pub wrong: Vec<(usize, usize, u16, u16)>,
    /// How often each of the 16 colour bits differed, bit 15 first.
    pub bits: [usize; 16],
}

impl Difference {
    /// Smallest rectangle holding every wrong pixel: x0, y0, x1, y1.
    pub fn bounds(&self) -> Option<(usize, usize, usize, usize)> {
        let first = self.wrong.first()?;
        Some(self.wrong.iter().fold((first.0, first.1, first.0, first.1), |(x0, y0, x1, y1), &(x, y, ..)| {
            (x0.min(x), y0.min(y), x1.max(x), y1.max(y))
        }))
    }
}

/// Compares `a` with `b`. Pixels of `a` are the "expected" ones.
pub fn compare(a: &Picture, b: &Picture) -> Result<Difference> {
    if (a.width, a.height) != (b.width, b.height) {
        bail!("the pictures differ in size: {}x{} and {}x{}", a.width, a.height, b.width, b.height);
    }
    let mut difference = Difference { total: a.pixels.len(), wrong: Vec::new(), bits: [0; 16] };
    for (i, (&want, &got)) in a.pixels.iter().zip(&b.pixels).enumerate() {
        if want != got {
            difference.wrong.push((i % a.width, i / a.width, want, got));
            for (bit, count) in difference.bits.iter_mut().enumerate() {
                if (want ^ got) & (0x8000 >> bit) != 0 {
                    *count += 1;
                }
            }
        }
    }
    Ok(difference)
}

/// A picture that shows where two pictures differ: `b` dimmed, with
/// every wrong pixel in full red.
pub fn difference_picture(b: &Picture, difference: &Difference) -> Picture {
    let mut out = b.clone();
    for colour in &mut out.pixels {
        // Halve each colour so the red marks stand out.
        *colour = (*colour >> 1) & 0x7BEF;
    }
    for &(x, y, ..) in &difference.wrong {
        out.pixels[y * out.width + x] = 0xF800;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widening_and_narrowing_loses_nothing() {
        for colour in 0..=u16::MAX {
            assert_eq!(rgb888_to_565(rgb565_to_888(colour)), colour);
        }
        assert_eq!(rgb565_to_888(0xFFFF), [255, 255, 255]);
        assert_eq!(rgb565_to_888(0xF800), [255, 0, 0]);
    }

    #[test]
    fn png_round_trip_and_compare() {
        let card = Picture::panel_card();
        let dir = std::env::temp_dir().join(format!("boardctl-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("card.png");
        card.save(&path).unwrap();
        let mut back = Picture::load(&path).unwrap();
        assert_eq!(compare(&card, &back).unwrap().wrong.len(), 0);

        back.pixels[5 * 320 + 7] ^= 0x8001;
        let difference = compare(&card, &back).unwrap();
        assert_eq!(difference.wrong.len(), 1);
        assert_eq!(difference.bounds(), Some((7, 5, 7, 5)));
        assert_eq!((difference.bits[0], difference.bits[15]), (1, 1));

        // An indexed PNG reads back as the colours of its palette.
        let indices = fb_card();
        let path = dir.join("fb.png");
        save_indexed(160, 120, &indices, &test_card::PALETTE, &path).unwrap();
        let fb = Picture::load(&path).unwrap();
        assert_eq!(fb.pixels[0], test_card::PALETTE[usize::from(indices[0])]);
        assert_eq!(fb.scaled(2).pixels[321], fb.pixels[0]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
