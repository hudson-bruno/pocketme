//! Random play: random buttons, sticks, triggers and fingers, at random
//! and odd call times, from every screen. Nothing may panic (the chip
//! build keeps overflow checks on), the picture may only use the game's
//! palette, and the world must stay inside its rules.
//!
//! Length: `KESTREL_FUZZ` calls (default 12,000); another random
//! sequence: `KESTREL_FUZZ_SEED` (a number). A long run keeps the
//! overflow checks of the chip build on:
//! `CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true KESTREL_FUZZ=300000 cargo test --release --test fuzz`.

mod common;

use common::{Driver, check_palette};
use game_kit::{Frame, Touch};
use kestrel::app::{Keys, Scene};
use kestrel::course::{CORRIDOR_X, CORRIDOR_Y, MISSIONS};
use kestrel::poly::{M8, Renderer};
use kestrel::scene::{self, Detail, Eye};
use kestrel::world::{HULL_MAX, State, World};

struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn i16(&mut self) -> i16 {
        match self.below(4) {
            0 => 0,
            1 => i16::MIN,
            2 => i16::MAX,
            _ => self.next() as i16,
        }
    }
}

fn length() -> usize {
    std::env::var("KESTREL_FUZZ")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12_000)
}

/// The seed for one test: its own, or mixed with `KESTREL_FUZZ_SEED`.
fn seed(own: u64) -> u64 {
    let extra: u64 = std::env::var("KESTREL_FUZZ_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    (own ^ extra.wrapping_mul(0x9E37_79B9_7F4A_7C15)).max(1)
}

fn random_keys(d: &mut Dice, held: &mut u32) -> Keys {
    // Buttons change now and then, so presses and holds both happen.
    if d.below(4) == 0 {
        *held = (d.next() as u32) & (d.next() as u32) & 0xFFFF;
    }
    let pressed = *held & d.next() as u32;
    let touch = match d.below(6) {
        0 | 1 => None,
        n => Some(Touch {
            x: if n == 2 {
                d.i16()
            } else {
                d.below(170) as i16 - 5
            },
            y: if n == 2 {
                d.i16()
            } else {
                d.below(130) as i16 - 5
            },
            down: d.below(3) != 0,
            started: d.below(3) == 0,
            ended: d.below(5) == 0,
        }),
    };
    Keys {
        held: *held,
        pressed,
        left: (d.i16(), d.i16()),
        right: (d.i16(), d.i16()),
        triggers: (d.next() as u16, d.next() as u16),
        touch,
    }
}

fn check_world(w: &World, what: &str) {
    assert!(w.hull <= HULL_MAX, "{what}: hull {}", w.hull);
    assert!(w.ship.x.abs() <= CORRIDOR_X * 16, "{what}: x {}", w.ship.x);
    assert!(w.ship.y.abs() <= CORRIDOR_Y * 16, "{what}: y {}", w.ship.y);
    assert!(usize::from(w.mission) < MISSIONS.len());
    assert!(w.next_gate <= w.m().gates + 1);
    if w.state == State::Lost {
        assert_eq!(w.hull, 0, "{what}");
    }
}

#[test]
fn random_play_never_breaks_a_rule() {
    let calls = length();
    let mut dice = Dice(seed(0x00F0_221E));
    let mut d = Driver::new();
    let mut held = 0u32;
    let mut leaves = 0;
    let mut scenes = [0usize; 5];
    let mut worst_tris = 0;
    for n in 0..calls {
        // Now and then jump into a screen that random keys rarely reach.
        if dice.below(1500) == 0 {
            let m = dice.below(3) as usize;
            d.app.world.reset(m);
            d.app.opts.mission = m as u8;
            d.app.opts.detail = Detail::ALL[dice.below(3) as usize];
            d.app.opts.eye = Eye::ALL[dice.below(3) as usize];
            d.app.scene =
                [Scene::Brief, Scene::Play, Scene::Result, Scene::End][dice.below(4) as usize];
            d.app.scene_t = dice.below(100) as u32;
            if d.app.scene == Scene::Play {
                d.app.world.ship.z = (dice.below(1300) as i32 * M8) << 4;
                d.app.world.next_gate = dice.below(u64::from(d.app.world.m().gates) + 1) as u8;
            }
        }
        // Odd call times: none, tiny, normal, long, very long.
        d.now_us += match dice.below(10) {
            0 => 0,
            1 => dice.below(3_000),
            2 => 1_000_000 + dice.below(60_000_000),
            3 | 4 => 16_667,
            _ => 20_000 + dice.below(40_000),
        };
        let k = random_keys(&mut dice, &mut held);
        let going = d.call_keys(k);
        let what = format!("call {n} scene {:?}", d.app.scene);
        if !going {
            leaves += 1;
            d = Driver::new();
            continue;
        }
        check_world(&d.app.world, &what);
        if n % 7 == 0 {
            check_palette(&d.frame, &what);
        }
        if d.app.scene == Scene::Play {
            let s = d.app.render.stats;
            assert_eq!(s.dropped, 0, "{what}");
            worst_tris = worst_tris.max(s.tris);
        }
        scenes[d.app.scene as usize] += 1;
    }
    println!(
        "{calls} calls: scenes title/brief/play/result/end {scenes:?}, leaves {leaves}, worst triangles {worst_tris}"
    );
    assert!(worst_tris <= 300, "{worst_tris} triangles");
    if calls >= 5_000 {
        assert!(
            scenes.iter().all(|&s| s > 0),
            "a screen was never reached: {scenes:?}"
        );
    }
}

#[test]
fn the_scene_drawn_from_anywhere_in_any_setting() {
    let n = (length() / 4).max(500);
    let mut dice = Dice(seed(0x5CE9E));
    let mut r = Box::new(Renderer::new());
    let mut f = Box::new(Frame::new());
    let mut w = World::new(0);
    let mut worst = (0, 0);
    for i in 0..n {
        let m = dice.below(3) as usize;
        if i % 50 == 0 || usize::from(w.mission) != m {
            w.reset(m);
        }
        let end = (w.m().station_z() + 60 * M8) << 4;
        w.ship.x = (dice.below(2 * CORRIDOR_X as u64 * 16 + 1) as i32) - CORRIDOR_X * 16;
        w.ship.y = (dice.below(2 * CORRIDOR_Y as u64 * 16 + 1) as i32) - CORRIDOR_Y * 16;
        w.ship.z = dice.below(end as u64) as i32;
        w.ship.vx = dice.below(800) as i32 - 400;
        w.ship.vy = dice.below(800) as i32 - 400;
        w.ship.roll_left = dice.below(31) as u8;
        w.ship.roll_dir = if dice.below(2) == 0 { -1 } else { 1 };
        w.cam = (
            w.ship.x + dice.below(20_000) as i32 - 10_000,
            w.ship.y + dice.below(20_000) as i32 - 10_000,
        );
        w.ticks = dice.next() as u32 % 100_000;
        w.next_gate = dice.below(u64::from(w.m().gates) + 1) as u8;
        let detail = Detail::ALL[dice.below(3) as usize];
        let eye = Eye::ALL[dice.below(3) as usize];
        f.0.fill(0);
        scene::draw(&mut f, &mut r, &w, detail, eye);
        let what = format!("draw {i}: {detail:?} {eye:?} mission {m}");
        check_palette(&f, &what);
        let s = r.stats;
        assert_eq!(s.dropped, 0, "{what}");
        assert!(s.tris <= 300, "{what}: {} triangles", s.tris);
        if s.tris > worst.0 {
            worst = (s.tris, i);
        }
    }
    println!(
        "{n} random scenes: most triangles {} (scene {})",
        worst.0, worst.1
    );
}
