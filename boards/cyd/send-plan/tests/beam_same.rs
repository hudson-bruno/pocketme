//! The move of the beam logic out of boards/cyd/src/beam.rs changed no
//! decision: the code as it was before the move (copied below, only the
//! reads turned into closures) and send_plan::beam give the same results
//! for the same reads, apart from the changes made on purpose
//! (MAX_FLOOR_US 60 -> 40, a survey with a missed wrap refused, and a
//! survey with an odd step or uneven wraps refused).

use send_plan::beam::{self, Counter, MAX_FLOOR_US, Refused, anchor_try, calibration};

/// The code before the move, from boards/cyd/src/beam.rs.
mod old {
    pub const CPU_MHZ: u32 = 240;
    pub const SCANLINE_MASK: u16 = 0x3FF;
    const FIT_LO: u16 = 10;
    const FIT_HI: u16 = 320;
    const INNER_SLACK_US: u32 = 8;
    const FLOOR_READS: u32 = 64;
    pub const MAX_FLOOR_US: u32 = 60;
    const ANCHOR_EDGE_LINES: u16 = 2;
    const HELD_LINES: f32 = 3.0;

    #[derive(Default)]
    pub struct Survey {
        pub samples: u32,
        pub rejected: u32,
        pub rejected_error: u32,
        pub rejected_slow: u32,
        pub rejected_bits: u32,
        pub inner_min: u32,
        pub spread: [u32; 5],
        pub line_min: u16,
        pub line_max: u16,
        pub wraps: u32,
        first_wrap: u32,
        last_wrap: u32,
        fit_dt: u64,
        fit_dl: u32,
        outer_sum: u64,
        held_line: u16,
        held_cycles: u32,
    }

    impl Survey {
        pub fn period_cycles(&self) -> Option<f32> {
            (self.wraps > 1).then(|| {
                self.last_wrap.wrapping_sub(self.first_wrap) as f32 / (self.wraps - 1) as f32
            })
        }
        pub fn line_cycles(&self) -> Option<f32> {
            (self.fit_dl > 0).then(|| self.fit_dt as f32 / self.fit_dl as f32)
        }
        fn read_cycles(&self) -> u32 {
            (self.outer_sum / u64::from(self.samples.max(1))) as u32
        }
        pub fn inner_limit(&self) -> u32 {
            self.inner_min.saturating_add(INNER_SLACK_US * CPU_MHZ)
        }
    }

    pub fn survey(
        mut read: impl FnMut() -> Option<(u16, u32, u32)>,
        mut cycles: impl FnMut() -> u32,
        ms: u32,
    ) -> Survey {
        let span = ms * 1000 * CPU_MHZ;
        let mut s = Survey {
            line_min: u16::MAX,
            held_line: u16::MAX,
            inner_min: u32::MAX,
            ..Default::default()
        };
        let mut prev: Option<(u16, u32)> = None;
        let mut in_refresh = false;
        let (mut first, mut last) = (None::<(u16, u32)>, None::<(u16, u32)>);
        let (mut run_line, mut run_start) = (u16::MAX, 0u32);
        for _ in 0..FLOOR_READS {
            if let Some((_, a, b)) = read() {
                s.inner_min = s.inner_min.min(b.wrapping_sub(a));
            }
        }
        let start = cycles();
        while cycles().wrapping_sub(start) < span {
            let t0 = cycles();
            let Some((raw, a, b)) = read() else {
                s.rejected += 1;
                s.rejected_error += 1;
                continue;
            };
            let line = raw & SCANLINE_MASK;
            let outer = cycles().wrapping_sub(t0);
            let inner = b.wrapping_sub(a);
            s.inner_min = s.inner_min.min(inner);
            let over = (inner - s.inner_min) / CPU_MHZ;
            s.spread[match over {
                0..=1 => 0,
                2 => 1,
                3..=4 => 2,
                5..=8 => 3,
                _ => 4,
            }] += 1;
            if inner > s.inner_limit() || raw != line {
                s.rejected += 1;
                if raw != line {
                    s.rejected_bits += 1;
                } else {
                    s.rejected_slow += 1;
                }
                continue;
            }
            let stamp = a.wrapping_add(inner / 2);
            s.samples += 1;
            s.outer_sum += u64::from(outer);
            s.line_min = s.line_min.min(line);
            s.line_max = s.line_max.max(line);
            if let Some((pl, pt)) = prev {
                if line + 100 < pl {
                    if s.wraps == 0 {
                        s.first_wrap = stamp;
                    }
                    s.last_wrap = stamp;
                    s.wraps += 1;
                    if in_refresh
                        && let (Some(f), Some(l)) = (first, last)
                        && l.0 > f.0
                    {
                        s.fit_dt += u64::from(l.1.wrapping_sub(f.1));
                        s.fit_dl += u32::from(l.0 - f.0);
                    }
                    in_refresh = true;
                    first = None;
                    last = None;
                }
                if line != run_line {
                    let held = pt.wrapping_sub(run_start);
                    if held > s.held_cycles {
                        s.held_cycles = held;
                        s.held_line = run_line;
                    }
                    run_line = line;
                    run_start = stamp;
                }
            } else {
                run_line = line;
                run_start = stamp;
            }
            if in_refresh && (FIT_LO..=FIT_HI).contains(&line) {
                if first.is_none() {
                    first = Some((line, stamp));
                }
                last = Some((line, stamp));
            }
            prev = Some((line, stamp));
        }
        s
    }

    /// Beam::calibrate's result: (inner_limit, line_cycles, lines,
    /// line_min, line_max, held_line, read_cycles).
    pub type Cal = (u32, f32, f32, u16, u16, u16, u32);

    pub fn calibrate(s: &Survey) -> Option<Cal> {
        let (Some(period), Some(line)) = (s.period_cycles(), s.line_cycles()) else {
            return None;
        };
        let lines = period / line;
        if !(322.0..=574.0).contains(&lines) {
            return None;
        }
        if s.inner_min > MAX_FLOOR_US * CPU_MHZ {
            return None;
        }
        let held_line = if s.held_cycles as f32 > HELD_LINES * line {
            s.held_line
        } else {
            u16::MAX
        };
        Some((
            s.inner_limit(),
            line,
            lines,
            s.line_min,
            s.line_max,
            held_line,
            s.read_cycles(),
        ))
    }

    pub struct Range {
        pub line_min: u16,
        pub line_max: u16,
        pub held_line: u16,
    }

    fn trusted(r: &Range, line: u16) -> bool {
        line >= r.line_min + ANCHOR_EDGE_LINES
            && line + ANCHOR_EDGE_LINES <= r.line_max
            && (r.held_line == u16::MAX || line.abs_diff(r.held_line) > 1)
    }

    /// One try of Beam::anchor: Ok((anchor_cycles, line)) or the miss.
    pub fn anchor_try(
        (la, lb): (u32, u32),
        (raw, a, b): (u16, u32, u32),
        inner_limit: u32,
        r: &Range,
    ) -> Result<(u32, u16), usize> {
        let line = raw & SCANLINE_MASK;
        let own = b.wrapping_sub(a);
        let inner = lb.wrapping_sub(la).max(own);
        let miss = if inner > inner_limit {
            Some(0)
        } else if raw != line {
            Some(1)
        } else if !trusted(r, line) {
            Some(2)
        } else {
            None
        };
        if let Some(m) = miss {
            return Err(m);
        }
        Ok((la.wrapping_add(inner.min(own) / 2), line))
    }
}

/// A small fixed random source (no crates).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
}

/// A rough panel and reads with random lengths, failures, bad bits and
/// gaps; the same stream for both versions. Like the panel, a read
/// answers with the line at the start of the read before it, so a gap
/// (a pause after a read) never sits between two close reads' answers.
/// One read's result (`None`: it failed) and the cycle count after it.
type Step = (Option<(u16, u32, u32)>, u32);

fn reads(seed: u64, base_us: u32) -> Vec<Step> {
    let mut rng = Rng(seed);
    let mut t: u32 = rng.next().wrapping_mul(7); // any start, wraps too
    let line_cycles = 49.4 + rng.below(30) as f32 / 100.0;
    let lines = 340.0 + rng.below(80) as f32 / 10.0;
    let mut pos = rng.below(300) as f32;
    // The line at the start of the read before (the first read answers
    // its own).
    let mut latched = (pos as u16 + 1).min(333);
    let mut out = Vec::new();
    // 64 floor reads plus 200 ms of reads at about 18 us each, plus room.
    for _ in 0..40_000 {
        let inner = base_us * 240
            + match rng.below(100) {
                0..=89 => rng.below(240),
                90..=97 => rng.below(240 * 10),
                _ => rng.below(240 * 40),
            };
        let pre = 60;
        let a = t.wrapping_add(pre);
        let b = a.wrapping_add(inner);
        let gap = if rng.below(2000) == 0 {
            rng.below(240 * 30_000)
        } else {
            0
        };
        let step = pre + inner + 200 + gap;
        let value = std::mem::replace(&mut latched, (pos as u16 + 1).min(333));
        pos = (pos + step as f32 / (line_cycles * 240.0)) % lines;
        let raw = if rng.below(500) == 0 {
            value | 0x400
        } else {
            value
        };
        let r = (rng.below(300) != 0).then_some((raw, a, b));
        t = t.wrapping_add(step);
        out.push((r, t));
    }
    out
}

fn run<S>(
    stream: &[Step],
    f: impl FnOnce(&mut dyn FnMut() -> Option<(u16, u32, u32)>, &mut dyn FnMut() -> u32) -> S,
) -> S {
    let i = std::cell::Cell::new(0usize);
    let now = std::cell::Cell::new(stream[0].1);
    let mut read = || {
        let (r, t) = stream[i.get().min(stream.len() - 1)];
        i.set(i.get() + 1);
        now.set(t);
        r
    };
    let mut cycles = || now.get();
    f(&mut read, &mut cycles)
}

#[test]
fn the_survey_and_calibration_decide_as_before() {
    let (mut same, mut floor_only, mut wrap_only, mut uneven_only) = (0, 0, 0, 0);
    let mut first_lost = 0;
    for seed in 0..300u64 {
        let base = [12, 17, 17, 17, 25, 30, 39, 45, 55, 61][seed as usize % 10];
        let stream = reads(seed, base);
        let o = run(&stream, |r, c| old::survey(r, c, 200));
        let n = run(&stream, |r, c| beam::survey(r, c, 200));
        assert_eq!(
            (
                o.samples,
                o.rejected,
                o.rejected_error,
                o.rejected_slow,
                o.rejected_bits
            ),
            (
                n.samples,
                n.rejected,
                n.rejected_error,
                n.rejected_slow,
                n.rejected_bits
            ),
            "seed {seed}"
        );
        assert_eq!(
            (o.inner_min, o.spread),
            (n.inner_min, n.spread),
            "seed {seed}"
        );
        assert_eq!(
            (o.line_min, o.line_max),
            (n.line_min, n.line_max),
            "seed {seed}"
        );
        assert_eq!(o.inner_limit(), n.inner_limit(), "seed {seed}");
        // A change on purpose: a drop right after the first kept read is
        // no wrap now (nothing checks the first answer). When the first
        // step was a real wrap, the new survey has one wrap fewer and
        // takes its figures from the other refreshes: the same decision,
        // with figures that agree.
        if o.wraps == n.wraps + 1 {
            let (oc, nc) = (old::calibrate(&o), calibration(&n));
            match (oc, nc) {
                (Some(oc), Ok(c)) => {
                    assert!((oc.1 / c.line_cycles - 1.0).abs() < 0.002, "seed {seed}");
                    assert!((oc.2 - c.lines).abs() < 0.5, "seed {seed}");
                }
                (None, Err(_)) => {}
                (o, n) => panic!("seed {seed}: first wrap lost: before {o:?}, now {n:?}"),
            }
            first_lost += 1;
            continue;
        }
        assert_eq!(o.wraps, n.wraps, "seed {seed}");
        assert_eq!(o.period_cycles(), n.period_cycles(), "seed {seed}");
        assert_eq!(o.line_cycles(), n.line_cycles(), "seed {seed}");

        let oc = old::calibrate(&o);
        let nc = calibration(&n);
        match (oc, nc) {
            (Some(oc), Ok(c)) => {
                let nc = (
                    c.inner_limit,
                    c.line_cycles,
                    c.lines,
                    c.counter.line_min,
                    c.counter.line_max,
                    c.counter.held_line,
                    c.read_cycles,
                );
                assert_eq!(oc, nc, "seed {seed}");
                same += 1;
            }
            (None, Err(_)) => same += 1,
            // The changes on purpose, and nothing else.
            (Some(_), Err(Refused::SlowReads)) => {
                assert!(n.inner_min > MAX_FLOOR_US * 240 && n.inner_min <= old::MAX_FLOOR_US * 240);
                floor_only += 1;
            }
            (Some(_), Err(Refused::MissedWrap)) => wrap_only += 1,
            // Wraps lost to gaps in a pattern that the longest gap does
            // not show (these streams have no wrong answers, and no odd
            // step may come up at all): every survey refused only for
            // this had wrong figures before (these panels have lines of
            // 49.4 to 49.7 us and 340 to 348 lines).
            (Some(oc), Err(Refused::UnevenWraps)) => {
                let line_us = oc.1 / 240.0;
                assert!(
                    !(48.5..=50.5).contains(&line_us) || !(337.0..=351.0).contains(&oc.2),
                    "seed {seed}: {oc:?}"
                );
                uneven_only += 1;
            }
            (o, n) => panic!("seed {seed}: before {o:?}, now {n:?}"),
        }
    }
    eprintln!(
        "same {same}, refused now for the floor {floor_only}, for a missed wrap {wrap_only}, for uneven wraps {uneven_only}, first wrap lost {first_lost}"
    );
    // The streams reach every case.
    assert!(same > 100, "same {same}");
    assert!(floor_only > 0, "floor {floor_only}");
    assert!(wrap_only > 0, "wrap {wrap_only}");
}

#[test]
fn the_anchor_try_decides_as_before() {
    let mut rng = Rng(7);
    let mut seen = [0u32; 4];
    for _ in 0..200_000 {
        let r = Counter {
            line_min: rng.below(5) as u16,
            line_max: 300 + rng.below(40) as u16,
            held_line: if rng.below(3) == 0 {
                u16::MAX
            } else {
                300 + rng.below(40) as u16
            },
        };
        let limit = (17 + rng.below(30)) * 240;
        let la = rng.next().wrapping_mul(3);
        let lb = la.wrapping_add(rng.below(50 * 240));
        let a = lb.wrapping_add(rng.below(500));
        let b = a.wrapping_add(rng.below(50 * 240));
        let raw = if rng.below(20) == 0 {
            rng.below(0x800) as u16
        } else {
            rng.below(345) as u16
        };
        let o = old::anchor_try(
            (la, lb),
            (raw, a, b),
            limit,
            &old::Range {
                line_min: r.line_min,
                line_max: r.line_max,
                held_line: r.held_line,
            },
        );
        let n = anchor_try((la, lb), (raw, a, b), limit, &r).map(|x| (x.cycles, x.line));
        assert_eq!(o, n);
        seen[n.err().unwrap_or(3)] += 1;
    }
    assert!(seen.iter().all(|&k| k > 1000), "{seen:?}");
}
