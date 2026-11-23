//! The car (the spec's section 1): one step of its rules, in the
//! spec's order, in whole numbers.
//!
//! Where these rules differ from the spec's words, the reason and the
//! sums are beside the rule and in the report (kept outside git):
//!
//! * directions use game-3d's yaw (forward `(sin, cos)`, right
//!   `(cos, -sin)`); the spec wrote `f = (cos, sin)`, which is the same
//!   rule with the angle measured from another axis;
//! * the engine's step is rounded up, and the forward drag is scaled
//!   by the throttle while the car is under the surface's top speed, so
//!   full throttle settles at `TOP` exactly (with the spec's rule as
//!   written, full throttle on gravel settles at 17.7 m/s, not 30);
//! * the grip's step is rounded up, so a slide ends exactly;
//! * reversing, rolling to a stop and the edge's push have DEFAULT
//!   rules (the spec names them, gives no numbers);
//! * the engine adds nothing while the brake is pressed at least as
//!   hard as the throttle (review F2: with `ACC` 41 against the brake's
//!   `CAP` 31, both pedals held drove a standing car to 7.3 m/s);
//! * with the throttle held, the steering turns the nose as if the car
//!   moved at least `TURN_FLOOR` forward (review F3: a car stopped
//!   against the block's edge or a thing could not turn away);
//! * the edge takes its share of the speed only on a real hit (more
//!   than `EDGE_HIT_MIN` outward), not on every step the car leans on it,
//!   and keeps the speed along the edge (review F3);
//! * power out of a slide (R2, the owner's rule, 2026-11-22): the
//!   throttle held in a slide raises the grip and its cap and turns part
//!   of the sideways speed taken into speed along the nose. Why it was
//!   needed: the steering turns the nose at its own rate (up to 90
//!   degrees a second) whatever the grip, while the path follows the
//!   nose only as fast as the cap takes the sideways speed away (gravel
//!   0.7 g), and that speed is lost, not turned; the engine refills the
//!   speed along the nose, so with the throttle held the speed stayed up
//!   and the path widened (v squared over the cap) while more steering
//!   only turned the nose further from the path (`tests/car.rs`,
//!   `power_out_of_a_slide`).

use crate::land::Ground;
use crate::maths::{dir, len2, mul14, right};
use crate::tune::{
    ACC, BRAKE, EDGE, EDGE_HIT_MIN, EDGE_KEEP, G, GRASS, HANDLING, HB, HB_TURN_DEN, HB_TURN_NUM,
    LAND_DIV, LAND_FREE, LAND_MAX, LAND_SIDE, LAND_SIDE_MIN_AIR, LOW_GRASS, POWER_GRIP, POWER_PUSH,
    POWER_SLIDE, RAMP_IN, RAMP_OUT, REV_ACC, REV_TOP, TURN, TURN_FLOOR, VT,
};

/// One step's controls, already turned into the rules' units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Controls {
    /// Wanted steering, -256 (full left) to 256 (full right).
    pub aim: i32,
    /// Throttle and brake, 0 to 256.
    pub throttle: i32,
    pub brake: i32,
    /// The handbrake is held.
    pub handbrake: bool,
}

/// The car's state (the spec's list, plus what the rules keep between
/// steps).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Car {
    /// The middle of the car on the ground (subs).
    pub pos: [i32; 3],
    /// Horizontal speed (subs a step), x and z.
    pub vel: [i32; 2],
    /// Upward speed (subs a step).
    pub vy: i32,
    /// Where the nose points.
    pub yaw: u16,
    /// Steering now, -256 to 256.
    pub steer: i32,
    /// In the air.
    pub air: bool,
    /// Steps in the air so far (0 on the ground).
    pub air_steps: u16,
    /// The surface under the car's middle at this step's start.
    pub surface: u8,
}

/// What happened in a step (for the log, the tests and the drawing).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Events {
    /// The car left the ground this step.
    pub took_off: bool,
    /// The car came down this step, after this many steps in the air,
    /// and lost this much of its speed (of 256).
    pub landed: Option<(u16, i32)>,
    /// The car was pushed back from the block's edge.
    pub edge: bool,
}

impl Car {
    /// A car standing still at (x, z) on the land, pointing along `yaw`.
    pub fn standing<Gr: Ground>(g: &Gr, x: i32, z: i32, yaw: u16) -> Car {
        let s = g.at(x, z);
        Car {
            pos: [x, s.height, z],
            vel: [0, 0],
            vy: 0,
            yaw,
            steer: 0,
            air: false,
            air_steps: 0,
            surface: s.surface,
        }
    }

    /// Forward and sideways speed (subs a step), along the nose and to
    /// its right.
    pub fn fwd_side(&self) -> (i32, i32) {
        split(self.vel, self.yaw)
    }

    /// The horizontal speed (subs a step).
    pub fn speed(&self) -> i32 {
        len2(self.vel[0], self.vel[1])
    }

    /// One step of the rules.
    pub fn step<Gr: Ground>(&mut self, c: &Controls, g: &Gr) -> Events {
        let mut ev = Events::default();

        // 1. Input: the steering moves towards the wanted value.
        self.steer = ramp(self.steer, c.aim.clamp(-256, 256));
        let b = c.brake.clamp(0, 256);
        // The engine adds nothing while the brake is pressed at least as
        // hard (review F2).
        let t_in = c.throttle.clamp(0, 256);
        let t = if b >= t_in { 0 } else { t_in };

        // 2. Surface.
        let here = g.at(self.pos[0], self.pos[2]);
        self.surface = here.surface;
        let h = HANDLING[usize::from(here.surface).min(HANDLING.len() - 1)];

        // 3. Split the speed along the nose (before any turn).
        let (mut fwd, mut side) = split(self.vel, self.yaw);
        let v = len2(self.vel[0], self.vel[1]);
        let old_yaw = self.yaw;

        if !self.air {
            // 4. Steering, with the sign of fwd (reversing turns the
            // other way).
            // With the throttle held and the car not going backwards,
            // at least TURN_FLOOR counts (DEFAULT, review F3), so a car
            // stopped against the edge or a thing can turn away.
            let along = if t > 0 && fwd >= 0 {
                fwd.max(TURN_FLOOR)
            } else {
                fwd.abs()
            };
            let mut turn = self.steer * TURN * along.min(VT) / VT / 256;
            if fwd < 0 {
                turn = -turn;
            }
            if c.handbrake {
                turn = turn * HB_TURN_NUM / HB_TURN_DEN;
            }
            self.yaw = self.yaw.wrapping_add(turn as i16 as u16);

            // 5. Engine and brake.
            // Reverse only with the throttle let go (both pedals hold
            // the car still).
            let reversing = t_in == 0 && b > 0 && fwd <= 0;
            if t > 0 && fwd < h.top {
                // Rounded up: with the floor the car stops short of TOP
                // by TOP / ACC (97.6 % on gravel).
                let num = i64::from(ACC) * i64::from(t) * i64::from(h.top - fwd);
                let den = 256 * i64::from(h.top);
                fwd += ((num + den - 1) / den) as i32;
            }
            if reversing {
                // DEFAULT: the brake held at a standstill drives
                // backwards up to REV_TOP, falling off like the engine.
                if fwd > -REV_TOP {
                    let num = i64::from(REV_ACC) * i64::from(b) * i64::from(REV_TOP + fwd);
                    let den = 256 * i64::from(REV_TOP);
                    fwd -= ((num + den - 1) / den) as i32;
                }
            } else if b > 0 {
                let by = (BRAKE * b / 256).min(h.cap);
                fwd = toward_zero(fwd, by);
            }

            // 6. Grip and slide.
            let (grip, cap) = if c.handbrake {
                (h.grip * HB / 256, h.cap * HB / 256)
            } else {
                (h.grip, h.cap)
            };
            // Power out of a slide (R2): with the throttle held and no
            // handbrake, the slide's share (0 to 256 as the sideways
            // speed reaches POWER_SLIDE of 256 of the speed) times the
            // throttle raises the grip and its cap, and part of the
            // sideways speed taken goes along the nose.
            let power = if c.handbrake || v == 0 || matches!(here.surface, GRASS | LOW_GRASS) {
                0
            } else {
                t * (side.abs() * 256 / v).min(POWER_SLIDE) / POWER_SLIDE
            };
            let grip = grip + grip * power * POWER_GRIP / 65_536;
            let cap = cap + cap * power * POWER_GRIP / 65_536;
            let share = grip * 256 / (256 + v / 64);
            // Rounded up, so a small slide ends instead of staying.
            let take = ((side.abs() * share + 255) / 256).min(cap);
            side = toward_zero(side, take);
            if fwd >= 0 && fwd < h.top {
                fwd = (fwd + take * power * POWER_PUSH / 65_536).min(h.top);
            }
        }

        // 7. Drag, rounded toward 0. On the ground the forward drag is
        // scaled by what the throttle leaves (256 - t) while the car is
        // at or under the surface's top speed: full throttle then
        // settles at TOP (see the module's notes). Above TOP (a fast car
        // driving into the ford) and in the air it applies in full.
        let fwd_drag = if !self.air && fwd.abs() <= h.top {
            h.drag * (256 - t)
        } else {
            h.drag * 256
        };
        let mut lost = (i64::from(fwd) * i64::from(fwd_drag) / 65_536) as i32;
        if !self.air && t == 0 && lost == 0 && fwd != 0 {
            // DEFAULT: rolling with no throttle loses at least 1 sub a
            // step, so a car on gravel (drag 1 of 256) comes to a stop
            // instead of rolling for ever under 1.9 m/s.
            lost = fwd.signum();
        }
        fwd -= lost;
        side -= side * h.drag / 256;

        // 8. Rebuild the speed along the old nose (the split's axes), so
        // turning does not drag the speed round with it.
        let (fx, fz) = dir(old_yaw);
        let (rx, rz) = right(old_yaw);
        self.vel = [
            mul14(fwd, fx) + mul14(side, rx),
            mul14(fwd, fz) + mul14(side, rz),
        ];

        // 9. Height, from the land under the next position.
        let next = g.at(self.pos[0] + self.vel[0], self.pos[2] + self.vel[1]);
        if !self.air {
            if next.height < self.pos[1] + self.vy - G {
                // The land falls away faster than the car can fall.
                self.air = true;
                self.air_steps = 1;
                ev.took_off = true;
                self.vy -= G;
                self.pos[1] += self.vy;
            } else {
                self.vy = next.height - here.height;
                self.pos[1] = next.height;
            }
        } else {
            self.vy -= G;
            let y = self.pos[1] + self.vy;
            if y <= next.height {
                ev.landed = Some((self.air_steps, self.land(next.height, here.height)));
            } else {
                self.pos[1] = y;
                self.air_steps = self.air_steps.saturating_add(1);
            }
        }

        // 10. Position.
        self.pos[0] += self.vel[0];
        self.pos[2] += self.vel[1];
        if ev.landed.is_some() {
            // The landing's loss of speed shortened this step's move:
            // the car stands on the land where it is now.
            self.pos[1] = g.at(self.pos[0], self.pos[2]).height;
        }

        // 11. Edge.
        ev.edge = self.keep_inside(g);
        if ev.edge {
            self.settle(g);
        }
        ev
    }

    /// The car comes down on land at `ground`: the loss of speed, and
    /// the car on the land. Returns the loss (of 256).
    fn land(&mut self, ground: i32, here: i32) -> i32 {
        let down = -self.vy;
        let mut loss = ((down - LAND_FREE).max(0) / LAND_DIV).min(LAND_MAX);
        if self.air_steps >= LAND_SIDE_MIN_AIR {
            let (_, side) = split(self.vel, self.yaw);
            let v = len2(self.vel[0], self.vel[1]).max(1);
            loss += side.abs() * LAND_SIDE / v;
        }
        let loss = loss.min(256);
        self.vel[0] = self.vel[0] * (256 - loss) / 256;
        self.vel[1] = self.vel[1] * (256 - loss) / 256;
        self.pos[1] = ground;
        // DEFAULT: on the ground again the upward speed is the land's
        // change under the car this step.
        self.vy = ground - here;
        self.air = false;
        self.air_steps = 0;
        loss
    }

    /// Keeps the car's middle `EDGE` from the block's edges: pushed
    /// back and its speed out of the block taken away. Only on a real
    /// hit (more than `EDGE_HIT_MIN` outward) does it also lose
    /// `256 - EDGE_KEEP` of 256 of the rest, once; a car that only leans
    /// on the edge (the engine pushing it there) keeps its speed along
    /// the edge (DEFAULT, review F3). True when it was pushed.
    pub fn keep_inside<Gr: Ground>(&mut self, g: &Gr) -> bool {
        let (sx, sz) = g.size();
        let mut pushed = false;
        let mut hit = false;
        for (axis, size) in [(0usize, sx), (2usize, sz)] {
            let v = if axis == 0 { 0 } else { 1 };
            if self.pos[axis] < EDGE {
                self.pos[axis] = EDGE;
                hit |= -self.vel[v] > EDGE_HIT_MIN;
                self.vel[v] = self.vel[v].max(0);
                pushed = true;
            } else if self.pos[axis] > size - EDGE {
                self.pos[axis] = size - EDGE;
                hit |= self.vel[v] > EDGE_HIT_MIN;
                self.vel[v] = self.vel[v].min(0);
                pushed = true;
            }
        }
        if hit {
            self.vel[0] = self.vel[0] * EDGE_KEEP / 256;
            self.vel[1] = self.vel[1] * EDGE_KEEP / 256;
        }
        pushed
    }

    /// After the car was moved sideways (the edge, a thing): on the
    /// ground it stands on the land again; in the air it is never left
    /// under it (it lands there).
    pub fn settle<Gr: Ground>(&mut self, g: &Gr) {
        let s = g.at(self.pos[0], self.pos[2]);
        if !self.air {
            self.pos[1] = s.height;
        } else if self.pos[1] < s.height {
            self.pos[1] = s.height;
            self.vy = 0;
            self.air = false;
            self.air_steps = 0;
        }
    }
}

/// Speed split along a yaw: (forward, to the right), subs a step,
/// rounded to the nearest.
pub fn split(vel: [i32; 2], yaw: u16) -> (i32, i32) {
    let (fx, fz) = dir(yaw);
    let (rx, rz) = right(yaw);
    (
        mul14(vel[0], fx) + mul14(vel[1], fz),
        mul14(vel[0], rx) + mul14(vel[1], rz),
    )
}

/// `v` moved toward 0 by at most `by` (never past 0).
pub fn toward_zero(v: i32, by: i32) -> i32 {
    if v > 0 {
        (v - by).max(0)
    } else {
        (v + by).min(0)
    }
}

/// The steering's ramp: away from straight by `RAMP_IN` a step, back
/// toward straight by `RAMP_OUT`, never past the wanted value.
pub fn ramp(steer: i32, aim: i32) -> i32 {
    let d = aim - steer;
    if d == 0 {
        return steer;
    }
    let away = steer == 0 || (d > 0) == (steer > 0);
    let rate = if away { RAMP_IN } else { RAMP_OUT };
    if d > 0 {
        steer + d.min(rate)
    } else {
        steer - (-d).min(rate)
    }
}

/// The stick's x (-32,767 to 32,767) as a wanted steering (-256 to
/// 256), with the dead zone `DEAD` (the spec's step 1).
pub fn stick_aim(x: i16) -> i32 {
    use crate::tune::DEAD;
    let x = i32::from(x);
    let a = x.abs();
    if a < DEAD {
        return 0;
    }
    let s = ((a - DEAD) * 256 / (32_767 - DEAD)).min(256);
    if x < 0 { -s } else { s }
}
