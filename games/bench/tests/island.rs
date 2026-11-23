//! ISLAND (test 13): the same picture for the same tick, counts that are
//! what was painted, a loop without a jump, nothing drawn outside the
//! work area, a camera that never comes near a solid thing, a figure
//! that never runs through one, and a slow exact checker of the draw
//! order. Writes its pictures into `.local/bench/shots/` (run with
//! `--nocapture` for the tables).
//!
//! The counts and checker tests need the PC-only feature `check` (the
//! renderer's recorder): `cargo test --all-features`.

mod common;

use bench::island::{
    self, LEVEL_NAMES, LOOP, RAMP1, RAMP2, THINGS, camera, ground, heading, model_of, pos_of,
    rot_of, runner,
};
use bench::load::{Counts, Kind, TESTS, WORK, Work};
use bench::poly::{M8, NEAR};
use common::{Driver, check_palette, save_frame, shots};
use game_kit::{Frame, W};

fn index() -> usize {
    TESTS.iter().position(|t| t.kind == Kind::Island).unwrap()
}

/// One call on a fresh picture whose band holds a marker colour.
fn one(w: &mut Work, level: u32, tick: u32) -> (Box<Frame>, Counts) {
    let mut f = Box::new(Frame::new());
    f.0.fill(250);
    let c = w.run(&mut f, Kind::Island, level, tick, true);
    (f, c)
}

fn work() -> Box<Work> {
    Box::new(Work::new())
}

fn cm(units: i32) -> i32 {
    units * 100 / M8
}

#[test]
fn island_is_test_13_with_five_fixed_levels() {
    let t = &TESTS[index()];
    assert_eq!(index(), 12);
    assert_eq!(t.name, "ISLAND");
    assert_eq!(t.loads, &[1, 2, 3, 4, 5]);
    // No ramp: A does nothing (kmax 0, the ramp's only load is level 1).
    assert_eq!(t.kmax, 0);
    assert_eq!(LEVEL_NAMES, ["LOW", "MID", "HIGH", "XL", "XXL"]);
}

#[test]
fn the_same_tick_gives_the_same_picture() {
    for level in 1..=5 {
        for tick in [0, 1, 100, 383, 767] {
            let (a, ca) = one(&mut work(), level, tick);
            // A used work space (the sort, the figure) changes nothing.
            let mut w = work();
            one(&mut w, 5, tick + 77);
            let (b, cb) = one(&mut w, level, tick);
            assert_eq!(a.0, b.0, "level {level} tick {tick}");
            assert_eq!(ca, cb);
        }
    }
}

#[test]
fn nothing_is_drawn_outside_the_work_area() {
    for level in 1..=5 {
        for tick in (0..LOOP).step_by(7) {
            let (f, _) = one(&mut work(), level, tick);
            for (i, &p) in f.0.iter().enumerate() {
                let y = (i / W) as i32;
                if !(WORK.y0..WORK.y1).contains(&y) {
                    assert_eq!(p, 250, "level {level} tick {tick}: pixel {i}");
                }
            }
        }
    }
}

#[test]
fn the_loop_closes_without_a_jump() {
    // The tick after the last is the first: the same picture.
    for level in 1..=5 {
        for tick in [0, 5, 400] {
            let (a, _) = one(&mut work(), level, tick);
            let (b, _) = one(&mut work(), level, tick + LOOP);
            assert_eq!(a.0, b.0);
        }
    }
    // And the step from tick 767 to 0 is like any other step: the figure,
    // the camera and their turns move no more than they do elsewhere.
    let step = |t: u32| {
        let (a, b) = (runner(t), runner(t + 1));
        let (ca, cb) = (camera(t), camera(t + 1));
        let d = |p: [i32; 3], q: [i32; 3]| (0..3).map(|k| (p[k] - q[k]).abs()).max().unwrap();
        let (ha, hb) = (heading(t, 5), heading(t + 1, 5));
        let (ka, kb) = (heading(t, 30), heading(t + 1, 30));
        (
            d(a.feet, b.feet),
            d(ca.eye, cb.eye),
            (ha[0] - hb[0]).abs().max((ha[1] - hb[1]).abs()),
            (ka[0] - kb[0]).abs().max((ka[1] - kb[1]).abs()),
        )
    };
    let worst = (0..LOOP - 1).map(step).fold((0, 0, 0, 0), |m, s| {
        (m.0.max(s.0), m.1.max(s.1), m.2.max(s.2), m.3.max(s.3))
    });
    let wrap = step(LOOP - 1);
    println!("worst step (units, turn): {worst:?}, at the wrap: {wrap:?}");
    assert!(wrap.0 <= worst.0 && wrap.1 <= worst.1);
    assert!(wrap.2 <= worst.2 && wrap.3 <= worst.3);
    // Steps are small: the feet at most 25 cm a tick (the end of the
    // jump down from platform 2), the eye at most 20 cm, the camera's
    // turn at most 1/25 of a radian a tick.
    assert!(worst.0 <= M8 / 4 && worst.1 <= M8 / 5, "{worst:?}");
    assert!(worst.3 <= 16_384 / 25, "{worst:?}");
}

/// A thing's world box (cm): its model's corners turned and moved.
fn world_box(i: usize, t: u32) -> ([i32; 3], [i32; 3]) {
    let th = &THINGS[i];
    let m = model_of(th);
    let r = rot_of(i, th, t);
    let p = pos_of(th);
    let mut lo = [i32::MAX; 3];
    let mut hi = [i32::MIN; 3];
    for v in m.verts {
        let w = r.apply(v.map(i32::from));
        for k in 0..3 {
            let c = cm(p[k] + w[k]);
            lo[k] = lo[k].min(c);
            hi[k] = hi[k].max(c);
        }
    }
    (lo, hi)
}

/// How far a point is from a box (cm, 0 inside).
fn dist(p: [i32; 3], b: &([i32; 3], [i32; 3])) -> i32 {
    let d: Vec<i64> = (0..3)
        .map(|k| i64::from((b.0[k] - p[k]).max(p[k] - b.1[k]).max(0)))
        .collect();
    ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) as f64).sqrt() as i32
}

#[test]
fn the_camera_never_comes_near_a_solid_thing() {
    // Solid things (not hidden near the eye) keep at least 60 cm from the
    // eye: no corner of theirs ever reaches the near plane (50 cm), so no
    // face is cut there. Small things are hidden instead (the bush fix).
    assert_eq!(NEAR * 100 / M8, 50);
    let mut closest = (i32::MAX, 0, 0);
    for t in 0..LOOP {
        let eye = camera(t).eye.map(cm);
        for (i, th) in THINGS.iter().enumerate() {
            if th.hide || th.layer == 0 {
                continue;
            }
            let d = dist(eye, &world_box(i, t));
            if d < closest.0 {
                closest = (d, i, t);
            }
        }
    }
    println!("closest solid thing to the eye: {closest:?}");
    assert!(closest.0 >= 60, "{closest:?}");
}

#[test]
fn the_figure_never_runs_through_a_thing() {
    // The figure's box (arms out, 30 cm each side, 126 cm high) never
    // meets any thing's box at any level, except that its feet may touch
    // the top of what it stands on (3 cm; 16 cm at an edge).
    let mut worst = Vec::new();
    for t in 0..LOOP {
        let f = runner(t).feet.map(cm);
        let lo = [f[0] - 30, f[1] + 3, f[2] - 30];
        let hi = [f[0] + 30, f[1] + 126, f[2] + 30];
        for (i, th) in THINGS.iter().enumerate() {
            let mut b = world_box(i, t);
            // A ramp is a wedge rising along z: its top under the figure.
            let ramp = match th.role {
                island::ON_RAMP1 => Some(RAMP1),
                island::ON_RAMP2 => Some(RAMP2),
                _ => None,
            };
            if let Some(r) = ramp {
                let z = f[2].clamp(r[2], r[3]);
                b.1[1] = r[4] + (r[5] - r[4]) * (z - r[2]) / (r[3] - r[2]);
            }
            // Over the edge of something to stand on (a terrace, a
            // ramp, a platform) the back of the box may be 16 cm below its
            // top (30 cm back on the steeper ramp is 15 cm).
            let lo_y = if th.role != 0 { f[1] + 16 } else { lo[1] };
            let lo = [lo[0], lo_y, lo[2]];
            if (0..3).all(|k| lo[k] < b.1[k] && hi[k] > b.0[k]) {
                worst.push((t, i, *th));
            }
        }
    }
    assert!(worst.is_empty(), "{:?}", &worst[..worst.len().min(5)]);
}

#[test]
fn small_things_stand_clear_of_solid_ones() {
    // A tree, bush, fence, grass patch or gem never pokes into a solid
    // thing at a level where both are drawn (its box may reach 3 cm
    // into the top of the floor or terrace it stands on).
    let mut bad = Vec::new();
    for (a, ta) in THINGS.iter().enumerate() {
        if !ta.hide {
            continue;
        }
        let ba = world_box(a, 0);
        for (b, tb) in THINGS.iter().enumerate() {
            // What it stands or floats over is not checked (a gem over
            // a ramp is inside the ramp's box, not in the ramp).
            let support = ta.on != 0 && tb.role == ta.on;
            if tb.hide || tb.layer != 2 || ta.levels & tb.levels == 0 || support {
                continue;
            }
            let bb = world_box(b, 0);
            let lo_y = ba.0[1] + 3;
            let hit = (0..3).all(|k| {
                let lo = if k == 1 { lo_y } else { ba.0[k] };
                lo < bb.1[k] && ba.1[k] > bb.0[k]
            });
            if hit {
                bad.push((a, b));
            }
        }
    }
    assert!(bad.is_empty(), "{bad:?}");
}

#[test]
fn the_figure_stands_on_something_when_it_runs() {
    for t in 0..LOOP {
        let r = runner(t);
        let f = r.feet.map(cm);
        for lv in 0..5 {
            let (g, _) = ground(f[0], f[2], f[1], lv);
            if r.jump.is_none() {
                assert!((f[1] - g).abs() <= 2, "tick {t} level {lv}: {f:?} over {g}");
            } else {
                assert!(f[1] >= g - 2, "tick {t}: {f:?} under {g}");
            }
        }
    }
}

#[test]
fn pictures_and_counts() {
    let i = index();
    println!("ISLAND: per tick over one loop (avg / most): objects, tris, px");
    for (lv, name) in LEVEL_NAMES.iter().enumerate() {
        let mut d = Driver::new();
        d.pick(i, lv);
        d.idle(100);
        check_palette(&d.frame, "island");
        save_frame(
            &shots().join(format!("island-{}-{}.png", lv + 1, name.to_lowercase())),
            &d.frame,
            4,
        );
        let mut w = work();
        let mut s = [0u64; 6];
        let mut m = [0u32; 6];
        let mut over = [0u32; 2];
        let mut dropped = 0;
        for tick in 0..LOOP {
            let (_, c) = one(&mut w, lv as u32 + 1, tick);
            // ESTIMATED loop time on the board (µs): round 2's cost
            // model fitted to the board, from the
            // counts; and the simpler 1 ms + 13.4 µs a triangle.
            let units =
                128 * u64::from(c.verts) + 625 * u64::from(c.tris) + 74 * u64::from(c.px) / 10;
            let fit = 2_435 + (1_381 * units / 100_000) as u32;
            let simple = 1_000 + c.tris * 134 / 10;
            for (k, v) in [c.objs, c.tris, c.px, c.verts, fit, simple]
                .into_iter()
                .enumerate()
            {
                s[k] += u64::from(v);
                m[k] = m[k].max(v);
            }
            over[0] += u32::from(fit > 16_667);
            over[1] += u32::from(fit > 33_333);
            dropped += c.dropped;
        }
        let n = u64::from(LOOP);
        println!(
            "{name:>5}: objs {:>3} / {:>3}  tris {:>4} / {:>4}  px {:>6} / {:>6}  corners {:>4} / {:>4}  dropped {dropped}",
            s[0] / n,
            m[0],
            s[1] / n,
            m[1],
            s[2] / n,
            m[2],
            s[3] / n,
            m[3]
        );
        println!(
            "{name:>5}: ESTIMATED loop {:>5} / {:>5} us (simple {:>5} / {:>5}); ticks over 16.7 ms {:>3}, over 33.3 ms {:>3} of {LOOP}",
            s[4] / n,
            m[4],
            s[5] / n,
            m[5],
            over[0],
            over[1]
        );
        assert_eq!(dropped, 0);
    }
    // Six pictures half a second apart (15 calls at 30) at MID.
    let mut d = Driver::new();
    d.pick(i, 1);
    d.idle(100);
    for k in 0..6 {
        save_frame(
            &shots().join(format!("island-mid-{}.png", k + 1)),
            &d.frame,
            4,
        );
        d.idle(15);
    }
    // Sheets of twelve ticks across the loop (work area, twice the size).
    for level in [2u32, 5] {
        let (cols, rows, sc) = (4usize, 3usize, 2usize);
        let (fw, fh) = (160 * sc, 100 * sc);
        let mut rgb = vec![0u8; cols * fw * rows * fh * 3];
        let mut w = work();
        for n in 0..cols * rows {
            let tick = n as u32 * LOOP / (cols * rows) as u32 + 20;
            let (f, _) = one(&mut w, level, tick);
            let (ox, oy) = ((n % cols) * fw, (n / cols) * fh);
            for y in 0..fh {
                for x in 0..fw {
                    let c = common::rgb_of(f.0[(20 + y / sc) * W + x / sc]);
                    let at = ((oy + y) * cols * fw + ox + x) * 3;
                    rgb[at..at + 3].copy_from_slice(&c);
                }
            }
        }
        common::write_png(
            &shots().join(format!(
                "island-sheet-{}.png",
                LEVEL_NAMES[level as usize - 1].to_lowercase()
            )),
            cols * fw,
            rows * fh,
            &rgb,
        );
    }
}

#[test]
fn the_figures_shadow_and_parts_stay_within_the_renderer() {
    // Every model fits the renderer (64 corners, 128 faces), at every
    // level the frame's triangles fit (dropped stays 0: checked in
    // `shots`), and the other owner numbers (fine strips from 0xFF00,
    // the figure from 0xFFF0) never meet THINGS.
    assert!(THINGS.len() < 0xFF00);
    for th in THINGS.iter() {
        let m = model_of(th);
        assert!(m.verts.len() <= 64 && m.faces.len() <= 128);
    }
    let _ = island::HORIZON;
}

#[cfg(feature = "check")]
mod checker {
    //! The slow exact checker: the picture is drawn again from the
    //! recorder's triangles, one pixel at a time, and each pixel's shown
    //! triangle is compared with the nearest triangle that covers it.

    use super::*;
    use bench::poly::{REC_MAX, Rect, fill_tri};

    /// 1/z at the middle of pixel (x, y) of a triangle (screen corners in
    /// 1/16 pixel, camera depths), in 2^40 / units: the screen-space
    /// interpolation of 1/z is exact for a flat triangle.
    fn inv_z(p: &[[i16; 2]; 3], z: &[i16; 3], x: i32, y: i32) -> i128 {
        let (px, py) = (i128::from(16 * x + 8), i128::from(16 * y + 8));
        let q = p.map(|c| [i128::from(c[0]), i128::from(c[1])]);
        let area =
            (q[1][0] - q[0][0]) * (q[2][1] - q[0][1]) - (q[2][0] - q[0][0]) * (q[1][1] - q[0][1]);
        if area == 0 {
            return 0;
        }
        let w =
            |a: usize, b: usize| (q[b][0] - px) * (q[a][1] - py) - (q[a][0] - px) * (q[b][1] - py);
        // Weights of corners 0, 1, 2 (sum = area, same sign).
        let l = [w(1, 2), w(2, 0), w(0, 1)];
        let iz = z.map(|v| (1i128 << 40) / i128::from(v.max(1)));
        -(l[0] * iz[0] + l[1] * iz[1] + l[2] * iz[2]) / area
    }

    /// Wrong pixels of one picture, and what the replay found.
    struct Check {
        wrong: u32,
        painted: u32,
        tris: u32,
        mismatch: u32,
        pairs: Vec<((u16, u16), u32)>,
        mask: Vec<bool>,
        frame: Box<Frame>,
    }

    fn check(w: &mut Work, level: u32, tick: u32) -> (Check, Counts) {
        let (f, c) = one(w, level, tick);
        let r = &w.r;
        assert!(r.nrec <= REC_MAX, "the recorder ran out");
        let n = r.nrec;
        let area = WORK;
        let mut scratch = Box::new(Frame::new());
        // Per pixel: the last triangle that painted it, the nearest one.
        let mut last = vec![u32::MAX; W * 120];
        let mut best = vec![0i128; W * 120];
        let mut best_owner = vec![0u16; W * 120];
        let mut painted = 0;
        let mut tris = 0;
        for (k, t) in r.rec[..n].iter().enumerate() {
            let p = t.p.map(|q| [i32::from(q[0]), i32::from(q[1])]);
            scratch.0.fill(0);
            let got = fill_tri(&mut scratch, &area, &p, 1);
            painted += got;
            tris += u32::from(got > 0);
            if got == 0 {
                continue;
            }
            let lo = |a: usize| (p[0][a].min(p[1][a]).min(p[2][a]) / 16 - 1).max(0);
            let hi = |a: usize| p[0][a].max(p[1][a]).max(p[2][a]) / 16 + 2;
            let b = Rect {
                x0: lo(0).max(area.x0),
                y0: lo(1).max(area.y0),
                x1: hi(0).min(area.x1),
                y1: hi(1).min(area.y1),
            };
            for y in b.y0..b.y1 {
                for x in b.x0..b.x1 {
                    let i = y as usize * W + x as usize;
                    if scratch.0[i] == 1 {
                        last[i] = k as u32;
                        let iz = inv_z(&t.p, &t.z, x, y);
                        if iz > best[i] {
                            best[i] = iz;
                            best_owner[i] = t.owner;
                        }
                    }
                }
            }
        }
        let mut wrong = 0;
        let mut mismatch = 0;
        let mut pairs = std::collections::HashMap::new();
        let mut mask = vec![false; W * 120];
        for y in area.y0..area.y1 {
            for x in area.x0..area.x1 {
                let i = y as usize * W + x as usize;
                let Some(t) = r.rec.get(last[i] as usize) else {
                    continue;
                };
                if last[i] == u32::MAX {
                    continue;
                }
                mismatch += u32::from(f.0[i] != t.c);
                let shown = inv_z(&t.p, &t.z, x, y);
                // Wrong: something covering this pixel is nearer by more
                // than 1 % (rounding of the corners and of the cut).
                if shown * 100 < best[i] * 99 {
                    wrong += 1;
                    mask[i] = true;
                    *pairs.entry((t.owner, best_owner[i])).or_insert(0u32) += 1;
                }
            }
        }
        (
            Check {
                wrong,
                painted,
                tris,
                mismatch,
                pairs: {
                    let mut v: Vec<_> = pairs.into_iter().collect();
                    v.sort_by_key(|p| std::cmp::Reverse(p.1));
                    v.truncate(4);
                    v
                },
                mask,
                frame: f,
            },
            c,
        )
    }

    #[test]
    fn counts_are_what_was_painted() {
        for level in 1..=5 {
            let mut w = work();
            for tick in (0..LOOP).step_by(24) {
                let (k, c) = check(&mut w, level, tick);
                assert_eq!(k.mismatch, 0, "the replay is the picture");
                assert_eq!(k.painted, c.px, "level {level} tick {tick}");
                assert_eq!(k.tris, c.tris, "level {level} tick {tick}");
            }
        }
    }

    #[test]
    fn the_draw_order_is_right_almost_everywhere() {
        // Wrong pixels per picture: shown although a triangle covering
        // the same pixel is nearer. Every 4th tick of the loop.
        println!("level   pictures  wrong px: avg  most  (shown owner, nearer owner) of the worst");
        for level in 1..=5 {
            let mut w = work();
            let (mut sum, mut most, mut at, mut pics) = (0u64, 0u32, (0, Vec::new()), 0u64);
            let mut shot = None;
            for tick in (0..LOOP).step_by(4) {
                let (k, _) = check(&mut w, level, tick);
                sum += u64::from(k.wrong);
                pics += 1;
                if k.wrong > most {
                    most = k.wrong;
                    at = (tick, k.pairs.clone());
                    let mut f = k.frame;
                    for (i, &m) in k.mask.iter().enumerate() {
                        if m {
                            f.0[i] = 250;
                        }
                    }
                    shot = Some(f);
                }
            }
            // The worst picture, its wrong pixels in magenta (250 is no
            // palette colour).
            if let Some(f) = shot {
                save_frame(&shots().join(format!("island-check-{level}.png")), &f, 4);
            }
            println!(
                "{:>5}   {pics:>8}  {:>13}  {most:>4}  tick {} {:?}",
                LEVEL_NAMES[level as usize - 1],
                sum / pics,
                at.0,
                at.1
            );
            assert!(most <= 160, "level {level}: {most} wrong pixels at {at:?}");
        }
    }
}
