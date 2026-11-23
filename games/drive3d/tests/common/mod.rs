//! Test grounds and helpers shared by the tests.
#![allow(dead_code)]

use drive3d::car::{Car, Controls};
use drive3d::land::{Ground, Spot};
use drive3d::tune::M;

/// A flat land of one surface, 131 km a side (no edge in reach).
pub struct Flat {
    pub surface: u8,
    pub height: i32,
}

impl Ground for Flat {
    fn at(&self, _x: i32, _z: i32) -> Spot {
        Spot {
            height: self.height,
            surface: self.surface,
        }
    }
    fn size(&self) -> (i32, i32) {
        (1 << 29, 1 << 29)
    }
}

/// A land that climbs `num / den` along +z (gravel).
pub struct Slope {
    pub num: i32,
    pub den: i32,
}

impl Ground for Slope {
    fn at(&self, _x: i32, z: i32) -> Spot {
        Spot {
            height: (i64::from(z) * i64::from(self.num) / i64::from(self.den)) as i32,
            surface: 0,
        }
    }
    fn size(&self) -> (i32, i32) {
        (1 << 29, 1 << 29)
    }
}

/// The middle of a big test land (subs).
pub const MID: i32 = 1 << 28;

/// A car standing in the middle of a test land, nose along +z.
pub fn car_on<G: Ground>(g: &G) -> Car {
    Car::standing(g, MID, MID, 0)
}

/// Full throttle, wheel straight.
pub const GAS: Controls = Controls {
    aim: 0,
    throttle: 256,
    brake: 0,
    handbrake: false,
};

/// Nothing pressed.
pub const COAST: Controls = Controls {
    aim: 0,
    throttle: 0,
    brake: 0,
    handbrake: false,
};

/// m/s from subs a step.
pub fn mps(v: i32) -> f64 {
    f64::from(v) * 30.0 / f64::from(M)
}

/// A car rolling at `v` subs a step along its nose (+z).
pub fn rolling<G: Ground>(g: &G, v: i32) -> Car {
    let mut c = car_on(g);
    c.vel = [0, v];
    c
}

use drive3d::maths::{angle_diff, yaw_of};
use drive3d::play::{Input, Play};
use drive3d::road::{POINTS, ROAD};

/// A test driver: steers toward the road point 5 ahead (10 m) of the
/// last one passed, in proportion to the angle (full lock at 16
/// degrees off), and holds `cruise` subs a step on straights and
/// `corner` in the corners (pieces 5 and 7), braking when 1 m/s over.
pub fn autopilot(p: &Play, cruise: i32, corner: i32) -> Input {
    let k = (usize::from(p.last_road) + 5).min(POINTS - 1);
    let c = &p.car;
    let (tx, tz) = if usize::from(p.last_road) + 5 >= POINTS {
        // Past the end: straight on along the last heading.
        let (s, k2) = (
            drive3d::maths::dir(ROAD.heading[POINTS - 1]).0,
            drive3d::maths::dir(ROAD.heading[POINTS - 1]).1,
        );
        (
            c.pos[0] + (i64::from(s) * 10 * i64::from(M) / 16_384) as i32,
            c.pos[2] + (i64::from(k2) * 10 * i64::from(M) / 16_384) as i32,
        )
    } else {
        (ROAD.x[k], ROAD.z[k])
    };
    let want = yaw_of(tx - c.pos[0], tz - c.pos[2]).unwrap_or(c.yaw);
    let off = angle_diff(c.yaw, want);
    let aim = (off * 256 / 2_912).clamp(-256, 256);
    let piece = ROAD.piece[(usize::from(p.last_road) + 8).min(POINTS - 1)];
    let target = if piece == 5 || piece == 7 {
        corner
    } else {
        cruise
    };
    let v = c.speed();
    let (throttle, brake) = if v < target {
        (256, 0)
    } else if v > target + M / 30 {
        (0, 256)
    } else {
        (0, 0)
    };
    Input {
        controls: Controls {
            aim,
            throttle,
            brake,
            handbrake: false,
        },
        ..Input::default()
    }
}

use drive3d::camera::VIEW;
use drive3d::tune::{CAR_HEIGHT, CAR_LEN, CAR_WIDTH};

/// Where the car is on the picture: its middle (pixels) and the height
/// in pixels of the box round its 8 corners (`CAR_LEN` by `CAR_WIDTH`
/// by `CAR_HEIGHT`, turned by its yaw), and the nearest corner's depth
/// in front of the camera (game units).
pub struct OnScreen {
    pub mid: (i32, i32),
    pub height_px: i32,
    pub nearest: i32,
}

pub fn car_on_screen(p: &Play) -> OnScreen {
    let cam = p.cam.camera();
    let c = &p.car;
    let (fx, fz) = drive3d::maths::dir(c.yaw);
    let (rx, rz) = drive3d::maths::right(c.yaw);
    let at = |ahead: i32, right: i32, up: i32| {
        let m = |a: i32, b: i32| (i64::from(a) * i64::from(b) / 16_384) as i32;
        let w = [
            c.pos[0] + m(ahead, fx) + m(right, rx),
            c.pos[1] + up,
            c.pos[2] + m(ahead, fz) + m(right, rz),
        ];
        let q = cam.to_cam([w[0] / 16, w[1] / 16, w[2] / 16]);
        (VIEW.project(q), q[2])
    };
    let mut lo = i32::MAX;
    let mut hi = i32::MIN;
    let mut nearest = i32::MAX;
    for a in [-CAR_LEN / 2, CAR_LEN / 2] {
        for r in [-CAR_WIDTH / 2, CAR_WIDTH / 2] {
            for u in [0, CAR_HEIGHT] {
                let (px, z) = at(a, r, u);
                lo = lo.min(px[1]);
                hi = hi.max(px[1]);
                nearest = nearest.min(z);
            }
        }
    }
    let (m, _) = at(0, 0, CAR_HEIGHT / 2);
    OnScreen {
        mid: (m[0] / 16, m[1] / 16),
        height_px: (hi - lo) / 16,
        nearest,
    }
}

/// The largest amount (subs) by which the land rises above the line
/// from the camera to the car's roof, looked at in `n` points (0 when
/// the roof is in sight).
pub fn roof_hidden_by<G: Ground>(p: &Play, g: &G, n: i32) -> i32 {
    let cam = p.cam.pos;
    let c = &p.car;
    let roof = c.pos[1] + CAR_HEIGHT;
    let mut worst = 0;
    for k in 1..n {
        let f = |a: i32, b: i32| a + ((i64::from(b - a) * i64::from(k)) / i64::from(n)) as i32;
        let (x, y, z) = (f(cam[0], c.pos[0]), f(cam[1], roof), f(cam[2], c.pos[2]));
        worst = worst.max(g.at(x, z).height - y);
    }
    worst
}
