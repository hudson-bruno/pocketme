//! Levels: how hard each one is, and its ground.
//!
//! The ground comes from a random number maker seeded with the level
//! number, so level 7 always has the same ground, on the PC and on the
//! chip. Levels never run out: after level [`HARDEST`] the numbers stop
//! growing, but the ground still changes with each level.

use crate::fixed::{PX, Rng};

/// The widest world, in pixels (two screens).
pub const MAX_WORLD: usize = 320;
/// Ground is never higher (smaller y) than this, so the craft has room.
pub const GROUND_TOP: i32 = 58;
/// ... and never lower than this (the picture is 120 rows).
pub const GROUND_LOW: i32 = 114;
/// The ground is made from points this far apart, joined by lines.
const KNOT: usize = 8;
/// The most landing pads in a level.
pub const MAX_PADS: usize = 3;
/// From this level on the numbers stop growing.
pub const HARDEST: u16 = 12;
/// From this level on the world is two screens wide (and scrolls).
pub const WIDE_FROM: u16 = 3;

/// One flat landing pad.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pad {
    /// Left column and width, in pixels.
    pub x: u16,
    pub w: u16,
    /// The row of its surface.
    pub y: u8,
    /// Its score multiplier: smaller pads pay more.
    pub mult: u8,
}

impl Pad {
    /// Whether column `x` (whole pixels) is on the pad.
    pub fn holds(&self, x: i32) -> bool {
        x >= i32::from(self.x) && x < i32::from(self.x) + i32::from(self.w)
    }
}

/// How hard a level is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    /// The level's number (1 and up).
    pub number: u16,
    /// Pull down, Q16 pixels per tick per tick.
    pub gravity: i32,
    /// Fuel at the start, in fuel units (a full burn uses [`BURN`] a tick).
    pub fuel: u32,
    /// World width in pixels: 160 or 320.
    pub width: u16,
    /// Steady wind, Q16 pixels per tick per tick (+ blows right).
    pub wind: i32,
    /// How much the wind swings around the steady value (same units).
    pub gust: i32,
    /// How far the ground may climb or drop between two points.
    pub rough: i32,
    /// Pad widths (0: no pad).
    pub pads: [u16; MAX_PADS],
}

/// Fuel used per tick at full thrust.
pub const BURN: u32 = 64;
/// Ticks per second.
pub const HZ: u32 = 60;

impl Params {
    /// The numbers of level `n` (0 counts as 1).
    pub fn of(n: u16) -> Params {
        let n = n.max(1);
        let k = i32::from(n.min(HARDEST)) - 1; // 0 ..= 11
        let mut rng = Rng::new(0x51ED_0000 ^ u32::from(n));
        // Gravity: about 3.3 pixels/s^2 at level 1, 6.3 at the hardest.
        let gravity = 60 + 5 * k;
        // Fuel: 30 s of full burn at level 1, 13 s at the hardest.
        let secs = (30 - 3 * k / 2).max(13) as u32;
        let width = if n >= WIDE_FROM { 320 } else { 160 };
        let side = if rng.roll() & 1 == 0 { 1 } else { -1 };
        // Wind and gusts together stay under a fifth of gravity: leaning
        // as far as a landing allows (8 steps, about 11 degrees), the
        // engine can still hold the craft still against them. See
        // `the_wind_can_always_be_held`.
        let wind = if n >= WIDE_FROM {
            side * (3 * k / 2).min(12)
        } else {
            0
        };
        let gust = if n >= 5 { k.min(7) } else { 0 };
        let pads = match n {
            1 => [32, 24, 16],
            2 => [24, 16, 16],
            3..=5 => [24, 24, 16],
            6..=8 => [24, 16, 16],
            _ => [16, 16, 0],
        };
        Params {
            number: n,
            gravity,
            fuel: secs * HZ * BURN,
            width,
            wind,
            gust,
            rough: 8 + k * 2,
            pads,
        }
    }

    /// The wind now, after `tick` ticks of flight.
    pub fn wind_at(&self, tick: u32) -> i32 {
        if self.gust == 0 {
            return self.wind;
        }
        // A slow triangle wave, 8 s long: -gust .. +gust.
        const PERIOD: i32 = 480;
        let p = (tick % PERIOD as u32) as i32;
        let tri = (2 * p - PERIOD).abs() * 2 - PERIOD; // -480 ..= 480
        self.wind + self.gust * tri / PERIOD
    }
}

/// The ground of one level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct World {
    pub params: Params,
    /// The surface row of each column (`width` of them are used).
    pub ground: [u8; MAX_WORLD],
    pub pads: [Pad; MAX_PADS],
    pub pad_count: u8,
    /// Where the craft starts, whole pixels, and its first push.
    pub start_x: i32,
    pub start_vx: i32,
}

impl World {
    /// Level `n`'s ground: always the same for the same `n`.
    pub fn make(n: u16) -> World {
        let params = Params::of(n);
        let width = usize::from(params.width);
        let mut rng = Rng::new(0x1A4D_E500 ^ (u32::from(params.number) << 3));
        let knots = width / KNOT + 1;
        let mut h = [0i32; MAX_WORLD / KNOT + 1];
        let mut y = 90 + rng.spread(12);
        for v in h.iter_mut().take(knots) {
            y = (y + rng.spread(params.rough)).clamp(GROUND_TOP + 6, GROUND_LOW - 4);
            *v = y;
        }
        // The craft starts near one side and drifts to the other.
        let left = rng.roll() & 1 == 0;
        let start_x = if left {
            16 + rng.below(16) as i32
        } else {
            width as i32 - 16 - rng.below(16) as i32
        };
        let start_vx = (PX / 60) * (8 + rng.below(8) as i32) * if left { 1 } else { -1 };

        // Pads: flat runs of knots, apart from each other, never right
        // under the start.
        let mut pads = [Pad::default(); MAX_PADS];
        let mut taken = [false; MAX_WORLD / KNOT + 1];
        let mut count = 0;
        for &w in params.pads.iter().filter(|&&w| w > 0) {
            let span = usize::from(w) / KNOT; // knots covered: span + 1
            // Random places first; if none fits, the first place that does.
            let last = knots - span - 2; // k0 runs 1 ..= last
            let tries = (0..40)
                .map(|_| 1 + rng.below(last as u32) as usize)
                .chain(1..=last);
            let fits = |k0: usize, taken: &[bool]| {
                let x0 = (k0 * KNOT) as i32;
                let clear =
                    (k0.saturating_sub(1)..=(k0 + span + 1).min(knots - 1)).all(|k| !taken[k]);
                let under_start = (x0 - 8..x0 + i32::from(w) + 8).contains(&start_x);
                clear && !under_start
            };
            let mut found = None;
            for k0 in tries {
                if fits(k0, &taken) {
                    found = Some(k0);
                    break;
                }
            }
            if let Some(k0) = found {
                let x0 = (k0 * KNOT) as i32;
                let py = h[k0..=k0 + span].iter().sum::<i32>() / (span as i32 + 1);
                let py = py.clamp(GROUND_TOP + 10, GROUND_LOW - 6);
                for k in k0..=k0 + span {
                    h[k] = py;
                    taken[k] = true;
                }
                pads[count] = Pad {
                    x: x0 as u16,
                    w,
                    y: py as u8,
                    mult: match w {
                        0..=16 => 4,
                        17..=24 => 2,
                        _ => 1,
                    },
                };
                count += 1;
            }
        }
        pads[..count].sort_unstable_by_key(|p| p.x);

        let mut ground = [GROUND_LOW as u8; MAX_WORLD];
        for (x, g) in ground.iter_mut().enumerate().take(width) {
            let k = x / KNOT;
            let (a, b) = (h[k], h[(k + 1).min(knots - 1)]);
            let t = (x % KNOT) as i32;
            let mut y = a + (b - a) * t / KNOT as i32;
            // A little roughness, never on a pad.
            if !pads[..count].iter().any(|p| p.holds(x as i32)) {
                y += rng.spread(1);
            }
            *g = y.clamp(GROUND_TOP, GROUND_LOW) as u8;
        }
        World {
            params,
            ground,
            pads,
            pad_count: count as u8,
            start_x,
            start_vx,
        }
    }

    /// The surface row under column `x` (whole pixels). Outside the
    /// world: the lowest row, so nothing stands there.
    pub fn ground_at(&self, x: i32) -> i32 {
        if x < 0 || x >= i32::from(self.params.width) {
            return 200;
        }
        i32::from(self.ground[x as usize])
    }

    pub fn pads(&self) -> &[Pad] {
        &self.pads[..usize::from(self.pad_count)]
    }

    /// The pad under column `x`, if any.
    pub fn pad_at(&self, x: i32) -> Option<&Pad> {
        self.pads().iter().find(|p| p.holds(x))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_level_gives_the_same_ground() {
        for n in [1, 2, 3, 9, 40, u16::MAX] {
            assert_eq!(World::make(n), World::make(n));
        }
        assert_ne!(World::make(4).ground, World::make(5).ground);
    }

    #[test]
    fn every_level_has_its_pads_flat_and_inside() {
        for n in 1..=400u16 {
            let w = World::make(n);
            let want = w.params.pads.iter().filter(|&&p| p > 0).count();
            assert_eq!(usize::from(w.pad_count), want, "level {n}");
            for p in w.pads() {
                assert!(u32::from(p.x) + u32::from(p.w) <= u32::from(w.params.width));
                for x in p.x..p.x + p.w {
                    assert_eq!(w.ground[usize::from(x)], p.y, "level {n}");
                }
                assert!(!p.holds(w.start_x));
            }
            for x in 0..usize::from(w.params.width) {
                let g = i32::from(w.ground[x]);
                assert!((GROUND_TOP..=GROUND_LOW).contains(&g));
            }
        }
    }

    #[test]
    fn the_wind_can_always_be_held() {
        // tan(11.25 degrees) is 0.199; keep under 0.19 of gravity.
        for n in 0..=40u16 {
            let p = Params::of(n);
            assert!((p.wind.abs() + p.gust) * 100 <= p.gravity * 19, "level {n}");
        }
    }

    #[test]
    fn levels_get_harder_then_stay() {
        let (a, b) = (Params::of(1), Params::of(HARDEST));
        assert!(b.gravity > a.gravity && b.fuel < a.fuel);
        assert_eq!(Params::of(HARDEST + 5).gravity, b.gravity);
        assert_eq!(Params::of(0), Params::of(1));
        for n in [1, 5, 12, 300] {
            let p = Params::of(n);
            for t in [0, 1, 239, 240, 479, 480, u32::MAX] {
                assert!(p.wind_at(t).abs() <= p.wind.abs() + p.gust);
            }
        }
    }
}
