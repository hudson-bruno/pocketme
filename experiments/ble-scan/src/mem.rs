//! Memory figures: the heap and the main stack.
//!
//! The heap is the one the radio driver takes its memory from. The main
//! stack is where everything this firmware's own code does is run: the
//! Bluetooth library, the handling of each advertisement and the logging.
//! The radio's own threads have stacks of their own, which esp-rtos takes
//! from the heap, so they show up in the heap figures and not in the stack
//! figure.

use core::{
    alloc::Layout,
    sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
};

use enumset::EnumSet;
use esp_alloc::{EspHeap, HEAP, MemoryCapability};

// ------------------------------------------------------------------ heap

/// The least free heap there ever was, in bytes.
static MIN_FREE: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Requests the heap could not serve. Any number above zero is bad news:
/// the radio driver does not expect to be refused.
static REFUSED: AtomicU32 = AtomicU32::new(0);

/// Requests served since start. Shows whether the radio keeps asking for
/// memory while it runs or only at start-up.
static REQUESTS: AtomicU32 = AtomicU32::new(0);

/// Set while [`largest_block`] is trying sizes. Its requests are not the
/// firmware's and must not count.
static PROBING: AtomicBool = AtomicBool::new(false);

/// Called by the allocator after every request (esp-alloc's "alloc-hooks"
/// feature). Looking only once a second would miss memory that is taken
/// and given back in between, so the low mark is taken here.
#[unsafe(no_mangle)]
fn _esp_alloc_alloc(heap: &EspHeap, _caps: EnumSet<MemoryCapability>, ptr: usize, _size: usize) {
    if PROBING.load(Ordering::Relaxed) {
        return;
    }
    if ptr == 0 {
        REFUSED.fetch_add(1, Ordering::Relaxed);
    }
    REQUESTS.fetch_add(1, Ordering::Relaxed);
    MIN_FREE.fetch_min(heap.free(), Ordering::Relaxed);
}

/// The allocator wants this one as well. Giving memory back cannot lower
/// the free amount, so there is nothing to note.
#[unsafe(no_mangle)]
fn _esp_alloc_dealloc(_heap: &EspHeap, _ptr: usize, _size: usize) {}

/// One look at the heap.
pub struct Heap {
    pub size: usize,
    pub free: usize,
    pub min_free: usize,
    pub largest: usize,
    pub requests: u32,
    pub refused: u32,
}

pub fn heap() -> Heap {
    let free = HEAP.free();
    Heap {
        size: free + HEAP.used(),
        free,
        min_free: MIN_FREE.load(Ordering::Relaxed).min(free),
        largest: largest_block(),
        requests: REQUESTS.load(Ordering::Relaxed),
        refused: REFUSED.load(Ordering::Relaxed),
    }
}

/// The largest single piece the heap could hand out right now, to within
/// 16 bytes.
///
/// The allocator has no call for this, so it is found by asking: try a
/// size, give it straight back, and halve the range. Interrupts are off
/// meanwhile, so the radio's threads cannot ask for memory while most of
/// it is briefly taken. About 13 tries; each is a short walk over the
/// free list.
fn largest_block() -> usize {
    critical_section::with(|_| {
        PROBING.store(true, Ordering::Relaxed);
        let mut fits = 0;
        let mut too_big = HEAP.free() + 1;
        while too_big - fits > 16 {
            let size = fits + (too_big - fits) / 2;
            // A layout of this size and alignment is always valid here:
            // the size is far below isize::MAX and 4 is a power of two.
            let Ok(layout) = Layout::from_size_align(size, 4) else { break };
            // SAFETY: the layout has a size above zero, and the block is
            // given back with the same layout before anything else runs.
            let block = unsafe { alloc::alloc::alloc(layout) };
            if block.is_null() {
                too_big = size;
            } else {
                unsafe { alloc::alloc::dealloc(block, layout) };
                fits = size;
            }
        }
        PROBING.store(false, Ordering::Relaxed);
        fits
    })
}

// ----------------------------------------------------------------- stack
//
// Neither esp-hal nor esp-rtos says how much of a stack was used. So, as
// in an earlier experiment of the author's: early on, the part of the
// stack not yet in use is filled with a pattern, and later the pattern is
// looked at to see how far down something wrote over it.
//
// The figure can be too small: a function that sets room aside without
// writing to it leaves the pattern in place.

/// What the unused part is filled with: a word a program is unlikely to
/// write there by itself.
const PATTERN: u32 = 0x5AC3_A53C;

/// esp-rtos keeps a guard word near the low end of the stack. The pattern
/// stops this far above the end, well clear of it. A stack used down into
/// these last bytes is reported as "more than was measured".
const FLOOR: usize = 4096;

/// How far below the painter itself the pattern starts, to stay clear of
/// what the caller has in use.
const CLEAR: usize = 1024;

unsafe extern "C" {
    // The linker script's names for the two ends of the main stack. It
    // grows downwards, from "start" to "end".
    static _stack_start_cpu0: u32;
    static _stack_end_cpu0: u32;
}

fn ends() -> (usize, usize) {
    let low = (&raw const _stack_end_cpu0) as usize + FLOOR;
    let top = (&raw const _stack_start_cpu0) as usize;
    (low, top)
}

/// Fills the unused part of the main stack with the pattern. Call once,
/// from the main task, as early as possible.
#[inline(never)]
pub fn paint_stack() {
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
            // the guard word and a kilobyte below everything this task
            // has in use. With interrupts off, and no call made from this
            // loop, nothing else puts anything there meanwhile.
            unsafe { (at as *mut u32).write_volatile(PATTERN) };
            at += 4;
        }
    });
}

/// How much of the main stack was ever in use, and how large it is, in
/// bytes. `None` for the first when the pattern is gone down to its lowest
/// word: then the use is that much or more.
pub fn stack() -> (Option<usize>, usize) {
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
