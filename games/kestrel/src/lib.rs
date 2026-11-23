//! KESTREL: fly a small courier craft down a drifting rock belt, gate
//! by gate, and dock at HALDEN station before the clock runs out. Real
//! 3D: every rock, wreck, gate and the craft itself are models made of
//! flat-shaded triangles, turned, seen in perspective and filled.
//!
//! How it is built (README.md has the whole story):
//!
//! * [`poly`]: the 3D, in whole numbers and on its own (another game can
//!   take it with [`fixed`]): triangle filler, clipping, back faces,
//!   painter's order, flat shading through the palette.
//! * [`models`]: the model tables. [`art`]: the palette and the backdrop.
//! * [`course`]: the three missions; everything along a course is worked
//!   out from a hash, nothing is stored. [`world`]: one mission in
//!   flight, in fixed ticks of 1/60 s.
//! * [`scene`]: settings, camera, what is drawn in which order.
//!   [`hud`]: everything round the view. [`app`]: the screens and the
//!   controls.
//! * This file: the console side. It turns the [`Ctx`] into
//!   [`app::Keys`], sets the palette, asks for the picture rate and the
//!   video hint, and logs what happens.
//!
//! Uses only the game kit. No heap, no floating point, no `unsafe`.

#![no_std]

pub mod app;
pub mod art;
pub mod course;
pub mod fixed;
pub mod hud;
pub mod models;
pub mod poly;
pub mod scene;
pub mod world;

use app::{App, Event, Keys};
use game_kit::{Ctx, Flow, Game, hint, rate};

pub struct Kestrel {
    pub app: App,
    /// The video settings last asked for (rate, hint), so a refused
    /// request is not asked again every frame.
    asked: (u16, u16),
    /// Mission 1 is loaded in place on the first update, not in `start`,
    /// so the start needs no big value on the stack.
    loaded: bool,
}

impl Kestrel {
    /// The rate and hint the game wants now.
    pub fn wanted(app: &App) -> (u16, u16) {
        let r = if app.still() {
            rate::ON_DEMAND
        } else if app.opts.fast {
            rate::FPS60
        } else {
            rate::FPS30
        };
        (r, hint::MOSTLY_STILL)
    }
}

/// The console's input as the game's.
pub fn keys_of(ctx: &Ctx) -> Keys {
    let p1 = ctx.player(0);
    Keys {
        held: p1.held_bits(),
        pressed: p1.pressed_bits(),
        left: ctx.left_stick(),
        right: ctx.right_stick(),
        triggers: ctx.triggers(),
        touch: ctx.touch(),
    }
}

/// The game before mission 1 is loaded, as a constant: `start` copies
/// it straight into the console's slot.
const START: Kestrel = Kestrel {
    app: App::blank(),
    asked: (u16::MAX, u16::MAX),
    loaded: false,
};

impl Game for Kestrel {
    // Not inlined, so the copy of START stays out of the console's
    // update frame.
    #[inline(never)]
    fn start(ctx: &mut Ctx) -> Self {
        ctx.log("kestrel: started");
        START
    }

    fn update(&mut self, ctx: &mut Ctx) -> Flow {
        if !self.loaded {
            self.app.load();
            self.loaded = true;
        }
        if ctx.started() || ctx.must_redraw() {
            ctx.set_colours(0, &art::COLOURS);
        }
        let keys = keys_of(ctx);
        let going = self.app.update(&keys, ctx.time_us());
        if going {
            self.app.draw(ctx.screen());
        }
        while let Some(e) = self.app.take_event() {
            log(ctx, e);
        }
        if !going {
            return Flow::Done;
        }
        let want = Kestrel::wanted(&self.app);
        let now = ctx.video();
        if want != (now.rate, now.hint) && want != self.asked {
            self.asked = want;
            let mut v = now;
            v.rate = want.0;
            v.hint = want.1;
            if ctx.ask_video(v).is_err() {
                ctx.log("kestrel: video settings refused");
            }
        }
        Flow::Run
    }
}

fn log(ctx: &mut Ctx, e: Event) {
    match e {
        Event::Start {
            mission,
            detail,
            eye,
        } => ctx
            .log_line()
            .text("kestrel: mission ")
            .num(mission)
            .text(", detail ")
            .text(detail.name())
            .text(", view ")
            .text(eye.name())
            .end(),
        Event::Over {
            mission,
            state,
            score,
            ticks,
        } => ctx
            .log_line()
            .text("kestrel: mission ")
            .num(mission)
            .text(" ")
            .text(state.name())
            .text(", score ")
            .num(score)
            .text(", ")
            .num(ticks / 60)
            .text(" s")
            .end(),
        Event::Settings { detail, eye, fast } => ctx
            .log_line()
            .text("kestrel: detail ")
            .text(detail.name())
            .text(", view ")
            .text(eye.name())
            .text(", ")
            .text(if fast { "60" } else { "30" })
            .text(" a second")
            .end(),
        Event::Cost { detail, stats: s } => ctx
            .log_line()
            .text("kestrel: cost ")
            .text(detail.name())
            .text(" objects ")
            .num(s.objects)
            .text(" verts ")
            .num(s.verts)
            .text(" faces ")
            .num(s.faces)
            .text(" front ")
            .num(s.front)
            .text(" clipped ")
            .num(s.clipped)
            .text(" tris ")
            .num(s.tris)
            .text(" dropped ")
            .num(s.dropped)
            .text(" px ")
            .num(s.pixels)
            .end(),
        Event::End { score } => ctx
            .log_line()
            .text("kestrel: the end, score ")
            .num(score)
            .end(),
        Event::Quit => ctx.log("kestrel: leave"),
    }
}

game_kit::game!(Kestrel);
