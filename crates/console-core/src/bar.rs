//! The bar at the bottom of the home screen. From left to right: the
//! time since start, a gamepad mark (bright when a pad is connected), the
//! battery (unknown for now: "?"), short hints for the buttons, and a
//! gear: the console menu's button (gold while the menu is open).
//!
//! There is no real clock yet, so the time is the time since the console
//! started, as hours and minutes ("0:05"). It changes once a minute, so
//! an idle screen stays still: nothing is sent, and the chip can sleep.

use console_video::Frame;

use crate::gfx::{self, GEAR_ICON, PAD_ICON};
use crate::theme::{BAR_H, BAR_Y, GEAR_X, GEAR_Y, GOLD, LINE, MUTE, OK, PANEL, PAPER};

/// One minute in microseconds.
pub const MINUTE_US: u64 = 60_000_000;

/// Which button hints to show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Hints {
    /// None (no game to choose).
    #[default]
    None,
    /// "<> MOVE   A START".
    Games,
    /// "<> MOVE   A OPEN": the SETTINGS tile is chosen (feature
    /// `settings`, placement A).
    #[cfg(feature = "settings")]
    Open,
}

/// What the bar shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// Microseconds since the console started.
    pub uptime_us: u64,
    pub pad_connected: bool,
    /// Battery level in percent, when known. No board measures it yet.
    pub battery: Option<u8>,
    pub hints: Hints,
    /// Whether the console menu is open (the gear shows gold).
    pub menu: bool,
}

/// The hints end here (exclusive), a few pixels left of the gear.
pub const HINTS_END: i16 = GEAR_X - 4;

/// When the bar's picture changes next by itself (the next minute).
pub fn next_change(start_us: u64, now_us: u64) -> u64 {
    let up = now_us.saturating_sub(start_us);
    start_us + (up / MINUTE_US + 1) * MINUTE_US
}

/// Draws the bar.
pub fn draw(status: &Status, f: &mut Frame) {
    let text_y = BAR_Y + 4;
    f.rect(0, BAR_Y, 160, BAR_H, PANEL);
    f.hline(0, BAR_Y, 160, LINE);

    // Time since start, H:MM. Fixed size text: no heap.
    let minutes = status.uptime_us / MINUTE_US;
    let (h, m) = (minutes / 60, minutes % 60);
    let mut buf = [0u8; 8];
    let time = clock_text(h.min(999) as u16, m as u8, &mut buf);
    let mut x = f.text(4, text_y, time, PAPER) + 5;

    // The pad mark.
    let c = if status.pad_connected { OK } else { LINE };
    PAD_ICON.draw(f, x, BAR_Y + 3, 1, PAD_ICON.h, c);
    x += PAD_ICON.w as i16 + 5;

    // The battery: an outline, its level inside when known, else "?".
    f.outline(x, BAR_Y + 4, 9, 5, MUTE);
    f.vline(x + 9, BAR_Y + 5, 3, MUTE);
    match status.battery {
        Some(p) => {
            let w = (u16::from(p.min(100)) * 7).div_ceil(100);
            f.rect(x + 1, BAR_Y + 5, w, 3, OK);
        }
        None => {
            f.text(x + 12, text_y, "?", MUTE);
        }
    }

    // Hints, left of the gear.
    if status.hints == Hints::Games {
        gfx::text_right(f, HINTS_END, text_y, "<> MOVE  A START", MUTE);
    }
    #[cfg(feature = "settings")]
    if status.hints == Hints::Open {
        gfx::text_right(f, HINTS_END, text_y, "<> MOVE  A OPEN", MUTE);
    }

    // The gear, at the right end.
    let c = if status.menu { GOLD } else { MUTE };
    GEAR_ICON.draw(f, GEAR_X, GEAR_Y, 1, GEAR_ICON.h, c);
}

/// Writes `h:mm` into `buf` and returns it as text.
fn clock_text(h: u16, m: u8, buf: &mut [u8; 8]) -> &str {
    let mut n = 0;
    let mut digits = [0u8; 3];
    let mut k = 0;
    let mut v = h;
    loop {
        digits[k] = b'0' + (v % 10) as u8;
        k += 1;
        v /= 10;
        if v == 0 || k == digits.len() {
            break;
        }
    }
    while k > 0 {
        k -= 1;
        buf[n] = digits[k];
        n += 1;
    }
    buf[n] = b':';
    buf[n + 1] = b'0' + m / 10;
    buf[n + 2] = b'0' + m % 10;
    n += 3;
    core::str::from_utf8(&buf[..n]).unwrap_or("?")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_text_is_hours_and_minutes() {
        let mut b = [0; 8];
        assert_eq!(clock_text(0, 5, &mut b), "0:05");
        assert_eq!(clock_text(12, 40, &mut b), "12:40");
        assert_eq!(clock_text(999, 59, &mut b), "999:59");
    }

    #[test]
    fn the_bar_changes_once_a_minute() {
        assert_eq!(next_change(1_000, 1_000), 1_000 + MINUTE_US);
        assert_eq!(next_change(0, MINUTE_US), 2 * MINUTE_US);
    }
}
