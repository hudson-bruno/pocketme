//! Random keys for a long time on every track at both rates, with the
//! picture drawn every call. Nothing may panic (run it in release with
//! overflow checks and debug assertions on), no ship may leave the track
//! for good or get stuck, and the numbers stay in range. Part of the
//! time L and R are mashed with the stick swinging, or a finger is held
//! in the carve strip; the carve's numbers stay in range too, and the
//! same keys give the same race. Random keys seldom hold a carve through
//! a corner, so a third run lets the computer's pilot drive the player's
//! ship with random L / R holds and stick overrides on top: there slings
//! of every step happen, and the same seed again gives the same race.
//! The rivals carve too, under the same rules: their carve numbers are
//! checked the same way every tick, a rival's sling is never above its
//! class's let-go step, and the third run counts the rivals' slings at
//! every step. Every sling any ship begins has exactly the reward's
//! ticks for its step.
//!
//! `FUZZ_MIN` sets the game time per track and rate in minutes
//! (default 3; the long run is 60):
//!
//! ```text
//! CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true \
//! CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true \
//! FUZZ_MIN=60 cargo test --release --test fuzz -- --nocapture
//! ```

mod common;

use camber::app::{Keys, Scene};
use camber::race::{
    COUNT, Carve, Class, HEAT_3, LAPS, Mode, Phase, REWARD, Race, SHIELD_MAX, SHIPS, Ship,
};
use camber::scene::{Detail, ViewKind};
use camber::track::{FM, TRACKS};
use common::{CALL_30, CALL_60, Driver};
use game_kit::{Touch, button};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
}

#[derive(Default)]
struct Seen {
    races: u32,
    finishes: u32,
    falls: u32,
    pauses: u32,
    min_progress: i64,
}

fn run(track: u8, fast: bool, minutes: u64, seed: u64) -> Seen {
    let mut rng = Rng(seed | 1);
    let mut d = Driver::new();
    d.call_us = if fast { CALL_60 } else { CALL_30 };
    let calls = minutes * 60 * 1_000_000 / u64::from(d.call_us);
    let mut seen = Seen {
        min_progress: i64::MAX,
        ..Seen::default()
    };
    // Held keys change now and then.
    let mut held = 0u32;
    let mut stick = (0i16, 0i16);
    let mut hold_for = 0u32;
    // Mashing L and R with the stick swinging, or a finger in the strip.
    let mut mash = false;
    let mut strip: Option<i16> = None;
    // Progress checks: every ship over windows of 20 s of race.
    let mut last_prog = [i64::MIN; SHIPS];
    let mut last_tick = 0u32;
    let mut was_fall = [0u8; SHIPS];
    let mut off = [0u32; SHIPS];
    for _ in 0..calls {
        let app = &d.app;
        if app.scene == Scene::Title {
            // Start a race with random settings.
            d.app.opts.track = track;
            d.app.opts.class = Class::ALL[rng.below(3) as usize];
            d.app.opts.mode = if rng.below(5) == 0 {
                Mode::Trial
            } else {
                Mode::Race
            };
            d.app.opts.detail = Detail::ALL[rng.below(3) as usize];
            d.app.opts.view = ViewKind::ALL[rng.below(3) as usize];
            d.app.opts.fast = fast;
            d.app.sel = 0;
            d.hold(0, (0, 0));
            d.hold(button::A, (0, 0));
            d.hold(0, (0, 0));
            assert_eq!(d.app.scene, Scene::Race);
            seen.races += 1;
            last_prog = [i64::MIN; SHIPS];
            continue;
        }
        if hold_for == 0 {
            hold_for = 1 + rng.below(90);
            held = 0;
            if rng.below(4) != 0 {
                held |= button::A;
            }
            match rng.below(8) {
                0 => held |= button::L,
                1 => held |= button::R,
                2 => held |= button::B,
                3 => held |= button::LEFT,
                4 => held |= button::RIGHT,
                _ => {}
            }
            stick = match rng.below(4) {
                0 => (0, 0),
                1 => (-32_768, 0),
                2 => (32_767, 0),
                _ => (rng.next() as i16, rng.next() as i16),
            };
            mash = rng.below(4) == 0;
            strip = if rng.below(6) == 0 {
                Some(rng.below(160) as i16)
            } else {
                None
            };
        }
        hold_for -= 1;
        if mash {
            held = (held & !(button::L | button::R))
                | match rng.below(4) {
                    0 => button::L,
                    1 => button::R,
                    2 => button::L | button::R,
                    _ => 0,
                };
            if rng.below(3) == 0 {
                stick = (rng.next() as i16, 0);
            }
        }
        let mut k = Keys {
            held,
            pressed: 0,
            stick,
            trig: ((rng.below(4) * 340) as u16, (rng.below(4) * 340) as u16),
            touch: None,
            delta_us: 0,
        };
        // Now and then: a menu key, a touch, a pause.
        match rng.below(400) {
            0 if rng.below(5) == 0 => k.pressed = button::START,
            1 => k.pressed = button::X,
            2 => k.pressed = button::Y,
            3 => k.pressed = button::SELECT,
            4..=6 => {
                let (x, y) = (rng.below(160) as i16, rng.below(120) as i16);
                k.touch = Some(Touch {
                    x,
                    y,
                    down: true,
                    started: true,
                    ended: false,
                });
            }
            _ => {}
        }
        if let Some(x) = strip
            && k.touch.is_none()
        {
            // A finger held low on the screen, sliding about.
            let x = (i32::from(x) + rng.below(41) as i32 - 20).clamp(0, 159) as i16;
            strip = Some(x);
            k.touch = Some(Touch {
                x,
                y: camber::app::TOUCH_STRIP + 1 + rng.below(18) as i16,
                down: true,
                started: false,
                ended: false,
            });
        }
        if d.app.pause.is_some() {
            seen.pauses += 1;
            // In the pause menu: move about, sometimes pick a line (but
            // never LEAVE, which would end the run).
            k.pressed = match rng.below(12) {
                0 => button::UP,
                1 => button::DOWN,
                2 => button::LEFT,
                3 if d.app.pause != Some(6) && rng.below(3) == 0 => button::A,
                4 => button::START,
                _ => 0,
            };
            k.touch = None;
        }
        if d.app.results() {
            k.pressed = if rng.below(3) == 0 { button::A } else { 0 };
            k.touch = None;
        }
        d.held = held;
        let going = d.call(k);
        assert!(going, "the game ended: only LEAVE may do that");

        // Checks.
        let r = &d.app.race;
        let tr = r.tr();
        let len = tr.len();
        for i in 0..usize::from(r.n) {
            let sh = &r.ships[i];
            carve_in_range(sh, i, r.class);
            assert!((0..len).contains(&sh.s), "ship {i} s {}", sh.s);
            assert!((0..=SHIELD_MAX).contains(&sh.shield));
            assert!(sh.lap <= LAPS + 1);
            assert!(sh.h < FM * 60 && sh.h > -FM * 80, "ship {i} h {}", sh.h);
            let half = tr.at(sh.s).half * 256;
            if sh.fall == 0 {
                // Beside the road (in the air over an open edge) only for
                // a moment: it lands or falls and is put back.
                if sh.x.abs() > half + FM / 2 {
                    off[i] += 1;
                    assert!(
                        off[i] < 120,
                        "ship {i} off the track for 2 s: x {} half {} h {}",
                        sh.x,
                        half,
                        sh.h
                    );
                } else {
                    off[i] = 0;
                }
            } else {
                off[i] = 0;
                assert!(sh.fall <= 45, "ship {i} falls for good");
            }
            if sh.fall == 1 && was_fall[i] == 0 {
                seen.falls += 1;
            }
            was_fall[i] = sh.fall;
        }
        assert!((1..=r.n).contains(&r.place(0)));
        // Progress: every 20 s of race, every ship not finished has gone
        // some way.
        if r.phase != Phase::Count && r.tick >= last_tick + 1_200 {
            for (i, last) in last_prog.iter_mut().enumerate().take(usize::from(r.n)) {
                let sh = &r.ships[i];
                let p = sh.prog(len);
                if *last != i64::MIN && !sh.finished() {
                    let gone = p - *last;
                    if i == 0 {
                        seen.min_progress = seen.min_progress.min(gone);
                    }
                    // The rivals drive; the player's random keys brake,
                    // scrape and fall, but still get somewhere.
                    let least = if i == 0 { 60 } else { 300 };
                    assert!(
                        gone >= i64::from(FM) * least,
                        "ship {i} stuck: {} m in 20 s (track {track}, tick {})",
                        gone / i64::from(FM),
                        r.tick
                    );
                }
                *last = p;
            }
            last_tick = r.tick;
        } else if r.tick < last_tick {
            last_tick = r.tick;
            last_prog = [i64::MIN; SHIPS];
        }
        if r.ships[0].finished() && r.phase == Phase::Done && r.after == 1 {
            seen.finishes += 1;
        }
    }
    seen
}

/// The carve's numbers: a way or none, heat only while carving and never
/// above the top, a sling with its step and no longer than the reward
/// gives that step. A
/// rival (`i` above 0) lets go at its class's step at the latest, so its
/// sling is never of a higher step.
fn carve_in_range(sh: &Ship, i: usize, class: Class) {
    assert!((-1..=1).contains(&sh.carve), "ship {i} carve {}", sh.carve);
    if sh.carve == 0 {
        assert_eq!(sh.heat, 0, "ship {i}: heat without a carve");
    }
    assert!((0..=HEAT_3).contains(&sh.heat), "ship {i} heat {}", sh.heat);
    assert!(sh.sling_step <= 3);
    if sh.sling_step > 0 {
        let most = REWARD.ticks[usize::from(sh.sling_step - 1)];
        assert!(sh.sling <= most, "ship {i} sling {} over {most}", sh.sling);
    }
    assert_eq!(
        sh.sling == 0,
        sh.sling_step == 0,
        "ship {i} sling {} step {}",
        sh.sling,
        sh.sling_step
    );
    if i != 0 {
        assert!(
            sh.sling_step <= class.skill().step,
            "rival {i} in {} slung at step {}",
            class.name(),
            sh.sling_step
        );
    }
}

/// L and R mashed with the stick swinging, and the strip touched, from
/// one seed: every ship's every number after each call.
fn mashed(track: u8, seed: u64, calls: u32) -> Vec<[Ship; SHIPS]> {
    let mut rng = Rng(seed | 1);
    let mut d = Driver::new();
    d.draw = false;
    d.set(Detail::Low, ViewKind::Chase, true);
    d.race(track, Class::Brisk, Mode::Race);
    let mut out = Vec::new();
    for _ in 0..calls {
        let mut held = button::A;
        held |= match rng.below(5) {
            0 => button::L,
            1 => button::R,
            2 => button::L | button::R,
            _ => 0,
        };
        let mut k = Keys {
            held,
            pressed: 0,
            stick: (rng.next() as i16, 0),
            trig: (0, 0),
            touch: None,
            delta_us: 0,
        };
        if rng.below(10) == 0 {
            k.touch = Some(Touch {
                x: rng.below(160) as i16,
                y: 16 + rng.below(104) as i16,
                down: true,
                started: false,
                ended: false,
            });
        }
        d.held = held;
        d.call(k);
        for (i, sh) in d.app.race.ships.iter().enumerate() {
            carve_in_range(sh, i, d.app.race.class);
        }
        out.push(d.app.race.ships);
    }
    out
}

#[test]
fn mashing_l_and_r_with_the_stick_swinging_repeats_exactly() {
    for track in 0..TRACKS.len() as u8 {
        let a = mashed(track, 0xC4_0000 + u64::from(track), 60 * 40);
        let b = mashed(track, 0xC4_0000 + u64::from(track), 60 * 40);
        assert!(
            a == b,
            "{}: the same keys gave another race",
            TRACKS[usize::from(track)].name
        );
        let carved = a.iter().filter(|s| s[0].carve != 0).count();
        let slung = a.iter().filter(|s| s[0].sling != 0).count();
        eprintln!(
            "{}: carving {carved} calls, slinging {slung} of {}",
            TRACKS[usize::from(track)].name,
            a.len()
        );
        assert!(carved > 0);
    }
}

#[test]
fn random_keys_on_every_track_at_both_rates() {
    let minutes: u64 = std::env::var("FUZZ_MIN")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    for track in 0..TRACKS.len() as u8 {
        for fast in [false, true] {
            let s = run(
                track,
                fast,
                minutes,
                0x5EED_0000 + u64::from(track) * 7 + u64::from(fast),
            );
            eprintln!(
                "{} {}fps {} min: from the title {} finished {} falls {} paused calls {} least player progress {} m/20 s",
                TRACKS[usize::from(track)].name,
                if fast { 60 } else { 30 },
                minutes,
                s.races,
                s.finishes,
                s.falls,
                s.pauses,
                s.min_progress / i64::from(FM)
            );
            assert!(s.races >= 1);
        }
    }
}

/// The computer's pilot drives the player's ship through a whole race
/// (and 600 ticks past the line), with random carve holds (Off, Left,
/// Right, Both, Keep; held 1 to 90 ticks) and now and then the stick
/// forced full either way or flipped every tick, and the brakes. Every
/// ship's numbers are checked every tick; the player's ships after each
/// tick are returned with the slings started at each step, the player's
/// and the rivals'.
fn piloted(track: u8, class: Class, mode: Mode, seed: u64) -> (Vec<Ship>, [u32; 3], [u32; 3]) {
    let mut rng = Rng(seed | 1);
    let mut r = Race::new();
    r.setup(track, class, mode, seed as u32);
    let mut out = Vec::new();
    let mut slings = [0u32; 3];
    let mut rival_slings = [0u32; 3];
    let (mut carve, mut left) = (Carve::Off, 0u32);
    let (mut stick, mut stick_left) = (None::<i32>, 0u32);
    let mut after = 0;
    let mut tick = 0u32;
    while after < 600 && tick < 60 * 60 * 6 {
        tick += 1;
        if left == 0 {
            carve = match rng.below(6) {
                0 | 1 => Carve::Off,
                2 => Carve::Left,
                3 => Carve::Right,
                4 => Carve::Both,
                _ => Carve::Keep,
            };
            left = 1 + rng.below(90);
        }
        left -= 1;
        if stick_left == 0 {
            stick = match rng.below(8) {
                0 => Some(256),
                1 => Some(-256),
                2 => Some(i32::MAX),
                3 => Some(i32::MIN),
                _ => None,
            };
            stick_left = 1 + rng.below(40);
        }
        stick_left -= 1;
        let mut c = if tick > COUNT {
            r.drive_plain(0)
        } else {
            Default::default()
        };
        c.carve = carve;
        match stick {
            Some(i32::MAX) => c.steer = if tick.is_multiple_of(2) { 256 } else { -256 },
            Some(v) if v != i32::MIN => c.steer = v,
            _ => {}
        }
        if rng.below(200) == 0 {
            c.brake_l = true;
            c.brake_r = rng.below(2) == 0;
        }
        let was = r.ships[0].sling_step;
        let before = r.ships;
        r.step(Some(c));
        let sh = &r.ships[0];
        if sh.sling_step != 0
            && (was == 0 || sh.sling == REWARD.ticks[usize::from(sh.sling_step - 1)] - 1)
        {
            slings[usize::from(sh.sling_step - 1)] += 1;
        }
        for (i, sh) in r.ships.iter().enumerate() {
            carve_in_range(sh, i, r.class);
            assert!(sh.s >= 0 && sh.s < r.tr().len(), "ship {i} s {}", sh.s);
            // A sling begins: the reward's ticks for its step (counted down
            // once in the tick of the let-go); a rival's is counted.
            if sh.sling_step != 0 && sh.sling > before[i].sling {
                let k = usize::from(sh.sling_step - 1);
                assert_eq!(sh.sling, REWARD.ticks[k] - 1, "ship {i} step {k}");
                if i > 0 {
                    rival_slings[k] += 1;
                }
            }
        }
        out.push(r.ships[0]);
        if r.phase == Phase::Done || r.ships[0].lap > LAPS {
            after += 1;
        }
    }
    (out, slings, rival_slings)
}

#[test]
fn the_pilot_with_random_carves_reaches_every_sling_and_repeats_exactly() {
    // Four seeds a FUZZ_MIN minute (12 by default), each in every class.
    let seeds: u64 = 4 * std::env::var("FUZZ_MIN")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(3);
    let mut total = [0u32; 3];
    let mut rivals_total = [[0u32; 3]; 3];
    for track in 0..TRACKS.len() as u8 {
        let mut slings = [0u32; 3];
        let mut carving = 0usize;
        let mut ticks = 0usize;
        for seed in 0..seeds {
            for (k, class) in Class::ALL.into_iter().enumerate() {
                let mode = if (seed + k as u64).is_multiple_of(2) {
                    Mode::Race
                } else {
                    Mode::Trial
                };
                let sd = 0x5_1146 + u64::from(track) * 1_000 + seed * 10 + k as u64;
                let (a, sa, ra) = piloted(track, class, mode, sd);
                for j in 0..3 {
                    rivals_total[k][j] += ra[j];
                }
                if seed == 0 {
                    let (b, _, _) = piloted(track, class, mode, sd);
                    assert!(
                        a == b,
                        "{}: the same seed gave another race",
                        TRACKS[usize::from(track)].name
                    );
                }
                carving += a.iter().filter(|s| s.carve != 0).count();
                ticks += a.len();
                for j in 0..3 {
                    slings[j] += sa[j];
                }
            }
        }
        eprintln!(
            "{}: {} races, carving {carving} of {ticks} ticks, slings at steps 1/2/3: {}/{}/{}",
            TRACKS[usize::from(track)].name,
            seeds * 3,
            slings[0],
            slings[1],
            slings[2]
        );
        assert!(
            slings[0] > 0,
            "{}: no sling",
            TRACKS[usize::from(track)].name
        );
        for j in 0..3 {
            total[j] += slings[j];
        }
    }
    assert!(
        total.iter().all(|&n| n > 0),
        "every step's sling was reached: {total:?}"
    );
    // The rivals carve in every class, up to their class's step.
    eprintln!("rivals' slings at steps 1/2/3 by class (STEADY, BRISK, WILD): {rivals_total:?}");
    for (k, class) in Class::ALL.into_iter().enumerate() {
        let step = usize::from(class.skill().step);
        assert!(
            rivals_total[k][..step].iter().all(|&n| n > 0),
            "{}: rivals' slings {:?}",
            class.name(),
            rivals_total[k]
        );
    }
}
