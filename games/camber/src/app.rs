//! The screens (title, race, pause, results), the menus and the controls.
//!
//! Time: the race moves in ticks of 1/60 s; each call runs as many ticks
//! as the time since the last call holds (at most [`MAX_TICKS`]), so 30
//! and 60 frames a second play the same.

use game_kit::{Frame, Touch, button};

use crate::art;
use crate::hud;
use crate::poly::Stats;
use crate::race::{COUNT, Carve, CatchUp, Class, Ctl, Ev, Mode, Phase, Race};
use crate::scene::{self, Cam, Detail, ViewKind};
use crate::track::TRACKS;
use crate::tv::{self, Tv};

/// One tick, in microseconds.
pub const TICK_US: u32 = 16_667;
/// At most this many ticks in one call.
pub const MAX_TICKS: u32 = 6;
/// A stick pushed less than this counts as let go.
pub const DEAD_ZONE: i32 = 6_000;
/// A finger below this line (the bottom sixth) is in the carve strip.
pub const TOUCH_STRIP: i16 = 100;
/// Ticks after the player's finish before the results show.
pub const RESULTS_AFTER: u32 = 150;

// ---- Layout shared by drawing and touch ------------------------------------

pub const LINE: i16 = 9;
/// The title's column on the left: its width, and its rows (one line of
/// the menu each, the label over the value).
pub const COL_W: i16 = 72;
pub const ROWS_Y: i16 = 30;
pub const ROW: i16 = 13;
/// Rows on the title's two pages.
pub const MAIN_LINES: u8 = 6;
pub const OPT_LINES: u8 = 6;
pub const PAUSE_BOX: (i16, i16, u16, u16) = (24, 18, 112, 84);
pub const PAUSE_Y: i16 = 32;
pub const PAUSE_LINES: u8 = 7;
pub const END_BOX: (i16, i16, u16, u16) = (10, 8, 140, 104);
pub const END_Y: i16 = 92;
pub const END_LINES: u8 = 2;

/// The title's row a touch at (`x`, `y`) is on (only inside the column).
pub fn row_at(x: i16, y: i16, lines: u8) -> Option<u8> {
    let k = (y - ROWS_Y).div_euclid(ROW);
    (x < COL_W && (0..i16::from(lines)).contains(&k)).then_some(k as u8)
}

/// The menu line a touch at `y` is on.
pub fn line_at(y: i16, top: i16, lines: u8) -> Option<u8> {
    let k = (y - top + 2).div_euclid(LINE);
    (0..i16::from(lines)).contains(&k).then_some(k as u8)
}

// ---- Input ------------------------------------------------------------------

/// Everything the game reads in one call (player 1 and the touch screen).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    pub held: u32,
    pub pressed: u32,
    pub stick: (i16, i16),
    /// Left and right trigger, 0..=1023.
    pub trig: (u16, u16),
    pub touch: Option<Touch>,
    pub delta_us: u32,
}

impl Keys {
    fn tap(&self) -> Option<(i16, i16)> {
        self.touch.filter(|t| t.started).map(|t| (t.x, t.y))
    }
}

// ---- Settings -----------------------------------------------------------------

/// The player's choices (all of them can be changed on the title; detail,
/// rate and view also in the pause menu).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opts {
    pub track: u8,
    pub class: Class,
    pub mode: Mode,
    pub detail: Detail,
    /// 60 frames a second (otherwise 30).
    pub fast: bool,
    pub view: ViewKind,
    /// The rivals' catch-up in a race ([`Race::pace_of`]); LIGHT unless
    /// the player picks OFF.
    pub catchup: CatchUp,
    /// What moves behind the title's menu (RACE unless the player picks
    /// SHIP).
    pub backdrop: Backdrop,
}

/// Something for the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Log {
    Start,
    Lap { lap: i16, time: u32 },
    Finish { place: u8, time: u32 },
    Fell,
    Pause,
    Resume,
    Leave,
}

/// The title's two pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// PLAY, TRACK, CLASS, MODE, OPTIONS, LEAVE.
    Main,
    /// DETAIL, RATE, VIEW, CATCH-UP, BACKDROP, BACK.
    Options,
}

impl Page {
    pub fn lines(self) -> u8 {
        match self {
            Page::Main => MAIN_LINES,
            Page::Options => OPT_LINES,
        }
    }
}

/// What moves behind the title's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backdrop {
    /// A made-up race of the computer's pilots, filmed ([`tv`]).
    Race,
    /// The player's ship alone on the grid, turning ([`tv::stand`]).
    Ship,
}

impl Backdrop {
    pub const ALL: [Backdrop; 2] = [Backdrop::Race, Backdrop::Ship];

    pub fn name(self) -> &'static str {
        match self {
            Backdrop::Race => "RACE",
            Backdrop::Ship => "SHIP",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Title,
    Race,
}

/// A short message in the middle of the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    None,
    Lap(i16, u32),
    Final(u32),
    Boost,
}

pub struct App {
    pub opts: Opts,
    pub scene: Scene,
    pub page: Page,
    /// The title's cameras.
    pub tv: Tv,
    pub sel: u8,
    pub pause: Option<u8>,
    pub end_sel: u8,
    pub race: Race,
    pub cam: Cam,
    acc: u32,
    pub note: Note,
    pub note_t: u32,
    /// The best lap of this session on each track and class (ticks).
    pub best: [[u32; 3]; 3],
    logs: [Option<Log>; 4],
    /// What the renderer did in the last picture.
    pub stats: Stats,
    /// Ticks run in the last call (for the tests).
    pub ticks: u32,
    pub frames: u32,
}

impl App {
    pub const fn new() -> App {
        App {
            opts: Opts {
                track: 0,
                class: Class::Brisk,
                mode: Mode::Race,
                detail: Detail::High,
                fast: true,
                view: ViewKind::Chase,
                catchup: CatchUp::Light,
                backdrop: Backdrop::Race,
            },
            scene: Scene::Title,
            page: Page::Main,
            tv: Tv::new(),
            sel: 0,
            pause: None,
            end_sel: 0,
            race: Race::new(),
            cam: Cam::new(),
            acc: 0,
            note: Note::None,
            note_t: 0,
            best: [[0; 3]; 3],
            logs: [None; 4],
            stats: Stats {
                verts: 0,
                projected: 0,
                polys: 0,
                clipped: 0,
                tris: 0,
                rows: 0,
                pixels: 0,
                dropped: 0,
            },
            ticks: 0,
            frames: 0,
        }
    }

    pub fn take_log(&mut self) -> Option<Log> {
        let l = self.logs[0];
        if l.is_some() {
            self.logs.copy_within(1.., 0);
            self.logs[3] = None;
        }
        l
    }

    fn log(&mut self, l: Log) {
        if let Some(s) = self.logs.iter_mut().find(|x| x.is_none()) {
            *s = Some(l);
        }
    }

    /// Nothing moves: a paused race (the title has its backdrop).
    pub fn still(&self) -> bool {
        self.scene == Scene::Race && self.pause.is_some()
    }

    /// The theme the palette must hold.
    pub fn theme(&self) -> u8 {
        match self.scene {
            Scene::Title => self.opts.track,
            Scene::Race => self.race.track,
        }
    }

    pub fn results(&self) -> bool {
        self.race.phase == Phase::Done && self.race.after >= RESULTS_AFTER
    }

    /// One call: read the keys, run the ticks. False: leave the game.
    pub fn update(&mut self, k: &Keys) -> bool {
        self.ticks = 0;
        match self.scene {
            Scene::Title => self.title(k),
            Scene::Race => self.racing(k),
        }
    }

    fn title(&mut self, k: &Keys) -> bool {
        self.backdrop_step(k.delta_us);
        let lines = self.page.lines();
        let mut step = 0i32;
        let mut act = false;
        let p = k.pressed;
        if p & button::UP != 0 {
            self.sel = (self.sel + lines - 1) % lines;
        }
        if p & button::DOWN != 0 {
            self.sel = (self.sel + 1) % lines;
        }
        if p & button::LEFT != 0 {
            step = -1;
        }
        if p & button::RIGHT != 0 {
            step = 1;
        }
        if p & (button::A | button::START) != 0 {
            act = true;
            step = 1;
        }
        if p & button::B != 0 && self.page == Page::Options {
            self.page = Page::Main;
            self.sel = 4;
            return true;
        }
        if let Some(line) = k.tap().and_then(|(x, y)| row_at(x, y, lines)) {
            self.sel = line;
            act = true;
            step = 1;
        }
        if step == 0 {
            return true;
        }
        let o = &mut self.opts;
        match (self.page, self.sel) {
            (Page::Main, 0) if act => {
                self.start();
                return true;
            }
            (Page::Main, 1) => {
                o.track = (i32::from(o.track) + step).rem_euclid(TRACKS.len() as i32) as u8;
            }
            (Page::Main, 2) => o.class = cycle(&Class::ALL, o.class, step),
            (Page::Main, 3) => o.mode = cycle(&[Mode::Race, Mode::Trial], o.mode, step),
            (Page::Main, 4) if act => {
                self.page = Page::Options;
                self.sel = 0;
            }
            (Page::Main, 5) if act => {
                self.log(Log::Leave);
                return false;
            }
            (Page::Options, 0) => o.detail = cycle(&Detail::ALL, o.detail, step),
            (Page::Options, 1) => o.fast = !o.fast,
            (Page::Options, 2) => o.view = cycle(&ViewKind::ALL, o.view, step),
            (Page::Options, 3) => o.catchup = cycle(&CatchUp::ALL, o.catchup, step),
            (Page::Options, 4) => o.backdrop = cycle(&Backdrop::ALL, o.backdrop, step),
            (Page::Options, 5) if act => {
                self.page = Page::Main;
                self.sel = 4;
            }
            _ => {}
        }
        // A new track, class or backdrop: show it at once.
        if !self.shows_opts() {
            self.backdrop_restart();
        }
        true
    }

    /// Whether the backdrop shows the menu's track, class and backdrop
    /// (the SHIP backdrop is the only one with a time trial's one ship).
    fn shows_opts(&self) -> bool {
        let (o, r) = (&self.opts, &self.race);
        r.track == o.track
            && r.class == o.class
            && (r.mode == Mode::Trial) == (o.backdrop == Backdrop::Ship)
    }

    /// The title's backdrop: its ticks for `delta_us` (the same clock as
    /// a race), a new made-up race when the menu's track or class is not
    /// the one shown or the last one is over. Its events are dropped: no
    /// best lap, no log line, no note. [`App::ticks`] stays 0.
    fn backdrop_step(&mut self, delta_us: u32) {
        if !self.tv.live || !self.shows_opts() {
            self.backdrop_restart();
        }
        self.acc = self.acc.saturating_add(delta_us).min(TICK_US * MAX_TICKS);
        while self.acc >= TICK_US {
            self.acc -= TICK_US;
            match self.opts.backdrop {
                Backdrop::Race => {
                    if tv::done(&self.race) {
                        self.backdrop_restart();
                    }
                    self.race.step(None);
                    while self.race.take_ev().is_some() {}
                    self.tv.step(&self.race, &mut self.cam);
                }
                // Not a race: only its clock runs (the ship's turn, bob
                // and engine).
                Backdrop::Ship => {
                    self.race.tick = self.race.tick.wrapping_add(1);
                    tv::stand(&mut self.race, &mut self.cam);
                }
            }
        }
    }

    fn backdrop_restart(&mut self) {
        self.cam = Cam::new();
        let o = self.opts;
        match o.backdrop {
            Backdrop::Race => {
                self.tv
                    .restart(&mut self.race, &mut self.cam, o.track, o.class);
            }
            Backdrop::Ship => {
                self.race.setup(o.track, o.class, Mode::Trial, 0);
                self.tv.live = true;
                tv::stand(&mut self.race, &mut self.cam);
            }
        }
    }

    pub fn start(&mut self) {
        let o = self.opts;
        self.race.setup(o.track, o.class, o.mode, self.frames);
        self.race.catchup = o.catchup;
        self.cam = Cam::new();
        self.cam.follow(self.race.tr(), &self.race.ships[0], o.view);
        self.scene = Scene::Race;
        self.pause = None;
        self.note = Note::None;
        self.end_sel = 0;
        self.acc = 0;
        self.log(Log::Start);
    }

    fn back_to_title(&mut self) {
        self.scene = Scene::Title;
        self.pause = None;
        self.page = Page::Main;
        self.sel = 0;
        self.acc = 0;
        self.backdrop_restart();
    }

    fn racing(&mut self, k: &Keys) -> bool {
        if let Some(sel) = self.pause {
            return self.paused(k, sel);
        }
        let top_tap = k.tap().is_some_and(|(_, y)| y < 16);
        if self.results() {
            return self.end(k);
        }
        if k.pressed & button::START != 0 || top_tap {
            self.pause = Some(0);
            self.log(Log::Pause);
            return true;
        }
        if k.pressed & button::X != 0 {
            self.opts.detail = cycle(&Detail::ALL, self.opts.detail, 1);
        }
        if k.pressed & button::Y != 0 {
            self.opts.view = cycle(&ViewKind::ALL, self.opts.view, 1);
            self.cam.set = false;
        }
        if k.pressed & button::SELECT != 0 {
            self.opts.fast = !self.opts.fast;
        }
        let ctl = control(k);
        // Saturating: the console hands up to u32::MAX microseconds after
        // a long gap, and a wrapped sum would stop the chip.
        self.acc = self.acc.saturating_add(k.delta_us).min(TICK_US * MAX_TICKS);
        while self.acc >= TICK_US {
            self.acc -= TICK_US;
            self.tick(ctl);
        }
        true
    }

    /// One tick of the race, the camera and the notes.
    pub fn tick(&mut self, ctl: Ctl) {
        self.ticks += 1;
        self.race.step(Some(ctl));
        let view = self.opts.view;
        self.cam.follow(self.race.tr(), &self.race.ships[0], view);
        self.note_t = self.note_t.saturating_add(1);
        while let Some(e) = self.race.take_ev() {
            match e {
                Ev::Lap { ship: 0, lap, time } => {
                    self.set_best(time);
                    self.note = if lap == crate::race::LAPS {
                        Note::Final(time)
                    } else {
                        Note::Lap(lap, time)
                    };
                    self.note_t = 0;
                    self.log(Log::Lap { lap: lap - 1, time });
                }
                Ev::Finish {
                    ship: 0,
                    place,
                    time,
                } => {
                    let best = self.race.ships[0].best;
                    self.set_best(best);
                    self.note = Note::None;
                    self.log(Log::Finish { place, time });
                }
                Ev::Fell { ship: 0 } => self.log(Log::Fell),
                Ev::Boost { ship: 0 } if matches!(self.note, Note::None) || self.note_t > 60 => {
                    self.note = Note::Boost;
                    self.note_t = 0;
                }
                _ => {}
            }
        }
    }

    fn set_best(&mut self, t: u32) {
        let (a, b) = (
            usize::from(self.race.track) % 3,
            class_index(self.race.class),
        );
        if t > 0 && (self.best[a][b] == 0 || t < self.best[a][b]) {
            self.best[a][b] = t;
        }
    }

    /// The session's best lap on `track` in `class` (0: none yet).
    pub fn session_best(&self, track: u8, class: Class) -> u32 {
        self.best[usize::from(track) % 3][class_index(class)]
    }

    fn paused(&mut self, k: &Keys, sel: u8) -> bool {
        let mut sel = sel;
        let p = k.pressed;
        let mut step = 0;
        let mut act = false;
        if p & button::UP != 0 {
            sel = (sel + PAUSE_LINES - 1) % PAUSE_LINES;
        }
        if p & button::DOWN != 0 {
            sel = (sel + 1) % PAUSE_LINES;
        }
        if p & button::LEFT != 0 {
            step = -1;
        }
        if p & button::RIGHT != 0 {
            step = 1;
        }
        if p & button::A != 0 {
            act = true;
            step = 1;
        }
        if p & (button::START | button::B) != 0 {
            self.pause = None;
            self.log(Log::Resume);
            return true;
        }
        if let Some((x, y)) = k.tap() {
            if in_box(x, y, PAUSE_BOX) {
                if let Some(line) = line_at(y, PAUSE_Y, PAUSE_LINES) {
                    sel = line;
                    act = true;
                    step = 1;
                }
            } else {
                self.pause = None;
                self.log(Log::Resume);
                return true;
            }
        }
        self.pause = Some(sel);
        if step == 0 {
            return true;
        }
        match sel {
            0 if act => {
                self.pause = None;
                self.log(Log::Resume);
            }
            1 => self.opts.detail = cycle(&Detail::ALL, self.opts.detail, step),
            2 => self.opts.fast = !self.opts.fast,
            3 => {
                self.opts.view = cycle(&ViewKind::ALL, self.opts.view, step);
                self.cam.set = false;
                let v = self.opts.view;
                self.cam.follow(self.race.tr(), &self.race.ships[0], v);
            }
            4 if act => self.start(),
            5 if act => self.back_to_title(),
            6 if act => {
                self.log(Log::Leave);
                return false;
            }
            _ => {}
        }
        true
    }

    fn end(&mut self, k: &Keys) -> bool {
        let p = k.pressed;
        if p & (button::UP | button::DOWN) != 0 {
            self.end_sel ^= 1;
        }
        let mut act = p & (button::A | button::START) != 0;
        if let Some(line) = k.tap().and_then(|(_, y)| line_at(y, END_Y, END_LINES)) {
            self.end_sel = line;
            act = true;
        }
        // The others go on racing behind the panel.
        self.acc = self.acc.saturating_add(k.delta_us).min(TICK_US * MAX_TICKS);
        while self.acc >= TICK_US {
            self.acc -= TICK_US;
            self.tick(Ctl::default());
        }
        if act {
            if self.end_sel == 0 {
                self.start();
            } else {
                self.back_to_title();
            }
        }
        true
    }

    /// Draws the picture for this call.
    pub fn draw(&mut self, f: &mut Frame, time: u32) {
        self.frames = self.frames.wrapping_add(1);
        let mut st = Stats::default();
        let (view, time) = if self.scene == Scene::Title {
            (ViewKind::Show, self.race.tick)
        } else {
            (self.opts.view, time)
        };
        scene::draw(
            f,
            &self.race,
            &self.cam,
            view,
            self.opts.detail,
            time,
            &mut st,
        );
        self.stats = st;
        hud::draw(self, f);
    }
}

impl Default for App {
    fn default() -> Self {
        App::new()
    }
}

/// The player's controls from the keys: stick or pad to steer, A or the
/// right trigger to thrust, B or the left trigger the brake (both air
/// brakes), L and R the carve; on the touch screen a finger thrusts and
/// steers towards itself, and in the bottom strip far to one side it
/// carves that way (anywhere in the strip it keeps a carve going).
pub fn control(k: &Keys) -> Ctl {
    let mut steer = 0;
    let sx = i32::from(k.stick.0);
    if sx.abs() > DEAD_ZONE {
        steer = (sx - DEAD_ZONE * sx.signum()) * 256 / (32_767 - DEAD_ZONE);
    }
    if k.held & button::LEFT != 0 {
        steer = -256;
    }
    if k.held & button::RIGHT != 0 {
        steer = 256;
    }
    let mut thrust = k.held & button::A != 0 || k.trig.1 > 300;
    let brake = k.held & button::B != 0 || k.trig.0 > 500;
    let mut carve = match (k.held & button::L != 0, k.held & button::R != 0) {
        (true, true) => Carve::Both,
        (true, false) => Carve::Left,
        (false, true) => Carve::Right,
        (false, false) => Carve::Off,
    };
    if let Some(t) = k.touch.filter(|t| t.down && t.y >= 16) {
        thrust = true;
        steer = ((i32::from(t.x) - 80) * 256 / 48).clamp(-256, 256);
        if t.y > TOUCH_STRIP {
            carve = if steer < -128 {
                Carve::Left
            } else if steer > 128 {
                Carve::Right
            } else {
                Carve::Keep
            };
        }
    }
    Ctl {
        steer: steer.clamp(-256, 256),
        thrust,
        brake_l: brake,
        brake_r: brake,
        carve,
    }
}

pub fn class_index(c: Class) -> usize {
    Class::ALL.iter().position(|&x| x == c).unwrap_or(0)
}

fn in_box(x: i16, y: i16, b: (i16, i16, u16, u16)) -> bool {
    x >= b.0 && y >= b.1 && x < b.0 + b.2 as i16 && y < b.1 + b.3 as i16
}

fn cycle<T: Copy + PartialEq>(all: &[T], now: T, step: i32) -> T {
    let i = all.iter().position(|&x| x == now).unwrap_or(0) as i32;
    all[(i + step).rem_euclid(all.len() as i32) as usize]
}

/// The count's number (3, 2, 1) or 0 for GO, while it shows.
pub fn count_shown(race: &Race) -> Option<u32> {
    if race.phase == Phase::Count {
        Some(3 - race.tick * 3 / COUNT)
    } else if race.clock() < 40 && race.phase == Phase::Run {
        Some(0)
    } else {
        None
    }
}

/// Which colour a place gets on the results.
pub fn place_colour(place: u8) -> u8 {
    match place {
        1 => art::GOLD,
        2 => art::SILVER,
        3 => art::BRONZE,
        _ => art::TEXT,
    }
}
