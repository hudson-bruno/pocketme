//! The record format and the rules around it: defaults, no write when
//! nothing changed, unknown records, damaged areas, wear, errors, the
//! save timer.

mod common;

use board_api::fake::FakeStore;
use board_api::link::SECTOR;
use board_api::{Store, StoreError};
use common::{BASE, CutFlash, Rng};
use settings_store::{
    AREA, BOOT_ANIM_FULL, BOOT_ANIM_OFF, BOOT_ANIM_SHORT, Error, KIND_SETTINGS, QUIET_US, RECORD,
    SALT, SLOTS, SaveTimer, Saved, Settings, SettingsStore, crc32,
};

/// A record as a newer firmware might write it.
fn record(seq: u32, kind: u8, values: [u8; 7]) -> [u8; RECORD] {
    let mut r = [0xFF; RECORD];
    r[0..4].copy_from_slice(&seq.to_le_bytes());
    r[4] = kind;
    r[5..12].copy_from_slice(&values);
    let sum = crc32(&r[..12]) ^ SALT;
    r[12..16].copy_from_slice(&sum.to_le_bytes());
    r
}

fn put(f: &mut CutFlash, sector: u32, slot: u32, r: &[u8; RECORD]) {
    let at = (BASE + sector * SECTOR + slot * RECORD as u32) as usize;
    f.bytes[at..at + RECORD].copy_from_slice(r);
}

fn load(f: &mut CutFlash) -> (SettingsStore, Settings) {
    let mut s = SettingsStore::new(BASE);
    let got = s.load(f);
    (s, got)
}

#[test]
fn nothing_stored_gives_the_defaults_and_writes_nothing() {
    let mut f = CutFlash::blank();
    let (mut s, got) = load(&mut f);
    assert_eq!(got, Settings::DEFAULT);
    assert_eq!(got.brightness, 4, "the brightest step");
    assert!(!s.found());
    // Saving the defaults over nothing: nothing written.
    assert_eq!(s.save(&mut f, &Settings::DEFAULT), Ok(Saved::Unchanged));
    assert_eq!(f.writes, 0);
}

#[test]
fn a_save_survives_a_restart_and_the_same_value_is_not_written_again() {
    let mut f = CutFlash::blank();
    let (mut s, _) = load(&mut f);
    let two = Settings::DEFAULT.with_brightness(2);
    assert_eq!(s.save(&mut f, &two), Ok(Saved::Written { erased: false }));
    assert_eq!(f.writes, 1);
    assert_eq!(f.units, RECORD as u64, "one 16-byte record");
    assert_eq!(s.save(&mut f, &two), Ok(Saved::Unchanged));
    let (mut s, got) = load(&mut f);
    assert_eq!(got, two);
    assert!(s.found());
    assert_eq!(s.save(&mut f, &two), Ok(Saved::Unchanged));
    assert_eq!(f.writes, 1, "no write when nothing changed");
    // Back to the default value: that is a change now.
    assert!(matches!(
        s.save(&mut f, &Settings::DEFAULT),
        Ok(Saved::Written { .. })
    ));
    assert_eq!(load(&mut f).1, Settings::DEFAULT);
}

#[test]
fn steps_above_the_brightest_read_as_the_brightest() {
    assert_eq!(Settings::DEFAULT.with_brightness(9).brightness, 4);
    let mut f = CutFlash::blank();
    put(
        &mut f,
        0,
        0,
        &record(1, KIND_SETTINGS, [7, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]),
    );
    let (mut s, got) = load(&mut f);
    assert_eq!(got.brightness, 4);
    // It reads as 4, so saving 4 writes nothing.
    assert_eq!(s.save(&mut f, &got), Ok(Saved::Unchanged));
}

#[test]
fn unknown_records_are_skipped_but_their_numbers_and_slots_count() {
    let mut f = CutFlash::blank();
    put(
        &mut f,
        0,
        0,
        &record(5, KIND_SETTINGS, [1, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]),
    );
    // A newer firmware's kind 9, numbered later, and a settings record
    // with values this firmware does not use.
    put(&mut f, 0, 1, &record(6, 9, [0; 7]));
    put(
        &mut f,
        0,
        2,
        &record(7, KIND_SETTINGS, [3, 11, 0xFF, 0xFF, 0xFF, 0xFF, 22]),
    );
    put(&mut f, 0, 3, &record(8, 9, [1; 7]));
    let (mut s, got) = load(&mut f);
    assert_eq!(got.brightness, 3, "the newest settings record");
    assert_eq!(s.used(), 4);
    let one = got.with_brightness(1);
    s.save(&mut f, &one).unwrap();
    // Written after the kind-9 record, numbered after it, with the
    // values it did not know kept.
    let at = (BASE + 4 * RECORD as u32) as usize;
    assert_eq!(
        f.bytes[at..at + RECORD],
        record(9, KIND_SETTINGS, [1, 11, 0xFF, 0xFF, 0xFF, 0xFF, 22])
    );
    let (_, back) = load(&mut f);
    assert_eq!(back, one);
}

#[test]
fn a_damaged_or_blank_area_gives_the_defaults_and_still_works() {
    let mut rng = Rng::new(9);
    for fill in ["zeros", "random", "half"] {
        let mut f = CutFlash::blank();
        let area = BASE as usize..(BASE + AREA) as usize;
        for i in area.clone() {
            f.bytes[i] = match fill {
                "zeros" => 0,
                "random" => rng.next() as u8,
                _ if (i / 512) % 2 == 0 => rng.next() as u8,
                _ => 0xFF,
            };
        }
        let (mut s, got) = load(&mut f);
        assert_eq!(got, Settings::DEFAULT, "{fill}");
        assert!(!s.found(), "{fill}");
        for step in [0, 3, 1] {
            let v = Settings::DEFAULT.with_brightness(step);
            assert!(
                matches!(s.save(&mut f, &v), Ok(Saved::Written { .. })),
                "{fill}"
            );
            assert_eq!(load(&mut f).1, v, "{fill}");
        }
    }
}

#[test]
fn a_record_with_one_wrong_bit_is_skipped() {
    let mut f = CutFlash::blank();
    put(
        &mut f,
        0,
        0,
        &record(1, KIND_SETTINGS, [1, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]),
    );
    let mut bad = record(2, KIND_SETTINGS, [2, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    bad[5] ^= 0x10;
    put(&mut f, 0, 1, &bad);
    let (s, got) = load(&mut f);
    assert_eq!(got.brightness, 1, "the older good record");
    assert_eq!(s.used(), 2, "the bad slot is not used again");
}

#[test]
fn wear_one_erase_per_sector_per_512_changes() {
    let mut f = CutFlash::blank();
    let (mut s, mut cur) = load(&mut f);
    let n = 10 * 512;
    let mut first_erase = None;
    for i in 1..=n {
        cur = cur.with_brightness((cur.brightness + 1) % 5);
        if s.save(&mut f, &cur) == Ok(Saved::Written { erased: true }) && first_erase.is_none() {
            first_erase = Some(i);
        }
    }
    let per_sector = &f.erases[1..3];
    println!("{n} changes: erases per sector {per_sector:?}, the first at change {first_erase:?}");
    assert_eq!(
        first_erase,
        Some(2 * SLOTS as usize + 1),
        "512 changes before any erase"
    );
    // After that, one erase per 256 changes, the two sectors in turn.
    let total = (n - 2 * SLOTS as usize).div_ceil(SLOTS as usize) as u32;
    assert_eq!(per_sector.iter().sum::<u32>(), total);
    assert!(per_sector[0].abs_diff(per_sector[1]) <= 1);
    assert_eq!(f.erases[0] + f.erases[3], 0, "nothing outside the area");
}

#[test]
fn prepare_erases_the_spare_only_when_worth_it() {
    let mut f = CutFlash::blank();
    let (mut s, mut cur) = load(&mut f);
    assert_eq!(s.prepare(&mut f), Ok(false), "empty");
    // Sector 0 full, sector 1 half: sector 0 is the spare, not blank.
    for _ in 0..SLOTS + SLOTS / 2 {
        cur = cur.with_brightness((cur.brightness + 1) % 5);
        s.save(&mut f, &cur).unwrap();
    }
    assert_eq!(s.prepare(&mut f), Ok(true));
    assert_eq!(s.prepare(&mut f), Ok(false));
    assert_eq!(load(&mut f).1, cur, "the value is kept");
    // The rest of sector 1 and the switch need no erase now.
    for _ in 0..SLOTS / 2 + 10 {
        cur = cur.with_brightness((cur.brightness + 1) % 5);
        assert_eq!(s.save(&mut f, &cur), Ok(Saved::Written { erased: false }));
    }
}

#[test]
fn an_area_that_does_not_fit_is_an_error_and_loads_the_defaults() {
    let mut f = CutFlash::blank();
    for base in [1, SECTOR + 4, 3 * SECTOR, 4 * SECTOR, u32::MAX - 4095] {
        let mut s = SettingsStore::new(base);
        assert_eq!(s.load(&mut f), Settings::DEFAULT, "{base}");
        let v = Settings::DEFAULT.with_brightness(1);
        assert_eq!(s.save(&mut f, &v), Err(Error::Area), "{base}");
    }
    assert_eq!(f.writes, 0);
}

/// A flash whose cells in one place no longer clear (worn out).
struct Stuck {
    inner: CutFlash,
    stuck: core::ops::Range<u32>,
}

impl Store for Stuck {
    fn size(&self) -> u32 {
        self.inner.size()
    }
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), StoreError> {
        self.inner.read(offset, buf)
    }
    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), StoreError> {
        if self.stuck.contains(&offset) {
            return Ok(()); // says yes, changes nothing
        }
        self.inner.write(offset, data)
    }
    fn erase_sector(&mut self, offset: u32) -> Result<(), StoreError> {
        self.inner.erase_sector(offset)
    }
}

#[test]
fn a_worn_slot_is_left_and_the_next_one_used() {
    let r = RECORD as u32;
    // Slot 0 is worn: the next one is used.
    let mut f = Stuck {
        inner: CutFlash::blank(),
        stuck: BASE..BASE + r,
    };
    let mut s = SettingsStore::new(BASE);
    s.load(&mut f);
    let v = Settings::DEFAULT.with_brightness(0);
    assert_eq!(s.save(&mut f, &v), Ok(Saved::Written { erased: false }));
    assert_eq!(SettingsStore::new(BASE).load(&mut f.inner), v);
    // Three worn slots in a row: an error, and the value before stays.
    let mut f = Stuck {
        inner: CutFlash::blank(),
        stuck: BASE..BASE + 3 * r,
    };
    let mut s = SettingsStore::new(BASE);
    s.load(&mut f);
    assert_eq!(s.save(&mut f, &v), Err(Error::Verify));
    assert_eq!(
        SettingsStore::new(BASE).load(&mut f.inner),
        Settings::DEFAULT
    );
    // The next save goes on after them.
    assert!(s.save(&mut f, &v).is_ok());
    assert_eq!(SettingsStore::new(BASE).load(&mut f.inner), v);
}

#[test]
fn a_store_that_cannot_be_read_gives_the_defaults() {
    let mut f = CutFlash::blank();
    f.dead = true;
    let mut s = SettingsStore::new(BASE);
    assert_eq!(s.load(&mut f), Settings::DEFAULT);
    let v = Settings::DEFAULT.with_brightness(1);
    assert_eq!(s.save(&mut f, &v), Err(Error::Store(StoreError::Io)));
    f.power_on();
    assert!(s.save(&mut f, &v).is_ok(), "works once the store does");
}

#[test]
fn works_on_the_board_layers_fake_store() {
    // board-api's own fake flash: 4 sectors; the area at sector 2.
    let mut f = FakeStore::new();
    let mut s = SettingsStore::new(2 * SECTOR);
    assert_eq!(s.load(&mut f), Settings::DEFAULT);
    let v = Settings::DEFAULT.with_brightness(2);
    s.save(&mut f, &v).unwrap();
    assert_eq!(SettingsStore::new(2 * SECTOR).load(&mut f), v);
    assert!(f.bytes[..2 * SECTOR as usize].iter().all(|&b| b == 0xFF));
}

#[test]
fn the_save_timer_waits_for_quiet() {
    let mut t = SaveTimer::new();
    assert!(!t.due(u64::MAX) && t.deadline().is_none());
    t.changed(1_000);
    assert_eq!(t.deadline(), Some(1_000 + QUIET_US));
    assert!(!t.due(1_000 + QUIET_US - 1));
    // Another change moves it on.
    t.changed(500_000);
    assert!(!t.due(1_000 + QUIET_US));
    assert!(t.pending());
    assert!(t.due(500_000 + QUIET_US));
    assert!(!t.due(500_000 + QUIET_US), "once");
    assert!(!t.pending());
}

#[test]
fn the_boot_animation_is_full_when_nothing_is_stored() {
    let mut f = CutFlash::blank();
    let (_, got) = load(&mut f);
    assert_eq!(got.boot_anim(), BOOT_ANIM_FULL);
    // Choosing full over nothing stored: nothing to write.
    let (mut s, _) = load(&mut f);
    let full = got.with_boot_anim(BOOT_ANIM_FULL);
    assert_eq!(full, Settings::DEFAULT, "a blank value stays blank");
    assert_eq!(s.save(&mut f, &full), Ok(Saved::Unchanged));
    assert_eq!(f.writes, 0);
}

#[test]
fn the_boot_animation_is_saved_and_read_back_with_the_brightness() {
    let mut f = CutFlash::blank();
    let (mut s, _) = load(&mut f);
    for anim in [BOOT_ANIM_SHORT, BOOT_ANIM_OFF, BOOT_ANIM_FULL] {
        let new = s.current().with_boot_anim(anim).with_brightness(1);
        s.save(&mut f, &new).unwrap();
        let (_, back) = load(&mut f);
        assert_eq!(back.boot_anim(), anim);
        assert_eq!(back.brightness, 1, "the brightness is kept beside it");
    }
    // It is value byte 6 (record byte 6), 0 1 2.
    let (mut s, _) = load(&mut f);
    let off = s.current().with_boot_anim(BOOT_ANIM_OFF);
    s.save(&mut f, &off).unwrap();
    let at = (BASE + (s.used() as u32 - 1) * RECORD as u32) as usize;
    assert_eq!(f.bytes[at + 6], 2);
}

#[test]
fn an_old_record_or_an_unknown_boot_animation_reads_as_full() {
    // A record written before the value existed: byte 6 is 0xFF.
    let mut f = CutFlash::blank();
    put(
        &mut f,
        0,
        0,
        &record(1, KIND_SETTINGS, [2, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]),
    );
    let (_, got) = load(&mut f);
    assert_eq!((got.brightness, got.boot_anim()), (2, BOOT_ANIM_FULL));

    // A value from a newer firmware: full here, and kept as it was when
    // this firmware saves a brightness change.
    let mut f = CutFlash::blank();
    put(
        &mut f,
        0,
        0,
        &record(1, KIND_SETTINGS, [2, 7, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]),
    );
    let (mut s, got) = load(&mut f);
    assert_eq!(got.boot_anim(), BOOT_ANIM_FULL);
    let new = got.with_boot_anim(BOOT_ANIM_FULL).with_brightness(3);
    s.save(&mut f, &new).unwrap();
    let at = (BASE + RECORD as u32) as usize;
    assert_eq!(
        f.bytes[at..at + RECORD],
        record(2, KIND_SETTINGS, [3, 7, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF])
    );
    // Above the highest value counts as full.
    assert_eq!(
        Settings::DEFAULT.with_boot_anim(9).boot_anim(),
        BOOT_ANIM_FULL
    );
}
