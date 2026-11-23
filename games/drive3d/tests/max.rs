//! MAX, the level above HIGH: its guard.
//!
//! * The triangle count over the scripted drive and the sweep stays in
//!   MAX's budget (the 95 % point at most the budget, the worst at most
//!   5 % over it), and no picture loses a triangle (the renderer's list
//!   never runs over).
//! * The driving rules are the same as HIGH's: the same drives at HIGH
//!   and at MAX give the very same states, step by step.
//! * The far edge's test in `scene::cell_unit` (a squared distance) is
//!   the old one (a place in the order table from the level's first
//!   entry on), for every level.
//!
//! Set `MAX_NUMBERS` to a file to get the counts (HIGH and MAX) there.

mod common;

use common::autopilot;
use drive3d::detail::Detail;
use drive3d::draw::{self, Opts};
use drive3d::look::Look;
use drive3d::order::{MAX_GROUP, N, NQ, ORDER, REACH, STARTS, group, key};
use drive3d::play::{Input, Play};
use drive3d::road::{POINTS, ROAD};
use drive3d::scene::Painter;
use drive3d::tune::M;
use game_3d::Stats;
use game_kit::{Frame, H, W};

const PLAIN: Opts = Opts {
    line: false,
    plain: false,
};

fn stats(p: &Play, d: Detail) -> Stats {
    let mut p = *p;
    p.detail = d;
    let mut paint = Box::new(Painter::new());
    let mut f = Box::new(Frame([0; W * H]));
    let look = Look::new(&p, 1);
    draw::picture(&mut f, &mut paint, &look, None, PLAIN, &mut ());
    paint.r.stats
}

/// The scripted drive (`tests/speed_counts.rs`): the test driver at 20
/// m/s, 14 in the corners, from the start until the car stands past
/// the finish.
fn scripted() -> Vec<Play> {
    let mut p = Play::new();
    let mut out = vec![p];
    for _ in 0..1_500 {
        let input = autopilot(&p, 20 * M / 30, 14 * M / 30);
        p.step(&input);
        out.push(p);
        if p.finished.is_some() && p.car.speed() == 0 {
            break;
        }
    }
    out
}

/// `tests/draw.rs`'s drives: plain, or sliding (faster into the corners,
/// the handbrake in bursts there).
fn drive(slide: bool) -> Vec<Play> {
    let mut p = Play::new();
    let mut out = vec![p];
    let (cruise, corner) = if slide {
        (20 * M / 30, 17 * M / 30)
    } else {
        (20 * M / 30, 14 * M / 30)
    };
    for n in 0..1_500u32 {
        let mut input: Input = autopilot(&p, cruise, corner);
        if slide {
            let piece = ROAD.piece[usize::from(p.last_road).min(POINTS - 1)];
            if (piece == 5 || piece == 7) && n % 14 < 5 {
                input.controls.handbrake = true;
            }
        }
        p.step(&input);
        out.push(p);
        if p.finished.is_some_and(|_| p.steps > 0) && usize::from(p.last_road) >= POINTS - 1 {
            for _ in 0..20 {
                let i = autopilot(&p, cruise, corner);
                p.step(&i);
                out.push(p);
            }
            break;
        }
    }
    out
}

/// `tests/draw.rs`'s sweep: every 3rd step of the plain drive and every
/// 3rd step of the sliding drive in the corners.
fn sweep() -> Vec<Play> {
    let mut out: Vec<Play> = drive(false).into_iter().step_by(3).collect();
    for p in drive(true).into_iter().step_by(3) {
        let piece = ROAD.piece[usize::from(p.last_road).min(POINTS - 1)];
        if piece == 5 || piece == 7 {
            out.push(p);
        }
    }
    out
}

/// The 95 % point (the one `tests/draw.rs` takes) and the most.
fn p95_max(v: &[u32]) -> (u32, u32) {
    let mut s = v.to_vec();
    s.sort_unstable();
    (s[(s.len() * 95).div_ceil(100) - 1], s[s.len() - 1])
}

fn mean(v: &[u32]) -> u64 {
    v.iter().map(|&t| u64::from(t)).sum::<u64>() / v.len() as u64
}

#[test]
fn max_keeps_its_budget_and_drops_no_triangle() {
    let mut out = String::from("# MAX counts (MEASURED, PC, tests/max.rs)\n");
    let mut fails = Vec::new();
    for (name, plays) in [("scripted drive", scripted()), ("sweep", sweep())] {
        for d in [Detail::High, Detail::Max] {
            let (mut tris, mut px) = (Vec::new(), Vec::new());
            let mut worst_at = 0;
            for (i, p) in plays.iter().enumerate() {
                let s = stats(p, d);
                if s.dropped != 0 {
                    fails.push(format!("{} {name} {i}: {} dropped", d.name(), s.dropped));
                }
                if tris.iter().all(|&t| s.tris > t) {
                    worst_at = i;
                }
                tris.push(s.tris);
                px.push(s.pixels);
            }
            let (tp, tm) = p95_max(&tris);
            let (pp, pm) = p95_max(&px);
            let budget = u32::from(d.reach().budget);
            let line = format!(
                "{} {name} ({} pictures): triangles mean {} p95 {tp} max {tm} (budget {budget}; most at picture {worst_at}, road point {}) | pixels mean {} p95 {pp} max {pm}\n",
                d.name(),
                plays.len(),
                mean(&tris),
                plays[worst_at].last_road,
                mean(&px),
            );
            print!("{line}");
            out.push_str(&line);
            if d == Detail::Max {
                if tp > budget {
                    fails.push(format!("MAX {name}: 95 % point {tp} over {budget}"));
                }
                if tm * 100 > budget * 105 {
                    fails.push(format!(
                        "MAX {name}: worst {tm} more than 5 % over {budget}"
                    ));
                }
            }
        }
    }
    if let Ok(path) = std::env::var("MAX_NUMBERS") {
        std::fs::write(path, out).expect("write the numbers");
    }
    assert!(fails.is_empty(), "{fails:#?}");
}

/// The same drives at HIGH and at MAX: the same state at every step
/// (the level itself aside), and the same at the end, X presses and the
/// menu's level included.
#[test]
fn max_drives_by_the_same_rules_as_high() {
    for (cruise, corner, brake_bursts) in [(20, 14, false), (20, 17, true), (28, 24, false)] {
        let mut a = Play::new();
        let mut b = Play::new();
        a.detail = Detail::High;
        b.detail = Detail::Max;
        for n in 0..1_800u32 {
            let mut input = autopilot(&a, cruise * M / 30, corner * M / 30);
            if brake_bursts && n % 14 < 5 {
                input.controls.handbrake = true;
            }
            a.step(&input);
            b.step(&input);
            let mut bb = b;
            bb.detail = a.detail;
            assert_eq!(a, bb, "{cruise} m/s, step {n}");
            if a.finished.is_some() && a.car.speed() == 0 {
                break;
            }
        }
        assert!(a.finished.is_some(), "{cruise} m/s: finished");
    }
    // X from HIGH goes to MAX, from MAX round to LOW; the step is the
    // same as at any other level.
    let x = Input {
        level: true,
        ..Input::default()
    };
    let mut p = Play::new();
    p.detail = Detail::High;
    let mut q = Play::new();
    q.detail = Detail::Mid;
    p.step(&x);
    q.step(&x);
    assert_eq!(p.detail, Detail::Max);
    assert_eq!(q.detail, Detail::High);
    assert_eq!(p.print, q.print);
    p.step(&x);
    assert_eq!(p.detail, Detail::Low);
}

/// `cell_unit` keeps a block when its squared offset is within the
/// level's far edge; the walk draws the order's quarter from the
/// level's first entry, turned back into the whole order by `group`.
/// Both are the same blocks, in the whole order's order, for every
/// level; and the key grows along the order (all also checked while
/// compiling).
#[test]
fn the_far_edge_test_is_the_walks() {
    for d in [Detail::Low, Detail::Mid, Detail::High, Detail::Max] {
        let b = i32::from(d.reach().far_m) / 16;
        assert!(b <= REACH);
        assert_eq!(i32::from(d.reach().far_m) % 16, 0);
        let mut walked = Vec::new();
        let mut at = usize::from(STARTS[d as usize]);
        let mut row = [[0i8; 2]; MAX_GROUP];
        while at < NQ {
            let (n, next) = group(at, &mut row);
            walked.extend_from_slice(&row[..n]);
            at = next;
        }
        let within: Vec<[i8; 2]> = ORDER
            .iter()
            .copied()
            .filter(|&[di, dj]| {
                let (di, dj) = (i32::from(di), i32::from(dj));
                di * di + dj * dj <= b * b
            })
            .collect();
        assert_eq!(walked, within, "{d:?}");
        assert_eq!(
            &ORDER[N - within.len()..],
            &within[..],
            "{d:?}: the order's tail"
        );
        // Offsets not in the order lie beyond every far edge.
        for dj in -REACH - 2..=REACH + 2 {
            for di in -REACH - 2..=REACH + 2 {
                if di * di + dj * dj > REACH * REACH {
                    assert!(di * di + dj * dj > b * b);
                }
            }
        }
    }
    for w in ORDER.windows(2) {
        let [a, b] = [w[0], w[1]].map(|[i, j]| key(i32::from(i), i32::from(j)));
        assert!(a < b);
    }
    assert!(key(0, 0) * 4 + 3 < 1 << 24);
}
