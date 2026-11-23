//! Step G2: the way down and the way up (SPEC 3.3 ENTRY, 3.7 TAKE-OFF).
//!
//! ENTRY (180 ticks): the sky's bands come in one by one from the
//! horizon up out of black and the stars fade (0-120), 24 plasma streaks
//! run in from the view's edges, shading from the star's colour to the
//! air's, and the view shakes; the site fades in, the fog's far edge
//! growing from nothing to the detail's reach (60-180). The ship comes
//! down a fixed path to the landing start, seen from the chase camera.
//! A after tick 60 skips to the end.
//!
//! TAKE-OFF (180 ticks): the ship rises 60 m, a ring of dust blows out,
//! the camera pulls up and back, and from tick 90 the sky goes black
//! from the top down. Then the App takes the ship back to space.

use super::lander::{FINE, Phase};
use super::palette;
use super::tune::{
    DUST, DUST_FROM, ENTRY_FOG, ENTRY_OUT, ENTRY_SKY, ENTRY_TICKS, ENTRY_UP, RING_DUST, STREAKS,
    TAKEOFF_FADE, TAKEOFF_TICKS, TAKEOFF_UP,
};
use super::{Below, VIEW};
use crate::art;
use crate::fixed::{V3, cos, hash, norm, sin};
use crate::poly::{Camera, SUB, View, line};
use crate::scene::dot_px;
use crate::world::M;
use game_kit::Frame;

/// The entry path's start offset from the landing start (world units):
/// up, and away from the pad.
fn entry_offset(start: V3) -> V3 {
    let away = norm([start[0], 0, start[2]], ENTRY_OUT);
    [away[0], ENTRY_UP, away[2]]
}

/// The ship's feet (fine) on the entry's path at tick `t`: from the
/// path's start down to the landing start, arriving at rest.
pub fn entry_at(start: V3, t: u16) -> V3 {
    let off = entry_offset(start);
    let left = i64::from(ENTRY_TICKS.saturating_sub(t));
    let n = i64::from(ENTRY_TICKS);
    [0, 1, 2].map(|k| {
        let along = i64::from(off[k]) * left * left / (n * n);
        ((i64::from(start[k]) + along) * i64::from(FINE >> 8)) as i32
    })
}

/// The take-off's height over where the ship stood (fine) at tick `t`:
/// eased in and out.
pub fn takeoff_up(t: u16) -> i32 {
    let u = i64::from(t.min(TAKEOFF_TICKS));
    let n = i64::from(TAKEOFF_TICKS);
    // u^2 (3n - 2u) / n^3: 0 to 1.
    let e = u * u * (3 * n - 2 * u);
    (i64::from(TAKEOFF_UP) * i64::from(FINE >> 8) * e / (n * n * n)) as i32
}

/// The sky's bands lit (0..=10) now.
pub fn bands_lit(b: &Below) -> u8 {
    let all = i32::from(palette::SKY_BANDS);
    let n = match b.phase {
        Phase::Entry => i32::from(b.phase_t) * all / i32::from(ENTRY_SKY),
        Phase::TakeOff if b.phase_t > TAKEOFF_FADE => {
            let gone = i32::from(b.phase_t - TAKEOFF_FADE);
            all - gone * (all + 1) / i32::from(TAKEOFF_TICKS - TAKEOFF_FADE)
        }
        _ => all,
    };
    n.clamp(0, all) as u8
}

/// How far the fog lets the eye see, in 1/256 of the reach.
pub fn fog(b: &Below) -> i32 {
    if b.phase != Phase::Entry {
        return 256;
    }
    let t = i32::from(b.phase_t);
    let (from, to) = (i32::from(ENTRY_FOG), i32::from(ENTRY_TICKS));
    ((t - from).max(0) * 256 / (to - from)).min(256)
}

/// The stars, on the black part of the sky only, fewer as the bands
/// come in (24 at most).
pub fn stars(f: &mut Frame, view: &View, cam: &Camera, b: &Below) {
    let lit = u32::from(bands_lit(b));
    let all = u32::from(palette::SKY_BANDS);
    if lit >= all {
        return;
    }
    let n = STREAKS * (all - lit) / all;
    let a = view.area;
    for k in 0..n {
        let h = hash(k, 0x57A2);
        let yaw = h as u16;
        let up = 1000 + (h >> 16) % 12_000;
        let d = [
            (sin(yaw) * cos(up as u16)) >> 14,
            sin(up as u16),
            (cos(yaw) * cos(up as u16)) >> 14,
        ];
        let c = cam.rot.apply(d);
        if c[2] < 64 {
            continue;
        }
        let p = view.project(c);
        let (x, y) = (p[0].div_euclid(SUB), p[1].div_euclid(SUB));
        if x < a.x0 || x >= a.x1 || y < a.y0 || y >= a.y1 {
            continue;
        }
        if f.get(x as u16, y as u16) == 0 {
            f.pixel(x as i16, y as i16, art::STAR);
        }
    }
}

/// The entry's plasma streaks: 2 px lines running in from the view's
/// edges toward its middle, the star's colour shading to the air's.
pub fn streaks(f: &mut Frame, b: &Below) {
    if b.phase != Phase::Entry || b.phase_t >= ENTRY_SKY {
        return;
    }
    let t = u32::from(b.phase_t);
    let colour = palette::streak((t * 3 / u32::from(ENTRY_SKY)) as u8);
    let a = VIEW.area;
    let (cx, cy) = (a.x0 + a.w() / 2, a.y0 + a.h() / 2);
    for k in 0..STREAKS {
        let h = hash(k, 0x5713);
        let dir = h as u16;
        // From 96 px out in to 36 px, each at its own phase.
        let r = 96 - ((t * 2 + (h >> 16) % 60) % 60) as i32;
        let len = 6 + (h >> 24) as i32 % 10;
        let (s, c) = (sin(dir), cos(dir));
        let p = |r: i32| [cx + ((s * r) >> 14), cy + ((c * r) >> 14) * 3 / 4];
        let (o, i) = (p(r), p(r - len));
        line(f, &a, o, i, colour);
        line(f, &a, [o[0] + 1, o[1]], [i[0] + 1, i[1]], colour);
    }
}

/// Dust blown out from under the ship (under 15 m, more the lower it
/// is), and the take-off's ring. Drawn as specks on the ground.
pub fn dust(f: &mut Frame, view: &View, cam: &Camera, b: &Below, under: V3, alt: i32) {
    let (n, spread) = match b.phase {
        Phase::TakeOff if b.phase_t < TAKEOFF_TICKS / 2 => {
            (RING_DUST, 6 * M + i32::from(b.phase_t) * M / 3)
        }
        Phase::Fly | Phase::Entry if alt < DUST_FROM => {
            let n = DUST as i32 * (DUST_FROM - alt.max(0)) / DUST_FROM;
            (n as u32, 0)
        }
        _ => return,
    };
    let colour = palette::DUST;
    for k in 0..n {
        let h = hash(k, 0xD057);
        let a = h as u16;
        let r = if spread > 0 {
            spread + (h >> 16) as i32 % (2 * M)
        } else {
            // Running out from 4 m to 14 m, each at its own phase.
            4 * M + (b.t.wrapping_mul(9).wrapping_add(h >> 16) % 2560) as i32
        };
        let p = [
            under[0] + ((sin(a) * (r >> 4)) >> 10),
            under[1] + M / 4,
            under[2] + ((cos(a) * (r >> 4)) >> 10),
        ];
        let c = cam.rot.apply(crate::fixed::sub(p, cam.pos));
        if c[2] < M {
            continue;
        }
        let s = view.project(c);
        dot_px(
            f,
            &view.area,
            s[0].div_euclid(SUB),
            s[1].div_euclid(SUB),
            colour,
            2,
        );
    }
}
