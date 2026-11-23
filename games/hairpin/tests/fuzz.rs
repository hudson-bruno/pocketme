//! Random and extreme input: every button, stick, trigger and touch
//! value, and odd gaps in time, on every course, detail, view and rate.
//! The game must never panic (the tests run with overflow checks, as
//! the chip build does), and its picture must stay inside the palette it
//! set (no palette number of 240 or more, which belong to the console).
//!
//! Also: the view drawn from every segment of every course, with the
//! player's kart in the middle and at both far edges of where a kart can
//! go, the rivals scattered round it, in every detail and view.
//!
//! `HAIRPIN_FUZZ=<calls>` runs longer (default 12 000 calls).

mod common;

use common::{Dice, Driver};
use game_kit::{Frame, Touch, button};
use hairpin::app::{Keys, Scene};
use hairpin::art;
use hairpin::race::{KARTS, Race, SUB, X_LIMIT};
use hairpin::render::{self, Cam, Cost, Detail};
use hairpin::track::{COURSES, SEG};

fn one_in(d: &mut Dice, n: u64) -> bool {
    d.below(n) == 0
}

fn any_i16(d: &mut Dice) -> i16 {
    match d.below(6) {
        0 => i16::MIN,
        1 => i16::MAX,
        2 => 0,
        3 => d.below(400) as i16 - 100,
        _ => d.next() as i16,
    }
}

fn any_u16(d: &mut Dice) -> u16 {
    match d.below(4) {
        0 => 0,
        1 => u16::MAX,
        2 => 32_767,
        _ => d.next() as u16,
    }
}

fn random_keys(d: &mut Dice) -> Keys {
    // Mostly the driving buttons, so the kart gets round the course.
    let held = if one_in(d, 4) {
        d.next() as u32
    } else {
        let drive = button::A | button::B | button::X | button::LEFT | button::RIGHT;
        (d.next() as u32) & drive
    };
    Keys {
        held,
        pressed: held & d.next() as u32,
        left: (any_i16(d), any_i16(d)),
        right: (any_i16(d), any_i16(d)),
        triggers: (any_u16(d), any_u16(d)),
        touch: one_in(d, 3).then(|| Touch {
            x: any_i16(d),
            y: any_i16(d),
            down: one_in(d, 2),
            started: one_in(d, 2),
            ended: one_in(d, 4),
        }),
    }
}

fn check(f: &Frame, what: &str) {
    let max = art::COLOURS.len() as u8;
    assert!(max <= 240);
    if let Some((i, p)) = f.0.iter().enumerate().find(|(_, p)| **p >= max) {
        panic!("{what}: pixel {i} has palette number {p}");
    }
}

#[test]
fn random_input_never_panics_and_keeps_to_the_palette() {
    let calls: u64 = std::env::var("HAIRPIN_FUZZ")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12_000);
    let mut dice = Dice(0x4A12_B1E5_0000_0001);
    let mut scenes = [0u64; 4];
    let mut done = 0u64;
    'runs: while done < calls {
        let mut d = Driver::new();
        d.app.opts.course = dice.below(COURSES.len() as u64) as u8;
        d.app.opts.detail = Detail::ALL[dice.below(3) as usize];
        d.app.opts.cam = Cam::ALL[dice.below(2) as usize];
        d.app.opts.fast = one_in(&mut dice, 2);
        d.play_from_title();
        // Some runs jump to the finish card or the cup's end, which
        // random play alone seldom reaches.
        d.app.scene = match dice.below(8) {
            0 => Scene::Finish,
            1 => Scene::Cup,
            _ => Scene::Race,
        };
        for _ in 0..3000 {
            // Odd time steps: none, one microsecond, a long stall, the
            // two rates.
            d.call_us = match dice.below(8) {
                0 => 0,
                1 => 1,
                2 => 1_000_000,
                3 => 5_000,
                4 => 16_667,
                _ => 33_333,
            };
            let k = random_keys(&mut dice);
            done += 1;
            if !d.call_keys(k) {
                // LEAVE chosen on the title: a new run.
                continue 'runs;
            }
            scenes[d.app.scene as usize] += 1;
            check(&d.frame, &format!("call {done}"));
            // Now and then back into a race, so the run is not stuck on
            // the title or a card.
            if d.app.scene != Scene::Race && d.app.pause.is_none() && one_in(&mut dice, 20) {
                d.app.scene = Scene::Title;
                d.app.menu = 0;
                d.call_keys(Keys {
                    pressed: button::A,
                    held: button::A,
                    ..Keys::default()
                });
            }
            // Sometimes far into a race at once, so laps, the finish and
            // the rivals' long run are reached too.
            if d.app.scene == Scene::Race && one_in(&mut dice, 400) {
                let lap = d.app.race.lap_prog();
                d.app.race.karts[0].prog += lap - SEG * SUB * 3;
            }
            if done >= calls {
                break 'runs;
            }
        }
    }
    eprintln!("calls {done}; calls per scene (title, race, finish, cup): {scenes:?}");
    assert!(scenes[Scene::Race as usize] > 0);
}

#[test]
fn the_view_draws_from_every_segment_of_every_course() {
    let mut frame = Box::new(Frame::new());
    let mut race = Box::new(Race::empty());
    let mut dice = Dice(0x5EED_0F0A_D500_0001);
    let mut cost = Cost::default();
    let mut views = 0u64;
    let mut worst = Cost::default();
    for (c, course) in COURSES.iter().enumerate() {
        race.reset(c as u8);
        for seg in 0..course.segs {
            for (n, x) in [0, -X_LIMIT, X_LIMIT].into_iter().enumerate() {
                let along = seg as i32 * SEG * SUB + (dice.below((SEG * SUB) as u64) as i32);
                race.karts[0].prog = along;
                race.karts[0].x = x;
                for k in 1..KARTS {
                    // Rivals near and far, ahead and behind, anywhere across.
                    let dz = dice.below(40 * (SEG * SUB) as u64) as i32 - 8 * SEG * SUB;
                    race.karts[k].prog = along + dz;
                    race.karts[k].x = dice.below(2 * X_LIMIT as u64) as i32 - X_LIMIT;
                    race.karts[k].braking = one_in(&mut dice, 2);
                }
                race.lean = (n as i8) - 1;
                race.boosting = one_in(&mut dice, 3);
                race.shake = (dice.below(3) * 8) as u8;
                race.bg = dice.below(256 * 256) as i32;
                race.tick = dice.below(100_000) as u32;
                for detail in Detail::ALL {
                    for cam in Cam::ALL {
                        render::draw(&mut frame, &race, detail, cam, &mut cost);
                        views += 1;
                        if cost.sprite_px > worst.sprite_px {
                            worst = cost;
                        }
                        let max = art::COLOURS.len() as u8;
                        if let Some(p) = frame.0.iter().find(|p| **p >= max) {
                            panic!(
                                "course {c} segment {seg} x {x} {detail:?} {cam:?}: palette {p}"
                            );
                        }
                    }
                }
            }
        }
    }
    eprintln!("views drawn {views}; the most sprite pixels in one view: {worst:?}");
}
