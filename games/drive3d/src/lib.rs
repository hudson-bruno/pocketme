//! DRIVE3D: the rally prototype's first part (its spec is kept outside
//! the repository). One car slides on gravel,
//! grass and a ford along one short stretch of road on a height map,
//! with a chase camera. A working label, not a game name.
//!
//! This crate holds the rules (everything a test can check without a
//! picture) and the 3D picture, which reads the rules through
//! [`look::Look`] only.
//!
//! * [`tune`]: every starting value, with its unit and reason.
//! * [`maths`]: angles, rounding, the fingerprint.
//! * [`road`], [`land`], [`things`]: the stretch from records, baked
//!   while compiling, and what the rules ask of it.
//! * [`car`], [`camera`]: the car's step and the chase camera.
//! * [`clock`]: the fixed step. [`detail`]: LOW, MID, HIGH, MAX as values.
//! * [`play`]: one whole step. [`look`]: the drawing's read interface.
//! * [`draw`]: the palette, sky, text and menu; [`scene`]: the land,
//!   road, things, shadow, car and dust in 3D, far to near;
//!   [`models`]: the fixed models; [`order`]: the land's order table;
//!   [`menu`]: the START menu.
//!
//! Controls (player 1; the simulator's keys in brackets):
//! * steer: left stick, or the d-pad left and right (arrow keys; the
//!   stick on I J K L);
//! * throttle: right trigger, or A held (Z; the trigger on 2);
//! * brake, and reverse once stopped: left trigger, or B held (X; the
//!   trigger on 1);
//! * handbrake: R held (W);
//! * back to the road: Y (S), after 1.5 s;
//! * detail LOW, MID, HIGH, MAX: X (key A);
//! * the menu (LOW, MID, HIGH, MAX, RESTART; the run stands still while it
//!   is open): START (Return); UP and DOWN move, A or START takes, B
//!   closes;
//! * the line of numbers: SELECT (Backspace).
//!
//! The rally is played with the gamepad only (owner's rule, 2026-11-22):
//! touch does nothing here.

#![no_std]

pub mod camera;
pub mod car;
pub mod clock;
pub mod detail;
pub mod draw;
pub mod land;
pub mod look;
pub mod maths;
pub mod menu;
pub mod models;
pub mod order;
pub mod play;
pub mod road;
pub mod scene;
pub mod things;
pub mod tune;

use game_kit::{Ctx, Flow, Game, button};

use crate::car::{Controls, stick_aim};
use crate::clock::Clock;
use crate::draw::Opts;
use crate::look::Look;
use crate::menu::{Choice, Menu};
use crate::play::{Input, Play};
use crate::scene::Painter;

/// The game.
pub struct Drive {
    pub play: Play,
    pub clock: Clock,
    /// Presses not yet handed to a step (kept over an update that runs
    /// no step; an update that runs two hands them to the first).
    pending: u32,
    /// Steps run in the last update.
    pub steps_last: u8,
    /// The line of numbers is shown (SELECT).
    pub line: bool,
    /// The START menu, while it is open.
    pub menu: Option<Menu>,
    /// A and B as they were held when a menu line (a level or RESTART)
    /// was taken: they count for nothing until released once, so the A
    /// that took the line neither drives the car nor starts the run
    /// clock.
    ignore: u32,
    /// The picture's drawing state.
    pub paint: Painter,
}

/// Player 1's held controls (buttons, stick, triggers) as the car's
/// controls. Gamepad only: touch is not read.
pub fn controls(held: u32, stick_x: i16, triggers: (u16, u16)) -> Controls {
    let mut aim = stick_aim(stick_x);
    if aim == 0 {
        if held & button::LEFT != 0 {
            aim -= 256;
        }
        if held & button::RIGHT != 0 {
            aim += 256;
        }
    }
    let scale = |t: u16| (i32::from(t) * 256 / 32_767).min(256);
    let mut throttle = scale(triggers.1);
    let mut brake = scale(triggers.0);
    if held & button::A != 0 {
        throttle = 256;
    }
    if held & button::B != 0 {
        brake = 256;
    }
    Controls {
        aim,
        throttle,
        brake,
        handbrake: held & button::R != 0,
    }
}

impl Game for Drive {
    fn start(ctx: &mut Ctx) -> Self {
        ctx.log("drive3d: started");
        ctx.set_colours(0, &draw::PALETTE);
        Drive {
            play: Play::new(),
            clock: Clock::new(),
            pending: 0,
            steps_last: 0,
            line: false,
            menu: None,
            ignore: 0,
            // A compile-time value of all zero bytes: it is written
            // straight into the state. Built at run time it went onto
            // the stack first, 10.7 KB twice over, and on the CYD the
            // stack ran over at the game's start (2026-11-22).
            paint: const { Painter::new() },
        }
    }

    fn update(&mut self, ctx: &mut Ctx) -> Flow {
        if ctx.must_redraw() {
            ctx.set_colours(0, &draw::PALETTE);
        }
        let p = ctx.player(0);
        let mut held = p.held_bits();
        let mut pressed = p.pressed_bits();
        if self.menu.is_none() {
            // A new press means the button was let go in between.
            self.ignore &= held & !pressed;
            held &= !self.ignore;
            pressed &= !self.ignore;
        }
        let stick = p.left_stick();
        let triggers = p.triggers();
        if pressed & button::SELECT != 0 {
            self.line = !self.line;
        }
        if let Some(m) = &mut self.menu {
            // The run stands still: no step, no time kept.
            let choice = m.press(pressed);
            if matches!(choice, Choice::Level(_) | Choice::Restart) {
                self.ignore = held & (button::A | button::B);
            }
            match choice {
                Choice::Stay => {}
                Choice::Close => self.menu = None,
                Choice::Level(d) => {
                    self.menu = None;
                    self.play.detail = d;
                    ctx.log_line().text("drive3d: detail ").text(d.name()).end();
                }
                Choice::Restart => {
                    self.menu = None;
                    self.pending |= button::START;
                }
            }
            self.steps_last = 0;
        } else if pressed & button::START != 0 {
            self.menu = Some(Menu::open(self.play.detail));
            self.pending |= pressed & !button::START;
            self.steps_last = 0;
        } else {
            self.pending |= pressed;
            let c = controls(held, stick.0, triggers);
            let n = self.clock.add(ctx.delta_us());
            for _ in 0..n {
                let input = Input {
                    controls: c,
                    reset: self.pending & button::Y != 0,
                    restart: self.pending & button::START != 0,
                    level: self.pending & button::X != 0,
                };
                self.pending = 0;
                self.play.step(&input);
                self.log_events(ctx);
            }
            self.steps_last = n;
        }
        let look = Look::new(&self.play, self.steps_last);
        draw::picture(
            ctx.screen(),
            &mut self.paint,
            &look,
            self.menu.as_ref(),
            Opts {
                line: self.line,
                plain: false,
            },
            &mut (),
        );
        Flow::Run
    }
}

impl Drive {
    fn log_events(&self, ctx: &mut Ctx) {
        let ev = self.play.events;
        if ev.restarted {
            ctx.log("drive3d: from the start");
        }
        if ev.level {
            ctx.log_line()
                .text("drive3d: detail ")
                .text(self.play.detail.name())
                .end();
        }
        if let Some((steps, loss)) = ev.car.landed
            && steps >= 4
        {
            ctx.log_line()
                .text("drive3d: landed after ")
                .num(u32::from(steps))
                .text(" steps, lost ")
                .num(loss)
                .text("/256")
                .end();
        }
        if let Some(i) = ev.hit {
            ctx.log_line()
                .text("drive3d: hit thing ")
                .num(u32::from(i))
                .end();
        }
        if ev.put_back {
            ctx.log_line()
                .text("drive3d: back on the road at point ")
                .num(u32::from(self.play.last_road))
                .end();
        }
        if let Some(steps) = ev.finished {
            ctx.log_line()
                .text("drive3d: finish after ")
                .num(steps)
                .text(" steps")
                .end();
        }
    }
}

game_kit::game!(Drive);
