//! Does risk pay? Four scripted pilots fly whole careers (launch,
//! fight, jump, auto-dock: the world's own rules) in each of the 10
//! galaxies until they hold the charter's 50,000 credits: one who
//! trades only in SECURE systems, a careful one who trades in SECURE
//! and QUIET systems, and a bold one who also uses RISKY and LAWLESS
//! ones; all three trade up to the cargo ship (DRAY, step M5: it took
//! the big bay's place) and buy the same gear at the same sums. The
//! fourth (step M5) is a bounty hunter: it trades carefully until it can
//! buy the combat ship (SHRIKE), then flies to RISKY systems (lawful:
//! their stations pay for TAGs), waits out at the arrival point for the
//! raiders (`HUNT_LINGER`), fights them, fetches their TAGs and cashes
//! them in at the dock, carrying what little cargo its hold takes. A lost ship goes back
//! to the last dock with the hold empty and the TAGs gone
//! (`Career::wrecked`), as in the game. After a fight the pilots fetch
//! the raiders' TAGs within 3 km and are paid for them at a lawful
//! dock. A LAWLESS station pays nothing, so once the bold pilot is
//! owed `BALANCE_CASH_AT` credits (default 2,500) in a LAWLESS system,
//! its next trip goes to a lawful one to cash them in, as a player
//! would; `BALANCE_CASH_AT=0` turns that off (it then cashes in only
//! when a trade happens to lead to a lawful station).
//!
//! `BALANCE_TRIPS` (default 400) caps a career's trips.

mod common;

use common::*;
use mercator::app::{GALAXIES, seed_of};
use mercator::galaxy;
use mercator::market::RISK_PAY;
use mercator::world::{self, Career, WIN_CREDITS, World};

/// One career's outcome.
#[derive(Debug, Default, Clone, Copy)]
struct Run {
    /// Jumps (one trip each) until 50,000 credits, or None.
    trips: Option<u32>,
    /// Days, minutes flown, ships lost, raiders downed.
    days: u32,
    minutes: f64,
    deaths: u32,
    kills: u32,
    /// Docks in RISKY or LAWLESS systems.
    risky_docks: u32,
    /// Credits after 30 trips (or when the goal was reached).
    at_30: u32,
    /// What happened to the raiders' TAGs, and how many were still
    /// carried (unpaid) at the end.
    tags: Tally,
    carried: u32,
    /// Step M5: the trip on which the pilot's ship was bought.
    bought: Option<u32>,
}

/// Gear every pilot buys, in this order, once credits pass the sum:
/// (gear, credits). Step M5: the big bay (first, at 2,000) is gone; the
/// ship plan ([`Pilot::ship`]) takes its place.
const GEAR_PLAN: [(usize, u32); 2] = [(0, 6_000), (2, 8_000)];

/// The ship is bought once the credits pass its cost (the flown ship
/// traded in) plus this, kept for cargo (as with the big bay: 2,000
/// credits for a 900 cr bay left 1,100).
const SHIP_KEEP: i64 = 1_100;

/// Step M5: how long the hunter waits out at the arrival point of a
/// RISKY system for raiders before it docks (3 minutes).
const HUNT_LINGER: u32 = 60 * 60 * 3;

/// A scripted pilot: the most dangerous systems it trades in, the ship
/// it trades up to, and whether it is the bounty hunter.
#[derive(Clone, Copy)]
struct Pilot {
    name: &'static str,
    max_danger: u8,
    ship: u8,
    hunts: bool,
}

const PILOTS: [Pilot; 4] = [
    Pilot {
        name: "secure",
        max_danger: 0,
        ship: world::DRAY,
        hunts: false,
    },
    Pilot {
        name: "careful",
        max_danger: 1,
        ship: world::DRAY,
        hunts: false,
    },
    Pilot {
        name: "bold",
        max_danger: 3,
        ship: world::DRAY,
        hunts: false,
    },
    Pilot {
        name: "hunter",
        max_danger: 1,
        ship: world::SHRIKE,
        hunts: true,
    },
];

/// The hunter's choice: the reachable RISKY system with the best profit
/// a tonne, or [`plan_any`]'s when none is in reach.
fn plan_hunt(w: &World) -> (u8, usize, i64) {
    let here = w.sys;
    let mut best: Option<(u8, usize, i64)> = None;
    for i in 0..galaxy::SYSTEMS {
        let s = galaxy::system(w.car.seed, i as u8);
        if s.id == here.id || here.dist(&s) > u32::from(w.car.fuel) || s.danger != 2 {
            continue;
        }
        if let Some((g, p)) = best_good(w, s.id)
            && best.is_none_or(|b| p > b.2)
        {
            best = Some((s.id, g, p));
        }
    }
    best.unwrap_or_else(|| plan_any(w, 1))
}

fn career(galaxy: u8, pilot: Pilot, max_trips: u32, cash_at: u32) -> Run {
    let max_danger = pilot.max_danger;
    let mut w = Box::new(World::empty());
    w.begin(Career::new(seed_of(galaxy)));
    let mut r = Run::default();
    let mut ticks = 0u64;
    let mut lost = false;
    for t in 0..max_trips {
        sell_all(&mut w);
        if t <= 30 {
            r.at_30 = w.car.credits;
        }
        if w.car.credits >= WIN_CREDITS {
            r.trips = Some(t);
            break;
        }
        // Step M5: the ship first, when it pays.
        let ship = usize::from(pilot.ship);
        if w.car.ship != pilot.ship
            && w.ship_sold_here(ship)
            && i64::from(w.car.credits) > SHIP_KEEP + i64::from(w.car.ship_cost(ship))
        {
            w.buy_ship(ship).unwrap();
            r.bought = Some(t);
        }
        for (g, sum) in GEAR_PLAN {
            if !w.car.has(g) && w.car.credits > sum && w.sold_here(g) {
                w.buy_gear(g).unwrap();
            }
        }
        let hunting = pilot.hunts && w.car.ship == pilot.ship;
        // Right after a lost ship the pilot keeps to the safe systems for
        // one trip (as a player would; else the same ambush can repeat).
        // Owed enough in a LAWLESS system (no bounty office): the next
        // trip goes to a lawful one.
        let cash = cash_at > 0
            && u32::from(w.car.owed) >= cash_at
            && !world::LAWLESS_PAYS
            && w.sys.danger >= 3;
        let careful = if lost {
            1
        } else if cash {
            max_danger.min(2)
        } else {
            max_danger
        };
        let (to, g, profit) = if hunting && !lost {
            plan_hunt(&w)
        } else {
            plan_any(&w, careful)
        };
        if profit > 0 {
            buy_max(&mut w, g, 0);
        }
        let linger = if hunting && galaxy::system(w.car.seed, to).danger == 2 {
            HUNT_LINGER
        } else {
            0
        };
        let last_dock = w.car;
        lost = false;
        // 20 minutes: a bold pilot in a raider system may fight and
        // fetch TAGs for more than 10 before it docks.
        match trip_hunt(&mut w, to, 60 * 60 * 20, &mut r.tags, linger) {
            Trip::Docked { ticks: k, kills } => {
                ticks += u64::from(k);
                r.kills += kills;
                if galaxy::system(w.car.seed, to).danger >= 2 {
                    r.risky_docks += 1;
                }
            }
            Trip::Died { ticks: k } => {
                ticks += u64::from(k);
                r.deaths += 1;
                lost = true;
                w.begin(last_dock.wrecked());
            }
            Trip::TimedOut => panic!("galaxy {galaxy}: a trip to {to} never ended"),
        }
    }
    r.days = u32::from(w.car.day);
    r.carried = u32::from(w.car.tags);
    r.minutes = ticks as f64 / 3600.0;
    r
}

fn trips_or_cap(r: &Run, cap: u32) -> u32 {
    r.trips.unwrap_or(cap)
}

/// A number from the environment, or `default`.
fn env_or(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[test]
fn risk_pays_and_the_goal_is_not_a_few_jumps_away() {
    let cap = env_or("BALANCE_TRIPS", 400);
    // All 10 galaxies take about 10 s in a release build; a debug build
    // is many times slower, so it flies the first 3 unless told.
    let galaxies = env_or(
        "BALANCE_GALAXIES",
        if cfg!(debug_assertions) { 3 } else { 10 },
    )
    .clamp(1, u32::from(GALAXIES)) as u8;
    println!("RISK_PAY {RISK_PAY}: trips (jumps) to {WIN_CREDITS} credits, cap {cap}");
    let cash_at = env_or("BALANCE_CASH_AT", 2_500);
    println!("the bold pilot leaves LAWLESS space to cash in at {cash_at} cr owed (0: never)");
    println!(
        "pilot: trips days minutes-flown ships-lost raiders-downed risky-docks credits-after-30-trips \
         tags-scooped/paid/lost-at-death ship-bought-on-trip"
    );
    const N: usize = PILOTS.len();
    let mut sum = [0u32; N];
    let mut lost = [0u32; N];
    let mut at_30 = [0u64; N];
    let mut kills = [0u32; N];
    let mut tags = [Tally::default(); N];
    let mut carried = [0u32; N];
    let mut reached = [0u32; N];
    let mut fewest = u32::MAX;
    let mut bold_slower = Vec::new();
    for g in 1..=galaxies {
        print!("galaxy {g:>2}");
        let mut row = [0u32; N];
        for (k, pilot) in PILOTS.into_iter().enumerate() {
            let name = pilot.name;
            let r = career(g, pilot, cap, cash_at);
            reached[k] += u32::from(r.trips.is_some());
            print!(
                " | {name} {} {} {:.0} {} {} {} {} {}/{}/{} {}",
                fmt(r.trips),
                r.days,
                r.minutes,
                r.deaths,
                r.kills,
                r.risky_docks,
                r.at_30,
                r.tags.scooped,
                r.tags.paid,
                r.tags.lost,
                fmt(r.bought)
            );
            kills[k] += r.kills;
            let (a, b) = (&mut tags[k], r.tags);
            a.dropped += b.dropped;
            a.scooped += b.scooped;
            a.paid += b.paid;
            a.paid_cr += b.paid_cr;
            a.unpaid_docks += b.unpaid_docks;
            a.lost += b.lost;
            carried[k] += r.carried;
            let t = trips_or_cap(&r, cap);
            row[k] = t;
            sum[k] += t;
            fewest = fewest.min(t);
            lost[k] += r.deaths;
            at_30[k] += u64::from(r.at_30);
        }
        println!();
        if row[2] >= row[1] {
            bold_slower.push(g);
        }
    }
    let n = f64::from(galaxies);
    for (k, pilot) in PILOTS.into_iter().enumerate() {
        let name = pilot.name;
        println!(
            "{name}: mean trips to the goal {:.1}, ships lost {}, mean credits after 30 trips {:.0}, \
             reached the goal in {} of {galaxies}",
            f64::from(sum[k]) / n,
            lost[k],
            at_30[k] as f64 / n,
            reached[k]
        );
        let t = tags[k];
        println!(
            "{name}: raiders downed on trips that docked {} (mean {:.1}); TAGs dropped {}, scooped {}, not scooped {}, \
             lost at death {}, paid {} for {} cr, still carried at the end {}, docks with TAGs left unpaid {}",
            kills[k],
            f64::from(kills[k]) / n,
            t.dropped,
            t.scooped,
            t.dropped - t.scooped,
            t.lost,
            t.paid,
            t.paid_cr,
            carried[k],
            t.unpaid_docks
        );
        // Each TAG scooped is paid, lost with the ship, or still carried.
        assert_eq!(t.paid + t.lost + carried[k], t.scooped);
        assert!(t.scooped <= t.dropped);
    }
    assert!(
        fewest >= 15,
        "50,000 credits must take more than a few jumps ({fewest})"
    );
    if RISK_PAY > 0 {
        assert!(sum[2] < sum[1], "the bold pilot must reach the goal sooner");
    }
    println!("galaxies where bold is not faster than careful: {bold_slower:?}");
    // Step M5: every pilot's career reaches the goal in every galaxy
    // flown (the cargo ship does not stall the traders, the combat ship
    // does not stall the hunter), and the hunter earns a real part of
    // its money from TAGs.
    for (k, pilot) in PILOTS.into_iter().enumerate() {
        assert_eq!(
            reached[k],
            u32::from(galaxies),
            "{}: the goal not reached in every galaxy",
            pilot.name
        );
    }
    let hunter = tags[3];
    println!(
        "hunter: TAGs paid {} cr in all, {:.0} cr a galaxy",
        hunter.paid_cr,
        f64::from(hunter.paid_cr) / n
    );
    // MEASURED ON THE PC at step M5 (all 10 galaxies, release): 25,522 cr
    // a galaxy, 3,508 raiders downed against careful's 272; the first 3
    // galaxies (debug): 13,833 cr a galaxy, 603 downed against 102.
    // 8,000 a galaxy is about a sixth of the goal.
    assert!(
        hunter.paid_cr >= 8_000 * u32::from(galaxies),
        "the hunter's TAGs pay too little: {} cr",
        hunter.paid_cr
    );
    assert!(
        kills[3] > kills[1],
        "the hunter must down more raiders than the careful trader"
    );
    // A bold pilot who cashes in its TAGs beats the careful one in at
    // least 4 galaxies of 5, and its mean stays under half the careful
    // one's: a broken TAG payment shows here (with the TAGs' rules as
    // built, a pilot who never fetches a TAG had a mean of 55.5 trips
    // against careful's 93.9 and was slower in 4 of the 10 galaxies).
    if RISK_PAY > 0 && cash_at > 0 {
        assert!(
            bold_slower.len() * 5 <= usize::from(galaxies),
            "the bold pilot is not faster than the careful one in galaxies {bold_slower:?}: are TAGs still paid?"
        );
        assert!(
            sum[2] * 2 < sum[1],
            "the bold pilot's mean is not under half the careful one's: are TAGs still paid?"
        );
    }
}

fn fmt(t: Option<u32>) -> String {
    t.map_or("-".to_string(), |t| t.to_string())
}
