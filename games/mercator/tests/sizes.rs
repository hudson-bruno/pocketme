//! Step M2, sizes and perspective: fighters 3x and freighters 2x the
//! fighters (variant A; `--cfg merc_sizes_b` builds variant B, fighters
//! 2x and freighters 2.5x), ships in true perspective at every distance,
//! a far ship as a 2x2 dot of its kind's colour, and a station four
//! times its old size whose mouth takes the biggest freighter.
//!
//! `M2_SHOTS=<folder> cargo test --release --test sizes -- --ignored
//! --nocapture` writes the step's pictures there and prints its tables
//! (what each kind looks like at each distance, what a big freighter
//! close by costs to draw: MEASURED ON THE PC, counts, not time).

mod common;

use common::*;
use game_kit::{Frame, button};
use mercator::app::{Scene, seed_of};
use mercator::fixed::{Mat3, ONE14, V3, scale14, sub};
use mercator::galaxy;
use mercator::models;
use mercator::poly::{Model, Renderer, Stats};
use mercator::scene::{self, Detail, Eye, VIEW};
use mercator::world::{
    self, Career, FIGHTER_X, FREIGHTER_X, KM, Kind, M, Mode, SIZES_B, STATION_U, STATION_X, State,
};

/// Look bits: the first hull (bits 0-1 = 3) or the second (0), at the
/// biggest size step (bits 2-3 = 3), no markings.
const BIG_FIRST: u8 = 0b1111;
const BIG_ALT: u8 = 0b1100;
/// Middle size step (bits 2-3 = 1), first hull.
const MID_FIRST: u8 = 0b0111;

/// The variant's name for pictures and tables.
fn variant() -> &'static str {
    if SIZES_B { "B" } else { "A" }
}

/// Flying in the start system of galaxy 1, still, 20 km out on the
/// station's axis, nothing else in space.
fn out_far(detail: Detail) -> Driver {
    let mut d = Driver::new();
    d.start();
    d.app.opts.detail = detail;
    d.idle(2);
    assert_eq!(d.app.scene, Scene::Docked);
    d.app.sel = 0;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Flight);
    let w = &mut d.app.world;
    let b = w.bodies;
    w.me.pos = world::add(b.station_pos, scale14(b.station_rot.col(2), 20 * KM));
    w.me.rot = Mat3::looking(b.station_rot.col(0), [0, ONE14, 0]);
    w.me.speed = 0;
    w.me.throttle = 0;
    w.ships = [None; world::SHIPS];
    d
}

/// Puts a still ship of `kind` with `look` at `p`, its nose along `nose`.
fn put(w: &mut world::World, kind: Kind, look: u8, p: V3, nose: V3) -> usize {
    let i = w.add_ship(kind, p, Mode::Travel).unwrap();
    let s = w.ships[i].as_mut().unwrap();
    s.rot = Mat3::looking(nose, [0, ONE14, 0]);
    s.speed = 0;
    s.target = world::NOBODY;
    s.look = look;
    i
}

/// The 3D view alone (no panel, no marks), drawn into a fresh frame.
fn view_only(w: &world::World, detail: Detail) -> (Box<Frame>, Stats) {
    let mut f = Box::new(Frame::new());
    let mut r = Box::new(Renderer::new());
    r.begin();
    let cam = scene::camera(w, Eye::Cockpit);
    scene::draw(&mut f, &mut r, w, detail, &VIEW, &cam);
    (f, r.stats)
}

/// How ship `kind`/`look` alone looks at `dist` metres, turned `turn`
/// (0 head-on, 1 side, 2 three quarters): its box on the screen
/// (pixels that differ from the empty view) and the pixels painted.
fn measure(kind: Kind, look: u8, dist: i32, turn: u8) -> (i32, i32, u32) {
    let mut d = out_far(Detail::Low);
    let w = &mut d.app.world;
    let (pos, ahead) = (w.me.pos, w.me.rot.col(2));
    let empty = view_only(w, Detail::Low).0;
    let p = world::add(pos, scale14(ahead, dist * M));
    let right = w.me.rot.col(0);
    let nose = match turn {
        0 => sub(pos, p),
        1 => right,
        _ => world::add(
            scale14(sub(pos, p), ONE14 / 2),
            scale14(right, dist * M / 2),
        ),
    };
    put(w, kind, look, p, nose);
    let (f, s) = view_only(w, Detail::Low);
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, -1, -1);
    for y in 0..96 {
        for x in 0..160 {
            let k = y as usize * game_kit::W + x as usize;
            if f.0[k] != empty.0[k] {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x1 < 0 {
        return (0, 0, s.pixels);
    }
    (x1 - x0 + 1, y1 - y0 + 1, s.pixels)
}

/// The half width and half height of a model's corners (model units).
fn halves(m: &Model) -> (i32, i32) {
    m.verts.iter().fold((0, 0), |(x, y), v| {
        (x.max(i32::from(v[0]).abs()), y.max(i32::from(v[1]).abs()))
    })
}

#[test]
fn freighters_are_bigger_than_fighters_by_the_variant() {
    if SIZES_B {
        assert_eq!((FIGHTER_X, FREIGHTER_X * 2), (2, FIGHTER_X * 5));
    } else {
        assert_eq!((FIGHTER_X, FREIGHTER_X), (3, FIGHTER_X * 2));
    }
    // Hit sizes follow the drawn sizes; a raider's since step F2 a
    // little over half its drawn half span (see `Kind::size`).
    assert_eq!(Kind::Raider.size(), 24 * M * FIGHTER_X);
    assert_eq!(Kind::Hauler.size(), 60 * M * FREIGHTER_X);
    assert_eq!(Kind::Warden.size(), 48 * M * FIGHTER_X);
    // A freighter is drawn bigger than any fighter.
    let freighter = Kind::Hauler.size() / FREIGHTER_X * FREIGHTER_X;
    assert!(freighter > 2 * Kind::Warden.size());
}

/// The biggest freighter (both hulls, the biggest size step) fits
/// through the station's mouth, and the mouth fits the hub's front.
#[test]
fn the_biggest_freighter_fits_the_mouth() {
    // Drawn at 20 sixteenths of the old size times FREIGHTER_X: world
    // units = model units * (768 * 20 * FREIGHTER_X) / 256.
    let scale = models::SHIP_SCALE * 3 / 16 * 20 * FREIGHTER_X;
    let mouth = (
        i32::from(models::MOUTH_W) * STATION_U,
        i32::from(models::MOUTH_H) * STATION_U,
    );
    for m in [&models::HAULER, &models::TANKER] {
        let (hx, hy) = halves(m);
        let (wx, wy) = (hx * scale / 256, hy * scale / 256);
        println!(
            "freighter {} x {} m, mouth {} x {} m",
            2 * wx / M,
            2 * wy / M,
            2 * mouth.0 / M,
            2 * mouth.1 / M
        );
        assert!(wx < mouth.0 && wy < mouth.1);
    }
    // The hub is an octagon of radius 1600 lying flat: its front face
    // reaches 1478 model units out sideways and up.
    assert!(i32::from(models::MOUTH_W) < 1478 && i32::from(models::MOUTH_H) < 1478);
    assert_eq!(STATION_U, 16 * STATION_X);
}

/// True perspective: a raider twice as far is drawn about half as wide
/// (before step M2 it was drawn the same from 600 m to 7 km).
#[test]
fn ships_shrink_with_distance() {
    let (w1, _, p1) = measure(Kind::Raider, MID_FIRST, 1000, 1);
    let (w2, _, p2) = measure(Kind::Raider, MID_FIRST, 2000, 1);
    let (w4, _, _) = measure(Kind::Raider, MID_FIRST, 4000, 1);
    println!("raider side: 1 km {w1} px ({p1}), 2 km {w2} px ({p2}), 4 km {w4} px");
    assert!(w2 * 10 < w1 * 6 && w2 * 10 > w1 * 3, "{w1} {w2}");
    assert!(w4 * 10 < w2 * 7, "{w2} {w4}");
    assert!(p2 * 3 < p1, "{p1} {p2}");
    // Every kind, both hulls, seen edge-on or side on, shows something
    // out to the edge of sight (a thin hull edge-on gets its dot).
    for kind in [Kind::Raider, Kind::Warden, Kind::Hauler, Kind::Canister] {
        for look in [BIG_FIRST, BIG_ALT, MID_FIRST] {
            for dist in [1000, 2400, 4000, 5000, 6000, 6900] {
                for turn in 0..3 {
                    let (w, h, _) = measure(kind, look, dist, turn);
                    assert!(w > 0 && h > 0, "{kind:?} {look} {dist} m turn {turn}");
                }
            }
        }
    }
}

/// A ship whose ball is under 3 pixels is a 2x2 dot of its kind's
/// colour where it is (a canister already at 2 km).
#[test]
fn a_far_ship_is_a_dot_of_its_colour() {
    for (kind, dist) in [(Kind::Canister, 2000), (Kind::Raider, 6800)] {
        let mut d = out_far(Detail::Low);
        let w = &mut d.app.world;
        let p = world::add(w.me.pos, scale14(w.me.rot.col(2), dist * M));
        let i = put(w, kind, MID_FIRST, p, [0, 0, ONE14]);
        let (f, _) = view_only(w, Detail::Low);
        let cam = scene::camera(w, Eye::Cockpit);
        let (x, y) = scene::on_screen(&VIEW, &cam, p).unwrap();
        let c = scene::blip_of(w.ships[i].as_ref().unwrap());
        let n = (0..4)
            .filter(|k| {
                let (xx, yy) = (x + k % 2, y + k / 2);
                f.0[yy as usize * game_kit::W + xx as usize] == c
            })
            .count();
        assert_eq!(n, 4, "{kind:?} at {dist} m");
    }
}

/// The station grew: docking, the tow lane and the auto-dock still
/// bring the ship in (from in front of the mouth, 5 km out).
#[test]
fn the_auto_dock_still_docks_at_the_bigger_station() {
    let mut d = Driver::new();
    d.start();
    d.idle(2);
    d.app.sel = 0;
    d.tap(button::A);
    let w = &mut d.app.world;
    let b = w.bodies;
    let side = scale14(b.station_rot.col(0), 2 * KM);
    w.me.pos = world::add(
        world::add(b.station_pos, scale14(b.station_rot.col(2), 5 * KM)),
        side,
    );
    w.me.rot = Mat3::looking(sub(b.station_pos, w.me.pos), [0, ONE14, 0]);
    w.ships = [None; world::SHIPS];
    d.tap(button::Y);
    let mut calls = 0;
    while d.app.scene == Scene::Flight && calls < 60 * 90 {
        d.idle(1);
        calls += 1;
    }
    println!("docked after {:.1} s", f64::from(calls) / 60.0);
    assert_eq!(d.app.scene, Scene::Docked, "after {calls} calls");
}

// ---- Pictures and tables (measuring only) -------------------------------------------

fn save(dir: &std::path::Path, d: &mut Driver, name: &str) {
    let c = d.app.colours().to_vec();
    save_frame(
        &dir.join(format!("{}-{name}.png", variant())),
        &d.frame,
        &c,
        3,
    );
}

/// One kind at each of the step's distances, three quarters on (with
/// the panel, as the player sees it).
fn kind_pictures(dir: &std::path::Path) {
    let kinds = [
        ("raider", Kind::Raider, BIG_FIRST),
        ("warden", Kind::Warden, BIG_FIRST),
        ("hauler", Kind::Hauler, BIG_FIRST),
        ("tanker", Kind::Hauler, BIG_ALT),
    ];
    for dist in [600, 1000, 2400, 5000] {
        for &(name, kind, look) in &kinds {
            let mut d = out_far(Detail::High);
            let w = &mut d.app.world;
            let (pos, ahead, right) = (w.me.pos, w.me.rot.col(2), w.me.rot.col(0));
            let p = world::add(pos, scale14(ahead, dist * M));
            let nose = world::add(
                scale14(sub(pos, p), ONE14 / 2),
                scale14(right, dist * M / 2),
            );
            put(w, kind, look, p, nose);
            d.idle(1);
            save(dir, &mut d, &format!("{name}-{dist}m"));
        }
        // All four in a row across the view.
        let mut d = out_far(Detail::High);
        let w = &mut d.app.world;
        let (pos, ahead, right) = (w.me.pos, w.me.rot.col(2), w.me.rot.col(0));
        let at = world::add(pos, scale14(ahead, dist * M));
        for (k, &(_, kind, look)) in kinds.iter().enumerate() {
            let p = world::add(at, scale14(right, (2 * k as i32 - 3) * dist * M / 5));
            let nose = world::add(
                scale14(sub(pos, p), ONE14 / 2),
                scale14(right, dist * M / 2),
            );
            put(w, kind, look, p, nose);
        }
        d.idle(1);
        save(dir, &mut d, &format!("row-{dist}m"));
    }
}

/// A freighter beside a raider (side on, 1.5 km), and beside the
/// station: leaving the mouth as the traffic does, and waiting in
/// front of it, seen from the side and from the approach.
fn pair_pictures(dir: &std::path::Path) {
    for (name, look) in [("hauler", BIG_FIRST), ("tanker", BIG_ALT)] {
        let mut d = out_far(Detail::High);
        let w = &mut d.app.world;
        let (pos, ahead, right) = (w.me.pos, w.me.rot.col(2), w.me.rot.col(0));
        let at = world::add(pos, scale14(ahead, 1500 * M));
        put(
            w,
            Kind::Hauler,
            look,
            world::add(at, scale14(right, -350 * M)),
            right,
        );
        put(
            w,
            Kind::Raider,
            BIG_FIRST,
            world::add(at, scale14(right, 450 * M)),
            right,
        );
        d.idle(1);
        save(dir, &mut d, &format!("{name}-beside-raider-1500m"));
    }
    // Beside the station.
    let shots = [
        // (name, eye: along the axis, to the side, up (m); the freighter
        // along the axis (m))
        (
            "station-hauler-leaving-side",
            (700, 2200, 300),
            200 * STATION_X,
        ),
        ("station-hauler-in-mouth-side", (500, 1900, 250), 400),
        (
            "station-hauler-leaving-approach",
            (2400, 450, 150),
            200 * STATION_X,
        ),
        ("station-hauler-waiting-approach", (3200, 300, 100), 1800),
        ("station-hauler-far", (5000, 1500, 400), 200 * STATION_X),
    ];
    for (name, (ax, sd, up), at) in shots {
        let mut d = out_far(Detail::High);
        let w = &mut d.app.world;
        let b = w.bodies;
        let (axis, side, top) = (
            b.station_rot.col(2),
            b.station_rot.col(0),
            b.station_rot.col(1),
        );
        let p = world::add(b.station_pos, scale14(axis, at * M));
        put(w, Kind::Hauler, BIG_FIRST, p, axis);
        let eye = world::add(
            world::add(b.station_pos, scale14(axis, ax * M)),
            world::add(scale14(side, sd * M), scale14(top, up * M)),
        );
        w.me.pos = eye;
        let look_at = world::add(b.station_pos, scale14(axis, at * M / 2));
        w.me.rot = Mat3::looking(sub(look_at, eye), [0, ONE14, 0]);
        d.idle(1);
        save(dir, &mut d, name);
    }
    // The docked backdrop and the launch, as the player sees them.
    let mut d = Driver::new();
    d.start();
    d.idle(30);
    save(dir, &mut d, "docked");
    d.app.sel = 0;
    d.tap(button::A);
    d.idle(50);
    save(dir, &mut d, "launch");
}

/// What each kind looks like at each distance (box on the screen in
/// pixels, painted pixels in brackets).
fn size_table() {
    println!();
    println!(
        "## Variant {}: on screen, width x height px (painted px), Low detail",
        variant()
    );
    let dists = [600, 1000, 2400, 4000, 5000, 7000];
    print!("| kind, turn |");
    for d in dists {
        print!(" {} km |", f64::from(d) / 1000.0);
    }
    println!();
    print!("|---|");
    for _ in dists {
        print!("---|");
    }
    println!();
    let kinds = [
        ("raider", Kind::Raider, BIG_FIRST),
        ("hook", Kind::Raider, BIG_ALT),
        ("warden", Kind::Warden, BIG_FIRST),
        ("hauler", Kind::Hauler, BIG_FIRST),
        ("tanker", Kind::Hauler, BIG_ALT),
        ("canister", Kind::Canister, MID_FIRST),
    ];
    for (name, kind, look) in kinds {
        for (tn, turn) in [("head-on", 0), ("side", 1)] {
            print!("| {name} {tn} |");
            for dist in dists {
                let (w, h, p) = measure(kind, look, dist, turn);
                print!(" {w}x{h} ({p}) |");
            }
            println!();
        }
    }
}

/// What a big freighter close by costs: alone at 150 to 900 m in three
/// turns, and a fight beside one (worst picture over 20 s), at each
/// detail. MEASURED ON THE PC: counts, not time.
fn cost_table() {
    println!();
    println!("## Variant {}: a big freighter close by", variant());
    println!("| case | detail | objects | tris | clipped | pixels | dropped |");
    println!("|---|---|---|---|---|---|---|");
    for detail in Detail::ALL {
        let mut worst: (Stats, String) = (Stats::default(), String::new());
        for (name, look) in [("hauler", BIG_FIRST), ("tanker", BIG_ALT)] {
            for dist in [150, 300, 600, 900] {
                for turn in 0..3 {
                    let mut d = out_far(detail);
                    let w = &mut d.app.world;
                    let (pos, ahead, right) = (w.me.pos, w.me.rot.col(2), w.me.rot.col(0));
                    let p = world::add(pos, scale14(ahead, dist * M));
                    let nose = match turn {
                        0 => sub(pos, p),
                        1 => right,
                        _ => world::add(
                            scale14(sub(pos, p), ONE14 / 2),
                            scale14(right, dist * M / 2),
                        ),
                    };
                    put(w, Kind::Hauler, look, p, nose);
                    d.idle(1);
                    let s = d.app.render.stats;
                    assert_eq!(s.dropped, 0);
                    if s.pixels > worst.0.pixels {
                        worst = (s, format!("{name} {dist} m turn {turn}"));
                    }
                }
            }
        }
        let s = worst.0;
        println!(
            "| alone, worst ({}) | {detail:?} | {} | {} | {} | {} | {} |",
            worst.1, s.objects, s.tris, s.clipped, s.pixels, s.dropped
        );
        // A fight beside a freighter: three raiders, the biggest hauler
        // 400 m off the line of the fight, the pilot hunting.
        let mut d = out_far(detail);
        {
            let w = &mut d.app.world;
            let (pos, ahead, right) = (w.me.pos, w.me.rot.col(2), w.me.rot.col(0));
            put(
                w,
                Kind::Hauler,
                BIG_FIRST,
                world::add(
                    pos,
                    world::add(scale14(ahead, 900 * M), scale14(right, 400 * M)),
                ),
                right,
            );
            for k in 0..3 {
                let p = world::add(pos, scale14(ahead, (1500 + k * 600) * M));
                let p = world::add(p, scale14(right, (k - 1) * 300 * M));
                w.add_ship(Kind::Raider, p, Mode::Hunt);
            }
        }
        let mut most = Stats::default();
        for _ in 0..200 {
            for _ in 0..6 {
                let w = &mut d.app.world;
                if w.state != State::Flying {
                    break;
                }
                let pad = match hunter(w, 20 * KM) {
                    Some(i) => fight_pad(w, i),
                    None => world::Pad::default(),
                };
                w.tick(&pad);
                w.me.shield = w.shield_max();
                w.car.hull = 100;
                // The freighter stays put.
                for s in w.ships.iter_mut().flatten() {
                    if s.kind == Kind::Hauler {
                        s.speed = 0;
                    }
                }
                while w.take_event().is_some() {}
            }
            d.idle(1);
            let s = d.app.render.stats;
            assert_eq!(s.dropped, 0);
            if s.pixels + 40 * s.tris > most.pixels + 40 * most.tris {
                most = s;
            }
        }
        println!(
            "| fight beside a freighter, worst | {detail:?} | {} | {} | {} | {} | {} |",
            most.objects, most.tris, most.clipped, most.pixels, most.dropped
        );
    }
}

#[test]
#[ignore = "measuring only: pictures into M2_SHOTS and the step's tables (best with --release)"]
fn pictures_and_tables() {
    if let Some(dir) = std::env::var_os("M2_SHOTS") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        kind_pictures(&dir);
        pair_pictures(&dir);
    } else {
        println!("M2_SHOTS not set: no pictures");
    }
    size_table();
    cost_table();
    // Keep the galaxy import used in both variants.
    let _ = galaxy::SYSTEMS;
    let _ = Career::new(seed_of(1));
}
