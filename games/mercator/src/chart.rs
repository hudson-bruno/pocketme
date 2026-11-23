//! The star chart: the systems near the ship (one pixel is 0.2 light
//! years) or the whole lens-shaped galaxy (one pixel is 0.4 light
//! years). Dots are
//! coloured by economy, bigger when surveyed; the ring is how far the
//! fuel in the tank reaches. The d-pad moves the selection to the
//! nearest system that way, a tap picks a dot.

use game_kit::Frame;

use crate::art::*;
use crate::fixed::wide;
use crate::galaxy::{self, DANGER_NAMES, Econ, SYSTEMS, System};
use crate::hud::{self, Txt, card, in_box};
use crate::poly::Rect;
use crate::scene::dot_px;
use crate::world::{self, World};

/// The map's part of the screen.
pub const MAP: Rect = Rect::new(0, 0, 160, 96);
/// Touch boxes under the info: ZOOM and JUMP.
pub const ZOOM_BOX: (i16, i16, u16, u16) = (100, 109, 26, 10);
pub const JUMP_BOX: (i16, i16, u16, u16) = (128, 109, 31, 10);

/// Where system `s` is on screen.
#[inline(never)]
pub fn at(w: &World, s: &System, galaxy_zoom: bool) -> (i32, i32) {
    if galaxy_zoom {
        // The lens is 51 x 33.6 light years: 127 x 84 pixels, centred.
        (16 + i32::from(s.x) / 4, 6 + i32::from(s.y) / 4)
    } else {
        (
            80 + (i32::from(s.x) - i32::from(w.sys.x)) / 2,
            48 + (i32::from(s.y) - i32::from(w.sys.y)) / 2,
        )
    }
}

#[inline(never)]
pub fn econ_colour(e: Econ) -> u8 {
    match e {
        Econ::Farm => ECON_FARM,
        Econ::Mine => ECON_MINE,
        Econ::Forge => ECON_FORGE,
        Econ::Lab => ECON_LAB,
        Econ::Haven => ECON_HAVEN,
    }
}

/// The system nearest to `from` in direction (dx, dy) (screen axes),
/// within 60 degrees either side; `from` itself when there is none.
#[inline(never)]
pub fn step(seed: u32, from: u8, dx: i32, dy: i32) -> u8 {
    let a = galaxy::system(seed, from);
    let mut best = (from, i64::MAX);
    for i in 0..SYSTEMS {
        let b = galaxy::system(seed, i as u8);
        let (ex, ey) = (
            i32::from(b.x) - i32::from(a.x),
            i32::from(b.y) - i32::from(a.y),
        );
        // Positions below 2^10 and dx, dy within +-1: every product
        // below is far inside i32.
        let along = ex.wrapping_mul(dx).wrapping_add(ey.wrapping_mul(dy));
        let across = ex
            .wrapping_mul(dy)
            .wrapping_sub(ey.wrapping_mul(dx))
            .unsigned_abs() as i32;
        // In front, and not more than 60 degrees off (across < 1.73 along).
        if along <= 0 || across.wrapping_mul(100) > along.wrapping_mul(173) {
            continue;
        }
        let d = wide(ex, ex) + wide(ey, ey);
        if d < best.1 {
            best = (i as u8, d);
        }
    }
    best.0
}

/// The system whose dot is nearest a tap, within 8 pixels.
#[inline(never)]
pub fn system_at(w: &World, galaxy_zoom: bool, x: i16, y: i16) -> Option<u8> {
    let mut best = (None, 65);
    for i in 0..SYSTEMS {
        let s = galaxy::system(w.car.seed, i as u8);
        let (px, py) = at(w, &s, galaxy_zoom);
        // Dots and taps within a few thousand pixels: no sum wraps.
        let (ex, ey) = (px.wrapping_sub(i32::from(x)), py.wrapping_sub(i32::from(y)));
        let d = ex.wrapping_mul(ex).wrapping_add(ey.wrapping_mul(ey));
        if d < best.1 {
            best = (Some(i as u8), d);
        }
    }
    best.0
}

/// The chart. `docked`: the bottom line says SET instead of JUMP.
#[inline(never)]
pub fn draw(s: &mut Frame, w: &World, sel: u8, galaxy_zoom: bool, docked: bool) {
    s.rect(0, 0, 160, 96, 0);
    // A faint grid every 2 light years (local view only).
    if !galaxy_zoom {
        let (ox, oy) = (i32::from(w.sys.x) % 40, i32::from(w.sys.y) % 40);
        let mut x = (80 - ox / 2).rem_euclid(20);
        while x < 160 {
            s.vline(x as i16, 0, 96, GRID);
            x += 20;
        }
        let mut y = (48 - oy / 2).rem_euclid(20);
        while y < 96 {
            s.hline(0, y as i16, 160, GRID);
            y += 20;
        }
    }
    let here = w.sys;
    let (hx, hy) = at(w, &here, galaxy_zoom);
    // Fuel range ring.
    let r = i32::from(w.car.fuel) / if galaxy_zoom { 4 } else { 2 };
    ring(s, hx, hy, r, MAP_RANGE);
    let seed = w.car.seed;
    let chosen = galaxy::system(seed, sel);
    let (sx, sy) = at(w, &chosen, galaxy_zoom);
    // Names never overlap: each label's box (x, y, width) is kept, the
    // chosen system's first; a name that would touch one is left out.
    let mut boxes = [(0i32, 0i32, 0i32); 16];
    boxes[0] = (
        sx.wrapping_add(5),
        sy,
        hud::width(chosen.name().bytes()) as i32,
    );
    let mut nb = 1;
    for i in 0..SYSTEMS {
        let sys = galaxy::system(seed, i as u8);
        let (x, y) = at(w, &sys, galaxy_zoom);
        if !(0..160).contains(&x) || !(0..96).contains(&y) {
            continue;
        }
        let reach = here.dist(&sys) <= u32::from(w.car.fuel);
        // Surveyed systems are bigger dots.
        let size = i32::from(!galaxy_zoom)
            .wrapping_add(1)
            .wrapping_add(i32::from(w.car.charted(i as u8)));
        dot_px(
            s,
            &MAP,
            x - size / 2,
            y - size / 2,
            econ_colour(sys.econ),
            size,
        );
        if !galaxy_zoom && reach && i as u8 != here.id && i as u8 != sel && nb < boxes.len() {
            // On the map: every sum below is small.
            let name = sys.name();
            let (lx, nw) = (x.wrapping_add(3), hud::width(name.bytes()) as i32);
            let free = boxes[..nb].iter().all(|&(bx, by, bw)| {
                lx >= bx.wrapping_add(bw).wrapping_add(5)
                    || bx >= lx.wrapping_add(nw).wrapping_add(5)
                    || y.wrapping_sub(by).abs() >= 7
            });
            if free {
                hud::put(s, lx as i16, y as i16 - 2, name.bytes(), TEXT_DIM, 1);
                boxes[nb] = (lx, y, nw);
                nb += 1;
            }
        }
    }
    // Here and the selection.
    s.hline(hx as i16 - 3, hy as i16, 7, TEXT);
    s.vline(hx as i16, hy as i16 - 3, 7, TEXT);
    ring(s, sx, sy, 4, ACCENT);
    if !galaxy_zoom {
        hud::put(
            s,
            sx as i16 + 5,
            sy as i16 - 2,
            chosen.name().bytes(),
            ACCENT,
            1,
        );
    }
    // Info.
    s.rect(0, 96, 160, 24, UI_BG);
    s.hline(0, 96, 160, UI_EDGE);
    hud::put(s, 3, 99, chosen.name().bytes(), ACCENT, 1);
    let mut t = Txt::new();
    t.s(chosen.econ.name())
        .s(" ")
        .s(DANGER_NAMES[usize::from(chosen.danger.min(3))])
        .at(s, 48, 99, econ_colour(chosen.econ));
    let mut t = Txt::new();
    t.s("TECH ")
        .num(u32::from(chosen.tech))
        .at(s, 124, 99, TEXT_DIM);
    let d = here.dist(&chosen);
    let mut t = Txt::new();
    t.tenths(d)
        .s(" LY  FUEL ")
        .tenths(u32::from(w.car.fuel))
        .at(
            s,
            3,
            106,
            if d <= u32::from(w.car.fuel) {
                TEXT
            } else {
                BAD
            },
        );
    s.text(
        3,
        113,
        if galaxy_zoom { "X: NEAR" } else { "X: GALAXY" },
        TEXT_DIM,
    );
    // The guild's fee for a first survey, or that it is done.
    let mut t = Txt::new();
    if w.car.charted(sel) {
        t.s("SURVEYED");
    } else {
        t.s("SURVEY +").num(world::survey_fee(&chosen));
    }
    t.at(s, 48, 113, TEXT_DIM);
    card(s, ZOOM_BOX);
    s.text(ZOOM_BOX.0 + 3, ZOOM_BOX.1 + 3, "ZOOM", TEXT_DIM);
    card(s, JUMP_BOX);
    s.text(
        JUMP_BOX.0 + 4,
        JUMP_BOX.1 + 3,
        if docked { "SET" } else { "JUMP" },
        ACCENT,
    );
    hud::back_box(s);
}

/// A thin circle (points every few degrees).
#[inline(never)]
fn ring(s: &mut Frame, cx: i32, cy: i32, r: i32, c: u8) {
    if r <= 0 {
        return;
    }
    let steps = (r * 6).clamp(16, 256) as u32;
    for k in 0..steps {
        let a = (k * 65536 / steps) as u16;
        let x = cx + ((crate::fixed::sin(a) * r + 8192) >> 14);
        let y = cy + ((crate::fixed::cos(a) * r + 8192) >> 14);
        dot_px(s, &MAP, x, y, c, 1);
    }
}

/// Which chart box a tap hits: 0 zoom, 1 jump, 2 back.
#[inline(never)]
pub fn box_at(x: i16, y: i16) -> Option<u8> {
    if in_box(x, y, ZOOM_BOX) {
        Some(0)
    } else if in_box(x, y, JUMP_BOX) {
        Some(1)
    } else if in_box(x, y, hud::BACK_BOX) {
        Some(2)
    } else {
        None
    }
}
