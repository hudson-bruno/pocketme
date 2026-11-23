//! Lab (feature `cpu1-start-test`): what happens to the second core's
//! cache RAM (0x4007_8000, 32 KB) when the second core is started?
//!
//! The first worker put code there (feature `cpu1-cache-ram`), safe only
//! while the second core is off. This test shows it on the board,
//! without crashing: it fills the whole 32 KB with a known pattern
//! (each word = its own address mixed with a key), then counts the words
//! that changed after each step. Nothing of ours runs from there during
//! the test (the radio is not in this build; the small test function
//! there is never called again).
//!
//! Letter `S` (the careful way, RAM only), steps:
//! 1. pattern written, words checked (expect 0 changed).
//! 2. the second core started by hand with its cache OFF, running a tiny
//!    loop that lives in normal code RAM and only counts. (esp-hal's own
//!    start switches the cache on first and runs start-up code from
//!    flash, so it cannot be used for this step.)
//! 3. its cache switched ON (as esp-hal does), loop still RAM only.
//! 4. the loop reads 128 KB of constant data from flash (through its
//!    cache).
//! 5. pattern again; the loop runs one function that lives in flash.
//! 6. the second core stopped again (held in reset, clock off, cache
//!    off), pattern again, checked after a pause.
//!
//! Letter `E` (the normal way): the pattern, then esp-rtos's
//! `start_second_core` as a real program would call it, then the
//! changed words right after, after 100 ms and after 1 s.
//!
//! Only one of the two per start of the board (restart in between).

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering::Acquire, Ordering::Relaxed, Ordering::Release};

use esp_hal::peripherals::{CPU_CTRL, FROM_CPU_INTR1};
use esp_hal::system::Stack;
use log::{info, warn};
use static_cell::ConstStaticCell;

use crate::beam::spin_us;
use crate::cpu1_cache::{AREA, AREA_LEN, regs};
use crate::lcd::{CPU_MHZ, cycles};

const DPORT: usize = 0x3FF0_0000;
const APPCPU_CTRL_A: usize = DPORT + 0x2C; // bit 0: held in reset
const APPCPU_CTRL_B: usize = DPORT + 0x30; // bit 0: clock on
const APPCPU_CTRL_C: usize = DPORT + 0x34; // bit 0: stalled
const APPCPU_CTRL_D: usize = DPORT + 0x38; // where it starts
const APP_CACHE_CTRL: usize = DPORT + 0x58; // bit 3 on, bit 4 flush, bit 5 flush done

/// The pattern: each word is its own address mixed with this key.
const KEY: u32 = 0xC0DE_5EED;

fn rd(addr: usize) -> u32 {
    // SAFETY: a DPORT register or a word in the area, read only.
    unsafe { (addr as *const u32).read_volatile() }
}
fn wr(addr: usize, v: u32) {
    // SAFETY: a DPORT register or a word in the area that nothing runs
    // from during this test.
    unsafe { (addr as *mut u32).write_volatile(v) }
}

fn fill() {
    let mut at = AREA;
    while at < AREA + AREA_LEN {
        wr(at, at as u32 ^ KEY);
        at += 4;
    }
}

/// Changed words in the whole area, and per 4 KB block.
fn changed() -> (u32, [u16; 8]) {
    let mut n = 0;
    let mut blocks = [0u16; 8];
    let mut at = AREA;
    while at < AREA + AREA_LEN {
        if rd(at) != at as u32 ^ KEY {
            n += 1;
            blocks[(at - AREA) / 4096] += 1;
        }
        at += 4;
    }
    (n, blocks)
}

fn log_changed(step: &str) -> u32 {
    let (n, b) = changed();
    info!(
        "cpu1 start test: {step}: changed_words={n} of {} per_4k_block={b:?} | {}",
        AREA_LEN / 4,
        regs()
    );
    n
}

// ---- the second core's RAM-only loop -------------------------------------

struct RamStack(UnsafeCell<[u32; 1024]>);
// SAFETY: only the second core uses it, as its stack.
unsafe impl Sync for RamStack {}
static RAM_STACK: RamStack = RamStack(UnsafeCell::new([0; 1024]));

static C2_TOP: AtomicU32 = AtomicU32::new(0);
static C2_COUNT: AtomicU32 = AtomicU32::new(0);
static C2_CMD: AtomicU32 = AtomicU32::new(0);
static C2_ARG: AtomicU32 = AtomicU32::new(0);
static C2_RESULT: AtomicU32 = AtomicU32::new(0);

/// Where the chip's start code (in ROM) sends the second core. In
/// normal code RAM. Like esp-hal's own start: interrupts off, its own
/// stack, then a function that never returns.
#[esp_hal::ram]
fn ram_entry() -> ! {
    // SAFETY: the second core's first instructions; nothing else runs on
    // it. The stack top is the end of RAM_STACK, set before the start.
    unsafe {
        xtensa_lx::interrupt::set_mask(0);
        xtensa_lx::set_stack_pointer(C2_TOP.load(Relaxed) as *mut u32);
    }
    ram_loop()
}

/// The second core's loop: counts, and does a command when asked.
/// No `match` (it could put a jump table into flash).
#[esp_hal::ram]
fn ram_loop() -> ! {
    let mut n: u32 = 0;
    loop {
        n = n.wrapping_add(1);
        C2_COUNT.store(n, Relaxed);
        let cmd = C2_CMD.load(Acquire);
        if cmd == 1 {
            // Read 128 KB of constant data from flash.
            let base = C2_ARG.load(Relaxed) as *const u32;
            let mut h = 0u32;
            let mut i = 0usize;
            while i < 32 * 1024 {
                // SAFETY: inside the 128 KB block whose address was given.
                h = h.rotate_left(1) ^ unsafe { base.add(i).read_volatile() };
                i += 1;
            }
            C2_RESULT.store(h, Relaxed);
            C2_CMD.store(0, Release);
        } else if cmd == 2 {
            // Run one function that lives in flash.
            // SAFETY: the address of `flash_work`, given by the first core.
            let f: fn(u32) -> u32 = unsafe { core::mem::transmute(C2_ARG.load(Relaxed) as usize) };
            C2_RESULT.store(f(1000), Relaxed);
            C2_CMD.store(0, Release);
        }
    }
}

/// A function in flash (not in RAM): some work with no memory use.
#[inline(never)]
fn flash_work(n: u32) -> u32 {
    let mut x = 0x2545_F491u32;
    for _ in 0..n {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
    }
    x
}

fn block_hash(base: *const u32) -> u32 {
    let mut h = 0u32;
    for i in 0..32 * 1024 {
        // SAFETY: inside the 128 KB block.
        h = h.rotate_left(1) ^ unsafe { base.add(i).read_volatile() };
    }
    h
}

/// Waits for the second core to finish a command (at most 1 s).
fn wait_cmd() -> bool {
    let t = cycles();
    while C2_CMD.load(Acquire) != 0 {
        if cycles().wrapping_sub(t) > 1_000_000 * CPU_MHZ {
            return false;
        }
    }
    true
}

static STACK: ConstStaticCell<Stack<8192>> = ConstStaticCell::new(Stack::new());

pub struct StartTest {
    periph: Option<(CPU_CTRL<'static>, FROM_CPU_INTR1<'static>)>,
    done: bool,
}

impl StartTest {
    pub const fn new() -> Self {
        Self {
            periph: None,
            done: false,
        }
    }

    pub fn give(&mut self, cpu: CPU_CTRL<'static>, int1: FROM_CPU_INTR1<'static>) {
        self.periph = Some((cpu, int1));
    }

    /// Letter `S`.
    pub fn ram_only(&mut self) {
        if self.done || self.periph.is_none() {
            warn!("cpu1 start test: only once per start of the board");
            return;
        }
        self.done = true;
        info!("cpu1 start test S (RAM only): begin");
        fill();
        log_changed("1 pattern written, second core off");

        // 2. Start it by hand, cache off.
        // SAFETY: RAM_STACK is used by nobody else.
        let top = unsafe { (*RAM_STACK.0.get()).as_ptr().add(1024) } as u32 & !15;
        C2_TOP.store(top, Release);
        let entry = ram_entry as fn() -> ! as usize as u32;
        wr(APPCPU_CTRL_D, entry);
        wr(APPCPU_CTRL_B, rd(APPCPU_CTRL_B) | 1);
        wr(APPCPU_CTRL_C, rd(APPCPU_CTRL_C) & !1);
        wr(APPCPU_CTRL_A, rd(APPCPU_CTRL_A) | 1);
        wr(APPCPU_CTRL_A, rd(APPCPU_CTRL_A) & !1);
        spin_us(200_000);
        let c1 = C2_COUNT.load(Relaxed);
        spin_us(10_000);
        let c2 = C2_COUNT.load(Relaxed);
        info!(
            "cpu1 start test: second core started at {entry:#x} (RAM), stack top {top:#x}; its loop counted {c1} then {c2} (running={})",
            c2 != c1
        );
        log_changed("2 second core runs a RAM-only loop, its cache OFF");

        // 3. Its cache on, the way esp-hal does it (flush, then enable).
        wr(APP_CACHE_CTRL, rd(APP_CACHE_CTRL) & !(1 << 4));
        wr(APP_CACHE_CTRL, rd(APP_CACHE_CTRL) | (1 << 4));
        while rd(APP_CACHE_CTRL) & (1 << 5) == 0 {}
        wr(APP_CACHE_CTRL, rd(APP_CACHE_CTRL) & !(1 << 4));
        wr(APP_CACHE_CTRL, rd(APP_CACHE_CTRL) | (1 << 3));
        spin_us(200_000);
        log_changed("3 its cache ON, loop still RAM only");

        // 4. The loop reads 128 KB of constant data from flash.
        let block = crate::cpu1_cache::flash_block();
        C2_ARG.store(block as u32, Relaxed);
        C2_CMD.store(1, Release);
        let ok = wait_cmd();
        let want = block_hash(block);
        info!(
            "cpu1 start test: second core read 128 KB of flash data at {:#x}: finished={ok} result_ok={}",
            block as usize,
            C2_RESULT.load(Relaxed) == want
        );
        log_changed("4 after the second core read flash data");

        // 5. Pattern again; the loop runs one function from flash.
        fill();
        log_changed("5a pattern written again");
        C2_ARG.store(flash_work as fn(u32) -> u32 as usize as u32, Relaxed);
        C2_CMD.store(2, Release);
        let ok = wait_cmd();
        info!(
            "cpu1 start test: second core ran flash_work at {:#x}: finished={ok} result_ok={}",
            flash_work as fn(u32) -> u32 as usize,
            C2_RESULT.load(Relaxed) == flash_work(core::hint::black_box(1000))
        );
        log_changed("5b after the second core ran one function from flash");

        // 6. Stop it again: held in reset, clock off, cache off.
        wr(APPCPU_CTRL_A, rd(APPCPU_CTRL_A) | 1);
        wr(APPCPU_CTRL_B, rd(APPCPU_CTRL_B) & !1);
        wr(APP_CACHE_CTRL, rd(APP_CACHE_CTRL) & !(1 << 3));
        fill();
        spin_us(500_000);
        log_changed("6 second core stopped, cache off, pattern again, 0.5 s later");
        crate::cpu1_cache::rebase();
        info!("cpu1 start test S: end");
    }

    /// Letter `E`.
    pub fn standard(&mut self) {
        if self.done {
            warn!("cpu1 start test: only once per start of the board");
            return;
        }
        let Some((cpu, int1)) = self.periph.take() else {
            return;
        };
        self.done = true;
        info!("cpu1 start test E (esp-rtos start_second_core): begin");
        fill();
        log_changed("1 pattern written, second core off");
        let t = cycles();
        esp_rtos::start_second_core(cpu, int1, STACK.take(), || {});
        let us = cycles().wrapping_sub(t) / CPU_MHZ;
        info!("cpu1 start test: start_second_core returned after {us} us");
        log_changed("2 right after start_second_core");
        spin_us(100_000);
        log_changed("3 100 ms later (second core idle in the scheduler)");
        spin_us(1_000_000);
        log_changed("4 1.1 s later");
        crate::cpu1_cache::rebase();
        info!("cpu1 start test E: end");
    }
}
