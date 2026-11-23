//! The real game files that games/build-chip.sh makes (PADTEST.BIN,
//! STARDUST.BIN), loaded through the loader into pretend slots.
//!
//! The files come from GAME_FILES_DIR (boards/desktop/check.sh sets it),
//! or from this crate's own build folder:
//!
//!   GAMES_TARGET=$PWD/target/chip ../../games/build-chip.sh
//!
//! MERCATOR.BIN is a game with a flash part ("GAMF"): its two halves are
//! beside it in flash/MERCATOR/ (CODE.BIN, RO.BIN), and a fake firmware
//! that holds them loads it.
//!
//! At the end: the spill probe's files (experiments/spill-probe), whose
//! code runs on past the code slot into the spill (stage 1). They come
//! from SPILL_FILES_DIR, or from the folder the probe's README builds
//! them into (boards/cyd/target/spill-probe/chip-games/files). Those
//! tests print a note and check nothing when the files are not there:
//! the probe is an experiment, not part of every build.

mod common;

use common::*;
use game_api::GameHeader;
use game_loader::{LoadError, Slots, check_file, load};

/// Loads `file` into slots full of rubbish.
fn load_file(file: &[u8]) -> (Result<game_loader::Loaded, LoadError>, Card, FakeSlots) {
    let mut card = Card::new(file.to_vec());
    let mut fake = FakeSlots::console();
    let got = load(&mut card, PATH, &Slots::CONSOLE, &mut fake);
    (got, card, fake)
}

/// The same comparison as games/chip/check_load.py: every byte the
/// linker placed (each section that takes memory, at its run-time
/// address) is what the slots now hold.
fn assert_matches_elf(name: &str, fake: &FakeSlots, elf: &[u8]) {
    let sections = elf_sections(elf);
    assert!(
        sections.iter().any(|s| s.name == ".text"),
        "{name}: no .text"
    );
    for s in &sections {
        for i in 0..s.size {
            let want = s.bytes.as_ref().map_or(0, |b| b[i as usize]);
            let got = fake.byte_at(s.addr + i);
            assert_eq!(
                got,
                Some(want),
                "{name}: {} byte at {:#x} should be {want:#04x}",
                s.name,
                s.addr + i
            );
        }
    }
}

#[test]
fn the_real_games_load_byte_for_byte_as_linked() {
    for (name, file, elf) in real_games() {
        let (got, card, fake) = load_file(&file);
        let loaded = got.unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_matches_elf(name, &fake, &elf);
        assert_eq!(loaded.entry(), elf_entry(&elf), "{name}: entry");

        // The numbers for the README: one pass (copy while checking)
        // against two (check the whole file first, then copy).
        let h = loaded.header();
        let mut again = Card::new(file.clone());
        let check = check_file(&mut again, PATH, &Slots::CONSOLE).unwrap();
        eprintln!(
            "{name}: file {} bytes (code {}, data {}, zeroed {}); \
             one pass: {} reads, {} bytes, {} code words; \
             two passes: {} reads, {} bytes",
            file.len(),
            h.code_size,
            h.data_size,
            h.bss_size,
            card.reads(),
            card.bytes_read(),
            fake.code_writes,
            card.reads() + again.reads(),
            card.bytes_read() + again.bytes_read(),
        );
        assert_eq!(card.bytes_read(), file.len());
        assert_eq!(loaded.stats().bytes as usize, file.len());
        assert_eq!(check, loaded.stats());
    }
}

#[test]
fn each_real_game_loads_over_the_other_and_after_a_failure() {
    let games = real_games();
    let mut fake = FakeSlots::console();
    for round in 0..2 {
        for (name, file, elf) in &games {
            // A damaged copy first: refused, leaves rubbish behind.
            let mut bad = file.clone();
            let last = bad.len() - 1;
            bad[last] ^= 0xFF;
            let got = load(&mut Card::new(bad), PATH, &Slots::CONSOLE, &mut fake);
            assert!(
                matches!(got, Err(LoadError::Damaged { .. })),
                "{name} {round}"
            );
            let good = load(
                &mut Card::new(file.clone()),
                PATH,
                &Slots::CONSOLE,
                &mut fake,
            );
            assert!(good.is_ok(), "{name} {round}");
            assert_matches_elf(name, &fake, elf);
        }
    }
}

#[test]
fn real_files_cut_short_at_every_length_are_refused() {
    for (name, file, _) in real_games() {
        for len in 0..file.len() {
            // The card tells the truth about the size...
            let (got, _, fake) = load_file(&file[..len]);
            assert!(got.is_err(), "{name} cut to {len}");
            assert_eq!(fake.touches(), 0, "{name} cut to {len}");
            // ...or still says the full size (the file shrank after it was
            // listed). Then the loader finds out while reading.
            let mut card = Card::new(file[..len].to_vec());
            card.size_says = Some(file.len() as u32);
            let mut fake = FakeSlots::console();
            let got = load(&mut card, PATH, &Slots::CONSOLE, &mut fake);
            let ok = match len {
                0..4 => got == Err(LoadError::NotAGame),
                4..36 => matches!(got, Err(LoadError::CutShort { .. })),
                _ => got == Err(LoadError::Changed),
            };
            assert!(ok, "{name} cut to {len}: {got:?}");
            assert_eq!(fake.zero_calls, 0);
        }
    }
}

#[test]
fn real_files_with_a_read_error_at_every_call_are_refused() {
    for (name, file, _) in real_games() {
        let calls = load_file(&file).1.calls.len();
        for n in 0..calls {
            let mut card = Card::new(file.clone());
            card.fail_call = Some(n);
            let mut fake = FakeSlots::console();
            let got = load(&mut card, PATH, &Slots::CONSOLE, &mut fake);
            assert_eq!(
                got,
                Err(LoadError::Card(board_api::FsError::Io)),
                "{name} call {n}"
            );
            assert_eq!(fake.zero_calls, 0);
        }
    }
}

#[test]
fn real_files_that_change_between_two_reads_are_refused() {
    for (name, file, _) in real_games() {
        let reads = load_file(&file).1.reads();
        let mut grown = file.clone();
        grown.extend_from_slice(&[0; 4]);
        let mut shrunk = file.clone();
        shrunk.truncate(file.len() - 4);
        // Before every read but the header (read 0).
        for after in 1..reads {
            for (what, new) in [("grows", &grown), ("shrinks", &shrunk)] {
                let mut card = Card::new(file.clone());
                card.change = Some((after, new.clone()));
                let got = load(&mut card, PATH, &Slots::CONSOLE, &mut FakeSlots::console());
                if what == "shrinks" && after == reads - 1 {
                    // Only the look past the end is left: every byte of the
                    // game was read and checked before the change.
                    assert!(got.is_ok(), "{name} {what} after {after}: {got:?}");
                } else {
                    assert_eq!(got, Err(LoadError::Changed), "{name} {what} after {after}");
                }
            }
        }
    }
}

/// Seeded random damage: flipped bits, changed bytes, bytes cut out or
/// put in, a changed header word. Every damaged file must be refused,
/// and no damage may make the loader panic (the pretend slots also
/// panic on any access outside a slot or a byte access to code).
#[test]
fn real_files_with_random_damage_are_never_accepted() {
    let mut rng = Rng::new(0xC0FF_EE00_1234_5678);
    let mut refused = std::collections::BTreeMap::<&str, u32>::new();
    for (name, file, _) in real_games() {
        for _ in 0..3000 {
            let mut bad = file.clone();
            match rng.below(6) {
                0 => {
                    // One to eight bits flipped anywhere.
                    for _ in 0..1 + rng.below(8) {
                        let at = rng.below(bad.len());
                        bad[at] ^= 1 << rng.below(8);
                    }
                }
                1 => {
                    // A run of random bytes.
                    let at = rng.below(bad.len());
                    let len = (1 + rng.below(64)).min(bad.len() - at);
                    for b in &mut bad[at..at + len] {
                        *b = rng.next() as u8;
                    }
                }
                2 => {
                    // Bytes cut out of the middle.
                    let at = rng.below(bad.len());
                    let len = (1 + rng.below(600)).min(bad.len() - at);
                    bad.drain(at..at + len);
                }
                3 => {
                    // Bytes put in.
                    let at = rng.below(bad.len() + 1);
                    let len = 1 + rng.below(600);
                    let more: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
                    bad.splice(at..at, more);
                }
                4 => {
                    // One header word changed by a little (often still a
                    // header that passes the checks: only the checksum
                    // finds it).
                    let word = rng.below(GameHeader::SIZE / 4) * 4;
                    let v = u32::from_le_bytes(bad[word..word + 4].try_into().unwrap());
                    let v = v.wrapping_add((rng.below(17) as u32).wrapping_sub(8).wrapping_mul(4));
                    bad[word..word + 4].copy_from_slice(&v.to_le_bytes());
                }
                _ => {
                    // A block of the card swapped for one from elsewhere
                    // in the file.
                    let blocks = bad.len() / 512;
                    if blocks >= 2 {
                        let (a, b) = (rng.below(blocks), rng.below(blocks));
                        let copy = bad[b * 512..b * 512 + 512].to_vec();
                        bad[a * 512..a * 512 + 512].copy_from_slice(&copy);
                    }
                }
            }
            if bad == file {
                continue;
            }
            let (got, _, _) = load_file(&bad);
            let Err(err) = got else {
                panic!("{name}: a damaged file was accepted");
            };
            *refused.entry(err.notice()).or_default() += 1;
        }
    }
    eprintln!("refusals by notice: {refused:?}");
    assert!(refused.contains_key("GAME FILE IS DAMAGED"));
}

// ---- The spill probe (experiments/spill-probe) ---------------------------

/// The folder with the probe's files: SPILL_FILES_DIR, or the folder its
/// README builds into.
fn probe_dir() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("SPILL_FILES_DIR") {
        return dir.into();
    }
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../boards/cyd/target/spill-probe/chip-games/files")
}

/// One probe file (`NAME.BIN`, and with `elf` also `NAME.elf`), or `None`
/// with a note when the probe was not built.
fn probe_file(name: &str, elf: bool) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    let dir = probe_dir();
    let bin = std::fs::read(dir.join(format!("{name}.BIN")));
    let Ok(bin) = bin else {
        eprintln!(
            "note: {} has no {name}.BIN: the spill probe's tests check nothing \
             (experiments/spill-probe/README.md says how to build it)",
            dir.display()
        );
        return None;
    };
    let elf = elf.then(|| {
        std::fs::read(dir.join(format!("{name}.elf")))
            .unwrap_or_else(|e| panic!("{name}.elf beside {name}.BIN: {e}"))
    });
    Some((bin, elf))
}

#[test]
fn the_spill_probes_load_byte_for_byte_as_linked() {
    for name in ["SPILL", "SPILLMAX"] {
        let Some((file, Some(elf))) = probe_file(name, true) else {
            return;
        };
        let (got, card, fake) = load_file(&file);
        let loaded = got.unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let h = loaded.header();
        let spill = h.code_size - Slots::CONSOLE.code_size;
        // A probe: its code really is past the slot.
        assert!(spill > 0, "{name}: no code past the code slot");
        // Every byte the linker placed is where the chip would run it
        // from; the spill's bytes are read through the mirror.
        assert_matches_elf(name, &fake, &elf);
        assert_eq!(loaded.entry(), elf_entry(&elf), "{name}: entry");
        // The spill's bytes are in the data slot, and no data or zeroing
        // went over them (the fake is strict: it would have panicked).
        assert_eq!(fake.code_bytes_in_data(), spill as usize, "{name}");
        assert_eq!(card.bytes_read(), file.len());
        eprintln!(
            "{name}: file {} bytes (code {} of which {spill} past the slot, \
             data {}, zeroed {}); {} code words written",
            file.len(),
            h.code_size,
            h.data_size,
            h.bss_size,
            fake.code_writes,
        );
    }
}

#[test]
fn the_spill_probes_load_over_each_other_and_over_a_normal_game() {
    let Some((spill, Some(spill_elf))) = probe_file("SPILL", true) else {
        return;
    };
    let Some((max, Some(max_elf))) = probe_file("SPILLMAX", true) else {
        return;
    };
    let normal = real_games();
    let mut fake = FakeSlots::console();
    for round in 0..2 {
        for (name, file, elf) in [
            ("SPILLMAX", &max, &max_elf),
            ("SPILL", &spill, &spill_elf),
            (normal[0].0, &normal[0].1, &normal[0].2),
            ("SPILL", &spill, &spill_elf),
        ] {
            let got = load(
                &mut Card::new(file.clone()),
                PATH,
                &Slots::CONSOLE,
                &mut fake,
            );
            assert!(got.is_ok(), "{name} {round}: {got:?}");
            assert_matches_elf(name, &fake, elf);
        }
    }
}

#[test]
fn the_bad_probe_files_are_refused_before_anything_is_written() {
    let Some((bad, _)) = probe_file("SPILLBAD", false) else {
        return;
    };
    let Some((big, _)) = probe_file("SPILLBIG", false) else {
        return;
    };
    let (got, _, fake) = load_file(&bad);
    println!("SPILLBAD: {got:?}");
    assert!(
        matches!(got, Err(LoadError::CodeMeetsData { .. })),
        "SPILLBAD: {got:?}"
    );
    assert_eq!(got.unwrap_err().notice(), "GAME TOO BIG FOR SLOTS");
    assert_eq!(fake.touches(), 0, "SPILLBAD touched the slots");
    let (got, _, fake) = load_file(&big);
    println!("SPILLBIG: {got:?}");
    assert!(
        matches!(got, Err(LoadError::CodeTooBig { .. })),
        "SPILLBIG: {got:?}"
    );
    assert_eq!(got.unwrap_err().notice(), "GAME CODE TOO BIG");
    assert_eq!(fake.touches(), 0, "SPILLBIG touched the slots");
}

#[test]
fn without_the_spill_the_probes_are_refused_and_normal_games_still_load() {
    // What the firmware does when its mirror check fails at start-up: it
    // sets the spill to 0. Games that need it are refused before anything
    // is written; the others load as before.
    let slots = Slots {
        spill_size: 0,
        ..Slots::CONSOLE
    };
    for name in ["SPILL", "SPILLMAX"] {
        let Some((file, _)) = probe_file(name, false) else {
            return;
        };
        let mut fake = FakeSlots::new(slots);
        let got = load(&mut Card::new(file), PATH, &slots, &mut fake);
        assert!(
            matches!(got, Err(LoadError::CodeTooBig { .. })),
            "{name}: {got:?}"
        );
        assert_eq!(fake.touches(), 0, "{name} touched the slots");
    }
    for (name, file, elf) in real_games() {
        let mut fake = FakeSlots::new(slots);
        let got = load(&mut Card::new(file), PATH, &slots, &mut fake);
        assert!(got.is_ok(), "{name}: {got:?}");
        assert_matches_elf(name, &fake, &elf);
    }
}

/// MERCATOR.BIN, its ELF and its two flash halves.
type FlashGame = (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>);

/// MERCATOR.BIN and its flash halves, or `None` (with a note) when the
/// built files have no MERCATOR (an older build folder).
fn mercator() -> Option<FlashGame> {
    let dir = game_files_dir();
    let read = |p: &str| std::fs::read(dir.join(p)).ok();
    match (
        read("MERCATOR.BIN"),
        read("MERCATOR.elf"),
        read("flash/MERCATOR/CODE.BIN"),
        read("flash/MERCATOR/RO.BIN"),
    ) {
        (Some(file), Some(elf), Some(code), Some(ro)) => Some((file, elf, code, ro)),
        _ => {
            eprintln!(
                "note: no MERCATOR flash part in {}: not checked",
                dir.display()
            );
            None
        }
    }
}

#[test]
fn mercator_with_its_flash_part_loads_as_linked() {
    let Some((file, elf, code, ro)) = mercator() else {
        return;
    };
    assert_eq!(&file[..4], b"GAMF");
    let ext = game_api::FlashPartHeader::from_bytes(file[36..68].try_into().unwrap());

    // A firmware without the part refuses it before writing anything.
    let (got, _, fake) = load_file(&file);
    assert_eq!(got, Err(LoadError::FlashPartMissing));
    assert_eq!(fake.touches(), 0);

    // One that holds other bytes at the same place: differs.
    let mut fake = FakeSlots::console();
    let mut other = code.clone();
    other[0] ^= 1;
    fake.holds = Some((ext, other, ro.clone()));
    let got = load(
        &mut Card::new(file.clone()),
        PATH,
        &Slots::CONSOLE,
        &mut fake,
    );
    assert!(
        matches!(got, Err(LoadError::FlashPartDiffers { .. })),
        "{got:?}"
    );
    assert_eq!(fake.touches(), 0);

    // The firmware that holds it: the slots equal the ELF, less the two
    // flash sections, which must be the held halves.
    let mut fake = FakeSlots::console();
    fake.holds = Some((ext, code.clone(), ro.clone()));
    let loaded = load(
        &mut Card::new(file.clone()),
        PATH,
        &Slots::CONSOLE,
        &mut fake,
    )
    .unwrap();
    assert_eq!(loaded.flash_part(), Some(&ext));
    assert_eq!(loaded.entry(), elf_entry(&elf));
    let sections = elf_sections(&elf);
    for s in &sections {
        let held = match s.name.as_str() {
            ".flash_text" => &code,
            ".flash_rodata" => &ro,
            _ => continue,
        };
        assert_eq!(s.bytes.as_deref(), Some(held.as_slice()), "{}", s.name);
    }
    for s in sections.iter().filter(|s| !s.name.starts_with(".flash_")) {
        for i in 0..s.size {
            let want = s.bytes.as_ref().map_or(0, |b| b[i as usize]);
            assert_eq!(
                fake.byte_at(s.addr + i),
                Some(want),
                "{} at {:#x}",
                s.name,
                s.addr + i
            );
        }
    }
    eprintln!(
        "MERCATOR: file {} bytes (RAM code {}, data {}); flash code {} at {:#x}, data {} at {:#x}",
        file.len(),
        loaded.header().code_size,
        loaded.header().data_size,
        ext.code_size,
        ext.code_addr,
        ext.ro_size,
        ext.ro_addr
    );
}
