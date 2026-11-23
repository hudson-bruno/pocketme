//! Opening a game: calling its entry function and reading its table.
//!
//! Boards use this (the chip's loader, the simulator, the fake board), so
//! the one place that reads a table the game made is written once. The
//! console itself then holds a plain copy of the table, and needs no
//! `unsafe` at all.

use core::mem::offset_of;

use crate::{
    ConsoleApi, ExitFn, GameCallbacks, GameEntry, GameFrame, MIN_GAME_VERSION, UpdateFn,
    abort_code, field_end,
};

/// Why a game's table was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenError {
    /// The entry function gave no table (null), or a crooked pointer.
    NoTable,
    /// The table is too small to hold even `update`.
    TooSmall,
    /// The game was built for a version below [`MIN_GAME_VERSION`]
    /// (1 or 2, or says 0).
    TooOld,
}

/// A game linked into a PC program (the simulator, tests): its folder
/// name on the card and its entry function. The simulator keeps a list
/// of these and looks the folder up in it.
///
/// Making one is `unsafe` (see [`LinkedGame::new`]); opening it is not.
#[derive(Clone, Copy, Debug)]
pub struct LinkedGame {
    /// The folder under `/CONSOLE/GAMES` (8 letters at most).
    pub folder: &'static str,
    entry: GameEntry,
}

impl LinkedGame {
    /// A linked game: its folder and its entry function.
    ///
    /// ```
    /// # use game_api::{ConsoleApi, GameCallbacks, LinkedGame};
    /// # unsafe extern "C" fn game_entry(_: *const ConsoleApi) -> *const GameCallbacks {
    /// #     core::ptr::null()
    /// # }
    /// static GAMES: &[LinkedGame] = &[
    ///     // SAFETY: made by `game_kit::game!`.
    ///     unsafe { LinkedGame::new("MYGAME", game_entry) },
    /// ];
    /// ```
    ///
    /// # Safety
    ///
    /// `entry` keeps the contract: given a real console table, it returns
    /// null or a pointer to a game table that stays readable and unchanged
    /// while the program runs, at least as long as the table's `size`
    /// says, with functions that keep the contract. The kit's `game!`
    /// macro makes such a function.
    pub const unsafe fn new(folder: &'static str, entry: GameEntry) -> LinkedGame {
        LinkedGame { folder, entry }
    }

    /// The entry function.
    pub fn entry(&self) -> GameEntry {
        self.entry
    }

    /// Opens the game: [`open_game`] with its entry function.
    pub fn open(&self, api: &'static ConsoleApi) -> Result<GameCallbacks, OpenError> {
        // SAFETY: `new` was promised a function that keeps the contract.
        unsafe { open_game(self.entry, api) }
    }
}

/// Calls a game's entry function with `api` and copies its table.
///
/// It is `unsafe`: it trusts the pointer the entry function returns, so
/// safe code cannot hand it a function that returns a made-up one:
///
/// ```compile_fail,E0133
/// use game_api::{ConsoleApi, GameCallbacks, UNWIND_CONSOLE_API, open_game};
/// extern "C" fn liar(_: *const ConsoleApi) -> *const GameCallbacks {
///     8 as *const GameCallbacks
/// }
/// let _ = open_game(liar, &UNWIND_CONSOLE_API);
/// ```
///
/// A [`LinkedGame`] can be opened from safe code; making one is the
/// `unsafe` step.
///
/// # Safety
///
/// `entry` keeps the contract, as [`LinkedGame::new`] says.
///
/// The table may be smaller than this console's [`GameCallbacks`] (an
/// older game): missing functions are filled with ones that do nothing
/// ([`ExitFn::NONE`]). It may be larger (a newer game): the rest is ignored.
/// `size` and `version` in the result are the game's own.
///
/// What it checks before it reads: null, alignment, size and version.
/// A pointer that is not null but points at nothing cannot be checked;
/// that is why the function is `unsafe`.
pub unsafe fn open_game(
    entry: GameEntry,
    api: &'static ConsoleApi,
) -> Result<GameCallbacks, OpenError> {
    // SAFETY: the caller promises `entry` keeps the contract, and `api`
    // is a real console table that lives for the whole program.
    let table = unsafe { entry(api) };
    if table.is_null()
        || !table
            .cast::<u8>()
            .addr()
            .is_multiple_of(align_of::<GameCallbacks>())
    {
        return Err(OpenError::NoTable);
    }
    let base = table.cast::<u8>();
    // SAFETY: the game says `table` points at its table, which starts with
    // `size` and `version` (8 bytes in every version); the caller promised
    // the game keeps its word.
    let (size, version) = unsafe {
        (
            base.add(offset_of!(GameCallbacks, size))
                .cast::<u32>()
                .read(),
            base.add(offset_of!(GameCallbacks, version))
                .cast::<u32>()
                .read(),
        )
    };
    if version < MIN_GAME_VERSION {
        return Err(OpenError::TooOld);
    }
    let size_ok = |end: usize| size as usize >= end;
    if !size_ok(field_end!(GameCallbacks, update)) {
        return Err(OpenError::TooSmall);
    }
    // SAFETY: the game's `size` says these bytes are its table; each
    // field is read only when `size` covers it, at its own offset. The
    // caller promised the functions keep the contract, which is what
    // `UpdateFn` and `ExitFn` ask of whoever makes one.
    let update = unsafe {
        base.add(offset_of!(GameCallbacks, update))
            .cast::<UpdateFn>()
            .read()
    };
    let exit = if size_ok(field_end!(GameCallbacks, exit)) {
        // SAFETY: as above.
        unsafe {
            base.add(offset_of!(GameCallbacks, exit))
                .cast::<ExitFn>()
                .read()
        }
    } else {
        ExitFn::NONE
    };
    Ok(GameCallbacks {
        size,
        version,
        update,
        exit,
    })
}

/// An `exit` that does nothing: stands in for a game whose table has none.
pub extern "C" fn no_exit(_frame: *mut GameFrame, _reason: u32) {}

/// An `abort` for PC boards: it panics with the code and detail. The
/// simulator and the tests use it (in [`UNWIND_CONSOLE_API`]). On the PC
/// the game kit catches panics before they get this far, so it is only
/// reached by a game that calls `abort` itself.
pub extern "C-unwind" fn abort_by_panic(code: u32, detail: u32) -> ! {
    let what = match code {
        abort_code::PANIC => "panic",
        abort_code::BAD_API => "console tables too old",
        _ => "unknown reason",
    };
    panic!("game aborted: {what} (code {code}, detail {detail})")
}

/// A ready [`ConsoleApi`] for PC boards, with [`abort_by_panic`].
pub static UNWIND_CONSOLE_API: ConsoleApi = ConsoleApi {
    size: size_of::<ConsoleApi>() as u32,
    version: crate::API_VERSION,
    abort: abort_by_panic,
};
