//! Whole matches and the controls, driven call by call as the console
//! drives the game.

mod common;

use common::{Driver, evidence, save_frame};
use game_kit::{Touch, button, rate};
use tundra::Tundra;
use tundra::app::{App, Ctl, END_BOX, Log, PAUSE_Y, Right, Scene, TICK_US};
use tundra::draw;
use tundra::fixed::PX;
use tundra::play::{Length, OT_TICKS, Owner, Phase};

/// Plays one call of exactly one tick on a copy of `app`.
fn one_tick(d: &Driver, app: &App, held: [u32; 4], touch: Option<Touch>) -> App {
    let mut a = app.clone();
    let mut k = d.keys(held, touch, TICK_US);
    // Every button counts as just pressed, as if held from this call on.
    for p in &mut k.pads {
        p.pressed = p.held;
    }
    assert!(a.update(&k));
    a
}

/// With `held` for one player only, exactly skater `own` moves
/// differently from the run where nobody presses anything.
/// Skaters far apart (so no push between them counts as moving one),
/// the puck loose in the middle.
fn fixed_spots(d: &Driver) -> App {
    let mut app = d.app.clone();
    let spots = [(30, 30), (30, 100), (130, 30), (130, 100)];
    for (s, (x, y)) in app.m.sk.iter_mut().zip(spots) {
        s.x = x * PX;
        s.y = y * PX;
    }
    app.m.puck.owner = Owner::Loose;
    app.m.puck.x = 80 * PX;
    app.m.puck.y = 66 * PX;
    app
}

fn only_own_skater_moves(d: &Driver, player: usize, own: usize) {
    let mut held = [0; 4];
    held[player] = button::UP | button::RIGHT;
    let app = fixed_spots(d);
    let still = one_tick(d, &app, [0; 4], None);
    let moved = one_tick(d, &app, held, None);
    for i in 0..4 {
        let (a, b) = (still.m.sk[i], moved.m.sk[i]);
        if i == own {
            assert_ne!(
                (a.vx, a.vy),
                (b.vx, b.vy),
                "player {} moves skater {i}",
                player + 1
            );
        } else {
            assert_eq!(a, b, "player {} must not move skater {i}", player + 1);
        }
    }
}

#[test]
fn a_one_player_match_reaches_a_goal_and_the_final_whistle() {
    let mut d = Driver::new([true, false, false, false]);
    // MATCH: 3 MIN back to 2 MIN.
    for _ in 0..3 {
        d.press(0, button::DOWN);
    }
    d.press(0, button::LEFT);
    assert_eq!(d.app.opts.length, Length::Min2);
    d.start();
    assert_eq!(d.app.ctl, [Ctl::Pad(0), Ctl::Cpu, Ctl::Cpu, Ctl::Cpu]);
    let mut calls = 0;
    while d.app.scene == Scene::Play {
        let held = d.bot_held();
        assert!(d.call([held, 0, 0, 0]));
        calls += 1;
        assert!(calls < 30 * 60 * 5, "the match never ended");
    }
    assert_eq!(d.app.scene, Scene::End);
    assert!(d.has_log(|l| matches!(l, Log::Goal { .. })), "{:?}", d.logs);
    let finals: Vec<_> = d
        .logs
        .iter()
        .filter(|l| matches!(l, Log::Final { .. }))
        .collect();
    assert_eq!(finals.len(), 1);
    let w = d.app.m.winner().expect("a timed match ends with a winner");
    assert_eq!(d.app.series[w], 1);
    println!(
        "1P match: {:?} in {calls} calls (overtime {})",
        d.app.m.score, d.app.m.overtime
    );
    // Rematch: a new match, the series kept.
    assert!(d.press(0, button::A));
    assert_eq!(d.app.scene, Scene::Play);
    assert_eq!(d.app.m.score, [0, 0]);
    assert_eq!(d.app.series[w], 1);
}

#[test]
fn with_two_players_each_moves_only_their_own_skater() {
    let mut d = Driver::new([true, true, false, false]);
    d.start();
    assert_eq!(d.app.ctl, [Ctl::Pad(0), Ctl::Cpu, Ctl::Pad(1), Ctl::Cpu]);
    d.until_live();
    d.idle(30);
    only_own_skater_moves(&d, 0, 0);
    only_own_skater_moves(&d, 1, 2);
    // Player 1 switches to skater 1 (X); player 2's skater stays.
    assert!(d.press(0, button::X));
    assert_eq!(d.app.ctl, [Ctl::Cpu, Ctl::Pad(0), Ctl::Pad(1), Ctl::Cpu]);
    only_own_skater_moves(&d, 0, 1);
    only_own_skater_moves(&d, 1, 2);
}

#[test]
fn four_players_each_hold_one_skater_and_nobody_switches() {
    let mut d = Driver::new([true; 4]);
    d.start();
    // Skaters 0 and 1 are AMBER (players 1 and 3), 2 and 3 BLUE (2 and 4).
    assert_eq!(
        d.app.ctl,
        [Ctl::Pad(0), Ctl::Pad(2), Ctl::Pad(1), Ctl::Pad(3)]
    );
    match d.logs.iter().find(|l| matches!(l, Log::Match { .. })) {
        Some(Log::Match { who, .. }) => assert!(who.iter().all(|c| *c != Ctl::Cpu)),
        _ => panic!("no match log"),
    }
    d.until_live();
    d.idle(20);
    for (player, skater) in [(0, 0), (1, 2), (2, 1), (3, 3)] {
        only_own_skater_moves(&d, player, skater);
    }
    // Two people on a team: X does not switch.
    let before = d.app.ctl;
    assert!(d.press(0, button::X));
    assert!(d.press(1, button::Y));
    assert_eq!(d.app.ctl, before);
    // A minute of all four skating about: everyone stays theirs.
    for k in 0..1800u32 {
        let dirs = [button::RIGHT, button::LEFT, button::UP, button::DOWN];
        let held = core::array::from_fn(|p| {
            dirs[(k as usize / 40 + p) % 4] | if k % 50 < 10 { button::A } else { 0 }
        });
        assert!(d.call(held));
        if d.app.scene != Scene::Play {
            break;
        }
        assert_eq!(d.app.ctl, before);
    }
}

#[test]
fn touch_takes_player_two_steers_and_a_tap_shoots() {
    // No pad 2: AUTO hands the right team to the computer until a
    // finger touches the ice.
    let mut d = Driver::new([true, false, false, false]);
    assert_eq!(d.app.opts.right, Right::Auto);
    d.start();
    assert_eq!(d.app.ctl[2], Ctl::Cpu);
    d.until_live();
    // A finger on the ice, far below-right of skater 2.
    let s = d.app.m.sk[2];
    let (fx, fy) = ((s.x / PX) as i16 + 30, (s.y / PX) as i16 + 30);
    assert!(d.finger(fx, fy, true));
    assert!(d.has_log(|l| *l == Log::TouchPlays));
    assert_eq!(d.app.ctl[2], Ctl::Touch);
    let start = d.app.m.sk[2];
    for _ in 0..10 {
        assert!(d.finger(fx, fy, false));
    }
    let now = d.app.m.sk[2];
    assert!(
        now.x > start.x + 4 * PX && now.y > start.y + 4 * PX,
        "{start:?} -> {now:?}"
    );
    // Up again (no tap: it was held long). Then give skater 2 the puck
    // and tap far to its left: a shot that way.
    assert!(d.call_with(
        [0; 4],
        Some(Touch {
            x: fx,
            y: fy,
            down: false,
            started: false,
            ended: true
        })
    ));
    assert!(d.app.m.puck.owner == Owner::Loose || d.app.m.carrier().is_some());
    d.app.m.puck.owner = Owner::Skater(2);
    d.app.m.sk[2].no_pick = 0;
    d.app.m.sk[2].stun = 0;
    let s = d.app.m.sk[2];
    let (tx, ty) = ((s.x / PX) as i16 - 40, (s.y / PX) as i16);
    assert!(d.tap(tx.max(10), ty));
    assert_eq!(d.app.m.puck.owner, Owner::Loose, "the tap shot the puck");
    assert!(
        d.app.m.puck.vx < -300,
        "towards the tap: {:?}",
        d.app.m.puck
    );
    // The claim lasts through the match; the title gives it back.
    d.idle(5);
    assert_eq!(d.app.ctl[2], Ctl::Touch);
}

#[test]
fn a_tap_without_the_puck_checks() {
    let mut d = Driver::new([true, false, false, false]);
    d.app.opts.right = Right::Touch;
    d.start();
    assert_eq!(d.app.ctl[2], Ctl::Touch);
    d.until_live();
    d.app.m.sk[2].cool = 0;
    let s = d.app.m.sk[2];
    assert!(d.tap((s.x / PX) as i16 - 20, (s.y / PX) as i16));
    let s = d.app.m.sk[2];
    assert!(s.cool > 0, "the check started: {s:?}");
}

#[test]
fn pad_two_wins_over_touch_in_auto() {
    let mut d = Driver::new([true, true, false, false]);
    d.start();
    d.until_live();
    assert!(d.finger(120, 60, true));
    assert_eq!(d.app.ctl[2], Ctl::Pad(1));
    assert!(!d.has_log(|l| *l == Log::TouchPlays));
}

#[test]
fn the_pause_menu_resumes_and_leaves() {
    let mut d = Driver::new([true, false, true, false]);
    d.start();
    d.until_live();
    // START on player 3's pad pauses too.
    assert!(d.press(2, button::START));
    assert_eq!(d.app.pause, Some(0));
    assert_eq!(Tundra::wanted(&d.app), rate::ON_DEMAND);
    let m = d.app.m.clone();
    d.idle(30);
    assert_eq!(d.app.m, m, "nothing moves while paused");
    // B resumes.
    assert!(d.press(0, button::B));
    assert_eq!(d.app.pause, None);
    assert_eq!(Tundra::wanted(&d.app), rate::FPS60);
    // A tap on the bar pauses; a tap on RESUME goes back.
    assert!(d.tap(80, 4));
    assert_eq!(d.app.pause, Some(0));
    assert!(d.tap(60, PAUSE_Y + 2));
    assert_eq!(d.app.pause, None);
    // COMPUTER: HARD from the pause menu, then LEAVE.
    assert!(d.press(0, button::START));
    assert!(d.press(0, button::DOWN));
    assert!(d.press(0, button::RIGHT));
    assert_eq!(d.app.m.level, tundra::play::Level::Hard);
    for _ in 0..4 {
        assert!(d.press(0, button::DOWN));
    }
    assert_eq!(d.app.pause, Some(5));
    assert!(!d.press(0, button::A), "LEAVE ends the game");
    assert!(d.has_log(|l| *l == Log::Leave));
}

#[test]
fn leave_on_the_title_and_title_from_the_pause_menu() {
    let mut d = Driver::new([true, false, false, false]);
    d.start();
    d.until_live();
    assert!(d.press(0, button::START));
    for _ in 0..4 {
        assert!(d.press(0, button::DOWN));
    }
    assert!(d.press(0, button::A));
    assert_eq!(d.app.scene, Scene::Title);
    assert!(d.press(0, button::UP));
    assert_eq!(d.app.sel, 6);
    assert!(!d.press(0, button::A));
}

#[test]
fn the_game_sleeps_when_nothing_moves() {
    let mut d = Driver::new([true, false, false, false]);
    // The title: the on-demand rate, and no change without input.
    assert_eq!(Tundra::wanted(&d.app), rate::ON_DEMAND);
    d.idle(5);
    assert!(d.sent[d.sent.len() - 5..].iter().all(|(px, _)| *px == 0));
    // Playing: 60 a second at the start (30 when chosen).
    d.start();
    assert_eq!(Tundra::wanted(&d.app), rate::FPS60);
    d.app.opts.fast = false;
    assert_eq!(Tundra::wanted(&d.app), rate::FPS30);
    d.app.opts.fast = true;
    // Paused: asleep, the picture still.
    assert!(d.press(0, button::START));
    assert_eq!(Tundra::wanted(&d.app), rate::ON_DEMAND);
    let n = d.sent.len();
    d.idle(5);
    assert!(d.sent[n..].iter().all(|(px, _)| *px == 0));
    // The result screen: asleep too.
    assert!(d.press(0, button::START));
    d.app.m.played = 3 * 60 * 60 - 2;
    d.app.m.score = [2, 1];
    while d.app.scene == Scene::Play {
        d.idle(1);
    }
    assert_eq!(Tundra::wanted(&d.app), rate::ON_DEMAND);
    d.idle(1);
    let n = d.sent.len();
    d.idle(5);
    assert!(d.sent[n..].iter().all(|(px, _)| *px == 0));
}

#[test]
fn the_same_input_plays_the_same_match() {
    let run = || {
        let mut d = Driver::new([true, true, false, false]);
        d.start();
        for k in 0..900u32 {
            let held = d.bot_held();
            let p2 = if k % 60 < 30 {
                button::LEFT
            } else {
                button::UP | button::A
            };
            assert!(d.call([held, p2, 0, 0]));
        }
        (d.app.m.clone(), d.frame.0.to_vec())
    };
    let (a, fa) = run();
    let (b, fb) = run();
    assert_eq!(a, b);
    assert!(fa == fb);
}

/// Puts the puck back on the centre spot, loose and still: from there no
/// shot reaches a goal within one call.
fn hold_puck(d: &mut Driver) {
    let m = &mut d.app.m;
    m.puck.owner = Owner::Loose;
    m.puck.x = tundra::play::CX;
    m.puck.y = tundra::play::CY;
    m.puck.vx = 0;
    m.puck.vy = 0;
}

#[test]
fn a_level_overtime_ends_as_a_draw_at_three_minutes() {
    let limit = 3 * 60 * 60; // the default MATCH, 3 MIN
    let mut d = Driver::new([true, false, false, false]);
    d.start();
    d.until_live();
    d.app.series = [2, 1];
    // Level at the end of the clock: overtime.
    d.app.m.score = [1, 1];
    d.app.m.played = limit - 1;
    hold_puck(&mut d);
    d.idle(1);
    assert!(d.app.m.overtime);
    assert_eq!(d.app.m.phase, Phase::Live);
    // The bar counts the overtime down from 3:00.
    assert!(d.app.m.clock() > OT_TICKS - 4 && d.app.m.clock() <= OT_TICKS);
    // A while into the overtime: the bar's clock, for the record. (The
    // skip jumps over the two seconds of the "OVERTIME" note: gone.)
    d.app.m.played = limit + 42 * 60;
    d.app.m.ot_note = 0;
    hold_puck(&mut d);
    d.idle(1);
    save_frame(&evidence("overtime").join("ot-clock.png"), &d.frame, 3);
    // Skip to just before the cap, then nobody scores.
    d.app.m.played = limit + OT_TICKS - 20;
    let mut calls = 0;
    while d.app.m.phase == Phase::Live {
        hold_puck(&mut d);
        d.idle(1);
        calls += 1;
        assert!(calls < 30, "overtime ran past its cap");
    }
    assert!(matches!(d.app.m.phase, Phase::Whistle(_)));
    assert_eq!(
        d.app.m.played,
        limit + OT_TICKS,
        "the whistle comes at the cap"
    );
    assert_eq!(d.app.m.score, [1, 1]);
    assert!(d.app.m.draw());
    while d.app.scene == Scene::Play {
        d.idle(1);
    }
    assert_eq!(d.app.scene, Scene::End);
    assert_eq!(d.app.m.winner(), None);
    assert_eq!(d.app.series, [2, 1], "a draw counts for nobody");
    assert!(d.has_log(|l| *l
        == Log::Final {
            score: [1, 1],
            series: [2, 1]
        }));
    // The draw screen: no team badges inside the panel.
    save_frame(&evidence("overtime").join("draw.png"), &d.frame, 3);
    let (x, y, w, h) = END_BOX;
    for py in y..y + h as i16 {
        for px in x..x + w as i16 {
            let c = d.frame.0[py as usize * game_kit::W + px as usize];
            assert!(
                ![draw::AMBER, draw::AMBER_DARK, draw::BLUE, draw::BLUE_DARK].contains(&c),
                "a team colour at ({px}, {py}) on the draw screen"
            );
        }
    }
    // REMATCH: a new match from 0-0, the series kept.
    assert!(d.press(0, button::A));
    assert_eq!(d.app.scene, Scene::Play);
    assert_eq!(d.app.m.score, [0, 0]);
    assert!(!d.app.m.overtime);
    assert_eq!(d.app.series, [2, 1]);
}

#[test]
fn a_goal_in_overtime_still_ends_the_match_at_once() {
    let limit = 3 * 60 * 60;
    let mut d = Driver::new([true, false, false, false]);
    d.start();
    d.until_live();
    d.app.m.played = limit - 1;
    hold_puck(&mut d);
    d.idle(1);
    assert!(d.app.m.overtime);
    // Half way through the overtime, the puck slides into the right goal.
    d.app.m.played = limit + OT_TICKS / 2;
    let m = &mut d.app.m;
    m.puck.owner = Owner::Loose;
    m.puck.x = 146 * PX;
    m.puck.y = 66 * PX;
    m.puck.vx = 700;
    m.puck.vy = 0;
    m.puck.ghost = 30;
    d.idle(2);
    assert_eq!(d.app.m.score, [1, 0]);
    while d.app.scene == Scene::Play {
        assert!(
            !matches!(d.app.m.phase, Phase::FaceOff(_)),
            "no face-off after an overtime goal"
        );
        d.idle(1);
    }
    assert!(d.app.m.played < limit + OT_TICKS);
    assert_eq!(d.app.m.winner(), Some(0));
    assert!(!d.app.m.draw());
    assert_eq!(d.app.series, [1, 0]);
    save_frame(&evidence("overtime").join("ot-win.png"), &d.frame, 3);
}

#[test]
fn a_goal_stops_play_and_the_face_off_follows() {
    let mut d = Driver::new([true, false, false, false]);
    d.start();
    d.until_live();
    // The puck slides into the right goal.
    let m = &mut d.app.m;
    m.puck.owner = Owner::Loose;
    m.puck.x = 146 * PX;
    m.puck.y = 66 * PX;
    m.puck.vx = 700;
    m.puck.vy = 0;
    m.puck.ghost = 30;
    d.idle(2);
    assert!(
        matches!(d.app.m.phase, Phase::Goal { team: 0, .. }),
        "{:?}",
        d.app.m.phase
    );
    assert_eq!(d.app.m.score, [1, 0]);
    assert!(d.has_log(|l| matches!(l, Log::Goal { team: 0, .. })));
    for _ in 0..60 {
        d.idle(1);
        if matches!(d.app.m.phase, Phase::FaceOff(_)) {
            return;
        }
    }
    panic!("no face-off after the goal");
}
