//! MERCATOR: a small trader's ship in a galaxy of 136 star systems.
//! Buy cheap where a good is made, fly, jump, sell dear where it is
//! wanted; fight raiders or run from them; fit a better laser, shields
//! and a sun skimmer, or trade the ship in for a bigger hold or a
//! tougher, faster fighter. Survey systems for the Mapmakers'
//! Guild: 20 surveys and 50,000 credits win its charter. A second pad
//! is the gunner. Real 3D in flat-shaded triangles.
//!
//! How it is built (README.md has more):
//!
//! * [`fixed`], [`poly`]: whole-number 3D, taken from KESTREL and trimmed
//!   (lines are clipped, not clamped).
//! * [`galaxy`]: every system worked out from the seed and its number;
//!   nothing is stored. [`market`]: goods and prices.
//! * [`world`]: one system in flight, in fixed ticks of 1/60 s: the
//!   player's ship, up to 8 other ships, the station, the career.
//! * [`models`], [`art`]: model tables and the palette. [`scene`]: what
//!   is drawn in which order. [`hud`], [`chart`]: everything round the
//!   view. [`app`]: the screens and the controls. [`save`]: the career
//!   as 52 bytes.
//! * This file: the console side. It turns the [`Ctx`] into
//!   [`app::Keys`], sets the palette, asks for the picture rate and the
//!   video hint, and logs what happens.
//!
//! Uses only the game kit. No heap, no floating point, no `unsafe`.

#![no_std]
// `max(a).min(b)` with constant bounds is used on purpose in the pixel
// helpers: it reads as "pull into range" and never panics.
#![allow(clippy::manual_clamp)]

pub mod app;
pub mod art;
pub mod chart;
pub mod fixed;
pub mod fx;
pub mod galaxy;
pub mod hud;
pub mod market;
pub mod models;
pub mod planet;
pub mod poly;
pub mod save;
pub mod scene;
pub mod world;

use app::{App, Event, Keys};
use game_kit::{Ctx, Flow, Game, hint, rate};
use hud::Txt;

pub struct Mercator {
    pub app: App,
    /// The video settings last asked for (rate, hint).
    asked: (u16, u16),
    /// The title galaxy is made in place on the first update.
    loaded: bool,
}

impl Mercator {
    /// The rate and hint the game wants now.
    pub fn wanted(app: &App) -> (u16, u16) {
        let r = if app.still() {
            rate::ON_DEMAND
        } else if app.opts.fast && !app.docked_view() && app.scene != app::Scene::Below {
            // Step G1: below, 30 a second always (SPEC BELOW_RATE).
            // Step M3: the live docked scene stays at 30 a second.
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
        gunner: ctx.player(1).held_bits() & game_kit::button::A != 0,
    }
}

const START: Mercator = Mercator {
    app: App::blank(),
    asked: (u16::MAX, u16::MAX),
    loaded: false,
};

impl Game for Mercator {
    #[inline(never)]
    fn start(ctx: &mut Ctx) -> Self {
        say(ctx, b"mercator: started");
        START
    }

    fn update(&mut self, ctx: &mut Ctx) -> Flow {
        if !self.loaded {
            self.app.load();
            self.loaded = true;
        }
        let keys = keys_of(ctx);
        let going = self.app.update(&keys, ctx.time_us());
        if ctx.started() || ctx.must_redraw() {
            let c = self.app.colours();
            ctx.set_colours(0, c);
        } else if let Some(c) = self.app.new_colours() {
            ctx.set_colours(0, c);
        }
        if going {
            self.app.draw(ctx.screen());
        }
        while let Some(e) = self.app.take_event() {
            log(ctx, e);
        }
        if !going {
            return Flow::Done;
        }
        let want = Mercator::wanted(&self.app);
        let now = ctx.video();
        if want != (now.rate, now.hint) && want != self.asked {
            self.asked = want;
            let mut v = now;
            v.rate = want.0;
            v.hint = want.1;
            if ctx.ask_video(v).is_err() {
                say(ctx, b"mercator: video settings refused");
            }
        }
        Flow::Run
    }
}

/// One log line, built without `core::fmt` (it costs code bytes).
fn log(ctx: &mut Ctx, e: Event) {
    let mut t = Txt::new();
    t.s("mercator: ");
    match e {
        Event::NewGame { galaxy } => {
            t.s("new game, galaxy ").num(u32::from(galaxy));
        }
        Event::Docked {
            system,
            credits,
            day,
        } => {
            t.s("docked at ")
                .num(u32::from(system))
                .s(", ")
                .num(credits)
                .s(" cr, day ")
                .num(u32::from(day));
        }
        Event::Jumped { to, fuel } => {
            t.s("jumped to ")
                .num(u32::from(to))
                .s(", fuel ")
                .tenths(u32::from(fuel));
        }
        Event::Killed { kind } => {
            t.s(match kind {
                world::Kind::Raider => "raider",
                world::Kind::Hauler => "hauler",
                world::Kind::Warden => "warden",
                world::Kind::Canister => "canister",
            })
            .s(" down");
        }
        Event::Bounty { sum } => {
            t.s("bounty paid, ").num(sum).s(" cr");
        }
        Event::Lost { credits, kills } => {
            t.s("ship lost, ")
                .num(credits)
                .s(" cr, kills ")
                .num(u32::from(kills));
        }
        Event::Won { day } => {
            t.s("charter granted, day ").num(u32::from(day));
        }
        Event::Ship { from, to, credits } => {
            let name = |i: u8| world::CRAFTS.get(usize::from(i)).map_or("?", |c| c.name);
            t.s("ship ")
                .s(name(from))
                .s(" traded for ")
                .s(name(to))
                .s(", ")
                .num(credits)
                .s(" cr");
        }
        Event::Settings => {
            t.s("settings changed");
        }
        Event::Cost { stats: s } => {
            t.s("cost tris ")
                .num(s.tris)
                .s(" px ")
                .num(s.pixels)
                .s(" faces ")
                .num(s.faces)
                .s(" dropped ")
                .num(s.dropped);
        }
        Event::Quit => {
            t.s("leave");
        }
    }
    say(ctx, t.bytes());
}

/// One log line, letter by letter through the kit's `log_line` (no
/// `core::fmt`, no UTF-8 check). A byte that is not a printable letter
/// is written as `?`, as before.
#[inline(never)]
fn say(ctx: &mut Ctx, line: &[u8]) {
    let mut l = ctx.log_line();
    for &b in line {
        l = l.text(crate::hud::letter(b));
    }
    l.end();
}

game_kit::game!(Mercator);
