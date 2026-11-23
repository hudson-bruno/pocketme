//! Each level: what it draws matches its load, and its picture is the
//! same every time.

mod common;

use bench::art::{FILL0, SKY};
use bench::load::{Kind, MODELS, SHAPES, TESTS, WORK, WORK_BYTES, Work};
use bench::poly::{Rect, fill_tri, fill_tri_slow};
use common::{Driver, hash};
use game_kit::{Frame, W};

fn work_area(f: &Frame) -> &[u8] {
    &f.0[20 * W..]
}

#[test]
fn every_shape_paints_its_pixels_wherever_it_is_put() {
    for s in &SHAPES {
        let xr = (160 - s.w) as u32;
        let yr = (100 - s.h) as u32;
        for i in 0..400u32 {
            let x = (i * 37 % xr) as i32 * 16;
            let y = (20 + (i * 23 % yr) as i32) * 16;
            let q = s.p.map(|v| [v[0] + x, v[1] + y]);
            let (mut a, mut b) = (Frame::new(), Frame::new());
            assert_eq!(fill_tri(&mut a, &WORK, &q, 1), s.px);
            assert_eq!(
                fill_tri_slow(&mut b, &Rect::new(0, 0, 160, 120), &q, 1),
                s.px
            );
            assert_eq!(a.0, b.0, "same pixels as the slow filler");
        }
    }
}

#[test]
fn triangle_levels_draw_their_load() {
    let mut d = Driver::new();
    for (i, t) in TESTS.iter().enumerate() {
        let Kind::Tri(s) = t.kind else { continue };
        for (lv, &load) in t.loads.iter().enumerate() {
            d.pick(i, lv);
            d.call(0);
            assert_eq!(d.app.last.tris, load, "{}", t.name);
            assert_eq!(d.app.last.px, load * SHAPES[s].px, "{}", t.name);
        }
    }
}

#[test]
fn fill_levels_fill_their_load() {
    let mut d = Driver::new();
    for (lv, &load) in TESTS[0].loads.iter().enumerate() {
        d.pick(0, lv);
        d.call(0);
        assert_eq!(d.app.last.px, load * WORK_BYTES);
        let last = FILL0 + ((load - 1) & 3) as u8;
        assert!(work_area(&d.frame).iter().all(|&p| p == last));
    }
}

#[test]
fn the_corner_level_turns_its_load() {
    let mut d = Driver::new();
    for (lv, &load) in TESTS[4].loads.iter().enumerate() {
        d.pick(4, lv);
        d.call(0);
        let c = d.app.last;
        assert_eq!(c.verts, load);
        assert_eq!(c.objs, load.div_ceil(50));
        assert_eq!((c.tris, c.px), (0, 0), "nothing drawn");
    }
    // The same call gives the same sum.
    let mut w = Work::new();
    let mut f = Frame::new();
    let a = w.run(&mut f, Kind::Vert, 1000, 17, false).check;
    let b = w.run(&mut f, Kind::Vert, 1000, 18, false).check;
    let c = Work::new().run(&mut f, Kind::Vert, 1000, 17, false).check;
    assert_eq!(a, c);
    assert_ne!(a, b, "the objects turn");
}

/// A ball is convex: its front faces never overlap, so one ball paints
/// each pixel it covers once. Not quite: a face seen almost edge-on can
/// be judged front from the rounded eye position although it turned
/// away, and then paints a pixel or two that a neighbour paints too.
/// That is rare (counted here) and small.
#[test]
fn a_lone_ball_paints_exactly_what_is_seen() {
    let mut w = Work::new();
    let mut seen_clipped = false;
    let (mut pictures, mut exact) = (0, 0);
    for (mi, m) in MODELS.iter().enumerate() {
        // Ticks of 1/60 s: every third call at 30, as before.
        for tick in (0..600).step_by(6) {
            let mut f = Frame::new();
            let c = w.run(&mut f, Kind::Scene(mi), 1, tick, true);
            let covered = work_area(&f).iter().filter(|&&p| p != SKY).count() as u32;
            assert_eq!(c.dropped, 0);
            assert!(c.objs <= 1);
            assert_eq!(c.faces, c.objs * m.faces.len() as u32);
            assert_eq!(c.verts, c.objs * m.verts.len() as u32);
            if c.clipped == 0 {
                pictures += 1;
                exact += u32::from(c.px == covered);
                assert!(
                    c.px >= covered && c.px <= covered + 3,
                    "ball {mi}, tick {tick}"
                );
                assert_eq!(c.tris, c.front);
            } else {
                seen_clipped = true;
                // Cut edges are worked out from each side on its own: a
                // pixel or two along them may be painted twice.
                assert!(
                    c.px >= covered && c.px <= covered + covered / 50 + 8,
                    "{c:?} {covered}"
                );
                assert!(c.tris + c.clipped >= c.front && c.tris <= c.front + 5 * c.clipped);
            }
        }
    }
    assert!(seen_clipped, "the near plane is crossed in the cycle");
    println!("{exact} of {pictures} pictures without a pixel painted twice");
    assert!(exact * 20 >= pictures * 19);
}

#[test]
fn scenes_count_what_they_draw() {
    let mut d = Driver::new();
    for (i, t) in TESTS.iter().enumerate() {
        let Kind::Scene(mi) = t.kind else { continue };
        let m = MODELS[mi];
        for (lv, &load) in t.loads.iter().enumerate() {
            d.pick(i, lv);
            for _ in 0..5 {
                d.call(0);
                let c = d.app.last;
                let covered = work_area(&d.frame).iter().filter(|&&p| p != SKY).count() as u32;
                assert!(c.objs <= load && c.objs > 0, "{} {c:?}", t.name);
                assert_eq!(c.faces, c.objs * m.faces.len() as u32);
                assert_eq!(c.dropped, 0);
                assert!(c.px >= covered, "every pixel seen was painted");
                assert!(c.front <= c.faces && c.tris > 0);
            }
        }
    }
}

/// One picture per level, after 40 calls (the numbers in the band have
/// come by then), against `tests/data/golden.txt`. `UPDATE_GOLDEN=1`
/// writes the file again.
#[test]
fn every_level_has_a_fixed_picture() {
    let mut lines = Vec::new();
    for (i, t) in TESTS.iter().enumerate() {
        for lv in 0..t.loads.len() {
            if t.loads[lv] > 4096 && matches!(t.kind, Kind::Tri(_) | Kind::Vert) {
                continue; // slow in a debug build; the counts are checked above
            }
            let pics: Vec<u64> = (0..2)
                .map(|_| {
                    let mut d = Driver::new();
                    d.pick(i, lv);
                    d.idle(40);
                    hash(&d.frame)
                })
                .collect();
            assert_eq!(
                pics[0], pics[1],
                "{} level {lv} is the same every run",
                t.name
            );
            lines.push(format!("{} {} {:016x}", t.name, t.loads[lv], pics[0]));
        }
    }
    let text = lines.join("\n") + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/golden.txt");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    let want = std::fs::read_to_string(&path).expect("tests/data/golden.txt (UPDATE_GOLDEN=1)");
    assert_eq!(text, want);
}

#[test]
fn the_static_levels_draw_the_same_picture_every_call() {
    for i in [0usize, 1, 2, 3, 4] {
        let mut d = Driver::new();
        d.pick(i, 1);
        d.idle(2);
        let a = d.frame.0;
        d.idle(1);
        assert_eq!(a, d.frame.0, "{}", TESTS[i].name);
    }
}

#[test]
fn the_band_shows_the_level_and_its_window() {
    let mut d = Driver::new();
    d.pick(2, 2);
    let l = d.app.band_lines(false);
    assert_eq!(l[0].as_str(), "TRI 100  2048 TRIS  AT 30");
    assert_eq!(l[1].as_str(), "FRAME ...");
    d.idle(40);
    let l = d.app.band_lines(false);
    assert_eq!(l[1].as_str(), "FRAME 33333 MAX 33333 LATE 0");
    assert_eq!(l[2].as_str(), "TRIS 2048 PX 204800");
    // X: 60 a second; the band says so once the console took it.
    d.call(game_kit::button::X);
    assert!(d.rate60);
    let l = d.app.band_lines(true);
    assert_eq!(l[0].as_str(), "TRI 100  2048 TRIS  AT 60");
    for (i, t) in TESTS.iter().enumerate() {
        for lv in 0..t.loads.len() {
            d.pick(i, lv);
            for l in d.app.band_lines(false) {
                assert!(game_kit::text_width(l.as_str()) <= 156, "{:?}", l.as_str());
            }
        }
    }
}

#[test]
fn keys_walk_the_menu() {
    let mut d = Driver::new();
    assert_eq!(d.app.test, 0);
    d.call(game_kit::button::LEFT);
    assert_eq!(d.app.test, TESTS.len() - 1, "LEFT wraps round");
    d.call(game_kit::button::RIGHT);
    assert_eq!(d.app.test, 0);
    assert_eq!(
        d.app.load(),
        TESTS[0].loads[1],
        "each test starts at its second load"
    );
    d.call(game_kit::button::UP);
    assert_eq!(d.app.load(), TESTS[0].loads[2]);
    for _ in 0..9 {
        d.call(game_kit::button::UP);
    }
    assert_eq!(
        d.app.load(),
        *TESTS[0].loads.last().unwrap(),
        "UP stops at the top"
    );
    let out = d.call(game_kit::button::START);
    assert!(out.quit);
    assert!(d.logs.iter().any(|l| l == "bench: FILL load 64 at 30"));
}
