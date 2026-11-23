//! The START menu: LOW, MID, HIGH, MAX and RESTART. While it is open the
//! run stands still (no step runs, no time is kept).
//!
//! UP and DOWN move, A or START takes the line, B closes the menu.

use crate::detail::Detail;
use game_kit::button;

/// The menu while it is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Menu {
    /// The marked line (0 to 4).
    pub at: u8,
}

/// What a press in the menu asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Nothing yet: the menu stays open.
    Stay,
    /// Close the menu, change nothing.
    Close,
    /// Close it and draw at this level.
    Level(Detail),
    /// Close it and start the run again.
    Restart,
}

impl Menu {
    pub const LINES: [&'static str; 5] = ["LOW", "MID", "HIGH", "MAX", "RESTART"];

    /// Opens on the level in use.
    pub fn open(now: Detail) -> Menu {
        Menu { at: now as u8 }
    }

    /// One update's new presses.
    pub fn press(&mut self, pressed: u32) -> Choice {
        let n = Self::LINES.len() as u8;
        if pressed & button::UP != 0 {
            self.at = (self.at + n - 1) % n;
        }
        if pressed & button::DOWN != 0 {
            self.at = (self.at + 1) % n;
        }
        if pressed & (button::A | button::START) != 0 {
            return match self.at {
                0 => Choice::Level(Detail::Low),
                1 => Choice::Level(Detail::Mid),
                2 => Choice::Level(Detail::High),
                3 => Choice::Level(Detail::Max),
                _ => Choice::Restart,
            };
        }
        if pressed & button::B != 0 {
            return Choice::Close;
        }
        Choice::Stay
    }
}
