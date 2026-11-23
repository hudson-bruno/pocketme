//! Trading: eight goods, their prices in each system, and what a station
//! has in stock.
//!
//! **How a price is made.** Each good has a base price. A system's
//! economy makes some goods (cheap there: down to about 64 % of the
//! base in a SECURE system) and wants others (dear: up to about 136 %).
//! On top of that each system has its own small difference per good (up
//! to 6 % either way) and prices drift a little from day to day (up to
//! 5 %; a day passes at each jump). A station buys back for 94 % of its
//! selling price, so a good bought and sold in the same place always
//! loses a little.
//!
//! **Risk pays.** Danger widens the gap between cheap and dear: few
//! traders dare to fly to a dangerous system, so what its economy makes
//! piles up (cheaper) and what it wants is scarce (dearer). Each danger
//! step moves a price [`RISK_PAY`] more percentage points per step of
//! the economy's want (see [`price`]); a SECURE system has exactly the
//! plain prices, and goods an economy neither makes nor wants do not
//! change. So buying in a RISKY or LAWLESS system and selling in one
//! both pay better than in a SECURE system of the same economy. The
//! market screen says RISK PAY in RISKY and LAWLESS systems.
//!
//! **Routes.** Profit comes from carrying a good from a system whose
//! economy makes it to one whose economy wants it: grain from a FARM to
//! a MINE, ore from a MINE to a FORGE, alloys from a FORGE to a LAB,
//! chips from a LAB to a FARM or a HAVEN, meds from a LAB to a MINE,
//! art from a HAVEN to a FORGE. The market screen marks what the local
//! economy makes, so routes can be learnt by flying them.

use crate::fixed::hash;
use crate::galaxy::{Econ, System};

/// Kinds of goods.
pub const GOODS: usize = 8;
pub const NAMES: [&str; GOODS] = [
    "GRAIN", "ICE", "ORE", "ALLOYS", "SPARES", "MEDS", "CHIPS", "ART",
];
/// Base price of one unit (a tonne), in credits.
pub const BASE: [u32; GOODS] = [8, 6, 14, 30, 46, 70, 96, 140];

/// Per economy and good: below 0 the economy makes it (cheap), above 0
/// it wants it (dear).
const AFF: [[i8; GOODS]; 5] = [
    // GRAIN ICE ORE ALLOYS SPARES MEDS CHIPS ART
    [-3, 1, 1, 1, 2, 1, 2, 0],  // FARM
    [3, -3, -3, 0, 2, 2, 1, 1], // MINE
    [2, 2, 3, -3, -2, 1, 0, 2], // FORGE
    [1, 1, 0, 2, 1, -3, -3, 1], // LAB
    [2, 1, -1, 1, 1, 2, 2, -3], // HAVEN
];

/// Whether the economy makes this good (shown on the market screen).
pub fn made_here(e: Econ, good: usize) -> bool {
    AFF[e as usize][good] < 0
}

/// How much danger widens prices: each danger step (SECURE 0 to
/// LAWLESS 3) adds this many percentage points per step of an economy's
/// want (`AFF`, -3 to 3) to the 12 a SECURE system has. At 4, a good
/// an economy makes most (want -3) costs 64 % of its base in a SECURE
/// system and 28 % in a LAWLESS one; one it wants most (want 3) costs
/// 136 % and 172 %. On the main routes that is about 14 to 24 % more
/// profit a tonne per danger step at either end, and about twice the
/// profit for a run from a LAWLESS system to a LAWLESS one.
pub const RISK_PAY: i32 = 4;

// The cheapest case must stay above 0 % (want -3, LAWLESS, own -6,
// drift -5), and the dearest product below 2^15 (see `price`).
const _: () = assert!(100 - 3 * (12 + 3 * RISK_PAY) - 6 - 5 > 0);
const _: () = assert!(140 * (100 + 3 * (12 + 3 * RISK_PAY) + 6 + 5) < 1 << 15);

/// What the station sells one unit of `good` for, on `day`: a pure
/// function of the system, the good and the day. At least 2, so the
/// 94 % a station pays back is always less.
pub fn price(s: &System, good: usize, day: u16) -> u32 {
    let aff = i32::from(AFF[s.econ as usize][good]);
    let own = (hash(s.h, good as u32) % 13) as i32 - 6;
    let drift = (hash(s.h ^ u32::from(day).wrapping_mul(0x9E37), good as u32) % 11) as i32 - 5;
    // danger 0..=3 (kept there even for a hand-made System): the step
    // per want is 12..=24 (with RISK_PAY 4), aff within +-3, own within
    // +-6, drift within +-5: pct 17..=183 (53..=147 when SECURE), and
    // the product below 2^15 (both checked above).
    let step = RISK_PAY
        .wrapping_mul(i32::from(s.danger.min(3)))
        .wrapping_add(12);
    let pct = 100i32
        .wrapping_add(aff.wrapping_mul(step))
        .wrapping_add(own)
        .wrapping_add(drift) as u32;
    (BASE[good].wrapping_mul(pct) / 100).max(2)
}

/// What the station pays for one unit: 94 % of its price, at least 1.
pub fn sell_price(s: &System, good: usize, day: u16) -> u32 {
    // Prices are below 2^9 (at most 140 x 183 % = 256).
    (price(s, good, day).wrapping_mul(94) / 100).max(1)
}

/// Units the station has when you dock: plenty of what the economy
/// makes, a little or nothing of the rest.
pub fn stock(s: &System, good: usize, day: u16) -> u8 {
    let h = hash(s.h.wrapping_add(u32::from(day)), 0x570C + good as u32);
    if made_here(s.econ, good) {
        (24 + h % 40) as u8
    } else {
        (h % 12) as u8
    }
}

/// The hold: units of each good.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hold(pub [u8; GOODS]);

impl Hold {
    /// Units carried in all.
    pub fn total(&self) -> u32 {
        // Eight bytes: the sum is at most 2,040.
        self.0
            .iter()
            .fold(0u32, |a, &n| a.wrapping_add(u32::from(n)))
    }
}
