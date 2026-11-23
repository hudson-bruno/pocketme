//! Input: the touch screen, the board's own buttons, and gamepads.
//!
//! Each part reports the state NOW. Finding presses and releases (what
//! changed since the last frame) is the console's job, so every board
//! stays simple and behaves the same.
//!
//! One thing the state now cannot tell: a button pressed and let go
//! between two console frames (a "short press"), or a tap on the screen
//! that starts and ends between two frames. When the console looks, the
//! button is up again, so it never saw the press. So each part has one
//! more method, added later with a default body: `take_pressed` (the
//! keys, the pad) and `take_tap` (the touch screen). The board notes
//! every press as it happens (a "latch": a note that stays until it is
//! read), and the console reads and clears the note once per frame.
//!
//! Releases need no note of their own. A press the board noted, of a
//! button that is up now, was let go again. A press it noted of a button
//! that was already down at the last frame means: let go, then pressed
//! again. The console works both out.
//!
//! A board that only knows "held now" keeps the defaults (nothing
//! noted). It then works exactly as before these methods existed: a
//! press shorter than a frame may be missed.
//!
//! Rule for a board that notes presses: the note and the state now must
//! describe the same moment. Collect both together before the step (as
//! the board loop says), so a press is never counted twice: once in the
//! note and again as a new "held".

/// A point on the panel, in panel pixels (0..320, 0..240 on the CYD).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TouchPoint {
    pub x: u16,
    pub y: u16,
}

/// The touch screen.
pub trait Touch {
    /// Where the finger (or stylus) is now, in panel pixels, or `None`
    /// when nothing touches the screen. The board does the calibration:
    /// the console only ever sees panel pixels.
    fn read(&mut self) -> Option<TouchPoint>;

    /// Where a touch STARTED since the last call of this method, even if
    /// the finger is already gone again; `None` if no touch started.
    /// Reading clears the note. If several touches started, the first
    /// one. In panel pixels, like [`Touch::read`].
    ///
    /// Default: `None` (this board notes nothing; see the module notes).
    fn take_tap(&mut self) -> Option<TouchPoint> {
        None
    }
}

/// Bits for [`Keys::held`]: the board's own buttons.
pub mod key {
    /// The BOOT button (GPIO0 on the CYD). On the PC: a key of the
    /// keyboard.
    pub const BOOT: u16 = 1 << 0;
    // Bits 1..15 are free for future boards (power, volume).
}

/// The board's own buttons (not the gamepad). The CYD has only BOOT
/// (RESET restarts the chip and is never seen by the code).
pub trait Keys {
    /// The buttons held down now, as [`key`] bits.
    fn held(&mut self) -> u16;

    /// The buttons that went DOWN since the last call of this method, as
    /// [`key`] bits, even if they are up again now. Reading clears the
    /// note.
    ///
    /// Default: 0 (this board notes nothing; see the module notes).
    fn take_pressed(&mut self) -> u16 {
        0
    }
}

/// Bits for [`PadState::buttons`]: every button a gamepad may have.
///
/// The numbering is the one of the pad decoder (crates/pad-view, `Btn`),
/// so a board can copy its `buttons` number as it is. The face buttons
/// are named by position (south, east, ...), because "A" is at the
/// bottom on one brand and on the right on another.
///
/// Which of these a GAME sees is not decided yet (owner decision D4).
/// The board layer carries them all; the console maps them.
pub mod btn {
    /// Bottom face button (A on Xbox, B on Nintendo, cross on PlayStation).
    pub const SOUTH: u32 = 1 << 0;
    /// Right face button (B on Xbox, A on Nintendo).
    pub const EAST: u32 = 1 << 1;
    /// Left face button (X on Xbox, Y on Nintendo).
    pub const WEST: u32 = 1 << 2;
    /// Top face button (Y on Xbox, X on Nintendo).
    pub const NORTH: u32 = 1 << 3;
    /// Left shoulder (LB, L, L1).
    pub const L: u32 = 1 << 4;
    /// Right shoulder (RB, R, R1).
    pub const R: u32 = 1 << 5;
    /// Left trigger pressed as a button (LT, ZL, L2).
    pub const ZL: u32 = 1 << 6;
    /// Right trigger pressed as a button (RT, ZR, R2).
    pub const ZR: u32 = 1 << 7;
    /// Select, Back, View, "-".
    pub const MINUS: u32 = 1 << 8;
    /// Start, Menu, Options, "+".
    pub const PLUS: u32 = 1 << 9;
    /// Home, Xbox button, PS button. Kept by the console (its menu).
    pub const HOME: u32 = 1 << 10;
    /// Capture, Share.
    pub const CAPTURE: u32 = 1 << 11;
    /// Left stick pressed in.
    pub const LSTICK: u32 = 1 << 12;
    /// Right stick pressed in.
    pub const RSTICK: u32 = 1 << 13;
    pub const UP: u32 = 1 << 14;
    pub const DOWN: u32 = 1 << 15;
    pub const LEFT: u32 = 1 << 16;
    pub const RIGHT: u32 = 1 << 17;
    /// All the bits above.
    pub const ALL: u32 = (1 << 18) - 1;
}

/// A thumb stick. 0 is the middle. -32767 is fully left (or up), +32767
/// fully right (or down): y grows downwards, like on the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stick {
    pub x: i16,
    pub y: i16,
}

/// The largest trigger value (fully pressed). Released is 0.
pub const TRIGGER_MAX: u16 = 1023;

/// One gamepad, now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PadState {
    /// False when no pad is there. All other fields are then zero.
    pub connected: bool,
    /// Held buttons, as [`btn`] bits.
    pub buttons: u32,
    pub left: Stick,
    pub right: Stick,
    /// Left trigger, 0 to [`TRIGGER_MAX`].
    pub lt: u16,
    /// Right trigger, 0 to [`TRIGGER_MAX`].
    pub rt: u16,
}

impl PadState {
    /// No pad.
    pub const NONE: PadState = PadState {
        connected: false,
        buttons: 0,
        left: Stick { x: 0, y: 0 },
        right: Stick { x: 0, y: 0 },
        lt: 0,
        rt: 0,
    };
}

/// A pad's motion sensor, now: accelerometer and gyroscope, x, y, z.
///
/// In the console's own units and axes, the same a game sees (see
/// `game_api::motion`): 4096 per g, 16 per degree per second; x to the
/// player's right, y away from the player, z up out of the buttons. The
/// board turns its pad's numbers into these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Motion {
    pub accel: [i16; 3],
    pub gyro: [i16; 3],
}

/// Gamepads. On the CYD they come over Bluetooth; on the PC the keyboard
/// fills the first two and scripts all four.
///
/// The console asks for pads 0 to 3 in every step, one per player
/// (`game_api::PLAYERS`). Pad 0 also drives the console's menus.
pub trait Pad {
    /// Pad number `index` (0 is the first). A pad that is not there, or
    /// an `index` the board does not have, gives [`PadState::NONE`].
    fn read(&mut self, index: u8) -> PadState;

    /// The motion sensor of pad `index`, when it sends motion now. Some
    /// pads send it only in one of their modes; then this is `None` in
    /// the others. Default: no motion.
    fn motion(&mut self, index: u8) -> Option<Motion> {
        let _ = index;
        None
    }

    /// The buttons of pad `index` that went DOWN since the last call of
    /// this method for that pad, as [`btn`] bits, even if they are up
    /// again now. Reading clears the note. A pad that is not there gives
    /// 0.
    ///
    /// Default: 0 (this board notes nothing; see the module notes).
    fn take_pressed(&mut self, index: u8) -> u32 {
        let _ = index;
        0
    }
}
