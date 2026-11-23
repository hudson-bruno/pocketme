//! The shared renderer: the default video mode of the console.
//!
//! The console draws into a small picture: 160x120 pixels, one byte
//! each. The byte is a number into a palette of 256 colours (RGB565).
//! Shown 2x bigger, it fills the 320x240 panel.
//!
//! Each frame, [`Video::present`] compares the picture with the one the
//! panel shows, finds the changed rectangles (at most [`MAX_AREAS`]),
//! and hands them to the board with a [`Source`] the board can ask for
//! finished pixels. The board decides how to send them.
//!
//! Memory (all fixed, no heap), written down because RAM is scarce.
//! Kept for as long as the console runs, inside [`Video`]:
//!
//! | what                          | bytes  |
//! |-------------------------------|--------|
//! | picture being drawn (`Frame`) | 19,200 |
//! | picture the panel shows       | 19,200 |
//! | palette (256 x 2) + version   |    516 |
//! | changed rectangles (16 x 8)   |    132 |
//! | flags, border colour          |     12 |
//! | **total**                     | 39,060 |
//!
//! (On the PC the count fields are 8 bytes, so it measures 39,064.)
//! Only during a call, on the stack: [`diff`] uses 720 bytes, `present`
//! about 130 for the rectangles in panel pixels. The font is 256 bytes
//! of fixed data (flash on the chip). The crate README lists ways to
//! shrink this if the chip runs short.

#![no_std]
#![forbid(unsafe_code)]

mod areas;
mod draw;
#[cfg(feature = "embedded-graphics")]
mod eg;
mod font;
mod frame;
mod palette;
mod source;
// Without `swap`: the renderer as it always was (this file is unchanged).
#[cfg(not(feature = "swap"))]
mod video;
// With `swap`: the two pictures swap roles at each present (README,
// "Swap: two cores, two pictures").
#[cfg(feature = "swap")]
#[path = "video_swap.rs"]
mod video;

pub use areas::{Areas, MAX_AREAS, WINDOW_COST_PX, diff};
pub use board_api::Rect;
pub use draw::{CLEAR, Sprite};
pub use font::{FONT_H, Glyph, LINE_H, glyph_for, glyph_ink, glyph_w, text_width};
pub use frame::{Frame, H, W};
pub use palette::{Palette, rgb565};
pub use source::{Source, View};
pub use video::Video;
#[cfg(feature = "swap")]
pub use video::{Handed, Inline, Sender};
