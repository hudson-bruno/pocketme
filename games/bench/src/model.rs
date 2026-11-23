//! The three balls the 3D scenes are made of, as tables made while
//! compiling (MERCATOR's sphere maker). Radius 1 m (256 units). A ball
//! with `r` bands of `s` pieces has `2 + (r - 1) * s` corners and
//! `2 * s * (r - 1)` triangles.
//!
//! | Model | Bands x pieces | Corners | Triangles |
//! |---|---|---|---|
//! | [`BALL12`] | 2 x 6 | 8 | 12 |
//! | [`BALL32`] | 3 x 8 | 18 | 32 |
//! | [`BALL96`] | 5 x 12 | 50 | 96 |

use crate::fixed::{ccos, csin};
use crate::poly::{MATS, Model, RawFace, normals, orient, radius_of};

/// The balls' radius in their tables (1 m).
pub const BALL: i32 = 256;

const fn rf(a: u8, b: u8, c: u8, mat: u8) -> RawFace {
    RawFace {
        v: [a, b, c],
        mat,
        part: 0,
    }
}

/// Corner `j` of band edge `i` (1..rings).
const fn ring(i: usize, j: usize, segs: usize) -> u8 {
    (1 + (i - 1) * segs + j % segs) as u8
}

const fn sphere_verts<const V: usize>(rings: usize, segs: usize) -> [[i16; 3]; V] {
    let mut v = [[0i16; 3]; V];
    v[0] = [0, BALL as i16, 0];
    v[V - 1] = [0, -BALL as i16, 0];
    let mut i = 1;
    while i < rings {
        let lat = (i * 32768 / rings) as u16;
        let y = ccos(lat) * BALL / 16384;
        let rr = csin(lat) * BALL / 16384;
        let mut j = 0;
        while j < segs {
            let lon = (j * 65536 / segs) as u16;
            v[ring(i, j, segs) as usize] = [
                (csin(lon) * rr / 16384) as i16,
                y as i16,
                (ccos(lon) * rr / 16384) as i16,
            ];
            j += 1;
        }
        i += 1;
    }
    v
}

/// A checker of the four materials: band and piece.
const fn mat_of(i: usize, j: usize) -> u8 {
    ((i % 2) * 2 + j % 2) as u8 % MATS
}

const fn sphere_faces<const F: usize>(rings: usize, segs: usize) -> [RawFace; F] {
    let mut f = [rf(0, 0, 0, 0); F];
    let bottom = (2 + (rings - 1) * segs - 1) as u8;
    let mut n = 0;
    let mut j = 0;
    while j < segs {
        f[n] = rf(0, ring(1, j, segs), ring(1, j + 1, segs), mat_of(0, j));
        f[n + 1] = rf(
            bottom,
            ring(rings - 1, j, segs),
            ring(rings - 1, j + 1, segs),
            mat_of(rings - 1, j),
        );
        n += 2;
        j += 1;
    }
    let mut i = 1;
    while i < rings - 1 {
        let mut j = 0;
        while j < segs {
            let m = mat_of(i, j);
            let (a, b) = (ring(i, j, segs), ring(i, j + 1, segs));
            let (c, d) = (ring(i + 1, j, segs), ring(i + 1, j + 1, segs));
            f[n] = rf(a, c, b, m);
            f[n + 1] = rf(b, c, d, m);
            n += 2;
            j += 1;
        }
        i += 1;
    }
    f
}

/// Builds a model's statics from a corner table and raw faces.
macro_rules! model {
    ($name:ident, $verts:expr, $raw:expr) => {
        pub static $name: Model = Model {
            verts: &$verts,
            faces: &orient(&$verts, &$raw),
            normals: &normals(&$verts, &orient(&$verts, &$raw)),
            radius: radius_of(&$verts),
        };
    };
}

model!(BALL12, sphere_verts::<8>(2, 6), sphere_faces::<12>(2, 6));
model!(BALL32, sphere_verts::<18>(3, 8), sphere_faces::<32>(3, 8));
model!(BALL96, sphere_verts::<50>(5, 12), sphere_faces::<96>(5, 12));
