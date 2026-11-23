//! The screen: the console says WHAT changed, the board decides HOW to
//! send it.
//!
//! The console keeps its picture small (for example 160x120, one byte a
//! pixel). It never builds the full 320x240 picture of 16-bit colours:
//! that would be 150 KB, more than the chip can spare. Instead it hands
//! the board a [`PixelSource`]: something the board can ask for finished
//! pixels, a short run at a time, in the order the board wants.
//!
//! * The CYD board asks for one panel column (a "gate line", 240 pixels)
//!   at a time, right into its small DMA buffer, in the order the panel
//!   refreshes. Then the picture does not tear.
//! * The PC board asks for whole rows and copies them into its image.

use crate::geom::Rect;

/// Which way a run of pixels walks across the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// Along a row, left to right (x grows).
    Right,
    /// Along a row, right to left (x shrinks).
    Left,
    /// Down a column, top to bottom (y grows).
    Down,
    /// Up a column, bottom to top (y shrinks).
    Up,
}

/// How each 16-bit colour is laid out in memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteOrder {
    /// The number as the program sees it (RGB565: 5 bits red, 6 green,
    /// 5 blue). Good for copying into an image on the PC.
    Native,
    /// High byte first in memory: the order the ST7789 panel reads the
    /// bytes over its wire. The CYD board can then send the buffer as it
    /// is, with no extra pass. (On the ESP32, which stores the low byte
    /// first, this is the colour with its two bytes swapped.)
    Wire,
}

impl ByteOrder {
    /// Puts one RGB565 colour into this order.
    #[inline(always)]
    pub const fn apply(self, rgb565: u16) -> u16 {
        match self {
            ByteOrder::Native => rgb565,
            ByteOrder::Wire => rgb565.to_be(),
        }
    }
}

/// Finished panel pixels, made on request by the console's video mode.
///
/// The board calls [`PixelSource::run`] as many times as it likes during
/// one [`Screen::present`], in any order. It must not keep the source
/// after `present` returns (the borrow checker makes sure of that).
pub trait PixelSource {
    /// Writes `out.len()` finished RGB565 pixels, starting at panel pixel
    /// `(x, y)` and walking in direction `dir`. Every pixel of the run is
    /// inside the panel (the board never asks for more). Pixels outside
    /// the console's picture (a border, if the picture does not fill the
    /// panel) come out in the border colour.
    ///
    /// The cost is one call per run, not per pixel, so a run of 240 pixels
    /// costs about the same as the CYD driver's own fill loop.
    fn run(&self, x: u16, y: u16, dir: Dir, order: ByteOrder, out: &mut [u16]);

    /// The fast path, for a board that wants it: the indexed picture
    /// behind this source, if it is one. The CYD board can then use its
    /// fill loops measured on the board (palette lookup straight into the
    /// DMA buffer, 12-bit colour) instead of `run`. A board may ignore
    /// this and always use `run`; the pixels are the same.
    fn indexed(&self) -> Option<Indexed<'_>> {
        None
    }
}

/// An indexed picture: one byte per pixel, each byte a number into a
/// 256-colour palette. Shown `scale` times bigger on the panel, with its
/// top-left corner at panel pixel `(left, top)`.
#[derive(Clone, Copy, Debug)]
pub struct Indexed<'a> {
    /// `width * height` bytes, row after row.
    pub pixels: &'a [u8],
    pub width: u16,
    pub height: u16,
    /// Each picture pixel becomes `scale x scale` panel pixels (2 for the
    /// 160x120 mode on a 320x240 panel).
    pub scale: u8,
    pub left: u16,
    pub top: u16,
    /// RGB565 colour of each palette number.
    pub palette: &'a [u16; 256],
    /// Changes every time any palette entry changes. A board that keeps
    /// its own copy of the palette in another form (for example ready
    /// for the wire) rebuilds it only when this number changed.
    pub palette_version: u32,
    /// RGB565 colour of panel pixels outside the picture.
    pub border: u16,
}

/// What one [`Screen::present`] did. Times are in microseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SendReport {
    /// Windows actually sent (after the board joined or split areas).
    pub areas: u16,
    /// Panel pixels sent.
    pub pixels: u32,
    /// How long sending takes. Measured on the CYD; on the PC worked out
    /// from numbers measured on the board (a model, not a measurement).
    pub send_us: u32,
    /// Time spent waiting for the panel before sending (the tear-free
    /// timing waits for the panel's refresh to be in the right place).
    pub wait_us: u32,
}

/// The panel.
pub trait Screen {
    /// Panel size in pixels, as the console uses it (landscape).
    /// CYD: 320 x 240.
    const PANEL_W: u16;
    const PANEL_H: u16;

    /// Sends these changed areas of the picture to the panel.
    ///
    /// `areas` are in panel pixels, inside the panel. They may touch or
    /// overlap. The board must send at least these pixels; it may send
    /// more (widen, join or split areas as its wire likes), because the
    /// source can make any pixel. An empty list sends nothing.
    ///
    /// When this returns, the board has read every pixel it needs from
    /// `src`. (The last bytes may still be on the wire; the board waits
    /// for them at the start of the next `present`, not here.)
    fn present<S: PixelSource + ?Sized>(&mut self, src: &S, areas: &[Rect]) -> SendReport;

    /// Backlight brightness, 0 (off) to 255 (full). Boards without a
    /// dimmable backlight ignore it.
    ///
    /// The console calls it once after its first picture was sent (so
    /// the screen is never lit before there is a picture), and then once
    /// each time the player changes the brightness. It never asks for 0
    /// (see `console_core::theme::BACKLIGHT`). Until the first call, the
    /// board should keep the backlight off.
    fn backlight(&mut self, _level: u8) {}
}

/// The whole panel as one rectangle.
pub const fn panel_rect<S: Screen + ?Sized>() -> Rect {
    Rect::new(0, 0, S::PANEL_W, S::PANEL_H)
}
