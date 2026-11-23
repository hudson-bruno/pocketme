//! Everything drawn flat: the flight panel (radar, bars, the charter's
//! gauges), the station beacon,
//! marks on the view, notes, and the cards and menus of every screen.
//! Numbers are written with [`Txt`], not with Rust's formatting, which
//! would cost about 2 KB of the code slot.

use game_kit::{Frame, text_width};

use crate::art::*;
use crate::fixed::{ONE14, V3, isqrt, norm, sub, wide};
use crate::galaxy;
use crate::market::{self, NAMES};
use crate::poly::Rect;
use crate::scene::{self, VIEW, blip_of, disc, dot_px};
use crate::world::{self, CRAFTS, GEAR, KM, Kind, M, Mode, PLAYER, Ship, World};

/// A short text built from pieces and numbers.
pub struct Txt {
    b: [u8; 40],
    n: usize,
}

impl Default for Txt {
    fn default() -> Self {
        Txt::new()
    }
}

impl Txt {
    pub const fn new() -> Txt {
        Txt { b: [0; 40], n: 0 }
    }

    #[inline(never)]
    pub fn s(&mut self, s: &str) -> &mut Txt {
        self.b(s.as_bytes())
    }

    /// Letters kept as bytes (a system's name).
    #[inline(never)]
    pub fn b(&mut self, s: &[u8]) -> &mut Txt {
        for &c in s {
            if self.n < self.b.len() {
                self.b[self.n] = c;
                self.n = self.n.wrapping_add(1);
            }
        }
        self
    }

    #[inline(never)]
    pub fn num(&mut self, mut v: u32) -> &mut Txt {
        let mut d = [0u8; 10];
        let mut k = 0;
        // At most ten digits: k stays within 0..=10.
        loop {
            d[k] = b'0' + (v % 10) as u8;
            k = k.wrapping_add(1);
            v /= 10;
            if v == 0 {
                break;
            }
        }
        while k > 0 {
            k = k.wrapping_sub(1);
            if self.n < self.b.len() {
                self.b[self.n] = d[k];
                self.n = self.n.wrapping_add(1);
            }
        }
        self
    }

    /// `v` tenths as "12.3".
    #[inline(never)]
    pub fn tenths(&mut self, v: u32) -> &mut Txt {
        self.num(v / 10).s(".").num(v % 10)
    }

    /// A distance in world units as "850 M" or "12.3 KM".
    #[inline(never)]
    pub fn dist(&mut self, units: i32) -> &mut Txt {
        let m = (units.max(0) / M) as u32;
        if m < 1000 {
            self.num(m).s(" M")
        } else if m < 100_000 {
            self.tenths(m / 100).s(" KM")
        } else {
            self.num(m / 1000).s(" KM")
        }
    }

    /// For tests and tools: the game itself draws and logs the bytes
    /// ([`put`]), so the chip build links no UTF-8 check.
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(self.bytes()).unwrap_or("?")
    }

    /// The text's letters (plain ASCII).
    pub fn bytes(&self) -> &[u8] {
        self.b.get(..self.n).unwrap_or(&[])
    }

    /// Draws the text at (x, y); returns the x after it.
    #[inline(never)]
    pub fn at(&self, s: &mut Frame, x: i16, y: i16, c: u8) -> i16 {
        put(s, x, y, self.bytes(), c, 1)
    }
}

/// The printable ASCII letters, space to tilde, as one text: a letter
/// kept as a byte is drawn or logged as its one-letter slice of this.
const ASCII: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";

/// The one-letter text for byte `b` ("?" for anything not printable).
#[inline(never)]
pub fn letter(b: u8) -> &'static str {
    let i = usize::from(b.wrapping_sub(32));
    ASCII.get(i..i.wrapping_add(1)).unwrap_or("?")
}

/// Draws the letters `b` at (x, y), `k` times as large; returns the x
/// after them, like `Frame::text`.
#[inline(never)]
pub fn put(s: &mut Frame, mut x: i16, y: i16, b: &[u8], c: u8, k: u8) -> i16 {
    for &ch in b {
        x = s.text_scaled(x, y, letter(ch), c, k);
    }
    x
}

/// Width of the letters `b` at size 1, like `text_width`.
#[inline(never)]
pub fn width(b: &[u8]) -> u16 {
    // Texts are at most 40 letters of at most 6 pixels: no sum wraps.
    b.iter()
        .fold(0u16, |w, &ch| {
            w.wrapping_add(text_width(letter(ch))).wrapping_add(1)
        })
        .saturating_sub(1)
}

/// A number at (x, y).
#[inline(never)]
pub fn num_at(s: &mut Frame, x: i16, y: i16, v: u32, c: u8) {
    let mut t = Txt::new();
    t.num(v).at(s, x, y, c);
}

/// A text that ends at `x_end`.
#[inline(never)]
pub fn right(s: &mut Frame, x_end: i16, y: i16, text: &[u8], c: u8) {
    put(s, x_end.wrapping_sub(width(text) as i16), y, text, c, 1);
}

// ---- Layout --------------------------------------------------------------------

/// First row of the panel under the view.
pub const PANEL_Y: i16 = 96;
pub(crate) const RADAR: (i32, i32, i32) = (19, 108, 11);
/// Menu lines are this far apart.
pub const MENU_STEP: i16 = 8;
/// Cards: x, y, w, h.
pub const TITLE_CARD: (i16, i16, u16, u16) = (24, 50, 112, 68);
pub const TITLE_Y: i16 = 53;
pub const PAUSE_CARD: (i16, i16, u16, u16) = (26, 22, 108, 64);
pub const PAUSE_Y: i16 = 33;
pub const DOCK_CARD: (i16, i16, u16, u16) = (4, 60, 70, 46);
pub const DOCK_Y: i16 = 63;
/// Step M3: the card behind the docked screen's info lines (they stood
/// straight on the scene, which now moves).
pub const INFO_CARD: (i16, i16, u16, u16) = (77, 65, 82, 42);
/// Step M3: MARKET and OUTFIT over the live scene: the rows from under
/// the top line to below the last list row are the scene darkened
/// ([`shade`]); under them, where no text sits, it shows clear.
pub const SHADE_Y: (i16, i16) = (10, 92);
/// Market and outfit rows.
pub const LIST_Y: i16 = 20;
pub const LIST_STEP: i16 = 9;
/// The BACK box on the market, outfit and chart screens.
pub const BACK_BOX: (i16, i16, u16, u16) = (126, 1, 33, 9);

/// The line of a menu a tap hits, if any.
#[inline(never)]
pub fn line_at(x: i16, y: i16, card: (i16, i16, u16, u16), first: i16, lines: u8) -> Option<u8> {
    // Taps and cards are within a few hundred pixels: no sum wraps.
    if x < card.0 || x >= card.0.wrapping_add(card.2 as i16) || y < first.wrapping_sub(1) {
        return None;
    }
    let l = y.wrapping_sub(first).wrapping_add(1) / MENU_STEP;
    (l >= 0 && l < i16::from(lines)).then_some(l as u8)
}

#[inline(never)]
pub fn in_box(x: i16, y: i16, b: (i16, i16, u16, u16)) -> bool {
    // Boxes are on the picture: no sum wraps.
    x >= b.0 && x < b.0.wrapping_add(b.2 as i16) && y >= b.1 && y < b.1.wrapping_add(b.3 as i16)
}

#[inline(never)]
pub fn centred(s: &mut Frame, y: i16, text: &[u8], c: u8) {
    let w = width(text) as i16;
    put(s, 80 - w / 2, y, text, c, 1);
}

#[inline(never)]
pub fn card(s: &mut Frame, (x, y, w, h): (i16, i16, u16, u16)) {
    s.rect(x, y, w, h, UI_BG);
    s.outline(x, y, w, h, UI_EDGE);
}

/// A menu line: a marker when selected, the label, the value.
#[inline(never)]
pub fn menu_line(s: &mut Frame, x: i16, y: i16, on: bool, label: &str, value: &str, vx: i16) {
    // Places on the picture: the sums are small.
    if on {
        s.text(x.wrapping_add(4), y, ">", ACCENT);
    }
    s.text(
        x.wrapping_add(10),
        y,
        label,
        if on { TEXT } else { TEXT_DIM },
    );
    if !value.is_empty() {
        s.text(
            x.wrapping_add(vx),
            y,
            value,
            if on { ACCENT } else { TEXT_DIM },
        );
    }
}

#[inline(never)]
pub fn back_box(s: &mut Frame) {
    card(s, BACK_BOX);
    s.text(BACK_BOX.0 + 5, BACK_BOX.1 + 2, "BACK", TEXT_DIM);
}

// ---- Flight -------------------------------------------------------------------

/// Where `p` lies on a round scope at `c` (r: radius): the middle is
/// straight ahead, the rim straight behind.
#[inline(never)]
fn scope(w: &World, p: V3, c: (i32, i32, i32)) -> (i32, i32) {
    let l = w.me.rot.apply_t(norm(sub(p, w.me.pos), ONE14));
    // sin(angle / 2): 0 ahead, 1 behind.
    // l is a unit vector (|l[i]| <= 2^14) and the scope small, so every
    // sum and product below stays far inside i32.
    let k = isqrt((ONE14.wrapping_sub(l[2]).max(0) as u64).wrapping_mul(16384) / 2) as i32;
    let side = isqrt(wide(l[0], l[0]).wrapping_add(wide(l[1], l[1])) as u64) as i32;
    if side == 0 {
        return (c.0, if l[2] > 0 { c.1 } else { c.1.wrapping_sub(c.2) });
    }
    let r = c.2.wrapping_mul(k) / ONE14;
    (
        c.0.wrapping_add(l[0].wrapping_mul(r) / side),
        c.1.wrapping_sub(l[1].wrapping_mul(r) / side),
    )
}

#[inline(never)]
pub(crate) fn ring(s: &mut Frame, c: (i32, i32, i32), col: u8) {
    // A small ring on the panel: the sums are small.
    let area = Rect::new(0, 96, 160, 24);
    disc(s, &area, c.0, c.1, c.2.wrapping_add(1), col);
    disc(s, &area, c.0, c.1, c.2, RADAR_BG);
    let across = c.2.wrapping_mul(2).wrapping_add(1) as u16;
    s.hline(c.0.wrapping_sub(c.2) as i16, c.1 as i16, across, RADAR_LINE);
    s.vline(c.0 as i16, c.1.wrapping_sub(c.2) as i16, across, RADAR_LINE);
}

/// A blip on a scope (`p` from [`scope`]: on the panel).
#[inline(never)]
pub(crate) fn blip_at(s: &mut Frame, p: (i32, i32), c: u8, size: i32) {
    let all = Rect::new(0, 0, 160, 120);
    dot_px(s, &all, p.0.wrapping_sub(1), p.1.wrapping_sub(1), c, size);
}

#[inline(never)]
pub(crate) fn bar(s: &mut Frame, x: i16, y: i16, w: u16, v: i32, most: i32, c: u8) {
    let y = y.wrapping_add(1);
    s.rect(x, y, w, 3, BAR_BG);
    let most = most.max(1);
    let fill = (wide(v.max(0).min(most), i32::from(w)) / i64::from(most)) as u16;
    s.rect(x, y, fill, 3, c);
}

/// The panel under the view.
#[inline(never)]
pub fn panel(s: &mut Frame, w: &World) {
    s.rect(0, PANEL_Y, 160, 24, PANEL_BG);
    s.hline(0, PANEL_Y, 160, UI_EDGE);
    // Radar: every ship within 12 km, and the station.
    ring(s, RADAR, RADAR_LINE);
    for sh in w.ships.iter().flatten() {
        if world::within(sh.pos, w.me.pos, 12 * KM) {
            let (x, y) = scope(w, sh.pos, RADAR);
            blip_at(s, (x, y), blip_of(sh), 2);
        }
    }
    let (x, y) = scope(w, w.bodies.station_pos, RADAR);
    blip_at(s, (x, y), BLIP_STATION, 2);
    // The auto-dock's key, and the guild charter's two gauges: surveys
    // and credits.
    let c = &w.car;
    s.text(124, 98, "AUTO", if w.me.auto { GOOD } else { TEXT_DIM });
    // TAGs carried, while there are any: on AUTO's row, ending where
    // the SVY and FND bars end (x 157), so at least 5 empty columns
    // (more than a word space) stay between AUTO (x 124..=141) and the
    // count. Every digit is 3 pixels wide plus 1 between: n digits
    // start at 159 - 4n.
    if c.tags > 0 {
        let x = if c.tags < 10 {
            155
        } else if c.tags < 100 {
            151
        } else {
            147
        };
        num_at(s, x, 98, u32::from(c.tags), ACCENT);
    }
    s.text(124, 105, "SVY", TEXT_DIM);
    bar(
        s,
        139,
        105,
        19,
        c.surveys() as i32,
        world::CHARTER_SURVEYS as i32,
        ACCENT,
    );
    s.text(124, 112, "FND", TEXT_DIM);
    bar(
        s,
        139,
        112,
        19,
        (c.credits / 64) as i32,
        (world::WIN_CREDITS / 64) as i32,
        GOOD,
    );
    // Bars.
    let me = &w.me;
    let (lx, bx) = (40, 54);
    s.text(lx, 98, "SPD", TEXT_DIM);
    // Step M5: against the ship's own top speed.
    let top = w.top();
    if me.speed > top {
        bar(s, bx, 98, 26, 1, 1, ACCENT);
    } else {
        bar(s, bx, 98, 26, me.speed, top, TEXT);
        // The throttle's mark.
        // The throttle is within 0..=top (at most 1536).
        let t = (me.throttle.wrapping_mul(25) / top) as i16;
        s.vline(bx.wrapping_add(t), 98, 5, ACCENT);
    }
    // Step M4: the overboost's charge, a line under the speed: amber
    // while it runs, green when it can start, dim below that.
    let od = i32::from(me.od);
    let c = if me.boost {
        ACCENT
    } else if me.od >= world::OD_MIN {
        GOOD
    } else {
        TEXT_DIM
    };
    s.hline(
        bx,
        103,
        (od.wrapping_mul(26) / i32::from(w.od_full())) as u16,
        c,
    );
    s.text(lx, 105, "SHD", TEXT_DIM);
    bar(s, bx, 105, 26, me.shield, w.shield_max(), SHIELD);
    s.text(lx, 112, "HUL", TEXT_DIM);
    let hull = i32::from(w.car.hull);
    let most = w.car.hull_max();
    // Red below 30 % of the ship's hull (hulls at most 160: small).
    bar(
        s,
        bx,
        112,
        26,
        hull,
        most,
        if hull * 10 < most * 3 { BAD } else { GOOD },
    );
    let (rx, rb) = (84, 102);
    s.text(rx, 98, "HEAT", TEXT_DIM);
    bar(
        s,
        rb,
        98,
        18,
        me.heat,
        world::HEAT_MAX,
        if me.hot { BAD } else { HEAT },
    );
    s.text(rx, 105, "FUEL", TEXT_DIM);
    bar(
        s,
        rb,
        105,
        18,
        i32::from(w.car.fuel),
        i32::from(world::TANK),
        FUEL,
    );
    let mut t = Txt::new();
    t.num(w.car.credits).s(" CR").at(s, rx, 112, TEXT);
}

/// The ship names of the view's target line, by [`Kind`].
pub const KIND_NAMES: [&str; 4] = ["RAIDER", "HAULER", "WARDEN", "CANISTER"];

/// The ships' marks on the view: brackets round the ships hunting the
/// player and round canisters (a closed amber box when the laser
/// reaches them, [`World::in_reach`]; a raider's TAG gets four corners
/// in [`TAG_MARK`] at every distance). Returns the ship the target line
/// names, and where it is on the view: of the ships drawn there
/// ([`seen`]), the one a shot would hit now ([`World::first_hit`]),
/// else the one nearest the view's middle.
#[inline(never)]
pub fn ship_marks<'a>(
    s: &mut Frame,
    w: &'a World,
    cam: &crate::poly::Camera,
) -> Option<(&'a Ship, i32, i32)> {
    let hit = w.first_hit().map(|h| h.0);
    let mut best = None;
    let mut most = u32::MAX;
    for (i, sh) in w.ships.iter().enumerate() {
        let Some(sh) = sh else { continue };
        let Some((x, y)) = seen(w, cam, sh.pos) else {
            continue;
        };
        if (sh.target == PLAYER && sh.mode == Mode::Hunt)
            || (sh.kind == Kind::Canister && sh.load.0 != world::ROCK)
        {
            // A fixed 9 px (step M2: inside a big or near ship's
            // outline, there is no distance floor any more). A TAG:
            // four corners in its own colour at every distance.
            let (c, reach) = if sh.load.0 == world::TAG {
                (TAG_MARK, false)
            } else {
                (blip_of(sh), w.in_reach(sh.pos))
            };
            bracket(s, x, y, 9, c, reach);
        }
        // On the picture: the sums are small. The hit ship counts as
        // 0 pixels off, every other at least 1.
        let d = if hit == Some(i) {
            0
        } else {
            x.wrapping_sub(80)
                .unsigned_abs()
                .wrapping_add(y.wrapping_sub(48).unsigned_abs())
                .wrapping_add(1)
        };
        if d < most {
            most = d;
            best = Some((sh, x, y));
        }
    }
    best
}

/// Whether the crosshair locks: a shot now would hit a ship
/// ([`World::first_hit`]) and the laser is not too hot to fire (nor
/// dead while the overboost runs, step M4).
pub fn locked(w: &World) -> bool {
    !w.me.hot && !w.me.boost && w.first_hit().is_some()
}

/// Where a ship at `p` is on the view, when it is near enough to be
/// drawn ([`scene::SHIP_SEEN`]) and its middle is on the view.
fn seen(w: &World, cam: &crate::poly::Camera, p: V3) -> Option<(i32, i32)> {
    if !world::within(p, w.me.pos, scene::SHIP_SEEN) {
        return None;
    }
    scene::on_screen(&VIEW, cam, p).filter(|&(x, y)| (0..160).contains(&x) && (0..96).contains(&y))
}

/// The target line for ship `sh` into `t`: its kind (a raider's TAG
/// is "RAIDER TAG") and distance ("RAIDER 1.8 KM", "HAULER 640 M"),
/// then IN RANGE when the laser reaches it ([`World::in_reach`]). The
/// laser passes through a TAG, so a TAG's line never says IN RANGE: it
/// ends in FLY INTO IT at any distance ("RAIDER TAG 420 M  FLY INTO
/// IT"; at most 124 pixels wide in play, 130 at the longest distance
/// text there is: clear of the beacon in the view's lower corners; a
/// beacon on the bottom edge is drawn first, under the letters). Returns
/// whether the laser reaches the ship.
#[inline(never)]
pub fn target_text(t: &mut Txt, w: &World, sh: &Ship) -> bool {
    let reach = w.in_reach(sh.pos);
    let tag = sh.load.0 == world::TAG;
    let name = if tag {
        "RAIDER TAG"
    } else if sh.load.0 == world::ROCK {
        "ROCK"
    } else {
        KIND_NAMES[sh.kind as usize]
    };
    t.s(name)
        .s(" ")
        .dist(crate::fixed::length(sub(sh.pos, w.me.pos)))
        .s(if tag {
            "  FLY INTO IT"
        } else if reach {
            "  IN RANGE"
        } else {
            ""
        });
    reach
}

/// Marks on the view: crosshair, brackets, the target line, the top
/// line, the hit flash.
///
/// The laser's own rules decide the marks. The crosshair locks (amber,
/// with a box in its middle) when a shot now would hit ([`locked`]). A
/// bracketed ship within laser reach ([`World::in_reach`]) gets a
/// closed amber box instead of the four corners in its own colour. The
/// target line names one ship ([`ship_marks`]) and its distance, with a
/// small bar under that ship: amber when the laser reaches it, else dim.
/// A raider's TAG (the laser passes through it) has its line, bar and
/// corners in [`TAG_MARK`] at every distance.
#[inline(never)]
pub fn view_marks(s: &mut Frame, w: &World, cam: &crate::poly::Camera, cockpit: bool) {
    let (cx, cy) = (80i16, 48i16);
    let lock = locked(w);
    let c = if w.me.hot || w.me.boost {
        BAD
    } else if lock {
        ACCENT
    } else {
        TEXT_DIM
    };
    if cockpit {
        s.hline(cx - 6, cy, 4, c);
        s.hline(cx + 3, cy, 4, c);
        s.vline(cx, cy - 6, 4, c);
        s.vline(cx, cy + 3, 4, c);
        if lock {
            s.outline(cx - 2, cy - 2, 5, 5, c);
        }
    }
    // Brackets round the station and the ships hunting the player; off
    // the view, a beacon on its edge shows the station's way.
    let st = cam.rot.apply(sub(w.bodies.station_pos, cam.pos));
    match scene::on_screen(&VIEW, cam, w.bodies.station_pos) {
        Some((x, y)) if (0..160).contains(&x) && (10..96).contains(&y) => {
            bracket(s, x, y, 5, BLIP_STATION, false);
        }
        _ => beacon(s, st, BLIP_STATION),
    }
    // Step G1: the planet's ring and landing site.
    crate::planet::space::draw_marks(s, w, cam);
    // The target line, at the foot of the view.
    if let Some((sh, x, y)) = ship_marks(s, w, cam) {
        let mut t = Txt::new();
        let reach = target_text(&mut t, w, sh);
        let c = if sh.load.0 == world::TAG {
            TAG_MARK
        } else if reach {
            ACCENT
        } else {
            TEXT_DIM
        };
        centred(s, 87, t.bytes(), c);
        // Under the ship's bracket. On the view: the i16 values fit;
        // below the view the panel covers it.
        s.hline(x as i16 - 2, y as i16 + 12, 5, c);
    }
    // Top line: the way to the station; the drive's state.
    let mut t = Txt::new();
    t.s("STATION ").dist(w.station_dist()).at(s, 2, 2, TEXT_DIM);
    let (state, c) = if w.wanted {
        ("OUTLAW", BAD)
    } else if w.me.auto {
        ("AUTO DOCK", GOOD)
    } else if w.me.cruise {
        ("CRUISE", ACCENT)
    } else if w.me.boost {
        ("OVERBOOST", ACCENT)
    } else if w.room() <= 0 {
        ("NO CRUISE", TEXT_DIM)
    } else {
        ("", TEXT)
    };
    right(s, 158, 2, state.as_bytes(), c);
    if w.me.flash > 0 {
        s.outline(0, 0, 160, 96, BAD);
        s.outline(1, 1, 158, 94, BAD);
    }
}

/// The station's way on the view's edge, from its place `c` in the
/// camera's axes (straight behind: at the bottom); step G1: in colour
/// `col` (the landing site's beacon is green).
#[inline(never)]
pub(crate) fn beacon(s: &mut Frame, c: V3, col: u8) {
    // Positions are below 2^30: the sides cut to 2^18 times 74 fit.
    let (dx, dy) = (c[0] >> 12, (c[1] >> 12).wrapping_neg());
    let m = dx.unsigned_abs().max(dy.unsigned_abs()).max(1) as i32;
    // On a box round the view's middle: 74 across, 36 up or down.
    let (x, y) = if dx == 0 && dy == 0 {
        (80, 88)
    } else {
        (dx.wrapping_mul(74) / m + 80, dy.wrapping_mul(36) / m + 52)
    };
    let all = Rect::new(0, 0, 160, 96);
    disc(s, &all, x, y, 3, col);
    disc(s, &all, x, y, 1, PANEL_BG);
}

/// Four corners round (x, y), `r` out, in colour `c`; with `reach`, a
/// closed amber box instead (a ship within laser reach).
#[inline(never)]
pub(crate) fn bracket(s: &mut Frame, x: i32, y: i32, r: i32, c: u8, reach: bool) {
    if !(0..160).contains(&x) || !(0..96).contains(&y) {
        return;
    }
    // On the picture and r small: the i16 sums cannot wrap.
    let (x, y, r) = (x as i16, y as i16, r as i16);
    if reach {
        let side = r.wrapping_mul(2).wrapping_add(1) as u16;
        s.outline(x.wrapping_sub(r), y.wrapping_sub(r), side, side, ACCENT);
        return;
    }
    for (sx, sy) in [(-1i16, -1i16), (1, -1), (-1, 1), (1, 1)] {
        let (px, py) = (
            x.wrapping_add(sx.wrapping_mul(r)),
            y.wrapping_add(sy.wrapping_mul(r)),
        );
        s.hline(px.min(px.wrapping_sub(sx.wrapping_mul(2))), py, 3, c);
        s.vline(px, py.min(py.wrapping_sub(sy.wrapping_mul(2))), 3, c);
    }
}

pub const NOTE_H: u16 = 9;

pub fn note(s: &mut Frame, text: &[u8], y: i16) {
    // Notes are short and on the picture: no sum wraps.
    let w = width(text).wrapping_add(6);
    let x = 80i16.wrapping_sub((w / 2) as i16);
    s.rect(x, y, w, NOTE_H, UI_BG);
    put(s, x.wrapping_add(3), y.wrapping_add(2), text, ACCENT, 1);
}

// ---- Menus ------------------------------------------------------------------------

#[inline(never)]
pub fn rate_name(fast: bool) -> &'static str {
    if fast { "60 A SECOND" } else { "30 A SECOND" }
}

#[inline(never)]
pub fn pitch_name(invert: bool) -> &'static str {
    if invert { "INVERTED" } else { "NORMAL" }
}

/// A menu card's lines.
#[inline(never)]
pub fn menu(
    s: &mut Frame,
    card_at: (i16, i16, u16, u16),
    first: i16,
    sel: u8,
    lines: &[(&str, &str)],
    vx: i16,
) {
    card(s, card_at);
    for (i, (a, v)) in lines.iter().enumerate() {
        let y = first + i as i16 * MENU_STEP;
        menu_line(s, card_at.0, y, i as u8 == sel, a, v, vx);
    }
}

// ---- Docked -----------------------------------------------------------------------

/// Step M3: darkens the scene in the box (x0..x1, y0..y1) behind text:
/// every lit material to its darkest shade (the scene still shows as
/// dim shapes), everything else (space, stars, glows, effects) to the
/// panel colour. Text on it reads as on a card.
#[inline(never)]
pub fn shade(s: &mut Frame, x0: i16, y0: i16, x1: i16, y1: i16) {
    let (x0, x1) = (x0.clamp(0, 160) as usize, x1.clamp(0, 160) as usize);
    for y in y0.clamp(0, 120)..y1.clamp(0, 120) {
        let at = y as usize * 160;
        for c in s.0[at + x0..at + x1.max(x0)].iter_mut() {
            let m = *c % MATS;
            *c = if *c < MATS * SHADES && m != mat::BLACK && GLOW & (1 << m) == 0 {
                m + (SHADES - 1) * MATS
            } else {
                UI_BG
            };
        }
    }
}

/// Step M3: a text with the scene darkened in a box round it.
#[inline(never)]
fn shaded(s: &mut Frame, x: i16, y: i16, text: &str, c: u8) {
    // Texts and places on the picture: the sums are small.
    let w = width(text.as_bytes()) as i16;
    shade(
        s,
        x.wrapping_sub(2),
        y.wrapping_sub(1),
        x.wrapping_add(w).wrapping_add(2),
        y.wrapping_add(6),
    );
    s.text(x, y, text, c);
}

/// Step M5: [`shaded`] for a [`Txt`].
#[inline(never)]
pub(crate) fn shaded_txt(s: &mut Frame, x: i16, y: i16, t: &Txt, c: u8) {
    // Texts and places on the picture: the sums are small.
    let w = width(t.bytes()) as i16;
    shade(
        s,
        x.wrapping_sub(2),
        y.wrapping_sub(1),
        x.wrapping_add(w).wrapping_add(2),
        y.wrapping_add(6),
    );
    t.at(s, x, y, c);
}

/// Step M3: the top line of MARKET and OUTFIT and the darkened panel
/// under it.
#[inline(never)]
fn list_panel(s: &mut Frame) {
    s.rect(0, 0, 160, SHADE_Y.0 as u16, UI_BG);
    shade(s, 0, SHADE_Y.0, 160, SHADE_Y.1);
}

/// The system's name and what it is, at the top.
#[inline(never)]
pub fn system_head(s: &mut Frame, w: &World) {
    let name = w.sys.name();
    s.rect(0, 0, 160, 17, UI_BG);
    s.hline(0, 17, 160, UI_EDGE);
    put(s, 3, 2, name.bytes(), ACCENT, 2);
    let mut t = Txt::new();
    t.s(w.sys.econ.name())
        .s("  TECH ")
        .num(u32::from(w.sys.tech));
    right(s, 158, 2, t.bytes(), TEXT);
    let danger = galaxy::DANGER_NAMES[usize::from(w.sys.danger.min(3))];
    let c = if w.sys.danger >= 2 { BAD } else { TEXT_DIM };
    right(s, 158, 9, danger.as_bytes(), c);
}

/// The docked screen's status lines (credits, fuel, hull, hold).
#[inline(never)]
pub fn status(s: &mut Frame, w: &World, y: i16) {
    s.rect(0, y, 160, (120 - y) as u16, UI_BG);
    s.hline(0, y, 160, UI_EDGE);
    let c = &w.car;
    // y is a row on the picture: the sum is small.
    let y = y.wrapping_add(3);
    let mut t = Txt::new();
    t.num(c.credits).s(" CR").at(s, 3, y, ACCENT);
    let mut t = Txt::new();
    t.s("FUEL ")
        .tenths(u32::from(c.fuel))
        .s(" LY")
        .at(s, 56, y, TEXT);
    let mut t = Txt::new();
    t.s("HOLD ")
        .num(c.hold.total())
        .s("/")
        .num(c.capacity())
        .at(s, 112, y, TEXT);
}

/// Step M5: SHIPYARD is the fifth line (the card grew by one line).
pub const DOCK_LINES: u8 = 5;

#[inline(never)]
pub fn docked(s: &mut Frame, w: &World, sel: u8, cut: bool) {
    system_head(s, w);
    let lines = [
        ("LAUNCH", ""),
        ("MARKET", ""),
        ("OUTFIT", ""),
        ("CHART", ""),
        ("SHIPYARD", ""),
    ];
    menu(s, DOCK_CARD, DOCK_Y, sel, &lines, 40);
    card(s, INFO_CARD);
    // Step G1: what the planet's landing site holds.
    let mut t = Txt::new();
    crate::planet::space::dock_line(&mut t, w, cut);
    shaded_txt(s, 3, 20, &t, GOOD);
    let c = &w.car;
    // TAGs not paid here (a LAWLESS station, or an OUTLAW), and their sum.
    if c.tags > 0 {
        let mut t = Txt::new();
        t.s("TAGS ")
            .num(u32::from(c.tags))
            .s(": ")
            .num(u32::from(c.owed))
            .s(" CR")
            .at(s, 80, 68, ACCENT);
    }
    s.text(80, 76, c.standing(), ACCENT);
    // Step M5: the ship flown and its hull in percent of its own.
    let mut t = Txt::new();
    t.s(c.craft().name)
        .s(" HULL ")
        .num(u32::from(c.hull) * 100 / c.hull_max().max(1) as u32)
        .s("%")
        .at(s, 80, 84, TEXT_DIM);
    // The goal: the guild's charter, for surveys and credits.
    let mut t = Txt::new();
    t.s("SURVEYS ")
        .num(c.surveys())
        .s("/")
        .num(world::CHARTER_SURVEYS)
        .at(s, 80, 92, TEXT_DIM);
    let mut t = Txt::new();
    if c.won {
        t.s("CHARTER: YOURS");
    } else {
        t.s("CHARTER ").num(world::WIN_CREDITS).s(" CR");
    }
    t.at(s, 80, 100, if c.won { GOOD } else { TEXT_DIM });
    status(s, w, 108);
}

/// The market: every good, its prices here, what the hold has.
#[inline(never)]
pub fn market(s: &mut Frame, w: &World, sel: u8) {
    list_panel(s);
    s.text(3, 3, "MARKET", ACCENT);
    put(s, 36, 3, w.sys.name().bytes(), TEXT_DIM, 1);
    back_box(s);
    let y0 = LIST_Y - 8;
    s.text(48, y0, "BUY", TEXT_DIM);
    s.text(76, y0, "SELL", TEXT_DIM);
    s.text(104, y0, "HOLD", TEXT_DIM);
    s.text(132, y0, "STOCK", TEXT_DIM);
    for (g, name) in NAMES.iter().enumerate() {
        let y = LIST_Y + g as i16 * LIST_STEP;
        let on = g as u8 == sel;
        if on {
            s.rect(1, y - 2, 158, 9, UI_EDGE);
        }
        let made = market::made_here(w.sys.econ, g);
        s.text(3, y, name, if made { GOOD } else { TEXT });
        let held = u32::from(w.car.hold.0[g]);
        let cols = [
            (w.price(g), TEXT),
            (w.sell_price(g), TEXT),
            (held, if held > 0 { ACCENT } else { TEXT_DIM }),
            (u32::from(w.stock[g]), TEXT_DIM),
        ];
        for (k, (v, c)) in cols.into_iter().enumerate() {
            num_at(s, 48 + 28 * k as i16, y, v, c);
        }
    }
    shaded(s, 3, 94, "GREEN: MADE HERE, CHEAP", GOOD);
    // Danger widens the prices here (see `market::RISK_PAY`).
    if w.sys.danger >= 2 {
        shaded(s, 122, 94, "RISK PAY", BAD);
    }
    shaded(s, 3, 101, "A/RIGHT BUY  X/LEFT SELL", TEXT_DIM);
    status(s, w, 109);
}

pub const OUTFIT_LINES: u8 = 2 + GEAR.len() as u8;

/// The outfitter: fuel, repairs, gear.
#[inline(never)]
pub fn outfit(s: &mut Frame, w: &World, sel: u8) {
    list_panel(s);
    s.text(3, 3, "OUTFIT", ACCENT);
    let mut t = Txt::new();
    t.s("TECH ")
        .num(u32::from(w.sys.tech))
        .at(s, 40, 3, TEXT_DIM);
    back_box(s);
    for i in 0..OUTFIT_LINES {
        let y = LIST_Y + i as i16 * LIST_STEP;
        let on = i == sel;
        if on {
            s.rect(1, y - 2, 158, 9, UI_EDGE);
        }
        let mut t = Txt::new();
        let (name, c) = match i {
            0 => {
                let need = u32::from(world::TANK - w.car.fuel.min(world::TANK));
                if need == 0 {
                    t.s("FULL");
                } else {
                    t.num(need * world::FUEL_PRICE).s(" CR");
                }
                ("FUEL", TEXT)
            }
            1 => {
                let need = (w.car.hull_max() as u32).saturating_sub(u32::from(w.car.hull));
                if need == 0 {
                    t.s("SOUND");
                } else {
                    t.num(need * world::REPAIR_PRICE).s(" CR");
                }
                ("REPAIR HULL", TEXT)
            }
            _ => {
                let g = usize::from(i - 2);
                let (name, price, tech) = GEAR[g];
                if w.car.has(g) {
                    t.s("FITTED");
                    (name, GOOD)
                } else if !w.sold_here(g) {
                    t.s("TECH ").num(u32::from(tech));
                    (name, TEXT_DIM)
                } else {
                    t.num(price).s(" CR");
                    (name, TEXT)
                }
            }
        };
        s.text(3, y, name, c);
        right(s, 156, y, t.bytes(), c);
    }
    shaded(s, 3, 101, "A: BUY", TEXT_DIM);
    status(s, w, 109);
}

/// Step M5: the shipyard's rows (one a ship) and where the turning
/// model of the selected one is drawn.
pub const YARD_LINES: u8 = CRAFTS.len() as u8;
pub const YARD_VIEW: (i16, i16, u16, u16) = (98, 48, 60, 44);

/// Step M5: the shipyard, a panel over the live docked scene like the
/// market's: each ship's hold, top speed, hull and what it costs with
/// the flown ship traded in; under the rows the selected ship's
/// handling, shield and overboost, beside its turning model (drawn by
/// the app into [`YARD_VIEW`]).
#[inline(never)]
pub fn shipyard(s: &mut Frame, w: &World, sel: u8) {
    list_panel(s);
    s.text(3, 3, "SHIPYARD", ACCENT);
    let mut t = Txt::new();
    t.s("TECH ")
        .num(u32::from(w.sys.tech))
        .at(s, 48, 3, TEXT_DIM);
    back_box(s);
    let y0 = LIST_Y - 8;
    s.text(40, y0, "HOLD", TEXT_DIM);
    s.text(62, y0, "M/S", TEXT_DIM);
    s.text(84, y0, "HULL", TEXT_DIM);
    s.text(124, y0, "COST", TEXT_DIM);
    let c = &w.car;
    for (i, k) in CRAFTS.iter().enumerate() {
        let y = LIST_Y + i as i16 * LIST_STEP;
        let on = i as u8 == sel;
        if on {
            s.rect(1, y - 2, 158, 9, UI_EDGE);
        }
        let mine = usize::from(c.ship) == i;
        let sold = w.ship_sold_here(i);
        let col = if mine {
            GOOD
        } else if sold {
            TEXT
        } else {
            TEXT_DIM
        };
        s.text(3, y, k.name, col);
        // Speeds in units a tick: x 60 / 256 is metres a second.
        let cols = [
            u32::from(k.hold),
            (k.top * 15 / 64) as u32,
            u32::from(k.hull),
        ];
        for (n, v) in cols.into_iter().enumerate() {
            num_at(s, 40 + 22 * n as i16, y, v, col);
        }
        let mut t = Txt::new();
        if mine {
            t.s("FLYING");
        } else if !sold {
            t.s("TECH ").num(u32::from(k.tech));
        } else {
            let cost = c.ship_cost(i);
            if cost < 0 {
                t.s("+");
            }
            t.num(cost.unsigned_abs()).s(" CR");
        }
        right(s, 156, y, t.bytes(), col);
    }
    // The selected ship's details.
    let k = &CRAFTS[usize::from(sel) % CRAFTS.len()];
    s.rect(YARD_VIEW.0, YARD_VIEW.1, YARD_VIEW.2, YARD_VIEW.3, UI_BG);
    s.outline(YARD_VIEW.0, YARD_VIEW.1, YARD_VIEW.2, YARD_VIEW.3, UI_EDGE);
    let x = 3;
    let mut y = 50;
    let mut t = Txt::new();
    t.s("LIST ").num(k.price).s(" CR");
    shaded_txt(s, x, y, &t, TEXT);
    y += 8;
    let mut t = Txt::new();
    t.s("TURN ").num(k.turn[1] as u32 * 100 / 190).s("%");
    shaded_txt(s, x, y, &t, TEXT_DIM);
    y += 8;
    let mut t = Txt::new();
    t.s("SHIELD ").num(u32::from(k.shield) * 25).s("%");
    shaded_txt(s, x, y, &t, TEXT_DIM);
    y += 8;
    let mut t = Txt::new();
    t.s("BOOST ")
        .tenths(u32::from(k.od_full) / 18)
        .s(" S TO ")
        .num((k.top * i32::from(k.od_top) / 4 * 15 / 64) as u32);
    shaded_txt(s, x, y, &t, TEXT_DIM);
    y += 8;
    let back = c.craft().price * world::TRADE_IN.0 / world::TRADE_IN.1;
    let mut t = Txt::new();
    t.s("TRADE-IN ")
        .num(
            back.saturating_sub(
                (c.hull_max() - i32::from(c.hull)).max(0) as u32 * world::REPAIR_PRICE,
            ),
        )
        .s(" CR");
    shaded_txt(s, x, y, &t, TEXT_DIM);
    shaded(s, 3, 101, "A: BUY  B: BACK", TEXT_DIM);
    status(s, w, 109);
}

#[inline(never)]
/// The end card: ship lost (`pilot`: the pilot fell on foot below, the
/// same loss under its own name), or (`won`) the guild's charter granted.
pub fn lost(s: &mut Frame, w: &World, ready: bool, won: bool, pilot: bool) {
    let c = (20, 30, 120, 50);
    card(s, c);
    if won {
        centred(s, 36, b"GUILD CHARTER GRANTED", GOOD);
        centred(s, 44, b"YOU WIN, MAPMAKER", TEXT);
    } else {
        let title: &[u8] = if pilot { b"PILOT DOWN" } else { b"SHIP LOST" };
        centred(s, 36, title, BAD);
        centred(s, 44, b"CARGO AND TAGS LOST", TEXT);
    }
    let mut t = Txt::new();
    t.s("SURVEYS ")
        .num(w.car.surveys())
        .s("  DAY ")
        .num(u32::from(w.car.day));
    centred(s, 54, t.bytes(), TEXT_DIM);
    if won {
        if ready {
            centred(s, 66, b"A: FLY ON", TEXT);
        }
    } else if ready {
        centred(s, 62, b"A: BACK TO LAST DOCK", TEXT);
        centred(s, 70, b"B: TITLE", TEXT_DIM);
    }
}

/// The jump tunnel: rings of streaks rushing out from the middle.
#[inline(never)]
pub fn tunnel(s: &mut Frame, t: u32) {
    s.rect(0, 0, 160, 96, 0);
    for k in 0..28u32 {
        let a = (k * 2341).wrapping_add(t.wrapping_mul(97)) as u16;
        let (sx, sy) = (crate::fixed::sin(a), crate::fixed::cos(a));
        let phase = t.wrapping_mul(6).wrapping_add(k * 37) % 100;
        let r0 = (phase * phase / 80 + 2) as i32;
        let r1 = r0 + 4 + (phase / 6) as i32;
        let p = |r: i32| [80 + ((sx * r) >> 14), 48 + ((sy * r * 3 / 4) >> 14)];
        let c = if k % 2 == 0 { TUNNEL1 } else { TUNNEL2 };
        crate::poly::line(s, &VIEW.area, p(r0), p(r1), c);
    }
}
