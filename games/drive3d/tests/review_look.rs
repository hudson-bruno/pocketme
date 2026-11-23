//! A reviewer's look at the picture (not a builder's test; it asserts
//! nothing about the game). With `DRIVE3D_REVIEW=<folder>` it writes
//! simulator input scripts for several drives into that folder and a
//! file of counts (`counts.txt`): triangles, pixels, cells, the car's
//! pixels, things that pop, the horizon, and how early the two corners
//! show. Without the variable it does nothing.
//!
//! Run: `DRIVE3D_REVIEW=<folder> cargo test --offline --test review_look -- --nocapture`

mod common;

use common::{autopilot, car_on_screen};
use drive3d::camera::VIEW;
use drive3d::detail::Detail;
use drive3d::draw::{self, Opts};
use drive3d::land::{Ground, Stretch};
use drive3d::look::Look;
use drive3d::maths::{angle_diff, dir, right, yaw_of};
use drive3d::models::{MATS, SHADES};
use drive3d::play::{Input, Play};
use drive3d::road::{POINTS, ROAD};
use drive3d::scene::{Painter, Rec};
use drive3d::things::{THINGS, kind};
use drive3d::tune::{CELL, CELLS_X, CELLS_Z, M};
use game_3d::{Camera, Object};
use game_kit::{Frame, H, W, button};
use std::fmt::Write as _;

/// The frame of the first driven step (the game starts at about 61).
const FIRST: u32 = 80;

/// One frame's pad.
#[derive(Clone, Default)]
struct Pad {
    a: bool,
    b: bool,
    r: bool,
    aim: i32,
    /// Buttons pressed and let go on this frame.
    taps: Vec<&'static str>,
}

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

/// A recorded drive: the script's lines and the state after each step
/// (with its frame and whether the line is on).
struct Drive {
    name: &'static str,
    lines: Vec<String>,
    states: Vec<(u32, Play, bool)>,
}

fn record(
    name: &'static str,
    steps: u32,
    every: u32,
    mut f: impl FnMut(&Play, u32) -> Pad,
) -> Drive {
    let mut lines = vec![
        format!("# review-look: {name} (made by tests/review_look.rs)"),
        "46 press right".into(),
        "47 release right".into(),
        "52 press right".into(),
        "53 release right".into(),
        "60 press south".into(),
        "61 release south".into(),
        "75 shot".into(),
    ];
    let mut p = Play::new();
    let mut held = Pad::default();
    let mut stick = 0i16;
    let mut line = false;
    let mut states = Vec::new();
    for n in 0..steps {
        let pad = f(&p, n);
        let frame = FIRST + n;
        for (now, was, nm) in [
            (pad.a, held.a, "south"),
            (pad.b, held.b, "east"),
            (pad.r, held.r, "r"),
        ] {
            if now != was {
                let w = if now { "press" } else { "release" };
                lines.push(format!("{frame} {w} {nm}"));
            }
        }
        let s = stick_for(pad.aim);
        if s != stick {
            lines.push(format!("{frame} stick left {s} 0"));
        }
        for t in &pad.taps {
            lines.push(format!("{frame} press {t}"));
            lines.push(format!("{frame} release {t}"));
            if *t == "minus" {
                line = !line;
            }
        }
        stick = s;
        let bits = if pad.a { button::A } else { 0 }
            | if pad.b { button::B } else { 0 }
            | if pad.r { button::R } else { 0 };
        let c = drive3d::controls(bits, stick, (0, 0));
        p.step(&Input {
            controls: c,
            reset: pad.taps.contains(&"north"),
            restart: false,
            level: pad.taps.contains(&"west"),
        });
        held = pad;
        states.push((frame, p, line));
        if n % every == every - 1 {
            lines.push(format!("{frame} shot"));
        }
    }
    let end = FIRST + steps;
    lines.push(format!("{end} release south"));
    lines.push(format!("{end} release east"));
    lines.push(format!("{end} release r"));
    lines.push(format!("{end} stick left 0 0"));
    lines.push(format!("# run with --frames {}", end + 2));
    Drive {
        name,
        lines,
        states,
    }
}

/// The autopilot's controls as a pad.
fn pilot(p: &Play, cruise: i32, corner: i32) -> Pad {
    let c = autopilot(p, cruise, corner).controls;
    Pad {
        a: c.throttle > 0,
        b: c.brake > 0,
        aim: c.aim,
        ..Pad::default()
    }
}

/// Drive toward a point at a speed.
fn go_to(p: &Play, tx: i32, tz: i32, speed: i32) -> Pad {
    let c = &p.car;
    let want = yaw_of(tx - c.pos[0], tz - c.pos[2]).unwrap_or(c.yaw);
    let off = angle_diff(c.yaw, want);
    let aim = (off * 256 / 2_912).clamp(-256, 256);
    let v = c.speed();
    Pad {
        a: v < speed,
        b: v > speed + M / 30,
        aim,
        ..Pad::default()
    }
}

fn piece_of(p: &Play) -> u8 {
    ROAD.piece[usize::from(p.last_road).min(POINTS - 1)]
}

/// Things on one side of the road (right when `right_side`), in road
/// order: (x, z, thing index).
fn things_on(right_side: bool) -> Vec<(i32, i32, usize)> {
    let mut out = Vec::new();
    for (i, t) in THINGS.iter().enumerate() {
        if !matches!(t.kind, kind::TREE | kind::HUT | kind::ROCK) {
            continue;
        }
        let (x, z) = t.at();
        let mut best = (i64::MAX, 0usize);
        for k in 0..POINTS {
            let (dx, dz) = (i64::from(x - ROAD.x[k]), i64::from(z - ROAD.z[k]));
            let d = dx * dx + dz * dz;
            if d < best.0 {
                best = (d, k);
            }
        }
        let k = best.1;
        let (rx, rz) = right(ROAD.heading[k]);
        let side =
            i64::from(x - ROAD.x[k]) * i64::from(rx) + i64::from(z - ROAD.z[k]) * i64::from(rz);
        if (side > 0) == right_side {
            out.push((k as i32, x, z, i));
        }
    }
    out.sort();
    out.into_iter().map(|(_, x, z, i)| (x, z, i)).collect()
}

fn drives() -> Vec<Drive> {
    let cruise = 20 * M / 30;
    let corner = 14 * M / 30;
    let mut out = Vec::new();
    // 1-3: clean drives at each level, the line on (X twice: LOW; once:
    // HIGH). At HIGH the line goes off half-way.
    for (name, xs) in [("clean-mid", 0), ("clean-low", 2), ("clean-high", 1)] {
        let every = if name == "clean-mid" { 12 } else { 15 };
        out.push(record(name, 720, every, |p, n| {
            let mut pad = pilot(p, cruise, corner);
            if n == 0 {
                pad.taps.push("minus");
            }
            if n >= 1 && n <= xs {
                pad.taps.push("west");
            }
            if name == "clean-high" && n == 360 {
                pad.taps.push("minus");
            }
            pad
        }));
    }
    // 4: too fast: 28 m/s on the straights and 24 in the corners.
    out.push(record("fast", 700, 12, |p, n| {
        let mut pad = pilot(p, 28 * M / 30, 24 * M / 30);
        if n == 0 {
            pad.taps.push("minus");
        }
        pad
    }));
    // 5: a bad drive: into a thing on the right, to the right edge, Y;
    // into a thing on the left, to the left edge, along it and back in
    // (the camera outside the land), Y.
    let right_things = things_on(true);
    let left_things = things_on(false);
    let mut phase = 0u32;
    let mut since = 0u32;
    let r_t = right_things[0];
    let l_t = *left_things.get(1).unwrap_or(&left_things[0]);
    let wide = CELLS_X as i32 * CELL;
    out.push(record("bad", 1100, 12, move |p, n| {
        since += 1;
        let c = &p.car;
        let mut pad = match phase {
            0 => go_to(p, r_t.0, r_t.1, 14 * M / 30),
            1 => go_to(p, wide - 2 * M, c.pos[2] + 40 * M, 18 * M / 30),
            2 => Pad::default(),
            3 => go_to(p, l_t.0, l_t.1, 14 * M / 30),
            4 => go_to(p, 2 * M, c.pos[2] + 30 * M, 18 * M / 30),
            5 => go_to(p, 4 * M, c.pos[2] + 80 * M, 12 * M / 30),
            6 => go_to(p, 60 * M, c.pos[2] - 20 * M, 12 * M / 30),
            _ => Pad::default(),
        };
        let next = match phase {
            0 | 3 => p.events.hit.is_some() || since > 220,
            1 | 4 => since > 200,
            5 => since > 90,
            6 => since > 90,
            2 => since > 55,
            _ => false,
        };
        if n == 0 {
            pad.taps.push("minus");
        }
        if next {
            phase += 1;
            since = 0;
            if phase == 2 || phase == 7 {
                pad.taps.push("north");
            }
        }
        pad
    }));
    // 6: slides: on the first straight full lock right with the
    // handbrake, then full lock left with it; Y; then the corners
    // with handbrake bursts at 17 m/s.
    let mut ph = 0u32;
    let mut since = 0u32;
    out.push(record("slides", 900, 12, move |p, n| {
        since += 1;
        let mut pad = match ph {
            0 => pilot(p, 22 * M / 30, corner),
            1 => Pad {
                a: true,
                r: since % 12 < 5,
                aim: 256,
                ..Pad::default()
            },
            2 => Pad {
                a: true,
                r: since % 12 < 5,
                aim: -256,
                ..Pad::default()
            },
            3 => Pad::default(),
            _ => {
                let mut q = pilot(p, 20 * M / 30, 17 * M / 30);
                let pc = piece_of(p);
                if (pc == 5 || pc == 7) && n % 14 < 5 {
                    q.r = true;
                }
                q
            }
        };
        let next = match ph {
            0 => since > 75,
            1 => since > 30,
            2 => since > 36,
            3 => since > 55,
            _ => false,
        };
        if n == 0 {
            pad.taps.push("minus");
        }
        if next {
            ph += 1;
            since = 0;
            if ph == 3 {
                pad.taps.push("north");
            }
        }
        pad
    }));
    // 7: standing still, then reversing (straight, left, right), off
    // the start's end of the land.
    out.push(record("still-reverse", 420, 12, |_p, n| {
        let mut pad = Pad::default();
        if n == 30 {
            pad.taps.push("minus");
        }
        if (90..330).contains(&n) {
            pad.b = true;
            if (150..210).contains(&n) {
                pad.aim = -256;
            } else if (210..270).contains(&n) {
                pad.aim = 256;
            }
        }
        pad
    }));
    // 8: the land's edges: to the left edge, along it, reverse into it
    // (the camera outside the land), across to the right edge, then
    // along it to the far end of the land.
    let long = CELLS_Z as i32 * CELL;
    let mut ph = 0u32;
    let mut since = 0u32;
    out.push(record("edge", 1300, 10, move |p, n| {
        since += 1;
        let c = &p.car;
        let mut pad = match ph {
            0 => go_to(p, 2 * M, c.pos[2] + 12 * M, 16 * M / 30),
            1 => go_to(p, 2 * M, c.pos[2] + 60 * M, 14 * M / 30),
            2 => Pad {
                b: c.speed() > 0,
                ..Pad::default()
            },
            3 => Pad {
                b: true,
                aim: -256,
                ..Pad::default()
            },
            4 => go_to(p, wide - 2 * M, c.pos[2] + 20 * M, 18 * M / 30),
            5 => go_to(p, wide - 2 * M, long, 18 * M / 30),
            _ => Pad::default(),
        };
        let next = match ph {
            0 => c.pos[0] < 12 * M || since > 400,
            1 => since > 120,
            2 => since > 40,
            3 => since > 90,
            4 => c.pos[0] > wide - 12 * M || since > 500,
            5 => since > 400,
            _ => false,
        };
        if n == 0 {
            pad.taps.push("minus");
        }
        if n % 100 == 0 {
            eprintln!(
                "edge n={n} ph={ph} pos=({}, {}) m speed={}",
                c.pos[0] / M,
                c.pos[2] / M,
                c.speed() * 30 / M
            );
        }
        if next {
            ph += 1;
            since = 0;
        }
        pad
    }));
    out
}

// ---- Counts ----------------------------------------------------------

/// Records the things drawn (their (x, z) in game units).
#[derive(Default)]
struct Seen {
    objs: Vec<[i32; 3]>,
}

impl Rec for Seen {
    fn object(&mut self, o: &Object<'_>, _cam: &Camera) {
        self.objs.push(o.pos);
    }
}

struct Pic {
    tris: u32,
    pixels: u32,
    dropped: u32,
    cells: u32,
    car_px: u32,
    things: Vec<usize>,
}

fn is_car(c: u8) -> bool {
    c < MATS * SHADES && (c % MATS) <= 5
}

fn picture(p: &Play, d: Detail) -> Pic {
    let mut p = *p;
    p.detail = d;
    let mut paint = Box::new(Painter::new());
    let mut f = Box::new(Frame([0; W * H]));
    let look = Look::new(&p, 1);
    let mut seen = Seen::default();
    draw::picture(
        &mut f,
        &mut paint,
        &look,
        None,
        Opts {
            line: false,
            plain: false,
        },
        &mut seen,
    );
    let car_px = f.0.iter().filter(|c| is_car(**c)).count() as u32;
    let mut things = Vec::new();
    for (i, t) in THINGS.iter().enumerate() {
        let (x, z) = t.at();
        if seen
            .objs
            .iter()
            .any(|o| (o[0] - x / 16).abs() <= 2 && (o[2] - z / 16).abs() <= 2)
        {
            things.push(i);
        }
    }
    let st = paint.r.stats;
    Pic {
        tris: st.tris,
        pixels: st.pixels,
        dropped: st.dropped,
        cells: paint.cells,
        car_px,
        things,
    }
}

fn pct(v: &mut [u32], q: usize) -> u32 {
    v.sort_unstable();
    v[(v.len() - 1) * q / 100]
}

/// Where a world point (subs) lands on the picture (pixels) and its
/// depth (game units), or None behind the eye.
fn on_pic(p: &Play, w: [i32; 3]) -> Option<(i32, i32, i32)> {
    let cam = p.cam.camera();
    let q = cam.to_cam([w[0] / 16, w[1] / 16, w[2] / 16]);
    if q[2] < 64 {
        return None;
    }
    let s = VIEW.project(q);
    Some((s[0] / 16, s[1] / 16, q[2]))
}

/// The bend's mark (12 m into the corner) is in the picture, within
/// `far` m and not behind the land (the land's heights on the line).
fn bend_shows(p: &Play, k: usize, far: i32) -> bool {
    let w = [ROAD.x[k], ROAD.y[k] + M / 4, ROAD.z[k]];
    let Some((x, y, _)) = on_pic(p, w) else {
        return false;
    };
    if !(0..160).contains(&x) || !(0..120).contains(&y) {
        return false;
    }
    let e = p.cam.pos;
    let (dx, dz) = (i64::from(w[0] - e[0]), i64::from(w[2] - e[2]));
    if dx * dx + dz * dz > i64::from(far * M) * i64::from(far * M) {
        return false;
    }
    for s in 1..30 {
        let f = |a: i32, b: i32| a + ((i64::from(b - a) * s) / 32) as i32;
        let (x, y, z) = (f(e[0], w[0]), f(e[1], w[1]), f(e[2], w[2]));
        if Stretch.at(x, z).height > y {
            return false;
        }
    }
    true
}

#[test]
fn review_the_look() {
    let Ok(dir_out) = std::env::var("DRIVE3D_REVIEW") else {
        return;
    };
    let dir_out = std::path::PathBuf::from(dir_out);
    std::fs::create_dir_all(&dir_out).unwrap();
    let mut rep = String::new();
    let all = drives();
    for d in &all {
        std::fs::write(
            dir_out.join(format!("{}.txt", d.name)),
            d.lines.join("\n") + "\n",
        )
        .unwrap();
    }
    // Per drive, per level: triangles, pixels, cells, dropped, car
    // pixels, pops, horizon.
    for d in &all {
        writeln!(rep, "== {} ({} steps)", d.name, d.states.len()).unwrap();
        for lv in [Detail::Low, Detail::Mid, Detail::High] {
            let mut tris = Vec::new();
            let mut pix = Vec::new();
            let mut cells = Vec::new();
            let mut dropped = 0;
            let mut worst = (0u32, 0u32);
            let mut car_min = (u32::MAX, 0u32);
            let mut car_drops = Vec::new();
            let mut pops = Vec::new();
            let mut prev: Option<Pic> = None;
            for (frame, p, _) in &d.states {
                let pic = picture(p, lv);
                tris.push(pic.tris);
                pix.push(pic.pixels);
                cells.push(pic.cells);
                dropped += pic.dropped;
                if pic.tris > worst.0 {
                    worst = (pic.tris, *frame);
                }
                let os = car_on_screen(p);
                let on = (0..160).contains(&os.mid.0) && (0..120).contains(&os.mid.1);
                if on && pic.car_px < car_min.0 {
                    car_min = (pic.car_px, *frame);
                }
                if let Some(pv) = &prev {
                    if pv.car_px > 60 && pic.car_px * 10 < pv.car_px * 6 {
                        car_drops.push(format!("{}:{}->{}", frame, pv.car_px, pic.car_px));
                    }
                    // A thing drawn last picture and not now, while still
                    // in the picture and well inside the reach: a pop.
                    let reach = i64::from(lv.reach().things_m) * i64::from(M);
                    for &i in &pv.things {
                        if pic.things.contains(&i) {
                            continue;
                        }
                        let (x, z) = THINGS[i].at();
                        let y = Stretch.at(x, z).height + THINGS[i].height() / 2;
                        let Some((sx, sy, _)) = on_pic(p, [x, y, z]) else {
                            continue;
                        };
                        let e = p.cam.pos;
                        let (dx, dz) = (i64::from(x - e[0]), i64::from(z - e[2]));
                        let dist = ((dx * dx + dz * dz) as f64).sqrt();
                        if (-8..168).contains(&sx)
                            && (-8..128).contains(&sy)
                            && dist < reach as f64 - 3.0 * f64::from(M)
                        {
                            pops.push(format!(
                                "{}:thing{}({})@{:.0}m",
                                frame,
                                i,
                                THINGS[i].kind,
                                dist / f64::from(M)
                            ));
                        }
                    }
                }
                prev = Some(pic);
            }
            let n = tris.len() as u32;
            let mean = tris.iter().sum::<u32>() / n;
            let pmean = pix.iter().sum::<u32>() / n;
            let pmax = *pix.iter().max().unwrap();
            let cmax = *cells.iter().max().unwrap();
            let p95 = pct(&mut tris.clone(), 95);
            writeln!(
                rep,
                "  {:4} tris mean {mean} p95 {p95} worst {} (frame {}) budget {} | pixels mean {pmean} max {pmax} | cells max {cmax} | dropped {dropped}",
                lv.name(),
                worst.0,
                worst.1,
                lv.reach().budget
            )
            .unwrap();
            writeln!(
                rep,
                "       car pixels min {} (frame {}); sudden drops {}: {:?}",
                car_min.0,
                car_min.1,
                car_drops.len(),
                &car_drops[..car_drops.len().min(12)]
            )
            .unwrap();
            writeln!(
                rep,
                "       pops inside reach {}: {:?}",
                pops.len(),
                &pops[..pops.len().min(16)]
            )
            .unwrap();
        }
        // Horizon and the car's place, at the drive's own level.
        let mut hmin = i32::MAX;
        let mut hmax = i32::MIN;
        let mut jump = (0, 0u32);
        let mut prev_h = None;
        let mut hmin_car = (i32::MAX, 0u32);
        let mut hmax_car = (i32::MIN, 0u32);
        let mut xr = (i32::MAX, i32::MIN);
        let mut yr = (i32::MAX, i32::MIN);
        let mut steady = 0u32;
        for (frame, p, _) in &d.states {
            let h = draw::horizon(p.cam.pitch);
            hmin = hmin.min(h);
            hmax = hmax.max(h);
            if let Some(ph) = prev_h {
                let j: i32 = h - ph;
                if j.abs() > jump.0 {
                    jump = (j.abs(), *frame);
                }
                if j != 0 {
                    steady += 1;
                }
            }
            prev_h = Some(h);
            let os = car_on_screen(p);
            if os.height_px < hmin_car.0 {
                hmin_car = (os.height_px, *frame);
            }
            if os.height_px > hmax_car.0 {
                hmax_car = (os.height_px, *frame);
            }
            xr = (xr.0.min(os.mid.0), xr.1.max(os.mid.0));
            yr = (yr.0.min(os.mid.1), yr.1.max(os.mid.1));
        }
        writeln!(
            rep,
            "  horizon row {hmin}..{hmax}, biggest jump {} px (frame {}), frames it moved {steady}",
            jump.0, jump.1
        )
        .unwrap();
        writeln!(
            rep,
            "  car height on screen {}..{} px (frames {}, {}); middle x {}..{}, y {}..{}",
            hmin_car.0, hmax_car.0, hmin_car.1, hmax_car.1, xr.0, xr.1, yr.0, yr.1
        )
        .unwrap();
        let fin = d
            .states
            .iter()
            .find(|s| s.1.finished.is_some())
            .map(|s| s.0);
        let hits = d.states.last().map(|s| s.1.hits).unwrap_or(0);
        let resets = d.states.last().map(|s| s.1.resets).unwrap_or(0);
        writeln!(rep, "  finish frame {fin:?}, hits {hits}, resets {resets}").unwrap();
    }
    // How early the corners show, on the clean drives and the fast one.
    writeln!(rep, "== corners (bend mark 12 m into the corner)").unwrap();
    for d in &all {
        if !(d.name.starts_with("clean") || d.name == "fast") {
            continue;
        }
        for (piece, label) in [(5u8, "corner 1 (right 60)"), (7u8, "corner 2 (left 90)")] {
            let k0 = (0..POINTS).find(|&k| ROAD.piece[k] == piece).unwrap();
            let km = k0 + 6;
            let arrive = d
                .states
                .iter()
                .position(|s| usize::from(s.1.last_road) >= k0);
            let Some(arrive) = arrive else { continue };
            writeln!(
                rep,
                "  {:10} {label}: the car reaches it at frame {}",
                d.name, d.states[arrive].0
            )
            .unwrap();
            for lv in [Detail::Low, Detail::Mid, Detail::High] {
                let far = i32::from(lv.reach().far_m);
                let mut first = arrive;
                while first > 0 && bend_shows(&d.states[first - 1].1, km, far) {
                    first -= 1;
                }
                let shown = bend_shows(&d.states[arrive].1, km, far);
                let s = &d.states[first].1;
                let dist = {
                    let (dx, dz) = (
                        f64::from(ROAD.x[k0] - s.car.pos[0]),
                        f64::from(ROAD.z[k0] - s.car.pos[2]),
                    );
                    (dx * dx + dz * dz).sqrt() / f64::from(M)
                };
                let v = f64::from(s.car.speed()) * 30.0 / f64::from(M);
                writeln!(
                    rep,
                    "  {:10} {label} {:4}: shown {} frames before the car reaches it ({:.0} m, at {:.1} m/s), shown at arrival {shown}",
                    d.name,
                    lv.name(),
                    arrive - first,
                    dist,
                    v
                )
                .unwrap();
            }
        }
    }
    // The car's yaw against the road at the slide moments, for reading
    // the slide pictures.
    writeln!(rep, "== slide angles (frames with |side| > 2 m/s)").unwrap();
    for d in &all {
        let mut list = Vec::new();
        for (frame, p, _) in &d.states {
            let (f, s) = p.car.fwd_side();
            if s.abs() > 2 * M / 30 {
                let ang = f64::from(s).atan2(f64::from(f)).to_degrees();
                list.push(format!("{frame}:{ang:.0}"));
            }
        }
        writeln!(
            rep,
            "  {}: {} frames {:?}",
            d.name,
            list.len(),
            &list[..list.len().min(40)]
        )
        .unwrap();
    }
    // Where each drive ends, to check the frame mapping against the
    // simulator's log.
    for d in &all {
        let last = &d.states.last().unwrap().1;
        writeln!(
            rep,
            "  end of {}: pos {:?} yaw {} speed {} road point {}",
            d.name,
            last.car.pos,
            last.car.yaw,
            last.car.speed(),
            last.last_road
        )
        .unwrap();
        let _ = dir(0);
        let _ = CELLS_Z;
    }
    std::fs::write(dir_out.join("counts.txt"), &rep).unwrap();
    println!("{rep}");
}

/// Details of single moments (thing, drive, frame, level).
#[test]
fn review_moments() {
    if std::env::var("DRIVE3D_REVIEW").is_err() {
        return;
    }
    let all = drives();
    let probe = [
        ("slides", 799u32, 27usize, Detail::Mid),
        ("slides", 618, 27, Detail::High),
        ("slides", 619, 27, Detail::High),
        ("fast", 669, 27, Detail::High),
        ("fast", 670, 27, Detail::High),
        ("bad", 450, 8, Detail::High),
        ("bad", 451, 8, Detail::High),
        ("slides", 181, 8, Detail::High),
        ("slides", 182, 8, Detail::High),
        ("fast", 487, 27, Detail::High),
        ("fast", 488, 27, Detail::High),
        ("slides", 181, 9, Detail::High),
        ("bad", 456, 9, Detail::High),
        ("edge", 688, 10, Detail::High),
        ("edge", 689, 10, Detail::High),
        ("edge", 694, 9, Detail::High),
        ("edge", 695, 9, Detail::High),
        ("edge", 711, 8, Detail::High),
        ("edge", 712, 8, Detail::High),
    ];
    // The car's state where the puffs come and go (reversing at HIGH).
    for (name, lo, hi) in [
        ("still-reverse", 236u32, 247u32),
        ("slides", 398, 406),
        ("fast", 294, 300),
    ] {
        let d = all.iter().find(|d| d.name == name).unwrap();
        for (frame, p, _) in d.states.iter().filter(|s| (lo..=hi).contains(&s.0)) {
            let look = Look::new(p, 1);
            let pic = picture(p, Detail::High);
            println!(
                "puffs {name} frame {frame}: air {} surface {} vel {:?} kmh {} steps {} car px {}",
                look.car_air(),
                look.surface(),
                look.car_vel(),
                look.car_kmh(),
                look.steps(),
                pic.car_px
            );
        }
    }
    for (name, frame, ti, lv) in probe {
        let d = all.iter().find(|d| d.name == name).unwrap();
        let (_, p, _) = d.states.iter().find(|s| s.0 == frame).unwrap();
        let pic = picture(p, lv);
        let t = &THINGS[ti];
        let (x, z) = t.at();
        let y = Stretch.at(x, z).height;
        let e = p.cam.pos;
        let c = p.car.pos;
        let dc = (((x - e[0]) as f64).powi(2) + ((z - e[2]) as f64).powi(2)).sqrt() / f64::from(M);
        let dcar =
            (((x - c[0]) as f64).powi(2) + ((z - c[2]) as f64).powi(2)).sqrt() / f64::from(M);
        let hides = p.cam.hides_car(&p.car, x, z, t.radius());
        println!(
            "{name} frame {frame} {}: thing {ti} kind {} size {} drawn {} | from eye {dc:.1} m, from car {dcar:.1} m, on picture {:?}, top {:?}, hides_car(r) {hides} | car on picture {:?} car px {} | car yaw {} cam yaw {}",
            lv.name(),
            t.kind,
            t.size,
            pic.things.contains(&ti),
            on_pic(p, [x, y, z]),
            on_pic(p, [x, y + t.height(), z]),
            car_on_screen(p).mid,
            pic.car_px,
            p.car.yaw,
            p.cam.yaw
        );
    }
}

// ---- Wrong order over the review drives -------------------------------
//
// A copy of the depth reference in tests/draw.rs (which checks a sweep
// along the road only), run over every review drive: the car seen from
// the side, the front and above, things close by, the land's edge.

type Tri = [[f64; 3]; 3];

#[derive(Default)]
struct Record {
    tris: Vec<Tri>,
    owner: Vec<usize>,
    things: usize,
    labels: Vec<String>,
}

fn poly_label(c: u8) -> String {
    use drive3d::draw::col;
    if (col::LAND..col::SHADOW).contains(&c) {
        let g = (c - col::LAND) / 12;
        ["grass", "verge", "water", "road", "ford-road", "low-grass"][usize::from(g)].to_string()
    } else if (col::SHADOW..col::SKY).contains(&c) {
        "shadow".to_string()
    } else if c == col::DUST || c == col::SPRAY {
        "dust".to_string()
    } else {
        format!("colour{c}")
    }
}

fn model_label(m: &game_3d::Model) -> String {
    use drive3d::models;
    let names: [(&game_3d::Model, &str); 10] = [
        (&models::CAR, "car"),
        (&models::CAR_PLAIN, "car"),
        (&models::BANNER_PLAIN, "banner"),
        (&models::TREE4, "tree"),
        (&models::TREE6, "tree"),
        (&models::TREE8, "tree"),
        (&models::ROCK, "rock"),
        (&models::HUT, "hut"),
        (&models::POST, "post"),
        (&models::BANNER, "banner"),
    ];
    for (k, n) in names {
        if core::ptr::eq(k, m) {
            return n.to_string();
        }
    }
    "model".to_string()
}

impl Rec for Record {
    fn poly(&mut self, pts: &[[i32; 3]], colour: u8) {
        self.labels.push(poly_label(colour));
        let f = |p: [i32; 3]| [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])];
        for k in 1..pts.len() - 1 {
            self.tris.push([f(pts[0]), f(pts[k]), f(pts[k + 1])]);
            self.owner.push(self.things);
        }
        self.things += 1;
    }

    fn object(&mut self, o: &Object<'_>, cam: &Camera) {
        let m = o.model;
        self.labels.push(model_label(m));
        let rot = |r: &game_3d::Mat3, v: [f64; 3]| {
            let mut out = [0.0; 3];
            for (i, o) in out.iter_mut().enumerate() {
                *o = (f64::from(r.0[i][0]) * v[0]
                    + f64::from(r.0[i][1]) * v[1]
                    + f64::from(r.0[i][2]) * v[2])
                    / 16_384.0;
            }
            out
        };
        let s = f64::from(o.scale) / 256.0;
        let to_cam = |v: [i16; 3]| {
            let v = [
                f64::from(v[0]) * s,
                f64::from(v[1]) * s,
                f64::from(v[2]) * s,
            ];
            let w = rot(&o.rot, v);
            let rel = [
                w[0] + f64::from(o.pos[0] - cam.pos[0]),
                w[1] + f64::from(o.pos[1] - cam.pos[1]),
                w[2] + f64::from(o.pos[2] - cam.pos[2]),
            ];
            rot(&cam.rot, rel)
        };
        let mut faces: Vec<(f64, usize, Tri)> = Vec::new();
        for (i, face) in m.faces.iter().enumerate() {
            let t = face.v.map(|k| to_cam(m.verts[usize::from(k)]));
            let n = m.normals[i];
            let n = rot(
                &cam.rot,
                rot(&o.rot, [f64::from(n[0]), f64::from(n[1]), f64::from(n[2])]),
            );
            if n[0] * t[0][0] + n[1] * t[0][1] + n[2] * t[0][2] >= 0.0 {
                continue;
            }
            faces.push(((t[0][2] + t[1][2] + t[2][2]) / 3.0, i, t));
        }
        faces.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
        for (_, _, t) in faces {
            self.tris.push(t);
            self.owner.push(self.things);
        }
        self.things += 1;
    }
}

const NEAR: f64 = 64.0;

fn clip_near(t: &Tri) -> Vec<[f64; 3]> {
    let mut out = Vec::new();
    for k in 0..3 {
        let (a, b) = (t[k], t[(k + 1) % 3]);
        let (ia, ib) = (a[2] >= NEAR, b[2] >= NEAR);
        if ia {
            out.push(a);
        }
        if ia != ib {
            let s = (NEAR - a[2]) / (b[2] - a[2]);
            out.push([a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s, NEAR]);
        }
    }
    out
}

/// Wrong-order pixels by (drawn last, nearest), most first.
fn wrong_pairs(r: &Record) -> (u32, Vec<(u32, String, String)>) {
    let mut last = vec![f64::INFINITY; W * H];
    let mut nearest = vec![f64::INFINITY; W * H];
    let mut last_of = vec![0usize; W * H];
    let mut nearest_of = vec![0usize; W * H];
    for (ti, t) in r.tris.iter().enumerate() {
        let poly = clip_near(t);
        if poly.len() < 3 {
            continue;
        }
        let u = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
        let v = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let nd = n[0] * t[0][0] + n[1] * t[0][1] + n[2] * t[0][2];
        let sp: Vec<[f64; 2]> = poly
            .iter()
            .map(|p| [80.0 + p[0] * 100.0 / p[2], 60.0 - p[1] * 100.0 / p[2]])
            .collect();
        for k in 1..sp.len() - 1 {
            let tri = [sp[0], sp[k], sp[k + 1]];
            let x0 = tri
                .iter()
                .map(|p| p[0])
                .fold(f64::INFINITY, f64::min)
                .floor()
                .max(0.0) as usize;
            let x1 = tri
                .iter()
                .map(|p| p[0])
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil()
                .min(W as f64) as usize;
            let y0 = tri
                .iter()
                .map(|p| p[1])
                .fold(f64::INFINITY, f64::min)
                .floor()
                .max(0.0) as usize;
            let y1 = tri
                .iter()
                .map(|p| p[1])
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil()
                .min(H as f64) as usize;
            let area = (tri[1][0] - tri[0][0]) * (tri[2][1] - tri[0][1])
                - (tri[1][1] - tri[0][1]) * (tri[2][0] - tri[0][0]);
            if area.abs() < 1e-12 {
                continue;
            }
            for y in y0..y1 {
                for x in x0..x1 {
                    let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                    let e = |a: [f64; 2], b: [f64; 2]| {
                        ((b[0] - a[0]) * (py - a[1]) - (b[1] - a[1]) * (px - a[0])) * area.signum()
                    };
                    if e(tri[0], tri[1]) < 0.0 || e(tri[1], tri[2]) < 0.0 || e(tri[2], tri[0]) < 0.0
                    {
                        continue;
                    }
                    let d = [(px - 80.0) / 100.0, -(py - 60.0) / 100.0, 1.0];
                    let den = n[0] * d[0] + n[1] * d[1] + n[2] * d[2];
                    let z = if den.abs() < 1e-9 {
                        poly.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min)
                    } else {
                        nd / den
                    };
                    let i = y * W + x;
                    last[i] = z;
                    last_of[i] = r.owner[ti];
                    if z < nearest[i] {
                        nearest[i] = z;
                        nearest_of[i] = r.owner[ti];
                    }
                }
            }
        }
    }
    let mut map = std::collections::BTreeMap::new();
    let mut wrong = 0;
    for i in 0..W * H {
        if last[i].is_finite() && last[i] > nearest[i] * 1.01 + 12.8 {
            wrong += 1;
            *map.entry((
                r.labels[last_of[i]].clone(),
                r.labels[nearest_of[i]].clone(),
            ))
            .or_insert(0u32) += 1;
        }
    }
    let mut v: Vec<(u32, String, String)> = map.into_iter().map(|((a, b), n)| (n, a, b)).collect();
    v.sort_by_key(|a| std::cmp::Reverse(a.0));
    if std::env::var("DRIVE3D_REVIEW_WHERE").is_ok() && wrong > 0 {
        let (mut x0, mut y0, mut x1, mut y1) = (W, H, 0, 0);
        for i in 0..W * H {
            if last[i].is_finite() && last[i] > nearest[i] * 1.01 + 12.8 {
                let (x, y) = (i % W, i / W);
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
        println!("  wrong pixels in x {x0}..={x1}, y {y0}..={y1}");
    }
    (wrong, v)
}

#[test]
fn review_order_where() {
    if std::env::var("DRIVE3D_REVIEW_WHERE").is_err() {
        return;
    }
    let all = drives();
    for (name, frame, lv) in [
        ("edge", 1319u32, Detail::Mid),
        ("edge", 1259, Detail::Mid),
        ("still-reverse", 266, Detail::High),
        ("fast", 551, Detail::Mid),
        ("slides", 185, Detail::Low),
    ] {
        let d = all.iter().find(|d| d.name == name).unwrap();
        let (_, p, _) = d.states.iter().find(|s| s.0 == frame).unwrap();
        let mut q = *p;
        q.detail = lv;
        let mut paint = Box::new(Painter::new());
        let mut f = Box::new(Frame([0; W * H]));
        let look = Look::new(&q, 1);
        let mut rec = Record::default();
        draw::picture(
            &mut f,
            &mut paint,
            &look,
            None,
            Opts {
                line: false,
                plain: false,
            },
            &mut rec,
        );
        println!(
            "{name} frame {frame} {}: car on picture {:?}",
            lv.name(),
            car_on_screen(&q).mid
        );
        let (w, pairs) = wrong_pairs(&rec);
        println!("  {w} {:?}", &pairs[..pairs.len().min(3)]);
    }
}

#[test]
fn review_order() {
    let Ok(dir_out) = std::env::var("DRIVE3D_REVIEW") else {
        return;
    };
    let stride: usize = std::env::var("DRIVE3D_REVIEW_STRIDE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    let mut rep = String::new();
    writeln!(rep, "Wrong-order pixels (the builder's depth reference, copied), every {stride}th step of each drive").unwrap();
    for d in &drives() {
        for lv in [Detail::Low, Detail::Mid, Detail::High] {
            let mut worst = (0u32, 0u32, Vec::new());
            let mut over8 = 0u32;
            let mut n = 0u32;
            let mut pairs_all = std::collections::BTreeMap::new();
            for (frame, p, _) in d.states.iter().step_by(stride) {
                let mut q = *p;
                q.detail = lv;
                let mut paint = Box::new(Painter::new());
                let mut f = Box::new(Frame([0; W * H]));
                let look = Look::new(&q, 1);
                let mut rec = Record::default();
                draw::picture(
                    &mut f,
                    &mut paint,
                    &look,
                    None,
                    Opts {
                        line: false,
                        plain: false,
                    },
                    &mut rec,
                );
                let (w, pairs) = wrong_pairs(&rec);
                n += 1;
                if w > 8 {
                    over8 += 1;
                }
                for (c, a, b) in &pairs {
                    *pairs_all.entry((a.clone(), b.clone())).or_insert(0u32) += c;
                }
                if w > worst.0 {
                    worst = (w, *frame, pairs);
                }
            }
            let mut pa: Vec<_> = pairs_all.into_iter().map(|(k, v)| (v, k)).collect();
            pa.sort_by_key(|a| std::cmp::Reverse(a.0));
            pa.truncate(4);
            writeln!(
                rep,
                "{:13} {:4}: pictures {n}, over 8: {over8}, worst {} at frame {} ({:?}); all pictures, most: {:?}",
                d.name,
                lv.name(),
                worst.0,
                worst.1,
                &worst.2[..worst.2.len().min(3)],
                pa
            )
            .unwrap();
        }
    }
    print!("{rep}");
    std::fs::write(std::path::Path::new(&dir_out).join("order.txt"), rep).unwrap();
}

#[test]
fn review_camera_clear() {
    if std::env::var("DRIVE3D_REVIEW").is_err() {
        return;
    }
    for d in &drives() {
        let mut low = (i32::MAX, 0u32);
        let mut hidden = Vec::new();
        for (frame, p, _) in &d.states {
            let e = p.cam.pos;
            let gap = e[1] - Stretch.at(e[0], e[2]).height;
            if gap < low.0 {
                low = (gap, *frame);
            }
            let h = common::roof_hidden_by(p, &Stretch, 16);
            if h > 0 {
                hidden.push((*frame, h));
            }
        }
        println!(
            "camera {}: lowest over the land {:.2} m (frame {}); roof hidden by land in {} steps, worst {:?}",
            d.name,
            f64::from(low.0) / f64::from(M),
            low.1,
            hidden.len(),
            hidden.iter().max_by_key(|h| h.1)
        );
    }
}
