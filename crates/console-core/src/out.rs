//! How a step's picture leaves the console: straight to the board's
//! screen ([`Direct`], always there), or handed to a sender on another
//! core ([`Handing`], only with the feature `swap`; see
//! `crates/console-video/README.md`, "Swap: two cores, two pictures").

use board_api::{Screen, SendReport};
use console_video::Video;

/// One way out for the picture.
pub(crate) trait Out<S: Screen> {
    /// Sends (or hands over) what changed. Called once per step.
    fn present(&mut self, video: &mut Video, screen: &mut S) -> SendReport;

    /// Returns once the last picture handed over is on the panel and the
    /// sender uses the screen no more. The console calls it before it
    /// uses the screen itself (the backlight).
    fn settle(&mut self) {}
}

/// The screen sends at once, on this core (every board without `swap`,
/// and `Console::step` with it).
pub(crate) struct Direct;

impl<S: Screen> Out<S> for Direct {
    #[inline(always)]
    fn present(&mut self, video: &mut Video, screen: &mut S) -> SendReport {
        video.present(screen)
    }
}

/// The board's sender gets the picture (`Console::step_with_sender`).
#[cfg(feature = "swap")]
pub(crate) struct Handing<'a, O: console_video::Sender + ?Sized>(pub(crate) &'a mut O);

#[cfg(feature = "swap")]
impl<S: Screen, O: console_video::Sender + ?Sized> Out<S> for Handing<'_, O> {
    fn present(&mut self, video: &mut Video, _screen: &mut S) -> SendReport {
        video.present_handing::<S, O>(self.0)
    }

    fn settle(&mut self) {
        self.0.wait_free();
    }
}
