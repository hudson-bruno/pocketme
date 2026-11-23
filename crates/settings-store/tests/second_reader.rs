//! The second reader's own power-cut and rubbish tests (round 5 check),
//! written apart from `cuts.rs` and `format.rs`, with their own pretend
//! flash and their own record writer (from the format in the crate's
//! notes, not from its code).
//!
//! What must always hold:
//! - never a panic;
//! - the value read is always 0..=4;
//! - after a cut, the value is the old one or the new one;
//! - never a stuck store: after any cut or any rubbish, the next save
//!   works and reads back;
//! - nothing outside the two sectors is ever touched.

use board_api::link::SECTOR;
use board_api::{Store, StoreError};
use settings_store::{AREA, RECORD, SLOTS, Saved, Settings, SettingsStore};

/// One guard sector on each side of the area.
const BASE: u32 = SECTOR;
const SIZE: u32 = AREA + 2 * SECTOR;
const GUARD: u8 = 0x5A;

/// How a cut leaves an erase.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EraseCut {
    /// Words erased one by one up to the cut; the word at the cut has
    /// some of its bits set.
    InOrder,
    /// Every word of the sector got some way: each 0 bit is 1 with
    /// even odds.
    Everywhere,
}

/// Flash rules: erase sets a sector to 0xFF, a write only clears bits.
/// Power for `budget` more 4-byte words (written or erased); at the cut
/// the word in hand is left half done and the power stays off.
struct Pretend {
    mem: Vec<u8>,
    budget: Option<u64>,
    words: u64,
    off: bool,
    erase_cut: EraseCut,
    rng: u64,
    erases: u32,
}

impl Pretend {
    fn new() -> Self {
        let mut mem = vec![GUARD; SIZE as usize];
        mem[BASE as usize..(BASE + AREA) as usize].fill(0xFF);
        Pretend {
            mem,
            budget: None,
            words: 0,
            off: false,
            erase_cut: EraseCut::InOrder,
            rng: 0x1234_5678_9abc_def1,
            erases: 0,
        }
    }

    fn rand(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    /// One more word of work: false when the power goes now.
    fn spend(&mut self) -> bool {
        self.words += 1;
        match &mut self.budget {
            Some(0) => {
                self.off = true;
                false
            }
            Some(b) => {
                *b -= 1;
                true
            }
            None => true,
        }
    }

    fn guards_untouched(&self) -> bool {
        self.mem[..BASE as usize].iter().all(|&b| b == GUARD)
            && self.mem[(BASE + AREA) as usize..]
                .iter()
                .all(|&b| b == GUARD)
    }

    fn area(&mut self) -> &mut [u8] {
        &mut self.mem[BASE as usize..(BASE + AREA) as usize]
    }
}

impl Store for Pretend {
    fn size(&self) -> u32 {
        SIZE
    }

    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), StoreError> {
        if self.off {
            return Err(StoreError::Io);
        }
        let end = offset as usize + buf.len();
        if end > SIZE as usize {
            return Err(StoreError::OutOfRange);
        }
        buf.copy_from_slice(&self.mem[offset as usize..end]);
        Ok(())
    }

    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), StoreError> {
        if self.off {
            return Err(StoreError::Io);
        }
        assert!(
            offset.is_multiple_of(4) && data.len().is_multiple_of(4),
            "unaligned write"
        );
        assert!(
            offset >= BASE && offset as usize + data.len() <= (BASE + AREA) as usize,
            "write outside the area at {offset:#x}"
        );
        for (k, w) in data.chunks(4).enumerate() {
            let at = offset as usize + 4 * k;
            if !self.spend() {
                // Half done: some of the bits that should clear did.
                for (i, &d) in w.iter().enumerate() {
                    let keep = self.rand() as u8;
                    let want = self.mem[at + i] & d;
                    self.mem[at + i] = want | (self.mem[at + i] & keep);
                }
                return Err(StoreError::Io);
            }
            for (i, &d) in w.iter().enumerate() {
                self.mem[at + i] &= d;
            }
        }
        Ok(())
    }

    fn erase_sector(&mut self, offset: u32) -> Result<(), StoreError> {
        if self.off {
            return Err(StoreError::Io);
        }
        assert!(offset.is_multiple_of(SECTOR), "unaligned erase");
        assert!(
            offset >= BASE && offset + SECTOR <= BASE + AREA,
            "erase outside the area at {offset:#x}"
        );
        let s = offset as usize;
        for w in 0..(SECTOR as usize / 4) {
            if !self.spend() {
                match self.erase_cut {
                    EraseCut::InOrder => {
                        for i in 0..4 {
                            let r = self.rand() as u8;
                            self.mem[s + 4 * w + i] |= r;
                        }
                    }
                    EraseCut::Everywhere => {
                        for i in 0..SECTOR as usize {
                            let r = self.rand() as u8;
                            self.mem[s + i] |= r;
                        }
                    }
                }
                return Err(StoreError::Io);
            }
            self.mem[s + 4 * w..s + 4 * w + 4].fill(0xFF);
        }
        self.erases += 1;
        Ok(())
    }
}

/// CRC-32 (zip), written here bit by bit.
fn crc(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// A record as the crate's notes describe it.
fn record(seq: u32, kind: u8, values: [u8; 7]) -> [u8; 16] {
    let mut r = [0u8; 16];
    r[0..4].copy_from_slice(&seq.to_le_bytes());
    r[4] = kind;
    r[5..12].copy_from_slice(&values);
    let sum = crc(&r[..12]) ^ settings_store::SALT;
    r[12..16].copy_from_slice(&sum.to_le_bytes());
    r
}

fn bright(b: u8) -> Settings {
    Settings::DEFAULT.with_brightness(b)
}

/// After any state: load, check the value, then the store must not be
/// stuck: a save of another value works, and a fresh load sees it.
fn load_and_unstick(f: &mut Pretend, allowed: &[u8], what: &str) -> u8 {
    f.off = false;
    f.budget = None;
    let mut s = SettingsStore::new(BASE);
    let got = s.load(f).brightness;
    assert!(got <= 4, "{what}: brightness {got} out of range");
    assert!(
        allowed.contains(&got),
        "{what}: brightness {got}, allowed {allowed:?}"
    );
    let other = (got + 1) % 5;
    let r = s.save(f, &bright(other));
    assert!(
        matches!(r, Ok(Saved::Written { .. })),
        "{what}: stuck store, save gave {r:?}"
    );
    let back = SettingsStore::new(BASE).load(f).brightness;
    assert_eq!(
        back, other,
        "{what}: the save after the cut did not read back"
    );
    // And once more, the way back.
    let mut s2 = SettingsStore::new(BASE);
    s2.load(f);
    assert!(matches!(
        s2.save(f, &bright(got)),
        Ok(Saved::Written { .. })
    ));
    assert_eq!(SettingsStore::new(BASE).load(f).brightness, got, "{what}");
    assert!(
        f.guards_untouched(),
        "{what}: bytes outside the area changed"
    );
    got
}

/// The values a run saves: each differs from the one before.
fn plan(n: usize, seed: u64) -> Vec<u8> {
    let mut x = seed | 1;
    let mut cur = 4u8;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let v = ((cur as u64 + 1 + x % 4) % 5) as u8;
            cur = v;
            v
        })
        .collect()
}

/// Runs `values` as saves from `start` (each with a fresh store loaded
/// at the beginning, the way a board starts), the power going after
/// `budget` words. Returns (value before the save that was cut, value
/// of that save) or None when the run finished.
fn run(f: &mut Pretend, values: &[u8], budget: Option<u64>) -> Option<(u8, u8)> {
    f.budget = budget;
    let mut s = SettingsStore::new(BASE);
    let mut cur = s.load(f).brightness;
    // The board's start: load, then prepare (may erase the spare sector).
    if s.prepare(f).is_err() {
        assert!(f.off);
        return Some((cur, cur));
    }
    for &v in values {
        match s.save(f, &bright(v)) {
            Ok(_) => cur = v,
            Err(_) => {
                assert!(f.off, "a save failed with the power on");
                return Some((cur, v));
            }
        }
    }
    // A cut can also come in prepare at the next start.
    let mut s = SettingsStore::new(BASE);
    s.load(f);
    if s.prepare(f).is_err() {
        assert!(f.off);
        return Some((cur, cur));
    }
    None
}

fn every_cut(start: &Pretend, values: &[u8], how: EraseCut, what: &str) -> (u64, u32) {
    let mut whole = clone(start);
    whole.erase_cut = how;
    assert!(run(&mut whole, values, None).is_none());
    let total = whole.words;
    let erases = whole.erases;
    for k in 0..=total {
        let mut f = clone(start);
        f.erase_cut = how;
        f.rng ^= k.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        match run(&mut f, values, Some(k)) {
            Some((old, new)) => {
                load_and_unstick(&mut f, &[old, new], &format!("{what} cut at word {k}"));
            }
            None => {
                let last = *values.last().unwrap();
                load_and_unstick(&mut f, &[last], &format!("{what} no cut at {k}"));
            }
        }
    }
    (total, erases)
}

fn clone(f: &Pretend) -> Pretend {
    Pretend {
        mem: f.mem.clone(),
        budget: None,
        words: 0,
        off: false,
        erase_cut: f.erase_cut,
        rng: f.rng,
        erases: 0,
    }
}

#[test]
fn every_cut_from_blank_through_two_erases() {
    // 800 saves: both sectors filled, then each erased once (the first
    // erase at save 513, the second at 769).
    let values = plan(800, 7);
    for how in [EraseCut::InOrder, EraseCut::Everywhere] {
        let (words, erases) = every_cut(&Pretend::new(), &values, how, "blank");
        assert!(erases >= 2, "{erases}");
        assert!(words > 600 * 4, "{words}");
    }
}

#[test]
fn every_cut_from_a_nearly_full_sector_and_after_prepare() {
    // Start with 250 records in sector 0, then cut through the switch,
    // the erase and the first records of the other sector.
    let mut f = Pretend::new();
    run(&mut f, &plan(250, 3), None);
    // Fill sector 1 with old records too, so its erase does work.
    let mut g = Pretend::new();
    run(&mut g, &plan(256 + 250, 5), None);
    for (start, name) in [(f, "250 used"), (g, "both used")] {
        for how in [EraseCut::InOrder, EraseCut::Everywhere] {
            every_cut(&start, &plan(40, 11), how, name);
        }
    }
}

#[test]
fn rubbish_never_panics_never_sticks() {
    let mut x = 0xDEAD_BEEF_u64;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for round in 0..300 {
        let mut f = Pretend::new();
        let kind = round % 6;
        for (i, b) in f.area().iter_mut().enumerate() {
            *b = match kind {
                0 => next() as u8, // random bytes
                1 => 0x00,         // all zero, nothing blank
                2 => {
                    if i % 16 < 8 {
                        0x00
                    } else {
                        0xFF
                    }
                } // half records
                3 => (next() as u8) | (next() as u8), // a half-done erase
                4 => (next() as u8) & (next() as u8), // a half-done write
                _ => 0xFF,
            };
        }
        // Some good records of every kind, number and value among it.
        let n = next() % 40;
        for _ in 0..n {
            let slot = (next() % (2 * SLOTS as u64)) as usize;
            let seq = match next() % 4 {
                0 => (next() % 1000) as u32 + 1,
                1 => (next() as u32) | 1,
                2 => 0,
                _ => (next() % 5) as u32,
            };
            let kind = [0u8, 1, 1, 1, 2, 0xFF][(next() % 6) as usize];
            let mut v = [0xFFu8; 7];
            v[0] = [0, 1, 2, 3, 4, 5, 200, 0xFF][(next() % 8) as usize];
            let r = record(seq, kind, v);
            f.area()[slot * RECORD..slot * RECORD + RECORD].copy_from_slice(&r);
        }
        load_and_unstick(
            &mut f,
            &[0, 1, 2, 3, 4],
            &format!("rubbish round {round} kind {kind}"),
        );
    }
}

/// KNOWN LIMIT (found by this check, not fixed): numbers do not wrap.
/// Record number 0xFFFF_FFFF is never read as a record, and the next
/// number stops there. So once the newest record carries 0xFFFF_FFFE (or
/// a run of saves gets there), every later save writes a record the
/// next load skips: `save` says `Written`, the flash keeps the old value.
/// Only 4.29 billion saves, or rubbish with a good salted CRC-32 and a
/// number at the top (odds about 2^-32 per slot), get there. A clean fix
/// needs a format change (see the round 5 check report). This test pins
/// what happens, so a change to it is noticed: never a panic, the value
/// stays in range and stays the old one.
#[test]
fn known_limit_a_number_at_the_top_stops_saves() {
    for seq in [u32::MAX - 1, u32::MAX - 2] {
        let mut f = Pretend::new();
        let mut v = [0xFF; 7];
        v[0] = 2;
        f.area()[..RECORD].copy_from_slice(&record(seq, 1, v));
        let mut s = SettingsStore::new(BASE);
        assert_eq!(s.load(&mut f).brightness, 2);
        for b in [3u8, 0, 4, 1] {
            let r = s.save(&mut f, &bright(b));
            assert!(matches!(r, Ok(Saved::Written { .. })), "{r:?}");
            let back = SettingsStore::new(BASE).load(&mut f).brightness;
            if seq == u32::MAX - 2 && b == 3 {
                // One number was left: this save is kept.
                assert_eq!(back, 3);
            } else {
                assert!(back == 2 || back == 3, "{back}");
            }
        }
        assert!(f.guards_untouched());
    }
    // Lower down (300 numbers left) everything still works.
    let mut f = Pretend::new();
    let mut v = [0xFF; 7];
    v[0] = 2;
    f.area()[..RECORD].copy_from_slice(&record(u32::MAX - 300, 2, v));
    load_and_unstick(&mut f, &[4], "300 numbers left");
}

#[test]
fn values_out_of_range_read_as_brightest_and_unknown_kinds_are_skipped() {
    let mut f = Pretend::new();
    let mut v = [0xFF; 7];
    v[0] = 200;
    f.area()[..16].copy_from_slice(&record(1, 1, v));
    assert_eq!(SettingsStore::new(BASE).load(&mut f).brightness, 4);
    v[0] = 1;
    f.area()[16..32].copy_from_slice(&record(2, 1, v));
    v[0] = 3;
    // A newer kind with a higher number: skipped, the settings stay 1.
    f.area()[32..48].copy_from_slice(&record(3, 7, v));
    assert_eq!(SettingsStore::new(BASE).load(&mut f).brightness, 1);
    // An older format (no salt in the sum): not a record.
    let mut old = record(9, 1, v);
    let sum = crc(&old[..12]);
    old[12..16].copy_from_slice(&sum.to_le_bytes());
    f.area()[48..64].copy_from_slice(&old);
    assert_eq!(SettingsStore::new(BASE).load(&mut f).brightness, 1);
    load_and_unstick(&mut f, &[1], "mixed kinds");
}

#[test]
fn bit_flips_after_many_saves() {
    let values = plan(700, 13);
    let mut base = Pretend::new();
    run(&mut base, &values, None);
    let last = *values.last().unwrap();
    let mut x = 77u64;
    for round in 0..400 {
        let mut f = clone(&base);
        let flips = 1 + round % 8;
        for _ in 0..flips {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let bit = (x % (AREA as u64 * 8)) as usize;
            f.area()[bit / 8] ^= 1 << (bit % 8);
        }
        // A flip may spoil the newest record: then an older value (or
        // the default) is right. Never anything else.
        let got = load_and_unstick(&mut f, &[0, 1, 2, 3, 4], &format!("flips round {round}"));
        let _ = (got, last);
    }
}

#[test]
fn a_sector_that_will_not_erase_does_not_stick_the_other() {
    // A flash whose erase always fails: saves fill the sector in use,
    // then fail, and never write outside the area or panic.
    struct NoErase(Pretend);
    impl Store for NoErase {
        fn size(&self) -> u32 {
            self.0.size()
        }
        fn read(&mut self, o: u32, b: &mut [u8]) -> Result<(), StoreError> {
            self.0.read(o, b)
        }
        fn write(&mut self, o: u32, d: &[u8]) -> Result<(), StoreError> {
            self.0.write(o, d)
        }
        fn erase_sector(&mut self, _: u32) -> Result<(), StoreError> {
            Err(StoreError::Io)
        }
    }
    let mut f = NoErase(Pretend::new());
    let mut s = SettingsStore::new(BASE);
    s.load(&mut f);
    let mut ok = 0;
    for (i, v) in plan(700, 21).into_iter().enumerate() {
        match s.save(&mut f, &bright(v)) {
            Ok(Saved::Written { .. }) => {
                ok += 1;
                assert_eq!(SettingsStore::new(BASE).load(&mut f).brightness, v, "{i}");
            }
            Ok(Saved::Unchanged) => assert_eq!(s.current().brightness, v),
            Err(_) => {}
        }
    }
    // Both sectors were blank: 512 records fit without an erase.
    assert_eq!(ok, 2 * SLOTS as usize);
    assert!(f.0.guards_untouched());
}

#[test]
fn a_worn_slot_that_drops_writes_is_stepped_over() {
    // Writes into some slots do nothing (worn cells that stay blank, or
    // stay half written): the save must notice on reading back, and use
    // the next slot.
    struct Worn(Pretend, Vec<usize>);
    impl Store for Worn {
        fn size(&self) -> u32 {
            self.0.size()
        }
        fn read(&mut self, o: u32, b: &mut [u8]) -> Result<(), StoreError> {
            self.0.read(o, b)
        }
        fn write(&mut self, o: u32, d: &[u8]) -> Result<(), StoreError> {
            let slot = (o - BASE) as usize / RECORD;
            if self.1.contains(&slot) {
                // Only the first word sticks.
                return self.0.write(o, &d[..4]);
            }
            self.0.write(o, d)
        }
        fn erase_sector(&mut self, o: u32) -> Result<(), StoreError> {
            self.0.erase_sector(o)
        }
    }
    let mut f = Worn(Pretend::new(), vec![3, 4, 100, 255, 256, 300, 301, 302]);
    let mut s = SettingsStore::new(BASE);
    s.load(&mut f);
    for (i, v) in plan(900, 31).into_iter().enumerate() {
        match s.save(&mut f, &bright(v)) {
            Ok(Saved::Written { .. }) => {
                assert_eq!(
                    SettingsStore::new(BASE).load(&mut f).brightness,
                    v,
                    "save {i}"
                );
            }
            Ok(Saved::Unchanged) => {}
            // Three worn slots in a row (300, 301, 302): this save gives
            // up, and the next one goes on after them.
            Err(settings_store::Error::Verify) => {}
            Err(e) => panic!("save {i}: {e:?}"),
        }
    }
    assert!(f.0.guards_untouched());
}
