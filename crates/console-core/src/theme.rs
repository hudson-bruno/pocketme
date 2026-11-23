//! The look of the console, in ONE place: colours, sizes and timings.
//!
//! Change a number here and every screen follows. The picture is 160x120
//! pixels; each pixel is a number into a palette of 256 colours.
//!
//! Palette numbers used by the console:
//! - 240 to 247: the console's own 8 colours ([`UI_COLOURS`]).
//! - 200 to 231: one colour per game tile ([`TILE_FIRST`]), read from the
//!   card. They are only set while the home screen shows (no game runs).
//! - 104 to 199: the tile pictures' colours, 16 per picture kept
//!   ([`ART_FIRST`], [`ART_SLOTS`]); also only while home shows.
//! - 0 to 239 belong to a game while one runs (it gets them with the
//!   start colours of `game_api::picture`). The home screen sets its tile
//!   colours again when the game ends.
//! - 248 to 255: not used by the console, and kept from games too.
//!
//! A colour change makes the renderer send the whole screen once, so the
//! console sets its colours once at start and never animates them.

// ---- Colours --------------------------------------------------------------

/// Darkest blue: the background.
pub const INK: u8 = 240;
/// Dark blue: the bar and the inside of panels.
pub const PANEL: u8 = 241;
/// Warm white: main text.
pub const PAPER: u8 = 242;
/// Yellow: the selection frame and highlights.
pub const GOLD: u8 = 243;
/// Grey blue: quiet text and icons.
pub const MUTE: u8 = 244;
/// Green: good news ("pad connected").
pub const OK: u8 = 245;
/// Orange: warnings ("cannot start").
pub const WARN: u8 = 246;
/// Blue grey: thin lines, and icons that are off.
pub const LINE: u8 = 247;

/// The console's colours as 0xRRGGBB, for palette numbers 240 to 247 in
/// order. These are the colours of the design page.
pub const UI_COLOURS: [u32; 8] = [
    0x0b0f1a, // INK
    0x1b2438, // PANEL
    0xf4f1e4, // PAPER
    0xf7d154, // GOLD
    0x7b93b8, // MUTE
    0x5fb04f, // OK
    0xe2714a, // WARN
    0x2e3a59, // LINE
];

/// First palette number for game tiles; game `i` uses `TILE_FIRST + i`.
pub const TILE_FIRST: u8 = 200;

// ---- Tile pictures (the games' ART.BIN, see `art`) --------------------------

/// How many tile pictures home keeps in memory at once. Each costs
/// [`crate::art::SLOT_BYTES`] (680) bytes of the lent RAM (see `lent`). 6 is
/// every tile on screen (at most 5) and the next one off it.
pub const ART_SLOTS: usize = 6;
/// Colours one tile picture may use (4 bits per pixel).
pub const ART_COLOURS: usize = 16;
/// First palette number for tile pictures: picture slot `k` uses
/// `ART_FIRST + 16 k` to `ART_FIRST + 16 k + 15`. Only while home shows
/// (a game owns 0 to 239 while it runs). 104 to 199 with 6 slots.
pub const ART_FIRST: u8 = 104;
const _: () = assert!(ART_FIRST as usize + ART_SLOTS * ART_COLOURS <= TILE_FIRST as usize);
/// A tile picture is as big as a tile.
pub const ART_W: u16 = TILE;
pub const ART_H: u16 = TILE;
/// Draw a thin [`LINE`] edge round a tile picture too (the letter tiles
/// always have one). Off: the picture fills its tile to the edge.
pub const ART_EDGE: bool = false;
/// Tile colours for games whose INFO.TXT gives none (0xRRGGBB), used in
/// turn.
pub const TILE_DEFAULTS: [u32; 4] = [0x2f7a4a, 0x2a86a0, 0x8650a8, 0xb3443c];

// ---- Sizes (picture pixels) -----------------------------------------------

/// Height of the bar at the bottom.
pub const BAR_H: u16 = 12;
/// Top of the bar (the picture is 120 tall).
pub const BAR_Y: i16 = 120 - BAR_H as i16;

/// A game tile is a square this wide.
pub const TILE: u16 = 36;
/// Space between two tiles.
pub const TILE_GAP: u16 = 8;
/// Left edge of the first tile when the row is not scrolled.
pub const TILE_LEFT: i16 = 14;
/// Top edge of the tiles.
pub const TILE_Y: i16 = 22;
/// The selected tile always keeps at least this much room to the edge
/// of the picture; the row scrolls to keep it so.
pub const TILE_MARGIN: i16 = 14;
/// Width of the selection frame, and the gap between it and the tile.
pub const FRAME_W: u16 = 2;
pub const FRAME_GAP: u16 = 1;

/// Where the selected game's name (twice the font size) starts.
pub const NAME_Y: i16 = 68;
/// Where the line under the name (its tag) starts.
pub const TAG_Y: i16 = 82;
/// Where the row of small dots (one per game) starts.
pub const DOTS_Y: i16 = 92;

/// Top line of the screen: the console's name and the number of games.
pub const TOP_Y: i16 = 5;
/// Where home writes the console's name on that line (font size).
pub const TITLE_X: i16 = 6;

/// The console's name (working name): home's title and the boot screen
/// write this one word.
pub const NAME: &str = "POCKETME";

/// A notification panel: its height and where it rests.
pub const NOTE_H: u16 = 13;
pub const NOTE_Y: i16 = 3;

/// The console menu over a game (RESUME, BRIGHTNESS, BACK TO HOME): a
/// box in the middle of the picture. The game's picture under it is
/// kept and put back (see `game`), so its size costs memory: 80 x 36 =
/// 2,880 bytes. The menu on the home screen uses the same width, and a
/// height that fits its items (see `pause`).
pub const MENU_W: u16 = 80;
pub const MENU_H: u16 = 36;
pub const MENU_X: i16 = (160 - MENU_W as i16) / 2;
pub const MENU_Y: i16 = (120 - MENU_H as i16) / 2;
/// The space between the box's edge and its first and last item.
pub const MENU_PAD: u16 = 3;
/// The first item of the menu over a game starts here; the items follow
/// one under the other. A text item is this tall...
pub const MENU_ITEM_Y: i16 = MENU_Y + MENU_PAD as i16;
pub const MENU_ITEM_H: u16 = 9;
/// ...and the BRIGHTNESS item this tall (a sun and five bars; a bigger
/// target for a finger).
pub const MENU_BRIGHT_H: u16 = 12;

/// The menu button on the home screen: a small gear at the right end of
/// the bar (clear of the notifications at the top and of the tiles). Its
/// left edge and top, in picture pixels. A tap on it opens the menu.
pub const GEAR_X: i16 = 147;
pub const GEAR_Y: i16 = BAR_Y + 2;
/// The area a tap counts in: bigger than the 9 x 9 drawing, to the
/// picture's right and bottom edges.
pub const GEAR_HIT_X: i16 = 141;
pub const GEAR_HIT_Y: i16 = BAR_Y - 2;

// ---- Brightness ---------------------------------------------------------------

/// The backlight values of the five brightness steps, dimmest first (0 =
/// off, 255 = full; see `board_api::Screen::backlight`). Never 0.
///
/// The light a backlight gives grows with its value, but the eye sees
/// brightness more like a cube root of the light. So the steps are
/// evenly spaced in lightness (CIE L*: 30, 47.5, 65, 82.5, 100), not as
/// numbers: value = 255 x ((L* + 16) / 116)^3. The dimmest step is L*
/// 30 (about 6 % of full light), meant to stay readable indoors. Not
/// checked on the CYD panel yet.
pub const BACKLIGHT: [u8; 5] = [16, 42, 87, 156, 255];
/// The number of steps. The brightest (`BRIGHT_STEPS - 1`) is the
/// default.
pub const BRIGHT_STEPS: u8 = BACKLIGHT.len() as u8;

// ---- Timings (microseconds) -----------------------------------------------

/// The boot screen ends by itself after this long.
pub const BOOT_US: u64 = 1_500_000;
/// Boot: the little console icon is drawn row by row over this time.
pub const BOOT_ICON_US: u64 = 360_000;
/// Boot: the first letter of the name appears at this time...
pub const BOOT_LETTERS_AT: u64 = 400_000;
/// ...and each next letter this much later...
pub const BOOT_LETTER_GAP: u64 = 70_000;
/// ...each dropping into place over this time.
pub const BOOT_DROP_US: u64 = 100_000;
/// Boot: the icon's screen lights up, and the line under the name grows
/// over [`BOOT_LINE_US`].
pub const BOOT_LIGHT_AT: u64 = 1_000_000;
pub const BOOT_LINE_US: u64 = 250_000;

/// The GATHER boot animation (feature `boot-gather`) asks for a frame
/// this often while it moves: 60 a second. Every other page uses
/// `crate::FRAME_US` (30 a second). 33,333 here would play it at 30 a
/// second: the animation is a pure function of the time, so it plays
/// the same cut either way, with every other picture left out (all
/// tests pass at either rate, except the stored boot fingerprints in
/// `tests/data/golden_gather.txt`, which are then written again).
#[cfg(feature = "boot-gather")]
pub const BOOT_FRAME_US: u64 = 16_667;

/// A notification slides in (and out) over this time...
pub const NOTE_SLIDE_US: u64 = 200_000;
/// ...and stays this long in between.
pub const NOTE_SHOW_US: u64 = 2_500_000;

/// A held direction acts once at once, then again after this delay...
pub const REPEAT_DELAY_US: u64 = 400_000;
/// ...and then once every this long.
pub const REPEAT_US: u64 = 120_000;

/// With no card or no games, the card is looked at again this often (the
/// CYD's card slot cannot tell when a card goes in). With games shown, a
/// quick look checks the card is still there.
pub const CARD_CHECK_US: u64 = 2_000_000;

// ---- Input ----------------------------------------------------------------

/// A stick counts as pushed past this (of 32,767): about 40 %.
pub const STICK_ON: i16 = 13_000;
/// ...and as back in the middle below this (a little lower, so a stick
/// held near the edge does not flicker on and off).
pub const STICK_OFF: i16 = 9_000;

// ---- Touch (picture pixels; the panel has 2 per picture pixel) -------------

/// A touch becomes a drag once the finger is this far from where it went
/// down, on either axis (4 picture pixels = 8 panel pixels). Below that,
/// lifting the finger is a tap. To be tuned on the board.
pub const DRAG_START: u16 = 4;
/// A finger down (a press, a drag, or a touch the console took away)
/// may be missing from this many reads in a row and the touch goes on
/// (the board's touch layer is noisy). One more missing read ends it:
/// so a lift is believed this many frames late, and a tap comes then.
/// 0: no grace.
pub const TOUCH_GRACE: u8 = 1;
/// A press held longer than this (from going down to the last read
/// that saw it) is not a tap when it lifts: it does nothing. To be
/// tuned on the board.
pub const TAP_MAX_US: u64 = 500_000;
/// A finger that has not moved for this long rests. The console then
/// stops drawing every frame for it. Let go after resting, a drag has
/// no speed (no flick).
pub const TOUCH_STILL_US: u64 = 100_000;
/// Two touch reads closer in time than this give no speed (too noisy).
pub const SPEED_MIN_DT_US: u64 = 10_000;
/// Highest finger speed counted, picture pixels a second.
pub const SPEED_MAX: i32 = 4_000;
/// A drag let go at this speed or faster (picture pixels a second) is a
/// flick: the row goes on in that direction to the next resting place,
/// and further for a faster flick.
pub const FLICK_MIN: i32 = 150;
/// How far a flick carries the row: its speed times this time
/// (100 ms: a flick at 600 pixels a second carries 60 pixels).
pub const FLICK_US: i32 = 100_000;
/// A flick carries the row at most this far (4 tiles).
pub const FLICK_MAX: i32 = 4 * (TILE + TILE_GAP) as i32;

/// Turns 0xRRGGBB into the panel's RGB565.
pub const fn rgb(hex: u32) -> u16 {
    console_video::rgb565((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}
