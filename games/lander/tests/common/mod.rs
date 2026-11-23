//! Shared by the tests: a stand-in console loop and an autopilot.
//!
//! The loop calls the game the way the console does at 30 (or 60)
//! pictures a second: game time grows by one period per call. The
//! tests count their calls at 30 a second, so [`Run::new`] sets RATE 30
//! itself; the game starts at 60, and a test that wants 60 sets it. After
//! each call it compares the new picture with the last one using the
//! console's own rule (`console_video::diff`), so every test can see
//! what each frame would cost the screen.

#![allow(dead_code)]

use console_video::{Areas, Frame, diff};
use game_kit::button;
use lander::app::{App, Event, Input, Scene};
use lander::craft::{FULL, THRUST, TURN_RATE};
use lander::fixed::{ONE, PX, cos_step};

pub const PERIOD_30: u64 = 33_333;
pub const PERIOD_60: u64 = 16_667;
/// The board's trigger range (scripts use it): 0..=1023.
pub const BOARD_TRIGGER: u32 = 1023;

/// What one call cost the screen.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cost {
    /// Pixels that differ from the last picture.
    pub changed: u32,
    /// Pixels the console would send (its areas cover more than the
    /// changed pixels), and how many areas.
    pub sent: u32,
    pub areas: usize,
}

pub struct Run {
    pub app: App,
    pub frame: Box<Frame>,
    pub shown: Box<Frame>,
    pub t: u64,
    pub calls: u64,
    pub events: Vec<Event>,
    pub last: Cost,
    pub done: bool,
}

impl Run {
    /// The game at its starting options, but RATE 30 (see above).
    pub fn new() -> Run {
        let mut app = App::new();
        app.opts.fast = false;
        Run {
            app,
            frame: Box::new(Frame::new()),
            shown: Box::new(Frame::new()),
            t: 0,
            calls: 0,
            events: Vec::new(),
            last: Cost::default(),
            done: false,
        }
    }

    /// One call of the game, then the clock moves one period.
    pub fn call(&mut self, inp: &Input) -> Cost {
        assert!(!self.done, "the game ended");
        let going = self.app.update(inp, self.t);
        while let Some(e) = self.app.take_event() {
            self.events.push(e);
        }
        if !going {
            self.done = true;
            return Cost::default();
        }
        self.app.draw(&mut self.frame);
        let mut a = Areas::new();
        diff(&self.shown, &self.frame, &mut a);
        let changed = (self.shown.0.iter().zip(self.frame.0.iter()))
            .filter(|(a, b)| a != b)
            .count() as u32;
        self.shown.0.copy_from_slice(&self.frame.0);
        self.calls += 1;
        self.t += if self.app.opts.fast {
            PERIOD_60
        } else {
            PERIOD_30
        };
        self.last = Cost {
            changed,
            sent: a.pixels(),
            areas: a.len(),
        };
        self.last
    }

    pub fn idle(&mut self, n: u32) {
        for _ in 0..n {
            self.call(&Input::default());
        }
    }

    /// A button pressed for one call, then let go for one.
    pub fn tap(&mut self, b: u32) {
        self.call(&press(b));
        self.call(&Input::default());
    }

    /// From the title: PLAY with the options as they are, then wait out
    /// the "LEVEL" card.
    pub fn play(&mut self) {
        assert_eq!(self.app.scene, Scene::Title);
        self.app.menu = 0;
        self.tap(button::A);
        assert_eq!(self.app.scene, Scene::Ready);
        while self.app.scene == Scene::Ready {
            self.call(&Input::default());
        }
    }

    /// Flies with the autopilot until the flight ends (at most `max`
    /// calls). Returns the calls used.
    pub fn autopilot(&mut self, max: u32) -> u32 {
        for n in 0..max {
            if self.app.scene != Scene::Fly {
                return n;
            }
            let p = pilot(&self.app);
            self.call(&p.input());
        }
        max
    }

    pub fn landed_count(&self) -> usize {
        self.events
            .iter()
            .filter(|e| matches!(e, Event::Landed { .. }))
            .count()
    }

    /// A short number made from the picture, to compare runs.
    pub fn print(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in self.frame.0.iter() {
            h = (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
        }
        h
    }
}

pub fn press(b: u32) -> Input {
    Input {
        held: b,
        pressed: b,
        ..Input::default()
    }
}

pub fn hold(b: u32) -> Input {
    Input {
        held: b,
        ..Input::default()
    }
}

/// What the autopilot does in one call, in the board's own numbers (so
/// a simulator script can say exactly the same).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pilot {
    /// Left stick x.
    pub stick_x: i16,
    /// Right trigger, 0..=1023.
    pub trigger: u16,
    /// B held: lean back to upright.
    pub level: bool,
}

impl Pilot {
    /// As the game sees it (the console scales triggers to 0..=32767).
    pub fn input(&self) -> Input {
        Input {
            held: if self.level { button::B } else { 0 },
            left: (self.stick_x, 0),
            triggers: (0, (u32::from(self.trigger) * 32767 / BOARD_TRIGGER) as u16),
            ..Input::default()
        }
    }
}

/// A simple autopilot: heads for the nearest pad above the ground on the
/// way, then sinks onto it slowly and upright. It reads the game's state,
/// as a player reads the screen. Integer maths only.
pub fn pilot(app: &App) -> Pilot {
    let w = &app.world;
    let c = &app.craft;
    let (cx, cy) = (i64::from(c.x), i64::from(c.y));
    let px = i64::from(PX);
    let pad = w
        .pads()
        .iter()
        .min_by_key(|p| ((i64::from(p.x) * 2 + i64::from(p.w)) * px / 2 - cx).abs())
        .expect("a pad");
    let mid = (i64::from(p_x(pad)) * 2 + i64::from(pad.w)) * px / 2;
    let dx = mid - cx;
    let feet = cy / px + 4;
    let over = (dx / px).abs() <= i64::from(pad.w) / 2 - 6;

    // Wanted sideways speed: 0.6 px/s for each pixel to go (at least 2),
    // at most 14.
    let per_s = |v: i64| v * px / 60; // pixels a second -> Q16 a tick
    let mut vx_want = (dx * 3 / 5 / 60).clamp(-per_s(14), per_s(14));
    if dx.abs() > px && vx_want.abs() < per_s(2) {
        vx_want = per_s(2) * dx.signum();
    }

    // The highest ground between here and the pad.
    let (a, b) = ((cx.min(mid) / px) as i32 - 8, (cx.max(mid) / px) as i32 + 8);
    let top = (a..=b).map(|x| w.ground_at(x)).min().unwrap_or(100);
    let h = i64::from(pad.y) - 1 - feet;
    let slow_side = (i64::from(c.vx)).abs() <= per_s(2);
    let vy_want = if over && slow_side {
        per_s((h / 2 + 2).clamp(2, 12))
    } else if over && h < 12 {
        // Low over the pad but still drifting: hold the height.
        0
    } else if h < 10 {
        // Low and off the pad: climb a little first.
        -per_s(4)
    } else {
        // Keep the feet about 18 pixels over the highest ground.
        let err = feet - (i64::from(top) - 18);
        per_s((-err / 2).clamp(-10, 6))
    };

    // Lean against the wind: about -wind/gravity radians (41 steps a
    // radian), as a player leans into the arrows.
    let g = i64::from(w.params.gravity);
    let wind = i64::from(w.params.wind_at(app.flight_t));
    let ff = (-wind * 41 / g.max(1)).clamp(-8, 8);

    // Lean to change the sideways speed: 3 steps per px/s, at most 20
    // (4 near the pad) on top of the wind lean.
    let dv = vx_want - i64::from(c.vx);
    let limit = if over && h < 14 { 4 } else { 20 };
    let mut steps = ff + (dv * 3 / per_s(1).max(1)).clamp(-limit, limit);
    // Just over the pad: B (upright) when there is no wind, else the
    // wind lean, kept inside the safe lean.
    let level = over && h < 5 && ff == 0;
    if over && h < 5 {
        steps = (ff + (dv * 2 / per_s(1).max(1)).clamp(-2, 2)).clamp(-7, 7);
    }
    let err = steps * 256 - i64::from(c.angle);
    let f = if err.abs() < 64 {
        0
    } else {
        (err * i64::from(FULL) / i64::from(TURN_RATE)).clamp(-i64::from(FULL), i64::from(FULL))
    };
    let stick_x = if f > 0 {
        4000 + f * 28767 / i64::from(FULL)
    } else if f < 0 {
        -4000 + f * 28767 / i64::from(FULL)
    } else {
        0
    };

    // Thrust: hold up against gravity, plus what the vertical speed
    // error asks for.
    let acc = g + (i64::from(c.vy) - vy_want) / 12;
    let cos = i64::from(cos_step(c.step())).max(ONE as i64 / 4);
    let thrust = (acc * i64::from(FULL) * i64::from(ONE) / (i64::from(THRUST) * cos))
        .clamp(0, i64::from(FULL));
    let trigger =
        ((thrust * i64::from(BOARD_TRIGGER) + i64::from(FULL) / 2) / i64::from(FULL)) as u16;
    Pilot {
        stick_x: stick_x.clamp(-32767, 32767) as i16,
        trigger,
        level,
    }
}

fn p_x(p: &lander::level::Pad) -> u16 {
    p.x
}
