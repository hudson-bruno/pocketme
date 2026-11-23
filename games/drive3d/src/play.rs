//! One whole step of the game: the car, the things, the progress along
//! the road, Y (back to the road), START (from the start again), X (the
//! detail level), the finish, and the fingerprint.
//!
//! The drawing reads a [`Play`] only through [`crate::look::Look`].

use crate::camera::Chase;
use crate::car::{Car, Controls, Events};
use crate::detail::Detail;
use crate::land::{Ground, Stretch, along_of};
use crate::maths::{FNV_START, dist2, fnv};
use crate::road::{POINTS, ROAD};
use crate::things;
use crate::tune::{
    BLOCKED_SPEED, DUST_SPEED, FORD, GRAVEL, M, PUFF_EVERY, PUFF_LIFE, PUFFS, RESET_STEPS,
    ROAD_CATCH,
};
use game_3d::{cos_fine, sin_fine};

/// One step's input, in the rules' units (`lib` turns buttons, the
/// stick and the triggers into this).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    pub controls: Controls,
    /// Y was pressed: back to the road after `RESET_STEPS`.
    pub reset: bool,
    /// START was pressed: from the start again.
    pub restart: bool,
    /// X was pressed: the next detail level.
    pub level: bool,
}

/// What happened in the last step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StepEvents {
    pub car: Events,
    /// A thing the car hit (its index).
    pub hit: Option<u8>,
    /// The car was put back on the road (Y).
    pub put_back: bool,
    /// The run finished this step (after this many steps).
    pub finished: Option<u32>,
    /// From the start again (START).
    pub restarted: bool,
    /// The level changed (X).
    pub level: bool,
}

/// One dust puff left in the world (see `tune::PUFFS`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Puff {
    /// Where it was made (subs, x and z); the drawing puts it on the
    /// road or the land there.
    pub at: [i32; 2],
    /// Steps since it was made, 1 to `PUFF_LIFE`; 0: no puff.
    pub age: u8,
    /// Made in the ford (spray, not dust).
    pub spray: bool,
}

/// The game's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Play {
    pub car: Car,
    pub cam: Chase,
    /// The last road point passed. It follows the nearest road point,
    /// so it can move back by up to 4 points when the car drives or
    /// rolls back (review rules F9); the finish still needs the car at
    /// the last point.
    pub last_road: u16,
    /// The dust puffs (a ring; `dust_next` is the one made next).
    pub dust: [Puff; PUFFS],
    pub dust_next: u8,
    /// Steps in a row the car has been blocked (`tune::BLOCKED_SPEED`).
    pub blocked: u16,
    pub detail: Detail,
    /// Steps left until the car is put back on the road (0: none).
    pub reset_in: u8,
    /// Steps since the run started (the first step with throttle).
    pub run_steps: u32,
    pub running: bool,
    /// The run's time in steps, once past the finish.
    pub finished: Option<u32>,
    /// Steps since the game (or START) began.
    pub steps: u32,
    /// Counts, for the log.
    pub flights: u16,
    pub resets: u16,
    pub hits: u16,
    pub events: StepEvents,
    /// A fingerprint of every step's state.
    pub print: u64,
}

impl Default for Play {
    fn default() -> Play {
        Play::new()
    }
}

impl Play {
    /// The car on the start line, standing.
    pub fn new() -> Play {
        let g = Stretch;
        let car = Car::standing(&g, ROAD.x[0], ROAD.z[0], ROAD.heading[0]);
        let cam = Chase::behind(&car, &g);
        Play {
            car,
            cam,
            last_road: 0,
            dust: [Puff::default(); PUFFS],
            dust_next: 0,
            blocked: 0,
            detail: Detail::START,
            reset_in: 0,
            run_steps: 0,
            running: false,
            finished: None,
            steps: 0,
            flights: 0,
            resets: 0,
            hits: 0,
            events: StepEvents::default(),
            print: FNV_START,
        }
    }

    /// One step.
    pub fn step(&mut self, input: &Input) {
        let g = Stretch;
        let mut ev = StepEvents::default();
        if input.restart {
            let detail = self.detail;
            *self = Play::new();
            self.detail = detail;
            self.events = StepEvents {
                restarted: true,
                ..StepEvents::default()
            };
            return;
        }
        if input.level {
            self.detail = self.detail.next();
            ev.level = true;
        }
        if input.reset && self.reset_in == 0 {
            self.reset_in = RESET_STEPS;
        }

        let mut c = input.controls;
        if self.finished.is_some() {
            // Past the finish: no throttle, the brakes on until the car
            // stands (and not on into reverse).
            let (fwd, _) = self.car.fwd_side();
            c.throttle = 0;
            c.brake = if fwd > 0 { 256 } else { 0 };
        }
        if !self.running && self.finished.is_none() && c.throttle > 0 {
            self.running = true;
        }

        ev.car = self.car.step(&c, &g);
        let hit = things::collide(&mut self.car.pos, &mut self.car.vel);
        if let Some(i) = hit.thing {
            self.car.settle(&g);
            ev.hit = Some(i);
            self.hits = self.hits.saturating_add(1);
        }
        if ev.car.took_off {
            self.flights = self.flights.saturating_add(1);
        }
        self.progress();
        self.dust_and_blocked(&c, hit.thing.is_some() || ev.car.edge);

        if self.reset_in > 0 {
            self.reset_in -= 1;
            if self.reset_in == 0 {
                self.put_back();
                ev.put_back = true;
            }
        }

        if self.running {
            self.run_steps = self.run_steps.saturating_add(1);
        }
        if self.finished.is_none() && usize::from(self.last_road) == POINTS - 1 {
            self.finished = Some(self.run_steps);
            self.running = false;
            ev.finished = self.finished;
        }

        self.cam.step(&self.car, &g, usize::from(self.last_road));
        self.steps = self.steps.saturating_add(1);
        self.events = ev;
        self.print = self.fingerprint_step(self.print);
    }

    /// Ages the dust and makes a new puff when the car throws one (see
    /// `tune::PUFFS`), and counts the steps the car is blocked:
    /// `touching` is a hit on a thing or the edge this step.
    fn dust_and_blocked(&mut self, c: &Controls, touching: bool) {
        for p in self.dust.iter_mut() {
            if p.age > 0 {
                p.age = if p.age >= PUFF_LIFE { 0 } else { p.age + 1 };
            }
        }
        let car = &self.car;
        let speed = car.speed();
        let pushing = c.throttle > 0 && speed < BLOCKED_SPEED;
        if pushing && touching {
            self.blocked = self.blocked.saturating_add(1);
        } else if !pushing {
            self.blocked = 0;
        }
        let (fwd, _) = car.fwd_side();
        let spin = pushing && touching;
        let dusty = car.surface == GRAVEL || car.surface == FORD;
        let throws = !car.air && ((dusty && fwd > DUST_SPEED) || spin);
        if !throws || !self.steps.is_multiple_of(PUFF_EVERY) {
            return;
        }
        // At a rear corner (2.1 m back, 0.96 m out: just behind the
        // bumper, beside the rear wheel), left and right in turn.
        let n = usize::from(self.dust_next) % PUFFS;
        let side = if n.is_multiple_of(2) { -1 } else { 1 };
        let (s, k) = (sin_fine(car.yaw), cos_fine(car.yaw));
        let back = -(M * 21 / 10);
        let wide = side * (M * 96 / 100);
        let mul = |a: i32, b: i32| (i64::from(a) * i64::from(b) / 16_384) as i32;
        self.dust[n] = Puff {
            at: [
                car.pos[0] + mul(back, s) + mul(wide, k),
                car.pos[2] + mul(back, k) - mul(wide, s),
            ],
            age: 1,
            spray: car.surface == FORD,
        };
        self.dust_next = ((n + 1) % PUFFS) as u8;
    }

    /// Moves `last_road` to the road point the car has passed: the
    /// nearest point from 4 behind to 16 ahead of the last, if it is
    /// within `ROAD_CATCH`; the car has passed it when it is level with
    /// it or beyond along the road's way, else the one before counts.
    pub fn progress(&mut self) {
        let last = usize::from(self.last_road);
        let lo = last.saturating_sub(4);
        let hi = (last + 16).min(POINTS - 1);
        let at = (self.car.pos[0], self.car.pos[2]);
        let mut best = last;
        let mut best_d = i64::MAX;
        for k in lo..=hi {
            let d = dist2(at, (ROAD.x[k], ROAD.z[k]));
            if d < best_d {
                best_d = d;
                best = k;
            }
        }
        let catch = i64::from(ROAD_CATCH);
        if best_d > catch * catch {
            return;
        }
        let along = along_of(&ROAD, best, at.0, at.1);
        let passed = if along >= 0 {
            best
        } else {
            best.saturating_sub(1)
        };
        self.last_road = passed as u16;
    }

    /// Y: the car stands on the last road point passed, along the road,
    /// and the camera is right behind it.
    pub fn put_back(&mut self) {
        let g = Stretch;
        let k = usize::from(self.last_road);
        self.car = Car::standing(&g, ROAD.x[k], ROAD.z[k], ROAD.heading[k]);
        self.cam = Chase::behind(&self.car, &g);
        self.blocked = 0;
        self.resets = self.resets.saturating_add(1);
    }

    /// The fingerprint after one more step.
    fn fingerprint_step(&self, h: u64) -> u64 {
        let c = &self.car;
        let mut h = h;
        for v in [
            c.pos[0],
            c.pos[1],
            c.pos[2],
            c.vel[0],
            c.vel[1],
            c.vy,
            i32::from(c.yaw),
            c.steer,
            i32::from(c.air),
            i32::from(self.last_road),
            self.cam.pos[0],
            self.cam.pos[1],
            self.cam.pos[2],
            i32::from(self.cam.yaw),
            i32::from(self.cam.pitch),
            // Everything else that changes a later step (review rules
            // F8): a waiting Y, the run's clock, the time in the air,
            // the blocked count and the dust.
            i32::from(c.air_steps),
            i32::from(self.reset_in),
            self.run_steps as i32,
            i32::from(self.running),
            self.finished.map_or(-1, |f| f as i32),
            i32::from(self.blocked),
            i32::from(self.dust_next),
        ] {
            h = fnv(h, i64::from(v));
        }
        for p in &self.dust {
            for v in [p.at[0], p.at[1], i32::from(p.age), i32::from(p.spray)] {
                h = fnv(h, i64::from(v));
            }
        }
        h
    }

    /// The land under the car (for the picture's text).
    pub fn ground_here(&self) -> crate::land::Spot {
        Stretch.at(self.car.pos[0], self.car.pos[2])
    }
}
