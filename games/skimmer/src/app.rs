//! The screens (title, match, pause, result), the menus, and who drives
//! which mallet: a pad, the touch screen or the computer.
//!
//! Sides: LEFT is CORAL (round mallets), RIGHT is TEAL (eight-sided
//! mallets). The title says who holds each side ([`Who`]):
//!
//! * LEFT AUTO (the default): pad 1 when one is connected, otherwise the
//!   touch screen. If pad 1 drops out while a finger already holds the
//!   right side, the finger keeps the right side and the computer plays
//!   the left until pad 1 comes back.
//! * RIGHT AUTO (the default): pad 2 when one is connected; otherwise
//!   the computer, until a finger comes down on the right half during a
//!   match (when the left side is not already on the touch screen): then
//!   the touch screen plays the right side for the rest of the match.
//!
//! The panel reads one finger only, so at most one side is on touch. In
//! 2 AGAINST 2 the second mallets are pads 3 (left) and 4 (right), or
//! the computer.
//!
//! Time: the match moves in ticks of 1/60 s; each call runs as many as
//! the time since the last call holds (at most [`MAX_TICKS`]), so 30 and
//! 60 frames a second play the same.

use game_kit::{Touch, button};

use crate::cpu;
use crate::fixed::{PX, UNIT, norm};
use crate::table::{BAR_H, CX, Ev, Intent, Length, Level, Match, Phase, Rules, TOUCH_V, YB, YT};

/// One physics tick, in microseconds.
pub const TICK_US: u32 = 16_667;
/// At most this many ticks in one call (a long wait is not caught up).
pub const MAX_TICKS: u32 = 6;
/// A stick pushed less than this counts as let go.
pub const DEAD_ZONE: i32 = 9_000;

// ---- Layout shared by drawing and touch -------------------------------

/// The title menu: line `k` at `TITLE_Y + k * LINE`.
pub const TITLE_Y: i16 = 31;
pub const LINE: i16 = 9;
pub const TITLE_LINES: u8 = 9;
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

/// Who holds a side, as chosen on the title.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    Auto,
    Pad1,
    Pad2,
    Touch,
    Computer,
}

impl Who {
    pub const ALL: [Who; 5] = [Who::Auto, Who::Pad1, Who::Pad2, Who::Touch, Who::Computer];

    pub fn name(self) -> &'static str {
        match self {
            Who::Auto => "AUTO",
            Who::Pad1 => "PAD 1",
            Who::Pad2 => "PAD 2",
            Who::Touch => "TOUCH",
            Who::Computer => "COMPUTER",
        }
    }

    /// Choices that only one side can have at a time.
    fn exclusive(self) -> bool {
        matches!(self, Who::Pad1 | Who::Pad2 | Who::Touch)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opts {
    pub side: [Who; 2],
    pub four: bool,
    pub level: Level,
    pub length: Length,
    pub wild: bool,
    /// 60 frames a second instead of 30.
    pub fast: bool,
}

impl Opts {
    pub const DEFAULT: Opts = Opts {
        side: [Who::Auto, Who::Auto],
        four: false,
        level: Level::Normal,
        length: Length::First7,
        wild: false,
        fast: true,
    };

    pub fn rules(&self) -> Rules {
        Rules {
            length: self.length,
            level: self.level,
            four: self.four,
            wild: self.wild,
        }
    }
}

/// Who drives a mallet now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ctl {
    Cpu,
    /// Player `n`'s pad (0 to 3).
    Pad(u8),
    Touch,
}

impl Ctl {
    /// The player number shown on the mallet (1 to 4), or none.
    pub fn player(self) -> Option<u8> {
        match self {
            Ctl::Cpu | Ctl::Touch => None,
            Ctl::Pad(n) => Some(n + 1),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Ctl::Cpu => "COMPUTER",
            Ctl::Pad(0) => "PAD 1",
            Ctl::Pad(1) => "PAD 2",
            Ctl::Pad(2) => "PAD 3",
            Ctl::Pad(_) => "PAD 4",
            Ctl::Touch => "TOUCH",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Title,
    Play,
    End,
}

/// The finger, while it is down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Finger {
    pub x: i16,
    pub y: i16,
    pub down: bool,
}

/// What happened, for the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Log {
    Match { who: [Ctl; 4], opts: Opts },
    Goal { side: u8, score: [u8; 2] },
    Stall { to: u8 },
    Final { score: [u8; 2], series: [u8; 2] },
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
    /// Who drives each mallet now.
    pub ctl: [Ctl; 4],
    /// RIGHT AUTO: a finger took the right side, until the title.
    pub touch_claimed: bool,
    pub finger: Option<Finger>,
    /// Matches won in a row of rematches.
    pub series: [u8; 2],
    /// Which pads were connected in the last call (for the title).
    pub pads_seen: [bool; 4],
    /// Who serves the next match's first puck.
    first: u8,
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
            touch_claimed: false,
            finger: None,
            series: [0; 2],
            pads_seen: [false; 4],
            first: 0,
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

    /// Who drives each mallet with these pads: the two sides, then the
    /// second pair (2 AGAINST 2).
    pub fn drivers(&self, pads: [bool; 4]) -> [Ctl; 4] {
        let pad = |n: u8| {
            if pads[usize::from(n)] {
                Ctl::Pad(n)
            } else {
                Ctl::Cpu
            }
        };
        let pick = |w: Who, side: usize| match w {
            Who::Pad1 => pad(0),
            Who::Pad2 => pad(1),
            Who::Touch => Ctl::Touch,
            Who::Computer => Ctl::Cpu,
            Who::Auto if side == 0 => {
                if pads[0] {
                    Ctl::Pad(0)
                } else if self.touch_claimed {
                    // A finger already holds TEAL: it keeps it when pad 1
                    // drops out, and the computer covers CORAL.
                    Ctl::Cpu
                } else {
                    Ctl::Touch
                }
            }
            Who::Auto => {
                if pads[1] {
                    Ctl::Pad(1)
                } else if self.touch_claimed {
                    Ctl::Touch
                } else {
                    Ctl::Cpu
                }
            }
        };
        let mut left = pick(self.opts.side[0], 0);
        let mut right = pick(self.opts.side[1], 1);
        // One finger, one pad each: a side set by hand wins over AUTO;
        // between two set by hand (the title does not allow it), the
        // left side keeps it.
        if right == left && right != Ctl::Cpu {
            if self.opts.side[0] == Who::Auto && self.opts.side[1] != Who::Auto {
                left = Ctl::Cpu;
            } else {
                right = Ctl::Cpu;
            }
        }
        let mut who = [left, right, Ctl::Cpu, Ctl::Cpu];
        if self.opts.four {
            for (i, w) in who.iter_mut().enumerate().skip(2) {
                let p = pad(i as u8);
                if p != left && p != right {
                    *w = p;
                }
            }
        }
        who
    }

    /// One call. False: leave the game.
    pub fn update(&mut self, keys: &Keys) -> bool {
        // A touch is on the screen (whatever the panel says).
        let mut k = *keys;
        if let Some(t) = &mut k.touch {
            t.x = t.x.clamp(0, 159);
            t.y = t.y.clamp(0, 119);
        }
        let keys = &k;
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
            go = Some(if x < 80 && k != 0 && k != 8 { -1 } else { 1 });
        }
        let Some(step) = go else {
            return true;
        };
        let o = &mut self.opts;
        match self.sel {
            0 if step > 0 => self.start_match(keys),
            1 | 2 => {
                let s = usize::from(self.sel - 1);
                let other = o.side[1 - s];
                let mut w = o.side[s];
                // Skip what the other side already holds alone.
                loop {
                    w = cycle(&Who::ALL, w, step);
                    if !(w.exclusive() && w == other) {
                        break;
                    }
                }
                o.side[s] = w;
            }
            3 => o.four = !o.four,
            4 => o.level = cycle(&Level::ALL, o.level, step),
            5 => o.length = cycle(&Length::ALL, o.length, step),
            6 => o.wild = !o.wild,
            7 => o.fast = !o.fast,
            8 if step > 0 => {
                self.note(Log::Leave);
                return false;
            }
            _ => {}
        }
        true
    }

    fn start_match(&mut self, keys: &Keys) {
        let seed = keys.frame.wrapping_mul(0x9E37_79B9) ^ 0x5C1A_4E27;
        self.m = Match::new(self.opts.rules(), self.first, seed);
        self.scene = Scene::Play;
        self.pause = None;
        self.finger = None;
        self.acc = 0;
        self.ctl = self.drivers(keys.pads.map(|p| p.connected));
        self.note(Log::Match {
            who: self.ctl,
            opts: self.opts,
        });
    }

    fn back_to_title(&mut self) {
        self.scene = Scene::Title;
        self.pause = None;
        self.sel = 0;
        self.touch_claimed = false;
        self.finger = None;
        self.series = [0; 2];
        self.first = 0;
    }

    // ---- The match ------------------------------------------------------------

    fn play(&mut self, keys: &Keys) {
        // The bar on top, touched: pause. START on any pad: pause.
        if let Some((_, y)) = keys.tap_start()
            && i32::from(y) < BAR_H
        {
            self.open_pause();
            return;
        }
        if keys.pressed() & button::START != 0 {
            self.open_pause();
            return;
        }
        let pads = keys.pads.map(|p| p.connected);
        // RIGHT AUTO with no pad 2: a finger on the right half takes it.
        if let Some(t) = keys.touch
            && t.started
            && i32::from(t.x) * PX >= CX
            && self.opts.side[1] == Who::Auto
            && !pads[1]
            && !self.touch_claimed
            && !self.drivers(pads).contains(&Ctl::Touch)
        {
            self.touch_claimed = true;
            self.note(Log::TouchPlays);
        }
        self.follow_finger(keys);
        self.ctl = self.drivers(pads);

        self.acc = self.acc.saturating_add(keys.delta_us);
        let n = (self.acc / TICK_US).min(MAX_TICKS);
        self.acc = if n == MAX_TICKS {
            0
        } else {
            self.acc - n * TICK_US
        };
        for k in 0..n {
            let first = k == 0;
            let mut its = [Intent::Stay; 4];
            for (i, it) in its.iter_mut().enumerate().take(self.m.mallets()) {
                *it = match self.ctl[i] {
                    Ctl::Cpu => cpu::intent(&mut self.m, i),
                    Ctl::Pad(p) => pad_intent(&keys.pads[usize::from(p)], first),
                    Ctl::Touch => self.touch_intent(),
                };
            }
            self.m.tick(&its);
            self.events();
            if self.m.phase == Phase::Over {
                self.finish();
                break;
            }
        }
        if self.finger.is_some_and(|f| !f.down) {
            self.finger = None;
        }
    }

    fn follow_finger(&mut self, keys: &Keys) {
        match keys.touch {
            Some(t) if t.started || self.finger.is_some() => {
                self.finger = Some(Finger {
                    x: t.x,
                    y: t.y,
                    down: t.down && !t.ended,
                });
            }
            Some(_) => {}
            None => {
                if let Some(f) = &mut self.finger {
                    f.down = false;
                }
            }
        }
    }

    /// The touch screen: the mallet goes to the finger, no faster than
    /// [`TOUCH_V`] (the table keeps it on its own half).
    fn touch_intent(&self) -> Intent {
        match self.finger {
            Some(f) if f.down => {
                let x = i32::from(f.x) * PX + PX / 2;
                let y = (i32::from(f.y) * PX + PX / 2).clamp(YT, YB);
                Intent::Go {
                    x,
                    y,
                    v: TOUCH_V,
                    dash: false,
                }
            }
            _ => Intent::Stay,
        }
    }

    fn events(&mut self) {
        while let Some(e) = self.m.take_event() {
            match e {
                Ev::Goal { side } => {
                    let score = self.m.score;
                    self.note(Log::Goal { side, score });
                }
                Ev::Stall { to } => self.note(Log::Stall { to }),
                _ => {}
            }
        }
    }

    /// The match is over: the winner's series goes up, the result shows,
    /// and the loser serves first in a rematch.
    fn finish(&mut self) {
        if let Some(w) = self.m.winner() {
            self.series[w] = self.series[w].saturating_add(1);
            self.first = 1 - w as u8;
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
                self.opts.level = cycle(&Level::ALL, self.opts.level, step);
                self.m.rules.level = self.opts.level;
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

/// A person's pad: the stick (or the d-pad) moves, A or B dashes.
pub fn pad_intent(pad: &PadIn, first: bool) -> Intent {
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
    Intent::Pad {
        dx,
        dy,
        dash: first && pad.pressed & (button::A | button::B) != 0,
    }
}

fn in_box(x: i16, y: i16, b: (i16, i16, u16, u16)) -> bool {
    (b.0..b.0 + b.2 as i16).contains(&x) && (b.1..b.1 + b.3 as i16).contains(&y)
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
