//! Edge cases of send_plan::beam that try to break it: no read at all,
//! every read refused, zero-length and absurd reads, a survey of no time,
//! panels outside the line range, read outages that lose or only delay a
//! refresh's wrap, and for the odd-step and uneven-wraps checks a wrong
//! answer and pauses.
//!
//! The panel here is simpler than tests/beam.rs's: a read answers with
//! the counter at its own start, 49.55 us a line, 343.4 lines a refresh
//! (or another count), held at 333 through the porch.
//!
//! A wrap is seen when a kept read shows a value more than 100 below the
//! kept read before it. Reads that fail for a short time around a wrap
//! only make it seen late; to LOSE a wrap no read may work for most of a
//! refresh (from just before the wrap until the counter is back where it
//! was, about 16 ms here).

use std::cell::Cell;

use send_plan::beam::{
    CPU_MHZ, Counter, FLOOR_READS, MAX_SURVEYS, MAX_WRAP_GAP, MIN_WRAP_GAP, Refused, Survey,
    anchor_try, calibration, survey,
};

const MHZ: u64 = CPU_MHZ as u64;
const LINE_CYCLES: f64 = 49.55 * MHZ as f64;
const LINES: f64 = 343.4;
const PERIOD: f64 = LINE_CYCLES * LINES;

fn us(n: f64) -> u64 {
    (n * MHZ as f64).round() as u64
}

fn value_of(t: u64, lines: f64) -> u16 {
    let p = (t as f64 / LINE_CYCLES) % lines;
    ((p as u16) + 1).min(333)
}

fn value(t: u64) -> u16 {
    value_of(t, LINES)
}

/// Runs a survey of 200 ms where `read(n, t)` gives read number n at
/// time t: `None` (failed) or `(answer, timed length)`; each read takes
/// its timed length plus 1.25 us.
fn run(read: impl Fn(u64, u64) -> Option<(u16, u64)>) -> Survey {
    run_ms(read, 200)
}

fn run_ms(read: impl Fn(u64, u64) -> Option<(u16, u64)>, ms: u32) -> Survey {
    let now = Cell::new(us(777.0));
    let n = Cell::new(0u64);
    survey(
        || {
            let t = now.get();
            let r = read(n.get(), t);
            n.set(n.get() + 1);
            let len = r.map_or(us(17.0), |(_, l)| l);
            now.set(t + 60 + len + 240);
            r.map(|(raw, l)| {
                let a = (t + 60) as u32;
                (raw, a, a.wrapping_add(l as u32))
            })
        },
        || now.get() as u32,
        ms,
    )
}

fn plain(_: u64, t: u64) -> Option<(u16, u64)> {
    Some((value(t), us(17.0)))
}

/// Refresh k begins at k * PERIOD (k = 1 to 11 fall into the survey).
fn wrap_at(k: u64) -> f64 {
    k as f64 * PERIOD
}

/// Whether no read works at time t because wrap k is lost: from 1 ms
/// before it until 16 ms after (the counter is then at about 323 again,
/// so the next kept read shows no drop).
fn losing(t: u64, k: u64) -> bool {
    let d = t as f64 - wrap_at(k);
    d > -(us(1000.0) as f64) && d < us(16_000.0) as f64
}

fn lines_of(s: &Survey) -> f32 {
    s.period_cycles().unwrap() / s.line_cycles().unwrap()
}

#[test]
fn a_plain_survey_is_used() {
    let s = run(plain);
    assert_eq!(s.rejected, 0);
    assert_eq!(s.wraps, 11);
    let c = calibration(&s).unwrap();
    assert!((f64::from(c.lines) - LINES).abs() < 1.0, "{}", c.lines);
    // The value steps of the counter make the fit up to about 0.3 percent
    // off in this simple panel (tests/beam.rs has the finer one).
    assert!((f64::from(c.line_cycles) / LINE_CYCLES - 1.0).abs() < 0.005);
    assert_eq!(s.fastest_read_us(), Some(17));
}

#[test]
fn a_survey_of_no_time_is_not_used() {
    let s = run_ms(plain, 0);
    assert_eq!((s.samples, s.rejected, s.wraps), (0, 0, 0));
    // The 64 floor reads still ran.
    assert_eq!(s.fastest_read_us(), Some(17));
    assert_eq!(calibration(&s), Err(Refused::NoFit));
}

#[test]
fn no_read_works_at_all() {
    let s = run(|_, _| None);
    assert_eq!(s.samples, 0);
    assert!(s.rejected > 10_000);
    assert_eq!(s.rejected, s.rejected_error);
    assert_eq!(s.fastest_read_us(), None);
    assert_eq!(s.inner_limit(), u32::MAX);
    assert_eq!(s.read_cycles(), 0);
    assert_eq!(s.spread, [0; 5]);
    assert_eq!(calibration(&s), Err(Refused::NoFit));
}

#[test]
fn every_read_has_bits_outside_the_counter() {
    let s = run(|_, t| Some((value(t) | 0x400, us(17.0))));
    assert_eq!(s.samples, 0);
    assert_eq!(s.rejected, s.rejected_bits);
    // The fastest read is still known (it only measures time).
    assert_eq!(s.fastest_read_us(), Some(17));
    assert_eq!(calibration(&s), Err(Refused::NoFit));
}

#[test]
fn every_read_is_slow_after_a_freak_zero_length_read() {
    // A zero-length read among the floor reads: the limit becomes 8 us,
    // every 17 us read is "slow", nothing is kept.
    let s = run(|n, t| Some((value(t), if n == 3 { 0 } else { us(17.0) })));
    assert_eq!(s.fastest_read_us(), Some(0));
    assert_eq!(s.inner_limit(), 8 * CPU_MHZ);
    assert_eq!(s.samples, 0);
    assert_eq!(s.rejected, s.rejected_slow);
    assert_eq!(s.spread[4], s.rejected);
    assert_eq!(calibration(&s), Err(Refused::NoFit));
}

#[test]
fn reads_that_all_take_zero_cycles_still_calibrate() {
    let s = run(|_, t| Some((value(t), 0)));
    assert_eq!(s.fastest_read_us(), Some(0));
    assert_eq!(s.rejected, 0);
    assert!(calibration(&s).is_ok());
}

#[test]
fn absurdly_long_reads_neither_panic_nor_calibrate() {
    // Every read "takes" almost 2^32 cycles: the limit saturates and no
    // arithmetic overflows; there is no time for two wraps. The 32-bit
    // time stamps of two reads almost 2^32 cycles apart look close (300
    // cycles), and the counter moved by thousands of lines between them:
    // odd steps. Since guard v3 that reason is named before NoFit.
    let s = run(|_, t| Some((value(t), u64::from(u32::MAX - 5))));
    assert_eq!(s.inner_limit(), u32::MAX);
    assert!(s.wraps < 2);
    assert!(s.odd_steps > 0);
    assert_eq!(calibration(&s), Err(Refused::OddStep));
}

#[test]
fn the_cycle_counter_wraps_inside_one_read() {
    // `survey` cuts times to 32 bits; here the counter is started so that
    // a read straddles 2^32: its length is still right (wrapping_sub).
    let start = (1u64 << 32) - us(50_000.0);
    let now = Cell::new(start);
    let s = survey(
        || {
            let t = now.get();
            now.set(t + 60 + us(17.0) + 240);
            let a = (t + 60) as u32;
            Some((value(t - start), a, a.wrapping_add(us(17.0) as u32)))
        },
        || now.get() as u32,
        200,
    );
    assert_eq!(s.rejected, 0);
    assert_eq!(s.fastest_read_us(), Some(17));
    let c = calibration(&s).unwrap();
    assert!((f64::from(c.lines) - LINES).abs() < 1.0);
}

/// Another panel (no wrong answer, so no odd step and even wraps) is
/// still refused as Lines, and the firmware does not survey again for
/// that.
#[test]
fn panels_outside_320_plus_2_to_254_lines_are_refused() {
    for (lines, ok) in [(321.5, false), (323.0, true), (570.0, true), (580.0, false)] {
        let s = run(|_, t| Some((value_of(t, lines), us(17.0))));
        assert_eq!(s.odd_steps, 0, "lines {lines}");
        let c = calibration(&s);
        assert_eq!(c.is_ok(), ok, "lines {lines}: {c:?}");
        if !ok {
            assert_eq!(c, Err(Refused::Lines), "lines {lines}");
        }
    }
    assert!(!Refused::Lines.retry());
}

/// Which refusals the firmware surveys again for (boards/cyd/src/main.rs,
/// at most MAX_SURVEYS surveys in all): only the two that a wrong answer
/// of the 45h read gives.
#[test]
fn only_a_wrong_answer_is_surveyed_again() {
    for (why, again) in [
        (Refused::NoFit, false),
        (Refused::Lines, false),
        (Refused::SlowReads, false),
        (Refused::MissedWrap, false),
        (Refused::OddStep, true),
        (Refused::UnevenWraps, true),
    ] {
        assert_eq!(why.retry(), again, "{why:?}");
    }
    assert_eq!(MAX_SURVEYS, 3);
}

/// Runs `run` with one read answering `wrong(v)`: the first read in
/// refresh 5 at or after the counter's `line` (minus half a line) that
/// shows `line`.
fn one_wrong(line: u16, wrong: impl Fn(u16) -> u16, read_us: f64) -> Survey {
    let at = (wrap_at(5) + (f64::from(line) - 1.5) * LINE_CYCLES) as u64;
    let done = Cell::new(false);
    let s = run(|_, t| {
        let v = value(t);
        if !done.get() && t >= at && v == line {
            done.set(true);
            return Some((wrong(v), us(read_us)));
        }
        Some((v, us(read_us)))
    });
    assert!(done.get(), "line {line}");
    s
}

/// What boot 169 most likely saw (BOARD: one survey, odd_steps=1,
/// refused as lines_out_of_range, no new survey, the defaults kept; the
/// survey's own figures were not printed). On the PC: one read where
/// the counter shows 256 to 265 answers with bit 8 cleared (0 to 9).
/// That drop is taken for a wrap that lands at the bottom, so it is no
/// odd step; the step back up to the true line is (from 20 or less to
/// above 140 is no porch jump). One more wrap makes the average refresh
/// one in eleven short: about 312 lines here, below 322. Before guard
/// v3 the line count was checked first, so this survey was refused as
/// Lines and not done again; now the odd step is named first and the
/// firmware surveys again.
#[test]
fn a_wrong_answer_that_pushes_the_lines_out_is_refused_as_an_odd_step() {
    for line in 256..=265u16 {
        let s = one_wrong(line, |v| v & !256, 17.0);
        let w = line - 256;
        assert_eq!(s.rejected, 0);
        assert_eq!(s.wraps, 12, "line {line}");
        assert_eq!(s.odd_steps, 1, "line {line}");
        assert_eq!(s.first_odd.map(|x| x.0), Some(w), "line {line}");
        let lines = lines_of(&s);
        assert!((300.0..322.0).contains(&lines), "line {line}: {lines}");
        let c = calibration(&s);
        assert_eq!(c, Err(Refused::OddStep), "line {line}");
        assert!(Refused::OddStep.retry());
    }
}

/// The same with no odd step at all: the counter shows 128 to 137 and
/// one read answers with bit 7 cleared (0 to 9). The drop is taken for
/// a wrap at the bottom and the step back up looks like the porch jump
/// (to 140 or less), so no step is odd; but the false wrap comes less
/// than half a refresh after the true one. Lines about 312: refused as
/// Lines before guard v3, as uneven wraps now (and surveyed again).
#[test]
fn a_false_wrap_with_no_odd_step_is_refused_as_uneven_wraps() {
    for line in 128..=137u16 {
        let s = one_wrong(line, |v| v & !128, 17.0);
        assert_eq!(s.wraps, 12, "line {line}");
        assert_eq!(s.odd_steps, 0, "line {line}");
        let lines = lines_of(&s);
        assert!((300.0..322.0).contains(&lines), "line {line}: {lines}");
        assert_eq!(calibration(&s), Err(Refused::UnevenWraps), "line {line}");
    }
}

/// The odd step is named before every other reason: here the reads are
/// all 41 us (over MAX_FLOOR_US, slow reads) and one answer is wrong.
/// The firmware surveys again; the next survey (no wrong answer) is
/// refused as slow reads and not done again.
#[test]
fn an_odd_step_is_named_before_slow_reads() {
    let s = one_wrong(223, |v| v >> 1, 41.0);
    assert_eq!(s.fastest_read_us(), Some(41));
    assert_eq!(s.odd_steps, 2);
    assert_eq!(calibration(&s), Err(Refused::OddStep));
    let s = run(|_, t| Some((value(t), us(41.0))));
    assert_eq!(calibration(&s), Err(Refused::SlowReads));
}

#[test]
fn one_lost_wrap_inside_the_survey_is_refused() {
    // Losing the first or the last wrap only shortens the span: the
    // period stays right and the survey is used. Losing one in between
    // made the period one refresh in nine too long (lines about 382,
    // which passed the old checks); it is refused now.
    for k in 1..=11u64 {
        let s = run(|_, t| (!losing(t, k)).then(|| (value(t), us(17.0))));
        assert_eq!(s.wraps, 10, "k {k}");
        if k == 1 || k == 11 {
            let c = calibration(&s).unwrap();
            assert!((f64::from(c.lines) - LINES).abs() < 1.0, "k {k}");
        } else {
            let lines = lines_of(&s);
            assert!((370.0..400.0).contains(&lines), "k {k}: {lines}");
            assert_eq!(calibration(&s), Err(Refused::MissedWrap), "k {k}");
        }
    }
}

#[test]
fn a_wrap_seen_late_is_not_refused_and_the_period_stays_right() {
    // No read from 1 ms before wrap 5 to 5 ms after it: wrap 5 is seen
    // 5 ms late, the gaps around it are a refresh plus and minus 5 ms,
    // the average refresh (first to last wrap) is unchanged.
    let s = run(|_, t| {
        let d = t as f64 - wrap_at(5);
        (!(d > -(us(1000.0) as f64) && d < us(5000.0) as f64)).then(|| (value(t), us(17.0)))
    });
    assert_eq!(s.wraps, 11);
    let c = calibration(&s).unwrap();
    assert!((f64::from(c.lines) - LINES).abs() < 1.0, "{}", c.lines);
    assert!(s.max_wrap_gap() as f64 > PERIOD + us(4900.0) as f64);
}

#[test]
fn two_lost_wraps_in_a_row_are_refused() {
    let s = run(|_, t| (!(losing(t, 4) || losing(t, 5))).then(|| (value(t), us(17.0))));
    assert_eq!(s.wraps, 9);
    assert_eq!(calibration(&s), Err(Refused::MissedWrap));
}

#[test]
fn every_other_wrap_lost_is_refused_by_the_line_count() {
    // Wraps 2, 4, 6, 8 and 10 lost: every gap is two refreshes, so the
    // longest gap is the average one, but the lines (about 687) are out.
    let s =
        run(|_, t| (![2, 4, 6, 8, 10].iter().any(|&k| losing(t, k))).then(|| (value(t), us(17.0))));
    assert_eq!(s.wraps, 6);
    assert_eq!(s.odd_steps, 0);
    assert!(s.max_wrap_gap() as f32 <= MAX_WRAP_GAP * s.period_cycles().unwrap());
    assert_eq!(calibration(&s), Err(Refused::Lines));
}

/// Wraps lost in a pattern (2, 5, 8 and 11: gaps of 2, 1, 2, 1, 2, 1
/// refreshes): the average refresh is 1.5 refreshes, the longest gap only
/// 1.33 times that (under MAX_WRAP_GAP), and the lines (about 515) are
/// inside 322..=574. It needs reads to fail for about 16 ms in three of
/// the eleven refreshes of the start-up survey.
///
/// Until 2026-11-19 this survey was used with a refresh 50 percent long
/// (the KNOWN GAP test `known_gap_every_third_wrap_lost_is_still_used`).
/// Its note said that comparing the longest gap with the SHORTEST one
/// would refuse it but would also refuse a survey whose reads failed for
/// a few ms around one wrap, a trade for the owner. MIN_WRAP_GAP does not
/// make that trade: it only compares wraps seen by close reads, and a
/// wrap seen after reads failed is not seen by close reads. Here wraps 3
/// and 4 are, and they are only 0.67 average refreshes apart.
/// `a_wrap_seen_late_is_not_refused_and_the_period_stays_right` and
/// `reads_failing_for_a_while_refuse_nothing_new` show the other side.
#[test]
fn every_third_wrap_lost_is_refused() {
    let s =
        run(|_, t| (![2, 5, 8, 11].iter().any(|&k| losing(t, k))).then(|| (value(t), us(17.0))));
    assert_eq!(s.wraps, 7);
    let lines = lines_of(&s);
    assert!((500.0..530.0).contains(&lines), "{lines}");
    assert_eq!(calibration(&s), Err(Refused::UnevenWraps));
    assert!((s.min_wrap_gap() as f32) < MIN_WRAP_GAP * s.period_cycles().unwrap());
}

/// What happens to one read in [`run_latched`]: a pause (us) before it,
/// a pause inside its timed part before the panel takes the line, one
/// inside it after that; or the read fails; or it answers wrong.
#[derive(Clone, Copy, Default)]
struct Hitch {
    before_us: f64,
    in_before_us: f64,
    in_after_us: f64,
    fails: bool,
    wrong: Option<fn(u16) -> u16>,
}

/// A survey of 200 ms starting at `start_us`, with the panel's own way of
/// answering: a read answers with the counter at the PREVIOUS 45h read
/// (taken 2 us into that read; a failed read takes it too). `hitch(n, t)`
/// says what happens to read n, which would start at time t; n counts
/// the floor reads too, so read FLOOR_READS is the survey's first.
fn run_latched(start_us: f64, hitch: impl Fn(u64, u64) -> Hitch) -> Survey {
    let now = Cell::new(us(start_us));
    let n = Cell::new(0u64);
    let taken = Cell::new(None::<u64>);
    survey(
        || {
            let h = hitch(n.get(), now.get());
            n.set(n.get() + 1);
            let a = now.get() + us(h.before_us) + 60;
            let at = a + us(2.0 + h.in_before_us);
            let b = a + us(17.0 + h.in_before_us + h.in_after_us);
            now.set(b + 240);
            let v = value(taken.replace(Some(at)).unwrap_or(at));
            let v = h.wrong.map_or(v, |g| g(v));
            (!h.fails).then_some((v, a as u32, b as u32))
        },
        || now.get() as u32,
        200,
    )
}

/// One wrong answer, inside the counter's ten bits: one read in refresh
/// 5 answers 111 where the counter shows 223 (one bit late). The checks
/// before the guard took it for a wrap and used the survey with lines
/// about 325 and the line time 3 percent short. One board start printed
/// figures like these (lines=325.19 line_us=47.978); that a wrong
/// answer caused them was worked out on the PC, the board's log does
/// not show the answers. The steps into and out of it are steps the
/// counter cannot make.
#[test]
fn one_wrong_answer_is_refused() {
    let at = (wrap_at(5) + 222.5 * LINE_CYCLES) as u64;
    let done = Cell::new(false);
    let s = run(|_, t| {
        let v = value(t);
        if !done.get() && t >= at && v == 223 {
            done.set(true);
            return Some((v >> 1, us(17.0)));
        }
        Some((v, us(17.0)))
    });
    assert!(done.get());
    assert_eq!(s.rejected, 0);
    assert_eq!(s.wraps, 12);
    let lines = lines_of(&s);
    assert!((320.0..330.0).contains(&lines), "{lines}");
    assert_eq!(s.odd_steps, 2);
    // For the log: the step into the wrong answer, one read apart (60
    // cycles, 17 us and 240 cycles in `run`).
    assert_eq!(s.first_odd, Some((223, 111, (60 + us(17.0) + 240) as u32)));
    assert_eq!(calibration(&s), Err(Refused::OddStep));
}

/// A pause between two reads (an interrupt, 3 ms): the read after it
/// answers the line from before it (the panel answers with the line at
/// the previous read). The pairs on both sides of the pause are not
/// close, so no step is odd.
#[test]
fn a_pause_between_reads_is_no_odd_step() {
    let s = run_latched(777.0, |n, _| Hitch {
        before_us: if n == 4000 { 3000.0 } else { 0.0 },
        ..Hitch::default()
    });
    assert_eq!(s.odd_steps, 0);
    assert_eq!(s.first_odd, None);
    assert!(calibration(&s).is_ok());
}

/// A pause of 0.6, 1 or 9 ms right before the first survey read (also
/// inside the last floor read after the panel took its line, which is
/// the same for the survey) or inside the first survey read, before or
/// after the panel takes its line, at 24 places in the refresh. The
/// first survey read answers with the line at the last floor read, from
/// before the pause, and the second read, about one read later, with the
/// line after it. The first pair of kept reads is never close, so no
/// good survey is refused. (When the first pair counted as close, 0.6 ms
/// or more refused a good survey at about half such places: MEASURED ON
/// THE PC.)
///
/// A drop right after the first kept read is no wrap, so a pause across
/// the survey's first wrap loses that wrap instead of stamping it late
/// (when it counted, the lines were off by up to 17 for 9 ms; now by at
/// most 0.73, MEASURED ON THE PC). A pause later in the survey can still
/// stamp a wrap late: an older weakness, the same with or without these
/// checks.
#[test]
fn a_pause_at_the_first_survey_read_refuses_nothing() {
    let first = u64::from(FLOOR_READS);
    let h = |before_us, in_before_us, in_after_us| Hitch {
        before_us,
        in_before_us,
        in_after_us,
        ..Hitch::default()
    };
    let mut worst = 0f64;
    for pause in [600.0, 1000.0, 9000.0] {
        for place in 0..24 {
            let start = 777.0 + f64::from(place) * 712.0;
            let hitches = [
                (first - 1, h(0.0, 0.0, pause)),
                (first, h(pause, 0.0, 0.0)),
                (first, h(0.0, pause, 0.0)),
                (first, h(0.0, 0.0, pause)),
            ];
            for (k, (at, hitch)) in hitches.into_iter().enumerate() {
                let s = run_latched(start, |n, _| if n == at { hitch } else { Hitch::default() });
                let why = format!("pause {pause} us, place {place}, kind {k}");
                assert_eq!(s.odd_steps, 0, "{why}: {:?}", s.first_odd);
                let c = calibration(&s).unwrap_or_else(|e| panic!("{why}: {e:?}"));
                worst = worst.max((f64::from(c.lines) - LINES).abs());
            }
        }
    }
    eprintln!("pause at the first survey read: all used, lines off by at most {worst:.2}");
}

/// The first survey read answers wrong (v << 1, v >> 1, v ^ 256), at 48
/// places in the refresh. Nothing can check the first answer (the first
/// pair of kept reads is never close), so a drop right after it is no
/// wrap and the survey is used with right figures. (When that drop
/// counted, v << 1 gave a false first wrap and wrong figures: 125 of
/// 232,920 surveys with one wrong answer, MEASURED ON THE PC.)
#[test]
fn a_wrong_first_answer_is_no_wrap() {
    let first = u64::from(FLOOR_READS);
    let kinds: [fn(u16) -> u16; 3] = [|v| v << 1, |v| v >> 1, |v| v ^ 256];
    for (k, g) in kinds.into_iter().enumerate() {
        for place in 0..48 {
            let start = 777.0 + f64::from(place) * 356.0;
            let s = run_latched(start, |n, _| Hitch {
                wrong: (n == first).then_some(g),
                ..Hitch::default()
            });
            let why = format!("kind {k}, place {place}");
            let c = calibration(&s).unwrap_or_else(|e| panic!("{why}: {e:?}"));
            assert!(
                (f64::from(c.lines) - LINES).abs() < 2.0,
                "{why}: {}",
                c.lines
            );
            assert!(
                (f64::from(c.line_cycles) / LINE_CYCLES - 1.0).abs() < 0.005,
                "{why}: {}",
                c.line_cycles
            );
        }
    }
}

/// Reads fail for 1 to 16 ms (each whole ms) around every wrap of the
/// survey (ending at it, centred on it, starting at it) and away from it
/// (in the middle of a refresh), with both ways of answering (this
/// file's simple one, and the panel's: the line at the previous read).
/// None of these surveys is refused for an odd step or uneven wraps:
/// each is used, or refused for a reason the checks before them gave
/// already (a 16 ms outage can lose a wrap: missed_wrap).
#[test]
fn reads_failing_for_a_while_refuse_nothing_new() {
    let (mut used, mut refused) = (0, 0);
    for ms in 1..=16u32 {
        let len = us(f64::from(ms) * 1000.0) as f64;
        for k in 1..=11u64 {
            let w = wrap_at(k);
            for from in [w - len, w - len / 2.0, w, w + (PERIOD - len) / 2.0] {
                let out = |t: u64| (from..from + len).contains(&(t as f64));
                let simple = run(|_, t| (!out(t)).then(|| (value(t), us(17.0))));
                let latched = run_latched(777.0, |_, t| Hitch {
                    fails: out(t),
                    ..Hitch::default()
                });
                for s in [simple, latched] {
                    let c = calibration(&s);
                    assert!(
                        !matches!(c, Err(Refused::OddStep | Refused::UnevenWraps)),
                        "{ms} ms from {from}: {c:?}, odd {:?}",
                        s.first_odd
                    );
                    if c.is_ok() {
                        used += 1;
                    } else {
                        refused += 1;
                    }
                }
            }
        }
    }
    eprintln!("reads failing for 1 to 16 ms: {used} used, {refused} refused by the older checks");
    assert!(used > 1000 && refused > 0, "{used} {refused}");
}

#[test]
fn sparse_reads_jitter_the_wraps_but_are_not_refused() {
    // Only one read in 100 works (one every 1.9 ms or so): the wrap's
    // time stamp can be up to that late, but no wrap is lost.
    let s = run(|n, t| (n % 100 == 0).then(|| (value(t), us(17.0))));
    assert_eq!(s.wraps, 11);
    let c = calibration(&s).unwrap();
    assert!((f64::from(c.lines) - LINES).abs() < 10.0, "{}", c.lines);
}

#[test]
fn anchor_try_at_the_extremes() {
    let counter = Counter {
        line_min: 1,
        line_max: 333,
        held_line: 333,
    };
    // Zero-length reads with a zero limit are kept.
    let ok = anchor_try((10, 10), (100, 20, 20), 0, &counter).unwrap();
    assert_eq!((ok.cycles, ok.line), (10, 100));
    // The first read straddles the counter's wrap: stamp from its start
    // plus half the second read's length.
    let a = u32::MAX - 100;
    let ok = anchor_try(
        (a, a.wrapping_add(4080)),
        (100, 5000, 9080),
        25 * CPU_MHZ,
        &counter,
    )
    .unwrap();
    assert_eq!(ok.cycles, a.wrapping_add(2040));
    // All bits set: refused as bits (index 1), not as a range.
    assert_eq!(
        anchor_try((0, 1), (u16::MAX, 2, 3), 25 * CPU_MHZ, &counter),
        Err(1)
    );
    // A second read whose end is before its start (a broken stamp) is
    // nearly 2^32 cycles long: refused as slow (index 0), no overflow.
    assert_eq!(
        anchor_try((0, 1), (100, 10, 5), 25 * CPU_MHZ, &counter),
        Err(0)
    );
    // The edges of the trusted range and the held value.
    assert_eq!(anchor_try((0, 1), (2, 2, 3), 99, &counter), Err(2));
    assert!(anchor_try((0, 1), (3, 2, 3), 99, &counter).is_ok());
    assert!(anchor_try((0, 1), (331, 2, 3), 99, &counter).is_ok());
    assert_eq!(anchor_try((0, 1), (332, 2, 3), 99, &counter), Err(2));
    let held = Counter {
        held_line: 200,
        ..counter
    };
    assert_eq!(anchor_try((0, 1), (201, 2, 3), 99, &held), Err(2));
    assert!(anchor_try((0, 1), (202, 2, 3), 99, &held).is_ok());
    let defaults = Counter {
        line_min: 1,
        line_max: 333,
        held_line: u16::MAX,
    };
    assert!(defaults.trusted(3) && defaults.trusted(331) && !defaults.trusted(332));
}
