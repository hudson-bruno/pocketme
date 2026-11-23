//! Effects: dust from the engine near the ground, and the pieces of a
//! broken craft. A fixed list, no heap; when it is full, new bits are
//! dropped (the oldest stay).

use crate::fixed::{PX, Rng};
use crate::level::World;

/// The most bits on show at once.
pub const MAX: usize = 48;

/// What a bit looks like (the drawing picks colours and size).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Dust,
    /// A piece of the hull: 2 x 2 pixels, colour index into the hull's.
    Piece(u8),
    /// A spark: bright, short.
    Spark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bit {
    pub x: i32,
    pub y: i32,
    pub vx: i32,
    pub vy: i32,
    /// Ticks left; 0 means the slot is free.
    pub life: u16,
    pub kind: Kind,
    /// A piece lying still on the ground: it stays until the level
    /// starts again, and costs no pixels.
    pub rest: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bits {
    pub list: [Bit; MAX],
}

impl Default for Bits {
    fn default() -> Self {
        Bits::new()
    }
}

impl Bits {
    pub const fn new() -> Bits {
        Bits {
            list: [Bit {
                x: 0,
                y: 0,
                vx: 0,
                vy: 0,
                life: 0,
                kind: Kind::Dust,
                rest: false,
            }; MAX],
        }
    }

    pub fn clear(&mut self) {
        *self = Bits::new();
    }

    pub fn live(&self) -> usize {
        self.list.iter().filter(|b| b.life > 0).count()
    }

    /// Bits that still move (or will vanish): while there are any, the
    /// picture keeps changing.
    pub fn moving(&self) -> usize {
        self.list.iter().filter(|b| b.life > 0 && !b.rest).count()
    }

    fn add(&mut self, b: Bit) {
        if let Some(slot) = self.list.iter_mut().find(|s| s.life == 0) {
            *slot = b;
        }
    }

    /// Dust thrown up where the engine's blast meets the ground at
    /// column `x`, row `y`. `strength` 0..=8.
    pub fn dust(&mut self, rng: &mut Rng, x: i32, y: i32, strength: i32) {
        for _ in 0..strength.clamp(0, 8) / 4 + 1 {
            let side = if rng.roll() & 1 == 0 { 1 } else { -1 };
            self.add(Bit {
                x: (x + rng.spread(2)) * PX,
                y: (y - 1) * PX,
                vx: side
                    * (PX / 8 + rng.below((PX / 4) as u32) as i32)
                    * (strength.clamp(1, 8) + 2)
                    / 6,
                vy: -(PX / 16) - rng.below((PX / 10) as u32) as i32,
                life: 12 + rng.below(14) as u16,
                kind: Kind::Dust,
                rest: false,
            });
        }
    }

    /// The craft breaks at `(x, y)` (Q16) moving at `(vx, vy)`.
    pub fn burst(&mut self, rng: &mut Rng, x: i32, y: i32, vx: i32, vy: i32) {
        for i in 0..12u8 {
            self.add(Bit {
                x: x + rng.spread(3) * PX,
                y: y + rng.spread(3) * PX,
                vx: vx / 3 + rng.spread(PX / 4),
                vy: vy / 4 - PX / 8 - rng.below((PX / 5) as u32) as i32,
                // Long enough to land: then they lie still (`rest`).
                life: 400 + rng.below(100) as u16,
                kind: Kind::Piece(i % 4),
                rest: false,
            });
        }
        for _ in 0..16 {
            self.add(Bit {
                x,
                y,
                vx: rng.spread(PX),
                vy: rng.spread(PX) - PX / 3,
                life: 10 + rng.below(16) as u16,
                kind: Kind::Spark,
                rest: false,
            });
        }
    }

    /// One tick: move, fall, bounce on the ground, fade.
    pub fn tick(&mut self, world: &World) {
        let g = world.params.gravity;
        for b in self.list.iter_mut().filter(|b| b.life > 0 && !b.rest) {
            b.life -= 1;
            let (fall, drag) = match b.kind {
                Kind::Dust => (g / 4, true),
                Kind::Piece(_) => (g * 4, false),
                Kind::Spark => (g, false),
            };
            b.vy = (b.vy + fall).clamp(-2 * PX, 2 * PX);
            if drag {
                b.vx -= b.vx / 16;
            }
            b.x = b.x.saturating_add(b.vx);
            b.y = b.y.saturating_add(b.vy);
            let (px, py) = (b.x.div_euclid(PX), b.y.div_euclid(PX));
            if px < -4 || px > i32::from(world.params.width) + 4 || !(-200..=130).contains(&py) {
                b.life = 0;
                continue;
            }
            let gy = world.ground_at(px);
            if py >= gy {
                match b.kind {
                    Kind::Piece(_) => {
                        // Bounce, losing most of the speed.
                        b.y = (gy - 1) * PX;
                        b.vy = -b.vy / 3;
                        b.vx /= 2;
                        if b.vy.abs() < PX / 16 {
                            b.vy = 0;
                            if b.vx.abs() < PX / 16 {
                                // Settled: it lies here from now on.
                                b.vx = 0;
                                b.rest = true;
                            }
                        }
                    }
                    _ => b.life = 0,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_come_to_rest_on_the_ground_and_the_list_never_overflows() {
        let w = World::make(1);
        let mut bits = Bits::new();
        let mut rng = Rng::new(7);
        for _ in 0..10 {
            bits.burst(&mut rng, 80 * PX, 60 * PX, PX, PX);
        }
        assert_eq!(bits.live(), MAX);
        for _ in 0..60 {
            bits.tick(&w);
        }
        for b in bits.list.iter().filter(|b| b.life > 0) {
            let (x, y) = (b.x / PX, b.y / PX);
            assert!(y < w.ground_at(x), "{x} {y}");
        }
        for _ in 0..600 {
            bits.tick(&w);
        }
        // Sparks are gone; the pieces lie still on the ground (or flew
        // out of the world).
        assert_eq!(bits.moving(), 0);
        assert!(bits.live() > 0);
        assert!(bits.list.iter().filter(|b| b.life > 0).all(|b| b.rest));
        let before = bits;
        bits.tick(&w);
        assert_eq!(bits, before);
    }
}
