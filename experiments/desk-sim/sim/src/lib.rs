//! The PC side of the desktop simulator.
//!
//! Two fake boards stand under the console's code: [`headless`] (no
//! window, a script decides the input) and `window` (SDL3, keyboard and
//! mouse; only with the `window` feature).

pub mod headless;
pub mod picture;
pub mod script;
#[cfg(feature = "window")]
pub mod window;
