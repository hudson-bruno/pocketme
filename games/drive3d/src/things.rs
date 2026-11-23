//! Things beside the road (the spec's section 3): records of 5 bytes
//! (kind, cell x, cell z, the place in the cell as two 4-bit numbers in
//! 1/2 m, and a size), and the car's collision with them.
//!
//! The records are made while compiling from a short source list that
//! says where each thing stands beside the road (a road point and a
//! distance to its right, minus for the left), so they follow the road
//! when its records change. A thing stands on the land (its base is the
//! land's height at its place; `land::LAND.at`).

use crate::land::Land;
use crate::road::{POINTS, ROAD, Road, walk};
use crate::tune::{CAR_RADIUS, CELL, CELLS_X, CELLS_Z, GENTLE, HIT_KEEP, M};
use game_3d::{cos_fine, isqrt, sin_fine};

/// Kinds of thing.
pub mod kind {
    /// A tree: a cone on a trunk (sides by detail level). Solid trunk.
    pub const TREE: u8 = 0;
    /// A rock: only on gentle cells. Solid.
    pub const ROCK: u8 = 1;
    /// The hut. Solid.
    pub const HUT: u8 = 2;
    /// A fence post (along the drop) or a crest post. Solid.
    pub const POST: u8 = 3;
    /// A banner over the road (the split, and the finish gate): a bar
    /// across, on two poles at the road's edges; not solid (the car
    /// drives under it).
    pub const BANNER: u8 = 4;
    /// MAX's roadside things (`DECOR`), drawn only: never solid, never
    /// in the rules. A corner marker: a post with a yellow board,
    /// turned along the road.
    pub const MARKER: u8 = 5;
    /// A round hay bale.
    pub const BALE: u8 = 6;
    /// The two rails of the fence along the drop, between two of its
    /// posts (the thing stands halfway), turned along the road.
    pub const RAIL: u8 = 7;
}

/// One thing (5 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Thing {
    pub kind: u8,
    /// The cell it stands in.
    pub cx: u8,
    pub cz: u8,
    /// Its place in the cell: low 4 bits x, high 4 bits z, in 1/2 m
    /// (the middle of a 1/2 m square: + 1/4 m).
    pub place: u8,
    /// Its height in 1/4 m (the banner: its bar's height over the road).
    pub size: u8,
}

impl Thing {
    /// Where it stands (x, z), subs.
    pub const fn at(&self) -> (i32, i32) {
        let half = M / 2;
        (
            self.cx as i32 * CELL + (self.place & 15) as i32 * half + half / 2,
            self.cz as i32 * CELL + (self.place >> 4) as i32 * half + half / 2,
        )
    }

    /// The solid circle's radius (subs); 0 is not solid. DEFAULT sizes
    /// (the spec gives none): trunk 0.5 m, rock 1 m, hut 3 m (it is a
    /// 5 m box; the circle stands in for it), post 0.15 m.
    pub const fn radius(&self) -> i32 {
        match self.kind {
            kind::TREE => M / 2,
            kind::ROCK => M,
            kind::HUT => 3 * M,
            kind::POST => M * 3 / 20,
            _ => 0,
        }
    }

    /// Its height (subs).
    pub const fn height(&self) -> i32 {
        self.size as i32 * (M / 4)
    }
}

/// One line of the source list: kind, road point, distance to the
/// right of the road (1/2 m, minus is left), size (1/4 m).
type Source = (u8, u16, i16, u8);

/// Where the things stand. Trees well off the road (most beyond 10 m,
/// on the shoulders' rise), rocks on the flat inside of the second
/// corner and the flat finish, a fence along the drop's right edge,
/// posts on both sides of the crest, the hut near the start, a banner
/// over the split straight and one over the finish (the gate, review
/// look F6).
const SOURCE: [Source; 32] = [
    // The start: the hut on the right, trees on both sides.
    (kind::HUT, 6, 36, 14),
    (kind::TREE, 2, -24, 24),
    (kind::TREE, 12, -28, 28),
    (kind::TREE, 16, 26, 22),
    // The hill.
    (kind::TREE, 26, -22, 26),
    (kind::TREE, 34, 24, 30),
    (kind::TREE, 42, -30, 22),
    (kind::TREE, 48, 22, 26),
    // The crest posts.
    (kind::POST, 56, 9, 4),
    (kind::POST, 56, -9, 4),
    // The fence along the drop (right side).
    (kind::POST, 64, 9, 4),
    (kind::POST, 68, 9, 4),
    (kind::POST, 72, 9, 4),
    (kind::POST, 76, 9, 4),
    (kind::TREE, 70, -26, 28),
    // The ford.
    (kind::TREE, 86, 30, 24),
    (kind::TREE, 90, -32, 30),
    // The first corner (right): trees on the outside.
    (kind::TREE, 98, -24, 26),
    (kind::TREE, 106, -26, 28),
    (kind::TREE, 112, 30, 22),
    // The split straight and its banner.
    (kind::BANNER, 124, 0, 18),
    (kind::TREE, 120, 26, 26),
    (kind::TREE, 128, -26, 24),
    // The second corner (left): rocks on the cut inside, trees outside.
    (kind::ROCK, 138, -14, 4),
    (kind::ROCK, 144, -18, 5),
    (kind::TREE, 140, 28, 28),
    (kind::TREE, 148, 26, 24),
    // The finish.
    (kind::ROCK, 162, 10, 3),
    (kind::TREE, 158, -26, 26),
    (kind::TREE, 166, 28, 30),
    (kind::TREE, 170, -24, 24),
    // The finish gate, over the last road point.
    (kind::BANNER, (POINTS - 1) as u16, 0, 18),
];

/// How many things there are.
pub const COUNT: usize = SOURCE.len();

/// The things' records, made while compiling.
pub static THINGS: [Thing; COUNT] = make(&walk());

/// MAX's roadside things (the stage's dressing): drawn at MAX only and
/// never in the rules (the car drives through them; they stand where
/// the drive seldom goes). Three yellow corner markers on the outside
/// of the approach to the second corner, 30, 20 and 10 m before it;
/// the rails of the fence along the drop, between its posts; three
/// round hay bales well outside the second corner.
const DECOR_SOURCE: [Source; 9] = [
    (kind::MARKER, 117, 12, 5),
    (kind::MARKER, 122, 12, 5),
    (kind::MARKER, 127, 12, 5),
    (kind::RAIL, 66, 9, 4),
    (kind::RAIL, 70, 9, 4),
    (kind::RAIL, 74, 9, 4),
    (kind::BALE, 142, 20, 5),
    (kind::BALE, 145, 22, 5),
    (kind::BALE, 149, 19, 5),
];

/// How many roadside things MAX draws.
pub const DECOR_COUNT: usize = DECOR_SOURCE.len();

/// MAX's roadside things' records, made while compiling.
pub static DECOR: [Thing; DECOR_COUNT] = make_list(&walk(), &DECOR_SOURCE);

const _: () = {
    // Each rail stands halfway between two fence posts 8 m apart, on
    // their side of the road.
    let mut n = 0;
    while n < DECOR_COUNT {
        let (k, point, side, _) = DECOR_SOURCE[n];
        if k == kind::RAIL {
            let mut posts = 0;
            let mut i = 0;
            while i < COUNT {
                let (pk, pp, ps, _) = SOURCE[i];
                if pk == kind::POST && ps == side && (pp + 2 == point || pp == point + 2) {
                    posts += 1;
                }
                i += 1;
            }
            assert!(posts == 2);
        }
        n += 1;
    }
};

/// Turns the source list into records.
pub const fn make(road: &Road) -> [Thing; COUNT] {
    make_list(road, &SOURCE)
}

/// Turns a source list into records.
const fn make_list<const K: usize>(road: &Road, source: &[Source; K]) -> [Thing; K] {
    let mut out = [Thing {
        kind: 0,
        cx: 0,
        cz: 0,
        place: 0,
        size: 0,
    }; K];
    let mut n = 0usize;
    while n < K {
        let (k, point, side, size) = source[n];
        let p = point as usize;
        let a = road.heading[p];
        // Right of the road: (cos, -sin).
        let off = side as i64 * (M as i64 / 2);
        let x = road.x[p] as i64 + off * cos_fine(a) as i64 / 16_384;
        let z = road.z[p] as i64 - off * sin_fine(a) as i64 / 16_384;
        let half = (M / 2) as i64;
        let (hx, hz) = (x.div_euclid(half), z.div_euclid(half));
        let cells = (CELL as i64) / half; // 16 half metres a cell
        let mut cx = hx.div_euclid(cells);
        let mut cz = hz.div_euclid(cells);
        if cx < 0 {
            cx = 0;
        }
        if cz < 0 {
            cz = 0;
        }
        if cx >= CELLS_X as i64 {
            cx = CELLS_X as i64 - 1;
        }
        if cz >= CELLS_Z as i64 {
            cz = CELLS_Z as i64 - 1;
        }
        let px = hx.rem_euclid(cells) as u8;
        let pz = hz.rem_euclid(cells) as u8;
        out[n] = Thing {
            kind: k,
            cx: cx as u8,
            cz: cz as u8,
            place: px | (pz << 4),
            size,
        };
        n += 1;
    }
    out
}

/// A cell is gentle when its four corners differ by at most `GENTLE`
/// height steps.
pub fn gentle(land: &Land, cx: usize, cz: usize) -> bool {
    let (lo, hi) = land.cell_range(cx, cz);
    hi - lo <= GENTLE
}

/// What a hit did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hit {
    /// The car touched a solid thing this step (its index).
    pub thing: Option<u8>,
}

/// Pushes the car (its middle at `pos`, speed `vel`) out of every
/// solid thing it overlaps. The car is a circle of `CAR_RADIUS`; it is
/// moved out along the line from the thing's middle and loses a quarter
/// of its speed (`HIT_KEEP`), and its speed into the thing is taken
/// away. DEFAULT rule (the spec gives none).
pub fn collide(pos: &mut [i32; 3], vel: &mut [i32; 2]) -> Hit {
    let mut hit = Hit::default();
    for (i, t) in THINGS.iter().enumerate() {
        let r = t.radius();
        if r == 0 {
            continue;
        }
        let (tx, tz) = t.at();
        let dx = i64::from(pos[0]) - i64::from(tx);
        let dz = i64::from(pos[2]) - i64::from(tz);
        let reach = i64::from(r + CAR_RADIUS);
        let d2 = dx * dx + dz * dz;
        if d2 >= reach * reach {
            continue;
        }
        let d = isqrt(d2 as u64) as i64;
        // The way out (1/16384): from the thing to the car, or against
        // the car's speed when it stands right on the middle.
        let (ux, uz) = if d > 0 {
            (dx * 16_384 / d, dz * 16_384 / d)
        } else {
            let (vx, vz) = (i64::from(vel[0]), i64::from(vel[1]));
            let v = isqrt((vx * vx + vz * vz) as u64) as i64;
            if v > 0 {
                (-vx * 16_384 / v, -vz * 16_384 / v)
            } else {
                (0, -16_384)
            }
        };
        pos[0] = (i64::from(tx) + ux * reach / 16_384) as i32;
        pos[2] = (i64::from(tz) + uz * reach / 16_384) as i32;
        // Speed into the thing is taken away; the rest keeps 3/4.
        let into = (i64::from(vel[0]) * ux + i64::from(vel[1]) * uz) / 16_384;
        if into < 0 {
            vel[0] = (i64::from(vel[0]) - into * ux / 16_384) as i32;
            vel[1] = (i64::from(vel[1]) - into * uz / 16_384) as i32;
        }
        vel[0] = vel[0] * HIT_KEEP / 256;
        vel[1] = vel[1] * HIT_KEEP / 256;
        hit.thing = Some(i as u8);
    }
    hit
}

/// The things' places as a list (x, z, kind), for the drawing and the
/// tests.
pub fn places() -> impl Iterator<Item = (i32, i32, &'static Thing)> {
    THINGS.iter().map(|t| {
        let (x, z) = t.at();
        (x, z, t)
    })
}

/// The road the things were placed beside (for tests: `THINGS` was made
/// from this).
pub fn road() -> &'static Road {
    &ROAD
}
