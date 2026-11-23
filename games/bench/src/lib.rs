//! BENCH: a speed test for the console. Each level is a fixed scene that
//! repeats for ever at one load; the ramp raises the load by itself and
//! finds the largest that fits 60 and 30 pictures a second.
//!
//! * [`load`]: the tests (fills, triangles of 20 / 100 / 400 pixels,
//!   corners, 3D scenes of 12 / 32 / 96-triangle balls, screen sends,
//!   spinning triangles, the city) and their workloads.
//! * [`city`]: CITY, a street through a city with traffic, drawn with
//!   the same renderer (the showcase scene).
//! * [`ramp`]: the search, and how a game can time itself from the time
//!   between its calls.
//! * [`app`]: the screens, the keys, the log lines.
//! * [`poly`], [`fixed`], [`model`]: the 3D (MERCATOR's filler and model
//!   tables, a renderer with 32-bit sums). [`text`], [`art`]: numbers
//!   and colours.
//! * This file: the console side. It turns the [`Ctx`] into
//!   [`app::Input`], sets the palette, asks for the rate and logs.
//!
//! README.md has the keys and how to read the numbers. Uses only the
//! game kit. No heap, no floating point, no `unsafe`.

#![no_std]

pub mod app;
pub mod art;
pub mod city;
pub mod fixed;
pub mod island;
pub mod load;
pub mod model;
pub mod poly;
pub mod ramp;
pub mod text;

use app::{App, Input};
use game_kit::{Ctx, Flow, Game, rate};
use load::Work;

pub struct Bench {
    pub app: App,
    /// The workloads' memory (renderer, island work space).
    pub work: Work,
    /// The rate last asked for (`u16::MAX`: none pending).
    asked: u16,
}

/// The work space at the start: all zero bytes, so the start value
/// fills it with zeros instead of copying a constant (a `const` start
/// value of the whole game would keep an 8 KB copy among the data
/// slot's constants; built while running, it needed a 12 KB stack frame).
// With the PC-only `check` feature the renderer keeps a recorder of about 180 KB.
#[cfg_attr(feature = "check", allow(clippy::large_const_arrays))]
const WORK0: Work = Work::new();

impl Game for Bench {
    #[inline(never)]
    fn start(ctx: &mut Ctx) -> Self {
        say(ctx, b"bench: started");
        Bench {
            app: App::new(),
            work: WORK0,
            asked: u16::MAX,
        }
    }

    fn update(&mut self, ctx: &mut Ctx) -> Flow {
        let fresh = ctx.started() || ctx.resumed() || ctx.must_redraw();
        if fresh {
            ctx.set_colours(0, &art::PALETTE);
        }
        let now = ctx.video().rate;
        let inp = Input {
            pressed: ctx.player(0).pressed_bits(),
            delta_us: ctx.delta_us(),
            fresh,
            rate60_now: now == rate::FPS60,
        };
        let out = self.app.update(&inp, ctx.screen(), &mut self.work);
        if !self.app.log.is_empty() {
            let log = self.app.log;
            for line in log.bytes().split(|&b| b == b'\n') {
                say(ctx, line);
            }
        }
        if out.quit {
            say(ctx, b"bench: leave");
            return Flow::Done;
        }
        let want = if out.rate60 { rate::FPS60 } else { rate::FPS30 };
        if now == want {
            self.asked = u16::MAX;
        } else if self.asked != want {
            self.asked = want;
            if ctx.set_rate(want).is_err() {
                say(ctx, b"bench: rate refused");
            }
        }
        Flow::Run
    }
}

/// A text as a log argument written as it is: `Ctx::log` formats with
/// `{}`, which brings in the formatter's padding code; this writes the
/// letters one by one (MERCATOR's way).
struct Plain<'a>(&'a [u8]);

impl core::fmt::Display for Plain<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for &b in self.0 {
            f.write_str(text::letter(b))?;
        }
        Ok(())
    }
}

/// One log line.
#[inline(never)]
fn say(ctx: &mut Ctx, line: &[u8]) {
    ctx.log_fmt(format_args!("{}", Plain(line)));
}

game_kit::game!(Bench);
