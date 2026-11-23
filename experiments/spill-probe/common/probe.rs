// The spill probe (experiments/spill-probe/README.md): a test game whose
// code is bigger than the 56 KB code slot, so that its end runs on into
// the spill, the top of the data slot seen from the instruction bus
// (game_api::layout::CODE_SPILL_ADDR, stage 1). Included by
// spill/src/lib.rs and spillmax/src/lib.rs, which set
// - PAD: how many bulk functions there are (this sets the code size);
// - BUF_WORDS: the data buffer, in 32-bit words;
// - GAP_BYTES: how far the data ends below the spill, measured on the PC
//   (only for the one-off SPILLOVER link, see `OVER`).
//
// A test game, not one for players: never in games/, never on a normal
// card. It uses only the game kit: no unsafe, no heap, no floating point.
//
// What it does:
// - Bulk code: PAD distinct functions `pad::<K>`, never inlined, reached
//   through a table of function pointers (`TABLE`, a constant in the data
//   slot). With enough of them the last ones lie in the spill, and one
//   crosses its start (checked on the PC with nm, README).
// - Self-check, at the start and once a second: every table entry is
//   called and the results are folded into a sum, which must equal
//   `EXPECTED`, worked out by the compiler from the same formula. The
//   table and the input pass through `core::hint::black_box`, so the
//   compiler cannot work the run-time sum out ahead and the calls really
//   go to the functions where they lie (checked in the disassembly,
//   README). Log: `spill: fns=N below=A above=B check=PASS sum=0x...`.
// - Data near the spill: `BUF`, sized so that SPILL's data ends within
//   64 bytes below the spill. Every frame the probe checks the pattern
//   it wrote the frame before and writes a new one.
// - Speed phases, 10 s each, in a loop: the same fixed loop (`spin`) run
//   from the copy with the lowest address (A), the highest (B), the
//   highest while also reading the buffer (C), the lowest while reading
//   it (D). Log: `spill phase=A fn=0x...`. The console's `perf` lines
//   give the time per frame.

use core::fmt::Write as _;
use core::hint::black_box;
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

use game_kit::api::layout::CODE_SPILL_ADDR;
use game_kit::{Ctx, Flow, Game, rgb565};

/// One bulk function.
pub type Pad = fn(u32) -> u32;
/// One copy of the speed loop: (loops, start value, data to read).
pub type Spin = fn(u32, u32, &[AtomicU32]) -> u32;

/// What every bulk function is given (through `black_box` at run time).
pub const SEED: u32 = 0x5EED_C0DE;

/// The work of `pad::<K>`: eight rounds of mixing, each with constants
/// made from `k`, so that no two functions are the same (the compiler
/// would merge equal ones) and each is about 100 to 200 bytes of code.
#[inline(always)]
pub const fn mix(k: u32, x: u32) -> u32 {
    let mut h = x ^ k.wrapping_mul(0x9E37_79B9);
    macro_rules! round {
        ($n:literal) => {
            h = (h ^ k
                .wrapping_mul(0x85EB_CA6B)
                .wrapping_add(0x27D4_EB2F_u32.wrapping_mul($n)))
            .rotate_left($n * 3 + 1)
            .wrapping_mul(0xCC9E_2D51 ^ ($n * 0x0101_0100));
            h ^= h >> 15;
        };
    }
    round!(1);
    round!(2);
    round!(3);
    round!(4);
    round!(5);
    round!(6);
    round!(7);
    round!(8);
    h
}

/// One bulk function. `const`, so `EXPECTED` can be worked out from the
/// same formula; never inlined, so each is real code at its own address.
#[inline(never)]
pub const fn pad<const K: u32>(x: u32) -> u32 {
    mix(K, x)
}

/// How the results are folded: order matters, so a call that lands in
/// the wrong function changes the sum.
pub const fn fold(sum: u32, v: u32) -> u32 {
    sum.rotate_left(1) ^ v
}

/// The sum the self-check must find, worked out by the compiler.
pub const EXPECTED: u32 = {
    let mut sum = 0;
    let mut k = 0;
    while k < PAD {
        sum = fold(sum, mix(k as u32, SEED));
        k += 1;
    }
    sum
};

// Ten bulk functions, `pad::<A*100 + B*10 + 0..9>`.
macro_rules! row {
    ($a:literal $b:literal) => {
        [
            pad::<{ $a * 100 + $b * 10 }>,
            pad::<{ $a * 100 + $b * 10 + 1 }>,
            pad::<{ $a * 100 + $b * 10 + 2 }>,
            pad::<{ $a * 100 + $b * 10 + 3 }>,
            pad::<{ $a * 100 + $b * 10 + 4 }>,
            pad::<{ $a * 100 + $b * 10 + 5 }>,
            pad::<{ $a * 100 + $b * 10 + 6 }>,
            pad::<{ $a * 100 + $b * 10 + 7 }>,
            pad::<{ $a * 100 + $b * 10 + 8 }>,
            pad::<{ $a * 100 + $b * 10 + 9 }>,
        ]
    };
}
// A hundred.
macro_rules! block {
    ($a:literal) => {
        [
            row!($a 0),
            row!($a 1),
            row!($a 2),
            row!($a 3),
            row!($a 4),
            row!($a 5),
            row!($a 6),
            row!($a 7),
            row!($a 8),
            row!($a 9),
        ]
    };
}

/// Every possible bulk function, `pad::<0>` to `pad::<999>`. Only the
/// first PAD go into `TABLE`; the link keeps only those (the rest is
/// never referred to, and the linker drops it).
const ALL: [[[Pad; 10]; 10]; 10] = [
    block!(0),
    block!(1),
    block!(2),
    block!(3),
    block!(4),
    block!(5),
    block!(6),
    block!(7),
    block!(8),
    block!(9),
];
const _: () = assert!(PAD >= 1 && PAD <= 1000);

/// The bulk functions the probe calls, in order (a constant: on the chip
/// it is in the data slot).
pub static TABLE: [Pad; PAD] = {
    let mut t: [Pad; PAD] = [ALL[0][0][0]; PAD];
    let mut i = 0;
    while i < PAD {
        t[i] = ALL[i / 100][i / 10 % 10][i % 10];
        i += 1;
    }
    t
};

/// What one self-check found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Check {
    /// Functions called.
    pub fns: u32,
    /// Of them, below the spill (in the code slot) and at or above its
    /// start (in the spill). On the PC the addresses mean nothing.
    pub below: u32,
    pub above: u32,
    /// The folded results, and whether they equal `EXPECTED`.
    pub sum: u32,
    pub pass: bool,
}

/// Calls every function of `table` with `x`, in order, and folds the
/// results; counts where the functions lie.
#[inline(never)]
pub fn run_table(table: &[Pad], x: u32) -> Check {
    let mut c = Check::default();
    for &f in table {
        if f as usize >= CODE_SPILL_ADDR as usize {
            c.above += 1;
        } else {
            c.below += 1;
        }
        c.sum = fold(c.sum, f(x));
        c.fns += 1;
    }
    c.pass = c.sum == EXPECTED;
    c
}

/// The self-check: `run_table` on `TABLE` and `SEED`, both through
/// `black_box`, so the compiler knows neither and must make the calls.
#[inline(never)]
pub fn self_check() -> Check {
    run_table(black_box(&TABLE[..]), black_box(SEED))
}

/// The speed loop: `n` rounds of a little mixing, and with `data` one
/// word of it read per round. Every copy is this same body.
macro_rules! spin_body {
    ($k:expr, $n:expr, $x:expr, $data:expr) => {{
        let (n, data) = ($n, $data);
        let mut x = $x ^ $k;
        let mut j = 0;
        let mut i = 0;
        while i < n {
            x = x.rotate_left(3).wrapping_mul(0x9E37_79B1).wrapping_add(i);
            if let Some(w) = data.get(j) {
                x ^= w.load(Relaxed);
                j += 1;
                if j == data.len() {
                    j = 0;
                }
            }
            i += 1;
        }
        x
    }};
}

/// Eight copies of the speed loop under one name and eight under another.
/// Two names because the linker happens to order a game's functions by
/// their (mangled) names: `low::<K>` comes before the `pad` functions, in
/// the code slot, and `spin::<K>` after them, in the spill. Where each
/// copy really lies is read at run time (the lowest and the highest) and
/// checked on the PC with nm (README).
#[inline(never)]
pub fn low<const K: u32>(n: u32, x: u32, data: &[AtomicU32]) -> u32 {
    spin_body!(K, n, x, data)
}

/// See [`low`].
#[inline(never)]
pub fn spin<const K: u32>(n: u32, x: u32, data: &[AtomicU32]) -> u32 {
    spin_body!(K, n, x, data)
}

/// The 16 copies of the speed loop (through `black_box` when used). `K`
/// starts at 1: with 0 the compiler drops the first step, and that copy
/// would be shorter than the others.
pub static SPINS: [Spin; 16] = [
    low::<1>, low::<2>, low::<3>, low::<4>, low::<5>, low::<6>, low::<7>, low::<8>, spin::<9>,
    spin::<10>, spin::<11>, spin::<12>, spin::<13>, spin::<14>, spin::<15>, spin::<16>,
];

/// Rounds of the speed loop per frame (a few milliseconds on the chip,
/// ESTIMATED; the board's `perf` line measures it).
pub const SPIN_ROUNDS: u32 = 120_000;
/// How long each speed phase lasts, in microseconds.
pub const PHASE_US: u64 = 10_000_000;

/// One word more than the gap, for the one-off SPILLOVER link (README):
/// `SPILL_PROBE_OVER=1` at build time makes the data exactly one word too
/// big, and the link must fail. Never set for a file that is used.
const OVER: usize = match option_env!("SPILL_PROBE_OVER") {
    Some(_) => GAP_BYTES / 4 + 1,
    None => 0,
};

/// The data buffer, near the spill (zeroed memory, so it costs the file
/// nothing). A static of atomics and not part of the game's state: the
/// game kit builds the state on the stack once at the start, and 40 KB
/// would not fit there. On this one-core use the atomics are plain loads
/// and stores.
pub static BUF: [AtomicU32; BUF_WORDS + OVER] = [const { AtomicU32::new(0) }; BUF_WORDS + OVER];

/// The pattern of generation `g` in word `i`.
pub const fn pattern(g: u32, i: usize) -> u32 {
    (i as u32).wrapping_mul(0x9E37_79B1) ^ g.wrapping_mul(0x85EB_CA77) ^ 0xA5A5_5A5A
}

/// Checks the pattern of generation `g` in `BUF` (when `g` is not 0) and
/// writes generation `g + 1`. Returns the words that were wrong.
pub fn data_pass(g: u32) -> u32 {
    let mut bad = 0;
    for (i, w) in BUF.iter().enumerate() {
        if g != 0 && w.load(Relaxed) != pattern(g, i) {
            bad += 1;
        }
        w.store(pattern(g.wrapping_add(1), i), Relaxed);
    }
    bad
}

// Colours (palette numbers this game sets at the start).
const BG: u8 = 1;
const TEXT: u8 = 2;
const GOOD: u8 = 3;
const BAD: u8 = 4;
const COLOURS: [u16; 4] = [
    rgb565(16, 20, 32),
    rgb565(230, 230, 240),
    rgb565(80, 220, 110),
    rgb565(240, 60, 60),
];

/// Text for the screen, without a heap.
struct Line {
    buf: [u8; 40],
    len: usize,
}

impl Line {
    fn new() -> Self {
        Line {
            buf: [0; 40],
            len: 0,
        }
    }
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("?")
    }
}

impl core::fmt::Write for Line {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for b in s.bytes() {
            if self.len < self.buf.len() {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
        Ok(())
    }
}

/// The phase names.
const PHASES: [&str; 4] = ["A", "B", "C", "D"];

pub struct Probe {
    /// The last self-check, and how many failed so far.
    check: Check,
    fails: u32,
    next_check_us: u64,
    /// The speed loop copies with the lowest and the highest address.
    low: Spin,
    high: Spin,
    /// The phase shown (4 = none yet).
    phase: usize,
    /// The data pattern's generation, and words found wrong so far.
    gen_: u32,
    data_bad: u32,
    /// The speed loop's result, kept so it is not thrown away.
    x: u32,
}

impl Probe {
    fn self_check(&mut self, ctx: &mut Ctx) {
        self.check = self_check();
        if !self.check.pass {
            self.fails += 1;
        }
        let c = self.check;
        ctx.log_fmt(format_args!(
            "spill: fns={} below={} above={} check={} sum={:#010x} expected={:#010x} data={} bad_words={} fails={}",
            c.fns,
            c.below,
            c.above,
            if c.pass { "PASS" } else { "FAIL" },
            c.sum,
            EXPECTED,
            if self.data_bad == 0 { "PASS" } else { "FAIL" },
            self.data_bad,
            self.fails,
        ));
    }

    fn ok(&self) -> bool {
        self.check.pass && self.fails == 0 && self.data_bad == 0
    }

    fn draw(&self, ctx: &mut Ctx) {
        if !ctx.has_screen() {
            return;
        }
        let ok = self.ok();
        let c = self.check;
        let s = ctx.screen();
        s.clear(BG);
        s.text(4, 4, "SPILL PROBE", TEXT);
        s.text_scaled(
            20,
            22,
            if ok { "PASS" } else { "FAIL" },
            if ok { GOOD } else { BAD },
            5,
        );
        let mut l = Line::new();
        let _ = write!(l, "FNS {} BELOW {} ABOVE {}", c.fns, c.below, c.above);
        s.text(4, 70, l.as_str(), TEXT);
        let mut l = Line::new();
        let _ = write!(l, "SUM {:08X}", c.sum);
        s.text(4, 80, l.as_str(), TEXT);
        let mut l = Line::new();
        let _ = write!(l, "DATA {} WORDS BAD {}", BUF.len(), self.data_bad);
        s.text(4, 90, l.as_str(), TEXT);
        let mut l = Line::new();
        let _ = write!(
            l,
            "PHASE {}",
            PHASES.get(self.phase).copied().unwrap_or("-")
        );
        s.text(4, 104, l.as_str(), TEXT);
    }
}

impl Game for Probe {
    fn start(ctx: &mut Ctx) -> Self {
        ctx.set_colours(BG, &COLOURS);
        // The copies of the speed loop with the lowest and the highest
        // address.
        let spins: &[Spin] = black_box(&SPINS[..]);
        let (mut low, mut high) = (spins[0], spins[0]);
        for &f in spins {
            if (f as usize) < (low as usize) {
                low = f;
            }
            if (f as usize) > (high as usize) {
                high = f;
            }
        }
        let mut p = Probe {
            check: Check::default(),
            fails: 0,
            next_check_us: 0,
            low,
            high,
            phase: 4,
            gen_: 0,
            data_bad: 0,
            x: 0,
        };
        p.self_check(ctx);
        p.next_check_us = ctx.time_us() + 1_000_000;
        p
    }

    fn update(&mut self, ctx: &mut Ctx) -> Flow {
        if ctx.started() || ctx.must_redraw() {
            ctx.set_colours(BG, &COLOURS);
        }
        // The data: last frame's pattern still there, then a new one.
        self.data_bad += data_pass(self.gen_);
        // (After 0xFFFF_FFFF comes 0, which `data_pass` does not check.)
        self.gen_ = self.gen_.wrapping_add(1);

        // The speed phase of this moment.
        let phase = (ctx.time_us() / PHASE_US % 4) as usize;
        let (f, data): (Spin, &[AtomicU32]) = match phase {
            0 => (self.low, &[]),
            1 => (self.high, &[]),
            2 => (self.high, &BUF[..]),
            _ => (self.low, &BUF[..]),
        };
        if phase != self.phase {
            self.phase = phase;
            ctx.log_fmt(format_args!(
                "spill phase={} fn={:#010x} data={} rounds={}",
                PHASES[phase],
                f as usize,
                if data.is_empty() { "no" } else { "read" },
                SPIN_ROUNDS
            ));
        }
        self.x = black_box(f)(black_box(SPIN_ROUNDS), self.x, data);

        if ctx.time_us() >= self.next_check_us {
            self.self_check(ctx);
            self.next_check_us = ctx.time_us() + 1_000_000;
        }
        self.draw(ctx);
        Flow::Run
    }
}

game_kit::game!(Probe);
