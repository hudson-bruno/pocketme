//! Random and extreme input: every button, stick, trigger, touch and
//! tilt value, and odd gaps in time. The game must never panic (the
//! tests run with overflow checks, as the chip build does), and its
//! picture must stay inside the palette it set.
//!
//! `LANDER_FUZZ=<calls>` runs longer (default 20 000 calls).

mod common;

use common::Run;
use game_kit::Touch;
use lander::app::{App, Input, Scene, Steer, View};
use lander::draw::COLOURS;

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

fn random_input(d: &mut Dice) -> Input {
    let held = d.next() as u32;
    Input {
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
        tilt: d.one_in(2).then(|| d.i16()),
    }
}

#[test]
fn random_input_never_breaks_the_game() {
    let calls: u64 = std::env::var("LANDER_FUZZ")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_000);
    let mut d = Dice(0x5EED_1A4D_E500_0001);
    let mut r = Run::new();
    let mut scenes = [0u64; 6];
    let mut restarts = 0;
    for _ in 0..calls {
        if r.done {
            r = Run::new();
            restarts += 1;
        }
        // Mostly calm input (so flights happen), sometimes anything.
        let inp = if d.one_in(4) {
            random_input(&mut d)
        } else {
            Input::default()
        };
        // The clock: now and then a big jump, or the same time again.
        match d.next() % 50 {
            0 => r.t = r.t.saturating_add(d.next() % 100_000_000),
            1 => r.t = r.t.saturating_sub(33_333),
            _ => {}
        }
        if d.one_in(500) {
            // Options the menus can reach, set straight away.
            r.app.opts.view = if d.one_in(2) {
                View::Still
            } else {
                View::Follow
            };
            r.app.opts.steer = if d.one_in(2) {
                Steer::Stick
            } else {
                Steer::Tilt
            };
            r.app.opts.fast = d.one_in(2);
        }
        r.call(&inp);
        if r.done {
            continue;
        }
        scenes[r.app.scene as usize] += 1;
        let top = COLOURS.len() as u8;
        assert!(
            r.frame.0.iter().all(|&p| p <= top),
            "a pixel outside the palette"
        );
    }
    eprintln!("calls {calls}, restarts after END {restarts}, calls per scene {scenes:?}");
    // The dice reached the flights and their endings.
    assert!(scenes[Scene::Fly as usize] > 0);
    assert!(scenes[Scene::Broken as usize] + scenes[Scene::Landed as usize] > 0);
}

#[test]
fn extreme_times_and_levels_never_break_the_game() {
    // The far end of the clock.
    let mut a = App::new();
    let mut f = console_video::Frame::new();
    for t in [0, u64::MAX / 2, u64::MAX - 1, u64::MAX, 5, u64::MAX] {
        a.update(&Input::default(), t);
        a.draw(&mut f);
    }
    // Very high levels: fly and fall on each.
    for level in [20, 99, 1000, u16::MAX - 1, u16::MAX] {
        let mut r = Run::new();
        r.app.opts.level = level;
        r.play();
        for _ in 0..900 {
            if r.app.scene != Scene::Fly {
                break;
            }
            r.call(&Input::default());
        }
        assert_eq!(r.app.scene, Scene::Broken, "level {level}");
        // Landing on the last level does not wrap the number round.
        r.app.level = u16::MAX;
        r.app.scene = Scene::Landed;
        r.app.scene_t = 1000;
        r.call(&common::press(game_kit::button::A));
        assert_eq!(r.app.level, u16::MAX);
    }
}
