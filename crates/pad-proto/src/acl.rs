//! ACL data packets: how L2CAP frames travel between host and controller.
//!
//! An ACL packet is: type 0x02, then a 4-byte header (connection handle
//! in 12 bits, 2 "packet boundary" bits, 2 broadcast bits, then a 16-bit
//! length), then the data. A long L2CAP frame is cut into several ACL
//! packets: the first is marked "start", the others "continuation".
//! Every L2CAP frame starts with its own 4-byte header (length, channel
//! number), which is how the receiver knows when it has the whole frame.

use crate::buf::{MAX_PACKET, Packet, u16_at};

/// Packet boundary flag: first piece of a frame (automatically
/// flushable), the usual start flag in both directions.
pub const PB_START: u8 = 0b10;
/// First piece of a frame, not automatically flushable (allowed from the
/// host; a controller may also use it).
pub const PB_START_NO_FLUSH: u8 = 0b00;
/// A later piece of the same frame.
pub const PB_CONTINUE: u8 = 0b01;

/// The most data one ACL packet can carry through the radio driver: the
/// driver takes at most 259 bytes, type byte and header included.
pub const MAX_FRAGMENT: usize = MAX_PACKET - 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub handle: u16,
    pub pb: u8,
    pub bc: u8,
    pub len: u16,
}

/// Reads an ACL packet (without the 0x02 type byte). Returns the header
/// and the data. `None` if it is shorter than its header says.
pub fn parse(p: &[u8]) -> Option<(Header, &[u8])> {
    let hw = u16_at(p, 0)?;
    let len = u16_at(p, 2)?;
    let data = p.get(4..4 + usize::from(len))?;
    Some((
        Header {
            handle: hw & 0x0FFF,
            pb: ((hw >> 12) & 3) as u8,
            bc: ((hw >> 14) & 3) as u8,
            len,
        },
        data,
    ))
}

/// Builds one ACL packet (type byte included).
pub fn packet(handle: u16, pb: u8, data: &[u8]) -> Packet {
    let mut p = Packet::new();
    p.put_u8(0x02);
    p.put_u16((handle & 0x0FFF) | u16::from(pb & 3) << 12);
    p.put_u16(data.len() as u16);
    p.put(data);
    p
}

/// Cuts a whole L2CAP frame (its 4-byte header included) into ACL
/// packets of at most `max` data bytes each (the smaller of the
/// controller's buffer size and [`MAX_FRAGMENT`]).
pub fn fragments(handle: u16, frame: &[u8], max: usize) -> impl Iterator<Item = Packet> + '_ {
    let max = max.clamp(1, MAX_FRAGMENT);
    frame
        .chunks(max)
        .enumerate()
        .map(move |(i, piece)| packet(handle, if i == 0 { PB_START } else { PB_CONTINUE }, piece))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReassemblyError {
    /// A continuation came with no frame started.
    UnexpectedContinuation,
    /// A new frame started before the last one was whole; the old part
    /// was thrown away.
    Interrupted,
    /// The frame is longer than the buffer; it is skipped.
    TooLong(usize),
    /// More data came than the frame's own header announced.
    Overrun,
}

/// Puts L2CAP frames back together from ACL pieces (one link).
pub struct Reassembler<const N: usize> {
    buf: [u8; N],
    have: usize,
    /// Whole frame length (header included) once known.
    need: Option<usize>,
    active: bool,
    /// Bytes still to skip of a frame that was too long.
    skipping: usize,
    /// Frames thrown away because a new one started before they were whole.
    pub lost: u32,
}

impl<const N: usize> Default for Reassembler<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Reassembler<N> {
    pub const fn new() -> Self {
        Self {
            buf: [0; N],
            have: 0,
            need: None,
            active: false,
            skipping: 0,
            lost: 0,
        }
    }

    pub fn reset(&mut self) {
        self.have = 0;
        self.need = None;
        self.active = false;
        self.skipping = 0;
    }

    /// Adds one piece. Returns the whole frame (L2CAP header included)
    /// when it is complete. An error is something to log; the reassembler
    /// is ready for the next frame afterwards.
    pub fn push(&mut self, pb: u8, data: &[u8]) -> Result<Option<&[u8]>, ReassemblyError> {
        let mut result_err = None;
        if pb == PB_START || pb == PB_START_NO_FLUSH {
            if self.active || self.skipping > 0 {
                result_err = Some(ReassemblyError::Interrupted);
                self.lost += 1;
            }
            self.reset();
            self.active = true;
        } else if self.skipping > 0 {
            self.skipping = self.skipping.saturating_sub(data.len());
            return Ok(None);
        } else if !self.active {
            return Err(ReassemblyError::UnexpectedContinuation);
        }

        if self.have + data.len() > N {
            // Learn the length, if we can, to skip the rest cleanly.
            let total = self.need.or_else(|| {
                let mut head = [0_u8; 2];
                let mut got = 0;
                for &b in self.buf[..self.have].iter().chain(data.iter()).take(2) {
                    head[got] = b;
                    got += 1;
                }
                (got == 2).then(|| usize::from(u16::from_le_bytes(head)) + 4)
            });
            let total = total.unwrap_or(usize::MAX);
            let seen = self.have + data.len();
            self.reset();
            self.skipping = total.saturating_sub(seen);
            return Err(ReassemblyError::TooLong(total));
        }
        self.buf[self.have..self.have + data.len()].copy_from_slice(data);
        self.have += data.len();
        if self.need.is_none() && self.have >= 2 {
            let need = usize::from(u16::from_le_bytes([self.buf[0], self.buf[1]])) + 4;
            if need > N {
                let seen = self.have;
                self.reset();
                self.skipping = need.saturating_sub(seen);
                return Err(ReassemblyError::TooLong(need));
            }
            self.need = Some(need);
        }
        match self.need {
            Some(need) if self.have == need => {
                self.active = false;
                let len = self.have;
                self.have = 0;
                self.need = None;
                // If an unfinished frame was dropped to start this one, it
                // is counted in `lost`; the whole new frame matters more.
                Ok(Some(&self.buf[..len]))
            }
            Some(need) if self.have > need => {
                self.reset();
                Err(ReassemblyError::Overrun)
            }
            _ => match result_err {
                Some(e) => Err(e),
                None => Ok(None),
            },
        }
    }
}

/// Counts the controller's free ACL buffers ("credits"). The controller
/// says how many it has (Read Buffer Size); each packet we send uses one;
/// the Number Of Completed Packets event gives them back.
#[derive(Clone, Copy, Debug, Default)]
pub struct Credits {
    pub total: u16,
    pub free: u16,
}

impl Credits {
    pub fn set_total(&mut self, total: u16) {
        self.total = total;
        self.free = total;
    }

    pub fn take(&mut self) -> bool {
        if self.free == 0 {
            return false;
        }
        self.free -= 1;
        true
    }

    /// Packets done (from Number Of Completed Packets, or all packets of a
    /// link that is gone). Never above the total.
    pub fn give_back(&mut self, n: u16) {
        self.free = self.free.saturating_add(n).min(self.total);
    }

    pub fn in_flight(&self) -> u16 {
        self.total - self.free
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l2cap(cid: u16, payload: &[u8]) -> Vec<u8> {
        let mut f = (payload.len() as u16).to_le_bytes().to_vec();
        f.extend_from_slice(&cid.to_le_bytes());
        f.extend_from_slice(payload);
        f
    }

    #[test]
    fn header_round_trip() {
        let p = packet(0x0B, PB_START, &[1, 2, 3]);
        assert_eq!(p.as_bytes(), &[0x02, 0x0B, 0x20, 0x03, 0x00, 1, 2, 3]);
        let (h, d) = parse(&p.as_bytes()[1..]).unwrap();
        assert_eq!(
            h,
            Header {
                handle: 0x0B,
                pb: PB_START,
                bc: 0,
                len: 3
            }
        );
        assert_eq!(d, &[1, 2, 3]);
        assert!(parse(&[0x0B, 0x20, 0x05, 0x00, 1]).is_none());
    }

    #[test]
    fn split_and_join_again() {
        let payload: Vec<u8> = (0..600_u32).map(|i| i as u8).collect();
        let frame = l2cap(0x40, &payload);
        let parts: Vec<Packet> = fragments(0x0C, &frame, 1021).collect();
        // 604 bytes, at most 254 per packet through the driver.
        assert_eq!(parts.len(), 3);
        assert!(parts.iter().all(|p| p.len() <= MAX_PACKET));
        let mut r: Reassembler<1024> = Reassembler::new();
        let mut out = None;
        for (i, p) in parts.iter().enumerate() {
            let (h, d) = parse(&p.as_bytes()[1..]).unwrap();
            assert_eq!(h.pb, if i == 0 { PB_START } else { PB_CONTINUE });
            if let Some(f) = r.push(h.pb, d).unwrap() {
                out = Some(f.to_vec());
            }
        }
        assert_eq!(out.unwrap(), frame);
    }

    #[test]
    fn small_controller_buffer() {
        let frame = l2cap(0x41, &[9; 20]);
        let parts: Vec<Packet> = fragments(1, &frame, 8).collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[2].len(), 5 + 8);
    }

    #[test]
    fn header_split_across_pieces() {
        let frame = l2cap(0x40, &[7; 10]);
        let mut r: Reassembler<64> = Reassembler::new();
        assert_eq!(r.push(PB_START, &frame[..1]), Ok(None));
        assert_eq!(r.push(PB_CONTINUE, &frame[1..5]), Ok(None));
        assert_eq!(r.push(PB_CONTINUE, &frame[5..]), Ok(Some(&frame[..])));
    }

    #[test]
    fn errors_are_reported_and_recovered() {
        let mut r: Reassembler<32> = Reassembler::new();
        assert_eq!(
            r.push(PB_CONTINUE, &[1, 2]),
            Err(ReassemblyError::UnexpectedContinuation)
        );
        // A frame of 100 bytes: too long, skipped, then the next one works.
        let big = l2cap(0x40, &[0; 96]);
        assert_eq!(
            r.push(PB_START, &big[..20]),
            Err(ReassemblyError::TooLong(100))
        );
        assert_eq!(r.push(PB_CONTINUE, &big[20..60]), Ok(None));
        assert_eq!(r.push(PB_CONTINUE, &big[60..]), Ok(None));
        let small = l2cap(0x40, &[5; 3]);
        assert_eq!(r.push(PB_START, &small), Ok(Some(&small[..])));
        // A start before the last frame was whole.
        let f = l2cap(0x40, &[1; 10]);
        assert_eq!(r.push(PB_START, &f[..6]), Ok(None));
        assert_eq!(
            r.push(PB_START, &small[..4]),
            Err(ReassemblyError::Interrupted)
        );
        assert_eq!(r.push(PB_CONTINUE, &small[4..]), Ok(Some(&small[..])));
    }

    #[test]
    fn credits_count() {
        let mut c = Credits::default();
        c.set_total(2);
        assert!(c.take());
        assert!(c.take());
        assert!(!c.take());
        c.give_back(5);
        assert_eq!(c.free, 2);
        assert_eq!(c.in_flight(), 0);
    }
}
