//! Two test pictures with known pixel values.
//!
//! The firmware draws them on the board, and the PC tool draws them into
//! memory with this same code (`boardctl testcard`). A screenshot taken
//! from the board can then be compared with what should be there, pixel
//! for pixel, by a program.
//!
//! * [`draw_panel_card`]: 320x240 in RGB565, for anything that draws
//!   straight to the panel.
//! * [`draw_fb_card`]: 160x120 with one byte per pixel, for the console's
//!   logical framebuffer, to be shown through [`PALETTE`].
//!
//! Each picture is made so that the usual mistakes show:
//!
//! * a one-pixel border, and a different colour in each corner pixel: a
//!   picture that is shifted, cropped, flipped or turned;
//! * an "F" and a diagonal that runs one way only: the same, to the eye;
//! * bars of known colours, and one stripe for each of the 16 bits of a
//!   pixel: swapped colour channels or bytes, a stuck or missed bit;
//! * one-pixel checks and stripes, and noise: pixels that smear into
//!   their neighbours or arrive one place late (what a too-fast clock
//!   does).

#![no_std]

use core::convert::Infallible;

use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::{Gray8, Rgb565, raw::RawU16},
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle, Triangle},
    text::{Baseline, Text},
};

/// Size of the panel picture.
pub const PANEL_W: usize = 320;
pub const PANEL_H: usize = 240;
/// Size of the framebuffer picture.
pub const FB_W: usize = 160;
pub const FB_H: usize = 120;

/// Rows at the bottom of the framebuffer picture that the card leaves
/// empty, for a firmware to write its own status into.
pub const FB_STATUS_Y: i32 = 108;

/// The eight colour bars of the panel picture, left to right, as RGB565.
pub const BARS: [u16; 8] = [0xFFFF, 0xFFE0, 0x07FF, 0x07E0, 0xF81F, 0xF800, 0x001F, 0x0000];

/// The corner pixels of the panel picture: top left, top right, bottom
/// left, bottom right.
pub const CORNERS: [u16; 4] = [0xF800, 0x07E0, 0x001F, 0xFFE0];

/// The framebuffer picture's corner pixels, same order, as palette
/// numbers (red, green, blue, yellow).
pub const FB_CORNERS: [u8; 4] = [8, 11, 12, 10];

/// The palette the framebuffer picture is meant for: 256 RGB565 colours.
///
/// The first 16 are named colours. The rest spread the byte's bits over
/// the channels (3 red, 3 green, 2 blue), so every entry is different
/// from its neighbours and a wrong palette entry shows.
pub const PALETTE: [u16; 256] = {
    const NAMED: [u16; 16] = [
        0x0000, // 0 black
        0x000F, // 1 navy
        0x03E0, // 2 dark green
        0x03EF, // 3 teal
        0x7800, // 4 maroon
        0x780F, // 5 purple
        0x7BE0, // 6 olive
        0xFFFF, // 7 white
        0xF800, // 8 red
        0xFD20, // 9 orange
        0xFFE0, // 10 yellow
        0x07E0, // 11 green
        0x001F, // 12 blue
        0x07FF, // 13 cyan
        0xF81F, // 14 magenta
        0x7BEF, // 15 grey
    ];
    let mut palette = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        palette[i] = if i < 16 {
            NAMED[i]
        } else {
            let red = ((i >> 5) & 7) * 31 / 7;
            let green = ((i >> 2) & 7) * 63 / 7;
            let blue = (i & 3) * 31 / 3;
            ((red << 11) | (green << 5) | blue) as u16
        };
        i += 1;
    }
    palette
};

fn rgb(raw: u16) -> Rgb565 {
    Rgb565::from(RawU16::new(raw))
}

/// Numbers that look random and are the same on every run and machine.
struct Noise(u32);

impl Iterator for Noise {
    type Item = u16;

    fn next(&mut self) -> Option<u16> {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        Some((self.0 >> 12) as u16)
    }
}

fn block<D: DrawTarget>(target: &mut D, x: i32, y: i32, w: u32, h: u32, colour: D::Color) -> Result<(), D::Error> {
    target.fill_solid(&Rectangle::new(Point::new(x, y), Size::new(w, h)), colour)
}

/// The parts both pictures share, at a scale of 1 (framebuffer) or 2
/// (panel): the border and corners, the F, the diagonal, the triangle.
fn marks<D: DrawTarget>(
    target: &mut D,
    (w, h): (i32, i32),
    scale: i32,
    y: i32,
    (white, f_colour, line_colour, triangle_colour): (D::Color, D::Color, D::Color, D::Color),
    corners: [D::Color; 4],
) -> Result<(), D::Error> {
    // The border: the outermost pixel all the way round.
    block(target, 0, 0, w as u32, 1, white)?;
    block(target, 0, h - 1, w as u32, 1, white)?;
    block(target, 0, 0, 1, h as u32, white)?;
    block(target, w - 1, 0, 1, h as u32, white)?;
    let places = [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)];
    for ((cx, cy), colour) in places.into_iter().zip(corners) {
        block(target, cx, cy, 1, 1, colour)?;
    }

    let s = scale;
    let su = scale as u32;
    // The F: an upright and two arms pointing right, the top one longer.
    let x = 8 * s;
    block(target, x, y, 5 * su, 25 * su, f_colour)?;
    block(target, x, y, 18 * su, 5 * su, f_colour)?;
    block(target, x, y + 10 * s, 13 * su, 5 * su, f_colour)?;

    // One pixel wide, from upper left to lower right.
    Line::new(Point::new(40 * s, y), Point::new(40 * s + 25 * s - 1, y + 25 * s - 1))
        .into_styled(PrimitiveStyle::with_stroke(line_colour, 1))
        .draw(target)?;

    // A triangle with its right angle at the lower right.
    Triangle::new(
        Point::new(80 * s, y + 25 * s - 1),
        Point::new(105 * s, y + 25 * s - 1),
        Point::new(105 * s, y),
    )
    .into_styled(PrimitiveStyle::with_fill(triangle_colour))
    .draw(target)
}

/// Draws the 320x240 picture. The target must be at least that large;
/// the picture goes to its top left.
pub fn draw_panel_card<D: DrawTarget<Color = Rgb565>>(target: &mut D) -> Result<(), D::Error> {
    let (w, h) = (PANEL_W as i32, PANEL_H as i32);
    let white = rgb(0xFFFF);
    block(target, 0, 0, w as u32, h as u32, rgb(0x0000))?;

    // Colour bars.
    for (i, &bar) in BARS.iter().enumerate() {
        block(target, 16 + 36 * i as i32, 12, 36, 40, rgb(bar))?;
    }
    // One stripe per bit of a pixel, the highest (red's top bit) first.
    for bit in 0..16 {
        block(target, 16 + 18 * bit, 56, 18, 20, rgb(0x8000 >> bit))?;
    }
    // Ramps: every level of red, of green and of blue.
    for level in 0..32 {
        block(target, 16 + 9 * level, 80, 9, 10, rgb((level as u16) << 11))?;
        block(target, 16 + 9 * level, 100, 9, 10, rgb(level as u16))?;
    }
    for level in 0..64 {
        block(target, 16 + 4 * level, 90, 4, 10, rgb((level as u16) << 5))?;
    }

    // One-pixel patterns and noise, each 64 (or 72) by 32.
    let patch = |x: i32, w: u32| Rectangle::new(Point::new(x, 116), Size::new(w, 32));
    let on_off = |on: bool| if on { white } else { rgb(0x0000) };
    let at = |i: u32| (i % 64, i / 64);
    target.fill_contiguous(&patch(16, 64), (0..64 * 32).map(|i| on_off((at(i).0 + at(i).1) % 2 == 0)))?;
    target.fill_contiguous(&patch(88, 64), (0..64 * 32).map(|i| on_off(at(i).0 % 2 == 0)))?;
    target.fill_contiguous(&patch(160, 64), (0..64 * 32).map(|i| on_off(at(i).1 % 2 == 0)))?;
    target.fill_contiguous(&patch(232, 72), Noise(1).take(72 * 32).map(rgb))?;

    let text = MonoTextStyle::new(&FONT_6X10, white);
    Text::with_baseline("SCREEN-MIRROR TEST CARD 320x240 RGB565", Point::new(16, 152), text, Baseline::Top)
        .draw(target)?;
    Text::with_baseline("corners: TL red TR green BL blue BR yellow", Point::new(16, 164), text, Baseline::Top)
        .draw(target)?;
    Text::with_baseline("BOTTOM-RIGHT", Point::new(240, 224), text, Baseline::Top).draw(target)?;

    marks(target, (w, h), 2, 180, (white, rgb(0xFFE0), rgb(0x07FF), rgb(0xF81F)), CORNERS.map(rgb))
}

/// Draws the 160x120 picture. The colours are palette numbers, carried in
/// `Gray8` because embedded-graphics has no colour type for "a number
/// into a palette".
pub fn draw_fb_card<D: DrawTarget<Color = Gray8>>(target: &mut D) -> Result<(), D::Error> {
    let (w, h) = (FB_W as i32, FB_H as i32);
    let index = Gray8::new;
    block(target, 0, 0, w as u32, h as u32, index(0))?;

    // The 16 named colours.
    for i in 0..16 {
        block(target, 8 + 9 * i, 6, 9, 12, index(i as u8))?;
    }
    // Every palette number once, 16 to a row.
    for i in 0..256 {
        block(target, 8 + 4 * (i % 16), 22 + 3 * (i / 16), 4, 3, index(i as u8))?;
    }
    // One-pixel checks and noise.
    let checks = Rectangle::new(Point::new(80, 22), Size::new(32, 24));
    target.fill_contiguous(&checks, (0..32 * 24).map(|i| index(if (i % 32 + i / 32) % 2 == 0 { 7 } else { 0 })))?;
    let noise = Rectangle::new(Point::new(116, 22), Size::new(36, 24));
    target.fill_contiguous(&noise, Noise(2).take(36 * 24).map(|n| index(n as u8)))?;

    let text = MonoTextStyle::new(&FONT_6X10, index(7));
    Text::with_baseline("FB 160x120", Point::new(80, 50), text, Baseline::Top).draw(target)?;
    Text::with_baseline("INDEXED", Point::new(80, 60), text, Baseline::Top).draw(target)?;

    marks(target, (w, h), 1, 76, (index(7), index(10), index(13), index(14)), FB_CORNERS.map(index))
}

/// A 160x120 framebuffer of palette numbers that embedded-graphics can
/// draw on. The firmware's logical screen and the PC's copy of it.
pub struct Fb<'a> {
    /// `FB_W * FB_H` bytes, row by row from the top left.
    pub pixels: &'a mut [u8],
}

impl<'a> Fb<'a> {
    /// Panics if `pixels` is not exactly `FB_W * FB_H` long.
    pub fn new(pixels: &'a mut [u8]) -> Self {
        assert_eq!(pixels.len(), FB_W * FB_H);
        Self { pixels }
    }
}

impl OriginDimensions for Fb<'_> {
    fn size(&self) -> Size {
        Size::new(FB_W as u32, FB_H as u32)
    }
}

impl DrawTarget for Fb<'_> {
    type Color = Gray8;
    type Error = Infallible;

    fn draw_iter<I: IntoIterator<Item = Pixel<Gray8>>>(&mut self, pixels: I) -> Result<(), Infallible> {
        for Pixel(point, colour) in pixels {
            // Anything outside the screen is left out, as on a display.
            if (0..FB_W as i32).contains(&point.x) && (0..FB_H as i32).contains(&point.y) {
                self.pixels[point.y as usize * FB_W + point.x as usize] = colour.luma();
            }
        }
        Ok(())
    }
}

/// A 320x240 RGB565 picture in memory: what the PC draws the panel card
/// into.
pub struct PanelBuf<'a> {
    /// `PANEL_W * PANEL_H` pixels, row by row from the top left.
    pub pixels: &'a mut [u16],
}

impl<'a> PanelBuf<'a> {
    /// Panics if `pixels` is not exactly `PANEL_W * PANEL_H` long.
    pub fn new(pixels: &'a mut [u16]) -> Self {
        assert_eq!(pixels.len(), PANEL_W * PANEL_H);
        Self { pixels }
    }
}

impl OriginDimensions for PanelBuf<'_> {
    fn size(&self) -> Size {
        Size::new(PANEL_W as u32, PANEL_H as u32)
    }
}

impl DrawTarget for PanelBuf<'_> {
    type Color = Rgb565;
    type Error = Infallible;

    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(&mut self, pixels: I) -> Result<(), Infallible> {
        for Pixel(point, colour) in pixels {
            if (0..PANEL_W as i32).contains(&point.x) && (0..PANEL_H as i32).contains(&point.y) {
                self.pixels[point.y as usize * PANEL_W + point.x as usize] = RawU16::from(colour).into_inner();
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::vec;

    use super::*;

    #[test]
    fn panel_card_has_its_landmarks() {
        let mut pixels = vec![0xAAAAu16; PANEL_W * PANEL_H];
        draw_panel_card(&mut PanelBuf::new(&mut pixels)).unwrap();
        let at = |x: usize, y: usize| pixels[y * PANEL_W + x];
        // Corners, border, inside the border.
        assert_eq!([at(0, 0), at(319, 0), at(0, 239), at(319, 239)], CORNERS);
        assert_eq!([at(1, 0), at(0, 1), at(319, 100), at(100, 239)], [0xFFFF; 4]);
        assert_eq!(at(1, 1), 0x0000);
        // Middle of each colour bar and of each bit stripe.
        for (i, &bar) in BARS.iter().enumerate() {
            assert_eq!(at(16 + 36 * i + 18, 30), bar);
        }
        for bit in 0..16 {
            assert_eq!(at(16 + 18 * bit + 9, 66), 0x8000 >> bit);
        }
        // The F is not its own mirror image.
        assert_eq!(at(40, 185), 0xFFE0);
        assert_eq!(at(40, 225), 0x0000);
        // Every pixel was drawn: what was in the buffer before does not
        // show through.
        let mut other = vec![0x5555u16; PANEL_W * PANEL_H];
        draw_panel_card(&mut PanelBuf::new(&mut other)).unwrap();
        assert!(pixels == other);
    }

    #[test]
    fn fb_card_has_its_landmarks() {
        let mut pixels = vec![0xEEu8; FB_W * FB_H];
        draw_fb_card(&mut Fb::new(&mut pixels)).unwrap();
        let at = |x: usize, y: usize| pixels[y * FB_W + x];
        assert_eq!([at(0, 0), at(159, 0), at(0, 119), at(159, 119)], FB_CORNERS);
        assert_eq!(at(80, 0), 7);
        // Every palette number is somewhere in the picture.
        for index in 0..=255u8 {
            assert!(pixels.contains(&index), "index {index}");
        }
        // The status rows are empty apart from the border.
        for y in FB_STATUS_Y as usize..FB_H - 1 {
            for x in 1..FB_W - 1 {
                assert_eq!(at(x, y), 0, "({x},{y})");
            }
        }
    }

    #[test]
    fn palette_is_what_the_comments_say() {
        assert_eq!(PALETTE[8], 0xF800);
        assert_eq!(PALETTE[255], 0xFFFF);
        // 16 = 000 100 00: no red, green 4 of 7, no blue.
        assert_eq!(PALETTE[16], 36 << 5);
    }
}
