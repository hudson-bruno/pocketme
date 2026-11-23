//! The rooms: small tables of tiles, one letter per 8 x 8 tile.
//!
//! A room is 14 tiles high (112 pixels, the picture under the 8-pixel
//! bar at the top) and 20 tiles wide (one screen) or wider (the view
//! scrolls). The tables are constants: on the chip they sit in the
//! game's data slot, one byte per tile.
//!
//! | Letter | Tile |
//! |---|---|
//! | `.` | air |
//! | `#` | brick roof: solid |
//! | `M` | metal (vents, tanks, ducts): solid |
//! | `=` | awning: stand on it, jump up through it |
//! | `^` `v` `<` `>` | broken glass pointing up, down, left, right: a cut sends you back |
//! | `S` | where the runner starts (air) |
//! | `D` | the door: reach it to finish the room |
//! | `a` | an aerial (just a picture, air) |
//! | `c` | a chimney top (solid) |
//!
//! The bottom edge is open: falling off it is a fall. The sides and the
//! top are walls.

/// A tile's size in pixels.
pub const TILE: i32 = 8;
/// The room's height in tiles.
pub const ROWS: usize = 14;
/// The view: one screen of room, under the bar.
pub const VIEW_W: i32 = 160;
pub const VIEW_H: i32 = ROWS as i32 * TILE;

pub const AIR: u8 = b'.';
pub const BRICK: u8 = b'#';
pub const METAL: u8 = b'M';
pub const AWNING: u8 = b'=';
pub const GLASS_UP: u8 = b'^';
pub const GLASS_DOWN: u8 = b'v';
pub const GLASS_LEFT: u8 = b'<';
pub const GLASS_RIGHT: u8 = b'>';
pub const START: u8 = b'S';
pub const DOOR: u8 = b'D';
pub const AERIAL: u8 = b'a';
pub const CHIMNEY: u8 = b'c';

/// The letters a room may use.
pub const LETTERS: &[u8] = b".#M=^v<>SDac";

pub fn is_solid(t: u8) -> bool {
    matches!(t, BRICK | METAL | CHIMNEY)
}

/// The sharp part of a glass tile, as (x, y, w, h) inside the tile. The
/// shards fill half the tile, on the side they stand on.
pub fn glass_rect(t: u8) -> Option<(i32, i32, i32, i32)> {
    match t {
        GLASS_UP => Some((1, 4, 6, 4)),
        GLASS_DOWN => Some((1, 0, 6, 4)),
        GLASS_RIGHT => Some((0, 1, 4, 6)),
        GLASS_LEFT => Some((4, 1, 4, 6)),
        _ => None,
    }
}

/// One room.
pub struct Room {
    pub name: &'static str,
    /// What the room teaches, for the README and the tests.
    pub idea: &'static str,
    /// [`ROWS`] rows of tiles, all the same length.
    pub rows: [&'static [u8]; ROWS],
}

impl Room {
    /// Width in tiles.
    pub fn cols(&self) -> i32 {
        self.rows[0].len() as i32
    }

    pub fn w_px(&self) -> i32 {
        self.cols() * TILE
    }

    pub fn h_px(&self) -> i32 {
        VIEW_H
    }

    /// Wider than one screen: the view scrolls.
    pub fn scrolls(&self) -> bool {
        self.w_px() > VIEW_W
    }

    /// The tile at a column and row; air outside the room.
    pub fn at(&self, col: i32, row: i32) -> u8 {
        if row < 0 || col < 0 || row >= ROWS as i32 {
            return AIR;
        }
        self.rows[row as usize]
            .get(col as usize)
            .copied()
            .unwrap_or(AIR)
    }

    /// The start tile (column, row); the top-left corner if none.
    pub fn start(&self) -> (i32, i32) {
        for (r, row) in self.rows.iter().enumerate() {
            if let Some(c) = row.iter().position(|&t| t == START) {
                return (c as i32, r as i32);
            }
        }
        (0, 0)
    }

    /// Does any tile under the pixel box `(x, y, w, h)` pass `test`?
    pub fn any_in(&self, x: i32, y: i32, w: i32, h: i32, test: impl Fn(u8) -> bool) -> bool {
        let c0 = x.div_euclid(TILE);
        let c1 = (x + w - 1).div_euclid(TILE);
        let r0 = y.div_euclid(TILE);
        let r1 = (y + h - 1).div_euclid(TILE);
        (r0..=r1).any(|r| (c0..=c1).any(|c| test(self.at(c, r))))
    }
}

/// The rooms, in order. Each one-screen room teaches one idea; the
/// later ones put them together; one is three screens wide.
pub static ROOMS: [Room; 8] = [
    Room {
        name: "FIRST ROOF",
        idea: "run and jump; the gaps drop to the street",
        rows: [
            b"....................",
            b"....................",
            b"....................",
            b"....................",
            b"....................",
            b"....................",
            b"....................",
            b"..................D.",
            b"..........a.......D.",
            b".........###...#####",
            b"S.......####...#####",
            b"#####...####...#####",
            b"#####...####...#####",
            b"#####...####...#####",
        ],
    },
    Room {
        name: "BROKEN GLASS",
        idea: "glass cuts; a short press makes a short hop under the low duct",
        rows: [
            b"....................",
            b"....................",
            b"....................",
            b".............M......",
            b".............M......",
            b".............M......",
            b".............M......",
            b"......MMMMMMMM......",
            b"......vvvvvvvv......",
            b"..................D.",
            b"S...^.....^....^^.D.",
            b"####################",
            b"####################",
            b"####################",
        ],
    },
    Room {
        name: "CHIMNEY",
        idea: "slide down a wall and jump off it; climb between two walls",
        rows: [
            b"....................",
            b"....................",
            b"..............D.....",
            b"...a..........D.....",
            b"#########...########",
            b"#########...########",
            b"#########...########",
            b"#########...########",
            b"#########...########",
            b"#########...########",
            b"#########...########",
            b"#########...########",
            b"#########.S.########",
            b"####################",
        ],
    },
    Room {
        name: "DASH",
        idea: "one dash in the air; landing gives it back",
        rows: [
            b"....................",
            b"....................",
            b"....................",
            b"....................",
            b"....................",
            b"....................",
            b"....................",
            b"....................",
            b"..................D.",
            b"..................D.",
            b"S................###",
            b"###......###.....###",
            b"###......###.....###",
            b"###......###.....###",
        ],
    },
    Room {
        name: "UPDRAFT",
        idea: "dash up, and up and across",
        rows: [
            b"....................",
            b"....................",
            b"..................D.",
            b"..................D.",
            b"..............######",
            b"..............######",
            b"......####....######",
            b"......####....######",
            b"......####....######",
            b"......####....######",
            b"S.....####....######",
            b"##########....######",
            b"##########....######",
            b"##########....######",
        ],
    },
    Room {
        name: "AWNING",
        idea: "an awning gives the dash back: dash to it, then dash again",
        rows: [
            b"..................D.",
            b"..................D.",
            b"..................##",
            b"..................##",
            b"..................##",
            b"##............==..##",
            b"##...####.........##",
            b"##...####.........##",
            b"##...####.........##",
            b"##...####.........##",
            b"##...####.........##",
            b"##.S.####.........##",
            b"#########^^^^^^^^^##",
            b"####################",
        ],
    },
    Room {
        name: "LONG ROOF",
        idea: "everything so far, three screens wide: the view scrolls",
        rows: [
            b"...............................................MMMM.........",
            b"...............................................MMMM.........",
            b"...............................................MMMM.......D.",
            b"...............................................MMMM....a..D.",
            b"...............................................MMMM...######",
            b"...............................................MMMM...######",
            b"..............................#####.....==.....MMMM...######",
            b"..............MMM.............#####............MMMM...######",
            b"..............vvv.............#####............MMMM...######",
            b".S...a........................#####...................######",
            b"########.......^..a.......#########...................######",
            b"########...#########......#########........#################",
            b"########...#########^^^^^^#########^^^^^^^^#################",
            b"########...#################################################",
        ],
    },
    Room {
        name: "LAST ROOF",
        idea: "all of it, tight: a low hop with a dash, a narrow landing, glass on the wall",
        rows: [
            b"##..................",
            b"##................D.",
            b"##......MMMMM.....D.",
            b"##......vvvvv...####",
            b"##..............####",
            b"##..............####",
            b"##...###.......<####",
            b"##...###.......<####",
            b"##...###.....##<####",
            b"##...###.....##<####",
            b"##...###.....##<####",
            b"##.S.###.....##<####",
            b"########^^^^^##^####",
            b"####################",
        ],
    },
];
