//! The game side of the console: everything a game needs, safely.
//!
//! A game is a `no_std` library crate that uses only this crate. It
//! writes one type that implements [`Game`] and names it once with
//! [`game!`]:
//!
//! ```
//! #![no_std]
//! use game_kit::{Ctx, Flow, Game, button};
//!
//! struct Hello { x: i16 }
//!
//! impl Game for Hello {
//!     fn start(_ctx: &mut Ctx) -> Self {
//!         Hello { x: 0 }
//!     }
//!     fn update(&mut self, ctx: &mut Ctx) -> Flow {
//!         if ctx.held(button::RIGHT) {
//!             self.x += 1;
//!         }
//!         let screen = ctx.screen();
//!         screen.clear(0);
//!         screen.text(self.x, 50, "HELLO", 239);
//!         if ctx.pressed(button::START) { Flow::Done } else { Flow::Run }
//!     }
//! }
//!
//! game_kit::game!(Hello);
//! # fn main() {}
//! ```
//!
//! What [`game!`] makes:
//!
//! * `pub unsafe extern "C" fn game_entry(...)`, the entry function. On
//!   the chip it keeps its name in the binary (`no_mangle`), so the game
//!   file can point at it. On the PC it is a plain function: the
//!   simulator links the game crate and lists
//!   `unsafe { LinkedGame::new(folder, the_game::game_entry) }`.
//! * The game's table and its two functions, which keep the game's
//!   state between calls.
//! * On the chip, the panic handler: a panic ends the game through the
//!   console's `abort`. On the PC a panic is caught instead, the game
//!   reports "aborted", and the program goes on.
//!
//! The state lives in a `static` on the chip (one game runs at a time)
//! and in a thread-local on the PC (so tests can run games side by side).
//!
//! `game_entry` is an `unsafe` function: it reads the table it is given,
//! so only the console (which hands a real one) may call it. Safe code
//! cannot call it with a made-up pointer:
//!
//! ```compile_fail,E0133
//! #![no_std]
//! use game_kit::{Ctx, Flow, Game};
//! struct G;
//! impl Game for G {
//!     fn start(_: &mut Ctx) -> Self { G }
//!     fn update(&mut self, _: &mut Ctx) -> Flow { Flow::Run }
//! }
//! game_kit::game!(G);
//! # extern crate std;
//! fn main() {
//!     // A pointer to nothing: must not compile without `unsafe`.
//!     let _ = game_entry(8 as *const game_kit::api::ConsoleApi);
//! }
//! ```
//!
//! The game's two functions and its state are hidden inside
//! `game_entry`, so the game's own code cannot call `update` from inside
//! `update`, or with a made-up frame:
//!
//! ```compile_fail,E0425
//! #![no_std]
//! use game_kit::{Ctx, Flow, Game};
//! struct G;
//! impl Game for G {
//!     fn start(_: &mut Ctx) -> Self { G }
//!     fn update(&mut self, _: &mut Ctx) -> Flow {
//!         // Must not compile: the name is out of reach.
//!         __game_kit_update(core::ptr::null_mut());
//!         Flow::Run
//!     }
//! }
//! game_kit::game!(G);
//! # extern crate std;
//! # fn main() {}
//! ```

#![no_std]

#[cfg(not(target_os = "none"))]
extern crate std;

mod ctx;
#[doc(hidden)]
pub mod rt;
/// A stand-in console for a game's tests (feature `testing`, PC only).
#[cfg(all(feature = "testing", not(target_os = "none")))]
pub mod testing;

pub use console_video::{CLEAR, FONT_H, Frame, H, LINE_H, Sprite, W, rgb565, text_width};
pub use ctx::{Ctx, LogLine, LogNum, Motion, Player, Refused, Touch, VideoOffers, VideoSettings};
/// The contract itself, for what this crate does not wrap.
pub use game_api as api;
pub use game_api::{
    PLAYERS, STICK_MAX, TRIGGER_MAX, answer, button, colour, hint, mode, motion as motion_units,
    rate,
};

/// What a game does. The console calls it once per frame (or, with the
/// on-demand rate, when something happened).
pub trait Game: Sized {
    /// Makes the game's state. Called in the first call of a run (the
    /// picture is then cleared and the palette holds the start colours).
    /// `update` follows at once, in the same call.
    fn start(ctx: &mut Ctx) -> Self;

    /// One frame: read the input, draw, and say whether to go on.
    fn update(&mut self, ctx: &mut Ctx) -> Flow;

    /// The console ends the game (the player chose "Back to home", or
    /// the console must stop). Drawing is useless here; logging works.
    /// Not called after [`Flow::Done`] or a panic.
    fn exit(&mut self, ctx: &mut Ctx, why: Exit) {
        let _ = (ctx, why);
    }
}

/// What [`Game::update`] returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    /// Keep going.
    Run,
    /// The game is over: the console goes back home.
    Done,
}

/// Why [`Game::exit`] is called.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// The player chose "Back to home" in the pause menu.
    Player,
    /// The console has to stop the game.
    Console,
    /// A reason newer than this kit.
    Other(u32),
}

impl Exit {
    fn from_code(code: u32) -> Exit {
        match code {
            game_api::exit_reason::PLAYER => Exit::Player,
            game_api::exit_reason::CONSOLE => Exit::Console,
            other => Exit::Other(other),
        }
    }
}

/// Makes a crate a game: `game_kit::game!(MyGame);` once, at the crate
/// root. See the crate notes for what it makes.
#[macro_export]
macro_rules! game {
    ($game:ty) => {
        /// The game's entry function (made by `game_kit::game!`).
        ///
        /// # Safety
        ///
        /// `console` is null or points at a console table that stays
        /// readable while the game runs. Only the console calls this.
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub unsafe extern "C" fn game_entry(
            console: *const $crate::api::ConsoleApi,
        ) -> *const $crate::api::GameCallbacks {
            // Everything below lives inside this function so that the
            // game's own code cannot name it: no call of `update` from
            // inside `update`, no call with a made-up frame. (Moved here
            // by the second reader, step 2.)
            #[cfg(target_os = "none")]
            static SLOT: $crate::rt::Slot<$game> = $crate::rt::Slot::new();

            #[cfg(not(target_os = "none"))]
            $crate::rt::thread_local! {
                static SLOT: $crate::rt::Slot<$game> = const { $crate::rt::Slot::new() };
            }

            fn with<R>(f: impl FnOnce(&$crate::rt::Slot<$game>) -> R) -> R {
                #[cfg(target_os = "none")]
                {
                    f(&SLOT)
                }
                #[cfg(not(target_os = "none"))]
                {
                    SLOT.with(f)
                }
            }

            // `unsafe` functions: they read the frame they are given, so
            // only a caller with a real frame may call them (the console,
            // through `UpdateFn::call`). Safe code cannot (step 3).
            unsafe extern "C" fn update(frame: *mut $crate::api::GameFrame) -> u32 {
                // SAFETY: the console hands its frame for this call.
                with(|slot| unsafe { $crate::rt::update::<$game>(slot, frame) })
            }

            unsafe extern "C" fn exit(frame: *mut $crate::api::GameFrame, reason: u32) {
                // SAFETY: the console hands its frame for this call.
                with(|slot| unsafe { $crate::rt::exit::<$game>(slot, frame, reason) })
            }

            // SAFETY: `update` and `exit` touch only the frame and what it
            // points at, and keep nothing after they return.
            static CALLBACKS: $crate::api::GameCallbacks =
                unsafe { $crate::rt::callbacks(update, exit) };

            // SAFETY: the caller hands a real table, kept while the game
            // runs; `rt::entry` checks null, alignment, size and version.
            unsafe { $crate::rt::entry(console, &CALLBACKS) }
        }

        #[cfg(target_os = "none")]
        #[panic_handler]
        fn __game_kit_panic(info: &::core::panic::PanicInfo<'_>) -> ! {
            $crate::rt::on_panic(info)
        }
    };
}
