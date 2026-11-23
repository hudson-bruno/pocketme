//! The test game for the loader experiment.
//!
//! It is not a game to play. It is a checklist in the shape of a game: it
//! uses every kind of memory and every kind of call that a real game will
//! use, and tells the console what it found through `report(id, value)`.
//! The console knows what each value has to be (`console/src/checks.rs`;
//! `tools/expected.py` computes them a second way, on the PC).
//!
//! What each part proves:
//!
//! | here                | proves                                                  |
//! |---------------------|---------------------------------------------------------|
//! | `HELLO`             | a string constant can be read byte by byte (`.rodata`)  |
//! | `SCORE`, `HASH`     | variables arrive with their starting values (`.data`)   |
//! | `UPDATES`, `TRAIL`  | zeroed variables really are zero (`.bss`)               |
//! | `PRIMES`, `SQUARES` | lookup tables of bytes and of 16-bit numbers            |
//! | `bonus`             | a `match` on constants (the compiler makes it a table)  |
//! | `dispatch`          | a `match` on code (the compiler makes it a jump table)  |
//! | `big_math`          | 64-bit multiply and divide (helper routines)            |
//! | `echo_sum`, `time_us` | calls into the console with arguments and results     |
//! | `copy_from_slice`   | a call into the chip's ROM (`memcpy`)                   |
//! | `panic`             | a panic reaches the console without any text formatting |
//!
//! The game has no `main`. The console calls `game_entry` once, and then
//! `init`, `update` (every frame) and `exit` through the table that
//! `game_entry` returned.

#![no_std]
#![no_main]

use core::{
    panic::PanicInfo,
    ptr,
    sync::atomic::{AtomicPtr, AtomicU32, Ordering},
};

use game_api::{API_VERSION, ConsoleApi, GameCallbacks, abort_code};

// ---------------------------------------------------------------------
// Memory of each kind
// ---------------------------------------------------------------------

/// A string constant. It lands in `.rodata`, in the data slot.
static HELLO: &str = "hello from the game slot";

/// A table of bytes in `.rodata`.
static PRIMES: [u8; 16] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53];

/// A table of 16-bit numbers in `.rodata`: the squares of 100, 111, 122...
static SQUARES: [u16; 8] = [10000, 12321, 14884, 17689, 20736, 24025, 27556, 31329];

/// Starts at 1000, so it is stored in the file (`.data`).
static SCORE: AtomicU32 = AtomicU32::new(1000);

/// A running mix of everything `update` computed. Also `.data`.
static HASH: AtomicU32 = AtomicU32::new(0x1234_5678);

/// Starts at zero, so it is not in the file: the console must zero it
/// (`.bss`). Counts the calls to `update`.
static UPDATES: AtomicU32 = AtomicU32::new(0);

/// A zeroed block that `update` writes single bytes into.
static mut TRAIL: [u8; 256] = [0; 256];

/// The console's table, kept from `game_entry` for everyone else here.
static API: AtomicPtr<ConsoleApi> = AtomicPtr::new(ptr::null_mut());

/// The game's own table. It holds three addresses of code in the code
/// slot, and itself lives in the data slot (`.rodata`).
static CALLBACKS: GameCallbacks =
    GameCallbacks { size: size_of::<GameCallbacks>() as u32, init, update, exit };

// ---------------------------------------------------------------------
// Report numbers. `console/src/checks.rs` has the same list.
// ---------------------------------------------------------------------

const R_DATA_AT_START: u32 = 1;
const R_BSS_AT_START: u32 = 2;
const R_BSS_BLOCK_AT_START: u32 = 3;
const R_ECHO: u32 = 4;
const R_BYTE_TABLE: u32 = 5;
const R_WORD_TABLE: u32 = 6;
const R_QUOTIENT_LOW: u32 = 7;
const R_QUOTIENT_HIGH: u32 = 8;
const R_REMAINDER: u32 = 9;
const R_SIGNED_QUOTIENT: u32 = 10;
const R_FLOAT: u32 = 11;
const R_CLOCK: u32 = 12;
const R_COPY: u32 = 13;
const R_UPDATES_AT_END: u32 = 20;
const R_SCORE_AT_END: u32 = 21;
const R_BSS_BLOCK_AT_END: u32 = 22;
const R_HASH_AT_END: u32 = 23;

// ---------------------------------------------------------------------
// The border
// ---------------------------------------------------------------------

/// The first thing the console calls. Its address is in the file header.
///
/// # Safety
///
/// `api` must point at the console's table, which must stay where it is
/// for as long as the game is loaded.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn game_entry(api: *const ConsoleApi) -> *const GameCallbacks {
    if api.is_null() {
        return ptr::null();
    }
    // SAFETY: the caller promises `api` points at a table. `size` and
    // `version` are its first two fields in every version there will be.
    let (size, version) = unsafe { ((*api).size, (*api).version) };
    // An older or smaller table would make the calls below jump to
    // whatever happens to follow it in memory. Refuse instead.
    if version != API_VERSION || (size as usize) < size_of::<ConsoleApi>() {
        return ptr::null();
    }
    API.store(api.cast_mut(), Ordering::Relaxed);
    &CALLBACKS
}

/// The console's table.
fn api() -> &'static ConsoleApi {
    // SAFETY: `game_entry` stored a checked pointer before it returned the
    // table through which every other function here is reached.
    unsafe { &*API.load(Ordering::Relaxed) }
}

fn log(text: &str) {
    (api().log)(text.as_ptr(), text.len() as u32);
}

fn report(id: u32, value: u32) {
    (api().report)(id, value);
}

/// A Rust panic ends here. The message is never formatted: turning it into
/// text would pull several kilobytes of formatting code into the game.
/// The line number costs nothing, so the console gets that.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let line = info.location().map_or(0, |place| place.line());
    let api = API.load(Ordering::Relaxed);
    if !api.is_null() {
        // SAFETY: as in `api()`.
        unsafe { ((*api).abort)(abort_code::PANIC, line) }
    }
    // No console to tell. Nothing sensible is left to do.
    loop {}
}

// ---------------------------------------------------------------------
// The checks
// ---------------------------------------------------------------------

/// Adds up the zeroed block, a byte at a time.
fn trail_sum() -> u32 {
    let mut sum = 0u32;
    for index in 0..256 {
        // SAFETY: the console calls the game from one task only, so
        // nothing else touches the block meanwhile.
        sum += u32::from(unsafe { (&raw const TRAIL).cast::<u8>().add(index).read() });
    }
    sum
}

/// A `match` whose arms are plain numbers. The compiler turns it into a
/// table of numbers in `.rodata` and one read from it.
#[inline(never)]
fn bonus(kind: u32) -> u32 {
    match kind {
        0 => 10,
        1 => 250,
        2 => 7,
        3 => 4000,
        4 => 99,
        5 => 123_456,
        6 => 31,
        7 => 8,
        _ => 0,
    }
}

/// A `match` whose arms are different pieces of code. The compiler turns
/// it into a jump table: a list of code addresses, and a jump through it.
#[inline(never)]
fn dispatch(kind: u32, x: u32) -> u32 {
    match kind {
        0 => x.wrapping_mul(3),
        1 => (api().echo_sum)(x, 100),
        2 => x.rotate_left(5) ^ 0xA5A5,
        3 => u32::from(SQUARES[(x & 7) as usize]),
        4 => x / 7,
        5 => x.count_ones(),
        6 => u32::from(PRIMES[(x & 15) as usize]),
        7 => x.wrapping_sub(9),
        _ => 0,
    }
}

/// 64-bit arithmetic. The processor has no 64-bit multiply or divide, so
/// the compiler calls helper routines for them.
#[inline(never)]
fn big_math(seed: u32, divisor: u32) -> (u64, u64, i64) {
    let seed = u64::from(seed);
    let cube = seed * seed * seed;
    let divisor = u64::from(divisor);
    let signed = -(cube as i64) / (divisor as i64);
    (cube / divisor, cube % divisor, signed)
}

extern "C" fn init() -> u32 {
    log(HELLO);

    report(R_DATA_AT_START, SCORE.load(Ordering::Relaxed));
    report(R_BSS_AT_START, UPDATES.load(Ordering::Relaxed));
    report(R_BSS_BLOCK_AT_START, trail_sum());

    let echo = api().echo_sum;
    report(R_ECHO, echo(40, 2));

    // The loop bounds come from the console, so the compiler cannot work
    // the sums out on the PC and must really read the tables on the chip.
    let count = echo(8, 8) as usize;
    let bytes: u32 = PRIMES.iter().take(count).map(|&prime| u32::from(prime)).sum();
    report(R_BYTE_TABLE, bytes);
    let count = echo(4, 4) as usize;
    let words: u32 = SQUARES.iter().take(count).map(|&square| u32::from(square)).sum();
    report(R_WORD_TABLE, words);

    let (quotient, remainder, signed) = big_math(echo(1_000_000, 7), echo(1_000_000, 3));
    report(R_QUOTIENT_LOW, quotient as u32);
    report(R_QUOTIENT_HIGH, (quotient >> 32) as u32);
    report(R_REMAINDER, remainder as u32);
    report(R_SIGNED_QUOTIENT, signed as u32);

    // Floating point: 16 * 1.5 + 0.25 = 24.25.
    let float = echo(8, 8) as f32 * 1.5 + 0.25;
    report(R_FLOAT, float.to_bits());

    // A 64-bit value coming back from the console, twice. Time moves on.
    let clock = api().time_us;
    let first = clock();
    let second = clock();
    report(R_CLOCK, u32::from(first > 0 && second >= first));

    // Copying a block whose length is only known on the chip makes the
    // compiler call `memcpy`. The game has none of its own: the linker
    // script points that name at the routine in the chip's ROM.
    let mut copy = [0u8; 32];
    let length = (echo(12, 12) as usize).min(HELLO.len()).min(copy.len());
    copy[..length].copy_from_slice(&HELLO.as_bytes()[..length]);
    report(R_COPY, copy.iter().map(|&byte| u32::from(byte)).sum());

    0
}

extern "C" fn update(frame: u32) -> u32 {
    UPDATES.fetch_add(1, Ordering::Relaxed);

    let step = dispatch(frame % 10, frame);
    let gain = bonus(frame % 9);
    SCORE.fetch_add(gain, Ordering::Relaxed);

    // One byte written into the zeroed block, read from the byte table.
    let prime = PRIMES[(frame & 15) as usize];
    // SAFETY: as in `trail_sum`; the index is below 256.
    unsafe { (&raw mut TRAIL).cast::<u8>().add((frame & 255) as usize).write(prime) };

    #[cfg(feature = "crash")]
    if frame == 150 {
        // Reads entry 150 of a table of 16: a real bug of the usual kind.
        // `black_box` hides the number from the compiler, which would
        // otherwise refuse to build this.
        let index = core::hint::black_box(frame as usize);
        log("the game is about to read past the end of a table");
        report(99, u32::from(PRIMES[index]));
    }

    #[cfg(feature = "fault")]
    if frame == 150 {
        // Reads through a pointer to nowhere, as a bug in `unsafe` code
        // does. This is no Rust panic: the processor itself refuses and
        // stops the program, and the game gets no chance to say anything.
        log("the game is about to read through a pointer to nowhere");
        let nowhere = core::hint::black_box(4usize) as *const u32;
        // SAFETY: none. That is the point.
        report(98, unsafe { nowhere.read_volatile() });
    }

    let hash = (HASH.load(Ordering::Relaxed).rotate_left(5) ^ step).wrapping_add(gain);
    HASH.store(hash, Ordering::Relaxed);
    hash
}

extern "C" fn exit() {
    report(R_UPDATES_AT_END, UPDATES.load(Ordering::Relaxed));
    report(R_SCORE_AT_END, SCORE.load(Ordering::Relaxed));
    report(R_BSS_BLOCK_AT_END, trail_sum());
    report(R_HASH_AT_END, HASH.load(Ordering::Relaxed));
    log("the game was asked to stop, and did");
}
