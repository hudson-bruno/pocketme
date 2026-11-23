//! Frames: how a message is marked off from the log text around it.
//!
//! ```text
//! byte 0      0xFE   start marker, first byte
//! byte 1      0xDB   start marker, second byte
//! byte 2      kind   what the message is (see `msg::kind`)
//! byte 3      seq    chosen by the PC; the board's answers repeat it
//! byte 4,5    len    payload length, low byte first, at most MAX_PAYLOAD
//! byte 6      hcrc   CRC-8 of bytes 2..=5
//! byte 7..    payload
//! last two    crc    CRC-16/CCITT-FALSE of bytes 2..end of payload, low byte first
//! ```
//!
//! Why this shape:
//!
//! * The line also carries log text, which is UTF-8, and maybe NMEA
//!   sentences from a GPS receiver, which are ASCII. Byte 0xFE can appear
//!   in neither, so real text never looks like a start marker.
//! * Noise can contain anything. The header has its own small checksum so
//!   that a false marker is dropped after seven bytes, instead of making
//!   the reader swallow a made-up length of what follows.
//! * When any check fails the reader gives up one byte and looks again
//!   from the next, so a real frame that follows junk is still found.

use crate::crc::{CRC16_INIT, crc8, crc16_update};

/// The two bytes every frame starts with.
pub const SOF: [u8; 2] = [0xFE, 0xDB];

/// Bytes before the payload.
pub const HEADER_LEN: usize = 7;

/// Bytes around the payload: header and the closing checksum.
pub const OVERHEAD: usize = HEADER_LEN + 2;

/// The longest payload anyone may send. Small on purpose: a frame is the
/// unit that is lost when a byte is damaged, and the unit a firmware must
/// send without log text getting in between.
pub const MAX_PAYLOAD: usize = 1024;

/// A buffer of this size holds any frame.
pub const MAX_FRAME: usize = MAX_PAYLOAD + OVERHEAD;

/// What can go wrong when building a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    /// The payload is longer than [`MAX_PAYLOAD`].
    PayloadTooLong,
    /// The output buffer cannot hold the frame.
    BufferTooSmall,
}

/// Writes the header for a payload of `len` bytes into `out[..7]`.
fn write_header(kind: u8, seq: u8, len: usize, out: &mut [u8]) {
    out[0] = SOF[0];
    out[1] = SOF[1];
    out[2] = kind;
    out[3] = seq;
    out[4] = len as u8;
    out[5] = (len >> 8) as u8;
    out[6] = crc8(&out[2..6]);
}

/// Builds a frame around `payload` in `out` and returns its length.
pub fn encode(kind: u8, seq: u8, payload: &[u8], out: &mut [u8]) -> Result<usize, EncodeError> {
    if payload.len() > MAX_PAYLOAD {
        return Err(EncodeError::PayloadTooLong);
    }
    let total = payload.len() + OVERHEAD;
    if out.len() < total {
        return Err(EncodeError::BufferTooSmall);
    }
    out[HEADER_LEN..HEADER_LEN + payload.len()].copy_from_slice(payload);
    Ok(finish(kind, seq, payload.len(), out))
}

/// For a payload that was written straight into `out[HEADER_LEN..]`, to
/// save a copy: adds the header and the checksum around it and returns
/// the frame's length. `out` must be at least `len + OVERHEAD` long and
/// `len` at most [`MAX_PAYLOAD`], or this panics.
pub fn finish(kind: u8, seq: u8, len: usize, out: &mut [u8]) -> usize {
    assert!(len <= MAX_PAYLOAD);
    write_header(kind, seq, len, out);
    let end = HEADER_LEN + len;
    let crc = crc16_update(CRC16_INIT, &out[2..end]);
    out[end] = crc as u8;
    out[end + 1] = (crc >> 8) as u8;
    end + 2
}

/// One received message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> {
    pub kind: u8,
    pub seq: u8,
    pub payload: &'a [u8],
}

/// What a [`Decoder`] makes of the bytes it is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event<'a> {
    /// A byte that belongs to no frame: log text, or junk.
    Text(u8),
    /// A frame whose checks all passed.
    Frame(Frame<'a>),
    /// A frame with a good header whose payload was damaged or cut short.
    /// It is dropped; the sender has to repeat it.
    Corrupt,
}

/// Finds frames in a stream of bytes, and hands back everything else as
/// text.
///
/// `BUF` is the size of its buffer in bytes. It accepts payloads of up to
/// `BUF - OVERHEAD` bytes and treats longer frames as junk. A firmware
/// that only receives short commands can use a small one
/// ([`COMMAND_DECODER_BUF`]); the PC uses [`MAX_FRAME`].
pub struct Decoder<const BUF: usize> {
    buf: [u8; BUF],
    /// The bytes not yet dealt with are `buf[start..end]`.
    start: usize,
    end: usize,
}

/// Buffer size for a decoder that only has to take the PC's commands.
pub const COMMAND_DECODER_BUF: usize = 96;

impl<const BUF: usize> Default for Decoder<BUF> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const BUF: usize> Decoder<BUF> {
    pub const fn new() -> Self {
        assert!(BUF > OVERHEAD);
        Self { buf: [0; BUF], start: 0, end: 0 }
    }

    /// The longest payload this decoder accepts.
    pub const fn max_payload(&self) -> usize {
        let room = BUF - OVERHEAD;
        if room < MAX_PAYLOAD { room } else { MAX_PAYLOAD }
    }

    /// True while the first bytes of a possible frame are being held back.
    pub fn mid_frame(&self) -> bool {
        self.start != self.end
    }

    /// Takes received bytes. `sink` is called for every byte of text and
    /// every frame they complete, in the order they arrived.
    pub fn feed(&mut self, bytes: &[u8], sink: &mut impl FnMut(Event<'_>)) {
        for &byte in bytes {
            self.push(byte, sink);
        }
    }

    /// Takes one received byte.
    pub fn push(&mut self, byte: u8, sink: &mut impl FnMut(Event<'_>)) {
        if self.end == BUF {
            // Make room by moving the waiting bytes to the front. They
            // never fill the whole buffer: a frame that would not fit is
            // refused at its header.
            self.buf.copy_within(self.start..self.end, 0);
            self.end -= self.start;
            self.start = 0;
        }
        self.buf[self.end] = byte;
        self.end += 1;
        self.scan(sink);
    }

    /// To be called when the line has been quiet for a while (100 ms is
    /// plenty). A frame is sent in one go, so bytes still held back by
    /// then are not the start of one: they are given up, and whatever real
    /// frame hides further on in them is found.
    pub fn idle(&mut self, sink: &mut impl FnMut(Event<'_>)) {
        while self.start != self.end {
            if self.header_ok() {
                // A good header with too little behind it: a cut frame.
                sink(Event::Corrupt);
                self.skip_frame(self.end - self.start);
            } else {
                sink(Event::Text(self.buf[self.start]));
                self.start += 1;
            }
            self.scan(sink);
        }
    }

    /// Whether the waiting bytes begin with a complete, valid header.
    fn header_ok(&self) -> bool {
        let held = &self.buf[self.start..self.end];
        held.len() >= HEADER_LEN
            && held[..2] == SOF
            && crc8(&held[2..6]) == held[6]
            && usize::from(u16::from_le_bytes([held[4], held[5]])) <= self.max_payload()
    }

    /// Drops a damaged frame of `total` bytes from the front, but only up
    /// to the next start marker inside it: a damaged frame may have
    /// swallowed the beginning of a good one.
    fn skip_frame(&mut self, total: usize) {
        let frame = &self.buf[self.start..self.start + total];
        let next = frame[1..].windows(2).position(|pair| pair == SOF).map(|at| at + 1);
        let mut skip = next.unwrap_or(total);
        // A marker's first byte may be the frame's last: keep it.
        if next.is_none() && frame[total - 1] == SOF[0] {
            skip = total - 1;
        }
        self.start += skip;
    }

    /// Deals with as many of the waiting bytes as can be decided now.
    fn scan(&mut self, sink: &mut impl FnMut(Event<'_>)) {
        loop {
            let held = &self.buf[self.start..self.end];
            let Some(&first) = held.first() else {
                break;
            };
            // Is this still the possible start of a frame? Each test
            // either says no (then the first byte is text), or needs more
            // bytes (then wait).
            let no_frame = if first != SOF[0] {
                true
            } else if held.len() < 2 {
                break;
            } else if held[1] != SOF[1] {
                true
            } else if held.len() < HEADER_LEN {
                break;
            } else {
                !self.header_ok()
            };
            if no_frame {
                sink(Event::Text(first));
                self.start += 1;
                continue;
            }

            let len = usize::from(u16::from_le_bytes([held[4], held[5]]));
            let total = len + OVERHEAD;
            if held.len() < total {
                break;
            }
            let end = HEADER_LEN + len;
            let crc = u16::from_le_bytes([held[end], held[end + 1]]);
            if crc16_update(CRC16_INIT, &held[2..end]) == crc {
                sink(Event::Frame(Frame { kind: held[2], seq: held[3], payload: &held[HEADER_LEN..end] }));
                self.start += total;
            } else {
                sink(Event::Corrupt);
                self.skip_frame(total);
            }
        }
        if self.start == self.end {
            self.start = 0;
            self.end = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::vec::Vec;

    use super::*;

    /// What came out of a decoder, in a form that is easy to compare.
    #[derive(Debug, PartialEq, Eq, Clone)]
    enum Got {
        Text(Vec<u8>),
        Frame(u8, u8, Vec<u8>),
        Corrupt,
    }

    fn run<const BUF: usize>(decoder: &mut Decoder<BUF>, bytes: &[u8], idle_at_end: bool) -> Vec<Got> {
        let mut got: Vec<Got> = Vec::new();
        let mut sink = |event: Event<'_>| match event {
            Event::Text(byte) => {
                if let Some(Got::Text(text)) = got.last_mut() {
                    text.push(byte);
                } else {
                    got.push(Got::Text(std::vec![byte]));
                }
            }
            Event::Frame(frame) => got.push(Got::Frame(frame.kind, frame.seq, frame.payload.to_vec())),
            Event::Corrupt => got.push(Got::Corrupt),
        };
        decoder.feed(bytes, &mut sink);
        if idle_at_end {
            decoder.idle(&mut sink);
        }
        got
    }

    fn frame(kind: u8, seq: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = std::vec![0u8; payload.len() + OVERHEAD];
        let n = encode(kind, seq, payload, &mut out).unwrap();
        assert_eq!(n, out.len());
        out
    }

    #[test]
    fn frame_between_log_lines() {
        let mut stream = b"INFO - hello\r\n".to_vec();
        stream.extend(frame(0x85, 7, b"heap=1234"));
        stream.extend(b"INFO - bye\r\n");
        let got = run(&mut Decoder::<MAX_FRAME>::new(), &stream, false);
        assert_eq!(
            got,
            [
                Got::Text(b"INFO - hello\r\n".to_vec()),
                Got::Frame(0x85, 7, b"heap=1234".to_vec()),
                Got::Text(b"INFO - bye\r\n".to_vec()),
            ]
        );
    }

    #[test]
    fn empty_and_largest_payload() {
        let big: Vec<u8> = (0..MAX_PAYLOAD).map(|i| (i * 7) as u8).collect();
        let mut stream = frame(1, 0, &[]);
        stream.extend(frame(2, 255, &big));
        let got = run(&mut Decoder::<MAX_FRAME>::new(), &stream, false);
        assert_eq!(got, [Got::Frame(1, 0, Vec::new()), Got::Frame(2, 255, big)]);
    }

    #[test]
    fn payload_may_contain_the_start_marker() {
        let payload = [0xFE, 0xDB, 0xFE, 0xDB, 0x01, 0xFE];
        let mut stream = frame(3, 1, &payload);
        stream.extend(frame(3, 2, &payload));
        let got = run(&mut Decoder::<64>::new(), &stream, false);
        assert_eq!(got, [Got::Frame(3, 1, payload.to_vec()), Got::Frame(3, 2, payload.to_vec())]);
    }

    #[test]
    fn nmea_and_noise_around_a_command() {
        // What a GPS receiver on the same wire sends, then noise that
        // contains the start marker more than once, then a real command
        // with no gap before it.
        let mut stream = b"$GPGGA,,,,,,0,00,99.99,,,,,,*48\r\n".to_vec();
        stream.extend([0xFE, 0xFE, 0xDB, 0x00, 0x11, 0xFE, 0xDB, 0xFE]);
        stream.extend(frame(0x05, 9, &[10, 0, 20, 0]));
        stream.extend(b"$GPGSV,1,1,00*79\r\n");
        let got = run(&mut Decoder::<COMMAND_DECODER_BUF>::new(), &stream, true);
        let frames: Vec<_> = got.iter().filter(|g| matches!(g, Got::Frame(..))).collect();
        assert_eq!(frames, [&Got::Frame(0x05, 9, std::vec![10, 0, 20, 0])]);
        // Nothing was lost: text and frame bytes add up to the input.
        let text: usize = got.iter().map(|g| if let Got::Text(t) = g { t.len() } else { 0 }).sum();
        assert_eq!(text + 4 + OVERHEAD, stream.len());
    }

    #[test]
    fn false_header_does_not_swallow_the_real_frame() {
        // A header that passes its own check and claims 40 bytes of
        // payload, cut off after 3. The real command comes right after.
        let mut fake = frame(0x02, 1, &[0xAA; 40]);
        fake.truncate(HEADER_LEN + 3);
        let real = frame(0x04, 2, &[0x10, 0x00]);
        let mut stream = fake;
        stream.extend(&real);
        // Enough bytes follow to fill the claimed length, so the decoder
        // can see that the checksum fails without waiting for a pause.
        stream.extend([b'x'; 60]);
        let got = run(&mut Decoder::<COMMAND_DECODER_BUF>::new(), &stream, true);
        assert!(got.contains(&Got::Corrupt));
        assert!(got.contains(&Got::Frame(0x04, 2, std::vec![0x10, 0x00])), "{got:?}");
    }

    #[test]
    fn cut_frame_is_given_up_when_the_line_goes_quiet() {
        let mut cut = frame(0x02, 1, &[0xAA; 40]);
        cut.truncate(HEADER_LEN + 3);
        let mut decoder = Decoder::<COMMAND_DECODER_BUF>::new();
        assert_eq!(run(&mut decoder, &cut, false), []);
        assert!(decoder.mid_frame());
        // After the pause the cut frame is gone and the next one is read.
        let mut sink_got = run(&mut decoder, &[], true);
        assert_eq!(sink_got, [Got::Corrupt]);
        assert!(!decoder.mid_frame());
        sink_got = run(&mut decoder, &frame(0x01, 3, &[]), false);
        assert_eq!(sink_got, [Got::Frame(0x01, 3, Vec::new())]);
    }

    #[test]
    fn damaged_byte_is_caught() {
        let good = frame(0x83, 4, b"some pixels");
        for at in 2..good.len() {
            let mut bad = good.clone();
            bad[at] ^= 0x20;
            let got = run(&mut Decoder::<MAX_FRAME>::new(), &bad, true);
            assert!(!got.iter().any(|g| matches!(g, Got::Frame(..))), "flip at {at} went through: {got:?}");
        }
    }

    #[test]
    fn frame_too_long_for_a_small_decoder_is_junk() {
        let long = frame(0x83, 1, &[0x55; 200]);
        let mut stream = long.clone();
        stream.extend(frame(0x01, 2, &[]));
        let got = run(&mut Decoder::<COMMAND_DECODER_BUF>::new(), &stream, true);
        let frames: Vec<_> = got.iter().filter(|g| matches!(g, Got::Frame(..))).collect();
        assert_eq!(frames, [&Got::Frame(0x01, 2, Vec::new())]);
    }

    #[test]
    fn random_junk_never_panics_and_frames_survive() {
        // A fixed pseudo-random stream, heavy in marker bytes, with real
        // frames dropped into it at intervals.
        let mut state = 0x1234_5678_u32;
        let mut next = move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        };
        let mut stream = Vec::new();
        let mut sent = Vec::new();
        for round in 0..200u32 {
            for _ in 0..(next() % 40) {
                let byte = match next() % 4 {
                    0 => 0xFE,
                    1 => 0xDB,
                    _ => next(),
                };
                stream.push(byte);
            }
            let payload: Vec<u8> = (0..(next() % 20)).map(|_| next()).collect();
            stream.extend(frame(0x04, round as u8, &payload));
            sent.push(Got::Frame(0x04, round as u8, payload));
            // A gap as long as the largest frame the decoder takes: no
            // junk header can then hold back the frame after it. (On the
            // real line the PC's pause and `idle` do the same job.)
            stream.extend([b'.'; COMMAND_DECODER_BUF]);
        }
        let got = run(&mut Decoder::<COMMAND_DECODER_BUF>::new(), &stream, true);
        let frames: Vec<Got> = got.into_iter().filter(|g| matches!(g, Got::Frame(..))).collect();
        // Every real frame arrives, in order. (Junk could in theory
        // form an extra valid frame; with this fixed stream it does not.)
        assert_eq!(frames, sent);
    }

    #[test]
    fn encode_refuses_what_does_not_fit() {
        let mut small = [0u8; 10];
        assert_eq!(encode(1, 1, &[0; 2], &mut small), Err(EncodeError::BufferTooSmall));
        let mut big = [0u8; MAX_FRAME + 10];
        assert_eq!(encode(1, 1, &[0; MAX_PAYLOAD + 1], &mut big), Err(EncodeError::PayloadTooLong));
    }
}
