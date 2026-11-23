//! Starting games: what the console asks the board when the player picks
//! a game on the home screen.
//!
//! The board finds the game by its folder name and hands back the game's
//! two functions ([`GameCallbacks`]). On the chip that is the loader: it
//! reads `/CONSOLE/GAMES/<FOLDER>/GAME.BIN` into the game slots and calls
//! the game's entry function. On the PC the games are linked into the
//! simulator, and the board looks the folder up in a list
//! ([`game_api::LinkedGame`], opened with its `open`). Either way,
//! [`game_api::open_game`] reads the game's table for the board.
//!
//! These are methods of [`crate::Board`] with default bodies, so a board
//! that knows nothing of games compiles unchanged: it simply cannot start
//! one ([`StartError::NotSupported`]).

pub use game_api::{ConsoleApi, GameCallbacks, GameEntry, GameFrame, OpenError};

/// Why a game could not start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartError {
    /// This board cannot start games (yet).
    NotSupported,
    /// No game in that folder (or the card is gone).
    NotFound,
    /// The game file is damaged, or built for other slots.
    BadFile,
    /// The game's table was refused (see [`OpenError`]).
    BadTable(OpenError),
    /// The board refused the game file, and says why in a short notice
    /// for the player: capitals, digits and spaces, at most 24 letters
    /// (the chip's loader gives one per refusal, such as "GAME FOR OTHER
    /// CODE SLOT"). The console shows it as it is. Added for the CYD's
    /// loader (board round 3).
    Refused(&'static str),
}

impl From<OpenError> for StartError {
    fn from(e: OpenError) -> Self {
        StartError::BadTable(e)
    }
}
