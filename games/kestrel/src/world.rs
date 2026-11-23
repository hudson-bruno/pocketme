//! One mission in flight, moved in fixed ticks of 1/60 s: the craft,
//! the clock, the hull, the gates, the hits. Nothing here draws.
//!
//! The craft flies forward along +z on its own; the player steers it
//! up, down and sideways inside the corridor, sets the speed (boost,
//! cruise, brake) and can roll aside quickly (a barrel roll) to dodge.

use crate::course::{CORRIDOR_X, CORRIDOR_Y, Kind, MISSIONS, Mission, Things};
use crate::poly::M8;

/// Game time of one tick (60 a second).
pub const TICK_US: u32 = 16_667;
/// Ticks a second.
pub const HZ: u32 = 60;
/// The craft's position units: 1/4096 m (16 times finer than the
/// world's 1/256 m, so slow moves are not lost to rounding).
pub const Q: i32 = 4096;
/// The craft's collision ball (1/256 m): a bit smaller than it looks.
pub const SHIP_R: i32 = 230;
/// Hits the hull takes.
pub const HULL_MAX: u8 = 3;
/// Ticks the craft cannot be hurt again after a hit.
pub const HURT_TICKS: u16 = 90;
/// A barrel roll: its length, and the wait after it.
pub const ROLL_TICKS: u8 = 30;
pub const ROLL_COOL: u8 = 24;
/// Most sideways and up-down speed, m/s.
pub const SIDE_MPS: i32 = 13;
pub const UPDOWN_MPS: i32 = 9;
/// Seconds (tenths) a time cell adds.
pub const CELL_S10: u32 = 20;
/// Points.
pub const GATE_POINTS: u32 = 100;
pub const CELL_POINTS: u32 = 50;
pub const SECOND_POINTS: u32 = 20;
pub const HULL_POINTS: u32 = 150;

/// The player's wishes for one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pad {
    /// Sideways (right +) and up (+), each -256..=256.
    pub steer: (i32, i32),
    /// -256 (full brake) .. 0 (cruise) .. 256 (full boost).
    pub throttle: i32,
    /// A barrel roll asked for: -1 left, 1 right, 0 none.
    pub roll: i8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Flying,
    /// Through the dock gate: the mission is done.
    Delivered,
    OutOfTime,
    /// The hull is gone.
    Lost,
    /// Flew past the dock gate instead of through it.
    MissedDock,
}

impl State {
    /// The name in the log (spelled as `{:?}` spells it).
    pub fn name(self) -> &'static str {
        match self {
            State::Flying => "Flying",
            State::Delivered => "Delivered",
            State::OutOfTime => "OutOfTime",
            State::Lost => "Lost",
            State::MissedDock => "MissedDock",
        }
    }
}

/// What happened in a tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// Through gate `n` (0-based).
    Gate(u8),
    /// Past gate `n`, outside it.
    Missed(u8),
    /// Hit gate `n`'s ring (counts as missed, and hurts).
    Ring(u8),
    /// A rock, wreck or pod hit the hull.
    Hit,
    /// A time cell taken.
    Cell,
    Delivered,
    OutOfTime,
    Lost,
    MissedDock,
}

/// The craft.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ship {
    /// Position in 1/4096 m.
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// Speeds in 1/4096 m a tick.
    pub vx: i32,
    pub vy: i32,
    pub speed: i32,
    /// Barrel roll: ticks left, its way (-1, 1), and the wait after.
    pub roll_left: u8,
    pub roll_dir: i8,
    pub roll_cool: u8,
    /// Ticks it cannot be hurt.
    pub hurt: u16,
}

impl Ship {
    /// Position in world units (1/256 m).
    pub fn pos(&self) -> [i32; 3] {
        [self.x >> 4, self.y >> 4, self.z >> 4]
    }

    /// Yaw, pitch, roll: the craft leans into its moves.
    pub fn angles(&self) -> (u16, u16, u16) {
        let yaw = (self.vx * 3) as u16;
        let pitch = (self.vy * 6) as u16;
        let mut roll = (self.vx * 9) as u16;
        if self.roll_left > 0 {
            let done = u32::from(ROLL_TICKS - self.roll_left);
            let turn = (done * 65_536 / u32::from(ROLL_TICKS)) as u16;
            roll = if self.roll_dir > 0 {
                roll.wrapping_add(turn)
            } else {
                roll.wrapping_sub(turn)
            };
        }
        (yaw, pitch, roll)
    }
}

/// Totals of one mission.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub passed: u8,
    pub missed: u8,
    pub cells: u8,
    pub hits: u8,
    pub rolls: u16,
}

pub struct World {
    /// Mission number (0-based).
    pub mission: u8,
    pub ticks: u32,
    pub ship: Ship,
    /// The camera's sideways and up-down place, following the craft a
    /// little late (1/4096 m).
    pub cam: (i32, i32),
    /// Ticks left on the clock.
    pub clock: u32,
    pub hull: u8,
    /// The gate to fly through next; `gates` is the dock.
    pub next_gate: u8,
    /// Whether each gate was flown through (bit n), for its colour.
    pub gate_ok: u32,
    pub tally: Tally,
    /// Points of this mission so far.
    pub score: u32,
    /// Time cells taken, one bit per course slice.
    taken: [u32; 16],
    pub state: State,
    events: [Option<Event>; 4],
}

impl Default for World {
    fn default() -> Self {
        World::new(0)
    }
}

impl World {
    /// A world before any mission is loaded (a `const`, so the game's
    /// start needs no big value on the stack).
    pub const fn empty() -> World {
        World {
            mission: 0,
            ticks: 0,
            ship: Ship {
                x: 0,
                y: 0,
                z: 0,
                vx: 0,
                vy: 0,
                speed: 0,
                roll_left: 0,
                roll_dir: 0,
                roll_cool: 0,
                hurt: 0,
            },
            cam: (0, 0),
            clock: 0,
            hull: HULL_MAX,
            next_gate: 0,
            gate_ok: 0,
            tally: Tally {
                passed: 0,
                missed: 0,
                cells: 0,
                hits: 0,
                rolls: 0,
            },
            score: 0,
            taken: [0; 16],
            state: State::Flying,
            events: [None; 4],
        }
    }

    pub fn new(mission: usize) -> World {
        let mut w = World::empty();
        w.reset(mission);
        w
    }

    /// Mission `n` (0-based) from its start, in place.
    pub fn reset(&mut self, n: usize) {
        let n = n.min(MISSIONS.len() - 1);
        let m = &MISSIONS[n];
        self.mission = n as u8;
        self.ticks = 0;
        self.ship = Ship {
            x: 0,
            y: 0,
            z: 0,
            vx: 0,
            vy: 0,
            speed: cruise_speed(m),
            roll_left: 0,
            roll_dir: 0,
            roll_cool: 0,
            hurt: 0,
        };
        self.cam = (0, 0);
        self.clock = m.start_s10 * HZ / 10;
        self.hull = HULL_MAX;
        self.next_gate = 0;
        self.gate_ok = 0;
        self.tally = Tally::default();
        self.score = 0;
        self.taken = [0; 16];
        self.state = State::Flying;
        self.events = [None; 4];
    }

    pub fn m(&self) -> &'static Mission {
        &MISSIONS[usize::from(self.mission)]
    }

    /// The next thing that happened, oldest first.
    pub fn take_event(&mut self) -> Option<Event> {
        let e = self.events[0].take();
        self.events.rotate_left(1);
        e
    }

    fn event(&mut self, e: Event) {
        if let Some(s) = self.events.iter_mut().find(|s| s.is_none()) {
            *s = Some(e);
        }
    }

    /// Whether the time cell with this id was taken.
    pub fn is_taken(&self, id: u32) -> bool {
        let slice = (id >> 3) as usize;
        slice < 512 && self.taken[slice / 32] & (1 << (slice % 32)) != 0
    }

    fn take(&mut self, id: u32) {
        let slice = (id >> 3) as usize;
        if slice < 512 {
            self.taken[slice / 32] |= 1 << (slice % 32);
        }
    }

    /// Seconds left on the clock, in tenths (rounded up).
    pub fn clock_s10(&self) -> u32 {
        (self.clock * 10).div_ceil(HZ)
    }

    /// One tick. The state changes to the end states here.
    // Not inlined: its working lists then get a stack frame of their
    // own instead of making the caller's bigger (see README, "Stack").
    #[inline(never)]
    pub fn tick(&mut self, pad: &Pad) {
        if self.state != State::Flying {
            return;
        }
        self.ticks += 1;
        let m = self.m();
        let s = &mut self.ship;

        // Barrel roll.
        if pad.roll != 0 && s.roll_left == 0 && s.roll_cool == 0 {
            s.roll_left = ROLL_TICKS;
            s.roll_dir = pad.roll.signum();
            self.tally.rolls = self.tally.rolls.saturating_add(1);
        }
        let side_max = SIDE_MPS * Q / HZ as i32;
        let up_max = UPDOWN_MPS * Q / HZ as i32;
        let (mut want_x, want_y) = (
            pad.steer.0.clamp(-256, 256) * side_max / 256,
            pad.steer.1.clamp(-256, 256) * up_max / 256,
        );
        if s.roll_left > 0 {
            // A roll throws the craft aside, faster than steering.
            want_x = i32::from(s.roll_dir) * side_max * 3 / 2;
            s.roll_left -= 1;
            if s.roll_left == 0 {
                s.roll_cool = ROLL_COOL;
            }
        } else {
            s.roll_cool = s.roll_cool.saturating_sub(1);
        }
        s.vx += (want_x - s.vx) / 5;
        s.vy += (want_y - s.vy) / 5;
        s.x += s.vx;
        s.y += s.vy;
        let (lx, ly) = (CORRIDOR_X * 16, CORRIDOR_Y * 16);
        if s.x.abs() > lx {
            s.x = s.x.clamp(-lx, lx);
            s.vx = 0;
        }
        if s.y.abs() > ly {
            s.y = s.y.clamp(-ly, ly);
            s.vy = 0;
        }
        // Speed: cruise, boost up to half again, brake to a little more
        // than half.
        let base = cruise_speed(m);
        let t = pad.throttle.clamp(-256, 256);
        let want = if t >= 0 {
            base + base * t / 512
        } else {
            base + base * t * 115 / 65_536
        };
        s.speed += (want - s.speed) / 24;
        let z0 = s.z;
        s.z += s.speed;
        let z1 = s.z;
        s.hurt = s.hurt.saturating_sub(1);
        // The camera follows, a little late.
        self.cam.0 += (s.x - self.cam.0) / 5;
        self.cam.1 += (s.y - self.cam.1) / 5;

        // The clock.
        self.clock = self.clock.saturating_sub(1);

        // The next gate: through, past, or into its ring?
        let g = m.gate(self.next_gate, self.ticks);
        if (z0 >> 4) < g.z && (z1 >> 4) >= g.z {
            let p = self.ship.pos();
            let (dx, dy) = (i64::from(p[0] - g.x), i64::from(p[1] - g.y));
            let d2 = dx * dx + dy * dy;
            let hole = i64::from(g.inner() - SHIP_R / 2);
            let ring = i64::from(g.outer() + SHIP_R / 2);
            let n = self.next_gate;
            if d2 < hole * hole {
                self.gate_ok |= 1 << n;
                if g.dock {
                    self.finish(State::Delivered);
                    return;
                }
                self.tally.passed += 1;
                self.score += GATE_POINTS;
                self.clock += m.bonus_s10 * HZ / 10;
                self.event(Event::Gate(n));
            } else {
                if g.dock {
                    self.finish(State::MissedDock);
                    return;
                }
                self.tally.missed += 1;
                if d2 < ring * ring {
                    self.event(Event::Ring(n));
                    self.hurt();
                } else {
                    self.event(Event::Missed(n));
                }
            }
            self.next_gate += 1;
        }

        // Rocks, wrecks, pods and time cells near the craft.
        let p = self.ship.pos();
        let c = Mission::cell_of(p[2]);
        for slice in c - 1..=c + 1 {
            let mut list = Things::new();
            m.things(slice, self.ticks, &mut list);
            for t in list.as_slice() {
                if !t.touches(p[0], p[1], p[2], SHIP_R) {
                    continue;
                }
                if t.kind == Kind::Cell {
                    if !self.is_taken(t.id) {
                        self.take(t.id);
                        self.tally.cells += 1;
                        self.score += CELL_POINTS;
                        self.clock += CELL_S10 * HZ / 10;
                        self.event(Event::Cell);
                    }
                } else if t.hurts() && self.ship.hurt == 0 {
                    self.event(Event::Hit);
                    self.hurt();
                }
            }
        }
        if self.state != State::Flying {
            return;
        }
        if self.clock == 0 {
            self.finish(State::OutOfTime);
        }
    }

    fn hurt(&mut self) {
        if self.ship.hurt > 0 {
            return;
        }
        self.tally.hits += 1;
        self.hull = self.hull.saturating_sub(1);
        self.ship.hurt = HURT_TICKS;
        self.ship.speed /= 2;
        if self.hull == 0 {
            self.finish(State::Lost);
        }
    }

    fn finish(&mut self, s: State) {
        self.state = s;
        if s == State::Delivered {
            self.score +=
                self.clock_s10() / 10 * SECOND_POINTS + u32::from(self.hull) * HULL_POINTS;
        }
        self.event(match s {
            State::Delivered => Event::Delivered,
            State::OutOfTime => Event::OutOfTime,
            State::Lost => Event::Lost,
            State::MissedDock => Event::MissedDock,
            State::Flying => return,
        });
    }

    /// How far along the course the craft is, 0..=256.
    pub fn progress(&self) -> i32 {
        let end = self.m().dock_z();
        ((self.ship.z >> 4) * 256 / end.max(1)).clamp(0, 256)
    }
}

/// Cruise speed in 1/4096 m a tick.
pub fn cruise_speed(m: &Mission) -> i32 {
    m.cruise * Q / HZ as i32
}

/// The metre in world units, for other modules.
pub const METRE: i32 = M8;
