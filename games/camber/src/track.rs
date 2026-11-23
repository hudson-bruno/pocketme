//! The tracks: closed ribbons of road in 3D, written as a list of pieces
//! and worked out while compiling into one record per 8 m segment.
//!
//! A piece says how many segments it has, how far the road turns over
//! it, and the climb (pitch), the tilt (bank) and the half width it has
//! at its end; pitch, bank and width change evenly along the piece. Its
//! kind says what the road is there (walls, a tunnel roof, a gap to jump,
//! a boost pad, the recharge lane) and its scenery what stands beside it.
//!
//! The bake ([`bake`]) follows the pieces, adds up the positions, spreads
//! the small error that is left at the end over the whole loop (so the
//! ribbon closes exactly), then works out from the corrected points each
//! boundary's heading and climb, the racing line the rivals follow and
//! the speed they may take into each segment. All of this happens while
//! compiling; the game only reads the records.

use crate::fixed::{atan2, cos, deg, hash, isqrt, sin, turn};

/// One segment's length in world units (1/256 m): 8 m.
pub const SEG: i32 = 2048;
/// Bits of a segment's length in fine units (1/65536 m): 8 m = 2^19.
pub const SEG_BITS: u32 = 19;
/// One metre in fine units.
pub const FM: i32 = 65_536;

/// The road's width, in percent of what the pieces below say. Every
/// piece's half width is scaled by it once, in the bake (while
/// compiling): 110 made every track about 10 % wider (the owner's wish,
/// 2026-11-21). 100 gives the widths as written (within 3/16 m between
/// pieces of different width; see `bake`). The scaled half width is kept
/// in 1/16 m ([`Seg::half`]), so the scale is exact to 1/32 m.
pub const WIDEN: i32 = 110;
/// World units (1/256 m) in one unit of [`Seg::half`] (1/16 m).
pub const HALF_UNIT: i32 = 16;

// ---- Kinds of road (per segment) -------------------------------------------

/// A wall on the left edge.
pub const WALL_L: u8 = 1;
/// A wall on the right edge.
pub const WALL_R: u8 = 2;
/// Both walls.
pub const WALLS: u8 = WALL_L | WALL_R;
/// A tunnel: tall walls and a roof.
pub const ROOF: u8 = 4;
/// No road: the gap of a jump.
pub const GAP: u8 = 8;
/// A boost pad on this segment.
pub const BOOST: u8 = 16;
/// The recharge lane along the left edge.
pub const CHARGE: u8 = 32;

// ---- Scenery (asked per piece, placed per boundary) ------------------------

/// Light posts on both sides.
pub const LIGHTS: u8 = 1;
/// An arch over the road.
pub const ARCH: u8 = 2;
/// Things beside the road, left.
pub const SCEN_L: u8 = 4;
/// Things beside the road, right.
pub const SCEN_R: u8 = 8;
/// Pillars under the road.
pub const PILLAR: u8 = 16;
/// The start gantry (set by the bake at boundary 0).
pub const GANTRY: u8 = 32;
/// The mouth of a tunnel (set by the bake where a roof starts).
pub const PORTAL: u8 = 64;

/// One piece of a track as written.
#[derive(Clone, Copy, Debug)]
pub struct Piece {
    pub n: u8,
    /// Turn over the piece, degrees (positive: right).
    pub turn: i16,
    /// Climb at the end, degrees.
    pub pitch: i8,
    /// Bank at the end, degrees (positive: right side low).
    pub bank: i8,
    /// Half width at the end, 1/4 m, as written (the bake scales it by
    /// [`WIDEN`]).
    pub half: u8,
    pub kind: u8,
    pub deco: u8,
    /// Boost pad middle, 1/2 m right of the middle of the road.
    pub pad: i8,
}

/// A piece (no boost pad).
pub const fn p(n: u8, turn: i16, pitch: i8, bank: i8, half: u8, kind: u8, deco: u8) -> Piece {
    Piece {
        n,
        turn,
        pitch,
        bank,
        half,
        kind,
        deco,
        pad: 0,
    }
}

/// A piece with a boost pad on every segment, `pad` half metres right.
pub const fn b(n: u8, turn: i16, pitch: i8, bank: i8, half: u8, kind: u8, pad: i8) -> Piece {
    Piece {
        n,
        turn,
        pitch,
        bank,
        half,
        kind: kind | BOOST,
        deco: LIGHTS,
        pad,
    }
}

/// One boundary of the track and the segment that starts at it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seg {
    /// The middle of the road, world units (1/256 m).
    pub p: [i32; 3],
    /// Heading (0 along +z, a quarter turn along +x).
    pub yaw: u16,
    /// Climb and bank, in angle units (65,536 a turn).
    pub pitch: i16,
    pub bank: i16,
    /// Half width, 1/16 m (the pieces' width scaled by [`WIDEN`]).
    pub half: u8,
    /// The segment from here to the next boundary: [`WALL_L`] etc.
    pub kind: u8,
    /// What stands at this boundary: [`LIGHTS`] etc.
    pub deco: u8,
    /// The racing line here, 1/4 m right of the middle.
    pub line: i8,
    /// The speed the rivals may carry here, m/s.
    pub fast: u8,
    /// Boost pad middle, 1/2 m right of the middle.
    pub pad: i8,
}

const ZERO: Seg = Seg {
    p: [0; 3],
    yaw: 0,
    pitch: 0,
    bank: 0,
    half: 0,
    kind: 0,
    deco: 0,
    line: 0,
    fast: 0,
    pad: 0,
};

/// The number of segments of a list of pieces.
pub const fn count(pieces: &[Piece]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < pieces.len() {
        n += pieces[i].n as usize;
        i += 1;
    }
    n
}

/// Works a list of pieces out into `N` segment records (see the module
/// notes). Runs while compiling.
pub const fn bake<const N: usize>(pieces: &[Piece]) -> [Seg; N] {
    let mut s = [ZERO; N];
    let mut pos = [[0i64; 3]; N];
    // 1. Follow the pieces.
    let (mut yaw, mut pitch, mut bank, mut half) = (0i64, 0i64, 0i64, 28i64);
    let (mut x, mut y, mut z) = (0i64, 0i64, 0i64);
    let mut i = 0;
    let mut pi = 0;
    let mut roof_before = pieces[pieces.len() - 1].kind & ROOF != 0;
    while pi < pieces.len() {
        let pc = pieces[pi];
        let n = pc.n as i64;
        let (p0, b0, h0) = (pitch, bank, half);
        let (pe, be, he) = (
            deg(pc.pitch as i32) as i64,
            deg(pc.bank as i32) as i64,
            pc.half as i64,
        );
        let tt = deg(pc.turn as i32) as i64;
        let mut k = 0;
        while k < n {
            pos[i] = [x, y, z];
            let pk = p0 + (pe - p0) * k / n;
            s[i].bank = (b0 + (be - b0) * k / n) as i16;
            // The width as written (1/4 m, as before), then scaled by
            // WIDEN into 1/16 m, rounded.
            let q = h0 + (he - h0) * k / n;
            s[i].half = ((q * 4 * WIDEN as i64 + 50) / 100) as u8;
            s[i].kind = pc.kind;
            s[i].pad = pc.pad;
            // Scenery: asked per piece, placed by position.
            let mut d = 0u8;
            if pc.deco & LIGHTS != 0 && i % 2 == 0 {
                d |= LIGHTS;
            }
            if pc.deco & ARCH != 0 && k % 6 == 3 {
                d |= ARCH;
            }
            if pc.deco & PILLAR != 0 && i % 3 == 1 {
                d |= PILLAR;
            }
            let h = hash(i as u32, pi as u32);
            if pc.deco & SCEN_L != 0 && h % 5 < 2 {
                d |= SCEN_L;
            }
            if pc.deco & SCEN_R != 0 && (h >> 8) % 5 < 2 {
                d |= SCEN_R;
            }
            let roof = pc.kind & ROOF != 0;
            if roof && !roof_before {
                d |= PORTAL;
            }
            roof_before = roof;
            if i == 0 {
                d |= GANTRY;
            }
            s[i].deco = d;
            // To the next boundary, along the direction in the middle.
            let dy = tt * (k + 1) / n - tt * k / n;
            let pk1 = p0 + (pe - p0) * (k + 1) / n;
            let ym = (yaw + dy / 2) as i32 as u16;
            let pm = ((pk + pk1) / 2) as i32 as u16;
            let (sy, cy, sp, cp) = (
                sin(ym) as i64,
                cos(ym) as i64,
                sin(pm) as i64,
                cos(pm) as i64,
            );
            let l = SEG as i64;
            x += l * sy * cp / (1 << 28);
            y += l * sp / (1 << 14);
            z += l * cy * cp / (1 << 28);
            yaw += dy;
            i += 1;
            k += 1;
        }
        pitch = pe;
        bank = be;
        half = he;
        pi += 1;
    }
    // 2. Close the loop: spread what is left over every boundary.
    let e = [x, y, z];
    let mut j = 0;
    while j < N {
        let mut c = 0;
        while c < 3 {
            pos[j][c] -= e[c] * j as i64 / N as i64;
            c += 1;
        }
        s[j].p = [pos[j][0] as i32, pos[j][1] as i32, pos[j][2] as i32];
        j += 1;
    }
    // 3. Heading and climb from the neighbours.
    j = 0;
    while j < N {
        let a = pos[(j + N - 1) % N];
        let c = pos[(j + 1) % N];
        let (dx, dy, dz) = (c[0] - a[0], c[1] - a[1], c[2] - a[2]);
        s[j].yaw = atan2(dx, dz);
        let flat = isqrt((dx * dx + dz * dz) as u64) as i64;
        s[j].pitch = atan2(dy, flat) as i16;
        j += 1;
    }
    // 4. The racing line: towards the inside of the coming turns.
    let mut bend = [0i32; N];
    j = 0;
    while j < N {
        bend[j] = turn(s[j].yaw, s[(j + 1) % N].yaw);
        j += 1;
    }
    j = 0;
    while j < N {
        let mut sum = 0i32;
        let mut w = 0;
        while w < 7 {
            sum += bend[(j + w) % N] * mx(4 - abs(w as i32 - 2), 1);
            w += 1;
        }
        // About 1.2 m per degree a segment turns, at most 2.5 m from the
        // edges.
        let room = mx(s[j].half as i32 / 4 - 10, 0);
        let line = mn(mx(sum / 18 * 48 / 1000, -room), room);
        s[j].line = line as i8;
        j += 1;
    }
    // 4b. Let the line move at most LINE_STEP a segment on each side, by
    // starting each move earlier and ending it later (never by cutting a
    // peak short): a jump of the line is a jump of the computer's aim,
    // which starts the swing from side to side.
    let mut line0 = [0i32; N];
    j = 0;
    while j < N {
        line0[j] = s[j].line as i32;
        j += 1;
    }
    j = 0;
    while j < N {
        let (mut pos, mut neg) = (0i32, 0i32);
        let mut d = 0;
        while d <= 2 * LINE_REACH {
            let k = (j + N + d - LINE_REACH) % N;
            let far = abs(d as i32 - LINE_REACH as i32) * LINE_STEP;
            pos = mx(pos, line0[k] - far);
            neg = mx(neg, -line0[k] - far);
            d += 1;
        }
        let room = mx(s[j].half as i32 / 4 - 10, 0);
        s[j].line = mn(mx(pos - neg, -room), room) as i8;
        j += 1;
    }
    // 5. The speed the rivals may carry: v = turn rate / curvature, and
    // never more than they can brake away before the next segment.
    j = 0;
    while j < N {
        let k = mx(
            mx(abs(bend[j]), abs(bend[(j + 1) % N])),
            abs(bend[(j + 2) % N]),
        );
        // At most 1.5 rad/s through a turn of k angle units in 8 m, and
        // at most GRIP_A m/s^2 sideways: v = sqrt(a * 8 m * 10430 / k).
        let v = if k == 0 {
            255
        } else {
            mn(125_163 / k, isqrt((GRIP_A * 8 * 10_430 / k) as u64) as i32)
        };
        s[j].fast = if v > 255 { 255 } else { v as u8 };
        j += 1;
    }
    let mut round = 0;
    while round < 2 {
        j = N;
        while j > 0 {
            j -= 1;
            let next = s[(j + 1) % N].fast as i32 + 2;
            if (s[j].fast as i32) > next {
                s[j].fast = next as u8;
            }
        }
        round += 1;
    }
    s
}

/// How far the racing line may move a segment (1/4 m units: 0.5 m in
/// 8 m), and how many segments either side the bake looks for a move.
const LINE_STEP: i32 = 2;
const LINE_REACH: usize = 10;

/// The sideways pull the rivals plan a turn for (m/s^2).
const GRIP_A: i32 = 45;

const fn mx(a: i32, b: i32) -> i32 {
    if a > b { a } else { b }
}

const fn mn(a: i32, b: i32) -> i32 {
    if a < b { a } else { b }
}

const fn abs(x: i32) -> i32 {
    if x < 0 { -x } else { x }
}

// ---- The three tracks -----------------------------------------------------------

const W: u8 = WALLS;
const L: u8 = LIGHTS;
const SL: u8 = SCEN_L | SCEN_R;
const SP: u8 = SCEN_L | SCEN_R | PILLAR;

/// LAGOON: wide, flowing, gentle; for learning.
const LAGOON_P: [Piece; 17] = [
    p(10, 0, 0, 0, 28, W | CHARGE, L),
    b(2, 0, 0, 0, 28, W, -4),
    p(2, 0, 0, 0, 28, W, L),
    p(12, 60, 2, 8, 28, W, SL),
    p(5, 0, 4, 0, 28, W, SP | ARCH),
    p(5, 0, 0, 0, 28, W, SP),
    p(8, -30, -2, -6, 28, W, SL),
    p(18, 90, -3, 12, 28, W, SL | L),
    b(2, 10, 3, 4, 28, W, 6),
    p(10, 50, 3, 10, 28, W, SL),
    p(10, -40, 3, -8, 28, W, SP),
    p(16, 80, 0, 12, 28, W, SL | L),
    p(14, 0, -3, 0, 28, W, SP | ARCH),
    p(14, 70, 0, 10, 28, W, SL),
    p(8, -30, 0, -6, 28, W, SL),
    p(8, 30, 0, 6, 28, W, SL),
    p(12, 70, 0, 8, 28, W, L),
];

/// FURNACE: narrow and twisting, two tunnels, a hairpin.
const FURNACE_P: [Piece; 21] = [
    p(8, 0, 0, 0, 22, W | CHARGE, L),
    b(2, 0, 0, 0, 22, W, 3),
    p(2, 0, 0, 0, 22, W, L),
    p(8, -45, 0, -10, 22, W, SL),
    p(6, 0, 3, 0, 22, W | ROOF, 0),
    p(8, 50, 3, 12, 22, W | ROOF, 0),
    p(8, -50, 0, -12, 22, W, SL),
    p(10, 0, -3, 0, 24, W, SP | ARCH),
    p(14, -150, 0, -16, 22, W, SL | L),
    p(6, 0, 4, 0, 22, W, SL),
    b(2, 0, 4, 0, 22, W, -3),
    p(4, 0, 4, 0, 22, W, SL),
    p(6, 40, 4, 8, 22, W, SL),
    p(6, -40, 0, -8, 22, W, SL),
    p(4, 0, -2, 0, 22, W | ROOF, 0),
    p(5, 35, -3, 10, 22, W | ROOF, 0),
    p(7, -70, -4, -12, 22, W | ROOF, 0),
    p(5, 35, -4, 8, 22, W, SL),
    p(12, -90, 0, -14, 22, W, SL | L),
    p(6, 0, 0, 0, 24, W, SP),
    p(10, -75, 0, -12, 22, W, L),
];

/// GLACIER: big climbs and drops, a jump, open edges on the high road.
const GLACIER_P: [Piece; 25] = [
    p(8, 0, 0, 0, 26, W | CHARGE, L),
    b(2, 0, 0, 0, 26, W, 0),
    p(2, 0, 0, 0, 26, W, L),
    p(10, 0, 10, 0, 26, W, SP),
    p(6, 0, 10, 0, 26, W, SP),
    p(6, 0, -2, 0, 26, 0, PILLAR),
    p(10, 0, -10, 0, 26, W, SP),
    p(6, 0, 0, 0, 26, W, SL | ARCH),
    p(14, 90, 0, 14, 26, W, SL | L),
    p(8, 0, 8, 0, 26, W, SP),
    p(4, 0, 4, 0, 26, W, SP),
    b(2, 0, 4, 0, 24, W, 0),
    p(4, 0, 5, 0, 24, W, PILLAR),
    p(1, 0, -1, 0, 24, W, PILLAR),
    p(3, 0, -3, 0, 24, GAP, 0),
    p(6, 0, 0, 0, 26, W, SP | ARCH),
    p(16, 120, 0, 14, 26, W, SL | L),
    p(10, 0, -8, 0, 26, W, SP),
    p(10, 0, 8, 0, 26, W, SP),
    p(8, 0, 0, 0, 26, 0, PILLAR),
    p(8, 0, -8, 0, 26, W, SP),
    p(10, 0, 6, 0, 26, W, SL),
    p(10, 0, 0, 0, 26, W, SL | ARCH),
    p(12, 60, -6, 10, 26, W, SL),
    p(10, 90, 0, 14, 26, W, L),
];

const LAGOON_N: usize = count(&LAGOON_P);
const FURNACE_N: usize = count(&FURNACE_P);
const GLACIER_N: usize = count(&GLACIER_P);

static LAGOON: [Seg; LAGOON_N] = bake(&LAGOON_P);
static FURNACE: [Seg; FURNACE_N] = bake(&FURNACE_P);
static GLACIER: [Seg; GLACIER_N] = bake(&GLACIER_P);

/// A track: its name, its segments and its look ([`crate::art::THEMES`]).
pub struct Track {
    pub name: &'static str,
    pub segs: &'static [Seg],
}

/// The tracks, in menu order.
pub static TRACKS: [Track; 3] = [
    Track {
        name: "LAGOON",
        segs: &LAGOON,
    },
    Track {
        name: "FURNACE",
        segs: &FURNACE,
    },
    Track {
        name: "GLACIER",
        segs: &GLACIER,
    },
];

/// A place on the track worked out between two boundaries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Spot {
    /// The middle of the road, world units.
    pub p: [i32; 3],
    pub yaw: u16,
    pub pitch: i32,
    pub bank: i32,
    /// Half width, world units.
    pub half: i32,
    /// The segment and how far along it (0..65536).
    pub seg: usize,
    pub frac: i32,
}

impl Seg {
    /// Half width in world units (1/256 m).
    pub fn half_world(&self) -> i32 {
        i32::from(self.half) * HALF_UNIT
    }
}

impl Track {
    pub fn n(&self) -> usize {
        self.segs.len()
    }

    /// The length of a lap in fine units.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> i32 {
        (self.segs.len() as i32) << SEG_BITS
    }

    /// Segment `i`, counted round the loop.
    #[inline(never)]
    pub fn seg(&self, i: usize) -> &Seg {
        &self.segs[i % self.segs.len()]
    }

    /// The segment `s` (fine units along the lap) is on.
    #[inline(never)]
    pub fn index(&self, s: i32) -> usize {
        (s.rem_euclid(self.len()) >> SEG_BITS) as usize
    }

    /// The track at `s` (fine units along the lap).
    pub fn at(&self, s: i32) -> Spot {
        let s = s.rem_euclid(self.len());
        let i = (s >> SEG_BITS) as usize;
        let f = (s & ((1 << SEG_BITS) - 1)) >> (SEG_BITS - 16);
        let a = self.seg(i);
        let b = self.seg(i + 1);
        let lerp = |x: i32, y: i32| x + (((y - x) as i64 * f as i64) >> 16) as i32;
        Spot {
            p: [
                lerp(a.p[0], b.p[0]),
                lerp(a.p[1], b.p[1]),
                lerp(a.p[2], b.p[2]),
            ],
            yaw: (i32::from(a.yaw) + ((turn(a.yaw, b.yaw) * f) >> 16)) as u16,
            pitch: lerp(i32::from(a.pitch), i32::from(b.pitch)),
            bank: lerp(i32::from(a.bank), i32::from(b.bank)),
            half: lerp(i32::from(a.half), i32::from(b.half)) * HALF_UNIT,
            seg: i,
            frac: f,
        }
    }

    /// The road's half width at `s` (world units): `at(s).half` without
    /// the rest of the spot.
    pub fn half(&self, s: i32) -> i32 {
        let s = s.rem_euclid(self.len());
        let i = (s >> SEG_BITS) as usize;
        let f = (s & ((1 << SEG_BITS) - 1)) >> (SEG_BITS - 16);
        let (x, y) = (i32::from(self.seg(i).half), i32::from(self.seg(i + 1).half));
        (x + (((y - x) as i64 * f as i64) >> 16) as i32) * HALF_UNIT
    }

    /// The racing line at `s`, fine units right of the middle.
    #[inline(never)]
    pub fn line(&self, s: i32) -> i32 {
        let sp = self.index(s);
        let f = (s.rem_euclid(self.len()) & ((1 << SEG_BITS) - 1)) >> (SEG_BITS - 16);
        let a = i32::from(self.seg(sp).line);
        let b = i32::from(self.seg(sp + 1).line);
        (a * (65_536 - f) + b * f) / 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_piece_is_widened_by_widen() {
        // The road is the old one made 10 % wider.
        assert_eq!(WIDEN, 110);
        // Follow each track's pieces again here, with the widths as they
        // are written (1/4 m, eased from one piece to the next as the
        // bake does from 7 m at the start), and check every baked segment
        // is that width times 1.1 to within 1/32 m, in its own unit
        // (1/16 m) and as read in world units.
        let lists: [(&str, &[Piece]); 3] = [
            ("LAGOON", &LAGOON_P),
            ("FURNACE", &FURNACE_P),
            ("GLACIER", &GLACIER_P),
        ];
        for (t, (name, pieces)) in TRACKS.iter().zip(lists) {
            assert_eq!(t.name, name);
            let mut i = 0;
            let mut half = 28i64;
            for pc in pieces {
                let (h0, he, n) = (half, i64::from(pc.half), i64::from(pc.n));
                for k in 0..n {
                    let q = h0 + (he - h0) * k / n;
                    // 1/16 m times 100: q quarter metres times 4 times 1.1.
                    let want = q * 4 * 110;
                    let got = i64::from(t.segs[i].half) * 100;
                    assert!(
                        (got - want).abs() <= 50,
                        "{name} segment {i}: half {} sixteenths, want {q} x 4 x 1.1",
                        t.segs[i].half
                    );
                    // In world units (1/256 m): q * 64 * 1.1.
                    let w = i64::from(t.segs[i].half_world());
                    assert!(
                        (w * 10 - q * 64 * 11).abs() <= 80,
                        "{name} segment {i}: {w}"
                    );
                    i += 1;
                }
                half = he;
            }
            assert_eq!(i, t.segs.len(), "{name}: every segment checked");
        }
    }

    #[test]
    fn every_track_closes_and_keeps_its_segments_near_8_m() {
        for t in &TRACKS {
            let n = t.n();
            for i in 0..n {
                let a = t.seg(i).p;
                let b = t.seg(i + 1).p;
                let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]].map(i64::from);
                let len = isqrt((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) as u64) as i32;
                assert!(
                    (len - SEG).abs() < SEG / 25,
                    "{} segment {i} is {len} long",
                    t.name
                );
            }
        }
    }
}
