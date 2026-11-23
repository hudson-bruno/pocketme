//! Asking an Xbox-mode pad for its battery level.
//!
//! In its Xbox modes the pad lists input report 04, the battery report,
//! in its descriptor, but it may send it only now and then, or never. So
//! the console asks once per link with a GET_REPORT ("send me input
//! report 04 now") on the HID control channel. There are three outcomes:
//!
//! - The pad answers with the report: the About page shows the level.
//! - The pad answers with an error code (a HANDSHAKE): "not reported".
//! - No answer within [`WAIT_MS`]: "not reported".
//!
//! The pad may also send report 04 by itself; the decoder reads that too
//! (see `decode.rs`).

use core::fmt::Write;

/// The battery report's id in the Xbox modes.
pub const REPORT_ID: u8 = 0x04;

/// How long to wait for the answer, in milliseconds.
pub const WAIT_MS: u32 = 1000;

/// Where the question stands on this link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// Not asked yet: not an Xbox-mode pad, or no report yet.
    NotAsked,
    /// Handed to the host; waiting since this time (None: no clock, so
    /// it never times out).
    Waiting { since: Option<u32> },
    /// The pad sent the report.
    Answered,
    /// The pad answered with this HANDSHAKE error code (for example 2:
    /// "no such report", 3: "this request is not supported").
    Refused(u8),
    /// No answer within [`WAIT_MS`].
    NoAnswer,
}

/// The About-page line. `value` is the battery byte when one came.
///
/// The byte's meaning is the one Xbox pads use: bits 0 and 1 are the
/// level (0 empty to 3 full) and bit 4 means "charging". It is not yet
/// checked on the KingKong 2 Pro, so the raw byte is shown too.
pub fn line(out: &mut impl Write, value: Option<u8>, ask: Ask) {
    let _ = match (value, ask) {
        (Some(b), _) => write!(
            out,
            "Battery: level {} of 3{} (byte {:#04x})",
            b & 0x03,
            if b & 0x10 != 0 { ", charging" } else { "" },
            b
        ),
        (None, Ask::NotAsked) => out.write_str("Battery: not asked yet"),
        (None, Ask::Waiting { .. }) => out.write_str("Battery: asking the pad"),
        // An answer without the byte cannot happen; say so plainly.
        (None, Ask::Answered) => out.write_str("Battery: answer had no level"),
        (None, Ask::Refused(code)) => {
            write!(out, "Battery: not reported (pad said no, code {code})")
        }
        (None, Ask::NoAnswer) => out.write_str("Battery: not reported (no answer in 1 s)"),
    };
}
