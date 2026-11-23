//! GATHER: the boot animation (feature `boot-gather`), made of 2,048
//! points that come together.
//!
//! One spark on the dark background bursts into a sphere of points that
//! turns in 3D, lit as a ball; the sphere condenses into a glowing
//! two-colour trefoil knot that streams along itself; the knot then
//! unwinds strand by strand, each strand flying into one letter of the
//! name, left to right; each letter heats up from ember to solid paper
//! white as its points land, a gold gleam runs across the name once, and
//! the name shrinks and moves to exactly where the home screen writes its
//! title, in the home screen's colour, while the home screen's bar slides
//! in. The last frame is the home screen's background, stars, bar and
//! title ([`draw_final`]).
//!
//! No std, no heap, no floating point, no unsafe, and no memory from one
//! frame to the next: every frame is a pure function of the time, so a
//! dropped, repeated or late frame never leaves anything behind, and a
//! skip just draws a later time. It needs no scratch memory. The tables
//! (sine, sphere, perspective, the name, the palette) are built while
//! compiling and sit in flash on the chip.
//!
//! The name is [`crate::theme::NAME`] and the letters are console-video's
//! font, so a renamed console stays in step. The places it hands over to
//! (the title, the stars, the bar) are home's own constants.
//!
//! Numbers: space units are 32 to a picture pixel at depth 0; angles are
//! `u16` turns (65,536 = one turn); sine and matrices are in 1/16384;
//! time shaping is in 1/4096; screen places are in 1/16 pixel; the
//! shrinking name is placed in 1/256 pixel.
//!
//! The cut was made and measured on the PC (the notes are kept outside
//! the repository).

mod name;
mod tables;

pub use name::{CELLS, LETTERS, NAME_W};
use tables::{COS_SIN_SHIFT, RECIP, RECIP_FIRST, SPHERE_R, cos, sin};
pub use tables::{PALETTE, PLAIN};

use crate::gfx;
use crate::theme::{self, GOLD, INK, LINE, MUTE, PANEL, PAPER};

/// The picture: 160 x 120, one palette number per pixel.
const W: usize = console_video::W;
const H: usize = console_video::H;

// ---- The home screen's first picture (home's own constants) ------------------
/// Where home writes the console's name (font size).
const HOME_TITLE_X: i32 = theme::TITLE_X as i32;
const HOME_TITLE_Y: i32 = theme::TOP_Y as i32;
/// The bar (bar.rs draws PANEL with a LINE row on top).
const BAR_Y: i32 = theme::BAR_Y as i32;
const BAR_H: i32 = theme::BAR_H as i32;
/// The home screen's still stars, drawn in LINE.
const STARS: [(i32, i32); gfx::STARS.len()] = {
    let mut s = [(0, 0); gfx::STARS.len()];
    let mut i = 0;
    while i < s.len() {
        s[i] = (gfx::STARS[i].0 as i32, gfx::STARS[i].1 as i32);
        i += 1;
    }
    s
};

// ---- The big name -------------------------------------------------------------
/// How much bigger than the font the name is written in the middle: the
/// biggest whole size that fits with a margin.
pub const NAME_SCALE: i32 = name_scale(NAME_W);

/// The size for a name `w` pixels wide at font size.
const fn name_scale(w: usize) -> i32 {
    if w * 3 <= 150 {
        3
    } else if w * 2 <= 154 {
        2
    } else {
        1
    }
}
pub const NAME_X: i32 = (W as i32 - NAME_W as i32 * NAME_SCALE) / 2;
pub const NAME_Y: i32 = 53;

// ---- Timeline (ms) -------------------------------------------------------------
/// The burst starts; before it the spark grows in the middle.
const BURST_AT: i32 = 110;
const BURST_MS: i32 = 400;
/// Per-point extra delay of the burst (0..this).
const BURST_SPREAD: i32 = 90;
/// Sphere to knot: point i starts at CONDENSE_AT + CONDENSE_SPREAD * i / N
/// (a wave from one pole to the other).
const CONDENSE_AT: i32 = 560;
const CONDENSE_SPREAD: i32 = 180;
const CONDENSE_MS: i32 = 360;
/// The strands leave the knot: letter k at STREAM_AT + k * LETTER_GAP,
/// its cells one after another over LETTER_SPREAD.
const STREAM_AT: i32 = 1180;
/// The letters start over the same time whatever the name's length (62 ms
/// apart for 8 letters), so a long name is solid before the shrink starts
/// and a short one leaves no long still moment (checker's fix).
const WRITE_MS: i32 = 434;
pub(crate) const LETTER_GAP: i32 = letter_gap(LETTERS);

/// The time between two letters of a name of `letters` letters.
const fn letter_gap(letters: usize) -> i32 {
    let g = WRITE_MS / if letters > 1 { letters as i32 - 1 } else { 1 };
    if g > 145 { 145 } else { g }
}
pub(crate) const LETTER_SPREAD: i32 = 110;
/// Extra per-point delay (0..31 ms) and the flight time.
const FLY_MS: i32 = 290;
/// A cell starts to glow this long after its first points leave, and is
/// solid FILL_MS later.
const FILL_DELAY: i32 = 200;
const FILL_MS: i32 = 150;
/// The ten star points leave one after another and land as the home
/// screen's stars, fading to LINE.
const STAR_AT: i32 = 1120;
const STAR_GAP: i32 = 45;
const STAR_FLY: i32 = 420;
/// Short enough that the last star is LINE before the hand-over draws the
/// stars in LINE (360 ms cut the last fades short: a 4-pixel snap at
/// SHRINK_AT; checker's fix).
const STAR_FADE_MS: i32 = 230;
/// The gold gleam crosses the solid name once.
const GLEAM_AT: i32 = 1880;
/// The moment for a start-up chime, when one is built (no sound is made
/// today): the gold gleam starts across the solid name, in ms of the
/// animation's time (SHORT joins the cut later, so it comes
/// [`SHORT_LEAD_MS`] after a SHORT boot starts; OFF has none).
pub const CHIME_MS: u32 = GLEAM_AT as u32;
/// SHORT joins the cut this long before the gleam: the solid name stands
/// still for a moment first.
pub const SHORT_LEAD_MS: u32 = 120;
/// Where SHORT joins the cut (ms of the animation's time). SHORT lasts
/// [`END_MS`] minus this: 960 ms.
pub const SHORT_AT_MS: u32 = CHIME_MS - SHORT_LEAD_MS;
const GLEAM_MS: i32 = 300;
/// The name shrinks to the home screen's title...
const SHRINK_AT: i32 = 2180;
/// Where the shrink starts (ms of the animation's time): from here SHORT
/// and FULL draw the same pictures.
pub const SHRINK_AT_MS: u32 = SHRINK_AT as u32;
const SHRINK_MS: i32 = 380;
/// ...while the home screen's bar slides in.
const BAR_AT: i32 = 2290;
const BAR_MS: i32 = 270;
/// Everything that the hand-over draws in its final state must be
/// finished before it starts, for a name of `letters` letters: every
/// letter's points landed, every letter cell solid, every star LINE.
const fn done_before_shrink(letters: usize) -> bool {
    let last = if letters > 0 { letters - 1 } else { 0 };
    let last_leave = STREAM_AT + last as i32 * letter_gap(letters) + LETTER_SPREAD;
    let last_star = STAR_AT + (STARS.len() as i32 - 1) * STAR_GAP + STAR_FLY + STAR_FADE_MS;
    last_leave + 31 + FLY_MS <= SHRINK_AT
        && last_leave + FILL_DELAY + FILL_MS <= SHRINK_AT
        && last_star <= SHRINK_AT
}
const _: () = assert!(
    done_before_shrink(LETTERS),
    "a letter or a star is not finished at the shrink"
);
/// From here every frame is the final one.
pub const FINAL_MS: u32 = 2560;
/// The animation is over (the last frame is the final frame).
pub const END_MS: u32 = 2720;
const _: () = assert!(END_MS - SHORT_AT_MS <= 1000, "SHORT lasts at most 1 s");

// ---- Space -----------------------------------------------------------------------
/// Camera distance (space units).
pub(crate) const CAM: i32 = 5120;
pub(crate) const SPHERE: i32 = 1450;
/// How far each sphere point is moved off its place on the regular
/// spiral (breaks up the lattice the eye would see).
const SPHERE_JITTER: i32 = 52;
const KNOT_R: i32 = 1020;
const KNOT_TUBE_R: i32 = 460;
const TUBE: i32 = 70;
/// Colour bands along the knot, alternately ice and violet (half as many
/// slices round the sphere's axis: the sphere is a turning beach ball).
const BANDS: usize = 8;
/// Q14 one.
const ONE: i32 = 16384;

/// The sphere is lit as a ball from the upper left and in front (picture
/// space: x right, y down, z away from the viewer; Q12, length 4096).
const LIGHT: [i32; 3] = [-1310, -1801, -3438];
/// The lighting comes in while the burst settles into the sphere.
const LIT_AT: i32 = 230;
const LIT_MS: i32 = 170;

/// The lit ball's levels: shadowed side, full sunlit side, highlight, and
/// the few far-side points.
const BALL_AMBIENT: i32 = 9;
const BALL_DIFFUSE: i32 = 40;
const BALL_GLINT: i32 = 64;
const BALL_BACK: i32 = 9;
/// A faint cool rim on the shadowed edge, so the ball keeps its outline.
const BALL_RIM: i32 = 22;

/// Number of points.
pub const POINTS: usize = 2048;
/// Points that fly to the home screen's still stars instead of a letter.
pub const STAR_POINTS: usize = STARS.len();
// Divisions by the point count are shifts by 11.
const _: () = assert!(POINTS == 2048);

// ---- Palette layout (see tables::PALETTE) ---------------------------------------
pub(crate) const ICE: u8 = 0;
pub(crate) const VIOLET: u8 = 64;
pub(crate) const EMBER: u8 = 128;
pub(crate) const GLEAM: u8 = 192;
pub(crate) const TITLE: u8 = 200;
pub(crate) const TINTS: usize = 4;
pub(crate) const STAR_FADE: u8 = 236;
const TOP: i32 = 63;
/// Cool light grows eight times slower above this level (see `add`),
/// coming in as the sphere starts to condense (the spark and the burst
/// keep their white centre).
const KNEE: i32 = 44;
const KNEE_AT: i32 = 560;
const KNEE_MS: i32 = 200;

/// Draws the frame at `t_ms` of the animation's time, with the colours
/// of [`PALETTE`] (the console sets them; they never change during the
/// animation). Returns true once the animation is over (`t_ms >=
/// END_MS`); the frame drawn then is the final one, ready to hand over
/// to the home screen.
///
/// `short`: SHORT's look before the shrink: the name stands solid with
/// the stars in place and no points, and the gleam crosses it. From the
/// shrink on both are the same.
pub fn draw(t_ms: u32, short: bool, pixels: &mut [u8; W * H]) -> bool {
    if t_ms >= FINAL_MS {
        draw_final(pixels);
        return t_ms >= END_MS;
    }
    let t = t_ms as i32;
    pixels.fill(INK);
    if t >= SHRINK_AT {
        hand_over(
            pixels,
            smoother(ramp(t, SHRINK_AT, SHRINK_MS)),
            ease_out(ramp(t, BAR_AT, BAR_MS)),
        );
        return false;
    }
    if short {
        // Every cell solid (they all are by the shrink), every star in
        // place, as the full cut has them just before the shrink.
        cells(pixels, SHRINK_AT);
        for &(x, y) in &STARS {
            pixels[y as usize * W + x as usize] = LINE;
        }
        gleam(pixels, t);
        return false;
    }

    // The letters, heating up cell by cell under the points.
    cells(pixels, t);

    let now = Frame::at(t);
    for i in 0..POINTS {
        now.point(i, pixels);
    }

    gleam(pixels, t);
    false
}

/// The last frame: exactly the home screen's first picture without what
/// depends on the card and the pads: the background, the still stars,
/// the bar's panel and the console's name where home writes it, in
/// home's colour. Needs no memory and no history.
pub fn draw_final(pixels: &mut [u8; W * H]) {
    pixels.fill(INK);
    hand_over(pixels, 4096, 4096);
}

// ---- Time shaping (Q12: 4096 = 1.0) --------------------------------------------

/// 0 before `at`, 4096 after `at + dur`, a straight line between.
#[inline(always)]
fn ramp(t: i32, at: i32, dur: i32) -> i32 {
    ((t - at) * 4096 / dur).clamp(0, 4096)
}

#[inline(always)]
fn ease_in_out(x: i32) -> i32 {
    let x2 = (x * x) >> 12;
    (x2 * (3 * 4096 - 2 * x)) >> 12
}

#[inline(always)]
fn ease_out(x: i32) -> i32 {
    let y = 4096 - x;
    4096 - ((((y * y) >> 12) * y) >> 12)
}

/// Ease out with a small overshoot (about 6 %).
#[inline(always)]
fn ease_out_back(x: i32) -> i32 {
    let y = x - 4096;
    let y2 = (y * y) >> 12;
    let y3 = (y2 * y) >> 12;
    4096 + ((8602 * y3) >> 12) + ((4506 * y2) >> 12)
}

/// 6x^5 - 15x^4 + 10x^3 (per frame only).
fn smoother(x: i32) -> i32 {
    let x = i64::from(x);
    let x3 = (x * x * x) >> 24;
    ((x3 * (x * (x * 6 - 15 * 4096) / 4096 + 10 * 4096)) >> 12) as i32
}

#[inline(always)]
fn hash(i: u32) -> u32 {
    let mut x = i.wrapping_mul(0x9E37_79B1);
    x ^= x >> 15;
    x = x.wrapping_mul(0x85EB_CA77);
    x ^ (x >> 13)
}

#[inline(always)]
fn lerp(a: i32, b: i32, w: i32) -> i32 {
    a + (((b - a) * w) >> 12)
}

// ---- The letters -----------------------------------------------------------------

/// When cell `c` starts to glow.
#[inline(always)]
fn fill_at(c: usize) -> i32 {
    STREAM_AT + i32::from(name::CELL_T[c]) + FILL_DELAY
}

/// The big name, cell by cell: dark ember to gold to solid paper.
fn cells(px: &mut [u8; W * H], t: i32) {
    if t < fill_at(0) {
        return;
    }
    for (c, &[cx, cy]) in name::CELL_XY.iter().enumerate() {
        let f = ramp(t, fill_at(c), FILL_MS);
        if f == 0 {
            continue;
        }
        let colour = if f >= 4096 {
            PAPER
        } else {
            EMBER + ((TOP * ease_in_out(f)) >> 12) as u8
        };
        let (x0, y0) = (
            NAME_X + i32::from(cx) * NAME_SCALE,
            NAME_Y + i32::from(cy) * NAME_SCALE,
        );
        for y in y0..y0 + NAME_SCALE {
            let o = y as usize * W + x0 as usize;
            px[o..o + NAME_SCALE as usize].fill(colour);
        }
    }
}

/// A slanted gold band crossing the solid letters once.
fn gleam(px: &mut [u8; W * H], t: i32) {
    if !(GLEAM_AT..GLEAM_AT + GLEAM_MS).contains(&t) {
        return;
    }
    let w = NAME_W as i32 * NAME_SCALE;
    let h = name::FONT_H as i32 * NAME_SCALE;
    // Even speed, and it clears the last letter just as the shrink starts
    // (an eased band left the name about 50 ms early and, with the
    // shrink's slow start, made a still moment of about 70 ms).
    let pos = NAME_X - 14 + (((w + 28) * ramp(t, GLEAM_AT, GLEAM_MS)) >> 12);
    // Gold in the middle, softening to paper on both sides.
    const BAND: [u8; 8] = [
        GOLD,
        GOLD,
        GOLD,
        GLEAM + 1,
        GLEAM + 2,
        GLEAM + 4,
        GLEAM + 6,
        GLEAM + 7,
    ];
    const HALF: i32 = BAND.len() as i32 - 1;
    for y in NAME_Y..NAME_Y + h {
        let x_mid = pos - (y - NAME_Y) / 2;
        for x in (x_mid - HALF).max(0)..(x_mid + HALF + 1).min(W as i32) {
            let p = &mut px[y as usize * W + x as usize];
            if *p == PAPER {
                *p = BAND[(x - x_mid).unsigned_abs() as usize];
            }
        }
    }
}

/// The hand-over: the name at `e` (Q12) of the way from the big name to
/// the home screen's title, the stars, and the bar `bar` (Q12) of the way
/// in. At `e = bar = 4096` this is the final frame.
fn hand_over(px: &mut [u8; W * H], e: i32, bar: i32) {
    for &(x, y) in &STARS {
        px[y as usize * W + x as usize] = LINE;
    }
    let top = BAR_Y + (((BAR_H) * (4096 - bar)) >> 12);
    if top < H as i32 {
        px[top as usize * W..(top as usize + 1) * W].fill(LINE);
        px[(top as usize + 1) * W..].fill(PANEL);
    }
    let s = lerp(NAME_SCALE * 256, 256, e);
    let x = lerp(NAME_X * 256, HOME_TITLE_X * 256, e);
    let y = lerp(NAME_Y * 256, HOME_TITLE_Y * 256, e);
    let tint = ((e * TINTS as i32) >> 12).min(TINTS as i32 - 1);
    title(px, x, y, s, tint);
}

/// The name with its left top corner at (x0, y0) and `s` picture pixels
/// to a font pixel (all in 1/256 pixel, s >= 1 pixel), each pixel shaded
/// by how much of it the letters cover (8 steps), in tint `tint`.
fn title(px: &mut [u8; W * H], x0: i32, y0: i32, s: i32, tint: i32) {
    let s = s.max(256);
    // How much of pixel row/column p the font rows/columns k and k + 1
    // cover (1/256 each).
    let span = |p: i32, at: i32| {
        let a = p * 256 - at;
        let k = a.div_euclid(s);
        let cut = (k + 1) * s; // where cell k ends
        let first = (cut - a).min(256);
        (k, first, 256 - first)
    };
    let x_end = ((x0 + NAME_W as i32 * s + 255) >> 8).min(W as i32);
    let y_end = ((y0 + name::FONT_H as i32 * s + 255) >> 8).min(H as i32);
    let full = match tint {
        0 => PAPER,
        t if t == TINTS as i32 - 1 => MUTE,
        t => TITLE + t as u8 * 8 + 7,
    };
    for py in (y0 >> 8).max(0)..y_end {
        let (j, oy0, oy1) = span(py, y0);
        for pxx in (x0 >> 8).max(0)..x_end {
            let (i, ox0, ox1) = span(pxx, x0);
            let mut cov = 0;
            if name::ink(i, j) {
                cov += ox0 * oy0;
            }
            if name::ink(i + 1, j) {
                cov += ox1 * oy0;
            }
            if name::ink(i, j + 1) {
                cov += ox0 * oy1;
            }
            if name::ink(i + 1, j + 1) {
                cov += ox1 * oy1;
            }
            let level = (cov * 8 + 32_768) >> 16;
            if level > 0 {
                px[py as usize * W + pxx as usize] = if level >= 8 {
                    full
                } else {
                    TITLE + (tint * 8 + level - 1) as u8
                };
            }
        }
    }
}

// ---- One moment in time ----------------------------------------------------------

type Mat = [[i32; 3]; 3];

#[inline(always)]
fn turn(m: &Mat, x: i32, y: i32, z: i32) -> (i32, i32, i32) {
    (
        (m[0][0] * x + m[0][1] * y + m[0][2] * z) >> 14,
        (m[1][0] * x + m[1][1] * y + m[1][2] * z) >> 14,
        (m[2][0] * x + m[2][1] * y + m[2][2] * z) >> 14,
    )
}

/// Everything that is the same for all points at one time.
struct Frame {
    t: i32,
    /// The figure's turn, Q14.
    turn: Mat,
    /// The figure's size, Q12 (it draws back as it empties).
    size: i32,
    /// How far the points have streamed along the knot (u16 turn).
    flow: u16,
    /// The spark's size (space units) and the sphere's ripple.
    spark: i32,
    ripple: i32,
    wave: u16,
    /// Overall brightness, Q8 (the spark, the burst's flash).
    gain: i32,
    /// How much the sphere is lit as a solid ball (Q12): 0 in the spark
    /// and the early burst, 4096 once the sphere has formed.
    lit: i32,
    /// The light's direction (from the surface towards the light) and the
    /// direction towards the viewer, both turned into the figure's own
    /// space, Q12, so a sphere point's normal needs no turning.
    light: [i32; 3],
    view: [i32; 3],
    /// Cool light grows slowly above this level (see `add`): none in the
    /// spark, the burst and the sphere (TOP), KNEE from the condense on.
    knee: i32,
}

impl Frame {
    fn at(t: i32) -> Frame {
        // The figure: tilted at first so the sphere is seen turning on a
        // slanted axis, then easing towards the viewer so the knot reads;
        // always spinning in its own plane and swaying from side to side.
        let pitch = lerp(10_800, 4_200, smoother(ramp(t, 520, 760)));
        let roll = (t as u32 * 44) as u16;
        let yaw = (5_600 * sin((t as u32 * 21) as u16)) >> 14;
        let turn = rotation(yaw as u16, pitch as u16, roll);
        let size = 4096 - ((1100 * ease_in_out(ramp(t, STREAM_AT - 120, 700))) >> 12);
        let flow = ((t - CONDENSE_AT).max(0) as u32 * 20) as u16;
        let spark = 18 + ((60 * ramp(t, 0, BURST_AT)) >> 12);
        let ripple = (160 * ramp(t, 420, 300)) >> 12;
        let wave = (t as u32 * 190) as u16;
        let gain = if t < BURST_AT {
            90 + ((166 * ramp(t, 0, BURST_AT)) >> 12)
        } else {
            256 + ((300 * (4096 - ease_out(ramp(t, BURST_AT, 520)))) >> 12)
        };
        // turn^T * v: a picture-space direction in the figure's space.
        let back = |v: [i32; 3]| {
            let mut o = [0i32; 3];
            for (j, oj) in o.iter_mut().enumerate() {
                *oj = (turn[0][j] * v[0] + turn[1][j] * v[1] + turn[2][j] * v[2]) >> 14;
            }
            o
        };
        Frame {
            t,
            turn,
            size,
            flow,
            spark,
            ripple,
            wave,
            gain,
            lit: ramp(t, LIT_AT, LIT_MS),
            light: back(LIGHT),
            view: back([0, 0, -4096]),
            knee: TOP - (((TOP - KNEE) * ramp(t, KNEE_AT, KNEE_MS)) >> 12),
        }
    }

    /// Point `i` at this time: where it is, how bright, and drawn.
    #[inline(always)]
    fn point(&self, i: usize, px: &mut [u8; W * H]) {
        let t = self.t;
        let ii = i as i32;
        let h = hash(i as u32);
        let h2 = hash(h);
        let (jx, jy, jz) = (
            i32::from(h as i8),
            i32::from((h >> 8) as i8),
            i32::from((h >> 16) as i8),
        );
        // Its place on the sphere's spiral (golden angle) and on the knot:
        // the same direction round the axis, so condensing moves it only
        // in and out and up and down, never round (no tangle).
        let a = (i as u32).wrapping_mul(40_503) as u16;
        let up = (a >> 1).wrapping_add(if i >= POINTS / 2 { 32_768 } else { 0 });

        // Where it flies to and when.
        let star = is_star(i);
        let (leave, fly_ms, tx, ty) = match star {
            Some(k) => {
                let (sx, sy) = STARS[k];
                (
                    STAR_AT + k as i32 * STAR_GAP,
                    STAR_FLY,
                    sx * 16 + 8,
                    sy * 16 + 8,
                )
            }
            None => {
                // The knot's strand of the name: the cell along the knot.
                let c = (usize::from(up) * CELLS) >> 16;
                let [cx, cy] = name::CELL_XY[c];
                if t >= fill_at(c) + FILL_MS {
                    return; // inside a solid letter
                }
                let sub_x = ((h >> 24) & 15) as i32 * (NAME_SCALE * 16 - 8) / 15 + 4;
                let sub_y = ((h >> 28) & 15) as i32 * (NAME_SCALE * 16 - 8) / 15 + 4;
                (
                    STREAM_AT + i32::from(name::CELL_T[c]) + (h2 & 31) as i32,
                    FLY_MS,
                    (NAME_X + i32::from(cx) * NAME_SCALE) * 16 + sub_x,
                    (NAME_Y + i32::from(cy) * NAME_SCALE) * 16 + sub_y,
                )
            }
        };
        let fly_raw = ramp(t, leave, fly_ms);
        if fly_raw >= 4096 {
            match star {
                Some(_) => {
                    // A landed star: one pixel, fading to the home star.
                    let f = ramp(t, leave + fly_ms, STAR_FADE_MS);
                    let c = if f >= 4096 {
                        LINE
                    } else {
                        STAR_FADE + ((f * 4) >> 12) as u8
                    };
                    px[(ty >> 4) as usize * W + (tx >> 4) as usize] = c;
                }
                None => splat(px, tx, ty, 22, EMBER, TOP),
            }
            return;
        }
        let fly = ease_in_out(fly_raw);

        // 1. The sphere: a jittered golden spiral, bursting out of the
        // spark, rippling as it turns.
        let m = ease_in_out(ramp(
            t,
            CONDENSE_AT + ((CONDENSE_SPREAD * ii) >> 11),
            CONDENSE_MS,
        ));
        let (mut x, mut y, mut z) = (0, 0, 0);
        // The sphere as a lit ball: its light in levels, and how much of
        // it counts (none in the burst, fading out as it condenses).
        let (mut ball, mut ball_w) = (0, 0);
        if m < 4096 {
            let delay = (((h2 >> 8) & 255) as i32 * BURST_SPREAD) >> 8;
            let burst = ease_out_back(ramp(t, BURST_AT + delay, BURST_MS));
            let r = i32::from(SPHERE_R[i]);
            let zs = ((SPHERE * (2 * ii + 1)) >> 11) - SPHERE;
            let wv = a
                .wrapping_mul(3)
                .wrapping_add((i as u16).wrapping_mul(56))
                .wrapping_sub(self.wave);
            let k = (burst * (4096 + ((self.ripple * sin(wv)) >> 14))) >> 12;
            let sx = ((r * cos(a)) >> COS_SIN_SHIFT) + ((jx * SPHERE_JITTER) >> 7);
            let sy = ((r * sin(a)) >> COS_SIN_SHIFT) + ((jy * SPHERE_JITTER) >> 7);
            let sz = zs + ((jz * SPHERE_JITTER) >> 7);
            // The spark: a tiny ball that the burst blows apart.
            let spark = (self.spark * (4096 - burst.min(4096))) >> 12;
            x = ((sx * k) >> 12) + ((jx * spark) >> 7);
            y = ((sy * k) >> 12) + ((jy * spark) >> 7);
            z = ((sz * k) >> 12) + ((jz * spark) >> 7);
            ball_w = (self.lit * (4096 - m)) >> 12;
            if ball_w > 0 {
                ball = self.ball(sx, sy, sz, h2);
            }
        }
        if m > 0 {
            // 2. The trefoil knot, (2,3) on a torus, with a fuzzy tube, the
            // points streaming along it.
            let u = up.wrapping_add(self.flow);
            let (ph, th) = (u.wrapping_mul(2), u.wrapping_mul(3));
            let rho = KNOT_R + ((KNOT_TUBE_R * cos(th)) >> COS_SIN_SHIFT);
            let kx = ((rho * cos(ph)) >> COS_SIN_SHIFT) + ((jx * TUBE) >> 7);
            let ky = ((rho * sin(ph)) >> COS_SIN_SHIFT) + ((jy * TUBE) >> 7);
            let kz = ((KNOT_TUBE_R * sin(th)) >> COS_SIN_SHIFT) + ((jz * TUBE) >> 7);
            if m < 4096 {
                x = lerp(x, kx, m);
                y = lerp(y, ky, m);
                z = lerp(z, kz, m);
            } else {
                (x, y, z) = (kx, ky, kz);
            }
        }
        (x, y, z) = turn(&self.turn, x, y, z);
        if self.size < 4096 {
            x = (x * self.size) >> 12;
            y = (y * self.size) >> 12;
            z = (z * self.size) >> 12;
        }
        // Nearer is brighter: depth -1600 (near) .. +1600 (far).
        let depth = 256 - (((z + 1600).clamp(0, 3200) * 17) >> 8);
        let base = if m < 2048 { 34 } else { 30 };
        let mut level = (base * depth) >> 8;
        if ball_w > 0 {
            level = lerp(level, ball, ball_w);
        }
        let mut hue = if ((usize::from(up) * BANDS) >> 16) & 1 == 0 {
            ICE
        } else {
            VIOLET
        };

        if fly > 0 {
            // 3. To its letter along a curve that swings towards the viewer
            // and up: a quadratic Bezier from where it is on the figure.
            // (2 * tx - 2559, not - 2560: half a 1/16 pixel off, and the
            // esp toolchain's LLVM cannot compile `2 * x - 2560` at
            // opt-level 3, the firmware's setting: "Cannot select".)
            let (bx, by) = (2 * tx - (16 * W as i32 - 1), 2 * ty - 16 * H as i32);
            let pull = 500 + ((h2 >> 16) & 15) as i32 * 40;
            let lift = -300 - ((h2 >> 20) & 15) as i32 * 30;
            let (cx, cy, cz) = ((x + bx) / 2, (y + by) / 2 + lift, z / 2 - pull);
            let (ax, ay, az) = (lerp(x, cx, fly), lerp(y, cy, fly), lerp(z, cz, fly));
            let (dx, dy, dz) = (lerp(cx, bx, fly), lerp(cy, by, fly), lerp(cz, 0, fly));
            x = lerp(ax, dx, fly);
            y = lerp(ay, dy, fly);
            z = lerp(az, dz, fly);
            if fly_raw > 1200 {
                hue = if star.is_some() { ICE } else { EMBER };
            }
        }

        // Look: perspective by a table of reciprocals (no division).
        let zc = (z + CAM).clamp(RECIP_FIRST as i32 * 8, 8191);
        let r = i32::from(RECIP[(zc >> 3) as usize - RECIP_FIRST]);
        let sx = 16 * (W as i32 / 2) + ((x * r) >> 16);
        let sy = 16 * (H as i32 / 2) + ((y * r) >> 16);
        if !(-16..16 * W as i32).contains(&sx) || !(-16..16 * H as i32).contains(&sy) {
            return;
        }
        let b = (level * self.gain) >> 8;
        splat(px, sx, sy, b, hue, self.knee);
    }

    /// A sphere point's light as part of a solid ball (levels, before the
    /// burst's flash): (sx, sy, sz) is its place on the sphere (radius
    /// about SPHERE), which is also its normal. Lit from the upper left
    /// and in front with a soft highlight; the far side mostly hidden.
    #[inline(always)]
    fn ball(&self, sx: i32, sy: i32, sz: i32, h2: u32) -> i32 {
        // Dot products with a direction, as cosines in Q12
        // (x / 1450 = (x >> 6) * 2893 >> 16).
        let cosine = |d: [i32; 3]| (((sx * d[0] + sy * d[1] + sz * d[2]) >> 6) * 2893) >> 16;
        let facing = cosine(self.view);
        if facing < 0 {
            // The far side: one point in four, faint, seen through the ball.
            return if h2 >> 30 == 0 { BALL_BACK } else { 0 };
        }
        let d = cosine(self.light).clamp(0, 4096);
        let d2 = (d * d) >> 12;
        let d4 = (d2 * d2) >> 12;
        let lit = BALL_AMBIENT + ((BALL_DIFFUSE * d2) >> 12) + ((BALL_GLINT * d4 * d4) >> 24);
        // Towards the edge the points crowd together on the picture: dim
        // them there so the edge does not read as a bright ring.
        let lit = (lit * (2560 + (facing * 3) / 8)) >> 12;
        // The rim: strongest at the edge (facing 0) on the side away from
        // the light.
        let edge = (4096 - facing).clamp(0, 4096);
        let edge3 = (((edge * edge) >> 12) * edge) >> 12;
        lit + ((((BALL_RIM * edge3) >> 12) * (4096 - d)) >> 12)
    }
}

/// Star `k` for the points that fly to a star.
#[inline(always)]
fn is_star(i: usize) -> Option<usize> {
    const STEP: usize = POINTS / STAR_POINTS;
    (i % STEP == STEP / 2 && i / STEP < STAR_POINTS).then_some(i / STEP)
}

/// Rotation (roll first, then yaw, then pitch), Q14. Per frame, so
/// 64-bit products are fine here.
fn rotation(yaw: u16, pitch: u16, roll: u16) -> Mat {
    let (sy, cy) = (sin(yaw), cos(yaw));
    let (sp, cp) = (sin(pitch), cos(pitch));
    let (sr, cr) = (sin(roll), cos(roll));
    let ry = [[cy, 0, sy], [0, ONE, 0], [-sy, 0, cy]];
    let rx = [[ONE, 0, 0], [0, cp, -sp], [0, sp, cp]];
    let rz = [[cr, -sr, 0], [sr, cr, 0], [0, 0, ONE]];
    mul(&mul(&rx, &ry), &rz)
}

fn mul(a: &Mat, b: &Mat) -> Mat {
    let mut m = [[0i32; 3]; 3];
    for (r, row) in m.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            let s: i64 = (0..3)
                .map(|k| i64::from(a[r][k]) * i64::from(b[k][c]))
                .sum();
            *cell = (s >> 14) as i32;
        }
    }
    m
}

// ---- Plotting --------------------------------------------------------------------

/// Adds `w` levels of light to one pixel, in ramp `hue`, saturating. The
/// last ramp to touch a pixel colours it, except that cool (ice or
/// violet) light never recolours an ember pixel: the letters' cells heat
/// in ember, and a cool point crossing one must not tint it. The letters,
/// the gleam and the stars are in front: points never draw over them.
///
/// Cool light above `knee` (KNEE from the condense on) adds only an eighth, so where many points pile
/// up (a strand seen end-on) the pile stays a pale glint instead of a
/// white blob; ember light is not limited (the letters heat to paper).
#[inline(always)]
fn add(p: &mut u8, w: i32, hue: u8, knee: i32) {
    if w <= 0 {
        return;
    }
    let lv = match *p {
        INK => 0,
        q if q < EMBER => i32::from(q & 63),
        q if q < GLEAM => {
            if hue != EMBER {
                return;
            }
            i32::from(q & 63)
        }
        _ => return,
    };
    let mut sum = lv + w;
    if hue != EMBER && sum > knee {
        sum = if lv >= knee {
            lv + (w >> 3)
        } else {
            knee + ((sum - knee) >> 3)
        };
    }
    *p = hue | sum.min(TOP) as u8;
}

/// A point of light at (sx, sy) in 1/16 pixel, shared over the 4 pixels
/// round it by how near it is to each (bilinear), so slow motion glides
/// instead of jumping a pixel at a time.
#[inline(always)]
fn splat(px: &mut [u8; W * H], sx: i32, sy: i32, b: i32, hue: u8, knee: i32) {
    let (ix, iy) = (sx >> 4, sy >> 4);
    let (fx, fy) = (sx & 15, sy & 15);
    let w0 = b * (16 - fx);
    let w1 = b * fx;
    let (a, c) = (16 - fy, fy);
    if (0..W as i32 - 1).contains(&ix) && (0..H as i32 - 1).contains(&iy) {
        let o = iy as usize * W + ix as usize;
        add(&mut px[o], (w0 * a) >> 8, hue, knee);
        add(&mut px[o + 1], (w1 * a) >> 8, hue, knee);
        add(&mut px[o + W], (w0 * c) >> 8, hue, knee);
        add(&mut px[o + W + 1], (w1 * c) >> 8, hue, knee);
    } else {
        for (dx, dy, w) in [
            (0, 0, w0 * a),
            (1, 0, w1 * a),
            (0, 1, w0 * c),
            (1, 1, w1 * c),
        ] {
            let (x, y) = (ix + dx, iy + dy);
            if (0..W as i32).contains(&x) && (0..H as i32).contains(&y) {
                add(&mut px[y as usize * W + x as usize], w >> 8, hue, knee);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cell that is heating (any ember level) keeps its colour under
    /// cool light: no blue-white smear on a letter (gather2 fix).
    #[test]
    fn cool_light_never_recolours_an_ember_pixel() {
        for q in EMBER..GLEAM {
            for hue in [ICE, VIOLET] {
                for knee in [KNEE, TOP] {
                    let mut p = q;
                    add(&mut p, 40, hue, knee);
                    assert_eq!(p, q);
                }
            }
        }
    }

    /// Cool light piles up only slowly above the knee: sixteen knot
    /// points of 8 levels on one pixel are white without the knee and stay
    /// well below white with it (gather2 fix of the hot spot).
    #[test]
    fn piled_cool_light_stays_below_white_after_the_knee() {
        let pile = |knee| {
            let mut p = INK;
            for _ in 0..16 {
                add(&mut p, 8, ICE, knee);
            }
            i32::from(p & 63)
        };
        assert_eq!(pile(TOP), TOP);
        assert!(pile(KNEE) < 58, "level {}", pile(KNEE));
    }

    /// Any name the font tables take (1 to 64 pixels wide, so at most 16
    /// letters) is written whole and still before the shrink, and its
    /// big form fits the picture. (The name itself is checked while
    /// compiling; this covers the names it could be renamed to: ORBI,
    /// LITTLEPOCKET and the like.)
    #[test]
    fn every_name_length_is_finished_before_the_shrink_and_fits() {
        for letters in 1..=16 {
            assert!(done_before_shrink(letters), "{letters} letters");
        }
        for w in 1..=64usize {
            let s = name_scale(w);
            let x = (W as i32 - w as i32 * s) / 2;
            assert!(x >= 0 && x + w as i32 * s <= W as i32, "{w} pixels wide");
            assert!(NAME_Y + name::FONT_H as i32 * s <= H as i32);
        }
        assert_eq!(LETTER_GAP, 62, "POCKETME: the cut as measured");
    }
}
