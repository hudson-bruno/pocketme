//! The beam model's decisions (send_plan::beam) against a simulated
//! panel: the start-up survey, its checks, and the anchor try.
//!
//! The simulated panel counts gate lines like the CYD's: value
//! floor(position) + 1, held at 333 through the rest of the porch, about
//! 343.4 lines a refresh of 49.55 us each. Like the real panel, a read
//! answers with the value at the time of the PREVIOUS read. Times are
//! cycles of a 240 MHz counter; the simulation keeps them in u64 and
//! hands the code the low 32 bits, so the counter can wrap.

use std::cell::Cell;

use send_plan::beam::{
    Anchor, CPU_MHZ, Calibration, Counter, DEFAULT_MAX_INNER_US, MAX_FLOOR_US, MISS_BITS,
    MISS_RANGE, MISS_SLOW, Refused, Survey, anchor_try, calibration, stamp, survey,
};

const MHZ: u64 = CPU_MHZ as u64;
const LINE_US: f64 = 49.55;
const LINES: f64 = 343.4;
const LINE_MAX: u16 = 333;
const SURVEY_MS: u32 = 200;
/// Cycles from the start of a read to its first timed cycle, and after
/// its last one (the clock switches around the timed part).
const PRE: u64 = 60;
const POST: u64 = 240;
/// Cycles from the timed start until the panel takes its line, in a read
/// with no extra delay.
const LATCH: u64 = 2 * MHZ;

fn us(n: f64) -> u64 {
    (n * MHZ as f64).round() as u64
}

/// One read as the simulation makes it: its timed length, the part of
/// it spent before the panel takes the line (beyond LATCH), and whether
/// it fails.
#[derive(Clone, Copy)]
struct Shape {
    inner: u64,
    extra: u64,
    fail: bool,
}

fn fast() -> Shape {
    Shape {
        inner: us(17.0),
        extra: 0,
        fail: false,
    }
}

struct Sim {
    now: Cell<u64>,
    /// Added to the time before it is cut to 32 bits.
    offset: u64,
    line_cycles: f64,
    reads: Cell<u64>,
    prev_latch: Cell<Option<u64>>,
}

impl Sim {
    fn new(offset: u64) -> Self {
        Sim {
            now: Cell::new(us(1234.5)),
            offset,
            line_cycles: LINE_US * MHZ as f64,
            reads: Cell::new(0),
            prev_latch: Cell::new(None),
        }
    }

    fn counter(&self, t: u64) -> u32 {
        (t + self.offset) as u32
    }

    /// The panel's position (lines since refresh 0 began, not wrapped).
    fn position(&self, t: u64) -> f64 {
        t as f64 / self.line_cycles
    }

    fn value(&self, t: u64) -> u16 {
        let p = self.position(t) % LINES;
        ((p as u16) + 1).min(LINE_MAX)
    }

    /// One read with this shape: `(answer, a, b)`, and the time the
    /// panel took the line.
    fn read(&self, s: Shape) -> (Option<(u16, u32, u32)>, u64) {
        let a = self.now.get() + PRE;
        let b = a + s.inner;
        let latch = a + LATCH + s.extra;
        self.now.set(b + POST);
        self.reads.set(self.reads.get() + 1);
        let answer_at = self.prev_latch.replace(Some(latch)).unwrap_or(latch);
        let out = (!s.fail).then(|| (self.value(answer_at), self.counter(a), self.counter(b)));
        (out, latch)
    }

    /// A survey where read number n (from 0, floor reads included) and
    /// its start time decide its shape.
    fn survey(&self, shape: impl Fn(u64, u64) -> Shape) -> Survey {
        survey(
            || {
                let s = shape(self.reads.get(), self.now.get());
                self.read(s).0
            },
            || self.counter(self.now.get()),
            SURVEY_MS,
        )
    }
}

fn line_us(c: &Calibration) -> f64 {
    f64::from(c.line_cycles) / MHZ as f64
}

/// A survey of fast, even reads gives the panel's line time, its lines
/// per refresh and its held value.
#[test]
fn even_fast_reads_give_the_panels_timing() {
    let sim = Sim::new(0);
    let s = sim.survey(|_, _| fast());
    assert_eq!(s.fastest_read_us(), Some(17));
    assert_eq!(s.rejected, 0);
    assert_eq!(s.spread[1..], [0, 0, 0, 0]);
    assert!(s.samples > 10_000, "samples {}", s.samples);
    assert_eq!(s.wraps, 11);
    let c = calibration(&s).unwrap();
    assert!(
        (line_us(&c) - LINE_US).abs() < 0.003 * LINE_US,
        "line {}",
        line_us(&c)
    );
    assert!(
        (f64::from(c.lines) - LINES).abs() < 0.003 * LINES,
        "lines {}",
        c.lines
    );
    assert_eq!(
        c.counter,
        Counter {
            line_min: 1,
            line_max: LINE_MAX,
            held_line: LINE_MAX,
        }
    );
    assert_eq!(c.inner_limit, (17 + 8) * CPU_MHZ);
}

/// Reads that are all slow by the same amount give the same line time,
/// and their time stamp moves by half the extra time.
#[test]
fn even_slow_reads_give_the_same_timing_and_a_bias_of_half_the_extra() {
    let extra = us(13.0);
    let slow = Shape {
        inner: us(17.0) + extra,
        extra,
        fail: false,
    };
    let fast_c = calibration(&Sim::new(0).survey(|_, _| fast())).unwrap();
    let s = Sim::new(0).survey(|_, _| slow);
    assert_eq!(s.fastest_read_us(), Some(30));
    assert_eq!(s.rejected, 0);
    let slow_c = calibration(&s).unwrap();
    assert!((line_us(&slow_c) - LINE_US).abs() < 0.003 * LINE_US);
    assert!((slow_c.lines - fast_c.lines).abs() < 0.5);

    // Where the stamp lands against the moment the panel took its line.
    let late = |shape: Shape| {
        let sim = Sim::new(0);
        let (_, latch) = sim.read(shape);
        let a = latch - LATCH - shape.extra;
        latch as i64 - i64::from(stamp(a as u32, (a + shape.inner) as u32))
    };
    assert_eq!(late(slow) - late(fast()), (extra / 2) as i64);
}

/// One read faster than all others lowers the limit: the others still
/// pass while they are within the slack of it.
#[test]
fn one_freak_fast_read_only_tightens_the_limit() {
    let freak_at = 5_000;
    let s = Sim::new(0).survey(|n, _| {
        if n == freak_at {
            Shape {
                inner: us(15.0),
                ..fast()
            }
        } else {
            fast()
        }
    });
    assert_eq!(s.fastest_read_us(), Some(15));
    assert_eq!(s.rejected, 0);
    // The reads after it are 2 us over the fastest.
    assert!(s.spread[1] > 4_000, "spread {:?}", s.spread);
    let c = calibration(&s).unwrap();
    assert_eq!(c.inner_limit, (15 + 8) * CPU_MHZ);
    assert!((line_us(&c) - LINE_US).abs() < 0.003 * LINE_US);
}

/// A read more than the slack faster than all others (9 us: faster than
/// the SPI transfer itself allows, ESTIMATE) makes every later read
/// "slow". Early in the survey that leaves too little to fit: the survey
/// is not used. Known and accepted: no such read has been seen.
#[test]
fn a_freak_read_more_than_the_slack_faster_rejects_all_later_reads() {
    let s = Sim::new(0).survey(|n, _| {
        if n == 100 {
            Shape {
                inner: us(8.0),
                ..fast()
            }
        } else {
            fast()
        }
    });
    assert_eq!(s.fastest_read_us(), Some(8));
    assert!(s.rejected_slow > 10_000);
    assert_eq!(calibration(&s), Err(Refused::NoFit));
}

/// Cold reads at the start (code not yet in the flash cache) do not
/// spoil the survey: the fastest read lowers the limit as soon as it
/// comes.
#[test]
fn cold_first_reads_do_not_spoil_the_survey() {
    // All 64 floor reads and the first 6 survey reads cold (37 us).
    let s = Sim::new(0).survey(|n, _| {
        if n < 70 {
            Shape {
                inner: us(37.0),
                extra: us(20.0),
                fail: false,
            }
        } else {
            fast()
        }
    });
    assert_eq!(s.fastest_read_us(), Some(17));
    assert_eq!(s.rejected, 0);
    let c = calibration(&s).unwrap();
    assert_eq!(c.inner_limit, (17 + 8) * CPU_MHZ);
    assert!((line_us(&c) - LINE_US).abs() < 0.003 * LINE_US);

    // Only the first three cold: the floor reads find 17 us at once.
    let s = Sim::new(0).survey(|n, _| {
        if n < 3 {
            Shape {
                inner: us(37.0),
                extra: us(20.0),
                fail: false,
            }
        } else {
            fast()
        }
    });
    assert_eq!(s.fastest_read_us(), Some(17));
    assert_eq!(s.rejected, 0);
    assert_eq!(s.spread[1..], [0, 0, 0, 0]);
    assert!(calibration(&s).is_ok());
}

/// The cycle counter wraps (every 17.9 s) in the middle of the survey:
/// the result is the very same.
#[test]
fn a_counter_wrap_in_the_survey_changes_nothing() {
    let plain = calibration(&Sim::new(0).survey(|_, _| fast())).unwrap();
    // The counter reaches 2^32 about 100 ms into the survey.
    let offset = (1u64 << 32) - us(1234.5) - us(100_000.0);
    let sim = Sim::new(offset);
    let s = sim.survey(|_, _| fast());
    assert!(
        sim.counter(sim.now.get()) < sim.counter(0),
        "the counter did not wrap"
    );
    assert_eq!(calibration(&s).unwrap(), plain);
}

/// No read is kept for a little more than one refresh: the survey sees
/// one wrap fewer, and its average refresh comes out about 10 percent
/// long. The lines check alone (322 to 574) lets that through; the wrap
/// gap check refuses it.
#[test]
fn a_missed_wrap_is_refused() {
    let period = (LINES * LINE_US * MHZ as f64) as u64;
    let t_gap = us(1234.5) + us(100_000.0);
    let s = Sim::new(0).survey(|_, t| Shape {
        fail: (t_gap..t_gap + period + us(2000.0)).contains(&t),
        ..fast()
    });
    assert_eq!(s.wraps, 10);
    assert!(s.rejected_error > 900);
    let lines = s.period_cycles().unwrap() / s.line_cycles().unwrap();
    assert!((322.0..=574.0).contains(&lines), "lines {lines}");
    assert!(lines > 1.05 * LINES as f32, "lines {lines}");
    assert!(s.max_wrap_gap() as f32 > 1.9 * period as f32);
    assert_eq!(calibration(&s), Err(Refused::MissedWrap));

    // A gap shorter than a refresh loses no wrap.
    let s = Sim::new(0).survey(|_, t| Shape {
        fail: (t_gap..t_gap + period / 2).contains(&t),
        ..fast()
    });
    assert_eq!(s.wraps, 11);
    assert!(calibration(&s).is_ok());
}

/// Even reads of 39, 40 and 41 us: the survey is used up to
/// MAX_FLOOR_US (40) and refused above it.
#[test]
fn the_floor_limit_is_40_us() {
    assert_eq!(MAX_FLOOR_US, 40);
    let at = |inner: u64| {
        let s = Sim::new(0).survey(|_, _| Shape {
            inner,
            extra: inner - us(17.0),
            fail: false,
        });
        assert_eq!(s.rejected, 0);
        calibration(&s)
    };
    let ok = at(us(39.0)).unwrap();
    assert_eq!(ok.inner_limit, (39 + 8) * CPU_MHZ);
    assert!((line_us(&ok) - LINE_US).abs() < 0.003 * LINE_US);
    assert!(at(us(40.0)).is_ok());
    assert_eq!(at(us(40.0) + 1), Err(Refused::SlowReads));
    assert_eq!(at(us(41.0)), Err(Refused::SlowReads));
}

/// Before any survey: the default limit.
#[test]
fn the_default_limit_is_25_us() {
    assert_eq!(DEFAULT_MAX_INNER_US, 25);
    assert_eq!(Survey::new().fastest_read_us(), None);
    assert_eq!(calibration(&Survey::new()), Err(Refused::NoFit));
}

/// An anchor try as the firmware makes it: two reads of the given
/// lengths on a panel calibrated by a survey of fast reads, at a time
/// where the counter is well inside its range.
fn anchor_with(
    sim: &Sim,
    c: &Calibration,
    first: Shape,
    second: Shape,
) -> (Result<Anchor, usize>, u64) {
    // The first read's answer is the throwaway read's line; the second's
    // is the first's.
    let _ = sim.read(fast());
    let (first_out, latch) = sim.read(first);
    let (second_out, _) = sim.read(second);
    let (_, la, lb) = first_out.unwrap();
    (
        anchor_try((la, lb), second_out.unwrap(), c.inner_limit, &c.counter),
        latch,
    )
}

/// The second core reads 5 us slower, or 5 us faster, than the survey on
/// the first core did: its anchors are still used (the limit is 8 us
/// over the survey's fastest read), stamped half a read after the first
/// read's start.
#[test]
fn the_second_core_reading_5_us_slower_or_faster_still_anchors() {
    let c = calibration(&Sim::new(0).survey(|_, _| fast())).unwrap();
    for delta in [-5.0, 0.0, 5.0] {
        let shape = Shape {
            inner: us(17.0 + delta),
            ..fast()
        };
        let sim = Sim::new(0);
        sim.now.set(us(5000.0));
        let (got, latch) = anchor_with(&sim, &c, shape, shape);
        let a = got.unwrap();
        // Stamp: the first read's start plus half the second's length.
        let la = latch - LATCH;
        assert_eq!(u64::from(a.cycles), la + shape.inner / 2);
        assert_eq!(a.line, sim.value(latch));
        // The model then puts the panel within one line of the truth.
        let off = sim.position(u64::from(a.cycles)) % LINES - f64::from(a.line);
        assert!((-1.2..=0.2).contains(&off), "delta {delta}: {off}");
    }

    // 9 us slower is over the limit: every try misses as slow.
    let slow = Shape {
        inner: us(26.0),
        ..fast()
    };
    let sim = Sim::new(0);
    sim.now.set(us(5000.0));
    assert_eq!(anchor_with(&sim, &c, slow, slow).0, Err(MISS_SLOW));
    // Either read alone over the limit is enough.
    let sim = Sim::new(0);
    sim.now.set(us(5000.0));
    assert_eq!(anchor_with(&sim, &c, slow, fast()).0, Err(MISS_SLOW));
    let sim = Sim::new(0);
    sim.now.set(us(5000.0));
    assert_eq!(anchor_with(&sim, &c, fast(), slow).0, Err(MISS_SLOW));
}

/// The two reads of an anchor straddle the counter's wrap.
#[test]
fn an_anchor_across_a_counter_wrap() {
    let c = calibration(&Sim::new(0).survey(|_, _| fast())).unwrap();
    let t = us(5000.0);
    let sim = Sim::new((1u64 << 32) - t - us(20.0));
    sim.now.set(t);
    let (got, latch) = anchor_with(&sim, &c, fast(), fast());
    let a = got.unwrap();
    assert_eq!(a.cycles, sim.counter(latch - LATCH + us(17.0) / 2));
}

/// The order of the checks, and the counter's untrusted values.
#[test]
fn anchor_checks_in_order() {
    let counter = Counter {
        line_min: 1,
        line_max: 333,
        held_line: 333,
    };
    let limit = 25 * CPU_MHZ;
    let ok = 17 * CPU_MHZ;
    let slow = 26 * CPU_MHZ;
    let at = |first: u32, raw: u16, second: u32| {
        anchor_try(
            (100, 100 + first),
            (raw, 9000, 9000 + second),
            limit,
            &counter,
        )
    };
    assert_eq!(
        at(ok, 100, ok),
        Ok(Anchor {
            cycles: 100 + ok / 2,
            line: 100,
        })
    );
    // Too slow wins over bad bits; bad bits over the range.
    assert_eq!(at(slow, 0x400 | 100, ok), Err(MISS_SLOW));
    assert_eq!(at(ok, 0x400 | 2, ok), Err(MISS_BITS));
    // The ends of the range and the held value.
    assert_eq!(at(ok, 2, ok), Err(MISS_RANGE));
    assert_eq!(at(ok, 3, ok).map(|a| a.line), Ok(3));
    assert_eq!(at(ok, 331, ok).map(|a| a.line), Ok(331));
    assert_eq!(at(ok, 332, ok), Err(MISS_RANGE));
    let held_inside = Counter {
        held_line: 200,
        ..counter
    };
    assert!(!held_inside.trusted(199));
    assert!(!held_inside.trusted(201));
    assert!(held_inside.trusted(198));
    assert!(held_inside.trusted(202));
    // The limit itself still passes.
    assert!(at(limit, 100, limit).is_ok());
}
