//! Measuring only (feature `xip-probe`, phase A-i of the study of game
//! code run straight from flash): a block of 128 KB of probe code in the
//! firmware's own flash code (`.text`, the area the bootloader already
//! maps, before the hot block), run on the FIRST core, to measure on the
//! board what a flash cache miss costs and what flash code run by the
//! first core does to the second core's timed screen windows. Phase A-i
//! was run on the board on 2026-11-22; its results are kept outside the
//! repository. README "Flash code probe (xip-probe,
//! measuring only)"; `xip-check.py` checks a built firmware against this
//! file.
//!
//! The block is 4096 lines of 32 bytes, one flash cache line each, all
//! the same, written as bytes (no assembler can change them):
//!
//! ```text
//! 0  0b 22  addi.n a2, a2, -1   one line counted
//! 2  cc 02  bnez.n a2, +4       not the last: on past the return
//! 4  0d f0  ret.n               the last line: back (through a0)
//! 6  3d f0  nop.n               x13, so the whole line is fetched
//! ```
//!
//! then one more `ret.n` after the end. No literal, no call, no absolute
//! jump: the block never reaches out of itself, and it can be entered at
//! any line with `a2` = the number of lines to run. It is called with
//! `callx0` from two small trampolines in RAM (`.rwtext`), between two
//! reads of the cycle counter, so no flash code but the block is timed;
//! the `quiet` one masks this core's interrupts while it runs (never the
//! other core's).
//!
//! Every TIMED run (A1 and the per-frame load alike) goes in chunks of
//! [`CHUNK`] lines (1 KB), each through the quiet trampoline: interrupts
//! fall between chunks, so the summed time holds no interrupt handler,
//! and no chunk masks interrupts for long (ESTIMATED: 32 misses, about
//! 122 us at 40 MHz, FACTS 3.10). The whole part's wall time (chunks,
//! calls and the interrupts between them) is kept beside it.
//!
//! No cache flush is used anywhere: a part is made cold by running other
//! probe code in between (the whole rest of the block, 124 KB or more,
//! for every load and for A1), which pushes it out of the first core's
//! 32 KB two-way cache.
//!
//! Letters (serial port, `boardctl text`), first core only:
//! - `0`..`5`: a load of 0, 8, 16, 32, 48 or 64 KB of probe code every
//!   console frame (A2/A3), each frame the NEXT part of the block in turn,
//!   so every part is cold; also measures the warm time for that size and
//!   resets the probe's counters.
//! - `j`: the next start offset of the rotation (0, 4, 8, 12 KB), which
//!   picks the cache sets the parts use; resets the counters.
//! - `d`: the next delay of the part after a handed picture (0, 2, 4,
//!   8 ms), to move the part over the second core's send; resets the
//!   counters.
//! - `a`: A1, the cost of one miss: 4 KB and 16 KB parts, warm and cold,
//!   8 times each (blocks the loop for about a quarter of a second).
//! - `r` also clears the probe's counters, `m` also logs them.

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};

use log::info;

use crate::lcd::{CPU_MHZ, cycles};

/// Bytes in one flash cache line, and in one line of the block.
const LINE: u32 = 32;
/// Lines in the block (128 KB).
const LINES: u32 = 4096;
/// Lines 16 KB apart share a cache set: 512 sets of two ways.
const SET_LINES: u32 = 512;
/// Lines per quiet call of a timed run (1 KB).
const CHUNK: u32 = 32;
/// The loads of the letters `0`..`5`, in KB per console frame.
const LOADS_KB: [u32; 6] = [0, 8, 16, 32, 48, 64];
/// The delays of the letter `d`, in ms after a handed picture.
const DELAYS_MS: [u32; 4] = [0, 2, 4, 8];
/// The parts of A1, in KB.
const A1_KB: [u32; 2] = [4, 16];
const A1_REPEATS: u32 = 8;
/// Timed warm runs per measurement (the fastest counts).
const WARM_TRIES: u32 = 3;
/// Before any A1: a part is cold only if it took at least this many
/// cycles a line more than warm. ESTIMATED: about half of the fastest
/// miss estimate (1.05 us at 80 MHz QIO = 252 cycles, FACTS 3.10); this
/// board runs DIO, which is slower.
const FLOOR_CYC: u32 = 120;

/// The layout as the firmware uses it, for `xip-check.py` (it checks the
/// values against the block): "XIPP", lines, line bytes, lines per cache
/// way, then the four start offsets of the letter `j`, in lines (0, 4, 8,
/// 12 KB). Read at run time through `black_box`, so the firmware uses
/// exactly these values and the linker keeps the table.
#[unsafe(no_mangle)]
static CYD_XIP_PROBE_LAYOUT: [u32; 8] = [0x5050_4958, LINES, LINE, SET_LINES, 0, 128, 256, 384];

/// Where the start offsets begin in [`CYD_XIP_PROBE_LAYOUT`].
const STARTS_AT: usize = 4;
const STARTS: u32 = 4;

// The block (in flash code, `.text`, by esp-hal's catch-all for
// `.text.*`; not in the hot block, whose list names other sections) and
// the two trampolines (in RAM, by esp-hal's `.rwtext.*`). Each trampoline
// is called with the windowed ABI (a2 = entry address, a3 = lines; the
// cycles come back in a2), keeps its return address in a5 while
// `callx0` uses a0, and times only the `callx0`. The cycle counter's
// 32-bit difference is exact for anything under 17.9 s.
core::arch::global_asm!(
    ".pushsection .text.cyd_xip_probe_block,\"ax\",@progbits",
    ".p2align 5",
    ".globl _cyd_xip_probe_start",
    ".type _cyd_xip_probe_start,@function",
    "_cyd_xip_probe_start:",
    ".rept 4096",
    ".byte 0x0b,0x22,0xcc,0x02,0x0d,0xf0",
    ".rept 13",
    ".byte 0x3d,0xf0",
    ".endr",
    ".endr",
    ".globl _cyd_xip_probe_end",
    "_cyd_xip_probe_end:",
    ".byte 0x0d,0xf0",
    ".size _cyd_xip_probe_start, 131072",
    ".popsection",
    "",
    ".pushsection .rwtext.cyd_xip_probe_run,\"ax\",@progbits",
    ".p2align 2",
    ".globl cyd_xip_probe_run",
    ".type cyd_xip_probe_run,@function",
    "cyd_xip_probe_run:",
    "entry a1, 32",
    "or a5, a0, a0",
    "or a4, a2, a2",
    "or a2, a3, a3",
    "rsr.ccount a6",
    "callx0 a4",
    "rsr.ccount a7",
    "or a0, a5, a5",
    "sub a2, a7, a6",
    "retw",
    ".size cyd_xip_probe_run, . - cyd_xip_probe_run",
    "",
    ".p2align 2",
    ".globl cyd_xip_probe_run_quiet",
    ".type cyd_xip_probe_run_quiet,@function",
    "cyd_xip_probe_run_quiet:",
    "entry a1, 32",
    "or a5, a0, a0",
    "or a4, a2, a2",
    "or a2, a3, a3",
    "rsil a9, 5",
    "rsr.ccount a6",
    "callx0 a4",
    "rsr.ccount a7",
    "wsr.ps a9",
    "rsync",
    "or a0, a5, a5",
    "sub a2, a7, a6",
    "retw",
    ".size cyd_xip_probe_run_quiet, . - cyd_xip_probe_run_quiet",
    ".popsection",
);

unsafe extern "C" {
    /// The block's first line (its bytes are never read as data).
    static _cyd_xip_probe_start: [u8; 0];
    /// Runs `lines` lines from `entry` with interrupts on; the cycles.
    fn cyd_xip_probe_run(entry: u32, lines: u32) -> u32;
    /// The same with this core's interrupts masked (level 5) meanwhile.
    fn cyd_xip_probe_run_quiet(entry: u32, lines: u32) -> u32;
}

/// The block's first line, as an address.
fn block() -> u32 {
    (&raw const _cyd_xip_probe_start) as u32
}

/// The layout table, read so that the compiler cannot fold it away.
fn layout(i: usize) -> u32 {
    core::hint::black_box(&CYD_XIP_PROBE_LAYOUT)[i]
}

/// One call into the block: `n` (at least 1) lines from line `at`, which
/// must not run past the block's end. The cycles inside the block.
fn call(at: u32, n: u32, quiet: bool) -> u32 {
    let entry = block() + at * layout(2);
    // SAFETY: `entry` is the start of a line of the block and `n` (at
    // least 1) lines follow it inside the block (the callers keep
    // `at + n` within the layout's lines), so the run ends at a `ret.n`
    // of the block: it touches no memory and no register the trampoline
    // does not save, and it returns to the trampoline, which returns
    // here (README "Flash code probe").
    unsafe {
        if quiet {
            cyd_xip_probe_run_quiet(entry, n)
        } else {
            cyd_xip_probe_run(entry, n)
        }
    }
}

/// Runs `n` lines from line `at` (taken modulo the block), untimed, with
/// interrupts on; over the end it goes on at line 0 (two calls).
fn run(at: u32, n: u32) {
    let lines = layout(1);
    let (mut at, mut left) = (at % lines, n.min(lines));
    while left > 0 {
        let now = left.min(lines - at);
        call(at, now, false);
        left -= now;
        at = 0;
    }
}

/// Runs `n` lines from line `at` timed, in quiet chunks of [`CHUNK`]
/// lines: (the chunks' cycles summed, the wall cycles of the whole run,
/// with the calls and the interrupts that came between chunks).
fn timed(at: u32, n: u32) -> (u32, u32) {
    let lines = layout(1);
    let (mut at, mut left, mut sum) = (at % lines, n.min(lines), 0u32);
    let t0 = cycles();
    while left > 0 {
        let now = left.min(CHUNK).min(lines - at);
        sum = sum.wrapping_add(call(at, now, true));
        left -= now;
        at = (at + now) % lines;
    }
    (sum, cycles().wrapping_sub(t0))
}

/// A sum of cycles in two words (no 64-bit atomics on this chip). Only
/// the first core's console task uses it.
struct Sum {
    lo: AtomicU32,
    hi: AtomicU32,
}

impl Sum {
    const fn new() -> Sum {
        Sum {
            lo: AtomicU32::new(0),
            hi: AtomicU32::new(0),
        }
    }

    fn get(&self) -> u64 {
        u64::from(self.hi.load(Relaxed)) << 32 | u64::from(self.lo.load(Relaxed))
    }

    fn add(&self, c: u32) {
        let s = self.get() + u64::from(c);
        self.lo.store(s as u32, Relaxed);
        self.hi.store((s >> 32) as u32, Relaxed);
    }

    fn clear(&self) {
        self.lo.store(0, Relaxed);
        self.hi.store(0, Relaxed);
    }
}

// The settings and counters. Only the console's task on the first core
// uses them (one at a time), so plain loads and stores are enough.
static READY: AtomicBool = AtomicBool::new(false);
/// Index into [`LOADS_KB`], the start offset's index (letter `j`) and the
/// index into [`DELAYS_MS`] (letter `d`).
static LOAD: AtomicU32 = AtomicU32::new(0);
static START: AtomicU32 = AtomicU32::new(0);
static DELAY: AtomicU32 = AtomicU32::new(0);
/// The line the next part starts at.
static NEXT: AtomicU32 = AtomicU32::new(0);
/// The warm time of one part of the current load, in cycles, and whether
/// it is worked out from a 16 KB part (a part over 16 KB cannot be run
/// warm in a 32 KB two-way cache by sure).
static WARM: AtomicU32 = AtomicU32::new(0);
static WARM_EST: AtomicBool = AtomicBool::new(false);
/// A1's average cost of one miss (16 KB parts), in cycles per line; 0
/// before the first A1 since the start.
static A1_LINE: AtomicU32 = AtomicU32::new(0);
/// Console frames seen, those handed to the second core, parts run, parts
/// that were not cold, calls on the second core (never expected), parts
/// that began / ended while the second core held a job.
static FRAMES: AtomicU32 = AtomicU32::new(0);
static HANDED: AtomicU32 = AtomicU32::new(0);
static PARTS: AtomicU32 = AtomicU32::new(0);
static NOTCOLD: AtomicU32 = AtomicU32::new(0);
static CPU1: AtomicU32 = AtomicU32::new(0);
static BUSY_START: AtomicU32 = AtomicU32::new(0);
static BUSY_END: AtomicU32 = AtomicU32::new(0);
/// A part's cycles (quiet chunks summed): smallest, largest, sum; and its
/// wall cycles: largest, sum.
static MIN: AtomicU32 = AtomicU32::new(u32::MAX);
static MAX: AtomicU32 = AtomicU32::new(0);
static SUM: Sum = Sum::new();
static WALL_MAX: AtomicU32 = AtomicU32::new(0);
static WALL: Sum = Sum::new();

fn reset() {
    for c in [
        &FRAMES,
        &HANDED,
        &PARTS,
        &NOTCOLD,
        &CPU1,
        &BUSY_START,
        &BUSY_END,
        &MAX,
        &WALL_MAX,
    ] {
        c.store(0, Relaxed);
    }
    SUM.clear();
    WALL.clear();
    MIN.store(u32::MAX, Relaxed);
}

fn load_kb() -> u32 {
    LOADS_KB[LOAD.load(Relaxed) as usize]
}

fn delay_ms() -> u32 {
    DELAYS_MS[DELAY.load(Relaxed) as usize]
}

/// The start offset in lines (letter `j`).
fn start_line() -> u32 {
    layout(STARTS_AT + START.load(Relaxed) as usize)
}

/// From one part to the next: the part itself, but at least 16 KB, so
/// every part starts at the same place of the cache's sets (the start
/// offset's) and 8 KB parts share their sets with the other parts. From
/// 16 KB up the parts follow on without a gap, so between two runs of a
/// line the whole rest of the block runs (7 other lines of its set); an
/// 8 KB part's lines see the 7 other parts on their sets in between.
fn step(lines: u32) -> u32 {
    lines.max(SET_LINES)
}

/// The cache set of a line of the block (its first set for a part).
fn set_of(line: u32) -> u32 {
    (block() + line * LINE) / LINE % SET_LINES
}

fn us(c: u64) -> u64 {
    c / u64::from(CPU_MHZ)
}

/// Cycles to nanoseconds.
fn ns(c: u64) -> u64 {
    c * 1000 / u64::from(CPU_MHZ)
}

fn is_first_core() -> bool {
    esp_hal::system::Cpu::current() == esp_hal::system::Cpu::ProCpu
}

/// Measures the warm time of a part of `lines` at the start offset (the
/// fastest of [`WARM_TRIES`] timed runs, each right after the same part
/// ran): parts over 16 KB are worked out from a 16 KB one. Then runs the
/// rest of the block once, so the next part is cold again.
fn measure_warm(lines: u32) {
    let at = start_line();
    let n = lines.min(SET_LINES);
    let mut best = u32::MAX;
    for _ in 0..WARM_TRIES {
        run(at, n);
        best = best.min(timed(at, n).0);
    }
    run(at + n, LINES - n);
    let warm = u64::from(best) * u64::from(lines) / u64::from(n);
    WARM.store(warm as u32, Relaxed);
    WARM_EST.store(lines > n, Relaxed);
}

/// The first line, once (at the first console frame).
fn ready() {
    if !READY.swap(true, Relaxed) {
        info!(
            "xip: block {:#x}+{} ({} lines of {} B) set0={}, letters a j d 0-5",
            block(),
            layout(1) * layout(2),
            layout(1),
            layout(2),
            set_of(0),
        );
    }
}

/// Once per console frame (App::step, right after the picture was handed
/// to the second core), on the first core: the next part of the load,
/// after the delay of the letter `d` when a picture was handed.
pub fn frame(handed: bool) {
    if !is_first_core() {
        CPU1.fetch_add(1, Relaxed);
        return;
    }
    ready();
    FRAMES.fetch_add(1, Relaxed);
    if handed {
        HANDED.fetch_add(1, Relaxed);
    }
    let kb = load_kb();
    if kb == 0 {
        return;
    }
    let wait = delay_ms() * 1000 * CPU_MHZ;
    if handed && wait > 0 {
        let t = cycles();
        while cycles().wrapping_sub(t) < wait {
            core::hint::spin_loop();
        }
    }
    let lines = kb * 1024 / LINE;
    let at = NEXT.load(Relaxed);
    let busy = crate::core2::busy();
    let (c, wall) = timed(at, lines);
    if busy {
        BUSY_START.fetch_add(1, Relaxed);
    }
    if crate::core2::busy() {
        BUSY_END.fetch_add(1, Relaxed);
    }
    NEXT.store((at + step(lines)) % LINES, Relaxed);
    PARTS.fetch_add(1, Relaxed);
    SUM.add(c);
    MIN.fetch_min(c, Relaxed);
    MAX.fetch_max(c, Relaxed);
    WALL.add(wall);
    WALL_MAX.fetch_max(wall, Relaxed);
    if c < not_cold_below(lines) {
        NOTCOLD.fetch_add(1, Relaxed);
    }
}

/// A part faster than this was not cold: warm plus half of A1's miss
/// cost for every line, or warm plus [`FLOOR_CYC`] a line before any A1.
fn not_cold_below(lines: u32) -> u32 {
    let warm = WARM.load(Relaxed);
    let per = match A1_LINE.load(Relaxed) {
        0 => FLOOR_CYC,
        miss => miss / 2,
    };
    warm.saturating_add(per.saturating_mul(lines))
}

/// A serial letter; true when it is the probe's alone (`r` and `m` also
/// go on to the console's own handling).
pub fn letter(l: u8) -> bool {
    match l {
        b'0'..=b'5' => {
            LOAD.store(u32::from(l - b'0'), Relaxed);
            restart("load");
            true
        }
        b'j' => {
            START.store((START.load(Relaxed) + 1) % STARTS, Relaxed);
            restart("start");
            true
        }
        b'd' => {
            DELAY.store((DELAY.load(Relaxed) + 1) % DELAYS_MS.len() as u32, Relaxed);
            restart("delay");
            true
        }
        b'a' => {
            a1();
            true
        }
        b'r' => {
            reset();
            info!("xip: counters reset");
            false
        }
        b'm' => {
            report();
            false
        }
        _ => false,
    }
}

/// After a load, start or delay letter: warm time, rotation from the
/// start offset, counters to zero.
fn restart(what: &str) {
    let kb = load_kb();
    let lines = kb * 1024 / LINE;
    if kb > 0 {
        measure_warm(lines);
    } else {
        WARM.store(0, Relaxed);
        WARM_EST.store(false, Relaxed);
    }
    NEXT.store(start_line(), Relaxed);
    reset();
    info!(
        "xip: {what}: load {kb} KB/frame, start {} KB (set {}), step {} KB, delay {} ms",
        start_line() * LINE / 1024,
        set_of(start_line()),
        step(lines) * LINE / 1024,
        delay_ms(),
    );
    info!(
        "xip: warm={} cyc ({}), counters reset",
        WARM.load(Relaxed),
        if WARM_EST.load(Relaxed) {
            "est from 16 KB"
        } else {
            "measured"
        },
    );
}

/// Letter `a`: A1, the cost of one cache miss. First waits until the
/// second core holds no job. Then for each part size, at the start
/// offset, [`A1_REPEATS`] times: the part once (warm it), the part timed
/// (warm), the REST of the block (at least 112 KB of other probe code:
/// the part is pushed out, no flush), the part timed (cold). Timed runs
/// in quiet chunks ([`timed`]). Per line = (cold - warm) / lines.
fn a1() {
    let joined = crate::core2::join() / CPU_MHZ;
    let at = start_line();
    let presents = crate::core2::snapshot().counters.presents;
    let t0 = cycles();
    info!(
        "xip a1: start {} KB (set {}), parts 4+16 KB, {A1_REPEATS} repeats, joined {joined} us",
        at * LINE / 1024,
        set_of(at),
    );
    for kb in A1_KB {
        let n = kb * 1024 / LINE;
        let (mut w_lo, mut w_hi, mut c_lo, mut c_hi) = (u32::MAX, 0, u32::MAX, 0);
        let (mut l_lo, mut l_hi, mut l_sum) = (u32::MAX, 0, 0u64);
        for _ in 0..A1_REPEATS {
            run(at, n);
            let warm = timed(at, n).0;
            run(at + n, LINES - n);
            let cold = timed(at, n).0;
            let per = cold.saturating_sub(warm) / n;
            (w_lo, w_hi) = (w_lo.min(warm), w_hi.max(warm));
            (c_lo, c_hi) = (c_lo.min(cold), c_hi.max(cold));
            (l_lo, l_hi) = (l_lo.min(per), l_hi.max(per));
            l_sum += u64::from(per);
        }
        let avg = l_sum / u64::from(A1_REPEATS);
        info!("xip a1 {kb}K warm_cyc={w_lo}/{w_hi} cold_cyc={c_lo}/{c_hi} (min/max)");
        info!(
            "xip a1 {kb}K line_ns min={} avg={} max={} = (cold-warm)/{n} lines",
            ns(u64::from(l_lo)),
            ns(avg),
            ns(u64::from(l_hi)),
        );
        if kb == 16 {
            A1_LINE.store(avg as u32, Relaxed);
        }
    }
    let ms = cycles().wrapping_sub(t0) / CPU_MHZ / 1000;
    let during = crate::core2::snapshot()
        .counters
        .presents
        .wrapping_sub(presents);
    info!("xip a1: done in {ms} ms, presents during it={during}");
}

/// Letter `m`: the probe's five lines (before the console's own report;
/// short, as the log cuts long lines).
fn report() {
    let kb = load_kb();
    let lines = kb * 1024 / LINE;
    let parts = PARTS.load(Relaxed);
    info!(
        "xip load={kb}K start={}K step={}K delay={}ms next={} frames={} handed={}",
        start_line() * LINE / 1024,
        step(lines) * LINE / 1024,
        delay_ms(),
        NEXT.load(Relaxed),
        FRAMES.load(Relaxed),
        HANDED.load(Relaxed),
    );
    info!(
        "xip parts={parts} notcold={} (below {} cyc)",
        NOTCOLD.load(Relaxed),
        not_cold_below(lines),
    );
    info!(
        "xip cpu1={} c2busy={}/{} (parts in a send job at start/end)",
        CPU1.load(Relaxed),
        BUSY_START.load(Relaxed),
        BUSY_END.load(Relaxed),
    );
    let min = if parts == 0 { 0 } else { MIN.load(Relaxed) };
    info!(
        "xip cyc min={min} max={} warm={}{} a1_line={}",
        MAX.load(Relaxed),
        WARM.load(Relaxed),
        if WARM_EST.load(Relaxed) { "(est)" } else { "" },
        A1_LINE.load(Relaxed),
    );
    let avg = SUM.get() / u64::from(parts.max(1));
    let wall = WALL.get() / u64::from(parts.max(1));
    let warm = u64::from(WARM.load(Relaxed));
    info!(
        "xip us avg={} max={} wall={}/{} line_ns={}",
        us(avg),
        us(u64::from(MAX.load(Relaxed))),
        us(wall),
        us(u64::from(WALL_MAX.load(Relaxed))),
        ns(avg.saturating_sub(warm) / u64::from(lines.max(1))),
    );
}
