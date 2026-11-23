//! The game's clock and its input for one step.
//!
//! The rules run in fixed steps of 1/30 s ([`STEP_US`]). Each update adds
//! the console's `delta_us` to a store and runs one step for each whole
//! step in it, at most [`MAX_STEPS`]; the rest stays for the next update
//! (at most two steps' worth is kept, so a long stall does not make the
//! game race to catch up).

use crate::tune::{MAX_STEPS, STEP_US};

/// The time store.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Clock {
    /// Microseconds not yet turned into steps.
    pub store: u32,
}

impl Clock {
    /// The store starts half a step full, so calls that come a little
    /// early or late (jitter round 33,333 us) still run one step each.
    pub const fn new() -> Clock {
        Clock { store: STEP_US / 2 }
    }

    /// Adds `delta_us` and gives back how many steps to run now (0 to
    /// [`MAX_STEPS`]).
    pub fn add(&mut self, delta_us: u32) -> u8 {
        self.store = self.store.saturating_add(delta_us);
        let mut n = 0;
        while self.store >= STEP_US && n < MAX_STEPS {
            self.store -= STEP_US;
            n += 1;
        }
        self.store = self.store.min(STEP_US * u32::from(MAX_STEPS));
        n
    }

    /// Forgets the stored time (after the pause menu), back to half a
    /// step as at the start.
    pub fn reset(&mut self) {
        self.store = STEP_US / 2;
    }
}

/// Player 1's input for one step. `pressed` and `released` are the
/// changes since the last step (an update that runs two steps hands
/// them to the first only; one that runs none keeps them for the next).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pad {
    /// `game_kit::button` bits held now, pressed and let go.
    pub held: u32,
    pub pressed: u32,
    pub released: u32,
    /// Sticks, -32767 to 32767, y grows downwards (game-api's).
    pub left: (i16, i16),
    pub right: (i16, i16),
}

impl Pad {
    /// Nothing held.
    pub const NONE: Pad = Pad {
        held: 0,
        pressed: 0,
        released: 0,
        left: (0, 0),
        right: (0, 0),
    };

    /// The same buttons and sticks held, with no new presses or releases
    /// (what the landing ring's look ahead uses).
    pub fn held_only(&self) -> Pad {
        Pad {
            pressed: 0,
            released: 0,
            ..*self
        }
    }
}
