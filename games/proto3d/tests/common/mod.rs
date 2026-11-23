//! Shared helpers for the rule tests: pads, small test worlds and a bot
//! that steers the hero to points.
#![allow(dead_code)]

use game_kit::button;
use proto3d::hero::Hero;
use proto3d::island::{Block, World, flag, mat, ramp};
use proto3d::maths::cm;
use proto3d::play::Play;
use proto3d::step::Pad;

/// Full stick forward (camera's way), in the game's terms.
pub const FWD: (i16, i16) = (0, -32_767);

/// A pad with the left stick at `s` and these buttons held, nothing new.
pub fn pad(s: (i16, i16), held: u32) -> Pad {
    Pad {
        held,
        pressed: 0,
        released: 0,
        left: s,
        right: (0, 0),
    }
}

/// The same, with A pressed in this step (and held).
pub fn jump_pad(s: (i16, i16), held: u32) -> Pad {
    Pad {
        held: held | button::A,
        pressed: button::A,
        ..pad(s, 0)
    }
}

/// A flat walkable box in cm.
pub const fn flat(lo: [i16; 3], hi: [i16; 3]) -> Block {
    Block {
        lo,
        hi,
        top: mat::GRASS,
        side: mat::DIRT,
        flags: flag::WALK,
        ramp: ramp::FLAT,
    }
}

/// A hero standing at (x, y, z) cm on block `top`.
pub fn hero_at(x: i16, y: i16, z: i16, top: u8) -> Hero {
    Hero::new([cm(x), cm(y), cm(z)], top)
}

/// Runs the hero `n` steps with one pad, camera yaw 0 (stick forward
/// is +z). Returns the highest feet height seen.
pub fn run(h: &mut Hero, p: &Pad, n: usize, blocks: &[Block]) -> i32 {
    let mut top = h.pos[1];
    for _ in 0..n {
        h.step(p, 0, blocks);
        top = top.max(h.pos[1]);
    }
    top
}

/// The stick that runs towards the world direction (dx, dz) for a
/// camera with this yaw, at `speed` (0 to 1).
pub fn stick_towards(dx: f64, dz: f64, cam_yaw: u16, speed: f64) -> (i16, i16) {
    let l = (dx * dx + dz * dz).sqrt();
    if l == 0.0 {
        return (0, 0);
    }
    let a = f64::from(cam_yaw) * core::f64::consts::TAU / 65_536.0;
    let (s, c) = (a.sin(), a.cos());
    // Camera right is (cos, -sin), forward (sin, cos).
    let r = (dx * c - dz * s) / l;
    let f = (dx * s + dz * c) / l;
    // Past the dead zone: stick length 6000 + mag * (32767 - 6000).
    let len = 6_000.0 + speed.clamp(0.0, 1.0) * 26_767.0;
    ((r * len).round() as i16, (-f * len).round() as i16)
}

/// One leg of a scripted route.
#[derive(Clone, Copy, Debug)]
pub struct Leg {
    /// Where to go (cm, x and z).
    pub to: [i16; 2],
    /// Stick strength (0 to 1).
    pub speed: f64,
    /// Press A once when the hero's middle is this near `to` (cm).
    pub jump_within: Option<i32>,
    /// The leg is done when the hero stands on this block within 40 cm
    /// of `to` (or anywhere on it when `anywhere`).
    pub on: u8,
    pub anywhere: bool,
}

/// Follows legs with the full rules (`Play`). Calls `each` after every
/// step. Panics with the hero's state if a leg takes over `limit` steps.
pub fn follow(play: &mut Play, legs: &[Leg], limit: usize, mut each: impl FnMut(&Play)) {
    for (n, leg) in legs.iter().enumerate() {
        let mut jumped = false;
        let mut steps = 0;
        let mut held_a = 0;
        loop {
            let h = &play.hero;
            let dx = f64::from(cm(leg.to[0]) - h.pos[0]);
            let dz = f64::from(cm(leg.to[1]) - h.pos[2]);
            let d_cm = (dx * dx + dz * dz).sqrt() * 25.0 / 1024.0;
            let here = h.ground == Some(leg.on) && h.frozen == 0;
            if here && (leg.anywhere || d_cm < 40.0) {
                break;
            }
            let near = (d_cm / 100.0).min(1.0);
            let speed = leg.speed * near.max(0.25);
            let s = stick_towards(dx, dz, play.cam.yaw, speed);
            let mut p = pad(s, 0);
            if let Some(j) = leg.jump_within
                && !jumped
                && d_cm <= f64::from(j)
                && h.ground.is_some()
            {
                p = jump_pad(s, 0);
                jumped = true;
                held_a = 20;
            } else if held_a > 0 {
                p.held |= button::A;
                held_a -= 1;
            }
            play.step(&p);
            each(play);
            steps += 1;
            assert!(
                steps < limit,
                "leg {n} {leg:?} not done in {limit} steps: hero {:?}",
                play.hero
            );
        }
    }
}

/// A small pseudo-random number source (the same every run).
pub struct Lcg(pub u64);

impl Lcg {
    pub fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }

    /// A random pad: sticks, the whole d-pad, every button (A, B, X,
    /// START, SELECT and the shoulders), changing now and then (`held` is
    /// the last pad's buttons). Rules review F7: it once made only A, L,
    /// R, UP and LEFT, so no random run glided, carried or placed.
    pub fn pad(&mut self, last: &Pad) -> Pad {
        let r = self.next();
        if !r.is_multiple_of(8) {
            let mut p = *last;
            p.pressed = 0;
            p.released = 0;
            return p;
        }
        let stick = |v: u32| (v % 65_535) as i32 - 32_767;
        let left = (stick(self.next()) as i16, stick(self.next()) as i16);
        let right = if self.next().is_multiple_of(3) {
            (stick(self.next()) as i16, stick(self.next()) as i16)
        } else {
            (0, 0)
        };
        let mut held = 0;
        for b in [
            button::A,
            button::B,
            button::X,
            button::START,
            button::SELECT,
            button::L,
            button::R,
            button::UP,
            button::DOWN,
            button::LEFT,
            button::RIGHT,
        ] {
            if self.next().is_multiple_of(4) {
                held |= b;
            }
        }
        Pad {
            held,
            pressed: held & !last.held,
            released: last.held & !held,
            left,
            right,
        }
    }
}

/// A world on its own (leaked once per test, fine in tests).
pub fn world(blocks: &'static [Block], start: [i16; 3]) -> &'static World {
    Box::leak(Box::new(World {
        blocks,
        props: &[],
        coins: &[],
        coins_low: 0,
        start,
        critters: &[],
    }))
}

/// A leg that goes to `to` on block `on`.
pub const fn leg(to: [i16; 2], speed: f64, jump_within: Option<i32>, on: u8) -> Leg {
    Leg {
        to,
        speed,
        jump_within,
        on,
        anywhere: false,
    }
}

/// A route over the whole test island, from the start: onto the step
/// (jump), the floating block (jump), the far corner, up the ramp, over
/// the terrace, down to the main top and across the gap to the islet.
/// It passes every collectible.
pub const ROUTE: [Leg; 8] = {
    use proto3d::island::{FLOAT, ISLET, MAIN, STEP, TERRACE};
    [
        leg([200, 0], 0.5, Some(170), STEP),
        leg([280, 380], 1.0, Some(310), FLOAT),
        leg([-350, -500], 1.0, None, MAIN),
        leg([-350, -250], 1.0, None, MAIN),
        leg([-350, 300], 1.0, None, TERRACE),
        leg([-400, 400], 1.0, None, TERRACE),
        leg([0, 450], 1.0, None, MAIN),
        leg([0, 1150], 1.0, Some(450), ISLET),
    ]
};
