//! The screen as an embedded-graphics drawing target.
//!
//! pad-view draws through the `DrawTarget` trait. Every drawing call
//! becomes one window on the panel ("write these pixels into this
//! rectangle") followed by the pixels:
//!
//! - `fill_solid` (a rectangle of one colour) and `fill_contiguous` (a
//!   rectangle of given colours, for example one letter of text) send one
//!   window and then the pixels in pieces through a small buffer. There is
//!   no picture of the whole screen in memory.
//! - `draw_iter` (single pixels, for lines and outlines) joins pixels that
//!   follow each other on a row into one window.
//!
//! Counters say how many pixels and windows each draw sent, for the
//! PAD-UI log line.

use embedded_graphics::{
    Pixel,
    pixelcolor::{Rgb565, raw::RawU16},
    prelude::*,
    primitives::Rectangle,
};

use crate::lcd::{FIFO_BELOW, H, Lcd, LcdError, STRIP_BYTES, W};

/// Pixels joined by `draw_iter` before they are sent.
const RUN_MAX: usize = 64;

pub struct Display {
    pub lcd: Lcd,
    /// Pixels and windows sent since the counters were last taken.
    pub pixels: u32,
    pub windows: u32,
    /// Windows that failed on the bus (should stay 0).
    pub errors: u32,
    /// Time spent inside windows (bus work), microseconds, since taken.
    pub bus_us: u64,
    run: Run,
}

/// Pixels in a row waiting to be sent by `draw_iter`.
struct Run {
    x: i32,
    y: i32,
    len: usize,
    bytes: [u8; RUN_MAX * 2],
}

impl Display {
    pub fn new(lcd: Lcd) -> Self {
        Self {
            lcd,
            pixels: 0,
            windows: 0,
            errors: 0,
            bus_us: 0,
            run: Run {
                x: 0,
                y: 0,
                len: 0,
                bytes: [0; RUN_MAX * 2],
            },
        }
    }

    /// Takes and clears the pixel and window counters.
    pub fn take_counts(&mut self) -> (u32, u32) {
        let c = (self.pixels, self.windows);
        self.pixels = 0;
        self.windows = 0;
        c
    }

    /// Sends `n` pixels into the window `r` (already clipped to the
    /// screen), taking them from `next`, high byte first. In fast memory
    /// (IRAM): it runs for every letter and line.
    #[esp_hal::ram]
    fn window(
        &mut self,
        r: &Rectangle,
        n: usize,
        mut next: impl FnMut() -> u16,
    ) -> Result<(), LcdError> {
        let x0 = r.top_left.x as u16;
        let y0 = r.top_left.y as u16;
        let x1 = x0 + r.size.width as u16 - 1;
        let y1 = y0 + r.size.height as u16 - 1;
        self.pixels += n as u32;
        self.windows += 1;
        let t0 = esp_hal::time::Instant::now();
        let result = self.lcd.begin(x0, y0, x1, y1).and_then(|()| {
            let bytes = n * 2;
            if bytes < FIFO_BELOW {
                // Small: one piece through the FIFO.
                let mut buf = [0u8; FIFO_BELOW];
                for px in buf[..bytes].chunks_exact_mut(2) {
                    px.copy_from_slice(&next().to_be_bytes());
                }
                return self.lcd.write_small(&buf[..bytes]);
            }
            // Large: by DMA in strips of whole 32-bit words; an odd last
            // pixel goes through the FIFO afterwards.
            let dma_bytes = bytes & !3;
            let strips = dma_bytes.div_ceil(STRIP_BYTES);
            self.lcd.stream(strips, |k, buf| {
                let len = (dma_bytes - k * STRIP_BYTES).min(STRIP_BYTES);
                for px in buf[..len].chunks_exact_mut(2) {
                    px.copy_from_slice(&next().to_be_bytes());
                }
                len
            })?;
            if bytes > dma_bytes {
                self.lcd.write_small(&next().to_be_bytes())?;
            }
            Ok(())
        });
        self.lcd.end();
        self.bus_us += t0.elapsed().as_micros();
        if result.is_err() {
            self.errors += 1;
        }
        result
    }

    fn flush_run(&mut self) -> Result<(), LcdError> {
        if self.run.len == 0 {
            return Ok(());
        }
        let r = Rectangle::new(
            Point::new(self.run.x, self.run.y),
            Size::new(self.run.len as u32, 1),
        );
        let n = self.run.len;
        self.run.len = 0;
        let bytes = self.run.bytes;
        let mut i = 0;
        self.window(&r, n, || {
            let v = u16::from_be_bytes([bytes[i], bytes[i + 1]]);
            i += 2;
            v
        })
    }
}

fn raw(c: Rgb565) -> u16 {
    RawU16::from(c).into_inner()
}

impl OriginDimensions for Display {
    fn size(&self) -> Size {
        Size::new(W as u32, H as u32)
    }
}

impl DrawTarget for Display {
    type Color = Rgb565;
    type Error = LcdError;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(p, c) in pixels {
            if p.x < 0 || p.y < 0 || p.x >= W as i32 || p.y >= H as i32 {
                continue;
            }
            let joins = self.run.len > 0
                && self.run.len < RUN_MAX
                && p.y == self.run.y
                && p.x == self.run.x + self.run.len as i32;
            if !joins {
                self.flush_run()?;
                self.run.x = p.x;
                self.run.y = p.y;
            }
            let at = self.run.len * 2;
            self.run.bytes[at..at + 2].copy_from_slice(&raw(c).to_be_bytes());
            self.run.len += 1;
        }
        self.flush_run()
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        let clip = area.intersection(&self.bounding_box());
        if clip.size.width == 0 || clip.size.height == 0 {
            return Ok(());
        }
        let n = (clip.size.width * clip.size.height) as usize;
        if clip == *area {
            let mut it = colors.into_iter();
            self.window(&clip, n, || it.next().map_or(0, raw))
        } else {
            // Partly off screen: keep only the colours of visible points.
            // They come in the same order the window fills (row by row).
            let mut it = area
                .points()
                .zip(colors)
                .filter(|(p, _)| clip.contains(*p))
                .map(|(_, c)| c);
            self.window(&clip, n, || it.next().map_or(0, raw))
        }
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
        let clip = area.intersection(&self.bounding_box());
        if clip.size.width == 0 || clip.size.height == 0 {
            return Ok(());
        }
        let n = (clip.size.width * clip.size.height) as usize;
        let v = raw(color);
        self.window(&clip, n, || v)
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.fill_solid(&self.bounding_box(), color)
    }
}
