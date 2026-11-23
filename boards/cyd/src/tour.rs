//! The tour (feature `tour`): a demo that runs by itself, so the owner
//! can watch the screen for tearing. Started and stopped by holding BOOT
//! for a second, or by the letter `t` on the serial port.
//!
//! It loops: right along all the tiles (the row scrolls once the
//! selection leaves the screen), a notification, back left, a
//! notification. One step every [`STEP_US`]. It presses buttons on the
//! cable pad, so the first press also shows "PAD CONNECTED". How far it
//! goes each way comes from the console's game list (the demo card's
//! fourteen tiles today, or whatever the card holds), read at the start of
//! each round.

use board_api::btn;
use console_core::Console;

use crate::board::CablePad;

/// Time between two tour steps.
pub const STEP_US: u64 = 700_000;
/// At most this many moves each way (a card with more games is toured
/// only this far).
const MAX_MOVES: u8 = 100;

/// Moves each way for a home row of `tiles` games: one per tile, one
/// more than the row needs from its first tile to its last, so the ends
/// are always reached (a press past the end does nothing).
fn moves_for(tiles: usize) -> u8 {
    u8::try_from(tiles).unwrap_or(MAX_MOVES).clamp(1, MAX_MOVES)
}

pub struct Tour {
    on: bool,
    next: u64,
    /// Moves each way in this round.
    moves: u8,
    /// Where in the loop: 0..moves right, moves the notice, then left.
    at: u8,
}

impl Tour {
    pub const fn new() -> Self {
        Self {
            on: false,
            next: 0,
            moves: 1,
            at: 0,
        }
    }

    pub fn on(&self) -> bool {
        self.on
    }

    /// Starts or stops; returns whether it runs now.
    pub fn toggle(&mut self, now: u64) -> bool {
        self.on = !self.on;
        self.next = now;
        self.at = 0;
        self.on
    }

    /// When the next tour step is due, while it runs.
    pub fn due(&self) -> Option<u64> {
        self.on.then_some(self.next)
    }

    /// Does the step that is due, if any.
    pub fn tick(&mut self, now: u64, pad: &mut CablePad, console: &mut Console) {
        if !self.on || now < self.next {
            return;
        }
        self.next = now + STEP_US;
        if self.at == 0 {
            self.moves = moves_for(console.home().count());
        }
        let moves = self.moves;
        let round = 2 * (moves + 1);
        let at = self.at;
        self.at = (self.at + 1) % round;
        let press = |pad: &mut CablePad, b: u32| {
            // Pressed and let go at once: the pad keeps the press for the
            // console's next read.
            pad.set(b);
            pad.set(0);
        };
        if at < moves {
            press(pad, btn::RIGHT);
        } else if at == moves {
            console.notify("TOUR: ROW DONE", now);
        } else if at < 2 * moves + 1 {
            press(pad, btn::LEFT);
        } else {
            console.notify("TOUR: BACK AT START", now);
        }
    }
}
