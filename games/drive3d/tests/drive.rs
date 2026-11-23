//! Whole runs of the game's rules: the scripted drive, random input,
//! the fingerprint, Y, START and the finish.

mod common;

use common::autopilot;
use drive3d::car::Controls;
use drive3d::land::{Ground, Stretch};
use drive3d::play::{Input, Play};
use drive3d::road::POINTS;
use drive3d::tune::{CELL, CELLS_X, CELLS_Z, CLEAR, EDGE, M, RESET_STEPS};

/// The scripted drive: the test driver at 20 m/s, 14 m/s in the
/// corners, until it stands past the finish. Returns the play and the
/// steps run.
fn scripted() -> (Play, u32) {
    let mut p = Play::new();
    for n in 0..1_500 {
        let input = autopilot(&p, 20 * M / 30, 14 * M / 30);
        p.step(&input);
        if p.finished.is_some() && p.car.speed() == 0 {
            return (p, n + 1);
        }
    }
    (p, 1_500)
}

#[test]
fn a_scripted_drive_finishes_the_stretch() {
    // 344 m: at 20 m/s on 248 m of straights (12.4 s) and 14 m/s on the
    // 88 m of corners (6.3 s), plus the start (0 to 20 m/s, about 3 s
    // at 9 to 4 m/s/s, losing about 1.5 s) and the slowing before the
    // corners: about 21 s, 630 steps (ESTIMATED). MEASURED 643 steps.
    let (p, _) = scripted();
    let t = p.finished.expect("finished");
    assert!((560..=720).contains(&t), "{t} steps");
    assert_eq!(usize::from(p.last_road), POINTS - 1);
    assert_eq!(p.resets, 0);
    assert_eq!(p.hits, 0);
    // Over the crest at about 20 m/s the car flew once.
    assert!(p.flights >= 1);
    println!("finished after {t} steps ({} s)", t / 30);
}

#[test]
fn the_same_inputs_give_the_same_fingerprint() {
    let (a, na) = scripted();
    let (b, nb) = scripted();
    assert_eq!(na, nb);
    assert_eq!(a.print, b.print);
    assert_eq!(a, b);
    // The value, pinned: a change of any rule shows here. (Update it
    // after a deliberate change.)
    println!("fingerprint {:#018x} after {na} steps", a.print);
    assert_eq!(a.print, FINGERPRINT);
}

/// The scripted drive's fingerprint (MEASURED, PC). Re-pinned in the
/// final round: the fingerprint now also takes in the run's clock, the
/// waiting Y, the time in the air, the blocked count and the dust
/// (review rules F8), and the clock and the edge rules changed (F1,
/// F2, F3). Re-pinned in R1 (2026-11-22): the grass is slower and
/// slipperier (the road wins the corner). Re-pinned in R2
/// (2026-11-22): the throttle held in a slide gives grip back.
const FINGERPRINT: u64 = 0xf2ef_684b_235e_18dd;

/// A small fixed random number maker (xorshift), so the run is the
/// same every time.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }
    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % (hi - lo + 1) as u32) as i32
    }
}

#[test]
fn ten_minutes_of_random_input_stay_inside_and_above_the_land() {
    // 18,000 steps; a new random input every 1 to 30 steps; Y now and
    // then, START rarely. Overflow checks are on in tests. Every step:
    // the car's middle inside the block (8 m from its edges), never
    // under the land, the camera at least CLEAR above the land.
    let mut p = Play::new();
    let mut r = Rng(0x2545_f491_4f6c_dd1d);
    let mut input = Input::default();
    let mut left = 0;
    let (sx, sz) = (CELLS_X as i32 * CELL, CELLS_Z as i32 * CELL);
    let mut air = 0u32;
    let mut far = 0u32;
    for _ in 0..18_000 {
        if left == 0 {
            left = r.range(1, 30);
            input = Input {
                controls: Controls {
                    aim: r.range(-256, 256),
                    throttle: if r.range(0, 3) > 0 {
                        256
                    } else {
                        r.range(0, 256)
                    },
                    brake: if r.range(0, 4) == 0 { 256 } else { 0 },
                    handbrake: r.range(0, 5) == 0,
                },
                reset: r.range(0, 60) == 0,
                restart: r.range(0, 400) == 0,
                level: r.range(0, 100) == 0,
            };
        } else {
            input.reset = false;
            input.restart = false;
            input.level = false;
        }
        left -= 1;
        p.step(&input);
        let c = &p.car;
        assert!((EDGE..=sx - EDGE).contains(&c.pos[0]), "x {}", c.pos[0]);
        assert!((EDGE..=sz - EDGE).contains(&c.pos[2]), "z {}", c.pos[2]);
        let land = Stretch.at(c.pos[0], c.pos[2]).height;
        assert!(c.pos[1] >= land, "under the land by {}", land - c.pos[1]);
        if !c.air {
            assert_eq!(c.pos[1], land, "{:?} {:?}", p.events, c);
        } else {
            air += 1;
        }
        let cam_land = Stretch.at(p.cam.pos[0], p.cam.pos[2]).height;
        assert!(p.cam.pos[1] - cam_land >= CLEAR);
        if c.pos[0] < 2 * EDGE || c.pos[2] < 2 * EDGE {
            far += 1;
        }
    }
    println!("steps in the air {air}, near the edge {far}");
}

#[test]
fn y_puts_the_car_back_on_the_road_after_45_steps() {
    // Drive off the road to the right, press Y: for RESET_STEPS - 1
    // steps the car goes on; on the 45th it stands on the last road
    // point passed, along the road, with no speed.
    let mut p = Play::new();
    let gas_right = Input {
        controls: Controls {
            aim: 128,
            throttle: 256,
            brake: 0,
            handbrake: false,
        },
        ..Input::default()
    };
    for _ in 0..90 {
        p.step(&gas_right);
    }
    let k = usize::from(p.last_road);
    assert!(k > 0);
    p.step(&Input {
        reset: true,
        ..gas_right
    });
    assert_eq!(p.reset_in, RESET_STEPS - 1);
    for _ in 0..RESET_STEPS - 2 {
        p.step(&gas_right);
        assert!(p.reset_in > 0);
    }
    p.step(&Input::default());
    assert!(p.events.put_back);
    let k = usize::from(p.last_road);
    let road = &drive3d::road::ROAD;
    assert_eq!((p.car.pos[0], p.car.pos[2]), (road.x[k], road.z[k]));
    assert_eq!(p.car.yaw, road.heading[k]);
    assert_eq!(p.car.vel, [0, 0]);
    assert_eq!(p.resets, 1);
}

#[test]
fn start_begins_again_and_keeps_the_level() {
    let mut p = Play::new();
    let gas = Input {
        controls: common::GAS,
        ..Input::default()
    };
    for _ in 0..60 {
        p.step(&gas);
    }
    p.step(&Input {
        level: true,
        ..Input::default()
    });
    // X from HIGH, the start.
    assert_eq!(p.detail.name(), "MAX");
    p.step(&Input {
        restart: true,
        ..Input::default()
    });
    let mut fresh = Play::new();
    fresh.detail = p.detail;
    fresh.events = p.events;
    assert_eq!(p, fresh);
    assert!(p.events.restarted);
}

#[test]
fn past_the_finish_the_car_brakes_to_a_stop_and_the_time_stays() {
    let (p, _) = scripted();
    let t = p.finished.unwrap();
    assert_eq!(p.car.speed(), 0);
    let mut p = p;
    for _ in 0..60 {
        p.step(&Input {
            controls: common::GAS,
            ..Input::default()
        });
    }
    // Throttle does nothing after the finish; the time is kept.
    assert_eq!(p.car.speed(), 0);
    assert_eq!(p.finished, Some(t));
}

/// The stick's x that gives `aim` through `stick_aim` (the smallest one).
fn stick_for(aim: i32) -> i16 {
    if aim == 0 {
        return 0;
    }
    let mut a = drive3d::tune::DEAD + (aim.abs() * (32_767 - drive3d::tune::DEAD)) / 256;
    while drive3d::car::stick_aim(a as i16) < aim.abs() {
        a += 1;
    }
    (if aim < 0 { -a } else { a }) as i16
}

/// The scripted drive as a simulator input script, when `DRIVE3D_SCRIPT`
/// names a file: from the home screen (two RIGHTs to the DRIVE3D tile,
/// the third one, A to start; the first key only at frame 146, after the simulator's
/// default GATHER boot animation has ended by itself, about 81 frames,
/// so the same script works with `--boot off`: a key during the
/// animation only skips it) the test driver's controls frame by frame (one step a
/// frame at 30 a second: `STEP_US` is the simulator's tick), A for the
/// throttle, B for the brake and the left stick for the steering, each
/// first turned back through `controls` so the run in the simulator
/// is this one. Eight `shot` lines spread over the drive.
#[test]
fn the_scripted_drive_as_a_simulator_script() {
    let Ok(path) = std::env::var("DRIVE3D_SCRIPT") else {
        return;
    };
    use game_kit::button;
    const FIRST: u32 = 180;
    let mut p = Play::new();
    let mut lines = vec![
        "# DRIVE3D: home, two RIGHTs to its tile, A to start, then the".to_string(),
        "# test driver's drive to the finish (made by tests/drive.rs).".to_string(),
        "# From boards/desktop: cargo run --release --offline --locked --".to_string(),
        "#   --frames N --script FILE --shot last --out FOLDER [--boot off]".to_string(),
        "# (N on the last line; the default boot and --boot off both work).".to_string(),
        "146 press right".to_string(),
        "147 release right".to_string(),
        "152 press right".to_string(),
        "153 release right".to_string(),
        "160 press south".to_string(),
        "161 release south".to_string(),
    ];
    let (mut held, mut stick) = (0u32, 0i16);
    let mut finish = None;
    let mut n = 0u32;
    while n < 1_500 {
        let a = autopilot(&p, 20 * M / 30, 14 * M / 30).controls;
        let want =
            if a.throttle > 0 { button::A } else { 0 } | if a.brake > 0 { button::B } else { 0 };
        let s = stick_for(a.aim);
        let frame = FIRST + n;
        for (bit, name) in [(button::A, "south"), (button::B, "east")] {
            if want & bit != held & bit {
                let word = if want & bit != 0 { "press" } else { "release" };
                lines.push(format!("{frame} {word} {name}"));
            }
        }
        if s != stick {
            lines.push(format!("{frame} stick left {s} 0"));
        }
        held = want;
        stick = s;
        let c = drive3d::controls(held, stick, (0, 0));
        assert_eq!(c.aim, a.aim, "the stick gives the driver's aim");
        p.step(&Input {
            controls: c,
            ..Input::default()
        });
        n += 1;
        if finish.is_none() && p.finished.is_some() {
            finish = Some(n);
        }
        if p.finished.is_some() && p.car.speed() == 0 {
            break;
        }
    }
    let end = FIRST + n;
    lines.push(format!("{end} release south"));
    lines.push(format!("{end} release east"));
    lines.push(format!("{end} stick left 0 0"));
    let fin = finish.expect("finished");
    let mut shots = vec![170];
    shots.extend((1..6).map(|k| FIRST + fin * k / 6));
    shots.push(FIRST + fin + 2);
    shots.push(end + 10);
    for s in &shots {
        lines.push(format!("{s} shot"));
    }
    lines.push(format!(
        "# finish after {fin} steps, stands after {n}; run with --frames {}",
        end + 12
    ));
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
}
