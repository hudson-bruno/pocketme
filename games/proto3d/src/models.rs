//! The fixed models, made while compiling: the hero in six poses, the
//! gold nut, trees, bushes, grass tufts, the rock cones under the
//! islands and the far islets. Sizes are written in cm and turned into
//! game units (1/256 m) here, the same way as every record.
//!
//! Each model's origin is where it stands (the feet, the foot of the
//! trunk, the top of a cone), except the nut, whose origin is its middle.

use game_3d::{Face, Model, RawFace, normals, orient_by_middle, radius_of, sin_fine};

use crate::island::mat as im;

/// The materials (palette number = shade * [`MATS`] + material). The
/// first ones are the records' (`island::mat`).
pub mod mat {
    pub use crate::island::mat::*;
    pub const GOLD: u8 = 7;
    /// Bushes (darker leaves).
    pub const LEAF2: u8 = 8;
    /// The hero's crimson top.
    pub const HERO: u8 = 13;
    /// The hero's white cap.
    pub const CAP: u8 = 14;
    pub const SKIN: u8 = 15;
    pub const PANTS: u8 = 16;
    /// Lighter rims on walkable tops, one per top material.
    pub const RIM_GRASS: u8 = 18;
    pub const RIM_STONE: u8 = 19;
    pub const RIM_SAND: u8 = 20;
    pub const RIM_BLOCK: u8 = 21;
}

/// The sides of a box, for [`Mb::boxm`]'s `skip`.
pub mod side {
    pub const LEFT: u8 = 1 << 0;
    pub const RIGHT: u8 = 1 << 1;
    pub const BOTTOM: u8 = 1 << 2;
    pub const TOP: u8 = 1 << 3;
    pub const BACK: u8 = 1 << 4;
    pub const FRONT: u8 = 1 << 5;
}

/// Materials in the lit part of the palette.
pub const MATS: u8 = 24;

/// cm to game units, exactly as `maths::unit(maths::cm(c))` (subs
/// first, then a shift that rounds down), so model corners and record
/// corners land on the same units.
pub const fn uc(c: i32) -> i32 {
    (c * 1024 / 25) >> 4
}

/// A model under construction: `V` corners and `F` faces, filled in
/// order. [`Mb::done`] stops the build if the counts are not exact.
#[derive(Clone, Copy)]
pub struct Mb<const V: usize, const F: usize> {
    pub v: [[i16; 3]; V],
    pub f: [RawFace; F],
    nv: usize,
    nf: usize,
    part: u8,
}

impl<const V: usize, const F: usize> Default for Mb<V, F> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const V: usize, const F: usize> Mb<V, F> {
    pub const fn new() -> Self {
        Mb {
            v: [[0; 3]; V],
            f: [RawFace {
                v: [0; 3],
                mat: 0,
                part: 0,
            }; F],
            nv: 0,
            nf: 0,
            part: 0,
        }
    }

    /// A corner in cm.
    const fn vert(&mut self, p: [i32; 3]) -> u8 {
        self.v[self.nv] = [uc(p[0]) as i16, uc(p[1]) as i16, uc(p[2]) as i16];
        self.nv += 1;
        (self.nv - 1) as u8
    }

    const fn tri(&mut self, a: u8, b: u8, c: u8, mat: u8) {
        self.f[self.nf] = RawFace {
            v: [a, b, c],
            mat,
            part: self.part,
        };
        self.nf += 1;
    }

    const fn quad(&mut self, a: u8, b: u8, c: u8, d: u8, mat: u8) {
        self.tri(a, b, c, mat);
        self.tri(a, c, d, mat);
    }

    /// A box from `lo` to `hi` (cm): `top` on its top, `side` elsewhere,
    /// leaving out the faces in `skip` (bits of [`side`]: faces always
    /// hidden inside another part or on the ground). 8 corners, 2 faces
    /// for each side kept.
    pub const fn boxm(mut self, lo: [i32; 3], hi: [i32; 3], top: u8, sd: u8, skip: u8) -> Self {
        let mut k = [0u8; 8];
        let mut i = 0;
        while i < 8 {
            let x = if i & 1 != 0 { hi[0] } else { lo[0] };
            let y = if i & 2 != 0 { hi[1] } else { lo[1] };
            let z = if i & 4 != 0 { hi[2] } else { lo[2] };
            k[i] = self.vert([x, y, z]);
            i += 1;
        }
        let quads = [
            [k[0], k[2], k[6], k[4]], // -x
            [k[1], k[3], k[7], k[5]], // +x
            [k[0], k[1], k[5], k[4]], // -y
            [k[2], k[3], k[7], k[6]], // +y
            [k[0], k[1], k[3], k[2]], // -z
            [k[4], k[5], k[7], k[6]], // +z
        ];
        let mut q = 0;
        while q < 6 {
            if skip & (1 << q) == 0 {
                let m = if q == 3 { top } else { sd };
                let c = quads[q];
                self.quad(c[0], c[1], c[2], c[3], m);
            }
            q += 1;
        }
        self.part += 1;
        self
    }

    /// A box with all six sides.
    pub const fn box2(self, lo: [i32; 3], hi: [i32; 3], top: u8, sd: u8) -> Self {
        self.boxm(lo, hi, top, sd, 0)
    }

    /// A cone of `n` sides: base ring of radius `r` round `c`, tip `h`
    /// above it (below when `h` is negative), all cm; `turn` turns the
    /// ring. With `base`, the ring is closed too. `n + 1` corners, `n`
    /// faces (`2n - 2` with the base).
    #[allow(clippy::too_many_arguments)]
    pub const fn cone(
        mut self,
        n: usize,
        c: [i32; 3],
        r: i32,
        h: i32,
        turn: u16,
        m: u8,
        base: bool,
    ) -> Self {
        let tip = self.vert([c[0], c[1] + h, c[2]]);
        let first = self.nv as u8;
        let mut k = 0;
        while k < n {
            let a = ((k * 65_536 / n) as u16).wrapping_add(turn);
            self.vert([
                c[0] + sin_fine(a) * r / 16_384,
                c[1],
                c[2] + sin_fine(a.wrapping_add(16_384)) * r / 16_384,
            ]);
            k += 1;
        }
        let mut k = 0;
        while k < n {
            let a = first + k as u8;
            let b = first + ((k + 1) % n) as u8;
            self.tri(tip, a, b, m);
            k += 1;
        }
        if base {
            let mut k = 1;
            while k + 1 < n {
                self.tri(first, first + k as u8, first + k as u8 + 1, m);
                k += 1;
            }
        }
        self.part += 1;
        self
    }

    /// An island's rock underside that meets the island's square bottom
    /// edge exactly: a square of corners on the axes at radius `sq`,
    /// `lift` cm above the origin (inside the island, so the rock and
    /// the dirt side overlap instead of meeting on one line), narrowing
    /// in `m1` to a ring of `n` sides, radius `r`, `h1` cm below the
    /// origin, then in `m2` to a point `h2` cm lower. Every ring edge
    /// goes to the square corner nearest it and every square edge to the
    /// ring corner nearest its middle, so the surface has no gap (a cone
    /// alone, round or not, leaves the island's corners open). `n + 5`
    /// corners, `2n + 4` faces.
    #[allow(clippy::too_many_arguments)]
    pub const fn underside(
        mut self,
        n: usize,
        sq: i32,
        lift: i32,
        r: i32,
        h1: i32,
        h2: i32,
        m1: u8,
        m2: u8,
    ) -> Self {
        let sq0 = self.nv as u8;
        let mut j = 0;
        while j < 4 {
            self.vert(round_at((j * 16_384) as u16, sq, lift));
            j += 1;
        }
        let first = self.nv as u8;
        let mut k = 0;
        while k < n {
            self.vert(round_at(ring_turn(k, n), r, -h1));
            k += 1;
        }
        let tip = self.vert([0, -h1 - h2, 0]);
        let mut k = 0;
        while k < n {
            // The square corner nearest this ring edge's middle.
            let mid = ring_turn(k, n).wrapping_add((32_768 / n) as u16);
            let c = (mid.wrapping_add(8_192) >> 14) as u8;
            self.tri(sq0 + c, first + k as u8, first + ((k + 1) % n) as u8, m1);
            k += 1;
        }
        let mut j = 0;
        while j < 4 {
            // The ring corner on this square edge's middle.
            self.tri(
                sq0 + j as u8,
                sq0 + ((j + 1) % 4) as u8,
                first + (j * n / 4) as u8,
                m1,
            );
            j += 1;
        }
        self.part += 1;
        let mut k = 0;
        while k < n {
            self.tri(tip, first + k as u8, first + ((k + 1) % n) as u8, m2);
            k += 1;
        }
        self.part += 1;
        self
    }

    /// An upright prism of `n` sides from `y0` to `y1` (cm), ring radius
    /// `r` round the y axis, turned by `turn`, without top or bottom; with
    /// `tip`, a low point at that height closes its top. `2n` corners
    /// (one more with the tip), `2n` faces (`3n` with the tip).
    #[allow(clippy::too_many_arguments)]
    pub const fn prism(
        mut self,
        n: usize,
        r: i32,
        y0: i32,
        y1: i32,
        turn: u16,
        m: u8,
        tip: Option<i32>,
    ) -> Self {
        let first = self.nv as u8;
        let mut k = 0;
        while k < n {
            let a = ((k * 65_536 / n) as u16).wrapping_add(turn);
            let (x, z) = (
                sin_fine(a) * r / 16_384,
                sin_fine(a.wrapping_add(16_384)) * r / 16_384,
            );
            self.vert([x, y0, z]);
            self.vert([x, y1, z]);
            k += 1;
        }
        let mut k = 0;
        while k < n {
            let a = first + 2 * k as u8;
            let b = first + 2 * ((k + 1) % n) as u8;
            self.quad(a, b, b + 1, a + 1, m);
            k += 1;
        }
        if let Some(h) = tip {
            let t = self.vert([0, h, 0]);
            let mut k = 0;
            while k < n {
                let a = first + 2 * k as u8 + 1;
                let b = first + 2 * ((k + 1) % n) as u8 + 1;
                self.tri(t, a, b, m);
                k += 1;
            }
        }
        self.part += 1;
        self
    }

    /// Checks that every corner and face was filled.
    pub const fn done(self) -> Self {
        assert!(
            self.nv == V && self.nf == F,
            "model: wrong corner or face count"
        );
        self
    }
}

/// The turn of [`Mb::underside`]'s ring corner `k` of `n`: the ring
/// starts on a square edge's middle (45 degrees), so the ring corner
/// nearest each middle is never a tie.
const fn ring_turn(k: usize, n: usize) -> u16 {
    (8_192 + k * 65_536 / n) as u16
}

/// The point at turn `a`, radius `r` round the y axis, height `y` (cm).
const fn round_at(a: u16, r: i32, y: i32) -> [i32; 3] {
    [
        sin_fine(a) * r / 16_384,
        y,
        sin_fine(a.wrapping_add(16_384)) * r / 16_384,
    ]
}

/// A [`Model`] from a builder, made while compiling.
macro_rules! model {
    ($(#[$m:meta])* $name:ident, $nv:expr, $nf:expr, $build:expr) => {
        $(#[$m])*
        pub static $name: Model = {
            const B: Mb<{ $nv }, { $nf }> = $build.done();
            const F: [Face; $nf] = orient_by_middle(&B.v, &B.f);
            const N: [[i16; 3]; $nf] = normals(&B.v, &F);
            const V: [[i16; 3]; $nv] = B.v;
            Model {
                verts: &V,
                faces: &F,
                normals: &N,
                radius: radius_of(&V),
            }
        };
    };
}

// ---------------------------------------------------------------- hero

/// The hero's poses: 0 standing, 1 and 2 running, 3 rising, 4 falling,
/// 5 squashed.
pub const POSES: usize = 6;
const HV: usize = 64;
const HF: usize = 86;
const LV: usize = 40;
const LF: usize = 52;

/// Squash: `(y num, y den, width num, width den)`.
type Squash = (i32, i32, i32, i32);

/// One pose's boxes (cm, feet at 0, facing +z). Full: two legs, body,
/// two arms, head, cap, brim (64 corners, 86 faces). `low`: one box for
/// both legs, no arms (40 corners, 52 faces). Faces always hidden (leg
/// tops in the body, the head's top in the cap, its bottom on the body,
/// the cap's bottom on the head) are left out. Every pose has the same
/// boxes in the same order, so one face table serves all poses.
const fn hero_pose<const V: usize, const F: usize>(pose: usize, low: bool) -> Mb<V, F> {
    // Run: legs and arms swing forward and back.
    let (sw_l, sw_r) = match pose {
        1 => (7, -7),
        2 => (-7, 7),
        3 => (4, -4),
        _ => (0, 0),
    };
    // Arms: x from the body out, y range.
    let (ax0, ax1, ay0, ay1) = match pose {
        3 => (13, 19, 70, 104), // jump: arms up
        4 => (13, 32, 58, 66),  // fall: arms out
        _ => (13, 19, 38, 70),
    };
    // Squash: lower and wider (y * 3/4, x and z * 9/8).
    let sq: Squash = if pose == 5 {
        (3, 4, 9, 8)
    } else {
        (1, 1, 1, 1)
    };
    let mut b = Mb::new();
    if low {
        b = hbox(b, [-11, 0, -5], [11, 28, 5], mat::PANTS, sq, side::TOP);
    } else {
        b = hbox(
            b,
            [2, 0, -5 + sw_l],
            [11, 28, 5 + sw_l],
            mat::PANTS,
            sq,
            side::TOP,
        );
        b = hbox(
            b,
            [-11, 0, -5 + sw_r],
            [-2, 28, 5 + sw_r],
            mat::PANTS,
            sq,
            side::TOP,
        );
    }
    b = hbox(b, [-13, 27, -8], [13, 74, 8], mat::HERO, sq, 0);
    if !low {
        b = hbox(
            b,
            [ax0, ay0, -4 + sw_r],
            [ax1, ay1, 4 + sw_r],
            mat::HERO,
            sq,
            0,
        );
        b = hbox(
            b,
            [-ax1, ay0, -4 + sw_l],
            [-ax0, ay1, 4 + sw_l],
            mat::HERO,
            sq,
            0,
        );
    }
    b = hbox(
        b,
        [-11, 74, -10],
        [11, 103, 10],
        mat::SKIN,
        sq,
        side::TOP | side::BOTTOM,
    );
    b = hbox(
        b,
        [-12, 101, -11],
        [12, 126, 11],
        mat::CAP,
        sq,
        side::BOTTOM,
    );
    hbox(b, [-10, 101, 11], [10, 106, 23], mat::CAP, sq, 0)
}

/// A hero box, squashed.
const fn hbox<const V: usize, const F: usize>(
    b: Mb<V, F>,
    lo: [i32; 3],
    hi: [i32; 3],
    m: u8,
    (yn, yd, wn, wd): Squash,
    skip: u8,
) -> Mb<V, F> {
    b.boxm(
        [lo[0] * wn / wd, lo[1] * yn / yd, lo[2] * wn / wd],
        [hi[0] * wn / wd, hi[1] * yn / yd, hi[2] * wn / wd],
        m,
        m,
        skip,
    )
}

const HERO_B: [Mb<HV, HF>; POSES] = [
    hero_pose(0, false).done(),
    hero_pose(1, false).done(),
    hero_pose(2, false).done(),
    hero_pose(3, false).done(),
    hero_pose(4, false).done(),
    hero_pose(5, false).done(),
];
// All poses are boxes in the same corner order, so the standing pose's
// face turns and directions hold for every pose.
static HERO_F: [Face; HF] = orient_by_middle(&HERO_B[0].v, &HERO_B[0].f);
static HERO_N: [[i16; 3]; HF] = normals(&HERO_B[0].v, &HERO_F);
static HERO_V: [[[i16; 3]; HV]; POSES] = [
    HERO_B[0].v,
    HERO_B[1].v,
    HERO_B[2].v,
    HERO_B[3].v,
    HERO_B[4].v,
    HERO_B[5].v,
];
/// The largest radius of the six poses.
const HERO_R: i32 = {
    let mut r = 0;
    let mut i = 0;
    while i < POSES {
        let ri = radius_of(&HERO_B[i].v);
        if ri > r {
            r = ri;
        }
        i += 1;
    }
    r
};

/// The hero at MID and HIGH, one model a pose. Feet at the origin,
/// facing +z.
pub static HERO: [Model; POSES] = {
    let mut out = [Model {
        verts: &HERO_V[0],
        faces: &HERO_F,
        normals: &HERO_N,
        radius: HERO_R,
    }; POSES];
    let mut i = 1;
    while i < POSES {
        out[i].verts = &HERO_V[i];
        i += 1;
    }
    out
};

/// The faces of the hero's parts, in [`hero_pose`]'s order: legs and
/// body, the arm on +x, the arm on -x (the head, cap and brim follow).
pub const HERO_BODY: core::ops::Range<usize> = 0..32;
pub const HERO_ARM_PX: core::ops::Range<usize> = 32..44;
pub const HERO_ARM_NX: core::ops::Range<usize> = 44..56;

/// MAX: a part of the hero in pose `k` (0..5, not the squash), the faces
/// `part` of [`HERO`]'s table (no bytes of its own: the same corners,
/// faces and directions).
pub fn hero_part(k: usize, part: core::ops::Range<usize>) -> Model {
    let k = k.min(POSES - 1);
    let part = part.start.min(HF)..part.end.min(HF);
    Model {
        verts: &HERO_V[k],
        faces: &HERO_F[part.clone()],
        normals: &HERO_N[part],
        radius: HERO_R,
    }
}

/// The faces of the cap's brim in [`HERO`] (the last box).
pub const HERO_BRIM: core::ops::Range<usize> = 74..86;

model!(
    /// MAX: the hero's rounder head and cap, drawn as their own object
    /// between his body and his nearer arm (poses 0 to 4; the squash
    /// keeps the HIGH hero). An eight-sided skin head (flat sides 11 cm
    /// from the middle, like the box's 11 and 10) and an eight-sided
    /// white cap with a low point on top (120 to 128 cm). The brim is
    /// [`HERO`]'s own ([`HERO_BRIM`]).
    HERO_HEAD, 33, 40,
    Mb::new()
        .prism(8, 12, 74, 103, 4_096, mat::SKIN, None)
        .prism(8, 13, 101, 120, 4_096, mat::CAP, Some(128))
);

const LOW_B: [Mb<LV, LF>; 2] = [hero_pose(0, true).done(), hero_pose(5, true).done()];
static LOW_F: [Face; LF] = orient_by_middle(&LOW_B[0].v, &LOW_B[0].f);
static LOW_N: [[i16; 3]; LF] = normals(&LOW_B[0].v, &LOW_F);
static LOW_V: [[[i16; 3]; LV]; 2] = [LOW_B[0].v, LOW_B[1].v];

/// The hero at LOW: no arms, the legs one box; standing (every pose
/// but the squash) and squashed.
pub static HERO_LOW: [Model; 2] = [
    Model {
        verts: &LOW_V[0],
        faces: &LOW_F,
        normals: &LOW_N,
        radius: radius_of(&LOW_V[0]),
    },
    Model {
        verts: &LOW_V[1],
        faces: &LOW_F,
        normals: &LOW_N,
        radius: radius_of(&LOW_V[0]),
    },
];

// --------------------------------------------------------- collectible

/// The six corners of the nut's face, ring `k` at angle `k * 60` deg.
const fn nut() -> Mb<12, 20> {
    let mut b = Mb::new();
    let (r, t) = (14, 4);
    let mut side = 0;
    while side < 2 {
        let z = if side == 0 { t } else { -t };
        let mut k = 0;
        while k < 6 {
            let a = (k * 65_536 / 6) as u16;
            b.vert([
                sin_fine(a) * r / 16_384,
                sin_fine(a.wrapping_add(16_384)) * r / 16_384,
                z,
            ]);
            k += 1;
        }
        side += 1;
    }
    let mut k = 0u8;
    while k < 6 {
        let n = (k + 1) % 6;
        b.quad(k, n, n + 6, k + 6, mat::GOLD);
        k += 1;
    }
    let mut k = 1u8;
    while k < 5 {
        b.tri(0, k, k + 1, mat::GOLD);
        b.tri(6, 6 + k, 7 + k, mat::GOLD);
        k += 1;
    }
    b
}

model!(
    /// The collectible: a solid gold six-sided nut, 28 cm across and 8
    /// cm thick, standing on edge. Origin in its middle.
    NUT, 12, 20, nut()
);

/// LOW: the nut's two faces without its six sides (`nut` makes the
/// sides' 12 triangles first): about 4 triangles a nut where the whole
/// one fills about 10 (repair round, defect 4).
pub static NUT_LOW: Model = Model {
    verts: NUT.verts,
    faces: NUT.faces.split_at(12).1,
    normals: NUT.normals.split_at(12).1,
    radius: NUT.radius,
};

// ------------------------------------------------------------- props

/// A trunk: four sides (its top is inside the leaves, its bottom on the
/// ground).
const fn trunk<const V: usize, const F: usize>() -> Mb<V, F> {
    Mb::new().boxm(
        [-12, 0, -12],
        [12, 60, 12],
        im::TRUNK,
        im::TRUNK,
        side::TOP | side::BOTTOM,
    )
}

model!(
    /// LOW tree: a four-sided leaf pyramid.
    TREE_LOW, 13, 12,
    trunk::<13, 12>().cone(4, [0, 50, 0], 75, 170, 0, im::LEAF, false)
);
model!(
    /// MID tree: a seven-sided leaf cone, closed below.
    TREE_MID, 16, 20,
    trunk::<16, 20>().cone(7, [0, 50, 0], 75, 170, 0, im::LEAF, true)
);
model!(
    /// HIGH tree: two eight-sided cones.
    TREE_HIGH, 26, 36,
    trunk::<26, 36>()
        .cone(8, [0, 45, 0], 85, 120, 0, im::LEAF, true)
        .cone(8, [0, 125, 0], 62, 115, 4096, im::LEAF, true)
);
model!(
    /// MAX tree: three ten-sided cones, the middle one turned half a
    /// side, inside the same 85 cm wide, 240 cm high outline as HIGH's.
    TREE_MAX, 41, 62,
    trunk::<41, 62>()
        .cone(10, [0, 45, 0], 85, 105, 0, im::LEAF, true)
        .cone(10, [0, 100, 0], 70, 95, 3_277, im::LEAF, true)
        .cone(10, [0, 150, 0], 50, 90, 0, im::LEAF, true)
);
model!(
    /// A bush: a dark leaf box, 44 cm wide, 34 cm high.
    BUSH, 8, 10,
    Mb::new().boxm([-22, 0, -22], [22, 34, 22], mat::LEAF2, mat::LEAF2, side::BOTTOM)
);

// ----------------------------------------------------------- creatures

impl<const V: usize, const F: usize> Mb<V, F> {
    /// Two flat eyes (`m`) on the front at `z` cm, `x0` to `x1` cm out
    /// from the middle line, `y0` to `y1` cm high, joined to the last
    /// part (so they face out from its middle).
    const fn eyes(mut self, z: i32, x: [i32; 2], y: [i32; 2], m: u8) -> Self {
        self.part -= 1;
        let mut s = 0;
        while s < 2 {
            let k = if s == 0 { 1 } else { -1 };
            let a = self.vert([k * x[0], y[0], z]);
            let b = self.vert([k * x[1], y[0], z]);
            let c = self.vert([k * x[1], y[1], z]);
            let d = self.vert([k * x[0], y[1], z]);
            self.quad(a, b, c, d, m);
            s += 1;
        }
        self.part += 1;
        self
    }
}

/// The burr's faces drawn at each detail (LOW the body, MID and up the
/// eyes too).
pub const BURR_FACES: [usize; 4] = [18, 22, 22, 22];

model!(
    /// The burr (the enemy): a slate six-sided dome 40 cm wide, 34 cm
    /// high, and two gold eyes. Feet at the origin, facing +z.
    BURR, 21, 22,
    Mb::new()
        .prism(6, 20, 0, 18, 5_461, mat::SLATE, Some(34))
        .eyes(18, [4, 9], [8, 14], mat::GOLD)
);

/// The tuftling's faces drawn at each detail (LOW the body and cap, MID
/// and up the eyes too).
pub const TUFT_FACES: [usize; 4] = [22, 26, 26, 26];

model!(
    /// The tuftling (the friend): a pale skin-coloured six-sided body 26 cm wide
    /// under a teal six-sided cap 48 cm wide (34 cm high in all) and two
    /// dark eyes. Feet at the origin, facing +z.
    TUFT, 27, 26,
    Mb::new()
        .prism(6, 13, 0, 20, 5_461, mat::SKIN, None)
        .cone(6, [0, 18, 0], 24, 16, 5_461, mat::SAIL, true)
        .eyes(12, [3, 7], [8, 13], mat::PANTS)
);

/// The first `n` faces of `m` (a creature at a lower detail).
pub fn first_faces(m: &'static Model, n: usize) -> Model {
    let n = n.min(m.faces.len());
    Model {
        verts: m.verts,
        faces: &m.faces[..n],
        normals: &m.normals[..n],
        radius: m.radius,
    }
}

/// A small fixed hash (spreads the tufts and MAX's meadow).
pub const fn hash(a: u32, b: u32) -> u32 {
    let mut h = 2_166_136_261u32 ^ a;
    h = h.wrapping_mul(16_777_619) ^ b;
    h = h.wrapping_mul(16_777_619);
    h ^ (h >> 15)
}

const fn tufts() -> Mb<48, 36> {
    let mut b = Mb::new();
    let mut k = 0;
    while k < 12 {
        let h = hash(7, k);
        let x = (h % 220) as i32 - 110;
        let z = ((h >> 12) % 220) as i32 - 110;
        b = b.cone(3, [x, 0, z], 12, 22, (h >> 16) as u16, im::GRASS2, false);
        k += 1;
    }
    b
}

model!(
    /// Grass tufts: twelve small three-sided pyramids in a 2.2 m patch.
    TUFTS, 48, 36, tufts()
);

/// How far (cm) the rock under an island starts above the island's
/// bottom, inside the island: the rock's top edge and the dirt side's
/// bottom edge then overlap, where meeting on one line left a dashed
/// line of sky between them (the two are rounded to pixels apart).
const LIFT: i32 = 2;

model!(
    /// The main island's rock underside at LOW: one four-sided point.
    /// The cones under the islands have no lid, so their top ring must
    /// stay inside the island's footprint (or the sky shows through the
    /// open top). Their record turns them by 45 degrees (yaw 32), so the
    /// four corners here, on the axes, land on the island's corners:
    /// 990 / 1.414 = 700 cm.
    CONE_MAIN_LOW, 5, 4,
    Mb::new().cone(4, [0, LIFT, 0], 990, -540 - LIFT, 0, im::ROCK, false)
);
model!(
    /// At MID: the island's square narrowing to an eight-sided ring 2 m
    /// down, then an eight-sided point, in two rock colours. The square's
    /// corners land on the island's corners as LOW's.
    CONE_MAIN_MID, 13, 20,
    Mb::new().underside(8, 990, LIFT, 560, 200, 360, im::ROCK, im::ROCK2)
);
model!(
    /// At HIGH and MAX: the same with twelve sides.
    CONE_MAIN_HIGH, 17, 28,
    Mb::new().underside(12, 990, LIFT, 560, 200, 360, im::ROCK, im::ROCK2)
);
model!(
    /// The islet's rock point (corners on the islet's corners after the
    /// record's 45 degree turn: 283 / 1.414 = 200 cm).
    CONE_ISLET, 5, 4,
    Mb::new().cone(4, [0, LIFT, 0], 283, -300 - LIFT, 0, im::ROCK, false)
);
model!(
    /// A far islet (the smallest; drawn bigger with the object's size):
    /// a grass box, a rock point and a small tree.
    FAR_ISLET, 28, 28,
    Mb::new()
        .boxm([-120, -30, -120], [120, 0, 120], im::GRASS, im::DIRT, side::BOTTOM)
        .cone(4, [0, -30, 0], 170, -240, 8192, im::ROCK, false)
        .boxm([-8, 0, -8], [8, 40, 8], im::TRUNK, im::TRUNK, side::TOP | side::BOTTOM)
        .cone(6, [0, 35, 0], 50, 110, 0, im::LEAF, false)
);

/// A tree for a detail level.
pub fn tree(level: u8) -> &'static Model {
    match level {
        0 => &TREE_LOW,
        1 => &TREE_MID,
        2 => &TREE_HIGH,
        _ => &TREE_MAX,
    }
}

/// The main island's underside for a detail level.
pub fn cone_main(level: u8) -> &'static Model {
    match level {
        0 => &CONE_MAIN_LOW,
        1 => &CONE_MAIN_MID,
        _ => &CONE_MAIN_HIGH,
    }
}

// ------------------------------------------------------------- areas

/// The height of the sign board's middle (cm), where the area's name is
/// written.
pub const SIGN_BOARD_MID: i16 = 115;

model!(
    /// A stub area's sign: a 1 m wooden post and a brass board, 1.2 m
    /// wide, 0.5 m high and 8 cm thick, facing along z.
    SIGN, 16, 20,
    Mb::new()
        .boxm([-5, 0, -5], [5, 100, 5], im::TRUNK, im::TRUNK, side::TOP | side::BOTTOM)
        .boxm([-60, 90, -4], [60, 140, 4], im::BRASS, im::BRASS, 0)
);

/// The MACHINES bob's size (cm): a cube.
pub const BOB_SIDE: i32 = 50;

model!(
    /// MACHINES: the bob while carried (a resting bob is its block
    /// record). A 0.5 m brass cube, origin in the middle of its bottom.
    BOB, 8, 12,
    Mb::new().box2([-25, 0, -25], [25, BOB_SIDE, 25], im::BRASS, im::BRASS)
);

/// The set-down marker: a 0.5 m square outline of four 8 cm strips, 1 cm
/// over the origin, tops only (8 triangles). Origin in the middle, on
/// the top it lies on.
///
/// Each strip is a lone quad wound so that its front looks up: a part
/// made of one flat quad has its middle in its own plane, so
/// `orient_by_middle` cannot judge it and keeps the winding as given
/// (built with `boxm` and its sides left out, half the strips looked down
/// and vanished from most camera turns).
const fn marker(m: u8) -> Mb<16, 8> {
    outline(m, 25, 8)
}

/// A square outline (see [`marker`]) of half side `h` and strip width
/// `w` (cm), 1 cm over the origin, tops only (8 triangles).
const fn outline(m: u8, h: i32, w: i32) -> Mb<16, 8> {
    let strips = [
        ([-h, -h], [h, -h + w]),
        ([-h, h - w], [h, h]),
        ([-h, -h + w], [-h + w, h - w]),
        ([h - w, -h + w], [h, h - w]),
    ];
    let mut b = Mb::new();
    let mut i = 0;
    while i < 4 {
        let (lo, hi) = strips[i];
        let a = b.vert([lo[0], 1, lo[1]]);
        let c = b.vert([lo[0], 1, hi[1]]);
        let d = b.vert([hi[0], 1, hi[1]]);
        let e = b.vert([hi[0], 1, lo[1]]);
        // (lo, lo) -> (lo, hi) -> (hi, hi): (0,0,dz) x (dx,0,dz) points up.
        b.quad(a, c, d, e, m);
        b.part += 1;
        i += 1;
    }
    b
}

model!(
    /// MACHINES: the marker where the bob can be set down (gold).
    MARKER_OK, 16, 8, marker(mat::GOLD)
);
model!(
    /// MACHINES: the marker where it cannot (dark blue-grey).
    MARKER_NO, 16, 8, marker(mat::PANTS)
);
model!(
    /// STONE: the stone's outline, drawn under the feet in the air while
    /// an air place would work: a 1 m square of 6 cm gold strips.
    STONE_OUTLINE, 16, 8, outline(mat::GOLD, 50, 6)
);
model!(
    /// MACHINES: the goal's beacon, a 1 m post with a gold diamond on
    /// it. Origin at the foot of the post.
    BEACON, 18, 16,
    Mb::new()
        .boxm([-4, 0, -4], [4, 100, 4], im::TRUNK, im::TRUNK, side::TOP | side::BOTTOM)
        .cone(4, [0, 120, 0], 20, 28, 0, mat::GOLD, false)
        .cone(4, [0, 120, 0], 20, -28, 0, mat::GOLD, false)
);

/// SAIL: where the sail's origin sits on the gliding hero (cm, hero
/// space: x right, y up from the feet, z forward): behind the back at
/// shoulder height, so that its sort depth is nearer the camera than the
/// hero's middle when the camera is behind him.
pub const SAIL_AT: [i32; 3] = [0, 62, -12];

/// The sail's corners (cm, from [`SAIL_AT`]): each wing a triangle from
/// the body's side (x 13, from the hip, 30 cm under the origin, to the
/// shoulder, 12 cm over it) out to a tip 62 cm from the middle, 30 cm
/// past the outstretched hand (x 32), at shoulder height and 8 cm further
/// back: two triangles in a shallow V, the crimson back left open between
/// them. (A first try 26 cm high at the root read as a thin line from
/// the follow camera.)
const SAIL_V: [[i32; 3]; 6] = [
    [13, -30, 3],
    [13, 12, 3],
    [62, 14, -5],
    [-13, -30, 3],
    [-13, 12, 3],
    [-62, 14, -5],
];

/// The faces, wound by hand: both sides of each wing (a lone flat face
/// has its part middle in its own plane, so `orient_by_middle` cannot
/// judge it). `(b - a) x (c - a)`: the right wing's (0, 1, 2) looks back
/// (-z), the left wing's (3, 5, 4) too.
const SAIL_F: [Face; 4] = [
    Face {
        v: [0, 1, 2],
        mat: im::SAIL,
    },
    Face {
        v: [0, 2, 1],
        mat: im::SAIL,
    },
    Face {
        v: [3, 5, 4],
        mat: im::SAIL,
    },
    Face {
        v: [3, 4, 5],
        mat: im::SAIL,
    },
];

const fn sail_verts() -> [[i16; 3]; 6] {
    let mut out = [[0i16; 3]; 6];
    let mut i = 0;
    while i < 6 {
        let p = SAIL_V[i];
        out[i] = [uc(p[0]) as i16, uc(p[1]) as i16, uc(p[2]) as i16];
        i += 1;
    }
    out
}

/// SAIL: the cloth spread between the arms behind the back while
/// gliding (4 faces, 2 seen from any side). Origin at [`SAIL_AT`].
pub static SAIL: Model = {
    const V: [[i16; 3]; 6] = sail_verts();
    const N: [[i16; 3]; 4] = normals(&V, &SAIL_F);
    Model {
        verts: &V,
        faces: &SAIL_F,
        normals: &N,
        radius: radius_of(&V),
    }
};
