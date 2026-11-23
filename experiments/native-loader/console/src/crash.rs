//! What the console does when it cannot go on, and how the next start
//! learns about it.
//!
//! A game shares all of its memory with the console. This chip has no way
//! to fence a game in, so after a game has panicked, or made the processor
//! stop on a bad instruction or a bad address, nothing in memory can be
//! trusted any more. The only safe way back is to restart the chip.
//!
//! Before restarting, a short note is written to the small memory of the
//! chip's clock circuit ("RTC fast memory"). A restart the chip does to
//! itself leaves that memory alone; power-on and the reset button clear
//! it. The next start reads the note, reports it, and wipes it.

use esp_hal::delay::Delay;
use portable_atomic::{AtomicU32, Ordering};

/// Says "the words next to me were written by this firmware". The memory
/// holds anything after a reset at a bad moment, and anything else is read
/// as "no note".
const MARK: u32 = 0x6E6F_7465; // "note"

#[esp_hal::ram(unstable(rtc_fast, persistent))]
static NOTE_MARK: AtomicU32 = AtomicU32::new(0);
#[esp_hal::ram(unstable(rtc_fast, persistent))]
static NOTE_KIND: AtomicU32 = AtomicU32::new(0);
#[esp_hal::ram(unstable(rtc_fast, persistent))]
static NOTE_CODE: AtomicU32 = AtomicU32::new(0);
#[esp_hal::ram(unstable(rtc_fast, persistent))]
static NOTE_DETAIL: AtomicU32 = AtomicU32::new(0);
#[esp_hal::ram(unstable(rtc_fast, persistent))]
static NOTE_FRAME: AtomicU32 = AtomicU32::new(0);

/// A counter for experiments that need several restarts (probe.rs). It
/// lives here because it has to outlast a restart like the note does.
#[cfg(feature = "probe-byte-read")]
#[esp_hal::ram(unstable(rtc_fast, persistent))]
pub static EXPERIMENT_STEP: AtomicU32 = AtomicU32::new(0);
/// What those experiments found so far, one bit per step.
#[cfg(feature = "probe-byte-read")]
#[esp_hal::ram(unstable(rtc_fast, persistent))]
pub static EXPERIMENT_RESULTS: AtomicU32 = AtomicU32::new(0);

/// Not zero while the console is inside one of the game's functions. Plain
/// RAM: it only has to last until the panic handler looks at it.
static IN_GAME: AtomicU32 = AtomicU32::new(0);
/// The frame the game was last asked to draw.
static FRAME: AtomicU32 = AtomicU32::new(0);

/// How the start before this one ended, if it ended badly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LastStart {
    /// The game called the console's `abort` (a Rust panic in the game).
    GameAborted { code: u32, detail: u32, frame: u32 },
    /// The console panicked, or the processor stopped on a fault, while
    /// the console was inside a game function.
    StoppedInsideGame { frame: u32 },
    /// The console panicked, or the processor stopped on a fault, in its
    /// own code.
    ConsoleStopped,
}

const KIND_GAME_ABORTED: u32 = 1;
const KIND_STOPPED_IN_GAME: u32 = 2;
const KIND_CONSOLE_STOPPED: u32 = 3;

/// Reads the note the last start left, and wipes it. Call once, early.
pub fn take_note() -> Option<LastStart> {
    if NOTE_MARK.swap(0, Ordering::Relaxed) != MARK {
        return None;
    }
    let frame = NOTE_FRAME.load(Ordering::Relaxed);
    match NOTE_KIND.load(Ordering::Relaxed) {
        KIND_GAME_ABORTED => Some(LastStart::GameAborted {
            code: NOTE_CODE.load(Ordering::Relaxed),
            detail: NOTE_DETAIL.load(Ordering::Relaxed),
            frame,
        }),
        KIND_STOPPED_IN_GAME => Some(LastStart::StoppedInsideGame { frame }),
        KIND_CONSOLE_STOPPED => Some(LastStart::ConsoleStopped),
        _ => None,
    }
}

fn leave_note(kind: u32, code: u32, detail: u32) {
    NOTE_KIND.store(kind, Ordering::Relaxed);
    NOTE_CODE.store(code, Ordering::Relaxed);
    NOTE_DETAIL.store(detail, Ordering::Relaxed);
    NOTE_FRAME.store(FRAME.load(Ordering::Relaxed), Ordering::Relaxed);
    // The mark goes in last: a note is only believed once it is complete.
    NOTE_MARK.store(MARK, Ordering::Relaxed);
}

/// Tells this module that the console is about to call into the game.
pub fn entering_game(frame: u32) {
    FRAME.store(frame, Ordering::Relaxed);
    IN_GAME.store(1, Ordering::Relaxed);
}

/// Tells this module that the game function has returned.
pub fn left_game() {
    IN_GAME.store(0, Ordering::Relaxed);
}

/// Restarts the chip. Whatever was printed last is given time to leave the
/// serial port first: its buffer of 128 bytes takes 11 ms at 115200 baud.
fn restart() -> ! {
    Delay::new().delay_millis(20);
    esp_hal::system::software_reset()
}

/// Where the game's `abort` call ends (see `api.rs`).
pub fn game_aborted(code: u32, detail: u32) -> ! {
    leave_note(KIND_GAME_ABORTED, code, detail);
    restart()
}

/// Where a panic of the console ends, after esp-backtrace has printed its
/// report. A processor fault (bad address, bad instruction) arrives here
/// too: esp-hal turns it into a panic.
#[unsafe(no_mangle)]
fn custom_halt() -> ! {
    let kind = if IN_GAME.load(Ordering::Relaxed) != 0 { KIND_STOPPED_IN_GAME } else { KIND_CONSOLE_STOPPED };
    leave_note(kind, 0, 0);
    restart()
}
