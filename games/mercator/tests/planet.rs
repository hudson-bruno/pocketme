//! Step G1: MERCATOR's planet mode, its frame and its land (SPEC
//! "GROUNDSIDE" section 3): the ring's rules, entering and leaving, the
//! sites over many seeds, and the land's picture (no cracks, no wrong
//! order) at the three detail levels.
//!
//! `PLANET_SEEDS` sets the seeds per look of the site sweep (default 24
//! in a debug build, 1,000 in a release build).

mod common;

use common::below::{below_in, dive, docked_at, land, system_with_look};
use game_kit::{Frame, button};
use mercator::app::{Scene, seed_of};
use mercator::fixed::{Mat3, ONE14, dot, length, scale14, sub};
use mercator::galaxy;
use mercator::planet::land::{self, Paint, ToFrame};
use mercator::planet::lander::Phase;
use mercator::planet::place::{self, MAX_TRIES};
use mercator::planet::site;
use mercator::planet::space::{self, Ring};
use mercator::planet::tune::{
    ENTRY_TICKS, FAR_R, GIANT, LIFT_FUEL, LIFT_HOLD, MESA_LIFT, PAD_R, PLINTH_BASE, RING_ALT,
    RING_CLEAR, SHAPE, START_UP, TAKEOFF_TICKS, TERRACE,
};
use mercator::planet::{self, Below};
use mercator::poly::Stats;
use mercator::scene::Eye;
use mercator::world::{self, Career, KM, Kind, M, MAX_SPEED, Mode};

/// The looks with a surface (all but the giant).
const SURFACES: [u8; 7] = [0, 1, 2, 3, 5, 6, 7];

#[test]
fn the_planet_state_fits_its_budget() {
    let n = core::mem::size_of::<Below>();
    println!("Below: {n} B");
    assert!(n <= 2048, "Below is {n} B, more than 2,048");
}

fn seeds() -> u32 {
    std::env::var("PLANET_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(if cfg!(debug_assertions) { 24 } else { 1000 })
}

/// Over many seeds and every look: heights bounded, the pad flat and
/// dry, placement's loops bounded, every seam and the wreck reachable
/// by the walk's rules, nothing on the pad, the start high over the
/// ground, the same site from the same seed.
#[test]
fn sites_keep_their_rules_over_many_seeds() {
    let n = seeds();
    let half = 75i32; // 150 m round the pad on the 2 m grid
    let side = (2 * half + 1) as usize;
    let mut h = vec![0i32; side * side];
    let mut seen = vec![0u8; side * side];
    let mut queue = vec![0u32; 2 * side * side];
    let (mut worst_tries, mut low_start) = (0u16, i32::MAX);
    for &look in SURFACES.iter() {
        let sh = &SHAPE[usize::from(look)];
        let bound = sh.amp.iter().sum::<i32>() + MESA_LIFT + TERRACE + PLINTH_BASE + 3 * M;
        let mut misses = 0;
        for i in 0..n {
            let seed = i.wrapping_mul(0x9E37_79B9) ^ u32::from(look) << 24;
            let made = site::make(seed, look).expect("a site");
            assert_eq!(
                site::make(seed, look),
                Some(made),
                "the same seed, the same site"
            );
            let mut s = made;
            let p = place::place(&mut s, (i % 4) as u8);
            // Heights over the far ring's reach.
            let reach = FAR_R[2];
            for z in (-reach..=reach).step_by((16 * M) as usize) {
                for x in (-reach..=reach).step_by((16 * M) as usize) {
                    let y = site::height(&s, x, z);
                    assert!(
                        y.abs() <= bound,
                        "look {look} seed {i}: height {y} at {x},{z}"
                    );
                }
            }
            // The pad: flat, dry.
            for z in [-6, -2, 2, 6] {
                for x in [-6, -2, 2, 6] {
                    assert_eq!(
                        site::height(&s, x * M, z * M),
                        s.pad,
                        "look {look} seed {i}"
                    );
                }
            }
            assert!(s.pad > s.wet, "look {look} seed {i}: the pad under water");
            // Placement.
            assert!(
                p.tries <= MAX_TRIES,
                "look {look} seed {i}: {} tries",
                p.tries
            );
            worst_tries = worst_tries.max(p.tries);
            assert_eq!(p.drones, (2 + (i % 4) as u8).min(4));
            let on_pad = |q: [i32; 2]| {
                let r = ((i64::from(q[0])).pow(2) + i64::from(q[1]).pow(2)).isqrt();
                r <= i64::from(PAD_R)
            };
            for q in p.seams.iter().chain(core::iter::once(&p.wreck)) {
                assert!(!on_pad(*q), "look {look} seed {i}: a thing on the pad");
            }
            for q in p.boulders.iter().flatten() {
                assert!(!on_pad(*q), "look {look} seed {i}: a boulder on the pad");
            }
            let over = p.start[1] - site::surface(&s, p.start[0], p.start[2]);
            assert_eq!(over, START_UP);
            low_start = low_start.min(over);
            let missed = place::reach(&s, &p, half, &mut h, &mut seen, &mut queue);
            if missed != 0 {
                misses += 1;
                println!("look {look} seed {i}: not reached {missed:05b}");
            }
        }
        assert_eq!(
            misses, 0,
            "look {look}: {misses} of {n} sites with a thing out of reach"
        );
    }
    println!(
        "{n} seeds x 7 looks: most tries {worst_tries} of {MAX_TRIES}, start at least {} m up",
        low_start / M
    );
}

/// The ring keeps off the station in every surface system of ten
/// galaxies, and sits RING_ALT over the ground; the giant has none.
#[test]
fn the_ring_keeps_clear_of_the_station() {
    let mut d = docked_at(1, 0);
    let (mut rings, mut close, mut giants) = (0, 0, 0);
    for g in 1..=10u8 {
        let seed = seed_of(g);
        for id in 0..galaxy::SYSTEMS as u8 {
            let mut c = Career::new(seed);
            c.system = id;
            c.target = id;
            d.app.world.begin(c);
            let w = &d.app.world;
            match space::site(w) {
                None => {
                    assert_eq!(w.sys.look, GIANT);
                    giants += 1;
                }
                Some(s) => {
                    assert_ne!(w.sys.look, GIANT);
                    rings += 1;
                    let alt = length(s.ring) - w.bodies.planet_r;
                    assert!((alt - RING_ALT).abs() < 4 * M, "ring {} m up", alt / M);
                    if length(sub(s.ring, w.bodies.station_pos)) <= RING_CLEAR {
                        close += 1;
                    }
                    // The site's direction faces the sun (light points
                    // from the sun).
                    assert!(dot(s.dir, w.bodies.light) < 0, "the site on the night side");
                }
            }
        }
    }
    println!(
        "{rings} rings, {giants} giants, {close} within {} km",
        RING_CLEAR / KM
    );
    assert!(rings > 0 && giants > 0);
    assert_eq!(close, 0);
}

/// A world in flight at a surface system with the ship `up` over the
/// ring's middle, `out` across it, going along `nose`; in the cockpit
/// view (from the chase eye the site's bracket can land on the hint's
/// row, which the hint's pixel check would count as no hint).
fn at_ring(up: i32, out: i32, nose: [i32; 3]) -> (common::Driver, space::Site) {
    let (g, id) = system_with_look(0);
    let mut d = docked_at(g, id);
    d.app.opts.eye = Eye::Cockpit;
    d.app.sel = 0;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Flight);
    let s = space::site(&d.app.world).unwrap();
    let w = &mut d.app.world;
    w.ships = [None; world::SHIPS];
    let (e1, _) = space::across(s.dir);
    w.me.pos = world::add(world::add(s.ring, scale14(s.dir, up)), scale14(e1, out));
    w.me.rot = Mat3::looking(mercator::fixed::norm(nose, ONE14), e1);
    w.me.speed = 600;
    w.me.throttle = 600;
    w.me.cruise = false;
    w.me.boost = false;
    (d, s)
}

/// One step of `m` along the nose from the ship's place, checked by the
/// ring rule.
fn check_step(d: &mut common::Driver, m: i32) -> Ring {
    let w = &mut d.app.world;
    let old = w.me.pos;
    w.me.pos = world::add(old, scale14(w.me.rot.col(2), m));
    space::ring_check(w, old)
}

#[test]
fn the_ring_lets_in_only_by_its_rules() {
    let down = |s: &space::Site| scale14(s.dir, -ONE14);
    // In: nose down, through the middle.
    let (_, s) = at_ring(M, 0, [0, 0, 1]);
    let (mut d, _) = at_ring(M, 0, down(&s));
    assert_eq!(check_step(&mut d, 2 * M), Ring::Enter);
    // Going up through it: no.
    let (mut d, _) = at_ring(-M, 0, s.dir);
    assert_eq!(check_step(&mut d, 2 * M), Ring::Miss);
    // Down but 350 m out: no.
    let (mut d, _) = at_ring(M, 350 * M, down(&s));
    assert_eq!(check_step(&mut d, 2 * M), Ring::Miss);
    // Down but not reaching the plane yet: no.
    let (mut d, _) = at_ring(3 * M, 0, down(&s));
    assert_eq!(check_step(&mut d, 2 * M), Ring::Miss);
    // Down at 60 degrees from straight down: the nose rule.
    let (e1, _) = space::across(s.dir);
    let slant = world::add(scale14(s.dir, -ONE14 / 2), scale14(e1, 14_189));
    let (mut d, _) = at_ring(M, 0, slant);
    assert_eq!(
        check_step(&mut d, 4 * M),
        Ring::Refused("NOSE DOWN INTO THE RING")
    );
    // At 30 degrees: fine.
    let slant = world::add(scale14(s.dir, -14_189), scale14(e1, ONE14 / 2));
    let (mut d, _) = at_ring(M, 0, slant);
    assert_eq!(check_step(&mut d, 4 * M), Ring::Enter);
    // Fuel for the lift-off.
    let (mut d, _) = at_ring(M, 0, down(&s));
    d.app.world.car.fuel = LIFT_FUEL - 1;
    assert_eq!(
        check_step(&mut d, 2 * M),
        Ring::Refused("NEED 0.3 LY TO LIFT OFF")
    );
    d.app.world.car.fuel = LIFT_FUEL;
    d.app.world.me.pos = world::add(s.ring, scale14(s.dir, M));
    assert_eq!(check_step(&mut d, 2 * M), Ring::Enter);
    // Too fast: over the top speed, cruising, boosting.
    for k in 0..3 {
        let (mut d, _) = at_ring(M, 0, down(&s));
        let w = &mut d.app.world;
        match k {
            0 => w.me.speed = w.top() + 1,
            1 => w.me.cruise = true,
            _ => w.me.boost = true,
        }
        assert_eq!(
            check_step(&mut d, 2 * M),
            Ring::Refused("TOO FAST FOR THE RING"),
            "{k}"
        );
    }
    // A raider hunting within 3 km closes it, before every other rule.
    let (mut d, _) = at_ring(M, 0, slant);
    let w = &mut d.app.world;
    let near = world::add(w.me.pos, scale14(e1, 2 * KM));
    let r = w
        .add_ship(Kind::Raider, near, Mode::Hunt)
        .expect("room for a raider");
    assert_eq!(w.ships[r].unwrap().target, world::PLAYER);
    w.car.fuel = 0;
    w.me.speed = MAX_SPEED * 2;
    assert_eq!(
        check_step(&mut d, 4 * M),
        Ring::Refused("RING CLOSED: RAIDERS NEAR")
    );
    // Docked: never.
    let (g, id) = system_with_look(0);
    let d = docked_at(g, id);
    assert_eq!(
        space::ring_check(&d.app.world, d.app.world.me.pos),
        Ring::Miss
    );
}

#[test]
fn a_refusal_is_noted_and_the_ship_flies_on() {
    let (_, s) = at_ring(M, 0, [0, 0, 1]);
    let (mut d, _) = at_ring(20 * M, 0, scale14(s.dir, -ONE14));
    d.app.world.car.fuel = 1;
    for _ in 0..40 {
        d.idle(1);
        if d.app.note.is_some() {
            break;
        }
    }
    assert_eq!(d.app.scene, Scene::Flight);
    assert_eq!(d.app.note.map(|n| n.0), Some("NEED 0.3 LY TO LIFT OFF"));
}

#[test]
fn the_giant_has_no_site() {
    let (g, id) = system_with_look(GIANT);
    let d = docked_at(g, id);
    assert!(space::site(&d.app.world).is_none());
    assert!(site::make(d.app.world.sys.h, GIANT).is_none());
    let mut t = mercator::hud::Txt::new();
    space::dock_line(&mut t, &d.app.world, false);
    assert_eq!(t.as_str(), "SITE  NO SURFACE");
}

/// The whole world unchanged, the ship too.
fn same(a: &world::World, b: &world::World) -> bool {
    a.same_but_ship(b) && a.me == b.me && a.car.fuel == b.car.fuel
}

/// Entering freezes space; the entry, the flown landing (the scripted
/// pilot) and the take-off leave it as it was; on the ground a short
/// hold does nothing; a full hold takes off, and the take-off's end is
/// at the ring flying out, with only the ship and the lift's fuel
/// changed.
#[test]
fn entering_and_leaving_keeps_space_as_it_was() {
    for &look in SURFACES.iter() {
        let (g, id) = system_with_look(look);
        let mut d = docked_at(g, id);
        dive(&mut d, 30 * M);
        assert_eq!(d.app.scene, Scene::Below);
        assert!(d.app.below.on);
        assert_eq!(d.app.below.site.look, look);
        assert_eq!(d.app.below.phase, Phase::Entry);
        let at_entry = d.app.world.copy_for_test();
        // 30 pictures a second below, whatever the setting.
        d.app.opts.fast = true;
        assert_eq!(mercator::Mercator::wanted(&d.app).0, game_kit::rate::FPS30);
        d.idle(u32::from(ENTRY_TICKS));
        assert_eq!(d.app.below.phase, Phase::Fly, "look {look}");
        assert!(same(&d.app.world, &at_entry), "space moved in the entry");
        land(&mut d, 6000);
        assert_eq!(d.app.below.phase, Phase::Landed, "look {look}");
        assert!(d.app.below.touched.is_some_and(|l| l.down()));
        assert!(same(&d.app.world, &at_entry), "space moved while below");
        // A hold shorter than half a second, then let go.
        for _ in 0..LIFT_HOLD - 5 {
            d.hold(button::B);
        }
        d.idle(5);
        assert_eq!(d.app.scene, Scene::Below);
        assert_eq!(d.app.below.phase, Phase::Landed);
        assert!(same(&d.app.world, &at_entry));
        // The full hold, then the take-off's 3 s.
        for _ in 0..LIFT_HOLD + 2 {
            d.hold(button::B);
        }
        assert_eq!(d.app.below.phase, Phase::TakeOff, "look {look}");
        for _ in 0..u32::from(TAKEOFF_TICKS) + 4 {
            d.idle(1);
            if d.app.scene != Scene::Below {
                break;
            }
        }
        assert_eq!(d.app.scene, Scene::Flight, "look {look}");
        assert!(!d.app.below.on);
        let w = &d.app.world;
        assert!(
            w.same_but_ship(&at_entry),
            "look {look}: space changed by the visit"
        );
        assert_eq!(w.car.fuel, at_entry.car.fuel - LIFT_FUEL);
        let s = space::site(w).unwrap();
        let out = (dot(sub(w.me.pos, s.ring), s.dir) >> 14) as i32;
        assert!(
            (0..8 * M).contains(&out),
            "look {look}: {} m over the ring",
            out / M
        );
        assert!(dot(w.me.rot.col(2), s.dir) >> 14 > 16_000, "nose out");
        assert_eq!(w.me.throttle, MAX_SPEED / 2);
        // Flying out, never back in.
        d.idle(120);
        assert_eq!(d.app.scene, Scene::Flight);
        let w = &d.app.world;
        assert!(dot(sub(w.me.pos, s.ring), s.dir) >> 14 > i64::from(100 * M));
    }
}

#[test]
fn below_the_title_continues_below() {
    let mut d = below_in(5);
    d.idle(10);
    let b = d.app.below;
    // Pause, and the game is still below behind it.
    d.tap(button::START);
    assert_ne!(d.app.scene, Scene::Flight);
    assert!(d.app.below.on);
    assert_eq!(d.app.below.site, b.site);
}

/// A test's [`Paint`]: the triangles in painter's order, each pixel's
/// last and nearest depth (from the triangle's plane, sampled at the
/// pixel's middle), and the real fill into a frame.
struct Rec {
    frame: Box<Frame>,
    last: Vec<f64>,
    near: Vec<f64>,
    tris: u32,
}

const HOLE: u8 = 255;

impl Rec {
    fn new() -> Rec {
        let mut frame = Box::new(Frame::new());
        frame.clear(HOLE);
        Rec {
            frame,
            last: vec![f64::INFINITY; 160 * 120],
            near: vec![f64::INFINITY; 160 * 120],
            tris: 0,
        }
    }
}

impl Paint for Rec {
    fn tri(&mut self, c: &[[i32; 3]; 3], s: &[[i32; 2]; 3], colour: u8) -> u32 {
        self.tris += 1;
        // Depth by the triangle's plane: 1/z is linear on the screen.
        let p: Vec<(f64, f64)> = s
            .iter()
            .map(|q| (q[0] as f64 / 16.0, q[1] as f64 / 16.0))
            .collect();
        let iz: Vec<f64> = c.iter().map(|q| 1.0 / (q[2].max(1) as f64)).collect();
        let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
        if area.abs() > 1e-9 {
            let x0 = p
                .iter()
                .map(|q| q.0)
                .fold(f64::MAX, f64::min)
                .floor()
                .max(0.0) as usize;
            let x1 = p
                .iter()
                .map(|q| q.0)
                .fold(f64::MIN, f64::max)
                .ceil()
                .min(160.0) as usize;
            let y0 = p
                .iter()
                .map(|q| q.1)
                .fold(f64::MAX, f64::min)
                .floor()
                .max(0.0) as usize;
            let y1 = p
                .iter()
                .map(|q| q.1)
                .fold(f64::MIN, f64::max)
                .ceil()
                .min(96.0) as usize;
            for y in y0..y1 {
                for x in x0..x1 {
                    let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                    let w0 = ((p[1].0 - px) * (p[2].1 - py) - (p[2].0 - px) * (p[1].1 - py)) / area;
                    let w1 = ((p[2].0 - px) * (p[0].1 - py) - (p[0].0 - px) * (p[2].1 - py)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let z = 1.0 / (w0 * iz[0] + w1 * iz[1] + w2 * iz[2]);
                    let k = y * 160 + x;
                    self.last[k] = z;
                    self.near[k] = self.near[k].min(z);
                }
            }
        }
        // Step G2: the view's area (the chase camera looks down onto
        // land that runs on under the panel).
        mercator::poly::fill_tri(&mut self.frame, &planet::VIEW.area, s, colour)
    }
}

/// Holes: unpainted pixels with land above and below them in their
/// column. Wrong order: pixels whose last triangle is more than 2 %
/// farther than the nearest one there.
fn judge(r: &Rec) -> (u32, u32, u32) {
    let (mut holes, mut wrong, mut covered) = (0, 0, 0);
    for x in 0..160u16 {
        let col: Vec<bool> = (0..120u16).map(|y| r.frame.get(x, y) != HOLE).collect();
        let (Some(top), Some(bot)) = (col.iter().position(|&c| c), col.iter().rposition(|&c| c))
        else {
            continue;
        };
        for (y, _) in col
            .iter()
            .enumerate()
            .take(bot + 1)
            .skip(top)
            .filter(|(_, c)| !**c)
        {
            holes += 1;
            if std::env::var("PLANET_HOLES").is_ok() {
                println!("hole at {x},{y}");
            }
        }
    }
    for k in 0..160 * 120 {
        if r.last[k].is_finite() {
            covered += 1;
            if r.last[k] > r.near[k] * 1.02 + 4.0 {
                wrong += 1;
            }
        }
    }
    (holes, wrong, covered)
}

/// Step G2: the landing camera's places for the land's checks: behind
/// the ship at the landing start (120 m up, 150 m out, looking at the
/// pad), 60 m up 40 m out, 20 m over the pad, and 6 m over it facing
/// the other way.
fn chase_spots(b: &Below) -> Vec<([i32; 3], u16)> {
    let st = b.places.start;
    let to_pad = planet::yaw_to(-st[0], -st[2]);
    let pad = b.site.pad;
    let out = [st[0] * 40 / 150, 0, st[2] * 40 / 150];
    vec![
        ([st[0], st[1] + b.ride, st[2]], to_pad),
        ([out[0], pad + 60 * M, out[2]], to_pad),
        ([0, pad + 20 * M + b.ride, 0], to_pad.wrapping_add(0x3000)),
        ([0, pad + 6 * M + b.ride, 0], to_pad.wrapping_add(0x8000)),
    ]
}

#[test]
fn the_land_has_no_cracks_and_no_wrong_order() {
    let mut rows = String::new();
    for &look in SURFACES.iter() {
        let b: Below = below_in(look).app.below;
        for lv in 0..3 {
            let mut cams = Vec::new();
            for (at, yaw) in chase_spots(&b) {
                cams.push(("chase", planet::camera::chase_at(&b, at, yaw)));
            }
            for (at, yaw) in [
                ([0, 0], 0x2000u16),
                ([0, 0], 0xA000),
                ([9 * M, -4 * M], 0x6000),
            ] {
                cams.push(("walk", planet::walk_camera(&b, at, yaw)));
            }
            for (eye, cam) in cams {
                let mut r = Rec::new();
                let mut st = Stats::default();
                land::draw(
                    &mut r,
                    &planet::VIEW,
                    &cam,
                    &b.site,
                    lv,
                    b.sun,
                    256,
                    &mut st,
                );
                let (holes, wrong, covered) = judge(&r);
                rows += &format!(
                    "look {look} lv {lv} {eye:6} tris {:4} covered {covered:5} holes {holes} wrong {wrong}\n",
                    r.tris
                );
                assert_eq!(st.tris, r.tris);
                assert!(r.tris > 0);
                if std::env::var("PLANET_HOLES").is_err() {
                    assert_eq!(holes, 0, "look {look} level {lv} {eye}: {holes} holes");
                    assert!(
                        wrong * 200 <= covered,
                        "look {look} level {lv} {eye}: {wrong} of {covered} pixels in the wrong order"
                    );
                }
            }
        }
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/mercator");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("planet-order.txt"), &rows);
}

/// The real fill and ToFrame give the same picture as the recorder's
/// fill (the recorder stands for the game).
#[test]
fn the_recorder_paints_as_the_game_does() {
    let b: Below = below_in(0).app.below;
    let (at, yaw) = chase_spots(&b)[0];
    let cam = planet::camera::chase_at(&b, at, yaw);
    let mut r = Rec::new();
    let mut st = Stats::default();
    land::draw(&mut r, &planet::VIEW, &cam, &b.site, 1, b.sun, 256, &mut st);
    let mut f = Box::new(Frame::new());
    f.clear(HOLE);
    let mut st2 = Stats::default();
    let mut to = ToFrame {
        f: &mut f,
        area: planet::VIEW.area,
    };
    land::draw(
        &mut to,
        &planet::VIEW,
        &cam,
        &b.site,
        1,
        b.sun,
        256,
        &mut st2,
    );
    assert_eq!(st, st2);
    for y in 0..120 {
        assert_eq!(f.row(y), r.frame.row(y));
    }
}

/// Step G2: on the ground the panel says LANDED and HOLD B: LIFT OFF,
/// and the hold fills a bar under it.
#[test]
fn the_lift_off_hold_shows_on_the_panel() {
    let mut d = below_in(0);
    d.idle(u32::from(ENTRY_TICKS));
    land(&mut d, 6000);
    d.idle(2);
    let bar = |f: &Frame| {
        (84..127u16)
            .filter(|&x| f.get(x, 118) == mercator::art::ACCENT)
            .count()
    };
    let lit = |f: &Frame, x0: u16, y: u16, c: u8| (x0..x0 + 30).any(|x| f.get(x, y) == c);
    assert!(lit(&d.frame, 84, 99, mercator::art::GOOD), "LANDED");
    assert_eq!(bar(&d.frame), 0);
    for _ in 0..LIFT_HOLD / 2 {
        d.hold(button::B);
    }
    assert_eq!(d.app.below.hold, LIFT_HOLD / 2);
    assert_eq!(bar(&d.frame), 16, "half the bar after half the hold");
}

/// Whether the entry hint is on the picture: every pixel the hint
/// alone paints is there in its colour.
fn hint_shown(f: &Frame) -> bool {
    let mut only = Frame::new();
    mercator::hud::centred(
        &mut only,
        space::HINT_Y,
        b"LINE THE SITE UP INSIDE THE RING",
        mercator::art::TEXT,
    );
    let mut n = 0;
    for (a, b) in only.0.iter().zip(f.0.iter()) {
        if *a == mercator::art::TEXT {
            n += 1;
            if b != a {
                return false;
            }
        }
    }
    n > 0
}

/// Step G2 (CHOICE): the hint shows with the ring's lights in view and
/// the ship off its axis, and not on the axis nor under the ring.
#[test]
fn the_ring_hint_shows_off_the_axis_only() {
    let (_, s) = at_ring(M, 0, [0, 0, 1]);
    let to_ring = |up: i32, out: i32| {
        let (e1, _) = space::across(s.dir);
        let pos = world::add(world::add(s.ring, scale14(s.dir, up)), scale14(e1, out));
        sub(s.ring, pos)
    };
    let (mut d, _) = at_ring(KM, 600 * M, to_ring(KM, 600 * M));
    d.idle(1);
    assert_eq!(d.app.scene, Scene::Flight);
    assert!(hint_shown(&d.frame), "off the axis: the hint");
    std::fs::create_dir_all(common::shots().join("planet-g2")).unwrap();
    common::shot(&mut d, "planet-g2/ring-hint");
    let (mut d, _) = at_ring(KM, 100 * M, to_ring(KM, 100 * M));
    d.idle(1);
    assert!(!hint_shown(&d.frame), "on the axis: no hint");
    // Looking away from the ring: no lights in view, no hint.
    let (mut d, _) = at_ring(KM, 600 * M, s.dir);
    d.idle(1);
    assert!(!hint_shown(&d.frame), "the ring out of view: no hint");
}
