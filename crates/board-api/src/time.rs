//! Time, and when the console wants to run again.
//!
//! The console never waits by itself. The board runs the loop:
//!
//! ```text
//! loop {
//!     (the board brings its clock and input up to date)
//!     let wake = console.step(&mut board);   // one frame of work
//!     (the board sleeps until `wake` says so)
//! }
//! ```
//!
//! A test moves the clock itself, so a test of a minute of play takes a
//! few milliseconds and gives the same result every time.

/// The time since the board started, in microseconds.
///
/// u64: it does not run out (a u32 would after 71 minutes).
pub trait Clock {
    /// Microseconds since start. Never goes backwards. During one step it
    /// should not move (on the CYD it does a little; the console reads it
    /// once at the start of a step and uses that number).
    fn now_us(&self) -> u64;
}

/// When the board should call `step` again. The answer of every step.
///
/// Calling early or late is always safe: the console looks at the clock
/// and does only what is due. So a board may call sooner (for example
/// when debug-link bytes arrive), and a busy board may call later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wake {
    /// Call again at this clock time (microseconds). Used for a fixed
    /// frame rate: 30 frames a second asks for "now + 33,333".
    /// Input that arrives before then is simply read in that step.
    At(u64),
    /// Call again as soon as input changes (a touch, a button, a pad, a
    /// link byte), or at this clock time at the latest, whichever comes
    /// first. Lets the chip sleep when nothing moves: saves battery.
    /// `u64::MAX` means "only when input changes".
    InputOr(u64),
}

impl Wake {
    /// The latest time the board should call again.
    pub const fn deadline(self) -> u64 {
        match self {
            Wake::At(t) | Wake::InputOr(t) => t,
        }
    }

    /// Whether input changing should wake the console before the deadline.
    pub const fn on_input(self) -> bool {
        matches!(self, Wake::InputOr(_))
    }
}
