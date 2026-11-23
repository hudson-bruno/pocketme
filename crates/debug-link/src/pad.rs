//! Injected gamepad input (kind [`KIND`]): the PC plays a gamepad, so the
//! console's real pad code (the board's gamepad host) can be tested
//! without the pad, the radio or a person.
//!
//! One frame kind with an operation byte first, so [`crate::msg::Request`]
//! stays as it is (programs match it whole). A firmware without a gamepad
//! host answers [`crate::msg::status::UNSUPPORTED`], like any kind it does
//! not know.
//!
//! | op | payload after it | what the board does |
//! |---|---|---|
//! | [`op::CONNECT`] | mode u8 ([`mode`]) | a pad in that mode connects (no report yet) |
//! | [`op::REPORT`] | 1..=[`MAX_REPORT`] bytes | one input report, as the pad sends it without its 0xA1 byte |
//! | [`op::DISCONNECT`] | reason u8 | the pad goes (0x13: switched off) |
//! | [`op::REPEAT`] | period_ms u16 | the last report again every `period_ms` with a resting pad's small changes (0: stop) |
//!
//! Every one is answered with `Ack` (request [`KIND`]): `OK`; `FAILED`
//! when the host says no (a real pad is linked, no injection runs, a mode
//! it cannot play); `BAD_REQUEST` for a payload that makes no sense.

use crate::frame::{self, EncodeError, Frame};
use crate::msg::ParseError;

/// The frame kind (a request: below 0x80).
pub const KIND: u8 = 0x08;

/// Longest injected report. A Switch full report is 49 bytes; the Xbox
/// modes' reports are shorter.
pub const MAX_REPORT: usize = 64;

/// The operation byte.
pub mod op {
    pub const CONNECT: u8 = 0;
    pub const REPORT: u8 = 1;
    pub const DISCONNECT: u8 = 2;
    pub const REPEAT: u8 = 3;
}

/// The pad's mode for [`PadRequest::Connect`] (the GuliKit's modes that
/// need no report descriptor).
pub mod mode {
    pub const SWITCH: u8 = 0;
    pub const XINPUT: u8 = 1;
    pub const ANDROID: u8 = 2;

    /// Names as the PC tool and the logs use them.
    pub const NAMES: [(&str, u8); 3] = [("switch", SWITCH), ("xinput", XINPUT), ("android", ANDROID)];

    /// The mode for a name, in any letter case.
    pub fn from_name(name: &str) -> Option<u8> {
        NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|&(_, m)| m)
    }
}

/// One injected-pad request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadRequest<'a> {
    Connect { mode: u8 },
    Report { bytes: &'a [u8] },
    Disconnect { reason: u8 },
    Repeat { period_ms: u16 },
}

impl<'a> PadRequest<'a> {
    /// Builds the frame in `out` and returns its length. 96 bytes are
    /// enough for any of them.
    pub fn encode(&self, seq: u8, out: &mut [u8]) -> Result<usize, EncodeError> {
        let mut p = [0u8; 1 + MAX_REPORT];
        let len = match *self {
            Self::Connect { mode } => {
                p[0] = op::CONNECT;
                p[1] = mode;
                2
            }
            Self::Report { bytes } => {
                if bytes.is_empty() || bytes.len() > MAX_REPORT {
                    return Err(EncodeError::PayloadTooLong);
                }
                p[0] = op::REPORT;
                p[1..1 + bytes.len()].copy_from_slice(bytes);
                1 + bytes.len()
            }
            Self::Disconnect { reason } => {
                p[0] = op::DISCONNECT;
                p[1] = reason;
                2
            }
            Self::Repeat { period_ms } => {
                p[0] = op::REPEAT;
                p[1..3].copy_from_slice(&period_ms.to_le_bytes());
                3
            }
        };
        frame::encode(KIND, seq, &p[..len], out)
    }

    /// Reads one out of a received frame of kind [`KIND`].
    pub fn parse(frame: &Frame<'a>) -> Result<Self, ParseError> {
        if frame.kind != KIND {
            return Err(ParseError::UnknownKind);
        }
        let p = frame.payload;
        match (p.first().copied(), p.len()) {
            (Some(op::CONNECT), 2) => Ok(Self::Connect { mode: p[1] }),
            (Some(op::REPORT), n) if (2..=1 + MAX_REPORT).contains(&n) => Ok(Self::Report { bytes: &p[1..] }),
            (Some(op::DISCONNECT), 2) => Ok(Self::Disconnect { reason: p[1] }),
            (Some(op::REPEAT), 3) => Ok(Self::Repeat { period_ms: u16::from_le_bytes([p[1], p[2]]) }),
            _ => Err(ParseError::BadPayload),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{COMMAND_DECODER_BUF, Decoder, Event};
    use crate::msg::Request;

    fn round_trip(r: PadRequest<'_>) {
        let mut out = [0u8; 128];
        let n = r.encode(42, &mut out).unwrap();
        // The board's own decoder (96 bytes) takes it whole.
        let mut d = Decoder::<COMMAND_DECODER_BUF>::new();
        let mut got = None;
        d.feed(&out[..n], &mut |e| {
            if let Event::Frame(f) = e {
                assert_eq!(f.seq, 42);
                assert_eq!(f.kind, KIND);
                assert_eq!(PadRequest::parse(&f), Ok(r));
                // msg::Request does not know it: the board's old answer.
                assert_eq!(Request::parse(&f), Err(ParseError::UnknownKind));
                got = Some(());
            }
        });
        assert!(got.is_some(), "{r:?}");
    }

    #[test]
    fn every_request_goes_through_the_boards_decoder() {
        round_trip(PadRequest::Connect { mode: mode::SWITCH });
        round_trip(PadRequest::Disconnect { reason: 0x13 });
        round_trip(PadRequest::Repeat { period_ms: 10 });
        round_trip(PadRequest::Repeat { period_ms: 0 });
        let full = [0x30u8; MAX_REPORT];
        round_trip(PadRequest::Report { bytes: &full });
        round_trip(PadRequest::Report { bytes: &[0x01] });
    }

    #[test]
    fn bad_payloads_are_refused() {
        let bad: [&[u8]; 8] = [
            &[],
            &[op::CONNECT],
            &[op::CONNECT, 0, 0],
            &[op::REPORT],
            &[op::DISCONNECT],
            &[op::REPEAT, 1],
            &[9, 0],
            &[op::REPORT; 2 + MAX_REPORT],
        ];
        for p in bad {
            let f = Frame { kind: KIND, seq: 0, payload: p };
            assert_eq!(PadRequest::parse(&f), Err(ParseError::BadPayload), "{p:?}");
        }
        let other = Frame { kind: 0x01, seq: 0, payload: &[] };
        assert_eq!(PadRequest::parse(&other), Err(ParseError::UnknownKind));
        let mut out = [0u8; 128];
        assert!(PadRequest::Report { bytes: &[] }.encode(0, &mut out).is_err());
        assert!(PadRequest::Report { bytes: &[0; MAX_REPORT + 1] }.encode(0, &mut out).is_err());
    }

    #[test]
    fn modes_by_name() {
        assert_eq!(mode::from_name("Switch"), Some(mode::SWITCH));
        assert_eq!(mode::from_name("xinput"), Some(mode::XINPUT));
        assert_eq!(mode::from_name("ANDROID"), Some(mode::ANDROID));
        assert_eq!(mode::from_name("dinput"), None);
    }
}
