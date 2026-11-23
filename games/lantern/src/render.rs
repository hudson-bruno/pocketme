//! The first-person view, by ray casting, in whole numbers.
//!
//! For every ray (one per column, or one per two columns in the light
//! detail) the renderer walks the map grid from the player's cell, one
//! cell border at a time, until it meets a wall (the "DDA" walk: it
//! always steps to whichever border, x or y, is nearer along the ray).
//! The distance to that wall, measured straight ahead (not along the
//! ray, so walls do not bulge), gives the column's wall height. The
//! wall's texture is read straight down one texture column. A small
//! table per column keeps that distance (the depth buffer) so sprites
//! further away than the wall are hidden behind it.
//!
//! Every pixel's colour is `shade * MATS + material` (see
//! [`crate::art`]): the lamp's light falls off with distance through the
//! palette, at the cost of one addition.
//!
//! The renderer counts its own work in [`Cost`] (rays, grid steps,
//! multiplications, divisions, pixels written). The counts are plain
//! additions per ray or per row, cheap enough to keep on the chip too.

use game_kit::{Frame, W};

use crate::art::{self, DARK, MATS, SEE_THROUGH, SPR, SPRITES, TEX, TEXS, Texture, shaded};
use crate::fixed::{ONE, SHIFT, TRIG_SHIFT, cos, sin};
use crate::level::MAX_SIDE;
use crate::world::{Kind, MAX_THINGS, Mood, World, cell, is_door};

/// How much the renderer draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    /// A 120 x 80 window, 60 rays each two pixels wide, plain floor and
    /// ceiling. About half the pixels of NORMAL to send.
    Light,
    /// The full 160 x 104 view, 160 rays, plain floor and ceiling.
    Normal,
    /// NORMAL with a textured floor and ceiling: every pixel of the view
    /// changes when the player moves.
    Heavy,
}

impl Detail {
    pub const ALL: [Detail; 3] = [Detail::Light, Detail::Normal, Detail::Heavy];

    pub fn name(self) -> &'static str {
        match self {
            Detail::Light => "LIGHT",
            Detail::Normal => "NORMAL",
            Detail::Heavy => "HEAVY",
        }
    }

    /// Where the 3D view is and how it is drawn.
    pub const fn view(self) -> View {
        match self {
            Detail::Light => View {
                x0: 20,
                y0: 12,
                w: 120,
                h: 80,
                rays: 60,
                col_w: 2,
                textured: false,
            },
            Detail::Normal => View {
                x0: 0,
                y0: 0,
                w: 160,
                h: VIEW_H,
                rays: 160,
                col_w: 1,
                textured: false,
            },
            Detail::Heavy => View {
                x0: 0,
                y0: 0,
                w: 160,
                h: VIEW_H,
                rays: 160,
                col_w: 1,
                textured: true,
            },
        }
    }
}

/// The height of the play picture above the status line.
pub const VIEW_H: i32 = 104;
/// The camera's half width at distance 1 (the field of view: about 66
/// degrees), in 1/16384.
pub const PLANE: i32 = 10_813;
/// Rays closer than this (1/64 cell) are clamped (the player never gets
/// this close, a wall is at least 0.27 cells away).
const MIN_DIST: i32 = 64;
/// Ray step size for an axis the ray barely moves along: 64 cells, more
/// than any map's diagonal, and small enough that 4096 x it fits in i32.
const FAR: i32 = 1 << 18;
/// How far sprites are looked at: creatures' eyes show in the dark.
const SPRITE_REACH: i32 = 9 * ONE;

/// A view: where on the picture and how many rays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    pub x0: i32,
    pub y0: i32,
    pub w: i32,
    pub h: i32,
    pub rays: i32,
    pub col_w: i32,
    pub textured: bool,
}

impl View {
    /// Pixels from the eye to the picture: (w / 2) / (plane width).
    pub const fn focal(&self) -> i32 {
        self.w * 16_384 / (2 * PLANE)
    }
}

/// The work of the last frame, counted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cost {
    pub rays: u32,
    /// Grid steps of all rays (the walk through the map).
    pub steps: u32,
    pub muls: u32,
    pub divs: u32,
    /// Pixels written: wall columns, plain floor and ceiling, textured
    /// floor and ceiling, sprites.
    pub wall_px: u32,
    pub flat_px: u32,
    pub floor_px: u32,
    pub sprite_px: u32,
    /// Sprites drawn (at least one column in front of the walls).
    pub sprites: u32,
}

impl Cost {
    pub fn pixels(&self) -> u32 {
        self.wall_px + self.flat_px + self.floor_px + self.sprite_px
    }
}

/// The renderer's tables (kept in the game's state, never on the stack).
pub struct Renderer {
    /// Straight-ahead distance of each ray's wall (the depth buffer).
    pub zbuf: [i32; 160],
    /// Each ray's wall top and bottom on the picture (rows).
    top: [i16; 160],
    bottom: [i16; 160],
    /// Sprites to draw: thing index and depth, sorted far to near.
    order: [(u8, i32); MAX_THINGS],
    pub cost: Cost,
}

impl Default for Renderer {
    fn default() -> Self {
        Renderer::new()
    }
}

impl Renderer {
    pub const fn new() -> Renderer {
        Renderer {
            zbuf: [0; 160],
            top: [0; 160],
            bottom: [0; 160],
            order: [(0, 0); MAX_THINGS],
            cost: Cost {
                rays: 0,
                steps: 0,
                muls: 0,
                divs: 0,
                wall_px: 0,
                flat_px: 0,
                floor_px: 0,
                sprite_px: 0,
                sprites: 0,
            },
        }
    }

    /// Draws the world as the player sees it into the view's part of
    /// the picture.
    pub fn draw(&mut self, s: &mut Frame, w: &World, detail: Detail) {
        self.cost = Cost::default();
        let v = detail.view();
        let focal = v.focal();
        let cy = v.y0 + v.h / 2;
        let (dx, dy) = (cos(w.angle), sin(w.angle));
        let (plx, ply) = ((-dy * PLANE) >> TRIG_SHIFT, (dx * PLANE) >> TRIG_SHIFT);
        self.cost.muls += 2;

        // Light: shade = distance x 7 / reach, up to black. The flash
        // lights everything three steps brighter for a moment.
        let reach = w.light();
        let k = ((DARK as i32) << 16) / reach;
        self.cost.divs += 1;
        let boost = if w.glow > 0 { 3 } else { 0 };
        let shade = |d: i32| -> u8 {
            let sh = ((d.min(8 * ONE) * k) >> 16).min(DARK as i32);
            (sh - boost).max(0) as u8
        };

        // Plain floor and ceiling: one colour per row, by its distance.
        let mut row_col = [0u8; VIEW_H as usize];
        if !v.textured {
            for r in 0..v.h / 2 {
                // Eye at half a wall: a row r below the middle shows the
                // floor at focal * 0.5 / (r + 0.5) cells.
                let d = (focal * ONE) / (2 * r + 1);
                self.cost.divs += 1;
                let sh = shade(d);
                let below = (v.h / 2 + r) as usize;
                let above = (v.h / 2 - 1 - r) as usize;
                row_col[below] = shaded(art::DIRT1, sh);
                row_col[above] = shaded(art::SLATE, sh);
            }
        }

        // ---- Walls ------------------------------------------------------
        let px = w.x;
        let py = w.y;
        for i in 0..v.rays {
            self.cost.rays += 1;
            // Camera x from -1 (left edge) to +1 (right edge), 1/16384.
            let cam = ((2 * i + 1 - v.rays) * 16_384) / v.rays;
            let rdx = dx + ((plx * cam) >> TRIG_SHIFT);
            let rdy = dy + ((ply * cam) >> TRIG_SHIFT);
            self.cost.divs += 1;
            self.cost.muls += 2;
            let hit = self.cast(w, px, py, rdx, rdy);
            let (perp, tex, tx, side) = hit;
            self.zbuf[i as usize] = perp;

            let line = (focal * ONE) / perp;
            self.cost.divs += 1;
            let t0 = cy - line / 2;
            let top = t0.max(v.y0);
            let bottom = (cy + line / 2).min(v.y0 + v.h);
            self.top[i as usize] = top as i16;
            self.bottom[i as usize] = bottom as i16;
            let sh = (shade(perp) + side).min(DARK);
            let off = sh * MATS as u8;
            // Texture rows per picture row, in 1/65536.
            let step = ((TEX as i32) << 16) / line.max(1);
            self.cost.divs += 1;
            let pos = ((top - t0) * step) as u32;
            self.cost.muls += 1;
            let column = &TEXS[tex].as_chunks::<TEX>().0[tx & (TEX - 1)];
            let n_top = (top - v.y0) as usize;
            let n_wall = (bottom - top).max(0) as usize;
            let rows = &mut s.0[v.y0 as usize * W..(v.y0 + v.h) as usize * W];
            let flats = &row_col[..v.h as usize];
            let x = (v.x0 + i * v.col_w) as usize;
            let c = Col {
                x,
                n_top,
                n_wall,
                tex: column,
                p: pos,
                step: step as u32,
                off,
                flat: !v.textured,
            };
            if v.col_w == 2 {
                c.draw::<2>(rows, flats);
            } else {
                c.draw::<1>(rows, flats);
            }
            let wall = n_wall as u32 * v.col_w as u32;
            self.cost.wall_px += wall;
            if !v.textured {
                self.cost.flat_px += (v.h as u32 * v.col_w as u32) - wall;
            }
        }

        // ---- Textured floor and ceiling (HEAVY) --------------------------
        if v.textured {
            self.floor(s, w, &v, focal, reach, &shade, (dx, dy), (plx, ply));
        }

        // ---- Sprites ------------------------------------------------------
        self.sprites(s, w, &v, focal, &shade, (dx, dy));
    }

    /// Walks one ray through the grid. Returns the straight-ahead
    /// distance, the texture, its column and 1 for a y-side wall (drawn
    /// one step darker, so corners show).
    fn cast(&mut self, w: &World, px: i32, py: i32, rdx: i32, rdy: i32) -> (i32, usize, usize, u8) {
        let mut mx = px >> SHIFT;
        let mut my = py >> SHIFT;
        // Distance along the ray from one x border to the next (and y).
        let ddx = if rdx.abs() < 256 {
            FAR
        } else {
            ((1 << 26) / rdx.abs()).min(FAR)
        };
        let ddy = if rdy.abs() < 256 {
            FAR
        } else {
            ((1 << 26) / rdy.abs()).min(FAR)
        };
        self.cost.divs += 2;
        let fx = px & (ONE - 1);
        let fy = py & (ONE - 1);
        let (sx, mut side_x) = if rdx < 0 {
            (-1, (fx * ddx) >> SHIFT)
        } else {
            (1, ((ONE - fx) * ddx) >> SHIFT)
        };
        let (sy, mut side_y) = if rdy < 0 {
            (-1, (fy * ddy) >> SHIFT)
        } else {
            (1, ((ONE - fy) * ddy) >> SHIFT)
        };
        self.cost.muls += 2;
        // At most this many steps: a map is closed by walls, so a ray
        // always stops sooner; this only guards against a bad map.
        for _ in 0..(2 * MAX_SIDE as u32 + 2) {
            self.cost.steps += 1;
            let side = if side_x < side_y {
                side_x = side_x.saturating_add(ddx);
                mx += sx;
                0u8
            } else {
                side_y = side_y.saturating_add(ddy);
                my += sy;
                1u8
            };
            let c = w.at(mx, my);
            if c == cell::FLOOR {
                continue;
            }
            let perp = if side == 0 {
                side_x - ddx
            } else {
                side_y - ddy
            }
            .max(MIN_DIST);
            // Where along the face the ray hit, 0..4096.
            let along = if side == 0 {
                py + ((perp as i64 * rdy as i64) >> TRIG_SHIFT) as i32
            } else {
                px + ((perp as i64 * rdx as i64) >> TRIG_SHIFT) as i32
            };
            self.cost.muls += 1;
            // Measured along the map (+x or +y), so a door slides the same
            // way seen from either side.
            let mut frac = along & (ONE - 1);
            if is_door(c) {
                // A door slides sideways as it opens: the open part lets
                // the ray through.
                // 255 (fully open) becomes 4096: every ray passes.
                let o = (i32::from(w.open_at(mx, my)) * 4112) >> 8;
                if frac < o {
                    continue;
                }
                frac -= o;
            }
            // Texture column 0 on the viewer's left. Looking east the
            // screen's right is +y, looking south it is -x: so a face
            // seen while looking west (-x) or south (+y) is turned round.
            if (side == 0 && rdx < 0) || (side == 1 && rdy > 0) {
                frac = ONE - 1 - frac;
            }
            let tex = texture_of(c);
            return (perp, tex, (frac >> (SHIFT - 5)) as usize & (TEX - 1), side);
        }
        (FAR, art::T_ROCK, 0, 1)
    }

    /// The textured floor and ceiling, row by row, under and over the
    /// walls already drawn.
    #[allow(clippy::too_many_arguments)]
    fn floor(
        &mut self,
        s: &mut Frame,
        w: &World,
        v: &View,
        focal: i32,
        reach: i32,
        shade: &dyn Fn(i32) -> u8,
        (dx, dy): (i32, i32),
        (plx, ply): (i32, i32),
    ) {
        let floor: &Texture = &TEXS[art::T_FLOOR];
        let ceil: &Texture = &TEXS[art::T_CEIL];
        // The leftmost and rightmost rays.
        let (l_x, l_y) = (dx - plx, dy - ply);
        let (r_x, r_y) = (dx + plx, dy + ply);
        let half = v.h / 2;
        let black = shaded(art::BLACK, 0);
        for r in 0..half {
            let d = (focal * ONE) / (2 * r + 1);
            self.cost.divs += 1;
            let below = v.y0 + half + r;
            let above = v.y0 + half - 1 - r;
            let x0 = v.x0 as usize;
            let (wb, wa) = (below as usize * W + x0, above as usize * W + x0);
            if d > reach {
                // Too far for the lamp: black, where no wall covers it.
                let mut n = 0u32;
                let row_b = &mut s.0[wb..][..v.w as usize];
                for (p, &b) in row_b.iter_mut().zip(self.bottom.iter()) {
                    if below >= i32::from(b) {
                        *p = black;
                        n = n.wrapping_add(1);
                    }
                }
                let row_a = &mut s.0[wa..][..v.w as usize];
                for (p, &t) in row_a.iter_mut().zip(self.top.iter()) {
                    if above < i32::from(t) {
                        *p = black;
                        n = n.wrapping_add(1);
                    }
                }
                self.cost.floor_px += n;
                continue;
            }
            // The floor point under the left ray, and the step per column.
            let fx = w.x + ((d * l_x) >> TRIG_SHIFT);
            let fy = w.y + ((d * l_y) >> TRIG_SHIFT);
            let sx = ((d * (r_x - l_x)) >> TRIG_SHIFT) / v.w;
            let sy = ((d * (r_y - l_y)) >> TRIG_SHIFT) / v.w;
            self.cost.muls += 4;
            self.cost.divs += 2;
            let off = shade(d) * MATS as u8;
            // The same floor point is above (ceiling) and below (floor);
            // the sums wrap on purpose (see `Col::draw`): positions stay
            // far inside i32.
            let mut n = 0u32;
            let row_b = &mut s.0[wb..][..v.w as usize];
            let (mut x, mut y) = (fx, fy);
            for (p, &b) in row_b.iter_mut().zip(self.bottom.iter()) {
                if below >= i32::from(b) {
                    let t = (((x >> 7) & 31) << 5 | ((y >> 7) & 31)) as usize;
                    *p = floor[t & 1023].wrapping_add(off);
                    n = n.wrapping_add(1);
                }
                x = x.wrapping_add(sx);
                y = y.wrapping_add(sy);
            }
            let row_a = &mut s.0[wa..][..v.w as usize];
            let (mut x, mut y) = (fx, fy);
            for (p, &t) in row_a.iter_mut().zip(self.top.iter()) {
                if above < i32::from(t) {
                    let t = (((x >> 7) & 31) << 5 | ((y >> 7) & 31)) as usize;
                    *p = ceil[t & 1023].wrapping_add(off);
                    n = n.wrapping_add(1);
                }
                x = x.wrapping_add(sx);
                y = y.wrapping_add(sy);
            }
            self.cost.floor_px += n;
        }
    }

    fn sprites(
        &mut self,
        s: &mut Frame,
        w: &World,
        v: &View,
        focal: i32,
        shade: &dyn Fn(i32) -> u8,
        (c, sn): (i32, i32),
    ) {
        // Which things are in front, and how far.
        let mut n = 0;
        for (i, t) in w.things().iter().enumerate() {
            if !t.alive {
                continue;
            }
            let (dx, dy) = (t.x - w.x, t.y - w.y);
            if dx.abs() > SPRITE_REACH || dy.abs() > SPRITE_REACH {
                continue;
            }
            let depth = (c * dx + sn * dy) >> TRIG_SHIFT;
            self.cost.muls += 2;
            if depth < ONE / 5 {
                continue;
            }
            // Things (not creatures) out of the light are not drawn: they
            // would be black on black.
            if !t.kind.creature() && shade(depth) >= DARK {
                continue;
            }
            self.order[n] = (i as u8, depth);
            n += 1;
        }
        // Far to near (insertion sort: a handful of things).
        for i in 1..n {
            let mut j = i;
            while j > 0 && self.order[j - 1].1 < self.order[j].1 {
                self.order.swap(j - 1, j);
                j -= 1;
            }
        }
        let cy = v.y0 + v.h / 2;
        for k in 0..n {
            let (i, depth) = self.order[k];
            let t = &w.things()[usize::from(i)];
            let (dx, dy) = (t.x - w.x, t.y - w.y);
            let side = (c * dy - sn * dx) >> TRIG_SHIFT;
            self.cost.muls += 2;
            let sx = v.x0 + v.w / 2 + (focal * side) / depth;
            let line = (focal * ONE) / depth;
            self.cost.divs += 2;
            self.cost.muls += 1;
            let (sprite, size, lift) = look(t, w.ticks);
            let size_px = (line * size) >> 8;
            if size_px < 2 {
                continue;
            }
            let bottom = cy + line / 2 - ((line * lift) >> 8);
            let top = bottom - size_px;
            let left = sx - size_px / 2;
            self.cost.muls += 3;
            let step = ((SPR as i32) << 16) / size_px;
            self.cost.divs += 1;
            let off = shade(depth) * MATS as u8;
            let tex = &SPRITES[sprite];
            let x_from = left.max(v.x0);
            let x_to = (left + size_px).min(v.x0 + v.w);
            let y_from = top.max(v.y0);
            let y_to = bottom.min(v.y0 + v.h);
            if x_from >= x_to || y_from >= y_to {
                continue;
            }
            let mut drew = false;
            let mut n = 0u32;
            let rows = &mut s.0[y_from as usize * W..y_to as usize * W];
            for x in x_from..x_to {
                let ray = ((x - v.x0) / v.col_w) as usize;
                if depth >= self.zbuf[ray] {
                    continue;
                }
                drew = true;
                let tx = (((x - left) * step) >> 16) as usize & (SPR - 1);
                let column = &tex.as_chunks::<SPR>().0[tx];
                let mut p = ((y_from - top) * step) as u32;
                let xu = x as usize;
                for r in rows.as_chunks_mut::<W>().0 {
                    let m = column[(p >> 16) as usize & (SPR - 1)];
                    if m != SEE_THROUGH {
                        r[xu] = m.wrapping_add(off);
                        n = n.wrapping_add(1);
                    }
                    p = p.wrapping_add(step as u32);
                }
            }
            self.cost.sprite_px += n;
            if drew {
                self.cost.sprites += 1;
            }
        }
    }
}

/// A wall cell's texture.
pub fn texture_of(c: u8) -> usize {
    match c {
        cell::TIMBER => art::T_TIMBER,
        cell::VEIN => art::T_VEIN,
        cell::PANEL => art::T_PANEL,
        cell::DOOR => art::T_DOOR,
        cell::GATE_A => art::T_GATE_A,
        cell::GATE_B => art::T_GATE_B,
        cell::SW_A => art::T_SW_A_OFF,
        cell::SW_A_ON => art::T_SW_A_ON,
        cell::SW_B => art::T_SW_B_OFF,
        cell::SW_B_ON => art::T_SW_B_ON,
        cell::LIFT => art::T_LIFT,
        _ => art::T_ROCK,
    }
}

/// A thing's sprite, its size and its height above the floor (both in
/// 1/256 of a wall), animated by the tick.
pub fn look(t: &crate::world::Thing, ticks: u32) -> (usize, i32, i32) {
    let flip = (t.phase / 8).is_multiple_of(2);
    match t.kind {
        Kind::Crystal => (
            if (ticks / 20 + t.phase as u32).is_multiple_of(3) {
                art::S_CRYSTAL1
            } else {
                art::S_CRYSTAL0
            },
            96,
            0,
        ),
        Kind::Oil => (art::S_OIL, 80, 0),
        Kind::Flask => (art::S_FLASK, 80, 0),
        Kind::Crank => (art::S_CRANK, 96, 0),
        Kind::Moth => {
            let bob = (sin(t.phase.wrapping_mul(700)) * 20) >> TRIG_SHIFT;
            let f = if (t.phase / 4).is_multiple_of(2) {
                art::S_MOTH0
            } else {
                art::S_MOTH1
            };
            (f, 104, 84 + bob)
        }
        Kind::Crawler => {
            let f = if t.mood == Mood::Stunned {
                art::S_BUG_CURL
            } else if flip {
                art::S_BUG0
            } else {
                art::S_BUG1
            };
            (f, 128, 0)
        }
        Kind::Digger => {
            let f = if t.mood == Mood::Stunned {
                art::S_DIG_STOP
            } else if (t.phase / 3).is_multiple_of(2) {
                art::S_DIG0
            } else {
                art::S_DIG1
            };
            (f, 176, 0)
        }
    }
}

/// The map cell kind a wall texture stands for is drawn in [`crate::hud`];
/// this keeps the cell index maths in one place.
pub fn cell_index(cx: i32, cy: i32) -> usize {
    cy as usize * MAX_SIDE + cx as usize
}

/// One wall column to draw: `n_top` rows of ceiling, `n_wall` rows of
/// wall (one texture column, stretched), the rest floor.
struct Col<'a> {
    x: usize,
    n_top: usize,
    n_wall: usize,
    tex: &'a [u8; TEX],
    /// Texture row of the first wall row, and the step per picture row,
    /// in 1/65536.
    p: u32,
    step: u32,
    /// Added to every texel: the shade (see [`crate::art`]).
    off: u8,
    /// Draw the plain ceiling and floor too (not in HEAVY).
    flat: bool,
}

impl Col<'_> {
    /// Draws the column `CW` pixels wide into the view's rows. The
    /// sums wrap on purpose: the chip build checks every `+` for
    /// overflow, which would cost a branch per pixel here, and none
    /// of these can overflow (texel + shade < 224; `p` stays below
    /// 32 << 16 plus one step).
    #[inline(always)]
    fn draw<const CW: usize>(&self, rows: &mut [u8], flats: &[u8]) {
        // Plain indexes stepping a row (W) at a time: one bounds check
        // a pixel, nothing else (iterator chains cost twice as many
        // instructions on the chip, counted in the disassembly).
        let mut i = self.x;
        let n_top = self.n_top.min(flats.len());
        let n_wall = self.n_wall.min(flats.len() - n_top);
        if self.flat {
            for &f in &flats[..n_top] {
                for k in 0..CW {
                    rows[i + k] = f;
                }
                i = i.wrapping_add(W);
            }
        } else {
            i = i.wrapping_add(n_top * W);
        }
        let mut p = self.p;
        for _ in 0..n_wall {
            let t = self.tex[(p >> 16) as usize & (TEX - 1)].wrapping_add(self.off);
            for k in 0..CW {
                rows[i + k] = t;
            }
            i = i.wrapping_add(W);
            p = p.wrapping_add(self.step);
        }
        if self.flat {
            for &f in &flats[n_top + n_wall..] {
                for k in 0..CW {
                    rows[i + k] = f;
                }
                i = i.wrapping_add(W);
            }
        }
    }
}
