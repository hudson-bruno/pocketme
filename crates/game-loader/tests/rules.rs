//! The loader's rules, on small made-up game files: what a good load
//! writes, and every way a file is refused.

mod common;

use board_api::FsError;
use common::*;
use game_api::{GameHeader, MAGIC, layout};
use game_loader::{GameSlots, LoadError, NOTICE_MAX, PIECE, Slots, check_file, check_header, load};

fn try_load(file: Vec<u8>) -> (Result<game_loader::Loaded, LoadError>, Card, FakeSlots) {
    try_load_into(file, Slots::CONSOLE)
}

fn try_load_into(
    file: Vec<u8>,
    slots: Slots,
) -> (Result<game_loader::Loaded, LoadError>, Card, FakeSlots) {
    let mut card = Card::new(file);
    let mut fake = FakeSlots::new(slots);
    let got = load(&mut card, PATH, &slots, &mut fake);
    (got, card, fake)
}

/// The file with its header changed and the checksum made right again,
/// so only the change itself can be refused.
fn with_header(change: impl FnOnce(&mut GameHeader)) -> Vec<u8> {
    let file = small_game();
    let mut head = GameHeader::from_bytes(file[..36].try_into().unwrap());
    change(&mut head);
    pack(head, &file[36..])
}

#[test]
fn a_good_file_lands_exactly_where_it_should() {
    let file = small_game();
    let (got, card, fake) = try_load(file.clone());
    let loaded = got.unwrap();
    assert_eq!(loaded.entry(), layout::CODE_SLOT_ADDR + 4);
    let payload = &file[36..];
    assert_eq!(&fake.code[..1000], &payload[..1000]);
    assert_eq!(&fake.data[..300], &payload[1000..1300]);
    assert!(
        fake.data[300..364].iter().all(|&b| b == 0),
        "zeroed variables"
    );
    // Nothing past the game was touched.
    assert!(fake.code[1000..].iter().all(|&b| b == RUBBISH));
    assert!(fake.data[364..].iter().all(|&b| b == RUBBISH));
    // Every code word written once and read back once.
    assert_eq!((fake.code_writes, fake.code_reads), (250, 250));
    assert_eq!(fake.zero_calls, 1);
    // Header, then pieces that end on multiples of PIECE in the file,
    // then the one-byte look past the end.
    let reads: Vec<(u32, usize)> = card
        .calls
        .iter()
        .filter_map(|c| match *c {
            Call::Read { offset, len, .. } => Some((offset, len)),
            Call::Size => None,
        })
        .collect();
    assert_eq!(
        reads,
        [(0, 36), (36, 476), (512, 512), (1024, 312), (1336, 1)]
    );
    assert_eq!(
        card.calls[1],
        Call::Size,
        "the size is asked right after the header"
    );
    assert_eq!(loaded.stats().reads, 5);
    assert_eq!(loaded.stats().bytes as usize, file.len());
}

#[test]
fn reads_are_lined_up_with_card_blocks() {
    let file = pack(header(4096, 4096, 0), &pattern(8192, 9));
    let end = file.len();
    let (got, card, _) = try_load(file);
    assert!(got.is_ok(), "{got:?}");
    for c in &card.calls[2..] {
        if let Call::Read { offset, len, .. } = *c {
            let stop = offset as usize + len;
            assert!(len <= PIECE);
            // Each piece ends on a block, or at the end of the file (and
            // the last read is the one-byte look past the end).
            assert!(
                stop.is_multiple_of(PIECE) || stop == end || offset as usize == end,
                "{offset} + {len}"
            );
        }
    }
}

#[test]
fn short_reads_still_load_the_same_bytes() {
    let file = small_game();
    let (want, _, good) = try_load(file.clone());
    let mut card = Card::new(file);
    card.max_read = Some(7);
    let mut fake = FakeSlots::console();
    let got = load(&mut card, PATH, &Slots::CONSOLE, &mut fake).unwrap();
    assert_eq!(got.header(), want.unwrap().header());
    assert_eq!((fake.code, fake.data), (good.code, good.data));
}

#[test]
fn checking_alone_reads_the_same_and_writes_nothing() {
    let file = small_game();
    let (loaded, _, _) = try_load(file.clone());
    let mut card = Card::new(file.clone());
    let stats = check_file(&mut card, PATH, &Slots::CONSOLE).unwrap();
    assert_eq!(stats, loaded.unwrap().stats());
    let mut bad = file;
    bad[700] ^= 1;
    let mut card = Card::new(bad);
    assert!(matches!(
        check_file(&mut card, PATH, &Slots::CONSOLE),
        Err(LoadError::Damaged { .. })
    ));
}

/// A refused header: nothing written, and only the header was read.
fn refused_before_writing(file: Vec<u8>) -> LoadError {
    let (got, card, fake) = try_load(file);
    let err = got.unwrap_err();
    assert_eq!(fake.touches(), 0, "{err:?}: the slots were touched");
    // Only the header was asked for (twice when the file is shorter than
    // the header: the second read says "the end").
    assert!(
        card.calls.iter().all(|c| matches!(
            c,
            Call::Read {
                offset: 0..36,
                len: 1..=36,
                ..
            }
        )),
        "{err:?}: more than the header was read: {:?}",
        card.calls
    );
    err
}

#[test]
fn not_a_game_file() {
    let mut file = small_game();
    file[0] = b'g';
    assert_eq!(refused_before_writing(file), LoadError::NotAGame);
    let text = b"NOT A GAME FILE. In the simulator this game is linked in.".to_vec();
    assert_eq!(refused_before_writing(text), LoadError::NotAGame);
    for len in 0..4 {
        assert_eq!(
            refused_before_writing(b"GAME"[..len].to_vec()),
            LoadError::NotAGame
        );
    }
}

#[test]
fn old_versions_are_refused() {
    for version in [0, 1] {
        let file = with_header(|h| h.api_version = version);
        assert_eq!(refused_before_writing(file), LoadError::TooOld { version });
    }
}

#[test]
fn a_later_version_is_let_through_as_the_contract_says() {
    // crates/game-api README: a newer game may run on an older console;
    // the game itself checks the console's tables. Whether the loader
    // should have an upper limit is a question for the owner.
    for version in [game_api::API_VERSION + 1, u32::MAX] {
        let (got, _, _) = try_load(with_header(|h| h.api_version = version));
        assert_eq!(got.unwrap().header().api_version, version);
    }
}

#[test]
fn wrong_addresses_are_refused() {
    let file = with_header(|h| {
        h.code_addr += 0x1100;
        h.entry += 0x1100;
    });
    assert_eq!(
        refused_before_writing(file),
        LoadError::OtherCodeSlot {
            addr: layout::CODE_SLOT_ADDR + 0x1100
        }
    );
    let file = with_header(|h| h.data_addr -= 4);
    assert_eq!(
        refused_before_writing(file),
        LoadError::OtherDataSlot {
            addr: layout::DATA_SLOT_ADDR - 4
        }
    );
}

#[test]
fn odd_sizes_are_refused() {
    for change in [
        |h: &mut GameHeader| h.code_size += 2,
        |h: &mut GameHeader| h.data_size += 1,
        |h: &mut GameHeader| h.bss_size += 3,
    ] {
        assert_eq!(
            refused_before_writing(with_header(change)),
            LoadError::OddSize
        );
    }
}

#[test]
fn too_big_is_refused() {
    // The code may run on past the code slot into the spill, but no
    // further (layout::CODE_MAX).
    let room = layout::CODE_MAX;
    let file = with_header(|h| h.code_size = room + 4);
    assert_eq!(
        refused_before_writing(file),
        LoadError::CodeTooBig {
            size: room + 4,
            room
        }
    );
    let file = with_header(|h| h.code_size = u32::MAX - 3);
    assert!(matches!(
        refused_before_writing(file),
        LoadError::CodeTooBig { .. }
    ));

    let room = layout::DATA_SLOT_SIZE;
    // Data alone, zeroed variables alone, and the two together.
    for (data, bss) in [(room + 4, 0), (0, room + 4), (room - 296, 300)] {
        let file = with_header(|h| (h.data_size, h.bss_size) = (data, bss));
        assert!(
            matches!(refused_before_writing(file), LoadError::DataTooBig { room: r, .. } if r == room),
            "{data} + {bss}"
        );
    }
    // Two huge sizes whose sum wraps round to a small number in 32 bits.
    let file = with_header(|h| (h.data_size, h.bss_size) = (0x8000_0000, 0x8000_0000));
    assert!(matches!(
        refused_before_writing(file),
        LoadError::DataTooBig { .. }
    ));
    // Exactly full is fine.
    let (got, _, fake) = try_load(pack(header(64, room - 64, 64), &pattern(room as usize, 1)));
    assert!(got.is_ok(), "{got:?}");
    assert!(fake.data[room as usize - 64..].iter().all(|&b| b == 0));
}

/// Slots so big that header, code and data together pass 4 GB (only a
/// test can make such slots; the chip's are 112 KB). The file could not
/// have that size, so the header is refused before anything is read
/// past it. (Second reader, step 3: before, the end of the file was
/// worked out with a saturating sum, so the size check passed for a
/// card that said u32::MAX, and the loader went on reading.)
#[test]
fn sizes_that_pass_four_gigabytes_together_are_refused() {
    let slots = Slots {
        code_addr: 0x1000_0000,
        code_size: 0xFFFF_FFFC,
        spill_size: 0,
        data_addr: 0,
        data_size: 0xFFFF_FFFC,
    };
    let mut h = header(0x8000_0000, 0x8000_0000, 0);
    h.code_addr = slots.code_addr;
    h.data_addr = slots.data_addr;
    h.entry = slots.code_addr;
    assert!(matches!(
        check_header(&h, &slots),
        Err(LoadError::DataTooBig { .. })
    ));
    let mut card = Card::new(pack(h, &[]));
    card.size_says = Some(u32::MAX);
    let mut fake = FakeSlots::new(Slots {
        code_size: 64,
        data_size: 64,
        ..slots
    });
    let got = load(&mut card, PATH, &slots, &mut fake);
    assert!(matches!(got, Err(LoadError::DataTooBig { .. })), "{got:?}");
    assert_eq!(card.calls.len(), 1, "only the header was read");
    // The largest sum that still fits is let through by the header check.
    let mut h = header(0x8000_0000, 0x7FFF_FFD8, 0);
    h.code_addr = slots.code_addr;
    h.data_addr = slots.data_addr;
    h.entry = slots.code_addr;
    assert_eq!(check_header(&h, &slots), Ok(()));
}

#[test]
fn an_entry_outside_the_code_is_refused() {
    let base = layout::CODE_SLOT_ADDR;
    for entry in [0, base - 4, base + 1000, base + 1004, u32::MAX] {
        let file = with_header(|h| h.entry = entry);
        assert_eq!(refused_before_writing(file), LoadError::BadEntry { entry });
    }
    // No code at all: no entry can be inside it.
    let file = pack(
        GameHeader {
            entry: base,
            ..header(0, 8, 0)
        },
        &pattern(8, 0),
    );
    assert_eq!(
        refused_before_writing(file),
        LoadError::BadEntry { entry: base }
    );
    // The first and the last word are inside.
    for entry in [base, base + 996] {
        let (got, _, _) = try_load(with_header(|h| h.entry = entry));
        assert_eq!(got.unwrap().entry(), entry);
    }
}

#[test]
fn a_wrong_checksum_is_found_in_header_code_and_data() {
    let good = small_game();
    let crc_at = 32;
    let places = [
        ("checksum itself", crc_at),
        ("header: zeroed size", 24),
        ("header: entry", 28),
        ("first code byte", 36),
        ("last code byte", 36 + 999),
        ("first data byte", 36 + 1000),
        ("last data byte", 36 + 1299),
    ];
    for (what, at) in places {
        let mut file = good.clone();
        // +4 keeps sizes and the entry valid, so only the checksum can
        // find it.
        file[at] = file[at].wrapping_add(4);
        let (got, _, fake) = try_load(file);
        assert!(
            matches!(got, Err(LoadError::Damaged { .. })),
            "{what}: {got:?}"
        );
        // The checksum is known only at the end: the slots were written
        // (that is allowed), but the zeroed variables were not.
        assert_eq!(fake.zero_calls, 0, "{what}");
    }
}

#[test]
fn a_file_cut_short_at_every_length_is_refused() {
    let file = small_game();
    for len in 0..file.len() {
        let (got, card, fake) = try_load(file[..len].to_vec());
        let err = got.unwrap_err();
        match len {
            0..4 => assert_eq!(err, LoadError::NotAGame),
            4..36 => assert_eq!(
                err,
                LoadError::CutShort {
                    size: len as u32,
                    want: 36
                }
            ),
            _ => assert_eq!(
                err,
                LoadError::CutShort {
                    size: len as u32,
                    want: file.len() as u32
                }
            ),
        }
        // The size gives it away before a single byte is written.
        assert_eq!(fake.touches(), 0, "{len}");
        assert!(card.calls.len() <= 2);
        assert_eq!(
            err.notice(),
            if len < 4 {
                "NOT A GAME FILE"
            } else {
                "GAME FILE CUT SHORT"
            }
        );
    }
}

#[test]
fn a_file_cut_short_while_the_card_still_says_the_full_size() {
    let file = small_game();
    for len in 36..file.len() {
        let mut card = Card::new(file[..len].to_vec());
        card.size_says = Some(file.len() as u32);
        let mut fake = FakeSlots::console();
        let got = load(&mut card, PATH, &Slots::CONSOLE, &mut fake);
        assert_eq!(got, Err(LoadError::Changed), "{len}");
        assert_eq!(fake.zero_calls, 0);
    }
}

#[test]
fn a_file_too_long_is_refused() {
    let mut file = small_game();
    file.push(0);
    assert_eq!(
        refused_before_writing_after_size(file),
        LoadError::TooLong {
            size: 1337,
            want: 1336
        }
    );
}

fn refused_before_writing_after_size(file: Vec<u8>) -> LoadError {
    let (got, card, fake) = try_load(file);
    let err = got.unwrap_err();
    assert_eq!(fake.touches(), 0, "{err:?}");
    assert_eq!(card.calls.len(), 2, "{err:?}");
    err
}

#[test]
fn a_file_that_grows_or_shrinks_between_two_reads_is_refused() {
    let file = small_game();
    // A good load reads 5 times: the header, three pieces, and a look past
    // the end. Change the file before each of reads 1 to 4.
    for after in 1..5 {
        let mut grown = file.clone();
        grown.extend_from_slice(&[1, 2, 3, 4]);
        let mut shrunk = file.clone();
        shrunk.truncate(file.len() - 4);
        for (what, new) in [("grows", grown), ("shrinks", shrunk)] {
            if what == "shrinks" && after == 4 {
                // Every byte was already read: the game that was checked
                // is the one in the slots. Shrinking after that is fine.
                continue;
            }
            let mut card = Card::new(file.clone());
            card.change = Some((after, new));
            let mut fake = FakeSlots::console();
            let got = load(&mut card, PATH, &Slots::CONSOLE, &mut fake);
            assert_eq!(got, Err(LoadError::Changed), "{what} after read {after}");
            assert_eq!(fake.zero_calls, 0);
        }
    }
}

#[test]
fn a_file_swapped_for_another_good_game_half_way_is_refused() {
    let file = small_game();
    let other = pack(header(1000, 300, 64), &pattern(1300, 99));
    // Before reads 1 to 3 (the pieces). Read 4 only looks past the end.
    for after in 1..4 {
        let mut card = Card::new(file.clone());
        card.change = Some((after, other.clone()));
        let got = load(&mut card, PATH, &Slots::CONSOLE, &mut FakeSlots::console());
        // Whatever mix of the two games was read, it does not match this
        // header's checksum (which covers the header too).
        assert!(matches!(got, Err(LoadError::Damaged { .. })), "{after}");
    }
}

#[test]
fn a_read_error_at_every_call_is_reported() {
    let file = small_game();
    let calls = {
        let (_, card, _) = try_load(file.clone());
        card.calls.len()
    };
    assert_eq!(calls, 6);
    for n in 0..calls {
        let mut card = Card::new(file.clone());
        card.fail_call = Some(n);
        let mut fake = FakeSlots::console();
        let got = load(&mut card, PATH, &Slots::CONSOLE, &mut fake);
        assert_eq!(got, Err(LoadError::Card(FsError::Io)), "call {n}");
        assert_eq!(got.unwrap_err().notice(), "CARD READ ERROR");
        assert_eq!(fake.zero_calls, 0);
    }
}

#[test]
fn a_missing_file_says_so() {
    let mut card = Card::new(small_game());
    let got = load(
        &mut card,
        "/CONSOLE/GAMES/NOPE/GAME.BIN",
        &Slots::CONSOLE,
        &mut FakeSlots::console(),
    );
    assert_eq!(got, Err(LoadError::Card(FsError::NotFound)));
    assert_eq!(got.unwrap_err().notice(), "GAME NOT FOUND");
}

#[test]
fn a_board_that_claims_too_many_bytes_is_not_trusted() {
    let mut card = Card::new(small_game());
    card.overclaim = true;
    let got = load(&mut card, PATH, &Slots::CONSOLE, &mut FakeSlots::console());
    assert_eq!(got, Err(LoadError::Card(FsError::Io)));
}

#[test]
fn a_code_slot_that_does_not_keep_its_words_is_reported() {
    let mut card = Card::new(small_game());
    let mut fake = FakeSlots::console();
    fake.broken_at = Some(520);
    let got = load(&mut card, PATH, &Slots::CONSOLE, &mut fake);
    assert!(
        matches!(got, Err(LoadError::CodeSlotFailed { offset: 520, .. })),
        "{got:?}"
    );
    assert_eq!(got.unwrap_err().notice(), "CODE SLOT WRITE FAILED");
}

#[test]
fn after_a_failed_load_the_next_one_works() {
    let good = small_game();
    let mut bad = good.clone();
    bad[36 + 1200] ^= 0x40;
    let (clean, _, clean_slots) = try_load(good.clone());
    let clean = clean.unwrap();

    // One pair of slots for all three loads.
    let mut fake = FakeSlots::console();
    let other = pack(header(2000, 600, 200), &pattern(2600, 50));
    assert!(load(&mut Card::new(other), PATH, &Slots::CONSOLE, &mut fake).is_ok());
    assert!(load(&mut Card::new(bad), PATH, &Slots::CONSOLE, &mut fake).is_err());
    let again = load(&mut Card::new(good), PATH, &Slots::CONSOLE, &mut fake).unwrap();
    assert_eq!(again, clean);
    // Every byte the game uses is as after a load into clean slots.
    assert_eq!(fake.code[..1000], clean_slots.code[..1000]);
    assert_eq!(fake.data[..364], clean_slots.data[..364]);
}

/// The code slot was 64 KB at 0x4009_0000 until 2026-11-17; the owner
/// then shrank it to 56 KB at 0x4009_2000 (game_api::layout) so the
/// Bluetooth radio's RAM code fits below it. Games built for one layout
/// must be refused by the other, with a clear message.
#[test]
fn a_game_for_another_slot_layout_is_refused() {
    let old = Slots {
        code_addr: 0x4009_0000,
        code_size: 65_536,
        spill_size: 0,
        ..Slots::CONSOLE
    };
    assert_ne!(old, Slots::CONSOLE, "the old layout is not today's");
    // Built for today's console, loaded by an old console.
    let (got, _, fake) = try_load_into(small_game(), old);
    let err = got.unwrap_err();
    assert_eq!(
        err,
        LoadError::OtherCodeSlot {
            addr: layout::CODE_SLOT_ADDR
        }
    );
    assert_eq!(err.notice(), "GAME FOR OTHER CODE SLOT");
    assert_eq!(fake.touches(), 0);

    // Built for the old console, loaded by today's.
    let mut head = header(1000, 300, 64);
    head.code_addr = old.code_addr;
    head.entry = old.code_addr + 8;
    let old_game = pack(head, &pattern(1300, 5));
    let (got, _, fake) = try_load(old_game.clone());
    assert_eq!(
        got.unwrap_err(),
        LoadError::OtherCodeSlot { addr: 0x4009_0000 }
    );
    assert_eq!(fake.touches(), 0);
    // ...and it loads on the console it was built for.
    let (got, _, fake) = try_load_into(old_game, old);
    assert!(got.is_ok(), "{got:?}");
    assert_eq!(fake.code.len(), 65_536);

    // The slot shrunk from the top (same start, 4,352 bytes less room):
    // a game that still fits loads; one with more code than the new slot
    // is refused. Counted from today's slot size,
    // so this test still holds if game_api::layout changes that size.
    // (Second reader, step 3: it said 61,184 here and failed when the
    // console's own slot was made that small in a test copy.)
    // A console with no spill (as before stage 1), so the code slot is
    // all the room there is.
    let room = layout::CODE_SLOT_SIZE - 4_352;
    let shrunk = Slots {
        code_size: room,
        spill_size: 0,
        ..Slots::CONSOLE
    };
    let big = pack(header(room + 4, 4, 0), &pattern(room as usize + 8, 1));
    let (got, _, fake) = try_load_into(big.clone(), shrunk);
    let err = got.unwrap_err();
    assert_eq!(
        err,
        LoadError::CodeTooBig {
            size: room + 4,
            room
        }
    );
    assert_eq!(err.notice(), "GAME CODE TOO BIG");
    assert_eq!(fake.touches(), 0);
    let (got, _, _) = try_load(big);
    assert!(got.is_ok(), "{got:?}");

    // The data slot moved or shrunk.
    let moved = Slots {
        data_addr: layout::DATA_SLOT_ADDR + 0x1000,
        data_size: layout::DATA_SLOT_SIZE - 0x1000,
        ..Slots::CONSOLE
    };
    let (got, _, _) = try_load_into(small_game(), moved);
    assert_eq!(got.unwrap_err().notice(), "GAME FOR OTHER DATA SLOT");
    let small = Slots {
        data_size: 360,
        ..Slots::CONSOLE
    };
    let (got, _, _) = try_load_into(small_game(), small);
    assert_eq!(
        got.unwrap_err(),
        LoadError::DataTooBig {
            size: 364,
            room: 360
        }
    );
}

/// Today's slot sizes come from game_api::layout and nowhere else.
#[test]
fn the_console_slots_are_the_layout() {
    let s = Slots::CONSOLE;
    assert_eq!(
        (
            s.code_addr,
            s.code_size,
            s.spill_size,
            s.data_addr,
            s.data_size
        ),
        (
            layout::CODE_SLOT_ADDR,
            layout::CODE_SLOT_SIZE,
            layout::CODE_SPILL_MAX,
            layout::DATA_SLOT_ADDR,
            layout::DATA_SLOT_SIZE
        )
    );
    // The spill follows the code slot directly.
    assert_eq!(s.code_addr + s.code_size, layout::CODE_SPILL_ADDR);
}

/// The loader's header check and game-api's own `validate` agree on
/// every header, for the console's slots: one says yes exactly when the
/// other does, and they give the same reason when code and data meet.
#[test]
fn the_header_check_agrees_with_game_api() {
    let mut rng = Rng::new(0x5EED_0001);
    let mut yes = 0;
    let mut spill_yes = 0;
    let mut meet = 0;
    let spill = 24 * 1024;
    for _ in 0..200_000 {
        let mut h = header(1000, 300, 64);
        // Change a few fields to values near the edges.
        for _ in 0..1 + rng.below(3) {
            let near = |rng: &mut Rng, v: u32| match rng.below(4) {
                0 => v,
                1 => v.wrapping_add(rng.below(9) as u32).wrapping_sub(4),
                2 => rng.next() as u32,
                _ => [0, 4, u32::MAX, 0x8000_0000][rng.below(4)],
            };
            match rng.below(8) {
                0 => h.magic = near(&mut rng, MAGIC),
                1 => h.api_version = near(&mut rng, 2),
                2 => h.code_addr = near(&mut rng, layout::CODE_SLOT_ADDR),
                3 => {
                    // The end of the slot, a spill of 24 KB, the end of
                    // the spill.
                    let edge = [
                        layout::CODE_SLOT_SIZE,
                        layout::CODE_SLOT_SIZE + spill,
                        layout::CODE_MAX,
                    ][rng.below(3)];
                    h.code_size = near(&mut rng, edge)
                }
                4 => h.data_addr = near(&mut rng, layout::DATA_SLOT_ADDR),
                5 => {
                    // Full without a spill, full beside a 24 KB spill.
                    let edge = [
                        layout::DATA_SLOT_SIZE - 64,
                        layout::DATA_SLOT_SIZE - spill - 64,
                    ][rng.below(2)];
                    h.data_size = near(&mut rng, edge)
                }
                6 => h.bss_size = near(&mut rng, 64),
                _ => h.entry = near(&mut rng, layout::CODE_SLOT_ADDR + 996),
            }
        }
        let ours = check_header(&h, &Slots::CONSOLE);
        assert_eq!(
            ours.is_ok(),
            h.validate().is_ok(),
            "{h:?}: {ours:?} vs {:?}",
            h.validate()
        );
        assert_eq!(
            matches!(ours, Err(LoadError::CodeMeetsData { .. })),
            h.validate() == Err(game_api::HeaderError::CodeMeetsData),
            "{h:?}: {ours:?} vs {:?}",
            h.validate()
        );
        yes += ours.is_ok() as u32;
        spill_yes += (ours.is_ok() && h.code_size > layout::CODE_SLOT_SIZE) as u32;
        meet += matches!(ours, Err(LoadError::CodeMeetsData { .. })) as u32;
    }
    assert!(yes > 10_000, "too few good headers to mean anything: {yes}");
    assert!(spill_yes > 1_000, "too few good spill headers: {spill_yes}");
    assert!(
        meet > 1_000,
        "too few headers where code meets data: {meet}"
    );
}

#[test]
fn every_notice_is_short_and_in_capitals() {
    let all = [
        LoadError::Card(FsError::NoCard),
        LoadError::Card(FsError::NotFound),
        LoadError::Card(FsError::BadName),
        LoadError::Card(FsError::Io),
        LoadError::Card(FsError::Unsupported),
        LoadError::NotAGame,
        LoadError::TooOld { version: 1 },
        LoadError::OtherCodeSlot { addr: 0 },
        LoadError::OtherDataSlot { addr: 0 },
        LoadError::OddSize,
        LoadError::CodeTooBig { size: 0, room: 0 },
        LoadError::DataTooBig { size: 0, room: 0 },
        LoadError::CodeMeetsData {
            spill: 0,
            data: 0,
            room: 0,
        },
        LoadError::BadEntry { entry: 0 },
        LoadError::CutShort { size: 0, want: 0 },
        LoadError::TooLong { size: 0, want: 0 },
        LoadError::Changed,
        LoadError::Damaged {
            in_file: 0,
            computed: 0,
        },
        LoadError::CodeSlotFailed {
            offset: 0,
            wrote: 0,
            read: 0,
        },
        LoadError::CodeOverwritten { spill: 0 },
        LoadError::BadFlashPart,
        LoadError::FlashPartMissing,
        LoadError::FlashPartDiffers {
            got_code: 0,
            got_ro: 0,
        },
    ];
    let mut seen = std::collections::HashSet::new();
    for e in all {
        let t = e.notice();
        assert!(t.len() <= NOTICE_MAX, "{t}");
        assert!(
            t.bytes().all(|b| b.is_ascii_uppercase() || b == b' '),
            "{t}"
        );
        assert!(seen.insert(t), "two errors share the text {t}");
    }
    // The console's notice panel takes 32 letters (console-core
    // notify::NOTICE_LEN); these stay well under it.
    const { assert!(NOTICE_MAX <= 32) };
}

#[test]
#[should_panic(expected = "byte access to the code slot")]
fn the_fake_code_slot_refuses_a_byte_access() {
    FakeSlots::console().write_code_word(2, 0);
}

#[test]
#[should_panic(expected = "outside the code slot")]
fn the_fake_code_slot_refuses_a_word_past_its_end() {
    FakeSlots::console().read_code_word(layout::CODE_MAX);
}

#[test]
#[should_panic(expected = "outside the code slot")]
fn the_fake_code_slot_without_a_spill_ends_at_the_slot() {
    FakeSlots::new(Slots {
        spill_size: 0,
        ..Slots::CONSOLE
    })
    .read_code_word(layout::CODE_SLOT_SIZE);
}

#[test]
#[should_panic(expected = "data over code")]
fn the_strict_fake_refuses_data_over_code() {
    let mut fake = FakeSlots::console();
    fake.write_code_word(0, 1);
    fake.write_code_word(layout::CODE_SLOT_SIZE, 2);
    fake.zero_data(layout::DATA_SLOT_SIZE - 8, 8);
}

#[test]
#[should_panic(expected = "outside the data slot")]
fn the_fake_data_slot_refuses_bytes_past_its_end() {
    FakeSlots::console().zero_data(layout::DATA_SLOT_SIZE - 4, 8);
}

#[test]
fn a_loaded_game_starts_only_from_a_good_load() {
    // The only way to get an entry address is `Loaded`, and `load`
    // gives one only on success. This test just pins that a damaged
    // file gives no `Loaded` even though its bytes went into the slots.
    let mut file = small_game();
    file[40] ^= 0x80;
    let (got, _, fake) = try_load(file);
    assert!(got.is_err());
    assert!(
        fake.code_writes > 0,
        "the code was written before the checksum was known"
    );
}

// Code past the code slot ("the spill", stage 1). On the chip the spill
// is the top of the data slot seen from the instruction bus, in reverse
// word order; the fake stores it there too (tests/common, `Mirror`).

const SLOT: u32 = layout::CODE_SLOT_SIZE;
const ROOM: u32 = layout::DATA_SLOT_SIZE;

/// A game with `code` bytes of code (more than the slot: a spill),
/// `data` bytes of data and `bss` zeroed, and its payload.
fn spill_game(code: u32, data: u32, bss: u32) -> (Vec<u8>, Vec<u8>) {
    let payload = pattern((code + data) as usize, 11);
    (pack(header(code, data, bss), &payload), payload)
}

/// Data and zeroed bytes that add up to `total`, split about in half.
fn split(total: u32) -> (u32, u32) {
    let data = total / 8 * 4;
    (data, total - data)
}

#[test]
fn a_game_with_code_past_the_slot_lands_on_the_top_of_the_data_slot() {
    let (file, payload) = spill_game(SLOT + 8, 300, 64);
    let (got, _, fake) = try_load(file);
    let loaded = got.unwrap();
    assert_eq!(loaded.header().code_size, 57_352);
    // The code slot is full.
    assert_eq!(&fake.code[..], &payload[..SLOT as usize]);
    // The first word past the slot is the data slot's LAST word, the
    // second word the one before it; the bytes inside a word in order.
    let room = ROOM as usize;
    let code = SLOT as usize;
    assert_eq!(&fake.data[room - 4..], &payload[code..code + 4]);
    assert_eq!(&fake.data[room - 8..room - 4], &payload[code + 4..code + 8]);
    assert_eq!(fake.code_bytes_in_data(), 8);
    // Through the instruction bus, the spill reads as the code.
    for i in 0..8 {
        assert_eq!(
            fake.byte_at(layout::CODE_SPILL_ADDR + i),
            Some(payload[code + i as usize])
        );
    }
    // The data at the bottom, the zeroes after it, rubbish in between.
    assert_eq!(&fake.data[..300], &payload[code + 8..]);
    assert!(fake.data[300..364].iter().all(|&b| b == 0));
    assert!(fake.data[364..room - 8].iter().all(|&b| b == RUBBISH));
    // Every word written once and read back once, and the two spill
    // words read once more at the end (step 6b).
    let words = (SLOT as usize + 8) / 4;
    assert_eq!((fake.code_writes, fake.code_reads), (words, words + 2));
}

#[test]
fn code_and_data_that_would_meet_are_refused_before_writing() {
    for spill in [4, 1_024, 24_576, 49_148] {
        // One word too many, as data, as zeroed, or split.
        let total = ROOM - spill + 4;
        for (data, bss) in [(total, 0), (0, total), split(total)] {
            let (file, _) = spill_game(SLOT + spill, data, bss);
            let err = refused_before_writing(file);
            assert_eq!(
                err,
                LoadError::CodeMeetsData {
                    spill,
                    data: total,
                    room: ROOM
                },
                "spill {spill}, data {data}, zeroed {bss}"
            );
            assert_eq!(err.notice(), "GAME TOO BIG FOR SLOTS");
        }
    }
}

#[test]
fn code_and_data_that_exactly_fill_the_room_load() {
    for spill in [0, 4, 1_024, 24_576, 49_148, 49_152] {
        let (data, bss) = split(ROOM - spill);
        let (file, payload) = spill_game(SLOT + spill, data, bss);
        // The strict fake panics if one data or zero byte lands on code.
        let (got, _, mut fake) = try_load(file);
        assert!(got.is_ok(), "spill {spill}: {got:?}");
        assert_eq!(fake.code_bytes_in_data(), spill as usize);
        // Every code word, in the slot and in the spill, reads back.
        let (words, _) = payload[..(SLOT + spill) as usize].as_chunks::<4>();
        for (i, &word) in words.iter().enumerate() {
            let want = u32::from_le_bytes(word);
            assert_eq!(fake.read_code_word(i as u32 * 4), want, "spill {spill}");
        }
        // The data and the zeroes are whole too.
        let at = (SLOT + spill) as usize;
        assert_eq!(&fake.data[..data as usize], &payload[at..]);
        assert!(
            fake.data[data as usize..(data + bss) as usize]
                .iter()
                .all(|&b| b == 0)
        );
    }
}

#[test]
fn code_over_slot_and_spill_is_refused() {
    let (file, _) = spill_game(106_500, 0, 0);
    let err = refused_before_writing(file);
    assert_eq!(
        err,
        LoadError::CodeTooBig {
            size: 106_500,
            room: 106_496
        }
    );
    assert_eq!(err.notice(), "GAME CODE TOO BIG");
    // The whole spill is allowed, but then not one byte of data.
    let (file, _) = spill_game(layout::CODE_MAX, 0, 4);
    assert!(matches!(
        refused_before_writing(file),
        LoadError::CodeMeetsData { .. }
    ));
}

#[test]
fn an_old_console_refuses_a_spill_game_cleanly() {
    // Before stage 1 (or when the start-up mirror check failed) a console
    // has no spill.
    let old = Slots {
        spill_size: 0,
        ..Slots::CONSOLE
    };
    let (file, _) = spill_game(SLOT + 4, 300, 64);
    let (got, card, fake) = try_load_into(file, old);
    let err = got.unwrap_err();
    assert_eq!(
        err,
        LoadError::CodeTooBig {
            size: SLOT + 4,
            room: 57_344
        }
    );
    assert_eq!(err.notice(), "GAME CODE TOO BIG");
    assert_eq!(fake.touches(), 0);
    assert_eq!(card.calls.len(), 1, "only the header was read");
    // A game that fits in the slot still loads there.
    let (file, _) = spill_game(SLOT, 300, 64);
    let (got, _, _) = try_load_into(file, old);
    assert!(got.is_ok(), "{got:?}");
}

#[test]
fn an_old_game_leaves_the_top_of_the_data_slot_alone() {
    // A small game, and one whose code fills the slot exactly.
    for code in [1000, SLOT] {
        let (file, _) = spill_game(code, 300, 64);
        let (got, _, fake) = try_load(file);
        assert!(got.is_ok(), "{code}: {got:?}");
        assert!(
            fake.data[364..].iter().all(|&b| b == RUBBISH),
            "{code}: something was written past the zeroed bytes"
        );
        assert_eq!(fake.code_bytes_in_data(), 0);
        // No spill, so no second read of it (step 6b).
        assert_eq!(fake.code_reads, fake.code_writes);
    }
}

#[test]
fn an_entry_in_the_spill_is_accepted_and_past_the_code_refused() {
    let code = SLOT + 1_024;
    let (_, payload) = spill_game(code, 300, 64);
    let entry_file = |entry: u32| {
        let mut h = header(code, 300, 64);
        h.entry = entry;
        pack(h, &payload)
    };
    for entry in [layout::CODE_SPILL_ADDR, layout::CODE_SPILL_ADDR + 1_020] {
        let (got, _, _) = try_load(entry_file(entry));
        assert_eq!(got.unwrap().entry(), entry);
    }
    let past = layout::CODE_SPILL_ADDR + 1_024;
    assert_eq!(
        refused_before_writing(entry_file(past)),
        LoadError::BadEntry { entry: past }
    );
}

#[test]
fn huge_sizes_with_a_spill_do_not_wrap() {
    let s = Slots::CONSOLE;
    let h = |code, data, bss| header(code, data, bss);
    assert!(matches!(
        check_header(&h(0xFFFF_FFFC, 0, 0), &s),
        Err(LoadError::CodeTooBig { .. })
    ));
    for (data, bss) in [
        (0xFFFF_FFFC, 0),
        (0, 0xFFFF_FFFC),
        (0x8000_0000, 0x8000_0000),
    ] {
        assert!(
            matches!(
                check_header(&h(SLOT + 8, data, bss), &s),
                Err(LoadError::DataTooBig { .. })
            ),
            "{data} + {bss}"
        );
    }
    // Made-up slots with a spill near 4 GB: spill + data must not wrap
    // round to a small number in 32 bits.
    let big = Slots {
        code_addr: 0x1000_0000,
        code_size: 4,
        spill_size: u32::MAX,
        data_addr: 0,
        data_size: 0xFFFF_FFF0,
    };
    let mut head = h(0xFFFF_FFF4, 0x20, 0);
    head.code_addr = big.code_addr;
    head.data_addr = big.data_addr;
    head.entry = big.code_addr;
    assert_eq!(
        check_header(&head, &big),
        Err(LoadError::CodeMeetsData {
            spill: 0xFFFF_FFF0,
            data: 0x20,
            room: 0xFFFF_FFF0
        })
    );
    // The room for code is the slot and at most the data slot, added
    // without wrapping.
    head.code_size = u32::MAX - 3;
    head.data_size = 0;
    assert_eq!(
        check_header(&head, &big),
        Err(LoadError::CodeTooBig {
            size: u32::MAX - 3,
            room: 0xFFFF_FFF4
        })
    );
}

#[test]
fn a_spill_overwritten_after_the_copy_is_found() {
    let (file, _) = spill_game(SLOT + 1_024, 300, 64);
    // Something changes one spill word after the zeroing.
    let mut card = Card::new(file.clone());
    let mut fake = FakeSlots::console();
    fake.spoil_after_zero = Some(SLOT + 512);
    let got = load(&mut card, PATH, &Slots::CONSOLE, &mut fake);
    assert_eq!(got, Err(LoadError::CodeOverwritten { spill: 1_024 }));
    assert_eq!(got.unwrap_err().notice(), "GAME CODE OVERWRITTEN");

    // A board whose mirror is straight, not reversed: the spill is the
    // BOTTOM of the data slot, and the data is written over it. Each code
    // word still read back right at once; only the check at the end
    // finds it.
    let mut card = Card::new(file.clone());
    let mut fake = FakeSlots::console();
    fake.mirror = Mirror::Straight;
    fake.strict = false;
    let got = load(&mut card, PATH, &Slots::CONSOLE, &mut fake);
    assert_eq!(got, Err(LoadError::CodeOverwritten { spill: 1_024 }));

    // Checking alone writes nothing, so it has nothing to check again.
    let mut card = Card::new(file);
    assert!(check_file(&mut card, PATH, &Slots::CONSOLE).is_ok());
}

#[test]
fn the_fake_mirror_is_the_layouts_rule() {
    // The fake writes spill words where its own copy of the rule says;
    // that copy must be game_api::layout's, word for word.
    let mut fake = FakeSlots::console();
    let slot = layout::CODE_SLOT_SIZE;
    for k in (0..layout::CODE_SPILL_MAX).step_by(4) {
        fake.write_code_word(slot + k, k ^ 0x5A5A_0000);
    }
    for k in (0..layout::CODE_SPILL_MAX).step_by(4) {
        let d = layout::sram1_iram_to_dram(layout::CODE_SPILL_ADDR + k);
        let at = (d - layout::DATA_SLOT_ADDR) as usize;
        let word = u32::from_le_bytes(fake.data[at..at + 4].try_into().unwrap());
        assert_eq!(word, k ^ 0x5A5A_0000, "spill word {k}");
    }
}

#[test]
fn the_room_for_code_and_data_follows_the_layout() {
    let s = Slots::CONSOLE;
    // Code: the slot and the spill; the spill never more than the data
    // slot, also on made-up slots.
    assert_eq!(s.code_room(), u64::from(layout::CODE_MAX));
    let small = Slots {
        spill_size: 4_096,
        ..s
    };
    assert_eq!(small.code_room(), u64::from(SLOT + 4_096));
    let off = Slots { spill_size: 0, ..s };
    assert_eq!(off.code_room(), u64::from(SLOT));
    let wide = Slots {
        code_size: u32::MAX,
        spill_size: u32::MAX,
        ..s
    };
    assert_eq!(
        wide.code_room(),
        u64::from(u32::MAX) + u64::from(s.data_size)
    );

    // Data: all of the slot while the code stays in the code slot.
    assert_eq!(s.data_room(0), s.data_size);
    assert_eq!(s.data_room(SLOT), s.data_size);
    // Code in the spill takes the data slot from the top: the room ends
    // exactly at the data word the layout maps the last code word to.
    for k in [4, 4_096, 9_344, 40_560, layout::CODE_SPILL_MAX - 4] {
        let room = s.data_room(SLOT + k + 4);
        let last = layout::sram1_iram_to_dram(layout::CODE_SPILL_ADDR + k);
        assert_eq!(room, last - layout::DATA_SLOT_ADDR, "spill {k}");
        assert!(layout::fits(SLOT + k + 4, room, 0));
        assert!(!layout::fits(SLOT + k + 4, room + 4, 0));
    }
    // All of the spill used: no room left, and never below 0.
    assert_eq!(s.data_room(layout::CODE_MAX), 0);
    assert_eq!(s.data_room(u32::MAX), 0);
}

// Games with a flash part ("GAMF").

fn load_with(
    file: Vec<u8>,
    mut fake: FakeSlots,
) -> (Result<game_loader::Loaded, LoadError>, FakeSlots) {
    let mut card = Card::new(file);
    let got = load(&mut card, PATH, &Slots::CONSOLE, &mut fake);
    (got, fake)
}

#[test]
fn a_flash_game_is_refused_by_a_board_without_flash_parts() {
    // The trait's default answer, before any slot is touched.
    let (got, fake) = load_with(small_flash_game(), FakeSlots::console());
    assert_eq!(got, Err(LoadError::FlashPartMissing));
    assert_eq!(fake.flash_asks, 1);
    assert_eq!(fake.touches(), 0);
    assert_eq!(LoadError::FlashPartMissing.notice(), "FLASH PART MISSING");

    // A board that only has the four required methods says the same.
    struct Plain(FakeSlots);
    impl GameSlots for Plain {
        fn write_code_word(&mut self, o: u32, w: u32) {
            self.0.write_code_word(o, w)
        }
        fn read_code_word(&mut self, o: u32) -> u32 {
            self.0.read_code_word(o)
        }
        fn write_data(&mut self, o: u32, b: &[u8]) {
            self.0.write_data(o, b)
        }
        fn zero_data(&mut self, o: u32, l: u32) {
            self.0.zero_data(o, l)
        }
    }
    let mut plain = Plain(holding_slots());
    let mut card = Card::new(small_flash_game());
    assert_eq!(
        load(&mut card, PATH, &Slots::CONSOLE, &mut plain),
        Err(LoadError::FlashPartMissing)
    );
    assert_eq!(plain.0.touches(), 0);
}

#[test]
fn a_flash_game_whose_part_differs_is_refused_before_any_write() {
    let mut fake = holding_slots();
    fake.holds.as_mut().unwrap().1[17] ^= 0x40;
    let (got, fake) = load_with(small_flash_game(), fake);
    let Err(LoadError::FlashPartDiffers { got_code, got_ro }) = got else {
        panic!("{got:?}")
    };
    let (code, ro) = small_flash_halves();
    assert_ne!(got_code, crc_of(&code));
    assert_eq!(got_ro, crc_of(&ro));
    assert_eq!(fake.touches(), 0);

    // A part at other addresses or of another size is missing, not
    // different.
    let mut moved = holding_slots();
    moved.holds.as_mut().unwrap().0.code_addr += 32;
    assert_eq!(
        load_with(small_flash_game(), moved).0,
        Err(LoadError::FlashPartMissing)
    );
    let mut longer = holding_slots();
    longer.holds.as_mut().unwrap().2.extend_from_slice(&[0; 4]);
    assert_eq!(
        load_with(small_flash_game(), longer).0,
        Err(LoadError::FlashPartMissing)
    );
}

#[test]
fn a_flash_game_held_exactly_loads_like_a_plain_one() {
    let file = small_flash_game();
    let (got, fake) = load_with(file.clone(), holding_slots());
    let loaded = got.unwrap();
    let (code, ro) = small_flash_halves();
    assert_eq!(loaded.flash_part(), Some(&flash_ext(&code, &ro)));
    assert_eq!(loaded.entry(), layout::CODE_SLOT_ADDR + 4);
    // The payload starts after the extension, and lands as a plain one.
    let payload = &file[68..];
    assert_eq!(&fake.code[..1000], &payload[..1000]);
    assert_eq!(&fake.data[..300], &payload[1000..1300]);
    assert!(fake.data[300..364].iter().all(|&b| b == 0));
    assert_eq!(
        (fake.code_writes, fake.code_reads, fake.zero_calls),
        (250, 250, 1)
    );
    assert_eq!(fake.flash_asks, 1);

    // The plain file with the same RAM part loads the same slots, and
    // has no flash part.
    let (plain, plain_fake) = load_with(small_game(), FakeSlots::console());
    assert_eq!(plain.unwrap().flash_part(), None);
    assert_eq!(plain_fake.code, fake.code);
    assert_eq!(plain_fake.data, fake.data);
    assert_eq!(plain_fake.flash_asks, 0, "a plain file never asks");
}

#[test]
fn the_checksum_covers_the_flash_extension() {
    // Change one CRC in the extension, and let the board hold exactly
    // what the changed extension names: only the file's checksum can
    // catch it.
    let mut file = small_flash_game();
    file[48] ^= 0x01; // code_crc32
    let ext = game_api::FlashPartHeader::from_bytes(file[36..68].try_into().unwrap());
    let mut fake = holding_slots();
    fake.holds.as_mut().unwrap().0 = ext;
    // A board that holds what the extension names (it checks the
    // addresses and sizes, and trusts the CRCs).
    struct Trusting(FakeSlots);
    impl GameSlots for Trusting {
        fn write_code_word(&mut self, o: u32, w: u32) {
            self.0.write_code_word(o, w)
        }
        fn read_code_word(&mut self, o: u32) -> u32 {
            self.0.read_code_word(o)
        }
        fn write_data(&mut self, o: u32, b: &[u8]) {
            self.0.write_data(o, b)
        }
        fn zero_data(&mut self, o: u32, l: u32) {
            self.0.zero_data(o, l)
        }
        fn flash_part(&mut self, part: &game_api::FlashPartHeader) -> game_loader::FlashHeld {
            assert_eq!(Some(part), self.0.holds.as_ref().map(|h| &h.0));
            game_loader::FlashHeld::Held
        }
    }
    let mut t = Trusting(fake);
    let mut card = Card::new(file);
    assert!(matches!(
        load(&mut card, PATH, &Slots::CONSOLE, &mut t),
        Err(LoadError::Damaged { .. })
    ));
}

#[test]
fn a_bad_flash_extension_is_refused_before_any_write() {
    let (code, ro) = small_flash_halves();
    let good = flash_ext(&code, &ro);
    let bad = [
        game_api::FlashPartHeader {
            reserved: 1,
            ..good
        },
        game_api::FlashPartHeader {
            ext_size: 28,
            ..good
        },
        game_api::FlashPartHeader {
            code_addr: layout::CODE_SLOT_ADDR,
            ..good
        },
        game_api::FlashPartHeader {
            ro_addr: layout::FLASH_RO_ADDR + layout::FLASH_RO_SIZE,
            ..good
        },
        game_api::FlashPartHeader {
            code_size: 0,
            ..good
        },
    ];
    for ext in bad {
        let file = pack_flash(header(1000, 300, 64), &ext, &pattern(1300, 3));
        let (got, fake) = load_with(file, holding_slots());
        assert_eq!(got, Err(LoadError::BadFlashPart), "{ext:?}");
        assert_eq!((fake.touches(), fake.flash_asks), (0, 0));
    }
    assert_eq!(LoadError::BadFlashPart.notice(), "BAD FLASH PART IN FILE");
}

#[test]
fn a_flash_file_cut_short_is_refused() {
    let file = small_flash_game();
    for len in [40, 67] {
        let (got, fake) = load_with(file[..len].to_vec(), holding_slots());
        assert_eq!(
            got,
            Err(LoadError::CutShort {
                size: len as u32,
                want: 68
            })
        );
        assert_eq!((fake.touches(), fake.flash_asks), (0, 0));
    }
    // Cut in the payload: the size check, still before the board is asked.
    let (got, fake) = load_with(file[..file.len() - 4].to_vec(), holding_slots());
    assert!(matches!(got, Err(LoadError::CutShort { .. })), "{got:?}");
    assert_eq!((fake.touches(), fake.flash_asks), (0, 0));
    // One byte too many.
    let mut long = file.clone();
    long.push(0);
    assert!(matches!(
        load_with(long, holding_slots()).0,
        Err(LoadError::TooLong { .. })
    ));
}

#[test]
fn check_file_reads_a_flash_file_without_asking_a_board() {
    let mut card = Card::new(small_flash_game());
    assert!(check_file(&mut card, PATH, &Slots::CONSOLE).is_ok());
    let mut file = small_flash_game();
    file[100] ^= 1;
    let mut card = Card::new(file);
    assert!(matches!(
        check_file(&mut card, PATH, &Slots::CONSOLE),
        Err(LoadError::Damaged { .. })
    ));
}

#[test]
fn a_flash_header_follows_the_plain_rules_for_its_ram_part() {
    let (code, ro) = small_flash_halves();
    let mut head = header(1000, 300, 64);
    head.entry = layout::FLASH_CODE_ADDR; // the entry may not be in flash
    let file = pack_flash(head, &flash_ext(&code, &ro), &pattern(1300, 3));
    let (got, fake) = load_with(file, holding_slots());
    assert_eq!(
        got,
        Err(LoadError::BadEntry {
            entry: layout::FLASH_CODE_ADDR
        })
    );
    assert_eq!(fake.touches(), 0);
    let mut h = header(1000, 300, 64);
    h.magic = game_api::MAGIC_FLASH;
    assert_eq!(check_header(&h, &Slots::CONSOLE), Ok(()));
    h.magic = u32::from_le_bytes(*b"GAMX");
    assert_eq!(check_header(&h, &Slots::CONSOLE), Err(LoadError::NotAGame));
}
