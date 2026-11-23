//! Models: tables of corners and faces, made ready while compiling.

use crate::fixed::{ONE14, isqrt};

/// A face as written in a model table: three corners, a material, and
/// the convex part it belongs to (for turning its front outwards).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawFace {
    pub v: [u8; 3],
    pub mat: u8,
    pub part: u8,
}

/// A face ready to draw: corners in front-face order, and a material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Face {
    pub v: [u8; 3],
    pub mat: u8,
}

/// A model: corners in 1/256 m, faces, and each face's direction (unit
/// length in 1/16384, made while compiling), and the radius of a ball
/// round the origin that holds every corner (1/256 m).
///
/// Corner numbers are bytes, so a model has at most 256 corners; the
/// renderer draws only as many as its corner list holds (`V`).
#[derive(Clone, Copy, Debug)]
pub struct Model {
    pub verts: &'static [[i16; 3]],
    pub faces: &'static [Face],
    pub normals: &'static [[i16; 3]],
    pub radius: i32,
}

/// The face's direction before any turning: `(b - a) x (c - a)`. The
/// front of a face is the side this points to.
pub const fn cross_of(v: &[[i16; 3]], f: [u8; 3]) -> [i64; 3] {
    let a = v[f[0] as usize];
    let b = v[f[1] as usize];
    let c = v[f[2] as usize];
    let u = [
        (b[0] - a[0]) as i64,
        (b[1] - a[1]) as i64,
        (b[2] - a[2]) as i64,
    ];
    let w = [
        (c[0] - a[0]) as i64,
        (c[1] - a[1]) as i64,
        (c[2] - a[2]) as i64,
    ];
    [
        u[1] * w[2] - u[2] * w[1],
        u[2] * w[0] - u[0] * w[2],
        u[0] * w[1] - u[1] * w[0],
    ]
}

/// The middle of a part: the average of its faces' corners.
pub const fn part_middle(v: &[[i16; 3]], faces: &[RawFace], part: u8) -> [i64; 3] {
    let mut s = [0i64; 3];
    let mut n = 0i64;
    let mut i = 0;
    while i < faces.len() {
        if faces[i].part == part {
            let mut k = 0;
            while k < 3 {
                let p = v[faces[i].v[k] as usize];
                s[0] += p[0] as i64;
                s[1] += p[1] as i64;
                s[2] += p[2] as i64;
                k += 1;
            }
            n += 3;
        }
        i += 1;
    }
    if n == 0 {
        return s;
    }
    [s[0] / n, s[1] / n, s[2] / n]
}

/// Turns every face so that its front looks away from the middle of its
/// part (two corners swapped where needed), judged from the face's
/// first corner (KESTREL's rule). Runs while compiling.
pub const fn orient<const N: usize>(v: &[[i16; 3]], raw: &[RawFace; N]) -> [Face; N] {
    orient_by(v, raw, false)
}

/// As [`orient`], but judged from the face's own middle (CAMBER's rule):
/// right also for thin parts where a corner lies near the part's middle.
pub const fn orient_by_middle<const N: usize>(v: &[[i16; 3]], raw: &[RawFace; N]) -> [Face; N] {
    orient_by(v, raw, true)
}

const fn orient_by<const N: usize>(v: &[[i16; 3]], raw: &[RawFace; N], middle: bool) -> [Face; N] {
    let mut out = [Face { v: [0; 3], mat: 0 }; N];
    let mut i = 0;
    while i < N {
        let f = raw[i];
        let m = part_middle(v, raw, f.part);
        let n = cross_of(v, f.v);
        let a = v[f.v[0] as usize];
        let out_dir = if middle {
            let b = v[f.v[1] as usize];
            let c = v[f.v[2] as usize];
            [
                (a[0] as i64 + b[0] as i64 + c[0] as i64) - 3 * m[0],
                (a[1] as i64 + b[1] as i64 + c[1] as i64) - 3 * m[1],
                (a[2] as i64 + b[2] as i64 + c[2] as i64) - 3 * m[2],
            ]
        } else {
            [a[0] as i64 - m[0], a[1] as i64 - m[1], a[2] as i64 - m[2]]
        };
        let dot = n[0] * out_dir[0] + n[1] * out_dir[1] + n[2] * out_dir[2];
        out[i] = if dot < 0 {
            Face {
                v: [f.v[0], f.v[2], f.v[1]],
                mat: f.mat,
            }
        } else {
            Face { v: f.v, mat: f.mat }
        };
        i += 1;
    }
    out
}

/// Each face's direction, one long (16384). Runs while compiling.
pub const fn normals<const N: usize>(v: &[[i16; 3]], faces: &[Face; N]) -> [[i16; 3]; N] {
    let mut out = [[0i16; 3]; N];
    let mut i = 0;
    while i < N {
        let n = cross_of(v, faces[i].v);
        let len = isqrt((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]) as u64) as i64;
        if len > 0 {
            out[i] = [
                (n[0] * ONE14 as i64 / len) as i16,
                (n[1] * ONE14 as i64 / len) as i16,
                (n[2] * ONE14 as i64 / len) as i16,
            ];
        }
        i += 1;
    }
    out
}

/// The radius of the smallest ball round the origin holding every
/// corner (rounded up). Runs while compiling.
pub const fn radius_of(v: &[[i16; 3]]) -> i32 {
    let mut best = 0u64;
    let mut i = 0;
    while i < v.len() {
        let p = v[i];
        let d = (p[0] as i64 * p[0] as i64 + p[1] as i64 * p[1] as i64 + p[2] as i64 * p[2] as i64)
            as u64;
        if d > best {
            best = d;
        }
        i += 1;
    }
    isqrt(best) as i32 + 1
}
