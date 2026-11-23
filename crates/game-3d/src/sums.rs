//! Checked or wrapping sums, chosen while compiling.
//!
//! Every sum, difference and product the library works out on corners,
//! planes and pixels goes through one of two types:
//!
//! * [`Checked`]: plain Rust sums. With overflow checks on (the chip
//!   build of a game has them on unless the game turns them off, as
//!   CAMBER does; the PC tests always have them on) a sum that runs
//!   over stops the game, at the line of the library that did it.
//!   This is how KESTREL and CAMBER were written. With checks off they
//!   wrap round like [`Wrapping`] ones.
//! * [`Wrapping`]: sums that wrap round silently, as MERCATOR writes
//!   the ones whose range is known. The chip code is smaller (no check,
//!   no panic call after each sum), but a sum that does run over gives
//!   a wrong number, so a wrong picture, instead of a stop. In debug
//!   builds (the PC tests) every such sum is still checked with a
//!   `debug_assert!`, so tests and fuzz runs catch a range that is
//!   wrong.
//!
//! A game picks one in its [`crate::Setup`]; the compiler then builds
//! only that one. Nothing is decided at run time.
//!
//! Every function here is `#[inline(always)]` and `#[track_caller]`:
//! the sum is written into the caller, and a stop names the caller's
//! line, as if the sum had been written there by hand.

/// The sums the library uses. Two kinds exist: [`Checked`] and
/// [`Wrapping`]. (A game may write its own, but has no reason to.)
pub trait Sums: 'static {
    fn add(a: i32, b: i32) -> i32;
    fn sub(a: i32, b: i32) -> i32;
    fn mul(a: i32, b: i32) -> i32;
    fn neg(a: i32) -> i32;
    fn add64(a: i64, b: i64) -> i64;
    fn sub64(a: i64, b: i64) -> i64;
    fn mul64(a: i64, b: i64) -> i64;
    fn neg64(a: i64) -> i64;
    /// `a * b` in 64 bits (it can never run over).
    #[inline(always)]
    #[track_caller]
    fn wide(a: i32, b: i32) -> i64 {
        Self::mul64(i64::from(a), i64::from(b))
    }
    fn add8(a: u8, b: u8) -> u8;
    fn sub8(a: u8, b: u8) -> u8;
    fn mul8(a: u8, b: u8) -> u8;
    /// A counter going up (statistics, pixels painted).
    fn count(a: u32, b: u32) -> u32;
    fn uadd(a: usize, b: usize) -> usize;
    fn umul(a: usize, b: usize) -> usize;
    /// Bit `k` of a 32-bit mask (one bit per material).
    fn bit(k: u8) -> u32;
    /// `n` divided by `d` (above 0) rounded down, and the remainder
    /// (0..d).
    fn floor_div(n: i32, d: i32) -> (i32, i32);
}

/// Plain sums: they stop the game when they run over (with overflow
/// checks on, as in every chip build and every PC test).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Checked;

/// Sums that wrap round; checked only in debug builds. Smaller chip
/// code, but a range mistake gives a wrong picture instead of a stop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wrapping;

impl Sums for Checked {
    #[inline(always)]
    #[track_caller]
    fn add(a: i32, b: i32) -> i32 {
        a + b
    }
    #[inline(always)]
    #[track_caller]
    fn sub(a: i32, b: i32) -> i32 {
        a - b
    }
    #[inline(always)]
    #[track_caller]
    fn mul(a: i32, b: i32) -> i32 {
        a * b
    }
    #[inline(always)]
    #[track_caller]
    fn neg(a: i32) -> i32 {
        -a
    }
    #[inline(always)]
    #[track_caller]
    fn add64(a: i64, b: i64) -> i64 {
        a + b
    }
    #[inline(always)]
    #[track_caller]
    fn sub64(a: i64, b: i64) -> i64 {
        a - b
    }
    #[inline(always)]
    #[track_caller]
    fn mul64(a: i64, b: i64) -> i64 {
        a * b
    }
    #[inline(always)]
    #[track_caller]
    fn neg64(a: i64) -> i64 {
        -a
    }
    #[inline(always)]
    #[track_caller]
    fn add8(a: u8, b: u8) -> u8 {
        a + b
    }
    #[inline(always)]
    #[track_caller]
    fn sub8(a: u8, b: u8) -> u8 {
        a - b
    }
    #[inline(always)]
    #[track_caller]
    fn mul8(a: u8, b: u8) -> u8 {
        a * b
    }
    #[inline(always)]
    #[track_caller]
    fn count(a: u32, b: u32) -> u32 {
        a + b
    }
    #[inline(always)]
    #[track_caller]
    fn uadd(a: usize, b: usize) -> usize {
        a + b
    }
    #[inline(always)]
    #[track_caller]
    fn umul(a: usize, b: usize) -> usize {
        a * b
    }
    #[inline(always)]
    #[track_caller]
    fn bit(k: u8) -> u32 {
        1 << k
    }
    #[inline(always)]
    #[track_caller]
    fn floor_div(n: i32, d: i32) -> (i32, i32) {
        (n.div_euclid(d), n.rem_euclid(d))
    }
}

/// Checks a wrapping sum in debug builds only.
macro_rules! wrap {
    ($a:expr, $b:expr, $checked:ident, $wrapping:ident) => {{
        debug_assert!($a.$checked($b).is_some(), "game-3d: a sum ran over");
        $a.$wrapping($b)
    }};
}

impl Sums for Wrapping {
    #[inline(always)]
    #[track_caller]
    fn add(a: i32, b: i32) -> i32 {
        wrap!(a, b, checked_add, wrapping_add)
    }
    #[inline(always)]
    #[track_caller]
    fn sub(a: i32, b: i32) -> i32 {
        wrap!(a, b, checked_sub, wrapping_sub)
    }
    #[inline(always)]
    #[track_caller]
    fn mul(a: i32, b: i32) -> i32 {
        wrap!(a, b, checked_mul, wrapping_mul)
    }
    #[inline(always)]
    #[track_caller]
    fn neg(a: i32) -> i32 {
        debug_assert!(a.checked_neg().is_some(), "game-3d: a sum ran over");
        a.wrapping_neg()
    }
    #[inline(always)]
    #[track_caller]
    fn add64(a: i64, b: i64) -> i64 {
        wrap!(a, b, checked_add, wrapping_add)
    }
    #[inline(always)]
    #[track_caller]
    fn sub64(a: i64, b: i64) -> i64 {
        wrap!(a, b, checked_sub, wrapping_sub)
    }
    #[inline(always)]
    #[track_caller]
    fn mul64(a: i64, b: i64) -> i64 {
        wrap!(a, b, checked_mul, wrapping_mul)
    }
    #[inline(always)]
    #[track_caller]
    fn neg64(a: i64) -> i64 {
        debug_assert!(a.checked_neg().is_some(), "game-3d: a sum ran over");
        a.wrapping_neg()
    }
    #[inline(always)]
    #[track_caller]
    fn add8(a: u8, b: u8) -> u8 {
        wrap!(a, b, checked_add, wrapping_add)
    }
    #[inline(always)]
    #[track_caller]
    fn sub8(a: u8, b: u8) -> u8 {
        wrap!(a, b, checked_sub, wrapping_sub)
    }
    #[inline(always)]
    #[track_caller]
    fn mul8(a: u8, b: u8) -> u8 {
        wrap!(a, b, checked_mul, wrapping_mul)
    }
    #[inline(always)]
    #[track_caller]
    fn count(a: u32, b: u32) -> u32 {
        wrap!(a, b, checked_add, wrapping_add)
    }
    #[inline(always)]
    #[track_caller]
    fn uadd(a: usize, b: usize) -> usize {
        wrap!(a, b, checked_add, wrapping_add)
    }
    #[inline(always)]
    #[track_caller]
    fn umul(a: usize, b: usize) -> usize {
        wrap!(a, b, checked_mul, wrapping_mul)
    }
    #[inline(always)]
    #[track_caller]
    fn bit(k: u8) -> u32 {
        debug_assert!(k < 32, "game-3d: material number above 31");
        1u32.wrapping_shl(u32::from(k))
    }
    #[inline(always)]
    fn floor_div(n: i32, d: i32) -> (i32, i32) {
        floor_div(n, d)
    }
}

/// MERCATOR's division: one call for every edge, small numbers only
/// (below 2^28), so the sums cannot run over.
#[inline(never)]
fn floor_div(n: i32, d: i32) -> (i32, i32) {
    let d = d.max(1);
    let (q, r) = (n / d, n % d);
    if r < 0 {
        (q.wrapping_sub(1), r.wrapping_add(d))
    } else {
        (q, r)
    }
}
