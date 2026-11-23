//! Step G3: the site clock and the storm (SPEC 3.6, "The site clock"):
//! `SITE_TIME` by danger plus a greased landing's bonus, from the
//! touchdown; the warning 30 s before the end; then the storm: the fog
//! closes to 24 m over 20 s, the sky and the air slide toward the
//! weather's colour, streaks cross the view in the wind, and the suit
//! drains away from the hatch.

use super::Below;
use super::lander::Phase;
use super::tune::{
    HAZE_LOOK_REACH, MID_R, SITE_TIME, STORM_CLOSE, STORM_DRAIN, STORM_NEAR, STORM_STEPS,
    STORM_WARN,
};
use super::walker;
use crate::fixed::{cos, hash, sin};
use crate::poly::Rect;
use game_kit::Frame;

/// The warning per look (OCEAN, DESERT, ICE, LAVA, GIANT, JUNGLE, HAZE,
/// RUST).
const WARN: [&str; 8] = [
    "RAIN SQUALL IN 30 S",
    "DUST STORM IN 30 S",
    "WHITEOUT IN 30 S",
    "ASH FALL IN 30 S",
    "RAIN SQUALL IN 30 S",
    "RAIN SQUALL IN 30 S",
    "RAIN SQUALL IN 30 S",
    "DUST STORM IN 30 S",
];

/// Step G4: the notes when the storm starts, out and in the ship.
pub const STORM_FOOT: &str = "STORM: BACK TO THE SHIP";
pub const STORM_SHIP: &str = "STORM: LIFT OFF";

/// The site's time in ticks.
pub fn total(b: &Below) -> i32 {
    (i32::from(SITE_TIME[usize::from(b.danger.min(3))]) + i32::from(b.bonus)) * 60
}

/// Ticks left on the clock (negative in the storm).
pub fn left(b: &Below) -> i32 {
    total(b) - i32::from(b.since)
}

/// Ticks of storm so far (0 before it).
pub fn storm(b: &Below) -> i32 {
    (-left(b)).max(0)
}

/// The clock runs on the ground, from the touchdown.
fn running(b: &Below) -> bool {
    matches!(b.phase, Phase::Landed | Phase::Foot)
}

/// One tick of the clock: the warning, the storm's drain.
#[inline(never)]
pub fn tick(b: &mut Below) {
    if !running(b) {
        return;
    }
    b.since = b.since.saturating_add(1);
    let now = left(b);
    if now <= i32::from(STORM_WARN) * 60 && b.pilot.said & walker::SAID_WARN == 0 {
        b.pilot.said |= walker::SAID_WARN;
        if now > 0 {
            b.note = Some(WARN[usize::from(b.site.look % 8)]);
        }
    }
    // Step G4 (BOARD-G3 item 7): when the storm starts the note says
    // what to do; it takes the countdown's place.
    if now <= 0 && b.pilot.said & walker::SAID_STORM == 0 {
        b.pilot.said |= walker::SAID_STORM;
        b.note = Some(if b.phase == Phase::Foot {
            STORM_FOOT
        } else {
            STORM_SHIP
        });
    }
    if b.phase == Phase::Foot && storm(b) > 0 && !walker::at_hatch(b) {
        b.pilot.suit -= STORM_DRAIN;
        walker::suit_check(b);
    }
}

/// The storm's share so far, 0..=256 (full after `STORM_CLOSE`).
pub fn closing(b: &Below) -> i32 {
    (storm(b) * 256 / i32::from(STORM_CLOSE)).min(256)
}

/// The fog (1/256 of the land's reach at level `lv`) the storm leaves:
/// from the whole reach down to `STORM_NEAR`.
pub fn fog(b: &Below, lv: usize) -> i32 {
    let k = closing(b);
    if k == 0 {
        return 256;
    }
    let mut reach = MID_R[lv.min(2)];
    if b.site.look == 6 {
        reach = reach * HAZE_LOOK_REACH / 100;
    }
    let near = STORM_NEAR * 256 / reach;
    256 - (256 - near) * k / 256
}

/// The palette's storm step (0..=`STORM_STEPS`): the App asks for a new
/// palette when it changes.
pub fn step(b: &Below) -> u8 {
    if !b.on {
        return 0;
    }
    (closing(b) * i32::from(STORM_STEPS) / 256) as u8
}

/// The storm's streaks: short lines crossing the view in the wind
/// (from the site's west, turned by the camera), more as it closes.
pub fn streaks(f: &mut Frame, area: &Rect, b: &Below, cam_yaw: u16, colour: u8) {
    let k = closing(b);
    if k == 0 {
        return;
    }
    let n = 8 + k as u32 * 40 / 256;
    // The wind blows toward +x; on the view it runs across by the sine
    // of its turn from the camera's ahead.
    let wind = 0x4000u16.wrapping_sub(cam_yaw);
    let dx = sin(wind) >> 11;
    let dy = 2 + (cos(wind).abs() >> 13);
    let (w, h) = (area.w() as u32, area.h() as u32);
    for i in 0..n {
        let r = hash(i, 0x57_0E);
        let speed = 3 + (r >> 28) as i32;
        // The tick count only goes up while the ship sits out the storm:
        // kept to 20 bits here so the products below cannot overflow.
        let t = (b.t & 0xF_FFFF) as i32 * speed;
        let x0 = ((r & 0xFFFF) as i32 + t * dx / 2).rem_euclid(w as i32);
        let y0 = (((r >> 16) & 0xFFF) as i32 + t * dy / 2).rem_euclid(h as i32);
        let (x, y) = (area.x0 + x0, area.y0 + y0);
        crate::poly::line(f, area, [x, y], [x + dx / 2, y + dy / 2], colour);
    }
}
