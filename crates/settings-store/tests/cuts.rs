//! Power cuts. The pretend flash stops after any byte of a write or any
//! word of an erase. For long random sequences of saves, EVERY cut
//! point of every save is tried (not a sample): after the cut and a
//! restart, the store gives the old value or the new one, never
//! anything else, and the next save works (it never gets stuck).
//!
//! More saves: `CUT_SAVES=3000 cargo test --release --test cuts`.

mod common;

use common::{BASE, CutFlash, Rng};
use settings_store::{SLOTS, Saved, Settings, SettingsStore};

fn saves() -> usize {
    std::env::var("CUT_SAVES")
        .ok()
        .and_then(|v| v.parse().ok())
        // Enough to fill both sectors and erase each one once from
        // blank: 2 x 256 + 2 x 256 / 2.
        .unwrap_or(2 * SLOTS as usize + SLOTS as usize + 40)
}

/// A random step different from `not`.
fn other_step(rng: &mut Rng, not: u8) -> u8 {
    (not + 1 + rng.below(4) as u8) % 5
}

/// The store, freshly started on `f` (as after a restart).
fn restart(f: &mut CutFlash) -> (SettingsStore, Settings) {
    f.power_on();
    let mut s = SettingsStore::new(BASE);
    let got = s.load(f);
    (s, got)
}

/// After a cut while `new` was saved over `old`: a restart gives one of
/// the two, and two more saves work and read back.
fn check_after_cut(f: &mut CutFlash, old: Settings, new: Settings, what: &str) {
    let outside: Vec<u8> = [
        &f.bytes[..BASE as usize],
        &f.bytes[(BASE + settings_store::AREA) as usize..],
    ]
    .concat();
    let (mut s, got) = restart(f);
    assert!(
        got == old || got == new,
        "{what}: got {got:?}, old {old:?}, new {new:?}"
    );
    for k in 1..=2 {
        let next = got.with_brightness((got.brightness + k) % 5);
        let r = s.save(f, &next);
        assert!(
            matches!(r, Ok(Saved::Written { .. })),
            "{what}: stuck: {r:?}"
        );
        assert_eq!(restart(f).1, next, "{what}: save {k} after the cut");
    }
    let now: Vec<u8> = [
        &f.bytes[..BASE as usize],
        &f.bytes[(BASE + settings_store::AREA) as usize..],
    ]
    .concat();
    assert!(outside == now, "{what}: bytes outside the area changed");
}

/// Runs `n` random saves on `image` without cuts, keeping the flash
/// before each save; then cuts every save at every unit it uses.
/// Returns (cut points tried, erases seen in the uncut run).
fn every_cut(image: Vec<u8>, seed: u64, n: usize, partial: bool) -> (u64, u32) {
    let mut rng = Rng::new(seed);
    let mut f = CutFlash::from(image);
    let mut s = SettingsStore::new(BASE);
    let mut cur = s.load(&mut f);
    // (flash before, old, new, units used)
    let mut steps: Vec<(Vec<u8>, Settings, Settings, u64)> = Vec::with_capacity(n);
    for i in 0..n {
        let new = cur.with_brightness(other_step(&mut rng, cur.brightness));
        let before = f.bytes.clone();
        let u0 = f.units;
        let r = s.save(&mut f, &new).unwrap();
        assert!(matches!(r, Saved::Written { .. }), "save {i}: {r:?}");
        steps.push((before, cur, new, f.units - u0));
        cur = new;
        // What the running store believes is what a restart reads.
        let mut fresh = SettingsStore::new(BASE);
        assert_eq!(fresh.load(&mut f), new, "save {i}");
        assert_eq!(
            fresh, s,
            "save {i}: the running store drifted from the flash"
        );
    }
    let erases = f.erases.iter().sum();
    let mut tried = 0;
    for (i, (before, old, new, units)) in steps.into_iter().enumerate() {
        for cut in 0..units {
            let mut f = CutFlash::from(before.clone());
            if partial {
                f.partial = Some(Rng::new(seed ^ (i as u64) << 20 ^ cut));
            }
            let mut s = SettingsStore::new(BASE);
            assert_eq!(s.load(&mut f), old);
            f.budget = Some(cut);
            assert!(
                s.save(&mut f, &new).is_err(),
                "save {i} cut at {cut} did not fail"
            );
            check_after_cut(
                &mut f,
                old,
                new,
                &format!("seed {seed} save {i} cut at {cut}"),
            );
            tried += 1;
        }
    }
    (tried, erases)
}

#[test]
fn every_cut_point_from_a_blank_area() {
    let n = saves();
    let blank = vec![0xFF; common::STORE_SIZE as usize];
    let (tried, erases) = every_cut(blank.clone(), 1, n, false);
    println!("{n} saves from blank: {tried} cut points, {erases} erases");
    assert!(erases >= 2, "the sequence must erase both sectors once");
    let (tried, _) = every_cut(blank, 2, n, true);
    println!("the same with half-done bits at the cut: {tried} cut points");
}

#[test]
fn every_cut_point_from_a_damaged_area() {
    // Random bytes in the whole store (as if something else used it).
    let mut rng = Rng::new(7);
    let junk: Vec<u8> = (0..common::STORE_SIZE).map(|_| rng.next() as u8).collect();
    let (tried, erases) = every_cut(junk, 3, SLOTS as usize + 20, true);
    println!("from random bytes: {tried} cut points, {erases} erases");
    assert!(erases >= 1);
}

/// Many cuts in a row, some during the save after a cut, some in
/// `prepare`; random values, random cut points, half-done bits.
#[test]
fn random_cuts_in_a_long_sequence() {
    for seed in 0..6u64 {
        let mut rng = Rng::new(100 + seed);
        let mut f = CutFlash::blank();
        let (mut s, mut stored) = restart(&mut f);
        let mut writes = 0u64;
        for op in 0..4000 {
            let what = format!("seed {seed} op {op}");
            let cut = rng.chance(250);
            if cut {
                // Somewhere in the next operation (it may also finish).
                let far = if rng.chance(100) { 1100 } else { 20 };
                f.budget = Some(rng.below(far));
                f.partial = rng.chance(500).then(|| Rng::new(rng.next()));
            }
            if rng.chance(30) {
                let r = s.prepare(&mut f);
                if r.is_err() {
                    assert!(cut, "{what}: prepare failed with no cut: {r:?}");
                    let (s2, got) = restart(&mut f);
                    assert_eq!(got, stored, "{what}: a cut prepare lost the value");
                    s = s2;
                }
                f.power_on();
                continue;
            }
            let new = stored.with_brightness(other_step(&mut rng, stored.brightness));
            match s.save(&mut f, &new) {
                Ok(Saved::Written { .. }) => {
                    writes += 1;
                    stored = new;
                    f.power_on();
                }
                Ok(Saved::Unchanged) => panic!("{what}: a change not written"),
                Err(e) => {
                    assert!(cut, "{what}: failed with no cut: {e:?}");
                    let (s2, got) = restart(&mut f);
                    assert!(got == stored || got == new, "{what}: got {got:?}");
                    s = s2;
                    stored = got;
                }
            }
        }
        let erases: u32 = f.erases.iter().sum();
        println!("seed {seed}: {writes} saves, {erases} erases");
        assert_eq!(restart(&mut f).1, stored);
    }
}

/// A cut in the erase of `prepare`, at every word: the value stays.
#[test]
fn every_cut_point_of_prepare() {
    let mut f = CutFlash::blank();
    let mut s = SettingsStore::new(BASE);
    s.load(&mut f);
    // Fill sector 0, then half of sector 1: sector 0 is the spare.
    let mut cur = Settings::DEFAULT;
    for i in 0..(SLOTS as usize + SLOTS as usize / 2) {
        cur = cur.with_brightness((i % 4) as u8);
        s.save(&mut f, &cur).unwrap();
    }
    let image = f.bytes.clone();
    let mut t = CutFlash::from(image.clone());
    let mut s = SettingsStore::new(BASE);
    s.load(&mut t);
    let u0 = t.units;
    assert_eq!(s.prepare(&mut t), Ok(true));
    let units = t.units - u0;
    assert_eq!(units, 1024, "one 4 KB sector, word by word");
    assert_eq!(s.prepare(&mut t), Ok(false), "the spare is blank now");
    for cut in 0..units {
        let mut t = CutFlash::from(image.clone());
        t.partial = Some(Rng::new(cut));
        let mut s = SettingsStore::new(BASE);
        s.load(&mut t);
        t.budget = Some(cut);
        assert!(s.prepare(&mut t).is_err());
        check_after_cut(&mut t, cur, cur, &format!("prepare cut at {cut}"));
    }
}
