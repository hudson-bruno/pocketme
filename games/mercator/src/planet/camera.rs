//! Step G2: the landing half of the planet's camera (SPEC 3.5, "The
//! landing camera"): a chase camera 24 m behind and 10 m above the
//! ship, looking 30 degrees down, following by 1/8 a tick and never
//! under the ground plus 2 m. L and R turn it by 45 degrees (a tap);
//! both put it behind the ship's nose. The walk's half comes with G3.

use super::Below;
use super::lander::{Phase, TO_WORLD};
use super::site;
use super::tune::{
    CAM_BACK, CAM_CLEAR, CAM_EASE, CAM_PITCH, CAM_UP, ENTRY_SKY, ENTRY_TICKS, TAKEOFF_CAM,
    TAKEOFF_TICKS,
};
use crate::fixed::{Mat3, V3, cos, hash, sin};
use crate::fx::SHAKE_ON;
use crate::poly::Camera;

/// The ship's middle as drawn (world units): its feet plus its ride.
pub fn ship_at(b: &Below) -> V3 {
    let p = b.p.map(|v| v >> TO_WORLD);
    [p[0], p[1] + b.ride, p[2]]
}

/// Where the camera wants to be for a ship at `at` (world units),
/// looking along `yaw`: behind and above, more of both while taking
/// off (`pull` 0..=256).
pub fn goal(at: V3, yaw: u16, pull: i32) -> V3 {
    let back = CAM_BACK + 2 * TAKEOFF_CAM * pull / 256;
    let up = CAM_UP + TAKEOFF_CAM * pull / 256;
    [
        at[0] - ((sin(yaw) * (back >> 6)) >> 8),
        at[1] + up,
        at[2] - ((cos(yaw) * (back >> 6)) >> 8),
    ]
}

/// How far the take-off has pulled the camera (0..=256).
fn pull(b: &Below) -> i32 {
    if b.phase == Phase::TakeOff {
        i32::from(b.phase_t.min(TAKEOFF_TICKS)) * 256 / i32::from(TAKEOFF_TICKS)
    } else {
        0
    }
}

/// Keeps a camera place over the ground.
fn clear(b: &Below, c: V3) -> V3 {
    let floor = site::surface(&b.site, c[0], c[2]) + CAM_CLEAR;
    [c[0], c[1].max(floor), c[2]]
}

/// One tick of the camera: its turn and its place ease toward their
/// goals (`snap`: straight there, at the entry's start).
pub fn follow(b: &mut Below, snap: bool) {
    let d = i32::from(b.cam_goal.wrapping_sub(b.cam_yaw) as i16);
    let step = if snap || d.abs() < CAM_EASE {
        d
    } else {
        d / CAM_EASE
    };
    b.cam_yaw = b.cam_yaw.wrapping_add(step as u16);
    let g = goal(ship_at(b), b.cam_yaw, pull(b));
    if snap {
        b.cam = g;
    } else {
        for (c, g) in b.cam.iter_mut().zip(g) {
            *c += (g - *c) / CAM_EASE;
        }
    }
    b.cam = clear(b, b.cam);
}

/// A camera at `pos` looking along `yaw`, `down` (65536 a turn) under
/// the horizon, turned a little by `shake` (yaw, pitch).
pub fn looking(pos: V3, yaw: u16, down: u16, shake: (i32, i32)) -> Camera {
    let rot = Mat3::rotation(
        yaw.wrapping_add(shake.0 as u16),
        down.wrapping_neg().wrapping_add(shake.1 as u16),
        0,
    );
    Camera {
        pos,
        rot: rot.transposed(),
    }
}

/// The shake: during the entry's first part and after a hard knock,
/// about a pixel either way (`fx::SHAKE_ON` is the switch).
fn shake(b: &Below) -> (i32, i32) {
    let on = (b.phase == Phase::Entry && b.phase_t < ENTRY_SKY) || b.jolt > 0;
    if !SHAKE_ON || !on {
        return (0, 0);
    }
    // About 1 px at focal 96: 1/96 of a radian is 109 turn units.
    let h = hash(b.t, 0x5A4E);
    (
        (h % 3) as i32 * 109 - 109,
        ((h >> 8) % 3) as i32 * 109 - 109,
    )
}

/// How far down the camera looks: 15 degrees at the entry's start (the
/// sky coming in from the horizon up is seen, the ship still in view),
/// easing to 30 degrees down by its end; 30 degrees after.
pub fn pitch(b: &Below) -> u16 {
    if b.phase == Phase::Entry {
        let t = u32::from(b.phase_t.min(ENTRY_TICKS));
        let half = u32::from(CAM_PITCH) / 2;
        (half + half * t / u32::from(ENTRY_TICKS)) as u16
    } else {
        CAM_PITCH
    }
}

/// The landing camera as it is now.
pub fn camera(b: &Below) -> Camera {
    looking(b.cam, b.cam_yaw, pitch(b), shake(b))
}

/// The chase camera's goal for the ship at `at` looking along `yaw`,
/// kept over the ground (the tests' and pictures' still camera).
pub fn chase_at(b: &Below, at: V3, yaw: u16) -> Camera {
    looking(clear(b, goal(at, yaw, 0)), yaw, CAM_PITCH, (0, 0))
}

// ---- Step G3: the camera on foot (SPEC 3.6, 3.8) ---------------------------------
//
// Third person, over his right shoulder: `FOOT_BACK` behind the pilot
// along the camera's turn, `FOOT_SIDE` to its right and `FOOT_UP` (plus
// the right stick's tilt) over his feet, looking at a point `FOOT_AHEAD`
// in front of him at chest height. So he stands left of the middle and
// low, and what he walks to or cuts is in the middle of the view, not
// behind his back. L and R turn it by 45 degrees (a tap), both put it
// behind him, the right stick turns and tilts it. It is kept over the
// ground and over the ground between it and the point it looks at, so
// the land never hides him.

use super::tune::{FOOT_AHEAD, FOOT_AIM, FOOT_BACK, FOOT_CLEAR, FOOT_EASE, FOOT_SIDE, FOOT_UP};

/// Where the camera looks on foot along `yaw`: `FOOT_AHEAD` in front of
/// the pilot and `FOOT_SIDE` to the right, at chest height over him or
/// over the ground there, whichever is higher (world units).
fn foot_aim_at(b: &Below, yaw: u16) -> V3 {
    let p = b.pilot.at();
    let (s, c) = (sin(yaw), cos(yaw));
    let x = p[0] + ((s * (FOOT_AHEAD >> 6) + c * (FOOT_SIDE >> 6)) >> 8);
    let z = p[2] + ((c * (FOOT_AHEAD >> 6) - s * (FOOT_SIDE >> 6)) >> 8);
    let y = p[1].max(site::surface(&b.site, x, z)) + FOOT_AIM;
    [x, y, z]
}

/// Where the camera looks on foot now.
pub fn foot_aim(b: &Below) -> V3 {
    foot_aim_at(b, b.cam_yaw)
}

/// The camera's goal on foot along `yaw`, before the ground's checks.
fn foot_goal(b: &Below, yaw: u16) -> V3 {
    let p = b.pilot.at();
    let (s, c) = (sin(yaw), cos(yaw));
    [
        p[0] + ((c * (FOOT_SIDE >> 6) - s * (FOOT_BACK >> 6)) >> 8),
        p[1] + FOOT_UP + b.pilot.tilt,
        p[2] - ((s * (FOOT_SIDE >> 6) + c * (FOOT_BACK >> 6)) >> 8),
    ]
}

/// Raises `eye` until the ground under it and at three points on the
/// way to `aim` is `FOOT_CLEAR` under the line of sight.
pub fn foot_clear(b: &Below, eye: V3, aim: V3) -> V3 {
    let mut y = eye[1].max(site::surface(&b.site, eye[0], eye[2]) + FOOT_CLEAR);
    for k in 1..4 {
        let x = eye[0] + (aim[0] - eye[0]) * k / 4;
        let z = eye[2] + (aim[2] - eye[2]) * k / 4;
        let h = site::surface(&b.site, x, z) + FOOT_CLEAR;
        // The line's height at k/4 is y + (aim - y) k / 4; it must be h
        // or more.
        let need = (4 * h - k * aim[1]) / (4 - k);
        y = y.max(need);
    }
    [eye[0], y, eye[2]]
}

/// One tick of the camera on foot (`snap`: straight there).
pub fn foot_follow(b: &mut Below, snap: bool) {
    let d = i32::from(b.cam_goal.wrapping_sub(b.cam_yaw) as i16);
    let step = if snap || d.abs() < CAM_EASE {
        d
    } else {
        d / CAM_EASE
    };
    b.cam_yaw = b.cam_yaw.wrapping_add(step as u16);
    let aim = foot_aim(b);
    let g = foot_clear(b, foot_goal(b, b.cam_yaw), aim);
    if snap {
        b.cam = g;
    } else {
        for (c, g) in b.cam.iter_mut().zip(g) {
            *c += (g - *c) / FOOT_EASE;
        }
    }
    b.cam = foot_clear(b, b.cam, aim);
}

/// The camera on foot as it is now.
pub fn foot_camera(b: &Below) -> Camera {
    let mut c = super::look_at(b.cam, foot_aim(b));
    let s = shake(b);
    if s != (0, 0) {
        c.rot = Mat3::rotation(s.0 as u16, s.1 as u16, 0)
            .transposed()
            .mul(&c.rot);
    }
    c
}

/// The camera on foot for the pilot as he stands, along `yaw` (the
/// tests' and pictures' still camera).
pub fn foot_at(b: &Below, yaw: u16) -> Camera {
    let aim = foot_aim_at(b, yaw);
    super::look_at(foot_clear(b, foot_goal(b, yaw), aim), aim)
}
