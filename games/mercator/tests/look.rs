//! Pictures of every screen (into `.local/mercator/shots/`), what the
//! heaviest scenes cost (triangles, pixels), and determinism: the same
//! keys give the same pictures.

mod common;

use common::*;
use game_kit::button;
use mercator::app::{Scene, seed_of};
use mercator::fixed::{Mat3, ONE14, V3, norm, scale14, sub};
use mercator::galaxy;
use mercator::poly::Stats;
use mercator::scene::{Detail, Eye};
use mercator::world::{self, Career, KM, Kind, M, Mode, Pad, State};

/// A driver docked at the first system of `danger` in galaxy 1.
fn docked_in(danger: u8) -> Driver {
    let mut d = Driver::new();
    d.start();
    let mut c = Career::new(seed_of(1));
    c.system = (0..galaxy::SYSTEMS as u8)
        .find(|&i| galaxy::system(c.seed, i).danger == danger)
        .unwrap();
    c.target = c.system;
    d.app.world.begin(c);
    d.app.last_dock = c;
    d.idle(2);
    d
}

/// From the dock: launch, and a few calls of flight.
fn launch(d: &mut Driver) {
    assert_eq!(d.app.scene, Scene::Docked);
    d.app.sel = 0;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Flight);
}

/// The ship put at `pos`, looking at `at`, at cruising speed.
fn place(d: &mut Driver, pos: V3, at: V3) {
    let w = &mut d.app.world;
    w.me.pos = pos;
    w.me.rot = Mat3::looking(sub(at, pos), [0, ONE14, 0]);
    w.me.speed = 600;
    w.me.throttle = 600;
    w.ships = [None; world::SHIPS];
}

/// One call drawn, and what it cost.
fn draw_cost(d: &mut Driver) -> Stats {
    d.idle(1);
    d.app.render.stats
}

/// A fight set up in a lawless system: three raiders ahead.
fn fight(d: &mut Driver) {
    let w = &mut d.app.world;
    let out = w.bodies.station_rot.col(2);
    let at = world::add(w.bodies.station_pos, scale14(out, 14 * KM));
    let side = w.bodies.station_rot.col(0);
    w.me.pos = at;
    w.me.rot = Mat3::looking(side, [0, ONE14, 0]);
    w.ships = [None; world::SHIPS];
    for k in 0..3 {
        let p = world::add(at, scale14(side, 900 * M + k * 500 * M));
        let p = world::add(p, [0, (k - 1) * 120 * M, (k - 1) * 200 * M]);
        w.add_ship(Kind::Raider, p, Mode::Hunt);
    }
}

/// The fight flown by the pilot for `ticks` (world level), then drawn.
fn fight_on(d: &mut Driver, ticks: u32) {
    for _ in 0..ticks {
        let w = &mut d.app.world;
        if w.state != State::Flying {
            break;
        }
        let pad = match hunter(w, 20 * KM) {
            Some(i) => fight_pad(w, i),
            None => Pad::default(),
        };
        w.tick(&pad);
        while w.take_event().is_some() {}
    }
}

#[test]
fn every_screen_has_a_picture() {
    // Title.
    let mut d = Driver::new();
    d.idle(40);
    shot(&mut d, "01-title");
    // Docked, market (with something bought), outfit.
    d.start();
    d.idle(30);
    shot(&mut d, "02-docked");
    d.tap(button::DOWN);
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Market);
    d.tap(button::DOWN);
    d.tap(button::A);
    d.tap(button::A);
    d.idle(2);
    shot(&mut d, "03-market");
    d.tap(button::B);
    d.tap(button::DOWN);
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Outfit);
    d.tap(button::DOWN);
    d.tap(button::DOWN);
    shot(&mut d, "04-outfit");
    d.tap(button::B);
    // The chart: near, then the whole galaxy.
    d.tap(button::SELECT);
    assert_eq!(d.app.scene, Scene::Chart);
    d.tap(button::RIGHT);
    d.idle(2);
    shot(&mut d, "05-chart-near");
    d.tap(button::X);
    d.idle(2);
    shot(&mut d, "06-chart-galaxy");
    d.tap(button::X);
    d.tap(button::B);
    // Launch: out of the mouth.
    launch(&mut d);
    d.idle(50);
    shot(&mut d, "07-flight-launch");
    // Near the station, in front of its mouth, looking at it.
    let b = d.app.world.bodies;
    let axis = b.station_rot.col(2);
    let side = b.station_rot.col(0);
    let pos = world::add(
        b.station_pos,
        // Step M2: times STATION_X (was 1300 m out, 500 m aside).
        world::add(
            scale14(axis, 1300 * M * world::STATION_X),
            scale14(side, 500 * M * world::STATION_X),
        ),
    );
    place(&mut d, pos, b.station_pos);
    d.idle(2);
    shot(&mut d, "08-flight-station");
    // Near the planet: low over the day side, looking along the ground.
    let up = norm(b.sun_pos, ONE14);
    let pos = scale14(up, b.planet_r + 500 * M);
    let ahead = world::add(scale14(up, b.planet_r / 2), scale14(side, 3 * KM));
    place(&mut d, pos, ahead);
    d.idle(2);
    shot(&mut d, "09-flight-planet");
    // Farther out: the planet whole, with the sun beside it.
    let pos = world::add(scale14(up, b.planet_r * 3), scale14(side, b.planet_r * 2));
    place(&mut d, pos, [0; 3]);
    d.idle(2);
    shot(&mut d, "10-flight-planet-far");
    // Pause.
    d.tap(button::START);
    shot(&mut d, "11-pause");
    d.tap(button::START);
    // Jump: the chart from flight, a system in reach, the tunnel.
    place(
        &mut d,
        world::add(b.station_pos, scale14(axis, 40 * KM)),
        b.station_pos,
    );
    let here = d.app.world.sys;
    let to = (0..galaxy::SYSTEMS as u8)
        .find(|&i| i != here.id && here.dist(&galaxy::system(d.app.world.car.seed, i)) <= 50)
        .unwrap();
    d.tap(button::SELECT);
    d.app.chart_sel = to;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Jump, "note: {:?}", d.app.note);
    d.idle(30);
    shot(&mut d, "12-jump");
    d.idle(80);
    assert_eq!(d.app.scene, Scene::Flight);
    d.idle(30);
    shot(&mut d, "13-arrived");

    // A fight in a lawless system, cockpit and chase.
    let mut d = docked_in(3);
    d.app.opts.eye = Eye::Cockpit;
    launch(&mut d);
    fight(&mut d);
    fight_on(&mut d, 40);
    d.idle(1);
    shot(&mut d, "14-fight-start");
    let mut best = 0;
    for k in 0..40 {
        fight_on(&mut d, 6);
        d.hold(button::A);
        let w = &d.app.world;
        let busy = w.beams.iter().filter(|b| b.age > 0).count()
            + 2 * w
                .blasts
                .iter()
                .filter(|b| b.age < world::BLAST_TICKS)
                .count();
        if busy > best && w.state == State::Flying {
            best = busy;
            shot(&mut d, "15-fight-busy");
        }
        if k == 20 {
            shot(&mut d, "16-fight-later");
        }
    }
    d.app.opts.eye = Eye::Chase;
    d.idle(1);
    shot(&mut d, "17-fight-chase");
    d.app.opts.detail = Detail::Low;
    d.idle(1);
    shot(&mut d, "18-fight-chase-low");

    // Chase view near the station.
    let mut d = docked_in(0);
    d.app.opts.eye = Eye::Chase;
    launch(&mut d);
    d.idle(90);
    shot(&mut d, "19-chase-launch");

    // Lost.
    let mut d = docked_in(0);
    launch(&mut d);
    d.idle(30);
    let p = d.app.world.me.pos;
    d.app
        .world
        .add_ship(Kind::Raider, world::add(p, [0, 0, 600 * M]), Mode::Hunt);
    d.idle(20);
    // The last hit.
    d.app.world.car.hull = 0;
    for _ in 0..600 {
        if d.app.scene == Scene::Lost {
            break;
        }
        d.idle(1);
    }
    assert_eq!(d.app.scene, Scene::Lost);
    d.idle(90);
    assert!(shows(
        &d,
        |f| mercator::hud::centred(f, 44, b"CARGO AND TAGS LOST", mercator::art::TEXT),
        mercator::art::TEXT
    ));
    shot(&mut d, "20-lost");

    // Won: the charter's credits and surveys done, the auto-dock home.
    let mut d = docked_in(0);
    launch(&mut d);
    d.app.world.car.credits = world::WIN_CREDITS + 120;
    d.app.world.car.charted = [0xFF; world::CHART_BYTES];
    d.idle(30);
    d.tap(button::Y);
    for _ in 0..60 * 120 {
        if d.app.scene != Scene::Flight {
            break;
        }
        d.idle(1);
    }
    assert_eq!(d.app.scene, Scene::Won);
    d.idle(90);
    shot(&mut d, "21-won");
    d.tap(button::A);
    d.idle(5);
    shot(&mut d, "22-docked-after-win");
}

/// The heaviest scenes: what one picture costs (MEASURED ON THE PC:
/// counts, not time). The flight scenes in both views (2026-11-23: the
/// game starts in the chase view, whose own ship adds to the picture).
#[test]
fn the_heaviest_scenes_cost() {
    let mut rows: Vec<(String, Stats)> = Vec::new();
    // Step M3: the weight the fight's worst picture is picked by (one
    // triangle's set-up counted as 40 pixels).
    let score = |s: &Stats| s.pixels + 40 * s.tris;
    let mut note = |name: String, s: Stats| {
        assert_eq!(s.dropped, 0, "{name}: triangles dropped");
        rows.push((name, s));
    };
    for detail in Detail::ALL {
        let mut d = Driver::new();
        d.app.opts.detail = detail;
        d.idle(2);
        note(format!("title {detail:?}"), draw_cost(&mut d));
        d.start();
        note(format!("docked {detail:?}"), draw_cost(&mut d));
        launch(&mut d);
        for eye in Eye::ALL {
            d.app.opts.eye = eye;
            let b = d.app.world.bodies;
            let axis = b.station_rot.col(2);
            // Station filling the view, at several distances.
            let mut most = Stats::default();
            // Step M2: the station's distances times STATION_X.
            for dist in [300, 500, 800, 1300, 2500] {
                let pos = world::add(b.station_pos, scale14(axis, dist * M * world::STATION_X));
                place(&mut d, pos, b.station_pos);
                let s = draw_cost(&mut d);
                if s.pixels > most.pixels {
                    most = s;
                }
            }
            note(format!("near the station {detail:?} {eye:?}"), most);
            // Planet filling the view.
            let mut most = Stats::default();
            let up = norm(b.sun_pos, ONE14);
            for alt in [200, 500, 1500, 4000] {
                let pos = scale14(up, b.planet_r + alt * M);
                place(&mut d, pos, [0; 3]);
                let s = draw_cost(&mut d);
                if s.tris > most.tris {
                    most = s;
                }
            }
            note(format!("near the planet {detail:?} {eye:?}"), most);
            // Station in front of the planet: both.
            let pos = world::add(b.station_pos, scale14(axis, 6 * KM));
            place(&mut d, pos, b.station_pos);
            note(
                format!("station + planet {detail:?} {eye:?}"),
                draw_cost(&mut d),
            );
            // A fight, worst picture over 20 s.
            let mut f = docked_in(3);
            f.app.opts.detail = detail;
            f.app.opts.eye = eye;
            launch(&mut f);
            fight(&mut f);
            let mut most = Stats::default();
            for _ in 0..200 {
                fight_on(&mut f, 6);
                let s = draw_cost(&mut f);
                if score(&s) > score(&most) {
                    most = s;
                }
            }
            note(format!("fight {detail:?} {eye:?}"), most);
            // Step M3: a fight in front of the station and the planet, worst
            // picture over 20 s (the docked view's bodies plus raiders).
            for km in [3, 6] {
                let mut f = docked_in(3);
                f.app.opts.detail = detail;
                f.app.opts.eye = eye;
                launch(&mut f);
                let b = f.app.world.bodies;
                let pos = world::add(b.station_pos, scale14(b.station_rot.col(2), km * KM));
                place(&mut f, pos, b.station_pos);
                let ahead = norm(sub(b.station_pos, pos), ONE14);
                for k in 0..3 {
                    let p = world::add(pos, scale14(ahead, 900 * M + k * 500 * M));
                    let p = world::add(p, [(k - 1) * 200 * M, (k - 1) * 120 * M, 0]);
                    f.app.world.add_ship(Kind::Raider, p, Mode::Hunt);
                }
                let mut most = Stats::default();
                for _ in 0..200 {
                    fight_on(&mut f, 6);
                    let s = draw_cost(&mut f);
                    if score(&s) > score(&most) {
                        most = s;
                    }
                }
                note(
                    format!("fight at the station {km} km {detail:?} {eye:?}"),
                    most,
                );
            }
        }
        // Step M3: the live docked scene, worst picture over 15 s on each
        // docked screen, at the calm and the lawless dock.
        for danger in [0u8, 3] {
            let mut d = docked_in(danger);
            d.app.opts.detail = detail;
            // Step M5: and the SHIPYARD (its turning model in front).
            for (sel, name) in [(0, "docked"), (1, "market"), (2, "outfit"), (4, "shipyard")] {
                if sel > 0 {
                    d.app.sel = sel;
                    d.tap(button::A);
                }
                let mut most = Stats::default();
                for _ in 0..900 {
                    d.idle(1);
                    let s = d.app.render.stats;
                    if score(&s) > score(&most) {
                        most = s;
                    }
                }
                note(format!("live {name} d{danger} {detail:?}"), most);
                if sel > 0 {
                    d.tap(button::B);
                }
            }
        }
    }
    let mut worst = Stats::default();
    // The heaviest flight scene's score in each view (cockpit, chase).
    let mut flight = [0; 2];
    for (name, s) in &rows {
        println!(
            "{name:<38} objects {:>2}  faces {:>3}  tris {:>3}  clipped {:>2}  pixels {:>6}  score {:>6}",
            s.objects,
            s.faces,
            s.tris,
            s.clipped,
            s.pixels,
            score(s)
        );
        worst.tris = worst.tris.max(s.tris);
        worst.pixels = worst.pixels.max(s.pixels);
        if !["title", "docked", "live"]
            .iter()
            .any(|p| name.starts_with(p))
        {
            let k = usize::from(name.ends_with("Chase"));
            flight[k] = flight[k].max(score(s));
        }
    }
    println!("worst: {} triangles, {} pixels", worst.tris, worst.pixels);
    println!(
        "heaviest flight scene: score {} (cockpit), {} (chase)",
        flight[0], flight[1]
    );
    // The budget the renderer was copied with (KESTREL's): 160 triangles.
    assert!(worst.tris <= 160, "too many triangles");
    // Step M3: the live docked scene stays under the heaviest flight
    // scene, in either view.
    for (name, s) in rows.iter().filter(|(n, _)| n.starts_with("live")) {
        assert!(
            score(s) < flight[0].min(flight[1]),
            "{name} above the heaviest flight scene"
        );
    }
}

/// Plays a fixed script of keys; returns the picture hashes and the end.
fn script_run() -> (Vec<u64>, String) {
    let mut d = Driver::new();
    let mut hashes = Vec::new();
    let mut r = 0x1234_5678u32;
    d.start();
    launch(&mut d);
    for call in 0..60 * 120 {
        r ^= r << 13;
        r ^= r >> 17;
        r ^= r << 5;
        let held = if call % 90 < 45 {
            [
                button::A,
                button::UP,
                button::LEFT | button::X,
                0,
                button::R,
            ][(r % 5) as usize]
        } else {
            0
        };
        // No pause, no chart: the flight goes on.
        let held = held & !(button::START | button::SELECT);
        d.hold(held);
        if call % 30 == 0 {
            hashes.push(d.picture_hash());
        }
    }
    let w = &d.app.world;
    (
        hashes,
        format!("{:?} {:?} {:?} {:?}", w.car, w.me.pos, w.me.rot, w.ships),
    )
}

#[test]
fn the_same_keys_give_the_same_game() {
    let a = script_run();
    let b = script_run();
    assert_eq!(a.0.len(), b.0.len());
    assert_eq!(a.0, b.0, "pictures differ");
    assert_eq!(a.1, b.1, "worlds differ");
    // And not every picture is the same (the game did move).
    let distinct: std::collections::HashSet<_> = a.0.iter().collect();
    assert!(distinct.len() > a.0.len() / 2);
}

/// What one picture sends to the screen (the console's own rule,
/// `console_video::diff`, in game pixels), in three flights at 30 and
/// 60 a second: the auto-dock from launch to the next dock, a fight in
/// a lawless system, and a cruise past the planet. MEASURED on the PC.
#[test]
fn what_a_picture_sends() {
    use console_video::{Areas, Frame as Shown, diff};
    for fast in [false, true] {
        let ticks = if fast { 1 } else { 2 };
        for what in ["auto-dock", "fight", "planet pass"] {
            let mut d = docked_in(if what == "fight" { 3 } else { 1 });
            d.app.opts.fast = fast;
            launch(&mut d);
            match what {
                "fight" => fight(&mut d),
                "planet pass" => {
                    let b = d.app.world.bodies;
                    let up = norm(b.sun_pos, ONE14);
                    let side = norm(b.station_rot.col(0), ONE14);
                    let pos =
                        world::add(scale14(up, b.planet_r + 1500 * M), scale14(side, -9 * KM));
                    place(&mut d, pos, world::add(pos, side));
                }
                _ => {}
            }
            d.app.draw(&mut d.frame);
            let mut shown = Box::new(Shown(d.frame.0));
            let (mut n, mut sum, mut worst, mut over) = (0u32, 0u64, 0u32, 0u32);
            let mut tris = 0u32;
            for i in 0..60 * 90 / ticks {
                let w = &mut d.app.world;
                if w.state != State::Flying {
                    break;
                }
                for _ in 0..ticks {
                    let pad = match what {
                        "fight" => match hunter(w, 20 * KM) {
                            Some(k) => fight_pad(w, k),
                            None => Pad::default(),
                        },
                        "auto-dock" => Pad {
                            auto: !w.me.auto && i % 30 == 0,
                            ..Pad::default()
                        },
                        _ => Pad {
                            throttle: 256,
                            ..Pad::default()
                        },
                    };
                    w.tick(&pad);
                    while w.take_event().is_some() {}
                }
                d.app.draw(&mut d.frame);
                let mut a = Areas::new();
                diff(&shown, &Shown(d.frame.0), &mut a);
                shown.0.copy_from_slice(&d.frame.0);
                let px = a.pixels();
                n += 1;
                sum += u64::from(px);
                worst = worst.max(px);
                over += u32::from(px > 6_000);
                tris = tris.max(d.app.render.stats.tris);
            }
            println!(
                "{what:12} {:2} a second: {n:5} pictures, sent mean {:5}, worst {:5}, over 6,000 {:4.1} %, triangles worst {tris}",
                if fast { 60 } else { 30 },
                sum / u64::from(n.max(1)),
                worst,
                f64::from(over) * 100.0 / f64::from(n.max(1)),
            );
            assert!(n > 100, "{what}: too short a flight");
        }
    }
}

/// The three kinds of ship side by side at 1.5 km, head on, from the
/// side and from behind (with the engines' glow): each must be told
/// apart by outline and colour (pictures `23-kinds-*`).
#[test]
fn ship_kinds_read_at_a_distance() {
    let mut d = docked_in(0);
    launch(&mut d);
    let b = d.app.world.bodies;
    let axis = b.station_rot.col(2);
    let side = b.station_rot.col(0);
    let pos = world::add(b.station_pos, scale14(axis, 20 * KM));
    let ahead = world::add(pos, scale14(side, 1500 * M));
    for (name, turn) in [("head-on", 0), ("side", 1), ("tail", 2)] {
        place(&mut d, pos, ahead);
        d.app.world.me.speed = 0;
        d.app.world.me.throttle = 0;
        let w = &mut d.app.world;
        let right = w.me.rot.col(0);
        for (k, kind) in [Kind::Raider, Kind::Hauler, Kind::Warden]
            .into_iter()
            .enumerate()
        {
            let p = world::add(ahead, scale14(right, (k as i32 - 1) * 450 * M));
            let i = w.add_ship(kind, p, Mode::Travel).unwrap();
            let s = w.ships[i].as_mut().unwrap();
            let to_me = sub(pos, p);
            let look = match turn {
                0 => to_me,
                1 => scale14(right, ONE14),
                _ => sub(p, pos),
            };
            s.rot = Mat3::looking(look, [0, ONE14, 0]);
            s.speed = 0;
            s.target = world::NOBODY;
        }
        d.idle(1);
        let kinds: Vec<Kind> = d.app.world.ships.iter().flatten().map(|s| s.kind).collect();
        assert!(kinds.len() >= 3, "{kinds:?}");
        shot(&mut d, &format!("23-kinds-{name}"));
    }
}

/// An explosion's stages side by side at 1 km (picture
/// `24-blast-stages`): flash, fireball, sparks and fragments, smoke.
#[test]
fn a_blast_has_stages() {
    let mut d = docked_in(0);
    launch(&mut d);
    let b = d.app.world.bodies;
    let pos = world::add(b.station_pos, scale14(b.station_rot.col(2), 20 * KM));
    let ahead = world::add(pos, scale14(b.station_rot.col(0), 1000 * M));
    place(&mut d, pos, ahead);
    d.app.world.me.speed = 0;
    d.app.world.me.throttle = 0;
    let right = d.app.world.me.rot.col(0);
    for (k, age) in [1u8, 7, 15, 30].into_iter().enumerate() {
        let p = world::add(ahead, scale14(right, (k as i32 * 2 - 3) * 160 * M));
        // One tick passes before the picture is drawn.
        d.app.world.blasts[k] = world::Blast {
            pos: p,
            age: age - 1,
            size: 28,
        };
    }
    d.idle(1);
    shot(&mut d, "24-blast-stages");
}

/// MERCATOR's own parts (pictures `25-tow-lane`, `26-gunner`,
/// `27-beacon`): the station's tow lane taking the ship in, player 2's
/// dorsal turret firing at a raider off the nose, and the station's
/// beacon on the view's edge with the charter gauges on the panel.
#[test]
fn the_tow_lane_the_gunner_and_the_beacon_show() {
    let mut d = docked_in(0);
    launch(&mut d);
    let b = d.app.world.bodies;
    let axis = b.station_rot.col(2);
    let gate = world::add(b.station_pos, scale14(axis, 1200 * M));
    place(&mut d, gate, b.station_pos);
    d.app.world.me.speed = 250;
    d.app.world.me.throttle = 250;
    d.idle(20);
    assert!(d.app.world.me.auto, "the tow lane did not take the ship");
    shot(&mut d, "25-tow-lane");

    let mut d = docked_in(2);
    launch(&mut d);
    let b = d.app.world.bodies;
    let pos = world::add(b.station_pos, scale14(b.station_rot.col(2), 12 * KM));
    let ahead = world::add(pos, scale14(b.station_rot.col(0), 3 * KM));
    place(&mut d, pos, ahead);
    d.app.world.me.speed = 0;
    d.app.world.me.throttle = 0;
    let w = &mut d.app.world;
    // A raider 700 m off, 30 degrees right of the nose and a little up.
    let to = world::add(
        world::add(
            scale14(w.me.rot.col(2), 600 * M),
            scale14(w.me.rot.col(0), 350 * M),
        ),
        scale14(w.me.rot.col(1), 80 * M),
    );
    let i = w
        .add_ship(Kind::Raider, world::add(pos, to), Mode::Travel)
        .unwrap();
    let s = w.ships[i].as_mut().unwrap();
    s.speed = 0;
    s.target = world::NOBODY;
    s.rot = Mat3::looking(scale14(to, -1), [0, ONE14, 0]);
    let hp = s.hp;
    let mut fired = false;
    for _ in 0..12 {
        d.call_keys(mercator::app::Keys {
            gunner: true,
            ..mercator::app::Keys::default()
        });
        if d.app.world.beams.iter().any(|b| b.age == 2) && !fired {
            fired = true;
            shot(&mut d, "26-gunner");
        }
    }
    assert!(fired, "the turret never fired");
    assert!(d.app.world.ships[i].is_none_or(|s| s.hp < hp));
    // The station behind and to the left: its beacon on the edge.
    let w = &mut d.app.world;
    w.ships = [None; world::SHIPS];
    w.car.charted = [0x55; world::CHART_BYTES];
    w.car.credits = 31_000;
    d.idle(1);
    shot(&mut d, "27-beacon");
}

/// The market in a SECURE and a LAWLESS system of the same economy
/// (galaxy 1): danger widens the prices, and RISKY and LAWLESS markets
/// say RISK PAY (`market::RISK_PAY`).
#[test]
fn a_lawless_market_shows_risk_pay() {
    use mercator::market::{GOODS, made_here};
    let seed = seed_of(1);
    let first = |danger: u8, econ: Option<galaxy::Econ>| {
        (0..galaxy::SYSTEMS as u8)
            .map(|i| galaxy::system(seed, i))
            .find(|s| s.danger == danger && econ.is_none_or(|e| s.econ == e))
            .unwrap()
    };
    let wild = first(3, None);
    let calm = first(0, Some(wild.econ));
    let mut prices = Vec::new();
    for (s, name) in [(calm, "28-market-secure"), (wild, "28-market-lawless")] {
        let mut d = Driver::new();
        d.start();
        let mut c = Career::new(seed);
        c.system = s.id;
        c.target = s.id;
        d.app.world.begin(c);
        d.app.last_dock = c;
        d.idle(2);
        d.app.sel = 1;
        d.tap(button::A);
        assert_eq!(d.app.scene, Scene::Market);
        d.idle(2);
        println!(
            "{name}: {} {:?} {}",
            s.name().as_str(),
            s.econ,
            galaxy::DANGER_NAMES[usize::from(s.danger)]
        );
        shot(&mut d, name);
        prices.push((0..GOODS).map(|g| d.app.world.price(g)).collect::<Vec<_>>());
    }
    // Made goods cost less in the LAWLESS system on average, wanted
    // ones more (each system also has its own small differences).
    let (mut made, mut rest) = ((0i64, 0i64), (0i64, 0i64));
    for (g, (&c, &w)) in prices[0].iter().zip(&prices[1]).enumerate() {
        let t = if made_here(wild.econ, g) {
            &mut made
        } else {
            &mut rest
        };
        t.0 += i64::from(c);
        t.1 += i64::from(w);
    }
    println!(
        "made goods: {} -> {}, the rest: {} -> {}",
        made.0, made.1, rest.0, rest.1
    );
    assert!(made.1 < made.0 && rest.1 > rest.0);
}

/// The range marks in flight (pictures `29-range-*`): a raider out of
/// laser reach (corner brackets, the target line dim), in reach but off
/// the aim line (a closed amber box, IN RANGE), and in reach on the aim
/// line (the crosshair locks too); a hauler named by the target line,
/// with its bar; and the market while A is held (`30-market-hold`).
#[test]
fn the_range_marks_and_a_held_market_show() {
    use mercator::art::ACCENT;
    use mercator::hud;
    // The cockpit view: its crosshair shows the lock.
    let mut d = docked_in(1);
    d.app.opts.eye = Eye::Cockpit;
    launch(&mut d);
    let b = d.app.world.bodies;
    let pos = world::add(b.station_pos, scale14(b.station_rot.col(2), 20 * KM));
    let ahead = world::add(pos, scale14(b.station_rot.col(0), 1500 * M));
    // (picture, kind, metres right, up, ahead, expected line)
    let cases: [(&str, Kind, [i32; 3], &str); 4] = [
        (
            "29-range-out",
            Kind::Raider,
            [300, 120, 3200],
            "RAIDER 3.2 KM",
        ),
        (
            "29-range-in-off-aim",
            Kind::Raider,
            [260, 100, 1600],
            "RAIDER 1.6 KM  IN RANGE",
        ),
        (
            "29-range-on-aim",
            Kind::Raider,
            [0, 0, 1250],
            "RAIDER 1.2 KM  IN RANGE",
        ),
        (
            "29-range-hauler",
            Kind::Hauler,
            // Step M2: off the aim of a hauler 360 m big (was -150, 60,
            // 900 and 914 m).
            [-450, 120, 750],
            "HAULER 882 M  IN RANGE",
        ),
    ];
    for (name, kind, m, want) in cases {
        place(&mut d, pos, ahead);
        let w = &mut d.app.world;
        w.me.speed = 0;
        w.me.throttle = 0;
        let r = w.me.rot;
        let p = [0, 1, 2]
            .iter()
            .fold(pos, |p, &k| world::add(p, scale14(r.col(k), m[k] * M)));
        let i = w.add_ship(kind, p, Mode::Travel).unwrap();
        let s = w.ships[i].as_mut().unwrap();
        s.pos = p;
        s.rot = Mat3::looking(sub(pos, p), [0, ONE14, 0]);
        s.speed = 0;
        s.cool = 250;
        if kind == Kind::Raider {
            s.target = world::PLAYER;
            s.mode = Mode::Hunt;
        } else {
            s.target = world::NOBODY;
        }
        // A raider out of reach in the hauler's picture too: brackets
        // stay on hunters only.
        if kind == Kind::Hauler {
            let q = world::add(pos, scale14(r.col(2), 2900 * M));
            let q = world::add(q, scale14(r.col(0), 900 * M));
            let j = w.add_ship(Kind::Raider, q, Mode::Hunt).unwrap();
            let s = w.ships[j].as_mut().unwrap();
            s.pos = q;
            s.target = world::PLAYER;
            s.speed = 0;
            s.cool = 250;
        }
        d.app.draw(&mut d.frame);
        let mut t = hud::Txt::new();
        let w = &d.app.world;
        hud::target_text(&mut t, w, &w.ships[i].unwrap());
        assert_eq!(t.as_str(), want, "{name}");
        let lock = d.frame.0[48 * game_kit::W + 74] == ACCENT;
        assert_eq!(lock, name == "29-range-on-aim", "{name}");
        println!("{name}: {want}, lock {lock}");
        shot(&mut d, name);
    }

    // The market, A held for one second: 1 + 4 repeats.
    let mut d = docked_in(0);
    d.app.world.car.credits = 5000;
    d.app.sel = 1;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Market);
    d.idle(2);
    for _ in 0..=60 {
        d.hold(button::A);
    }
    assert_eq!(d.app.world.car.hold.0[0], 5);
    shot(&mut d, "30-market-hold");
    d.hold(0);
}

/// Whether everything `draw` puts on an empty picture in colour `c`
/// is on the driver's picture too (and there is something): a text is
/// shown where and as the game draws it.
fn shows(d: &Driver, draw: impl Fn(&mut game_kit::Frame), c: u8) -> bool {
    let mut f = Box::new(game_kit::Frame::new());
    draw(&mut f);
    let mut n = 0;
    for (a, b) in f.0.iter().zip(d.frame.0.iter()) {
        if *a == c {
            n += 1;
            if *b != c {
                return false;
            }
        }
    }
    n > 0
}

/// The panel's TAG count as it must look: the number in the accent
/// colour on AUTO's row, its last column at x 157 (where the SVY and
/// FND bars end).
fn tag_count(f: &mut game_kit::Frame, n: u32) {
    let mut t = mercator::hud::Txt::new();
    t.num(n);
    mercator::hud::right(f, 158, 98, t.bytes(), mercator::art::ACCENT);
}

/// The dock screen's unpaid TAGs line, as the game draws it.
fn line(f: &mut game_kit::Frame, text: &str) {
    let mut t = mercator::hud::Txt::new();
    t.s(text).at(f, 80, 68, mercator::art::ACCENT);
}

/// Docks from flight with the auto-dock (no ships about); the dock
/// screen's first call.
fn auto_dock_home(d: &mut Driver) {
    let mut n = 0;
    while d.app.scene == Scene::Flight {
        d.hold(if d.app.world.me.auto { 0 } else { button::Y });
        d.app.world.ships = [None; world::SHIPS];
        n += 1;
        assert!(n < 60 * 120, "never docked");
    }
    d.hold(0);
}

/// The raiders' TAGs (pictures `31-` to `36-`): the kill note, a TAG
/// in flight (bracketed, the target line RAIDER TAG), the panel's TAG
/// count, the bounty note with its sum, NO BOUNTY OFFICE HERE at a
/// LAWLESS station, and the dock screen with TAGs unpaid. The SHIP LOST
/// card's CARGO AND TAGS LOST line is in `20-lost`.
#[test]
fn the_raiders_tags_show() {
    use mercator::art::ACCENT;
    use mercator::hud;
    // The kill: a raider shot down ahead (one hull point left).
    let mut d = docked_in(2);
    launch(&mut d);
    let b = d.app.world.bodies;
    let pos = world::add(b.station_pos, scale14(b.station_rot.col(2), 20 * KM));
    let ahead = world::add(pos, scale14(b.station_rot.col(0), 1500 * M));
    place(&mut d, pos, ahead);
    d.app.world.me.speed = 0;
    d.app.world.me.throttle = 0;
    let fwd = d.app.world.me.rot.col(2);
    let at = world::add(pos, scale14(fwd, 420 * M));
    let i = d.app.world.add_ship(Kind::Raider, at, Mode::Hunt).unwrap();
    d.app.world.ships[i].as_mut().unwrap().hp = 1;
    let mut n = 0;
    while d.app.world.ships[i].is_some_and(|s| s.kind == Kind::Raider) {
        let s = d.app.world.ships[i].as_mut().unwrap();
        s.pos = at;
        s.cool = 250;
        d.hold(button::A);
        n += 1;
        assert!(n < 120, "never fired");
    }
    d.hold(0);
    assert_eq!(d.app.note.unwrap().0, "RAIDER DOWN: SCOOP ITS TAG");
    shot(&mut d, "31-kill-note");

    // The TAG in flight, held still 420 m ahead and a little off the
    // aim line, once the note has gone.
    let tag = d.app.world.ships[i].unwrap();
    assert_eq!(tag.load.0, world::TAG);
    let r = d.app.world.me.rot;
    let spot = world::add(
        world::add(at, scale14(r.col(0), 40 * M)),
        scale14(r.col(1), 20 * M),
    );
    for _ in 0..100 {
        d.app.world.ships[i].as_mut().unwrap().pos = spot;
        d.app.world.me.speed = 0;
        d.idle(1);
    }
    // The blink's "on" half.
    while d.app.world.ships[i].unwrap().flash == 0 {
        d.app.world.ships[i].as_mut().unwrap().pos = spot;
        d.idle(1);
    }
    d.app.world.ships[i].as_mut().unwrap().pos = spot;
    d.app.draw(&mut d.frame);
    let mut t = hud::Txt::new();
    let w = &d.app.world;
    hud::target_text(&mut t, w, &w.ships[i].unwrap());
    assert!(t.as_str().starts_with("RAIDER TAG "), "{}", t.as_str());
    // The laser passes through a TAG: never IN RANGE, the way to scoop
    // it instead.
    assert!(t.as_str().ends_with("  FLY INTO IT"), "{}", t.as_str());
    assert!(!t.as_str().contains("IN RANGE"), "{}", t.as_str());
    println!("31-tag-in-flight: {}", t.as_str());
    shot(&mut d, "31-tag-in-flight");

    // Scooped, and more carried: the panel's count.
    d.app.world.ships[i].as_mut().unwrap().pos = d.app.world.me.pos;
    d.idle(1);
    assert_eq!(d.app.note.unwrap().0, "SCOOPED: RAIDER TAG");
    assert_eq!(d.app.world.car.tags, 1);
    assert!(shows(&d, |f| tag_count(f, 1), ACCENT));
    d.app.world.car.tags = 12;
    d.app.world.car.owed = 750;
    d.idle(1);
    assert!(shows(&d, |f| tag_count(f, 12), ACCENT));
    shot(&mut d, "32-scooped-panel-count");
    d.app.world.car.tags = 255;
    d.app.world.car.owed = 25_500;
    d.idle(100);
    assert!(shows(&d, |f| tag_count(f, 255), ACCENT));
    shot(&mut d, "32-panel-count-255");
    // None carried: no count.
    d.app.world.car.tags = 0;
    d.app.world.car.owed = 0;
    d.idle(1);
    assert!(!shows(&d, |f| tag_count(f, 0), ACCENT));
    assert!(!shows(&d, |f| tag_count(f, 255), ACCENT));

    // Home at a lawful (RISKY) station: paid, with the sum.
    d.app.world.car.tags = 3;
    d.app.world.car.owed = 225;
    auto_dock_home(&mut d);
    assert_eq!(d.app.scene, Scene::Docked);
    assert_eq!(d.app.note.unwrap().0, "BOUNTY PAID: ");
    assert_eq!(d.app.note_cr, 225);
    // The sum is drawn in the note.
    assert!(shows(
        &d,
        |f| hud::note(f, b"BOUNTY PAID: 225 CR", 20),
        ACCENT
    ));
    shot(&mut d, "33-bounty-note");

    // A LAWLESS station: not paid, kept; the dock screen says so.
    let mut d = docked_in(3);
    d.app.world.car.charted = [0xFF; world::CHART_BYTES];
    d.app.last_dock = d.app.world.car;
    launch(&mut d);
    d.app.world.car.tags = 4;
    d.app.world.car.owed = 325;
    auto_dock_home(&mut d);
    if world::LAWLESS_PAYS {
        // The switch turned round: paid here like anywhere. The TAGs
        // are put back for the dock line below.
        assert_eq!(d.app.note.unwrap().0, "BOUNTY PAID: ");
        assert_eq!(d.app.note_cr, 325);
        assert_eq!(d.app.world.car.tags, 0);
        d.app.world.car.tags = 4;
        d.app.world.car.owed = 325;
    } else {
        assert_eq!(d.app.note.unwrap().0, "NO BOUNTY OFFICE HERE");
        assert_eq!(d.app.note_cr, 0);
        assert_eq!(d.app.world.car.tags, 4);
        shot(&mut d, "34-lawless-no-bounty-office");
    }
    d.idle(100);
    assert!(d.app.note.is_none());
    assert!(shows(&d, |f| line(f, "TAGS 4: 325 CR"), ACCENT));
    shot(&mut d, "35-docked-tags-unpaid");
    // The most there can be.
    d.app.world.car.tags = 255;
    d.app.world.car.owed = 25_500;
    d.idle(1);
    assert!(shows(&d, |f| line(f, "TAGS 255: 25500 CR"), ACCENT));
    shot(&mut d, "35-docked-tags-unpaid-255");
    // None carried: no line.
    d.app.world.car.tags = 0;
    d.app.world.car.owed = 0;
    d.idle(1);
    assert!(!shows(&d, |f| line(f, "TAGS 255: 25500 CR"), ACCENT));

    // An OUTLAW at a lawful station.
    let mut d = docked_in(1);
    d.app.world.car.charted = [0xFF; world::CHART_BYTES];
    launch(&mut d);
    d.app.world.car.tags = 2;
    d.app.world.car.owed = 100;
    d.app.world.wanted = true;
    auto_dock_home(&mut d);
    assert_eq!(d.app.note.unwrap().0, "OUTLAW: NO BOUNTY PAID");
    shot(&mut d, "36-outlaw-no-bounty");
}

/// A raider's TAG blinks where a player looks for it: on the view (its
/// model near, its dot far) and on the radar. The same scene drawn with
/// the blink on and off must differ there, whichever way the TAG faces
/// (a new TAG faces the player; one turned sideways shows its sides).
#[test]
fn a_tag_blinks_on_the_view_and_on_the_radar() {
    let mut d = docked_in(2);
    launch(&mut d);
    let b = d.app.world.bodies;
    let pos = world::add(b.station_pos, scale14(b.station_rot.col(2), 20 * KM));
    // Looking away from the station, so nothing else is in the view.
    let away = world::add(pos, scale14(b.station_rot.col(2), KM));
    place(&mut d, pos, away);
    d.app.world.me.speed = 0;
    d.idle(1);
    let fwd = d.app.world.me.rot.col(2);
    let side = d.app.world.me.rot.col(0);
    for dist in [100, 300, 2_000, 6_500] {
        for sideways in [false, true] {
            d.app.world.ships = [None; world::SHIPS];
            let at = world::add(pos, scale14(fwd, dist * M));
            let i = d
                .app
                .world
                .add_ship(Kind::Canister, at, Mode::Travel)
                .unwrap();
            let s = d.app.world.ships[i].as_mut().unwrap();
            s.load = (world::TAG, 3);
            if sideways {
                s.rot = Mat3::looking(side, [0, ONE14, 0]);
            }
            let mut pics = Vec::new();
            for flash in [0, 1, 0] {
                d.app.world.ships[i].as_mut().unwrap().flash = flash;
                d.app.draw(&mut d.frame);
                pics.push(d.frame.0);
                if !sideways && pics.len() < 3 {
                    let half = if flash > 0 { "on" } else { "off" };
                    shot(&mut d, &format!("37-tag-blink-{dist}m-{half}"));
                }
            }
            let differ = |a: &[u8], b: &[u8], ys: std::ops::Range<usize>| {
                ys.flat_map(|y| (0..game_kit::W).map(move |x| y * game_kit::W + x))
                    .filter(|&k| a[k] != b[k])
                    .count()
            };
            // The same picture twice with the blink off.
            assert_eq!(differ(&pics[0], &pics[2], 0..120), 0);
            let view = differ(&pics[0], &pics[1], 0..96);
            let radar = differ(&pics[0], &pics[1], 96..120);
            println!(
                "TAG at {dist} m, sideways {sideways}: {view} view pixels, {radar} radar pixels blink"
            );
            assert!(
                view > 0,
                "{dist} m, sideways {sideways}: no blink on the view"
            );
            assert!(
                radar > 0,
                "{dist} m, sideways {sideways}: no blink on the radar"
            );
        }
    }
}

/// The panel's TAG count stands apart from AUTO (pictures
/// `38-tag-count-*`): for every count 1 to 255, with AUTO lit and dim,
/// the count is drawn right-aligned to x 157, at least 5 empty columns
/// (more than a word space) after AUTO's last column, inside the
/// picture, and it changes nothing else on the picture.
#[test]
fn the_tag_count_stands_apart_from_auto() {
    use mercator::art::{ACCENT, GOOD, TEXT_DIM};
    let mut d = docked_in(2);
    launch(&mut d);
    d.idle(30);
    for auto in [true, false] {
        d.app.world.me.auto = auto;
        d.app.world.car.tags = 0;
        d.app.draw(&mut d.frame);
        let base = d.frame.0;
        // AUTO's last lit column on its row.
        let ac = if auto { GOOD } else { TEXT_DIM };
        let auto_end = (120..160)
            .filter(|&x| (98..=102).any(|y| base[y * game_kit::W + x] == ac))
            .max()
            .unwrap();
        assert_eq!(auto_end, 141, "AUTO moved");
        for n in 1..=255u8 {
            d.app.world.car.tags = n;
            d.app.draw(&mut d.frame);
            let (mut first, mut last) = (usize::MAX, 0);
            for (k, (&a, &b)) in base.iter().zip(d.frame.0.iter()).enumerate() {
                if a == b {
                    continue;
                }
                let (x, y) = (k % game_kit::W, k / game_kit::W);
                assert!(
                    b == ACCENT && (98..=102).contains(&y),
                    "{n}: pixel {x},{y} changed outside the count"
                );
                first = first.min(x);
                last = last.max(x);
            }
            let gap = first - auto_end - 1;
            if [1, 9, 10, 99, 100, 255].contains(&n) {
                println!(
                    "AUTO {}, {n} TAGs: count x {first}..={last}, {gap} empty columns after AUTO",
                    if auto { "lit" } else { "dim" }
                );
            }
            assert!(gap >= 5, "{n}: the count touches AUTO ({gap} columns)");
            assert_eq!(last, 157, "{n}: not right-aligned to the bars");
            assert!(shows(&d, |f| tag_count(f, u32::from(n)), ACCENT), "{n}");
            if [1, 12, 99, 100, 255].contains(&n) {
                let lit = if auto { "lit" } else { "dim" };
                shot(&mut d, &format!("38-tag-count-auto-{lit}-{n}"));
            }
        }
    }
}

/// The target line on a TAG (pictures `39-tag-line-*`): the laser
/// passes through a TAG, so its line never says IN RANGE, in reach or
/// not; it says FLY INTO IT, and at the longest distance text the line
/// fits the picture and stays clear of the station's beacon when that
/// sits in one of the view's lower corners (x 3..=9 and 151..=157). On
/// the bottom edge between them (y 88) the beacon can sit on the line's
/// row: the line is drawn after it, so the letters stay whole (picture
/// `37-tag-blink-100m-off`). A raider and a cargo canister in reach keep
/// IN RANGE.
#[test]
fn a_tags_target_line_says_fly_into_it_never_in_range() {
    use mercator::art::{ACCENT, TAG_MARK, TEXT_DIM};
    use mercator::hud;
    let mut d = docked_in(2);
    launch(&mut d);
    let b = d.app.world.bodies;
    let pos = world::add(b.station_pos, scale14(b.station_rot.col(2), 20 * KM));
    // Looking along the station's side: its edge beacon is off the line.
    let at = world::add(pos, scale14(b.station_rot.col(0), KM));
    place(&mut d, pos, at);
    d.app.world.me.speed = 0;
    d.app.world.me.throttle = 0;
    let r = d.app.world.me.rot;
    // What a TAG's line says after its distance, as the game wrote it.
    let mut hint = String::new();
    // (metres ahead, kind, load, how the line must end)
    let cases: [(i32, Kind, bool, &str); 5] = [
        (420, Kind::Canister, true, "  FLY INTO IT"),
        (4000, Kind::Canister, true, "  FLY INTO IT"),
        (6900, Kind::Canister, true, "  FLY INTO IT"),
        (420, Kind::Raider, false, "  IN RANGE"),
        (420, Kind::Canister, false, "  IN RANGE"),
    ];
    for (m, kind, tag, end) in cases {
        d.app.world.ships = [None; world::SHIPS];
        let p = world::add(pos, scale14(r.col(2), m * M));
        let p = world::add(p, scale14(r.col(0), m * M / 12));
        let i = d.app.world.add_ship(kind, p, Mode::Travel).unwrap();
        let s = d.app.world.ships[i].as_mut().unwrap();
        s.pos = p;
        s.speed = 0;
        s.cool = 250;
        s.target = world::NOBODY;
        if tag {
            s.load = (world::TAG, 3);
            s.flash = 1;
        } else if kind == Kind::Canister {
            s.load = (0, 3);
        }
        d.app.draw(&mut d.frame);
        let mut t = hud::Txt::new();
        let w = &d.app.world;
        let reach = hud::target_text(&mut t, w, &w.ships[i].unwrap());
        let line = t.as_str().to_string();
        println!("{m} m, {kind:?}, TAG {tag}: {line} (laser reaches: {reach})");
        assert_eq!(reach, w.in_reach(p));
        assert_eq!(reach, m < 1000, "{line}");
        assert!(line.ends_with(end), "{line}");
        assert_eq!(line.contains("IN RANGE"), !tag, "{line}");
        assert_eq!(line.starts_with("RAIDER TAG "), tag, "{line}");
        if tag {
            let mut dt = hud::Txt::new();
            dt.s("RAIDER TAG ")
                .dist(mercator::fixed::length(sub(p, pos)));
            hint = line.strip_prefix(dt.as_str()).unwrap().to_string();
        }
        // Drawn as the game draws it: a TAG's line in its own colour,
        // in reach or not.
        let c = if tag {
            TAG_MARK
        } else if reach {
            ACCENT
        } else {
            TEXT_DIM
        };
        assert!(
            shows(&d, |f| hud::centred(f, 87, line.as_bytes(), c), c),
            "{line}"
        );
        if tag {
            shot(&mut d, &format!("39-tag-line-{m}m"));
        }
    }
    // The longest distance texts there are ("99.9 KM", and "8388 KM"
    // for the most a distance can be): the line, centred as drawn,
    // fits and keeps off the corner beacons.
    assert_eq!(hint, "  FLY INTO IT");
    for units in [99_999 * M, i32::MAX] {
        let mut t = hud::Txt::new();
        t.s("RAIDER TAG ").dist(units).s(&hint);
        let wide = hud::width(t.bytes()) as i32;
        let x0 = 80 - wide / 2;
        println!("{}: {wide} pixels, x {x0}..={}", t.as_str(), x0 + wide - 1);
        assert!(wide <= 160, "{}", t.as_str());
        assert!(x0 >= 10 && x0 + wide - 1 <= 150, "{}", t.as_str());
    }
    // The widest a TAG's line gets in play: a ship is named only while
    // it is drawn, within `SHIP_SEEN` (every whole metre up to it).
    let seen = mercator::scene::SHIP_SEEN / M;
    let (widest, at) = (0..=seen)
        .map(|m| {
            let mut t = hud::Txt::new();
            t.s("RAIDER TAG ").dist(m * M).s(&hint);
            (hud::width(t.bytes()), m)
        })
        .max()
        .unwrap();
    println!("widest TAG line within {seen} m: {widest} pixels (at {at} m, among others)");
    assert!(widest <= 130);
}

/// A TAG's marks have a colour of their own (pictures `40-mark-*`): its
/// target line, the bar under it and its four corners are in
/// `TAG_MARK` near (the laser reaches it) and far, with its blink on
/// and off; never a closed box, and nothing on the view is amber. A
/// raider hunting the player is as before: in reach a closed amber box
/// with an amber line and bar, out of reach its own red corners with a
/// dim line and bar.
#[test]
fn a_tags_marks_have_their_own_colour_at_every_distance() {
    use mercator::art::{ACCENT, BLIP_RAIDER, TAG_MARK, TEXT_DIM};
    use mercator::hud;
    use mercator::scene;
    let mut d = docked_in(2);
    launch(&mut d);
    let b = d.app.world.bodies;
    let pos = world::add(b.station_pos, scale14(b.station_rot.col(2), 20 * KM));
    let at = world::add(pos, scale14(b.station_rot.col(0), KM));
    place(&mut d, pos, at);
    d.app.world.me.speed = 0;
    d.app.world.me.throttle = 0;
    let r = d.app.world.me.rot;
    // (picture, metres ahead, a TAG; else a raider hunting the player)
    let cases: [(&str, i32, bool); 4] = [
        ("40-mark-tag-near", 420, true),
        ("40-mark-tag-far", 4000, true),
        ("40-mark-raider-near", 420, false),
        ("40-mark-raider-far", 3200, false),
    ];
    for (name, m, tag) in cases {
        let blinks: &[u8] = if tag { &[1, 0] } else { &[0] };
        for &blink in blinks {
            d.app.world.ships = [None; world::SHIPS];
            let p = world::add(pos, scale14(r.col(2), m * M));
            let p = world::add(p, scale14(r.col(0), m * M / 12));
            let kind = if tag { Kind::Canister } else { Kind::Raider };
            let i = d.app.world.add_ship(kind, p, Mode::Travel).unwrap();
            let s = d.app.world.ships[i].as_mut().unwrap();
            s.pos = p;
            s.speed = 0;
            s.cool = 250;
            if tag {
                s.load = (world::TAG, 3);
                s.flash = blink;
                s.target = world::NOBODY;
            } else {
                s.target = world::PLAYER;
                s.mode = Mode::Hunt;
            }
            d.app.draw(&mut d.frame);
            let w = &d.app.world;
            let reach = w.in_reach(p);
            assert_eq!(reach, m < 1000, "{name}");
            let cam = scene::camera(w, d.app.opts.eye);
            let mut scratch = Box::new(game_kit::Frame::new());
            let (sh, x, y) = hud::ship_marks(&mut scratch, w, &cam).unwrap();
            assert_eq!(sh.pos, p, "{name}: the line names it");
            let mut t = hud::Txt::new();
            hud::target_text(&mut t, w, sh);
            let line = t.as_str().to_string();
            // (line and bar, a corner, a closed box)
            let (c, corner, boxed) = if tag {
                (TAG_MARK, TAG_MARK, false)
            } else if reach {
                (ACCENT, ACCENT, true)
            } else {
                (TEXT_DIM, BLIP_RAIDER, false)
            };
            let px = |x: i32, y: i32| d.frame.0[y as usize * game_kit::W + x as usize];
            assert!(
                shows(&d, |f| hud::centred(f, 87, line.as_bytes(), c), c),
                "{name}, blink {blink}: the line {line}"
            );
            for k in -2..=2 {
                assert_eq!(px(x + k, y + 12), c, "{name}, blink {blink}: the bar");
            }
            assert_eq!(px(x - 9, y - 9), corner, "{name}, blink {blink}: a corner");
            assert_eq!(
                px(x - 9, y) == ACCENT,
                boxed,
                "{name}, blink {blink}: the closed box"
            );
            let amber = d.frame.0[..96 * game_kit::W]
                .iter()
                .filter(|&&p| p == ACCENT)
                .count();
            if tag {
                assert_eq!(amber, 0, "{name}, blink {blink}: amber on the view");
            }
            println!(
                "{name}, blink {blink}: {line} at {x},{y} (laser reaches: {reach}), {amber} amber pixels on the view"
            );
            if blink == blinks[0] {
                shot(&mut d, name);
            }
        }
    }
}

// ---- Step G1: the planet ----------------------------------------------------------

/// The planet looks that have a surface, with their names.
const SURFACES: [u8; 7] = [0, 1, 2, 3, 5, 6, 7];

/// Step G1 (SPEC 3.14 item 7): every look below, from the circling
/// camera (about 40 m up) and from a pilot's eye on the pad (1.7 m), at
/// each detail level; and what each picture cost (triangles, pixels).
/// The cost rows go to `planet/cost.txt` beside the pictures.
#[test]
fn the_planet_looks_and_costs() {
    use mercator::planet::{self, Below};
    let dir = shots().join("planet");
    std::fs::create_dir_all(&dir).unwrap();
    let mut rows = String::from("look     detail eye     tris  pixels  land+ship\n");
    for &look in SURFACES.iter() {
        let mut d = common::below::below_in(look);
        assert_eq!(d.app.scene, Scene::Below);
        d.idle(30);
        let name = mercator::art::LOOK_NAMES[usize::from(look)].to_ascii_lowercase();
        // Step G2: the landing's views, flown (no fog, no streaks): the
        // camera high behind the ship at the landing start (the far
        // ring's whole reach in view), and low, 20 m over the pad with
        // the shadow; and G1's walk eye.
        let mut high: Below = d.app.below;
        high.phase = mercator::planet::lander::Phase::Fly;
        let st = high.places.start;
        high.p = st.map(|v| v << mercator::planet::lander::TO_WORLD);
        let yaw = planet::yaw_to(-st[0], -st[2]);
        high.yaw = yaw;
        let mut low = high;
        low.p = [
            0,
            (low.site.pad + 20 * M) << mercator::planet::lander::TO_WORLD,
            0,
        ];
        for det in Detail::ALL {
            d.app.opts.detail = det;
            for (eye, b) in [("high", high), ("low", low), ("walk", high)] {
                let cam = if eye == "walk" {
                    planet::walk_camera(&b, [0, 0], 0x2000)
                } else {
                    planet::camera::chase_at(&b, planet::ship_pos(&b), yaw)
                };
                d.app.render.begin();
                planet::draw_from(&mut d.frame, &mut d.app.render, &b, det, &cam);
                planet::panel(&mut d.frame, &b);
                let st = d.app.render.stats;
                rows += &format!(
                    "{:8} {:6} {:6} {:5} {:7}\n",
                    name,
                    det.name(),
                    eye,
                    st.tris,
                    st.pixels
                );
                if det == Detail::Mid || look == 0 {
                    let c = d.app.colours().to_vec();
                    save_frame(
                        &dir.join(format!(
                            "{name}-{eye}-{}.png",
                            det.name().to_ascii_lowercase()
                        )),
                        &d.frame,
                        &c,
                        3,
                    );
                }
            }
        }
    }
    std::fs::write(dir.join("cost.txt"), &rows).unwrap();
    println!("{rows}");
}
