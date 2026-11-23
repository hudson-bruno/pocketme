//! Memory: the heap's size, the game slots, and how deep the main stack
//! was ever used.
//!
//! The stack is measured the old way: early on, the part of the stack
//! not yet in use is filled with a pattern; later the pattern is looked
//! at to see how far down something wrote over it. (Copied from
//! experiments/display-dma/src/stack.rs.) The number can be too small:
//! a function that sets aside room it never writes leaves the pattern.

use log::info;

/// The heap: all of the second block of data RAM (0x3FFE_7E30, RAM the
/// boot loader used, given back; esp-hal's memory.x) up to the game's
/// data slot: 49,616 bytes (game_api::layout). Nothing in this firmware
/// allocates; the radio does. game-slots.x checks that it ends before the
/// slot.
pub const HEAP_SIZE: usize = game_api::layout::CONSOLE_HEAP_SIZE as usize;

/// What the unused part of the stack is filled with.
const PATTERN: u32 = 0x5AC3_A53C;
/// esp-rtos keeps a guard word near the low end of the stack. The
/// pattern stops well above it.
const FLOOR: usize = 4096;
/// How far below the painter itself the pattern starts.
const CLEAR: usize = 1024;

unsafe extern "C" {
    // The linker script's names for the two ends of the main stack. It
    // grows downwards, from "start" to "end".
    static _stack_start_cpu0: u32;
    static _stack_end_cpu0: u32;
    // The game slots (game-slots.x).
    static _game_code_slot_start: u32;
    static _game_data_slot_start: u32;
    // End of the console's own RAM code (hot-block.x: .rwtext,
    // .rwtext.wifi and the hot block's RAM part), and of its data and bss.
    static _cyd_iram_end: u32;
    static _bss_end: u32;
}

fn ends() -> (usize, usize) {
    let low = (&raw const _stack_end_cpu0) as usize + FLOOR;
    let top = (&raw const _stack_start_cpu0) as usize;
    (low, top)
}

/// Fills the unused part of the main stack with the pattern. Called once,
/// as early as possible.
#[inline(never)]
pub fn paint_stack() {
    let here = 0_u32;
    let here = core::hint::black_box(&raw const here) as usize;
    let (low, _) = ends();
    let Some(high) = here.checked_sub(CLEAR).map(|high| high & !3) else {
        return;
    };
    critical_section::with(|_| {
        let mut at = low;
        while at < high {
            // SAFETY: `at` is inside the main stack, a whole word, above
            // the guard word and more than a kilobyte below everything in
            // use. With interrupts off nothing else writes there meanwhile.
            unsafe { (at as *mut u32).write_volatile(PATTERN) };
            at += 4;
        }
    });
}

/// How much of the main stack was ever in use, in bytes, and how large it
/// is. `None` for the first when the pattern is gone down to its lowest
/// word (then the use is that much or more).
pub fn stack_deepest() -> (Option<usize>, usize) {
    let (low, top) = ends();
    let size = top - (low - FLOOR);
    let mut at = low;
    while at < top {
        // SAFETY: a whole word inside the main stack, only read.
        if unsafe { (at as *const u32).read_volatile() } != PATTERN {
            break;
        }
        at += 4;
    }
    ((at > low).then_some(top - at), size)
}

/// One `mem` line in the log.
pub fn report(when: &str) {
    let heap = esp_alloc::HEAP.stats();
    let (used, size) = stack_deepest();
    let stack_top = (&raw const _stack_start_cpu0) as usize;
    let stack_end = (&raw const _stack_end_cpu0) as usize;
    let bss_end = (&raw const _bss_end) as usize;
    let rwtext_end = (&raw const _cyd_iram_end) as usize;
    let code_slot = (&raw const _game_code_slot_start) as usize;
    let data_slot = (&raw const _game_data_slot_start) as usize;
    info!(
        "mem when={when} heap_size={} heap_used={} heap_free={} stack_deepest={} stack_size={size} stack={stack_end:#x}..{stack_top:#x} bss_end={bss_end:#x} rwtext_end={rwtext_end:#x} code_slot={code_slot:#x} iram_left={} data_slot={data_slot:#x}",
        heap.size,
        heap.current_usage,
        heap.size - heap.current_usage,
        used.map_or(-1, |u| u as i64),
        code_slot.saturating_sub(rwtext_end),
    );
}
