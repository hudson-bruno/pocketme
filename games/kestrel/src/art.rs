//! The palette and the fixed backdrop (stars and a ringed planet).
//!
//! **Shading through the palette.** Every material is in the palette at
//! [`SHADES`] brightness steps: palette number = shade x [`MATS`] +
//! material (see [`crate::poly::Shading`]). Shade 0 is the brightest.
//! The palette is set once, so it never forces a full-screen send (a
//! palette change does on this console).
//!
//! **The backdrop does not move.** The camera only slides; it never
//! turns. Stars and the planet are "at infinity", so they stay on the
//! same pixels whatever the craft does, and the console sends nothing
//! for them. Only the places where objects are, or were, cost a send.

use game_kit::{Frame, W, rgb565};

use crate::fixed::hash;
use crate::poly::Rect;

/// Materials.
pub mod mat {
    /// Space: palette number 0 is black.
    pub const BLACK: u8 = 0;
    pub const HULL: u8 = 1;
    pub const HULL2: u8 = 2;
    pub const CANOPY: u8 = 3;
    pub const WING: u8 = 4;
    pub const WING2: u8 = 5;
    /// The engine's glow (shines by itself).
    pub const ENGINE: u8 = 6;
    pub const ROCK1: u8 = 7;
    pub const ROCK2: u8 = 8;
    pub const ICE: u8 = 9;
    pub const RUST: u8 = 10;
    pub const METAL: u8 = 11;
    pub const POD: u8 = 12;
    /// The gate model's lights; swapped for one of the three below.
    pub const GATE_LIGHT: u8 = 13;
    /// The next gate: bright cyan.
    pub const GATE_NEXT: u8 = 14;
    /// A later gate: dim blue.
    pub const GATE_LATER: u8 = 15;
    /// A gate flown through: dull green. A missed one: dull red.
    pub const GATE_DONE: u8 = 16;
    pub const GATE_MISSED: u8 = 17;
    pub const CELL: u8 = 18;
    /// The dock gate and the station's face: gold.
    pub const DOCK: u8 = 19;
    pub const STATION: u8 = 20;
    /// Red warning lights.
    pub const WARN: u8 = 21;
}

/// Materials.
pub const MATS: u8 = 22;
/// Brightness steps.
pub const SHADES: u8 = 8;

/// Materials that shine by themselves (no light, still fog).
pub const GLOW: u32 = (1 << mat::ENGINE)
    | (1 << mat::GATE_NEXT)
    | (1 << mat::GATE_LATER)
    | (1 << mat::GATE_DONE)
    | (1 << mat::GATE_MISSED)
    | (1 << mat::CELL)
    | (1 << mat::DOCK)
    | (1 << mat::WARN);

/// Each material in full light, (red, green, blue) 0 to 255.
const MAT_RGB: [(u32, u32, u32); MATS as usize] = [
    (0, 0, 0),       // BLACK
    (196, 206, 218), // HULL
    (112, 124, 146), // HULL2
    (70, 130, 196),  // CANOPY
    (236, 128, 44),  // WING
    (178, 84, 30),   // WING2
    (140, 230, 255), // ENGINE
    (150, 128, 104), // ROCK1
    (112, 108, 116), // ROCK2
    (176, 204, 218), // ICE
    (156, 74, 46),   // RUST
    (108, 112, 102), // METAL
    (232, 192, 56),  // POD
    (255, 0, 255),   // GATE_LIGHT (never drawn as is)
    (70, 246, 232),  // GATE_NEXT
    (52, 88, 150),   // GATE_LATER
    (70, 130, 80),   // GATE_DONE
    (140, 56, 56),   // GATE_MISSED
    (130, 255, 120), // CELL
    (255, 200, 70),  // DOCK
    (214, 208, 192), // STATION
    (255, 70, 56),   // WARN
];

/// Brightness of each shade step, in 1/256.
const SHADE_K: [u32; SHADES as usize] = [256, 214, 178, 148, 120, 94, 68, 44];

/// First palette number after the shaded materials.
const UI: u8 = MATS * SHADES;
pub const STAR_DIM: u8 = UI;
pub const STAR: u8 = UI + 1;
pub const STAR_BRIGHT: u8 = UI + 2;
pub const PLANET_DARK: u8 = UI + 3;
pub const PLANET_1: u8 = UI + 4;
pub const PLANET_2: u8 = UI + 5;
pub const PLANET_3: u8 = UI + 6;
pub const PLANET_RING: u8 = UI + 7;
pub const UI_BG: u8 = UI + 8;
pub const UI_EDGE: u8 = UI + 9;
pub const TEXT: u8 = UI + 10;
pub const TEXT_DIM: u8 = UI + 11;
pub const ACCENT: u8 = UI + 12;
pub const GOOD: u8 = UI + 13;
pub const BAD: u8 = UI + 14;
pub const BOX_ON: u8 = UI + 15;
pub const FRAME: u8 = UI + 16;
pub const FRAME_LIGHT: u8 = UI + 17;
/// Palette numbers the game sets.
pub const COLOURS_LEN: usize = UI as usize + 18;

const UI_RGB: [(u8, u8, u8); 18] = [
    (70, 74, 96),    // STAR_DIM
    (150, 156, 180), // STAR
    (236, 240, 255), // STAR_BRIGHT
    (40, 22, 40),    // PLANET_DARK
    (120, 62, 82),   // PLANET_1
    (168, 94, 96),   // PLANET_2
    (206, 148, 118), // PLANET_3
    (92, 84, 110),   // PLANET_RING
    (14, 18, 34),    // UI_BG
    (52, 66, 104),   // UI_EDGE
    (228, 234, 244), // TEXT
    (118, 128, 156), // TEXT_DIM
    (255, 196, 70),  // ACCENT
    (110, 236, 130), // GOOD
    (255, 92, 70),   // BAD
    (46, 84, 140),   // BOX_ON
    (24, 30, 50),    // FRAME
    (60, 74, 112),   // FRAME_LIGHT
];

/// The palette, made while compiling.
pub static COLOURS: [u16; COLOURS_LEN] = palette();

const fn palette() -> [u16; COLOURS_LEN] {
    let mut p = [0u16; COLOURS_LEN];
    let mut s = 0;
    while s < SHADES as usize {
        let mut m = 0;
        while m < MATS as usize {
            let (r, g, b) = MAT_RGB[m];
            let k = SHADE_K[s];
            p[s * MATS as usize + m] = rgb565(
                (r * k / 256) as u8,
                (g * k / 256) as u8,
                (b * k / 256) as u8,
            );
            m += 1;
        }
        s += 1;
    }
    let mut i = 0;
    while i < UI_RGB.len() {
        let (r, g, b) = UI_RGB[i];
        p[UI as usize + i] = rgb565(r, g, b);
        i += 1;
    }
    p
}

/// Stars: x, y (0..160, 0..120) and colour, the same every time.
const STARS: usize = 70;
static STAR_AT: [(u8, u8, u8); STARS] = stars();

const fn stars() -> [(u8, u8, u8); STARS] {
    let mut s = [(0u8, 0u8, 0u8); STARS];
    let mut i = 0;
    while i < STARS {
        let h = hash(0x5747, i as u32);
        let c = match h >> 29 {
            0 => STAR_BRIGHT,
            1..=3 => STAR,
            _ => STAR_DIM,
        };
        s[i] = ((h % 160) as u8, ((h >> 8) % 120) as u8, c);
        i += 1;
    }
    s
}

/// The planet: middle and radius in pixels (seen in the full view).
const PLANET: (i32, i32, i32) = (128, 30, 22);

/// Draws the backdrop inside `area`: black, the stars, the planet with
/// its bands and ring. The same pixels every frame for the same area.
pub fn backdrop(f: &mut Frame, area: &Rect) {
    let a = area.on_screen();
    for y in a.y0..a.y1 {
        let at = y as usize * W;
        f.0[at + a.x0 as usize..at + a.x1 as usize].fill(0);
    }
    for &(x, y, c) in STAR_AT.iter() {
        let (x, y) = (i32::from(x), i32::from(y));
        if x >= a.x0 && x < a.x1 && y >= a.y0 && y < a.y1 {
            f.0[y as usize * W + x as usize] = c;
        }
    }
    // The planet: bands by height, a shadow on its lower left, and a
    // thin ring in front (an ellipse 3.2 times as wide as high).
    let (px, py, r) = PLANET;
    let inside = |x: i32, y: i32| x >= a.x0 && x < a.x1 && y >= a.y0 && y < a.y1;
    for y in (py - r)..=(py + r) {
        for x in (px - r)..=(px + r) {
            let (dx, dy) = (x - px, y - py);
            let d2 = dx * dx + dy * dy;
            if d2 > r * r || !inside(x, y) {
                continue;
            }
            // Light from the upper right: dark where -dx + dy is big.
            let shade = (dy - dx) * 16 / r + (d2 * 6 / (r * r));
            let band = ((dy + r) * 7 / (2 * r)) % 3;
            let c = if shade > 18 {
                PLANET_DARK
            } else if shade > 9 {
                if band == 0 { PLANET_DARK } else { PLANET_1 }
            } else {
                match band {
                    0 => PLANET_1,
                    1 => PLANET_2,
                    _ => PLANET_3,
                }
            };
            f.0[y as usize * W + x as usize] = c;
        }
    }
    // The ring: points of an ellipse, the part behind the planet hidden.
    let (rx, ry) = (r * 16 / 10, r * 5 / 10);
    for y in (py - ry - 1)..=(py + ry + 1) {
        for x in (px - rx - 1)..=(px + rx + 1) {
            let (dx, dy) = (x - px, y - py);
            // On the ellipse when (dx/rx)^2 + (dy/ry)^2 is near 1.
            let e = dx * dx * ry * ry + dy * dy * rx * rx;
            let one = rx * rx * ry * ry;
            if (e - one).abs() * 10 > one * 3 || !inside(x, y) {
                continue;
            }
            let behind = dy < 0 && dx * dx + dy * dy <= r * r;
            if !behind {
                f.0[y as usize * W + x as usize] = PLANET_RING;
            }
        }
    }
}
