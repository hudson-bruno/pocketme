//! Drawing: everything the player sees, from the game's state.
//!
//! The whole picture is drawn again every call; the console compares it
//! with what the screen shows and sends only what changed. So the
//! cost on the screen depends on how much really changes, not on how
//! much is drawn here. Drawing itself is bounded: one pass over the
//! 19,200 pixels for sky and ground, plus small things (at most 48
//! bits, one craft of at most 29 x 29 pixels, a few lines of text).
//!
//! Sizes and colours are named constants at the top, so the look is
//! easy to change.

use game_kit::{Frame, rgb565, text_width};

use crate::app::{App, Cam, Ending, PAUSE_LINES, Scene, Steer, TITLE_LINES, View};
use crate::craft::{FULL, SAFE_DOWN, SAFE_SIDE};
use crate::fixed::{PX, Rng, turn};
use crate::fx::Kind;
use crate::level::World;

// ---- Colours: palette numbers 1.. that the game sets at start -----------

pub const SKY: u8 = 1;
pub const STAR: u8 = 2;
pub const STAR_DIM: u8 = 3;
pub const SURFACE: u8 = 4;
pub const ROCK1: u8 = 5;
pub const ROCK2: u8 = 6;
pub const ROCK3: u8 = 7;
pub const PIT: u8 = 8;
pub const PAD: u8 = 9;
pub const PAD_DARK: u8 = 10;
pub const HULL: u8 = 11;
pub const SHADE: u8 = 12;
pub const GLASS: u8 = 13;
pub const FOIL: u8 = 14;
pub const LEG: u8 = 15;
pub const HOT: u8 = 16;
pub const FLAME: u8 = 17;
pub const TIP: u8 = 18;
pub const DUST: u8 = 19;
pub const HUD: u8 = 20;
pub const TEXT: u8 = 21;
pub const QUIET: u8 = 22;
pub const GOOD: u8 = 23;
pub const BAD: u8 = 24;
pub const WARN: u8 = 25;
pub const PANEL: u8 = 26;
pub const EDGE: u8 = 27;
pub const GOLD: u8 = 28;

/// Palette numbers 1 to 28, in order.
pub const COLOURS: [u16; 28] = [
    rgb565(6, 8, 22),      // SKY
    rgb565(220, 224, 245), // STAR
    rgb565(90, 96, 135),   // STAR_DIM
    rgb565(196, 192, 182), // SURFACE: the ground's top row
    rgb565(132, 128, 120), // ROCK1
    rgb565(88, 85, 82),    // ROCK2
    rgb565(56, 54, 56),    // ROCK3
    rgb565(36, 34, 38),    // PIT: small dark dots in the rock
    rgb565(255, 214, 64),  // PAD
    rgb565(160, 116, 32),  // PAD_DARK
    rgb565(232, 236, 242), // HULL
    rgb565(122, 130, 148), // SHADE
    rgb565(70, 205, 235),  // GLASS
    rgb565(235, 140, 40),  // FOIL
    rgb565(170, 175, 186), // LEG
    rgb565(255, 252, 210), // HOT: flame by the nozzle
    rgb565(255, 168, 40),  // FLAME
    rgb565(215, 60, 32),   // TIP
    rgb565(160, 154, 142), // DUST
    rgb565(14, 18, 40),    // HUD: the top bar
    rgb565(232, 234, 242), // TEXT
    rgb565(132, 142, 176), // QUIET
    rgb565(90, 222, 112),  // GOOD
    rgb565(245, 72, 60),   // BAD
    rgb565(250, 192, 50),  // WARN
    rgb565(18, 24, 52),    // PANEL: boxes behind text
    rgb565(100, 120, 190), // EDGE: their outline
    rgb565(255, 205, 80),  // GOLD: the title
];

// ---- Sizes and places --------------------------------------------------

/// The top bar's height.
pub const HUD_H: i32 = 9;
/// Rows of the ground's bands below its surface (world pixels).
const BAND1: i32 = 5;
const BAND2: i32 = 16;
/// One rock pixel in this many gets a dark dot (texture).
const PIT_EVERY: u32 = 23;
/// Stars, and the band of rows they sit in.
const STARS: usize = 40;
/// Title menu: left, first line, line step.
pub const MENU_X: i16 = 10;
pub const MENU_Y: i16 = 36;
pub const MENU_STEP: i16 = 9;
/// The game's pause menu box.
const PAUSE_X: i16 = 40;
const PAUSE_Y: i16 = 34;
const PAUSE_W: u16 = 80;
const PAUSE_FIRST: i16 = PAUSE_Y + 20;
/// The big craft on the title: its middle, and its size.
const TITLE_CRAFT: (i32, i32) = (128, 52);
const TITLE_ZOOM: i32 = 3;

// ---- The craft's picture: 9 x 9, middle at (4, 4) ------------------------

const O: u8 = 0; // nothing
#[rustfmt::skip]
const CRAFT: [u8; 81] = [
    O,    O,    O,    HULL, HULL, HULL, O,    O,    O,
    O,    O,    HULL, GLASS,GLASS,HULL, SHADE,O,    O,
    O,    HULL, HULL, GLASS,HULL, HULL, HULL, SHADE,O,
    O,    FOIL, FOIL, FOIL, FOIL, FOIL, FOIL, FOIL, O,
    O,    SHADE,HULL, HULL, HULL, HULL, HULL, SHADE,O,
    O,    O,    SHADE,SHADE,SHADE,SHADE,SHADE,O,    O,
    O,    LEG,  O,    O,    O,    O,    O,    LEG,  O,
    LEG,  O,    O,    O,    O,    O,    O,    O,    LEG,
    LEG,  LEG,  O,    O,    O,    O,    O,    LEG,  LEG,
];
/// Where the flame starts, below the middle (pixels).
const NOZZLE_Y: i32 = 2;
/// The longest flame, pixels.
const FLAME_LEN: i32 = 9;

// ---- Small helpers -----------------------------------------------------

/// A number written out.
pub struct Num {
    buf: [u8; 11],
    at: usize,
}

impl Num {
    pub fn new(v: i64) -> Num {
        let mut n = Num {
            buf: [0; 11],
            at: 11,
        };
        let neg = v < 0;
        let mut u = v.unsigned_abs().min(9_999_999_999);
        loop {
            n.at -= 1;
            n.buf[n.at] = b'0' + (u % 10) as u8;
            u /= 10;
            if u == 0 {
                break;
            }
        }
        if neg {
            n.at -= 1;
            n.buf[n.at] = b'-';
        }
        n
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[self.at..]).unwrap_or("?")
    }
}

fn centre(text: &str, scale: u16) -> i16 {
    (160u16.saturating_sub(text_width(text).saturating_mul(scale)) / 2) as i16
}

/// A box with an outline.
fn panel(f: &mut Frame, x: i16, y: i16, w: u16, h: u16) {
    f.rect(x, y, w, h, PANEL);
    f.outline(x, y, w, h, EDGE);
}

/// Which title menu line is at picture pixel `(x, y)`.
pub fn title_line_at(x: i16, y: i16) -> Option<u8> {
    if !(0..110).contains(&x) {
        return None;
    }
    // In i32: a touch can be anywhere in the i16 range.
    let k = (i32::from(y) - i32::from(MENU_Y) + 2).div_euclid(i32::from(MENU_STEP));
    (0..TITLE_LINES as i32).contains(&k).then_some(k as u8)
}

/// Which pause menu line is at picture pixel `(x, y)`.
pub fn pause_line_at(x: i16, y: i16) -> Option<u8> {
    if x < PAUSE_X || x >= PAUSE_X + PAUSE_W as i16 {
        return None;
    }
    let k = (i32::from(y) - i32::from(PAUSE_FIRST) + 2).div_euclid(i32::from(MENU_STEP));
    (0..PAUSE_LINES as i32).contains(&k).then_some(k as u8)
}

// ---- The scene -----------------------------------------------------------

impl App {
    /// Draws the whole picture.
    pub fn draw(&self, f: &mut Frame) {
        ground(f, &self.world, self.cam);
        stars(f, self.cam);
        if self.scene == Scene::Title {
            self.draw_craft(f);
            self.title(f);
            return;
        }
        pad_labels(f, &self.world, self.cam);
        self.draw_bits(f);
        self.wind_label(f);
        let gone = self.scene == Scene::Broken || self.scene == Scene::Over;
        if !gone {
            self.draw_craft(f);
        }
        self.hud(f);
        match self.scene {
            Scene::Ready => self.card_ready(f),
            Scene::Landed => self.card_landed(f),
            Scene::Broken => self.card_broken(f),
            Scene::Over => self.card_over(f),
            _ => {}
        }
        if let Some((text, _)) = self.note {
            let w = text_width(text) + 8;
            let x = ((160 - w) / 2) as i16;
            panel(f, x, HUD_H as i16 + 3, w, 11);
            f.text(x + 4, HUD_H as i16 + 6, text, WARN);
        }
        if let Some(sel) = self.pause {
            pause(f, sel);
        }
    }

    fn draw_craft(&self, f: &mut Frame) {
        let (sx, sy) = self.to_screen(self.craft.x, self.craft.y);
        if self.scene == Scene::Title {
            craft_at(
                f,
                TITLE_CRAFT.0,
                TITLE_CRAFT.1,
                self.craft.step(),
                TITLE_ZOOM,
                self.craft.burn,
                self.clock,
            );
            return;
        }
        craft_at(
            f,
            sx,
            sy,
            self.craft.step(),
            self.cam.zoom,
            self.craft.burn,
            self.clock,
        );
        // Off the top: a small arrow at the craft's column.
        if sy < HUD_H - 2 && self.scene != Scene::Title {
            let x = sx.clamp(2, 157) as i16;
            let y = HUD_H as i16 + 1;
            f.pixel(x, y, WARN);
            f.hline(x - 1, y + 1, 3, WARN);
            f.hline(x - 2, y + 2, 5, WARN);
        }
    }

    fn draw_bits(&self, f: &mut Frame) {
        let z = self.cam.zoom;
        for b in self.bits.list.iter().filter(|b| b.life > 0) {
            let (x, y) = self.to_screen(b.x, b.y);
            match b.kind {
                Kind::Dust => plot(f, x, y, z, DUST),
                Kind::Spark => plot(f, x, y, z, if b.life > 8 { HOT } else { FLAME }),
                Kind::Piece(i) => {
                    let c = [HULL, FOIL, SHADE, GLASS][usize::from(i % 4)];
                    let s = (2 * z) as u16;
                    f.rect(x as i16, y as i16, s, s, c);
                }
            }
        }
    }

    // ---- The top bar ------------------------------------------------------

    fn hud(&self, f: &mut Frame) {
        f.rect(0, 0, 160, HUD_H as u16, HUD);
        let c = &self.craft;
        let x = f.text(2, 2, "FUEL", QUIET) + 2;
        // Fuel bar: 24 pixels.
        let full = self.world.params.fuel.max(1);
        let pct = u64::from(c.fuel) * 100 / u64::from(full);
        let w = (u64::from(c.fuel) * 24).div_ceil(u64::from(full)) as u16;
        let col = match pct {
            0..=9 => BAD,
            10..=24 => WARN,
            _ => GOOD,
        };
        f.rect(x, 2, 24, 5, PANEL);
        f.rect(x, 2, w.min(24), 5, col);
        // Speeds in pixels a second: down, and sideways.
        let vy = i64::from(c.vy) * 60 / i64::from(PX);
        let vx = i64::from(c.vx) * 60 / i64::from(PX);
        let flying = matches!(self.scene, Scene::Fly | Scene::Ready);
        let ok_down = c.vy <= SAFE_DOWN || !flying;
        let ok_side = c.vx.abs() <= SAFE_SIDE || !flying;
        let x = f.text(x + 28, 2, "V", QUIET) + 2;
        f.text(
            x,
            2,
            Num::new(vy).as_str(),
            if ok_down { GOOD } else { BAD },
        );
        let x = f.text(x + 14, 2, "H", QUIET) + 2;
        f.text(
            x,
            2,
            Num::new(vx).as_str(),
            if ok_side { GOOD } else { BAD },
        );
        // Craft left: small marks; then the score at the right edge.
        let score = Num::new(i64::from(self.score));
        let sx = 158 - text_width(score.as_str()) as i16;
        f.text(sx, 2, score.as_str(), TEXT);
        for i in 0..i16::from(self.lives.min(5)) {
            let lx = sx - 6 - i * 5;
            f.rect(lx, 2, 3, 3, HULL);
            f.pixel(lx, 5, LEG);
            f.pixel(lx + 2, 5, LEG);
        }
        let lv = Num::new(i64::from(self.level));
        let lx = sx - 8 - 5 * i16::from(self.lives.min(5)) - text_width(lv.as_str()) as i16 - 6;
        let after = f.text(lx, 2, "L", QUIET);
        f.text(after + 1, 2, lv.as_str(), TEXT);
    }

    /// Wind, under the bar, when there is some. Drawn before the craft,
    /// so the craft passes in front of it.
    fn wind_label(&self, f: &mut Frame) {
        let wind = self.world.params.wind_at(self.flight_t);
        if wind != 0 && self.pause.is_none() {
            let n = (wind.abs() / 8 + 1).min(4) as usize;
            let arrows = if wind > 0 { ">>>>" } else { "<<<<" };
            let x = f.text(118, HUD_H as i16 + 2, "WIND ", QUIET);
            f.text(x, HUD_H as i16 + 2, &arrows[..n], QUIET);
        }
    }

    // ---- Cards in the middle ------------------------------------------------

    fn card_ready(&self, f: &mut Frame) {
        let lv = Num::new(i64::from(self.level));
        let mut buf = [0u8; 16];
        let title = join(&mut buf, "LEVEL ", lv.as_str());
        panel(f, 30, 30, 100, 34);
        f.text_scaled(centre(title, 2), 35, title, GOLD, 2);
        let hint = if self.level == 1 {
            "LAND SLOWLY ON A PAD"
        } else {
            "GET READY"
        };
        f.text(centre(hint, 1), 52, hint, QUIET);
    }

    fn card_landed(&self, f: &mut Frame) {
        let t = &self.tally;
        panel(f, 24, 24, 112, 60);
        f.text_scaled(centre("LANDED", 2), 29, "LANDED", GOOD, 2);
        let mut buf = [0u8; 24];
        let fuel = Num::new(i64::from(t.fuel));
        let line = join(&mut buf, "FUEL LEFT ", fuel.as_str());
        f.text(32, 44, line, TEXT);
        f.text(32 + text_width(line) as i16, 44, "%", TEXT);
        if t.soft {
            f.text(32, 51, "SOFT TOUCH +25", TEXT);
        }
        let m = Num::new(i64::from(t.mult));
        let mut buf2 = [0u8; 16];
        f.text(32, 58, join(&mut buf2, "PAD X", m.as_str()), PAD);
        let p = Num::new(i64::from(t.points));
        let mut buf3 = [0u8; 24];
        f.text(32, 65, join(&mut buf3, "POINTS ", p.as_str()), GOLD);
        if self.scene_t >= crate::app::LANDED_WAIT {
            f.text(centre("A: NEXT LEVEL", 1), 75, "A: NEXT LEVEL", QUIET);
        }
    }

    fn card_broken(&self, f: &mut Frame) {
        if self.scene_t < 20 {
            return; // the burst first
        }
        let (big, why) = match self.ending {
            Ending::Crash(w) => ("CRASHED", w.text()),
            Ending::Lost => ("LOST", "FLEW OUT OF RANGE"),
        };
        panel(f, 24, 28, 112, 46);
        f.text_scaled(centre(big, 2), 33, big, BAD, 2);
        f.text(centre(why, 1), 48, why, TEXT);
        let n = Num::new(i64::from(self.lives));
        let mut buf = [0u8; 20];
        let left = join(&mut buf, "CRAFT LEFT ", n.as_str());
        f.text(centre(left, 1), 56, left, QUIET);
        if self.scene_t >= crate::app::BROKEN_WAIT && self.lives > 0 {
            f.text(centre("A: TRY AGAIN", 1), 65, "A: TRY AGAIN", QUIET);
        }
    }

    fn card_over(&self, f: &mut Frame) {
        panel(f, 24, 28, 112, 50);
        f.text_scaled(centre("GAME OVER", 2), 33, "GAME OVER", WARN, 2);
        let s = Num::new(i64::from(self.score));
        let b = Num::new(i64::from(self.best));
        let mut buf = [0u8; 20];
        let mut buf2 = [0u8; 20];
        let line = join(&mut buf, "SCORE ", s.as_str());
        f.text(centre(line, 1), 49, line, TEXT);
        let line = join(&mut buf2, "BEST ", b.as_str());
        f.text(centre(line, 1), 57, line, GOLD);
        if self.scene_t >= crate::app::BROKEN_WAIT / 3 {
            f.text(centre("A: TITLE", 1), 67, "A: TITLE", QUIET);
        }
    }

    // ---- The title --------------------------------------------------------

    fn title(&self, f: &mut Frame) {
        f.text_scaled(8, 8, "LANDER", GOLD, 3);
        f.text(9, 26, "SET IT DOWN GENTLY", QUIET);
        panel(
            f,
            MENU_X - 6,
            MENU_Y - 4,
            86,
            MENU_STEP as u16 * TITLE_LINES as u16 + 5,
        );
        let lv = Num::new(i64::from(self.opts.level));
        let steer = match (self.opts.steer, self.has_motion) {
            (Steer::Stick, _) => "STICK",
            (Steer::Tilt, true) => "TILT",
            (Steer::Tilt, false) => "TILT?",
        };
        let lines: [(&str, &str); TITLE_LINES] = [
            ("PLAY", ""),
            ("LEVEL", lv.as_str()),
            (
                "VIEW",
                if self.opts.view == View::Still {
                    "STILL"
                } else {
                    "FOLLOW"
                },
            ),
            ("STEER", steer),
            ("RATE", if self.opts.fast { "60 FPS" } else { "30 FPS" }),
            ("END", ""),
        ];
        for (i, (name, value)) in lines.iter().enumerate() {
            let y = MENU_Y + i as i16 * MENU_STEP;
            let on = usize::from(self.menu) == i;
            let c = if on { TEXT } else { QUIET };
            if on {
                f.text(MENU_X - 4, y, ">", GOLD);
            }
            f.text(MENU_X, y, name, c);
            f.text(MENU_X + 28, y, value, if on { GOLD } else { QUIET });
        }
        let help = match (self.menu, self.opts.steer, self.has_motion) {
            (0, _, _) => "A OR START: PLAY",
            (1, _, _) => "LEFT/RIGHT: START LEVEL",
            (2, _, _) if self.opts.view == View::Follow => "SCROLLS AND ZOOMS: HEAVY",
            (2, _, _) => "CAMERA STANDS STILL: LIGHT",
            (3, Steer::Tilt, false) => "NO MOTION FROM THE PAD",
            (3, Steer::Tilt, true) => "TILT THE PAD TO LEAN",
            (3, _, _) => "LEAN WITH STICK OR D-PAD",
            (4, _, _) => "60 FPS: TWICE THE SENDS",
            _ => "BACK TO THE HOME SCREEN",
        };
        f.rect(0, 111, 160, 9, HUD);
        f.text(centre(help, 1), 113, help, QUIET);
        if self.best > 0 {
            let b = Num::new(i64::from(self.best));
            let mut buf = [0u8; 20];
            let line = join(&mut buf, "BEST ", b.as_str());
            f.text(156 - text_width(line) as i16, 2, line, GOLD);
        }
    }
}

/// The game's own pause menu.
fn pause(f: &mut Frame, sel: u8) {
    panel(f, PAUSE_X, PAUSE_Y, PAUSE_W, 48);
    f.text_scaled(centre("PAUSED", 2), PAUSE_Y + 5, "PAUSED", TEXT, 2);
    for (i, name) in ["RESUME", "RESTART LEVEL", "TITLE"].iter().enumerate() {
        let y = PAUSE_FIRST + i as i16 * MENU_STEP;
        let on = usize::from(sel) == i;
        if on {
            f.text(PAUSE_X + 5, y, ">", GOLD);
        }
        f.text(PAUSE_X + 10, y, name, if on { TEXT } else { QUIET });
    }
}

/// The craft with its middle at picture pixel `(sx, sy)`, leaning
/// `step` (1/256 turns), each craft pixel `z` x `z` picture pixels, with
/// a flame for engine power `burn` (0..=FULL). The flame flickers by
/// `clock`. Work: at most (14z + 1)^2 look-ups (841 at z = 2).
pub fn craft_at(f: &mut Frame, sx: i32, sy: i32, step: i32, z: i32, burn: i32, clock: u32) {
    let z = z.clamp(1, 4);
    // The flame first, under the craft.
    if burn > 0 {
        let mut rng = Rng::new(clock.wrapping_mul(2_654_435_761) | 1);
        let len = (burn.min(FULL) * FLAME_LEN / FULL).max(1) + rng.spread(1);
        for t in 1..=len.max(1) {
            // Three pixels wide for the first half, then one: a hot
            // middle by the nozzle, orange, a red tip.
            let wide = if t * 2 <= len + 1 { 1 } else { 0 };
            for k in -wide..=wide {
                let c = if k == 0 && t <= 2 {
                    HOT
                } else if t <= len * 2 / 3 {
                    FLAME
                } else {
                    TIP
                };
                let (dx, dy) = turn(k * z, (NOZZLE_Y + t) * z, step);
                plot(f, sx + round14(dx), sy + round14(dy), z, c);
            }
        }
    }
    // The hull: each picture pixel near the craft looks up which craft
    // pixel lands on it (turned back), so no gaps appear.
    let r = 7 * z;
    let unit = z << 14;
    for dy in -r..=r {
        for dx in -r..=r {
            let (u, v) = turn(dx, dy, -step);
            // Q14 to craft pixels, rounded, at this zoom.
            let (u, v) = (
                (u + unit / 2).div_euclid(unit),
                (v + unit / 2).div_euclid(unit),
            );
            if (-4..=4).contains(&u) && (-4..=4).contains(&v) {
                let c = CRAFT[((v + 4) * 9 + u + 4) as usize];
                if c != O {
                    put(f, sx + dx, sy + dy, c);
                }
            }
        }
    }
}

/// `a` then `b` in `buf` (cut if too long).
fn join<'a>(buf: &'a mut [u8], a: &str, b: &str) -> &'a str {
    let mut n = 0;
    for &c in a.as_bytes().iter().chain(b.as_bytes()) {
        if n < buf.len() {
            buf[n] = c;
            n += 1;
        }
    }
    core::str::from_utf8(&buf[..n]).unwrap_or("?")
}

/// Q14 to whole pixels, rounded.
fn round14(v: i32) -> i32 {
    (v + (1 << 13)) >> 14
}

/// One pixel, if inside the picture.
fn put(f: &mut Frame, x: i32, y: i32, c: u8) {
    if (0..160).contains(&x) && (0..120).contains(&y) {
        f.0[y as usize * 160 + x as usize] = c;
    }
}

/// One world pixel at this zoom (a square of `z` x `z`).
fn plot(f: &mut Frame, x: i32, y: i32, z: i32, c: u8) {
    if z == 1 {
        put(f, x, y, c);
    } else {
        f.rect(
            x.clamp(-100, 300) as i16,
            y.clamp(-100, 300) as i16,
            z as u16,
            z as u16,
            c,
        );
    }
}

/// Sky and ground: one pass over every pixel, column by column.
fn ground(f: &mut Frame, world: &World, cam: Cam) {
    let z = cam.zoom.max(1);
    let top = cam.y.div_euclid(PX);
    for sx in 0..160i32 {
        // The world column under this picture column.
        let wx = (cam.x + sx * PX / z).div_euclid(PX);
        let gy = world.ground_at(wx);
        let on_pad = world.pad_at(wx).is_some();
        // The surface's picture row: (gy - top) * z, minus the camera's
        // part-pixel.
        let sy0 = ((i64::from(gy) * i64::from(PX) - i64::from(cam.y)) * i64::from(z))
            .div_euclid(i64::from(PX))
            .clamp(0, 120) as i32;
        for sy in 0..120i32 {
            let c = if sy < sy0 {
                SKY
            } else {
                let depth = (sy - sy0) / z;
                let wy = top + sy / z;
                if depth == 0 {
                    if on_pad { PAD } else { SURFACE }
                } else if depth == 1 && on_pad {
                    PAD_DARK
                } else if depth >= 2 && pit(wx, wy) {
                    PIT
                } else if depth < BAND1 {
                    ROCK1
                } else if depth < BAND2 {
                    ROCK2
                } else {
                    ROCK3
                }
            };
            f.0[sy as usize * 160 + sx as usize] = c;
        }
    }
}

/// The rock's dark dots: a fixed scatter, the same wherever it is seen.
fn pit(x: i32, y: i32) -> bool {
    let h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    (h ^ (h >> 15)).is_multiple_of(PIT_EVERY)
}

/// Stars: far away, so they move at half the camera's speed, and only
/// on the sky.
fn stars(f: &mut Frame, cam: Cam) {
    let mut rng = Rng::new(0x5EED_57A2);
    let shift = cam.x.div_euclid(PX) / 2;
    for i in 0..STARS {
        let x = (rng.below(320) as i32 - shift).rem_euclid(320);
        let y = 10 + rng.below(70) as i32;
        if x < 160 {
            let at = y as usize * 160 + x as usize;
            if f.0[at] == SKY {
                f.0[at] = if i % 3 == 0 { STAR } else { STAR_DIM };
            }
        }
    }
}

/// "X2" under each pad.
fn pad_labels(f: &mut Frame, world: &World, cam: Cam) {
    let z = cam.zoom;
    for p in world.pads() {
        let m = Num::new(i64::from(p.mult));
        let mut buf = [0u8; 8];
        let label = join(&mut buf, "X", m.as_str());
        let mid = i32::from(p.x) * PX + i32::from(p.w) * PX / 2;
        let x =
            ((i64::from(mid) - i64::from(cam.x)) * i64::from(z)).div_euclid(i64::from(PX)) as i32;
        let y = ((i64::from(p.y) * i64::from(PX) + 3 * i64::from(PX) - i64::from(cam.y))
            * i64::from(z))
        .div_euclid(i64::from(PX)) as i32;
        let w = i32::from(text_width(label)) * z;
        f.text_scaled(
            (x - w / 2).clamp(-200, 400) as i16,
            y.clamp(-200, 400) as i16,
            label,
            PAD,
            z as u8,
        );
    }
}
