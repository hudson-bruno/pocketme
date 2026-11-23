//! Is a fast clock clean? Pictures with known pixels, sent at the clock
//! under test and read back from the panel at the slow read clock, every
//! pixel compared.
//!
//! A "scene" is a list of draws. Every pixel of a draw is a pure
//! function of its position and a seed, so what the panel should hold is
//! known without keeping a copy of the screen (a 320x240 copy would be
//! 150 KB, more than this chip can spare): it is the colour of the last
//! draw that covers the pixel.
//!
//! The draws:
//!
//! * a full screen, either through the real pipeline (framebuffer +
//!   palette, doubled) or "raw" (any panel pixel, no doubling): noise,
//!   one-pixel checkerboard, one-pixel lines, and 0x5555/0xAAAA, which
//!   flips the data wire on every clock;
//! * 12 raw rectangles at odd positions with odd widths;
//! * 4 rectangles through the pipeline (partial updates).
//!
//! A wrong pixel is read a second time. If the second read is right, the
//! first read was the fault (counted as `read_glitch`); if it is wrong
//! the same way twice, the panel's memory holds a wrong pixel
//! (`wrong_pixels`): the write failed.

use log::{info, warn};

use crate::{
    lcd::{self, Depth, H, Lcd, Send, W, cycles},
    pipeline::{self, Frame, LW, Palette, Rect},
};

/// Deterministic noise (a hash of position and seed).
#[inline(always)]
pub fn hash(x: u32, y: u32, seed: u32) -> u32 {
    let mut h =
        x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77) ^ seed.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Through the pipeline: framebuffer of noise, palette, 2x.
    Logical,
    /// Raw panel pixels.
    Noise,
    Checker,
    VLines,
    HLines,
    Bits,
}

pub const KINDS: [Kind; 6] = [
    Kind::Logical,
    Kind::Noise,
    Kind::Checker,
    Kind::VLines,
    Kind::HLines,
    Kind::Bits,
];

#[derive(Clone, Copy, Debug)]
pub struct Draw {
    /// Panel pixels.
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub kind: Kind,
    pub seed: u32,
}

/// The palette index a logical pixel gets in a `Logical` draw.
#[inline(always)]
fn logical_index(lx: u32, ly: u32, seed: u32) -> u8 {
    hash(lx, ly, seed) as u8
}

/// RGB565 of a raw draw's pixel.
#[inline(always)]
fn raw_colour(kind: Kind, seed: u32, x: u32, y: u32) -> u16 {
    match kind {
        Kind::Noise => hash(x, y, seed) as u16,
        Kind::Checker => {
            if (x + y + seed) & 1 == 0 {
                0xFFFF
            } else {
                0x0000
            }
        }
        Kind::VLines => {
            if x % 5 == seed % 5 {
                0xFFFF
            } else {
                0x0000
            }
        }
        Kind::HLines => {
            if y % 3 == seed % 3 {
                0xF81F
            } else {
                0x07E0
            }
        }
        Kind::Bits => {
            if (x + seed) & 1 == 0 {
                0x5555
            } else {
                0xAAAA
            }
        }
        Kind::Logical => 0,
    }
}

impl Draw {
    fn covers(&self, x: u16, y: u16) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    /// RGB565 of a pixel of this draw.
    fn colour(&self, pal: &Palette, x: u32, y: u32) -> u16 {
        match self.kind {
            Kind::Logical => pal.rgb565[usize::from(logical_index(x / 2, y / 2, self.seed))],
            kind => raw_colour(kind, self.seed, x, y),
        }
    }
}

/// The palette used by the checks: noise, with the four entries that
/// stress the wire most at the start.
pub fn stress_palette(pal: &mut Palette) {
    for i in 0..256 {
        let colour = match i {
            0 => 0x5555,
            1 => 0xAAAA,
            2 => 0x0000,
            3 => 0xFFFF,
            _ => hash(i as u32, 0, 77) as u16,
        };
        pal.set(i, colour);
    }
}

/// Sends a raw draw (any panel pixels, no framebuffer).
fn send_raw(lcd: &mut Lcd, draw: &Draw, how: Send) -> Result<lcd::Timing, lcd::LcdError> {
    let depth = lcd.depth();
    let (x0, y0, w, h) = (
        u32::from(draw.x),
        u32::from(draw.y),
        u32::from(draw.w),
        u32::from(draw.h),
    );
    let row = depth.bytes(w as usize);
    // Whole rows per strip, a multiple of 8 rows so every strip is a
    // whole number of 32-bit words in either depth (the caller makes
    // heights multiples of 8 in 12-bit mode, even in 16-bit mode).
    let per = ((lcd::STRIP_BYTES / row) / 8 * 8).max(8).min(h as usize) as u32;
    let strips = h.div_ceil(per) as usize;
    let (kind, seed) = (draw.kind, draw.seed);
    lcd.draw(
        (draw.x, draw.y, draw.x + draw.w - 1, draw.y + draw.h - 1),
        strips,
        how,
        |k, buf| {
            let first = y0 + k as u32 * per;
            let last = (first + per).min(y0 + h);
            let mut at = 0;
            match depth {
                Depth::Rgb565 => {
                    for y in first..last {
                        for x in x0..x0 + w {
                            let [a, b] = raw_colour(kind, seed, x, y).to_be_bytes();
                            buf[at] = a;
                            buf[at + 1] = b;
                            at += 2;
                        }
                    }
                }
                Depth::Rgb444 => {
                    // Pixels in pairs, 3 bytes a pair, running on across
                    // row ends: the panel takes the window as one stream.
                    let mut pending: Option<u16> = None;
                    for y in first..last {
                        for x in x0..x0 + w {
                            let c = pipeline::rgb565_to_444(raw_colour(kind, seed, x, y));
                            match pending.take() {
                                None => pending = Some(c),
                                Some(p) => {
                                    buf[at] = (p >> 4) as u8;
                                    buf[at + 1] = (((p & 0xF) << 4) | (c >> 8)) as u8;
                                    buf[at + 2] = c as u8;
                                    at += 3;
                                }
                            }
                        }
                    }
                }
            }
            // Round up to a word; the caller's sizes make this a no-op.
            at.div_ceil(4) * 4
        },
    )
}

/// Puts a logical draw's pixels into the framebuffer and sends that
/// rectangle through the real pipeline.
fn send_logical(
    lcd: &mut Lcd,
    fb: &mut Frame,
    pal: &Palette,
    draw: &Draw,
    how: Send,
) -> Result<lcd::Timing, lcd::LcdError> {
    let rect = Rect {
        x: draw.x / 2,
        y: draw.y / 2,
        w: draw.w / 2,
        h: draw.h / 2,
    };
    for ly in rect.y..rect.y + rect.h {
        for lx in rect.x..rect.x + rect.w {
            fb.0[usize::from(ly) * LW + usize::from(lx)] =
                logical_index(u32::from(lx), u32::from(ly), draw.seed);
        }
    }
    pipeline::present_rect(lcd, fb, pal, rect, 8, how)
}

/// The scene of iteration `i`. Returns the number of draws.
fn scene(i: u32, depth: Depth, draws: &mut [Draw; 17]) -> usize {
    let seed = i.wrapping_mul(7919).wrapping_add(13);
    draws[0] = Draw {
        x: 0,
        y: 0,
        w: W as u16,
        h: H as u16,
        kind: KINDS[(i % 6) as usize],
        seed,
    };
    let mut n = 1;
    // 12 raw rectangles, one per 80x80 cell, at odd positions.
    for cell in 0..12u32 {
        let r = hash(cell, i, 99);
        let (cx, cy) = ((cell % 4) * 80, (cell / 4) * 80);
        let x = cx + 1 + 2 * (r % 10);
        let y = cy + 1 + 2 * ((r >> 4) % 8);
        let w = 1 + 2 * ((r >> 8) % 23);
        let h = match depth {
            Depth::Rgb565 => 2 + 2 * ((r >> 16) % 20),
            Depth::Rgb444 => 8 * (1 + (r >> 16) % 5),
        };
        // In 12-bit the pixel count must be a multiple of 8 (whole words).
        let kind = KINDS[1 + ((r >> 24) % 5) as usize];
        draws[n] = Draw {
            x: x as u16,
            y: y as u16,
            w: w as u16,
            h: h as u16,
            kind,
            seed: r,
        };
        n += 1;
    }
    // 4 partial updates through the pipeline.
    for j in 0..4u32 {
        let r = hash(j, i, 1234);
        let mut rect = Rect {
            x: (r % 140) as u16,
            y: ((r >> 8) % 100) as u16,
            w: (4 + (r >> 16) % 17) as u16,
            h: (2 + (r >> 24) % 19) as u16,
        };
        if depth == Depth::Rgb444 {
            rect = rect.to_quads();
        }
        draws[n] = Draw {
            x: rect.x * 2,
            y: rect.y * 2,
            w: rect.w * 2,
            h: rect.h * 2,
            kind: Kind::Logical,
            seed: r,
        };
        n += 1;
    }
    n
}

/// A planted fault: in the first scene of every run, a 4x2 block of
/// panel pixels is written with the opposite of what the scene says
/// (every bit flipped). The check must find exactly these 8 pixels; if it
/// does not, its "0 wrong" means nothing.
const PLANT: (u16, u16, u16, u16) = (101, 101, 4, 2);

fn planted(i: u32, x: u16, y: u16) -> bool {
    i == 0 && x >= PLANT.0 && x < PLANT.0 + PLANT.2 && y >= PLANT.1 && y < PLANT.1 + PLANT.3
}

/// Writes the planted block (see [`PLANT`]).
fn plant(lcd: &mut Lcd, pal: &Palette, draws: &[Draw]) -> Result<lcd::Timing, lcd::LcdError> {
    let depth = lcd.depth();
    let (x0, y0, w, h) = PLANT;
    lcd.draw(
        (x0, y0, x0 + w - 1, y0 + h - 1),
        1,
        Send::Overlap,
        |_, buf| {
            let mut colours = [0u16; 8];
            for (k, c) in colours.iter_mut().enumerate() {
                let (x, y) = (x0 + k as u16 % w, y0 + k as u16 / w);
                *c = !expect(Depth::Rgb565, pal, draws, x, y);
            }
            match depth {
                Depth::Rgb565 => {
                    for (k, c) in colours.iter().enumerate() {
                        buf[2 * k..2 * k + 2].copy_from_slice(&c.to_be_bytes());
                    }
                    16
                }
                Depth::Rgb444 => {
                    for k in 0..4 {
                        let (p, c) = (
                            pipeline::rgb565_to_444(colours[2 * k]),
                            pipeline::rgb565_to_444(colours[2 * k + 1]),
                        );
                        buf[3 * k] = (p >> 4) as u8;
                        buf[3 * k + 1] = (((p & 0xF) << 4) | (c >> 8)) as u8;
                        buf[3 * k + 2] = c as u8;
                    }
                    12
                }
            }
        },
    )
}

/// Result of one check run.
#[derive(Default, Debug)]
pub struct Tally {
    pub scenes: u32,
    pub draws: u32,
    pub bytes: u64,
    pub pixels_checked: u64,
    /// Wrong twice the same way: the panel's memory is wrong.
    pub wrong_pixels: u64,
    /// Wrong once, right on the second read: the read was the fault.
    pub read_glitch: u64,
    /// Wrong twice, differently.
    pub unstable: u64,
    /// Wrong pixels by the kind of draw that covered them.
    pub wrong_by_kind: [u64; 6],
    pub write_cycles: u64,
    /// Planted wrong pixels the check found (8 expected per run).
    pub planted_found: u64,
    pub errors: u32,
}

/// Sends one scene at the current clock and depth, and checks it.
fn run_scene(
    lcd: &mut Lcd,
    fb: &mut Frame,
    pal: &Palette,
    i: u32,
    scratch: &mut [u8],
    tally: &mut Tally,
    printed: &mut u32,
) {
    let depth = lcd.depth();
    let mut draws = [Draw {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
        kind: Kind::Noise,
        seed: 0,
    }; 17];
    let n = scene(i, depth, &mut draws);
    let start = cycles();
    for draw in &draws[..n] {
        let result = match draw.kind {
            Kind::Logical => send_logical(lcd, fb, pal, draw, Send::Overlap),
            _ => send_raw(lcd, draw, Send::Overlap),
        };
        match result {
            Ok(t) => tally.bytes += u64::from(t.bytes),
            Err(e) => {
                tally.errors += 1;
                warn!("check: sending failed: {e:?}");
            }
        }
    }
    if i == 0 && plant(lcd, pal, &draws[..n]).is_err() {
        tally.errors += 1;
    }
    tally.write_cycles += u64::from(cycles().wrapping_sub(start));
    tally.scenes += 1;
    tally.draws += n as u32;

    // Read back at the slow clock. The read format does not follow
    // COLMOD (3 bytes a pixel), but the panel is put in 16-bit mode for
    // reading anyway, so the check does not depend on that.
    if depth == Depth::Rgb444 {
        let _ = lcd.command(lcd::cmd::COLMOD, &[Depth::Rgb565.colmod()]);
    }
    let mut first = [0u8; 3 * W];
    for y in 0..H as u16 {
        match lcd.read_row(0, y, W as u16, scratch) {
            Ok(row) => first.copy_from_slice(row),
            Err(_) => {
                tally.errors += 1;
                continue;
            }
        }
        tally.pixels_checked += W as u64;
        if compare_row(depth, pal, &draws[..n], y, &first) == 0 {
            continue;
        }
        // Read the row a second time and sort each wrong pixel out.
        let Ok(second) = lcd.read_row(0, y, W as u16, scratch) else {
            tally.errors += 1;
            continue;
        };
        for x in 0..W {
            let expected = expect(depth, pal, &draws[..n], x as u16, y);
            let a = &first[3 * x..3 * x + 3];
            let b = &second[3 * x..3 * x + 3];
            let (ga, gb) = (got(depth, a), got(depth, b));
            if planted(i, x as u16, y) {
                // Must be wrong, and read the same twice.
                if ga != expected && ga == gb {
                    tally.planted_found += 1;
                }
                continue;
            }
            if ga == expected && gb == expected {
                continue;
            }
            if ga == gb {
                tally.wrong_pixels += 1;
                if let Some(d) = draws[..n].iter().rev().find(|d| d.covers(x as u16, y)) {
                    tally.wrong_by_kind[KINDS.iter().position(|k| *k == d.kind).unwrap_or(0)] += 1;
                }
                if *printed < 8 {
                    *printed += 1;
                    warn!(
                        "check: WRONG x={x} y={y} expected={expected:04x} got={ga:04x} (twice) scene={i}"
                    );
                }
            } else if ga == expected || gb == expected {
                tally.read_glitch += 1;
            } else {
                tally.unstable += 1;
                if *printed < 8 {
                    *printed += 1;
                    warn!(
                        "check: UNSTABLE x={x} y={y} expected={expected:04x} got={ga:04x} then {gb:04x} scene={i}"
                    );
                }
            }
        }
    }
    if depth == Depth::Rgb444 {
        let _ = lcd.command(lcd::cmd::COLMOD, &[Depth::Rgb444.colmod()]);
    }
}

#[inline(always)]
fn got(depth: Depth, rgb: &[u8]) -> u16 {
    match depth {
        Depth::Rgb565 => lcd::read_to_565(rgb),
        Depth::Rgb444 => lcd::read_to_444(rgb),
    }
}

#[inline(always)]
fn expect(depth: Depth, pal: &Palette, draws: &[Draw], x: u16, y: u16) -> u16 {
    let d = draws
        .iter()
        .rev()
        .find(|d| d.covers(x, y))
        .expect("the first draw covers the screen");
    let c = d.colour(pal, u32::from(x), u32::from(y));
    match depth {
        Depth::Rgb565 => c,
        Depth::Rgb444 => pipeline::rgb565_to_444(c),
    }
}

/// Number of pixels of row `y` that differ from what the draws say.
fn compare_row(depth: Depth, pal: &Palette, draws: &[Draw], y: u16, row: &[u8]) -> u32 {
    let mut wrong = 0;
    for x in 0..W {
        if got(depth, &row[3 * x..3 * x + 3]) != expect(depth, pal, draws, x as u16, y) {
            wrong += 1;
        }
    }
    wrong
}

/// Runs scenes for `seconds` at `mhz` and `depth`, prints a summary
/// line, and leaves the panel at 16-bit and the clock it had.
pub fn soak(
    lcd: &mut Lcd,
    fb: &mut Frame,
    pal: &mut Palette,
    scratch: &mut [u8],
    mhz: u32,
    depth: Depth,
    seconds: u32,
) -> Tally {
    let before_mhz = lcd.write_mhz();
    let _ = lcd.set_write_mhz(mhz);
    let _ = lcd.set_depth(depth);
    stress_palette(pal);
    let mut tally = Tally::default();
    let mut printed = 0;
    let start = esp_hal::time::Instant::now();
    let mut i = 0;
    let mut next_report = 20;
    info!(
        "check: start mhz={mhz} depth={} seconds={seconds}",
        depth.bits()
    );
    while start.elapsed().as_secs() < u64::from(seconds) {
        run_scene(lcd, fb, pal, i, scratch, &mut tally, &mut printed);
        i += 1;
        if start.elapsed().as_secs() >= next_report {
            next_report += 20;
            info!(
                "check: progress mhz={mhz} depth={} seconds={} scenes={} pixels_checked={} wrong_pixels={} read_glitch={} unstable={}",
                depth.bits(),
                start.elapsed().as_secs(),
                tally.scenes,
                tally.pixels_checked,
                tally.wrong_pixels,
                tally.read_glitch,
                tally.unstable
            );
        }
    }
    let secs = start.elapsed().as_millis();
    info!(
        "check: result mhz={mhz} depth={} seconds={}.{:03} scenes={} draws={} bytes_written={} pixels_checked={} planted_found={}_of_8 wrong_pixels={} read_glitch={} unstable={} errors={} write_ms={} wrong_by_kind[logical,noise,checker,vlines,hlines,bits]={:?}",
        depth.bits(),
        secs / 1000,
        secs % 1000,
        tally.scenes,
        tally.draws,
        tally.bytes,
        tally.pixels_checked,
        tally.planted_found,
        tally.wrong_pixels,
        tally.read_glitch,
        tally.unstable,
        tally.errors,
        tally.write_cycles / (u64::from(lcd::CPU_MHZ) * 1000),
        tally.wrong_by_kind
    );
    let _ = lcd.set_depth(Depth::Rgb565);
    let _ = lcd.set_write_mhz(before_mhz);
    tally
}
