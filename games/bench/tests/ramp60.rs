//! CITY's ramp at 60: real-speed runs spread over the street loop
//! (`city::ramp60_tick`), and the "near panel pace" label
//! (`ramp::SCREEN_MAX_US`; a label only, the step counts as over). The model parts use `common/pace.rs`
//! (ESTIMATED: a model of the board's send, not the board). Run with
//! `--nocapture` for the tables.

mod common;
#[path = "common/pace.rs"]
mod pace;

use bench::app::Mode;
use bench::city::{
    CYCLE, RAMP_CALLS, RAMP_STRIDE, SEG_CALLS, SEG_GAP, SEG_LEAD, SEG_MEASURED, SEG_WARM, SEGMENTS,
    STEP60_CALLS, ramp60_tick,
};
use bench::load::{Kind, TESTS, Work};
use bench::ramp::{
    PERIOD_30, PERIOD_60, Ramp, SCREEN_MAX_US, SWEEP_US, Step, WARM, late_limit, load_of, period,
};
use common::{Driver, save_frame, shots};
use game_kit::{Frame, H, W, button};
use pace::{Cost, Pace, REFRESH_US, box_of, city_cost};

fn index(kind: Kind) -> usize {
    TESTS.iter().position(|t| t.kind == kind).unwrap()
}

/// The game's own rows (below the band), hashed.
fn scene_hash(f: &Frame) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for &b in &f.0[20 * W..H * W] {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01B3);
    }
    h
}

#[test]
fn a_sixty_step_shows_sixteen_real_speed_runs_over_the_whole_loop() {
    assert_eq!(STEP60_CALLS, 196);
    assert_eq!(SEG_LEAD, WARM);
    let calls: Vec<(u32, bool)> = (0..STEP60_CALLS).map(ramp60_tick).collect();
    // The lead: 4 calls not counted, just before the first run.
    for (j, &(t, counted)) in calls.iter().take(SEG_LEAD as usize).enumerate() {
        assert!(!counted);
        assert_eq!(t, CYCLE - SEG_LEAD + j as u32);
    }
    let counted: Vec<u32> = calls.iter().filter(|c| c.1).map(|c| c.0).collect();
    assert_eq!(counted.len() as u32, RAMP_CALLS);
    for s in 0..SEGMENTS {
        let run = &calls[(SEG_LEAD + s * SEG_CALLS) as usize..][..SEG_CALLS as usize];
        for (c, &(t, n)) in run.iter().enumerate() {
            // One tick a call: the speed of the fixed test at 60.
            assert_eq!(t, s * SEG_GAP + c as u32);
            assert_eq!(n, c as u32 >= SEG_WARM);
        }
    }
    // Every sixteenth of the loop has SEG_MEASURED counted pictures.
    for s in 0..SEGMENTS {
        let n = counted
            .iter()
            .filter(|&&t| t / SEG_GAP == s && t % SEG_GAP >= SEG_WARM)
            .count();
        assert_eq!(n as u32, SEG_MEASURED);
    }
    // A step at 60 is 196 calls; 196 = 100 + 96, so the ticks of the 30
    // half (16 a call, a cycle every 96 calls) are where they were with
    // the old 100-call steps.
    assert!((STEP60_CALLS - (WARM + RAMP_CALLS)).is_multiple_of(RAMP_CALLS));
    assert_eq!(RAMP_CALLS * RAMP_STRIDE, CYCLE);
}

/// The CITY ramp at 60, its first step, with the given times between
/// calls: (scene hash, counts) per call.
fn first_step(delta: impl Fn(u32) -> u32) -> Vec<(u64, bench::load::Counts)> {
    let mut d = Driver::new();
    d.pick(index(Kind::City), 1);
    d.call(button::A);
    let mut out = Vec::new();
    for j in 0..STEP60_CALLS {
        assert_eq!(d.app.mode, Mode::Ramp);
        assert!(d.app.ramp.rate60);
        d.call_full(0, delta(j), false);
        out.push((scene_hash(&d.frame), d.app.last));
    }
    out
}

#[test]
fn the_sixty_pictures_do_not_depend_on_how_late_the_calls_come() {
    let on_time = first_step(|_| PERIOD_60);
    let late = first_step(|j| if j % 3 == 0 { 17_018 } else { 30_000 });
    assert_eq!(on_time, late);
    // And they are the pictures of `ramp60_tick` at the step's load.
    let load = TESTS[index(Kind::City)].base;
    let mut w = Work::new();
    let mut f = Box::new(Frame::new());
    for (j, got) in on_time.iter().enumerate() {
        let (t, _) = ramp60_tick(j as u32);
        let c = w.run(&mut f, Kind::City, load, t, j == 0);
        assert_eq!(c, got.1, "call {j}");
    }
}

/// A ramp fed `delta` for one whole step at `period`.
fn one_step(rule: bool, delta: impl Fn(u32) -> u32) -> (Ramp, Step) {
    // Base 16: a fit moves the search to k 4, an over step to k 2.
    let mut r = Ramp::new(16, 28);
    r.screen_rule = rule;
    let mut j = 0;
    loop {
        if let Some(s) = r.frame_n(delta(j), 0, true, RAMP_CALLS) {
            return (r, s);
        }
        j += 1;
    }
}

#[test]
fn near_panel_pace_is_a_label_and_the_step_still_counts_as_over() {
    assert_eq!(SWEEP_US, 17_018);
    assert_eq!(SCREEN_MAX_US, 17_283);
    assert!(SCREEN_MAX_US < late_limit(PERIOD_60));
    // Held to the panel's sweep, none late: labelled, but not a fit, and
    // the search goes on exactly as for a plain over step.
    let (r, s) = one_step(true, |_| 17_018);
    assert!(!s.fits && s.screen, "{s:?}");
    let (r_plain, s_plain) = one_step(false, |_| 17_018);
    assert!(!s_plain.fits && !s_plain.screen);
    assert_eq!(r.load(), r_plain.load());
    assert_eq!(r.k, r_plain.k);
    let (r_fit, s_fit) = one_step(true, |_| PERIOD_60);
    assert!(s_fit.fits && !s_fit.screen);
    assert_eq!(
        (r.k, r_fit.k),
        (2, 4),
        "a labelled step must not move the search like a fit"
    );
    // A steady 17,100 us (a drawing overload looks the same): labelled,
    // still over. BENCH cannot tell the two; the words say so.
    let (_, s) = one_step(true, |_| 17_100);
    assert!(!s.fits && s.screen);
    // Up to SCREEN_MAX_US labelled; above it plain over.
    assert!(one_step(true, |_| SCREEN_MAX_US).1.screen);
    let (_, s) = one_step(true, |_| SCREEN_MAX_US + 1);
    assert!(!s.fits && !s.screen);
    // One dropped sweep (one call of two sweeps, late): still labelled,
    // as a fit may have late calls too (board, round 3: avg 17,165 max
    // 34,147 late 1).
    let (_, s) = one_step(true, |j| if j == 40 { 34_139 } else { 17_018 });
    assert!(!s.fits && s.screen && s.late == 1, "{s:?}");
    // More late calls than a fit may have (7 of 96): plain over.
    let (_, s) = one_step(true, |j| if j % 13 == 5 { 19_000 } else { 16_990 });
    assert!(s.late > 6 && !s.fits && !s.screen, "{s:?}");
    // The other ramps (no rule): never labelled.
    let (_, s) = one_step(false, |_| 17_018);
    assert!(!s.fits && !s.screen);
    // Only CITY's ramp has the rule.
    for (i, t) in TESTS.iter().enumerate() {
        if t.kmax == 0 {
            continue;
        }
        let mut d = Driver::new();
        d.pick(i, 0);
        d.call(button::A);
        assert_eq!(d.app.ramp.screen_rule, t.kind == Kind::City, "{}", t.name);
    }
}

/// Round 3's CITY step lines at 60 on the board (board3/ramp-city.txt and
/// probe-ramp60-*.txt): (load, avg, max, late) of 96 calls, MEASURED ON
/// THE BOARD in round 3. Loads 2 to 54 were held near the sweep, 76 and
/// 108 were real overloads.
const BOARD3_60: [(u32, u32, u32, u32); 20] = [
    (2, 17_041, 17_818, 0),
    (2, 17_045, 17_701, 0),
    (3, 17_044, 17_659, 0),
    (3, 17_047, 17_760, 0),
    (4, 17_044, 17_620, 0),
    (4, 17_044, 20_550, 1),
    (6, 17_042, 17_676, 0),
    (6, 17_165, 34_147, 1),
    (9, 16_942, 17_740, 0),
    (9, 17_047, 17_711, 0),
    (13, 17_043, 17_582, 0),
    (13, 17_045, 17_682, 0),
    (19, 17_041, 17_715, 0),
    (19, 17_046, 20_028, 1),
    (27, 17_040, 17_358, 0),
    (27, 17_050, 20_228, 1),
    (38, 17_038, 17_566, 0),
    (38, 17_069, 34_139, 1),
    (54, 17_052, 19_096, 3),
    (54, 17_060, 19_105, 2),
];

#[test]
fn every_held_step_of_round_three_gets_the_label() {
    use bench::ramp::Meter;
    let held = |&(_, avg, worst, late): &(u32, u32, u32, u32)| Meter {
        frames: 96,
        sum: avg * 96,
        worst,
        late,
        extra: 0,
        skip: 0,
    };
    for l in BOARD3_60.iter() {
        let m = held(l);
        assert!(!m.fits(PERIOD_60), "{l:?}");
        assert!(m.screen_pace(PERIOD_60), "{l:?}");
    }
    for (load, avg, worst) in [
        (76, 23_007, 24_798),
        (76, 23_027, 24_811),
        (108, 31_162, 32_584),
    ] {
        let m = held(&(load, avg, worst, 96));
        assert!(!m.screen_pace(PERIOD_60), "{load}");
    }
    // The steps that fit on the board are not labelled.
    for (avg, worst, late) in [(16_847, 17_557, 0), (16_924, 23_636, 1)] {
        let m = held(&(1, avg, worst, late));
        assert!(m.fits(PERIOD_60) && !m.screen_pace(PERIOD_60));
    }
}

/// The whole CITY ramp with a made-up console: `delta(load, rate60)`.
/// Returns the driver after the result's extra line.
fn made_up_ramp(delta: impl Fn(u32, bool) -> u32) -> (Driver, bool) {
    let mut d = Driver::new();
    d.pick(index(Kind::City), 1);
    d.call(button::A);
    let mut n = 0;
    let mut band_said = false;
    let mut n_after = 0;
    while (d.app.mode == Mode::Ramp || n_after < 2) && n < 100_000 {
        if d.app.mode == Mode::Result {
            n_after += 1;
        }
        let dt = if d.app.mode == Mode::Result {
            period(d.rate60)
        } else {
            delta(d.app.ramp.load(), d.rate60)
        };
        d.call_full(0, dt, false);
        if !band_said
            && d.app.band_lines(d.rate60)[2]
                .as_str()
                .contains("OVER, NEAR PANEL")
        {
            band_said = true;
            save_frame(&shots().join("ramp60-band-near-panel.png"), &d.frame, 3);
            // The band's line fits the screen's width.
            let mut f = Frame::new();
            let end = d.app.band_lines(d.rate60)[2].at(&mut f, 2, 0, 1, 1);
            assert!(end <= W as i16, "{end}");
        }
        n += 1;
    }
    assert_eq!(d.app.mode, Mode::Result);
    (d, band_said)
}

#[test]
fn the_app_says_near_panel_pace_in_plain_words_and_fit_stays_strict() {
    // A made-up console: at 60 the calls keep to the frame up to load 16,
    // to the panel's sweep from 17 to 54, and are late above; at 30 they
    // fit up to 108.
    let (d, band_said) = made_up_ramp(|load, rate60| {
        if rate60 {
            match load {
                0..=16 => PERIOD_60,
                17..=54 => 17_018,
                _ => 25_000,
            }
        } else if load <= 108 {
            PERIOD_30
        } else {
            40_000
        }
    });
    save_frame(&shots().join("ramp60-result-near-panel.png"), &d.frame, 3);
    let r = d.app.ramp.results[0];
    // FIT is strict: the largest load that kept the full 60.
    assert_eq!(r.fit, 16);
    assert!(r.screen > 0);
    assert_eq!(r.screen_us, 17_018);
    assert_eq!(r.screen_load, 19);
    assert_eq!(d.app.ramp.results[1].fit, 108);
    assert!(band_said);
    assert!(d.logs.iter().any(|l| l.ends_with(" over, near panel pace")));
    let line = d
        .logs
        .iter()
        .find(|l| l.starts_with("bench: result CITY 60: "))
        .expect("the near-panel-pace result line");
    println!("{line}");
    assert!(
        line.ends_with("near panel pace (counted over), 17018 us; first at load 19"),
        "{line}"
    );
    // The console cuts a game's log line at 96 letters: this one stays
    // under it with the largest numbers it can show.
    let worst = "bench: result CITY 60: 255 near panel pace (counted over), 17283 us; first at load 4294967295";
    assert!(worst.len() <= 96, "{}", worst.len());
    assert!(line.len() <= worst.len());
    // A labelled step line with CITY's largest load, a stall of 999,999 us
    // and n of 4 digits stays under it too.
    let step = "bench: ramp 60 k 28 load 128: avg 17283 max 999999 late 6 of 96 n 9999 over, near panel pace";
    assert!(step.len() <= 96, "{}", step.len());
    // The result screen's three lines fit the width with their largest
    // numbers.
    let mut f = Frame::new();
    for text in [
        "60: 255 NEAR PANEL PACE, 17283 US",
        "PANEL OR DRAWING? COUNTED OVER",
        "FIRST AT LOAD 4294967295",
        // CITY's largest load is 128.
        "LAST 128: 17283 US OVER, NEAR PANEL",
    ] {
        let mut t = bench::text::Txt::new();
        t.s(text);
        let end = t.at(&mut f, 2, 0, 1, 1);
        assert!(end <= W as i16, "{text}: {end}");
    }
    // On time everywhere: no near-panel words at all.
    let mut d = Driver::new();
    d.pick(index(Kind::City), 1);
    d.call(button::A);
    while d.app.mode == Mode::Ramp {
        d.call(0);
    }
    d.idle(3);
    assert!(!d.logs.iter().any(|l| l.contains("near panel")));
    assert_eq!(d.app.ramp.results[0].screen, 0);
    assert_eq!(d.app.ramp.results[0].screen_load, 0);
}

#[test]
fn a_label_never_lets_the_search_jump_past_untried_loads() {
    // The check's case P3 (from a check report kept outside the
    // repository): the full 60 up to
    // load 54, 17,050 us (near panel pace) at 55 to 64, late above. The
    // strict answer is 54, and the ramp must find it.
    let (d, _) = made_up_ramp(|load, rate60| {
        if rate60 {
            match load {
                0..=54 => PERIOD_60,
                55..=64 => 17_050,
                _ => 25_000,
            }
        } else {
            PERIOD_30
        }
    });
    let r = d.app.ramp.results[0];
    assert_eq!(r.fit, 54, "{r:?}");
    assert_eq!(r.screen_load, 64);
    assert!(r.screen >= 1);
}
/// One run of pictures through the send model: the average time between
/// the counted calls (us), full-width boxes, boxes sent.
fn model_run(
    kind: Kind,
    load: u32,
    pictures: &[(u32, bool)],
    draw: impl Fn(u32) -> f64,
    rate60: bool,
) -> (f64, u32, u32) {
    let mut w = Work::new();
    let mut shown = Box::new(Frame::new());
    let mut f = Box::new(Frame::new());
    let mut areas = console_video::Areas::new();
    let mut p = Pace::new();
    let (mut sum, mut n) = (0u64, 0u32);
    for (j, &(t, counted)) in pictures.iter().enumerate() {
        let (start, delta) = p.begin(period(rate60));
        if counted && j > 0 {
            sum += u64::from(delta);
            n += 1;
        }
        let c = w.run(&mut f, kind, load, t, j == 0);
        let b = box_of(&shown, &f, &mut areas);
        p.end(start, draw(c.tris), b, true);
        shown.0.copy_from_slice(&f.0);
    }
    (sum as f64 / f64::from(n.max(1)), p.full, p.sent)
}

/// Three schedules of CITY pictures, three times over: the old ramp's
/// (16 ticks a call, a step every 100 calls), the new one's (196-call
/// steps), and the fixed test's at 60 (one tick a call).
fn schedules() -> [(&'static str, Vec<(u32, bool)>); 3] {
    let old: Vec<(u32, bool)> = (0..300)
        .map(|i| ((i + 1) * RAMP_STRIDE % CYCLE, i % 100 >= WARM))
        .collect();
    let new: Vec<(u32, bool)> = (0..3 * STEP60_CALLS)
        .map(|i| ramp60_tick(i % STEP60_CALLS))
        .collect();
    let real: Vec<(u32, bool)> = (0..3 * STEP60_CALLS)
        .map(|i| (i % CYCLE, i >= 20))
        .collect();
    [("old ramp", old), ("new ramp", new), ("real speed", real)]
}

#[test]
fn model_the_old_ramp_is_held_to_the_sweep_and_the_new_one_is_not() {
    let cost: Cost = city_cost();
    let fit_limit = f64::from(PERIOD_60 + PERIOD_60 / 64);
    println!(
        "model (ESTIMATED): CITY at 60, draw {:.0} us + {:.0} us a building + {:.2} us a triangle; panel sweep {:.0} us",
        cost.fixed_us, cost.load_us, cost.tri_us, REFRESH_US
    );
    println!("load  old ramp us (full/sent)   new ramp us (full/sent)   real speed us (full/sent)");
    let [old, new, real] = schedules();
    for load in [2, 4, 8, 16, 27, 38, 45, 54, 64] {
        let o = model_run(Kind::City, load, &old.1, |t| cost.us(load, t), true);
        let n = model_run(Kind::City, load, &new.1, |t| cost.us(load, t), true);
        let r = model_run(Kind::City, load, &real.1, |t| cost.us(load, t), true);
        println!(
            "{load:>4}  {:>8.0} ({:>3}/{:>3})        {:>8.0} ({:>3}/{:>3})        {:>8.0} ({:>3}/{:>3})",
            o.0, o.1, o.2, n.0, n.1, n.2, r.0, r.1, r.2
        );
        if load <= 38 {
            // The old pictures: held to the sweep, judged over (as on
            // the board: 17,038 to 17,052 us).
            assert!(o.0 > fit_limit && o.0 < REFRESH_US + 30.0, "{load}: {o:?}");
            // The new ones and the fixed test's: on the frame.
            assert!(n.0 <= fit_limit, "{load}: {n:?}");
            assert!(r.0 <= fit_limit, "{load}: {r:?}");
        }
        // Where the fixed test's pictures keep to the frame, the new step
        // is within 1 % of their pace (above, both are over).
        if r.0 <= fit_limit {
            assert!((n.0 - r.0).abs() <= r.0 / 100.0, "{load}: {n:?} {r:?}");
        } else {
            assert!(n.0 > fit_limit, "{load}: {n:?} {r:?}");
        }
    }
}

#[test]
fn model_tri_spin_and_scene_ramps_are_not_held_to_the_sweep() {
    // Their ramps move 2 ticks a call at both rates (twice the real speed
    // at 60). With a small draw time the send alone decides the pace. Not
    // CITY's flaw: their ramp pictures are never held to the sweep more
    // than the same pictures at real speed (the big scenes are near the
    // sweep both ways: they change across the whole width).
    println!("model (ESTIMATED): ramp pictures (2 ticks a call) at 60, draw 4,000 us");
    println!("test          load   ramp us (full/sent)   real speed us (full/sent)");
    for kind in [Kind::Spin, Kind::Scene(0), Kind::Scene(1), Kind::Scene(2)] {
        let Some(i) = TESTS.iter().position(|t| t.kind == kind) else {
            continue;
        };
        let t = &TESTS[i];
        if t.kmax == 0 {
            continue;
        }
        for &load in t.loads.iter() {
            let ramp: Vec<(u32, bool)> = (0..400u32).map(|j| ((j + 1) * 2, j >= WARM)).collect();
            let real: Vec<(u32, bool)> = (0..400u32).map(|j| (j + 1, j >= WARM)).collect();
            let a = model_run(t.kind, load, &ramp, |_| 4000.0, true);
            let b = model_run(t.kind, load, &real, |_| 4000.0, true);
            println!(
                "{:<12} {load:>5}   {:>7.0} ({:>3}/{:>3})     {:>7.0} ({:>3}/{:>3})",
                t.name, a.0, a.1, a.2, b.0, b.1, b.2
            );
            // Within 100 us of the real-speed pace (the worst: SCENE 96
            // at 64, both near the sweep, 400 and 398 full-width boxes).
            assert!(a.0 <= b.0 + 100.0, "{}: {a:?} {b:?}", t.name);
            // Never the CITY case: the ramp held to the sweep while the
            // same pictures at real speed keep cleanly to the frame.
            assert!(!(a.0 > REFRESH_US - 60.0 && b.0 <= f64::from(PERIOD_60) + 10.0));
        }
    }
    // ISLAND has no ramp.
    assert_eq!(TESTS[index(Kind::Island)].kmax, 0);
}

/// The whole CITY ramp through the send model, the draw time moved by
/// `off` us. Returns the driver at the result.
fn model_app_ramp(off: f64) -> Driver {
    let cost = city_cost();
    let mut d = Driver::new();
    d.pick(index(Kind::City), 1);
    d.call(button::A);
    let mut p = Pace::new();
    while d.app.mode == Mode::Ramp {
        let (start, delta) = p.begin(period(d.rate60));
        let shown = Box::new(d.frame.as_ref().clone());
        let load = d.app.ramp.load();
        d.call_full(0, delta, false);
        let mut areas = console_video::Areas::new();
        let b = box_of(&shown, &d.frame, &mut areas);
        p.end(start, cost.us(load, d.app.last.tris) + off, b, true);
    }
    d.idle(3);
    d
}

#[test]
fn model_the_new_ramp_finds_what_the_fixed_test_holds() {
    // The whole app through the model: the 60 result is the largest
    // ladder load whose real-speed pictures keep to the frame.
    let cost = city_cost();
    let d = model_app_ramp(0.0);
    let [r60, r30] = d.app.ramp.results;
    println!(
        "model: CITY ramp 60 fit {} (near panel pace {}), 30 fit {}",
        r60.fit, r60.screen, r30.fit
    );
    // Real speed in the same model: load 54 keeps to the frame, 64 not.
    let [_, _, real] = schedules();
    let fit_limit = f64::from(PERIOD_60 + PERIOD_60 / 64);
    let ok = |load| model_run(Kind::City, load, &real.1, |t| cost.us(load, t), true).0 <= fit_limit;
    let next = (0..=TESTS[index(Kind::City)].kmax)
        .map(|k| load_of(1, k))
        .find(|&l| l > r60.fit)
        .unwrap();
    println!(
        "model: real speed keeps to the frame at {} and not at {next}",
        r60.fit
    );
    assert!(ok(r60.fit) && !ok(next));
    assert_eq!(r60.screen, 0);
    assert_eq!(r30.fit, 108);
}

#[test]
fn model_a_drawing_overload_near_the_sweep_is_not_a_fit() {
    // The check's case P1 (same report): the draw time 200 to
    // 400 us longer (inside the fit's own error), so that drawing CITY 54
    // takes a little more than the frame. The step at 54 may be labelled
    // near panel pace, but FIT must stay where real speed keeps the frame.
    let cost = city_cost();
    let [_, _, real] = schedules();
    let fit_limit = f64::from(PERIOD_60 + PERIOD_60 / 64);
    for off in [200.0, 300.0, 400.0] {
        let d = model_app_ramp(off);
        let r60 = d.app.ramp.results[0];
        let real54 = model_run(Kind::City, 54, &real.1, |t| cost.us(54, t) + off, true).0;
        let real45 = model_run(Kind::City, 45, &real.1, |t| cost.us(45, t) + off, true).0;
        println!(
            "model, draw +{off} us: ramp 60 fit {} (near panel pace {}, first at {}); real speed 45 {real45:.0} us, 54 {real54:.0} us",
            r60.fit, r60.screen, r60.screen_load
        );
        assert!(real45 <= fit_limit);
        // FIT is never above what real speed keeps. At +200 real speed
        // keeps 54 by 8 us and the ramp is 16 us over (its runs start
        // with a jump): FIT 45, one ladder step low at that exact edge.
        assert_eq!(r60.fit, 45, "+{off}");
        assert!(
            !d.logs
                .iter()
                .any(|l| l.contains("load 54") && l.ends_with(" fits")),
            "+{off}"
        );
    }
}
