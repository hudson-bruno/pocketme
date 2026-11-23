//! The camera, the view (the window on the picture and its lens) and
//! the five planes a drawn point must be inside of.

use crate::fill::Rect;
use crate::fixed::{Mat3, isqrt};
use crate::sums::{Checked, Sums};
use crate::{NEAR, SUB};

/// Where the eye is and which way it looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Camera {
    /// The eye, in world units (1/256 m).
    pub pos: [i32; 3],
    /// World to camera: the camera's own turning, taken back (for a
    /// camera turned by `R`, this is `R` transposed).
    pub rot: Mat3,
}

impl Camera {
    /// A world point in camera space (checked sums).
    #[inline(always)]
    pub fn to_cam(&self, p: [i32; 3]) -> [i32; 3] {
        self.to_cam_with::<Checked>(p)
    }

    /// A world point in camera space, with the sums `S`.
    #[inline(never)]
    pub fn to_cam_with<S: Sums>(&self, p: [i32; 3]) -> [i32; 3] {
        self.rot.apply_with::<S>([
            S::sub(p[0], self.pos[0]),
            S::sub(p[1], self.pos[1]),
            S::sub(p[2], self.pos[2]),
        ])
    }
}

/// The window on the picture the 3D view is drawn in, and its lens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    /// The pixels the view may paint.
    pub area: Rect,
    /// The lens: a thing 1 m wide at 1 m distance is this many pixels.
    pub focal: i32,
    /// The guard band: faces reaching more than this many times half
    /// the view's width (or height) beyond the middle are cut in 3D;
    /// closer ones are only cut by the filler.
    pub guard: i32,
}

impl View {
    pub const fn new(area: Rect, focal: i32) -> View {
        View {
            area,
            focal,
            guard: 2,
        }
    }

    /// The middle of the view in 1/16 pixel.
    pub const fn centre(&self) -> (i32, i32) {
        (
            self.area.x0 * SUB + self.area.w() * SUB / 2,
            self.area.y0 * SUB + self.area.h() * SUB / 2,
        )
    }

    /// Where a point in camera space lands, in 1/16 pixel (z at least
    /// the near plane), in 64 bits ([`crate::Lens::Wide`], checked sums).
    #[inline(always)]
    pub fn project(&self, p: [i32; 3]) -> [i32; 2] {
        self.project_with::<Checked>(p)
    }

    /// [`View::project`] with the sums `S`.
    pub fn project_with<S: Sums>(&self, p: [i32; 3]) -> [i32; 2] {
        let (cx, cy) = self.centre();
        let f = i64::from(S::mul(self.focal, SUB));
        let z = i64::from(p[2].max(1));
        [
            S::add(cx, (S::mul64(i64::from(p[0]), f) / z) as i32),
            S::sub(cy, (S::mul64(i64::from(p[1]), f) / z) as i32),
        ]
    }

    /// CAMBER's lens ([`crate::Lens::Fast`]): in 32 bits when |x| and
    /// |y| are below 2^20 and the lens below 128 pixels, else in 64 bits
    /// with the result held to +-2^24.
    pub fn project_fast<S: Sums>(&self, p: [i32; 3]) -> [i32; 2] {
        let (cx, cy) = self.centre();
        let f = S::mul(self.focal, SUB);
        let z = p[2].max(1);
        if p[0].unsigned_abs() < 1 << 20 && p[1].unsigned_abs() < 1 << 20 && f < 2048 {
            [
                S::add(cx, S::mul(p[0], f) / z),
                S::sub(cy, S::mul(p[1], f) / z),
            ]
        } else {
            let (f, z) = (i64::from(f), i64::from(z));
            [
                S::add(
                    cx,
                    (S::mul64(i64::from(p[0]), f) / z).clamp(-1 << 24, 1 << 24) as i32,
                ),
                S::sub(
                    cy,
                    (S::mul64(i64::from(p[1]), f) / z).clamp(-1 << 24, 1 << 24) as i32,
                ),
            ]
        }
    }

    /// BENCH's lens ([`crate::Lens::Narrow`]): in 32 bits only. `|x|`
    /// and `|y|` times the lens times 16 must stay below 2^31.
    pub fn project_narrow<S: Sums>(&self, p: [i32; 3]) -> [i32; 2] {
        let (cx, cy) = self.centre();
        let f = S::mul(self.focal, SUB);
        let z = p[2].max(1);
        [
            S::add(cx, S::mul(p[0], f) / z),
            S::sub(cy, S::mul(p[1], f) / z),
        ]
    }

    /// The five planes a drawn point must be inside of, as (a, b, c, e):
    /// inside when `a*x + b*y + c*z + e >= 0`. Near (at [`NEAR`]), left,
    /// right, top, bottom (the sides at the guard band).
    #[inline(always)]
    pub fn planes(&self) -> [[i32; 4]; 5] {
        self.planes_at(NEAR)
    }

    /// The five planes, with the near plane at `near` (1/256 m).
    pub const fn planes_at(&self, near: i32) -> [[i32; 4]; 5] {
        let f = self.focal;
        let gx = self.area.w() / 2 * self.guard;
        let gy = self.area.h() / 2 * self.guard;
        [
            [0, 0, 1, -near],
            [f, 0, gx, 0],
            [-f, 0, gx, 0],
            [0, -f, gy, 0],
            [0, f, gy, 0],
        ]
    }

    /// The planes and their lengths, worked out once for many objects.
    #[inline(always)]
    pub fn clip_planes(&self) -> Planes {
        self.clip_planes_at(NEAR)
    }

    /// [`View::clip_planes`] with the near plane at `near`. A `const fn`:
    /// the planes of a view that never changes can be a constant.
    pub const fn clip_planes_at(&self, near: i32) -> Planes {
        let p = self.planes_at(near);
        let mut len = [0i64; 5];
        let mut k = 0;
        while k < 5 {
            let pl = p[k];
            let sq = pl[0] as i64 * pl[0] as i64
                + pl[1] as i64 * pl[1] as i64
                + pl[2] as i64 * pl[2] as i64;
            len[k] = isqrt(sq as u64) as i64;
            k += 1;
        }
        Planes { p, len }
    }
}

/// A view's five planes ([`View::planes`]) and the length of each
/// plane's (a, b, c), so the distance of a point from a plane is
/// `(a*x + b*y + c*z + e) / len`. The square roots are taken once per
/// view, not once per object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Planes {
    pub p: [[i32; 4]; 5],
    pub len: [i64; 5],
}

impl Planes {
    /// How far `c` (camera space) is inside plane `k`, times the
    /// plane's length (negative: outside).
    pub fn side<S: Sums>(&self, k: usize, c: [i32; 3]) -> i64 {
        plane_dist_with::<S>(&self.p[k], &c)
    }

    /// Which planes a camera-space point is outside of (bit k: plane k).
    pub fn outcode<S: Sums>(&self, c: &[i32; 3]) -> u8 {
        let mut out = 0;
        for (k, pl) in self.p.iter().enumerate() {
            if plane_dist_with::<S>(pl, c) < 0 {
                out |= 1 << k;
            }
        }
        out
    }
}

/// `a*x + b*y + c*z + e` for the plane (a, b, c, e) and the point: how far
/// the point is inside the plane, times the length of (a, b, c)
/// (negative: outside). Checked sums.
#[inline(always)]
pub fn plane_dist(pl: &[i32; 4], p: &[i32; 3]) -> i64 {
    plane_dist_with::<Checked>(pl, p)
}

/// [`plane_dist`] with the sums `S`. One function for every caller, not
/// inlined: on the chip a 64-bit sum is long, and the code slot is
/// small.
#[inline(never)]
pub fn plane_dist_with<S: Sums>(pl: &[i32; 4], p: &[i32; 3]) -> i64 {
    S::add64(
        S::add64(
            S::add64(S::wide(pl[0], p[0]), S::wide(pl[1], p[1])),
            S::wide(pl[2], p[2]),
        ),
        i64::from(pl[3]),
    )
}
