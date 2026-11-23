//! Whole matches through the game's own input, call by call: every mode
//! reaches a final score, the menus do what they say, touch drives its
//! mallet and keeps it on its half, the game sleeps when nothing moves,
//! 30 and 60 calls a second play the same, and the same input gives the
//! same game.

mod common;

use common::{CALL_US, Driver};
use game_kit::{Touch, button, rate};
use skimmer::Skimmer;
use skimmer::app::{
    App, Ctl, END_Y, Keys, LINE, Log, PAUSE_Y, Scene, TICK_US, TITLE_Y, Who, line_at,
};
use skimmer::fixed::{PX, len};
use skimmer::table::{CX, HIT_R, Length, Level, MALLET_R, Match, Phase, TOUCH_V, YB, YT, zone_x};

/// A final: the match is over and the score fits its length.
fn assert_final(d: &Driver) {
    let m = &d.app.m;
    assert_eq!(m.phase, Phase::Over);
    assert_eq!(d.app.scene, Scene::End);
    match m.rules.length.goals() {
        Some(g) => {
            assert_eq!(m.score.iter().copied().max(), Some(g), "{:?}", m.score);
            assert!(m.score.iter().copied().min() < Some(g));
        }
        None => assert!(m.clock == 0 || m.overtime),
    }
    assert!(d.has_log(|l| matches!(l, Log::Final { .. })));
}

#[test]
fn one_player_against_each_computer_level_reaches_a_final() {
    for level in Level::ALL {
        let mut d = Driver::new([true, false, false, false]);
        d.app.opts.level = level;
        d.start();
        assert_eq!(d.app.ctl[..2], [Ctl::Pad(0), Ctl::Cpu]);
        let calls = d.play_to_end(30 * 60 * 20);
        assert_final(&d);
        println!(
            "pad against {}: {}-{} in {} s",
            level.name(),
            d.app.m.score[0],
            d.app.m.score[1],
            calls / 30
        );
    }
}

#[test]
fn pad_against_pad_reaches_a_final() {
    let mut d = Driver::new([true, true, false, false]);
    d.start();
    assert_eq!(d.app.ctl[..2], [Ctl::Pad(0), Ctl::Pad(1)]);
    d.play_to_end(30 * 60 * 20);
    assert_final(&d);
}

#[test]
fn pad_against_touch_reaches_a_final_when_a_finger_takes_the_right_side() {
    let mut d = Driver::new([true, false, false, false]);
    d.start();
    // RIGHT AUTO with no pad 2: the computer, until a finger comes down
    // on the right half.
    assert_eq!(d.app.ctl[1], Ctl::Cpu);
    let t = d.touch_to(Some((140, 66)));
    assert!(d.call_with([0; 4], t));
    assert!(d.app.touch_claimed);
    assert_eq!(d.app.ctl[1], Ctl::Touch);
    assert!(d.has_log(|l| *l == Log::TouchPlays));
    d.play_to_end(30 * 60 * 20);
    assert_final(&d);
    // Back on the title the claim is gone.
    assert!(d.tap(80, END_Y + LINE + 2));
    assert_eq!(d.app.scene, Scene::Title);
    assert!(!d.app.touch_claimed);
}

#[test]
fn touch_against_the_computer_with_no_pads_reaches_a_final() {
    let mut d = Driver::new([false; 4]);
    d.start();
    assert_eq!(d.app.ctl[..2], [Ctl::Touch, Ctl::Cpu]);
    d.play_to_end(30 * 60 * 20);
    assert_final(&d);
}

#[test]
fn touch_set_by_hand_on_the_right_plays_pad_1_on_the_left() {
    let mut d = Driver::new([true, false, false, false]);
    d.app.opts.side[1] = Who::Touch;
    d.start();
    assert_eq!(d.app.ctl[..2], [Ctl::Pad(0), Ctl::Touch]);
    d.play_to_end(30 * 60 * 20);
    assert_final(&d);
}

#[test]
fn two_against_two_with_four_pads_reaches_a_final() {
    let mut d = Driver::new([true; 4]);
    d.app.opts.four = true;
    d.start();
    assert_eq!(
        d.app.ctl,
        [Ctl::Pad(0), Ctl::Pad(1), Ctl::Pad(2), Ctl::Pad(3)]
    );
    d.play_to_end(30 * 60 * 20);
    assert_final(&d);
}

#[test]
fn two_against_two_with_two_pads_fills_in_computers() {
    let mut d = Driver::new([true, true, false, false]);
    d.app.opts.four = true;
    d.start();
    assert_eq!(d.app.ctl, [Ctl::Pad(0), Ctl::Pad(1), Ctl::Cpu, Ctl::Cpu]);
    d.play_to_end(30 * 60 * 20);
    assert_final(&d);
}

#[test]
fn computer_against_computer_on_the_wild_table_reaches_a_final() {
    let mut d = Driver::new([false; 4]);
    d.app.opts.side = [Who::Computer, Who::Computer];
    d.app.opts.wild = true;
    d.app.opts.length = Length::First11;
    d.start();
    assert_eq!(d.app.ctl[..2], [Ctl::Cpu, Ctl::Cpu]);
    d.play_to_end(30 * 60 * 30);
    assert_final(&d);
}

#[test]
fn a_timed_match_reaches_a_final_and_says_who_won() {
    let mut d = Driver::new([true, false, false, false]);
    d.app.opts.length = Length::Timed;
    d.start();
    let calls = d.play_to_end(30 * 60 * 5);
    assert_final(&d);
    // Three minutes, and at most one more of golden goal.
    assert!((30 * 180 - 2..=30 * 240 + 2).contains(&calls), "{calls}");
}

#[test]
fn first_to_5_and_11_end_there() {
    for (length, g) in [(Length::First5, 5), (Length::First11, 11)] {
        let mut d = Driver::new([true, true, false, false]);
        d.app.opts.length = length;
        d.start();
        d.play_to_end(30 * 60 * 30);
        assert_eq!(d.app.m.score.iter().copied().max(), Some(g));
    }
}

#[test]
fn after_a_goal_the_side_that_let_it_in_serves() {
    let mut d = Driver::new([true, true, false, false]);
    d.start();
    let mut goals = 0;
    for _ in 0..30 * 60 * 10 {
        let before = d.app.m.phase;
        assert!(d.play_call());
        if let (Phase::Goal { side, .. }, Phase::Serve { side: s, .. }) = (before, d.app.m.phase) {
            assert_eq!(s, 1 - side, "the side that conceded serves");
            let p = d.app.m.puck;
            assert_eq!(p.x < CX, s == 0, "the puck waits on the server's half");
            goals += 1;
            if goals == 4 {
                return;
            }
        }
        if d.app.scene != Scene::Play {
            break;
        }
    }
    panic!("only {goals} goals");
}

#[test]
fn the_touch_mallet_stays_on_its_half_and_under_its_top_speed() {
    // The finger goes everywhere, the far half included, and jumps; the mallet follows no faster than TOUCH_V a tick and
    // never leaves its zone.
    let mut d = Driver::new([false; 4]);
    d.app.opts.side[1] = Who::Computer;
    d.start();
    assert_eq!(d.app.ctl[0], Ctl::Touch);
    d.until_live();
    let (x0, x1) = zone_x(0, false);
    let mut seed = 0x1234_5678u32;
    let mut followed = 0;
    for k in 0..30 * 60 * 3u32 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let to = if k % 40 < 34 {
            Some(((seed % 160) as i16, (14 + (seed >> 8) % 106) as i16))
        } else {
            None
        };
        let before = d.app.m.m[0];
        let was_goal = matches!(d.app.m.phase, Phase::Goal { .. });
        let sq = d.app.m.squeezes;
        let t = d.touch_to(to);
        assert!(d.call_with([0; 4], t));
        if d.app.scene != Scene::Play {
            // A final: REMATCH.
            d.touch_at = None;
            assert!(d.tap(60, END_Y + 2));
            assert_eq!(d.app.scene, Scene::Play);
            continue;
        }
        let m = d.app.m.m[0];
        assert!((x0..=x1).contains(&m.x), "left the zone: {m:?}");
        assert!(m.x < CX, "on the far half");
        assert!((YT + MALLET_R..=YB - MALLET_R).contains(&m.y));
        // Two ticks a call (a mallet that had to give way to a squeezed
        // puck may be pushed a little more, and a mallet standing on the
        // serve spot steps off it when the puck is put down).
        let moved = len(m.x - before.x, m.y - before.y);
        let served = was_goal && matches!(d.app.m.phase, Phase::Serve { .. });
        if d.app.m.squeezes == sq && !served {
            assert!(moved <= 2 * TOUCH_V + 2, "moved {moved} in a call");
        }
        if moved > 0 {
            followed += 1;
        }
    }
    assert!(followed > 1000, "the mallet hardly moved: {followed}");
}

#[test]
fn a_finger_on_the_bar_pauses_and_does_not_steer() {
    let mut d = Driver::new([false; 4]);
    d.start();
    d.until_live();
    assert!(d.tap(80, 4));
    assert_eq!(d.app.pause, Some(0));
    assert!(Skimmer::wanted(&d.app) == rate::ON_DEMAND);
    // A tap outside the panel goes back to the match.
    assert!(d.tap(4, 115));
    assert_eq!(d.app.pause, None);
}

#[test]
fn the_pause_menu_works_with_pads_and_touch() {
    let mut d = Driver::new([true, false, false, false]);
    d.start();
    d.until_live();
    assert!(d.press(0, button::START));
    assert_eq!(d.app.pause, Some(0));
    let age = d.app.m.age;
    d.idle(30);
    assert_eq!(d.app.m.age, age, "paused: nothing moves");
    // COMPUTER: a step to the right.
    assert!(d.press(0, button::DOWN));
    assert!(d.press(0, button::RIGHT));
    assert_eq!(d.app.opts.level, Level::Hard);
    assert_eq!(d.app.m.rules.level, Level::Hard);
    // RATE by touch, on the right half.
    assert!(d.tap(110, PAUSE_Y + 2 * LINE + 2));
    assert!(!d.app.opts.fast);
    assert_eq!(Skimmer::wanted(&d.app), rate::ON_DEMAND);
    // RESTART: a new match, the old score gone.
    d.app.m.score = [3, 2];
    assert!(d.tap(80, PAUSE_Y + 3 * LINE + 2));
    assert_eq!(d.app.pause, None);
    assert_eq!(d.app.m.score, [0, 0]);
    assert_eq!(Skimmer::wanted(&d.app), rate::FPS30);
    // TITLE.
    assert!(d.press(0, button::START));
    assert!(d.tap(80, PAUSE_Y + 4 * LINE + 2));
    assert_eq!(d.app.scene, Scene::Title);
    // LEAVE from the pause menu ends the game.
    d.start();
    assert!(d.press(0, button::START));
    assert!(!d.tap(80, PAUSE_Y + 5 * LINE + 2));
    assert!(d.has_log(|l| *l == Log::Leave));
}

#[test]
fn the_title_menu_steps_every_setting_and_keeps_one_finger_one_side() {
    let mut d = Driver::new([true, false, false, false]);
    // Down to CORAL, then through its choices.
    assert!(d.press(0, button::DOWN));
    assert_eq!(d.app.sel, 1);
    let mut seen = Vec::new();
    for _ in 0..5 {
        assert!(d.press(0, button::RIGHT));
        seen.push(d.app.opts.side[0]);
    }
    assert_eq!(
        seen,
        [Who::Pad1, Who::Pad2, Who::Touch, Who::Computer, Who::Auto]
    );
    // CORAL on TOUCH: TEAL skips TOUCH.
    d.app.opts.side[0] = Who::Touch;
    assert!(d.press(0, button::DOWN));
    let mut right = Vec::new();
    for _ in 0..4 {
        assert!(d.press(0, button::RIGHT));
        right.push(d.app.opts.side[1]);
    }
    assert_eq!(right, [Who::Pad1, Who::Pad2, Who::Computer, Who::Auto]);
    // The others, each by a tap on its line (right half: forwards).
    let before = d.app.opts;
    for k in 3..=7 {
        assert!(d.tap(120, TITLE_Y + k * LINE + 2));
    }
    let o = d.app.opts;
    assert_ne!(o.four, before.four);
    assert_eq!(o.level, Level::Hard);
    assert_eq!(o.length, Length::First11);
    assert_ne!(o.wild, before.wild);
    assert_ne!(o.fast, before.fast);
    // A tap on the left half steps back.
    assert!(d.tap(20, TITLE_Y + 4 * LINE + 2));
    assert_eq!(d.app.opts.level, Level::Normal);
    assert_eq!(line_at(TITLE_Y + 2, TITLE_Y, 9), Some(0));
    // LEAVE.
    assert!(!d.tap(80, TITLE_Y + 8 * LINE + 2));
}

#[test]
fn auto_picks_sensible_sides() {
    let app = App::new();
    let none = [false; 4];
    assert_eq!(app.drivers(none)[..2], [Ctl::Touch, Ctl::Cpu]);
    assert_eq!(
        app.drivers([true, false, false, false])[..2],
        [Ctl::Pad(0), Ctl::Cpu]
    );
    assert_eq!(
        app.drivers([true, true, false, false])[..2],
        [Ctl::Pad(0), Ctl::Pad(1)]
    );
    // Only pad 2: CORAL on touch, TEAL on pad 2.
    assert_eq!(
        app.drivers([false, true, false, false])[..2],
        [Ctl::Touch, Ctl::Pad(1)]
    );
    // TEAL set to TOUCH by hand wins over CORAL AUTO.
    let mut a = App::new();
    a.opts.side[1] = Who::Touch;
    assert_eq!(a.drivers(none)[..2], [Ctl::Cpu, Ctl::Touch]);
    // CORAL set to PAD 2 by hand, TEAL AUTO: TEAL does not take pad 2.
    let mut a = App::new();
    a.opts.side[0] = Who::Pad2;
    assert_eq!(
        a.drivers([true, true, false, false])[..2],
        [Ctl::Pad(1), Ctl::Cpu]
    );
    // Never one person on two mallets, whatever the settings.
    for l in Who::ALL {
        for r in Who::ALL {
            for bits in 0..16u8 {
                let pads = core::array::from_fn(|n| bits & (1 << n) != 0);
                for four in [false, true] {
                    let mut a = App::new();
                    a.opts.side = [l, r];
                    a.opts.four = four;
                    a.touch_claimed = bits & 1 != 0;
                    let w = a.drivers(pads);
                    for i in 0..4 {
                        for j in i + 1..4 {
                            assert!(
                                w[i] == Ctl::Cpu || w[i] != w[j],
                                "{l:?} {r:?} {pads:?} {w:?}"
                            );
                        }
                    }
                    for c in w {
                        if let Ctl::Pad(n) = c {
                            assert!(pads[usize::from(n)]);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn a_pad_that_goes_away_hands_its_mallet_to_the_computer_and_back() {
    let mut d = Driver::new([true, true, false, false]);
    d.start();
    d.until_live();
    d.pads[1] = false;
    assert!(d.play_call());
    assert_eq!(d.app.ctl[1], Ctl::Cpu);
    d.pads[1] = true;
    assert!(d.play_call());
    assert_eq!(d.app.ctl[1], Ctl::Pad(1));
}

#[test]
fn pad_1_leaving_while_a_finger_holds_teal_leaves_the_finger_on_teal() {
    let mut d = Driver::new([true, false, false, false]);
    d.start();
    d.until_live();
    let t = d.touch_to(Some((140, 60)));
    assert!(d.call_with([0; 4], t));
    assert_eq!(d.app.ctl[..2], [Ctl::Pad(0), Ctl::Touch]);
    // Pad 1 drops out: the finger stays on TEAL, the computer covers CORAL
    // (before the fix the finger jumped over to CORAL and the computer
    // took TEAL).
    d.pads[0] = false;
    let t = d.touch_to(Some((140, 60)));
    assert!(d.call_with([0; 4], t));
    assert_eq!(d.app.ctl[..2], [Ctl::Cpu, Ctl::Touch]);
    d.pads[0] = true;
    let t = d.touch_to(Some((140, 60)));
    assert!(d.call_with([0; 4], t));
    assert_eq!(d.app.ctl[..2], [Ctl::Pad(0), Ctl::Touch]);
}

#[test]
fn a_rematch_counts_the_series_and_the_loser_serves_first() {
    let mut d = Driver::new([true, false, false, false]);
    d.app.opts.length = Length::First5;
    d.start();
    d.play_to_end(30 * 60 * 20);
    let w = d.app.m.winner().unwrap();
    assert_eq!(d.app.series[w], 1);
    assert_eq!(Skimmer::wanted(&d.app), rate::ON_DEMAND);
    assert!(d.press(0, button::A));
    assert_eq!(d.app.scene, Scene::Play);
    assert_eq!(d.app.m.score, [0, 0]);
    assert!(
        matches!(d.app.m.phase, Phase::Serve { side, .. } if usize::from(side) == 1 - w),
        "{:?}",
        d.app.m.phase
    );
    d.play_to_end(30 * 60 * 20);
    assert_eq!(d.app.series.iter().sum::<u8>(), 2);
    // TITLE clears the series.
    assert!(d.press(0, button::DOWN));
    assert!(d.press(0, button::A));
    assert_eq!(d.app.scene, Scene::Title);
    assert_eq!(d.app.series, [0, 0]);
}

#[test]
fn the_game_sleeps_when_nothing_moves() {
    let mut d = Driver::new([true, false, false, false]);
    assert_eq!(Skimmer::wanted(&d.app), rate::ON_DEMAND);
    // The title, left alone: nothing changes, nothing is sent.
    d.idle(20);
    assert!(d.sent[d.sent.len() - 19..].iter().all(|(px, _)| *px == 0));
    d.start();
    assert_eq!(Skimmer::wanted(&d.app), rate::FPS60);
    d.app.opts.fast = false;
    assert_eq!(Skimmer::wanted(&d.app), rate::FPS30);
    assert!(d.press(0, button::START));
    assert_eq!(Skimmer::wanted(&d.app), rate::ON_DEMAND);
    let n = d.sent.len();
    d.idle(10);
    assert!(d.sent[n..].iter().all(|(px, _)| *px == 0));
}

/// Plays `ticks` of computer against computer and a dragged finger,
/// with calls of `per_call` ticks each.
fn run_at(per_call: u32, ticks: u32) -> (Match, Vec<Log>) {
    let mut d = Driver::new([false; 4]);
    d.app.opts.side[1] = Who::Computer;
    d.delta_us = per_call * TICK_US;
    d.start();
    // The tap on PLAY ends in the first call of the match: one tick at
    // 60, two at 30. Level them.
    if per_call == 1 {
        assert!(d.call([0; 4]));
    }
    assert_eq!(d.app.m.age, 2);
    while d.app.m.age < ticks && d.app.scene == Scene::Play {
        // The finger moves only on even ticks, so both rates see it the
        // same.
        let t = d.app.m.age / 2;
        let x = 20 + (t % 50) as i16;
        let y = 30 + (t % 70) as i16;
        let tch = d.touch_to(Some((x, y)));
        assert!(d.call_with([0; 4], tch));
    }
    (d.app.m.clone(), d.logs.clone())
}

#[test]
fn thirty_and_sixty_calls_a_second_play_the_same() {
    let (a, la) = run_at(1, 60 * 60 * 2);
    let (b, lb) = run_at(2, 60 * 60 * 2);
    assert_eq!(a.age, b.age);
    assert_eq!(a.score, b.score);
    assert_eq!(a.puck, b.puck);
    assert_eq!(a.m, b.m);
    assert_eq!(a.phase, b.phase);
    assert_eq!(la, lb);
    assert!(a.score[0] + a.score[1] > 0, "no goals: {:?}", a.score);
}

#[test]
fn uneven_frame_times_lose_no_time() {
    // 30-ish calls a second with jitter: the match's age follows the
    // time given, a tick at most behind.
    let mut d = Driver::new([false; 4]);
    d.app.opts.side = [Who::Computer, Who::Computer];
    d.app.opts.length = Length::Timed;
    d.start();
    let mut total = 0u64;
    let start_age = d.app.m.age;
    for k in 0..3_000u32 {
        d.delta_us = 25_000 + (k * 7_919) % 17_000;
        total += u64::from(d.delta_us);
        assert!(d.call([0; 4]));
    }
    let ticks = u64::from(d.app.m.age - start_age);
    let want = total / u64::from(TICK_US);
    assert!(ticks + 1 >= want && ticks <= want, "{ticks} of {want}");
}

#[test]
fn the_same_input_gives_the_same_game() {
    let run = || {
        let mut d = Driver::new([true, false, false, false]);
        d.app.opts.wild = true;
        d.start();
        let mut hash = 0xCBF2_9CE4_8422_2325u64;
        for _ in 0..30 * 60 * 3 {
            assert!(d.play_call());
            if d.app.scene != Scene::Play {
                break;
            }
            for &b in d.frame.0.iter().step_by(7) {
                hash = (hash ^ u64::from(b)).wrapping_mul(0x100_0000_01B3);
            }
        }
        (hash, d.app.m.score, d.app.m.age, d.logs.clone())
    };
    assert_eq!(run(), run());
}

#[test]
fn the_puck_never_ends_up_inside_a_mallet_in_play() {
    // Four pads and the test players, both tables: after every call the
    // puck is off every mallet.
    for wild in [false, true] {
        let mut d = Driver::new([true; 4]);
        d.app.opts.four = true;
        d.app.opts.wild = wild;
        d.start();
        for _ in 0..30 * 60 * 3 {
            assert!(d.play_call());
            if d.app.scene != Scene::Play {
                break;
            }
            let m = &d.app.m;
            if m.puck_shown() {
                for i in 0..4 {
                    let g = len(m.puck.x - m.m[i].x, m.puck.y - m.m[i].y);
                    assert!(g >= HIT_R - 4, "inside mallet {i}: {g}");
                }
            }
        }
    }
}

#[test]
fn a_touch_tap_in_one_call_does_not_break_anything() {
    // A tap that comes down and up between two calls.
    let mut d = Driver::new([false; 4]);
    d.start();
    d.until_live();
    let t = Touch {
        x: 30,
        y: 60,
        down: false,
        started: true,
        ended: true,
    };
    assert!(d.call_with([0; 4], Some(t)));
    d.idle(5);
    assert_eq!(d.app.scene, Scene::Play);
    let k = Keys {
        delta_us: CALL_US,
        ..Keys::default()
    };
    assert!(d.app.update(&k));
    assert_eq!(MALLET_R / PX, 6);
}
