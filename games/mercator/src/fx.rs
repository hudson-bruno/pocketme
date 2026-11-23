//! Short-lived things that only make the view livelier: sparks where a
//! laser hits, pieces of a destroyed ship tumbling away, the stars
//! streaming past after a jump, the view shaking under fire.
//!
//! **No dice.** Nothing here uses the world's dice or changes what
//! happens in a fight: directions come from [`hash`] of a running
//! number, so the balance tests see exactly the same game.
//!
//! **Where they are drawn.** A spark or a piece keeps the point it
//! started from (`at`) and how far it has flown since (`off`). The
//! scene draws them at `at + off`, in true perspective like the ships
//! (step M2: there is no distance floor any more).
//!
//! Every effect can be switched off here (the scene and the world test
//! these constants; a switched-off effect costs no time and its code is
//! left out by the compiler).

use crate::fixed::{V3, hash, norm};
use crate::world::clamp_pos;

/// Sparks at laser hits and in explosions.
pub const SPARKS_ON: bool = true;
/// Tumbling pieces of a destroyed ship.
pub const DEBRIS_ON: bool = true;
/// The view shakes when the player's ship is hit or crashes, and the
/// shield shows a ripple (or the view a red edge when the shield is down).
pub const SHAKE_ON: bool = true;
/// Stars drawn as streaks after a jump and in cruise.
pub const STREAKS_ON: bool = true;
/// A coloured nebula far behind the stars (MID and HIGH detail).
pub const NEBULA_ON: bool = true;
/// The sun's corona and slowly turning rays.
pub const CORONA_ON: bool = true;
/// Ghost rings across the view when the sun is in sight (MID, HIGH).
pub const FLARE_ON: bool = true;
/// A glowing rim of air round the planet, brighter on its day side.
pub const AIR_ON: bool = true;
/// Blinking lights on the station's wing tips and a strobe on its hub.
pub const LIGHTS_ON: bool = true;
/// Running guide lights in front of the station's mouth.
pub const GUIDE_ON: bool = true;
/// Engine trails that grow with a ship's speed.
pub const TRAILS_ON: bool = true;
/// A shock ring round each explosion.
pub const SHOCK_ON: bool = true;
/// Step M3: a ship below [`SMOKE_HALF`] of its hull trails smoke, and
/// heavy smoke with flickers of fire below [`SMOKE_HEAVY`] (where a
/// raider turns to flee); the player's own ship too, in the chase view,
/// and wisps rise over the cockpit view. Drawn from the tick alone: no
/// state, no dice.
pub const SMOKE_ON: bool = true;
/// Step M3: while docked the scene behind the menus lives on (the
/// station turns, traffic passes, `World::scenery`) and the game draws
/// 30 pictures a second there instead of resting. Off: the docked
/// screens are still pictures again, drawn only when something changes.
pub const DOCK_LIVE_ON: bool = true;

/// Hull shares (percent) under which a ship smokes, and heavily
/// (the heavy one: the share where a raider flees, 3/10 in `npcs`).
pub const SMOKE_HALF: i32 = 50;
pub const SMOKE_HEAVY: i32 = 30;

/// How a ship with `hp` of `hull` smokes: 0 not, 1 light, 2 heavy.
#[inline(never)]
pub fn smoke_of(hp: i32, hull: i32) -> u8 {
    if !SMOKE_ON || hull <= 0 {
        return 0;
    }
    // Hit points below 2^12: the products stay small.
    let p = hp.max(0).wrapping_mul(100);
    if p < hull.wrapping_mul(SMOKE_HEAVY) {
        2
    } else if p < hull.wrapping_mul(SMOKE_HALF) {
        1
    } else {
        0
    }
}

/// Puffs drawn for each smoke level.
pub const PUFFS: [u32; 3] = [0, 3, 5];

pub const SPARKS: usize = 24;
pub const BITS: usize = 12;
/// Ticks a piece of a wreck flies.
pub const BIT_LIFE: u8 = 100;
/// Ticks the stars stream after a jump.
pub const WARP_TICKS: u8 = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spark {
    pub at: V3,
    pub off: V3,
    /// Per tick, 1/256 m.
    pub vel: [i16; 3],
    pub age: u8,
    /// Dead when `age >= life`.
    pub life: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bit {
    pub at: V3,
    pub off: V3,
    pub vel: [i16; 3],
    /// Yaw and pitch, turning by `spin` each tick.
    pub ang: [u16; 2],
    pub spin: [i16; 2],
    pub age: u8,
    /// The wreck's main material (the piece's outer faces).
    pub mat: u8,
}

const NO_SPARK: Spark = Spark {
    at: [0; 3],
    off: [0; 3],
    vel: [0; 3],
    age: 0,
    life: 0,
};

const NO_BIT: Bit = Bit {
    at: [0; 3],
    off: [0; 3],
    vel: [0; 3],
    ang: [0; 2],
    spin: [0; 2],
    age: BIT_LIFE,
    mat: 0,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fx {
    pub sparks: [Spark; SPARKS],
    pub bits: [Bit; BITS],
    /// Ticks the stars still stream after a jump.
    pub warp: u8,
    /// Ticks the view still shakes after a hit.
    pub jolt: u8,
    seq: u32,
}

impl Fx {
    pub const NONE: Fx = Fx {
        sparks: [NO_SPARK; SPARKS],
        bits: [NO_BIT; BITS],
        warp: 0,
        jolt: 0,
        seq: 0,
    };

    /// One tick: everything flies on and ages.
    #[inline(never)]
    pub fn tick(&mut self) {
        self.warp = self.warp.saturating_sub(1);
        self.jolt = self.jolt.saturating_sub(1);
        // Offsets: at most 255 ticks of at most 2^15 a tick, far inside
        // i32; the wrapping sums are exact.
        let fly = |off: &mut V3, vel: &[i16; 3]| {
            for k in 0..3 {
                off[k] = off[k].wrapping_add(i32::from(vel[k]));
            }
        };
        for s in self.sparks.iter_mut().filter(|s| s.age < s.life) {
            fly(&mut s.off, &s.vel);
            s.age = s.age.wrapping_add(1);
        }
        for b in self.bits.iter_mut().filter(|b| b.age < BIT_LIFE) {
            fly(&mut b.off, &b.vel);
            b.ang[0] = b.ang[0].wrapping_add(b.spin[0] as u16);
            b.ang[1] = b.ang[1].wrapping_add(b.spin[1] as u16);
            b.age = b.age.wrapping_add(1);
        }
    }

    /// The next direction, `len` long (len below 2^14).
    fn dir(&mut self, len: i32) -> V3 {
        self.seq = self.seq.wrapping_add(1);
        let h = hash(0xF1A5, self.seq);
        let g = hash(h, 3);
        let part = |v: u32| ((v % 2001) as i32).wrapping_sub(1000);
        let v = [part(h), part(h >> 11), part(g)];
        let v = norm(v, len);
        if v == [0; 3] { [0, len, 0] } else { v }
    }

    /// `n` sparks flying out from `at` at about `speed` (1/256 m a tick,
    /// below 2^14), each living up to `life` ticks.
    #[inline(never)]
    pub fn sparks(&mut self, at: V3, n: u32, speed: i32, life: u8) {
        if !SPARKS_ON {
            return;
        }
        for _ in 0..n {
            // The oldest (or a dead) one makes room.
            let k = (0..SPARKS)
                .max_by_key(|&k| {
                    let s = &self.sparks[k];
                    if s.age >= s.life { 255 } else { s.age }
                })
                .unwrap_or(0);
            let h = hash(self.seq, 9);
            // Speeds 1/2 to 1 of `speed`, lives 2/3 to 1 of `life`.
            // Below 2^14 times 2^8: no sum leaves i32.
            let sp = (speed / 2).wrapping_add(((h % 256) as i32).wrapping_mul(speed / 2) / 256);
            let v = self.dir(sp);
            self.sparks[k] = Spark {
                // A shot's end may lie past the world's edge.
                at: clamp_pos(at),
                off: [0; 3],
                vel: v.map(|c| c as i16),
                age: 0,
                life: life.wrapping_sub((h >> 8) as u8 % (life / 3).wrapping_add(1)),
            };
        }
    }

    /// `n` pieces of a wreck of material `mat` at `at`, all drifting on
    /// at half of `drift` (1/256 m a tick: the wreck's own speed, below
    /// 2^13), flying apart `grown` (1..=6: the wreck's growth in step
    /// M2) times as fast as before.
    #[inline(never)]
    pub fn debris(&mut self, at: V3, drift: V3, mat: u8, n: u32, grown: i32) {
        if !DEBRIS_ON {
            return;
        }
        for _ in 0..n {
            let k = (0..BITS).max_by_key(|&k| self.bits[k].age).unwrap_or(0);
            let h = hash(self.seq, 5);
            // Below 520 times 6.
            let sp = 200i32
                .wrapping_add((h % 320) as i32)
                .wrapping_mul(grown.clamp(1, 6));
            let v = self.dir(sp);
            // Each part below 2^12 + 2^12: an i16.
            self.bits[k] = Bit {
                at: clamp_pos(at),
                off: [0; 3],
                vel: [0, 1, 2].map(|c| v[c].wrapping_add(drift[c].clamp(-8192, 8192) / 2) as i16),
                ang: [h as u16, (h >> 16) as u16],
                spin: [
                    ((h >> 3) % 1600) as i16 - 800,
                    ((h >> 13) % 1600) as i16 - 800,
                ],
                age: 0,
                mat,
            };
        }
    }
}
