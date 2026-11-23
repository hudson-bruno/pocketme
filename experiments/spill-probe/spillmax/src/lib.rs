//! SPILLMAX: the spill probe with a large spill (about 40 KB of code past
//! the code slot). See experiments/spill-probe/README.md; the game itself
//! is common/probe.rs.

#![no_std]

/// Bulk functions (README: "Tuning").
pub const PAD: usize = 350;

/// Words in the data buffer: small, the spill takes most of the data
/// slot.
pub const BUF_WORDS: usize = 256;

/// Not measured for this probe (its data is not near the spill); only
/// the one-off SPILLOVER link of SPILL uses this number.
pub const GAP_BYTES: usize = 0;

include!("../../common/probe.rs");
