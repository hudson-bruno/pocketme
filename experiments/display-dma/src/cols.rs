//! The columns way: the picture sent transposed, along the refresh.
//!
//! In landscape (MADCTL 60h, MV set) a memory write fills picture ROWS,
//! across the sweep: that is why rows tear (see `tear.rs`). With MV
//! clear the address counter runs along one gate line (the 240-pixel
//! direction) and then steps to the next gate line (data sheet p124,
//! 8.12: "In horizontal addressing mode (MV=0), the X-address increments
//! after each byte ... and Y increments to address the next row"). In
//! our landscape picture one gate line is one picture COLUMN. So with an
//! MV=0 value in MADCTL, a window whose data is the picture sent column
//! by column (column 0 top to bottom, then column 1, ...) is written
//! gate line after gate line, in the order the sweep visits them: one
//! window per changed rectangle, no slices. At 80 MHz 16-bit one gate
//! line (240 pixels, 480 bytes) takes 48 us, the sweep 49.53 us.
//!
//! Which MV=0 value? 60h is MY=0, MX=1, MV=1. Dropping MV gives 40h,
//! under which interface column c (0..239) should be picture y and
//! interface row r (0..319) picture x, no mirror. The paper cannot
//! confirm it (the mapping figure on p125 is not readable as text, and
//! register C0h has an XMX bit set by default that flips MX), so
//! [`check`] finds out at start-up by reading pixels back, and the way
//! refuses to run until the check has confirmed the value used.
//!
//! Everything about the address mapping is here, in one place:
//! [`MADCTL_COLS`], [`MIRROR_C`], and the direction (`Beam::reversed`).

use log::{info, warn};

use crate::{
    lcd::{
        self, Depth, H, Lcd, LcdError, MADCTL_LANDSCAPE, MADCTL_MY, STRIP_BYTES, Send, Timing, W,
    },
    pipeline::{self, Frame, Palette, Rect},
};

// ---- the mapping: set from the start-up check ---------------------------

/// MADCTL for the columns way (an MV=0 value: 00h, 40h, 80h or C0h).
/// `bench=tear_orient` prints which value returns the pattern with no
/// mirror (`use=`); put that here.
pub const MADCTL_COLS: u8 = 0x40;

/// Whether each gate line must be sent bottom to top (interface column
/// c = 239 - y). Only needed if no MV=0 value maps the picture with no
/// mirror (if MX and MY each flip one axis, one of the four does). It
/// costs nothing in time: it only changes the order inside a gate line.
pub const MIRROR_C: bool = false;

// There is no row mirror flag on purpose: a row mirror (interface row
// r = 319 - x) makes the write run against the sweep, which defeats the
// way. The row order is set by the direction instead (see `Config`).

const _: () = assert!(
    MADCTL_COLS & 0x3F == 0,
    "an MV=0 value has only MY and MX set"
);

/// First strip of a window at most this many bytes (at least one logical
/// column), the second at most [`SECOND_STRIP_BYTES`], then whole strip
/// buffers. The wire waits for the first strip to be filled; a small one
/// starts it at once (one full-height column: 960 bytes, filled in about
/// 9 us), and each strip is filled while the one before is on the wire
/// (the fill is about 10 times faster than the wire at 80 MHz).
const FIRST_STRIP_BYTES: usize = 1024;
const SECOND_STRIP_BYTES: usize = 4096;

/// The interface mapping used for one frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Config {
    pub madctl: u8,
    /// Interface column c = 239 - y (each gate line bottom to top).
    pub mirror_c: bool,
    /// Interface row r = 319 - x (picture columns sent right to left).
    pub mirror_r: bool,
}

impl Config {
    /// The mapping for a sweep direction. Direction A (left to right):
    /// [`MADCTL_COLS`], rows not mirrored, so the counter's row steps go
    /// left to right with the sweep. Direction B (right to left): the
    /// write must run the other way, so MY is flipped (the interface row
    /// counts the gate lines the other way) and the picture columns are
    /// sent right to left (r = 319 - x). The start-up check tests both.
    pub fn for_direction(reversed: bool) -> Config {
        Config {
            madctl: if reversed {
                MADCTL_COLS ^ MADCTL_MY
            } else {
                MADCTL_COLS
            },
            mirror_c: MIRROR_C,
            mirror_r: reversed,
        }
    }

    /// Bit of this mapping in [`Orient::matches`].
    fn bit(self) -> u16 {
        1 << (u16::from(self.madctl >> 6) * 4
            + u16::from(self.mirror_c) * 2
            + u16::from(self.mirror_r))
    }

    /// Interface window (CASET c0..c1, RASET r0..r1) for a logical
    /// rectangle.
    pub fn window(self, rect: Rect) -> (u16, u16, u16, u16) {
        // Panel (landscape) corners, inclusive.
        let (x0, y0, x1, y1) = rect.panel_window();
        let (c0, c1) = if self.mirror_c {
            (H as u16 - 1 - y1, H as u16 - 1 - y0)
        } else {
            (y0, y1)
        };
        let (r0, r1) = if self.mirror_r {
            (W as u16 - 1 - x1, W as u16 - 1 - x0)
        } else {
            (x0, x1)
        };
        (c0, r0, c1, r1)
    }

    /// Interface address of panel (landscape) pixel (x, y).
    fn address(self, x: u16, y: u16) -> (u16, u16) {
        let c = if self.mirror_c { H as u16 - 1 - y } else { y };
        let r = if self.mirror_r { W as u16 - 1 - x } else { x };
        (c, r)
    }
}

/// What the start-up check found: for each MV=0 value (00h, 40h, 80h,
/// C0h) and each pair of mirrors, whether the pattern came back.
#[derive(Clone, Copy, Default)]
pub struct Orient {
    /// The check ran and its control read (under 60h) was right.
    pub checked: bool,
    /// Bit (value >> 6) * 4 + mirror_c * 2 + mirror_r: that mapping
    /// returned all marks.
    pub matches: u16,
    /// The columns way already said once that it refuses to run.
    pub refusal_logged: bool,
}

impl Orient {
    pub const fn new() -> Self {
        Self {
            checked: false,
            matches: 0,
            refusal_logged: false,
        }
    }

    /// The check confirmed this mapping.
    pub fn confirms(&self, config: Config) -> bool {
        self.checked && self.matches & config.bit() != 0
    }
}

// ---- sending ---------------------------------------------------------------

/// The rectangle actually written: 12-bit needs whole groups of 4 pixels
/// down each column (3 words), so it is widened as `to_quads` widens
/// rows.
pub fn written(rect: Rect, depth: Depth) -> Rect {
    if depth == Depth::Rgb444 {
        rect.to_quads_y()
    } else {
        rect
    }
}

/// The strips of a window: how many logical columns each holds.
#[derive(Clone, Copy)]
struct Strips {
    first: usize,
    second: usize,
    full: usize,
    cols: usize,
}

impl Strips {
    fn new(depth: Depth, cols: usize, h: usize) -> Strips {
        // Two gate lines a logical column.
        let col = 2 * pipeline::col_bytes(depth, h);
        let full = (STRIP_BYTES / col).max(1);
        let first = (FIRST_STRIP_BYTES / col).clamp(1, full);
        let second = (SECOND_STRIP_BYTES / col).clamp(1, full);
        Strips {
            first,
            second,
            full,
            cols,
        }
    }

    /// First column (counted in sending order) of strip `k`, clamped.
    fn start(&self, k: usize) -> usize {
        let s = match k {
            0 => 0,
            1 => self.first,
            _ => self.first + self.second + (k - 2) * self.full,
        };
        s.min(self.cols)
    }

    fn count(&self) -> usize {
        if self.cols <= self.first {
            1
        } else if self.cols <= self.first + self.second {
            2
        } else {
            2 + (self.cols - self.first - self.second).div_ceil(self.full)
        }
    }
}

/// Sends a rectangle of the logical screen transposed, as one window
/// under `config`. Every strip is whole logical columns, so its length
/// is a multiple of 4 bytes (16-bit: 8h bytes a column; 12-bit: 6h with
/// h a multiple of 4).
pub fn present_rect(
    lcd: &mut Lcd,
    fb: &Frame,
    pal: &Palette,
    rect: Rect,
    config: Config,
    how: Send,
) -> Result<Timing, LcdError> {
    let depth = lcd.depth();
    let rect = written(rect, depth);
    let (x0, cols, y0, h) = (
        usize::from(rect.x),
        usize::from(rect.w),
        usize::from(rect.y),
        usize::from(rect.h),
    );
    let strips = Strips::new(depth, cols, h);
    lcd.draw_in(
        config.madctl,
        config.window(rect),
        strips.count(),
        how,
        |k, buf| {
            let (a, b) = (strips.start(k), strips.start(k + 1));
            // Sending order is right to left when rows are mirrored: strip
            // columns a..b counted from the right edge.
            let first = if config.mirror_r {
                x0 + cols - b
            } else {
                x0 + a
            };
            pipeline::fill_cols(
                depth,
                fb,
                pal,
                first,
                b - a,
                y0,
                h,
                config.mirror_r,
                config.mirror_c,
                buf,
            )
        },
    )
}

// ---- the start-up check ----------------------------------------------------

/// Marks written under 60h: (x, y) on the panel and an RGB565 colour.
/// Near three corners, at offsets that no flip maps onto another mark,
/// in three colours, on black: only the right mapping reads all three
/// back. Pure red, green and blue read back exactly (6 bits a channel).
const MARKS: [(u16, u16, u16); 3] = [(3, 5, 0xF800), (310, 9, 0x07E0), (17, 228, 0x001F)];

/// Which MV=0 MADCTL value, with which mirrors, maps interface (column,
/// row) = (y, x) onto landscape pixel (x, y)? Covers the panel with
/// black, writes [`MARKS`] under 60h, reads them back under 60h (the
/// control), then for each of 00h, 40h, 80h, C0h and each pair of
/// mirrors reads the transposed addresses. MADCTL only changes how
/// interface addresses map to memory, so the reads see where the data
/// really is. Reads as `check.rs` does (16-bit, slow clock, a dummy
/// byte, 3 bytes a pixel). Prints one `bench=tear_orient` line. Leaves
/// 60h, the picture black with three dots (the caller sends a picture
/// after). About 25 ms.
pub fn check(lcd: &mut Lcd, scratch: &mut [u8]) -> Orient {
    let depth = lcd.depth();
    let _ = lcd.set_depth(Depth::Rgb565);
    let mut out = Orient::new();
    let mut errors = 0u32;

    // Black, then the marks, all under 60h.
    let frame_bytes = W * H * 2;
    let black = lcd.draw(
        (0, 0, W as u16 - 1, H as u16 - 1),
        frame_bytes.div_ceil(STRIP_BYTES),
        Send::Overlap,
        |k, buf| {
            let len = buf.len().min(frame_bytes - k * STRIP_BYTES);
            buf[..len].fill(0);
            len
        },
    );
    errors += u32::from(black.is_err());
    for (x, y, colour) in MARKS {
        // One pixel is 2 bytes; DMA wants whole words, so the window is
        // two pixels wide and both get the mark's colour. Only the left
        // one is read; no mirror maps either onto another mark.
        let r = lcd.draw((x, y, x + 1, y), 1, Send::Overlap, |_, buf| {
            let [a, b] = colour.to_be_bytes();
            buf[..4].copy_from_slice(&[a, b, a, b]);
            4
        });
        errors += u32::from(r.is_err());
    }

    // Reads one pixel at interface (c, r) under `madctl`.
    let mut read = |lcd: &mut Lcd, madctl: u8, c: u16, r: u16| -> Option<u16> {
        match lcd.read_row_in(madctl, c, r, 1, scratch) {
            Ok(px) => Some(lcd::read_to_565(px)),
            Err(_) => None,
        }
    };

    let mut control = true;
    for (x, y, colour) in MARKS {
        match read(lcd, MADCTL_LANDSCAPE, x, y) {
            Some(got) => control &= got == colour,
            None => {
                errors += 1;
                control = false;
            }
        }
    }
    out.checked = control;
    for value in [0x00u8, 0x40, 0x80, 0xC0] {
        for mirror_c in [false, true] {
            for mirror_r in [false, true] {
                let config = Config {
                    madctl: value,
                    mirror_c,
                    mirror_r,
                };
                let mut all = true;
                for (x, y, colour) in MARKS {
                    let (c, r) = config.address(x, y);
                    match read(lcd, value, c, r) {
                        Some(got) => all &= got == colour,
                        None => {
                            errors += 1;
                            all = false;
                        }
                    }
                }
                if all {
                    out.matches |= config.bit();
                }
            }
        }
    }
    // Back to landscape for everything else (the next window would do
    // it anyway).
    let _ = lcd.set_madctl(MADCTL_LANDSCAPE);
    let _ = lcd.set_depth(depth);

    // The value that needs no mirror at all, if any: the one to use.
    let plain = [0x00u8, 0x40, 0x80, 0xC0].into_iter().find(|&v| {
        out.matches
            & Config {
                madctl: v,
                mirror_c: false,
                mirror_r: false,
            }
            .bit()
            != 0
    });
    let a = Config::for_direction(false);
    let b = Config::for_direction(true);
    info!(
        "bench=tear_orient control_ok={} errors={errors} matches={} use={} configured=0x{:02x} mirror_c={} confirmed_a={} confirmed_b={} madctl_now={}",
        control,
        Matches(out.matches),
        plain.map_or(Hex(None), |v| Hex(Some(v))),
        MADCTL_COLS,
        MIRROR_C,
        out.confirms(a),
        out.confirms(b),
        Hex(lcd.madctl()),
    );
    if !out.confirms(a) {
        warn!(
            "cols: the start-up check did not confirm MADCTL {MADCTL_COLS:02x}h (direction A); the columns way falls back to slices_locked"
        );
    }
    out
}

/// A MADCTL value for the log, or `unknown`.
struct Hex(Option<u8>);

impl core::fmt::Display for Hex {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Some(v) => write!(f, "0x{v:02x}"),
            None => write!(f, "none"),
        }
    }
}

/// The match mask as a list: `40h:none,00h:c,...` (mirrors: `c` column,
/// `r` row, `cr` both, `none`), or `none` when nothing matched.
struct Matches(u16);

impl core::fmt::Display for Matches {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.0 == 0 {
            return write!(f, "none");
        }
        let mut first = true;
        for bit in 0..16u16 {
            if self.0 & (1 << bit) == 0 {
                continue;
            }
            let value = (bit / 4) << 6;
            let mirrors = match bit % 4 {
                0 => "none",
                1 => "r",
                2 => "c",
                _ => "cr",
            };
            if !first {
                write!(f, ",")?;
            }
            first = false;
            write!(f, "{value:02x}h:{mirrors}")?;
        }
        Ok(())
    }
}
