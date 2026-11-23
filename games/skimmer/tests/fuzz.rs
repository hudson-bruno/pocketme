//! Long random play: the whole game, call by call, with random pads,
//! sticks, fingers, frame times (huge ones too) and pads coming and
//! going. After every call: the puck is on the table (or in a slot),
//! never inside a mallet, every mallet in its zone, one person on one
//! mallet at most, the score and the clock sane, and the puck never
//! stays on one half past the stall time. The picture is drawn every
//! call. In a debug build (or with debug assertions on) the table also
//! checks every sub-step: the puck never jumps through a mallet.
//!
//! The default is short. For hours of game time:
//!
//! ```text
//! SKIMMER_FUZZ=40 cargo test --release --offline \
//!     --config profile.release.overflow-checks=true \
//!     --config profile.release.debug-assertions=true --test fuzz -- --nocapture
//! ```
//!
//! (`SKIMMER_FUZZ` = how many runs; each run is about 30 minutes of game
//! time, and as many computer-only matches.)

use game_kit::{Frame, Touch, button};
use skimmer::app::{App, Ctl, END_Y, Keys, Log, PadIn, Scene, TICK_US, TITLE_Y};
use skimmer::cpu;
use skimmer::draw;
use skimmer::fixed::len;
use skimmer::table::{
    CY, GOAL_HALF, HIT_R, Intent, Length, Level, MALLET_R, Match, OT_TICKS, PUCK_MAX, PUCK_R,
    Phase, Rules, STALL_TICKS, TIMED_TICKS, XL, XR, YB, YT, zone_x,
};

/// Every rally ends: a goal, or the stall rule. None may last this long
/// (3 minutes); the runs print the longest they saw.
const RALLY_MAX: u32 = 3 * 60 * 60;

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

/// What the runs found.
#[derive(Default, Debug)]
struct Seen {
    calls: u64,
    game_ticks: u64,
    matches: u32,
    finals: u32,
    draws: u32,
    goals: u32,
    stalls: u32,
    leaves: u32,
    squeezes: u64,
    longest_rally_ticks: u32,
    longest_half_ticks: u16,
}

fn runs() -> u64 {
    std::env::var("SKIMMER_FUZZ")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// One run of `calls` calls.
fn run(seed: u64, calls: u32, seen: &mut Seen) {
    let mut r = Rng(seed | 1);
    let mut app = App::new();
    let mut frame = Box::new(Frame::new());
    let mut pads = [false; 4];
    for p in &mut pads {
        *p = r.chance(500);
    }
    let mut held = [0u32; 4];
    let mut sticks = [(0i16, 0i16); 4];
    let busy: [u32; 4] = core::array::from_fn(|_| r.below(400));
    let mut finger: Option<(i16, i16)> = None;
    let mut frame_no = 0u32;
    let mut first = true;

    for _ in 0..calls {
        // Pads come and go.
        for (n, p) in pads.iter_mut().enumerate() {
            if r.chance(if n == 1 { 4 } else { 1 }) {
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
                let mut flip = r.next() & button::ALL;
                if !r.chance(30) {
                    flip &= !(button::START | button::SELECT);
                }
                held[n] ^= flip & (r.next() | r.next());
            }
            if r.chance(60) {
                // Anything, the far corners and dead centre too.
                sticks[n] = match r.below(4) {
                    0 => (0, 0),
                    1 => (i16::MIN, i16::MAX),
                    _ => (r.next() as i16, r.next() as i16),
                };
            }
            let quick = if r.chance(15) {
                r.next() & button::ALL & !button::START
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
        // The finger: comes down, drags, jumps, comes up; taps; now and
        // then far off the screen.
        k.touch = match finger {
            None if r.chance(40) => {
                let at = if r.chance(20) {
                    (r.next() as i16, r.next() as i16)
                } else {
                    (r.below(160) as i16, r.below(120) as i16)
                };
                if r.chance(300) {
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
                let (nx, ny) = if r.chance(30) {
                    (r.below(160) as i16, r.below(120) as i16)
                } else {
                    (
                        x.saturating_add(r.below(13) as i16 - 6),
                        y.saturating_add(r.below(13) as i16 - 6),
                    )
                };
                if r.chance(25) {
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
        // Calm the menus so matches get played; in play, the bar (pause)
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
            && (0..12).contains(&t.y)
            && !r.chance(50)
        {
            t.y += 12;
            if let Some(f) = &mut finger {
                f.1 = t.y;
            }
        }
        // Frame times: 30 or 60 a second, now and then a long or a huge
        // wait, the first call none.
        k.delta_us = if first {
            0
        } else {
            match r.below(100) {
                0 => r.next(),
                1..=3 => r.below(2_000_000),
                4..=40 => 16_667,
                41..=45 => r.below(60_000),
                _ => 33_333,
            }
        };
        first = false;

        let age_was = app.m.age;
        let going = app.update(&k);
        seen.calls += 1;
        frame_no = frame_no.wrapping_add(1);
        if app.scene == Scene::Play && app.m.age > age_was {
            seen.game_ticks += u64::from(app.m.age - age_was);
        }
        while let Some(l) = app.take_log() {
            match l {
                Log::Match { .. } => seen.matches += 1,
                Log::Goal { .. } => seen.goals += 1,
                Log::Stall { .. } => seen.stalls += 1,
                Log::Final { score, .. } => {
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
        draw::draw(&app, &mut frame);
        if app.scene != Scene::Title {
            check(&app, seed);
            seen.longest_rally_ticks = seen.longest_rally_ticks.max(app.m.rally);
            assert!(app.m.rally < RALLY_MAX, "seed {seed}: a rally of 3 minutes");
            seen.longest_half_ticks = seen.longest_half_ticks.max(app.m.half_ticks);
            seen.squeezes = seen.squeezes.max(u64::from(app.m.squeezes));
        }
    }
}

/// Everything on the table, score and clock sane.
fn check(app: &App, seed: u64) {
    let m = &app.m;
    check_match(m, &format!("seed {seed}"));
    // One person on one mallet at most.
    let mut used = [0u8; 5];
    for c in app.ctl.iter().take(m.mallets()) {
        match c {
            Ctl::Pad(n) => used[usize::from(*n)] += 1,
            Ctl::Touch => used[4] += 1,
            Ctl::Cpu => {}
        }
    }
    assert!(
        used.iter().all(|&n| n <= 1),
        "seed {seed}: one person on two mallets: {:?}",
        app.ctl
    );
}

fn check_match(m: &Match, at: &str) {
    let p = m.puck;
    if m.puck_shown() {
        assert!(
            p.y >= YT + PUCK_R && p.y <= YB - PUCK_R,
            "{at}: puck y {} off the table",
            p.y
        );
        if (p.y - CY).abs() >= GOAL_HALF {
            assert!(
                p.x >= XL + PUCK_R && p.x <= XR - PUCK_R,
                "{at}: puck in a rail at ({}, {})",
                p.x,
                p.y
            );
        } else {
            assert!(
                p.x >= XL - PUCK_R && p.x <= XR + PUCK_R,
                "{at}: puck through the slot at ({}, {})",
                p.x,
                p.y
            );
        }
        for i in 0..m.mallets() {
            let g = len(p.x - m.m[i].x, p.y - m.m[i].y);
            assert!(g >= HIT_R - 4, "{at}: puck inside mallet {i} ({g})");
        }
    }
    assert!(
        p.speed() <= PUCK_MAX + 2,
        "{at}: puck too fast {}",
        p.speed()
    );
    for i in 0..m.mallets() {
        let (x0, x1) = zone_x(i, m.rules.four);
        let mm = m.m[i];
        assert!(
            (x0..=x1).contains(&mm.x) && (YT + MALLET_R..=YB - MALLET_R).contains(&mm.y),
            "{at}: mallet {i} out of its zone at ({}, {})",
            mm.x,
            mm.y
        );
    }
    // Never stuck: the stall rule acts in time.
    assert!(
        m.half_ticks <= STALL_TICKS,
        "{at}: the puck stayed {} ticks on one half",
        m.half_ticks
    );
    if let Some(g) = m.rules.length.goals() {
        assert!(m.score.iter().all(|&s| s <= g), "{at}: past first to {g}");
        assert!(!m.overtime);
    } else {
        let cap = if m.overtime { OT_TICKS } else { TIMED_TICKS };
        assert!(m.clock <= cap, "{at}: clock {}", m.clock);
    }
    if m.phase == Phase::Over && m.score[0] == m.score[1] {
        assert!(
            m.rules.length == Length::Timed && m.overtime && m.clock == 0,
            "{at}: a draw before the golden goal ran out"
        );
    }
}

#[test]
fn random_play_never_breaks_the_game() {
    let n = runs();
    // Default: 3 short runs (about 5 minutes of game time each).
    let (n, calls) = if n == 0 { (3, 9_000) } else { (n, 54_000) };
    let mut seen = Seen::default();
    for s in 0..n {
        run(0x5EED_0000 + s * 0x9E37_79B9, calls, &mut seen);
    }
    println!(
        "fuzz: {n} runs, {} calls, {} min of match time, {seen:?}",
        seen.calls,
        seen.game_ticks * u64::from(TICK_US) / 60_000_000
    );
    assert!(seen.matches > 0 && seen.goals > 0 && seen.finals > 0);
}

/// Only computers (every level, both tables, 1 and 2 a side, every
/// length), many matches: every match ends, every rally ends, and the
/// checks above hold after every tick.
#[test]
fn computer_matches_always_end() {
    let n = runs().max(3) * 4;
    let mut r = Rng(0xC0FF_EE00);
    let (mut overtimes, mut draws, mut longest_rally, mut longest_match) = (0, 0, 0u32, 0u32);
    let mut goals = 0u32;
    for s in 0..n {
        let rules = Rules {
            length: Length::ALL[r.below(4) as usize],
            level: Level::ALL[r.below(3) as usize],
            four: r.chance(400),
            wild: r.chance(400),
        };
        let mut m = Match::new(rules, r.below(2) as u8, r.next());
        let mut ticks = 0u32;
        while m.phase != Phase::Over {
            let mut its = [Intent::Stay; 4];
            for (i, it) in its.iter_mut().enumerate().take(m.mallets()) {
                *it = cpu::intent(&mut m, i);
            }
            m.tick(&its);
            while m.take_event().is_some() {}
            ticks += 1;
            longest_rally = longest_rally.max(m.rally);
            assert!(m.rally < RALLY_MAX, "match {s}: a rally of 3 minutes");
            check_match(&m, &format!("match {s} {rules:?} tick {ticks}"));
            assert!(ticks < 60 * 60 * 30, "match {s}: half an hour");
        }
        goals += u32::from(m.score[0]) + u32::from(m.score[1]);
        longest_match = longest_match.max(ticks);
        if m.score[0] == m.score[1] {
            draws += 1;
        }
        if m.overtime {
            overtimes += 1;
        }
    }
    println!(
        "computer matches: {n}, goals {goals}, overtimes {overtimes}, draws {draws}, \
         longest rally {} s, longest match {} s",
        longest_rally / 60,
        longest_match / 60
    );
}
