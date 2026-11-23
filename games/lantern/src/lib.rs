//! LANTERN: a first-person walk through an old mine by lamplight.
//! Collect crystals, keep the lamp fed with oil, find the crank and work
//! the lift; moths drink the oil, crawlers bite, diggers roll on their
//! tracks, and the lamp's flash stops them all for a while.
//!
//! How it is built (README.md has the whole story):
//!
//! * [`render`]: the 3D view by ray casting, in whole numbers, with its
//!   own work counted. Three details: LIGHT, NORMAL, HEAVY.
//! * [`world`]: one level in play, moved in fixed ticks of 1/60 s.
//!   [`level`]: the maps as tables of letters. [`art`]: the palette, the
//!   wall textures and the sprites, all made while compiling.
//!   [`fixed`]: whole-number maths (sine table, angles).
//! * [`app`]: the screens (title, play, map, pause, cards) and the
//!   controls. [`hud`]: what is drawn around the view.
//! * This file: the console side. It turns the [`Ctx`] into
//!   [`app::Keys`], sets the palette, asks for the frame rate and the
//!   video hint, and logs what happens.
//!
//! Uses only the game kit. No heap, no floating point, no `unsafe`.

#![no_std]

pub mod app;
pub mod art;
pub mod fixed;
pub mod hud;
pub mod level;
pub mod render;
pub mod world;

use app::{App, Event, Keys, Scene};
use game_kit::{Ctx, Flow, Game, hint, rate};

pub struct Lantern {
    pub app: App,
    /// The video settings last asked for (rate, hint), so a refused
    /// request is not asked again every frame.
    asked: (u16, u16),
    /// Level 1 is loaded in place on the first update, not in `start`,
    /// so the start needs no big value on the stack.
    loaded: bool,
}

impl Lantern {
    /// The rate and hint the game wants now.
    pub fn wanted(app: &App) -> (u16, u16) {
        let r = if app.still() {
            rate::ON_DEMAND
        } else if app.opts.fast {
            rate::FPS60
        } else {
            rate::FPS30
        };
        let h = if app.scene == Scene::Play && app.pause.is_none() {
            hint::SCROLLS
        } else {
            hint::MOSTLY_STILL
        };
        (r, h)
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

/// The game before level 1 is loaded, as a constant: `start` copies it
/// straight into the console's slot.
const START: Lantern = Lantern {
    app: App::blank(),
    asked: (u16::MAX, u16::MAX),
    loaded: false,
};

impl Game for Lantern {
    // Not inlined, so the copy of START stays out of the console's
    // update frame. The game kit's `*s = Some(G::start(..))` still keeps
    // one 6 KB temporary there; only a game-kit change removes that.
    #[inline(never)]
    fn start(ctx: &mut Ctx) -> Self {
        ctx.log("lantern: started");
        START
    }

    fn update(&mut self, ctx: &mut Ctx) -> Flow {
        if !self.loaded {
            self.app.load_title_level();
            self.loaded = true;
        }
        if ctx.started() || ctx.must_redraw() {
            ctx.set_colours(0, &art::COLOURS);
        }
        let keys = keys_of(ctx);
        let going = self.app.update(&keys, ctx.time_us());
        if !going {
            while let Some(e) = self.app.take_event() {
                log(ctx, e);
            }
            return Flow::Done;
        }
        self.app.draw(ctx.screen());
        while let Some(e) = self.app.take_event() {
            log(ctx, e);
        }

        let want = Lantern::wanted(&self.app);
        let now = ctx.video();
        if want != (now.rate, now.hint) && want != self.asked {
            self.asked = want;
            let mut v = now;
            v.rate = want.0;
            v.hint = want.1;
            if ctx.ask_video(v).is_err() {
                ctx.log("lantern: video settings refused");
            }
        }
        Flow::Run
    }
}

fn log(ctx: &mut Ctx, e: Event) {
    match e {
        Event::Start { level, detail } => ctx
            .log_line()
            .text("lantern: level ")
            .num(level)
            .text(", detail ")
            .text(detail.name())
            .end(),
        Event::Clear {
            level,
            crystals,
            of,
            seconds,
        } => ctx
            .log_line()
            .text("lantern: level ")
            .num(level)
            .text(" clear, crystals ")
            .num(crystals)
            .text("/")
            .num(of)
            .text(", ")
            .num(seconds)
            .text(" s")
            .end(),
        Event::Fainted { level } => ctx
            .log_line()
            .text("lantern: fainted on level ")
            .num(level)
            .end(),
        Event::Detail(d) => ctx.log_line().text("lantern: detail ").text(d.name()).end(),
        Event::Cost { detail, cost: c } => ctx
            .log_line()
            .text("lantern: cost ")
            .text(detail.name())
            .text(" rays ")
            .num(c.rays)
            .text(" steps ")
            .num(c.steps)
            .text(" mul ")
            .num(c.muls)
            .text(" div ")
            .num(c.divs)
            .text(" px wall ")
            .num(c.wall_px)
            .text(" flat ")
            .num(c.flat_px)
            .text(" floor ")
            .num(c.floor_px)
            .text(" sprite ")
            .num(c.sprite_px)
            .end(),
        Event::End { crystals, retries } => ctx
            .log_line()
            .text("lantern: the end, crystals ")
            .num(crystals)
            .text(", falls ")
            .num(retries)
            .end(),
        Event::Quit => ctx.log("lantern: leave"),
    }
}

game_kit::game!(Lantern);
