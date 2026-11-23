//! The palette, the wall and floor pictures (textures) and the sprites.
//! All of it is made while compiling: textures by small whole-number
//! pattern functions, sprites from letter tables drawn by hand.
//!
//! **Shading through the palette.** A texture holds *materials* (0 to
//! [`MATS`] - 1), not colours. The palette holds every material at
//! [`SHADES`] brightness steps: palette number = shade x [`MATS`] +
//! material. Darkening a pixel by distance is one addition, and the
//! palette is set once, so it never forces a full-screen send (a palette
//! change does on this console). Shade 0 is in full lamp light, shade
//! [`DARK`] is black.

use game_kit::rgb565;

// ---- Materials --------------------------------------------------------------

pub const BLACK: u8 = 0;
pub const ROCK1: u8 = 1;
pub const ROCK2: u8 = 2;
pub const ROCK3: u8 = 3;
pub const ROCK4: u8 = 4;
pub const WOOD1: u8 = 5;
pub const WOOD2: u8 = 6;
pub const WOOD3: u8 = 7;
pub const IRON1: u8 = 8;
pub const IRON2: u8 = 9;
pub const IRON3: u8 = 10;
pub const RUST: u8 = 11;
pub const CRYS1: u8 = 12;
pub const CRYS2: u8 = 13;
pub const CRYS3: u8 = 14;
pub const GOLD: u8 = 15;
pub const WHITE: u8 = 16;
pub const RED: u8 = 17;
pub const MOTH1: u8 = 18;
pub const MOTH2: u8 = 19;
pub const BUG1: u8 = 20;
pub const BUG2: u8 = 21;
pub const HAZARD: u8 = 22;
pub const MOSS: u8 = 23;
pub const DIRT1: u8 = 24;
pub const DIRT2: u8 = 25;
/// Eyes and lamps: as bright in the dark as in the light.
pub const GLOW: u8 = 26;
pub const SLATE: u8 = 27;

/// Materials.
pub const MATS: usize = 28;
/// Brightness steps.
pub const SHADES: usize = 8;
/// The darkest step: black.
pub const DARK: u8 = SHADES as u8 - 1;
/// Sprite pixels that are not drawn.
pub const SEE_THROUGH: u8 = 0xFF;

/// Each material in full light, as (red, green, blue), 0 to 255.
const MAT_RGB: [(u32, u32, u32); MATS] = [
    (0, 0, 0),       // BLACK
    (92, 84, 78),    // ROCK1
    (122, 112, 100), // ROCK2
    (156, 144, 124), // ROCK3
    (52, 46, 44),    // ROCK4: cracks
    (120, 78, 44),   // WOOD1
    (162, 112, 64),  // WOOD2
    (74, 48, 30),    // WOOD3
    (96, 100, 112),  // IRON1
    (156, 162, 172), // IRON2
    (50, 54, 66),    // IRON3
    (150, 74, 40),   // RUST
    (70, 200, 220),  // CRYS1
    (170, 246, 252), // CRYS2
    (120, 84, 210),  // CRYS3
    (250, 196, 70),  // GOLD
    (244, 240, 228), // WHITE
    (214, 52, 44),   // RED
    (214, 200, 162), // MOTH1: wings
    (118, 92, 108),  // MOTH2: body
    (78, 140, 64),   // BUG1: shell
    (36, 76, 40),    // BUG2: shell edge
    (232, 182, 40),  // HAZARD paint
    (86, 124, 52),   // MOSS
    (98, 76, 56),    // DIRT1
    (68, 54, 42),    // DIRT2
    (255, 236, 120), // GLOW
    (58, 66, 86),    // SLATE
];

/// Light left at each step, in 1/256, and how much blue the lamp's warm
/// light takes away (1/256) at that step.
const SHADE_LIGHT: [u32; SHADES] = [256, 204, 158, 118, 84, 54, 28, 0];
const SHADE_WARM: [u32; SHADES] = [40, 30, 20, 10, 0, 0, 0, 0];

// ---- Colours of the screens around the 3D view --------------------------------

/// The first palette number after the shaded materials.
pub const UI: u8 = (MATS * SHADES) as u8;
pub const UI_BG: u8 = UI;
pub const UI_EDGE: u8 = UI + 1;
pub const TEXT: u8 = UI + 2;
pub const TEXT_DIM: u8 = UI + 3;
pub const ACCENT: u8 = UI + 4;
pub const HEART: u8 = UI + 5;
pub const HEART_OFF: u8 = UI + 6;
pub const OIL: u8 = UI + 7;
pub const OIL_LOW: u8 = UI + 8;
pub const CRYSTAL: u8 = UI + 9;
pub const MAP_WALL: u8 = UI + 10;
pub const MAP_FLOOR: u8 = UI + 11;
pub const MAP_DOOR: u8 = UI + 12;
pub const MAP_LIFT: u8 = UI + 13;
pub const MAP_ME: u8 = UI + 14;
pub const MAP_GATE: u8 = UI + 15;
/// Palette numbers the game sets: 0 to `COLOURS.len() - 1`.
pub const UI_COUNT: usize = 16;

const UI_RGB: [(u32, u32, u32); UI_COUNT] = [
    (18, 14, 20),    // UI_BG
    (88, 70, 52),    // UI_EDGE
    (244, 234, 212), // TEXT
    (150, 138, 124), // TEXT_DIM
    (255, 186, 72),  // ACCENT
    (224, 64, 60),   // HEART
    (74, 40, 42),    // HEART_OFF
    (246, 186, 64),  // OIL
    (226, 84, 44),   // OIL_LOW
    (96, 222, 240),  // CRYSTAL
    (136, 124, 112), // MAP_WALL
    (44, 38, 36),    // MAP_FLOOR
    (176, 112, 62),  // MAP_DOOR
    (96, 210, 120),  // MAP_LIFT
    (255, 240, 120), // MAP_ME
    (120, 132, 156), // MAP_GATE
];

/// Every colour the game sets, from palette number 0: the materials at
/// each shade, then the screens' colours. 240 numbers: all the game has.
pub static COLOURS: [u16; MATS * SHADES + UI_COUNT] = colours();

const fn colours() -> [u16; MATS * SHADES + UI_COUNT] {
    let mut out = [0u16; MATS * SHADES + UI_COUNT];
    let mut s = 0;
    while s < SHADES {
        let mut m = 0;
        while m < MATS {
            let (r, g, b) = MAT_RGB[m];
            let (r, g, b) = if m == GLOW as usize {
                // Eyes and lamps keep their light, a little less far off.
                let k = 256 - (s as u32) * 12;
                (r * k / 256, g * k / 256, b * k / 256)
            } else {
                let k = SHADE_LIGHT[s];
                let warm = 256 - SHADE_WARM[s];
                (r * k / 256, g * k / 256, b * k / 256 * warm / 256)
            };
            out[s * MATS + m] = rgb565(r as u8, g as u8, b as u8);
            m += 1;
        }
        s += 1;
    }
    let mut i = 0;
    while i < UI_COUNT {
        let (r, g, b) = UI_RGB[i];
        out[MATS * SHADES + i] = rgb565(r as u8, g as u8, b as u8);
        i += 1;
    }
    out
}

/// The palette number of a material at a shade.
pub const fn shaded(mat: u8, shade: u8) -> u8 {
    shade * MATS as u8 + mat
}

// ---- Textures -----------------------------------------------------------------

/// Texture side in texels (a texture is one map cell's face).
pub const TEX: usize = 32;
/// Bits of [`TEX`].
pub const TEX_SHIFT: u32 = 5;
/// One texture, stored column by column (`x * 32 + y`) so a wall column
/// is read straight down.
pub type Texture = [u8; TEX * TEX];

/// Texture numbers.
pub const T_ROCK: usize = 0;
pub const T_TIMBER: usize = 1;
pub const T_VEIN: usize = 2;
pub const T_PANEL: usize = 3;
pub const T_DOOR: usize = 4;
pub const T_GATE_A: usize = 5;
pub const T_GATE_B: usize = 6;
pub const T_SW_A_OFF: usize = 7;
pub const T_SW_A_ON: usize = 8;
pub const T_SW_B_OFF: usize = 9;
pub const T_SW_B_ON: usize = 10;
pub const T_LIFT: usize = 11;
pub const T_FLOOR: usize = 12;
pub const T_CEIL: usize = 13;
pub const TEXTURES: usize = 14;

pub static TEXS: [Texture; TEXTURES] = [
    make(T_ROCK),
    make(T_TIMBER),
    make(T_VEIN),
    make(T_PANEL),
    make(T_DOOR),
    make(T_GATE_A),
    make(T_GATE_B),
    make(T_SW_A_OFF),
    make(T_SW_A_ON),
    make(T_SW_B_OFF),
    make(T_SW_B_ON),
    make(T_LIFT),
    make(T_FLOOR),
    make(T_CEIL),
];

/// A small whole-number hash: the same "random" texel every build.
const fn hash(x: u32, y: u32, seed: u32) -> u32 {
    let mut h = x
        .wrapping_mul(374_761_393)
        .wrapping_add(y.wrapping_mul(668_265_263))
        .wrapping_add(seed.wrapping_mul(2_246_822_519));
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
}

/// Rough stone blocks with dark cracks between them.
const fn rock(x: u32, y: u32, seed: u32) -> u8 {
    // Four bands of blocks; each band's blocks start at another x.
    let bands = [0u32, 9, 16, 24, 32];
    let mut b = 0;
    while b < 4 && y >= bands[b + 1] {
        b += 1;
    }
    let top = bands[b];
    let shift = (b as u32 * 7 + seed) % 11;
    let bw = 11 + (b as u32 % 2);
    let xs = (x + shift) % 32;
    let block = xs / bw;
    let in_x = xs % bw;
    if y == top || in_x == 0 {
        return ROCK4;
    }
    let n = hash(x, y, seed) % 23;
    if n == 0 {
        return ROCK4;
    }
    if y == top + 1 || n == 1 {
        return ROCK3;
    }
    match hash(block, b as u32, seed + 7) % 3 {
        0 => ROCK1,
        1 => ROCK2,
        _ => {
            if n < 8 {
                ROCK2
            } else {
                ROCK1
            }
        }
    }
}

const fn wood(x: u32, y: u32, along_y: bool) -> u8 {
    // Grain lines along the plank.
    let across = if along_y { x } else { y };
    let along = if along_y { y } else { x };
    if hash(across, along / 9, 3).is_multiple_of(5) {
        WOOD3
    } else if across % 3 == 0 && !hash(across, along, 4).is_multiple_of(3) {
        WOOD1
    } else {
        WOOD2
    }
}

const fn texel(t: usize, x: u32, y: u32) -> u8 {
    match t {
        T_ROCK => rock(x, y, 1),
        T_TIMBER => {
            // Two posts and a beam across the top: the mine's props.
            let post = (x >= 2 && x <= 6) || (x >= 25 && x <= 29);
            if y >= 2 && y <= 7 {
                if y == 2 || y == 7 {
                    WOOD3
                } else {
                    wood(x, y, false)
                }
            } else if post {
                if x == 2 || x == 6 || x == 25 || x == 29 {
                    WOOD3
                } else {
                    wood(x, y, true)
                }
            } else {
                rock(x, y, 2)
            }
        }
        T_VEIN => {
            // Crystal veins across the rock.
            let a = (x as i32 - (y as i32 * 3) / 4 - 6).abs();
            let b = (x as i32 + y as i32 / 2 - 26).abs();
            if a <= 1 || b <= 1 {
                match hash(x, y, 9) % 4 {
                    0 => CRYS2,
                    1 => CRYS3,
                    _ => CRYS1,
                }
            } else if a == 2 || b == 2 {
                ROCK4
            } else {
                rock(x, y, 3)
            }
        }
        T_PANEL => {
            // Iron plates with rivets, and a band of warning paint.
            if y >= 25 && y <= 29 {
                if (x + y) % 8 < 4 { HAZARD } else { BLACK }
            } else if x.is_multiple_of(16) || y == 0 || y == 15 || y == 24 {
                IRON3
            } else if (x % 16 == 2 || x % 16 == 13) && (y == 2 || y == 13 || y == 17 || y == 22)
                || x % 16 == 1
                || y == 1
                || y == 16
            {
                // Rivets, and the plates' lit edges.
                IRON2
            } else if hash(x, y, 5).is_multiple_of(19) {
                RUST
            } else {
                IRON1
            }
        }
        T_DOOR => {
            // Upright planks, two iron bands, a handle.
            if x == 0 || x == 31 || y == 0 {
                WOOD3
            } else if (y >= 5 && y <= 7) || (y >= 24 && y <= 26) {
                if (y == 6 || y == 25) && x % 7 == 3 {
                    IRON2
                } else {
                    IRON1
                }
            } else if x >= 23 && x <= 26 && y >= 14 && y <= 17 {
                if y == 14 { IRON2 } else { IRON3 }
            } else if x.is_multiple_of(8) {
                WOOD3
            } else {
                wood(x, y, true)
            }
        }
        T_GATE_A | T_GATE_B => {
            // Iron bars with a coloured plate on top.
            let plate = if t == T_GATE_A { HAZARD } else { CRYS1 };
            if y <= 3 {
                if y == 3 {
                    IRON3
                } else if x % 4 == 1 {
                    BLACK
                } else {
                    plate
                }
            } else if (y >= 14 && y <= 15) || y >= 29 {
                if y == 14 || y == 29 { IRON2 } else { IRON1 }
            } else if x % 6 == 2 {
                IRON2
            } else if x % 6 == 3 {
                IRON1
            } else if x % 6 == 4 {
                IRON3
            } else if hash(x, y, 11).is_multiple_of(17) {
                ROCK4
            } else {
                BLACK
            }
        }
        T_SW_A_OFF | T_SW_A_ON | T_SW_B_OFF | T_SW_B_ON => {
            let on = t == T_SW_A_ON || t == T_SW_B_ON;
            let plate = if t == T_SW_A_OFF || t == T_SW_A_ON {
                HAZARD
            } else {
                CRYS1
            };
            if x >= 9 && x <= 22 && y >= 6 && y <= 26 {
                let edge = x == 9 || x == 22 || y == 6 || y == 26;
                if edge {
                    IRON3
                } else if y <= 9 {
                    plate
                } else if x >= 18 && x <= 20 && y >= 11 && y <= 13 {
                    // The lamp: red while off, glowing once on.
                    if on { GLOW } else { RED }
                } else if x >= 14 && x <= 16 {
                    // The lever: up when on, down when off.
                    let (a, b) = if on { (11, 18) } else { (18, 25) };
                    if y >= a && y <= b {
                        if (on && y <= a + 1) || (!on && y >= b - 1) {
                            RED
                        } else {
                            IRON2
                        }
                    } else if y == 18 {
                        IRON3
                    } else {
                        IRON1
                    }
                } else {
                    IRON1
                }
            } else {
                rock(x, y, 4)
            }
        }
        T_LIFT => {
            // A cage door with warning stripes and an arrow up.
            if y <= 3 || y >= 28 {
                if (x + y) % 8 < 4 { HAZARD } else { BLACK }
            } else if x <= 1 || x >= 30 {
                IRON3
            } else {
                let ax = (x as i32 - 16).abs();
                let arrow =
                    (y >= 8 && y <= 14 && ax <= (y as i32 - 8)) || (y > 14 && y <= 22 && ax <= 2);
                if arrow {
                    GOLD
                } else if x.is_multiple_of(5) {
                    IRON2
                } else if y.is_multiple_of(6) {
                    IRON1
                } else {
                    SLATE
                }
            }
        }
        T_FLOOR => {
            let n = hash(x, y, 21) % 29;
            if n == 0 {
                ROCK2
            } else if n == 1 {
                ROCK3
            } else if (x / 4 + y / 4).is_multiple_of(2) && n < 14
                || hash(x / 2, y / 2, 22).is_multiple_of(3)
            {
                DIRT2
            } else {
                DIRT1
            }
        }
        _ => {
            // T_CEIL: rock with one timber beam across each cell.
            if y >= 13 && y <= 18 {
                if y == 13 || y == 18 {
                    WOOD3
                } else {
                    wood(x, y, false)
                }
            } else if hash(x, y, 31).is_multiple_of(7) {
                ROCK4
            } else if hash(x / 3, y / 3, 32).is_multiple_of(2) {
                ROCK1
            } else {
                SLATE
            }
        }
    }
}

const fn make(t: usize) -> Texture {
    let mut out = [0u8; TEX * TEX];
    let mut x = 0;
    while x < TEX {
        let mut y = 0;
        while y < TEX {
            out[x * TEX + y] = texel(t, x as u32, y as u32);
            y += 1;
        }
        x += 1;
    }
    out
}

// ---- Sprites ------------------------------------------------------------------

/// Sprite side in texels.
pub const SPR: usize = 16;
/// One sprite, column by column (`x * 16 + y`), [`SEE_THROUGH`] where
/// nothing is drawn.
pub type SpriteTex = [u8; SPR * SPR];

/// Letters of the sprite tables.
const fn ink(c: u8) -> u8 {
    match c {
        b'.' => SEE_THROUGH,
        b'k' => BLACK,
        b'r' => ROCK2,
        b'R' => ROCK3,
        b'w' => WOOD1,
        b'W' => WOOD2,
        b'v' => WOOD3,
        b'i' => IRON1,
        b'I' => IRON2,
        b'j' => IRON3,
        b'u' => RUST,
        b'c' => CRYS1,
        b'C' => CRYS2,
        b'p' => CRYS3,
        b'y' => GOLD,
        b'Y' => WHITE,
        b'e' => RED,
        b'm' => MOTH1,
        b'n' => MOTH2,
        b'g' => BUG1,
        b'G' => BUG2,
        b'h' => HAZARD,
        b'E' => GLOW,
        b's' => SLATE,
        _ => panic!("sprite: unknown letter"),
    }
}

/// Turns 16 rows of 16 letters into a sprite (column by column).
const fn sprite(rows: [&[u8]; SPR]) -> SpriteTex {
    let mut out = [SEE_THROUGH; SPR * SPR];
    let mut y = 0;
    while y < SPR {
        assert!(rows[y].len() == SPR, "sprite rows are 16 letters");
        let mut x = 0;
        while x < SPR {
            out[x * SPR + y] = ink(rows[y][x]);
            x += 1;
        }
        y += 1;
    }
    out
}

pub const S_CRYSTAL0: usize = 0;
pub const S_CRYSTAL1: usize = 1;
pub const S_OIL: usize = 2;
pub const S_FLASK: usize = 3;
pub const S_CRANK: usize = 4;
pub const S_MOTH0: usize = 5;
pub const S_MOTH1: usize = 6;
pub const S_BUG0: usize = 7;
pub const S_BUG1: usize = 8;
pub const S_BUG_CURL: usize = 9;
pub const S_DIG0: usize = 10;
pub const S_DIG1: usize = 11;
pub const S_DIG_STOP: usize = 12;

pub static SPRITES: [SpriteTex; 13] = [
    sprite([
        b"................",
        b"................",
        b".......Y........",
        b"......cCc.......",
        b"......cCc.......",
        b".....pcCcp..Y...",
        b".....pcCcp.cCc..",
        b".....pccCp.cCc..",
        b"..Y..pccCp.ccp..",
        b".cCc.pccCp.ccp..",
        b".cCc.pccCppccp..",
        b".ccp.pcccppcpp..",
        b".pcp.pcccppcp...",
        b"..rrRRrrrrRRrr..",
        b".rRrrrrRrrrrrRr.",
        b"................",
    ]),
    sprite([
        b"................",
        b"................",
        b"................",
        b"......cCc.......",
        b"......cYc.......",
        b".....pcCcp......",
        b".....pcCcp.cCc..",
        b".....pccCp.cYc..",
        b".....pccCp.ccp..",
        b".cCc.pccCp.ccp..",
        b".cYc.pccCppccp..",
        b".ccp.pcccppcpp..",
        b".pcp.pcccppcp...",
        b"..rrRRrrrrRRrr..",
        b".rRrrrrRrrrrrRr.",
        b"................",
    ]),
    sprite([
        b"................",
        b"................",
        b".....jjjjj......",
        b".....j...j......",
        b"....jjjjjjjj....",
        b"...jIIIiiiiij...",
        b"...jIyyyyyyijjj.",
        b"...jIyhhhhyij.j.",
        b"...jIyhkkhyij...",
        b"...jIyhhhhyij...",
        b"...jIyyyyyyij...",
        b"...jIiiiiiiij...",
        b"...jIiiuiiiij...",
        b"...jjjjjjjjjj...",
        b"................",
        b"................",
    ]),
    sprite([
        b"................",
        b"......vvv.......",
        b"......wWw.......",
        b"......jij.......",
        b"....jjIIIjj.....",
        b"...jIIIIIIIj....",
        b"..jIIeeIeeIIj...",
        b"..jIeeeeeeeIj...",
        b"..jIeeeeeeeIj...",
        b"..jIIeeeeeIIj...",
        b"..jIIIeeeIIIj...",
        b"..jIIIIeIIIIj...",
        b"...jIIIIIIIj....",
        b"....jjjjjjj.....",
        b"................",
        b"................",
    ]),
    sprite([
        b"................",
        b"................",
        b"..........WWw...",
        b"..........WYw...",
        b"..........WWw...",
        b"..........jjj...",
        b"..........IiI...",
        b"..........IiI...",
        b"..........IiI...",
        b"..IYIIIIIIIiI...",
        b"..iiiiiiiiiij...",
        b"..jjjjjjjjjjj...",
        b"..IjI...........",
        b"..IjI...........",
        b"..jjj...........",
        b"................",
    ]),
    sprite([
        b"................",
        b"..mm........mm..",
        b".mmmm......mmmm.",
        b"mmnmmm....mmmnmm",
        b"mmnnmmm..mmmnnmm",
        b"mmmnnmmnnmmnnmmm",
        b".mmmnnmnnmnnmmm.",
        b"..mmmmnEEnmmmm..",
        b"...mmmnnnnmmm...",
        b"..mmmmnnnnmmmm..",
        b".mmnmm.nn.mmnmm.",
        b".mnnm..nn..mnnm.",
        b"..mm...nn...mm..",
        b"................",
        b"................",
        b"................",
    ]),
    sprite([
        b"................",
        b"................",
        b"......m..m......",
        b".....mm..mm.....",
        b"....mnm..mnm....",
        b"....mnmnnmnm....",
        b"....mmnnnnmm....",
        b".....mnEEnm.....",
        b".....mnnnnm.....",
        b"....mmnnnnmm....",
        b"...mnm.nn.mnm...",
        b"...mm..nn..mm...",
        b".......nn.......",
        b"................",
        b"................",
        b"................",
    ]),
    sprite([
        b"................",
        b"................",
        b"................",
        b"................",
        b"....GGGGGGGG....",
        b"..GGggggggggGG..",
        b".GggggGggGgggggG",
        b".GgggGggggGggggG",
        b"GgggGggggggGgggG",
        b"GGGGGGGGGGGGGGGG",
        b"GkEkGkkkkkkGkEkG",
        b".GkkkkkkkkkkkkG.",
        b"..vGv.vGGv.vGv..",
        b".v..v.v..v.v..v.",
        b"v...v.v..v.v...v",
        b"................",
    ]),
    sprite([
        b"................",
        b"................",
        b"................",
        b"................",
        b"....GGGGGGGG....",
        b"..GGggggggggGG..",
        b".GggggGggGgggggG",
        b".GgggGggggGggggG",
        b"GgggGggggggGgggG",
        b"GGGGGGGGGGGGGGGG",
        b"GkEkGkkkkkkGkEkG",
        b".GkkkkkkkkkkkkG.",
        b"..Gv..vGGv..vG..",
        b"..v..v....v..v..",
        b".v...v....v...v.",
        b"................",
    ]),
    sprite([
        b"................",
        b"................",
        b"................",
        b"................",
        b"................",
        b"................",
        b"................",
        b"................",
        b"....GGGGGGGG....",
        b"..GGggGggGggGG..",
        b".GgggggggggggggG",
        b"GgggGggggggGgggG",
        b"GGGGGGGGGGGGGGGG",
        b".GkkkkkkkkkkkkG.",
        b"..kk........kk..",
        b"................",
    ]),
    sprite([
        b"................",
        b".....jjjjjj.....",
        b".....jEEEEj.....",
        b"...jjjjjjjjjj...",
        b"..jhhkkhhkkhhj..",
        b"..jhkkhhkkhhkj..",
        b"..jiiiIIIIiiij..",
        b"..jiiIjiijIiij..",
        b"..jiIjIiiIjIij..",
        b"..jiIiiIIiiIij..",
        b"..jiiIjiijIiij..",
        b"..jiiiIIIIiiij..",
        b"..jjjjjjjjjjjj..",
        b".kkk........kkk.",
        b".kRk........kRk.",
        b".kkk........kkk.",
    ]),
    sprite([
        b"................",
        b".....jjjjjj.....",
        b".....jEEEEj.....",
        b"...jjjjjjjjjj...",
        b"..jkhhkkhhkkhj..",
        b"..jhhkkhhkkhhj..",
        b"..jiiiIjjIiiij..",
        b"..jiiIIiiIIiij..",
        b"..jijIiIIiIjij..",
        b"..jiIjIiiIjIij..",
        b"..jiiIIiiIIiij..",
        b"..jiiiIjjIiiij..",
        b"..jjjjjjjjjjjj..",
        b".kkk........kkk.",
        b".kRk........kRk.",
        b".kkk........kkk.",
    ]),
    sprite([
        b"................",
        b".....jjjjjj.....",
        b".....jkkkkj.....",
        b"...jjjjjjjjjj...",
        b"..jhhkkhhkkhhj..",
        b"..jhkkhhkkhhkj..",
        b"..jiiiIIIIiiij..",
        b"..jiiIjiijIiij..",
        b"..jiIjIiiIjIij..",
        b"..jiIiiIIiiIij..",
        b"..jiiIjiijIiij..",
        b"..jiiiIIIIiiij..",
        b"..jjjjjjjjjjjj..",
        b".kkk........kkk.",
        b".kRk........kRk.",
        b".kkk........kkk.",
    ]),
];
