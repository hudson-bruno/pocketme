//! The world of one room: the runner, the tiles, and the physics.
//!
//! Everything here is whole numbers, so the PC and the chip compute the
//! same thing bit for bit:
//!
//! * Positions are whole pixels (`i32`) of the room, plus a remainder
//!   in 1/256 of a pixel ("Q8": 8 bits after the point).
//! * Speeds are in 1/256 pixel per tick.
//! * One tick is 1/60 of a second ([`TICK_US`]). The game runs as many
//!   ticks per call as the time that passed asks for, so the physics is
//!   the same at 30 and at 60 calls a second.
//!
//! The runner moves one pixel at a time and stops at the first solid
//! pixel, so it can never pass through a wall, whatever its speed.
//!
//! This module draws nothing. [`World::tick`] is all a test needs to
//! play a room.

use crate::rooms::{self, Room, TILE};

/// One tick: 1/60 of a second, in microseconds (rounded down).
pub const TICK_US: u32 = 16_666;

// ---- Handling: every number that shapes how the runner feels ------------
//
// Speeds are 1/256 pixel per tick (256 = one pixel a tick = 60 pixels a
// second). Times are ticks (60 a second).

/// Top running speed (1 pixel a tick).
pub const RUN_MAX: i32 = 256;
/// Speeding up and slowing down on the ground.
pub const GROUND_ACCEL: i32 = 40;
pub const GROUND_DECEL: i32 = 52;
/// The same in the air: less grip.
pub const AIR_ACCEL: i32 = 28;
pub const AIR_DECEL: i32 = 12;
/// Gravity, and the softer gravity near the top of a held jump.
pub const GRAVITY: i32 = 40;
pub const APEX_GRAVITY: i32 = 20;
/// Below this vertical speed (up or down) a held jump floats.
pub const APEX_BAND: i32 = 72;
/// Fastest fall.
pub const FALL_MAX: i32 = 560;
/// Jump speed (upwards).
pub const JUMP_V: i32 = 620;
/// Letting go of jump while rising keeps only 1/this of the rising
/// speed: short press, short hop.
pub const JUMP_CUT_DIV: i32 = 2;
/// Fastest slide down a wall while pushing into it.
pub const WALL_SLIDE_MAX: i32 = 128;
/// A wall jump pushes away from the wall this fast...
pub const WALL_JUMP_VX: i32 = 300;
/// ...and holds the stick's sideways push off for this many ticks.
pub const WALL_JUMP_LOCK: u8 = 8;
/// A wall this close (pixels) still counts for a wall jump.
pub const WALL_REACH: i32 = 2;
/// Ticks after running off a ledge in which a jump still works.
pub const COYOTE: u8 = 6;
/// Ticks a jump press is remembered before landing.
pub const JUMP_BUFFER: u8 = 6;
/// Ticks a dash press is remembered (when the dash is not ready yet).
pub const DASH_BUFFER: u8 = 4;
/// The short freeze when the dash starts (the world stops).
pub const DASH_FREEZE: u8 = 3;
/// The dash itself: this many ticks at this speed.
pub const DASH_TICKS: u8 = 10;
pub const DASH_SPEED: i32 = 720;
/// The same speed on each axis of a diagonal dash (720 / sqrt 2).
pub const DASH_DIAG: i32 = 509;
/// What is left of the dash's speed when it ends (sideways, upwards).
pub const DASH_END_X: i32 = 300;
pub const DASH_END_UP: i32 = 200;
/// How far a jump that grazes a corner is nudged past it (pixels).
pub const CORNER: i32 = 2;
/// How far a sideways dash that grazes a ledge is lifted onto it.
pub const LEDGE_POP: i32 = 3;
/// The ticks a refilled dash makes the runner flash.
pub const REFILL_FLASH: u8 = 8;

// ---- The runner's box --------------------------------------------------

/// The box that meets walls and floors: 5 x 8 pixels. The sprite is
/// 8 x 10, so the cap and the bag stick out a little (fair edges).
pub const BOX_W: i32 = 5;
pub const BOX_H: i32 = 8;
/// The smaller box that meets glass: 3 x 5 inside the first one.
pub const HURT_X: i32 = 1;
pub const HURT_Y: i32 = 2;
pub const HURT_W: i32 = 3;
pub const HURT_H: i32 = 5;

/// Dash ghosts kept for drawing.
pub const GHOSTS: usize = 4;
/// Ticks a ghost stays.
pub const GHOST_LIFE: u8 = 12;
/// Ticks the puff of a fall or a cut stays.
pub const PUFF_LIFE: u8 = 16;

/// What the runner is asked to do in one tick. Made from the pad by the
/// game, or from a script by the tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pad {
    /// Wanted running speed, -[`RUN_MAX`] to +[`RUN_MAX`] (the stick can
    /// ask for less than full speed).
    pub run: i32,
    /// The direction held, each -1, 0 or +1 (up is -1): aims the dash.
    pub dx: i8,
    pub dy: i8,
    pub jump_held: bool,
    /// Jump went down this tick.
    pub jump: bool,
    /// Dash went down this tick.
    pub dash: bool,
}

/// What a tick can end with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// The runner touched glass, and is back at the start.
    Cut,
    /// The runner fell off the bottom, and is back at the start.
    Fell,
    /// The runner reached the door.
    Cleared,
}

/// What the runner looks like now (for drawing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pose {
    Stand,
    Run,
    Rise,
    Fall,
    Slide,
    Dash,
}

/// How the view follows the runner in a wide room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Follow {
    /// Pixel by pixel, with a small dead zone: smooth, and every frame
    /// it moves changes the whole picture (the heavy case).
    Smooth,
    /// Half a screen at a time: one whole-picture change per step.
    Step,
}

/// The runner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Runner {
    /// Top-left of the 5 x 8 box, room pixels.
    pub x: i32,
    pub y: i32,
    /// What is left over below a pixel (1/256).
    rx: i32,
    ry: i32,
    pub vx: i32,
    pub vy: i32,
    /// +1 right, -1 left.
    pub facing: i8,
    pub on_ground: bool,
    /// The wall being slid down: -1 left, +1 right, 0 none.
    pub sliding: i8,
    coyote: u8,
    jump_buf: u8,
    dash_buf: u8,
    /// Dashes ready: 0 or 1.
    pub dashes: u8,
    /// Ticks of dash left, and of the freeze before it.
    pub dash_t: u8,
    pub freeze: u8,
    dash_dx: i8,
    dash_dy: i8,
    /// Rising from a jump that a release may still cut.
    jumping: bool,
    /// Sideways push forced away from a wall for a few ticks.
    lock_t: u8,
    lock_dir: i8,
    /// Counts up while running (the legs).
    pub stride: u16,
    /// Ticks of the "dash is back" flash.
    pub flash: u8,
}

impl Runner {
    /// Standing at a room's start.
    pub fn at_start(room: &Room) -> Runner {
        let (c, r) = room.start();
        Runner {
            x: c * TILE + (TILE - BOX_W) / 2,
            y: (r + 1) * TILE - BOX_H,
            rx: 0,
            ry: 0,
            vx: 0,
            vy: 0,
            facing: 1,
            on_ground: true,
            sliding: 0,
            coyote: 0,
            jump_buf: 0,
            dash_buf: 0,
            dashes: 1,
            dash_t: 0,
            freeze: 0,
            dash_dx: 1,
            dash_dy: 0,
            jumping: false,
            lock_t: 0,
            lock_dir: 0,
            stride: 0,
            flash: 0,
        }
    }

    pub fn pose(&self) -> Pose {
        if self.dash_t > 0 || self.freeze > 0 {
            Pose::Dash
        } else if self.sliding != 0 {
            Pose::Slide
        } else if !self.on_ground {
            if self.vy < 0 { Pose::Rise } else { Pose::Fall }
        } else if self.vx != 0 {
            Pose::Run
        } else {
            Pose::Stand
        }
    }

    /// The box's middle, room pixels.
    pub fn centre(&self) -> (i32, i32) {
        (self.x + BOX_W / 2, self.y + BOX_H / 2)
    }
}

/// A fading copy of the runner left by the dash.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ghost {
    pub x: i32,
    pub y: i32,
    pub facing: i8,
    /// Ticks left; 0 = gone.
    pub life: u8,
}

/// One room being played.
pub struct World {
    pub room: &'static Room,
    pub runner: Runner,
    /// Left edge of the view, room pixels (0 in a one-screen room).
    pub cam: i32,
    pub follow: Follow,
    pub ghosts: [Ghost; GHOSTS],
    /// Where the last fall or cut was, and ticks left of its puff.
    pub puff: (i32, i32, u8),
    /// Ticks played in this room (all tries).
    pub ticks: u32,
}

impl World {
    pub fn new(room: &'static Room, follow: Follow) -> World {
        let runner = Runner::at_start(room);
        let mut w = World {
            room,
            runner,
            cam: 0,
            follow,
            ghosts: [Ghost::default(); GHOSTS],
            puff: (0, 0, 0),
            ticks: 0,
        };
        w.cam = w.cam_snap();
        w
    }

    /// One tick of play.
    pub fn tick(&mut self, pad: &Pad) -> Option<Event> {
        self.ticks = self.ticks.saturating_add(1);
        for g in &mut self.ghosts {
            g.life = g.life.saturating_sub(1);
        }
        self.puff.2 = self.puff.2.saturating_sub(1);
        let ev = self.step_runner(pad);
        if let Some(Event::Cut | Event::Fell) = ev {
            let (cx, cy) = self.runner.centre();
            self.puff = (cx, cy.min(self.room.h_px() - 2), PUFF_LIFE);
            self.runner = Runner::at_start(self.room);
            self.ghosts = [Ghost::default(); GHOSTS];
            self.cam = self.cam_snap();
            return ev;
        }
        self.follow_camera();
        ev
    }

    fn step_runner(&mut self, pad: &Pad) -> Option<Event> {
        let room = self.room;
        let r = &mut self.runner;
        if pad.jump {
            r.jump_buf = JUMP_BUFFER;
        }
        if pad.dash {
            r.dash_buf = DASH_BUFFER;
        }
        r.flash = r.flash.saturating_sub(1);

        // The freeze before a dash: the world stands still. The aim is
        // read on its last tick, so a late stick still counts.
        if r.freeze > 0 {
            r.freeze -= 1;
            if r.freeze == 0 {
                let (dx, dy) = if pad.dx == 0 && pad.dy == 0 {
                    (r.facing, 0)
                } else {
                    (pad.dx, pad.dy)
                };
                r.dash_dx = dx;
                r.dash_dy = dy;
                if dx != 0 {
                    r.facing = dx;
                }
                r.dash_t = DASH_TICKS;
                r.jump_buf = 0;
            }
            return None;
        }

        r.on_ground = on_ground(room, r.x, r.y);
        if r.on_ground {
            r.coyote = COYOTE;
            if r.dash_t == 0 && r.dashes == 0 {
                r.dashes = 1;
                r.flash = REFILL_FLASH;
            }
        } else {
            r.coyote = r.coyote.saturating_sub(1);
        }

        // A dash starts: the freeze first.
        if r.dash_buf > 0 && r.dashes > 0 && r.dash_t == 0 {
            r.dashes = 0;
            r.dash_buf = 0;
            r.freeze = DASH_FREEZE;
            r.vx = 0;
            r.vy = 0;
            r.rx = 0;
            r.ry = 0;
            r.jumping = false;
            r.sliding = 0;
            r.lock_t = 0;
            return None;
        }
        r.dash_buf = r.dash_buf.saturating_sub(1);

        let mut ghost = None;
        if r.dash_t > 0 {
            // Dashing: straight, no gravity, no steering.
            let (sx, sy) = if r.dash_dx != 0 && r.dash_dy != 0 {
                (DASH_DIAG, DASH_DIAG)
            } else {
                (DASH_SPEED, DASH_SPEED)
            };
            r.vx = i32::from(r.dash_dx) * sx;
            r.vy = i32::from(r.dash_dy) * sy;
            r.dash_t -= 1;
            r.sliding = 0;
            if r.dash_t % 2 == 1 {
                ghost = Some(Ghost {
                    x: r.x,
                    y: r.y,
                    facing: r.facing,
                    life: GHOST_LIFE,
                });
            }
            // A jump from the ground ends a level dash early and keeps
            // some of its speed (a long jump).
            if r.jump_buf > 0 && r.coyote > 0 && r.dash_dy >= 0 {
                r.dash_t = 0;
                r.vx =
                    r.vx.clamp(-DASH_END_X - RUN_MAX / 2, DASH_END_X + RUN_MAX / 2);
                jump(r);
            } else if r.dash_t == 0 {
                r.vx = i32::from(r.dash_dx) * DASH_END_X;
                r.vy = if r.dash_dy < 0 {
                    -DASH_END_UP
                } else {
                    i32::from(r.dash_dy) * DASH_END_X
                };
            }
        } else {
            // Running.
            let want = if r.lock_t > 0 {
                r.lock_t -= 1;
                i32::from(r.lock_dir) * RUN_MAX
            } else {
                pad.run.clamp(-RUN_MAX, RUN_MAX)
            };
            let (accel, decel) = if r.on_ground {
                (GROUND_ACCEL, GROUND_DECEL)
            } else {
                (AIR_ACCEL, AIR_DECEL)
            };
            let speeding_up = want != 0
                && (r.vx == 0 || want.signum() == r.vx.signum())
                && r.vx.abs() < want.abs();
            r.vx = approach(r.vx, want, if speeding_up { accel } else { decel });
            if want != 0 {
                r.facing = if want > 0 { 1 } else { -1 };
            }

            // Falling, floating at the top of a held jump.
            if r.jumping && (!pad.jump_held || r.vy >= 0) {
                if r.vy < 0 && !pad.jump_held {
                    r.vy /= JUMP_CUT_DIV;
                }
                r.jumping = false;
            }
            let g = if pad.jump_held && r.vy.abs() < APEX_BAND {
                APEX_GRAVITY
            } else {
                GRAVITY
            };
            r.vy = (r.vy + g).min(FALL_MAX);

            // Sliding down a wall while pushing into it.
            let push = want.signum() as i8;
            r.sliding = 0;
            if !r.on_ground && r.vy > 0 && push != 0 && wall_at(room, r.x, r.y, push, 1) {
                r.sliding = push;
                r.vy = r.vy.min(WALL_SLIDE_MAX);
            }

            // Jumps: from the ground (or just after it), else off a wall.
            if r.jump_buf > 0 {
                if r.coyote > 0 {
                    jump(r);
                } else if let Some(side) = near_wall(room, r.x, r.y, push) {
                    jump(r);
                    r.vx = -i32::from(side) * WALL_JUMP_VX;
                    r.lock_dir = -side;
                    r.lock_t = WALL_JUMP_LOCK;
                    r.facing = -side;
                    r.sliding = 0;
                }
            }
        }
        r.jump_buf = r.jump_buf.saturating_sub(1);
        if r.on_ground && r.vx != 0 {
            r.stride = r.stride.wrapping_add(1);
        }

        let dashing = r.dash_t > 0;
        move_x(room, r, dashing);
        move_y(room, r);
        if let Some(g) = ghost {
            self.ghosts.rotate_right(1);
            self.ghosts[0] = g;
        }
        let r = &self.runner;

        // Glass, the bottom, the door.
        if touches_glass(room, r.x, r.y) {
            return Some(Event::Cut);
        }
        if r.y >= room.h_px() - 4 {
            return Some(Event::Fell);
        }
        if room.any_in(r.x, r.y, BOX_W, BOX_H, |t| t == rooms::DOOR) {
            return Some(Event::Cleared);
        }
        None
    }

    /// The view's left edge with the runner in its middle.
    fn cam_snap(&self) -> i32 {
        let (cx, _) = self.runner.centre();
        (cx - rooms::VIEW_W / 2).clamp(0, (self.room.w_px() - rooms::VIEW_W).max(0))
    }

    fn follow_camera(&mut self) {
        let most = self.room.w_px() - rooms::VIEW_W;
        if most <= 0 {
            self.cam = 0;
            return;
        }
        let (cx, _) = self.runner.centre();
        let at = cx - self.cam;
        match self.follow {
            Follow::Smooth => {
                // A dead zone of 24 pixels in the middle: small steps back
                // and forth do not move the view.
                const LO: i32 = rooms::VIEW_W / 2 - 12;
                const HI: i32 = rooms::VIEW_W / 2 + 12;
                if at < LO {
                    self.cam = cx - LO;
                } else if at > HI {
                    self.cam = cx - HI;
                }
            }
            Follow::Step => {
                // Half a screen at a time, when the runner nears an edge.
                if !(32..rooms::VIEW_W - 32).contains(&at) {
                    self.cam = cx - rooms::VIEW_W / 2;
                }
            }
        }
        self.cam = self.cam.clamp(0, most);
    }
}

fn jump(r: &mut Runner) {
    r.vy = -JUMP_V;
    r.jumping = true;
    r.coyote = 0;
    r.jump_buf = 0;
    r.on_ground = false;
}

/// `v` moved towards `to` by at most `step`.
fn approach(v: i32, to: i32, step: i32) -> i32 {
    if v < to {
        (v + step).min(to)
    } else {
        (v - step).max(to)
    }
}

/// Adds a speed to a Q8 remainder; returns the whole pixels to move and
/// keeps the rest (-128 to 127).
fn take_pixels(rem: &mut i32, add: i32) -> i32 {
    *rem += add;
    let px = (*rem + 128).div_euclid(256);
    *rem -= px * 256;
    px
}

fn move_x(room: &Room, r: &mut Runner, dashing: bool) {
    let mut m = take_pixels(&mut r.rx, r.vx);
    while m != 0 {
        let s = m.signum();
        if solid_box(room, r.x + s, r.y) {
            // A level dash that only grazes a ledge is lifted onto it.
            let pop = dashing && r.dash_dy == 0;
            let up = (1..=LEDGE_POP).find(|&up| pop && !solid_box(room, r.x + s, r.y - up));
            if let Some(up) = up {
                r.y -= up;
                r.x += s;
                m -= s;
                continue;
            }
            r.vx = 0;
            r.rx = 0;
            return;
        }
        r.x += s;
        m -= s;
    }
}

fn move_y(room: &Room, r: &mut Runner) {
    // Standing: no speed and no part pixel builds up, so how long the
    // runner stood never changes the next jump.
    if r.vy >= 0 && (solid_box(room, r.x, r.y + 1) || on_platform(room, r.x, r.y)) {
        r.vy = 0;
        r.ry = 0;
        r.on_ground = true;
        return;
    }
    let mut m = take_pixels(&mut r.ry, r.vy);
    while m != 0 {
        let s = m.signum();
        let blocked = if s > 0 {
            solid_box(room, r.x, r.y + 1) || on_platform(room, r.x, r.y)
        } else {
            solid_box(room, r.x, r.y - 1)
        };
        if blocked {
            if s < 0 {
                // A jump that grazes a corner slides past it.
                let pref = if r.vx < 0 { -1 } else { 1 };
                let nudge = (1..=CORNER).flat_map(|d| [d * pref, -d * pref]).find(|&d| {
                    !solid_box(room, r.x + d, r.y - 1) && !solid_box(room, r.x + d, r.y)
                });
                if let Some(d) = nudge {
                    r.x += d;
                    continue;
                }
            }
            r.vy = 0;
            r.ry = 0;
            if s > 0 {
                r.on_ground = true;
            }
            return;
        }
        r.y += s;
        m -= s;
    }
}

/// Does a box at `(x, y)` meet a solid tile, or the room's side or top
/// edge? (The bottom is open: that is where the street is.)
pub fn solid_box(room: &Room, x: i32, y: i32) -> bool {
    if x < 0 || y < 0 || x + BOX_W > room.w_px() {
        return true;
    }
    room.any_in(x, y, BOX_W, BOX_H, rooms::is_solid)
}

/// The box stands exactly on the top of a one-way awning.
fn on_platform(room: &Room, x: i32, y: i32) -> bool {
    let feet = y + BOX_H;
    feet % TILE == 0 && room.any_in(x, feet, BOX_W, 1, |t| t == rooms::AWNING)
}

pub fn on_ground(room: &Room, x: i32, y: i32) -> bool {
    solid_box(room, x, y + 1) || on_platform(room, x, y)
}

/// A wall `dist` pixels to the `side` (-1 left, +1 right). The room's
/// sides count.
fn wall_at(room: &Room, x: i32, y: i32, side: i8, dist: i32) -> bool {
    let nx = x + i32::from(side) * dist;
    nx < 0 || nx + BOX_W > room.w_px() || room.any_in(nx, y.max(0), BOX_W, BOX_H, rooms::is_solid)
}

/// The side of a wall within [`WALL_REACH`]: the pushed side first.
fn near_wall(room: &Room, x: i32, y: i32, push: i8) -> Option<i8> {
    let sides = if push < 0 { [-1, 1] } else { [1, -1] };
    sides
        .into_iter()
        .find(|&s| (1..=WALL_REACH).any(|d| wall_at(room, x, y, s, d)))
}

/// Does the small hurt box touch the glass of any glass tile?
pub fn touches_glass(room: &Room, x: i32, y: i32) -> bool {
    let (hx, hy) = (x + HURT_X, y + HURT_Y);
    let c0 = hx.div_euclid(TILE);
    let c1 = (hx + HURT_W - 1).div_euclid(TILE);
    let r0 = hy.div_euclid(TILE);
    let r1 = (hy + HURT_H - 1).div_euclid(TILE);
    for row in r0..=r1 {
        for col in c0..=c1 {
            if let Some((gx, gy, gw, gh)) = rooms::glass_rect(room.at(col, row)) {
                let (ax, ay) = (col * TILE + gx, row * TILE + gy);
                if hx < ax + gw && ax < hx + HURT_W && hy < ay + gh && ay < hy + HURT_H {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_keeps_the_rest() {
        let mut rem = 0;
        let mut total = 0;
        for _ in 0..256 {
            total += take_pixels(&mut rem, 100);
            assert!((-128..128).contains(&rem));
        }
        assert_eq!(total * 256 + rem, 25_600);
        let mut rem = 0;
        assert_eq!(take_pixels(&mut rem, -129), -1);
        assert_eq!(rem, 127);
    }

    #[test]
    fn approach_stops_at_the_target() {
        assert_eq!(approach(0, 100, 40), 40);
        assert_eq!(approach(90, 100, 40), 100);
        assert_eq!(approach(-90, -100, 40), -100);
        assert_eq!(approach(50, 0, 60), 0);
    }
}
