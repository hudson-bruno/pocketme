//! The board layer: everything the console may ask of the hardware.
//!
//! PROVISIONAL. The owner has not decided the shape of this layer yet.
//! It covers only what the first simulator slice needs: a screen, touch,
//! a gamepad, time and a log line. Bluetooth, the SD card, saved
//! settings, sound and the rest come later.
//!
//! The console's code talks only to the [`Board`] trait. On the PC a fake
//! board sits behind it (a window, or a script with no window). On the
//! ESP32 the real board would sit behind it. So the console's code is the
//! same in both places.
//!
//! This crate has no `std` (the PC's standard library) and no
//! dependencies, so the same file can be built for the ESP32.

#![no_std]

/// Width of the screen in pixels.
pub const SCREEN_W: usize = 320;
/// Height of the screen in pixels.
pub const SCREEN_H: usize = 240;

/// Makes an RGB565 colour (the panel's 16-bit colour: 5 bits of red,
/// 6 of green, 5 of blue) from 8-bit red, green and blue.
pub const fn rgb565(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3)
}

/// One tick of the frame clock: which frame this is and the time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tick {
    /// Counts from 0, one more each frame.
    pub frame: u32,
    /// Microseconds since the console started.
    pub now_us: u64,
}

/// The touch screen.
///
/// The position is in screen pixels (0..320, 0..240). When `pressed` is
/// false, the position is where the finger was last.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Touch {
    pub pressed: bool,
    pub x: u16,
    pub y: u16,
}

/// A thumb stick. 0 is the middle; -128 is fully left or up, 127 fully
/// right or down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stick {
    pub x: i8,
    pub y: i8,
}

/// The gamepad's state at the start of the frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pad {
    /// False when no pad is there. The other fields are then all zero.
    pub connected: bool,
    /// One bit per button, see [`button`].
    pub buttons: u16,
    pub left: Stick,
    pub right: Stick,
    /// How far the left trigger is pulled: 0 (not at all) to 255.
    pub left_trigger: u8,
    /// How far the right trigger is pulled: 0 (not at all) to 255.
    pub right_trigger: u8,
}

impl Pad {
    /// True while `bit` (one of [`button`]) is held down.
    pub const fn held(&self, bit: u16) -> bool {
        self.buttons & bit != 0
    }
}

/// The gamepad's buttons, one bit each.
///
/// The first 12 are the same bits as the debug link's button mask
/// (`crates/debug-link`, `msg::button`), so a mask sent over the cable
/// means the same thing here. `HOME` is new. Which list the console keeps
/// for good is not decided yet.
pub mod button {
    pub const UP: u16 = 1 << 0;
    pub const DOWN: u16 = 1 << 1;
    pub const LEFT: u16 = 1 << 2;
    pub const RIGHT: u16 = 1 << 3;
    pub const A: u16 = 1 << 4;
    pub const B: u16 = 1 << 5;
    pub const X: u16 = 1 << 6;
    pub const Y: u16 = 1 << 7;
    pub const L: u16 = 1 << 8;
    pub const R: u16 = 1 << 9;
    pub const START: u16 = 1 << 10;
    pub const SELECT: u16 = 1 << 11;
    pub const HOME: u16 = 1 << 12;

    /// Every button with its name, in this order.
    pub const NAMES: [(&str, u16); 13] = [
        ("up", UP),
        ("down", DOWN),
        ("left", LEFT),
        ("right", RIGHT),
        ("a", A),
        ("b", B),
        ("x", X),
        ("y", Y),
        ("l", L),
        ("r", R),
        ("start", START),
        ("select", SELECT),
        ("home", HOME),
    ];

    /// The bit for a button's name, in any letter case.
    pub fn from_name(name: &str) -> Option<u16> {
        NAMES
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(name))
            .map(|&(_, bit)| bit)
    }
}

/// What the console may ask of the board.
///
/// The console's main loop looks like this, on the PC and on the ESP32:
///
/// ```ignore
/// while let Some(tick) = board.wait_frame() {
///     let pad = board.pad();      // read the input
///     draw(board.screen(), pad);  // draw the new picture
///     board.present();            // show it
/// }
/// ```
pub trait Board {
    /// Waits until it is time for the next frame, then reads the input.
    /// Returns `None` when the console must stop (the window was closed,
    /// or a script came to its end).
    fn wait_frame(&mut self) -> Option<Tick>;

    /// Microseconds since the console started.
    fn now_us(&self) -> u64;

    /// The picture the console draws into: `SCREEN_W * SCREEN_H` RGB565
    /// pixels, row by row from the top left. Nothing shows until
    /// [`Board::present`].
    fn screen(&mut self) -> &mut [u16];

    /// Shows what was drawn into [`Board::screen`].
    fn present(&mut self);

    /// The touch screen, as read by the last [`Board::wait_frame`].
    fn touch(&self) -> Touch;

    /// The gamepad, as read by the last [`Board::wait_frame`].
    fn pad(&self) -> Pad;

    /// Writes one line to the log (no line break needed).
    fn log(&mut self, line: &str);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_land_in_the_right_bits() {
        assert_eq!(rgb565(255, 0, 0), 0xF800);
        assert_eq!(rgb565(0, 255, 0), 0x07E0);
        assert_eq!(rgb565(0, 0, 255), 0x001F);
        assert_eq!(rgb565(255, 255, 255), 0xFFFF);
    }

    #[test]
    fn button_names_find_their_bits() {
        assert_eq!(button::from_name("Start"), Some(button::START));
        assert_eq!(button::from_name("home"), Some(button::HOME));
        assert_eq!(button::from_name("turbo"), None);
        // Every bit is used once.
        let all = button::NAMES.iter().fold(0u16, |acc, &(_, bit)| {
            assert_eq!(acc & bit, 0);
            acc | bit
        });
        assert_eq!(all, (1 << 13) - 1);
    }
}
