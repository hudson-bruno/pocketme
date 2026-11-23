//! Step G2: the landing panel (SPEC 3.9, "The panel, LANDING"), in the
//! flight panel's place and style:
//!
//! - left: the site map, a 22 px circle 200 m across each way, the
//!   camera's ahead up, with the pad and the ship;
//! - middle: HUL (hull) and JET (heat) bars, ALT in m; V (the speed
//!   down, red over 4 m/s, with its arrow) and H (sideways, red over
//!   3 m/s); on the ground LANDED and HOLD B: LIFT OFF instead;
//! - right: the JETS box (a finger's box): ASSIST lit while it is on,
//!   its mode (SINK, HOLD or OFF), HOT while the jets are cut out.

use super::Below;
use super::lander::{self, ASSIST_HOLD, ASSIST_OFF, HEAT_UNIT, MPS, Phase};
use super::tune::{self, ENTRY_SKIP, HEAT_MAX, LAND_S, LAND_V, LIFT_HOLD};
use crate::art::{ACCENT, BAD, GOOD, PANEL_BG, RADAR_LINE, TEXT, TEXT_DIM, UI_EDGE};
use crate::fixed::{cos, sin};
use crate::hud::{self, RADAR, Txt};
use crate::world::M;
use game_kit::Frame;

/// The panel's top row.
const Y: i16 = 96;
/// The site map's range (world units) over its radius.
const MAP_RANGE: i32 = 200 * M;

/// The site map: the pad and the ship, the camera's ahead up.
fn map(f: &mut Frame, b: &Below) {
    hud::ring(f, RADAR, RADAR_LINE);
    let at = lander::at(b);
    let (s, c) = (sin(b.cam_yaw), cos(b.cam_yaw));
    // The pad (the site's middle) from the ship.
    let (x, z) = (-at[0], -at[2]);
    let right = (i64::from(x) * i64::from(c) - i64::from(z) * i64::from(s)) >> 14;
    let ahead = (i64::from(x) * i64::from(s) + i64::from(z) * i64::from(c)) >> 14;
    let r = i64::from(RADAR.2);
    let (mut px, mut py) = (
        right * r / i64::from(MAP_RANGE),
        ahead * r / i64::from(MAP_RANGE),
    );
    let len = crate::fixed::isqrt((px * px + py * py) as u64) as i64;
    if len > r {
        px = px * r / len;
        py = py * r / len;
    }
    hud::blip_at(f, (RADAR.0 + px as i32, RADAR.1 - py as i32), ACCENT, 3);
    // Step G4: the wreck while its smoke rises (the guide from the air).
    if b.wreck < tune::WRECK_STAGES {
        let w = b.places.wreck;
        place(
            f,
            b.cam_yaw,
            [w[0] - at[0], w[1] - at[2]],
            MAP_RANGE,
            TEXT_DIM,
            1,
        );
    }
    hud::blip_at(f, (RADAR.0, RADAR.1), TEXT, 2);
}

/// A dot on a map for a place `d` (x, z, world units) from the map's
/// middle, the camera's ahead up, `range` to the edge (kept on the
/// ring when farther).
fn place(f: &mut Frame, yaw: u16, d: [i32; 2], range: i32, c: u8, size: i32) {
    let (s, co) = (sin(yaw), cos(yaw));
    let (x, z) = (i64::from(d[0]), i64::from(d[1]));
    let right = (x * i64::from(co) - z * i64::from(s)) >> 14;
    let ahead = (x * i64::from(s) + z * i64::from(co)) >> 14;
    let r = i64::from(RADAR.2);
    let (mut px, mut py) = (right * r / i64::from(range), ahead * r / i64::from(range));
    let len = crate::fixed::isqrt((px * px + py * py) as u64) as i64;
    if len > r {
        px = px * r / len;
        py = py * r / len;
    }
    hud::blip_at(f, (RADAR.0 + px as i32, RADAR.1 - py as i32), c, size);
}

/// A speed (fine a tick) in tenths of m/s.
fn tenths(v: i32) -> u32 {
    (i64::from(v.unsigned_abs().min(1 << 30)) * 10 / i64::from(MPS)) as u32
}

/// The arrow beside V: down (or up when rising).
fn arrow(f: &mut Frame, x: i16, y: i16, down: bool, c: u8) {
    f.vline(x + 1, y, 5, c);
    let tip = if down { y + 3 } else { y + 1 };
    f.pixel(x, tip, c);
    f.pixel(x + 2, tip, c);
}

/// Draws the landing panel (on foot: [`foot`]).
pub fn draw(f: &mut Frame, b: &Below) {
    f.rect(0, Y, 160, 24, PANEL_BG);
    f.hline(0, Y, 160, UI_EDGE);
    if super::on_foot(b) {
        foot(f, b);
        return;
    }
    map(f, b);
    // The middle: hull, heat, height.
    let (lx, bx, bw) = (40, 58, 22);
    f.text(lx, 98, "HUL", TEXT_DIM);
    let low = i32::from(b.hull) * 4 < i32::from(b.hull_max);
    hud::bar(
        f,
        bx,
        98,
        bw,
        i32::from(b.hull),
        i32::from(b.hull_max),
        if low { BAD } else { GOOD },
    );
    f.text(lx, 105, "JET", if b.hot { BAD } else { TEXT_DIM });
    hud::bar(
        f,
        bx,
        105,
        26,
        b.heat / HEAT_UNIT,
        HEAT_MAX,
        if b.hot { BAD } else { ACCENT },
    );
    f.text(lx, 112, "ALT", TEXT_DIM);
    let alt = (lander::alt(b) >> lander::TO_WORLD).max(0) / M;
    hud::num_at(f, bx, 112, alt as u32, TEXT);
    // Beside them: the speeds, or the ground's lines.
    let x = 84;
    match b.phase {
        Phase::Landed => {
            f.text(x, 98, "LANDED", GOOD);
            let c = if b.hold > 0 { ACCENT } else { TEXT };
            f.text(x, 105, "HOLD B:", c);
            f.text(x, 112, "LIFT OFF", c);
            if b.hold > 0 {
                let w = u32::from(b.hold) * 32 / u32::from(LIFT_HOLD);
                f.hline(x, 118, w as u16, ACCENT);
            }
        }
        Phase::TakeOff | Phase::Foot | Phase::Board => {
            f.text(x, 98, "LIFT OFF", ACCENT);
        }
        Phase::Entry | Phase::Fly => {
            let down = b.v[1] <= 0;
            let v = tenths(b.v[1]);
            let vc = if down && v * 100 > LAND_V as u32 {
                BAD
            } else {
                TEXT
            };
            let mut t = Txt::new();
            t.s("V ").tenths(v);
            let end = t.at(f, x, 98, vc);
            arrow(f, end + 1, 98, down, vc);
            let h = tenths(lander::hspeed(b.v));
            let hc = if h * 100 > LAND_S as u32 { BAD } else { TEXT };
            let mut t = Txt::new();
            t.s("H ").tenths(h);
            t.at(f, x, 105, hc);
            if b.phase == Phase::Entry && b.phase_t >= ENTRY_SKIP {
                f.text(x, 112, "A: SKIP", TEXT_DIM);
            }
        }
    }
    // The JETS box.
    let on = b.assist != ASSIST_OFF;
    f.text(127, 98, "ASSIST", if on { GOOD } else { TEXT_DIM });
    let mode = match b.assist {
        ASSIST_OFF => "OFF",
        ASSIST_HOLD => "HOLD",
        _ => "SINK",
    };
    f.text(127, 105, mode, if on { ACCENT } else { TEXT_DIM });
    if b.hot {
        f.text(127, 112, "HOT", BAD);
    } else {
        f.text(127, 112, "JETS", TEXT_DIM);
    }
}

// ---- Step G3: the panel on foot (SPEC 3.9, "The panel, ON FOOT") ------------
//
// - left: the site map, 60 m to its edge, the camera's ahead up: the
//   ship white, the seams not yet burst amber (G3 shows them without a
//   scan), the wreck grey, chunks as dots; step G4: the drones red
//   once the scan found them or while they aim, the boulders grey
//   once found;
// - middle: SUIT, JET (energy) and CUT (heat) bars, HOLD n/m;
// - right: the site clock, red under 30 s; B: BOARD at the hatch.

/// The map's range on foot (world units) over its radius.
const FOOT_RANGE: i32 = 60 * M;

/// A dot on the foot map for a place (x, z) seen from the pilot.
fn dot(f: &mut Frame, b: &Below, at: [i32; 2], c: u8, size: i32) {
    let me = b.pilot.at();
    place(
        f,
        b.cam_yaw,
        [at[0] - me[0], at[1] - me[2]],
        FOOT_RANGE,
        c,
        size,
    );
}

/// The clock as m:ss.
fn clock_txt(t: &mut Txt, ticks: i32) {
    let secs = (ticks.max(0) as u32).div_ceil(60);
    t.num(secs / 60).s(":");
    if secs % 60 < 10 {
        t.s("0");
    }
    t.num(secs % 60);
}

/// Draws the panel on foot.
pub fn foot(f: &mut Frame, b: &Below) {
    hud::ring(f, RADAR, RADAR_LINE);
    let ship = lander::at(b);
    for (k, s) in b.places.seams.iter().enumerate() {
        if b.seams[k] < tune::SEAM_STAGES {
            dot(f, b, *s, ACCENT, 2);
        }
    }
    if b.wreck < tune::WRECK_STAGES {
        dot(f, b, b.places.wreck, TEXT_DIM, 2);
    }
    for c in &b.chunks {
        if c.state != super::loose::CHUNK_NONE {
            let a = c.at();
            dot(f, b, [a[0], a[2]], GOOD, 1);
        }
    }
    for (k, r) in b.rocks.iter().enumerate() {
        if b.marks & super::mark_rock(k) != 0 && r.state != super::loose::ROCK_NONE {
            let a = r.at();
            dot(f, b, [a[0], a[2]], TEXT_DIM, 1);
        }
    }
    for (k, d) in b.drones.iter().enumerate() {
        let seen = b.marks & super::mark_drone(k) != 0 || d.mode == super::drones::AIM;
        if seen && d.alive() {
            dot(f, b, [d.p[0], d.p[2]], BAD, 2);
        }
    }
    dot(f, b, [ship[0], ship[2]], TEXT, 3);
    hud::blip_at(f, (RADAR.0, RADAR.1), GOOD, 1);
    let pl = &b.pilot;
    let (lx, bx, bw) = (40, 58, 22);
    let low = pl.suit * 100 < i32::from(pl.suit_max) * 60 * tune::SUIT_LOW;
    f.text(lx, 98, "SUIT", if low { BAD } else { TEXT_DIM });
    hud::bar(
        f,
        bx,
        98,
        bw,
        pl.suit.max(0) / 60,
        i32::from(pl.suit_max).max(1),
        if low { BAD } else { GOOD },
    );
    f.text(lx, 105, "JET", TEXT_DIM);
    hud::bar(
        f,
        bx,
        105,
        bw,
        pl.energy / 60,
        tune::HOP_ENERGY / 60,
        ACCENT,
    );
    let hot = b.cutter.hot;
    f.text(lx, 112, "CUT", if hot { BAD } else { TEXT_DIM });
    hud::bar(
        f,
        bx,
        112,
        bw,
        b.cutter.heat / 60,
        tune::CUT_HEAT_MAX / 60,
        if hot { BAD } else { ACCENT },
    );
    let x = 84;
    let mut t = Txt::new();
    t.s("HOLD ")
        .num(b.cargo.total())
        .s("/")
        .num(u32::from(b.room));
    let full = b.cargo.total() >= u32::from(b.room);
    t.at(f, x, 98, if full { BAD } else { TEXT });
    if super::walker::at_hatch(b) {
        let c = if b.hold > 0 { ACCENT } else { TEXT };
        f.text(x, 105, "HOLD B:", c);
        f.text(x, 112, "LIFT OFF", c);
        if b.hold > 0 {
            let w = u32::from(b.hold) * 32 / u32::from(LIFT_HOLD);
            f.hline(x, 118, w as u16, ACCENT);
        }
    } else if hot {
        f.text(x, 105, "HOT", BAD);
    }
    // The clock.
    let left = super::clock::left(b);
    let red = left <= i32::from(tune::STORM_WARN) * 60;
    f.text(127, 98, "TIME", TEXT_DIM);
    let mut t = Txt::new();
    clock_txt(&mut t, left);
    let blink = left > 0 || (b.t / 15).is_multiple_of(2);
    if blink {
        t.at(f, 127, 105, if red { BAD } else { TEXT });
    }
    if left <= 0 {
        f.text(127, 112, "STORM", BAD);
    } else if pl.energy >= tune::SCAN_COST && b.scan_t >= tune::SCAN_GROW {
        // Step G4: the scan is there to use.
        f.text(127, 112, "Y SCAN", TEXT_DIM);
    }
}
