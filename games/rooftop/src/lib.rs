//! ROOFTOP: a precision platformer. A night courier crosses the city's
//! roofs, room by room: run, jump, slide down walls and jump off them,
//! and dash once in the air. Broken glass and the street below send the
//! runner back to the start of the room at once; there are no lives.
//!
//! How the crate is built:
//!
//! * [`sim`]: the physics, in whole numbers with a fixed 1/60 s tick.
//! * [`rooms`]: the rooms, as small tables of letters.
//! * [`draw`]: the palette, the pixel art and the painting.
//! * [`Rooftop`] (here): the screens (title, play, end) as a plain state
//!   machine. It takes [`Keys`] and a picture and gives back [`Out`], so
//!   tests drive the whole game without a console.
//! * The `Game` impl at the bottom only turns the console's [`Ctx`] into
//!   [`Keys`] and [`Out`] back into calls on it.
//!
//! Uses only the game kit. No heap, no floating point, no `unsafe`.

#![no_std]

pub mod draw;
pub mod rooms;
pub mod sim;

use game_kit::{
    Ctx, Flow, Frame, Game, H, STICK_MAX, TRIGGER_MAX, Touch, W, button, rate, text_width,
};

use draw::{Line, centre};
use rooms::{ROOMS, VIEW_W};
use sim::{Event, Follow, Pad, RUN_MAX, TICK_US, World};

/// The number of rooms in a full run.
pub const ROOM_COUNT: usize = ROOMS.len();

// ---- Controls -----------------------------------------------------------

/// Buttons that jump: the bottom and right face buttons, and L.
pub const JUMP_BUTTONS: u32 = button::A | button::B | button::L;
/// Buttons that dash: the left and top face buttons, and R.
pub const DASH_BUTTONS: u32 = button::X | button::Y | button::R;
/// A stick pushed this far counts as a direction (for the dash, menus).
pub const STICK_DIR: i16 = 16_000;
/// Stick running: nothing below `STICK_DEAD`, full speed from
/// `STICK_FULL`, in between a slower walk.
pub const STICK_DEAD: i32 = 6_000;
pub const STICK_FULL: i32 = 26_000;
/// A trigger pulled past half counts as pressed (left: jump, right: dash).
pub const TRIGGER_ON: u16 = TRIGGER_MAX / 2;
/// The most ticks one call may run (after a long stall): 0.1 s.
pub const MAX_TICKS_PER_CALL: u32 = 6;

/// One call's input, as plain numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    /// [`button`] bits held, and pressed since the last call.
    pub held: u32,
    pub pressed: u32,
    /// Left stick, -32767..32767, y grows downwards.
    pub stick: (i16, i16),
    /// Left and right trigger, 0..32767.
    pub triggers: (u16, u16),
    pub touch: Option<Touch>,
    /// Game time since the last call.
    pub delta_us: u32,
}

/// What a call asks of the console.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Out {
    /// The game is over: back to the home screen.
    pub done: bool,
    /// Ask for this frame rate ([`rate`]).
    pub rate: Option<u16>,
    /// Things to write to the log, in order.
    pub notes: [Option<Note>; 4],
}

impl Out {
    fn note(&mut self, n: Note) {
        if let Some(slot) = self.notes.iter_mut().find(|s| s.is_none()) {
            *slot = Some(n);
        }
    }
}

/// A log line, as numbers (the adapter writes the words).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// A run began at this room (1-based).
    Run {
        from: u8,
    },
    /// Back at the room's start (1-based room).
    Cut {
        room: u8,
    },
    Fell {
        room: u8,
    },
    Restart {
        room: u8,
    },
    /// A room was finished: its ticks and retries.
    Cleared {
        room: u8,
        ticks: u32,
        retries: u32,
    },
    /// The run was finished.
    Delivered {
        ticks: u32,
        retries: u32,
    },
    /// Back to the title in the middle of a run.
    Quit,
}

// ---- The screens --------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Title,
    Play,
    End,
}

/// The title menu's lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Start,
    Room,
    Scroll,
    Rate,
    Leave,
}

pub const ITEMS: [Item; 5] = [
    Item::Start,
    Item::Room,
    Item::Scroll,
    Item::Rate,
    Item::Leave,
];

/// Title menu layout (picture pixels).
pub const MENU_X: i16 = 34;
pub const MENU_Y: i16 = 40;
pub const MENU_W: u16 = 92;
pub const MENU_LINE: i16 = 10;
pub const MENU_TOP: i16 = MENU_Y + 6;

/// One room's result in a run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Score {
    pub ticks: u32,
    pub retries: u32,
    pub done: bool,
}

/// A run in progress (or just finished).
pub struct Run {
    pub world: World,
    /// The room being played (0-based), and where the run began.
    pub room: usize,
    pub first: usize,
    pub ticks: u32,
    pub retries: u32,
    pub scores: [Score; ROOM_COUNT],
}

/// The whole game.
pub struct Rooftop {
    pub scene: Scene,
    /// The title menu's cursor (index into [`ITEMS`]).
    pub sel: usize,
    /// The room a run starts at (0-based).
    pub from: usize,
    pub follow: Follow,
    /// 60 calls a second instead of 30 (60 at the start).
    pub fast: bool,
    pub run: Run,
    /// Game time not yet turned into ticks.
    acc_us: u32,
    /// The triggers were past half at the last call.
    trig_was: (bool, bool),
    /// The best full run this session (ticks); nothing is saved.
    pub best: Option<u32>,
}

impl Rooftop {
    pub fn new() -> Rooftop {
        Rooftop {
            scene: Scene::Title,
            sel: 0,
            from: 0,
            follow: Follow::Step,
            fast: true,
            run: Run {
                world: World::new(&ROOMS[0], Follow::Smooth),
                room: 0,
                first: 0,
                ticks: 0,
                retries: 0,
                scores: [Score::default(); ROOM_COUNT],
            },
            acc_us: 0,
            trig_was: (false, false),
            best: None,
        }
    }

    /// The frame rate the RATE setting asks for ([`rate`]).
    pub fn rate(&self) -> u16 {
        if self.fast { rate::FPS60 } else { rate::FPS30 }
    }

    /// One call: read the keys, move the game on, paint the whole
    /// picture.
    pub fn update(&mut self, k: &Keys, s: &mut Frame) -> Out {
        let mut out = Out::default();
        let trig = (k.triggers.0 >= TRIGGER_ON, k.triggers.1 >= TRIGGER_ON);
        let trig_down = (trig.0 && !self.trig_was.0, trig.1 && !self.trig_was.1);
        self.trig_was = trig;
        match self.scene {
            Scene::Title => self.title(k, &mut out),
            Scene::Play => self.play(k, trig, trig_down, &mut out),
            Scene::End => self.end(k, &mut out),
        }
        match self.scene {
            Scene::Title => self.paint_title(s),
            Scene::Play => self.paint_play(s),
            Scene::End => self.paint_end(s),
        }
        out
    }

    // ---- Title ----------------------------------------------------------

    fn title(&mut self, k: &Keys, out: &mut Out) {
        let (up, down, left, right) = menu_dirs(k);
        if up {
            self.sel = (self.sel + ITEMS.len() - 1) % ITEMS.len();
        }
        if down {
            self.sel = (self.sel + 1) % ITEMS.len();
        }
        let mut choose = k.pressed & (button::A | button::START) != 0;
        let mut step = i32::from(right) - i32::from(left);
        if let Some(t) = k.touch.filter(|t| t.started) {
            let i = (i32::from(t.y) - i32::from(MENU_TOP) + 2).div_euclid(i32::from(MENU_LINE));
            let inside = (i32::from(MENU_X)..i32::from(MENU_X) + i32::from(MENU_W))
                .contains(&i32::from(t.x));
            if inside && (0..ITEMS.len() as i32).contains(&i) {
                self.sel = i as usize;
                match ITEMS[self.sel] {
                    Item::Start | Item::Leave => choose = true,
                    _ => step = if t.x < W as i16 / 2 { -1 } else { 1 },
                }
            }
        }
        if k.pressed & button::B != 0 {
            out.done = true;
            return;
        }
        match ITEMS[self.sel] {
            Item::Start if choose => self.begin(out),
            Item::Leave if choose => out.done = true,
            Item::Room if step != 0 || choose => {
                let step = if step == 0 { 1 } else { step };
                self.from = (self.from as i32 + step).rem_euclid(ROOM_COUNT as i32) as usize;
            }
            Item::Scroll if step != 0 || choose => {
                self.follow = match self.follow {
                    Follow::Smooth => Follow::Step,
                    Follow::Step => Follow::Smooth,
                };
            }
            Item::Rate if step != 0 || choose => {
                self.fast = !self.fast;
                out.rate = Some(self.rate());
            }
            _ => {}
        }
    }

    /// Starts a run at the chosen room.
    pub fn begin(&mut self, out: &mut Out) {
        self.run = Run {
            world: World::new(&ROOMS[self.from], self.follow),
            room: self.from,
            first: self.from,
            ticks: 0,
            retries: 0,
            scores: [Score::default(); ROOM_COUNT],
        };
        self.acc_us = 0;
        self.scene = Scene::Play;
        out.note(Note::Run {
            from: self.from as u8 + 1,
        });
    }

    // ---- Play -----------------------------------------------------------

    fn play(&mut self, k: &Keys, trig: (bool, bool), trig_down: (bool, bool), out: &mut Out) {
        if k.pressed & button::SELECT != 0 {
            self.scene = Scene::Title;
            out.note(Note::Quit);
            return;
        }
        let run = &mut self.run;
        let room_no = run.room as u8 + 1;
        if k.pressed & button::START != 0 {
            run.world = World::new(&ROOMS[run.room], self.follow);
            run.retries = run.retries.saturating_add(1);
            run.scores[run.room].retries = run.scores[run.room].retries.saturating_add(1);
            out.note(Note::Restart { room: room_no });
        }
        // Whole ticks for the time that passed (pauses do not count).
        self.acc_us = self.acc_us.saturating_add(k.delta_us.min(100_000));
        let n = (self.acc_us / TICK_US).min(MAX_TICKS_PER_CALL);
        self.acc_us %= TICK_US;
        for i in 0..n {
            let pad = pad_from(k, trig, trig_down, i == 0);
            let ev = run.world.tick(&pad);
            run.ticks = run.ticks.saturating_add(1);
            let sc = &mut run.scores[run.room];
            sc.ticks = sc.ticks.saturating_add(1);
            match ev {
                Some(Event::Cut | Event::Fell) => {
                    run.retries = run.retries.saturating_add(1);
                    sc.retries = sc.retries.saturating_add(1);
                    out.note(if ev == Some(Event::Cut) {
                        Note::Cut { room: room_no }
                    } else {
                        Note::Fell { room: room_no }
                    });
                }
                Some(Event::Cleared) => {
                    sc.done = true;
                    out.note(Note::Cleared {
                        room: room_no,
                        ticks: sc.ticks,
                        retries: sc.retries,
                    });
                    if run.room + 1 < ROOM_COUNT {
                        run.room += 1;
                        run.world = World::new(&ROOMS[run.room], self.follow);
                    } else {
                        out.note(Note::Delivered {
                            ticks: run.ticks,
                            retries: run.retries,
                        });
                        if run.first == 0 {
                            self.best = Some(self.best.map_or(run.ticks, |b| b.min(run.ticks)));
                        }
                        self.scene = Scene::End;
                        self.sel = 0;
                    }
                    // The rest of this call's time starts the next room.
                    self.acc_us = 0;
                    break;
                }
                None => {}
            }
        }
    }

    // ---- End ------------------------------------------------------------

    fn end(&mut self, k: &Keys, out: &mut Out) {
        let tap = k.touch.filter(|t| t.started);
        if k.pressed & button::B != 0 || tap.is_some_and(|t| t.x >= W as i16 / 2) {
            out.done = true;
        } else if k.pressed & (button::A | button::START) != 0 || tap.is_some() {
            self.scene = Scene::Title;
        }
    }

    // ---- Painting -------------------------------------------------------

    fn paint_play(&self, s: &mut Frame) {
        let run = &self.run;
        draw::paint_play(s, &run.world);
        draw::paint_bar(
            s,
            run.room + 1,
            ROOMS[run.room].name,
            run.retries,
            run.ticks,
            run.world.runner.dashes > 0,
        );
    }

    /// The city at night with a strip of roof, for the title and the end.
    fn paint_city(s: &mut Frame) {
        // The moon clear of the big title.
        draw::paint_back_moon(s, 0, 145, 22);
        s.rect(0, 0, W as u16, draw::BAR_H as u16, draw::SKY_TOP);
        for x in (0..W as i16).step_by(8) {
            s.rect(x, H as i16 - 8, 8, 8, draw::BRICK);
            s.rect(x, H as i16 - 8, 8, 1, draw::ROOF_CAP);
            s.rect(x, H as i16 - 7, 8, 1, draw::BRICK_DARK);
            s.rect(x + 3, H as i16 - 6, 1, 3, draw::BRICK_DARK);
            s.rect(x, H as i16 - 3, 8, 1, draw::BRICK_DARK);
        }
    }

    fn paint_title(&self, s: &mut Frame) {
        Self::paint_city(s);
        const TITLE: &str = "ROOFTOP";
        let x = centre(TITLE, 3);
        s.text_scaled(x + 1, 9, TITLE, draw::INK, 3);
        s.text_scaled(x, 8, TITLE, draw::ACCENT, 3);
        let sub = "A NIGHT DELIVERY";
        s.text(centre(sub, 1), 27, sub, draw::TEXT_DIM);

        s.rect(MENU_X, MENU_Y, MENU_W, 58, draw::BAR);
        s.outline(MENU_X, MENU_Y, MENU_W, 58, draw::METAL_DARK);
        for (i, item) in ITEMS.iter().enumerate() {
            let y = MENU_TOP + i as i16 * MENU_LINE;
            let on = i == self.sel;
            let c = if on { draw::TEXT } else { draw::TEXT_DIM };
            if on {
                s.text(MENU_X + 4, y, ">", draw::ACCENT);
            }
            let mut l = Line::new();
            match item {
                Item::Start => {
                    l.push("START");
                }
                Item::Room => {
                    l.push("ROOM < ").num(self.from as u32 + 1, 1).push(" >");
                }
                Item::Scroll => {
                    l.push(match self.follow {
                        Follow::Smooth => "SCROLL < SMOOTH >",
                        Follow::Step => "SCROLL < STEP >",
                    });
                }
                Item::Rate => {
                    l.push(if self.fast {
                        "RATE < 60 FPS >"
                    } else {
                        "RATE < 30 FPS >"
                    });
                }
                Item::Leave => {
                    l.push("LEAVE");
                }
            }
            s.text(MENU_X + 10, y, l.as_str(), c);
        }
        if let Some(b) = self.best {
            let mut l = Line::new();
            l.push("BEST ").time(b, true);
            s.text(centre(l.as_str(), 1), 101, l.as_str(), draw::LAMP_ON);
        }
        // The runner on the roof, lamp lit.
        let mut w = World::new(&ROOMS[0], Follow::Smooth);
        w.runner.x = 16;
        w.runner.y = rooms::VIEW_H - sim::BOX_H;
        w.cam = 0;
        draw::draw_actors(s, &w);
    }

    fn paint_end(&self, s: &mut Frame) {
        Self::paint_city(s);
        let run = &self.run;
        s.rect(6, 4, W as u16 - 12, 106, draw::BAR);
        s.outline(6, 4, W as u16 - 12, 106, draw::METAL_DARK);
        let head = "DELIVERED";
        s.text_scaled(centre(head, 2), 8, head, draw::ACCENT, 2);
        let mut l = Line::new();
        l.push("TIME ")
            .time(run.ticks, true)
            .push("   RETRIES ")
            .num(run.retries, 1);
        s.text(centre(l.as_str(), 1), 22, l.as_str(), draw::TEXT);
        if run.first > 0 {
            let mut l = Line::new();
            l.push("PRACTICE FROM ROOM ").num(run.first as u32 + 1, 1);
            s.text(centre(l.as_str(), 1), 29, l.as_str(), draw::TEXT_DIM);
        }
        for (i, sc) in run.scores.iter().enumerate() {
            let y = 37 + i as i16 * 8;
            let c = if sc.done { draw::TEXT } else { draw::TEXT_DIM };
            let mut l = Line::new();
            l.num(i as u32 + 1, 1).push(" ").push(ROOMS[i].name);
            s.text(12, y, l.as_str(), c);
            if sc.done {
                let mut t = Line::new();
                t.time(sc.ticks, true);
                s.text(118 - text_width(t.as_str()) as i16, y, t.as_str(), c);
                let mut r = Line::new();
                r.push("X").num(sc.retries, 1);
                s.text(
                    148 - text_width(r.as_str()) as i16,
                    y,
                    r.as_str(),
                    draw::TEXT_DIM,
                );
            }
        }
        let hint = "A: TITLE      B: LEAVE";
        s.text(centre(hint, 1), 102, hint, draw::TEXT_DIM);
    }
}

impl Default for Rooftop {
    fn default() -> Self {
        Rooftop::new()
    }
}

/// Up, down, left, right pressed on the d-pad (menus).
fn menu_dirs(k: &Keys) -> (bool, bool, bool, bool) {
    (
        k.pressed & button::UP != 0,
        k.pressed & button::DOWN != 0,
        k.pressed & button::LEFT != 0,
        k.pressed & button::RIGHT != 0,
    )
}

/// The runner's input for one tick of this call. Presses count only in
/// the call's first tick.
pub fn pad_from(k: &Keys, trig: (bool, bool), trig_down: (bool, bool), first: bool) -> Pad {
    let (sx, sy) = (i32::from(k.stick.0), i32::from(k.stick.1));
    let dir = |neg: u32, pos: u32, v: i32| -> i8 {
        if k.held & neg != 0 || v <= -i32::from(STICK_DIR) {
            -1
        } else if k.held & pos != 0 || v >= i32::from(STICK_DIR) {
            1
        } else {
            0
        }
    };
    let dx = dir(button::LEFT, button::RIGHT, sx);
    let dy = dir(button::UP, button::DOWN, sy);
    let run = if k.held & (button::LEFT | button::RIGHT) != 0 {
        i32::from(dx) * RUN_MAX
    } else {
        stick_run(sx)
    };
    let jump_now = k.pressed & JUMP_BUTTONS != 0 || trig_down.0;
    Pad {
        run,
        dx,
        dy,
        jump_held: k.held & JUMP_BUTTONS != 0 || trig.0 || (first && jump_now),
        jump: first && jump_now,
        dash: first && (k.pressed & DASH_BUTTONS != 0 || trig_down.1),
    }
}

/// Running speed asked by the stick's x: a slow walk near the middle,
/// full speed near the edge.
pub fn stick_run(x: i32) -> i32 {
    let m = x.abs().min(i32::from(STICK_MAX));
    if m <= STICK_DEAD {
        return 0;
    }
    let v = if m >= STICK_FULL {
        RUN_MAX
    } else {
        RUN_MAX * (m - STICK_DEAD) / (STICK_FULL - STICK_DEAD)
    };
    // At least a quarter speed once past the dead zone.
    x.signum() * v.max(RUN_MAX / 4)
}

// ---- The console's side ---------------------------------------------------

impl Game for Rooftop {
    fn start(ctx: &mut Ctx) -> Self {
        ctx.log("rooftop: started");
        Rooftop::new()
    }

    fn update(&mut self, ctx: &mut Ctx) -> Flow {
        if ctx.started() || ctx.must_redraw() {
            ctx.set_colours(draw::FIRST, &draw::COLOURS);
        }
        // The console starts every game at 30 a second: ask for the
        // RATE setting's own (60 at the start).
        if ctx.started() && ctx.set_rate(self.rate()).is_err() {
            ctx.log("rooftop: that rate is not offered");
        }
        let (lx, ly) = ctx.left_stick();
        let keys = Keys {
            held: ctx.player(0).held_bits(),
            pressed: ctx.player(0).pressed_bits(),
            stick: (lx, ly),
            triggers: ctx.triggers(),
            touch: ctx.touch(),
            delta_us: ctx.delta_us(),
        };
        let out = self.update(&keys, ctx.screen());
        if let Some(r) = out.rate
            && ctx.set_rate(r).is_err()
        {
            ctx.log("rooftop: that rate is not offered");
        }
        for n in out.notes.iter().flatten() {
            log_note(ctx, n);
        }
        if out.done {
            ctx.log("rooftop: the end");
            return Flow::Done;
        }
        Flow::Run
    }
}

fn log_note(ctx: &mut Ctx, n: &Note) {
    match *n {
        Note::Run { from } => room_line(ctx, "rooftop: run from room ", from),
        Note::Cut { room } => room_line(ctx, "rooftop: cut in room ", room),
        Note::Fell { room } => room_line(ctx, "rooftop: fell in room ", room),
        Note::Restart { room } => room_line(ctx, "rooftop: restart room ", room),
        Note::Cleared {
            room,
            ticks,
            retries,
        } => ctx
            .log_line()
            .text("rooftop: room ")
            .num(room)
            .text(" clear, ")
            .num(ticks)
            .text(" ticks, ")
            .num(retries)
            .text(" retries")
            .end(),
        Note::Delivered { ticks, retries } => ctx
            .log_line()
            .text("rooftop: delivered, ")
            .num(ticks)
            .text(" ticks, ")
            .num(retries)
            .text(" retries")
            .end(),
        Note::Quit => ctx.log("rooftop: back to the title"),
    }
}

/// A line that ends with a room number.
fn room_line(ctx: &mut Ctx, text: &str, room: impl game_kit::LogNum) {
    ctx.log_line().text(text).num(room).end();
}

game_kit::game!(Rooftop);

// The view is the picture's width; the bar plus the room its height.
const _: () = assert!(VIEW_W == W as i32);
const _: () = assert!(draw::BAR_H as i32 + rooms::VIEW_H == H as i32);
