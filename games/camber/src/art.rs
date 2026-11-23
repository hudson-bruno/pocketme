//! The look: the palette, the three track themes, the entrants and the
//! ship model.
//!
//! Palette (the game sets 1..=239; 0 stays black):
//!
//! | Entries | What |
//! |---|---|
//! | 1..=144 | track materials: 12 materials x 4 haze steps x 3 lights |
//! | 145..=176 | ships: 8 entrants' bodies x 4 lights |
//! | 177..=187 | canopy (4), underside (4), engine, engine on boost, shadow |
//! | 188..=203 | sky, 16 steps from the horizon up |
//! | 204..=207 | ground (or sea), from the horizon down |
//! | 208..=212 | far hills (2), sun, star, sun ring |
//! | 213..=226 | the text and panels |
//! | 227..=238 | ships: 3 trims x 4 lights |

use game_kit::rgb565;

use crate::poly::{Face, Model, RawFace, normals, orient_by_middle, radius_of};

// ---- Track materials ---------------------------------------------------------

pub const ROAD: u8 = 0;
pub const STRIPE: u8 = 1;
pub const WALL: u8 = 2;
pub const UNDER: u8 = 3;
pub const ROOFM: u8 = 4;
pub const BOOSTM: u8 = 5;
pub const CHARGEM: u8 = 6;
pub const METAL: u8 = 7;
pub const SCEN_A: u8 = 8;
pub const SCEN_B: u8 = 9;
pub const LAMP: u8 = 10;
pub const CHECK: u8 = 11;
/// Materials that shine by themselves (the haze touches them less).
const GLOWS: u16 = 1 << BOOSTM | 1 << CHARGEM | 1 << LAMP;

/// The palette entry of a track material at a haze step (0 near .. 3
/// far) and a light (0 lit, 1 the other shade, 2 dark).
pub const fn mat(m: u8, haze: u8, light: u8) -> u8 {
    1 + m * 12 + haze * 3 + light
}

// ---- Ships and the rest ---------------------------------------------------------

/// An entrant's body colour, light 0..3 (entrant 0 is the player).
pub const fn body(e: u8) -> u8 {
    145 + e * 4
}
/// An entrant's trim colour, light 0..3 (one of the [`TRIMS`]).
pub const fn trim(e: u8) -> u8 {
    TRIM0 + ENTRANTS[e as usize % ENTRANTS.len()].trim * 4
}
const TRIM0: u8 = 227;
pub const CANOPY: u8 = 177;
pub const DARK: u8 = 181;
pub const ENGINE: u8 = 185;
pub const ENGINE_HOT: u8 = 186;
pub const SHADOW: u8 = 187;
pub const SKY: u8 = 188;
pub const SKY_STEPS: i32 = 16;
pub const GROUND: u8 = 204;
pub const HILL: u8 = 208;
pub const SUN: u8 = 210;
pub const STAR: u8 = 211;
pub const SUN_RING: u8 = 212;

pub const TEXT: u8 = 213;
pub const TEXT_DIM: u8 = 214;
pub const INK: u8 = 215;
pub const PANEL: u8 = 216;
pub const PANEL_LIGHT: u8 = 217;
pub const SEL: u8 = 218;
pub const BAR: u8 = 219;
pub const SHIELD: u8 = 220;
pub const SHIELD_LOW: u8 = 221;
pub const BOOST_UI: u8 = 222;
pub const GOLD: u8 = 223;
pub const SILVER: u8 = 224;
pub const BRONZE: u8 = 225;
pub const ACCENT: u8 = 226;
/// The carve's sparks (and its HUD marks) by the heat's step: dust before
/// the first step, then orange, red, white-hot. Existing colours, nothing
/// added, and none of them the player's ship's own (magenta body, yellow
/// trim, cyan engine), so the sparks never read as longer wings.
pub const SPARK: [u8; 4] = [TEXT_DIM, BRONZE, SHIELD_LOW, TEXT];
/// The colours this game sets: 1..=238.
pub const COLOURS: usize = 239;

type Rgb = [u8; 3];

const UI: [Rgb; 14] = [
    [0xF2, 0xF4, 0xF8], // TEXT
    [0x96, 0xA2, 0xB4], // TEXT_DIM
    [0x0C, 0x0E, 0x14], // INK
    [0x1A, 0x22, 0x32], // PANEL
    [0x34, 0x44, 0x5E], // PANEL_LIGHT
    [0x6C, 0xE8, 0xF0], // SEL
    [0x10, 0x14, 0x1E], // BAR
    [0x5C, 0xD6, 0xFF], // SHIELD
    [0xFF, 0x50, 0x40], // SHIELD_LOW
    [0xFF, 0xE0, 0x60], // BOOST_UI
    [0xFF, 0xC8, 0x30], // GOLD
    [0xC8, 0xD0, 0xDC], // SILVER
    [0xD0, 0x84, 0x48], // BRONZE
    [0x50, 0xA8, 0xFF], // ACCENT
];

/// An entrant: the name on the results and the ship's colours (its
/// own body colour, a trim from [`TRIMS`]). Eight entrants, each on its
/// own; the first is the player, whose colours no rival shares.
pub struct Entrant {
    pub name: &'static str,
    pub body: Rgb,
    pub trim: u8,
}

/// The trims: light, dark, and the player's own.
const TRIMS: [Rgb; 3] = [[0xF4, 0xF6, 0xFA], [0x30, 0x30, 0x36], [0xFF, 0xE8, 0x50]];

/// The eight entrants: the player, then the seven rivals in ship order.
pub const ENTRANTS: [Entrant; 8] = [
    Entrant {
        name: "YOU",
        body: [0xE8, 0x2C, 0x8C],
        trim: 2,
    },
    Entrant {
        name: "TIDEWAY",
        body: [0x2E, 0x8C, 0xF0],
        trim: 0,
    },
    Entrant {
        name: "CINDER",
        body: [0xE0, 0x3A, 0x2C],
        trim: 1,
    },
    Entrant {
        name: "SAFFRON",
        body: [0xF0, 0xC0, 0x30],
        trim: 1,
    },
    Entrant {
        name: "VERDANT",
        body: [0x3C, 0xC0, 0x6A],
        trim: 0,
    },
    Entrant {
        name: "CHALK",
        body: [0xE4, 0xE8, 0xEE],
        trim: 1,
    },
    Entrant {
        name: "BASALT",
        body: [0x44, 0x48, 0x54],
        trim: 0,
    },
    Entrant {
        name: "MARIGOLD",
        body: [0xF0, 0x84, 0x20],
        trim: 0,
    },
];

// ---- Themes ---------------------------------------------------------------------

/// What stands beside the road.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flora {
    /// Tall trunks with a crown (palms by the sea).
    Palms,
    /// Rock needles and chimneys.
    Needles,
    /// Ice spires and dark firs.
    Firs,
}

/// One track's look.
pub struct Theme {
    /// Sky at the horizon, half way up, at the top.
    pub sky: [Rgb; 3],
    /// Ground (or sea) at the horizon and right below.
    pub ground: [Rgb; 2],
    /// Far hills, far and near.
    pub hills: [Rgb; 2],
    /// How high the hills reach (pixels at the lens of the game).
    pub hill_h: i32,
    pub hill_seed: u32,
    /// The haze far things fade into.
    pub haze: Rgb,
    pub sun: Rgb,
    /// Where the sun stands: heading and height (angle units).
    pub sun_yaw: u16,
    pub sun_up: u16,
    /// The sun's radius in pixels.
    pub sun_r: i32,
    pub stars: bool,
    /// Each material: lit and other colour (the dark one is made).
    pub mats: [[Rgb; 2]; 12],
    pub flora: Flora,
}

pub static THEMES: [Theme; 3] = [
    // LAGOON: a bright day over a shallow sea.
    Theme {
        sky: [[0xD8, 0xEC, 0xF6], [0x86, 0xC0, 0xEE], [0x2C, 0x6C, 0xD0]],
        ground: [[0x9C, 0xCC, 0xDC], [0x1C, 0x7C, 0xA0]],
        hills: [[0x8C, 0xB4, 0xB8], [0x4C, 0x88, 0x78]],
        hill_h: 9,
        hill_seed: 11,
        haze: [0xC4, 0xE0, 0xEC],
        sun: [0xFF, 0xFC, 0xE8],
        sun_yaw: 0x3000,
        sun_up: 0x0C00,
        sun_r: 4,
        stars: false,
        mats: [
            [[0x5A, 0x64, 0x74], [0x50, 0x5A, 0x6A]], // ROAD
            [[0xF4, 0xF4, 0xF0], [0xE8, 0x48, 0x38]], // STRIPE
            [[0xE6, 0xEA, 0xEE], [0xC4, 0xCC, 0xD4]], // WALL
            [[0x7C, 0x84, 0x90], [0x60, 0x68, 0x74]], // UNDER
            [[0x80, 0x88, 0x94], [0x6C, 0x74, 0x80]], // ROOF
            [[0xFF, 0xE8, 0x50], [0xFF, 0x98, 0x20]], // BOOST
            [[0x60, 0xF0, 0xC8], [0x30, 0xB8, 0x98]], // CHARGE
            [[0xD0, 0xD8, 0xE0], [0x9C, 0xA6, 0xB2]], // METAL
            [[0x3C, 0xA0, 0x48], [0x2C, 0x80, 0x3C]], // SCEN_A: crowns
            [[0xA0, 0x7C, 0x54], [0x84, 0x64, 0x44]], // SCEN_B: trunks
            [[0xFF, 0xFF, 0xF0], [0xFF, 0xF0, 0xB0]], // LAMP
            [[0xF8, 0xF8, 0xF8], [0x20, 0x22, 0x28]], // CHECK
        ],
        flora: Flora::Palms,
    },
    // FURNACE: dusk in a canyon of dark rock.
    Theme {
        sky: [[0xFF, 0xA8, 0x5C], [0xB4, 0x50, 0x5C], [0x2A, 0x1C, 0x44]],
        ground: [[0x8C, 0x50, 0x40], [0x3A, 0x22, 0x1E]],
        hills: [[0x7A, 0x40, 0x38], [0x48, 0x24, 0x22]],
        hill_h: 22,
        hill_seed: 23,
        haze: [0xB0, 0x68, 0x58],
        sun: [0xFF, 0xD8, 0x90],
        sun_yaw: 0xA000,
        sun_up: 0x0280,
        sun_r: 7,
        stars: false,
        mats: [
            [[0x4C, 0x40, 0x44], [0x42, 0x38, 0x3C]], // ROAD
            [[0xFF, 0xB8, 0x30], [0x30, 0x2A, 0x2C]], // STRIPE
            [[0x9C, 0x8C, 0x88], [0x7C, 0x6C, 0x6A]], // WALL
            [[0x5C, 0x44, 0x40], [0x48, 0x34, 0x30]], // UNDER
            [[0x5A, 0x4C, 0x4C], [0x4A, 0x3E, 0x40]], // ROOF
            [[0xFF, 0xF0, 0x70], [0xFF, 0x90, 0x10]], // BOOST
            [[0x70, 0xE8, 0xFF], [0x30, 0xA8, 0xD8]], // CHARGE
            [[0x8C, 0x84, 0x88], [0x68, 0x60, 0x64]], // METAL
            [[0x6C, 0x38, 0x2C], [0x54, 0x2C, 0x24]], // SCEN_A: needles
            [[0x40, 0x34, 0x34], [0x30, 0x28, 0x28]], // SCEN_B: chimneys
            [[0xFF, 0xC0, 0x60], [0xFF, 0x80, 0x30]], // LAMP
            [[0xF0, 0xE8, 0xE0], [0x24, 0x1C, 0x1C]], // CHECK
        ],
        flora: Flora::Needles,
    },
    // GLACIER: a clear night over snow and ice.
    Theme {
        sky: [[0x30, 0x4C, 0x7C], [0x14, 0x22, 0x4C], [0x04, 0x08, 0x1C]],
        ground: [[0x50, 0x64, 0x8C], [0xB4, 0xC8, 0xE4]],
        hills: [[0x3C, 0x50, 0x7C], [0x8C, 0xA4, 0xCC]],
        hill_h: 18,
        hill_seed: 37,
        haze: [0x2C, 0x40, 0x68],
        sun: [0xE8, 0xF0, 0xFF],
        sun_yaw: 0x6000,
        sun_up: 0x1400,
        sun_r: 3,
        stars: true,
        mats: [
            [[0x6C, 0x78, 0x98], [0x60, 0x6C, 0x8C]], // ROAD
            [[0x80, 0xF0, 0xFF], [0x20, 0x30, 0x58]], // STRIPE
            [[0xC8, 0xD8, 0xF0], [0xA0, 0xB4, 0xD4]], // WALL
            [[0x48, 0x54, 0x74], [0x38, 0x44, 0x60]], // UNDER
            [[0x58, 0x68, 0x88], [0x4C, 0x5A, 0x78]], // ROOF
            [[0xFF, 0xF0, 0x80], [0xFF, 0xA0, 0x40]], // BOOST
            [[0x80, 0xFF, 0xB0], [0x40, 0xC8, 0x80]], // CHARGE
            [[0xA8, 0xB8, 0xD0], [0x80, 0x90, 0xAC]], // METAL
            [[0xD0, 0xE8, 0xFF], [0xA0, 0xC0, 0xE8]], // SCEN_A: ice
            [[0x1C, 0x34, 0x30], [0x14, 0x28, 0x24]], // SCEN_B: firs
            [[0xC0, 0xE8, 0xFF], [0x80, 0xC0, 0xFF]], // LAMP
            [[0xF0, 0xF4, 0xFF], [0x18, 0x1C, 0x2C]], // CHECK
        ],
        flora: Flora::Firs,
    },
];

const fn mix(a: Rgb, b: Rgb, t: i32, of: i32) -> Rgb {
    let mut o = [0u8; 3];
    let mut i = 0;
    while i < 3 {
        o[i] = (a[i] as i32 + (b[i] as i32 - a[i] as i32) * t / of) as u8;
        i += 1;
    }
    o
}

const fn shade(a: Rgb, num: i32) -> Rgb {
    mix([0, 0, 0], a, num, 16)
}

fn c565(c: Rgb) -> u16 {
    rgb565(c[0], c[1], c[2])
}

/// How much haze each step adds, of 16.
const HAZE: [i32; 4] = [0, 5, 9, 13];

/// The game's colours for a theme: entries 0..[`COLOURS`].
pub fn palette(th: &Theme, out: &mut [u16; COLOURS]) {
    out[0] = 0;
    for (m, pair) in th.mats.iter().enumerate() {
        let glow = GLOWS & (1 << m) != 0;
        let lights = [pair[0], pair[1], shade(pair[0], 10)];
        for h in 0..4u8 {
            let k = if glow {
                HAZE[h as usize] / 2
            } else {
                HAZE[h as usize]
            };
            for (l, c) in lights.iter().enumerate() {
                out[usize::from(mat(m as u8, h, l as u8))] = c565(mix(*c, th.haze, k, 16));
            }
        }
    }
    for l in 0..4 {
        let k = 16 - 3 * l as i32;
        for (e, en) in ENTRANTS.iter().enumerate() {
            out[usize::from(body(e as u8)) + l] = c565(shade(en.body, k));
        }
        for (t, c) in TRIMS.iter().enumerate() {
            out[usize::from(TRIM0) + t * 4 + l] = c565(shade(*c, k));
        }
        out[usize::from(CANOPY) + l] = c565(shade([0x30, 0x48, 0x60], k));
        out[usize::from(DARK) + l] = c565(shade([0x3A, 0x3E, 0x48], k));
    }
    out[usize::from(ENGINE)] = c565([0x80, 0xD8, 0xFF]);
    out[usize::from(ENGINE_HOT)] = c565([0xFF, 0xF0, 0xA0]);
    out[usize::from(SHADOW)] = c565(shade(th.mats[0][0], 7));
    for k in 0..SKY_STEPS {
        let c = if k < 6 {
            mix(th.sky[0], th.sky[1], k, 6)
        } else {
            mix(th.sky[1], th.sky[2], k - 6, SKY_STEPS - 7)
        };
        out[usize::from(SKY) + k as usize] = c565(c);
    }
    for k in 0..4 {
        out[usize::from(GROUND) + k as usize] = c565(mix(th.ground[0], th.ground[1], k, 3));
    }
    out[usize::from(HILL)] = c565(th.hills[0]);
    out[usize::from(HILL) + 1] = c565(th.hills[1]);
    out[usize::from(SUN)] = c565(th.sun);
    out[usize::from(STAR)] = c565(mix(th.sun, th.sky[2], 4, 16));
    out[usize::from(SUN_RING)] = c565(mix(th.sun, th.sky[0], 8, 16));
    for (k, c) in UI.iter().enumerate() {
        out[usize::from(TEXT) + k] = c565(*c);
    }
}

/// Where the light comes from, as the direction it falls (unit, 1/16384):
/// from above and a little from the side, the same for every track.
pub const LIGHT: [i32; 3] = [-5_500, -14_000, 6_000];

// ---- The ship ---------------------------------------------------------------------

/// Ship materials: body, trim, canopy, underside, engine.
pub const S_BODY: u8 = 0;
pub const S_TRIM: u8 = 1;
pub const S_CANOPY: u8 = 2;
pub const S_DARK: u8 = 3;
pub const S_ENGINE: u8 = 4;

/// The ship: an arrow-headed hull with swept wings and one fin. About
/// 4.1 m long and 2.6 m across (1/256 m; x right, y up, z forward).
const SHIP_V: [[i16; 3]; 21] = [
    [0, 26, 600],     // 0 nose
    [0, 112, 150],    // 1 front ring: top
    [-118, 40, 150],  // 2 left
    [118, 40, 150],   // 3 right
    [0, -28, 150],    // 4 bottom
    [0, 122, -420],   // 5 rear ring: top
    [-150, 52, -420], // 6 left
    [150, 52, -420],  // 7 right
    [0, -30, -420],   // 8 bottom
    [-108, 46, 40],   // 9 left wing: root front
    [-142, 50, -400], // 10 root rear
    [-336, 18, -480], // 11 tip
    [-170, 18, -260], // 12 under
    [108, 46, 40],    // 13 right wing
    [142, 50, -400],  // 14
    [336, 18, -480],  // 15
    [170, 18, -260],  // 16
    [0, 112, -170],   // 17 fin: front
    [0, 206, -470],   // 18 top
    [-20, 112, -400], // 19 left foot
    [20, 112, -400],  // 20 right foot
];

const fn rf(a: u8, b: u8, c: u8, mat: u8, part: u8) -> RawFace {
    RawFace {
        v: [a, b, c],
        mat,
        part,
    }
}

const SHIP_RAW: [RawFace; 23] = [
    rf(0, 2, 1, S_CANOPY, 0),
    rf(0, 1, 3, S_CANOPY, 0),
    rf(0, 4, 2, S_DARK, 0),
    rf(0, 3, 4, S_DARK, 0),
    rf(1, 2, 6, S_BODY, 0),
    rf(1, 6, 5, S_BODY, 0),
    rf(1, 5, 7, S_BODY, 0),
    rf(1, 7, 3, S_BODY, 0),
    rf(2, 4, 8, S_DARK, 0),
    rf(2, 8, 6, S_DARK, 0),
    rf(3, 7, 8, S_DARK, 0),
    rf(3, 8, 4, S_DARK, 0),
    rf(5, 6, 8, S_ENGINE, 0),
    rf(5, 8, 7, S_ENGINE, 0),
    rf(9, 10, 11, S_TRIM, 1),
    rf(9, 11, 12, S_DARK, 1),
    rf(10, 12, 11, S_DARK, 1),
    rf(13, 14, 15, S_TRIM, 2),
    rf(13, 15, 16, S_DARK, 2),
    rf(14, 16, 15, S_DARK, 2),
    rf(17, 18, 19, S_TRIM, 3),
    rf(17, 20, 18, S_TRIM, 3),
    rf(18, 20, 19, S_TRIM, 3),
];

const SHIP_F: [Face; 23] = orient_by_middle(&SHIP_V, &SHIP_RAW);
const SHIP_N: [[i16; 3]; 23] = normals(&SHIP_V, &SHIP_F);

pub static SHIP: Model = Model {
    verts: &SHIP_V,
    faces: &SHIP_F,
    normals: &SHIP_N,
    radius: radius_of(&SHIP_V),
};
