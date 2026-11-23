//! Pictures and pad scripts for the "life" round (2026-11-22): drawn by
//! the game's own drawing code on the PC, the same code the simulator
//! and the chip run. Ignored; run with
//!
//! ```text
//! LIFE_OUT=<folder> cargo test --offline --test life_shots -- --ignored --nocapture
//! ```
//!
//! It writes `<folder>/pad/course.pad` (a script for `boardctl pad
//! script`, from right after the game's tile was opened), the moments
//! as PNG files (4x) and `counts.txt` with each moment's triangle and
//! pixel counts (MEASURED, PC).
//!
//! The course is driven by the tests' test driver in a closed loop on
//! the PC; the pad script replays its inputs blind, so on the board the
//! car can drift a little from the PC's path. Each stop ends with Y
//! (back to the road), which puts the car on a road point again.

mod common;

use common::{autopilot, mps};
use drive3d::car::Controls;
use drive3d::detail::Detail;
use drive3d::draw::{self, Opts, PALETTE};
use drive3d::land::{Ground, Stretch};
use drive3d::look::Look;
use drive3d::maths::{angle_diff, yaw_of};
use drive3d::play::{Input, Play};
use drive3d::road::{POINTS, ROAD, surface_at};
use drive3d::scene::Painter;
use drive3d::tune::{DEAD, FORD, GRASS, LOW_GRASS, M, RESET_STEPS};
use game_kit::{Frame, H, W};

/// A PNG (8-bit RGB, stored zlib), each pixel `k` times `k`.
fn png(f: &Frame, k: usize) -> Vec<u8> {
    let (w, h) = (W * k, H * k);
    let mut raw = Vec::with_capacity((w * 3 + 1) * h);
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            let c = PALETTE
                .get(usize::from(f.0[(y / k) * W + x / k]))
                .copied()
                .unwrap_or(0);
            let (r, g, b) = ((c >> 11) & 31, (c >> 5) & 63, c & 31);
            raw.extend([
                (r * 255 / 31) as u8,
                (g * 255 / 63) as u8,
                (b * 255 / 31) as u8,
            ]);
        }
    }
    let mut z = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, ch) in chunks.iter().enumerate() {
        z.push(u8::from(i + 1 == chunks.len()));
        let n = ch.len() as u16;
        z.extend(n.to_le_bytes());
        z.extend((!n).to_le_bytes());
        z.extend_from_slice(ch);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend(((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend((w as u32).to_be_bytes());
    ihdr.extend((h as u32).to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    for (kind, data) in [(b"IHDR", ihdr), (b"IDAT", z), (b"IEND", Vec::new())] {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(&data);
        out.extend(&body);
        let mut c = 0xFFFF_FFFFu32;
        for &x in &body {
            c ^= u32::from(x);
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    (c >> 1) ^ 0xEDB8_8320
                } else {
                    c >> 1
                };
            }
        }
        out.extend((!c).to_be_bytes());
    }
    out
}

/// One step of the course: the input and whether X or Y was tapped.
#[derive(Clone, Copy, Default)]
struct Rec {
    input: Input,
}

/// A course driven on the PC, step by step, with named moments.
struct Course {
    p: Play,
    steps: Vec<Rec>,
    states: Vec<Play>,
    marks: Vec<(String, usize)>,
}

fn ford_point() -> usize {
    (0..POINTS)
        .find(|&k| Stretch.at(ROAD.x[k], ROAD.z[k]).surface == FORD)
        .expect("a ford on the road")
}

impl Course {
    fn new() -> Course {
        Course {
            p: Play::new(),
            steps: Vec::new(),
            states: vec![Play::new()],
            marks: Vec::new(),
        }
    }

    fn step(&mut self, input: Input) {
        self.p.step(&input);
        self.steps.push(Rec { input });
        self.states.push(self.p);
    }

    fn mark(&mut self, name: &str) {
        self.marks.push((name.to_string(), self.states.len() - 1));
    }

    /// Nothing pressed for `n` steps.
    fn idle(&mut self, n: usize) {
        for _ in 0..n {
            self.step(Input::default());
        }
    }

    /// The test driver until `until` holds (at most `max` steps), with
    /// `cruise` m/s (and `slow_at`: from that road point on `slow` m/s).
    fn drive(&mut self, cruise: i32, corner: i32, max: usize, until: &dyn Fn(&Play) -> bool) {
        for _ in 0..max {
            if until(&self.p) {
                return;
            }
            let i = autopilot(&self.p, cruise * M / 30, corner * M / 30);
            self.step(i);
        }
        panic!("the course did not get there");
    }

    /// Brakes to a stop (steering as the test driver), then nothing.
    fn stop(&mut self) {
        for _ in 0..600 {
            if self.p.car.speed() == 0 {
                return;
            }
            let mut i = autopilot(&self.p, 0, 0);
            i.controls.throttle = 0;
            i.controls.brake = if self.p.car.fwd_side().0 > 0 { 256 } else { 0 };
            self.step(i);
        }
        panic!("the car did not stop");
    }

    /// Brakes until the car stands, either way (the throttle stops a
    /// car rolling backwards).
    fn stop_any(&mut self) {
        for _ in 0..600 {
            if self.p.car.speed() == 0 {
                return;
            }
            let mut i = autopilot(&self.p, 0, 0);
            i.controls = Controls {
                aim: 0,
                throttle: if self.p.car.fwd_side().0 < 0 { 256 } else { 0 },
                brake: if self.p.car.fwd_side().0 > 0 { 256 } else { 0 },
                handbrake: false,
            };
            self.step(i);
        }
        panic!("the car did not stop");
    }

    /// Y, then nothing until the car stands on the road again.
    fn back_to_road(&mut self) {
        self.step(Input {
            reset: true,
            ..Input::default()
        });
        self.idle(usize::from(RESET_STEPS) + 5);
    }

    /// Holds `c` for `n` steps.
    fn hold(&mut self, c: Controls, n: usize) {
        for _ in 0..n {
            self.step(Input {
                controls: c,
                ..Input::default()
            });
        }
    }
}

/// The stick's x for a wanted steering (the inverse of `stick_aim`).
fn stick_for(aim: i32) -> i32 {
    if aim == 0 {
        return 0;
    }
    let a = aim.abs();
    let mut x = DEAD + a * (32_767 - DEAD) / 256;
    while drive3d::car::stick_aim(x as i16).abs() < a {
        x += 1;
    }
    if aim < 0 { -x } else { x }
}

/// The course as a pad script: one line per change, from `t0` ms.
fn pad_script(c: &Course, t0: u64, head: &str) -> String {
    let mut out = String::new();
    out.push_str(head);
    out.push_str(&format!("{:<6} stick left 0 0\n", 0));
    out.push_str(&format!("{:<6} repeat 10\n", 50));
    let (mut stick, mut held) = (0i32, Vec::<&str>::new());
    for (n, r) in c.steps.iter().enumerate() {
        let ms = t0 + (n as u64 * 1000 + 15) / 30;
        let ctl = r.input.controls;
        let s = stick_for(ctl.aim);
        if s != stick {
            out.push_str(&format!("{ms:<6} stick left {s} 0\n"));
            stick = s;
        }
        let mut want = Vec::new();
        if ctl.throttle >= 256 {
            want.push("south");
        }
        if ctl.brake >= 256 {
            want.push("east");
        }
        if ctl.handbrake {
            want.push("r");
        }
        for b in &held {
            if !want.contains(b) {
                out.push_str(&format!("{ms:<6} release {b}\n"));
            }
        }
        for b in &want {
            if !held.contains(b) {
                out.push_str(&format!("{ms:<6} press {b}\n"));
            }
        }
        held = want;
        if r.input.reset {
            out.push_str(&format!("{ms:<6} tap north\n"));
        }
        if r.input.level {
            out.push_str(&format!("{ms:<6} tap west\n"));
        }
    }
    let end = t0 + (c.steps.len() as u64 * 1000) / 30;
    for b in &held {
        out.push_str(&format!("{end:<6} release {b}\n"));
    }
    out.push_str(&format!("{end:<6} stick left 0 0\n"));
    out
}

/// The picture of `p` at level `d` and its counts.
fn shot(p: &Play, d: Detail) -> (Box<Frame>, String) {
    let mut p = *p;
    p.detail = d;
    let mut paint = Box::new(Painter::new());
    let mut f = Box::new(Frame([0; W * H]));
    let look = Look::new(&p, 1);
    draw::picture(
        &mut f,
        &mut paint,
        &look,
        None,
        Opts {
            line: false,
            plain: false,
        },
        &mut (),
    );
    let counts = format!(
        "road point {} triangles {} pixels {} cells {} objects {}",
        p.last_road, paint.r.stats.tris, paint.r.stats.pixels, paint.cells, paint.r.stats.objects
    );
    (f, counts)
}

/// Whether `LIFE_EXTRA` (a comma list) names `what`.
fn extra(what: &str) -> bool {
    std::env::var("LIFE_EXTRA").is_ok_and(|v| v.split(',').any(|w| w == what))
}

fn nearest_k(x: i32, z: i32) -> usize {
    (0..POINTS)
        .min_by_key(|&k| {
            let dx = i64::from(x) - i64::from(ROAD.x[k]);
            let dz = i64::from(z) - i64::from(ROAD.z[k]);
            dx * dx + dz * dz
        })
        .expect("points")
}

fn deg(a: i32) -> f64 {
    f64::from(a) * 360.0 / 65_536.0
}

/// tests/review_rules.rs `pursue`: steer the nose at the point
/// `look_s` ahead, full lock at `full_deg` off.
fn pursue(p: &Play, look_s: f64, full_deg: f64) -> i32 {
    let c = &p.car;
    let k0 = nearest_k(c.pos[0], c.pos[2]);
    let n = ((mps(c.speed()) * look_s).max(6.0) / 2.0).ceil() as usize;
    let k = (k0 + n).min(POINTS - 1);
    let want = yaw_of(ROAD.x[k] - c.pos[0], ROAD.z[k] - c.pos[2]).unwrap_or(c.yaw);
    let off = deg(angle_diff(c.yaw, want));
    ((off / full_deg) * 256.0).round().clamp(-256.0, 256.0) as i32
}

/// tests/review_rules.rs `drift_aim`: the nose up to `max_slip`
/// degrees inside the travel toward the point ahead.
fn drift_aim(p: &Play, look_s: f64, max_slip: f64) -> i32 {
    let c = &p.car;
    if c.speed() < 300 {
        return pursue(p, look_s, 10.0);
    }
    let travel = yaw_of(c.vel[0], c.vel[1]).unwrap_or(c.yaw);
    let k0 = nearest_k(c.pos[0], c.pos[2]);
    let n = ((mps(c.speed()) * look_s).max(6.0) / 2.0).ceil() as usize;
    let k = (k0 + n).min(POINTS - 1);
    let target = yaw_of(ROAD.x[k] - c.pos[0], ROAD.z[k] - c.pos[2]).unwrap_or(travel);
    let off = deg(angle_diff(travel, target));
    let slip = (off * 3.0).clamp(-max_slip, max_slip);
    let want = (f64::from(travel) + slip * 65_536.0 / 360.0).rem_euclid(65_536.0) as u16;
    let e = deg(angle_diff(c.yaw, want));
    ((e / 6.0) * 256.0).round().clamp(-256.0, 256.0) as i32
}

/// The second corner as the plan "h held slide, full" of
/// tests/review_rules.rs, to road point 162; marks the moment of the
/// largest slide under full throttle ("4-power"). Returns the steps on
/// grass.
fn power_corner(c: &mut Course) -> u32 {
    let (entry, _) = ROAD.piece_points(7);
    let (v_entry, hb_before, hb_steps, look_s, drift) = (20.5, 4.0, 6u32, 0.5, 30.0);
    let mut hb_left: Option<u32> = None;
    let (mut grass, mut best) = (0, 0.0f64);
    for _ in 0..600 {
        let car = c.p.car;
        let k0 = nearest_k(car.pos[0], car.pos[2]);
        if k0 >= 162 {
            break;
        }
        let v = mps(car.speed());
        let to_entry = (entry as f64 - k0 as f64) * 2.0;
        if hb_left.is_none() && to_entry <= hb_before {
            hb_left = Some(hb_steps);
        }
        let aim = if hb_left.is_some() || to_entry <= hb_before {
            drift_aim(&c.p, look_s, drift)
        } else {
            pursue(&c.p, look_s, 6.0)
        };
        let mut ctl = Controls {
            aim,
            throttle: 0,
            brake: 0,
            handbrake: false,
        };
        match hb_left {
            Some(h) if h > 0 => {
                ctl.aim = -256;
                ctl.handbrake = true;
                hb_left = Some(h - 1);
            }
            Some(_) => ctl.throttle = 256,
            None => {
                let d = (to_entry - hb_before).max(0.0);
                if v * v - v_entry * v_entry >= 2.0 * 5.5 * d {
                    if v > v_entry + 0.5 {
                        ctl.brake = 256;
                    } else if v < v_entry {
                        ctl.throttle = 256;
                    }
                } else {
                    ctl.throttle = 256;
                }
            }
        }
        c.hold(ctl, 1);
        if matches!(c.p.car.surface, GRASS | LOW_GRASS) {
            grass += 1;
        }
        if ctl.throttle == 256 && hb_left == Some(0) {
            let travel = yaw_of(c.p.car.vel[0], c.p.car.vel[1]).unwrap_or(c.p.car.yaw);
            let slide = deg(angle_diff(travel, c.p.car.yaw)).abs();
            if slide > best {
                best = slide;
                c.marks.retain(|(n, _)| n != "4-power");
                c.mark("4-power");
            }
        }
    }
    println!("power corner: largest slide under full throttle {best:.0} degrees");
    grass
}

/// The base course: the start, the climb, the ford, the hairpin and the
/// finish, a stop of 4 s or more at each (marks at the stops).
fn base_course() -> Course {
    let mut c = Course::new();
    let ford = ford_point();
    c.idle(150);
    c.mark("1-start");
    // The climb through the trees.
    c.drive(20, 14, 900, &|p| p.last_road >= 30);
    c.stop();
    c.idle(60);
    c.mark("2-forest");
    c.idle(60);
    c.back_to_road();
    if extra("spray") {
        // R3: through the ford at 20 m/s (the spray at full size), a
        // stop past it, then 4 s or more backwards through the water in
        // reverse (small sheets all the way), Y.
        let wet = |p: &Play| surface_at(nearest_k(p.car.pos[0], p.car.pos[2])) == FORD;
        c.drive(20, 20, 900, &|p| wet(p));
        for _ in 0..8 {
            let i = autopilot(&c.p, 20 * M / 30, 20 * M / 30);
            c.step(i);
        }
        c.mark("3-splash");
        c.drive(20, 20, 900, &|p| !wet(p));
        c.stop();
        c.idle(120);
        let back = Controls {
            aim: 0,
            throttle: 0,
            brake: 256,
            handbrake: false,
        };
        let mut in_water = 0;
        for _ in 0..600 {
            c.hold(back, 1);
            if wet(&c.p) {
                in_water += 1;
                if in_water >= 40 {
                    break;
                }
            } else if in_water > 0 {
                break;
            }
        }
        c.mark("3-reverse");
        c.stop_any();
        c.idle(120);
        // A circle in the water at 8 m/s, full lock right, 5 s.
        let mut wet_steps = 0;
        for n in 0..150 {
            let v = c.p.car.speed();
            let throttle = if v < 8 * M / 30 { 256 } else { 0 };
            c.hold(
                Controls {
                    aim: 256,
                    throttle,
                    brake: 0,
                    handbrake: false,
                },
                1,
            );
            if wet(&c.p) {
                wet_steps += 1;
            }
            if n == 100 {
                c.mark("3-circle");
            }
        }
        println!("circle: {wet_steps} of 150 steps in the water");
        println!("reversing in the water: {in_water} steps");
        c.idle(45);
        c.back_to_road();
    } else {
        // Slowly into the ford, and stand in the water.
        c.drive(20, 14, 900, &|p| usize::from(p.last_road) + 12 >= ford);
        c.drive(6, 6, 900, &|p| p.car.surface == FORD);
        c.stop();
        c.idle(60);
        c.mark("3-ford");
        c.idle(60);
        c.back_to_road();
    }
    if extra("grass") {
        // R1: straight on at the second corner's entry, full throttle
        // over the grass for 4 s (it holds the car to 12 m/s).
        c.drive(20, 14, 1_200, &|p| p.last_road >= 122);
        let mut on = 0;
        for _ in 0..240 {
            c.hold(
                Controls {
                    aim: 0,
                    throttle: 256,
                    brake: 0,
                    handbrake: false,
                },
                1,
            );
            if matches!(c.p.car.surface, GRASS | LOW_GRASS) {
                on += 1;
                if on == 60 {
                    c.mark("4-grass");
                }
            }
            if on >= 120 {
                break;
            }
        }
        c.stop();
        c.idle(30);
        c.back_to_road();
    }
    if extra("power") {
        // R2: the second corner taken on the throttle in a slide, the
        // way tests/review_rules.rs calls "h held slide, full" (its
        // fastest plan on the road: 20.5 m/s in, the handbrake 4 m
        // before the corner for 6 steps, the nose held up to 30 degrees
        // inside the travel, full throttle all through the corner).
        c.drive(20, 14, 1_200, &|p| p.last_road >= 116);
        let grass = power_corner(&mut c);
        println!(
            "power corner: {grass} steps on grass, out at {:.1} m/s",
            f64::from(c.p.car.speed()) * 30.0 / f64::from(M)
        );
        c.mark("5-exit");
        c.stop();
        c.idle(60);
        c.mark("5-stop");
        c.idle(60);
        c.back_to_road();
    } else {
        // Into the hairpin.
        c.drive(20, 14, 1_200, &|p| p.last_road >= 134);
        c.stop();
        c.idle(60);
        c.mark("5-hairpin");
        c.idle(60);
        c.back_to_road();
    }
    // Past the finish.
    c.drive(20, 14, 1_200, &|p| {
        p.finished.is_some() && p.car.speed() == 0
    });
    c.idle(60);
    c.mark("6-finish");
    c.idle(60);
    c
}

#[test]
#[ignore]
fn life_pictures_and_pad() {
    let Ok(dir) = std::env::var("LIFE_OUT") else {
        return;
    };
    let level = match std::env::var("LIFE_LEVEL").as_deref() {
        Ok("LOW") => Detail::Low,
        Ok("HIGH") => Detail::High,
        Ok("MID") => Detail::Mid,
        Ok("MAX") => Detail::Max,
        _ => Detail::START,
    };
    std::fs::create_dir_all(format!("{dir}/pad")).expect("folder");
    let c = base_course();
    let head = "# DRIVE3D course: start, climb, ford, hairpin, finish (4 s stops)\n\
                # from right after the game's tile was opened; the level is\n\
                # the game's (HIGH at the start; X steps it, the course is the same)\n";
    let head = if extra("power") {
        "# DRIVE3D course: start, climb, ford (4 s stops), the second corner\n\
         # on the throttle in a slide (no stop in it), a 4 s stop after it,\n\
         # finish; from right after the game's tile was opened (HIGH)\n"
    } else if extra("spray") {
        "# DRIVE3D course: start, climb (4 s stop), through the ford at\n\
         # 20 m/s (the spray shows only while the car moves), a 4 s stop\n\
         # past it, back into the water in reverse, a 4 s stop, a 5 s\n\
         # circle at 8 m/s in the water, a 1.5 s stop and Y, then on:\n\
         # hairpin, finish; from right after the game's tile was opened\n\
         # (HIGH)\n"
    } else {
        head
    };
    std::fs::write(format!("{dir}/pad/course.pad"), pad_script(&c, 2_000, head)).expect("pad");
    let mut counts = String::new();
    for (name, at) in &c.marks {
        let p = c.states[*at];
        let (f, n) = shot(&p, level);
        let tag = format!("{name}-{}", level.name().to_lowercase());
        std::fs::write(format!("{dir}/{tag}.png"), png(&f, 4)).expect("png");
        let ms = 2_000 + (*at as u64 * 1000) / 30;
        counts.push_str(&format!("{tag}: pad time {ms} ms, step {at}: {n}\n"));
    }
    println!("{counts}");
    std::fs::write(format!("{dir}/counts.txt"), counts).expect("counts");
}
