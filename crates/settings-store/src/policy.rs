//! When a board with real flash saves: the rules, apart from the flash,
//! so they are tested on the PC.
//!
//! On the CYD a flash write or erase turns every interrupt off and stops
//! the loop (a record write well under a millisecond, an erase tens of
//! milliseconds). So the board saves:
//!
//! - [`QUIET_US`](crate::QUIET_US) after the last change (a finger sliding along the
//!   brightness bars makes one record, not five), or
//! - at once when a menu closes with a change waiting (the player is
//!   done; the moment is between two menu frames, not in a game);
//! - never while a game runs unpaused: a save that comes due then waits
//!   until the game is paused, or left, or the menu closes over it.
//! - A save that fails is tried again after another quiet time, at most
//!   [`TRIES_AFTER_FAIL`] times in a row; then it waits for the next
//!   change. A failure never stops the console.
//!
//! "Not more than needed" is the store's own rule as well:
//! [`crate::SettingsStore::save`] writes nothing when the flash already
//! holds the same settings (a change and a change back).

use crate::SaveTimer;

/// Saves tried after the first one failed, before the board waits for
/// the next change.
pub const TRIES_AFTER_FAIL: u8 = 2;

/// What the console was doing in the step just run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Moment {
    /// The settings changed in this step (`take_settings_changed`).
    pub changed: bool,
    /// A menu is open: the home screen's menu, or a game's pause menu.
    pub menu: bool,
    /// A game runs and is not paused (full-rate game frames).
    pub game_running: bool,
}

/// The save rules of a board. 24 bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SavePolicy {
    timer: SaveTimer,
    menu_was_open: bool,
    /// Failed saves in a row since the last change.
    fails: u8,
}

impl SavePolicy {
    pub const fn new() -> Self {
        SavePolicy {
            timer: SaveTimer::new(),
            menu_was_open: false,
            fails: 0,
        }
    }

    /// Call after every console step: true when the board should save
    /// now. (Then call [`SavePolicy::failed`] if the save did not work.)
    pub fn after_step(&mut self, now_us: u64, m: Moment) -> bool {
        if m.changed {
            self.timer.changed(now_us);
            self.fails = 0;
        }
        let closed = self.menu_was_open && !m.menu;
        self.menu_was_open = m.menu;
        if !self.timer.pending() || (m.game_running && !closed) {
            return false;
        }
        if closed {
            self.timer = SaveTimer::new();
            return true;
        }
        self.timer.due(now_us)
    }

    /// The save just asked for did not work: try again after another
    /// quiet time, unless it already failed too often.
    pub fn failed(&mut self, now_us: u64) {
        if self.fails < TRIES_AFTER_FAIL {
            self.fails += 1;
            self.timer.changed(now_us);
        }
    }

    /// When the board should wake to save, if a save waits. None while a
    /// game runs (its frames wake the board anyway, and it does not save
    /// then).
    pub fn deadline(&self, game_running: bool) -> Option<u64> {
        if game_running {
            None
        } else {
            self.timer.deadline()
        }
    }

    /// Whether a save waits.
    pub fn pending(&self) -> bool {
        self.timer.pending()
    }
}
