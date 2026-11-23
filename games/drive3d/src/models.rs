//! The fixed models, made while compiling: the rally car, trees with 4,
//! 6, 8 and 12 sides, a rock, the hut, a post, the split banner, and
//! MAX's corner marker, fence rails and hay bale. Sizes are written in cm and turned
//! into game-3d units (1/256 m) here.
//!
//! Each model's origin is where it stands on the land (the middle of
//! the car's base, the foot of a trunk), x to the right, y up, z
//! forward (the car's nose looks along +z).
//!
//! Shared corners are written once (the builder looks each corner up
//! before adding it), and faces that meet along an edge always share
//! both of its corners: a big face with a corner in the middle of one
//! edge is written as a fan that has that corner too, so no "T" joint
//! can open a crack between two faces of one model.

use game_3d::{Face, Model, RawFace, normals, orient_by_middle, radius_of, sin_fine};

/// The materials of the lit part of the palette (palette number =
/// shade * [`MATS`] + material; `draw::PALETTE`).
pub mod mat {
    /// The car's paint (an orange red).
    pub const BODY: u8 = 0;
    /// The car's roof (white).
    pub const ROOF: u8 = 1;
    /// Glass (dark blue grey).
    pub const GLASS: u8 = 2;
    /// Tyres (near black).
    pub const TYRE: u8 = 3;
    /// The rear lamps: they shine by themselves (`GLOW`).
    pub const LAMP: u8 = 4;
    /// Bumper and wing (dark grey).
    pub const TRIM: u8 = 5;
    /// Tree crowns.
    pub const LEAF: u8 = 6;
    pub const TRUNK: u8 = 7;
    pub const ROCK: u8 = 8;
    /// The hut's walls (planks).
    pub const WALL: u8 = 9;
    /// The hut's roof.
    pub const HUT_ROOF: u8 = 10;
    /// Posts and the banner's light squares (white).
    pub const POST: u8 = 11;
    /// The banner's dark squares (yellow).
    pub const FLAG: u8 = 12;
    /// The banner's poles.
    pub const POLE: u8 = 13;
}

/// Materials in the lit part of the palette.
pub const MATS: u8 = 14;
/// Shades of each material: 0 (lit) to 3 (turned away), plus up to 2
/// haze steps far away.
pub const SHADES: u8 = 6;
/// Materials that shine by themselves (one bit each).
pub const GLOW: u32 = 1 << mat::LAMP;

/// cm to game units (1/256 m), rounded down.
pub const fn uc(c: i32) -> i16 {
    (c * 64).div_euclid(25) as i16
}

/// A model under construction: up to `V` corners and exactly `F` faces.
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

    /// A corner in cm: the one already there at that place, or a new one.
    const fn vert(&mut self, p: [i32; 3]) -> u8 {
        let q = [uc(p[0]), uc(p[1]), uc(p[2])];
        let mut i = 0;
        while i < self.nv {
            let o = self.v[i];
            if o[0] == q[0] && o[1] == q[1] && o[2] == q[2] {
                return i as u8;
            }
            i += 1;
        }
        self.v[self.nv] = q;
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

    /// A flat convex face (cm), as a fan of triangles from its first
    /// corner (so the first corner must not lie on a line with two
    /// neighbouring corners). `n - 2` faces.
    pub const fn poly(mut self, p: &[[i32; 3]], mat: u8) -> Self {
        let first = self.vert(p[0]);
        let mut k = 1;
        while k + 1 < p.len() {
            let b = self.vert(p[k]);
            let c = self.vert(p[k + 1]);
            self.tri(first, b, c, mat);
            k += 1;
        }
        self
    }

    /// Ends a convex part (the faces since the last one turn their fronts
    /// away from its middle).
    pub const fn part(mut self) -> Self {
        self.part += 1;
        self
    }

    /// A box from `lo` to `hi` (cm), in `m` with `top` on its top,
    /// leaving out the sides in `skip` (bit 0 -x, 1 +x, 2 -y, 3 +y,
    /// 4 -z, 5 +z). Two faces for each side kept; one part.
    pub const fn boxm(mut self, lo: [i32; 3], hi: [i32; 3], top: u8, m: u8, skip: u8) -> Self {
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
            [k[0], k[2], k[6], k[4]],
            [k[1], k[3], k[7], k[5]],
            [k[0], k[1], k[5], k[4]],
            [k[2], k[3], k[7], k[6]],
            [k[0], k[1], k[3], k[2]],
            [k[4], k[5], k[7], k[6]],
        ];
        let mut q = 0;
        while q < 6 {
            if skip & (1 << q) == 0 {
                let mm = if q == 3 { top } else { m };
                let c = quads[q];
                self.tri(c[0], c[1], c[2], mm);
                self.tri(c[0], c[2], c[3], mm);
            }
            q += 1;
        }
        self.part += 1;
        self
    }

    /// A cone of `n` sides: a ring of radius `r` round `c` at the height
    /// `c[1]`, its tip `h` above (cm); with `base` the ring is closed
    /// too. One part.
    pub const fn cone(mut self, n: usize, c: [i32; 3], r: i32, h: i32, m: u8, base: bool) -> Self {
        let tip = self.vert([c[0], c[1] + h, c[2]]);
        let mut ring = [0u8; 16];
        let mut k = 0;
        while k < n {
            let a = (k * 65_536 / n) as u16;
            ring[k] = self.vert([
                c[0] + sin_fine(a) * r / 16_384,
                c[1],
                c[2] + sin_fine(a.wrapping_add(16_384)) * r / 16_384,
            ]);
            k += 1;
        }
        let mut k = 0;
        while k < n {
            self.tri(tip, ring[k], ring[(k + 1) % n], m);
            k += 1;
        }
        if base {
            let mut k = 1;
            while k + 1 < n {
                self.tri(ring[0], ring[k], ring[k + 1], m);
                k += 1;
            }
        }
        self.part += 1;
        self
    }

    /// A drum of `n` sides lying along x: rings of radius `r` round the
    /// line (y `cy`, z 0) at x = -`l` and `l` (cm), its sides in `m`
    /// and both ends in `end`. One part.
    pub const fn drum(mut self, n: usize, l: i32, cy: i32, r: i32, m: u8, end: u8) -> Self {
        let mut ring = [[0u8; 2]; 16];
        let mut k = 0;
        while k < n {
            let a = (k * 65_536 / n) as u16;
            let y = cy + sin_fine(a.wrapping_add(16_384)) * r / 16_384;
            let z = sin_fine(a) * r / 16_384;
            ring[k] = [self.vert([-l, y, z]), self.vert([l, y, z])];
            k += 1;
        }
        let mut k = 0;
        while k < n {
            let [a, b] = ring[k];
            let [c, d] = ring[(k + 1) % n];
            self.tri(a, b, d, m);
            self.tri(a, d, c, m);
            k += 1;
        }
        let mut k = 1;
        while k + 1 < n {
            self.tri(ring[0][0], ring[k][0], ring[k + 1][0], end);
            self.tri(ring[0][1], ring[k + 1][1], ring[k][1], end);
            k += 1;
        }
        self.part += 1;
        self
    }

    /// Stops the build unless every face was filled.
    pub const fn done(self) -> Self {
        assert!(self.nf == F, "model: wrong face count");
        self
    }

    /// Corners used.
    pub const fn corners(&self) -> usize {
        self.nv
    }
}

/// Makes a [`Model`] from a finished builder: faces turned outwards,
/// directions, radius. `$n` is the corner count (checked).
macro_rules! model {
    ($name:ident, $mb:expr, $v:expr, $f:expr, $n:expr) => {
        model!($name, $mb, $v, $f, $n, orient_by_middle(&VERTS, &B.f));
    };
    ($name:ident, $mb:expr, $v:expr, $f:expr, $n:expr, $faces:expr) => {
        pub static $name: Model = {
            const B: Mb<$v, $f> = $mb.done();
            const _: () = assert!(B.corners() == $n, "model: wrong corner count");
            const VERTS: [[i16; 3]; $n] = {
                let mut out = [[0i16; 3]; $n];
                let mut i = 0;
                while i < $n {
                    out[i] = B.v[i];
                    i += 1;
                }
                out
            };
            const FACES: [Face; $f] = $faces;
            static V: [[i16; 3]; $n] = VERTS;
            static F: [Face; $f] = FACES;
            static N: [[i16; 3]; $f] = normals(&VERTS, &FACES);
            Model {
                verts: &V,
                faces: &F,
                normals: &N,
                radius: radius_of(&VERTS),
            }
        };
    };
}

// ---- The car -------------------------------------------------------
//
// 4.0 m long, 1.76 m of body and 2.08 m over the wheels (so from behind
// at the chase distance it is about 20 to 26 px wide), 1.4 m tall. A
// low body with a rear panel of a dark bumper, two lamps and the paint
// between them; a cabin narrower than the body with sloped glass and a
// white roof; a wing over the boot; four wheels that stick out at the
// corners. Nothing here is taken from a real car or livery.

/// Half widths and heights of the car (cm).
const BX: i32 = 88;
/// The wheels' outer side.
const WX: i32 = 104;
const BY0: i32 = 22;
const BY1: i32 = 92;
const BUMP: i32 = 50;
const LAMP_X: i32 = 58;
const ZR: i32 = -200;
const ZF: i32 = 200;
const CAB_X0: i32 = 74;
const CAB_X1: i32 = 62;
const CAB_Y: i32 = 140;
const CAB_R0: i32 = -110;
const CAB_F0: i32 = 70;
const CAB_R1: i32 = -70;
const CAB_F1: i32 = 30;

const fn car() -> Mb<96, 92> {
    let b = Mb::<96, 92>::new();
    // The rear panel (z = ZR): bumper, then lamp, paint, lamp.
    let b = b.poly(
        &[
            [BX, BY0, ZR],
            [BX, BUMP, ZR],
            [LAMP_X, BUMP, ZR],
            [-LAMP_X, BUMP, ZR],
            [-BX, BUMP, ZR],
            [-BX, BY0, ZR],
        ],
        mat::TRIM,
    );
    let b = b.poly(
        &[
            [BX, BUMP, ZR],
            [BX, BY1, ZR],
            [LAMP_X, BY1, ZR],
            [LAMP_X, BUMP, ZR],
        ],
        mat::LAMP,
    );
    let b = b.poly(
        &[
            [LAMP_X, BUMP, ZR],
            [LAMP_X, BY1, ZR],
            [-LAMP_X, BY1, ZR],
            [-LAMP_X, BUMP, ZR],
        ],
        mat::BODY,
    );
    let b = b.poly(
        &[
            [-LAMP_X, BUMP, ZR],
            [-LAMP_X, BY1, ZR],
            [-BX, BY1, ZR],
            [-BX, BUMP, ZR],
        ],
        mat::LAMP,
    );
    // The boot lid (top, from the rear edge to the cabin), in three
    // pieces so every corner of both edges is shared.
    let b = b.poly(
        &[
            [BX, BY1, ZR],
            [LAMP_X, BY1, ZR],
            [CAB_X0, BY1, CAB_R0],
            [BX, BY1, CAB_R0],
        ],
        mat::BODY,
    );
    let b = b.poly(
        &[
            [LAMP_X, BY1, ZR],
            [-LAMP_X, BY1, ZR],
            [-CAB_X0, BY1, CAB_R0],
            [CAB_X0, BY1, CAB_R0],
        ],
        mat::BODY,
    );
    let b = b.poly(
        &[
            [-LAMP_X, BY1, ZR],
            [-BX, BY1, ZR],
            [-BX, BY1, CAB_R0],
            [-CAB_X0, BY1, CAB_R0],
        ],
        mat::BODY,
    );
    // Beside the cabin.
    let b = b.poly(
        &[
            [BX, BY1, CAB_R0],
            [CAB_X0, BY1, CAB_R0],
            [CAB_X0, BY1, CAB_F0],
            [BX, BY1, CAB_F0],
        ],
        mat::BODY,
    );
    let b = b.poly(
        &[
            [-BX, BY1, CAB_R0],
            [-BX, BY1, CAB_F0],
            [-CAB_X0, BY1, CAB_F0],
            [-CAB_X0, BY1, CAB_R0],
        ],
        mat::BODY,
    );
    // The bonnet.
    let b = b.poly(
        &[
            [BX, BY1, ZF],
            [BX, BY1, CAB_F0],
            [CAB_X0, BY1, CAB_F0],
            [-CAB_X0, BY1, CAB_F0],
            [-BX, BY1, CAB_F0],
            [-BX, BY1, ZF],
        ],
        mat::BODY,
    );
    // The sides (with every corner of the edges they share), each in
    // three pieces along the car so no long thin triangle sorts wrongly
    // against a wheel when the car is seen from the side.
    let b = b.poly(
        &[
            [BX, BY0, ZF],
            [BX, BY1, ZF],
            [BX, BY1, CAB_F0],
            [BX, BY0, CAB_F0],
        ],
        mat::BODY,
    );
    let b = b.poly(
        &[
            [BX, BY0, CAB_F0],
            [BX, BY1, CAB_F0],
            [BX, BY1, CAB_R0],
            [BX, BY0, CAB_R0],
        ],
        mat::BODY,
    );
    let b = b.poly(
        &[
            [BX, BY0, CAB_R0],
            [BX, BY1, CAB_R0],
            [BX, BY1, ZR],
            [BX, BUMP, ZR],
            [BX, BY0, ZR],
        ],
        mat::BODY,
    );
    let b = b.poly(
        &[
            [-BX, BY0, ZF],
            [-BX, BY0, CAB_F0],
            [-BX, BY1, CAB_F0],
            [-BX, BY1, ZF],
        ],
        mat::BODY,
    );
    let b = b.poly(
        &[
            [-BX, BY0, CAB_F0],
            [-BX, BY0, CAB_R0],
            [-BX, BY1, CAB_R0],
            [-BX, BY1, CAB_F0],
        ],
        mat::BODY,
    );
    let b = b.poly(
        &[
            [-BX, BY0, CAB_R0],
            [-BX, BY0, ZR],
            [-BX, BUMP, ZR],
            [-BX, BY1, ZR],
            [-BX, BY1, CAB_R0],
        ],
        mat::BODY,
    );
    // The nose.
    let b = b.poly(
        &[[BX, BY0, ZF], [-BX, BY0, ZF], [-BX, BY1, ZF], [BX, BY1, ZF]],
        mat::TRIM,
    );
    let b = b.part();
    // The cabin: sloped rear glass and windscreen, side windows, roof.
    let b = b.poly(
        &[
            [CAB_X1, CAB_Y, CAB_R1],
            [CAB_X1, CAB_Y, CAB_F1],
            [-CAB_X1, CAB_Y, CAB_F1],
            [-CAB_X1, CAB_Y, CAB_R1],
        ],
        mat::ROOF,
    );
    let b = b.poly(
        &[
            [CAB_X0, BY1, CAB_R0],
            [CAB_X1, CAB_Y, CAB_R1],
            [-CAB_X1, CAB_Y, CAB_R1],
            [-CAB_X0, BY1, CAB_R0],
        ],
        mat::GLASS,
    );
    let b = b.poly(
        &[
            [CAB_X0, BY1, CAB_F0],
            [-CAB_X0, BY1, CAB_F0],
            [-CAB_X1, CAB_Y, CAB_F1],
            [CAB_X1, CAB_Y, CAB_F1],
        ],
        mat::GLASS,
    );
    let b = b.poly(
        &[
            [CAB_X0, BY1, CAB_R0],
            [CAB_X0, BY1, CAB_F0],
            [CAB_X1, CAB_Y, CAB_F1],
            [CAB_X1, CAB_Y, CAB_R1],
        ],
        mat::GLASS,
    );
    let b = b.poly(
        &[
            [-CAB_X0, BY1, CAB_R0],
            [-CAB_X1, CAB_Y, CAB_R1],
            [-CAB_X1, CAB_Y, CAB_F1],
            [-CAB_X0, BY1, CAB_F0],
        ],
        mat::GLASS,
    );
    let b = b.part();
    // The wing over the boot.
    let b = b.boxm(
        [-80, 114, -198],
        [80, 122, -168],
        mat::BODY,
        mat::TRIM,
        1 << 2,
    );
    // Wheels: outer side, top, front and back (the inner side and the
    // bottom are never seen). Each starts at the body's side, so no
    // wheel face reaches into the body (a face inside another part
    // cannot be sorted against it: MEASURED, PC, up to 14 wrong-order
    // pixels in a slide when they did).
    let b = b.boxm(
        [BX, 0, -158],
        [WX, 64, -94],
        mat::TYRE,
        mat::TYRE,
        1 | 1 << 2,
    );
    let b = b.boxm(
        [-WX, 0, -158],
        [-BX, 64, -94],
        mat::TYRE,
        mat::TYRE,
        2 | 1 << 2,
    );
    let b = b.boxm([BX, 0, 94], [WX, 64, 158], mat::TYRE, mat::TYRE, 1 | 1 << 2);
    b.boxm(
        [-WX, 0, 94],
        [-BX, 64, 158],
        mat::TYRE,
        mat::TYRE,
        2 | 1 << 2,
    )
}

model!(CAR, car(), 96, 92, 70);

/// The car for LOW: the same shape and colours from behind (rear panel
/// with its lamps, boot, cabin, wing), with fewer faces where they do
/// not change the look: the top in two pieces, whole sides, the wing's
/// top and back only, and wheels without their tops (front wheels with
/// their outer side only).
const fn plain_car() -> Mb<64, 56> {
    let b = Mb::<64, 56>::new();
    // The rear panel, as on the full car.
    let b = b.poly(
        &[
            [BX, BY0, ZR],
            [BX, BUMP, ZR],
            [LAMP_X, BUMP, ZR],
            [-LAMP_X, BUMP, ZR],
            [-BX, BUMP, ZR],
            [-BX, BY0, ZR],
        ],
        mat::TRIM,
    );
    let b = b.poly(
        &[
            [BX, BUMP, ZR],
            [BX, BY1, ZR],
            [LAMP_X, BY1, ZR],
            [LAMP_X, BUMP, ZR],
        ],
        mat::LAMP,
    );
    let b = b.poly(
        &[
            [LAMP_X, BUMP, ZR],
            [LAMP_X, BY1, ZR],
            [-LAMP_X, BY1, ZR],
            [-LAMP_X, BUMP, ZR],
        ],
        mat::BODY,
    );
    let b = b.poly(
        &[
            [-LAMP_X, BUMP, ZR],
            [-LAMP_X, BY1, ZR],
            [-BX, BY1, ZR],
            [-BX, BUMP, ZR],
        ],
        mat::LAMP,
    );
    // The boot (from the cabin's back edge, so its rear edge keeps the
    // lamps' corners) and the rest of the top in one piece.
    let b = b.poly(
        &[
            [BX, BY1, CAB_R0],
            [BX, BY1, ZR],
            [LAMP_X, BY1, ZR],
            [-LAMP_X, BY1, ZR],
            [-BX, BY1, ZR],
            [-BX, BY1, CAB_R0],
        ],
        mat::BODY,
    );
    let b = b.poly(
        &[
            [BX, BY1, ZF],
            [BX, BY1, CAB_R0],
            [-BX, BY1, CAB_R0],
            [-BX, BY1, ZF],
        ],
        mat::BODY,
    );
    // The sides and the nose.
    let b = b.poly(
        &[
            [BX, BY0, ZF],
            [BX, BY1, ZF],
            [BX, BY1, CAB_R0],
            [BX, BY1, ZR],
            [BX, BUMP, ZR],
            [BX, BY0, ZR],
        ],
        mat::BODY,
    );
    let b = b.poly(
        &[
            [-BX, BY0, ZF],
            [-BX, BY0, ZR],
            [-BX, BUMP, ZR],
            [-BX, BY1, ZR],
            [-BX, BY1, CAB_R0],
            [-BX, BY1, ZF],
        ],
        mat::BODY,
    );
    let b = b.poly(
        &[[BX, BY0, ZF], [-BX, BY0, ZF], [-BX, BY1, ZF], [BX, BY1, ZF]],
        mat::TRIM,
    );
    let b = b.part();
    // The cabin, as on the full car.
    let b = b.poly(
        &[
            [CAB_X1, CAB_Y, CAB_R1],
            [CAB_X1, CAB_Y, CAB_F1],
            [-CAB_X1, CAB_Y, CAB_F1],
            [-CAB_X1, CAB_Y, CAB_R1],
        ],
        mat::ROOF,
    );
    let b = b.poly(
        &[
            [CAB_X0, BY1, CAB_R0],
            [CAB_X1, CAB_Y, CAB_R1],
            [-CAB_X1, CAB_Y, CAB_R1],
            [-CAB_X0, BY1, CAB_R0],
        ],
        mat::GLASS,
    );
    let b = b.poly(
        &[
            [CAB_X0, BY1, CAB_F0],
            [-CAB_X0, BY1, CAB_F0],
            [-CAB_X1, CAB_Y, CAB_F1],
            [CAB_X1, CAB_Y, CAB_F1],
        ],
        mat::GLASS,
    );
    let b = b.poly(
        &[
            [CAB_X0, BY1, CAB_R0],
            [CAB_X0, BY1, CAB_F0],
            [CAB_X1, CAB_Y, CAB_F1],
            [CAB_X1, CAB_Y, CAB_R1],
        ],
        mat::GLASS,
    );
    let b = b.poly(
        &[
            [-CAB_X0, BY1, CAB_R0],
            [-CAB_X1, CAB_Y, CAB_R1],
            [-CAB_X1, CAB_Y, CAB_F1],
            [-CAB_X0, BY1, CAB_F0],
        ],
        mat::GLASS,
    );
    let b = b.part();
    // The wing: its top and its back.
    let b = b.boxm(
        [-80, 114, -198],
        [80, 122, -168],
        mat::BODY,
        mat::TRIM,
        0b10_0111,
    );
    // Rear wheels: outer side, front and back; front wheels: outer side.
    let b = b.boxm(
        [BX, 0, -158],
        [WX, 64, -94],
        mat::TYRE,
        mat::TYRE,
        0b00_1101,
    );
    let b = b.boxm(
        [-WX, 0, -158],
        [-BX, 64, -94],
        mat::TYRE,
        mat::TYRE,
        0b00_1110,
    );
    let b = b.boxm([BX, 0, 94], [WX, 64, 158], mat::TYRE, mat::TYRE, 0b11_1101);
    b.boxm(
        [-WX, 0, 94],
        [-BX, 64, 158],
        mat::TYRE,
        mat::TYRE,
        0b11_1110,
    )
}

model!(CAR_PLAIN, plain_car(), 64, 56, 64);

// ---- Things --------------------------------------------------------

/// A tree 6 m tall (size 24): a thin three-sided trunk and a crown of
/// `n` sides from 1.5 m up.
const fn tree<const F: usize>(n: usize) -> Mb<20, F> {
    Mb::<20, F>::new()
        .cone(3, [0, 0, 0], 24, 200, mat::TRUNK, false)
        .cone(n, [0, 150, 0], 190, 450, mat::LEAF, true)
}

model!(TREE4, tree::<9>(4), 20, 9, 9);
model!(TREE6, tree::<13>(6), 20, 13, 11);
model!(TREE8, tree::<17>(8), 20, 17, 13);
model!(TREE12, tree::<25>(12), 20, 25, 17);

/// The size (1/4 m) the tree models are built for.
pub const TREE_SIZE: i32 = 24;

/// A rock 1 m tall (size 4): a low five-sided cone standing on its base
/// (the scene sets it on the highest land under that base).
const fn rock() -> Mb<6, 5> {
    Mb::<6, 5>::new().cone(5, [0, 0, 0], 120, 100, mat::ROCK, false)
}

model!(ROCK, rock(), 6, 5, 6);

/// The size (1/4 m) the rock and post models are built for.
pub const SMALL_SIZE: i32 = 4;

/// The hut: a 5 m by 4 m shed of planks with a pitched roof.
const fn hut() -> Mb<14, 16> {
    Mb::<14, 16>::new()
        .boxm(
            [-250, 0, -200],
            [250, 250, 200],
            mat::WALL,
            mat::WALL,
            1 << 2 | 1 << 3,
        )
        .poly(
            &[
                [-270, 250, -220],
                [270, 250, -220],
                [270, 360, 0],
                [-270, 360, 0],
            ],
            mat::HUT_ROOF,
        )
        .poly(
            &[
                [-270, 250, 220],
                [-270, 360, 0],
                [270, 360, 0],
                [270, 250, 220],
            ],
            mat::HUT_ROOF,
        )
        .poly(
            &[[-270, 250, -220], [-270, 360, 0], [-270, 250, 220]],
            mat::WALL,
        )
        .poly(
            &[[270, 250, -220], [270, 250, 220], [270, 360, 0]],
            mat::WALL,
        )
        .poly(
            &[
                [-270, 250, -220],
                [-270, 250, 220],
                [270, 250, 220],
                [270, 250, -220],
            ],
            mat::HUT_ROOF,
        )
        .part()
}

model!(HUT, hut(), 14, 16, 14);

/// A post 1 m tall (size 4), 16 cm square, with no bottom.
const fn post() -> Mb<8, 10> {
    Mb::<8, 10>::new().boxm([-8, 0, -8], [8, 100, 8], mat::POST, mat::POST, 1 << 2)
}

model!(POST, post(), 8, 10, 8);

// ---- MAX's roadside things (drawing only, `things::DECOR`) ----------

/// A corner marker 1.3 m tall (size 5): a white post and a yellow
/// board on it, 40 cm wide, facing along z (it is turned along the
/// road). The board has no thickness: a front and a back on the same
/// four corners (its part's middle lies in its plane, so each keeps the
/// turn it is written with), which at 160 x 120 looks the same as a
/// board 8 cm thick and is 84 bytes smaller.
const fn marker() -> Mb<12, 12> {
    let (l, r, lo, hi) = (-20, 20, 90, 130);
    Mb::<12, 12>::new()
        .boxm(
            [-6, 0, -6],
            [6, 90, 6],
            mat::POST,
            mat::POST,
            1 << 2 | 1 << 3,
        )
        .poly(&[[l, lo, 0], [r, lo, 0], [r, hi, 0], [l, hi, 0]], mat::FLAG)
        .poly(&[[l, lo, 0], [l, hi, 0], [r, hi, 0], [r, lo, 0]], mat::FLAG)
        .part()
}

model!(MARKER, marker(), 12, 12, 12);

/// The fence's two rails between two of its posts 8 m apart (the
/// thing stands halfway), along z: white boards 0.45 to 0.55 m and 0.82
/// to 0.94 m over the line from post foot to post foot, which falls 1 m
/// over the 8 m (the drop falls 5 m over its 40 m), stopping 8 cm short
/// of each post's middle.
const fn rail() -> Mb<8, 8> {
    let b = Mb::<8, 8>::new();
    let (z0, z1) = (-392, 392);
    let b = board(b, z0, z1, 45, 55);
    board(b, z0, z1, 82, 94)
}

/// One board of [`rail`] from `z0` to `z1`, `lo` to `hi` over the line
/// y = -z / 8 (cm), with no thickness: a side facing -x and one facing
/// +x on the same four corners (the part's middle lies in their plane,
/// so each keeps the turn it is written with). At 160 x 120 it looks
/// the same as a board 4 cm thick with a top and a bottom, and is 128
/// bytes smaller for the two boards. One part.
const fn board(b: Mb<8, 8>, z0: i32, z1: i32, lo: i32, hi: i32) -> Mb<8, 8> {
    let (y0, y1) = (-z0 / 8, -z1 / 8);
    b.poly(
        &[
            [0, y0 + lo, z0],
            [0, y1 + lo, z1],
            [0, y1 + hi, z1],
            [0, y0 + hi, z0],
        ],
        mat::POST,
    )
    .poly(
        &[
            [0, y0 + lo, z0],
            [0, y0 + hi, z0],
            [0, y1 + hi, z1],
            [0, y1 + lo, z1],
        ],
        mat::POST,
    )
    .part()
}

model!(RAIL, rail(), 8, 8, 8);

/// A round hay bale 1.2 m across and 1.2 m long (size 5), lying on its
/// side: an eight-sided drum, yellow straw round, its cut ends in the
/// planks' paler brown.
const fn bale() -> Mb<16, 28> {
    Mb::<16, 28>::new().drum(8, 60, 60, 60, mat::FLAG, mat::WALL)
}

model!(BALE, bale(), 16, 28, 16);

/// The split banner: two poles at the road's edges and a bar across
/// the road 4 to 4.7 m up, in four squares of yellow and white.
const fn banner<const F: usize>(pieces: usize) -> Mb<36, F> {
    let b = Mb::<36, F>::new()
        .boxm(
            [-348, 0, -8],
            [-332, 400, 8],
            mat::POLE,
            mat::POLE,
            1 << 2 | 1 << 3,
        )
        .boxm(
            [332, 0, -8],
            [348, 400, 8],
            mat::POLE,
            mat::POLE,
            1 << 2 | 1 << 3,
        );
    // The bar: front, back and bottom in `pieces` squares each (4, or
    // 2 at LOW), and its two ends; the top is never seen from the road.
    let xs: [i32; 5] = if pieces == 4 {
        [-348, -174, 0, 174, 348]
    } else {
        [-348, 0, 348, 0, 0]
    };
    let (y0, y1, z0, z1) = (400, 470, -6, 6);
    let mut b = b;
    let mut k = 0;
    while k < pieces {
        let m = if k % 2 == 0 { mat::FLAG } else { mat::POST };
        let (a, c) = (xs[k], xs[k + 1]);
        b = b
            .poly(&[[a, y0, z0], [c, y0, z0], [c, y1, z0], [a, y1, z0]], m)
            .poly(&[[a, y0, z1], [a, y1, z1], [c, y1, z1], [c, y0, z1]], m)
            .poly(&[[a, y0, z0], [a, y0, z1], [c, y0, z1], [c, y0, z0]], m);
        k += 1;
    }
    b.poly(
        &[
            [-348, y0, z0],
            [-348, y1, z0],
            [-348, y1, z1],
            [-348, y0, z1],
        ],
        mat::FLAG,
    )
    .poly(
        &[[348, y0, z0], [348, y0, z1], [348, y1, z1], [348, y1, z0]],
        mat::POST,
    )
    .part()
}

model!(BANNER, banner::<44>(4), 36, 44, 36);
model!(BANNER_PLAIN, banner::<32>(2), 36, 32, 28);

// ---- The ford's spray (R3, drawing only) -----------------------------

/// The faces as written (no turning): for sheets with no thickness,
/// each written once per side.
const fn as_written<const N: usize>(raw: &[RawFace; N]) -> [Face; N] {
    let mut out = [Face { v: [0; 3], mat: 0 }; N];
    let mut i = 0;
    while i < N {
        out[i] = Face {
            v: raw[i].v,
            mat: raw[i].mat,
        };
        i += 1;
    }
    out
}

/// One sheet of water pushed aside by a wheel on the car's right,
/// at its full size (1.5 m high, reaching 2.6 m out and 1.9 m back): its
/// origin is the middle of its foot beside the tyre, and it fans out,
/// up and back like a bow wave, bent once so its foot and its crest
/// take different light. White, with no thickness (each half written
/// once per side). The left side's sheets are this one mirrored.
const fn spray() -> Mb<6, 8> {
    let (f0, f1) = ([0, 0, 30], [0, 0, -30]);
    let (top, far) = ([220, 150, -72], [260, 70, -192]);
    let (m0, m1) = ([100, 80, -21], [130, 40, -111]);
    Mb::<6, 8>::new()
        .poly(&[f0, m0, m1, f1], mat::POST)
        .poly(&[f0, f1, m1, m0], mat::POST)
        .poly(&[m0, top, far, m1], mat::POST)
        .poly(&[m0, m1, far, top], mat::POST)
}

model!(SPRAY, spray(), 6, 8, 6, as_written(&B.f));
