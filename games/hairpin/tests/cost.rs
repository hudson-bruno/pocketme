//! What each picture costs the screen, for every setting: the game
//! pixels the console would send (its own rule, `console_video::diff`,
//! at most 16 areas a picture), average and worst, while the autopilot
//! races whole courses.
//!
//! The rule of thumb from the board (LANTERN, measured there): about
//! 6,000 sent game pixels a picture hold 30 pictures a second on one
//! core; about 11,000 gave 26.6. So the lightest setting (LIGHT, CHASE,
//! 30; the default until 2026-11-23, now HEAVY, CHASE, 60) must stay at
//! or below 6,000 on average and 8,000 at worst.
//!
//! The frames-a-second column is an ESTIMATE from those board numbers,
//! not a measurement (see `step_us`). Run with `--nocapture` to see
//! the table; it is also written to `.local/next/racing/cost.txt`.

mod common;

use common::{Driver, evidence, plan, save_frame};
use console_video::{Areas, Frame, H, W, diff};
use game_kit::button;
use hairpin::app::Scene;
use hairpin::render::{Cam, Detail};
use hairpin::track::COURSES;

const FULL: u32 = (W * H) as u32;

/// LIGHT, CHASE, 30's limits (game pixels sent a picture).
const LIGHT_MEAN: u64 = 6_000;
const LIGHT_WORST: u32 = 8_000;

#[derive(Default, Clone)]
struct Sum {
    pictures: u64,
    sent: u64,
    worst: u32,
    worst_areas: usize,
    /// Sent pixels per picture, for the 95th percentile.
    all: Vec<u32>,
    /// Pixels the renderer wrote (ground rows across the view, plus
    /// sprite pixels): total and worst, for the drawing time.
    drawn: u64,
    drawn_worst: u32,
}

impl Sum {
    fn mean(&self) -> u64 {
        self.sent / self.pictures.max(1)
    }

    fn p95(&self) -> u32 {
        let mut v = self.all.clone();
        v.sort_unstable();
        v.get(v.len() * 95 / 100).copied().unwrap_or(0)
    }

    fn add(&mut self, o: &Sum) {
        self.pictures += o.pictures;
        self.sent += o.sent;
        self.worst = self.worst.max(o.worst);
        self.worst_areas = self.worst_areas.max(o.worst_areas);
        self.all.extend_from_slice(&o.all);
        self.drawn += o.drawn;
        self.drawn_worst = self.drawn_worst.max(o.drawn_worst);
    }
}

/// ESTIMATE of one console step on the board, in microseconds, for a
/// picture that sends `px` game pixels. A model fitted by hand to the
/// LANTERN board measurements (one core), not measured for this game:
///
/// * up to about 8,000 game pixels the send fits one panel refresh
///   window: the step is about 1.5 ms (drawing and the rest; LANTERN's
///   ray casting took 3 to 4 ms, this road is simpler) plus 2 us a game
///   pixel (LANTERN LIGHT: about 6,000 game pixels, about 12 ms);
/// * above that the send waits for a second window: about 37.6 ms in
///   all (LANTERN NORMAL, 11,000 game pixels: 26.6 a second; HEAVY,
///   15,000: 26.1).
fn step_us(px: u32) -> u64 {
    if px <= 8_000 {
        1_500 + u64::from(px) * 2
    } else {
        37_600
    }
}

/// ESTIMATE of pictures a second: every picture takes its step, but at
/// least the period of the chosen rate (33.3 ms or 16.7 ms).
fn fps_estimate(s: &Sum, fast: bool) -> String {
    let period: u64 = if fast { 16_667 } else { 33_333 };
    let total: u64 = s.all.iter().map(|&px| step_us(px).max(period)).sum();
    let tenths = s.all.len() as u64 * 10_000_000 / total.max(1);
    format!("{}.{}", tenths / 10, tenths % 10)
}

/// Races the whole of course `c` with this setting; counts every racing
/// picture (the countdown too).
fn race(c: u8, detail: Detail, cam: Cam, fast: bool) -> Sum {
    let mut d = Driver::new();
    d.app.opts.detail = detail;
    d.app.opts.cam = cam;
    d.app.opts.fast = fast;
    if fast {
        d.call_us = 16_667;
    }
    d.race_course(c);
    let mut shown = Box::new(Frame(d.frame.0));
    let mut sum = Sum::default();
    let mut worst_pic = None;
    for _ in 0..60 * 400 {
        if d.app.scene != Scene::Race {
            break;
        }
        let mut k = plan(&d.app);
        k.pressed = k.held & !d.held;
        d.held = k.held;
        d.call_keys(k);
        if d.app.scene != Scene::Race {
            // The finish card opens: a one-off, counted with the other
            // screens below, not as racing.
            break;
        }
        let mut a = Areas::new();
        diff(&shown, &d.frame, &mut a);
        let before = Box::new(Frame(shown.0));
        shown.0.copy_from_slice(&d.frame.0);
        assert!(a.len() <= 16);
        sum.pictures += 1;
        sum.sent += u64::from(a.pixels());
        if a.pixels() > sum.worst {
            worst_pic = Some((Box::new(Frame(before.0)), Box::new(Frame(d.frame.0)), a));
        }
        sum.worst = sum.worst.max(a.pixels());
        sum.worst_areas = sum.worst_areas.max(a.len());
        sum.all.push(a.pixels());
        let v = detail.view();
        let c = d.app.cost;
        let drawn = u32::from(c.rows) * (v.x1 - v.x0) as u32 + c.sprite_px;
        sum.drawn += u64::from(drawn);
        sum.drawn_worst = sum.drawn_worst.max(drawn);
    }
    assert_eq!(d.app.scene, Scene::Finish, "course {} not finished", c + 1);
    // The worst picture of LIGHT, CHASE, 30, with its areas marked,
    // for a person to see what costs most.
    if detail == Detail::Light
        && cam == Cam::Chase
        && !fast
        && let Some((before, after, a)) = worst_pic
    {
        let dir = evidence("cost");
        save_frame(
            &dir.join(format!("worst-course{}-before.png", c + 1)),
            &before,
            3,
        );
        let mut marked = Box::new(Frame(after.0));
        for r in a.as_slice() {
            marked.outline(r.x as i16, r.y as i16, r.w, r.h, 15);
        }
        save_frame(
            &dir.join(format!("worst-course{}-areas.png", c + 1)),
            &marked,
            3,
        );
    }
    sum
}

#[test]
fn every_setting_costs_what_it_says() {
    let mut out = String::new();
    out.push_str(&format!(
        "One picture is {W} x {H} = {FULL} game pixels (one game pixel is 4 panel pixels).\n\
         sent: MEASURED on the PC with console_video::diff (the console's own rule), \
         while the autopilot races every course to the finish.\n\
         drawn: COUNTED pixels the game wrote for the 3D view (ground rows across the view plus sprite pixels).\n\
         fps: ESTIMATE for the board from the LANTERN board measurements (tests/cost.rs).\n\n"
    ));
    let mut rows = Vec::new();
    for fast in [false, true] {
        for cam in Cam::ALL {
            for detail in Detail::ALL {
                let mut all = Sum::default();
                let mut per = Vec::new();
                for c in 0..COURSES.len() as u8 {
                    let s = race(c, detail, cam, fast);
                    per.push((s.mean(), s.worst));
                    all.add(&s);
                }
                let line = format!(
                    "{:<6} {:<5} rate {}: pictures {:5} | sent px mean {:5} p95 {:5} worst {:5} areas worst {:2} \
                     | drawn px mean {:5} worst {:5} | per course mean/worst {:?} | ESTIMATE fps {}",
                    detail.name(),
                    cam.name(),
                    if fast { 60 } else { 30 },
                    all.pictures,
                    all.mean(),
                    all.p95(),
                    all.worst,
                    all.worst_areas,
                    all.drawn / all.pictures.max(1),
                    all.drawn_worst,
                    per,
                    fps_estimate(&all, fast),
                );
                out.push_str(&line);
                out.push('\n');
                rows.push((detail, cam, fast, all));
            }
        }
    }

    // The other screens.
    let mut d = Driver::new();
    let mut shown = Box::new(Frame(d.frame.0));
    let mut step = |d: &mut Driver, what: &str, out: &mut String| {
        let mut a = Areas::new();
        diff(&shown, &d.frame, &mut a);
        shown.0.copy_from_slice(&d.frame.0);
        out.push_str(&format!(
            "{what:<26} sent px {:5} areas {:2}\n",
            a.pixels(),
            a.len()
        ));
        a.pixels()
    };
    out.push('\n');
    d.hold(0);
    let still = step(&mut d, "title, no input", &mut out);
    assert_eq!(still, 0);
    d.hold(button::DOWN);
    step(&mut d, "title menu step", &mut out);
    d.hold(0);
    d.app.menu = 0;
    d.hold(button::A);
    step(&mut d, "title -> race", &mut out);
    d.hold(0);
    step(&mut d, "countdown", &mut out);
    d.hold(button::START);
    step(&mut d, "pause opens", &mut out);
    d.hold(0);
    let still = step(&mut d, "pause, no input", &mut out);
    assert_eq!(still, 0);

    eprintln!("{out}");
    std::fs::write(evidence("").join("cost.txt"), &out).unwrap();

    let get = |detail: Detail, cam: Cam, fast: bool| {
        rows.iter()
            .find(|r| r.0 == detail && r.1 == cam && r.2 == fast)
            .map(|r| r.3.clone())
            .unwrap()
    };
    // LIGHT, CHASE, 30 holds the rule of thumb.
    let light = get(Detail::Light, Cam::Chase, false);
    assert!(light.mean() <= LIGHT_MEAN, "LIGHT mean {}", light.mean());
    assert!(light.worst <= LIGHT_WORST, "LIGHT worst {}", light.worst);
    // More detail sends more.
    for cam in Cam::ALL {
        let m: Vec<u64> = Detail::ALL
            .iter()
            .map(|d| get(*d, cam, false).mean())
            .collect();
        assert!(m[0] < m[1] && m[1] < m[2], "{cam:?}: {m:?}");
    }
}
