//! HAIRPIN: a kart race on mountain roads at dusk, drawn the way the
//! old arcade racers drew their roads. Three courses, four rivals,
//! three laps each; finish in the top three to go on to the next course.
//!
//! How it is built (README.md has the whole story):
//!
//! * [`track`]: the courses as lists of sections (bends, hills, what
//!   stands beside the road); segments are worked out when needed.
//! * [`race`]: one race, moved in fixed ticks of 1/60 s: the karts'
//!   speed and grip, the rivals' driving, laps, places, the boost.
//! * [`render`]: the 3D view, in whole numbers, with its own work
//!   counted. Three details: LIGHT, NORMAL, HEAVY; two views: CHASE,
//!   NOSE.
//! * [`art`]: the palette and the sprites, all made while compiling.
//! * [`app`]: the screens (title, race, pause, cards) and the controls.
//!   [`hud`]: what is drawn around the view.
//! * This file: the console side. It turns the [`Ctx`] into
//!   [`app::Keys`], sets the palette, asks for the frame rate and the
//!   video hint, and logs what happens.
//!
//! Uses only the game kit. No heap, no floating point, no `unsafe`.

#![no_std]

pub mod app;
pub mod art;
pub mod hud;
pub mod race;
pub mod render;
pub mod track;

use app::{App, Event, Keys, Scene};
use game_kit::{Ctx, Flow, Game, hint, rate};

pub struct Hairpin {
    pub app: App,
    /// The video settings last asked for (rate, hint), so a refused
    /// request is not asked again every frame.
    asked: (u16, u16),
    /// The title's course is set up in place on the first update, not in
    /// `start`, so the start needs no big value on the stack.
    loaded: bool,
}

impl Hairpin {
    /// The rate and hint the game wants now.
    pub fn wanted(app: &App) -> (u16, u16) {
        let r = if app.still() {
            rate::ON_DEMAND
        } else if app.opts.fast {
            rate::FPS60
        } else {
            rate::FPS30
        };
        let h = if app.scene == Scene::Race && app.pause.is_none() {
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

/// The game before the title's course is set up, as a constant: `start`
/// copies it straight into the console's slot.
const START: Hairpin = Hairpin {
    app: App::blank(),
    asked: (u16::MAX, u16::MAX),
    loaded: false,
};

impl Game for Hairpin {
    // Not inlined, so the copy of START stays out of the console's
    // update frame. The game kit's `*s = Some(G::start(..))` still keeps
    // one temporary of the game's size there, so the game is kept small.
    #[inline(never)]
    fn start(ctx: &mut Ctx) -> Self {
        ctx.log("hairpin: started");
        START
    }

    fn update(&mut self, ctx: &mut Ctx) -> Flow {
        if !self.loaded {
            self.app.load_title();
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

        let want = Hairpin::wanted(&self.app);
        let now = ctx.video();
        if want != (now.rate, now.hint) && want != self.asked {
            self.asked = want;
            let mut v = now;
            v.rate = want.0;
            v.hint = want.1;
            if ctx.ask_video(v).is_err() {
                ctx.log("hairpin: video settings refused");
            }
        }
        Flow::Run
    }
}

fn log(ctx: &mut Ctx, e: Event) {
    match e {
        Event::Start {
            course,
            detail,
            cam,
            fast,
        } => ctx
            .log_line()
            .text("hairpin: course ")
            .num(course)
            .text(", detail ")
            .text(detail.name())
            .text(", view ")
            .text(cam.name())
            .text(", rate ")
            .text(rate(fast))
            .end(),
        Event::Lap { lap, ticks } => ctx
            .log_line()
            .text("hairpin: lap ")
            .num(lap)
            .text(" in ")
            .num(ticks)
            .text(" ticks")
            .end(),
        Event::Finish {
            course,
            place,
            ticks,
        } => ctx
            .log_line()
            .text("hairpin: course ")
            .num(course + 1)
            .text(" done, place ")
            .num(place)
            .text(", ")
            .num(ticks)
            .text(" ticks")
            .end(),
        Event::Crash => ctx.log("hairpin: crash"),
        Event::Detail(d) => ctx.log_line().text("hairpin: detail ").text(d.name()).end(),
        Event::View(c) => ctx.log_line().text("hairpin: view ").text(c.name()).end(),
        Event::Rate(fast) => ctx.log_line().text("hairpin: rate ").text(rate(fast)).end(),
        Event::Cost { detail, cost: c } => ctx
            .log_line()
            .text("hairpin: cost ")
            .text(detail.name())
            .text(" segs ")
            .num(c.segs)
            .text(" rows ")
            .num(c.rows)
            .text(" sprites ")
            .num(c.sprites)
            .text(" sprite px ")
            .num(c.sprite_px)
            .end(),
        Event::Cup { places } => ctx
            .log_line()
            .text("hairpin: cup done, places ")
            .num(places[0])
            .text(" ")
            .num(places[1])
            .text(" ")
            .num(places[2])
            .end(),
        Event::Quit => ctx.log("hairpin: leave"),
    }
}

/// The frame rate as it is logged.
fn rate(fast: bool) -> &'static str {
    if fast { "60" } else { "30" }
}

game_kit::game!(Hairpin);
