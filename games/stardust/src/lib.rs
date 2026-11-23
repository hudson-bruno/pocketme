//! STARDUST: a small toy. Steer a comet and catch the stars.
//!
//! * Steer with the direction pad or the left stick, or hold a finger
//!   on the screen: the comet flies towards it.
//! * Each star caught adds one to the count at the top. A new star then
//!   appears somewhere else.
//! * Start ends the toy (back to the home screen).
//!
//! It is here to prove the game path, not to be a finished game:
//! drawing (pixel-art tables), the palette (the stars twinkle by
//! changing one palette colour, not by drawing), input, and time (the
//! comet moves by the time that passed, so it flies at the same speed
//! whatever the frame rate). It runs at the console's default rate, 30
//! frames a second.
//!
//! Uses only the game kit. No heap, no floating point. Positions are in
//! 1/256 of a pixel. Its state is about 200 bytes.

#![no_std]

use game_kit::{CLEAR, Ctx, Flow, Frame, Game, STICK_MAX, Sprite, button, rgb565};

// ---- Colours: palette numbers this game sets at start -------------------

const SKY: u8 = 1;
const FAR: u8 = 2;
const EDGE: u8 = 3;
const CORE: u8 = 4;
const TAIL1: u8 = 5;
const TAIL2: u8 = 6;
const TAIL3: u8 = 7;
const STAR: u8 = 8;
const TWINKLE: u8 = 9;
const TEXT: u8 = 10;

/// Palette numbers 1 to 10.
const COLOURS: [u16; 10] = [
    rgb565(8, 10, 28),     // SKY
    rgb565(70, 76, 120),   // FAR: the slow background dots
    rgb565(120, 200, 255), // EDGE of the comet
    rgb565(240, 250, 255), // CORE of the comet
    rgb565(90, 160, 230),  // TAIL1, nearest the comet
    rgb565(50, 100, 170),  // TAIL2
    rgb565(30, 56, 110),   // TAIL3
    rgb565(255, 200, 60),  // STAR
    rgb565(255, 255, 200), // TWINKLE (changed every few frames)
    rgb565(220, 224, 240), // TEXT
];

/// The twinkle colour steps through these, one step every 0.15 s.
const TWINKLES: [u16; 4] = [
    rgb565(255, 255, 200),
    rgb565(255, 220, 120),
    rgb565(255, 160, 60),
    rgb565(255, 220, 120),
];

// ---- Pixel art -------------------------------------------------------------

const O: u8 = CLEAR; // see-through

#[rustfmt::skip]
const COMET: [u8; 25] = [
    O,    EDGE, EDGE, EDGE, O,
    EDGE, CORE, CORE, CORE, EDGE,
    EDGE, CORE, CORE, CORE, EDGE,
    EDGE, CORE, CORE, CORE, EDGE,
    O,    EDGE, EDGE, EDGE, O,
];

#[rustfmt::skip]
const STAR_ART: [u8; 25] = [
    O,    O,    STAR,    O,    O,
    O,    STAR, STAR,    STAR, O,
    STAR, STAR, TWINKLE, STAR, STAR,
    O,    STAR, STAR,    STAR, O,
    O,    O,    STAR,    O,    O,
];

// ---- Numbers ---------------------------------------------------------------

/// One pixel in position units.
const PX: i32 = 256;
/// The picture's size in position units.
const W: i32 = 160 * PX;
const H: i32 = 120 * PX;
/// Top speed: 70 pixels a second.
const SPEED: i32 = 70;
/// How many stars are out at once.
const STARS: usize = 3;
/// Tail length (points) and how often a point is added (microseconds).
const TAIL: usize = 8;
const TAIL_EVERY_US: u64 = 50_000;
/// Background dots and how slowly they drift (microseconds per pixel).
const DOTS: usize = 24;
const DRIFT_US: u64 = 125_000;
/// A catch flashes a ring for this long.
const FLASH_US: u64 = 300_000;
/// The random numbers' fixed start: the same stars every run, so tests
/// repeat.
const SEED: u32 = 0x2545_F491;

pub struct Stardust {
    /// The comet's middle, in 1/256 pixel.
    x: i32,
    y: i32,
    /// The tail: older positions, in whole pixels, newest first.
    tail: [(i16, i16); TAIL],
    next_tail_us: u64,
    stars: [(i16, i16); STARS],
    dots: [(u8, u8); DOTS],
    caught: u32,
    /// Where and when the last catch was (for the flash).
    flash: (i16, i16, u64),
    rng: u32,
}

impl Game for Stardust {
    fn start(ctx: &mut Ctx) -> Self {
        let mut rng = SEED;
        let mut dots = [(0, 0); DOTS];
        for d in &mut dots {
            *d = (
                (next(&mut rng) % 160) as u8,
                (8 + next(&mut rng) % 112) as u8,
            );
        }
        let mut stars = [(0, 0); STARS];
        for s in &mut stars {
            *s = place(&mut rng);
        }
        ctx.log("stardust: started");
        Stardust {
            x: W / 2,
            y: H / 2,
            tail: [(80, 60); TAIL],
            next_tail_us: 0,
            stars,
            dots,
            caught: 0,
            flash: (0, 0, 0),
            rng,
        }
    }

    fn update(&mut self, ctx: &mut Ctx) -> Flow {
        if ctx.started() || ctx.must_redraw() {
            ctx.set_colours(SKY, &COLOURS);
        }
        if ctx.pressed(button::START) {
            ctx.log("stardust: the end");
            return Flow::Done;
        }
        let now = ctx.time_us();
        // Twinkle: one palette colour changes; every star follows.
        let step = (now / 150_000 % TWINKLES.len() as u64) as usize;
        ctx.set_colour(TWINKLE, TWINKLES[step]);

        self.steer(ctx);
        if now >= self.next_tail_us {
            self.tail.copy_within(0..TAIL - 1, 1);
            self.tail[0] = self.pixel();
            self.next_tail_us = now + TAIL_EVERY_US;
        }
        self.catch(ctx, now);
        self.draw(ctx.screen(), now);
        Flow::Run
    }
}

game_kit::game!(Stardust);

impl Stardust {
    /// The comet's middle in whole pixels.
    fn pixel(&self) -> (i16, i16) {
        ((self.x / PX) as i16, (self.y / PX) as i16)
    }

    /// Moves the comet by the input, for the time that passed.
    fn steer(&mut self, ctx: &Ctx) {
        // Where to go: -STICK_MAX..STICK_MAX on each axis.
        let (mut dx, mut dy) = ctx.left_stick();
        let full = STICK_MAX;
        if ctx.held(button::LEFT) {
            dx = -full;
        }
        if ctx.held(button::RIGHT) {
            dx = full;
        }
        if ctx.held(button::UP) {
            dy = -full;
        }
        if ctx.held(button::DOWN) {
            dy = full;
        }
        if let Some(t) = ctx.touch().filter(|t| t.down) {
            let (cx, cy) = self.pixel();
            // Full speed from 8 pixels away; slower when close.
            let pull = |d: i16| (i32::from(d) * i32::from(full) / 8).clamp(-32767, 32767) as i16;
            dx = pull(t.x - cx);
            dy = pull(t.y - cy);
        }
        // Distance = speed x time. 64-bit, so a long pause cannot overflow.
        let dt = i64::from(ctx.delta_us().min(100_000));
        let move_by = |d: i16| {
            (i64::from(d) * i64::from(SPEED * PX) * dt / (i64::from(STICK_MAX) * 1_000_000)) as i32
        };
        self.x = (self.x + move_by(dx)).rem_euclid(W);
        self.y = (self.y + move_by(dy)).rem_euclid(H);
    }

    /// A star within 5 pixels of the comet is caught.
    fn catch(&mut self, ctx: &mut Ctx, now: u64) {
        let (cx, cy) = self.pixel();
        for i in 0..STARS {
            let (sx, sy) = self.stars[i];
            if (sx - cx).abs() <= 5 && (sy - cy).abs() <= 5 {
                self.caught += 1;
                self.flash = (sx, sy, now + FLASH_US);
                self.stars[i] = place(&mut self.rng);
                ctx.log_line()
                    .text("stardust: star ")
                    .num(self.caught)
                    .end();
            }
        }
    }

    fn draw(&self, s: &mut Frame, now: u64) {
        s.clear(SKY);
        // Background dots drift left, slowly.
        let shift = (now / DRIFT_US % 160) as i16;
        for &(x, y) in &self.dots {
            let x = (i16::from(x) - shift).rem_euclid(160);
            s.pixel(x, i16::from(y), FAR);
        }
        let star = Sprite::new(5, 5, &STAR_ART);
        for &(x, y) in &self.stars {
            s.sprite(x - 2, y - 2, &star, CLEAR);
        }
        if now < self.flash.2 {
            let (x, y, _) = self.flash;
            let r = 4 + ((self.flash.2 - now) / 50_000) as u16;
            s.outline(x - r as i16, y - r as i16, 2 * r + 1, 2 * r + 1, TWINKLE);
        }
        // The tail, oldest first, fading out.
        for (i, &(x, y)) in self.tail.iter().enumerate().rev() {
            let c = match i {
                0..=2 => TAIL1,
                3..=5 => TAIL2,
                _ => TAIL3,
            };
            s.rect(x - 1, y - 1, 2, 2, c);
        }
        let (cx, cy) = self.pixel();
        s.sprite(cx - 2, cy - 2, &Sprite::new(5, 5, &COMET), CLEAR);

        let at = s.text(2, 2, "STARS ", TEXT);
        let mut buf = [0u8; 10];
        s.text(at, 2, count(self.caught, &mut buf), TEXT);
    }
}

/// A new place for a star, away from the edges and the score.
fn place(rng: &mut u32) -> (i16, i16) {
    ((8 + next(rng) % 144) as i16, (14 + next(rng) % 98) as i16)
}

/// A small random number maker (xorshift): the same numbers every run.
fn next(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

/// `n` written out, in `buf`.
fn count(mut n: u32, buf: &mut [u8; 10]) -> &str {
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    core::str::from_utf8(&buf[i..]).unwrap_or("?")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stars_land_inside_the_screen_and_clear_of_the_score() {
        let mut rng = SEED;
        for _ in 0..10_000 {
            let (x, y) = place(&mut rng);
            assert!((8..152).contains(&x) && (14..112).contains(&y));
        }
    }

    #[test]
    fn the_score_is_written_out() {
        let mut buf = [0; 10];
        assert_eq!(count(0, &mut buf), "0");
        assert_eq!(count(u32::MAX, &mut buf), "4294967295");
    }
}
