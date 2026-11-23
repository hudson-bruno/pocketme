//! Drawing whole objects (from KESTREL's tests): nothing is mirrored,
//! back faces are dropped, the view is never painted outside, objects
//! half off the view are cut at its edge, faces through the eye plane
//! are cut, not dropped. And each choice made while compiling: wrapping
//! sums, the lenses, back faces by face direction, the sort depth, the
//! near plane and short lists.
//!
//! Tests may use floating point; the library never does.

mod common;

use common::{CUBES, Dice, FULL, STONE, blank, flat};
use game_3d::{
    Camera, Checked, Count, Cull, Depth, Face, Filled, Lens, M8, Mat3, Model, Object, Planes, Rect,
    Renderer, Setup, Standard, Sums, View, Wrapping, deg,
};
use game_kit::{Frame, H, W};

type Std = Renderer<Standard, 48, 112>;

struct Wrap;
impl Setup for Wrap {
    type Sums = Wrapping;
}

struct Fast;
impl Setup for Fast {
    type Sums = Checked;
    const LENS: Lens = Lens::Fast;
}

struct Narrow;
impl Setup for Narrow {
    type Sums = Wrapping;
    const LENS: Lens = Lens::Narrow;
}

struct ByNormal;
impl Setup for ByNormal {
    type Sums = Checked;
    const CULL: Cull = Cull::Normal;
}

struct Farthest;
impl Setup for Farthest {
    type Sums = Checked;
    const DEPTH: Depth = Depth::Farthest;
}

struct Close;
impl Setup for Close {
    type Sums = Checked;
    const NEAR: i32 = M8 / 8;
}

/// KESTREL's choices without the whole-ball shortcut.
struct NoShortcut;
impl Setup for NoShortcut {
    type Sums = Checked;
    const INSIDE_SHORTCUT: bool = false;
}

/// KESTREL's choices without object sizes.
struct NoScale;
impl Setup for NoScale {
    type Sums = Checked;
    const SCALE: bool = false;
}

/// CAMBER's choices.
struct Camberish;
impl Setup for Camberish {
    type Sums = Checked;
    const NEAR: i32 = M8 / 4;
    const LENS: Lens = Lens::Fast;
    const CULL: Cull = Cull::Normal;
    const DEPTH: Depth = Depth::Farthest;
    const SEAM_PROOF: bool = true;
    const SCALE: bool = false;
    const INSIDE_SHORTCUT: bool = false;
}

/// A counter like CAMBER's: corners projected and filler rows too.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counts {
    verts: u32,
    projected: u32,
    front: u32,
    clipped: u32,
    tris: u32,
    rows: u32,
    pixels: u32,
    dropped: u32,
}

impl Count for Counts {
    const ROWS: bool = true;
    const ZERO: Self = Counts {
        verts: 0,
        projected: 0,
        front: 0,
        clipped: 0,
        tris: 0,
        rows: 0,
        pixels: 0,
        dropped: 0,
    };
    fn verts<S: Sums>(&mut self, n: u32) {
        self.verts += n;
    }
    fn projected<S: Sums>(&mut self) {
        self.projected += 1;
    }
    fn front<S: Sums>(&mut self) {
        self.front += 1;
    }
    fn clipped<S: Sums>(&mut self) {
        self.clipped += 1;
    }
    fn filled<S: Sums>(&mut self, done: Filled) {
        self.tris += 1;
        self.rows += done.rows;
        self.pixels += done.pixels;
    }
    fn dropped<S: Sums>(&mut self) {
        self.dropped += 1;
    }
}

/// The middle (in pixels) of each material's pixels: Some((x, y)).
fn middles(f: &Frame) -> [Option<(f64, f64)>; 4] {
    let mut out = [None; 4];
    for (m, slot) in out.iter_mut().enumerate().skip(1) {
        let (mut sx, mut sy, mut n) = (0f64, 0f64, 0f64);
        for (i, &p) in f.0.iter().enumerate() {
            if usize::from(p) == m {
                sx += (i % W) as f64;
                sy += (i / W) as f64;
                n += 1.0;
            }
        }
        if n > 0.0 {
            *slot = Some((sx / n, sy / n));
        }
    }
    out
}

fn cubes(rot: Mat3) -> Object<'static> {
    Object {
        model: &CUBES,
        pos: [0, 0, 0],
        rot,
        scale: 256,
        swap: (0, 0),
    }
}

/// The three cubes turned by `rot`, seen from 4 m behind (eye at -z,
/// looking along +z).
fn look_at_cubes(rot: Mat3) -> [Option<(f64, f64)>; 4] {
    let view = View::new(FULL, 96);
    let cam = Camera {
        pos: [0, 0, -4 * M8],
        rot: Mat3::IDENTITY,
    };
    let mut r = Box::new(Std::new());
    let mut f = blank();
    assert!(r.draw(&mut f, &view, &cam, &flat(), &cubes(rot), 40 * M8));
    middles(&f)
}

#[test]
fn nothing_is_mirrored_and_turnings_go_the_documented_way() {
    let (cx, cy) = (80.0, 60.0);
    // Unturned: +x is right of the middle, +y above it (rows grow down).
    let m = look_at_cubes(Mat3::IDENTITY);
    let (x, _) = m[1].expect("the +x cube is seen");
    assert!(x > cx + 10.0, "+x drawn at x {x}");
    let (_, y) = m[2].expect("the +y cube is seen");
    assert!(y < cy - 10.0, "+y drawn at y {y}");
    // Yaw +90 degrees turns the nose (+z) towards +x: the +z cube moves
    // to the right.
    let m = look_at_cubes(Mat3::rotation(deg(90), 0, 0));
    let (x, _) = m[3].expect("the turned +z cube is seen");
    assert!(x > cx + 10.0, "yaw: +z cube at x {x}");
    // Pitch +30 degrees lifts the nose towards +y.
    let m = look_at_cubes(Mat3::rotation(0, deg(30), 0));
    let (_, y) = m[3].expect("+z cube");
    assert!(y < cy - 3.0, "pitch: +z cube at y {y}");
    // Roll +45 degrees banks right: the top (+y) leans to the right.
    let m = look_at_cubes(Mat3::rotation(0, 0, deg(45)));
    let (x, y) = m[2].expect("+y cube");
    assert!(x > cx + 5.0 && y < cy - 5.0, "roll: +y cube at ({x}, {y})");
}

/// A random eye round and inside the model, looking every way, and a
/// random object (model, turn, size).
fn random_scene(d: &mut Dice, n: usize) -> (Camera, Object<'static>) {
    let cam = Camera {
        pos: [d.range(-600, 600), d.range(-600, 600), d.range(-900, 600)],
        rot: Mat3::rotation(d.next() as u16, d.next() as u16, d.next() as u16).transposed(),
    };
    let model: &'static Model = if n.is_multiple_of(2) { &CUBES } else { &STONE };
    let o = Object {
        model,
        pos: [0, 0, 0],
        rot: Mat3::rotation(d.next() as u16, d.next() as u16, d.next() as u16),
        scale: d.range(64, 900),
        swap: (0, 0),
    };
    (cam, o)
}

fn painted_outside(f: &Frame, a: &Rect) -> Option<(i32, i32)> {
    for (i, &p) in f.0.iter().enumerate() {
        let (x, y) = ((i % W) as i32, (i / W) as i32);
        if !(x >= a.x0 && x < a.x1 && y >= a.y0 && y < a.y1) && p != 0 {
            return Some((x, y));
        }
    }
    None
}

#[test]
fn back_faces_are_dropped_and_the_view_is_never_painted_outside() {
    // A cube seen face on shows one face (two triangles), seen from a
    // corner at most three faces.
    let view = View::new(Rect::new(20, 20, 120, 80), 70);
    let sh = flat();
    let mut r = Box::new(Std::new());
    let face_on = Camera {
        pos: [256, 0, -3 * M8],
        rot: Mat3::IDENTITY,
    };
    let mut f = blank();
    r.begin();
    r.draw(
        &mut f,
        &view,
        &face_on,
        &sh,
        &cubes(Mat3::IDENTITY),
        40 * M8,
    );
    // Each cube shows at most three of its six sides (six triangles).
    assert!(r.stats.front <= 3 * 6, "front faces {}", r.stats.front);
    assert!(r.stats.front >= 2, "front faces {}", r.stats.front);
    assert!(
        r.stats.front * 2 <= r.stats.faces,
        "about half the faces are back faces"
    );
    let mut d = Dice(0xA11);
    for n in 0..3000 {
        let (cam, o) = random_scene(&mut d, n);
        let mut f = blank();
        r.begin();
        r.draw(&mut f, &view, &cam, &sh, &o, 200 * M8);
        assert_eq!(r.stats.dropped, 0);
        assert_eq!(painted_outside(&f, &view.area), None);
    }
}

#[test]
fn an_object_half_off_the_view_is_cut_at_its_edge() {
    // The same object drawn into a small view and a big one: inside the
    // small view the pixels are the same.
    let small = View::new(Rect::new(40, 30, 80, 60), 60);
    let big = View {
        area: Rect::new(-40, -30, 240, 180),
        ..small
    };
    let cam = Camera {
        pos: [0, 0, -3 * M8],
        rot: Mat3::IDENTITY,
    };
    let mut r = Box::new(Std::new());
    for x in [-700, -400, 0, 400, 700] {
        let o = Object {
            model: &STONE,
            pos: [x, 100, 0],
            rot: Mat3::rotation(deg(20), deg(10), 0),
            scale: 300,
            swap: (0, 0),
        };
        let (mut a, mut b) = (blank(), blank());
        r.draw(&mut a, &small, &cam, &flat(), &o, 40 * M8);
        r.draw(&mut b, &big, &cam, &flat(), &o, 40 * M8);
        let s = small.area;
        for y in s.y0..s.y1 {
            for xx in s.x0..s.x1 {
                let i = y as usize * W + xx as usize;
                assert_eq!(a.0[i], b.0[i], "x {x}: pixel ({xx},{y})");
            }
        }
    }
}

static FLOOR_V: [[i16; 3]; 4] = [
    [-4000, 0, -4000],
    [4000, 0, -4000],
    [4000, 0, 4000],
    [-4000, 0, 4000],
];
static FLOOR_F: [Face; 2] = [
    Face {
        v: [0, 2, 1],
        mat: 1,
    },
    Face {
        v: [0, 3, 2],
        mat: 1,
    },
];
static FLOOR_N: [[i16; 3]; 2] = [[0, 16384, 0], [0, 16384, 0]];
static FLOOR: Model = Model {
    verts: &FLOOR_V,
    faces: &FLOOR_F,
    normals: &FLOOR_N,
    radius: 5700,
};

#[test]
fn a_face_through_the_eye_plane_is_cut_not_dropped() {
    // A big flat floor under the eye reaching behind it: it must be
    // drawn (cut at the near plane), filling the lower part of the view.
    let view = View::new(FULL, 80);
    let cam = Camera {
        pos: [0, 200, 0],
        rot: Mat3::IDENTITY,
    };
    let mut r = Box::new(Std::new());
    let mut f = blank();
    let o = Object {
        model: &FLOOR,
        pos: [0, 0, 0],
        rot: Mat3::IDENTITY,
        scale: 256,
        swap: (0, 0),
    };
    r.begin();
    assert!(r.draw(&mut f, &view, &cam, &flat(), &o, 100 * M8));
    assert!(r.stats.clipped > 0);
    // The bottom row is floor, the top row is not.
    assert!(f.0[(H - 1) * W..].iter().all(|&p| p == 1));
    assert!(f.0[..W].iter().all(|&p| p == 0));
}

/// Draws the same random scenes with two setups and counts the scenes
/// whose pictures differ.
fn differing<A: Setup, B: Setup>(seed: u64, scenes: usize, far_eye: bool) -> usize {
    let view = View::new(Rect::new(8, 6, 144, 108), 90);
    let sh = common::lit();
    let mut ra = Box::new(Renderer::<A, 48, 112>::new());
    let mut rb = Box::new(Renderer::<B, 48, 112>::new());
    let mut d = Dice(seed);
    let mut differ = 0;
    for n in 0..scenes {
        let (mut cam, o) = random_scene(&mut d, n);
        if far_eye {
            // The object 3 to 10 m in front of the eye.
            cam = Camera {
                pos: [0, 0, 0],
                rot: Mat3::IDENTITY,
            };
            let o2 = Object {
                pos: [
                    d.range(-900, 900),
                    d.range(-700, 700),
                    d.range(3 * M8, 10 * M8),
                ],
                ..o
            };
            let (mut a, mut b) = (blank(), blank());
            ra.begin();
            rb.begin();
            let da = ra.draw(&mut a, &view, &cam, &sh, &o2, 200 * M8);
            let db = rb.draw(&mut b, &view, &cam, &sh, &o2, 200 * M8);
            assert_eq!(da, db);
            if a.0 != b.0 {
                differ += 1;
            }
            continue;
        }
        let (mut a, mut b) = (blank(), blank());
        ra.begin();
        rb.begin();
        let da = ra.draw(&mut a, &view, &cam, &sh, &o, 200 * M8);
        let db = rb.draw(&mut b, &view, &cam, &sh, &o, 200 * M8);
        assert_eq!(da, db);
        if a.0 != b.0 {
            differ += 1;
        }
        assert_eq!(painted_outside(&a, &view.area), None);
        assert_eq!(painted_outside(&b, &view.area), None);
    }
    differ
}

#[test]
fn wrapping_sums_draw_the_same_pictures_as_checked_ones() {
    assert_eq!(differing::<Standard, Wrap>(0x3A9, 2000, false), 0);
    assert_eq!(differing::<Standard, Wrap>(0x3AA, 1000, true), 0);
}

#[test]
fn the_fast_and_narrow_lenses_draw_what_the_wide_one_draws() {
    // Inside their ranges the 32-bit lenses give the very same pixels.
    assert_eq!(differing::<Standard, Fast>(0x1E5, 2000, false), 0);
    assert_eq!(differing::<Standard, Narrow>(0x1E6, 1000, true), 0);
}

#[test]
fn back_faces_by_face_direction_match_back_faces_by_corners() {
    // The two rules can differ only for faces seen almost edge on
    // (rounding of the stored directions); on objects in front of the
    // eye they agree nearly always.
    let differ = differing::<Standard, ByNormal>(0xB0C, 1000, true);
    assert!(differ <= 10, "{differ} of 1000 pictures differ");
}

#[test]
fn faces_sorted_by_their_farthest_corner_still_draw_a_whole_model() {
    let differ = differing::<Standard, Farthest>(0xFA2, 500, true);
    // A single convex part draws the same in any face order.
    assert!(differ < 500, "every picture differs");
    let view = View::new(FULL, 96);
    let cam = Camera {
        pos: [0, 0, -4 * M8],
        rot: Mat3::IDENTITY,
    };
    let mut r = Box::new(Renderer::<Farthest, 48, 112>::new());
    let mut f = blank();
    r.draw(
        &mut f,
        &view,
        &cam,
        &flat(),
        &cubes(Mat3::IDENTITY),
        40 * M8,
    );
    let m = middles(&f);
    assert!(m[1].is_some() && m[2].is_some() && m[3].is_some());
}

#[test]
fn a_nearer_near_plane_cuts_less() {
    // A triangle 0.2 m in front of the eye: cut at KESTREL's near plane
    // (0.5 m), whole at 1/8 m.
    static TRI_V: [[i16; 3]; 3] = [[-40, -30, 0], [40, -30, 0], [0, 40, 0]];
    static TRI_F: [Face; 1] = [Face {
        v: [0, 2, 1],
        mat: 2,
    }];
    static TRI_N: [[i16; 3]; 1] = [[0, 0, -16384]];
    static TRI: Model = Model {
        verts: &TRI_V,
        faces: &TRI_F,
        normals: &TRI_N,
        radius: 51,
    };
    let view = View::new(FULL, 20);
    let cam = Camera {
        pos: [0, 0, 0],
        rot: Mat3::IDENTITY,
    };
    let o = Object {
        model: &TRI,
        pos: [0, 0, 51],
        rot: Mat3::IDENTITY,
        scale: 256,
        swap: (0, 0),
    };
    let mut std = Box::new(Std::new());
    let mut close = Box::new(Renderer::<Close, 48, 112>::new());
    let (mut a, mut b) = (blank(), blank());
    assert!(!std.draw(&mut a, &view, &cam, &flat(), &o, 10 * M8));
    assert!(close.draw(&mut b, &view, &cam, &flat(), &o, 10 * M8));
    assert_eq!(close.stats.clipped, 0);
    assert!(b.0.iter().filter(|&&p| p == 2).count() > 100);
}

#[test]
fn objects_beyond_far_are_left_out() {
    let view = View::new(FULL, 96);
    let cam = Camera {
        pos: [0, 0, -20 * M8],
        rot: Mat3::IDENTITY,
    };
    let mut r = Box::new(Std::new());
    let mut f = blank();
    assert!(!r.draw(
        &mut f,
        &view,
        &cam,
        &flat(),
        &cubes(Mat3::IDENTITY),
        15 * M8
    ));
    assert!(f.0.iter().all(|&p| p == 0));
    assert!(r.draw(
        &mut f,
        &view,
        &cam,
        &flat(),
        &cubes(Mat3::IDENTITY),
        25 * M8
    ));
}

#[test]
fn a_short_triangle_list_drops_and_counts_what_does_not_fit() {
    let view = View::new(FULL, 96);
    let cam = Camera {
        pos: [0, 0, -4 * M8],
        rot: Mat3::IDENTITY,
    };
    let mut r = Box::new(Renderer::<Standard, 48, 4>::new());
    let mut f = blank();
    r.begin();
    r.draw(
        &mut f,
        &view,
        &cam,
        &flat(),
        &cubes(Mat3::IDENTITY),
        40 * M8,
    );
    assert_eq!(r.stats.tris, 4);
    assert!(r.stats.dropped > 0);
    // And a short corner list leaves out the faces of the corners it has
    // no room for.
    let mut r = Box::new(Renderer::<Standard, 8, 112>::new());
    let mut g = blank();
    r.draw(
        &mut g,
        &view,
        &cam,
        &flat(),
        &cubes(Mat3::IDENTITY),
        40 * M8,
    );
    let m = middles(&g);
    assert!(m[1].is_some() && m[2].is_none() && m[3].is_none());
}

#[test]
fn a_material_swap_changes_only_that_material() {
    let view = View::new(FULL, 96);
    let cam = Camera {
        pos: [0, 0, -4 * M8],
        rot: Mat3::IDENTITY,
    };
    let mut r = Box::new(Std::new());
    let (mut a, mut b) = (blank(), blank());
    r.draw(
        &mut a,
        &view,
        &cam,
        &flat(),
        &cubes(Mat3::IDENTITY),
        40 * M8,
    );
    let o = Object {
        swap: (2, 6),
        ..cubes(Mat3::IDENTITY)
    };
    r.draw(&mut b, &view, &cam, &flat(), &o, 40 * M8);
    for (x, y) in a.0.iter().zip(b.0.iter()) {
        assert_eq!(*y, if *x == 2 { 6 } else { *x });
    }
    assert!(b.0.contains(&6));
}

/// Draws random scenes with two setups, every object at its own size
/// (256), and counts the scenes whose pictures differ.
fn differing_unscaled<A: Setup, B: Setup>(seed: u64, scenes: usize) -> usize {
    let view = View::new(Rect::new(8, 6, 144, 108), 90);
    let sh = common::lit();
    let mut ra = Box::new(Renderer::<A, 48, 112>::new());
    let mut rb = Box::new(Renderer::<B, 48, 112>::new());
    let mut d = Dice(seed);
    let mut differ = 0;
    for n in 0..scenes {
        let (cam, o) = random_scene(&mut d, n);
        let o = Object { scale: 256, ..o };
        let (mut a, mut b) = (blank(), blank());
        ra.draw(&mut a, &view, &cam, &sh, &o, 200 * M8);
        rb.draw(&mut b, &view, &cam, &sh, &o, 200 * M8);
        if a.0 != b.0 {
            differ += 1;
        }
    }
    differ
}

#[test]
fn leaving_out_the_whole_ball_shortcut_draws_the_same_pictures() {
    assert_eq!(differing::<Standard, NoShortcut>(0x5C1, 2000, false), 0);
    assert_eq!(differing::<Standard, NoShortcut>(0x5C2, 1000, true), 0);
}

#[test]
fn without_sizes_objects_are_drawn_at_their_own_size() {
    assert_eq!(differing_unscaled::<Standard, NoScale>(0x5CA, 2000), 0);
    // And the size is not read.
    let view = View::new(FULL, 96);
    let cam = Camera {
        pos: [0, 0, -4 * M8],
        rot: Mat3::IDENTITY,
    };
    let mut r = Box::new(Renderer::<NoScale, 48, 112>::new());
    let (mut a, mut b) = (blank(), blank());
    let o = cubes(Mat3::IDENTITY);
    r.draw(
        &mut a,
        &view,
        &cam,
        &flat(),
        &Object { scale: 256, ..o },
        40 * M8,
    );
    r.draw(
        &mut b,
        &view,
        &cam,
        &flat(),
        &Object { scale: 900, ..o },
        40 * M8,
    );
    assert!(a.0 == b.0);
}

#[test]
fn a_counter_of_its_own_counts_what_the_usual_one_counts_and_more() {
    let view = View::new(Rect::new(8, 6, 144, 108), 90);
    let sh = common::lit();
    let mut usual = Box::new(Renderer::<Camberish, 48, 112, 16>::new());
    let mut own = Box::new(Renderer::<Camberish, 48, 112, 16, Counts>::new());
    let mut none = Box::new(Renderer::<Camberish, 48, 112, 16, ()>::new());
    let mut d = Dice(0xC0C0);
    let (mut rows, mut cut) = (0u64, 0u32);
    for n in 0..2000 {
        let (cam, o) = random_scene(&mut d, n);
        let (mut a, mut b, mut c) = (blank(), blank(), blank());
        usual.begin();
        own.begin();
        let da = usual.draw(&mut a, &view, &cam, &sh, &o, 200 * M8);
        let db = own.draw(&mut b, &view, &cam, &sh, &o, 200 * M8);
        let dc = none.draw(&mut c, &view, &cam, &sh, &o, 200 * M8);
        assert_eq!((da, da), (db, dc));
        assert!(a.0 == b.0 && a.0 == c.0, "scene {n}");
        let (u, k) = (usual.stats, own.stats);
        assert_eq!(
            (u.verts, u.front, u.clipped, u.tris, u.pixels, u.dropped),
            (k.verts, k.front, k.clipped, k.tris, k.pixels, k.dropped),
            "scene {n}"
        );
        // Corners inside are projected once, cut corners once more.
        assert!(k.projected <= k.verts + 16 * k.clipped, "scene {n}: {k:?}");
        // A triangle may lie wholly outside the view's rows (in the
        // guard band); one that paints visits rows.
        if k.pixels > 0 {
            assert!(k.rows > 0 && k.projected > 0, "scene {n}: {k:?}");
        }
        rows += u64::from(k.rows);
        cut += k.clipped;
    }
    assert!(rows > 10_000 && cut > 100, "rows {rows}, cut {cut}");
}

/// A view that never changes, and its planes worked out while compiling.
const FIXED_VIEW: View = View::new(Rect::new(8, 6, 144, 108), 90);
const FIXED_PLANES: Planes = FIXED_VIEW.clip_planes_at(Camberish::NEAR);

#[test]
fn drawing_in_given_planes_is_drawing_in_the_views_own() {
    assert_eq!(FIXED_PLANES, FIXED_VIEW.clip_planes_at(Camberish::NEAR));
    let other = View::new(Rect::new(30, 20, 90, 70), 60);
    let sh = common::lit();
    let mut a = Box::new(Renderer::<Camberish, 48, 112, 16, Counts>::new());
    let mut b = Box::new(Renderer::<Camberish, 48, 112, 16, Counts>::new());
    let mut d = Dice(0xD1A);
    for n in 0..1500 {
        let (cam, o) = random_scene(&mut d, n);
        let (mut fa, mut fb) = (blank(), blank());
        a.begin();
        b.begin();
        let da = a.draw(&mut fa, &FIXED_VIEW, &cam, &sh, &o, 200 * M8);
        let db = b.draw_in(&mut fb, &FIXED_VIEW, &FIXED_PLANES, &cam, &sh, &o, 200 * M8);
        assert_eq!(da, db);
        assert!(fa.0 == fb.0, "scene {n}");
        assert_eq!(a.stats, b.stats, "scene {n}");
        // After planes were given, the renderer works out another
        // view's planes again.
        if n % 7 == 0 {
            let (mut ga, mut gb) = (blank(), blank());
            let mut c = Box::new(Renderer::<Camberish, 48, 112, 16, Counts>::new());
            c.draw(&mut ga, &other, &cam, &sh, &o, 200 * M8);
            b.draw(&mut gb, &other, &cam, &sh, &o, 200 * M8);
            assert!(ga.0 == gb.0, "scene {n}, other view");
        }
    }
}
