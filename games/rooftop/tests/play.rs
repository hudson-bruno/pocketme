//! The whole game played by script through its own screens: a full run
//! from the title to the end screen, practice from a later room,
//! restart, quit, leave, the title menu by buttons and by touch, every
//! control, the same run at 60 frames a second, and the same pictures
//! every time. Pictures go to .local/games/platformer/frames/.

mod common;

use common::*;
use game_kit::{Touch, button, rate};
use rooftop::rooms::ROOMS;
use rooftop::sim::{Follow, Runner};
use rooftop::{Item, Keys, Note, ROOM_COUNT, Rooftop, Scene};

/// All the rooms' solution scripts, one after the other, from `from`.
fn full_script(from: usize) -> Vec<Hold> {
    (from..ROOM_COUNT).flat_map(solution).collect()
}

/// Game ticks each room's script takes (two per call, ending in the
/// call that reaches the door).
fn room_ticks(room: usize) -> u32 {
    solution(room).iter().map(|h| h.calls).sum::<u32>() * 2
}

fn press(b: u32) -> Keys {
    keys(b, b)
}

fn hash(f: &game_kit::Frame) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in f.0.iter() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

#[test]
fn a_full_run_from_the_title_to_the_end_screen() {
    let mut d = Driver::new();
    assert_eq!(d.game.scene, Scene::Title);
    save_frame(&evidence("frames").join("title.png"), &d.frame, 2);
    d.call(press(button::A));
    assert_eq!(d.game.scene, Scene::Play);
    d.play(&full_script(0));
    assert_eq!(d.game.scene, Scene::End);

    let notes = d.notes();
    assert_eq!(notes[0], Note::Run { from: 1 });
    let mut total = 0;
    for room in 0..ROOM_COUNT {
        let t = room_ticks(room);
        total += t;
        assert!(
            notes.contains(&Note::Cleared {
                room: room as u8 + 1,
                ticks: t,
                retries: 0
            }),
            "room {}",
            room + 1
        );
    }
    assert_eq!(
        notes.last(),
        Some(&Note::Delivered {
            ticks: total,
            retries: 0
        })
    );
    assert!(
        !notes
            .iter()
            .any(|n| matches!(n, Note::Cut { .. } | Note::Fell { .. }))
    );
    assert_eq!(d.game.best, Some(total));
    println!("full run: {total} ticks = {} s", total / 60);
    save_frame(&evidence("frames").join("end.png"), &d.frame, 2);

    // A: back to the title, which now shows the best time.
    d.tap(button::A);
    assert_eq!(d.game.scene, Scene::Title);
    save_frame(&evidence("frames").join("title-best.png"), &d.frame, 2);
    // A second, slower run does not beat it.
    d.call(press(button::A));
    d.play(&parse("10 -"));
    d.play(&full_script(0));
    let n = d.notes();
    assert_eq!(
        d.game.scene,
        Scene::End,
        "{:?}",
        &n[n.len().saturating_sub(12)..]
    );
    assert_eq!(d.game.best, Some(total));
    // B on the end screen leaves the game.
    assert!(d.tap(button::B).done);
}

#[test]
fn practice_from_a_later_room_has_no_best_time() {
    let mut d = Driver::new();
    // ROOM is the second line: down, then right twice.
    d.tap(button::DOWN);
    d.tap(button::RIGHT);
    d.tap(button::RIGHT);
    assert_eq!(d.game.from, 2);
    d.tap(button::UP);
    d.call(press(button::START));
    assert_eq!(d.game.run.room, 2);
    d.play(&full_script(2));
    assert_eq!(d.game.scene, Scene::End);
    assert_eq!(d.game.best, None);
    assert!(!d.game.run.scores[0].done && !d.game.run.scores[1].done);
    assert!(d.game.run.scores[2..].iter().all(|s| s.done));
    save_frame(&evidence("frames").join("end-practice.png"), &d.frame, 2);
    // A tap on the left half: the title; on the right half: leave.
    d.call(Keys {
        touch: Some(tap(30, 60)),
        delta_us: CALL_US,
        ..Keys::default()
    });
    assert_eq!(d.game.scene, Scene::Title);
}

fn tap(x: i16, y: i16) -> Touch {
    Touch {
        x,
        y,
        down: true,
        started: true,
        ended: false,
    }
}

#[test]
fn start_restarts_the_room_and_select_goes_back_to_the_title() {
    let mut d = Driver::new();
    d.call(press(button::A));
    d.play(&parse("10 R"));
    let start = Runner::at_start(&ROOMS[0]);
    assert_ne!(d.game.run.world.runner, start);
    d.call(press(button::START));
    // The restart happens before this call's ticks; standing still
    // keeps the runner at the start.
    assert_eq!(d.game.run.world.runner.x, start.x);
    assert_eq!(d.game.run.retries, 1);
    assert!(d.notes().contains(&Note::Restart { room: 1 }));
    save_frame(&evidence("frames").join("restart.png"), &d.frame, 2);

    d.call(press(button::SELECT));
    assert_eq!(d.game.scene, Scene::Title);
    assert!(d.notes().contains(&Note::Quit));
    // A new run starts clean.
    d.call(press(button::A));
    assert_eq!(d.game.run.retries, 0);
    assert_eq!(d.game.run.ticks, 0);
}

#[test]
fn leaving_from_the_title() {
    // B.
    let mut d = Driver::new();
    assert!(d.tap(button::B).done);
    // LEAVE, by the d-pad (up wraps to the last line).
    let mut d = Driver::new();
    d.tap(button::UP);
    assert_eq!(rooftop::ITEMS[d.game.sel], Item::Leave);
    assert!(d.tap(button::A).done);
    // LEAVE, by touch.
    let mut d = Driver::new();
    let y = rooftop::MENU_TOP + 4 * rooftop::MENU_LINE + 2;
    let out = d.call(Keys {
        touch: Some(tap(60, y)),
        delta_us: CALL_US,
        ..Keys::default()
    });
    assert!(out.done);
}

#[test]
fn the_title_menu_by_buttons_and_by_touch() {
    let mut d = Driver::new();
    assert_eq!(
        d.game.follow,
        Follow::Step,
        "the calm camera is the default"
    );
    assert!(d.game.fast, "60 frames a second is the default");
    assert_eq!(d.game.rate(), rate::FPS60);
    // ROOM: left from room 1 wraps to the last.
    d.tap(button::DOWN);
    d.tap(button::LEFT);
    assert_eq!(d.game.from, ROOM_COUNT - 1);
    d.tap(button::RIGHT);
    assert_eq!(d.game.from, 0);
    // SCROLL.
    d.tap(button::DOWN);
    d.tap(button::RIGHT);
    assert_eq!(d.game.follow, Follow::Smooth);
    save_frame(&evidence("frames").join("title-smooth.png"), &d.frame, 2);
    d.tap(button::A);
    assert_eq!(d.game.follow, Follow::Step);
    // RATE asks the console for 30, then 60.
    d.tap(button::DOWN);
    assert_eq!(d.tap(button::RIGHT).rate, Some(rate::FPS30));
    assert!(!d.game.fast);
    assert_eq!(d.tap(button::LEFT).rate, Some(rate::FPS60));
    assert!(d.game.fast);

    // Touch: the ROOM line's right half steps forward, left half back.
    let y = rooftop::MENU_TOP + rooftop::MENU_LINE + 2;
    d.call(Keys {
        touch: Some(tap(110, y)),
        delta_us: CALL_US,
        ..Keys::default()
    });
    assert_eq!(d.game.from, 1);
    d.call(Keys {
        touch: Some(tap(40, y)),
        delta_us: CALL_US,
        ..Keys::default()
    });
    assert_eq!(d.game.from, 0);
    // A finger held down does not repeat.
    d.call(Keys {
        touch: Some(Touch {
            started: false,
            ..tap(110, y)
        }),
        delta_us: CALL_US,
        ..Keys::default()
    });
    assert_eq!(d.game.from, 0);
    // Touch outside the menu does nothing.
    d.call(Keys {
        touch: Some(tap(5, 5)),
        delta_us: CALL_US,
        ..Keys::default()
    });
    assert_eq!(d.game.scene, Scene::Title);
    // START by touch.
    d.call(Keys {
        touch: Some(tap(60, rooftop::MENU_TOP + 2)),
        delta_us: CALL_US,
        ..Keys::default()
    });
    assert_eq!(d.game.scene, Scene::Play);
}

/// A game already in room 1, standing.
fn playing() -> Driver {
    let mut d = Driver::new();
    d.call(press(button::A));
    d.play(&parse("4 -"));
    d
}

#[test]
fn every_jump_control_jumps() {
    for b in [button::A, button::B, button::L] {
        let mut d = playing();
        d.call(press(b));
        assert!(d.game.run.world.runner.vy < 0, "button {b:#x}");
    }
    // ZL, past half way, on the way down only.
    let mut d = playing();
    d.call(Keys {
        triggers: (32_767, 0),
        delta_us: CALL_US,
        ..Keys::default()
    });
    assert!(d.game.run.world.runner.vy < 0);
    let mut d = playing();
    d.call(Keys {
        triggers: (10_000, 0),
        delta_us: CALL_US,
        ..Keys::default()
    });
    assert!(
        d.game.run.world.runner.vy >= 0,
        "a light touch of ZL is not a press"
    );
}

#[test]
fn every_dash_control_dashes() {
    for b in [button::X, button::Y, button::R] {
        let mut d = playing();
        d.call(press(b));
        assert_eq!(d.game.run.world.runner.dashes, 0, "button {b:#x}");
    }
    let mut d = playing();
    d.call(Keys {
        triggers: (0, 32_767),
        delta_us: CALL_US,
        ..Keys::default()
    });
    assert_eq!(d.game.run.world.runner.dashes, 0);
    // Holding ZR does not dash again after landing.
    d.play(&parse("40 -"));
    let mut held = d.game.run.world.runner;
    for _ in 0..20 {
        d.call(Keys {
            triggers: (0, 32_767),
            delta_us: CALL_US,
            ..Keys::default()
        });
        held = d.game.run.world.runner;
    }
    assert_eq!(held.dashes, 1);
}

#[test]
fn the_stick_runs_at_its_own_speed_and_aims_the_dash() {
    // Run right on the d-pad first (away from the room's left side),
    // then hold the stick for 8 calls.
    let speed = |x: i16| {
        let mut d = playing();
        d.play(&parse("10 R"));
        for _ in 0..8 {
            d.call(Keys {
                stick: (x, 0),
                delta_us: CALL_US,
                ..Keys::default()
            });
        }
        d.game.run.world.runner.vx
    };
    let full = speed(32_767);
    let half = speed(16_000);
    let nudge = speed(4_000);
    let left = speed(-32_768);
    println!("stick speeds: full {full}, half {half}, nudge {nudge}, left {left}");
    assert_eq!(full, rooftop::sim::RUN_MAX);
    assert!(half > 0 && half < full, "half way is slower");
    assert_eq!(nudge, 0, "the dead zone");
    assert_eq!(left, -full);

    // Stick up + dash goes straight up, without a jump.
    let mut d = playing();
    let y0 = d.game.run.world.runner.y;
    d.call(Keys {
        held: button::X,
        pressed: button::X,
        stick: (0, -32_767),
        delta_us: CALL_US,
        ..Keys::default()
    });
    for _ in 0..8 {
        d.call(Keys {
            stick: (0, -32_767),
            delta_us: CALL_US,
            ..Keys::default()
        });
    }
    let r = d.game.run.world.runner;
    assert!(y0 - r.y >= 20, "rose {}", y0 - r.y);
    assert_eq!(r.x, Runner::at_start(&ROOMS[0]).x);
    // The d-pad aims the same way.
    let mut d = playing();
    d.call(press(button::X | button::UP));
    d.play(&parse("8 U"));
    assert!(y0 - d.game.run.world.runner.y >= 20);
    // And left/right on the d-pad run.
    let mut d = playing();
    d.play(&parse("10 R"));
    assert!(d.game.run.world.runner.vx > 0);
    d.play(&parse("20 L"));
    assert!(d.game.run.world.runner.vx < 0);
}

#[test]
fn sixty_frames_a_second_plays_the_same() {
    let script = full_script(0);
    // At 60 calls a second each call is one tick, so each line of the
    // script lasts twice as many calls; a press is in its first call
    // only.
    let doubled: Vec<Hold> = script
        .iter()
        .map(|h| Hold {
            calls: h.calls * 2,
            held: h.held,
        })
        .collect();
    let mut d = Driver::new();
    d.delta_us = 16_667;
    // 60 a second is the RATE setting at the start.
    assert!(d.game.fast);
    assert_eq!(d.game.rate(), rate::FPS60);
    d.call(Keys {
        held: button::A,
        pressed: button::A,
        delta_us: 16_667,
        ..Keys::default()
    });
    d.play(&doubled);
    assert_eq!(d.game.scene, Scene::End);
    let total: u32 = (0..ROOM_COUNT).map(room_ticks).sum();
    assert!(d.notes().contains(&Note::Delivered {
        ticks: total,
        retries: 0
    }));
}

#[test]
fn the_same_input_gives_the_same_pictures() {
    let run = || {
        let mut d = Driver::new();
        let mut hashes = vec![hash(&d.frame)];
        d.call(press(button::A));
        for (held, pressed) in calls(&full_script(0)) {
            d.call(keys(held, pressed));
            hashes.push(hash(&d.frame));
        }
        hashes
    };
    let a = run();
    let b = run();
    assert_eq!(a, b);
    // And the pictures do change.
    assert!(a.windows(2).filter(|w| w[0] != w[1]).count() > 100);
}

#[test]
fn time_is_only_what_the_console_gives() {
    // A call with no time passed moves nothing (the first call, or one
    // just after the home menu).
    let mut d = playing();
    let before = d.game.run.world.runner;
    let t = d.game.run.ticks;
    d.call(Keys {
        held: button::RIGHT,
        pressed: button::RIGHT,
        delta_us: 0,
        ..Keys::default()
    });
    assert_eq!(d.game.run.ticks, t);
    assert_eq!(d.game.run.world.runner, before);
    // A huge step (a stall) runs at most six ticks, never a jump through
    // the floor.
    d.call(Keys {
        held: button::RIGHT,
        delta_us: u32::MAX,
        ..Keys::default()
    });
    assert_eq!(d.game.run.ticks, t + 6);
}

#[test]
fn random_play_never_panics() {
    // Random keys, sticks, triggers, touches and times, through every
    // screen. The test build has overflow checks on, like the chip.
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut d = Driver::new();
    let mut leaves = 0;
    for i in 0..20_000 {
        let r = next();
        let held = (r as u32)
            & 0xFFF
            & !if i % 500 == 0 {
                0
            } else {
                button::B | button::SELECT
            };
        let k = Keys {
            held,
            pressed: held & (next() as u32),
            stick: ((next() >> 16) as i16, (next() >> 16) as i16),
            triggers: ((next() as u16) >> 1, (next() as u16) >> 1),
            touch: if r >> 40 & 15 == 0 {
                Some(Touch {
                    x: (next() % 200) as i16 - 20,
                    y: (next() % 160) as i16 - 20,
                    down: true,
                    started: next() & 1 == 0,
                    ended: false,
                })
            } else {
                None
            },
            delta_us: match r >> 48 & 7 {
                0 => 0,
                1 => u32::MAX,
                _ => (next() % 70_000) as u32,
            },
        };
        if d.call(k).done {
            leaves += 1;
            d = Driver::new();
        }
    }
    println!("left the game {leaves} times");
    // And a run that has been going for ever: the clock stops at its top.
    let mut d = playing();
    d.game.run.ticks = u32::MAX - 2;
    d.game.run.scores[0].ticks = u32::MAX - 2;
    d.play(&parse("10 R"));
    assert_eq!(d.game.run.ticks, u32::MAX);
}

#[test]
fn a_new_game_value_is_the_title() {
    let g = Rooftop::default();
    assert_eq!(g.scene, Scene::Title);
    assert_eq!(g.best, None);
}
