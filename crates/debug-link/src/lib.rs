//! The console's debug link: one definition of the wire protocol, shared
//! by the firmware and the PC tool (`tools/boardctl`).
//!
//! It lets someone who cannot see the board find out what its screen
//! shows, and press its buttons, over the USB serial cable alone. The
//! messages travel on the same wire as the ordinary log text, wrapped in
//! frames that cannot be mistaken for text.
//!
//! * [`frame`]: how a message is marked off from the text around it, and
//!   the [`frame::Decoder`] that finds frames in a stream of bytes.
//! * [`msg`]: the messages ([`msg::Request`] from the PC, [`msg::Reply`]
//!   from the board) and their layout.
//! * [`pad`]: injected gamepad input (the PC plays a gamepad).
//! * [`rle`]: run-length packing of pixels.
//! * [`image`]: sending a picture as a series of frames
//!   ([`image::ImageSender`], for the firmware) and putting it together
//!   again (`image::ImageReceiver`, for the PC, behind the `std`
//!   feature).
//!
//! The crate does no input or output and allocates nothing in a firmware
//! build. `README.md` says how a firmware uses it.

#![cfg_attr(not(test), no_std)]

pub mod crc;
pub mod frame;
pub mod image;
pub mod msg;
pub mod pad;
pub mod rle;

/// The speed the line rests at, and the one all requests are sent at:
/// that of the boot loader's and the firmware's log.
pub const BAUD: u32 = 115_200;

/// Writes formatted text into a byte slice: for a firmware to put its
/// stats together without a heap. What does not fit is cut off.
pub struct TextBuf<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> TextBuf<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, len: 0 }
    }

    /// The text written so far.
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

impl core::fmt::Write for TextBuf<'_> {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        let room = self.buf.len() - self.len;
        let take = text.len().min(room);
        self.buf[self.len..self.len + take].copy_from_slice(&text.as_bytes()[..take]);
        self.len += take;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use core::fmt::Write;

    use super::*;

    #[test]
    fn text_buf_cuts_off_instead_of_failing() {
        let mut bytes = [0u8; 12];
        let mut text = TextBuf::new(&mut bytes);
        writeln!(text, "heap={}", 1234).unwrap();
        writeln!(text, "stack={}", 99).unwrap();
        assert_eq!(text.as_bytes(), b"heap=1234\nst");
    }
}
