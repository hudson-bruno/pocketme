//! An outside checker's attacks: the 3D view at the edges of the world
//! (corners of the world, inside the sun, on the planet, inside the
//! station, ships on the camera), the economy (no money from nothing),
//! saves made of random bytes, and taps anywhere on every screen. Run it
//! in release with the checks on:
//!
//! ```text
//! CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true \
//! CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true \
//! ATTACK_ROUNDS=20000 cargo test --release --test attack -- --nocapture
//! ```

mod common;

use common::Driver;
use game_kit::{Touch, button};
use mercator::app::{GALAXIES, Keys, Scene, seed_of};
use mercator::fixed::{Mat3, ONE14, V3, norm, scale14};
use mercator::galaxy::{self, SYSTEMS};
use mercator::market::{self, GOODS};
use mercator::save;
use mercator::scene::{Detail, Eye};
use mercator::world::{
    self, Beam, Blast, CREDITS_MAX, CRUISE_MAX, Career, KM, Kind, LIMIT, M, Mode, Pad, State, World,
};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi - lo + 1) as u32) as i32
    }
    fn dir(&mut self) -> V3 {
        loop {
            let v = [
                self.range(-1000, 1000),
                self.range(-1000, 1000),
                self.range(-1000, 1000),
            ];
            let n = norm(v, ONE14);
            if n != [0; 3] {
                return n;
            }
        }
    }
}

fn rounds(default: u32) -> u32 {
    std::env::var("ATTACK_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// A place the camera should survive: corners, the sun, the planet, the
/// station, anywhere.
fn nasty_place(r: &mut Rng, w: &World) -> V3 {
    let b = &w.bodies;
    let c = |v: V3| world::clamp_pos(v);
    match r.below(9) {
        0 => [0, 1, 2].map(|_| if r.below(2) == 0 { LIMIT } else { -LIMIT }),
        1 => [0, 1, 2].map(|_| r.range(-LIMIT, LIMIT)),
        // Just outside or inside the sun.
        2 => c(world::add(
            b.sun_pos,
            scale14(r.dir(), b.sun_r / 8 * r.range(7, 30)),
        )),
        // On or just above the planet.
        3 => c(scale14(r.dir(), b.planet_r + r.range(0, 3000) * M)),
        // Inside or round the station.
        4 => c(world::add(
            b.station_pos,
            scale14(r.dir(), r.range(0, 500) * M),
        )),
        // In the mouth.
        5 => c(world::add(
            b.station_pos,
            scale14(b.station_rot.col(2), r.range(0, 200) * M),
        )),
        // The planet's middle.
        6 => [0, 0, 0],
        // Far out on one axis, the sun on the other side.
        7 => c(scale14(b.sun_pos, -ONE14 * 5)),
        _ => c(world::add(
            b.station_pos,
            scale14(r.dir(), r.range(1, 60) * KM),
        )),
    }
}

/// The 3D view, the panel and every overlay, drawn from the worst
/// places with every setting; the world ticks there too.
#[test]
fn the_view_survives_the_edges_of_the_world() {
    let n = rounds(3000);
    let mut r = Rng(0x1234_5678_9ABC_DEF1);
    let mut d = Driver::new();
    d.start();
    let mut most_tris = 0;
    let mut most_px = 0;
    for round in 0..n {
        // Another system now and then (other bodies, giants, big suns).
        if round % 20 == 0 {
            let seed = if r.below(2) == 0 {
                seed_of(1 + r.below(u32::from(GALAXIES)) as u8)
            } else {
                r.next()
            };
            let mut c = Career::new(seed);
            c.system = r.below(galaxy::SYSTEMS as u32) as u8;
            d.app.world.begin(c);
            d.app.world.launch();
            d.app.scene = Scene::Flight;
        }
        let place = nasty_place(&mut r, &d.app.world);
        let w = &mut d.app.world;
        w.state = State::Flying;
        w.car.hull = 100;
        w.me.pos = place;
        w.me.rot = Mat3::rotation(r.next() as u16, r.next() as u16, r.next() as u16);
        w.me.speed = match r.below(3) {
            0 => 0,
            1 => CRUISE_MAX,
            _ => r.range(0, CRUISE_MAX),
        };
        w.me.cruise = w.me.speed > world::MAX_SPEED;
        w.ships = [None; world::SHIPS];
        for _ in 0..r.below(9) {
            let kind =
                [Kind::Raider, Kind::Hauler, Kind::Warden, Kind::Canister][r.below(4) as usize];
            let off = match r.below(4) {
                0 => [0; 3],
                1 => scale14(r.dir(), r.range(0, 60) * M),
                2 => scale14(r.dir(), r.range(0, 8000) * M),
                _ => scale14(r.dir(), r.range(0, 22) * KM),
            };
            let p = world::clamp_pos(world::add(place, off));
            let mode = [Mode::Hunt, Mode::Break, Mode::Flee, Mode::Travel][r.below(4) as usize];
            if let Some(i) = w.add_ship(kind, p, mode)
                && let Some(s) = w.ships[i].as_mut()
            {
                s.rot = Mat3::rotation(r.next() as u16, r.next() as u16, r.next() as u16);
                s.angry = r.below(2) == 0;
            }
        }
        for k in 0..4 {
            let at = world::clamp_pos(world::add(place, scale14(r.dir(), r.range(0, 3000) * M)));
            w.blasts[k] = Blast {
                pos: at,
                age: r.below(60) as u8,
                size: r.below(256) as u8,
            };
            w.beams[k] = Beam {
                from: place,
                to: world::clamp_pos(world::add(place, scale14(r.dir(), r.range(0, 9000) * M))),
                foe: r.below(2) == 0,
                age: r.below(5) as u8,
                // Step M3: every bolt colour, without a draw of the dice
                // (the places stay as before).
                lv: (k % 3) as u8,
            };
        }
        w.wanted = r.below(3) == 0;
        // Every setting draws this place.
        for det in Detail::ALL {
            for eye in Eye::ALL {
                d.app.opts.detail = det;
                d.app.opts.eye = eye;
                d.app.draw(&mut d.frame);
                let s = d.app.render.stats;
                most_tris = most_tris.max(s.tris);
                most_px = most_px.max(s.pixels);
                assert_eq!(s.dropped, 0, "round {round}: triangles dropped");
            }
        }
        // A few ticks there, with wild controls and draws.
        for _ in 0..r.below(30) {
            let pad = Pad {
                steer: (r.range(-256, 256), r.range(-256, 256)),
                roll: r.range(-256, 256),
                throttle: r.range(-256, 256),
                fire: r.below(2) == 0,
                cruise: r.below(10) == 0,
                auto: r.below(10) == 0,
                turret: r.below(3) == 0,
                boost: r.below(2) == 0,
            };
            d.app.world.tick(&pad);
            while d.app.world.take_event().is_some() {}
            if d.app.world.state != State::Flying {
                break;
            }
            d.app.draw(&mut d.frame);
        }
        assert!(d.app.world.car.credits <= CREDITS_MAX);
    }
    println!("{n} nasty places: most triangles {most_tris}, most pixels {most_px}");
}

/// No money from nothing: at any station on any day, selling what was
/// just bought there gives back less; prices stay positive; a day far in
/// the future works too.
#[test]
fn no_money_from_nothing() {
    let mut checked = 0u64;
    for g in 1..=GALAXIES {
        let seed = seed_of(g);
        for id in 0..SYSTEMS {
            let s = galaxy::system(seed, id as u8);
            for day in (0..200u16).chain([u16::MAX - 1, u16::MAX]) {
                for good in 0..GOODS {
                    let buy = market::price(&s, good, day);
                    let sell = market::sell_price(&s, good, day);
                    assert!(buy >= 1 && sell >= 1);
                    assert!(
                        sell < buy,
                        "{g}/{id} day {day} good {good}: {sell} >= {buy}"
                    );
                    checked += 1;
                }
            }
        }
    }
    println!("{checked} prices checked");
    // The world's own buying and selling, over and over at one dock: the
    // credits only fall.
    let mut w = Box::new(World::empty());
    w.begin(Career::new(seed_of(1)));
    let mut r = Rng(77);
    let mut before = w.car.credits;
    for _ in 0..100_000 {
        let g = r.below(GOODS as u32) as usize;
        if r.below(2) == 0 {
            w.buy(g);
        } else {
            w.sell(g);
        }
        // Money plus what the hold would fetch here never grows.
        let worth = w.car.credits
            + (0..GOODS)
                .map(|g| u32::from(w.car.hold.0[g]) * w.sell_price(g))
                .sum::<u32>();
        assert!(worth <= before, "worth grew: {worth} > {before}");
        before = worth;
    }
    // Selling what is not in the hold, buying with no money or no room.
    let mut w = Box::new(World::empty());
    w.begin(Career::new(seed_of(2)));
    w.car.credits = 0;
    for g in 0..GOODS {
        assert!(!w.sell(g));
        assert!(!w.buy(g));
    }
    assert!(!w.sell(GOODS) && !w.buy(GOODS) && w.buy_gear(99).is_err());
    assert_eq!(w.buy_fuel(), 0);
    assert_eq!(w.repair(), 0);
    // Not while flying.
    w.car.credits = 10_000;
    w.launch();
    for g in 0..GOODS {
        assert!(!w.buy(g) && !w.sell(g));
    }
}

/// Random bytes, and random bytes with a right CRC: refused, or a career
/// that plays (it flies, fights, jumps, docks and draws) without a fault.
#[test]
fn random_saves_are_refused_or_play() {
    let n = rounds(3000);
    let mut r = Rng(0xFEED_F00D_1234_5678);
    let (mut refused, mut played) = (0u32, 0u32);
    let mut d = Driver::new();
    d.start();
    for round in 0..n * 30 {
        let mut b = [0u8; save::SIZE];
        for x in b.iter_mut() {
            *x = r.next() as u8;
        }
        if round % 2 == 0 {
            // A right version and CRC, the rest random but often in range.
            b[0] = save::VERSION;
            if r.below(2) == 0 {
                b[6..10].copy_from_slice(&r.below(CREDITS_MAX + 1).to_le_bytes());
                b[10] = r.below(u32::from(world::TANK) + 1) as u8;
                for v in &mut b[11..19] {
                    *v = r.below(5) as u8;
                }
                // Step M5: bit 4 (the old bay) is zero too, the ship
                // byte names one of the three, and the hull fits it.
                b[19] &= 0x6F;
                b[19] = (b[19] & !0x0F) | (r.below(3) as u8) | (r.below(3) as u8) << 2;
                b[47] = r.below(world::CRAFTS.len() as u32) as u8;
                b[20] = 1 + r.below(u32::from(world::CRAFTS[usize::from(b[47])].hull)) as u8;
                b[26] = r.below(3) as u8 | (r.below(8) as u8) << 2;
                b[5] = r.below(galaxy::SYSTEMS as u32) as u8;
                b[25] = r.below(galaxy::SYSTEMS as u32) as u8;
                // TAGs worth 25, 50, 75 or 100 each (up to the 255
                // limit).
                let tags = r.below(256);
                b[44] = tags as u8;
                let owed = 25 * (tags + r.below(tags * 3 + 1));
                b[45..47].copy_from_slice(&(owed as u16).to_le_bytes());
            }
            let crc = save::crc32(&b[..save::SIZE - 4]);
            b[save::SIZE - 4..].copy_from_slice(&crc.to_le_bytes());
        }
        match save::from_bytes(&b) {
            Err(_) => refused += 1,
            Ok((c, settings)) => {
                // What comes back saves to the same bytes.
                assert_eq!(save::to_bytes(&c, settings), b, "round {round}");
                if played < n {
                    played += 1;
                    play_a_while(&mut d, c, &mut r);
                }
            }
        }
    }
    // Every single flipped bit of a good save is refused.
    let good = save::to_bytes(&Career::new(seed_of(3)), 0);
    for bit in 0..save::SIZE * 8 {
        let mut b = good;
        b[bit / 8] ^= 1 << (bit % 8);
        assert!(save::from_bytes(&b).is_err(), "bit {bit} slipped through");
    }
    println!("{refused} refused, {played} accepted and played");
    assert!(played > 0);
}

fn play_a_while(d: &mut Driver, c: Career, r: &mut Rng) {
    d.app.world.begin(c);
    d.app.last_dock = c;
    d.app.scene = Scene::Docked;
    d.app.pause = None;
    d.app.sel = 0;
    d.idle(1);
    let w = &mut d.app.world;
    for g in 0..GOODS {
        while w.sell(g) {}
    }
    w.buy_fuel();
    w.repair();
    for i in 0..6 {
        let _ = w.buy_gear(i);
    }
    // Launch, a jump if one is in reach, then random flying.
    d.app.sel = 0;
    d.tap(button::A);
    let to = d.app.world.car.target;
    if d.app.world.can_jump(to).is_ok() {
        d.app.world.jump(to);
    }
    for _ in 0..r.below(400) {
        let k = Keys {
            held: r.next() & 0x0FFF & !button::START,
            pressed: 0,
            left: (r.next() as i16, r.next() as i16),
            right: (r.next() as i16, r.next() as i16),
            triggers: (r.next() as u16, r.next() as u16),
            touch: None,
            gunner: r.below(3) == 0,
        };
        d.call_keys(k);
    }
}

/// Taps at every spot of every screen, also off the picture (outside the
/// console's promise, 0..160 x 0..120, but cheap to survive).
#[test]
fn taps_anywhere_on_every_screen() {
    let mut d = Driver::new();
    d.start();
    let scenes = [
        Scene::Title,
        Scene::Docked,
        Scene::Market,
        Scene::Outfit,
        Scene::Chart,
        Scene::Flight,
        Scene::Lost,
        Scene::Won,
    ];
    let spots: Vec<(i16, i16)> = (-4..=165)
        .step_by(3)
        .flat_map(|x| (-4..=125).step_by(3).map(move |y| (x as i16, y as i16)))
        .collect();
    for sc in scenes {
        for &(x, y) in &spots {
            let mut c = Career::new(seed_of(1));
            c.credits = 20_000;
            d.app.world.begin(c);
            d.app.playing = true;
            d.app.pause = None;
            if sc == Scene::Flight || sc == Scene::Lost {
                d.app.world.launch();
            }
            if sc == Scene::Lost {
                d.app.world.car.hull = 0;
                d.app.world.state = State::Dead;
            }
            d.app.scene = sc;
            d.app.scene_t = 200;
            d.app.sel = 0;
            let t = Touch {
                x,
                y,
                down: true,
                started: true,
                ended: false,
            };
            let going = d.call_keys(Keys {
                touch: Some(t),
                ..Keys::default()
            });
            if !going {
                d = Driver::new();
                d.start();
                continue;
            }
            d.call_keys(Keys {
                touch: Some(Touch {
                    down: false,
                    started: false,
                    ended: true,
                    ..t
                }),
                ..Keys::default()
            });
            let w = &d.app.world;
            assert!(w.car.credits <= CREDITS_MAX);
            assert!(w.car.hold.total() <= w.car.capacity());
        }
    }
}

/// Raiders always come for the player, and a player who does nothing in
/// a lawless system is found and loses the ship (a fight can be lost);
/// a raider can always be killed by the starting laser.
#[test]
fn raiders_find_a_sitting_player() {
    for g in 1..=GALAXIES {
        let seed = seed_of(g);
        let id = (0..galaxy::SYSTEMS as u8)
            .find(|&i| galaxy::system(seed, i).danger == 3)
            .unwrap();
        let mut c = Career::new(seed);
        c.system = id;
        let mut w = Box::new(World::empty());
        w.begin(c);
        w.launch();
        // Away from the station, then sit still.
        let out = w.bodies.station_rot.col(2);
        w.me.pos = world::add(w.bodies.station_pos, scale14(out, 20 * KM));
        let mut t = 0u32;
        let stop = Pad {
            throttle: -256,
            ..Pad::default()
        };
        while w.state == State::Flying && t < 60 * 60 * 30 {
            w.tick(&stop);
            while w.take_event().is_some() {}
            t += 1;
        }
        assert_eq!(
            w.state,
            State::Dead,
            "galaxy {g}: a sitting player lived 30 min"
        );
        println!(
            "galaxy {g}: a sitting player in a LAWLESS system lasts {:.0} s",
            t as f64 / 60.0
        );
    }
}
