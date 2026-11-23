//! One level being played: the map, the player, the things and the
//! creatures, moved in fixed ticks of 1/60 s ([`TICK_US`]).
//!
//! Everything is whole numbers (see [`crate::fixed`]); the same pad
//! input gives the same world on the PC and on the chip.

use crate::fixed::{self, ONE, Rng, SHIFT, angle_to, cos, dist2, dist2_of, sin};
use crate::level::{LEVELS, Level, MAX_CELLS, MAX_SIDE};

/// One tick: 1/60 s of game time.
pub const TICK_US: u32 = 16_666;

/// Map cells.
pub mod cell {
    pub const FLOOR: u8 = 0;
    pub const ROCK: u8 = 1;
    pub const TIMBER: u8 = 2;
    pub const VEIN: u8 = 3;
    pub const PANEL: u8 = 4;
    pub const DOOR: u8 = 5;
    pub const GATE_A: u8 = 6;
    pub const GATE_B: u8 = 7;
    pub const SW_A: u8 = 8;
    pub const SW_B: u8 = 9;
    pub const SW_A_ON: u8 = 10;
    pub const SW_B_ON: u8 = 11;
    pub const LIFT: u8 = 12;
}

// ---- Tuning (all per tick unless said) -------------------------------------

/// The player's half size: a box of 0.27 x 2 cells.
pub const PLAYER_R: i32 = 1100;
/// Walking speed at full push: 0.05 cells a tick (3 cells a second).
pub const WALK: i32 = 205;
/// Side-step speed.
pub const STRAFE: i32 = 170;
/// Turning speed at full push, in angle units (65,536 a turn): about
/// 200 degrees a second.
pub const TURN_MAX: i32 = 600;
/// Lamp oil: full can, burn, a flash, what a refill can holds.
pub const OIL_MAX: i32 = 6000;
pub const FLASH_COST: i32 = 700;
pub const OIL_CAN: i32 = 2500;
/// Hearts.
pub const LIFE_MAX: u8 = 3;
/// A door or gate this open lets a body through (255 is fully open).
pub const PASS_OPEN: u8 = 220;
/// How far the flash reaches, and its half-width (as a ratio to the
/// distance: 1 is 45 degrees each side).
pub const FLASH_RANGE: i32 = 5 * ONE;
/// Ticks: no hurt again for this long; the flash's glow; between flashes.
pub const HURT_TICKS: u16 = 90;
pub const FLASH_GLOW: u16 = 10;
pub const FLASH_COOL: u16 = 40;
/// How long creatures stay stopped by a flash.
pub const STUN_CRAWLER: u16 = 300;
pub const STUN_DIGGER: u16 = 180;
pub const MOTH_FLEE: u16 = 45;
/// The most things a level may hold.
pub const MAX_THINGS: usize = 48;
/// How far ahead the use button reaches.
pub const REACH: i32 = ONE * 5 / 4;

/// What a thing is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Crystal,
    Oil,
    Flask,
    Crank,
    Moth,
    Crawler,
    Digger,
}

impl Kind {
    pub fn creature(self) -> bool {
        matches!(self, Kind::Moth | Kind::Crawler | Kind::Digger)
    }
}

/// A creature's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    /// Doing what it does.
    Normal,
    /// Stopped by the flash for `timer` ticks: harmless.
    Stunned,
    /// A moth leaving for good.
    Fleeing,
}

/// A thing or creature in the level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Thing {
    pub kind: Kind,
    pub x: i32,
    pub y: i32,
    pub alive: bool,
    pub mood: Mood,
    pub timer: u16,
    /// Walking direction: an angle (crawlers, moths) or the track's
    /// sign and axis (diggers: 0 east, 1 south, 2 west, 3 north).
    pub dir: u16,
    /// Animation and wobble clock.
    pub phase: u16,
    /// A crawler that can see the player walks to it.
    pub chase: bool,
}

impl Thing {
    const NONE: Thing = Thing {
        kind: Kind::Crystal,
        x: 0,
        y: 0,
        alive: false,
        mood: Mood::Normal,
        timer: 0,
        dir: 0,
        phase: 0,
        chase: false,
    };

    /// Half the size of its box, for walls.
    pub fn radius(&self) -> i32 {
        match self.kind {
            Kind::Moth => 700,
            Kind::Crawler => 1000,
            Kind::Digger => 1300,
            _ => 0,
        }
    }

    /// Can it hurt the player now?
    pub fn harmful(&self) -> bool {
        self.alive && self.mood == Mood::Normal && matches!(self.kind, Kind::Crawler | Kind::Digger)
    }
}

/// The short message shown at the top of the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Msg {
    None,
    Locked,
    NeedCrank,
    GotCrank,
    GatesOpen,
    LampLow,
    MothDrinks,
    Hurt,
    Crystal,
    Oil,
    Flask,
}

impl Msg {
    pub fn text(self) -> &'static str {
        match self {
            Msg::None => "",
            Msg::Locked => "LOCKED: FIND ITS SWITCH",
            Msg::NeedCrank => "THE LIFT NEEDS ITS CRANK",
            Msg::GotCrank => "THE CRANK! FIND THE LIFT",
            Msg::GatesOpen => "A GATE OPENS",
            Msg::LampLow => "TOO LITTLE OIL TO FLASH",
            Msg::MothDrinks => "A MOTH DRINKS YOUR OIL",
            Msg::Hurt => "OUCH!",
            Msg::Crystal => "A CRYSTAL",
            Msg::Oil => "LAMP OIL",
            Msg::Flask => "A WARM FLASK: ONE HEART",
        }
    }
}

/// What a tick reports to the screens above it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// The lift was worked with the crank: the level is done.
    Clear,
    /// The last heart is gone: the level starts again.
    Fainted,
}

/// One tick's controls, already turned into amounts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pad {
    /// -256 (back) to 256 (forward).
    pub forward: i32,
    /// -256 (left) to 256 (right).
    pub turn: i32,
    /// -256 (left) to 256 (right).
    pub strafe: i32,
    /// The use button went down (a door, a switch, the lift; with nothing
    /// to use ahead it does nothing).
    pub act: bool,
    /// The flash button went down.
    pub flash: bool,
}

/// Running totals of a level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub crystals: u16,
    pub crystals_total: u16,
    pub moths_sent: u16,
    pub stuns: u16,
    pub hurts: u16,
    pub flashes: u16,
}

/// A level in play.
pub struct World {
    pub level: usize,
    pub w: i32,
    pub h: i32,
    pub map: [u8; MAX_CELLS],
    /// How open each door or gate is: 0 shut, 255 open. Once it starts
    /// to open it goes on to the end.
    pub open: [u8; MAX_CELLS],
    /// Cells the lamp has shown (the map view draws only these).
    pub seen: [u8; MAX_CELLS],
    pub things: [Thing; MAX_THINGS],
    pub n_things: usize,
    pub x: i32,
    pub y: i32,
    pub angle: u16,
    pub life: u8,
    pub oil: i32,
    pub crank: bool,
    pub tally: Tally,
    /// Ticks left: no hurt; the flash's glow; until the next flash.
    pub hurt: u16,
    pub glow: u16,
    pub cool: u16,
    pub msg: Msg,
    pub msg_t: u16,
    pub ticks: u32,
    pub rng: Rng,
}

impl World {
    /// Level `n` (0-based) from its table.
    pub fn new(n: usize) -> World {
        let mut w = World::empty();
        w.reset(n);
        w
    }

    /// An empty world (no level loaded yet): a `const`, so the game's
    /// state can be built in place.
    pub const fn empty() -> World {
        World {
            level: 0,
            w: 0,
            h: 0,
            map: [cell::ROCK; MAX_CELLS],
            open: [0; MAX_CELLS],
            seen: [0; MAX_CELLS],
            things: [Thing::NONE; MAX_THINGS],
            n_things: 0,
            x: fixed::centre(1),
            y: fixed::centre(1),
            angle: 0,
            life: LIFE_MAX,
            oil: OIL_MAX,
            crank: false,
            tally: Tally {
                crystals: 0,
                crystals_total: 0,
                moths_sent: 0,
                stuns: 0,
                hurts: 0,
                flashes: 0,
            },
            hurt: 0,
            glow: 0,
            cool: 0,
            msg: Msg::None,
            msg_t: 0,
            ticks: 0,
            rng: Rng(0),
        }
    }

    /// Starts level `n` (0-based) again from its table, in place (no
    /// 4 KB copy on the stack). `n` past the last level is the last.
    pub fn reset(&mut self, n: usize) {
        let n = n.min(LEVELS.len() - 1);
        self.level = n;
        self.map = [cell::ROCK; MAX_CELLS];
        self.open = [0; MAX_CELLS];
        self.seen = [0; MAX_CELLS];
        self.things = [Thing::NONE; MAX_THINGS];
        self.n_things = 0;
        self.angle = 0;
        self.life = LIFE_MAX;
        self.oil = OIL_MAX;
        self.crank = false;
        self.tally = Tally::default();
        self.hurt = 0;
        self.glow = 0;
        self.cool = 0;
        self.msg = Msg::None;
        self.msg_t = 0;
        self.ticks = 0;
        self.rng = Rng(0x5EED_0000 + n as u32);
        self.load(&LEVELS[n]);
        self.look_around();
    }

    fn load(&mut self, lv: &Level) {
        self.w = lv.w() as i32;
        self.h = lv.h() as i32;
        for (y, row) in lv.rows.iter().enumerate() {
            for (x, &c) in row.iter().enumerate() {
                let i = y * MAX_SIDE + x;
                let (cx, cy) = (fixed::centre(x as i32), fixed::centre(y as i32));
                self.map[i] = match c {
                    b'#' => cell::ROCK,
                    b'B' => cell::TIMBER,
                    b'C' => cell::VEIN,
                    b'M' => cell::PANEL,
                    b'D' => cell::DOOR,
                    b'G' => cell::GATE_A,
                    b'H' => cell::GATE_B,
                    b'S' => cell::SW_A,
                    b'T' => cell::SW_B,
                    b'L' => cell::LIFT,
                    _ => cell::FLOOR,
                };
                let start = match c {
                    b'>' => Some(0u16),
                    b'v' => Some(16_384),
                    b'<' => Some(32_768),
                    b'^' => Some(49_152),
                    _ => None,
                };
                if let Some(a) = start {
                    self.x = cx;
                    self.y = cy;
                    self.angle = a;
                }
                let thing = match c {
                    b'*' => Some((Kind::Crystal, 0)),
                    b'o' => Some((Kind::Oil, 0)),
                    b'+' => Some((Kind::Flask, 0)),
                    b'k' => Some((Kind::Crank, 0)),
                    b'm' => Some((Kind::Moth, 0)),
                    b'w' => Some((Kind::Crawler, 0)),
                    b'd' => Some((Kind::Digger, 0)),
                    b'q' => Some((Kind::Digger, 1)),
                    _ => None,
                };
                if let Some((kind, dir)) = thing
                    && self.n_things < MAX_THINGS
                {
                    self.things[self.n_things] = Thing {
                        kind,
                        x: cx,
                        y: cy,
                        alive: true,
                        mood: Mood::Normal,
                        timer: 0,
                        dir,
                        phase: (self.n_things as u16).wrapping_mul(37),
                        chase: false,
                    };
                    if kind == Kind::Crystal {
                        self.tally.crystals_total += 1;
                    }
                    self.n_things += 1;
                }
            }
        }
    }

    /// The level's table.
    pub fn level_info(&self) -> &'static Level {
        &LEVELS[self.level]
    }

    /// The cell at (cx, cy); outside the map reads as rock.
    pub fn at(&self, cx: i32, cy: i32) -> u8 {
        if cx < 0 || cy < 0 || cx >= self.w || cy >= self.h {
            return cell::ROCK;
        }
        self.map[cy as usize * MAX_SIDE + cx as usize]
    }

    /// How open the door or gate at (cx, cy) is (0 for anything else).
    pub fn open_at(&self, cx: i32, cy: i32) -> u8 {
        if cx < 0 || cy < 0 || cx >= self.w || cy >= self.h {
            return 0;
        }
        self.open[cy as usize * MAX_SIDE + cx as usize]
    }

    /// Whether a body cannot stand in the cell.
    pub fn solid(&self, cx: i32, cy: i32) -> bool {
        match self.at(cx, cy) {
            cell::FLOOR => false,
            cell::DOOR | cell::GATE_A | cell::GATE_B => self.open_at(cx, cy) < PASS_OPEN,
            _ => true,
        }
    }

    /// Whether a box of half size `r` around (x, y) touches no solid cell.
    pub fn box_free(&self, x: i32, y: i32, r: i32) -> bool {
        let (x0, x1) = ((x - r) >> SHIFT, (x + r) >> SHIFT);
        let (y0, y1) = ((y - r) >> SHIFT, (y + r) >> SHIFT);
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                if self.solid(cx, cy) {
                    return false;
                }
            }
        }
        true
    }

    /// Moves a box by (dx, dy), each below one cell, one axis at a time,
    /// stopping flush at walls. True when a wall stopped it.
    pub fn slide(&self, x: &mut i32, y: &mut i32, dx: i32, dy: i32, r: i32) -> bool {
        let mut hit = false;
        if dx != 0 {
            let nx = *x + dx;
            if self.box_free(nx, *y, r) {
                *x = nx;
            } else {
                hit = true;
                if dx > 0 {
                    let edge = ((nx + r) >> SHIFT) << SHIFT;
                    *x = (edge - r - 1).max(*x);
                } else {
                    let edge = (((nx - r) >> SHIFT) + 1) << SHIFT;
                    *x = (edge + r).min(*x);
                }
            }
        }
        if dy != 0 {
            let ny = *y + dy;
            if self.box_free(*x, ny, r) {
                *y = ny;
            } else {
                hit = true;
                if dy > 0 {
                    let edge = ((ny + r) >> SHIFT) << SHIFT;
                    *y = (edge - r - 1).max(*y);
                } else {
                    let edge = (((ny - r) >> SHIFT) + 1) << SHIFT;
                    *y = (edge + r).min(*y);
                }
            }
        }
        hit
    }

    /// Whether the straight line between two points crosses no solid
    /// cell (checked every quarter cell).
    pub fn clear_line(&self, ax: i32, ay: i32, bx: i32, by: i32) -> bool {
        let (dx, dy) = (bx - ax, by - ay);
        let n = (dx.abs().max(dy.abs()) >> (SHIFT - 2)) + 1;
        for k in 1..n {
            let x = ax + dx * k / n;
            let y = ay + dy * k / n;
            if self.solid(x >> SHIFT, y >> SHIFT) {
                return false;
            }
        }
        true
    }

    /// The lamp's reach now: from 2.5 cells (no oil) to 7 cells (full),
    /// in quarter cells so it changes seldom.
    pub fn light(&self) -> i32 {
        let r = ONE * 5 / 2 + (ONE * 9 / 2) * self.oil.clamp(0, OIL_MAX) / OIL_MAX;
        r & !(ONE / 4 - 1)
    }

    fn say(&mut self, m: Msg) {
        self.msg = m;
        self.msg_t = 120;
    }

    /// One tick. Returns `Some` when the level ends.
    pub fn tick(&mut self, pad: &Pad) -> Option<Event> {
        self.ticks = self.ticks.wrapping_add(1);
        self.hurt = self.hurt.saturating_sub(1);
        self.glow = self.glow.saturating_sub(1);
        self.cool = self.cool.saturating_sub(1);
        self.msg_t = self.msg_t.saturating_sub(1);
        if self.msg_t == 0 {
            self.msg = Msg::None;
        }
        // The lamp burns: one unit every second tick (200 s from full).
        if self.ticks.is_multiple_of(2) {
            self.oil = (self.oil - 1).max(0);
        }

        // Turning and walking.
        let turn = pad.turn.clamp(-256, 256) * TURN_MAX / 256;
        self.angle = self.angle.wrapping_add(turn as i16 as u16);
        let (c, s) = (cos(self.angle), sin(self.angle));
        let f = pad.forward.clamp(-256, 256) * WALK / 256;
        let st = pad.strafe.clamp(-256, 256) * STRAFE / 256;
        // Forward along (c, s); right is (-s, c).
        let dx = (f * c - st * s) >> fixed::TRIG_SHIFT;
        let dy = (f * s + st * c) >> fixed::TRIG_SHIFT;
        let (mut x, mut y) = (self.x, self.y);
        self.slide(&mut x, &mut y, dx, dy, PLAYER_R);
        self.x = x;
        self.y = y;

        // Doors and gates that are opening go on opening (about 0.5 s).
        for cy in 0..self.h {
            for cx in 0..self.w {
                let i = cy as usize * MAX_SIDE + cx as usize;
                let o = self.open[i];
                if o > 0 && o < 255 {
                    self.open[i] = o.saturating_add(9);
                }
            }
        }

        let mut ended = None;
        if pad.act && self.use_ahead() {
            ended = Some(Event::Clear);
        }
        if pad.flash {
            self.flash();
        }

        self.pick_up();
        self.move_creatures();
        if self.life == 0 {
            return Some(Event::Fainted);
        }
        if self.ticks.is_multiple_of(6) {
            self.look_around();
        }
        ended
    }

    /// The use button: the first wall cell ahead within reach. Returns
    /// true when the lift was worked (level done). With nothing to use
    /// there (open floor, a plain wall, a switch already on) it does
    /// nothing at all: no flash, no oil (the owner's choice; B, Y and the
    /// FLASH box flash).
    fn use_ahead(&mut self) -> bool {
        let (c, s) = (cos(self.angle), sin(self.angle));
        let mut target = None;
        for k in 1..=5 {
            let d = REACH * k / 5;
            let tx = (self.x + ((d * c) >> fixed::TRIG_SHIFT)) >> SHIFT;
            let ty = (self.y + ((d * s) >> fixed::TRIG_SHIFT)) >> SHIFT;
            let t = self.at(tx, ty);
            if t != cell::FLOOR && !(self.open_at(tx, ty) >= PASS_OPEN && is_door(t)) {
                target = Some((tx, ty, t));
                break;
            }
        }
        let Some((tx, ty, t)) = target else {
            return false;
        };
        let i = ty as usize * MAX_SIDE + tx as usize;
        match t {
            cell::DOOR => {
                if self.open[i] == 0 {
                    self.open[i] = 1;
                }
            }
            cell::GATE_A | cell::GATE_B => {
                if self.open[i] == 0 {
                    self.say(Msg::Locked);
                }
            }
            cell::SW_A | cell::SW_B => {
                let (on, gate) = if t == cell::SW_A {
                    (cell::SW_A_ON, cell::GATE_A)
                } else {
                    (cell::SW_B_ON, cell::GATE_B)
                };
                self.map[i] = on;
                for j in 0..MAX_CELLS {
                    if self.map[j] == gate && self.open[j] == 0 {
                        self.open[j] = 1;
                    }
                }
                self.say(Msg::GatesOpen);
            }
            cell::LIFT => {
                if self.crank {
                    return true;
                }
                self.say(Msg::NeedCrank);
            }
            _ => {}
        }
        false
    }

    /// The lamp's flash: costs oil; stops creatures in front.
    fn flash(&mut self) {
        if self.cool > 0 {
            return;
        }
        if self.oil < FLASH_COST {
            self.say(Msg::LampLow);
            return;
        }
        self.oil -= FLASH_COST;
        self.glow = FLASH_GLOW;
        self.cool = FLASH_COOL;
        self.tally.flashes = self.tally.flashes.saturating_add(1);
        let (c, s) = (cos(self.angle), sin(self.angle));
        for i in 0..self.n_things {
            let t = self.things[i];
            if !t.alive || !t.kind.creature() || t.mood == Mood::Fleeing {
                continue;
            }
            let (dx, dy) = (t.x - self.x, t.y - self.y);
            if dx.abs() > FLASH_RANGE || dy.abs() > FLASH_RANGE {
                continue;
            }
            let depth = (c * dx + s * dy) >> fixed::TRIG_SHIFT;
            let side = (c * dy - s * dx) >> fixed::TRIG_SHIFT;
            if depth <= 0 || depth > FLASH_RANGE || side.abs() > depth + ONE / 2 {
                continue;
            }
            if !self.clear_line(self.x, self.y, t.x, t.y) {
                continue;
            }
            let t = &mut self.things[i];
            match t.kind {
                Kind::Moth => {
                    t.mood = Mood::Fleeing;
                    t.timer = MOTH_FLEE;
                }
                Kind::Crawler => {
                    t.mood = Mood::Stunned;
                    t.timer = STUN_CRAWLER;
                    self.tally.stuns = self.tally.stuns.saturating_add(1);
                }
                _ => {
                    t.mood = Mood::Stunned;
                    t.timer = STUN_DIGGER;
                    self.tally.stuns = self.tally.stuns.saturating_add(1);
                }
            }
        }
    }

    fn pick_up(&mut self) {
        for i in 0..self.n_things {
            let t = self.things[i];
            if !t.alive || t.kind.creature() {
                continue;
            }
            if dist2(t.x, t.y, self.x, self.y) > dist2_of(ONE * 6 / 10) {
                continue;
            }
            let (take, msg) = match t.kind {
                Kind::Crystal => {
                    self.tally.crystals += 1;
                    (true, Msg::Crystal)
                }
                Kind::Oil => {
                    self.oil = (self.oil + OIL_CAN).min(OIL_MAX);
                    (true, Msg::Oil)
                }
                // A flask stays until a heart is missing.
                Kind::Flask if self.life < LIFE_MAX => {
                    self.life += 1;
                    (true, Msg::Flask)
                }
                Kind::Crank => {
                    self.crank = true;
                    (true, Msg::GotCrank)
                }
                _ => (false, Msg::None),
            };
            if take {
                self.things[i].alive = false;
                self.say(msg);
            }
        }
    }

    fn move_creatures(&mut self) {
        for i in 0..self.n_things {
            let mut t = self.things[i];
            if !t.alive || !t.kind.creature() {
                continue;
            }
            t.phase = t.phase.wrapping_add(1);
            match t.mood {
                Mood::Stunned => {
                    t.timer = t.timer.saturating_sub(1);
                    if t.timer == 0 {
                        t.mood = Mood::Normal;
                    }
                }
                Mood::Fleeing => {
                    let away = angle_to(t.x - self.x, t.y - self.y);
                    self.step_creature(&mut t, away, 260);
                    t.timer = t.timer.saturating_sub(1);
                    if t.timer == 0 {
                        t.alive = false;
                        self.tally.moths_sent += 1;
                    }
                }
                Mood::Normal => match t.kind {
                    Kind::Moth => self.moth(&mut t),
                    Kind::Crawler => self.crawler(&mut t, i),
                    _ => self.digger(&mut t),
                },
            }
            self.things[i] = t;
            // Touching the player.
            let near = dist2(t.x, t.y, self.x, self.y);
            if t.harmful() && near < dist2_of(ONE * 6 / 10) && self.hurt == 0 {
                self.life = self.life.saturating_sub(1);
                self.hurt = HURT_TICKS;
                self.tally.hurts = self.tally.hurts.saturating_add(1);
                self.say(Msg::Hurt);
                // Thrown back, away from it.
                let a = angle_to(self.x - t.x, self.y - t.y);
                let (kx, ky) = (
                    (1400 * cos(a)) >> fixed::TRIG_SHIFT,
                    (1400 * sin(a)) >> fixed::TRIG_SHIFT,
                );
                let (mut x, mut y) = (self.x, self.y);
                self.slide(&mut x, &mut y, kx, ky, PLAYER_R);
                self.x = x;
                self.y = y;
                // And it backs off for a moment.
                let tt = &mut self.things[i];
                if tt.kind == Kind::Crawler {
                    tt.mood = Mood::Stunned;
                    tt.timer = 50;
                }
            }
            if t.kind == Kind::Moth && t.mood == Mood::Normal && near < dist2_of(ONE * 7 / 10) {
                self.oil = (self.oil - 4).max(0);
                if self.msg != Msg::MothDrinks && self.msg_t < 60 {
                    self.say(Msg::MothDrinks);
                }
            }
        }
    }

    /// Moves a creature `speed` units along angle `a`, sliding on walls.
    fn step_creature(&self, t: &mut Thing, a: u16, speed: i32) -> bool {
        let dx = (speed * cos(a)) >> fixed::TRIG_SHIFT;
        let dy = (speed * sin(a)) >> fixed::TRIG_SHIFT;
        let (mut x, mut y) = (t.x, t.y);
        let hit = self.slide(&mut x, &mut y, dx, dy, t.radius());
        t.x = x;
        t.y = y;
        hit
    }

    fn moth(&mut self, t: &mut Thing) {
        let near = dist2(t.x, t.y, self.x, self.y);
        // Flies to the lamp when it sees it, with a flutter.
        let a = if near < dist2_of(6 * ONE) && self.clear_line(t.x, t.y, self.x, self.y) {
            let wobble = (sin(t.phase.wrapping_mul(1500)) * 9000) >> fixed::TRIG_SHIFT;
            angle_to(self.x - t.x, self.y - t.y).wrapping_add(wobble as i16 as u16)
        } else {
            if t.phase.is_multiple_of(50) {
                t.dir = t
                    .dir
                    .wrapping_add((self.rng.below(24_000) as u16).wrapping_sub(12_000));
            }
            t.dir
        };
        if near > dist2_of(ONE * 4 / 10) && self.step_creature(t, a, 115) {
            t.dir = t.dir.wrapping_add(20_000);
        }
    }

    fn crawler(&mut self, t: &mut Thing, i: usize) {
        // Looks for the player now and then (not every tick: a line
        // check is the dearest part of a creature).
        if (t.phase as usize + i * 7).is_multiple_of(20) {
            t.chase = dist2(t.x, t.y, self.x, self.y) < dist2_of(7 * ONE)
                && self.clear_line(t.x, t.y, self.x, self.y);
        }
        if t.chase {
            let a = angle_to(self.x - t.x, self.y - t.y);
            self.step_creature(t, a, 95);
        } else {
            let a = t.dir & 0xC000; // one of four ways
            if self.step_creature(t, a, 70) {
                t.dir = (self.rng.below(4) as u16) << 14;
            }
        }
    }

    fn digger(&mut self, t: &mut Thing) {
        let a = (t.dir & 3) << 14;
        if self.step_creature(t, a, 150) {
            // The end of its track: back the other way.
            t.dir = (t.dir + 2) & 3;
        }
    }

    /// Marks what the lamp shows now: 24 rays over the view's width, as
    /// far as the light reaches. Every 6 ticks, whatever the detail.
    fn look_around(&mut self) {
        let reach = self.light() + ONE;
        for k in 0..24i32 {
            let a = self.angle.wrapping_add(((k - 12) * 520) as i16 as u16);
            let (c, s) = (cos(a), sin(a));
            let mut d = 0;
            while d < reach {
                let cx = (self.x + ((d * c) >> fixed::TRIG_SHIFT)) >> SHIFT;
                let cy = (self.y + ((d * s) >> fixed::TRIG_SHIFT)) >> SHIFT;
                if cx < 0 || cy < 0 || cx >= self.w || cy >= self.h {
                    break;
                }
                self.seen[cy as usize * MAX_SIDE + cx as usize] = 1;
                if self.solid(cx, cy) {
                    break;
                }
                d += ONE / 4;
            }
        }
        let (cx, cy) = (self.x >> SHIFT, self.y >> SHIFT);
        self.seen[cy as usize * MAX_SIDE + cx as usize] = 1;
    }

    /// Live things, for the renderer and the tests.
    pub fn things(&self) -> &[Thing] {
        &self.things[..self.n_things]
    }

    /// Whether the lamp has shown this cell (the map view draws it).
    pub fn cell_seen(&self, cx: i32, cy: i32) -> bool {
        cx >= 0
            && cy >= 0
            && cx < self.w
            && cy < self.h
            && self.seen[cy as usize * MAX_SIDE + cx as usize] != 0
    }
}

/// Doors and gates (they open).
pub fn is_door(c: u8) -> bool {
    matches!(c, cell::DOOR | cell::GATE_A | cell::GATE_B)
}
