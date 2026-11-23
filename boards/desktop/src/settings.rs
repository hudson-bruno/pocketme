//! The console's settings kept in the board's store, as the real board
//! will keep them (crates/settings-store): read once at start, written
//! [`settings_store::QUIET_US`] after the player's last change.
//!
//! [`Kept`] wraps the console: it is what the loop steps instead of the
//! console itself (`&mut kept` for a script run). With `--store FILE` the brightness (and, with
//! the feature `boot-gather`, the boot animation) survives a restart of
//! the simulator.
//!
//! Like the board, it saves only after the quiet time: a run that ends
//! within 2 s of a change does not keep that change (a board that loses
//! power then does not either).

use board_api::Wake;
use board_api::link::LogOut;
use console_core::Console;
use settings_store::{SaveTimer, Saved, SettingsStore};

use crate::board::DesktopBoard;
use crate::run::Steps;

/// Where the settings start in the store (they use 2 sectors, 8 KB, of
/// the 16 KB). The simulator keeps nothing else there.
pub const SETTINGS_BASE: u32 = 0;

/// How the boot animation plays, as `--boot` names it (feature
/// `boot-gather`; see [`set_boot`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boot {
    Full,
    Short,
    Off,
}

impl Boot {
    /// `full`, `short` or `off`.
    pub fn parse(word: &str) -> Option<Boot> {
        match word {
            "full" => Some(Boot::Full),
            "short" => Some(Boot::Short),
            "off" => Some(Boot::Off),
            _ => None,
        }
    }

    /// Its word in the log, as the console's menu writes it.
    pub fn word(self) -> &'static str {
        match self {
            Boot::Full => "FULL",
            Boot::Short => "SHORT",
            Boot::Off => "OFF",
        }
    }
}

/// Gives the console how its boot animation plays, before the first
/// step. Returns false when this build has no boot animation (without
/// the feature `boot-gather` the console has only its plain boot screen;
/// nothing changes then).
///
/// Tests about home and the games use `Boot::Off`: then they run the
/// same with and without the feature, apart from the boot itself.
pub fn set_boot(console: &mut Console, boot: Boot) -> bool {
    #[cfg(feature = "boot-gather")]
    {
        use console_core::boot::Anim;
        console.set_boot_anim(match boot {
            Boot::Full => Anim::Full,
            Boot::Short => Anim::Short,
            Boot::Off => Anim::Off,
        });
        true
    }
    #[cfg(not(feature = "boot-gather"))]
    {
        let _ = (console, boot);
        false
    }
}

/// The console, with its settings loaded from the store and saved back.
pub struct Kept<'a> {
    pub console: &'a mut Console,
    pub store: SettingsStore,
    pub timer: SaveTimer,
    /// Records written (for tests and the log).
    pub saves: u32,
}

impl<'a> Kept<'a> {
    /// Reads the settings from the board's store and gives them to the
    /// console. Call before the first step.
    pub fn start(console: &'a mut Console, board: &mut DesktopBoard) -> Kept<'a> {
        let mut store = SettingsStore::new(SETTINGS_BASE);
        let settings = store.load(&mut board.store);
        console.set_brightness(settings.brightness);
        // The boot animation: the saved one. With nothing saved the
        // console keeps its own (a new console: FULL, the same as a
        // blank store would say).
        #[cfg(feature = "boot-gather")]
        if store.found() {
            let anim = console_core::boot::Anim::from_byte(settings.boot_anim());
            console.set_boot_anim(anim);
        }
        let line = format!(
            "settings: brightness {} ({})",
            settings.brightness,
            if store.found() { "saved" } else { "default" }
        );
        board.log.line(&line);
        #[cfg(feature = "boot-gather")]
        board.log.line(&format!(
            "settings: boot anim {}",
            console.boot_anim().word()
        ));
        Kept {
            console,
            store,
            timer: SaveTimer::new(),
            saves: 0,
        }
    }

    /// `--boot`: how this start's boot animation plays, over the saved
    /// value, with a log line saying so. Call before the first step.
    /// False (and no line) when this build has no boot animation.
    pub fn boot_option(&mut self, board: &mut DesktopBoard, boot: Boot) -> bool {
        let done = set_boot(self.console, boot);
        if done {
            let line = format!("settings: boot anim {} (--boot)", boot.word());
            board.log.line(&line);
        }
        done
    }

    /// One console step; then the settings are saved if they are due.
    /// The answer wakes the board in time for a save that waits.
    pub fn step(&mut self, board: &mut DesktopBoard) -> Wake {
        let wake = self.console.step(board);
        let now = board.clock.now_us;
        if self.console.take_settings_changed() {
            self.timer.changed(now);
        }
        if self.timer.due(now) {
            self.save(board, now);
        }
        match (wake, self.timer.deadline()) {
            (Wake::At(t), Some(d)) => Wake::At(t.min(d)),
            (Wake::InputOr(t), Some(d)) => Wake::InputOr(t.min(d)),
            (w, None) => w,
        }
    }

    fn save(&mut self, board: &mut DesktopBoard, now: u64) {
        let new = self
            .store
            .current()
            .with_brightness(self.console.brightness());
        #[cfg(feature = "boot-gather")]
        let new = new.with_boot_anim(self.console.boot_anim().byte());
        let line = match self.store.save(&mut board.store, &new) {
            Ok(Saved::Unchanged) => return,
            Ok(Saved::Written { erased }) => {
                self.saves += 1;
                format!(
                    "settings: saved brightness {} (record {} of {}{})",
                    new.brightness,
                    self.store.used(),
                    settings_store::SLOTS,
                    if erased { ", erased a sector" } else { "" }
                )
            }
            Err(e) => {
                // Try again after another quiet time.
                self.timer.changed(now);
                format!("settings: not saved: {e:?}")
            }
        };
        board.log.line(&line);
    }
}

impl Steps for &mut Kept<'_> {
    fn step(&mut self, board: &mut DesktopBoard) -> Wake {
        Kept::step(self, board)
    }

    fn notify(&mut self, text: &str, now_us: u64) -> bool {
        self.console.notify(text, now_us)
    }
}
