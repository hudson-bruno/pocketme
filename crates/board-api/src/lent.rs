//! Lent RAM: memory the board lends the console while no game is
//! loaded ([`crate::Board::parts_and_lent`]).
//!
//! Why: the chip has one data slot for games (48 KB, see
//! `game_api::layout`). While the player is on the home screen no game
//! is loaded and the slot is empty; while a game runs, the home screen
//! is not shown. So the home screen keeps its game list and its tile
//! pictures in the slot, and the console's own RAM is that much smaller.
//!
//! The rules, for every board that lends RAM:
//!
//! 1. Lent only while no game is loaded: before the first
//!    [`crate::Board::start_game`], after [`crate::Board::end_game`], and
//!    after a `start_game` that returned an error. While a game is
//!    loaded (from a good start to `end_game`) the board lends nothing.
//! 2. A call to `start_game` ends the loan, whatever it returns: the
//!    bytes may be overwritten (the chip's loader writes the game into
//!    the very same RAM, and wipes it again after a load that failed).
//!    The console never trusts anything it wrote there before.
//! 3. Between two loans with no `start_game` in between, the board gives
//!    the same bytes back, unchanged.
//! 4. The first loan's bytes are anything (the chip: zeros after its
//!    start-up wipe; the simulator and the fake board: rubbish, on
//!    purpose, so a console that reads before it writes is caught).
//! 5. Always the same length. The console says how much it needs
//!    (`console_core::home::LENT_BYTES`); a shorter loan is not used.
//!
//! Plain bytes, not a typed value: the console has no `unsafe`, and any
//! byte pattern is a valid `[u8]`. So rubbish in the RAM can make the
//! console show wrong text at worst, never crash it; and the console
//! checks a stamp of its own before it reads (see console-core's
//! `lent` module).
//!
//! A board that lends nothing (the default) still works: the console then
//! has no room for a game list and the home screen shows no games.
