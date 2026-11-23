//! A career as 52 bytes, and back. The console has no save call for
//! games yet, so only the tests use this module (the game keeps its
//! "last dock" as a plain [`Career`] value, and the chip build leaves
//! this code out). When the console gets a save call, these bytes
//! are what to store (52 of the 64 a save may have).
//!
//! | Bytes | What |
//! |---|---|
//! | 0 | format version (5: step M5's ships to buy) |
//! | 1..5 | galaxy seed, u32 little end first |
//! | 5 | system (below 136) |
//! | 6..10 | credits, u32 (at most 9,999,999) |
//! | 10 | fuel in tenths of a light year (at most 65) |
//! | 11..19 | the hold, one byte a good (in all at most the ship's hold) |
//! | 19 | gear: laser (bits 0-1, at most 2), shield (bits 2-3, at most 2), scoop (bit 5), game won (bit 6); bits 4 and 7 zero |
//! | 20 | hull, 1..= the ship's most |
//! | 21..23 | kills, u16 |
//! | 23..25 | day, u16 |
//! | 25 | the chart's selected system (below 136) |
//! | 26 | settings: detail (bits 0-1, at most 2), chase view (bit 2), 60 a second (bit 3), pitch inverted (bit 4); bits 5-7 zero |
//! | 27..44 | surveyed systems, one bit each (system n: byte 27 + n / 8, bit n % 8) |
//! | 44 | raider TAGs carried, not yet paid |
//! | 45..47 | credits they are worth, u16: each TAG 25, 50, 75 or 100, so a multiple of 25 from 25 x TAGs to 100 x TAGs |
//! | 47 | the ship flown (0 LARK, 1 DRAY, 2 SHRIKE) |
//! | 48..52 | CRC-32 of bytes 0..48 |
//!
//! Loading refuses bytes with another version, a wrong CRC, or any
//! number out of range, so a damaged save can never start a broken game.
//!
//! **Version 4** (51 bytes: no ship byte, CRC at 47..51; bit 4 of the
//! gear byte was the CARGO BAY 40T, gone in step M5) still loads, as
//! the LARK: a fitted bay is paid back at its price (900 cr, credits
//! capped as always), and tonnes past the LARK's 24 are sold at the
//! save's station's sell prices, goods taken in the market's order.

use crate::galaxy::{self, DANGER_NAMES, SYSTEMS};
use crate::market::{self, GOODS, Hold};
use crate::world::{CHART_BYTES, CRAFTS, CREDITS_MAX, Career, LARK, TAG_VALUE, TANK};

pub const SIZE: usize = 52;
pub const VERSION: u8 = 5;
/// Version 4's size (no ship byte), and the price paid back for its
/// CARGO BAY 40T.
pub const SIZE_V4: usize = 51;
pub const BAY_PRICE: u32 = 900;
/// Where the surveys, the TAGs, the ship and the CRC are.
const CHART: usize = 27;
const TAGS: usize = CHART + CHART_BYTES;
const SHIP: usize = TAGS + 3;
const CRC: usize = SHIP + 1;
// Every bit of the survey bytes is a system: no bit to check for zero.
const _: () = assert!(SYSTEMS.is_multiple_of(8) && CRC + 4 == SIZE && SHIP + 4 == SIZE_V4);

/// Why bytes were refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bad {
    Version,
    Crc,
    Range,
}

/// The CRC-32 (the common one, as in zip files), bit by bit: small code.
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// The career and the settings byte as bytes.
pub fn to_bytes(c: &Career, settings: u8) -> [u8; SIZE] {
    let mut b = [0u8; SIZE];
    b[0] = VERSION;
    b[1..5].copy_from_slice(&c.seed.to_le_bytes());
    b[5] = c.system;
    b[6..10].copy_from_slice(&c.credits.to_le_bytes());
    b[10] = c.fuel;
    b[11..19].copy_from_slice(&c.hold.0);
    b[19] = c.laser | c.shield << 2 | u8::from(c.scoop) << 5 | u8::from(c.won) << 6;
    b[20] = c.hull;
    b[21..23].copy_from_slice(&c.kills.to_le_bytes());
    b[23..25].copy_from_slice(&c.day.to_le_bytes());
    b[25] = c.target;
    b[26] = settings;
    b[CHART..TAGS].copy_from_slice(&c.charted);
    b[TAGS] = c.tags;
    b[TAGS + 1..SHIP].copy_from_slice(&c.owed.to_le_bytes());
    b[SHIP] = c.ship;
    let crc = crc32(&b[..CRC]);
    b[CRC..].copy_from_slice(&crc.to_le_bytes());
    b
}

/// The career and the settings byte back, or why not: [`SIZE`] bytes
/// of this version, or [`SIZE_V4`] bytes of version 4 (see the module
/// notes for what becomes of its cargo bay).
pub fn from_bytes(b: &[u8]) -> Result<(Career, u8), Bad> {
    let v4 = match (b.first(), b.len()) {
        (Some(&VERSION), SIZE) => false,
        (Some(4), SIZE_V4) => true,
        _ => return Err(Bad::Version),
    };
    let crc_at = if v4 { SHIP } else { CRC };
    let crc = u32::from_le_bytes([b[crc_at], b[crc_at + 1], b[crc_at + 2], b[crc_at + 3]]);
    if crc != crc32(&b[..crc_at]) {
        return Err(Bad::Crc);
    }
    let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let mut hold = [0u8; GOODS];
    hold.copy_from_slice(&b[11..19]);
    let gear = b[19];
    let mut charted = [0u8; CHART_BYTES];
    charted.copy_from_slice(&b[CHART..TAGS]);
    let bay = v4 && gear & 16 != 0;
    let mut c = Career {
        seed: u32_at(1),
        system: b[5],
        credits: u32_at(6),
        fuel: b[10],
        hold: Hold(hold),
        laser: gear & 3,
        shield: (gear >> 2) & 3,
        ship: if v4 { LARK } else { b[SHIP] },
        scoop: gear & 32 != 0,
        hull: b[20],
        kills: u16_at(21),
        day: u16_at(23),
        target: b[25],
        won: gear & 64 != 0,
        charted,
        tags: b[TAGS],
        owed: u16_at(TAGS + 1),
    };
    // Each TAG is worth TAG_VALUE times 1 to 4 (`world::tag_worth`: the
    // raider's gun level plus half its kill system's danger).
    let tags = u32::from(c.tags) * u32::from(TAG_VALUE);
    let owed = u32::from(c.owed);
    let settings = b[26];
    // Version 4's hold took 40 tonnes with the bay, 24 without.
    let hold_most = if v4 {
        if bay { 40 } else { 24 }
    } else {
        c.capacity()
    };
    let ok = c.credits <= CREDITS_MAX
        && usize::from(c.ship) < CRAFTS.len()
        && usize::from(c.system) < SYSTEMS
        && usize::from(c.target) < SYSTEMS
        && c.fuel <= TANK
        && c.laser <= 2
        && c.shield <= 2
        && gear & 0x80 == 0
        && (v4 || gear & 16 == 0)
        && c.hull >= 1
        && i32::from(c.hull) <= c.hull_max()
        && c.hold.total() <= hold_most
        && settings & 3 <= 2
        && settings & 0xE0 == 0
        && (tags..=tags * DANGER_NAMES.len() as u32).contains(&owed)
        && owed.is_multiple_of(u32::from(TAG_VALUE));
    if !ok {
        return Err(Bad::Range);
    }
    if bay {
        c.credits = c.credits.saturating_add(BAY_PRICE).min(CREDITS_MAX);
        let sys = galaxy::system(c.seed, c.system);
        for g in 0..GOODS {
            let over = c.hold.total().saturating_sub(c.capacity());
            let n = u32::from(c.hold.0[g]).min(over);
            if n > 0 {
                let p = market::sell_price(&sys, g, c.day);
                c.hold.0[g] -= n as u8;
                c.credits = c.credits.saturating_add(n * p).min(CREDITS_MAX);
            }
        }
    }
    Ok((c, settings))
}
