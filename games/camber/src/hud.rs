//! Everything drawn over the 3D picture: the race's few numbers, the
//! count, the notes, the title, the pause menu and the results.

use game_kit::{Frame, W, text_width};

use crate::app::{
    App, COL_W, END_BOX, END_LINES, END_Y, LINE, Note, PAUSE_BOX, PAUSE_LINES, PAUSE_Y, Page,
    RESULTS_AFTER, ROW, ROWS_Y, Scene, count_shown, place_colour,
};
use crate::art::{
    ACCENT, BOOST_UI, ENTRANTS, GOLD, INK, PANEL, PANEL_LIGHT, SEL, SHIELD, SHIELD_LOW, SPARK,
    TEXT, TEXT_DIM,
};
use crate::race::{HEAT_1, HEAT_2, HEAT_3, LAPS, Mode, Phase, Race, SHIELD_MAX, heat_step};
use crate::scene::ViewKind;
use crate::track::{FM, TRACKS};

// ---- Text without a heap ---------------------------------------------------------

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
        while k > 0 && self.len < self.buf.len() {
            k -= 1;
            self.buf[self.len] = d[k];
            self.len += 1;
        }
        self
    }

    /// Ticks (1/60 s) as `1:02.3`.
    pub fn time(&mut self, ticks: u32) -> &mut Line {
        let tenths = ticks / 6;
        self.num(tenths / 600, 1)
            .push(":")
            .num(tenths / 10 % 60, 2)
            .push(".")
            .num(tenths % 10, 1)
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

/// `s` centred on `cx`.
fn centre(f: &mut Frame, cx: i16, y: i16, s: &str, c: u8, scale: u8) {
    let w = text_width(s) as i16 * i16::from(scale.max(1));
    f.text_scaled(cx - w / 2, y, s, c, scale);
}

/// `s` ending at `x1`.
fn right(f: &mut Frame, x1: i16, y: i16, s: &str, c: u8, scale: u8) {
    let w = text_width(s) as i16 * i16::from(scale.max(1));
    f.text_scaled(x1 - w, y, s, c, scale);
}

/// Text with a dark outline all round, so it reads over sky, snow, lamp
/// posts and wall stripes alike (cheaper in code than backing bars, and
/// it hides less of the picture).
fn inked(f: &mut Frame, x: i16, y: i16, s: &str, c: u8, scale: u8) {
    // The eight neighbours; the middle one is covered by the text itself.
    for d in (0..9).filter(|&d| d != 4) {
        f.text_scaled(x + d % 3 - 1, y + d / 3 - 1, s, INK, scale);
    }
    f.text_scaled(x, y, s, c, scale);
}

fn inked_centre(f: &mut Frame, cx: i16, y: i16, s: &str, c: u8, scale: u8) {
    let w = text_width(s) as i16 * i16::from(scale.max(1));
    inked(f, cx - w / 2, y, s, c, scale);
}

fn inked_right(f: &mut Frame, x1: i16, y: i16, s: &str, c: u8, scale: u8) {
    let w = text_width(s) as i16 * i16::from(scale.max(1));
    inked(f, x1 - w, y, s, c, scale);
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
        right(f, x1, y, s.as_str(), if on { SEL } else { TEXT_DIM }, 1);
    }
}

fn ordinal(n: u8) -> &'static str {
    match n {
        1 => "ST",
        2 => "ND",
        3 => "RD",
        _ => "TH",
    }
}

// ---- The screens --------------------------------------------------------------------

pub fn draw(app: &App, f: &mut Frame) {
    match app.scene {
        Scene::Title => title(app, f),
        Scene::Race => {
            if app.results() {
                results(app, f);
            } else {
                hud(app, f);
                if let Some(sel) = app.pause {
                    pause(app, sel, f);
                }
            }
        }
    }
}

/// The title: a column on the left (the name, the menu's page, the
/// session's best lap) over the backdrop. The column is the same from
/// picture to picture except for the small cue, so only the backdrop
/// right of it is sent.
fn title(app: &App, f: &mut Frame) {
    let w = COL_W as u16;
    f.rect(0, 0, w - 1, 120, PANEL);
    f.vline(COL_W - 1, 0, 120, PANEL_LIGHT);
    // The name, twice the size, with a shadow; an accent stripe under it.
    f.text_scaled(7, 5, crate::NAME, INK, 2);
    f.text_scaled(6, 4, crate::NAME, TEXT, 2);
    f.rect(6, 16, 60, 2, ACCENT);
    f.rect(6, 18, 60, 1, INK);
    let options = app.page == Page::Options;
    let head = if options { "OPTIONS" } else { "HOVER RACING" };
    f.text(6, 21, head, if options { ACCENT } else { TEXT_DIM });

    let o = &app.opts;
    let tr = &TRACKS[usize::from(o.track) % TRACKS.len()];
    let main: [(&str, Option<&str>); 6] = [
        ("PLAY", None),
        ("TRACK", Some(tr.name)),
        ("CLASS", Some(o.class.name())),
        ("MODE", Some(o.mode.name())),
        ("OPTIONS", None),
        ("LEAVE", None),
    ];
    let opts: [(&str, Option<&str>); 6] = [
        ("DETAIL", Some(o.detail.name())),
        ("RATE", Some(if o.fast { "60 FPS" } else { "30 FPS" })),
        ("VIEW", Some(o.view.name())),
        ("CATCH-UP", Some(o.catchup.name())),
        ("BACKDROP", Some(o.backdrop.name())),
        ("BACK", None),
    ];
    let lines: &[(&str, Option<&str>)] = if options { &opts } else { &main };
    // The cue slides out and back, a pixel every four pictures.
    let cue = [0, 1, 2, 1][(app.frames / 4 % 4) as usize];
    for (k, (label, value)) in lines.iter().enumerate() {
        let y = ROWS_Y + k as i16 * ROW;
        let on = k == usize::from(app.sel);
        if on {
            f.rect(1, y, w - 3, ROW as u16 - 1, PANEL_LIGHT);
            f.rect(1, y, 2, ROW as u16 - 1, SEL);
            f.text(
                4 + cue,
                y + 1 + if value.is_some() { 0 } else { 3 },
                ">",
                SEL,
            );
        }
        let c = if on { SEL } else { TEXT };
        match value {
            Some(v) => {
                f.text(10, y + 1, label, c);
                let mut s = Line::new();
                if on {
                    s.push("< ").push(v).push(" >");
                } else {
                    s.push(v);
                }
                right(
                    f,
                    COL_W - 5,
                    y + 7,
                    s.as_str(),
                    if on { SEL } else { TEXT_DIM },
                    1,
                );
            }
            None => {
                f.text(10, y + 4, label, c);
            }
        }
    }
    let best = app.session_best(o.track, o.class);
    let mut s = Line::new();
    s.push("BEST ");
    if best > 0 {
        s.time(best);
    } else {
        s.push("-:--.-");
    }
    f.rect(6, 109, 60, 1, PANEL_LIGHT);
    f.text(6, 112, s.as_str(), if best > 0 { GOLD } else { TEXT_DIM });
}

/// The carve's heat, low in the middle: three marks that light up in the
/// step's colour, the next one filling as the heat grows; during a
/// sling, the step it pays. Nothing when neither.
fn carve_marks(f: &mut Frame, sh: &crate::race::Ship, tick: u32, nose: bool) {
    if sh.carve == 0 && sh.sling == 0 {
        return;
    }
    let (heat, step) = if sh.carve != 0 {
        (sh.heat, heat_step(sh.heat))
    } else {
        (0, sh.sling_step)
    };
    let lit = SPARK[usize::from(step.min(3))];
    // The top step blinks (white and dark), so 2 and 3 differ at a glance.
    let lit = if step >= 3 && (tick >> 3) & 1 != 0 {
        PANEL_LIGHT
    } else {
        lit
    };
    let steps = [0, HEAT_1, HEAT_2, HEAT_3];
    f.rect(65, 111, 30, 6, INK);
    for k in 0..3usize {
        let x = 66 + 10 * k as i16;
        f.rect(x, 112, 8, 4, PANEL_LIGHT);
        if usize::from(step) > k {
            f.rect(x, 112, 8, 4, lit);
        } else if sh.carve != 0 && usize::from(step) == k {
            let part = (heat - steps[k]) * 8 / (steps[k + 1] - steps[k]);
            if part > 0 {
                f.rect(x, 112, part.min(8) as u16, 4, TEXT_DIM);
            }
        }
    }
    // From the nose the ship is not in the picture: two small sparks rise
    // from the bottom edge beside the marks instead, 3, 6 and 10 pixels
    // high by the step (a pixel of dust below the first), flickering.
    if nose && sh.carve != 0 {
        let c = SPARK[usize::from(step.min(3))];
        let tall = [1i16, 3, 6, 10][usize::from(step.min(3))] + (tick & 1) as i16;
        for (x0, way) in [(60i16, -1i16), (99, 1)] {
            for k in 0..tall {
                f.rect(x0 + way * (k / 3), 119 - k, 2, 1, c);
            }
        }
    }
}

/// The race's numbers: place, lap and time, shield, speed.
fn hud(app: &App, f: &mut Frame) {
    let race = &app.race;
    let sh = &race.ships[0];
    // Place, big, top left (a race only).
    if race.mode == Mode::Race {
        let place = race.place(0);
        let mut s = Line::new();
        s.num(u32::from(place), 1);
        inked(f, 3, 3, s.as_str(), place_colour(place), 3);
        let mut s = Line::new();
        s.push(ordinal(place)).push("/").num(u32::from(race.n), 1);
        inked(
            f,
            4 + text_width(Line::new().num(u32::from(place), 1).as_str()) as i16 * 3,
            3,
            s.as_str(),
            TEXT,
            1,
        );
    }
    // Lap and time, top right.
    let lap = sh.lap.clamp(1, LAPS) as u32;
    let mut s = Line::new();
    if sh.finished() {
        s.push("FINISHED");
    } else if sh.lap == LAPS {
        s.push("FINAL LAP");
    } else {
        s.push("LAP ").num(lap, 1).push("/").num(LAPS as u32, 1);
    }
    inked_right(f, W as i16 - 3, 3, s.as_str(), TEXT, 1);
    let now = if sh.finished() {
        sh.finish
    } else {
        race.clock()
    };
    let mut s = Line::new();
    s.time(now);
    inked_right(f, W as i16 - 3, 10, s.as_str(), TEXT, 1);
    if sh.best > 0 {
        let mut s = Line::new();
        s.push("BEST ").time(sh.best);
        inked_right(f, W as i16 - 3, 17, s.as_str(), TEXT_DIM, 1);
    }
    // Shield, bottom left.
    let full = 44i32;
    let w = i32::from(sh.shield.max(0)) * full / i32::from(SHIELD_MAX);
    let low = sh.limp || sh.shield < 300;
    f.rect(3, 111, (full + 2) as u16, 6, INK);
    if w > 0 {
        f.rect(4, 112, w as u16, 4, if low { SHIELD_LOW } else { SHIELD });
    }
    if sh.limp && app.frames & 8 != 0 {
        inked(f, 3, 103, "LIMP", SHIELD_LOW, 1);
    } else {
        inked(f, 3, 103, "SHIELD", TEXT_DIM, 1);
    }
    // Speed, bottom right (km/h).
    let (vs, _) = sh.v_track();
    let kmh = (i64::from(vs.max(0)) * 60 * 36 / (i64::from(FM) * 10)) as u32;
    let mut s = Line::new();
    s.num(kmh, 1);
    let c = if sh.boost > 0 || sh.sling > 0 {
        BOOST_UI
    } else {
        TEXT
    };
    inked_right(f, W as i16 - 22, 105, s.as_str(), c, 2);
    inked_right(f, W as i16 - 3, 110, "KMH", TEXT_DIM, 1);
    carve_marks(f, sh, race.tick, app.opts.view == ViewKind::Nose);

    // The count, the notes.
    if let Some(n) = count_shown(race) {
        let mut s = Line::new();
        if n == 0 {
            s.push("GO!");
        } else {
            s.num(n, 1);
        }
        inked_centre(
            f,
            80,
            36,
            s.as_str(),
            if n == 0 { BOOST_UI } else { TEXT },
            4,
        );
    } else if race.phase == Phase::Done && race.after < RESULTS_AFTER {
        let place = race.place(0);
        let mut s = Line::new();
        if race.mode == Mode::Race {
            s.num(u32::from(place), 1)
                .push(ordinal(place))
                .push(" PLACE");
        } else {
            s.push("FINISHED");
        }
        inked_centre(f, 80, 40, s.as_str(), place_colour(place), 2);
    } else if app.note_t < 90 {
        let mut s = Line::new();
        let c = match app.note {
            Note::Lap(lap, t) => {
                s.push("LAP ")
                    .num((lap - 1).max(1) as u32, 1)
                    .push("  ")
                    .time(t);
                TEXT
            }
            Note::Final(t) => {
                s.push("FINAL LAP  ").time(t);
                SEL
            }
            Note::Boost if app.note_t < 40 => {
                s.push("BOOST");
                BOOST_UI
            }
            _ => TEXT,
        };
        if !s.as_str().is_empty() {
            inked_centre(f, 80, 30, s.as_str(), c, 1);
        }
    }
}

fn pause(app: &App, sel: u8, f: &mut Frame) {
    panel(f, PAUSE_BOX);
    centre(f, 80, PAUSE_BOX.1 + 5, "PAUSED", TEXT, 1);
    let (x0, x1) = (PAUSE_BOX.0 + 6, PAUSE_BOX.0 + PAUSE_BOX.2 as i16 - 6);
    let o = &app.opts;
    let lines: [(&str, Option<&str>); PAUSE_LINES as usize] = [
        ("RESUME", None),
        ("DETAIL", Some(o.detail.name())),
        ("RATE", Some(if o.fast { "60" } else { "30" })),
        ("VIEW", Some(o.view.name())),
        ("RESTART", None),
        ("TITLE", None),
        ("LEAVE", None),
    ];
    for (k, (label, value)) in lines.iter().enumerate() {
        let y = PAUSE_Y + k as i16 * LINE;
        menu_line(f, x0, x1, y, label, *value, k == usize::from(sel));
    }
}

fn results(app: &App, f: &mut Frame) {
    let race: &Race = &app.race;
    let sh = &race.ships[0];
    panel(f, END_BOX);
    let mut s = Line::new();
    let place = race.place(0);
    if race.mode == Mode::Race {
        s.num(u32::from(place), 1)
            .push(ordinal(place))
            .push(" PLACE");
    } else {
        s.push("TIME TRIAL");
    }
    centre(f, 80, END_BOX.1 + 4, s.as_str(), place_colour(place), 2);
    let top = END_BOX.1 + 18;
    if race.mode == Mode::Race {
        for (k, &i) in race.order.iter().take(usize::from(race.n)).enumerate() {
            let y = top + k as i16 * 8;
            let o = &race.ships[usize::from(i)];
            let me = i == 0;
            let c = if me { SEL } else { TEXT };
            let mut s = Line::new();
            s.num(k as u32 + 1, 1);
            f.text(END_BOX.0 + 6, y, s.as_str(), place_colour(k as u8 + 1));
            let name = ENTRANTS[usize::from(i) % ENTRANTS.len()].name;
            f.text(END_BOX.0 + 16, y, name, c);
            let mut s = Line::new();
            if o.finished() {
                s.time(o.finish);
            } else {
                s.push("RACING");
            }
            right(
                f,
                END_BOX.0 + END_BOX.2 as i16 - 6,
                y,
                s.as_str(),
                if o.finished() { c } else { TEXT_DIM },
                1,
            );
        }
    } else {
        let mut s = Line::new();
        s.push("TOTAL  ").time(sh.finish);
        centre(f, 80, top + 8, s.as_str(), TEXT, 1);
        let mut s = Line::new();
        s.push("BEST LAP  ").time(sh.best);
        centre(f, 80, top + 20, s.as_str(), SEL, 1);
        let best = app.session_best(race.track, race.class);
        let mut s = Line::new();
        s.push("SESSION BEST  ").time(best);
        centre(f, 80, top + 32, s.as_str(), TEXT_DIM, 1);
    }
    let (x0, x1) = (END_BOX.0 + 30, END_BOX.0 + END_BOX.2 as i16 - 30);
    menu_line(f, x0, x1, END_Y, "AGAIN", None, app.end_sel == 0);
    menu_line(f, x0, x1, END_Y + LINE, "TITLE", None, app.end_sel == 1);
    let _ = END_LINES;
}
