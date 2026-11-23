//! The galaxy: the same from the same seed, sane for any seed, and
//! every system reachable under the fuel rules (or the fraction said).

use mercator::app::{GALAXIES, seed_of};
use mercator::fixed::hash;
use mercator::galaxy::{self, SYSTEMS, System};
use mercator::market::{self, GOODS};
use mercator::world::TANK;

fn all(seed: u32) -> Vec<System> {
    (0..SYSTEMS)
        .map(|i| galaxy::system(seed, i as u8))
        .collect()
}

#[test]
fn the_same_seed_makes_the_same_galaxy() {
    for n in 1..=GALAXIES {
        let seed = seed_of(n);
        let a = all(seed);
        let b = all(seed);
        assert_eq!(a, b);
        let names: Vec<String> = a.iter().map(|s| s.name().as_str().to_string()).collect();
        let again: Vec<String> = b.iter().map(|s| s.name().as_str().to_string()).collect();
        assert_eq!(names, again);
    }
    // And another seed makes another galaxy.
    assert_ne!(all(seed_of(1)), all(seed_of(2)));
}

/// Known values: if these change, every saved career's galaxy changes.
#[test]
fn galaxy_one_is_pinned() {
    let seed = seed_of(1);
    let start = galaxy::start_system(seed);
    let s = galaxy::system(seed, start);
    println!(
        "galaxy 1 starts at {} ({}), {:?}, danger {}, tech {}",
        start,
        s.name().as_str(),
        s.econ,
        s.danger,
        s.tech
    );
    assert_eq!((start, s.name().as_str()), (10, "JAKEL"));
    assert_eq!(s.danger, 0, "a new career starts in a secure system");
    assert_ne!(s.econ, galaxy::Econ::Haven);
}

#[test]
fn a_thousand_seeds_are_sane() {
    let mut names = std::collections::HashSet::new();
    let mut lens = [0u32; 12];
    let mut econ = [0u32; 5];
    let mut danger = [0u32; 4];
    let mut min_price = u32::MAX;
    let mut max_price = 0;
    let mut same_place = 0u32;
    let mut twins = 0usize;
    for k in 0..1000u32 {
        let seed = hash(k, 0x5EED);
        let sys = all(seed);
        let mut places = std::collections::HashSet::new();
        for s in &sys {
            let name = s.name();
            let n = name.as_str();
            assert!(!n.is_empty(), "seed {seed:#x} system {} has no name", s.id);
            assert!(n.len() <= 9, "{n}: too long for the screens");
            assert!(
                n.bytes()
                    .all(|b| b.is_ascii_uppercase() || b == b' ' || b == b'-'),
                "{n}: not printable with the console's font"
            );
            lens[n.len()] += 1;
            if k < 20 {
                names.insert(n.to_string());
            }
            assert!(u32::from(s.x) < galaxy::WIDTH && u32::from(s.y) < galaxy::HEIGHT);
            assert!(s.danger <= 3 && s.tech >= 1 && s.tech <= 12, "{s:?}");
            econ[s.econ as usize] += 1;
            danger[usize::from(s.danger)] += 1;
            if !places.insert((s.x, s.y)) {
                same_place += 1;
            }
            for g in 0..GOODS {
                for day in [0u16, 1, 77, 65_535] {
                    let p = market::price(s, g, day);
                    let q = market::sell_price(s, g, day);
                    assert!(p > 0 && q > 0, "{n} good {g}: price {p}/{q}");
                    assert!(q <= p, "selling back must never make money");
                    min_price = min_price.min(p);
                    max_price = max_price.max(p);
                }
            }
        }
        let start = galaxy::start_system(seed);
        assert!(usize::from(start) < SYSTEMS);
        let unique: std::collections::HashSet<String> =
            sys.iter().map(|s| s.name().as_str().to_string()).collect();
        if unique.len() < SYSTEMS {
            twins += SYSTEMS - unique.len();
        }
    }
    println!("name lengths: {lens:?}");
    println!("economies: {econ:?}, danger: {danger:?}");
    println!("prices {min_price}..={max_price}; systems sharing a place: {same_place}");
    println!(
        "different names in 20 galaxies: {} of {}; same-name systems in 1000 galaxies: {twins}",
        names.len(),
        20 * SYSTEMS
    );
    assert!(twins <= 10, "two systems of one galaxy share a name");
    assert_eq!(same_place, 0, "two systems on one spot");
    assert!(names.len() > 20 * SYSTEMS / 2, "names repeat too often");
    assert!(
        econ.iter().all(|&e| e > 1000 * SYSTEMS as u32 / 10),
        "an economy is rare"
    );
}

/// Which systems a career can reach from the start: jumps of at most a
/// full tank (fuel is sold at every station).
fn reachable(seed: u32) -> usize {
    let sys = all(seed);
    let start = usize::from(galaxy::start_system(seed));
    let mut seen = [false; SYSTEMS];
    let mut todo = vec![start];
    seen[start] = true;
    while let Some(i) = todo.pop() {
        for j in 0..SYSTEMS {
            if !seen[j] && sys[i].dist(&sys[j]) <= u32::from(TANK) {
                seen[j] = true;
                todo.push(j);
            }
        }
    }
    seen.iter().filter(|&&s| s).count()
}

#[test]
fn every_system_is_reachable_with_a_full_tank() {
    let mut worst = SYSTEMS;
    let mut sum = 0usize;
    let mut full = 0;
    for k in 0..1000u32 {
        let seed = hash(k, 0x5EED);
        let n = reachable(seed);
        worst = worst.min(n);
        sum += n;
        full += usize::from(n == SYSTEMS);
    }
    for n in 1..=GALAXIES {
        assert_eq!(reachable(seed_of(n)), SYSTEMS, "galaxy {n} must be whole");
    }
    println!(
        "1000 seeds: {full} wholly reachable; worst {worst} of {SYSTEMS}; reachable in all {:.4} %",
        sum as f64 * 100.0 / (1000.0 * SYSTEMS as f64)
    );
    assert_eq!(full, 1000, "a system out of reach");
}

#[test]
fn neighbours_are_near() {
    // Every system has at least two others within a full tank.
    for n in 1..=GALAXIES {
        let sys = all(seed_of(n));
        for a in &sys {
            let near = sys
                .iter()
                .filter(|b| b.id != a.id && a.dist(b) <= u32::from(TANK))
                .count();
            assert!(near >= 2, "galaxy {n}: {} has {near} neighbours", a.id);
        }
    }
}
