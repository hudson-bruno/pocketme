//! The board layer: everything the console may ask of the hardware.
//!
//! One small trait per kind of part (screen, touch, keys, pad, clock,
//! files, link, log, store), gathered by one [`Board`] trait. Optional
//! parts (classic Bluetooth, pass-by, GPS, sound) exist only when their
//! Cargo feature is on.
//!
//! The console is generic over `B: Board`: the compiler makes one copy of
//! the console for the board it is built for. No `dyn`, no heap, no cost
//! on the chip. This crate has no `std`, and depends only on `game-api`
//! (plain `core`, no dependencies of its own), so the very same file
//! builds for the ESP32 and for the PC.
//!
//! Starting games ([`Board::start_game`]), pad motion ([`Pad::motion`])
//! and lent RAM ([`Board::parts_and_lent`], see [`lent`]) were added
//! later, as methods with default bodies: a board written before them
//! compiles unchanged.
//!
//! The README explains the three main choices: the screen line, how the
//! console borrows several parts at once, and the per-frame contract.

#![no_std]
#![forbid(unsafe_code)]

pub mod absent;
pub mod files;
pub mod game;
pub mod geom;
pub mod input;
pub mod lent;
pub mod link;
pub mod optional;
pub mod screen;
pub mod time;

#[cfg(feature = "testing")]
pub mod fake;

pub use absent::Absent;
pub use files::{DirEntry, Files, FsError};
pub use game::{GameCallbacks, StartError};
pub use geom::Rect;
pub use input::{Keys, Motion, Pad, PadState, Stick, Touch, TouchPoint, btn, key};
pub use link::{Link, LogOut, Store, StoreError};
#[cfg(feature = "sound")]
pub use optional::Audio;
#[cfg(feature = "passby")]
pub use optional::Beacon;
#[cfg(feature = "gps")]
pub use optional::Gps;
#[cfg(feature = "bt-classic")]
pub use optional::Hci;
pub use screen::{ByteOrder, Dir, Indexed, PixelSource, Screen, SendReport};
pub use time::{Clock, Wake};

/// A board: names the part it uses for each kind, and lends them all out
/// at once through [`Board::parts`].
pub trait Board {
    type Screen: Screen;
    type Touch: Touch;
    type Keys: Keys;
    type Pad: Pad;
    type Clock: Clock;
    type Files: Files;
    type Link: Link;
    type Log: LogOut;
    type Store: Store;
    #[cfg(feature = "bt-classic")]
    type Hci: Hci;
    #[cfg(feature = "passby")]
    type Beacon: Beacon;
    #[cfg(feature = "gps")]
    type Gps: Gps;
    #[cfg(feature = "sound")]
    type Audio: Audio;

    /// Lends every part at the same time, each as its own `&mut`.
    ///
    /// A board keeps each part in its own field and writes
    /// `Parts { screen: &mut self.screen, clock: &mut self.clock, .. }`.
    /// The borrow checker accepts this because the fields are separate.
    /// The console can then read the touch screen while it holds the
    /// screen, with no `RefCell`, no `dyn` and no heap.
    fn parts(&mut self) -> Parts<'_, Self>;

    /// Starts the game in `/CONSOLE/GAMES/<folder>/` and returns its
    /// functions. The console calls them itself, one frame at a time.
    /// On the chip: load `GAME.BIN` into the slots and call its entry.
    /// On the PC: look `folder` up among the linked-in games. Use
    /// [`game_api::LinkedGame::open`] (PC) or [`game_api::open_game`]
    /// (chip) to read the game's table.
    ///
    /// Default: this board cannot start games.
    fn start_game(&mut self, folder: &str) -> Result<GameCallbacks, StartError> {
        let _ = folder;
        Err(StartError::NotSupported)
    }

    /// The game started last has ended (it is done, it aborted, or the
    /// player left it). The console never calls its functions again. A
    /// board may free what it held for it. Default: nothing to do.
    fn end_game(&mut self) {}

    /// The parts (as [`Board::parts`]), and the RAM the board lends the
    /// console while no game is loaded: on the chip, the game data slot.
    /// The rules are in [`lent`]: lent only while no game is loaded, the
    /// same bytes from one loan to the next, and every
    /// [`Board::start_game`] call ends the loan (the bytes may be gone).
    ///
    /// Default: the parts, and no lent RAM (the home screen then shows
    /// no games).
    fn parts_and_lent(&mut self) -> (Parts<'_, Self>, Option<&mut [u8]>) {
        (self.parts(), None)
    }
}

/// Every part of a board, borrowed at once. See [`Board::parts`].
pub struct Parts<'a, B: Board + ?Sized> {
    pub screen: &'a mut B::Screen,
    pub touch: &'a mut B::Touch,
    pub keys: &'a mut B::Keys,
    pub pad: &'a mut B::Pad,
    pub clock: &'a mut B::Clock,
    pub files: &'a mut B::Files,
    pub link: &'a mut B::Link,
    pub log: &'a mut B::Log,
    pub store: &'a mut B::Store,
    #[cfg(feature = "bt-classic")]
    pub hci: &'a mut B::Hci,
    #[cfg(feature = "passby")]
    pub beacon: &'a mut B::Beacon,
    #[cfg(feature = "gps")]
    pub gps: &'a mut B::Gps,
    #[cfg(feature = "sound")]
    pub audio: &'a mut B::Audio,
}
