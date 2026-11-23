//! The boot page with the GATHER animation (feature `boot-gather`; the
//! drawing is in [`crate::gather`]). Without the feature, `boot.rs` is
//! the boot page, unchanged.
//!
//! The animation is a pure function of the time since the boot page's
//! first step: each step draws the picture for the step's clock, so a
//! late or dropped step just shows a later picture. While it moves it
//! asks for a frame every [`BOOT_FRAME_US`] (60 a second).
//!
//! How it plays is a setting ([`Anim`], kept by the board like the
//! brightness): FULL (the whole cut, the default), SHORT (it joins the
//! cut late: the solid name, its gleam, the shrink into home; under
//! 1 s), OFF (straight to home). Any button or a tap ends it at once.

use console_video::Frame;

use crate::gather;
use crate::input::Input;
pub use crate::theme::{BOOT_FRAME_US, NAME};

/// How the boot animation plays. The numbers are what the board stores
/// (crate settings-store, `BOOT_ANIM_FULL`, `_SHORT`, `_OFF`: kept in
/// step by hand). FULL is 0, so a new console (all zero bytes) plays
/// the whole cut.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Anim {
    /// The whole cut (about 2.7 s).
    #[default]
    Full = 0,
    /// The end of the cut only: the solid name with its gleam, then the
    /// shrink into home (under 1 s).
    Short = 1,
    /// No animation: straight to home.
    Off = 2,
}

impl Anim {
    /// Every value, in the order the menu goes through them.
    pub const ALL: [Anim; 3] = [Anim::Full, Anim::Short, Anim::Off];

    /// The value stored as `b`; an unknown one is FULL.
    pub const fn from_byte(b: u8) -> Anim {
        match b {
            1 => Anim::Short,
            2 => Anim::Off,
            _ => Anim::Full,
        }
    }

    /// The value as stored.
    pub const fn byte(self) -> u8 {
        self as u8
    }

    /// The next value, and from the last back to the first.
    pub const fn next(self) -> Anim {
        Anim::ALL[(self as usize + 1) % Anim::ALL.len()]
    }

    /// The value before, stopping at the first.
    pub const fn prev(self) -> Anim {
        Anim::ALL[(self as usize).saturating_sub(1)]
    }

    /// The value after, stopping at the last.
    pub const fn later(self) -> Anim {
        let i = self as usize + 1;
        if i < Anim::ALL.len() {
            Anim::ALL[i]
        } else {
            self
        }
    }

    /// Where it joins the animation's time (ms).
    const fn join_ms(self) -> u32 {
        match self {
            Anim::Short => gather::SHORT_AT_MS,
            _ => 0,
        }
    }

    /// Its word in the menu and the log.
    pub const fn word(self) -> &'static str {
        match self {
            Anim::Full => "FULL",
            Anim::Short => "SHORT",
            Anim::Off => "OFF",
        }
    }
}

/// The boot page.
#[derive(Default)]
pub struct Boot {
    start_us: Option<u64>,
    anim: Anim,
}

impl Boot {
    pub const fn new() -> Self {
        Boot {
            start_us: None,
            anim: Anim::Full,
        }
    }

    /// How the boot plays (see [`Anim`]).
    pub fn anim(&self) -> Anim {
        self.anim
    }

    /// Sets how the boot plays. Only the boot that has not started yet
    /// follows it (a board sets it before the first step).
    pub fn set_anim(&mut self, anim: Anim) {
        self.anim = anim;
    }

    fn elapsed(&self, now_us: u64) -> u64 {
        now_us.saturating_sub(self.start_us.unwrap_or(now_us))
    }

    /// The animation's time at `now_us`, in ms (SHORT starts late).
    pub fn anim_ms(&self, now_us: u64) -> u32 {
        let ms = (self.elapsed(now_us) / 1000).min(u64::from(gather::END_MS));
        self.anim.join_ms() + ms as u32
    }

    /// One frame. Returns true when the boot page is finished: the
    /// animation is over, it is OFF, or a button or a tap skipped it.
    pub fn update(&mut self, now_us: u64, input: &Input) -> bool {
        self.start_us.get_or_insert(now_us);
        input.any() || self.anim == Anim::Off || self.anim_ms(now_us) >= gather::END_MS
    }

    /// True while something still moves; after that the last picture
    /// (home's background, stars, bar and title) stays until
    /// [`Boot::end_us`].
    pub fn moving(&self, now_us: u64) -> bool {
        self.anim_ms(now_us) < gather::FINAL_MS
    }

    /// When the boot page ends by itself.
    pub fn end_us(&self) -> u64 {
        let ms = gather::END_MS.saturating_sub(self.anim.join_ms());
        self.start_us.unwrap_or(0) + u64::from(ms) * 1000
    }

    /// The picture at `now_us`. The colours are [`gather::PALETTE`]; the
    /// console sets them while the boot page shows.
    pub fn draw(&self, now_us: u64, f: &mut Frame) {
        gather::draw(self.anim_ms(now_us), self.anim == Anim::Short, &mut f.0);
    }
}
