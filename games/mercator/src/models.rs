//! The 3D models, as small tables made while compiling. Every face is
//! turned to look outwards from the middle of its convex part
//! ([`orient`]), so the tables need no care about the order of corners.
//! Models look along +z (their nose), +x to their right, +y up.
//!
//! Ships and the station are written in 1/16 m and drawn at size
//! [`SHIP_SCALE`]; the spheres have radius [`BALL`] and are drawn at the
//! size of the body they stand for.
//!
//! | Model | Corners | Faces | What |
//! |---|---|---|---|
//! | [`COURIER`] | 12 | 16 | the player's first ship, the LARK: a wedge on a wide flat wing |
//! | [`DRAY`] | 25 | 30 | the cargo ship: a tall box with a glass-topped snout, stub wings and a wide glowing engine framed in the box's back |
//! | [`SHRIKE`] | 22 | 20 | the combat ship: a dark diamond dart with four fins in an X |
//! | [`RAIDER`] | 8 | 12 | a pirate: a red three-pointed claw round a dark nose |
//! | [`HAULER`] | 16 | 24 | a trader: a big gold cargo block pushed by a small cab |
//! | [`WARDEN`] | 9 | 9 | a patrol: a white and blue lozenge with a tall fin |
//! | [`HOOK`] | 12 | 16 | a pirate's second hull: a diamond between two forward-swept blades |
//! | [`TANKER`] | 16 | 20 | a trader's second hull: two three-sided tanks and a low wedge cab |
//! | [`CUTTER`] | 13 | 14 | a patrol's second hull: a long arrowhead with two fins leaning out |
//! | [`CANISTER`] | 8 | 12 | a loose cargo canister |
//! | [`ROCK`] | 9 | 14 | a drifting rock: a lopsided lump of grey facets |
//! | [`TAG_BOX`] | 8 | 12 | a raider's TAG: the canister in one colour, so its blink shows from any side |
//! | [`SHARD`] | 4 | 4 | a piece of a wreck (its outside in the wreck's colour) |
//! | [`STATION`] | 40 | 64 | the station: a long hub, two panel wings, a lit mouth |
//! | [`PLANET_HI`] | 62 | 120 | a planet, 6 bands of 12 |
//! | [`PLANET_LO`] | 34 | 64 | a planet, 5 bands of 8 |
//! | [`SUN_BALL`] | 26 | 48 | the sun, 4 bands of 8 |

use crate::art::mat;
use crate::fixed::{ccos, csin, hash};
use crate::poly::{Model, RawFace, normals, orient, radius_of};

/// The size ships and the station are drawn at (their tables are in
/// 1/16 m).
pub const SHIP_SCALE: i32 = 4096;
/// The spheres' radius in their tables.
pub const BALL: i32 = 16_000;

pub(crate) const fn rf(a: u8, b: u8, c: u8, mat: u8, part: u8) -> RawFace {
    RawFace {
        v: [a, b, c],
        mat,
        part,
    }
}

/// Builds a model's statics from a corner table and raw faces.
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

// ---- Ships ------------------------------------------------------------------------

const COURIER_V: [[i16; 3]; 12] = [
    // Body: nose, spine, four rear corners.
    [0, 0, 260],
    [0, 56, -60],
    [-60, 44, -250],
    [60, 44, -250],
    [-60, -36, -250],
    [60, -36, -250],
    // Wing: front, left tip, right tip; top then bottom.
    [0, -4, 60],
    [-330, -4, -230],
    [330, -4, -230],
    [0, -16, 60],
    [-330, -16, -230],
    [330, -16, -230],
];

const COURIER_RAW: [RawFace; 16] = {
    use mat::*;
    [
        rf(0, 1, 2, GLASS, 0),
        rf(0, 1, 3, GLASS, 0),
        rf(1, 2, 3, HULL, 0),
        rf(0, 2, 4, HULL2, 0),
        rf(0, 3, 5, HULL2, 0),
        rf(0, 4, 5, HULL2, 0),
        rf(2, 3, 5, ENGINE, 0),
        rf(2, 5, 4, ENGINE, 0),
        rf(6, 7, 8, HULL, 1),
        rf(9, 10, 11, HULL2, 1),
        rf(6, 7, 10, HULL2, 1),
        rf(6, 10, 9, HULL2, 1),
        rf(6, 8, 11, HULL2, 1),
        rf(6, 11, 9, HULL2, 1),
        rf(7, 8, 11, GUNMETAL, 1),
        rf(7, 11, 10, GUNMETAL, 1),
    ]
};
model!(COURIER, COURIER_V, COURIER_RAW);

// Step M5: the ships to buy. Each is drawn at ME_X like the courier and
// seen from behind and above in the chase view, so each has its own
// outline from there: the LARK a flat wide wing, the DRAY a tall box,
// the SHRIKE an X of fins. Faces that would lie inside another part
// (a wing's or fin's root, the snout's base) are left out: there is no
// depth buffer, so a hidden face could be painted over the hull.

const DRAY_V: [[i16; 3]; 25] = [
    // The cargo box (x low/high in bit 0, y in bit 1, z in bit 2).
    [-150, -110, -300],
    [150, -110, -300],
    [-150, 100, -300],
    [150, 100, -300],
    [-150, -110, 120],
    [150, -110, 120],
    [-150, 100, 120],
    [150, 100, 120],
    // The engine on the back: its lower left corner (the other three
    // are the last corners).
    [-110, -70, -300],
    // The snout: its base on the box's front, its point.
    [-110, -80, 120],
    [110, -80, 120],
    [-110, 70, 120],
    [110, 70, 120],
    [0, -10, 290],
    // The stub wings: root front, root back, tip, root top; left, right.
    [-150, -70, 40],
    [-150, -70, -260],
    [-300, -90, -230],
    [-150, -30, -200],
    [150, -70, 40],
    [150, -70, -260],
    [300, -90, -230],
    [150, -30, -200],
    // The engine on the back: lower right, upper right, upper left.
    [110, -70, -300],
    [110, 60, -300],
    [-110, 60, -300],
];

const DRAY_RAW: [RawFace; 30] = {
    use mat::*;
    [
        // The back: a frame (dark below, light above, the box's grey at
        // the sides) round one wide glowing engine.
        rf(0, 1, 22, GUNMETAL, 0),
        rf(0, 22, 8, GUNMETAL, 0),
        rf(1, 3, 23, HULL2, 0),
        rf(1, 23, 22, HULL2, 0),
        rf(3, 2, 24, HULL, 0),
        rf(3, 24, 23, HULL, 0),
        rf(2, 0, 8, HULL2, 0),
        rf(2, 8, 24, HULL2, 0),
        rf(8, 22, 23, ENGINE, 0),
        rf(8, 23, 24, ENGINE, 0),
        // Front, sides, top and bottom of the box.
        rf(4, 5, 7, HULL2, 0),
        rf(4, 7, 6, HULL2, 0),
        rf(0, 2, 6, HULL2, 0),
        rf(0, 6, 4, HULL2, 0),
        rf(1, 3, 7, HULL2, 0),
        rf(1, 7, 5, HULL2, 0),
        rf(2, 3, 7, HULL, 0),
        rf(2, 7, 6, HULL, 0),
        rf(0, 1, 5, GUNMETAL, 0),
        rf(0, 5, 4, GUNMETAL, 0),
        // The snout: glass on top.
        rf(11, 12, 13, GLASS, 1),
        rf(9, 11, 13, HULL2, 1),
        rf(10, 12, 13, HULL2, 1),
        rf(9, 10, 13, GUNMETAL, 1),
        // The wings: dark below, light above.
        rf(14, 15, 16, GUNMETAL, 2),
        rf(14, 16, 17, HULL, 2),
        rf(15, 16, 17, HULL2, 2),
        rf(18, 19, 20, GUNMETAL, 3),
        rf(18, 20, 21, HULL, 3),
        rf(19, 20, 21, HULL2, 3),
    ]
};
model!(DRAY, DRAY_V, DRAY_RAW);

const SHRIKE_V: [[i16; 3]; 22] = [
    // The dart: nose, the ring (top, right, bottom, left), tail.
    [0, 0, 300],
    [0, 60, -150],
    [70, 0, -150],
    [0, -50, -150],
    [-70, 0, -150],
    [0, 0, -250],
    // Four fins in an X, each a thin wedge: root front, root back
    // (both inside the dart), tip, and a corner that gives it its
    // thickness (inside the dart too). Upper right, upper left, lower
    // right, lower left.
    [15, 12, -40],
    [12, 8, -200],
    [220, 170, -250],
    [2, 22, -180],
    [-15, 12, -40],
    [-12, 8, -200],
    [-220, 170, -250],
    [-2, 22, -180],
    [15, -10, -40],
    [12, -7, -200],
    [200, -150, -240],
    [2, -20, -180],
    [-15, -10, -40],
    [-12, -7, -200],
    [-200, -150, -240],
    [-2, -20, -180],
];

const SHRIKE_RAW: [RawFace; 20] = {
    use mat::*;
    let mut f = [rf(0, 0, 0, 0, 0); 20];
    // The dart: a glass canopy on top in front, dark below, the engines
    // glowing on the back's upper faces.
    f[0] = rf(0, 1, 2, GLASS, 0);
    f[1] = rf(0, 4, 1, GLASS, 0);
    f[2] = rf(0, 2, 3, GUNMETAL, 0);
    f[3] = rf(0, 3, 4, GUNMETAL, 0);
    f[4] = rf(5, 1, 2, ENGINE, 0);
    f[5] = rf(5, 4, 1, ENGINE, 0);
    f[6] = rf(5, 2, 3, GUNMETAL, 0);
    f[7] = rf(5, 3, 4, GUNMETAL, 0);
    // The fins: their two broad faces and the thin trailing one (the
    // root face lies inside the dart and is left out). Upper fins light,
    // lower fins darker.
    let mut k = 0;
    while k < 4 {
        let a = 6 + 4 * k as u8;
        let (broad, edge) = if k < 2 {
            (HULL, HULL2)
        } else {
            (HULL2, GUNMETAL)
        };
        let at = 8 + 3 * k;
        f[at] = rf(a, a + 2, a + 3, broad, 1 + k as u8);
        f[at + 1] = rf(a, a + 1, a + 2, broad, 1 + k as u8);
        f[at + 2] = rf(a + 1, a + 2, a + 3, edge, 1 + k as u8);
        k += 1;
    }
    f
};
model!(SHRIKE, SHRIKE_V, SHRIKE_RAW);

/// Step M5: each ship's model, in [`crate::world::CRAFTS`] order.
pub static CRAFT_MODELS: [&Model; 3] = [&COURIER, &DRAY, &SHRIKE];

/// The model of ship `i` (the LARK's for a number past the table).
pub fn craft_model(i: u8) -> &'static Model {
    CRAFT_MODELS
        .get(usize::from(i))
        .copied()
        .unwrap_or(&COURIER)
}

/// A piece of a wreck: a bent splinter. Its [`mat::RAIDER`] faces are
/// swapped for the wreck's own colour when drawn.
const SHARD_V: [[i16; 3]; 4] = [[0, 10, 130], [-90, -30, -70], [85, -40, -55], [10, 60, -40]];

const SHARD_RAW: [RawFace; 4] = {
    use mat::*;
    [
        rf(0, 1, 2, RAIDER, 0),
        rf(0, 2, 3, RAIDER, 0),
        rf(0, 3, 1, GUNMETAL, 0),
        rf(1, 2, 3, GUNMETAL, 0),
    ]
};
model!(SHARD, SHARD_V, SHARD_RAW);

/// A box's twelve faces from its eight corners (`a` + 0..8: x low/high
/// in bit 0, y in bit 1, z in bit 2). Faces at low z get `back`, at high
/// z `front`, the rest `side` (top and bottom `top`).
pub(crate) const fn cuboid(f: &mut [RawFace], at: usize, a: u8, m: [u8; 4], part: u8) {
    let [side, top, front, back] = m;
    let q: [[u8; 4]; 6] = [
        [0, 1, 3, 2], // back (z low)
        [4, 5, 7, 6], // front (z high)
        [0, 2, 6, 4], // left
        [1, 3, 7, 5], // right
        [2, 3, 7, 6], // top
        [0, 1, 5, 4], // bottom
    ];
    let mats = [back, front, side, side, top, top];
    let mut k = 0;
    while k < 6 {
        let v = q[k];
        f[at + 2 * k] = rf(a + v[0], a + v[1], a + v[2], mats[k], part);
        f[at + 2 * k + 1] = rf(a + v[0], a + v[2], a + v[3], mats[k], part);
        k += 1;
    }
}

/// The eight corners of a box from its low and high corner.
pub(crate) const fn box_corners(v: &mut [[i16; 3]], at: usize, lo: [i16; 3], hi: [i16; 3]) {
    let mut k = 0;
    while k < 8 {
        v[at + k] = [
            if k & 1 == 0 { lo[0] } else { hi[0] },
            if k & 2 == 0 { lo[1] } else { hi[1] },
            if k & 4 == 0 { lo[2] } else { hi[2] },
        ];
        k += 1;
    }
}

// Every ship kind has its own outline seen head on (the way a hunter
// comes at you), and its own colour: the raider a red three-pointed
// claw, the hauler a gold block, the warden a white lozenge with a
// blue fin. A few large faces each, so flat shading gives two or three
// clear tones even at 15 pixels across.

const RAIDER_V: [[i16; 3]; 8] = [
    // Nose and tail.
    [0, 0, 250],
    [0, 0, -170],
    // The ring the claws grow from: top, lower left, lower right.
    [0, 80, -10],
    [-70, -40, -10],
    [70, -40, -10],
    // Claw tips, swept back: upper left, down, upper right.
    [-234, 135, -100],
    [0, -270, -100],
    [234, 135, -100],
];

const RAIDER_RAW: [RawFace; 12] = {
    use mat::*;
    let mut f = [rf(0, 0, 0, 0, 0); 12];
    // The nose cone.
    f[0] = rf(0, 2, 3, RAIDER2, 0);
    f[1] = rf(0, 3, 4, GUNMETAL, 0);
    f[2] = rf(0, 4, 2, RAIDER2, 0);
    // Three claws: a bright front face, two dark back faces to the tail.
    let ring = [2u8, 3, 4];
    let mut k = 0;
    while k < 3 {
        let (a, b, t) = (ring[k], ring[(k + 1) % 3], 5 + k as u8);
        f[3 + 3 * k] = rf(a, b, t, RAIDER, 1 + k as u8);
        f[4 + 3 * k] = rf(a, t, 1, RAIDER2, 1 + k as u8);
        f[5 + 3 * k] = rf(t, b, 1, GUNMETAL, 1 + k as u8);
        k += 1;
    }
    f
};
model!(RAIDER, RAIDER_V, RAIDER_RAW);

const HAULER_V: [[i16; 3]; 16] = {
    let mut v = [[0i16; 3]; 16];
    box_corners(&mut v, 0, [-120, -96, -300], [120, 96, 100]);
    // The cab: a box narrowing to the front (written corner by corner).
    let cab = [
        [-70, -60, 100],
        [70, -60, 100],
        [-70, 60, 100],
        [70, 60, 100],
        [-44, -36, 270],
        [44, -36, 270],
        [-44, 36, 270],
        [44, 36, 270],
    ];
    let mut k = 0;
    while k < 8 {
        v[8 + k] = cab[k];
        k += 1;
    }
    v
};

const HAULER_RAW: [RawFace; 24] = {
    use mat::*;
    let mut f = [rf(0, 0, 0, 0, 0); 24];
    cuboid(&mut f, 0, 0, [HAULER, HAULER2, HAULER, HAULER2], 0);
    cuboid(&mut f, 12, 8, [HAULER, HAULER, GLASS, HAULER2], 1);
    f
};
model!(HAULER, HAULER_V, HAULER_RAW);

const WARDEN_V: [[i16; 3]; 9] = [
    // Nose, then the lozenge at the back: left, top, right, bottom.
    [0, 0, 240],
    [-190, 0, -200],
    [0, 100, -200],
    [190, 0, -200],
    [0, -100, -200],
    // The fin: base left, base right, base back, top.
    [-36, 60, -70],
    [36, 60, -70],
    [0, 90, -200],
    [0, 230, -250],
];

const WARDEN_RAW: [RawFace; 9] = {
    use mat::*;
    let mut f = [rf(0, 0, 0, 0, 0); 9];
    // Light above, blue below.
    f[0] = rf(0, 1, 2, WARDEN, 0);
    f[1] = rf(0, 2, 3, WARDEN, 0);
    f[2] = rf(0, 3, 4, WARDEN2, 0);
    f[3] = rf(0, 4, 1, WARDEN2, 0);
    f[4] = rf(1, 2, 3, WARDEN2, 0);
    f[5] = rf(1, 3, 4, WARDEN2, 0);
    f[6] = rf(5, 6, 8, WARDEN2, 1);
    f[7] = rf(6, 7, 8, WARDEN, 1);
    f[8] = rf(7, 5, 8, WARDEN, 1);
    f
};
model!(WARDEN, WARDEN_V, WARDEN_RAW);

// Step L3: a second hull for each role. Each keeps its role's colours
// and a head-on outline unlike the first hull's and unlike the other
// roles' (the claw, the block, the lozenge).

/// The two tanks' corners: a three-sided bar, apex up, along z.
const fn prism(v: &mut [[i16; 3]], at: usize, x0: i16, x1: i16, z0: i16, z1: i16) {
    let (mid, lo, hi) = ((x0 + x1) / 2, -56, 64);
    let ring = [[x0, lo], [x1, lo], [mid, hi]];
    let mut k = 0;
    while k < 3 {
        v[at + k] = [ring[k][0], ring[k][1], z0];
        v[at + 3 + k] = [ring[k][0], ring[k][1], z1];
        k += 1;
    }
}

/// A three-sided bar's eight faces (corners `a` + 0..6).
const fn prism_faces(f: &mut [RawFace], at: usize, a: u8, side: u8, top: u8, cap: u8, part: u8) {
    f[at] = rf(a, a + 1, a + 2, cap, part);
    f[at + 1] = rf(a + 3, a + 4, a + 5, cap, part);
    let mats = [side, top, top];
    let mut k = 0;
    while k < 3 {
        let (p, q) = (a + k as u8, a + ((k + 1) % 3) as u8);
        f[at + 2 + 2 * k] = rf(p, q, q + 3, mats[k], part);
        f[at + 3 + 2 * k] = rf(p, q + 3, p + 3, mats[k], part);
        k += 1;
    }
}

const TANKER_V: [[i16; 3]; 16] = {
    let mut v = [[0i16; 3]; 16];
    prism(&mut v, 0, -156, -26, -320, 150);
    prism(&mut v, 6, 26, 156, -320, 150);
    // The cab: a low wedge ahead of and between the tanks.
    v[12] = [-60, -40, 150];
    v[13] = [60, -40, 150];
    v[14] = [0, 44, 150];
    v[15] = [0, -10, 300];
    v
};

const TANKER_RAW: [RawFace; 20] = {
    use mat::*;
    let mut f = [rf(0, 0, 0, 0, 0); 20];
    prism_faces(&mut f, 0, 0, HAULER2, HAULER, GUNMETAL, 0);
    prism_faces(&mut f, 8, 6, HAULER2, HAULER, GUNMETAL, 1);
    f[16] = rf(12, 13, 14, HAULER2, 2);
    f[17] = rf(12, 13, 15, HAULER2, 2);
    f[18] = rf(13, 14, 15, GLASS, 2);
    f[19] = rf(14, 12, 15, GLASS, 2);
    f
};
model!(TANKER, TANKER_V, TANKER_RAW);

const HOOK_V: [[i16; 3]; 12] = [
    // The body, a diamond: nose, tail, top, bottom, left, right.
    [0, 0, 210],
    [0, 0, -170],
    [0, 64, -30],
    [0, -44, -30],
    [-56, 0, -30],
    [56, 0, -30],
    // Two blades swept forward like a crescent: elbow, tip, ridge.
    [-230, 0, -150],
    [-250, 0, 190],
    [-170, 56, -30],
    [230, 0, -150],
    [250, 0, 190],
    [170, 56, -30],
];

const HOOK_RAW: [RawFace; 16] = {
    use mat::*;
    let mut f = [rf(0, 0, 0, 0, 0); 16];
    // The diamond: bright above, dark below.
    let (n, t, up, dn, l, r) = (0u8, 1u8, 2u8, 3u8, 4u8, 5u8);
    f[0] = rf(n, up, l, RAIDER, 0);
    f[1] = rf(n, up, r, RAIDER, 0);
    f[2] = rf(n, dn, l, GUNMETAL, 0);
    f[3] = rf(n, dn, r, GUNMETAL, 0);
    f[4] = rf(t, up, l, RAIDER2, 0);
    f[5] = rf(t, up, r, RAIDER2, 0);
    f[6] = rf(t, dn, l, GUNMETAL, 0);
    f[7] = rf(t, dn, r, GUNMETAL, 0);
    // Each blade: root, elbow, tip, ridge.
    let mut k = 0;
    while k < 2 {
        let (root, e) = (4 + k as u8, 6 + 3 * k as u8);
        let (tip, ridge) = (e + 1, e + 2);
        let at = 8 + 4 * k;
        f[at] = rf(root, tip, ridge, RAIDER, 1 + k as u8);
        f[at + 1] = rf(e, tip, ridge, RAIDER2, 1 + k as u8);
        f[at + 2] = rf(root, e, ridge, RAIDER2, 1 + k as u8);
        f[at + 3] = rf(root, e, tip, GUNMETAL, 1 + k as u8);
        k += 1;
    }
    f
};
model!(HOOK, HOOK_V, HOOK_RAW);

const CUTTER_V: [[i16; 3]; 13] = [
    // A long arrowhead: nose, back left, back right, back top, back bottom.
    [0, 0, 290],
    [-120, 0, -220],
    [120, 0, -220],
    [0, 56, -220],
    [0, -46, -220],
    // Two fins leaning out: front, back, tip, inner.
    [-40, 26, -60],
    [-74, 20, -215],
    [-150, 136, -260],
    [-46, 40, -205],
    [40, 26, -60],
    [74, 20, -215],
    [150, 136, -260],
    [46, 40, -205],
];

const CUTTER_RAW: [RawFace; 14] = {
    use mat::*;
    let mut f = [rf(0, 0, 0, 0, 0); 14];
    f[0] = rf(0, 1, 3, WARDEN, 0);
    f[1] = rf(0, 3, 2, WARDEN, 0);
    f[2] = rf(0, 2, 4, WARDEN2, 0);
    f[3] = rf(0, 4, 1, WARDEN2, 0);
    f[4] = rf(1, 2, 3, GUNMETAL, 0);
    f[5] = rf(1, 4, 2, GUNMETAL, 0);
    let mut k = 0;
    while k < 2 {
        let a = 5 + 4 * k as u8;
        let at = 6 + 4 * k;
        f[at] = rf(a, a + 1, a + 2, WARDEN2, 1 + k as u8);
        f[at + 1] = rf(a, a + 2, a + 3, WARDEN, 1 + k as u8);
        f[at + 2] = rf(a + 1, a + 2, a + 3, WARDEN2, 1 + k as u8);
        f[at + 3] = rf(a, a + 1, a + 3, GUNMETAL, 1 + k as u8);
        k += 1;
    }
    f
};
model!(CUTTER, CUTTER_V, CUTTER_RAW);

const CANISTER_V: [[i16; 3]; 8] = {
    let mut v = [[0i16; 3]; 8];
    box_corners(&mut v, 0, [-40, -40, -40], [40, 40, 40]);
    v
};
const CANISTER_RAW: [RawFace; 12] = {
    use mat::*;
    let mut f = [rf(0, 0, 0, 0, 0); 12];
    cuboid(&mut f, 0, 0, [CANISTER, CANISTER, GUNMETAL, GUNMETAL], 0);
    f
};
model!(CANISTER, CANISTER_V, CANISTER_RAW);

// Step L4: a rock, a seven-sided ring of uneven corners between two
// off-centre peaks; facets in two greys.
const ROCK_V: [[i16; 3]; 9] = [
    [14, 118, -10],
    [-10, -104, 18],
    [120, 20, 0],
    [70, -10, 96],
    [-36, 30, 116],
    [-112, -16, 50],
    [-98, 26, -64],
    [-20, -20, -122],
    [76, 34, -84],
];

const ROCK_RAW: [RawFace; 14] = {
    use mat::*;
    let mut f = [rf(0, 0, 0, 0, 0); 14];
    let mut k = 0;
    while k < 7 {
        let (a, b) = (2 + k as u8, 2 + ((k + 1) % 7) as u8);
        let (up, down) = if k % 2 == 0 {
            (STATION2, GUNMETAL)
        } else {
            (GUNMETAL, STATION2)
        };
        f[2 * k] = rf(0, a, b, up, 0);
        f[2 * k + 1] = rf(1, a, b, down, 0);
        k += 1;
    }
    f
};
model!(ROCK, ROCK_V, ROCK_RAW);

const TAG_RAW: [RawFace; 12] = {
    use mat::*;
    let mut f = [rf(0, 0, 0, 0, 0); 12];
    cuboid(&mut f, 0, 0, [CANISTER; 4], 0);
    f
};
model!(TAG_BOX, CANISTER_V, TAG_RAW);

// ---- The station -------------------------------------------------------------------

/// Hub radius, back and front (1/16 m).
const HUB_R: i32 = 1600;
const HUB_BACK: i16 = -1800;
/// Where the hub's front face is; the mouth stands out of it.
pub const HUB_FRONT: i16 = 1400;
/// The mouth's front, half width and half height (1/16 m).
pub const MOUTH_Z: i16 = 1520;
/// Step M2: wider and taller (600 and 260 before), so the biggest
/// freighter fits through it with the station at STATION_X.
pub const MOUTH_W: i16 = 900;
pub const MOUTH_H: i16 = 560;

const STATION_V: [[i16; 3]; 40] = {
    let mut v = [[0i16; 3]; 40];
    let mut k = 0;
    while k < 8 {
        // Corners at 22.5 degrees + k * 45, so the hub lies flat.
        let a = (4096 + k * 8192) as u16;
        let (x, y) = (
            (csin(a) * HUB_R / 16384) as i16,
            (ccos(a) * HUB_R / 16384) as i16,
        );
        v[k] = [x, y, HUB_BACK];
        v[8 + k] = [x, y, HUB_FRONT];
        k += 1;
    }
    box_corners(
        &mut v,
        16,
        [-MOUTH_W, -MOUTH_H, HUB_FRONT - 200],
        [MOUTH_W, MOUTH_H, MOUTH_Z],
    );
    box_corners(&mut v, 24, [1560, -30, -1100], [4300, 30, 500]);
    box_corners(&mut v, 32, [-4300, -30, -1100], [-1560, 30, 500]);
    v
};

const STATION_RAW: [RawFace; 64] = {
    use mat::*;
    let mut f = [rf(0, 0, 0, 0, 0); 64];
    let mut k = 0u8;
    while k < 8 {
        let n = (k + 1) % 8;
        let m = if k.is_multiple_of(2) {
            STATION
        } else {
            STATION2
        };
        f[2 * k as usize] = rf(k, n, 8 + n, m, 0);
        f[2 * k as usize + 1] = rf(k, 8 + n, 8 + k, m, 0);
        k += 1;
    }
    // Back and front caps: fans from corner 0 (and 8).
    let mut k = 1u8;
    while k < 7 {
        let back = if k == 3 { LIGHT_R } else { STATION2 };
        f[16 + (k - 1) as usize] = rf(0, k, k + 1, back, 0);
        f[22 + (k - 1) as usize] = rf(8, 8 + k, 9 + k, STATION, 0);
        k += 1;
    }
    cuboid(&mut f, 28, 16, [LIGHT_G, LIGHT_G, MOUTH, STATION2], 1);
    cuboid(&mut f, 40, 24, [GUNMETAL, PANEL, GUNMETAL, GUNMETAL], 2);
    cuboid(&mut f, 52, 32, [GUNMETAL, PANEL, GUNMETAL, GUNMETAL], 3);
    f
};
model!(STATION, STATION_V, STATION_RAW);

// ---- Spheres ------------------------------------------------------------------------

/// Corner `j` of band edge `i` (1..rings) of a sphere.
const fn ring(i: usize, j: usize, segs: usize) -> u8 {
    (1 + (i - 1) * segs + j % segs) as u8
}

const fn sphere_verts<const V: usize>(rings: usize, segs: usize) -> [[i16; 3]; V] {
    let mut v = [[0i16; 3]; V];
    v[0] = [0, BALL as i16, 0];
    v[V - 1] = [0, -BALL as i16, 0];
    let mut i = 1;
    while i < rings {
        let lat = (i * 32768 / rings) as u16;
        let y = ccos(lat) * BALL / 16384;
        let rr = csin(lat) * BALL / 16384;
        let mut j = 0;
        while j < segs {
            let lon = (j * 65536 / segs) as u16;
            v[ring(i, j, segs) as usize] = [
                (csin(lon) * rr / 16384) as i16,
                y as i16,
                (ccos(lon) * rr / 16384) as i16,
            ];
            j += 1;
        }
        i += 1;
    }
    v
}

/// The material of band `i`, piece `j` of a sphere with the pattern:
/// 0 land and sea with ice caps, 1 bands, 2 a sun.
const fn sphere_mat(i: usize, j: usize, rings: usize, pattern: u8) -> u8 {
    let h = hash(i as u32 * 7 + 3, (j / 2) as u32 + pattern as u32 * 100);
    match pattern {
        0 => {
            if i == 0 || i == rings - 1 {
                mat::PLANET_C
            } else if h % 5 < 2 {
                mat::PLANET_B
            } else {
                mat::PLANET_A
            }
        }
        1 => match i % 3 {
            0 => mat::PLANET_A,
            1 => mat::PLANET_B,
            _ => mat::PLANET_C,
        },
        _ => {
            if (i + j).is_multiple_of(3) {
                mat::SUN2
            } else {
                mat::SUN
            }
        }
    }
}

const fn sphere_faces<const F: usize>(rings: usize, segs: usize, pattern: u8) -> [RawFace; F] {
    let mut f = [rf(0, 0, 0, 0, 0); F];
    let bottom = (2 + (rings - 1) * segs - 1) as u8;
    let mut n = 0;
    let mut j = 0;
    while j < segs {
        f[n] = rf(
            0,
            ring(1, j, segs),
            ring(1, j + 1, segs),
            sphere_mat(0, j, rings, pattern),
            0,
        );
        f[n + 1] = rf(
            bottom,
            ring(rings - 1, j, segs),
            ring(rings - 1, j + 1, segs),
            sphere_mat(rings - 1, j, rings, pattern),
            0,
        );
        n += 2;
        j += 1;
    }
    let mut i = 1;
    while i < rings - 1 {
        let mut j = 0;
        while j < segs {
            let m = sphere_mat(i, j, rings, pattern);
            let (a, b) = (ring(i, j, segs), ring(i, j + 1, segs));
            let (c, d) = (ring(i + 1, j, segs), ring(i + 1, j + 1, segs));
            f[n] = rf(a, c, b, m, 0);
            f[n + 1] = rf(b, c, d, m, 0);
            n += 2;
            j += 1;
        }
        i += 1;
    }
    f
}

model!(
    PLANET_HI,
    sphere_verts::<62>(6, 12),
    sphere_faces::<120>(6, 12, 0)
);
model!(
    GIANT_HI,
    sphere_verts::<62>(6, 12),
    sphere_faces::<120>(6, 12, 1)
);
model!(
    PLANET_LO,
    sphere_verts::<34>(5, 8),
    sphere_faces::<64>(5, 8, 0)
);
model!(
    GIANT_LO,
    sphere_verts::<34>(5, 8),
    sphere_faces::<64>(5, 8, 1)
);
model!(
    SUN_BALL,
    sphere_verts::<26>(4, 8),
    sphere_faces::<48>(4, 8, 2)
);
