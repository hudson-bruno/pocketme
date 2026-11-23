//! SPILL: the spill probe with a small spill (about 6 to 8 KB of code
//! past the code slot), and its data ending just below the spill. See
//! experiments/spill-probe/README.md; the game itself is
//! common/probe.rs.

#![no_std]

/// Bulk functions. Chosen on the PC so that one of them crosses the
/// start of the spill (README: "Tuning").
pub const PAD: usize = 228;

/// Words in the data buffer. Chosen on the PC so that the data ends at
/// most 64 bytes below the spill (README: "Tuning").
pub const BUF_WORDS: usize = 9080;

/// Bytes between the end of the data and the spill with the numbers
/// above (MEASURED ON THE PC, README). Only the one-off SPILLOVER link
/// uses it.
pub const GAP_BYTES: usize = 28;

include!("../../common/probe.rs");
