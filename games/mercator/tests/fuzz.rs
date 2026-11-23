//! Long random play: no panic, no overflow, no rule broken, no stuck
//! state. Run it long in release with the checks on:
//!
//! ```text
//! CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true \
//! CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true \
//! FUZZ_MINUTES=600 FUZZ_HOURS=20 cargo test --release --test fuzz -- --nocapture
//! ```
//!
//! `FUZZ_MINUTES`: game minutes of the whole game (menus, pictures) with
//! random keys and taps. `FUZZ_HOURS`: game hours of the world alone
//! (flying, fighting, trading, jumping) with a random pilot.

mod common;

use common::{Driver, buy_max, fight_pad, hunter, sell_all, steer_to};
use game_kit::{Touch, button};
use mercator::app::{App, Keys, Scene};
use mercator::galaxy::SYSTEMS;
use mercator::market::GOODS;
use mercator::world::{
    self, CREDITS_MAX, CRUISE_MAX, Career, HEAT_MAX, LIMIT, Pad, State, TANK, World,
};

/// A small random source (xorshift), seeded per run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
    fn chance(&mut self, percent: u32) -> bool {
        self.below(100) < percent
    }
    fn stick(&mut self) -> i16 {
        match self.below(4) {
            0 => 0,
            1 => 32_767,
            2 => -32_767,
            _ => self.next() as i16,
        }
    }
}

fn env_num(name: &str, default: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Every rule the world must keep, at any moment.
fn check_world(w: &World, at: &str) {
    let c = &w.car;
    assert!(c.credits <= CREDITS_MAX, "{at}: credits {}", c.credits);
    // Step M5: each ship's own hull, never past the toughest's.
    assert!(i32::from(c.hull) <= c.hull_max(), "{at}: hull {}", c.hull);
    assert!(c.hull_max() <= world::HULL_MOST && usize::from(c.ship) < world::CRAFTS.len());
    assert!(c.fuel <= TANK, "{at}: fuel {}", c.fuel);
    assert!(c.hold.total() <= c.capacity(), "{at}: hold {:?}", c.hold);
    assert!(c.laser <= 2 && c.shield <= 2, "{at}: gear");
    assert!(usize::from(c.system) < SYSTEMS && usize::from(c.target) < SYSTEMS);
    // Each TAG is worth 25, 50, 75 or 100 (TAG_VALUE times its gun
    // level plus half its kill system's danger, at most 4: step M1).
    let tags = u32::from(c.tags) * u32::from(world::TAG_VALUE);
    let most = tags * mercator::galaxy::DANGER_NAMES.len() as u32;
    assert!(
        (tags..=most).contains(&u32::from(c.owed)) && c.owed.is_multiple_of(world::TAG_VALUE),
        "{at}: {} TAGs worth {}",
        c.tags,
        c.owed
    );
    let me = &w.me;
    assert!(
        me.pos.iter().all(|v| v.abs() <= LIMIT),
        "{at}: pos {:?}",
        me.pos
    );
    assert!(
        (0..=CRUISE_MAX).contains(&me.speed),
        "{at}: speed {}",
        me.speed
    );
    assert!((0..=w.top()).contains(&me.throttle), "{at}: throttle");
    assert!((0..=HEAT_MAX).contains(&me.heat), "{at}: heat {}", me.heat);
    assert!(
        (0..=w.shield_max()).contains(&me.shield),
        "{at}: shield {}",
        me.shield
    );
    assert!(
        me.rates.iter().all(|r| r.abs() <= 420),
        "{at}: rates {:?}",
        me.rates
    );
    // The turn stays a turn: every column about one long.
    for k in 0..3 {
        let l = mercator::fixed::length(me.rot.col(k));
        assert!((15_000..=17_800).contains(&l), "{at}: axis {k} is {l} long");
    }
    for s in w.ships.iter().flatten() {
        assert!(s.pos.iter().all(|v| v.abs() <= LIMIT), "{at}: ship pos");
        assert!(s.speed.abs() <= CRUISE_MAX, "{at}: ship speed {}", s.speed);
    }
    for g in 0..GOODS {
        assert!(w.price(g) > 0, "{at}: price of {g}");
    }
    if w.state == State::Dead {
        assert_eq!(c.hull, 0, "{at}: dead with a hull");
    }
}

// ---- The whole game, with random keys ------------------------------------------------

/// Random keys for one call.
fn random_keys(r: &mut Rng, held: &mut u32, left: &mut u32) -> Keys {
    const BUTTONS: [u32; 12] = [
        button::A,
        button::A,
        button::A,
        button::B,
        button::X,
        button::Y,
        button::L,
        button::R,
        button::UP,
        button::DOWN,
        button::LEFT,
        button::RIGHT,
    ];
    let before = *held;
    if *left == 0 {
        *left = 1 + r.below(90);
        *held = 0;
        for _ in 0..r.below(3) {
            *held |= BUTTONS[r.below(BUTTONS.len() as u32) as usize];
        }
        // Rare: the menu and the chart.
        if r.chance(1) {
            *held |= button::START;
        }
        if r.chance(3) {
            *held |= button::SELECT;
        }
    }
    *left -= 1;
    let touch = if r.chance(3) {
        Some(Touch {
            x: r.below(170) as i16 - 5,
            y: r.below(130) as i16 - 5,
            down: r.chance(70),
            started: r.chance(50),
            ended: r.chance(30),
        })
    } else {
        None
    };
    Keys {
        held: *held,
        pressed: *held & !before,
        left: (r.stick(), r.stick()),
        right: (r.stick(), r.stick()),
        triggers: (r.next() as u16 / 2, r.next() as u16 / 2),
        touch,
        gunner: r.chance(10),
    }
}

/// From wherever the game is: to the title by the keys a player would
/// use, then back into the career. Fails when that takes too long (a
/// stuck state).
fn escape_and_return(d: &mut Driver) {
    for _ in 0..2000 {
        if d.app.scene == Scene::Title && d.app.pause.is_none() {
            break;
        }
        if let Some(sel) = d.app.pause {
            // The pause menu: down to "QUIT TO TITLE" (line 5), A.
            if sel == 5 {
                d.tap(button::A);
            } else {
                d.tap(button::DOWN);
            }
            continue;
        }
        match d.app.scene {
            Scene::Jump => d.idle(1),
            Scene::Lost => {
                d.wait_card();
                d.tap(button::B);
            }
            Scene::Won => {
                d.wait_card();
                d.tap(button::A);
            }
            Scene::Market | Scene::Outfit => d.tap(button::B),
            _ => d.tap(button::START),
        }
    }
    assert_eq!(d.app.scene, Scene::Title, "stuck in {:?}", d.app.scene);
    // CONTINUE (or PLAY): line 0.
    for _ in 0..10 {
        if d.app.sel == 0 {
            break;
        }
        d.tap(button::DOWN);
    }
    d.tap(button::A);
    assert_ne!(d.app.scene, Scene::Title, "the title would not let go");
}

#[test]
fn random_keys_never_break_the_game() {
    let minutes = env_num("FUZZ_MINUTES", 20.0);
    let calls = (minutes * 60.0 * 60.0) as u64;
    let mut r = Rng(0x9E37_79B9_7F4A_7C15 ^ env_num("FUZZ_SEED", 1.0) as u64);
    let mut d = Driver::new();
    d.start();
    let (mut held, mut left) = (0u32, 0u32);
    let mut in_scene = [0u64; 9];
    let mut leaves = 0;
    let mut since_escape = 0u64;
    let mut escapes = 0;
    let mut macros = (0u32, 0u32);
    let mut seen = [0u32; 9];
    let mut prev = d.app.scene;
    let mut flight_ticks = (0u64, d.app.world.ticks);
    for call in 0..calls {
        // Now and then a player's move instead of noise: a jump through
        // the chart, or the auto-dock (sometimes with the charter sum on
        // board, so the win card shows too).
        if d.app.scene == Scene::Flight && d.app.pause.is_none() && left == 0 && r.chance(4) {
            if r.chance(60) {
                d.tap(button::SELECT);
                for _ in 0..r.below(6) {
                    d.tap(
                        [button::UP, button::DOWN, button::LEFT, button::RIGHT]
                            [r.below(4) as usize],
                    );
                }
                d.tap(button::A);
                macros.0 += 1;
            } else {
                if r.chance(25) {
                    d.app.world.car.credits = d.app.world.car.credits.max(world::WIN_CREDITS);
                    d.app.world.car.charted = [0xFF; world::CHART_BYTES];
                }
                d.tap(button::Y);
                for _ in 0..90 * 60 {
                    if d.app.scene != Scene::Flight || d.app.pause.is_some() {
                        break;
                    }
                    d.hold(0);
                }
                macros.1 += 1;
            }
            held = 0;
            check_world(&d.app.world, "after a move");
            continue;
        }
        let k = random_keys(&mut r, &mut held, &mut left);
        let going = d.call_keys(k);
        if !going {
            // LEAVE was chosen: a new start, as the console would do.
            leaves += 1;
            d = Driver::new();
            d.start();
            flight_ticks.1 = d.app.world.ticks;
            continue;
        }
        in_scene[d.app.scene as usize] += 1;
        if d.app.scene != prev {
            seen[d.app.scene as usize] += 1;
            // Back at the dock after a loss: no cargo, no TAGs.
            if prev == Scene::Lost && d.app.scene == Scene::Docked {
                let c = d.app.world.car;
                assert_eq!((c.hold.total(), c.tags, c.owed), (0, 0, 0), "call {call}");
            }
            prev = d.app.scene;
        }
        check_world(&d.app.world, &format!("call {call} in {:?}", d.app.scene));
        // While the flight scene runs unpaused, world time goes on.
        if d.app.scene == Scene::Flight && d.app.pause.is_none() {
            flight_ticks.0 += 1;
            if flight_ticks.0.is_multiple_of(600) {
                assert!(d.app.world.ticks > flight_ticks.1, "flight stands still");
                flight_ticks.1 = d.app.world.ticks;
            }
        }
        since_escape += 1;
        if since_escape >= 5 * 3600 {
            since_escape = 0;
            escapes += 1;
            escape_and_return(&mut d);
            check_world(&d.app.world, "after an escape");
            // Some flying: launch from the dock.
            if d.app.scene == Scene::Docked {
                d.app.sel = 0;
                d.tap(button::A);
            }
        }
    }
    let names = [
        "title", "docked", "market", "outfit", "chart", "flight", "jump", "lost", "won",
    ];
    let share: Vec<String> = names
        .iter()
        .zip(in_scene)
        .map(|(n, c)| format!("{n} {:.1}%", c as f64 * 100.0 / calls as f64))
        .collect();
    let mut kinds = std::collections::BTreeMap::new();
    for e in &d.events {
        *kinds
            .entry(
                format!("{e:?}")
                    .split([' ', '{', '('])
                    .next()
                    .unwrap()
                    .to_string(),
            )
            .or_insert(0u32) += 1;
    }
    println!(
        "{minutes} game minutes ({calls} calls): {}; {leaves} leaves, {escapes} escapes",
        share.join(", ")
    );
    println!("events in the last life: {kinds:?}");
    println!(
        "scenes entered: {:?}; chart jumps tried {}, auto-docks {}",
        names.iter().zip(seen).collect::<Vec<_>>(),
        macros.0,
        macros.1
    );
}

// ---- The world alone, with a random pilot ---------------------------------------------

/// What the random pilot does for a while.
#[derive(Clone, Copy, Debug)]
enum Plan {
    /// Random sticks and buttons.
    Wild,
    /// Fly to a system and dock there (trading on the way).
    Trade,
    /// Fight whoever hunts, else wander.
    Fight,
}

fn random_pad(r: &mut Rng) -> Pad {
    let v = |r: &mut Rng| match r.below(3) {
        0 => 0,
        1 => 256 - 512 * r.below(2) as i32,
        _ => r.below(513) as i32 - 256,
    };
    Pad {
        steer: (v(r), v(r)),
        roll: v(r),
        throttle: v(r),
        fire: r.chance(40),
        cruise: r.chance(1),
        auto: r.chance(1),
        turret: r.chance(20),
        // Step M4: the faster key held (overboost with a cruise press).
        boost: r.chance(50),
    }
}

/// Does random docked things: trade, gear, fuel, repair.
fn docked_turn(w: &mut World, r: &mut Rng) {
    for _ in 0..r.below(12) {
        let g = r.below(GOODS as u32) as usize;
        match r.below(6) {
            0 => {
                w.buy(g);
            }
            1 => {
                w.sell(g);
            }
            2 => {
                // Step M5: now and then a ship (4: one past the table).
                if r.below(4) == 0 {
                    let _ = w.buy_ship(r.below(4) as usize);
                } else {
                    let _ = w.buy_gear(r.below(6) as usize);
                }
            }
            3 => {
                w.buy_fuel();
            }
            4 => {
                w.repair();
            }
            _ => {
                buy_max(w, g, r.below(500));
            }
        }
        check_world(w, "docked");
    }
}

#[test]
fn a_random_pilot_never_breaks_the_world() {
    let hours = env_num("FUZZ_HOURS", 1.0);
    let ticks = (hours * 3600.0 * 60.0) as u64;
    let mut r = Rng(0xD1B5_4A32_D192_ED03 ^ env_num("FUZZ_SEED", 1.0) as u64);
    let mut w = Box::new(World::empty());
    let mut galaxy = 1u8;
    w.begin(Career::new(mercator::app::seed_of(galaxy)));
    let mut last_dock = w.car;
    let mut plan = Plan::Trade;
    let mut plan_left = 0u32;
    let mut target = w.car.system;
    let (mut docks, mut jumps, mut deaths, mut kills, mut wins, mut lives) = (0, 0, 0, 0u32, 0, 1);
    let mut max_credits = 0;
    let mut strandings = 0u32;
    let mut flying_since = 0u64;
    let mut longest_flight = 0u64;
    let mut t = 0u64;
    while t < ticks {
        match w.state {
            State::Docked => {
                last_dock = w.car;
                if w.car.charter_ready() {
                    wins += 1;
                    // A new career now and then, in another galaxy.
                    galaxy = galaxy % 8 + 1;
                    w.begin(Career::new(mercator::app::seed_of(galaxy)));
                    lives += 1;
                    continue;
                }
                sell_all(&mut w);
                docked_turn(&mut w, &mut r);
                let max_danger = r.below(4) as u8;
                if let Some((to, g, profit)) =
                    common::plan(&w, max_danger).or_else(|| common::plan(&w, 3))
                {
                    if profit > 0 && r.chance(80) {
                        buy_max(&mut w, g, 0);
                    }
                    target = to;
                } else {
                    // Stranded (the random turn spent the fuel money): the
                    // way out a player has too, launch and dock again with
                    // an empty hold for the free tank.
                    sell_all(&mut w);
                    strandings += 1;
                    target = w.car.system;
                }
                w.launch();
                flying_since = t;
                plan = if r.chance(70) {
                    Plan::Trade
                } else {
                    Plan::Wild
                };
                plan_left = 600 + r.below(6000);
            }
            State::Dead => {
                deaths += 1;
                // The card shows; the world goes on a little; then back to
                // the last dock (or, now and then, a new career).
                for _ in 0..120 {
                    w.tick(&Pad::default());
                    while w.take_event().is_some() {}
                    t += 1;
                }
                if r.chance(20) {
                    w.begin(Career::new(mercator::app::seed_of(galaxy)));
                    lives += 1;
                } else {
                    w.begin(last_dock.wrecked());
                    // The wreck's cargo and TAGs are gone.
                    assert_eq!(w.car.hold.total(), 0);
                    assert_eq!((w.car.tags, w.car.owed), (0, 0));
                }
                continue;
            }
            State::Flying => {}
        }
        longest_flight = longest_flight.max(t - flying_since);
        if plan_left == 0 {
            plan = match r.below(10) {
                0..=5 => Plan::Trade,
                6..=7 => Plan::Fight,
                _ => Plan::Wild,
            };
            plan_left = 300 + r.below(3600);
        }
        plan_left -= 1;
        let pad = match plan {
            Plan::Wild => random_pad(&mut r),
            Plan::Fight => match hunter(&w, 8 * world::KM) {
                Some(i) => fight_pad(&w, i),
                None => random_pad(&mut r),
            },
            Plan::Trade => {
                if let Some(i) = hunter(&w, 5 * world::KM) {
                    fight_pad(&w, i)
                } else if target != w.car.system {
                    if w.can_jump(target).is_ok() {
                        w.jump(target);
                        jumps += 1;
                        Pad::default()
                    } else if w.car.fuel < 10 {
                        target = w.car.system;
                        Pad::default()
                    } else {
                        let away = world::add(w.me.pos, w.me.rot.col(2));
                        Pad {
                            steer: steer_to(&w, away),
                            throttle: 256,
                            cruise: !w.me.cruise && w.me.throttle >= w.top() && w.room() > 0,
                            ..Pad::default()
                        }
                    }
                } else {
                    Pad {
                        auto: !w.me.auto && t.is_multiple_of(60),
                        fire: false,
                        ..Pad::default()
                    }
                }
            }
        };
        let k0 = w.car.kills;
        w.tick(&pad);
        while let Some(e) = w.take_event() {
            if e == world::Event::Docked {
                docks += 1;
            }
        }
        kills += u32::from(w.car.kills.saturating_sub(k0));
        max_credits = max_credits.max(w.car.credits);
        t += 1;
        if t.is_multiple_of(7) {
            check_world(&w, &format!("tick {t}, {plan:?}"));
        }
        // Nobody flies for ever: after 40 game minutes in one system,
        // the pilot goes back to trading (auto-dock), and the run fails
        // if a flight passes three hours.
        if t - flying_since > 40 * 3600 && !matches!(plan, Plan::Trade) {
            plan = Plan::Trade;
            target = w.car.system;
            plan_left = 60 * 3600;
        }
        assert!(
            t - flying_since < 3 * 60 * 3600,
            "a flight that never ends ({:?}, auto {}, target {} in {}, fuel {}, station {} m, \
             from the middle {} km, speed {}, room {}, hunter {:?}, ships {:?})",
            plan,
            w.me.auto,
            target,
            w.car.system,
            w.car.fuel,
            w.station_dist() / world::M,
            mercator::fixed::length(w.me.pos) / world::KM,
            w.me.speed,
            w.room(),
            hunter(&w, 20 * world::KM),
            w.ships
                .iter()
                .flatten()
                .map(|s| (
                    s.kind,
                    s.mode,
                    s.target,
                    mercator::fixed::length(mercator::fixed::sub(s.pos, w.me.pos)) / world::M
                ))
                .collect::<Vec<_>>()
        );
        if w.state == State::Docked {
            flying_since = t;
        }
    }
    println!(
        "{hours} game hours: {lives} careers, {docks} docks, {jumps} jumps, {kills} kills, \
         {deaths} deaths, {wins} wins, {strandings} strandings; most credits {max_credits}; longest flight {:.1} min",
        longest_flight as f64 / 3600.0
    );
    assert!(docks > 0 && jumps > 0, "the pilot never got anywhere");
}

#[test]
fn a_fresh_app_is_sane() {
    let a = App::new();
    check_world(&a.world, "new");
}
