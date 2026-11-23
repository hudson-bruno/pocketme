//! Long random play (added by the checker, 2026-11-18): the whole game,
//! call by call, with random pads, sticks, touches, frame times and pads
//! coming and going, for 1 to 4 players and the touch screen. After
//! every call it checks that nothing left the ice, the score and the
//! clock are sane (no overtime runs past its 3-minute cap), and that a
//! loose puck never lies untouched for ever; the picture is drawn every
//! call.
//!
//! The default is short (a few minutes of game time). For hours:
//!
//! ```text
//! TUNDRA_FUZZ=40 cargo test --release --offline \
//!     --config profile.release.overflow-checks=true --test fuzz
//! ```
//!
//! (`TUNDRA_FUZZ` = how many runs; each run is about 30 minutes of game
//! time.) Overflow checks on, as in the chip build.

use game_kit::{Frame, Touch, button};
use tundra::app::{App, END_Y, Keys, PadIn, Scene, TITLE_Y};
use tundra::draw;
use tundra::play::{
    BOARD_B, BOARD_L, BOARD_R, BOARD_T, Ev, NET_DEPTH, OT_TICKS, Owner, PUCK_R, Phase, XL, XR, YB,
    YT,
};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
    fn chance(&mut self, in_1000: u32) -> bool {
        self.below(1000) < in_1000
    }
}

/// What one run found.
#[derive(Default, Debug)]
struct Seen {
    calls: u64,
    game_us: u64,
    matches: u32,
    finals: u32,
    draws: u32,
    goals: u32,
    leaves: u32,
    pause_in_goal: u32,
    pause_in_faceoff: u32,
    pause_in_whistle: u32,
    restarts: u32,
    titles: u32,
    longest_loose_ticks: u32,
}

const ALL_BUTTONS: u32 = button::ALL;

/// One run: `calls` calls with seed `seed`. Players present at random,
/// changing now and then.
fn run(seed: u64, calls: u32) -> Seen {
    let mut r = Rng(seed | 1);
    let mut app = App::new();
    let mut frame = Box::new(Frame::new());
    let mut seen = Seen::default();
    let mut pads = [false; 4];
    for p in &mut pads {
        *p = r.chance(500);
    }
    let mut held = [0u32; 4];
    let mut sticks = [(0i16, 0i16); 4];
    // How "busy" each player is: some mash, some barely touch.
    let busy: [u32; 4] = [r.below(300), r.below(300), r.below(300), r.below(300)];
    let mut finger: Option<(i16, i16)> = None;
    let mut loose_ticks = 0u32;
    let mut last_played = 0u32;
    let mut frame_no = 0u32;
    let mut first = true;

    for _ in 0..calls {
        // Pads come and go (pad 2 more often, against the touch).
        for (n, p) in pads.iter_mut().enumerate() {
            let odds = if n == 1 { 4 } else { 1 };
            if r.chance(odds) {
                *p = !*p;
            }
        }
        let mut k = Keys {
            frame: frame_no,
            ..Keys::default()
        };
        for n in 0..4 {
            if !pads[n] {
                held[n] = 0;
                continue;
            }
            let was = held[n];
            if r.below(1000) < busy[n] {
                // Flip a few buttons; Start and Select rarely.
                let mut flip = r.next() & ALL_BUTTONS;
                if !r.chance(30) {
                    flip &= !(button::START | button::SELECT);
                }
                held[n] ^= flip & (r.next() | r.next());
            }
            if r.chance(20) {
                sticks[n] = (r.next() as i16, r.next() as i16);
            }
            // Short presses the console noted (down and up between calls).
            let quick = if r.chance(15) {
                r.next() & ALL_BUTTONS & !button::START
            } else {
                0
            };
            k.pads[n] = PadIn {
                connected: true,
                held: held[n],
                pressed: (held[n] & !was) | quick,
                stick: sticks[n],
            };
        }
        // The touch: a finger comes down, moves, comes up; taps.
        let t = match finger {
            None if r.chance(25) => {
                let at = (r.below(160) as i16, r.below(120) as i16);
                if r.chance(400) {
                    // A tap over in one call.
                    Some(Touch {
                        x: at.0,
                        y: at.1,
                        down: false,
                        started: true,
                        ended: true,
                    })
                } else {
                    finger = Some(at);
                    Some(Touch {
                        x: at.0,
                        y: at.1,
                        down: true,
                        started: true,
                        ended: false,
                    })
                }
            }
            None => None,
            Some((x, y)) => {
                let nx = (x + r.below(9) as i16 - 4).clamp(0, 159);
                let ny = (y + r.below(9) as i16 - 4).clamp(0, 119);
                if r.chance(40) {
                    finger = None;
                    Some(Touch {
                        x: nx,
                        y: ny,
                        down: false,
                        started: false,
                        ended: true,
                    })
                } else {
                    finger = Some((nx, ny));
                    Some(Touch {
                        x: nx,
                        y: ny,
                        down: true,
                        started: false,
                        ended: false,
                    })
                }
            }
        };
        k.touch = t;
        // Calm the chaos so matches get played: in a menu, most calls
        // carry no presses and no new touch, and now and then one that
        // goes on (PLAY, resume, rematch); in play, the top bar (pause)
        // is touched only rarely.
        let menu = app.scene != Scene::Play || app.pause.is_some();
        if menu && !r.chance(60) {
            for p in &mut k.pads {
                p.pressed = 0;
            }
            if k.touch.is_some_and(|t| t.started) {
                k.touch = None;
                finger = None;
            }
            if r.chance(30) {
                let at = match app.scene {
                    Scene::Title => (120, TITLE_Y + 2),
                    Scene::End => (60, END_Y + 2),
                    // Outside the pause panel: back to the match.
                    Scene::Play => (2, 115),
                };
                k.touch = Some(Touch {
                    x: at.0,
                    y: at.1,
                    down: false,
                    started: true,
                    ended: true,
                });
            }
        }
        if !menu
            && let Some(t) = &mut k.touch
            && t.started
            && t.y < 12
            && !r.chance(50)
        {
            t.y += 12;
            if let Some(f) = &mut finger {
                f.1 = t.y;
            }
        }
        // Frame times: 30 or 60 a second, now and then a long wait (the
        // console sleeping, a slow frame), the first call none.
        k.delta_us = if first {
            0
        } else {
            match r.below(100) {
                0 => r.next(),
                1..=3 => r.below(500_000),
                4..=40 => 16_667,
                _ => 33_333,
            }
        };
        first = false;

        let scene_was = app.scene;
        let pause_was = app.pause.is_some();
        let phase_was = app.m.phase;
        let age_was = app.m.age;
        let going = app.update(&k);
        seen.calls += 1;
        seen.game_us += u64::from(k.delta_us.min(6 * 16_667));
        frame_no = frame_no.wrapping_add(1);
        while let Some(l) = app.take_log() {
            match l {
                tundra::app::Log::Match { .. } => seen.matches += 1,
                tundra::app::Log::Goal { .. } => seen.goals += 1,
                tundra::app::Log::Final { score, .. } => {
                    seen.finals += 1;
                    if score[0] == score[1] {
                        seen.draws += 1;
                    }
                }
                _ => {}
            }
        }
        if !going {
            seen.leaves += 1;
            app = App::new();
            first = true;
            continue;
        }
        if !pause_was && app.pause.is_some() {
            match phase_was {
                Phase::Goal { .. } => seen.pause_in_goal += 1,
                Phase::FaceOff(_) => seen.pause_in_faceoff += 1,
                Phase::Whistle(_) => seen.pause_in_whistle += 1,
                _ => {}
            }
        }
        if scene_was == Scene::Play && pause_was && app.scene == Scene::Title {
            seen.titles += 1;
        }
        if scene_was == Scene::Play && pause_was && app.scene == Scene::Play && app.m.age < age_was
        {
            seen.restarts += 1;
        }
        draw::draw(&app, &mut frame);

        if app.scene == Scene::Play {
            check(&app, seed);
            // A loose puck that nobody touches, while the clock runs.
            let m = &app.m;
            let played = m.played;
            if m.phase == Phase::Live && m.puck.owner == Owner::Loose && played > last_played {
                loose_ticks += played - last_played;
            } else if m.puck.owner != Owner::Loose || m.phase != Phase::Live {
                loose_ticks = 0;
            }
            last_played = played;
            seen.longest_loose_ticks = seen.longest_loose_ticks.max(loose_ticks);
            assert!(
                loose_ticks < 60 * 60,
                "seed {seed}: the puck lay loose for a minute at ({}, {}) v ({}, {})",
                m.puck.x / 256,
                m.puck.y / 256,
                m.puck.vx,
                m.puck.vy
            );
        } else {
            loose_ticks = 0;
            last_played = 0;
        }
    }
    seen
}

/// Everything on the ice, score and clock sane.
fn check(app: &App, seed: u64) {
    let m = &app.m;
    let p = m.puck;
    // The puck: on the ice, or inside a net (never through its back).
    assert!(
        p.x >= XL - NET_DEPTH && p.x <= XR + NET_DEPTH,
        "seed {seed}: puck x {} off the rink",
        p.x
    );
    assert!(
        p.y >= YT + PUCK_R - 1 && p.y <= YB - PUCK_R + 1,
        "seed {seed}: puck y {} off the rink",
        p.y
    );
    if p.x < XL || p.x > XR {
        // Only in a net: inside the mouth.
        assert!(
            (p.y - (YT + YB) / 2).abs() <= tundra::play::MOUTH,
            "seed {seed}: puck behind the boards at ({}, {})",
            p.x,
            p.y
        );
    }
    for (i, s) in m.sk.iter().enumerate() {
        assert!(
            s.x >= XL && s.x <= XR && s.y >= YT && s.y <= YB,
            "seed {seed}: skater {i} off the ice at ({}, {})",
            s.x,
            s.y
        );
        assert!(
            s.vx.abs() < 2_000 && s.vy.abs() < 2_000,
            "seed {seed}: skater {i} too fast"
        );
    }
    assert!(
        p.vx.abs() < 4_000 && p.vy.abs() < 4_000,
        "seed {seed}: puck too fast"
    );
    for g in &m.gk {
        assert!(g.y >= YT && g.y <= YB, "seed {seed}: goalie off the ice");
    }
    // The picture's rink is where the play thinks it is.
    const { assert!(BOARD_L < BOARD_R && BOARD_T < BOARD_B) };
    // Score: at most a goal every ~150 ticks of the match's age.
    let goals = u32::from(m.score[0]) + u32::from(m.score[1]);
    assert!(
        goals <= m.age / 100 + 1,
        "seed {seed}: {goals} goals in {} ticks",
        m.age
    );
    if let Some(n) = m.length.goals() {
        assert!(
            m.score.iter().all(|&s| s <= n),
            "seed {seed}: first to {n} went past"
        );
    }
    if let Some(t) = m.length.ticks() {
        if m.overtime {
            assert!(
                m.played <= t + OT_TICKS,
                "seed {seed}: the overtime ran past its cap"
            );
        } else {
            assert!(m.played <= t, "seed {seed}: the clock ran past its end");
        }
    } else {
        assert!(!m.overtime, "seed {seed}: overtime in a FIRST TO 5 match");
    }
    // A match that is over and level: only after a full overtime.
    if matches!(m.phase, Phase::Whistle(_) | Phase::Over) && m.score[0] == m.score[1] {
        let t = m.length.ticks().expect("only a timed match can end level");
        assert!(
            m.overtime && m.played == t + OT_TICKS,
            "seed {seed}: a draw before the cap"
        );
    }
    // Who drives the skaters: each person on at most one skater.
    let mut people = [0u8; 5];
    for c in app.ctl {
        if let Some(n) = c.player() {
            people[usize::from(n)] += 1;
        }
    }
    assert!(
        people.iter().all(|&n| n <= 1),
        "seed {seed}: one player on two skaters: {:?}",
        app.ctl
    );
}

#[test]
fn random_play_never_breaks_the_game() {
    let runs: u64 = std::env::var("TUNDRA_FUZZ")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    // Default: 3 short runs (about 5 minutes of game time each).
    let (n, calls) = if runs == 0 {
        (3, 9_000)
    } else {
        (runs, 54_000)
    };
    let mut total = Seen::default();
    for s in 0..n {
        let seen = run(0x5EED_0000 + s * 0x9E37_79B9, calls);
        total.calls += seen.calls;
        total.game_us += seen.game_us;
        total.matches += seen.matches;
        total.finals += seen.finals;
        total.draws += seen.draws;
        total.goals += seen.goals;
        total.leaves += seen.leaves;
        total.pause_in_goal += seen.pause_in_goal;
        total.pause_in_faceoff += seen.pause_in_faceoff;
        total.pause_in_whistle += seen.pause_in_whistle;
        total.restarts += seen.restarts;
        total.titles += seen.titles;
        total.longest_loose_ticks = total.longest_loose_ticks.max(seen.longest_loose_ticks);
    }
    println!("fuzz: {total:?}");
    assert!(total.matches > 0 && total.goals > 0);
}

/// The computer alone (nobody holds a skater), many matches: a loose
/// puck is always taken again, every match ends, and no overtime lasts
/// longer than its cap.
#[test]
fn computer_matches_always_end() {
    let runs: u64 = std::env::var("TUNDRA_FUZZ")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
        .max(2);
    let (mut overtimes, mut draws) = (0, 0);
    for s in 0..runs {
        let mut r = Rng(0xC0FF_EE00 + s);
        let length = tundra::play::Length::ALL[r.below(4) as usize];
        let level = if r.chance(500) {
            tundra::play::Level::Easy
        } else {
            tundra::play::Level::Hard
        };
        let mut m = tundra::play::Match::new(length, level, true, r.next());
        let mut loose = 0u32;
        let mut ticks = 0u32;
        while m.phase != Phase::Over {
            let mut its = [tundra::play::Intent::default(); 4];
            for (i, it) in its.iter_mut().enumerate() {
                *it = tundra::cpu::intent(&mut m, i);
            }
            m.tick(&its);
            while let Some(e) = m.take_event() {
                let _ = matches!(e, Ev::Goal { .. });
            }
            ticks += 1;
            if m.phase == Phase::Live && m.puck.owner == Owner::Loose {
                loose += 1;
                assert!(loose < 60 * 30, "run {s}: a loose puck nobody takes");
            } else {
                loose = 0;
            }
            assert!(ticks < 60 * 60 * 30, "run {s}: a match of half an hour");
            match length.ticks() {
                Some(t) => assert!(
                    m.played <= t + OT_TICKS,
                    "run {s}: the overtime ran past its cap"
                ),
                None => assert!(!m.overtime, "run {s}: overtime in FIRST TO 5"),
            }
        }
        if m.score[0] == m.score[1] {
            draws += 1;
            let t = length.ticks().expect("FIRST TO 5 cannot end level");
            assert_eq!(m.played, t + OT_TICKS, "run {s}: a draw before the cap");
        }
        if m.overtime {
            overtimes += 1;
        }
    }
    println!("computer matches: {runs}, overtimes {overtimes}, draws {draws}");
}
