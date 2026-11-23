//! The extracted timing rules and the replay model.

use send_plan::model::{
    Area, Baseline, BoardTimes, Ctx, LoopTimes, Planner, Rng, Sim, Slot, parse_trace, replay,
    trace_line,
};
use send_plan::{GUARD_LINES, Job, Panel, crossed, plan};

const PANEL: Panel = Panel { w: 320, h: 240 };
const LINES: f32 = 343.41;
const LINE_US: f32 = 49.571;

fn job(x: u16, y: u16, w: u16, h: u16) -> Job {
    Job::new((x, y, w, h), PANEL, |x| PANEL.gate(x), 1.0 / LINE_US, 80)
}

#[test]
fn a_job_is_widened_to_whole_picture_pixels() {
    let j = job(3, 5, 10, 3);
    assert_eq!((j.x0, j.y0, j.w, j.h), (2, 4, 12, 4));
    let j = job(318, 238, 5, 5);
    assert_eq!((j.x0, j.y0, j.w, j.h), (318, 238, 2, 2));
    assert_eq!((j.g0, j.g1), (318.0, 319.0));
}

#[test]
fn the_wire_takes_h_times_a_fifth_of_a_microsecond_per_column() {
    // 2 bytes a pixel, 8 bits, 80 MHz, 1.5 % slower: 0.203 us per row.
    let j = job(0, 0, 320, 240);
    let us = j.per_gate * LINE_US;
    assert!((us - 240.0 * 0.2 * 1.015).abs() < 0.01, "{us}");
    let whole = j.lines * LINE_US;
    assert!(
        (whole - (35.0 + 320.0 * 48.72 + 10.0)).abs() < 1.0,
        "{whole}"
    );
}

#[test]
fn a_full_screen_window_starts_just_after_the_refresh_left_column_0() {
    let j = job(0, 0, 320, 240);
    let (lo, hi) = j.window(LINES);
    // The writer (0.983 line times a column at 80 MHz) is a little
    // faster than the refresh: it starts behind it by the guard plus
    // what it gains over 319 columns (319 x 0.017 = 5.5 lines).
    let gain = 319.0 * (1.0 - j.per_gate);
    assert!((lo - (1.0 + GUARD_LINES + gain)).abs() < 1e-3, "{lo}");
    assert!(lo > 14.0 && lo < 15.0, "{lo}");
    assert!(hi - lo > 300.0, "{lo} {hi}");
}

#[test]
fn a_wide_low_band_may_only_start_late_in_the_refresh() {
    // 320 x 12: written about 20 times faster than the refresh moves, so
    // it must wait until the refresh has nearly left the screen.
    let j = job(0, 100, 320, 12);
    let (lo, hi) = j.window(LINES);
    assert!(lo > 300.0 && lo < 320.0, "{lo}");
    assert!(hi > lo && hi < LINES, "{hi}");
    assert!(hi - lo < 40.0, "span {}", hi - lo);
}

#[test]
fn plan_finds_one_pass_for_side_by_side_areas() {
    let jobs = [job(0, 0, 40, 40), job(100, 0, 40, 40), job(200, 0, 40, 40)];
    let k = plan(&jobs, 10.0, LINES).expect("a pass");
    assert!(k == 0.0 || k == 1.0);
}

#[test]
fn plan_finds_no_pass_for_stacked_wide_bands() {
    let jobs: Vec<Job> = (0..6).map(|i| job(0, i * 40, 320, 12)).collect();
    assert_eq!(plan(&jobs, 0.0, LINES), None);
    // One alone always fits.
    assert!(plan(&jobs[..1], 0.0, LINES).is_some());
}

#[test]
fn crossed_sees_a_writer_that_overtakes_the_refresh() {
    let j = job(0, 0, 320, 12);
    let (lo, _) = j.window(LINES);
    let ve = |vs: f32| vs + f32::from(j.w) * j.per_gate;
    // In its window: no meeting.
    let vs = lo + 1.0;
    assert!(!crossed(vs, ve(vs), j.g0, j.g1, LINES));
    // Far too early: the writer runs into the refresh.
    let vs = 50.0;
    assert!(crossed(vs, ve(vs), j.g0, j.g1, LINES));
}

fn frame30(draw_us: f64) -> LoopTimes {
    LoopTimes {
        period_us: 33_333.0,
        draw_us,
        loop_us: 100.0,
    }
}

#[test]
fn an_idle_run_keeps_the_rate() {
    let mut sim = Sim::new(
        BoardTimes::default(),
        frame30(3_000.0),
        Baseline::default(),
        0.0,
    );
    for _ in 0..31 {
        sim.step(&[]);
    }
    assert!((sim.c.fps() - 30.0).abs() < 0.01, "{}", sim.c.fps());
    assert_eq!(sim.c.presents, 0);
    assert_eq!(sim.c.late_steps, 0);
}

#[test]
fn one_small_area_waits_at_most_two_refreshes_and_is_never_late() {
    let mut rng = Rng(7);
    for _ in 0..200 {
        let phase = rng.next_f64() * f64::from(LINES);
        let mut sim = Sim::new(
            BoardTimes::default(),
            frame30(3_000.0),
            Baseline::default(),
            phase,
        );
        for _ in 0..20 {
            sim.step(&[Area {
                x: 100,
                y: 100,
                w: 20,
                h: 20,
            }]);
        }
        let c = &sim.c;
        assert_eq!(c.late, 0);
        assert_eq!(c.crossed, 0);
        assert_eq!(c.crossed_true, 0);
        assert!(
            c.present_max < 2.0 * 17_100.0 + 2_000.0,
            "{}",
            c.present_max
        );
    }
}

#[test]
fn the_time_split_adds_up() {
    let areas: Vec<Area> = (0..6)
        .map(|i| Area {
            x: 0,
            y: i * 40,
            w: 320,
            h: 12,
        })
        .collect();
    let mut sim = Sim::new(
        BoardTimes::default(),
        frame30(3_000.0),
        Baseline::default(),
        123.0,
    );
    for _ in 0..50 {
        sim.step(&areas);
    }
    let c = &sim.c;
    let parts = c.anchor_us + c.cpu_us + c.spin_us + c.overhead_us + c.wire_us;
    assert!(
        (parts - c.present_sum).abs() < 1.0,
        "{parts} {}",
        c.present_sum
    );
    assert_eq!(sim.planner.no_plan, 50);
    assert_eq!(c.windows, 300);
}

#[test]
fn a_replay_repeats_exactly() {
    let steps: Vec<_> = (0..40u16)
        .map(|i| send_plan::model::Step {
            t_us: u64::from(i) * 33_333,
            areas: vec![
                Area {
                    x: 0,
                    y: i * 2,
                    w: 320,
                    h: 10,
                },
                Area {
                    x: 40,
                    y: 150,
                    w: 200,
                    h: 30,
                },
            ],
        })
        .collect();
    let a = replay(
        &steps,
        BoardTimes::default(),
        frame30(3_000.0),
        Baseline::default(),
        77.0,
    );
    let b = replay(
        &steps,
        BoardTimes::default(),
        frame30(3_000.0),
        Baseline::default(),
        77.0,
    );
    assert_eq!(a.c.present_sum, b.c.present_sum);
    assert_eq!(a.c.late, b.c.late);
    assert_eq!(a.c.windows, b.c.windows);
}

/// A planner that knows every start time at once: all windows in one go,
/// untimed. It shows that a planner plugs in and that untimed windows
/// are counted.
struct AllAtOnce(Vec<Area>);

impl Planner for AllAtOnce {
    fn name(&self) -> String {
        "all-at-once".into()
    }
    fn begin(&mut self, areas: &[Area], _v: Option<f32>, ctx: &Ctx, jobs: &mut Vec<Job>) {
        self.0 = areas.to_vec();
        jobs.clear();
        jobs.extend(areas.iter().map(|&a| ctx.job(a)));
    }
    fn slot(&mut self, _i: usize, _jobs: &[Job], _v: Option<f32>, _ctx: &Ctx) -> Slot {
        Slot::default()
    }
}

#[test]
fn another_planner_plugs_in() {
    let mut sim = Sim::new(
        BoardTimes::default(),
        frame30(3_000.0),
        AllAtOnce(vec![]),
        0.0,
    );
    let areas = [
        Area {
            x: 0,
            y: 0,
            w: 320,
            h: 12,
        },
        Area {
            x: 0,
            y: 100,
            w: 320,
            h: 12,
        },
    ];
    sim.step(&areas);
    assert_eq!(sim.c.untimed, 2);
    assert_eq!(sim.c.spin_us, 0.0);
    assert_eq!(sim.planner.0.len(), 2);
}

#[test]
fn a_trace_reads_back() {
    let areas = [
        Area {
            x: 1,
            y: 2,
            w: 3,
            h: 4,
        },
        Area {
            x: 10,
            y: 20,
            w: 30,
            h: 40,
        },
    ];
    let text = format!(
        "# send-plan trace 1\n# game LANTERN\n{}\n{}\n",
        trace_line(0, &areas),
        trace_line(33_333, &[])
    );
    let tr = parse_trace(&text).unwrap();
    assert_eq!(tr.meta("game"), Some("LANTERN"));
    assert_eq!(tr.steps.len(), 2);
    assert_eq!(tr.steps[0].areas, areas);
    assert!(tr.steps[1].areas.is_empty());
    assert!(parse_trace("S 0 2 1,2,3,4").is_err());
}
