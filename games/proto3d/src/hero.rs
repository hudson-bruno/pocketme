//! The hero: run, jump, fall, land, the shadow and the landing ring
//! (PROTOTYPE-SPEC.md, section 1).
//!
//! One step, in this order:
//! 1. **Run**: the stick (or the d-pad) turned by the camera's yaw gives
//!    a wanted speed; the speed moves towards it by at most [`ACC`] on a
//!    top, [`ACC_AIR`] in the air; with no input it drops by [`BRAKE`] on
//!    a top and not at all in the air. The facing turns towards the
//!    wanted direction by at most [`TURN`].
//! 2. **Jump**: a new press of A fills the buffer; a jump starts while the
//!    buffer lasts, on a top or within [`COYOTE`] steps of leaving one.
//! 3. **Cut**: A not held while rising faster than [`CUT`]: speed [`CUT`].
//! 4. **Gravity**, then the position: across first (sides, step-up),
//!    then up or down (landing, head bump), then the step-down snap.
//! 5. Counters: steps in the air, the buffer, the squash, the safe spot,
//!    falling off.
//!
//! The test areas (AREAS-SPEC.md, section 2) tune the hero each step with
//! a [`Body`] ([`Hero::step_with`]): the running speed and the jump (a
//! weight carried), the glide (a limit on the fall while B is held), the
//! height below which the hero has fallen off and the wait before being
//! put back. [`Hero::step`] is [`Body::NORMAL`], the test island's.
//! A hero standing on a top that moves up or down goes with it
//! ([`Hero::ride`]). Switched-off blocks ([`flag::HIDE`]) are not solid.

use game_3d::ONE14;
use game_kit::button;

use crate::island::{Block, body_fits, flag, top_under};
use crate::maths::{approach, clamp, len2, shorten, turn_by_yaw, turn_towards, yaw_of};
use crate::step::Pad;
use crate::tune::*;

/// The hero's pose: each a fixed model (no bones).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pose {
    Stand,
    /// Running, frame 0 or 1.
    Run(u8),
    /// Rising (arms up).
    Jump,
    /// Falling.
    Fall,
    /// Just landed.
    Squash,
    /// Gliding (only with [`Body::glide`]): the falling model, the sail
    /// drawn by the area.
    Glide,
    /// Fallen off, waiting to be put back: not drawn.
    Gone,
}

/// The shadow: a flat disc on the top under the hero's middle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shadow {
    /// Block number of the top it lies on (its parent when drawing).
    pub top: u8,
    /// Middle of the disc (subs), on that top.
    pub spot: [i32; 3],
    /// Radius (subs).
    pub r: i32,
}

/// The landing ring: where the jump going on now comes down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ring {
    /// Block number of the top it lands on.
    pub top: u8,
    /// Middle of the ring (subs): the feet where they land.
    pub spot: [i32; 3],
    /// Steps from now to the landing.
    pub steps: u16,
}

/// The hero's tuning for one step (AREAS-SPEC.md, section 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Body {
    /// Top running speed: subs a step.
    pub run: i32,
    /// Upward speed a jump starts with: subs a step.
    pub jump: i32,
    /// Holding B while falling in the air glides.
    pub glide: bool,
    /// Below this the hero has fallen off: subs.
    pub kill_y: i32,
    /// Steps frozen after falling off, before being put back (at least 1).
    pub respawn: u8,
}

impl Body {
    /// The test island's hero: today's values.
    pub const NORMAL: Body = Body {
        run: RUN,
        jump: JUMP,
        glide: false,
        kill_y: KILL_Y,
        respawn: RESPAWN_STEPS,
    };
}

impl Default for Body {
    fn default() -> Body {
        Body::NORMAL
    }
}

/// What happened in one step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeroEvents {
    pub jumped: bool,
    /// Came down on this block (from the air).
    pub landed: Option<u8>,
    pub bumped: bool,
    pub fell: bool,
    pub respawned: bool,
}

/// The hero's state. Positions in subs (feet middle), speeds in subs a
/// step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hero {
    pub pos: [i32; 3],
    pub vel: [i32; 3],
    /// Facing (1/65536 turn; 0 looks along +z).
    pub yaw: u16,
    /// The block whose top is under the feet, while standing.
    pub ground: Option<u8>,
    /// Steps since the feet left a top (0 while standing; past
    /// [`COYOTE`] after a jump).
    pub air_steps: u8,
    /// Steps a press of A is still kept.
    pub jump_buffer: u8,
    /// Where the hero is put back after falling off, and its block.
    pub safe: [i32; 3],
    pub safe_top: u8,
    /// The block stood on without a break, and for how many steps.
    pub on_top: u8,
    pub on_steps: u16,
    /// Steps of squash pose left.
    pub squash: u8,
    /// Steps left frozen after falling off (0: playing).
    pub frozen: u8,
    /// Ground covered while standing (subs, wraps): the running frame.
    pub run_phase: u32,
    /// The stick or d-pad asked to move in the last step.
    pub running: bool,
    /// Jumps started and falls off so far.
    pub jumps: u16,
    pub falls: u16,
    /// Glide steps left in this take-off ([`GLIDE_STEPS`] on a top).
    pub glide_left: u8,
    /// Glided in the last step.
    pub gliding: bool,
    /// Steps since a creature knocked him back ([`crate::life::STUN`]
    /// down to 0; no input while above [`crate::life::STUN_FREE`]).
    pub stun: u8,
}

/// The wanted direction (world x and z, in 1/16384) and strength (1 to
/// 256) from the stick, or else the d-pad, for a camera turned by
/// `cam_yaw`. `None`: no input.
pub fn wish(pad: &Pad, cam_yaw: u16) -> Option<([i32; 2], i32)> {
    let (x, y) = (i32::from(pad.left.0), i32::from(pad.left.1));
    let l = len2(x, y);
    let (right, fwd, mag) = if l > DEAD {
        let mag = ((l - DEAD) * 256 / (32_767 - DEAD)).min(256);
        (x * ONE14 / l, -y * ONE14 / l, mag)
    } else {
        let h = pad.held;
        let dx = i32::from(h & button::RIGHT != 0) - i32::from(h & button::LEFT != 0);
        let df = i32::from(h & button::UP != 0) - i32::from(h & button::DOWN != 0);
        if dx == 0 && df == 0 {
            return None;
        }
        // A diagonal is as long as a straight push (11585 = 16384 / sqrt 2).
        let k = if dx != 0 && df != 0 { 11_585 } else { ONE14 };
        (dx * k, df * k, 256)
    };
    if mag == 0 {
        return None;
    }
    Some((turn_by_yaw(right, fwd, cam_yaw), mag))
}

impl Hero {
    /// A hero standing at `pos` (subs) on block `top`, facing +z.
    pub const fn new(pos: [i32; 3], top: u8) -> Hero {
        Hero {
            pos,
            vel: [0; 3],
            yaw: 0,
            ground: Some(top),
            air_steps: 0,
            jump_buffer: 0,
            safe: pos,
            safe_top: top,
            on_top: top,
            on_steps: 0,
            squash: 0,
            frozen: 0,
            run_phase: 0,
            running: false,
            jumps: 0,
            falls: 0,
            glide_left: GLIDE_STEPS,
            gliding: false,
            stun: 0,
        }
    }

    /// The head's height (subs).
    pub fn head(&self) -> i32 {
        self.pos[1] + HERO_H
    }

    /// One step of the rules with the test island's tuning
    /// ([`Body::NORMAL`]).
    pub fn step(&mut self, pad: &Pad, cam_yaw: u16, blocks: &[Block]) -> HeroEvents {
        self.step_with(pad, cam_yaw, blocks, &Body::NORMAL)
    }

    /// One step of the rules with this tuning.
    pub fn step_with(
        &mut self,
        pad: &Pad,
        cam_yaw: u16,
        blocks: &[Block],
        body: &Body,
    ) -> HeroEvents {
        let mut ev = HeroEvents::default();
        if self.frozen > 0 {
            self.frozen -= 1;
            if self.frozen == 0 {
                self.respawn();
                ev.respawned = true;
            }
            return ev;
        }
        let was_ground = self.ground.is_some();
        // The glide hook: B held, falling, in the air, glide left. A
        // coyote jump in this step cancels it (below).
        let mut glide = body.glide
            && !was_ground
            && pad.held & button::B != 0
            && self.vel[1] <= 0
            && self.glide_left > 0;

        // 1. Run and facing.
        let mut vh = [self.vel[0], self.vel[2]];
        let want = wish(pad, cam_yaw);
        self.running = want.is_some();
        match want {
            Some((d, mag)) => {
                let top = if glide { GLIDE_RUN } else { body.run };
                let speed = top * mag / 256;
                let target = [d[0] * speed / ONE14, d[1] * speed / ONE14];
                approach(&mut vh, target, if was_ground { ACC } else { ACC_AIR });
                if let Some(a) = yaw_of(d[0], d[1]) {
                    self.yaw = turn_towards(self.yaw, a, TURN);
                }
            }
            None if was_ground => shorten(&mut vh, BRAKE),
            None => {}
        }
        self.vel[0] = vh[0];
        self.vel[2] = vh[1];

        // 2. Jump. A press is kept in its own step and the [`BUFFER`]
        // steps after it (like [`COYOTE`]: a press up to BUFFER - 1 steps
        // before the landing step jumps right after the landing).
        let fresh = pad.pressed & button::A != 0;
        if fresh {
            self.jump_buffer = BUFFER;
        }
        let mut jumped = false;
        if self.jump_buffer > 0 && (was_ground || self.air_steps <= COYOTE) {
            self.vel[1] = body.jump;
            self.jump_buffer = 0;
            self.air_steps = COYOTE + 1;
            self.jumps = self.jumps.saturating_add(1);
            jumped = true;
            ev.jumped = true;
            glide = false;
        }

        // 3. Cut: a short hop when A is let go early.
        if pad.held & button::A == 0 && self.vel[1] > CUT {
            self.vel[1] = CUT;
        }

        // 4. Gravity (or the glide's limit on the fall), then the position.
        self.gliding = glide;
        if glide {
            self.glide_left -= 1;
            let v = self.vel[1];
            self.vel[1] = if v >= -GLIDE_FALL {
                (v - G).max(-GLIDE_FALL)
            } else {
                (v + FALL_BRAKE).min(-GLIDE_FALL)
            };
        } else {
            self.vel[1] = (self.vel[1] - G).max(-FALL_MAX);
        }
        self.ground = None;
        self.pos[0] += self.vel[0];
        self.pos[2] += self.vel[2];
        // A push out of one block can push into another one checked
        // before it: a few passes settle it.
        for _ in 0..3 {
            if !self.sides(blocks) {
                break;
            }
        }
        let before = self.pos[1];
        self.pos[1] += self.vel[1];
        if self.vel[1] <= 0 {
            self.land(blocks, before);
        } else {
            self.bump(blocks, before, &mut ev);
        }
        if self.ground.is_none() && was_ground && !jumped {
            self.snap_down(blocks);
        }

        // 5. Counters.
        match self.ground {
            Some(g) => {
                if was_ground {
                    self.squash = self.squash.saturating_sub(1);
                } else {
                    self.squash = SQUASH;
                    ev.landed = Some(g);
                }
                self.air_steps = 0;
                self.glide_left = GLIDE_STEPS;
                self.gliding = false;
                if self.on_top == g {
                    self.on_steps = self.on_steps.saturating_add(1);
                } else {
                    self.on_top = g;
                    self.on_steps = 1;
                }
                if self.on_steps >= SAFE_STEPS {
                    self.keep_safe(blocks, g);
                }
                let moved = len2(self.vel[0], self.vel[2]);
                self.run_phase = self.run_phase.wrapping_add(moved as u32);
            }
            None => {
                self.air_steps = self.air_steps.saturating_add(1);
                self.squash = 0;
                self.on_steps = 0;
            }
        }
        if !fresh {
            self.jump_buffer = self.jump_buffer.saturating_sub(1);
        }
        if self.pos[1] < body.kill_y {
            self.frozen = body.respawn.max(1);
            self.gliding = false;
            self.falls = self.falls.saturating_add(1);
            self.vel = [0; 3];
            ev.fell = true;
        }
        ev
    }

    /// Sides: a block the body moved into pushes it out on x or z,
    /// whichever overlap is smaller; a top at most [`STEP_UP`] above the
    /// feet is stepped onto instead. While rising, such a top does not
    /// push either: the feet are lifted to it (or left alone when this
    /// step's rise passes it anyway), so a jump up a ramp keeps its line.
    /// Gives whether it pushed.
    fn sides(&mut self, blocks: &[Block]) -> bool {
        let mut pushed = false;
        for (i, b) in blocks.iter().enumerate() {
            let bb = b.bounds();
            let (x, z) = (self.pos[0], self.pos[2]);
            if !b.solid() || !bb.overlaps_square(x, z, HALF) {
                continue;
            }
            let top = b.surface(x, z);
            let feet = self.pos[1];
            if feet >= top || feet + HERO_H <= bb.lo[1] {
                continue;
            }
            if top - feet <= STEP_UP {
                if self.vel[1] <= 0 {
                    self.pos[1] = top;
                    self.vel[1] = 0;
                    self.ground = Some(i as u8);
                } else if feet + self.vel[1] < top {
                    self.pos[1] = top;
                }
                continue;
            }
            let out_lo_x = x + HALF - bb.lo[0];
            let out_hi_x = bb.hi[0] - (x - HALF);
            let out_lo_z = z + HALF - bb.lo[2];
            let out_hi_z = bb.hi[2] - (z - HALF);
            let (ox, sx) = if out_lo_x < out_hi_x {
                (out_lo_x, -1)
            } else {
                (out_hi_x, 1)
            };
            let (oz, sz) = if out_lo_z < out_hi_z {
                (out_lo_z, -1)
            } else {
                (out_hi_z, 1)
            };
            if ox <= oz {
                self.pos[0] += sx * ox;
                if self.vel[0] * sx < 0 {
                    self.vel[0] = 0;
                }
            } else {
                self.pos[2] += sz * oz;
                if self.vel[2] * sz < 0 {
                    self.vel[2] = 0;
                }
            }
            pushed = true;
        }
        pushed
    }

    /// Landing: the feet crossed a top from above (at or above it before,
    /// at or below it after) with the square over it. The highest wins.
    fn land(&mut self, blocks: &[Block], before: i32) {
        let (x, z, after) = (self.pos[0], self.pos[2], self.pos[1]);
        let mut best: Option<(u8, i32)> = None;
        for (i, b) in blocks.iter().enumerate() {
            if !b.solid() || !b.bounds().overlaps_square(x, z, HALF) {
                continue;
            }
            let s = b.surface(x, z);
            if before >= s && after <= s && best.is_none_or(|(_, t)| s > t) {
                best = Some((i as u8, s));
            }
        }
        if let Some((i, s)) = best {
            self.pos[1] = s;
            self.vel[1] = 0;
            self.ground = Some(i);
        }
    }

    /// Head bump: the head crossed a block's underside going up.
    fn bump(&mut self, blocks: &[Block], before: i32, ev: &mut HeroEvents) {
        let (x, z, after) = (self.pos[0], self.pos[2], self.pos[1]);
        let mut low: Option<i32> = None;
        for b in blocks {
            let bb = b.bounds();
            if !b.solid() || !bb.overlaps_square(x, z, HALF) {
                continue;
            }
            let l = bb.lo[1];
            if before + HERO_H <= l && after + HERO_H > l && low.is_none_or(|m| l < m) {
                low = Some(l);
            }
        }
        if let Some(l) = low {
            self.pos[1] = l - HERO_H;
            self.vel[1] = 0;
            ev.bumped = true;
        }
    }

    /// Walking (not jumping) over a drop of at most [`SNAP_DOWN`] keeps
    /// the feet on the top (down a ramp).
    fn snap_down(&mut self, blocks: &[Block]) {
        let (x, z, feet) = (self.pos[0], self.pos[2], self.pos[1]);
        let mut best: Option<(u8, i32)> = None;
        for (i, b) in blocks.iter().enumerate() {
            if !b.solid() || !b.bounds().overlaps_square(x, z, HALF) {
                continue;
            }
            let s = b.surface(x, z);
            if s <= feet && feet - s <= SNAP_DOWN && best.is_none_or(|(_, t)| s > t) {
                best = Some((i as u8, s));
            }
        }
        if let Some((i, s)) = best {
            self.pos[1] = s;
            self.vel[1] = 0;
            self.ground = Some(i);
        }
    }

    /// The safe spot: the feet moved at least [`SAFE_IN`] inside the
    /// top's edges (its middle if it is narrower), kept only on a
    /// walkable top and where the body fits.
    fn keep_safe(&mut self, blocks: &[Block], g: u8) {
        let b = &blocks[usize::from(g)];
        if b.flags & flag::WALK == 0 || !b.solid() {
            return;
        }
        let bb = b.bounds();
        let inside = |p: i32, lo: i32, hi: i32| {
            if hi - lo <= 2 * SAFE_IN {
                (lo + hi) / 2
            } else {
                clamp(p, lo + SAFE_IN, hi - SAFE_IN)
            }
        };
        let x = inside(self.pos[0], bb.lo[0], bb.hi[0]);
        let z = inside(self.pos[2], bb.lo[2], bb.hi[2]);
        let spot = [x, b.surface(x, z), z];
        if body_fits(blocks, spot) {
            self.safe = spot;
            self.safe_top = g;
        }
    }

    /// Back at the safe spot, standing, speed 0.
    fn respawn(&mut self) {
        self.pos = self.safe;
        self.vel = [0; 3];
        self.ground = Some(self.safe_top);
        self.air_steps = 0;
        self.jump_buffer = 0;
        self.squash = 0;
        self.on_top = self.safe_top;
        self.on_steps = 0;
        self.running = false;
        self.glide_left = GLIDE_STEPS;
        self.gliding = false;
    }

    /// Riding: standing on block `g` whose top moved between `before`
    /// and `after` (the same block numbers), the feet go with it, up or
    /// down (movers only move up and down, so there is no sideways carry).
    /// A block switched off under the feet drops the hero into the air.
    /// The area checks that the body fits before it moves a top up.
    pub fn ride(&mut self, before: &[Block], after: &[Block]) {
        let Some(g) = self.ground else { return };
        if self.frozen > 0 {
            return;
        }
        let (Some(b0), Some(b1)) = (before.get(usize::from(g)), after.get(usize::from(g))) else {
            return;
        };
        if !b1.solid() {
            self.ground = None;
            return;
        }
        let (x, z) = (self.pos[0], self.pos[2]);
        let (s0, s1) = (b0.surface(x, z), b1.surface(x, z));
        if s0 != s1 {
            self.pos[1] = s1;
        }
    }

    /// The pose to draw.
    pub fn pose(&self) -> Pose {
        if self.frozen > 0 {
            Pose::Gone
        } else if self.ground.is_some() {
            if self.squash > 0 {
                Pose::Squash
            } else if self.vel[0] != 0 || self.vel[2] != 0 {
                Pose::Run(((self.run_phase / RUN_FRAME) & 1) as u8)
            } else {
                Pose::Stand
            }
        } else if self.vel[1] > 0 {
            Pose::Jump
        } else if self.gliding {
            Pose::Glide
        } else {
            Pose::Fall
        }
    }

    /// The shadow, while a top is under the hero's middle: radius
    /// `max(SH_MIN, SH_MAX - h * 14 / 150)`, `h` the feet's height above
    /// that top. While standing it lies on the top stood on, its middle
    /// kept on that top (a hero on a rim, middle past the edge, still has
    /// one).
    pub fn shadow(&self, blocks: &[Block]) -> Option<Shadow> {
        if self.frozen > 0 {
            return None;
        }
        let (x, z) = (self.pos[0], self.pos[2]);
        if let Some(g) = self.ground
            && blocks.get(usize::from(g)).is_some_and(Block::solid)
        {
            let b = &blocks[usize::from(g)];
            let bb = b.bounds();
            let (cx, cz) = (clamp(x, bb.lo[0], bb.hi[0]), clamp(z, bb.lo[2], bb.hi[2]));
            return Some(Shadow {
                top: g,
                spot: [cx, b.surface(cx, cz), cz],
                r: SH_MAX,
            });
        }
        let (top, s) = top_under(blocks, x, z, self.pos[1])?;
        let h = self.pos[1] - s;
        Some(Shadow {
            top,
            spot: [x, s, z],
            r: (SH_MAX - h * 14 / 150).max(SH_MIN),
        })
    }

    /// The landing ring, while in the air with no top under the middle:
    /// the rules run forward from now, at most [`RING_LOOK`] steps, with
    /// the same sticks and buttons held and no new presses. `None` when
    /// it lands on nothing in that time.
    pub fn ring(&self, pad: &Pad, cam_yaw: u16, blocks: &[Block]) -> Option<Ring> {
        self.ring_with(pad, cam_yaw, blocks, &Body::NORMAL)
    }

    /// [`Hero::ring`] with this tuning: the look ahead runs
    /// [`Hero::step_with`], so it follows a glide (B held) exactly.
    pub fn ring_with(
        &self,
        pad: &Pad,
        cam_yaw: u16,
        blocks: &[Block],
        body: &Body,
    ) -> Option<Ring> {
        if self.frozen > 0 || self.ground.is_some() || self.shadow(blocks).is_some() {
            return None;
        }
        let mut h = *self;
        let p = pad.held_only();
        for n in 1..=RING_LOOK {
            h.step_with(&p, cam_yaw, blocks, body);
            if h.frozen > 0 {
                return None;
            }
            if let Some(top) = h.ground {
                return Some(Ring {
                    top,
                    spot: h.pos,
                    steps: n,
                });
            }
        }
        None
    }
}
