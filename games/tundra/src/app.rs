//! The screens (title, match, pause, result), the menus, and who drives
//! which skater: a pad, the touch screen or the computer.
//!
//! Players and skaters: player 1 and 3 play for AMBER (left), player 2
//! and 4 for BLUE (right). Player `p` starts on skater [`HOME`]`[p]`;
//! the computer drives every skater no person holds. A person alone on
//! a team can switch to the team mate (X or Y), and follows their own
//! pass to it.
//!
//! The right team's player 2 is chosen on the title ([`Right`]): AUTO
//! (the default) gives it to pad 2 when one is connected, otherwise to
//! the touch screen once a finger touches the ice during a match, and to
//! the computer until then.
//!
//! Time: the match moves in ticks of 1/60 s; each call runs as many as
//! the time since the last call holds (at most [`MAX_TICKS`]), so 30 and
//! 60 frames a second play at the same speed.

use game_kit::{Touch, button};

use crate::cpu;
use crate::fixed::{PX, UNIT, len, norm};
use crate::play::{BAR_H, CY, Ev, Intent, Length, Level, Match, Phase, goal_x};

/// One physics tick, in microseconds.
pub const TICK_US: u32 = 16_667;
/// At most this many ticks in one call (a long wait is not caught up).
pub const MAX_TICKS: u32 = 6;
/// Player `p` starts on this skater (skater `i` plays for team `i / 2`).
pub const HOME: [usize; 4] = [0, 2, 1, 3];
/// A stick pushed less than this counts as let go.
pub const DEAD_ZONE: i32 = 9_000;
/// A touch that comes up within this many ticks, having moved less than
/// [`TAP_MOVE`] pixels, is a tap.
pub const TAP_TICKS: u16 = 15;
pub const TAP_MOVE: i32 = 8;

// ---- Layout shared by drawing and touch -------------------------------

/// The title menu: line `k` at `TITLE_Y + k * LINE`.
pub const TITLE_Y: i16 = 34;
pub const LINE: i16 = 9;
pub const TITLE_LINES: u8 = 7;
/// The pause panel and its lines.
pub const PAUSE_BOX: (i16, i16, u16, u16) = (26, 22, 108, 80);
pub const PAUSE_Y: i16 = 38;
pub const PAUSE_LINES: u8 = 6;
/// The result panel and its two lines.
pub const END_BOX: (i16, i16, u16, u16) = (22, 26, 116, 76);
pub const END_Y: i16 = 80;

/// The menu line a touch at `y` is on, for lines from `top`.
pub fn line_at(y: i16, top: i16, lines: u8) -> Option<u8> {
    let k = (y - top + 2).div_euclid(LINE);
    (0..i16::from(lines)).contains(&k).then_some(k as u8)
}

// ---- Input --------------------------------------------------------------

/// One player's pad in one call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PadIn {
    pub connected: bool,
    pub held: u32,
    pub pressed: u32,
    pub stick: (i16, i16),
}

/// Everything the game reads in one call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    pub pads: [PadIn; 4],
    pub touch: Option<Touch>,
    pub delta_us: u32,
    /// The console's call count (seeds each match).
    pub frame: u32,
}

impl Keys {
    /// All connected pads' presses together (for the menus).
    fn pressed(&self) -> u32 {
        self.pads.iter().fold(0, |b, p| b | p.pressed)
    }

    /// A touch that came down in this call.
    fn tap_start(&self) -> Option<(i16, i16)> {
        self.touch.filter(|t| t.started).map(|t| (t.x, t.y))
    }
}

// ---- Settings ------------------------------------------------------------

/// Who drives the right team's first skater (player 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Right {
    Auto,
    Computer,
    Touch,
    Pad,
}

impl Right {
    pub const ALL: [Right; 4] = [Right::Auto, Right::Computer, Right::Touch, Right::Pad];

    pub fn name(self) -> &'static str {
        match self {
            Right::Auto => "AUTO",
            Right::Computer => "COMPUTER",
            Right::Touch => "TOUCH",
            Right::Pad => "PAD 2",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opts {
    pub right: Right,
    pub level: Level,
    pub length: Length,
    /// 60 frames a second instead of 30 (60 at the start).
    pub fast: bool,
    pub marks: bool,
}

impl Opts {
    pub const DEFAULT: Opts = Opts {
        right: Right::Auto,
        level: Level::Easy,
        length: Length::Min3,
        fast: true,
        marks: true,
    };
}

/// Who drives a skater now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ctl {
    Cpu,
    /// Player `n`'s pad (0 to 3).
    Pad(u8),
    /// The touch screen, for player 2.
    Touch,
}

impl Ctl {
    /// The player number shown on the skater (1 to 4), or none.
    pub fn player(self) -> Option<u8> {
        match self {
            Ctl::Cpu => None,
            Ctl::Pad(n) => Some(n + 1),
            Ctl::Touch => Some(2),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Title,
    Play,
    End,
}

/// A finger on the screen, followed while it is down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Finger {
    pub x: i16,
    pub y: i16,
    start: (i16, i16),
    /// Ticks since it came down.
    age: u16,
    pub down: bool,
    /// It came up as a tap in this call.
    tap: bool,
}

/// What happened, for the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Log {
    Match {
        who: [Ctl; 4],
        level: Level,
        length: Length,
        fast: bool,
    },
    Goal {
        team: u8,
        score: [u8; 2],
        clock: u32,
    },
    Final {
        score: [u8; 2],
        series: [u8; 2],
    },
    TouchPlays,
    Pause,
    Resume,
    Leave,
}

#[derive(Clone)]
pub struct App {
    pub scene: Scene,
    pub opts: Opts,
    /// The title's chosen line.
    pub sel: u8,
    /// The pause menu's chosen line, while it is open.
    pub pause: Option<u8>,
    pub end_sel: u8,
    pub m: Match,
    /// Who drives each skater now.
    pub ctl: [Ctl; 4],
    /// The skater each player holds.
    pub at: [usize; 4],
    /// The touch screen has taken player 2 (AUTO), until the title.
    pub touch_claimed: bool,
    pub finger: Option<Finger>,
    /// Matches won in a row of rematches.
    pub series: [u8; 2],
    /// Which pads were connected in the last call (for the team sheet).
    pub pads_seen: [bool; 4],
    /// Time not yet turned into ticks.
    acc: u32,
    log: [Option<Log>; 8],
}

impl App {
    pub const fn new() -> App {
        App {
            scene: Scene::Title,
            opts: Opts::DEFAULT,
            sel: 0,
            pause: None,
            end_sel: 0,
            m: Match::blank(),
            ctl: [Ctl::Cpu; 4],
            at: HOME,
            touch_claimed: false,
            finger: None,
            series: [0; 2],
            pads_seen: [false; 4],
            acc: 0,
            log: [None; 8],
        }
    }

    /// The next line for the log, oldest first.
    pub fn take_log(&mut self) -> Option<Log> {
        let l = self.log[0].take();
        self.log.rotate_left(1);
        l
    }

    fn note(&mut self, l: Log) {
        if let Some(slot) = self.log.iter_mut().find(|s| s.is_none()) {
            *slot = Some(l);
        }
    }

    /// Nothing moves on its own: the console may wait for input.
    pub fn still(&self) -> bool {
        self.scene != Scene::Play || self.pause.is_some()
    }

    /// The driver of each player (not yet placed on skaters).
    pub fn players(&self, keys: &Keys) -> [Ctl; 4] {
        let pad = |n: usize| {
            if keys.pads[n].connected {
                Ctl::Pad(n as u8)
            } else {
                Ctl::Cpu
            }
        };
        let p2 = match self.opts.right {
            Right::Computer => Ctl::Cpu,
            Right::Touch => Ctl::Touch,
            Right::Pad => pad(1),
            Right::Auto if keys.pads[1].connected => Ctl::Pad(1),
            Right::Auto if self.touch_claimed => Ctl::Touch,
            Right::Auto => Ctl::Cpu,
        };
        [pad(0), p2, pad(2), pad(3)]
    }

    /// Places the players on skaters.
    fn place(&mut self, keys: &Keys) {
        let who = self.players(keys);
        self.ctl = [Ctl::Cpu; 4];
        for team in 0..2 {
            let (a, b) = (team, team + 2); // the team's two players
            let both = who[a] != Ctl::Cpu && who[b] != Ctl::Cpu;
            for p in [a, b] {
                if both || who[p] == Ctl::Cpu || self.at[p] / 2 != team {
                    self.at[p] = HOME[p];
                }
            }
            for p in [a, b] {
                if who[p] != Ctl::Cpu {
                    self.ctl[self.at[p]] = who[p];
                }
            }
        }
    }

    /// One call. False: leave the game.
    pub fn update(&mut self, keys: &Keys) -> bool {
        self.pads_seen = keys.pads.map(|p| p.connected);
        match self.scene {
            Scene::Title => self.title(keys),
            Scene::Play if self.pause.is_some() => self.paused(keys),
            Scene::Play => {
                self.play(keys);
                true
            }
            Scene::End => {
                self.end(keys);
                true
            }
        }
    }

    // ---- Title -------------------------------------------------------------

    fn title(&mut self, keys: &Keys) -> bool {
        let pressed = keys.pressed();
        let mut go = None;
        if pressed & button::UP != 0 {
            self.sel = (self.sel + TITLE_LINES - 1) % TITLE_LINES;
        }
        if pressed & button::DOWN != 0 {
            self.sel = (self.sel + 1) % TITLE_LINES;
        }
        if pressed & button::LEFT != 0 {
            go = Some(-1);
        }
        if pressed & (button::RIGHT | button::A | button::START) != 0 {
            go = Some(1);
        }
        if let Some((x, y)) = keys.tap_start()
            && let Some(k) = line_at(y, TITLE_Y, TITLE_LINES)
        {
            self.sel = k;
            // PLAY and LEAVE act on any tap; a value steps back on the
            // left half.
            go = Some(if x < 80 && k != 0 && k != 6 { -1 } else { 1 });
        }
        let Some(step) = go else {
            return true;
        };
        let o = &mut self.opts;
        match self.sel {
            0 if step > 0 => self.start_match(keys),
            1 => o.right = cycle(&Right::ALL, o.right, step),
            2 => o.level = other_level(o.level),
            3 => o.length = cycle(&Length::ALL, o.length, step),
            4 => o.fast = !o.fast,
            5 => o.marks = !o.marks,
            6 if step > 0 => {
                self.note(Log::Leave);
                return false;
            }
            _ => {}
        }
        true
    }

    fn start_match(&mut self, keys: &Keys) {
        let seed = keys.frame.wrapping_mul(0x9E37_79B9) ^ 0x7154_D2A3;
        self.m = Match::new(self.opts.length, self.opts.level, self.opts.marks, seed);
        self.scene = Scene::Play;
        self.pause = None;
        self.finger = None;
        self.acc = 0;
        self.at = HOME;
        self.place(keys);
        let who = self.players(keys);
        self.note(Log::Match {
            who,
            level: self.opts.level,
            length: self.opts.length,
            fast: self.opts.fast,
        });
    }

    fn back_to_title(&mut self) {
        self.scene = Scene::Title;
        self.pause = None;
        self.sel = 0;
        self.touch_claimed = false;
        self.finger = None;
        self.series = [0; 2];
    }

    // ---- The match ------------------------------------------------------------

    fn play(&mut self, keys: &Keys) {
        // The bar on top, touched: pause. START on any pad: pause.
        if let Some((_, y)) = keys.tap_start()
            && y < BAR_H as i16
        {
            self.open_pause();
            return;
        }
        if keys.pressed() & button::START != 0 {
            self.open_pause();
            return;
        }
        // A finger on the ice takes player 2 (AUTO, no pad 2).
        if let Some(t) = keys.touch
            && t.started
            && self.opts.right == Right::Auto
            && !keys.pads[1].connected
            && !self.touch_claimed
        {
            self.touch_claimed = true;
            self.note(Log::TouchPlays);
        }
        self.follow_finger(keys);
        self.place(keys);

        self.acc = self.acc.saturating_add(keys.delta_us);
        let n = (self.acc / TICK_US).min(MAX_TICKS);
        self.acc = if n == MAX_TICKS {
            0
        } else {
            self.acc - n * TICK_US
        };
        for k in 0..n {
            let first = k == 0;
            if first {
                self.switches(keys);
            }
            let mut its = [Intent::default(); 4];
            for (i, it) in its.iter_mut().enumerate() {
                *it = match self.ctl[i] {
                    Ctl::Cpu => cpu::intent(&mut self.m, i),
                    Ctl::Pad(p) => self.pad_intent(i, &keys.pads[usize::from(p)], first),
                    Ctl::Touch => self.touch_intent(i, first),
                };
            }
            self.m.tick(&its);
            self.events();
            if let Some(f) = &mut self.finger {
                f.age = f.age.saturating_add(1);
            }
            if self.m.phase == Phase::Over {
                self.finish();
                break;
            }
        }
        if self.finger.is_some_and(|f| !f.down) {
            self.finger = None;
        }
    }

    /// Follows the finger: a new one when it comes down, a tap when it
    /// comes up soon and close.
    fn follow_finger(&mut self, keys: &Keys) {
        let Some(t) = keys.touch else {
            if let Some(f) = &mut self.finger {
                f.down = false;
            }
            return;
        };
        if t.started {
            self.finger = Some(Finger {
                x: t.x,
                y: t.y,
                start: (t.x, t.y),
                age: 0,
                down: true,
                tap: false,
            });
        }
        if let Some(f) = &mut self.finger {
            f.x = t.x;
            f.y = t.y;
            if t.ended || !t.down {
                let moved = len(i32::from(t.x - f.start.0), i32::from(t.y - f.start.1));
                f.down = false;
                f.tap = f.age <= TAP_TICKS && moved < TAP_MOVE;
            }
        }
    }

    /// X or Y: a person alone on a team takes the other skater.
    fn switches(&mut self, keys: &Keys) {
        for p in 0..4 {
            let Ctl::Pad(n) = self.players(keys)[p] else {
                continue;
            };
            if keys.pads[usize::from(n)].pressed & (button::X | button::Y) == 0 {
                continue;
            }
            let mate = self.at[p] ^ 1;
            if self.ctl[mate] == Ctl::Cpu {
                self.ctl[mate] = self.ctl[self.at[p]];
                self.ctl[self.at[p]] = Ctl::Cpu;
                self.at[p] = mate;
            }
        }
    }

    /// A person's pad on skater `i`.
    fn pad_intent(&self, i: usize, pad: &PadIn, first: bool) -> Intent {
        let (sx, sy) = (i32::from(pad.stick.0), i32::from(pad.stick.1));
        let (dx, dy) = if sx.abs() > DEAD_ZONE || sy.abs() > DEAD_ZONE {
            let (x, y) = (sx * UNIT / 32_767, sy * UNIT / 32_767);
            crate::fixed::clamp_len(x, y, UNIT)
        } else {
            let on = |b| i32::from(pad.held & b != 0);
            let x = on(button::RIGHT) - on(button::LEFT);
            let y = on(button::DOWN) - on(button::UP);
            norm(x, y, UNIT)
        };
        Intent {
            dx,
            dy,
            charge: pad.held & (button::A | button::R) != 0,
            act: first && pad.pressed & button::B != 0 && self.m.carrier() != Some(i),
            ..Intent::default()
        }
    }

    /// The touch screen on skater `i`: skate to the finger; a tap shoots
    /// there (with the puck) or checks (without).
    fn touch_intent(&self, i: usize, first: bool) -> Intent {
        let Some(f) = self.finger else {
            return Intent::default();
        };
        let s = self.m.sk[i];
        let (fx, fy) = (i32::from(f.x) * PX + PX / 2, i32::from(f.y) * PX + PX / 2);
        let (dx, dy) = (fx - s.x, fy - s.y);
        let d = len(dx, dy);
        let mut it = Intent::default();
        if f.down {
            let speed = if d > 6 * PX {
                UNIT
            } else {
                UNIT * d / (6 * PX)
            };
            (it.dx, it.dy) = norm(dx, dy, speed);
        }
        if f.tap && first {
            if self.m.carrier() == Some(i) {
                let t = i / 2;
                let aim = if d > 12 * PX {
                    (dx, dy)
                } else {
                    (goal_x(1 - t) - s.x, CY - s.y)
                };
                it.shot = Some((aim.0, aim.1, 820));
            } else {
                (it.dx, it.dy) = norm(dx, dy, UNIT);
                it.act = true;
            }
        }
        it
    }

    /// Reacts to what the match did: control follows a pass, goals and
    /// the end are logged.
    fn events(&mut self) {
        while let Some(e) = self.m.take_event() {
            match e {
                Ev::Pass { from, to } => {
                    let (from, to) = (usize::from(from), usize::from(to));
                    if self.ctl[from] != Ctl::Cpu && self.ctl[to] == Ctl::Cpu {
                        for p in 0..4 {
                            if self.at[p] == from && self.ctl[from].player() == Some(p as u8 + 1) {
                                self.at[p] = to;
                            }
                        }
                        self.ctl[to] = self.ctl[from];
                        self.ctl[from] = Ctl::Cpu;
                    }
                }
                Ev::Goal { team } => {
                    let clock = self.m.clock();
                    self.note(Log::Goal {
                        team,
                        score: self.m.score,
                        clock,
                    });
                }
                _ => {}
            }
        }
    }

    /// The match is over: the winner's series goes up (a draw counts for
    /// nobody), the result screen shows.
    fn finish(&mut self) {
        if let Some(w) = self.m.winner() {
            self.series[w] = self.series[w].saturating_add(1);
        }
        self.scene = Scene::End;
        self.end_sel = 0;
        self.finger = None;
        self.note(Log::Final {
            score: self.m.score,
            series: self.series,
        });
    }

    // ---- Pause -------------------------------------------------------------------

    fn open_pause(&mut self) {
        self.pause = Some(0);
        self.finger = None;
        self.note(Log::Pause);
    }

    fn resume(&mut self) {
        self.pause = None;
        self.acc = 0;
        self.note(Log::Resume);
    }

    fn paused(&mut self, keys: &Keys) -> bool {
        let pressed = keys.pressed();
        let mut sel = self.pause.unwrap_or(0);
        if pressed & (button::START | button::B) != 0 {
            self.resume();
            return true;
        }
        if pressed & button::UP != 0 {
            sel = (sel + PAUSE_LINES - 1) % PAUSE_LINES;
        }
        if pressed & button::DOWN != 0 {
            sel = (sel + 1) % PAUSE_LINES;
        }
        let mut go = None;
        if pressed & button::LEFT != 0 {
            go = Some(-1);
        }
        if pressed & (button::RIGHT | button::A) != 0 {
            go = Some(1);
        }
        if let Some((x, y)) = keys.tap_start() {
            match line_at(y, PAUSE_Y, PAUSE_LINES) {
                Some(k) if (PAUSE_BOX.0..PAUSE_BOX.0 + PAUSE_BOX.2 as i16).contains(&x) => {
                    sel = k;
                    go = Some(if x < 80 && (k == 1 || k == 2) { -1 } else { 1 });
                }
                // A touch outside the panel goes back to the match.
                _ if !in_box(x, y, PAUSE_BOX) => {
                    self.resume();
                    return true;
                }
                _ => {}
            }
        }
        self.pause = Some(sel);
        let Some(step) = go else {
            return true;
        };
        match sel {
            0 => self.resume(),
            1 => {
                self.opts.level = other_level(self.opts.level);
                self.m.level = self.opts.level;
            }
            2 => self.opts.fast = !self.opts.fast,
            3 if step > 0 => self.start_match(keys),
            4 if step > 0 => self.back_to_title(),
            5 if step > 0 => {
                self.note(Log::Leave);
                return false;
            }
            _ => {}
        }
        true
    }

    // ---- Result -------------------------------------------------------------------

    fn end(&mut self, keys: &Keys) {
        let pressed = keys.pressed();
        if pressed & (button::UP | button::DOWN) != 0 {
            self.end_sel ^= 1;
        }
        let mut go = pressed & (button::A | button::START) != 0;
        if let Some((_, y)) = keys.tap_start()
            && let Some(k) = line_at(y, END_Y, 2)
        {
            self.end_sel = k;
            go = true;
        }
        if go {
            if self.end_sel == 0 {
                self.start_match(keys);
            } else {
                self.back_to_title();
            }
        }
    }
}

impl Default for App {
    fn default() -> Self {
        App::new()
    }
}

fn in_box(x: i16, y: i16, b: (i16, i16, u16, u16)) -> bool {
    (b.0..b.0 + b.2 as i16).contains(&x) && (b.1..b.1 + b.3 as i16).contains(&y)
}

fn other_level(l: Level) -> Level {
    match l {
        Level::Easy => Level::Hard,
        Level::Hard => Level::Easy,
    }
}

/// The value after (or before) `now` in `all`, round.
fn cycle<T: Copy + PartialEq>(all: &[T], now: T, step: i32) -> T {
    let n = all.len();
    let k = all.iter().position(|&v| v == now).unwrap_or(0);
    let k = if step < 0 {
        (k + n - 1) % n
    } else {
        (k + 1) % n
    };
    all[k]
}
