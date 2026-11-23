//! Lab (feature `cpu1-cache-ram`): the second core's cache RAM as extra
//! code RAM.
//!
//! The ESP32 keeps 32 KB of RAM at 0x4007_8000 for the second core's
//! flash cache. This firmware never starts the second core, so that RAM
//! is free. cpu1-cache.x links a section to run there; the bootloader
//! cannot put it there (it runs from that RAM itself while it loads the
//! firmware), so it stores the bytes at the start of the game code slot
//! (free at start-up) and [`start`] copies them over before anything
//! uses them, then clears the slot.
//!
//! What this file does:
//! - [`start`]: (first thing in `main`) reads the chip's cache settings,
//!   tests the whole 32 KB with five patterns, copies the code there,
//!   checks every word of the copy, fills the rest with zeros (a zero
//!   word is not a valid instruction: a jump there stops the CPU with an
//!   error instead of running garbage) and remembers a checksum of all
//!   32 KB.
//! - [`probe`]: runs one small test function from there, from normal
//!   RAM and from flash, and compares the results and the time.
//! - [`tick`]: called by the loop. It reads 2 KB more of a 128 KB block
//!   of constant data in flash (through the first core's cache) each
//!   time; after a whole pass it compares the block's checksum with the
//!   one worked out when the firmware was built, and the 32 KB's
//!   checksum with the one from `start`. A cache that used "our" RAM, or
//!   a broken cache, would show up as a wrong sum (or a crash).
//! - [`byte_read`] / [`byte_write`]: the letters `B` and `W`: one
//!   single-byte access to the area, to see if it has the same "whole
//!   32-bit words only" rule as the rest of the code RAM. Expected to
//!   stop the board with an error (it restarts with `boardctl` or the
//!   reset button).

use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

use log::{info, warn};

/// The second core's cache RAM.
pub const AREA: usize = 0x4007_8000;
pub const AREA_LEN: usize = 0x8000;

/// The chip's DPORT registers (Technical Reference Manual, and the
/// offsets in the esp32 register crate).
const DPORT: usize = 0x3FF0_0000;
const APPCPU_CTRL_A: usize = DPORT + 0x2C; // bit 0: second core held in reset
const APPCPU_CTRL_B: usize = DPORT + 0x30; // bit 0: second core's clock on
const APPCPU_CTRL_C: usize = DPORT + 0x34; // bit 0: second core stalled
const PRO_CACHE_CTRL: usize = DPORT + 0x40; // bit 3: first core's cache on
const APP_CACHE_CTRL: usize = DPORT + 0x58; // bit 3: second core's cache on
const CACHE_MUX_MODE: usize = DPORT + 0x7C; // which cache uses which half

unsafe extern "C" {
    // cpu1-cache.x: where the section runs, and where its bytes are
    // stored until `start` copies them.
    static _cpu1_cache_text_start: u32;
    static _cpu1_cache_text_end: u32;
    static _cpu1_cache_text_load: u32;
    // End of this firmware's own functions there (the PHY's code follows).
    static _cpu1_cache_own_end: u32;
}

fn reg(addr: usize) -> u32 {
    // SAFETY: a DPORT register, only read; reading has no side effect.
    unsafe { (addr as *const u32).read_volatile() }
}

/// The six cache and second-core registers, as one log text.
pub struct Regs {
    pub pro_cache: u32,
    pub app_cache: u32,
    pub mux: u32,
    pub appcpu_a: u32,
    pub appcpu_b: u32,
    pub appcpu_c: u32,
}

pub fn regs() -> Regs {
    Regs {
        pro_cache: reg(PRO_CACHE_CTRL),
        app_cache: reg(APP_CACHE_CTRL),
        mux: reg(CACHE_MUX_MODE),
        appcpu_a: reg(APPCPU_CTRL_A),
        appcpu_b: reg(APPCPU_CTRL_B),
        appcpu_c: reg(APPCPU_CTRL_C),
    }
}

impl core::fmt::Display for Regs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "pro_cache_ctrl={:#x} (on={}) app_cache_ctrl={:#x} (on={}) cache_mux_mode={} appcpu reset={} clock={} stall={}",
            self.pro_cache,
            (self.pro_cache >> 3) & 1,
            self.app_cache,
            (self.app_cache >> 3) & 1,
            self.mux & 3,
            self.appcpu_a & 1,
            self.appcpu_b & 1,
            self.appcpu_c & 1,
        )
    }
}

/// A checksum over 32-bit words (FNV-1a on words). Order matters, so a
/// word moved to another place changes it too.
const fn mix(h: u32, w: u32) -> u32 {
    (h ^ w).wrapping_mul(16_777_619)
}
const SUM_START: u32 = 2_166_136_261;

fn area_sum() -> u32 {
    let mut h = SUM_START;
    let mut at = AREA;
    while at < AREA + AREA_LEN {
        // SAFETY: a whole word inside the area, only read.
        h = mix(h, unsafe { (at as *const u32).read_volatile() });
        at += 4;
    }
    h
}

/// What `start` saw; also kept for the reports.
static AREA_SUM: AtomicU32 = AtomicU32::new(0);
static AREA_CHECKS: AtomicU32 = AtomicU32::new(0);
static AREA_BAD: AtomicU32 = AtomicU32::new(0);
static CODE_BYTES: AtomicU32 = AtomicU32::new(0);

/// First thing in `main`: tests the area, copies the code, checks it.
/// Panics (before anything can jump there) if the copy is wrong.
pub fn start() {
    let before = regs();
    // The clock is not set up yet this early: time in CPU cycles.
    let t0 = xtensa_lx::timer::get_cycle_count();

    // 1. Patterns over all 32 KB: write every word, then read every word.
    // The fifth "pattern" is each word's own address, which also finds
    // two addresses that land on the same memory.
    let patterns: [Option<u32>; 5] = [
        Some(0x0000_0000),
        Some(0xFFFF_FFFF),
        Some(0x5555_5555),
        Some(0xAAAA_AAAA),
        None,
    ];
    let mut bad = 0u32;
    let mut first_bad = 0usize;
    for p in patterns {
        let mut at = AREA;
        while at < AREA + AREA_LEN {
            let v = p.unwrap_or(at as u32);
            // SAFETY: a whole word in the area; nothing runs from there
            // yet and nothing else uses it while the second core is off.
            unsafe { (at as *mut u32).write_volatile(v) };
            at += 4;
        }
        let mut at = AREA;
        while at < AREA + AREA_LEN {
            let v = p.unwrap_or(at as u32);
            // SAFETY: as above, only read.
            if unsafe { (at as *const u32).read_volatile() } != v {
                if bad == 0 {
                    first_bad = at;
                }
                bad += 1;
            }
            at += 4;
        }
    }
    let t1 = xtensa_lx::timer::get_cycle_count();

    // 2. The copy, word by word (this RAM takes whole words only).
    let start = (&raw const _cpu1_cache_text_start) as usize;
    let end = (&raw const _cpu1_cache_text_end) as usize;
    let load = (&raw const _cpu1_cache_text_load) as usize;
    let len = end - start;
    let mut i = 0;
    while i < len {
        // SAFETY: `load + i` is inside the stored copy (in the game code
        // slot, loaded there by the bootloader; no game yet); `start + i`
        // is inside the area. Whole words only: both are code RAM.
        unsafe {
            let w = ((load + i) as *const u32).read_volatile();
            ((start + i) as *mut u32).write_volatile(w);
        }
        i += 4;
    }
    // The rest of the area: zeros (not a valid instruction).
    let mut at = end;
    while at < AREA + AREA_LEN {
        // SAFETY: a whole word in the area, after the code.
        unsafe { (at as *mut u32).write_volatile(0) };
        at += 4;
    }
    // 3. Every word of the copy against the stored bytes.
    let mut copy_bad = 0u32;
    let mut i = 0;
    while i < len {
        // SAFETY: both reads as in the copy above.
        let (a, b) = unsafe {
            (
                ((load + i) as *const u32).read_volatile(),
                ((start + i) as *const u32).read_volatile(),
            )
        };
        if a != b {
            copy_bad += 1;
        }
        i += 4;
    }
    // 4. The game code slot back to zeros, as a game would find it.
    let mut i = 0;
    while i < len {
        // SAFETY: inside the stored copy, which is no longer needed.
        unsafe { ((load + i) as *mut u32).write_volatile(0) };
        i += 4;
    }
    // The test meant to fail (feature cpu1-wipe-test): wipe everything
    // after this firmware's own functions, that is the radio's PHY code.
    #[cfg(feature = "cpu1-wipe-test")]
    {
        let own_end = (&raw const _cpu1_cache_own_end) as usize;
        let mut at = own_end;
        while at < end {
            // SAFETY: a whole word in the area; nothing runs there yet.
            unsafe { (at as *mut u32).write_volatile(0) };
            at += 4;
        }
        warn!(
            "cpu1 WIPE TEST: {:#x}..{end:#x} set to zeros; starting the radio must fail",
            own_end
        );
    }
    let sum = area_sum();
    let t2 = xtensa_lx::timer::get_cycle_count();
    AREA_SUM.store(sum, Relaxed);
    CODE_BYTES.store(len as u32, Relaxed);

    info!("cpu1 regs at start: {before}");
    info!(
        "cpu1 pattern test: {AREA_LEN} bytes at {AREA:#x}, 5 patterns, bad_words={bad} first_bad={first_bad:#x} in {} cpu cycles",
        t1.wrapping_sub(t0)
    );
    info!(
        "cpu1 copy: {len} bytes of code from {load:#x} to {start:#x}..{end:#x}, bad_words={copy_bad}, {} bytes of the area left, area sum={sum:#010x}, in {} cpu cycles",
        AREA + AREA_LEN - end,
        t2.wrapping_sub(t1)
    );
    if bad != 0 || copy_bad != 0 {
        panic!("cpu1: the second core's cache RAM failed the test; not running code from it");
    }
}

// ---- the test function, three times ------------------------------------

/// The work of the test function: a small loop with no memory use.
#[inline(always)]
fn work(n: u32) -> u32 {
    let mut x = 0x1234_5678u32;
    let mut s = 0u32;
    let mut i = 0;
    while i < n {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        s = s.wrapping_add(x).rotate_left(3);
        i += 1;
    }
    s
}

/// Runs from the second core's cache RAM. (Each copy returns its own
/// "salt" mixed in, so the compiler cannot fold the three into one.)
#[inline(never)]
#[unsafe(link_section = ".cpu1_cache_text.lab_probe")]
fn probe_area(n: u32) -> u32 {
    work(n) ^ 0xA
}

/// Runs from the normal code RAM (IRAM at 0x4008_xxxx).
#[inline(never)]
#[unsafe(link_section = ".rwtext.lab_probe")]
fn probe_iram(n: u32) -> u32 {
    work(n) ^ 0xB
}

/// Runs from flash (through the cache).
#[inline(never)]
fn probe_flash(n: u32) -> u32 {
    work(n) ^ 0xC
}

fn cycles(f: fn(u32) -> u32, n: u32) -> (u32, u32) {
    let t = xtensa_lx::timer::get_cycle_count();
    let r = f(core::hint::black_box(n));
    (r, xtensa_lx::timer::get_cycle_count().wrapping_sub(t))
}

/// Runs the test function from all three places, twice each, and logs
/// the results and the CPU cycles.
pub fn probe() {
    const N: u32 = 10_000;
    let fns: [(&str, fn(u32) -> u32, u32); 3] = [
        ("area", probe_area, 0xA),
        ("iram", probe_iram, 0xB),
        ("flash", probe_flash, 0xC),
    ];
    let want = work(core::hint::black_box(N));
    for (name, f, salt) in fns {
        let (r1, c1) = cycles(f, N);
        let (r2, c2) = cycles(f, N);
        let ok = r1 ^ salt == want && r2 ^ salt == want;
        info!(
            "cpu1 probe: {name} fn at {:#x} result={:#010x} ok={ok} cycles first={c1} second={c2}",
            f as usize,
            r1 ^ salt
        );
        if !ok {
            warn!("cpu1 probe: {name} gave the WRONG result");
        }
    }
}

// ---- running check: flash block and area ------------------------------

/// 128 KB of made-up constant data in flash: four times the first core's
/// 32 KB cache, so every pass pushes all of it out again.
const BLOCK_WORDS: usize = 32 * 1024;
static FLASH_BLOCK: [u32; BLOCK_WORDS] = make_block();
/// Its checksum, worked out when the firmware is built.
const BLOCK_SUM: u32 = block_sum();

const fn make_block() -> [u32; BLOCK_WORDS] {
    let mut b = [0u32; BLOCK_WORDS];
    let mut x = 0x9E37_79B9u32;
    let mut i = 0;
    while i < BLOCK_WORDS {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        b[i] = x;
        i += 1;
    }
    b
}

const fn block_sum() -> u32 {
    let b = make_block();
    let mut h = SUM_START;
    let mut i = 0;
    while i < BLOCK_WORDS {
        h = mix(h, b[i]);
        i += 1;
    }
    h
}

/// Words read per `tick` (2 KB).
const CHUNK: usize = 512;

pub struct Check {
    /// The letter `F` turns the flash part off and on (to see what the
    /// check itself costs the screen timing).
    pub on: bool,
    at: usize,
    sum: u32,
    pass_start: u64,
    pub passes: u32,
    pub bad: u32,
    pub pass_us_max: u64,
    pub chunk_us_max: u64,
}

impl Check {
    pub const fn new() -> Self {
        Self {
            on: true,
            at: 0,
            sum: SUM_START,
            pass_start: 0,
            passes: 0,
            bad: 0,
            pass_us_max: 0,
            chunk_us_max: 0,
        }
    }

    /// Reads the next 2 KB of the flash block; after a whole pass checks
    /// both sums.
    pub fn tick(&mut self) {
        if !self.on {
            return;
        }
        let t = crate::board::now_us();
        if self.at == 0 {
            self.pass_start = t;
        }
        let base = FLASH_BLOCK.as_ptr();
        let end = (self.at + CHUNK).min(BLOCK_WORDS);
        while self.at < end {
            // SAFETY: inside FLASH_BLOCK; volatile so that every word is
            // really read through the cache.
            self.sum = mix(self.sum, unsafe { base.add(self.at).read_volatile() });
            self.at += 1;
        }
        let now = crate::board::now_us();
        self.chunk_us_max = self.chunk_us_max.max(now - t);
        if self.at < BLOCK_WORDS {
            return;
        }
        self.pass_us_max = self.pass_us_max.max(now - self.pass_start);
        self.passes += 1;
        if self.sum != BLOCK_SUM {
            self.bad += 1;
            warn!(
                "cpu1 FLASH BLOCK WRONG: sum={:#010x} want={BLOCK_SUM:#010x} pass={}",
                self.sum, self.passes
            );
        }
        self.at = 0;
        self.sum = SUM_START;
        check_area();
    }
}

/// Compares the area's checksum with the one from `start`.
pub fn check_area() {
    let sum = area_sum();
    AREA_CHECKS.fetch_add(1, Relaxed);
    if sum != AREA_SUM.load(Relaxed) {
        AREA_BAD.fetch_add(1, Relaxed);
        warn!(
            "cpu1 AREA CHANGED: sum={sum:#010x} want={:#010x}",
            AREA_SUM.load(Relaxed)
        );
    }
}

/// Lab (cpu1-start-test): where the 128 KB flash block is.
#[cfg(feature = "cpu1-start-test")]
pub fn flash_block() -> *const u32 {
    FLASH_BLOCK.as_ptr()
}

/// Lab (cpu1-start-test): the area was overwritten on purpose; take its
/// new checksum as the one to compare with.
#[cfg(feature = "cpu1-start-test")]
pub fn rebase() {
    AREA_SUM.store(area_sum(), Relaxed);
}

/// One `cpu1` line for the 30-second report.
pub fn report(check: &Check) {
    check_area();
    info!(
        "cpu1 code_bytes={} area_checks={} area_bad={} flash_check={} flash_passes={} flash_bad={} flash_pass_us_max={} flash_chunk_us_max={} block_sum={BLOCK_SUM:#010x}",
        CODE_BYTES.load(Relaxed),
        AREA_CHECKS.load(Relaxed),
        AREA_BAD.load(Relaxed),
        if check.on { "on" } else { "off" },
        check.passes,
        check.bad,
        check.pass_us_max,
        check.chunk_us_max,
    );
    info!("cpu1 regs now: {}", regs());
}

/// Letter `R` (second reader): reads 64 KB of flash (this firmware's own
/// image, from 0x1_0000) twice through the flash driver (esp-storage, the
/// way saved pairings and game files will be read later), then checks
/// the area. Only reads; never writes or erases. Returns (first sum,
/// second sum, read errors).
pub fn driver_read_test() -> (u32, u32, u32) {
    let mut buf = [0u32; 1024];
    let mut sums = [SUM_START; 2];
    let mut errors = 0;
    for sum in sums.iter_mut() {
        let mut blk = 0u32;
        while blk < 16 {
            // SAFETY: 4 KB into a 4 KB word buffer, inside the 4 MB flash.
            let r = unsafe {
                esp_storage::ll::spiflash_read(0x1_0000 + blk * 4096, buf.as_mut_ptr(), 4096)
            };
            if r.is_err() {
                errors += 1;
            }
            for w in buf {
                *sum = mix(*sum, w);
            }
            blk += 1;
        }
    }
    check_area();
    (sums[0], sums[1], errors)
}

/// Letter `Z` (second reader's test, MEANT to break things): wipes the
/// radio's PHY code in the area to zeros NOW, while the radio is running.
/// - If the board then stops with an "illegal instruction" error at
///   0x4007_8xxx..0x4007_Fxxx, that code is used all the time, so a slow
///   corruption there would soon show up as a crash.
/// - If the board goes on running, that code is NOT used while the radio
///   just sits there: then only the checksum would notice a corruption.
/// The next area check also reports "AREA CHANGED": that shows the check
/// itself works. Restart the board afterwards.
pub fn wipe_phy_now() -> (usize, usize) {
    let from = (&raw const _cpu1_cache_own_end) as usize;
    let to = (&raw const _cpu1_cache_text_end) as usize;
    let mut at = from;
    while at < to {
        // SAFETY: a whole word in the area. Breaking the code there is
        // the point of this test.
        unsafe { (at as *mut u32).write_volatile(0) };
        at += 4;
    }
    (from, to)
}

/// Letter `B`: one single-byte read in the unused end of the area.
pub fn byte_read() -> u8 {
    let at = AREA + AREA_LEN - 15;
    // SAFETY: inside the area. This is the test: whether a byte read
    // here faults like it does in the rest of the code RAM.
    unsafe { (at as *const u8).read_volatile() }
}

/// Letter `W`: one single-byte write in the unused end of the area.
pub fn byte_write() {
    let at = AREA + AREA_LEN - 14;
    // SAFETY: inside the area, after the code (the zero fill). This is
    // the test, as for `byte_read`.
    unsafe { (at as *mut u8).write_volatile(0x5A) };
}
