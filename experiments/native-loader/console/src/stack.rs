//! How deep the main stack was ever used.
//!
//! The game runs on the console's main stack, so its appetite shows up
//! here. Neither esp-hal nor esp-rtos says how much of a stack was used,
//! so this measures it the old way: early on, the part of the stack not
//! yet in use is filled with a pattern, and later the pattern is looked at
//! to see how far down something wrote over it.
//!
//! The number is everything that ran on the main stack: start-up, the
//! Bluetooth host code (it shares this stack), the loader, the game, and
//! the interrupts that arrived meanwhile. The radio's own threads have
//! stacks of their own, taken from the heap, and are not seen here.
//!
//! The number can be too small: a function that sets aside room it does
//! not write to leaves the pattern in place.
//!
//! (Same method as an earlier project of the author's on this board.)

/// What the unused part is filled with: a word a program is unlikely to
/// write there by itself.
const PATTERN: u32 = 0x5AC3_A53C;

/// The pattern stops this far above the stack's low end. esp-hal keeps a
/// guard word near that end and watches it; a stack used down into these
/// last bytes is reported as "deeper than was measured".
const FLOOR: usize = 1024;

/// How far below the painter itself the pattern starts.
const CLEAR: usize = 1024;

unsafe extern "C" {
    // The linker script's names for the two ends of the main stack. It
    // grows downwards, from "start" to "end".
    static _stack_start_cpu0: u32;
    static _stack_end_cpu0: u32;
}

/// The lowest address of the stack, and its top.
pub fn ends() -> (usize, usize) {
    ((&raw const _stack_end_cpu0) as usize, (&raw const _stack_start_cpu0) as usize)
}

/// Fills the unused part of the main stack with the pattern. To be called
/// once, from the main task, as early as possible.
#[inline(never)]
pub fn paint() {
    // Something of this function's own, to learn where the stack is now.
    let here = 0_u32;
    let here = core::hint::black_box(&raw const here) as usize;
    let low = ends().0 + FLOOR;
    let Some(high) = here.checked_sub(CLEAR).map(|high| high & !3) else {
        return;
    };

    critical_section::with(|_| {
        let mut at = low;
        while at < high {
            // SAFETY: `at` is inside the main stack, a whole word, above
            // the guard word and a kilobyte below everything this task has
            // in use. With interrupts off, and no call made from this
            // loop, nothing else puts anything there meanwhile.
            unsafe { (at as *mut u32).write_volatile(PATTERN) };
            at += 4;
        }
    });
}

/// How much of the main stack was ever in use, in bytes. `None` when the
/// pattern is gone down to its lowest word: then the use is the whole
/// stack but its last kilobyte, or more.
pub fn deepest() -> Option<usize> {
    let (end, top) = ends();
    let low = end + FLOOR;
    let mut at = low;
    while at < top {
        // SAFETY: a whole word inside the main stack, only read.
        if unsafe { (at as *const u32).read_volatile() } != PATTERN {
            break;
        }
        at += 4;
    }
    (at > low).then_some(top - at)
}
