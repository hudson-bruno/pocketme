//! Shared test helpers: a dice, blank pictures, small models.

#![allow(dead_code)]

use game_3d::{Face, Model, ONE14, RawFace, Rect, SUB, Shading, normals, orient, radius_of};
use game_kit::{Frame, H, W};

pub struct Dice(pub u64);

impl Dice {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % (hi - lo + 1) as u64) as i32
    }
}

pub fn blank() -> Box<Frame> {
    Box::new(Frame::new())
}

pub const FULL: Rect = Rect::new(0, 0, W as i32, H as i32);

/// A corner somewhere on or round the picture, in 1/16 pixel. Some are
/// exactly on pixel middles or pixel edges, to test the edge rule.
pub fn corner(d: &mut Dice) -> [i32; 2] {
    match d.next() % 5 {
        0 => [
            d.range(0, 159) * SUB + SUB / 2,
            d.range(0, 119) * SUB + SUB / 2,
        ],
        1 => [d.range(0, 160) * SUB, d.range(0, 120) * SUB],
        2 => [d.range(-60 * SUB, 220 * SUB), d.range(-60 * SUB, 180 * SUB)],
        3 => [
            d.range(-game_3d::GUARD, game_3d::GUARD),
            d.range(-game_3d::GUARD, game_3d::GUARD),
        ],
        _ => [d.range(0, 160 * SUB), d.range(0, 120 * SUB)],
    }
}

/// `a*x + b*y + c*z + e` in plain 64-bit sums (the test's own copy).
pub fn plane_d(pl: &[i32; 4], p: &[i32; 3]) -> i64 {
    i64::from(pl[0]) * i64::from(p[0])
        + i64::from(pl[1]) * i64::from(p[1])
        + i64::from(pl[2]) * i64::from(p[2])
        + i64::from(pl[3])
}

/// Three cubes as one model: at +x (material 1), +y (material 2) and +z
/// (material 3), 0.4 m each, 1 m from the middle. For telling left from
/// right and up from down.
pub const fn cube_verts() -> [[i16; 3]; 24] {
    let mut v = [[0i16; 3]; 24];
    let mid = [[256, 0, 0], [0, 256, 0], [0, 0, 256]];
    let mut c = 0;
    while c < 3 {
        let mut k = 0;
        while k < 8 {
            let mut a = 0;
            while a < 3 {
                let s = if k & (1 << a) != 0 { 50 } else { -50 };
                v[8 * c + k][a] = (mid[c][a] + s) as i16;
                a += 1;
            }
            k += 1;
        }
        c += 1;
    }
    v
}

pub const fn cube_raw() -> [RawFace; 36] {
    // Corner k has x from bit 0, y from bit 1, z from bit 2.
    const Q: [[u8; 4]; 6] = [
        [0, 2, 6, 4],
        [1, 3, 7, 5],
        [0, 1, 5, 4],
        [2, 3, 7, 6],
        [0, 1, 3, 2],
        [4, 5, 7, 6],
    ];
    let mut f = [RawFace {
        v: [0; 3],
        mat: 0,
        part: 0,
    }; 36];
    let mut c = 0;
    while c < 3 {
        let mut q = 0;
        while q < 6 {
            let o = 8 * c as u8;
            let s = Q[q];
            let mat = 1 + c as u8;
            f[12 * c + 2 * q] = RawFace {
                v: [o + s[0], o + s[1], o + s[2]],
                mat,
                part: c as u8,
            };
            f[12 * c + 2 * q + 1] = RawFace {
                v: [o + s[0], o + s[2], o + s[3]],
                mat,
                part: c as u8,
            };
            q += 1;
        }
        c += 1;
    }
    f
}

static CUBES_V: [[i16; 3]; 24] = cube_verts();
static CUBES_F: [Face; 36] = orient(&cube_verts(), &cube_raw());
static CUBES_N: [[i16; 3]; 36] = normals(&cube_verts(), &orient(&cube_verts(), &cube_raw()));
pub static CUBES: Model = Model {
    verts: &CUBES_V,
    faces: &CUBES_F,
    normals: &CUBES_N,
    radius: radius_of(&cube_verts()),
};

/// A lumpy stone: an octahedron with uneven corners (one part).
const STONE_VV: [[i16; 3]; 6] = [
    [300, 20, -10],
    [-260, -30, 15],
    [10, 280, 30],
    [-20, -310, -25],
    [15, -10, 330],
    [-30, 25, -270],
];
const STONE_RAW: [RawFace; 8] = {
    const T: [[u8; 3]; 8] = [
        [0, 2, 4],
        [2, 1, 4],
        [1, 3, 4],
        [3, 0, 4],
        [2, 0, 5],
        [1, 2, 5],
        [3, 1, 5],
        [0, 3, 5],
    ];
    let mut f = [RawFace {
        v: [0; 3],
        mat: 0,
        part: 0,
    }; 8];
    let mut i = 0;
    while i < 8 {
        f[i] = RawFace {
            v: T[i],
            mat: 1 + (i % 3) as u8,
            part: 0,
        };
        i += 1;
    }
    f
};
static STONE_V: [[i16; 3]; 6] = STONE_VV;
static STONE_F: [Face; 8] = orient(&STONE_VV, &STONE_RAW);
static STONE_N: [[i16; 3]; 8] = normals(&STONE_VV, &orient(&STONE_VV, &STONE_RAW));
pub static STONE: Model = Model {
    verts: &STONE_V,
    faces: &STONE_F,
    normals: &STONE_N,
    radius: radius_of(&STONE_VV),
};

/// Flat colours: palette number = material (no light, no fog).
pub fn flat() -> Shading {
    Shading {
        mats: 8,
        shades: 1,
        light: [0, 0, ONE14],
        glow: 0xFF,
        fog_from: 0,
        fog_to: 0,
        fog_shades: 0,
        clear: 0,
        dark_side: 0,
    }
}

/// Light from above, the dark side three shades down, fog from 4 m to
/// 30 m (3 shades).
pub fn lit() -> Shading {
    Shading {
        mats: 4,
        shades: 8,
        light: [7373, -9011, 11469],
        glow: 1 << 3,
        fog_from: 4 * 256,
        fog_to: 30 * 256,
        fog_shades: 3,
        clear: 0,
        dark_side: 4,
    }
}
