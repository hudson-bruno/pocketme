//! The one small read-only interface between the rules and the drawing.
//!
//! The drawing gets a [`Look`] for each picture and reads the game only
//! through it: it can change nothing. Units are given on each method:
//! *subs* are 1/4096 m, game-3d's world unit is 1/256 m (`subs >> 4`,
//! or [`Look::unit`]), angles are 1/65,536 of a turn (game-3d's yaw: 0
//! along +z, a quarter turn along +x).

use crate::camera::{Chase, VIEW};
use crate::detail::{Detail, Reach};
use crate::land::{Ground, LAND, Land, Spot, Stretch};
use crate::maths::{len2, yaw_of};
use crate::play::{Play, Puff, StepEvents};
use crate::road::{ROAD, Road};
use crate::things::{THINGS, Thing};
use crate::tune::{BLOCKED_HINT, M, SUB_PER_UNIT, SURFACE_NAMES};
use game_3d::{Camera, Mat3, View, cos_fine, sin_fine};

/// What the drawing may read.
#[derive(Clone, Copy)]
pub struct Look<'a> {
    play: &'a Play,
    /// Steps run in this update (the line of numbers' `S`).
    steps_last: u8,
}

impl<'a> Look<'a> {
    pub fn new(play: &'a Play, steps_last: u8) -> Look<'a> {
        Look { play, steps_last }
    }

    // ---- The car --------------------------------------------------

    /// The middle of the car's base on the land (subs).
    pub fn car_pos(&self) -> [i32; 3] {
        self.play.car.pos
    }
    /// Where its nose points (angle).
    pub fn car_yaw(&self) -> u16 {
        self.play.car.yaw
    }
    /// Its horizontal speed (subs a step: 4,096 is 30 m/s), x and z.
    pub fn car_vel(&self) -> [i32; 2] {
        self.play.car.vel
    }
    /// Its speed in km/h, rounded down (subs a step times 30 * 3.6 /
    /// 4096 = 108 / 4096).
    pub fn car_kmh(&self) -> i32 {
        self.play.car.speed() * 108 / 4096
    }
    /// Forward and sideways speed (subs a step; sideways plus to the
    /// right): a slide shows as a large sideways part.
    pub fn car_fwd_side(&self) -> (i32, i32) {
        self.play.car.fwd_side()
    }
    /// The steering, -256 (full left) to 256 (full right), for the
    /// front wheels.
    pub fn car_steer(&self) -> i32 {
        self.play.car.steer
    }
    /// In the air.
    pub fn car_air(&self) -> bool {
        self.play.car.air
    }
    /// The surface under the car (`tune::GRAVEL`, `GRASS`, `FORD`,
    /// `LOW_GRASS`) and
    /// its name.
    pub fn surface(&self) -> u8 {
        self.play.car.surface
    }
    pub fn surface_name(&self) -> &'static str {
        SURFACE_NAMES[usize::from(self.play.car.surface).min(SURFACE_NAMES.len() - 1)]
    }
    /// The car's turning for its model, drawing only: yaw, then the
    /// tilt of the land 2 m ahead and behind (pitch) and 1 m to each
    /// side (roll). Level in the air. In 1/16384 (game-3d's `Mat3`).
    pub fn car_rotation(&self) -> Mat3 {
        let (pitch, roll) = self.car_tilt();
        Mat3::rotation_fine(self.play.car.yaw, pitch, roll)
    }
    /// The tilt as angles (pitch: nose up is plus; roll: left side up
    /// is plus, game-3d's).
    pub fn car_tilt(&self) -> (u16, u16) {
        let c = &self.play.car;
        if c.air {
            return (0, 0);
        }
        let (s, k) = (sin_fine(c.yaw), cos_fine(c.yaw));
        let at = |ahead: i32, right: i32| {
            let x = c.pos[0] + mul(ahead, s) + mul(right, k);
            let z = c.pos[2] + mul(ahead, k) - mul(right, s);
            Stretch.at(x, z).height
        };
        let rise = at(2 * M, 0) - at(-2 * M, 0);
        let lean = at(0, -M) - at(0, M);
        let pitch = signed_angle(rise, 4 * M);
        let roll = signed_angle(lean, 2 * M);
        (pitch, roll)
    }

    // ---- The camera -----------------------------------------------

    /// The chase camera's eye (subs), yaw and pitch (down is plus here;
    /// [`Look::camera`] turns it into game-3d's sign).
    pub fn chase(&self) -> &Chase {
        &self.play.cam
    }
    /// game-3d's camera (the eye in game units).
    pub fn camera(&self) -> Camera {
        self.play.cam.camera()
    }
    /// The view: the whole 160 by 120 picture, lens 100.
    pub fn view(&self) -> View {
        VIEW
    }
    /// True when a thing at (x, z) of radius `r` (subs) stands between
    /// the camera and the car (the drawing may leave it out).
    pub fn hides_car(&self, x: i32, z: i32, r: i32) -> bool {
        self.play.cam.hides_car(&self.play.car, x, z, r)
    }

    // ---- The stretch ----------------------------------------------

    /// The baked land: corner heights (1/4 m) and cell bytes (see
    /// `land`). Constants, the same every run.
    pub fn land(&self) -> &'static Land {
        &LAND
    }
    /// The land's height (subs) and surface under (x, z) (subs), from
    /// the triangle that is drawn.
    pub fn ground(&self, x: i32, z: i32) -> Spot {
        Stretch.at(x, z)
    }
    /// The road's points (one every 2 m; subs; heading as an angle).
    pub fn road(&self) -> &'static Road {
        &ROAD
    }
    /// The last road point the car passed.
    pub fn last_road(&self) -> usize {
        usize::from(self.play.last_road)
    }
    /// The things beside the road (5-byte records; `Thing::at` gives
    /// the place in subs, `Thing::height` the height in subs).
    pub fn things(&self) -> &'static [Thing] {
        &THINGS
    }

    // ---- The run --------------------------------------------------

    /// The detail level and what it sets.
    pub fn detail(&self) -> Detail {
        self.play.detail
    }
    pub fn reach(&self) -> Reach {
        self.play.detail.reach()
    }
    /// Steps left until Y puts the car back (None: no Y pending).
    pub fn resetting(&self) -> Option<u8> {
        (self.play.reset_in > 0).then_some(self.play.reset_in)
    }
    /// The run's time once finished (steps; 30 a second).
    pub fn finished(&self) -> Option<u32> {
        self.play.finished
    }
    /// Steps since the run started (the first throttle).
    pub fn run_steps(&self) -> u32 {
        self.play.run_steps
    }
    /// Steps since the start (or START), the picture's clock (for
    /// spray, blinking and the like).
    pub fn steps(&self) -> u32 {
        self.play.steps
    }
    /// Steps run in this update (1 or 2; 0 when none ran).
    pub fn steps_last(&self) -> u8 {
        self.steps_last
    }
    /// The dust puffs left in the world (age 0: none; see
    /// `tune::PUFFS`).
    pub fn puffs(&self) -> &[Puff] {
        &self.play.dust
    }
    /// The car has been blocked (throttle held against a thing or the
    /// edge, under 1 m/s) for a second or more.
    pub fn blocked(&self) -> bool {
        self.play.blocked >= BLOCKED_HINT
    }
    /// What happened in the last step (took off, landed, hit, ...).
    pub fn events(&self) -> StepEvents {
        self.play.events
    }

    /// A position in subs as game-3d units.
    pub fn unit(p: [i32; 3]) -> [i32; 3] {
        [
            p[0] / SUB_PER_UNIT,
            p[1] / SUB_PER_UNIT,
            p[2] / SUB_PER_UNIT,
        ]
    }
}

/// `a * b / 16384` in 64 bits.
fn mul(a: i32, b: i32) -> i32 {
    (i64::from(a) * i64::from(b) / 16_384) as i32
}

/// The angle (plus or minus) whose tangent is `up / along` (`along`
/// greater than 0).
fn signed_angle(up: i32, along: i32) -> u16 {
    let a = yaw_of(up.abs(), along).unwrap_or(0);
    if up < 0 { a.wrapping_neg() } else { a }
}

/// The horizontal distance (subs) between the camera and the car, for
/// the drawing's tests.
pub fn chase_distance(play: &Play) -> i32 {
    len2(
        play.cam.pos[0] - play.car.pos[0],
        play.cam.pos[2] - play.car.pos[2],
    )
}
