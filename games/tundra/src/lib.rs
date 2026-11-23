//! TUNDRA: top-down arcade ice hockey on one screen, for 1 to 4 players.
//! Two teams of two skaters and a goalie; the computer plays every
//! skater no person holds, and both goalies.
//!
//! How it is built (README.md has the rules and the controls):
//!
//! * [`fixed`]: whole-number maths (lengths, directions, chance).
//! * [`play`]: one match in fixed ticks of 1/60 s: skating, the puck,
//!   the boards, goalies, checks, goals, the clock.
//! * [`cpu`]: the computer's skaters, EASY or HARD.
//! * [`app`]: the screens and menus, and who drives which skater (pads,
//!   the touch screen as player 2, the computer).
//! * [`draw`]: the palette and the painting.
//! * This file: the console side. It turns the [`Ctx`] into
//!   [`app::Keys`], sets the palette, asks for the frame rate (none while
//!   nothing moves) and logs what happens.
//!
//! Uses only the game kit. No heap, no floating point, no `unsafe`.

#![no_std]

pub mod app;
pub mod cpu;
pub mod draw;
pub mod fixed;
pub mod play;

use app::{App, Ctl, Keys, Log, PadIn};
use game_kit::{Ctx, Flow, Game, PLAYERS, hint, rate};

pub struct Tundra {
    pub app: App,
    /// The rate last asked for, so a refused request is not asked again
    /// every frame.
    asked: u16,
}

impl Tundra {
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

const START: Tundra = Tundra {
    app: App::new(),
    asked: u16::MAX,
};

impl Game for Tundra {
    #[inline(never)]
    fn start(ctx: &mut Ctx) -> Self {
        ctx.log("tundra: started");
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

        let want = Tundra::wanted(&self.app);
        let now = ctx.video();
        if (want != now.rate || now.hint != hint::MOSTLY_STILL) && want != self.asked {
            self.asked = want;
            let mut v = now;
            v.rate = want;
            v.hint = hint::MOSTLY_STILL;
            if ctx.ask_video(v).is_err() {
                ctx.log("tundra: video settings refused");
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
        Log::Match {
            who: w,
            level,
            length,
            fast,
        } => ctx
            .log_line()
            .text("tundra: match, p1 ")
            .text(who(w[0]))
            .text(" p2 ")
            .text(who(w[1]))
            .text(" p3 ")
            .text(who(w[2]))
            .text(" p4 ")
            .text(who(w[3]))
            .text(", computer ")
            .text(level.name())
            .text(", ")
            .text(length.name())
            .text(", rate ")
            .text(if fast { "60" } else { "30" })
            .end(),
        Log::Goal { team, score, clock } => ctx
            .log_line()
            .text("tundra: goal ")
            .text(draw::TEAM[usize::from(team)])
            .text(", ")
            .num(score[0])
            .text("-")
            .num(score[1])
            .text(", clock ")
            .num(clock / 60)
            .text("s")
            .end(),
        Log::Final { score, series } => ctx
            .log_line()
            .text("tundra: final ")
            .num(score[0])
            .text("-")
            .num(score[1])
            .text(if score[0] == score[1] { " draw" } else { "" })
            .text(", series ")
            .num(series[0])
            .text("-")
            .num(series[1])
            .end(),
        Log::TouchPlays => ctx.log("tundra: touch plays player 2"),
        Log::Pause => ctx.log("tundra: pause"),
        Log::Resume => ctx.log("tundra: resume"),
        Log::Leave => ctx.log("tundra: leave"),
    }
}

game_kit::game!(Tundra);
