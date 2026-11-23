//! The galaxy: 136 star systems made from one seed number. Nothing is
//! stored: [`system`] works a system out again from the seed and its
//! number whenever it is needed (a few hashes), so a galaxy costs no
//! memory and no card space.
//!
//! **Where systems are.** The galaxy is a lens: 12 rows of cells, 5 to
//! 15 cells long, each row centred on the same middle column
//! ([`ROWS`]), so the whole shape fills the chart's wide box. A cell is
//! 3.4 x 2.8 light years and one system sits somewhere inside it (at
//! least 0.4 light years from its edges). So the next system along a
//! row is at most 6.2 light years away (5.9 across, 1.9 down) and the
//! one in the cell above or below at most 5.4 (2.5 across, 4.7 down):
//! a full tank (6.5 light years) always reaches every neighbour, and
//! every system of every galaxy can be reached from every other.
//! Positions are in tenths of a light year ([`LY`]).
//!
//! **What a system is like.** Its economy ([`Econ`]: what it makes and
//! what it pays for), its danger (how many pirates and patrols fly
//! there), its tech level (which gear its station sells), its planet's
//! and its sun's look, and a name made of two or three syllables from
//! the game's own table.

use crate::fixed::hash;

/// The lens's rows, top to bottom: how many cells each has. Every row
/// is centred on column 7, so a shorter row lies wholly above or below
/// a longer one and the rows join.
pub const ROWS: [u8; 12] = [5, 9, 11, 13, 15, 15, 15, 15, 13, 11, 9, 5];
/// Systems in a galaxy: one per cell.
pub const SYSTEMS: usize = {
    let mut n = 0;
    let mut r = 0;
    while r < ROWS.len() {
        n += ROWS[r] as usize;
        r += 1;
    }
    n
};
/// Cells across the widest row.
pub const COLS: u32 = 15;
/// One cell, in tenths of a light year.
pub const CELL_W: u32 = 34;
pub const CELL_H: u32 = 28;
/// The galaxy's size in tenths of a light year.
pub const WIDTH: u32 = COLS * CELL_W;
pub const HEIGHT: u32 = ROWS.len() as u32 * CELL_H;

/// The cell (column, row) of system `id` (ids run row by row).
#[inline(never)]
pub fn cell(id: u8) -> (u32, u32) {
    let mut i = u32::from(id);
    let mut r = 0;
    // ids past the last row stay in it (only tests ask for those).
    while r + 1 < ROWS.len() && i >= u32::from(ROWS[r]) {
        i = i.wrapping_sub(u32::from(ROWS[r]));
        r = r.wrapping_add(1);
    }
    let start = (COLS - u32::from(ROWS[r])) / 2;
    (start.wrapping_add(i), r as u32)
}

/// Whether the lens has a cell at (column, row).
#[inline(never)]
fn has_cell(c: u32, r: u32) -> bool {
    ROWS.get(r as usize).is_some_and(|&n| {
        let start = (COLS - u32::from(n)) / 2;
        c >= start && c < start.wrapping_add(u32::from(n))
    })
}
/// A light year, in position units.
pub const LY: u32 = 10;

/// What a system's economy makes and buys (see `market`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Econ {
    /// Grows food.
    Farm,
    /// Digs ore and ice.
    Mine,
    /// Smelts and builds: alloys and spare parts.
    Forge,
    /// Research: medicine and chips.
    Lab,
    /// A rich garden world: makes art, pays well for the rest.
    Haven,
}

impl Econ {
    pub const ALL: [Econ; 5] = [Econ::Farm, Econ::Mine, Econ::Forge, Econ::Lab, Econ::Haven];

    pub fn name(self) -> &'static str {
        match self {
            Econ::Farm => "FARM",
            Econ::Mine => "MINE",
            Econ::Forge => "FORGE",
            Econ::Lab => "LAB",
            Econ::Haven => "HAVEN",
        }
    }
}

/// How dangerous a system is: 0 to 3.
pub const DANGER_NAMES: [&str; 4] = ["SECURE", "QUIET", "RISKY", "LAWLESS"];

/// The syllables names are made of: the game's own table.
const SYL: [&str; 32] = [
    "AR", "BEL", "CO", "DUN", "EN", "FA", "GOR", "HI", "IS", "JA", "KEL", "LO", "MAR", "NU", "OR",
    "PEL", "QUA", "RI", "SAN", "TOR", "UL", "VE", "WEN", "XI", "YAR", "ZE", "THO", "CRA", "DRI",
    "SOL", "VAN", "EST",
];

/// A system's name: up to three syllables of up to three letters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    b: [u8; 9],
    n: u8,
}

impl Name {
    /// For tests and tools: the game draws [`Name::bytes`].
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(self.bytes()).unwrap_or("?")
    }

    /// The name's letters (capitals only).
    pub fn bytes(&self) -> &[u8] {
        self.b.get(..usize::from(self.n)).unwrap_or(&[])
    }

    fn push(&mut self, s: &str) {
        for &c in s.as_bytes() {
            if usize::from(self.n) < self.b.len() {
                self.b[usize::from(self.n)] = c;
                self.n = self.n.wrapping_add(1);
            }
        }
    }
}

/// One star system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct System {
    pub id: u8,
    /// Position in tenths of a light year.
    pub x: u16,
    pub y: u16,
    pub econ: Econ,
    /// 0 (SECURE) to 3 (LAWLESS).
    pub danger: u8,
    /// 1 to 12: which gear the station sells.
    pub tech: u8,
    /// The planet's look (palette set), 0 to 7.
    pub look: u8,
    /// The sun's colour, 0 to 4.
    pub star: u8,
    /// A number for everything else about it (sizes, places).
    pub h: u32,
    /// The name's key: a shuffle of the system numbers, different in
    /// each galaxy, so no two systems of a galaxy share a name.
    pub key: u8,
}

/// System `id` of the galaxy made from `seed`.
pub fn system(seed: u32, id: u8) -> System {
    let i = u32::from(id);
    let h = hash(seed, i);
    let h2 = hash(h, 0x5EED);
    let (cx, cy) = cell(id);
    // Inside the cell, 4 tenths from each edge.
    let x = cx * CELL_W + 4 + h % (CELL_W - 8);
    let y = cy * CELL_H + 4 + (h >> 8) % (CELL_H - 8);
    let econ = Econ::ALL[((h >> 16) % 5) as usize];
    // Danger: more often low; the lens's rim (a cell with a side open
    // to empty space) is rougher. Cells are small numbers: the sums
    // cannot wrap (a missing -1 wraps to a huge column: no cell).
    let rim = u32::from(
        !has_cell(cx.wrapping_sub(1), cy)
            || !has_cell(cx.wrapping_add(1), cy)
            || !has_cell(cx, cy.wrapping_sub(1))
            || !has_cell(cx, cy.wrapping_add(1)),
    );
    let danger = (match (h >> 20) % 16 {
        0..=4 => 0,
        5..=9 => 1,
        10..=13 => 2,
        _ => 3,
    } + rim)
        .min(3) as u8;
    let tech = (1
        + (h2 % 8)
        + match econ {
            Econ::Lab => 4,
            Econ::Forge | Econ::Haven => 2,
            _ => 0,
        }) as u8;
    System {
        id,
        x: x as u16,
        y: y as u16,
        econ,
        danger,
        tech: tech.min(12),
        look: ((h2 >> 8) % 8) as u8,
        star: ((h2 >> 12) % 5) as u8,
        h: h2,
        // Odd times id plus a constant: every key once per galaxy.
        key: (i.wrapping_mul(((seed >> 8) & 126) | 1).wrapping_add(seed)) as u8,
    }
}

impl System {
    /// The system's name.
    pub fn name(&self) -> Name {
        let mut n = Name { b: [0; 9], n: 0 };
        let h = self.h;
        // The first two syllables come from the key (each key gives
        // another pair), a third one from the system's number. The
        // second is never the first again: it moves on inside its own
        // group of four, so the pair still tells the key.
        let k = u32::from(self.key);
        let first = k % 32;
        let mut second = (k / 32 * 4 + (h >> 3) % 4) % 32;
        if second == first {
            second = (second & !3) | (second.wrapping_add(1) & 3);
        }
        n.push(SYL[first as usize]);
        n.push(SYL[second as usize]);
        if (h >> 24) % 5 >= 3 {
            n.push(SYL[((h >> 8) % 32) as usize]);
        }
        n
    }

    /// Distance to `o`, in tenths of a light year (rounded up, so a jump
    /// never costs less fuel than the chart shows).
    pub fn dist(&self, o: &System) -> u32 {
        let dx = u64::from(self.x.abs_diff(o.x));
        let dy = u64::from(self.y.abs_diff(o.y));
        // Coordinates are u8 or u16 sized: nothing here can run over.
        let d2 = dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy));
        let r = crate::fixed::isqrt(d2);
        r.wrapping_add(u64::from(r.wrapping_mul(r) < d2)) as u32
    }
}

/// The system a new game starts in: the first SECURE one that is not a
/// HAVEN (a quiet place to learn), or system 0.
pub fn start_system(seed: u32) -> u8 {
    (0..SYSTEMS as u8)
        .find(|&i| {
            let s = system(seed, i);
            s.danger == 0 && s.econ != Econ::Haven
        })
        .unwrap_or(0)
}
