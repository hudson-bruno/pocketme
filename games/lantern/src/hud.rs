//! Everything drawn around and over the 3D view: the status line, the
//! frame of the light detail, messages, the map, the menus and cards,
//! and where a finger lands on them.

use game_kit::{Frame, text_width};

use crate::app::{Options, PAUSE_LINES, Run, TITLE_LINES};
use crate::art::{
    ACCENT, CRYSTAL, HEART, HEART_OFF, MAP_DOOR, MAP_FLOOR, MAP_GATE, MAP_LIFT, MAP_ME, MAP_WALL,
    OIL, OIL_LOW, TEXT, TEXT_DIM, UI_BG, UI_EDGE,
};
use crate::fixed::{SHIFT, TRIG_SHIFT, cos, sin};
use crate::level::LEVELS;
use crate::render::{Detail, VIEW_H};
use crate::world::{FLASH_COST, Kind, LIFE_MAX, OIL_MAX, World, cell};

/// The status line: its top row and height.
pub const STATUS_Y: i16 = VIEW_H as i16;
pub const STATUS_H: u16 = 120 - VIEW_H as u16;

/// The boxes on the status line a finger can tap: (x, width).
pub const FLASH_BOX: (i16, u16) = (93, 26);
pub const MAP_BOX: (i16, u16) = (121, 20);
pub const PAUSE_BOX: (i16, u16) = (143, 16);

/// What a tap on the status line hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusBox {
    Flash,
    Map,
    Pause,
}

/// The status-line box at (x, y), if any.
pub fn status_box_at(x: i16, y: i16) -> Option<StatusBox> {
    if y < STATUS_Y {
        return None;
    }
    let inside = |b: (i16, u16)| x >= b.0 - 1 && x < b.0 + b.1 as i16 + 1;
    if inside(FLASH_BOX) {
        Some(StatusBox::Flash)
    } else if inside(MAP_BOX) {
        Some(StatusBox::Map)
    } else if inside(PAUSE_BOX) {
        Some(StatusBox::Pause)
    } else {
        None
    }
}

/// Whether (x, y) is the middle of the view, where a tap uses what is
/// ahead (a door, a switch, the lift).
pub fn is_use_spot(detail: Detail, x: i16, y: i16) -> bool {
    let v = detail.view();
    let (x, y) = (i32::from(x) - v.x0, i32::from(y) - v.y0);
    x >= v.w / 3 && x < v.w * 2 / 3 && y >= v.h / 3 && y < v.h * 2 / 3
}

/// A finger held on the view steers: the top third walks forward, the
/// bottom third backs off, left and right of the middle turn (more the
/// further out). (forward, turn), each -256..=256; None off the view.
pub fn steer_at(detail: Detail, x: i16, y: i16) -> Option<(i32, i32)> {
    let v = detail.view();
    let (x, y) = (i32::from(x) - v.x0, i32::from(y) - v.y0);
    if x < 0 || y < 0 || x >= v.w || y >= v.h {
        return None;
    }
    let forward = if y < v.h / 3 {
        256
    } else if y >= v.h * 2 / 3 {
        -256
    } else {
        0
    };
    let off = x - v.w / 2;
    let dead = v.w / 6;
    let turn = if off.abs() <= dead {
        0
    } else {
        ((off - off.signum() * dead) * 256 / (v.w / 2 - dead)).clamp(-256, 256)
    };
    Some((forward, turn))
}

// ---- Menus --------------------------------------------------------------------

/// The title menu card and its lines.
pub const CARD_X: i16 = 26;
pub const CARD_Y: i16 = 24;
pub const CARD_W: u16 = 108;
pub const MENU_X: i16 = 36;
pub const MENU_Y: i16 = 30;
pub const MENU_STEP: i16 = 11;
/// The pause card's first line.
pub const PAUSE_Y: i16 = 44;

/// The title line at (x, y), if any.
pub fn title_line_at(x: i16, y: i16) -> Option<u8> {
    line_at(x, y, MENU_Y, TITLE_LINES)
}

/// The pause line at (x, y), if any.
pub fn pause_line_at(x: i16, y: i16) -> Option<u8> {
    line_at(x, y, PAUSE_Y, PAUSE_LINES)
}

fn line_at(x: i16, y: i16, first: i16, lines: u8) -> Option<u8> {
    if x < CARD_X || x >= CARD_X + CARD_W as i16 {
        return None;
    }
    let k = (i32::from(y) - i32::from(first) + 3).div_euclid(i32::from(MENU_STEP));
    (0..i32::from(lines)).contains(&k).then_some(k as u8)
}

/// Text centred on x = 80.
fn centred(s: &mut Frame, y: i16, text: &str, c: u8) {
    let x = 80 - (text_width(text) / 2) as i16;
    s.text(x, y, text, c);
}

fn card(s: &mut Frame, x: i16, y: i16, w: u16, h: u16) {
    s.rect(x, y, w, h, UI_BG);
    s.outline(x, y, w, h, UI_EDGE);
}

/// The title: the name in the top border, the menu over the view, the
/// level's name on the status line.
pub fn title(s: &mut Frame, sel: u8, o: &Options) {
    s.rect(0, 0, 160, 12, UI_BG);
    let name = "LANTERN";
    let w = text_width(name) * 2;
    s.text_scaled(80 - (w / 2) as i16, 1, name, ACCENT, 2);
    card(s, CARD_X, CARD_Y, CARD_W, 5 * MENU_STEP as u16 + 8);
    let mut level = [0u8; 12];
    let level = fmt_num(&mut level, "LEVEL ", u32::from(o.level) + 1);
    let lines: [(&str, &str); TITLE_LINES as usize] = [
        ("PLAY", ""),
        (level, ""),
        ("DETAIL", o.detail.name()),
        ("RATE", if o.fast { "60" } else { "30" }),
        ("LEAVE", ""),
    ];
    for (i, (a, b)) in lines.iter().enumerate() {
        let y = MENU_Y + i as i16 * MENU_STEP;
        let on = i as u8 == sel;
        let c = if on { TEXT } else { TEXT_DIM };
        if on {
            s.text(MENU_X - 6, y, ">", ACCENT);
        }
        s.text(MENU_X, y, a, c);
        if !b.is_empty() {
            s.text(MENU_X + 44, y, b, if on { ACCENT } else { TEXT_DIM });
        }
    }
    s.rect(0, STATUS_Y, 160, STATUS_H, UI_BG);
    s.hline(0, STATUS_Y, 160, UI_EDGE);
    centred(s, STATUS_Y + 6, LEVELS[usize::from(o.level)].name, TEXT_DIM);
}

/// The game's pause menu.
pub fn pause(s: &mut Frame, sel: u8) {
    card(s, CARD_X, PAUSE_Y - 14, CARD_W, 3 * MENU_STEP as u16 + 18);
    centred(s, PAUSE_Y - 10, "PAUSED", ACCENT);
    for (i, t) in ["RESUME", "RESTART LEVEL", "TITLE"].iter().enumerate() {
        let y = PAUSE_Y + 2 + i as i16 * MENU_STEP;
        let on = i as u8 == sel;
        if on {
            s.text(MENU_X - 6, y, ">", ACCENT);
        }
        s.text(MENU_X, y, t, if on { TEXT } else { TEXT_DIM });
    }
}

/// A note at the bottom of the view. In LIGHT the message line sits in
/// the same rows (under the window), so the whole width is cleared there:
/// otherwise the message's ends show on both sides of the note.
pub fn note(s: &mut Frame, text: &str, detail: Detail) {
    let w = text_width(text) + 6;
    let x = 80 - (w / 2) as i16;
    if detail == Detail::Light {
        s.rect(0, STATUS_Y - 10, 160, 9, UI_BG);
    }
    s.rect(x, STATUS_Y - 10, w, 9, UI_BG);
    s.text(x + 3, STATUS_Y - 8, text, ACCENT);
}

/// The level-clear card. `ready`: it takes a press now.
pub fn clear_card(s: &mut Frame, w: &World, ready: bool) {
    card(s, 22, 18, 116, 68);
    centred(s, 23, "THE LIFT CLIMBS", ACCENT);
    let t = w.tally;
    let mut b = [0u8; 24];
    centred(
        s,
        36,
        fmt_of(
            &mut b,
            "CRYSTALS ",
            t.crystals.into(),
            t.crystals_total.into(),
        ),
        CRYSTAL,
    );
    let mut b = [0u8; 24];
    centred(
        s,
        46,
        fmt_num(&mut b, "MOTHS SENT AWAY ", t.moths_sent.into()),
        TEXT,
    );
    let mut b = [0u8; 24];
    centred(
        s,
        56,
        fmt_num(&mut b, "CREATURES STOPPED ", t.stuns.into()),
        TEXT,
    );
    let mut b = [0u8; 24];
    centred(s, 66, fmt_time(&mut b, "TIME ", w.ticks / 60), TEXT_DIM);
    if ready {
        centred(s, 77, "A: ON", TEXT);
    }
}

/// The card when the last heart goes.
pub fn faint_card(s: &mut Frame, ready: bool) {
    card(s, 22, 30, 116, 40);
    centred(s, 36, "THE LAMP FALLS", HEART);
    centred(s, 47, "BACK TO THE LEVEL START", TEXT_DIM);
    if ready {
        centred(s, 58, "A: TRY AGAIN", TEXT);
    }
}

/// After the last level.
pub fn end(s: &mut Frame, r: &Run) {
    s.clear(UI_BG);
    s.outline(4, 4, 152, 112, UI_EDGE);
    let w = text_width("DAYLIGHT") * 2;
    s.text_scaled(80 - (w / 2) as i16, 14, "DAYLIGHT", ACCENT, 2);
    centred(s, 32, "THE LAST LIFT REACHES THE TOP", TEXT_DIM);
    let mut b = [0u8; 24];
    centred(
        s,
        48,
        fmt_of(&mut b, "CRYSTALS ", r.crystals, r.crystals_total),
        CRYSTAL,
    );
    let mut b = [0u8; 24];
    centred(s, 58, fmt_num(&mut b, "MOTHS SENT AWAY ", r.moths), TEXT);
    let mut b = [0u8; 24];
    centred(s, 68, fmt_num(&mut b, "CREATURES STOPPED ", r.stuns), TEXT);
    let mut b = [0u8; 24];
    centred(s, 78, fmt_num(&mut b, "FALLS ", r.retries), TEXT);
    let mut b = [0u8; 24];
    centred(s, 88, fmt_time(&mut b, "TIME ", r.ticks / 60), TEXT_DIM);
    centred(s, 104, "A: TITLE", TEXT);
}

// ---- The view's surroundings ----------------------------------------------------

/// The frame round the light detail's smaller window, with the level's
/// name on top. It does not change while playing, so it costs nothing
/// to send after the first picture.
pub fn light_frame(s: &mut Frame, w: &World) {
    let v = Detail::Light.view();
    let (x0, y0, x1, y1) = (
        v.x0 as i16,
        v.y0 as i16,
        (v.x0 + v.w) as i16,
        (v.y0 + v.h) as i16,
    );
    s.rect(0, 0, 160, y0 as u16, UI_BG);
    s.rect(0, y1, 160, (VIEW_H as i16 - y1) as u16, UI_BG);
    s.rect(0, y0, x0 as u16, v.h as u16, UI_BG);
    s.rect(x1, y0, (160 - x1) as u16, v.h as u16, UI_BG);
    s.outline(x0 - 2, y0 - 2, v.w as u16 + 4, v.h as u16 + 4, UI_EDGE);
    centred(s, 3, w.level_info().name, TEXT_DIM);
}

/// Over the view: the world's message, and a red edge after a hurt.
pub fn view_marks(s: &mut Frame, w: &World, detail: Detail) {
    let v = detail.view();
    if w.hurt > crate::world::HURT_TICKS - 12 {
        s.outline(v.x0 as i16, v.y0 as i16, v.w as u16, v.h as u16, HEART);
        s.outline(
            v.x0 as i16 + 1,
            v.y0 as i16 + 1,
            v.w as u16 - 2,
            v.h as u16 - 2,
            HEART,
        );
    }
    let text = w.msg.text();
    if text.is_empty() {
        return;
    }
    if detail == Detail::Light {
        // In the bottom border, under the window.
        let y = (v.y0 + v.h) as i16 + 3;
        s.rect(0, y - 1, 160, 7, UI_BG);
        centred(s, y, text, ACCENT);
    } else {
        let tw = text_width(text);
        let x = 80 - (tw / 2) as i16;
        s.rect(x - 2, 2, tw + 4, 9, UI_BG);
        s.text(x, 4, text, ACCENT);
    }
}

/// The status line: hearts, lamp oil, crystals, the crank, and the
/// FLASH, MAP and pause boxes for fingers.
pub fn status(s: &mut Frame, w: &World, _detail: Detail) {
    s.rect(0, STATUS_Y, 160, STATUS_H, UI_BG);
    s.hline(0, STATUS_Y, 160, UI_EDGE);
    let y = STATUS_Y + 5;
    for i in 0..LIFE_MAX {
        let c = if i < w.life { HEART } else { HEART_OFF };
        heart(s, 3 + i as i16 * 8, y, c);
    }
    // Lamp oil: a bar, red when too low for a flash.
    let (bx, bw) = (27i16, 28u16);
    s.outline(bx, y - 1, bw, 7, UI_EDGE);
    let fill = (w.oil.clamp(0, OIL_MAX) * (bw as i32 - 2) / OIL_MAX) as u16;
    let c = if w.oil < FLASH_COST { OIL_LOW } else { OIL };
    s.rect(bx + 1, y, fill, 5, c);
    // Crystals.
    gem(s, 58, y, CRYSTAL);
    let mut b = [0u8; 12];
    s.text(
        64,
        y,
        fmt_of(
            &mut b,
            "",
            w.tally.crystals.into(),
            w.tally.crystals_total.into(),
        ),
        TEXT,
    );
    // The crank, once found.
    if w.crank {
        s.rect(86, y, 2, 5, ACCENT);
        s.rect(86, y, 4, 2, ACCENT);
    }
    let boxed = |s: &mut Frame, b: (i16, u16), t: &str, c: u8| {
        s.outline(b.0, STATUS_Y + 2, b.1, STATUS_H - 4, UI_EDGE);
        let tx = b.0 + (b.1 as i16 - text_width(t) as i16) / 2;
        s.text(tx, y, t, c);
    };
    let ready = w.cool == 0 && w.oil >= FLASH_COST;
    boxed(s, FLASH_BOX, "FLASH", if ready { TEXT } else { TEXT_DIM });
    boxed(s, MAP_BOX, "MAP", TEXT);
    boxed(s, PAUSE_BOX, "II", TEXT);
}

fn heart(s: &mut Frame, x: i16, y: i16, c: u8) {
    s.rect(x, y, 2, 2, c);
    s.rect(x + 3, y, 2, 2, c);
    s.hline(x, y + 1, 5, c);
    s.hline(x, y + 2, 5, c);
    s.hline(x + 1, y + 3, 3, c);
    s.pixel(x + 2, y + 4, c);
}

fn gem(s: &mut Frame, x: i16, y: i16, c: u8) {
    s.pixel(x + 2, y, c);
    s.hline(x + 1, y + 1, 3, c);
    s.hline(x, y + 2, 5, c);
    s.hline(x + 1, y + 3, 3, c);
    s.pixel(x + 2, y + 4, c);
}

// ---- The map ----------------------------------------------------------------------

/// The map of every cell the lamp has shown, with the player, the
/// crystals seen and the lift.
pub fn map(s: &mut Frame, w: &World) {
    s.rect(0, 0, 160, VIEW_H as u16, UI_BG);
    let k = (96 / w.w.max(w.h)).clamp(2, 8);
    let (ox, oy) = (4 + (96 - w.w * k) / 2, 4 + (96 - w.h * k) / 2);
    s.outline(
        ox as i16 - 2,
        oy as i16 - 2,
        (w.w * k + 4) as u16,
        (w.h * k + 4) as u16,
        UI_EDGE,
    );
    for cy in 0..w.h {
        for cx in 0..w.w {
            if !w.cell_seen(cx, cy) {
                continue;
            }
            let c = match w.at(cx, cy) {
                cell::FLOOR => MAP_FLOOR,
                cell::DOOR => {
                    if w.open_at(cx, cy) > 0 {
                        MAP_FLOOR
                    } else {
                        MAP_DOOR
                    }
                }
                cell::GATE_A | cell::GATE_B => {
                    if w.open_at(cx, cy) > 0 {
                        MAP_FLOOR
                    } else {
                        MAP_GATE
                    }
                }
                cell::SW_A | cell::SW_B | cell::SW_A_ON | cell::SW_B_ON => ACCENT,
                cell::LIFT => MAP_LIFT,
                _ => MAP_WALL,
            };
            s.rect(
                (ox + cx * k) as i16,
                (oy + cy * k) as i16,
                k as u16,
                k as u16,
                c,
            );
        }
    }
    for t in w.things() {
        if !t.alive || t.kind.creature() {
            continue;
        }
        let (cx, cy) = (t.x >> SHIFT, t.y >> SHIFT);
        if !w.cell_seen(cx, cy) {
            continue;
        }
        let c = match t.kind {
            Kind::Crystal => CRYSTAL,
            Kind::Crank => ACCENT,
            _ => OIL,
        };
        let px = ox + ((t.x * k) >> SHIFT) - k / 4;
        let py = oy + ((t.y * k) >> SHIFT) - k / 4;
        s.rect(
            px as i16,
            py as i16,
            (k / 2).max(1) as u16,
            (k / 2).max(1) as u16,
            c,
        );
    }
    // The player and a line for where it looks.
    let (px, py) = (ox + ((w.x * k) >> SHIFT), oy + ((w.y * k) >> SHIFT));
    let (c, sn) = (cos(w.angle), sin(w.angle));
    for d in 1..=(k + 2) {
        s.pixel(
            (px + ((d * c) >> TRIG_SHIFT)) as i16,
            (py + ((d * sn) >> TRIG_SHIFT)) as i16,
            MAP_ME,
        );
    }
    s.rect(px as i16 - 1, py as i16 - 1, 3, 3, MAP_ME);

    // Beside the map: the level and what the colours are.
    let lx = 106i16;
    let mut b = [0u8; 12];
    s.text(lx, 6, fmt_num(&mut b, "LEVEL ", w.level as u32 + 1), ACCENT);
    let name = w.level_info().name;
    // The name, one word a line.
    let mut y = 14;
    for word in name.split(' ') {
        s.text(lx, y, word, TEXT);
        y += 7;
    }
    let key: [(u8, &str); 5] = [
        (MAP_DOOR, "DOOR"),
        (MAP_GATE, "GATE"),
        (ACCENT, "SWITCH"),
        (MAP_LIFT, "LIFT"),
        (CRYSTAL, "CRYSTAL"),
    ];
    for (i, (c, t)) in key.iter().enumerate() {
        let y = 36 + i as i16 * 8;
        s.rect(lx, y, 5, 5, *c);
        s.text(lx + 8, y, t, TEXT_DIM);
    }
    s.text(lx, 82, "SELECT OR", TEXT_DIM);
    s.text(lx, 89, "TAP: BACK", TEXT_DIM);
}

// ---- Numbers into text, without a heap ---------------------------------------------

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

/// `prefix` and minutes:seconds.
pub fn fmt_time<'a>(buf: &'a mut [u8], prefix: &str, secs: u32) -> &'a str {
    let mut len = 0;
    push_str(buf, &mut len, prefix);
    push_num(buf, &mut len, secs / 60);
    push_str(buf, &mut len, ":");
    if secs % 60 < 10 {
        push_str(buf, &mut len, "0");
    }
    push_num(buf, &mut len, secs % 60);
    as_str(buf, len)
}

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
