//! One star system in flight, in fixed ticks of 1/60 s, and the
//! player's career (money, hold, gear) that goes from system to system.
//!
//! **Units.** Positions in 1/256 m ([`M`]), kept inside +-[`LIMIT`]
//! (2,097 km), so a difference of two positions fits an `i32` and the
//! square of a distance fits an `i64`. Speeds in units a tick (300 m/s
//! is 1,280). Turning in angle steps a tick (65,536 a turn).
//!
//! **A system.** The planet sits at the middle; the station flies at
//! 2.4 planet radii, its mouth facing away from the planet; the sun is
//! 270 to 390 km away. A ship arrives 30 km from the station.
//!
//! **Flight is arcade flight**: the ship goes where its nose points, at
//! the speed the throttle sets (no drifting). Far from everything the
//! CRUISE drive goes much faster, and slows down by itself near the
//! station, the planet or the sun, so a trip across the system takes
//! seconds; it switches off near any other ship that is hunting you.
//!
//! **Numbers that are proved to stay in range** (the fuzz test checks
//! them too): credits 0..=[`CREDITS_MAX`], fuel 0..=[`TANK`], heat
//! 0..=[`HEAT_MAX`], shield 0..=its maximum, hull 0..=[`HULL_MAX`],
//! speed 0..=[`CRUISE_MAX`], every position inside +-[`LIMIT`].

use crate::art::mat;
use crate::fixed::{Dice, Mat3, ONE14, V3, cos, dot, hash, length, norm, scale14, sin, sub, wide};
use crate::fx::{self, Fx};
use crate::galaxy::{self, System};
use crate::market::{self, GOODS, Hold};

/// Ticks a second.
pub const HZ: u32 = 60;
/// One tick in microseconds (rounded down; the app counts in these).
pub const TICK_US: u32 = 16_666;
/// One metre, one kilometre.
pub const M: i32 = 256;
pub const KM: i32 = 1000 * M;
/// Positions stay inside +-LIMIT on each axis.
pub const LIMIT: i32 = 1 << 29;
/// Top speed without cruise: 300 m/s (the LARK's; step M5: each ship
/// has its own, [`Craft::top`]; cruise starts at this speed for all).
pub const MAX_SPEED: i32 = 1280;
/// Top cruise speed: 15 km/s.
pub const CRUISE_MAX: i32 = 64_000;
/// Step M4, overboost: the press of X at top speed that cruise refuses
/// (near the station, under attack), held, runs the drive past its limit
/// to 7/4 of top speed (525 m/s) while the charge lasts. The laser and
/// the turret are dead meanwhile and the ship turns at half its rate.
/// (The LARK's; step M5: per ship, [`Craft::od_top`].)
pub const OD_TOP: i32 = MAX_SPEED * 7 / 4;
/// The charge: full is 3 s of overboost (3 a tick spent, 1 a tick won
/// back when it is off: empty to full in 9 s); it starts only with at
/// least [`OD_MIN`] (3/4 s). (The LARK's; per ship, [`Craft::od_full`].)
pub const OD_FULL: u16 = 540;
pub const OD_MIN: u16 = 135;
/// While it runs the raiders aim as at a pilot turning this hard
/// (`Me::jink`): their hit chance is 48/144, a third.
pub const OD_JINK: i32 = 96;
/// A level-3 raider overboosts too, to 7/4 of its top speed: for the
/// first 3 s ([`OD_RUN`]) of a flight, and when it hunts the player and
/// the player is out of its gun's reach: 3 s at most (less when the
/// player comes within reach), at most once every 12 s ([`OD_EVERY`]).
/// It never fires meanwhile.
pub const OD_RUN: u16 = 180;
pub const OD_EVERY: u16 = 720;
/// Step M4 switch: the level-3 raiders' overboost.
pub const RAIDER_OD_ON: bool = true;
/// Credits never go above this (the screen has room for seven digits).
pub const CREDITS_MAX: u32 = 9_999_999;
/// A full tank: 6.5 light years.
pub const TANK: u8 = 65;
pub const HEAT_MAX: i32 = 1000;
/// The starting ship's hull (the LARK's, [`CRAFTS`]): the hull a new
/// career has. Each ship's own most is [`Career::hull_max`].
pub const HULL_MAX: i32 = 100;
/// Other ships at once (canisters count).
pub const SHIPS: usize = 12;
/// Slots `0..FIGHT` are the ones the fights, canisters and the old
/// traffic use, exactly as before step L2; slots `FIGHT..SHIPS` hold only
/// the extra traffic of [`World::traffic`], so it never takes a slot a
/// raider or a canister needs (step F1: and the first canister of a
/// hauler downed there, which takes its slot).
pub const FIGHT: usize = 8;
/// Step L2 switch: the extra traffic (ships leaving and arriving at the
/// station, a patrol, convoys with escorts or raiders on their tail).
pub const TRAFFIC_ON: bool = true;
/// Step L2 switch: a raider pack closes in as a V behind its leader and
/// breaks up 2 km from its prey (before L2 each flew straight at it).
pub const WINGS_ON: bool = true;
/// Ticks a ship leaving the station flies straight out (40 s).
const DEPART: u16 = 2400;
/// Formation places behind a leader (`Ship::wing`): metres to the side,
/// up and ahead (negative: behind), times the kind's growth
/// ([`Kind::grown`], step M2).
const WINGS: [[i32; 3]; 4] = [[0, 0, 0], [190, 0, -230], [-190, 0, -230], [-280, 110, -40]];
/// Laser reach: 2.4 km.
pub const LASER_RANGE: i32 = 2400 * M;
/// The dorsal turret's reach, and its damage a hit, heat a shot and
/// ticks between shots (weaker than the nose laser: it aims itself).
pub const TURRET_RANGE: i32 = 1500 * M;
const TURRET: (i32, i32, u8) = (6, 45, 10);
/// Credits a tenth of a light year of fuel costs.
pub const FUEL_PRICE: u32 = 1;
/// Credits a point of hull costs to mend.
pub const REPAIR_PRICE: u32 = 2;
/// Step M2, sizes. Variant B (fighters 2x, freighters 2.5x the
/// fighters) is built only with `--cfg merc_sizes_b`; the normal build
/// is variant A.
pub const SIZES_B: bool = cfg!(merc_sizes_b);
/// Raiders and wardens are this many times their size before M2 (A: 3).
pub const FIGHTER_X: i32 = if SIZES_B { 2 } else { 3 };
/// Haulers are this many times their size before M2 (A: 6, twice the
/// fighters; B: 5, two and a half times the fighters).
pub const FREIGHTER_X: i32 = if SIZES_B { 5 } else { 6 };
/// Rocks, canisters and TAGs are twice their size before M2.
pub const SMALL_X: i32 = 2;
/// The station is this many times its size before M2 (its model is in
/// 1/16 m; drawn and hit at `16 * STATION_X` world units a model unit).
pub const STATION_X: i32 = 4;
/// World units per station model unit.
pub const STATION_U: i32 = 16 * STATION_X;
/// Step F1: the player's courier is drawn this many times its model's
/// size (before: 1, 32 m long, the small one; at 6, 191 m long, 81 % of
/// a raider's length and 94 % of its span). The chase camera, the trail,
/// the launch point and the hub's crash margin ([`ME_R`]) follow it.
pub const ME_X: i32 = 6;
/// The courier's half span at size 1 (20 m) times [`ME_X`].
pub const ME_R: i32 = 20 * M * ME_X;
/// Step F1: a hauler's hull points (80 before).
pub const HAUL_HP: i32 = 540;
/// Step F1: per economy (`galaxy::Econ` order), the two goods it wants
/// most: a hauler's load ([`World::shed`]).
const HAUL: [[u8; 2]; 5] = [[4, 6], [0, 5], [2, 7], [3, 7], [5, 6]];
/// How near the station a ship must be before the auto-dock flies the
/// last part (the approach point's distance in front of the mouth).
const APPROACH: i32 = 750 * M * STATION_X;

/// Gear a station sells: `(name, price, lowest tech level)`. Step M5:
/// the CARGO BAY 40T (900 cr, tech 3) is gone: the hold belongs to the
/// ship ([`CRAFTS`]), so a bigger hold means another ship.
pub const GEAR: [(&str, u32, u8); 5] = [
    ("LASER MK2", 1500, 4),
    ("LASER MK3", 4800, 8),
    ("SHIELD MK2", 1200, 5),
    ("SHIELD MK3", 4200, 9),
    ("SUN SKIMMER", 700, 6),
];

/// Step M5: a ship a station sells ([`CRAFTS`]). Speeds are world units
/// a tick (1280 = 300 m/s), turn rates angle steps a tick at full stick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Craft {
    pub name: &'static str,
    /// List price in credits.
    pub price: u32,
    /// The lowest tech level of a station that sells it.
    pub tech: u8,
    /// Tonnes the hold takes.
    pub hold: u8,
    /// Top speed without cruise or overboost.
    pub top: i32,
    /// Turn rates: pitch, yaw, roll.
    pub turn: [i32; 3],
    /// Hull points when whole.
    pub hull: u8,
    /// The fitted shield's most, in quarters of [`SHIELDS`]' (4: as
    /// listed). Its charging speed is the shield's own.
    pub shield: u8,
    /// A full overboost charge in ticks of overboost times 3 (as
    /// [`OD_FULL`]).
    pub od_full: u16,
    /// The overboost's top speed in quarters of [`Craft::top`].
    pub od_top: u8,
    /// The model's rear in model units (1/16 m): where the engine trail
    /// starts.
    pub tail: i32,
}

/// Which ship a career flies ([`Career::ship`]).
pub const LARK: u8 = 0;
pub const DRAY: u8 = 1;
pub const SHRIKE: u8 = 2;

/// Step M5, the ships to buy. LARK is the courier every career starts
/// with (exactly the ship before M5). DRAY carries 2.5 times as much,
/// but is slower than a raider (1000), turns slowly and breaks sooner.
/// SHRIKE carries half as little, but is the fastest and toughest, turns
/// fastest, and its shields and overboost last longer.
pub const CRAFTS: [Craft; 3] = [
    Craft {
        name: "LARK",
        price: 1600,
        tech: 1,
        hold: 24,
        top: MAX_SPEED,
        turn: [230, 190, 420],
        hull: HULL_MAX as u8,
        shield: 4,
        od_full: OD_FULL,
        od_top: 7,
        tail: 250,
    },
    Craft {
        name: "DRAY",
        price: 3600,
        tech: 4,
        hold: 60,
        top: 960,
        turn: [160, 130, 300],
        hull: 70,
        shield: 4,
        od_full: 360,
        od_top: 6,
        tail: 300,
    },
    Craft {
        name: "SHRIKE",
        price: 9000,
        tech: 7,
        hold: 12,
        top: 1536,
        turn: [270, 225, 420],
        hull: 160,
        shield: 5,
        od_full: 720,
        od_top: 7,
        tail: 250,
    },
];

/// The most any ship's hold takes, hull and turn rate: the bounds the
/// overflow notes below rely on.
pub const HOLD_MOST: u32 = 60;
pub const HULL_MOST: i32 = 160;
const _: () = {
    let mut i = 0;
    while i < CRAFTS.len() {
        let c = &CRAFTS[i];
        assert!(c.hold as u32 <= HOLD_MOST && c.hull as i32 <= HULL_MOST && c.hull > 0);
        assert!(c.top <= MAX_SPEED * 6 / 5 && c.top > 300 && c.od_top >= 4);
        assert!(c.turn[0] <= 420 && c.turn[1] <= 420 && c.turn[2] <= 420);
        assert!(c.od_full >= OD_MIN && c.shield <= 8);
        i += 1;
    }
};

/// Step M5: a ship's trade-in when another is bought: 3/4 of its list
/// price, less what mending its hull would cost.
pub const TRADE_IN: (u32, u32) = (3, 4);

/// Laser by level: damage a hit, heat a shot, ticks between shots.
const LASER: [(i32, i32, u8); 3] = [(8, 60, 8), (14, 55, 7), (22, 50, 6)];

/// Step M1: a raider's gun by level (`Ship::gun`, 0..=2 for levels 1
/// to 3, like the player's laser): damage a hit, the fewest ticks
/// between shots (the dice add 0 to 15 more), and the hit chance in
/// percent at [`SURE_NEAR`] or nearer against a pilot who does not turn.
pub const RAIDER_GUNS: [(i32, u32, u32); 3] = [(5, 28, 40), (8, 22, 50), (16, 18, 60)];
/// A raider's gun reaches as far as the player's laser.
pub const RAIDER_REACH: i32 = LASER_RANGE;
/// Within this the raider's hit chance is whole; out at the reach it is
/// 2/3 of it, in a straight line between.
pub const SURE_NEAR: i32 = 1200;
/// The player's turning spoils a raider's aim: the hit chance is
/// multiplied by `DODGE / (DODGE + Me::jink)` (full stick on one axis
/// turns about 190 to 230 steps a tick).
pub const DODGE: u32 = 48;
/// Turning slower than this (the small turns that keep a foe in the
/// sights) does not spoil a raider's aim; only the rest counts.
pub const STEADY: u32 = 32;
/// The player's turning counts for the raiders' aim as an average over
/// about this many ticks (`Me::jink`): a pilot who jinks now and then
/// stays hard to hit while it lines up a shot.
pub const JINK_SPAN: i32 = 32;
/// Gun levels (0-based) by the system's danger, one of four picked by a
/// hash when the raider comes: SECURE all level 1, QUIET half 1 and half
/// 2, RISKY 2 with one in four 3, LAWLESS 3 with one in four 2.
pub const GUN_ODDS: [[u8; 4]; 4] = [[0; 4], [0, 0, 1, 1], [1, 1, 1, 2], [1, 2, 2, 2]];
/// Raider packs by danger: the fewest ships and how many more the dice
/// may add (SECURE 1, as before, so the start system stays gentle;
/// QUIET 1-3, RISKY 2-4, LAWLESS 3-5). A pack of more than three comes
/// as two V's, three in front and the rest [`PACK_GAP`] farther out on
/// the same line.
pub const PACKS: [(u32, u32); 4] = [(1, 0), (1, 2), (2, 2), (3, 2)];
pub const PACK_GAP: i32 = 3 * KM;
/// Shots a raider must have fired before it may flee (it flees below 30 %
/// of its hull only once it has traded fire).
pub const FLEE_AFTER: u8 = 2;
/// A fleeing raider runs for this long (10 s), then comes back to the
/// hunt unless the player is within [`CHASED`]; it runs at 5/4 of its
/// top speed and jinks from side to side.
pub const FLEE_TICKS: u16 = 600;
pub const CHASED: i32 = 3 * KM;
/// Shield by level: most, and ticks per point won back.
const SHIELDS: [(i32, u32); 3] = [(80, 15), (140, 12), (220, 10)];

/// What the player keeps from system to system (and what a save holds).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Career {
    /// The galaxy's seed.
    pub seed: u32,
    /// The system the player is in.
    pub system: u8,
    pub credits: u32,
    /// Tenths of a light year, 0..=TANK.
    pub fuel: u8,
    pub hold: Hold,
    /// 0..=2: MK1 to MK3.
    pub laser: u8,
    pub shield: u8,
    /// Step M5: the ship flown ([`CRAFTS`]: [`LARK`], [`DRAY`] or
    /// [`SHRIKE`]).
    pub ship: u8,
    pub scoop: bool,
    pub hull: u8,
    pub kills: u16,
    /// Days pass at each jump; prices drift with them.
    pub day: u16,
    /// The system the chart has selected.
    pub target: u8,
    /// The guild's charter is granted: the game is won (play goes on).
    pub won: bool,
    /// The systems this career has surveyed (docked at), one bit each.
    pub charted: [u8; CHART_BYTES],
    /// Raider TAGs carried, not yet paid (at most [`TAGS_MAX`]).
    pub tags: u8,
    /// Credits those TAGs are worth (each fixed at its kill).
    pub owed: u16,
}

/// A raider's TAG is worth 1 to 4 times this ([`tag_worth`]: its gun
/// level plus half the danger of the system where it died).
pub const TAG_VALUE: u16 = 25;
/// The canister load's good number that marks a raider's TAG (one past
/// the goods); the load's count is its worth in [`TAG_VALUE`]s.
pub const TAG: u8 = GOODS as u8;
/// Step L4 (switch [`ROCKS_ON`]): the canister load's good number
/// that marks a drifting rock (the count is the good it breaks into:
/// ORE or ICE). Rocks are never scooped; shot apart, each leaves one
/// canister of 1-3 tonnes of that good.
pub const ROCK: u8 = TAG + 1;
pub const ROCKS_ON: bool = true;
/// A rock's hull points and life (ticks; 90 s at 60 a second).
const ROCK_HP: i32 = 24;
const ROCK_LIFE: u16 = 60 * 90;
/// TAGs carried at most: a further one is not scooped.
pub const TAGS_MAX: u8 = 255;
/// Whether a LAWLESS station pays bounties for TAGs (the one switch).
pub const LAWLESS_PAYS: bool = false;
/// Ticks a cargo canister floats before it is gone: 90 s at 60 a second.
pub const CARGO_LIFE: u16 = 60 * 90;
/// Ticks a raider's TAG floats before it is gone: 120 s at 60 a second
/// (the owner, 2026-11-22). 7,200 fits the canister's u16 timer.
pub const TAG_LIFE: u16 = 60 * 120;

/// Credits at a dock that, with [`CHARTER_SURVEYS`], win the charter.
pub const WIN_CREDITS: u32 = 50_000;
/// Systems surveyed that, with [`WIN_CREDITS`], win the charter.
pub const CHARTER_SURVEYS: u32 = 20;
/// One bit per system.
pub const CHART_BYTES: usize = galaxy::SYSTEMS.div_ceil(8);
/// The standing a career's surveys give: the first name until the
/// first count, and so on.
pub const STANDING: [(u32, &str); 5] = [
    (3, "DRIFTER"),
    (8, "SKETCHER"),
    (14, "PLOTTER"),
    (CHARTER_SURVEYS, "CHARTWRIGHT"),
    (u32::MAX, "MAPMAKER"),
];

impl Career {
    /// A new career in the galaxy made from `seed`.
    pub fn new(seed: u32) -> Career {
        let s = galaxy::start_system(seed);
        Career {
            seed,
            system: s,
            credits: 300,
            fuel: TANK,
            hold: Hold([0; GOODS]),
            laser: 0,
            shield: 0,
            ship: LARK,
            scoop: false,
            hull: HULL_MAX as u8,
            kills: 0,
            day: 0,
            target: s,
            won: false,
            // The home system counts as surveyed (no fee).
            charted: chart_bit(s),
            tags: 0,
            owed: 0,
        }
    }

    /// The career a lost ship goes back to: this one (the last dock),
    /// with the hold empty and the TAGs and their money gone.
    #[inline(never)]
    pub fn wrecked(mut self) -> Career {
        self.hold = Hold([0; GOODS]);
        self.tags = 0;
        self.owed = 0;
        self
    }

    /// Whether system `id` is surveyed.
    pub fn charted(&self, id: u8) -> bool {
        let (i, m) = chart_spot(id);
        self.charted[i] & m != 0
    }

    /// Systems surveyed.
    #[inline(never)]
    pub fn surveys(&self) -> u32 {
        // At most 136: no sum wraps.
        self.charted
            .iter()
            .fold(0u32, |n, b| n.wrapping_add(b.count_ones()))
    }

    /// The guild's charter can be granted: credits and surveys.
    pub fn charter_ready(&self) -> bool {
        self.credits >= WIN_CREDITS && self.surveys() >= CHARTER_SURVEYS
    }

    /// The standing's name ([`STANDING`]).
    pub fn standing(&self) -> &'static str {
        let n = self.surveys();
        STANDING
            .iter()
            .find(|(most, _)| n < *most)
            .map_or("MAPMAKER", |s| s.1)
    }

    /// The ship flown ([`CRAFTS`]; a number past the table reads as
    /// the LARK, though loading and buying never make one).
    pub fn craft(&self) -> &'static Craft {
        CRAFTS.get(usize::from(self.ship)).unwrap_or(&CRAFTS[0])
    }

    /// Tonnes the hold takes (the ship's, step M5).
    pub fn capacity(&self) -> u32 {
        u32::from(self.craft().hold)
    }

    /// The ship's hull points when whole.
    pub fn hull_max(&self) -> i32 {
        i32::from(self.craft().hull)
    }

    /// What ship `i` costs here with this one traded in (negative: the
    /// station pays the difference). The list price less the trade-in
    /// ([`TRADE_IN`]): 3/4 of the flown ship's price less the cost of
    /// mending its hull.
    #[inline(never)]
    pub fn ship_cost(&self, i: usize) -> i32 {
        let Some(c) = CRAFTS.get(i) else {
            return 0;
        };
        let own = self.craft();
        // Prices below 2^14, damage below 2^8: small sums.
        let mend = (self.hull_max() - i32::from(self.hull)).max(0) * REPAIR_PRICE as i32;
        let back = (own.price * TRADE_IN.0 / TRADE_IN.1) as i32 - mend;
        c.price as i32 - back.max(0)
    }

    pub fn room(&self) -> u32 {
        self.capacity().saturating_sub(self.hold.total())
    }

    /// Whether gear item `i` ([`GEAR`]) is fitted (or something better).
    pub fn has(&self, i: usize) -> bool {
        match i {
            0 => self.laser >= 1,
            1 => self.laser >= 2,
            2 => self.shield >= 1,
            3 => self.shield >= 2,
            _ => self.scoop,
        }
    }

    fn add_credits(&mut self, n: u32) {
        self.credits = self.credits.saturating_add(n).min(CREDITS_MAX);
    }
}

/// System `id`'s byte and bit in a chart (ids past the last system
/// share the last byte: only tests ask for those).
fn chart_spot(id: u8) -> (usize, u8) {
    (usize::from(id / 8).min(CHART_BYTES - 1), 1 << (id % 8))
}

/// A chart with only system `id` surveyed.
const fn chart_bit(id: u8) -> [u8; CHART_BYTES] {
    let mut c = [0; CHART_BYTES];
    if (id as usize) < galaxy::SYSTEMS {
        c[id as usize / 8] = 1 << (id % 8);
    }
    c
}

/// The guild's fee for the first survey of system `s`: more where it
/// is rougher.
pub fn survey_fee(s: &System) -> u32 {
    // danger is 0..=3.
    u32::from(s.danger).wrapping_add(2).wrapping_mul(25)
}

/// Kinds of other ships.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Pirates: they hunt the player.
    Raider,
    /// Traders: fly to the station, run when shot at.
    Hauler,
    /// The system's patrol: hunts raiders, and the player when WANTED.
    Warden,
    /// A loose cargo canister.
    Canister,
}

impl Kind {
    /// Hull points when new.
    #[inline(never)]
    pub fn hull(self, danger: u8) -> i32 {
        match self {
            Kind::Raider => i32::from(danger).wrapping_mul(30).wrapping_add(90),
            Kind::Hauler => HAUL_HP,
            Kind::Warden => 120,
            Kind::Canister => 10,
        }
    }

    /// Speed (units a tick) and turn rate (angle steps a tick).
    #[inline(never)]
    fn drive(self) -> (i32, i32) {
        match self {
            Kind::Raider => (1000, 190),
            Kind::Hauler => (560, 110),
            Kind::Warden => (1100, 200),
            Kind::Canister => (60, 0),
        }
    }

    /// How near the middle a shot must pass to hit (world units).
    /// Step F2: a raider's is 24 m times FIGHTER_X (72 m at FIGHTER_X
    /// 3), a little over half the 126 m it had after M2 (its body ring
    /// is about 45 m, its claw tips about 150 m out), so a shot through
    /// the hull still hits and one past the claws misses, and shots land
    /// about as often as before M2.
    #[inline(never)]
    pub fn size(self) -> i32 {
        match self {
            Kind::Raider => 24 * M * FIGHTER_X,
            Kind::Hauler => 60 * M * FREIGHTER_X,
            Kind::Warden => 48 * M * FIGHTER_X,
            Kind::Canister => 18 * M * SMALL_X,
        }
    }

    /// How many times its size before M2 this kind is drawn and placed
    /// (formation places, blasts).
    #[inline(never)]
    pub fn grown(self) -> i32 {
        match self {
            Kind::Hauler => FREIGHTER_X,
            Kind::Canister => SMALL_X,
            _ => FIGHTER_X,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Flying at its target.
    Hunt,
    /// Turning away after a pass, for `timer` ticks.
    Break,
    /// Running away.
    Flee,
    /// Going about its business.
    Travel,
}

/// One of [`LANES`]: kind, from, to, fly, round, offset.
pub type Lane = (Kind, [i16; 3], [i16; 3], u16, u16, u16);

/// Step M3: the docked scene's traffic lanes, looks only. Each: the
/// kind, where it starts and ends (metres along the station's side, its
/// up and out of its mouth), the ticks it flies, the ticks of its round
/// and where in its round the first dock finds it. The camera looks from
/// about (1830, 720, 2800) (`scene::station_camera`).
pub const LANES: [Lane; 3] = [
    // Out of the mouth and away, off the camera's side.
    (
        Kind::Hauler,
        [0, 0, 700],
        [-5000, 1500, 7000],
        2400,
        3000,
        0,
    ),
    // In from far ahead, over the station and away behind it.
    (
        Kind::Hauler,
        [-5500, 1400, 5500],
        [5500, 600, -4000],
        3000,
        3600,
        1500,
    ),
    // Below it the other way: only where no warden circles (RISKY and
    // LAWLESS systems).
    (
        Kind::Hauler,
        [-6000, -900, -3500],
        [4000, -300, 6500],
        3000,
        3600,
        3000,
    ),
];
/// Step M3: the docked scene's patrol (SECURE and QUIET systems) circles
/// the station this far out (metres).
pub const PATROL_R: i32 = 2000;

/// Who a ship is after.
pub const NOBODY: u8 = 255;
pub const PLAYER: u8 = 254;

/// One other ship.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ship {
    pub kind: Kind,
    pub pos: V3,
    pub rot: Mat3,
    pub speed: i32,
    pub hp: i32,
    pub mode: Mode,
    pub timer: u16,
    pub target: u8,
    /// Ticks until it may fire again.
    pub cool: u8,
    /// A canister's load: good and units.
    pub load: (u8, u8),
    /// The player shot it.
    pub angry: bool,
    /// Ticks the "hit" flash still shows on it.
    pub flash: u8,
    /// The slot of the ship it flies in formation with, or NOBODY; and
    /// its place there (an index into `WINGS`).
    pub lead: u8,
    pub wing: u8,
    /// Looks only (step L3), from the system and the moment it came:
    /// bits 0-1 pick the hull (see `scene::model_of`), 2-3 its size,
    /// 4-5 its markings; bits 6-7 pick a raider's gun level when it
    /// comes (step M1), and the rules read it nowhere else.
    pub look: u8,
    /// A raider's gun level, 0..=2 ([`RAIDER_GUNS`]), picked by the
    /// look's bits 6-7 and the danger ([`GUN_ODDS`]); unused by the others.
    pub gun: u8,
    /// Shots it has fired since it came or came back (stops at 255).
    pub shots: u8,
}

/// An explosion: where, how old (ticks), how big (metres).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blast {
    pub pos: V3,
    pub age: u8,
    pub size: u8,
}

/// Ticks an explosion lasts.
pub const BLAST_TICKS: u8 = 48;

/// A laser beam on screen for a few ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Beam {
    pub from: V3,
    pub to: V3,
    /// Fired at the player (the rules' tests read it; the colour no
    /// longer does).
    pub foe: bool,
    pub age: u8,
    /// Step M3: the gun's level, 0..=2, picks the bolt's colour
    /// (`scene::BEAM_LV`): the player's laser (MK1..MK3), a raider's gun
    /// (`RAIDER_GUNS`), 0 for the dorsal turret, 2 for a warden (its
    /// shot is as hard as a level-3 raider's). Looks only.
    pub lv: u8,
}

/// The player's ship in flight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Me {
    pub pos: V3,
    pub rot: Mat3,
    pub speed: i32,
    /// The speed asked for, 0..= the ship's top speed (at most
    /// `MAX_SPEED * 6 / 5`).
    pub throttle: i32,
    pub cruise: bool,
    /// The auto-dock is flying.
    pub auto: bool,
    pub shield: i32,
    pub heat: i32,
    /// Too hot: the laser waits until the heat is down to half.
    pub hot: bool,
    pub cool: u8,
    /// Turn rates now: pitch, yaw, roll.
    pub rates: [i32; 3],
    /// How hard the ship has turned lately (step M1): pitch and yaw
    /// rates past [`STEADY`], averaged over about [`JINK_SPAN`] ticks.
    /// It spoils the raiders' aim.
    pub jink: i32,
    /// Ticks the "hit" flash still shows.
    pub flash: u8,
    /// Ticks the laser shows firing.
    pub firing: u8,
    /// Ticks until the dorsal turret can fire again.
    pub tcool: u8,
    /// Step M4: the overboost runs, and its charge (0..= the ship's
    /// [`Craft::od_full`]).
    pub boost: bool,
    pub od: u16,
}

/// Where the bodies of the system are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bodies {
    pub planet_r: i32,
    pub planet_rot: Mat3,
    pub sun_pos: V3,
    pub sun_r: i32,
    pub station_pos: V3,
    /// The station's turning without its spin; its nose (z) is the
    /// mouth's direction.
    pub station_rot: Mat3,
    pub spin: u16,
    /// Light direction: from the sun towards the planet, 16384 long.
    pub light: V3,
    /// Step G1: the landing site's direction and ring (None at the
    /// giant), worked out once on entering the system (from flash).
    pub site: Option<crate::planet::space::Site>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Flying,
    Docked,
    /// Blown up; the app shows the card.
    Dead,
}

/// What happened in a tick (the app shows notes and logs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Docked,
    Crash,
    /// A ship the player's shot destroyed (a raider drops its TAG).
    Killed {
        kind: Kind,
    },
    Scooped {
        good: u8,
        n: u8,
    },
    HoldFull,
    Hit,
    Overheat,
    Wanted,
    Died,
    AutoOff,
    AutoNo,
    CruiseNo,
    Fuel,
    Raiders,
    /// The first dock at a system: surveyed, the guild's fee paid.
    Surveyed,
    /// The tow lane has the ship: the auto-dock flies.
    Towed,
    /// Docked with TAGs: `sum` credits paid for them.
    Bounty {
        sum: u32,
    },
    /// Docked with TAGs, none paid: in a LAWLESS system (`lawless`), or
    /// as an OUTLAW.
    Unpaid {
        lawless: bool,
    },
}

/// The pilot's wishes for one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pad {
    /// Nose right, nose up: -256..=256.
    pub steer: (i32, i32),
    /// Bank right: -256..=256.
    pub roll: i32,
    /// Throttle change: -256..=256 (full: from stop to top in a second).
    pub throttle: i32,
    pub fire: bool,
    /// Pressed this tick: cruise on or off, auto-dock on or off.
    pub cruise: bool,
    pub auto: bool,
    /// A second player holds the dorsal turret's trigger.
    pub turret: bool,
    /// Step M4: the faster key (or the finger on the faster bars) is
    /// held: with a refused cruise press it starts the overboost, and
    /// it keeps it running.
    pub boost: bool,
}

/// A direction from a system's hash `h` (`k` picks which; 16384 long),
/// flattened toward the system's plane: where [`World::enter`] puts the
/// sun and the station, and (step G1) where the planet's landing site
/// is (`planet::space`). The same sums as before G1, moved out of
/// `enter` so the planet part can use them.
#[inline(never)]
pub fn seed_dir(h: u32, k: u32) -> V3 {
    // Each part is -1000..=1000: no overflow.
    let r = |n: u32| (hash(h, k.wrapping_mul(4).wrapping_add(n)) % 2001) as i32 - 1000;
    let v = norm([r(0), r(1) / 2, r(2)], ONE14);
    if v == [0, 0, 0] { [0, 0, ONE14] } else { v }
}

pub struct World {
    pub car: Career,
    pub sys: System,
    pub bodies: Bodies,
    pub me: Me,
    pub ships: [Option<Ship>; SHIPS],
    pub blasts: [Blast; 4],
    pub beams: [Beam; 4],
    /// Sparks, pieces and shakes: looks only, no dice (see [`fx`]).
    pub fx: Fx,
    pub state: State,
    /// The player shot a hauler or a warden here.
    pub wanted: bool,
    /// What the docked station has for sale.
    pub stock: [u8; GOODS],
    pub ticks: u32,
    spawn: u16,
    dice: Dice,
    /// Ticks the auto-dock has flown (it gives up after a while).
    auto_t: u16,
    /// The hold-full note was given for the canister in this slot.
    full_told: u8,
    events: [Option<Event>; 6],
    /// Step M3: looks only. Ticks the docked scene has played since the
    /// dock: the station turns, traffic passes ([`World::scenery`]). No
    /// rule reads it; [`World::dock`] and [`World::launch`] set it to 0.
    pub show: u32,
}

const NO_BEAM: Beam = Beam {
    from: [0; 3],
    to: [0; 3],
    foe: false,
    age: 0,
    lv: 0,
};
const NO_BLAST: Blast = Blast {
    pos: [0; 3],
    age: BLAST_TICKS,
    size: 0,
};

impl World {
    /// A world before any career: a `const`, so the chip builds it in
    /// place. Call [`World::begin`] before use.
    pub const fn empty() -> World {
        World {
            car: Career {
                seed: 0,
                system: 0,
                credits: 0,
                fuel: 0,
                hold: Hold([0; GOODS]),
                laser: 0,
                shield: 0,
                ship: LARK,
                scoop: false,
                hull: 100,
                kills: 0,
                day: 0,
                target: 0,
                won: false,
                charted: [0; CHART_BYTES],
                tags: 0,
                owed: 0,
            },
            sys: System {
                id: 0,
                x: 0,
                y: 0,
                econ: galaxy::Econ::Farm,
                danger: 0,
                tech: 1,
                look: 0,
                star: 0,
                h: 0,
                key: 0,
            },
            bodies: Bodies {
                planet_r: KM,
                planet_rot: Mat3::IDENTITY,
                sun_pos: [0, 0, 100 * KM],
                sun_r: 20 * KM,
                station_pos: [0, 0, 3 * KM],
                station_rot: Mat3::IDENTITY,
                spin: 0,
                light: [0, 0, -ONE14],
                site: None,
            },
            me: Me {
                pos: [0; 3],
                rot: Mat3::IDENTITY,
                speed: 0,
                throttle: 0,
                cruise: false,
                auto: false,
                shield: 0,
                heat: 0,
                hot: false,
                cool: 0,
                rates: [0; 3],
                jink: 0,
                flash: 0,
                firing: 0,
                tcool: 0,
                boost: false,
                od: 0,
            },
            ships: [None; SHIPS],
            blasts: [NO_BLAST; 4],
            beams: [NO_BEAM; 4],
            fx: Fx::NONE,
            state: State::Docked,
            wanted: false,
            stock: [0; GOODS],
            ticks: 0,
            spawn: 0,
            dice: Dice(1),
            auto_t: 0,
            full_told: NOBODY,
            events: [None; 6],
            show: 0,
        }
    }

    /// Starts from `car`, docked at its system's station.
    #[inline(never)]
    pub fn begin(&mut self, car: Career) {
        self.car = car;
        self.enter(car.system);
        self.dock();
        self.events = [None; 6];
    }

    /// The next thing that happened, oldest first.
    pub fn take_event(&mut self) -> Option<Event> {
        let e = self.events[0];
        for i in 1..self.events.len() {
            self.events[i - 1] = self.events[i];
        }
        self.events[self.events.len() - 1] = None;
        e
    }

    #[inline(never)]
    fn event(&mut self, e: Event) {
        if let Some(slot) = self.events.iter_mut().find(|s| s.is_none()) {
            *slot = Some(e);
        }
    }

    /// Most shield points the fitted shield holds.
    #[inline(never)]
    pub fn shield_max(&self) -> i32 {
        // At most 220 x 8 / 4: small.
        SHIELDS[usize::from(self.car.shield.min(2))].0 * i32::from(self.car.craft().shield) / 4
    }

    /// Step G1, for the tests only (nothing in the game calls it, so
    /// the chip's linker drops it): a field-by-field copy, private
    /// fields too, so a test can check that space comes back from the
    /// planet unchanged.
    #[doc(hidden)]
    pub fn copy_for_test(&self) -> World {
        World {
            car: self.car,
            sys: self.sys,
            bodies: self.bodies,
            me: self.me,
            ships: self.ships,
            blasts: self.blasts,
            beams: self.beams,
            fx: self.fx,
            state: self.state,
            wanted: self.wanted,
            stock: self.stock,
            ticks: self.ticks,
            spawn: self.spawn,
            dice: self.dice,
            auto_t: self.auto_t,
            full_told: self.full_told,
            events: self.events,
            show: self.show,
        }
    }

    /// Step G1, for the tests only: whether every field but the
    /// player's ship (`me`) and the career's fuel is the same.
    #[doc(hidden)]
    pub fn same_but_ship(&self, o: &World) -> bool {
        let mut a = self.car;
        a.fuel = o.car.fuel;
        a == o.car
            && self.sys == o.sys
            && self.bodies == o.bodies
            && self.ships == o.ships
            && self.blasts == o.blasts
            && self.beams == o.beams
            && self.fx == o.fx
            && self.state == o.state
            && self.wanted == o.wanted
            && self.stock == o.stock
            && self.ticks == o.ticks
            && self.spawn == o.spawn
            && self.dice == o.dice
            && self.auto_t == o.auto_t
            && self.full_told == o.full_told
            && self.events == o.events
            && self.show == o.show
    }

    /// Step M5: the ship's top speed without cruise or overboost.
    pub fn top(&self) -> i32 {
        self.car.craft().top
    }

    /// Step M5: the ship's full overboost charge and overboost top speed.
    pub fn od_full(&self) -> u16 {
        self.car.craft().od_full
    }

    pub fn od_top(&self) -> i32 {
        let c = self.car.craft();
        c.top * i32::from(c.od_top) / 4
    }

    /// Sets up system `id`: its bodies, no ships; the player at the
    /// arrival point.
    #[inline(never)]
    fn enter(&mut self, id: u8) {
        let s = galaxy::system(self.car.seed, id);
        self.sys = s;
        self.car.system = id;
        self.wanted = false;
        self.auto_t = 0;
        self.ships = [None; SHIPS];
        self.blasts = [NO_BLAST; 4];
        self.beams = [NO_BEAM; 4];
        self.fx = Fx::NONE;
        self.dice = Dice(hash(s.h, u32::from(self.car.day)));
        let h = s.h;
        // All sizes below are far inside i32 (the largest, the sun's
        // distance, below 2^27): written without checks.
        let pick = |k: u32, n: u32| (hash(h, k) % n) as i32;
        let dir = |k: u32| seed_dir(h, k);
        let giant = s.look == 4;
        let mut pr = pick(1, 256).wrapping_mul(1800).wrapping_add(563_200);
        if giant {
            pr = pr.wrapping_mul(3) / 2;
        }
        let sun_dir = dir(1);
        let sun_d = pick(2, 30_000).wrapping_mul(1000).wrapping_add(70_000_000);
        let axis = dir(2);
        let b = &mut self.bodies;
        b.planet_r = pr;
        b.planet_rot = Mat3::rotation((h >> 7) as u16, ((h % 5000) as u16).wrapping_add(2000), 0);
        b.sun_pos = scale14(sun_dir, sun_d);
        b.sun_r = pick(3, 4096).wrapping_mul(1000).wrapping_add(5_000_000);
        b.station_pos = scale14(axis, pr.wrapping_mul(12) / 5);
        b.station_rot = Mat3::looking(axis, [0, ONE14, 0]);
        b.light = sun_dir.map(i32::wrapping_neg);
        b.site = crate::planet::space::locate(h, s.look, b);
        // Arrive 30 km out, on the sunny side of the station (unit
        // vectors: parts within 2^14).
        let out = norm(add(add(axis, axis), sun_dir), ONE14);
        let at = add(b.station_pos, scale14(out, 30 * KM));
        let st = b.station_pos;
        self.me = Me {
            pos: at,
            rot: Mat3::looking(sub(st, at), [0, ONE14, 0]),
            speed: MAX_SPEED / 2,
            throttle: MAX_SPEED / 2,
            cruise: false,
            auto: false,
            shield: self.shield_max(),
            heat: 0,
            hot: false,
            cool: 0,
            rates: [0; 3],
            jink: 0,
            flash: 0,
            firing: 0,
            tcool: 0,
            boost: false,
            od: self.od_full(),
        };
        self.state = State::Flying;
        self.spawn = 240;
        // Some life at once: a hauler on its way in, a warden by the
        // station in safer systems.
        self.add_ship(Kind::Hauler, add(at, scale14(out, -4 * KM)), Mode::Travel);
        if s.danger < 2 {
            let p = add(st, scale14(axis, 3 * KM));
            self.add_ship(Kind::Warden, p, Mode::Travel);
        }
    }

    /// Docks: hull and stock set, cruise and auto off.
    #[inline(never)]
    fn dock(&mut self) {
        self.state = State::Docked;
        self.me.cruise = false;
        self.me.auto = false;
        self.me.speed = 0;
        self.me.heat = 0;
        self.me.hot = false;
        self.me.shield = self.shield_max();
        self.ships = [None; SHIPS];
        self.fx = Fx::NONE;
        // Step M3: the docked scene starts afresh, without the flight's
        // last shots and blasts (looks only).
        self.blasts = [NO_BLAST; 4];
        self.beams = [NO_BEAM; 4];
        self.show = 0;
        for g in 0..GOODS {
            self.stock[g] = market::stock(&self.sys, g, self.car.day);
        }
        // Nobody is stranded: a pilot with an empty hold and less than a
        // tank's price gets the tank filled free.
        let c = &mut self.car;
        if c.hold.total() == 0 && c.credits < u32::from(TANK) * FUEL_PRICE {
            c.fuel = TANK;
        }
    }

    /// The first dock at a system surveys it: the guild pays its fee.
    #[inline(never)]
    fn survey(&mut self) {
        let (i, m) = chart_spot(self.car.system);
        if self.car.charted[i] & m == 0 {
            self.car.charted[i] |= m;
            self.car.add_credits(survey_fee(&self.sys));
            self.event(Event::Surveyed);
        }
    }

    /// At a dock with TAGs: the bounty office pays what they are worth,
    /// unless the system is LAWLESS ([`LAWLESS_PAYS`]) or the player is
    /// an OUTLAW here; then the TAGs are kept.
    #[inline(never)]
    fn pay(&mut self) {
        let c = &mut self.car;
        if c.tags == 0 {
            return;
        }
        let lawless = !LAWLESS_PAYS && self.sys.danger >= 3;
        let e = if lawless || self.wanted {
            Event::Unpaid { lawless }
        } else {
            let sum = u32::from(c.owed);
            c.add_credits(sum);
            c.tags = 0;
            c.owed = 0;
            Event::Bounty { sum }
        };
        self.event(e);
    }

    /// Leaves the station: out of the mouth at 100 m/s.
    #[inline(never)]
    pub fn launch(&mut self) {
        // Step M3: the docked scene's traffic goes (it is looks only), so
        // the flight starts exactly as before.
        self.ships = [None; SHIPS];
        self.show = 0;
        let b = self.bodies;
        let axis = b.station_rot.col(2);
        // Step F1: farther out by the chase camera's growth, so its eye
        // starts where it did (570 m out), not inside the station.
        self.me.pos = add(
            b.station_pos,
            scale14(axis, 160 * M * STATION_X + 70 * M * (ME_X - 1)),
        );
        self.me.rot = b.station_rot;
        self.me.speed = 400;
        self.me.throttle = 400;
        self.me.rates = [0; 3];
        self.me.shield = self.shield_max();
        self.me.boost = false;
        self.me.od = self.od_full();
        self.state = State::Flying;
        self.spawn = 600;
        self.auto_t = 0;
        // The press that launched does not fire (half a second).
        self.me.cool = 30;
        // Wardens near the station when it is safe.
        if self.sys.danger < 2 {
            let p = add(b.station_pos, scale14(axis, 2 * KM));
            self.add_ship(Kind::Warden, p, Mode::Travel);
        }
        #[cfg(merc_site_test)]
        self.site_test_start();
    }

    /// MEASURING ONLY (`--cfg merc_site_test`, BOARD-G1.md instruction 1):
    /// the flight starts on the site ring's axis 1 km above the ring
    /// (2.5 km above the site), nose down at it, at half the top speed,
    /// with no other ship; with no key pressed it crosses the ring in
    /// about 8 s. Never in a build handed over for use.
    #[cfg(merc_site_test)]
    #[inline(never)]
    fn site_test_start(&mut self) {
        let Some(s) = self.bodies.site else {
            return;
        };
        let (e1, _) = crate::planet::space::across(s.dir);
        self.me.pos = add(s.ring, scale14(s.dir, KM));
        self.me.rot = Mat3::looking(scale14(s.dir, -ONE14), e1);
        self.me.speed = self.top() / 2;
        self.me.throttle = self.me.speed;
        self.ships = [None; SHIPS];
        self.spawn = 1800;
    }

    /// Step M3: one tick of the docked scene, looks only. The station
    /// turns (the scene adds `show` to its spin) and traffic passes:
    /// ships in the traffic slots placed by `show` alone ([`LANES`], a
    /// warden circling where it is safe). Nothing else changes: no rule
    /// time passes, no dice are rolled, and [`World::launch`] takes the
    /// traffic away, so the flight starts as if the dock had been still.
    #[inline(never)]
    pub fn scenery(&mut self) {
        if !fx::DOCK_LIVE_ON || self.state != State::Docked {
            return;
        }
        self.show = self.show.wrapping_add(1);
        let b = self.bodies;
        let r = b.station_rot;
        // Station metres (side, up, out of the mouth; within 2^14) to
        // the world: within 2^22 before the turn, inside i32.
        let world = |p: V3| add(b.station_pos, r.apply(p.map(|c| c.wrapping_mul(M))));
        let safe = self.sys.danger < 2;
        for (k, &(kind, from, to, fly, round, off)) in LANES.iter().enumerate() {
            let slot = FIGHT + k;
            self.ships[slot] = None;
            // The third lane only where no warden circles.
            let t = self.show.wrapping_add(u32::from(off));
            let (n, p) = (t / u32::from(round), t % u32::from(round));
            if p >= u32::from(fly) || (k == 2 && safe) {
                continue;
            }
            let (f, way) = (
                from.map(i32::from),
                sub(to.map(i32::from), from.map(i32::from)),
            );
            // Metres within 2^14 times p below 2^12: inside i32.
            let at = [0, 1, 2]
                .map(|c| f[c].wrapping_add(way[c].wrapping_mul(p as i32) / i32::from(fly)));
            self.put_ship(slot, kind, world(at), Mode::Travel);
            if let Some(s) = &mut self.ships[slot] {
                s.rot = Mat3::looking(r.apply(way), r.col(1));
                s.speed = length(way).wrapping_mul(M) / i32::from(fly);
                s.look = hash(
                    self.sys.h ^ 0x5C3E,
                    n.wrapping_mul(4).wrapping_add(k as u32),
                ) as u8;
            }
        }
        let slot = FIGHT + LANES.len();
        self.ships[slot] = None;
        if safe {
            // A turn in 4096 ticks (68 s), PATROL_R out, 250 m up.
            let a = self.show.wrapping_mul(16) as u16;
            let (sn, cs) = (sin(a), cos(a));
            let at = [
                sn.wrapping_mul(PATROL_R) >> 14,
                250,
                cs.wrapping_mul(PATROL_R) >> 14,
            ];
            self.put_ship(slot, Kind::Warden, world(at), Mode::Travel);
            if let Some(s) = &mut self.ships[slot] {
                s.rot = Mat3::looking(r.apply([cs, 0, sn.wrapping_neg()]), r.col(1));
                // 2 pi PATROL_R metres in 4096 ticks.
                s.speed = PATROL_R * M / 652;
            }
        }
    }

    /// Distance from the player to the station's middle.
    #[inline(never)]
    pub fn station_dist(&self) -> i32 {
        length(sub(self.bodies.station_pos, self.me.pos))
    }

    /// Whether a jump may start now: not docked, not near the station
    /// or the planet. `Err` holds the reason for the note.
    #[inline(never)]
    pub fn can_jump(&self, to: u8) -> Result<u32, &'static str> {
        if self.state != State::Flying {
            return Err("LAUNCH FIRST");
        }
        let d = self.sys.dist(&galaxy::system(self.car.seed, to));
        if to == self.car.system {
            return Err("ALREADY HERE");
        }
        if d > u32::from(self.car.fuel) {
            return Err("NOT ENOUGH FUEL");
        }
        // Radii below 2^24.
        if self.station_dist() < 5 * KM {
            return Err("TOO NEAR THE STATION");
        }
        if length(self.me.pos) < self.bodies.planet_r.wrapping_add(3 * KM) {
            return Err("TOO NEAR THE PLANET");
        }
        Ok(d)
    }

    /// The jump itself (the app shows the tunnel first): fuel used, a
    /// day passes, the new system is entered.
    #[inline(never)]
    pub fn jump(&mut self, to: u8) {
        let d = self.sys.dist(&galaxy::system(self.car.seed, to));
        self.car.fuel = self.car.fuel.saturating_sub(d.min(255) as u8);
        self.car.day = self.car.day.saturating_add(1);
        self.enter(to);
        self.fx.warp = fx::WARP_TICKS;
    }

    // ---- Trading and gear (docked) ----------------------------------------------

    #[inline(never)]
    pub fn price(&self, g: usize) -> u32 {
        market::price(&self.sys, g, self.car.day)
    }

    #[inline(never)]
    pub fn sell_price(&self, g: usize) -> u32 {
        market::sell_price(&self.sys, g, self.car.day)
    }

    /// Buys one unit of good `g`. False when it cannot.
    #[inline(never)]
    pub fn buy(&mut self, g: usize) -> bool {
        // The good is checked before its price is looked up (the price
        // table has GOODS entries).
        if self.state != State::Docked || g >= GOODS || self.stock[g] == 0 || self.car.room() == 0 {
            return false;
        }
        let p = self.price(g);
        if self.car.credits < p {
            return false;
        }
        // Checked above: credits >= p, room and stock above 0 (a hold
        // of at most HOLD_MOST units).
        self.car.credits = self.car.credits.wrapping_sub(p);
        self.car.hold.0[g] = self.car.hold.0[g].wrapping_add(1);
        self.stock[g] = self.stock[g].wrapping_sub(1);
        true
    }

    /// Sells one unit of good `g`. False when there is none.
    #[inline(never)]
    pub fn sell(&mut self, g: usize) -> bool {
        if self.state != State::Docked || g >= GOODS || self.car.hold.0[g] == 0 {
            return false;
        }
        let p = self.sell_price(g);
        // Checked above: at least one in the hold.
        self.car.hold.0[g] = self.car.hold.0[g].wrapping_sub(1);
        self.car.add_credits(p);
        self.stock[g] = self.stock[g].saturating_add(1);
        true
    }

    /// Fills the tank as far as the money goes. Credits spent.
    #[inline(never)]
    pub fn buy_fuel(&mut self) -> u32 {
        let want = u32::from(TANK - self.car.fuel.min(TANK));
        // n is at most the room in the tank and what the credits buy.
        let n = want.min(self.car.credits / FUEL_PRICE);
        self.car.fuel = self.car.fuel.wrapping_add(n as u8);
        let cost = n.wrapping_mul(FUEL_PRICE);
        self.car.credits = self.car.credits.wrapping_sub(cost);
        cost
    }

    /// Mends the hull as far as the money goes. Credits spent.
    #[inline(never)]
    pub fn repair(&mut self) -> u32 {
        let want = (self.car.hull_max() as u32).saturating_sub(u32::from(self.car.hull));
        // n is at most the damage and what the credits buy.
        let n = want.min(self.car.credits / REPAIR_PRICE);
        self.car.hull = self.car.hull.wrapping_add(n as u8);
        let cost = n.wrapping_mul(REPAIR_PRICE);
        self.car.credits = self.car.credits.wrapping_sub(cost);
        cost
    }

    /// Whether gear item `i` is sold here.
    #[inline(never)]
    pub fn sold_here(&self, i: usize) -> bool {
        i < GEAR.len() && self.sys.tech >= GEAR[i].2
    }

    /// Buys gear item `i`. `Err` holds the reason.
    #[inline(never)]
    pub fn buy_gear(&mut self, i: usize) -> Result<(), &'static str> {
        if i >= GEAR.len() {
            return Err("?");
        }
        let (_, price, _) = GEAR[i];
        if self.car.has(i) {
            return Err("ALREADY FITTED");
        }
        if !self.sold_here(i) {
            return Err("NOT SOLD HERE");
        }
        if self.car.credits < price {
            return Err("NOT ENOUGH CREDITS");
        }
        // Checked above.
        self.car.credits = self.car.credits.wrapping_sub(price);
        match i {
            0 => self.car.laser = 1,
            1 => self.car.laser = 2,
            2 => self.car.shield = 1,
            3 => self.car.shield = 2,
            _ => self.car.scoop = true,
        }
        self.me.shield = self.shield_max();
        Ok(())
    }

    /// Whether ship `i` ([`CRAFTS`]) is sold here.
    #[inline(never)]
    pub fn ship_sold_here(&self, i: usize) -> bool {
        CRAFTS.get(i).is_some_and(|c| self.sys.tech >= c.tech)
    }

    /// Step M5: buys ship `i` with the flown one traded in
    /// ([`Career::ship_cost`]). The laser, shield, sun skimmer, fuel,
    /// cargo and TAGs move over; the new ship's hull is whole. `Err`
    /// holds the reason.
    #[inline(never)]
    pub fn buy_ship(&mut self, i: usize) -> Result<(), &'static str> {
        let Some(c) = CRAFTS.get(i) else {
            return Err("?");
        };
        if self.state != State::Docked {
            return Err("?");
        }
        if usize::from(self.car.ship) == i {
            return Err("ALREADY FLYING IT");
        }
        if !self.ship_sold_here(i) {
            return Err("NOT SOLD HERE");
        }
        if self.car.hold.total() > u32::from(c.hold) {
            return Err("SELL CARGO FIRST");
        }
        let cost = self.car.ship_cost(i);
        if cost > 0 && self.car.credits < cost as u32 {
            return Err("NOT ENOUGH CREDITS");
        }
        if cost >= 0 {
            // Checked above.
            self.car.credits = self.car.credits.wrapping_sub(cost as u32);
        } else {
            self.car.add_credits(cost.unsigned_abs());
        }
        self.car.ship = i as u8;
        self.car.hull = c.hull;
        self.me.shield = self.shield_max();
        self.me.od = self.od_full();
        Ok(())
    }

    // ---- Flight -------------------------------------------------------------------

    /// Whether another ship is hunting the player within `r` (step G1:
    /// the planet ring's raider rule asks it too).
    #[inline(never)]
    pub(crate) fn hunted_within(&self, r: i32) -> bool {
        self.ships
            .iter()
            .flatten()
            .any(|s| s.target == PLAYER && s.mode == Mode::Hunt && within(s.pos, self.me.pos, r))
    }

    /// The room before cruise must stop: the nearest of the station
    /// (3 km), the planet's surface (2 km) and the sun (2 radii). Below
    /// zero: no cruise.
    #[inline(never)]
    pub fn room(&self) -> i32 {
        // Distances below 2^30, body radii below 2^24: no overflow.
        let b = &self.bodies;
        let st = self.station_dist().wrapping_sub(3 * KM);
        let pl = length(self.me.pos).wrapping_sub(b.planet_r.wrapping_add(2 * KM));
        let sun = length(sub(b.sun_pos, self.me.pos)).wrapping_sub(b.sun_r.wrapping_mul(2));
        st.min(pl).min(sun)
    }

    /// One tick.
    #[inline(never)]
    pub fn tick(&mut self, pad: &Pad) {
        self.ticks = self.ticks.wrapping_add(1);
        self.bodies.spin = self.bodies.spin.wrapping_add(30);
        for b in self.blasts.iter_mut() {
            b.age = b.age.saturating_add(1).min(BLAST_TICKS);
        }
        for b in self.beams.iter_mut() {
            b.age = b.age.saturating_sub(1);
        }
        self.fx.tick();
        match self.state {
            State::Flying => {}
            State::Dead => {
                self.npcs();
                return;
            }
            State::Docked => return,
        }
        self.fly(pad);
        if self.state != State::Flying {
            return;
        }
        self.heat_and_shield();
        self.fire(pad.fire);
        self.turret(pad.turret);
        self.npcs();
        self.pick_up();
        self.spawner();
        if self.car.hull == 0 {
            self.die();
        }
    }

    #[inline(never)]
    fn fly(&mut self, pad: &Pad) {
        let me = self.me;
        // Pad values are within +-256 and the throttle within 0..=1536:
        // the products and sums are far inside i32 (checked in debug).
        debug_assert!(
            [pad.steer.0, pad.steer.1, pad.roll, pad.throttle]
                .iter()
                .all(|v| v.abs() <= 256)
        );
        let m = |v: i32, k: i32| v.wrapping_mul(k) / 256;
        // Step M5: the ship's own turn rates and top speed.
        let craft = self.car.craft();
        let top = craft.top;
        let [kp, ky, kr] = craft.turn;
        let mut steer = [m(pad.steer.1, kp), m(pad.steer.0, ky), m(pad.roll, kr)];
        let mut throttle = me.throttle.wrapping_add(m(pad.throttle, 22)).clamp(0, top);
        let mut cruise = me.cruise;
        let mut auto = me.auto;
        let room = self.room();
        let hunted = self.hunted_within(6 * KM);
        // Step M4: the overboost runs on while the key is held.
        let mut boost = me.boost && pad.boost;
        if pad.cruise {
            if cruise {
                cruise = false;
            } else if room <= 0 || hunted {
                // Where cruise is refused, the held press overboosts.
                if pad.boost && me.od >= OD_MIN {
                    boost = true;
                } else {
                    self.event(Event::CruiseNo);
                }
            } else {
                cruise = true;
            }
        }
        if pad.auto {
            if auto {
                auto = false;
                self.event(Event::AutoOff);
            } else if hunted {
                self.event(Event::AutoNo);
            } else {
                auto = true;
                self.auto_t = 0;
            }
        }
        if !auto && !hunted {
            auto = self.towed();
        }
        if auto && (hunted || self.auto_t > 60 * 90) {
            auto = false;
            self.event(Event::AutoOff);
        }
        if auto {
            // At most 90 s (5,400 ticks), so the count stays in a u16.
            self.auto_t = self.auto_t.wrapping_add(1);
            let (aim, speed, far) = self.auto_aim();
            let (p, y) = aim_at(&me.rot, me.pos, aim, 240);
            steer = [p, y, 0];
            throttle = speed.min(top);
            cruise = room > 0 && far;
        }
        if cruise && (room <= 0 || hunted) {
            cruise = false;
        }
        // Step M4: on until the charge or the key gives out; never with
        // cruise or the auto-dock. Half the turn rate meanwhile. The
        // charge stays within 0..= the ship's full charge.
        let boost = boost && !cruise && !auto && me.od > 0;
        let od = if boost {
            steer = steer.map(|t| t >> 1);
            me.od.saturating_sub(3)
        } else {
            me.od.wrapping_add(1).min(craft.od_full)
        };
        // Turning eases in and out.
        let mut rates = me.rates;
        // Rates and steers are within +-420.
        for (r, t) in rates.iter_mut().zip(steer) {
            *r = r.wrapping_add(t.wrapping_sub(*r) / 3);
            if t.wrapping_sub(*r).unsigned_abs() < 3 {
                *r = t;
            }
        }
        let rot = if rates == [0; 3] {
            me.rot
        } else {
            me.rot.turned(rates[0], rates[1], rates[2])
        };
        // Speeds and the throttle are within 0..=CRUISE_MAX (64,000).
        let v = me.speed;
        let speed = if cruise {
            let want = (room / 192).clamp(MAX_SPEED, CRUISE_MAX);
            v.wrapping_add(want.wrapping_sub(v) / 12)
        } else if boost {
            let od_top = top * i32::from(craft.od_top) / 4;
            v.wrapping_add(od_top.wrapping_sub(v).clamp(-48, 48))
        } else if v > top {
            // Out of cruise: down to normal speed quickly.
            v.wrapping_sub(v.wrapping_sub(throttle) / 6)
                .wrapping_sub(1)
                .max(throttle)
        } else {
            v.wrapping_add(throttle.wrapping_sub(v).clamp(-24, 24))
        };
        let speed = speed.clamp(0, CRUISE_MAX);
        let pos = clamp_pos(add(me.pos, scale14(rot.col(2), speed)));
        self.me.rot = rot;
        self.me.rates = rates;
        // Rates within +-420: small sums.
        let turn = rates[0]
            .unsigned_abs()
            .wrapping_add(rates[1].unsigned_abs())
            .saturating_sub(STEADY) as i32;
        self.me.jink = self
            .me
            .jink
            .wrapping_add(turn.wrapping_sub(self.me.jink) / JINK_SPAN);
        self.me.speed = speed;
        self.me.throttle = throttle;
        self.me.cruise = cruise;
        self.me.auto = auto;
        self.me.boost = boost;
        self.me.od = od;
        self.me.pos = pos;
        self.collide();
    }

    /// The tow lane: 800 m to 3 km in front of the station's middle
    /// (step M2: 200 m and 120 m times STATION_X, out to the approach
    /// point), within 480 m of its axis, facing in and not faster than top speed, the
    /// station's tow takes the ship in. Once a flight: not after the
    /// pilot has used the auto-dock or turned it off.
    #[inline(never)]
    fn towed(&mut self) -> bool {
        let r = self.bodies.station_rot;
        // In the station's axes.
        let l = r.apply_t(sub(self.me.pos, self.bodies.station_pos));
        let nose = r.apply_t(self.me.rot.col(2));
        let on = self.auto_t == 0
            && self.me.speed <= self.top()
            && l[2] > 200 * M * STATION_X
            && l[2] < APPROACH
            && l[0].unsigned_abs() < (120 * M * STATION_X) as u32
            && l[1].unsigned_abs() < (120 * M * STATION_X) as u32
            && nose[2] < -ONE14 * 9 / 10;
        if on {
            self.event(Event::Towed);
        }
        on
    }

    /// The auto-dock's aim, speed, and whether the gate is still far
    /// (more than 6 km: cruise allowed). First to a point in front of
    /// the mouth, then in; round the station and the planet, never
    /// through them. Positions are inside `LIMIT` (2^29) and the planet
    /// and station sizes below 2^27, so the sums below fit.
    #[inline(never)]
    fn auto_aim(&self) -> (V3, i32, bool) {
        let me = &self.me;
        let b = &self.bodies;
        let axis = b.station_rot.col(2);
        let gate = add(b.station_pos, scale14(axis, APPROACH));
        let to_gate = length(sub(gate, me.pos));
        // Where the ship is against the mouth's plane and its axis.
        let rel = sub(me.pos, b.station_pos);
        let along = (dot(rel, axis) >> 14) as i32;
        let in_front = along > 0 && dot(me.rot.col(2), axis) < -wide(ONE14, ONE14) * 9 / 10;
        let perp = sub(rel, scale14(axis, along));
        let off = length(perp);
        // On the last stretch into the mouth (near the axis, in front):
        // straight in. Step M2: the station's distances times STATION_X
        // (the slow part only within 1.2 km, so the way in stays short).
        const S: i32 = M * STATION_X;
        let last = along > 60 * S && off < 60 * S;
        let (aim, speed) = if along < 150 * S && !last {
            // Behind or beside the station: first straight out to the
            // side, then round to the front.
            let out = if off < 64 * S {
                b.station_rot.col(0)
            } else {
                norm(perp, ONE14)
            };
            // (Half as far out as the station's size times STATION_X: the
            // wing tips are 1.1 km out, the turn 3 km.)
            let side = add(b.station_pos, scale14(out, 750 * S));
            let ahead = if off >= 600 * S { 600 * S } else { along };
            (add(side, scale14(axis, ahead)), MAX_SPEED)
        } else if to_gate < APPROACH / 3 || (in_front && to_gate < APPROACH) {
            // In: to a point on the axis half way to the mouth, so the
            // way bends onto the axis before the mouth.
            // Step F2: the speed falls with the way left along the axis
            // (1/1000 of it, so 1280 at 1.28 M units, i.e. 5 km, and
            // 300 from 1.2 km in), instead of 420 all the way in from up
            // to 6 km out. A plain division and clamp: no overflow. The
            // speed's own limit (24 units a tick per tick) keeps the
            // slowing smooth.
            let slow = (along / 1000).clamp(300, MAX_SPEED);
            (add(b.station_pos, scale14(axis, along / 2)), slow)
        } else {
            (gate, MAX_SPEED)
        };
        // Round the planet: when the way to the aim passes near the
        // planet's middle (the origin), aim high over the way's nearest
        // spot instead.
        let to = sub(aim, me.pos);
        let dir = norm(to, ONE14);
        let s = (dot(me.pos, dir) >> 14).wrapping_neg() as i32;
        let safe = b.planet_r.wrapping_add(2 * KM);
        let near = add(me.pos, scale14(dir, s));
        let aim = if s > 0 && s < length(to) && length(near) < safe {
            let up = norm(near, ONE14);
            let up = if up == [0; 3] {
                b.station_rot.col(0)
            } else {
                up
            };
            scale14(up, safe.wrapping_add(KM))
        } else {
            aim
        };
        (aim, speed, to_gate > 6 * KM)
    }

    /// The station (dock or crash), the planet and the sun.
    #[inline(never)]
    fn collide(&mut self) {
        let b = self.bodies;
        let rel = sub(self.me.pos, b.station_pos);
        // Step M2: every station size times STATION_X (S below).
        const S: i32 = M * STATION_X;
        if within(self.me.pos, b.station_pos, 400 * S) {
            // In the station's own axes (STATION_U per table unit).
            use crate::models::{HUB_FRONT, MOUTH_H, MOUTH_W, MOUTH_Z};
            let l = b.station_rot.apply_t(rel);
            let mouth = i32::from(MOUTH_Z) * STATION_U;
            let front = i32::from(HUB_FRONT) * STATION_U;
            let r2 = wide(l[0], l[0]).wrapping_add(wide(l[1], l[1]));
            let facing =
                dot(self.me.rot.col(2), b.station_rot.col(2)) < -wide(ONE14, ONE14) * 8 / 10;
            // The door: the mouth's opening less 20 m all round (the
            // courier is 41 m wide), so a ship that docks never touches
            // its edge.
            let door = |half: i16| (i32::from(half) * STATION_U).wrapping_sub(20 * M) as u32;
            if l[2] > front
                && l[2] < mouth.wrapping_add(30 * S)
                && l[0].unsigned_abs() < door(MOUTH_W)
                && l[1].unsigned_abs() < door(MOUTH_H)
            {
                if facing && self.me.speed <= 480 {
                    self.dock();
                    self.event(Event::Docked);
                    self.survey();
                    self.pay();
                    return;
                }
                self.crash(rel);
                return;
            }
            // Step F1: the hub with the courier's half span round it (the
            // door above is tested first, so docking is unchanged).
            const HUB: i32 = 104 * S + ME_R;
            let hub =
                l[2] > -30_000 * STATION_X - ME_R && l[2] < mouth + ME_R && r2 < wide(HUB, HUB);
            let wing = l[1].unsigned_abs() < (6 * S) as u32
                && l[0].unsigned_abs() > (96 * S) as u32
                && l[0].unsigned_abs() < (270 * S) as u32
                && l[2] > -70 * S
                && l[2] < 33 * S;
            if hub || wing {
                // Off a wing: pushed out across it (straight out from
                // the middle would leave the ship inside the wing).
                let up = b.station_rot.col(1);
                let away = match (wing, l[1] < 0) {
                    (false, _) => rel,
                    (true, false) => up,
                    (true, true) => up.map(i32::wrapping_neg),
                };
                self.crash(away);
                return;
            }
        }
        // Radii below 2^24, distances below 2^30.
        let alt = length(self.me.pos).wrapping_sub(b.planet_r);
        if alt < 60 * M {
            self.crash(self.me.pos);
            let up = b.planet_r.wrapping_add(400 * M);
            self.me.pos = clamp_pos(scale14(norm(self.me.pos, ONE14), up));
        }
        let sun = length(sub(self.me.pos, b.sun_pos));
        if sun < b.sun_r.wrapping_add(b.sun_r / 10) {
            self.car.hull = 0;
        }
    }

    /// Hit something: damage by speed, stopped, pushed out along `away`.
    #[inline(never)]
    fn crash(&mut self, away: V3) {
        // Speeds within 0..=CRUISE_MAX.
        let dmg = (self.me.speed / 40).wrapping_add(15);
        self.damage(dmg, true);
        self.me.speed = 0;
        self.me.throttle = 0;
        self.me.cruise = false;
        self.me.auto = false;
        self.me.pos = clamp_pos(add(self.me.pos, scale14(norm(away, ONE14), 30 * M)));
        self.fx.jolt = 16;
        self.fx.sparks(self.me.pos, 6, 5 * M, 12);
        self.event(Event::Crash);
    }

    /// Damage to the player: the shield first (unless `hull_only`).
    #[inline(never)]
    fn damage(&mut self, n: i32, hull_only: bool) {
        // Damage below 2^16, shields 0..=440, hull 0..=HULL_MOST.
        let mut n = n.max(0);
        if !hull_only {
            let s = self.me.shield.min(n).max(0);
            self.me.shield = self.me.shield.wrapping_sub(s);
            n = n.wrapping_sub(s);
        }
        let most = self.car.hull_max();
        self.car.hull = i32::from(self.car.hull).wrapping_sub(n).clamp(0, most) as u8;
        self.me.flash = 8;
    }

    #[inline(never)]
    fn heat_and_shield(&mut self) {
        let b = self.bodies;
        let d = length(sub(self.me.pos, b.sun_pos));
        let r = b.sun_r;
        // Sun radii below 2^24, heat within 0..=HEAT_MAX: no overflow.
        let r3 = r.wrapping_mul(3);
        let mut heat = self.me.heat.wrapping_sub(3);
        if d < r3 {
            let t = (wide(r3.wrapping_sub(d), 1024) / i64::from(r.wrapping_mul(2)).max(1)) as i32;
            heat = heat.wrapping_add(t / 96);
            if self.car.scoop
                && d < r.wrapping_mul(12) / 5
                && self.me.speed > 0
                && self.ticks.is_multiple_of(6)
                && self.car.fuel < TANK
            {
                self.car.fuel = self.car.fuel.wrapping_add(1);
                if self.car.fuel == TANK {
                    self.event(Event::Fuel);
                }
            }
        }
        self.me.heat = heat.clamp(0, HEAT_MAX);
        if self.me.heat >= HEAT_MAX {
            if !self.me.hot {
                self.event(Event::Overheat);
            }
            self.me.hot = true;
            if self.ticks.is_multiple_of(4) {
                self.damage(1, true);
            }
        } else if self.me.heat < HEAT_MAX / 2 {
            self.me.hot = false;
        }
        let every = SHIELDS[usize::from(self.car.shield.min(2))].1;
        let most = self.shield_max();
        if self.me.shield < most && self.ticks.is_multiple_of(every) {
            self.me.shield = self.me.shield.wrapping_add(1);
        }
        self.me.shield = self.me.shield.max(0).min(most);
        self.me.flash = self.me.flash.saturating_sub(1);
        self.me.firing = self.me.firing.saturating_sub(1);
    }

    /// The player's laser.
    #[inline(never)]
    fn fire(&mut self, fire: bool) {
        self.me.cool = self.me.cool.saturating_sub(1);
        // Step M4: no shot while the overboost runs.
        if !fire || self.me.cool > 0 || self.me.hot || self.me.boost {
            return;
        }
        let (dmg, heat, every) = LASER[usize::from(self.car.laser.min(2))];
        self.me.cool = every;
        // Heat within 0..=HEAT_MAX, a shot's heat below 100.
        self.me.heat = self.me.heat.wrapping_add(heat).min(HEAT_MAX);
        self.me.firing = 4;
        let nose = self.me.rot.col(2);
        let hit = self.first_hit();
        let reach = match hit {
            Some((i, d)) => {
                self.hurt(i, dmg, true);
                d
            }
            None => LASER_RANGE,
        };
        let to = add(self.me.pos, scale14(nose, reach));
        let gun = add(self.me.pos, scale14(self.me.rot.col(1), -3 * M));
        self.beam(gun, to, false, self.car.laser);
        if hit.is_some() {
            self.fx.sparks(to, 5, 6 * M, 12);
        }
    }

    /// The dorsal turret, a second player's gun: it aims by itself, any
    /// way round, at a raider or a ship the player has shot, within
    /// [`TURRET_RANGE`]. It shares the laser's heat.
    #[inline(never)]
    fn turret(&mut self, on: bool) {
        self.me.tcool = self.me.tcool.saturating_sub(1);
        if !on || self.me.tcool > 0 || self.me.hot || self.me.boost {
            return;
        }
        // The first ship in the list that qualifies: it stays the target
        // while it lives and stays in reach.
        let Some((i, to)) = self.ships.iter().enumerate().find_map(|(i, s)| {
            s.filter(|s| {
                (s.kind == Kind::Raider || (s.angry && s.kind != Kind::Canister))
                    && within(s.pos, self.me.pos, TURRET_RANGE)
            })
            .map(|s| (i, s.pos))
        }) else {
            return;
        };
        let (dmg, heat, every) = TURRET;
        self.me.tcool = every;
        // Heat within 0..=HEAT_MAX, a shot's heat below 100.
        self.me.heat = self.me.heat.wrapping_add(heat).min(HEAT_MAX);
        self.hurt(i, dmg, true);
        self.fx.sparks(to, 4, 6 * M, 12);
        // The turret sits on the spine, ahead of the cockpit: its bolts
        // start in sight.
        let r = &self.me.rot;
        let gun = add(
            self.me.pos,
            add(scale14(r.col(1), 4 * M), scale14(r.col(2), 24 * M)),
        );
        self.beam(gun, to, false, 0);
    }

    /// Whether a shot now would hit a raider (or, when wanted, a
    /// warden): touch steering fires by itself then.
    #[inline(never)]
    pub fn foe_in_sights(&self) -> bool {
        self.first_hit()
            .and_then(|(i, _)| self.ships[i])
            .is_some_and(|s| s.kind == Kind::Raider || s.angry)
    }

    /// Whether a point is within the laser's reach ([`LASER_RANGE`]) of
    /// the player: the reach test of [`World::first_hit`] itself. The
    /// view's in-range brackets use it too, so they show exactly the
    /// ships the laser can reach.
    #[inline(never)]
    pub fn in_reach(&self, p: V3) -> bool {
        within(p, self.me.pos, LASER_RANGE)
    }

    /// The ship a shot from the nose would hit now, and how far ahead it
    /// is along the nose: the nearest ship within laser reach
    /// ([`World::in_reach`]) whose middle the nose line passes close
    /// enough to (its size, plus a little help: 1/40 of the distance;
    /// step F2 made a raider's size 72 m, smaller than it is drawn).
    /// Any kind of ship, canisters too, but not a raider's TAG: the
    /// laser passes through it (so it never shields a raider behind
    /// it). The laser fires at this one; the view's crosshair locks on
    /// it.
    #[inline(never)]
    pub fn first_hit(&self) -> Option<(usize, i32)> {
        let (pos, rot) = (self.me.pos, &self.me.rot);
        let mut best: Option<(usize, i32)> = None;
        for (i, s) in self.ships.iter().enumerate() {
            let Some(s) = s else { continue };
            if !self.in_reach(s.pos) || s.load.0 == TAG {
                continue;
            }
            let l = rot.apply_t(sub(s.pos, pos));
            if l[2] <= 0 {
                continue;
            }
            // Within laser reach (2^20): every product below is far
            // inside i64.
            // A rock is drawn bigger than a canister (see ROCK).
            let size = if s.load.0 == ROCK {
                48 * M * SMALL_X
            } else {
                s.kind.size()
            };
            // Step M2: the help halved (1/40 of the distance), as the
            // ships grew; see `World::first_hit`'s doc.
            let reach = i64::from(size.wrapping_add(l[2] / 40));
            let side = wide(l[0], l[0]).wrapping_add(wide(l[1], l[1]));
            if side <= reach.wrapping_mul(reach) && best.is_none_or(|(_, d)| l[2] < d) {
                best = Some((i, l[2]));
            }
        }
        best
    }

    /// Ship `i` takes `dmg`; `by_me` when the player fired.
    #[inline(never)]
    fn hurt(&mut self, i: usize, dmg: i32, by_me: bool) {
        let Some(mut s) = self.ships[i] else { return };
        // A raider's TAG takes no damage and never turns into a foe (so
        // the sights never call it one). The laser passes through it
        // (first_hit) and the turret never aims at a canister: this is
        // only a guard.
        if s.load.0 == TAG {
            return;
        }
        // Hull points below 600 (HAUL_HP), damage below 30.
        let whole = s.hp >= HAUL_HP / 2;
        s.hp = s.hp.wrapping_sub(dmg);
        if by_me {
            // Step F1: the player's shot takes a hauler below half its
            // hull: it sheds a canister to get away (once: hulls only
            // go down).
            if s.kind == Kind::Hauler && whole && s.hp < HAUL_HP / 2 && s.hp > 0 {
                self.shed(&s, 0, 0, 3, None);
            }
            s.angry = true;
            if matches!(s.kind, Kind::Hauler | Kind::Warden) && !self.wanted {
                self.wanted = true;
                self.event(Event::Wanted);
            }
            if s.kind == Kind::Hauler {
                s.mode = Mode::Flee;
            }
        }
        if s.hp > 0 {
            s.flash = 5;
            self.ships[i] = Some(s);
            return;
        }
        self.ships[i] = None;
        let rock = s.kind == Kind::Canister && s.load.0 == ROCK;
        let (size, m) = match s.kind {
            Kind::Canister if rock => (22, mat::STATION2),
            Kind::Canister => (8, mat::CANISTER),
            Kind::Hauler => (40, mat::HAULER),
            Kind::Raider => (28, mat::RAIDER),
            Kind::Warden => (28, mat::WARDEN),
        };
        // Speeds below 2^11.
        self.blast(
            s.pos,
            size,
            s.kind.grown(),
            m,
            scale14(s.rot.col(2), s.speed),
        );
        if rock {
            // A canister of its good takes the rock's place (look bits
            // 0-1: 1-3 tonnes). No dice.
            self.put_ship(i, Kind::Canister, s.pos, Mode::Travel);
            if let Some(c) = self.ships[i].as_mut() {
                c.load = (s.load.1, (s.look & 3) % 3 + 1);
                c.speed = 20;
            }
        }
        if s.kind == Kind::Canister {
            return;
        }
        if by_me {
            // A raider the player downed drops its TAG, first, into the
            // slot it leaves (so there is always room); its worth is fixed
            // by the raider's gun level and this system's danger (step M1:
            // level + danger / 2, so 1 to 4 times TAG_VALUE). No dice: the
            // rolls below stay.
            if s.kind == Kind::Raider
                && let Some(k) = self.add_ship(Kind::Canister, s.pos, Mode::Travel)
                && let Some(c) = self.ships[k].as_mut()
            {
                c.load = (TAG, tag_worth(s.gun, self.sys.danger));
                c.timer = TAG_LIFE;
            }
            self.car.kills = self.car.kills.saturating_add(1);
            self.event(Event::Killed { kind: s.kind });
        }
        // Cargo floats free.
        if s.kind == Kind::Hauler || self.dice.below(2) == 0 {
            let good = if s.kind == Kind::Hauler {
                self.dice.below(GOODS as u32) as u8
            } else {
                [5, 6, 7][self.dice.below(3) as usize]
            };
            let n = 1 + self.dice.below(3) as u8;
            if s.kind == Kind::Hauler {
                // Step F1: its whole load, by its size step (look bits
                // 2-3): 10, 12, 14 or 16 tonnes in 3, 3, 4 or 4
                // canisters, split evenly (the sum of (t + k) / c is t).
                // The first takes its slot, so at least one comes. The
                // rolls above stay (the dice run as before); the good
                // rolled picks which wanted good comes first.
                let step = u32::from((s.look >> 2) & 3);
                let (t, c) = (
                    step.wrapping_mul(2).wrapping_add(10),
                    (step >> 1).wrapping_add(3),
                );
                for k in 0..c {
                    let at = if k == 0 { Some(i) } else { None };
                    self.shed(&s, k, u32::from(good), (t.wrapping_add(k) / c) as u8, at);
                }
            } else if let Some(k) = self.add_ship(Kind::Canister, s.pos, Mode::Travel)
                && let Some(c) = self.ships[k].as_mut()
            {
                c.load = (good, n);
                c.rot = s.rot;
            }
        }
    }

    /// Step F1: canister `k` of hauler `s`'s load: `n` tonnes of one of
    /// the two goods this economy wants most ([`HAUL`], `roll` picks
    /// which first), 200 m apart across the hauler from 300 m to its
    /// left (off the line of a shot at its middle from behind or ahead)
    /// and 120 m before or behind its middle in turn, drifting slowly
    /// along its nose; into `slot`, else into a free fight slot (none
    /// free: none).
    #[inline(never)]
    fn shed(&mut self, s: &Ship, k: u32, roll: u32, n: u8, slot: Option<usize>) {
        let good = HAUL[self.sys.econ as usize][(roll.wrapping_add(k) & 1) as usize];
        // k below 4: within +-300 m across, +-120 m along (below 2^17; so
        // they are apart seen from the side too).
        let k = k as i32;
        let x = k.wrapping_mul(200 * M).wrapping_sub(300 * M);
        let z = (k & 1).wrapping_mul(240 * M).wrapping_sub(120 * M);
        let at = add(s.pos, s.rot.apply([x, 0, z]));
        let Some(j) = slot.or_else(|| self.ships[..FIGHT].iter().position(Option::is_none)) else {
            return;
        };
        self.put_ship(j, Kind::Canister, at, Mode::Travel);
        if let Some(c) = self.ships[j].as_mut() {
            c.load = (good, n);
            c.rot = s.rot;
        }
    }

    /// An explosion of `size` (8..=40, as before M2) times `grown` (the
    /// kind's growth, 1..=6: drawn at most 240): as many pieces and
    /// sparks as before, flying `grown` times as fast.
    #[inline(never)]
    fn blast(&mut self, pos: V3, size: u8, grown: i32, m: u8, drift: V3) {
        let slot = (0..4).max_by_key(|&k| self.blasts[k].age).unwrap_or(0);
        // At most 40 times 6.
        let big = (i32::from(size).wrapping_mul(grown)).min(255) as u8;
        self.blasts[slot] = Blast {
            pos,
            age: 0,
            size: big,
        };
        // Size 8..=40: 1 to 6 pieces.
        self.fx
            .debris(pos, drift, m, u32::from(size / 7).max(1), grown);
        self.fx.sparks(pos, u32::from(size / 4), 5 * M * grown, 18);
    }

    #[inline(never)]
    fn beam(&mut self, from: V3, to: V3, foe: bool, lv: u8) {
        let slot = (0..4).min_by_key(|&k| self.beams[k].age).unwrap_or(0);
        self.beams[slot] = Beam {
            from,
            to,
            foe,
            age: 4,
            lv,
        };
    }

    /// Step G2: the ship lost below (the hull gone on the ground): the
    /// same end as a loss in space.
    #[inline(never)]
    pub fn lose(&mut self) {
        self.die();
    }

    #[inline(never)]
    fn die(&mut self) {
        self.state = State::Dead;
        self.me.speed = 0;
        self.me.cruise = false;
        self.me.auto = false;
        self.blast(self.me.pos, 40, 1, mat::HULL, [0; 3]);
        self.fx.jolt = 24;
        self.event(Event::Died);
    }

    /// Adds a ship near `pos`; returns its slot (tests use it too).
    #[inline(never)]
    pub fn add_ship(&mut self, kind: Kind, pos: V3, mode: Mode) -> Option<usize> {
        let i = self.ships[..FIGHT].iter().position(Option::is_none)?;
        self.put_ship(i, kind, pos, mode);
        Some(i)
    }

    /// A new ship in slot `i`.
    #[inline(never)]
    fn put_ship(&mut self, i: usize, kind: Kind, pos: V3, mode: Mode) {
        let to_me = sub(self.me.pos, pos);
        let rot = if kind == Kind::Hauler {
            Mat3::looking(sub(self.bodies.station_pos, pos), [0, ONE14, 0])
        } else {
            Mat3::looking(to_me, [0, ONE14, 0])
        };
        let look = hash(self.sys.h ^ 0x10C5, self.ticks ^ ((i as u32) << 24)) as u8;
        self.ships[i] = Some(Ship {
            kind,
            pos: clamp_pos(pos),
            rot,
            speed: kind.drive().0,
            hp: kind.hull(self.sys.danger),
            mode,
            timer: if kind == Kind::Canister {
                CARGO_LIFE
            } else {
                0
            },
            target: if kind == Kind::Raider { PLAYER } else { NOBODY },
            cool: 60,
            load: (0, 0),
            angry: false,
            flash: 0,
            lead: NOBODY,
            wing: 0,
            look,
            // From the look's free bits 6-7 (no dice, so the fights roll
            // as before); only raiders use it.
            gun: GUN_ODDS[usize::from(self.sys.danger.min(3))][usize::from(look >> 6)],
            shots: 0,
        });
    }

    /// Now and then: a raider pack, a hauler, a warden, by the system's
    /// danger. Far ships leave.
    #[inline(never)]
    fn spawner(&mut self) {
        for s in self.ships.iter_mut() {
            if s.is_some_and(|s| !within(s.pos, self.me.pos, 22 * KM)) {
                *s = None;
            }
        }
        self.traffic();
        if self.spawn > 0 {
            self.spawn -= 1;
            return;
        }
        let danger = u32::from(self.sys.danger);
        self.spawn = 600u16.wrapping_add(self.dice.below(600) as u16);
        // Not within sight of the station in SECURE and QUIET systems:
        // there the station's patrol keeps its approaches clear.
        let near_station = self.station_dist() < 8 * KM;
        let roll = self.dice.below(100);
        let around = |w: &mut World, km: i32| {
            let d = [w.dice.spread(1000), w.dice.spread(400), w.dice.spread(1000)];
            let d = norm(d, ONE14);
            let d = if d == [0; 3] { [0, 0, ONE14] } else { d };
            add(w.me.pos, scale14(d, km * KM))
        };
        // Step M1: no new pack while raiders still press an attack on the
        // player within 9 km (a fleeing one does not count), so packs
        // come one after another, never piled up; the roll then falls
        // through to a hauler or a warden.
        let busy = self.ships.iter().flatten().any(|s| {
            s.kind == Kind::Raider
                && s.target == PLAYER
                && s.mode != Mode::Flee
                && within(s.pos, self.me.pos, 9 * KM)
        });
        if roll < [8, 20, 34, 52][danger as usize] && !(near_station && danger < 2) && !busy {
            let (least, more) = PACKS[danger as usize];
            // Up to six fit this (PACKS gives five): three in a V, the rest in a V
            // PACK_GAP farther out on the same line.
            let n = least.wrapping_add(self.dice.below(more.wrapping_add(1)));
            let at = around(self, 7);
            let back = add(at, scale14(norm(sub(at, self.me.pos), ONE14), PACK_GAP));
            let mut first = NOBODY;
            for k in 0..n {
                let place = k % 3;
                if place == 0 {
                    first = NOBODY;
                }
                let base = if k < 3 { at } else { back };
                // place is below 3 (step M2: 300 m times FIGHTER_X apart).
                let p = add(
                    base,
                    [(place as i32).wrapping_mul(300 * M * FIGHTER_X), 0, 0],
                );
                if let Some(j) = self.add_ship(Kind::Raider, p, Mode::Hunt) {
                    if first == NOBODY {
                        first = j as u8;
                    } else if WINGS_ON && let Some(s) = self.ships[j].as_mut() {
                        s.lead = first;
                        s.wing = place as u8;
                    }
                }
            }
            self.event(Event::Raiders);
        } else if roll < 70 {
            let at = around(self, 6);
            self.add_ship(Kind::Hauler, at, Mode::Travel);
        } else if roll < 70 + [25, 18, 10, 3][danger as usize] {
            let at = around(self, 6);
            self.add_ship(Kind::Warden, at, Mode::Travel);
        }
    }

    /// The extra traffic (step L2), in slots `FIGHT..SHIPS` only, and
    /// without the world's dice (a hash of the tick), so the fights roll
    /// exactly as before. Every 64 ticks, one time in four: near the
    /// station a ship leaves through the mouth, one comes in along the
    /// approach (a pair now and then), or (SECURE and QUIET) a warden
    /// patrols; far from it a convoy of haulers crosses, with a warden
    /// escort in SECURE and QUIET systems and two raiders on its tail in
    /// RISKY and LAWLESS ones.
    #[inline(never)]
    fn traffic(&mut self) {
        if !TRAFFIC_ON || self.ticks & 63 != 0 {
            return;
        }
        let h = hash(self.sys.h ^ 0x7AF1, self.ticks);
        // Traffic left 12 km behind leaves (fights keep 22 km), so the
        // slots free up for what lies ahead (step L4).
        for s in self.ships[FIGHT..].iter_mut() {
            if s.is_some_and(|s| !within(s.pos, self.me.pos, 12 * KM)) {
                *s = None;
            }
        }
        let free = (FIGHT..SHIPS).filter(|&i| self.ships[i].is_none()).count();
        if free == 0 || h & 3 != 0 {
            return;
        }
        let b = self.bodies;
        let axis = b.station_rot.col(2);
        let lawful = self.sys.danger < 2;
        // Fills the next free traffic slot; its index (or NOBODY).
        let put = |w: &mut World, kind: Kind, pos: V3, lead: u8, wing: u8| -> u8 {
            let Some(i) = (FIGHT..SHIPS).find(|&i| w.ships[i].is_none()) else {
                return NOBODY;
            };
            w.put_ship(i, kind, pos, Mode::Travel);
            if let Some(s) = w.ships[i].as_mut() {
                s.lead = lead;
                s.wing = wing;
            }
            i as u8
        };
        // A spread of +-1 km from the hash (h >> s, 11 bits).
        let jit = |s: u32| {
            (((h >> s) & 2047) as i32)
                .wrapping_sub(1024)
                .wrapping_mul(M)
        };
        if self.station_dist() < 9 * KM {
            match (h >> 2) & 3 {
                0 | 1 => {
                    // Out through the mouth, unless the player is there
                    // (step M2: the mouth is 380 m out and a hauler up to
                    // 870 m long, so it starts with its tail in the mouth).
                    let at = add(b.station_pos, scale14(axis, 200 * M * STATION_X));
                    if within(self.me.pos, at, 1500 * M) {
                        return;
                    }
                    let i = put(self, Kind::Hauler, at, NOBODY, 0);
                    if let Some(s) = self.ships.get_mut(usize::from(i)).and_then(|s| s.as_mut()) {
                        s.rot = Mat3::looking(axis, b.station_rot.col(1));
                        s.speed = 120;
                        s.timer = DEPART;
                    }
                }
                2 => {
                    // In along the approach, 6 km out; now and then a pair.
                    let side = [jit(4), jit(15) / 2, 0];
                    let at = add(add(b.station_pos, scale14(axis, 6 * KM)), side);
                    let i = put(self, Kind::Hauler, at, NOBODY, 0);
                    if i != NOBODY && h & 0x40_0000 != 0 {
                        let back = add(at, scale14(axis, 240 * M * FREIGHTER_X));
                        put(self, Kind::Hauler, back, i, 1);
                    }
                }
                _ => {
                    // One patrol at a time, in lawful systems only.
                    let patrol = self.ships[FIGHT..]
                        .iter()
                        .flatten()
                        .any(|s| s.kind == Kind::Warden);
                    if lawful && !patrol {
                        let at = add(b.station_pos, [jit(4).wrapping_mul(3), jit(15), jit(21)]);
                        put(self, Kind::Warden, at, NOBODY, 0);
                    }
                }
            }
            return;
        }
        // 0: a convoy; 1, 2: rocks (three rocks). Both need three free
        // slots (step M2: before, rocks came into two; a convoy spread
        // wider leaves one ship at a time, and pairs of rocks then kept
        // the slots for good: 1 convoy in ten minutes instead of 3).
        let rocks = matches!((h >> 2) & 3, 1 | 2);
        if (h >> 2) & 3 == 3 || free < 3 || (rocks && !ROCKS_ON) {
            return;
        }
        // A convoy 6 km away (or rocks at 3 km), within about 20 degrees
        // of the player's nose (so it is seen), its haulers bound for the
        // station. The nose is a unit axis (+-2^14), the spreads below 2^13.
        let n = self.me.rot.col(2);
        let d = [
            n[0].wrapping_add(jit(4) / 48),
            n[1].wrapping_add(jit(15) / 96),
            n[2].wrapping_add(jit(21) / 48),
        ];
        let d = norm(d, ONE14);
        let d = if d == [0; 3] { [0, 0, ONE14] } else { d };
        if rocks {
            self.rocks(add(self.me.pos, scale14(d, 3 * KM)), h);
            return;
        }
        let at = add(self.me.pos, scale14(d, 6 * KM));
        let lead = put(self, Kind::Hauler, at, NOBODY, 0);
        if lead == NOBODY {
            return;
        }
        // Step M2: the gaps times the ships' growth.
        const HG: i32 = 300 * M * FREIGHTER_X;
        put(self, Kind::Hauler, add(at, [0, 0, HG]), lead, 1);
        if lawful {
            put(self, Kind::Hauler, add(at, [HG, 0, 0]), lead, 2);
            put(
                self,
                Kind::Warden,
                add(at, [0, 200 * M * FIGHTER_X, 0]),
                lead,
                3,
            );
        } else {
            // Raiders a little farther out, closing on the haulers.
            let tail = add(at, scale14(d, 3 * KM));
            let r = put(self, Kind::Raider, tail, NOBODY, 0);
            put(
                self,
                Kind::Raider,
                add(tail, [300 * M * FIGHTER_X, 0, 0]),
                r,
                1,
            );
        }
    }

    /// Step L4: up to three rocks drifting round `at`, in the free
    /// traffic slots.
    #[inline(never)]
    fn rocks(&mut self, at: V3, h: u32) {
        for k in 0..3 {
            let Some(i) = (FIGHT..SHIPS).find(|&i| self.ships[i].is_none()) else {
                return;
            };
            let g = hash(h, k);
            // Each part 10 bits or fewer: +-512 m at most, times
            // SMALL_X (step M2: the rocks grew).
            let part = |s: u32, n: u32| ((g >> s) & (n - 1)) as i32 - (n / 2) as i32;
            const R: i32 = M * SMALL_X;
            let off = [part(0, 1024) * R, part(10, 512) * R, part(19, 1024) * R];
            self.put_ship(i, Kind::Canister, add(at, off), Mode::Travel);
            if let Some(s) = self.ships[i].as_mut() {
                // ICE (1) or ORE (2).
                s.load = (ROCK, if g >> 31 != 0 { 1 } else { 2 });
                s.hp = ROCK_HP;
                s.speed = 6;
                s.timer = ROCK_LIFE;
                s.rot = Mat3::rotation(g as u16, (g >> 16) as u16, 0);
            }
        }
    }

    /// Canisters close by go into the hold.
    #[inline(never)]
    fn pick_up(&mut self) {
        for i in 0..SHIPS {
            let Some(s) = self.ships[i] else { continue };
            // Step M2: 70 m, the canisters grew to 30 m.
            if s.kind != Kind::Canister || s.load.0 == ROCK || !within(s.pos, self.me.pos, 70 * M) {
                continue;
            }
            // A TAG takes no hold space: scooped even with a full hold,
            // but not past TAGS_MAX (then it stays in space).
            if s.load.0 == TAG {
                let c = &mut self.car;
                if c.tags < TAGS_MAX {
                    c.tags = c.tags.wrapping_add(1);
                    // At most 255 TAGs of at most 100: below 2^16.
                    c.owed = c
                        .owed
                        .wrapping_add(u16::from(s.load.1).wrapping_mul(TAG_VALUE));
                    self.ships[i] = None;
                    self.event(Event::Scooped { good: TAG, n: 1 });
                }
                continue;
            }
            let room = self.car.room();
            if room == 0 {
                if self.full_told != i as u8 {
                    self.full_told = i as u8;
                    self.event(Event::HoldFull);
                }
                continue;
            }
            let n = u32::from(s.load.1).min(room) as u8;
            let g = usize::from(s.load.0) % GOODS;
            // n is at most the room: the hold stays at most HOLD_MOST.
            self.car.hold.0[g] = self.car.hold.0[g].wrapping_add(n);
            self.ships[i] = None;
            self.full_told = NOBODY;
            self.event(Event::Scooped { good: g as u8, n });
        }
    }

    /// Every other ship thinks, turns, moves and fires.
    #[inline(never)]
    fn npcs(&mut self) {
        let playing = self.state == State::Flying;
        for i in 0..SHIPS {
            let Some(mut s) = self.ships[i] else { continue };
            s.flash = s.flash.saturating_sub(1);
            if s.kind == Kind::Canister {
                // A TAG blinks (half a second on, half off) so it is found.
                if s.load.0 == TAG && self.ticks & 32 != 0 {
                    s.flash = 1;
                }
                s.pos = clamp_pos(add(s.pos, scale14(s.rot.col(2), s.speed)));
                s.timer = s.timer.saturating_sub(1);
                self.ships[i] = if s.timer == 0 { None } else { Some(s) };
                continue;
            }
            let (top, turn) = s.kind.drive();
            s.cool = s.cool.saturating_sub(1);
            s.timer = s.timer.saturating_sub(1);
            // Who to go for.
            s.target = match s.kind {
                // A convoy's raiders (traffic slots) go for its haulers
                // first, for the player once none is near or when shot.
                Kind::Raider if i >= FIGHT && !s.angry => {
                    match self.nearest(s.pos, Kind::Hauler, 9 * KM) {
                        NOBODY if playing => PLAYER,
                        t => t,
                    }
                }
                Kind::Raider if playing => PLAYER,
                Kind::Warden if self.wanted && playing && within(s.pos, self.me.pos, 9 * KM) => {
                    PLAYER
                }
                Kind::Warden => self.nearest(s.pos, Kind::Raider, 9 * KM),
                Kind::Hauler if s.angry && playing => PLAYER,
                _ => NOBODY,
            };
            let goal = match s.target {
                PLAYER => Some(self.me.pos),
                NOBODY => None,
                t => self.ships[usize::from(t)].map(|o| o.pos),
            };
            let home = self.bodies.station_pos;
            let mut speed = top;
            let form = self.formation(&mut s, goal);
            let aim = if let Some((at, v)) = form {
                speed = v;
                at
            } else {
                match (s.kind, s.mode, goal) {
                    (Kind::Hauler, _, _) if s.mode == Mode::Flee || s.angry => {
                        goal.map(|g| away(s.pos, g)).unwrap_or(home)
                    }
                    (Kind::Hauler, _, _) if s.timer > 0 => {
                        // Leaving the station: straight out, then gone.
                        if s.timer == 1 {
                            self.ships[i] = None;
                            continue;
                        }
                        speed = top.wrapping_mul(3) / 4;
                        add(s.pos, scale14(s.rot.col(2), 4 * KM))
                    }
                    (Kind::Hauler, _, _) => {
                        speed = top.wrapping_mul(3) / 4;
                        if within(s.pos, home, 200 * M * STATION_X) {
                            // In at the station (step M2: its nose at
                            // the mouth).
                            self.ships[i] = None;
                            continue;
                        }
                        home
                    }
                    (Kind::Raider, Mode::Flee, Some(g)) => {
                        // Step M1: running for its life, flat out and
                        // jinking from side to side about once a second;
                        // after FLEE_TICKS back to the hunt unless chased.
                        speed = top.wrapping_mul(5) / 4;
                        if s.timer == 0 {
                            if within(s.pos, g, CHASED) {
                                s.timer = 120;
                            } else {
                                s.mode = Mode::Hunt;
                                s.shots = 0;
                            }
                        }
                        let jink = if self.ticks & 64 == 0 { KM } else { -KM };
                        add(away(s.pos, g), scale14(s.rot.col(0), jink))
                    }
                    (_, Mode::Flee, Some(g)) => away(s.pos, g),
                    (_, Mode::Break, Some(g)) => {
                        if s.timer == 0 {
                            s.mode = Mode::Hunt;
                        }
                        // Away, and off to one side.
                        let side = scale14(s.rot.col(0), 2 * KM);
                        add(away(s.pos, g), side)
                    }
                    (_, _, Some(g)) => {
                        s.mode = Mode::Hunt;
                        // Step M4: a level-3 raider whose prey got out
                        // of its gun's reach opens an overboost window;
                        // the prey back in reach ends the overboost (the
                        // window's wait goes on).
                        if RAIDER_OD_ON
                            && s.kind == Kind::Raider
                            && s.gun == 2
                            && s.target == PLAYER
                        {
                            let far = !within(s.pos, g, RAIDER_REACH);
                            if s.timer == 0 && far {
                                s.timer = OD_EVERY;
                            } else if !far {
                                s.timer = s.timer.min(OD_EVERY - OD_RUN);
                            }
                        }
                        // Step M2: 200 m times FIGHTER_X.
                        if within(s.pos, g, 200 * M * FIGHTER_X) {
                            s.mode = Mode::Break;
                            s.timer = 70u16.wrapping_add(self.dice.below(40) as u16);
                        }
                        g
                    }
                    (_, _, None) => {
                        // Patrol round the station.
                        speed = top / 2;
                        if within(s.pos, home, 5 * KM) {
                            add(
                                s.pos,
                                add(scale14(s.rot.col(2), 2 * KM), scale14(s.rot.col(0), KM)),
                            )
                        } else {
                            home
                        }
                    }
                }
            };
            // Below 30 % of its hull, but only once it has traded fire
            // (step M1: before, it turned away before its first shot).
            // Hull points below 600 (HAUL_HP).
            if s.kind == Kind::Raider
                && s.mode != Mode::Flee
                && s.shots >= FLEE_AFTER
                && s.hp.wrapping_mul(10) < s.kind.hull(self.sys.danger).wrapping_mul(3)
            {
                s.mode = Mode::Flee;
                s.timer = FLEE_TICKS;
            }
            if s.mode == Mode::Flee && goal.is_some_and(|g| !within(s.pos, g, 14 * KM)) {
                self.ships[i] = None;
                continue;
            }
            let (p, y) = aim_at(&s.rot, s.pos, aim, turn);
            s.rot = s.rot.turned(p, y, 0);
            // Step M4: overboost, 7/4 of its top speed, and no shot.
            let od = raider_boost(&s);
            if od {
                speed = top.wrapping_mul(7) / 4;
            }
            // Speeds within 0..=1,750.
            s.speed = s
                .speed
                .wrapping_add(speed.wrapping_sub(s.speed).clamp(-20, 20));
            s.pos = clamp_pos(add(s.pos, scale14(s.rot.col(2), s.speed)));
            // Fire when the target is in front and near: a raider as far
            // as the player's laser (step M1), a warden within 1.1 km.
            let reach = if s.kind == Kind::Raider {
                RAIDER_REACH
            } else {
                1100 * M
            };
            let fires = matches!(s.kind, Kind::Raider | Kind::Warden)
                && s.mode == Mode::Hunt
                && !od
                && s.cool == 0
                && goal.is_some_and(|g| within(s.pos, g, reach));
            self.ships[i] = Some(s);
            if fires && let Some(g) = goal {
                self.npc_fire(i, g);
            }
        }
    }

    /// Ship `i` fires at `g` (the player or another ship).
    #[inline(never)]
    fn npc_fire(&mut self, i: usize, g: V3) {
        let Some(mut s) = self.ships[i] else { return };
        // Within 2.4 km (2^20): every product is far inside i64.
        let l = s.rot.apply_t(sub(g, s.pos));
        let side = wide(l[0], l[0]).wrapping_add(wide(l[1], l[1]));
        let cone = i64::from(l[2]) / 8;
        if l[2] <= 0 || side > cone.wrapping_mul(cone) {
            return;
        }
        // Wardens are the law: sure and hard everywhere, faster with the
        // system's danger. Raiders fire by their gun's level (step M1).
        // Danger 0..=3, levels 0..=2: small sums.
        let law = s.kind == Kind::Warden;
        let danger = u32::from(self.sys.danger);
        let (dmg, every, mut sure) = if law {
            (16, 36u32.wrapping_sub(danger.wrapping_mul(4)), 60)
        } else {
            RAIDER_GUNS[usize::from(s.gun.min(2))]
        };
        s.cool = every.wrapping_add(self.dice.below(16)) as u8;
        s.shots = s.shots.saturating_add(1);
        self.ships[i] = Some(s);
        if !law && s.target == PLAYER {
            // Past SURE_NEAR down to 2/3 at the reach (2.4 km at most:
            // the metres past it below 1,300).
            let far = (l[2] / M).wrapping_sub(SURE_NEAR).clamp(0, 1200) as u32;
            sure = sure.wrapping_sub(sure.wrapping_mul(far) / 3600);
            // The player's turning lately spoils the aim (below 840).
            // Step M4: the overboost spoils it as hard turning does.
            let jink = if self.me.boost { OD_JINK } else { 0 };
            let turn = self.me.jink.max(jink) as u32;
            sure = sure.wrapping_mul(DODGE) / DODGE.wrapping_add(turn);
        }
        let hits = self.dice.below(100) < sure;
        // Step M3: the bolt's colour by the gun (a warden's: the top).
        let lv = if law { 2 } else { s.gun };
        if s.target == PLAYER {
            // A hit ends on the hull below the cockpit, not in the eye:
            // a line through the eye would be drawn as one dot.
            let miss = if hits {
                scale14(self.me.rot.col(1), -5 * M)
            } else {
                // Step F1: past the grown courier's wing tip.
                scale14(s.rot.col(0), 24 * M * ME_X)
            };
            self.beam(s.pos, add(g, miss), true, lv);
            if hits {
                self.fx.jolt = 10;
                self.fx.sparks(add(g, miss), 4, 5 * M, 10);
                self.damage(dmg, false);
                self.event(Event::Hit);
            }
        } else if let Some(t) = (s.target != NOBODY).then_some(usize::from(s.target)) {
            self.beam(s.pos, g, false, lv);
            if hits {
                self.fx.sparks(g, 3, 5 * M, 10);
                self.hurt(t, 15, false);
            }
        }
    }

    /// Where ship `s` flies to keep its place by its leader (a point
    /// ahead of that place) and how fast; None when it flies on its own:
    /// no leader or the leader gone, it was shot at or flees, or it has
    /// a goal (a raider breaks off 2 km from its prey, a warden at once).
    /// Once broken, for good.
    #[inline(never)]
    fn formation(&self, s: &mut Ship, goal: Option<V3>) -> Option<(V3, i32)> {
        if s.lead == NOBODY {
            return None;
        }
        let keep = !s.angry
            && s.mode != Mode::Flee
            && goal.is_none_or(|g| s.kind == Kind::Raider && !within(s.pos, g, 2 * KM));
        let lead = self
            .ships
            .get(usize::from(s.lead))
            .copied()
            .flatten()
            .filter(|l| l.kind != Kind::Canister && l.lead == NOBODY);
        let (true, Some(l)) = (keep, lead) else {
            s.lead = NOBODY;
            return None;
        };
        // Places within 300 m times the kind's growth (step M2, at most
        // 6): scale14 of a unit axis stays small.
        let g = M.wrapping_mul(s.kind.grown());
        let w = WINGS[usize::from(s.wing) % WINGS.len()].map(|c| c.wrapping_mul(g));
        let r = &l.rot;
        let side = add(scale14(r.col(0), w[0]), scale14(r.col(1), w[1]));
        let slot = clamp_pos(add(l.pos, add(side, scale14(r.col(2), w[2]))));
        // Both inside +-LIMIT: the difference fits.
        let gap = sub(slot, s.pos);
        let speed = if dot(gap, r.col(2)) < 0 {
            // Past its place: let the leader come up.
            l.speed / 2
        } else if within(s.pos, slot, g.wrapping_mul(150)) {
            l.speed
        } else {
            s.kind.drive().0
        };
        Some((add(slot, scale14(r.col(2), g.wrapping_mul(600))), speed))
    }

    /// The nearest ship of `kind` within `r` of `pos`, or NOBODY.
    #[inline(never)]
    fn nearest(&self, pos: V3, kind: Kind, r: i32) -> u8 {
        let mut best = (NOBODY, i64::MAX);
        for (i, s) in self.ships.iter().enumerate() {
            if let Some(s) = s
                && s.kind == kind
            {
                let d = sub(s.pos, pos);
                let d2 = dot(d, d);
                if d2 < best.1 && d2 <= wide(r, r) {
                    best = (i as u8, d2);
                }
            }
        }
        best.0
    }
}

/// Step M4: a level-3 raider's overboost is running: the first
/// [`OD_RUN`] ticks of its flight, or of an [`OD_EVERY`] window it opens
/// while it hunts the player out of its gun's reach.
pub fn raider_boost(s: &Ship) -> bool {
    let start = match s.mode {
        Mode::Flee => FLEE_TICKS,
        Mode::Hunt => OD_EVERY,
        _ => return false,
    };
    RAIDER_OD_ON && s.kind == Kind::Raider && s.gun == 2 && s.timer > start.wrapping_sub(OD_RUN)
}

/// A raider's TAG worth in [`TAG_VALUE`]s (1..=4): its gun level (1 to
/// 3) plus half the danger, at most 4.
pub fn tag_worth(gun: u8, danger: u8) -> u8 {
    // Both small: no sum wraps.
    gun.min(2)
        .wrapping_add(1)
        .wrapping_add(danger.min(3) / 2)
        .min(4)
}

/// `a + b`. Callers add offsets to positions inside `LIMIT` (2^29),
/// so the sum fits; checked in debug builds only.
#[inline(never)]
pub fn add(a: V3, b: V3) -> V3 {
    debug_assert!((0..3).all(|k| a[k].checked_add(b[k]).is_some()));
    [
        a[0].wrapping_add(b[0]),
        a[1].wrapping_add(b[1]),
        a[2].wrapping_add(b[2]),
    ]
}

/// A position pulled inside +-LIMIT.
#[inline(never)]
pub fn clamp_pos(p: V3) -> V3 {
    p.map(|c| c.clamp(-LIMIT, LIMIT))
}

/// Whether `a` and `b` are at most `r` apart.
#[inline(never)]
pub fn within(a: V3, b: V3, r: i32) -> bool {
    let d = sub(a, b);
    // A quick look first: one axis already too far.
    if d.iter().any(|c| c.unsigned_abs() > r as u32) {
        return false;
    }
    dot(d, d) <= wide(r, r)
}

/// A point straight away from `g`, seen from `p`.
#[inline(never)]
fn away(p: V3, g: V3) -> V3 {
    clamp_pos(add(p, scale14(norm(sub(p, g), ONE14), 4 * KM)))
}

/// The pitch and yaw (angle steps, at most `max`) that turn a ship at
/// `pos` turned by `rot` towards `to`.
#[inline(never)]
pub fn aim_at(rot: &Mat3, pos: V3, to: V3, max: i32) -> (i32, i32) {
    // max is a turn rate (below 2^9): its negative fits.
    let neg = max.wrapping_neg();
    let l = rot.apply_t(norm(sub(to, pos), ONE14));
    if l[2] < 0 {
        // Behind: turn as hard as allowed, the shorter way.
        let y = if l[0] >= 0 { max } else { neg };
        let p = if l[1].unsigned_abs() > l[0].unsigned_abs() {
            if l[1] >= 0 { max } else { neg }
        } else {
            0
        };
        return if p != 0 { (p, 0) } else { (0, y) };
    }
    let lim = |v: i32| v.max(neg).min(max);
    (lim(l[1] / 12), lim(l[0] / 12))
}
