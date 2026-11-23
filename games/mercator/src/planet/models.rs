//! Step G3: the models below (SPEC 3.10, "Models"), original shapes made
//! while compiling as the ships are. Corners in 1/256 m, drawn at scale
//! 256; +z is ahead, +y up, the origin on the ground under the middle.
//!
//! | Model | Corners | Faces | What |
//! |---|---|---|---|
//! | [`PILOT`] (x3 poses) | 40 | 60 | white suit, orange chest stripe, glass visor, dark jet pack; legs swing by pose |
//! | [`SEAMS`] (x4 stages) | 13 | 18 | a faceted mound of the seam's good in rock; each stage pushes it open, the last lies flat |
//! | [`CHUNK`] | 4 | 4 | a tonne of the good, tumbling |
//! | [`WRECK`] | 16 | 20 | a broken hull wedge, one fin, one wing on the ground |
//! | [`PAD`] | 8 | 12 | a low dark slab, 16 m square |
//!
//! The boulder is the space rock (`models::ROCK`) drawn in the rock's
//! colour. The materials below are space-only ones the planet's palette
//! recolours (`palette::below`): see [`DEPOSIT`], [`ROCK`], [`SUIT`],
//! [`STRIPE`].

use crate::art::mat;
use crate::fixed::{ccos, csin};
use crate::models::{box_corners, cuboid, rf};
use crate::poly::{Model, RawFace, normals, orient, radius_of};

/// The seam's good (copper, ice or pods, by look).
pub const DEPOSIT: u8 = mat::PLANET_A;
/// Rock (seams, boulders), by look.
pub const ROCK: u8 = mat::PLANET_B;
/// The suit's white.
pub const SUIT: u8 = mat::PLANET_C;
/// The suit's orange stripe.
pub const STRIPE: u8 = mat::MOUTH;
const PACK: u8 = mat::GUNMETAL;
const VISOR: u8 = mat::GLASS;

macro_rules! model {
    ($name:ident, $verts:expr, $raw:expr) => {
        pub static $name: Model = Model {
            verts: &$verts,
            faces: &orient(&$verts, &$raw),
            normals: &normals(&$verts, &orient(&$verts, &$raw)),
            radius: radius_of(&$verts),
        };
    };
}

// ---- The pilot ----------------------------------------------------------------

/// A leg: a box from the hip (y 230, z 0) to the foot (y 0, z `foot`).
const fn leg(v: &mut [[i16; 3]], at: usize, x0: i16, x1: i16, foot: i16) {
    let mut k = 0;
    while k < 8 {
        let top = k & 2 != 0;
        let base = if top { 0 } else { foot };
        v[at + k] = [
            if k & 1 == 0 { x0 } else { x1 },
            if top { 230 } else { 0 },
            base + if k & 4 == 0 { -20 } else { 20 },
        ];
        k += 1;
    }
}

const fn pilot_v(stride: i16) -> [[i16; 3]; 40] {
    let mut v = [[0i16; 3]; 40];
    leg(&mut v, 0, -50, -10, stride);
    leg(&mut v, 8, 10, 50, -stride);
    box_corners(&mut v, 16, [-60, 230, -35], [60, 400, 35]);
    box_corners(&mut v, 24, [-36, 400, -34], [36, 466, 38]);
    box_corners(&mut v, 32, [-46, 250, -82], [46, 390, -35]);
    v
}

const PILOT_RAW: [RawFace; 60] = {
    let mut f = [rf(0, 0, 0, 0, 0); 60];
    cuboid(&mut f, 0, 0, [SUIT, SUIT, SUIT, SUIT], 0);
    cuboid(&mut f, 12, 8, [SUIT, SUIT, SUIT, SUIT], 1);
    cuboid(&mut f, 24, 16, [SUIT, SUIT, STRIPE, PACK], 2);
    cuboid(&mut f, 36, 24, [SUIT, SUIT, VISOR, SUIT], 3);
    cuboid(&mut f, 48, 32, [PACK, PACK, PACK, STRIPE], 4);
    f
};

const PILOT_STAND_V: [[i16; 3]; 40] = pilot_v(0);
const PILOT_A_V: [[i16; 3]; 40] = pilot_v(64);
const PILOT_B_V: [[i16; 3]; 40] = pilot_v(-64);
model!(PILOT_STAND, PILOT_STAND_V, PILOT_RAW);
model!(PILOT_A, PILOT_A_V, PILOT_RAW);
model!(PILOT_B, PILOT_B_V, PILOT_RAW);
/// The pilot standing, then the two walk poses.
pub static PILOT: [&Model; 3] = [&PILOT_STAND, &PILOT_A, &PILOT_B];

// ---- Seams ----------------------------------------------------------------------

/// A seam at stage `s` (0..=2 whole and cracking, 3 burst): a top, a
/// ring of six at mid height and a ring of six at the ground. Each
/// stage pushes every other mid corner out and the top down.
const fn seam_v(s: i32) -> [[i16; 3]; 13] {
    let mut v = [[0i16; 3]; 13];
    let flat = s >= 3;
    v[0] = [0, if flat { 50 } else { (200 - 25 * s) as i16 }, 0];
    let mut i = 0;
    while i < 6 {
        let a = (i as u32 * 65536 / 6) as u16;
        let (sn, cs) = (csin(a), ccos(a));
        let mid = if flat {
            210
        } else {
            130 + 20 * s + if i % 2 == 0 { 30 * s } else { 0 }
        };
        let y = if flat { 35 } else { 110 + 10 * s };
        v[1 + i] = [
            ((sn * mid) >> 14) as i16,
            y as i16,
            ((cs * mid) >> 14) as i16,
        ];
        let base = 230 + if flat { 40 } else { 0 };
        v[7 + i] = [((sn * base) >> 14) as i16, -10, ((cs * base) >> 14) as i16];
        i += 1;
    }
    v
}

const SEAM_RAW: [RawFace; 18] = {
    let mut f = [rf(0, 0, 0, 0, 0); 18];
    let mut i = 0;
    while i < 6 {
        let (a, b) = (1 + i as u8, 1 + ((i + 1) % 6) as u8);
        let (lo_a, lo_b) = (a + 6, b + 6);
        let (m1, m2) = if i % 2 == 0 {
            (DEPOSIT, ROCK)
        } else {
            (ROCK, DEPOSIT)
        };
        f[3 * i] = rf(0, a, b, m1, 0);
        f[3 * i + 1] = rf(a, lo_a, lo_b, m2, 0);
        f[3 * i + 2] = rf(a, lo_b, b, m2, 0);
        i += 1;
    }
    f
};

const SEAM0_V: [[i16; 3]; 13] = seam_v(0);
const SEAM1_V: [[i16; 3]; 13] = seam_v(1);
const SEAM2_V: [[i16; 3]; 13] = seam_v(2);
const SEAM3_V: [[i16; 3]; 13] = seam_v(3);
model!(SEAM0, SEAM0_V, SEAM_RAW);
model!(SEAM1, SEAM1_V, SEAM_RAW);
model!(SEAM2, SEAM2_V, SEAM_RAW);
model!(SEAM3, SEAM3_V, SEAM_RAW);
/// A seam by stage (3: burst).
pub static SEAMS: [&Model; 4] = [&SEAM0, &SEAM1, &SEAM2, &SEAM3];

// ---- Chunk, wreck, pad --------------------------------------------------------------

const CHUNK_V: [[i16; 3]; 4] = [[0, 60, 0], [-55, -30, -40], [55, -30, -40], [0, -30, 62]];
const CHUNK_RAW: [RawFace; 4] = [
    rf(0, 1, 2, DEPOSIT, 0),
    rf(0, 2, 3, DEPOSIT, 0),
    rf(0, 3, 1, ROCK, 0),
    rf(1, 2, 3, ROCK, 0),
];
model!(CHUNK, CHUNK_V, CHUNK_RAW);

const WRECK_V: [[i16; 3]; 16] = [
    // The hull: a wedge, its broken end at the back.
    [-330, -20, -700],
    [330, -20, -700],
    [-220, 380, -700],
    [220, 380, -700],
    [-160, -20, 900],
    [160, -20, 900],
    [-80, 180, 900],
    [80, 180, 900],
    // The fin.
    [-25, 370, -650],
    [25, 370, -650],
    [0, 900, -760],
    [0, 370, -220],
    // A wing torn down onto the ground.
    [300, 30, -420],
    [300, 30, 260],
    [980, 0, -320],
    [330, 140, -100],
];
const WRECK_RAW: [RawFace; 20] = {
    use mat::{GUNMETAL, HULL2};
    let mut f = [rf(0, 0, 0, 0, 0); 20];
    cuboid(&mut f, 0, 0, [HULL2, HULL2, GUNMETAL, GUNMETAL], 0);
    f[12] = rf(8, 9, 10, HULL2, 1);
    f[13] = rf(8, 10, 11, HULL2, 1);
    f[14] = rf(9, 11, 10, HULL2, 1);
    f[15] = rf(8, 11, 9, GUNMETAL, 1);
    f[16] = rf(12, 13, 14, GUNMETAL, 2);
    f[17] = rf(12, 14, 15, HULL2, 2);
    f[18] = rf(13, 15, 14, HULL2, 2);
    f[19] = rf(12, 15, 13, GUNMETAL, 2);
    f
};
model!(WRECK, WRECK_V, WRECK_RAW);

const PAD_V: [[i16; 3]; 8] = {
    let mut v = [[0i16; 3]; 8];
    box_corners(&mut v, 0, [-2048, -80, -2048], [2048, 12, 2048]);
    v
};
const PAD_RAW: [RawFace; 12] = {
    let mut f = [rf(0, 0, 0, 0, 0); 12];
    cuboid(&mut f, 0, 0, [mat::GUNMETAL; 4], 0);
    f
};
model!(PAD, PAD_V, PAD_RAW);
/// The pad's half width (1/256 m) for its lights.
pub const PAD_HALF: i32 = 2048;

// ---- Step G4: the drone, the props -------------------------------------------------
//
// | Model | Corners | Faces | What |
// |---|---|---|---|
// | [`DRONE`] | 10 | 11 | a flat diamond, dark above, its stripe colour below, a red eye ahead |
// | [`ROCK_PROP`] | 5 | 4 | a low rock (solid) |
// | [`PLANTS`] (x7 looks) | 5..8 | 4..7 | reeds, scrub, ice spikes, a basalt pillar, a fern, a spore stalk |
//
// One model a kind at every detail level: the drone's 11 faces are
// under SPEC 3.10's MID budget (12), the props' 4 to 7 under its MID
// budgets (rock 6, plant 6; the pillar's 7 is one over, HIGH allows 8).

/// The drone's stripe (space's PANEL, recoloured below).
pub const DRONE_STRIPE: u8 = mat::PANEL;
/// The plants' colour (space's CANISTER, recoloured per look below).
pub const PLANT: u8 = mat::CANISTER;

const DRONE_V: [[i16; 3]; 10] = [
    [0, 90, 0],
    [0, -110, 0],
    [0, 0, 170],
    [150, 0, 0],
    [0, 0, -150],
    [-150, 0, 0],
    // The eye.
    [-35, 12, 160],
    [35, 12, 160],
    [0, -30, 160],
    [0, 0, 215],
];
const DRONE_RAW: [RawFace; 11] = {
    use mat::{GUNMETAL, LIGHT_R};
    let s = DRONE_STRIPE;
    [
        rf(0, 2, 3, GUNMETAL, 0),
        rf(0, 3, 4, GUNMETAL, 0),
        rf(0, 4, 5, GUNMETAL, 0),
        rf(0, 5, 2, GUNMETAL, 0),
        rf(1, 2, 3, s, 0),
        rf(1, 3, 4, s, 0),
        rf(1, 4, 5, s, 0),
        rf(1, 5, 2, s, 0),
        rf(9, 6, 7, LIGHT_R, 1),
        rf(9, 7, 8, LIGHT_R, 1),
        rf(9, 8, 6, LIGHT_R, 1),
    ]
};
model!(DRONE, DRONE_V, DRONE_RAW);

const ROCK_PROP_V: [[i16; 3]; 5] = [
    [-200, -10, -60],
    [60, -10, -200],
    [200, -10, 80],
    [-40, -10, 200],
    [30, 180, 10],
];
const ROCK_PROP_RAW: [RawFace; 4] = [
    rf(4, 0, 1, ROCK, 0),
    rf(4, 1, 2, ROCK, 0),
    rf(4, 2, 3, ROCK, 0),
    rf(4, 3, 0, ROCK, 0),
];
model!(ROCK_PROP, ROCK_PROP_V, ROCK_PROP_RAW);

/// A three-sided open pyramid's faces (base corners `at`..`at + 3`,
/// apex `at + 3`), part `part`, into `f` from `i`.
const fn spike(f: &mut [RawFace], i: usize, at: u8, part: u8) {
    f[i] = rf(at + 3, at, at + 1, PLANT, part);
    f[i + 1] = rf(at + 3, at + 1, at + 2, PLANT, part);
    f[i + 2] = rf(at + 3, at + 2, at, PLANT, part);
}

// Reeds (OCEAN): two thin spikes.
const REED_V: [[i16; 3]; 8] = [
    [-70, 0, -20],
    [-20, 0, -30],
    [-45, 0, 25],
    [-60, 420, -10],
    [30, 0, 0],
    [70, 0, -20],
    [60, 0, 30],
    [80, 330, 10],
];
const TWO_SPIKES: [RawFace; 6] = {
    let mut f = [rf(0, 0, 0, 0, 0); 6];
    spike(&mut f, 0, 0, 0);
    spike(&mut f, 3, 4, 1);
    f
};
model!(REED, REED_V, TWO_SPIKES);

// Scrub (DESERT): a low dome of four faces.
const SCRUB_V: [[i16; 3]; 5] = [
    [-170, 0, 0],
    [0, 0, -150],
    [160, 0, 10],
    [0, 0, 170],
    [10, 140, 0],
];
const SCRUB_RAW: [RawFace; 4] = [
    rf(4, 0, 1, PLANT, 0),
    rf(4, 1, 2, PLANT, 0),
    rf(4, 2, 3, PLANT, 0),
    rf(4, 3, 0, PLANT, 0),
];
model!(SCRUB, SCRUB_V, SCRUB_RAW);

// Ice spikes (ICE): a tall one and a leaning one.
const ICE_V: [[i16; 3]; 8] = [
    [-60, 0, -40],
    [60, 0, -50],
    [0, 0, 70],
    [20, 560, 10],
    [70, 0, 40],
    [140, 0, 30],
    [110, 0, 110],
    [180, 300, 90],
];
model!(ICE_SPIKE, ICE_V, TWO_SPIKES);

// A basalt pillar (LAVA, RUST): a three-sided column with a top.
const PILLAR_V: [[i16; 3]; 6] = [
    [-150, 0, -90],
    [150, 0, -90],
    [0, 0, 170],
    [-130, 640, -75],
    [130, 640, -75],
    [0, 640, 150],
];
const PILLAR_RAW: [RawFace; 7] = [
    rf(0, 1, 4, PLANT, 0),
    rf(0, 4, 3, PLANT, 0),
    rf(1, 2, 5, PLANT, 0),
    rf(1, 5, 4, PLANT, 0),
    rf(2, 0, 3, PLANT, 0),
    rf(2, 3, 5, PLANT, 0),
    rf(3, 4, 5, PLANT, 0),
];
model!(PILLAR, PILLAR_V, PILLAR_RAW);

// A fern (JUNGLE): three fronds spread from one root.
const FERN_V: [[i16; 3]; 5] = [
    [0, 0, 0],
    [250, 220, 0],
    [-125, 220, 216],
    [-125, 220, -216],
    [0, 170, 0],
];
const FERN_RAW: [RawFace; 6] = [
    rf(0, 1, 2, PLANT, 0),
    rf(0, 2, 3, PLANT, 0),
    rf(0, 3, 1, PLANT, 0),
    rf(4, 1, 2, PLANT, 0),
    rf(4, 2, 3, PLANT, 0),
    rf(4, 3, 1, PLANT, 0),
];
model!(FERN, FERN_V, FERN_RAW);

// A spore stalk (HAZE): a long point up to a cap.
const SPORE_V: [[i16; 3]; 5] = [
    [0, 0, 0],
    [130, 330, 0],
    [-65, 330, 113],
    [-65, 330, -113],
    [0, 470, 0],
];
model!(SPORE, SPORE_V, FERN_RAW);

/// The plant per look (`art::LOOK_NAMES` order; GIANT has no ground).
pub static PLANTS: [&Model; 8] = [
    &REED, &SCRUB, &ICE_SPIKE, &PILLAR, &SCRUB, &FERN, &SPORE, &PILLAR,
];
