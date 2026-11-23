//! Reviewer's measurements (look and feel review, 2026-11-22). Not part
//! of the builders' tests: it changes nothing, only steps the rules and
//! prints numbers. With `REVIEW_OUT` set to a folder it also writes the
//! long route as a simulator input script and a per-step table.
//!
//! Run: `REVIEW_OUT=<dir> cargo test --offline --test review_look -- --nocapture --test-threads 1`

mod common;

use common::*;
use game_kit::button;
use proto3d::island::*;
use proto3d::maths::cm;
use proto3d::play::{Detail, Play};
use proto3d::step::Pad;
use proto3d::tune::M;
use std::fmt::Write as _;

fn m(s: i32) -> f64 {
    f64::from(s) / f64::from(M)
}

fn deg(a: i32) -> f64 {
    f64::from(a) * 360.0 / 65_536.0
}

/// Signed angle difference a - b in degrees, -180..180.
fn ang_diff(a: f64, b: f64) -> f64 {
    let mut d = a - b;
    while d > 180.0 {
        d -= 360.0;
    }
    while d < -180.0 {
        d += 360.0;
    }
    d
}

/// The yaw (degrees, 0 = +z, 90 = +x) the eye looks along, from eye to aim.
fn eye_yaw(p: &Play) -> f64 {
    let dx = f64::from(p.cam.aim[0] - p.cam.eye[0]);
    let dz = f64::from(p.cam.aim[2] - p.cam.eye[2]);
    dx.atan2(dz).to_degrees()
}

/// A recorder: every step's pad goes into a simulator script (frame =
/// first frame + step), and every step's state into a table.
struct Rec {
    play: Play,
    last: Pad,
    script: String,
    table: String,
    frame0: u32,
    n: u32,
    notes: Vec<String>,
}

impl Rec {
    fn new(play: Play, frame0: u32) -> Rec {
        let mut table = String::new();
        table.push_str("step\tframe\tx_m\ty_m\tz_m\tground\tpose\tshadow_r_m\tring\tring_x\tring_z\tring_steps\tcam_yaw_deg\teye_yaw_deg\taim_y_m\teye_y_m\ttilt_deg\ttaken\tevents\n");
        Rec {
            play,
            last: Pad::NONE,
            script: String::new(),
            table,
            frame0,
            n: 0,
            notes: Vec::new(),
        }
    }

    fn step(&mut self, mut p: Pad) {
        // Presses and releases come from the held bits.
        p.pressed = p.held & !self.last.held;
        p.released = self.last.held & !p.held;
        let f = self.frame0 + self.n;
        if p.left != self.last.left {
            let _ = writeln!(self.script, "{f} stick left {} {}", p.left.0, p.left.1);
        }
        if p.right != self.last.right {
            let _ = writeln!(self.script, "{f} stick right {} {}", p.right.0, p.right.1);
        }
        for (b, name) in [(button::A, "south"), (button::L, "l"), (button::R, "r")] {
            if p.pressed & b != 0 {
                let _ = writeln!(self.script, "{f} press {name}");
            }
            if p.released & b != 0 {
                let _ = writeln!(self.script, "{f} release {name}");
            }
        }
        self.play.step(&p);
        self.last = p;
        self.check();
        let q = &self.play;
        let h = &q.hero;
        let (ring, rx, rz, rs) = match q.ring {
            Some(r) => (
                format!("{}", r.top),
                format!("{:.2}", m(r.spot[0])),
                format!("{:.2}", m(r.spot[2])),
                format!("{}", r.steps),
            ),
            None => ("-".into(), "-".into(), "-".into(), "-".into()),
        };
        let ev = &q.events;
        let mut e = String::new();
        if ev.hero.jumped {
            e.push_str("jump ");
        }
        if let Some(g) = ev.hero.landed {
            let _ = write!(e, "land{g} ");
        }
        if ev.hero.bumped {
            e.push_str("bump ");
        }
        if ev.hero.fell {
            e.push_str("fell ");
        }
        if ev.hero.respawned {
            e.push_str("respawn ");
        }
        if let Some(t) = ev.took {
            let _ = write!(e, "took{t} ");
        }
        let _ = writeln!(
            self.table,
            "{}\t{}\t{:.2}\t{:.2}\t{:.2}\t{}\t{:?}\t{}\t{}\t{}\t{}\t{}\t{:.1}\t{:.1}\t{:.2}\t{:.2}\t{:.1}\t{:06b}\t{}",
            self.n,
            f,
            m(h.pos[0]),
            m(h.pos[1]),
            m(h.pos[2]),
            h.ground.map_or("-".into(), |g| g.to_string()),
            q.pose(),
            q.shadow.map_or("-".into(), |s| format!("{:.2}", m(s.r))),
            ring,
            rx,
            rz,
            rs,
            deg(i32::from(q.cam.yaw)),
            eye_yaw(q),
            m(q.cam.aim[1]),
            m(q.cam.eye[1]),
            deg(i32::from(q.cam.tilt)),
            q.coins.taken,
            e.trim_end()
        );
        self.n += 1;
    }

    /// Look-related checks on the rules' state after a step.
    fn check(&mut self) {
        let q = &self.play;
        let h = &q.hero;
        let n = self.n;
        if h.frozen == 0 {
            match (h.ground, q.shadow) {
                (Some(g), None) => self.notes.push(format!(
                    "step {n}: standing on {g} at ({:.2},{:.2},{:.2}) with no shadow",
                    m(h.pos[0]),
                    m(h.pos[1]),
                    m(h.pos[2])
                )),
                (Some(g), Some(s)) if s.top != g => self.notes.push(format!(
                    "step {n}: standing on {g} at ({:.2},{:.2},{:.2}), shadow on {} at y {:.2}",
                    m(h.pos[0]),
                    m(h.pos[1]),
                    m(h.pos[2]),
                    s.top,
                    m(s.spot[1])
                )),
                _ => {}
            }
        }
        if let Some(b) = inside_any(q.world.blocks, q.cam.eye) {
            self.notes.push(format!("step {n}: eye inside block {b}"));
        }
        if q.cam.eye[1] < -cm(60) {
            self.notes.push(format!(
                "step {n}: eye below the island's skirt ({:.2} m), hero y {:.2}, frozen {}",
                m(q.cam.eye[1]),
                m(h.pos[1]),
                h.frozen
            ));
        }
        // A tree or bush ball crossing the line eye to hero chest while
        // drawn at more than half size.
        let chest = [h.pos[0], h.pos[1] + proto3d::tune::HERO_H / 2, h.pos[2]];
        for (i, pr) in q.world.props.iter().enumerate() {
            if !q.prop_shown(i) {
                continue;
            }
            if let Some((c, r)) = pr.ball() {
                let d2 = dist2_to_segment(c, q.cam.eye, chest);
                if d2 <= i64::from(r) * i64::from(r) && q.cam.scale[i] > 128 {
                    self.notes.push(format!(
                        "step {n}: prop {i} on the eye-hero line at scale {}",
                        q.cam.scale[i]
                    ));
                }
            }
        }
    }

    /// Stand still for `n` steps.
    fn wait(&mut self, n: u32) {
        for _ in 0..n {
            self.step(pad((0, 0), 0));
        }
    }

    /// Turn the camera a full turn with the right stick (full right).
    fn cam_turn(&mut self) {
        let steps = 65_536 / 546 + 1;
        for _ in 0..steps {
            let mut p = pad((0, 0), 0);
            p.right = (32_767, 0);
            self.step(p);
        }
        self.wait(10);
    }

    /// A leg, as `common::follow`, recorded.
    fn leg(&mut self, leg: Leg, limit: usize) {
        let mut jumped = false;
        let mut held_a = 0;
        for steps in 0.. {
            let h = &self.play.hero;
            let dx = f64::from(cm(leg.to[0]) - h.pos[0]);
            let dz = f64::from(cm(leg.to[1]) - h.pos[2]);
            let d_cm = (dx * dx + dz * dz).sqrt() * 25.0 / 1024.0;
            let here = h.ground == Some(leg.on) && h.frozen == 0;
            if here && (leg.anywhere || d_cm < 40.0) {
                break;
            }
            assert!(steps < limit, "leg {leg:?} not done: {:?}", self.play.hero);
            let near = (d_cm / 100.0).min(1.0);
            let speed = leg.speed * near.max(0.25);
            let s = stick_towards(dx, dz, self.play.cam.yaw, speed);
            let mut p = pad(s, 0);
            if let Some(j) = leg.jump_within
                && !jumped
                && d_cm <= f64::from(j)
                && h.ground.is_some()
            {
                p.held |= button::A;
                jumped = true;
                held_a = 20;
            } else if held_a > 0 {
                p.held |= button::A;
                held_a -= 1;
            }
            self.step(p);
        }
        // Let go of the stick for one step (the next leg starts clean).
        self.step(pad((0, 0), 0));
    }

    /// Walk towards a point off the island until the hero fell and came
    /// back.
    fn fall_off(&mut self, to: [i16; 2]) {
        let falls = self.play.hero.falls;
        let mut back = false;
        for _ in 0..400 {
            let h = &self.play.hero;
            if h.falls > falls && h.frozen == 0 {
                back = true;
                break;
            }
            let dx = f64::from(cm(to[0]) - h.pos[0]);
            let dz = f64::from(cm(to[1]) - h.pos[2]);
            let s = if h.falls > falls {
                (0, 0)
            } else {
                stick_towards(dx, dz, self.play.cam.yaw, 1.0)
            };
            self.step(pad(s, 0));
        }
        assert!(back, "no fall towards {to:?}");
        self.wait(12);
    }
}

/// The long route: every top, every rim once, four camera turns.
fn route(detail: Detail, frame0: u32) -> Rec {
    let mut r = Rec::new(Play::on(&ISLAND, detail), frame0);
    r.wait(5);
    r.cam_turn(); // turn 1: the start
    for l in ROUTE[..2].iter() {
        r.leg(*l, 400); // onto the step, onto the floating block
    }
    r.cam_turn(); // turn 2: on the floating block
    for l in ROUTE[2..6].iter() {
        r.leg(*l, 400); // far corner, up the ramp, over the terrace
    }
    r.cam_turn(); // turn 3: on the terrace
    r.fall_off([-900, 450]); // the terrace's outer rim (-x)
    for l in ROUTE[6..].iter() {
        r.leg(*l, 400); // down to the main top, across the gap
    }
    r.cam_turn(); // turn 4: on the islet
    r.fall_off([0, 1500]); // islet, far rim (+z)
    // Back from the islet: run to its far side, then jump to the island.
    r.leg(leg([0, 1250], 1.0, None, ISLET), 400);
    r.leg(leg([0, 450], 1.0, Some(520), MAIN), 400);
    r.fall_off([0, 850]); // main +z rim, walking into the gap
    r.fall_off([900, -300]); // main +x rim
    r.fall_off([-900, -300]); // main -x rim
    r.fall_off([0, -900]); // main -z rim
    r.wait(10);
    r
}

#[test]
fn review_route_is_recorded() {
    let r = route(Detail::Mid, 0);
    let falls = r.play.hero.falls;
    println!(
        "route: {} steps, falls {falls}, taken {:06b}",
        r.n, r.play.coins.taken
    );
    println!("notes: {}", r.notes.len());
    for l in r.notes.iter() {
        println!("  {l}");
    }
    if let Ok(dir) = std::env::var("REVIEW_OUT") {
        std::fs::write(format!("{dir}/route-table.tsv"), &r.table).unwrap();
        std::fs::write(format!("{dir}/route-steps.txt"), &r.script).unwrap();
    }
}

/// Frames from the press of A to the feet leaving the top; airtime.
#[test]
fn review_jump_timing() {
    for (name, hold) in [("full", 40u32), ("tap", 1)] {
        let mut p = Play::new();
        for _ in 0..5 {
            p.step(&pad((0, 0), 0));
        }
        let y0 = p.hero.pos[1];
        let mut left = None;
        let mut land = None;
        let mut peak = 0;
        for k in 0..60u32 {
            let mut q = pad((0, 0), if k < hold { button::A } else { 0 });
            if k == 0 {
                q.pressed = button::A;
            }
            p.step(&q);
            peak = peak.max(p.hero.pos[1] - y0);
            if left.is_none() && p.hero.pos[1] > y0 {
                left = Some(k);
            }
            if left.is_some() && land.is_none() && p.hero.ground.is_some() {
                land = Some(k);
            }
        }
        println!(
            "jump {name}: press in step 0, feet up in step {:?}, landed in step {:?}, airtime {} steps, peak {:.3} m",
            left,
            land,
            land.unwrap() - left.unwrap(),
            m(peak)
        );
    }
}

/// Running straight at the +z rim (into the gap): where the press of A
/// still gives a jump, and where it still reaches the islet.
#[test]
#[ignore = "a measurement for the report: prints, asserts nothing (rules review F8); run with --ignored"]
fn review_last_safe_jump_at_the_gap() {
    let mut first_ok: Option<f64> = None;
    let mut last_ok: Option<f64> = None;
    let mut last_jump: Option<f64> = None;
    let mut fails_between = Vec::new();
    for t in 0..=260 {
        let press_z = cm(500) + t * cm(2); // 5.00 m to 10.2 m
        let mut p = Play::new();
        p.hero = hero_at(0, 0, 100, MAIN);
        p.cam.snap(&p.hero, &BLOCKS, ISLAND.props, 1);
        let mut pressed_at = None;
        let mut result = "none";
        for k in 0..200 {
            let mut q = pad(FWD, 0);
            if pressed_at.is_none() && p.hero.pos[2] >= press_z {
                q = jump_pad(FWD, 0);
                pressed_at = Some(p.hero.pos[2]);
            } else if pressed_at.is_some() {
                q.held |= button::A;
            }
            let _ = k;
            let jumps = p.hero.jumps;
            p.step(&q);
            if p.hero.jumps > jumps {
                result = "jumped";
            }
            if pressed_at.is_some() && p.hero.ground == Some(ISLET) {
                result = "islet";
                break;
            }
            if p.hero.frozen > 0 {
                break;
            }
            if pressed_at.is_some() && p.hero.ground == Some(MAIN) && result == "jumped" {
                result = "back on main";
                break;
            }
        }
        let Some(z) = pressed_at else { continue };
        let z = m(z);
        if result == "islet" {
            first_ok.get_or_insert(z);
            last_ok = Some(z);
        } else if first_ok.is_some() && last_ok.is_some() {
            fails_between.push((z, result));
        }
        if result != "none" {
            last_jump = Some(z);
        }
    }
    println!(
        "gap (rim at z 7.00 m, hero half width 0.25 m): presses from z {:?} to {:?} reach the islet; last press that still jumps at all {:?}",
        first_ok, last_ok, last_jump
    );
    println!(
        "after the first success, misses: {:?}",
        &fails_between[..fails_between.len().min(6)]
    );
}

/// Running at the +x rim where there is nothing beyond: how far past the
/// rim a press still jumps (coyote).
#[test]
#[ignore = "a measurement for the report: prints, asserts nothing (rules review F8); run with --ignored"]
fn review_coyote_distance_at_a_plain_rim() {
    let mut last = None;
    for t in 0..=100 {
        let press_x = cm(650) + t * cm(1);
        let mut p = Play::new();
        p.hero = hero_at(300, 0, -300, MAIN);
        // Camera looks along +x: stick forward runs +x.
        p.cam.yaw = 16_384;
        p.cam.snap(&p.hero, &BLOCKS, ISLAND.props, 1);
        let mut pressed = false;
        let mut jumped = false;
        for _ in 0..80 {
            let q = if !pressed && p.hero.pos[0] >= press_x {
                pressed = true;
                jump_pad(FWD, 0)
            } else {
                pad(FWD, 0)
            };
            let j = p.hero.jumps;
            p.step(&q);
            if p.hero.jumps > j {
                jumped = true;
                last = Some((m(press_x), m(p.hero.pos[0])));
            }
            if pressed && !jumped && p.hero.jumps == j && p.hero.air_steps > 8 {
                break;
            }
        }
    }
    println!(
        "plain rim at x 7.00 m: the last press position that still jumps (threshold, hero x after the jump step): {last:?}"
    );
}

/// The camera through a full jump on flat ground and a jump across the
/// gap: aim height, eye height, tilt.
#[test]
#[ignore = "a measurement for the report: prints, asserts nothing (rules review F8); run with --ignored"]
fn review_camera_in_jumps() {
    let mut p = Play::new();
    for _ in 0..30 {
        p.step(&pad((0, 0), 0));
    }
    let (mut amin, mut amax, mut emin, mut emax) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
    for k in 0..30u32 {
        let mut q = pad((0, 0), button::A);
        if k == 0 {
            q.pressed = button::A;
        }
        p.step(&q);
        amin = amin.min(p.cam.aim[1]);
        amax = amax.max(p.cam.aim[1]);
        emin = emin.min(p.cam.eye[1]);
        emax = emax.max(p.cam.eye[1]);
    }
    println!(
        "flat jump: aim y {:.3}..{:.3} m, eye y {:.3}..{:.3} m",
        m(amin),
        m(amax),
        m(emin),
        m(emax)
    );

    // Gap jump, straight, full run, press at z 6.9 m.
    let mut p = Play::new();
    p.hero = hero_at(0, 0, 200, MAIN);
    p.cam.snap(&p.hero, &BLOCKS, ISLAND.props, 1);
    let mut pressed = false;
    let mut line = String::new();
    for _ in 0..60 {
        let q = if !pressed && p.hero.pos[2] >= cm(690) {
            pressed = true;
            jump_pad(FWD, 0)
        } else if pressed {
            pad(FWD, button::A)
        } else {
            pad(FWD, 0)
        };
        p.step(&q);
        if pressed {
            let _ = write!(
                line,
                "[z {:.2} y {:.2} aim_y {:.2} eye_y {:.2} tilt {:.1} ring {}] ",
                m(p.hero.pos[2]),
                m(p.hero.pos[1]),
                m(p.cam.aim[1]),
                m(p.cam.eye[1]),
                deg(i32::from(p.cam.tilt)),
                p.ring.map_or("-".to_string(), |r| format!(
                    "{}@{:.2}",
                    r.steps,
                    m(r.spot[2])
                ))
            );
            if p.hero.ground == Some(ISLET) {
                break;
            }
        }
    }
    println!("gap jump: {line}");
}

/// How far the eye's own direction lags behind the camera's yaw in a
/// right-stick turn, in an L release turn, and in a U-turn of the hero.
#[test]
#[ignore = "a measurement for the report: prints, asserts nothing (rules review F8); run with --ignored"]
fn review_eye_lag() {
    let mut p = Play::new();
    for _ in 0..10 {
        p.step(&pad((0, 0), 0));
    }
    let mut worst: f64 = 0.0;
    for _ in 0..60 {
        let mut q = pad((0, 0), 0);
        q.right = (32_767, 0);
        p.step(&q);
        worst = worst.max(ang_diff(deg(i32::from(p.cam.yaw)), eye_yaw(&p)).abs());
    }
    let mut settle = 0;
    for k in 0..60 {
        p.step(&pad((0, 0), 0));
        if ang_diff(deg(i32::from(p.cam.yaw)), eye_yaw(&p)).abs() < 1.0 {
            settle = k + 1;
            break;
        }
    }
    println!(
        "right stick full turn: eye lags the yaw by up to {worst:.1} deg; within 1 deg {settle} steps after letting go"
    );

    // L released: 45 degrees over 6 steps.
    let mut p = Play::new();
    for _ in 0..10 {
        p.step(&pad((0, 0), 0));
    }
    p.step(&pad((0, 0), button::L));
    let mut lag: Vec<String> = Vec::new();
    for _ in 0..16 {
        p.step(&pad((0, 0), 0));
        lag.push(format!(
            "{:.0}/{:.0}",
            deg(i32::from(p.cam.yaw)),
            eye_yaw(&p)
        ));
    }
    println!(
        "L release (cam yaw / eye yaw, deg, per step): {}",
        lag.join(" ")
    );

    // Hero runs +z for 40 steps (camera behind), then turns to -z:
    // the camera is left looking at the hero's face; how long until the
    // auto turn brings it round (it only acts within 120 degrees).
    let mut p = Play::new();
    for _ in 0..40 {
        let s = stick_towards(0.0, 1.0, p.cam.yaw, 1.0);
        p.step(&pad(s, 0));
    }
    let y0 = deg(i32::from(p.cam.yaw));
    for _ in 0..60 {
        p.step(&pad((32_767, 0), 0)); // stick right, held still
    }
    println!(
        "holding the stick right for 2 s after running forward: camera yaw {:.1} -> {:.1} deg, hero yaw {:.1} deg",
        y0,
        deg(i32::from(p.cam.yaw)),
        deg(i32::from(p.hero.yaw))
    );
}

/// A player holds a diagonal stick still (does not steer like the bot)
/// and jumps the gap: does the first ring match the landing when the
/// auto camera turns during the jump?
#[test]
#[ignore = "a measurement for the report: prints, asserts nothing (rules review F8); run with --ignored"]
fn review_ring_with_a_still_stick() {
    for (name, stick, press_z) in [
        ("straight", (0i16, -32_767i16), 690),
        ("diagonal 20 deg", (-11_207, -30_791), 690),
        ("diagonal 35 deg", (-18_794, -26_841), 680),
    ] {
        let mut p = Play::new();
        p.hero = hero_at(150, 0, 0, MAIN);
        p.cam.snap(&p.hero, &BLOCKS, ISLAND.props, 1);
        let mut pressed = false;
        let mut first = None;
        let mut rings = Vec::new();
        let mut landed = None;
        for _ in 0..200 {
            let q = if !pressed && p.hero.pos[2] >= cm(press_z) {
                pressed = true;
                jump_pad(stick, 0)
            } else if pressed {
                pad(stick, button::A)
            } else {
                pad(stick, 0)
            };
            p.step(&q);
            if pressed {
                println!(
                    "  {name}: hero ({:.2}, {:.2}, {:.2}) ground {:?} cam {:.1} ring {:?}",
                    m(p.hero.pos[0]),
                    m(p.hero.pos[1]),
                    m(p.hero.pos[2]),
                    p.hero.ground,
                    deg(i32::from(p.cam.yaw)),
                    p.ring.map(|r| (r.top, m(r.spot[0]), m(r.spot[2]), r.steps))
                );
            }
            if let Some(r) = p.ring {
                first.get_or_insert(r);
                rings.push((m(r.spot[0]), m(r.spot[2])));
            }
            if pressed && p.hero.ground.is_some() {
                landed = Some((p.hero.ground, m(p.hero.pos[0]), m(p.hero.pos[2])));
                break;
            }
            if p.hero.frozen > 0 {
                landed = Some((None, m(p.hero.pos[0]), m(p.hero.pos[2])));
                break;
            }
        }
        let spread = rings
            .windows(2)
            .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
            .fold(0.0f64, f64::max);
        println!(
            "ring, {name}: first ring {:?}, {} steps of ring, largest move of the ring between two steps {spread:.3} m, landing {landed:?}",
            first.map(|r| (r.top, m(r.spot[0]), m(r.spot[2]), r.steps)),
            rings.len()
        );
    }
}

/// The hero may stand with its middle up to half its width past a top s
/// edge (the square still overlaps it). Where does the shadow go then?
#[test]
#[ignore = "a measurement for the report: prints, asserts nothing (rules review F8); run with --ignored"]
fn review_shadow_at_an_edge() {
    for (name, x, y, z, top) in [
        (
            "step, middle 0.15 m off its -x edge",
            85i16,
            50i16,
            0i16,
            STEP,
        ),
        (
            "main island, middle 0.15 m past the +x rim",
            715,
            0,
            -300,
            MAIN,
        ),
        ("islet, middle 0.15 m past its near rim", 0, 0, 935, ISLET),
        (
            "floating block, middle 0.15 m off its -x edge",
            185,
            150,
            380,
            FLOAT,
        ),
    ] {
        let mut p = Play::new();
        p.hero = hero_at(x, y, z, top);
        p.cam.snap(&p.hero, &BLOCKS, ISLAND.props, 1);
        for _ in 0..5 {
            p.step(&pad((0, 0), 0));
        }
        println!(
            "{name}: ground {:?}, feet y {:.2}, shadow {:?}, ring {:?}",
            p.hero.ground,
            m(p.hero.pos[1]),
            p.shadow.map(|s| (s.top, m(s.spot[1]), m(s.r))),
            p.ring.map(|r| r.top)
        );
    }
}
