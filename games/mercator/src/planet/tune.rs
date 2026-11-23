//! Step G1: the planet's numbers (SPEC 3.12, all CHOICE), under the
//! spec's names. Only the ones the frame and the land use are here; the
//! lander, the pilot, the cutter and the drones bring theirs with them.
//!
//! Lengths are in world units (`M` = 1/256 m), times in ticks (60 a
//! second), angles as the cosine in 1/16384.

use crate::world::{KM, M};

// ---- The ring in space (SPEC 3.2) ---------------------------------------------

/// The ring's height over the surface, above the site.
pub const RING_ALT: i32 = 1500 * M;
/// The ring's radius: a crossing within it enters.
pub const RING_R: i32 = 300 * M;
/// The nose must point within 45 degrees of straight down: cos 45 in
/// 1/16384.
pub const RING_NOSE: i32 = 11_585;
/// No entry while a raider hunts the ship within this.
pub const RING_HUNT: i32 = 3 * KM;
/// Fuel (tenths of a light year) the ship must have to enter, and what
/// lifting off costs.
pub const LIFT_FUEL: u8 = 3;
/// The ring may not lie this close to the station (else the next
/// direction is taken).
pub const RING_CLEAR: i32 = 2 * KM;
/// The seed directions tried for the site, in order (`world::seed_dir`;
/// 1 and 2 are the sun and the station).
pub const SITE_DIRS: [u32; 3] = [11, 12, 13];
/// The ring's lights: how many, and the ticks each bright one stays.
pub const RING_LIGHTS: u32 = 8;
pub const RING_STEP: u32 = 8;
/// The ring's lights are drawn up to this far.
pub const RING_SEEN: i32 = 6 * KM;

// ---- The site (SPEC 3.4) -----------------------------------------------------

/// Salts of the site's hashes.
pub const SITE_SALT: u32 = 0x51_7E;
pub const OCT_SALT: [u32; 3] = [0x0C7A_1281, 0x0C7A_3227, 0x0C7A_0815];
pub const PLACE_SALT: u32 = 0x9_1ACE;
/// The look with no surface (the banded giant).
pub const GIANT: u8 = 4;

/// One look's shape: the three octaves' heights (128 m, 32 m and 8 m
/// cells) and where water (or lava) lies, in world units. `wet` at or
/// below `-LOW` means none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    pub amp: [i32; 3],
    pub wet: i32,
}

/// Lower than any ground: "no water".
pub const LOW: i32 = 100 * M;

/// Per look (`art::LOOK_NAMES` order). CHOICE: SPEC 3.4's table.
pub const SHAPE: [Shape; 8] = [
    // OCEAN: rolling hills over a shallow sea at SEA = -2 m.
    Shape {
        amp: [14 * M, 4 * M, M * 8 / 10],
        wet: -2 * M,
    },
    // DESERT: dunes (the 32 m octave folded).
    Shape {
        amp: [10 * M, 6 * M, M * 6 / 10],
        wet: -LOW,
    },
    // ICE: a plain with pressure ridges.
    Shape {
        amp: [8 * M, 3 * M, M * 12 / 10],
        wet: -LOW,
    },
    // LAVA: basalt steps; the lowest ground glows.
    Shape {
        amp: [12 * M, 5 * M, M / 2],
        wet: -8 * M,
    },
    // GIANT: no surface (never used).
    Shape {
        amp: [0, 0, 0],
        wet: -LOW,
    },
    // JUNGLE: wet hollows.
    Shape {
        amp: [12 * M, 5 * M, M * 8 / 10],
        wet: -5 * M,
    },
    // HAZE: as JUNGLE, a shorter haze.
    Shape {
        amp: [12 * M, 5 * M, M * 8 / 10],
        wet: -5 * M,
    },
    // RUST: mesas.
    Shape {
        amp: [10 * M, 5 * M, M * 6 / 10],
        wet: -LOW,
    },
];

/// LAVA's terraces: 3 m steps, the rise over the top 0.6 m of each.
pub const TERRACE: i32 = 3 * M;
pub const RISER: i32 = M * 6 / 10;
/// RUST's mesas: ground whose 128 m octave is over `MESA_FROM` is
/// lifted by up to `MESA_LIFT`, eased in over `MESA_EASE`.
pub const MESA_FROM: i32 = 3 * M;
pub const MESA_EASE: i32 = 4 * M;
pub const MESA_LIFT: i32 = 10 * M;
/// ICE's ridges: the 8 m octave only where the 32 m octave is over 60 %.
pub const RIDGE_FROM: i32 = 16_384 * 6 / 10;

/// The pad: flat within `PAD_R`, eased into the land up to `PAD_BLEND`.
pub const PAD_R: i32 = 12 * M;
pub const PAD_BLEND: i32 = 24 * M;
/// The pad stands at least this far above water.
pub const PAD_DRY: i32 = M / 2;
/// Tries for the site's origin (the land under the pad).
pub const ORIGIN_TRIES: u32 = 8;

/// The ledge's plinth: radius, and 3 m plus `hash % 3` m high.
pub const PLINTH_R: i32 = 3 * M;
pub const PLINTH_BASE: i32 = 3 * M;

// ---- Placement (SPEC 3.4) ----------------------------------------------------

/// Seams 1 to 3 and seam 4 (the ledge), the wreck, the boulders.
pub const SEAMS: usize = 4;
pub const BOULDERS: usize = 4;
pub const TRIES: u32 = 16;
pub const SEAM_NEAR: i32 = 40 * M;
pub const SEAM_FAR: i32 = 140 * M;
pub const LEDGE_NEAR: i32 = 50 * M;
pub const WRECK_NEAR: i32 = 60 * M;
pub const WRECK_FAR: i32 = 120 * M;
pub const APART: i32 = 25 * M;
pub const BOULDER_NEAR: i32 = 8 * M;
pub const BOULDER_FAR: i32 = 20 * M;
/// Slopes as tan in 1/16384: 8, 10, 15 degrees.
pub const TAN8: i32 = 2303;
pub const TAN10: i32 = 2889;
pub const TAN15: i32 = 4390;
/// Drones: `min(DRONES_BASE + danger, DRONES_MAX)`.
pub const DRONES_BASE: u8 = 2;
pub const DRONES_MAX: u8 = 4;
/// The landing start: this far from the pad, this high over the ground.
pub const START_OUT: i32 = 150 * M;
pub const START_UP: i32 = 120 * M;

/// The walk the reach check allows (SPEC 3.4's proof): a 2 m grid,
/// 35 degrees at most (1.40 m a 2 m step, 1.98 m a diagonal one), and
/// one jet hop of at most 5 m.
pub const WALK_GRID: i32 = 2 * M;
pub const WALK_STEP: i32 = M * 140 / 100;
pub const WALK_DIAG: i32 = M * 198 / 100;
pub const HOP: i32 = 5 * M;

// ---- Lift (SPEC 3.5; G1 keeps the numbers for G2) ----------------------------

/// Lift per ship in `world::CRAFTS` order (LARK, DRAY, SHRIKE), in
/// 1/100 of the site's gravity: the spec's [1.8, 1.6, 1.35] by weight
/// class (light SHRIKE 1.8, middle LARK 1.6, heavy DRAY 1.35).
pub const LIFT: [u16; 3] = [160, 135, 180];

// ---- Step G2: the way down and the flown landing (SPEC 3.3, 3.5, 3.7) ------

/// The entry: 180 ticks (3 s). The sky's bands come in over the first
/// `ENTRY_SKY` ticks (with the streaks, the stars' fade and the shake);
/// the fog's far edge grows from `ENTRY_FOG` to the end; A skips from
/// `ENTRY_SKIP` on.
pub const ENTRY_TICKS: u16 = 180;
pub const ENTRY_SKY: u16 = 120;
pub const ENTRY_FOG: u16 = 60;
pub const ENTRY_SKIP: u16 = 60;
/// Where the entry's path starts, from the landing start: this much
/// higher and this much farther from the pad. The ship eases in along
/// `(1 - u)^2` and reaches the landing start at rest.
pub const ENTRY_UP: i32 = 140 * M;
pub const ENTRY_OUT: i32 = 80 * M;
/// The entry's plasma streaks.
pub const STREAKS: u32 = 24;

/// Gravity in 1/100 m/s^2: `G_LOW` at the smallest planet (2,200 m),
/// `G_LOW + G_SPAN` at 2,200 + 1,800 m (SPEC 3.5).
pub const G_LOW: i32 = 400;
pub const G_SPAN: i32 = 500;
pub const G_R0: i32 = 2200 * M;
pub const G_RSPAN: i32 = 1800 * M;
/// The least lift (1/100 of g), whatever the load: one tonne carried
/// takes 1/100 off `LIFT` (SPEC 3.5).
pub const LIFT_MIN: u16 = 110;
/// The horizontal push (1/100 m/s^2), the drag (a share a second, in
/// 1/100) and the top horizontal speed (m/s).
pub const HPUSH: i32 = 500;
pub const DRAG: i32 = 50;
pub const HSPEED_MAX: i32 = 20;
/// The sink assist: the speed down it holds is alt / `ASSIST_DIV`,
/// within `ASSIST_MIN`..=`ASSIST_MAX` m/s; it closes the gap in about
/// `ASSIST_T` ticks.
pub const ASSIST_DIV: i32 = 5;
pub const ASSIST_MIN: i32 = 2;
pub const ASSIST_MAX: i32 = 15;
pub const ASSIST_T: i32 = 20;
/// Step G2's addition to the spec's rule: the assist's speed down is
/// also kept under what the jets can stop in the height left, braking
/// with `ASSIST_BRAKE` % of the spare lift (`k - 1`). Without it a
/// heavy ship (k near 1.1) comes down the alt/5 line far faster than
/// its 0.1 g of spare lift can stop.
pub const ASSIST_BRAKE: i32 = 70;
/// Jets' heat (in 1/1000): it rises by `HEAT_PER x lift / (HEAT_REF x
/// g) - HEAT_COOL` a second; at `HEAT_MAX` the jets cut out until
/// `HEAT_BACK`. `HEAT_REF` (1/100 of g) is the middle ship's full
/// lift, so a hover heats every ship alike (G2 report: the spec's share
/// of each ship's own full lift would overheat a full DRAY on the
/// assisted way down). `HEAT_PER` is 21, not the spec's 25 (G2 report:
/// with 25 the spec's own assisted descent from 120 m, about 18 s near
/// a hover plus its braking, ends with the jets cut out a few metres up;
/// with 21 a hover gains about 1.1 a second, full lift about 9).
pub const HEAT_PER: i32 = 21_000;
pub const HEAT_REF: i32 = 160;
pub const HEAT_COOL: i32 = 12_000;
pub const HEAT_MAX: i32 = 100_000;
pub const HEAT_BACK: i32 = 60_000;
/// No climbing more than this over the ground; no flying farther than
/// `ROAM` from the pad.
pub const CEILING: i32 = 250 * M;
pub const ROAM: i32 = 400 * M;
/// The feet: at this many m right/left and fore/aft of the middle.
pub const FOOT: i32 = 3 * M;
/// Touchdown speeds in mm/s (SPEC 3.5's table).
pub const GREASE_V: i32 = 1000;
pub const GREASE_S: i32 = 1000;
pub const LAND_V: i32 = 4000;
pub const LAND_S: i32 = 3000;
pub const CRASH_V: i32 = 8000;
pub const CRASH_S: i32 = 6000;
/// The steepest ground the feet may stand on: tan 12 degrees in
/// 1/16384.
pub const TAN12: i32 = 3483;
/// Hull lost: per m/s over `LAND_V` (HARD), when tipped over, per m/s
/// of total speed (CRASH).
pub const HARD_HULL: i32 = 5;
pub const TIP_HULL: u8 = 15;
pub const CRASH_HULL: i32 = 6;
/// The bounce after a tip or a crash (m/s up).
pub const BOUNCE: i32 = 2;
/// Seconds a greased touchdown adds to the site clock (G3's clock).
pub const GREASE_BONUS: u16 = 20;
/// The ship turns toward where it flies at this much a tick (65536 a
/// turn), while it moves faster than `TURN_FROM` m/s.
pub const TURN_RATE: i32 = 120;
pub const TURN_FROM: i32 = 2;
/// How far the ship leans into the push (65536 a turn: 8 degrees).
pub const LEAN: i32 = 1456;

/// The chase camera: behind, above, looking down (65536 a turn: 30
/// degrees), following by 1/`CAM_EASE` a tick, never under the ground
/// plus `CAM_CLEAR`. A tap of L or R (released within `TAP` ticks)
/// turns it 45 degrees.
pub const CAM_BACK: i32 = 24 * M;
pub const CAM_UP: i32 = 10 * M;
pub const CAM_PITCH: u16 = 5461;
pub const CAM_EASE: i32 = 8;
pub const CAM_CLEAR: i32 = 2 * M;
pub const TAP: u8 = 15;

/// The ship's shadow: a hexagon this wide (corner to middle), shown
/// under `SHADOW_FROM`, the darker shade under `SHADOW_DARK`.
pub const SHADOW_R: i32 = 5 * M;
pub const SHADOW_FROM: i32 = 80 * M;
pub const SHADOW_DARK: i32 = 30 * M;
/// Dust under the ship below this height (SPEC 3.5), at most `DUST` specks.
pub const DUST_FROM: i32 = 15 * M;
pub const DUST: u32 = 16;
/// The legs come out under this height.
pub const LEGS_FROM: i32 = 30 * M;

/// The take-off (SPEC 3.7): B (or a finger on JETS) held `LIFT_HOLD`
/// ticks, then `TAKEOFF_TICKS` ticks: the ship rises `TAKEOFF_UP`, the
/// dust ring blows out (`RING_DUST` specks), the camera pulls up by
/// `TAKEOFF_CAM` and back by twice that, and from `TAKEOFF_FADE` the
/// sky goes black.
pub const LIFT_HOLD: u16 = 30;
pub const TAKEOFF_TICKS: u16 = 180;
pub const TAKEOFF_UP: i32 = 60 * M;
pub const TAKEOFF_CAM: i32 = 20 * M;
pub const TAKEOFF_FADE: u16 = 90;
pub const RING_DUST: u32 = 32;

/// The ship's size below: its table units times this / 256. SHIP_SCALE
/// x 3/8 makes the LARK about 12 m long, as SPEC 3.10 asks ("its
/// existing model at landing scale, 12 m long").
pub const LAND_SCALE: i32 = crate::models::SHIP_SCALE * 3 / 8;

// ---- The picture (SPEC 3.10) -------------------------------------------------

/// Ring sizes per detail level (LOW, MID, HIGH): the half-width of each
/// square ring around the camera. Near rings are snapped to 16 m and
/// must be multiples of 16 m (SPEC's 56 and 88 m become 48 and 80 m);
/// mid and far rings are snapped to 32 m and are multiples of 32 m.
pub const NEAR_R: [i32; 3] = [32 * M, 48 * M, 80 * M];
pub const MID_R: [i32; 3] = [96 * M, 160 * M, 224 * M];
pub const FAR_R: [i32; 3] = [256 * M, 384 * M, 448 * M];
/// Step G2: the mid ring while the far ring is on (the camera 20 m up or
/// more: the descent's views). HIGH's is 192 m, not 224 m: the board
/// timed G1's high view at HIGH at 27.2 to 28.1 ms a step with 914
/// triangles (lead's notes after G1), and the descent's view adds the
/// shadow, the dust and the landing panel; 192 m takes HIGH's worst
/// high view from 983 to 887 triangles (MEASURED ON THE PC, G2.md).
/// The walk's view (under 20 m) keeps `MID_R`.
pub const MID_R_HIGH: [i32; 3] = [96 * M, 160 * M, 192 * M];
/// The far ring is drawn only while the camera is this high or more.
pub const FAR_FROM: i32 = 20 * M;
/// Haze begins at this share of the reach (in 1/100) and is full at
/// `HAZE_FULL`; HAZE's own air is thicker (SPEC: 60 % reach).
pub const HAZE_FROM: i32 = 50;
pub const HAZE_FULL: i32 = 75;
pub const HAZE_LOOK_REACH: i32 = 60;
/// Faces steeper than 25 degrees are rock (B): the up part of a face's
/// normal is under cos 25 (in 1/16384).
pub const STEEP_COS: i32 = 14_849;

// ---- Step G3: on foot (SPEC 3.6, 3.7, 3.12; all CHOICE) ----------------------
//
// Speeds are in m/s and accelerations in m/s^2 here; the walker turns
// them into fine units (`lander::FINE`) a tick. Times are in ticks.

/// The landed ship opens its hatch this long after the touchdown (B
/// held before that lifts off at once, as in G2), and the pilot steps
/// out.
pub const HATCH_OPEN: u16 = 60;
/// The hatch: this far to the ship's right of its middle.
pub const HATCH_OUT: i32 = 5 * M;
/// Within this of the hatch the suit refills, the jets fill, and B
/// held `LIFT_HOLD` ticks boards the ship and lifts off.
pub const HATCH_R: i32 = 3 * M;
/// Ticks the pilot takes to go in (he is drawn shrinking into the
/// hatch), then the take-off.
pub const BOARD_TICKS: u16 = 10;
/// The pilot: radius and height (1/256 m).
pub const PILOT_R: i32 = M * 4 / 10;
pub const PILOT_H: i32 = M * 18 / 10;
/// Walking: top speed, acceleration and braking (m/s, m/s^2), turn
/// (65536 a turn a tick: 540 degrees a second).
pub const WALK: i32 = 4;
pub const WALK_ACC: i32 = 24;
pub const WALK_BRAKE: i32 = 30;
pub const WALK_TURN: i32 = 1638;
/// Shallow water slows the walk to this (1/10 m/s).
pub const WADE_TENTHS: i32 = 25;
/// The pilot steps up this much at once (1/256 m).
pub const STEP_UP: i32 = M / 2;
/// Slopes over 35 degrees (tan in 1/16384) cannot be climbed and slide
/// the pilot down at `SLIDE` m/s.
pub const TAN35: i32 = 11_472;
pub const SLIDE: i32 = 3;
/// The jump's speed up, in 1/10 m/s.
pub const JUMP_TENTHS: i32 = 45;
/// Hop jets: from `HOP_AFTER` ticks in the air, A held pushes up at
/// g + `HOP_ACC` until the climb is `HOP_CLIMB_TENTHS`; the stick steers
/// at `AIR_ACC` up to `AIR_TOP` m/s in the air.
pub const HOP_AFTER: u16 = 12;
pub const HOP_ACC: i32 = 4;
pub const HOP_CLIMB_TENTHS: i32 = 30;
pub const AIR_ACC: i32 = 6;
pub const AIR_TOP: i32 = 5;
/// Jet energy: full, burnt a tick while jetting, refilled a tick
/// `HOP_REFILL_AFTER` ticks after landing (energy in 1/60: 100 is
/// 6,000; 30 a second is 30 a tick).
pub const HOP_ENERGY: i32 = 100 * 60;
pub const HOP_BURN: i32 = 30;
pub const HOP_REFILL: i32 = 25;
pub const HOP_REFILL_AFTER: u16 = 30;
/// Falls: landing faster than `FALL_SAFE` m/s costs (v - 12) x
/// `FALL_COST` suit points.
pub const FALL_SAFE: i32 = 12;
pub const FALL_COST: i32 = 4;
/// The suit by shield level (`car.shield` 0..=2), its refill at the
/// hatch (a second; kept in 1/60 of a point, so it is also a tick's
/// share), `SUIT_LOW` percent for the note.
pub const SUIT_MAX: [u8; 3] = [80, 100, 130];
pub const SUIT_REFILL: i32 = 25;
pub const SUIT_LOW: i32 = 25;
/// The walk's limit: past `WALK_R` from the pad the walk is slowed to a
/// quarter (note SUIT RANGE: TURN BACK); `WALK_EDGE` is a wall.
pub const WALK_R: i32 = 170 * M;
pub const WALK_EDGE: i32 = 190 * M;
/// Solid things the pilot walks round (radius, 1/256 m): the ship, the
/// wreck, an uncut seam, a boulder at rest.
pub const SHIP_SOLID: i32 = M * 35 / 10;
pub const WRECK_SOLID: i32 = 3 * M;
pub const SEAM_SOLID: i32 = M;
pub const BOULDER_R: i32 = M * 15 / 10;

/// The camera on foot: this far behind the pilot, `FOOT_SIDE` to the
/// right (over his shoulder) and this high over his feet, looking at a
/// point `FOOT_AHEAD` in front of him at chest height (`FOOT_AIM`), so
/// he stands left of the middle and what he walks to is in the middle;
/// it eases by 1/`FOOT_EASE` a tick;
/// the right stick turns it by up to `FOOT_TURN` a tick and lifts it
/// by up to `FOOT_TILT_STEP` a tick within `FOOT_TILT`.
pub const FOOT_BACK: i32 = 6 * M;
pub const FOOT_UP: i32 = M * 25 / 10;
pub const FOOT_AIM: i32 = M * 12 / 10;
pub const FOOT_SIDE: i32 = M * 9 / 10;
pub const FOOT_AHEAD: i32 = 4 * M;
pub const FOOT_EASE: i32 = 4;
pub const FOOT_TURN: i32 = 546;
pub const FOOT_TILT_STEP: i32 = M / 12;
pub const FOOT_TILT: [i32; 2] = [-(3 * M / 2), 6 * M];
/// The camera on foot stays this far over the ground and over the
/// ground between it and the pilot.
pub const FOOT_CLEAR: i32 = M;

/// The cutter: reach and cone (cos 25 degrees in 1/16384), heat in
/// 1/60 (100 is 6,000: +40 a second firing, -30 a second idle, cut out
/// at 100 until 40).
pub const CUT_REACH: i32 = 30 * M;
pub const CUT_CONE: i32 = 14_849;
pub const CUT_HEAT_MAX: i32 = 100 * 60;
pub const CUT_HEAT_UP: i32 = 40;
pub const CUT_HEAT_DOWN: i32 = 30;
pub const CUT_RESUME: i32 = 40 * 60;
/// A seam stage's beam time by laser level (0.8, 0.6, 0.45 s), the
/// wreck's the same, a boulder's base `BOULDER_LOOSE`.
pub const STAGE_TIME: [u16; 3] = [48, 36, 27];
pub const SEAM_STAGES: u8 = 3;
pub const WRECK_STAGES: u8 = 2;
pub const BOULDER_LOOSE: u16 = 30;
/// A seam bursts into `SEAM_TONNES[danger]` chunks of its good; the
/// wreck into `WRECK_TONNES` of SPARES.
///
/// Step G4 balance (BOARD-G3 4.4): SPEC 3.6 has `2 + danger`
/// ([2, 3, 4, 5]); raised at SECURE and QUIET and held at 4 from RISKY
/// on, so that a whole site's chunks still fit the pool (`CHUNKS`, 24:
/// checked below; the planet state has no room for more, `Below` is
/// near its 2,048 B) and a SECURE visit pays about the SPEC's 330 CR.
pub const SEAM_TONNES: [u8; 4] = [3, 3, 4, 4];
/// SPEC 3.6: 2 (kept).
pub const WRECK_TONNES: u8 = 2;
/// Step G4: a downed drone drops `DRONE_TONNES` of SPARES (SPEC 3.11:
/// "2 drones x 1 t SPARES").
pub const DRONE_TONNES: u8 = 1;

/// Chunks: how many can be loose (4 seams of 5 and the wreck's 2 fit),
/// the throw (out 2..4 m/s, up 3 m/s), the roll (0.6 g x gradient,
/// slowed by 2 m/s^2), the stop (under 0.3 m/s, or after 6 s), the
/// pickup reach (1.5 m), the chunk's radius.
pub const CHUNKS: usize = 24;
pub const THROW_OUT: [i32; 2] = [2, 4];
pub const THROW_UP: i32 = 3;
pub const CHUNK_PUSH: i32 = 60;
pub const CHUNK_DRAG: i32 = 2;
pub const CHUNK_STOP_TENTHS: i32 = 3;
pub const CHUNK_LIFE: u16 = 360;
pub const PICKUP_R: i32 = M * 15 / 10;
pub const CHUNK_R: i32 = M / 5;
/// Boulders: the roll (0.8 g x gradient, no drag over 5 degrees, else
/// 3 m/s^2, 15 m/s at most), the hit (30 suit points, knocked away at
/// 4 m/s), and a roll's longest time (then it stops where it is).
pub const BOULDER_PUSH: i32 = 80;
pub const TAN5: i32 = 1433;
pub const BOULDER_DRAG: i32 = 3;
pub const BOULDER_TOP: i32 = 15;
pub const BOULDER_HIT: i32 = 30;
pub const KNOCK: i32 = 4;
pub const BOULDER_LIFE: u16 = 1200;

/// The site clock by danger (s), from the touchdown, plus the greased
/// landing's bonus; the warning `STORM_WARN` s before its end; then the
/// storm closes the fog to `STORM_NEAR` over `STORM_CLOSE` ticks and the
/// suit loses `STORM_DRAIN` a second (1/60 a tick) away from the hatch.
///
/// Step G4 balance (BOARD-G3 4.4): SPEC 3.9 has [240, 210, 180, 150];
/// the drones' fights take time, and at 150 s a LAWLESS visit cracked
/// two or three seams, so the dangerous sites got more time (the order
/// kept: the more dangerous, the shorter).
pub const SITE_TIME: [u16; 4] = [240, 225, 210, 195];
pub const STORM_WARN: u16 = 30;
pub const STORM_NEAR: i32 = 24 * M;
pub const STORM_CLOSE: u16 = 1200;
pub const STORM_DRAIN: i32 = 3;
/// The storm's palette steps (the sky and the air slide toward the
/// weather's colour in this many steps over `STORM_CLOSE`).
pub const STORM_STEPS: u8 = 4;

// ---- Step G4: drones, the scan, props, sparks (SPEC 3.6, 3.10, 3.12; all
// CHOICE unless the spec gives the number) ----------------------------------

/// A drone: its patrol (`DRONE_UP` over the ground, circling its seam at
/// `DRONE_ORBIT` and `DRONE_SPEED` m/s), its body's radius, and its eye's
/// height over its middle.
pub const DRONE_UP: i32 = 3 * M;
pub const DRONE_ORBIT: i32 = 12 * M;
pub const DRONE_SPEED: i32 = 2;
pub const DRONE_R: i32 = M * 6 / 10;
/// Sight: within `SIGHT_FAR` and the cone (cos 60 degrees in 1/16384)
/// of its facing, or within `SIGHT_NEAR` any way round. No line of
/// sight test (SPEC 3.6).
pub const SIGHT_FAR: i32 = 25 * M;
pub const SIGHT_NEAR: i32 = 10 * M;
pub const SIGHT_CONE: i32 = 8192;
/// The telegraph (SPEC 3.6, never cut): `DRONE_AIM` ticks of a red line
/// from the eye to the pilot; the last `DRONE_LOCK` ticks it stops
/// following him (locked, flashing white), then the bolt.
pub const DRONE_AIM: u16 = 60;
pub const DRONE_LOCK: u16 = 9;
/// Where the lock aims: this high over the pilot's feet (his middle).
pub const DRONE_AIM_H: i32 = M * 9 / 10;
/// The bolt: 30 m/s (`BOLT_STEP` world units a tick, 0.5 m), it hits
/// within `BOLT_HIT` of the pilot's upright axis, it flies at most
/// `BOLT_TICKS` (40 m).
pub const BOLT_STEP: i32 = M / 2;
pub const BOLT_HIT: i32 = M * 7 / 10;
pub const BOLT_TICKS: u8 = 80;
/// The bolt's damage, `BOLT_BASE + BOLT_PER x danger` suit points; the
/// drone's points `DRONE_HP + DRONE_HP_PER x danger`.
pub const BOLT_BASE: i32 = 8;
pub const BOLT_PER: i32 = 4;
pub const DRONE_HP: i32 = 40;
pub const DRONE_HP_PER: i32 = 10;
/// After a bolt it cools this long, then aims again if it still sees
/// the pilot; past `DRONE_LEASH` from its seam it goes back to patrol.
pub const DRONE_COOL: u16 = 96;
pub const DRONE_LEASH: i32 = 40 * M;
/// The cutter's damage to a drone a second by laser level (points).
pub const CUT_DPS: [i32; 3] = [30, 45, 70];
/// A hit drone flashes this many ticks; the burst lasts `BURST_TICKS`.
pub const HIT_FLASH: u8 = 4;
pub const BURST_TICKS: u16 = 4;
/// A rolling boulder crushes a drone whose middle is within
/// `BOULDER_R + DRONE_R + CRUSH_SLACK` across (SPEC: 1.5 m plus the
/// drone's radius) and whose bottom is under the boulder's top.
pub const CRUSH_SLACK: i32 = 0;

/// The scan (Y on foot): costs `SCAN_COST` jet energy (1/60: 20), the
/// ring grows to `SCAN_R` in `SCAN_GROW` ticks with `SCAN_POINTS`
/// points; what it passes is marked for `SCAN_MARK` ticks.
pub const SCAN_COST: i32 = 20 * 60;
pub const SCAN_R: i32 = 60 * M;
pub const SCAN_GROW: u16 = 60;
pub const SCAN_POINTS: u32 = 48;
pub const SCAN_MARK: u16 = 600;

/// Props: one try per `PROP_CELL` square cell, kept by the cell's hash
/// against `PROP_DENSITY` (in 1/256, per look), drawn up to `PROP_REACH`
/// by level; one in `ROCK_EVERY` is a rock (solid, `ROCK_PROP_R`).
/// None within `PROP_CLEAR` of the pad's middle or of a seam, the wreck
/// or a boulder.
pub const PROP_CELL: i32 = 8 * M;
pub const PROP_DENSITY: [u32; 8] = [110, 60, 70, 80, 0, 150, 120, 70];
pub const PROP_REACH: [i32; 3] = [24 * M, 48 * M, 72 * M];
pub const ROCK_EVERY: u32 = 4;
pub const ROCK_PROP_R: i32 = M * 8 / 10;
pub const PROP_CLEAR: i32 = 20 * M;
pub const PROP_NEAR_THING: i32 = 4 * M;
/// Flattening: jets (the ship's or the pilot's) within `FLAT_R` lay a
/// plant down for `FLAT_TICKS`; at most `FLATS` at once (the oldest is
/// reused).
pub const FLAT_R: i32 = 4 * M;
pub const FLAT_TICKS: u16 = 180;
pub const FLATS: usize = 16;

/// The particle pool (SPEC 3.10): 64 sparks of 8 bytes.
pub const SPARKS: usize = 64;
/// The wreck's smoke column: `SMOKE_PUFFS` puffs rising to `SMOKE_UP`,
/// one every `SMOKE_EVERY` ticks, until the wreck is stripped.
pub const SMOKE_PUFFS: u32 = 8;
pub const SMOKE_UP: i32 = 40 * M;
pub const SMOKE_EVERY: u32 = 45;

/// Step G4: a whole site's chunks (every seam, the wreck, every drone)
/// fit the chunk pool at every danger.
const _: () = {
    let mut d = 0;
    while d < 4 {
        let drones = if DRONES_BASE as usize + d < DRONES_MAX as usize {
            DRONES_BASE as usize + d
        } else {
            DRONES_MAX as usize
        };
        assert!(
            SEAMS * SEAM_TONNES[d] as usize
                + WRECK_TONNES as usize
                + drones * DRONE_TONNES as usize
                <= CHUNKS
        );
        d += 1;
    }
};
