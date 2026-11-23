//! The renderer with the feature `swap`: the two pictures swap roles at
//! each present, so that another core can send one picture while the
//! console already draws the next one in the other.
//!
//! Without the feature, `src/video.rs` is used instead and this file is
//! not compiled at all. The rule, the cases and the costs are written
//! down in the crate README, "Swap: two cores, two pictures".
//!
//! In short. There are two pictures, A and B. One is "drawn" (the
//! console and a game write into it); the other is "shown" (what the
//! panel shows, or is being sent to it). At a present:
//!
//! 1. Find what changed: compare shown with drawn. Only reads.
//! 2. [`Sender::wait_free`]: the sender lets go of the picture it got
//!    last time (that is the shown one). From now on nobody reads it.
//! 3. [`Sender::hand_over`]: the drawn picture goes to the sender, which
//!    may read it (only read) until its next `wait_free` returns.
//! 4. Catch up: copy the changed rectangles from the handed-over picture
//!    into the other one (writes only the one let go in step 2, reads
//!    the handed-over one, which the sender also only reads).
//! 5. Swap roles: the console goes on drawing in the caught-up picture.
//!
//! So the console never writes the picture the sender holds, and it
//! never needs a third copy. No `unsafe` here: the sender gets a plain
//! shared reference, and only a board that keeps it past the call (to
//! read it on another core) needs `unsafe`, in its own code.

use board_api::{Rect, Screen, SendReport};

use crate::areas::{Areas, MAX_AREAS, diff};
use crate::frame::{Frame, H, W};
use crate::palette::Palette;
use crate::source::{Source, View};

/// One picture handed to a [`Sender`] (made by [`Video::present_handing`]
/// on the stack, about 40 bytes).
///
/// What the sender may use, and how long:
///
/// | field                  | may be read                                   |
/// |------------------------|-----------------------------------------------|
/// | `frame` (19,200 bytes) | until the sender's next `wait_free` returns    |
/// | `palette` (516 bytes)  | only during `hand_over`: copy what you need   |
/// | `areas` (at most 16)   | only during `hand_over`: copy them            |
/// | `view`, `border`       | plain values: copy them                       |
///
/// Nobody may write any of it through these references (they are shared
/// references; a board that turns `frame` into a raw pointer must still
/// only read through it).
#[derive(Clone, Copy)]
pub struct Handed<'a> {
    /// The picture to send. The console does not write it until the
    /// sender's next [`Sender::wait_free`] has returned.
    pub frame: &'a Frame,
    /// The colours. The console's own palette, which the console (and a
    /// running game) may change right after `hand_over` returns.
    pub palette: &'a Palette,
    /// Where the picture sits on the panel.
    pub view: View,
    /// RGB565 colour of panel pixels outside the picture.
    pub border: u16,
    /// What to send, in panel pixels (the same list `Screen::present`
    /// gets without the feature).
    pub areas: &'a [Rect],
}

impl<'a> Handed<'a> {
    /// The finished-pixel source for this picture, as
    /// `Screen::present` gets it. Valid while the palette is: during
    /// `hand_over` only, unless the board builds its own source from a
    /// palette copy (see the crate README).
    pub fn source(&self) -> Source<'a> {
        Source::new(self.frame, self.palette, self.view, self.border)
    }
}

/// Whoever sends the pictures: the board's second core, or (with
/// [`Inline`]) the board's screen right away.
///
/// The console calls `wait_free` and then `hand_over`, in that order,
/// once per present that changes something; and `wait_free` alone
/// whenever it needs the last picture to be on the panel (before it
/// lights the backlight).
pub trait Sender {
    /// Returns once the sender reads nothing of any picture handed over
    /// before (at once when it holds none). After it returns, the console
    /// may write that picture again. A board that sends on another core
    /// waits here for that core (with an acquire load of the "done" flag
    /// the other core sets with release, after its last read).
    fn wait_free(&mut self);

    /// Starts sending `h`. May return before the picture is sent. The
    /// board may keep reading `h.frame` (and only that) until its next
    /// `wait_free` returns; everything else in `h` it must copy now.
    /// Only called right after `wait_free`, so the sender holds nothing
    /// else at this moment.
    ///
    /// The report: what the board knows when this returns. A board that
    /// sends right away gives the real report. A board that sends on
    /// another core gives at least the areas and pixels; its times may be
    /// those of the send before (say which in the board's notes).
    fn hand_over(&mut self, h: &Handed<'_>) -> SendReport;
}

/// The [`Sender`] that sends at once, on the calling core, through a
/// [`Screen`]: what every board with one core does. [`Video::present`]
/// uses it.
pub struct Inline<'s, S: Screen>(pub &'s mut S);

impl<S: Screen> Sender for Inline<'_, S> {
    fn wait_free(&mut self) {}

    fn hand_over(&mut self, h: &Handed<'_>) -> SendReport {
        self.0.present(&h.source(), h.areas)
    }
}

/// The two pictures, the palette, and which picture is drawn. The same
/// size as without the feature (the extra flag fits in padding; see the
/// crate README, "Swap"). Make it once and keep it (for example in a
/// `static` on the chip): `new` is `const`, and all zero bytes.
///
/// A board that sends on another core must not move the `Video` (or the
/// console holding it) while a picture is handed over: the sender reads
/// it where it was. A `static` never moves.
pub struct Video {
    /// The two pictures. `pics[drawn_index]` is drawn; the other one is
    /// shown (and may be held by the sender).
    pics: [Frame; 2],
    palette: Palette,
    /// The palette version the panel shows; a different one forces a
    /// full send.
    shown_palette: u32,
    /// The panel shows what the shown picture holds (see `video.rs`:
    /// "in step", so that a new `Video` is all zero bytes).
    in_step: bool,
    /// Which picture is drawn: `false` = `pics[0]` (so a new `Video` is
    /// all zeros).
    second: bool,
    border: u16,
    areas: Areas,
}

impl Video {
    pub const fn new() -> Self {
        Video {
            pics: [Frame::new(), Frame::new()],
            palette: Palette::new(),
            shown_palette: 0,
            in_step: false,
            second: false,
            border: 0,
            areas: Areas::new(),
        }
    }

    fn drawn_index(&self) -> usize {
        usize::from(self.second)
    }

    /// The picture to draw into. Never the one a sender may hold.
    pub fn frame_mut(&mut self) -> &mut Frame {
        let i = self.drawn_index();
        &mut self.pics[i]
    }

    /// The picture as drawn so far (for screenshots). Never the one a
    /// sender may hold; it changes only when the console draws, so it is
    /// steady between two steps.
    pub fn frame(&self) -> &Frame {
        &self.pics[self.drawn_index()]
    }

    pub fn palette_mut(&mut self) -> &mut Palette {
        &mut self.palette
    }

    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// The picture being drawn and the 256 palette colours, for a game to
    /// draw into during one call (see `video.rs`). With `swap` the
    /// picture's address changes from one present to the next (the game
    /// interface allows that: the pointers are valid for one call only).
    pub fn game_buffers(&mut self) -> (&mut [u8; W * H], &mut [u16; 256]) {
        let i = self.drawn_index();
        (&mut self.pics[i].0, self.palette.colours_mut())
    }

    /// RGB565 colour around the picture, when it does not fill the panel.
    pub fn set_border(&mut self, rgb565: u16) {
        if self.border != rgb565 {
            self.border = rgb565;
            self.in_step = false;
        }
    }

    /// Send the whole screen at the next present.
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
        let shown: &Frame = &self.pics[1 - self.drawn_index()];
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

    /// Finds what changed and has `screen` send it right away (through
    /// [`Inline`]). The same pixels, areas and report as without `swap`.
    pub fn present<S: Screen>(&mut self, screen: &mut S) -> SendReport {
        self.present_handing::<S, _>(&mut Inline(screen))
    }

    /// Finds what changed since the last present and hands the drawn
    /// picture to `out`; then goes on in the other picture, brought up
    /// to date first. `S` gives the panel size only. Nothing changed:
    /// `out` is not called, nothing swaps, and the report is all zeros.
    ///
    /// The steps (numbers as in the file notes): 1 find, 2 wait_free,
    /// 3 hand_over, 4 catch up, 5 swap.
    pub fn present_handing<S: Screen, O: Sender + ?Sized>(&mut self, out: &mut O) -> SendReport {
        let view = View::fit(S::PANEL_W, S::PANEL_H);
        let full = !self.in_step || self.palette.version() != self.shown_palette;
        let second = self.second;
        let [a, b] = &mut self.pics;
        let (drawn, shown) = if second { (b, a) } else { (a, b) };
        // 1. Find. `shown` may still be read by the sender: both only read.
        if full {
            self.areas = Areas::full();
        } else {
            diff(shown, drawn, &mut self.areas);
        }
        if self.areas.is_empty() {
            return SendReport::default();
        }
        // The same rectangles in panel pixels (as in `video.rs`).
        let mut panel = [Rect::EMPTY; MAX_AREAS];
        let n = self.areas.len();
        for (p, r) in panel.iter_mut().zip(self.areas.as_slice()) {
            *p = view.to_panel(*r);
        }
        if !self.in_step {
            panel[0] = Rect::new(0, 0, S::PANEL_W, S::PANEL_H);
        }
        // 2. The sender lets go of `shown`.
        out.wait_free();
        // 3. `drawn` goes to the sender: from here on, read only.
        let report = out.hand_over(&Handed {
            frame: drawn,
            palette: &self.palette,
            view,
            border: self.border,
            areas: &panel[..n],
        });
        // 4. Catch up: `shown` (let go in step 2) becomes equal to
        //    `drawn` (only read here). Outside the changed rectangles the
        //    two are already equal (the invariant after every present).
        if full {
            shown.0.copy_from_slice(&drawn.0);
        } else {
            for r in self.areas.as_slice() {
                copy_rect(drawn, shown, *r);
            }
        }
        // 5. Swap: the console goes on in the caught-up picture.
        self.second = !second;
        self.shown_palette = self.palette.version();
        self.in_step = true;
        report
    }
}

/// Copies the pixels of `r` (picture pixels, inside the picture) from
/// `from` to `to`.
fn copy_rect(from: &Frame, to: &mut Frame, r: Rect) {
    let (x, w) = (usize::from(r.x), usize::from(r.w));
    for y in usize::from(r.y)..usize::from(r.bottom()) {
        let at = y * W + x;
        to.0[at..at + w].copy_from_slice(&from.0[at..at + w]);
    }
}

impl Default for Video {
    fn default() -> Self {
        Self::new()
    }
}
