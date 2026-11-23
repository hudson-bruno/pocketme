//! The levels, as tables of letters (one letter a map cell, rows from
//! north to south).
//!
//! | Letter | What |
//! |---|---|
//! | `#` | rock wall |
//! | `B` | rock with timber props |
//! | `C` | rock with a crystal vein (only a look) |
//! | `M` | iron panel (machine rooms) |
//! | `D` | a door: walk up to it and press the use button |
//! | `G` `H` | a gate: opened by the switch of its colour, never by hand |
//! | `S` `T` | a switch on the wall: `S` opens every `G`, `T` every `H` |
//! | `L` | the lift: the way out, once the crank is found |
//! | `.` | floor |
//! | `>` `v` `<` `^` | the start, looking east, south, west or north |
//! | `*` | a crystal (points) |
//! | `o` | a can of lamp oil |
//! | `+` | a flask (one heart back) |
//! | `k` | the lift's crank |
//! | `m` | a moth: flies to the lamp and drinks its oil |
//! | `w` | a crawler: walks to the player and bites |
//! | `d` `q` | a digger: a mining machine on a fixed track, east-west (`d`) or north-south (`q`) |
//!
//! Every level is closed by walls all round (a test checks it), so no
//! ray and no walker can leave the map.

/// The largest map side.
pub const MAX_SIDE: usize = 32;
/// Cells in the largest map.
pub const MAX_CELLS: usize = MAX_SIDE * MAX_SIDE;

/// One level.
pub struct Level {
    pub name: &'static str,
    pub rows: &'static [&'static [u8]],
}

impl Level {
    pub fn w(&self) -> usize {
        self.rows[0].len()
    }

    pub fn h(&self) -> usize {
        self.rows.len()
    }
}

pub static LEVELS: [Level; 4] = [
    Level {
        name: "OLD ENTRANCE",
        rows: &[
            b"############",
            b"#>..*#*...*#",
            b"#.##.#.##B.#",
            b"#.##.D.#m..#",
            b"#....#.#.BB#",
            b"##B.##.D...#",
            b"#o..#..#B#.#",
            b"#.#.#*.#k..#",
            b"#.#.####.###",
            b"#.*........L",
            b"#.####*C####",
            b"############",
        ],
    },
    Level {
        name: "TIMBER HALLS",
        rows: &[
            b"################",
            b"#>....#....*...#",
            b"#.##..D..w.....#",
            b"#.##..#.....##.#",
            b"#*....###B###..#",
            b"###.###.....#..#",
            b"#o..#...*m..D..#",
            b"#.###.#####.#.*#",
            b"#...S.#..*#.####",
            b"#.#####.###.#..#",
            b"#..w......#.G..#",
            b"#B#.####..#.#k.#",
            b"#...#*..w.#.#..#",
            b"#.###.###.####D#",
            b"#*......+.#*...L",
            b"################",
        ],
    },
    Level {
        name: "CRYSTAL DEEP",
        rows: &[
            b"########################",
            b"#>...#.....*#....m.....#",
            b"#....D.....w#.CCC.####.#",
            b"#..*.#......D...C.#**#.#",
            b"##.###BBB.###.#...#..#.#",
            b"#....#..#...#.#.###..#.#",
            b"#.o..#..#.m.#.#......D.#",
            b"#....#..#...#.##C#####.#",
            b"####.####D###.#...*..#.#",
            b"#*.....w.....d.....#.#.#",
            b"#.#####.#.####.###.#.#.#",
            b"#.#..+#.#.#..#.#*..#...#",
            b"#.#...#.#.#..#.#.###H###",
            b"#.##D##.#.##.#.#.#.....#",
            b"#.....#.#.S#.#...#..k..#",
            b"#####.#.####.#####.....#",
            b"#*..#.#....#...q.#M#G#M#",
            b"#...#.####.###...#.#.#.#",
            b"#.w.#....#...#...#.....#",
            b"#...####.#.T.#.*.#MMM.M#",
            b"##D######.####.###*m...#",
            b"#.....o...........#M...#",
            b"#*......m..C...*..D...L#",
            b"########################",
        ],
    },
    Level {
        name: "DEEP WORKINGS",
        rows: &[
            b"################################",
            b"#>....#...........#.....*......#",
            b"#.....D...w.......D.........m..#",
            b"#.....#...........#............#",
            b"###.######B#B#B####.######D#####",
            b"#*...#.........#..........#....#",
            b"#....#..m...*..#.d........#.o..#",
            b"#....#.........#..........#....#",
            b"#.o..###.###D###.###.######....#",
            b"#....#.....#.....#.....#..#B##.#",
            b"##D###..w..#..*..#..+..#..#*...#",
            b"#....#.....#.....#.....#..D....#",
            b"#.*..###.###.###.###G###..#....#",
            b"#..............................#",
            b"#....#.CCC.#.#.....#.#.#..#....#",
            b"#....#.C*C.#.#..w..#.#S#..######",
            b"###D##.C.C.#.#.....#.###..#....#",
            b"#....#.....#.###.###......D.m..#",
            b"#.m..#.....#...#.#...q....#....#",
            b"#....###D###...#.#........#....#",
            b"#..........*...#.#.MMMMMM.##H###",
            b"#.######.#####.#.#.M*...M.#....#",
            b"#.#..T.#.#.....#.#.M..w.M.#.k..#",
            b"#.#....#.#.###.#.#.MM.MMM.#....#",
            b"#.##.###.#.#*#.#.#........#....#",
            b"#......#.#.#.#.#.###D######M.M##",
            b"#.w....#.#...#.#.#.....#.......#",
            b"######.#.#####.#.#..*..#..d....#",
            b"#*.....#.......#.#.....#.......#",
            b"#.o....D...m...#.#.....#....+..#",
            b"#......#.......#...*...#.......L",
            b"################################",
        ],
    },
];
