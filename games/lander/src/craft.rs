//! The craft: one physics tick, and what happens when it touches ground.

use crate::fixed::{PX, QUARTER, STEP, cos_step, step_of, turn};
use crate::level::{BURN, World};

/// Thrust at full power, Q16 pixels per tick per tick (about 9.6
/// pixels/s^2: half again the strongest gravity, 6.3).
pub const THRUST: i32 = 175;
/// Fastest turn: about 130 degrees a second.
pub const TURN_RATE: i32 = 96 * STEP / 60 + 1;
/// Speed limit on each axis, Q16 pixels per tick (2 pixels a tick).
pub const MAX_SPEED: i32 = 2 * PX;

/// A landing is safe up to these: 7 pixels/s down, 4 sideways, and
/// leaning 8 steps (11 degrees).
pub const SAFE_DOWN: i32 = 7 * PX / 60;
pub const SAFE_SIDE: i32 = 4 * PX / 60;
pub const SAFE_LEAN: i32 = 8 * STEP;

/// Full input on a scale (sticks, triggers and turns all use this).
pub const FULL: i32 = 32767;

/// The craft's outline points for touching ground, in pixels from its
/// middle (upright). The first two are the feet.
const POINTS: [(i32, i32); 9] = [
    (-4, 4),
    (4, 4),
    (-3, 2),
    (3, 2),
    (-3, -1),
    (3, -1),
    (-2, -3),
    (2, -3),
    (0, -4),
];
/// The feet, from the middle, upright.
pub const FOOT_Y: i32 = 4;

/// What the pilot asks for in one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Controls {
    /// Engine power, 0 ..= [`FULL`].
    pub thrust: i32,
    /// Turning speed, -FULL ..= FULL (+ turns right), when `aim` is None.
    pub turn: i32,
    /// Lean towards this angle (Q8) at full turning speed instead.
    pub aim: Option<i32>,
}

/// How a flight ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Touch {
    /// Down safely on a pad (its index).
    Landed(usize),
    /// Broken: why.
    Crashed(Why),
    /// Flew off the side or too high: gone.
    Lost,
}

/// Why a craft broke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// Not both feet on one pad.
    MissedPad,
    /// Down too fast.
    TooFast,
    /// Sideways too fast.
    Sliding,
    /// Leaning too far.
    Tilted,
}

impl Why {
    pub fn text(self) -> &'static str {
        match self {
            Why::MissedPad => "MISSED THE PAD",
            Why::TooFast => "TOO FAST",
            Why::Sliding => "SLID SIDEWAYS",
            Why::Tilted => "NOT UPRIGHT",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Craft {
    /// The middle, Q16 pixels.
    pub x: i32,
    pub y: i32,
    /// Speed, Q16 pixels per tick.
    pub vx: i32,
    pub vy: i32,
    /// Lean, Q8 steps (0 upright, + right), within a quarter turn.
    pub angle: i32,
    /// Fuel left, in [`BURN`] units per full-power tick.
    pub fuel: u32,
    /// Engine power used in the last tick, 0 ..= FULL (for the flame).
    pub burn: i32,
    /// The downward speed when it last touched ground (Q16 per tick).
    pub touch_vy: i32,
}

impl Craft {
    /// At the level's start.
    pub fn new(world: &World) -> Craft {
        Craft {
            x: world.start_x * PX,
            y: 16 * PX,
            vx: world.start_vx,
            vy: 0,
            angle: 0,
            fuel: world.params.fuel,
            burn: 0,
            touch_vy: 0,
        }
    }

    /// Whole pixels of the middle.
    pub fn pixel(&self) -> (i32, i32) {
        (self.x.div_euclid(PX), self.y.div_euclid(PX))
    }

    pub fn step(&self) -> i32 {
        step_of(self.angle)
    }

    /// One tick of flight. Returns how the flight ended, if it did.
    pub fn tick(&mut self, c: Controls, world: &World, wind: i32) -> Option<Touch> {
        // Turning.
        let rate = match c.aim {
            Some(a) => (a.clamp(-QUARTER, QUARTER) - self.angle).clamp(-TURN_RATE, TURN_RATE),
            None => i32::try_from(
                i64::from(c.turn.clamp(-FULL, FULL)) * i64::from(TURN_RATE) / i64::from(FULL),
            )
            .unwrap_or(0),
        };
        self.angle = (self.angle + rate).clamp(-QUARTER, QUARTER);

        // The engine, while there is fuel.
        let mut power = c.thrust.clamp(0, FULL);
        let need = (power as u32 * BURN).div_ceil(FULL as u32);
        if self.fuel == 0 {
            power = 0;
        } else if need > self.fuel {
            power = power * self.fuel as i32 / need as i32;
        }
        self.fuel = self.fuel.saturating_sub(need);
        self.burn = power;
        let push = THRUST * power / FULL; // 0 ..= THRUST
        let (ax, ay) = turn(0, -push, self.step()); // Q14
        let ax = (ax >> 14).saturating_add(wind);
        let ay = (ay >> 14).saturating_add(world.params.gravity);
        self.vx = self.vx.saturating_add(ax).clamp(-MAX_SPEED, MAX_SPEED);
        self.vy = self.vy.saturating_add(ay).clamp(-MAX_SPEED, MAX_SPEED);
        self.x = self.x.saturating_add(self.vx);
        self.y = self.y.saturating_add(self.vy);

        let (cx, cy) = self.pixel();
        let w = i32::from(world.params.width);
        if cx < -8 || cx > w + 8 || cy < -150 {
            return Some(Touch::Lost);
        }
        self.contact(world)
    }

    /// The outline points, turned with the craft, in whole pixels.
    pub fn points(&self) -> [(i32, i32); 9] {
        let s = self.step();
        let (cx, cy) = (self.x, self.y);
        POINTS.map(|(px, py)| {
            let (tx, ty) = turn(px, py, s);
            // Q14 offset to Q16, then whole pixels.
            ((cx + tx * 4).div_euclid(PX), (cy + ty * 4).div_euclid(PX))
        })
    }

    /// Whether the craft touches ground now, and how that ends.
    fn contact(&mut self, world: &World) -> Option<Touch> {
        let pts = self.points();
        if !pts.iter().any(|&(x, y)| y >= world.ground_at(x)) {
            return None;
        }
        self.touch_vy = self.vy;
        let (l, r) = (pts[0], pts[1]);
        let pad = world.pads().iter().position(|p| {
            // Both feet over the pad, the lower one on it, the other
            // at most a few pixels above (a slight lean).
            let y = i32::from(p.y);
            p.holds(l.0) && p.holds(r.0) && l.1.max(r.1) >= y - 1 && l.1.min(r.1) >= y - 4
        });
        let why = if pad.is_none() {
            Some(Why::MissedPad)
        } else if self.angle.abs() > SAFE_LEAN {
            Some(Why::Tilted)
        } else if self.vy > SAFE_DOWN {
            Some(Why::TooFast)
        } else if self.vx.abs() > SAFE_SIDE {
            Some(Why::Sliding)
        } else {
            None
        };
        match (why, pad) {
            (None, Some(i)) => {
                // Settle on the pad: upright, still, feet on the surface.
                let p = world.pads[i];
                self.angle = 0;
                self.vx = 0;
                self.vy = 0;
                self.burn = 0;
                self.y = (i32::from(p.y) - 1 - FOOT_Y) * PX + PX / 2;
                Some(Touch::Landed(i))
            }
            (Some(w), _) => Some(Touch::Crashed(w)),
            (None, None) => Some(Touch::Crashed(Why::MissedPad)),
        }
    }

    /// Height of the feet above the ground under the middle, in pixels
    /// (0 on the ground; can be negative while sinking into it).
    pub fn height(&self, world: &World) -> i32 {
        let (cx, cy) = self.pixel();
        world.ground_at(cx) - 1 - (cy + FOOT_Y)
    }

    /// Up-pointing part of the craft, Q14 (16384 upright).
    pub fn upright(&self) -> i32 {
        cos_step(self.step())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::World;

    fn flat_world() -> World {
        let mut w = World::make(1);
        w.ground = [100; crate::level::MAX_WORLD];
        w.pads[0] = crate::level::Pad {
            x: 60,
            w: 40,
            y: 100,
            mult: 1,
        };
        w.pad_count = 1;
        w.params.gravity = 0;
        w
    }

    fn drop_from(w: &World, x: i32, vy: i32, vx: i32, angle: i32) -> Touch {
        let mut c = Craft {
            x: x * PX,
            y: 90 * PX,
            vx,
            vy,
            angle,
            fuel: 0,
            burn: 0,
            touch_vy: 0,
        };
        for _ in 0..2000 {
            if let Some(t) = c.tick(Controls::default(), w, 0) {
                return t;
            }
            // Hold the speed for the test: no gravity, no wind.
            c.vy = vy;
            c.vx = vx;
        }
        panic!("never touched");
    }

    #[test]
    fn slow_and_upright_on_a_pad_lands() {
        let w = flat_world();
        assert_eq!(drop_from(&w, 80, SAFE_DOWN, 0, 0), Touch::Landed(0));
        assert_eq!(
            drop_from(&w, 80, SAFE_DOWN, SAFE_SIDE, SAFE_LEAN),
            Touch::Landed(0)
        );
    }

    #[test]
    fn each_way_to_crash() {
        let w = flat_world();
        assert_eq!(
            drop_from(&w, 80, SAFE_DOWN + 1, 0, 0),
            Touch::Crashed(Why::TooFast)
        );
        assert_eq!(
            drop_from(&w, 80, SAFE_DOWN, 0, SAFE_LEAN + 1),
            Touch::Crashed(Why::Tilted)
        );
        assert_eq!(
            drop_from(&w, 80, PX / 10, SAFE_SIDE + 1, 0),
            Touch::Crashed(Why::Sliding)
        );
        assert_eq!(
            drop_from(&w, 30, PX / 10, 0, 0),
            Touch::Crashed(Why::MissedPad)
        );
        // One foot over the edge.
        assert_eq!(
            drop_from(&w, 61, PX / 10, 0, 0),
            Touch::Crashed(Why::MissedPad)
        );
        assert_eq!(drop_from(&w, 64, PX / 10, 0, 0), Touch::Landed(0));
    }

    #[test]
    fn flying_off_is_lost() {
        let w = flat_world();
        let mut c = Craft::new(&w);
        c.vx = -MAX_SPEED;
        let mut out = None;
        for _ in 0..400 {
            out = c.tick(
                Controls {
                    thrust: FULL,
                    turn: 0,
                    aim: None,
                },
                &w,
                0,
            );
            if out.is_some() {
                break;
            }
        }
        assert_eq!(out, Some(Touch::Lost));
    }

    #[test]
    fn extreme_controls_are_safe() {
        let w = flat_world();
        let mut c = Craft::new(&w);
        c.fuel = u32::MAX;
        for i in 0..2000 {
            let k = Controls {
                thrust: if i % 2 == 0 { i32::MAX } else { i32::MIN },
                turn: if i % 3 == 0 { i32::MIN } else { i32::MAX },
                aim: if i % 5 == 0 { Some(i32::MIN) } else { None },
            };
            if c.tick(k, &w, i32::MAX / 4).is_some() {
                c = Craft::new(&w);
                c.fuel = u32::MAX;
            }
            assert!(c.angle.abs() <= QUARTER);
        }
    }

    #[test]
    fn fuel_runs_out() {
        let w = flat_world();
        let mut c = Craft::new(&w);
        c.fuel = BURN * 3 / 2;
        let full = Controls {
            thrust: FULL,
            turn: 0,
            aim: None,
        };
        c.tick(full, &w, 0);
        assert_eq!(c.burn, FULL);
        c.tick(full, &w, 0);
        assert!(c.burn > 0 && c.burn < FULL);
        assert_eq!(c.fuel, 0);
        c.tick(full, &w, 0);
        assert_eq!(c.burn, 0);
    }
}
