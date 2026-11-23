//! One match on the table: the puck, the mallets, the rails, the goals,
//! the score and the clock, moved in fixed ticks of 1/60 s. Whole
//! numbers only (positions in 1/256 pixel, see [`crate::fixed`]).
//!
//! The table fills the picture below a 12-pixel bar. Side 0 (CORAL)
//! defends the left goal, side 1 (TEAL) the right one. Mallet `i` plays
//! for side `i % 2`; mallets 2 and 3 are the second pair, used only in
//! 2 AGAINST 2.
//!
//! **How the puck can never pass through anything.** Every tick is cut
//! into sub-steps so small that, in each one, the puck moves at most
//! [`STEP`] (1.5 pixels) relative to any mallet, which is far less than
//! the puck's radius. After each sub-step every overlap is undone at
//! once: the puck is pushed out of mallets and bumpers along the line
//! between the centres (and bounces off them), then the rails and posts
//! push it back onto the table; a mallet that still overlaps it (the
//! puck squeezed against a rail) gives way itself. So the puck's centre
//! can never get to the far side of a mallet, a rail or a post, at any
//! speed and at any frame rate. The tests check this after every
//! sub-step (debug assertions).
//!
//! This file knows nothing of buttons: each tick takes one [`Intent`]
//! per mallet (from a pad, the touch screen or [`crate::cpu`]).

use crate::fixed::{PX, Rng, UNIT, clamp_len, dir64, len, len2, mul_div, norm};

// ---- The table (pixels and position units) -------------------------------

/// The bar on top (pixels).
pub const BAR_H: i32 = 12;
/// The rails' inner edges: the playing surface is inside them.
pub const XL: i32 = 2 * PX;
pub const XR: i32 = 158 * PX;
pub const YT: i32 = 14 * PX;
pub const YB: i32 = 118 * PX;
/// The middle of the table (the centre line is at `CX`).
pub const CX: i32 = (XL + XR) / 2;
pub const CY: i32 = (YT + YB) / 2;
/// Half the goal slot in each end rail.
pub const GOAL_HALF: i32 = 16 * PX;
pub const PUCK_R: i32 = 3 * PX + PX / 2;
pub const MALLET_R: i32 = 6 * PX + PX / 2;
/// Puck and mallet touch at this distance between their centres.
pub const HIT_R: i32 = PUCK_R + MALLET_R;
/// A mallet's centre may come this close to the centre line.
pub const LINE_GAP: i32 = 2 * PX;
/// The WILD table's two bumpers, on the centre line.
pub const BUMPER_R: i32 = 4 * PX;
pub const BUMPERS: [(i32, i32); 2] = [(CX, CY - 30 * PX), (CX, CY + 30 * PX)];
/// In 2 AGAINST 2, the back mallet stays within `BACK_X` of its goal
/// line, the front one at least `FRONT_X` from it.
pub const BACK_X: i32 = 46 * PX;
pub const FRONT_X: i32 = 32 * PX;

// ---- Movement (per tick of 1/60 s) ------------------------------------------

/// The puck's top speed (6.5 pixels a tick: 390 pixels a second).
pub const PUCK_MAX: i32 = 6 * PX + PX / 2;
/// The puck keeps this much speed a tick (of 256) on the air, and loses
/// [`DRAG`] more, so a puck nobody hits always comes to rest.
pub const KEEP: i32 = 255;
pub const DRAG: i32 = 1;
/// Bounciness (of 256): rails and posts, mallets, the WILD bumpers
/// (above 256: they kick).
pub const E_RAIL: i32 = 220;
pub const E_MALLET: i32 = 200;
pub const E_BUMPER: i32 = 300;
/// A pad mallet: top speed, how fast it gets there, and the dash.
pub const PAD_V: i32 = 3 * PX + PX / 2;
pub const PAD_ACC: i32 = 230;
pub const DASH_V: i32 = 6 * PX;
pub const DASH_TICKS: u8 = 8;
pub const DASH_COOL: u8 = 40;
/// A touch mallet follows the finger at most this fast (5 pixels a tick:
/// it cannot jump through the puck or across the table).
pub const TOUCH_V: i32 = 5 * PX;
/// The most the puck may move against any mallet in one sub-step.
pub const STEP: i32 = 3 * PX / 2;
/// Most sub-steps in a tick (the speeds above give at most 9).
pub const MAX_SUB: i32 = 12;
/// A squeezed puck is moved aside at most twice this in one sub-step.
pub const GIVE: i32 = 3 * PX;
/// The furthest a mallet slides round a squeezed puck in one sub-step
/// (without this cap, a mallet pinned in a corner of its zone could pop
/// 14 pixels round the puck in one go).
pub const SLIDE: i32 = 4 * PX;

// ---- Timing (ticks) ------------------------------------------------------------

/// The puck waits this long on its serve spot before it is free.
pub const SERVE_TICKS: u16 = 50;
/// GOAL! is shown this long.
pub const GOAL_TICKS: u16 = 80;
/// A puck left on one half this long (7 seconds) goes to the other side.
pub const STALL_TICKS: u16 = 420;
/// TOO SLOW is shown this long.
pub const STALL_SHOW: u16 = 50;
/// A timed match; and its golden goal when level, at most this long.
pub const TIMED_TICKS: u32 = 3 * 60 * 60;
pub const OT_TICKS: u32 = 60 * 60;
/// A hit at least this hard (relative speed along the hit) makes sparks.
pub const SPARK_V: i32 = 3 * PX;
pub const SPARK_TICKS: u8 = 9;

/// The serve spot on `side`'s half.
pub const fn serve_x(side: usize) -> i32 {
    if side == 0 {
        CX - 34 * PX
    } else {
        CX + 34 * PX
    }
}

/// Which way `side` attacks along x: +1 or -1.
pub const fn attack(side: usize) -> i32 {
    if side == 0 { 1 } else { -1 }
}

/// The goal line `side` defends.
pub const fn goal_line(side: usize) -> i32 {
    if side == 0 { XL } else { XR }
}

// ---- Settings -------------------------------------------------------------------

/// How strong the computer is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Easy,
    Normal,
    Hard,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Easy, Level::Normal, Level::Hard];

    pub fn name(self) -> &'static str {
        match self {
            Level::Easy => "EASY",
            Level::Normal => "NORMAL",
            Level::Hard => "HARD",
        }
    }
}

/// How a match ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Length {
    First5,
    First7,
    First11,
    /// Three minutes; the most goals wins (a level end plays a golden
    /// goal of at most one minute, then it is a draw).
    Timed,
}

impl Length {
    pub const ALL: [Length; 4] = [
        Length::First5,
        Length::First7,
        Length::First11,
        Length::Timed,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Length::First5 => "FIRST TO 5",
            Length::First7 => "FIRST TO 7",
            Length::First11 => "FIRST TO 11",
            Length::Timed => "3 MINUTES",
        }
    }

    pub fn goals(self) -> Option<u8> {
        match self {
            Length::First5 => Some(5),
            Length::First7 => Some(7),
            Length::First11 => Some(11),
            Length::Timed => None,
        }
    }
}

/// Everything that sets up a match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rules {
    pub length: Length,
    pub level: Level,
    /// 2 AGAINST 2: mallets 2 and 3 play too.
    pub four: bool,
    /// The WILD table, with two bumpers on the centre line.
    pub wild: bool,
}

impl Rules {
    pub const DEFAULT: Rules = Rules {
        length: Length::First7,
        level: Level::Normal,
        four: false,
        wild: false,
    };
}

// ---- Parts ----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Puck {
    pub x: i32,
    pub y: i32,
    pub vx: i32,
    pub vy: i32,
}

impl Puck {
    pub fn speed(&self) -> i32 {
        len(self.vx, self.vy)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mallet {
    pub x: i32,
    pub y: i32,
    /// The speed a pad has built up (per tick).
    pub vx: i32,
    pub vy: i32,
    /// How far it really moved in the last tick: its speed in a hit.
    pub dx: i32,
    pub dy: i32,
    /// Dash ticks left, its direction, and the wait before the next.
    pub dash: u8,
    pub dash_dir: (i32, i32),
    pub cool: u8,
}

/// What a mallet's driver wants in one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Intent {
    /// Hold still (or finish a dash).
    #[default]
    Stay,
    /// A pad: a direction (`UNIT` long at most: full speed), and a dash
    /// asked for in this tick.
    Pad { dx: i32, dy: i32, dash: bool },
    /// Move towards a point at most `v` a tick (the touch screen, the
    /// computer); `dash` lunges that way.
    Go { x: i32, y: i32, v: i32, dash: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The puck waits on `side`'s serve spot; `t` counts down.
    Serve {
        side: u8,
        t: u16,
    },
    Live,
    /// `side` scored; `t` counts down.
    Goal {
        side: u8,
        t: u16,
    },
    /// The puck stayed too long on one half: it goes to `to`.
    Stall {
        to: u8,
        t: u16,
    },
    Over,
}

/// What happened, for the screens and the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ev {
    Goal { side: u8 },
    Stall { to: u8 },
    Hit { speed: i32 },
    Overtime,
    Over,
}

/// The computer's memory for one mallet (see [`crate::cpu`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mind {
    pub tx: i32,
    pub ty: i32,
    /// Ticks to the next look at the table.
    pub wait: u8,
    /// This rally's aiming error (position units).
    pub aim: i32,
}

#[derive(Clone)]
pub struct Match {
    pub rules: Rules,
    pub puck: Puck,
    pub m: [Mallet; 4],
    pub phase: Phase,
    pub score: [u8; 2],
    /// Ticks of play left on a timed match's clock (or its golden goal).
    pub clock: u32,
    pub overtime: bool,
    /// The half the puck is on, and how long it has been there.
    pub half: u8,
    pub half_ticks: u16,
    /// Ticks of the rally going on (from the end of the serve).
    pub rally: u32,
    /// Ticks since the match began.
    pub age: u32,
    /// The puck's last places (pixels), newest first, for its trail.
    pub trail: [(i16, i16); 3],
    /// Sparks of a hard hit: pixel and ticks left.
    pub spark: Option<(i16, i16, u8)>,
    pub mind: [Mind; 4],
    pub rng: Rng,
    events: [Option<Ev>; 8],
    /// Mallets that had to give way to a squeezed puck (tests count it).
    pub squeezes: u32,
}

/// The x range a mallet's centre may use.
pub fn zone_x(i: usize, four: bool) -> (i32, i32) {
    let side = i % 2;
    let (near, far) = if !four {
        (MALLET_R, CX - goal_line(0) - LINE_GAP)
    } else if i < 2 {
        (MALLET_R, BACK_X)
    } else {
        (FRONT_X, CX - goal_line(0) - LINE_GAP)
    };
    if side == 0 {
        (XL + near, XL + far)
    } else {
        (XR - far, XR - near)
    }
}

/// Where mallet `i` starts.
pub fn home(i: usize, four: bool) -> (i32, i32) {
    let side = i % 2;
    let a = attack(side);
    let g = goal_line(side);
    if !four || i < 2 {
        (g + a * 16 * PX, CY)
    } else {
        (CX - a * 20 * PX, CY - a * 24 * PX)
    }
}

impl Match {
    /// A match nobody plays (before the first one).
    pub const fn blank() -> Match {
        Match {
            rules: Rules::DEFAULT,
            puck: Puck {
                x: CX,
                y: CY,
                vx: 0,
                vy: 0,
            },
            m: [Mallet {
                x: CX,
                y: CY,
                vx: 0,
                vy: 0,
                dx: 0,
                dy: 0,
                dash: 0,
                dash_dir: (0, 0),
                cool: 0,
            }; 4],
            phase: Phase::Over,
            score: [0; 2],
            clock: 0,
            overtime: false,
            half: 0,
            half_ticks: 0,
            rally: 0,
            age: 0,
            trail: [(0, 0); 3],
            spark: None,
            mind: [Mind {
                tx: CX,
                ty: CY,
                wait: 0,
                aim: 0,
            }; 4],
            rng: Rng::new(1),
            events: [None; 8],
            squeezes: 0,
        }
    }

    /// A new match; `first` serves first.
    pub fn new(rules: Rules, first: u8, seed: u32) -> Match {
        let mut m = Match::blank();
        m.rules = rules;
        m.rng = Rng::new(seed);
        m.clock = if rules.length == Length::Timed {
            TIMED_TICKS
        } else {
            0
        };
        for i in 0..4 {
            let (x, y) = home(i, rules.four);
            m.m[i].x = x;
            m.m[i].y = y;
            m.mind[i].tx = x;
            m.mind[i].ty = y;
        }
        m.serve(first.min(1));
        m
    }

    /// How many mallets play (2 or 4).
    pub fn mallets(&self) -> usize {
        if self.rules.four { 4 } else { 2 }
    }

    pub fn take_event(&mut self) -> Option<Ev> {
        let e = self.events[0].take();
        self.events.rotate_left(1);
        e
    }

    fn event(&mut self, e: Ev) {
        if let Some(s) = self.events.iter_mut().find(|s| s.is_none()) {
            *s = Some(e);
        }
    }

    /// The winner, once the match is over (none for a draw).
    pub fn winner(&self) -> Option<usize> {
        if self.phase != Phase::Over || self.score[0] == self.score[1] {
            return None;
        }
        Some(usize::from(self.score[1] > self.score[0]))
    }

    /// Whether the puck is on the table and free to move.
    pub fn puck_free(&self) -> bool {
        self.phase == Phase::Live
    }

    /// Whether the puck is shown.
    pub fn puck_shown(&self) -> bool {
        matches!(
            self.phase,
            Phase::Live | Phase::Serve { .. } | Phase::Stall { .. }
        )
    }

    fn serve(&mut self, side: u8) {
        self.puck = Puck {
            x: serve_x(usize::from(side)),
            y: CY,
            vx: 0,
            vy: 0,
        };
        self.trail = [((self.puck.x / PX) as i16, (self.puck.y / PX) as i16); 3];
        self.phase = Phase::Serve {
            side,
            t: SERVE_TICKS,
        };
        self.half = side;
        self.half_ticks = 0;
        self.rally = 0;
        // A mallet standing on the serve spot steps off it.
        for i in 0..self.mallets() {
            self.give_way(i, None);
        }
    }

    // ---- One tick -----------------------------------------------------------------

    /// One tick of 1/60 s with one intent per mallet (the unused second
    /// pair's are ignored).
    pub fn tick(&mut self, intents: &[Intent; 4]) {
        if self.phase == Phase::Over {
            return;
        }
        self.age = self.age.wrapping_add(1);
        if let Some((x, y, t)) = self.spark {
            self.spark = (t > 1).then_some((x, y, t - 1));
        }
        // The clock runs all the time but for GOAL!.
        if self.rules.length == Length::Timed && !matches!(self.phase, Phase::Goal { .. }) {
            self.clock = self.clock.saturating_sub(1);
            if self.clock == 0 {
                self.time_up();
                if self.phase == Phase::Over {
                    return;
                }
            }
        }
        // The mallets' wishes for this tick.
        let n = self.mallets();
        let mut d = [(0i32, 0i32); 4];
        for (i, di) in d.iter_mut().enumerate().take(n) {
            *di = self.drive(i, &intents[i]);
        }
        match self.phase {
            Phase::Live => self.move_all(&d),
            _ => {
                // No live puck: the mallets move on their own.
                self.move_all(&d);
                self.next_phase();
            }
        }
        self.trail = [
            ((self.puck.x / PX) as i16, (self.puck.y / PX) as i16),
            self.trail[0],
            self.trail[1],
        ];
        if self.phase == Phase::Live {
            self.rally = self.rally.saturating_add(1);
            self.watch_half();
        }
    }

    /// Serve, goal and stall count down; then the next thing happens.
    fn next_phase(&mut self) {
        match self.phase {
            Phase::Serve { side, t } => {
                self.phase = if t <= 1 {
                    Phase::Live
                } else {
                    Phase::Serve { side, t: t - 1 }
                };
            }
            Phase::Goal { side, t } => {
                if t > 1 {
                    self.phase = Phase::Goal { side, t: t - 1 };
                } else if self.decided() {
                    self.finish();
                } else {
                    // The side that let the goal in serves.
                    self.serve(1 - side);
                }
            }
            Phase::Stall { to, t } => {
                if t > 1 {
                    self.phase = Phase::Stall { to, t: t - 1 };
                } else {
                    self.serve(to);
                }
            }
            Phase::Live | Phase::Over => {}
        }
    }

    /// Whether the score ends the match.
    fn decided(&self) -> bool {
        match self.rules.length.goals() {
            Some(g) => self.score[0] >= g || self.score[1] >= g,
            // Timed: a goal in the golden goal ends it.
            None => self.overtime && self.score[0] != self.score[1],
        }
    }

    fn finish(&mut self) {
        self.phase = Phase::Over;
        self.event(Ev::Over);
    }

    /// The clock of a timed match ran out.
    fn time_up(&mut self) {
        if matches!(self.phase, Phase::Goal { .. }) {
            return;
        }
        if self.overtime || self.score[0] != self.score[1] {
            self.finish();
        } else {
            self.overtime = true;
            self.clock = OT_TICKS;
            self.event(Ev::Overtime);
        }
    }

    /// A puck that stays on one half too long goes to the other side.
    fn watch_half(&mut self) {
        let half = u8::from(self.puck.x >= CX);
        if half != self.half {
            self.half = half;
            self.half_ticks = 0;
            return;
        }
        self.half_ticks = self.half_ticks.saturating_add(1);
        if self.half_ticks >= STALL_TICKS {
            let to = 1 - half;
            self.phase = Phase::Stall { to, t: STALL_SHOW };
            self.event(Ev::Stall { to });
        }
    }

    /// How far mallet `i` wants to move this tick (before the table
    /// stops it), from its driver's intent.
    fn drive(&mut self, i: usize, it: &Intent) -> (i32, i32) {
        let (px, py) = (self.puck.x, self.puck.y);
        let m = &mut self.m[i];
        m.cool = m.cool.saturating_sub(1);
        let wants_dash = match *it {
            Intent::Pad { dash, .. } | Intent::Go { dash, .. } => dash,
            Intent::Stay => false,
        };
        if wants_dash && m.dash == 0 && m.cool == 0 {
            // The dash goes the way the driver points, or at the puck.
            let way = match *it {
                Intent::Pad { dx, dy, .. } if (dx, dy) != (0, 0) => (dx, dy),
                Intent::Go { x, y, .. } if (x - m.x, y - m.y) != (0, 0) => (x - m.x, y - m.y),
                _ => (px - m.x, py - m.y),
            };
            let dir = norm(way.0, way.1, UNIT);
            if dir != (0, 0) {
                m.dash = DASH_TICKS;
                m.dash_dir = dir;
                m.cool = DASH_COOL;
            }
        }
        if m.dash > 0 {
            m.dash -= 1;
            let (dx, dy) = m.dash_dir;
            let v = (mul_div(dx, DASH_V, UNIT), mul_div(dy, DASH_V, UNIT));
            if m.dash == 0 {
                // Out of the dash at a pad's top speed, the same way.
                (m.vx, m.vy) = clamp_len(v.0, v.1, PAD_V);
            } else {
                (m.vx, m.vy) = v;
            }
            return v;
        }
        match *it {
            Intent::Stay => {
                let l = len(m.vx, m.vy);
                (m.vx, m.vy) = if l <= 3 * PAD_ACC {
                    (0, 0)
                } else {
                    clamp_len(m.vx, m.vy, l - 3 * PAD_ACC)
                };
                (m.vx, m.vy)
            }
            Intent::Pad { dx, dy, .. } => {
                let (dx, dy) = clamp_len(dx, dy, UNIT);
                let want = (mul_div(dx, PAD_V, UNIT), mul_div(dy, PAD_V, UNIT));
                // Faster to stop than to start: a let-go stops at once.
                let acc = if want == (0, 0) { 3 * PAD_ACC } else { PAD_ACC };
                let (sx, sy) = clamp_len(want.0 - m.vx, want.1 - m.vy, acc);
                m.vx += sx;
                m.vy += sy;
                (m.vx, m.vy)
            }
            Intent::Go { x, y, v, .. } => {
                let v = v.clamp(0, DASH_V);
                let step = clamp_len(x - m.x, y - m.y, v);
                (m.vx, m.vy) = step;
                step
            }
        }
    }

    /// Moves the mallets by `d` and, while the puck is live, the puck by
    /// its speed, in sub-steps (see the file's notes).
    fn move_all(&mut self, d: &[(i32, i32); 4]) {
        let n = self.mallets();
        let live = self.phase == Phase::Live;
        let start: [(i32, i32); 4] = core::array::from_fn(|i| (self.m[i].x, self.m[i].y));
        // Enough sub-steps for the fastest pair (puck against mallet).
        let pv = if live {
            len(self.puck.vx, self.puck.vy)
        } else {
            0
        };
        let fastest = d[..n].iter().map(|v| len(v.0, v.1)).max().unwrap_or(0);
        let subs = ((pv + fastest) / STEP + 1).clamp(1, MAX_SUB);
        // A puck waiting to be served (or shown after TOO SLOW) is solid.
        let solid = matches!(self.phase, Phase::Serve { .. } | Phase::Stall { .. });
        for k in 1..=subs {
            // Each mallet moves in even parts of its tick; the puck by its
            // speed now (a hit changes it), split the same way.
            let back: [(i32, i32); 4] = core::array::from_fn(|i| (self.m[i].x, self.m[i].y));
            #[cfg(debug_assertions)]
            let before = (back, (self.puck.x, self.puck.y));
            let part = |v: i32| mul_div(v, k, subs) - mul_div(v, k - 1, subs);
            for (i, di) in d.iter().enumerate().take(n) {
                self.m[i].x += part(di.0);
                self.m[i].y += part(di.1);
                self.keep_mallet(i);
            }
            self.mallets_apart();
            if live {
                self.puck.x += part(self.puck.vx);
                self.puck.y += part(self.puck.vy);
                if self.resolve(d, &back) {
                    return; // a goal
                }
            } else if solid {
                for (i, b) in back.iter().enumerate().take(n) {
                    self.give_way(i, Some(*b));
                }
            }
            #[cfg(debug_assertions)]
            if live || solid {
                self.check_step(&before);
            }
        }
        for (m, s) in self.m.iter_mut().zip(start).take(n) {
            m.dx = m.x - s.0;
            m.dy = m.y - s.1;
        }
        if live {
            // The air is nearly free: a little drag, more at low speed.
            let p = &mut self.puck;
            p.vx = mul_div(p.vx, KEEP, 256);
            p.vy = mul_div(p.vy, KEEP, 256);
            p.vx -= p.vx.signum() * DRAG.min(p.vx.abs());
            p.vy -= p.vy.signum() * DRAG.min(p.vy.abs());
        }
    }

    /// Undoes every overlap of the live puck. True: it went in a goal.
    fn resolve(&mut self, d: &[(i32, i32); 4], back: &[(i32, i32); 4]) -> bool {
        let n = self.mallets();
        for _ in 0..3 {
            let mut hit = false;
            for (i, di) in d.iter().enumerate().take(n) {
                // A mallet stopped by the table hits with what it moved.
                hit |= self.hit_mallet(i, *di);
            }
            if self.rules.wild {
                for b in BUMPERS {
                    hit |= self.hit_round(b.0, b.1, BUMPER_R + PUCK_R, (0, 0), E_BUMPER);
                }
            }
            if let Some(side) = self.puck_rails() {
                self.goal(side);
                return true;
            }
            if !hit {
                break;
            }
        }
        // Still against a mallet: the puck is squeezed against a rail, a
        // bumper or another mallet. The mallet gives way.
        for (i, b) in back.iter().enumerate().take(n) {
            if self.give_way(i, Some(*b)) {
                self.squeezes = self.squeezes.saturating_add(1);
            }
        }
        false
    }

    /// The puck against mallet `i` (moving `v` a tick). True: they
    /// touched.
    fn hit_mallet(&mut self, i: usize, v: (i32, i32)) -> bool {
        let (mx, my) = (self.m[i].x, self.m[i].y);
        self.hit_round(mx, my, HIT_R, v, E_MALLET)
    }

    /// The puck against a round thing at `(cx, cy)`, `r` from the
    /// puck's centre, moving `v` a tick, with bounciness `e`.
    fn hit_round(&mut self, cx: i32, cy: i32, r: i32, v: (i32, i32), e: i32) -> bool {
        let p = &mut self.puck;
        let (dx, dy) = (p.x - cx, p.y - cy);
        if len2(dx, dy) >= len2(r, 0) {
            return false;
        }
        let (nx, ny) = if (dx, dy) == (0, 0) {
            // Dead centre: out the way the puck's half faces.
            (if p.x < CX { -UNIT } else { UNIT }, 0)
        } else {
            norm(dx, dy, UNIT)
        };
        // Out to just past touching (the direction is a little short of
        // `UNIT`, so 3 units more).
        p.x = cx + mul_div(nx, r + 3, UNIT);
        p.y = cy + mul_div(ny, r + 3, UNIT);
        let (rx, ry) = (p.vx - v.0, p.vy - v.1);
        let vn = mul_div(rx, nx, UNIT) + mul_div(ry, ny, UNIT);
        if vn < 0 {
            let k = mul_div(vn, 256 + e, 256);
            p.vx -= mul_div(k, nx, UNIT);
            p.vy -= mul_div(k, ny, UNIT);
            (p.vx, p.vy) = clamp_len(p.vx, p.vy, PUCK_MAX);
            if -vn >= SPARK_V {
                let at = (
                    ((cx + mul_div(nx, r - PUCK_R, UNIT)) / PX) as i16,
                    ((cy + mul_div(ny, r - PUCK_R, UNIT)) / PX) as i16,
                );
                self.spark = Some((at.0, at.1, SPARK_TICKS));
                self.event(Ev::Hit { speed: -vn });
            }
        }
        true
    }

    /// The rails, the posts and the goal slots. Some(side): the puck
    /// went into `side`'s goal.
    fn puck_rails(&mut self) -> Option<u8> {
        let p = &mut self.puck;
        // The long rails.
        if p.y < YT + PUCK_R {
            p.y = (2 * (YT + PUCK_R) - p.y).min(YB - PUCK_R);
            if p.vy < 0 {
                p.vy = mul_div(-p.vy, E_RAIL, 256);
            }
        }
        if p.y > YB - PUCK_R {
            p.y = (2 * (YB - PUCK_R) - p.y).max(YT + PUCK_R);
            if p.vy > 0 {
                p.vy = -mul_div(p.vy, E_RAIL, 256);
            }
        }
        // The end rails, each with its slot.
        for side in 0..2usize {
            let (line, a) = (goal_line(side), attack(side));
            let inside = (p.x - line) * a; // how far onto the table
            if inside >= PUCK_R {
                continue;
            }
            if inside < 0 && (p.y - CY).abs() < GOAL_HALF {
                return Some(side as u8);
            }
            // The two pieces of rail beside the slot; their ends are the
            // posts.
            for (y0, y1) in [(YT, CY - GOAL_HALF), (CY + GOAL_HALF, YB)] {
                let cy = p.y.clamp(y0, y1);
                let (dx, dy) = (p.x - line, p.y - cy);
                let (nx, ny) = if dx * a <= 0 && dy == 0 {
                    // On or behind the rail itself: straight back out.
                    (a * UNIT, 0)
                } else if len2(dx, dy) < len2(PUCK_R, 0) {
                    norm(dx, dy, UNIT)
                } else {
                    continue;
                };
                p.x = line + mul_div(nx, PUCK_R + 3, UNIT);
                p.y = cy + mul_div(ny, PUCK_R + 3, UNIT);
                let vn = mul_div(p.vx, nx, UNIT) + mul_div(p.vy, ny, UNIT);
                if vn < 0 {
                    let k = mul_div(vn, 256 + E_RAIL, 256);
                    p.vx -= mul_div(k, nx, UNIT);
                    p.vy -= mul_div(k, ny, UNIT);
                }
            }
        }
        // Last guard: never off the surface (the slot aside).
        p.y = p.y.clamp(YT + PUCK_R, YB - PUCK_R);
        if (p.y - CY).abs() >= GOAL_HALF {
            p.x = p.x.clamp(XL + PUCK_R, XR - PUCK_R);
        }
        None
    }

    fn goal(&mut self, conceded: u8) {
        let side = 1 - conceded;
        let s = &mut self.score[usize::from(side)];
        *s = s.saturating_add(1);
        self.phase = Phase::Goal {
            side,
            t: GOAL_TICKS,
        };
        self.puck.vx = 0;
        self.puck.vy = 0;
        self.event(Ev::Goal { side });
    }

    /// Keeps mallet `i` in its zone and off the bumpers.
    fn keep_mallet(&mut self, i: usize) {
        let (x0, x1) = zone_x(i, self.rules.four);
        let m = &mut self.m[i];
        m.x = m.x.clamp(x0, x1);
        m.y = m.y.clamp(YT + MALLET_R, YB - MALLET_R);
        if self.rules.wild {
            for (bx, by) in BUMPERS {
                let r = BUMPER_R + MALLET_R;
                let (dx, dy) = (m.x - bx, m.y - by);
                if len2(dx, dy) < len2(r, 0) {
                    let (nx, ny) = if (dx, dy) == (0, 0) {
                        (-attack(i % 2) * UNIT, 0)
                    } else {
                        norm(dx, dy, UNIT)
                    };
                    m.x = bx + mul_div(nx, r + 3, UNIT);
                    m.y = by + mul_div(ny, r + 3, UNIT);
                }
            }
            m.x = m.x.clamp(x0, x1);
            m.y = m.y.clamp(YT + MALLET_R, YB - MALLET_R);
        }
    }

    /// Mallets never overlap: each pair that does is pushed apart.
    fn mallets_apart(&mut self) {
        let n = self.mallets();
        let r = 2 * MALLET_R;
        for a in 0..n {
            for b in a + 1..n {
                let (dx, dy) = (self.m[b].x - self.m[a].x, self.m[b].y - self.m[a].y);
                if len2(dx, dy) >= len2(r, 0) {
                    continue;
                }
                let (nx, ny) = if (dx, dy) == (0, 0) {
                    (0, UNIT)
                } else {
                    norm(dx, dy, UNIT)
                };
                let push = (r - len(dx, dy)) / 2 + 2;
                self.m[a].x -= mul_div(nx, push, UNIT);
                self.m[a].y -= mul_div(ny, push, UNIT);
                self.m[b].x += mul_div(nx, push, UNIT);
                self.m[b].y += mul_div(ny, push, UNIT);
                self.keep_mallet(a);
                self.keep_mallet(b);
            }
        }
    }

    /// Mallet `i` steps off the puck if it overlaps it. True: it had to.
    ///
    /// Straight away from the puck when the table lets it. When its zone,
    /// a rail or a bumper is in the way: to the nearest free spot round
    /// the puck on the side it is already on, or back where it was at the
    /// start of this sub-step when that is nearer and free (a mallet never
    /// jumps round a puck pinned against it). When there is none: back where it was
    /// at the start of this sub-step (`back`), and the puck a short step
    /// ([`GIVE`]) aside if it still touches. Without `back` (a new serve):
    /// the nearest free spot on any side.
    fn give_way(&mut self, i: usize, back: Option<(i32, i32)>) -> bool {
        let p = self.puck;
        let (ox, oy) = (self.m[i].x, self.m[i].y);
        let (dx, dy) = (ox - p.x, oy - p.y);
        if len2(dx, dy) >= len2(HIT_R, 0) {
            return false;
        }
        let (nx, ny) = if (dx, dy) == (0, 0) {
            (-attack(i % 2) * UNIT, 0)
        } else {
            norm(dx, dy, UNIT)
        };
        let (sx, sy) = (
            p.x + mul_div(nx, HIT_R + 3, UNIT),
            p.y + mul_div(ny, HIT_R + 3, UNIT),
        );
        if self.mallet_to(i, sx, sy) {
            return true;
        }
        // Staying where it was at the start of this sub-step is a choice
        // too when that spot is clear (always, while the puck waits to be
        // served or after TOO SLOW): then the mallet never jumps round
        // the puck by more than this sub-step's own move.
        let mut best: Option<(i64, i32, i32)> = back.and_then(|(bx, by)| {
            (len2(bx - p.x, by - p.y) >= len2(HIT_R, 0)).then(|| (len2(bx - ox, by - oy), bx, by))
        });
        for r in [HIT_R + 3, HIT_R + 3 * PX, HIT_R + 8 * PX] {
            for k in 0..32 {
                let (ux, uy) = dir64(2 * k);
                // Its own side of the puck only (a quarter turn each way).
                let side = i64::from(ux) * i64::from(nx) + i64::from(uy) * i64::from(ny);
                if back.is_some() && side < 0 {
                    continue;
                }
                let (x, y) = (p.x + mul_div(ux, r, UNIT), p.y + mul_div(uy, r, UNIT));
                if self.mallet_to(i, x, y) {
                    let (mx, my) = (self.m[i].x, self.m[i].y);
                    let cost = len2(mx - ox, my - oy);
                    // In play a mallet slides at most SLIDE round the
                    // puck; further, the puck gives way instead (below).
                    let short = back.is_none() || cost <= len2(SLIDE, 0);
                    if short && best.is_none_or(|b| cost < b.0) {
                        best = Some((cost, mx, my));
                    }
                }
            }
            if best.is_some() {
                break;
            }
        }
        let (x, y) = match (best, back) {
            (Some(b), _) => (b.1, b.2),
            (None, Some(b)) => b,
            // (Nowhere free cannot happen on this table; then it stays.)
            (None, None) => (ox, oy),
        };
        self.m[i].x = x;
        self.m[i].y = y;
        if len2(x - p.x, y - p.y) < len2(HIT_R, 0) {
            // The puck came at it too: the puck goes to the nearest free
            // spot beside the mallet.
            self.puck_aside(i);
        }
        true
    }

    /// The puck to the nearest spot just off mallet `i` that is on the
    /// table, clear of the other mallets and the bumpers, and at most
    /// two [`GIVE`] away (none: it stays).
    fn puck_aside(&mut self, i: usize) {
        let (mx, my) = (self.m[i].x, self.m[i].y);
        let p = self.puck;
        let mut best: Option<(i64, i32, i32)> = None;
        for k in 0..64 {
            let (ux, uy) = dir64(k);
            let (x, y) = (
                mx + mul_div(ux, HIT_R + 3, UNIT),
                my + mul_div(uy, HIT_R + 3, UNIT),
            );
            let on = (XL + PUCK_R..=XR - PUCK_R).contains(&x)
                && (YT + PUCK_R..=YB - PUCK_R).contains(&y);
            let clear = (0..self.mallets())
                .all(|j| j == i || len2(x - self.m[j].x, y - self.m[j].y) >= len2(HIT_R, 0));
            let off_bumpers = !self.rules.wild
                || BUMPERS
                    .iter()
                    .all(|b| len2(x - b.0, y - b.1) >= len2(BUMPER_R + PUCK_R, 0));
            let cost = len2(x - p.x, y - p.y);
            // The nearest spot wins; it may be up to two GIVE away when
            // two mallets and a rail close the nearer ones.
            let near = cost <= len2(2 * GIVE, 0);
            if on && clear && off_bumpers && near && best.is_none_or(|b| cost < b.0) {
                best = Some((cost, x, y));
            }
        }
        if let Some((_, x, y)) = best {
            self.puck.x = x;
            self.puck.y = y;
        }
    }

    /// Puts mallet `i` at `(x, y)` as far as its zone and the bumpers
    /// let it. True: it is then clear of the puck.
    fn mallet_to(&mut self, i: usize, x: i32, y: i32) -> bool {
        self.m[i].x = x;
        self.m[i].y = y;
        self.keep_mallet(i);
        let p = self.puck;
        len2(self.m[i].x - p.x, self.m[i].y - p.y) >= len2(HIT_R, 0)
    }

    /// Debug builds: after every sub-step the puck is on the table (or
    /// just scored) and off every mallet; and against each mallet its
    /// straight path in the sub-step stayed out of the mallet's core
    /// (half the touching distance round its centre): it went round a
    /// mallet, never through. No jump longer than the touching distance.
    #[cfg(debug_assertions)]
    fn check_step(&self, before: &([(i32, i32); 4], (i32, i32))) {
        let p = self.puck;
        assert!(
            p.y >= YT + PUCK_R && p.y <= YB - PUCK_R,
            "puck off the table: {p:?}"
        );
        assert!(
            p.x >= XL - PUCK_R && p.x <= XR + PUCK_R,
            "puck off the table: {p:?}"
        );
        if (p.y - CY).abs() >= GOAL_HALF {
            assert!(
                p.x >= XL + PUCK_R && p.x <= XR - PUCK_R,
                "puck in a rail: {p:?}"
            );
        }
        for i in 0..self.mallets() {
            let m = self.m[i];
            let (dx, dy) = (p.x - m.x, p.y - m.y);
            assert!(
                len2(dx, dy) >= len2(HIT_R - 4, 0),
                "puck inside mallet {i}: {p:?} {m:?}"
            );
            // The path against this mallet: from b to (dx, dy).
            let (bx, by) = (before.1.0 - before.0[i].0, before.1.1 - before.0[i].1);
            let (sx, sy) = (dx - bx, dy - by);
            let moved = len(sx, sy);
            assert!(
                moved <= HIT_R,
                "puck jumped {moved} against mallet {i}: {p:?} {m:?} {before:?} {:?}",
                self.phase
            );
            // The point of that path nearest the mallet's centre.
            let l2 = len2(sx, sy).max(1);
            let (b, s) = (
                (i64::from(bx), i64::from(by)),
                (i64::from(sx), i64::from(sy)),
            );
            let t = (-(b.0 * s.0 + b.1 * s.1)).clamp(0, l2);
            let (cx, cy) = (b.0 + s.0 * t / l2, b.1 + s.1 * t / l2);
            assert!(
                cx * cx + cy * cy >= i64::from(HIT_R / 2).pow(2),
                "puck passed through mallet {i}: {p:?} {m:?} {before:?}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STILL: [Intent; 4] = [Intent::Stay; 4];

    fn live(rules: Rules) -> Match {
        let mut m = Match::new(rules, 0, 7);
        m.phase = Phase::Live;
        m
    }

    /// Puts mallet 0 at `(x, y)` and mallet 1 far away.
    fn place(m: &mut Match, x: i32, y: i32) {
        m.m[0].x = x;
        m.m[0].y = y;
        m.m[1].x = XR - MALLET_R;
        m.m[1].y = YT + MALLET_R;
    }

    #[test]
    fn the_table_is_symmetric() {
        assert_eq!(CX - XL, XR - CX);
        assert_eq!(CY - YT, YB - CY);
        for i in 0..4 {
            for four in [false, true] {
                let (a0, a1) = zone_x(i, four);
                let (b0, b1) = zone_x(i ^ 1, four);
                assert_eq!(a0 - XL, XR - b1, "{i} {four}");
                assert_eq!(a1 - XL, XR - b0, "{i} {four}");
                let (hx, hy) = home(i, four);
                let (gx, gy) = home(i ^ 1, four);
                assert_eq!(hx - XL, XR - gx);
                assert_eq!(hy - CY, CY - gy);
            }
        }
    }

    #[test]
    fn a_puck_at_top_speed_never_passes_a_still_mallet() {
        // Every angle, every offset across the mallet: the puck comes
        // back, or glances off, but never comes out on the far side.
        for a in 0..64 {
            for off in -12..=12 {
                let mut m = live(Rules::DEFAULT);
                place(&mut m, 40 * PX, CY);
                let (ux, uy) = dir64(a);
                // Start 30 pixels away, aimed at the mallet with an offset
                // across the line of travel.
                let side = (-uy, ux);
                m.puck.x = 40 * PX - mul_div(ux, 30 * PX, UNIT) + mul_div(side.0, off * PX, UNIT);
                m.puck.y = CY - mul_div(uy, 30 * PX, UNIT) + mul_div(side.1, off * PX, UNIT);
                if m.puck.y < YT + PUCK_R || m.puck.y > YB - PUCK_R || m.puck.x < XL + PUCK_R {
                    continue;
                }
                m.puck.vx = mul_div(ux, PUCK_MAX, UNIT);
                m.puck.vy = mul_div(uy, PUCK_MAX, UNIT);
                let along0 = along(&m, ux, uy);
                let mut near = false;
                for _ in 0..20 {
                    m.tick(&STILL);
                    if m.phase != Phase::Live {
                        break;
                    }
                    let gap = len(m.puck.x - m.m[0].x, m.puck.y - m.m[0].y);
                    assert!(gap >= HIT_R - 4, "angle {a} offset {off}: inside");
                    // Once it has come by and gone, this pass is over (a
                    // puck back off a rail is another pass).
                    if near && gap >= 2 * HIT_R {
                        break;
                    }
                    near |= gap < 2 * HIT_R;
                    // Past the mallet along the travel line while close
                    // across it would mean it went through.
                    let across = mul_div(m.puck.x - m.m[0].x, side.0, UNIT)
                        + mul_div(m.puck.y - m.m[0].y, side.1, UNIT);
                    if along(&m, ux, uy) > 0 && along0 < 0 {
                        assert!(across.abs() >= HIT_R / 3, "angle {a} offset {off}: through");
                    }
                }
            }
        }
    }

    fn along(m: &Match, ux: i32, uy: i32) -> i32 {
        mul_div(m.puck.x - m.m[0].x, ux, UNIT) + mul_div(m.puck.y - m.m[0].y, uy, UNIT)
    }

    #[test]
    fn a_dashing_mallet_never_passes_a_still_puck() {
        for a in 0..64 {
            let mut m = live(Rules::DEFAULT);
            let (ux, uy) = dir64(a);
            m.puck.x = 40 * PX;
            m.puck.y = CY;
            place(
                &mut m,
                40 * PX - mul_div(ux, 20 * PX, UNIT),
                CY - mul_div(uy, 20 * PX, UNIT),
            );
            m.m[0].cool = 0;
            let it = [
                Intent::Pad {
                    dx: ux,
                    dy: uy,
                    dash: true,
                },
                Intent::Stay,
                Intent::Stay,
                Intent::Stay,
            ];
            let mut hit = false;
            for _ in 0..12 {
                m.tick(&it);
                if m.phase != Phase::Live {
                    break; // a goal: the puck is off the table
                }
                let gap = len(m.puck.x - m.m[0].x, m.puck.y - m.m[0].y);
                assert!(gap >= HIT_R - 4, "angle {a}: inside");
                hit |= m.puck.speed() > 0;
            }
            assert!(hit, "angle {a}: the dash never reached the puck");
        }
    }

    #[test]
    fn a_hit_carries_the_mallet_speed_up_to_the_top_speed() {
        let mut m = live(Rules::DEFAULT);
        m.puck.x = 50 * PX;
        m.puck.y = CY;
        place(&mut m, 30 * PX, CY);
        let push = [
            Intent::Pad {
                dx: UNIT,
                dy: 0,
                dash: false,
            },
            Intent::Stay,
            Intent::Stay,
            Intent::Stay,
        ];
        for _ in 0..30 {
            m.tick(&push);
            if m.puck.speed() > 0 {
                break;
            }
        }
        // A mallet at full pad speed gives the puck more than its own.
        assert!(m.puck.vx > PAD_V, "{:?}", m.puck);
        assert!(m.puck.speed() <= PUCK_MAX);
    }

    #[test]
    fn rails_and_posts_hold_a_puck_at_any_angle_and_top_speed() {
        for a in 0..64 {
            for y in [
                YT + 5 * PX,
                CY - GOAL_HALF,
                CY - GOAL_HALF + 2 * PX,
                CY,
                YB - 5 * PX,
            ] {
                let mut m = live(Rules::DEFAULT);
                m.m[0].x = XL + MALLET_R;
                m.m[0].y = YT + MALLET_R;
                m.m[1].x = XR - MALLET_R;
                m.m[1].y = YT + MALLET_R;
                m.puck.x = CX;
                m.puck.y = y;
                let (ux, uy) = dir64(a);
                m.puck.vx = mul_div(ux, PUCK_MAX, UNIT);
                m.puck.vy = mul_div(uy, PUCK_MAX, UNIT);
                for _ in 0..600 {
                    m.tick(&STILL);
                    if m.phase != Phase::Live {
                        break;
                    }
                }
            }
        }
    }

    #[test]
    fn a_puck_through_the_slot_is_a_goal_for_the_other_side() {
        let mut m = live(Rules::DEFAULT);
        place(&mut m, XL + MALLET_R, YT + MALLET_R);
        m.puck.x = 30 * PX;
        m.puck.y = CY + 3 * PX;
        m.puck.vx = -PUCK_MAX;
        for _ in 0..20 {
            m.tick(&STILL);
        }
        assert_eq!(m.score, [0, 1]);
        assert!(matches!(m.phase, Phase::Goal { side: 1, .. }));
        assert_eq!(m.take_event(), Some(Ev::Goal { side: 1 }));
        // After GOAL!, the side that let it in serves.
        for _ in 0..GOAL_TICKS {
            m.tick(&STILL);
        }
        assert!(matches!(m.phase, Phase::Serve { side: 0, .. }));
        assert!(m.puck.x < CX);
    }

    #[test]
    fn a_puck_off_the_slot_bounces_off_the_end_rail() {
        let mut m = live(Rules::DEFAULT);
        place(&mut m, 40 * PX, YB - MALLET_R);
        m.puck.x = 30 * PX;
        m.puck.y = YT + 8 * PX;
        m.puck.vx = -PUCK_MAX;
        for _ in 0..10 {
            m.tick(&STILL);
        }
        assert_eq!(m.score, [0, 0]);
        assert!(m.puck.vx > 0);
    }

    #[test]
    fn a_puck_nobody_hits_comes_to_rest_and_then_changes_sides() {
        let mut m = live(Rules::DEFAULT);
        place(&mut m, XL + MALLET_R, YT + MALLET_R);
        m.puck.x = 50 * PX;
        m.puck.y = YT + 20 * PX;
        m.puck.vx = 0;
        m.puck.vy = PX;
        let mut t = 0;
        while m.phase == Phase::Live {
            m.tick(&STILL);
            t += 1;
            assert!(t < 2000);
        }
        assert_eq!(m.puck.speed(), 0);
        assert!(matches!(m.phase, Phase::Stall { to: 1, .. }));
        for _ in 0..STALL_SHOW {
            m.tick(&STILL);
        }
        assert!(matches!(m.phase, Phase::Serve { side: 1, .. }));
        assert!(m.puck.x > CX);
    }

    #[test]
    fn mallets_stay_in_their_zones() {
        for four in [false, true] {
            let mut m = live(Rules {
                four,
                wild: true,
                ..Rules::DEFAULT
            });
            for k in 0..400 {
                let (ux, uy) = dir64(k / 20 * 7);
                let it = Intent::Pad {
                    dx: ux,
                    dy: uy,
                    dash: k % 30 == 0,
                };
                m.tick(&[it; 4]);
                for i in 0..m.mallets() {
                    let (x0, x1) = zone_x(i, four);
                    let mm = m.m[i];
                    assert!((x0..=x1).contains(&mm.x), "{i}: {mm:?}");
                    assert!((YT + MALLET_R..=YB - MALLET_R).contains(&mm.y));
                    for b in BUMPERS {
                        assert!(len(mm.x - b.0, mm.y - b.1) >= BUMPER_R + MALLET_R - 4);
                    }
                }
            }
        }
    }

    #[test]
    fn first_to_five_ends_at_five() {
        let mut m = live(Rules {
            length: Length::First5,
            ..Rules::DEFAULT
        });
        for g in 0..5 {
            m.phase = Phase::Live;
            m.puck.x = XL + 6 * PX;
            m.puck.y = CY;
            m.puck.vx = -PUCK_MAX;
            m.puck.vy = 0;
            place(&mut m, XL + MALLET_R, YT + MALLET_R);
            for _ in 0..GOAL_TICKS + 4 {
                m.tick(&STILL);
            }
            assert_eq!(m.score[1], g + 1);
        }
        assert_eq!(m.phase, Phase::Over);
        assert_eq!(m.winner(), Some(1));
    }

    #[test]
    fn a_timed_match_level_at_the_end_plays_a_golden_goal_then_draws() {
        let mut m = Match::new(
            Rules {
                length: Length::Timed,
                ..Rules::DEFAULT
            },
            0,
            3,
        );
        let mut t = 0u32;
        // Nobody plays: the puck goes back and forth by the stall rule.
        while m.phase != Phase::Over {
            m.tick(&STILL);
            t += 1;
            assert!(t < TIMED_TICKS + OT_TICKS + 1000);
        }
        assert!(m.overtime);
        assert_eq!(m.winner(), None);
        assert!(t >= TIMED_TICKS + OT_TICKS);
    }
}
