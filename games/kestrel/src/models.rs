//! The 3D models, as small tables: corners in 1/256 m, faces as three
//! corner numbers with a material and a part. Every face is turned to
//! look outwards while compiling ([`poly::orient`]), so the tables need
//! no care about the order of corners. Models look along +z (their
//! nose), with +x to their right and +y up.
//!
//! | Model | Corners | Faces | What |
//! |---|---|---|---|
//! | [`SHIP`] | 32 | 48 | the KESTREL: body, two swept wings, a fin, engine glow |
//! | [`ROCK_LO`] | 6 | 8 | a far rock (a lumpy eight-sided stone) |
//! | [`ROCK_A`], [`ROCK_B`] | 12 | 20 | rocks (lumpy twenty-sided stones) |
//! | [`ROCK_HI`] | 42 | 80 | a near rock in HIGH detail |
//! | [`GATE`] | 32 | 64 | a beacon gate: an eight-sided ring with lights front and back |
//! | [`WRECK`] | 22 | 32 | a broken hauler: hull, bridge, a torn plate |
//! | [`POD`] | 12 | 20 | a loose cargo pod (six-sided, lying along x) |
//! | [`CELL`] | 6 | 8 | a time cell (glowing crystal) |
//! | [`STATION`] | 24 | 40 | HALDEN station: a drum and its hub |

use crate::art::mat;
use crate::fixed::isqrt;
use crate::poly::{Model, RawFace, normals, orient, radius_of};

const fn rf(a: u8, b: u8, c: u8, mat: u8, part: u8) -> RawFace {
    RawFace {
        v: [a, b, c],
        mat,
        part,
    }
}

/// Builds a model's statics from a corner table and raw faces.
macro_rules! model {
    ($name:ident, $verts:ident, $raw:ident) => {
        pub static $name: Model = Model {
            verts: &$verts,
            faces: &orient(&$verts, &$raw),
            normals: &normals(&$verts, &orient(&$verts, &$raw)),
            radius: radius_of(&$verts),
        };
    };
}

// ---- The KESTREL ------------------------------------------------------------------

const SHIP_V: [[i16; 3]; 32] = [
    // Body: nose, a ring of six round the middle, a smaller ring of six
    // at the back, the engine's middle.
    [0, 0, 430],
    [0, 96, -20],
    [118, 40, -50],
    [96, -50, -70],
    [0, -66, -70],
    [-96, -50, -70],
    [-118, 40, -50],
    [0, 62, -320],
    [76, 26, -320],
    [62, -32, -320],
    [0, -42, -320],
    [-62, -32, -320],
    [-76, 26, -320],
    [0, 8, -362],
    // Right wing: root front, root back, tip; top then bottom.
    [100, 8, 30],
    [80, 8, -280],
    [470, -36, -330],
    [100, -20, 30],
    [80, -20, -280],
    [470, -44, -330],
    // Left wing.
    [-100, 8, 30],
    [-80, 8, -280],
    [-470, -36, -330],
    [-100, -20, 30],
    [-80, -20, -280],
    [-470, -44, -330],
    // Fin.
    [10, 70, -160],
    [10, 60, -330],
    [3, 230, -385],
    [-10, 70, -160],
    [-10, 60, -330],
    [-3, 230, -385],
];

const SHIP_RAW: [RawFace; 48] = {
    use mat::*;
    let mut f = [rf(0, 0, 0, 0, 0); 48];
    let mut n = 0;
    // Nose cone to the middle ring: the upper two faces are the canopy.
    let mut i = 0;
    while i < 6 {
        let a = 1 + i as u8;
        let b = 1 + ((i + 1) % 6) as u8;
        let m = if i == 0 || i == 5 { CANOPY } else { HULL };
        f[n] = rf(0, a, b, m, 0);
        n += 1;
        i += 1;
    }
    // The middle ring to the back ring.
    i = 0;
    while i < 6 {
        let a = 1 + i as u8;
        let b = 1 + ((i + 1) % 6) as u8;
        let m = if i == 2 || i == 3 { HULL2 } else { HULL };
        f[n] = rf(a, b, b + 6, m, 0);
        f[n + 1] = rf(a, b + 6, a + 6, m, 0);
        n += 2;
        i += 1;
    }
    // The engine: the back ring to its middle.
    i = 0;
    while i < 6 {
        let a = 7 + i as u8;
        let b = 7 + ((i + 1) % 6) as u8;
        f[n] = rf(13, a, b, ENGINE, 0);
        n += 1;
        i += 1;
    }
    // Wings and fin: thin three-sided slabs (8 faces each).
    let mut p = 0;
    while p < 3 {
        let o = 14 + 6 * p as u8;
        let (top, edge) = if p == 2 {
            (HULL2, HULL2)
        } else {
            (WING, WING2)
        };
        let part = 1 + p as u8;
        f[n] = rf(o, o + 1, o + 2, top, part);
        f[n + 1] = rf(o + 3, o + 4, o + 5, HULL2, part);
        f[n + 2] = rf(o, o + 2, o + 5, edge, part);
        f[n + 3] = rf(o, o + 5, o + 3, edge, part);
        f[n + 4] = rf(o + 1, o + 2, o + 5, edge, part);
        f[n + 5] = rf(o + 1, o + 5, o + 4, edge, part);
        f[n + 6] = rf(o, o + 1, o + 4, edge, part);
        f[n + 7] = rf(o, o + 4, o + 3, edge, part);
        n += 8;
        p += 1;
    }
    assert!(n == 48);
    f
};
model!(SHIP, SHIP_V, SHIP_RAW);

// ---- Rocks ------------------------------------------------------------------------

/// An icosahedron of radius about 1 m (256): 1 and the golden ratio
/// scaled to 135 and 218.
const ICO_V: [[i16; 3]; 12] = [
    [-135, 218, 0],
    [135, 218, 0],
    [-135, -218, 0],
    [135, -218, 0],
    [0, -135, 218],
    [0, 135, 218],
    [0, -135, -218],
    [0, 135, -218],
    [218, 0, -135],
    [218, 0, 135],
    [-218, 0, -135],
    [-218, 0, 135],
];

const ICO_F: [[u8; 3]; 20] = [
    [0, 11, 5],
    [0, 5, 1],
    [0, 1, 7],
    [0, 7, 10],
    [0, 10, 11],
    [1, 5, 9],
    [5, 11, 4],
    [11, 10, 2],
    [10, 7, 6],
    [7, 1, 8],
    [3, 9, 4],
    [3, 4, 2],
    [3, 2, 6],
    [3, 6, 8],
    [3, 8, 9],
    [4, 9, 5],
    [2, 4, 11],
    [6, 2, 10],
    [8, 6, 7],
    [9, 8, 1],
];

/// Each corner pushed in or out by a factor from the dice (1/256),
/// between `lo` and `lo + span`.
const fn lumpy<const N: usize>(v: [[i16; 3]; N], seed: u32, lo: i32, span: u32) -> [[i16; 3]; N] {
    let mut out = v;
    let mut i = 0;
    while i < N {
        let k = lo + (crate::fixed::hash(seed, i as u32) % span) as i32;
        out[i] = [
            (v[i][0] as i32 * k / 256) as i16,
            (v[i][1] as i32 * k / 256) as i16,
            (v[i][2] as i32 * k / 256) as i16,
        ];
        i += 1;
    }
    out
}

const fn faces_of<const N: usize>(f: [[u8; 3]; N], mat: u8, alt: u8, every: usize) -> [RawFace; N] {
    let mut out = [rf(0, 0, 0, 0, 0); N];
    let mut i = 0;
    while i < N {
        let m = if every > 0 && i % every == 0 {
            alt
        } else {
            mat
        };
        out[i] = rf(f[i][0], f[i][1], f[i][2], m, 0);
        i += 1;
    }
    out
}

const ROCK_A_V: [[i16; 3]; 12] = lumpy(ICO_V, 11, 205, 70);
const ROCK_A_RAW: [RawFace; 20] = faces_of(ICO_F, mat::ROCK1, mat::ROCK2, 3);
model!(ROCK_A, ROCK_A_V, ROCK_A_RAW);

const ROCK_B_V: [[i16; 3]; 12] = lumpy(ICO_V, 29, 195, 80);
const ROCK_B_RAW: [RawFace; 20] = faces_of(ICO_F, mat::ICE, mat::ROCK2, 4);
model!(ROCK_B, ROCK_B_V, ROCK_B_RAW);

const OCT_V: [[i16; 3]; 6] = [
    [256, 0, 0],
    [-256, 0, 0],
    [0, 256, 0],
    [0, -256, 0],
    [0, 0, 256],
    [0, 0, -256],
];
const OCT_F: [[u8; 3]; 8] = [
    [0, 2, 4],
    [2, 1, 4],
    [1, 3, 4],
    [3, 0, 4],
    [2, 0, 5],
    [1, 2, 5],
    [3, 1, 5],
    [0, 3, 5],
];
const ROCK_LO_V: [[i16; 3]; 6] = lumpy(OCT_V, 5, 215, 60);
const ROCK_LO_RAW: [RawFace; 8] = faces_of(OCT_F, mat::ROCK1, mat::ROCK2, 2);
model!(ROCK_LO, ROCK_LO_V, ROCK_LO_RAW);

/// The icosahedron with every face cut in four (42 corners, 80 faces),
/// the new corners pushed out to the ball.
const fn subdivide() -> ([[i16; 3]; 42], [[u8; 3]; 80]) {
    let mut v = [[0i16; 3]; 42];
    let mut i = 0;
    while i < 12 {
        v[i] = ICO_V[i];
        i += 1;
    }
    let mut nv = 12;
    // Middle corner of each edge, found again by its two ends.
    let mut edges = [[0u8; 3]; 30];
    let mut ne = 0;
    let mut f = [[0u8; 3]; 80];
    let mut nf = 0;
    let mut k = 0;
    while k < 20 {
        let t = ICO_F[k];
        let mut mid = [0u8; 3];
        let mut e = 0;
        while e < 3 {
            let a = t[e];
            let b = t[(e + 1) % 3];
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            let mut found = 255u8;
            let mut j = 0;
            while j < ne {
                if edges[j][0] == lo && edges[j][1] == hi {
                    found = edges[j][2];
                }
                j += 1;
            }
            if found == 255 {
                let pa = ICO_V[a as usize];
                let pb = ICO_V[b as usize];
                let m = [
                    (pa[0] as i64 + pb[0] as i64) / 2,
                    (pa[1] as i64 + pb[1] as i64) / 2,
                    (pa[2] as i64 + pb[2] as i64) / 2,
                ];
                let len = isqrt((m[0] * m[0] + m[1] * m[1] + m[2] * m[2]) as u64) as i64;
                v[nv] = [
                    (m[0] * 256 / len) as i16,
                    (m[1] * 256 / len) as i16,
                    (m[2] * 256 / len) as i16,
                ];
                found = nv as u8;
                edges[ne] = [lo, hi, found];
                ne += 1;
                nv += 1;
            }
            mid[e] = found;
            e += 1;
        }
        // mid[0] is a-b, mid[1] b-c, mid[2] c-a.
        f[nf] = [t[0], mid[0], mid[2]];
        f[nf + 1] = [t[1], mid[1], mid[0]];
        f[nf + 2] = [t[2], mid[2], mid[1]];
        f[nf + 3] = [mid[0], mid[1], mid[2]];
        nf += 4;
        k += 1;
    }
    assert!(nv == 42 && nf == 80 && ne == 30);
    (v, f)
}

const GEO: ([[i16; 3]; 42], [[u8; 3]; 80]) = subdivide();
const ROCK_HI_V: [[i16; 3]; 42] = lumpy(GEO.0, 11, 215, 50);
const ROCK_HI_RAW: [RawFace; 80] = faces_of(GEO.1, mat::ROCK1, mat::ROCK2, 3);
model!(ROCK_HI, ROCK_HI_V, ROCK_HI_RAW);

// ---- The gate ---------------------------------------------------------------------

/// Gate sizes (1/256 m): the ring's inner and outer radius, and half
/// its thickness. Inner 3.4 m, outer 4.3 m.
pub const GATE_INNER: i32 = 870;
pub const GATE_OUTER: i32 = 1100;
const GATE_HALF: i32 = 64;

/// cos and sin of 22.5 + 45k degrees, in 1/256: the ring is flat on top.
const OCT_DIR: [[i32; 2]; 8] = [
    [237, 98],
    [98, 237],
    [-98, 237],
    [-237, 98],
    [-237, -98],
    [-98, -237],
    [98, -237],
    [237, -98],
];

const fn gate_verts() -> [[i16; 3]; 32] {
    let mut v = [[0i16; 3]; 32];
    let mut k = 0;
    while k < 8 {
        let d = OCT_DIR[k];
        let r = [GATE_INNER, GATE_OUTER];
        let z = [-GATE_HALF, GATE_HALF];
        // 4k: inner front, 4k+1: outer front, 4k+2: inner back, 4k+3: outer back.
        let mut j = 0;
        while j < 4 {
            let rr = r[j % 2];
            v[4 * k + j] = [
                (d[0] * rr / 256) as i16,
                (d[1] * rr / 256) as i16,
                z[j / 2] as i16,
            ];
            j += 1;
        }
        k += 1;
    }
    v
}

const GATE_RAW: [RawFace; 64] = {
    use mat::*;
    let mut f = [rf(0, 0, 0, 0, 0); 64];
    let mut k = 0;
    while k < 8 {
        let a = 4 * k as u8;
        let b = 4 * ((k + 1) % 8) as u8;
        let p = k as u8;
        let n = 8 * k;
        // Front and back: the lights.
        f[n] = rf(a, a + 1, b + 1, GATE_LIGHT, p);
        f[n + 1] = rf(a, b + 1, b, GATE_LIGHT, p);
        f[n + 2] = rf(a + 2, a + 3, b + 3, GATE_LIGHT, p);
        f[n + 3] = rf(a + 2, b + 3, b + 2, GATE_LIGHT, p);
        // Outside and inside: the frame.
        f[n + 4] = rf(a + 1, a + 3, b + 3, HULL2, p);
        f[n + 5] = rf(a + 1, b + 3, b + 1, HULL2, p);
        f[n + 6] = rf(a, a + 2, b + 2, HULL, p);
        f[n + 7] = rf(a, b + 2, b, HULL, p);
        k += 1;
    }
    f
};
const GATE_V: [[i16; 3]; 32] = gate_verts();
model!(GATE, GATE_V, GATE_RAW);

// ---- Wreck, pod, cell, station ----------------------------------------------------------

/// The wreck's collision box (half sizes, 1/256 m), round its origin,
/// lying along z: x, y down, y up, z.
pub const WRECK_BOX: [i32; 4] = [440, 330, 640, 1340];

const WRECK_V: [[i16; 3]; 22] = [
    // Hull: a long box, the front end slanted.
    [-410, -300, -1300],
    [410, -300, -1300],
    [410, 300, -1300],
    [-410, 300, -1300],
    [-410, -300, 1100],
    [410, -300, 1100],
    [380, 260, 1300],
    [-380, 260, 1300],
    // Bridge: a smaller box on top, set back, leaning.
    [-230, 300, -900],
    [230, 300, -900],
    [200, 620, -760],
    [-200, 620, -760],
    [-230, 300, -200],
    [230, 300, -200],
    [200, 600, -300],
    [-200, 600, -300],
    // A torn plate sticking out to the right.
    [410, 80, 200],
    [410, 40, 760],
    [860, -150, 520],
    [400, 40, 200],
    [400, 0, 760],
    [850, -190, 520],
];

const fn box_faces(o: u8, m_side: u8, m_top: u8, m_end: u8, part: u8) -> [RawFace; 12] {
    // Corners o..o+3: back end (z-), o+4..o+7 front end; 0 1 2 3 go
    // round (-x-y, +x-y, +x+y, -x+y).
    [
        rf(o, o + 1, o + 2, m_end, part),
        rf(o, o + 2, o + 3, m_end, part),
        rf(o + 4, o + 5, o + 6, m_end, part),
        rf(o + 4, o + 6, o + 7, m_end, part),
        rf(o, o + 1, o + 5, m_side, part),
        rf(o, o + 5, o + 4, m_side, part),
        rf(o + 3, o + 2, o + 6, m_top, part),
        rf(o + 3, o + 6, o + 7, m_top, part),
        rf(o, o + 3, o + 7, m_side, part),
        rf(o, o + 7, o + 4, m_side, part),
        rf(o + 1, o + 2, o + 6, m_side, part),
        rf(o + 1, o + 6, o + 5, m_side, part),
    ]
}

const WRECK_RAW: [RawFace; 32] = {
    use mat::*;
    let mut f = [rf(0, 0, 0, 0, 0); 32];
    let hull = box_faces(0, RUST, METAL, METAL, 0);
    let bridge = box_faces(8, METAL, RUST, WARN, 1);
    let mut i = 0;
    while i < 12 {
        f[i] = hull[i];
        f[12 + i] = bridge[i];
        i += 1;
    }
    // The plate: a thin three-sided slab.
    let o = 16u8;
    f[24] = rf(o, o + 1, o + 2, RUST, 2);
    f[25] = rf(o + 3, o + 4, o + 5, METAL, 2);
    f[26] = rf(o, o + 2, o + 5, RUST, 2);
    f[27] = rf(o, o + 5, o + 3, RUST, 2);
    f[28] = rf(o + 1, o + 2, o + 5, RUST, 2);
    f[29] = rf(o + 1, o + 5, o + 4, RUST, 2);
    f[30] = rf(o, o + 1, o + 4, RUST, 2);
    f[31] = rf(o, o + 4, o + 3, RUST, 2);
    f
};
model!(WRECK, WRECK_V, WRECK_RAW);

/// A six-sided prism along x: radius `r`, half length `h` (1/256 m),
/// round the point (0, 0, `z`).
const fn hex_prism(r: i32, h: i32, z: i32, along_z: bool) -> [[i16; 3]; 12] {
    // cos, sin of 0, 60, ... degrees in 1/256.
    const D: [[i32; 2]; 6] = [
        [256, 0],
        [128, 222],
        [-128, 222],
        [-256, 0],
        [-128, -222],
        [128, -222],
    ];
    let mut v = [[0i16; 3]; 12];
    let mut k = 0;
    while k < 6 {
        let a = D[k][0] * r / 256;
        let b = D[k][1] * r / 256;
        if along_z {
            v[k] = [a as i16, b as i16, (z - h) as i16];
            v[6 + k] = [a as i16, b as i16, (z + h) as i16];
        } else {
            v[k] = [-h as i16, a as i16, b as i16];
            v[6 + k] = [h as i16, a as i16, b as i16];
        }
        k += 1;
    }
    v
}

const fn hex_faces(o: u8, side: u8, side2: u8, cap: u8, part: u8) -> [RawFace; 20] {
    let mut f = [rf(0, 0, 0, 0, 0); 20];
    let mut k = 0;
    while k < 6 {
        let a = o + k as u8;
        let b = o + ((k + 1) % 6) as u8;
        let m = if k % 2 == 0 { side } else { side2 };
        f[2 * k] = rf(a, b, b + 6, m, part);
        f[2 * k + 1] = rf(a, b + 6, a + 6, m, part);
        k += 1;
    }
    let mut i = 0;
    while i < 4 {
        f[12 + i] = rf(o, o + 1 + i as u8, o + 2 + i as u8, cap, part);
        f[16 + i] = rf(o + 6, o + 7 + i as u8, o + 8 + i as u8, cap, part);
        i += 1;
    }
    f
}

const POD_V: [[i16; 3]; 12] = hex_prism(200, 260, 0, false);
const POD_RAW: [RawFace; 20] = hex_faces(0, mat::POD, mat::POD, mat::WARN, 0);
model!(POD, POD_V, POD_RAW);

const CELL_V: [[i16; 3]; 6] = [
    [90, 0, 0],
    [-90, 0, 0],
    [0, 160, 0],
    [0, -160, 0],
    [0, 0, 90],
    [0, 0, -90],
];
const CELL_RAW: [RawFace; 8] = faces_of(OCT_F, mat::CELL, mat::CELL, 0);
model!(CELL, CELL_V, CELL_RAW);

const fn station_verts() -> [[i16; 3]; 24] {
    let drum = hex_prism(2300, 900, 0, true);
    let hub = hex_prism(900, 700, -1500, true);
    let mut v = [[0i16; 3]; 24];
    let mut i = 0;
    while i < 12 {
        v[i] = drum[i];
        v[12 + i] = hub[i];
        i += 1;
    }
    v
}
const STATION_V: [[i16; 3]; 24] = station_verts();
const STATION_RAW: [RawFace; 40] = {
    use mat::*;
    let drum = hex_faces(0, STATION, HULL2, DOCK, 0);
    let hub = hex_faces(12, HULL2, STATION, WARN, 1);
    let mut f = [rf(0, 0, 0, 0, 0); 40];
    let mut i = 0;
    while i < 20 {
        f[i] = drum[i];
        f[20 + i] = hub[i];
        i += 1;
    }
    f
};
model!(STATION, STATION_V, STATION_RAW);

/// Every model, for the tests.
pub fn all() -> [(&'static str, &'static Model); 10] {
    [
        ("ship", &SHIP),
        ("rock-lo", &ROCK_LO),
        ("rock-a", &ROCK_A),
        ("rock-b", &ROCK_B),
        ("rock-hi", &ROCK_HI),
        ("gate", &GATE),
        ("wreck", &WRECK),
        ("pod", &POD),
        ("cell", &CELL),
        ("station", &STATION),
    ]
}
