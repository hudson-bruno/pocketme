//! SKIMMER: air hockey on one screen, for 1 to 4 players. One mallet a
//! side (two in 2 AGAINST 2), a fast puck, a goal slot at each end.
//!
//! How it is built (README.md has the rules and the controls):
//!
//! * [`fixed`]: whole-number maths (lengths, directions, chance).
//! * [`table`]: one match in fixed ticks of 1/60 s: the puck, the
//!   mallets, the rails, goals, serves, the clock. The puck moves in
//!   small steps, so it never passes through a mallet or a rail.
//! * [`cpu`]: the computer's mallets, EASY, NORMAL or HARD.
//! * [`app`]: the screens and menus, and who drives which mallet (pads,
//!   the touch screen, the computer).
//! * [`draw`]: the palette and the painting.
//! * This file: the console side. It turns the [`Ctx`] into
//!   [`app::Keys`], sets the palette, asks for the frame rate (none while
//!   nothing moves) and logs what happens.
//!
//! Uses only the game kit. No heap, no floating point, no `unsafe`.

#![no_std]

#[cfg(test)]
extern crate std;

pub mod app;
pub mod cpu;
pub mod draw;
pub mod fixed;
pub mod table;

use app::{App, Ctl, Keys, Log, PadIn};
use game_kit::{Ctx, Flow, Game, PLAYERS, hint, rate};

pub struct Skimmer {
    pub app: App,
    /// The rate last asked for, so a refused request is not asked again
    /// every frame.
    asked: u16,
}

impl Skimmer {
    /// The frame rate the game wants now: none while nothing moves.
    pub fn wanted(app: &App) -> u16 {
        if app.still() {
            rate::ON_DEMAND
        } else if app.opts.fast {
            rate::FPS60
        } else {
            rate::FPS30
        }
    }
}

/// The console's input as the game's.
pub fn keys_of(ctx: &Ctx) -> Keys {
    let mut pads = [PadIn::default(); 4];
    for (n, p) in pads.iter_mut().enumerate().take(PLAYERS) {
        let pl = ctx.player(n);
        *p = PadIn {
            connected: pl.connected(),
            held: pl.held_bits(),
            pressed: pl.pressed_bits(),
            stick: pl.left_stick(),
        };
    }
    Keys {
        pads,
        touch: ctx.touch(),
        delta_us: ctx.delta_us(),
        frame: ctx.frame(),
    }
}

const START: Skimmer = Skimmer {
    app: App::new(),
    asked: u16::MAX,
};

impl Game for Skimmer {
    #[inline(never)]
    fn start(ctx: &mut Ctx) -> Self {
        ctx.log("skimmer: started");
        START
    }

    fn update(&mut self, ctx: &mut Ctx) -> Flow {
        if ctx.started() || ctx.must_redraw() {
            ctx.set_colours(0, &draw::COLOURS);
        }
        let keys = keys_of(ctx);
        let going = self.app.update(&keys);
        while let Some(l) = self.app.take_log() {
            log(ctx, l);
        }
        if !going {
            return Flow::Done;
        }
        draw::draw(&self.app, ctx.screen());

        let want = Skimmer::wanted(&self.app);
        let now = ctx.video();
        if (want != now.rate || now.hint != hint::MOSTLY_STILL) && want != self.asked {
            self.asked = want;
            let mut v = now;
            v.rate = want;
            v.hint = hint::MOSTLY_STILL;
            if ctx.ask_video(v).is_err() {
                ctx.log("skimmer: video settings refused");
            }
        }
        Flow::Run
    }
}

fn who(c: Ctl) -> &'static str {
    match c {
        Ctl::Cpu => "cpu",
        Ctl::Pad(_) => "pad",
        Ctl::Touch => "touch",
    }
}

fn log(ctx: &mut Ctx, l: Log) {
    match l {
        Log::Match { who: w, opts } => ctx
            .log_line()
            .text("skimmer: match, coral ")
            .text(who(w[0]))
            .text(" teal ")
            .text(who(w[1]))
            .text(if opts.four { ", 2 vs 2" } else { "" })
            .text(", computer ")
            .text(opts.level.name())
            .text(", ")
            .text(opts.length.name())
            .text(", ")
            .text(if opts.wild { "wild" } else { "plain" })
            .text(", rate ")
            .text(if opts.fast { "60" } else { "30" })
            .end(),
        Log::Goal { side, score } => ctx
            .log_line()
            .text("skimmer: goal ")
            .text(draw::SIDE[usize::from(side)])
            .text(", ")
            .num(score[0])
            .text("-")
            .num(score[1])
            .end(),
        Log::Stall { to } => ctx
            .log_line()
            .text("skimmer: too slow, puck to ")
            .text(draw::SIDE[usize::from(to)])
            .end(),
        Log::Final { score, series } => ctx
            .log_line()
            .text("skimmer: final ")
            .num(score[0])
            .text("-")
            .num(score[1])
            .text(if score[0] == score[1] { " draw" } else { "" })
            .text(", series ")
            .num(series[0])
            .text("-")
            .num(series[1])
            .end(),
        Log::TouchPlays => ctx.log("skimmer: touch plays teal"),
        Log::Pause => ctx.log("skimmer: pause"),
        Log::Resume => ctx.log("skimmer: resume"),
        Log::Leave => ctx.log("skimmer: leave"),
    }
}

game_kit::game!(Skimmer);
