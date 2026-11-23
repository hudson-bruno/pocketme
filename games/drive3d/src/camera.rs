//! The chase camera (the spec's section 2), one step after the car.
//!
//! Order each step (my reading of the spec, where it leaves the order
//! open; see the report):
//!
//! 1. the aim: the car's yaw, moved `VEL_BLEND` of 256 of the way to the
//!    way the car travels (only going forwards above `BLEND_MIN_V`),
//!    then turned toward the road's way `ROAD_LOOK` points ahead by
//!    `ROAD_TURN` of 256 of the gap, at most `ROAD_CAP`;
//! 2. the camera's yaw moves `EASE_YAW` of 256 of the way to the aim;
//! 3. the wanted place: `BACK` behind the car along the camera's yaw,
//!    `UP` above it, plus up to `DIP_RISE` when the land at the car is
//!    lower than at the wanted place (a dip);
//! 4. the place moves `EASE_POS` of 256 of the way to the wanted place;
//! 5. two floors, applied straight away (not eased, so they always
//!    hold): at least `CLEAR` above the land under the camera, then the
//!    crest check: `HIDE_POINTS` points on the line from the camera to
//!    the car's roof; where the land is above a point, the camera rises
//!    by what lifts that point clear (the gap times the line's share
//!    left), the largest, at most `HIDE_RISE`;
//! 6. the pitch looks down at the look point (`AIM_AHEAD` ahead of the
//!    car along the camera's yaw, `AIM_UP` above it), 0 to `PITCH_MAX`.

use crate::car::Car;
use crate::land::Ground;
use crate::maths::{angle_diff, dir, len2, turn, yaw_of};
use crate::road::{POINTS, ROAD};
use crate::tune::{
    AIM_AHEAD, AIM_UP, BACK, BLEND_MIN_V, CAR_HEIGHT, CLEAR, DIP_RISE, EASE_POS, EASE_YAW, FOCAL,
    HIDE_POINTS, HIDE_RISE, PITCH_MAX, ROAD_CAP, ROAD_LOOK, ROAD_TURN, SUB_PER_UNIT, UP, VEL_BLEND,
};
use game_3d::{Camera, Mat3, Rect, View};

/// The picture's view (all 160 by 120 pixels, the spec's lens).
pub const VIEW: View = View::new(Rect::new(0, 0, 160, 120), FOCAL);

/// The chase camera's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chase {
    /// The eye (subs).
    pub pos: [i32; 3],
    /// Where it looks (angle) and how far down (angle, 0 is level).
    pub yaw: u16,
    pub pitch: u16,
    /// The last step's rises (subs), for the tests and the line.
    pub dip: i32,
    pub hide: i32,
}

impl Chase {
    /// The camera right behind a car, settled (after the start and Y).
    pub fn behind<Gr: Ground>(car: &Car, g: &Gr) -> Chase {
        let mut c = Chase {
            pos: [0; 3],
            yaw: car.yaw,
            pitch: 0,
            dip: 0,
            hide: 0,
        };
        c.pos = c.wanted(car, g);
        c.floors(car, g);
        c.look(car);
        c
    }

    /// The aim (step 1).
    pub fn aim(car: &Car, last_road: usize) -> u16 {
        let mut aim = car.yaw;
        let (fwd, _) = car.fwd_side();
        if fwd > 0
            && car.speed() > BLEND_MIN_V
            && let Some(travel) = yaw_of(car.vel[0], car.vel[1])
        {
            aim = turn(aim, angle_diff(aim, travel) * VEL_BLEND / 256);
        }
        let ahead = (last_road + usize::from(ROAD_LOOK)).min(POINTS - 1);
        let road = ROAD.heading[ahead];
        let by = (angle_diff(aim, road) * ROAD_TURN / 256).clamp(-ROAD_CAP, ROAD_CAP);
        turn(aim, by)
    }

    /// One step after the car.
    pub fn step<Gr: Ground>(&mut self, car: &Car, g: &Gr, last_road: usize) {
        let aim = Chase::aim(car, last_road);
        self.yaw = turn(self.yaw, angle_diff(self.yaw, aim) * EASE_YAW / 256);
        let want = self.wanted(car, g);
        for (p, w) in self.pos.iter_mut().zip(want) {
            *p += ((i64::from(w) - i64::from(*p)) * i64::from(EASE_POS) / 256) as i32;
        }
        self.floors(car, g);
        self.look(car);
    }

    /// The wanted place (step 3).
    fn wanted<Gr: Ground>(&mut self, car: &Car, g: &Gr) -> [i32; 3] {
        let (fx, fz) = dir(self.yaw);
        let x = car.pos[0] - mul(BACK, fx);
        let z = car.pos[2] - mul(BACK, fz);
        let mut y = car.pos[1] + UP;
        let land_cam = g.at(x, z).height;
        let land_car = g.at(car.pos[0], car.pos[2]).height;
        self.dip = (land_cam - land_car).clamp(0, DIP_RISE);
        y += self.dip;
        [x, y, z]
    }

    /// The floors (step 5): clear of the land, and the car's roof not
    /// hidden by it.
    fn floors<Gr: Ground>(&mut self, car: &Car, g: &Gr) {
        let floor = g.at(self.pos[0], self.pos[2]).height + CLEAR;
        if self.pos[1] < floor {
            self.pos[1] = floor;
        }
        self.hide = hide_rise(self.pos, car, g);
        self.pos[1] += self.hide;
    }

    /// The pitch (step 6).
    fn look(&mut self, car: &Car) {
        let (fx, fz) = dir(self.yaw);
        let lx = car.pos[0] + mul(AIM_AHEAD, fx);
        let lz = car.pos[2] + mul(AIM_AHEAD, fz);
        let ly = car.pos[1] + AIM_UP;
        let flat = len2(lx - self.pos[0], lz - self.pos[2]);
        let down = self.pos[1] - ly;
        self.pitch = if down <= 0 {
            0
        } else {
            yaw_of(down, flat.max(1)).unwrap_or(0).min(PITCH_MAX)
        };
    }

    /// game-3d's camera: the eye in game units and the turning taken
    /// back. Looking down is a negative pitch in `Mat3::rotation`.
    pub fn camera(&self) -> Camera {
        let r = Mat3::rotation_fine(self.yaw, self.pitch.wrapping_neg(), 0);
        Camera {
            pos: [
                self.pos[0] / SUB_PER_UNIT,
                self.pos[1] / SUB_PER_UNIT,
                self.pos[2] / SUB_PER_UNIT,
            ],
            rot: r.transposed(),
        }
    }

    /// True when the thing standing at (x, z) with radius `r` (subs) is
    /// between the camera and the car, on the ground line between them,
    /// so the drawing may leave it out (a tree in front of the car).
    pub fn hides_car(&self, car: &Car, x: i32, z: i32, r: i32) -> bool {
        let (ax, az) = (i64::from(self.pos[0]), i64::from(self.pos[2]));
        let (bx, bz) = (i64::from(car.pos[0]), i64::from(car.pos[2]));
        let (dx, dz) = (bx - ax, bz - az);
        let len2 = dx * dx + dz * dz;
        if len2 == 0 {
            return false;
        }
        let (px, pz) = (i64::from(x) - ax, i64::from(z) - az);
        let along = px * dx + pz * dz;
        if along <= 0 || along >= len2 {
            return false;
        }
        // Squared distance from the line, times len2.
        let cross = px * dz - pz * dx;
        let r = i64::from(r);
        cross / 64 * (cross / 64) < r * r / 4096 * len2
    }
}

/// `a * b / 16384` (b a sine or cosine), in 64 bits.
fn mul(a: i32, b: i32) -> i32 {
    (i64::from(a) * i64::from(b) / 16_384) as i32
}

/// The crest check: how far the camera at `cam` must rise so that the
/// land is under every one of `HIDE_POINTS` points on the line to the
/// car's roof (at most `HIDE_RISE`). Point k of n sits at k / (n + 1) of
/// the way; raising the camera by R raises it by R (n + 1 - k) / (n + 1).
pub fn hide_rise<Gr: Ground>(cam: [i32; 3], car: &Car, g: &Gr) -> i32 {
    let roof = car.pos[1] + CAR_HEIGHT;
    let n = HIDE_POINTS + 1;
    let mut rise = 0i32;
    for k in 1..n {
        let px = cam[0] + ((i64::from(car.pos[0] - cam[0]) * i64::from(k)) / i64::from(n)) as i32;
        let pz = cam[2] + ((i64::from(car.pos[2] - cam[2]) * i64::from(k)) / i64::from(n)) as i32;
        let py = cam[1] + ((i64::from(roof - cam[1]) * i64::from(k)) / i64::from(n)) as i32;
        let land = g.at(px, pz).height;
        if land > py {
            let need = (land - py) * n / (n - k);
            rise = rise.max(need);
        }
    }
    rise.min(HIDE_RISE)
}
