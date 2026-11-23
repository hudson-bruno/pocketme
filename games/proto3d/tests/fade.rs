//! The camera fault of AREAS-SPEC.md section 0: a solid box between the
//! eye and the hero must not hide the hero. Written first, against the
//! camera as it was, where it failed (the floating block filled the
//! view); it passes since FADE blocks shrink away. The other tests are
//! the spec's section 0 list.

mod common;

use common::Lcg;
use game_kit::{Frame, button};
use proto3d::camera::FULL;
use proto3d::draw::Painter;
use proto3d::hero::{Hero, Pose};
use proto3d::island::{BLOCKS, FLOAT, ISLAND, World, body_fits, first_hit_mask, flag, inside_any};
use proto3d::maths::{cm, sub};
use proto3d::models::{self, MATS, mat};
use proto3d::pause::Menu;
use proto3d::play::{Detail, Play};
use proto3d::step::Pad;
use proto3d::tune::{DIST, DIST_MIN, WALL_GAP};

fn picture(p: &Play) -> Box<Frame> {
    let mut paint = Box::new(Painter::new());
    let mut f = Box::new(Frame::new());
    paint.picture(&mut f, p, &Menu::new(), false, 1);
    f
}

/// Pixels of the hero's own materials (top, cap, skin, trousers).
fn hero_pixels(f: &Frame) -> usize {
    f.0.iter()
        .filter(|&&c| {
            c < 96 && {
                let m = c % models::MATS;
                m == mat::HERO || m == mat::CAP || m == mat::SKIN || m == mat::PANTS
            }
        })
        .count()
}

/// Feet spots (cm) on every walkable top of the test island: a 1 m grid,
/// and a 25 cm ring round the floating block's footprint (where the
/// fault was). Only spots where the body fits.
fn spots() -> Vec<[i16; 3]> {
    let mut out = Vec::new();
    for b in BLOCKS.iter().filter(|b| b.flags & flag::WALK != 0) {
        let mut x = b.lo[0] + 50;
        while x <= b.hi[0] - 25 {
            let mut z = b.lo[2] + 50;
            while z <= b.hi[2] - 25 {
                out.push([x, 0, z]);
                z += 100;
            }
            x += 100;
        }
    }
    let f = &BLOCKS[usize::from(FLOAT)];
    for x in (f.lo[0] - 100..=f.hi[0] + 100).step_by(25) {
        for z in (f.lo[2] - 100..=f.hi[2] + 100).step_by(25) {
            let near_edge = x < f.lo[0] || x > f.hi[0] || z < f.lo[2] || z > f.hi[2];
            if near_edge {
                out.push([x, 0, z]);
            }
        }
    }
    // The feet on the highest walkable top under each spot where the body
    // fits there.
    out.into_iter()
        .filter_map(|[x, _, z]| {
            let (xs, zs) = (cm(x), cm(z));
            let (top, y) = proto3d::island::top_under(&BLOCKS, xs, zs, i32::MAX)?;
            let p = [xs, y, zs];
            body_fits(&BLOCKS, p).then(|| {
                let _ = top;
                [x, (i64::from(y) * 25 / 1024) as i16, z]
            })
        })
        .collect()
}

/// The hero stands at `at` (cm) facing `yaw`, the camera behind it, then
/// eight still steps so the camera settles.
fn standing(at: [i16; 3], yaw: u16, d: Detail) -> Play {
    let mut p = Play::on(&ISLAND, d);
    let pos = [cm(at[0]), 0, cm(at[2])];
    let (top, y) = proto3d::island::top_under(&BLOCKS, pos[0], pos[2], i32::MAX).unwrap();
    p.hero = Hero::new([pos[0], y, pos[2]], top);
    p.hero.yaw = yaw;
    p.cam.yaw = yaw;
    p.cam.snap(&p.hero, ISLAND.blocks, ISLAND.props, d.level());
    for _ in 0..8 {
        p.step(&Pad::NONE);
    }
    p
}

/// The brief's test: the hero over every walkable top of ISLAND, the
/// camera turned in 16 steps round it; the hero's own pixels must be in
/// the picture every time.
#[test]
fn no_box_hides_the_hero_anywhere_on_the_island() {
    let spots = spots();
    let mut bad = Vec::new();
    let mut fewest = usize::MAX;
    for at in &spots {
        for k in 0..16u16 {
            let p = standing(*at, k * 4096, Detail::Mid);
            if p.pose() == Pose::Gone {
                continue;
            }
            let n = hero_pixels(&picture(&p));
            fewest = fewest.min(n);
            if n < 12 {
                bad.push((*at, k, n));
            }
        }
    }
    println!(
        "{} spots x 16 turns, fewest hero pixels {fewest}, {} pictures hide the hero",
        spots.len(),
        bad.len()
    );
    assert!(
        bad.is_empty(),
        "the hero is hidden: {:?}",
        &bad[..bad.len().min(20)]
    );
}

/// The probe's pad list (AREAS-SPEC.md, section 0): idle, run up with a
/// jump, idle, R (turn the camera), run right past the floating block.
fn probe_pads() -> Vec<Pad> {
    let mut held = Vec::new();
    held.extend([0u32; 68]);
    for i in 0..60 {
        held.push(button::UP | if (30..42).contains(&i) { button::A } else { 0 });
    }
    held.extend([0u32; 10]);
    held.push(button::R);
    held.extend([0u32; 9]);
    held.extend([button::RIGHT; 30]);
    held.extend([0u32; 12]);
    let mut last = 0;
    held.into_iter()
        .map(|h| {
            let p = Pad {
                held: h,
                pressed: h & !last,
                released: last & !h,
                ..Pad::NONE
            };
            last = h;
            p
        })
        .collect()
}

/// FLOAT's fade (FLOAT is the island's first FADE block).
fn float_fade(p: &Play) -> u16 {
    p.cam.fade_of(&BLOCKS, usize::from(FLOAT))
}

/// ISLAND without its creatures. The probe's run up goes through the
/// first burr's patrol, and the burr knocks the hero back there (checked
/// in `the_probe_meets_the_first_burr`), so the camera's probe runs on
/// an ISLAND without them.
fn bare_island() -> &'static World {
    Box::leak(Box::new(World {
        critters: &[],
        ..ISLAND
    }))
}

/// The probe with and without the creatures: the same until the step the
/// first burr knocks the hero back.
#[test]
fn the_probe_meets_the_first_burr() {
    let mut with = Play::new();
    let mut without = Play::on(bare_island(), Detail::Mid);
    let mut first = None;
    for (n, pad) in probe_pads().iter().enumerate() {
        with.step(pad);
        without.step(pad);
        if with.hero != without.hero {
            first = Some(n);
            assert_eq!(with.events.life.hit(), Some(0), "step {n}");
            break;
        }
    }
    println!("the burr knocks the probe back at step {first:?}");
    assert!(first.is_some());
}

#[test]
fn the_floating_block_fades_when_it_hides_the_hero() {
    let mut p = Play::on(bare_island(), Detail::Mid);
    let mut first_hiding = None;
    let mut gone_at = None;
    for (n, pad) in probe_pads().iter().enumerate() {
        p.step(pad);
        if !(166..=189).contains(&n) {
            continue;
        }
        let f = float_fade(&p);
        if f < FULL && first_hiding.is_none() {
            first_hiding = Some(n);
        }
        if f == 0 && gone_at.is_none() {
            gone_at = Some(n);
        }
        let d = p.cam.dist();
        assert!(
            d >= i64::from(cm(400)),
            "step {n}: the eye is {d} subs from the aim"
        );
    }
    let (a, b) = (
        first_hiding.expect("FLOAT never hid the hero"),
        gone_at.expect("never gone"),
    );
    println!("FLOAT starts to fade at step {a}, gone at step {b}");
    assert!(
        b - a < 6,
        "gone {} steps after it first hid the hero",
        b - a
    );
}

#[test]
fn fade_blocks_never_pin_the_eye() {
    // Random play as in the camera's eye test. Whenever the eye is nearer
    // than DIST_MIN + WALL_GAP, a FADE block alone on its line (from the
    // aim out to the full distance) would mean the FADE block pinned it.
    // (Near for other reasons is fine: a block that is not FADE, or the
    // eye catching up after a fall.)
    let near = i64::from(sub(DIST_MIN + WALL_GAP));
    let mut rng = Lcg(3);
    let (mut close, mut fading, mut held_by_fade) = (0, 0, 0);
    for d in [Detail::Low, Detail::High] {
        let mut play = Play::on(&ISLAND, d);
        let mut last = Pad::NONE;
        for n in 0..9_000 {
            last = rng.pad(&last);
            play.step(&last);
            assert_eq!(inside_any(&BLOCKS, play.eye()), None, "step {n}");
            if float_fade(&play) < FULL {
                fading += 1;
            }
            if play.cam.dist() >= near {
                continue;
            }
            close += 1;
            // Falling under the held aim (off the island): the eye is
            // catching up after a fall, and may be lifted out of a FADE
            // block's box on its way (camera step 4). Found once the
            // random pad pressed every button (rules review F7).
            if play.hero.ground.is_none() && play.hero.pos[1] < play.aim()[1] {
                continue;
            }
            // The eye's own line from the aim, out to the full distance.
            let (aim, eye, dist) = (play.aim(), play.eye(), play.cam.dist().max(1));
            let out: [i32; 3] = std::array::from_fn(|k| {
                aim[k] + (i64::from(eye[k] - aim[k]) * i64::from(sub(DIST)) / dist) as i32
            });
            let wall = first_hit_mask(&BLOCKS, aim, out, flag::FADE | flag::HIDE);
            let any = first_hit_mask(&BLOCKS, aim, out, flag::HIDE);
            if wall.is_none() && any.is_some() {
                held_by_fade += 1;
                println!("step {n}: eye {dist} subs from the aim, only a FADE block on its line");
            }
        }
    }
    println!(
        "{close} close steps, {held_by_fade} with only a FADE block on the line; {fading} steps with FLOAT fading"
    );
    assert_eq!(held_by_fade, 0);
}

/// The share of pixels in FLOAT's top shades (its top and its rim).
fn float_top_share(f: &Frame) -> usize {
    let n =
        f.0.iter()
            .filter(|&&c| c < 96 && (c % MATS == mat::BLOCK_TOP || c % MATS == mat::RIM_BLOCK))
            .count();
    n * 100 / f.0.len()
}

#[test]
fn frame390_like() {
    // The probe's step 175 at MID: in the fault, FLOAT's top filled the
    // picture.
    let mut p = Play::new();
    for pad in &probe_pads()[..=175] {
        p.step(pad);
    }
    let f = picture(&p);
    let share = float_top_share(&f);
    let hero = hero_pixels(&f);
    println!("step 175: {share}% of the pixels on FLOAT's top, {hero} hero pixels");
    assert!(share < 25, "{share}% of the picture is FLOAT's top");
    assert!(hero >= 12, "the hero is hidden ({hero} pixels)");
}
