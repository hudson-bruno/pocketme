//! Replays recorded console steps through the model of today's send, at
//! many refresh phases, and prints the board's counters for each trace.
//!
//! cargo +1.99.0 run --offline --release --features model \
//!     --manifest-path boards/cyd/send-plan/Cargo.toml --example replay -- \
//!     [--phases N] [--draw-us US] [--read-us US] [--extra-us US] \
//!     [--loop-us US] [--plan-us US] [--window-cpu-us US] [--shapes] TRACE...
//!
//! The rate comes from the trace's `# rate` line (30 or 60), the time
//! besides the present from `# draw_us` unless `--draw-us` is given.

use std::process::ExitCode;

use send_plan::model::{Area, Baseline, BoardTimes, Counters, LoopTimes, Rng, Trace, parse_trace};

struct Opts {
    phases: u32,
    draw_us: Option<f64>,
    loop_us: f64,
    board: BoardTimes,
    shapes: bool,
    files: Vec<String>,
}

fn opts() -> Result<Opts, String> {
    let mut o = Opts {
        phases: 200,
        draw_us: None,
        loop_us: 100.0,
        board: BoardTimes::default(),
        shapes: false,
        files: vec![],
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut num = |name: &str| -> Result<f64, String> {
            args.next()
                .and_then(|s| s.parse().ok())
                .ok_or(format!("{name} needs a number"))
        };
        match a.as_str() {
            "--phases" => o.phases = num("--phases")? as u32,
            "--draw-us" => o.draw_us = Some(num("--draw-us")?),
            "--loop-us" => o.loop_us = num("--loop-us")?,
            "--read-us" => o.board.read_us = num("--read-us")?,
            "--extra-us" => o.board.send_extra_us = num("--extra-us")?,
            "--plan-us" => o.board.plan_us = num("--plan-us")?,
            "--window-cpu-us" => o.board.per_window_cpu_us = num("--window-cpu-us")?,
            "--shapes" => o.shapes = true,
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            _ => o.files.push(a),
        }
    }
    if o.files.is_empty() {
        return Err("no trace files".into());
    }
    Ok(o)
}

/// Sum of counters over phases, and the spread of fps and present avg.
#[derive(Default)]
struct Sum {
    c: Counters,
    no_plan: u32,
    runs: u32,
    fps: Vec<f64>,
    pavg: Vec<f64>,
}

fn add(sum: &mut Sum, c: &Counters, no_plan: u32) {
    let s = &mut sum.c;
    s.frames += c.frames;
    s.presents += c.presents;
    s.windows += c.windows;
    s.untimed += c.untimed;
    s.no_anchor += c.no_anchor;
    s.anchor_miss += c.anchor_miss;
    s.late += c.late;
    s.crossed += c.crossed;
    s.crossed_true += c.crossed_true;
    s.capped += c.capped;
    s.step_sum += c.step_sum;
    s.step_max = s.step_max.max(c.step_max);
    s.present_sum += c.present_sum;
    s.present_max = s.present_max.max(c.present_max);
    s.anchor_us += c.anchor_us;
    s.cpu_us += c.cpu_us;
    s.spin_us += c.spin_us;
    s.overhead_us += c.overhead_us;
    s.wire_us += c.wire_us;
    s.late_steps += c.late_steps;
    s.gap_sum += c.gap_sum;
    s.gaps += c.gaps;
    s.pixels += c.pixels;
    sum.no_plan += no_plan;
    sum.runs += 1;
    sum.fps.push(c.fps());
    sum.pavg.push(c.present_avg_ms());
}

fn spread(v: &mut [f64]) -> (f64, f64, f64) {
    v.sort_by(f64::total_cmp);
    let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    (q(0.05), q(0.5), q(0.95))
}

fn shapes(tr: &Trace) {
    let mut presents = 0u32;
    let mut areas = 0u32;
    let mut over16 = 0u32;
    let mut full_w = 0u32;
    let mut wide = 0u32;
    let mut h_hist = [0u32; 6]; // <=4, <=8, <=16, <=32, <=64, more
    let mut w_hist = [0u32; 5]; // <=40, <=80, <=160, <=300, more
    let mut px = 0u64;
    let mut same_x0 = 0u32;
    let mut n_hist = [0u32; 5]; // 1, 2-4, 5-8, 9-16, >16
    for s in &tr.steps {
        if s.areas.is_empty() {
            continue;
        }
        presents += 1;
        let n = s.areas.len();
        n_hist[match n {
            1 => 0,
            2..=4 => 1,
            5..=8 => 2,
            9..=16 => 3,
            _ => 4,
        }] += 1;
        if n > 16 {
            over16 += 1;
        }
        let mut xs: Vec<u16> = s.areas.iter().map(|a| a.x & !1).collect();
        xs.sort_unstable();
        xs.dedup();
        same_x0 += (n - xs.len()) as u32;
        for a in &s.areas {
            let Area { w, h, .. } = *a;
            areas += 1;
            px += u64::from(w) * u64::from(h);
            if w >= 316 {
                full_w += 1;
            }
            if w >= 4 * h {
                wide += 1;
            }
            h_hist[match h {
                0..=4 => 0,
                5..=8 => 1,
                9..=16 => 2,
                17..=32 => 3,
                33..=64 => 4,
                _ => 5,
            }] += 1;
            w_hist[match w {
                0..=40 => 0,
                41..=80 => 1,
                81..=160 => 2,
                161..=300 => 3,
                _ => 4,
            }] += 1;
        }
    }
    let pc = |x: u32, of: u32| 100.0 * f64::from(x) / f64::from(of.max(1));
    println!(
        "  shapes: {presents} presents, {:.2} areas each, {:.0} panel px each; \
         >16 areas (full screen) {:.1} %",
        f64::from(areas) / f64::from(presents.max(1)),
        px as f64 / f64::from(presents.max(1)),
        pc(over16, presents)
    );
    println!(
        "  areas a present: 1 {:.1} %, 2-4 {:.1} %, 5-8 {:.1} %, 9-16 {:.1} %, >16 {:.1} %",
        pc(n_hist[0], presents),
        pc(n_hist[1], presents),
        pc(n_hist[2], presents),
        pc(n_hist[3], presents),
        pc(n_hist[4], presents)
    );
    println!(
        "  area height (panel px): <=4 {:.1} %, 5-8 {:.1} %, 9-16 {:.1} %, 17-32 {:.1} %, 33-64 {:.1} %, >64 {:.1} %",
        pc(h_hist[0], areas),
        pc(h_hist[1], areas),
        pc(h_hist[2], areas),
        pc(h_hist[3], areas),
        pc(h_hist[4], areas),
        pc(h_hist[5], areas)
    );
    println!(
        "  area width: <=40 {:.1} %, 41-80 {:.1} %, 81-160 {:.1} %, 161-300 {:.1} %, >300 {:.1} %; \
         full width (>=316) {:.1} %, width >= 4 x height {:.1} %, sharing a first column with another {:.1} %",
        pc(w_hist[0], areas),
        pc(w_hist[1], areas),
        pc(w_hist[2], areas),
        pc(w_hist[3], areas),
        pc(w_hist[4], areas),
        pc(full_w, areas),
        pc(wide, areas),
        pc(same_x0, areas)
    );
}

fn main() -> ExitCode {
    let o = match opts() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("replay: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "| trace | fps | step avg/max ms | present avg/max ms | anchor | cpu | wait | lead+tail | wire (ms a present) | no-plan % | windows/present | late | crossed (belief/true) | anchor_miss | capped | fps 5/50/95 % |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for f in &o.files {
        let text = match std::fs::read_to_string(f) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("replay: {f}: {e}");
                return ExitCode::FAILURE;
            }
        };
        let tr = match parse_trace(&text) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("replay: {f}: {e}");
                return ExitCode::FAILURE;
            }
        };
        let rate: f64 = tr.meta("rate").and_then(|s| s.parse().ok()).unwrap_or(30.0);
        let draw = o
            .draw_us
            .or_else(|| tr.meta("draw_us").and_then(|s| s.parse().ok()))
            .unwrap_or(3_000.0);
        let frame = LoopTimes {
            period_us: if rate >= 59.0 { 16_667.0 } else { 33_333.0 },
            draw_us: draw,
            loop_us: o.loop_us,
        };
        let mut sum = Sum::default();
        let mut rng = Rng(0x5eed_1234_abcd_0001);
        let lines = f64::from(o.board.lines);
        for _ in 0..o.phases {
            let phase = rng.next_f64() * lines;
            let sim =
                send_plan::model::replay(&tr.steps, o.board, frame, Baseline::default(), phase);
            add(&mut sum, &sim.c, sim.planner.no_plan);
        }
        let c = &sum.c;
        let (f5, f50, f95) = spread(&mut sum.fps);
        let runs = f64::from(sum.runs.max(1));
        let name = std::path::Path::new(f)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        println!(
            "| {name} | {:.2} | {:.1}/{:.1} | {:.1}/{:.1} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.0} | {:.2} | {:.0} | {:.1}/{:.1} | {:.0} | {:.0} | {f5:.2}/{f50:.2}/{f95:.2} |",
            c.fps(),
            c.step_avg_ms(),
            c.step_max / 1000.0,
            c.present_avg_ms(),
            c.present_max / 1000.0,
            c.anchor_ms(),
            c.cpu_ms(),
            c.spin_ms(),
            c.overhead_ms(),
            c.wire_ms(),
            100.0 * f64::from(sum.no_plan) / f64::from(c.presents.max(1)),
            c.windows_per_present(),
            f64::from(c.late) / runs,
            f64::from(c.crossed) / runs,
            f64::from(c.crossed_true) / runs,
            f64::from(c.anchor_miss) / runs,
            f64::from(c.capped) / runs,
        );
        if o.shapes {
            shapes(&tr);
        }
    }
    ExitCode::SUCCESS
}
