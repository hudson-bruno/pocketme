//! The follow camera (PROTOTYPE-SPEC.md, section 2).
//!
//! One step, in this order (the hero has already moved):
//! 1. **Turn**: the right stick's x turns the yaw, its y the pitch (inside
//!    [`PITCH_MIN`]..[`PITCH_MAX`]); each release of L or R adds a 45
//!    degree turn, done over [`SHOULDER_STEPS`] steps (both together:
//!    behind the hero); a press and release between two steps counts as
//!    a tap. After [`AUTO_WAIT`] steps of running on a top with no camera
//!    input the yaw eases towards the running direction, when that is
//!    within [`AUTO_SPAN`] of the view. While the hero glides (SAIL) with
//!    no camera input the yaw eases towards his run whatever the angle,
//!    by a [`GLIDE_EASE`]th of the gap a step, at most [`GLIDE_TURN`].
//! 2. **Sky tilt**: standing on nothing and with no top under the hero,
//!    [`SKY_TILT`] more pitch, eased in and out over [`SKY_STEPS`] steps.
//! 3. **Aim**: over the hero, at the held ground height plus [`AIM_UP`]
//!    (in a fall it follows the hero down to [`HOLD`] under the lowest
//!    walkable top, no further).
//! 4. **Eye**: a quarter of the way towards the wanted eye, then pulled in
//!    by the wall ray (which passes through [`flag::FADE`] blocks), pushed
//!    out to [`DIST_MIN`], lifted out of any box (FADE ones too).
//! 5. **Fade**: a FADE block between the eye and the hero's chest or head
//!    (or within [`WALL_GAP`] of the eye) shrinks away in the picture over
//!    6 steps; the rest grow back (AREAS-SPEC.md, section 0). Collision
//!    does not change.
//! 6. **Props** near the line of sight shrink; the rest grow back.

use game_3d::{Camera, Mat3, ONE14};
use game_kit::button;

use crate::hero::Hero;
use crate::island::{
    Block, Prop, box_dist2, crosses, dist2_to_segment, first_hit_mask, flag, inside_any, top_under,
};
use crate::maths::{angle_diff, len3, sub, unit, yaw_of};
use crate::step::Pad;
use crate::tune::*;

/// Most props a world may have (the scale table's size).
pub const MAX_PROPS: usize = 36;
/// A prop's full size in [`Cam::scale`].
pub const FULL: u16 = 256;
/// How much a prop shrinks or grows in one step (256 / [`SHRINK_STEPS`],
/// rounded up so it takes exactly that many steps).
const SHRINK: u16 = (FULL as i32 / SHRINK_STEPS + 1) as u16;

/// The camera's state. Points in subs (1/4096 m), angles in 1/65536 of a
/// turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cam {
    /// Facing (0 looks along +z, like the hero's).
    pub yaw: u16,
    /// Pitch down chosen by the player.
    pub pitch: u16,
    /// Extra pitch down over open sky, 0 to [`SKY_TILT`].
    pub tilt: u16,
    /// The held ground height the aim is built on (subs).
    pub ground_y: i32,
    /// The aim point and the eye (subs).
    pub aim: [i32; 3],
    pub eye: [i32; 3],
    /// Shoulder turn still to do (angle) and its steps left.
    pub turn_left: i32,
    pub turn_n: u8,
    /// L and R bits seen held since both were last up.
    pub shoulders: u32,
    /// Steps of running with no camera input.
    pub idle: u16,
    /// False until the first step: the first places the eye at once.
    pub placed: bool,
    /// Each prop's size (0 to [`FULL`]), in the world's prop order.
    pub scale: [u16; MAX_PROPS],
    /// Each FADE block's size in the picture (0 to [`FULL`]), in block
    /// order (the first [`MAX_FADE`] FADE blocks; see [`Cam::fade_of`]).
    pub fade: [u16; MAX_FADE],
}

/// The lowest walkable top (subs); 0 with none.
fn lowest_walk(blocks: &[Block]) -> i32 {
    blocks
        .iter()
        .filter(|b| b.flags & flag::WALK != 0 && b.solid())
        .map(|b| b.bounds().hi[1])
        .min()
        .unwrap_or(0)
}

/// A stick axis past the dead zone, as -256..=256.
fn axis(v: i16) -> i32 {
    let v = i32::from(v);
    let a = v.abs();
    if a <= DEAD {
        return 0;
    }
    let m = ((a - DEAD) * 256 / (32_767 - DEAD)).min(256);
    if v < 0 { -m } else { m }
}

impl Cam {
    /// A camera behind a hero facing `yaw`, not yet placed.
    pub const fn new(yaw: u16, ground_y: i32) -> Cam {
        Cam {
            yaw,
            pitch: PITCH,
            tilt: 0,
            ground_y,
            aim: [0; 3],
            eye: [0; 3],
            turn_left: 0,
            turn_n: 0,
            shoulders: 0,
            idle: 0,
            placed: false,
            scale: [FULL; MAX_PROPS],
            fade: [FULL; MAX_FADE],
        }
    }

    /// The picture size (0 to [`FULL`]) of block `i`: its fade if it is a
    /// FADE block among the first [`MAX_FADE`], else [`FULL`].
    pub fn fade_of(&self, blocks: &[Block], i: usize) -> u16 {
        let Some(b) = blocks.get(i) else {
            return FULL;
        };
        if b.flags & flag::FADE == 0 {
            return FULL;
        }
        let k = blocks[..i]
            .iter()
            .filter(|o| o.flags & flag::FADE != 0)
            .count();
        self.fade.get(k).copied().unwrap_or(FULL)
    }

    /// The camera's own turn, camera to world (game-3d's rotation; its
    /// third column is the forward direction, pitched down). game-3d's
    /// `Camera::rot` is the other way round: use [`Cam::camera`].
    pub fn rot(&self) -> Mat3 {
        Mat3::rotation_fine(
            self.yaw,
            self.pitch.saturating_add(self.tilt).wrapping_neg(),
            0,
        )
    }

    /// The eye the camera wants for the current aim, yaw and pitch.
    pub fn wanted_eye(&self) -> [i32; 3] {
        let f = self.rot().col(2);
        core::array::from_fn(|k| self.aim[k] - f[k] * sub(DIST) / ONE14)
    }

    /// Puts the camera behind the hero at once (start, after a fall).
    pub fn snap(&mut self, hero: &Hero, blocks: &[Block], props: &[Prop], detail: u8) {
        self.ground_y = hero.pos[1];
        self.turn_left = 0;
        self.turn_n = 0;
        self.shoulders = 0;
        self.idle = 0;
        self.tilt = 0;
        self.placed = false;
        self.place(hero, blocks, props, detail);
    }

    /// One step. `hero` has already taken its step.
    pub fn step(&mut self, pad: &Pad, hero: &Hero, blocks: &[Block], props: &[Prop], detail: u8) {
        // 1. Turn.
        let (sx, sy) = (axis(pad.right.0), axis(pad.right.1));
        let mut input = sx != 0 || sy != 0;
        self.yaw = self
            .yaw
            .wrapping_add((sx * i32::from(CAM_TURN) / 256) as u16);
        // Stick up (y below 0) lowers the pitch: the camera looks flatter.
        let p = i32::from(self.pitch) + sy * i32::from(CAM_TURN) / 256;
        self.pitch = p.clamp(i32::from(PITCH_MIN), i32::from(PITCH_MAX)) as u16;

        let both = button::L | button::R;
        let held = pad.held & both;
        // A press and its release inside one step (an update that ran no
        // step kept both) is seen as held for that step.
        self.shoulders |= (held | pad.pressed) & both;
        if held != 0 {
            input = true;
        } else if self.shoulders != 0 {
            if self.shoulders & both == both {
                self.turn_left = angle_diff(self.yaw, hero.yaw);
            } else if self.shoulders & button::L != 0 {
                self.turn_left -= i32::from(SHOULDER_TURN);
            } else {
                self.turn_left += i32::from(SHOULDER_TURN);
            }
            self.turn_n = SHOULDER_STEPS;
            self.shoulders = 0;
            input = true;
        }
        if self.turn_n > 0 {
            let d = self.turn_left / i32::from(self.turn_n);
            self.yaw = self.yaw.wrapping_add(d as u16);
            self.turn_left -= d;
            self.turn_n -= 1;
            input = true;
        }

        if input || !hero.running || hero.ground.is_none() {
            self.idle = 0;
        } else {
            self.idle = self.idle.saturating_add(1);
        }
        if self.idle > AUTO_WAIT
            && let Some(dir) = yaw_of(hero.vel[0], hero.vel[2])
        {
            let diff = angle_diff(self.yaw, dir);
            if diff.abs() <= AUTO_SPAN {
                let d = (diff / 32).clamp(-AUTO_MAX, AUTO_MAX);
                self.yaw = self.yaw.wrapping_add(d as u16);
            }
        }

        // The glide follow (SAIL only: nothing else glides): gliding with
        // no camera input, the yaw eases round behind the hero's run, at
        // most GLIDE_TURN a step, so a turn made on the ground (SAIL's 90°
        // at P2) is followed in the air as one gentle turn.
        if !input
            && hero.gliding
            && let Some(dir) = yaw_of(hero.vel[0], hero.vel[2])
        {
            let diff = angle_diff(self.yaw, dir);
            let d = (diff / GLIDE_EASE).clamp(-GLIDE_TURN, GLIDE_TURN);
            self.yaw = self.yaw.wrapping_add(d as u16);
        }

        // 2. Sky tilt.
        let open = hero.ground.is_none()
            && top_under(blocks, hero.pos[0], hero.pos[2], hero.pos[1]).is_none();
        let want = if open { SKY_TILT } else { 0 };
        let by = SKY_TILT / SKY_STEPS;
        self.tilt = if self.tilt < want {
            (self.tilt + by).min(want)
        } else {
            self.tilt.saturating_sub(by).max(want)
        };

        self.place(hero, blocks, props, detail);
    }

    /// Steps 3 to 5: aim, eye, props.
    fn place(&mut self, hero: &Hero, blocks: &[Block], props: &[Prop], detail: u8) {
        let first = !self.placed;
        // 3. Aim: the ground height held during jumps.
        if hero.ground.is_some() {
            self.ground_y = hero.pos[1];
        } else {
            let d = hero.pos[1] - self.ground_y;
            if d.abs() > sub(HOLD) {
                // Not down a fatal fall: the view keeps the island.
                let floor = lowest_walk(blocks) - sub(HOLD);
                self.ground_y = (self.ground_y + d / 4).max(floor.min(self.ground_y));
            }
        }
        self.aim = [hero.pos[0], self.ground_y + sub(AIM_UP), hero.pos[2]];

        // 4. Eye.
        let want = self.wanted_eye();
        if self.placed {
            for (k, w) in want.iter().enumerate() {
                self.eye[k] += (w - self.eye[k]) / 4;
            }
        } else {
            self.eye = want;
            self.placed = true;
        }
        // FADE blocks never pull the eye in: pinned at DIST_MIN behind a
        // thin raised block, the eye would sit just over its top and the
        // top would fill the picture (AREAS-SPEC.md, section 0).
        if let Some((f, _)) = first_hit_mask(blocks, self.aim, self.eye, flag::FADE | flag::HIDE) {
            let len = len3(self.off());
            let d = len * i64::from(f) / 65_536;
            let keep = (d - i64::from(sub(WALL_GAP))).max(i64::from(sub(DIST_MIN)));
            if keep < len {
                self.set_dist(keep, want);
            }
        }
        if len3(self.off()) < i64::from(sub(DIST_MIN)) {
            // A few subs over, so rounding down cannot leave it short.
            self.set_dist(i64::from(sub(DIST_MIN)) + 8, want);
        }
        for _ in 0..blocks.len() {
            match inside_any(blocks, self.eye) {
                Some(i) => {
                    let b = &blocks[usize::from(i)];
                    self.eye[1] = b.surface(self.eye[0], self.eye[2]) + sub(WALL_GAP);
                }
                None => break,
            }
        }

        // 5. Fade: a FADE block that hides the hero's chest or head, or
        // sits within WALL_GAP of the eye, shrinks away; the rest grow.
        let chest = [hero.pos[0], hero.pos[1] + HERO_H / 2, hero.pos[2]];
        let head = [hero.pos[0], hero.pos[1] + HERO_H - 205, hero.pos[2]];
        let gap = i64::from(sub(WALL_GAP));
        let fading = blocks.iter().filter(|b| b.flags & flag::FADE != 0);
        for (k, b) in fading.take(MAX_FADE).enumerate() {
            let hides = b.solid()
                && (crosses(b, chest, self.eye)
                    || crosses(b, head, self.eye)
                    || box_dist2(b, self.eye) <= gap * gap);
            let f = &mut self.fade[k];
            *f = match (hides, first) {
                (true, true) => 0,
                (false, true) => FULL,
                (true, false) => f.saturating_sub(FADE_STEP),
                (false, false) => (*f + FADE_STEP).min(FULL),
            };
        }

        // 6. Props.
        let eye = self.eye.map(unit);
        let aim = self.aim.map(unit);
        let chest = chest.map(unit);
        for (i, p) in props.iter().enumerate().take(MAX_PROPS) {
            let s = &mut self.scale[i];
            let Some((c, r)) = p.ball() else {
                *s = FULL;
                continue;
            };
            if p.min_detail() > detail {
                *s = FULL;
                continue;
            }
            let (c, r) = (c.map(unit), i64::from(unit(r)));
            let line = dist2_to_segment(c, aim, eye).min(dist2_to_segment(c, chest, eye));
            let near_eye = dist2_to_segment(c, eye, eye);
            let within = |d2: i64, gap: i32| d2 <= (r + i64::from(gap)) * (r + i64::from(gap));
            if within(line, 0) {
                *s = 0;
            } else if within(line, PROP_LINE) || within(near_eye, PROP_EYE) {
                *s = s.saturating_sub(SHRINK);
            } else {
                *s = (*s + SHRINK).min(FULL);
            }
        }
    }

    /// The eye minus the aim (subs).
    fn off(&self) -> [i32; 3] {
        core::array::from_fn(|k| self.eye[k] - self.aim[k])
    }

    /// Moves the eye along its line from the aim to `dist` subs away (the
    /// wanted eye's line if the eye sits on the aim).
    fn set_dist(&mut self, dist: i64, want: [i32; 3]) {
        let mut v = self.off();
        let mut l = len3(v);
        if l == 0 {
            v = core::array::from_fn(|k| want[k] - self.aim[k]);
            l = len3(v).max(1);
        }
        for (k, c) in v.iter().enumerate() {
            self.eye[k] = self.aim[k] + (i64::from(*c) * dist / l) as i32;
        }
    }

    /// The camera as game-3d's renderer wants it: the eye in world units
    /// (1/256 m) and the world-to-camera turn.
    pub fn camera(&self) -> Camera {
        Camera {
            pos: self.eye.map(unit),
            rot: self.rot().transposed(),
        }
    }

    /// The eye's distance from the aim (subs).
    pub fn dist(&self) -> i64 {
        len3(self.off())
    }
}
