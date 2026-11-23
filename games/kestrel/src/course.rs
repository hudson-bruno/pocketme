//! The three missions and everything along their courses.
//!
//! A course runs along +z. The craft starts at z = 0 and flies forward;
//! gates stand across the way, and the dock gate in front of HALDEN
//! station ends the course. Nothing is stored: every rock, wreck, pod
//! and time cell is worked out from the mission's number and its place
//! (the course is cut into [`CELL_M`]-metre slices; a slice's contents
//! come from a hash of the slice's number). So the same mission is
//! always the same course, and the game needs no memory for it.
//!
//! Moving things (tumbling rocks, swinging pods, sliding gates) move as
//! a function of the mission's tick count, so a replay is exact.
//!
//! Units: positions in 1/256 m ([`M8`]), time in ticks of 1/60 s.

use crate::fixed::{Dice, deg, hash, sin};
use crate::models::{GATE_INNER, GATE_OUTER, WRECK_BOX};
use crate::poly::M8;

/// Length of one slice of the course.
pub const CELL_M: i32 = 10;
/// The craft's corridor: it cannot leave |x| <= this, |y| <= `CORRIDOR_Y`.
pub const CORRIDOR_X: i32 = 14 * M8;
pub const CORRIDOR_Y: i32 = 9 * M8;
/// Where the first gate is.
pub const FIRST_GATE: i32 = 120 * M8;
/// Most things in one slice.
pub const MAX_IN_CELL: usize = 4;

/// One mission's rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mission {
    pub name: &'static str,
    /// What the briefing says.
    pub brief: [&'static str; 2],
    /// Gates before the dock.
    pub gates: u8,
    /// Metres between gates.
    pub gap_m: i32,
    /// Seconds on the clock at the start, and added for each gate
    /// flown through (in tenths of a second).
    pub start_s10: u32,
    pub bonus_s10: u32,
    /// Cruising speed, m/s.
    pub cruise: i32,
    /// Chance of a rock in a slice, in 1/256 (two dice per slice).
    pub rocks: u32,
    /// Biggest rock radius, 1/256 m.
    pub rock_max: i32,
    /// How far rocks drift to and fro (1/256 m).
    pub drift: i32,
    /// A wreck every this many slices (0: none).
    pub wreck_every: i32,
    /// A swinging cargo pod every this many slices (0: none).
    pub pod_every: i32,
    /// How far gates slide to and fro (1/256 m; 0: they stand still).
    pub gate_slide: i32,
    /// How far from the middle gates may stand (1/256 m), x and y.
    pub spread: (i32, i32),
    /// Gate size (256 = the model's).
    pub gate_scale: i32,
    pub seed: u32,
}

pub static MISSIONS: [Mission; 3] = [
    Mission {
        name: "THE SHALLOWS",
        brief: ["THIN ICE AND STONE.", "LEARN THE CRAFT."],
        gates: 8,
        gap_m: 110,
        start_s10: 240,
        bonus_s10: 40,
        cruise: 22,
        rocks: 120,
        rock_max: 560,
        drift: 0,
        wreck_every: 0,
        pod_every: 0,
        gate_slide: 0,
        spread: (7 * M8, 4 * M8),
        gate_scale: 300,
        seed: 0x51A1,
    },
    Mission {
        name: "HULL GRAVEYARD",
        brief: ["OLD HAULERS LIE ACROSS", "THE WAY. PODS SWING."],
        gates: 10,
        gap_m: 105,
        start_s10: 170,
        bonus_s10: 40,
        cruise: 24,
        rocks: 130,
        rock_max: 600,
        drift: 2 * M8,
        wreck_every: 9,
        pod_every: 13,
        gate_slide: 0,
        spread: (8 * M8, 5 * M8),
        gate_scale: 276,
        seed: 0x6B2C,
    },
    Mission {
        name: "THE SIEVE",
        brief: ["DENSE ROCK, MOVING GATES.", "THE CLOCK IS TIGHT."],
        gates: 12,
        gap_m: 100,
        start_s10: 150,
        bonus_s10: 36,
        cruise: 26,
        rocks: 170,
        rock_max: 640,
        drift: 3 * M8,
        wreck_every: 14,
        pod_every: 8,
        gate_slide: 3 * M8,
        spread: (8 * M8, 5 * M8),
        gate_scale: 256,
        seed: 0x7C3D,
    },
];

impl Mission {
    /// Where gate `i` stands (0-based; `self.gates` is the dock), at
    /// tick `t`.
    pub fn gate(&self, i: u8, t: u32) -> Gate {
        let z = FIRST_GATE + i32::from(i) * self.gap_m * M8;
        if i >= self.gates {
            return Gate {
                x: 0,
                y: 0,
                z,
                scale: DOCK_SCALE,
                dock: true,
            };
        }
        let h = hash(self.seed, 1000 + u32::from(i));
        let x = (h % (2 * self.spread.0 as u32 + 1)) as i32 - self.spread.0;
        let y = ((h >> 12) % (2 * self.spread.1 as u32 + 1)) as i32 - self.spread.1;
        let slide = if self.gate_slide > 0 {
            let phase = (h >> 20) as u16;
            (self.gate_slide * sin(phase.wrapping_add(t.wrapping_mul(300) as u16))) >> 14
        } else {
            0
        };
        Gate {
            x: (x + slide).clamp(-CORRIDOR_X + 3 * M8, CORRIDOR_X - 3 * M8),
            y,
            z,
            scale: self.gate_scale,
            dock: false,
        }
    }

    /// Where the dock gate stands (the course ends there).
    pub fn dock_z(&self) -> i32 {
        FIRST_GATE + i32::from(self.gates) * self.gap_m * M8
    }

    /// The station, behind the dock gate.
    pub fn station_z(&self) -> i32 {
        self.dock_z() + 40 * M8
    }

    /// The slice a z position is in.
    pub fn cell_of(z: i32) -> i32 {
        z.div_euclid(CELL_M * M8)
    }

    /// Everything in slice `c` at tick `t`.
    pub fn things(&self, c: i32, t: u32, out: &mut Things) {
        out.n = 0;
        // Nothing just after the start or after the dock.
        if c < 6 || c * CELL_M * M8 > self.dock_z() - 15 * M8 {
            return;
        }
        let mut d = Dice(hash(self.seed, c as u32));
        let z0 = c * CELL_M * M8;
        // The nearest gate to this slice: things keep out of its hole.
        let gi = ((z0 - FIRST_GATE + self.gap_m * M8 / 2) / (self.gap_m * M8)).clamp(0, 255) as u8;
        let near_gate = self.gate(gi, t);
        let gate_close = (z0 + CELL_M * M8 / 2 - near_gate.z).abs() < 16 * M8;

        // Wrecks: big, lying across one side of the way.
        if self.wreck_every > 0 && c % self.wreck_every == 3 && !gate_close {
            let side = if d.below(2) == 0 { -1 } else { 1 };
            let across = d.below(3) != 0;
            let x = side * d.range(5 * M8, 9 * M8);
            let y = d.range(-5 * M8, 4 * M8);
            out.push(Thing {
                kind: Kind::Wreck,
                x,
                y,
                z: z0 + 5 * M8,
                size: 256,
                angles: (if across { deg(90) } else { 0 }, 0, 0),
                id: (c as u32) << 3,
            });
        }
        // Pods swing across the whole way.
        if self.pod_every > 0 && c % self.pod_every == 5 && !gate_close {
            let phase = d.roll() as u16;
            let speed = 160 + d.below(120);
            let a = phase.wrapping_add(t.wrapping_mul(speed) as u16);
            let x = ((CORRIDOR_X + 2 * M8) * sin(a)) >> 14;
            let y = d.range(-6 * M8, 6 * M8);
            out.push(Thing {
                kind: Kind::Pod,
                x,
                y,
                z: z0 + 4 * M8,
                size: 256,
                angles: (0, 0, t.wrapping_mul(500) as u16),
                id: ((c as u32) << 3) | 1,
            });
        }
        // Rocks: up to two a slice.
        for k in 0..2u32 {
            if d.below(256) >= self.rocks {
                continue;
            }
            let size = d.range(self.rock_max / 3, self.rock_max);
            let mut x = d.range(-CORRIDOR_X - 3 * M8, CORRIDOR_X + 3 * M8);
            let mut y = d.range(-CORRIDOR_Y - 2 * M8, CORRIDOR_Y + 2 * M8);
            let z = z0 + d.range(0, CELL_M * M8 - 1);
            let spin = (d.roll() as u16, d.roll() as u16);
            let rate = (d.range(-90, 90), d.range(-90, 90));
            if self.drift > 0 {
                let ph = d.roll() as u16;
                x += (self.drift * sin(ph.wrapping_add(t.wrapping_mul(120) as u16))) >> 14;
            }
            // Keep the gate's hole and a little round it clear.
            if gate_close {
                let (dx, dy) = (x - near_gate.x, y - near_gate.y);
                let keep = near_gate.outer() + size + M8;
                if dx.abs() < keep && dy.abs() < keep {
                    // Pushed out sideways, past the ring.
                    x = near_gate.x + if dx < 0 { -keep } else { keep };
                    y = y.clamp(-CORRIDOR_Y - 2 * M8, CORRIDOR_Y + 2 * M8);
                }
            }
            let look = d.below(2) as u8;
            out.push(Thing {
                kind: Kind::Rock(look),
                x,
                y,
                z,
                size,
                angles: (
                    spin.0.wrapping_add((t as i32).wrapping_mul(rate.0) as u16),
                    spin.1.wrapping_add((t as i32).wrapping_mul(rate.1) as u16),
                    0,
                ),
                id: ((c as u32) << 3) | (2 + k),
            });
        }
        // A time cell now and then, off the straight line.
        if c % 4 == 2 && !gate_close {
            let x = d.range(-9 * M8, 9 * M8);
            let y = d.range(-6 * M8, 6 * M8);
            out.push(Thing {
                kind: Kind::Cell,
                x,
                y,
                z: z0 + 5 * M8,
                size: 256,
                angles: (t.wrapping_mul(700) as u16, 0, 0),
                id: ((c as u32) << 3) | 4,
            });
        }
    }
}

/// The dock gate's size (256 = the gate model's).
pub const DOCK_SCALE: i32 = 420;

/// A gate where it stands now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gate {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub scale: i32,
    pub dock: bool,
}

impl Gate {
    /// The hole's radius (the ring's inside), 1/256 m.
    pub fn inner(&self) -> i32 {
        GATE_INNER * self.scale / 256
    }

    /// The ring's outside.
    pub fn outer(&self) -> i32 {
        GATE_OUTER * self.scale / 256
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A rock; which of two looks.
    Rock(u8),
    Wreck,
    Pod,
    Cell,
}

/// One thing on the course, where it is now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Thing {
    pub kind: Kind,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// Rock radius (1/256 m); 256 for the others.
    pub size: i32,
    /// Yaw, pitch, roll.
    pub angles: (u16, u16, u16),
    /// Slice << 3 | number in the slice: the same thing has the same id.
    pub id: u32,
}

impl Thing {
    /// Whether a ball at (x, y, z) with radius `r` touches this thing.
    pub fn touches(&self, x: i32, y: i32, z: i32, r: i32) -> bool {
        match self.kind {
            Kind::Wreck => {
                // A box; across means turned a quarter (x and z swap).
                let across = self.angles.0 != 0;
                let (hx, hz) = if across {
                    (WRECK_BOX[3], WRECK_BOX[0])
                } else {
                    (WRECK_BOX[0], WRECK_BOX[3])
                };
                let dx = (x - self.x).abs() - hx;
                let dz = (z - self.z).abs() - hz;
                let dy = if y < self.y {
                    self.y - WRECK_BOX[1] - y
                } else {
                    y - (self.y + WRECK_BOX[2])
                };
                let (dx, dy, dz) = (dx.max(0), dy.max(0), dz.max(0));
                i64::from(dx) * i64::from(dx)
                    + i64::from(dy) * i64::from(dy)
                    + i64::from(dz) * i64::from(dz)
                    < i64::from(r) * i64::from(r)
            }
            _ => {
                let reach = match self.kind {
                    // Rocks are a little smaller than they look: fair.
                    Kind::Rock(_) => self.size * 7 / 8,
                    Kind::Pod => 230,
                    // Time cells are easy to catch.
                    _ => 300,
                } + r;
                let (dx, dy, dz) = (
                    i64::from(x - self.x),
                    i64::from(y - self.y),
                    i64::from(z - self.z),
                );
                dx * dx + dy * dy + dz * dz < i64::from(reach) * i64::from(reach)
            }
        }
    }

    /// Whether this thing can hurt the craft.
    pub fn hurts(&self) -> bool {
        !matches!(self.kind, Kind::Cell)
    }
}

/// Things of one slice.
#[derive(Clone, Copy, Debug)]
pub struct Things {
    pub list: [Thing; MAX_IN_CELL],
    pub n: usize,
}

impl Default for Things {
    fn default() -> Self {
        Things::new()
    }
}

impl Things {
    pub const fn new() -> Things {
        Things {
            list: [Thing {
                kind: Kind::Cell,
                x: 0,
                y: 0,
                z: 0,
                size: 0,
                angles: (0, 0, 0),
                id: 0,
            }; MAX_IN_CELL],
            n: 0,
        }
    }

    fn push(&mut self, t: Thing) {
        if self.n < MAX_IN_CELL {
            self.list[self.n] = t;
            self.n += 1;
        }
    }

    pub fn as_slice(&self) -> &[Thing] {
        &self.list[..self.n]
    }
}
