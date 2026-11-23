//! The 3D picture: the backdrop, the track, the scenery and the ships.
//!
//! The track is drawn from the far end towards the camera, one segment
//! at a time; the track's own order is the painter's order. Each
//! boundary is turned into the camera's view once ([`Bnd`]) and its
//! corners are shared by every polygon that touches them. Inside one
//! segment the order is: road, inner wall faces and roof; the ships on
//! that segment (shadow, trail, hull); the outer faces (walls from
//! outside, the underside); then what stands on the segment's near
//! boundary (posts, arches, the gantry, a tunnel mouth, pillars, trees).
//!
//! The haze grows in four steps with the distance in segments, so the
//! far end of the track fades into the sky instead of popping in.

use game_kit::{Frame, H, W};

use crate::art::{
    self, BOOSTM, CHARGEM, CHECK, ENGINE, ENGINE_HOT, Flora, LAMP, METAL, ROAD, ROOFM, SCEN_A,
    SCEN_B, SHADOW, STRIPE, THEMES, Theme, UNDER, WALL, mat,
};
use crate::fixed::{Mat3, ONE14, atan2, cos, hash, sin};
use crate::poly::{
    Camber, Camera, MAX_POLY, NEAR as NEAR_PLANE, Object, Planes, Pt, Rect, Renderer, Stats, Steps,
    View, draw_poly,
};
use crate::race::{HOVER, Race, SHIPS, Ship, heat_step};
use crate::track::{
    ARCH, BOOST, CHARGE, FM, GANTRY, GAP, LIGHTS, PILLAR, PORTAL, ROOF, SCEN_L, SCEN_R, SEG_BITS,
    Track, WALL_L, WALL_R,
};

/// The lens: 1 m at 1 m is this many pixels (about 80 degrees across).
pub const FOCAL: i32 = 96;
/// The whole picture.
pub const VIEW: View = View::new(Rect::new(0, 0, W as i32, H as i32), FOCAL);
/// Its five planes, worked out while compiling.
const PLANES: Planes = VIEW.clip_planes_at(NEAR_PLANE);

/// One metre in world units.
const M: i32 = 256;
const WALL_H: i32 = M * 11 / 10;
const ROOF_Y: i32 = M * 52 / 10;
const SKIRT: i32 = M * 12 / 10;
const STRIPE_W: i32 = M * 8 / 10;
const CHARGE_W: i32 = M * 35 / 10;

// ---- Settings ---------------------------------------------------------------

/// How much is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    Low,
    Mid,
    High,
}

impl Detail {
    pub const ALL: [Detail; 3] = [Detail::Low, Detail::Mid, Detail::High];

    pub fn name(self) -> &'static str {
        match self {
            Detail::Low => "LOW",
            Detail::Mid => "MID",
            Detail::High => "HIGH",
        }
    }

    /// Segments drawn ahead of the camera.
    pub fn segs(self) -> i32 {
        match self {
            Detail::Low => 16,
            Detail::Mid => 24,
            Detail::High => 32,
        }
    }

    /// Segments with edge stripes, posts and arches.
    fn near(self) -> i32 {
        match self {
            Detail::Low => 6,
            Detail::Mid => 14,
            Detail::High => 32,
        }
    }

    /// Segments with trees and rocks.
    fn flora(self) -> i32 {
        match self {
            Detail::Low => 0,
            Detail::Mid => 14,
            Detail::High => 28,
        }
    }
}

/// Where the camera is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    /// Behind and above the ship.
    Chase,
    /// Further back and higher.
    Far,
    /// From the nose, no ship in the picture.
    Nose,
    /// The title's still shot of the grid.
    Show,
}

impl ViewKind {
    pub const ALL: [ViewKind; 3] = [ViewKind::Chase, ViewKind::Far, ViewKind::Nose];

    pub fn name(self) -> &'static str {
        match self {
            ViewKind::Chase => "CHASE",
            ViewKind::Far => "FAR",
            ViewKind::Nose => "NOSE",
            ViewKind::Show => "SHOW",
        }
    }

    /// Back, up (fine units) and the downward look (angle units).
    fn place(self) -> (i32, i32, i32) {
        match self {
            ViewKind::Chase => (FM * 65 / 10, FM * 21 / 10, 1_000),
            ViewKind::Far => (FM * 11, FM * 36 / 10, 1_500),
            ViewKind::Nose => (-FM * 15 / 10, FM * 5 / 10, 200),
            ViewKind::Show => (FM * 20, FM * 7, 2_000),
        }
    }
}

/// The camera, following the player's ship with a little lag (moved
/// once per tick, so 30 and 60 frames a second show the same).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cam {
    pub s: i32,
    pub x: i32,
    pub h: i32,
    pub yaw: i32,
    pub pitch: i32,
    pub roll: i32,
    /// False until the first follow (then it jumps into place).
    pub set: bool,
}

impl Cam {
    pub const fn new() -> Cam {
        Cam {
            s: 0,
            x: 0,
            h: 0,
            yaw: 0,
            pitch: 0,
            roll: 0,
            set: false,
        }
    }

    /// One tick of following ship `sh`.
    pub fn follow(&mut self, tr: &Track, sh: &Ship, kind: ViewKind) {
        let (back, up, down) = kind.place();
        let s = sh.s - back;
        let at = tr.at(s);
        let ahead = tr.at(sh.s + FM * 14);
        let lean = (sh.rate * 5).clamp(-2_400, 2_400);
        let half = at.half * 256;
        let tx = (sh.x * 9 / 10).clamp(-half + FM / 2, (half - FM / 2).max(-half + FM / 2));
        let th = sh.h.max(-FM * 2) + up;
        let tyaw = sh.psi / 2;
        let tpitch = (at.pitch + ahead.pitch) / 2 - down;
        let troll = at.bank
            + if kind == ViewKind::Nose {
                lean
            } else {
                lean / 3
            };
        if !self.set || kind == ViewKind::Show {
            *self = Cam {
                s,
                x: tx,
                h: th,
                yaw: tyaw,
                pitch: tpitch,
                roll: troll,
                set: true,
            };
        } else {
            self.s = s;
            self.x += (tx - self.x) / 5;
            self.h += (th - self.h) / 4;
            self.yaw += (tyaw - self.yaw) / 6;
            self.pitch += (tpitch - self.pitch) / 5;
            self.roll += (troll - self.roll) / 6;
        }
        if kind == ViewKind::Nose {
            self.x = sh.x;
            self.yaw = sh.psi;
        }
        // Inside a tunnel, below its roof.
        let seg = tr.seg(at.seg);
        if seg.kind & ROOF != 0 {
            self.h = self.h.min(FM * 46 / 10);
        }
        self.h = self.h.max(FM / 3);
    }

    /// The camera as the renderer wants it, and its heading.
    pub fn camera(&self, tr: &Track) -> (Camera, u16) {
        let at = tr.at(self.s);
        let pos = world_at(tr, self.s, self.x, self.h);
        let yaw = (i32::from(at.yaw) + self.yaw) as u16;
        let rot = Mat3::rotation_fine(yaw, self.pitch as u16, self.roll as u16).transposed();
        (Camera { pos, rot }, yaw)
    }
}

impl Default for Cam {
    fn default() -> Self {
        Cam::new()
    }
}

// ---- The backdrop -------------------------------------------------------------

const BAYER: [[i32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Sky, sun, stars, far hills and the ground (or sea) to the horizon.
pub fn backdrop(f: &mut Frame, cam: &Camera, yaw: u16, th: &Theme, st: &mut Stats) {
    // The world's up in camera space; a pixel's ray is (dx, -dy, focal).
    let wu = cam.rot.apply([0, ONE14, 0]);
    let (cx, cy) = (W as i32 / 2, H as i32 / 2);
    for y in 0..H as i32 {
        let dy = cy * 2 - 2 * y - 1;
        let e0 = (wu[0] * (1 - 2 * cx) + wu[1] * dy + wu[2] * 2 * FOCAL) / 2;
        let row = f.row_mut(y as usize);
        let b = &BAYER[(y & 3) as usize];
        let mut e = e0;
        for (x, px) in row.iter_mut().enumerate() {
            let d = b[x & 3];
            *px = if e >= 0 {
                let k = (e * 18 + (d << 16)) >> 20;
                art::SKY + k.min(art::SKY_STEPS - 1) as u8
            } else {
                let k = ((-e) * 27 + (d << 18)) >> 22;
                art::GROUND + k.min(3) as u8
            };
            e += wu[0];
        }
    }
    st.pixels += (W * H) as u32;
    // Stars and the sun (only above the horizon).
    if th.stars {
        for k in 0..60u32 {
            let hh = hash(k, 7);
            let a = (hh & 0xFFFF) as u16;
            let up = ((hh >> 16) % 12_000 + 600) as u16;
            if let Some((x, y)) = sky_point(cam, a, up) {
                f.pixel(x, y, art::STAR);
            }
        }
    }
    if let Some((x, y)) = sky_point(cam, th.sun_yaw, th.sun_up) {
        let r = th.sun_r;
        disc(f, x, y, r + 2, art::SUN_RING);
        disc(f, x, y, r, art::SUN);
    }
    // Far hills along the horizon: two rows, the nearer lower and darker.
    if wu[1] > 2_000 {
        for x in 0..W as i32 {
            let dx2 = 2 * x + 1 - 2 * cx;
            // The horizon row in this column (where e = 0).
            let yh2 = 2 * cy - (-(wu[0] * dx2) - wu[2] * 2 * FOCAL) / wu[1];
            let yh = yh2.div_euclid(2);
            let az = i32::from(yaw) + i32::from(COLUMN_AZ[x as usize]);
            for (layer, (seed, scale)) in [(0usize, (th.hill_seed, 10)), (1, (th.hill_seed + 5, 6))]
            {
                let hgt = hill(seed, az + layer as i32 * 9_000) * th.hill_h * scale / (255 * 10);
                if hgt > 0 {
                    let top = (yh - hgt).max(0);
                    let bot = (yh + 1).min(H as i32);
                    if top < bot {
                        f.vline(
                            x as i16,
                            top as i16,
                            (bot - top) as u16,
                            art::HILL + layer as u8,
                        );
                    }
                }
            }
        }
    }
}

/// The heading of each column's middle seen from the eye, in 1/65536 of
/// a turn (atan of the column's offset over the focal length, made at
/// build time), so the far hills turn with the stars and the road.
static COLUMN_AZ: [i16; W] = column_az();

const fn column_az() -> [i16; W] {
    let mut t = [0i16; W];
    let mut x = 0;
    while x < W {
        let dx2 = 2 * x as i64 + 1 - W as i64;
        t[x] = atan2(dx2, 2 * FOCAL as i64) as i16;
        x += 1;
    }
    t
}

/// Height 0..255 of a range of hills at heading `az` (smooth, repeats
/// once a turn).
fn hill(seed: u32, az: i32) -> i32 {
    let a = (az as u32 & 0xFFFF) as i32;
    let i = a >> 10;
    let fr = a & 1023;
    let h0 = (hash(seed, i as u32) & 255) as i32;
    let h1 = (hash(seed, ((i + 1) & 63) as u32) & 255) as i32;
    let g0 = (hash(seed + 1, (i >> 2) as u32) & 255) as i32;
    let g1 = (hash(seed + 1, (((i >> 2) + 1) & 15) as u32) & 255) as i32;
    let fine = h0 + (h1 - h0) * fr / 1024;
    let fr2 = (a & 4095) * 1024 / 4096;
    let broad = g0 + (g1 - g0) * fr2 / 1024;
    (fine / 3 + broad * 2 / 3).clamp(0, 255) * broad / 255
}

fn sky_point(cam: &Camera, yaw: u16, up: u16) -> Option<(i16, i16)> {
    let cu = cos(up);
    let d = [(sin(yaw) * cu) >> 14, sin(up), (cos(yaw) * cu) >> 14];
    let c = cam.rot.apply(d);
    if c[2] < 2_000 {
        return None;
    }
    let x = W as i32 / 2 + c[0] * FOCAL / c[2];
    let y = H as i32 / 2 - c[1] * FOCAL / c[2];
    ((-10..W as i32 + 10).contains(&x) && (-10..H as i32 + 10).contains(&y))
        .then_some((x as i16, y as i16))
}

fn disc(f: &mut Frame, x: i16, y: i16, r: i32, c: u8) {
    for dy in -r..=r {
        let w = crate::fixed::isqrt((r * r - dy * dy) as u64) as i16;
        f.hline(x - w, y + dy as i16, (2 * w + 1) as u16, c);
    }
}

// ---- The track -------------------------------------------------------------------

/// One boundary in the camera's view.
#[derive(Clone, Copy, Default)]
struct Bnd {
    /// Middle, right and up (unit, 1/16384) in camera space.
    c: [i32; 3],
    r: [i32; 3],
    u: [i32; 3],
    half: i32,
    /// Road edges, stripe edges, low wall tops, tunnel tops, skirts, the
    /// recharge lane's edge.
    l: Pt,
    rr: Pt,
    li: Pt,
    ri: Pt,
    wl: Pt,
    wr: Pt,
    tl: Pt,
    tr: Pt,
    sl: Pt,
    sr: Pt,
    ci: Pt,
}

/// Road points of a boundary, left to right, as bits: the road's edges,
/// the stripes' inner edges and the recharge lane's edge.
const P_L: u8 = 1;
const P_LI: u8 = 2;
const P_CI: u8 = 4;
const P_RI: u8 = 8;
const P_RR: u8 = 16;

impl Bnd {
    /// Road point `bit` (one of the `P_` bits).
    fn road(&self, bit: u8) -> &Pt {
        match bit {
            P_L => &self.l,
            P_LI => &self.li,
            P_CI => &self.ci,
            P_RI => &self.ri,
            _ => &self.rr,
        }
    }
}

/// The road points segment `kind` cuts its road at (drawn with the
/// stripes when `near`).
fn road_points(kind: u8, near: bool) -> u8 {
    if kind & GAP != 0 {
        0
    } else if kind & CHARGE != 0 {
        P_L | P_CI | P_RR
    } else if near {
        P_L | P_LI | P_RI | P_RR
    } else {
        P_L | P_RR
    }
}

#[inline(never)]
fn off(b: &Bnd, x: i32, y: i32) -> [i32; 3] {
    [
        b.c[0] + ((b.r[0] * x + b.u[0] * y) >> 14),
        b.c[1] + ((b.r[1] * x + b.u[1] * y) >> 14),
        b.c[2] + ((b.r[2] * x + b.u[2] * y) >> 14),
    ]
}

#[inline(never)]
fn dot(a: [i32; 3], b: [i32; 3]) -> i64 {
    i64::from(a[0]) * i64::from(b[0])
        + i64::from(a[1]) * i64::from(b[1])
        + i64::from(a[2]) * i64::from(b[2])
}

/// Everything one picture's drawing needs.
struct Ctx<'a> {
    f: &'a mut Frame,
    cam: Camera,
    tr: &'a Track,
    /// The ships' renderer; its counts are the whole picture's.
    rn: &'a mut Renderer,
    /// The world's up in camera space.
    wu: [i32; 3],
    flora: Flora,
    /// The title's lone turning ship (the SHIP backdrop): its shadow
    /// turns with it.
    spin: bool,
}

impl Ctx<'_> {
    #[inline(never)]
    fn pt(&mut self, c: [i32; 3]) -> Pt {
        Pt::new::<Camber>(c, &VIEW, &PLANES, &mut self.rn.stats)
    }

    #[inline(never)]
    fn poly(&mut self, pts: &[&Pt], c: u8) {
        draw_poly::<Camber, MAX_POLY>(self.f, &VIEW, &PLANES, pts, c, &mut self.rn.stats);
    }

    /// The road between road points `lo` and `hi` (`P_` bits) of two
    /// boundaries, through every point either boundary has between them
    /// (`en`, `ef`): a neighbour that cuts a boundary at more points
    /// then shares each of them, and no pixel row slips between the two.
    #[inline(never)]
    fn strip(&mut self, (n, en): (&Bnd, u8), (f, ef): (&Bnd, u8), lo: u8, hi: u8, c: u8) {
        let mut r: [&Pt; 10] = [&n.l; 10];
        let mut m = 0;
        let mut bit = lo;
        while bit <= hi {
            if (en | lo | hi) & bit != 0 {
                r[m] = n.road(bit);
                m += 1;
            }
            bit <<= 1;
        }
        let mut bit = hi;
        while bit >= lo {
            if (ef | lo | hi) & bit != 0 {
                r[m] = f.road(bit);
                m += 1;
            }
            bit >>= 1;
        }
        self.poly(&r[..m], c);
    }

    /// `base` moved `h` up the world (camera space).
    #[inline(never)]
    fn up(&self, base: [i32; 3], h: i32) -> [i32; 3] {
        [
            base[0] + ((self.wu[0] * h) >> 14),
            base[1] + ((self.wu[1] * h) >> 14),
            base[2] + ((self.wu[2] * h) >> 14),
        ]
    }

    /// An upright box `x0..x1` across and `y0..y1` up in boundary `b`'s
    /// plane.
    #[inline(never)]
    fn rect(&mut self, b: &Bnd, x0: i32, y0: i32, x1: i32, y1: i32, c: u8) {
        self.poly_c(
            &[
                off(b, x0, y0),
                off(b, x1, y0),
                off(b, x1, y1),
                off(b, x0, y1),
            ],
            c,
        );
    }

    /// A polygon of camera-space points made on the spot.
    #[inline(never)]
    fn poly_c(&mut self, pts: &[[i32; 3]], c: u8) {
        let mut p = [Pt::default(); 6];
        let n = pts.len().min(6);
        for k in 0..n {
            p[k] = self.pt(pts[k]);
        }
        let r: [&Pt; 6] = [&p[0], &p[1], &p[2], &p[3], &p[4], &p[5]];
        self.poly(&r[..n], c);
    }

    fn bnd(&mut self, j: usize, kinds: u8) -> Bnd {
        let s = *self.tr.seg(j);
        let m = Mat3::rotation_fine(s.yaw, s.pitch as u16, s.bank as u16);
        let c = self.cam.to_cam(s.p);
        let r = self.cam.rot.apply(m.col(0));
        let u = self.cam.rot.apply(m.col(1));
        let half = s.half_world();
        let mut b = Bnd {
            c,
            r,
            u,
            half,
            ..Bnd::default()
        };
        b.l = self.pt(off(&b, -half, 0));
        b.rr = self.pt(off(&b, half, 0));
        b.li = self.pt(off(&b, -half + STRIPE_W, 0));
        b.ri = self.pt(off(&b, half - STRIPE_W, 0));
        b.sl = self.pt(off(&b, -half, -SKIRT));
        b.sr = self.pt(off(&b, half, -SKIRT));
        if kinds & (WALL_L | WALL_R) != 0 {
            b.wl = self.pt(off(&b, -half, WALL_H));
            b.wr = self.pt(off(&b, half, WALL_H));
        }
        if kinds & ROOF != 0 {
            b.tl = self.pt(off(&b, -half, ROOF_Y));
            b.tr = self.pt(off(&b, half, ROOF_Y));
        }
        if kinds & CHARGE != 0 {
            b.ci = self.pt(off(&b, -half + CHARGE_W, 0));
        }
        b
    }
}

/// The 3D picture of a race, seen from `cam`. Each ship is drawn in the
/// turn of the segment under its tail, so nearer road, walls and crests
/// paint over it. From the nose the player's own ship is not drawn;
/// from behind, a rival passing the camera is left out (see below).
pub fn draw(
    f: &mut Frame,
    race: &Race,
    cam: &Cam,
    kind: ViewKind,
    detail: Detail,
    time: u32,
    st: &mut Stats,
) {
    let tr = race.tr();
    let th = &THEMES[usize::from(race.track) % THEMES.len()];
    let (camera, yaw) = cam.camera(tr);
    let wu = camera.rot.apply([0, ONE14, 0]);
    let mut rn = Renderer::new();
    let mut cx = Ctx {
        f,
        cam: camera,
        tr,
        rn: &mut rn,
        wu,
        flora: th.flora,
        spin: kind == ViewKind::Show && race.n == 1,
    };
    backdrop(cx.f, &cx.cam, yaw, th, &mut cx.rn.stats);
    let n = tr.n() as i32;
    let c0 = (cam.s.rem_euclid(tr.len()) >> SEG_BITS) - 1;
    let segs = detail.segs();
    // Segments ahead of the camera (`fw`, from the one behind it) and
    // behind it (`bk`): a race camera looks ahead; the title's cameras
    // may look back along the road, and then share the same number of
    // segments out by their heading against the track.
    let (fw, bk) = if kind == ViewKind::Show {
        let ahead = (segs * (ONE14 + cos(cam.yaw as u16)) / (2 * ONE14)).clamp(2, segs);
        (ahead, segs - ahead)
    } else {
        (segs, 0)
    };
    // Which ships are drawn in which segment's turn: the segment under
    // the tail, so all road under a ship is painted before it and all
    // that stands nearer (a wall, a tunnel mouth, a crest) after it
    // (behind the camera: under the nose, as a negative number).
    // From behind the ship, a rival passing the camera (from a segment
    // behind it to NEAR ahead) is left out: it would fill the picture
    // and hide the player's ship. The title's cameras leave out a ship
    // going through the camera.
    let mut on = [i32::MIN; SHIPS];
    for (i, sh) in race.ships.iter().enumerate().take(usize::from(race.n)) {
        let own = i == 0 && kind == ViewKind::Nose;
        let past = if kind == ViewKind::Show {
            (sh.s - cam.s + FM * 4).rem_euclid(tr.len()) < FM * 8 && (sh.x - cam.x).abs() < FM * 3
        } else {
            i != 0
                && kind != ViewKind::Nose
                && (sh.s - cam.s + FM * 8).rem_euclid(tr.len()) < FM * 8 + NEAR
        };
        if own || past || sh.fall > 30 || (sh.ghost > 0 && time & 2 != 0) {
            continue;
        }
        let k = (((sh.s - TAIL) >> SEG_BITS) - c0).rem_euclid(n);
        if k <= fw {
            on[i] = k;
        } else {
            let b = (c0 - ((sh.s + TAIL) >> SEG_BITS)).rem_euclid(n).max(1);
            if b <= bk {
                on[i] = -b;
            }
        }
    }
    let kind_at = |k: i32| tr.seg((c0 + k).rem_euclid(n) as usize).kind;
    // The road points of boundary `k` (shared by segments `k - 1` and
    // `k`): what either of them cuts its road at.
    let points = |k: i32| {
        road_points(kind_at(k), k.abs() <= detail.near())
            | road_points(kind_at(k - 1), (k - 1).abs() <= detail.near())
    };
    let mut far = cx.bnd(
        (c0 + fw + 1).rem_euclid(n) as usize,
        kind_at(fw) | kind_at(fw + 1),
    );
    let mut far_pts = points(fw + 1);
    // Behind the camera: the far boundary is the lower one.
    let mut back = Bnd::default();
    let mut back_pts = 0;
    if bk > 0 {
        back = cx.bnd(
            (c0 - bk).rem_euclid(n) as usize,
            kind_at(-bk) | kind_at(-bk - 1),
        );
        back_pts = points(-bk);
    }
    for d in (0..=fw.max(bk)).rev() {
        let haze = ((d - 1).max(0) * 4 / segs).min(3) as u8;
        let close = d <= detail.near();
        let plain = d > detail.near() / 2 && detail != Detail::High;
        if d >= 1 && d <= bk {
            // Segment `-d` lies between boundary `-d` (far) and `1 - d`.
            let j = (c0 - d).rem_euclid(n) as usize;
            let near = cx.bnd(
                (c0 - d + 1).rem_euclid(n) as usize,
                kind_at(-d) | kind_at(1 - d),
            );
            let near_pts = points(1 - d);
            let seg = *tr.seg(j);
            // In the track's order, so boost arrows point the right way.
            let outer = segment(
                &mut cx,
                j,
                seg.kind,
                (&back, back_pts),
                (&near, near_pts),
                haze,
                close,
                time,
                seg.pad,
            );
            ships_on(&mut cx, race, &on, -d, time, plain);
            outer_faces(&mut cx, &back, &near, seg.kind, outer, haze);
            if d > 1 {
                deco(
                    &mut cx,
                    (c0 - d + 1).rem_euclid(n) as usize,
                    &near,
                    haze,
                    d - 1,
                    detail,
                );
            }
            back = near;
            back_pts = near_pts;
        }
        if d > fw {
            continue;
        }
        let k = d;
        let j = (c0 + k).rem_euclid(n) as usize;
        let near = cx.bnd(j, kind_at(k) | kind_at(k - 1));
        let near_pts = points(k);
        let seg = *tr.seg(j);
        let outer = segment(
            &mut cx,
            j,
            seg.kind,
            (&near, near_pts),
            (&far, far_pts),
            haze,
            close,
            time,
            seg.pad,
        );
        ships_on(&mut cx, race, &on, k, time, plain);
        outer_faces(&mut cx, &near, &far, seg.kind, outer, haze);
        deco(&mut cx, j, &near, haze, k, detail);
        far = near;
        far_pts = near_pts;
    }
    *st = cx.rn.stats;
}

/// The ships drawn in the turn of segment `k` (see `on` in [`draw`]),
/// far to near: ahead of the camera the furthest along first, behind
/// it the least far along first.
fn ships_on(cx: &mut Ctx<'_>, race: &Race, on: &[i32; SHIPS], k: i32, time: u32, plain: bool) {
    let mut order = [0usize; SHIPS];
    let mut m = 0;
    for (i, &o) in on.iter().enumerate() {
        if o == k {
            order[m] = i;
            m += 1;
        }
    }
    // By raw position: two ships in one segment either side of the
    // start line (within 5 m before and 3 m after it) are put in the
    // wrong order for a moment. Fixing it cost about 200 bytes of
    // code (MEASURED, 2026-11-18), so it is left.
    for a in 1..m {
        let mut b = a;
        while b > 0 && race.ships[order[b]].s < race.ships[order[b - 1]].s {
            order.swap(b, b - 1);
            b -= 1;
        }
    }
    if k < 0 {
        order[..m].reverse();
    }
    for &i in order[..m].iter().rev() {
        ship(cx, &race.ships[i], time, i, plain);
    }
}

/// How far behind its middle a ship is put for the drawing order (fine
/// units): past the hull's back (1.9 m) and most of the engine's trail.
const TAIL: i32 = FM * 3;
/// How far ahead of a camera behind the ship a rival must be to be drawn.
const NEAR: i32 = FM * 3;

/// What [`segment`] left for the outer pass.
#[derive(Clone, Copy, Default)]
struct Outer {
    under: bool,
    wall_l: bool,
    wall_r: bool,
    edge_l: bool,
    edge_r: bool,
    roof_top: bool,
}

/// The road, the inner wall faces and the roof of segment `j`.
#[allow(clippy::too_many_arguments)]
fn segment(
    cx: &mut Ctx<'_>,
    j: usize,
    kind: u8,
    nb: (&Bnd, u8),
    fb: (&Bnd, u8),
    haze: u8,
    near: bool,
    time: u32,
    pad: i8,
) -> Outer {
    let mut o = Outer::default();
    let (n, f) = (nb.0, fb.0);
    let alt = (j & 1) as u8;
    let roof = kind & ROOF != 0;
    if kind & GAP == 0 {
        if dot(n.u, n.l.c) < 0 || dot(f.u, f.l.c) < 0 {
            let road = mat(ROAD, haze, alt);
            if kind & CHARGE != 0 {
                let lane = mat(CHARGEM, haze, ((time >> 3) as u8 ^ alt) & 1);
                cx.strip(nb, fb, P_L, P_CI, lane);
                cx.strip(nb, fb, P_CI, P_RR, road);
            } else if near {
                let kerb = mat(STRIPE, haze, ((j >> 1) & 1) as u8);
                cx.strip(nb, fb, P_L, P_LI, kerb);
                cx.strip(nb, fb, P_LI, P_RI, road);
                cx.strip(nb, fb, P_RI, P_RR, kerb);
            } else {
                cx.strip(nb, fb, P_L, P_RR, road);
            }
            if kind & BOOST != 0 {
                chevrons(cx, n, f, i32::from(pad) * 128, haze, time);
            }
        } else {
            o.under = true;
        }
    }
    let wall = if roof { ROOFM } else { WALL };
    if kind & WALL_L != 0 {
        let top = if roof { (&n.tl, &f.tl) } else { (&n.wl, &f.wl) };
        if dot(n.r, n.l.c) < 0 {
            cx.poly(&[&n.l, &f.l, top.1, top.0], mat(wall, haze, 1));
        } else {
            o.wall_l = true;
        }
    } else {
        o.edge_l = dot(n.r, n.l.c) > 0;
    }
    if kind & WALL_R != 0 {
        let top = if roof { (&n.tr, &f.tr) } else { (&n.wr, &f.wr) };
        if dot(n.r, n.rr.c) > 0 {
            cx.poly(&[&n.rr, &f.rr, top.1, top.0], mat(wall, haze, 0));
        } else {
            o.wall_r = true;
        }
    } else {
        o.edge_r = dot(n.r, n.rr.c) < 0;
    }
    if roof {
        if dot(n.u, n.tl.c) > 0 {
            cx.poly(&[&n.tl, &n.tr, &f.tr, &f.tl], mat(ROOFM, haze, 2));
            // A strip of lights along the roof, every other segment.
            if near && j & 1 == 0 {
                let a = off(n, -M / 2, ROOF_Y - 8);
                let b = off(n, M / 2, ROOF_Y - 8);
                let c = off(f, M / 2, ROOF_Y - 8);
                let d = off(f, -M / 2, ROOF_Y - 8);
                cx.poly_c(&[a, b, c, d], mat(LAMP, haze, 0));
            }
        } else {
            o.roof_top = true;
        }
    }
    o
}

/// Two arrows on a boost pad, flashing forwards.
fn chevrons(cx: &mut Ctx<'_>, n: &Bnd, f: &Bnd, x: i32, haze: u8, time: u32) {
    let lift = 6;
    for k in 0..2 {
        let t0 = k * 8;
        let at = |t: i32, dx: i32| lerp16(off(n, x + dx, lift), off(f, x + dx, lift), t);
        let w = M * 16 / 10;
        let tip = at(t0 + 7, 0);
        let bl = at(t0 + 1, -w);
        let br = at(t0 + 1, w);
        let notch = at(t0 + 4, 0);
        let light = ((time >> 2) as i32 + k) as u8 & 1;
        let c = mat(BOOSTM, haze, light);
        cx.poly_c(&[bl, tip, notch], c);
        cx.poly_c(&[notch, tip, br], c);
    }
}

/// The faces seen from outside: walls, road edges, the underside, roofs
/// from above.
fn outer_faces(cx: &mut Ctx<'_>, n: &Bnd, f: &Bnd, kind: u8, o: Outer, haze: u8) {
    let c = mat(UNDER, haze, 1);
    let roof = kind & ROOF != 0;
    if o.under {
        cx.poly(&[&n.sl, &n.sr, &f.sr, &f.sl], mat(UNDER, haze, 2));
    }
    if o.wall_l {
        let top = if roof { (&n.tl, &f.tl) } else { (&n.wl, &f.wl) };
        cx.poly(&[&n.sl, &f.sl, top.1, top.0], c);
    } else if o.edge_l {
        cx.poly(&[&n.sl, &f.sl, &f.l, &n.l], c);
    }
    if o.wall_r {
        let top = if roof { (&n.tr, &f.tr) } else { (&n.wr, &f.wr) };
        cx.poly(&[&n.sr, &f.sr, top.1, top.0], mat(UNDER, haze, 0));
    } else if o.edge_r {
        cx.poly(&[&n.sr, &f.sr, &f.rr, &n.rr], mat(UNDER, haze, 0));
    }
    if o.roof_top {
        cx.poly(&[&n.tl, &n.tr, &f.tr, &f.tl], mat(ROOFM, haze, 0));
    }
}

// ---- Scenery -----------------------------------------------------------------------

/// A flat shape standing upright at `base` (camera space), facing the
/// camera: corners as (across, up) in world units.
#[inline(never)]
fn stand(cx: &mut Ctx<'_>, base: [i32; 3], shape: &[(i32, i32)], c: u8) {
    let mut p = [[0i32; 3]; 6];
    let n = shape.len().min(6);
    for k in 0..n {
        let (a, u) = shape[k];
        let q = cx.up(base, u);
        p[k] = [q[0] + a, q[1], q[2]];
    }
    cx.poly_c(&p[..n], c);
}

#[inline(never)]
fn post(cx: &mut Ctx<'_>, base: [i32; 3], w: i32, h: i32, c: u8) {
    stand(cx, base, &[(-w, 0), (w, 0), (w, h), (-w, h)], c);
}

/// What stands at boundary `j`.
fn deco(cx: &mut Ctx<'_>, j: usize, b: &Bnd, haze: u8, k: i32, detail: Detail) {
    let s = *cx.tr.seg(j);
    if s.deco == 0 || b.c[2] < -M * 30 {
        return;
    }
    let half = b.half;
    let metal = mat(METAL, haze, 0);
    let near = k <= detail.near();
    if s.deco & PILLAR != 0 {
        let top = off(b, 0, -SKIRT);
        post(cx, top, M, -M * 40, mat(UNDER, haze, 2));
    }
    if s.deco & (SCEN_L | SCEN_R) != 0 && k <= detail.flora() {
        for side in [-1i32, 1] {
            let bit = if side < 0 { SCEN_L } else { SCEN_R };
            if s.deco & bit == 0 {
                continue;
            }
            let hh = hash(j as u32, (side + 10) as u32);
            let dist = half + M * 5 + (hh & 2047) as i32 * 4;
            let base = off(b, side * dist, -M * 3);
            let kind = cx.flora;
            flora(cx, base, hh, haze, kind);
        }
    }
    if s.deco & LIGHTS != 0 && near {
        for side in [-1i32, 1] {
            let base = off(b, side * (half + M), 0);
            post(cx, base, M / 8, M * 6, metal);
            let top = cx.up(base, M * 6);
            stand(
                cx,
                top,
                &[
                    (-M / 3, -M / 4),
                    (M / 3, -M / 4),
                    (M / 3, M / 4),
                    (-M / 3, M / 4),
                ],
                mat(LAMP, haze, 0),
            );
        }
    }
    if s.deco & PORTAL != 0 {
        let wall = mat(WALL, haze, 1);
        let top = M * 10;
        let side = M * 9;
        cx.rect(b, -half - side, -M * 2, -half, top, wall);
        cx.rect(b, half, -M * 2, half + side, top, wall);
        cx.rect(b, -half, ROOF_Y, half, top, mat(WALL, haze, 0));
        // A band of lights over the mouth.
        cx.rect(
            b,
            -half,
            ROOF_Y + M / 2,
            half,
            ROOF_Y + M,
            mat(LAMP, haze, 1),
        );
    }
    if s.deco & (ARCH | GANTRY) != 0 && (near || s.deco & GANTRY != 0) {
        let gantry = s.deco & GANTRY != 0;
        let (hgt, leg, beam) = if gantry {
            (M * 8, M * 6 / 10, M * 18 / 10)
        } else {
            (M * 7, M * 4 / 10, M * 7 / 10)
        };
        let x = half + M * 12 / 10;
        for side in [-1i32, 1] {
            let base = off(b, side * x, -M);
            post(cx, base, leg, hgt + beam, metal);
        }
        if gantry {
            // A banner of checks across the road.
            let cells = 8;
            for c in 0..cells {
                let x0 = -x + 2 * x * c / cells;
                let x1 = -x + 2 * x * (c + 1) / cells;
                for row in 0..2 {
                    let y0 = hgt + beam * row / 2;
                    let y1 = hgt + beam * (row + 1) / 2;
                    let colour = mat(CHECK, haze, ((c + row) & 1) as u8);
                    cx.rect(b, x0, y0, x1, y1, colour);
                }
            }
        } else {
            cx.rect(b, -x, hgt, x, hgt + beam, mat(METAL, haze, 1));
            cx.rect(
                b,
                -x / 2,
                hgt + beam / 4,
                x / 2,
                hgt + beam * 3 / 4,
                mat(LAMP, haze, 1),
            );
        }
    }
}

/// A tree, a rock needle or an ice spire.
fn flora(cx: &mut Ctx<'_>, base: [i32; 3], hh: u32, haze: u8, kind: Flora) {
    let size = 8 + ((hh >> 12) & 7) as i32;
    let a = mat(SCEN_A, haze, ((hh >> 16) & 1) as u8);
    let b = mat(SCEN_B, haze, 0);
    match kind {
        Flora::Palms => {
            let h = M * size;
            post(cx, base, M / 3, h, b);
            let top = cx.up(base, h);
            stand(
                cx,
                top,
                &[
                    (-M * 3, 0),
                    (0, -M),
                    (M * 3, 0),
                    (M * 2, M),
                    (0, M * 3 / 2),
                    (-M * 2, M),
                ],
                a,
            );
        }
        Flora::Needles => {
            let h = M * size * 2;
            stand(
                cx,
                base,
                &[(-M * 3, 0), (M * 3, 0), (M / 2, h), (-M / 2, h * 9 / 10)],
                a,
            );
            if hh & 0x100 != 0 {
                let c = mat(SCEN_B, haze, 1);
                post(cx, [base[0] + M * 5, base[1], base[2]], M, h / 2, c);
            }
        }
        Flora::Firs => {
            if hh & 0x300 == 0 {
                let h = M * size * 2;
                stand(cx, base, &[(-M * 2, 0), (M * 2, 0), (0, h)], a);
            } else {
                let h = M * size;
                stand(
                    cx,
                    base,
                    &[(-M * 5 / 2, M), (M * 5 / 2, M), (0, h * 3 / 5)],
                    b,
                );
                let up = cx.up(base, h / 2);
                stand(cx, up, &[(-M * 2, 0), (M * 2, 0), (0, h * 3 / 5)], b);
            }
        }
    }
}

// ---- Ships ---------------------------------------------------------------------------

/// Where a ship is in the world and how it is turned.
#[inline(never)]
pub fn ship_pose(tr: &Track, sh: &Ship, time: u32, i: usize) -> ([i32; 3], Mat3, Mat3) {
    let at = tr.at(sh.s);
    let m = Mat3::rotation_fine(at.yaw, at.pitch as u16, at.bank as u16);
    let bob = sin((time.wrapping_mul(1_800) as u16).wrapping_add(i as u16 * 9_000)) >> 11;
    let pos = add3(at.p, m.apply([sh.x / 256, sh.h / 256 + bob, 0]));
    let lean = (sh.rate * 8).clamp(-2_800, 2_800);
    let nose = (sh.vh / 3).clamp(-1_800, 1_800);
    let rot = Mat3::rotation_fine(
        (i32::from(at.yaw) + sh.psi) as u16,
        (at.pitch + nose) as u16,
        (at.bank + lean) as u16,
    );
    (pos, rot, m)
}

#[inline(never)]
fn ship(cx: &mut Ctx<'_>, sh: &Ship, time: u32, i: usize, plain: bool) {
    let (pos, rot, road) = ship_pose(cx.tr, sh, time, i);
    // The shadow on the road, while low.
    if sh.h < HOVER * 5 && sh.fall == 0 && !plain {
        let at = cx.tr.at(sh.s);
        let x = sh.x / 256;
        // Turned with the ship's nose only for the title's turning ship
        // (a race's shadow keeps to the road, as it always has).
        let (m, x0, x1) = if cx.spin {
            let yaw = (i32::from(at.yaw) + sh.psi) as u16;
            let m = Mat3::rotation_fine(yaw, at.pitch as u16, at.bank as u16);
            (m, x, 0)
        } else {
            (road, 0, x)
        };
        let base = add3(at.p, road.apply([x0, 0, 0]));
        let corner = |a: i32, z: i32| cx.cam.to_cam(add3(base, m.apply([x1 + a, 5, z])));
        let w = M * 9 / 10;
        let q = [
            corner(-w / 3, M * 17 / 10),
            corner(w / 3, M * 17 / 10),
            corner(w, -M * 13 / 10),
            corner(-w, -M * 13 / 10),
        ];
        cx.poly_c(&q, SHADOW);
    }
    // The engine's trail behind the hull (long and hot on a boost pad's
    // push and on a sling).
    let hot = sh.boost > 0 || sh.sling > 0;
    if sh.ctl.thrust || hot {
        let len = if hot { M * 2 } else { M * 2 / 3 } + ((time & 3) as i32) * M / 8;
        let tc = cx
            .cam
            .to_cam(add3(pos, rot.apply([0, 0, -M * 17 / 10 - len])));
        let root = add3(pos, rot.apply([0, 0, -M * 16 / 10]));
        let rc = cx.cam.to_cam([root[0], root[1] + M / 3, root[2]]);
        let w = M / 3;
        let c = if hot { ENGINE_HOT } else { ENGINE };
        cx.poly_c(
            &[[rc[0] - w, rc[1], rc[2]], [rc[0] + w, rc[1], rc[2]], tc],
            c,
        );
    }
    // The sparks at every distance (a plain far ship too): a rival's
    // carve must show from afar.
    if sh.carve != 0 {
        sparks(cx, sh, pos, rot, time);
    }
    let look = Steps {
        base: [
            art::body(i as u8),
            art::trim(i as u8),
            art::CANOPY,
            art::DARK,
            if hot { ENGINE_HOT } else { ENGINE },
            0,
            0,
            0,
        ],
        glow: 1 << art::S_ENGINE,
        light: art::LIGHT,
    };
    let ship = Object {
        model: &art::SHIP,
        pos,
        rot,
        scale: 256,
        swap: (0, 0),
    };
    cx.rn
        .draw_in(cx.f, &VIEW, &PLANES, &cx.cam, &look, &ship, M * 400);
}

/// The carve's sparks: thrown from under the back of the hull at both
/// skids, drawn before the hull so they show only behind and beside it,
/// the outer side (the tail's side) longer, in the heat step's colour
/// ([`art::SPARK`]); each step is clearly longer and wider (about 0.3,
/// 0.7, 1.1 and 1.6 m before the flicker). They flicker with the game's
/// time. Beyond SPARK_NEAR from the eye they grow with the distance (up
/// to SPARK_GROW times), so a carving rival far ahead still shows a few
/// pixels of them.
/// Sparks keep their size up to this far from the eye (16 m), and then
/// grow with the distance, at most SPARK_GROW times (at 64 m and beyond).
const SPARK_NEAR: i32 = M * 16;
const SPARK_GROW: i32 = 4;

#[inline(never)]
fn sparks(cx: &mut Ctx<'_>, sh: &Ship, pos: [i32; 3], rot: Mat3, time: u32) {
    // Growth in 256ths: 1 up to SPARK_NEAR, then in step with the depth.
    let depth = cx.cam.to_cam(pos)[2].clamp(0, SPARK_NEAR * SPARK_GROW);
    let g = (depth * 256 / SPARK_NEAR).clamp(256, 256 * SPARK_GROW);
    let step = heat_step(sh.heat);
    let c = art::SPARK[usize::from(step)];
    let dir = i32::from(sh.carve);
    let s = i32::from(step);
    let flick = ((time.wrapping_add(sh.s as u32 >> 12) & 3) as i32) * M / 10;
    let reach = [3, 7, 11, 16][usize::from(step)];
    for side in [-1i32, 1] {
        // The tail swings out on the side away from the turn.
        let outer = side == -dir;
        let len = (M * reach / 10 + flick + if outer { M * 4 / 10 } else { 0 }) * g / 256;
        let root = add3(pos, rot.apply([side * M * 4 / 10, -M / 8, -M * 11 / 10]));
        let tip = add3(
            pos,
            rot.apply([
                side * (M * 6 / 10 + len / 3),
                (M / 6 + s * M / 10) * g / 256,
                -M * 13 / 10 - len,
            ]),
        );
        let rc = cx.cam.to_cam(root);
        let tc = cx.cam.to_cam(tip);
        let w = (M / 6 + s * M / 16) * g / 256;
        cx.poly_c(
            &[[rc[0] - w, rc[1], rc[2]], [rc[0] + w, rc[1] + w, rc[2]], tc],
            c,
        );
    }
}

/// A world point at `s` along the track, `x` across, `h` up (fine units).
#[inline(never)]
pub fn world_at(tr: &Track, s: i32, x: i32, h: i32) -> [i32; 3] {
    let at = tr.at(s);
    let m = Mat3::rotation_fine(at.yaw, at.pitch as u16, at.bank as u16);
    add3(at.p, m.apply([x / 256, h / 256, 0]))
}

/// From `a` towards `b`, `t` sixteenths of the way.
#[inline(never)]
fn lerp16(a: [i32; 3], b: [i32; 3], t: i32) -> [i32; 3] {
    [
        a[0] + (b[0] - a[0]) * t / 16,
        a[1] + (b[1] - a[1]) * t / 16,
        a[2] + (b[2] - a[2]) * t / 16,
    ]
}

/// `a + b`.
#[inline(never)]
fn add3(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
