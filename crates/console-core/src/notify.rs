//! Notifications: a small panel slides down from the top, stays a few
//! seconds, and slides back up. Messages wait in a queue, so two in a row
//! are both shown, one after the other.
//!
//! Memory: [`MAX_NOTICES`] x ([`NOTICE_LEN`] + 2) bytes, plus a time:
//! 160 bytes on the PC (with padding). No heap.

use console_video::Frame;

use crate::gfx::{self, CX, Text};
use crate::theme::{GOLD, INK, NOTE_H, NOTE_SHOW_US, NOTE_SLIDE_US, NOTE_Y, OK, WARN};

/// Most messages waiting at once (fixed size: no heap). A message that
/// comes when the queue is full is dropped.
pub const MAX_NOTICES: usize = 4;
/// Longest message in bytes (longer ones are cut). 32 letters of the
/// font are about 130 pixels: they still fit the 160-pixel screen.
pub const NOTICE_LEN: usize = 32;
/// How long one message shows, from start of sliding in to end of
/// sliding out, in microseconds.
pub const SHOW_US: u64 = 2 * NOTE_SLIDE_US + NOTE_SHOW_US;

/// The colour of a message.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    /// Gold: plain news.
    #[default]
    Info,
    /// Green: something good happened.
    Good,
    /// Orange: something did not work.
    Warn,
}

#[derive(Clone, Copy, Default)]
struct Notice {
    text: Text<NOTICE_LEN>,
    tone: Tone,
}

/// The queue. The first message is the one on show.
#[derive(Default)]
pub struct Notices {
    queue: [Notice; MAX_NOTICES],
    len: usize,
    /// When the first message started to show (`None`: not yet; it
    /// starts at the next [`Notices::update`]).
    start_us: Option<u64>,
}

impl Notices {
    pub const fn new() -> Self {
        Notices {
            queue: [Notice {
                text: Text::new(),
                tone: Tone::Info,
            }; MAX_NOTICES],
            len: 0,
            start_us: None,
        }
    }

    /// Adds a message (cut to [`NOTICE_LEN`] bytes). Returns false, and
    /// drops it, when [`MAX_NOTICES`] are already waiting. It shows from
    /// the next frame on (or after the ones before it).
    pub fn push(&mut self, text: &str, tone: Tone) -> bool {
        if self.len == MAX_NOTICES {
            return false;
        }
        self.queue[self.len] = Notice {
            text: Text::of(text),
            tone,
        };
        self.len += 1;
        true
    }

    /// Messages waiting, the one on show included.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The message on show, if any.
    pub fn current(&self) -> Option<&str> {
        (self.len > 0).then(|| self.queue[0].text.as_str())
    }

    /// Ends the message whose time is over and starts the next one.
    pub fn update(&mut self, now_us: u64) {
        if self.len == 0 {
            return;
        }
        let start = *self.start_us.get_or_insert(now_us);
        if now_us >= start + SHOW_US {
            self.queue.copy_within(1..self.len, 0);
            self.len -= 1;
            self.start_us = (self.len > 0).then_some(now_us);
        }
    }

    /// How far the message on show is into its time.
    fn elapsed(&self, now_us: u64) -> Option<u64> {
        let start = self.start_us?;
        (self.len > 0).then(|| now_us.saturating_sub(start))
    }

    /// True while the panel slides (then every frame shows a change).
    pub fn moving(&self, now_us: u64) -> bool {
        match self.elapsed(now_us) {
            Some(e) => !(NOTE_SLIDE_US..NOTE_SLIDE_US + NOTE_SHOW_US).contains(&e),
            None => self.len > 0,
        }
    }

    /// When the panel next starts to move, while it stays still.
    pub fn next_change(&self, now_us: u64) -> Option<u64> {
        let start = self.start_us?;
        let e = self.elapsed(now_us)?;
        (e < NOTE_SLIDE_US + NOTE_SHOW_US).then_some(start + NOTE_SLIDE_US + NOTE_SHOW_US)
    }

    pub fn draw(&self, now_us: u64, f: &mut Frame) {
        let Some(e) = self.elapsed(now_us) else {
            return;
        };
        let n = &self.queue[0];
        // How much of the slide is done, 0..=SLIDE (in, stay, then out).
        let s = if e < NOTE_SLIDE_US {
            e
        } else if e < NOTE_SLIDE_US + NOTE_SHOW_US {
            NOTE_SLIDE_US
        } else {
            SHOW_US.saturating_sub(e)
        };
        // From fully above the screen to resting at NOTE_Y.
        let travel = (NOTE_Y + NOTE_H as i16) as u64;
        let y = -(NOTE_H as i16) + (travel * s / NOTE_SLIDE_US) as i16;
        let text = n.text.as_str();
        let w = gfx::width(text, 1) + 12;
        let x = CX - w / 2;
        let fill = match n.tone {
            Tone::Info => GOLD,
            Tone::Good => OK,
            Tone::Warn => WARN,
        };
        f.rect(x, y, w as u16, NOTE_H, INK);
        f.rect(x + 1, y + 1, w as u16 - 2, NOTE_H - 2, fill);
        f.text(x + 6, y + 4, text, INK);
    }
}
