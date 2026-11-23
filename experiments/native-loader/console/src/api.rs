//! The console's half of the border: the functions a game may call.
//!
//! The game gets a pointer to [`API`] and calls through it. These
//! functions run on the console's main stack, in the middle of a game
//! function, so they must be quick and must not wait for anything.

use core::{cell::RefCell, slice};

use critical_section::Mutex;
use esp_hal::time::Instant;
use game_api::{API_VERSION, ConsoleApi, layout};
use heapless::Vec;
use log::{info, warn};
use portable_atomic::{AtomicU32, Ordering};

use crate::crash;

/// The table handed to every game.
pub static API: ConsoleApi = ConsoleApi {
    size: size_of::<ConsoleApi>() as u32,
    version: API_VERSION,
    log,
    report,
    abort,
    echo_sum,
    time_us,
};

/// The longest text a game may log in one call.
const LOG_MAX: u32 = 200;

/// What the game reported since [`start_recording`], in order.
static REPORTS: Mutex<RefCell<Vec<(u32, u32), 32>>> = Mutex::new(RefCell::new(Vec::new()));
/// The first text the game logged since [`start_recording`], for the
/// checks.
static LAST_LOG: Mutex<RefCell<Vec<u8, 64>>> = Mutex::new(RefCell::new(Vec::new()));
static LOG_CALLS: AtomicU32 = AtomicU32::new(0);
static ECHO_CALLS: AtomicU32 = AtomicU32::new(0);
/// When not zero, `report` does not print (the long soak would otherwise
/// be nothing but these lines).
static QUIET: AtomicU32 = AtomicU32::new(0);

/// Forgets what an earlier game reported.
pub fn start_recording(quiet: bool) {
    critical_section::with(|cs| {
        REPORTS.borrow_ref_mut(cs).clear();
        LAST_LOG.borrow_ref_mut(cs).clear();
    });
    LOG_CALLS.store(0, Ordering::Relaxed);
    ECHO_CALLS.store(0, Ordering::Relaxed);
    QUIET.store(u32::from(quiet), Ordering::Relaxed);
}

/// The value the game reported under `id`, if it did.
pub fn reported(id: u32) -> Option<u32> {
    critical_section::with(|cs| {
        REPORTS.borrow_ref(cs).iter().rev().find(|(reported, _)| *reported == id).map(|(_, value)| *value)
    })
}

/// How often the game called `log` and `echo_sum`.
pub fn call_counts() -> (u32, u32) {
    (LOG_CALLS.load(Ordering::Relaxed), ECHO_CALLS.load(Ordering::Relaxed))
}

/// Whether the first text the game logged was exactly `text`.
pub fn first_log_was(text: &str) -> bool {
    critical_section::with(|cs| LAST_LOG.borrow_ref(cs).as_slice() == text.as_bytes())
}

extern "C" fn log(text: *const u8, len: u32) {
    let calls = LOG_CALLS.fetch_add(1, Ordering::Relaxed);
    // A game can only have text in its own data slot. Anything else is a
    // bug in the game, and reading it could stop the console (the code
    // slot cannot be read a byte at a time).
    let start = text as u32;
    let inside = len <= LOG_MAX
        && start >= layout::DATA_SLOT_ADDR
        && start - layout::DATA_SLOT_ADDR <= layout::DATA_SLOT_SIZE - len;
    if !inside {
        warn!("game: log call refused: {len} bytes at {start:#010x} are not in the game's data");
        return;
    }
    // SAFETY: the range was just checked to lie inside the data slot,
    // which is plain RAM that the game owns. The game is not running
    // while it is inside this call.
    let bytes = unsafe { slice::from_raw_parts(text, len as usize) };
    if calls == 0 {
        critical_section::with(|cs| {
            let mut kept = LAST_LOG.borrow_ref_mut(cs);
            kept.clear();
            let _ = kept.extend_from_slice(&bytes[..bytes.len().min(64)]);
        });
    }
    if QUIET.load(Ordering::Relaxed) != 0 {
        return;
    }
    match core::str::from_utf8(bytes) {
        Ok(text) => info!("game says: \"{text}\" ({len} bytes at {start:#010x})"),
        Err(_) => warn!("game: logged {len} bytes at {start:#010x} that are not text"),
    }
}

extern "C" fn report(id: u32, value: u32) {
    let kept = critical_section::with(|cs| REPORTS.borrow_ref_mut(cs).push((id, value)).is_ok());
    if QUIET.load(Ordering::Relaxed) == 0 {
        info!("game reports #{id} = {value:#010x}{}", if kept { "" } else { " (list full, not kept)" });
    }
}

extern "C" fn abort(code: u32, detail: u32) -> ! {
    log::error!("game: asked to be stopped, code {code}, detail {detail}. Restarting the console.");
    crash::game_aborted(code, detail)
}

extern "C" fn echo_sum(a: u32, b: u32) -> u32 {
    ECHO_CALLS.fetch_add(1, Ordering::Relaxed);
    a.wrapping_add(b)
}

extern "C" fn time_us() -> u64 {
    Instant::now().duration_since_epoch().as_micros()
}
