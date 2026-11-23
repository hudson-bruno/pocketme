//! PADTEST: shows everything a game receives, so you can see each part
//! work.
//!
//! * The 12 buttons: lit while held.
//! * Both sticks: a dot in a box. Both triggers: a bar that fills up.
//! * The touch: a red cross where the finger is, and its position.
//! * The motion sensor: a spirit level (the bubble goes to the high
//!   side) and the six numbers. "NO MOTION" when the pad sends none.
//! * All four players, small, at the right: a number 1 to 4 and, for a
//!   connected pad, a row of 12 marks lit while that button is held; a
//!   dash for a pad that is not there. The big picture is player 1's.
//!
//! It also shows the "only when I say so" frame rate. After one second
//! with nothing changing, it asks the console to stop calling it every
//! frame. It is then called only when the input changes, or when its
//! own timer runs out (every quarter of a second, to look at the motion
//! sensor, which does not wake a game by itself). It draws only when
//! something changed, so while it sleeps the picture stays still and the
//! counter at the top right (pictures drawn) stops. At the first change
//! it asks for 30 calls a second again.
//!
//! Uses only the game kit. No heap, no floating point. Its state is a
//! few dozen bytes.

#![no_std]

use game_kit::{
    Ctx, Flow, Frame, Game, Motion, PLAYERS, STICK_MAX, TRIGGER_MAX, button, rate, rgb565,
};

// ---- Colours: palette numbers this game sets at start -------------------

const BG: u8 = 1;
const OFF: u8 = 2;
const ON: u8 = 3;
const TEXT: u8 = 4;
const DIM: u8 = 5;
const MARK: u8 = 6;
const BUBBLE: u8 = 7;
const LINE: u8 = 8;

/// The colours of palette numbers 1 to 8 (a game owns 0 to 239).
const COLOURS: [u16; 8] = [
    rgb565(18, 22, 34),    // BG: dark blue-grey
    rgb565(58, 64, 84),    // OFF: a button not held
    rgb565(255, 196, 40),  // ON: a button held
    rgb565(232, 232, 240), // TEXT
    rgb565(120, 128, 152), // DIM: labels
    rgb565(240, 64, 64),   // MARK: the touch cross, warnings
    rgb565(96, 224, 128),  // BUBBLE
    rgb565(84, 92, 120),   // LINE: outlines
];

/// Calls with nothing changed before asking for the on-demand rate (one
/// second at 30 calls a second).
const IDLE_CALLS: u16 = 30;

/// While sleeping, wake up this often (in microseconds) to look at the
/// motion sensor.
const CHECK_US: u64 = 250_000;

// ---- The game ------------------------------------------------------------

/// Everything the game receives that it draws. Two equal snapshots mean
/// nothing changed.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Snapshot {
    pad: bool,
    held: u32,
    left: (i16, i16),
    right: (i16, i16),
    triggers: (u16, u16),
    /// Where the finger is, while it is down.
    touch: Option<(i16, i16)>,
    motion: Option<Motion>,
    /// Every player's pad: connected, and the buttons held.
    players: [(bool, u32); PLAYERS],
}

impl Snapshot {
    fn read(ctx: &Ctx) -> Snapshot {
        Snapshot {
            pad: ctx.pad_connected(),
            held: ctx.player(0).held_bits(),
            left: ctx.left_stick(),
            right: ctx.right_stick(),
            triggers: ctx.triggers(),
            touch: ctx.touch().filter(|t| t.down).map(|t| (t.x, t.y)),
            motion: ctx.motion(),
            players: core::array::from_fn(|n| {
                let p = ctx.player(n);
                (p.connected(), p.held_bits())
            }),
        }
    }
}

pub struct PadTest {
    last: Snapshot,
    /// Calls in a row with nothing changed.
    idle: u16,
    /// Pictures drawn so far (shown, so you can see it stop while
    /// sleeping).
    draws: u32,
    /// The frame rate the last picture shows.
    drawn_rate: u16,
}

impl Game for PadTest {
    fn start(ctx: &mut Ctx) -> Self {
        ctx.log("padtest: started");
        PadTest {
            last: Snapshot::read(ctx),
            idle: 0,
            draws: 0,
            drawn_rate: u16::MAX,
        }
    }

    fn update(&mut self, ctx: &mut Ctx) -> Flow {
        if ctx.started() || ctx.must_redraw() {
            ctx.set_colours(BG, &COLOURS);
        }
        let now = Snapshot::read(ctx);
        let changed = now != self.last;
        self.last = now;
        self.idle = if changed {
            0
        } else {
            self.idle.saturating_add(1)
        };

        // The frame rate: 30 a second while things change, on demand
        // when still.
        let sleeping = ctx.video().rate == rate::ON_DEMAND;
        if sleeping && changed {
            if ctx.set_rate(rate::FPS30).is_ok() {
                ctx.log("padtest: awake, 30 calls a second");
            }
        } else if !sleeping && self.idle >= IDLE_CALLS && ctx.set_rate(rate::ON_DEMAND).is_ok() {
            ctx.log("padtest: still, sleeping until something changes");
        }
        // The timer must be set again in every call that wants it.
        ctx.wake_after(CHECK_US);

        // Draw only what changed since the last picture (the console keeps
        // the picture between calls).
        let rate_now = ctx.video().rate;
        if changed || ctx.must_redraw() || rate_now != self.drawn_rate {
            self.draws = self.draws.wrapping_add(1);
            self.drawn_rate = rate_now;
            let rate_name = match rate_now {
                rate::FPS30 => "30 FPS",
                rate::FPS60 => "60 FPS",
                _ => "ON DEMAND",
            };
            draw(ctx.screen(), &now, rate_name, self.draws);
        }
        Flow::Run
    }
}

game_kit::game!(PadTest);

// ---- Drawing -------------------------------------------------------------

fn draw(s: &mut Frame, now: &Snapshot, rate_name: &str, draws: u32) {
    s.clear(BG);
    s.text(2, 2, "PADTEST", TEXT);
    s.text(40, 2, rate_name, DIM);
    let n = Num::new(draws as i32);
    s.text(158 - width(n.as_str()), 2, n.as_str(), DIM);
    if !now.pad {
        s.text(92, 2, "NO PAD", MARK);
    }
    let held = |b: u32| now.held & b != 0;

    // Shoulders, with the triggers as bars under them.
    key(s, 4, 11, 36, 7, held(button::L), "L");
    key(s, 120, 11, 36, 7, held(button::R), "R");
    bar(s, 4, 20, now.triggers.0);
    bar(s, 120, 20, now.triggers.1);
    s.text(44, 20, "ZL", DIM);
    s.text(108, 20, "ZR", DIM);

    // The direction pad, as a cross.
    s.rect(21, 40, 9, 9, LINE);
    key(s, 21, 30, 9, 9, held(button::UP), "");
    key(s, 21, 50, 9, 9, held(button::DOWN), "");
    key(s, 11, 40, 9, 9, held(button::LEFT), "");
    key(s, 31, 40, 9, 9, held(button::RIGHT), "");

    // The four face buttons: A at the bottom, as the console confirms
    // with the bottom one (B right, X left, Y top).
    key(s, 129, 30, 9, 9, held(button::Y), "Y");
    key(s, 119, 40, 9, 9, held(button::X), "X");
    key(s, 139, 40, 9, 9, held(button::B), "B");
    key(s, 129, 50, 9, 9, held(button::A), "A");

    key(s, 52, 34, 26, 9, held(button::SELECT), "SEL");
    key(s, 82, 34, 26, 9, held(button::START), "START");

    // The sticks.
    stick(s, 46, 52, now.left);
    stick(s, 89, 52, now.right);
    s.text(53, 80, "LS", DIM);
    s.text(96, 80, "RS", DIM);

    // The motion sensor.
    level(s, now.motion);

    // All four players, small.
    players(s, &now.players);

    // The touch, last, so it is on top.
    match now.touch {
        Some((x, y)) => {
            let at = s.text(40, 112, "TOUCH ", TEXT);
            let at = s.text(at, 112, Num::new(x.into()).as_str(), TEXT);
            let at = s.text(at, 112, ",", TEXT);
            s.text(at + 1, 112, Num::new(y.into()).as_str(), TEXT);
            s.hline(x - 5, y, 11, MARK);
            s.vline(x, y - 5, 11, MARK);
            s.outline(x - 2, y - 2, 5, 5, MARK);
        }
        None => {
            s.text(40, 112, "TOUCH -", DIM);
        }
    }
}

/// A button: filled when held, with a label in the middle.
fn key(s: &mut Frame, x: i16, y: i16, w: u16, h: u16, on: bool, label: &str) {
    s.rect(x, y, w, h, if on { ON } else { OFF });
    if !label.is_empty() {
        let tx = x + (w as i16 - width(label)) / 2;
        s.text(
            tx,
            y + (h as i16 - 5) / 2,
            label,
            if on { BG } else { TEXT },
        );
    }
}

/// The four players at the right, one line each: the player's number,
/// then a mark per button (lit while held), or a dash when its pad is not
/// connected.
fn players(s: &mut Frame, players: &[(bool, u32); PLAYERS]) {
    const X: i16 = 116;
    const Y: i16 = 61;
    for (n, &(connected, held)) in players.iter().enumerate() {
        let y = Y + n as i16 * 7;
        let label = ["1", "2", "3", "4"][n];
        s.text(X, y, label, if connected { TEXT } else { DIM });
        if !connected {
            s.text(X + 6, y, "-", DIM);
            continue;
        }
        for b in 0..12 {
            let on = held & (1 << b) != 0;
            s.rect(X + 6 + b as i16 * 3, y, 2, 5, if on { ON } else { OFF });
        }
    }
}

/// A trigger: an outlined bar, filled as far as it is pulled.
fn bar(s: &mut Frame, x: i16, y: i16, v: u16) {
    s.outline(x, y, 36, 5, LINE);
    let fill = (u32::from(v) * 34 / u32::from(TRIGGER_MAX)) as u16;
    if fill > 0 {
        s.rect(x + 1, y + 1, fill, 3, ON);
    }
}

/// A stick: a box 25 pixels wide with a dot where the stick points.
fn stick(s: &mut Frame, x: i16, y: i16, (sx, sy): (i16, i16)) {
    s.outline(x, y, 25, 25, LINE);
    s.hline(x + 10, y + 12, 5, LINE);
    s.vline(x + 12, y + 10, 5, LINE);
    let off = |v: i16| (i32::from(v) * 10 / i32::from(STICK_MAX)) as i16;
    let moved = sx != 0 || sy != 0;
    s.rect(
        x + 11 + off(sx),
        y + 11 + off(sy),
        3,
        3,
        if moved { ON } else { TEXT },
    );
}

/// The spirit level and the motion numbers, or "NO MOTION".
fn level(s: &mut Frame, motion: Option<Motion>) {
    const CX: i16 = 20;
    const CY: i16 = 103;
    const R: i16 = 14;
    circle(s, CX, CY, R, if motion.is_some() { LINE } else { OFF });
    s.hline(CX - 2, CY, 5, LINE);
    s.vline(CX, CY - 2, 5, LINE);
    let Some(m) = motion else {
        s.text(40, 96, "NO MOTION", MARK);
        return;
    };
    // The bubble goes to the high side: accel x grows when the right side
    // is up, accel y when the far side is up (shown as up on the screen).
    // 4096 is one g, a full tilt; the bubble then sits at the rim.
    let (mut bx, mut by) = (
        i32::from(m.accel[0]) * 11 / 4096,
        -i32::from(m.accel[1]) * 11 / 4096,
    );
    // Keep it inside the circle (a square check is enough here).
    bx = bx.clamp(-11, 11);
    by = by.clamp(-11, 11);
    s.rect(CX - 2 + bx as i16, CY - 2 + by as i16, 5, 5, BUBBLE);

    s.text(40, 90, "ACCEL", DIM);
    s.text(40, 101, "GYRO", DIM);
    for i in 0..3 {
        let x = 70 + i as i16 * 30;
        s.text(x, 90, Num::new(m.accel[i].into()).as_str(), TEXT);
        s.text(x, 101, Num::new(m.gyro[i].into()).as_str(), TEXT);
    }
}

/// A circle outline (the midpoint way: whole numbers only).
fn circle(s: &mut Frame, cx: i16, cy: i16, r: i16, c: u8) {
    let (mut x, mut y, mut err) = (r, 0i16, 1 - r);
    while x >= y {
        for (dx, dy) in [(x, y), (y, x), (-y, x), (-x, y)] {
            s.pixel(cx + dx, cy + dy, c);
            s.pixel(cx - dx, cy - dy, c);
        }
        y += 1;
        if err < 0 {
            err += 2 * y + 1;
        } else {
            x -= 1;
            err += 2 * (y - x) + 1;
        }
    }
}

fn width(text: &str) -> i16 {
    game_kit::text_width(text) as i16
}

// ---- Numbers as text, with no heap -----------------------------------------

/// A whole number written out, in a 12-byte buffer.
struct Num {
    buf: [u8; 12],
    start: usize,
}

impl Num {
    fn new(v: i32) -> Num {
        let mut buf = [0u8; 12];
        let mut i = buf.len();
        let mut n = v.unsigned_abs();
        loop {
            i -= 1;
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        if v < 0 {
            i -= 1;
            buf[i] = b'-';
        }
        Num { buf, start: i }
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[self.start..]).unwrap_or("?")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_written_out_even_the_smallest() {
        assert_eq!(Num::new(0).as_str(), "0");
        assert_eq!(Num::new(-4096).as_str(), "-4096");
        assert_eq!(Num::new(i32::MIN).as_str(), "-2147483648");
    }
}
