//! Everything drawn around and over the 3D view: the top bar, the
//! dashboard with the BRAKE and BOOST boxes, the side panels of the
//! light detail, the countdown, the menus and cards, and where a finger
//! lands on them.

use game_kit::{Frame, text_width};

use crate::app::{Options, PAUSE_LINES, TITLE_LINES};
use crate::art::{
    ACCENT, BAD, CHARGE, CHARGE_DIM, GOOD, KART, TEXT, TEXT_DIM, UI_BG, UI_EDGE, UI_HI, WHITE,
};
use crate::race::{CHARGE_MAX, COUNTDOWN, KARTS, LAPS, Race, V_MAX};
use crate::render::{CX, Detail, TOP};
use crate::track::COURSES;

/// Top speed as shown (km/h).
pub const SHOWN_TOP: i32 = 120;

/// A short text built without a heap.
pub struct Buf {
    b: [u8; 32],
    n: usize,
}

impl Buf {
    pub fn new() -> Buf {
        Buf { b: [0; 32], n: 0 }
    }

    pub fn push(&mut self, s: &str) -> &mut Buf {
        for &c in s.as_bytes() {
            if self.n < self.b.len() {
                self.b[self.n] = c;
                self.n += 1;
            }
        }
        self
    }

    /// A number, with at least `digits` digits (zeros in front).
    pub fn num(&mut self, v: u32, digits: u32) -> &mut Buf {
        let mut d = [0u8; 10];
        let mut k = 0;
        let mut v = v;
        loop {
            d[k] = b'0' + (v % 10) as u8;
            v /= 10;
            k += 1;
            if v == 0 && k as u32 >= digits {
                break;
            }
        }
        while k > 0 {
            k -= 1;
            if self.n < self.b.len() {
                self.b[self.n] = d[k];
                self.n += 1;
            }
        }
        self
    }

    /// Ticks (1/60 s) as minutes, seconds and tenths: `1:02.3`.
    pub fn time(&mut self, ticks: u32) -> &mut Buf {
        let tenths = ticks / 6;
        self.num(tenths / 600, 1)
            .push(":")
            .num(tenths / 10 % 60, 2)
            .push(".")
            .num(tenths % 10, 1)
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.b[..self.n]).unwrap_or("")
    }
}

impl Default for Buf {
    fn default() -> Self {
        Buf::new()
    }
}

/// "1ST" to "5TH".
pub fn place_name(p: u8) -> &'static str {
    match p {
        1 => "1ST",
        2 => "2ND",
        3 => "3RD",
        4 => "4TH",
        _ => "5TH",
    }
}

fn centre(s: &mut Frame, y: i16, text: &str, c: u8) {
    let x = CX as i16 - (text_width(text) / 2) as i16;
    s.text(x, y, text, c);
}

fn panel(s: &mut Frame, x: i16, y: i16, w: u16, h: u16) {
    s.rect(x, y, w, h, UI_BG);
    s.outline(x, y, w, h, UI_EDGE);
}

// ---- Touch -------------------------------------------------------------------------

/// The pause box in the top bar starts here.
pub const PAUSE_X: i16 = 140;

/// What a finger on the race screen does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    /// Drive, steering by how far from the middle the finger is
    /// (-256 to 256).
    Drive(i32),
    Brake,
    Boost,
    Pause,
}

/// A box on the screen: (x, y, w, h).
pub type Rect = (i16, i16, u16, u16);

/// The BRAKE and BOOST boxes.
pub fn boxes(detail: Detail) -> (Rect, Rect) {
    match detail {
        Detail::Heavy => ((1, 106, 34, 13), (125, 106, 34, 13)),
        _ => ((1, 102, 38, 16), (121, 102, 38, 16)),
    }
}

/// How far from the middle a finger must be before it steers, and where
/// it steers fully (pixels).
pub const STEER_DEAD: i32 = 6;
pub const STEER_FULL: i32 = 56;

/// Steering from a finger's column.
pub fn steer_of(x: i16) -> i32 {
    let d = i32::from(x) - CX;
    let a = d.abs();
    if a <= STEER_DEAD {
        0
    } else {
        d.signum() * ((a - STEER_DEAD) * 256 / (STEER_FULL - STEER_DEAD)).min(256)
    }
}

/// What a finger at (x, y) does while racing.
pub fn zone_at(detail: Detail, x: i16, y: i16) -> Zone {
    if y < TOP as i16 && x >= PAUSE_X {
        return Zone::Pause;
    }
    let (b, o) = boxes(detail);
    let inside = |r: (i16, i16, u16, u16)| {
        x >= r.0 - 1 && x < r.0 + r.2 as i16 + 1 && y >= r.1 - 1 && y < r.1 + r.3 as i16 + 1
    };
    if inside(b) {
        Zone::Brake
    } else if inside(o) {
        Zone::Boost
    } else {
        Zone::Drive(steer_of(x))
    }
}

// ---- Racing ------------------------------------------------------------------------

/// The bar at the top: lap, time, place, (speed in the richest detail)
/// and the pause box.
pub fn top_bar(s: &mut Frame, race: &Race, detail: Detail) {
    s.rect(0, 0, 160, TOP as u16 - 1, UI_BG);
    s.hline(0, TOP as i16 - 1, 160, UI_EDGE);
    let mut b = Buf::new();
    b.push("L").num(race.lap(), 1).push("/").num(LAPS, 1);
    s.text(2, 2, b.as_str(), TEXT);
    let mut b = Buf::new();
    let t = if race.finished {
        race.finish_ticks
    } else {
        race.race_ticks()
    };
    b.time(t);
    s.text(24, 2, b.as_str(), TEXT);
    s.text(60, 2, place_name(race.place), ACCENT);
    if detail == Detail::Heavy {
        let mut b = Buf::new();
        b.num(speed_shown(race), 1);
        let x = s.text(84, 2, b.as_str(), TEXT);
        s.text(x + 1, 2, "KMH", TEXT_DIM);
        charge_bar(s, race, 112, 3, 24, 3);
    }
    // Pause: two bars.
    s.rect(PAUSE_X + 7, 2, 2, 5, TEXT_DIM);
    s.rect(PAUSE_X + 11, 2, 2, 5, TEXT_DIM);
}

/// The speed as shown, in km/h.
pub fn speed_shown(race: &Race) -> u32 {
    (race.karts[0].v.max(0) * SHOWN_TOP / V_MAX) as u32
}

fn charge_bar(s: &mut Frame, race: &Race, x: i16, y: i16, w: u16, h: u16) {
    let full = (race.charge * i32::from(w) / CHARGE_MAX) as u16;
    s.rect(x, y, w, h, CHARGE_DIM);
    let c = if race.boosting { WHITE } else { CHARGE };
    s.rect(x, y, full, h, c);
}

/// The dashboard below the view (light and normal), or the two boxes
/// over the view's lower corners (heavy).
pub fn dash(s: &mut Frame, race: &Race, detail: Detail, braking: bool) {
    let (b, o) = boxes(detail);
    if detail != Detail::Heavy {
        s.rect(0, 100, 160, 20, UI_BG);
        s.hline(0, 100, 160, UI_EDGE);
        let mut buf = Buf::new();
        buf.num(speed_shown(race), 1);
        let w = text_width(buf.as_str()) as i16 * 2;
        let x = s.text_scaled(84 - w, 103, buf.as_str(), TEXT, 2);
        s.text(x + 1, 108, "KMH", TEXT_DIM);
        charge_bar(s, race, 44, 115, 72, 3);
    }
    // Only the words light up while a box is in use: a whole box that
    // changes colour would cost its whole size to send.
    s.rect(b.0, b.1, b.2, b.3, UI_BG);
    s.outline(b.0, b.1, b.2, b.3, UI_EDGE);
    let label = if detail == Detail::Heavy {
        "BRK"
    } else {
        "BRAKE"
    };
    let lx = b.0 + (b.2 as i16 - text_width(label) as i16) / 2;
    s.text(
        lx,
        b.1 + (b.3 as i16 - 5) / 2,
        label,
        if braking { BAD } else { TEXT },
    );

    let ready = race.charge > 0;
    s.rect(o.0, o.1, o.2, o.3, UI_BG);
    s.outline(o.0, o.1, o.2, o.3, if ready { CHARGE } else { UI_EDGE });
    let label = if detail == Detail::Heavy {
        "BST"
    } else {
        "BOOST"
    };
    let lx = o.0 + (o.2 as i16 - text_width(label) as i16) / 2;
    let c = if race.boosting {
        WHITE
    } else if ready {
        CHARGE
    } else {
        TEXT_DIM
    };
    s.text(lx, o.1 + (o.3 as i16 - 5) / 2, label, c);
}

/// The frame round the light detail's small view (rows [`TOP`] to 100):
/// above the view the last and best lap and the lap as a strip with
/// every kart on it, right of it the order of the karts. All of it
/// changes seldom or in few rows, so it costs little to send. The left
/// side stays empty on purpose: anything that changed there would share
/// its rows with the view and widen what is sent.
pub fn light_frame(s: &mut Frame, race: &Race) {
    let v = Detail::Light.view();
    let (x0, x1) = (v.x0 as i16, v.x1 as i16);
    let (y0, y1) = (v.y0 as i16, v.y1 as i16);
    let top = TOP as i16;
    // Everything round the view, then a line round it.
    s.rect(0, top, 160, (y0 - top) as u16, UI_BG);
    s.rect(0, y1, 160, (100 - y1) as u16, UI_BG);
    s.rect(0, y0, x0 as u16, (y1 - y0) as u16, UI_BG);
    s.rect(x1, y0, (160 - x1) as u16, (y1 - y0) as u16, UI_BG);
    s.outline(
        x0 - 1,
        y0 - 1,
        (x1 - x0 + 2) as u16,
        (y1 - y0 + 2) as u16,
        UI_EDGE,
    );

    // Above the view: the last and the best lap.
    let mut b = Buf::new();
    let done = race.laps.iter().rposition(|&t| t > 0);
    b.push("LAST ");
    match done {
        Some(i) => b.time(race.laps[i]),
        None => b.push("-:--.-"),
    };
    b.push("  BEST ");
    if race.best_lap > 0 {
        b.time(race.best_lap);
    } else {
        b.push("-:--.-");
    }
    centre(s, top + 3, b.as_str(), TEXT);

    // The lap strip, across above the view: the start line at the left,
    // every kart where it is on the lap. It has rows of its own, so a
    // kart moving on it costs only those few rows to send.
    let (sx0, sx1) = (x0 + 4, x1 - 4);
    let sy = y0 - 7;
    s.hline(sx0, sy, (sx1 - sx0) as u16, UI_EDGE);
    s.rect(sx0, sy - 2, 1, 5, WHITE);
    s.rect(sx1, sy - 2, 1, 5, WHITE);
    let lap = race.lap_prog();
    for k in (0..KARTS).rev() {
        let p = race.karts[k].prog.rem_euclid(lap);
        let x = sx0 + (i64::from(p) * i64::from(sx1 - sx0) / i64::from(lap)) as i16;
        let c = KART + 2 * k as u8;
        if k == 0 {
            s.rect(x - 1, sy - 2, 3, 5, c);
        } else {
            s.rect(x - 1, sy - 1, 2, 3, c);
        }
    }

    // The order, first at the top.
    let mut order = [0usize; KARTS];
    for (k, o) in order.iter_mut().enumerate() {
        *o = k;
    }
    order.sort_unstable_by(|a, b| race.karts[*b].prog.cmp(&race.karts[*a].prog));
    for (i, k) in order.iter().enumerate() {
        let y = y0 + 4 + i as i16 * 12;
        let c = KART + 2 * *k as u8;
        let mut b = Buf::new();
        b.num(i as u32 + 1, 1);
        s.text(x1 + 4, y, b.as_str(), TEXT_DIM);
        s.rect(x1 + 10, y, 6, 5, c);
        if *k == 0 {
            s.outline(x1 + 9, y - 1, 8, 7, WHITE);
        }
    }
}

/// The start lights and "3", "2", "1", "GO" in a box at the top of the
/// view, below the start arch's banner.
pub fn countdown(s: &mut Frame, race: &Race, detail: Detail) {
    let (text, c, lit) = if race.tick < COUNTDOWN {
        let left = (COUNTDOWN - race.tick).div_ceil(60);
        (
            ["1", "2", "3"][(left as usize - 1).min(2)],
            ACCENT,
            4 - left.min(3),
        )
    } else if race.tick < COUNTDOWN + 45 {
        ("GO", GOOD, 4)
    } else {
        return;
    };
    let v = detail.view();
    let y = (v.y0 as i16 + 18).max(46);
    let w = text_width(text) as i16 * 2;
    let bw = 44u16;
    let bx = CX as i16 - bw as i16 / 2;
    panel(s, bx, y, bw, 22);
    // Three lamps: one more red each second, all green at the start.
    for k in 0..3u32 {
        let col = if lit == 4 {
            GOOD
        } else if k < lit {
            BAD
        } else {
            UI_EDGE
        };
        s.rect(bx + 5 + k as i16 * 13, y + 3, 8, 4, col);
    }
    s.text_scaled(CX as i16 - w / 2, y + 10, text, c, 2);
}

/// Where a note goes: in LIGHT inside the top of the view (not over the
/// lap times and the lap strip above it), just below the top bar while a
/// panel (pause menu, finish card) is up, else over the top of the view.
pub fn note_y(detail: Detail, panel_up: bool) -> i16 {
    if panel_up {
        TOP as i16
    } else if detail == Detail::Light {
        Detail::Light.view().y0 as i16 + 3
    } else {
        14
    }
}

/// The height of a note's box.
pub const NOTE_H: u16 = 11;

/// A short message, its box's top at `y`.
pub fn note(s: &mut Frame, text: &str, y: i16) {
    let w = text_width(text) + 8;
    let x = CX as i16 - (w / 2) as i16;
    panel(s, x, y, w, NOTE_H);
    s.text(x + 4, y + 3, text, ACCENT);
}

// ---- Menus -------------------------------------------------------------------------

/// The first line of the menus, and the line height.
pub const MENU_Y: i16 = 36;
pub const MENU_LINE: i16 = 10;
const MENU_X: i16 = 22;
const MENU_W: u16 = 116;

fn menu_line(s: &mut Frame, i: u8, sel: u8, name: &str, value: Option<&str>) {
    let y = MENU_Y + i16::from(i) * MENU_LINE;
    let on = i == sel;
    if on {
        s.rect(MENU_X + 2, y - 2, MENU_W - 4, 9, UI_HI);
    }
    s.text(MENU_X + 6, y, name, if on { WHITE } else { TEXT });
    if let Some(v) = value {
        let w = text_width(v) as i16;
        let x = MENU_X + MENU_W as i16 - 12 - w;
        s.text(x, y, v, if on { ACCENT } else { TEXT_DIM });
        if on {
            s.text(x - 6, y, "<", TEXT_DIM);
            s.text(MENU_X + MENU_W as i16 - 9, y, ">", TEXT_DIM);
        }
    }
}

/// The menu line at (x, y), if any, of a menu with `lines` lines.
pub fn menu_line_at(x: i16, y: i16, lines: u8) -> Option<u8> {
    if x < MENU_X || x >= MENU_X + MENU_W as i16 || y < MENU_Y - 3 {
        return None;
    }
    let i = (y - (MENU_Y - 3)) / MENU_LINE;
    (i < i16::from(lines)).then_some(i as u8)
}

/// Whether a tap on a value line is on its left arrow (to step back).
pub fn is_left_half(x: i16) -> bool {
    x < CX as i16 - 10
}

pub fn title_line_at(x: i16, y: i16) -> Option<u8> {
    menu_line_at(x, y, TITLE_LINES)
}

pub fn pause_line_at(x: i16, y: i16) -> Option<u8> {
    menu_line_at(x, y, PAUSE_LINES)
}

fn rate_name(fast: bool) -> &'static str {
    if fast { "60" } else { "30" }
}

/// The title: the name, the menu, the best times.
pub fn title(s: &mut Frame, sel: u8, o: &Options, best: &[u32; 3]) {
    let w = text_width("HAIRPIN") as i16 * 3;
    s.rect(0, TOP as i16, 160, 22, UI_BG);
    s.text_scaled(CX as i16 - w / 2 + 1, TOP as i16 + 4, "HAIRPIN", BAD, 3);
    s.text_scaled(CX as i16 - w / 2, TOP as i16 + 3, "HAIRPIN", ACCENT, 3);
    panel(
        s,
        MENU_X,
        MENU_Y - 5,
        MENU_W,
        TITLE_LINES as u16 * MENU_LINE as u16 + 5,
    );
    let course = COURSES[usize::from(o.course)].name;
    menu_line(s, 0, sel, "PLAY", None);
    menu_line(s, 1, sel, "COURSE", Some(course));
    menu_line(s, 2, sel, "DETAIL", Some(o.detail.name()));
    menu_line(s, 3, sel, "RATE", Some(rate_name(o.fast)));
    menu_line(s, 4, sel, "VIEW", Some(o.cam.name()));
    menu_line(s, 5, sel, "LEAVE", None);
    // The record of the chosen course.
    s.rect(0, 100, 160, 20, UI_BG);
    s.hline(0, 100, 160, UI_EDGE);
    let mut b = Buf::new();
    b.push("COURSE ")
        .num(u32::from(o.course) + 1, 1)
        .push(" BEST ");
    let rec = best[usize::from(o.course)];
    if rec == 0 {
        b.push("-:--.-");
    } else {
        b.time(rec);
    }
    centre(s, 104, b.as_str(), TEXT_DIM);
    centre(s, 112, "TOP 3 GOES ON TO THE NEXT", TEXT_DIM);
}

/// The game's pause menu.
pub fn pause(s: &mut Frame, sel: u8, o: &Options) {
    panel(
        s,
        MENU_X,
        MENU_Y - 15,
        MENU_W,
        PAUSE_LINES as u16 * MENU_LINE as u16 + 15,
    );
    centre(s, MENU_Y - 11, "PAUSED", ACCENT);
    menu_line(s, 0, sel, "RESUME", None);
    menu_line(s, 1, sel, "DETAIL", Some(o.detail.name()));
    menu_line(s, 2, sel, "RATE", Some(rate_name(o.fast)));
    menu_line(s, 3, sel, "VIEW", Some(o.cam.name()));
    menu_line(s, 4, sel, "RESTART", None);
    menu_line(s, 5, sel, "TITLE", None);
}

/// Lines of the finish card: go on (next course or again), title.
pub const CARD_LINES: u8 = 2;
const CARD_Y: i16 = 76;

/// The card after a race.
pub fn finish_card(s: &mut Frame, race: &Race, sel: u8, ready: bool, last: bool) {
    panel(s, 20, 22, 120, 76);
    let place = race.finish_place;
    let good = place <= 3;
    let w = text_width(place_name(place)) as i16 * 2;
    s.text_scaled(
        CX as i16 - w / 2,
        27,
        place_name(place),
        if good { GOOD } else { BAD },
        2,
    );
    let mut b = Buf::new();
    b.push("TIME ").time(race.finish_ticks);
    centre(s, 42, b.as_str(), TEXT);
    let mut b = Buf::new();
    b.push("BEST LAP ").time(race.best_lap);
    centre(s, 50, b.as_str(), TEXT_DIM);
    let msg = if !good {
        "TOP 3 TO GO ON"
    } else if last {
        "THE CUP IS DONE"
    } else {
        "ON TO THE NEXT"
    };
    centre(s, 60, msg, if good { ACCENT } else { TEXT_DIM });
    if !ready {
        return;
    }
    let first = if !good {
        "RACE AGAIN"
    } else if last {
        "SEE THE CUP"
    } else {
        "NEXT COURSE"
    };
    for (i, t) in [first, "TITLE"].iter().enumerate() {
        let y = CARD_Y + i as i16 * MENU_LINE;
        let on = i as u8 == sel;
        if on {
            s.rect(30, y - 2, 100, 9, UI_HI);
        }
        centre(s, y, t, if on { WHITE } else { TEXT });
    }
}

pub fn card_line_at(x: i16, y: i16) -> Option<u8> {
    if !(30..130).contains(&x) || y < CARD_Y - 3 {
        return None;
    }
    let i = (y - (CARD_Y - 3)) / MENU_LINE;
    (i < i16::from(CARD_LINES)).then_some(i as u8)
}

/// The end of the cup: each course's place and time.
pub fn cup(s: &mut Frame, places: &[u8; 3], times: &[u32; 3]) {
    s.rect(0, 0, 160, 120, UI_BG);
    s.outline(4, 4, 152, 112, UI_EDGE);
    // "CUP DONE" only when every course was raced in this cup (PLAY
    // can start at a later course).
    let head = if places.iter().all(|&p| p > 0) {
        "CUP DONE"
    } else {
        "RESULTS"
    };
    let w = text_width(head) as i16 * 2;
    s.text_scaled(CX as i16 - w / 2, 12, head, ACCENT, 2);
    let mut total = 0;
    for (c, course) in COURSES.iter().enumerate() {
        let y = 36 + c as i16 * 12;
        s.text(16, y, course.name, TEXT);
        if places[c] == 0 {
            s.text(84, y, "-", TEXT_DIM);
            continue;
        }
        s.text(
            84,
            y,
            place_name(places[c]),
            if places[c] <= 3 { GOOD } else { BAD },
        );
        let mut b = Buf::new();
        b.time(times[c]);
        s.text(108, y, b.as_str(), TEXT);
        total += times[c];
    }
    let mut b = Buf::new();
    b.push("TOTAL ").time(total);
    centre(s, 80, b.as_str(), WHITE);
    centre(s, 100, "PRESS A OR TAP", TEXT_DIM);
}
