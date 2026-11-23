//! The palette: the game's own colours, set at start.

use game_kit::rgb565;

use crate::poly::{MATS, SHADES};

/// The text band's background (and its second colour, for the send
/// level that changes every row).
pub const BAND: u8 = 1;
pub const BAND2: u8 = 2;
pub const TEXT: u8 = 3;
pub const DIM: u8 = 4;
pub const GOOD: u8 = 5;
pub const BAD: u8 = 6;
/// The work area's background.
pub const SKY: u8 = 7;
/// The four colours the fill level cycles through.
pub const FILL0: u8 = 8;
/// The eight triangle colours.
pub const TRI0: u8 = 12;
/// The two colours of the send levels.
pub const SEND_A: u8 = 20;
pub const SEND_B: u8 = 21;
/// The scene colours: `SCENE0 + shade * MATS + material`.
pub const SCENE0: u8 = 22;
/// The ball scenes' last colour + 1 (the palette before the city).
pub const SCENE_END: usize = SCENE0 as usize + (MATS as usize) * (SHADES as usize);
/// The city's materials: `CITY0 + material * CITY_SHADES + shade`
/// (materials in [`crate::city`]).
pub const CITY0: u8 = SCENE_END as u8;
/// Shades of a city material: 0 the brightest, each 8 % darker.
pub const CITY_SHADES: u8 = SHADES - 1;
/// The city's materials at full light, in the order of
/// [`crate::city`]'s material numbers.
const CITY_RGB: [(u8, u8, u8); CITY_MATS] = [
    (78, 80, 88),    // asphalt
    (236, 196, 64),  // yellow line
    (232, 232, 226), // white line
    (226, 208, 170), // walls: cream
    (184, 86, 62),   //   brick
    (156, 158, 164), //   concrete
    (214, 176, 122), //   sand
    (206, 118, 84),  //   terracotta
    (150, 184, 212), //   pale blue
    (240, 240, 234), //   white
    (98, 108, 124),  //   slate
    (34, 52, 86),    // glass: dark blue
    (44, 92, 108),   //   teal
    (214, 40, 40),   // cars: red
    (44, 86, 206),   //   blue
    (244, 204, 40),  //   yellow
    (244, 244, 244), //   white
    (40, 156, 72),   //   green
    (38, 38, 44),    //   black
];
/// City materials.
pub const CITY_MATS: usize = 19;
/// The city's flat colours: three sky bands (top to horizon), the ground.
pub const SKY_HI: u8 = CITY0 + (CITY_MATS as u8) * CITY_SHADES;
pub const SKY_MID: u8 = SKY_HI + 1;
pub const SKY_LOW: u8 = SKY_HI + 2;
pub const PAVE: u8 = SKY_HI + 3;
const CITY_FLAT: [(u8, u8, u8); 4] = [
    (84, 140, 214),  // SKY_HI
    (136, 180, 228), // SKY_MID
    (190, 212, 234), // SKY_LOW
    (150, 146, 136), // PAVE
];
/// The island's own materials ([`crate::island`]), four shades each:
/// `ISLE0 + material * ISLE_SHADES + shade`. The island also uses city
/// colours (stone, roofs, rock, the figure's trousers and pack).
pub const ISLE0: u8 = PAVE + 1;
/// Shades of an island material: 0 the brightest, each 15 % darker.
pub const ISLE_SHADES: u8 = 4;
/// The island's materials at full light, in the order of
/// [`crate::island`]'s `ISLE_*` numbers.
const ISLE_RGB: [(u8, u8, u8); ISLE_MATS] = [
    (110, 200, 70),  // grass
    (80, 165, 55),   // grass, the second tile colour
    (160, 105, 60),  // dirt
    (120, 75, 40),   // trunks, fences
    (30, 125, 95),   // leaves, bushes
    (110, 90, 220),  // platform sides
    (170, 160, 255), // platform tops
    (90, 250, 250),  // gems
    (245, 205, 165), // skin
    (250, 130, 40),  // the figure's shirt
];
/// Island materials.
pub const ISLE_MATS: usize = 10;
/// The island's flat colours: the shadow, the haze below the horizon.
pub const SHADOW: u8 = ISLE0 + (ISLE_MATS as u8) * ISLE_SHADES;
pub const HAZE: u8 = SHADOW + 1;
const ISLE_FLAT: [(u8, u8, u8); 2] = [
    (30, 60, 25),    // SHADOW
    (215, 225, 240), // HAZE
];
/// Colours set (palette numbers 0 to COLOURS - 1).
pub const COLOURS: usize = HAZE as usize + 1;

const BASE: [u16; SCENE0 as usize] = [
    rgb565(0, 0, 0),       // 0: unused
    rgb565(20, 26, 40),    // BAND
    rgb565(48, 20, 20),    // BAND2
    rgb565(236, 238, 244), // TEXT
    rgb565(130, 138, 160), // DIM
    rgb565(110, 220, 120), // GOOD
    rgb565(240, 90, 80),   // BAD
    rgb565(10, 14, 28),    // SKY
    rgb565(60, 70, 110),   // FILL0
    rgb565(70, 110, 70),
    rgb565(110, 70, 60),
    rgb565(90, 90, 90),
    rgb565(230, 90, 70), // TRI0
    rgb565(240, 170, 60),
    rgb565(230, 220, 90),
    rgb565(110, 210, 110),
    rgb565(80, 200, 210),
    rgb565(90, 130, 240),
    rgb565(170, 110, 230),
    rgb565(230, 120, 190),
    rgb565(250, 250, 250), // SEND_A
    rgb565(30, 30, 30),    // SEND_B
];

/// The four materials at full light.
const MAT_RGB: [(u8, u8, u8); MATS as usize] = [
    (230, 100, 70),
    (90, 170, 235),
    (240, 205, 90),
    (120, 215, 125),
];

const fn palette() -> [u16; COLOURS] {
    let mut p = [0u16; COLOURS];
    let mut i = 0;
    while i < SCENE0 as usize {
        p[i] = BASE[i];
        i += 1;
    }
    let mut s = 0;
    while s < SHADES as usize {
        let mut m = 0;
        while m < MATS as usize {
            let (r, g, b) = MAT_RGB[m];
            // Shade 0 full light, each step a tenth darker.
            let k = 10 - s as u32;
            p[SCENE0 as usize + s * MATS as usize + m] = rgb565(
                (r as u32 * k / 10) as u8,
                (g as u32 * k / 10) as u8,
                (b as u32 * k / 10) as u8,
            );
            m += 1;
        }
        s += 1;
    }
    let mut m = 0;
    while m < CITY_MATS {
        let (r, g, b) = CITY_RGB[m];
        let mut s = 0;
        while s < CITY_SHADES as usize {
            let k = 100 - 8 * s as u32;
            p[CITY0 as usize + m * CITY_SHADES as usize + s] = rgb565(
                (r as u32 * k / 100) as u8,
                (g as u32 * k / 100) as u8,
                (b as u32 * k / 100) as u8,
            );
            s += 1;
        }
        m += 1;
    }
    let mut i = 0;
    while i < CITY_FLAT.len() {
        let (r, g, b) = CITY_FLAT[i];
        p[SKY_HI as usize + i] = rgb565(r, g, b);
        i += 1;
    }
    let mut m = 0;
    while m < ISLE_MATS {
        let (r, g, b) = ISLE_RGB[m];
        let mut s = 0;
        while s < ISLE_SHADES as usize {
            let k = 100 - 15 * s as u32;
            p[ISLE0 as usize + m * ISLE_SHADES as usize + s] = rgb565(
                (r as u32 * k / 100) as u8,
                (g as u32 * k / 100) as u8,
                (b as u32 * k / 100) as u8,
            );
            s += 1;
        }
        m += 1;
    }
    let mut i = 0;
    while i < ISLE_FLAT.len() {
        let (r, g, b) = ISLE_FLAT[i];
        p[SHADOW as usize + i] = rgb565(r, g, b);
        i += 1;
    }
    p
}

/// Palette numbers 0 to [`COLOURS`] - 1.
pub static PALETTE: [u16; COLOURS] = palette();
