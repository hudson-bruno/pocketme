//! The new choice: windows in one pass, or one box, whichever is done
//! sooner by the code's own model.

use send_plan::choose::{
    Area, Rules, Shape, bounding_box, choose, choose_with, column_groups, end_in_pass,
    fallback_end, plan_end,
};
use send_plan::{Job, MAX_JOBS, Panel, plan};

const PANEL: Panel = Panel { w: 320, h: 240 };
const LINES: f32 = 343.41;
const LINE_US: f32 = 49.571;

fn job(x: u16, y: u16, w: u16, h: u16) -> Job {
    Job::new((x, y, w, h), PANEL, |x| PANEL.gate(x), 1.0 / LINE_US, 80)
}

fn make(a: Area) -> Job {
    job(a.0, a.1, a.2, a.3)
}

/// Stacked wide bands, as a turning 3D view gives (sorted by column:
/// they all start at 0).
fn bands(n: u16) -> Vec<Job> {
    (0..n).map(|i| job(0, 20 + i * 20, 320, 8)).collect()
}

#[test]
fn plan_end_gives_the_same_pass_as_plan() {
    let sets = [
        bands(6),
        bands(1),
        vec![job(0, 0, 40, 40), job(100, 0, 40, 40), job(200, 0, 40, 40)],
        vec![job(0, 0, 320, 240)],
    ];
    for jobs in &sets {
        for i in 0..200 {
            let v = i as f32 * 7.3;
            let a = plan(jobs, v, LINES);
            let b = plan_end(jobs, v, LINES).map(|(k, _)| k);
            assert_eq!(a, b, "v={v}");
            if let Some((k, end)) = plan_end(jobs, v, LINES) {
                assert_eq!(end_in_pass(jobs, k, v, LINES), Some(end));
                assert!(end > v);
            }
        }
    }
}

#[test]
fn the_box_holds_every_window() {
    let jobs = [job(10, 50, 30, 10), job(100, 20, 41, 9), job(5, 200, 3, 3)];
    let (x, y, w, h) = bounding_box(&jobs);
    for j in &jobs {
        assert!(j.x0 >= x && j.x0 + j.w <= x + w);
        assert!(j.y0 >= y && j.y0 + j.h <= y + h);
    }
    assert_eq!((x, y), (4, 20));
    assert_eq!(bounding_box(&[]), (0, 0, 0, 0));
}

#[test]
fn column_groups_join_only_windows_that_share_columns() {
    let jobs = [
        job(0, 0, 40, 10),
        job(20, 100, 40, 10),
        job(100, 50, 20, 20),
        job(200, 0, 10, 10),
        job(206, 30, 10, 10),
    ];
    let mut out = [(0, 0, 0, 0); MAX_JOBS];
    let n = column_groups(&jobs, 0, &mut out);
    assert_eq!(n, 3);
    assert_eq!(out[0], (0, 0, 60, 110));
    assert_eq!(out[1], (100, 50, 20, 20));
    assert_eq!(out[2], (200, 0, 16, 40));
    // A gap of 40 joins the first two groups too.
    assert_eq!(column_groups(&jobs, 40, &mut out), 2);
    // The output is never overrun.
    assert_eq!(column_groups(&jobs, 0, &mut out[..2]), 2);
}

#[test]
fn stacked_wide_bands_are_sent_as_one_box_in_one_pass() {
    let jobs = bands(8);
    let mut out = [Job::default(); MAX_JOBS];
    for i in 0..100 {
        let v = i as f32 * 3.43;
        assert_eq!(plan(&jobs, v, LINES), None, "today's rule finds no pass");
        let c = choose(&jobs, make, v, LINES, &mut out);
        assert_eq!(c.shape, Shape::Box, "v={v}");
        assert_eq!(c.n, 1);
        let k = c.pass.expect("a pass");
        assert_eq!(plan(&out[..1], v, LINES), Some(k));
        assert!(c.end < fallback_end(&jobs, v, LINES));
        assert_eq!(
            (out[0].x0, out[0].y0, out[0].w, out[0].h),
            (0, 20, 320, 148)
        );
    }
}

#[test]
fn side_by_side_areas_stay_windows() {
    // Small areas across the screen fit one pass; the box around them
    // is nearly the whole screen and ends no sooner.
    let jobs = [
        job(0, 100, 20, 20),
        job(150, 10, 20, 20),
        job(300, 200, 20, 20),
    ];
    let mut out = [Job::default(); MAX_JOBS];
    for i in 0..100 {
        let v = i as f32 * 3.43;
        let c = choose(&jobs, make, v, LINES, &mut out);
        assert_eq!(c.shape, Shape::Windows, "v={v}");
        assert_eq!(c.n, 3);
        assert_eq!(c.pass, plan(&jobs, v, LINES));
    }
}

#[test]
fn one_window_is_never_changed() {
    let jobs = [job(0, 100, 320, 8)];
    let mut out = [Job::default(); MAX_JOBS];
    let c = choose(&jobs, make, 10.0, LINES, &mut out);
    assert_eq!(c.shape, Shape::Windows);
    assert_eq!(c.n, 1);
    assert_eq!(out[0], Job::default());
}

#[test]
fn the_choice_is_never_later_than_today() {
    // Random area lists: the chosen way is never done later than
    // today's windows, by the same model.
    let mut seed = 0x1234_5678u32;
    let mut rnd = |m: u16| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed % u32::from(m)) as u16
    };
    let mut out = [Job::default(); MAX_JOBS];
    for _ in 0..2_000 {
        let n = 1 + usize::from(rnd(10));
        let mut jobs: Vec<Job> = (0..n)
            .map(|_| {
                let (x, y) = (rnd(300), rnd(220));
                job(x, y, 2 + rnd(320 - x - 1), 2 + rnd(240 - y - 1))
            })
            .collect();
        jobs.sort_by_key(|j| j.x0);
        let v = f32::from(rnd(3000)) / 3.0;
        let today = match plan_end(&jobs, v, LINES) {
            Some((_, end)) => end,
            None => fallback_end(&jobs, v, LINES),
        };
        for rules in [Rules::FIRMWARE, Rules::NEW] {
            let c = choose_with(&jobs, make, v, LINES, &mut out, rules);
            assert!(c.end <= today, "{} > {today}", c.end);
            if c.shape != Shape::Windows {
                assert!(c.pass.is_some());
            }
        }
    }
}

#[test]
fn the_firmware_rule_leaves_out_the_groups() {
    assert_eq!(
        Rules::FIRMWARE,
        Rules {
            groups: false,
            ..Rules::NEW
        }
    );
    let r = Rules::FIRMWARE;
    assert_eq!(
        (r.groups, r.boxed, r.only_without_pass),
        (false, true, false)
    );
    assert_eq!(r.window_cost, 0.0);
}
