//! The final round's checks for the picture reviewer's findings
//! (F1 to F10, kept outside the repository): each test failed on the
//! builders' code (or could not be built against it, where it needs a
//! new read of the game) and passes now.

mod common;

use common::autopilot;
use drive3d::camera::Chase;
use drive3d::car::{Car, Controls};
use drive3d::detail::Detail;
use drive3d::draw::{self, Opts, PALETTE, col, group};
use drive3d::land::{Ground, LAND, Stretch};
use drive3d::look::Look;
use drive3d::maths::yaw_of;
use drive3d::menu::Menu;
use drive3d::play::{Input, Play};
use drive3d::road::{POINTS, ROAD};
use drive3d::scene::Painter;
use drive3d::things::{THINGS, kind};
use drive3d::tune::{GRAVEL, LOW_GRASS, M, PUFFS};
use game_kit::{Frame, H, W, text_width};

const NONE: Opts = Opts {
    line: false,
    plain: false,
};

fn picture(p: &Play, d: Detail, opts: Opts, menu: Option<&Menu>) -> Box<Frame> {
    let mut p = *p;
    p.detail = d;
    let mut paint = Box::new(Painter::new());
    let mut f = Box::new(Frame([0; W * H]));
    let look = Look::new(&p, 1);
    draw::picture(&mut f, &mut paint, &look, menu, opts, &mut ());
    f
}

fn count(f: &Frame, c: u8) -> usize {
    f.0.iter().filter(|&&x| x == c).count()
}

fn drive_to<F: Fn(&Play) -> bool>(stop: F) -> Vec<Play> {
    let mut p = Play::new();
    let mut out = vec![p];
    for _ in 0..2_000 {
        let i = autopilot(&p, 20 * M / 30, 14 * M / 30);
        p.step(&i);
        out.push(p);
        if stop(&p) {
            break;
        }
    }
    out
}

fn hold(c: Controls) -> Input {
    Input {
        controls: c,
        ..Input::default()
    }
}

// ---- F1: the dust ----------------------------------------------------

/// The step the car leaves the ground at the crest, the dust it threw
/// is still there (it used to vanish all at once and come back on
/// landing).
#[test]
fn finding_f1_the_dust_does_not_vanish_when_the_car_takes_off() {
    let run = drive_to(|p| p.flights > 0 && !p.car.air);
    let at = run
        .windows(2)
        .position(|w| !w[0].car.air && w[1].car.air)
        .expect("the car flies at the crest")
        + 1;
    for d in [Detail::Mid, Detail::High] {
        let before = count(&picture(&run[at - 1], d, NONE, None), col::DUST);
        let flying = count(&picture(&run[at], d, NONE, None), col::DUST);
        assert!(before > 0, "{d:?}: dust before the crest");
        assert!(flying > 0, "{d:?}: dust gone on the step the car flies");
    }
}

/// Reversing throws no dust (the old puffs, pulled back along the
/// speed, went into the car's body).
#[test]
fn finding_f1_reversing_throws_no_dust() {
    let mut p = Play::new();
    for _ in 0..90 {
        p.step(&hold(Controls {
            brake: 256,
            ..Controls::default()
        }));
    }
    assert!(p.car.fwd_side().0 < -400, "reversing at speed");
    for d in [Detail::Low, Detail::Mid, Detail::High] {
        assert_eq!(count(&picture(&p, d, NONE, None), col::DUST), 0, "{d:?}");
    }
}

/// A puff stays where it was made, ages one step at a time and ends;
/// at most `PUFFS` are kept, and LOW, MID and HIGH draw the newest 2,
/// 4 and 8.
#[test]
fn finding_f1_puffs_stay_in_the_world_and_end_by_age() {
    let run = drive_to(|p| p.steps >= 120);
    assert_eq!(run.last().map(|p| p.car.surface), Some(GRAVEL));
    let mut seen = 0;
    for w in run.windows(2) {
        for (a, b) in w[0].dust.iter().zip(w[1].dust.iter()) {
            if a.age > 0 && b.age == a.age + 1 {
                assert_eq!(a.at, b.at, "a puff moved");
                seen += 1;
            }
        }
    }
    assert!(seen > 100, "puffs followed: {seen}");
    let p = run.last().expect("run");
    let live = p.dust.iter().filter(|q| q.age > 0).count();
    assert!(live > 0 && live <= PUFFS);
    for (d, n) in [(Detail::Low, 2), (Detail::Mid, 4), (Detail::High, 8)] {
        let mut q = *p;
        q.detail = d;
        let look = Look::new(&q, 1);
        let drawn = (0..PUFFS)
            .filter(|&k| drive3d::scene::puff(&look, k).is_some())
            .count();
        assert_eq!(drawn, live.min(n), "{d:?}");
    }
}

// ---- F2: blocked with no visible reason -------------------------------

/// Driving into the hut with the throttle held: after a second the
/// picture says how to get out (it used to show a car standing still
/// and nothing more).
#[test]
fn finding_f2_a_blocked_car_shows_how_to_get_out() {
    let hut = THINGS
        .iter()
        .find(|t| t.kind == kind::HUT)
        .expect("the hut");
    let (hx, hz) = hut.at();
    let (sx, sz) = (hx - 12 * M, hz);
    let yaw = yaw_of(hx - sx, hz - sz).expect("a way");
    let mut p = Play::new();
    p.car = Car::standing(&Stretch, sx, sz, yaw);
    p.cam = Chase::behind(&p.car, &Stretch);
    let push = hold(Controls {
        throttle: 256,
        ..Controls::default()
    });
    let hint_rows = |p: &Play| {
        let f = picture(p, Detail::Mid, NONE, None);
        f.0[28 * W..40 * W]
            .iter()
            .filter(|&&c| c == col::WARN)
            .count()
    };
    let mut first_hit = None;
    for n in 0..150 {
        p.step(&push);
        if first_hit.is_none() && p.hits > 0 {
            first_hit = Some(n);
            assert_eq!(hint_rows(&p), 0, "no hint at the first touch");
        }
    }
    assert!(
        first_hit.is_some_and(|n| n < 100),
        "the car reaches the hut"
    );
    assert!(p.car.speed() < 137, "the car is stopped");
    assert!(hint_rows(&p) > 20, "the hint shows");
}

// ---- F3: the cut's colour ----------------------------------------------

/// The cut inside the second corner is low grass, drawn in a lighter,
/// yellower green than the grass, and nothing like the road's browns.
#[test]
fn finding_f3_the_cut_is_low_grass_in_a_light_green() {
    let (entry, end) = ROAD.piece_points(8);
    let mut low = 0;
    for k in entry..=end {
        let a = ROAD.heading[k];
        // 10 m to the left of the line (the corner's inside).
        let off = -10 * M;
        let x = ROAD.x[k] + (i64::from(off) * i64::from(game_3d::cos_fine(a)) / 16_384) as i32;
        let z = ROAD.z[k] - (i64::from(off) * i64::from(game_3d::sin_fine(a)) / 16_384) as i32;
        if LAND.at(x, z).surface == LOW_GRASS {
            low += 1;
        }
    }
    assert!(
        low * 2 > end - entry,
        "low grass at {low} of {} points",
        end - entry + 1
    );
    let rgb = |c: u16| {
        (
            (c >> 11) * 255 / 31,
            ((c >> 5) & 63) * 255 / 63,
            (c & 31) * 255 / 31,
        )
    };
    for light in 0..3 {
        let (r, g, b) = rgb(PALETTE[usize::from(draw::land(group::LOW_GRASS, light, 0))]);
        let (gr, gg, _) = rgb(PALETTE[usize::from(draw::land(group::GRASS, light, 0))]);
        let (vr, vg, vb) = rgb(PALETTE[usize::from(draw::land(group::VERGE, light, 0))]);
        // Green: more green than red, far more than blue (the verge,
        // a brown, has as much red as green).
        assert!(g > r + 10 && g > b * 2, "green: {r} {g} {b}");
        assert!(vr >= vg, "the verge is brown: {vr} {vg} {vb}");
        assert!(g > gg && r > gr, "lighter and yellower than grass");
        let apart = r.abs_diff(vr) + g.abs_diff(vg) + b.abs_diff(vb);
        assert!(apart > 30, "far from the verge: {apart}");
    }
}

// ---- F4: the shadow on bent land ---------------------------------------

/// The car's shadow is left out where the land under it bends: along
/// the land block's left edge the drawn picture has no shadow colour
/// where the plate would cut into the land.
#[test]
fn finding_f4_no_shadow_plate_on_bent_land() {
    let mut bent = 0;
    let mut kept = 0;
    let (w, _) = Stretch.size();
    for zi in 0..60 {
        let z = 40 * M + zi * 4 * M;
        for x in [9 * M, w - 9 * M] {
            let mut p = Play::new();
            p.car = Car::standing(&Stretch, x, z, 0);
            p.cam = Chase::behind(&p.car, &Stretch);
            let f = picture(&p, Detail::Mid, NONE, None);
            let shadow =
                f.0.iter()
                    .filter(|&&c| (col::SHADOW..col::SKY).contains(&c))
                    .count();
            // The land's own bend under the plate (as the drawing
            // measures it: the middle against the corners' mean).
            let h = |dx: i32, dz: i32| Stretch.at(x + dx, z + dz).height;
            let (l, s) = (M * 21 / 10, M * 8 / 10);
            let mean = (h(-s, -l) + h(s, -l) + h(s, l) + h(-s, l)) / 4;
            if (h(0, 0) - mean).abs() > M / 4 {
                bent += 1;
                assert_eq!(shadow, 0, "a shadow on bent land at z {}", z / M);
            } else {
                kept += usize::from(shadow > 0);
            }
        }
    }
    assert!(bent > 0, "some places bend");
    assert!(kept > 0, "flat places keep their shadow");
}

// ---- F6: the finish ---------------------------------------------------

/// A gate stands over the finish (it used to be marked only by the
/// text).
#[test]
fn finding_f6_a_gate_stands_over_the_finish() {
    let last = POINTS - 1;
    let gate = THINGS.iter().any(|t| {
        let (x, z) = t.at();
        t.kind == kind::BANNER
            && (i64::from(x - ROAD.x[last]).pow(2) + i64::from(z - ROAD.z[last]).pow(2))
                < i64::from(M).pow(2)
    });
    assert!(gate);
}

// ---- F9: words away from the edges --------------------------------------

/// No word (nor its shadow) comes within 4 pixels of the picture's
/// edges: the level name, the time, the speed, the line of numbers.
#[test]
fn finding_f9_words_keep_four_pixels_from_the_edges() {
    let words = [col::TEXT, col::SHADE, col::GOLD, col::WARN];
    let run = drive_to(|p| p.finished.is_some());
    let picks = [0, run.len() / 3, run.len() / 2, run.len() - 1];
    for &i in &picks {
        for d in [Detail::Low, Detail::Mid, Detail::High] {
            for line in [false, true] {
                let f = picture(&run[i], d, Opts { line, plain: false }, None);
                for y in 0..H {
                    for x in 0..W {
                        let edge = x < 4 || y < 4 || x >= W - 4 || y >= H - 4;
                        assert!(
                            !(edge && words.contains(&f.0[y * W + x])),
                            "step {i} {d:?} line {line}: a word at ({x}, {y})"
                        );
                    }
                }
            }
        }
    }
}

// ---- F10: the menu's mark ---------------------------------------------

/// The level in use is marked with "<" (a star read as a cross).
#[test]
fn finding_f10_the_menu_marks_the_level_in_use_with_an_arrow() {
    for now in [Detail::Low, Detail::Mid, Detail::High] {
        let mut m = Menu::open(now);
        m.at = 3;
        let p = Play::new();
        let f = picture(&p, now, NONE, Some(&m));
        let i = now as usize;
        let line = Menu::LINES[i];
        let (x, y) = (
            48i16 + 14 + text_width(line) as i16 + 3,
            32i16 + 14 + i as i16 * 8,
        );
        let mut g = f.clone();
        g.text(x, y, "<", col::TEXT);
        assert!(f.0 == g.0, "{now:?}: the mark is not \"<\"");
        let mut s = f.clone();
        s.text(x, y, "*", col::TEXT);
        assert!(f.0 != s.0, "{now:?}: the mark is a star");
    }
}
