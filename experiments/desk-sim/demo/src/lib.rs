//! A small console program that proves every input reaches the console.
//!
//! It only talks to the board layer ([`board_layer::Board`]), never to
//! hardware, so the same code runs in the PC simulator and (later) on the
//! ESP32. It draws with embedded-graphics, the drawing library the
//! firmware experiments already use.
//!
//! Scenes:
//! * Start: a title. START, A or a tap goes on.
//! * Play: a little character moved by the d-pad or the left stick, a
//!   mark where the screen is touched, a lamp for each button, the two
//!   sticks and triggers, the frame number and the time.
//! * Two test pictures from `crates/test-card`, drawn by that crate's own
//!   code, unchanged. SELECT steps Play, panel card, framebuffer card,
//!   Play. HOME goes back to Start from anywhere.

#![no_std]

use core::fmt::{self, Write};

use board_layer::{Board, Pad, SCREEN_H, SCREEN_W, Stick, Tick, Touch, button, rgb565};
use embedded_graphics::{
    mono_font::{
        MonoTextStyle,
        ascii::{FONT_6X10, FONT_10X20},
    },
    pixelcolor::{Rgb565, raw::RawU16},
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use test_card::{FB_H, FB_W, Fb, PALETTE, PanelBuf, draw_fb_card, draw_panel_card};

/// Colours, as RGB565 numbers. Public so tests can look for them.
pub mod colour {
    use super::rgb565;
    pub const BACKGROUND: u16 = rgb565(16, 20, 40);
    pub const START_BACKGROUND: u16 = rgb565(0, 0, 96);
    pub const TEXT: u16 = rgb565(255, 255, 255);
    pub const FRAME_LINE: u16 = rgb565(100, 100, 120);
    pub const LAMP_OFF: u16 = rgb565(60, 60, 70);
    pub const LAMP_ON: u16 = rgb565(255, 96, 32);
    pub const STICK_DOT: u16 = rgb565(0, 255, 255);
    pub const TRIGGER: u16 = rgb565(255, 0, 255);
    pub const TOUCH_MARK: u16 = rgb565(255, 255, 0);
    pub const HERO_BODY: u16 = rgb565(80, 220, 100);
    pub const HERO_EYE: u16 = rgb565(255, 255, 255);
    pub const HERO_PUPIL: u16 = rgb565(0, 0, 0);
}

/// Where things are on the Play screen. Public so tests can look there.
pub mod layout {
    /// The play field: the character stays inside it.
    pub const FIELD: (i32, i32, i32, i32) = (0, 14, 320, 156); // x, y, width, height
    /// The character is 8x8 art pixels, each drawn 3x3 screen pixels.
    pub const HERO_SCALE: i32 = 3;
    pub const HERO_SIZE: i32 = 8 * HERO_SCALE;
    /// Where the character starts (top left corner).
    pub const HERO_START: (i32, i32) = (148, 80);
    /// The row of button lamps: the middle of each lamp.
    pub const LAMP_Y: i32 = 180;
    pub const fn lamp_x(index: usize) -> i32 {
        12 + 24 * index as i32
    }
    /// The stick boxes (40x40, top left corner) and trigger bars (8x40).
    pub const LEFT_STICK_BOX: (i32, i32) = (8, 198);
    pub const LEFT_TRIGGER_BAR: (i32, i32) = (52, 198);
    pub const RIGHT_TRIGGER_BAR: (i32, i32) = (260, 198);
    pub const RIGHT_STICK_BOX: (i32, i32) = (272, 198);
    pub const BOX: i32 = 40;
}

/// Short labels under the lamps, in the order of `button::NAMES`.
const LAMP_LABELS: [&str; 13] = [
    "^", "v", "<", ">", "A", "B", "X", "Y", "L", "R", "St", "Se", "H",
];

/// The character, one letter per art pixel: `X` body, `o` eye, `k` pupil,
/// `.` nothing (the background shows through).
const HERO_ART: [&[u8; 8]; 8] = [
    b"...XX...", //
    b"..XXXX..",
    b".XooXooX",
    b".XkoXkoX",
    b"XXXXXXXX",
    b"XX.XX.XX",
    b"X..XX..X",
    b"..X..X..",
];

/// Sticks closer to the middle than this count as "not pushed". Real
/// sticks never rest exactly at 0.
const DEAD_ZONE: i32 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Start,
    Play,
    PanelCard,
    FbCard,
}

impl Scene {
    fn name(self) -> &'static str {
        match self {
            Scene::Start => "start screen",
            Scene::Play => "play",
            Scene::PanelCard => "panel test card",
            Scene::FbCard => "framebuffer test card",
        }
    }
}

/// The demo's state. Public fields so tests can check what it saw.
pub struct Demo {
    pub scene: Scene,
    /// Top left corner of the character, in screen pixels.
    pub hero: (i32, i32),
    /// The input of the previous frame, to see what changed.
    last_pad: Pad,
    last_touch: Touch,
    /// The 160x120 framebuffer picture, before it is doubled.
    fb: [u8; FB_W * FB_H],
}

impl Default for Demo {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs the demo until the board says stop. This is the whole console
/// main loop.
pub fn run<B: Board>(board: &mut B) {
    let mut demo = Demo::new();
    board.log("demo: start screen");
    while let Some(tick) = board.wait_frame() {
        demo.frame(board, tick);
    }
    board.log("demo: stopped");
}

impl Demo {
    pub const fn new() -> Self {
        Self {
            scene: Scene::Start,
            hero: layout::HERO_START,
            last_pad: Pad {
                connected: false,
                buttons: 0,
                left: Stick { x: 0, y: 0 },
                right: Stick { x: 0, y: 0 },
                left_trigger: 0,
                right_trigger: 0,
            },
            last_touch: Touch {
                pressed: false,
                x: 0,
                y: 0,
            },
            fb: [0; FB_W * FB_H],
        }
    }

    /// One frame: read the input, change the state, draw, present.
    pub fn frame<B: Board>(&mut self, board: &mut B, tick: Tick) {
        let pad = board.pad();
        let touch = board.touch();
        self.log_changes(board, &pad, &touch);

        // Buttons that went down this frame (not the ones still held).
        let went_down = pad.buttons & !self.last_pad.buttons;
        let tapped = touch.pressed && !self.last_touch.pressed;
        let next = match self.scene {
            _ if went_down & button::HOME != 0 => Scene::Start,
            Scene::Start if went_down & (button::START | button::A) != 0 || tapped => Scene::Play,
            Scene::Play if went_down & button::SELECT != 0 => Scene::PanelCard,
            Scene::PanelCard if went_down & button::SELECT != 0 => Scene::FbCard,
            Scene::FbCard if went_down & button::SELECT != 0 => Scene::Play,
            same => same,
        };
        if next != self.scene {
            self.scene = next;
            log(board, format_args!("demo: {}", next.name()));
        }

        if self.scene == Scene::Play {
            self.move_hero(&pad);
        }

        let screen = board.screen();
        match self.scene {
            Scene::Start => draw_start(screen, tick),
            Scene::Play => self.draw_play(screen, tick, &pad, &touch),
            // The test card's own code, unchanged.
            Scene::PanelCard => {
                let _ = draw_panel_card(&mut PanelBuf::new(screen));
            }
            Scene::FbCard => {
                let _ = draw_fb_card(&mut Fb::new(&mut self.fb));
                double_through_palette(&self.fb, screen);
            }
        }
        board.present();

        self.last_pad = pad;
        self.last_touch = touch;
    }

    /// Writes a log line for every button and touch that changed.
    fn log_changes<B: Board>(&self, board: &mut B, pad: &Pad, touch: &Touch) {
        let changed = pad.buttons ^ self.last_pad.buttons;
        for &(name, bit) in button::NAMES.iter() {
            if changed & bit != 0 {
                let what = if pad.held(bit) { "down" } else { "up" };
                log(board, format_args!("pad: {name} {what}"));
            }
        }
        if touch.pressed != self.last_touch.pressed {
            let what = if touch.pressed { "down" } else { "up" };
            log(
                board,
                format_args!("touch: {what} at {},{}", touch.x, touch.y),
            );
        }
    }

    fn move_hero(&mut self, pad: &Pad) {
        // The d-pad moves 2 pixels a frame; the stick up to 4.
        let mut dx = 0;
        let mut dy = 0;
        if pad.held(button::LEFT) {
            dx -= 2;
        }
        if pad.held(button::RIGHT) {
            dx += 2;
        }
        if pad.held(button::UP) {
            dy -= 2;
        }
        if pad.held(button::DOWN) {
            dy += 2;
        }
        let (sx, sy) = (pad.left.x as i32, pad.left.y as i32);
        if sx.abs() > DEAD_ZONE {
            dx += sx / 32;
        }
        if sy.abs() > DEAD_ZONE {
            dy += sy / 32;
        }
        let (fx, fy, fw, fh) = layout::FIELD;
        let size = layout::HERO_SIZE;
        self.hero.0 = (self.hero.0 + dx).clamp(fx + 2, fx + fw - 2 - size);
        self.hero.1 = (self.hero.1 + dy).clamp(fy + 2, fy + fh - 2 - size);
    }

    fn draw_play(&self, screen: &mut [u16], tick: Tick, pad: &Pad, touch: &Touch) {
        let mut target = PanelBuf::new(screen);
        let t = &mut target;
        fill(
            t,
            0,
            0,
            SCREEN_W as i32,
            SCREEN_H as i32,
            colour::BACKGROUND,
        );

        // Top line: frame number and time.
        let secs = tick.now_us / 1_000_000;
        let millis = (tick.now_us / 1000) % 1000;
        text(
            t,
            4,
            2,
            format_args!("frame {:6}   time {secs:4}.{millis:03} s", tick.frame),
        );

        // The play field and the character.
        let (fx, fy, fw, fh) = layout::FIELD;
        outline(t, fx, fy, fw, fh, colour::FRAME_LINE);
        draw_hero(t, self.hero.0, self.hero.1, layout::HERO_SCALE);

        // One lamp per button.
        for (i, &(_, bit)) in button::NAMES.iter().enumerate() {
            let x = layout::lamp_x(i);
            let lit = if pad.held(bit) {
                colour::LAMP_ON
            } else {
                colour::LAMP_OFF
            };
            let _ = Circle::with_center(Point::new(x, layout::LAMP_Y), 15)
                .into_styled(PrimitiveStyle::with_fill(c(lit)))
                .draw(t);
            let style = MonoTextStyle::new(&FONT_6X10, c(colour::TEXT));
            let centred = TextStyleBuilder::new()
                .alignment(Alignment::Center)
                .baseline(Baseline::Top)
                .build();
            let _ = Text::with_text_style(
                LAMP_LABELS[i],
                Point::new(x, layout::LAMP_Y + 8),
                style,
                centred,
            )
            .draw(t);
        }

        // Sticks and triggers.
        draw_stick(t, layout::LEFT_STICK_BOX, pad.left);
        draw_trigger(t, layout::LEFT_TRIGGER_BAR, pad.left_trigger);
        draw_trigger(t, layout::RIGHT_TRIGGER_BAR, pad.right_trigger);
        draw_stick(t, layout::RIGHT_STICK_BOX, pad.right);

        // The words in the middle.
        let state = if touch.pressed { "down" } else { "up" };
        text(
            t,
            72,
            200,
            format_args!("touch {:3},{:3} {state}", touch.x, touch.y),
        );
        text(t, 72, 212, format_args!("SELECT: test card"));
        text(t, 72, 224, format_args!("HOME: start screen"));

        // Last, so it is on top: the mark where the finger is.
        if touch.pressed {
            let (x, y) = (touch.x as i32, touch.y as i32);
            let line = PrimitiveStyle::with_stroke(c(colour::TOUCH_MARK), 1);
            let _ = Line::new(Point::new(x - 8, y), Point::new(x + 8, y))
                .into_styled(line)
                .draw(t);
            let _ = Line::new(Point::new(x, y - 8), Point::new(x, y + 8))
                .into_styled(line)
                .draw(t);
            let _ = Circle::with_center(Point::new(x, y), 11)
                .into_styled(line)
                .draw(t);
        }
    }
}

/// The middle pixel of a stick's dot, for a stick in a box at `corner`.
pub const fn stick_dot(corner: (i32, i32), stick: Stick) -> (i32, i32) {
    let half = layout::BOX / 2;
    (
        corner.0 + half + stick.x as i32 * 16 / 128,
        corner.1 + half + stick.y as i32 * 16 / 128,
    )
}

/// How many pixels of a trigger bar are filled, from the bottom up.
pub const fn trigger_fill(pull: u8) -> i32 {
    pull as i32 * (layout::BOX - 2) / 255
}

fn draw_start(screen: &mut [u16], tick: Tick) {
    let mut target = PanelBuf::new(screen);
    let t = &mut target;
    fill(
        t,
        0,
        0,
        SCREEN_W as i32,
        SCREEN_H as i32,
        colour::START_BACKGROUND,
    );
    let centred = TextStyleBuilder::new()
        .alignment(Alignment::Center)
        .baseline(Baseline::Top)
        .build();
    let big = MonoTextStyle::new(&FONT_10X20, c(colour::TEXT));
    let small = MonoTextStyle::new(&FONT_6X10, c(colour::TEXT));
    let _ = Text::with_text_style("DESK SIM", Point::new(160, 40), big, centred).draw(t);
    let _ = Text::with_text_style(
        "console demo, no hardware",
        Point::new(160, 66),
        small,
        centred,
    )
    .draw(t);
    draw_hero(t, 160 - 24, 96, 6);
    // Blinks: half a second on, half a second off. Counted in time, not
    // in frames, so it blinks the same at 30 and at 60 frames a second.
    if (tick.now_us / 500_000).is_multiple_of(2) {
        let _ = Text::with_text_style("press START or tap", Point::new(160, 170), small, centred)
            .draw(t);
    }
}

fn draw_hero(t: &mut PanelBuf, x: i32, y: i32, scale: i32) {
    for (row, line) in HERO_ART.iter().enumerate() {
        for (col, &letter) in line.iter().enumerate() {
            let colour = match letter {
                b'X' => colour::HERO_BODY,
                b'o' => colour::HERO_EYE,
                b'k' => colour::HERO_PUPIL,
                _ => continue,
            };
            fill(
                t,
                x + col as i32 * scale,
                y + row as i32 * scale,
                scale,
                scale,
                colour,
            );
        }
    }
}

fn draw_stick(t: &mut PanelBuf, corner: (i32, i32), stick: Stick) {
    outline(
        t,
        corner.0,
        corner.1,
        layout::BOX,
        layout::BOX,
        colour::FRAME_LINE,
    );
    let (x, y) = stick_dot(corner, stick);
    fill(t, x - 3, y - 3, 7, 7, colour::STICK_DOT);
}

fn draw_trigger(t: &mut PanelBuf, corner: (i32, i32), pull: u8) {
    outline(t, corner.0, corner.1, 8, layout::BOX, colour::FRAME_LINE);
    let h = trigger_fill(pull);
    fill(
        t,
        corner.0 + 1,
        corner.1 + layout::BOX - 1 - h,
        6,
        h,
        colour::TRIGGER,
    );
}

/// Draws the 160x120 picture at twice the size: each palette number
/// becomes a 2x2 block of its RGB565 colour. This is how the console's
/// planned 160x120 screen would reach the 320x240 panel.
fn double_through_palette(fb: &[u8], screen: &mut [u16]) {
    for (i, pixel) in screen.iter_mut().enumerate() {
        let (x, y) = (i % SCREEN_W, i / SCREEN_W);
        *pixel = PALETTE[fb[(y / 2) * FB_W + x / 2] as usize];
    }
}

// ---- small drawing helpers ----
// Drawing into memory cannot fail, so the results are ignored.

fn c(raw: u16) -> Rgb565 {
    Rgb565::from(RawU16::new(raw))
}

fn fill(t: &mut PanelBuf, x: i32, y: i32, w: i32, h: i32, colour: u16) {
    if w > 0 && h > 0 {
        let _ = t.fill_solid(
            &Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32)),
            c(colour),
        );
    }
}

fn outline(t: &mut PanelBuf, x: i32, y: i32, w: i32, h: i32, colour: u16) {
    let _ = Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32))
        .into_styled(PrimitiveStyle::with_stroke(c(colour), 1))
        .draw(t);
}

fn text(t: &mut PanelBuf, x: i32, y: i32, args: fmt::Arguments) {
    let line = Line64::from(args);
    let style = MonoTextStyle::new(&FONT_6X10, c(colour::TEXT));
    let _ = Text::with_baseline(line.as_str(), Point::new(x, y), style, Baseline::Top).draw(t);
}

fn log<B: Board>(board: &mut B, args: fmt::Arguments) {
    board.log(Line64::from(args).as_str());
}

/// A line of up to 64 bytes of text, made without a heap (there is no
/// heap on the board). Longer text is cut off.
struct Line64 {
    bytes: [u8; 64],
    len: usize,
}

impl Line64 {
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("?")
    }
}

impl From<fmt::Arguments<'_>> for Line64 {
    fn from(args: fmt::Arguments) -> Self {
        let mut line = Line64 {
            bytes: [0; 64],
            len: 0,
        };
        let _ = line.write_fmt(args);
        line
    }
}

impl Write for Line64 {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let room = self.bytes.len() - self.len;
        let n = s.len().min(room);
        self.bytes[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
        Ok(())
    }
}
