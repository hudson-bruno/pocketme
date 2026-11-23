//! Constant tables, all computed while compiling with whole numbers only.
//! In flash on the chip. Sine, sphere and reciprocal tables are SWARM's.

use super::{CAM, POINTS, SPHERE};
use crate::theme::{self, UI_COLOURS};

/// Sine and cosine are in 1/2^14.
pub const COS_SIN_SHIFT: u32 = 14;
const QUARTER: usize = 256;

/// sin(k * 90 degrees / 256) in 1/16384, k = 0..=256 (games/kestrel's
/// table: a Taylor series in 128-bit whole numbers).
const SIN_Q: [i16; QUARTER + 1] = sin_table();

const PI_Q40: i128 = 3_454_217_652_358;

const fn sin_table() -> [i16; QUARTER + 1] {
    let mut t = [0i16; QUARTER + 1];
    let mut k = 0;
    while k <= QUARTER {
        let x = PI_Q40 * k as i128 / (2 * QUARTER as i128);
        let x2 = (x * x) >> 40;
        let mut term = x;
        let mut sum = x;
        let mut n = 1i128;
        while n < 10 {
            term = -((term * x2) >> 40) / ((2 * n) * (2 * n + 1));
            sum += term;
            n += 1;
        }
        t[k] = ((sum + (1 << 25)) >> 26) as i16;
        k += 1;
    }
    t
}

/// A whole turn of sine, 1,024 steps, so a lookup needs no quadrant
/// test. 2,048 bytes.
static SIN_FULL: [i16; 1024] = {
    let mut t = [0i16; 1024];
    let mut i = 0;
    while i < 1024 {
        let q = i & (QUARTER - 1);
        t[i] = match i / QUARTER {
            0 => SIN_Q[q],
            1 => SIN_Q[QUARTER - q],
            2 => -SIN_Q[q],
            _ => -SIN_Q[QUARTER - q],
        };
        i += 1;
    }
    t
};

/// Sine of a `u16` angle, in 1/16384 (1,024 steps a turn).
#[inline(always)]
pub fn sin(a: u16) -> i32 {
    i32::from(SIN_FULL[usize::from(a >> 6)])
}

#[inline(always)]
pub fn cos(a: u16) -> i32 {
    sin(a.wrapping_add(16_384))
}

const fn isqrt(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// Radius of the sphere's ring at point i's height (Fibonacci sphere).
/// 4,096 bytes.
pub static SPHERE_R: [u16; POINTS] = {
    let mut t = [0u16; POINTS];
    let n = POINTS as i64;
    let s = SPHERE as i64;
    let mut i = 0;
    while i < POINTS {
        let y = s * (2 * i as i64 + 1 - n) / n;
        t[i] = isqrt(s * s - y * y) as u16;
        i += 1;
    }
    t
};

/// First depth slot in [`RECIP`] (depths from 8 * this to 8,191).
pub const RECIP_FIRST: usize = 325;
/// 16 * 32,768 * CAM / depth / 16: picture position in 1/16 pixel is
/// (x * RECIP[depth / 8]) >> 16. 1,398 bytes.
pub static RECIP: [u16; 1024 - RECIP_FIRST] = {
    let mut t = [0u16; 1024 - RECIP_FIRST];
    let mut k = 0;
    while k < t.len() {
        let z = ((k + RECIP_FIRST) * 8 + 4) as i64;
        t[k] = (32_768 * CAM as i64 / z) as u16;
        k += 1;
    }
    t
};

/// RGB565 from 0xRRGGBB, as the console's own colours.
const fn rgb(hex: u32) -> u16 {
    theme::rgb(hex)
}

/// `a` to `b` (0xRRGGBB), `f` of `d` of the way, rounded.
const fn mix(a: u32, b: u32, f: u32, d: u32) -> u32 {
    let mut out = 0u32;
    let mut sh = 0;
    while sh <= 16 {
        let x = ((a >> sh) & 255) as i32;
        let y = ((b >> sh) & 255) as i32;
        let v = x + ((y - x) * f as i32 + (d as i32) / 2) / d as i32;
        out |= (v as u32) << sh;
        sh += 8;
    }
    out
}

/// One ramp of 64 levels, straight lines between key colours.
const fn ramp(p: &mut [u16; 256], first: usize, keys: &[(u32, u32)]) {
    let mut k = 0;
    while k + 1 < keys.len() {
        let (l0, c0) = keys[k];
        let (l1, c1) = keys[k + 1];
        let mut l = l0;
        while l <= l1 {
            p[first + l as usize] = rgb(mix(c0, c1, l - l0, l1 - l0));
            l += 1;
        }
        k += 1;
    }
}

const C_INK: u32 = UI_COLOURS[0];
const C_PAPER: u32 = UI_COLOURS[(theme::PAPER - theme::INK) as usize];
const C_GOLD: u32 = UI_COLOURS[(theme::GOLD - theme::INK) as usize];
const C_MUTE: u32 = UI_COLOURS[(theme::MUTE - theme::INK) as usize];
const C_LINE: u32 = UI_COLOURS[(theme::LINE - theme::INK) as usize];
/// A star just landed (the brightest of the star fade).
const C_STAR: u32 = 0xc8ecff;

/// The 256 colours, set once on the first frame and never changed:
/// - 0..63 ice, 64..127 violet, 128..191 ember: light ramps for the
///   points (level 0 is the background; ember 63 is exactly PAPER);
/// - 192..199 the gleam: gold (192) to almost paper (199);
/// - 200..231 the name while it shrinks: 4 tints (paper to mute) x 8
///   coverages (1/8 to 8/8 over the background);
/// - 236..239 a landed star fading to LINE;
/// - 240..247 the console's own 8 colours (`theme::UI_COLOURS`), the
///   same as the console sets. 232..235 and 248..255 unused (0).
pub static PALETTE: [u16; 256] = {
    let mut p = [0u16; 256];
    ramp(
        &mut p,
        super::ICE as usize,
        &[
            (0, C_INK),
            (8, 0x0f1d44),
            (20, 0x1a4a9a),
            (34, 0x2f8fe0),
            (48, 0x7fdcff),
            (58, 0xd6f6ff),
            (63, 0xffffff),
        ],
    );
    ramp(
        &mut p,
        super::VIOLET as usize,
        &[
            (0, C_INK),
            (8, 0x24113f),
            (20, 0x55208c),
            (34, 0xa040d0),
            (48, 0xe88af0),
            (58, 0xffd8f8),
            (63, 0xfff4ff),
        ],
    );
    ramp(
        &mut p,
        super::EMBER as usize,
        &[
            (0, C_INK),
            (8, 0x2a1408),
            (20, 0x7a3410),
            (34, 0xd0701e),
            (46, C_GOLD),
            (56, 0xf8e9a8),
            (63, C_PAPER),
        ],
    );
    let mut k = 0;
    while k < 8 {
        p[super::GLEAM as usize + k] = rgb(mix(C_GOLD, C_PAPER, k as u32, 9));
        k += 1;
    }
    let mut tint = 0;
    while tint < super::TINTS {
        let c = mix(C_PAPER, C_MUTE, tint as u32, super::TINTS as u32 - 1);
        let mut cov = 1;
        while cov <= 8 {
            p[super::TITLE as usize + tint * 8 + cov - 1] = rgb(mix(C_INK, c, cov as u32, 8));
            cov += 1;
        }
        tint += 1;
    }
    let mut s = 0;
    while s < 4 {
        p[super::STAR_FADE as usize + s] = rgb(mix(C_STAR, C_LINE, s as u32 + 1, 5));
        s += 1;
    }
    let mut i = 0;
    while i < UI_COLOURS.len() {
        p[theme::INK as usize + i] = rgb(UI_COLOURS[i]);
        i += 1;
    }
    p
};

/// The palette the console has without the boot animation: only its own
/// colours (`theme::UI_COLOURS` at 240..247), every other number 0. Set
/// when the boot hands over, so home's colours are exactly those of a
/// console without the animation.
pub static PLAIN: [u16; 256] = {
    let mut p = [0u16; 256];
    let mut i = 0;
    while i < UI_COLOURS.len() {
        p[theme::INK as usize + i] = rgb(UI_COLOURS[i]);
        i += 1;
    }
    p
};
