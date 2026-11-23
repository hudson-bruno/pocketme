//! Scores the send rules over LANTERN's recorded area lists (MODEL: every
//! number it prints is an estimate from the code's own timing model).
//!
//! cargo +1.99.0 run --offline --release --features model \
//!     --manifest-path boards/cyd/send-plan/Cargo.toml --example score -- \
//!     [--phases N] [--extra-us US] [--lead-extra-us US] DIR
//!
//! DIR holds `{light,normal,heavy}-{30,60}.txt`: one line per present,
//! `P <tick> x,y,w,h ...` (panel pixels), as recorded from the
//! simulator (the recordings are kept outside the repository). The time
//! each step spends besides the present comes from the board's logs
//! (step sum minus present sum, per frame; L4 runs of 2026-11-17).

use std::process::ExitCode;

use send_plan::choose::Rules;
use send_plan::model::{Area, Baseline, BoardTimes, LoopTimes, NewRule, Planner, Rng, Sim, Step};

/// Time a step spends besides the present, us (MEASURED on the board,
/// L4 logs, kept outside the repository:
/// (step_avg x frames - present_avg x presents) / frames).
fn draw_us(detail: &str, rate: u32) -> f64 {
    match (detail, rate) {
        ("light", 30) => 2_750.0,
        ("light", _) => 2_510.0,
        ("normal", 30) => 3_670.0,
        ("normal", _) => 3_540.0,
        ("heavy", 30) => 4_930.0,
        _ => 4_890.0,
    }
}

fn load(path: &str) -> Result<Vec<Step>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut steps = Vec::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        if it.next() != Some("P") {
            continue;
        }
        let tick: u64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let mut areas = Vec::new();
        for part in it {
            let v: Vec<u16> = part.split(',').filter_map(|s| s.parse().ok()).collect();
            if v.len() != 4 {
                return Err(format!("{path}: bad area {part}"));
            }
            areas.push(Area {
                x: v[0],
                y: v[1],
                w: v[2],
                h: v[3],
            });
        }
        steps.push(Step {
            t_us: tick * 16_667,
            areas,
        });
    }
    Ok(steps)
}

#[derive(Default)]
struct Row {
    runs: u32,
    fps: f64,
    present_sum: f64,
    present_max: f64,
    presents: u64,
    windows: u64,
    late: u64,
    crossed: u64,
    crossed_true: u64,
    pixels: u64,
    no_plan: u64,
    shapes: [u64; 3],
    spin_us: f64,
    wire_us: f64,
}

impl Row {
    fn add(&mut self, sim: &SimAny) {
        let c = sim.c();
        self.runs += 1;
        self.fps += c.fps();
        self.present_sum += c.present_sum;
        self.present_max = self.present_max.max(c.present_max);
        self.presents += u64::from(c.presents);
        self.windows += u64::from(c.windows);
        self.late += u64::from(c.late);
        self.crossed += u64::from(c.crossed);
        self.crossed_true += u64::from(c.crossed_true);
        self.pixels += c.pixels;
        self.spin_us += c.spin_us;
        self.wire_us += c.wire_us;
        let (np, sh) = sim.planner_counts();
        self.no_plan += u64::from(np);
        for (sum, n) in self.shapes.iter_mut().zip(sh) {
            *sum += u64::from(n);
        }
    }
}

enum SimAny {
    Old(Sim<Baseline>),
    New(Sim<NewRule>),
}

impl SimAny {
    fn c(&self) -> &send_plan::model::Counters {
        match self {
            SimAny::Old(s) => &s.c,
            SimAny::New(s) => &s.c,
        }
    }
    fn planner_counts(&self) -> (u32, [u32; 3]) {
        match self {
            SimAny::Old(s) => (s.planner.no_plan, [s.c.presents, 0, 0]),
            SimAny::New(s) => (s.planner.no_plan, s.planner.shapes),
        }
    }
}

fn run<P: Planner>(
    steps: &[Step],
    board: BoardTimes,
    frame: LoopTimes,
    p: P,
    phase: f64,
) -> Sim<P> {
    send_plan::model::replay(steps, board, frame, p, phase)
}

fn main() -> ExitCode {
    let mut phases = 40u32;
    let mut extra = 0.0f64;
    let mut lead_extra = 0.0f64;
    let mut dir = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--phases" => phases = args.next().and_then(|s| s.parse().ok()).unwrap_or(40),
            "--extra-us" => extra = args.next().and_then(|s| s.parse().ok()).unwrap_or(0.0),
            "--lead-extra-us" => {
                lead_extra = args.next().and_then(|s| s.parse().ok()).unwrap_or(0.0)
            }
            _ => dir = Some(a),
        }
    }
    let Some(dir) = dir else {
        eprintln!("score: give the folder with the area lists");
        return ExitCode::FAILURE;
    };
    let board = BoardTimes {
        send_extra_us: extra,
        lead_extra_us: lead_extra,
        ..BoardTimes::default()
    };
    let rules: Vec<(&str, Option<Rules>)> = vec![
        ("today", None),
        (
            "box when no pass",
            Some(Rules {
                groups: false,
                boxed: true,
                only_without_pass: true,
                ..Rules::NEW
            }),
        ),
        ("FIRMWARE: sooner of windows/box", Some(Rules::FIRMWARE)),
        (
            "sooner: windows/groups",
            Some(Rules {
                boxed: false,
                ..Rules::NEW
            }),
        ),
        ("sooner of windows/groups/box", Some(Rules::NEW)),
        (
            "sooner, groups gap 16",
            Some(Rules {
                gap: 16,
                ..Rules::NEW
            }),
        ),
        (
            "firmware, +2 lines a window",
            Some(Rules {
                window_cost: 2.0,
                ..Rules::FIRMWARE
            }),
        ),
    ];
    println!(
        "send_extra_us={extra} lead_extra_us={lead_extra} phases={phases} (MODEL: send-plan model.rs over the simulator's area lists)"
    );
    println!(
        "| run | rule | fps | present avg / max ms | wait ms | wire ms | no one-pass % | windows/present | windows/groups/box % | late /run | crossed /run (belief/true) | px/present |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
    for detail in ["light", "normal", "heavy"] {
        for rate in [30u32, 60] {
            let path = format!("{dir}/{detail}-{rate}.txt");
            let steps = match load(&path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("score: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let frame = LoopTimes {
                period_us: if rate == 60 { 16_667.0 } else { 33_333.0 },
                draw_us: draw_us(detail, rate),
                loop_us: 100.0,
            };
            for (name, r) in &rules {
                let mut row = Row::default();
                let mut rng = Rng(0x5eed_1234_abcd_0001);
                for _ in 0..phases {
                    let phase = rng.next_f64() * f64::from(board.lines);
                    let sim = match r {
                        None => SimAny::Old(run(&steps, board, frame, Baseline::default(), phase)),
                        Some(r) => SimAny::New(run(&steps, board, frame, NewRule::new(*r), phase)),
                    };
                    row.add(&sim);
                }
                let n = row.presents.max(1) as f64;
                let runs = f64::from(row.runs.max(1));
                println!(
                    "| {detail}/{rate} | {name} | {:.1} | {:.1} / {:.1} | {:.1} | {:.1} | {:.0} | {:.2} | {:.0}/{:.0}/{:.0} | {:.1} | {:.2}/{:.2} | {:.0} |",
                    row.fps / runs,
                    row.present_sum / n / 1000.0,
                    row.present_max / 1000.0,
                    row.spin_us / n / 1000.0,
                    row.wire_us / n / 1000.0,
                    100.0 * row.no_plan as f64 / n,
                    row.windows as f64 / n,
                    100.0 * row.shapes[0] as f64 / n,
                    100.0 * row.shapes[1] as f64 / n,
                    100.0 * row.shapes[2] as f64 / n,
                    row.late as f64 / runs,
                    row.crossed as f64 / runs,
                    row.crossed_true as f64 / runs,
                    row.pixels as f64 / n,
                );
            }
        }
    }
    ExitCode::SUCCESS
}
