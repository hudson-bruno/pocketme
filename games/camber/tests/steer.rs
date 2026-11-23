//! How the computer's pilot steers (`Race::drive`), measured on the PC:
//! the swing ("weave") the owner saw, before and after a change of the
//! pilot. All tests here are measurements (`--ignored --nocapture`); the
//! tables are printed and, with `CAMBER_STEER_OUT` set to a folder,
//! written there as `<test>-<CAMBER_STEER_TAG>.txt`.
//!
//! - `straight_bench`: the ship alone on an endless straight, started 3 m
//!   off the line, at 58 to 126 m/s: overshoot, how long it swings, the
//!   swing left after 3 and 10 s, how often the stick flips.
//! - `rivals_steer`: whole races with all eight ships computer-driven:
//!   the rivals' finish times, walls, falls, pads, stick flips, and how
//!   far they stay off their line on straight road.
//! - `pilot_time`: the PC's time for a race tick (a rough look only).
//!
//! The hand-over after the player's finish is `handover` in
//! `tests/laps.rs` (it needs the scripted players there).

use std::fmt::Write as _;
use std::time::Instant;

use camber::fixed::turn;
use camber::race::{COUNT, Class, Ctl, Ev, HOVER, Mode, Phase, Race, SHIPS, mps};
use camber::track::{BOOST, CHARGE, FM, SEG_BITS, TRACKS};

/// Writes `text` to `CAMBER_STEER_OUT/<name>-<tag>.txt` when that is set.
fn save(name: &str, text: &str) {
    eprintln!("{text}");
    if let Some(dir) = std::env::var_os("CAMBER_STEER_OUT") {
        let tag = std::env::var("CAMBER_STEER_TAG").unwrap_or_else(|_| "now".into());
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}-{tag}.txt")), text).unwrap();
    }
}

fn m(v: i32) -> f64 {
    f64::from(v) / f64::from(FM)
}

/// Angle units to degrees.
fn deg(a: i32) -> f64 {
    f64::from(a) * 360.0 / 65_536.0
}

/// Sign changes of the stick among the ticks where it is pushed more
/// than 32 of 256 ("big" flips: a small wobble round the middle does not
/// count).
pub fn big_flips(steer: &[i32]) -> u32 {
    let mut last = 0;
    let mut n = 0;
    for &s in steer {
        if s.abs() > 32 {
            let g = s.signum();
            if last != 0 && g != last {
                n += 1;
            }
            last = g;
        }
    }
    n
}

fn pp(v: &[i32]) -> i32 {
    if v.is_empty() {
        return 0;
    }
    v.iter().max().unwrap() - v.iter().min().unwrap()
}

/// The turning points of `e` (local tops and bottoms, a move back of at
/// least `hyst` counts), in time order: (tick, value).
pub fn turning_points(e: &[i32], hyst: i32) -> Vec<(usize, i32)> {
    let mut out = Vec::new();
    if e.is_empty() {
        return out;
    }
    // dir: 1 rising, -1 falling, 0 not known yet.
    let mut dir = 0;
    let mut ext = (0usize, e[0]);
    for (t, &v) in e.iter().enumerate() {
        match dir {
            0 => {
                if v >= ext.1 + hyst {
                    dir = 1;
                    ext = (t, v);
                } else if v <= ext.1 - hyst {
                    dir = -1;
                    ext = (t, v);
                }
            }
            1 => {
                if v > ext.1 {
                    ext = (t, v);
                } else if v <= ext.1 - hyst {
                    out.push(ext);
                    dir = -1;
                    ext = (t, v);
                }
            }
            _ => {
                if v < ext.1 {
                    ext = (t, v);
                } else if v >= ext.1 + hyst {
                    out.push(ext);
                    dir = 1;
                    ext = (t, v);
                }
            }
        }
    }
    out
}

/// What the bench runs: a class, the ship's pace, and what keeps it above
/// its top (a pad or a sling held on).
#[derive(Clone, Copy)]
enum Extra {
    None,
    Boost,
    Sling3,
}

/// One bench run: the ship on a GLACIER straight with a flat racing line
/// (`flat_boundary`), put back by one segment whenever it reaches the
/// next boundary, so the road ahead never ends and the target never
/// moves.
struct Bench {
    e: Vec<i32>,
    psi: Vec<i32>,
    rate: Vec<i32>,
    steer: Vec<i32>,
    knocks: u32,
    speed: i32,
}

/// The first boundary from 1 on where GLACIER's racing line stays the
/// same for the next ten boundaries (80 m, more than the pilot looks
/// ahead at 126 m/s), with no turn of 100 angle units or more, no
/// recharge lane, no pad where the ship drives (boundaries k and k + 1)
/// and none off the line further on (a pad on the line changes no
/// target). Before 2026-11-22 the bench used boundary 1 (the line in the
/// middle on boundaries 1 to 11, the pad on 8 and 9 on the line); since
/// the line moves at most `LINE_STEP` a segment
/// (`src/track.rs`, step 4b) the start straight's line slopes towards
/// the first turn, and putting the ship back one segment would jump its
/// target, so the bench looks further on.
pub fn flat_boundary() -> usize {
    let tr = &TRACKS[2];
    (1..tr.n())
        .find(|&k| {
            let l0 = tr.seg(k).line;
            (k..k + 11).all(|j| {
                let a = tr.seg(j);
                a.line == l0
                    && a.kind & CHARGE == 0
                    && (a.kind & BOOST == 0
                        || (j > k + 1 && i32::from(a.pad) * 2 == i32::from(a.line)))
                    && turn(a.yaw, tr.seg(j + 1).yaw).abs() < 100
            })
        })
        .expect("GLACIER has a straight with a flat racing line")
}

fn bench(class: Class, pace: i16, extra: Extra, v: i32, ticks: usize, x0: i32) -> Bench {
    let mut r = Race::new();
    r.setup(2, class, Mode::Trial, 0);
    for _ in 0..=COUNT {
        r.step(Some(Ctl::default()));
    }
    assert_eq!(r.phase, Phase::Run);
    let tr = r.tr();
    let home = flat_boundary();
    let seg = 1i32 << SEG_BITS;
    let first = home as i32 * seg;
    {
        let sh = &mut r.ships[0];
        sh.s = first + (1 << (SEG_BITS - 1));
        sh.x = tr.line(sh.s) + x0;
        sh.h = HOVER;
        sh.vh = 0;
        sh.psi = 0;
        sh.rate = 0;
        sh.vl = 0;
        sh.vf = mps(v);
        sh.pace = pace;
        sh.pitch = tr.at(sh.s).pitch;
        sh.lose_carve();
    }
    let mut b = Bench {
        e: Vec::new(),
        psi: Vec::new(),
        rate: Vec::new(),
        steer: Vec::new(),
        knocks: 0,
        speed: 0,
    };
    let mut was_hit = 0;
    for _ in 0..ticks {
        {
            let sh = &mut r.ships[0];
            match extra {
                Extra::None => {}
                Extra::Boost => sh.boost = 75,
                Extra::Sling3 => {
                    sh.sling = 195;
                    sh.sling_step = 3;
                }
            }
        }
        r.step(None);
        while r.take_ev().is_some() {}
        let sh = &mut r.ships[0];
        assert_eq!(sh.carve, 0, "the bench never carves");
        assert_eq!(sh.fall, 0, "the bench never falls");
        if sh.s >= first + seg {
            sh.s -= seg;
            sh.pitch = tr.at(sh.s).pitch;
        }
        assert!(tr.index(sh.s) == home, "the bench stays on its boundary");
        if sh.hit == 8 && was_hit != 8 {
            b.knocks += 1;
        }
        was_hit = sh.hit;
        b.e.push(sh.x - tr.line(sh.s));
        b.psi.push(sh.psi);
        b.rate.push(sh.rate);
        b.steer.push(sh.ctl.steer);
        b.speed = sh.v_track().0;
    }
    b
}

/// The bench at the speeds of the steering diagnosis (kept outside the
/// repository):
/// a STEADY ship (58 m/s), a BRISK rival (pace 957 of 1024, 67 m/s), a
/// BRISK ship (72), a WILD ship (86), WILD on a pad (112) and in a sling
/// at the third step (126). Started 3 m right of the line with the nose
/// straight and no slip; 30 s.
#[test]
#[ignore]
fn straight_bench() {
    let cases: [(&str, Class, i16, Extra, i32); 6] = [
        ("STEADY ship 58", Class::Steady, 1024, Extra::None, 58),
        ("BRISK rival 67", Class::Brisk, 957, Extra::None, 67),
        ("BRISK ship 72", Class::Brisk, 1024, Extra::None, 72),
        ("WILD ship 86", Class::Wild, 1024, Extra::None, 86),
        ("WILD pad 112", Class::Wild, 1024, Extra::Boost, 112),
        ("WILD sling3 126", Class::Wild, 1024, Extra::Sling3, 126),
    ];
    let ticks = 60 * 30;
    let home = flat_boundary();
    let mut out = String::new();
    out.push_str(&format!(
        "MEASURED ON THE PC, tests/steer.rs straight_bench. The real Race::step(None) on a GLACIER\n\
         straight with a flat racing line (boundary {home}, `flat_boundary`), the ship put back one\n\
         segment at a time so the road never ends; alone (TIME TRIAL);\n\
         started 3.00 m right of the racing line, nose straight, no slip; 30 s of ticks of 1/60 s.\n\
         over: the deepest point past the line in the first 4 s (m, - is the far side).\n\
         turns: the first five turning points of x - line (m), in time order (a move back of 2 cm counts).\n\
         period: mean time between the turning points 2 apart in the first 10 s.\n\
         settle: the time after which x stays within 0.25 m of the line to the end ('never' if not).\n\
         x p-p / nose p-p: peak to peak of x - line (m) and of the nose (deg) over 3-10 s and 10-30 s.\n\
         flips/s: stick sign changes a second among ticks with the stick beyond 32 of 256, over 0-3 s,\n\
         3-10 s and 10-30 s. knocks: wall touches. v: speed along the road at the end (m/s).\n\n"
    ));
    out.push_str("case               over   turns (m)                              period  settle   x p-p 3-10/10-30   nose p-p 3-10/10-30   flips/s 0-3/3-10/10-30  knocks  v\n");
    for (name, class, pace, extra, v) in cases {
        let b = bench(class, pace, extra, v, ticks, 3 * FM);
        let over = b.e[..240].iter().min().copied().unwrap();
        let tp = turning_points(&b.e[..600], FM / 50);
        let turns: Vec<String> = tp
            .iter()
            .take(5)
            .map(|&(_, x)| format!("{:+.2}", m(x)))
            .collect();
        let period = if tp.len() >= 3 {
            let k = tp.len() - 1;
            let span = (tp[k].0 - tp[0].0) as f64 / k as f64 * 2.0 / 60.0;
            format!("{span:.2} s")
        } else {
            "-".into()
        };
        let last_out = b.e.iter().rposition(|&x| x.abs() > FM / 4);
        let settle = match last_out {
            None => "0.00 s".to_string(),
            Some(t) if t + 1 >= ticks => "never".to_string(),
            Some(t) => format!("{:.2} s", (t + 1) as f64 / 60.0),
        };
        let fl =
            |a: usize, z: usize| f64::from(big_flips(&b.steer[a..z])) / ((z - a) as f64 / 60.0);
        writeln!(
            out,
            "{name:<17} {:+.2}  {:<38} {period:>7} {settle:>7}   {:5.2} / {:5.2}       {:5.2} / {:5.2}          {:4.2} / {:4.2} / {:4.2}      {:3}   {:5.1}",
            m(over),
            turns.join(" "),
            m(pp(&b.e[180..600])),
            m(pp(&b.e[600..])),
            deg(pp(&b.psi[180..600])),
            deg(pp(&b.psi[600..])),
            fl(0, 180),
            fl(180, 600),
            fl(600, ticks),
            b.knocks,
            f64::from(b.speed) * 60.0 / f64::from(FM),
        )
        .unwrap();
    }
    save("bench", &out);
}

/// What one rival did in a race.
#[derive(Default, Clone, Copy)]
struct RivalSum {
    races: u32,
    finish: u64,
    fastest: u32,
    best_lap: u64,
    walls: u32,
    falls: u32,
    boosts: u32,
    knocks: u32,
    flips: u32,
    /// The turn rate's sign changes beyond 100 units a tick (the ship's
    /// roll in the picture is 8 times the rate), and chatter: ticks the
    /// stick jumps from one side to the other with at least 128 of 256
    /// on both.
    rolls: u32,
    chatter: u32,
    /// Ticks the stick was counted on (not carving, not falling, racing).
    ticks: u32,
    /// x - (line + lane) on plain straight road: squares summed (m^2
    /// times 10^4), the count, and the largest.
    sq: f64,
    n: u32,
    worst: i32,
}

/// Is the ship `i` on plain straight road, alone, where its target is
/// its line and lane: the next six boundaries turn less than 100 angle
/// units each, the line does not change on them, no pad and no recharge
/// lane on them or at the look point, no other ship within 24 m ahead,
/// not carving, settling, falling or a ghost.
fn plain(r: &Race, i: usize) -> bool {
    let tr = r.tr();
    let sh = &r.ships[i];
    if sh.carve != 0 || sh.settle > 0 || sh.fall > 0 || sh.ghost > 0 || sh.finished() {
        return false;
    }
    let k = tr.index(sh.s);
    let l0 = tr.seg(k).line;
    for j in 0..6 {
        let a = tr.seg(k + j);
        if turn(a.yaw, tr.seg(k + j + 1).yaw).abs() >= 100
            || a.line != l0
            || a.kind & (BOOST | CHARGE) != 0
        {
            return false;
        }
    }
    let len = tr.len();
    for (j, o) in r.ships.iter().enumerate() {
        if j == i {
            continue;
        }
        let mut ds = o.s - sh.s;
        if ds > len / 2 {
            ds -= len;
        } else if ds < -len / 2 {
            ds += len;
        }
        if ds > -FM * 4 && ds < FM * 24 {
            return false;
        }
    }
    true
}

/// Whole races, all eight ships computer-driven, every track and class,
/// seeds 0 to 6 (`CAMBER_SEEDS`): per track and class, summed over the
/// seven rivals (ships 1 to 7) of every race. Ship 0 is reported apart:
/// it is the player's ship driven by the same pilot at the class's full
/// pace (as after the player's finish).
#[test]
#[ignore]
fn rivals_steer() {
    let seeds: u32 = std::env::var("CAMBER_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(7);
    let mut out = format!(
        "MEASURED ON THE PC, tests/steer.rs rivals_steer. All eight ships computer-driven\n\
         (Race::step(None)), seeds 0 to {}, every track and class; a rival is ships 1 to 7, summed\n\
         over every race. Ticks of 1/60 s.\n\
         finish: the rivals' mean finish tick and the fastest; best lap: the mean of the rivals' best laps.\n\
         walls / falls / pads: Ev::Wall (hard hits), Ev::Fell, Ev::Boost, a rival per lap.\n\
         knocks: a tick a wall or ship touches a rival after one that did not, per rival per lap.\n\
         flips/s: stick sign changes a second (stick beyond 32 of 256), on ticks not carving;\n\
         rolls/s: the turn rate's sign changes a second beyond 100 units a tick (the roll seen);\n\
         chat/s: ticks a second the stick jumps side to side with 128 of 256 or more on both ticks.\n\
         off-line: x - (line + own lane) on plain straight road with no ship close ahead (see `plain`):\n\
         root mean square and the largest (m), and the share of race ticks that were plain.\n\
         Ship 0 (the player's ship, full pace, lane 0) is on the line after the '|'.\n\n\
         track    class   finish mean fastest  best lap   walls  falls  pads  knocks  flips/s rolls/s chat/s  off-line rms / max  plain%  | ship0 finish flips/s rms / max\n",
        seeds - 1
    );
    for t in 0..TRACKS.len() as u8 {
        for class in Class::ALL {
            let mut riv = RivalSum {
                fastest: u32::MAX,
                ..RivalSum::default()
            };
            let mut s0 = RivalSum::default();
            let mut race_ticks = 0u64;
            for seed in 0..seeds {
                let mut r = Race::new();
                r.setup(t, class, Mode::Race, seed);
                let mut was_hit = [0u8; SHIPS];
                let mut last = [0i32; SHIPS];
                let mut last_rate = [0i32; SHIPS];
                let mut prev = [0i32; SHIPS];
                for _ in 0..60 * 400 {
                    r.step(None);
                    while let Some(e) = r.take_ev() {
                        match e {
                            Ev::Wall { ship } if ship > 0 => riv.walls += 1,
                            Ev::Fell { ship } if ship > 0 => riv.falls += 1,
                            Ev::Boost { ship } if ship > 0 => riv.boosts += 1,
                            _ => {}
                        }
                    }
                    if r.phase == Phase::Count {
                        continue;
                    }
                    for i in 0..SHIPS {
                        let sh = r.ships[i];
                        if sh.finished() {
                            continue;
                        }
                        let acc = if i == 0 { &mut s0 } else { &mut riv };
                        if i > 0 {
                            race_ticks += 1;
                        }
                        if sh.hit == 8 && was_hit[i] != 8 {
                            acc.knocks += 1;
                        }
                        was_hit[i] = sh.hit;
                        if sh.carve == 0 && sh.fall == 0 {
                            acc.ticks += 1;
                            let s = sh.ctl.steer;
                            if s.abs() > 32 {
                                if last[i] != 0 && s.signum() != last[i] {
                                    acc.flips += 1;
                                }
                                last[i] = s.signum();
                            }
                            if s.abs() >= 128
                                && prev[i].abs() >= 128
                                && s.signum() != prev[i].signum()
                            {
                                acc.chatter += 1;
                            }
                            prev[i] = s;
                            if sh.rate.abs() > 100 {
                                if last_rate[i] != 0 && sh.rate.signum() != last_rate[i] {
                                    acc.rolls += 1;
                                }
                                last_rate[i] = sh.rate.signum();
                            }
                        }
                        if plain(&r, i) {
                            let tr = r.tr();
                            let e = sh.x - tr.line(sh.s) - i32::from(sh.lane) * FM / 4;
                            let em = m(e);
                            acc.sq += em * em;
                            acc.n += 1;
                            acc.worst = acc.worst.max(e.abs());
                        }
                    }
                    if r.ships.iter().all(|s| s.finished()) {
                        break;
                    }
                }
                assert!(
                    r.ships.iter().all(|s| s.finished()),
                    "a ship never finished"
                );
                for sh in &r.ships[1..] {
                    riv.races += 1;
                    riv.finish += u64::from(sh.finish);
                    riv.fastest = riv.fastest.min(sh.finish);
                    riv.best_lap += u64::from(sh.best);
                }
                s0.races += 1;
                s0.finish += u64::from(r.ships[0].finish);
            }
            let laps = f64::from(riv.races) * 3.0;
            let rms = |a: &RivalSum| (a.sq / f64::from(a.n.max(1))).sqrt();
            writeln!(
                out,
                "{:<8} {:<6}  {:8}  {:6}  {:8}   {:5.2}  {:5.2}  {:4.2}  {:6.2}  {:6.2}  {:6.2} {:6.2}     {:5.2} / {:5.2}     {:5.1}   | {:6}  {:5.2}  {:5.2} / {:5.2}",
                TRACKS[usize::from(t)].name,
                class.name(),
                riv.finish / u64::from(riv.races),
                riv.fastest,
                riv.best_lap / u64::from(riv.races),
                f64::from(riv.walls) / laps,
                f64::from(riv.falls) / laps,
                f64::from(riv.boosts) / laps,
                f64::from(riv.knocks) / laps,
                f64::from(riv.flips) / (f64::from(riv.ticks.max(1)) / 60.0),
                f64::from(riv.rolls) / (f64::from(riv.ticks.max(1)) / 60.0),
                f64::from(riv.chatter) / (f64::from(riv.ticks.max(1)) / 60.0),
                rms(&riv),
                m(riv.worst),
                f64::from(riv.n) * 100.0 / race_ticks.max(1) as f64,
                s0.finish / u64::from(s0.races),
                f64::from(s0.flips) / (f64::from(s0.ticks.max(1)) / 60.0),
                rms(&s0),
                m(s0.worst),
            )
            .unwrap();
        }
    }
    save("steer-rivals", &out);
}

/// The PC's time for whole races, all eight ships computer-driven (BRISK,
/// every track, seed 0), per race tick. Only a rough look: it is the PC's
/// time, not the board's, and the PC is busy with other work.
#[test]
#[ignore]
fn pilot_time() {
    let mut out = String::from(
        "MEASURED ON THE PC (wall clock, noisy), tests/steer.rs pilot_time: Race::step(None) with eight\n\
         computer-driven ships, BRISK, seed 0, each track run 20 times; nanoseconds a race tick (best of 20).\n",
    );
    for t in 0..TRACKS.len() as u8 {
        let mut best = f64::MAX;
        for _ in 0..20 {
            let mut r = Race::new();
            r.setup(t, Class::Brisk, Mode::Race, 0);
            let start = Instant::now();
            let mut n = 0u32;
            for _ in 0..60 * 400 {
                r.step(None);
                while r.take_ev().is_some() {}
                n += 1;
                if r.ships.iter().all(|s| s.finished()) {
                    break;
                }
            }
            let ns = start.elapsed().as_nanos() as f64 / f64::from(n);
            best = best.min(ns);
        }
        writeln!(
            out,
            "{:<8} {:8.0} ns a tick",
            TRACKS[usize::from(t)].name,
            best
        )
        .unwrap();
    }
    save("pilot-time", &out);
}

/// The baked racing line (`Seg::line`, 1/4 m units): how far it moves
/// from one 8 m segment to the next. A big move is a jump of the
/// computer's aim. MEASURED ON THE PC from the baked tables.
#[test]
#[ignore]
fn line_steps() {
    let mut out = String::from(
        "MEASURED ON THE PC: the baked racing line, moves between neighbouring segments (8 m).\n\
         track    segs  max move (m)  moves over 0.5 m  over 1 m  max off middle (m)  segs off middle\n",
    );
    for t in &TRACKS {
        let n = t.n();
        let (mut big, mut over_half, mut over_one, mut far, mut off) = (0i32, 0, 0, 0i32, 0);
        for j in 0..n {
            let a = i32::from(t.seg(j).line);
            let b = i32::from(t.seg(j + 1).line);
            let d = (b - a).abs();
            big = big.max(d);
            over_half += u32::from(d > 2);
            over_one += u32::from(d > 4);
            far = far.max(a.abs());
            off += u32::from(a != 0);
        }
        let _ = writeln!(
            out,
            "{:<8} {:4}  {:12.2}  {:16}  {:8}  {:18.2}  {:15}",
            t.name,
            n,
            f64::from(big) / 4.0,
            over_half,
            over_one,
            f64::from(far) / 4.0,
            off
        );
    }
    save("line-steps", &out);
}
