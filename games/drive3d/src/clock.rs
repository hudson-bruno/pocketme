//! The fixed step's clock (the spec's section 0).
//!
//! Each update adds the console's `delta_us` to a store and runs one
//! step for each whole `STEP_US` in it, at most `MAX_STEPS`; the rest
//! waits. The store starts half full, so updates that come a little
//! early or late still run one step each. After an update that brought
//! more time than `MAX_STEPS` steps can use (a stall, a slow picture,
//! the first frames after loading) the backlog is dropped and the store
//! is set to half a step again: a long stall does not make the game race
//! to catch up, and the half-step phase comes back, so jittered updates
//! do not run 0, 2, 0, 2 steps afterwards (review F1).

use crate::tune::{MAX_STEPS, STEP_US};

/// The time store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    /// Microseconds not yet run as steps.
    pub store: u32,
}

impl Default for Clock {
    fn default() -> Clock {
        Clock::new()
    }
}

impl Clock {
    /// Half a step in the store.
    pub const fn new() -> Clock {
        Clock { store: STEP_US / 2 }
    }

    /// Adds `delta_us`; returns how many steps to run now (0 to
    /// `MAX_STEPS`).
    pub fn add(&mut self, delta_us: u32) -> u8 {
        self.store = self.store.saturating_add(delta_us);
        let mut n = 0u8;
        while n < MAX_STEPS && self.store >= STEP_US {
            self.store -= STEP_US;
            n += 1;
        }
        if self.store >= STEP_US {
            // Only after MAX_STEPS ran: drop the backlog, keep the phase.
            self.store = STEP_US / 2;
        }
        n
    }
}
