//! The renderer as one piece: draw into it, then present each frame.

use board_api::{Screen, SendReport};

use crate::areas::{Areas, MAX_AREAS, diff};
use crate::frame::{Frame, H, W};
use crate::palette::Palette;
use crate::source::{Source, View};

/// The picture being drawn, a copy of what the panel shows, and the
/// palette. About 39 KB (see the crate notes). Make it once and keep it
/// (for example in a `static` on the chip): `new` is `const`.
pub struct Video {
    cur: Frame,
    shown: Frame,
    palette: Palette,
    /// The palette version the panel shows; a different one forces a
    /// full send.
    shown_palette: u32,
    /// The panel shows what `shown` holds, so only the changes need to
    /// be sent. `false` sends everything next time: at start, after a
    /// border change, or when asked.
    ///
    /// (It is "in step", not "send everything", so that a new `Video` is
    /// all zero bytes. Then a `static` holding the console takes no room
    /// in the chip's flash and costs no copy at start-up: see
    /// `crates/console-core/README.md`, "Starting as zeros".)
    in_step: bool,
    border: u16,
    areas: Areas,
}

impl Video {
    pub const fn new() -> Self {
        Video {
            cur: Frame::new(),
            shown: Frame::new(),
            palette: Palette::new(),
            shown_palette: 0,
            in_step: false,
            border: 0,
            areas: Areas::new(),
        }
    }

    /// The picture to draw into.
    pub fn frame_mut(&mut self) -> &mut Frame {
        &mut self.cur
    }

    /// The picture as drawn so far (for screenshots).
    pub fn frame(&self) -> &Frame {
        &self.cur
    }

    pub fn palette_mut(&mut self) -> &mut Palette {
        &mut self.palette
    }

    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// The picture being drawn (19,200 bytes, row after row, aligned to
    /// 4) and the 256 palette colours, for a game to draw into during one
    /// call (the console wraps them in a `game_api::LiveFrame`). Both come
    /// from this one borrow, so they stay usable together until `self` is
    /// used again. After colours were written this way, call
    /// [`Palette::touch`] (through [`Video::palette_mut`]).
    pub fn game_buffers(&mut self) -> (&mut [u8; W * H], &mut [u16; 256]) {
        (&mut self.cur.0, self.palette.colours_mut())
    }

    /// RGB565 colour around the picture, when it does not fill the panel.
    pub fn set_border(&mut self, rgb565: u16) {
        if self.border != rgb565 {
            self.border = rgb565;
            self.in_step = false;
        }
    }

    /// Send the whole screen at the next [`Video::present`].
    pub fn force_full(&mut self) {
        self.in_step = false;
    }

    /// Changes colours with `set` (through [`Palette::set`] or
    /// [`Palette::set_all`]) without the whole screen being sent for it,
    /// when no pixel of the picture on the panel uses a changed colour:
    /// the panel then looks the same with the new colours, and the next
    /// present sends only what was drawn differently. If a pixel on the
    /// panel uses a changed colour (or the panel is not in step anyway),
    /// the next present sends the whole screen, as after any colour
    /// change. The palette's version changes as always, so a board that
    /// keeps the colours in another form still rebuilds them.
    ///
    /// For a page that hands over to another page with other colours
    /// (console-core's boot animation to the home screen). Costs one
    /// look at every pixel of the picture the panel shows (19,200).
    pub fn recolour_unseen(&mut self, set: impl FnOnce(&mut Palette)) {
        let before = *self.palette.colours();
        let was_in_step = self.in_step && self.palette.version() == self.shown_palette;
        set(&mut self.palette);
        if !was_in_step || self.palette.version() == self.shown_palette {
            return;
        }
        let now = self.palette.colours();
        // One bit per palette number: its colour changed.
        let mut changed = [0u32; 8];
        for (i, (a, b)) in before.iter().zip(now).enumerate() {
            if a != b {
                changed[i / 32] |= 1 << (i % 32);
            }
        }
        let shown: &Frame = &self.shown;
        let seen = shown
            .0
            .iter()
            .any(|&p| changed[usize::from(p / 32)] >> (p % 32) & 1 != 0);
        if !seen {
            self.shown_palette = self.palette.version();
        }
    }

    /// The changed rectangles of the last present, in picture pixels.
    pub fn last_areas(&self) -> &Areas {
        &self.areas
    }

    /// Finds what changed since the last call, has the board send it, and
    /// remembers what the panel now shows. Nothing changed: the board is
    /// not called and the report is all zeros.
    pub fn present<S: Screen>(&mut self, screen: &mut S) -> SendReport {
        let view = View::fit(S::PANEL_W, S::PANEL_H);
        if !self.in_step || self.palette.version() != self.shown_palette {
            self.areas = Areas::full();
        } else {
            diff(&self.shown, &self.cur, &mut self.areas);
        }
        if self.areas.is_empty() {
            return SendReport::default();
        }
        // The same rectangles in panel pixels. When the picture does not
        // fill the panel, a full send also covers the border.
        let mut panel = [board_api::Rect::EMPTY; MAX_AREAS];
        let n = self.areas.len();
        for (p, a) in panel.iter_mut().zip(self.areas.as_slice()) {
            *p = view.to_panel(*a);
        }
        if !self.in_step {
            panel[0] = board_api::Rect::new(0, 0, S::PANEL_W, S::PANEL_H);
        }
        let src = Source::new(&self.cur, &self.palette, view, self.border);
        let report = screen.present(&src, &panel[..n]);
        self.shown.0.copy_from_slice(&self.cur.0);
        self.shown_palette = self.palette.version();
        self.in_step = true;
        report
    }
}

impl Default for Video {
    fn default() -> Self {
        Self::new()
    }
}
