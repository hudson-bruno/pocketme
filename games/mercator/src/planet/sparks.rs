//! Step G4: the particle pool (SPEC 3.10, "Particles"): 64 sparks of 8
//! bytes (a place in 1/128 m from the site's middle, a life in ticks, a
//! kind and a variant). A full pool reuses the slot after the newest, so
//! it never runs over. A spark keeps no speed: its step a tick comes
//! from its kind, its variant (its direction), its slot and its age.
//!
//! What goes in: footsteps (dust, ice glitter, splashes), the cutter's
//! chips and dust, the hop jets' cone, their scorch ring on the ground,
//! the touchdown's dust ring, a drone's burst (sparks and three parts
//! that fall and bounce), a bolt's end, a rolling boulder's dust trail.
//!
//! Not in the pool (G4 report): the entry's plasma streaks and the
//! storm's streaks are drawn on the view from the clock (they live in
//! screen space, and the plasma's path lies out of the pool's range);
//! the dust under the ship and the take-off's ring are drawn from the
//! lander's height (G2); the wreck's smoke column from the clock. None
//! of them takes a slot.

use super::Below;
use super::models;
use super::palette;
use super::site;
use super::tune::SPARKS;
use crate::art;
use crate::fixed::{V3, cos, hash, sin, sub};
use crate::poly::{Camera, SUB};
use crate::scene::dot_px;
use crate::world::M;
use game_kit::Frame;

/// The kinds (low four bits of `Spark::kind`; the variant is the high
/// four).
pub const NONE: u8 = 0;
pub const DUST: u8 = 1;
pub const GLITTER: u8 = 2;
pub const SPLASH: u8 = 3;
pub const CHIP_ORE: u8 = 4;
pub const CHIP_METAL: u8 = 5;
pub const CHIP_DUST: u8 = 6;
pub const JET: u8 = 7;
pub const SCORCH: u8 = 8;
pub const RING: u8 = 9;
pub const SPARK: u8 = 10;
pub const PART: u8 = 11;
pub const PART_B: u8 = 12;
pub const TRAIL: u8 = 13;

/// Each kind's life (ticks).
const LIFE: [u8; 14] = [0, 24, 14, 18, 9, 9, 9, 8, 120, 40, 20, 60, 45, 30];

/// One spark (8 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spark {
    /// Its place in 1/128 m (world units / 2).
    pub p: [i16; 3],
    /// Ticks left (0: a free slot).
    pub life: u8,
    /// Kind (low 4 bits) and variant (high 4 bits).
    pub kind: u8,
}

impl Spark {
    pub const NONE: Spark = Spark {
        p: [0; 3],
        life: 0,
        kind: NONE,
    };

    /// Its place in world units.
    pub fn at(&self) -> V3 {
        self.p.map(|v| i32::from(v) * 2)
    }
}

/// The pool (kept in `Below`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pool {
    pub s: [Spark; SPARKS],
    /// Where the next search for a free slot starts.
    pub next: u8,
}

impl Pool {
    pub const EMPTY: Pool = Pool {
        s: [Spark::NONE; SPARKS],
        next: 0,
    };

    /// Sparks alive now.
    pub fn live(&self) -> usize {
        self.s.iter().filter(|s| s.life > 0).count()
    }
}

/// A spark of `kind` (variant `v`, 0..16) at `at` (world units): in the
/// first free slot from `next` on, or, with none free, in the slot at
/// `next` (the oldest is usually there).
pub fn add(pool: &mut Pool, at: V3, kind: u8, v: u8) {
    let start = usize::from(pool.next) % SPARKS;
    let mut slot = start;
    for k in 0..SPARKS {
        let i = (start + k) % SPARKS;
        if pool.s[i].life == 0 {
            slot = i;
            break;
        }
    }
    let c = |x: i32| (x >> 1).clamp(-i32::from(i16::MAX), i32::from(i16::MAX)) as i16;
    let kind = kind.min(TRAIL);
    pool.s[slot] = Spark {
        p: [c(at[0]), c(at[1]), c(at[2])],
        life: LIFE[usize::from(kind)],
        kind: kind | (v << 4),
    };
    pool.next = ((slot + 1) % SPARKS) as u8;
}

/// The step of spark `i` this tick (1/128 m).
fn step(s: &Spark, i: usize) -> [i32; 3] {
    let kind = s.kind & 15;
    let age = i32::from(LIFE[usize::from(kind)].saturating_sub(s.life));
    let h = hash(i as u32, u32::from(s.kind) << 8 | 0x5A);
    let a = (u16::from(s.kind >> 4) << 12).wrapping_add((h & 0xFFF) as u16);
    let out = |r: i32| [(sin(a) * r) >> 14, (cos(a) * r) >> 14];
    let every = |n: i32| i32::from(age % n == 0);
    let (o, up) = match kind {
        DUST => (out(every(2)), every(2)),
        SPLASH => (out(1), 3 - age / 3),
        CHIP_ORE | CHIP_METAL | CHIP_DUST => (out(2 + (h >> 28) as i32 % 3), 4 - age),
        JET => (out(((h >> 20) & 1) as i32), -5),
        RING => (out(4), every(4)),
        SPARK => (out(5), ((h >> 24) % 9) as i32 - 3 - age / 4),
        PART => (out(2), 6 - age / 2),
        PART_B => (out(1), 3 - age / 2),
        TRAIL => ([0, 0], every(3)),
        _ => ([0, 0], 0),
    };
    [o[0], up, o[1]]
}

/// One tick of the pool: each spark ages and moves; a part that meets
/// the ground bounces once (then lies still).
#[inline(never)]
pub fn tick(b: &mut Below) {
    for i in 0..SPARKS {
        let mut s = b.sparks.s[i];
        if s.life == 0 {
            continue;
        }
        s.life -= 1;
        if s.life == 0 {
            s.kind = NONE;
            b.sparks.s[i] = s;
            continue;
        }
        let d = step(&s, i);
        let n = [0, 1, 2].map(|k| {
            (i32::from(s.p[k]) + d[k]).clamp(-i32::from(i16::MAX), i32::from(i16::MAX)) as i16
        });
        let kind = s.kind & 15;
        if kind == PART || kind == PART_B {
            let g = site::surface(&b.site, i32::from(n[0]) * 2, i32::from(n[2]) * 2) / 2;
            if i32::from(n[1]) <= g {
                if kind == PART {
                    s.kind = (s.kind & 0xF0) | PART_B;
                    s.life = LIFE[usize::from(PART_B)];
                    s.p = [n[0], g as i16, n[2]];
                } else {
                    s.p[1] = g as i16;
                }
                b.sparks.s[i] = s;
                continue;
            }
        }
        s.p = n;
        b.sparks.s[i] = s;
    }
}

/// A spark's colour and size (px) at `near` (closer than 12 m).
fn look(s: &Spark, near: bool) -> (u8, i32) {
    let big = if near { 2 } else { 1 };
    let fire = [art::FIRE1, art::FIRE2, art::FIRE3][usize::from(s.life % 3)];
    match s.kind & 15 {
        DUST | RING | TRAIL | CHIP_DUST => (palette::DUST, big),
        GLITTER => (art::TEXT, 1),
        SPLASH => (palette::water(0, 0), 1),
        CHIP_ORE => (models::DEPOSIT, 1),
        CHIP_METAL => (art::FIRE1, 1),
        JET | SPARK => (fire, big),
        SCORCH => (palette::SHADOW, big),
        _ => (art::mat::GUNMETAL, big),
    }
}

/// Draws the pool's sparks: `ground` the scorch marks (before the
/// site's things), else the rest (after them). Only within 60 m.
pub fn draw(f: &mut Frame, cam: &Camera, pool: &Pool, ground: bool) {
    let view = &super::VIEW;
    for s in &pool.s {
        if s.life == 0 || ((s.kind & 15) == SCORCH) != ground {
            continue;
        }
        let c = cam.rot.apply(sub(s.at(), cam.pos));
        if c[2] < M / 2 || c[2] > 60 * M {
            continue;
        }
        let p = view.project(c);
        let (colour, size) = look(s, c[2] < 12 * M);
        dot_px(
            f,
            &view.area,
            p[0].div_euclid(SUB),
            p[1].div_euclid(SUB),
            colour,
            size,
        );
    }
}
