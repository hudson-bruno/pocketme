//! Drawing the three pages.
//!
//! The screen driver sends every drawing call over the SPI bus, so only
//! what changed is drawn. Each thing on screen (a button, a stick, a line
//! of text) has a "slot" that remembers a hash of what was last drawn
//! there. When the hash is the same, nothing is sent. A page change, a
//! layout change, a connection or a disconnection redraws everything.
//!
//! Text is always drawn with a background colour: the font code then sends
//! each letter as one small filled rectangle instead of pixel by pixel.

use core::fmt::Write;

use embedded_graphics::{
    mono_font::{
        MonoFont, MonoTextStyleBuilder,
        ascii::{FONT_5X8, FONT_6X10, FONT_10X20},
    },
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{
        Circle, Line as GLine, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle, RoundedRectangle,
        StrokeAlignment,
    },
    text::{Baseline, Text},
};

use crate::decode::{Btn, Face, Layout};
use crate::descriptor::Kind;
use crate::startup::{Phase, STEPS, StepState, VARIANTS};
use crate::text::{Line, class_words, fnv, hex, signed_2dp, signed_fixed, usage_name};
use crate::{Page, View};

pub const WIDTH: u32 = 320;
pub const HEIGHT: u32 = 240;

const SLOTS: usize = 128;
/// Lines whose last text is kept, so that only the changed letters are
/// drawn again (the long lines of numbers that change all the time).
const DIFF_LINES: usize = 16;
const DIFF_LEN: usize = 64;

// Colours (RGB565 takes red 0-31, green 0-63, blue 0-31).
const BG: Rgb565 = Rgb565::new(0, 0, 0);
const HEAD_BG: Rgb565 = Rgb565::new(2, 10, 9);
const TEXT: Rgb565 = Rgb565::new(31, 63, 31);
const DIM: Rgb565 = Rgb565::new(13, 27, 14);
const YELLOW: Rgb565 = Rgb565::new(31, 58, 0);
const CYAN: Rgb565 = Rgb565::new(0, 52, 31);
const RED: Rgb565 = Rgb565::new(31, 8, 6);
const BODY: Rgb565 = Rgb565::new(5, 11, 7);
const BODY_EDGE: Rgb565 = Rgb565::new(13, 27, 16);
const BTN_OFF: Rgb565 = Rgb565::new(10, 21, 12);
const GATE: Rgb565 = Rgb565::new(2, 5, 3);
const GREEN: Rgb565 = Rgb565::new(6, 60, 8);

/// What was last drawn, per slot.
pub struct Screen {
    hash: [u32; SLOTS],
    valid: [bool; SLOTS],
    /// Width in pixels of the text last drawn in the slot.
    width: [u16; SLOTS],
    full: bool,
    /// What kind of picture is on screen (page and "waiting" or not).
    shown: u8,
    /// Last text of the "diff" lines, and its length (0xFF: not drawn).
    prev: [[u8; DIFF_LEN]; DIFF_LINES],
    prev_len: [u8; DIFF_LINES],
    /// Where the two stick dots were drawn.
    dots: [Option<Point>; 2],
    /// Where the motion ball was drawn, and its colour.
    ball: Option<(Point, Rgb565)>,
    /// When the raw bytes (hex) were last drawn, see [`RAW_GAP_MS`].
    raw_ms: Option<u32>,
}

/// The lines of raw bytes in hex (the last report on the Pad page, the
/// last report of each id on the Data page) are drawn again at most twice
/// a second. With the motion sensor on, almost every byte from 13 on
/// changes in every report (sensor noise), so drawing them on every call
/// would cost about a quarter of the board's time. Twice a second is
/// still enough to read them.
const RAW_GAP_MS: u32 = 500;

impl Screen {
    pub const fn new() -> Self {
        Screen {
            hash: [0; SLOTS],
            valid: [false; SLOTS],
            width: [0; SLOTS],
            full: true,
            shown: 0xFF,
            prev: [[0; DIFF_LEN]; DIFF_LINES],
            prev_len: [0xFF; DIFF_LINES],
            dots: [None; 2],
            ball: None,
            raw_ms: None,
        }
    }
    /// Redraw everything next time.
    pub fn invalidate(&mut self) {
        self.full = true;
    }
    /// The connection changed: every live value changes colour.
    pub fn invalidate_live(&mut self) {
        self.full = true;
    }
    fn changed(&mut self, slot: usize, h: u32) -> bool {
        if slot >= SLOTS {
            return true;
        }
        if self.valid[slot] && self.hash[slot] == h {
            return false;
        }
        self.valid[slot] = true;
        self.hash[slot] = h;
        true
    }
}

/// Mix a few numbers into one hash.
fn mix(parts: &[u32]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for p in parts {
        for b in p.to_le_bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
    }
    h
}

struct Painter<'a, D> {
    t: &'a mut D,
    s: &'a mut Screen,
    /// The raw hex lines may be drawn in this call (see [`RAW_GAP_MS`]).
    raw_due: bool,
}

impl<D: DrawTarget<Color = Rgb565>> Painter<'_, D> {
    fn fill(&mut self, x: i32, y: i32, w: u32, h: u32, c: Rgb565) -> Result<(), D::Error> {
        self.t
            .fill_solid(&Rectangle::new(Point::new(x, y), Size::new(w, h)), c)
    }

    /// Draw text in a slot if it changed, and clear what is left of a
    /// longer old text.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        slot: usize,
        x: i32,
        y: i32,
        font: &MonoFont<'_>,
        fg: Rgb565,
        bg: Rgb565,
        s: &str,
    ) -> Result<(), D::Error> {
        self.line(slot, None, x, y, font, fg, bg, s)
    }

    /// Like `text`, but remembers the letters (in diff line `k`) and draws
    /// again only the stretch from the first to the last changed letter.
    #[allow(clippy::too_many_arguments)]
    fn text_diff(
        &mut self,
        slot: usize,
        k: usize,
        x: i32,
        y: i32,
        font: &MonoFont<'_>,
        fg: Rgb565,
        bg: Rgb565,
        s: &str,
    ) -> Result<(), D::Error> {
        self.line(slot, Some(k), x, y, font, fg, bg, s)
    }

    #[allow(clippy::too_many_arguments)]
    fn line(
        &mut self,
        slot: usize,
        diff: Option<usize>,
        x: i32,
        y: i32,
        font: &MonoFont<'_>,
        fg: Rgb565,
        bg: Rgb565,
        s: &str,
    ) -> Result<(), D::Error> {
        let cw = font.character_size.width + font.character_spacing;
        // Cut the text at the right edge of the screen.
        let fits = ((WIDTH as i32 - x).max(0) as u32 / cw) as usize;
        let mut end = s.len().min(fits);
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        let s = &s[..end];
        // The hash covers the colours and place, not the letters, for diff
        // lines: a colour change redraws the whole line.
        let diff = diff.filter(|k| *k < DIFF_LINES && s.is_ascii() && s.len() <= DIFF_LEN);
        let what = if diff.is_some() { 0 } else { fnv(s.as_bytes()) };
        let h = mix(&[
            what,
            fg.into_storage() as u32,
            bg.into_storage() as u32,
            x as u32,
            y as u32,
        ]);
        let same_look = !self.s.changed(slot, h);
        let style = MonoTextStyleBuilder::new()
            .font(font)
            .text_color(fg)
            .background_color(bg)
            .build();
        match diff {
            Some(k) => {
                let old_len = self.s.prev_len[k];
                let (from, to) = if same_look && old_len != 0xFF {
                    let old = &self.s.prev[k][..old_len as usize];
                    let new = s.as_bytes();
                    let n = old.len().min(new.len());
                    let first = (0..n).find(|&i| old[i] != new[i]).unwrap_or(n);
                    // The last changed letter, among those both texts have.
                    let last = (first..n)
                        .rev()
                        .find(|&i| old[i] != new[i])
                        .map(|i| i + 1)
                        .unwrap_or(first);
                    // Letters past the shorter text count as changed.
                    let to = if new.len() > n { new.len() } else { last };
                    (first, to)
                } else {
                    // Different colours, or never drawn: the whole line.
                    (0, s.len())
                };
                if from < to {
                    let px = x + (from as u32 * cw) as i32;
                    Text::with_baseline(&s[from..to], Point::new(px, y), style, Baseline::Top)
                        .draw(self.t)?;
                }
                self.s.prev[k][..s.len()].copy_from_slice(s.as_bytes());
                self.s.prev_len[k] = s.len() as u8;
            }
            None => {
                if same_look {
                    return Ok(());
                }
                Text::with_baseline(s, Point::new(x, y), style, Baseline::Top).draw(self.t)?;
            }
        }
        let new_w = (s.len() as u32 * cw).min(WIDTH) as u16;
        if slot < SLOTS {
            let old_w = self.s.width[slot];
            if old_w > new_w {
                self.fill(
                    x + new_w as i32,
                    y,
                    (old_w - new_w) as u32,
                    font.character_size.height,
                    bg,
                )?;
            }
            self.s.width[slot] = new_w;
        }
        Ok(())
    }
}

// ----------------------------------------------------------------------
// Picture geometry (page 1). All in screen pixels.

const LSTICK: Point = Point::new(78, 82);
const RSTICK: Point = Point::new(200, 124);
const STICK_R: i32 = 20;
/// How far the dot moves from the centre at full push.
const STICK_TRAVEL: f32 = 13.0;
const FACE: Point = Point::new(242, 82);
const FACE_OFF: i32 = 18;
const FACE_D: u32 = 19;
const DPAD: Point = Point::new(120, 124);
const DPAD_S: u32 = 13;
const LT_BAR: Rectangle = Rectangle::new(Point::new(44, 23), Size::new(70, 9));
const RT_BAR: Rectangle = Rectangle::new(Point::new(206, 23), Size::new(70, 9));
const LB: Rectangle = Rectangle::new(Point::new(44, 33), Size::new(70, 12));
const RB: Rectangle = Rectangle::new(Point::new(206, 33), Size::new(70, 12));
const BODY_RECT: Rectangle = Rectangle::new(Point::new(24, 46), Size::new(272, 110));
const MINUS: Rectangle = Rectangle::new(Point::new(127, 60), Size::new(18, 11));
const PLUS: Rectangle = Rectangle::new(Point::new(175, 60), Size::new(18, 11));
const CAPTURE: Rectangle = Rectangle::new(Point::new(129, 80), Size::new(14, 13));
const HOME: Point = Point::new(184, 86);

/// The motion ball level (Switch layout only), in the lower left grip.
const BALL_GATE: Point = Point::new(61, 128);
const BALL_GATE_D: u32 = 43;
/// How far the ball moves from the centre when the pad lies on a side or
/// stands on its nose. Small enough that the 9x9 box around the ball
/// always stays inside the round gate.
const BALL_TRAVEL: i32 = 13;
/// The ball turns green within this many degrees of level.
const LEVEL_DEG: f32 = 3.0;
/// The gyroscope numbers, in the lower right grip.
const GYRO_X: i32 = 230;
const GYRO_Y: i32 = 113;

/// Lines of numbers under the picture.
const INFO_Y: i32 = 160;

/// The unmapped-input lamp: a "?" and one line of small text along the
/// bottom of the body, below the d-pad and the right stick (and right of
/// the motion ball, left of the gyroscope numbers on a Switch pad).
const LAMP: Point = Point::new(88, 146);
/// Letters of the lamp text (5 pixels each): up to x = 228.
const LAMP_TEXT: usize = 26;
/// "triggers: on/off", between the two trigger bars.
const TRIG_NOTE: Point = Point::new(120, 24);

// Slots, per page (a page change clears them all).
const S_STATUS: usize = 0;
const S_PAGE: usize = 1;
const S_SUB: usize = 2;
const S_BODY: usize = 3;
const S_BTN: usize = 4; // 18 slots, one per Btn
const S_LSTICK: usize = 24;
const S_RSTICK: usize = 25;
const S_LT: usize = 26;
const S_RT: usize = 27;
const S_INFO: usize = 30; // 10 lines
const S_CELLS: usize = 20; // Data page: up to 64 cells
const S_RAW: usize = 90; // Data page: raw lines
const S_ABOUT: usize = 10; // About page lines
const S_MOTION: usize = 112; // Pad page: 6 slots; Data page: 16 lines
const S_TRIG_NOTE: usize = 44; // Pad page
const S_LAMP: usize = 45; // Pad page: lamp and its text
const S_UNMAPPED: usize = 97; // Data page: 3 lines
/// Lines of "last report of each id" on the Data page.
const RAW_IDS_SHOWN: usize = 4;

const CELL_COLS: usize = 4;
const CELL_ROWS: usize = 16;
const CELLS: usize = CELL_COLS * CELL_ROWS;

impl View {
    /// Draw what changed since the last call. Everything on the first call
    /// or after a page change. Cheap when nothing changed.
    pub fn draw<D: DrawTarget<Color = Rgb565>>(&mut self, target: &mut D) -> Result<(), D::Error> {
        let waiting = !self.have_device && !self.live && self.page == Page::Controller;
        let shown = match (waiting, self.page) {
            (true, _) => 0,
            (false, Page::Controller) => 1,
            (false, Page::Data) => 2,
            (false, Page::About) => 3,
        };
        // Take the slot memory out while drawing, so the rest of `self` can
        // be read at the same time; put it back at the end.
        let mut screen = core::mem::replace(&mut self.screen, Screen::new());
        if screen.shown != shown {
            screen.full = true;
            screen.shown = shown;
        }
        // Motion: the gate is offered the newest values here too, so a
        // last small move that came less than 100 ms after the one before
        // still shows once the reports stop.
        self.offer_motion();
        // Unmapped input: a new snapshot at most every 250 ms.
        self.offer_unmapped();
        // Raw hex lines: always on a full redraw or without a clock, else
        // at most every RAW_GAP_MS.
        let raw_due = screen.full
            || match (self.now_ms, screen.raw_ms) {
                (Some(now), Some(at)) => now.wrapping_sub(at) >= RAW_GAP_MS,
                _ => true,
            };
        if raw_due {
            screen.raw_ms = self.now_ms;
        }
        let r = (|| {
            let mut p = Painter {
                t: target,
                s: &mut screen,
                raw_due,
            };
            if p.s.full {
                p.s.full = false;
                p.s.valid = [false; SLOTS];
                p.s.width = [0; SLOTS];
                p.s.prev_len = [0xFF; DIFF_LINES];
                p.s.dots = [None; 2];
                p.s.ball = None;
                p.t.clear(BG)?;
            }
            match shown {
                0 => self.draw_waiting(&mut p),
                1 => self.draw_controller(&mut p),
                2 => self.draw_data(&mut p),
                _ => self.draw_about(&mut p),
            }
        })();
        self.screen = screen;
        r
    }

    fn header<D: DrawTarget<Color = Rgb565>>(
        &self,
        p: &mut Painter<'_, D>,
    ) -> Result<(), D::Error> {
        if p.s.changed(S_BODY + 100, 1) {
            // (slot 103: the header bar, drawn once per page)
            p.fill(0, 0, WIDTH, 11, HEAD_BG)?;
        }
        let mut l: Line<40> = Line::new();
        let _ = write!(l, "{}", self.shown_status());
        p.text(S_STATUS, 2, 1, &FONT_6X10, YELLOW, HEAD_BG, l.as_str())?;
        let tag = match self.page {
            Page::Controller => "1/3 Pad",
            Page::Data => "2/3 Data",
            Page::About => "3/3 About",
        };
        p.text(
            S_PAGE,
            WIDTH as i32 - 2 - 9 * 6,
            1,
            &FONT_6X10,
            TEXT,
            HEAD_BG,
            "         ",
        )?;
        p.text(
            S_PAGE + 100,
            WIDTH as i32 - 2 - tag.len() as i32 * 6,
            1,
            &FONT_6X10,
            TEXT,
            HEAD_BG,
            tag,
        )?;
        Ok(())
    }

    fn draw_waiting<D: DrawTarget<Color = Rgb565>>(
        &self,
        p: &mut Painter<'_, D>,
    ) -> Result<(), D::Error> {
        let s = self.shown_status();
        let s = if s.is_empty() { "Starting" } else { s };
        // Big status, centred, cut to what fits (32 letters of 10 px).
        let n = s.len().min(30);
        let x = (WIDTH as i32 - n as i32 * 10) / 2;
        let mut l: Line<32> = Line::new();
        l.set(&s[..n]);
        // Clear the whole line when the text changes, as it is centred.
        let h = l.hash();
        if p.s.changed(S_STATUS + 50, h) {
            // Centred text moves: clear the whole line, then draw it.
            p.fill(0, 80, WIDTH, 20, BG)?;
            p.s.valid[S_STATUS] = false;
            p.s.width[S_STATUS] = 0;
        }
        p.text(S_STATUS, x.max(0), 80, &FONT_10X20, YELLOW, BG, l.as_str())?;
        let hints = [
            "Put the gamepad in pairing mode.",
            "GuliKit KK2 Pro: hold its pairing",
            "button 2 s, until the lights scroll.",
            "",
            "BOOT button: next page",
        ];
        for (i, h) in hints.iter().enumerate() {
            let x = (WIDTH as i32 - h.len() as i32 * 6) / 2;
            p.text(
                S_STATUS + 1 + i,
                x,
                120 + i as i32 * 12,
                &FONT_6X10,
                if i < 3 { TEXT } else { DIM },
                BG,
                h,
            )?;
        }
        Ok(())
    }

    fn face(&self) -> Face {
        self.layout.face()
    }

    fn draw_controller<D: DrawTarget<Color = Rgb565>>(
        &self,
        p: &mut Painter<'_, D>,
    ) -> Result<(), D::Error> {
        self.header(p)?;
        let live = self.live;
        let face = self.face();

        // Second line: name and layout.
        let mut l: Line<54> = Line::new();
        let name = if self.name.is_empty() {
            "(no name)"
        } else {
            self.name.as_str()
        };
        let _ = write!(
            l,
            "{:.28} [{}{}]",
            name,
            self.layout.name(),
            if self.layout.is_guess() {
                ", guessed"
            } else {
                ""
            }
        );
        p.text(
            S_SUB,
            2,
            12,
            &FONT_6X10,
            if self.layout.is_guess() { YELLOW } else { CYAN },
            BG,
            l.as_str(),
        )?;

        // The body, drawn once.
        if p.s.changed(S_BODY, 1) {
            let style = PrimitiveStyleBuilder::new()
                .fill_color(BODY)
                .stroke_color(BODY_EDGE)
                .stroke_width(2)
                .build();
            RoundedRectangle::with_equal_corners(BODY_RECT, Size::new(44, 44))
                .into_styled(style)
                .draw(p.t)?;
            // Labels next to the trigger bars.
            p.text(
                S_BODY + 70,
                30,
                23,
                &FONT_6X10,
                DIM,
                BG,
                face_pick(face, "LT", "ZL", "L2"),
            )?;
            p.text(
                S_BODY + 71,
                279,
                23,
                &FONT_6X10,
                DIM,
                BG,
                face_pick(face, "RT", "ZR", "R2"),
            )?;
        }

        let on = |b: Btn| live && self.pad.pressed(b);
        // Bumpers.
        for (b, r) in [(Btn::L, LB), (Btn::R, RB)] {
            let pressed = on(b);
            if p.s
                .changed(S_BTN + b as usize, mix(&[pressed as u32, live as u32]))
            {
                let (fill, fg) = colours(pressed, live);
                let style = PrimitiveStyleBuilder::new()
                    .fill_color(fill)
                    .stroke_color(BODY_EDGE)
                    .stroke_width(1)
                    .build();
                RoundedRectangle::with_equal_corners(r, Size::new(6, 6))
                    .into_styled(style)
                    .draw(p.t)?;
                let label = b.name(face);
                let x = r.top_left.x + (r.size.width as i32 - label.len() as i32 * 6) / 2;
                Text::with_baseline(
                    label,
                    Point::new(x, r.top_left.y + 1),
                    text_style(&FONT_6X10, fg, fill),
                    Baseline::Top,
                )
                .draw(p.t)?;
            }
        }
        // Triggers.
        self.trigger(p, S_LT, LT_BAR, self.pad.lt.value, self.pad.lt.present)?;
        self.trigger(p, S_RT, RT_BAR, self.pad.rt.value, self.pad.rt.present)?;
        // Triggers that are only buttons in this mode: say so once.
        let note = if self.triggers_on_off() {
            "triggers: on/off"
        } else {
            ""
        };
        p.text(
            S_TRIG_NOTE,
            TRIG_NOTE.x,
            TRIG_NOTE.y,
            &FONT_5X8,
            DIM,
            BG,
            note,
        )?;

        // Face buttons.
        for (b, dx, dy) in [
            (Btn::North, 0, -FACE_OFF),
            (Btn::South, 0, FACE_OFF),
            (Btn::West, -FACE_OFF, 0),
            (Btn::East, FACE_OFF, 0),
        ] {
            let pressed = on(b);
            if p.s.changed(
                S_BTN + b as usize,
                mix(&[pressed as u32, live as u32, face as u32]),
            ) {
                let (fill, fg) = colours(pressed, live);
                let c = Point::new(FACE.x + dx, FACE.y + dy);
                let style = PrimitiveStyleBuilder::new()
                    .fill_color(fill)
                    .stroke_color(BODY_EDGE)
                    .stroke_width(1)
                    .build();
                Circle::with_center(c, FACE_D)
                    .into_styled(style)
                    .draw(p.t)?;
                let label = face.letter(b);
                Text::with_baseline(
                    label,
                    Point::new(c.x - 2, c.y - 5),
                    text_style(&FONT_6X10, fg, fill),
                    Baseline::Top,
                )
                .draw(p.t)?;
            }
        }

        // D-pad: four squares around a plain centre.
        let s = DPAD_S as i32;
        let half = s / 2;
        if p.s.changed(S_BODY + 60, 1) {
            p.fill(DPAD.x - half, DPAD.y - half, DPAD_S, DPAD_S, BTN_OFF)?;
        }
        for (b, dx, dy, arrow) in [
            (Btn::Up, 0, -s, "^"),
            (Btn::Down, 0, s, "v"),
            (Btn::Left, -s, 0, "<"),
            (Btn::Right, s, 0, ">"),
        ] {
            let pressed = on(b);
            if p.s
                .changed(S_BTN + b as usize, mix(&[pressed as u32, live as u32]))
            {
                let (fill, fg) = colours(pressed, live);
                let x = DPAD.x + dx - half;
                let y = DPAD.y + dy - half;
                p.fill(x, y, DPAD_S, DPAD_S, fill)?;
                Text::with_baseline(
                    arrow,
                    Point::new(x + 4, y + 2),
                    text_style(&FONT_6X10, fg, fill),
                    Baseline::Top,
                )
                .draw(p.t)?;
            }
        }

        // The small middle buttons.
        for (b, label) in [(Btn::Minus, "-"), (Btn::Plus, "+"), (Btn::Capture, "C")] {
            let r = match b {
                Btn::Minus => MINUS,
                Btn::Plus => PLUS,
                _ => CAPTURE,
            };
            let pressed = on(b);
            if p.s
                .changed(S_BTN + b as usize, mix(&[pressed as u32, live as u32]))
            {
                let (fill, fg) = colours(pressed, live);
                let style = PrimitiveStyleBuilder::new()
                    .fill_color(fill)
                    .stroke_color(BODY_EDGE)
                    .stroke_width(1)
                    .build();
                RoundedRectangle::with_equal_corners(r, Size::new(4, 4))
                    .into_styled(style)
                    .draw(p.t)?;
                let x = r.top_left.x + (r.size.width as i32 - 5) / 2;
                let y = r.top_left.y + (r.size.height as i32 - 8) / 2;
                Text::with_baseline(
                    label,
                    Point::new(x, y),
                    text_style(&FONT_5X8, fg, fill),
                    Baseline::Top,
                )
                .draw(p.t)?;
            }
        }
        {
            let pressed = on(Btn::Home);
            if p.s.changed(
                S_BTN + Btn::Home as usize,
                mix(&[pressed as u32, live as u32]),
            ) {
                let (fill, fg) = colours(pressed, live);
                let style = PrimitiveStyleBuilder::new()
                    .fill_color(fill)
                    .stroke_color(BODY_EDGE)
                    .stroke_width(1)
                    .build();
                Circle::with_center(HOME, 15).into_styled(style).draw(p.t)?;
                Text::with_baseline(
                    "H",
                    Point::new(HOME.x - 2, HOME.y - 4),
                    text_style(&FONT_5X8, fg, fill),
                    Baseline::Top,
                )
                .draw(p.t)?;
            }
        }

        // Sticks.
        self.stick(
            p,
            S_LSTICK,
            0,
            LSTICK,
            self.pad.lx,
            self.pad.ly,
            on(Btn::LStick),
        )?;
        self.stick(
            p,
            S_RSTICK,
            1,
            RSTICK,
            self.pad.rx,
            self.pad.ry,
            on(Btn::RStick),
        )?;

        self.lamp(p)?;

        // Motion: only the Switch layout has it. Other layouts draw
        // nothing here, so their page looks as before.
        if self.layout == Layout::SwitchPro {
            self.motion_ball(p)?;
            self.motion_gyro(p)?;
        }

        // Numbers under the picture.
        let fg = if live { TEXT } else { DIM };
        let mut y = INFO_Y;
        for (i, (label, ax, ay)) in [
            ("LS", self.pad.lx, self.pad.ly),
            ("RS", self.pad.rx, self.pad.ry),
        ]
        .into_iter()
        .enumerate()
        {
            let mut l: Line<54> = Line::new();
            if ax.present || ay.present {
                let _ = write!(l, "{} x ", label);
                signed_2dp(&mut l, ax.norm);
                let _ = write!(l, " ({:>6})  y ", ax.raw);
                signed_2dp(&mut l, ay.norm);
                let _ = write!(l, " ({:>6})", ay.raw);
            } else {
                let _ = write!(l, "{} --", label);
            }
            p.text_diff(S_INFO + i, i, 2, y, &FONT_6X10, fg, BG, l.as_str())?;
            y += 10;
        }
        {
            let mut l: Line<54> = Line::new();
            for (name, t) in [
                (face_pick(face, "LT", "ZL", "L2"), self.pad.lt),
                (face_pick(face, "RT", "ZR", "R2"), self.pad.rt),
            ] {
                if !t.present {
                    let _ = write!(l, "{} --          ", name);
                } else if t.analog {
                    let _ = write!(l, "{} {:>4} raw {:>5}  ", name, t.value, t.raw);
                } else {
                    let _ = write!(
                        l,
                        "{} {} (button)   ",
                        name,
                        if t.value > 0 { "on " } else { "off" }
                    );
                }
            }
            if let Some(b) = self.pad.battery {
                let _ = write!(l, "Bat {}", b);
            }
            p.text_diff(S_INFO + 2, 2, 2, y, &FONT_6X10, fg, BG, l.as_str())?;
            y += 10;
        }
        {
            let mut l: Line<54> = Line::new();
            match self.rate_x10 {
                Some(r) => {
                    let _ = write!(l, "{:>3}.{}/s ", r / 10, r % 10);
                }
                None => {
                    let _ = write!(l, " --/s ");
                }
            }
            let _ = write!(l, "total {}", self.reports);
            if self.last_len > 0 {
                let _ = write!(
                    l,
                    "  id {:02X} len {}",
                    self.key_of(&self.last[..1]),
                    self.last_len
                );
            }
            p.text_diff(S_INFO + 3, 3, 2, y, &FONT_6X10, fg, BG, l.as_str())?;
            y += 10;
        }
        {
            // Which buttons are held, by name, and leftovers by number.
            let mut l: Line<54> = Line::new();
            let _ = write!(l, "Held:");
            if live {
                for b in Btn::ALL {
                    if self.pad.pressed(b) {
                        let _ = write!(l, " {}", b.name(face));
                    }
                }
                for n in 0..32 {
                    if self.pad.extra & (1 << n) != 0 {
                        let _ = write!(l, " B{}", n + 1);
                    }
                }
            }
            p.text_diff(
                S_INFO + 4,
                4,
                2,
                y,
                &FONT_6X10,
                if live { YELLOW } else { DIM },
                BG,
                l.as_str(),
            )?;
            y += 11;
        }
        // The last report in hex: 3 lines of 21 bytes in the small font.
        // At most twice a second (see RAW_GAP_MS).
        if !p.raw_due {
            return Ok(());
        }
        let n = self.last_len.min(crate::REPORT_KEEP);
        for k in 0..3 {
            let mut l: Line<64> = Line::new();
            let from = k * 21;
            if from < n {
                hex(&mut l, &self.last[from..n], 21);
            }
            if k == 2 && self.last_len > 63 {
                l.clear();
                hex(&mut l, &self.last[42..n.min(61)], 19);
                let _ = write!(l, " ..");
            }
            p.text_diff(
                S_INFO + 5 + k,
                5 + k,
                2,
                y,
                &FONT_5X8,
                if live { CYAN } else { DIM },
                BG,
                l.as_str(),
            )?;
            y += 8;
        }
        Ok(())
    }

    /// The unmapped-input lamp: a "?" that lights while something the
    /// decoder does not map is on (or was on for a moment since the last
    /// snapshot), with the first such thing by name and "+N" for more.
    fn lamp<D: DrawTarget<Color = Rgb565>>(&self, p: &mut Painter<'_, D>) -> Result<(), D::Error> {
        let (items, total) = self.unmapped_shown();
        let on = self.live && total > 0;
        let mut l: Line<40> = Line::new();
        match items.first() {
            Some(it) if on => {
                it.label(&mut l);
                if total > 1 {
                    let mut more: Line<8> = Line::new();
                    let _ = write!(more, " +{}", total - 1);
                    // Keep the "+N" even when the label is long.
                    let room = LAMP_TEXT.saturating_sub(more.len());
                    if l.len() > room {
                        let cut: Line<40> = l.clone();
                        l.set(&cut.as_str()[..room]);
                    }
                    let _ = l.write_str(more.as_str());
                }
            }
            _ => l.set("unmapped input: none"),
        }
        if p.s.changed(S_LAMP, mix(&[on as u32, self.live as u32])) {
            let (fill, fg) = if on {
                (YELLOW, BG)
            } else {
                (BTN_OFF, if self.live { TEXT } else { DIM })
            };
            p.fill(LAMP.x - 2, LAMP.y - 1, 9, 10, fill)?;
            Text::with_baseline(
                "?",
                Point::new(LAMP.x, LAMP.y),
                text_style(&FONT_5X8, fg, fill),
                Baseline::Top,
            )
            .draw(p.t)?;
        }
        let fg = if on { YELLOW } else { DIM };
        p.text(
            S_LAMP + 1,
            LAMP.x + 10,
            LAMP.y,
            &FONT_5X8,
            fg,
            BODY,
            l.as_str(),
        )?;
        Ok(())
    }

    fn trigger<D: DrawTarget<Color = Rgb565>>(
        &self,
        p: &mut Painter<'_, D>,
        slot: usize,
        r: Rectangle,
        value: u16,
        present: bool,
    ) -> Result<(), D::Error> {
        let inner = r.size.width as i32 - 2;
        let fill_w = if present && self.live {
            (value as i32 * inner + 511) / 1023
        } else {
            0
        };
        if !p.s.changed(
            slot,
            mix(&[fill_w as u32, self.live as u32, present as u32]),
        ) {
            return Ok(());
        }
        r.into_styled(PrimitiveStyle::with_stroke(
            if self.live { BODY_EDGE } else { DIM },
            1,
        ))
        .draw(p.t)?;
        let x = r.top_left.x + 1;
        let y = r.top_left.y + 1;
        let h = r.size.height - 2;
        if fill_w > 0 {
            p.fill(x, y, fill_w as u32, h, CYAN)?;
        }
        if fill_w < inner {
            p.fill(x + fill_w, y, (inner - fill_w) as u32, h, GATE)?;
        }
        Ok(())
    }

    /// A stick: a round gate with a cross, and a dot where the stick is.
    /// When only the position changed, the old dot is painted over and the
    /// new one drawn (a few hundred pixels instead of the whole gate).
    #[allow(clippy::too_many_arguments)]
    fn stick<D: DrawTarget<Color = Rgb565>>(
        &self,
        p: &mut Painter<'_, D>,
        slot: usize,
        which: usize,
        c: Point,
        x: crate::decode::Axis,
        y: crate::decode::Axis,
        clicked: bool,
    ) -> Result<(), D::Error> {
        let present = x.present || y.present;
        let (dx, dy) = if self.live {
            dot_offset(x.norm, y.norm)
        } else {
            (0, 0)
        };
        let dot_at = Point::new(c.x + dx, c.y + dy);
        let look_same = !p.s.changed(
            slot,
            mix(&[clicked as u32, self.live as u32, present as u32]),
        );
        let old = p.s.dots[which];
        if look_same && old == Some(dot_at) {
            return Ok(());
        }
        let d = (STICK_R * 2 + 1) as u32;
        let dot_d = if clicked { 13 } else { 11 };
        match old {
            Some(o) if look_same && !clicked => {
                // Only the dot moved: paint the old one over.
                Circle::with_center(o, dot_d)
                    .into_styled(PrimitiveStyle::with_fill(GATE))
                    .draw(p.t)?;
            }
            _ => {
                p.fill(c.x - STICK_R, c.y - STICK_R, d, d, BODY)?;
                let edge = if clicked {
                    YELLOW
                } else if self.live {
                    BODY_EDGE
                } else {
                    DIM
                };
                let style = PrimitiveStyleBuilder::new()
                    .fill_color(GATE)
                    .stroke_color(edge)
                    .stroke_width(if clicked { 3 } else { 1 })
                    .stroke_alignment(StrokeAlignment::Inside)
                    .build();
                Circle::with_center(c, d).into_styled(style).draw(p.t)?;
            }
        }
        let cross = PrimitiveStyle::with_stroke(BODY, 1);
        GLine::new(
            Point::new(c.x - STICK_R + 3, c.y),
            Point::new(c.x + STICK_R - 3, c.y),
        )
        .into_styled(cross)
        .draw(p.t)?;
        GLine::new(
            Point::new(c.x, c.y - STICK_R + 3),
            Point::new(c.x, c.y + STICK_R - 3),
        )
        .into_styled(cross)
        .draw(p.t)?;
        let dot = if !self.live || !present {
            DIM
        } else if clicked {
            RED
        } else {
            TEXT
        };
        Circle::with_center(dot_at, dot_d)
            .into_styled(PrimitiveStyle::with_fill(dot))
            .draw(p.t)?;
        p.s.dots[which] = Some(dot_at);
        Ok(())
    }

    /// The motion "ball level": a round gate seen from above with a ball
    /// that rolls to the lower side. Flat: the ball sits in the small
    /// square in the middle and turns green. On the right grip: the ball
    /// goes right. Triggers up: the ball comes down (towards you). Face
    /// down: the ball turns red. No motion data: a grey gate, no ball.
    ///
    /// Like the sticks, a move only paints the old ball over and draws the
    /// new one: four small rectangles for the marks and two for the ball.
    fn motion_ball<D: DrawTarget<Color = Rgb565>>(
        &self,
        p: &mut Painter<'_, D>,
    ) -> Result<(), D::Error> {
        let live = self.live;
        let shown = self.motion_gate.shown();
        let c = BALL_GATE;
        let ball = shown.map(|m| {
            let (dx, dy) = m.downhill(BALL_TRAVEL);
            let t = m.tilt();
            let colour = if !live {
                DIM
            } else if t.face_down {
                RED
            } else if t.is_level(LEVEL_DEG) {
                GREEN
            } else {
                YELLOW
            };
            (Point::new(c.x + dx, c.y + dy), colour)
        });
        let look_same =
            !p.s.changed(S_MOTION, mix(&[live as u32, shown.is_some() as u32]));
        if look_same && p.s.ball == ball {
            return Ok(());
        }
        let edge = if live && shown.is_some() {
            BODY_EDGE
        } else {
            DIM
        };
        match p.s.ball {
            Some((old, _)) if look_same => p.fill(old.x - 4, old.y - 4, 9, 9, GATE)?,
            _ => {
                let style = PrimitiveStyleBuilder::new()
                    .fill_color(GATE)
                    .stroke_color(edge)
                    .stroke_width(1)
                    .stroke_alignment(StrokeAlignment::Inside)
                    .build();
                Circle::with_center(c, BALL_GATE_D)
                    .into_styled(style)
                    .draw(p.t)?;
            }
        }
        // A cross, and a small square: "level" is inside it.
        let r = BALL_GATE_D as i32 / 2;
        p.fill(c.x - r + 3, c.y, (2 * r - 5) as u32, 1, BODY)?;
        p.fill(c.x, c.y - r + 3, 1, (2 * r - 5) as u32, BODY)?;
        p.fill(c.x - 6, c.y - 6, 13, 1, edge)?;
        p.fill(c.x - 6, c.y + 6, 13, 1, edge)?;
        p.fill(c.x - 6, c.y - 5, 1, 11, edge)?;
        p.fill(c.x + 6, c.y - 5, 1, 11, edge)?;
        if let Some((at, colour)) = ball {
            // A round-ish ball of 9x9 from two rectangles (two windows on
            // the screen; a drawn circle would take nine).
            p.fill(at.x - 3, at.y - 4, 7, 9, colour)?;
            p.fill(at.x - 4, at.y - 3, 9, 7, colour)?;
        }
        p.s.ball = ball;
        Ok(())
    }

    /// The gyroscope in degrees per second, three short lines in the lower
    /// right grip. "No motion data yet" instead when there is none.
    fn motion_gyro<D: DrawTarget<Color = Rgb565>>(
        &self,
        p: &mut Painter<'_, D>,
    ) -> Result<(), D::Error> {
        let fg = if self.live { TEXT } else { DIM };
        let mut lines: [Line<10>; 4] = [const { Line::new() }; 4];
        match self.motion_gate.shown() {
            Some(m) => {
                lines[0].set("Gyro d/s");
                for (k, (axis, v)) in ["x", "y", "z"].iter().zip(m.gyro_dps()).enumerate() {
                    let _ = write!(lines[k + 1], "{} {:>+6}", axis, round(v));
                }
            }
            None => {
                lines[0].set("No motion");
                lines[1].set("data yet");
            }
        }
        for (k, l) in lines.iter().enumerate() {
            let colour = if k == 0 || self.motion_gate.shown().is_none() {
                DIM
            } else {
                fg
            };
            let y = GYRO_Y + k as i32 * 9;
            if k == 0 {
                p.text(S_MOTION + 1, GYRO_X, y, &FONT_5X8, colour, BODY, l.as_str())?;
            } else {
                p.text_diff(
                    S_MOTION + 1 + k,
                    7 + k,
                    GYRO_X,
                    y,
                    &FONT_5X8,
                    colour,
                    BODY,
                    l.as_str(),
                )?;
            }
        }
        Ok(())
    }

    /// The id of the input report seen most often (the "main" report).
    fn main_id(&self) -> Option<usize> {
        let ids = &self.ids[..self.ids_n];
        let mut best: Option<usize> = None;
        for (i, e) in ids.iter().enumerate() {
            // A Switch pad in full mode: prefer 0x30 over the 0x21 answers.
            let better = match best {
                None => true,
                Some(b) => e.count > ids[b].count,
            };
            if better {
                best = Some(i);
            }
        }
        best
    }

    /// The bytes of the last report with this id: the whole report (up to
    /// REPORT_KEEP bytes) when it was also the very last one, else the
    /// first ID_BYTES bytes kept for its id.
    fn bytes_of<'a>(&'a self, m: &'a crate::IdRaw) -> &'a [u8] {
        if self.last_len > 0 && self.key_of(&self.last[..1]) == m.id {
            &self.last[..self.last_len.min(crate::REPORT_KEEP)]
        } else {
            &m.bytes[..(m.len as usize).min(m.bytes.len())]
        }
    }

    fn draw_data<D: DrawTarget<Color = Rgb565>>(
        &self,
        p: &mut Painter<'_, D>,
    ) -> Result<(), D::Error> {
        self.header(p)?;
        let live = self.live;
        let fg = if live { TEXT } else { DIM };
        let main = self.main_id().map(|i| self.ids[i]);
        // Subtitle.
        let mut l: Line<54> = Line::new();
        match (main, self.layout) {
            (None, _) => {
                let _ = write!(l, "No report yet. Fields from the descriptor:");
            }
            (Some(m), Layout::SwitchPro | Layout::Xbox) => {
                let _ = write!(
                    l,
                    "Report {:02X} ({}B), known layout {}",
                    m.id,
                    m.len,
                    self.layout.name()
                );
            }
            (Some(m), _) => {
                let _ = write!(l, "Report {:02X} ({}B), descriptor fields", m.id, m.len);
            }
        }
        p.text(S_SUB, 2, 12, &FONT_6X10, CYAN, BG, l.as_str())?;

        // The cells. A Switch pad uses only the left two columns: its
        // motion values go on the right.
        let motion_block = self.layout == Layout::SwitchPro;
        let cell_max = if motion_block { 2 * CELL_ROWS } else { CELLS };
        let mut cells: [Line<16>; CELLS] = [const { Line::new() }; CELLS];
        let mut n = 0usize;
        let mut more = 0usize;
        {
            let mut push = |name: &dyn Fn(&mut Line<16>), value: &dyn Fn(&mut Line<16>)| {
                if n < cell_max {
                    let c = &mut cells[n];
                    name(c);
                    while c.len() < 6 {
                        let _ = c.write_char(' ');
                    }
                    value(c);
                    n += 1;
                } else {
                    more += 1;
                }
            };
            match (main, self.layout) {
                (Some(m), Layout::SwitchPro | Layout::Xbox) => {
                    known_cells(self.layout, self.bytes_of(&m), &mut push);
                }
                _ => {
                    let id = main.map(|m| m.id).unwrap_or(0);
                    let id = if self.desc.uses_report_ids { id } else { 0 };
                    let body: &[u8] = match &main {
                        Some(m) => {
                            let all = self.bytes_of(m);
                            if self.desc.uses_report_ids {
                                all.get(1..).unwrap_or(&[])
                            } else {
                                all
                            }
                        }
                        None => &[],
                    };
                    let mut any = false;
                    for f in self.desc.input_fields(id).filter(|f| !f.is_constant()) {
                        any = true;
                        push(
                            &|c| {
                                usage_name(c, f.usage_page, f.usage);
                                if !f.is_variable() {
                                    let _ = c.write_str("[]");
                                } else if f.count > 1 {
                                    let _ = write!(c, "*{}", f.count);
                                }
                            },
                            &|c| {
                                let _ = c.write_char(' ');
                                match f.value(body, 0) {
                                    Some(v) => {
                                        let _ = write!(c, "{}", v);
                                    }
                                    None => {
                                        let _ = c.write_str("-");
                                    }
                                }
                            },
                        );
                    }
                    if !any && self.desc.is_empty() {
                        push(&|c| c.set("(no descriptor)"), &|_| {});
                    }
                }
            }
        }
        if more > 0 {
            let last = &mut cells[cell_max - 1];
            last.clear();
            let _ = write!(last, "+{} more", more + 1);
        }
        if motion_block {
            self.motion_data(p)?;
        }
        for (i, c) in cells[..cell_max].iter().enumerate() {
            let col = (i / CELL_ROWS) as i32;
            let row = (i % CELL_ROWS) as i32;
            p.text(
                S_CELLS + i,
                2 + col * 80,
                24 + row * 9,
                &FONT_5X8,
                fg,
                BG,
                c.as_str(),
            )?;
        }

        // Raw bytes, one line per report id.
        let y0 = 24 + CELL_ROWS as i32 * 9 + 3;
        let mut head: Line<64> = Line::new();
        let _ = head.write_str("Last report of each id (id, length, count):");
        if self.ids_n > RAW_IDS_SHOWN {
            let _ = write!(head, " +{}", self.ids_n - RAW_IDS_SHOWN);
        }
        p.text(S_RAW, 2, y0, &FONT_5X8, DIM, BG, head.as_str())?;
        // What the decoder does not map, under the raw lines. Changed at
        // most 4 times a second (the snapshot in View::offer_unmapped).
        self.unmapped_lines(p, y0 + 9 + RAW_IDS_SHOWN as i32 * 8 + 2)?;
        // At most twice a second (see RAW_GAP_MS).
        if !p.raw_due {
            return Ok(());
        }
        for k in 0..RAW_IDS_SHOWN {
            let mut l: Line<64> = Line::new();
            if k < self.ids_n {
                let e = &self.ids[k];
                let _ = write!(l, "{:02X} {:>3}B {:>5}x ", e.id, e.len, e.count.min(99_999));
                let n = (e.len as usize).min(e.bytes.len());
                hex(&mut l, &e.bytes[1.min(n)..n], 15);
                if n > 16 {
                    let _ = l.write_str("..");
                }
            }
            p.text_diff(
                S_RAW + 1 + k,
                k,
                2,
                y0 + 9 + k as i32 * 8,
                &FONT_5X8,
                if live { CYAN } else { DIM },
                BG,
                l.as_str(),
            )?;
        }
        Ok(())
    }

    /// The Data page's list of unmapped things that are on: a heading
    /// with the count of changes, then two lines of labels ("07 b8.0
    /// vendor FF00:1", "02 b1.0 Play/Pause").
    fn unmapped_lines<D: DrawTarget<Color = Rgb565>>(
        &self,
        p: &mut Painter<'_, D>,
        y: i32,
    ) -> Result<(), D::Error> {
        let (items, total) = self.unmapped_shown();
        let (_, reports) = self.unmapped_changes();
        let on = self.live && total > 0;
        let mut head: Line<64> = Line::new();
        let _ = write!(
            head,
            "Unmapped input on (id, byte.bit, name), {} changes:",
            reports
        );
        p.text(
            S_UNMAPPED,
            2,
            y,
            &FONT_5X8,
            if on { YELLOW } else { DIM },
            BG,
            head.as_str(),
        )?;
        // Pack the labels into two lines of 63 letters.
        let mut lines: [Line<64>; 2] = [const { Line::new() }; 2];
        let mut k = 0usize;
        let mut placed = 0usize;
        if on {
            for it in items {
                let mut one: Line<40> = Line::new();
                it.label(&mut one);
                // Room for "  " before and for a "+N" at the end.
                while k < 2
                    && lines[k].len() + 2 + one.len() > if k == 1 { 58 } else { 63 }
                    && !lines[k].is_empty()
                {
                    k += 1;
                }
                if k >= 2 {
                    break;
                }
                if !lines[k].is_empty() {
                    let _ = lines[k].write_str("  ");
                }
                let _ = lines[k].write_str(one.as_str());
                placed += 1;
            }
            if total > placed {
                let _ = write!(lines[1], "  +{}", total - placed);
            }
        } else {
            lines[0].set(if self.live { "none" } else { "-" });
        }
        for (i, l) in lines.iter().enumerate() {
            p.text(
                S_UNMAPPED + 1 + i,
                2,
                y + 9 + i as i32 * 8,
                &FONT_5X8,
                if on { TEXT } else { DIM },
                BG,
                l.as_str(),
            )?;
        }
        Ok(())
    }

    /// The motion block of the Data page (Switch layout): the newest
    /// sample in g and degrees per second with its raw numbers, and the
    /// tilt. In the two right columns of the cells.
    fn motion_data<D: DrawTarget<Color = Rgb565>>(
        &self,
        p: &mut Painter<'_, D>,
    ) -> Result<(), D::Error> {
        let fg = if self.live { TEXT } else { DIM };
        let mut lines: [Line<32>; CELL_ROWS] = [const { Line::new() }; CELL_ROWS];
        lines[0].set("Motion sensor, newest sample");
        match self.motion_gate_data.shown() {
            Some(m) => {
                lines[1].set("        value      raw");
                let raw = m.newest();
                let g = m.accel_g();
                let d = m.gyro_dps();
                for k in 0..6 {
                    let name = ["Acc X", "Acc Y", "Acc Z", "Gyr X", "Gyr Y", "Gyr Z"][k];
                    let mut v: Line<12> = Line::new();
                    if k < 3 {
                        signed_fixed(&mut v, g[k], 2);
                    } else {
                        signed_fixed(&mut v, d[k - 3], 1);
                    }
                    let unit = if k < 3 { "g" } else { "d/s" };
                    let _ = write!(
                        lines[2 + k],
                        "{:<6}{:>7} {:<3} {:>6}",
                        name,
                        v.as_str(),
                        unit,
                        raw[k]
                    );
                }
                let t = m.tilt();
                lines[9].set("Tilt, from gravity alone:");
                let _ = lines[10].write_str("roll  ");
                signed_fixed(&mut lines[10], t.roll_deg, 1);
                let _ = lines[10].write_str(" deg");
                let _ = lines[11].write_str("pitch ");
                signed_fixed(&mut lines[11], t.pitch_deg, 1);
                let _ = lines[11].write_str(" deg");
                if t.face_down {
                    lines[12].set("(lying face down)");
                }
                lines[14].set("4096 = 1 g; 1 = 0.07 deg/s");
                lines[15].set("Redrawn 4x/s at most");
            }
            None => {
                lines[2].set("No motion data yet.");
                lines[3].set("All zeros: the sensor is off");
                lines[4].set("(it needs subcommand 40 01)");
                lines[5].set("or the pad has none.");
            }
        }
        let x = 2 + 2 * 80;
        for (k, l) in lines.iter().enumerate() {
            let y = 24 + k as i32 * 9;
            let colour = match k {
                0 => CYAN,
                1 | 9 | 14 | 15 => DIM,
                _ => fg,
            };
            if (2..=7).contains(&k) || (10..=12).contains(&k) {
                // Values that change often: only the changed letters
                // (diff lines 6-11 and 12-14; 0-5 are the raw lines).
                let diff = if k <= 7 { k + 4 } else { k + 2 };
                p.text_diff(S_MOTION + k, diff, x, y, &FONT_5X8, colour, BG, l.as_str())?;
            } else {
                p.text(S_MOTION + k, x, y, &FONT_5X8, colour, BG, l.as_str())?;
            }
        }
        Ok(())
    }

    fn draw_about<D: DrawTarget<Color = Rgb565>>(
        &self,
        p: &mut Painter<'_, D>,
    ) -> Result<(), D::Error> {
        self.header(p)?;
        let mut lines: [Line<54>; 20] = [const { Line::new() }; 20];
        let mut i = 0usize;
        let mut next = |f: &mut dyn FnMut(&mut Line<54>)| {
            if i < lines.len() {
                f(&mut lines[i]);
                i += 1;
            }
        };
        next(&mut |l| {
            let _ = write!(
                l,
                "Name: {}",
                if self.name.is_empty() {
                    "(none)"
                } else {
                    self.name.as_str()
                }
            );
        });
        next(&mut |l| {
            let _ = write!(l, "Class {:06X}: ", self.class_of_device);
            class_words(l, self.class_of_device);
        });
        next(&mut |l| match (self.vendor_id, self.product_id) {
            (Some(v), Some(pid)) => {
                let _ = write!(l, "Vendor {:04X} product {:04X}", v, pid);
                if let Some(ver) = self.version {
                    let _ = write!(l, " version {:04X}", ver);
                }
                match self.vendor_id_source {
                    Some(1) => {
                        let _ = l.write_str(" (BT)");
                    }
                    Some(2) => {
                        let _ = l.write_str(" (USB)");
                    }
                    _ => {}
                }
            }
            _ => {
                let _ = write!(l, "Vendor/product: no Device ID record yet");
            }
        });
        next(&mut |l| {
            let _ = write!(
                l,
                "Layout: {} ({})",
                self.layout.name(),
                if self.layout.is_guess() {
                    "guessed"
                } else {
                    "known"
                }
            );
        });
        next(&mut |l| {
            let _ = write!(l, "  why: {}", self.reason);
        });
        next(&mut |l| {
            let _ = write!(l, "GuliKit mode: {}", self.mode_guess());
        });
        let d = &self.desc;
        next(&mut |l| {
            if self.desc_len == 0 {
                let _ = write!(l, "Descriptor: not read");
            } else {
                let _ = write!(
                    l,
                    "Descriptor: {} bytes, {} fields",
                    self.desc_len,
                    d.fields().len()
                );
                if d.damaged {
                    let _ = l.write_str(", damaged");
                }
                if d.dropped > 0 {
                    let _ = write!(l, ", {} dropped", d.dropped);
                }
            }
        });
        next(&mut |l| {
            let (b, a, h) = d.summary();
            let _ = write!(l, "Declares: buttons {}, axes {}, hats {}", b, a, h);
        });
        for (kind, label) in [
            (Kind::Input, "In: "),
            (Kind::Output, "Out:"),
            (Kind::Feature, "Feat:"),
        ] {
            next(&mut |l| {
                let _ = l.write_str(label);
                let mut count = 0;
                // 53 letters fit on a line; keep 3 for " ..".
                for r in d.reports().iter().filter(|r| r.kind == kind) {
                    let mut item: Line<16> = Line::new();
                    let _ = write!(item, " {:02X}:{}B", r.id, r.bytes(d.uses_report_ids));
                    if l.len() + item.len() > 50 {
                        let _ = l.write_str(" ..");
                        break;
                    }
                    let _ = l.write_str(item.as_str());
                    count += 1;
                }
                if count == 0 {
                    let _ = l.write_str(" -");
                }
            });
        }
        let head = self.desc_len.min(self.desc_head.len());
        next(&mut |l| {
            let _ = l.write_str("Start: ");
            hex(l, &self.desc_head[..head], 15);
        });
        next(&mut |l| {
            if head > 15 {
                let _ = l.write_str("       ");
                hex(l, &self.desc_head[15..head], 15);
            }
        });
        if self.startup.active {
            let st = &self.startup;
            // Three lines: how the probe went, the steps, the pad's info.
            next(&mut |l| {
                let _ = l.write_str("Start-up: ");
                match st.phase() {
                    Phase::Waiting => {
                        let _ = l.write_str(if st.step(0x03) == StepState::ByReports {
                            "0x30 came unasked; not started"
                        } else {
                            "waiting to start"
                        });
                    }
                    Phase::Probing(v) => {
                        let _ = write!(
                            l,
                            "trying {} ({}/{}), try {}",
                            VARIANTS[v].name,
                            v + 1,
                            VARIANTS.len(),
                            st.probing().map_or(0, |p| p.1)
                        );
                    }
                    Phase::Steps(_) | Phase::Finished => {
                        let _ = write!(l, "pad answered {}", st.won().map_or("?", |v| v.name));
                    }
                    Phase::AllFailed => {
                        let _ = write!(l, "no answer to {} ways; 3F only", VARIANTS.len());
                    }
                }
            });
            next(&mut |l| {
                let _ = l.write_str("Steps:");
                for (k, (id, arg, _)) in STEPS.iter().enumerate() {
                    let _ = write!(
                        l,
                        " {:02X} {:02X} {}",
                        id,
                        arg,
                        match st.steps()[k] {
                            StepState::NotSent => "-",
                            StepState::Sent(_) => "wait",
                            StepState::Answered(_) | StepState::ByReports => "ok",
                            StepState::NoAnswer => "no",
                        }
                    );
                    if k + 1 < STEPS.len() {
                        let _ = l.write_str(",");
                    }
                }
            });
            next(&mut |l| {
                match st.device_info {
                    Some(d) => {
                        let _ = write!(
                            l,
                            "Pad info: firmware {}.{:02}, type {}",
                            d.firmware.0, d.firmware.1, d.kind
                        );
                    }
                    None => {
                        let _ = l.write_str("Pad info: not answered");
                    }
                }
                if let Some(h) = st.handshake {
                    let _ = write!(l, "; handshake {}", h);
                }
            });
        } else {
            next(&mut |l| {
                let _ = l.write_str("Start-up conversation: none needed");
            });
        }
        // The motion sensor: which mode has one, and whether the pad
        // answered the "motion sensor on" subcommand (0x40).
        if self.layout == Layout::SwitchPro {
            let step = Some(self.startup.step(0x40));
            let have = self.motion_gate.shown().is_some();
            next(&mut |l| {
                let _ = l.write_str(match (have, step) {
                    (true, _) => "Motion sensor: yes (accelerometer + gyroscope,",
                    (false, Some(StepState::NotSent) | None) => {
                        "Motion sensor: not asked yet, no data"
                    }
                    (false, _) => "Motion sensor: asked, no data yet",
                });
            });
            next(&mut |l| {
                if have {
                    let _ = l.write_str("  Switch report 0x30); ");
                } else {
                    let _ = l.write_str("  ");
                }
                let _ = l.write_str("pad answered 40 01: ");
                let _ = l.write_str(match step {
                    Some(StepState::Answered(_) | StepState::ByReports) => "yes",
                    Some(StepState::Sent(_)) => "waiting",
                    Some(StepState::NoAnswer) => "no",
                    Some(StepState::NotSent) | None => "not sent yet",
                });
            });
        } else {
            // Pads of other kinds may carry motion in their own bytes (a
            // DualShock 4 does); pad-view does not read it, and says so.
            // A Sony pad (DualShock 4, DualSense) sends motion in vendor
            // bytes its descriptor does not name; the HID sensor page
            // (0x20) is the standard way to declare one.
            let sony = self.vendor_id == Some(0x054C);
            let sensors = d
                .fields()
                .iter()
                .any(|f| f.kind == Kind::Input && f.usage_page == 0x20);
            next(&mut |l| {
                let _ = l.write_str(if sony {
                    "Motion sensor: Sony pads have one; not read here"
                } else if sensors {
                    "Motion sensor: listed in descriptor, not read"
                } else if self.layout == Layout::Xbox {
                    // The Xbox descriptor declares none, and SDL reads none.
                    "Motion sensor: not in this mode"
                } else {
                    // A pad decoded from its descriptor (DInput mode, for
                    // example): nothing is known for sure.
                    "Motion sensor: none found"
                });
            });
        }
        next(&mut |l| {
            let _ = write!(
                l,
                "Reports: {} total, {} understood",
                self.reports, self.decoded
            );
        });
        if self.layout == Layout::Xbox {
            next(&mut |l| crate::battery::line(l, self.pad.battery, self.battery_ask));
        }
        if !self.pairings.as_str().is_empty() {
            next(&mut |l| {
                let _ = l.write_str(self.pairings.as_str());
            });
        }
        for (k, line) in lines.iter().enumerate() {
            let fg = if k == 0 { YELLOW } else { TEXT };
            p.text(
                S_ABOUT + k,
                2,
                13 + k as i32 * 11,
                &FONT_6X10,
                fg,
                BG,
                line.as_str(),
            )?;
        }
        Ok(())
    }
}

/// Where the stick dot goes, in pixels from the gate centre. The two axes
/// are each -1..1, so the corners would be outside the round gate: the
/// offset is pulled back onto a circle of STICK_TRAVEL pixels.
fn dot_offset(nx: f32, ny: f32) -> (i32, i32) {
    let dx = (nx * STICK_TRAVEL) as i32;
    let dy = (ny * STICK_TRAVEL) as i32;
    let r = STICK_TRAVEL as i32;
    let len2 = dx * dx + dy * dy;
    if len2 <= r * r {
        return (dx, dy);
    }
    let len = isqrt(len2 as u32) as i32;
    (dx * r / len.max(1), dy * r / len.max(1))
}

/// Round to the nearest whole number (halves away from zero).
fn round(v: f32) -> i32 {
    if v >= 0.0 {
        (v + 0.5) as i32
    } else {
        (v - 0.5) as i32
    }
}

/// Whole-number square root (core has no f32::sqrt without std).
fn isqrt(v: u32) -> u32 {
    let mut x = v;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}

fn face_pick(face: Face, x: &'static str, n: &'static str, p: &'static str) -> &'static str {
    match face {
        Face::Xbox => x,
        Face::Nintendo => n,
        Face::PlayStation => p,
    }
}

/// Fill and letter colour of a button.
fn colours(pressed: bool, live: bool) -> (Rgb565, Rgb565) {
    if pressed {
        (YELLOW, BG)
    } else if live {
        (BTN_OFF, TEXT)
    } else {
        (GATE, DIM)
    }
}

fn text_style<'f>(
    font: &'f MonoFont<'f>,
    fg: Rgb565,
    bg: Rgb565,
) -> embedded_graphics::mono_font::MonoTextStyle<'f, Rgb565> {
    MonoTextStyleBuilder::new()
        .font(font)
        .text_color(fg)
        .background_color(bg)
        .build()
}

/// Adds one cell to the Data page: one closure writes the name, the other
/// the value.
type CellPush<'a> = dyn FnMut(&dyn Fn(&mut Line<16>), &dyn Fn(&mut Line<16>)) + 'a;

/// The named values of a known layout, for the Data page.
fn known_cells(layout: Layout, r: &[u8], push: &mut CellPush<'_>) {
    let b = |i: usize| r.get(i).copied();
    let w = |i: usize| {
        Some(u16::from_le_bytes([
            r.get(i).copied()?,
            r.get(i + 1).copied()?,
        ]))
    };
    let mut put = |name: &str, v: Option<i32>, hexv: bool| {
        push(&|c| c.set(name), &|c| {
            let _ = c.write_char(' ');
            match v {
                Some(v) if hexv => {
                    let _ = write!(c, "{:02X}", v);
                }
                Some(v) => {
                    let _ = write!(c, "{}", v);
                }
                None => {
                    let _ = c.write_str("-");
                }
            }
        })
    };
    match (layout, r.first()) {
        (Layout::SwitchPro, Some(0x3F)) => {
            put("Btn1", b(1).map(i32::from), true);
            put("Btn2", b(2).map(i32::from), true);
            put("Hat", b(3).map(i32::from), false);
            put("LX", w(4).map(i32::from), false);
            put("LY", w(6).map(i32::from), false);
            put("RX", w(8).map(i32::from), false);
            put("RY", w(10).map(i32::from), false);
        }
        (Layout::SwitchPro, Some(id)) => {
            put("Timer", b(1).map(i32::from), false);
            put("Batt", b(2).map(|v| (v >> 4) as i32), false);
            put("Conn", b(2).map(|v| (v & 0xF) as i32), false);
            put("BtnR", b(3).map(i32::from), true);
            put("BtnMid", b(4).map(i32::from), true);
            put("BtnL", b(5).map(i32::from), true);
            if r.len() >= 12 {
                let (lx, ly) = crate::decode::twelve(&r[6..9]);
                let (rx, ry) = crate::decode::twelve(&r[9..12]);
                put("LX", Some(lx), false);
                put("LY", Some(ly), false);
                put("RX", Some(rx), false);
                put("RY", Some(ry), false);
            }
            put("Vib", b(12).map(i32::from), true);
            if *id == 0x21 {
                put("Ack", b(13).map(i32::from), true);
                put("SubId", b(14).map(i32::from), true);
            }
            // The motion samples (bytes 13-48) have a block of their own
            // on the right.
        }
        (Layout::Xbox, Some(0x01)) => {
            put("LX", w(1).map(i32::from), false);
            put("LY", w(3).map(i32::from), false);
            put("RX", w(5).map(i32::from), false);
            put("RY", w(7).map(i32::from), false);
            put("LT", w(9).map(i32::from), false);
            put("RT", w(11).map(i32::from), false);
            put("Hat", b(13).map(i32::from), false);
            put("Btn1", b(14).map(i32::from), true);
            put("Btn2", b(15).map(i32::from), true);
            if r.len() > 16 {
                put("Btn3", b(16).map(i32::from), true);
            }
        }
        (_, Some(_)) => {
            for (i, v) in r.iter().enumerate().skip(1).take(40) {
                let mut name: Line<16> = Line::new();
                let _ = write!(name, "Byte{}", i);
                put(name.as_str(), Some(*v as i32), true);
            }
        }
        _ => {}
    }
}
