//! The model renderer: one object at a time, its faces far to near.

use core::marker::PhantomData;

use game_kit::Frame;

use crate::clip::clip_polygon_with;
use crate::count::{Count, fill_into};
use crate::fixed::Mat3;
use crate::model::Model;
use crate::setup::{Cull, Setup};
use crate::shade::Shade;
use crate::sums::Sums;
use crate::view::{Camera, Planes, View, plane_dist_with};
use crate::{GUARD, MAX_POLY};

/// What the renderer did for the last picture (for cost tests and the
/// log): KESTREL's counts, and the renderer's usual [`Count`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Objects drawn (at least partly in view).
    pub objects: u32,
    /// Corners turned and moved into camera space (each one a matrix
    /// times a vector; most are also projected: two divisions).
    pub verts: u32,
    /// Faces of those objects (before back faces are dropped).
    pub faces: u32,
    /// Faces left after back faces and faces fully outside are dropped
    /// (for [`crate::draw_poly`]: polygons drawn).
    pub front: u32,
    /// Faces that had to be cut by a plane.
    pub clipped: u32,
    /// Triangles filled.
    pub tris: u32,
    /// Triangles that did not fit the renderer's list (should stay 0).
    pub dropped: u32,
    /// Pixels painted by the filler (a pixel painted twice counts twice).
    pub pixels: u32,
}

/// One triangle waiting for its turn: corners in 1/16 pixel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Tri {
    p: [[i16; 2]; 3],
    c: u8,
}

/// The renderer's working memory: one model's corners (`V` of them at
/// most) and triangles (`T` at most, up to 256), and the choices `C`.
/// About `16 * V + 12 * T` bytes; keep it in the game's state, never on
/// the stack.
///
/// Two more choices have defaults: `P`, the corners a face may have
/// after it is cut (a triangle cut by five planes has at most 8, so
/// [`MAX_POLY`]; a game that also cuts bigger polygons with
/// [`crate::draw_poly`] gives the same `P` here, so both share one
/// copy of the cutting code), and `K`, what is counted in
/// [`Renderer::stats`] ([`Stats`]; see [`Count`]).
///
/// ```
/// use game_3d::{Renderer, Standard};
/// // KESTREL's: 48 corners, 112 triangles (about 2.4 KB).
/// type MyRenderer = Renderer<Standard, 48, 112>;
/// static RENDER: MyRenderer = MyRenderer::new();
/// ```
pub struct Renderer<
    C: Setup,
    const V: usize,
    const T: usize,
    const P: usize = MAX_POLY,
    K: Count = Stats,
> {
    /// Corners in camera space.
    cs: [[i32; 3]; V],
    /// Corners on screen (1/16 pixel), for corners inside all planes.
    sp: [[i16; 2]; V],
    /// Which planes each corner is outside of (bit k: plane k).
    out: [u8; V],
    tris: [Tri; T],
    /// Depth (bigger is farther) << 8 | triangle number, for sorting.
    keys: [u32; T],
    /// The last view drawn in, and its planes (worked out again only
    /// when the view changes).
    seen: Option<View>,
    planes: Planes,
    /// The counts since [`Renderer::begin`].
    pub stats: K,
    setup: PhantomData<C>,
}

impl<C: Setup, const V: usize, const T: usize, const P: usize, K: Count> Default
    for Renderer<C, V, T, P, K>
{
    fn default() -> Self {
        Self::new()
    }
}

/// One object to draw: a model, where it is, how it is turned and how
/// big it is.
#[derive(Clone, Copy, Debug)]
pub struct Object<'a> {
    pub model: &'a Model,
    /// Its middle, in world units (1/256 m).
    pub pos: [i32; 3],
    /// Model to world turning.
    pub rot: Mat3,
    /// Size: 256 is the model's own size. Not read when the setup has
    /// `SCALE = false`.
    pub scale: i32,
    /// Faces of material `swap.0` are drawn in material `swap.1` (one
    /// model, several looks). `(0, 0)` changes nothing.
    pub swap: (u8, u8),
}

/// Where an object's middle is in camera space, and whether its ball is
/// in view at all (`planes` from [`View::clip_planes_at`] with the
/// setup's near plane), nearer than `far`.
pub fn in_view<C: Setup>(
    planes: &Planes,
    cam: &Camera,
    pos: [i32; 3],
    radius: i32,
    far: i32,
) -> Option<[i32; 3]> {
    type S<C> = <C as Setup>::Sums;
    let rel = [
        S::<C>::sub(pos[0], cam.pos[0]),
        S::<C>::sub(pos[1], cam.pos[1]),
        S::<C>::sub(pos[2], cam.pos[2]),
    ];
    let c = cam.rot.apply_with::<S<C>>(rel);
    if S::<C>::add(c[2], radius) < C::NEAR || S::<C>::sub(c[2], radius) > far {
        return None;
    }
    for k in 1..5 {
        if planes.side::<S<C>>(k, c)
            < S::<C>::mul64(S::<C>::neg64(i64::from(radius)), planes.len[k])
        {
            return None;
        }
    }
    Some(c)
}

impl<C: Setup, const V: usize, const T: usize, const P: usize, K: Count> Renderer<C, V, T, P, K> {
    /// A triangle's number must fit the 8 low bits of its sort key.
    const FITS: () = assert!(
        T <= 256 && P >= MAX_POLY,
        "game-3d: at most 256 triangles per model, and room for 8 corners of a cut triangle"
    );

    pub const fn new() -> Self {
        #[allow(clippy::let_unit_value)]
        let () = Self::FITS;
        Renderer {
            cs: [[0; 3]; V],
            sp: [[0; 2]; V],
            out: [0; V],
            tris: [Tri {
                p: [[0; 2]; 3],
                c: 0,
            }; T],
            keys: [0; T],
            seen: None,
            planes: Planes {
                p: [[0; 4]; 5],
                len: [0; 5],
            },
            stats: K::ZERO,
            setup: PhantomData,
        }
    }

    /// Starts a new picture's counts.
    pub fn begin(&mut self) {
        self.stats = K::ZERO;
    }

    /// Draws one object into `f`, inside `view.area`, faces far to near.
    /// Objects themselves must be drawn far to near by the caller.
    /// Objects whose ball lies beyond `far` (1/256 m) are left out.
    /// Returns false when the object is not in view.
    pub fn draw<H: Shade>(
        &mut self,
        f: &mut Frame,
        view: &View,
        cam: &Camera,
        shading: &H,
        o: &Object<'_>,
        far: i32,
    ) -> bool {
        if self.seen != Some(*view) {
            self.seen = Some(*view);
            self.planes = view.clip_planes_at(C::NEAR);
        }
        self.draw_object(f, view, cam, shading, o, far)
    }

    /// [`Renderer::draw`] in the planes the caller gives: those of `view`
    /// with the setup's near plane, `view.clip_planes_at(C::NEAR)`. For a
    /// view that never changes they can be a constant worked out while
    /// compiling (CAMBER's); then the renderer never works them out and
    /// carries no code for it.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_in<H: Shade>(
        &mut self,
        f: &mut Frame,
        view: &View,
        planes: &Planes,
        cam: &Camera,
        shading: &H,
        o: &Object<'_>,
        far: i32,
    ) -> bool {
        // Forget the cached view: these planes may be another view's.
        self.seen = None;
        self.planes = *planes;
        self.draw_object(f, view, cam, shading, o, far)
    }

    /// The drawing itself, in `self.planes`.
    #[inline(always)]
    fn draw_object<H: Shade>(
        &mut self,
        f: &mut Frame,
        view: &View,
        cam: &Camera,
        shading: &H,
        o: &Object<'_>,
        far: i32,
    ) -> bool {
        type S<C> = <C as Setup>::Sums;
        let m = o.model;
        // A scaled radius is rounded down, so one more keeps the ball
        // round every corner.
        let radius = if C::SCALE {
            S::<C>::add(S::<C>::mul(m.radius, o.scale) / 256, 1)
        } else {
            m.radius
        };
        let Some(centre) = in_view::<C>(&self.planes, cam, o.pos, radius, far) else {
            return false;
        };
        let nv = m.verts.len().min(V);
        let planes = self.planes.p;
        // Model to camera: the camera's turn after the object's, scaled.
        let to_cam = cam.rot.mul_with::<S<C>>(&o.rot);
        let to_cam = if C::SCALE {
            to_cam.scaled_with::<S<C>>(o.scale)
        } else {
            to_cam
        };
        // Is the whole ball inside every plane? Then nothing is cut.
        let all_in = C::INSIDE_SHORTCUT
            && S::<C>::sub(centre[2], radius) >= C::NEAR
            && (1..5).all(|k| {
                self.planes.side::<S<C>>(k, centre)
                    >= S::<C>::mul64(i64::from(radius), self.planes.len[k])
            });
        for i in 0..nv {
            let v = m.verts[i];
            let p = to_cam.apply_with::<S<C>>([i32::from(v[0]), i32::from(v[1]), i32::from(v[2])]);
            let p = [
                S::<C>::add(p[0], centre[0]),
                S::<C>::add(p[1], centre[1]),
                S::<C>::add(p[2], centre[2]),
            ];
            self.cs[i] = p;
            let mut out = 0u8;
            if !all_in {
                for (k, pl) in planes.iter().enumerate() {
                    if plane_dist_with::<S<C>>(pl, &p) < 0 {
                        out |= 1 << k;
                    }
                }
            }
            self.out[i] = out;
            if out == 0 {
                let s = C::project(view, p);
                self.stats.projected::<S<C>>();
                self.sp[i] = [s[0] as i16, s[1] as i16];
            }
        }
        self.stats.object::<S<C>>();
        self.stats.verts::<S<C>>(nv as u32);
        let mut nt = 0usize;
        for (fi, face) in m.faces.iter().enumerate() {
            self.stats.face::<S<C>>();
            let [ia, ib, ic] = face.v.map(usize::from);
            if ia >= nv || ib >= nv || ic >= nv {
                continue;
            }
            let (oa, ob, oc) = (self.out[ia], self.out[ib], self.out[ic]);
            if oa & ob & oc != 0 {
                continue;
            }
            let (a, b, c) = (self.cs[ia], self.cs[ib], self.cs[ic]);
            let front = match C::CULL {
                Cull::Corners => faces_eye::<S<C>>(a, b, c),
                Cull::Normal => {
                    let nm = m.normals[fi];
                    let nm = [i32::from(nm[0]), i32::from(nm[1]), i32::from(nm[2])];
                    faces_eye_normal::<S<C>>(to_cam.apply_with::<S<C>>(nm), a)
                }
            };
            if !front {
                continue;
            }
            self.stats.front::<S<C>>();
            let nm = m.normals[fi];
            let nw =
                o.rot
                    .apply_with::<S<C>>([i32::from(nm[0]), i32::from(nm[1]), i32::from(nm[2])]);
            let depth = C::depth(a[2], b[2], c[2]);
            let mat = if face.mat == o.swap.0 {
                o.swap.1
            } else {
                face.mat
            };
            let colour = shading.paint::<S<C>>(mat, nw, depth);
            if oa | ob | oc == 0 {
                let t = Tri {
                    p: [self.sp[ia], self.sp[ib], self.sp[ic]],
                    c: colour,
                };
                self.push(&mut nt, t, depth);
                continue;
            }
            // Cut by the planes the corners are outside of.
            self.stats.clipped::<S<C>>();
            let mut poly = [[0i32; 3]; P];
            poly[0] = a;
            poly[1] = b;
            poly[2] = c;
            let n = clip_polygon_with::<C, P>(&mut poly, 3, &planes, oa | ob | oc);
            if n < 3 {
                continue;
            }
            let mut sp = [[0i16; 2]; P];
            for k in 0..n {
                let s = C::project(view, poly[k]);
                self.stats.projected::<S<C>>();
                sp[k] = [
                    s[0].clamp(-GUARD, GUARD) as i16,
                    s[1].clamp(-GUARD, GUARD) as i16,
                ];
            }
            // A fan of triangles from the first corner.
            for k in 1..n - 1 {
                let t = Tri {
                    p: [sp[0], sp[k], sp[k + 1]],
                    c: colour,
                };
                self.push(&mut nt, t, depth);
            }
        }
        // Far to near (insertion sort: a few dozen triangles).
        let keys = &mut self.keys[..nt];
        for i in 1..keys.len() {
            let k = keys[i];
            let mut j = i;
            while j > 0 && keys[j - 1] < k {
                keys[j] = keys[j - 1];
                j -= 1;
            }
            keys[j] = k;
        }
        for i in 0..nt {
            let t = self.tris[(self.keys[i] & 0xFF) as usize];
            let p = t.p.map(|q| [i32::from(q[0]), i32::from(q[1])]);
            fill_into::<S<C>, K>(f, &view.area, &p, t.c, &mut self.stats);
        }
        true
    }

    fn push(&mut self, nt: &mut usize, t: Tri, depth: i32) {
        if *nt >= T {
            self.stats.dropped::<C::Sums>();
            return;
        }
        self.tris[*nt] = t;
        self.keys[*nt] = ((depth.clamp(0, 0xFF_FFFF) as u32) << 8) | *nt as u32;
        *nt += 1;
    }
}

/// Whether the front of the face a, b, c (camera space) looks at the
/// eye (at the origin): its direction points back towards the eye.
/// ([`Cull::Corners`].)
pub fn faces_eye<S: Sums>(a: [i32; 3], b: [i32; 3], c: [i32; 3]) -> bool {
    let u = [
        i64::from(S::sub(b[0], a[0])),
        i64::from(S::sub(b[1], a[1])),
        i64::from(S::sub(b[2], a[2])),
    ];
    let w = [
        i64::from(S::sub(c[0], a[0])),
        i64::from(S::sub(c[1], a[1])),
        i64::from(S::sub(c[2], a[2])),
    ];
    let n = [
        S::sub64(S::mul64(u[1], w[2]), S::mul64(u[2], w[1])),
        S::sub64(S::mul64(u[2], w[0]), S::mul64(u[0], w[2])),
        S::sub64(S::mul64(u[0], w[1]), S::mul64(u[1], w[0])),
    ];
    S::add64(
        S::add64(
            S::mul64(n[0], i64::from(a[0])),
            S::mul64(n[1], i64::from(a[1])),
        ),
        S::mul64(n[2], i64::from(a[2])),
    ) < 0
}

/// Whether a face with the direction `n` (camera space, 1/16384) and a
/// corner at `a` (camera space) shows its front to the eye.
/// ([`Cull::Normal`].)
pub fn faces_eye_normal<S: Sums>(n: [i32; 3], a: [i32; 3]) -> bool {
    S::add64(
        S::add64(S::wide(n[0], a[0]), S::wide(n[1], a[1])),
        S::wide(n[2], a[2]),
    ) < 0
}
