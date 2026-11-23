//! Risk pays: the danger rule in the prices (`market::RISK_PAY`).
//! SECURE prices are exactly the old ones; danger only widens the gap
//! between what an economy makes and what it wants; a station never
//! pays back what it sold for; no new money loop between neighbours.

use mercator::app::{GALAXIES, seed_of};
use mercator::fixed::hash;
use mercator::galaxy::{self, SYSTEMS, System};
use mercator::market::{self, BASE, GOODS, RISK_PAY, made_here};

/// Days to check: the first months, and the far end of the counter.
fn days() -> impl Iterator<Item = u16> {
    (0..200u16).chain([1000, u16::MAX - 1, u16::MAX])
}

/// Every system of the 10 galaxies the title menu offers.
fn systems() -> Vec<System> {
    (1..=GALAXIES)
        .flat_map(|g| (0..SYSTEMS as u8).map(move |i| galaxy::system(seed_of(g), i)))
        .collect()
}

/// Per economy and good, the economy's want (a copy of `market`'s own
/// table): below 0 it makes the good, above 0 it wants it.
const AFF: [[i32; GOODS]; 5] = [
    [-3, 1, 1, 1, 2, 1, 2, 0],
    [3, -3, -3, 0, 2, 2, 1, 1],
    [2, 2, 3, -3, -2, 1, 0, 2],
    [1, 1, 0, 2, 1, -3, -3, 1],
    [2, 1, -1, 1, 1, 2, 2, -3],
];

/// The price rule before danger counted (copied from the old
/// `market::price`), worked in plain checked numbers.
fn old_price(s: &System, good: usize, day: u16) -> u32 {
    let aff = AFF[s.econ as usize][good];
    let own = (hash(s.h, good as u32) % 13) as i32 - 6;
    let drift = (hash(s.h ^ u32::from(day).wrapping_mul(0x9E37), good as u32) % 11) as i32 - 5;
    let pct = 100 + aff * 12 + own + drift;
    (BASE[good] * pct as u32 / 100).max(1)
}

#[test]
fn a_secure_system_has_exactly_the_old_prices() {
    let mut n = 0u64;
    for s in systems() {
        // Every system priced as if it were SECURE (the same system,
        // danger 0) gives the old rule's price, to the credit.
        let mut calm = s;
        calm.danger = 0;
        for day in days() {
            for g in 0..GOODS {
                assert_eq!(
                    market::price(&calm, g, day),
                    old_price(&s, g, day),
                    "{} good {g} day {day}",
                    s.name().as_str()
                );
                n += 1;
            }
        }
    }
    println!("{n} SECURE prices equal the old rule (RISK_PAY {RISK_PAY})");
}

#[test]
fn danger_only_widens_the_gap_between_cheap_and_dear() {
    let (mut cheaper, mut dearer, mut same) = (0u64, 0u64, 0u64);
    for s in systems() {
        for day in days() {
            for (g, &want) in AFF[s.econ as usize].iter().enumerate() {
                let at = |d: u8| {
                    let mut t = s;
                    t.danger = d;
                    market::price(&t, g, day)
                };
                let w = want.signum();
                assert_eq!(w < 0, made_here(s.econ, g));
                for d in 1..=3u8 {
                    let (before, now) = (at(d - 1), at(d));
                    match w {
                        -1 => {
                            assert!(now <= before, "made goods only get cheaper");
                            cheaper += u64::from(now < before);
                        }
                        1 => {
                            assert!(now >= before, "wanted goods only get dearer");
                            dearer += u64::from(now > before);
                        }
                        _ => {
                            assert_eq!(now, before, "goods nobody makes or wants keep their price");
                            same += 1;
                        }
                    }
                }
            }
        }
    }
    println!("per danger step: {cheaper} prices fell, {dearer} rose, {same} stayed (no want)");
    assert!(RISK_PAY == 0 || (cheaper > 0 && dearer > 0));
}

/// A station never pays back what it sold for, at any danger (also a
/// danger past 3, which only a hand-made System can have), on any day;
/// no price is 0 and none comes near the overflow limits.
#[test]
fn the_same_station_on_the_same_day_always_loses() {
    let (mut lo, mut hi, mut n) = (u32::MAX, 0, 0u64);
    for s in systems() {
        for d in [0u8, 1, 2, 3, 4, 200, 255] {
            let mut t = s;
            t.danger = d;
            for day in days() {
                for g in 0..GOODS {
                    let buy = market::price(&t, g, day);
                    let sell = market::sell_price(&t, g, day);
                    assert!(buy >= 2 && sell >= 1, "{buy}/{sell}");
                    assert!(sell < buy, "danger {d} good {g} day {day}: {sell} >= {buy}");
                    // A pure function: the same answer every time.
                    assert_eq!(buy, market::price(&t, g, day));
                    lo = lo.min(buy);
                    hi = hi.max(buy);
                    n += 1;
                }
            }
        }
    }
    println!("{n} prices: {lo}..={hi} credits (sell-back always less)");
    assert!(hi < 1 << 9, "prices stay below 2^9 (sell_price's sum)");
    // Danger above 3 counts as LAWLESS.
    let s = galaxy::system(seed_of(1), 0);
    let (mut a, mut b) = (s, s);
    a.danger = 3;
    b.danger = 255;
    for g in 0..GOODS {
        assert_eq!(market::price(&a, g, 9), market::price(&b, g, 9));
    }
}

/// The old rule's buy-back price (94 %).
fn old_sell(s: &System, good: usize, day: u16) -> u32 {
    (old_price(s, good, day) * 94 / 100).max(1)
}

/// Neighbours (within a full tank) and the same good carried back and
/// forth: A to B on one day, B to A the next. Day-to-day drift lets a
/// few such pairs pay both ways already under the old rule; the new
/// rule must not make that a real money loop: such pairs stay rare,
/// and their weaker leg stays far below what a real route pays.
#[test]
fn no_good_pays_well_both_ways_between_neighbours() {
    // (pairs paying both ways, best weaker leg, best one-way profit)
    let mut old = (0u64, 0i64, 0i64);
    let mut new = (0u64, 0i64, 0i64);
    let mut pairs = 0u64;
    for g in 1..=GALAXIES {
        let seed = seed_of(g);
        let sys: Vec<System> = (0..SYSTEMS as u8)
            .map(|i| galaxy::system(seed, i))
            .collect();
        for a in &sys {
            for b in &sys {
                if a.id == b.id || a.dist(b) > 65 {
                    continue;
                }
                pairs += 1;
                for day in 0..60u16 {
                    for good in 0..GOODS {
                        let p = |s: &System, d: u16| i64::from(market::price(s, good, d));
                        let q = |s: &System, d: u16| i64::from(market::sell_price(s, good, d));
                        let po = |s: &System, d: u16| i64::from(old_price(s, good, d));
                        let qo = |s: &System, d: u16| i64::from(old_sell(s, good, d));
                        for (t, there, back) in [
                            (
                                &mut new,
                                q(b, day + 1) - p(a, day),
                                q(a, day + 2) - p(b, day + 1),
                            ),
                            (
                                &mut old,
                                qo(b, day + 1) - po(a, day),
                                qo(a, day + 2) - po(b, day + 1),
                            ),
                        ] {
                            t.2 = t.2.max(there);
                            if there > 0 && back > 0 {
                                t.0 += 1;
                                t.1 = t.1.max(there.min(back));
                            }
                        }
                    }
                }
            }
        }
    }
    let cases = pairs * 60 * GOODS as u64;
    println!("{pairs} neighbour pairs x 60 days x 8 goods = {cases} cases");
    println!(
        "old rule: {} pay both ways, weaker leg at most {} a tonne; best one way {} a tonne",
        old.0, old.1, old.2
    );
    println!(
        "new rule: {} pay both ways, weaker leg at most {} a tonne; best one way {} a tonne",
        new.0, new.1, new.2
    );
    assert!(
        new.0 * 10_000 < cases,
        "pairs paying both ways must stay rare"
    );
    assert!(
        new.1 * 5 < new.2,
        "a back-and-forth must pay far less than a real route"
    );
}
