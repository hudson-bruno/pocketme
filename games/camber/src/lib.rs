//! [`NAME`]: 3D hover-ship racing. Eight ships, three laps, three tracks
//! with banked turns, hills, tunnels and a jump; boost pads, a shield
//! that walls wear down and a lane that fills it again.
//!
//! How it is built (README.md has the rules and the controls):
//!
//! * [`fixed`]: whole-number maths (angles, sine, matrices, roots).
//! * [`track`]: the tracks as pieces, baked at compile time into
//!   segments of 8 m with their racing line and safe speed.
//! * [`race`]: one race in fixed ticks of 1/60 s: handling, walls,
//!   jumps, the shield, the rivals, laps and places.
//! * [`poly`]: the polygon renderer (clipping, projection, triangles,
//!   models sorted far to near).
//! * [`scene`]: the backdrop, the track, the scenery and the ships.
//! * [`art`]: colours, themes and the ship's model.
//! * [`app`] and [`hud`]: the screens, the menus and the controls.
//! * [`tv`]: the title's backdrop, a made-up race and its cameras.
//! * This file: the console side. It turns the [`Ctx`] into
//!   [`app::Keys`], sets the palette, asks for the frame rate (none while
//!   nothing moves) and logs what happens.
//!
//! Uses only the game kit and the shared 3D library (`crates/game-3d`).
//! No heap, no floating point, no `unsafe`.

#![no_std]

pub mod app;
pub mod art;
pub mod fixed;
pub mod hud;
pub mod poly;
pub mod race;
pub mod scene;
pub mod track;
pub mod tv;

use app::{App, Keys, Log};
use game_kit::{Ctx, Flow, Game, hint, rate};

/// The game's name on the title. README.md lists every other place
/// that spells it.
pub const NAME: &str = "CAMBER";
/// What each log line starts with.
const LOG: &str = "camber: ";

pub struct Racer {
    pub app: App,
    /// The rate last asked for, so a refused request is not asked again
    /// every frame.
    asked: u16,
    /// The theme the palette holds (`u8::MAX`: none yet).
    theme: u8,
    /// Microseconds of game time so far (for the ships' bob and the
    /// blinking lights).
    time: u32,
}

impl Racer {
    /// The frame rate the game wants now: none while nothing moves (a
    /// paused race), 30 on the title, the player's rate in a race.
    pub fn wanted(app: &App) -> u16 {
        if app.still() {
            rate::ON_DEMAND
        } else if app.opts.fast && app.scene == app::Scene::Race {
            rate::FPS60
        } else {
            rate::FPS30
        }
    }
}

/// The console's input as the game's.
pub fn keys_of(ctx: &Ctx) -> Keys {
    let p = ctx.player(0);
    Keys {
        held: p.held_bits(),
        pressed: p.pressed_bits(),
        stick: p.left_stick(),
        trig: p.triggers(),
        touch: ctx.touch(),
        delta_us: ctx.delta_us(),
    }
}

const START: Racer = Racer {
    app: App::new(),
    asked: u16::MAX,
    theme: u8::MAX,
    time: 0,
};

impl Game for Racer {
    #[inline(never)]
    fn start(ctx: &mut Ctx) -> Self {
        say(ctx, "started");
        START
    }

    fn update(&mut self, ctx: &mut Ctx) -> Flow {
        let keys = keys_of(ctx);
        let going = self.app.update(&keys);
        while let Some(l) = self.app.take_log() {
            log(ctx, &self.app, l);
        }
        if !going {
            return Flow::Done;
        }
        let theme = self.app.theme();
        if ctx.started() || ctx.must_redraw() || theme != self.theme {
            self.theme = theme;
            set_palette(ctx, theme);
        }
        if !self.app.still() {
            self.time = self.time.wrapping_add(self.app.ticks);
        }
        self.app.draw(ctx.screen(), self.time);

        let want = Racer::wanted(&self.app);
        let now = ctx.video();
        if (want != now.rate || now.hint != hint::MOSTLY_STILL) && want != self.asked {
            self.asked = want;
            let mut v = now;
            v.rate = want;
            v.hint = hint::MOSTLY_STILL;
            if ctx.ask_video(v).is_err() {
                say(ctx, "video settings refused");
            }
        }
        Flow::Run
    }
}

/// The palette for a track's theme (kept off the update's own frame).
#[inline(never)]
fn set_palette(ctx: &mut Ctx, theme: u8) {
    let mut pal = [0u16; art::COLOURS];
    art::palette(
        &art::THEMES[usize::from(theme) % art::THEMES.len()],
        &mut pal,
    );
    ctx.set_colours(0, &pal);
}

/// A log line of plain words.
fn say(ctx: &mut Ctx, what: &str) {
    let mut s = hud::Line::new();
    s.push(LOG).push(what);
    ctx.log(s.as_str());
}

fn log(ctx: &mut Ctx, app: &App, l: Log) {
    let mut s = hud::Line::new();
    s.push(LOG);
    match l {
        Log::Start => {
            let o = &app.opts;
            let tr = app.race.tr();
            s.push(o.mode.name())
                .push(" ")
                .push(tr.name)
                .push(" ")
                .push(o.class.name());
        }
        Log::Lap { lap, time } => {
            s.push("lap ")
                .num(lap.max(0) as u32, 1)
                .push(" ")
                .time(time);
        }
        Log::Finish { place, time } => {
            s.push("finish ")
                .num(u32::from(place), 1)
                .push(" ")
                .time(time);
        }
        Log::Fell => {
            s.push("fell");
        }
        Log::Pause => {
            s.push("pause");
        }
        Log::Resume => {
            s.push("resume");
        }
        Log::Leave => {
            s.push("leave");
        }
    }
    ctx.log(s.as_str());
}

#[cfg(not(feature = "rec"))]
game_kit::game!(Racer);

/// The measuring recorder (feature `rec`, off in the normal build).
#[cfg(feature = "rec")]
pub mod rec;

#[cfg(feature = "rec")]
game_kit::game!(rec::RecRacer);
