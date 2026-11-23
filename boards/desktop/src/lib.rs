//! The simulator: the console's own code (console-core) on a desktop
//! board. Not an emulator of the chip: it cannot show memory limits,
//! speed, tearing or the radio. The board stays the judge of those.
//!
//! * [`board`]: the desktop board, every part of board-api on the PC.
//! * [`controls`]: keyboard, mouse or script, into the board's input.
//! * [`games`]: the games linked into the program.
//! * [`keymap`]: the one table of which key is which button.
//! * [`script`]: the frame-numbered input scripts.
//! * [`run`]: the board's loop, a fixed tick of simulated time.
//! * [`link`]: the debug link over TCP.
//! * [`picture`]: PNG files and fingerprints, no outside library.
//! * [`settings`]: the brightness kept in the store across restarts.
//! * `window` (feature `window`): the SDL3 window.

pub mod board;
pub mod controls;
pub mod games;
pub mod keymap;
pub mod link;
pub mod picture;
pub mod run;
pub mod script;
pub mod settings;
#[cfg(feature = "window")]
pub mod window;

/// The sample card folder that comes with the simulator: one folder for
/// each linked game (see `games`), with its tile picture. See the README.
pub const SAMPLE_CARD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/card");
