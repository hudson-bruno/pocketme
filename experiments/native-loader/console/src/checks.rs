//! What the test game (`../game`) has to report, and the comparison.
//!
//! The numbers come from `tools/expected.py`, which does the game's sums a
//! second way on the PC. Copy its output here when the game changes.

use log::{error, info};

use crate::api;

struct Check {
    /// The number the game reports under (see the list in the game).
    id: u32,
    expected: u32,
    what: &'static str,
}

/// Frames the game is run for. `tools/expected.py` assumes the same.
pub const FRAMES: u32 = 300;

/// The text the game logs first. It proves a string constant was read.
const HELLO: &str = "hello from the game slot";

/// How often the game calls `echo_sum`: 7 times in `init`, and in one
/// frame of every ten.
const ECHO_CALLS: u32 = 7 + FRAMES / 10;

#[rustfmt::skip]
const CHECKS: [Check; 17] = [
    Check { id: 1, expected: 0x000003e8, what: ".data arrives with its value (SCORE)" },
    Check { id: 2, expected: 0x00000000, what: ".bss is zero (UPDATES)" },
    Check { id: 3, expected: 0x00000000, what: ".bss block is all zero (TRAIL)" },
    Check { id: 4, expected: 0x0000002a, what: "echo_sum(40, 2)" },
    Check { id: 5, expected: 0x0000017d, what: "byte table summed" },
    Check { id: 6, expected: 0x00026b4c, what: "16-bit table summed" },
    Check { id: 7, expected: 0xd5b7b8dd, what: "64-bit quotient, low half" },
    Check { id: 8, expected: 0x000000e8, what: "64-bit quotient, high half" },
    Check { id: 9, expected: 0x00000040, what: "64-bit remainder" },
    Check { id: 10, expected: 0x2a484723, what: "signed 64-bit quotient, low half" },
    Check { id: 11, expected: 0x41c20000, what: "float 24.25 as bits" },
    Check { id: 12, expected: 0x00000001, what: "64-bit clock read twice, in order" },
    Check { id: 13, expected: 0x000008e5, what: "memcpy in ROM: bytes copied and summed" },
    Check { id: 20, expected: 0x0000012c, what: "update calls counted in .bss" },
    Check { id: 21, expected: 0x00406708, what: "SCORE after all frames (const match)" },
    Check { id: 22, expected: 0x000017d0, what: "TRAIL after all frames (byte writes)" },
    Check { id: 23, expected: 0x3c959550, what: "HASH after all frames (jump-table match)" },
];

/// Compares everything the game reported with what it should have.
/// `last_update` is what the last call to `update` returned, which must be
/// the same number the game reports as its hash. Returns (passed, total).
pub fn verdict(last_update: u32, quiet: bool) -> (u32, u32) {
    let mut passed = 0;
    let mut total = 0;
    // `expected` is None for a check that is a yes/no and has no number:
    // printing a made-up 0 next to it would read like a measured value.
    let mut check = |ok: bool, what: &str, got: Option<u32>, expected: Option<u32>| {
        total += 1;
        if ok {
            passed += 1;
            if !quiet {
                match expected {
                    Some(expected) => info!("  pass  {what}: {expected:#010x}"),
                    None => info!("  pass  {what}"),
                }
            }
        } else {
            match (got, expected) {
                (Some(got), Some(expected)) => error!("  FAIL  {what}: got {got:#010x}, expected {expected:#010x}"),
                (None, Some(_)) => error!("  FAIL  {what}: the game never reported it"),
                (_, None) => error!("  FAIL  {what}"),
            }
        }
    };

    for entry in &CHECKS {
        let got = api::reported(entry.id);
        check(got == Some(entry.expected), entry.what, got, Some(entry.expected));
    }
    let hash = CHECKS[CHECKS.len() - 1].expected;
    check(last_update == hash, "value returned by the last update call", Some(last_update), Some(hash));

    let (log_calls, echo_calls) = api::call_counts();
    check(api::first_log_was(HELLO), "string constant printed through the console, compared byte for byte", None, None);
    check(log_calls == 2, "log calls (one in init, one in exit)", Some(log_calls), Some(2));
    check(echo_calls == ECHO_CALLS, "echo_sum calls counted by the console", Some(echo_calls), Some(ECHO_CALLS));

    (passed, total)
}
