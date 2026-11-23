//! Step G1: the palette below (SPEC 3.10, "The palette below").
//!
//! MERCATOR's palette is shade-major: entry = shade x 25 + material
//! (`art::MATS` 25, `art::SHADES` 8), then 40 UI entries from 200. The
//! spec's "entries 40-167" assumed material-major entries, so the plan
//! is moved onto this layout (G1 report): below, the eight shades of
//! the space-only materials 5 to 12 (raider, hauler, warden and station
//! colours, never drawn below) are 64 flat entries, numbered by
//! [`flat`]. Materials 0-4 (the ship's hull, glass and engine), 14-15
//! (lights), 23-24 (gunmetal, flash) and every UI entry keep their
//! space colours; 13 and 16-18 are G3's models, 21 (canister) and 22
//! (panel) G4's plants and drone stripe.

use super::models;
use super::tune::STORM_STEPS;
use crate::art::{self, COLOURS_LEN, MATS};

/// The flat entry `k` (0..64): shade `k % 8` of material `5 + k / 8`.
pub const fn flat(k: u8) -> u8 {
    (k % 8) * MATS + 5 + k / 8
}

/// Terrain: material (A, B, C) x 12 + haze x 4 + light (0 brightest).
pub const fn land(m: u8, haze: u8, light: u8) -> u8 {
    flat(m * 12 + haze * 4 + light)
}
/// Water (or glowing lava): haze x 4 + light.
pub const fn water(haze: u8, light: u8) -> u8 {
    flat(36 + haze * 4 + light)
}
/// The sky's ten bands, 0 at the horizon.
pub const SKY_BANDS: u8 = 10;
pub const fn sky(band: u8) -> u8 {
    flat(48 + band)
}
/// Shadows: dark and lighter.
pub const SHADOW: u8 = flat(58);
pub const SHADOW2: u8 = flat(59);
/// Step G2: the entry's streaks, from the star's colour (0) to the
/// air's (2).
pub const fn streak(k: u8) -> u8 {
    flat(60 + if k > 2 { 2 } else { k })
}
/// Step G2: dust blown off the ground.
pub const DUST: u8 = flat(63);

/// Each look's ground, A (flat), B (steep), C (high); original colours
/// picked for G1.
const GROUND: [[(u8, u8, u8); 3]; 8] = [
    [(96, 150, 72), (112, 104, 92), (214, 206, 172)], // OCEAN: grass, rock, pale stone
    [(214, 170, 110), (160, 108, 66), (236, 212, 164)], // DESERT: sand, red rock, pale dune
    [(222, 232, 242), (150, 186, 214), (112, 122, 142)], // ICE: snow, blue ice, dark rock
    [(78, 62, 58), (46, 38, 38), (132, 100, 86)],     // LAVA: basalt, dark, ash
    [(96, 150, 72), (112, 104, 92), (214, 206, 172)], // GIANT: never drawn
    [(58, 128, 60), (84, 76, 52), (150, 186, 110)],   // JUNGLE: green, mud, moss
    [(128, 98, 150), (86, 64, 108), (186, 160, 206)], // HAZE: violet ground
    [(182, 98, 64), (124, 64, 48), (214, 172, 140)],  // RUST: rust, dark red, pale
];
/// Water per look (LAVA: glowing rock, lit by itself).
const WATER: [(u8, u8, u8); 8] = [
    (40, 104, 160),
    (40, 104, 160),
    (40, 104, 160),
    (255, 124, 40),
    (40, 104, 160),
    (52, 98, 74),
    (84, 72, 132),
    (40, 104, 160),
];
/// The four light steps (in 1/256) and the haze steps' share of air.
const LIGHT_K: [u32; 4] = [256, 205, 160, 118];
const HAZE_K: [u32; 3] = [0, 102, 192];

fn scale((r, g, b): (u8, u8, u8), k: u32) -> (u8, u8, u8) {
    // k at most 256: the products fit.
    let s = |v: u8| (u32::from(v) * k / 256).min(255) as u8;
    (s(r), s(g), s(b))
}

fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: u32) -> (u8, u8, u8) {
    let m = |x: u8, y: u8| ((u32::from(x) * (256 - t) + u32::from(y) * t) / 256) as u8;
    (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// The palette below for a system: space's (`art::palette`), with the
/// flat entries made the land, the water, the sky and the shadows. On
/// return to space the app sets space's palette again.
#[inline(never)]
pub fn below(out: &mut [u16; COLOURS_LEN], look: u8, star: u8, h: u32, storm: u8) {
    art::palette(out, look, star, h);
    let l = usize::from(look % 8);
    // Step G3: the storm slides the air (and so the haze and the sky)
    // toward the weather's colour.
    let k = u32::from(storm.min(STORM_STEPS)) * 200 / u32::from(STORM_STEPS);
    let air = mix(art::AIRS[l], WEATHER[l], k);
    let put = |out: &mut [u16; COLOURS_LEN], e: u8, rgb| out[usize::from(e)] = art::c565(rgb, 256);
    for m in 0..3u8 {
        for hz in 0..3u8 {
            for li in 0..4u8 {
                let c = scale(GROUND[l][usize::from(m)], LIGHT_K[usize::from(li)]);
                put(out, land(m, hz, li), mix(c, air, HAZE_K[usize::from(hz)]));
            }
        }
    }
    for hz in 0..3u8 {
        for li in 0..4u8 {
            // Lava glows: no light steps.
            let k = if look == 3 {
                256
            } else {
                LIGHT_K[usize::from(li)]
            };
            let c = scale(WATER[l], k);
            put(out, water(hz, li), mix(c, air, HAZE_K[usize::from(hz)]));
        }
    }
    // The sky: the air at the horizon to a zenith at a quarter of its
    // brightness, tinted 30 % by the sun's rim colour.
    let zenith = mix(scale(air, 64), art::STARS[usize::from(star % 5)][1], 77);
    for b in 0..SKY_BANDS {
        let t = u32::from(b) * 256 / u32::from(SKY_BANDS - 1);
        put(out, sky(b), mix(air, zenith, t));
    }
    put(out, SHADOW, scale(GROUND[l][0], 90));
    put(out, SHADOW2, scale(GROUND[l][0], 128));
    // Step G2: the streaks run from the star's rim colour to the air;
    // the dust is the flat ground, paled by the air.
    let rim = art::STARS[usize::from(star % 5)][1];
    for k in 0..3u8 {
        put(out, streak(k), mix(rim, air, u32::from(k) * 110));
    }
    put(out, DUST, mix(GROUND[l][0], air, 110));
    // Step G3: the models below (planet::models): the seam's good, the
    // rock, the suit's white and its orange stripe, in eight shades.
    let mats = [
        (
            models::DEPOSIT,
            DEPOSIT[usize::from(super::cutter::seam_good(look))],
        ),
        (models::ROCK, scale(GROUND[l][1], 200)),
        (models::SUIT, (232, 236, 240)),
        (models::STRIPE, (250, 130, 40)),
        // Step G4: the plants' colour by look, the drones' stripe.
        (models::PLANT, PLANT[l]),
        (models::DRONE_STRIPE, (200, 64, 48)),
    ];
    for (m, rgb) in mats {
        for sh in 0..art::SHADES {
            let e = usize::from(sh * MATS + m);
            out[e] = art::c565(rgb, SHADE_K[usize::from(sh)]);
        }
    }
}

/// Step G4: the plants per look (OCEAN reeds, DESERT scrub, ICE spikes,
/// LAVA basalt, GIANT none, JUNGLE ferns, HAZE spore stalks, RUST
/// pillars).
const PLANT: [(u8, u8, u8); 8] = [
    (118, 146, 78),
    (136, 124, 72),
    (196, 228, 246),
    (62, 54, 54),
    (118, 146, 78),
    (64, 150, 62),
    (204, 132, 204),
    (150, 82, 60),
];
/// The deposit's colour by good (GRAIN pods, ICE core, ORE copper).
const DEPOSIT: [(u8, u8, u8); 3] = [(120, 210, 90), (150, 220, 255), (226, 140, 70)];
/// The storm's colour per look (OCEAN, DESERT, ICE, LAVA, GIANT,
/// JUNGLE, HAZE, RUST): rain grey, dust, white, ash.
const WEATHER: [(u8, u8, u8); 8] = [
    (96, 110, 124),
    (176, 140, 96),
    (230, 236, 244),
    (70, 62, 60),
    (96, 110, 124),
    (84, 104, 96),
    (110, 100, 124),
    (164, 104, 76),
];
/// The shades' brightness (as `art`'s, in 1/256).
const SHADE_K: [u32; 8] = [256, 216, 180, 148, 118, 90, 64, 40];
