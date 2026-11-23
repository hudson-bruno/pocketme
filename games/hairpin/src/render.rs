//! The 3D view: a road drawn row by row, the way the old arcade racers
//! did it ("pseudo-3D"), in whole numbers.
//!
//! How one picture is made:
//!
//! 1. **Project.** The camera sits behind (or in) the player's kart.
//!    The next [`DRAW`] segment ends ahead of it are worked out: how far
//!    away each is, how much the bends have moved the road sideways by
//!    then, and how high the road is there. Each end becomes a screen
//!    row, a road middle and a road width.
//! 2. **Ground, front to back.** Each segment fills the screen rows
//!    between its two ends: grass, then the road, the kerbs and the
//!    middle line. A row is drawn only once: a far segment hidden
//!    behind a hill top is skipped.
//! 3. **Backdrop.** The rows above the farthest ground: the sky, the
//!    sun or moon, the far mountains (which slide sideways in bends).
//! 4. **Sprites, back to front.** Trees, signs, posts and the karts, each
//!    shrunk with its distance and cut off where a nearer hill hides it.
//!
//! The view is smaller in the lighter details ([`Detail::view`]) and
//! leaves out the decorations that make most of the screen change
//! every frame (the grass bands), so the console has less to send.

use game_kit::Frame;

use crate::art::{
    BANNER, BLACK, CLEAR, FAR, FAR_LAND, FAR_LIT, GLOW, GRASS_A, GRASS_B, HILLS, KART,
    KART_LEAN_ROWS, KART_PIC, LINE, METAL, METAL_DARK, NEAR, NEAR_DARK, ORANGE, Pic, RIDGE, ROAD_A,
    ROAD_B, RUMBLE_A, RUMBLE_B, SEAT, SIDE, SKY_STEPS, STAR, SUN, TAIL, TAIL_ON, TEXT, TYRE,
    TYRE_HI, WHITE, course_base, ground,
};
use crate::race::{KARTS, Race, X_ONE, wrap};
use crate::track::{Kind, ROAD_HALF, RUMBLE_SEGS, SEG, Shoulder};

/// The middle of the screen, across.
pub const CX: i32 = 80;
/// The horizon row (where flat ground very far away would meet the sky).
pub const HZ: i32 = 40;
/// The lens: a thing 1 world unit wide, `D` units away, is 1 pixel wide.
pub const D: i32 = 72;
/// The chase camera's height above the road.
pub const CAM_H: i32 = 1100;
/// The nose camera's height (the driver's eyes).
pub const NOSE_H: i32 = 560;
/// The row under the player's kart in the chase view.
pub const KART_ROW: i32 = 97;
/// How far the chase camera is behind the kart, so the kart stands on
/// [`KART_ROW`].
pub const CAM_BACK: i32 = CAM_H * D / (KART_ROW - HZ);
/// Segments drawn ahead.
pub const DRAW: usize = 80;
/// Roadside things are drawn up to this many segments ahead.
pub const THINGS_DRAW: usize = 64;
/// A kart's width in world units (its picture is 32 pixels wide when
/// it is as far away as the player's kart).
pub const KART_WORLD_W: i32 = 32 * CAM_BACK / D;
/// The top bar's height: the view starts below it.
pub const TOP: i32 = 10;
/// Nearer than this a segment end is not projected (it would be huge).
const NEAR_Z: i32 = 30;

/// How much is drawn. The player can change it at any time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    /// A narrower view inside a frame, plain grass: the least to send.
    Light,
    /// The full width, bushes and near hills.
    Normal,
    /// The full screen, grass bands, edge lines.
    Heavy,
}

/// The part of the screen the view fills: columns `x0..x1`, rows
/// `y0..y1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    pub x0: i32,
    pub x1: i32,
    pub y0: i32,
    pub y1: i32,
}

impl Detail {
    pub const ALL: [Detail; 3] = [Detail::Light, Detail::Normal, Detail::Heavy];

    pub fn name(self) -> &'static str {
        match self {
            Detail::Light => "LIGHT",
            Detail::Normal => "NORMAL",
            Detail::Heavy => "HEAVY",
        }
    }

    pub fn view(self) -> View {
        match self {
            // 112 x 66: small enough that even a picture in which all of
            // it changes stays inside the send budget (about 8,000).
            Detail::Light => View {
                x0: 24,
                x1: 136,
                y0: 31,
                y1: 97,
            },
            Detail::Normal => View {
                x0: 0,
                x1: 160,
                y0: TOP,
                y1: 100,
            },
            Detail::Heavy => View {
                x0: 0,
                x1: 160,
                y0: TOP,
                y1: 120,
            },
        }
    }

    /// The road, the kerbs and the grass in alternating bands (the
    /// strongest feel of speed, but every road row changes every
    /// picture). LIGHT draws them in one colour each.
    pub fn bands(self) -> bool {
        self != Detail::Light
    }

    /// The mountains slide sideways in bends. In LIGHT the backdrop
    /// stays put, so the sky costs nothing to send.
    pub fn backdrop_turns(self) -> bool {
        self != Detail::Light
    }

    /// The next one, round.
    pub fn next(self) -> Detail {
        match self {
            Detail::Light => Detail::Normal,
            Detail::Normal => Detail::Heavy,
            Detail::Heavy => Detail::Light,
        }
    }
}

/// Where the camera is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cam {
    /// Behind and above the kart: you see your kart.
    Chase,
    /// From the driver's seat.
    Nose,
}

impl Cam {
    pub const ALL: [Cam; 2] = [Cam::Chase, Cam::Nose];

    pub fn name(self) -> &'static str {
        match self {
            Cam::Chase => "CHASE",
            Cam::Nose => "NOSE",
        }
    }

    pub fn next(self) -> Cam {
        match self {
            Cam::Chase => Cam::Nose,
            Cam::Nose => Cam::Chase,
        }
    }
}

/// What the last picture took (for the log and the tests).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cost {
    /// Segments that put at least one row on screen.
    pub segs: u16,
    /// Ground rows drawn.
    pub rows: u16,
    /// Sprites drawn (things and karts).
    pub sprites: u16,
    /// Sprite pixels written.
    pub sprite_px: u32,
}

/// One segment end, projected.
#[derive(Clone, Copy)]
struct End {
    /// Distance ahead of the camera (world units).
    z: i32,
    /// Road middle across the screen, in 1/16 pixel.
    cx16: i32,
    /// Screen row, in 1/16 pixel.
    y16: i32,
    /// Size: pixels per world unit, in 1/65536.
    s: i32,
    /// The lowest row still free when this end's segment was reached
    /// (things here are hidden below it).
    clip: i16,
}

const END0: End = End {
    z: 0,
    cx16: 0,
    y16: 0,
    s: 0,
    clip: 0,
};

/// Colours that change for a kart picture.
#[derive(Clone, Copy)]
struct Remap {
    body: u8,
    dark: u8,
    tail: u8,
}

const SAME: Remap = Remap {
    body: KART,
    dark: KART + 1,
    tail: TAIL,
};

impl Remap {
    #[inline(always)]
    fn of(&self, p: u8) -> u8 {
        if p == KART {
            self.body
        } else if p == KART + 1 {
            self.dark
        } else if p == TAIL {
            self.tail
        } else {
            p
        }
    }
}

/// The colours of kart `k`.
fn kart_remap(k: usize, braking: bool) -> Remap {
    Remap {
        body: KART + 2 * k as u8,
        dark: KART + 2 * k as u8 + 1,
        tail: if braking { TAIL_ON } else { TAIL },
    }
}

/// Draws the 3D view of `race` into its part of the screen.
pub fn draw(s: &mut Frame, race: &Race, detail: Detail, cam: Cam, cost: &mut Cost) {
    *cost = Cost::default();
    let v = detail.view();
    let course = race.course();
    let cno = usize::from(race.course);
    let lap_z = course.lap_z();
    let pz = race.z_of(0);
    let (back, height) = match cam {
        Cam::Chase => (CAM_BACK, CAM_H),
        Cam::Nose => (0, NOSE_H),
    };
    let cam_z = (pz - back).rem_euclid(lap_z);
    let cam_x16 = race.karts[0].x * ROAD_HALF / X_ONE * 16;
    let cam_y = course.y_at(pz) + height;
    let base = (cam_z / SEG) as u32;
    let frac = cam_z % SEG;

    // 1. Project the segment ends. First the road's sideways drift and
    // height, then the screen values (the turn of the view needs the
    // drift where the kart is).
    let mut ends = [END0; DRAW + 1];
    let mut xs = [0i32; DRAW + 1];
    let mut x16 = 0i32;
    let mut dx16 = -course.seg(base).curve16 * frac / SEG;
    for (n, e) in ends.iter_mut().enumerate() {
        let i = base + n as u32;
        let seg = course.seg(i);
        xs[n] = x16;
        e.z = n as i32 * SEG - frac;
        e.y16 = seg.y - cam_y; // world height for now
        x16 += dx16;
        dx16 += seg.curve16;
    }
    // The view turns with the road: the road under the kart is ahead.
    let shift16 = if back > 0 {
        let n = ((back + frac) / SEG) as usize;
        let t = (back + frac) % SEG;
        let xk = xs[n] + (xs[n + 1] - xs[n]) * t / SEG;
        xk * D / back
    } else {
        0
    };
    for (n, e) in ends.iter_mut().enumerate() {
        let z = e.z.max(NEAR_Z);
        let sc = (D << 16) / z;
        e.s = sc;
        e.cx16 = CX * 16 + ((i64::from(xs[n] - cam_x16) * i64::from(sc)) >> 16) as i32 - shift16;
        e.y16 = HZ * 16 - ((i64::from(e.y16) * 16 * i64::from(sc)) >> 16) as i32;
    }

    // 2. The ground, front to back.
    let mut maxy = v.y1;
    for n in 0..DRAW {
        ends[n].clip = maxy as i16;
        let (a, b) = (ends[n], ends[n + 1]);
        if b.y16 >= a.y16 {
            // Facing away (behind a hill top): hidden.
            continue;
        }
        let top = ((b.y16 + 15) >> 4).max(v.y0);
        let bottom = ((a.y16 + 15) >> 4).min(maxy);
        if top >= bottom {
            if top < maxy {
                maxy = top;
            }
            continue;
        }
        let i = base + n as u32;
        let paint = Paint::of(course, cno, i, n, detail);
        let hw_a = (ROAD_HALF * a.s) >> 12;
        let hw_b = (ROAD_HALF * b.s) >> 12;
        let dy = (a.y16 - b.y16).max(1);
        for y in top..bottom {
            let f = ((y << 4) + 8 - b.y16).clamp(0, dy);
            let cx = b.cx16 + (a.cx16 - b.cx16) * f / dy;
            let hw = hw_b + (hw_a - hw_b) * f / dy;
            ground_row(s.row_mut(y as usize), &v, cx, hw, &paint);
        }
        cost.segs += 1;
        cost.rows += (bottom - top) as u16;
        maxy = top;
    }
    ends[DRAW].clip = maxy as i16;

    // 3. The backdrop above the farthest ground.
    let bg = if detail.backdrop_turns() {
        race.bg >> 8
    } else {
        0
    };
    backdrop(s, &v, cno, bg, maxy, detail);

    // 4. Sprites, back to front: the things at the far end of each
    // segment, then the karts inside it.
    let mut order = [(0i32, 0usize); KARTS];
    let mut shown = 0;
    for k in 0..KARTS {
        let dz = if k == 0 {
            back
        } else {
            wrap(race.z_of(k) - cam_z, lap_z)
        };
        // In view, or the player's own kart seen from behind.
        let in_view = dz > NEAR_Z * 4 && dz < (DRAW as i32 - 1) * SEG - frac;
        if in_view || (k == 0 && cam == Cam::Chase) {
            order[shown] = (dz, k);
            shown += 1;
        }
    }
    // Farthest first.
    order[..shown].sort_unstable_by_key(|a| core::cmp::Reverse(a.0));
    let mut next_kart = 0;
    for n in (0..DRAW).rev() {
        let far = n + 1;
        if far <= THINGS_DRAW && ends[far].z > NEAR_Z * 3 {
            let i = base + far as u32;
            things(s, race, &v, &ends[far], i, detail, cost);
        }
        while next_kart < shown {
            let (dz, k) = order[next_kart];
            let seg_n = ((dz + frac) / SEG) as usize;
            if seg_n < n {
                break;
            }
            next_kart += 1;
            if k == 0 {
                if cam == Cam::Chase {
                    player_kart(s, race, &v, cost);
                }
                continue;
            }
            let t = (dz + frac) % SEG;
            let (a, b) = (&ends[seg_n], &ends[seg_n + 1]);
            let lerp = |p: i32, q: i32| p + (q - p) * t / SEG;
            let sc = lerp(a.s, b.s);
            let cx16 = lerp(a.cx16, b.cx16);
            let y16 = lerp(a.y16, b.y16);
            let kx = race.karts[k].x * ROAD_HALF / X_ONE;
            let x16 = cx16 + ((i64::from(kx) * 16 * i64::from(sc)) >> 16) as i32;
            let w16 = ((i64::from(KART_WORLD_W) * 16 * i64::from(sc)) >> 16) as i32;
            let remap = kart_remap(k, race.karts[k].braking);
            blit_scaled(
                s,
                &KART_PIC,
                x16,
                y16 >> 4,
                w16,
                false,
                i32::from(a.clip),
                &v,
                &remap,
                cost,
            );
        }
    }

    if cam == Cam::Nose {
        nose(s, race, &v);
    }
}

/// The colours of one segment's rows.
struct Paint {
    grass: u8,
    road: u8,
    /// The kerb (also when the stripe is the other colour).
    kerb: u8,
    line: Option<u8>,
    /// Edge lines (the richest detail).
    edge: Option<u8>,
    shoulder: Shoulder,
    side: u8,
    /// The start line's chequers.
    chequer: bool,
}

impl Paint {
    fn of(course: &crate::track::Course, c: usize, i: u32, n: usize, detail: Detail) -> Paint {
        let haze = if n < 18 {
            0
        } else if n < 42 {
            1
        } else {
            2
        };
        let stripe = !detail.bands() || (i / RUMBLE_SEGS).is_multiple_of(2);
        let grass = if detail == Detail::Heavy && !stripe {
            GRASS_B
        } else {
            GRASS_A
        };
        Paint {
            grass: ground(c, grass, haze),
            road: ground(c, if stripe { ROAD_A } else { ROAD_B }, haze),
            kerb: ground(c, if stripe { RUMBLE_A } else { RUMBLE_B }, haze),
            line: (i / 2).is_multiple_of(2).then_some(ground(c, LINE, haze)),
            edge: (detail == Detail::Heavy).then_some(ground(c, LINE, haze)),
            shoulder: course.shoulder(i),
            side: ground(c, SIDE, haze),
            chequer: i.is_multiple_of(course.segs),
        }
    }
}

/// Fills columns `a16..b16` (1/16 pixel) of a row, inside the view.
#[inline(always)]
fn span(row: &mut [u8], v: &View, a16: i32, b16: i32, c: u8) {
    let a = ((a16 + 8) >> 4).max(v.x0);
    let b = ((b16 + 8) >> 4).min(v.x1);
    if a < b {
        row[a as usize..b as usize].fill(c);
    }
}

/// One ground row: the road's middle at `cx` and half its width `hw`,
/// both in 1/16 pixel.
fn ground_row(row: &mut [u8], v: &View, cx: i32, hw: i32, p: &Paint) {
    row[v.x0 as usize..v.x1 as usize].fill(p.grass);
    match p.shoulder {
        Shoulder::None => {}
        Shoulder::Gravel => span(row, v, cx - hw * 3 / 2, cx + hw * 3 / 2, p.side),
        Shoulder::WaterLeft => span(row, v, v.x0 * 16 - 16, cx - hw * 13 / 10, p.side),
    }
    let kerb = (hw >> 3).max(8);
    span(row, v, cx - hw - kerb, cx + hw + kerb, p.kerb);
    span(row, v, cx - hw, cx + hw, p.road);
    if p.chequer {
        // Black and white squares, eight across.
        let sq = (hw >> 2).max(16);
        let mut x = cx - hw;
        let mut k = 0;
        while x < cx + hw {
            if k % 2 == 0 {
                span(row, v, x, (x + sq).min(cx + hw), WHITE);
            } else {
                span(row, v, x, (x + sq).min(cx + hw), BLACK);
            }
            x += sq;
            k += 1;
        }
        return;
    }
    if let Some(c) = p.edge {
        let w = (hw / 40).max(4);
        span(row, v, cx - hw * 9 / 10 - w, cx - hw * 9 / 10 + w, c);
        span(row, v, cx + hw * 9 / 10 - w, cx + hw * 9 / 10 + w, c);
    }
    if let Some(c) = p.line {
        let w = hw / 40;
        if w >= 4 {
            span(row, v, cx - w, cx + w, c);
        }
    }
}

/// Sky, sun or moon, stars, mountains and the far land, in the rows
/// `v.y0..maxy`. `bg`: how far the backdrop has turned, in pixels.
fn backdrop(s: &mut Frame, v: &View, c: usize, bg: i32, maxy: i32, detail: Detail) {
    let b = course_base(c);
    // The sun (low, in the dusk) or the moon (high, at night).
    let (sun_x, sun_y, sun_r) = match c {
        0 => (150, HZ - 9, 7),
        1 => (40, HZ - 20, 4),
        _ => (200, HZ - 23, 5),
    };
    let stars = c != 0;
    let hills = detail != Detail::Light;
    for y in v.y0..maxy.min(v.y1) {
        let row = s.row_mut(y as usize);
        // The same sky in every detail: the steps count from the top bar.
        let step = ((y - TOP) * SKY_STEPS as i32 / (HZ - TOP + 1)).clamp(0, SKY_STEPS as i32 - 1);
        let sky = b + step as u8;
        if y >= HZ + 2 {
            row[v.x0 as usize..v.x1 as usize].fill(b + FAR_LAND);
            continue;
        }
        for x in v.x0..v.x1 {
            let bx = (x + bg).rem_euclid(256) as usize;
            let ridge = HZ + 2 - i32::from(RIDGE[bx]) - 5;
            let p =
                if hills && y >= HZ + 1 - i32::from(HILLS[(x + bg * 2).rem_euclid(256) as usize]) {
                    let hx = (x + bg * 2).rem_euclid(256) as usize;
                    if HILLS[(hx + 1) % 256] > HILLS[hx] {
                        b + NEAR_DARK
                    } else {
                        b + NEAR
                    }
                } else if y >= ridge {
                    // The top two rows of the ridge catch the light.
                    if y < ridge + 2 { b + FAR_LIT } else { b + FAR }
                } else {
                    let sx = (bx as i32 - sun_x + 128).rem_euclid(256) - 128;
                    let sy = y - sun_y;
                    if sx * sx + sy * sy <= sun_r * sun_r {
                        b + SUN
                    } else if stars && y < TOP + 14 && star_at(bx as u32, y as u32) {
                        b + STAR
                    } else {
                        sky
                    }
                };
            row[x as usize] = p;
        }
    }
}

/// A few stars, always in the same places of the turning sky.
fn star_at(x: u32, y: u32) -> bool {
    let h = (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)) >> 7;
    h.is_multiple_of(67)
}

/// The roadside things at the far end of segment `i`.
fn things(s: &mut Frame, race: &Race, v: &View, e: &End, i: u32, detail: Detail, cost: &mut Cost) {
    let course = race.course();
    for t in course.things(i).into_iter().flatten() {
        if t.kind == Kind::Bush && detail == Detail::Light {
            continue;
        }
        let x_world = i32::from(t.x) * ROAD_HALF / 1024;
        let x16 = e.cx16 + ((i64::from(x_world) * 16 * i64::from(e.s)) >> 16) as i32;
        if t.kind == Kind::Gantry {
            gantry(s, v, e, cost);
            continue;
        }
        let w16 = ((i64::from(t.kind.width()) * 16 * i64::from(e.s)) >> 16) as i32;
        blit_scaled(
            s,
            t.kind.pic(),
            x16,
            e.y16 >> 4,
            w16,
            t.flip,
            i32::from(e.clip),
            v,
            &SAME,
            cost,
        );
    }
}

/// The start and finish arch: two posts and a banner over the road.
fn gantry(s: &mut Frame, v: &View, e: &End, cost: &mut Cost) {
    let px = |w: i32| ((i64::from(w) * i64::from(e.s)) >> 16) as i32;
    let ground = e.y16 >> 4;
    let clip = i32::from(e.clip).min(v.y1);
    let half = px(1300);
    let post = px(120).max(1);
    let top = ground - px(1700);
    let band = px(380).max(2);
    let cx = e.cx16 >> 4;
    let mut rect = |x0: i32, y0: i32, x1: i32, y1: i32, c: u8| {
        let (x0, x1) = (x0.max(v.x0), x1.min(v.x1));
        let (y0, y1) = (y0.max(v.y0), y1.min(clip));
        for y in y0..y1 {
            if x0 < x1 {
                s.row_mut(y as usize)[x0 as usize..x1 as usize].fill(c);
                cost.sprite_px += (x1 - x0) as u32;
            }
        }
    };
    rect(cx - half - post, top, cx - half, ground, METAL);
    rect(cx + half, top, cx + half + post, ground, METAL_DARK);
    rect(cx - half - post, top, cx + half + post, top + band, BANNER);
    // A chequered strip under the banner.
    let sq = (band / 3).max(1);
    let mut x = cx - half - post;
    let mut k = 0;
    while x < cx + half + post {
        let c = if k % 2 == 0 { WHITE } else { BLACK };
        rect(
            x,
            top + band,
            (x + sq).min(cx + half + post),
            top + band + sq,
            c,
        );
        x += sq;
        k += 1;
    }
    cost.sprites += 1;
    // The word, once the banner is big enough to hold it.
    if band >= 9 && top + 2 >= v.y0 && top + band <= clip {
        let w = game_kit::text_width("HAIRPIN") as i32;
        let x0 = cx - w / 2;
        if x0 >= v.x0 && x0 + w <= v.x1 {
            s.text(x0 as i16, (top + (band - 5) / 2) as i16, "HAIRPIN", TEXT);
        }
    }
}

/// Draws `pic` shrunk (or grown) to `w16`/16 pixels wide, standing with
/// its middle at `x16` (1/16 pixel) on row `bottom`, hidden from row
/// `clip` down and outside the view.
#[allow(clippy::too_many_arguments)]
fn blit_scaled(
    s: &mut Frame,
    pic: &Pic,
    x16: i32,
    bottom: i32,
    w16: i32,
    flip: bool,
    clip: i32,
    v: &View,
    remap: &Remap,
    cost: &mut Cost,
) {
    let w = (w16 + 8) >> 4;
    if w < 1 {
        return;
    }
    let pw = i32::from(pic.w);
    let ph = i32::from(pic.h);
    let h = (w * ph + pw / 2) / pw;
    if h < 1 {
        return;
    }
    let x0 = ((x16 + 8) >> 4) - w / 2;
    let y0 = bottom - h;
    let (cx0, cx1) = (x0.max(v.x0), (x0 + w).min(v.x1));
    let (cy0, cy1) = (y0.max(v.y0), bottom.min(clip).min(v.y1));
    if cx0 >= cx1 || cy0 >= cy1 {
        return;
    }
    cost.sprites += 1;
    // Steps through the picture, in 1/65536 of its pixels.
    let step_x = (pw << 16) / w;
    let step_y = (ph << 16) / h;
    for y in cy0..cy1 {
        let sy = (((y - y0) * step_y) >> 16).min(ph - 1);
        let src = &pic.px[(sy * pw) as usize..((sy + 1) * pw) as usize];
        let row = s.row_mut(y as usize);
        let mut fx = (cx0 - x0) * step_x;
        for x in cx0..cx1 {
            let sx = (fx >> 16).min(pw - 1);
            let sx = if flip { pw - 1 - sx } else { sx };
            let p = src[sx as usize];
            if p != CLEAR {
                row[x as usize] = remap.of(p);
                cost.sprite_px += 1;
            }
            fx += step_x;
        }
    }
}

/// The player's kart at its own size, below the middle of the view.
fn player_kart(s: &mut Frame, race: &Race, v: &View, cost: &mut Cost) {
    let me = &race.karts[0];
    let bounce = if race.shake > 0 && (race.tick / 2).is_multiple_of(2) {
        1
    } else {
        0
    };
    let x0 = CX - i32::from(KART_PIC.w) / 2;
    let y0 = KART_ROW - i32::from(KART_PIC.h) - bounce;
    let remap = kart_remap(0, me.braking);
    let lean = i32::from(race.lean);
    // The tyres' tread moves with the speed.
    let roll = (me.prog / (crate::race::SUB * 90)) % 2 == 0;
    let w = i32::from(KART_PIC.w);
    for py in 0..i32::from(KART_PIC.h) {
        let y = y0 + py;
        if y < v.y0 || y >= v.y1 {
            continue;
        }
        let dx = if py < i32::from(KART_LEAN_ROWS) {
            lean
        } else {
            0
        };
        let row = s.row_mut(y as usize);
        for px in 0..w {
            let x = x0 + px + dx;
            if x < v.x0 || x >= v.x1 {
                continue;
            }
            let mut p = KART_PIC.px[(py * w + px) as usize];
            if p == CLEAR {
                continue;
            }
            if roll && (p == TYRE || p == TYRE_HI) {
                p = if p == TYRE { TYRE_HI } else { TYRE };
            }
            row[x as usize] = remap.of(p);
            cost.sprite_px += 1;
        }
    }
    // Boost flames.
    if race.boosting {
        let flick = race.tick % 4 < 2;
        let y = KART_ROW - 2;
        for (dx, c) in [(-3, ORANGE), (-2, GLOW), (1, GLOW), (2, ORANGE)] {
            s.pixel((CX + dx) as i16, y as i16, c);
            if flick {
                s.pixel((CX + dx) as i16, (y + 1) as i16, c);
            }
        }
    }
    cost.sprites += 1;
}

/// The front of the kart, seen from the driver's seat.
fn nose(s: &mut Frame, race: &Race, v: &View) {
    let body = KART;
    let y1 = v.y1;
    // The front wheels at the corners, the nose in the middle.
    for k in 0..8 {
        let y = y1 - 8 + k;
        let half = 14 + k * 2;
        s.rect(
            (CX - half) as i16,
            y as i16,
            (half * 2) as u16,
            1,
            if k < 2 { body } else { body + 1 },
        );
    }
    s.rect((CX - 6) as i16, (y1 - 10) as i16, 12, 2, body);
    s.rect((CX - 2) as i16, (y1 - 9) as i16, 4, 1, SEAT);
    let wheel = |s: &mut Frame, x: i32| {
        s.rect(x as i16, (y1 - 9) as i16, 10, 9, TYRE);
        s.rect(x as i16 + 1, (y1 - 8) as i16, 8, 1, TYRE_HI);
    };
    wheel(s, CX - 46 - i32::from(race.lean));
    wheel(s, CX + 36 - i32::from(race.lean));
}
