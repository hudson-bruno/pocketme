//! Drawing: the palette, the pixel art, and painting a room.
//!
//! Every colour is a named palette number set once at start (and again
//! when the console asks for a redraw). The game never changes a
//! palette colour while it runs: on this console any palette change
//! sends the whole screen again. Animation is done by drawing other
//! pixels instead.
//!
//! The look is easy to change: colours are in [`COLOURS`], sizes and
//! places are named constants, and the art is letters in small tables.

use game_kit::{CLEAR, Frame, Sprite, W, rgb565, text_width};

use crate::rooms::{self, Room, TILE, VIEW_H, VIEW_W};
use crate::sim::{self, BOX_W, Ghost, Pose, Runner, World};

// ---- Palette numbers --------------------------------------------------------

pub const SKY_TOP: u8 = 1;
pub const SKY_MID: u8 = 2;
pub const SKY_LOW: u8 = 3;
pub const FAR: u8 = 4;
pub const FAR_LIT: u8 = 5;
pub const STAR: u8 = 6;
pub const BRICK: u8 = 7;
pub const BRICK_DARK: u8 = 8;
pub const ROOF_CAP: u8 = 9;
pub const METAL: u8 = 10;
pub const METAL_LIGHT: u8 = 11;
pub const METAL_DARK: u8 = 12;
pub const AWN_RED: u8 = 13;
pub const AWN_WHITE: u8 = 14;
pub const GLASS: u8 = 15;
pub const GLASS_DARK: u8 = 16;
pub const DOOR_LIGHT: u8 = 17;
pub const DOOR_FRAME: u8 = 18;
pub const INK: u8 = 19;
pub const SKIN: u8 = 20;
pub const CAP: u8 = 21;
pub const JACKET: u8 = 22;
pub const BAG: u8 = 23;
pub const LAMP_ON: u8 = 24;
pub const LAMP_OFF: u8 = 25;
pub const WHITE: u8 = 26;
pub const GHOST: u8 = 27;
pub const TEXT: u8 = 28;
pub const TEXT_DIM: u8 = 29;
pub const BAR: u8 = 30;
pub const ACCENT: u8 = 31;
pub const MOON: u8 = 32;
pub const PANTS: u8 = 33;
pub const MOON_DARK: u8 = 34;

/// The first palette number set; [`COLOURS`] fills from here.
pub const FIRST: u8 = SKY_TOP;

/// The colours, in palette order from [`FIRST`]. A night over the roofs:
/// dark blues, warm brick, one bright lamp.
pub const COLOURS: [u16; 34] = [
    rgb565(14, 16, 40),    // SKY_TOP
    rgb565(26, 28, 64),    // SKY_MID
    rgb565(52, 38, 84),    // SKY_LOW: a little light from the city
    rgb565(32, 30, 58),    // FAR buildings
    rgb565(120, 104, 60),  // FAR_LIT windows
    rgb565(190, 196, 230), // STAR
    rgb565(122, 62, 58),   // BRICK
    rgb565(74, 36, 44),    // BRICK_DARK: mortar
    rgb565(176, 112, 92),  // ROOF_CAP: the lit top edge
    rgb565(84, 98, 120),   // METAL
    rgb565(140, 156, 176), // METAL_LIGHT
    rgb565(46, 54, 72),    // METAL_DARK
    rgb565(206, 72, 60),   // AWN_RED
    rgb565(236, 226, 206), // AWN_WHITE
    rgb565(170, 236, 246), // GLASS
    rgb565(70, 150, 176),  // GLASS_DARK
    rgb565(252, 208, 96),  // DOOR_LIGHT
    rgb565(60, 36, 30),    // DOOR_FRAME
    rgb565(20, 16, 30),    // INK: outlines, shoes
    rgb565(240, 192, 152), // SKIN
    rgb565(236, 238, 244), // CAP: a white helmet
    rgb565(244, 140, 44),  // JACKET: a bright orange vest
    rgb565(150, 110, 70),  // BAG
    rgb565(255, 240, 110), // LAMP_ON: the dash is ready
    rgb565(70, 70, 92),    // LAMP_OFF: the dash is used
    rgb565(255, 255, 255), // WHITE
    rgb565(90, 130, 190),  // GHOST: dash copies
    rgb565(240, 236, 224), // TEXT
    rgb565(150, 146, 178), // TEXT_DIM
    rgb565(10, 10, 22),    // BAR
    rgb565(255, 170, 70),  // ACCENT
    rgb565(236, 232, 200), // MOON
    rgb565(40, 44, 90),    // PANTS
    rgb565(190, 186, 160), // MOON_DARK
];

// ---- Layout -------------------------------------------------------------------

/// The bar at the top: room, retries, time.
pub const BAR_H: i16 = 8;
/// Where the room's picture starts (under the bar).
pub const ROOM_Y: i16 = BAR_H;
/// The sky bands (room pixels from the top) and the dithered rows above
/// each change.
pub const SKY_MID_Y: i32 = 36;
pub const SKY_LOW_Y: i32 = 72;
pub const DITHER_ROWS: i32 = 4;
/// Far buildings: block width, lowest and highest (pixels above the
/// bottom of the view).
pub const FAR_BLOCK: i32 = 12;
pub const FAR_MIN: i32 = 26;
pub const FAR_SPAN: i32 = 44;
/// The moon's top-left (room view pixels; it does not scroll).
pub const MOON_X: i16 = 126;
pub const MOON_Y: i16 = 6;

// ---- Pixel art ------------------------------------------------------------

/// Turns art letters into palette numbers, while compiling.
const fn ink(c: u8) -> u8 {
    match c {
        b'.' => CLEAR,
        b'b' => BRICK,
        b'd' => BRICK_DARK,
        b'r' => ROOF_CAP,
        b'm' => METAL,
        b'M' => METAL_LIGHT,
        b'n' => METAL_DARK,
        b'R' => AWN_RED,
        b'W' => AWN_WHITE,
        b'g' => GLASS,
        b'G' => GLASS_DARK,
        b'L' => DOOR_LIGHT,
        b'F' => DOOR_FRAME,
        b'o' => INK,
        b's' => SKIN,
        b'c' => CAP,
        b'j' => JACKET,
        b'p' => PANTS,
        b'B' => BAG,
        b'l' => LAMP_ON,
        b'w' => WHITE,
        b'y' => MOON,
        b'Y' => MOON_DARK,
        b'a' => ACCENT,
        _ => panic!("art: unknown letter"),
    }
}

const fn art<const N: usize>(s: &[u8; N]) -> [u8; N] {
    let mut out = [0; N];
    let mut i = 0;
    while i < N {
        out[i] = ink(s[i]);
        i += 1;
    }
    out
}

#[rustfmt::skip]
const BRICK_A: [u8; 64] = art(b"\
bbbdbbbb\
bbbdbbbb\
bbbdbbbb\
dddddddd\
bbbbbbbd\
bbbbbbbd\
bbbbbbbd\
dddddddd");

#[rustfmt::skip]
const BRICK_CAP: [u8; 64] = art(b"\
rrrrrrrr\
dddddddd\
bbbdbbbb\
bbbdbbbb\
dddddddd\
bbbbbbbd\
bbbbbbbd\
dddddddd");

#[rustfmt::skip]
const METAL_ART: [u8; 64] = art(b"\
MMMMMMMn\
MmmmmmMn\
Mmnmmnmn\
Mmmmmmmn\
Mmmmmmmn\
Mmnmmnmn\
Mmmmmmmn\
nnnnnnnn");

#[rustfmt::skip]
const CHIMNEY_ART: [u8; 64] = art(b"\
nnnnnnnn\
MMMMMMMM\
nbbbbbbn\
.bbdbbb.\
.dddddd.\
.bbbbdb.\
.bbbbdb.\
.dddddd.");

#[rustfmt::skip]
const AWNING_ART: [u8; 64] = art(b"\
RRWWRRWW\
RRWWRRWW\
RRWWRRWW\
.R..R..R\
........\
........\
........\
........");

#[rustfmt::skip]
const GLASS_UP_ART: [u8; 64] = art(b"\
........\
........\
........\
........\
..g...g.\
.gG..gG.\
.gGgggGg\
GGGGGGGG");

#[rustfmt::skip]
const GLASS_DOWN_ART: [u8; 64] = art(b"\
GGGGGGGG\
gGgggGg.\
.Gg..Gg.\
.g...g..\
........\
........\
........\
........");

#[rustfmt::skip]
const GLASS_RIGHT_ART: [u8; 64] = art(b"\
G.......\
Ggg.....\
GGg.....\
Gg......\
G.......\
Ggg.....\
GGgg....\
Gg......");

#[rustfmt::skip]
const GLASS_LEFT_ART: [u8; 64] = art(b"\
.......G\
.....ggG\
.....gGG\
......gG\
.......G\
.....ggG\
....ggGG\
......gG");

#[rustfmt::skip]
const DOOR_TOP: [u8; 64] = art(b"\
FFFFFFFF\
FaaaaaaF\
FFFFFFFF\
FLLLLLLF\
FLLLLLLF\
FLLLLLLF\
FLLLLLLF\
FLLLLLLF");

#[rustfmt::skip]
const DOOR_BOTTOM: [u8; 64] = art(b"\
FLLLLLLF\
FLLLLLLF\
FLLLLoLF\
FLLLLLLF\
FLLLLLLF\
FLLLLLLF\
FLLLLLLF\
FFFFFFFF");

#[rustfmt::skip]
const AERIAL_ART: [u8; 64] = art(b"\
....R...\
....M...\
..MMMMM.\
....M...\
...MMM..\
....M...\
....M...\
...nnn..");

#[rustfmt::skip]
const MOON_ART: [u8; 100] = art(b"\
...yyyy...\
..yyyyyy..\
.yyYyyyyy.\
yyyyyyYyyy\
yyyyyyyyyy\
yYyyyyyyyy\
yyyyyYyyyy\
.yyyyyyyy.\
..yyyyyy..\
...yyyy...");

/// The runner, 8 x 10, facing right. `l` is the lamp: lit while the
/// dash is ready, dark after it is used.
pub const SPRITE_W: i32 = 8;
pub const SPRITE_H: i32 = 10;

#[rustfmt::skip]
const STAND: [u8; 80] = art(b"\
..cccc..\
..cccccc\
..ssss..\
..ssos..\
BBjjjj..\
lBjjjjs.\
BBjjjj..\
..pppp..\
..p..p..\
..oo.oo.");

#[rustfmt::skip]
const RUN1: [u8; 80] = art(b"\
..cccc..\
..cccccc\
..ssss..\
..ssos..\
BBjjjj..\
lBjjjjs.\
BBjjjj..\
..pppp..\
.pp...p.\
.o....oo");

#[rustfmt::skip]
const RUN2: [u8; 80] = art(b"\
........\
..cccc..\
..cccccc\
..ssss..\
BBssos..\
lBjjjj..\
BBjjjjs.\
..pppp..\
...pp...\
...ooo..");

#[rustfmt::skip]
const RISE: [u8; 80] = art(b"\
..cccc..\
..cccccc\
..ssss.s\
..ssosj.\
BBjjjj..\
lBjjjj..\
BBjjjj..\
..pppp..\
..p.pp..\
..o..o..");

#[rustfmt::skip]
const FALL: [u8; 80] = art(b"\
..cccc..\
..cccccc\
..ssss..\
..ssos..\
BBjjjjs.\
lBjjjj..\
BBjjjj..\
..pppp..\
.p...p..\
.o...oo.");

#[rustfmt::skip]
const SLIDE: [u8; 80] = art(b"\
..cccc..\
..cccccc\
..ssss.s\
..ssosjs\
BBjjjj..\
lBjjjj..\
BBjjjj..\
..pppp..\
..pp.p..\
..oo..o.");

#[rustfmt::skip]
const DASH: [u8; 80] = art(b"\
........\
...cccc.\
...ccccc\
..sssos.\
BBjjjjj.\
lBjjjjjs\
BBjjjj..\
.pppp...\
pp..pp..\
o....oo.");

/// Puff dots: where each goes (x, y steps per 2 ticks).
const PUFF_DIRS: [(i32, i32); 8] = [
    (2, 0),
    (-2, 0),
    (0, -2),
    (0, 2),
    (1, 1),
    (-1, 1),
    (1, -1),
    (-1, -1),
];

/// A few fixed stars (view pixels).
const STARS: [(i16, i16); 18] = [
    (6, 4),
    (20, 14),
    (33, 6),
    (47, 22),
    (58, 3),
    (71, 16),
    (85, 9),
    (96, 27),
    (104, 4),
    (117, 20),
    (141, 26),
    (152, 7),
    (12, 30),
    (66, 34),
    (90, 40),
    (134, 38),
    (26, 44),
    (110, 46),
];

// ---- Painting ---------------------------------------------------------------

/// A small mixing function for the city's random looks (same every run).
pub fn hash(a: i32, b: i32) -> u32 {
    let mut x = (a as u32).wrapping_mul(0x9E37_79B1) ^ (b as u32).wrapping_mul(0x85EB_CA77);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x
}

/// The top of the far building at room view column `wx` (shifted by
/// the parallax), in view pixels.
fn far_top(wx: i32) -> i32 {
    let b = wx.div_euclid(FAR_BLOCK);
    VIEW_H - FAR_MIN - (hash(b, 7) % FAR_SPAN as u32) as i32
}

/// The sky colour of a view pixel.
fn sky(x: i32, y: i32) -> u8 {
    let dither = |edge: i32, above: u8, below: u8| {
        if y >= edge || (y >= edge - DITHER_ROWS && (x + y) % 2 == 0) {
            below
        } else {
            above
        }
    };
    if y < SKY_MID_Y {
        dither(SKY_MID_Y, SKY_TOP, SKY_MID)
    } else {
        dither(SKY_LOW_Y, SKY_MID, SKY_LOW)
    }
}

/// Paints the sky, the moon, the stars and the far city of a view whose
/// left edge is `cam` (room pixels). The far city moves at half speed.
/// Writes every pixel of the view: 160 x 112.
pub fn paint_back(s: &mut Frame, cam: i32) {
    paint_back_moon(s, cam, MOON_X, MOON_Y);
}

/// [`paint_back`] with the moon somewhere else (view pixels).
pub fn paint_back_moon(s: &mut Frame, cam: i32, moon_x: i16, moon_y: i16) {
    let par = cam / 2;
    let mut tops = [0i32; W];
    for (x, t) in tops.iter_mut().enumerate() {
        *t = far_top(x as i32 + par);
    }
    for y in 0..VIEW_H {
        let start = (y + i32::from(ROOM_Y)) as usize * W;
        let row = &mut s.0[start..start + W];
        for (x, p) in row.iter_mut().enumerate() {
            let xi = x as i32;
            *p = if y >= tops[x] {
                let wx = xi + par;
                let lx = wx.rem_euclid(FAR_BLOCK);
                let lit = matches!(lx, 3 | 4 | 7 | 8)
                    && y % 6 >= 2
                    && y % 6 < 4
                    && hash(wx.div_euclid(FAR_BLOCK), y / 6).is_multiple_of(4);
                if lit { FAR_LIT } else { FAR }
            } else {
                sky(xi, y)
            };
        }
    }
    for &(x, y) in &STARS {
        if i32::from(y) < tops[x as usize] {
            s.pixel(x, y + ROOM_Y, STAR);
        }
    }
    s.sprite(
        moon_x,
        moon_y + ROOM_Y,
        &Sprite::new(10, 10, &MOON_ART),
        CLEAR,
    );
}

/// The art of the tile at (col, row), or `None` for air.
fn tile_art(room: &Room, col: i32, row: i32) -> Option<&'static [u8; 64]> {
    let t = room.at(col, row);
    Some(match t {
        rooms::BRICK => {
            if rooms::is_solid(room.at(col, row - 1)) || row == 0 {
                &BRICK_A
            } else {
                &BRICK_CAP
            }
        }
        rooms::METAL => &METAL_ART,
        rooms::CHIMNEY => &CHIMNEY_ART,
        rooms::AWNING => &AWNING_ART,
        rooms::GLASS_UP => &GLASS_UP_ART,
        rooms::GLASS_DOWN => &GLASS_DOWN_ART,
        rooms::GLASS_LEFT => &GLASS_LEFT_ART,
        rooms::GLASS_RIGHT => &GLASS_RIGHT_ART,
        rooms::DOOR => {
            if room.at(col, row - 1) == rooms::DOOR {
                &DOOR_BOTTOM
            } else {
                &DOOR_TOP
            }
        }
        rooms::AERIAL => &AERIAL_ART,
        _ => return None,
    })
}

/// Paints the room's tiles over the back, for a view at `cam`.
pub fn paint_tiles(s: &mut Frame, room: &Room, cam: i32) {
    let c0 = cam.div_euclid(TILE);
    let c1 = (cam + VIEW_W - 1).div_euclid(TILE);
    for row in 0..rooms::ROWS as i32 {
        for col in c0..=c1 {
            if let Some(a) = tile_art(room, col, row) {
                let x = (col * TILE - cam) as i16;
                let y = (row * TILE) as i16 + ROOM_Y;
                s.sprite(x, y, &Sprite::new(8, 8, a), CLEAR);
            }
        }
    }
}

/// The runner's art for a pose and the time.
fn frame_for(r: &Runner) -> &'static [u8; 80] {
    match r.pose() {
        Pose::Stand => &STAND,
        Pose::Run => {
            if (r.stride / 6).is_multiple_of(2) {
                &RUN1
            } else {
                &RUN2
            }
        }
        Pose::Rise => &RISE,
        Pose::Fall => &FALL,
        Pose::Slide => &SLIDE,
        Pose::Dash => &DASH,
    }
}

/// Draws an 8 x 10 runner picture with its box at room (`bx`, `by`).
/// `recolour` turns each art pixel (and its column and row) into the
/// colour drawn, or `None` to skip it.
fn draw_figure(
    s: &mut Frame,
    pic: &[u8; 80],
    bx: i32,
    by: i32,
    facing: i8,
    cam: i32,
    mut recolour: impl FnMut(u8, i32, i32) -> Option<u8>,
) {
    // The body sits in columns 2..7 facing right (the bag is behind).
    let left = if facing >= 0 {
        bx - 2
    } else {
        bx - (SPRITE_W - BOX_W - 2)
    };
    let top = by - 2;
    for j in 0..SPRITE_H {
        for i in 0..SPRITE_W {
            let src = if facing >= 0 { i } else { SPRITE_W - 1 - i };
            let p = pic[(j * SPRITE_W + src) as usize];
            if p == CLEAR {
                continue;
            }
            let x = left + i - cam;
            let y = top + j;
            if (0..VIEW_W).contains(&x)
                && (0..VIEW_H).contains(&y)
                && let Some(c) = recolour(p, i, j)
            {
                s.pixel(x as i16, y as i16 + ROOM_Y, c);
            }
        }
    }
}

/// Draws the runner, its dash ghosts and the last puff.
pub fn draw_actors(s: &mut Frame, w: &World) {
    let cam = w.cam;
    for g in w.ghosts.iter().rev() {
        draw_ghost(s, g, cam);
    }
    let r = &w.runner;
    let pic = frame_for(r);
    let lamp = if r.flash > 0 {
        WHITE
    } else if r.dashes > 0 {
        LAMP_ON
    } else {
        LAMP_OFF
    };
    let freeze = r.freeze > 0;
    draw_figure(s, pic, r.x, r.y, r.facing, cam, |p, _, _| {
        Some(if freeze {
            WHITE
        } else if p == LAMP_ON {
            lamp
        } else {
            p
        })
    });
    // A lit lamp glows a little: one pixel above and below it.
    if r.dashes > 0 && !freeze {
        let lx = if r.facing >= 0 {
            r.x - 3
        } else {
            r.x + BOX_W + 2
        };
        let ly = r.y + 3;
        for (dx, dy) in [(0, 0), (0, 2)] {
            let (x, y) = (lx + dx - cam, ly + dy);
            if (0..VIEW_W).contains(&x) && (0..VIEW_H).contains(&y) {
                s.pixel(x as i16, y as i16 + ROOM_Y, LAMP_ON);
            }
        }
    }
    let (px, py, life) = w.puff;
    if life > 0 {
        let k = i32::from(sim::PUFF_LIFE - life) / 2;
        let c = if life > sim::PUFF_LIFE / 2 {
            WHITE
        } else {
            TEXT_DIM
        };
        for &(dx, dy) in &PUFF_DIRS {
            let x = px + dx * k - cam;
            let y = py + dy * k;
            if (0..VIEW_W - 1).contains(&x) && (0..VIEW_H - 1).contains(&y) {
                s.rect(x as i16, y as i16 + ROOM_Y, 2, 2, c);
            }
        }
    }
}

fn draw_ghost(s: &mut Frame, g: &Ghost, cam: i32) {
    if g.life == 0 {
        return;
    }
    // Old ghosts show every other pixel: they fade out.
    let thin = g.life < sim::GHOST_LIFE / 2;
    draw_figure(s, &DASH, g.x, g.y, g.facing, cam, |_, i, j| {
        (!thin || (i + j) % 2 == 0).then_some(GHOST)
    });
}

/// Paints everything of the play screen except the bar.
pub fn paint_play(s: &mut Frame, w: &World) {
    paint_back(s, w.cam);
    paint_tiles(s, w.room, w.cam);
    draw_actors(s, w);
}

// ---- Text -------------------------------------------------------------------

/// A small text buffer for numbers and names (no heap).
pub struct Line {
    buf: [u8; 40],
    len: usize,
}

impl Line {
    pub const fn new() -> Line {
        Line {
            buf: [0; 40],
            len: 0,
        }
    }

    pub fn push(&mut self, s: &str) -> &mut Line {
        for &b in s.as_bytes() {
            if self.len < self.buf.len() {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
        self
    }

    /// A number, at least `width` digits (zeros in front).
    pub fn num(&mut self, n: u32, width: usize) -> &mut Line {
        let mut digits = [0u8; 10];
        let mut i = digits.len();
        let mut v = n;
        loop {
            i -= 1;
            digits[i] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        while digits.len() - i < width && i > 0 {
            i -= 1;
            digits[i] = b'0';
        }
        for &d in &digits[i..] {
            if self.len < self.buf.len() {
                self.buf[self.len] = d;
                self.len += 1;
            }
        }
        self
    }

    /// Ticks (1/60 s) as M:SS, or M:SS.HH with hundredths.
    pub fn time(&mut self, ticks: u32, hundredths: bool) -> &mut Line {
        let cs = u64::from(ticks) * 100 / 60;
        let secs = cs / 100;
        let mins = (secs / 60).min(99) as u32;
        let s = if secs / 60 > 99 {
            59
        } else {
            (secs % 60) as u32
        };
        self.num(mins, 1).push(":").num(s, 2);
        if hundredths {
            self.push(".").num((cs % 100) as u32, 2);
        }
        self
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("?")
    }
}

impl Default for Line {
    fn default() -> Self {
        Line::new()
    }
}

/// The left edge that centres `text` at `scale`.
pub fn centre(text: &str, scale: u16) -> i16 {
    ((W as u16).saturating_sub(text_width(text).saturating_mul(scale)) / 2) as i16
}

/// The bar at the top during play.
pub fn paint_bar(s: &mut Frame, room_no: usize, name: &str, retries: u32, ticks: u32, lamp: bool) {
    s.rect(0, 0, W as u16, BAR_H as u16, BAR);
    let mut l = Line::new();
    l.num(room_no as u32, 1).push(" ").push(name);
    s.text(2, 1, l.as_str(), TEXT);
    let mut t = Line::new();
    t.time(ticks, false);
    let tx = W as i16 - 2 - text_width(t.as_str()) as i16;
    s.text(tx, 1, t.as_str(), TEXT);
    let mut r = Line::new();
    r.push("X").num(retries.min(9999), 1);
    let rx = tx - 8 - text_width(r.as_str()) as i16;
    s.text(rx, 1, r.as_str(), TEXT_DIM);
    // The dash lamp, as on the runner's back.
    let c = if lamp { LAMP_ON } else { LAMP_OFF };
    s.rect(rx - 7, 2, 3, 4, c);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_written_out() {
        let mut l = Line::new();
        l.time(0, true);
        assert_eq!(l.as_str(), "0:00.00");
        let mut l = Line::new();
        l.time(60 * 75 + 30, true);
        assert_eq!(l.as_str(), "1:15.50");
        let mut l = Line::new();
        l.time(u32::MAX, true);
        assert!(l.as_str().starts_with("99:59"));
    }

    #[test]
    fn a_line_never_overflows() {
        let mut l = Line::new();
        for _ in 0..20 {
            l.push("ROOFTOP").num(u32::MAX, 12);
        }
        assert_eq!(l.as_str().len(), 40);
    }
}
