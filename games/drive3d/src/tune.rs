//! Every tunable number of the rules, with its unit and where it comes
//! from. All are first guesses (ESTIMATED) to change in play; the
//! prototype's spec, kept outside the repository, is "the spec".
//!
//! Units (the spec's terms):
//! * *sub*: 1/4096 m (positions and speeds); drawing uses `sub >> 4`,
//!   game-3d's unit of 1/256 m;
//! * *step*: one turn of the rules, 1/30 s; a speed is in subs a step,
//!   so 30 m/s is 4,096;
//! * *angle*: 1/65,536 of a turn (`u16`, wraps), game-3d's: yaw 0
//!   looks along +z, a quarter turn along +x (to the right);
//! * *of 256*: a share; 128 of 256 is a half.

// ---- Units and the clock ----------------------------------------------

/// Subs in one metre.
pub const M: i32 = 4096;
/// Subs in one game-3d unit (1/256 m).
pub const SUB_PER_UNIT: i32 = 16;
/// One step in microseconds (30 steps a second).
pub const STEP_US: u32 = 33_333;
/// Most steps run in one update (the rest of the time waits).
pub const MAX_STEPS: u8 = 2;

// ---- Input ------------------------------------------------------------

/// Stick dead zone (of 32,767): 1/8 of the stick. Spec, guess.
pub const DEAD: i32 = 4096;
/// Steering moves towards the wanted value by this much a step (of
/// 256): 6 steps from straight to full lock. Spec, from TALUS.
pub const RAMP_IN: i32 = 43;
/// And back towards straight by this much: 4 steps. Spec, from TALUS.
pub const RAMP_OUT: i32 = 64;

// ---- The car ----------------------------------------------------------

/// Turning at full lock: 546 angle units a step, 90 degrees a second
/// (546 * 30 = 16,380, a quarter turn is 16,384). Spec, guess.
pub const TURN: i32 = 546;
/// Full turning from this forward speed (10 m/s); slower turns less,
/// in proportion. Spec, guess.
pub const VT: i32 = 1365;
/// Engine: speed added a step at a standstill with full throttle (subs
/// a step a step); it falls off in a straight line to 0 at `TOP`.
///
/// **Changed from the spec's 23.** The spec wants "0 to 25 m/s in about
/// 6 s on gravel". Its rule `fwd += ACC * (TOP - fwd) / TOP` reaches a
/// share `1 - e^(-n ACC / TOP)` of `TOP` after n steps; 25 of 30 m/s
/// needs `n = TOP / ACC * ln 6 = 4096 / 23 * 1.79 = 319` steps
/// (10.6 s) with 23, and `ACC = 4096 * 1.79 / 180 = 40.8` for 6 s.
/// Hence 41 (a start of 41 * 900 / 4096 = 9.0 m/s/s, 0.92 g).
pub const ACC: i32 = 41;
/// Brakes: 36 subs a step a step is 7.9 m/s/s (0.8 g), held to the
/// surface's `CAP`. Spec, guess.
pub const BRAKE: i32 = 36;
/// Reverse: the brake held at a standstill drives backwards, up to
/// this speed (subs a step: 8 m/s). DEFAULT (the spec has reversing in
/// a test but no rule for it).
pub const REV_TOP: i32 = 1092;
/// Reverse acceleration at a standstill (subs a step a step), falling
/// off like the engine's. DEFAULT: half of `ACC`.
pub const REV_ACC: i32 = 20;
/// Gravity: 45 subs a step a step is 9.8 m/s/s (45 * 900 / 4096 =
/// 9.89). Spec, the checker's fix.
pub const G: i32 = 45;
/// Grip and its cap with the handbrake: 96 of 256 (3/8). Spec,
/// between TALUS (40 %) and SCREE (25 %).
pub const HB: i32 = 96;
/// Handbrake turning: the turn times 3/2. Spec.
pub const HB_TURN_NUM: i32 = 3;
pub const HB_TURN_DEN: i32 = 2;
/// Landing: no loss below this downward speed (subs a step: 1.3 m/s).
/// Spec, guess.
pub const LAND_FREE: i32 = 180;
/// Landing: the loss (of 256) is the downward speed over `LAND_FREE`
/// divided by this. Spec, guess.
pub const LAND_DIV: i32 = 8;
/// Landing: at most this loss (of 256: 25 %). Spec, guess.
pub const LAND_MAX: i32 = 64;
/// Landing crossed up: the extra loss is `|side| * LAND_SIDE / v` of
/// 256 (25 % landing fully sideways). Spec.
pub const LAND_SIDE: i32 = 64;
/// Landing crossed up counts only after a flight of at least this many
/// steps. DEFAULT, not in the spec: triangle land has a slope break at
/// every corner, and the car hops over many of them for 1 to 3 steps
/// (see the report); without this every such hop in a slide would cost
/// speed.
pub const LAND_SIDE_MIN_AIR: u16 = 4;
/// Y: steps from the press to the car standing on the road again
/// (1.5 s). Spec, guess.
pub const RESET_STEPS: u8 = 45;

/// Dust (review look F1): puffs are left in the world at the rear
/// wheels, one every `PUFF_EVERY` steps, alternating sides, while the
/// car is on the ground on gravel or in the ford, going forward faster
/// than `DUST_SPEED` (subs a step: 3 m/s), or spinning its wheels
/// against a thing or the edge (throttle held, slower than
/// `BLOCKED_SPEED`). A puff ends after `PUFF_LIFE` steps. All DEFAULT.
pub const PUFFS: usize = 8;
pub const PUFF_EVERY: u32 = 2;
pub const PUFF_LIFE: u8 = 16;
pub const DUST_SPEED: i32 = 410;
/// A car slower than this (subs a step: 1 m/s) with the throttle held
/// that touches a thing or the edge is blocked; after `BLOCKED_HINT`
/// steps of that (1 s) the picture says how to get out (review look
/// F2). DEFAULT.
pub const BLOCKED_SPEED: i32 = 137;
pub const BLOCKED_HINT: u16 = 30;

/// The car's middle is kept at least this far from the land block's
/// edge (8 m). Spec.
pub const EDGE: i32 = 8 * M;
/// A car that hits the edge keeps this share of its speed (of 256): it
/// loses 32. Spec.
pub const EDGE_KEEP: i32 = 224;
/// The edge's loss counts only when the car meets it faster than this
/// outward (subs a step: 1 m/s); leaning on it with the engine costs
/// nothing more than the outward speed. DEFAULT (review F3: the loss on
/// every pressed step held the car there).
pub const EDGE_HIT_MIN: i32 = 137;
/// With the throttle held, the steering turns the nose at least as much
/// as at this forward speed (subs a step: 1.25 m/s, an eighth of `VT`),
/// so a car stopped against the edge or a thing can turn away. DEFAULT
/// (review F3).
pub const TURN_FLOOR: i32 = VT / 8;

/// Power out of a slide (R2, the owner's rule of 2026-11-22: throttle
/// in a slide gives grip back and pushes the car out of the corner).
/// The slide's share counts from 0 to full as the sideways speed grows
/// to `POWER_SLIDE` of 256 of the speed (64: about 14 degrees of slip).
pub const POWER_SLIDE: i32 = 64;
/// Full throttle in a full slide raises the grip and its cap by this
/// share (of 256): 256, twice (gravel's cap 0.7 g -> 1.4 g). Not on
/// grass: the grass gives nothing back (else the verge beats the road).
/// Tuned on the PC corner tests (`tests/review_rules.rs`): 128 kept
/// "full throttle in" off the road, 192 let the road win by one step.
pub const POWER_GRIP: i32 = 256;
/// And turns this share (of 256) of the sideways speed the grip takes
/// into speed along the nose, under the surface's top: 32, an eighth
/// (64 and more let a line over the grass beat the road).
pub const POWER_PUSH: i32 = 32;

/// The car, as the rules see it against things beside the road: a
/// circle of this radius (1.2 m). DEFAULT.
pub const CAR_RADIUS: i32 = M * 6 / 5;
/// A car that hits a thing keeps this share of its speed (of 256).
/// DEFAULT: a quarter lost.
pub const HIT_KEEP: i32 = 192;
/// The car's box for drawing and for the camera tests (subs): 4.0 m
/// long, 1.8 m wide, 1.4 m tall, the middle of its base at `pos`.
/// DEFAULT (the spec gives no size); the drawing may change it, and
/// then the camera test's 12 to 40 px must be checked again.
pub const CAR_LEN: i32 = 4 * M;
pub const CAR_WIDTH: i32 = M * 9 / 5;
pub const CAR_HEIGHT: i32 = M * 7 / 5;

/// One surface's handling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handling {
    /// Share of the sideways speed the tyres can take away in a step
    /// at a standstill (of 256); less at speed (see `car`). SCREE's.
    pub grip: i32,
    /// Most sideways (and braking) change in one step (subs a step a
    /// step). TALUS's grip in g: 31 is 0.7 g (31 * 900 / 4096 = 6.8
    /// m/s/s).
    pub cap: i32,
    /// Share of the speed lost each step (of 256). SCREE's.
    pub drag: i32,
    /// Top speed (subs a step). SCREE's shares of 30 m/s.
    pub top: i32,
}

/// Surface numbers, as kept in the land's cell bytes (3 bits).
pub const GRAVEL: u8 = 0;
pub const GRASS: u8 = 1;
pub const FORD: u8 = 2;
/// The low grass on the cut inside the second corner (the spec's
/// "shorter, slower" line): rough, so the shorter way costs more time
/// than it saves (review F4).
pub const LOW_GRASS: u8 = 3;
/// How many surfaces there are.
pub const SURFACES: usize = 4;

/// The table of the spec, section 1, by surface number.
pub const HANDLING: [Handling; SURFACES] = [
    // Gravel (the road): 30 m/s.
    Handling {
        grip: 150,
        cap: 31,
        drag: 1,
        top: 4096,
    },
    // Grass (R1, owner's rule 2026-11-22: the road is the best way
    // through a corner): 0.22 g, 40 % (12 m/s). Was 0.5 g, 85 %, which
    // let a line over the verge beat the road. Light drag (3): heavy drag
    // above TOP works as a brake stronger than the brakes, and a car
    // that runs onto the grass at the apex then corners faster.
    // `tests/review_rules.rs` (`finding_the_cut...`) holds the rule.
    Handling {
        grip: 50,
        cap: 10,
        drag: 3,
        top: 1638,
    },
    // Ford: 0.45 g, 45 % (13.5 m/s).
    Handling {
        grip: 90,
        cap: 20,
        drag: 10,
        top: 1843,
    },
    // Low grass on the cut: since R1 the same as the grass (it gripped
    // better before, 110/22/6/1638, and the verge then beat the road;
    // review F4, `tests/review_rules.rs`). Only its colour differs.
    Handling {
        grip: 50,
        cap: 10,
        drag: 3,
        top: 1638,
    },
];

/// The surfaces' names, for the screen and the log.
pub const SURFACE_NAMES: [&str; SURFACES] = ["GRAVEL", "GRASS", "FORD", "LOW GRASS"];

// ---- The chase camera (spec section 2) ---------------------------------

/// Behind the car along the aim (9.5 m) and above it (2.8 m). SCREE.
pub const BACK: i32 = 38_912;
pub const UP: i32 = 11_469;
/// The look point: ahead of the car (6 m) and above it (0.6 m). TALUS.
pub const AIM_AHEAD: i32 = 24_576;
pub const AIM_UP: i32 = 2_458;
/// In a slide the aim moves this share (of 256) towards the way the
/// car travels (0.35). SCREE.
pub const VEL_BLEND: i32 = 90;
/// The aim only follows the travel above this speed (subs a step: 1
/// m/s), and only going forwards. DEFAULT: a standing or reversing
/// car has no useful travel direction for the camera.
pub const BLEND_MIN_V: i32 = 137;
/// The road's direction this many road points (of 2 m) ahead of the
/// last one passed: 18 m. TALUS.
pub const ROAD_LOOK: u16 = 9;
/// The aim turns this share (of 256) towards that direction (35 %),
/// at most `ROAD_CAP` (8 degrees). TALUS, ABREAST.
pub const ROAD_TURN: i32 = 90;
pub const ROAD_CAP: i32 = 1_456;
/// The camera stays at least this far above the land under it (1.5
/// m). SCREE.
pub const CLEAR: i32 = 6_144;
/// In a dip it rises by up to 1 m; to see over a crest by up to 3 m.
/// ABREAST, guess.
pub const DIP_RISE: i32 = 4_096;
pub const HIDE_RISE: i32 = 12_288;
/// Points checked on the line from the camera to the car's roof.
/// Spec: four.
pub const HIDE_POINTS: i32 = 4;
/// The camera's place moves this share of the way to its wanted place
/// each step (a quarter); its yaw this share (a fifth). Guess.
pub const EASE_POS: i32 = 64;
pub const EASE_YAW: i32 = 52;
/// Most downward look (about 20 degrees, ABREAST's 0.35 radians).
pub const PITCH_MAX: u16 = 3_650;
/// The lens: pixels for 1 m at 1 m (as in all three study mocks).
pub const FOCAL: i32 = 100;

// ---- The stretch (spec section 3) ---------------------------------------

/// The land block: 32 by 48 cells of 8 m.
pub const CELLS_X: usize = 32;
pub const CELLS_Z: usize = 48;
pub const CORNERS_X: usize = CELLS_X + 1;
pub const CORNERS_Z: usize = CELLS_Z + 1;
/// One cell's side (8 m) in subs.
pub const CELL: i32 = 8 * M;
/// One step of the height map (1/4 m) in subs.
pub const HEIGHT_STEP: i32 = M / 4;
/// The road's first point: 104 m across (a corner column, so the long
/// first straight lies on corners), 24 m in, 8 m up. DEFAULT.
pub const START_X: i32 = 104 * M;
pub const START_Z: i32 = 24 * M;
pub const START_Y: i32 = 8 * M;
/// Land within this distance of the road's line is at the road's
/// height (6 m: half the road, 3 m, plus 3 m of flat shoulder).
/// DEFAULT: the spec starts the rise at the road's edge; a flat 3 m
/// keeps the land under the road within 0.25 m on the curves.
pub const FLAT: i32 = 6 * M;
/// Beyond `FLAT` the land rises 1 m for every 8 m out. Spec.
pub const RISE_PER: i32 = 8;
/// The fixed pattern of 0 to 0.5 m starts this far from the road
/// (14 m). DEFAULT: nearer, it would put bumps on the shoulders.
pub const PATTERN_FROM: i32 = 14 * M;
/// The water band across the land at the ford: 16 m wide. Spec.
pub const FORD_BAND: i32 = 16 * M;
/// The inside of the second corner is cut flat this far out (16 m).
/// DEFAULT (the spec says "low grass" and no width).
pub const CUT_WIDTH: i32 = 16 * M;
/// A cell is gentle (a rock may stand on it) when its four corners
/// differ by at most this many height steps (0.5 m). DEFAULT.
pub const GENTLE: u8 = 2;
/// The car moves `last_road` only to a road point within this
/// distance (24 m), so a car far off the road keeps its last point.
/// DEFAULT.
pub const ROAD_CATCH: i32 = 24 * M;
