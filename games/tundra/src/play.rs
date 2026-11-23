//! One match on the rink: skaters, goalies, the puck, the clock and the
//! score, moved in fixed ticks of 1/60 s. Whole numbers only (positions
//! in 1/256 pixel, see [`crate::fixed`]).
//!
//! The rink is the whole picture below a 12-pixel bar. The goals are
//! openings in the end boards with the net behind them, so the puck goes
//! in by crossing the board line inside the mouth. Team 0 (AMBER) defends
//! the left goal and attacks to the right; team 1 (BLUE) the other way.
//! Skater `i` belongs to team `i / 2`; slot `i % 2` is its place in the
//! team (player 1 and 2 take slot 0, players 3 and 4 slot 1).
//!
//! This file knows nothing of buttons: each tick takes one [`Intent`]
//! per skater (from a pad, the touch screen or [`crate::cpu`]).

use crate::fixed::{PX, Rng, UNIT, clamp_len, len, norm, scale};

// ---- The rink --------------------------------------------------------

/// The bar on top (pixels).
pub const BAR_H: i32 = 12;
/// The board lines (pixels). The ice is inside them.
pub const BOARD_L: i32 = 6;
pub const BOARD_R: i32 = 153;
pub const BOARD_T: i32 = 12;
pub const BOARD_B: i32 = 118;
/// The ice, in position units: x from `XL` to `XR`, y from `YT` to `YB`.
pub const XL: i32 = (BOARD_L + 1) * PX;
pub const XR: i32 = BOARD_R * PX;
pub const YT: i32 = (BOARD_T + 1) * PX;
pub const YB: i32 = BOARD_B * PX;
/// The middle of the ice.
pub const CX: i32 = (XL + XR) / 2;
pub const CY: i32 = (YT + YB) / 2;
/// The corners are cut at 45 degrees, this far along each board.
pub const CUT: i32 = 8 * PX;
/// Half the goal mouth (the opening in the end board).
pub const MOUTH: i32 = 10 * PX;
/// How deep the net is behind the board line.
pub const NET_DEPTH: i32 = 4 * PX;
pub const SKATER_R: i32 = 3 * PX;
pub const PUCK_R: i32 = PX;
/// The goalies stand this far out from their board line.
pub const GOALIE_OUT: i32 = 3 * PX;
/// Half the goalie's width and height (the box that stops the puck).
pub const GOALIE_HW: i32 = 2 * PX;
pub const GOALIE_HH: i32 = 6 * PX;
/// How far a goalie may move from the middle of the mouth.
pub const GOALIE_RANGE: i32 = 7 * PX;

// ---- Movement (per tick) ----------------------------------------------

/// Top skating speed, and with the puck.
pub const SKATE_V: i32 = 232;
pub const CARRY_V: i32 = 212;
/// How fast a skater changes speed (the ice is slippery).
pub const ACCEL: i32 = 9;
/// Gliding with no input keeps this much speed a tick (of 256).
pub const GLIDE: i32 = 250;
/// The check: a short dash.
pub const DASH_V: i32 = 430;
pub const DASH_TICKS: u8 = 8;
pub const CHECK_COOL: u8 = 50;
/// A skater who lost the puck to a check cannot take it for this long.
pub const STUN_TICKS: u8 = 36;
/// After shooting or passing, the puck cannot be taken back at once.
pub const NO_PICK_TICKS: u8 = 12;
/// The shoot button, by a person: let go within `TAP_TICKS` it passes;
/// held longer it shoots, from `SHOT_MIN` up to `SHOT_MAX` when held for
/// `CHARGE_MAX` ticks.
pub const TAP_TICKS: u8 = 7;
pub const SHOT_MIN: i32 = 520;
pub const SHOT_MAX: i32 = 960;
pub const CHARGE_MAX: u8 = 30;
/// The puck keeps this much speed a tick (of 256), and this much at a
/// board.
pub const PUCK_KEEP: i32 = 253;
pub const BOUNCE: i32 = 190;
/// A puck slower than this is covered by the goalie it hits.
pub const COVER_V: i32 = 400;
/// The goalie holds a covered puck this long, then passes it out.
pub const HOLD_TICKS: u8 = 40;

// ---- Phases (ticks) ----------------------------------------------------

pub const FACEOFF_TICKS: u16 = 50;
pub const GOAL_TICKS: u16 = 100;
pub const WHISTLE_TICKS: u16 = 90;
/// "OVERTIME" shows this long.
pub const OT_NOTE_TICKS: u16 = 120;
/// Overtime is sudden death for at most this many ticks of live play (3
/// minutes, the owner's choice of 2026-11-18). Nobody scores in that
/// time: the match ends as a draw.
pub const OT_TICKS: u32 = 3 * 60 * 60;

/// The direction team `t` attacks in: +1 right, -1 left.
pub const fn attack(team: usize) -> i32 {
    if team == 0 { 1 } else { -1 }
}

/// The board line of the goal team `t` defends.
pub const fn goal_x(team: usize) -> i32 {
    if team == 0 { XL } else { XR }
}

/// Where team `t`'s goalie stands (x).
pub const fn goalie_x(team: usize) -> i32 {
    goal_x(team) + attack(team) * GOALIE_OUT
}

// ---- Settings ------------------------------------------------------------

/// How well the computer plays (its skaters and both goalies).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Easy,
    Hard,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::Easy => "EASY",
            Level::Hard => "HARD",
        }
    }

    pub fn skill(self) -> Skill {
        match self {
            Level::Easy => Skill {
                think: 9,
                speed: 204,
                shot_range: 40 * PX,
                shot_speed: 700,
                aim_err: 6 * PX,
                pass_odds: 40,
                check_odds: 14,
                lead: 0,
                goalie_v: 150,
                goalie_think: 6,
                predict: false,
            },
            Level::Hard => Skill {
                think: 3,
                speed: 246,
                shot_range: 48 * PX,
                shot_speed: 860,
                aim_err: 3 * PX,
                pass_odds: 8,
                check_odds: 3,
                lead: 8,
                goalie_v: 240,
                goalie_think: 2,
                predict: true,
            },
        }
    }
}

/// The numbers behind a [`Level`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Skill {
    /// A computer skater decides again every this many ticks.
    pub think: u8,
    /// Its top speed, of 256.
    pub speed: i32,
    /// It shoots from this close to the goal.
    pub shot_range: i32,
    pub shot_speed: i32,
    /// How far its aim may be off.
    pub aim_err: i32,
    /// Under pressure it passes once in this many decisions.
    pub pass_odds: u32,
    /// Next to the carrier it checks once in this many decisions.
    pub check_odds: u32,
    /// Ticks it looks ahead when chasing a sliding puck.
    pub lead: i32,
    /// Goalie speed, how often it looks again, and whether it follows
    /// the puck's line instead of the puck.
    pub goalie_v: i32,
    pub goalie_think: u8,
    pub predict: bool,
}

/// How long a match is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Length {
    Min2,
    Min3,
    Min5,
    /// First to five goals; the clock counts up.
    First5,
}

impl Length {
    pub const ALL: [Length; 4] = [Length::Min2, Length::Min3, Length::Min5, Length::First5];

    pub fn name(self) -> &'static str {
        match self {
            Length::Min2 => "2 MIN",
            Length::Min3 => "3 MIN",
            Length::Min5 => "5 MIN",
            Length::First5 => "FIRST TO 5",
        }
    }

    /// Playing ticks on the clock, if it counts down.
    pub fn ticks(self) -> Option<u32> {
        match self {
            Length::Min2 => Some(2 * 60 * 60),
            Length::Min3 => Some(3 * 60 * 60),
            Length::Min5 => Some(5 * 60 * 60),
            Length::First5 => None,
        }
    }

    /// Goals that win, if that is the rule.
    pub fn goals(self) -> Option<u8> {
        match self {
            Length::First5 => Some(5),
            _ => None,
        }
    }
}

// ---- The things on the ice ----------------------------------------------

/// What a skater wants this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Intent {
    /// The way to skate, at most [`UNIT`] long (shorter is slower).
    pub dx: i32,
    pub dy: i32,
    /// The shoot button is held. Let go after a short tap it passes,
    /// after a longer hold it shoots the way the skater faces.
    pub charge: bool,
    /// Shoot now towards `(x, y)` (a direction) at this speed: for the
    /// computer and the touch screen, which aim.
    pub shot: Option<(i32, i32, i32)>,
    /// Pass with the puck, check without it.
    pub act: bool,
    /// Top speed, of 256 (0 counts as 256).
    pub speed: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Skater {
    pub x: i32,
    pub y: i32,
    pub vx: i32,
    pub vy: i32,
    /// The way it faces, [`UNIT`] long.
    pub fx: i32,
    pub fy: i32,
    /// Ticks the shoot button has been held (up to [`CHARGE_MAX`]).
    pub charge: u8,
    pub dash: u8,
    pub cool: u8,
    pub stun: u8,
    pub no_pick: u8,
    /// Ticks it has carried the puck.
    pub carry: u16,
    /// The computer: ticks to its next decision, and its way meanwhile.
    pub think: u8,
    pub want: (i32, i32),
}

impl Skater {
    /// Where the stick's blade is (the puck sits there when carried).
    pub fn blade(&self) -> (i32, i32) {
        (self.x + self.fx * 5, self.y + self.fy * 5)
    }

    pub fn speed(&self) -> i32 {
        len(self.vx, self.vy)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Goalie {
    pub y: i32,
    pub target: i32,
    pub think: u8,
    /// Ticks left holding a covered puck.
    pub hold: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Owner {
    #[default]
    Loose,
    Skater(u8),
    Goalie(u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Puck {
    pub x: i32,
    pub y: i32,
    pub vx: i32,
    pub vy: i32,
    pub owner: Owner,
    /// Ticks the goalies let it pass (just passed out by one).
    pub ghost: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Everyone waits at the centre; the puck drops at 0.
    FaceOff(u16),
    Live,
    /// `team` scored; the play stops for a moment.
    Goal {
        team: u8,
        t: u16,
    },
    /// The match is decided; "FINAL" shows.
    Whistle(u16),
    Over,
}

/// Something that happened in a tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ev {
    FaceOff,
    Goal { team: u8 },
    Save { team: u8 },
    Shot { by: u8 },
    Pass { from: u8, to: u8 },
    Steal { by: u8 },
    Take { by: u8 },
    Overtime,
    Whistle,
}

/// Skate marks on the ice: the last [`MARKS`] spots, oldest replaced.
pub const MARKS: usize = 160;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marks {
    pub at: [(u8, u8); MARKS],
    pub len: u16,
    pub next: u16,
}

impl Marks {
    pub const fn new() -> Marks {
        Marks {
            at: [(0, 0); MARKS],
            len: 0,
            next: 0,
        }
    }

    pub fn add(&mut self, x: i32, y: i32) {
        let (x, y) = (x / PX, y / PX);
        if !(BOARD_L + 1..BOARD_R).contains(&x) || !(BOARD_T + 1..BOARD_B).contains(&y) {
            return;
        }
        self.at[usize::from(self.next)] = (x as u8, y as u8);
        self.next = (self.next + 1) % MARKS as u16;
        self.len = (self.len + 1).min(MARKS as u16);
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.next = 0;
    }

    pub fn iter(&self) -> impl Iterator<Item = (u8, u8)> + '_ {
        self.at[..usize::from(self.len)].iter().copied()
    }
}

impl Default for Marks {
    fn default() -> Self {
        Marks::new()
    }
}

/// A whole match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub sk: [Skater; 4],
    pub gk: [Goalie; 2],
    pub puck: Puck,
    pub score: [u8; 2],
    /// Ticks of live play (the clock).
    pub played: u32,
    pub length: Length,
    pub level: Level,
    pub phase: Phase,
    pub overtime: bool,
    /// Ticks "OVERTIME" still shows.
    pub ot_note: u16,
    pub marks_on: bool,
    pub marks: Marks,
    pub rng: Rng,
    /// Ticks since the match began (all phases).
    pub age: u32,
    events: [Option<Ev>; 8],
}

impl Match {
    /// No match yet (the title): everything zero, the play over.
    pub const fn blank() -> Match {
        const SK: Skater = Skater {
            x: 0,
            y: 0,
            vx: 0,
            vy: 0,
            fx: UNIT,
            fy: 0,
            charge: 0,
            dash: 0,
            cool: 0,
            stun: 0,
            no_pick: 0,
            carry: 0,
            think: 0,
            want: (0, 0),
        };
        const GK: Goalie = Goalie {
            y: CY,
            target: CY,
            think: 0,
            hold: 0,
        };
        Match {
            sk: [SK; 4],
            gk: [GK; 2],
            puck: Puck {
                x: CX,
                y: CY,
                vx: 0,
                vy: 0,
                owner: Owner::Loose,
                ghost: 0,
            },
            score: [0; 2],
            played: 0,
            length: Length::Min3,
            level: Level::Easy,
            phase: Phase::Over,
            overtime: false,
            ot_note: 0,
            marks_on: false,
            marks: Marks::new(),
            rng: Rng::new(1),
            age: 0,
            events: [None; 8],
        }
    }

    pub fn new(length: Length, level: Level, marks_on: bool, seed: u32) -> Match {
        let mut m = Match {
            length,
            level,
            marks_on,
            rng: Rng::new(seed),
            ..Match::blank()
        };
        m.face_off();
        m
    }

    // ---- Reading ----------------------------------------------------------

    /// The next thing that happened, oldest first.
    pub fn take_event(&mut self) -> Option<Ev> {
        let e = self.events[0].take();
        self.events.rotate_left(1);
        e
    }

    fn event(&mut self, e: Ev) {
        if let Some(slot) = self.events.iter_mut().find(|s| s.is_none()) {
            *slot = Some(e);
        }
    }

    pub fn skill(&self) -> Skill {
        self.level.skill()
    }

    /// The skater with the puck.
    pub fn carrier(&self) -> Option<usize> {
        match self.puck.owner {
            Owner::Skater(i) => Some(usize::from(i)),
            _ => None,
        }
    }

    /// Whether team `t` has the puck (a skater or its goalie).
    pub fn team_has_puck(&self, t: usize) -> bool {
        match self.puck.owner {
            Owner::Skater(i) => usize::from(i) / 2 == t,
            Owner::Goalie(g) => usize::from(g) == t,
            Owner::Loose => false,
        }
    }

    /// The clock to show, in ticks: what is left of the match or of the
    /// overtime, or (first to five) what has been played.
    pub fn clock(&self) -> u32 {
        match self.length.ticks() {
            Some(t) if !self.overtime => t.saturating_sub(self.played),
            Some(t) => (t + OT_TICKS).saturating_sub(self.played),
            None => self.played,
        }
    }

    /// The match is over (or its final whistle is on) and level: a draw.
    /// Only a timed match whose overtime ran out ends so.
    pub fn draw(&self) -> bool {
        matches!(self.phase, Phase::Whistle(_) | Phase::Over) && self.score[0] == self.score[1]
    }

    /// The winning team once the match is decided (none for a draw).
    pub fn winner(&self) -> Option<usize> {
        if !matches!(self.phase, Phase::Whistle(_) | Phase::Over) {
            return None;
        }
        match self.score[0].cmp(&self.score[1]) {
            core::cmp::Ordering::Greater => Some(0),
            core::cmp::Ordering::Less => Some(1),
            core::cmp::Ordering::Equal => None,
        }
    }

    // ---- Setting up --------------------------------------------------------

    /// Everyone to the centre; the puck drops after [`FACEOFF_TICKS`].
    pub fn face_off(&mut self) {
        // Slot 0 meets the puck; slot 1 waits behind, one high and one
        // low, so the teams are mirror images.
        let spots = [
            (CX - 9 * PX, CY),
            (CX - 40 * PX, CY - 20 * PX),
            (CX + 9 * PX, CY),
            (CX + 40 * PX, CY + 20 * PX),
        ];
        for (i, s) in self.sk.iter_mut().enumerate() {
            let dir = attack(i / 2);
            *s = Skater {
                x: spots[i].0,
                y: spots[i].1,
                fx: dir * UNIT,
                ..Skater::default()
            };
        }
        for g in &mut self.gk {
            *g = Goalie {
                y: CY,
                target: CY,
                ..Goalie::default()
            };
        }
        self.puck = Puck {
            x: CX,
            y: CY,
            ..Puck::default()
        };
        self.phase = Phase::FaceOff(FACEOFF_TICKS);
        self.event(Ev::FaceOff);
    }

    // ---- One tick -------------------------------------------------------------

    pub fn tick(&mut self, intents: &[Intent; 4]) {
        self.age = self.age.wrapping_add(1);
        self.ot_note = self.ot_note.saturating_sub(1);
        match self.phase {
            Phase::Over => return,
            Phase::FaceOff(t) => {
                self.goalies();
                if t <= 1 {
                    self.phase = Phase::Live;
                } else {
                    self.phase = Phase::FaceOff(t - 1);
                }
                return;
            }
            Phase::Goal { team, t } => {
                self.coast();
                if t <= 1 {
                    if self.decided() {
                        self.whistle();
                    } else {
                        self.face_off();
                    }
                } else {
                    self.phase = Phase::Goal { team, t: t - 1 };
                }
                return;
            }
            Phase::Whistle(t) => {
                self.coast();
                self.phase = if t <= 1 {
                    Phase::Over
                } else {
                    Phase::Whistle(t - 1)
                };
                return;
            }
            Phase::Live => {}
        }

        // The clock: at the end of a timed match, the whistle or (level)
        // overtime; at the end of the overtime, the whistle for a draw.
        self.played = self.played.saturating_add(1);
        if let Some(limit) = self.length.ticks() {
            if !self.overtime && self.played >= limit {
                if self.score[0] != self.score[1] {
                    self.whistle();
                    return;
                }
                self.overtime = true;
                self.ot_note = OT_NOTE_TICKS;
                self.event(Ev::Overtime);
            } else if self.overtime && self.played >= limit + OT_TICKS {
                self.whistle();
                return;
            }
        }

        for s in &mut self.sk {
            s.cool = s.cool.saturating_sub(1);
            s.stun = s.stun.saturating_sub(1);
            s.no_pick = s.no_pick.saturating_sub(1);
            s.dash = s.dash.saturating_sub(1);
        }
        for (i, it) in intents.iter().enumerate() {
            self.drive(i, it);
        }
        self.bump_skaters();
        for i in 0..4 {
            self.checks(i);
        }
        self.goalies();
        self.move_puck();
        if self.marks_on && self.age.is_multiple_of(5) {
            for i in 0..4 {
                let s = self.sk[i];
                if s.speed() > 150 {
                    self.marks.add(s.x - s.fx * 2, s.y - s.fy * 2 + 2 * PX);
                }
            }
        }
    }

    /// Whether the match is over after the last goal.
    fn decided(&self) -> bool {
        if self.overtime {
            return true;
        }
        match self.length.goals() {
            Some(n) => self.score.iter().any(|&s| s >= n),
            None => false,
        }
    }

    fn whistle(&mut self) {
        self.phase = Phase::Whistle(WHISTLE_TICKS);
        self.event(Ev::Whistle);
    }

    /// The play has stopped: skaters slow down, the puck slides on.
    fn coast(&mut self) {
        for s in &mut self.sk {
            s.vx = scale(s.vx, 236);
            s.vy = scale(s.vy, 236);
            s.x += s.vx;
            s.y += s.vy;
            s.charge = 0;
            keep_in(&mut s.x, &mut s.y, &mut s.vx, &mut s.vy, SKATER_R, None);
        }
        if self.puck.owner == Owner::Loose {
            self.puck_steps(false);
        }
    }

    /// One skater follows its intent: skate, shoot, pass or check.
    fn drive(&mut self, i: usize, it: &Intent) {
        let carrying = self.carrier() == Some(i);
        let mut s = self.sk[i];
        if len(it.dx, it.dy) > 96 {
            (s.fx, s.fy) = norm(it.dx, it.dy, UNIT);
        }
        if s.dash == 0 {
            let mut top = if carrying { CARRY_V } else { SKATE_V };
            if it.speed > 0 {
                top = scale(top, it.speed);
            }
            if s.stun > 0 {
                top /= 2;
            }
            let (dx, dy) = clamp_len(it.dx, it.dy, UNIT);
            if dx == 0 && dy == 0 {
                s.vx = scale(s.vx, GLIDE);
                s.vy = scale(s.vy, GLIDE);
            } else {
                let (tx, ty) = (scale(dx, top), scale(dy, top));
                let (ax, ay) = clamp_len(tx - s.vx, ty - s.vy, ACCEL);
                s.vx += ax;
                s.vy += ay;
            }
        }
        s.x += s.vx;
        s.y += s.vy;
        keep_in(&mut s.x, &mut s.y, &mut s.vx, &mut s.vy, SKATER_R, None);
        self.sk[i] = s;

        // Shooting: an aimed shot now, or letting go of a charged one.
        if let Some((ax, ay, v)) = it.shot {
            self.sk[i].charge = 0;
            if carrying {
                self.shoot(i, ax, ay, v);
            }
        } else if it.charge {
            self.sk[i].charge = (s.charge + 1).min(CHARGE_MAX);
        } else if s.charge > 0 {
            self.sk[i].charge = 0;
            if carrying && s.charge < TAP_TICKS {
                self.pass(i);
            } else if carrying {
                let held = i32::from(s.charge - TAP_TICKS);
                let v = SHOT_MIN + (SHOT_MAX - SHOT_MIN) * held / i32::from(CHARGE_MAX - TAP_TICKS);
                self.shoot(i, s.fx, s.fy, v);
            }
        }
        if it.act {
            if self.carrier() == Some(i) {
                self.pass(i);
            } else if s.cool == 0 && s.stun == 0 && s.dash == 0 {
                let s = &mut self.sk[i];
                s.dash = DASH_TICKS;
                s.cool = CHECK_COOL;
                s.vx = scale(s.fx, DASH_V);
                s.vy = scale(s.fy, DASH_V);
                if self.marks_on {
                    let (x, y) = (s.x, s.y + 2 * PX);
                    self.marks.add(x - PX, y);
                    self.marks.add(x + PX, y);
                }
            }
        }
    }

    /// Lets the puck go from skater `i` towards `(ax, ay)` at speed `v`.
    fn shoot(&mut self, i: usize, ax: i32, ay: i32, v: i32) {
        let (dx, dy) = norm(ax, ay, v);
        if dx == 0 && dy == 0 {
            return;
        }
        let s = &mut self.sk[i];
        s.no_pick = NO_PICK_TICKS;
        s.carry = 0;
        s.fx = norm(dx, dy, UNIT).0;
        s.fy = norm(dx, dy, UNIT).1;
        let (svx, svy) = (s.vx, s.vy);
        self.puck.vx = dx + svx / 2;
        self.puck.vy = dy + svy / 2;
        self.puck.owner = Owner::Loose;
        self.event(Ev::Shot { by: i as u8 });
    }

    /// Passes from skater `i` to its team mate, a little ahead of it.
    fn pass(&mut self, i: usize) {
        let j = i ^ 1;
        let t = self.sk[j];
        let (lx, ly) = (t.x + t.vx * 12, t.y + t.vy * 12);
        let (dx, dy) = (lx - self.puck.x, ly - self.puck.y);
        let v = (360 + len(dx, dy) / 24).clamp(360, 760);
        self.shoot(i, dx, dy, v);
        // It is a pass, not a shot.
        if let Some(e) = self.events.iter_mut().rev().flatten().next()
            && *e == (Ev::Shot { by: i as u8 })
        {
            *e = Ev::Pass {
                from: i as u8,
                to: j as u8,
            };
        }
        self.sk[j].no_pick = 0;
    }

    /// A dashing skater takes the puck from a carrier it reaches.
    fn checks(&mut self, i: usize) {
        let s = self.sk[i];
        if s.dash == 0 {
            return;
        }
        let Some(k) = self.carrier() else {
            return;
        };
        if k / 2 == i / 2 {
            return;
        }
        let c = self.sk[k];
        let (bx, by) = s.blade();
        let near_body = len(c.x - s.x, c.y - s.y) <= 8 * PX;
        let near_puck = len(self.puck.x - bx, self.puck.y - by) <= 5 * PX;
        if near_body || near_puck {
            self.puck.owner = Owner::Skater(i as u8);
            self.sk[k].stun = STUN_TICKS;
            self.sk[k].carry = 0;
            self.sk[k].charge = 0;
            let s = &mut self.sk[i];
            s.dash = 0;
            s.carry = 0;
            s.vx /= 2;
            s.vy /= 2;
            self.event(Ev::Steal { by: i as u8 });
        }
    }

    /// Skaters push each other apart; goalies stand firm.
    fn bump_skaters(&mut self) {
        for a in 0..4 {
            for b in a + 1..4 {
                let (sa, sb) = (self.sk[a], self.sk[b]);
                let (mut dx, dy) = (sb.x - sa.x, sb.y - sa.y);
                let d = len(dx, dy);
                let min = 2 * SKATER_R;
                if d >= min {
                    continue;
                }
                if d == 0 {
                    dx = 1;
                }
                let (px, py) = norm(dx, dy, (min - d) / 2 + 1);
                let (ux, uy) = norm(dx, dy, UNIT);
                // Their closing speed along the line between them is shared.
                let vn = scale(sb.vx - sa.vx, ux) + scale(sb.vy - sa.vy, uy);
                let (ix, iy) = if vn < 0 {
                    (scale(ux, vn / 2), scale(uy, vn / 2))
                } else {
                    (0, 0)
                };
                let a_ = &mut self.sk[a];
                a_.x -= px;
                a_.y -= py;
                a_.vx += ix;
                a_.vy += iy;
                let b_ = &mut self.sk[b];
                b_.x += px;
                b_.y += py;
                b_.vx -= ix;
                b_.vy -= iy;
            }
        }
        for i in 0..4 {
            for g in 0..2 {
                let (gx, gy) = (goalie_x(g), self.gk[g].y);
                let s = &mut self.sk[i];
                let (hw, hh) = (GOALIE_HW + SKATER_R, GOALIE_HH + SKATER_R);
                let (ox, oy) = (s.x - gx, s.y - gy);
                if ox.abs() < hw && oy.abs() < hh {
                    // Out by the shorter way.
                    if hw - ox.abs() < hh - oy.abs() {
                        s.x = gx + ox.signum().max(-1) * hw + if ox == 0 { hw } else { 0 };
                        s.vx = 0;
                    } else {
                        s.y = gy + if oy < 0 { -hh } else { hh };
                        s.vy = 0;
                    }
                }
            }
            let s = &mut self.sk[i];
            keep_in(&mut s.x, &mut s.y, &mut s.vx, &mut s.vy, SKATER_R, None);
        }
    }

    /// The goalies follow the puck along their mouth, and pass out a
    /// puck they hold.
    fn goalies(&mut self) {
        let sk = self.skill();
        for g in 0..2 {
            let gx = goalie_x(g);
            let dir = attack(g);
            let held = self.puck.owner == Owner::Goalie(g as u8);
            if !held {
                if self.gk[g].think == 0 {
                    self.gk[g].think = sk.goalie_think;
                    let p = self.puck;
                    let mut y = CY + (p.y - CY) * 3 / 4;
                    // Coming this way: stand in its line.
                    if sk.predict && p.owner == Owner::Loose && p.vx * dir < -64 {
                        let t = (gx - p.x) / p.vx;
                        y = fold(p.y + p.vy * t);
                    }
                    self.gk[g].target = y.clamp(CY - GOALIE_RANGE, CY + GOALIE_RANGE);
                }
                self.gk[g].think -= 1;
                if matches!(self.phase, Phase::FaceOff(_)) {
                    self.gk[g].target = CY;
                }
                let d = (self.gk[g].target - self.gk[g].y).clamp(-sk.goalie_v, sk.goalie_v);
                self.gk[g].y += d;
                continue;
            }
            // Holding: the puck sits in front; pass it out when the time
            // is up.
            self.puck.x = gx + dir * 5 * PX;
            self.puck.y = self.gk[g].y;
            self.puck.vx = 0;
            self.puck.vy = 0;
            self.gk[g].hold = self.gk[g].hold.saturating_sub(1);
            if self.gk[g].hold == 0 {
                let mate = (g * 2..g * 2 + 2)
                    .min_by_key(|&i| len(self.sk[i].x - self.puck.x, self.sk[i].y - self.puck.y))
                    .unwrap_or(g * 2);
                let t = self.sk[mate];
                let (mut dx, dy) = (t.x - self.puck.x, t.y - self.puck.y);
                if dx * dir < dy.abs() / 2 {
                    dx = dir * (dy.abs() / 2).max(PX);
                }
                let (vx, vy) = norm(dx, dy, 520);
                self.puck.vx = vx;
                self.puck.vy = vy;
                self.puck.owner = Owner::Loose;
                self.puck.ghost = 12;
                self.sk[mate].no_pick = 0;
            }
        }
    }

    /// The puck: on a blade, in a goalie's glove, or sliding.
    fn move_puck(&mut self) {
        match self.puck.owner {
            Owner::Goalie(_) => {}
            Owner::Skater(i) => {
                let i = usize::from(i);
                let s = self.sk[i];
                self.sk[i].carry = s.carry.saturating_add(1);
                let (bx, by) = s.blade();
                let p = &mut self.puck;
                p.x = bx;
                p.y = by;
                p.vx = s.vx;
                p.vy = s.vy;
                keep_in(&mut p.x, &mut p.y, &mut p.vx, &mut p.vy, PUCK_R, None);
                // Carried into a goalie: it covers the puck.
                for g in 0..2 {
                    if self.hits_goalie(g) {
                        self.cover(g);
                        self.sk[i].no_pick = 30;
                        break;
                    }
                }
            }
            Owner::Loose => self.puck_steps(true),
        }
    }

    fn hits_goalie(&self, g: usize) -> bool {
        let p = self.puck;
        (p.x - goalie_x(g)).abs() < GOALIE_HW + PUCK_R
            && (p.y - self.gk[g].y).abs() < GOALIE_HH + PUCK_R
    }

    fn cover(&mut self, g: usize) {
        self.puck.owner = Owner::Goalie(g as u8);
        self.puck.vx = 0;
        self.puck.vy = 0;
        self.gk[g].hold = HOLD_TICKS;
        self.event(Ev::Save { team: g as u8 });
    }

    /// A sliding puck, in steps of under a pixel so nothing is skipped.
    /// `live`: goals, saves and pick-ups count.
    fn puck_steps(&mut self, live: bool) {
        let p = self.puck;
        let n = (p.vx.abs().max(p.vy.abs()) / 160 + 1).min(8);
        let (sx, sy) = (p.vx / n, p.vy / n);
        for k in 0..n {
            let p = &mut self.puck;
            p.x += if k == n - 1 { p.vx - sx * (n - 1) } else { sx };
            p.y += if k == n - 1 { p.vy - sy * (n - 1) } else { sy };
            if let Some(team) = puck_walls(p)
                && live
                && self.phase == Phase::Live
            {
                self.score[team] = self.score[team].saturating_add(1);
                self.phase = Phase::Goal {
                    team: team as u8,
                    t: GOAL_TICKS,
                };
                self.event(Ev::Goal { team: team as u8 });
            }
            if !live || self.phase != Phase::Live {
                continue;
            }
            if self.puck.ghost == 0 {
                for g in 0..2 {
                    if self.hits_goalie(g) {
                        self.save(g);
                        break;
                    }
                }
            }
            if self.puck.owner != Owner::Loose {
                break;
            }
            if self.pick_up() {
                break;
            }
        }
        let p = &mut self.puck;
        p.ghost = p.ghost.saturating_sub(1);
        if p.owner == Owner::Loose {
            p.vx = scale(p.vx, PUCK_KEEP);
            p.vy = scale(p.vy, PUCK_KEEP);
            if len(p.vx, p.vy) < 6 {
                p.vx = 0;
                p.vy = 0;
            }
        }
    }

    /// The puck hit goalie `g`: covered when slow, knocked away when fast.
    fn save(&mut self, g: usize) {
        let v = len(self.puck.vx, self.puck.vy);
        if v < COVER_V {
            self.cover(g);
            return;
        }
        let dir = attack(g);
        let gy = self.gk[g].y;
        let p = &mut self.puck;
        p.vx = dir * p.vx.abs() / 2;
        p.vy = p.vy / 2 + (p.y - gy) / 6;
        p.x = goalie_x(g) + dir * (GOALIE_HW + PUCK_R + PX / 2);
        self.event(Ev::Save { team: g as u8 });
    }

    /// A skater near a sliding puck takes it. Who is asked first changes
    /// at random, so a tie is fair. True when someone took it.
    fn pick_up(&mut self) -> bool {
        let p = self.puck;
        let fast = len(p.vx, p.vy) > 600;
        let reach = if fast { 3 * PX } else { 5 * PX };
        let first = self.rng.below(4) as usize;
        for k in 0..4 {
            let i = (first + k) % 4;
            let s = self.sk[i];
            if s.no_pick > 0 || s.stun > 0 {
                continue;
            }
            let (bx, by) = s.blade();
            if len(p.x - bx, p.y - by) <= reach || len(p.x - s.x, p.y - s.y) <= reach {
                self.puck.owner = Owner::Skater(i as u8);
                self.puck.vx = 0;
                self.puck.vy = 0;
                self.sk[i].carry = 0;
                self.event(Ev::Take { by: i as u8 });
                return true;
            }
        }
        false
    }
}

/// A y folded back into the ice once (a puck's line off a board).
fn fold(y: i32) -> i32 {
    if y < YT {
        2 * YT - y
    } else if y > YB {
        2 * YB - y
    } else {
        y
    }
    .clamp(YT, YB)
}

/// The puck against the boards, the corners and into the nets. Returns
/// the team that scored when it crossed a goal line.
fn puck_walls(p: &mut Puck) -> Option<usize> {
    let in_mouth = (p.y - CY).abs() <= MOUTH - PUCK_R;
    let at_end = p.x < XL + PUCK_R || p.x > XR - PUCK_R;
    let in_net = p.x < XL || p.x > XR;
    if !(at_end && (in_mouth || in_net)) {
        keep_in(
            &mut p.x,
            &mut p.y,
            &mut p.vx,
            &mut p.vy,
            PUCK_R,
            Some(BOUNCE),
        );
        return None;
    }
    // In a goal mouth, or already in the net: the net holds it.
    let team = if p.x < CX { 0 } else { 1 };
    let (line, side) = (goal_x(team), attack(team));
    let depth = (p.x - line) * side; // how far onto the ice
    let scored = (depth < -PX / 2).then_some(1 - team);
    if depth < -(NET_DEPTH - PUCK_R) {
        p.x = line - side * (NET_DEPTH - PUCK_R);
        p.vx = -p.vx / 4;
    }
    if depth < 0 && (p.y - CY).abs() > MOUTH - PUCK_R {
        p.y = CY + (p.y - CY).signum() * (MOUTH - PUCK_R);
        p.vy = -p.vy / 2;
    }
    scored
}

/// Keeps a round thing of radius `r` on the ice: the side boards, the
/// end boards and the cut corners. `bounce`: it bounces back with this
/// much of its speed (of 256); `None`: it stops against the board.
pub fn keep_in(x: &mut i32, y: &mut i32, vx: &mut i32, vy: &mut i32, r: i32, bounce: Option<i32>) {
    let hit = |v: &mut i32, away: i32| {
        // `away`: the sign that leaves the board.
        if *v * away < 0 {
            *v = match bounce {
                Some(b) => -scale(*v, b),
                None => 0,
            };
        }
    };
    if *x < XL + r {
        *x = XL + r;
        hit(vx, 1);
    }
    if *x > XR - r {
        *x = XR - r;
        hit(vx, -1);
    }
    if *y < YT + r {
        *y = YT + r;
        hit(vy, 1);
    }
    if *y > YB - r {
        *y = YB - r;
        hit(vy, -1);
    }
    // The corners: the line a + b = CUT in each corner's own frame
    // (a, b measured into the ice from the two boards).
    let need = CUT + r * 362 / 256; // r times the square root of 2
    for (sa, sb) in [(1, 1), (-1, 1), (1, -1), (-1, -1)] {
        let a = if sa > 0 { *x - XL } else { XR - *x };
        let b = if sb > 0 { *y - YT } else { YB - *y };
        if a + b >= need {
            continue;
        }
        let d = need - (a + b);
        *x += sa * (d / 2 + 1);
        *y += sb * (d / 2 + 1);
        let (va, vb) = (sa * *vx, sb * *vy);
        if va + vb < 0 {
            let (na, nb) = match bounce {
                Some(bv) => (-scale(vb, bv), -scale(va, bv)),
                None => {
                    let n = (va + vb) / 2;
                    (va - n, vb - n)
                }
            };
            *vx = sa * na;
            *vy = sb * nb;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> [Intent; 4] {
        [Intent::default(); 4]
    }

    #[test]
    fn the_rink_is_symmetric() {
        assert_eq!(CX - XL, XR - CX);
        assert_eq!(goalie_x(0) - XL, XR - goalie_x(1));
        const { assert!(CY - MOUTH > YT + CUT) };
    }

    #[test]
    fn a_slow_puck_stops_and_a_fast_one_stays_on_the_ice() {
        let mut p = Puck {
            x: CX,
            y: CY - 30 * PX,
            vx: 300,
            vy: -900,
            ..Puck::default()
        };
        for _ in 0..2000 {
            let n = (p.vx.abs().max(p.vy.abs()) / 160 + 1).min(8);
            for _ in 0..n {
                p.x += p.vx / n;
                p.y += p.vy / n;
                assert_eq!(puck_walls(&mut p), None);
            }
            p.vx = scale(p.vx, PUCK_KEEP);
            p.vy = scale(p.vy, PUCK_KEEP);
            assert!((XL..=XR).contains(&p.x) && (YT..=YB).contains(&p.y));
        }
        assert_eq!((p.vx, p.vy), (0, 0));
    }

    #[test]
    fn a_puck_through_the_mouth_is_a_goal_for_the_other_team() {
        let mut p = Puck {
            x: XL + 2 * PX,
            y: CY,
            vx: -300,
            ..Puck::default()
        };
        let mut scored = None;
        for _ in 0..10 {
            p.x += p.vx;
            scored = scored.or(puck_walls(&mut p));
        }
        assert_eq!(scored, Some(1));
        assert!(p.x >= XL - NET_DEPTH, "the net holds it");
        // Beside the mouth it bounces.
        let mut p = Puck {
            x: XR - 2 * PX,
            y: CY + MOUTH + 4 * PX,
            vx: 300,
            ..Puck::default()
        };
        p.x += p.vx;
        assert_eq!(puck_walls(&mut p), None);
        assert!(p.vx < 0);
    }

    #[test]
    fn the_puck_drops_after_the_face_off_and_someone_takes_it() {
        let mut m = Match::new(Length::Min2, Level::Hard, true, 7);
        for _ in 0..FACEOFF_TICKS {
            assert!(matches!(m.phase, Phase::FaceOff(_)));
            m.tick(&idle());
        }
        assert_eq!(m.phase, Phase::Live);
        m.tick(&idle());
        assert!(m.carrier().is_some_and(|i| i == 0 || i == 2));
    }

    #[test]
    fn the_clock_runs_out_into_overtime_when_level() {
        let mut m = Match::new(Length::Min2, Level::Easy, false, 1);
        m.phase = Phase::Live;
        m.played = 2 * 60 * 60 - 1;
        m.tick(&idle());
        assert!(m.overtime);
        assert_eq!(m.phase, Phase::Live);
        // A goal in overtime ends it.
        m.score[1] = 1;
        m.phase = Phase::Goal { team: 1, t: 1 };
        m.tick(&idle());
        assert!(matches!(m.phase, Phase::Whistle(_)));
        assert_eq!(m.winner(), Some(1));
        assert!(!m.draw());
    }

    #[test]
    fn overtime_counts_down_and_ends_level_as_a_draw_exactly_at_the_cap() {
        let limit = 2 * 60 * 60;
        let mut m = Match::new(Length::Min2, Level::Easy, false, 1);
        m.phase = Phase::Live;
        m.played = limit - 1;
        m.tick(&idle());
        assert!(m.overtime);
        assert_eq!(m.clock(), OT_TICKS, "the overtime clock starts full");
        // Hold the play: the puck kept by nobody near the middle, the
        // skaters idle, so nobody can score.
        let mut ticks = 0;
        while m.phase == Phase::Live {
            m.puck = Puck {
                x: CX,
                y: CY,
                ..Puck::default()
            };
            m.tick(&idle());
            ticks += 1;
            assert!(ticks <= OT_TICKS, "overtime ran past its cap");
            if m.phase == Phase::Live {
                assert_eq!(m.clock(), OT_TICKS - ticks, "the clock counts down");
            }
        }
        assert_eq!(ticks, OT_TICKS, "the whistle comes exactly at the cap");
        assert_eq!(m.played, limit + OT_TICKS);
        assert!(matches!(m.phase, Phase::Whistle(_)), "{:?}", m.phase);
        assert_eq!(m.score, [0, 0]);
        assert_eq!(m.winner(), None);
        assert!(m.draw());
        for _ in 0..WHISTLE_TICKS {
            m.tick(&idle());
        }
        assert_eq!(m.phase, Phase::Over);
        assert!(m.draw());
    }

    #[test]
    fn a_goal_in_the_last_tick_of_overtime_still_wins() {
        let limit = 3 * 60 * 60;
        let mut m = Match::new(Length::Min3, Level::Hard, false, 2);
        m.overtime = true;
        m.phase = Phase::Live;
        m.played = limit + OT_TICKS - 1;
        // The goal phase stops the clock; the goal is decided before the
        // cap is looked at again.
        m.score[0] = 1;
        m.phase = Phase::Goal { team: 0, t: 1 };
        m.tick(&idle());
        assert!(matches!(m.phase, Phase::Whistle(_)));
        assert_eq!(m.winner(), Some(0));
        assert!(!m.draw());
    }

    #[test]
    fn first_to_five_has_no_overtime_and_cannot_end_level() {
        let mut m = Match::new(Length::First5, Level::Hard, false, 4);
        m.phase = Phase::Live;
        m.played = 60 * 60 * 60;
        m.tick(&idle());
        assert!(!m.overtime);
        assert_eq!(m.phase, Phase::Live, "no clock ends a FIRST TO 5 match");
        // Only a fifth goal ends it, and then one side has 5, the other less.
        m.score = [4, 4];
        m.phase = Phase::Goal { team: 1, t: 1 };
        m.tick(&idle());
        assert!(matches!(m.phase, Phase::FaceOff(_)), "4-4 plays on");
        m.score = [4, 5];
        m.phase = Phase::Goal { team: 1, t: 1 };
        m.tick(&idle());
        assert!(matches!(m.phase, Phase::Whistle(_)));
        assert_eq!(m.winner(), Some(1));
        assert!(!m.draw());
    }

    #[test]
    fn skaters_never_leave_the_ice() {
        let mut m = Match::new(Length::Min2, Level::Hard, false, 3);
        m.phase = Phase::Live;
        let ways = [
            (256, 0),
            (-256, 0),
            (0, 256),
            (0, -256),
            (181, 181),
            (-181, -181),
        ];
        for (k, &(dx, dy)) in ways.iter().enumerate() {
            for _ in 0..300 {
                let it = Intent {
                    dx,
                    dy,
                    act: k % 2 == 0,
                    ..Intent::default()
                };
                m.tick(&[it; 4]);
                for s in &m.sk {
                    assert!((XL + SKATER_R..=XR - SKATER_R).contains(&s.x), "{s:?}");
                    assert!((YT + SKATER_R..=YB - SKATER_R).contains(&s.y), "{s:?}");
                }
            }
        }
    }
}
