//! The boot screen: a little handheld drawn row by row, the console's
//! name dropping in letter by letter, the handheld's screen lighting up
//! and a gold line growing under the name. About 1.25 s of animation,
//! 1.5 s in all ([`BOOT_US`]). Any button or a tap skips it.
//!
//! Each frame changes only a few small spots (a row of the icon, one or
//! two letters, a few pixels of the line), so it is cheap to send.

use console_video::Frame;

use crate::gfx::{self, CX, HANDHELD, HANDHELD_SCREEN};
use crate::input::Input;
pub use crate::theme::BOOT_US;
use crate::theme::{
    BOOT_DROP_US, BOOT_ICON_US, BOOT_LETTER_GAP, BOOT_LETTERS_AT, BOOT_LIGHT_AT, BOOT_LINE_US,
    GOLD, INK, OK, PAPER,
};

/// The name on the boot screen: the console's name (`theme::NAME`).
pub use crate::theme::NAME;

/// Where things sit on the boot screen.
const ICON_K: u16 = 2;
const ICON_X: i16 = CX - (HANDHELD.w * ICON_K) as i16 / 2;
const ICON_Y: i16 = 18;
const NAME_Y: i16 = 64;
const NAME_SCALE: u8 = 2;
/// How far a letter falls before it rests.
const DROP_PX: u64 = 4;
const LINE_Y: i16 = NAME_Y + 14;

#[derive(Default)]
pub struct Boot {
    start_us: Option<u64>,
}

impl Boot {
    pub const fn new() -> Self {
        Boot { start_us: None }
    }

    fn elapsed(&self, now_us: u64) -> u64 {
        now_us.saturating_sub(self.start_us.unwrap_or(now_us))
    }

    /// One frame. Returns true when the boot screen is finished: its time
    /// is over, or a button or a tap skipped it.
    pub fn update(&mut self, now_us: u64, input: &Input) -> bool {
        self.start_us.get_or_insert(now_us);
        input.any() || self.elapsed(now_us) >= BOOT_US
    }

    /// True while something still moves; after that the screen stays
    /// still until [`Boot::end_us`].
    pub fn moving(&self, now_us: u64) -> bool {
        self.elapsed(now_us) < BOOT_LIGHT_AT + BOOT_LINE_US
    }

    /// When the boot screen ends by itself.
    pub fn end_us(&self) -> u64 {
        self.start_us.unwrap_or(0) + BOOT_US
    }

    pub fn draw(&self, now_us: u64, f: &mut Frame) {
        let t = self.elapsed(now_us);
        f.clear(INK);

        // 1. The handheld, row by row.
        let rows = ((t + 1) * u64::from(HANDHELD.h)).div_ceil(BOOT_ICON_US) as u16;
        HANDHELD.draw(f, ICON_X, ICON_Y, ICON_K, rows, INK);
        if t >= BOOT_LIGHT_AT {
            let (sx, sy, sw, sh) = HANDHELD_SCREEN;
            let (x, y) = (ICON_X + (sx * ICON_K) as i16, ICON_Y + (sy * ICON_K) as i16);
            f.rect(x, y, sw * ICON_K, sh * ICON_K, OK);
            // A glint in the corner.
            f.rect(x, y, ICON_K, ICON_K, PAPER);
        }

        // 2. The name, one letter at a time, each dropping into place.
        let full_w = gfx::width(NAME, NAME_SCALE);
        let mut x = CX - full_w / 2;
        let mut buf = [0u8; 4];
        for (i, ch) in NAME.chars().enumerate() {
            let s: &str = ch.encode_utf8(&mut buf);
            let at = BOOT_LETTERS_AT + i as u64 * BOOT_LETTER_GAP;
            let next_x = x + gfx::width(s, NAME_SCALE) + i16::from(NAME_SCALE);
            if t >= at {
                let e = t - at;
                let lift = if e < BOOT_DROP_US {
                    (DROP_PX - DROP_PX * e / BOOT_DROP_US) as i16
                } else {
                    0
                };
                f.text_scaled(x, NAME_Y - lift, s, PAPER, NAME_SCALE);
            }
            x = next_x;
        }

        // 3. A gold line that grows from the middle under the name.
        if t >= BOOT_LIGHT_AT {
            let e = (t - BOOT_LIGHT_AT).min(BOOT_LINE_US);
            let half = (full_w as u64 / 2 * e / BOOT_LINE_US) as i16;
            if half > 0 {
                f.hline(CX - half, LINE_Y, (2 * half) as u16, GOLD);
            }
        }
    }
}
