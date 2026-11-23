//! LOW, MID and HIGH (the spec's section 4) and MAX as values, read by
//! the picture (`scene.rs`). Picked in the START menu or with X.
//!
//! MAX is the level above HIGH for a richer picture at about 24 a
//! second on the board (the owner's sizing, 2026-11-22). Every level
//! changes the picture only: the driving rules are the same at all four.

/// One detail level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    Low,
    Mid,
    High,
    Max,
}

/// What a level sets (the spec's table; SCREE's ranges, ESTIMATED).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reach {
    /// 16 m blocks each side drawn as fine 8 m cells.
    pub fine_blocks: u8,
    /// How far the land is drawn (m).
    pub far_m: u16,
    /// How far things are drawn (m).
    pub things_m: u16,
    /// Sides of a tree's cone.
    pub tree_sides: u8,
    /// Dust puffs behind the car.
    pub puffs: u8,
    /// Road ribbon pieces drawn as one (each is 4 m): LOW draws 8 m
    /// pieces to keep within its budget.
    pub road_join: u8,
    /// The car and the finish banner with fewer faces, and four-cornered
    /// dust puffs (LOW, to keep within its budget).
    pub plain_car: bool,
    /// Each dust puff drawn as a small cloud: the puff and a smaller one
    /// beside it, a little lower (MAX). The puffs themselves (where,
    /// how many, how old) are the rules' and the same at every level.
    pub cloud: bool,
    /// MAX's roadside things (`things::DECOR`): corner markers, the
    /// fence's rails and hay bales, drawn only (never in the rules).
    pub stage: bool,
    /// The triangle budget (the 95 % point of a scripted drive).
    pub budget: u16,
}

impl Detail {
    /// HIGH, the start (the owner's decision of 2026-11-23: every game
    /// starts at its level called HIGH; the spec's start was MID).
    pub const START: Detail = Detail::High;

    pub const fn name(self) -> &'static str {
        match self {
            Detail::Low => "LOW",
            Detail::Mid => "MID",
            Detail::High => "HIGH",
            Detail::Max => "MAX",
        }
    }

    /// The next level, round (X).
    pub const fn next(self) -> Detail {
        match self {
            Detail::Low => Detail::Mid,
            Detail::Mid => Detail::High,
            Detail::High => Detail::Max,
            Detail::Max => Detail::Low,
        }
    }

    pub const fn reach(self) -> Reach {
        match self {
            Detail::Low => Reach {
                fine_blocks: 1,
                far_m: 80,
                things_m: 32,
                tree_sides: 4,
                puffs: 2,
                road_join: 2,
                plain_car: true,
                cloud: false,
                stage: false,
                budget: 180,
            },
            Detail::Mid => Reach {
                fine_blocks: 2,
                far_m: 144,
                things_m: 56,
                tree_sides: 6,
                puffs: 4,
                road_join: 1,
                plain_car: false,
                cloud: false,
                stage: false,
                budget: 430,
            },
            Detail::High => Reach {
                fine_blocks: 5,
                far_m: 240,
                things_m: 112,
                tree_sides: 8,
                puffs: 8,
                road_join: 1,
                plain_car: false,
                cloud: false,
                stage: false,
                budget: 940,
            },
            // The land and the things much farther than HIGH (M1),
            // rounder trees and the dust in small clouds (M2), the
            // roadside things (M3).
            Detail::Max => Reach {
                fine_blocks: 5,
                far_m: 352,
                things_m: 192,
                tree_sides: 12,
                puffs: 8,
                road_join: 1,
                plain_car: false,
                cloud: true,
                stage: true,
                budget: 1_075,
            },
        }
    }
}
