//! The palette and all the painting. The whole picture is painted again
//! every call; the console sends only what changed.
//!
//! The teams differ in shape as well as colour: AMBER skaters are round
//! with round-cornered goalies, BLUE skaters are square with square
//! goalies. A person's skater carries their player number; the
//! computer's carry a small cross.

use game_kit::{Frame, W, rgb565, text_width};

use crate::app::{
    App, Ctl, END_BOX, END_Y, LINE, PAUSE_BOX, PAUSE_LINES, PAUSE_Y, Right, Scene, TITLE_LINES,
    TITLE_Y,
};
use crate::fixed::{OCTANT_STEP, PX, isqrt, octant};
use crate::play::{
    BOARD_B, BOARD_L, BOARD_R, BOARD_T, CHARGE_MAX, CY, Match, Owner, Phase, TAP_TICKS, goalie_x,
};

// ---- Palette ------------------------------------------------------------

pub const ICE: u8 = 1;
pub const MARK: u8 = 2;
pub const LINE_RED: u8 = 3;
pub const LINE_BLUE: u8 = 4;
pub const CREASE: u8 = 5;
pub const BOARD: u8 = 6;
pub const STANDS: u8 = 7;
pub const NET: u8 = 8;
pub const POST: u8 = 9;
pub const BAR: u8 = 10;
pub const TEXT: u8 = 11;
pub const TEXT_DIM: u8 = 12;
pub const AMBER: u8 = 13;
pub const AMBER_DARK: u8 = 14;
pub const BLUE: u8 = 15;
pub const BLUE_DARK: u8 = 16;
pub const INK: u8 = 17;
pub const JERSEY: u8 = 18;
pub const FLASH: u8 = 19;
pub const PANEL: u8 = 20;
pub const PANEL_LIGHT: u8 = 21;
pub const SEL: u8 = 22;
pub const CHARGE: u8 = 23;
pub const STICK: u8 = 24;
pub const NET_DARK: u8 = 25;

/// Palette numbers from 0 on (0 is not used).
pub const COLOURS: [u16; 26] = [
    rgb565(0x00, 0x00, 0x00),
    rgb565(0xE6, 0xEF, 0xF4), // ICE
    rgb565(0xC4, 0xD4, 0xDE), // MARK
    rgb565(0xE8, 0x86, 0x86), // LINE_RED
    rgb565(0x86, 0xA8, 0xE0), // LINE_BLUE
    rgb565(0xBE, 0xD8, 0xF2), // CREASE
    rgb565(0xFA, 0xFA, 0xFA), // BOARD
    rgb565(0x2A, 0x33, 0x40), // STANDS
    rgb565(0xA8, 0xB0, 0xB8), // NET
    rgb565(0xD8, 0x30, 0x30), // POST
    rgb565(0x14, 0x1B, 0x26), // BAR
    rgb565(0xF2, 0xF4, 0xF8), // TEXT
    rgb565(0x8A, 0x98, 0xAA), // TEXT_DIM
    rgb565(0xF0, 0x90, 0x28), // AMBER
    rgb565(0x7A, 0x40, 0x08), // AMBER_DARK
    rgb565(0x2F, 0x5F, 0xD0), // BLUE
    rgb565(0x10, 0x22, 0x60), // BLUE_DARK
    rgb565(0x12, 0x14, 0x18), // INK
    rgb565(0xFF, 0xFF, 0xFF), // JERSEY
    rgb565(0xFF, 0xD8, 0x40), // FLASH
    rgb565(0x22, 0x2E, 0x40), // PANEL
    rgb565(0x36, 0x48, 0x62), // PANEL_LIGHT
    rgb565(0xFF, 0xD0, 0x40), // SEL
    rgb565(0x40, 0xD0, 0x60), // CHARGE
    rgb565(0x5A, 0x3A, 0x1C), // STICK
    rgb565(0x1A, 0x20, 0x2A), // NET_DARK
];

pub const TEAM: [&str; 2] = ["AMBER", "BLUE"];
const FILL: [u8; 2] = [AMBER, BLUE];
const EDGE: [u8; 2] = [AMBER_DARK, BLUE_DARK];

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

/// Writes `s` ending at `right`.
fn right(f: &mut Frame, x1: i16, y: i16, s: &str, c: u8) {
    f.text(x1 - text_width(s) as i16, y, s, c);
}

// ---- The picture ------------------------------------------------------------

pub fn draw(app: &App, f: &mut Frame) {
    match app.scene {
        Scene::Title => title(app, f),
        Scene::Play | Scene::End => {
            rink(&app.m, f);
            players(app, f);
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

/// The ice, its lines, the boards, the nets and the skate marks.
pub fn rink(m: &Match, f: &mut Frame) {
    f.clear(STANDS);
    let (l, r, t, b) = (
        BOARD_L as i16,
        BOARD_R as i16,
        BOARD_T as i16,
        BOARD_B as i16,
    );
    f.rect(l + 1, t + 1, (r - l - 1) as u16, (b - t - 1) as u16, ICE);
    // The goal creases: half circles in front of each goal.
    let cy2 = 2 * CY / PX; // twice the middle row, in pixel halves
    for y in t + 1..b {
        for (gx2, xs) in [(2 * (l + 1), l + 1..l + 10), (2 * r, r - 9..r)] {
            for x in xs {
                let dx = i32::from(2 * x + 1 - gx2);
                let dy = i32::from(2 * y + 1) - cy2;
                if dx * dx + dy * dy <= 16 * 16 {
                    f.pixel(x, y, CREASE);
                }
            }
        }
    }
    // The centre line and the two blue lines.
    f.rect(79, t + 1, 2, (b - t - 1) as u16, LINE_RED);
    f.rect(51, t + 1, 2, (b - t - 1) as u16, LINE_BLUE);
    f.rect(107, t + 1, 2, (b - t - 1) as u16, LINE_BLUE);
    // The centre circle (radius 14, round the middle of the ice).
    let (cx, cyp) = (80i16, (CY / PX) as i16 + 1); // pixel right of / below the middle
    let ring = 14i32;
    for o in 0..=ring {
        let q = isqrt((ring * ring - o * o) as u64) as i16;
        let o = o as i16;
        for (x, y) in [(o, q), (q, o)] {
            f.pixel(cx + x, cyp + y, LINE_BLUE);
            f.pixel(cx - 1 - x, cyp + y, LINE_BLUE);
            f.pixel(cx + x, cyp - 1 - y, LINE_BLUE);
            f.pixel(cx - 1 - x, cyp - 1 - y, LINE_BLUE);
        }
    }
    // Face-off dots in each end.
    for x in [26, 132] {
        for y in [cyp - 22, cyp + 20] {
            f.rect(x, y, 2, 2, LINE_RED);
        }
    }
    f.rect(79, cyp - 1, 2, 2, LINE_BLUE);
    // Skate marks.
    for (x, y) in m.marks.iter() {
        f.pixel(i16::from(x), i16::from(y), MARK);
    }
    // The boards, with cut corners.
    f.hline(l, t, (r - l + 1) as u16, BOARD);
    f.hline(l, b, (r - l + 1) as u16, BOARD);
    f.vline(l, t, (b - t + 1) as u16, BOARD);
    f.vline(r, t, (b - t + 1) as u16, BOARD);
    for (sx, sy, x0, y0) in [
        (1, 1, l + 1, t + 1),
        (-1, 1, r - 1, t + 1),
        (1, -1, l + 1, b - 1),
        (-1, -1, r - 1, b - 1),
    ] {
        for a in -1i16..9 {
            for c in -1i16..9 {
                let (x, y) = (x0 + sx * a, y0 + sy * c);
                if a + c < 7 {
                    f.pixel(x, y, STANDS);
                } else if a + c == 7 {
                    f.pixel(x, y, BOARD);
                }
            }
        }
    }
    // The nets: openings in the end boards with a mesh behind.
    let flash = match m.phase {
        Phase::Goal { team, t } if (t / 6) % 2 == 0 => Some(1 - usize::from(team)),
        _ => None,
    };
    // The mouth is rows 56 to 75, round the middle of the ice (65.5).
    let top = (CY / PX) as i16 - 9;
    for team in 0..2usize {
        let (line, back) = if team == 0 { (l, l - 5) } else { (r, r + 1) };
        let mesh = if flash == Some(team) { FLASH } else { NET };
        for y in top..top + 20 {
            for x in back..back + 5 {
                let c = if (x + y) % 2 == 0 { mesh } else { NET_DARK };
                f.pixel(x, y, c);
            }
        }
        let from = back.min(line);
        f.hline(from, top - 1, 6, mesh);
        f.hline(from, top + 20, 6, mesh);
        let outer = if team == 0 { back } else { back + 4 };
        f.vline(outer, top, 20, mesh);
        // The goal line across the mouth, and the posts.
        f.vline(line, top, 20, LINE_RED);
        f.pixel(line, top - 1, POST);
        f.pixel(line, top + 20, POST);
    }
}

/// Goalies, skaters, the puck, the shot meter and the finger.
fn players(app: &App, f: &mut Frame) {
    let m = &app.m;
    for g in 0..2 {
        let (x, y) = ((goalie_x(g) / PX) as i16, (m.gk[g].y / PX) as i16);
        goalie(f, x, y, g);
    }
    for (i, s) in m.sk.iter().enumerate() {
        let (x, y) = ((s.x / PX) as i16, (s.y / PX) as i16);
        let team = i / 2;
        // The stick first, the body over its root.
        let (ox, oy) = OCTANT_STEP[octant(s.fx, s.fy)];
        f.pixel(x + ox * 4, y + oy * 4, STICK);
        f.pixel(x + ox * 5, y + oy * 5, STICK);
        let stunned = s.stun > 0 && (m.age / 4).is_multiple_of(2);
        body(f, x, y, team, if stunned { None } else { Some(app.ctl[i]) });
        if m.carrier() == Some(i) && s.charge >= TAP_TICKS {
            let w = 1 + 6 * u16::from(s.charge - TAP_TICKS) / u16::from(CHARGE_MAX - TAP_TICKS);
            f.hline(x - 3, y + 5, w, CHARGE);
        }
    }
    let p = m.puck;
    let (px, py) = ((p.x / PX) as i16, (p.y / PX) as i16);
    if !matches!(p.owner, Owner::Goalie(_)) || (m.age / 8).is_multiple_of(2) {
        f.hline(px - 1, py, 3, INK);
        f.pixel(px, py - 1, INK);
        f.pixel(px, py + 1, INK);
    }
    if let Some(fg) = app.finger
        && fg.down
        && app.ctl.contains(&Ctl::Touch)
        && app.pause.is_none()
    {
        let (x, y) = (fg.x, fg.y);
        f.hline(x - 1, y - 3, 3, BLUE_DARK);
        f.hline(x - 1, y + 3, 3, BLUE_DARK);
        f.vline(x - 3, y - 1, 3, BLUE_DARK);
        f.vline(x + 3, y - 1, 3, BLUE_DARK);
        f.pixel(x - 2, y - 2, BLUE_DARK);
        f.pixel(x + 2, y - 2, BLUE_DARK);
        f.pixel(x - 2, y + 2, BLUE_DARK);
        f.pixel(x + 2, y + 2, BLUE_DARK);
    }
}

/// Whether `(dx, dy)` (each -3 to 3) is inside a team's 7 x 7 body.
fn inside(team: usize, dx: i16, dy: i16) -> bool {
    if dx.abs() > 3 || dy.abs() > 3 {
        return false;
    }
    team == 1 || dx.abs() + dy.abs() <= 4
}

/// A skater's body: AMBER round, BLUE square, a dark edge; inside, the
/// player number of a person (`Some(Pad/Touch)`), a cross for the
/// computer (`Some(Cpu)`), nothing while stunned (`None`).
pub fn body(f: &mut Frame, x: i16, y: i16, team: usize, who: Option<Ctl>) {
    for dy in -3..=3 {
        for dx in -3..=3 {
            if !inside(team, dx, dy) {
                continue;
            }
            let edge = !(inside(team, dx - 1, dy)
                && inside(team, dx + 1, dy)
                && inside(team, dx, dy - 1)
                && inside(team, dx, dy + 1));
            f.pixel(x + dx, y + dy, if edge { EDGE[team] } else { FILL[team] });
        }
    }
    match who.map(Ctl::player) {
        Some(Some(n)) => {
            let d = [b'0' + n];
            f.text(
                x - 1,
                y - 2,
                core::str::from_utf8(&d).unwrap_or("?"),
                JERSEY,
            );
        }
        Some(None) => {
            f.hline(x - 1, y, 3, EDGE[team]);
            f.vline(x, y - 1, 3, EDGE[team]);
        }
        None => {}
    }
}

/// A goalie: 5 x 13, round corners for AMBER, square for BLUE, a white
/// mask in the middle.
fn goalie(f: &mut Frame, x: i16, y: i16, team: usize) {
    for dy in -6i16..=6 {
        for dx in -2i16..=2 {
            if team == 0 && dx.abs() == 2 && dy.abs() == 6 {
                continue; // AMBER's corners are cut
            }
            let edge = dx.abs() == 2 || dy.abs() == 6;
            f.pixel(x + dx, y + dy, if edge { EDGE[team] } else { FILL[team] });
        }
    }
    f.rect(x - 1, y - 1, 3, 3, JERSEY);
    f.pixel(x, y, EDGE[team]);
}

/// The bar on top: both scores and the clock.
pub fn hud(m: &Match, f: &mut Frame) {
    f.rect(0, 0, W as u16, 12, BAR);
    body(f, 5, 6, 0, None);
    body(f, 154, 6, 1, None);
    let mut s = Line::new();
    s.num(u32::from(m.score[0]), 1);
    f.text_scaled(11, 1, s.as_str(), AMBER, 2);
    let mut s = Line::new();
    s.num(u32::from(m.score[1]), 1);
    let w = text_width(s.as_str()) as i16 * 2;
    f.text_scaled(149 - w, 1, s.as_str(), BLUE_LIGHT_TEXT, 2);
    let mut c = Line::new();
    match m.phase {
        Phase::Whistle(_) | Phase::Over => {
            c.push("FINAL");
        }
        _ if m.overtime => {
            // What is left of the overtime, counting down from 3:00.
            c.push("OT ").clock(m.clock());
        }
        _ => {
            c.clock(m.clock());
        }
    }
    centre(f, 80, 4, c.as_str(), TEXT, 1);
}

/// BLUE's score on the dark bar: the team blue is too dark to read
/// there, so a lighter blue.
const BLUE_LIGHT_TEXT: u8 = LINE_BLUE;

/// A note in a dark box near the top of the ice.
fn note_box(f: &mut Frame, y: i16, s: &str, c: u8, scale: u8) {
    let w = text_width(s) as i16 * i16::from(scale);
    let h = 5 * i16::from(scale);
    f.rect(80 - w / 2 - 4, y - 3, (w + 8) as u16, (h + 6) as u16, PANEL);
    centre(f, 80, y, s, c, scale);
}

/// FACE-OFF, GOAL!, OVERTIME, FINAL.
fn notes(m: &Match, f: &mut Frame) {
    match m.phase {
        Phase::FaceOff(_) => note_box(f, 22, "FACE-OFF", TEXT, 1),
        Phase::Goal { team, .. } => {
            note_box(f, 20, "GOAL!", FLASH, 2);
            let mut s = Line::new();
            s.push(TEAM[usize::from(team)]).push(" SCORES");
            note_box(f, 37, s.as_str(), FILL[usize::from(team)], 1);
        }
        Phase::Whistle(_) => note_box(f, 22, "FINAL WHISTLE", TEXT, 1),
        _ if m.ot_note > 0 => note_box(f, 22, "OVERTIME: NEXT GOAL WINS", FLASH, 1),
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
    // A draw (overtime ran out level): no badges, the word in white.
    let (head, c) = match m.winner() {
        Some(w) => (if w == 0 { "AMBER WINS" } else { "BLUE WINS" }, FILL[w]),
        None => ("DRAW", TEXT),
    };
    if let Some(w) = m.winner() {
        // The two badges just outside the words (at 2x, "AMBER WINS" is
        // 95 pixels wide: fixed badges would cover its first and last
        // letter).
        let half = text_width(head) as i16 + 5;
        body(f, 80 - half, END_BOX.1 + 10, w, None);
        body(f, 80 + half, END_BOX.1 + 10, w, None);
    }
    centre(
        f,
        80,
        END_BOX.1 + 6,
        head,
        if c == BLUE { LINE_BLUE } else { c },
        2,
    );
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
    if m.overtime {
        s.push("  (OT)");
    }
    centre(f, 80, END_BOX.1 + 38, s.as_str(), TEXT_DIM, 1);
    let (x0, x1) = (END_BOX.0 + 24, END_BOX.0 + END_BOX.2 as i16 - 24);
    menu_line(f, x0, x1, END_Y, "REMATCH", None, app.end_sel == 0);
    menu_line(f, x0, x1, END_Y + LINE, "TITLE", None, app.end_sel == 1);
}

/// The skater names on the team sheet: "P1", "CPU", "P2 TOUCH"...
fn who_name(app: &App, player: usize, pad: bool) -> &'static str {
    const PADS: [&str; 4] = ["P1", "P2", "P3", "P4"];
    if player == 1 {
        return match app.opts.right {
            Right::Computer => "CPU",
            Right::Touch => "P2 TOUCH",
            Right::Pad if pad => "P2",
            Right::Pad => "CPU",
            Right::Auto if pad => "P2",
            Right::Auto => "CPU/TOUCH",
        };
    }
    if pad { PADS[player] } else { "CPU" }
}

fn title(app: &App, f: &mut Frame) {
    f.clear(PANEL);
    f.rect(0, 0, W as u16, 29, BAR);
    // The name, with a blue shadow, between the two team badges.
    centre(f, 81, 5, "TUNDRA", BLUE, 3);
    centre(f, 80, 4, "TUNDRA", TEXT, 3);
    body(f, 18, 11, 0, None);
    body(f, 141, 11, 1, None);
    centre(f, 80, 22, "ICE HOCKEY FOR 1 TO 4", TEXT_DIM, 1);
    f.hline(0, 29, W as u16, LINE_BLUE);

    let o = &app.opts;
    let fast = if o.fast { "60 FPS" } else { "30 FPS" };
    let marks = if o.marks { "ON" } else { "OFF" };
    let lines: [(&str, Option<&str>); TITLE_LINES as usize] = [
        ("PLAY", None),
        ("PLAYER 2", Some(o.right.name())),
        ("COMPUTER", Some(o.level.name())),
        ("MATCH", Some(o.length.name())),
        ("RATE", Some(fast)),
        ("ICE MARKS", Some(marks)),
        ("LEAVE", None),
    ];
    for (k, (label, value)) in lines.iter().enumerate() {
        let y = TITLE_Y + k as i16 * LINE;
        menu_line(f, 14, 146, y, label, *value, k == usize::from(app.sel));
    }

    // The team sheet: who plays each skater, as the match would start.
    let pads = app.pads_seen;
    f.hline(0, 98, W as u16, PANEL_LIGHT);
    for team in 0..2usize {
        let y = 102 + team as i16 * 9;
        body(f, 8, y + 2, team, None);
        f.text(15, y, TEAM[team], [AMBER, LINE_BLUE][team]);
        let a = who_name(app, team, pads[team]);
        let b = who_name(app, team + 2, pads[team + 2]);
        f.text(48, y, a, TEXT);
        f.text(104, y, b, TEXT);
    }
}
