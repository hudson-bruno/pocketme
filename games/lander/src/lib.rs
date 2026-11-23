//! LANDER: a moon lander. Bring a small craft down on a landing pad,
//! slowly and upright, before the fuel runs out.
//!
//! How it is built (README.md has the whole story):
//!
//! * [`app`]: the game's state and rules, apart from the console. It
//!   takes one call's [`app::Input`] and the game time. Tests drive it
//!   directly.
//! * [`craft`]: one physics tick and touching ground. [`level`]: each
//!   level's numbers and its ground, from a seed (the level number).
//!   [`fx`]: dust and broken pieces. [`fixed`]: whole-number maths.
//! * [`draw`]: the picture, drawn whole every call (the console sends
//!   only what changed).
//! * This file: the console side. It turns the [`Ctx`] into an
//!   [`app::Input`], sets the palette, asks for the frame rate and the
//!   video hint, and logs what happens.
//!
//! Physics runs in fixed ticks of 1/60 s taken from the game time, so a
//! run is the same on the PC and the chip, at 30 or 60 pictures a
//! second. Positions are Q16 (1/65536 pixel), angles 256 steps a turn.
//!
//! Uses only the game kit. No heap, no floating point, no `unsafe`.

#![no_std]

pub mod app;
pub mod craft;
pub mod draw;
pub mod fixed;
pub mod fx;
pub mod level;

use app::{App, Event, Input, View};
use game_kit::{Ctx, Flow, Game, hint, rate};

pub struct Lander {
    app: App,
    /// The video settings last asked for (rate, hint), so a refused
    /// request is not asked again every frame.
    asked: (u16, u16),
}

impl Lander {
    /// The rate and hint the game wants now.
    fn wanted(&self) -> (u16, u16) {
        let r = if self.app.still() {
            rate::ON_DEMAND
        } else if self.app.opts.fast {
            rate::FPS60
        } else {
            rate::FPS30
        };
        let h = match self.app.opts.view {
            View::Still => hint::MOSTLY_STILL,
            View::Follow => hint::SCROLLS,
        };
        (r, h)
    }
}

/// The console's input as the game's.
pub fn input_of(ctx: &Ctx) -> Input {
    let p1 = ctx.player(0);
    Input {
        held: p1.held_bits(),
        pressed: p1.pressed_bits(),
        left: ctx.left_stick(),
        right: ctx.right_stick(),
        triggers: ctx.triggers(),
        touch: ctx.touch(),
        tilt: ctx.motion().map(|m| m.accel[0]),
    }
}

impl Game for Lander {
    fn start(ctx: &mut Ctx) -> Self {
        ctx.log("lander: started");
        Lander {
            app: App::new(),
            asked: (u16::MAX, u16::MAX),
        }
    }

    fn update(&mut self, ctx: &mut Ctx) -> Flow {
        if ctx.started() || ctx.must_redraw() {
            ctx.set_colours(1, &draw::COLOURS);
        }
        let input = input_of(ctx);
        let going = self.app.update(&input, ctx.time_us());
        while let Some(e) = self.app.take_event() {
            log(ctx, e);
        }
        if !going {
            return Flow::Done;
        }
        self.app.draw(ctx.screen());

        let want = self.wanted();
        let now = ctx.video();
        if want != (now.rate, now.hint) && want != self.asked {
            self.asked = want;
            let mut v = now;
            v.rate = want.0;
            v.hint = want.1;
            if ctx.ask_video(v).is_err() {
                ctx.log("lander: video settings refused");
            }
        }
        Flow::Run
    }
}

fn log(ctx: &mut Ctx, e: Event) {
    match e {
        Event::Start { level } => ctx.log_line().text("lander: level ").num(level).end(),
        Event::Landed {
            level,
            points,
            score,
        } => ctx
            .log_line()
            .text("lander: landed on level ")
            .num(level)
            .text(", ")
            .num(points)
            .text(" points, score ")
            .num(score)
            .end(),
        Event::Crashed { level, why } => ctx
            .log_line()
            .text("lander: crashed on level ")
            .num(level)
            .text(": ")
            .text(why.text())
            .end(),
        Event::Lost { level } => ctx
            .log_line()
            .text("lander: lost on level ")
            .num(level)
            .end(),
        Event::Over { score, best } => ctx
            .log_line()
            .text("lander: game over, score ")
            .num(score)
            .text(", best ")
            .num(best)
            .end(),
        Event::View(View::Still) => ctx.log("lander: view still"),
        Event::View(View::Follow) => ctx.log("lander: view follow"),
        Event::Quit => ctx.log("lander: the end"),
    }
}

game_kit::game!(Lander);
