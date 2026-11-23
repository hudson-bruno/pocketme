//! The shared room of stage 1, swept: code past the code slot and the
//! game's data share the data slot. 20,000 seeded random sizes near the
//! edges: the loader says yes exactly when the rule says yes, and the
//! good ones load into the strict pretend slots (tests/common), which
//! panic if one data or zero byte lands on code.
//!
//! The rule is written out here again on purpose, from the slot sizes
//! alone, not with `game_api::layout::fits`: a second way to say it.
//!
//! Loading is slow in a debug build (a full file is about 104 KB), so
//! by default only 300 of the good cases are loaded, spread over the
//! sweep. `SHARED_LOADS=all cargo test --test shared` loads every one
//! (9,322; 36 seconds in a debug build, measured on the PC).

mod common;

use common::*;
use game_api::layout;
use game_loader::{LoadError, Slots, check_header, load};

const SLOT: u32 = layout::CODE_SLOT_SIZE;
const ROOM: u32 = layout::DATA_SLOT_SIZE;

/// The rule, in 64 bits: code at most the slot and the whole data slot,
/// and code past the slot + data + zeroed at most the data slot.
fn rule(code: u32, data: u32, bss: u32) -> bool {
    let spill = u64::from(code).saturating_sub(u64::from(SLOT));
    u64::from(code) <= u64::from(SLOT) + u64::from(ROOM)
        && spill + u64::from(data) + u64::from(bss) <= u64::from(ROOM)
}

/// A word-sized number near `v` (within 16 words either side).
fn near(rng: &mut Rng, v: u32) -> u32 {
    let d = rng.below(33) as u32 * 4;
    (v + d).saturating_sub(64) & !3
}

#[test]
fn the_shared_room_is_the_rule_and_good_games_load() {
    let all = std::env::var("SHARED_LOADS").is_ok_and(|v| v == "all");
    let (loads, every) = if all { (usize::MAX, 1) } else { (300, 13) };
    let mut rng = Rng::new(0x5EED_5911);
    let (mut yes, mut no, mut loaded, mut met) = (0, 0, 0, 0);
    for n in 0..20_000 {
        // The code: anywhere, near the end of the slot, or near the end
        // of the spill. At least one word (the entry is its first).
        let code = match rng.below(3) {
            0 => (rng.below((SLOT + ROOM + 64) as usize / 4) as u32 * 4).max(4),
            1 => near(&mut rng, SLOT).max(4),
            _ => near(&mut rng, SLOT + ROOM),
        };
        let spill = code.saturating_sub(SLOT);
        // Data and zeroed: near what is left beside the spill, or
        // anywhere, now and then huge.
        let total = match rng.below(4) {
            0 => rng.below((ROOM + 64) as usize / 4) as u32 * 4,
            3 if n % 50 == 0 => 0xFFFF_FFF8,
            _ => near(&mut rng, ROOM.saturating_sub(spill)),
        };
        let data = (rng.below(total as usize / 4 + 1) as u32 * 4).min(total);
        let bss = total - data;

        let mut h = header(code, data, bss);
        h.entry = layout::CODE_SLOT_ADDR;
        let got = check_header(&h, &Slots::CONSOLE);
        let want = rule(code, data, bss);
        assert_eq!(
            got.is_ok(),
            want,
            "code {code}, data {data}, zeroed {bss}: {got:?}"
        );
        assert_eq!(
            h.validate().is_ok(),
            want,
            "game-api: code {code}, data {data}, zeroed {bss}"
        );
        if !want {
            no += 1;
            met += matches!(got, Err(LoadError::CodeMeetsData { .. })) as u32;
            continue;
        }
        yes += 1;
        // Load every few good cases, so the loads spread over the sweep.
        if loaded < loads && n % every == 0 {
            let payload = pattern((code + data) as usize, n as u8);
            let mut card = Card::new(pack(h, &payload));
            let mut fake = FakeSlots::console();
            let got = load(&mut card, PATH, &Slots::CONSOLE, &mut fake);
            assert!(
                got.is_ok(),
                "code {code}, data {data}, zeroed {bss}: {got:?}"
            );
            assert_eq!(fake.code_bytes_in_data(), spill as usize);
            loaded += 1;
        }
    }
    eprintln!("shared room: {yes} good, {no} refused ({met} code meets data), {loaded} loaded");
    assert!(yes > 4_000 && no > 4_000, "{yes} good, {no} refused");
    assert!(met > 2_000, "too few cases where code meets data: {met}");
    assert!(loaded >= 300, "only {loaded} loaded");
}
