//! Things to collect (PROTOTYPE-SPEC.md, section 4): small gold nuts that
//! spin and bob; one bit each says whether it was taken.
//!
//! One is taken when the hero's square is within [`TAKE`] of its middle on
//! x and on z, and its middle's height is between the feet minus
//! [`TAKE_Y`] and the head plus [`TAKE_Y`]. The bob is only drawn; the
//! take test uses the middle without it.

use game_3d::sin;

use crate::hero::Hero;
use crate::island::World;
use crate::tune::{BOB, BOB_STEPS, HALF, HERO_H, SPIN, TAKE, TAKE_Y};

/// The collectibles' state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Coins {
    /// Bit `i` set: coin `i` was taken.
    pub taken: u8,
    /// Turn of every coin (1/65536 of a turn).
    pub spin: u16,
    /// Step in the bob, 0 to `BOB_STEPS - 1`.
    pub bob_step: u16,
}

/// Most collectibles a world may have: the taken bits are one `u8`.
/// Collectibles past this many do not exist.
pub const MAX_COINS: usize = 8;

/// Whether coin `i` exists at this detail (0 LOW, 1 MID, 2 HIGH).
pub fn exists(world: &World, i: usize, detail: u8) -> bool {
    i < world.coins.len().min(MAX_COINS) && (detail > 0 || i < usize::from(world.coins_low))
}

impl Coins {
    pub const fn new() -> Coins {
        Coins {
            taken: 0,
            spin: 0,
            bob_step: 0,
        }
    }

    /// One step: spin, bob, and the first coin the hero takes (at most
    /// one a step; another one in reach goes the next step).
    pub fn step(&mut self, hero: &Hero, world: &World, detail: u8) -> Option<u8> {
        self.spin = self.spin.wrapping_add(SPIN);
        self.bob_step = (self.bob_step + 1) % BOB_STEPS;
        if hero.frozen > 0 {
            return None;
        }
        let (feet, head) = (hero.pos[1], hero.pos[1] + HERO_H);
        for (i, c) in world.coins.iter().enumerate() {
            if !exists(world, i, detail) || self.taken & (1 << i) != 0 {
                continue;
            }
            let p = c.pos();
            let reach = HALF + TAKE;
            if (p[0] - hero.pos[0]).abs() <= reach
                && (p[2] - hero.pos[2]).abs() <= reach
                && p[1] >= feet - TAKE_Y
                && p[1] <= head + TAKE_Y
            {
                self.taken |= 1 << i;
                return Some(i as u8);
            }
        }
        None
    }

    /// The bob now: subs above the middle (-`BOB` to `BOB`).
    pub fn bob(&self) -> i32 {
        let a = (u32::from(self.bob_step) * 65_536 / u32::from(BOB_STEPS)) as u16;
        BOB * sin(a) / game_3d::ONE14
    }

    /// Taken and existing at this detail: `(n, total)` for the counter.
    pub fn count(&self, world: &World, detail: u8) -> (u8, u8) {
        let mut n = 0;
        let mut total = 0;
        for i in 0..world.coins.len() {
            if exists(world, i, detail) {
                total += 1;
                if self.taken & (1 << i) != 0 {
                    n += 1;
                }
            }
        }
        (n, total)
    }

    /// Whether coin `i` is still there to draw at this detail.
    pub fn shown(&self, world: &World, i: usize, detail: u8) -> bool {
        exists(world, i, detail) && self.taken & (1 << i) == 0
    }
}
