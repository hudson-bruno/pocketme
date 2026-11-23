//! The tests and their workloads. Each workload is a fixed scene that
//! depends only on its load and one time number ([`Work::run`]), so the
//! same load at the same time gives the same picture every time.
//!
//! **Time.** The moving tests ([`Kind::moves`]: the ball scenes, TRI
//! SPIN, CITY, ISLAND) take the game time in ticks of 1/60 s, so they
//! move at the same real speed at 30 and at 60 calls a second (60 shows
//! twice the pictures). The rest take the call count (the send tests
//! must change every call; the static ones do not move).
//!
//! The picture: rows 0 to 19 are the text band (drawn by [`crate::app`]);
//! rows 20 to 119 are the work area ([`WORK`], 16,000 bytes in one
//! piece), the only place a workload draws (except the 100 % send level,
//! which also changes the band's background).

use game_kit::{Frame, W};

use crate::art::{DIM, FILL0, SCENE0, SEND_A, SEND_B, SKY, TRI0};
use crate::city;
use crate::fixed::{Mat3, SHIFT14, V3, cos, hash, sin};
use crate::island;
use crate::model::{BALL12, BALL32, BALL96};
use crate::poly::{
    Light, M8, Model, Object, Rect, Renderer, SUB, View, fill_tri, in_view, rect_fill,
};

/// Rows of the text band.
pub const BAND_H: i32 = 20;
/// The work area.
pub const WORK: Rect = Rect::new(0, BAND_H, 160, 100);
/// Bytes of the work area.
pub const WORK_BYTES: u32 = 16_000;
/// The 3D view: the work area, a lens of 100 pixels per metre at 1 m.
pub const VIEW: View = View::new(WORK, 100);
/// Most objects in a scene.
pub const MAX_OBJ: usize = 512;
/// Nothing farther than this is drawn (32 m).
pub const FAR: i32 = 32 * M8;

/// What a test does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Load: whole work areas filled with one colour, one after the other.
    Fill,
    /// Load: triangles of the shape [`SHAPES`]`[n]`.
    Tri(usize),
    /// Load: corners turned and projected, nothing drawn.
    Vert,
    /// Load: objects of model [`MODELS`]`[n]`, flying at the eye.
    Scene(usize),
    /// Load: per cent of the picture's rows changed every call.
    Rows,
    /// Load: per cent of 240 squares of 8 x 8 pixels changed every call.
    Dots,
    /// Load: triangles of about 100 pixels, each turning round its own
    /// middle every call ([`spin`]).
    Spin,
    /// Load: buildings of the city street ([`crate::city`]).
    City,
    /// Load: a detail level of the floating island ([`crate::island`]).
    Island,
}

impl Kind {
    /// Whether the test moves by game time (ticks of 1/60 s) rather
    /// than by calls.
    pub const fn moves(self) -> bool {
        matches!(
            self,
            Kind::Scene(_) | Kind::Spin | Kind::City | Kind::Island
        )
    }
}

/// One tick of game time (µs): 1/60 s, the console's frame at 60.
pub const TICK_US: u32 = 16_667;

/// One test of the menu.
#[derive(Clone, Copy, Debug)]
pub struct Test {
    pub name: &'static str,
    pub kind: Kind,
    /// What one unit of load is (on the screen and in the log).
    pub unit: &'static str,
    /// The fixed loads (UP and DOWN).
    pub loads: &'static [u32],
    /// The ramp's first load and its top step (0: no ramp).
    pub base: u32,
    pub kmax: u8,
}

/// The menu, in LEFT / RIGHT order.
pub const TESTS: [Test; 13] = [
    Test {
        name: "FILL",
        kind: Kind::Fill,
        unit: "FILLS",
        loads: &[1, 16, 64, 256, 1024],
        base: 1,
        kmax: 44,
    },
    Test {
        name: "TRI 20",
        kind: Kind::Tri(0),
        unit: "TRIS",
        loads: &[256, 1024, 4096, 16384],
        base: 16,
        kmax: 44,
    },
    Test {
        name: "TRI 100",
        kind: Kind::Tri(1),
        unit: "TRIS",
        loads: &[128, 512, 2048, 8192],
        base: 8,
        kmax: 44,
    },
    Test {
        name: "TRI 400",
        kind: Kind::Tri(2),
        unit: "TRIS",
        loads: &[64, 256, 1024, 4096],
        base: 4,
        kmax: 44,
    },
    Test {
        name: "VERT",
        kind: Kind::Vert,
        unit: "VERTS",
        loads: &[256, 1024, 4096, 16384],
        base: 32,
        kmax: 44,
    },
    Test {
        name: "SCENE 12",
        kind: Kind::Scene(0),
        unit: "OBJ",
        loads: &[8, 32, 128, 512],
        base: 1,
        kmax: 36,
    },
    Test {
        name: "SCENE 32",
        kind: Kind::Scene(1),
        unit: "OBJ",
        loads: &[4, 16, 64, 256],
        base: 1,
        kmax: 32,
    },
    Test {
        name: "SCENE 96",
        kind: Kind::Scene(2),
        unit: "OBJ",
        loads: &[2, 8, 32, 64],
        base: 1,
        kmax: 28,
    },
    Test {
        name: "SEND ROWS",
        kind: Kind::Rows,
        unit: "%",
        loads: &[0, 25, 50, 75, 100],
        base: 0,
        kmax: 0,
    },
    Test {
        name: "SEND DOTS",
        kind: Kind::Dots,
        unit: "%",
        loads: &[5, 10, 25, 50],
        base: 0,
        kmax: 0,
    },
    Test {
        name: "TRI SPIN",
        kind: Kind::Spin,
        unit: "TRIS",
        loads: &[128, 512, 1024, 2048, 4096],
        base: 8,
        kmax: 40,
    },
    Test {
        name: "CITY",
        kind: Kind::City,
        unit: "BLDG",
        loads: &[8, 32, 64, 96, 128],
        base: 1,
        kmax: 28,
    },
    Test {
        name: "ISLAND",
        kind: Kind::Island,
        unit: "LEVEL",
        loads: &[1, 2, 3, 4, 5],
        base: 0,
        kmax: 0,
    },
];

/// The scene models: 12, 32 and 96 triangles.
pub static MODELS: [&Model; 3] = [&BALL12, &BALL32, &BALL96];

/// A triangle shape for the triangle tests: corners in 1/16 pixel from
/// its top-left, its size in whole pixels, and the pixels it paints
/// (the same wherever it is put, moved by whole pixels).
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub p: [[i32; 2]; 3],
    pub w: i32,
    pub h: i32,
    pub px: u32,
}

/// 20, 100 and 400 pixels: the same triangle, `(0, 0), (13, 4), (4, 11)`
/// times 9, 20 and 40 sixteenths of a pixel, moved by a fraction of a
/// pixel so it paints exactly that many (found by a search with the
/// slow filler; `tests/levels.rs` checks them).
pub const SHAPES: [Shape; 3] = [
    Shape {
        p: [[0, 0], [117, 36], [36, 99]],
        w: 8,
        h: 7,
        px: 20,
    },
    Shape {
        p: [[0, 4], [260, 84], [80, 224]],
        w: 17,
        h: 15,
        px: 100,
    },
    Shape {
        p: [[2, 12], [522, 172], [162, 452]],
        w: 33,
        h: 29,
        px: 400,
    },
];

/// What one call of a workload did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Triangles filled.
    pub tris: u32,
    /// Pixels written (fills and triangles; a pixel written twice counts
    /// twice), or for the send tests the pixels changed.
    pub px: u32,
    /// Objects drawn (scenes) or turned (corner test).
    pub objs: u32,
    /// Corners turned and projected.
    pub verts: u32,
    /// Faces looked at, faces facing the eye, faces cut, triangles
    /// dropped (scenes).
    pub faces: u32,
    pub front: u32,
    pub clipped: u32,
    pub dropped: u32,
    /// A sum of every projected corner (corner test), so the work cannot
    /// be left out by the compiler and can be compared.
    pub check: u32,
}

/// The light: from the upper left, behind the eye.
const LIGHT: Light = Light {
    dir: [7239, -11375, 9307],
    first: SCENE0,
};

/// The scenes' cycle: an object flies from 24 m to -2 m (26 m) and
/// starts again; it moves [`SPEED`] units a tick of 1/60 s (24 a call
/// at 30, as in the earlier rounds).
const CYCLE: u32 = 26 * M8 as u32;
const SPEED: u32 = 12;
const Z_FAR: i32 = 24 * M8;

/// The workloads' memory: the renderer and the island's work space
/// (about 5.4 KB, all zero at the start). The sort lists of the scenes,
/// the city and the island (2 KB) are on the stack of the call that
/// uses them (round 3: the data slot was full; the game kit keeps a
/// copy of the game value on its stack anyway, so the deepest stack
/// does not grow).
pub struct Work {
    pub r: Renderer,
    /// The island's posed figure and fine floor strip (about 1.4 KB).
    pub isl: island::Scratch,
}

impl Default for Work {
    fn default() -> Self {
        Work::new()
    }
}

impl Work {
    pub const fn new() -> Work {
        Work {
            r: Renderer::new(),
            isl: island::Scratch::new(),
        }
    }

    /// One call of a test at a load. `t` is the time: for the moving
    /// tests ([`Kind::moves`]) the game time in ticks of 1/60 s since
    /// the test started, for the others the calls since it started.
    /// `first` is set on its first call (and after the picture was
    /// lost): static levels draw their background then.
    pub fn run(&mut self, f: &mut Frame, kind: Kind, load: u32, t: u32, first: bool) -> Counts {
        let tick = t;
        if first && !kind.moves() && kind != Kind::Fill {
            rect_fill(f, &WORK, SKY);
            if kind == Kind::Vert {
                f.text(26, 66, "CORNERS ONLY, NOTHING DRAWN", DIM);
            }
        }
        match kind {
            Kind::Fill => fill(f, load),
            Kind::Tri(s) => tris(f, &SHAPES[s % SHAPES.len()], load),
            Kind::Vert => self.verts(load, tick),
            Kind::Scene(m) => self.scene(f, MODELS[m % MODELS.len()], load, tick),
            Kind::Rows => rows(f, load, tick),
            Kind::Dots => dots(f, load, tick),
            Kind::Spin => spin(f, load, tick),
            Kind::City => self.city(f, load, tick),
            Kind::Island => self.island(f, load, tick),
        }
    }

    /// One call of the island (see [`crate::island`]).
    #[inline(never)]
    fn island(&mut self, f: &mut Frame, load: u32, tick: u32) -> Counts {
        let (mut keys, mut list) = ([0; MAX_OBJ], [0; MAX_OBJ]);
        island::draw(
            &mut self.r,
            &mut keys,
            &mut list,
            &mut self.isl,
            f,
            load,
            tick,
        )
    }

    /// One call of the city (see [`crate::city`]).
    #[inline(never)]
    fn city(&mut self, f: &mut Frame, load: u32, tick: u32) -> Counts {
        let (mut keys, mut list) = ([0; MAX_OBJ], [0; MAX_OBJ]);
        city::draw(&mut self.r, &mut keys, &mut list, f, load, tick)
    }

    /// `load` corners of the 96-triangle ball, 50 to an object: each
    /// object turned by its own matrix, moved, and projected (the
    /// renderer's corner stage, with every corner in view).
    fn verts(&mut self, load: u32, tick: u32) -> Counts {
        let m = &BALL96;
        let per = m.verts.len() as u32;
        let objs = load.div_ceil(per);
        let mut check = 0u32;
        let spin = (tick as u16).wrapping_mul(256);
        for j in 0..objs {
            let rot = Mat3::rotation(
                spin.wrapping_add((j as u16).wrapping_mul(1000)),
                (j as u16).wrapping_mul(700),
                0,
            );
            // Eight across, four down, 12 m away: all in view.
            let centre = [
                ((j % 8) as i32 - 4) * 2 * M8 + M8,
                (((j / 8) % 4) as i32 - 2) * 2 * M8 + M8,
                12 * M8,
            ];
            let left = load.saturating_sub(j.saturating_mul(per)).min(per) as usize;
            let nv = self.r.verts(&VIEW, m, &rot, centre, true, left);
            for p in &self.r.sp[..nv] {
                check =
                    check.rotate_left(5) ^ (u32::from(p[0] as u16) | u32::from(p[1] as u16) << 16);
            }
        }
        Counts {
            verts: load,
            objs,
            check: core::hint::black_box(check),
            ..Counts::default()
        }
    }

    /// Where object `i` of `k` is, `off` units into the cycle (world =
    /// camera place: the eye is at the origin).
    fn place(i: u32, k: u32, off: u32) -> V3 {
        // i below 512, CYCLE below 2^13: no overflow.
        let phase = (i * CYCLE / k + off) % CYCLE;
        let h = hash(i, 7);
        [
            ((h & 2047) as i32 - 1024) * 3 / 4,
            ((h >> 11) & 1023) as i32 - 512,
            Z_FAR - phase as i32,
        ]
    }

    /// `load` balls flying at the eye in a tunnel 6 m wide and 4 m high,
    /// evenly spaced from 24 m to 2 m behind the eye, each turning; the
    /// eye sways a little. Culled against the view, sorted far to near,
    /// cut where they cross the near plane, flat shaded.
    ///
    /// `tick` is in 1/60 s. Every motion is written so that tick `2n`
    /// gives exactly call `n` of the earlier rounds (at 30): the sway's
    /// angles step 100 and 65 a tick (were 200 and 130 a call), the
    /// objects 12 units (were 24), and their turns 311 / 2 and 173 / 2
    /// (the product halved after the multiplication, exact for even
    /// ticks, rounded down for odd ones).
    /// (Not inlined: its 2 KB sort lists stay out of `run`'s frame.)
    #[inline(never)]
    fn scene(&mut self, f: &mut Frame, m: &Model, load: u32, tick: u32) -> Counts {
        rect_fill(f, &WORK, SKY);
        let k = load.min(MAX_OBJ as u32);
        self.r.begin();
        if k == 0 {
            return Counts::default();
        }
        let t = tick as u16;
        let cam = Mat3::rotation(
            (sin(t.wrapping_mul(100)) >> 5) as u16,
            0,
            (sin(t.wrapping_mul(65)) >> 5) as u16,
        )
        .transposed();
        // Below 2 * CYCLE * SPEED (2^18): no overflow.
        let off = (tick % (2 * CYCLE)) * SPEED % CYCLE;
        let turn_a = (tick.wrapping_mul(311) >> 1) as u16;
        let turn_b = (tick.wrapping_mul(173) >> 1) as u16;
        // The farthest object is the first whose place wrapped round:
        // start there, so the list is nearly sorted already.
        let i0 = ((CYCLE - off) * k).div_ceil(CYCLE) % k;
        let (mut keys, mut list) = ([0i16; MAX_OBJ], [0u16; MAX_OBJ]);
        let mut n = 0usize;
        for s in 0..k {
            let i = (i0 + s) % k;
            if let Some(c) = in_view(&VIEW, &cam, Self::place(i, k, off), m.radius, FAR) {
                keys[n] = c[2] as i16;
                list[n] = i as u16;
                n += 1;
            }
        }
        // Far to near (insertion sort; nearly sorted already).
        for a in 1..n {
            let (key, id) = (keys[a], list[a]);
            let mut b = a;
            while b > 0 && keys[b - 1] < key {
                keys[b] = keys[b - 1];
                list[b] = list[b - 1];
                b -= 1;
            }
            keys[b] = key;
            list[b] = id;
        }
        for &i in &list[..n] {
            let i = u32::from(i);
            let h = hash(i, 9);
            let rot = Mat3::rotation(
                (h as u16).wrapping_add(turn_a),
                ((h >> 16) as u16).wrapping_add(turn_b),
                0,
            );
            let o = Object {
                model: m,
                rot,
                centre: cam.apply(Self::place(i, k, off)),
            };
            self.r.draw(f, &VIEW, &cam, &LIGHT, &o);
        }
        let s = self.r.stats;
        Counts {
            tris: s.tris,
            px: s.pixels,
            objs: s.objects,
            verts: s.verts,
            faces: s.faces,
            front: s.front,
            clipped: s.clipped,
            dropped: s.dropped,
            check: 0,
        }
    }
}

/// `load` fills of the whole work area, colours in turn.
fn fill(f: &mut Frame, load: u32) -> Counts {
    let area = &mut f.0[(BAND_H as usize) * W..];
    for i in 0..load {
        area.fill(FILL0 + (i & 3) as u8);
        // The fills before the last must not be left out.
        core::hint::black_box(&mut *area);
    }
    Counts {
        px: load.saturating_mul(WORK_BYTES),
        ..Counts::default()
    }
}

/// `load` triangles of one shape, each moved by whole pixels to its own
/// place in the work area, colours in turn.
fn tris(f: &mut Frame, s: &Shape, load: u32) -> Counts {
    let xr = (W as i32 - s.w) as u32;
    let yr = (WORK.h() - s.h) as u32;
    let mut px = 0u32;
    for i in 0..load {
        let x = (i.wrapping_mul(37) % xr) as i32 * SUB;
        let y = (BAND_H + (i.wrapping_mul(23) % yr) as i32) * SUB;
        let q = s.p.map(|v| [v[0] + x, v[1] + y]);
        px = px.wrapping_add(fill_tri(f, &WORK, &q, TRI0 + (i & 7) as u8));
    }
    Counts {
        tris: load,
        px,
        ..Counts::default()
    }
}

/// The bottom `load` per cent of the picture's rows, in one colour on
/// even calls and another on odd ones.
fn rows(f: &mut Frame, load: u32, tick: u32) -> Counts {
    let n = (load.min(100) * 120 / 100) as i32;
    let c = if tick & 1 == 0 { SEND_A } else { SEND_B };
    rect_fill(f, &Rect::new(0, 120 - n, 160, n), c);
    Counts {
        px: n as u32 * 160,
        ..Counts::default()
    }
}

/// Squares of 8 x 8 on a 20 x 12 grid in the work area: `load` per cent
/// of the 240 changed every call (spread: square b when b * 37 mod 240
/// is below the count).
fn dots(f: &mut Frame, load: u32, tick: u32) -> Counts {
    let count = load.min(100) * 240 / 100;
    let c = if tick & 1 == 0 { SEND_A } else { SEND_B };
    for b in 0..240u32 {
        if b * 37 % 240 < count {
            let (x, y) = ((b % 20) as i32 * 8, BAND_H + (b / 20) as i32 * 8);
            rect_fill(f, &Rect::new(x, y, 8, 8), c);
        }
    }
    Counts {
        px: count * 64,
        ..Counts::default()
    }
}

/// The spinning triangle's corners round its middle (1/16 pixel): it
/// paints about 100 pixels at any turn (99.4 by its area).
pub const SPIN_TRI: [[i32; 2]; 3] = [[-113, -100], [147, -20], [-34, 120]];
/// The farthest corner from the middle, in whole pixels (rounded up).
pub const SPIN_REACH: i32 = 10;

/// Where spinning triangle `i` has its middle (1/16 pixel): spread over
/// the work area like the other triangle tests, never nearer than
/// [`SPIN_REACH`] to its edges.
pub fn spin_middle(i: u32) -> [i32; 2] {
    [
        (SPIN_REACH + (i.wrapping_mul(37) % 141) as i32) * SUB + SUB / 2,
        (BAND_H + SPIN_REACH + (i.wrapping_mul(23) % 81) as i32) * SUB + SUB / 2,
    ]
}

/// How far spinning triangle `i` has turned at tick `tick` of 1/60 s
/// (one turn in 128 ticks, 2.13 s: 64 calls at 30; every other one the
/// other way round, each from its own start).
pub fn spin_angle(i: u32, tick: u32) -> u16 {
    let a = (tick as u16).wrapping_mul(512);
    let a = if i & 1 == 0 { a } else { a.wrapping_neg() };
    a.wrapping_add((i as u16).wrapping_mul(9973))
}

/// `load` triangles of about 100 pixels on a cleared work area, each
/// turned round its own middle (by game time) (the corners turned with the
/// sine table, as a game would), colours in turn. Counts the pixels
/// each one really painted.
#[inline(never)]
pub fn spin(f: &mut Frame, load: u32, tick: u32) -> Counts {
    rect_fill(f, &WORK, SKY);
    let mut px = 0u32;
    for i in 0..load {
        let a = spin_angle(i, tick);
        let (s, c) = (sin(a), cos(a));
        let m = spin_middle(i);
        // Corners below 2^8, sine below 2^15: no overflow.
        let q = SPIN_TRI.map(|v| {
            [
                m[0].wrapping_add(
                    v[0].wrapping_mul(c).wrapping_sub(v[1].wrapping_mul(s)) >> SHIFT14,
                ),
                m[1].wrapping_add(
                    v[0].wrapping_mul(s).wrapping_add(v[1].wrapping_mul(c)) >> SHIFT14,
                ),
            ]
        });
        px = px.wrapping_add(fill_tri(f, &WORK, &q, TRI0 + (i & 7) as u8));
    }
    Counts {
        tris: load,
        px,
        ..Counts::default()
    }
}
