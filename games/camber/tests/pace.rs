//! The rivals' fair play and the catch-up (the rebuild of 2026-11-22):
//! no rival is faster than the player's ship can be, the catch-up stays
//! inside its written size and works both ways, the same race runs the
//! same every time, the classes and the front and the pack come in
//! order, the player's ship eases round after its finish, and over every
//! track, class, grid order and catch-up no rival sticks, rides a wall
//! or leaves the road.

use camber::race::{
    CATCH_FREE, CATCH_FULL, CATCH_MAX, CatchUp, Class, Ev, Mode, PACE, Phase, REWARD, Race,
    SHIP_HW, SHIPS, mps,
};
use camber::track::{FM, TRACKS};

const SEEDS: u32 = 7;

fn new_race(track: u8, class: Class, seed: u32, catchup: CatchUp) -> Race {
    let mut r = Race::new();
    r.setup(track, class, Mode::Race, seed);
    r.catchup = catchup;
    r
}

/// Runs a race of the computer alone (ship 0 by the rivals' pilot) until
/// every ship has finished and `extra` ticks more, calling `each` after
/// every tick with the race and the ships before it.
fn run(r: &mut Race, extra: u32, mut each: impl FnMut(&mut Race, &[camber::race::Ship; SHIPS])) {
    let mut done = None;
    for _ in 0..60 * 400 {
        let before = r.ships;
        r.step(None);
        each(r, &before);
        if done.is_none() && r.ships.iter().all(|s| s.finished()) {
            done = Some(r.tick);
        }
        if done.is_some_and(|t| r.tick >= t + extra) {
            return;
        }
    }
    panic!("track {} {:?}: the race did not end", r.track, r.class);
}

/// The fastest a ship of `pace` can go along its nose in `class` on the
/// flat: the top of the reward's highest sling (a boost pad is less).
fn ceiling(class: Class, pace: i32) -> i32 {
    let top = class.top() * pace / 1024;
    top * REWARD.top[2] / 1024
}

#[test]
fn no_rival_is_given_more_than_the_player_s_ship() {
    // Every grid order: no rival's own pace is above the player's.
    for seed in 0..SEEDS {
        let r = new_race(0, Class::Wild, seed, CatchUp::Off);
        assert_eq!(r.ships[0].pace, 1024);
        let mut paces: Vec<i16> = r.ships[1..].iter().map(|s| s.pace).collect();
        paces.sort_unstable();
        let mut want = PACE.to_vec();
        assert!(
            want.iter().filter(|&&p| p == 1024).count() >= 2,
            "two or three fronts"
        );
        want.sort_unstable();
        assert_eq!(paces, want, "seed {seed}");
        assert!(r.ships.iter().all(|s| s.pace <= 1024), "seed {seed}");
    }
    // In the races, no rival goes faster than its own ship's ceiling
    // (with the catch-up's most) plus what a slope can give, and none
    // jumps along or across the road.
    let mut worst = 0i64;
    for track in 0..TRACKS.len() as u8 {
        let len = TRACKS[usize::from(track)].len();
        for class in Class::ALL {
            for catchup in [CatchUp::Off, CatchUp::Light] {
                for seed in [0, 3] {
                    let mut r = new_race(track, class, seed, catchup);
                    run(&mut r, 0, |r, before| {
                        for (i, (a, b)) in before.iter().zip(&r.ships).enumerate().skip(1) {
                            if b.fall > 0 || a.fall > 0 || b.finished() {
                                continue;
                            }
                            let most = i32::from(b.pace) * (1024 + CATCH_MAX) / 1024;
                            let lim = ceiling(class, most);
                            let v = b.vf.abs().max(b.vl.abs());
                            worst = worst.max(i64::from(v) * 1000 / i64::from(lim));
                            assert!(
                                v <= lim + lim / 10,
                                "track {track} {class:?} seed {seed} ship {i}: {v} over {lim}"
                            );
                            let mut ds = b.s - a.s;
                            if ds > len / 2 {
                                ds -= len;
                            } else if ds < -len / 2 {
                                ds += len;
                            }
                            let step = ds.abs() + (b.x - a.x).abs();
                            assert!(
                                step <= lim * 2,
                                "track {track} {class:?} seed {seed} ship {i}: moved {step} in a tick"
                            );
                        }
                    });
                }
            }
        }
    }
    eprintln!(
        "fastest rival: {}.{} % of its ceiling",
        worst / 10,
        worst % 10
    );
}

#[test]
fn the_catch_up_stays_inside_its_size_and_works_both_ways() {
    let (mut faster, mut slower) = (0u32, 0u32);
    for track in 0..TRACKS.len() as u8 {
        for class in Class::ALL {
            for seed in [1, 5] {
                let mut light = new_race(track, class, seed, CatchUp::Light);
                run(&mut light, 60, |r, _| {
                    let me = &r.ships[0];
                    let len = r.tr().len();
                    for i in 1..SHIPS {
                        let sh = &r.ships[i];
                        let p = i32::from(sh.pace);
                        let now = r.pace_of(i);
                        if r.phase != Phase::Run || sh.finished() || me.finished() {
                            assert_eq!(now, p, "the catch-up outside the race");
                            continue;
                        }
                        let d = me.prog(len) - sh.prog(len);
                        assert!(
                            (now - p).abs() <= p * CATCH_MAX / 1024,
                            "pace {now} of {p}: more than the catch-up's most"
                        );
                        if d.abs() <= i64::from(CATCH_FREE) {
                            assert_eq!(now, p, "the catch-up within {CATCH_FREE} of the player");
                        }
                        if d.abs() >= i64::from(CATCH_FULL) {
                            assert_eq!((now - p).abs(), p * CATCH_MAX / 1024, "full at a distance");
                        }
                        if now > p {
                            assert!(d > 0, "a rival ahead of the player is helped");
                            faster += 1;
                        }
                        if now < p {
                            assert!(d < 0, "a rival behind the player is held back");
                            slower += 1;
                        }
                    }
                });
                let mut off = new_race(track, class, seed, CatchUp::Off);
                run(&mut off, 0, |r, _| {
                    for i in 1..SHIPS {
                        assert_eq!(r.pace_of(i), i32::from(r.ships[i].pace));
                    }
                });
            }
        }
    }
    eprintln!("ticks helped {faster}, held back {slower}");
    assert!(
        faster > 0 && slower > 0,
        "both ways: helped {faster}, held back {slower}"
    );
}

#[test]
fn the_same_race_runs_the_same_every_time() {
    for track in 0..TRACKS.len() as u8 {
        for class in Class::ALL {
            for catchup in [CatchUp::Off, CatchUp::Light] {
                let mut a = new_race(track, class, 4, catchup);
                let mut b = new_race(track, class, 4, catchup);
                let mut seen = Vec::new();
                run(&mut a, 120, |r, _| seen.push(r.ships));
                let mut k = 0;
                run(&mut b, 120, |r, _| {
                    assert_eq!(r.ships, seen[k], "track {track} {class:?} tick {}", r.tick);
                    k += 1;
                });
                assert_eq!(a, b);
            }
        }
    }
}

/// The finish times (ticks) of the fastest front ship and the mean of the
/// fronts and of the pack, over the grid orders, rivals only (ship 0 held
/// out on the grid), the catch-up OFF.
fn times(track: u8, class: Class) -> (u64, u64, u64) {
    let (mut lead, mut front, mut pack) = (0u64, 0u64, 0u64);
    for seed in 0..SEEDS {
        let mut r = new_race(track, class, seed, CatchUp::Off);
        for _ in 0..60 * 400 {
            let p = &mut r.ships[0];
            p.fall = 1;
            p.vf = 0;
            p.vl = 0;
            p.vh = 0;
            r.step(None);
            if (1..SHIPS).all(|i| r.ships[i].finished()) {
                break;
            }
        }
        let fronts: Vec<u64> = (1..SHIPS)
            .filter(|&i| r.ships[i].pace == 1024)
            .map(|i| u64::from(r.ships[i].finish))
            .collect();
        let packs: Vec<u64> = (1..SHIPS)
            .filter(|&i| r.ships[i].pace < 1024)
            .map(|i| u64::from(r.ships[i].finish))
            .collect();
        assert!(
            fronts.iter().chain(&packs).all(|&f| f > 0),
            "a rival did not finish"
        );
        lead += fronts.iter().min().unwrap();
        front += fronts.iter().sum::<u64>() / fronts.len() as u64;
        pack += packs.iter().sum::<u64>() / packs.len() as u64;
    }
    let n = u64::from(SEEDS);
    (lead / n, front / n, pack / n)
}

#[test]
fn the_classes_and_the_front_and_the_pack_come_in_order() {
    for track in 0..TRACKS.len() as u8 {
        let t = Class::ALL.map(|c| times(track, c));
        for (c, (lead, front, pack)) in Class::ALL.iter().zip(t) {
            eprintln!(
                "{} {}: leader {lead}, fronts {front}, pack {pack} (+{}.{} %)",
                TRACKS[usize::from(track)].name,
                c.name(),
                (pack - front) * 100 / front,
                (pack - front) * 1000 / front % 10
            );
            // The pack's mean is behind the fronts' mean by at least 3 %.
            assert!(
                pack * 100 >= front * 103,
                "{c:?}: pack {pack}, fronts {front}"
            );
        }
        assert!(
            t[0].0 > t[1].0 && t[1].0 > t[2].0,
            "track {track}: leaders {t:?}"
        );
    }
}

#[test]
fn the_player_s_ship_eases_round_after_its_finish() {
    for track in 0..TRACKS.len() as u8 {
        let tr = &TRACKS[usize::from(track)];
        for (class, seed) in Class::ALL
            .into_iter()
            .flat_map(|c| (0..SEEDS).map(move |s| (c, s)))
        {
            let mut r = new_race(track, class, seed, CatchUp::Off);
            let mut after = 0u32;
            let mut flips = 0u32;
            let mut quick = 0u32;
            let mut flip_at = 0u32;
            let mut last = 0i32;
            let mut walls = 0u32;
            let mut early = 0u32;
            let mut slow = i32::MAX;
            for _ in 0..60 * 400 {
                let was = r.ships[0].vf;
                r.step(None);
                while let Some(e) = r.take_ev() {
                    if r.ships[0].finished() && matches!(e, Ev::Wall { ship: 0 }) {
                        // In its first second the ship still has the
                        // race's slide (it may cross the line in a carve).
                        if after < 60 {
                            early += 1;
                        } else {
                            walls += 1;
                        }
                    }
                }
                let sh = &r.ships[0];
                if !sh.finished() {
                    continue;
                }
                after += 1;
                let st = sh.ctl.steer;
                if st.abs() >= 64 {
                    if st.signum() != last.signum() && last != 0 {
                        flips += 1;
                        if flip_at > 0 && after - flip_at < 30 {
                            quick += 1;
                        }
                        flip_at = after;
                    }
                    last = st;
                }
                if after == 1 {
                    // The finish tick was still driven by the race's pilot.
                    continue;
                }
                assert!(!sh.ctl.carve.pressed(), "no carve after the finish");
                assert_eq!(sh.ctl.brake_l, sh.ctl.brake_r, "no air brake alone");
                if sh.ctl.brake_l {
                    assert!(was > class.top() * 3 / 4, "braking at {was}");
                }
                if after > 60 * 3 && !sh.ctl.thrust {
                    slow = slow.min(sh.vf);
                }
                assert!(sh.fall == 0, "{} {class:?}: fell after the finish", tr.name);
                if after == 60 * 20 {
                    break;
                }
            }
            assert_eq!(
                after,
                60 * 20,
                "{} {class:?}: ship 0 did not finish",
                tr.name
            );
            eprintln!(
                "{} {}: steer flips {flips} (within half a second {quick}), walls {early}+{walls}",
                tr.name,
                class.name()
            );
            assert!(
                early <= 1,
                "{} {class:?}: {early} walls in the first second",
                tr.name
            );
            // A turn of the stick to the other side is a new corner; one
            // within half a second of the last is a swing.
            assert!(
                quick <= 2,
                "{} {class:?}: the stick swung {quick} times in 20 s",
                tr.name
            );
            assert!(
                walls == 0,
                "{} {class:?}: {walls} walls after the finish",
                tr.name
            );
            assert!(
                slow >= class.top() / 2 - mps(3) && slow <= class.top() * 6 / 10,
                "{} {class:?}: cruise {slow}, top {}",
                tr.name,
                class.top()
            );
        }
    }
}

/// What went wrong for the rivals in one race.
#[derive(Debug, Default, Clone, Copy)]
struct Trouble {
    /// The longest a rival went (ticks) with under 1 m along the road a
    /// second, while racing.
    stuck: u32,
    /// The longest a rival was held at a wall (ticks in a row).
    wall: u32,
    /// Falls off the road.
    fell: u32,
}

#[test]
fn over_every_track_class_and_grid_no_rival_sticks_rides_a_wall_or_leaves_the_road() {
    let mut worst = Trouble::default();
    let mut falls = 0u32;
    for track in 0..TRACKS.len() as u8 {
        let tr = &TRACKS[usize::from(track)];
        let len = tr.len();
        for class in Class::ALL {
            for catchup in [CatchUp::Off, CatchUp::Light] {
                for seed in 0..SEEDS {
                    let mut t = Trouble::default();
                    let mut slow = [0u32; SHIPS];
                    let mut held = [0u32; SHIPS];
                    let mut back = [[0i64; 60]; SHIPS];
                    let mut r = new_race(track, class, seed, catchup);
                    run(&mut r, 0, |r, _| {
                        while let Some(e) = r.take_ev() {
                            if let Ev::Fell { ship } = e
                                && ship > 0
                            {
                                t.fell += 1;
                            }
                        }
                        let k = (r.tick % 60) as usize;
                        for i in 1..SHIPS {
                            let sh = &r.ships[i];
                            let racing = r.phase != Phase::Count && !sh.finished();
                            let p = sh.prog(len);
                            let gone = p - back[i][k];
                            back[i][k] = p;
                            if racing && sh.fall == 0 && r.tick > 180 + 60 && gone < i64::from(FM) {
                                slow[i] += 1;
                            } else {
                                slow[i] = 0;
                            }
                            t.stuck = t.stuck.max(slow[i]);
                            let half = r.tr().at(sh.s).half * 256;
                            let lim = (half - SHIP_HW).max(0);
                            if racing && sh.x.abs() >= lim {
                                held[i] += 1;
                            } else {
                                held[i] = 0;
                            }
                            t.wall = t.wall.max(held[i]);
                        }
                    });
                    if t.fell > 0 {
                        eprintln!("{} {class:?} {catchup:?} seed {seed}: {t:?}", tr.name);
                    }
                    falls += t.fell;
                    worst.stuck = worst.stuck.max(t.stuck);
                    worst.wall = worst.wall.max(t.wall);
                    assert!(
                        t.stuck == 0 && t.wall < 90,
                        "{} {class:?} {catchup:?} seed {seed}: {t:?}",
                        tr.name
                    );
                }
            }
        }
    }
    eprintln!("worst: {worst:?}, falls {falls}");
    // GLACIER's jump: before the rebuild the rivals fell into its gap 82
    // times in the 63 races with the catch-up OFF (79 of them STEADY;
    // measured on the PC, 2026-11-22); now 8 in these 126.
    assert!(falls <= 16, "{falls} falls");
}

/// The time of one race tick on the PC (release build): every track and
/// class, seeds 0 to 6, the computer driving all eight ships, the
/// catch-up OFF; best of three. A PC time only, not the chip's.
#[test]
#[ignore]
fn step_time() {
    let mut best = u128::MAX;
    let mut ticks = 0u64;
    for _ in 0..3 {
        ticks = 0;
        let t0 = std::time::Instant::now();
        for track in 0..camber::track::TRACKS.len() as u8 {
            for class in camber::race::Class::ALL {
                for seed in 0..7 {
                    let mut r = camber::race::Race::new();
                    r.setup(track, class, camber::race::Mode::Race, seed);
                    for _ in 0..60 * 400 {
                        r.step(None);
                        ticks += 1;
                        if r.ships.iter().all(|s| s.finished()) {
                            break;
                        }
                    }
                    std::hint::black_box(&r);
                }
            }
        }
        best = best.min(t0.elapsed().as_nanos());
    }
    eprintln!("ticks {ticks}, {} ns a tick", best / u128::from(ticks));
}
