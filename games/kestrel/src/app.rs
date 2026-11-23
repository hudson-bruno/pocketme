//! The game's screens and rules round the world: the title, the
//! briefing, flying, the game's own pause menu, the result of a mission,
//! the end. [`App::update`] takes one call's [`Keys`] and the game time;
//! [`App::draw`] draws everything. Tests drive these two directly, with
//! no console.
//!
//! **Every setting can be changed while flying** (the owner's wish):
//! X steps the detail, Y the view, SELECT the picture rate, and the
//! pause menu (START or the II box) has all three as well.

use game_kit::{Frame, Touch, button};

use crate::course::MISSIONS;
use crate::fixed::{Mat3, deg};
use crate::hud::{self, BoxKey};
use crate::models::SHIP;
use crate::poly::{Camera, M8, Object, Rect, Renderer, Stats, View};
use crate::scene::{self, Detail, Eye};
use crate::world::{Event as WorldEvent, HZ, Pad, State, TICK_US, World};

/// At most this many ticks are run in one call; a longer gap between
/// calls is skipped (the game slows down instead of jumping).
pub const MAX_CATCH_UP: u64 = 4;
/// Stick travel ignored in the middle (worn sticks drift).
pub const DEAD_ZONE: i32 = 4000;
/// Ticks before a card takes a press.
pub const CARD_WAIT: u32 = 40;
/// How often a cost line goes to the log while flying (ticks: 4 s).
pub const COST_EVERY: u32 = 240;
/// Lines of the title menu: PLAY, MISSION, DETAIL, VIEW, PICTURES, LEAVE.
pub const TITLE_LINES: u8 = 6;
/// Lines of the pause menu: RESUME, DETAIL, VIEW, PICTURES, RESTART, TITLE.
pub const PAUSE_LINES: u8 = 6;
/// Ticks a note stays.
pub const NOTE_TICKS: u16 = 75;

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
    /// The mission's briefing card.
    Brief,
    Play,
    /// The mission is over (delivered or not): its card.
    Result,
    /// After the last mission.
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// The mission PLAY starts at (0-based).
    pub mission: u8,
    pub detail: Detail,
    pub eye: Eye,
    /// 60 pictures a second instead of 30.
    pub fast: bool,
}

/// Something worth a log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Start {
        mission: u8,
        detail: Detail,
        eye: Eye,
    },
    Over {
        mission: u8,
        state: State,
        score: u32,
        ticks: u32,
    },
    Settings {
        detail: Detail,
        eye: Eye,
        fast: bool,
    },
    /// The renderer's counts for the last picture, now and then.
    Cost {
        detail: Detail,
        stats: Stats,
    },
    End {
        score: u32,
    },
    Quit,
}

/// Totals over the run (missions played one after the other).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Run {
    pub score: u32,
    pub gates: u32,
    pub gates_total: u32,
    pub cells: u32,
    pub hits: u32,
    pub retries: u32,
}

pub struct App {
    pub scene: Scene,
    /// Ticks since the scene began.
    pub scene_t: u32,
    /// Selected line of the title menu.
    pub menu: u8,
    /// The game's own pause menu, with its selected line.
    pub pause: Option<u8>,
    pub opts: Options,
    pub world: World,
    pub render: Renderer,
    pub run: Run,
    /// Ticks run (or skipped) since the game started.
    pub ticks: u64,
    /// A short note (like "DETAIL: MID") and the ticks it has left.
    pub note: Option<(&'static str, u16)>,
    /// The speed chosen with the touch boxes: -1 slow, 0 cruise, 1 fast.
    pub touch_throttle: i8,
    /// A roll asked for, waiting for the next tick.
    pending_roll: i8,
    /// The sticks' last directions, for menu steps and stick rolls.
    stick_dir: (i8, i8),
    right_dir: (i8, i8),
    /// The last tick's controls (tests read them).
    pub last_pad: Pad,
    /// The throttle shown on the boxes (-1, 0, 1).
    shown_throttle: i8,
    events: [Option<Event>; 4],
}

impl Default for App {
    fn default() -> Self {
        App::new()
    }
}

impl App {
    /// The game with mission 1 loaded (tests use this).
    pub fn new() -> App {
        let mut app = App::blank();
        app.load();
        app
    }

    /// The game before any mission is loaded: a `const`, so the chip can
    /// build it without a second copy on the stack. Call [`App::load`]
    /// before the first update.
    pub const fn blank() -> App {
        App {
            scene: Scene::Title,
            scene_t: 0,
            menu: 0,
            pause: None,
            opts: Options {
                mission: 0,
                detail: Detail::High,
                eye: Eye::Chase,
                fast: true,
            },
            world: World::empty(),
            render: Renderer::new(),
            run: Run {
                score: 0,
                gates: 0,
                gates_total: 0,
                cells: 0,
                hits: 0,
                retries: 0,
            },
            ticks: 0,
            note: None,
            touch_throttle: 0,
            pending_roll: 0,
            stick_dir: (0, 0),
            right_dir: (0, 0),
            last_pad: Pad {
                steer: (0, 0),
                throttle: 0,
                roll: 0,
            },
            shown_throttle: 0,
            events: [None; 4],
        }
    }

    /// Loads mission 1 in place, as the title shows it.
    pub fn load(&mut self) {
        self.world.reset(0);
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
        us / u64::from(TICK_US)
    }

    /// Whether nothing moves on screen until the player does something:
    /// the console may then call the game only on input.
    pub fn still(&self) -> bool {
        if self.pause.is_some() {
            return self.note.is_none();
        }
        match self.scene {
            Scene::Title | Scene::Play => false,
            Scene::Brief | Scene::Result => self.scene_t >= CARD_WAIT && self.note.is_none(),
            Scene::End => self.note.is_none(),
        }
    }

    /// One call. Returns false when the player chose LEAVE.
    pub fn update(&mut self, k: &Keys, now_us: u64) -> bool {
        let due = App::ticks_at(now_us);
        let mut n = due.saturating_sub(self.ticks).min(MAX_CATCH_UP);
        self.ticks = due.max(self.ticks);
        let stick = stick_steps(k.left, &mut self.stick_dir);
        let right = stick_steps(k.right, &mut self.right_dir);
        if let Some((t, left)) = self.note {
            let left = u64::from(left).saturating_sub(n);
            self.note = (left > 0).then_some((t, left as u16));
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
                Scene::Brief => {
                    if self.scene_t >= CARD_WAIT && (go_on(k) || k.pressed(button::START)) {
                        self.go(Scene::Play);
                    } else if k.pressed(button::B) {
                        self.back_to_title();
                    }
                }
                Scene::Play => self.play_keys(k, right),
                Scene::Result => {
                    if self.scene_t >= CARD_WAIT {
                        self.result_keys(k);
                    }
                }
                Scene::End => {
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

    /// The buttons that are not steering, while flying.
    fn play_keys(&mut self, k: &Keys, right: (i8, i8)) {
        if k.pressed(button::START) {
            self.pause = Some(0);
            return;
        }
        if k.pressed(button::X) {
            self.opts.detail = self.opts.detail.next();
            self.settings_changed("DETAIL");
        }
        if k.pressed(button::Y) {
            self.opts.eye = self.opts.eye.next();
            self.settings_changed("VIEW");
        }
        if k.pressed(button::SELECT) {
            self.opts.fast = !self.opts.fast;
            self.settings_changed("PICTURES");
        }
        if k.pressed(button::L) || right.0 < 0 {
            self.pending_roll = -1;
        }
        if k.pressed(button::R) || right.0 > 0 {
            self.pending_roll = 1;
        }
        if let Some((x, y)) = k.tap() {
            match hud::box_at(x, y) {
                Some(BoxKey::RollLeft) => self.pending_roll = -1,
                Some(BoxKey::RollRight) => self.pending_roll = 1,
                Some(BoxKey::Pause) => self.pause = Some(0),
                Some(BoxKey::Slow) => {
                    self.touch_throttle = if self.touch_throttle < 0 { 0 } else { -1 };
                }
                Some(BoxKey::Fast) => {
                    self.touch_throttle = if self.touch_throttle > 0 { 0 } else { 1 };
                }
                None => {}
            }
        }
    }

    fn settings_changed(&mut self, what: &'static str) {
        let text = match what {
            "DETAIL" => match self.opts.detail {
                Detail::Low => "DETAIL: LOW",
                Detail::Mid => "DETAIL: MID",
                Detail::High => "DETAIL: HIGH",
            },
            "VIEW" => match self.opts.eye {
                Eye::Chase => "VIEW: CHASE",
                Eye::Wide => "VIEW: WIDE",
                Eye::Nose => "VIEW: NOSE",
            },
            _ => {
                if self.opts.fast {
                    "60 PICTURES A SECOND"
                } else {
                    "30 PICTURES A SECOND"
                }
            }
        };
        self.note(text);
        self.event(Event::Settings {
            detail: self.opts.detail,
            eye: self.opts.eye,
            fast: self.opts.fast,
        });
    }

    fn result_keys(&mut self, k: &Keys) {
        let n = self.world.mission;
        if self.world.state == State::Delivered {
            if go_on(k) || k.pressed(button::START) {
                if usize::from(n) + 1 < MISSIONS.len() {
                    self.begin_mission(n + 1);
                } else {
                    self.event(Event::End {
                        score: self.run.score,
                    });
                    self.go(Scene::End);
                }
            }
            return;
        }
        let mut again = k.pressed(button::A | button::START);
        let mut title = k.pressed(button::B);
        if let Some((x, y)) = k.tap() {
            match hud::fail_box_at(x, y) {
                Some(true) => again = true,
                Some(false) => title = true,
                None => {}
            }
        }
        if again {
            self.run.retries += 1;
            self.begin_mission(n);
        } else if title {
            self.back_to_title();
        }
    }

    /// One tick (1/60 s).
    fn tick(&mut self, k: &Keys) {
        self.scene_t = self.scene_t.saturating_add(1);
        if self.scene != Scene::Play {
            return;
        }
        let mut pad = self.controls(k);
        pad.roll = core::mem::take(&mut self.pending_roll);
        self.last_pad = pad;
        self.world.tick(&pad);
        while let Some(e) = self.world.take_event() {
            match e {
                WorldEvent::Gate(_) => self.note("GATE  +TIME"),
                WorldEvent::Missed(_) => self.note("GATE MISSED"),
                WorldEvent::Ring(_) => self.note("HIT THE RING"),
                WorldEvent::Hit => self.note("HULL HIT"),
                WorldEvent::Cell => self.note("TIME CELL +2.0"),
                WorldEvent::Delivered
                | WorldEvent::OutOfTime
                | WorldEvent::Lost
                | WorldEvent::MissedDock => self.mission_over(),
            }
        }
        if self.world.ticks.is_multiple_of(COST_EVERY) {
            self.event(Event::Cost {
                detail: self.opts.detail,
                stats: self.render.stats,
            });
        }
    }

    fn mission_over(&mut self) {
        let w = &self.world;
        let t = w.tally;
        self.run.hits += u32::from(t.hits);
        if w.state == State::Delivered {
            self.run.score += w.score;
            self.run.gates += u32::from(t.passed);
            self.run.gates_total += u32::from(w.m().gates);
            self.run.cells += u32::from(t.cells);
        }
        self.event(Event::Over {
            mission: w.mission + 1,
            state: w.state,
            score: w.score,
            ticks: w.ticks,
        });
        self.note = None;
        self.go(Scene::Result);
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
        // Steering: left stick (up reads minus) or the d-pad.
        let mut sx = dead(k.left.0);
        let mut sy = -dead(k.left.1);
        if k.held(button::LEFT) {
            sx = -256;
        }
        if k.held(button::RIGHT) {
            sx = 256;
        }
        if k.held(button::UP) {
            sy = 256;
        }
        if k.held(button::DOWN) {
            sy = -256;
        }
        // Speed: A or the right trigger boost, B or the left trigger
        // brake, the right stick up and down does both; the touch boxes
        // set it too.
        let trig = |t: u16| i32::from(t) * 256 / 32_767;
        let mut throttle = trig(k.triggers.1) - trig(k.triggers.0) - dead(k.right.1);
        if k.held(button::A) {
            throttle = throttle.max(256);
        }
        if k.held(button::B) {
            throttle = throttle.min(-256);
        }
        if throttle == 0 {
            throttle = i32::from(self.touch_throttle) * 256;
        }
        // A finger on the view: the craft flies towards it.
        if let Some(t) = k.touch.filter(|t| t.down)
            && hud::on_view(self.opts.detail, t.x, t.y)
        {
            let (px, py) = scene::ship_on_screen(&self.world, self.opts.detail, self.opts.eye);
            let reach = 24;
            sx = ((i32::from(t.x) - px) * 256 / reach).clamp(-256, 256);
            sy = ((py - i32::from(t.y)) * 256 / reach).clamp(-256, 256);
        }
        Pad {
            steer: (sx.clamp(-256, 256), sy.clamp(-256, 256)),
            throttle: throttle.clamp(-256, 256),
            roll: 0,
        }
    }

    fn go(&mut self, s: Scene) {
        self.scene = s;
        self.scene_t = 0;
    }

    fn note(&mut self, text: &'static str) {
        self.note = Some((text, NOTE_TICKS));
    }

    /// Mission `n` (0-based) from its start (a new try too): the
    /// briefing first.
    fn begin_mission(&mut self, n: u8) {
        self.world.reset(usize::from(n));
        self.pause = None;
        self.pending_roll = 0;
        self.touch_throttle = 0;
        self.note = None;
        self.event(Event::Start {
            mission: n + 1,
            detail: self.opts.detail,
            eye: self.opts.eye,
        });
        self.go(Scene::Brief);
    }

    fn back_to_title(&mut self) {
        self.pause = None;
        self.note = None;
        self.world.reset(usize::from(self.opts.mission));
        self.go(Scene::Title);
    }

    /// Title menu. False for LEAVE.
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
        if let Some(line) = k.tap().and_then(|(x, y)| hud::title_line_at(x, y)) {
            sel = line;
            act = true;
        }
        self.menu = sel;
        if act {
            change = 1;
        }
        match sel {
            0 if act => {
                self.run = Run::default();
                self.begin_mission(self.opts.mission);
            }
            1 if change != 0 => {
                let n = MISSIONS.len() as i32;
                self.opts.mission = (i32::from(self.opts.mission) + change).rem_euclid(n) as u8;
                self.world.reset(usize::from(self.opts.mission));
            }
            2 if change != 0 => self.opts.detail = step(&Detail::ALL, self.opts.detail, change),
            3 if change != 0 => self.opts.eye = step(&Eye::ALL, self.opts.eye, change),
            4 if change != 0 => self.opts.fast = !self.opts.fast,
            5 if act => return false,
            _ => {}
        }
        true
    }

    /// The pause menu: RESUME, DETAIL, VIEW, PICTURES, RESTART, TITLE.
    fn pause_menu(&mut self, k: &Keys, mut sel: u8, stick: (i8, i8)) {
        if k.pressed(button::UP) || stick.1 < 0 {
            sel = (sel + PAUSE_LINES - 1) % PAUSE_LINES;
        }
        if k.pressed(button::DOWN) || stick.1 > 0 {
            sel = (sel + 1) % PAUSE_LINES;
        }
        let mut act = k.pressed(button::A);
        let mut change = 0i32;
        if k.pressed(button::LEFT) || stick.0 < 0 {
            change = -1;
        }
        if k.pressed(button::RIGHT) || stick.0 > 0 {
            change = 1;
        }
        if let Some((x, y)) = k.tap() {
            match hud::pause_line_at(x, y) {
                Some(line) => {
                    sel = line;
                    act = true;
                }
                // A tap on the card but not on a line (its PAUSED
                // heading, its edges) does nothing.
                None if hud::on_pause_card(x, y) => {}
                // A tap beside the card goes back to flying.
                None => {
                    self.pause = None;
                    return;
                }
            }
        }
        if k.pressed(button::START | button::B) {
            self.pause = None;
            return;
        }
        self.pause = Some(sel);
        if act {
            change = 1;
        }
        match sel {
            0 if act => self.pause = None,
            1 if change != 0 => {
                self.opts.detail = step(&Detail::ALL, self.opts.detail, change);
                self.settings_changed("DETAIL");
            }
            2 if change != 0 => {
                self.opts.eye = step(&Eye::ALL, self.opts.eye, change);
                self.settings_changed("VIEW");
            }
            3 if change != 0 => {
                self.opts.fast = !self.opts.fast;
                self.settings_changed("PICTURES");
            }
            4 if act => {
                self.run.retries += 1;
                self.begin_mission(self.world.mission);
            }
            5 if act => self.back_to_title(),
            _ => {}
        }
    }

    /// The throttle the boxes show: -1, 0 or 1.
    fn throttle_shown(&self) -> i8 {
        let t = self.last_pad.throttle;
        if self.scene != Scene::Play {
            0
        } else if t > 64 {
            1
        } else if t < -64 {
            -1
        } else {
            0
        }
    }

    /// Draws the whole picture.
    pub fn draw(&mut self, s: &mut Frame) {
        self.shown_throttle = self.throttle_shown();
        if self.scene == Scene::Title {
            self.draw_title(s);
            return;
        }
        let d = self.opts.detail;
        if d == Detail::Low {
            hud::low_frame(s);
        }
        scene::draw(s, &mut self.render, &self.world, d, self.opts.eye);
        hud::view_marks(s, d, self.opts.eye);
        hud::top_bar(s, &self.world);
        hud::bottom_bar(s, self.shown_throttle);
        let ready = self.scene_t >= CARD_WAIT;
        match self.scene {
            Scene::Brief => hud::brief(s, self.world.mission, ready),
            Scene::Result => hud::result(
                s,
                &self.world,
                ready,
                usize::from(self.world.mission) + 1 == MISSIONS.len(),
            ),
            Scene::End => hud::end(s, &self.run),
            _ => {}
        }
        if let Some(sel) = self.pause {
            hud::pause(s, sel, &self.opts);
        }
        if let Some((text, _)) = self.note {
            hud::note(s, text, d, self.pause.is_some());
        }
    }

    /// The title: stars, the craft turning slowly, the menu.
    fn draw_title(&mut self, s: &mut Frame) {
        let all = Rect::new(0, 0, 160, 120);
        crate::art::backdrop(s, &all);
        let view = View::new(Rect::new(20, 14, 120, 46), 70);
        let cam = Camera {
            pos: [0, M8 * 7 / 10, -4 * M8],
            rot: Mat3::IDENTITY,
        };
        let turn = (self.ticks as u32).wrapping_mul(180) as u16;
        let o = Object {
            model: &SHIP,
            pos: [0, 0, 0],
            rot: Mat3::rotation(turn, deg(-8), deg(10)),
            scale: 256,
            swap: (0, 0),
        };
        self.render.begin();
        let sh = scene::shading(Detail::High);
        self.render.draw(s, &view, &cam, &sh, &o, 40 * M8);
        hud::title(s, self.menu, &self.opts);
        if let Some((text, _)) = self.note {
            hud::note(s, text, Detail::Mid, false);
        }
    }

    /// Seconds flown in this mission, in tenths.
    pub fn flown_s10(&self) -> u32 {
        self.world.ticks * 10 / HZ
    }
}

/// The value after (or before) `now` in `all`.
fn step<T: Copy + PartialEq>(all: &[T], now: T, change: i32) -> T {
    let i = all.iter().position(|v| *v == now).unwrap_or(0) as i32;
    all[(i + change).rem_euclid(all.len() as i32) as usize]
}

/// Any of the "go on" inputs: A, a tap.
fn go_on(k: &Keys) -> bool {
    k.pressed(button::A) || k.tap().is_some()
}

/// Steps of a stick for menus (and rolls): -1, 0 or 1 on each axis,
/// only when the stick newly passes half way.
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
