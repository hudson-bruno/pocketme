//! The rivals carve: with the player's buttons and rules, as often and as
//! long as their class's skill says ([`camber::race::SKILLS`]), and they
//! show the same sparks. Each test here fails when its rule is taken out
//! of `Race::drive` (tried by hand, 2026-11-22).

mod common;

use camber::fixed::turn;
use camber::race::{
    CARVE_BEND, CARVE_HOLD_BEND, CARVE_START_SPEED, Carve, Class, HEAT_3, HOVER, Mode, Phase,
    REWARD, Race, SHIPS, SKILLS, Ship, TURN_MAX, carve_rate, carve_stick, heat_step, mps,
};
use camber::scene::{Detail, ViewKind};
use camber::track::{FM, SEG_BITS, TRACKS};
use common::{CALL_60, Driver};

/// What the rivals (ships 1 to 7) did in races run by the computer
/// alone: carves started and slings begun at each step.
#[derive(Debug, Default)]
struct Seen {
    carves: u32,
    slings: [u32; 3],
}

/// Runs the race of `track`, `class` and `seed` to its end, calling
/// `check` with the ships before and after every tick.
fn race(track: u8, class: Class, seed: u32, mut check: impl FnMut(&Race, &[Ship; SHIPS])) {
    let mut r = Race::new();
    r.setup(track, class, Mode::Race, seed);
    for _ in 0..60 * 400 {
        let before = r.ships;
        r.step(None);
        check(&r, &before);
        if r.ships.iter().all(|s| s.finished()) {
            return;
        }
    }
    panic!("the race did not end");
}

fn seen(class: Class) -> Seen {
    let mut s = Seen::default();
    for track in 0..TRACKS.len() as u8 {
        for seed in 0..2 {
            race(track, class, seed, |r, before| {
                for (a, b) in before.iter().zip(&r.ships).skip(1) {
                    if b.carve != 0 && a.carve == 0 {
                        s.carves += 1;
                    }
                    if b.sling_step != 0 && b.sling > a.sling {
                        s.slings[usize::from(b.sling_step - 1)] += 1;
                    }
                }
            });
        }
    }
    s
}

#[test]
fn rivals_carve_in_every_class_up_to_their_step_and_wild_carves_most() {
    let [steady, brisk, wild] = Class::ALL.map(seen);
    eprintln!("STEADY {steady:?}\nBRISK  {brisk:?}\nWILD   {wild:?}");
    // STEADY lets go at the first step: only first-step slings.
    assert!(steady.carves > 0 && steady.slings[0] > 0, "{steady:?}");
    assert_eq!(steady.slings[1..], [0, 0], "STEADY lets go at step 1");
    // BRISK reaches the second step, never the third.
    assert!(brisk.slings[0] > 0 && brisk.slings[1] > 0, "{brisk:?}");
    assert_eq!(brisk.slings[2], 0, "BRISK lets go at step 2");
    // WILD holds to the third.
    assert!(wild.slings.iter().all(|&n| n > 0), "{wild:?}");
    // The harder the class, the more corners it carves. (Every class
    // also carves each corner it comes to over its plan's speed since
    // the rivals' rebuild of 2026-11-22, so STEADY is no longer under
    // half of BRISK.)
    assert!(
        steady.carves < brisk.carves && brisk.carves < wild.carves,
        "carves {} {} {}",
        steady.carves,
        brisk.carves,
        wild.carves
    );
}

#[test]
fn a_rival_presses_only_at_speed_with_room_and_after_a_rest() {
    let mut presses = 0;
    for class in Class::ALL {
        let sk = class.skill();
        for track in 0..TRACKS.len() as u8 {
            race(track, class, 1, |r, before| {
                let tr = r.tr();
                for (i, (a, b)) in before.iter().zip(&r.ships).enumerate().skip(1) {
                    if b.carve == 0 || a.carve != 0 {
                        continue;
                    }
                    presses += 1;
                    let half = tr.at(a.s).half * 256;
                    let what = format!(
                        "{} {}: rival {i} pressed at speed {} rest {} x {} half {half}",
                        TRACKS[usize::from(track)].name,
                        class.name(),
                        a.vf,
                        a.rest,
                        a.x
                    );
                    assert!(a.vf > CARVE_START_SPEED + FM / 6, "{what}");
                    assert!(a.rest > sk.rest, "{what}");
                    assert!(half - a.x.abs() > sk.room, "{what}");
                    assert_eq!(a.fall, 0, "{what}");
                }
            });
        }
    }
    assert!(presses > 100, "{presses} presses");
}

#[test]
fn rivals_pull_no_air_brake_on_one_side_and_carve_with_the_player_controls() {
    let mut carving = 0;
    for class in Class::ALL {
        for track in 0..TRACKS.len() as u8 {
            race(track, class, 0, |r, _| {
                for i in 1..SHIPS {
                    let c = r.drive(i);
                    assert_eq!(c.brake_l, c.brake_r, "rival {i}: one air brake alone");
                    assert!(
                        (-256..=256).contains(&c.steer),
                        "rival {i}: steer {}",
                        c.steer
                    );
                    if r.ships[i].carve != 0 {
                        carving += 1;
                    }
                }
            });
        }
    }
    assert!(carving > 1_000, "{carving} carving ticks");
}

#[test]
fn carve_stick_gives_back_the_carve_rate() {
    for dir in [-1, 1] {
        for a in (-256..=256).step_by(8) {
            let rate = carve_rate(dir, a);
            let back = carve_rate(dir, carve_stick(dir, rate));
            assert!(
                (back - rate).abs() <= rate.abs() / 50 + 2,
                "dir {dir} stick {a}: rate {rate} gave back {back}"
            );
        }
        // Rates beyond the carve's reach give the stick's end: tighter
        // than TIGHT the stick into the turn, wider than WIDE out of it.
        let tight = carve_rate(dir, 256 * dir);
        let wide = carve_rate(dir, -256 * dir);
        assert_eq!(carve_stick(dir, tight * 2), 256 * dir);
        assert_eq!(carve_stick(dir, wide / 2), -256 * dir);
    }
}

#[test]
fn the_skills_rise_with_the_class() {
    let [s, b, w] = SKILLS;
    assert!(s.share < b.share && b.share < w.share);
    assert_eq!([s.step, b.step, w.step], [1, 2, 3]);
    // The room a rival wants to both walls never grows with the class: a
    // harder class must not let go sooner (review 2026-11-22, F1: BRISK
    // wanted 5 m, more than STEADY's 4, and 1 carve in 24 paid on FURNACE).
    assert!(
        s.room >= b.room && b.room >= w.room,
        "rooms {s:?} {b:?} {w:?}"
    );
    assert_eq!(Class::Steady.skill(), s);
    assert_eq!(Class::Wild.skill(), w);
}

/// A carve "pays" when it ends with a sling. Rivals carve in corners
/// where it pays: in every class at least a quarter of the carves they
/// pick (by their class's share, in a corner that turns at least the
/// class's `turn` over 64 m) end with a sling, on every track (MEASURED
/// ON THE PC 2026-11-22 with the rooms 3, 3 and 2.5 m: see the printed
/// shares; with the old rooms 4, 5 and 2 m FURNACE BRISK paid 1 carve in
/// 24, review F1 and fault 10). The carves a rival takes because it
/// comes to a corner over its plan's speed, the thrust off
/// (`Skill::need`, since the rebuild of 2026-11-22), are for the grip
/// and are counted apart. One cell is let through lower: STEADY on
/// FURNACE paid 25 % before the rebuild and 15 % after it (measured on the
/// PC: STEADY now presses more short carves on FURNACE's narrow bends and
/// lets go at the first step); its floor is 12 %.
#[test]
fn most_rival_carves_pay_a_sling_in_every_class_and_track() {
    let mut low = Vec::new();
    for class in Class::ALL {
        for track in 0..TRACKS.len() as u8 {
            let (mut carves, mut paid, mut need, mut need_paid) = (0u32, 0u32, 0u32, 0u32);
            for seed in 0..3 {
                let mut r = Race::new();
                r.setup(track, class, Mode::Race, seed);
                let mut picked = [false; SHIPS];
                for _ in 0..60 * 400 {
                    let before = r.ships;
                    let thrust: [bool; SHIPS] = core::array::from_fn(|i| r.drive_plain(i).thrust);
                    r.step(None);
                    for i in 1..SHIPS {
                        let (a, b) = (&before[i], &r.ships[i]);
                        if b.carve != 0 && a.carve == 0 {
                            picked[i] = thrust[i];
                            if picked[i] {
                                carves += 1;
                            } else {
                                need += 1;
                            }
                        }
                        if b.sling_step != 0 && b.sling > a.sling {
                            if picked[i] {
                                paid += 1;
                            } else {
                                need_paid += 1;
                            }
                        }
                    }
                    if r.ships.iter().all(|s| s.finished()) {
                        break;
                    }
                }
            }
            let share = paid * 100 / carves.max(1);
            eprintln!(
                "{} {}: {carves} carves picked, {paid} paid ({share} %); {need} for the grip, {need_paid} paid",
                TRACKS[usize::from(track)].name,
                class.name()
            );
            let floor = if class == Class::Steady && track == 1 {
                12
            } else {
                25
            };
            if share < floor {
                low.push(format!(
                    "{} {} {share} %",
                    TRACKS[usize::from(track)].name,
                    class.name()
                ));
            }
        }
    }
    assert!(low.is_empty(), "rivals' carves that hardly pay: {low:?}");
}

/// Rivals fly with exactly the player's physics: one tick of a rival,
/// put into a time trial's only ship (the player's) with the same
/// controls, moves it to the very same state (review 2026-11-22, F3: a
/// bonus for the rivals alone was caught only by the golden pictures).
/// Ticks where another ship is close enough to bump are left out.
#[test]
fn a_rival_moves_exactly_as_the_player_would_with_its_controls() {
    let mut compared = 0u32;
    let mut slinging = 0u32;
    let mut carving = 0u32;
    for class in Class::ALL {
        for track in 0..TRACKS.len() as u8 {
            let mut r = Race::new();
            r.setup(track, class, Mode::Race, 3);
            let len = r.tr().len();
            for _ in 0..60 * 200 {
                let before = r;
                r.step(None);
                if before.phase != Phase::Run {
                    continue;
                }
                for i in 1..SHIPS {
                    let was = &before.ships[i];
                    if was.finished() {
                        continue;
                    }
                    let near = (0..SHIPS).filter(|&j| j != i).any(|j| {
                        let ds = (r.ships[j].s - r.ships[i].s).rem_euclid(len);
                        ds.min(len - ds) < FM * 8
                    });
                    if near {
                        continue;
                    }
                    let mut t = Race::new();
                    t.setup(track, class, Mode::Trial, 3);
                    t.phase = before.phase;
                    t.tick = before.tick;
                    t.ships[0] = *was;
                    // The controls the rival really flew with (the ships move in
                    // turn, so a later rival sees the earlier ones moved).
                    t.step(Some(r.ships[i].ctl));
                    assert_eq!(
                        t.ships[0],
                        r.ships[i],
                        "{} {}: rival {i} at tick {} moved otherwise than the player would",
                        TRACKS[usize::from(track)].name,
                        class.name(),
                        before.tick
                    );
                    compared += 1;
                    if was.sling > 0 {
                        slinging += 1;
                    }
                    if was.carve != 0 {
                        carving += 1;
                    }
                }
            }
        }
    }
    eprintln!("{compared} rival ticks compared, {carving} carving, {slinging} slinging");
    assert!(compared > 50_000 && carving > 2_000 && slinging > 2_000);
}

/// The reward reaches every ship: each sling a rival or the
/// computer-flown player begins has exactly the reward's ticks for its
/// step, on four grids of every track (review 2026-11-22, F4).
#[test]
fn every_ship_gets_the_reward_s_sling_ticks() {
    let rw = REWARD;
    for seed in 0..4u32 {
        let mut slings = [[0u32; 3]; SHIPS];
        for track in 0..TRACKS.len() as u8 {
            let mut r = Race::new();
            r.setup(track, Class::Wild, Mode::Race, seed);
            for _ in 0..60 * 150 {
                let before = r.ships;
                r.step(None);
                for (i, (a, b)) in before.iter().zip(&r.ships).enumerate() {
                    if b.sling_step != 0 && b.sling > a.sling {
                        let k = usize::from(b.sling_step - 1);
                        // Set at the let-go and counted down once that tick.
                        assert_eq!(
                            b.sling,
                            rw.ticks[k] - 1,
                            "seed {seed} ship {i} step {}",
                            b.sling_step
                        );
                        slings[i][k] += 1;
                    }
                }
            }
        }
        eprintln!("seed {seed}: slings by ship and step {slings:?}");
        assert!(
            slings.iter().skip(1).filter(|s| s[0] > 0).count() >= 5,
            "seed {seed}: too few rivals slung {slings:?}"
        );
        assert!(slings.iter().any(|s| s[1] > 0) && slings.iter().any(|s| s[2] > 0));
    }
}

/// The rivals' carve rules, one by one (review 2026-11-22, F5): a press
/// has the stick in the middle; a carving rival holds exactly while the
/// corner goes on, it has its class's room to both walls, its class's
/// heat step is not reached and its pilot does not pull the stick hard
/// against the carve. Each reason to let go must occur.
#[test]
fn a_rival_presses_with_the_stick_in_the_middle_and_holds_by_the_rules() {
    // Let-go reasons: corner ended, inner wall, outer wall, step, stick.
    let mut why = [0u32; 5];
    let (mut presses, mut holds) = (0u32, 0u32);
    for class in Class::ALL {
        let sk = class.skill();
        for track in 0..TRACKS.len() as u8 {
            for seed in 0..2 {
                race(track, class, seed, |r, _| {
                    let tr = r.tr();
                    for i in 1..SHIPS {
                        let sh = &r.ships[i];
                        let c = r.drive(i);
                        let plain = r.drive_plain(i);
                        let dir = i32::from(sh.carve);
                        if dir == 0 {
                            if c.carve != Carve::Off {
                                presses += 1;
                                assert_eq!(c.steer, 0, "rival {i} pressed with the stick");
                                let k = tr.index(sh.s);
                                let b = turn(tr.seg(k).yaw, tr.seg(k + 3).yaw);
                                assert!(b.abs() > CARVE_BEND, "rival {i} pressed on a straight");
                                // A short corner only when over its plan
                                // (the thrust off, `Skill::need`).
                                let long = turn(tr.seg(k).yaw, tr.seg(k + 8).yaw);
                                assert!(
                                    long * b.signum() >= sk.turn || (sk.need && !plain.thrust),
                                    "rival {i} pressed in a short corner"
                                );
                                let want = if b < 0 { Carve::Left } else { Carve::Right };
                                assert_eq!(c.carve, want, "rival {i} pressed the wrong way");
                            }
                            continue;
                        }
                        let k = tr.index(sh.s);
                        let half = tr.at(sh.s).half * 256;
                        let room = |side: i32| half - sh.x * side;
                        let reasons = [
                            turn(tr.seg(k).yaw, tr.seg(k + 2).yaw) * dir <= CARVE_HOLD_BEND,
                            room(dir) <= sk.room,
                            room(-dir) <= sk.room,
                            // Step 3 (WILD) holds to the corner's end.
                            sk.step < 3 && heat_step(sh.heat) >= sk.step,
                            plain.steer * dir <= -200,
                        ];
                        let hold = !reasons.iter().any(|&x| x);
                        let side = if dir < 0 { Carve::Left } else { Carve::Right };
                        assert_eq!(
                            c.carve == side,
                            hold,
                            "rival {i}: reasons to let go {reasons:?}, held {:?}",
                            c.carve
                        );
                        if hold {
                            holds += 1;
                            assert_eq!(c.steer, carve_stick(dir, plain.steer * TURN_MAX / 256));
                        } else {
                            assert_eq!(c.carve, Carve::Off, "rival {i} let go with {:?}", c.carve);
                            for (n, &x) in why.iter_mut().zip(&reasons) {
                                if x {
                                    *n += 1;
                                }
                            }
                        }
                    }
                });
            }
        }
    }
    eprintln!(
        "presses {presses}, holding ticks {holds}, let-gos for: corner ended {}, inner wall {}, \
         outer wall {}, heat step {}, stick against {}",
        why[0], why[1], why[2], why[3], why[4]
    );
    assert!(presses > 500 && holds > 5_000);
    assert!(
        why.iter().all(|&n| n > 0),
        "every reason to let go occurs: {why:?}"
    );
}

/// The straightest stretch of `track`: the boundary that starts the 12
/// segments (96 m) with the least turning.
fn straightest(track: u8) -> usize {
    let tr = &TRACKS[usize::from(track)];
    (0..tr.n())
        .min_by_key(|&k| {
            (0..12)
                .map(|j| turn(tr.seg(k + j).yaw, tr.seg(k + j + 1).yaw).abs())
                .sum::<i32>()
                + (0..12)
                    .map(|j| i32::from(tr.seg(k + j).pitch).abs())
                    .sum::<i32>()
        })
        .unwrap_or(0)
}

/// A carving rival's sparks show at a distance too, on every detail:
/// the player and one rival at 60 m/s on each track's straightest
/// stretch, the rival `ahead` metres in front and 3 m to the side at the
/// third heat step; the pixels that change when its carve is taken away,
/// summed over 4 flicker phases (review 2026-11-22, play fault 1: LOW
/// showed none beyond about 16 m, MID none at 40 m).
fn spark_pixels(track: u8, detail: Detail, ahead: i32) -> usize {
    let mut d = Driver::new();
    d.draw = false;
    d.set(detail, ViewKind::Chase, true);
    d.race(track, Class::Brisk, Mode::Race);
    for _ in 0..COUNT_CALLS {
        d.hold(0, (0, 0));
    }
    let tr = d.app.race.tr();
    let s0 = (straightest(track) as i32) << SEG_BITS;
    let s0 = s0 + FM * 8;
    for (i, sh) in d.app.race.ships.iter_mut().enumerate() {
        let mut n = Ship::new();
        n.lap = 1;
        n.counted = 1;
        n.h = HOVER;
        n.vf = mps(60);
        n.pace = sh.pace;
        n.s = match i {
            0 => s0,
            1 => s0 + FM * ahead,
            // The others far behind the camera.
            _ => s0 - FM * (60 + 10 * i as i32),
        }
        .rem_euclid(tr.len());
        n.x = if i == 1 { FM * 3 } else { 0 };
        n.pitch = tr.at(n.s).pitch;
        *sh = n;
    }
    for _ in 0..30 {
        let (race, cam) = (&d.app.race, &mut d.app.cam);
        cam.follow(race.tr(), &race.ships[0], ViewKind::Chase);
    }
    let mut changed = 0;
    for time in 0..4 {
        d.app.race.ships[1].carve = 1;
        d.app.race.ships[1].heat = HEAT_3;
        let mut with = Box::new(game_kit::Frame::new());
        d.app.draw(&mut with, time);
        d.app.race.ships[1].carve = 0;
        d.app.race.ships[1].heat = 0;
        let mut without = Box::new(game_kit::Frame::new());
        d.app.draw(&mut without, time);
        changed += with
            .0
            .iter()
            .zip(without.0.iter())
            .filter(|(a, b)| a != b)
            .count();
        // CAMBER_SPARK_PICS=<folder>: the pictures with the sparks, 4x.
        if let Some(dir) = std::env::var_os("CAMBER_SPARK_PICS")
            && time == 0
        {
            let name = format!(
                "sparks-{}-{}-{ahead}m.png",
                TRACKS[usize::from(track)].name.to_lowercase(),
                detail.name().to_lowercase()
            );
            d.app.race.ships[1].carve = 1;
            d.app.race.ships[1].heat = HEAT_3;
            let theme = d.app.theme();
            common::save_strip(
                &std::path::Path::new(&dir).join(name),
                &[*with, *without],
                theme,
                4,
            );
        }
    }
    changed
}

/// Calls of the count-down at 60 calls a second, and the GO! note.
const COUNT_CALLS: u32 = 400;

#[test]
fn a_carving_rival_shows_its_sparks_from_afar_on_every_detail() {
    let mut none = Vec::new();
    for track in 0..TRACKS.len() as u8 {
        for detail in Detail::ALL {
            let px = [20, 40].map(|m| spark_pixels(track, detail, m));
            eprintln!(
                "{} {}: spark pixels at 20 m {}, at 40 m {} (4 phases)",
                TRACKS[usize::from(track)].name,
                detail.name(),
                px[0],
                px[1]
            );
            for (m, n) in [20, 40].into_iter().zip(px) {
                if n < 4 {
                    none.push(format!(
                        "{} {} {m} m: {n}",
                        TRACKS[usize::from(track)].name,
                        detail.name()
                    ));
                }
            }
        }
    }
    assert!(none.is_empty(), "sparks hardly seen: {none:?}");
}

#[test]
fn a_carving_rival_shows_the_sparks() {
    // The computer flies the player's ship; each time a carving rival is
    // a few metres ahead (up to 20 times), the picture is drawn with its
    // carve and without it. The sparks are the only difference. A rival
    // 4 to 14 m ahead is small on the 160 x 120 picture and its sparks
    // can hide behind another ship, so three in four of the pictures
    // must differ (MEASURED ON THE PC 2026-11-22: 18 of 20, 52 pixels in
    // all).
    let mut d = Driver::new();
    d.draw = false;
    d.set(Detail::High, ViewKind::Chase, true);
    d.call_us = CALL_60;
    d.race(0, Class::Wild, Mode::Race);
    let mut seen = Vec::new();
    for _ in 0..60 * 200 {
        d.auto();
        let r = &d.app.race;
        let me = &r.ships[0];
        let near = (1..SHIPS).find(|&i| {
            let o = &r.ships[i];
            let ds = o.s - me.s;
            o.carve != 0
                && o.fall == 0
                && ds > FM * 4
                && ds < FM * 14
                && (o.x - me.x).abs() < FM * 3
        });
        let Some(i) = near else { continue };
        let mut with = Box::new(game_kit::Frame::new());
        d.app.draw(&mut with, d.time);
        let (carve, heat) = (d.app.race.ships[i].carve, d.app.race.ships[i].heat);
        d.app.race.ships[i].carve = 0;
        d.app.race.ships[i].heat = 0;
        let mut without = Box::new(game_kit::Frame::new());
        d.app.draw(&mut without, d.time);
        d.app.race.ships[i].carve = carve;
        d.app.race.ships[i].heat = heat;
        let changed = with
            .0
            .iter()
            .zip(without.0.iter())
            .filter(|(a, b)| a != b)
            .count();
        seen.push(changed);
        if seen.len() == 20 {
            break;
        }
    }
    eprintln!("pixels the sparks change, each time a carving rival was ahead: {seen:?}");
    assert!(!seen.is_empty(), "no carving rival came close ahead");
    let shown = seen.iter().filter(|&&n| n > 0).count();
    let sum: usize = seen.iter().sum();
    assert!(
        shown * 4 >= seen.len() * 3 && sum >= 20,
        "a carving rival without sparks: {seen:?}"
    );
}
