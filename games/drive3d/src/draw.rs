//! The picture: the palette, the sky and far hills, the 3D scene
//! ([`crate::scene`]), the screen text, the START menu and the line of
//! numbers. It reads the game only through [`Look`].
//!
//! Drawing order (the spec's section 3): sky bands and a row of far
//! hills, the ground colour below the horizon (so a crack in the land
//! shows ground, not sky), the land far to near with the road ribbon,
//! the things, the car's shadow, the car and its dust inside that order,
//! then the speed and times, the menu, and last the line of numbers.

use crate::look::Look;
use crate::menu::Menu;
use crate::models::{MATS, SHADES};
use crate::scene::{Painter, Rec};
use game_3d::{Rect, cos_fine, rect_fill, sin_fine};
use game_kit::{Frame, H, W, rgb565, text_width};

/// Palette numbers.
pub mod col {
    use super::GROUPS;
    use crate::models::{MATS, SHADES};

    /// The lit models: shade * MATS + material, shades 0 (lit) to 5.
    pub const MODELS: u8 = 0;
    /// The land and the road: `LAND + group * 12 + light * 4 + haze`
    /// (light 0 bright, 1 middle, 2 dark; haze 0 near to 3 far).
    pub const LAND: u8 = MATS * SHADES;
    /// The car's shadow: `SHADOW + group`, one dark entry each.
    pub const SHADOW: u8 = LAND + GROUPS * 12;
    /// The sky: four bands, the top one first.
    pub const SKY: u8 = SHADOW + GROUPS;
    /// The far hills on the horizon.
    pub const HILLS: u8 = SKY + 4;
    /// Below the horizon, under the land (the land's farthest haze).
    pub const GROUND: u8 = HILLS + 1;
    /// A plain background for the crack test (never in the game's
    /// pictures).
    pub const PLAIN: u8 = GROUND + 1;
    /// Dust puffs, and spray in the ford.
    pub const DUST: u8 = PLAIN + 1;
    pub const SPRAY: u8 = DUST + 1;
    /// Text, its shadow, warnings, the speed, the menu's box and frame.
    pub const TEXT: u8 = SPRAY + 1;
    pub const SHADE: u8 = TEXT + 1;
    pub const WARN: u8 = SHADE + 1;
    pub const GOLD: u8 = WARN + 1;
    pub const BOX: u8 = GOLD + 1;
    pub const FRAME: u8 = BOX + 1;
    /// Colours used.
    pub const COUNT: usize = FRAME as usize + 1;
}

/// The land's colour groups (`col::LAND + group * 12 + ...`).
pub mod group {
    pub const GRASS: u8 = 0;
    /// The land's gravel triangles beside the road ribbon (a darker,
    /// worn verge, so the road's edges read).
    pub const VERGE: u8 = 1;
    pub const WATER: u8 = 2;
    /// The road ribbon.
    pub const ROAD: u8 = 3;
    /// The ribbon through the ford.
    pub const FORD_ROAD: u8 = 4;
    /// The low grass of the cut: a lighter, yellower green than the
    /// grass, so it never reads as a second road (review look F3).
    pub const LOW_GRASS: u8 = 5;
}

/// How many land colour groups there are.
pub const GROUPS: u8 = 6;

/// The land colour of a group, light (0..3) and haze step (0..4).
pub const fn land(group: u8, light: u8, haze: u8) -> u8 {
    col::LAND + group * 12 + light * 4 + haze
}

type Rgb = (u8, u8, u8);

const fn mix1(x: u8, y: u8, t: u32) -> u8 {
    ((x as u32 * (256 - t) + y as u32 * t) / 256) as u8
}

const fn mix(a: Rgb, b: Rgb, t: u32) -> Rgb {
    (mix1(a.0, b.0, t), mix1(a.1, b.1, t), mix1(a.2, b.2, t))
}

const fn scale1(x: u8, k: u32) -> u8 {
    let v = x as u32 * k / 256;
    if v > 255 { 255 } else { v as u8 }
}

const fn scale(a: Rgb, k: u32) -> Rgb {
    (scale1(a.0, k), scale1(a.1, k), scale1(a.2, k))
}

const fn c565(a: Rgb) -> u16 {
    rgb565(a.0, a.1, a.2)
}

/// The models' base colours, by material (`models::mat`).
const MODEL_BASE: [Rgb; MATS as usize] = [
    (214, 70, 34),   // body: orange red
    (236, 236, 228), // roof: white
    (44, 58, 80),    // glass
    (30, 30, 34),    // tyres
    (255, 70, 56),   // lamps (glow)
    (64, 64, 70),    // trim
    (46, 104, 50),   // leaves
    (96, 70, 46),    // trunk
    (132, 128, 120), // rock
    (168, 140, 100), // planks
    (118, 58, 44),   // hut roof
    (236, 236, 228), // posts, white squares
    (246, 196, 36),  // flag yellow
    (70, 70, 78),    // poles
];

/// The haze the far land and things fade into.
const HAZE: Rgb = (170, 186, 192);

/// Model shades: light (256 is full) and how far toward the haze.
const MODEL_LIGHT: [u32; SHADES as usize] = [256, 212, 172, 136, 124, 112];
const MODEL_HAZE: [u32; SHADES as usize] = [0, 0, 0, 24, 88, 140];

/// The land and road groups' base colours.
const LAND_BASE: [Rgb; GROUPS as usize] = [
    (78, 128, 56),   // grass
    (112, 108, 66),  // verge
    (60, 104, 150),  // water
    (178, 152, 110), // road gravel
    (110, 158, 186), // road through the ford
    (128, 150, 58),  // low grass on the cut
];
const LAND_LIGHT: [u32; 3] = [284, 246, 204];
const LAND_HAZE: [u32; 4] = [0, 70, 136, 196];

const SKY_BANDS: [Rgb; 4] = [
    (66, 116, 186),
    (98, 146, 206),
    (134, 174, 218),
    (168, 198, 226),
];

const fn palette() -> [u16; col::COUNT] {
    let mut p = [0u16; col::COUNT];
    let mut s = 0;
    while s < SHADES as usize {
        let mut m = 0;
        while m < MATS as usize {
            let lit = scale(MODEL_BASE[m], MODEL_LIGHT[s]);
            p[s * MATS as usize + m] = c565(mix(lit, HAZE, MODEL_HAZE[s]));
            m += 1;
        }
        s += 1;
    }
    let mut g = 0;
    while g < GROUPS as usize {
        let mut l = 0;
        while l < 3 {
            let mut h = 0;
            while h < 4 {
                let lit = scale(LAND_BASE[g], LAND_LIGHT[l]);
                p[land(g as u8, l as u8, h as u8) as usize] = c565(mix(lit, HAZE, LAND_HAZE[h]));
                h += 1;
            }
            l += 1;
        }
        p[col::SHADOW as usize + g] = c565(scale(LAND_BASE[g], 132));
        g += 1;
    }
    let mut b = 0;
    while b < 4 {
        p[col::SKY as usize + b] = c565(SKY_BANDS[b]);
        b += 1;
    }
    p[col::HILLS as usize] = c565((126, 152, 164));
    p[col::GROUND as usize] = c565(mix(scale(LAND_BASE[0], LAND_LIGHT[1]), HAZE, 220));
    p[col::PLAIN as usize] = c565((255, 0, 255));
    p[col::DUST as usize] = c565((198, 180, 146));
    p[col::SPRAY as usize] = c565((232, 242, 250));
    p[col::TEXT as usize] = c565((250, 250, 240));
    p[col::SHADE as usize] = c565((0, 0, 0));
    p[col::WARN as usize] = c565((255, 128, 40));
    p[col::GOLD as usize] = c565((255, 220, 60));
    p[col::BOX as usize] = c565((18, 22, 32));
    p[col::FRAME as usize] = c565((200, 200, 196));
    p
}

/// The palette, set once (and again when the console asks).
pub static PALETTE: [u16; col::COUNT] = palette();

/// Choices for one picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Opts {
    /// The line of numbers is shown (SELECT).
    pub line: bool,
    /// Tests only: sky and ground in one plain colour ([`col::PLAIN`])
    /// with no hills and no text, so a crack in the land can be found.
    pub plain: bool,
}

/// Where the line of numbers is (its top row, and its left end): every
/// word keeps at least 4 pixels from the picture's edges (review look
/// F9).
pub const LINE_Y: i16 = 4;
pub const LINE_X: i16 = 4;
/// The rightmost column a word's shadow may reach (the lowest row in
/// use is 115: the speed's and the time's shadows).
const RIGHT: i16 = 155;

/// The row of the horizon on the picture for a camera looking down by
/// `pitch` (down is plus), in pixels (may lie off the picture).
pub fn horizon(pitch: u16) -> i32 {
    let (s, c) = (sin_fine(pitch), cos_fine(pitch).max(1));
    60 - s * 100 / c
}

/// The column angles (1/65,536 turn, from the middle) of the 160
/// columns' middles, for the far hills: atan((x + 0.5 - 80) / 100),
/// made while compiling (atan by `t (0.9724 - 0.1919 t^2)`, about 0.3
/// degrees at worst for |t| up to 0.8).
static COLUMN_ANGLE: [i16; W] = {
    let mut out = [0i16; W];
    let mut x = 0;
    while x < W {
        // t in 1/65536.
        let t = ((2 * x as i64 + 1 - 160) * 65_536) / 200;
        let t2 = t * t / 65_536;
        let a = t * (63_728 - 12_576 * t2 / 65_536) / 65_536; // radians in 1/65536
        out[x] = (a * 10_430 / 65_536) as i16;
        x += 1;
    }
    out
};

/// A triangle wave of an angle: 0 at 0, 256 at half a turn.
fn tri_wave(a: u16) -> i32 {
    let v = i32::from(a) - 32_768;
    256 - v.abs() / 128
}

/// The far hills' height (pixels) in the direction `a`.
fn hill(a: u16) -> i32 {
    let h1 = tri_wave(a.wrapping_mul(2).wrapping_add(9_000));
    let h2 = tri_wave(a.wrapping_mul(5).wrapping_add(21_000));
    let h3 = tri_wave(a.wrapping_mul(11).wrapping_add(4_000));
    1 + (h1 * 6 + h2 * 3 + h3) / 256
}

/// The sky, the far hills and the ground band below the horizon.
pub fn sky(f: &mut Frame, look: &Look<'_>, plain: bool) {
    let all = Rect::new(0, 0, W as i32, H as i32);
    if plain {
        rect_fill(f, &all, col::PLAIN);
        return;
    }
    let ch = look.chase();
    let yh = horizon(ch.pitch).clamp(0, H as i32);
    let bands = [0, yh - 30, yh - 14, yh - 5, yh];
    for b in 0..4 {
        let (y0, y1) = (bands[b].clamp(0, yh), bands[b + 1].clamp(0, yh));
        if y1 > y0 {
            rect_fill(f, &Rect::new(0, y0, W as i32, y1 - y0), col::SKY + b as u8);
        }
    }
    if yh < H as i32 {
        rect_fill(f, &Rect::new(0, yh, W as i32, H as i32 - yh), col::GROUND);
    }
    hills(f, ch.yaw, yh);
}

/// The far hills over the horizon `yh` (0 to `H`, as `sky` clamps it;
/// none at 0), seen
/// from `yaw`: in each column the rows `yh - h` to `yh - 1`, `h` the
/// hill's height there but at most `yh`.
///
/// Written straight into the picture (the speed work; it was one
/// `vline` a column): `hill` is at least 1 (each `tri_wave` is at
/// least 0), so `h` lies in 1..=yh and every row is inside the picture;
/// `vline`'s cut never cut anything. (A `yh` past `H` or a hill of 0
/// would stop the game at the slice, never draw elsewhere.)
/// `tests/speed_proofs.rs` checks it against the `vline` way for every
/// yaw and horizon.
pub fn hills(f: &mut Frame, yaw: u16, yh: i32) {
    if yh <= 0 {
        return;
    }
    let yh = yh as usize;
    for (x, &da) in COLUMN_ANGLE.iter().enumerate() {
        let a = yaw.wrapping_add(da as u16);
        let h = (hill(a) as usize).min(yh);
        for px in f.0[(yh - h) * W + x..yh * W].iter_mut().step_by(W) {
            *px = col::HILLS;
        }
    }
}

/// Draws the whole picture. `menu`: the START menu when it is open.
pub fn picture<R: Rec>(
    f: &mut Frame,
    paint: &mut Painter,
    look: &Look<'_>,
    menu: Option<&Menu>,
    opts: Opts,
    rec: &mut R,
) {
    sky(f, look, opts.plain);
    paint.scene(f, look, rec);
    if opts.plain {
        return;
    }
    words(f, look);
    if let Some(m) = menu {
        menu_box(f, m, look);
    }
    if opts.line {
        let b = line_text(paint, look);
        shaded(f, LINE_X, LINE_Y, b.as_str(), col::GOLD);
    }
}

/// The line of numbers (the spec's section 5): `T` triangles, `O`
/// objects, `F` front faces, `P` pixels painted (game-3d's counts for
/// this picture), `C` land cells and blocks drawn, `S` steps run in this
/// update.
pub fn line_text(paint: &Painter, look: &Look<'_>) -> Buf {
    let st = paint.r.stats;
    let mut b = Buf::new();
    b.text("T");
    b.num(st.tris as i32);
    b.text(" O");
    b.num(st.objects as i32);
    b.text(" F");
    b.num(st.front as i32);
    b.text(" P");
    b.num(st.pixels as i32);
    b.text(" C");
    b.num(paint.cells as i32);
    b.text(" S");
    b.num(i32::from(look.steps_last()));
    b
}

/// Speed, detail, run time, RESET and FINISH.
fn words(f: &mut Frame, look: &Look<'_>) {
    // The speed, big, bottom left, and KMH beside it.
    let mut b = Buf::new();
    b.num(look.car_kmh());
    let w = text_width(b.as_str()) as i16 * 2;
    f.text_scaled(5, 106, b.as_str(), col::SHADE, 2);
    f.text_scaled(4, 105, b.as_str(), col::TEXT, 2);
    shaded(f, 8 + w, 110, "KMH", col::TEXT);
    // The detail level, top right.
    let name = look.detail().name();
    shaded(f, RIGHT - text_width(name) as i16, 4, name, col::TEXT);
    if look.resetting().is_some() {
        let w = text_width("RESET") as i16;
        shaded(f, 80 - w / 2, 30, "RESET", col::WARN);
    } else if look.blocked() {
        // Stuck against a thing or the edge with the throttle held for
        // a second: say what gets the car out (review look F2).
        let hint = "B: REVERSE  Y: ROAD";
        let w = text_width(hint) as i16;
        shaded(f, 80 - w / 2, 30, hint, col::WARN);
    }
    if let Some(steps) = look.finished() {
        let mut b = Buf::new();
        b.text("FINISH ");
        seconds(&mut b, steps);
        b.text(" S");
        let w = text_width(b.as_str()) as i16;
        shaded(f, 80 - w / 2, 30, b.as_str(), col::GOLD);
    } else if look.run_steps() > 0 {
        let mut b = Buf::new();
        seconds(&mut b, look.run_steps());
        let w = text_width(b.as_str()) as i16;
        shaded(f, RIGHT - w, 110, b.as_str(), col::TEXT);
    }
}

/// The START menu: LOW, MID, HIGH, MAX, RESTART, the chosen line marked,
/// the level in use marked with "<" (a star read as a cross, review
/// look F10).
fn menu_box(f: &mut Frame, m: &Menu, look: &Look<'_>) {
    // 8 pixels taller than with four lines (MAX's line).
    let (x, y, w, h) = (48i16, 32i16, 64u16, 56u16);
    f.rect(x, y, w, h, col::BOX);
    f.outline(x, y, w, h, col::FRAME);
    let title = "PAUSED";
    f.text(80 - text_width(title) as i16 / 2, y + 4, title, col::GOLD);
    let now = look.detail() as u8;
    for (i, line) in Menu::LINES.iter().enumerate() {
        let ly = y + 14 + i as i16 * 8;
        let c = if i == usize::from(m.at) {
            col::GOLD
        } else {
            col::TEXT
        };
        if i == usize::from(m.at) {
            f.text(x + 6, ly, ">", c);
        }
        f.text(x + 14, ly, line, c);
        if i < 4 && i as u8 == now {
            f.text(x + 14 + text_width(line) as i16 + 3, ly, "<", c);
        }
    }
}

/// `steps` (30 a second) as seconds with two places.
pub fn seconds(b: &mut Buf, steps: u32) {
    let hundredths = (steps * 10 / 3) as i32;
    b.num(hundredths / 100);
    b.text(".");
    let h = hundredths % 100;
    if h < 10 {
        b.text("0");
    }
    b.num(h);
}

/// Text with a dark shadow one pixel down and right, so it reads on any
/// land.
fn shaded(f: &mut Frame, x: i16, y: i16, s: &str, c: u8) {
    f.text(x + 1, y + 1, s, col::SHADE);
    f.text(x, y, s, c);
}

/// A short text buffer (no heap).
pub struct Buf {
    b: [u8; 40],
    n: usize,
}

impl Default for Buf {
    fn default() -> Self {
        Buf::new()
    }
}

impl Buf {
    pub fn new() -> Buf {
        Buf { b: [0; 40], n: 0 }
    }
    pub fn text(&mut self, s: &str) {
        for &c in s.as_bytes() {
            if self.n < self.b.len() {
                self.b[self.n] = c;
                self.n += 1;
            }
        }
    }
    pub fn num(&mut self, v: i32) {
        if v < 0 {
            self.text("-");
        }
        let mut d = [0u8; 10];
        let mut k = 0;
        let mut u = v.unsigned_abs();
        loop {
            d[k] = b'0' + (u % 10) as u8;
            k += 1;
            u /= 10;
            if u == 0 {
                break;
            }
        }
        while k > 0 {
            k -= 1;
            if self.n < self.b.len() {
                self.b[self.n] = d[k];
                self.n += 1;
            }
        }
    }
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.b[..self.n]).unwrap_or("")
    }
}
