//! The settings partition check and the store that cannot leave it: the
//! CYD's real table is taken, every broken table is refused, and no
//! read, write or erase of the store reaches the chip outside the area
//! (the saved gamepad pairing at 0x9000..0xB000 above all).

use board_api::link::SECTOR;
use board_api::{Store, StoreError};
use settings_store::area::{
    AreaStore, ENTRY_LEN, Entry, FindError, Flash, MAX_ENTRIES, find_area, label,
};
use settings_store::{AREA, Saved, Settings, SettingsStore};

const MB4: u32 = 4 * 1024 * 1024;
/// Where boards/cyd/partitions.csv puts it.
const EXPECTED: u32 = 0x3F_E000;

fn entry(kind: u8, subtype: u8, name: &str, offset: u32, len: u32) -> Entry {
    Entry {
        kind,
        subtype,
        label: label(name),
        offset,
        len,
        read_only: false,
        encrypted: false,
    }
}

/// The table of boards/cyd/partitions.csv.
fn cyd_table() -> Vec<Entry> {
    vec![
        entry(1, 0x02, "nvs", 0x9000, 0x6000),
        entry(1, 0x01, "phy_init", 0xF000, 0x1000),
        entry(0, 0x00, "factory", 0x1_0000, 0x3E_0000),
        entry(1, 0x06, "settings", EXPECTED, 0x2000),
    ]
}

#[test]
fn the_cyd_table_gives_the_last_8_kb() {
    let a = find_area(cyd_table(), MB4, EXPECTED).unwrap();
    assert_eq!(a.base(), 0x3F_E000);
    assert_eq!(a.end(), MB4);
}

#[test]
fn the_csv_file_is_the_table_the_tests_use() {
    // The CSV the board is flashed with, read here, so the two cannot
    // drift apart.
    let csv = include_str!("../../../boards/cyd/partitions.csv");
    let rows: Vec<Vec<&str>> = csv
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split(',').map(str::trim).collect())
        .collect();
    let num = |s: &str| u32::from_str_radix(s.trim_start_matches("0x"), 16).unwrap();
    let got: Vec<(String, u32, u32)> = rows
        .iter()
        .map(|r| (r[0].to_string(), num(r[3]), num(r[4])))
        .collect();
    let want: Vec<(String, u32, u32)> = cyd_table()
        .iter()
        .map(|e| {
            let n = e.label.iter().position(|&b| b == 0).unwrap();
            (
                String::from_utf8(e.label[..n].to_vec()).unwrap(),
                e.offset,
                e.len,
            )
        })
        .collect();
    assert_eq!(got, want);
    assert_eq!(rows[3][1], "data");
    assert_eq!(rows[3][2], "undefined");
    // nvs and phy_init exactly as espflash's default table has them.
    assert_eq!((got[0].1, got[0].2), (0x9000, 0x6000));
    assert_eq!((got[1].1, got[1].2), (0xF000, 0x1000));
}

/// The first 192 bytes of the binary table espflash 4.7 makes from
/// boards/cyd/partitions.csv (`espflash partition-table --to-binary`):
/// four entries, the MD5 line, blank flash.
const CYD_TABLE_BIN: [u8; 192] = [
    0xaa, 0x50, 0x01, 0x02, 0x00, 0x90, 0x00, 0x00, 0x00, 0x60, 0x00, 0x00, 0x6e, 0x76, 0x73, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xaa, 0x50, 0x01, 0x01, 0x00, 0xf0, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x70, 0x68, 0x79, 0x5f,
    0x69, 0x6e, 0x69, 0x74, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xaa, 0x50, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x3e, 0x00, 0x66, 0x61, 0x63, 0x74,
    0x6f, 0x72, 0x79, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xaa, 0x50, 0x01, 0x06, 0x00, 0xe0, 0x3f, 0x00, 0x00, 0x20, 0x00, 0x00, 0x73, 0x65, 0x74, 0x74,
    0x69, 0x6e, 0x67, 0x73, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xeb, 0xeb, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xf2, 0xc7, 0x00, 0x20, 0xdc, 0x0c, 0xc2, 0x1a, 0x9f, 0xce, 0xc6, 0xc3, 0x54, 0x43, 0xf3, 0x47,
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
];

/// The table's lines, read the way the board reads them: one 32-byte
/// line at a time, up to the first that is not an entry.
fn parse_all(bin: &[u8]) -> Vec<Entry> {
    bin.as_chunks::<ENTRY_LEN>()
        .0
        .iter()
        .map_while(Entry::parse)
        .collect()
}

#[test]
fn the_binary_table_parses_to_the_cyd_table() {
    assert_eq!(parse_all(&CYD_TABLE_BIN), cyd_table());
    let a = find_area(parse_all(&CYD_TABLE_BIN), MB4, EXPECTED).unwrap();
    assert_eq!(a.base(), EXPECTED);
}

#[test]
fn the_table_ends_at_the_md5_line_or_blank_flash() {
    let mut md5 = [0xFFu8; ENTRY_LEN];
    md5[..2].copy_from_slice(&[0xEB, 0xEB]);
    assert_eq!(Entry::parse(&md5), None);
    assert_eq!(Entry::parse(&[0xFF; ENTRY_LEN]), None);
    assert_eq!(Entry::parse(&[0x00; ENTRY_LEN]), None);
    // A blank table: no settings.
    assert_eq!(
        find_area(parse_all(&[0xFF; 192]), MB4, EXPECTED),
        Err(FindError::NoPartition)
    );
}

#[test]
fn the_flags_are_read_as_esp_idf_writes_them() {
    let settings = |flags: u8| {
        let mut l: [u8; ENTRY_LEN] = CYD_TABLE_BIN[96..128].try_into().unwrap();
        l[28] = flags;
        Entry::parse(&l).unwrap()
    };
    // Bit 0: encrypted. Bit 1: read-only (esp-idf-part 0.6: ENCRYPTED =
    // 0b01, READONLY = 0b10).
    let e = settings(0b01);
    assert!(e.encrypted && !e.read_only);
    let r = settings(0b10);
    assert!(r.read_only && !r.encrypted);
    let mut t = cyd_table();
    t[3] = e;
    assert_eq!(
        find_area(t.clone(), MB4, EXPECTED),
        Err(FindError::Encrypted)
    );
    t[3] = r;
    assert_eq!(find_area(t, MB4, EXPECTED), Err(FindError::ReadOnly));
}

#[test]
fn the_default_table_has_no_settings() {
    // espflash's own table (what the board had before): the store is
    // off, nothing is written.
    let t = vec![
        entry(1, 0x02, "nvs", 0x9000, 0x6000),
        entry(1, 0x01, "phy_init", 0xF000, 0x1000),
        entry(0, 0x00, "factory", 0x1_0000, 0x3F_0000),
    ];
    assert_eq!(find_area(t, MB4, EXPECTED), Err(FindError::NoPartition));
}

#[test]
fn the_name_type_and_subtype_must_all_match() {
    for (kind, sub, name) in [
        (0, 0x06, "settings"),
        (1, 0x02, "settings"),
        (1, 0x06, "setting"),
        (1, 0x06, "settings2"),
        (1, 0x06, "Settings"),
    ] {
        let mut t = cyd_table();
        t[3] = entry(kind, sub, name, EXPECTED, 0x2000);
        assert_eq!(
            find_area(t, MB4, EXPECTED),
            Err(FindError::NoPartition),
            "{kind} {sub} {name}"
        );
    }
    // A name that ends in 0xFF bytes (as ESP-IDF pads) is the same name.
    let mut t = cyd_table();
    t[3].label = [0xFF; 16];
    t[3].label[..8].copy_from_slice(b"settings");
    assert!(find_area(t, MB4, EXPECTED).is_ok());
}

#[test]
fn broken_entries_are_refused() {
    let with = |f: &dyn Fn(&mut Entry)| {
        let mut t = cyd_table();
        f(&mut t[3]);
        find_area(t, MB4, EXPECTED)
    };
    assert_eq!(with(&|e| e.read_only = true), Err(FindError::ReadOnly));
    assert_eq!(with(&|e| e.encrypted = true), Err(FindError::Encrypted));
    assert_eq!(with(&|e| e.len = 0x1000), Err(FindError::WrongSize));
    assert_eq!(with(&|e| e.len = 0x3000), Err(FindError::WrongSize));
    assert_eq!(
        with(&|e| e.offset = EXPECTED + 4),
        Err(FindError::NotAligned)
    );
    // Past the end of a 4 MB chip, and past the end of 32 bits.
    assert_eq!(with(&|e| e.offset = MB4), Err(FindError::PastEnd));
    assert_eq!(with(&|e| e.offset = MB4 - 0x1000), Err(FindError::PastEnd));
    assert_eq!(with(&|e| e.offset = 0xFFFF_F000), Err(FindError::PastEnd));
    // Below the table: the bootloader and the table itself.
    assert_eq!(with(&|e| e.offset = 0x8000), Err(FindError::BelowTable));
    assert_eq!(with(&|e| e.offset = 0x0), Err(FindError::BelowTable));
    // Somewhere free, but not where this firmware looks.
    let mut t = cyd_table();
    t[2].len = 0x3D_0000;
    t[3].offset = 0x3E_0000;
    assert_eq!(find_area(t, MB4, EXPECTED), Err(FindError::Moved));
}

#[test]
fn an_area_on_the_pairing_or_the_program_is_refused() {
    // On the saved gamepad pairing (the first 8 KB of nvs), anywhere in
    // nvs, on phy_init, and on the program.
    for at in [0x9000, 0xB000, 0xD000, 0xE000, 0xF000, 0x1_0000, 0x20_0000] {
        let mut t = cyd_table();
        t[3].offset = at;
        assert_eq!(find_area(t, MB4, at), Err(FindError::Overlaps), "{at:#x}");
    }
    // A program that runs to the end of the chip (the old size).
    let mut t = cyd_table();
    t[2].len = 0x3F_0000;
    assert_eq!(
        find_area(t.clone(), MB4, EXPECTED),
        Err(FindError::Overlaps)
    );
    // Touching edges are not an overlap.
    t[2].len = 0x3E_E000;
    assert!(find_area(t, MB4, EXPECTED).is_ok());
    // An entry that runs past 32 bits overlaps everything after it.
    let mut t = cyd_table();
    t.push(entry(1, 0x81, "huge", 0x3F_0000, u32::MAX));
    assert_eq!(find_area(t, MB4, EXPECTED), Err(FindError::Overlaps));
    // An empty entry overlaps nothing.
    let mut t = cyd_table();
    t.push(entry(1, 0x81, "empty", EXPECTED, 0));
    assert!(find_area(t, MB4, EXPECTED).is_ok());
    // One byte in, at either end, is an overlap.
    for (at, len) in [(MB4 - 1, 1), (EXPECTED - 0x1000, 0x1001)] {
        let mut t = cyd_table();
        t[2].len = 0x3D_0000;
        t.push(entry(1, 0x81, "edge", at, len));
        assert_eq!(
            find_area(t, MB4, EXPECTED),
            Err(FindError::Overlaps),
            "{at:#x}+{len:#x}"
        );
    }
}

#[test]
fn a_second_settings_entry_is_checked_as_another_partition() {
    let mut t = cyd_table();
    t.push(entry(1, 0x06, "settings", EXPECTED, 0x2000));
    assert_eq!(find_area(t, MB4, EXPECTED), Err(FindError::Overlaps));
}

#[test]
fn too_many_entries_are_not_guessed_at() {
    let mut t = cyd_table();
    let fill = |t: &mut Vec<Entry>, n: usize| {
        for i in 0..n {
            t.insert(0, entry(1, 0x81, "x", 0x10_0000 + i as u32 * 0x1000, 0));
        }
    };
    fill(&mut t, MAX_ENTRIES - 3);
    assert!(find_area(t.clone(), MB4, EXPECTED).is_ok());
    fill(&mut t, 1);
    assert_eq!(find_area(t, MB4, EXPECTED), Err(FindError::TooManyEntries));
}

/// A 4 MB chip that records every chip address it is asked for.
struct Chip {
    bytes: Vec<u8>,
    /// Every (first, end) of a write or erase.
    changed: Vec<(u32, u32)>,
    /// Every (first, end) of a read.
    read: Vec<(u32, u32)>,
}

impl Chip {
    fn new() -> Chip {
        let mut bytes = vec![0xFF; MB4 as usize];
        // Something in nvs, to see it stays.
        for (i, b) in bytes[0x9000..0xB000].iter_mut().enumerate() {
            *b = (i * 7 + 3) as u8;
        }
        Chip {
            bytes,
            changed: Vec::new(),
            read: Vec::new(),
        }
    }
}

impl Flash for Chip {
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), StoreError> {
        let a = addr as usize;
        buf.copy_from_slice(&self.bytes[a..a + buf.len()]);
        self.read.push((addr, addr + buf.len() as u32));
        Ok(())
    }

    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), StoreError> {
        let a = addr as usize;
        for (d, s) in self.bytes[a..a + data.len()].iter_mut().zip(data) {
            *d &= s;
        }
        self.changed.push((addr, addr + data.len() as u32));
        Ok(())
    }

    fn erase_sector(&mut self, addr: u32) -> Result<(), StoreError> {
        let a = addr as usize;
        self.bytes[a..a + SECTOR as usize].fill(0xFF);
        self.changed.push((addr, addr + SECTOR));
        Ok(())
    }
}

fn store() -> AreaStore<Chip> {
    let area = find_area(cyd_table(), MB4, EXPECTED).unwrap();
    AreaStore::new(Chip::new(), area)
}

#[test]
fn offsets_count_from_the_area() {
    let mut s = store();
    assert_eq!(s.size(), AREA);
    s.write(0, &[0x12, 0x34, 0x56, 0x78]).unwrap();
    s.erase_sector(SECTOR).unwrap();
    let mut b = [0u8; 4];
    s.read(AREA - 4, &mut b).unwrap();
    assert_eq!(
        s.flash().changed,
        [(EXPECTED, EXPECTED + 4), (EXPECTED + SECTOR, MB4)]
    );
    assert_eq!(s.flash().read, [(MB4 - 4, MB4)]);
    assert_eq!(s.flash().bytes[EXPECTED as usize], 0x12);
}

#[test]
fn nothing_outside_the_area_reaches_the_chip() {
    let mut s = store();
    let four = [0u8; 4];
    let mut buf = [0u8; 16];
    // Past the end, across the end, offsets that wrap, a chip address
    // given as an offset (the pairing at 0x9000), unaligned.
    for off in [
        AREA,
        AREA - 4,
        AREA - 2,
        u32::MAX - 3,
        0x9000,
        0x3F_E000,
        2,
        1,
    ] {
        assert_eq!(
            s.write(off, &[0; 8]),
            Err(StoreError::OutOfRange),
            "{off:#x}"
        );
        assert_eq!(
            s.read(off, &mut buf),
            Err(StoreError::OutOfRange),
            "{off:#x}"
        );
    }
    // Lengths that are not whole words.
    assert_eq!(s.write(0, &four[..3]), Err(StoreError::OutOfRange));
    assert_eq!(s.read(0, &mut buf[..5]), Err(StoreError::OutOfRange));
    // Erase: only the two sectors of the area.
    for off in [AREA, 2 * AREA, 4, 0x800, u32::MAX, 0xFFFF_F000] {
        assert_eq!(s.erase_sector(off), Err(StoreError::OutOfRange), "{off:#x}");
    }
    assert!(s.flash().changed.is_empty());
    assert!(s.flash().read.is_empty());
}

#[test]
fn a_thousand_saves_stay_inside_and_the_pairing_is_untouched() {
    let mut s = store();
    let before = s.flash().bytes.clone();
    let mut set = SettingsStore::new(0);
    assert_eq!(set.load(&mut s), Settings::DEFAULT);
    let mut erases = 0;
    for i in 0..1000u32 {
        let want = Settings::DEFAULT.with_brightness((i % 5) as u8);
        match set.save(&mut s, &want).unwrap() {
            Saved::Written { erased } => erases += u32::from(erased),
            Saved::Unchanged => {}
        }
        erases += u32::from(set.prepare(&mut s).unwrap());
        let mut again = SettingsStore::new(0);
        assert_eq!(again.load(&mut s), want, "save {i}");
    }
    assert!(erases > 0);
    for &(a, b) in s.flash().changed.iter().chain(&s.flash().read) {
        assert!(a >= EXPECTED && b <= MB4, "{a:#x}..{b:#x}");
    }
    let after = &s.flash().bytes;
    assert_eq!(after[..EXPECTED as usize], before[..EXPECTED as usize]);
}
