//! The palette: shaded materials for the 3D, and flat colours for the
//! rest (stars, panel, text, chart, fire).
//!
//! **Shading through the palette.** Every material is in the palette at
//! [`SHADES`] brightness steps: palette number = shade x [`MATS`] +
//! material (see [`crate::poly::Shading`]). Shade 0 is the brightest.
//!
//! **Each system looks different.** The planet's three materials and the
//! sun's two are set from the system's `look` and `star` ([`palette`]).
//! The palette is only changed on a jump or a new game, never in
//! flight, because a palette change makes the console send the whole
//! picture.

use game_kit::rgb565;

/// Materials.
pub mod mat {
    pub const BLACK: u8 = 0;
    /// The player's ship.
    pub const HULL: u8 = 1;
    pub const HULL2: u8 = 2;
    pub const GLASS: u8 = 3;
    /// Engine glow (shines by itself).
    pub const ENGINE: u8 = 4;
    pub const RAIDER: u8 = 5;
    pub const RAIDER2: u8 = 6;
    pub const HAULER: u8 = 7;
    pub const HAULER2: u8 = 8;
    pub const WARDEN: u8 = 9;
    pub const WARDEN2: u8 = 10;
    pub const STATION: u8 = 11;
    pub const STATION2: u8 = 12;
    /// The docking mouth: dark inside.
    pub const MOUTH: u8 = 13;
    /// Green and red lights (shine by themselves).
    pub const LIGHT_G: u8 = 14;
    pub const LIGHT_R: u8 = 15;
    /// The planet's three colours and the sun's two: set per system.
    pub const PLANET_A: u8 = 16;
    pub const PLANET_B: u8 = 17;
    pub const PLANET_C: u8 = 18;
    pub const SUN: u8 = 19;
    pub const SUN2: u8 = 20;
    pub const CANISTER: u8 = 21;
    /// Solar panels.
    pub const PANEL: u8 = 22;
    pub const GUNMETAL: u8 = 23;
    /// A ship's main colour while a shot of the player's lands (shines).
    pub const FLASH: u8 = 24;
}

/// Materials.
pub const MATS: u8 = 25;
/// Brightness steps.
pub const SHADES: u8 = 8;

/// Materials that shine by themselves.
pub const GLOW: u32 = (1 << mat::ENGINE)
    | (1 << mat::LIGHT_G)
    | (1 << mat::LIGHT_R)
    | (1 << mat::SUN)
    | (1 << mat::SUN2)
    | (1 << mat::FLASH);

/// Each material in full light, (red, green, blue).
const MAT_RGB: [(u8, u8, u8); MATS as usize] = [
    (0, 0, 0),       // BLACK
    (200, 206, 214), // HULL
    (96, 110, 132),  // HULL2
    (84, 170, 230),  // GLASS
    (120, 220, 255), // ENGINE
    (176, 52, 48),   // RAIDER
    (150, 60, 64),   // RAIDER2
    (222, 176, 70),  // HAULER
    (128, 96, 60),   // HAULER2
    (226, 232, 240), // WARDEN
    (50, 96, 200),   // WARDEN2
    (190, 194, 200), // STATION
    (120, 126, 140), // STATION2
    (24, 26, 40),    // MOUTH
    (90, 255, 120),  // LIGHT_G
    (255, 70, 60),   // LIGHT_R
    (0, 0, 0),       // PLANET_A (per system)
    (0, 0, 0),       // PLANET_B
    (0, 0, 0),       // PLANET_C
    (0, 0, 0),       // SUN (per system)
    (0, 0, 0),       // SUN2
    (236, 150, 40),  // CANISTER
    (40, 60, 120),   // PANEL
    (80, 84, 92),    // GUNMETAL
    // Mint like the player's own bolt, so a hit ship never looks like
    // the white warden.
    (170, 255, 200), // FLASH
];

/// The planet looks: three colours each (A, B, C).
pub const LOOKS: [[(u8, u8, u8); 3]; 8] = [
    [(40, 86, 176), (72, 146, 76), (226, 234, 240)], // sea and land
    [(206, 164, 102), (150, 100, 60), (236, 214, 174)], // desert
    [(170, 204, 228), (232, 240, 248), (118, 140, 176)], // ice
    [(64, 36, 34), (230, 96, 30), (128, 44, 32)],    // lava
    [(214, 172, 122), (170, 112, 72), (238, 218, 182)], // banded giant
    [(50, 132, 64), (30, 92, 52), (204, 222, 204)],  // jungle
    [(132, 92, 164), (90, 60, 124), (196, 164, 214)], // violet haze
    [(184, 92, 60), (130, 60, 46), (224, 202, 190)], // rust
];
pub const LOOK_NAMES: [&str; 8] = [
    "OCEAN", "DESERT", "ICE", "LAVA", "GIANT", "JUNGLE", "HAZE", "RUST",
];

/// The sun colours: bright middle and rim.
pub const STARS: [[(u8, u8, u8); 2]; 5] = [
    [(255, 236, 150), (255, 190, 80)],  // yellow
    [(244, 248, 255), (196, 214, 255)], // white
    [(255, 176, 90), (232, 112, 40)],   // orange
    [(255, 120, 86), (200, 60, 40)],    // red
    [(178, 206, 255), (120, 150, 255)], // blue
];

/// The nebula tints (full strength; drawn at 15 to 40 %).
pub const NEBULAS: [(u8, u8, u8); 6] = [
    (40, 150, 160), // teal
    (140, 70, 190), // violet
    (190, 60, 100), // rose
    (200, 120, 40), // amber
    (50, 90, 200),  // blue
    (60, 170, 100), // green
];

/// Each planet look's air, seen as a rim against space (step G1: and
/// the sky at the horizon below, `planet::palette`).
pub const AIRS: [(u8, u8, u8); 8] = [
    (110, 170, 255), // sea and land: blue
    (236, 190, 130), // desert: dust
    (200, 230, 255), // ice: pale
    (255, 110, 50),  // lava: fire
    (240, 200, 150), // giant: haze
    (140, 230, 150), // jungle: green
    (200, 150, 255), // violet haze
    (240, 150, 110), // rust
];

/// Brightness of each shade step, in 1/256.
const SHADE_K: [u32; SHADES as usize] = [256, 216, 180, 148, 118, 90, 64, 40];

/// First palette number after the shaded materials.
const UI: u8 = MATS * SHADES;
pub const STAR_DIM: u8 = UI;
pub const STAR: u8 = UI + 1;
pub const STAR_BRIGHT: u8 = UI + 2;
pub const DUST: u8 = UI + 3;
pub const UI_BG: u8 = UI + 4;
pub const UI_EDGE: u8 = UI + 5;
pub const TEXT: u8 = UI + 6;
pub const TEXT_DIM: u8 = UI + 7;
pub const ACCENT: u8 = UI + 8;
pub const GOOD: u8 = UI + 9;
pub const BAD: u8 = UI + 10;
pub const WARN: u8 = UI + 11;
pub const PANEL_BG: u8 = UI + 12;
pub const RADAR_BG: u8 = UI + 13;
pub const RADAR_LINE: u8 = UI + 14;
pub const BLIP_RAIDER: u8 = UI + 15;
pub const BLIP_HAULER: u8 = UI + 16;
pub const BLIP_WARDEN: u8 = UI + 17;
pub const BLIP_STATION: u8 = UI + 18;
pub const BLIP_CAN: u8 = UI + 19;
pub const BEAM_ME: u8 = UI + 20;
pub const BEAM_FOE: u8 = UI + 21;
pub const FIRE1: u8 = UI + 22;
pub const FIRE2: u8 = UI + 23;
pub const FIRE3: u8 = UI + 24;
pub const SMOKE: u8 = UI + 25;
pub const ECON_FARM: u8 = UI + 26;
pub const ECON_MINE: u8 = UI + 27;
pub const ECON_FORGE: u8 = UI + 28;
pub const ECON_LAB: u8 = UI + 29;
pub const ECON_HAVEN: u8 = UI + 30;
pub const MAP_RANGE: u8 = UI + 31;
pub const BAR_BG: u8 = UI + 32;
pub const SHIELD: u8 = UI + 33;
pub const HEAT: u8 = UI + 34;
pub const FUEL: u8 = UI + 35;
pub const GRID: u8 = UI + 36;
pub const TUNNEL1: u8 = UI + 37;
pub const TUNNEL2: u8 = UI + 38;
/// The far sun and planet as a dot (per system, like their materials).
pub const FAR_SUN: u8 = UI + 39;
/// The colours below live in shades 1..7 of the sun's two materials:
/// those glow, so the renderer only ever draws their shade 0, and the
/// console lets a game set only palette numbers below 240 (UI + 40).
/// (FAR_PLANET and HALO were UI + 40 and UI + 41 before step L1: the
/// console ignored them and showed its own colours there.)
const fn spare(shade: u8, m: u8) -> u8 {
    shade * MATS + m
}
pub const FAR_PLANET: u8 = spare(1, mat::SUN);
pub const HALO: u8 = spare(2, mat::SUN);
/// The space nebula's three steps, dim to bright (per system: a tint
/// from [`NEBULAS`] picked by the system's number).
pub const NEB1: u8 = spare(3, mat::SUN);
pub const NEB2: u8 = spare(4, mat::SUN);
pub const NEB3: u8 = spare(5, mat::SUN);
/// The planet's atmosphere rim, outer (dim) and inner (per look).
pub const ATMOS1: u8 = spare(6, mat::SUN);
pub const ATMOS2: u8 = spare(7, mat::SUN);
/// The sun's corona, outer (dim) and inner (per star).
pub const CORONA1: u8 = spare(1, mat::SUN2);
pub const CORONA2: u8 = spare(2, mat::SUN2);
/// A raider's TAG on the view: its target line, bracket and bar, in one
/// colour at every distance (the laser passes through a TAG, so the
/// amber "laser reaches" says nothing there). Magenta: no amber, no
/// warning red or orange, not the station's green, not the dim grey;
/// in flight nothing else uses it (the chart's HAVEN systems do).
pub const TAG_MARK: u8 = ECON_HAVEN;
/// Palette numbers the game sets.
pub const COLOURS_LEN: usize = UI as usize + 40;

const UI_RGB: [(u8, u8, u8); 39] = [
    (70, 74, 100),   // STAR_DIM
    (150, 156, 184), // STAR
    (240, 242, 255), // STAR_BRIGHT
    (110, 116, 140), // DUST
    (12, 16, 30),    // UI_BG
    (54, 70, 112),   // UI_EDGE
    (228, 234, 244), // TEXT
    (120, 130, 158), // TEXT_DIM
    (255, 196, 70),  // ACCENT
    (110, 236, 130), // GOOD
    (255, 92, 70),   // BAD
    (255, 150, 40),  // WARN
    (8, 10, 20),     // PANEL_BG
    (10, 26, 30),    // RADAR_BG
    (34, 84, 84),    // RADAR_LINE
    (255, 80, 64),   // BLIP_RAIDER
    (240, 200, 80),  // BLIP_HAULER
    (110, 160, 255), // BLIP_WARDEN
    (120, 255, 140), // BLIP_STATION
    (230, 230, 230), // BLIP_CAN
    (120, 255, 200), // BEAM_ME
    (255, 90, 80),   // BEAM_FOE
    (255, 250, 210), // FIRE1
    (255, 184, 60),  // FIRE2
    (220, 70, 34),   // FIRE3
    (84, 64, 60),    // SMOKE
    (110, 200, 90),  // ECON_FARM
    (200, 140, 90),  // ECON_MINE
    (230, 110, 70),  // ECON_FORGE
    (100, 190, 255), // ECON_LAB
    (230, 120, 230), // ECON_HAVEN
    (60, 120, 90),   // MAP_RANGE
    (30, 36, 56),    // BAR_BG
    (90, 160, 255),  // SHIELD
    (255, 120, 40),  // HEAT
    (110, 220, 120), // FUEL
    (22, 28, 48),    // GRID
    (90, 120, 255),  // TUNNEL1
    (200, 120, 255), // TUNNEL2
];

/// Step L3: traders and pirates wear a system's own colours (picked by
/// the system's number; the first of each is the old one). Patrols
/// keep white and blue everywhere, so the law always looks the same.
pub const LIVERY_ON: bool = true;
const TRADE_LIVERY: [[(u8, u8, u8); 2]; 4] = [
    [(222, 176, 70), (128, 96, 60)],  // gold and brown
    [(70, 186, 176), (36, 90, 96)],   // teal
    [(226, 214, 186), (150, 70, 44)], // cream and rust
    [(128, 156, 204), (62, 72, 104)], // slate blue
];
const PIRATE_LIVERY: [[(u8, u8, u8); 2]; 4] = [
    [(176, 52, 48), (150, 60, 64)],  // crimson
    [(146, 64, 176), (88, 40, 112)], // violet
    [(214, 104, 34), (72, 52, 42)],  // rust orange and soot
    [(204, 44, 124), (96, 30, 72)],  // magenta
];

pub(crate) fn c565((r, g, b): (u8, u8, u8), k: u32) -> u16 {
    // k is at most 256: the product fits.
    let s = |v: u8| (u32::from(v).wrapping_mul(k) / 256) as u8;
    rgb565(s(r), s(g), s(b))
}

/// The palette for a system's planet `look` (0..8), `star` (0..5) and
/// number `h` (its nebula's tint).
pub fn palette(out: &mut [u16; COLOURS_LEN], look: u8, star: u8, h: u32) {
    let air = AIRS[usize::from(look % 8)];
    let look = LOOKS[usize::from(look % 8)];
    let star = STARS[usize::from(star % 5)];
    for s in 0..SHADES as usize {
        for m in 0..MATS as usize {
            let rgb = match m as u8 {
                mat::PLANET_A => look[0],
                mat::PLANET_B => look[1],
                mat::PLANET_C => look[2],
                mat::SUN => star[0],
                mat::SUN2 => star[1],
                mat::HAULER | mat::HAULER2 if LIVERY_ON => {
                    TRADE_LIVERY[((h >> 4) % 4) as usize][m - usize::from(mat::HAULER)]
                }
                mat::RAIDER | mat::RAIDER2 if LIVERY_ON => {
                    PIRATE_LIVERY[((h >> 6) % 4) as usize][m - usize::from(mat::RAIDER)]
                }
                _ => MAT_RGB[m],
            };
            out[s * MATS as usize + m] = c565(rgb, SHADE_K[s]);
        }
    }
    for (i, &rgb) in UI_RGB.iter().enumerate() {
        out[UI as usize + i] = c565(rgb, 256);
    }
    out[usize::from(FAR_SUN)] = c565(star[0], 256);
    out[usize::from(FAR_PLANET)] = c565(look[0], 230);
    out[usize::from(HALO)] = c565(star[1], 120);
    let neb = NEBULAS[(h % 6) as usize];
    out[usize::from(NEB1)] = c565(neb, 34);
    out[usize::from(NEB2)] = c565(neb, 58);
    out[usize::from(NEB3)] = c565(neb, 92);
    out[usize::from(ATMOS1)] = c565(air, 110);
    out[usize::from(ATMOS2)] = c565(air, 190);
    out[usize::from(CORONA1)] = c565(star[1], 56);
    out[usize::from(CORONA2)] = c565(star[0], 200);
}
