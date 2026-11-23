//! The game's screens around the race: the title menu, racing, the
//! game's own pause menu, the finish card and the end of the cup.
//! [`App::update`] takes one call's [`Keys`] and the game time;
//! [`App::draw`] draws everything. Tests drive these two directly, with
//! no console.

use game_kit::{Frame, Touch, button};

use crate::hud::{self, Zone};
use crate::race::{LAPS, Pad, Race, RaceEvent};
use crate::render::{self, Cam, Cost, Detail};
use crate::track::COURSES;

/// One tick, in microseconds (60 a second).
pub const TICK_US: u64 = 16_667;
/// At most this many ticks are run in one call; a longer gap between
/// calls is skipped (the game slows down instead of jumping).
pub const MAX_CATCH_UP: u64 = 4;
/// Stick travel ignored in the middle (worn sticks drift).
pub const DEAD_ZONE: i32 = 4000;
/// Ticks before a card takes a press.
pub const CARD_WAIT: u32 = 40;
/// Ticks the race goes on after the player crosses the line, before
/// the finish card.
pub const FINISH_RUN: u32 = 90;
/// How often a cost line goes to the log while racing (ticks: 4 s).
pub const COST_EVERY: u32 = 240;
/// Lines of the title menu: PLAY, COURSE, DETAIL, RATE, VIEW, LEAVE.
pub const TITLE_LINES: u8 = 6;
/// Lines of the pause menu: RESUME, DETAIL, RATE, VIEW, RESTART, TITLE.
pub const PAUSE_LINES: u8 = 6;

/// One call's input, as the console gave it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    /// [`button`] bits held now, and pressed since the last call.
    pub held: u32,
    pub pressed: u32,
    pub left: (i16, i16),
    pub right: (i16, i16),
    /// Left and right trigger, 0..=32767.
    pub triggers: (u16, u16),
    pub touch: Option<Touch>,
}

impl Keys {
    fn pressed(&self, b: u32) -> bool {
        self.pressed & b != 0
    }
    fn held(&self, b: u32) -> bool {
        self.held & b != 0
    }
    /// A tap that started in this call.
    fn tap(&self) -> Option<(i16, i16)> {
        self.touch.filter(|t| t.started).map(|t| (t.x, t.y))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Title,
    Race,
    /// The race is over: place, time, what next.
    Finish,
    /// After the last course.
    Cup,
}

/// What the player can change, on the title and in the pause menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// The course PLAY starts at (0-based).
    pub course: u8,
    pub detail: Detail,
    /// 60 pictures a second instead of 30.
    pub fast: bool,
    pub cam: Cam,
}

/// Something worth a log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Start {
        course: u8,
        detail: Detail,
        cam: Cam,
        fast: bool,
    },
    Lap {
        lap: u8,
        ticks: u32,
    },
    Finish {
        course: u8,
        place: u8,
        ticks: u32,
    },
    Crash,
    Detail(Detail),
    View(Cam),
    Rate(bool),
    /// The renderer's counts for the last picture, now and then.
    Cost {
        detail: Detail,
        cost: Cost,
    },
    Cup {
        places: [u8; 3],
    },
    Quit,
}

pub struct App {
    pub scene: Scene,
    /// Ticks since the scene began.
    pub scene_t: u32,
    /// Selected line of the title menu.
    pub menu: u8,
    /// Selected line of the finish card.
    pub card: u8,
    /// The game's own pause menu, with its selected line.
    pub pause: Option<u8>,
    pub opts: Options,
    pub race: Race,
    pub cost: Cost,
    /// Ticks run (or skipped) since the game started.
    pub ticks: u64,
    /// A short note (like "LAP 2") and the ticks it has left.
    pub note: Option<(&'static str, u16)>,
    /// Best race time per course (ticks; 0: none yet). Kept while the
    /// game runs.
    pub best: [u32; 3],
    /// This cup's place and time per course (0: not raced).
    pub cup_places: [u8; 3],
    pub cup_times: [u32; 3],
    /// Ticks since the player crossed the line for the last time.
    after_finish: u32,
    /// The stick's last direction, for menu steps by stick.
    stick_dir: (i8, i8),
    /// The last tick's controls (tests read them).
    pub last_pad: Pad,
    events: [Option<Event>; 6],
}

impl Default for App {
    fn default() -> Self {
        App::new()
    }
}

impl App {
    /// The game with the title's course set up (tests use this).
    pub fn new() -> App {
        let mut app = App::blank();
        app.load_title();
        app
    }

    /// The game before the title's course is set up: a `const`, so the
    /// chip can build it without a second copy on the stack. Call
    /// [`App::load_title`] before the first update.
    pub const fn blank() -> App {
        App {
            scene: Scene::Title,
            scene_t: 0,
            menu: 0,
            card: 0,
            pause: None,
            opts: Options {
                course: 0,
                detail: Detail::Heavy,
                fast: true,
                cam: Cam::Chase,
            },
            race: Race::empty(),
            cost: Cost {
                segs: 0,
                rows: 0,
                sprites: 0,
                sprite_px: 0,
            },
            ticks: 0,
            note: None,
            best: [0; 3],
            cup_places: [0; 3],
            cup_times: [0; 3],
            after_finish: 0,
            stick_dir: (0, 0),
            last_pad: Pad {
                throttle: 0,
                brake: 0,
                steer: 0,
                boost: false,
            },
            events: [None; 6],
        }
    }

    /// Sets up the title's course in place.
    pub fn load_title(&mut self) {
        self.race.reset(self.opts.course);
    }

    /// The next thing worth a log line, oldest first.
    pub fn take_event(&mut self) -> Option<Event> {
        let e = self.events[0].take();
        self.events.rotate_left(1);
        e
    }

    fn event(&mut self, e: Event) {
        if let Some(slot) = self.events.iter_mut().find(|s| s.is_none()) {
            *slot = Some(e);
        }
    }

    /// Ticks due by game time `us` (60 a second).
    pub fn ticks_at(us: u64) -> u64 {
        us / TICK_US
    }

    /// Whether nothing moves on screen until the player does something:
    /// the console may then call the game only on input.
    pub fn still(&self) -> bool {
        if self.pause.is_some() {
            return self.note.is_none();
        }
        match self.scene {
            Scene::Title | Scene::Cup => self.note.is_none(),
            Scene::Finish => self.scene_t >= CARD_WAIT && self.note.is_none(),
            Scene::Race => false,
        }
    }

    /// One call. Returns false when the player chose LEAVE.
    pub fn update(&mut self, k: &Keys, now_us: u64) -> bool {
        let due = App::ticks_at(now_us);
        let mut n = due.saturating_sub(self.ticks).min(MAX_CATCH_UP);
        self.ticks = due.max(self.ticks);
        let stick = stick_steps(k.left, &mut self.stick_dir);
        if let Some((t, left)) = self.note {
            let left = u64::from(left).saturating_sub(n);
            self.note = if left == 0 {
                None
            } else {
                Some((t, left as u16))
            };
        }

        if let Some(sel) = self.pause {
            n = 0;
            self.pause_menu(k, sel, stick);
        } else {
            match self.scene {
                Scene::Title => {
                    if !self.title_menu(k, stick) {
                        self.event(Event::Quit);
                        return false;
                    }
                }
                Scene::Race => self.race_keys(k),
                Scene::Finish => {
                    if self.scene_t >= CARD_WAIT {
                        self.finish_card(k, stick);
                    }
                }
                Scene::Cup => {
                    n = 0;
                    if go_on(k) || k.pressed(button::START) {
                        self.back_to_title();
                    }
                }
            }
            if self.pause.is_some() {
                n = 0;
            }
        }

        for _ in 0..n {
            self.tick(k);
        }
        true
    }

    /// The buttons that are not driving, while racing.
    fn race_keys(&mut self, k: &Keys) {
        if k.pressed(button::START) {
            self.pause = Some(0);
            return;
        }
        if k.pressed(button::SELECT) {
            self.set_detail(self.opts.detail.next());
        }
        if k.pressed(button::Y) {
            self.set_cam(self.opts.cam.next());
        }
        if let Some((x, y)) = k.tap()
            && hud::zone_at(self.opts.detail, x, y) == Zone::Pause
        {
            self.pause = Some(0);
        }
    }

    fn set_detail(&mut self, d: Detail) {
        self.opts.detail = d;
        self.note(match d {
            Detail::Light => "DETAIL: LIGHT",
            Detail::Normal => "DETAIL: NORMAL",
            Detail::Heavy => "DETAIL: HEAVY",
        });
        self.event(Event::Detail(d));
    }

    fn set_cam(&mut self, c: Cam) {
        self.opts.cam = c;
        self.note(match c {
            Cam::Chase => "VIEW: CHASE",
            Cam::Nose => "VIEW: NOSE",
        });
        self.event(Event::View(c));
    }

    fn set_fast(&mut self, fast: bool) {
        self.opts.fast = fast;
        self.event(Event::Rate(fast));
    }

    /// One tick (1/60 s).
    fn tick(&mut self, k: &Keys) {
        self.scene_t = self.scene_t.saturating_add(1);
        if self.scene != Scene::Race {
            return;
        }
        let pad = self.controls(k);
        self.last_pad = pad;
        self.race.tick(&pad);
        while let Some(e) = self.race.take_event() {
            match e {
                RaceEvent::Go => {}
                RaceEvent::Lap { done, ticks } => {
                    self.event(Event::Lap { lap: done, ticks });
                    if u32::from(done) + 1 == LAPS {
                        self.note("FINAL LAP");
                    } else if u32::from(done) < LAPS {
                        self.note(["", "LAP 2", "LAP 3"][usize::from(done).min(2)]);
                    }
                }
                RaceEvent::Finish { place, ticks } => {
                    self.event(Event::Finish {
                        course: self.race.course,
                        place,
                        ticks,
                    });
                    self.note("FINISH");
                }
                RaceEvent::Crash => self.event(Event::Crash),
                RaceEvent::Bump => {}
            }
        }
        if self.race.finished {
            self.after_finish += 1;
            if self.after_finish >= FINISH_RUN {
                self.end_race();
            }
        }
        if self.race.race_ticks() > 0 && self.race.race_ticks().is_multiple_of(COST_EVERY) {
            self.event(Event::Cost {
                detail: self.opts.detail,
                cost: self.cost,
            });
        }
    }

    /// The player's wishes from the pad and the finger.
    pub fn controls(&self, k: &Keys) -> Pad {
        let dead = |v: i16| -> i32 {
            let v = i32::from(v);
            if v.abs() < DEAD_ZONE {
                0
            } else {
                (v - v.signum() * DEAD_ZONE) * 256 / (32_767 - DEAD_ZONE)
            }
        };
        let trig = |t: u16| i32::from(t) * 256 / 32_767;
        // Either stick steers (the left one wins when both are pushed).
        let mut steer = dead(k.left.0);
        if steer == 0 {
            steer = dead(k.right.0);
        }
        if k.held(button::LEFT) {
            steer = -256;
        }
        if k.held(button::RIGHT) {
            steer = 256;
        }
        let mut throttle = trig(k.triggers.1);
        if k.held(button::A | button::UP) {
            throttle = 256;
        }
        let mut brake = trig(k.triggers.0);
        if k.held(button::B | button::DOWN | button::L) {
            brake = 256;
        }
        let mut boost = k.held(button::X | button::R);
        if let Some(t) = k.touch.filter(|t| t.down) {
            match hud::zone_at(self.opts.detail, t.x, t.y) {
                Zone::Drive(s) => {
                    throttle = 256;
                    steer = s;
                }
                Zone::Brake => brake = 256,
                Zone::Boost => {
                    boost = true;
                    throttle = 256;
                }
                Zone::Pause => {}
            }
        }
        Pad {
            throttle: throttle.clamp(0, 256),
            brake: brake.clamp(0, 256),
            steer: steer.clamp(-256, 256),
            boost,
        }
    }

    fn go(&mut self, s: Scene) {
        self.scene = s;
        self.scene_t = 0;
    }

    fn note(&mut self, text: &'static str) {
        self.note = Some((text, 75));
    }

    /// Course `c` (0-based) from the grid.
    fn begin_race(&mut self, c: u8) {
        self.race.reset(c);
        self.pause = None;
        self.after_finish = 0;
        self.event(Event::Start {
            course: c + 1,
            detail: self.opts.detail,
            cam: self.opts.cam,
            fast: self.opts.fast,
        });
        self.go(Scene::Race);
    }

    /// The race is over: keep the record, show the card.
    fn end_race(&mut self) {
        let c = usize::from(self.race.course);
        let t = self.race.finish_ticks;
        if self.best[c] == 0 || t < self.best[c] {
            self.best[c] = t;
        }
        self.cup_places[c] = self.race.finish_place;
        self.cup_times[c] = t;
        self.card = 0;
        self.go(Scene::Finish);
    }

    fn back_to_title(&mut self) {
        self.pause = None;
        self.race.reset(self.opts.course);
        self.go(Scene::Title);
    }

    /// Steps an option line by `change` (+1 or -1).
    fn change_option(&mut self, line: u8, change: i32) {
        match line {
            1 => {
                let n = COURSES.len() as i32;
                self.opts.course = (i32::from(self.opts.course) + change).rem_euclid(n) as u8;
                self.race.reset(self.opts.course);
            }
            2 => {
                let i = Detail::ALL
                    .iter()
                    .position(|d| *d == self.opts.detail)
                    .unwrap_or(0) as i32;
                let n = Detail::ALL.len() as i32;
                self.set_detail(Detail::ALL[(i + change).rem_euclid(n) as usize]);
            }
            3 => self.set_fast(!self.opts.fast),
            4 => self.set_cam(self.opts.cam.next()),
            _ => {}
        }
    }

    /// Title menu: PLAY, COURSE, DETAIL, RATE, VIEW, LEAVE. False for
    /// LEAVE.
    fn title_menu(&mut self, k: &Keys, stick: (i8, i8)) -> bool {
        let mut sel = self.menu;
        let mut act = k.pressed(button::A);
        let mut change = 0i32;
        if k.pressed(button::UP) || stick.1 < 0 {
            sel = (sel + TITLE_LINES - 1) % TITLE_LINES;
        }
        if k.pressed(button::DOWN) || stick.1 > 0 {
            sel = (sel + 1) % TITLE_LINES;
        }
        if k.pressed(button::LEFT) || stick.0 < 0 {
            change = -1;
        }
        if k.pressed(button::RIGHT) || stick.0 > 0 {
            change = 1;
        }
        if k.pressed(button::START) {
            sel = 0;
            act = true;
        }
        let mut back = false;
        if let Some((x, y)) = k.tap()
            && let Some(line) = hud::title_line_at(x, y)
        {
            sel = line;
            act = true;
            back = hud::is_left_half(x);
        }
        self.menu = sel;
        if act {
            // A tap on the left half of a value line steps back.
            change = if back { -1 } else { 1 };
        }
        match sel {
            0 if act => {
                self.cup_places = [0; 3];
                self.cup_times = [0; 3];
                self.begin_race(self.opts.course);
            }
            1..=4 if change != 0 => self.change_option(sel, change),
            5 if act => return false,
            _ => {}
        }
        true
    }

    /// The pause menu: RESUME, DETAIL, RATE, VIEW, RESTART, TITLE.
    fn pause_menu(&mut self, k: &Keys, mut sel: u8, stick: (i8, i8)) {
        if k.pressed(button::UP) || stick.1 < 0 {
            sel = (sel + PAUSE_LINES - 1) % PAUSE_LINES;
        }
        if k.pressed(button::DOWN) || stick.1 > 0 {
            sel = (sel + 1) % PAUSE_LINES;
        }
        let mut change = 0;
        if k.pressed(button::LEFT) || stick.0 < 0 {
            change = -1;
        }
        if k.pressed(button::RIGHT) || stick.0 > 0 {
            change = 1;
        }
        let mut act = k.pressed(button::A);
        let mut back = false;
        if let Some((x, y)) = k.tap()
            && let Some(line) = hud::pause_line_at(x, y)
        {
            sel = line;
            act = true;
            back = hud::is_left_half(x);
        }
        if k.pressed(button::START | button::B) {
            self.pause = None;
            return;
        }
        self.pause = Some(sel);
        if act {
            change = if back { -1 } else { 1 };
        }
        match sel {
            0 if act => self.pause = None,
            // The pause menu's DETAIL, RATE, VIEW are the title's 2, 3, 4.
            1..=3 if change != 0 => self.change_option(sel + 1, change),
            4 if act => self.begin_race(self.race.course),
            5 if act => self.back_to_title(),
            _ => {}
        }
    }

    /// The finish card: go on (next course, race again, or the cup), or
    /// back to the title.
    fn finish_card(&mut self, k: &Keys, stick: (i8, i8)) {
        let mut sel = self.card;
        if k.pressed(button::UP | button::DOWN) || stick.1 != 0 {
            sel = 1 - sel.min(1);
        }
        let mut act = k.pressed(button::A);
        if let Some(line) = k.tap().and_then(|(x, y)| hud::card_line_at(x, y)) {
            sel = line;
            act = true;
        }
        if k.pressed(button::START) {
            sel = 0;
            act = true;
        }
        self.card = sel;
        if !act {
            return;
        }
        if sel == 1 {
            self.back_to_title();
            return;
        }
        let c = self.race.course;
        if self.race.finish_place > 3 {
            self.begin_race(c);
        } else if usize::from(c) + 1 < COURSES.len() {
            self.begin_race(c + 1);
        } else {
            self.event(Event::Cup {
                places: self.cup_places,
            });
            self.go(Scene::Cup);
        }
    }

    /// Whether the brake shows as pressed on the dashboard.
    fn braking(&self) -> bool {
        self.scene == Scene::Race && self.last_pad.brake > 0 && !self.race.finished
    }

    /// Draws the whole picture.
    pub fn draw(&mut self, s: &mut Frame) {
        if self.scene == Scene::Cup {
            hud::cup(s, &self.cup_places, &self.cup_times);
            return;
        }
        // The title shows the course behind its menu in the full view
        // (it is still: it costs nothing to send).
        let detail = if self.scene == Scene::Title {
            Detail::Normal
        } else {
            self.opts.detail
        };
        if detail == Detail::Light {
            hud::light_frame(s, &self.race);
        }
        render::draw(s, &self.race, detail, self.opts.cam, &mut self.cost);
        match self.scene {
            Scene::Title => {
                s.rect(0, 0, 160, render::TOP as u16 - 1, crate::art::UI_BG);
                s.hline(0, render::TOP as i16 - 1, 160, crate::art::UI_EDGE);
                s.text(2, 2, "PRESS A OR TAP A LINE", crate::art::TEXT_DIM);
                hud::title(s, self.menu, &self.opts, &self.best);
            }
            Scene::Race | Scene::Finish => {
                hud::top_bar(s, &self.race, detail);
                hud::dash(s, &self.race, detail, self.braking());
                hud::countdown(s, &self.race, detail);
                if self.scene == Scene::Finish {
                    let last = usize::from(self.race.course) + 1 == COURSES.len();
                    hud::finish_card(s, &self.race, self.card, self.scene_t >= CARD_WAIT, last);
                }
            }
            Scene::Cup => {}
        }
        if let Some(sel) = self.pause {
            hud::pause(s, sel, &self.opts);
        }
        if let Some((text, _)) = self.note {
            let panel_up = self.pause.is_some() || self.scene == Scene::Finish;
            hud::note(s, text, hud::note_y(detail, panel_up));
        }
    }
}

/// Any of the "go on" inputs: A, a tap.
fn go_on(k: &Keys) -> bool {
    k.pressed(button::A) || k.tap().is_some()
}

/// Steps of the left stick for menus: -1, 0 or 1 on each axis, only
/// when the stick newly passes half way.
fn stick_steps(stick: (i16, i16), last: &mut (i8, i8)) -> (i8, i8) {
    let dir = |v: i16| -> i8 {
        if v < -16000 {
            -1
        } else if v > 16000 {
            1
        } else {
            0
        }
    };
    let now = (dir(stick.0), dir(stick.1));
    let out = (
        if now.0 != last.0 { now.0 } else { 0 },
        if now.1 != last.1 { now.1 } else { 0 },
    );
    *last = now;
    out
}
