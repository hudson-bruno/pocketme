//! Experiment: what happens when code slot memory is read or written in
//! pieces smaller than a 32-bit word?
//!
//! The chip's manual says the executable RAM answers only to whole words
//! at addresses that are multiples of 4, and that anything else stops the
//! program with a fault. This tries it, one access per start: before each
//! try a note is written to the memory that outlasts a restart, so that
//! the start after a fault knows which try it was and goes on to the next.
//!
//! The finding matters for games: a compiler that put a text or a table
//! into the code slot and read it byte by byte would crash the console.

use core::ptr;

use esp_hal::rtc_cntl::SocResetReason;
use game_api::layout;
use log::{info, warn};
use portable_atomic::{AtomicU32, Ordering};

use crate::crash::{EXPERIMENT_RESULTS, EXPERIMENT_STEP};

/// Says "the step counter was written by this firmware".
const MARK: u32 = 0x7072_6f62; // "prob"

#[esp_hal::ram(unstable(rtc_fast, persistent))]
static PROBE_MARK: AtomicU32 = AtomicU32::new(0);
/// What each try read back, for the tries that came back.
#[esp_hal::ram(unstable(rtc_fast, persistent))]
static VALUES: [AtomicU32; TRIES.len()] = [const { AtomicU32::new(0) }; TRIES.len()];

/// The word put into the code slot before every try.
const WORD: u32 = 0x4433_2211;
/// Where: a multiple of 4, in the middle of the slot.
const PLACE: u32 = layout::CODE_SLOT_ADDR + 0x8000;

#[derive(Clone, Copy)]
enum Access {
    ReadWord,
    ReadByte { offset: u32 },
    ReadHalf { offset: u32 },
    WriteByte { offset: u32 },
    WriteHalf { offset: u32 },
    /// The same byte read from the data slot, where it is allowed.
    ReadByteInDataSlot,
}

const TRIES: [(Access, &str); 8] = [
    (Access::ReadWord, "read a 32-bit word at a multiple of 4 (what the loader does)"),
    (Access::ReadByteInDataSlot, "read one byte in the DATA slot (for comparison)"),
    (Access::ReadByte { offset: 0 }, "read one byte at a multiple of 4"),
    (Access::ReadByte { offset: 1 }, "read one byte at a multiple of 4, plus 1"),
    (Access::ReadHalf { offset: 0 }, "read 16 bits at a multiple of 4"),
    (Access::ReadHalf { offset: 2 }, "read 16 bits at a multiple of 4, plus 2"),
    (Access::WriteByte { offset: 0 }, "write one byte at a multiple of 4, then read the word"),
    (Access::WriteHalf { offset: 2 }, "write 16 bits at a multiple of 4, plus 2, then read the word"),
];

/// Two bits per try in `EXPERIMENT_RESULTS`.
const STARTED: u32 = 1;
const CAME_BACK: u32 = 2;

fn result_of(step: usize) -> u32 {
    (EXPERIMENT_RESULTS.load(Ordering::Relaxed) >> (2 * step)) & 3
}

fn set_result(step: usize, result: u32) {
    let all = EXPERIMENT_RESULTS.load(Ordering::Relaxed) & !(3 << (2 * step));
    EXPERIMENT_RESULTS.store(all | (result << (2 * step)), Ordering::Relaxed);
}

/// Does the next try, or, when all are done, prints what was found.
pub fn run() {
    // A start that was not the chip restarting itself begins the series.
    let own_restart = matches!(esp_hal::system::reset_reason(), Some(SocResetReason::CoreSw | SocResetReason::Cpu0Sw));
    if !own_restart || PROBE_MARK.load(Ordering::Relaxed) != MARK {
        info!("PROBE: starting the series of {} tries. A try that stops the program is followed by a restart.", TRIES.len());
        EXPERIMENT_STEP.store(0, Ordering::Relaxed);
        EXPERIMENT_RESULTS.store(0, Ordering::Relaxed);
        PROBE_MARK.store(MARK, Ordering::Relaxed);
    }

    loop {
        let step = EXPERIMENT_STEP.load(Ordering::Relaxed) as usize;
        let Some(&(access, what)) = TRIES.get(step) else { break };
        // Written down first: if the try stops the program, the next
        // start goes on with the one after it.
        EXPERIMENT_STEP.store(step as u32 + 1, Ordering::Relaxed);
        set_result(step, STARTED);
        warn!("PROBE try {step}: {what} ...");

        let value = try_it(access);

        VALUES[step].store(value, Ordering::Relaxed);
        set_result(step, CAME_BACK);
        info!("PROBE try {step}: came back with {value:#010x}");
    }

    info!("PROBE RESULTS (the word in the code slot was {WORD:#010x} before every try):");
    for (step, (_, what)) in TRIES.iter().enumerate() {
        match result_of(step) {
            CAME_BACK => info!("PROBE RESULT {step}: OK, value {:#010x}: {what}", VALUES[step].load(Ordering::Relaxed)),
            STARTED => warn!("PROBE RESULT {step}: STOPPED THE PROGRAM (it restarted): {what}"),
            _ => warn!("PROBE RESULT {step}: not tried: {what}"),
        }
    }
    // The next start by the chip itself would print the same again. A new
    // series starts with the reset button or a new flash.
}

/// One access. "Volatile" makes the compiler do exactly this access, with
/// exactly this width, and not something cleverer.
#[inline(never)]
fn try_it(access: Access) -> u32 {
    let word = PLACE as *mut u32;
    // SAFETY: the code slot is reserved by the linker and holds no game
    // while the probe runs. Whether the narrow accesses are "safe" is the
    // question being asked: the worst they can do is stop the program,
    // which `crash.rs` turns into a restart.
    unsafe {
        word.write_volatile(WORD);
        match access {
            Access::ReadWord => word.read_volatile(),
            Access::ReadByte { offset } => u32::from(ptr::with_exposed_provenance::<u8>((PLACE + offset) as usize).read_volatile()),
            Access::ReadHalf { offset } => u32::from(ptr::with_exposed_provenance::<u16>((PLACE + offset) as usize).read_volatile()),
            Access::WriteByte { offset } => {
                ptr::with_exposed_provenance_mut::<u8>((PLACE + offset) as usize).write_volatile(0xEE);
                word.read_volatile()
            }
            Access::WriteHalf { offset } => {
                ptr::with_exposed_provenance_mut::<u16>((PLACE + offset) as usize).write_volatile(0xBEEF);
                word.read_volatile()
            }
            Access::ReadByteInDataSlot => {
                let place = layout::DATA_SLOT_ADDR as *mut u32;
                place.write_volatile(WORD);
                u32::from(place.cast::<u8>().add(1).read_volatile())
            }
        }
    }
}
