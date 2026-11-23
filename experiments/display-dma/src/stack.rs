//! How deep the main stack was ever used ("stack high-water mark").
//!
//! Neither esp-hal nor esp-rtos says how much of a stack was used, so
//! this measures it the old way: early on, the part of the stack not yet
//! in use is filled with a pattern, and later the pattern is looked at to
//! see how far down something wrote over it. (The method is the one of
//! an earlier project of the author's on this board.)
//!
//! Everything the executor runs is polled on this one stack, and the
//! interrupts that arrive meanwhile use it too, so the number is the
//! whole firmware's.
//!
//! The number can be too small: a function that sets aside room it does
//! not write to leaves the pattern in place.

/// What the unused part is filled with: a word a program is unlikely to
/// write there by itself.
const PATTERN: u32 = 0x5AC3_A53C;

/// esp-rtos keeps a guard word near the low end of the stack. The
/// pattern stops well above it; a stack used down into this last part is
/// reported as "deeper than was measured".
const FLOOR: usize = 4096;

/// How far below the painter itself the pattern starts. Its caller's
/// saved registers may be put just under its own frame.
const CLEAR: usize = 1024;

unsafe extern "C" {
    // The linker script's names for the two ends of the main stack. It
    // grows downwards, from "start" to "end".
    static _stack_start_cpu0: u32;
    static _stack_end_cpu0: u32;
}

/// The lowest address the pattern goes to, and the top of the stack.
fn ends() -> (usize, usize) {
    let low = (&raw const _stack_end_cpu0) as usize + FLOOR;
    let top = (&raw const _stack_start_cpu0) as usize;
    (low, top)
}

/// Fills the unused part of the main stack with the pattern. To be
/// called once, from the main task, as early as possible.
#[inline(never)]
pub fn paint() {
    // Something of this function's own, to learn where the stack is now.
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
            // the guard word and more than a kilobyte below everything
            // this task has in use. With interrupts off, and no call made
            // from this loop, nothing else puts anything there meanwhile.
            unsafe { (at as *mut u32).write_volatile(PATTERN) };
            at += 4;
        }
    });
}

/// How much of the main stack was ever in use, in bytes, and how large it
/// is. `None` for the first when the pattern is gone down to its lowest
/// word: then the use is that much or more, and unknown.
pub fn deepest() -> (Option<usize>, usize) {
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
