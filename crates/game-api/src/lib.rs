//! The contract between the console and a game. Version 3.
//!
//! A game is a separate program. On the chip it is loaded from the SD
//! card into two fixed places in memory (see [`file`]). In the simulator
//! the same source is linked into the program. Either way, the two sides
//! talk ONLY through the tables in this crate:
//!
//! * [`ConsoleApi`]: given to the game once, when it starts. It holds the
//!   one function a game may call: [`ConsoleApi::abort`] (used by the
//!   game's panic handler on the chip).
//! * [`GameCallbacks`]: the game's two functions, `update` and `exit`.
//!   The game returns this table from its entry function ([`GameEntry`]).
//! * [`GameFrame`]: handed to `update` on every call. It points at the
//!   picture, the palette, the input and a log buffer, and carries the
//!   time, the frame number and what happened (started, resumed). The
//!   game writes its wishes back into it (palette changed, a video mode,
//!   when to wake it).
//! * [`Input`]: once per call, the touch screen plus one [`PlayerInput`]
//!   (buttons, sticks, triggers, motion) for each of the [`PLAYERS`]
//!   players.
//!
//! The README explains every choice. The short rules:
//!
//! * Only `repr(C)` structs, fixed-width integers, raw pointers and
//!   `extern "C"` functions cross the border. No `usize`, no `bool`, no
//!   Rust types such as `&str`, slices or `Option`: they have no fixed
//!   layout, and the two sides may be built by different compilers.
//! * Every table starts with `size` (its size in bytes, as the side that
//!   owns it was built) and `version`. New fields only ever go at the
//!   end. A reader treats a field past `size` as missing.
//! * Zero means "nothing" or "the default" in every field a game writes,
//!   so a game that does not know a field leaves it at its default.
//! * A pointer is 4 bytes on the chip and 8 on the PC. Each side is built
//!   for one world, so both agree. Sizes differ between the worlds, never
//!   between the two sides of one world. Pointers in [`GameFrame`] are
//!   valid only during the call they came with.
//! * Unknown values (a mode, a rate, a status) get a clean "no", never a
//!   crash.

#![no_std]

mod call;
pub mod file;
mod open;

pub use call::{ExitFn, LiveFrame, PICTURE_BYTES, UpdateFn};
pub use file::{
    Crc32, FlashPartHeader, GameHeader, HeaderError, MAGIC, MAGIC_FLASH, layout, read_game_header,
};
pub use open::{LinkedGame, OpenError, UNWIND_CONSOLE_API, abort_by_panic, no_exit, open_game};

/// The version of the tables. Version 1 was the loader experiment's test
/// contract; no game uses it. Version 2 was the first one games were
/// written against, with one player. Version 3 reshaped [`Input`] to carry
/// four players ([`PlayerInput`]); every game was rebuilt for it, and the
/// rule "only grow at the end" holds again from here on.
pub const API_VERSION: u32 = 3;

/// The oldest game version the console still runs. Version 2 games read
/// the old one-player [`Input`] and are refused.
pub const MIN_GAME_VERSION: u32 = 3;

/// How many players one [`Input`] carries. Player 0 is "player 1" on
/// screen: the one whose pad also drives the console's menus.
pub const PLAYERS: usize = 4;

/// Where games are on the card (the same on the PC's card folder).
pub mod card {
    /// One folder per game in here. The folder name (8 letters at most)
    /// is the game's name for the console.
    pub const GAMES_DIR: &str = "/CONSOLE/GAMES";
    /// The game file inside a game's folder.
    pub const GAME_FILE: &str = "GAME.BIN";
    /// The optional description inside a game's folder (KEY=VALUE lines).
    pub const INFO_FILE: &str = "INFO.TXT";
}

/// The picture of the default video mode.
pub mod picture {
    /// Width in pixels.
    pub const W: u16 = 160;
    /// Height in pixels.
    pub const H: u16 = 120;
    /// Palette numbers 0 to 239 belong to the game. It may set them and
    /// draw with them.
    pub const GAME_COLOURS: u16 = 240;
    /// Palette numbers 240 to 255 belong to the console (its
    /// notifications and pause menu). A game may draw with them but
    /// cannot change them.
    pub const CONSOLE_FIRST: u16 = 240;

    /// The colour each game palette number has when a game starts, as
    /// RGB565: a 6 x 8 x 5 colour cube (red, green, blue levels).
    /// Number `(r * 8 + g) * 5 + b` has red level `r` (0..6), green `g`
    /// (0..8), blue `b` (0..5). So 0 is black and 239 is white.
    pub const fn start_colour(i: u8) -> u16 {
        let i = if i as u16 >= GAME_COLOURS {
            0
        } else {
            i as u16
        };
        let (r, g, b) = (i / 40, (i / 5) % 8, i % 5);
        let (r, g, b) = (r * 255 / 5, g * 255 / 7, b * 255 / 4);
        ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)
    }
}

/// The 12 buttons a game sees, as bits of [`PlayerInput::held`] (and
/// `pressed`, `released`). The same bits as the debug link's button
/// mask (crates/debug-link, `button`).
///
/// A, B, X, Y are named by their job, not by where they sit: A is the
/// button the console's own menus use to confirm, B the one to go back.
/// Which physical button that is, is the console's choice (one table in
/// console-core), so games stay right whatever pad is used. Home never
/// reaches a game: the console keeps it for its pause menu.
pub mod button {
    pub const UP: u32 = 1 << 0;
    pub const DOWN: u32 = 1 << 1;
    pub const LEFT: u32 = 1 << 2;
    pub const RIGHT: u32 = 1 << 3;
    /// The main action, and "yes".
    pub const A: u32 = 1 << 4;
    /// The second action, and "back".
    pub const B: u32 = 1 << 5;
    pub const X: u32 = 1 << 6;
    pub const Y: u32 = 1 << 7;
    /// Left shoulder.
    pub const L: u32 = 1 << 8;
    /// Right shoulder.
    pub const R: u32 = 1 << 9;
    pub const START: u32 = 1 << 10;
    pub const SELECT: u32 = 1 << 11;
    /// All 12. Other bits are always 0.
    pub const ALL: u32 = (1 << 12) - 1;
}

/// Bits of [`PlayerInput::flags`] (one player's pad). They never share a
/// bit with [`input_flag`], so mixing the two up reads "no", not a wrong
/// "yes".
pub mod player_flag {
    /// This player's gamepad is connected.
    pub const PAD: u32 = 1 << 0;
    /// [`PlayerInput::accel`](crate::PlayerInput::accel) and `gyro` hold
    /// real numbers now. Some pads send motion only in one of their
    /// modes, so a game must check this every frame.
    pub const MOTION: u32 = 1 << 1;
}

/// Bits of [`Input::flags`] (the touch screen, shared by everyone).
pub mod input_flag {
    /// A finger (or stylus) is on the screen now.
    pub const TOUCH: u32 = 1 << 2;
    /// The touch started since the last call.
    pub const TOUCH_START: u32 = 1 << 3;
    /// The touch ended since the last call (the position is where it
    /// was last).
    pub const TOUCH_END: u32 = 1 << 4;
}

/// Units of the motion numbers in [`PlayerInput`]. They do not depend on the
/// pad: the board turns each pad's own numbers into these.
///
/// The axes, for a pad lying flat on a table, buttons up, the player
/// sitting behind it:
///
/// * x points to the player's right;
/// * y points away from the player (towards the shoulder buttons);
/// * z points up, out of the buttons.
///
/// The accelerometer feels gravity as a push upwards: a pad lying still
/// and flat reads about (0, 0, +4096). The gyroscope gives the turning
/// speed around each axis; positive is anticlockwise when you look from
/// the tip of that axis towards the pad (the "right-hand rule": curl the
/// fingers of the right hand with the thumb along the axis).
pub mod motion {
    /// Accelerometer: this many units per g (the pull of the Earth). An
    /// `i16` then reaches about 8 g.
    pub const ACCEL_PER_G: i16 = 4096;
    /// Gyroscope: this many units per degree per second. An `i16` then
    /// reaches about 2,047 degrees per second.
    pub const GYRO_PER_DPS: i16 = 16;
}

/// Largest stick value. A stick reads -32767 fully left (or up) to +32767
/// fully right (or down): y grows downwards, as on the screen. 0 is the
/// middle.
pub const STICK_MAX: i16 = 32767;
/// Largest trigger value (fully pressed); released is 0. A trigger that
/// is only a button reads 0 or this.
pub const TRIGGER_MAX: u16 = 32767;

/// What `update` returns.
pub mod status {
    /// Keep going.
    pub const RUN: u32 = 0;
    /// The game is over: go back to the home screen. The console shows a
    /// short notice. `exit` is not called after this.
    pub const DONE: u32 = 1;
    /// The game hit an error it cannot recover from (on the PC: a panic,
    /// caught by the game kit). The console goes home with a warning.
    /// Any value the console does not know counts as this too.
    pub const ABORTED: u32 = 2;
}

/// Bits of [`GameFrame::events`]: what happened since the last call.
pub mod event {
    /// The first call of this run. The picture is cleared to palette
    /// number 0 and the game palette holds the start colours
    /// ([`crate::picture::start_colour`]).
    pub const START: u32 = 1 << 0;
    /// The first call after the pause menu closed. Game time did not
    /// move while paused. The picture is as the game left it.
    pub const RESUME: u32 = 1 << 1;
    /// The console could not keep the picture: draw all of it again. Set
    /// together with START. Version 2 consoles never set it otherwise; a
    /// game that redraws everything anyway can ignore it.
    pub const REDRAW: u32 = 1 << 2;
}

/// Bits of [`GameFrame::requests`]: what the game asks for in this call.
/// The console clears them before every call.
pub mod request {
    /// "I changed palette entries." Without it, palette writes may show
    /// late or only in part.
    pub const PALETTE: u32 = 1 << 0;
    /// "Please use the video settings in `ask_mode`, `ask_rate`,
    /// `ask_hint`, `ask_colour`." Taken after this call returns; the
    /// answer is in `answer` at the next call.
    pub const VIDEO: u32 = 1 << 1;
}

/// Video modes ([`GameFrame::mode`]). Bit `n` of `offered_modes` is set
/// when mode `n` is offered.
pub mod mode {
    /// 160 x 120 pixels, one byte per pixel, 256 colours, shown 2x. The
    /// default, and the only one in version 2.
    pub const PIXELS_160X120: u16 = 0;
}

/// Frame rates ([`GameFrame::rate`]).
pub mod rate {
    /// 30 calls a second. The default.
    pub const FPS30: u16 = 0;
    /// 60 calls a second.
    pub const FPS60: u16 = 1;
    /// "Only when I say so": the game is called when input changes (a
    /// button, a stick, a trigger, the touch; NOT motion alone), when the
    /// time it put in `wake_at_us` comes, and after a start or resume.
    /// In between the console sends nothing and the chip can sleep.
    /// Buttons, the pad coming or going, and a finger going down or up
    /// call the game at once; sticks, triggers and a moving finger at
    /// most 60 times a second.
    pub const ON_DEMAND: u16 = 2;
}

/// What the picture usually does ([`GameFrame::hint`]). Only a hint: the
/// console may use it to send the picture in a cheaper way.
pub mod hint {
    /// Do not know. The default.
    pub const UNKNOWN: u16 = 0;
    /// Mostly still: small things move.
    pub const MOSTLY_STILL: u16 = 1;
    /// The whole screen scrolls often.
    pub const SCROLLS: u16 = 2;
}

/// Colour quality ([`GameFrame::colour`]).
pub mod colour {
    /// Every colour exactly as in the palette. The default.
    pub const FULL: u16 = 0;
    /// The console may send fewer colour bits (for example 12 instead of
    /// 16) when that makes the screen faster.
    pub const FAST_ALLOWED: u16 = 1;
}

/// The console's answer to the last video request ([`GameFrame::answer`]).
pub mod answer {
    /// No request yet.
    pub const NONE: u16 = 0;
    /// Done: the new settings are in use.
    pub const OK: u16 = 1;
    /// Not offered: this mode. Nothing was changed.
    pub const NO_MODE: u16 = 2;
    /// Not offered: this frame rate. Nothing was changed.
    pub const NO_RATE: u16 = 3;
    /// Not offered: this hint. Nothing was changed.
    pub const NO_HINT: u16 = 4;
    /// Not offered: this colour setting. Nothing was changed.
    pub const NO_COLOUR: u16 = 5;
}

/// Why `exit` is called.
pub mod exit_reason {
    /// The player chose "Back to home" in the pause menu.
    pub const PLAYER: u32 = 1;
    /// The console has to stop the game (for example to turn off).
    pub const CONSOLE: u32 = 2;
}

/// Why a game calls [`ConsoleApi::abort`].
pub mod abort_code {
    /// A Rust panic inside the game (the detail is the source line, or 0).
    pub const PANIC: u32 = 1;
    /// The console's tables are too old or too small for this game.
    pub const BAD_API: u32 = 2;
}

/// What the console gives the game once, when it starts. The game may
/// keep the pointer for as long as it runs.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ConsoleApi {
    /// `size_of::<ConsoleApi>()` as the console was built.
    pub size: u32,
    /// [`API_VERSION`] as the console was built.
    pub version: u32,
    /// Ends the game for good. Never returns. `code` is one of
    /// [`abort_code`]. On the chip the game's panic handler calls it, and
    /// the board decides what happens next (today on the CYD: it prints
    /// what happened on the serial port and stops until a reset). On the PC a panic is caught before it gets
    /// here, and this function panics (it may unwind, hence "C-unwind").
    pub abort: extern "C-unwind" fn(code: u32, detail: u32) -> !,
}

/// The game's functions. The game owns this table (a `static`) and
/// returns a pointer to it from its entry function.
///
/// The console calls these from its own loop, on its own stack. A game
/// never loops for ever: it does one frame's work in `update` and
/// returns. That keeps the radio, the timing and the pause menu alive.
///
/// Safe code cannot call a game's function with a made-up frame (the
/// game would read and write wherever the frame points):
///
/// ```compile_fail,E0618
/// use game_api::{GameCallbacks, GameFrame};
/// fn poke(cb: &GameCallbacks) -> u32 {
///     // A frame whose picture is at address 8: must not compile.
///     let mut f = GameFrame::new();
///     f.pixels = 8 as *mut u8;
///     (cb.update)(&mut f)
/// }
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GameCallbacks {
    /// `size_of::<GameCallbacks>()` as the game was built.
    pub size: u32,
    /// The [`API_VERSION`] the game was built against.
    pub version: u32,
    /// One frame of the game. Returns a [`status`].
    ///
    /// In memory: `unsafe extern "C" fn(frame: *mut GameFrame) -> u32`.
    /// The wrapper ([`UpdateFn`]) is only there so the console can call
    /// it with a [`LiveFrame`] and nothing else.
    pub update: UpdateFn,
    /// The console ends the game (`reason` is an [`exit_reason`]). Called
    /// at most once, never after `update` returned DONE or ABORTED. The
    /// game may still write to the log; drawing is useless.
    ///
    /// In memory: `unsafe extern "C" fn(frame: *mut GameFrame, reason: u32)`
    /// (see [`ExitFn`]).
    pub exit: ExitFn,
}

/// The game's entry function. On the chip, the file header says where it
/// is; in the simulator it is linked in by name. It receives the
/// console's table and returns the game's, or null when the game cannot
/// work with this console. It must not do a frame's work.
///
/// It is an `unsafe` function: it reads the table it is given, and its
/// caller trusts the pointer it returns. Only a board calls it, through
/// [`open_game`] or [`LinkedGame::open`]. A plain `extern "C" fn` fits
/// here too (the calling convention is the same).
pub type GameEntry = unsafe extern "C" fn(console: *const ConsoleApi) -> *const GameCallbacks;

/// Everything the game gets for one call of `update` (and of `exit`).
///
/// The console owns it; it lives on the console's stack during the call.
/// The game reads it and writes only the fields marked "game writes".
/// The pointers in it are valid only during that call: never keep them.
///
/// Field order: header, pointers, then 64-bit, 32-bit and 16-bit numbers,
/// so there are no gaps in either world (checked below).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GameFrame {
    /// `size_of::<GameFrame>()` as the console was built.
    pub size: u32,
    /// [`API_VERSION`] as the console was built.
    pub version: u32,

    /// The picture: `height` rows of `stride` bytes, each byte a palette
    /// number. The game draws here directly. Only `width` bytes of each
    /// row are shown. Aligned to 4 bytes.
    pub pixels: *mut u8,
    /// The palette: 256 RGB565 colours. The game may write entries
    /// `0..game_colours`, and must then set [`request::PALETTE`].
    pub palette: *mut u16,
    /// This call's input. Read it; never write it.
    pub input: *const Input,
    /// A buffer for log text: `log_cap` bytes. The game writes text from
    /// byte 0 and puts the count in `log_len`. Lines end with `\n`. The
    /// console prints them after the call (ASCII only; anything else
    /// shows as `?`).
    pub log: *mut u8,

    /// Game time: microseconds the game has been running, pauses not
    /// counted. 0 in the first call.
    pub time_us: u64,
    /// Game writes, for [`rate::ON_DEMAND`]: call again when game time
    /// reaches this, even with no input. 0: no timer (only input). The
    /// console sets 0 before every call, so set it again each time.
    pub wake_at_us: u64,

    /// Calls of `update` before this one in this run (0 in the first).
    pub frame: u32,
    /// Game time since the last call, in microseconds (0 in the first;
    /// at most `u32::MAX`, about 71 minutes).
    pub delta_us: u32,
    /// What happened since the last call: [`event`] bits.
    pub events: u32,
    /// Game writes: what it asks for in this call, [`request`] bits. The
    /// console sets 0 before every call.
    pub requests: u32,
    /// Video modes this console offers: bit `n` set means [`mode`] `n` is
    /// offered.
    pub offered_modes: u32,
    /// Frame rates offered: bit `n` means [`rate`] `n`.
    pub offered_rates: u32,
    /// Hints understood: bit `n` means [`hint`] `n`.
    pub offered_hints: u32,
    /// Colour settings offered: bit `n` means [`colour`] `n`.
    pub offered_colours: u32,

    /// Width of the picture in pixels (160 in the default mode).
    pub width: u16,
    /// Height of the picture in pixels (120).
    pub height: u16,
    /// Bytes from one row to the next (160).
    pub stride: u16,
    /// How many palette entries the game may set, from 0 (240).
    pub game_colours: u16,
    /// Size of the `log` buffer in bytes.
    pub log_cap: u16,
    /// Game writes: bytes of text put in `log` in this call. The console
    /// sets 0 before every call.
    pub log_len: u16,
    /// The video mode in use ([`mode`]).
    pub mode: u16,
    /// The frame rate in use ([`rate`]).
    pub rate: u16,
    /// The hint in use ([`hint`]).
    pub hint: u16,
    /// The colour setting in use ([`colour`]).
    pub colour: u16,
    /// Game writes, with [`request::VIDEO`]: the mode it wants.
    pub ask_mode: u16,
    /// Game writes, with [`request::VIDEO`]: the frame rate it wants.
    pub ask_rate: u16,
    /// Game writes, with [`request::VIDEO`]: its hint.
    pub ask_hint: u16,
    /// Game writes, with [`request::VIDEO`]: the colour setting it wants.
    pub ask_colour: u16,
    /// The answer to the last video request ([`answer`]).
    pub answer: u16,
    /// Always 0 (keeps the size a multiple of 8). May get a meaning in a
    /// later version.
    pub spare: u16,
}

impl GameFrame {
    /// A table with this console's size and version, null pointers and
    /// zeros elsewhere. The console fills in the rest.
    pub const fn new() -> GameFrame {
        GameFrame {
            size: size_of::<GameFrame>() as u32,
            version: API_VERSION,
            pixels: core::ptr::null_mut(),
            palette: core::ptr::null_mut(),
            input: core::ptr::null(),
            log: core::ptr::null_mut(),
            time_us: 0,
            wake_at_us: 0,
            frame: 0,
            delta_us: 0,
            events: 0,
            requests: 0,
            offered_modes: 0,
            offered_rates: 0,
            offered_hints: 0,
            offered_colours: 0,
            width: 0,
            height: 0,
            stride: 0,
            game_colours: 0,
            log_cap: 0,
            log_len: 0,
            mode: 0,
            rate: 0,
            hint: 0,
            colour: 0,
            ask_mode: 0,
            ask_rate: 0,
            ask_hint: 0,
            ask_colour: 0,
            answer: 0,
            spare: 0,
        }
    }
}

impl Default for GameFrame {
    fn default() -> Self {
        Self::new()
    }
}

/// One player's pad in one call. 40 bytes in both worlds (no pointers).
///
/// A player with no pad reads all zeros (no [`player_flag::PAD`]).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlayerInput {
    /// [`button`] bits held down now.
    pub held: u32,
    /// [`button`] bits that went down since the last call.
    pub pressed: u32,
    /// [`button`] bits that came up since the last call (also when the
    /// pad went away while they were held).
    pub released: u32,
    /// [`player_flag`] bits: pad connected, motion valid.
    pub flags: u32,
    /// Left stick, -[`STICK_MAX`] to +[`STICK_MAX`]; y grows downwards.
    pub left_x: i16,
    pub left_y: i16,
    /// Right stick, the same way.
    pub right_x: i16,
    pub right_y: i16,
    /// Left trigger, 0 to [`TRIGGER_MAX`].
    pub left_trigger: u16,
    /// Right trigger, 0 to [`TRIGGER_MAX`].
    pub right_trigger: u16,
    /// Accelerometer x, y, z in [`motion`] units. Valid with
    /// [`player_flag::MOTION`]; 0 otherwise.
    pub accel: [i16; 3],
    /// Gyroscope x, y, z in [`motion`] units. Valid with
    /// [`player_flag::MOTION`]; 0 otherwise.
    pub gyro: [i16; 3],
}

impl PlayerInput {
    /// No pad: nothing held, no sticks, no motion.
    pub const NONE: PlayerInput = PlayerInput {
        held: 0,
        pressed: 0,
        released: 0,
        flags: 0,
        left_x: 0,
        left_y: 0,
        right_x: 0,
        right_y: 0,
        left_trigger: 0,
        right_trigger: 0,
        accel: [0; 3],
        gyro: [0; 3],
    };
}

/// The input of one call: the touch screen, then [`PLAYERS`] players.
/// 176 bytes in both worlds (no pointers).
///
/// Player `n` is `players[n]`. Player 0 is the console's own pad (on the
/// CYD the Bluetooth pad); the others are empty until a board has more
/// pads.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    /// `size_of::<Input>()` as the console was built.
    pub size: u32,
    /// [`API_VERSION`] as the console was built.
    pub version: u32,
    /// [`input_flag`] bits: the touch screen.
    pub flags: u32,
    /// Where the screen is touched, in the game's own pixels (0..160,
    /// 0..120 in the default mode). Valid with [`input_flag::TOUCH`] or
    /// [`input_flag::TOUCH_END`].
    pub touch_x: i16,
    pub touch_y: i16,
    /// One entry per player.
    pub players: [PlayerInput; PLAYERS],
}

impl Input {
    /// Nothing held, nothing touched, no pads; this console's size and
    /// version.
    pub const fn new() -> Input {
        Input {
            size: size_of::<Input>() as u32,
            version: API_VERSION,
            flags: 0,
            touch_x: 0,
            touch_y: 0,
            players: [PlayerInput::NONE; PLAYERS],
        }
    }
}

/// Where a field ends, in bytes from the start of its table. A reader
/// has the field when the table's `size` is at least this. Works in a
/// `const` too.
///
/// ```
/// use game_api::{Input, field_end};
/// assert_eq!(field_end!(Input, touch_y), 16);
/// assert_eq!(field_end!(Input, players), 176);
/// ```
#[macro_export]
macro_rules! field_end {
    ($t:ty, $f:ident) => {{
        const fn size_of_field<F>(_: fn(&$t) -> &F) -> usize {
            ::core::mem::size_of::<F>()
        }
        ::core::mem::offset_of!($t, $f) + size_of_field(|t: &$t| &t.$f)
    }};
}

// ---- Layout checks, in both worlds --------------------------------------
//
// These run while compiling, for the chip (4-byte pointers) and for the PC
// (8-byte pointers). A field moved by mistake stops the build.

const PTR: usize = size_of::<*const u8>();

const _: () = {
    use core::mem::offset_of;
    assert!(PTR == 4 || PTR == 8);

    assert!(offset_of!(ConsoleApi, abort) == 8);
    assert!(size_of::<ConsoleApi>() == 8 + PTR);

    assert!(offset_of!(GameCallbacks, update) == 8);
    assert!(offset_of!(GameCallbacks, exit) == 8 + PTR);
    assert!(size_of::<GameCallbacks>() == 8 + 2 * PTR);

    assert!(offset_of!(GameFrame, pixels) == 8);
    assert!(offset_of!(GameFrame, log) == 8 + 3 * PTR);
    let p = 8 + 4 * PTR; // first byte after the pointers: 24 or 40
    assert!(offset_of!(GameFrame, time_us) == p);
    assert!(offset_of!(GameFrame, wake_at_us) == p + 8);
    assert!(offset_of!(GameFrame, frame) == p + 16);
    assert!(offset_of!(GameFrame, offered_colours) == p + 44);
    assert!(offset_of!(GameFrame, width) == p + 48);
    assert!(offset_of!(GameFrame, spare) == p + 78);
    assert!(size_of::<GameFrame>() == p + 80); // 104 or 120

    assert!(offset_of!(PlayerInput, flags) == 12);
    assert!(offset_of!(PlayerInput, left_x) == 16);
    assert!(offset_of!(PlayerInput, left_trigger) == 24);
    assert!(offset_of!(PlayerInput, accel) == 28);
    assert!(offset_of!(PlayerInput, gyro) == 34);
    assert!(size_of::<PlayerInput>() == 40);
    assert!(align_of::<PlayerInput>() == 4);

    assert!(offset_of!(Input, flags) == 8);
    assert!(offset_of!(Input, touch_x) == 12);
    assert!(offset_of!(Input, touch_y) == 14);
    assert!(offset_of!(Input, players) == 16);
    assert!(PLAYERS == 4);
    assert!(size_of::<Input>() == 16 + 40 * PLAYERS); // 176

    // The two flag sets never share a bit.
    assert!(
        (player_flag::PAD | player_flag::MOTION)
            & (input_flag::TOUCH | input_flag::TOUCH_START | input_flag::TOUCH_END)
            == 0
    );
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_colours_run_from_black_to_white() {
        assert_eq!(picture::start_colour(0), 0x0000);
        assert_eq!(picture::start_colour(239), 0xFFFF);
        // Pure red, green and blue at full level.
        assert_eq!(picture::start_colour(5 * 40), 0xF800);
        assert_eq!(picture::start_colour(7 * 5), 0x07E0);
        assert_eq!(picture::start_colour(4), 0x001F);
        // Outside the game's part: black.
        assert_eq!(picture::start_colour(240), 0);
    }

    #[test]
    fn new_tables_carry_their_size_and_version() {
        let f = GameFrame::new();
        assert_eq!(f.size as usize, size_of::<GameFrame>());
        assert_eq!(f.version, API_VERSION);
        let i = Input::new();
        assert_eq!(i.size, 176);
        assert_eq!(i.version, 3);
        assert_eq!(i.players, [PlayerInput::NONE; 4]);
        assert_eq!(PlayerInput::NONE, PlayerInput::default());
        assert_eq!(field_end!(GameFrame, spare), size_of::<GameFrame>());
        assert_eq!(field_end!(Input, touch_y), 16);
        assert_eq!(field_end!(PlayerInput, gyro), 40);
        // Player n starts at 16 + 40 n.
        let base = &i as *const Input as usize;
        for (n, p) in i.players.iter().enumerate() {
            assert_eq!(p as *const PlayerInput as usize - base, 16 + 40 * n);
        }
    }
}
