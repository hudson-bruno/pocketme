//! The checker's own tests of the danger rule in the prices
//! (`market::RISK_PAY`), written apart from `tests/market.rs`:
//!
//! - every price, worked again in wide plain numbers, against the game's
//!   wrapping sums (no price 0, buy-back below price, SECURE = old rule,
//!   no sum anywhere near its type's limit);
//! - money loops and the best profit per jump between neighbours, old
//!   rule against new, with the starting hold and the big one;
//! - a market-only greedy trader (no fights, no deaths) per galaxy, old
//!   rule against new, safe-only against all systems;
//! - per economy pair, the rule's average profit a tonne at danger 0/0
//!   and 3/3.
//!
//! `RISK_ALL_DAYS=1` checks every one of the 65,536 days (use --release).

use mercator::app::{GALAXIES, seed_of};
use mercator::fixed::hash;
use mercator::galaxy::{self, SYSTEMS, System};
use mercator::market::{self, BASE, GOODS, RISK_PAY};
use mercator::world::{FUEL_PRICE, TANK, WIN_CREDITS, survey_fee};

/// The economies' wants, typed again from the README / old code.
const AFF: [[i64; GOODS]; 5] = [
    [-3, 1, 1, 1, 2, 1, 2, 0],
    [3, -3, -3, 0, 2, 2, 1, 1],
    [2, 2, 3, -3, -2, 1, 0, 2],
    [1, 1, 0, 2, 1, -3, -3, 1],
    [2, 1, -1, 1, 1, 2, 2, -3],
];

fn own(s: &System, g: usize) -> i64 {
    i64::from(hash(s.h, g as u32) % 13) - 6
}

fn drift(s: &System, g: usize, day: u16) -> i64 {
    i64::from(hash(s.h ^ u32::from(day).wrapping_mul(0x9E37), g as u32) % 11) - 5
}

/// The rule before the change: 100 % + 12 % per want step + own + drift,
/// at least 1 credit.
fn old_price(s: &System, g: usize, day: u16) -> u32 {
    let pct = 100 + AFF[s.econ as usize][g] * 12 + own(s, g) + drift(s, g, day);
    (i64::from(BASE[g]) * pct / 100).max(1) as u32
}

fn old_sell(s: &System, g: usize, day: u16) -> u32 {
    (old_price(s, g, day) * 94 / 100).max(1)
}

/// The rule after the change, in wide numbers: the step per want grows
/// by RISK_PAY per danger step (danger capped at 3), at least 2.
fn wide_pct(aff: i64, danger: u8, own: i64, drift: i64) -> i64 {
    let d = i64::from(danger.min(3));
    100 + aff * (12 + i64::from(RISK_PAY) * d) + own + drift
}

fn systems() -> Vec<System> {
    (1..=GALAXIES)
        .flat_map(|g| (0..SYSTEMS as u8).map(move |i| galaxy::system(seed_of(g), i)))
        .collect()
}

fn days() -> Vec<u16> {
    if std::env::var("RISK_ALL_DAYS").is_ok() {
        return (0..=u16::MAX).collect();
    }
    let mut v: Vec<u16> = (0..=400).collect();
    v.extend((401..=u16::MAX).step_by(251));
    v.extend(u16::MAX - 4..=u16::MAX);
    v
}

/// Every combination the rule can meet (good, economy, danger 0..=3 and
/// past it, own -6..=6, drift -5..=5), in i64: pct stays above 0, the
/// product far below 2^31, the price at least 2, the buy-back below it.
#[test]
fn every_combination_of_the_rule_stays_in_range() {
    let (mut pmin, mut pmax, mut prod_max) = (i64::MAX, i64::MIN, 0i64);
    for (g, &base) in BASE.iter().enumerate() {
        for e in &AFF {
            for d in [0u8, 1, 2, 3, 4, 255] {
                for o in -6..=6 {
                    for r in -5..=5 {
                        let pct = wide_pct(e[g], d, o, r);
                        assert!(pct > 0, "pct {pct}");
                        let prod = i64::from(base) * pct;
                        assert!(prod < 1 << 15);
                        let price = (prod / 100).max(2);
                        let sell = (price * 94 / 100).max(1);
                        assert!(sell < price);
                        pmin = pmin.min(pct);
                        pmax = pmax.max(pct);
                        prod_max = prod_max.max(prod);
                    }
                }
            }
        }
    }
    println!(
        "RISK_PAY {RISK_PAY}: pct {pmin}..={pmax}, largest base x pct {prod_max} (limit 2^15 = 32768, u32 2^32)"
    );
}

/// The game's price, for every system of the 10 galaxies, every good,
/// danger as made and forced to 0..=3, on many days (all with
/// RISK_ALL_DAYS): equal to the wide rule; at danger 0 equal to the old
/// rule; never 0; buy-back always below.
#[test]
fn the_game_price_equals_the_wide_rule_everywhere() {
    let days = days();
    let (mut n, mut lo, mut hi, mut at2) = (0u64, u32::MAX, 0u32, 0u64);
    for s in systems() {
        for d in [s.danger, 0, 1, 2, 3] {
            let mut t = s;
            t.danger = d;
            for &day in &days {
                for g in 0..GOODS {
                    let p = market::price(&t, g, day);
                    let pct = wide_pct(AFF[s.econ as usize][g], d, own(&s, g), drift(&s, g, day));
                    let want = (i64::from(BASE[g]) * pct / 100).max(2) as u32;
                    assert_eq!(
                        p, want,
                        "galaxy system {} good {g} danger {d} day {day}",
                        s.id
                    );
                    if d == 0 {
                        assert_eq!(p, old_price(&s, g, day), "SECURE must be the old price");
                    }
                    let b = market::sell_price(&t, g, day);
                    assert!(p >= 2 && b >= 1 && b < p, "{p}/{b}");
                    lo = lo.min(p);
                    hi = hi.max(p);
                    at2 += u64::from(p == 2);
                    n += 1;
                }
            }
        }
    }
    println!(
        "{n} prices over {} days: {lo}..={hi} credits, {at2} held up by the floor of 2",
        days.len()
    );
}

/// Ordered pairs of different systems within one full tank, per galaxy.
fn neighbours(seed: u32) -> (Vec<System>, Vec<Vec<usize>>) {
    let sys: Vec<System> = (0..SYSTEMS as u8)
        .map(|i| galaxy::system(seed, i))
        .collect();
    let near = sys
        .iter()
        .map(|a| {
            (0..sys.len())
                .filter(|&j| sys[j].id != a.id && a.dist(&sys[j]) <= u32::from(TANK))
                .collect()
        })
        .collect();
    (sys, near)
}

type Rule = (
    fn(&System, usize, u16) -> u32,
    fn(&System, usize, u16) -> u32,
);
const OLD: Rule = (old_price, old_sell);
const NEW: Rule = (market::price, market::sell_price);

/// The rule at another strength `K` (for the owner's choice; at
/// K = RISK_PAY it is the game's rule, checked above).
fn k_price<const K: i64>(s: &System, g: usize, day: u16) -> u32 {
    let d = i64::from(s.danger.min(3));
    let pct = 100 + AFF[s.econ as usize][g] * (12 + K * d) + own(s, g) + drift(s, g, day);
    (i64::from(BASE[g]) * pct / 100).max(2) as u32
}

fn k_sell<const K: i64>(s: &System, g: usize, day: u16) -> u32 {
    (k_price::<K>(s, g, day) * 94 / 100).max(1)
}

/// Best profit of one jump A (day t) -> B (day t+1) with `hold` tonnes,
/// any amount of money, the station's stock as the limit, fuel paid.
fn best_leg(rule: Rule, a: &System, b: &System, day: u16, hold: u32) -> (i64, usize) {
    let fuel = i64::from(a.dist(b) * FUEL_PRICE);
    let mut best = (-fuel, GOODS);
    for g in 0..GOODS {
        let per = i64::from(rule.1(b, g, day + 1)) - i64::from(rule.0(a, g, day));
        let n = hold.min(u32::from(market::stock(a, g, day)));
        if per > 0 && n > 0 {
            let p = per * i64::from(n) - fuel;
            if p > best.0 {
                best = (p, g);
            }
        }
    }
    best
}

#[test]
fn money_loops_and_best_jumps_old_against_new() {
    const DAYS: u16 = 60;
    for (label, rule) in [("old", OLD), ("new", NEW)] {
        let (mut pairs, mut cases, mut both, mut both_worst) = (0u64, 0u64, 0u64, 0i64);
        let mut route_best = 0i64;
        // Best profit a jump: [hold 24, hold 40] x [SECURE-SECURE pairs, any pair].
        let mut best = [[0i64; 2]; 2];
        let mut sum = [[0i64; 2]; 2];
        let mut cnt = [0i64; 2];
        let mut by_danger = [[0i64; 4]; 4];
        let mut by_danger_n = [[0i64; 4]; 4];
        for gx in 1..=GALAXIES {
            let (sys, near) = neighbours(seed_of(gx));
            for (i, list) in near.iter().enumerate() {
                for &j in list {
                    let (a, b) = (&sys[i], &sys[j]);
                    pairs += 1;
                    for day in 0..DAYS {
                        for g in 0..GOODS {
                            let ab =
                                i64::from(rule.1(b, g, day + 1)) - i64::from(rule.0(a, g, day));
                            let ba =
                                i64::from(rule.1(a, g, day + 2)) - i64::from(rule.0(b, g, day + 1));
                            route_best = route_best.max(ab);
                            cases += 1;
                            if ab > 0 && ba > 0 {
                                both += 1;
                                both_worst = both_worst.max(ab.min(ba));
                            }
                        }
                        let safe = usize::from(a.danger == 0 && b.danger == 0);
                        for (h, hold) in [24u32, 40].into_iter().enumerate() {
                            let (p, _) = best_leg(rule, a, b, day, hold);
                            for k in 0..=safe {
                                let k = 1 - k; // 1 = any pair, 0 = SECURE pair
                                best[h][k] = best[h][k].max(p);
                                sum[h][k] += p.max(0);
                            }
                        }
                        cnt[1] += 1;
                        cnt[0] += safe as i64;
                        let (p, _) = best_leg(rule, a, b, day, 40);
                        by_danger[usize::from(a.danger)][usize::from(b.danger)] += p.max(0);
                        by_danger_n[usize::from(a.danger)][usize::from(b.danger)] += 1;
                    }
                }
            }
        }
        println!(
            "{label}: {pairs} ordered neighbour pairs, {cases} same-good back-and-forth cases, \
             {both} pay both ways (weaker leg at most {both_worst} cr/t); best single route {route_best} cr/t"
        );
        for (h, hold) in [24, 40].into_iter().enumerate() {
            println!(
                "{label}: hold {hold}: best jump SECURE->SECURE {} cr (mean of the best per pair-day {}), any pair {} cr (mean {})",
                best[h][0],
                sum[h][0] / cnt[0].max(1),
                best[h][1],
                sum[h][1] / cnt[1].max(1)
            );
        }
        println!("{label}: hold 40, mean best profit a jump by danger (rows: from, cols: to):");
        for (f, row) in by_danger.iter().enumerate() {
            let cells: Vec<String> = row
                .iter()
                .zip(by_danger_n[f])
                .map(|(s, n)| format!("{:>6}", s / n.max(1)))
                .collect();
            println!("  {:>8} {}", galaxy::DANGER_NAMES[f], cells.join(" "));
        }
        assert!(
            both * 10_000 < cases,
            "{label}: same-good loops must stay rare"
        );
        assert!(
            both_worst * 5 < route_best,
            "{label}: and pay far less than a route"
        );
    }
}

/// A trader on the markets alone (no fights, no deaths, no flying time):
/// from the start system, each jump goes to the neighbour within a full
/// tank whose best single good (limited by money, hold and stock) pays
/// most after fuel, among systems of danger <= `max_danger`. It buys the
/// 40 t bay at the first station that sells it once it has 2,000 credits.
/// Jumps until 50,000 credits (cap 400), and surveys by then.
fn greedy(rule: Rule, seed: u32, max_danger: u8) -> (u32, u32) {
    let (sys, near) = neighbours(seed);
    let start = galaxy::start_system(seed) as usize;
    let mut at = start;
    let mut credits: i64 = 300;
    let mut day = 0u16;
    let mut hold = 24u32;
    let mut seen = vec![false; sys.len()];
    seen[at] = true;
    for jumps in 0..400 {
        if credits >= i64::from(WIN_CREDITS) {
            return (jumps, seen.iter().filter(|&&s| s).count() as u32);
        }
        let a = &sys[at];
        if hold == 24 && credits > 2_000 && a.tech >= 3 {
            hold = 40;
            credits -= 900;
        }
        let mut pick = (i64::MIN, at, GOODS, 0u32);
        for &j in &near[at] {
            let b = &sys[j];
            if b.danger > max_danger {
                continue;
            }
            let fuel = i64::from(a.dist(b) * FUEL_PRICE);
            let fee = if seen[j] { 0 } else { i64::from(survey_fee(b)) };
            let mut leg = (-fuel + fee, GOODS, 0u32);
            for g in 0..GOODS {
                let buy = i64::from(rule.0(a, g, day));
                let per = i64::from(rule.1(b, g, day + 1)) - buy;
                let n = hold
                    .min(u32::from(market::stock(a, g, day)))
                    .min((credits / buy).clamp(0, 255) as u32);
                if per > 0 && n > 0 && per * i64::from(n) - fuel + fee > leg.0 {
                    leg = (per * i64::from(n) - fuel + fee, g, n);
                }
            }
            if leg.0 > pick.0 {
                pick = (leg.0, j, leg.1, leg.2);
            }
        }
        let (gain, to, _, _) = pick;
        if to == at {
            return (400, 0);
        }
        credits += gain;
        day += 1;
        at = to;
        seen[at] = true;
    }
    (400, seen.iter().filter(|&&s| s).count() as u32)
}

#[test]
fn a_market_only_trader_old_against_new() {
    let rules: [(&str, Rule); 6] = [
        ("old (0)", OLD),
        ("k=2", (k_price::<2>, k_sell::<2>)),
        ("k=3", (k_price::<3>, k_sell::<3>)),
        ("new (RISK_PAY)", NEW),
        ("k=5", (k_price::<5>, k_sell::<5>)),
        ("k=6", (k_price::<6>, k_sell::<6>)),
    ];
    for (label, rule) in rules {
        let mut line = Vec::new();
        let mut surveys = Vec::new();
        let mut mean = [0f64; 3];
        for gx in 1..=GALAXIES {
            let seed = seed_of(gx);
            let r = [
                greedy(rule, seed, 0),
                greedy(rule, seed, 1),
                greedy(rule, seed, 3),
            ];
            for k in 0..3 {
                mean[k] += f64::from(r[k].0) / f64::from(GALAXIES);
            }
            line.push(format!("{}/{}/{}", r[0].0, r[1].0, r[2].0));
            surveys.push(format!("{}/{}/{}", r[0].1, r[1].1, r[2].1));
        }
        println!(
            "{label} rule, jumps to 50,000 (SECURE only / SECURE+QUIET / all), market only: {}",
            line.join("  ")
        );
        println!(
            "{label} rule means: SECURE only {:.1}, SECURE+QUIET {:.1}, all {:.1}",
            mean[0], mean[1], mean[2]
        );
        println!(
            "{label} rule, systems surveyed by then: {}",
            surveys.join("  ")
        );
    }
}

/// The rule's average profit a tonne (own and drift spread evenly) for
/// the best good of each economy pair, at danger 0/0 and 3/3.
#[test]
fn economy_pairs_at_danger_0_and_3() {
    let avg_price = |aff: i64, d: u8, g: usize| -> f64 {
        let mut s = 0f64;
        for o in -6..=6 {
            for r in -5..=5 {
                s += ((i64::from(BASE[g]) * wide_pct(aff, d, o, r) / 100).max(2)) as f64;
            }
        }
        s / 143.0
    };
    let avg_sell = |aff: i64, d: u8, g: usize| -> f64 {
        let mut s = 0f64;
        for o in -6..=6 {
            for r in -5..=5 {
                let p = (i64::from(BASE[g]) * wide_pct(aff, d, o, r) / 100).max(2);
                s += ((p * 94 / 100).max(1)) as f64;
            }
        }
        s / 143.0
    };
    let names = ["FARM", "MINE", "FORGE", "LAB", "HAVEN"];
    println!("from -> to: best good, cr/t at 0/0, 3/3, gain");
    for (ea, wa) in AFF.iter().enumerate() {
        for (eb, wb) in AFF.iter().enumerate() {
            if ea == eb {
                continue;
            }
            let mut best = (0f64, 0f64, GOODS);
            for g in 0..GOODS {
                let p0 = avg_sell(wb[g], 0, g) - avg_price(wa[g], 0, g);
                let p3 = avg_sell(wb[g], 3, g) - avg_price(wa[g], 3, g);
                if p0 > best.0 {
                    best = (p0, p3, g);
                }
            }
            if best.2 < GOODS {
                println!(
                    "  {:>5} -> {:<5}: {:<6} {:6.1} {:6.1}  x{:.2}",
                    names[ea],
                    names[eb],
                    market::NAMES[best.2],
                    best.0,
                    best.1,
                    best.1 / best.0
                );
            }
        }
    }
}
