//! MEASURING ONLY: the `merc_site_test` start (BOARD-G1.md instruction
//! 1). Runs only in a build with `RUSTFLAGS='--cfg merc_site_test'`
//! (for example `--target-dir target/variant-b`); empty otherwise. With
//! the cfg the assist also starts on HOLD below (planet::ready), so the
//! other planet tests are not meant to run with it.
//!
//! The route of step G2's site pad script (kept outside the
//! repository), tick by tick (one call = one tick here, as the game
//! counts real time): PLAY, LAUNCH, no key through the ring and the
//! entry; the 120 m view held still by HOLD with UP held to fly over the
//! pad; Y twice (OFF, then SINK): the assist sets the ship down on the
//! pad with no steering; LANDED; B held; the take-off; space.
//!
//! Step G3: the site pad goes on foot (`.local/mercator/shots/planet-g3/
//! G3-site.pad`, which `the_g3_site_pad_walks_round_the_ship_and_cuts_a_seam`
//! writes): out of the hatch, a walk round the ship, to the nearest seam,
//! the seam cut, its chunks picked up, back to the hatch, B held, space.
#![cfg(merc_site_test)]

mod common;

use common::Driver;
use game_kit::button;
use mercator::app::Scene;
use mercator::planet::lander::{self, ASSIST_SINK, FINE, Phase};

/// UP held this long from the flight's start (ticks).
pub const UP_TICKS: u32 = 15 * 60;
/// The 120 m view (HOLD) this long from the flight's start (ticks).
pub const HOVER_TICKS: u32 = 20 * 60;
/// The second Y this many ticks after the first.
pub const Y_GAP: u32 = 6;

fn metres(v: i32) -> f64 {
    f64::from(v) / f64::from(FINE)
}

/// What one run of the route measured (printed through Debug).
#[derive(Debug)]
#[allow(dead_code)]
struct Run {
    to_below: u32,
    entry: u32,
    hover: (f64, f64),
    off_at_hover_end: f64,
    off: f64,
    hull: u8,
    heat: i32,
    down: u32,
}

/// One run from LAUNCH; `up`: ticks of UP.
fn route(up: u32, all: bool) -> Run {
    let mut d = Driver::new();
    d.start();
    assert!(
        d.app.world.bodies.site.is_some(),
        "the first system has a site"
    );
    d.app.sel = 0;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Flight);
    let mut to_below = 0;
    while d.app.scene != Scene::Below {
        d.idle(1);
        to_below += 1;
        assert!(to_below < 600, "below within 10 s ({to_below})");
    }
    let mut entry = 0;
    while d.app.below.phase == Phase::Entry {
        d.idle(1);
        entry += 1;
    }
    let off = |d: &Driver| {
        let b = &d.app.below;
        metres(lander::hspeed([b.p[0], 0, b.p[2]]))
    };
    let mut hover = (f64::MAX, 0.0f64);
    let mut heat = 0;
    for t in 0..HOVER_TICKS {
        d.hold(if t < up { button::UP } else { 0 });
        let h = metres(lander::alt(&d.app.below));
        hover = (hover.0.min(h), hover.1.max(h));
        heat = heat.max(d.app.below.heat / lander::HEAT_UNIT);
        if all && t % 120 == 0 {
            println!("  hover {t}: alt {h:.1} m, off {:.1} m", off(&d));
        }
        assert_eq!(d.app.scene, Scene::Below);
    }
    let off_at_hover_end = off(&d);
    d.tap(button::Y);
    d.idle(Y_GAP - 2);
    d.tap(button::Y);
    assert_eq!(d.app.below.assist, ASSIST_SINK);
    let mut down = Y_GAP;
    while d.app.below.phase != Phase::Landed {
        d.idle(1);
        heat = heat.max(d.app.below.heat / lander::HEAT_UNIT);
        down += 1;
        assert!(down < 60 * 60, "landed within a minute");
        assert_ne!(d.app.scene, Scene::Lost);
    }
    let run = Run {
        to_below,
        entry,
        hover,
        off_at_hover_end,
        off: off(&d),
        hull: d.app.below.hull,
        heat,
        down,
    };
    if all {
        // LANDED 20 s (step G3: the pilot steps out after a second and
        // stands at the hatch), B held, the take-off, space.
        d.idle(20 * 60);
        assert_eq!(d.app.below.phase, Phase::Foot);
        let mut n = 0;
        while d.app.scene == Scene::Below {
            d.hold(button::B);
            n += 1;
            assert!(n < 10 * 60);
        }
        d.hold(0);
        println!("  B held {n} ticks to space; scene {:?}", d.app.scene);
        assert_eq!(d.app.scene, Scene::Flight);
    }
    run
}

/// PLAY, LAUNCH, no key: below within 10 s; the script's route holds
/// 120 m, lands on the pad and goes back to space.
#[test]
fn the_site_route_lands_on_the_pad() {
    let r = route(UP_TICKS, true);
    println!("route: {r:?}");
    // HOLD keeps the height over the ground, which moves under the ship.
    assert!(r.hover.0 > 105.0 && r.hover.1 < 130.0, "about 120 m held");
    assert!(r.off < 6.0, "on the pad ({:.1} m off)", r.off);
    assert_eq!(r.hull, 100);
}

/// The route's margins: UP held a little shorter or longer (a board's
/// calls and the pad's reports do not fall on the same ticks).
#[test]
fn the_site_route_margins() {
    for up in [UP_TICKS - 30, UP_TICKS - 6, UP_TICKS + 6, UP_TICKS + 30] {
        let r = route(up, false);
        println!("UP {up} ticks: {r:?}");
        assert!(r.off < 12.0, "on the pad");
    }
}

// ---- Step G3: the site pad on foot ------------------------------------------------

use mercator::app::{DEAD_ZONE, Keys};
use mercator::fixed::{cos, sin};
use mercator::planet::cutter::{self, Lock};
use mercator::planet::loose::CHUNK_NONE;
use mercator::planet::tune::{SEAM_STAGES, SEAMS};
use mercator::planet::walker;
use mercator::world::M;

/// The pad's LAUNCH tap (ms from the pad's start): tick 0 here.
const LAUNCH_MS: u32 = 6000;

/// The pad's ms as the game's tick after LAUNCH (60 a second).
fn tick_of(ms: u32) -> u32 {
    (ms - LAUNCH_MS) * 60 / 1000
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    /// Down and up in back-to-back reports (`tap`).
    Tap(u32),
    Press(u32),
    Release(u32),
    Stick(i16, i16),
}

fn name(b: u32) -> &'static str {
    match b {
        button::A => "south",
        button::B => "east",
        button::X => "west",
        button::Y => "north",
        button::UP => "up",
        _ => panic!("no name for {b:#x}"),
    }
}

/// The pad's buttons and stick as the game sees them, one call a tick.
#[derive(Default)]
struct Player {
    held: u32,
    pressed: u32,
    up_after: u32,
    stick: (i16, i16),
    tick: u32,
    next: usize,
}

impl Player {
    fn act(&mut self, a: Act) {
        match a {
            Act::Tap(b) => {
                self.held |= b;
                self.pressed |= b;
                self.up_after |= b;
            }
            Act::Press(b) => {
                self.held |= b;
                self.pressed |= b;
            }
            Act::Release(b) => self.held &= !b,
            Act::Stick(x, y) => self.stick = (x, y),
        }
    }

    /// Calls until tick `t`, each after the events of its tick
    /// (`shift`: an event's tick moved by this, per event).
    fn run_to(&mut self, d: &mut Driver, ev: &[(u32, Act)], t: u32, shift: &dyn Fn(usize) -> i32) {
        while self.tick < t {
            while self.next < ev.len()
                && (tick_of(ev[self.next].0) as i32 + shift(self.next)).max(0) as u32 <= self.tick
            {
                self.act(ev[self.next].1);
                self.next += 1;
            }
            let k = Keys {
                held: self.held,
                pressed: self.pressed,
                left: self.stick,
                ..Keys::default()
            };
            d.call_keys(k);
            self.pressed = 0;
            self.held &= !self.up_after;
            self.up_after = 0;
            self.tick += 1;
        }
    }
}

/// The stick (pad values, y down) that walks along world (ux, uz) at
/// `k` of the top speed, in the camera's frame now.
fn stick_for(d: &Driver, ux: f64, uz: f64, k: f64) -> (i16, i16) {
    let b = &d.app.below;
    let (s, c) = (
        f64::from(sin(b.cam_yaw)) / 16384.0,
        f64::from(cos(b.cam_yaw)) / 16384.0,
    );
    let n = (ux * ux + uz * uz).sqrt().max(1e-9);
    let (ux, uz) = (ux / n * k, uz / n * k);
    let pr = ux * c - uz * s;
    let pf = ux * s + uz * c;
    let st = |v: f64| -> i16 {
        if v.abs() < 1e-3 {
            0
        } else {
            let m = f64::from(DEAD_ZONE) + v.abs() * f64::from(32_767 - DEAD_ZONE) + 1.0;
            (v.signum() * m.min(32_767.0)).round() as i16
        }
    };
    (st(pr), -st(pf))
}

fn pilot_xz(d: &Driver) -> [i32; 2] {
    let p = d.app.below.pilot.at();
    [p[0], p[2]]
}

/// The site pad's scenes: (name, start ms, end ms).
type Scenes = Vec<(&'static str, u32, u32)>;

/// Writes the pad: the G2 route to LANDED, then on foot steered by the
/// nominal run every 500 ms (the pad itself is blind: on the board the
/// same inputs give the same walk). Returns the events and the scenes.
fn g3_pad() -> (Vec<(u32, Act)>, Scenes, usize) {
    let mut ev: Vec<(u32, Act)> = vec![
        (LAUNCH_MS, Act::Tap(button::A)),
        (15_700, Act::Press(button::UP)),
        (30_700, Act::Release(button::UP)),
        (35_700, Act::Tap(button::Y)),
        (35_850, Act::Tap(button::Y)),
    ];
    let mut d = Driver::new();
    d.start();
    d.app.sel = 0;
    let mut p = Player::default();
    let none = |_: usize| 0;
    let mut scenes: Scenes = vec![
        // The dock, the launch and the way to the ring are one scene
        // (each scene lasts 15 s or more, BOARD-G2.md 3.5), and the
        // descent starts 300 ms before the two Y taps that start it.
        ("dock, launch, space, the ring, the entry", 0, 16_000),
        ("120 m view, UP held", 16_000, 35_400),
        ("assisted descent (Y, Y: OFF, SINK)", 35_400, 54_000),
    ];
    // Out of the hatch: stand still at it, the ship beside.
    let mut ms = 70_000;
    p.run_to(&mut d, &ev, tick_of(ms), &none);
    assert_eq!(d.app.below.phase, Phase::Foot);
    scenes.push(("LANDED, out of the hatch, standing", 54_000, ms));
    // A walk round the ship, 9 m out, steered every 500 ms toward a point
    // 0.4 rad ahead on the circle (anticlockwise seen from above).
    let ship = {
        let a = lander::at(&d.app.below);
        [a[0], a[2]]
    };
    let r = f64::from(9 * M);
    let angle = |q: [i32; 2]| f64::from(q[1] - ship[1]).atan2(f64::from(q[0] - ship[0]));
    let start = angle(pilot_xz(&d));
    let mut turned = 0.0f64;
    let mut last = start;
    ms += 300;
    while turned < std::f64::consts::TAU - 0.3 {
        let q = pilot_xz(&d);
        let a = angle(q);
        let mut da = a - last;
        if da < -std::f64::consts::PI {
            da += std::f64::consts::TAU;
        } else if da > std::f64::consts::PI {
            da -= std::f64::consts::TAU;
        }
        turned += da;
        last = a;
        let goal = a + 0.4;
        let (gx, gz) = (
            f64::from(ship[0]) + r * goal.cos(),
            f64::from(ship[1]) + r * goal.sin(),
        );
        let st = stick_for(&d, gx - f64::from(q[0]), gz - f64::from(q[1]), 1.0);
        ev.push((ms, Act::Stick(st.0, st.1)));
        ms += 500;
        p.run_to(&mut d, &ev, tick_of(ms), &none);
        assert!(ms < 89_000, "the circle within 19 s");
    }
    ev.push((ms, Act::Stick(0, 0)));
    let circle_end = 90_000;
    p.run_to(&mut d, &ev, tick_of(circle_end), &none);
    scenes.push(("walk round the ship", 70_000, circle_end));
    // To the nearest seam (not the ledge's): 6 m short of it.
    let me = pilot_xz(&d);
    let seam = (0..SEAMS - 1)
        .min_by_key(|&k| walker::apart(me, d.app.below.places.seams[k]))
        .unwrap();
    let to = d.app.below.places.seams[seam];
    ms = circle_end + 300;
    loop {
        let q = pilot_xz(&d);
        let dist = walker::apart(q, to);
        if dist <= 6 * M {
            break;
        }
        let k = if dist < 9 * M { 0.4 } else { 1.0 };
        let st = stick_for(&d, f64::from(to[0] - q[0]), f64::from(to[1] - q[1]), k);
        ev.push((ms, Act::Stick(st.0, st.1)));
        ms += 500;
        p.run_to(&mut d, &ev, tick_of(ms), &none);
        assert!(ms < circle_end + 60_000, "at the seam within a minute");
    }
    ev.push((ms, Act::Stick(0, 0)));
    let walk_end = (ms + 500).div_ceil(1000) * 1000 + 1000;
    let walk_end = walk_end.max(circle_end + 15_000);
    p.run_to(&mut d, &ev, tick_of(walk_end), &none);
    scenes.push(("walk to the seam", circle_end, walk_end));
    // The cut: X held 3 s, then its chunks picked up.
    assert_eq!(
        cutter::find(&d.app.below),
        Lock::Seam(seam as u8),
        "the seam ahead"
    );
    ms = walk_end + 300;
    ev.push((ms, Act::Press(button::X)));
    ms += 3_000;
    ev.push((ms, Act::Release(button::X)));
    ms += 1_000;
    p.run_to(&mut d, &ev, tick_of(ms), &none);
    assert!(d.app.below.seams[seam] >= SEAM_STAGES, "the seam cracked");
    let cut_from = ms;
    while ms < cut_from + 10_000 {
        let q = pilot_xz(&d);
        let b = &d.app.below;
        let next = b
            .chunks
            .iter()
            .filter(|c| c.state != CHUNK_NONE && !c.full)
            .map(|c| {
                let a = c.at();
                [a[0], a[2]]
            })
            .min_by_key(|&a| walker::apart(q, a));
        let st = match next {
            Some(a) => stick_for(&d, f64::from(a[0] - q[0]), f64::from(a[1] - q[1]), 0.6),
            None => (0, 0),
        };
        ev.push((ms, Act::Stick(st.0, st.1)));
        ms += 500;
        p.run_to(&mut d, &ev, tick_of(ms), &none);
    }
    ev.push((ms, Act::Stick(0, 0)));
    let cut_end = (ms + 1000).div_ceil(1000) * 1000;
    let cut_end = cut_end.max(walk_end + 15_000);
    p.run_to(&mut d, &ev, tick_of(cut_end), &none);
    scenes.push(("the seam cut, its chunks picked up", walk_end, cut_end));
    // Back to the hatch (within 1 m), then still.
    ms = cut_end + 300;
    loop {
        let q = pilot_xz(&d);
        let h = walker::hatch(&d.app.below);
        let dist = walker::apart(q, h);
        if dist <= M {
            break;
        }
        let k = if dist < 4 * M { 0.35 } else { 1.0 };
        let st = stick_for(&d, f64::from(h[0] - q[0]), f64::from(h[1] - q[1]), k);
        ev.push((ms, Act::Stick(st.0, st.1)));
        ms += 500;
        p.run_to(&mut d, &ev, tick_of(ms), &none);
        assert!(ms < cut_end + 60_000, "back within a minute");
    }
    ev.push((ms, Act::Stick(0, 0)));
    let back_end = (ms + 1000).div_ceil(1000) * 1000;
    let back_end = back_end.max(cut_end + 15_000);
    p.run_to(&mut d, &ev, tick_of(back_end), &none);
    scenes.push(("walk back to the hatch", cut_end, back_end));
    // B held 1 s at the hatch: aboard, the take-off, space.
    ev.push((back_end + 300, Act::Press(button::B)));
    ev.push((back_end + 1300, Act::Release(button::B)));
    let end = back_end + 16_000;
    scenes.push(("B held, aboard, the take-off, space", back_end, end));
    (ev, scenes, seam)
}

/// G3's pad head.
const G3_HEAD: &str = "# MERCATOR G3, MEASURING ONLY: the way down, then out of the ship, for\n\
         # the -site build (builds/G3-site; --cfg merc_site_test). Written by\n\
         # games/mercator/tests/site_test.rs (the_g3_site_pad_...), steered by\n\
         # the nominal run on the PC; the pad itself is blind.\n\
         # Start it right after the MERCATOR tile was opened (its title showing,\n\
         # PLAY selected). Times in ms from the pad's start.\n";

/// The pad's text (boardctl's script format).
fn pad_text(ev: &[(u32, Act)], scenes: &Scenes, seam: usize) -> String {
    pad_text_with(G3_HEAD, "the nearest after the circle", ev, scenes, seam)
}

/// The pad's text under `head`.
fn pad_text_with(head: &str, why: &str, ev: &[(u32, Act)], scenes: &Scenes, seam: usize) -> String {
    let mut t = String::from(head);
    t += &format!("# The seam cut: number {seam} ({why}).\n# Scenes (ms):\n");
    for (n, a, b) in scenes {
        t += &format!("#   {a:6} .. {b:6}  {n}\n");
    }
    t += "0      connect switch\n0      repeat 10\n1000   tap south\n";
    for &(ms, a) in ev {
        let line = match a {
            Act::Tap(b) => format!("tap {}", name(b)),
            Act::Press(b) => format!("press {}", name(b)),
            Act::Release(b) => format!("release {}", name(b)),
            Act::Stick(x, y) => format!("stick left {x} {y}"),
        };
        t += &format!("{ms:<6} {line}\n");
    }
    let end = scenes.last().unwrap().2;
    t += &format!("{end:<6} repeat 10\n");
    t
}

/// What one replay of the pad did.
#[derive(Debug, Default)]
struct Foot {
    circle_deg: f64,
    cracked: bool,
    picked: u32,
    at_hatch_at_b: bool,
    in_space: bool,
    sunk: bool,
}

/// Replays the pad (each event's tick moved by `shift`) and measures.
fn replay(ev: &[(u32, Act)], scenes: &Scenes, seam: usize, shift: &dyn Fn(usize) -> i32) -> Foot {
    let mut d = Driver::new();
    d.start();
    d.app.sel = 0;
    let mut p = Player::default();
    let mut f = Foot::default();
    let (c0, c1) = (scenes[4].1, scenes[4].2);
    let b_at = scenes[8].1 + 300;
    let end = scenes[8].2;
    let mut last: Option<f64> = None;
    let hold0 = d.app.world.car.hold.total();
    for t in 0..tick_of(end) {
        p.run_to(&mut d, ev, t + 1, shift);
        let b = &d.app.below;
        if d.app.scene == mercator::app::Scene::Below && planet_on_foot(b) {
            f.sunk |= walker::under(b) > 0;
            if (tick_of(c0)..tick_of(c1)).contains(&t) {
                let s = lander::at(b);
                let q = b.pilot.at();
                let a = f64::from(q[2] - s[2]).atan2(f64::from(q[0] - s[0]));
                if let Some(l) = last {
                    let mut da = a - l;
                    if da < -std::f64::consts::PI {
                        da += std::f64::consts::TAU;
                    } else if da > std::f64::consts::PI {
                        da -= std::f64::consts::TAU;
                    }
                    f.circle_deg += da.to_degrees();
                }
                last = Some(a);
            }
            f.cracked |= b.seams[seam] >= SEAM_STAGES;
            f.picked = u32::from(b.haul);
        }
        if t == tick_of(b_at) + 2 {
            f.at_hatch_at_b = walker::at_hatch(&d.app.below);
        }
    }
    f.in_space = d.app.scene == mercator::app::Scene::Flight;
    assert_eq!(
        d.app.world.car.hold.total(),
        hold0 + f.picked,
        "the chunks went into the hold"
    );
    f
}

fn planet_on_foot(b: &mercator::planet::Below) -> bool {
    mercator::planet::on_foot(b)
}

/// The G3 site pad: a walk round the ship, to a seam, the cut, the
/// chunks, back and up; the same with every foot event's time moved by
/// up to 0.1 s and 0.5 s (a board's calls and the pad's reports do not
/// fall on the PC's ticks). Writes the pad to the shots folder.
#[test]
fn the_g3_site_pad_walks_round_the_ship_and_cuts_a_seam() {
    let (ev, scenes, seam) = g3_pad();
    let text = pad_text(&ev, &scenes, seam);
    let dir = common::shots().join("planet-g3");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("G3-site.pad"), &text).unwrap();
    for (n, a, b) in &scenes {
        println!(
            "scene {a:6} .. {b:6} ms ({:5.1} s): {n}",
            f64::from(b - a) / 1000.0
        );
        // BOARD-G2.md 3.5: every scene 15 s or more, and no event of
        // the route within 300 ms of a scene border (the lead splits
        // the pad there).
        assert!(b - a >= 15_000, "scene {n} is shorter than 15 s");
        for (at, act) in &ev {
            for edge in [*a, *b] {
                assert!(
                    at.abs_diff(edge) >= 300,
                    "{act:?} at {at} ms is within 300 ms of a border ({edge})"
                );
            }
        }
    }
    let first_foot = ev.iter().position(|e| e.0 > 54_000).unwrap();
    let nominal = replay(&ev, &scenes, seam, &|_| 0);
    println!("nominal: {nominal:?}");
    assert!(nominal.circle_deg > 330.0, "round the ship");
    assert!(nominal.cracked && nominal.picked > 0);
    assert!(nominal.at_hatch_at_b && nominal.in_space && !nominal.sunk);
    for (k, m) in [(1u32, 6), (2, 30)] {
        for sign in [-1i32, 1] {
            let shift = |i: usize| {
                if i < first_foot {
                    0
                } else {
                    // Each foot event moved by a different amount, up to m.
                    let h = mercator::fixed::hash(i as u32, k * 7 + u32::from(sign > 0));
                    sign * (h % (m as u32 + 1)) as i32
                }
            };
            let r = replay(&ev, &scenes, seam, &shift);
            println!("foot events moved up to {} ticks: {r:?}", sign * m);
            assert!(r.circle_deg > 300.0, "round the ship");
            assert!(r.cracked, "the seam cracked");
            assert!(r.at_hatch_at_b && r.in_space, "back aboard and up");
            assert!(!r.sunk);
        }
    }
    // UP released earlier or later (the ship lands up to 6 m elsewhere on
    // the pad, so the seam is that much off the blind walk): within 0.1 s
    // the whole visit holds; 0.5 s is reported.
    let up = ev
        .iter()
        .position(|e| e.1 == Act::Release(button::UP))
        .unwrap();
    for m in [-30, -6, 6, 30] {
        let r = replay(&ev, &scenes, seam, &|i| if i == up { m } else { 0 });
        println!("UP released {m} ticks off: {r:?}");
        assert!(r.in_space && !r.sunk);
        if m.abs() <= 6 {
            assert!(r.cracked && r.at_hatch_at_b && r.circle_deg > 300.0);
        }
    }
}

// ---- Step G4: the site pad with a drone fight ------------------------------------

use mercator::planet::drones::{self, AIM};
use mercator::planet::tune::DRONE_AIM;

/// G4's pad head.
const G4_HEAD: &str = "# MERCATOR G4, MEASURING ONLY: the way down, out of the ship, a drone\n\
         # fight at a guarded seam, the cut, back and up, for the -site build\n\
         # (builds/G4-site; --cfg merc_site_test). Written by\n\
         # games/mercator/tests/site_test.rs (the_g4_site_pad_...), steered by\n\
         # the nominal run on the PC; the pad itself is blind.\n\
         # Start it right after the MERCATOR tile was opened (its title showing,\n\
         # PLAY selected). Times in ms from the pad's start.\n";

/// The fight's events: one every 250 ms.
const FIGHT_EVERY: u32 = 250;
/// The fight's first part (BOARD-G3.md 4.5: the heaviest drone fight
/// held 15 s or more): the pilot stands 12 to 20 m from the drone, no
/// cut and no side step, and takes its bolts (8 points each at danger
/// 0: about six of them, so the suit keeps about half). Standing still
/// is hit every time (`tests/drones.rs`), so a blind pad shifted by a
/// few ticks plays the same; open-loop dodges were not robust to 0.1 s.
const STAND_FOR: u32 = 16_000;

/// The nearest drone that is up, as (index, distance across).
fn nearest_drone(d: &Driver) -> Option<(usize, i32)> {
    let b = &d.app.below;
    let me = pilot_xz(d);
    (0..drones::N)
        .filter(|&k| b.drones[k].alive())
        .map(|k| (k, walker::apart(me, [b.drones[k].p[0], b.drones[k].p[2]])))
        .min_by_key(|&(_, r)| r)
}

/// A drone within `r` that is late in its aim or has a bolt in flight
/// (the pilot steps aside), as its index.
fn threat(d: &Driver, r: i32) -> Option<usize> {
    let b = &d.app.below;
    let me = pilot_xz(d);
    (0..drones::N).find(|&k| {
        let q = &b.drones[k];
        let late = q.mode == AIM && q.t >= DRONE_AIM - 24;
        (late || q.bolt_t > 0) && walker::apart(me, [q.p[0], q.p[2]]) < r
    })
}

/// Writes the G4 pad: the G2 route to LANDED, standing at the hatch, a
/// walk toward the seam of the site's first drone, the fight (every
/// 250 ms: aside when a drone is late in its aim or its bolt flies, else
/// toward the nearest drone with X held), the seam's cut, back to the
/// hatch, B held, space. Returns the events, the scenes and the seam.
fn g4_pad() -> (Vec<(u32, Act)>, Scenes, usize) {
    let mut ev: Vec<(u32, Act)> = vec![
        (LAUNCH_MS, Act::Tap(button::A)),
        (15_700, Act::Press(button::UP)),
        (30_700, Act::Release(button::UP)),
        (35_700, Act::Tap(button::Y)),
        (35_850, Act::Tap(button::Y)),
    ];
    let mut d = Driver::new();
    d.start();
    d.app.sel = 0;
    let mut p = Player::default();
    let none = |_: usize| 0;
    let mut scenes: Scenes = vec![
        ("dock, launch, space, the ring, the entry", 0, 16_000),
        ("120 m view, UP held", 16_000, 35_400),
        ("assisted descent (Y, Y: OFF, SINK)", 35_400, 54_000),
    ];
    let mut ms = 70_000;
    p.run_to(&mut d, &ev, tick_of(ms), &none);
    assert_eq!(d.app.below.phase, Phase::Foot);
    scenes.push(("LANDED, out of the hatch, standing", 54_000, ms));
    // The guarded seam: the nearest drone's.
    let (first, _) = nearest_drone(&d).expect("the site has drones");
    let seam = usize::from(d.app.below.drones[first].home) % SEAMS;
    let to = d.app.below.places.seams[seam];
    println!(
        "danger {}, drones {}, seam {seam}",
        d.app.below.danger,
        drones::up(&d.app.below)
    );
    // Toward it until a drone aims or 16 m short of the seam.
    ms += 300;
    loop {
        let q = pilot_xz(&d);
        let aiming = d.app.below.drones.iter().any(|q| q.mode == AIM);
        if walker::apart(q, to) <= 16 * M || aiming {
            break;
        }
        let st = stick_for(&d, f64::from(to[0] - q[0]), f64::from(to[1] - q[1]), 1.0);
        ev.push((ms, Act::Stick(st.0, st.1)));
        ms += 500;
        p.run_to(&mut d, &ev, tick_of(ms), &none);
        assert!(ms < 140_000, "at the seam's drones within a minute");
    }
    let walk_end = (ms + 300).div_ceil(1000) * 1000;
    let walk_end = walk_end.max(70_000 + 15_000);
    // Keep walking (same stick) up to the border; the fight starts there.
    p.run_to(&mut d, &ev, tick_of(walk_end), &none);
    scenes.push(("walk toward the guarded seam", 70_000, walk_end));
    // The fight: at least 20 s, until no drone of the seam is up (or
    // 40 s).
    let suit0 = d.app.below.pilot.suit;
    ms = walk_end + 300;
    let mut x_held = false;
    let mut side = 1.0f64;
    loop {
        let b = &d.app.below;
        let q = pilot_xz(&d);
        let near = nearest_drone(&d).filter(|&(_, r)| r < 40 * M);
        let left = ms - walk_end;
        if (near.is_none() && left >= 20_000) || left >= 40_000 {
            break;
        }
        let stand = near.is_some() && left < STAND_FOR;
        let (st, x) = if let Some(k) = threat(&d, 32 * M).filter(|_| !stand) {
            // Aside: across the line to the drone, at full speed.
            let a = b.drones[k].p;
            let (ux, uz) = (f64::from(a[0] - q[0]), f64::from(a[2] - q[1]));
            (stick_for(&d, -uz * side, ux * side, 1.0), false)
        } else if let Some((k, r)) = near
            && stand
        {
            // The first `STAND_FOR`: within 12 to 20 m of the drone,
            // standing. (A slow creep toward it, to have it in the
            // camera's view, ended the walk back stuck: not kept.)
            let a = b.drones[k].p;
            let (ux, uz) = (f64::from(a[0] - q[0]), f64::from(a[2] - q[1]));
            let k = if r > 20 * M {
                0.4
            } else if r < 12 * M {
                -0.3
            } else {
                0.0
            };
            (
                if k == 0.0 {
                    (0, 0)
                } else {
                    stick_for(&d, ux, uz, k)
                },
                false,
            )
        } else if let Some((k, r)) = near {
            let a = b.drones[k].p;
            let (ux, uz) = (f64::from(a[0] - q[0]), f64::from(a[2] - q[1]));
            let k = if r < 10 * M { 0.12 } else { 0.35 };
            side = -side;
            (stick_for(&d, ux, uz, k), true)
        } else {
            // None left near: to the seam.
            let (ux, uz) = (f64::from(to[0] - q[0]), f64::from(to[1] - q[1]));
            let k = if walker::apart(q, to) < 8 * M {
                0.0
            } else {
                0.6
            };
            (
                if k > 0.0 {
                    stick_for(&d, ux, uz, k)
                } else {
                    (0, 0)
                },
                false,
            )
        };
        ev.push((ms, Act::Stick(st.0, st.1)));
        if x != x_held {
            ev.push((
                ms,
                if x {
                    Act::Press(button::X)
                } else {
                    Act::Release(button::X)
                },
            ));
            x_held = x;
        }
        ms += FIGHT_EVERY;
        p.run_to(&mut d, &ev, tick_of(ms), &none);
        assert!(planet_on_foot(&d.app.below), "still on foot at {ms} ms");
    }
    if x_held {
        ev.push((ms, Act::Release(button::X)));
    }
    ev.push((ms, Act::Stick(0, 0)));
    let fight_end = (ms + 1000).div_ceil(1000) * 1000;
    p.run_to(&mut d, &ev, tick_of(fight_end), &none);
    println!(
        "fight {} s: drones up {}, kills {}, suit lost {} points",
        (fight_end - walk_end) / 1000,
        drones::up(&d.app.below),
        d.app.below.kills,
        (suit0 - d.app.below.pilot.suit) / 60
    );
    scenes.push(("the drone fight", walk_end, fight_end));
    // To 6 m of the seam, then the cut (X 3 s) and the chunks.
    ms = fight_end + 300;
    loop {
        let q = pilot_xz(&d);
        let dist = walker::apart(q, to);
        if dist <= 6 * M {
            break;
        }
        let k = if dist < 9 * M { 0.4 } else { 1.0 };
        let st = stick_for(&d, f64::from(to[0] - q[0]), f64::from(to[1] - q[1]), k);
        ev.push((ms, Act::Stick(st.0, st.1)));
        ms += 500;
        p.run_to(&mut d, &ev, tick_of(ms), &none);
        assert!(ms < fight_end + 30_000, "at the seam within 30 s");
    }
    ev.push((ms, Act::Stick(0, 0)));
    ms += 700;
    ev.push((ms, Act::Press(button::X)));
    ms += 3_000;
    ev.push((ms, Act::Release(button::X)));
    ms += 1_000;
    p.run_to(&mut d, &ev, tick_of(ms), &none);
    let cracked = d.app.below.seams[seam] >= SEAM_STAGES;
    let cut_from = ms;
    while cracked && ms < cut_from + 8_000 {
        let q = pilot_xz(&d);
        let b = &d.app.below;
        let next = b
            .chunks
            .iter()
            .filter(|c| c.state != CHUNK_NONE && !c.full)
            .map(|c| {
                let a = c.at();
                [a[0], a[2]]
            })
            .min_by_key(|&a| walker::apart(q, a));
        let st = match next {
            Some(a) => stick_for(&d, f64::from(a[0] - q[0]), f64::from(a[1] - q[1]), 0.6),
            None => (0, 0),
        };
        ev.push((ms, Act::Stick(st.0, st.1)));
        ms += 500;
        p.run_to(&mut d, &ev, tick_of(ms), &none);
    }
    ev.push((ms, Act::Stick(0, 0)));
    let cut_end = (ms + 1000).div_ceil(1000) * 1000;
    let cut_end = cut_end.max(fight_end + 15_000);
    p.run_to(&mut d, &ev, tick_of(cut_end), &none);
    println!("the seam cracked: {cracked}; haul {}", d.app.below.haul);
    scenes.push(("to the seam, the cut, its chunks", fight_end, cut_end));
    // Back to the hatch (within 1 m).
    ms = cut_end + 300;
    loop {
        let q = pilot_xz(&d);
        let h = walker::hatch(&d.app.below);
        let dist = walker::apart(q, h);
        if dist <= M {
            break;
        }
        let k = if dist < 4 * M { 0.35 } else { 1.0 };
        let st = stick_for(&d, f64::from(h[0] - q[0]), f64::from(h[1] - q[1]), k);
        ev.push((ms, Act::Stick(st.0, st.1)));
        ms += 500;
        p.run_to(&mut d, &ev, tick_of(ms), &none);
        assert!(ms < cut_end + 60_000, "back within a minute");
    }
    ev.push((ms, Act::Stick(0, 0)));
    let back_end = (ms + 1000).div_ceil(1000) * 1000;
    let back_end = back_end.max(cut_end + 15_000);
    p.run_to(&mut d, &ev, tick_of(back_end), &none);
    scenes.push(("walk back to the hatch", cut_end, back_end));
    ev.push((back_end + 300, Act::Press(button::B)));
    ev.push((back_end + 1300, Act::Release(button::B)));
    let end = back_end + 16_000;
    scenes.push(("B held, aboard, the take-off, space", back_end, end));
    (ev, scenes, seam)
}

/// What one replay of the G4 pad did.
#[derive(Debug, Default)]
#[allow(dead_code)]
struct Fight {
    kills: u8,
    suit_lost: i32,
    downed: bool,
    cracked: bool,
    picked: u32,
    at_hatch_at_b: bool,
    in_space: bool,
    most_sparks: usize,
}

/// Replays the G4 pad (each event's tick moved by `shift`).
fn replay_g4(
    ev: &[(u32, Act)],
    scenes: &Scenes,
    seam: usize,
    shift: &dyn Fn(usize) -> i32,
) -> Fight {
    let mut d = Driver::new();
    d.start();
    d.app.sel = 0;
    let mut p = Player::default();
    let mut f = Fight::default();
    let n = scenes.len();
    let b_at = scenes[n - 1].1 + 300;
    let end = scenes[n - 1].2;
    let mut suit0 = None;
    for t in 0..tick_of(end) {
        p.run_to(&mut d, ev, t + 1, shift);
        let b = &d.app.below;
        f.downed |= b.note == Some("PILOT DOWN");
        if d.app.scene == Scene::Below && planet_on_foot(b) {
            let s = *suit0.get_or_insert(b.pilot.suit);
            f.suit_lost = (s - b.pilot.suit) / 60;
            f.kills = b.kills;
            f.cracked |= b.seams[seam] >= SEAM_STAGES;
            f.picked = u32::from(b.haul);
            f.most_sparks = f.most_sparks.max(b.sparks.live());
        }
        if t == tick_of(b_at) + 2 {
            f.at_hatch_at_b = walker::at_hatch(&d.app.below);
        }
    }
    f.in_space = d.app.scene == Scene::Flight;
    f
}

/// The G4 site pad (BOARD-G3.md): the G3 rules for its scenes, one
/// scene with the drone fight of 15 s or more; the nominal run downs
/// the seam's drones, cracks the seam and goes back up; the same with
/// the foot events moved by up to 0.1 s and 0.5 s, and UP released
/// 0.1 s and 0.5 s off, is reported (the pilot must never go down and
/// the ship must reach space in every one within 0.1 s).
#[test]
fn the_g4_site_pad_fights_the_drones_at_a_seam() {
    let (ev, scenes, seam) = g4_pad();
    let text = pad_text_with(G4_HEAD, "the seam of the nearest drone", &ev, &scenes, seam);
    let dir = common::shots().join("planet-g4");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("G4-site.pad"), &text).unwrap();
    for (n, a, b) in &scenes {
        println!(
            "scene {a:6} .. {b:6} ms ({:5.1} s): {n}",
            f64::from(b - a) / 1000.0
        );
        assert!(b - a >= 15_000, "scene {n} is shorter than 15 s");
        for (at, act) in &ev {
            for edge in [*a, *b] {
                assert!(
                    at.abs_diff(edge) >= 300,
                    "{act:?} at {at} ms is within 300 ms of a border ({edge})"
                );
            }
        }
    }
    let fight = scenes.iter().find(|s| s.0 == "the drone fight").unwrap();
    assert!(fight.2 - fight.1 >= 15_000);
    let nominal = replay_g4(&ev, &scenes, seam, &|_| 0);
    println!("nominal: {nominal:?}");
    assert!(nominal.kills >= 1 && !nominal.downed && nominal.in_space);
    assert!(nominal.at_hatch_at_b);
    let first_foot = ev.iter().position(|e| e.0 > 54_000).unwrap();
    for (k, m) in [(1u32, 6), (2, 30)] {
        for sign in [-1i32, 1] {
            let shift = |i: usize| {
                if i < first_foot {
                    0
                } else {
                    let h = mercator::fixed::hash(i as u32, k * 7 + u32::from(sign > 0));
                    sign * (h % (m as u32 + 1)) as i32
                }
            };
            let r = replay_g4(&ev, &scenes, seam, &shift);
            println!("foot events moved up to {} ticks: {r:?}", sign * m);
            if m <= 6 {
                assert!(!r.downed && r.in_space, "up and away");
            }
        }
    }
    let up = ev
        .iter()
        .position(|e| e.1 == Act::Release(button::UP))
        .unwrap();
    for m in [-30, -6, 6, 30] {
        let r = replay_g4(&ev, &scenes, seam, &|i| if i == up { m } else { 0 });
        println!("UP released {m} ticks off: {r:?}");
        if m.abs() <= 6 {
            assert!(!r.downed && r.in_space, "up and away");
        }
    }
}
