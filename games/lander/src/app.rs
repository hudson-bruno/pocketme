//! The game's state and rules, apart from the console: the title menu,
//! flying, landing, breaking, scores. [`App::update`] takes one call's
//! input and the game time; [`App::draw`] (in `draw.rs`) draws it all.
//! Tests drive these two directly, with no console.

use game_kit::{Touch, button};

use crate::craft::{Controls, Craft, FULL, Touch as Contact, Why};
use crate::fixed::{PX, Rng, STEP, lean_towards, sin_step};
use crate::fx::Bits;
use crate::level::{HZ, World};

/// Physics ticks a second come from `level::HZ` (60). At most this many
/// are run in one call; when the game falls further behind (a long gap
/// between calls), the rest of that time is skipped.
pub const MAX_CATCH_UP: u64 = 4;
/// Craft at the start of a game.
pub const LIVES: u8 = 3;
/// The highest level the title menu lets you start at.
pub const MAX_START_LEVEL: u16 = 20;
/// Ticks: the "LEVEL n" banner before a flight; before the next level
/// can be called up; before a try-again.
pub const READY_TICKS: u32 = 60;
pub const LANDED_WAIT: u32 = 40;
pub const BROKEN_WAIT: u32 = 90;
/// Follow view: zoom in when the feet are this close to the ground
/// (pixels), zoom out again above `ZOOM_OUT`.
pub const ZOOM_IN: i32 = 22;
pub const ZOOM_OUT: i32 = 30;
/// Still view: the camera moves half a screen when the craft comes this
/// close to the picture's side.
pub const PAGE_EDGE: i32 = 20;
pub const PAGE_STEP: i32 = 80;
/// Engine dust: thrown up below this height (pixels).
pub const DUST_HEIGHT: i32 = 24;
/// Stick travel ignored in the middle (wobbly sticks).
pub const DEAD_ZONE: i32 = 4000;
/// How long a note (like "VIEW: FOLLOW") shows, in ticks.
pub const NOTE_TICKS: u32 = 75;

/// One call's input, as the console gave it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    /// [`button`] bits held now, and pressed since the last call.
    pub held: u32,
    pub pressed: u32,
    pub left: (i16, i16),
    pub right: (i16, i16),
    /// Left and right trigger, 0..=32767.
    pub triggers: (u16, u16),
    pub touch: Option<Touch>,
    /// Accelerometer x (4096 = 1 g; right side down reads minus), when
    /// the pad sends motion.
    pub tilt: Option<i16>,
}

impl Input {
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
pub enum View {
    /// The camera stands still and jumps half a screen when the craft
    /// nears a side: few pixels change. The default.
    Still,
    /// The camera follows the craft and zooms in twice near the
    /// ground: the whole picture changes every frame (the heavy case).
    Follow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Steer {
    /// Turn with the stick, the d-pad or the shoulders.
    Stick,
    /// Tilt the pad (motion) to lean the craft; the stick still works.
    Tilt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub level: u16,
    pub view: View,
    pub steer: Steer,
    /// 60 pictures a second instead of 30.
    pub fast: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Title,
    /// "LEVEL n" before a flight.
    Ready,
    Fly,
    Landed,
    /// Crashed or lost.
    Broken,
    Over,
}

/// How the last flight went wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    Crash(Why),
    Lost,
}

/// Something worth a log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Start { level: u16 },
    Landed { level: u16, points: u32, score: u32 },
    Crashed { level: u16, why: Why },
    Lost { level: u16 },
    Over { score: u32, best: u32 },
    View(View),
    Quit,
}

/// The score of one landing, shown on the "LANDED" card.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub mult: u8,
    /// Fuel left, percent.
    pub fuel: u32,
    pub soft: bool,
    pub points: u32,
}

/// Where the picture looks: its top-left in Q16 world pixels, and 1 or 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cam {
    pub x: i32,
    pub y: i32,
    pub zoom: i32,
}

/// Lines of the title menu.
pub const TITLE_LINES: usize = 6;
/// Lines of the game's own pause menu.
pub const PAUSE_LINES: usize = 3;

pub struct App {
    pub scene: Scene,
    /// Ticks since the scene began.
    pub scene_t: u32,
    /// Selected line of the title menu.
    pub menu: u8,
    /// The game's own pause menu, with its selected line.
    pub pause: Option<u8>,
    pub opts: Options,
    pub level: u16,
    pub lives: u8,
    pub score: u32,
    pub best: u32,
    pub world: World,
    pub craft: Craft,
    pub bits: Bits,
    pub cam: Cam,
    /// Physics ticks run (or skipped) since the game started.
    pub ticks: u64,
    /// Ticks of this flight (for the wind).
    pub flight_t: u32,
    pub rng: Rng,
    pub tally: Tally,
    pub ending: Ending,
    /// A short note and the tick it goes away.
    pub note: Option<(&'static str, u32)>,
    /// Ticks since the game started, for notes and blinking (wraps).
    pub clock: u32,
    /// Whether motion was sent in the last call.
    pub has_motion: bool,
    /// The stick's last direction, for menu steps by stick.
    stick_dir: (i8, i8),
    events: [Option<Event>; 4],
}

impl App {
    pub fn new() -> App {
        let world = World::make(1);
        let mut app = App {
            scene: Scene::Title,
            scene_t: 0,
            menu: 0,
            pause: None,
            opts: Options {
                level: 1,
                view: View::Still,
                steer: Steer::Stick,
                fast: true,
            },
            level: 1,
            lives: LIVES,
            score: 0,
            best: 0,
            craft: Craft::new(&world),
            world,
            bits: Bits::new(),
            cam: Cam {
                x: 0,
                y: 0,
                zoom: 1,
            },
            ticks: 0,
            flight_t: 0,
            rng: Rng::new(0x1A2D),
            tally: Tally::default(),
            ending: Ending::Lost,
            note: None,
            clock: 0,
            has_motion: false,
            stick_dir: (0, 0),
            events: [None; 4],
        };
        app.title_craft();
        app
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

    /// Physics ticks due by game time `us` (60 a second, rounded down).
    pub fn ticks_at(us: u64) -> u64 {
        (us / 1_000_000) * u64::from(HZ) + (us % 1_000_000) * u64::from(HZ) / 1_000_000
    }

    /// Whether nothing moves on screen until the player does something:
    /// the console may then call the game only on input.
    pub fn still(&self) -> bool {
        if self.pause.is_some() {
            return true;
        }
        let quiet = self.bits.moving() == 0 && self.note.is_none();
        match self.scene {
            Scene::Over => self.scene_t >= BROKEN_WAIT / 3 && quiet,
            Scene::Landed => self.scene_t >= LANDED_WAIT && quiet,
            Scene::Broken => self.scene_t >= BROKEN_WAIT && quiet && self.lives > 0,
            _ => false,
        }
    }

    /// One call. Returns false when the player chose END.
    pub fn update(&mut self, inp: &Input, now_us: u64) -> bool {
        let due = App::ticks_at(now_us);
        let mut n = due.saturating_sub(self.ticks).min(MAX_CATCH_UP);
        self.ticks = due.max(self.ticks);
        self.has_motion = inp.tilt.is_some();
        let stick = stick_steps(inp.left, &mut self.stick_dir);

        if let Some(sel) = self.pause {
            n = 0;
            if !self.pause_menu(inp, sel, stick) {
                return true;
            }
        } else {
            match self.scene {
                Scene::Title => {
                    if !self.title_menu(inp, stick) {
                        self.event(Event::Quit);
                        return false;
                    }
                }
                Scene::Ready => {
                    if inp.pressed(button::START) {
                        self.pause = Some(0);
                    } else if inp.pressed(button::A) || inp.tap().is_some() {
                        self.go(Scene::Fly);
                    }
                }
                Scene::Fly => {
                    if inp.pressed(button::START) {
                        self.pause = Some(0);
                    } else if inp.pressed(button::SELECT) {
                        self.opts.view = match self.opts.view {
                            View::Still => View::Follow,
                            View::Follow => View::Still,
                        };
                        self.note(match self.opts.view {
                            View::Still => "VIEW: STILL",
                            View::Follow => "VIEW: FOLLOW",
                        });
                        self.event(Event::View(self.opts.view));
                    }
                }
                Scene::Landed => {
                    if inp.pressed(button::START) {
                        self.pause = Some(0);
                    } else if self.scene_t >= LANDED_WAIT && go_on(inp) {
                        self.level = self.level.saturating_add(1);
                        self.begin_level();
                    }
                }
                Scene::Broken => {
                    if inp.pressed(button::START) && self.lives > 0 {
                        self.pause = Some(0);
                    } else if self.scene_t >= BROKEN_WAIT && self.lives > 0 && go_on(inp) {
                        self.begin_level();
                    }
                }
                Scene::Over => {
                    if self.scene_t >= BROKEN_WAIT / 3 && (go_on(inp) || inp.pressed(button::START))
                    {
                        self.back_to_title();
                    }
                }
            }
            if self.pause.is_some() {
                n = 0;
            }
        }

        for _ in 0..n {
            self.tick(inp);
        }
        self.aim_camera();
        true
    }

    /// One physics tick (1/60 s).
    fn tick(&mut self, inp: &Input) {
        self.clock = self.clock.wrapping_add(1);
        self.scene_t = self.scene_t.saturating_add(1);
        if self.note.is_some_and(|(_, until)| self.clock == until) {
            self.note = None;
        }
        self.bits.tick(&self.world);
        match self.scene {
            Scene::Title => {
                // The craft hovers on the title: the flame flickers.
                self.craft.burn = FULL * 3 / 4 + self.rng.spread(FULL / 5);
            }
            Scene::Ready => {
                if self.scene_t >= READY_TICKS {
                    self.go(Scene::Fly);
                }
            }
            Scene::Fly => self.fly(inp),
            Scene::Landed | Scene::Broken => {
                if self.scene == Scene::Broken && self.lives == 0 && self.scene_t >= BROKEN_WAIT {
                    self.game_over();
                }
            }
            Scene::Over => {}
        }
    }

    fn fly(&mut self, inp: &Input) {
        let c = self.controls(inp);
        let wind = self.world.params.wind_at(self.flight_t);
        self.flight_t = self.flight_t.saturating_add(1);
        let had_fuel = self.craft.fuel > 0;
        let out = self.craft.tick(c, &self.world, wind);
        if had_fuel && self.craft.fuel == 0 {
            self.note("NO FUEL");
        }
        // Dust where the blast meets the ground.
        let h = self.craft.height(&self.world);
        let s = self.craft.step();
        if self.craft.burn > 0 && (0..DUST_HEIGHT).contains(&h) && s.abs() < 40 {
            let (cx, cy) = self.craft.pixel();
            let foot = cy + crate::craft::FOOT_Y;
            let dx = -sin_step(s) * h / crate::fixed::ONE * 2;
            let gx = cx + dx;
            let strength = self.craft.burn * 8 / FULL * (DUST_HEIGHT - h) / DUST_HEIGHT;
            if self.clock.is_multiple_of(2) && foot < self.world.ground_at(gx) {
                let gy = self.world.ground_at(gx);
                self.bits.dust(&mut self.rng, gx, gy, strength);
            }
        }
        match out {
            None => {}
            Some(Contact::Landed(i)) => self.landed(i),
            Some(Contact::Crashed(why)) => {
                self.ending = Ending::Crash(why);
                let c = self.craft;
                self.bits.burst(&mut self.rng, c.x, c.y, c.vx, c.vy);
                self.event(Event::Crashed {
                    level: self.level,
                    why,
                });
                self.broken();
            }
            Some(Contact::Lost) => {
                self.ending = Ending::Lost;
                self.event(Event::Lost { level: self.level });
                self.broken();
            }
        }
    }

    /// The pilot's wishes from the input.
    pub fn controls(&self, inp: &Input) -> Controls {
        let dead = |v: i16| -> i32 {
            let v = i32::from(v);
            if v.abs() < DEAD_ZONE {
                0
            } else {
                (v - v.signum() * DEAD_ZONE) * FULL / (FULL - DEAD_ZONE)
            }
        };
        // Thrust: the strongest of the right trigger, the left trigger
        // (half power: a hover on pads whose triggers are only on/off),
        // the left stick pushed up, and A or Up (full).
        let mut thrust = i32::from(inp.triggers.1).max(i32::from(inp.triggers.0) / 2);
        thrust = thrust.max(-dead(inp.left.1));
        if inp.held(button::A | button::UP) {
            thrust = FULL;
        }
        // Turning: the sticks, the d-pad (full), the shoulders (slow).
        let mut turn = dead(inp.left.0);
        if turn == 0 {
            turn = dead(inp.right.0);
        }
        if inp.held(button::LEFT) {
            turn = -FULL;
        }
        if inp.held(button::RIGHT) {
            turn = FULL;
        }
        if inp.held(button::L) {
            turn = -FULL / 4;
        }
        if inp.held(button::R) {
            turn = FULL / 4;
        }
        let mut aim = None;
        if let Some(ax) = inp
            .tilt
            .filter(|_| turn == 0 && self.opts.steer == Steer::Tilt)
        {
            aim = Some(tilt_lean(ax));
        }
        if inp.held(button::B) {
            aim = Some(0);
        }
        // A finger: lean towards it and fire.
        if let Some(t) = inp.touch.filter(|t| t.down) {
            let (sx, sy) = self.to_screen(self.craft.x, self.craft.y);
            let lean = lean_towards(i32::from(t.x) - sx, i32::from(t.y) - sy);
            aim = Some(lean * STEP);
            thrust = FULL;
        }
        Controls { thrust, turn, aim }
    }

    /// World (Q16) to picture pixels with the current camera.
    pub fn to_screen(&self, x: i32, y: i32) -> (i32, i32) {
        let c = self.cam;
        let f = |v: i32, o: i32| {
            ((i64::from(v) - i64::from(o)) * i64::from(c.zoom))
                .div_euclid(i64::from(PX))
                .clamp(-30_000, 30_000) as i32
        };
        (f(x, c.x), f(y, c.y))
    }

    fn landed(&mut self, pad: usize) {
        let p = self.world.pads[pad];
        let fuel = u32::try_from(
            u64::from(self.craft.fuel) * 100 / u64::from(self.world.params.fuel.max(1)),
        )
        .unwrap_or(100);
        let soft = self.flight_soft();
        let base = 50 + fuel + if soft { 25 } else { 0 };
        let points = base.saturating_mul(u32::from(p.mult));
        self.score = self.score.saturating_add(points);
        self.tally = Tally {
            mult: p.mult,
            fuel,
            soft,
            points,
        };
        self.event(Event::Landed {
            level: self.level,
            points,
            score: self.score,
        });
        self.go(Scene::Landed);
    }

    /// The touchdown was very gentle: half the safe speed or less.
    fn flight_soft(&self) -> bool {
        self.craft.touch_vy <= crate::craft::SAFE_DOWN / 2
    }

    fn broken(&mut self) {
        self.lives = self.lives.saturating_sub(1);
        self.go(Scene::Broken);
    }

    fn game_over(&mut self) {
        self.best = self.best.max(self.score);
        self.event(Event::Over {
            score: self.score,
            best: self.best,
        });
        self.go(Scene::Over);
    }

    fn go(&mut self, s: Scene) {
        self.scene = s;
        self.scene_t = 0;
    }

    fn note(&mut self, text: &'static str) {
        self.note = Some((text, self.clock.wrapping_add(NOTE_TICKS)));
    }

    /// A new game from the title.
    fn new_game(&mut self) {
        self.level = self.opts.level;
        self.lives = LIVES;
        self.score = 0;
        self.begin_level();
    }

    /// The level `self.level`, from its start (a new try too).
    fn begin_level(&mut self) {
        self.world = World::make(self.level);
        self.craft = Craft::new(&self.world);
        self.bits.clear();
        self.flight_t = 0;
        self.pause = None;
        self.cam.zoom = 1;
        self.cam.x = page_for(self.craft.x, &self.world);
        self.event(Event::Start { level: self.level });
        self.go(Scene::Ready);
    }

    fn back_to_title(&mut self) {
        self.best = self.best.max(self.score);
        self.pause = None;
        self.world = World::make(1);
        self.bits.clear();
        self.title_craft();
        self.go(Scene::Title);
    }

    /// The craft hovering on the title picture.
    fn title_craft(&mut self) {
        self.craft = Craft::new(&self.world);
        self.craft.x = 118 * PX;
        self.craft.y = 50 * PX;
        self.craft.angle = 0;
        self.cam = Cam {
            x: 0,
            y: 0,
            zoom: 1,
        };
    }

    /// Title menu: PLAY, LEVEL, VIEW, STEER, RATE, END.
    /// Returns false for END.
    fn title_menu(&mut self, inp: &Input, stick: (i8, i8)) -> bool {
        let mut sel = self.menu;
        let mut act = inp.pressed(button::A);
        let mut change = 0;
        if inp.pressed(button::UP) || stick.1 < 0 {
            sel = (sel + TITLE_LINES as u8 - 1) % TITLE_LINES as u8;
        }
        if inp.pressed(button::DOWN) || stick.1 > 0 {
            sel = (sel + 1) % TITLE_LINES as u8;
        }
        if inp.pressed(button::LEFT) || stick.0 < 0 {
            change = -1;
        }
        if inp.pressed(button::RIGHT) || stick.0 > 0 {
            change = 1;
        }
        if inp.pressed(button::START) {
            sel = 0;
            act = true;
        }
        if let Some(line) = inp
            .tap()
            .and_then(|(x, y)| crate::draw::title_line_at(x, y))
        {
            sel = line;
            act = true;
        }
        self.menu = sel;
        if act {
            change = 1;
        }
        match sel {
            0 if act => self.new_game(),
            1 if change != 0 => {
                let n = i32::from(self.opts.level) + change;
                self.opts.level = if n < 1 {
                    MAX_START_LEVEL
                } else if n > i32::from(MAX_START_LEVEL) {
                    1
                } else {
                    n as u16
                };
            }
            2 if change != 0 => {
                self.opts.view = match self.opts.view {
                    View::Still => View::Follow,
                    View::Follow => View::Still,
                }
            }
            3 if change != 0 => {
                self.opts.steer = match self.opts.steer {
                    Steer::Stick => Steer::Tilt,
                    Steer::Tilt => Steer::Stick,
                }
            }
            4 if change != 0 => self.opts.fast = !self.opts.fast,
            5 if act => return false,
            _ => {}
        }
        true
    }

    /// The game's own pause menu: RESUME, RESTART LEVEL, TITLE.
    /// Returns false when it closed in this call.
    fn pause_menu(&mut self, inp: &Input, mut sel: u8, stick: (i8, i8)) -> bool {
        if inp.pressed(button::UP) || stick.1 < 0 {
            sel = (sel + PAUSE_LINES as u8 - 1) % PAUSE_LINES as u8;
        }
        if inp.pressed(button::DOWN) || stick.1 > 0 {
            sel = (sel + 1) % PAUSE_LINES as u8;
        }
        let mut act = inp.pressed(button::A);
        if let Some(line) = inp
            .tap()
            .and_then(|(x, y)| crate::draw::pause_line_at(x, y))
        {
            sel = line;
            act = true;
        }
        if inp.pressed(button::START | button::B) {
            self.pause = None;
            return false;
        }
        self.pause = Some(sel);
        if act {
            match sel {
                0 => self.pause = None,
                1 => self.begin_level(),
                _ => self.back_to_title(),
            }
            return false;
        }
        true
    }

    /// Moves the camera for this picture.
    fn aim_camera(&mut self) {
        let w = i32::from(self.world.params.width) * PX;
        let (cx, cy) = (self.craft.x, self.craft.y);
        let flying = matches!(
            self.scene,
            Scene::Ready | Scene::Fly | Scene::Landed | Scene::Broken
        );
        if self.scene == Scene::Title || self.scene == Scene::Over {
            self.cam = Cam {
                x: 0,
                y: 0,
                zoom: 1,
            };
            return;
        }
        match self.opts.view {
            View::Still => {
                self.cam.zoom = 1;
                self.cam.y = 0;
                let sx = (cx - self.cam.x) / PX;
                if sx < PAGE_EDGE {
                    self.cam.x -= PAGE_STEP * PX;
                } else if sx > 160 - PAGE_EDGE {
                    self.cam.x += PAGE_STEP * PX;
                }
                self.cam.x = self.cam.x.clamp(0, w - 160 * PX);
            }
            View::Follow => {
                let h = self.craft.height(&self.world);
                if self.scene != Scene::Broken {
                    if self.cam.zoom == 1 && flying && h < ZOOM_IN {
                        self.cam.zoom = 2;
                    } else if self.cam.zoom == 2 && h > ZOOM_OUT {
                        self.cam.zoom = 1;
                    }
                }
                if self.cam.zoom == 2 {
                    self.cam.x = (cx - 40 * PX).clamp(0, w - 80 * PX);
                    self.cam.y = (cy - 24 * PX).clamp(0, 60 * PX);
                } else {
                    self.cam.x = (cx - 80 * PX).clamp(0, w - 160 * PX);
                    self.cam.y = 0;
                }
            }
        }
    }
}

impl Default for App {
    fn default() -> Self {
        App::new()
    }
}

/// Any of the "go on" inputs: A, a tap.
fn go_on(inp: &Input) -> bool {
    inp.pressed(button::A) || inp.tap().is_some()
}

/// The camera's left edge (Q16) that shows the craft at `x` in the Still
/// view: a multiple of half a screen.
fn page_for(x: i32, world: &World) -> i32 {
    let w = i32::from(world.params.width) * PX;
    let page = ((x / PX - 80) / PAGE_STEP).max(0) * PAGE_STEP * PX;
    let page = if x / PX - page / PX > 160 - PAGE_EDGE {
        page + PAGE_STEP * PX
    } else {
        page
    };
    page.clamp(0, (w - 160 * PX).max(0))
}

/// The lean (Q8) for an accelerometer x reading: about 60 degrees of
/// craft for 30 degrees of pad. Little tilts count as level.
pub fn tilt_lean(ax: i16) -> i32 {
    let ax = i32::from(ax);
    if ax.abs() < 200 {
        return 0;
    }
    (-ax * STEP / 48).clamp(-crate::fixed::QUARTER, crate::fixed::QUARTER)
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
