//! Every starting value of the rules, in one place (PROTOTYPE-SPEC.md,
//! sections 1, 2 and 4). All of them are ESTIMATED first guesses, to be
//! tuned in play; the spec's table says where each came from.
//!
//! Units:
//! * **sub**: 1/4096 m (1/16 of game-3d's unit, `M8` = 256 a metre).
//!   Positions and speeds of the rules are in subs.
//! * **step**: one turn of the rules, 1/30 s.
//! * **angle**: 1/65536 of a turn (game-3d's).

use game_3d::deg;

// ---- Units ------------------------------------------------------------------

/// One metre in subs.
pub const M: i32 = 4096;
/// Subs in one game unit (game-3d's 1/256 m).
pub const SUB_PER_UNIT: i32 = 16;
/// One step in microseconds (1/30 s, rounded down).
pub const STEP_US: u32 = 33_333;
/// At most this many steps in one update.
pub const MAX_STEPS: u8 = 2;

// ---- The hero (section 1) -----------------------------------------------------

/// Hero height, feet to head: subs (1.26 m).
pub const HERO_H: i32 = 5_161;
/// Half the body's width (the square round the feet): subs (0.25 m).
pub const HALF: i32 = 1_024;
/// Full running speed: subs a step (5.0 m/s).
pub const RUN: i32 = 683;
/// Speed change towards the wanted speed on a top: subs a step, each
/// step (full speed in 6 steps, 0.2 s).
pub const ACC: i32 = 114;
/// The same in the air: subs a step, each step.
pub const ACC_AIR: i32 = 57;
/// Speed lost each step on a top with no input: subs a step (stops from
/// full speed in 3 steps, 0.1 s).
pub const BRAKE: i32 = 228;
/// Stick dead zone: stick units of 32,767 (18 %).
pub const DEAD: i32 = 6_000;
/// Most the hero's facing turns in one step: angle (22.5 degrees).
pub const TURN: u16 = 4_096;
/// Gravity: subs a step, each step (25 m/s squared).
pub const G: i32 = 114;
/// Upward speed a jump starts with: subs a step (8.2 m/s).
pub const JUMP: i32 = 1_120;
/// Upward speed left when A is let go early (short hop): subs a step.
pub const CUT: i32 = 400;
/// Fastest fall: subs a step (10 m/s).
pub const FALL_MAX: i32 = 1_365;
/// Steps after walking off a top in which a jump still starts.
pub const COYOTE: u8 = 4;
/// Steps a press of A is kept, waiting for a top to jump from.
pub const BUFFER: u8 = 4;
/// Highest top the hero walks onto without a jump: subs (0.25 m).
pub const STEP_UP: i32 = 1_024;
/// Lowest drop the hero follows down while walking (ramps): subs
/// (0.125 m). Not in the spec: without it the hero running down the
/// ramp leaves it every step and falls in small hops (no jump from them
/// past the coyote steps). ESTIMATED: twice the largest drop in one step
/// running down the ramp (228 subs).
pub const SNAP_DOWN: i32 = 512;
/// Steps the squash pose lasts after a landing.
pub const SQUASH: u8 = 4;
/// Below this the hero has fallen off: subs (20 m below the top).
pub const KILL_Y: i32 = -81_920;
/// Steps frozen after falling off, before the hero is put back.
pub const RESPAWN_STEPS: u8 = 15;
/// Steps standing on one top before its spot becomes the safe spot.
pub const SAFE_STEPS: u16 = 10;
/// How far inside a top's edges the safe spot is moved: subs (0.5 m).
pub const SAFE_IN: i32 = 2_048;
/// Shadow radius on the ground, and the smallest one: subs (0.30 m and
/// 0.16 m).
pub const SH_MAX: i32 = 1_229;
pub const SH_MIN: i32 = 655;
/// Landing ring, outer and inner radius: subs (0.50 m, 0.36 m).
pub const RING_OUT: i32 = 2_048;
pub const RING_IN: i32 = 1_475;
/// Steps the landing ring looks ahead (1.5 s).
pub const RING_LOOK: u16 = 45;
/// Ground covered for one of the two running frames: subs (0.4 m).
/// Not in the spec. ESTIMATED (arithmetic): 12.5 frame changes a second
/// at full speed; the drawer may change it.
pub const RUN_FRAME: u32 = 1_638;

// ---- The camera (section 2) ---------------------------------------------------

/// Eye distance from the aim point: game units (5.2 m).
pub const DIST: i32 = 1_331;
/// Nearest the eye comes to the aim point: game units (2 m).
pub const DIST_MIN: i32 = 512;
/// Pitch down at the start: angle (24 degrees).
pub const PITCH: u16 = deg(24);
/// The player's pitch limits: angle (12 and 40 degrees down).
pub const PITCH_MIN: u16 = deg(12);
pub const PITCH_MAX: u16 = deg(40);
/// Extra pitch over open sky: angle (5 degrees).
pub const SKY_TILT: u16 = deg(5);
/// Steps the sky tilt takes to come in or go.
pub const SKY_STEPS: u16 = 10;
/// Aim point above the ground: game units (0.9 m).
pub const AIM_UP: i32 = 230;
/// The aim's height is held until the feet are this far from it: game
/// units (1.5 m).
pub const HOLD: i32 = 384;
/// Fastest camera turn with the right stick: angle a step (3 degrees).
pub const CAM_TURN: u16 = 546;
/// One L or R turn: angle (45 degrees), over this many steps.
pub const SHOULDER_TURN: u16 = 8_192;
pub const SHOULDER_STEPS: u8 = 6;
/// Steps of running with no camera input before the automatic turn.
pub const AUTO_WAIT: u16 = 30;
/// Fastest automatic turn: angle a step (2 degrees).
pub const AUTO_MAX: i32 = 364;
/// The automatic turn follows only a running direction at most this far
/// from the camera's: angle (30 degrees). Not in the spec: running
/// towards the camera would spin it round, and since the stick is read
/// against the camera, holding it sideways would make the camera and the
/// hero go round in circles (the look review). 30 degrees keeps the
/// d-pad's diagonals (45) out of it. ESTIMATED, tunable.
pub const AUTO_SPAN: i32 = 5_461;
/// The glide follow (SAIL): each step the camera turns this fraction
/// (1/16) of the gap to the hero's run, and at most [`GLIDE_TURN`] (2.5
/// degrees a step, 75 a second): a 90 degree gap closes in about 1.3 s,
/// as a gentle turn. ESTIMATED (PC route), tunable.
pub const GLIDE_EASE: i32 = 16;
pub const GLIDE_TURN: i32 = 455;
/// Gap kept between the eye and a wall the ray hit: game units (0.3 m).
pub const WALL_GAP: i32 = 77;
/// A prop shrinks when its ball comes this near the aim-to-eye line:
/// game units (0.5 m) ...
pub const PROP_LINE: i32 = 128;
/// ... or this near the eye: game units (2 m).
pub const PROP_EYE: i32 = 512;
/// Steps a prop takes to shrink to nothing or grow back.
pub const SHRINK_STEPS: i32 = 6;

// ---- Things to collect (section 4) ------------------------------------------

/// Reach on x and z, from the hero's square: subs (0.4 m).
pub const TAKE: i32 = 1_638;
/// Reach below the feet and above the head: subs (0.2 m).
pub const TAKE_Y: i32 = 819;
/// Spin each step: angle (1/64 turn).
pub const SPIN: u16 = 1_024;
/// Bob: half the height (up and down from the middle): subs (5 cm),
/// and the steps of one whole bob (2 s).
pub const BOB: i32 = 205;
pub const BOB_STEPS: u16 = 60;

// ---- The test areas (AREAS-SPEC.md, sections 0 and 2) -------------------------

/// Top running speed carrying a weight: subs a step (3.5 m/s).
pub const HEAVY_RUN: i32 = 478;
/// Upward speed of a jump carrying a weight: subs a step (peak 0.56 m).
pub const HEAVY_JUMP: i32 = 780;
/// Slowest fall while gliding: subs a step (1.5 m/s).
pub const GLIDE_FALL: i32 = 205;
/// Top running speed in the air while gliding: subs a step (5.5 m/s).
pub const GLIDE_RUN: i32 = 751;
/// Steps of glide per take-off (2.0 s), refilled on any top.
pub const GLIDE_STEPS: u8 = 60;
/// How much a glide slows a faster fall each step: subs a step.
pub const FALL_BRAKE: i32 = 228;
/// How much a FADE block shrinks or grows in one step (of 256; 6 steps,
/// 0.2 s, the props' pace).
pub const FADE_STEP: u16 = 43;
/// Most FADE blocks a world may have (the camera's fade table).
pub const MAX_FADE: usize = 4;
