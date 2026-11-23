//! Everything drawn round and over the 3D view: the top bar (clock,
//! gates, hull), the touch boxes along the bottom, the LOW detail's
//! frame, notes, the title, the pause menu, the cards; and where a
//! finger lands on them.

use game_kit::{Frame, text_width};

use crate::app::{Options, PAUSE_LINES, Run, TITLE_LINES};
use crate::art::{ACCENT, BAD, BOX_ON, FRAME, FRAME_LIGHT, GOOD, TEXT, TEXT_DIM, UI_BG, UI_EDGE};
use crate::course::MISSIONS;
use crate::scene::{BOTTOM_Y, Detail, Eye, TOP_H};
use crate::world::{HULL_MAX, HZ, State, World};

// ---- Touch boxes ------------------------------------------------------------------------

/// The boxes along the bottom, left to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxKey {
    RollLeft,
    Slow,
    Pause,
    Fast,
    RollRight,
}

pub const BOXES: [BoxKey; 5] = [
    BoxKey::RollLeft,
    BoxKey::Slow,
    BoxKey::Pause,
    BoxKey::Fast,
    BoxKey::RollRight,
];

/// Each box is 32 pixels wide.
pub const BOX_W: i32 = 32;

/// The bottom box at (x, y), if any.
pub fn box_at(x: i16, y: i16) -> Option<BoxKey> {
    let (x, y) = (i32::from(x), i32::from(y));
    if !(BOTTOM_Y..120).contains(&y) || !(0..160).contains(&x) {
        return None;
    }
    Some(BOXES[(x / BOX_W) as usize])
}

/// Whether (x, y) is on the 3D view (where a finger steers).
pub fn on_view(d: Detail, x: i16, y: i16) -> bool {
    let a = d.area();
    let (x, y) = (i32::from(x), i32::from(y));
    // In LOW, the frame round the view steers too (a finger there
    // means "that way").
    let a = if d == Detail::Low {
        crate::poly::Rect {
            x0: 0,
            y0: TOP_H,
            x1: 160,
            y1: BOTTOM_Y,
        }
    } else {
        a
    };
    x >= a.x0 && x < a.x1 && y >= a.y0 && y < a.y1
}

// ---- Menus ------------------------------------------------------------------------------

pub const MENU_STEP: i16 = 9;
pub const TITLE_CARD: (i16, i16, u16, u16) = (14, 60, 132, 58);
pub const TITLE_Y: i16 = 64;
pub const PAUSE_CARD: (i16, i16, u16, u16) = (22, 24, 116, 74);
pub const PAUSE_Y: i16 = 38;
const LABEL_DX: i16 = 10;
const VALUE_DX: i16 = 58;

/// The title line at (x, y), if any.
pub fn title_line_at(x: i16, y: i16) -> Option<u8> {
    line_at(x, y, TITLE_CARD, TITLE_Y, TITLE_LINES)
}

/// The pause line at (x, y), if any.
pub fn pause_line_at(x: i16, y: i16) -> Option<u8> {
    line_at(x, y, PAUSE_CARD, PAUSE_Y, PAUSE_LINES)
}

/// Whether (x, y) is on the pause card (its lines or between them).
pub fn on_pause_card(x: i16, y: i16) -> bool {
    let (cx, cy, w, h) = PAUSE_CARD;
    (cx..cx + w as i16).contains(&x) && (cy..cy + h as i16).contains(&y)
}

fn line_at(x: i16, y: i16, card: (i16, i16, u16, u16), first: i16, lines: u8) -> Option<u8> {
    if x < card.0 || x >= card.0 + card.2 as i16 {
        return None;
    }
    let k = (i32::from(y) - i32::from(first) + 2).div_euclid(i32::from(MENU_STEP));
    (0..i32::from(lines)).contains(&k).then_some(k as u8)
}

/// The failed card's two boxes: TRY AGAIN (left) and TITLE (right).
pub const FAIL_BOXES_Y: i16 = 76;

/// On the failed card: Some(true) for TRY AGAIN, Some(false) for TITLE.
pub fn fail_box_at(x: i16, y: i16) -> Option<bool> {
    if !(FAIL_BOXES_Y - 2..FAIL_BOXES_Y + 12).contains(&y) {
        return None;
    }
    if (24..78).contains(&x) {
        Some(true)
    } else if (82..136).contains(&x) {
        Some(false)
    } else {
        None
    }
}

/// Text centred on x = 80.
// Not inlined: called from many places, and the code slot is small.
#[inline(never)]
pub fn centred(s: &mut Frame, y: i16, text: &str, c: u8) {
    let x = 80 - (text_width(text) / 2) as i16;
    s.text(x, y, text, c);
}

// Not inlined: called from many places, and the code slot is small.
#[inline(never)]
fn card(s: &mut Frame, (x, y, w, h): (i16, i16, u16, u16)) {
    s.rect(x, y, w, h, UI_BG);
    s.outline(x, y, w, h, UI_EDGE);
}

fn menu_line(s: &mut Frame, x: i16, y: i16, on: bool, label: &str, value: &str) {
    if on {
        s.text(x + LABEL_DX - 6, y, ">", ACCENT);
    }
    s.text(x + LABEL_DX, y, label, if on { TEXT } else { TEXT_DIM });
    if !value.is_empty() {
        s.text(x + VALUE_DX, y, value, if on { ACCENT } else { TEXT_DIM });
    }
}

fn rate_name(fast: bool) -> &'static str {
    if fast { "60 A SECOND" } else { "30 A SECOND" }
}

/// The title: the name, and the menu card (the turning craft is drawn
/// by the app).
pub fn title(s: &mut Frame, sel: u8, o: &Options) {
    let name = "KESTREL";
    let w = text_width(name) * 2;
    s.text_scaled(80 - (w / 2) as i16, 3, name, ACCENT, 2);
    card(s, TITLE_CARD);
    let mut b = [0u8; 12];
    let mission = fmt_num(&mut b, "MISSION ", u32::from(o.mission) + 1);
    let lines: [(&str, &str); TITLE_LINES as usize] = [
        ("PLAY", ""),
        (mission, MISSIONS[usize::from(o.mission)].name),
        ("DETAIL", o.detail.name()),
        ("VIEW", o.eye.name()),
        ("PICTURES", rate_name(o.fast)),
        ("LEAVE", ""),
    ];
    for (i, (a, v)) in lines.iter().enumerate() {
        let y = TITLE_Y + i as i16 * MENU_STEP;
        menu_line(s, TITLE_CARD.0, y, i as u8 == sel, a, v);
    }
}

/// The game's pause menu: every setting can be changed here.
pub fn pause(s: &mut Frame, sel: u8, o: &Options) {
    card(s, PAUSE_CARD);
    centred(s, PAUSE_CARD.1 + 5, "PAUSED", ACCENT);
    let lines: [(&str, &str); PAUSE_LINES as usize] = [
        ("RESUME", ""),
        ("DETAIL", o.detail.name()),
        ("VIEW", o.eye.name()),
        ("PICTURES", rate_name(o.fast)),
        ("RESTART", ""),
        ("TITLE", ""),
    ];
    for (i, (a, v)) in lines.iter().enumerate() {
        let y = PAUSE_Y + i as i16 * MENU_STEP;
        menu_line(s, PAUSE_CARD.0, y, i as u8 == sel, a, v);
    }
}

/// The briefing before a mission.
pub fn brief(s: &mut Frame, n: u8, ready: bool) {
    let m = &MISSIONS[usize::from(n)];
    card(s, (14, 26, 132, 68));
    let mut b = [0u8; 20];
    centred(
        s,
        31,
        fmt_of(&mut b, "MISSION ", u32::from(n) + 1, MISSIONS.len() as u32),
        TEXT_DIM,
    );
    centred(s, 41, m.name, ACCENT);
    centred(s, 52, m.brief[0], TEXT);
    centred(s, 60, m.brief[1], TEXT);
    let mut b = [0u8; 24];
    let mut len = 0;
    push_str(&mut b, &mut len, "GATES ");
    push_num(&mut b, &mut len, u32::from(m.gates));
    push_str(&mut b, &mut len, "  CLOCK ");
    push_num(&mut b, &mut len, m.start_s10 / 10);
    push_str(&mut b, &mut len, " S");
    centred(s, 71, as_str(&b, len), TEXT_DIM);
    if ready {
        centred(s, 83, "A OR TAP: LAUNCH", GOOD);
    }
}

/// After a mission: delivered, or why not.
pub fn result(s: &mut Frame, w: &World, ready: bool, last: bool) {
    let ok = w.state == State::Delivered;
    card(s, (14, 22, 132, 74));
    let (head, c) = match w.state {
        State::Delivered => ("CARGO DELIVERED", GOOD),
        State::OutOfTime => ("OUT OF TIME", BAD),
        State::Lost => ("HULL BROKEN", BAD),
        State::MissedDock => ("MISSED THE DOCK", BAD),
        State::Flying => ("", TEXT),
    };
    centred(s, 27, head, c);
    let t = w.tally;
    let mut b = [0u8; 24];
    centred(
        s,
        38,
        fmt_of(
            &mut b,
            "GATES ",
            u32::from(t.passed),
            u32::from(w.m().gates),
        ),
        TEXT,
    );
    let mut b = [0u8; 24];
    centred(
        s,
        46,
        fmt_num(&mut b, "TIME CELLS ", u32::from(t.cells)),
        TEXT,
    );
    let mut b = [0u8; 24];
    centred(
        s,
        54,
        fmt_of(&mut b, "HULL ", u32::from(w.hull), u32::from(HULL_MAX)),
        TEXT,
    );
    if ok {
        let mut b = [0u8; 24];
        centred(s, 62, fmt_s10(&mut b, "TIME LEFT ", w.clock_s10()), TEXT);
        let mut b = [0u8; 24];
        centred(s, 72, fmt_num(&mut b, "SCORE ", w.score), ACCENT);
        if ready {
            centred(
                s,
                84,
                if last {
                    "A OR TAP: FINISH"
                } else {
                    "A OR TAP: NEXT MISSION"
                },
                GOOD,
            );
        }
    } else if ready {
        for (x, t) in [(24i16, "TRY AGAIN"), (82, "TITLE")] {
            s.outline(x, FAIL_BOXES_Y - 2, 54, 13, UI_EDGE);
            let tx = x + 27 - (text_width(t) / 2) as i16;
            s.text(tx, FAIL_BOXES_Y + 2, t, TEXT);
        }
        centred(s, 66, "A: AGAIN   B: TITLE", TEXT_DIM);
    }
}

/// After the last mission.
pub fn end(s: &mut Frame, r: &Run) {
    card(s, (10, 16, 140, 88));
    let w = text_width("ALL DELIVERED") * 2;
    s.text_scaled(80 - (w / 2) as i16, 22, "ALL DELIVERED", GOOD, 2);
    centred(s, 38, "HALDEN STATION THANKS YOU", TEXT_DIM);
    let mut b = [0u8; 24];
    centred(s, 50, fmt_num(&mut b, "SCORE ", r.score), ACCENT);
    let mut b = [0u8; 24];
    centred(
        s,
        60,
        fmt_of(&mut b, "GATES ", r.gates, r.gates_total),
        TEXT,
    );
    let mut b = [0u8; 24];
    centred(s, 68, fmt_num(&mut b, "TIME CELLS ", r.cells), TEXT);
    let mut b = [0u8; 24];
    centred(s, 76, fmt_num(&mut b, "HITS TAKEN ", r.hits), TEXT);
    let mut b = [0u8; 24];
    centred(s, 84, fmt_num(&mut b, "TRIES AGAIN ", r.retries), TEXT);
    centred(s, 95, "A OR TAP: TITLE", GOOD);
}

// ---- Round the view -------------------------------------------------------------------

/// LOW's frame round its smaller view. It does not change while
/// playing, so after the first picture it costs nothing to send.
pub fn low_frame(s: &mut Frame) {
    let a = Detail::Low.area();
    let (x0, y0, x1, y1) = (a.x0 as i16, a.y0 as i16, a.x1 as i16, a.y1 as i16);
    let top = TOP_H as i16;
    let bottom = BOTTOM_Y as i16;
    s.rect(0, top, 160, (y0 - top) as u16, FRAME);
    s.rect(0, y1, 160, (bottom - y1) as u16, FRAME);
    s.rect(0, y0, x0 as u16, (y1 - y0) as u16, FRAME);
    s.rect(x1, y0, (160 - x1) as u16, (y1 - y0) as u16, FRAME);
    s.outline(
        x0 - 1,
        y0 - 1,
        (x1 - x0 + 2) as u16,
        (y1 - y0 + 2) as u16,
        FRAME_LIGHT,
    );
}

/// The top bar: the clock, the gates, the hull.
pub fn top_bar(s: &mut Frame, w: &World) {
    s.rect(0, 0, 160, TOP_H as u16, UI_BG);
    s.hline(0, TOP_H as i16 - 1, 160, UI_EDGE);
    let left = w.clock_s10();
    let mut b = [0u8; 16];
    let low = left < 50 && w.state == State::Flying;
    s.text(
        3,
        2,
        fmt_s10(&mut b, "TIME ", left),
        if low { BAD } else { TEXT },
    );
    let mut b = [0u8; 16];
    let gate = (u32::from(w.next_gate) + 1).min(u32::from(w.m().gates));
    let text = if w.next_gate >= w.m().gates {
        "DOCK"
    } else {
        fmt_of(&mut b, "GATE ", gate, u32::from(w.m().gates))
    };
    s.text(62, 2, text, TEXT);
    // Hull: one block per hit it can still take.
    let hit = w.ship.hurt > 0 && w.state == State::Flying;
    s.text(112, 2, "HULL", if hit { BAD } else { TEXT_DIM });
    for i in 0..HULL_MAX {
        let c = if i < w.hull { GOOD } else { UI_EDGE };
        s.rect(134 + i as i16 * 8, 2, 6, 5, c);
    }
}

/// The touch boxes. `throttle`: -1 braking, 0 cruise, 1 boosting.
pub fn bottom_bar(s: &mut Frame, throttle: i8) {
    let y = BOTTOM_Y as i16;
    s.rect(0, y, 160, (120 - BOTTOM_Y) as u16, UI_BG);
    s.hline(0, y, 160, UI_EDGE);
    for (i, k) in BOXES.iter().enumerate() {
        let x = i as i16 * BOX_W as i16;
        let (t, on) = match k {
            BoxKey::RollLeft => ("<ROLL", false),
            BoxKey::Slow => ("SLOW", throttle < 0),
            BoxKey::Pause => ("II", false),
            BoxKey::Fast => ("FAST", throttle > 0),
            BoxKey::RollRight => ("ROLL>", false),
        };
        if on {
            s.rect(x + 2, y + 2, BOX_W as u16 - 4, 10, BOX_ON);
        }
        s.outline(x + 1, y + 2, BOX_W as u16 - 2, 11, UI_EDGE);
        let tx = x + (BOX_W as i16 - text_width(t) as i16) / 2;
        s.text(tx, y + 5, t, if on { ACCENT } else { TEXT });
    }
}

/// Over the view: the sight in the nose view. (A hit shows in the top
/// bar and on the craft, not as a red edge round the view: an edge
/// touches every row of the view, and the console would send the whole
/// view twice.)
pub fn view_marks(s: &mut Frame, d: Detail, eye: Eye) {
    let a = d.area();
    if eye == Eye::Nose {
        let (cx, cy) = ((a.x0 + a.x1) / 2, (a.y0 + a.y1) / 2);
        let (cx, cy) = (cx as i16, cy as i16);
        s.hline(cx - 6, cy, 4, ACCENT);
        s.hline(cx + 3, cy, 4, ACCENT);
        s.vline(cx, cy - 5, 3, ACCENT);
        s.vline(cx, cy + 3, 3, ACCENT);
    }
}

/// Height of a note's box.
pub const NOTE_H: i16 = 9;

/// The top row of a note: at the top of the view, or, while the pause
/// card is open, above the card (in LOW the view's top would put the note
/// over the card's PAUSED line).
pub fn note_y(d: Detail, paused: bool) -> i16 {
    let y = d.area().y0 as i16 + 2;
    if paused {
        y.min(PAUSE_CARD.1 - NOTE_H - 2)
    } else {
        y
    }
}

/// A short note at the top of the view (see [`note_y`]).
pub fn note(s: &mut Frame, text: &str, d: Detail, paused: bool) {
    let w = text_width(text) + 6;
    let x = 80 - (w / 2) as i16;
    let y = note_y(d, paused);
    s.rect(x, y, w, NOTE_H as u16, UI_BG);
    s.text(x + 3, y + 2, text, ACCENT);
}

// ---- Numbers into text, without a heap ----------------------------------------------

/// `prefix` and `n` written into `buf`.
pub fn fmt_num<'a>(buf: &'a mut [u8], prefix: &str, n: u32) -> &'a str {
    let mut len = 0;
    push_str(buf, &mut len, prefix);
    push_num(buf, &mut len, n);
    as_str(buf, len)
}

/// `prefix`, `a`, "/", `b`.
pub fn fmt_of<'a>(buf: &'a mut [u8], prefix: &str, a: u32, b: u32) -> &'a str {
    let mut len = 0;
    push_str(buf, &mut len, prefix);
    push_num(buf, &mut len, a);
    push_str(buf, &mut len, "/");
    push_num(buf, &mut len, b);
    as_str(buf, len)
}

/// `prefix` and tenths of a second as "12.3".
pub fn fmt_s10<'a>(buf: &'a mut [u8], prefix: &str, s10: u32) -> &'a str {
    let mut len = 0;
    push_str(buf, &mut len, prefix);
    push_num(buf, &mut len, s10 / 10);
    push_str(buf, &mut len, ".");
    push_num(buf, &mut len, s10 % 10);
    as_str(buf, len)
}

/// Seconds from ticks, in tenths.
pub fn s10_of_ticks(t: u32) -> u32 {
    t * 10 / HZ
}

// Not inlined: called from many places, and the code slot is small.
#[inline(never)]
fn push_str(buf: &mut [u8], len: &mut usize, s: &str) {
    for &b in s.as_bytes() {
        if *len < buf.len() {
            buf[*len] = b;
            *len += 1;
        }
    }
}

fn push_num(buf: &mut [u8], len: &mut usize, n: u32) {
    let mut digits = [0u8; 10];
    let mut k = 0;
    let mut v = n;
    loop {
        digits[k] = b'0' + (v % 10) as u8;
        k += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    while k > 0 {
        k -= 1;
        if *len < buf.len() {
            buf[*len] = digits[k];
            *len += 1;
        }
    }
}

fn as_str(buf: &[u8], len: usize) -> &str {
    // Only ASCII was written.
    core::str::from_utf8(&buf[..len]).unwrap_or("")
}
