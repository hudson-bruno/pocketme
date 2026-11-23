//! Step G1: the planet's side in space (SPEC 3.2): where the landing
//! site is, the ring above it, the entry rule, and the marks drawn in
//! flight and docked.
//!
//! These run in flight on every tick or picture, so they stay in RAM
//! (`hot` lines in flash.list): the ring test, the ring's lights, the
//! site's bracket and beacon, the docked SITE line. They are small and
//! read `World` only.

use super::tune::{
    DRONES_BASE, DRONES_MAX, GIANT, LIFT_FUEL, RING_ALT, RING_CLEAR, RING_HUNT, RING_LIGHTS,
    RING_NOSE, RING_R, RING_SEEN, RING_STEP, SITE_DIRS,
};
use crate::art::{ACCENT, FIRE1, GOOD, TEXT};
use crate::fixed::{ONE14, V3, cos, cross14, dot, length, norm, scale14, sin, sub};
use crate::hud::{self, Txt};
use crate::poly::Camera;
use crate::scene::{self, VIEW};
use crate::world::{self, Bodies, State, World};
use game_kit::Frame;

/// The site's direction from the planet's middle (16384 long) and the
/// ring's middle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Site {
    pub dir: V3,
    pub ring: V3,
}

/// Where this system's site is; None at the giant (no surface). Worked
/// out once by [`locate`] when the system is entered.
pub fn site(w: &World) -> Option<Site> {
    w.bodies.site
}

/// Where the site of system `h` (with planet `look`) lies, for its
/// bodies: on the sunlit side, the ring clear of the station. Called by
/// `World::enter` only (from flash: flash.list `cold`).
#[inline(never)]
pub fn locate(h: u32, look: u8, b: &Bodies) -> Option<Site> {
    if look == GIANT {
        return None;
    }
    // The sunlit side, twice, plus a seed direction.
    let sun_side = scale14(b.light, -2 * ONE14);
    let mut out = None;
    for &k in SITE_DIRS.iter() {
        let d = world::seed_dir(h, k);
        let dir = norm(world::add(sun_side, d), ONE14);
        let dir = if dir == [0, 0, 0] { d } else { dir };
        let ring = scale14(dir, b.planet_r.wrapping_add(RING_ALT));
        out = Some(Site { dir, ring });
        if length(sub(ring, b.station_pos)) > RING_CLEAR {
            break;
        }
    }
    out
}

/// What a tick's move did at the ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ring {
    Miss,
    Enter,
    /// Crossed going down but a rule says no (the note to give).
    Refused(&'static str),
}

/// The entry rule (SPEC 3.2), after a tick in flight that moved the ship
/// from `old`: it enters when the move crosses the ring's plane going
/// down within the ring, nose down, not too fast, no raider near, fuel
/// enough to lift off again. Each refusal is told once a crossing (a
/// crossing happens on one tick only).
#[inline(never)]
pub fn ring_check(w: &World, old: V3) -> Ring {
    if w.state != State::Flying {
        return Ring::Miss;
    }
    let Some(s) = site(w) else {
        return Ring::Miss;
    };
    let now = w.me.pos;
    let (d0, d1) = (dot(sub(old, s.ring), s.dir), dot(sub(now, s.ring), s.dir));
    if !(d0 > 0 && d1 <= 0) {
        return Ring::Miss;
    }
    // Where the move crosses the plane: a tick's move is below 2^13,
    // d0 / (d0 - d1) at most 1.
    let mv = sub(now, old);
    let den = (d0 - d1).max(1);
    let at = [0, 1, 2].map(|k| old[k] + (i64::from(mv[k]) * d0 / den) as i32);
    if length(sub(at, s.ring)) > RING_R {
        return Ring::Miss;
    }
    let me = &w.me;
    if w.hunted_within(RING_HUNT) {
        Ring::Refused("RING CLOSED: RAIDERS NEAR")
    } else if w.car.fuel < LIFT_FUEL {
        Ring::Refused("NEED 0.3 LY TO LIFT OFF")
    } else if (dot(me.rot.col(2), s.dir) >> 14) > -i64::from(RING_NOSE) {
        Ring::Refused("NOSE DOWN INTO THE RING")
    } else if me.speed > w.top() || me.cruise || me.boost {
        Ring::Refused("TOO FAST FOR THE RING")
    } else {
        Ring::Enter
    }
}

/// Two axes across `dir` (16384 long each).
pub fn across(dir: V3) -> (V3, V3) {
    let a = if dir[1].abs() < 12_000 {
        [0, ONE14, 0]
    } else {
        [ONE14, 0, 0]
    };
    let e1 = norm(cross14(dir, a), ONE14);
    (e1, cross14(dir, e1))
}

/// The entry hint's row: under the notes' (y 11, 9 high).
pub const HINT_Y: i16 = 22;

/// The ring's lights, the site's bracket (or its beacon on the view's
/// edge), in flight. Called from `hud::view_marks`.
#[inline(never)]
pub fn draw_marks(s: &mut Frame, w: &World, cam: &Camera) {
    let Some(st) = site(w) else {
        return;
    };
    let all = VIEW.area;
    // The ring: 8 amber points, one bright one stepping round.
    let mut lights_seen = false;
    if length(sub(st.ring, cam.pos)) <= RING_SEEN {
        let (e1, e2) = across(st.dir);
        let lit = (w.ticks / RING_STEP) % RING_LIGHTS;
        for i in 0..RING_LIGHTS {
            let a = (i * (65_536 / RING_LIGHTS)) as u16;
            let off = world::add(
                scale14(e1, cos(a).wrapping_mul(RING_R >> 8) >> 6),
                scale14(e2, sin(a).wrapping_mul(RING_R >> 8) >> 6),
            );
            let p = world::add(st.ring, off);
            if let Some((x, y)) = scene::on_screen(&VIEW, cam, p)
                && !scene::behind_planet(w, cam, p)
            {
                let c = if i == lit { FIRE1 } else { ACCENT };
                scene::dot_px(s, &all, x, y, c, 2);
                lights_seen |= (0..160).contains(&x) && (10..96).contains(&y);
            }
        }
    }
    // Step G2 (CHOICE, BOARD-G1.md instruction 2.3): with the ring's
    // lights in view and the ship above the ring but off its axis, a hint
    // under the notes' row.
    if lights_seen {
        let rel = sub(w.me.pos, st.ring);
        let up = (dot(rel, st.dir) >> 14) as i32;
        if up > 0 && length(sub(rel, scale14(st.dir, up))) > RING_R {
            hud::centred(s, HINT_Y, b"LINE THE SITE UP INSIDE THE RING", TEXT);
        }
    }
    // The site's spot on the ground: bracketed when seen, else a green
    // beacon on the edge.
    let spot = scale14(st.dir, w.bodies.planet_r);
    let seen = dot(sub(cam.pos, spot), st.dir) > 0;
    match scene::on_screen(&VIEW, cam, spot) {
        Some((x, y)) if seen && (0..160).contains(&x) && (10..96).contains(&y) => {
            hud::bracket(s, x, y, 4, GOOD, false);
            if y >= 18 {
                s.text(x as i16 - 7, y as i16 - 11, "SITE", GOOD);
            }
        }
        _ => hud::beacon(s, cam.rot.apply(sub(spot, cam.pos)), GOOD),
    }
}

/// The docked line about the site (SPEC 3.9): what is down there.
#[inline(never)]
pub fn dock_line(t: &mut Txt, w: &World, cut: bool) {
    if w.sys.look == GIANT {
        t.s("SITE  NO SURFACE");
    } else if cut {
        // Step G3: every seam and the wreck were cut today.
        t.s("SITE  CUT TODAY");
    } else {
        // Step G4: the seams' good by the look (GRAIN, ICE or ORE).
        let d = DRONES_BASE.saturating_add(w.sys.danger).min(DRONES_MAX);
        let good = crate::market::NAMES[usize::from(super::cutter::seam_good(w.sys.look))];
        t.s("SITE  ")
            .s(good)
            .s(" ")
            .num(super::tune::SEAMS as u32)
            .s("  WRECK  DRONES ")
            .num(u32::from(d));
    }
}

/// Step G3: every seam and the wreck of this system's site were cut
/// today (the docked line's CUT TODAY).
pub fn cut_today(b: &super::Below, w: &World) -> bool {
    b.taken_day == Some((w.car.system, w.car.day)) && b.taken == super::cutter::ALL_TAKEN
}
