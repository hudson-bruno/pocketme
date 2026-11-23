//! The creatures of ISLAND (none in the test areas): one enemy kind and
//! one friendly kind, the same at every detail level. Everything here
//! runs in the fixed step, in whole numbers; nothing here draws.
//!
//! * The **burr** (enemy) walks a fixed straight patrol along x or z on
//!   a flat top, from its record's `at` for `walk` steps one way and
//!   back, turning on the spot at each end. Running into it throws the
//!   hero back ([`KNOCK`]) and takes his controls away for a moment
//!   ([`STUN`]); there are no lives. Coming down on it from above
//!   squashes it: the hero bounces ([`BOUNCE`], higher with A held) and
//!   the burr is gone until the area starts again.
//! * The **tuftling** (friend) stays at its home, turns to face the hero
//!   while he is near ([`NEAR`]) and greets him with a small hop. It does
//!   not touch the hero at all (no collision: it can never trap him).
//!
//! Every number is a first guess (ESTIMATED), the owner's to change.
//! Distances in subs (1/4096 m), times in steps (1/30 s). Small sums
//! whose range is known (places within the 16-bit records, steps within
//! a cycle) are written wrapping: the same results, less code on the
//! chip, where every checked sum costs bytes.

use crate::hero::Hero;
use crate::island::{Block, body_fits, top_under};
use crate::maths::{cm, fnv, yaw_of};
use crate::tune::{COYOTE, HALF, HERO_H, STEP_UP};

/// The most creatures a world may have (ISLAND has seven: each more
/// moves the play's later fields further in memory, which costs code on
/// the chip, see [`State`]).
pub const MAX_CRITTERS: usize = 7;

/// Creature kinds.
pub mod kind {
    /// The enemy (a burr).
    pub const BURR: u8 = 0;
    /// The friend (a tuftling).
    pub const TUFT: u8 = 1;
}

/// The four ways a burr can walk: +z, +x, -z, -x (its facing is the way
/// times a quarter turn).
pub mod way {
    pub const PZ: u8 = 0;
    pub const PX: u8 = 1;
    pub const NZ: u8 = 2;
    pub const NX: u8 = 3;
}

/// A creature's record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Critter {
    pub kind: u8,
    /// A burr: the way it first walks ([`way`]). A tuftling: the way it
    /// faces at rest (1/256 turn).
    pub turn: u8,
    /// A burr: the steps its walk takes each way. 0 for a tuftling.
    pub walk: u8,
    /// A burr: where its patrol cycle starts (steps, within the cycle).
    /// 0 for a tuftling.
    pub phase: u8,
    /// A burr: the patrol's first end, a tuftling: its home (feet, subs).
    pub at: [i32; 3],
}

impl Critter {
    /// A burr walking from `at` (cm) `len` cm the way `w` ([`way`]) and
    /// back, `phase` steps into its cycle at the start.
    pub const fn burr(at: [i16; 3], w: u8, len: i16, phase: u8) -> Critter {
        let walk = cm(len) / BURR_SPEED;
        assert!(walk > 0 && walk < 256 && w < 4);
        assert!((phase as i32) < 2 * (walk + BURR_PAUSE));
        Critter {
            kind: kind::BURR,
            turn: w,
            walk: walk as u8,
            phase,
            at: [cm(at[0]), cm(at[1]), cm(at[2])],
        }
    }

    /// A tuftling at home `at` (cm), facing `turn` (1/256 turn) at rest.
    pub const fn tuft(at: [i16; 3], turn: u8) -> Critter {
        Critter {
            kind: kind::TUFT,
            turn,
            walk: 0,
            phase: 0,
            at: [cm(at[0]), cm(at[1]), cm(at[2])],
        }
    }
}

// ---- Rules (ESTIMATED first guesses) ----------------------------------------

/// The creatures are drawn at this size (256 is the models' own size:
/// 3/2, so a burr stands 51 cm high, about 2/5 of the hero).
pub const SIZE: i32 = 384;
/// A burr's half width and height (the box that touches the hero).
pub const BURR_R: i32 = cm(30);
pub const BURR_H: i32 = cm(54);
/// A burr's walking speed: subs a step (0.6 m/s).
pub const BURR_SPEED: i32 = 82;
/// Steps a burr stands turning round at each end of its patrol (16: the
/// turn is a shift).
pub const BURR_PAUSE: i32 = 16;
/// A fall onto a burr squashes it when the feet were at most this far
/// below its top before the step (subs, 12 cm).
pub const STOMP_TOL: i32 = cm(12);
/// The hero's upward speed after a squash: subs a step (6.6 m/s; the
/// jump's cut makes it a small hop unless A is held).
pub const BOUNCE: i32 = 900;
/// The knock-back's speed away from the burr (along the axis the hero is
/// farther out on, both on a diagonal) and upward: subs a step (2.1 and
/// 2.9 m/s).
pub const KNOCK: i32 = 280;
pub const KNOCK_UP: i32 = 400;
/// The knock-back is only sideways when the ground goes on this many
/// steps of it that way (13: 89 cm, 126 cm on a diagonal); else it is
/// straight up.
pub const KNOCK_REACH: i32 = 13;
/// Steps after a knock: no input while above [`STUN_FREE`], no new
/// knock until 0.
pub const STUN: u8 = 30;
pub const STUN_FREE: u8 = 15;
/// Steps a squashed burr is seen flat before it is gone.
pub const FLAT: u8 = 15;
/// A tuftling's half width and height (its drawn size; it touches
/// nothing).
pub const TUFT_R: i32 = cm(27);
pub const TUFT_H: i32 = cm(51);
/// A tuftling notices the hero within this along x, y and z (subs, 3 m).
pub const NEAR: i32 = cm(300);
/// A tuftling's turn a step towards the hero, and back to rest (1/256
/// turn: 7 and 1.4 degrees).
pub const TUFT_TURN: i8 = 5;
pub const TUFT_BACK: i8 = 1;
/// The greeting hop: steps, and steps between hops while the hero stays
/// near. Its height is [`hop`]'s (19 cm at the top).
pub const HOP: u8 = 12;
pub const HOP_EVERY: u8 = 75;

/// What the creatures did in a step: each is a creature's number plus
/// one, 0 for none (one byte each, see [`State`]); read them with the
/// methods.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LifeEvents {
    squashed: u8,
    hit: u8,
    hopped: u8,
}

impl LifeEvents {
    /// The burr squashed.
    pub fn squashed(&self) -> Option<u8> {
        self.squashed.checked_sub(1)
    }
    /// The burr that knocked the hero back.
    pub fn hit(&self) -> Option<u8> {
        self.hit.checked_sub(1)
    }
    /// The tuftling that began a greeting hop.
    pub fn hopped(&self) -> Option<u8> {
        self.hopped.checked_sub(1)
    }
}

/// One creature's state: three bytes. A burr keeps only `t`; its place
/// and facing follow from its record and the play's step count
/// ([`Life::clock`]). The state is kept in bytes so that the play's
/// other fields keep their places in memory: a bigger state moved them
/// past the chip's short load reach and cost about 600 bytes of code
/// (MEASURED, chip build, 2026-11-22).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// A burr: 0 alive, then steps since the squash (gone at [`FLAT`]).
    /// A tuftling: steps of the hop left.
    pub t: u8,
    /// A tuftling: its facing from its rest facing (1/256 turn; 0 at
    /// the start, so the whole state starts as zero bytes).
    pub yaw: u8,
    /// A tuftling: steps the hero has been near, round [`HOP_EVERY`] (0
    /// while he is not).
    pub clock: u8,
}

/// The creatures' state, one [`State`] for each record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Life {
    pub c: [State; MAX_CRITTERS],
}

/// A burr's whole cycle (steps).
pub fn cycle(c: &Critter) -> i32 {
    i32::from(c.walk).wrapping_add(BURR_PAUSE).wrapping_mul(2)
}

/// A burr's place, facing and whether it walks, at `clock` (within its
/// cycle).
pub fn patrol(c: &Critter, clock: u16) -> ([i32; 3], u16, bool) {
    let w = i32::from(c.walk);
    let k = i32::from(clock);
    // The way back starts at `back` and ends at `home`.
    let back = w.wrapping_add(BURR_PAUSE);
    let home = back.wrapping_add(w);
    let fwd = u16::from(c.turn) << 14;
    // Steps out from the first end; the turn while it stands is 1/16 of
    // a half turn a step.
    let (j, yaw, walking) = if k < w {
        (k, fwd, true)
    } else if k < back {
        (w, fwd.wrapping_add((k.wrapping_sub(w) as u16) << 11), false)
    } else if k < home {
        (home.wrapping_sub(k), fwd ^ 0x8000, true)
    } else {
        let t = (k.wrapping_sub(home) as u16) << 11;
        (0, (fwd ^ 0x8000).wrapping_add(t), false)
    };
    let d = j.wrapping_mul(BURR_SPEED);
    let mut p = c.at;
    match c.turn & 3 {
        way::PZ => p[2] = p[2].wrapping_add(d),
        way::PX => p[0] = p[0].wrapping_add(d),
        way::NZ => p[2] = p[2].wrapping_sub(d),
        _ => p[0] = p[0].wrapping_sub(d),
    }
    (p, yaw, walking)
}

/// A tuftling's hop height (subs) with `t` steps of it left: a parabola,
/// 19 cm at the top.
pub fn hop(t: u8) -> i32 {
    let s = i32::from(HOP.saturating_sub(t));
    22i32
        .wrapping_mul(s)
        .wrapping_mul(i32::from(HOP).wrapping_sub(s))
}

impl Life {
    /// The creatures of `recs` at the start.
    pub fn new(_recs: &[Critter]) -> Life {
        Life::default()
    }

    /// A burr's step in its patrol cycle after `steps` play steps.
    pub fn clock(c: &Critter, steps: u32) -> u16 {
        let n = (cycle(c) as u32).max(1);
        (u32::from(c.phase).wrapping_add(steps) % n) as u16
    }

    /// Creature `i`'s feet middle (subs) and facing after `steps` play
    /// steps, as drawn: a burr where its patrol has it, a tuftling at
    /// home and its hop.
    pub fn place(&self, recs: &[Critter], i: usize, steps: u32) -> ([i32; 3], u16) {
        match (recs.get(i), self.c.get(i)) {
            (Some(c), Some(_)) if c.kind == kind::BURR => {
                let (p, yaw, _) = patrol(c, Life::clock(c, steps));
                (p, yaw)
            }
            (Some(c), Some(s)) => {
                let mut p = c.at;
                p[1] = p[1].wrapping_add(hop(s.t));
                (p, u16::from(c.turn.wrapping_add(s.yaw)) << 8)
            }
            _ => ([0; 3], 0),
        }
    }

    /// Creature `i`'s feet middle (subs) after `steps` play steps.
    pub fn pos(&self, recs: &[Critter], i: usize, steps: u32) -> [i32; 3] {
        self.place(recs, i, steps).0
    }

    /// Whether creature `i` is there to draw and touch.
    pub fn shown(&self, recs: &[Critter], i: usize) -> bool {
        match (recs.get(i), self.c.get(i)) {
            (Some(c), Some(s)) => c.kind != kind::BURR || s.t < FLAT,
            _ => false,
        }
    }

    /// One step, after the hero moved (his feet were at `before` high);
    /// `steps` is the play's step count after this step. Touches only
    /// while the hero plays (not frozen after a fall).
    #[inline(never)]
    pub fn step(
        &mut self,
        recs: &[Critter],
        hero: &mut Hero,
        before: i32,
        blocks: &[Block],
        steps: u32,
    ) -> LifeEvents {
        let mut ev = LifeEvents::default();
        hero.stun = hero.stun.saturating_sub(1);
        let play = hero.frozen == 0;
        for (i, (c, s)) in recs.iter().zip(self.c.iter_mut()).enumerate() {
            // The event's number (plus one: 0 is none).
            let i = (i as u8).wrapping_add(1);
            if c.kind == kind::BURR {
                if s.t > 0 {
                    s.t = s.t.wrapping_add(1).min(FLAT);
                    continue;
                }
                let (p, _, _) = patrol(c, Life::clock(c, steps));
                // Positions are within the world: plain differences
                // cannot overflow.
                let h = hero.pos;
                let dx = h[0].wrapping_sub(p[0]);
                let dy = h[1].wrapping_sub(p[1]);
                let dz = h[2].wrapping_sub(p[2]);
                let reach = (BURR_R + HALF) as u32;
                if !play
                    || dx.unsigned_abs() >= reach
                    || dz.unsigned_abs() >= reach
                    || dy >= BURR_H
                    || dy <= -HERO_H
                {
                    continue;
                }
                if hero.vel[1] < 0 && before.wrapping_sub(p[1]) >= BURR_H - STOMP_TOL {
                    // From above: squashed.
                    s.t = 1;
                    ev.squashed = i;
                    let top = [h[0], p[1].wrapping_add(BURR_H), h[2]];
                    if body_fits(blocks, top) {
                        hero.pos[1] = top[1];
                    }
                    hero.vel[1] = BOUNCE;
                    hero.ground = None;
                    hero.air_steps = COYOTE + 1;
                } else if hero.stun == 0 {
                    ev.hit = i;
                    knock(hero, dx, dz, blocks);
                }
            } else {
                let d = [
                    hero.pos[0].wrapping_sub(c.at[0]),
                    hero.pos[1].wrapping_sub(c.at[1]),
                    hero.pos[2].wrapping_sub(c.at[2]),
                ];
                s.t = s.t.saturating_sub(1);
                let (to, most) = if play && d.iter().all(|v| v.unsigned_abs() < NEAR as u32) {
                    if s.clock == 0 && s.t == 0 {
                        s.t = HOP;
                        ev.hopped = i;
                    }
                    s.clock = s.clock.wrapping_add(1);
                    if s.clock >= HOP_EVERY {
                        s.clock = 0;
                    }
                    let to = yaw_of(d[0], d[2]).map_or(c.turn, |y| (y >> 8) as u8);
                    (to.wrapping_sub(c.turn), TUFT_TURN)
                } else {
                    s.clock = 0;
                    (0, TUFT_BACK)
                };
                let turn = (to.wrapping_sub(s.yaw) as i8).clamp(-most, most);
                s.yaw = s.yaw.wrapping_add(turn as u8);
            }
        }
        ev
    }

    /// The state into a fingerprint.
    pub fn hash(&self, mut f: u64) -> u64 {
        for s in &self.c {
            f = fnv(f, i64::from(s.t));
            f = fnv(f, i64::from(s.yaw));
            f = fnv(f, i64::from(s.clock));
        }
        f
    }
}

/// Throws the hero away from a burr he is (`dx`, `dz`) from and up, and
/// stuns him: [`KNOCK`] a step along each axis he is at least half as far
/// out on as on the other (eight ways). Only up when the ground does not
/// go on [`KNOCK_REACH`] steps that way (the knock alone never throws him
/// off a top into the void).
#[inline(never)]
fn knock(hero: &mut Hero, dx: i32, dz: i32, blocks: &[Block]) {
    let (ax, az) = (dx.unsigned_abs(), dz.unsigned_abs());
    let away = |d: i32, a: u32, b: u32| {
        if a << 1 < b {
            0
        } else if d < 0 {
            -KNOCK
        } else {
            KNOCK
        }
    };
    let mut v = [away(dx, ax, az), KNOCK_UP, away(dz, az, ax)];
    let reach = |k: usize| hero.pos[k].wrapping_add(v[k].wrapping_mul(KNOCK_REACH));
    let (ex, ez) = (reach(0), reach(2));
    if top_under(blocks, ex, ez, hero.pos[1].wrapping_add(STEP_UP)).is_none() {
        v[0] = 0;
        v[2] = 0;
    }
    hero.vel = v;
    hero.ground = None;
    hero.air_steps = COYOTE + 1;
    hero.jump_buffer = 0;
    hero.stun = STUN;
}
