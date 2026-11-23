//! TRI SPIN and CITY: same pictures for the same call, counts that are
//! what was painted, a city cycle without a jump, nothing drawn outside
//! the work area, traffic that never meets. Writes their pictures into
//! `.local/bench/shots/` (run with `--nocapture` for the counts table).

mod common;

use bench::city::{self, BLOCK, CAR_HL, CAR_HW, CROSS, CYCLE, LANES, LENGTH, LOTS, MAX_LOAD, ROAD};
use bench::load::{Counts, Kind, SPIN_TRI, TESTS, WORK, Work, spin_angle, spin_middle};
use bench::poly::{fill_tri, fill_tri_slow};
use bench::ramp::load_of;
use common::{Driver, check_palette, save_frame, shots};
use game_kit::{Frame, W};

fn index(kind: Kind) -> usize {
    TESTS.iter().position(|t| t.kind == kind).unwrap()
}

/// One call on a fresh picture whose band holds a marker colour.
fn one(w: &mut Work, kind: Kind, load: u32, tick: u32) -> (Box<Frame>, Counts) {
    let mut f = Box::new(Frame::new());
    f.0.fill(250);
    let c = w.run(&mut f, kind, load, tick, true);
    (f, c)
}

fn loads(kind: Kind) -> Vec<u32> {
    let t = &TESTS[index(kind)];
    let mut l = t.loads.to_vec();
    l.push(load_of(t.base, t.kmax));
    l
}

#[test]
fn the_new_tests_come_after_the_ten_old_ones() {
    let names: Vec<&str> = TESTS.iter().map(|t| t.name).collect();
    assert_eq!(
        names,
        [
            "FILL",
            "TRI 20",
            "TRI 100",
            "TRI 400",
            "VERT",
            "SCENE 12",
            "SCENE 32",
            "SCENE 96",
            "SEND ROWS",
            "SEND DOTS",
            "TRI SPIN",
            "CITY",
            "ISLAND"
        ]
    );
    assert_eq!(index(Kind::Spin), 10);
    assert_eq!(index(Kind::City), 11);
    assert_eq!(index(Kind::Island), 12);
}

#[test]
fn the_ramps_are_sane() {
    for kind in [Kind::Spin, Kind::City] {
        let t = &TESTS[index(kind)];
        let mut last = 0;
        for k in 0..=t.kmax {
            let l = load_of(t.base, k);
            assert!(l >= last, "{} k {k}: {l} after {last}", t.name);
            last = l;
        }
        assert!(
            last >= *t.loads.last().unwrap(),
            "{}: the ramp reaches the top level",
            t.name
        );
        assert!(t.loads.windows(2).all(|w| w[0] < w[1]));
    }
    let city = &TESTS[index(Kind::City)];
    assert_eq!(load_of(city.base, city.kmax), MAX_LOAD);
    assert!(*city.loads.last().unwrap() <= MAX_LOAD);
    assert_eq!(load_of(8, 40), 8192);
}

#[test]
fn spin_counts_what_the_slow_filler_paints() {
    for tick in [0, 1, 17, 63, 64, 1000, u32::MAX] {
        let mut w = Work::new();
        let (f, c) = one(&mut w, Kind::Spin, 512, tick);
        let mut g = Box::new(Frame::new());
        let mut slow = 0;
        let mut fast = 0;
        for i in 0..512u32 {
            let (s, co) = (
                bench::fixed::sin(spin_angle(i, tick)),
                bench::fixed::cos(spin_angle(i, tick)),
            );
            let m = spin_middle(i);
            let q = SPIN_TRI.map(|v| {
                [
                    m[0] + ((v[0] * co - v[1] * s) >> 14),
                    m[1] + ((v[0] * s + v[1] * co) >> 14),
                ]
            });
            // Inside the work area at every turn.
            for p in q {
                assert!(p[0] >= 0 && p[0] < 160 * 16, "{p:?}");
                assert!(p[1] >= 20 * 16 && p[1] < 120 * 16, "{p:?}");
            }
            slow += fill_tri_slow(&mut g, &WORK, &q, 1);
            fast += fill_tri(&mut g, &WORK, &q, 1);
        }
        assert_eq!(c.px, slow, "tick {tick}");
        assert_eq!(c.px, fast);
        assert_eq!(c.tris, 512);
        // About 100 pixels each.
        assert!((90 * 512..=110 * 512).contains(&c.px), "{}", c.px);
        assert!(f.0[..20 * W].iter().all(|&p| p == 250), "band untouched");
    }
}

#[test]
fn spin_turns_every_tick() {
    let mut w = Work::new();
    let (a, _) = one(&mut w, Kind::Spin, 128, 10);
    let (b, _) = one(&mut w, Kind::Spin, 128, 11);
    let (c, _) = one(&mut w, Kind::Spin, 128, 10 + 128);
    assert_ne!(a.0[..], b.0[..]);
    assert_eq!(a.0[..], c.0[..], "a turn takes 128 ticks (2.13 s)");
}

#[test]
fn the_same_call_gives_the_same_picture() {
    for kind in [Kind::Spin, Kind::City] {
        for load in loads(kind) {
            for tick in [0, 5, 333, CYCLE - 1, 12_345] {
                let mut w1 = Work::new();
                let (a, ca) = one(&mut w1, kind, load, tick);
                // Another history first: nothing carries over.
                let mut w2 = Work::new();
                let mut f = Box::new(Frame::new());
                for t in 0..3 {
                    w2.run(
                        &mut f,
                        kind,
                        load / 2 + 1,
                        tick.wrapping_add(t * 77),
                        t == 0,
                    );
                }
                f.0.fill(250);
                let cb = w2.run(&mut f, kind, load, tick, false);
                assert_eq!(ca, cb, "{kind:?} {load} {tick}");
                assert_eq!(a.0[..], f.0[..], "{kind:?} {load} {tick}");
            }
        }
    }
}

#[test]
fn nothing_is_drawn_outside_the_work_area_and_nothing_is_dropped() {
    for kind in [Kind::Spin, Kind::City] {
        for load in loads(kind) {
            let ticks: Vec<u32> = if kind == Kind::City {
                (0..CYCLE).step_by(3).chain([u32::MAX]).collect()
            } else {
                vec![0, 1, 2, 31, 32, 63, u32::MAX]
            };
            let mut w = Work::new();
            for tick in ticks {
                let (f, c) = one(&mut w, kind, load, tick);
                assert!(
                    f.0[..20 * W].iter().all(|&p| p == 250),
                    "{kind:?} {load} {tick}: band"
                );
                assert!(
                    f.0[20 * W..].iter().all(|&p| p != 250),
                    "{kind:?} {load} {tick}: work area all drawn"
                );
                let mut g = f.clone();
                g.0[..20 * W].fill(0);
                check_palette(&g, "city");
                assert_eq!(c.dropped, 0, "{kind:?} {load} {tick}");
            }
        }
    }
}

/// Pixels of the work area that are not sky or ground.
fn painted_least(f: &Frame) -> u32 {
    use bench::art::{PAVE, SKY_HI, SKY_LOW, SKY_MID};
    f.0[20 * W..]
        .iter()
        .filter(|&&p| ![SKY_HI, SKY_MID, SKY_LOW, PAVE].contains(&p))
        .count() as u32
}

#[test]
fn city_counts_are_what_was_painted() {
    for load in loads(Kind::City) {
        let mut w = Work::new();
        for tick in [0, 100, 400, 700] {
            let (f, c) = one(&mut w, Kind::City, load, tick);
            assert!(c.tris > 0 && c.objs > 0);
            // Every pixel that is not sky or bare ground was painted by
            // a triangle; most were painted once or twice.
            let seen = painted_least(&f);
            assert!(c.px >= seen, "{load} {tick}: {} < {seen}", c.px);
            assert!(c.px <= 4 * 16_000, "{load} {tick}: {}", c.px);
        }
    }
}

#[test]
fn more_buildings_draw_more() {
    let mut last = Counts::default();
    let mut w = Work::new();
    for &load in TESTS[index(Kind::City)].loads {
        let mut sum = Counts::default();
        for tick in (0..CYCLE).step_by(16) {
            let (_, c) = one(&mut w, Kind::City, load, tick);
            sum.tris += c.tris;
            sum.objs += c.objs;
        }
        assert!(sum.tris > last.tris && sum.objs > last.objs, "{load}");
        last = sum;
    }
}

#[test]
fn the_city_cycle_has_no_jump() {
    for load in loads(Kind::City) {
        let mut w = Work::new();
        for k in [0, 1, 2, 255, 383, 767] {
            let (a, ca) = one(&mut w, Kind::City, load, k);
            let (b, cb) = one(&mut w, Kind::City, load, k + CYCLE);
            assert_eq!(ca, cb, "{load} {k}");
            assert_eq!(a.0[..], b.0[..], "{load} {k}");
        }
        // The step over the cycle's end is like any other step.
        let mut prev = one(&mut w, Kind::City, load, 0).1;
        let mut most = 0i64;
        let mut steps = Vec::new();
        for t in 1..=CYCLE {
            let c = one(&mut w, Kind::City, load, t).1;
            let d = (i64::from(c.tris) - i64::from(prev.tris)).abs();
            steps.push(d);
            if t < CYCLE {
                most = most.max(d);
            }
            prev = c;
        }
        assert!(steps[CYCLE as usize - 1] <= most, "{load}");
    }
}

/// The ground plan of every car on the road at a call: x, z and half
/// sizes (z in the city, 0 to LENGTH).
fn cars(tick: u32) -> Vec<(i32, i32, i32, i32)> {
    let mut v = Vec::new();
    for (lane, l) in LANES.iter().enumerate() {
        for k in 0..8 {
            v.push((l.0, city::lane_car_z(lane, k, tick), CAR_HW, CAR_HL));
        }
    }
    for (c, cr) in CROSS.iter().enumerate() {
        for b in 0..city::BLOCKS {
            let x = city::cross_car_x(c, b, tick);
            v.push((x, b * BLOCK + cr.0, CAR_HL, CAR_HW));
        }
    }
    v
}

#[test]
fn cars_never_meet() {
    for tick in 0..CYCLE {
        let v = cars(tick);
        for i in 0..v.len() {
            for j in i + 1..v.len() {
                let (a, b) = (v[i], v[j]);
                let dz = (a.1 - b.1).rem_euclid(LENGTH);
                let dz = dz.min(LENGTH - dz);
                let apart = (a.0 - b.0).abs() >= a.2 + b.2 + 32 || dz >= a.3 + b.3 + 32;
                assert!(apart, "tick {tick}: {a:?} and {b:?}");
            }
        }
    }
}

#[test]
fn crossing_cars_cross_the_main_road() {
    let mut on_road = 0;
    for tick in 0..CYCLE {
        for c in 0..2 {
            if city::cross_car_x(c, 0, tick).abs() < ROAD {
                on_road += 1;
            }
        }
    }
    assert!(on_road > 20, "{on_road}");
}

#[test]
fn the_lots_fit() {
    assert_eq!(LOTS.len(), 160);
    for w in LOTS.windows(2) {
        assert!(w[0].far() <= w[1].far());
    }
    for l in &LOTS {
        assert!(i32::from(l.x).abs() - i32::from(l.hx) >= ROAD + 2 * 128);
        assert!(l.far() < LENGTH);
        let (v, n) = {
            let mut v = [[0i16; 3]; 64];
            let n = city::bld_corners(l, &mut v);
            (v, n)
        };
        let (faces, normals) = city::bld_model_tables(usize::from(l.bands), l.top > 0);
        assert_eq!(faces.len(), normals.len());
        for f in faces {
            assert!(f.v.iter().all(|&i| usize::from(i) < n));
        }
        for p in &v[..n] {
            let d2 = p.iter().map(|&c| i64::from(c) * i64::from(c)).sum::<i64>();
            assert!(d2 <= i64::from(l.radius) * i64::from(l.radius));
        }
        // The eye (3 m) is below every roof.
        assert!(l.h >= 4 * 128);
    }
}

fn file(name: &str) -> std::path::PathBuf {
    shots().join(name)
}

#[test]
fn pictures_and_counts() {
    // Through the app, so the band shows the counts.
    let city = index(Kind::City);
    println!("CITY: load, then per call (avg / most over one cycle): objects, tris, px");
    for (lv, &load) in TESTS[city].loads.iter().enumerate() {
        let mut d = Driver::new();
        d.pick(city, lv);
        d.idle(100);
        check_palette(&d.frame, "city");
        save_frame(&file(&format!("city-load-{load:03}.png")), &d.frame, 4);
        let mut w = Work::new();
        let (mut so, mut st, mut sp, mut mo, mut mt, mut mp) = (0u64, 0u64, 0u64, 0, 0, 0);
        let (mut sv, mut sf, mut sr, mut sc) = (0u64, 0u64, 0u64, 0u64);
        for tick in 0..CYCLE {
            let (_, c) = one(&mut w, Kind::City, load, tick);
            so += u64::from(c.objs);
            st += u64::from(c.tris);
            sp += u64::from(c.px);
            sv += u64::from(c.verts);
            sf += u64::from(c.faces);
            sr += u64::from(c.front);
            sc += u64::from(c.clipped);
            mo = mo.max(c.objs);
            mt = mt.max(c.tris);
            mp = mp.max(c.px);
        }
        let n = u64::from(CYCLE);
        println!(
            "city {load:>4}: objs {:>4} / {mo:>4}  tris {:>5} / {mt:>5}  px {:>6} / {mp:>6}",
            so / n,
            st / n,
            sp / n
        );
        println!(
            "           avg corners {:>5}  faces {:>5}  facing the eye {:>5}  cut {:>4}",
            sv / n,
            sf / n,
            sr / n,
            sc / n
        );
    }
    // Three pictures a second apart (at 30) at the middle load.
    let mut d = Driver::new();
    d.pick(city, 2);
    d.idle(100);
    for s in 0..3 {
        save_frame(
            &file(&format!("city-064-second-{}.png", s + 1)),
            &d.frame,
            4,
        );
        d.idle(30);
    }
    let spin = index(Kind::Spin);
    let mut d = Driver::new();
    d.pick(spin, 1);
    d.idle(40);
    save_frame(&file("tri-spin-a.png"), &d.frame, 4);
    d.idle(5);
    save_frame(&file("tri-spin-b.png"), &d.frame, 4);
    for &load in TESTS[spin].loads {
        let mut w = Work::new();
        let (_, c) = one(&mut w, Kind::Spin, load, 7);
        println!("spin {load:>5}: tris {:>5}  px {:>7}", c.tris, c.px);
    }
}

#[test]
fn the_game_value_did_not_grow() {
    // The kit's `update` builds the game on its own stack frame. Round 3
    // added ISLAND's scratch (the figure's 64 corners and 96 normals,
    // the fine floor's 92 faces) and moved the 2 KB sort lists onto the
    // stack of the call that uses them: 6,640 -> 5,928 bytes on the PC
    // (5,920 on the chip). The ramp-60 change added the 60 schedule's
    // call count, the screen-pace counts and the pending screen-pace line:
    // 5,960 on the PC (5,952 on the chip). The PC-only `check` feature adds
    // the checker's recorder (about 180 KB): the bound is for game builds.
    if cfg!(feature = "check") {
        return;
    }
    let n = std::mem::size_of::<bench::Bench>();
    println!("Bench: {n} bytes");
    assert!(n <= 5_960, "{n}");
}

#[test]
fn a_sheet_of_one_cycle() {
    // Twelve calls across one cycle at load 64 (work area only), side by
    // side at twice the size: for looking for drawing faults.
    let (cols, rows, s) = (4usize, 3usize, 2usize);
    let (fw, fh) = (160 * s, 100 * s);
    let mut rgb = vec![0u8; cols * fw * rows * fh * 3];
    let mut w = Work::new();
    for n in 0..cols * rows {
        // + 14 ticks: the same pictures as call + 7 of the earlier rounds.
        let tick = n as u32 * CYCLE / (cols * rows) as u32 + 14;
        let (f, _) = one(&mut w, Kind::City, 64, tick);
        let (ox, oy) = ((n % cols) * fw, (n / cols) * fh);
        for y in 0..fh {
            for x in 0..fw {
                let c = common::rgb_of(f.0[(20 + y / s) * W + x / s]);
                let at = ((oy + y) * cols * fw + ox + x) * 3;
                rgb[at..at + 3].copy_from_slice(&c);
            }
        }
    }
    common::write_png(
        &file("city-064-cycle-sheet.png"),
        cols * fw,
        rows * fh,
        &rgb,
    );
}

// ---- After the check (defects A, B and C) ----------------------------------

/// Defect A: a triangle the box test skips paints nothing, so skipping it
/// never changes the picture: random triangles in and around the work
/// area, many of them thin or tiny, against the slow filler.
#[test]
fn the_box_test_never_skips_a_triangle_that_paints() {
    use bench::poly::no_pixel_middle;
    let mut s = 0x1234_5678u32;
    let mut next = |m: i32| {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        (s % m as u32) as i32
    };
    let (mut skipped, mut painting) = (0u32, 0u32);
    for i in 0..200_000 {
        // Corners in 1/16 pixel, from 20 pixels outside to 20 inside;
        // every third triangle is at most 2 pixels across.
        let a = [next(200 * 16) - 20 * 16, next(140 * 16)];
        let small = i % 3 == 0;
        let q = |a: [i32; 2], r: &mut dyn FnMut(i32) -> i32| {
            if small {
                [a[0] + r(33) - 16, a[1] + r(33) - 16]
            } else {
                [r(200 * 16) - 20 * 16, r(140 * 16)]
            }
        };
        let b = q(a, &mut next);
        let c = q(a, &mut next);
        let p = [a, b, c];
        let mut f = Box::new(Frame::new());
        let n = fill_tri_slow(&mut f, &WORK, &p, 1);
        if no_pixel_middle(&p, &WORK) {
            skipped += 1;
            assert_eq!(n, 0, "{p:?}");
        }
        if n > 0 {
            painting += 1;
        }
    }
    println!("box test: {skipped} of 200000 skipped, {painting} paint");
    assert!(skipped > 10_000 && painting > 50_000);
}

/// Defect A: TRIS counts only the triangles that painted (at least one
/// pixel each, so never more triangles than pixels).
#[test]
fn city_tris_are_triangles_that_painted() {
    for load in loads(Kind::City) {
        let mut w = Work::new();
        for tick in (0..CYCLE).step_by(48) {
            let (_, c) = one(&mut w, Kind::City, load, tick);
            assert!(c.tris <= c.px, "{load} {tick}: {} > {}", c.tris, c.px);
        }
    }
}

/// Defect B: cars are drawn to a fixed distance per load, which grows
/// with the load, from 48 m to at most the road's reach.
#[test]
fn cars_reach_a_fixed_distance_that_grows_with_the_load() {
    use bench::city::{CM, ROAD_RANGE, car_reach};
    let mut last = 0;
    for load in 0..=MAX_LOAD + 8 {
        let r = car_reach(load);
        assert!(r >= last, "{load}");
        assert!((48 * CM..=ROAD_RANGE).contains(&r), "{load}: {r}");
        last = r;
    }
    assert_eq!(car_reach(8), 48 * CM);
    assert!(car_reach(MAX_LOAD) > 190 * CM - 1);
    // Not shorter than about the stretch the buildings cover.
    for load in [32u32, 64, 96] {
        assert!(car_reach(load) >= load as i32 * 205, "{load}");
    }
}

/// Defect C: a CITY ramp step measures one whole cycle (every 8th call),
/// so the count at FIT is the cycle's average; the other tests' steps are
/// as before.
#[test]
fn a_city_ramp_step_measures_one_whole_cycle() {
    use bench::city::{RAMP_CALLS, RAMP_STRIDE};
    use bench::ramp::{MIN_FRAMES, WARM};
    assert_eq!(RAMP_CALLS * RAMP_STRIDE, CYCLE);
    let city = index(Kind::City);
    let mut d = Driver::new();
    d.pick(city, 1);
    d.call(game_kit::button::A);
    let mut n = 0;
    let mut steps = Vec::new();
    while d.app.mode == bench::app::Mode::Ramp && n < 200_000 {
        let before = d.app.last_step;
        d.call(0);
        if d.app.last_step != before {
            steps.push(d.app.last_step.unwrap());
        }
        n += 1;
    }
    assert_eq!(d.app.mode, bench::app::Mode::Result);
    assert!(!steps.is_empty());
    for s in &steps {
        assert_eq!(s.frames, RAMP_CALLS, "{s:?}");
    }
    // On time, everything fits: FIT is the top step at both rates. n at
    // 30 is the average over every 16th tick of the cycle; at 60 over the
    // counted pictures of the 16 real-speed runs (ticks s*96 + 6..12).
    let mut w = Work::new();
    let sum: u64 = (0..RAMP_CALLS)
        .map(|i| u64::from(one(&mut w, Kind::City, MAX_LOAD, i * RAMP_STRIDE).1.tris))
        .sum();
    let avg = (sum / u64::from(RAMP_CALLS)) as u32;
    let runs: Vec<u32> = (0..bench::city::STEP60_CALLS)
        .map(bench::city::ramp60_tick)
        .filter(|&(_, counted)| counted)
        .map(|(t, _)| t)
        .collect();
    assert_eq!(runs.len() as u32, RAMP_CALLS);
    let sum60: u64 = runs
        .iter()
        .map(|&t| u64::from(one(&mut w, Kind::City, MAX_LOAD, t).1.tris))
        .sum();
    let avg60 = (sum60 / u64::from(RAMP_CALLS)) as u32;
    let [r60, r30] = d.app.ramp.results;
    for r in [r60, r30] {
        assert_eq!((r.fit, r.capped), (MAX_LOAD, true));
        assert_eq!(r.screen, 0);
    }
    assert_eq!(r60.fit_extra, avg60);
    assert_eq!(r30.fit_extra, avg);
    // Both within 2 % of the average over every tick of the cycle.
    let all0: u64 = (0..CYCLE)
        .map(|t| u64::from(one(&mut w, Kind::City, MAX_LOAD, t).1.tris))
        .sum::<u64>()
        / u64::from(CYCLE);
    assert!(
        u64::from(avg60).abs_diff(all0) * 50 <= all0,
        "{avg60} {all0}"
    );
    // Close to the average over every call of the cycle (within 2 %).
    let all: u64 = (0..CYCLE)
        .map(|t| u64::from(one(&mut w, Kind::City, MAX_LOAD, t).1.tris))
        .sum::<u64>()
        / u64::from(CYCLE);
    assert!(u64::from(avg).abs_diff(all) * 50 <= all, "{avg} {all}");
    println!(
        "CITY ramp on time: {} steps of {RAMP_CALLS} counted calls, {n} calls; n at FIT 30 {avg}, 60 {avg60} (all calls {all})",
        steps.len()
    );
    // TRI SPIN keeps the old step (half a second, at least MIN_FRAMES).
    let mut d = Driver::new();
    d.pick(index(Kind::Spin), 0);
    d.call(game_kit::button::A);
    let mut calls = 0;
    while d.app.last_step.is_none() {
        d.call(0);
        calls += 1;
    }
    let s = d.app.last_step.unwrap();
    assert!(s.frames >= MIN_FRAMES && s.frames < RAMP_CALLS, "{s:?}");
    assert_eq!(calls, WARM + s.frames);
}
