//! The messages, and how each is laid out inside a frame's payload.
//!
//! Numbers of more than one byte are sent low byte first.

use crate::frame::{self, EncodeError, Frame};

/// The `kind` byte of each message. The PC's requests are below 0x80, the
/// board's answers from 0x80 up.
pub mod kind {
    /// PC: "are you there?" No payload. Answered with [`ACK`].
    pub const PING: u8 = 0x01;
    /// PC: send a screenshot. Payload: source u8, baud u32, read_khz u16.
    pub const SHOT: u8 = 0x02;
    /// PC: "I have changed speed, start sending". No payload.
    pub const GO: u8 = 0x03;
    /// PC: these buttons are held down now. Payload: mask u16.
    pub const BUTTONS: u8 = 0x04;
    /// PC: a tap on the touch screen. Payload: x u16, y u16.
    pub const TAP: u8 = 0x05;
    /// PC: send your stats. No payload.
    pub const STATS: u8 = 0x06;
    /// PC: send this command to the display panel and read its answer.
    /// Payload: command u8, dummy_bits u8, read_len u16, clock_khz u16,
    /// then the command's parameter bytes.
    pub const PANEL_RAW: u8 = 0x07;
    /// PC: injected gamepad input; see [`crate::pad`] (not a
    /// [`super::Request`]: its own type, `pad::PadRequest`).
    pub const PAD: u8 = crate::pad::KIND;
    /// PC: these buttons are held down now on one player's pad. Payload:
    /// player u8 (0 to 3), mask u16. Not a [`super::Request`]: its own
    /// type, [`super::PlayerButtons`], so firmwares built before it
    /// answer `UNSUPPORTED`.
    pub const PLAYER_BUTTONS: u8 = 0x09;

    /// Board: a request arrived. Payload: the request's kind u8, status u8.
    pub const ACK: u8 = 0x80;
    /// Board: a picture follows. Payload: source u8, format u8, width
    /// u16, height u16.
    pub const SHOT_BEGIN: u8 = 0x81;
    /// Board: the 256 colours of an indexed picture, RGB565, 512 bytes.
    pub const SHOT_PALETTE: u8 = 0x82;
    /// Board: a piece of the picture. Payload: index of its first pixel
    /// u32, number of pixels u16, then the pixels, run-length packed.
    pub const SHOT_DATA: u8 = 0x83;
    /// Board: the picture is complete. Payload: number of pixels u32,
    /// CRC-32 of the unpacked pixel bytes u32, milliseconds the board
    /// spent fetching the pixels u32.
    pub const SHOT_END: u8 = 0x84;
    /// Board: stats as text, one `key=value` per line.
    pub const STATS_TEXT: u8 = 0x85;
    /// Board: the bytes the panel answered with.
    pub const PANEL_RAW_REPLY: u8 = 0x86;
}

/// The `status` byte of an acknowledgement.
pub mod status {
    /// Done, or (for a screenshot) about to be done.
    pub const OK: u8 = 0;
    /// This firmware does not know the request.
    pub const UNSUPPORTED: u8 = 1;
    /// The request's payload makes no sense.
    pub const BAD_REQUEST: u8 = 2;
    /// The firmware tried and failed (for example the panel did not
    /// answer).
    pub const FAILED: u8 = 3;
}

/// Bits of the button mask. The names are the console's buttons; a
/// firmware is free to ignore any of them.
pub mod button {
    pub const UP: u16 = 1 << 0;
    pub const DOWN: u16 = 1 << 1;
    pub const LEFT: u16 = 1 << 2;
    pub const RIGHT: u16 = 1 << 3;
    pub const A: u16 = 1 << 4;
    pub const B: u16 = 1 << 5;
    pub const X: u16 = 1 << 6;
    pub const Y: u16 = 1 << 7;
    pub const L: u16 = 1 << 8;
    pub const R: u16 = 1 << 9;
    pub const START: u16 = 1 << 10;
    pub const SELECT: u16 = 1 << 11;

    /// Every button with the name the PC tool and the logs use for it.
    pub const NAMES: [(&str, u16); 12] = [
        ("up", UP),
        ("down", DOWN),
        ("left", LEFT),
        ("right", RIGHT),
        ("a", A),
        ("b", B),
        ("x", X),
        ("y", Y),
        ("l", L),
        ("r", R),
        ("start", START),
        ("select", SELECT),
    ];

    /// The bit for a button's name, in any letter case.
    pub fn from_name(name: &str) -> Option<u16> {
        NAMES.iter().find(|(known, _)| known.eq_ignore_ascii_case(name)).map(|&(_, bit)| bit)
    }
}

/// Where a screenshot is taken from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Source {
    /// The display panel's own memory, read back over its data-out wire:
    /// what the glass shows.
    Panel = 0,
    /// The firmware's framebuffer: what the firmware believes it drew.
    Framebuffer = 1,
}

impl Source {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Panel),
            1 => Some(Self::Framebuffer),
            _ => None,
        }
    }
}

/// How the pixels of a picture are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PixelFormat {
    /// Two bytes per pixel, low byte first: red in the top 5 bits, green
    /// in the middle 6, blue in the low 5.
    Rgb565 = 0,
    /// One byte per pixel, a number into a palette of 256 RGB565 colours
    /// that is sent along.
    Indexed8 = 1,
}

impl PixelFormat {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Rgb565),
            1 => Some(Self::Indexed8),
            _ => None,
        }
    }

    /// Bytes per pixel.
    pub const fn bytes(self) -> usize {
        match self {
            Self::Rgb565 => 2,
            Self::Indexed8 => 1,
        }
    }
}

/// A payload that does not fit its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// No message has this kind byte (or it is one of the other side's).
    UnknownKind,
    /// Too short, too long, or holding a value that is not allowed.
    BadPayload,
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// Most parameter bytes a [`Request::PanelRaw`] can carry.
pub const MAX_RAW_PARAMS: usize = 16;
/// Most bytes a [`Request::PanelRaw`] can ask the panel for.
pub const MAX_RAW_READ: usize = 256;

/// What the PC asks of the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request<'a> {
    Ping,
    /// Take a screenshot.
    ///
    /// `baud`: 0 to send it at the speed the line is at. Otherwise the
    /// board acknowledges, changes to that speed, waits for
    /// [`Request::Go`] at that speed, sends the picture, and changes
    /// back. `read_khz`: clock for reading the panel, 0 for the
    /// firmware's own choice.
    Shot { source: Source, baud: u32, read_khz: u16 },
    Go,
    /// The set of buttons held down from now on. To press and release,
    /// send the mask and then 0.
    Buttons { mask: u16 },
    /// A tap at a point of the 320x240 screen.
    Tap { x: u16, y: u16 },
    Stats,
    /// For finding out how a panel behaves without flashing new firmware:
    /// send `cmd` and `params` to the panel, clock `dummy_bits` bits and
    /// throw them away, then read `read_len` bytes, all at `clock_khz`
    /// (0: the firmware's own choice).
    PanelRaw { cmd: u8, dummy_bits: u8, read_len: u16, clock_khz: u16, params: &'a [u8] },
}

impl<'a> Request<'a> {
    pub fn kind(&self) -> u8 {
        match self {
            Self::Ping => kind::PING,
            Self::Shot { .. } => kind::SHOT,
            Self::Go => kind::GO,
            Self::Buttons { .. } => kind::BUTTONS,
            Self::Tap { .. } => kind::TAP,
            Self::Stats => kind::STATS,
            Self::PanelRaw { .. } => kind::PANEL_RAW,
        }
    }

    /// Builds the frame for this request in `out` and returns its length.
    /// 64 bytes are enough for any request.
    pub fn encode(&self, seq: u8, out: &mut [u8]) -> Result<usize, EncodeError> {
        let mut payload = [0u8; 6 + MAX_RAW_PARAMS];
        let len = match *self {
            Self::Ping | Self::Go | Self::Stats => 0,
            Self::Shot { source, baud, read_khz } => {
                payload[0] = source as u8;
                payload[1..5].copy_from_slice(&baud.to_le_bytes());
                payload[5..7].copy_from_slice(&read_khz.to_le_bytes());
                7
            }
            Self::Buttons { mask } => {
                payload[..2].copy_from_slice(&mask.to_le_bytes());
                2
            }
            Self::Tap { x, y } => {
                payload[..2].copy_from_slice(&x.to_le_bytes());
                payload[2..4].copy_from_slice(&y.to_le_bytes());
                4
            }
            Self::PanelRaw { cmd, dummy_bits, read_len, clock_khz, params } => {
                if params.len() > MAX_RAW_PARAMS {
                    return Err(EncodeError::PayloadTooLong);
                }
                payload[0] = cmd;
                payload[1] = dummy_bits;
                payload[2..4].copy_from_slice(&read_len.to_le_bytes());
                payload[4..6].copy_from_slice(&clock_khz.to_le_bytes());
                payload[6..6 + params.len()].copy_from_slice(params);
                6 + params.len()
            }
        };
        frame::encode(self.kind(), seq, &payload[..len], out)
    }

    /// Reads a request out of a received frame.
    pub fn parse(frame: &Frame<'a>) -> Result<Self, ParseError> {
        let p = frame.payload;
        let sized = |len: usize| if p.len() == len { Ok(()) } else { Err(ParseError::BadPayload) };
        match frame.kind {
            kind::PING => sized(0).map(|()| Self::Ping),
            kind::GO => sized(0).map(|()| Self::Go),
            kind::STATS => sized(0).map(|()| Self::Stats),
            kind::SHOT => {
                sized(7)?;
                let source = Source::from_u8(p[0]).ok_or(ParseError::BadPayload)?;
                Ok(Self::Shot { source, baud: u32_at(p, 1), read_khz: u16_at(p, 5) })
            }
            kind::BUTTONS => sized(2).map(|()| Self::Buttons { mask: u16_at(p, 0) }),
            kind::TAP => sized(4).map(|()| Self::Tap { x: u16_at(p, 0), y: u16_at(p, 2) }),
            kind::PANEL_RAW => {
                if p.len() < 6 || p.len() > 6 + MAX_RAW_PARAMS {
                    return Err(ParseError::BadPayload);
                }
                let read_len = u16_at(p, 2);
                if usize::from(read_len) > MAX_RAW_READ {
                    return Err(ParseError::BadPayload);
                }
                Ok(Self::PanelRaw { cmd: p[0], dummy_bits: p[1], read_len, clock_khz: u16_at(p, 4), params: &p[6..] })
            }
            _ => Err(ParseError::UnknownKind),
        }
    }
}

/// The buttons held down from now on, on one player's pad (kind
/// [`kind::PLAYER_BUTTONS`]). Player 0 is the same pad as
/// [`Request::Buttons`]; players 1 to 3 are the game's players 2 to 4.
/// Answered with `Ack`: `OK`, `BAD_REQUEST` for a player past 3, or
/// `UNSUPPORTED` from a firmware that does not know it.
///
/// Kept apart from [`Request`] so the firmwares that match every request
/// (the console's and the experiments') need no change for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerButtons {
    /// 0 to [`PlayerButtons::PLAYERS`] - 1.
    pub player: u8,
    pub mask: u16,
}

impl PlayerButtons {
    /// Players a firmware may have (the game interface's four).
    pub const PLAYERS: u8 = 4;

    /// Builds the frame in `out` and returns its length.
    pub fn encode(&self, seq: u8, out: &mut [u8]) -> Result<usize, EncodeError> {
        let [lo, hi] = self.mask.to_le_bytes();
        frame::encode(kind::PLAYER_BUTTONS, seq, &[self.player, lo, hi], out)
    }

    /// Reads one out of a received frame of kind [`kind::PLAYER_BUTTONS`].
    pub fn parse(frame: &Frame<'_>) -> Result<Self, ParseError> {
        if frame.kind != kind::PLAYER_BUTTONS {
            return Err(ParseError::UnknownKind);
        }
        match *frame.payload {
            [player, lo, hi] if player < Self::PLAYERS => {
                Ok(Self { player, mask: u16::from_le_bytes([lo, hi]) })
            }
            _ => Err(ParseError::BadPayload),
        }
    }
}

/// What the board sends to the PC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply<'a> {
    /// The request of kind `request` (and with this frame's `seq`) was
    /// received; `status` is one of [`status`].
    Ack { request: u8, status: u8 },
    ShotBegin { source: Source, format: PixelFormat, width: u16, height: u16 },
    /// 512 bytes: 256 RGB565 colours, low byte first.
    ShotPalette { colours: &'a [u8] },
    /// `packed` unpacks (see [`crate::rle`]) to `pixels` pixels, which
    /// are the picture's pixels from number `first` on, counted row by
    /// row from the top left.
    ShotData { first: u32, pixels: u16, packed: &'a [u8] },
    ShotEnd { pixels: u32, crc32: u32, capture_ms: u32 },
    /// `key=value` lines.
    StatsText { text: &'a [u8] },
    PanelRawReply { bytes: &'a [u8] },
}

/// Bytes of a [`Reply::ShotData`] payload before the packed pixels.
pub const SHOT_DATA_HEADER: usize = 6;

impl<'a> Reply<'a> {
    pub fn kind(&self) -> u8 {
        match self {
            Self::Ack { .. } => kind::ACK,
            Self::ShotBegin { .. } => kind::SHOT_BEGIN,
            Self::ShotPalette { .. } => kind::SHOT_PALETTE,
            Self::ShotData { .. } => kind::SHOT_DATA,
            Self::ShotEnd { .. } => kind::SHOT_END,
            Self::StatsText { .. } => kind::STATS_TEXT,
            Self::PanelRawReply { .. } => kind::PANEL_RAW_REPLY,
        }
    }

    /// Builds the frame for this reply in `out` and returns its length.
    /// `seq` is the request's. A buffer of [`frame::MAX_FRAME`] bytes
    /// holds any reply.
    pub fn encode(&self, seq: u8, out: &mut [u8]) -> Result<usize, EncodeError> {
        let mut small = [0u8; 12];
        let payload: &[u8] = match *self {
            Self::Ack { request, status } => {
                small[0] = request;
                small[1] = status;
                &small[..2]
            }
            Self::ShotBegin { source, format, width, height } => {
                small[0] = source as u8;
                small[1] = format as u8;
                small[2..4].copy_from_slice(&width.to_le_bytes());
                small[4..6].copy_from_slice(&height.to_le_bytes());
                &small[..6]
            }
            Self::ShotEnd { pixels, crc32, capture_ms } => {
                small[..4].copy_from_slice(&pixels.to_le_bytes());
                small[4..8].copy_from_slice(&crc32.to_le_bytes());
                small[8..12].copy_from_slice(&capture_ms.to_le_bytes());
                &small[..12]
            }
            Self::ShotPalette { colours } => colours,
            Self::StatsText { text } => text,
            Self::PanelRawReply { bytes } => bytes,
            Self::ShotData { first, pixels, packed } => {
                // The only reply with a header and a body: put together
                // in place.
                let len = SHOT_DATA_HEADER + packed.len();
                if len > frame::MAX_PAYLOAD {
                    return Err(EncodeError::PayloadTooLong);
                }
                if out.len() < len + frame::OVERHEAD {
                    return Err(EncodeError::BufferTooSmall);
                }
                let body = &mut out[frame::HEADER_LEN..];
                body[..4].copy_from_slice(&first.to_le_bytes());
                body[4..6].copy_from_slice(&pixels.to_le_bytes());
                body[SHOT_DATA_HEADER..len].copy_from_slice(packed);
                return Ok(frame::finish(self.kind(), seq, len, out));
            }
        };
        frame::encode(self.kind(), seq, payload, out)
    }

    /// Reads a reply out of a received frame.
    pub fn parse(frame: &Frame<'a>) -> Result<Self, ParseError> {
        let p = frame.payload;
        let sized = |len: usize| if p.len() == len { Ok(()) } else { Err(ParseError::BadPayload) };
        match frame.kind {
            kind::ACK => sized(2).map(|()| Self::Ack { request: p[0], status: p[1] }),
            kind::SHOT_BEGIN => {
                sized(6)?;
                Ok(Self::ShotBegin {
                    source: Source::from_u8(p[0]).ok_or(ParseError::BadPayload)?,
                    format: PixelFormat::from_u8(p[1]).ok_or(ParseError::BadPayload)?,
                    width: u16_at(p, 2),
                    height: u16_at(p, 4),
                })
            }
            kind::SHOT_PALETTE => sized(512).map(|()| Self::ShotPalette { colours: p }),
            kind::SHOT_DATA => {
                if p.len() < SHOT_DATA_HEADER {
                    return Err(ParseError::BadPayload);
                }
                Ok(Self::ShotData { first: u32_at(p, 0), pixels: u16_at(p, 4), packed: &p[SHOT_DATA_HEADER..] })
            }
            kind::SHOT_END => {
                sized(12)?;
                Ok(Self::ShotEnd { pixels: u32_at(p, 0), crc32: u32_at(p, 4), capture_ms: u32_at(p, 8) })
            }
            kind::STATS_TEXT => Ok(Self::StatsText { text: p }),
            kind::PANEL_RAW_REPLY => Ok(Self::PanelRawReply { bytes: p }),
            _ => Err(ParseError::UnknownKind),
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use crate::frame::{Decoder, Event, MAX_FRAME};

    /// Encodes with `encode`, runs the bytes through a decoder, and hands
    /// the frame to `check`.
    fn through_the_wire(bytes: &[u8], check: impl Fn(&Frame<'_>)) {
        let mut seen = 0;
        Decoder::<MAX_FRAME>::new().feed(bytes, &mut |event| {
            if let Event::Frame(frame) = event {
                check(&frame);
                seen += 1;
            } else {
                panic!("{event:?}");
            }
        });
        assert_eq!(seen, 1);
    }

    #[test]
    fn every_request_survives_the_wire() {
        let requests = [
            Request::Ping,
            Request::Go,
            Request::Stats,
            Request::Shot { source: Source::Panel, baud: 921_600, read_khz: 4000 },
            Request::Shot { source: Source::Framebuffer, baud: 0, read_khz: 0 },
            Request::Buttons { mask: button::A | button::UP },
            Request::Tap { x: 319, y: 239 },
            Request::PanelRaw { cmd: 0x04, dummy_bits: 1, read_len: 3, clock_khz: 1000, params: &[] },
            Request::PanelRaw { cmd: 0x2A, dummy_bits: 0, read_len: 0, clock_khz: 0, params: &[0, 0, 1, 0x3F] },
        ];
        for (seq, request) in requests.iter().enumerate() {
            let mut out = [0u8; 64];
            let len = request.encode(seq as u8, &mut out).unwrap();
            through_the_wire(&out[..len], |frame| {
                assert_eq!(frame.seq, seq as u8);
                assert_eq!(Request::parse(frame), Ok(*request));
            });
        }
    }

    #[test]
    fn every_reply_survives_the_wire() {
        let palette = [0x5Au8; 512];
        let replies = [
            Reply::Ack { request: kind::SHOT, status: status::OK },
            Reply::ShotBegin { source: Source::Framebuffer, format: PixelFormat::Indexed8, width: 160, height: 120 },
            Reply::ShotPalette { colours: &palette },
            Reply::ShotData { first: 70_000, pixels: 320, packed: &[0xFF, 1, 2, 0xFF, 1, 2, 0xBC, 1, 2] },
            Reply::ShotEnd { pixels: 76_800, crc32: 0xDEAD_BEEF, capture_ms: 612 },
            Reply::StatsText { text: b"heap_free=1000\nuptime_ms=5\n" },
            Reply::PanelRawReply { bytes: &[0x81, 0x81, 0xB3] },
        ];
        for reply in &replies {
            let mut out = [0u8; MAX_FRAME];
            let len = reply.encode(42, &mut out).unwrap();
            through_the_wire(&out[..len], |frame| {
                assert_eq!(frame.seq, 42);
                assert_eq!(Reply::parse(frame), Ok(*reply));
            });
        }
    }

    #[test]
    fn wrong_sizes_and_values_are_refused() {
        let bad = [
            Frame { kind: kind::TAP, seq: 0, payload: &[1, 2, 3] },
            Frame { kind: kind::SHOT, seq: 0, payload: &[9, 0, 0, 0, 0, 0, 0] },
            Frame { kind: kind::PING, seq: 0, payload: &[0] },
            Frame { kind: kind::PANEL_RAW, seq: 0, payload: &[0x2E, 8, 0xFF, 0xFF, 0, 0] },
        ];
        for frame in &bad {
            assert_eq!(Request::parse(frame), Err(ParseError::BadPayload), "{frame:?}");
        }
        assert_eq!(Request::parse(&Frame { kind: kind::ACK, seq: 0, payload: &[] }), Err(ParseError::UnknownKind));
        assert_eq!(Reply::parse(&Frame { kind: kind::PING, seq: 0, payload: &[] }), Err(ParseError::UnknownKind));
    }

    #[test]
    fn player_buttons_survive_the_wire_and_name_a_real_player() {
        for player in 0..PlayerButtons::PLAYERS {
            let pb = PlayerButtons { player, mask: button::B | button::SELECT };
            let mut out = [0u8; 64];
            let len = pb.encode(7, &mut out).unwrap();
            through_the_wire(&out[..len], |frame| {
                assert_eq!(frame.kind, kind::PLAYER_BUTTONS);
                assert_eq!(PlayerButtons::parse(frame), Ok(pb));
                // Not a `Request`: a firmware that only knows those says
                // "unsupported".
                assert_eq!(Request::parse(frame), Err(ParseError::UnknownKind));
            });
        }
        for payload in [&[4u8, 0, 0][..], &[255, 1, 0], &[1, 0], &[1, 0, 0, 0]] {
            let f = Frame { kind: kind::PLAYER_BUTTONS, seq: 0, payload };
            assert_eq!(PlayerButtons::parse(&f), Err(ParseError::BadPayload), "{payload:?}");
        }
        let other = Frame { kind: kind::BUTTONS, seq: 0, payload: &[0, 0] };
        assert_eq!(PlayerButtons::parse(&other), Err(ParseError::UnknownKind));
    }

    #[test]
    fn button_names() {
        assert_eq!(button::from_name("A"), Some(button::A));
        assert_eq!(button::from_name("select"), Some(button::SELECT));
        assert_eq!(button::from_name("turbo"), None);
        // Each name has its own bit.
        let all = button::NAMES.iter().fold(0u16, |mask, &(_, bit)| mask | bit);
        assert_eq!(all.count_ones() as usize, button::NAMES.len());
    }
}
