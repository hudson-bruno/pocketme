//! The land's drawing order (the spec's section 3, drawing step 2): 16 m
//! block offsets from the camera's block, the largest squared distance
//! first; equal distances in rows (dj), then columns (di), from -REACH
//! up ([`ORDER`], made while compiling, SCREE's way).
//!
//! A level draws the order from its own first block (the first within
//! its far edge) to the end, so the farthest blocks come first and the
//! camera's own block last. [`key`] gives each offset a number that
//! grows along the order, so a thing standing in a block can be drawn
//! right after that block.
//!
//! **What is stored.** Not the whole order, but one quarter of it
//! ([`QUARTER`]: offsets with `di >= 0` and `dj >= 0`, the larger
//! squared distance first, then `dj` up); [`group`] turns the quarter's
//! offsets of one squared distance back into the whole order's: for a
//! squared distance `d2` and a row `dj`, `di` can only be `-a` or `a`
//! (`a * a = d2 - dj * dj`), so the rows `-b` (largest `b` first), then
//! `b` (smallest first), each with `-a` then `a`, are exactly the
//! order's. The checks while compiling below compare every entry.
//!
//! (Until MAX the whole order was stored, with a table of each offset's
//! place: 3,340 bytes for 15 blocks each way. Only the order of those
//! places was ever used, so the key, worked out instead of read, and the
//! quarter draw the very same pictures, and reach MAX's far edge in
//! fewer bytes.)

use crate::detail::Detail;

/// Blocks reach at most this far each way: MAX's far edge (16 m a
/// block).
pub const REACH: i32 = Detail::Max.reach().far_m as i32 / 16;
/// A block's side in game-3d units (16 m).
pub const BLOCK: i32 = 16 * 256;

/// How many offsets lie within `REACH` blocks (squared distance at most
/// `REACH * REACH`), in the whole plane (`quarter`: with `di >= 0` and
/// `dj >= 0` only).
const fn count(quarter: bool) -> usize {
    let lo = if quarter { 0 } else { -REACH };
    let mut n = 0;
    let mut dj = lo;
    while dj <= REACH {
        let mut di = lo;
        while di <= REACH {
            if di * di + dj * dj <= REACH * REACH {
                n += 1;
            }
            di += 1;
        }
        dj += 1;
    }
    n
}

/// Entries in the whole order.
pub const N: usize = count(false);
/// Entries in the quarter.
pub const NQ: usize = count(true);
/// The largest squared distance in the table.
const D2_MAX: usize = (REACH * REACH) as usize;

/// The whole order: (di, dj) offsets, farthest first. Equal distances
/// keep the order rows (dj) then columns (di), from -REACH up. A
/// constant (not stored in the game): the checks and the tests compare
/// [`group`] with it.
pub const ORDER: [[i8; 2]; N] = build(false);

/// The stored quarter: (di, dj) with both at least 0, farthest first,
/// equal distances by dj up (see the top).
pub static QUARTER: [[u8; 2]; NQ] = quarter();

/// A counting sort by squared distance, largest first, rows then
/// columns from the lowest up (a few thousand steps while compiling);
/// the whole plane, or (`quarter`) only `di, dj >= 0`.
const fn build<const K: usize>(quarter: bool) -> [[i8; 2]; K] {
    let lo = if quarter { 0 } else { -REACH };
    // How many offsets have each squared distance.
    let mut count = [0usize; D2_MAX + 1];
    let mut dj = lo;
    while dj <= REACH {
        let mut di = lo;
        while di <= REACH {
            let d2 = (di * di + dj * dj) as usize;
            if d2 <= D2_MAX {
                count[d2] += 1;
            }
            di += 1;
        }
        dj += 1;
    }
    // Where each distance's run starts, largest distance first.
    let mut at = [0usize; D2_MAX + 1];
    let mut next = 0;
    let mut d = D2_MAX as i32;
    while d >= 0 {
        at[d as usize] = next;
        next += count[d as usize];
        d -= 1;
    }
    let mut out = [[0i8; 2]; K];
    let mut dj = lo;
    while dj <= REACH {
        let mut di = lo;
        while di <= REACH {
            let d2 = (di * di + dj * dj) as usize;
            if d2 <= D2_MAX {
                out[at[d2]] = [di as i8, dj as i8];
                at[d2] += 1;
            }
            di += 1;
        }
        dj += 1;
    }
    out
}

const fn quarter() -> [[u8; 2]; NQ] {
    let q = build::<NQ>(true);
    let mut out = [[0u8; 2]; NQ];
    let mut k = 0;
    while k < NQ {
        out[k] = [q[k][0] as u8, q[k][1] as u8];
        k += 1;
    }
    out
}

/// The squared distance of quarter entry `k`.
#[inline(always)]
const fn qd2(k: usize) -> i32 {
    let [a, b] = QUARTER[k];
    let (a, b) = (a as i32, b as i32);
    a * a + b * b
}

/// The most whole-order entries one squared distance has (worked out
/// while compiling).
pub const MAX_GROUP: usize = max_group();

const fn max_group() -> usize {
    let mut most = 0;
    let mut n = 0;
    let mut k = 0;
    while k < N {
        let [di, dj] = ORDER[k];
        let d2 = di as i32 * di as i32 + dj as i32 * dj as i32;
        if k > 0 {
            let [pi, pj] = ORDER[k - 1];
            if pi as i32 * pi as i32 + pj as i32 * pj as i32 != d2 {
                n = 0;
            }
        }
        n += 1;
        if n > most {
            most = n;
        }
        k += 1;
    }
    most
}

/// The whole order's entries of the squared distance of quarter entry
/// `at` (the first of its distance), in order, into `out`: how many,
/// and the quarter entry of the next distance.
#[inline(always)]
pub const fn group(at: usize, out: &mut [[i8; 2]; MAX_GROUP]) -> (usize, usize) {
    let d2 = qd2(at);
    let mut end = at + 1;
    while end < NQ && qd2(end) == d2 {
        end += 1;
    }
    let mut n = 0;
    // Rows -b, the largest b first (b > 0), then rows b, the smallest
    // first; each row -a then a (a once when it is 0).
    let mut pass = 0;
    while pass < 2 {
        let mut i = 0;
        while i < end - at {
            let k = if pass == 0 { end - 1 - i } else { at + i };
            let [a, b] = QUARTER[k];
            let (a, b) = (a as i8, b as i8);
            if pass == 1 || b > 0 {
                let dj = if pass == 0 { -b } else { b };
                if a > 0 {
                    out[n] = [-a, dj];
                    n += 1;
                }
                out[n] = [a, dj];
                n += 1;
            }
            i += 1;
        }
        pass += 1;
    }
    (n, end)
}

/// The first quarter entry a level draws: the first one whose block
/// lies within `blocks` blocks (squared distance at most `blocks *
/// blocks`).
pub const fn start(blocks: i32) -> usize {
    let mut k = 0;
    while k < NQ {
        if qd2(k) <= blocks * blocks {
            return k;
        }
        k += 1;
    }
    NQ
}

/// The first quarter entries of LOW, MID, HIGH and MAX, from their far
/// edges (16 m a block).
pub static STARTS: [u16; 4] = [
    start(Detail::Low.reach().far_m as i32 / 16) as u16,
    start(Detail::Mid.reach().far_m as i32 / 16) as u16,
    start(Detail::High.reach().far_m as i32 / 16) as u16,
    start(Detail::Max.reach().far_m as i32 / 16) as u16,
];

/// A number for the block offset (di, dj), its squared distance at most
/// `REACH * REACH`, that grows along [`ORDER`]: the larger squared
/// distance first, then rows (dj), then columns (di), as the order is
/// sorted. Below 2^22, so a unit (`key * 4 + 3`) fits in 24 bits.
#[inline(always)]
pub const fn key(di: i32, dj: i32) -> u32 {
    let d2 = (di * di + dj * dj) as u32;
    ((D2_MAX as u32 - d2) << 12) | (((dj + REACH) as u32) << 6) | (di + REACH) as u32
}

const _: () = {
    // The fields of `key` do not overlap, and a unit fits in 24 bits.
    assert!(2 * REACH < 64);
    assert!((D2_MAX as u32) < 1 << 10);
    // `key` grows along the order.
    let mut k = 1;
    while k < N {
        let ([a, b], [c, d]) = (ORDER[k - 1], ORDER[k]);
        assert!(key(a as i32, b as i32) < key(c as i32, d as i32));
        k += 1;
    }
    // The quarter's groups, one after the other, are the whole order,
    // entry by entry; and each level's first quarter entry starts the
    // whole order's first entry within its far edge.
    let mut out = [[0i8; 2]; MAX_GROUP];
    let mut at = 0;
    let mut whole = 0;
    // Levels whose first quarter entry was met (one bit each).
    let mut met = 0;
    while at < NQ {
        let mut level = 0;
        while level < 4 {
            if STARTS[level] as usize == at {
                let b = [Detail::Low, Detail::Mid, Detail::High, Detail::Max][level]
                    .reach()
                    .far_m as i32
                    / 16;
                let [di, dj] = ORDER[whole];
                assert!(di as i32 * di as i32 + dj as i32 * dj as i32 <= b * b);
                if whole > 0 {
                    let [pi, pj] = ORDER[whole - 1];
                    assert!(pi as i32 * pi as i32 + pj as i32 * pj as i32 > b * b);
                }
                met |= 1 << level;
            }
            level += 1;
        }
        let (n, next) = group(at, &mut out);
        let mut i = 0;
        while i < n {
            assert!(out[i][0] == ORDER[whole][0] && out[i][1] == ORDER[whole][1]);
            whole += 1;
            i += 1;
        }
        at = next;
    }
    assert!(whole == N && met == 15);
};
