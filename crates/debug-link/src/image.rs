//! Sending a picture as a series of frames, and putting it together
//! again.
//!
//! The board's half ([`ImageSender`]) does no input or output and keeps
//! no picture in memory. The firmware asks it for one frame at a time and
//! sends each as it likes; the sender in turn asks the firmware for one
//! row of pixels at a time. So a 150 KB panel screenshot needs one row
//! and one frame of working memory, and other tasks can run between
//! frames.

use crate::{
    crc::Crc32,
    frame::{self, HEADER_LEN, MAX_PAYLOAD, OVERHEAD},
    msg::{PixelFormat, Reply, SHOT_DATA_HEADER, Source, kind},
    rle,
};

/// The smallest frame buffer [`ImageSender::next_frame`] works with: the
/// palette frame must fit.
pub const MIN_SENDER_FRAME: usize = 512 + OVERHEAD;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Begin,
    Palette,
    Data,
    Done,
}

/// Turns a picture into frames, one per call.
///
/// The frames, in order: one `SHOT_BEGIN`, one `SHOT_PALETTE` if the
/// picture is indexed, as many `SHOT_DATA` as it takes, one `SHOT_END`.
pub struct ImageSender {
    seq: u8,
    source: Source,
    format: PixelFormat,
    width: u16,
    height: u16,
    stage: Stage,
    /// The row now in the caller's row buffer, when `row_loaded`.
    row: u16,
    row_loaded: bool,
    /// How many pixels of that row have been sent.
    in_row: usize,
    /// Pixels sent so far.
    sent: u32,
    crc: Crc32,
    /// Reported in the closing frame: how long the firmware spent
    /// fetching pixels (for the panel, reading them back). The firmware
    /// may set this at any time before the last call.
    pub capture_ms: u32,
}

impl ImageSender {
    /// `seq` is the request's sequence number; every frame repeats it.
    pub fn new(seq: u8, source: Source, format: PixelFormat, width: u16, height: u16) -> Self {
        Self {
            seq,
            source,
            format,
            width,
            height,
            stage: Stage::Begin,
            row: 0,
            row_loaded: false,
            in_row: 0,
            sent: 0,
            crc: Crc32::new(),
            capture_ms: 0,
        }
    }

    /// Bytes one row of this picture takes: the least size of `row_buf`.
    pub fn row_bytes(&self) -> usize {
        usize::from(self.width) * self.format.bytes()
    }

    /// Writes the next frame into `out` and returns its length, or `None`
    /// when the picture has been sent.
    ///
    /// * `row_buf`: at least [`Self::row_bytes`] long. It holds the row
    ///   being sent between calls, so the caller must leave it alone
    ///   until the picture is done.
    /// * `palette`: needed for an indexed picture, ignored otherwise.
    /// * `fetch_row(y, row)`: fills `row` with row `y` of the picture,
    ///   left to right; RGB565 pixels low byte first. Called once per
    ///   row, top to bottom. If it fails, the error is passed on and the
    ///   picture cannot be continued.
    /// * `out`: at least [`MIN_SENDER_FRAME`] long;
    ///   [`frame::MAX_FRAME`] makes the fewest frames.
    pub fn next_frame<E>(
        &mut self,
        row_buf: &mut [u8],
        palette: Option<&[u16; 256]>,
        mut fetch_row: impl FnMut(u16, &mut [u8]) -> Result<(), E>,
        out: &mut [u8],
    ) -> Result<Option<usize>, E> {
        assert!(out.len() >= MIN_SENDER_FRAME);
        let done = |len: Result<usize, frame::EncodeError>| Ok(Some(len.expect("the buffer was checked above")));

        match self.stage {
            Stage::Begin => {
                self.stage = match (self.format, palette) {
                    (PixelFormat::Indexed8, Some(_)) => Stage::Palette,
                    _ => Stage::Data,
                };
                let begin =
                    Reply::ShotBegin { source: self.source, format: self.format, width: self.width, height: self.height };
                done(begin.encode(self.seq, out))
            }
            Stage::Palette => {
                self.stage = Stage::Data;
                let body = &mut out[HEADER_LEN..HEADER_LEN + 512];
                for (bytes, colour) in body.chunks_exact_mut(2).zip(palette.into_iter().flatten()) {
                    bytes.copy_from_slice(&colour.to_le_bytes());
                }
                Ok(Some(frame::finish(kind::SHOT_PALETTE, self.seq, 512, out)))
            }
            Stage::Data => {
                let elem = self.format.bytes();
                let width = usize::from(self.width);
                let row_bytes = width * elem;
                let room = (out.len() - OVERHEAD).min(MAX_PAYLOAD);
                let first = self.sent;
                let mut pixels = 0usize;
                let mut written = SHOT_DATA_HEADER;

                loop {
                    if !self.row_loaded {
                        // The pixel count of a frame is a 16-bit number.
                        if self.row == self.height || pixels + width > usize::from(u16::MAX) {
                            break;
                        }
                        fetch_row(self.row, &mut row_buf[..row_bytes])?;
                        self.crc.update(&row_buf[..row_bytes]);
                        self.row_loaded = true;
                        self.in_row = 0;
                    }
                    let rest = &row_buf[self.in_row * elem..row_bytes];
                    let (packed_pixels, packed_bytes) = rle::encode(elem, rest, &mut out[HEADER_LEN + written..HEADER_LEN + room]);
                    self.in_row += packed_pixels;
                    pixels += packed_pixels;
                    written += packed_bytes;
                    if self.in_row < width {
                        // The frame is full; the rest of the row goes
                        // into the next one.
                        break;
                    }
                    self.row_loaded = false;
                    self.row += 1;
                }

                if pixels == 0 {
                    // Nothing left: this call makes the closing frame.
                    self.stage = Stage::Done;
                    let end = Reply::ShotEnd { pixels: self.sent, crc32: self.crc.finish(), capture_ms: self.capture_ms };
                    return done(end.encode(self.seq, out));
                }
                self.sent += pixels as u32;
                let body = &mut out[HEADER_LEN..];
                body[..4].copy_from_slice(&first.to_le_bytes());
                body[4..6].copy_from_slice(&(pixels as u16).to_le_bytes());
                Ok(Some(frame::finish(kind::SHOT_DATA, self.seq, written, out)))
            }
            Stage::Done => Ok(None),
        }
    }
}

#[cfg(any(test, feature = "std"))]
pub use receiver::{Image, ImageError, ImageReceiver};

#[cfg(any(test, feature = "std"))]
mod receiver {
    extern crate std;
    use std::{vec, vec::Vec};

    use crate::{
        crc::Crc32,
        frame::Frame,
        msg::{PixelFormat, Reply, Source, kind, status},
        rle,
    };

    /// A picture as it left the board.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Image {
        pub source: Source,
        pub format: PixelFormat,
        pub width: u16,
        pub height: u16,
        /// Row by row from the top left; RGB565 pixels low byte first.
        pub pixels: Vec<u8>,
        /// The 256 RGB565 colours of an indexed picture.
        pub palette: Option<Vec<u16>>,
        /// How long the board spent fetching the pixels.
        pub capture_ms: u32,
    }

    impl Image {
        /// The pixel at `(x, y)` as RGB565, through the palette if there
        /// is one.
        pub fn rgb565(&self, x: usize, y: usize) -> u16 {
            let at = y * usize::from(self.width) + x;
            match self.format {
                PixelFormat::Rgb565 => u16::from_le_bytes([self.pixels[at * 2], self.pixels[at * 2 + 1]]),
                PixelFormat::Indexed8 => {
                    let index = usize::from(self.pixels[at]);
                    self.palette.as_ref().map_or(0, |palette| palette[index])
                }
            }
        }
    }

    /// Why a picture could not be put together.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ImageError {
        /// A frame of the picture that makes no sense by itself.
        BadFrame(&'static str),
        /// A piece arrived that does not follow the one before: a frame
        /// was lost on the way.
        Gap { expected: u32, got: u32 },
        /// The end came with fewer or more pixels than the picture has.
        WrongPixelCount { expected: u32, got: u32 },
        /// All pieces arrived but the whole does not match the board's
        /// checksum.
        WrongChecksum,
        /// The board gave up (status from its acknowledgement).
        BoardFailed(u8),
    }

    impl core::fmt::Display for ImageError {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            match self {
                Self::BadFrame(what) => write!(f, "bad picture frame: {what}"),
                Self::Gap { expected, got } => {
                    write!(f, "a frame was lost: expected pixel {expected} next, got pixel {got}")
                }
                Self::WrongPixelCount { expected, got } => write!(f, "picture ended with {got} pixels of {expected}"),
                Self::WrongChecksum => write!(f, "the picture's checksum does not match"),
                Self::BoardFailed(status) => write!(f, "the board gave up on the screenshot (status {status})"),
            }
        }
    }

    impl std::error::Error for ImageError {}

    /// Collects the frames of one screenshot.
    pub struct ImageReceiver {
        seq: u8,
        image: Option<Image>,
        /// Pixels received so far.
        got: u32,
    }

    /// Larger pictures than this are refused: nothing on the console is
    /// near it, and a damaged header must not make the PC allocate
    /// gigabytes.
    const MAX_PIXELS: usize = 1024 * 1024;

    impl ImageReceiver {
        /// `seq` is the sequence number of the screenshot request. Frames
        /// with another number are ignored.
        pub fn new(seq: u8) -> Self {
            Self { seq, image: None, got: 0 }
        }

        /// How far along the picture is: `(pixels received, pixels in
        /// all)`, once it has begun.
        pub fn progress(&self) -> Option<(u32, u32)> {
            self.image.as_ref().map(|image| (self.got, u32::from(image.width) * u32::from(image.height)))
        }

        /// Takes a received frame. Returns the picture when this frame
        /// completed it.
        pub fn on_frame(&mut self, frame: &Frame<'_>) -> Result<Option<Image>, ImageError> {
            if frame.seq != self.seq {
                return Ok(None);
            }
            let reply = match frame.kind {
                kind::ACK | kind::SHOT_BEGIN | kind::SHOT_PALETTE | kind::SHOT_DATA | kind::SHOT_END => {
                    Reply::parse(frame).map_err(|_| ImageError::BadFrame("payload does not fit its kind"))?
                }
                _ => return Ok(None),
            };
            match reply {
                Reply::Ack { request: kind::SHOT, status } if status != status::OK => {
                    Err(ImageError::BoardFailed(status))
                }
                Reply::ShotBegin { source, format, width, height } => {
                    let pixels = usize::from(width) * usize::from(height);
                    if pixels == 0 || pixels > MAX_PIXELS {
                        return Err(ImageError::BadFrame("impossible size"));
                    }
                    self.got = 0;
                    self.image = Some(Image {
                        source,
                        format,
                        width,
                        height,
                        pixels: vec![0; pixels * format.bytes()],
                        palette: None,
                        capture_ms: 0,
                    });
                    Ok(None)
                }
                Reply::ShotPalette { colours } => {
                    let image = self.image.as_mut().ok_or(ImageError::BadFrame("palette before the beginning"))?;
                    image.palette =
                        Some(colours.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect());
                    Ok(None)
                }
                Reply::ShotData { first, pixels, packed } => {
                    let image = self.image.as_mut().ok_or(ImageError::BadFrame("pixels before the beginning"))?;
                    if first != self.got {
                        return Err(ImageError::Gap { expected: self.got, got: first });
                    }
                    let elem = image.format.bytes();
                    let from = first as usize * elem;
                    let to = from + usize::from(pixels) * elem;
                    let room = image.pixels.get_mut(from..to).ok_or(ImageError::BadFrame("pixels past the end"))?;
                    match rle::decode(elem, packed, room) {
                        Ok(bytes) if bytes == room.len() => {}
                        _ => return Err(ImageError::BadFrame("packed pixels do not unpack to the count given")),
                    }
                    self.got += u32::from(pixels);
                    Ok(None)
                }
                Reply::ShotEnd { pixels, crc32, capture_ms } => {
                    let mut image = self.image.take().ok_or(ImageError::BadFrame("end before the beginning"))?;
                    let expected = u32::from(image.width) * u32::from(image.height);
                    if pixels != expected || self.got != expected {
                        return Err(ImageError::WrongPixelCount { expected, got: self.got });
                    }
                    let mut crc = Crc32::new();
                    crc.update(&image.pixels);
                    if crc.finish() != crc32 {
                        return Err(ImageError::WrongChecksum);
                    }
                    if image.format == PixelFormat::Indexed8 && image.palette.is_none() {
                        return Err(ImageError::BadFrame("indexed picture without a palette"));
                    }
                    image.capture_ms = capture_ms;
                    Ok(Some(image))
                }
                _ => Ok(None),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::{vec, vec::Vec};

    use super::*;
    use crate::frame::{Decoder, Event, MAX_FRAME};

    /// A picture with flat areas, single odd pixels and a noisy corner.
    fn sample(format: PixelFormat, width: u16, height: u16) -> Vec<u8> {
        let elem = format.bytes();
        let mut state = 7u32;
        let mut pixels = Vec::new();
        for y in 0..usize::from(height) {
            for x in 0..usize::from(width) {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let value: u16 = if x < 20 && y < 20 {
                    (state >> 12) as u16
                } else if x == y {
                    0xFFFF
                } else {
                    ((y / 9) * 0x0841) as u16
                };
                pixels.extend(&value.to_le_bytes()[..elem]);
            }
        }
        pixels
    }

    fn palette() -> [u16; 256] {
        core::array::from_fn(|i| (i as u16) * 257)
    }

    /// Everything the board would put on the wire for this picture.
    fn frames_of(format: PixelFormat, width: u16, height: u16, pixels: &[u8], frame_buf: usize) -> Vec<Vec<u8>> {
        let mut sender = ImageSender::new(5, Source::Framebuffer, format, width, height);
        let mut row_buf = vec![0u8; sender.row_bytes()];
        let mut out = vec![0u8; frame_buf];
        let palette = palette();
        let mut frames = Vec::new();
        let mut next_row = 0;
        loop {
            let fetch = |y: u16, row: &mut [u8]| -> Result<(), ()> {
                assert_eq!(y, next_row, "rows are asked for once each, in order");
                next_row += 1;
                let from = usize::from(y) * row.len();
                row.copy_from_slice(&pixels[from..from + row.len()]);
                Ok(())
            };
            sender.capture_ms = 123;
            match sender.next_frame(&mut row_buf, Some(&palette), fetch, &mut out).unwrap() {
                Some(len) => frames.push(out[..len].to_vec()),
                None => break,
            }
        }
        assert_eq!(next_row, height);
        frames
    }

    /// What the PC makes of those bytes.
    fn receive(wire: &[u8]) -> Result<Option<Image>, ImageError> {
        let mut receiver = ImageReceiver::new(5);
        let mut result = Ok(None);
        Decoder::<MAX_FRAME>::new().feed(wire, &mut |event| {
            if let (Event::Frame(frame), Ok(None)) = (event, &result) {
                result = receiver.on_frame(&frame);
            }
        });
        result
    }

    #[test]
    fn pictures_arrive_whole() {
        for (format, width, height) in
            [(PixelFormat::Rgb565, 320, 240), (PixelFormat::Indexed8, 160, 120), (PixelFormat::Rgb565, 3, 1)]
        {
            for frame_buf in [MIN_SENDER_FRAME, MAX_FRAME] {
                let pixels = sample(format, width, height);
                let frames = frames_of(format, width, height, &pixels, frame_buf);
                // Log text between the frames must not matter.
                let wire: Vec<u8> =
                    frames.iter().flat_map(|frame| [b"INFO - tick\r\n".as_slice(), frame].concat()).collect();
                let image = receive(&wire).unwrap().expect("a whole picture");
                assert_eq!((image.format, image.width, image.height), (format, width, height));
                assert_eq!(image.pixels, pixels);
                assert_eq!(image.capture_ms, 123);
                assert_eq!(image.palette.is_some(), format == PixelFormat::Indexed8);
                if let Some(colours) = &image.palette {
                    assert_eq!(colours[..], palette()[..]);
                }
            }
        }
    }

    #[test]
    fn flat_picture_is_small_on_the_wire() {
        let pixels = vec![0u8; 320 * 240 * 2];
        let frames = frames_of(PixelFormat::Rgb565, 320, 240, &pixels, MAX_FRAME);
        let bytes: usize = frames.iter().map(Vec::len).sum();
        // 240 rows of three tokens, plus the frames around them.
        assert!(bytes < 2400, "{bytes}");
    }

    #[test]
    fn a_lost_frame_is_noticed() {
        let pixels = sample(PixelFormat::Rgb565, 320, 240);
        let frames = frames_of(PixelFormat::Rgb565, 320, 240, &pixels, MIN_SENDER_FRAME);
        assert!(frames.len() > 6, "{} frames", frames.len());
        // Frame 0 is the beginning; drop one from the middle of the data.
        let wire: Vec<u8> = frames.iter().enumerate().filter(|(i, _)| *i != 3).flat_map(|(_, f)| f.clone()).collect();
        assert!(matches!(receive(&wire), Err(ImageError::Gap { .. })));
        // The last data frame lost: the end does not add up.
        let last_data = frames.len() - 2;
        let wire: Vec<u8> =
            frames.iter().enumerate().filter(|(i, _)| *i != last_data).flat_map(|(_, f)| f.clone()).collect();
        assert!(matches!(receive(&wire), Err(ImageError::WrongPixelCount { .. })));
    }

    #[test]
    fn a_failing_fetch_stops_the_sender() {
        let mut sender = ImageSender::new(1, Source::Panel, PixelFormat::Rgb565, 320, 240);
        let mut row_buf = [0u8; 640];
        let mut out = [0u8; MAX_FRAME];
        // The beginning needs no pixels.
        assert!(sender.next_frame(&mut row_buf, None, |_, _| Err::<(), _>("no answer"), &mut out).is_ok());
        assert_eq!(sender.next_frame(&mut row_buf, None, |_, _| Err("no answer"), &mut out), Err("no answer"));
    }

    #[test]
    fn the_board_giving_up_is_reported() {
        let mut out = [0u8; 32];
        let len = Reply::Ack { request: kind::SHOT, status: crate::msg::status::FAILED }.encode(5, &mut out).unwrap();
        assert_eq!(receive(&out[..len]), Err(ImageError::BoardFailed(crate::msg::status::FAILED)));
    }
}
