//! The palette and the pictures (sprites), all made while compiling.
//!
//! **Palette.** 48 shared colours (screens, karts, roadside things),
//! then one block of 48 for each course: its sky, its mountains and its
//! ground. The ground colours come in three steps of haze (near, middle,
//! far), so the far road fades into the evening air. The palette is set
//! once when the game starts and never changes: on this console a
//! palette change sends the whole screen.
//!
//! **Sprites** are tables of letters, one letter per pixel, turned into
//! palette numbers by [`pic`] while compiling. `.` is see-through.

use game_kit::rgb565;

// ---- Shared colours --------------------------------------------------------------

pub const BLACK: u8 = 0;
pub const UI_BG: u8 = 1;
pub const UI_EDGE: u8 = 2;
pub const TEXT: u8 = 3;
pub const TEXT_DIM: u8 = 4;
pub const ACCENT: u8 = 5;
pub const GOOD: u8 = 6;
pub const BAD: u8 = 7;
pub const CHARGE: u8 = 8;
pub const CHARGE_DIM: u8 = 9;
pub const TYRE: u8 = 10;
pub const TYRE_HI: u8 = 11;
pub const SEAT: u8 = 12;
pub const METAL: u8 = 13;
pub const METAL_DARK: u8 = 14;
pub const WHITE: u8 = 15;
/// Kart bodies: kart `k` has its body at `KART + 2k` and its dark side
/// at `KART + 2k + 1`. Kart 0 is the player's.
pub const KART: u8 = 16;
pub const TAIL: u8 = 26;
pub const TAIL_ON: u8 = 27;
pub const GLOW: u8 = 28;
pub const PINE: u8 = 29;
pub const PINE_DARK: u8 = 30;
pub const TRUNK: u8 = 31;
pub const BIRCH: u8 = 32;
pub const LEAVES: u8 = 33;
pub const LEAVES_DARK: u8 = 34;
pub const ROCK: u8 = 35;
pub const ROCK_DARK: u8 = 36;
pub const SIGN: u8 = 37;
pub const SIGN_INK: u8 = 38;
pub const ORANGE: u8 = 39;
pub const SNOW: u8 = 40;
pub const SNOW_SHADE: u8 = 41;
pub const BUSH: u8 = 42;
pub const BUSH_DARK: u8 = 43;
pub const BANNER: u8 = 44;
pub const MAST_RED: u8 = 45;
pub const SHADOW: u8 = 46;
pub const UI_HI: u8 = 47;
pub const SHARED: usize = 48;

const SHARED_RGB: [(u32, u32, u32); SHARED] = [
    (0, 0, 0),       // BLACK
    (22, 20, 34),    // UI_BG
    (78, 70, 110),   // UI_EDGE
    (244, 240, 228), // TEXT
    (150, 146, 166), // TEXT_DIM
    (255, 178, 64),  // ACCENT
    (120, 226, 120), // GOOD
    (236, 84, 72),   // BAD
    (96, 210, 255),  // CHARGE
    (34, 70, 100),   // CHARGE_DIM
    (26, 24, 30),    // TYRE
    (78, 74, 82),    // TYRE_HI
    (44, 42, 52),    // SEAT
    (176, 176, 186), // METAL
    (92, 92, 104),   // METAL_DARK
    (246, 246, 240), // WHITE
    (250, 196, 40),  // kart 0 (you): sun yellow
    (172, 116, 18),
    (226, 64, 64), // kart 1: red
    (132, 30, 40),
    (40, 196, 172), // kart 2: teal
    (18, 112, 100),
    (166, 104, 234), // kart 3: violet
    (92, 52, 146),
    (232, 232, 238), // kart 4: white
    (136, 136, 150),
    (140, 30, 30),   // TAIL
    (255, 70, 60),   // TAIL_ON
    (255, 240, 170), // GLOW
    (36, 92, 58),    // PINE
    (18, 54, 36),    // PINE_DARK
    (96, 64, 42),    // TRUNK
    (228, 228, 218), // BIRCH
    (162, 178, 70),  // LEAVES
    (104, 128, 50),  // LEAVES_DARK
    (128, 120, 112), // ROCK
    (76, 70, 68),    // ROCK_DARK
    (252, 204, 40),  // SIGN
    (26, 24, 26),    // SIGN_INK
    (252, 124, 30),  // ORANGE
    (238, 244, 252), // SNOW
    (176, 188, 216), // SNOW_SHADE
    (64, 108, 52),   // BUSH
    (40, 74, 36),    // BUSH_DARK
    (44, 64, 150),   // BANNER
    (206, 44, 44),   // MAST_RED
    (24, 24, 30),    // SHADOW
    (70, 60, 120),   // UI_HI
];

// ---- Course colours ----------------------------------------------------------------

/// The first palette number of course `c`'s block.
pub const fn course_base(c: usize) -> u8 {
    (SHARED + c * COURSE_COLOURS) as u8
}
pub const COURSE_COLOURS: usize = 48;
/// Sky steps, from the top of the view down to the horizon.
pub const SKY_STEPS: usize = 14;
pub const SUN: u8 = 14;
pub const STAR: u8 = 15;
pub const FAR: u8 = 16;
pub const FAR_LIT: u8 = 17;
pub const NEAR: u8 = 18;
pub const NEAR_DARK: u8 = 19;
pub const FAR_LAND: u8 = 20;
/// The ground: material `m` at haze `h` is at `GROUND + 3m + h`.
pub const GROUND: u8 = 21;
pub const HAZES: usize = 3;
pub const GRASS_A: u8 = 0;
pub const GRASS_B: u8 = 1;
pub const ROAD_A: u8 = 2;
pub const ROAD_B: u8 = 3;
pub const RUMBLE_A: u8 = 4;
pub const RUMBLE_B: u8 = 5;
pub const LINE: u8 = 6;
/// The other ground beside the road: water, dirt, gravel.
pub const SIDE: u8 = 7;
pub const MATS: usize = 8;

/// One course's colours, as (red, green, blue).
struct Look {
    sky_top: (u32, u32, u32),
    sky_low: (u32, u32, u32),
    sun: (u32, u32, u32),
    star: (u32, u32, u32),
    far: (u32, u32, u32),
    far_lit: (u32, u32, u32),
    near: (u32, u32, u32),
    near_dark: (u32, u32, u32),
    far_land: (u32, u32, u32),
    haze: (u32, u32, u32),
    ground: [(u32, u32, u32); MATS],
}

const LOOKS: [Look; 3] = [
    // RESERVOIR: an orange dusk over a lake.
    Look {
        sky_top: (38, 42, 96),
        sky_low: (252, 156, 92),
        sun: (255, 224, 150),
        star: (255, 236, 200),
        far: (96, 72, 112),
        far_lit: (160, 104, 124),
        near: (58, 72, 70),
        near_dark: (40, 52, 54),
        far_land: (92, 92, 76),
        haze: (226, 150, 112),
        ground: [
            (92, 132, 58),   // grass
            (80, 118, 50),   // grass, second band
            (98, 96, 104),   // road
            (90, 88, 96),    // road, second band
            (206, 52, 50),   // kerb red
            (238, 236, 230), // kerb white
            (242, 240, 228), // line
            (58, 104, 156),  // water
        ],
    },
    // PINEWOOD: a violet dusk in a pine forest.
    Look {
        sky_top: (28, 18, 60),
        sky_low: (206, 112, 152),
        sun: (242, 232, 214),
        star: (250, 240, 255),
        far: (62, 42, 94),
        far_lit: (104, 72, 134),
        near: (30, 56, 54),
        near_dark: (20, 38, 40),
        far_land: (52, 66, 60),
        haze: (168, 108, 150),
        ground: [
            (46, 96, 54),
            (38, 84, 46),
            (86, 82, 92),
            (78, 74, 86),
            (234, 200, 40),  // kerb yellow
            (40, 40, 44),    // kerb black
            (244, 224, 126), // line
            (96, 74, 52),    // dirt
        ],
    },
    // SUMMIT: blue night falling over snow.
    Look {
        sky_top: (8, 10, 40),
        sky_low: (92, 112, 182),
        sun: (230, 240, 255),
        star: (255, 255, 255),
        far: (52, 62, 104),
        far_lit: (222, 232, 252),
        near: (72, 84, 124),
        near_dark: (44, 54, 92),
        far_land: (150, 160, 192),
        haze: (112, 132, 190),
        ground: [
            (212, 220, 238), // snow
            (192, 202, 228), // snow, second band
            (72, 74, 88),
            (64, 66, 80),
            (222, 58, 42),
            (242, 242, 244),
            (244, 244, 244),
            (124, 124, 140), // gravel
        ],
    },
];

/// How much of the haze colour each haze step takes, in 1/256.
const HAZE_MIX: [u32; HAZES] = [0, 76, 150];

const fn mix(a: (u32, u32, u32), b: (u32, u32, u32), k: u32) -> (u32, u32, u32) {
    (
        (a.0 * (256 - k) + b.0 * k) / 256,
        (a.1 * (256 - k) + b.1 * k) / 256,
        (a.2 * (256 - k) + b.2 * k) / 256,
    )
}

const fn c565(c: (u32, u32, u32)) -> u16 {
    rgb565(c.0 as u8, c.1 as u8, c.2 as u8)
}

/// Every colour the game sets, from palette number 0.
pub const COLOUR_COUNT: usize = SHARED + 3 * COURSE_COLOURS;
pub static COLOURS: [u16; COLOUR_COUNT] = colours();

const fn colours() -> [u16; COLOUR_COUNT] {
    let mut out = [0u16; COLOUR_COUNT];
    let mut i = 0;
    while i < SHARED {
        out[i] = c565(SHARED_RGB[i]);
        i += 1;
    }
    let mut c = 0;
    while c < 3 {
        let l = &LOOKS[c];
        let b = course_base(c) as usize;
        let mut s = 0;
        while s < SKY_STEPS {
            // Darker longer at the top, then quickly warmer near the
            // horizon: the mix grows with the square of the step.
            let k = (s * s * 256 / ((SKY_STEPS - 1) * (SKY_STEPS - 1))) as u32;
            out[b + s] = c565(mix(l.sky_top, l.sky_low, k));
            s += 1;
        }
        out[b + SUN as usize] = c565(l.sun);
        out[b + STAR as usize] = c565(l.star);
        out[b + FAR as usize] = c565(l.far);
        out[b + FAR_LIT as usize] = c565(l.far_lit);
        out[b + NEAR as usize] = c565(l.near);
        out[b + NEAR_DARK as usize] = c565(l.near_dark);
        out[b + FAR_LAND as usize] = c565(l.far_land);
        let mut m = 0;
        while m < MATS {
            let mut h = 0;
            while h < HAZES {
                out[b + GROUND as usize + m * HAZES + h] =
                    c565(mix(l.ground[m], l.haze, HAZE_MIX[h]));
                h += 1;
            }
            m += 1;
        }
        c += 1;
    }
    out
}

/// Palette number of ground material `m` at haze `h` on course `c`.
pub const fn ground(c: usize, m: u8, h: usize) -> u8 {
    course_base(c) + GROUND + m * HAZES as u8 + h as u8
}

// ---- Sprites -------------------------------------------------------------------------

/// A sprite's see-through pixels.
pub const CLEAR: u8 = 255;

/// A picture made of palette numbers ([`CLEAR`]: not drawn).
#[derive(Clone, Copy)]
pub struct Pic {
    pub w: u8,
    pub h: u8,
    pub px: &'static [u8],
}

/// The palette number of a letter in the tables below.
const fn letter(b: u8) -> u8 {
    match b {
        b'.' => CLEAR,
        b'k' => BLACK,
        b'w' => WHITE,
        b't' => TYRE,
        b'T' => TYRE_HI,
        b's' => SEAT,
        b'm' => METAL,
        b'M' => METAL_DARK,
        b'b' => KART,
        b'd' => KART + 1,
        b'r' => TAIL,
        b'y' => GLOW,
        b'g' => PINE,
        b'G' => PINE_DARK,
        b'n' => TRUNK,
        b'i' => BIRCH,
        b'l' => LEAVES,
        b'L' => LEAVES_DARK,
        b'o' => ROCK,
        b'O' => ROCK_DARK,
        b'Y' => SIGN,
        b'K' => SIGN_INK,
        b'a' => ORANGE,
        b'e' => SNOW,
        b'E' => SNOW_SHADE,
        b'u' => BUSH,
        b'U' => BUSH_DARK,
        b'R' => MAST_RED,
        b'c' => CHARGE,
        _ => panic!("unknown letter in a sprite"),
    }
}

/// Letter rows into palette numbers. Every row must be `W` letters
/// long and there must be `H` rows (checked while compiling).
pub const fn pic<const W: usize, const H: usize, const N: usize>(rows: [&str; H]) -> [u8; N] {
    assert!(W * H == N, "sprite size");
    let mut out = [CLEAR; N];
    let mut y = 0;
    while y < H {
        let r = rows[y].as_bytes();
        assert!(r.len() == W, "sprite row length");
        let mut x = 0;
        while x < W {
            out[y * W + x] = letter(r[x]);
            x += 1;
        }
        y += 1;
    }
    out
}

/// A kart from behind, 32 x 16. `b`/`d` are the body colours (changed
/// per kart), `r` the tail lights (brighter while braking).
static KART_PX: [u8; 32 * 16] = pic::<32, 16, { 32 * 16 }>([
    "..........wwwwwwwwwwww..........",
    ".........wwwwbbbbbbwwww.........",
    ".........wwwwbbbbbbwwww.........",
    "..........wwwwwwwwwwww..........",
    "...........kkkkkkkkkk...........",
    "..........ssssssssssss..........",
    "...bbbbbbbbbbbbbbbbbbbbbbbbbb...",
    "...dddddddddddddddddddddddddd...",
    "tttttt...mmssssssssssmm...tttttt",
    "tTTTTt...dbbbbbbbbbbbbd...tTTTTt",
    "tttttt..mdbrrbbccbbrrbdm..tttttt",
    "tTTTTtmmmdbrrbbccbbrrbdmmmtTTTTt",
    "tttttt...dbbbbbbbbbbbbd...tttttt",
    "tTTTTt...dddddddddddddd...tTTTTt",
    "tttttt....kkkkkkkkkkkk....tttttt",
    ".tttt......................tttt.",
]);
pub const KART_PIC: Pic = Pic {
    w: 32,
    h: 16,
    px: &KART_PX,
};
/// Rows of the kart that lean with the steering (driver and seat).
pub const KART_LEAN_ROWS: u8 = 6;

/// A birch: white trunk with dark marks, light crown. 12 x 30.
static BIRCH_PX: [u8; 12 * 30] = pic::<12, 30, { 12 * 30 }>([
    "....llll....",
    "..llllllL...",
    ".lllllllLL..",
    ".llllllLLLL.",
    "lllllllllLLL",
    "llLlllllLLLL",
    "lllllLllLLLL",
    ".lllllllLLL.",
    "llllllLlLLLL",
    "lLlllllLLLLL",
    ".llllilLLLL.",
    "..lllilLLL..",
    "...llilL....",
    ".....ii.....",
    ".....ik.....",
    ".....ii.....",
    ".....ii.....",
    ".....ki.....",
    ".....ii.....",
    ".....ii.....",
    ".....ik.....",
    ".....ii.....",
    ".....ii.....",
    ".....ki.....",
    ".....ii.....",
    ".....ii.....",
    ".....ik.....",
    ".....ii.....",
    ".....ii.....",
    "....iiii....",
]);
pub const BIRCH_PIC: Pic = Pic {
    w: 12,
    h: 30,
    px: &BIRCH_PX,
};

/// A pine, made by rule: four tiers, the right side in shade. 18 x 34.
static PINE_PX: [u8; 18 * 34] = pine();
pub const PINE_PIC: Pic = Pic {
    w: 18,
    h: 34,
    px: &PINE_PX,
};

const fn pine() -> [u8; 18 * 34] {
    let mut out = [CLEAR; 18 * 34];
    let mut y = 0;
    while y < 34 {
        let mut x = 0;
        while x < 18 {
            let c: i32 = 9;
            let dx = x as i32 - c;
            let p = if y >= 30 {
                if dx >= -1 && dx <= 0 { TRUNK } else { CLEAR }
            } else {
                let tier = (y / 8) as i32;
                let t = (y % 8) as i32;
                let mut half = 1 + tier + t * (tier + 3) / 7;
                if half > 8 {
                    half = 8;
                }
                // A ragged edge: the outermost pixel only on some rows.
                let edge = dx == -half || dx == half - 1;
                let ragged = edge && (y + x) % 3 == 0;
                if dx >= -half && dx < half && !ragged {
                    if dx >= 1 || (dx == 0 && t % 2 == 1) {
                        PINE_DARK
                    } else {
                        PINE
                    }
                } else {
                    CLEAR
                }
            };
            out[y * 18 + x] = p;
            x += 1;
        }
        y += 1;
    }
    out
}

/// A rock, made by rule: a flat oval with its lower right in shade.
static ROCK_PX: [u8; 16 * 10] = rock();
pub const ROCK_PIC: Pic = Pic {
    w: 16,
    h: 10,
    px: &ROCK_PX,
};

const fn rock() -> [u8; 16 * 10] {
    let mut out = [CLEAR; 16 * 10];
    let mut y = 0;
    while y < 10 {
        let mut x = 0;
        while x < 16 {
            // Inside the oval (x - 7.5)^2 / 8^2 + (y - 10)^2 / 10^2 < 1,
            // in whole numbers (times 4).
            let dx = 2 * x as i32 - 15;
            let dy = 2 * y as i32 - 19;
            let inside = dx * dx * 100 + dy * dy * 64 < 6400 * 4;
            if inside {
                out[y * 16 + x] = if dx + dy > 6 { ROCK_DARK } else { ROCK };
            }
            x += 1;
        }
        y += 1;
    }
    out
}

/// A low bush (the richest detail only). 12 x 6.
static BUSH_PX: [u8; 12 * 6] = pic::<12, 6, { 12 * 6 }>([
    "...uuuu.....",
    ".uuuuuuuU...",
    "uuuuuuuuUUU.",
    "uuuuuuuUUUUU",
    "uuuuuuUUUUUU",
    ".UUUUUUUUUU.",
]);
pub const BUSH_PIC: Pic = Pic {
    w: 12,
    h: 6,
    px: &BUSH_PX,
};

/// A white roadside post with an orange reflector. 4 x 12.
static POST_PX: [u8; 4 * 12] = pic::<4, 12, { 4 * 12 }>([
    "kkkk", "kaak", "kaak", "wwww", "wwww", "wwww", "wwww", "wwww", "wwww", "wwww", "wwww", "MMMM",
]);
pub const POST_PIC: Pic = Pic {
    w: 4,
    h: 12,
    px: &POST_PX,
};

/// A snow pole: orange and white bands. 2 x 24.
static POLE_PX: [u8; 2 * 24] = pic::<2, 24, { 2 * 24 }>([
    "aa", "aa", "aa", "aa", "ww", "ww", "ww", "ww", "aa", "aa", "aa", "aa", "ww", "ww", "ww", "ww",
    "aa", "aa", "aa", "aa", "ww", "ww", "ww", "ww",
]);
pub const POLE_PIC: Pic = Pic {
    w: 2,
    h: 24,
    px: &POLE_PX,
};

/// A street lamp, its head lit. 10 x 34.
static LAMP_PX: [u8; 10 * 34] = lamp();
pub const LAMP_PIC: Pic = Pic {
    w: 10,
    h: 34,
    px: &LAMP_PX,
};

const fn lamp() -> [u8; 10 * 34] {
    let mut out = [CLEAR; 10 * 34];
    let mut y = 0;
    while y < 34 {
        let mut x = 0;
        while x < 10 {
            let p = if (y == 1 || y == 2) && x >= 1 {
                METAL_DARK
            } else if (y >= 3 && y <= 4 && x >= 6) || (y == 5 && x >= 7 && x <= 8) {
                GLOW
            } else if y >= 1 && (x == 1 || x == 2) {
                if x == 1 { METAL } else { METAL_DARK }
            } else {
                CLEAR
            };
            out[y * 10 + x] = p;
            x += 1;
        }
        y += 1;
    }
    out
}

/// A bend sign: two black arrows on yellow, pointing right (drawn
/// mirrored for a left bend). 14 x 12.
static CHEVRON_PX: [u8; 14 * 12] = pic::<14, 12, { 14 * 12 }>([
    "KKKKKKKKKKKKKK",
    "KYKKYYYKKYYYYK",
    "KYYKKYYYKKYYYK",
    "KYYYKKYYYKKYYK",
    "KYYYYKKYYYKKYK",
    "KYYYKKYYYKKYYK",
    "KYYKKYYYKKYYYK",
    "KYKKYYYKKYYYYK",
    "KKKKKKKKKKKKKK",
    "......MM......",
    "......MM......",
    "......MM......",
]);
pub const CHEVRON_PIC: Pic = Pic {
    w: 14,
    h: 12,
    px: &CHEVRON_PX,
};

/// A short railing post by the water. 3 x 7.
static RAIL_PX: [u8; 3 * 7] =
    pic::<3, 7, { 3 * 7 }>(["mmm", ".m.", ".m.", "mmm", ".M.", ".M.", ".M."]);
pub const RAIL_PIC: Pic = Pic {
    w: 3,
    h: 7,
    px: &RAIL_PX,
};

/// A radio mast near the summit: red and white lattice. 8 x 40.
static MAST_PX: [u8; 8 * 40] = mast();
pub const MAST_PIC: Pic = Pic {
    w: 8,
    h: 40,
    px: &MAST_PX,
};

const fn mast() -> [u8; 8 * 40] {
    let mut out = [CLEAR; 8 * 40];
    let mut y = 0;
    while y < 40 {
        // Wider at the foot: half width 1 at the top to 4 at the bottom.
        let half = 1 + (y * 3 / 39) as i32;
        let band = if (y / 5) % 2 == 0 { MAST_RED } else { WHITE };
        let mut x = 0;
        while x < 8 {
            let dx = x as i32 - 4;
            let edge = dx == -half || dx == half - 1;
            let cross = (y % 5 == 0) && dx >= -half && dx < half;
            if y == 0 && (dx == -1 || dx == 0) {
                out[y * 8 + x] = TAIL_ON;
            } else if edge || cross {
                out[y * 8 + x] = band;
            }
            x += 1;
        }
        y += 1;
    }
    out
}

// ---- The far mountains ----------------------------------------------------------------

/// Heights of the far ridge and the near hills, one per column of a
/// 256-pixel-wide band that repeats as the view turns.
pub static RIDGE: [u8; 256] = ridge(17, 3, 7);
pub static HILLS: [u8; 256] = ridge(5, 11, 3);

/// A ridge made of a few overlapping triangle waves, in whole numbers.
const fn ridge(seed: u32, a: u32, b: u32) -> [u8; 256] {
    let mut out = [0u8; 256];
    let mut x = 0;
    while x < 256 {
        let xi = x as u32;
        let h = tri(xi + seed * 5, 256, 10)
            + tri(xi * a + seed, 128, 6)
            + tri(xi * b + seed * 3, 64, 3);
        out[x] = h as u8;
        x += 1;
    }
    out
}

/// A triangle wave: 0 up to `amp` and back down, once per `period`.
const fn tri(p: u32, period: u32, amp: u32) -> u32 {
    let q = p % period;
    let h = if q < period / 2 { q } else { period - q };
    h * amp * 2 / period
}
