//! How a face's material becomes a palette number.

use crate::fixed::{ONE14, SHIFT14};
use crate::sums::{Checked, Sums};

/// A shading rule: the palette number of a face from its material, its
/// direction (turned into the world, unit length in 1/16384) and its
/// depth (1/256 m from the eye). The renderer calls it once per face
/// that is drawn. Ready rules: [`Shading`], [`Lit`], [`Steps`], [`Flat`].
pub trait Shade {
    fn paint<S: Sums>(&self, mat: u8, n_world: [i32; 3], depth: i32) -> u8;
}

/// KESTREL's rule, with fog (`FOG = true`, called [`Shading`]) or
/// without it (`Lit<false>`: the fog fields are not read and the fog
/// sums are not built).
///
/// Palette number = `shade * mats + material`, with shade 0 the
/// brightest and `shades - 1` the darkest. Light falls along `light`
/// (unit length, in 1/16384, pointing from the light to the scene).
/// Materials whose bit is set in `glow` shine by themselves: no light,
/// but still fog. Fog darkens faces from `fog_from` to `fog_to` (camera
/// distance, 1/256 m) by up to `fog_shades` steps. Material numbers must
/// be below 32 (one bit each in `glow` and `clear`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lit<const FOG: bool> {
    pub mats: u8,
    pub shades: u8,
    pub light: [i32; 3],
    pub glow: u32,
    pub fog_from: i32,
    pub fog_to: i32,
    pub fog_shades: u8,
    /// Materials the fog never dims (bit per material): what the player
    /// must find from afar.
    pub clear: u32,
    /// Shade of a face turned fully away from the light (0..shades).
    pub dark_side: u8,
}

/// KESTREL's rule with fog.
pub type Shading = Lit<true>;

impl<const FOG: bool> Lit<FOG> {
    /// The shade of a face from its turned direction (world space,
    /// 1/16384), material and depth (checked sums).
    #[inline(always)]
    pub fn shade(&self, mat: u8, n_world: [i32; 3], depth: i32) -> u8 {
        self.shade_with::<Checked>(mat, n_world, depth)
    }

    /// [`Lit::shade`] with the sums `S`.
    #[inline]
    pub fn shade_with<S: Sums>(&self, mat: u8, n_world: [i32; 3], depth: i32) -> u8 {
        let lit = if self.glow & S::bit(mat) != 0 {
            0
        } else {
            // Light on the face: 0 (none) .. 16384 (straight on).
            let d = S::neg(S::add(
                S::add(
                    S::mul(n_world[0], self.light[0]),
                    S::mul(n_world[1], self.light[1]),
                ),
                S::mul(n_world[2], self.light[2]),
            )) >> SHIFT14;
            let d = d.clamp(0, ONE14);
            (S::mul(S::sub(ONE14, d), i32::from(self.dark_side)) >> SHIFT14) as u8
        };
        let fog = if !FOG
            || self.clear & S::bit(mat) != 0
            || depth <= self.fog_from
            || self.fog_to <= self.fog_from
        {
            0
        } else {
            let t = S::sub(depth, self.fog_from).min(S::sub(self.fog_to, self.fog_from));
            (S::mul(t, i32::from(self.fog_shades)) / S::sub(self.fog_to, self.fog_from)) as u8
        };
        S::add8(lit, fog).min(S::sub8(self.shades, 1))
    }

    /// The palette number of a material in a shade (checked sums).
    #[inline(always)]
    pub fn colour(&self, mat: u8, shade: u8) -> u8 {
        self.colour_with::<Checked>(mat, shade)
    }

    /// [`Lit::colour`] with the sums `S`.
    #[inline]
    pub fn colour_with<S: Sums>(&self, mat: u8, shade: u8) -> u8 {
        S::add8(S::mul8(shade, self.mats), mat)
    }
}

impl<const FOG: bool> Shade for Lit<FOG> {
    #[inline(always)]
    fn paint<S: Sums>(&self, mat: u8, n_world: [i32; 3], depth: i32) -> u8 {
        self.colour_with::<S>(mat, self.shade_with::<S>(mat, n_world, depth))
    }
}

/// CAMBER's rule: `base[mat]` plus a light step, 0 (lit) to 3 (dark),
/// except for glowing materials (always step 0). Materials 0..8; no fog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Steps {
    pub base: [u8; 8],
    /// Bit per material: shines by itself (always step 0).
    pub glow: u8,
    /// Light falls along this (unit, 1/16384, from the light).
    pub light: [i32; 3],
}

impl Shade for Steps {
    #[inline]
    fn paint<S: Sums>(&self, mat: u8, n_world: [i32; 3], _depth: i32) -> u8 {
        let m = usize::from(mat & 7);
        if self.glow & (1 << m) != 0 {
            return self.base[m];
        }
        let d = S::neg(S::add(
            S::add(
                S::mul(n_world[0], self.light[0]),
                S::mul(n_world[1], self.light[1]),
            ),
            S::mul(n_world[2], self.light[2]),
        )) >> SHIFT14;
        // 0..16384 of light: four steps, the darkest for faces turned away.
        let step = 3 - (S::mul(d.clamp(0, ONE14 - 1), 4) / ONE14).min(3);
        S::add8(self.base[m], step as u8)
    }
}

/// No light at all: the material number is the palette number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flat;

impl Shade for Flat {
    #[inline(always)]
    fn paint<S: Sums>(&self, mat: u8, _n_world: [i32; 3], _depth: i32) -> u8 {
        mat
    }
}
