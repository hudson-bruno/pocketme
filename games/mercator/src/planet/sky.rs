//! Step G1: the sky below (SPEC 3.10, "The sky"): ten bands from the
//! horizon up, filled as rows; under the horizon the first band (the
//! air at the horizon, which the far haze fades into); the sun as a
//! disc with a rim; the station as a point that blinks.

use super::palette;
use crate::art;
use crate::fixed::{ONE14, V3, cos, dot, norm, sin};
use crate::poly::{Camera, Rect, SUB, View, rect_fill};
use crate::scene::{disc, dot_px};
use game_kit::Frame;

/// Where each band starts above the horizon (65536 a turn): about 0, 1,
/// 2.5, 4.5, 7, 10, 14, 19, 26 and 36 degrees.
const BANDS: [u16; palette::SKY_BANDS as usize] =
    [0, 182, 455, 819, 1274, 1820, 2548, 3458, 4732, 6552];

/// The row (whole pixels, may be off the view) where the direction at
/// `elev` above the horizon, straight ahead, lands; None when it is not
/// in front of the eye.
fn row_of(view: &View, cam: &Camera, ahead: V3, elev: u16) -> Option<i32> {
    let d = [
        (ahead[0] * cos(elev)) >> 14,
        sin(elev),
        (ahead[2] * cos(elev)) >> 14,
    ];
    let c = cam.rot.apply(d);
    if c[2] < 64 {
        return None;
    }
    Some(view.project(c)[1].div_euclid(SUB))
}

/// Draws the sky and the ground's backdrop over the whole view. `sun`
/// and `station` point at them (16384 long, the site's axes); `t` is
/// the site's tick (the station blinks by it). Step G2: only the first
/// `lit` bands are drawn, the rest black (the entry brings them in from
/// the horizon up, the take-off takes them away from the top down).
#[inline(never)]
pub fn draw(f: &mut Frame, view: &View, cam: &Camera, sun: V3, station: V3, t: u32, lit: u8) {
    let band = |b: u8| if b < lit { palette::sky(b) } else { 0 };
    let a = view.area;
    // Straight ahead along the ground: the camera's nose, flattened.
    let fwd = cam.rot.0[2];
    let ahead = norm([fwd[0], 0, fwd[2]], ONE14);
    let ahead = if ahead == [0, 0, 0] {
        [0, 0, ONE14]
    } else {
        ahead
    };
    // From the horizon up: each band fills the rows from where it
    // starts up to the next band's start.
    let mut below = a.y1;
    let horizon = row_of(view, cam, ahead, 0).unwrap_or(a.y0 - 1);
    // Under the horizon: the first band.
    if horizon < a.y1 {
        let top = horizon.max(a.y0);
        rect_fill(f, &Rect::new(a.x0, top, a.w(), a.y1 - top), band(0));
        below = top;
    }
    for b in 0..BANDS.len() {
        let next = BANDS.get(b + 1).and_then(|&n| row_of(view, cam, ahead, n));
        let top = next.unwrap_or(a.y0).max(a.y0);
        if top < below {
            rect_fill(f, &Rect::new(a.x0, top, a.w(), below - top), band(b as u8));
            below = top;
        }
    }
    if lit == 0 {
        return;
    }
    // The sun, with a rim.
    let c = cam.rot.apply(sun);
    if c[2] > 0 {
        let p = view.project(c);
        let (x, y) = (p[0].div_euclid(SUB), p[1].div_euclid(SUB));
        disc(f, &a, x, y, 5, art::HALO);
        disc(f, &a, x, y, 4, art::FAR_SUN);
    }
    // The station: a point, bright for a second every 90 s; only above
    // the horizon (the land covers it otherwise).
    if dot(station, [0, ONE14, 0]) > 0 {
        let c = cam.rot.apply(station);
        if c[2] > 0 {
            let p = view.project(c);
            let lit = t % 5400 < 60;
            let (col, size) = if lit {
                (art::STAR_BRIGHT, 2)
            } else {
                (art::STAR, 1)
            };
            dot_px(f, &a, p[0].div_euclid(SUB), p[1].div_euclid(SUB), col, size);
        }
    }
}
