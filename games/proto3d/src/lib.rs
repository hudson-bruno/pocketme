//! PROTO3D: the showcase prototype (PROTOTYPE-SPEC.md, AREAS-SPEC.md). A
//! small hero with a crimson top and a white cap runs and jumps on
//! floating islands, followed by a camera. On the test island (ISLAND)
//! it collects gold nuts; three test areas (MACHINES, SAIL, STONE) each
//! try one idea. The picture has four detail levels, LOW, MID, HIGH and
//! MAX (HIGH at the start); MAX adds a rounder hero, finer ground near
//! him, rounder trees, bevelled edges and more grass (see [`draw`]).
//!
//! This crate holds the rules (everything a test can check without
//! looking at a picture) and the 3D picture.
//!
//! * [`tune`]: every starting value, with its unit.
//! * [`maths`]: units and angle helpers.
//! * [`island`]: the records (blocks, props, collectibles) and collision.
//! * [`step`]: the fixed step clock and one step's input.
//! * [`hero`], [`camera`], [`collect`]: the rules of each part.
//! * [`life`]: ISLAND's creatures (an enemy and a friend kind).
//! * [`area`]: the test areas, each with its own records and rules.
//! * [`play`]: one whole step, and the read interface for drawing.
//! * [`pause`]: the area menu (at the start) and the pause menu on START.
//! * [`draw`]: the picture (sky, island, HUD, line of numbers, menus).
//! * [`models`]: the fixed models (hero, nut, props), made while compiling.
//!
//! Controls (player 1): left stick or d-pad to run, A to jump (hold for
//! a full jump), right stick to turn the camera, L and R to turn it by
//! 45 degrees on release (both: behind the hero), START for the pause
//! menu (LOW, MID, HIGH, MAX, RESTART, AREAS; B closes it), SELECT for
//! the line of numbers, A on the DONE panel (after half a second) to
//! start the area again. The area buttons: in MACHINES X picks the bob
//! up and sets it down; in SAIL B held while falling glides; in STONE B
//! places the slab and, held while standing on it, raises it, and X
//! calls it back. The AREAS menu also takes a tap on a line (a tap
//! outside its box goes back when B would); the pause menu takes no tap.

#![no_std]

pub mod area;
pub mod camera;
pub mod collect;
pub mod draw;
pub mod hero;
pub mod island;
pub mod life;
pub mod maths;
pub mod models;
pub mod pause;
pub mod play;
pub mod step;
pub mod tune;

use game_kit::{Ctx, Flow, Game, button};

use crate::area::Area;
use crate::draw::{Painter, Shown};
use crate::pause::{AreaMenu, AreaOut, Menu, MenuOut, Tap};
use crate::play::{Detail, Play};
use crate::step::{Clock, Pad};

/// One call's input (player 1 and the touch), as the game reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    pub held: u32,
    pub pressed: u32,
    pub released: u32,
    pub left: (i16, i16),
    pub right: (i16, i16),
    pub touch: Option<Tap>,
    pub delta_us: u32,
}

/// How long the DONE panel ignores A (microseconds): half a second, 15
/// calls at 30 a second.
pub const DONE_WAIT_US: u32 = 500_000;

/// What a call did that goes into the log (besides the steps' own
/// events).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    None,
    /// An area was chosen from the menu (not the start menu's first
    /// choice).
    Area(Area),
    /// The detail setting was chosen.
    Detail(Detail),
    /// The area was started again (RESTART, or A on the done panel).
    Restart(Area),
    /// The area's goal was reached, with its time in steps if it keeps
    /// one.
    Done(Area, Option<u16>),
}

/// The game.
pub struct Proto {
    pub play: Play,
    pub clock: Clock,
    pub menu: Menu,
    /// The area menu: open at the start.
    pub areas: AreaMenu,
    /// The best time of each area in steps ([`Area::ALL`] order; 0 =
    /// none). Kept while the game runs, over RESTART and area changes.
    pub best: [u16; 4],
    /// The last finish set the best time.
    pub new_best: bool,
    /// The finish of this play was seen (its best time noted).
    done_seen: bool,
    /// Time the DONE panel has been up (microseconds): A restarts only
    /// after [`DONE_WAIT_US`], so a late jump press cannot skip it.
    done_us: u32,
    /// Presses and releases not yet handed to a step (kept over a call
    /// that runs no step).
    pending_pressed: u32,
    pending_released: u32,
    /// Steps run in the last call (the line of numbers' `S`).
    pub steps_last: u8,
    /// The line of numbers is shown (SELECT).
    pub line: bool,
    /// The drawing's working memory (the renderer).
    pub paint: Painter,
}

impl Default for Proto {
    fn default() -> Proto {
        Proto::new()
    }
}

impl Proto {
    /// The game at its start: ISLAND at HIGH behind the open area menu.
    pub fn new() -> Proto {
        Proto {
            play: Play::new(),
            clock: Clock::new(),
            menu: Menu::new(),
            areas: AreaMenu::new(),
            best: [0; 4],
            new_best: false,
            done_seen: false,
            done_us: 0,
            pending_pressed: 0,
            pending_released: 0,
            steps_last: 0,
            line: false,
            paint: Painter::new(),
        }
    }

    /// One call's rules: the menus in order (area menu, pause menu, the
    /// done panel), else the steps of play. `each` sees the state after
    /// every step (the game logs its events there).
    pub fn control(&mut self, i: &Input, mut each: impl FnMut(&Play)) -> Note {
        self.steps_last = 0;
        let area = self.play.area;
        if self.areas.open {
            let first = !self.areas.back_ok;
            match self.areas.update(i.pressed, i.left.1, i.touch) {
                AreaOut::Chosen(a) => {
                    self.begin(a);
                    if !first {
                        return Note::Area(a);
                    }
                }
                AreaOut::Back => self.close_menu(),
                AreaOut::Stay => {}
            }
        } else if self.menu.open {
            match self.menu.update(i.pressed, i.left.1) {
                MenuOut::Chosen(d) => {
                    self.play.set_detail(d);
                    self.close_menu();
                    return Note::Detail(d);
                }
                MenuOut::Restart => {
                    self.begin(area);
                    return Note::Restart(area);
                }
                MenuOut::Areas => self.areas.open(area, true, i.left.1),
                MenuOut::Closed => self.close_menu(),
                MenuOut::Stay => {}
            }
        } else if i.pressed & button::START != 0 {
            self.menu.open(self.play.detail, i.left.1);
        } else if self.play.done {
            self.done_us = self.done_us.saturating_add(i.delta_us);
            if i.pressed & button::A != 0 && self.done_us >= DONE_WAIT_US {
                self.begin(area);
                return Note::Restart(area);
            }
        } else {
            if i.pressed & button::SELECT != 0 {
                self.line = !self.line;
            }
            self.pending_pressed |= i.pressed;
            self.pending_released |= i.released;
            let n = self.clock.add(i.delta_us);
            for _ in 0..n {
                let pad = Pad {
                    held: i.held,
                    pressed: self.pending_pressed,
                    released: self.pending_released,
                    left: i.left,
                    right: i.right,
                };
                self.pending_pressed = 0;
                self.pending_released = 0;
                self.play.step(&pad);
                each(&self.play);
            }
            self.steps_last = n;
        }
        if self.play.done && !self.done_seen {
            self.done_seen = true;
            let t = self.play.kit.time();
            self.new_best = false;
            if let Some(t) = t {
                let b = &mut self.best[area.index()];
                if *b == 0 || t < *b {
                    *b = t;
                    self.new_best = true;
                }
            }
            return Note::Done(area, t);
        }
        Note::None
    }

    /// Starts an area from its start, with the same detail.
    fn begin(&mut self, a: Area) {
        self.play = Play::start(a, self.play.detail);
        self.done_seen = false;
        self.done_us = 0;
        self.new_best = false;
        self.close_menu();
    }

    /// After a menu: no stored time and no old presses.
    fn close_menu(&mut self) {
        self.clock.reset();
        self.pending_pressed = 0;
        self.pending_released = 0;
    }
}

impl Game for Proto {
    fn start(ctx: &mut Ctx) -> Self {
        ctx.log("proto3d: started");
        ctx.set_colours(0, &draw::PALETTE);
        Proto::new()
    }

    fn update(&mut self, ctx: &mut Ctx) -> Flow {
        if ctx.must_redraw() {
            ctx.set_colours(0, &draw::PALETTE);
        }
        let p = ctx.player(0);
        let i = Input {
            held: p.held_bits(),
            pressed: p.pressed_bits(),
            released: p.released_bits(),
            left: p.left_stick(),
            right: p.right_stick(),
            touch: ctx.touch().map(|t| Tap {
                x: t.x,
                y: t.y,
                ended: t.ended,
            }),
            delta_us: ctx.delta_us(),
        };
        let note = self.control(&i, |play| log_events(ctx, play));
        log_note(ctx, note);

        let shown = Shown {
            menu: &self.menu,
            areas: &self.areas,
            line: self.line,
            steps: self.steps_last,
            best: self.best[self.play.area.index()],
            new_best: self.new_best,
        };
        self.paint.show(ctx.screen(), &self.play, &shown);
        Flow::Run
    }
}

fn log_note(ctx: &mut Ctx, note: Note) {
    match note {
        Note::None => {}
        Note::Area(a) => ctx.log_line().text("proto3d: area ").text(a.name()).end(),
        Note::Detail(d) => ctx.log_line().text("proto3d: detail ").text(d.name()).end(),
        Note::Restart(a) => ctx
            .log_line()
            .text("proto3d: restart ")
            .text(a.name())
            .end(),
        Note::Done(a, t) => {
            let mut l = ctx.log_line().text("proto3d: done ").text(a.name());
            if let Some(t) = t {
                l = l.text(" ").num(t);
            }
            l.end();
        }
    }
}

fn log_events(ctx: &mut Ctx, play: &Play) {
    let ev = &play.events;
    if let Some(i) = ev.took {
        let (n, total) = play.count();
        ctx.log_line()
            .text("proto3d: took ")
            .num(i)
            .text(" ")
            .num(n)
            .text("/")
            .num(total)
            .end();
    }
    // The creatures' squash and knock lines were taken out for room
    // (repair round after V1: 160 bytes); the events stay in `Play`.
    if ev.hero.fell {
        ctx.log_line()
            .text("proto3d: fell ")
            .num(play.hero.falls)
            .end();
    }
}

game_kit::game!(Proto);
