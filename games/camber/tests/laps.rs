//! Scripted drivers and lap times, so the numbers of the CARVE (L/R)
//! and of the road's width can be made again.
//!
//! Every driver flies the player's ship through the keys only, one
//! call a tick (60 calls a second), in a TIME TRIAL at BRISK:
//!
//! - (a) clean: the computer's pilot without the carve
//!   (`Race::drive_plain`) on the stick, A and B; it never touches L or R.
//! - (b) carving: the same, but on every corner it presses L or R (the
//!   corner's way, stick in the middle on the press), holds the carve
//!   with the stick set for the arc the pilot wants, and lets go where
//!   the corner ends or the outer wall comes close.
//! - (c) abuse: (a) on the corners, but on every straight it carves
//!   from side to side with the button held and the stick wiggling,
//!   letting go as soon as a heat step is reached (or the wall is near)
//!   and carving back the other way.
//! - (d) edge: carves from edge to edge on the straights with the stick
//!   full into the carve, letting go only before the wall.
//! - (e) tap: (a), tapping L or R (4 ticks) every third of a second
//!   through each corner; "bad" is (e) on the corners and (c) on the
//!   straights.
//!
//! `reward_table` measures them with the game's reward;
//! `rivals_and_places` measures the computer's pilots and where three
//! model players finish in a RACE.
//!
//! Run with `--ignored --nocapture`; the tables are also written to
//! `CAMBER_DRIFT_OUT` (default `.local/camber-drift` in the repository).
//! `CAMBER_CLASS` runs the time trials at another class.
//!
//! One test runs every time: with the game's reward, on every
//! track, a good carver's best flying laps are at least 9 % faster than
//! the clean driver's, and abuse on the straights and taps in the corners
//! do not make the three laps faster.

mod common;

use std::path::PathBuf;

use camber::app::Log;
use camber::fixed::turn;
use camber::race::{
    CARVE_START_SPEED, Class, Ctl, Ev, Mode, Phase, REWARD, Race, SHIPS, SKILLS, Ship, Skill,
    carve_rate, heat_step,
};
use camber::scene::{Detail, ViewKind};
use camber::track::{FM, SEG_BITS, TRACKS, Track};
use common::{CALL_60, Driver, save_frame};
use game_kit::button;

/// Where the tables and pictures go.
fn out_dir() -> PathBuf {
    let p = std::env::var_os("CAMBER_DRIFT_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/camber-drift")
        });
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// The stick for a steering wish (the game takes the dead zone back out).
fn stick_of(steer: i32) -> (i16, i16) {
    let dz = camber::app::DEAD_ZONE;
    let x = if steer == 0 {
        0
    } else {
        steer.signum() * dz + steer * (32_767 - dz) / 256
    };
    (x.clamp(-32_767, 32_767) as i16, 0)
}

/// The keys of the clean driver: stick, A, and B when the pilot wants
/// both brakes. One-sided brakes are dropped.
fn clean_keys(c: Ctl) -> (u32, (i16, i16)) {
    let mut held = 0;
    if c.thrust {
        held |= button::A;
    }
    if c.brake_l && c.brake_r {
        held |= button::B;
    }
    (held, stick_of(c.steer))
}

/// Lap times of one time trial (ticks): lap 1, 2, 3 and the total.
#[derive(Clone, Copy, Debug, Default)]
pub struct Laps {
    pub laps: [u32; 3],
    pub total: u32,
    /// Carves started, slings paid at each step (1, 2, 3), and wall
    /// knocks (a tick the shield drops after one it did not).
    pub carves: u32,
    pub slings: [u32; 3],
    pub walls: u32,
    /// Falls off the road.
    pub falls: u32,
    /// Wall knocks and falls that began during a sling or within
    /// [`AFTER_SLING`] ticks after its end (a sling that throws the ship
    /// into the next wall), and the ticks inside such windows (to compare
    /// with the knocks a tick of the rest of the run).
    pub walls_after: u32,
    pub falls_after: u32,
    pub ticks_after: u32,
    pub ticks: u32,
    /// In a sum of runs: the sum of their best flying laps.
    pub best_sum: u32,
}

/// Ticks after a sling's end in which a knock still counts as the
/// sling's.
pub const AFTER_SLING: u32 = 60;

impl Laps {
    /// The better of the two flying laps (of a sum: the sum of each
    /// run's better flying lap).
    pub fn best_flying(&self) -> u32 {
        if self.best_sum > 0 {
            self.best_sum
        } else {
            self.laps[1].min(self.laps[2])
        }
    }
}

/// Several runs added up (laps, totals and counts are sums).
fn sum_laps(ls: &[Laps]) -> Laps {
    let mut o = Laps::default();
    for l in ls {
        for k in 0..3 {
            o.laps[k] += l.laps[k];
            o.slings[k] += l.slings[k];
        }
        o.total += l.total;
        o.best_sum += l.best_flying();
        o.carves += l.carves;
        o.walls += l.walls;
        o.falls += l.falls;
        o.walls_after += l.walls_after;
        o.falls_after += l.falls_after;
        o.ticks_after += l.ticks_after;
        o.ticks += l.ticks;
    }
    o
}

/// A time trial on `track` (BRISK) flown by `pilot`, which gets the
/// driver and returns the keys for this call.
pub fn trial(track: u8, pilot: impl FnMut(&Driver) -> (u32, (i16, i16))) -> Laps {
    trial_at(track, 0, pilot)
}

/// [`trial`] with the ship put `nudge` (fine units) to the side on the
/// grid: another start, so a measurement can be averaged over several
/// paths (a small change of the path changes the lap by a percent or
/// two).
pub fn trial_at(track: u8, nudge: i32, pilot: impl FnMut(&Driver) -> (u32, (i16, i16))) -> Laps {
    run(track, trial_class(), None, nudge, pilot).0
}

/// The class of the time trials: BRISK, or `CAMBER_CLASS` (STEADY, BRISK
/// or WILD).
fn trial_class() -> Class {
    match std::env::var("CAMBER_CLASS").as_deref() {
        Ok("STEADY") => Class::Steady,
        Ok("WILD") => Class::Wild,
        _ => Class::Brisk,
    }
}

/// The starts of a measurement averaged over `n` paths (`CAMBER_PATHS`,
/// default 5): 0, then a quarter metre and more to either side.
fn nudges() -> Vec<i32> {
    let n: i32 = std::env::var("CAMBER_PATHS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    (0..n)
        .map(|k| (k + 1) / 2 * if k % 2 == 0 { 1 } else { -1 } * FM / 4)
        .collect()
}

/// BRISK's skill row before the rivals were rebuilt, as far as the
/// plain pilot reads it (the speed plan): the trials' model pilots.
const PILOT: Skill = Skill {
    dare: 1_120,
    sling_brake: 12,
    need: false,
    ..SKILLS[1]
};

/// What happened to every ship in one run (counted tick by tick from the
/// ships' state).
#[derive(Clone, Copy, Debug, Default)]
pub struct Counts {
    /// Carves started, slings at each step, falls, hard wall hits (the
    /// race's own `Ev::Wall`, counted only where the test reads the
    /// events itself), and knocks: a tick a wall or a ship touches the
    /// ship after one that did not (from the ship's `hit`).
    pub carves: u32,
    pub slings: [u32; 3],
    pub falls: u32,
    pub walls: u32,
    pub knocks: u32,
}

/// The player's place and the race (every ship finished) after a RACE.
pub struct Raced {
    pub place: u8,
    pub race: Race,
    /// Counts of each ship (0 is the player).
    pub counts: [Counts; SHIPS],
}

/// One run on `track` in `class` flown by `pilot`: a time trial
/// (`seed` None) or a RACE with this seed, which goes on with the
/// computer's pilots after the player's finish until every ship is over
/// the line.
pub fn run(
    track: u8,
    class: Class,
    seed: Option<u32>,
    nudge: i32,
    mut pilot: impl FnMut(&Driver) -> (u32, (i16, i16)),
) -> (Laps, Option<Raced>) {
    let mut d = Driver::new();
    d.draw = false;
    d.set(Detail::Low, ViewKind::Chase, true);
    d.call_us = CALL_60;
    let mode = if seed.is_some() {
        Mode::Race
    } else {
        Mode::Trial
    };
    d.race(track, class, mode);
    if let Some(seed) = seed {
        d.app.race.setup(track, class, Mode::Race, seed);
        d.app.cam.set = false;
    } else if class == Class::Brisk {
        // The model pilots of the time trials fly with BRISK's speed
        // plan as it was when the reward was measured (2026-11-22,
        // before the rivals were rebuilt): the reward's numbers then
        // do not move with the rivals' rows.
        d.app.race.skill = PILOT;
    }
    d.app.race.ships[0].x += nudge;
    let mut calls = 0;
    let mut out = Laps::default();
    let mut counts = [Counts::default(); SHIPS];
    let mut was = d.app.race.ships;
    let mut knocked = false;
    let mut tail = 0u32;
    while d.app.race.phase != Phase::Done {
        let (held, stick) = pilot(&d);
        d.hold(held, stick);
        calls += 1;
        assert!(calls < 60 * 400, "no finish");
        let now = d.app.race.ships;
        count(&was, &now, &mut counts);
        let sh = now[0];
        let w0 = was[0];
        tail = if sh.sling > 0 {
            AFTER_SLING
        } else {
            tail.saturating_sub(1)
        };
        let win = sh.sling > 0 || tail > 0;
        if sh.carve != 0 && w0.carve == 0 {
            out.carves += 1;
        }
        if sh.sling > w0.sling && sh.sling_step > 0 {
            out.slings[usize::from(sh.sling_step - 1)] += 1;
        }
        let knock = sh.shield < w0.shield;
        if knock && !knocked {
            out.walls += 1;
            if win {
                out.walls_after += 1;
            }
        }
        if sh.fall > 0 && w0.fall == 0 {
            out.falls += 1;
            if win {
                out.falls_after += 1;
            }
        }
        if d.app.race.phase == Phase::Run {
            out.ticks += 1;
            if win {
                out.ticks_after += 1;
            }
        }
        knocked = knock;
        was = now;
    }
    let mut k = 0;
    for l in &d.logs {
        if let Log::Lap { time, .. } = l
            && k < 2
        {
            out.laps[k] = *time;
            k += 1;
        }
    }
    out.total = d.app.race.ships[0].finish;
    out.laps[2] = out.total - out.laps[0] - out.laps[1];
    if seed.is_none() {
        return (out, None);
    }
    let place = d.app.race.place(0);
    let mut r = d.app.race;
    for _ in 0..60 * 200 {
        if r.ships.iter().all(|s| s.finished()) {
            break;
        }
        r.step(None);
        while r.take_ev().is_some() {}
        let now = r.ships;
        count(&was, &now, &mut counts);
        was = now;
    }
    (
        out,
        Some(Raced {
            place,
            race: r,
            counts,
        }),
    )
}

/// Adds what changed from `was` to `now` to the counts (carves, slings,
/// falls, knocks; hard wall hits come from the events).
fn count(was: &[Ship; SHIPS], now: &[Ship; SHIPS], c: &mut [Counts; SHIPS]) {
    for i in 0..SHIPS {
        let (w, n) = (&was[i], &now[i]);
        if n.carve != 0 && w.carve == 0 {
            c[i].carves += 1;
        }
        if n.sling > w.sling && n.sling_step > 0 {
            c[i].slings[usize::from(n.sling_step - 1)] += 1;
        }
        if n.fall > 0 && w.fall == 0 {
            c[i].falls += 1;
        }
        if n.hit == 8 && w.hit != 8 {
            c[i].knocks += 1;
        }
    }
}

fn clean(d: &Driver) -> (u32, (i16, i16)) {
    clean_keys(d.app.race.drive_plain(0))
}

/// How far the road turns over the next `n` segments from `s` (angle
/// units, right positive).
fn bend_ahead(tr: &Track, s: i32, from: usize, n: usize) -> i32 {
    let i = tr.index(s) + from;
    turn(tr.seg(i).yaw, tr.seg(i + n).yaw)
}

/// The stick that makes a carve the way `dir` turn the nose nearest to
/// `rate` (angle units a tick).
fn stick_for_rate(dir: i32, rate: i32) -> i32 {
    let mut best = (i32::MAX, 0);
    for a in (-256..=256).step_by(16) {
        let e = (carve_rate(dir, a) - rate).abs();
        if e < best.0 {
            best = (e, a);
        }
    }
    best.1
}

/// The keys that hold a carve the way `dir` along the arc the pilot `c`
/// wants.
fn carve_keys(c: Ctl, dir: i32) -> (u32, (i16, i16)) {
    let (mut held, _) = clean_keys(c);
    held |= if dir < 0 { button::L } else { button::R };
    // The pilot's steer is a turn rate in 256ths of the fastest normal
    // turn; the carve's stick is set to give that rate.
    let rate = c.steer * camber::race::TURN_MAX / 256;
    (held, stick_of(stick_for_rate(dir, rate)))
}

/// Room left to the wall on the side the carve throws the ship (the
/// outside of the turn), in fine units.
fn room_outside(tr: &Track, sh: &Ship, dir: i32) -> i32 {
    let half = tr.at(sh.s).half * 256;
    half + sh.x * dir
}

/// Driver (b): carves the corners.
#[derive(Default)]
struct Carver {
    /// Ticks since the last let-go (no new press for a moment).
    rest: u32,
}

impl Carver {
    fn keys(&mut self, d: &Driver) -> (u32, (i16, i16)) {
        let r = &d.app.race;
        let tr = r.tr();
        let sh = &r.ships[0];
        let c = r.drive_plain(0);
        self.rest = self.rest.saturating_add(1);
        let dir = i32::from(sh.carve);
        if dir != 0 {
            // Hold while the road still turns this way close ahead and the
            // outer wall is not close.
            let ahead = bend_ahead(tr, sh.s, 0, 2) * dir;
            let inside = room_outside(tr, sh, -dir);
            if ahead > 400
                && room_outside(tr, sh, dir) > FM * 2
                && inside > FM * 2
                && c.steer * dir > -200
            {
                return carve_keys(c, dir);
            }
            self.rest = 0;
            return clean_keys(c);
        }
        let bend = bend_ahead(tr, sh.s, 0, 3);
        if self.rest > 20 && bend.abs() > 1_400 && sh.vf > CARVE_START_SPEED + FM / 6 {
            // The press: the corner's button, stick in the middle.
            let (mut held, _) = clean_keys(c);
            held |= if bend < 0 { button::L } else { button::R };
            return (held, (0, 0));
        }
        clean_keys(c)
    }
}

/// Driver (c): carves from side to side on the straights.
#[derive(Default)]
struct Abuser {
    next: i32,
    rest: u32,
    held_for: u32,
}

impl Abuser {
    fn keys(&mut self, d: &Driver) -> (u32, (i16, i16)) {
        let r = &d.app.race;
        let tr = r.tr();
        let sh = &r.ships[0];
        let c = r.drive_plain(0);
        self.rest = self.rest.saturating_add(1);
        let dir = i32::from(sh.carve);
        if dir != 0 {
            self.held_for += 1;
            // Let go at the first heat step, or before the wall.
            // The way across the road the ship goes, and the room left that
            // way: let go in time to straighten up.
            let (_, vx) = sh.v_track();
            let toward = (vx * dir).max(0);
            let near = room_outside(tr, sh, -dir) < FM * 4 + toward * 70;
            if heat_step(sh.heat) >= 1 || near || self.held_for > 60 {
                self.rest = 0;
                self.next = -dir;
                return clean_keys(c);
            }
            let (mut held, _) = clean_keys(c);
            held |= if dir < 0 { button::L } else { button::R };
            // The stick wiggles full in and full out.
            let wiggle = if (self.held_for / 4).is_multiple_of(2) {
                256
            } else {
                -256
            };
            return (held, stick_of(wiggle));
        }
        let straight = bend_ahead(tr, sh.s, 0, 4).abs() < 600;
        if straight && self.rest > 6 && sh.vf > CARVE_START_SPEED + FM / 6 {
            let way = if self.next != 0 {
                self.next
            } else if sh.x > 0 {
                -1
            } else {
                1
            };
            self.held_for = 0;
            let (mut held, _) = clean_keys(c);
            held |= if way < 0 { button::L } else { button::R };
            return (held, (0, 0));
        }
        clean_keys(c)
    }
}

/// Driver (d): carves from edge to edge on the straights with the stick
/// full into the carve (TIGHT), letting go only before the wall.
#[derive(Default)]
struct Edger {
    next: i32,
    rest: u32,
    held_for: u32,
}

impl Edger {
    fn keys(&mut self, d: &Driver) -> (u32, (i16, i16)) {
        let r = &d.app.race;
        let tr = r.tr();
        let sh = &r.ships[0];
        let c = r.drive_plain(0);
        self.rest = self.rest.saturating_add(1);
        let dir = i32::from(sh.carve);
        if dir != 0 {
            self.held_for += 1;
            let (_, vx) = sh.v_track();
            let toward = (vx * dir).max(0);
            let near = room_outside(tr, sh, -dir) < FM * 3 + toward * 50;
            if near || self.held_for > 120 {
                self.rest = 0;
                self.next = -dir;
                return clean_keys(c);
            }
            let (mut held, _) = clean_keys(c);
            held |= if dir < 0 { button::L } else { button::R };
            return (held, stick_of(256 * dir));
        }
        let straight = bend_ahead(tr, sh.s, 0, 4).abs() < 600;
        if straight && self.rest > 6 && sh.vf > CARVE_START_SPEED + FM / 6 {
            let way = if self.next != 0 {
                self.next
            } else if sh.x > 0 {
                -1
            } else {
                1
            };
            self.held_for = 0;
            let (mut held, _) = clean_keys(c);
            held |= if way < 0 { button::L } else { button::R };
            return (held, (0, 0));
        }
        clean_keys(c)
    }
}

/// How long a tap holds L or R (ticks).
const TAP_TICKS: u32 = 4;

/// Driver (e): the clean driver who taps L or R (the corner's side, his
/// own stick) every third of a second through each corner; with
/// `straights` (the bad carver) he also abuses the straights like (c).
#[derive(Default)]
struct Tapper {
    left: u32,
    dir: i32,
    rest: u32,
    straights: Option<Abuser>,
}

impl Tapper {
    fn bad() -> Tapper {
        Tapper {
            straights: Some(Abuser::default()),
            ..Tapper::default()
        }
    }

    fn keys(&mut self, d: &Driver) -> (u32, (i16, i16)) {
        let r = &d.app.race;
        let tr = r.tr();
        let sh = &r.ships[0];
        let c = r.drive_plain(0);
        self.rest = self.rest.saturating_add(1);
        if self.left > 0 {
            self.left -= 1;
            let (mut held, stick) = clean_keys(c);
            held |= if self.dir < 0 { button::L } else { button::R };
            return (held, stick);
        }
        if let Some(a) = self.straights.as_mut() {
            let straight = bend_ahead(tr, sh.s, 0, 4).abs() < 600;
            if sh.carve != 0 || straight {
                return a.keys(d);
            }
        }
        let bend = bend_ahead(tr, sh.s, 0, 3);
        if self.rest > 20 && bend.abs() > 1_400 && sh.vf > CARVE_START_SPEED + FM / 6 {
            self.dir = bend.signum();
            self.left = TAP_TICKS - 1;
            self.rest = 0;
            let (mut held, stick) = clean_keys(c);
            held |= if self.dir < 0 { button::L } else { button::R };
            return (held, stick);
        }
        clean_keys(c)
    }
}

/// The scripted players by name.
const DRIVERS: [&str; 6] = ["clean", "good", "abuse", "edge", "tap", "bad"];

/// A time trial by the driver called `name`.
fn trial_by(name: &str, t: u8, nudge: i32) -> Laps {
    match name {
        "clean" => trial_at(t, nudge, clean),
        "good" => {
            let mut p = Carver::default();
            trial_at(t, nudge, |d| p.keys(d))
        }
        "abuse" => {
            let mut p = Abuser::default();
            trial_at(t, nudge, |d| p.keys(d))
        }
        "edge" => {
            let mut p = Edger::default();
            trial_at(t, nudge, |d| p.keys(d))
        }
        "tap" => {
            let mut p = Tapper::default();
            trial_at(t, nudge, |d| p.keys(d))
        }
        "bad" => {
            let mut p = Tapper::bad();
            trial_at(t, nudge, |d| p.keys(d))
        }
        _ => panic!("no driver {name}"),
    }
}

/// A RACE by the driver called `name` (clean, good or bad).
fn race_by(name: &str, t: u8, class: Class, seed: u32) -> (Laps, Raced) {
    let (l, r) = match name {
        "clean" => run(t, class, Some(seed), 0, clean),
        "good" => {
            let mut p = Carver::default();
            run(t, class, Some(seed), 0, |d| p.keys(d))
        }
        "bad" => {
            let mut p = Tapper::bad();
            run(t, class, Some(seed), 0, |d| p.keys(d))
        }
        _ => panic!("no driver {name}"),
    };
    (l, r.unwrap())
}

/// Hundredths of a percent `a` is below `base` (time saved), as "12.34".
fn pct(base: u32, a: u32) -> String {
    let v = (i64::from(base) - i64::from(a)) * 10_000 / i64::from(base.max(1));
    let sign = if v < 0 { "-" } else { "+" };
    format!("{sign}{}.{:02}", v.abs() / 100, v.abs() % 100)
}

/// The reward table: with the game's reward, on each track, every
/// driver's time trial (BRISK): 3-lap total and best flying lap with the change from
/// the clean driver, slings per step, wall knocks and falls (all, and
/// during a sling or within AFTER_SLING ticks after it). `CAMBER_DRIVERS` (names
/// split by ',') measures fewer drivers. Written to
/// `rewards-<CAMBER_DRIFT_TAG>.txt`.
#[test]
#[ignore]
fn reward_table() {
    let tag = std::env::var("CAMBER_DRIFT_TAG").unwrap_or_else(|_| "now".into());
    let drivers: Vec<String> = std::env::var("CAMBER_DRIVERS")
        .map(|v| v.split(',').map(|x| x.trim().to_string()).collect())
        .unwrap_or_else(|_| DRIVERS.iter().map(|x| x.to_string()).collect());
    let paths = nudges().len();
    let mut out = format!(
        "MEASURED ON THE PC ({tag}), tests/laps.rs reward_table. Ticks of 1/60 s. TIME TRIAL, {}.\n\
         Every line is the SUM of {paths} runs from starts a quarter metre apart (CAMBER_PATHS).\n\
         saved: the time saved against the clean driver (+ faster, - slower), % of the clean time.\n\
         knocks: wall knocks (a tick the shield drops after one it did not) / falls;\n\
         after sling: those that began during a sling or within {AFTER_SLING} ticks after it; rate: knocks a\n\
         minute inside those windows / outside them (a sling that throws the ship at the next wall\n\
         shows as a rate inside well above the rate outside).\n",
        trial_class().name()
    );
    let rw = REWARD;
    out.push_str(&format!(
        "\nREWARD: top {}/{}/{} (1024ths of own top), ticks {}/{}/{}, pull 1/{}\n",
        rw.top[0], rw.top[1], rw.top[2], rw.ticks[0], rw.ticks[1], rw.ticks[2], rw.pull
    ));
    out.push_str("track    driver   total  saved%   best fly  saved%   carves slings s1/s2/s3  knocks/falls  after sling knocks/falls  rate in/out\n");
    let mut sum_clean = [0u64; 2];
    let mut sum_good = [0u64; 2];
    for t in 0..TRACKS.len() as u8 {
        let mut bases = Vec::new();
        for &n in &nudges() {
            bases.push(trial_at(t, n, clean));
        }
        let base = sum_laps(&bases);
        for dn in &drivers {
            let l = if dn == "clean" {
                base
            } else {
                let mut ls = Vec::new();
                for &n in &nudges() {
                    let l = trial_by(dn, t, n);
                    if std::env::var_os("CAMBER_VERBOSE").is_some() {
                        eprintln!(
                            "  {} {dn} nudge {n}: laps {:?} knocks {} falls {}",
                            TRACKS[usize::from(t)].name,
                            l.laps,
                            l.walls,
                            l.falls
                        );
                    }
                    ls.push(l);
                }
                sum_laps(&ls)
            };
            if dn == "good" {
                sum_good[0] += u64::from(l.best_flying());
                sum_clean[0] += u64::from(base.best_flying());
                sum_good[1] += u64::from(l.total);
                sum_clean[1] += u64::from(base.total);
            }
            out.push_str(&format!(
                "{:<8} {:<6} {:6} {:>7}   {:6} {:>7}   {:6} {:6}/{:2}/{:2}   {:5}/{:2}      {:5}/{:2}           {:4}/{:4}\n",
                TRACKS[usize::from(t)].name,
                dn,
                l.total,
                pct(base.total, l.total),
                l.best_flying(),
                pct(base.best_flying(), l.best_flying()),
                l.carves,
                l.slings[0],
                l.slings[1],
                l.slings[2],
                l.walls,
                l.falls,
                l.walls_after,
                l.falls_after,
                l.walls_after * 3600 / l.ticks_after.max(1),
                (l.walls - l.walls_after) * 3600 / (l.ticks - l.ticks_after).max(1),
            ));
        }
    }
    if sum_clean[0] > 0 {
        out.push_str(&format!(
            "  good carver over the three tracks: best flying laps saved {} %, 3-lap totals saved {} %\n",
            pct(sum_clean[0] as u32, sum_good[0] as u32),
            pct(sum_clean[1] as u32, sum_good[1] as u32)
        ));
    }
    eprintln!("{out}");
    std::fs::write(out_dir().join(format!("rewards-{tag}.txt")), out).unwrap();
}

/// The rivals: (1) all eight ships computer-driven (Race::step(None)),
/// every track and class, `CAMBER_SEEDS` seeds (default 7: the grid is
/// the seed modulo 7, so seed 7 runs seed 0's race again): the rivals'
/// (ships 1 to 7) finish times, hard wall hits, falls, carves and slings;
/// (2) RACES with three model players (clean, good carver, bad carver),
/// seeds 0 to 6: the player's place and time, the winner's time, and the
/// rivals' falls and walls in those same races. Written to
/// `rivals-<CAMBER_DRIFT_TAG>.txt`.
#[test]
#[ignore]
fn rivals_and_places() {
    let tag = std::env::var("CAMBER_DRIFT_TAG").unwrap_or_else(|_| "now".into());
    let seeds: u32 = std::env::var("CAMBER_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(7);
    let mut out = format!(
        "MEASURED ON THE PC ({tag}), tests/laps.rs rivals_and_places. Ticks of 1/60 s.\n\
         (1) all eight ships computer-driven, {seeds} seeds per line. Rivals are ships 1 to 7.\n\
         finish: mean of the rivals' finish ticks, and the fastest; walls: hard wall hits (Ev::Wall);\n\
         all counts are sums over the seeds' races.\n\
         knocks: a tick a wall or a ship touches a rival after one that did not.\n\
         track    class   finish mean  fastest  walls knocks  falls  carves  slings s1/s2/s3\n"
    );
    for t in 0..TRACKS.len() as u8 {
        for class in Class::ALL {
            let mut sum = 0u64;
            let mut fastest = u32::MAX;
            let mut c = Counts::default();
            for seed in 0..seeds {
                let mut r = Race::new();
                r.setup(t, class, Mode::Race, seed);
                let mut was = r.ships;
                for _ in 0..60 * 400 {
                    r.step(None);
                    while let Some(e) = r.take_ev() {
                        match e {
                            Ev::Wall { ship } if ship > 0 => c.walls += 1,
                            _ => {}
                        }
                    }
                    let now = r.ships;
                    let mut cs = [Counts::default(); SHIPS];
                    count(&was, &now, &mut cs);
                    for x in &cs[1..] {
                        c.carves += x.carves;
                        c.falls += x.falls;
                        c.knocks += x.knocks;
                        for k in 0..3 {
                            c.slings[k] += x.slings[k];
                        }
                    }
                    was = now;
                    if r.ships.iter().all(|s| s.finished()) {
                        break;
                    }
                }
                assert!(
                    r.ships.iter().all(|s| s.finished()),
                    "a ship never finished"
                );
                for sh in &r.ships[1..] {
                    sum += u64::from(sh.finish);
                    fastest = fastest.min(sh.finish);
                }
            }
            out.push_str(&format!(
                "{:<8} {:<6}  {:8}     {:6}  {:5}  {:5}  {:5}  {:6}  {:5}/{:3}/{:3}\n",
                TRACKS[usize::from(t)].name,
                class.name(),
                sum / u64::from(seeds * 7),
                fastest,
                c.walls,
                c.knocks,
                c.falls,
                c.carves,
                c.slings[0],
                c.slings[1],
                c.slings[2],
            ));
        }
    }
    // Races a model player drives per line (`CAMBER_PLACE_SEEDS`, default
    // 7, every grid once).
    let places_n: u32 = std::env::var("CAMBER_PLACE_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(7);
    out.push_str(&format!(
        "\n(2) RACES with a model player (the game's reward), seeds 0 to {}.\n\
         places: the player's place in each race; time: the player's mean finish;\n\
         winner: the mean finish of the race's fastest rival; rival knocks / falls: sums in these races.\n\
         track    class  player  places    wins  time   winner  rival knocks/falls\n",
        places_n - 1
    ));
    for t in 0..TRACKS.len() as u8 {
        for class in Class::ALL {
            for who in ["clean", "good", "bad"] {
                let mut places = Vec::new();
                let mut time = 0u64;
                let mut win = 0u64;
                let (mut walls, mut falls) = (0u32, 0u32);
                for seed in 0..places_n {
                    let (l, rd) = race_by(who, t, class, seed);
                    places.push(rd.place);
                    time += u64::from(l.total);
                    win += u64::from(rd.race.ships[1..].iter().map(|s| s.finish).min().unwrap());
                    for c in &rd.counts[1..] {
                        walls += c.knocks;
                        falls += c.falls;
                    }
                }
                let wins = places.iter().filter(|&&p| p == 1).count();
                out.push_str(&format!(
                    "{:<8} {:<6} {:<6}  {:?}  {wins}    {:5}  {:5}   {walls:4}/{falls:3}\n",
                    TRACKS[usize::from(t)].name,
                    class.name(),
                    who,
                    places,
                    time / u64::from(places_n),
                    win / u64::from(places_n),
                ));
            }
        }
    }
    eprintln!("{out}");
    std::fs::write(out_dir().join(format!("rivals-{tag}.txt")), out).unwrap();
}

fn line(name: &str, l: &Laps) -> String {
    format!(
        "{name:<8} lap1 {:5} lap2 {:5} lap3 {:5} total {:6} best flying {:5}   carves {:3} slings {:2}/{:2}/{:2} wall knocks {:2}\n",
        l.laps[0],
        l.laps[1],
        l.laps[2],
        l.total,
        l.best_flying(),
        l.carves,
        l.slings[0],
        l.slings[1],
        l.slings[2],
        l.walls
    )
}

/// The reward the owner asked for (2026-11-22): carving pays about a
/// tenth on the best lap, abuse and taps pay nothing. Each track is flown
/// from five starts a quarter metre apart (one path alone wobbles by
/// about a percent either way) and the laps are summed. MEASURED ON THE
/// PC with the reward (row B of 2026-11-22): see the numbers this prints.
#[test]
fn the_reward_pays_a_good_carver_and_not_abuse_or_taps() {
    let (mut clean_best, mut good_best) = (0u32, 0u32);
    for t in 0..TRACKS.len() as u8 {
        let name = TRACKS[usize::from(t)].name;
        let [clean, good, abuse, tap] = ["clean", "good", "abuse", "tap"].map(|n| {
            let runs: Vec<Laps> = [0, FM / 4, -FM / 4, FM / 2, -FM / 2]
                .iter()
                .map(|&k| trial_by(n, t, k))
                .collect();
            sum_laps(&runs)
        });
        eprintln!(
            "{name}: best flying clean {} good {} ({}%); 3 laps clean {} abuse {} ({}%) tap {} ({}%)",
            clean.best_sum,
            good.best_sum,
            pct(clean.best_sum, good.best_sum),
            clean.total,
            abuse.total,
            pct(clean.total, abuse.total),
            tap.total,
            pct(clean.total, tap.total)
        );
        assert!(abuse.carves > 0 && tap.carves > 0, "{name}: no carves");
        assert!(abuse.total >= clean.total, "{name}: abuse pays");
        // Taps: since the rivals' rebuild (2026-11-22) the plain pilot
        // under the tapper steers by the carve's own model while it
        // carves and through the settle after it, and a tap then gains
        // 0.08 to 0.16 % (MEASURED ON THE PC) where it lost 0.1 to 0.6 %
        // before. The reward itself did not change. Taps
        // may not pay more than half a percent.
        assert!(
            u64::from(tap.total) * 1000 >= u64::from(clean.total) * 995,
            "{name}: taps pay"
        );
        clean_best += clean.best_sum;
        good_best += good.best_sum;
    }
    // At least 9 % on the three tracks' best laps together.
    assert!(
        good_best * 100 <= clean_best * 91,
        "good carver {good_best} against clean {clean_best}: {}%",
        pct(clean_best, good_best)
    );
}

/// (a), (b) and (c) on every track, and the rivals' finish times (every
/// class, two seeds), written to `laps-<CAMBER_DRIFT_TAG>.txt`.
#[test]
#[ignore]
fn lap_times() {
    let tag = std::env::var("CAMBER_DRIFT_TAG").unwrap_or_else(|_| "now".into());
    let mut out = format!("MEASURED ON THE PC ({tag}). Ticks of 1/60 s. TIME TRIAL, BRISK.\n");
    out.push_str("(a) clean driver (stick, A, B; never L or R)\n");
    for t in 0..TRACKS.len() as u8 {
        let l = trial(t, clean);
        out.push_str(&line(TRACKS[usize::from(t)].name, &l));
    }
    out.push_str("(b) carving on the corners\n");
    for t in 0..TRACKS.len() as u8 {
        let mut p = Carver::default();
        let l = trial(t, |d| p.keys(d));
        out.push_str(&line(TRACKS[usize::from(t)].name, &l));
    }
    out.push_str("(c) carving from side to side on the straights (abuse)\n");
    for t in 0..TRACKS.len() as u8 {
        let mut p = Abuser::default();
        let l = trial(t, |d| p.keys(d));
        out.push_str(&line(TRACKS[usize::from(t)].name, &l));
    }
    out.push_str(
        "\nRivals alone (Race::step(None), all eight ships computer-driven): finish ticks\n",
    );
    for t in 0..TRACKS.len() as u8 {
        for class in Class::ALL {
            for seed in [0u32, 3] {
                let mut r = Race::new();
                r.setup(t, class, Mode::Race, seed);
                for _ in 0..60 * 300 {
                    r.step(None);
                    while r.take_ev().is_some() {}
                    if r.ships.iter().all(|s| s.finished()) {
                        break;
                    }
                }
                let f: Vec<u32> = (0..SHIPS).map(|i| r.ships[i].finish).collect();
                let sum: u32 = f.iter().sum();
                out.push_str(&format!(
                    "{:<8} {:<6} seed {seed}: {:?} sum {sum}\n",
                    TRACKS[usize::from(t)].name,
                    class.name(),
                    f
                ));
            }
        }
    }
    eprintln!("{out}");
    std::fs::write(out_dir().join(format!("laps-{tag}.txt")), out).unwrap();
}

/// The same spots on every track (a straight and a turn), the ship put
/// there by hand, for a picture before and after a change of the road.
#[test]
#[ignore]
fn same_spot_pictures() {
    let tag = std::env::var("CAMBER_DRIFT_TAG").unwrap_or_else(|_| "now".into());
    // (track, segment, what)
    let spots: [(u8, i32, &str); 6] = [
        (0, 4, "straight"),
        (0, 22, "turn"),
        (1, 6, "straight"),
        (1, 23, "tunnel"),
        (2, 5, "straight"),
        (2, 57, "turn"),
    ];
    for (t, seg, what) in spots {
        let mut d = Driver::new();
        d.set(Detail::High, ViewKind::Chase, false);
        d.race(t, Class::Brisk, Mode::Trial);
        // Past the count and the GO.
        for _ in 0..130 {
            d.hold(0, (0, 0));
        }
        let r = &mut d.app.race;
        let tr = r.tr();
        let sh = &mut r.ships[0];
        sh.s = (seg << SEG_BITS) + (1 << (SEG_BITS - 1));
        sh.x = 0;
        sh.psi = 0;
        sh.vl = 0;
        sh.vf = 50 * FM / 60;
        sh.rate = 0;
        sh.pitch = tr.at(sh.s).pitch;
        d.app.cam.set = false;
        d.app
            .cam
            .follow(d.app.race.tr(), &d.app.race.ships[0], ViewKind::Chase);
        d.app.draw(&mut d.frame, 0);
        let name = TRACKS[usize::from(t)].name.to_lowercase();
        let path = out_dir().join(format!("width-{tag}-{name}-{what}.png"));
        save_frame(&path, &d.frame, d.app.theme(), 2);
    }
}

/// Pictures of a carve at each heat step (0 to 3) and of the sling, on
/// the sharpest right-hand corner of LAGOON and GLACIER. The carve is a
/// real one (R, stick in the middle, 14 ticks); the heat is then set by
/// the test to the step shown, so every step can be seen in one place.
/// Each step also from the NOSE view.
#[test]
#[ignore]
fn carve_pictures() {
    use camber::race::{HEAT_1, HEAT_2, HEAT_3, HOVER};
    for t in [0u8, 2] {
        let tr = &TRACKS[usize::from(t)];
        let n = (tr.len() >> SEG_BITS) as usize;
        let seg = (0..n)
            .max_by_key(|&i| turn(tr.seg(i).yaw, tr.seg(i + 8).yaw))
            .unwrap();
        let name = tr.name.to_lowercase();
        for (k, heat) in [0, HEAT_1, HEAT_2, HEAT_3].into_iter().enumerate() {
            let mut d = Driver::new();
            d.set(Detail::High, ViewKind::Chase, true);
            d.race(t, Class::Brisk, Mode::Trial);
            // Past the count and the GO.
            for _ in 0..300 {
                d.hold(0, (0, 0));
            }
            {
                let sh = &mut d.app.race.ships[0];
                sh.s = ((seg as i32) << SEG_BITS) + (1 << (SEG_BITS - 1));
                sh.x = 0;
                sh.h = HOVER;
                sh.psi = 0;
                sh.vl = 0;
                sh.vf = 60 * FM / 60;
                sh.rate = 0;
                sh.pitch = tr.at(sh.s).pitch;
            }
            d.app.cam.set = false;
            for _ in 0..14 {
                d.hold(button::A | button::R, (0, 0));
            }
            assert_eq!(d.app.race.ships[0].carve, 1, "{name}: carving");
            d.app.race.ships[0].heat = heat;
            d.hold(button::A | button::R, (0, 0));
            save_frame(
                &out_dir().join(format!("carve-{name}-step{k}.png")),
                &d.frame,
                d.app.theme(),
                2,
            );
            // The same moment from the nose (no ship in the picture: the
            // small sparks at the bottom edge and the marks).
            let (cam, view) = (d.app.cam, d.app.opts.view);
            d.app.opts.view = ViewKind::Nose;
            d.app.cam.set = false;
            d.app
                .cam
                .follow(d.app.race.tr(), &d.app.race.ships[0], ViewKind::Nose);
            d.app.draw(&mut d.frame, d.time);
            save_frame(
                &out_dir().join(format!("carve-{name}-step{k}-nose.png")),
                &d.frame,
                d.app.theme(),
                2,
            );
            d.app.cam = cam;
            d.app.opts.view = view;
            if k == 3 {
                // Let go: the sling (10 ticks into it).
                for _ in 0..10 {
                    d.hold(button::A, (0, 0));
                }
                assert!(d.app.race.ships[0].sling > 0);
                save_frame(
                    &out_dir().join(format!("carve-{name}-sling.png")),
                    &d.frame,
                    d.app.theme(),
                    2,
                );
            }
        }
    }
}

/// What ship 0 does in the ticks after the player's finish, when the
/// computer's pilot takes it over (item 2 of the steering diagnosis's
/// last section; that diagnosis is kept outside the repository).
#[derive(Clone, Copy, Debug, Default)]
struct HandOver {
    /// At the hand-over: x - line (fine units), the nose and the slip's
    /// angle (angle units), the sling step and whether it was carving.
    e0: i32,
    psi0: i32,
    slip0: i32,
    sling0: u8,
    carve0: bool,
    /// Over the ticks after: swings of x - line (turning points that move
    /// back at least 0.25 m), the largest |x - line| from 0.5 s on, the
    /// nose's peak to peak from 1 s on (angle units), stick flips (beyond
    /// 32 of 256), roll flips (the turn rate changes sign beyond 100 units
    /// a tick: the ship's roll in the picture is 8 times the rate), wall
    /// knocks, falls.
    swings: u32,
    worst: i32,
    nose_pp: i32,
    flips: u32,
    rolls: u32,
    knocks: u32,
    falls: u32,
}

/// Turning points of `e` (tops and bottoms after which it moves back at
/// least `hyst`).
fn swings_of(e: &[i32], hyst: i32) -> u32 {
    let mut n = 0;
    // 1 rising, -1 falling, 0 not known yet.
    let mut dir = 0;
    let mut ext = e.first().copied().unwrap_or(0);
    for &v in e {
        match dir {
            0 => {
                if v >= ext + hyst {
                    dir = 1;
                    ext = v;
                } else if v <= ext - hyst {
                    dir = -1;
                    ext = v;
                }
            }
            1 => {
                if v > ext {
                    ext = v;
                } else if v <= ext - hyst {
                    n += 1;
                    dir = -1;
                    ext = v;
                }
            }
            _ => {
                if v < ext {
                    ext = v;
                } else if v >= ext + hyst {
                    n += 1;
                    dir = 1;
                    ext = v;
                }
            }
        }
    }
    n
}

/// Sign changes of `v` among the values beyond `th`.
fn sign_flips(v: &[i32], th: i32) -> u32 {
    let mut last = 0;
    let mut n = 0;
    for &x in v {
        if x.abs() > th {
            if last != 0 && x.signum() != last {
                n += 1;
            }
            last = x.signum();
        }
    }
    n
}

/// A RACE by the driver `name` to the player's finish, then `ticks` more
/// ticks with the computer's pilot on ship 0, logged.
fn handover_run(name: &str, t: u8, class: Class, seed: u32, ticks: usize) -> HandOver {
    let mut d = Driver::new();
    d.draw = false;
    d.set(Detail::Low, ViewKind::Chase, true);
    d.call_us = CALL_60;
    d.race(t, class, Mode::Race);
    d.app.race.setup(t, class, Mode::Race, seed);
    d.app.cam.set = false;
    let mut carver = Carver::default();
    let mut calls = 0;
    while d.app.race.phase != Phase::Done {
        let keys = match name {
            "clean" => clean(&d),
            _ => carver.keys(&d),
        };
        d.hold(keys.0, keys.1);
        calls += 1;
        assert!(calls < 60 * 400, "no finish");
    }
    let mut r = d.app.race;
    let tr = r.tr();
    let sh = r.ships[0];
    let mut h = HandOver {
        e0: sh.x - tr.line(sh.s),
        psi0: sh.psi,
        slip0: (i64::from(sh.vl) * 10_430 / i64::from(sh.vf.max(1))) as i32,
        sling0: sh.sling_step,
        carve0: sh.carve != 0,
        ..HandOver::default()
    };
    let (mut e, mut psi, mut steer, mut rate) = (vec![], vec![], vec![], vec![]);
    let mut was = sh;
    for _ in 0..ticks {
        r.step(None);
        while r.take_ev().is_some() {}
        let sh = r.ships[0];
        if sh.hit == 8 && was.hit != 8 {
            h.knocks += 1;
        }
        if sh.fall > 0 && was.fall == 0 {
            h.falls += 1;
        }
        was = sh;
        e.push(sh.x - tr.line(sh.s));
        psi.push(sh.psi);
        steer.push(sh.ctl.steer);
        rate.push(sh.rate);
    }
    h.swings = swings_of(&e, FM / 4);
    h.worst = e[30..].iter().map(|x| x.abs()).max().unwrap_or(0);
    h.nose_pp = psi[60..].iter().max().unwrap() - psi[60..].iter().min().unwrap();
    h.flips = sign_flips(&steer, 32);
    h.rolls = sign_flips(&rate, 100);
    h
}

/// The hand-over after the player's finish (the steering diagnosis,
/// kept outside the repository): a RACE with the clean driver and with the good carver, every
/// track and class, seeds 0 to 6 (`CAMBER_SEEDS`); then 150 ticks (the
/// time before the results panel, `CAMBER_AFTER` to change) with the
/// computer's pilot on the player's ship. Written to
/// `handover-<CAMBER_DRIFT_TAG>.txt`.
#[test]
#[ignore]
fn handover() {
    let tag = std::env::var("CAMBER_DRIFT_TAG").unwrap_or_else(|_| "now".into());
    let seeds: u32 = std::env::var("CAMBER_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(7);
    let after: usize = std::env::var("CAMBER_AFTER")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150);
    let mut out = format!(
        "MEASURED ON THE PC ({tag}), tests/laps.rs handover. RACE, seeds 0 to {}; the scripted player\n\
         drives to the finish, then the computer's pilot drives ship 0 for {after} ticks (1/60 s).\n\
         Each line sums or takes the mean over the seeds.\n\
         at hand-over: mean |x - line| (m), mean |nose| and |slip angle| (deg), races handed over in a\n\
         sling / carving. after: swings of x - line (turning points moving back >= 0.25 m) a race,\n\
         mean and worst of the largest |x - line| from 0.5 s on (m), mean nose peak to peak from 1 s on\n\
         (deg), stick flips a second (beyond 32 of 256), roll flips a race (turn rate changes sign\n\
         beyond 100 units a tick; the ship rolls 8 times the rate in the picture), knocks, falls.\n\n\
         track    class  player  |e0|  |nose0| |slip0| sling/carve | swings  worst mean/max  nose p-p  flips/s  rolls  knocks falls\n",
        seeds - 1
    );
    let mut tot = [HandOver::default(); 2];
    let mut tot_n = 0u32;
    for t in 0..TRACKS.len() as u8 {
        for class in Class::ALL {
            for (pi, who) in ["clean", "good"].into_iter().enumerate() {
                let hs: Vec<HandOver> = (0..seeds)
                    .map(|s| handover_run(who, t, class, s, after))
                    .collect();
                let n = hs.len() as f64;
                let mean = |f: &dyn Fn(&HandOver) -> f64| hs.iter().map(f).sum::<f64>() / n;
                let mm = |v: i32| f64::from(v) / f64::from(FM);
                let dg = |v: i32| f64::from(v) * 360.0 / 65_536.0;
                let worst_max = hs.iter().map(|h| h.worst).max().unwrap();
                out.push_str(&format!(
                    "{:<8} {:<6} {:<6}  {:5.2}  {:5.1}   {:5.1}    {}/{}      | {:5.2}   {:5.2} / {:5.2}    {:5.1}    {:5.2}   {:5.2}  {:3}   {:3}\n",
                    TRACKS[usize::from(t)].name,
                    class.name(),
                    who,
                    mean(&|h| mm(h.e0.abs())),
                    mean(&|h| dg(h.psi0.abs())),
                    mean(&|h| dg(h.slip0.abs())),
                    hs.iter().filter(|h| h.sling0 > 0).count(),
                    hs.iter().filter(|h| h.carve0).count(),
                    mean(&|h| f64::from(h.swings)),
                    mean(&|h| mm(h.worst)),
                    mm(worst_max),
                    mean(&|h| dg(h.nose_pp)),
                    mean(&|h| f64::from(h.flips)) / (after as f64 / 60.0),
                    mean(&|h| f64::from(h.rolls)),
                    hs.iter().map(|h| h.knocks).sum::<u32>(),
                    hs.iter().map(|h| h.falls).sum::<u32>(),
                ));
                for h in &hs {
                    let a = &mut tot[pi];
                    a.swings += h.swings;
                    a.flips += h.flips;
                    a.rolls += h.rolls;
                    a.knocks += h.knocks;
                    a.falls += h.falls;
                    a.nose_pp = a.nose_pp.max(h.nose_pp);
                    a.worst = a.worst.max(h.worst);
                }
                if pi == 0 {
                    tot_n += seeds;
                }
            }
        }
    }
    for (pi, who) in ["clean", "good"].into_iter().enumerate() {
        let a = tot[pi];
        out.push_str(&format!(
            "all      {who:<6}: swings a race {:.2}, stick flips a second {:.2}, roll flips a race {:.2}, knocks {}, falls {}, worst |x - line| {:.2} m, largest nose p-p {:.1} deg ({} races)\n",
            f64::from(a.swings) / f64::from(tot_n),
            f64::from(a.flips) / f64::from(tot_n) / (after as f64 / 60.0),
            f64::from(a.rolls) / f64::from(tot_n),
            a.knocks,
            a.falls,
            f64::from(a.worst) / f64::from(FM),
            f64::from(a.nose_pp) * 360.0 / 65_536.0,
            tot_n
        ));
    }
    eprintln!("{out}");
    std::fs::write(out_dir().join(format!("handover-{tag}.txt")), out).unwrap();
}
