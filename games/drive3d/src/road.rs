//! The stretch's road: road records (8 bytes each, the spec's section
//! 3), walked in 2 m steps while compiling into road points.
//!
//! A record says how long a piece is, how much it turns and climbs,
//! its surface and width. The walk keeps the heading and height of
//! every 2 m step; the land (`land`) and the progress along the road
//! (`play`) are worked out from these points.

use game_3d::{cos_fine, sin_fine};

use crate::tune::{FORD, GRAVEL, HEIGHT_STEP, M, START_X, START_Y, START_Z};

/// One road piece (8 bytes; TALUS's piece format adapted).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece {
    /// Length in 2 m steps.
    pub len: u8,
    /// Turn over the whole piece, in 1/256 of a turn (plus is right).
    pub turn: i8,
    /// Climb over the whole piece, in 1/4 m.
    pub climb: i8,
    /// Surface number (`tune::GRAVEL`, ...).
    pub surface: u8,
    /// Width in 1/4 m.
    pub width: u8,
    /// Pattern of things left and right (kept for later; unused now).
    pub things: u8,
    /// Note for later (pace notes): one of the `note` numbers.
    pub note: u8,
    /// `flag` bits.
    pub flags: u8,
}

/// Notes kept for later (no notes are shown yet).
pub mod note {
    pub const NONE: u8 = 0;
    pub const START: u8 = 1;
    pub const CREST: u8 = 2;
    pub const FORD: u8 = 3;
    pub const R5: u8 = 4;
    pub const SPLIT: u8 = 5;
    pub const L3: u8 = 6;
    pub const FINISH: u8 = 7;
}

/// Piece flags.
pub mod flag {
    /// The height goes over a rounded top: it starts with the last
    /// piece's slope and bends evenly to the end (a curve, not a
    /// straight climb).
    pub const CREST: u8 = 1;
    /// The split banner hangs over this piece.
    pub const SPLIT: u8 = 2;
    /// The run ends at this piece's end.
    pub const FINISH: u8 = 4;
    /// The inside of this corner is cut flat (low grass).
    pub const CUT: u8 = 8;
}

const fn piece(len_m: u8, turn: i8, climb: i8, surface: u8, note: u8, flags: u8) -> Piece {
    Piece {
        len: len_m / 2,
        turn,
        climb,
        surface,
        width: 24,
        things: 0,
        note,
        flags,
    }
}

/// The stretch (the spec's table, section 3). The crest climbs -0.75
/// m, not the table's -1 m: that is what "0.75 m up to 1.5 m down per
/// 8 m over 16 m" gives (the average slope (0.75 - 1.5) / 2 per 8 m,
/// times 2 cells, is -0.75 m).
pub const PIECES: [Piece; 9] = [
    piece(40, 0, 0, GRAVEL, note::START, 0),
    piece(64, 0, 24, GRAVEL, note::NONE, 0),
    piece(16, 0, -3, GRAVEL, note::CREST, flag::CREST),
    piece(40, 0, -20, GRAVEL, note::NONE, 0),
    piece(24, 0, 0, FORD, note::FORD, 0),
    piece(48, 43, 4, GRAVEL, note::R5, 0),
    piece(32, 0, 0, GRAVEL, note::SPLIT, flag::SPLIT),
    piece(40, -64, 0, GRAVEL, note::L3, flag::CUT),
    piece(40, 0, 0, GRAVEL, note::FINISH, flag::FINISH),
];

/// The road's points, one every 2 m from the start to the finish.
pub const POINTS: usize = road_steps() + 1;
/// One road step (2 m) in subs.
pub const STEP: i32 = 2 * M;

const fn road_steps() -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < PIECES.len() {
        n += PIECES[i].len as usize;
        i += 1;
    }
    n
}

/// The road's points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Road {
    /// Where each point is (subs).
    pub x: [i32; POINTS],
    pub z: [i32; POINTS],
    /// The road's height there (subs).
    pub y: [i32; POINTS],
    /// The way the road goes there (angle).
    pub heading: [u16; POINTS],
    /// The piece the point ends (point 0: piece 0).
    pub piece: [u8; POINTS],
}

/// The road, walked while compiling.
pub static ROAD: Road = walk();

/// Walks the records in 2 m steps (a `const fn`; tests also run it as
/// plain code and compare).
pub const fn walk() -> Road {
    let mut r = Road {
        x: [0; POINTS],
        z: [0; POINTS],
        y: [0; POINTS],
        heading: [0; POINTS],
        piece: [0; POINTS],
    };
    // Sums in 1/16 sub so 86 steps of rounding do not add up.
    let mut x16: i64 = START_X as i64 * 16;
    let mut z16: i64 = START_Z as i64 * 16;
    r.x[0] = START_X;
    r.z[0] = START_Z;
    r.y[0] = START_Y;
    let mut head: i32 = 0;
    let mut y0 = START_Y as i64;
    let mut prev_slope_num: i64 = 0; // climb of the last piece (subs)
    let mut prev_slope_den: i64 = 1; // over this many steps
    let mut n = 0usize;
    let mut p = 0usize;
    while p < PIECES.len() {
        let pc = PIECES[p];
        let len = pc.len as i64;
        let turn = pc.turn as i64 * 256; // angle units over the piece
        let climb = pc.climb as i64 * HEIGHT_STEP as i64;
        let mut k = 0i64;
        while k < len {
            // Heading in the middle of this step.
            let mid = head as i64 + turn * (2 * k + 1) / (2 * len);
            let a = (mid & 0xFFFF) as u16;
            x16 += 2 * M as i64 * sin_fine(a) as i64 / 1024;
            z16 += 2 * M as i64 * cos_fine(a) as i64 / 1024;
            let k1 = k + 1;
            let dy = if pc.flags & flag::CREST != 0 {
                // y(k) = s0 k + (C - s0 L) k^2 / L^2, s0 the last
                // piece's slope a step.
                (prev_slope_num * k1 * len * len
                    + (climb * prev_slope_den - prev_slope_num * len) * k1 * k1)
                    / (prev_slope_den * len * len)
            } else {
                climb * k1 / len
            };
            n += 1;
            r.x[n] = round16(x16);
            r.z[n] = round16(z16);
            r.y[n] = (y0 + dy) as i32;
            r.heading[n] = ((head as i64 + turn * k1 / len) & 0xFFFF) as u16;
            r.piece[n] = p as u8;
            k += 1;
        }
        head = (head as i64 + turn) as i32;
        y0 += climb;
        prev_slope_num = climb;
        prev_slope_den = len;
        p += 1;
    }
    r.heading[0] = 0;
    r
}

const fn round16(v: i64) -> i32 {
    ((v + 8).div_euclid(16)) as i32
}

impl Road {
    /// The first and last point of piece `p`.
    pub fn piece_points(&self, p: usize) -> (usize, usize) {
        let mut first = 0;
        for pc in PIECES.iter().take(p) {
            first += pc.len as usize;
        }
        (first, first + PIECES[p].len as usize)
    }

    /// The last point (the finish line).
    pub const fn last(&self) -> usize {
        POINTS - 1
    }
}

/// The surface of the road at point `i` (that of the piece it ends).
pub fn surface_at(i: usize) -> u8 {
    PIECES[usize::from(ROAD.piece[i])].surface
}

/// The road's half width at point `i` (subs).
pub fn half_width(i: usize) -> i32 {
    i32::from(PIECES[usize::from(ROAD.piece[i])].width) * crate::tune::HEIGHT_STEP / 2
}
