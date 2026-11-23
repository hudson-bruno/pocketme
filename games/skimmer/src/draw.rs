//! The palette and all the painting. The whole picture is painted again
//! every call; the console sends only what changed (the table is the
//! same every call, so in play only the puck and the mallets cost).
//!
//! The sides differ in shape as well as colour: CORAL (left) mallets are
//! round with a round knob, TEAL (right) mallets have eight sides and a
//! square knob. The posts of each goal carry the colour of the side that
//! defends it. A person's mallet shows their pad number on the knob; a
//! computer's knob has a dark dot; a touch mallet's knob is plain.

use game_kit::{Frame, W, rgb565, text_width};

use crate::app::{
    App, Ctl, END_BOX, END_Y, LINE, PAUSE_BOX, PAUSE_LINES, PAUSE_Y, Scene, TITLE_LINES, TITLE_Y,
    Who,
};
use crate::fixed::{PX, isqrt};
use crate::table::{
    BUMPER_R, BUMPERS, CX, CY, GOAL_HALF, Length, Match, PUCK_MAX, Phase, SERVE_TICKS, SPARK_TICKS,
    STALL_TICKS, XL, XR, YB, YT,
};

// ---- Palette ------------------------------------------------------------

pub const CAB: u8 = 1;
pub const RAIL: u8 = 2;
pub const RAIL_DARK: u8 = 3;
pub const SURFACE: u8 = 4;
pub const HOLE: u8 = 5;
pub const MARK: u8 = 6;
pub const SLOT: u8 = 7;
pub const PUCK: u8 = 8;
pub const PUCK_EDGE: u8 = 9;
pub const CORAL: u8 = 10;
pub const CORAL_DARK: u8 = 11;
pub const CORAL_LIGHT: u8 = 12;
pub const TEAL: u8 = 13;
pub const TEAL_DARK: u8 = 14;
pub const TEAL_LIGHT: u8 = 15;
pub const INK: u8 = 16;
pub const TEXT: u8 = 17;
pub const TEXT_DIM: u8 = 18;
pub const FLASH: u8 = 19;
pub const PANEL: u8 = 20;
pub const PANEL_LIGHT: u8 = 21;
pub const SEL: u8 = 22;
pub const BAR: u8 = 23;
pub const TRAIL: u8 = 24;
pub const SPARK: u8 = 25;
pub const BUMPER: u8 = 26;
pub const BUMPER_DARK: u8 = 27;

/// Palette numbers from 0 on (0 is not used).
pub const COLOURS: [u16; 28] = [
    rgb565(0x00, 0x00, 0x00),
    rgb565(0x0E, 0x12, 0x1A), // CAB
    rgb565(0xD4, 0xDA, 0xE2), // RAIL
    rgb565(0x7C, 0x86, 0x94), // RAIL_DARK
    rgb565(0x18, 0x36, 0x58), // SURFACE
    rgb565(0x24, 0x4A, 0x72), // HOLE
    rgb565(0x4C, 0x7C, 0xB0), // MARK
    rgb565(0x04, 0x06, 0x0A), // SLOT
    rgb565(0xFF, 0xDC, 0x4C), // PUCK
    rgb565(0xB0, 0x82, 0x10), // PUCK_EDGE
    rgb565(0xF2, 0x6E, 0x4C), // CORAL
    rgb565(0x86, 0x2A, 0x18), // CORAL_DARK
    rgb565(0xFF, 0xC4, 0xB0), // CORAL_LIGHT
    rgb565(0x2C, 0xC4, 0xB0), // TEAL
    rgb565(0x0A, 0x5A, 0x52), // TEAL_DARK
    rgb565(0xB4, 0xF4, 0xEA), // TEAL_LIGHT
    rgb565(0x10, 0x14, 0x18), // INK
    rgb565(0xF2, 0xF4, 0xF8), // TEXT
    rgb565(0x8A, 0x98, 0xAA), // TEXT_DIM
    rgb565(0xFF, 0xF0, 0x70), // FLASH
    rgb565(0x22, 0x2E, 0x40), // PANEL
    rgb565(0x36, 0x48, 0x62), // PANEL_LIGHT
    rgb565(0xFF, 0xD0, 0x40), // SEL
    rgb565(0x12, 0x18, 0x22), // BAR
    rgb565(0x7A, 0x74, 0x3C), // TRAIL
    rgb565(0xFF, 0xFF, 0xFF), // SPARK
    rgb565(0xB8, 0x6C, 0xE0), // BUMPER
    rgb565(0x52, 0x26, 0x6C), // BUMPER_DARK
];

pub const SIDE: [&str; 2] = ["CORAL", "TEAL"];
const FILL: [u8; 2] = [CORAL, TEAL];
const EDGE: [u8; 2] = [CORAL_DARK, TEAL_DARK];
const KNOB: [u8; 2] = [CORAL_LIGHT, TEAL_LIGHT];

// ---- Text without a heap ---------------------------------------------------

/// A short line of text built in place.
pub struct Line {
    buf: [u8; 40],
    len: usize,
}

impl Line {
    pub fn new() -> Line {
        Line {
            buf: [0; 40],
            len: 0,
        }
    }

    pub fn push(&mut self, s: &str) -> &mut Line {
        for &b in s.as_bytes() {
            if self.len < self.buf.len() {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
        self
    }

    pub fn num(&mut self, mut n: u32, width: usize) -> &mut Line {
        let mut d = [0u8; 10];
        let mut k = 0;
        loop {
            d[k] = b'0' + (n % 10) as u8;
            n /= 10;
            k += 1;
            if n == 0 && k >= width {
                break;
            }
        }
        while k > 0 {
            k -= 1;
            let c = [d[k]];
            self.push(core::str::from_utf8(&c).unwrap_or("?"));
        }
        self
    }

    /// Ticks (1/60 s) as minutes and seconds: `2:05`.
    pub fn clock(&mut self, ticks: u32) -> &mut Line {
        let secs = ticks.div_ceil(60);
        self.num(secs / 60, 1).push(":").num(secs % 60, 2)
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl Default for Line {
    fn default() -> Self {
        Line::new()
    }
}

/// Writes `s` centred on `cx`.
fn centre(f: &mut Frame, cx: i16, y: i16, s: &str, c: u8, scale: u8) {
    let w = text_width(s) as i16 * i16::from(scale.max(1));
    f.text_scaled(cx - w / 2, y, s, c, scale);
}

/// Writes `s` ending at `x1`.
fn right(f: &mut Frame, x1: i16, y: i16, s: &str, c: u8) {
    f.text(x1 - text_width(s) as i16, y, s, c);
}

/// A position as its pixel.
fn pix(v: i32) -> i16 {
    (v.div_euclid(PX)) as i16
}

// ---- The picture ------------------------------------------------------------

pub fn draw(app: &App, f: &mut Frame) {
    match app.scene {
        Scene::Title => title(app, f),
        Scene::Play | Scene::End => {
            table(&app.m, f);
            pieces(app, f);
            hud(&app.m, f);
            notes(&app.m, f);
            if app.scene == Scene::End {
                result(app, f);
            } else if let Some(sel) = app.pause {
                pause(app, sel, f);
            }
        }
    }
}

/// A ring of radius `r` round the pixel corner `(cx, cy)` (the middle
/// of the table sits between pixels).
fn ring(f: &mut Frame, cx: i16, cy: i16, r: i32, c: u8, half: Option<i16>) {
    for o in 0..=r {
        let q = isqrt((r * r - o * o) as u64) as i16;
        let o = o as i16;
        for (x, y) in [(o, q), (q, o)] {
            for (sx, sy) in [(1, 1), (-1, 1), (1, -1), (-1, -1)] {
                let px = if sx > 0 { cx + x } else { cx - 1 - x };
                let py = if sy > 0 { cy + y } else { cy - 1 - y };
                // `half`: only the part on that side of x = cx.
                if half.is_none_or(|h| (h > 0) == (sx > 0)) {
                    f.pixel(px, py, c);
                }
            }
        }
    }
}

/// The table: cabinet, rails, the air holes, the markings, the slots.
pub fn table(m: &Match, f: &mut Frame) {
    f.clear(CAB);
    let (l, r, t, b) = (pix(XL), pix(XR), pix(YT), pix(YB));
    // Rails: two pixels, the outer one darker.
    f.rect(
        l - 2,
        t - 2,
        (r - l + 4) as u16,
        (b - t + 4) as u16,
        RAIL_DARK,
    );
    f.rect(l - 1, t - 1, (r - l + 2) as u16, (b - t + 2) as u16, RAIL);
    f.rect(l, t, (r - l) as u16, (b - t) as u16, SURFACE);
    // Air holes on a 6-pixel grid.
    let mut y = t + 3;
    while y < b - 1 {
        let mut x = l + 3;
        while x < r - 1 {
            f.pixel(x, y, HOLE);
            x += 6;
        }
        y += 6;
    }
    // The centre line, the centre circle, the two goal arcs.
    let (cx, cy) = (pix(CX), pix(CY));
    f.rect(cx - 1, t, 2, (b - t) as u16, MARK);
    ring(f, cx, cy, 15, MARK, None);
    ring(f, l, cy, 22, MARK, Some(1));
    ring(f, r, cy, 22, MARK, Some(-1));
    // The WILD bumpers.
    if m.rules.wild {
        for (bx, by) in BUMPERS {
            disc(f, pix(bx), pix(by), BUMPER_R / PX, BUMPER, BUMPER_DARK);
        }
    }
    // The slots: gaps in the end rails, the posts in the defenders'
    // colour; the slot just scored in flashes.
    let flash = match m.phase {
        Phase::Goal { side, t } if (t / 6) % 2 == 0 => Some(1 - usize::from(side)),
        _ => None,
    };
    let (g0, g1) = (pix(CY - GOAL_HALF), pix(CY + GOAL_HALF));
    for (side, post) in FILL.iter().enumerate() {
        let x = if side == 0 { l - 2 } else { r };
        let c = if flash == Some(side) { FLASH } else { SLOT };
        f.rect(x, g0, 2, (g1 - g0) as u16, c);
        f.rect(x, g0 - 2, 2, 2, *post);
        f.rect(x, g1, 2, 2, *post);
    }
}

/// A filled disc of radius `r` pixels round pixel `(x, y)`, with an edge.
fn disc(f: &mut Frame, x: i16, y: i16, r: i32, fill: u8, edge: u8) {
    let r2 = r * r + r; // (r + 1/2) squared, near enough
    let inner = (r - 1) * (r - 1) + (r - 1);
    for dy in -r..=r {
        for dx in -r..=r {
            let d = dx * dx + dy * dy;
            if d <= r2 {
                let c = if d > inner { edge } else { fill };
                f.pixel(x + dx as i16, y + dy as i16, c);
            }
        }
    }
}

/// A mallet: CORAL round, TEAL eight-sided; the knob in the middle
/// with the player's number (pads), a dot (computer) or plain (touch).
pub fn mallet(f: &mut Frame, x: i16, y: i16, side: usize, who: Option<Ctl>, dash: bool) {
    let inside = |dx: i16, dy: i16| -> bool {
        if side == 0 {
            dx * dx + dy * dy <= 42
        } else {
            dx.abs() <= 6 && dy.abs() <= 6 && dx.abs() + dy.abs() <= 9
        }
    };
    let edge_c = if dash { SPARK } else { EDGE[side] };
    for dy in -6i16..=6 {
        for dx in -6i16..=6 {
            if !inside(dx, dy) {
                continue;
            }
            let edge = !(inside(dx - 1, dy)
                && inside(dx + 1, dy)
                && inside(dx, dy - 1)
                && inside(dx, dy + 1));
            f.pixel(x + dx, y + dy, if edge { edge_c } else { FILL[side] });
        }
    }
    // The knob: round or square, 5 x 5.
    for dy in -2i16..=2 {
        for dx in -2i16..=2 {
            if side == 0 && dx.abs() == 2 && dy.abs() == 2 {
                continue;
            }
            f.pixel(x + dx, y + dy, KNOB[side]);
        }
    }
    match who {
        Some(Ctl::Pad(n)) => {
            let d = [b'1' + n];
            f.text(x - 1, y - 2, core::str::from_utf8(&d).unwrap_or("?"), INK);
        }
        Some(Ctl::Cpu) => f.rect(x, y, 1, 1, INK),
        _ => {}
    }
}

/// Mallets, puck, trail, sparks and the finger.
fn pieces(app: &App, f: &mut Frame) {
    let m = &app.m;
    if m.puck_shown() {
        let p = m.puck;
        let (px, py) = (pix(p.x), pix(p.y));
        // A short trail behind a fast puck: where it was one and two
        // ticks ago (the puck covers part of the nearer one).
        if m.phase == Phase::Live && p.speed() > PUCK_MAX / 2 {
            let (ax, ay) = m.trail[1];
            let (bx, by) = m.trail[2];
            f.rect(ax - 1, ay - 1, 3, 3, TRAIL);
            f.hline(bx - 1, by, 3, TRAIL);
            f.vline(bx, by - 1, 3, TRAIL);
        }
        // Waiting to be served: it blinks.
        let blink = match m.phase {
            Phase::Serve { t, .. } => t < SERVE_TICKS - 8 && (t / 5) % 2 == 0,
            _ => false,
        };
        if !blink {
            disc(f, px, py, 3, PUCK, PUCK_EDGE);
        }
    }
    for i in 0..m.mallets() {
        let mm = m.m[i];
        mallet(
            f,
            pix(mm.x),
            pix(mm.y),
            i % 2,
            Some(app.ctl[i]),
            mm.dash > 0,
        );
    }
    if let Some((x, y, t)) = m.spark {
        let d = 2 + i16::from(SPARK_TICKS - t) / 2;
        let c = if t > SPARK_TICKS / 2 { SPARK } else { FLASH };
        for (sx, sy) in [(1, 1), (-1, 1), (1, -1), (-1, -1)] {
            f.pixel(x + sx * d, y + sy * d, c);
            f.pixel(x + sx * (d + 1), y + sy * (d + 1), c);
        }
    }
    // The finger, when it steers a mallet and is not on it.
    if let Some(fg) = app.finger
        && fg.down
        && app.pause.is_none()
        && let Some(i) = app.ctl.iter().position(|&c| c == Ctl::Touch)
    {
        let mm = m.m[i];
        let far = (i32::from(fg.x) - mm.x / PX).abs() + (i32::from(fg.y) - mm.y / PX).abs() > 5;
        if far {
            let (x, y) = (fg.x, fg.y);
            let c = TEXT_DIM;
            f.hline(x - 1, y - 3, 3, c);
            f.hline(x - 1, y + 3, 3, c);
            f.vline(x - 3, y - 1, 3, c);
            f.vline(x + 3, y - 1, 3, c);
        }
    }
}

/// The bar on top: both scores, and the goal to reach or the clock.
pub fn hud(m: &Match, f: &mut Frame) {
    f.rect(0, 0, W as u16, 12, BAR);
    let scored = match m.phase {
        Phase::Goal { side, t } if (t / 6) % 2 == 0 => Some(usize::from(side)),
        _ => None,
    };
    for side in 0..2usize {
        let mut s = Line::new();
        s.num(u32::from(m.score[side]), 1);
        let w = text_width(s.as_str()) as i16 * 2;
        let c = if scored == Some(side) {
            FLASH
        } else if side == 0 {
            CORAL
        } else {
            TEAL
        };
        let x = if side == 0 { 13 } else { 147 - w };
        f.text_scaled(x, 1, s.as_str(), c, 2);
        badge(f, if side == 0 { 6 } else { 153 }, 6, side);
    }
    let mut c = Line::new();
    match m.rules.length.goals() {
        Some(_) if m.phase == Phase::Over => {
            c.push("FINAL");
        }
        Some(g) => {
            c.push("FIRST TO ").num(u32::from(g), 1);
        }
        None if m.phase == Phase::Over => {
            c.push("FINAL");
        }
        None if m.overtime => {
            c.push("GOLDEN ").clock(m.clock);
        }
        None => {
            c.clock(m.clock);
        }
    }
    let colour = if m.rules.length == Length::Timed {
        TEXT
    } else {
        TEXT_DIM
    };
    centre(f, 80, 4, c.as_str(), colour, 1);
    // The puck's last three seconds on one half: a dot a second in the
    // bar, on the side of the half it is on, counting down.
    if m.phase == Phase::Live && m.half_ticks > STALL_TICKS - 180 {
        let left = (STALL_TICKS - m.half_ticks).div_ceil(60) as i16;
        for k in 0..left {
            let x = if m.half == 0 { 32 + k * 4 } else { 126 - k * 4 };
            f.rect(x, 5, 2, 2, FLASH);
        }
    }
}

/// A small side badge: a 7-pixel mallet shape.
fn badge(f: &mut Frame, x: i16, y: i16, side: usize) {
    for dy in -3i16..=3 {
        for dx in -3i16..=3 {
            let inside = if side == 0 {
                dx * dx + dy * dy <= 10
            } else {
                dx.abs() + dy.abs() <= 4
            };
            if inside {
                let c = if dx.abs() <= 1 && dy.abs() <= 1 {
                    KNOB[side]
                } else {
                    FILL[side]
                };
                f.pixel(x + dx, y + dy, c);
            }
        }
    }
}

/// A note in a dark box near the top of the table.
fn note_box(f: &mut Frame, y: i16, s: &str, c: u8, scale: u8) {
    let w = text_width(s) as i16 * i16::from(scale);
    let h = 5 * i16::from(scale);
    f.rect(80 - w / 2 - 4, y - 3, (w + 8) as u16, (h + 6) as u16, PANEL);
    f.outline(
        80 - w / 2 - 4,
        y - 3,
        (w + 8) as u16,
        (h + 6) as u16,
        PANEL_LIGHT,
    );
    centre(f, 80, y, s, c, scale);
}

/// SERVE, GOAL!, TOO SLOW, GOLDEN GOAL.
fn notes(m: &Match, f: &mut Frame) {
    match m.phase {
        Phase::Serve { side, t } if t > SERVE_TICKS / 3 => {
            let mut s = Line::new();
            if m.overtime {
                s.push("GOLDEN GOAL");
            } else {
                s.push(SIDE[usize::from(side)]).push(" SERVES");
            }
            note_box(f, 22, s.as_str(), FILL[usize::from(side)], 1);
        }
        Phase::Goal { side, .. } => {
            note_box(f, 46, "GOAL!", FLASH, 3);
            let mut s = Line::new();
            s.push(SIDE[usize::from(side)]).push(" SCORES");
            note_box(f, 70, s.as_str(), FILL[usize::from(side)], 1);
        }
        Phase::Stall { to, .. } => {
            let mut s = Line::new();
            s.push("TOO SLOW: ")
                .push(SIDE[usize::from(to)])
                .push("'S PUCK");
            note_box(f, 22, s.as_str(), TEXT, 1);
        }
        _ => {}
    }
}

fn panel(f: &mut Frame, b: (i16, i16, u16, u16)) {
    f.rect(b.0, b.1, b.2, b.3, PANEL);
    f.outline(b.0, b.1, b.2, b.3, PANEL_LIGHT);
}

/// One menu line: a label, and a value with arrows when it has one.
fn menu_line(f: &mut Frame, x0: i16, x1: i16, y: i16, label: &str, value: Option<&str>, on: bool) {
    if on {
        f.rect(x0 - 3, y - 2, (x1 - x0 + 6) as u16, 9, PANEL_LIGHT);
        f.text(x0 - 2, y, ">", SEL);
    }
    let c = if on { SEL } else { TEXT };
    f.text(x0 + 4, y, label, c);
    if let Some(v) = value {
        let mut s = Line::new();
        s.push("< ").push(v).push(" >");
        right(f, x1, y, s.as_str(), if on { SEL } else { TEXT_DIM });
    }
}

fn pause(app: &App, sel: u8, f: &mut Frame) {
    panel(f, PAUSE_BOX);
    centre(f, 80, PAUSE_BOX.1 + 5, "PAUSED", TEXT, 1);
    let (x0, x1) = (PAUSE_BOX.0 + 6, PAUSE_BOX.0 + PAUSE_BOX.2 as i16 - 6);
    let lines: [(&str, Option<&str>); PAUSE_LINES as usize] = [
        ("RESUME", None),
        ("COMPUTER", Some(app.opts.level.name())),
        ("RATE", Some(if app.opts.fast { "60" } else { "30" })),
        ("RESTART", None),
        ("TITLE", None),
        ("LEAVE", None),
    ];
    for (k, (label, value)) in lines.iter().enumerate() {
        let y = PAUSE_Y + k as i16 * LINE;
        menu_line(f, x0, x1, y, label, *value, k == usize::from(sel));
    }
}

fn result(app: &App, f: &mut Frame) {
    let m = &app.m;
    panel(f, END_BOX);
    let (head, c) = match m.winner() {
        Some(0) => ("CORAL WINS", CORAL),
        Some(_) => ("TEAL WINS", TEAL),
        None => ("DRAW", TEXT),
    };
    if let Some(w) = m.winner() {
        let half = text_width(head) as i16 + 6;
        badge(f, 80 - half, END_BOX.1 + 10, w);
        badge(f, 80 + half, END_BOX.1 + 10, w);
    }
    centre(f, 80, END_BOX.1 + 6, head, c, 2);
    let mut s = Line::new();
    s.num(u32::from(m.score[0]), 1)
        .push(" - ")
        .num(u32::from(m.score[1]), 1);
    centre(f, 80, END_BOX.1 + 22, s.as_str(), TEXT, 2);
    let mut s = Line::new();
    s.push("SERIES ")
        .num(u32::from(app.series[0]), 1)
        .push(" - ")
        .num(u32::from(app.series[1]), 1);
    centre(f, 80, END_BOX.1 + 38, s.as_str(), TEXT_DIM, 1);
    let (x0, x1) = (END_BOX.0 + 24, END_BOX.0 + END_BOX.2 as i16 - 24);
    menu_line(f, x0, x1, END_Y, "REMATCH", None, app.end_sel == 0);
    menu_line(f, x0, x1, END_Y + LINE, "TITLE", None, app.end_sel == 1);
}

fn title(app: &App, f: &mut Frame) {
    f.clear(PANEL);
    f.rect(0, 0, W as u16, 27, BAR);
    // The name with a teal shadow, between a mallet of each side.
    centre(f, 81, 4, "SKIMMER", TEAL_DARK, 3);
    centre(f, 80, 3, "SKIMMER", TEXT, 3);
    mallet(f, 12, 10, 0, None, false);
    mallet(f, 147, 10, 1, None, false);
    disc(f, 24, 16, 3, PUCK, PUCK_EDGE);
    centre(f, 80, 20, "AIR HOCKEY FOR 1 TO 4", TEXT_DIM, 1);
    f.hline(0, 27, W as u16, TEAL_DARK);

    let o = &app.opts;
    // AUTO says what it means with the pads there now.
    let now = app.drivers(app.pads_seen);
    let mut sides = [Line::new(), Line::new()];
    for (s, line) in sides.iter_mut().enumerate() {
        if o.side[s] == Who::Auto {
            line.push("AUTO: ").push(now[s].name());
        } else {
            line.push(o.side[s].name());
        }
    }
    let lines: [(&str, Option<&str>); TITLE_LINES as usize] = [
        ("PLAY", None),
        ("CORAL", Some(sides[0].as_str())),
        ("TEAL", Some(sides[1].as_str())),
        ("TEAMS", Some(if o.four { "2 VS 2" } else { "1 VS 1" })),
        ("COMPUTER", Some(o.level.name())),
        ("MATCH", Some(o.length.name())),
        ("TABLE", Some(if o.wild { "WILD" } else { "PLAIN" })),
        ("RATE", Some(if o.fast { "60 FPS" } else { "30 FPS" })),
        ("LEAVE", None),
    ];
    for (k, (label, value)) in lines.iter().enumerate() {
        let y = TITLE_Y + k as i16 * LINE;
        menu_line(f, 12, 148, y, label, *value, k == usize::from(app.sel));
    }
    badge(f, 9, TITLE_Y + LINE + 2, 0);
    badge(f, 9, TITLE_Y + 2 * LINE + 2, 1);
    f.hline(0, 112, W as u16, PANEL_LIGHT);
    centre(f, 80, 114, "PAD: STICK, A DASH   TOUCH: DRAG", TEXT_DIM, 1);
}
