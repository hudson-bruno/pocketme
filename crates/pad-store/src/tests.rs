//! Tests on the PC, against the fake flash. Every address and key here
//! is made up.

// `drop(s)` ends a borrow of the fake flash on purpose.
#![allow(dropping_references, clippy::drop_non_drop)]

use crate::fake::{Fake, FakeError, Op};
use crate::region::{Region, RegionError, Span, check_region};
use crate::store::{AREA, Bounded, COMPACT_BELOW, Error, MAX_PADS, Pairing, SLOTS, Saved, Store};

const NVS: Span = Span {
    offset: 0x9000,
    len: 0x6000,
};
const PHY: Span = Span {
    offset: 0xF000,
    len: 0x1000,
};
const FACTORY: Span = Span {
    offset: 0x1_0000,
    len: 0x3F_0000,
};
const CAPACITY: u32 = 0x40_0000;

fn region() -> Region {
    check_region(NVS, &[PHY, FACTORY], CAPACITY).unwrap()
}

/// Pad number `n` with key variant `v` (both made up).
fn pad(n: u8, v: u8) -> Pairing {
    Pairing {
        addr: [0x10, 0x20, 0x30, 0x40, 0x50, n],
        key: core::array::from_fn(|i| {
            (i as u8).wrapping_mul(17) ^ n.wrapping_mul(29) ^ v.wrapping_mul(101) ^ 0x5C
        }),
        key_type: 0x04,
    }
}

fn open(f: &mut Fake) -> Store<&mut Fake> {
    Store::open(f, region()).expect("open").0
}

fn all(s: &mut Store<&mut Fake>) -> Vec<Pairing> {
    let mut v = Vec::new();
    s.load(|p| v.push(*p)).unwrap();
    v
}

fn reopen_all(f: &mut Fake) -> Vec<Pairing> {
    let mut s = open(f);
    all(&mut s)
}

/// Nothing outside the store's two sectors changed, and every write and
/// erase stayed inside them.
fn outside_untouched(f: &Fake) {
    let base = region().base() as usize;
    let end = base + region().len() as usize;
    assert!(f.mem[..base].iter().all(|&b| b == 0xA5), "below the store");
    assert!(
        f.mem[end..0x1_0000].iter().all(|&b| b == 0xFF),
        "above the store"
    );
    for &(_, at, len) in &f.ops {
        assert!(
            at as usize >= base && (at + len) as usize <= end,
            "an operation outside the store: {at:#x} +{len:#x}"
        );
    }
}

/// How many valid key records the flash holds for `addr` in both areas
/// (a fresh scan of the raw bytes).
fn key_records(f: &Fake, addr: [u8; 6]) -> usize {
    let base = region().base() as usize;
    let mut n = 0;
    for area in 0..2 {
        for slot in 1..=SLOTS as usize {
            let r = &f.mem[base + area * AREA as usize + slot * 32..][..32];
            if r[0] == 0x50 && r[1] == 0x4B && r[3] == 1 && r[6..12] == addr {
                let crc = crate::crc::crc32(&r[..28]).to_le_bytes();
                if r[28..32] == crc {
                    n += 1;
                }
            }
        }
    }
    n
}

// ------------------------------------------------------------- the region

#[test]
fn the_default_table_is_accepted() {
    let r = region();
    assert_eq!((r.base(), r.len()), (0x9000, 0x2000));
}

#[test]
fn bad_partitions_are_refused() {
    let others = [PHY, FACTORY];
    let try_part = |offset, len| check_region(Span { offset, len }, &others, CAPACITY);
    assert_eq!(try_part(0x9100, 0x6000), Err(RegionError::NotAligned));
    assert_eq!(try_part(0x9000, 0x1000), Err(RegionError::TooSmall));
    assert_eq!(try_part(0x8000, 0x6000), Err(RegionError::BelowTable));
    assert_eq!(try_part(0x0000, 0x6000), Err(RegionError::BelowTable));
    // Would use 0xE000..0x10000: into phy_init.
    assert_eq!(try_part(0xE000, 0x2000), Err(RegionError::Overlaps));
    // Inside the program.
    assert_eq!(try_part(0x2_0000, 0x2000), Err(RegionError::Overlaps));
    assert_eq!(try_part(0x3F_F000, 0x2000), Err(RegionError::PastEnd));
    assert_eq!(try_part(0xFFFF_F000, 0x2000), Err(RegionError::PastEnd));
    // A table entry that runs past 32 bits counts as overlapping.
    let broken = [Span {
        offset: 0xFFFF_0000,
        len: 0x2_0000,
    }];
    assert_eq!(
        check_region(NVS, &broken, CAPACITY),
        Err(RegionError::Overlaps)
    );
}

#[test]
fn writes_and_erases_outside_the_area_are_refused() {
    let mut f = Fake::new();
    let mut b = Bounded::new(&mut f, region());
    let mut d = [0u32; 1];
    // SAFETY-free trick for an aligned 4-byte buffer: a u32 viewed as bytes.
    let four: &mut [u8] = bytemuck(&mut d);
    four.fill(0);
    let end = 0x2000;
    assert!(b.write(end - 4, four).is_ok(), "last word inside");
    assert_eq!(b.write(end, four), Err(Error::OutOfBounds));
    assert_eq!(b.write(end + 4, four), Err(Error::OutOfBounds));
    assert_eq!(b.write(u32::MAX - 3, four), Err(Error::OutOfBounds));
    assert_eq!(b.write(2, four), Err(Error::OutOfBounds), "not aligned");
    assert_eq!(b.write(0, &four[..3]), Err(Error::OutOfBounds), "length");
    assert!(b.erase(end - 4096).is_ok(), "last sector inside");
    assert_eq!(b.erase(end), Err(Error::OutOfBounds));
    assert_eq!(b.erase(4096 + 256), Err(Error::OutOfBounds));
    assert_eq!(b.erase(u32::MAX - 4095), Err(Error::OutOfBounds));
    let mut r = [0u8; 8];
    assert_eq!(b.read(end - 4, &mut r), Err(Error::OutOfBounds));
    drop(b);
    // The two calls that were allowed: inside; everything else untouched.
    assert_eq!(f.ops.len(), 2);
    outside_untouched(&f);
}

fn bytemuck(d: &mut [u32; 1]) -> &mut [u8] {
    // SAFETY: a [u32; 1] is 4 bytes, any byte value is valid, and the
    // borrow keeps it alive.
    unsafe { core::slice::from_raw_parts_mut(d.as_mut_ptr().cast::<u8>(), 4) }
}

// ------------------------------------------------------------ basic work

#[test]
fn save_then_load_after_a_restart() {
    let mut f = Fake::new();
    let (mut s, rep) = Store::open(&mut f, region()).unwrap();
    assert!(rep.created);
    assert_eq!(s.count(), 0);
    assert_eq!(s.save(&pad(1, 0)).unwrap(), Saved::New { evicted: None });
    assert_eq!(s.save(&pad(2, 0)).unwrap(), Saved::New { evicted: None });
    assert_eq!(s.count(), 2);
    drop(s);
    let (mut s, rep) = Store::open(&mut f, region()).unwrap();
    assert!(!rep.created && !rep.compacted && rep.damaged == 0);
    assert_eq!(all(&mut s), vec![pad(1, 0), pad(2, 0)]);
    drop(s);
    outside_untouched(&f);
}

#[test]
fn the_newest_record_wins_and_the_old_key_is_wiped() {
    let mut f = Fake::new();
    let mut s = open(&mut f);
    s.save(&pad(1, 0)).unwrap();
    s.save(&pad(2, 0)).unwrap();
    assert_eq!(s.save(&pad(1, 1)).unwrap(), Saved::Replaced);
    drop(s);
    // Pad 1 is now the newest.
    assert_eq!(reopen_all(&mut f), vec![pad(2, 0), pad(1, 1)]);
    assert_eq!(key_records(&f, pad(1, 0).addr), 1, "the old key is gone");
}

#[test]
fn saving_the_same_pairing_again_writes_nothing() {
    let mut f = Fake::new();
    let mut s = open(&mut f);
    s.save(&pad(1, 0)).unwrap();
    drop(s);
    let before = f.writes();
    let mut s = open(&mut f);
    let after_open = s.free_slots();
    assert_eq!(s.save(&pad(1, 0)).unwrap(), Saved::Unchanged);
    assert_eq!(s.free_slots(), after_open);
    drop(s);
    // Only the start-up header rewrite.
    assert_eq!(f.writes(), before + 1);
}

#[test]
fn forget_one() {
    let mut f = Fake::new();
    let mut s = open(&mut f);
    for n in 1..=3 {
        s.save(&pad(n, 0)).unwrap();
    }
    assert!(s.forget(pad(2, 0).addr).unwrap());
    assert!(!s.forget(pad(9, 0).addr).unwrap(), "never saved");
    assert_eq!(s.count(), 2);
    drop(s);
    assert_eq!(reopen_all(&mut f), vec![pad(1, 0), pad(3, 0)]);
    assert_eq!(
        key_records(&f, pad(2, 0).addr),
        0,
        "the key bytes are wiped"
    );
}

#[test]
fn forget_all() {
    let mut f = Fake::new();
    let mut s = open(&mut f);
    for n in 1..=3 {
        s.save(&pad(n, 0)).unwrap();
    }
    assert_eq!(s.forget_all().unwrap(), 3);
    assert_eq!(s.forget_all().unwrap(), 0);
    drop(s);
    assert!(reopen_all(&mut f).is_empty());
    for n in 1..=3 {
        assert_eq!(key_records(&f, pad(n, 0).addr), 0);
    }
    // And the store goes on working.
    let mut s = open(&mut f);
    s.save(&pad(4, 0)).unwrap();
    drop(s);
    assert_eq!(reopen_all(&mut f), vec![pad(4, 0)]);
}

#[test]
fn more_pads_than_the_limit_drop_the_oldest() {
    let mut f = Fake::new();
    let mut s = open(&mut f);
    for n in 1..=MAX_PADS as u8 {
        assert_eq!(s.save(&pad(n, 0)).unwrap(), Saved::New { evicted: None });
    }
    let fifth = pad(MAX_PADS as u8 + 1, 0);
    assert_eq!(
        s.save(&fifth).unwrap(),
        Saved::New {
            evicted: Some(pad(1, 0).addr)
        }
    );
    // A pad saved again counts as new: pad 2 is now the newest.
    s.save(&pad(2, 1)).unwrap();
    assert_eq!(
        s.save(&pad(7, 0)).unwrap(),
        Saved::New {
            evicted: Some(pad(3, 0).addr)
        }
    );
    drop(s);
    assert_eq!(
        reopen_all(&mut f),
        vec![pad(4, 0), fifth, pad(2, 1), pad(7, 0)]
    );
    assert_eq!(key_records(&f, pad(1, 0).addr), 0);
    assert_eq!(key_records(&f, pad(3, 0).addr), 0);
}

#[test]
fn damaged_records_are_skipped() {
    let mut f = Fake::new();
    let mut s = open(&mut f);
    for n in 1..=3 {
        s.save(&pad(n, 0)).unwrap();
    }
    drop(s);
    // One bit of pad 2's key flips (slot 2 of area 0).
    f.mem[0x9000 + 2 * 32 + 15] ^= 0x01;
    // Pad 3's record loses its check sum.
    f.mem[0x9000 + 3 * 32 + 30] = 0x00;
    let (mut s, rep) = Store::open(&mut f, region()).unwrap();
    assert_eq!(rep.damaged, 2);
    assert_eq!(all(&mut s), vec![pad(1, 0)]);
    // New records go after the damaged ones.
    s.save(&pad(4, 0)).unwrap();
    drop(s);
    assert_eq!(reopen_all(&mut f), vec![pad(1, 0), pad(4, 0)]);
}

#[test]
fn no_erase_ever_happens_after_start_up() {
    let mut f = Fake::new();
    let mut s = open(&mut f);
    s.save(&pad(1, 0)).unwrap();
    drop(s);
    let erases = f.erases();
    let mut s = open(&mut f);
    // Fill the area while "the radio runs".
    let mut v = 0;
    loop {
        v += 1;
        match s.save(&pad((v % 6) as u8 + 1, v as u8)) {
            Ok(_) => {}
            Err(Error::Full) => break,
            Err(e) => panic!("{e:?}"),
        }
        if v % 5 == 0 {
            let _ = s.forget(pad(3, 0).addr).unwrap();
        }
    }
    assert!(s.forget_all().is_ok(), "forget works even when full");
    assert_eq!(s.count(), 0);
    drop(s);
    assert_eq!(f.erases(), erases, "erased while the radio would run");
    assert!(
        reopen_all(&mut f).is_empty(),
        "forget-all held without a marker"
    );
    outside_untouched(&f);
}

#[test]
fn a_full_area_is_cleaned_up_at_the_next_start() {
    let mut f = Fake::new();
    let mut s = open(&mut f);
    s.save(&pad(1, 0)).unwrap();
    s.save(&pad(2, 0)).unwrap();
    // Pad 3's key changes until the area is full.
    let mut v = 0u8;
    let last = loop {
        v += 1;
        match s.save(&pad(3, v)) {
            Ok(_) => {}
            Err(Error::Full) => break pad(3, v - 1),
            Err(e) => panic!("{e:?}"),
        }
    };
    assert_eq!(s.free_slots(), 0);
    drop(s);
    let (mut s, rep) = Store::open(&mut f, region()).unwrap();
    assert!(rep.compacted);
    assert_eq!(rep.generation, 2);
    assert_eq!(s.free_slots(), SLOTS - 3);
    assert_eq!(all(&mut s), vec![pad(1, 0), pad(2, 0), last]);
    drop(s);
    // The old area (0) is blank now; area 1 is active.
    assert!(f.mem[0x9000..0xA000].iter().all(|&b| b == 0xFF));
    assert_eq!(&f.mem[0xA000..0xA004], b"PADS");
    // And again: area 1 back to area 0.
    let mut s = open(&mut f);
    for k in 0..SLOTS - 3 - COMPACT_BELOW + 1 {
        s.save(&pad(4, k as u8)).unwrap();
    }
    drop(s);
    let (mut s, rep) = Store::open(&mut f, region()).unwrap();
    assert!(rep.compacted && rep.generation == 3);
    assert_eq!(all(&mut s).len(), 4);
    drop(s);
    assert!(f.mem[0xA000..0xB000].iter().all(|&b| b == 0xFF));
    outside_untouched(&f);
}

#[test]
fn a_blank_partition_is_set_up() {
    let mut f = Fake::new();
    let (_, rep) = Store::open(&mut f, region()).unwrap();
    assert!(rep.created && !rep.repaired);
    assert_eq!(&f.mem[0x9000..0x9004], b"PADS");
    outside_untouched(&f);
}

#[test]
fn a_partition_with_other_data_is_left_alone() {
    // Somebody else's data: in area 0, in area 1, or only one byte.
    let cases: [&dyn Fn(&mut Fake); 4] = [
        &|f| f.mem[0x9000..0x9020].copy_from_slice(&[0xFE; 32]),
        &|f| f.mem[0xA100] = 0x00,
        &|f| f.mem[0x9800] = 0x7F,
        // An ESP-IDF style NVS page header (made up, but similar).
        &|f| f.mem[0x9000..0x9008].copy_from_slice(&[0xFE, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0]),
    ];
    for (k, make) in cases.iter().enumerate() {
        let mut f = Fake::new();
        make(&mut f);
        let before = f.mem.clone();
        match Store::open(&mut f, region()) {
            Err(Error::Foreign) => {}
            Err(e) => panic!("case {k}: {e:?}"),
            Ok(_) => panic!("case {k}: someone else's partition was taken"),
        }
        assert!(f.ops.is_empty(), "case {k}: something was written");
        assert_eq!(f.mem, before);
    }
}

#[test]
fn a_cut_first_header_is_repaired() {
    let mut f = Fake::new();
    f.budget = Some(10);
    assert!(Store::open(&mut f, region()).is_err());
    f.revive();
    let (mut s, rep) = Store::open(&mut f, region()).unwrap();
    assert!(rep.repaired && rep.created);
    s.save(&pad(1, 0)).unwrap();
    drop(s);
    assert_eq!(reopen_all(&mut f), vec![pad(1, 0)]);
}

#[test]
fn the_key_never_shows_in_debug_output() {
    let p = pad(1, 0);
    let text = format!("{p:?}");
    assert!(!text.contains(&format!("{}", p.key[0])));
    assert_eq!(text, "Pairing(key_type=0x04, key hidden)");
}

// ------------------------------------------------------------ power loss

/// What may and what must be there after a power cut.
struct Expect {
    /// Every pairing found must be one of these (same key, exactly).
    allowed: Vec<Pairing>,
    /// These addresses must be found (with one of their allowed keys).
    required: Vec<[u8; 6]>,
}

/// Runs `op` once to count its steps, then again from the same start
/// with the power cut after each single step. After every cut: the
/// power comes back, the store opens (never "foreign"), and what it
/// holds must match `expect`. Then it must still work.
fn cut_everywhere(setup: &dyn Fn() -> Fake, op: &dyn Fn(&mut Fake), expect: &Expect) {
    for backwards in [false, true] {
        let mut f = setup();
        f.erase_backwards = backwards;
        let start = f.steps;
        op(&mut f);
        let total = f.steps - start;
        assert!(!f.dead);
        assert!(total > 0, "the operation wrote nothing: no cut to test");
        check(&mut f, expect, "no cut");
        for cut in 0..=total {
            let mut f = setup();
            f.erase_backwards = backwards;
            f.budget = Some(cut);
            op(&mut f);
            f.revive();
            let what = format!("cut after {cut} of {total} steps, backwards={backwards}");
            check(&mut f, expect, &what);
            outside_untouched(&f);
        }
    }
}

fn check(f: &mut Fake, expect: &Expect, what: &str) {
    let got = match Store::open(&mut *f, region()) {
        Ok((mut s, _)) => all(&mut s),
        Err(e) => panic!("{what}: open failed: {e:?}"),
    };
    let mut seen = Vec::new();
    for p in &got {
        assert!(
            expect.allowed.contains(p),
            "{what}: a wrong or unexpected key"
        );
        assert!(!seen.contains(&p.addr), "{what}: one address twice");
        seen.push(p.addr);
    }
    for a in &expect.required {
        assert!(seen.contains(a), "{what}: a saved pairing was lost");
    }
    assert!(got.len() <= MAX_PADS, "{what}");
    // A second start finds the same.
    let again = reopen_all(f);
    assert_eq!(again, got, "{what}: a second start differs");
    // And it still works.
    let mut s = open(f);
    s.save(&pad(99, 9))
        .unwrap_or_else(|e| panic!("{what}: save after: {e:?}"));
    drop(s);
    assert!(reopen_all(f).contains(&pad(99, 9)), "{what}");
}

fn store_with(pads: &[Pairing]) -> Fake {
    let mut f = Fake::new();
    let mut s = open(&mut f);
    for p in pads {
        s.save(p).unwrap();
    }
    drop(s);
    f.ops.clear();
    f
}

fn open_and(
    f: &mut Fake,
    work: impl FnOnce(&mut Store<&mut Fake>) -> Result<(), Error<FakeError>>,
) {
    if let Ok((mut s, _)) = Store::open(f, region()) {
        let _ = work(&mut s);
    }
}

#[test]
fn power_loss_during_the_first_start_and_save() {
    cut_everywhere(
        &Fake::new,
        &|f| open_and(f, |s| s.save(&pad(1, 0)).map(drop)),
        &Expect {
            allowed: vec![pad(1, 0)],
            required: vec![],
        },
    );
}

#[test]
fn power_loss_while_saving_a_new_pad() {
    let a = pad(1, 0);
    let b = pad(2, 0);
    cut_everywhere(
        &|| store_with(&[a, b]),
        &|f| open_and(f, |s| s.save(&pad(3, 0)).map(drop)),
        &Expect {
            allowed: vec![a, b, pad(3, 0)],
            required: vec![a.addr, b.addr],
        },
    );
}

#[test]
fn power_loss_while_replacing_a_key() {
    let a = pad(1, 0);
    let b = pad(2, 0);
    cut_everywhere(
        &|| store_with(&[a, b]),
        &|f| open_and(f, |s| s.save(&pad(1, 1)).map(drop)),
        &Expect {
            allowed: vec![a, b, pad(1, 1)],
            required: vec![a.addr, b.addr],
        },
    );
}

#[test]
fn power_loss_while_a_fifth_pad_pushes_the_oldest_out() {
    let four: Vec<Pairing> = (1..=4).map(|n| pad(n, 0)).collect();
    let mut allowed = four.clone();
    allowed.push(pad(5, 0));
    cut_everywhere(
        &|| store_with(&four),
        &|f| open_and(f, |s| s.save(&pad(5, 0)).map(drop)),
        &Expect {
            allowed,
            required: four[1..].iter().map(|p| p.addr).collect(),
        },
    );
}

#[test]
fn power_loss_while_forgetting() {
    let a = pad(1, 0);
    let b = pad(2, 1);
    cut_everywhere(
        &|| store_with(&[a, b, pad(2, 2), pad(1, 3)]),
        &|f| open_and(f, |s| s.forget(b.addr).map(drop)),
        &Expect {
            allowed: vec![pad(1, 3), pad(2, 2)],
            required: vec![a.addr],
        },
    );
    cut_everywhere(
        &|| store_with(&[a, b, pad(3, 0)]),
        &|f| open_and(f, |s| s.forget_all().map(drop)),
        &Expect {
            allowed: vec![a, b, pad(3, 0)],
            required: vec![],
        },
    );
}

/// A store with `live` saved and the area nearly full (old keys of pad
/// 9, then pad 9 forgotten), so the next start cleans up.
fn nearly_full(live: &[Pairing]) -> Fake {
    let mut f = Fake::new();
    let mut s = open(&mut f);
    for p in live {
        s.save(p).unwrap();
    }
    while s.free_slots() > COMPACT_BELOW {
        let v = s.free_slots() as u8;
        s.save(&pad(9, v)).unwrap();
    }
    s.forget(pad(9, 0).addr).unwrap();
    drop(s);
    f.ops.clear();
    f
}

#[test]
fn power_loss_during_the_clean_up() {
    let live = [pad(1, 0), pad(2, 0), pad(3, 0)];
    cut_everywhere(
        &|| nearly_full(&live),
        &|f| open_and(f, |_| Ok(())),
        &Expect {
            allowed: live.to_vec(),
            required: live.iter().map(|p| p.addr).collect(),
        },
    );
    // The clean-up runs twice in a row (area 0 to 1, then 1 to 0).
    let twice = || {
        let mut f = nearly_full(&live);
        let mut s = open(&mut f);
        while s.free_slots() > COMPACT_BELOW {
            let v = s.free_slots() as u8;
            s.save(&pad(8, v)).unwrap();
        }
        s.forget(pad(8, 0).addr).unwrap();
        drop(s);
        f.ops.clear();
        f
    };
    cut_everywhere(
        &twice,
        &|f| open_and(f, |s| s.save(&pad(4, 0)).map(drop)),
        &Expect {
            allowed: vec![pad(1, 0), pad(2, 0), pad(3, 0), pad(4, 0)],
            required: live.iter().map(|p| p.addr).collect(),
        },
    );
}

#[test]
fn power_loss_with_a_half_erased_spare_area() {
    // A spare area with old data (as a cut clean-up leaves it).
    let live = [pad(1, 0), pad(2, 0)];
    let setup = || {
        let mut f = store_with(&live);
        f.mem[0xA000..0xA400].fill(0x3C);
        f
    };
    cut_everywhere(
        &setup,
        &|f| open_and(f, |s| s.save(&pad(3, 0)).map(drop)),
        &Expect {
            allowed: vec![pad(1, 0), pad(2, 0), pad(3, 0)],
            required: live.iter().map(|p| p.addr).collect(),
        },
    );
}

#[test]
fn the_fake_flash_behaves_like_nor_flash() {
    let mut f = Fake::new();
    use crate::store::Flash;
    let mut d = [0u32; 1];
    let b = bytemuck(&mut d);
    b.copy_from_slice(&[0x0F, 0xF0, 0xFF, 0x00]);
    f.write(0x9000, b).unwrap();
    b.copy_from_slice(&[0xFF, 0xFF, 0x0F, 0xFF]);
    f.write(0x9000, b).unwrap();
    assert_eq!(
        &f.mem[0x9000..0x9004],
        &[0x0F, 0xF0, 0x0F, 0x00],
        "bits only go to 0"
    );
    assert_eq!(f.write(0x9002, b), Err(FakeError::NotAligned));
    assert_eq!(f.erase_sector(0x9100), Err(FakeError::NotAligned));
    f.erase_sector(0x9000).unwrap();
    assert_eq!(&f.mem[0x9000..0x9004], &[0xFF; 4]);
    assert_eq!(f.ops.iter().filter(|o| o.0 == Op::Erase).count(), 1);
}

// ------------------------------------------- second reader (pad3 check)

/// The default table of espflash for this board, as entries.
fn default_table() -> Vec<crate::region::Entry> {
    use crate::region::{Entry, label};
    let e = |kind, subtype, name: &str, offset, len| Entry {
        kind,
        subtype,
        label: label(name),
        offset,
        len,
        read_only: false,
        encrypted: false,
    };
    vec![
        e(0x01, 0x02, "nvs", 0x9000, 0x6000),
        e(0x01, 0x01, "phy_init", 0xF000, 0x1000),
        e(0x00, 0x00, "factory", 0x1_0000, 0x3F_0000),
    ]
}

#[test]
fn the_nvs_partition_is_picked_by_type_and_exact_name() {
    use crate::region::{Entry, FindError, MAX_ENTRIES, find_nvs};
    let t = default_table();
    let r = find_nvs(t.iter().copied(), CAPACITY).unwrap();
    assert_eq!((r.base(), r.len()), (0x9000, 0x2000));
    // A name padded with 0xFF instead of zeros is the same name.
    let mut ff = t.clone();
    ff[0].label = [0xFF; 16];
    ff[0].label[..3].copy_from_slice(b"nvs");
    assert_eq!(find_nvs(ff, CAPACITY), Ok(r));
    // Order does not matter.
    let mut back = t.clone();
    back.reverse();
    assert_eq!(find_nvs(back, CAPACITY), Ok(r));
    // A wrong name, a wrong type, a wrong sub-type: not ours.
    for (kind, subtype, label) in [
        (0x01, 0x02, "nvs_key"),
        (0x01, 0x02, "NVS"),
        (0x01, 0x02, "nv"),
        (0x01, 0x02, "nvs!"),
        (0x00, 0x02, "nvs"),
        (0x01, 0x04, "nvs"),
    ] {
        let mut t = default_table();
        t[0].kind = kind;
        t[0].subtype = subtype;
        t[0].label = crate::region::label(label);
        assert_eq!(
            find_nvs(t, CAPACITY),
            Err(FindError::NoPartition),
            "{kind} {subtype} {label}"
        );
    }
    let mut t = default_table();
    t[0].read_only = true;
    assert_eq!(find_nvs(t, CAPACITY), Err(FindError::ReadOnly));
    let mut t = default_table();
    t[0].encrypted = true;
    assert_eq!(find_nvs(t, CAPACITY), Err(FindError::Encrypted));
    // A second "nvs" that overlaps the first one's two sectors counts as
    // another partition: refused.
    let mut t = default_table();
    t.push(Entry {
        offset: 0xA000,
        len: 0x1000,
        ..t[0]
    });
    assert_eq!(
        find_nvs(t, CAPACITY),
        Err(FindError::Region(RegionError::Overlaps))
    );
    // The program placed over the second sector (a hand-made table).
    let mut t = default_table();
    t[2].offset = 0xA000;
    assert_eq!(
        find_nvs(t, CAPACITY),
        Err(FindError::Region(RegionError::Overlaps))
    );
    // nvs on a smaller chip than the table claims.
    assert_eq!(
        find_nvs(default_table(), 0x9000 + 0x1000),
        Err(FindError::Region(RegionError::PastEnd))
    );
    // Too many entries to check: no guessing.
    let mut t = default_table();
    for k in 0..MAX_ENTRIES as u32 {
        t.push(Entry {
            label: crate::region::label("x"),
            offset: 0x40_0000 + k * 0x1000,
            ..t[1]
        });
    }
    assert_eq!(find_nvs(t, CAPACITY), Err(FindError::TooManyEntries));
    // Exactly as many as fit: fine.
    let mut t = default_table();
    for k in 0..(MAX_ENTRIES as u32 - 2) {
        t.push(Entry {
            label: crate::region::label("x"),
            offset: 0x40_0000 + k * 0x1000,
            ..t[1]
        });
    }
    assert!(find_nvs(t, CAPACITY).is_ok());
    // No table at all.
    assert_eq!(find_nvs([], CAPACITY), Err(FindError::NoPartition));
}

#[test]
fn a_huge_nvs_entry_still_gives_only_two_sectors() {
    let r = check_region(
        Span {
            offset: 0x9000,
            len: u32::MAX,
        },
        &[],
        CAPACITY,
    )
    .unwrap();
    assert_eq!((r.base(), r.len()), (0x9000, 0x2000));
    // An entry of length 0 elsewhere is not an overlap.
    let empty = Span {
        offset: 0x9000,
        len: 0,
    };
    assert!(check_region(NVS, &[empty, PHY, FACTORY], CAPACITY).is_ok());
    // An entry that touches only the second sector is an overlap.
    let tail = Span {
        offset: 0xAFFC,
        len: 4,
    };
    assert_eq!(
        check_region(NVS, &[tail], CAPACITY),
        Err(RegionError::Overlaps)
    );
    // One that starts right after the two sectors is not.
    let after = Span {
        offset: 0xB000,
        len: 4,
    };
    assert!(check_region(NVS, &[after], CAPACITY).is_ok());
}

/// The rest of the nvs partition (0xB000 on) and everything else keep
/// their bytes through a whole life: set-up, saves, forgets, two
/// clean-ups. And every write and erase lands inside 0x9000..0xB000.
#[test]
fn data_next_to_the_store_is_never_touched() {
    let mut f = Fake::new();
    for (i, b) in f.mem[0xB000..0x1_0000].iter_mut().enumerate() {
        *b = (i * 7) as u8;
    }
    let before = f.mem.clone();
    for round in 0..3u8 {
        let mut s = open(&mut f);
        for k in 0..60u8 {
            let _ = s.save(&pad(k % 6, round.wrapping_mul(61).wrapping_add(k)));
            if k % 9 == 0 {
                let _ = s.forget(pad(k % 6, 0).addr);
            }
        }
        let _ = s.forget_all();
        drop(s);
    }
    let base = 0x9000;
    let end = 0xB000;
    assert_eq!(&f.mem[..base], &before[..base]);
    assert_eq!(&f.mem[end..], &before[end..]);
    assert!(f.erases() >= 2, "the clean-up ran: {}", f.erases());
    for &(_, at, len) in &f.ops {
        assert!(
            at >= base as u32 && at + len <= end as u32,
            "{at:#x}+{len:#x}"
        );
    }
}

/// How many writes and erases each kind of event costs. The radio
/// stops for each one, so they must stay few, and an erase only ever
/// happens in `open`.
#[test]
fn flash_writes_per_event_are_few() {
    /// Opens the store, runs `op`, and returns (writes, erases) that `op`
    /// alone made (a normal start costs one header write, no erase).
    fn cost(f: &mut Fake, op: &dyn Fn(&mut Store<&mut Fake>)) -> (usize, usize) {
        let (w, e) = (f.writes(), f.erases());
        let (mut s, rep) = Store::open(&mut *f, region()).unwrap();
        assert!(!rep.compacted && !rep.cleaned_spare);
        op(&mut s);
        drop(s);
        (f.writes() - w - 1, f.erases() - e)
    }
    let mut f = Fake::new();
    drop(Store::open(&mut f, region()).unwrap());
    // The new header, then the same header again (the unlock write).
    assert_eq!((f.writes(), f.erases()), (2, 0), "a blank partition");
    assert_eq!(
        cost(&mut f, &|_| {}),
        (0, 0),
        "a normal start: the header only"
    );
    type Op<'a> = &'a dyn Fn(&mut Store<&mut Fake>);
    let cases: [(&str, Op, usize); 7] = [
        (
            "new pad",
            &|s| {
                s.save(&pad(1, 0)).unwrap();
            },
            1,
        ),
        (
            "same pairing again",
            &|s| {
                s.save(&pad(1, 0)).unwrap();
            },
            0,
        ),
        (
            "new key, known pad",
            &|s| {
                s.save(&pad(1, 1)).unwrap();
            },
            2,
        ),
        (
            "forget a pad",
            &|s| {
                s.forget(pad(1, 0).addr).unwrap();
            },
            2,
        ),
        (
            "forget an unknown pad",
            &|s| {
                s.forget(pad(7, 0).addr).unwrap();
            },
            0,
        ),
        (
            "three new pads, then forget all",
            &|s| {
                for n in 2..=4 {
                    s.save(&pad(n, 0)).unwrap();
                }
                s.forget_all().unwrap();
            },
            3 + 1 + 3,
        ),
        (
            "forget all with none saved",
            &|s| {
                s.forget_all().unwrap();
            },
            0,
        ),
    ];
    for (what, op, writes) in cases {
        assert_eq!(cost(&mut f, op), (writes, 0), "{what}");
    }
    // A fifth pad: its record, a marker for the oldest, the oldest's wipe.
    cost(&mut f, &|s| {
        for n in 10..14 {
            s.save(&pad(n, 0)).unwrap();
        }
    });
    assert_eq!(
        cost(&mut f, &|s| {
            s.save(&pad(14, 0)).unwrap();
        }),
        (3, 0),
        "a fifth pad"
    );
    // A full area: a save writes nothing and says "full". Two runs from
    // the same start, one with the extra saves, must write the same.
    let fill = |extra: bool| {
        let (mut s, _) = Store::open(f.clone(), region()).unwrap();
        let mut v = 0u8;
        while s.free_slots() > 0 {
            v = v.wrapping_add(1);
            s.save(&pad(10, v)).unwrap();
        }
        if extra {
            assert_eq!(s.save(&pad(10, 0)), Err(Error::Full), "new key");
            assert_eq!(s.save(&pad(20, 0)), Err(Error::Full), "new pad");
            assert_eq!(s.count(), MAX_PADS, "nothing dropped for it");
        }
        s.into_flash()
    };
    let (a, b) = (fill(false), fill(true));
    assert_eq!(a.writes(), b.writes());
    assert_eq!(a.mem, b.mem);
}

/// What a start finds, checked without saving anything more (the cheap
/// part of `check`, for the tests that run many thousands of cases).
fn check_light(f: &mut Fake, expect: &Expect, what: &str) {
    let got = match Store::open(&mut *f, region()) {
        Ok((mut s, _)) => all(&mut s),
        Err(e) => panic!("{what}: open failed: {e:?}"),
    };
    let mut seen = Vec::new();
    for p in &got {
        assert!(expect.allowed.contains(p), "{what}: a wrong key");
        assert!(!seen.contains(&p.addr), "{what}: one address twice");
        seen.push(p.addr);
    }
    for a in &expect.required {
        assert!(seen.contains(a), "{what}: a saved pairing was lost");
    }
    outside_untouched(f);
}

/// The power goes during the clean-up, comes back, and goes AGAIN while
/// the next start repairs things (and saves one more pad). Every pair of
/// cut points is tried.
#[test]
fn power_loss_twice_in_a_row_during_the_clean_up() {
    let live = [pad(1, 0), pad(2, 0), pad(3, 0)];
    let mut allowed = live.to_vec();
    allowed.push(pad(98, 0));
    let expect = Expect {
        allowed,
        required: live.iter().map(|p| p.addr).collect(),
    };
    let first = |f: &mut Fake| open_and(f, |_| Ok(()));
    let second = |f: &mut Fake| open_and(f, |s| s.save(&pad(98, 0)).map(drop));
    let base = nearly_full(&live);
    let mut g = base.clone();
    first(&mut g);
    let total1 = g.steps - base.steps;
    assert!(total1 > 100, "the clean-up ran: {total1} steps");
    let mut cases = 0;
    for backwards in [false, true] {
        for c1 in 0..=total1 {
            let mut f = base.clone();
            f.erase_backwards = backwards;
            f.budget = Some(c1);
            first(&mut f);
            f.revive();
            let mut g = f.clone();
            second(&mut g);
            let total2 = g.steps - f.steps;
            for c2 in 0..=total2 {
                let mut h = f.clone();
                h.budget = Some(c2);
                second(&mut h);
                h.revive();
                check_light(
                    &mut h,
                    &expect,
                    &format!("cuts at {c1} and {c2}, backwards={backwards}"),
                );
                cases += 1;
            }
        }
    }
    assert!(cases > 1_000, "{cases}");
}

/// A tiny random number maker, so the tests repeat exactly.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// A real chip does not promise to write bits in order: a cut write can
/// leave ANY mix of the bits it was turning to 0. Same for a cut wipe.
/// Thousands of random mixes, and every single bit flip of a stored
/// record (a worn cell): never a wrong key, never a lost pairing other
/// than the one being written or wiped.
#[test]
fn torn_writes_with_any_mix_of_bits_never_give_a_wrong_key() {
    let (a, b, c) = (pad(1, 0), pad(2, 0), pad(3, 0));
    let two = store_with(&[a, b]);
    let three = {
        let mut f = two.clone();
        let mut s = open(&mut f);
        s.save(&c).unwrap();
        drop(s);
        f
    };
    // Slots 1 and 2 hold a and b; c went to slot 3.
    let slot = |n: usize| 0x9000 + n * 32;
    let rec_a: Vec<u8> = three.mem[slot(1)..slot(1) + 32].to_vec();
    let rec_c: Vec<u8> = three.mem[slot(3)..slot(3) + 32].to_vec();
    assert_eq!(&rec_c[..2], &[0x50, 0x4B], "c's record is in slot 3");
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    let mask = |rng: &mut Rng| -> [u8; 32] {
        let mut m = [0u8; 32];
        match rng.next() % 3 {
            // A few bits.
            0 => {
                for _ in 0..=(rng.next() % 6) {
                    let bit = rng.next() % 256;
                    m[(bit / 8) as usize] |= 1 << (bit % 8);
                }
            }
            // About half.
            1 => {
                for x in &mut m {
                    *x = rng.next() as u8;
                }
            }
            // Most.
            _ => {
                for x in &mut m {
                    *x = (rng.next() as u8) | (rng.next() as u8);
                }
            }
        }
        m
    };
    for k in 0..3_000 {
        // c's record half written: some of its 0 bits are still 1.
        let mut f = two.clone();
        let m = mask(&mut rng);
        for i in 0..32 {
            f.mem[slot(3) + i] = rec_c[i] | m[i];
        }
        check_light(
            &mut f,
            &Expect {
                allowed: vec![a, b, c],
                required: vec![a.addr, b.addr],
            },
            &format!("torn write {k}"),
        );
        // a's record half wiped: some of its 1 bits went to 0.
        let mut f = three.clone();
        let m = mask(&mut rng);
        for i in 0..32 {
            f.mem[slot(1) + i] = rec_a[i] & !m[i];
        }
        check_light(
            &mut f,
            &Expect {
                allowed: vec![a, b, c],
                required: vec![b.addr, c.addr],
            },
            &format!("torn wipe {k}"),
        );
    }
    // Every single bit of a stored record flipped, either way.
    for bit in 0..256 {
        let mut f = three.clone();
        f.mem[slot(1) + bit / 8] ^= 1 << (bit % 8);
        check_light(
            &mut f,
            &Expect {
                allowed: vec![a, b, c],
                required: vec![b.addr, c.addr],
            },
            &format!("flip of bit {bit}"),
        );
    }
}

/// Two good records for one address (a cut between writing the new one
/// and wiping the old one leaves this): the later one wins, the address
/// is counted once, and forgetting it wipes both.
#[test]
fn two_good_records_for_one_address() {
    let (a0, a1, b) = (pad(1, 0), pad(1, 1), pad(2, 0));
    let mut f = store_with(&[a0, b]);
    let old: Vec<u8> = f.mem[0x9000 + 32..0x9000 + 64].to_vec();
    let mut s = open(&mut f);
    assert_eq!(s.save(&a1).unwrap(), Saved::Replaced);
    drop(s);
    // Put a0's record back, as if the wipe never happened.
    f.mem[0x9000 + 32..0x9000 + 64].copy_from_slice(&old);
    assert_eq!(key_records(&f, a0.addr), 2);
    let mut s = open(&mut f);
    assert_eq!(s.count(), 2);
    assert_eq!(all(&mut s), vec![b, a1], "the later record wins");
    // Saving the old key again is a change, not "unchanged".
    assert_eq!(s.save(&a0).unwrap(), Saved::Replaced);
    assert!(s.forget(a0.addr).unwrap());
    drop(s);
    assert_eq!(key_records(&f, a0.addr), 0, "every record of it wiped");
    assert_eq!(reopen_all(&mut f), vec![b]);
}

/// The flash refuses the "forget" marker (an error, not a power cut).
/// The keys must still be wiped: the owner was told they are gone, so
/// they must not come back at the next start.
#[test]
fn a_refused_forget_marker_still_wipes_the_keys() {
    let pads = [pad(1, 0), pad(2, 0), pad(3, 0)];
    // Forget all.
    let mut f = store_with(&pads);
    let n = f.writes() + 1; // after the start-up header rewrite
    f.refuse_write = Some(n);
    let mut s = open(&mut f);
    assert!(s.forget_all().is_err(), "the error is still reported");
    assert_eq!(s.count(), 0);
    drop(s);
    assert!(reopen_all(&mut f).is_empty(), "forgotten keys came back");
    for p in &pads {
        assert_eq!(key_records(&f, p.addr), 0);
    }
    // Forget one.
    let mut f = store_with(&pads);
    let n = f.writes() + 1;
    f.refuse_write = Some(n);
    let mut s = open(&mut f);
    assert!(s.forget(pads[1].addr).is_err());
    drop(s);
    assert_eq!(reopen_all(&mut f), vec![pads[0], pads[2]]);
    assert_eq!(key_records(&f, pads[1].addr), 0);
}

// ------------------------------------------------ read-only path (round 6)

/// Flash that can only be read: the fake chip's bytes, borrowed.
struct ReadOnly<'a>(&'a [u8]);

impl crate::ReadFlash for ReadOnly<'_> {
    type Error = FakeError;
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), FakeError> {
        let o = offset as usize;
        let src = self.0.get(o..o + buf.len()).ok_or(FakeError::OutsideChip)?;
        buf.copy_from_slice(src);
        Ok(())
    }
}

/// The pairings the read-only path finds in `f`, with its report.
fn read_only(f: &Fake) -> (Vec<Pairing>, crate::ReadReport) {
    let mut v = Vec::new();
    let rep = crate::read_pairings(&mut ReadOnly(&f.mem), region(), |p| v.push(*p)).unwrap();
    (v, rep)
}

/// What Store::open + load finds in a copy of `f` (open may write).
fn open_copy(f: &Fake) -> Vec<Pairing> {
    let mut g = f.clone();
    g.revive();
    match Store::open(&mut g, region()) {
        Ok((mut s, _)) => all(&mut s),
        Err(_) => Vec::new(),
    }
}

fn area_crc(f: &Fake) -> u32 {
    let base = region().base() as usize;
    crate::crc::crc32(&f.mem[base..base + 2 * AREA as usize])
}

/// A blank partition: nothing found, "blank", and the sum of 8 KB of 0xFF.
#[test]
fn read_only_on_a_blank_partition() {
    let f = Fake::new();
    let (v, rep) = read_only(&f);
    assert!(v.is_empty());
    assert!(rep.blank && !rep.found);
    assert_eq!(rep.crc, area_crc(&f));
    assert_eq!(rep.crc, crate::crc::crc32(&[0xFF; 8192]));
}

/// Random histories of saves, forgets, forget-alls, restarts (which
/// compact) and power cuts at random moments: after every step the
/// read-only path finds exactly what Store::open + load would, in the
/// same order, and its check sum is the CRC-32 of the 8 KB. It never
/// changes a byte (it cannot: ReadFlash has no write; checked anyway).
#[test]
fn read_only_finds_what_open_finds_over_random_histories() {
    let mut rng = Rng(0x0BAD_5EED_1234_5678);
    let mut checks = 0;
    for history in 0..60 {
        let mut f = Fake::new();
        for step in 0..120 {
            let what = rng.next() % 100;
            let n = (rng.next() % 7) as u8;
            let v = (rng.next() % 3) as u8;
            // Sometimes the power goes during this step.
            let cut = rng.next() % 10 == 0;
            if cut {
                f.budget = Some(rng.next() % 200);
            }
            if let Ok((mut s, _)) = Store::open(&mut f, region()) {
                let _ = match what {
                    0..=59 => s.save(&pad(n, v)).map(|_| ()),
                    60..=84 => s.forget(pad(n, 0).addr).map(|_| ()),
                    85..=89 => s.forget_all().map(|_| ()),
                    _ => Ok(()),
                };
            }
            f.revive();
            let before = f.mem.clone();
            let (got, rep) = read_only(&f);
            assert_eq!(f.mem, before, "the read-only path changed the flash");
            assert_eq!(rep.crc, area_crc(&f), "history {history} step {step}");
            assert_eq!(rep.count, got.len());
            let want = open_copy(&f);
            assert_eq!(got, want, "history {history} step {step}");
            checks += 1;
        }
    }
    assert!(checks >= 7_000);
}

/// Damaged records (single bit flips anywhere in the active area) are
/// counted, never handed out as a wrong key, and the result still
/// matches Store::open.
#[test]
fn read_only_with_bit_flips_matches_open() {
    let base = pad_store_base();
    let good = store_with(&[pad(1, 0), pad(2, 0), pad(3, 0)]);
    let mut rng = Rng(0x5151_7777_9999_3333);
    for _ in 0..2_000 {
        let mut f = good.clone();
        let at = base + (rng.next() % (2 * AREA as u64)) as usize;
        f.mem[at] ^= 1 << (rng.next() % 8);
        let (got, rep) = read_only(&f);
        assert_eq!(got, open_copy(&f));
        for p in &got {
            assert!([pad(1, 0), pad(2, 0), pad(3, 0)].contains(p), "a wrong key");
        }
        assert_eq!(rep.crc, area_crc(&f));
    }
}

fn pad_store_base() -> usize {
    region().base() as usize
}

/// The report's Debug output holds no address and no key.
#[test]
fn read_only_report_shows_no_secret() {
    let f = store_with(&[pad(9, 1)]);
    let (got, rep) = read_only(&f);
    assert_eq!(got.len(), 1);
    let text = format!("{rep:?} {:?}", got[0]);
    let key_hex: String = got[0].key.iter().map(|b| format!("{b:02x}")).collect();
    assert!(!text.contains(&key_hex[..8]), "{text}");
    assert!(!text.contains("16, 32, 48"), "{text}");
}
