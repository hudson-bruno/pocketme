//! The game's screens and rules around the world: the title menu,
//! playing, the map, the game's own pause menu, a cleared level, a
//! fainted player, the end. [`App::update`] takes one call's [`Keys`]
//! and the game time; [`App::draw`] draws everything. Tests drive these
//! two directly, with no console.

use game_kit::{Frame, Touch, button};

use crate::hud;
use crate::level::LEVELS;
use crate::render::{Cost, Detail, Renderer};
use crate::world::{Event as WorldEvent, Pad, TICK_US, World};

/// At most this many ticks are run in one call; a longer gap between
/// calls is skipped (the game slows down instead of jumping).
pub const MAX_CATCH_UP: u64 = 4;
/// Stick travel ignored in the middle (worn sticks drift).
pub const DEAD_ZONE: i32 = 4000;
/// Ticks before a "LEVEL CLEAR" or "THE LAMP FALLS" card takes a press.
pub const CARD_WAIT: u32 = 40;
/// How often a cost line goes to the log while playing (ticks: 4 s).
pub const COST_EVERY: u32 = 240;
/// Lines of the title menu: PLAY, LEVEL, DETAIL, RATE, LEAVE.
pub const TITLE_LINES: u8 = 5;
/// Lines of the game's pause menu: RESUME, RESTART LEVEL, TITLE.
pub const PAUSE_LINES: u8 = 3;

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
    Play,
    /// The map of what the lamp has shown; the world waits.
    Map,
    /// The lift was worked: the level's tally.
    Clear,
    /// The last heart went: the level starts again.
    Fainted,
    /// After the last level.
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// The level PLAY starts at (0-based).
    pub level: u8,
    pub detail: Detail,
    /// 60 pictures a second instead of 30.
    pub fast: bool,
}

/// Something worth a log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Start {
        level: u8,
        detail: Detail,
    },
    Clear {
        level: u8,
        crystals: u16,
        of: u16,
        seconds: u32,
    },
    Fainted {
        level: u8,
    },
    Detail(Detail),
    /// The renderer's counts for the last picture, now and then.
    Cost {
        detail: Detail,
        cost: Cost,
    },
    End {
        crystals: u32,
        retries: u32,
    },
    Quit,
}

/// Totals over the whole run (all levels).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Run {
    pub crystals: u32,
    pub crystals_total: u32,
    pub retries: u32,
    pub moths: u32,
    pub stuns: u32,
    /// Ticks played.
    pub ticks: u32,
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
    /// A short note (like "DETAIL: HEAVY") and the ticks it has left.
    pub note: Option<(&'static str, u16)>,
    /// Presses waiting for the next tick (a call may run no tick).
    pending_act: bool,
    pending_flash: bool,
    /// The stick's last direction, for menu steps by stick.
    stick_dir: (i8, i8),
    /// The last tick's controls (tests read them).
    pub last_pad: Pad,
    events: [Option<Event>; 4],
}

impl Default for App {
    fn default() -> Self {
        App::new()
    }
}

impl App {
    /// The game with level 1 loaded (tests and the simulator use this).
    pub fn new() -> App {
        let mut app = App::blank();
        app.load_title_level();
        app
    }

    /// The game before any level is loaded: a `const`, so the chip can
    /// build it without a second 6 KB copy on the stack. Call
    /// [`App::load_title_level`] before the first update.
    pub const fn blank() -> App {
        App {
            scene: Scene::Title,
            scene_t: 0,
            menu: 0,
            pause: None,
            opts: Options {
                level: 0,
                detail: Detail::Heavy,
                fast: true,
            },
            world: World::empty(),
            render: Renderer::new(),
            run: Run {
                crystals: 0,
                crystals_total: 0,
                retries: 0,
                moths: 0,
                stuns: 0,
                ticks: 0,
            },
            ticks: 0,
            note: None,
            pending_act: false,
            pending_flash: false,
            stick_dir: (0, 0),
            last_pad: Pad {
                forward: 0,
                turn: 0,
                strafe: 0,
                act: false,
                flash: false,
            },
            events: [None; 4],
        }
    }

    /// Loads level 1 in place, as the title shows it.
    pub fn load_title_level(&mut self) {
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
            Scene::Title | Scene::Map | Scene::End => self.note.is_none(),
            Scene::Clear | Scene::Fainted => self.scene_t >= CARD_WAIT,
            Scene::Play => false,
        }
    }

    /// One call. Returns false when the player chose LEAVE.
    pub fn update(&mut self, k: &Keys, now_us: u64) -> bool {
        let due = App::ticks_at(now_us);
        let mut n = due.saturating_sub(self.ticks).min(MAX_CATCH_UP);
        self.ticks = due.max(self.ticks);
        let stick = stick_steps(k.left, &mut self.stick_dir);
        // Notes run on game time, whatever the scene.
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
                Scene::Play => self.play_keys(k),
                Scene::Map => {
                    n = 0;
                    if k.pressed(button::SELECT | button::B | button::A) || k.tap().is_some() {
                        self.go(Scene::Play);
                    } else if k.pressed(button::START) {
                        self.go(Scene::Play);
                        self.pause = Some(0);
                    }
                }
                Scene::Clear => {
                    if self.scene_t >= CARD_WAIT && go_on(k) {
                        let next = usize::from(self.world.level as u8) + 1;
                        if next < LEVELS.len() {
                            self.begin_level(next as u8);
                        } else {
                            self.event(Event::End {
                                crystals: self.run.crystals,
                                retries: self.run.retries,
                            });
                            self.go(Scene::End);
                        }
                    }
                }
                Scene::Fainted => {
                    if self.scene_t >= CARD_WAIT && go_on(k) {
                        self.begin_level(self.world.level as u8);
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

    /// The buttons that are not movement, while playing.
    fn play_keys(&mut self, k: &Keys) {
        if k.pressed(button::START) {
            self.pause = Some(0);
            return;
        }
        if k.pressed(button::SELECT) {
            self.go(Scene::Map);
            return;
        }
        if k.pressed(button::X) {
            let i = Detail::ALL
                .iter()
                .position(|d| *d == self.opts.detail)
                .unwrap_or(0);
            self.opts.detail = Detail::ALL[(i + 1) % Detail::ALL.len()];
            self.note(match self.opts.detail {
                Detail::Light => "DETAIL: LIGHT",
                Detail::Normal => "DETAIL: NORMAL",
                Detail::Heavy => "DETAIL: HEAVY",
            });
            self.event(Event::Detail(self.opts.detail));
        }
        if k.pressed(button::A) {
            self.pending_act = true;
        }
        if k.pressed(button::B | button::Y) {
            self.pending_flash = true;
        }
        if let Some((x, y)) = k.tap() {
            match hud::status_box_at(x, y) {
                Some(hud::StatusBox::Flash) => self.pending_flash = true,
                Some(hud::StatusBox::Map) => self.go(Scene::Map),
                Some(hud::StatusBox::Pause) => self.pause = Some(0),
                None => {
                    if hud::is_use_spot(self.opts.detail, x, y) {
                        self.pending_act = true;
                    }
                }
            }
        }
    }

    /// One tick (1/60 s).
    fn tick(&mut self, k: &Keys) {
        self.scene_t = self.scene_t.saturating_add(1);
        if self.scene != Scene::Play {
            return;
        }
        let mut pad = self.controls(k);
        pad.act = core::mem::take(&mut self.pending_act);
        pad.flash = core::mem::take(&mut self.pending_flash);
        self.last_pad = pad;
        self.run.ticks = self.run.ticks.saturating_add(1);
        match self.world.tick(&pad) {
            None => {}
            Some(WorldEvent::Clear) => {
                let t = self.world.tally;
                self.run.crystals += u32::from(t.crystals);
                self.run.crystals_total += u32::from(t.crystals_total);
                self.run.moths += u32::from(t.moths_sent);
                self.run.stuns += u32::from(t.stuns);
                self.event(Event::Clear {
                    level: self.world.level as u8 + 1,
                    crystals: t.crystals,
                    of: t.crystals_total,
                    seconds: self.world.ticks / 60,
                });
                self.go(Scene::Clear);
            }
            Some(WorldEvent::Fainted) => {
                self.run.retries += 1;
                self.event(Event::Fainted {
                    level: self.world.level as u8 + 1,
                });
                self.go(Scene::Fainted);
            }
        }
        if self.world.ticks.is_multiple_of(COST_EVERY) {
            self.event(Event::Cost {
                detail: self.opts.detail,
                cost: self.render.cost,
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
        // Twin sticks: the left one walks (up and down) and steps
        // sideways (left and right); the right one turns. Stick up
        // reads minus.
        let mut forward = -dead(k.left.1);
        let mut strafe = dead(k.left.0);
        let mut turn = dead(k.right.0);
        if k.held(button::UP) {
            forward = 256;
        }
        if k.held(button::DOWN) {
            forward = -256;
        }
        if k.held(button::LEFT) {
            turn = -256;
        }
        if k.held(button::RIGHT) {
            turn = 256;
        }
        // Side steps: shoulders (full) or triggers (as far as pressed).
        let trig = |t: u16| i32::from(t) * 256 / 32_767;
        strafe += trig(k.triggers.1) - trig(k.triggers.0);
        if k.held(button::L) {
            strafe = -256;
        }
        if k.held(button::R) {
            strafe = 256;
        }
        // A finger on the view: up walks, down backs off, the sides turn.
        if let Some(t) = k.touch.filter(|t| t.down)
            && let Some((f, r)) = hud::steer_at(self.opts.detail, t.x, t.y)
        {
            forward = f;
            turn = r;
        }
        Pad {
            forward: forward.clamp(-256, 256),
            turn: turn.clamp(-256, 256),
            strafe: strafe.clamp(-256, 256),
            act: false,
            flash: false,
        }
    }

    fn go(&mut self, s: Scene) {
        self.scene = s;
        self.scene_t = 0;
    }

    fn note(&mut self, text: &'static str) {
        self.note = Some((text, 75));
    }

    /// Level `n` (0-based) from its start (a new try too).
    fn begin_level(&mut self, n: u8) {
        self.world.reset(usize::from(n));
        self.pause = None;
        self.pending_act = false;
        self.pending_flash = false;
        self.event(Event::Start {
            level: n + 1,
            detail: self.opts.detail,
        });
        self.go(Scene::Play);
    }

    fn back_to_title(&mut self) {
        self.pause = None;
        self.world.reset(0);
        self.go(Scene::Title);
    }

    /// Title menu: PLAY, LEVEL, DETAIL, RATE, LEAVE. False for LEAVE.
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
                self.begin_level(self.opts.level);
            }
            1 if change != 0 => {
                let n = LEVELS.len() as i32;
                self.opts.level = (i32::from(self.opts.level) + change).rem_euclid(n) as u8;
            }
            2 if change != 0 => {
                let i = Detail::ALL
                    .iter()
                    .position(|d| *d == self.opts.detail)
                    .unwrap_or(0) as i32;
                let n = Detail::ALL.len() as i32;
                self.opts.detail = Detail::ALL[(i + change).rem_euclid(n) as usize];
            }
            3 if change != 0 => self.opts.fast = !self.opts.fast,
            4 if act => return false,
            _ => {}
        }
        true
    }

    /// The game's pause menu: RESUME, RESTART LEVEL, TITLE.
    fn pause_menu(&mut self, k: &Keys, mut sel: u8, stick: (i8, i8)) {
        if k.pressed(button::UP) || stick.1 < 0 {
            sel = (sel + PAUSE_LINES - 1) % PAUSE_LINES;
        }
        if k.pressed(button::DOWN) || stick.1 > 0 {
            sel = (sel + 1) % PAUSE_LINES;
        }
        let mut act = k.pressed(button::A);
        if let Some(line) = k.tap().and_then(|(x, y)| hud::pause_line_at(x, y)) {
            sel = line;
            act = true;
        }
        if k.pressed(button::START | button::B) {
            self.pause = None;
            return;
        }
        self.pause = Some(sel);
        if act {
            match sel {
                0 => self.pause = None,
                1 => {
                    self.run.retries += 1;
                    self.begin_level(self.world.level as u8);
                }
                _ => self.back_to_title(),
            }
        }
    }

    /// Draws the whole picture.
    pub fn draw(&mut self, s: &mut Frame) {
        match self.scene {
            Scene::Map => {
                hud::map(s, &self.world);
                hud::status(s, &self.world, self.opts.detail);
            }
            Scene::End => hud::end(s, &self.run),
            _ => {
                let detail = if self.scene == Scene::Title {
                    Detail::Light
                } else {
                    self.opts.detail
                };
                if detail == Detail::Light {
                    hud::light_frame(s, &self.world);
                }
                self.render.draw(s, &self.world, detail);
                hud::view_marks(s, &self.world, detail);
                hud::status(s, &self.world, detail);
                match self.scene {
                    Scene::Title => hud::title(s, self.menu, &self.opts),
                    Scene::Clear => hud::clear_card(s, &self.world, self.scene_t >= CARD_WAIT),
                    Scene::Fainted => hud::faint_card(s, self.scene_t >= CARD_WAIT),
                    _ => {}
                }
            }
        }
        if let Some(sel) = self.pause {
            hud::pause(s, sel);
        }
        if let Some((text, _)) = self.note {
            hud::note(s, text, self.opts.detail);
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
