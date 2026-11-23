//! Random and extreme input: every button, stick, trigger and touch
//! value, and odd gaps in time, on every level and detail. The game must
//! never panic (the tests run with overflow checks, as the chip build
//! does), and its picture must stay inside the palette it set (no
//! palette number of 240 or more, which belong to the console).
//!
//! Also: the view drawn from every free cell of every level (its middle
//! and a corner as near as the player gets), looking 16 ways, in every
//! detail.
//!
//! `LANTERN_FUZZ=<calls>` runs longer (default 12 000 calls).

mod common;

use common::Driver;
use game_kit::Touch;
use lantern::app::{Keys, Scene};
use lantern::art;
use lantern::fixed::{ONE, centre};
use lantern::level::LEVELS;
use lantern::render::{Detail, Renderer};
use lantern::world::World;

struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn one_in(&mut self, n: u64) -> bool {
        self.next().is_multiple_of(n)
    }
    fn i16(&mut self) -> i16 {
        match self.next() % 6 {
            0 => i16::MIN,
            1 => i16::MAX,
            2 => 0,
            3 => (self.next() % 400) as i16 - 100,
            _ => self.next() as i16,
        }
    }
    fn u16(&mut self) -> u16 {
        match self.next() % 4 {
            0 => 0,
            1 => u16::MAX,
            2 => 32767,
            _ => self.next() as u16,
        }
    }
}

fn random_keys(d: &mut Dice) -> Keys {
    // Mostly the movement buttons, so the player gets around.
    let held = if d.one_in(4) {
        d.next() as u32
    } else {
        (d.next() as u32) & 0x0000_00F0 | (d.next() as u32 & 0x0F0F)
    };
    Keys {
        held,
        pressed: held & d.next() as u32,
        left: (d.i16(), d.i16()),
        right: (d.i16(), d.i16()),
        triggers: (d.u16(), d.u16()),
        touch: d.one_in(3).then(|| Touch {
            x: d.i16(),
            y: d.i16(),
            down: d.one_in(2),
            started: d.one_in(2),
            ended: d.one_in(4),
        }),
    }
}

fn check(d: &Driver, what: &str) {
    let max = art::COLOURS.len() as u8;
    assert!(max <= 240);
    if let Some((i, p)) = d.frame.0.iter().enumerate().find(|(_, p)| **p >= max) {
        panic!("{what}: pixel {i} has palette number {p}");
    }
}

#[test]
fn random_input_never_panics_and_keeps_to_the_palette() {
    let calls: u64 = std::env::var("LANTERN_FUZZ")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12_000);
    let mut dice = Dice(0x1A57_E2A5_0000_0001);
    let mut scenes = [0u32; 6];
    let mut done = 0;
    'runs: while done < calls {
        let mut d = Driver::new();
        d.app.opts.level = (dice.next() % LEVELS.len() as u64) as u8;
        d.app.opts.detail = Detail::ALL[(dice.next() % 3) as usize];
        d.play_from_title();
        // Some runs start on the cards and the last screen, which play
        // alone seldom reaches.
        d.app.scene = match dice.next() % 6 {
            0 => Scene::Clear,
            1 => Scene::Fainted,
            2 => Scene::End,
            _ => Scene::Play,
        };
        for _ in 0..1500 {
            // Odd time steps: none, one, many ticks, a long stall.
            d.call_us = match dice.next() % 8 {
                0 => 0,
                1 => 1,
                2 => 1_000_000,
                3 => 5_000,
                _ => 33_333,
            };
            let k = random_keys(&mut dice);
            done += 1;
            if !d.call_keys(k) {
                // LEAVE chosen on the title: a new run.
                continue 'runs;
            }
            scenes[d.app.scene as usize] += 1;
            check(&d, &format!("call {done}"));
            // Now and then put the game back in play, so it is not stuck
            // on the title for most of the run.
            if d.app.scene == Scene::Title && dice.one_in(20) {
                d.app.menu = 0;
                d.call_keys(Keys {
                    pressed: game_kit::button::A,
                    held: game_kit::button::A,
                    ..Keys::default()
                });
            }
            if done >= calls {
                break 'runs;
            }
        }
    }
    eprintln!("calls {done}; calls per scene (title, play, map, clear, fainted, end): {scenes:?}");
    assert!(scenes[Scene::Play as usize] > 0);
}

#[test]
fn the_view_draws_from_every_free_cell_looking_every_way() {
    let mut frame = Box::new(game_kit::Frame::new());
    let mut r = Renderer::new();
    let max = art::COLOURS.len() as u8;
    let mut views = 0u32;
    let mut worst_steps = 0;
    for (n, level) in LEVELS.iter().enumerate() {
        let mut w = World::new(n);
        for cy in 0..level.h() as i32 {
            for cx in 0..level.w() as i32 {
                if w.solid(cx, cy) {
                    continue;
                }
                // The centre, and near a corner (the player's radius).
                let corner = if (cx + cy) % 2 == 0 {
                    (cx * ONE + 1100, cy * ONE + 1100)
                } else {
                    ((cx + 1) * ONE - 1101, (cy + 1) * ONE - 1101)
                };
                for (x, y) in [(centre(cx), centre(cy)), corner] {
                    w.x = x;
                    w.y = y;
                    for a in 0..16u32 {
                        w.angle = (a * 4096 + 7 + cx as u32 * 97) as u16;
                        let detail = Detail::ALL[(a % 3) as usize];
                        r.draw(&mut frame, &w, detail);
                        views += 1;
                        worst_steps = worst_steps.max(r.cost.steps);
                        if let Some(p) = frame.0.iter().find(|p| **p >= max) {
                            panic!("level {n} cell ({cx},{cy}) angle {a}: palette number {p}");
                        }
                    }
                }
            }
        }
    }
    eprintln!("views drawn {views}, worst ray steps in one view {worst_steps}");
}
