//! KESTREL's own 3D checks. The 3D code itself (filler, clipping,
//! renderer, maths) is the shared library `game-3d` and is tested there
//! (`crates/game-3d/tests/`); here only what is KESTREL's:
//!
//! * every model is closed, every face looks outwards, and each fits
//!   the renderer's lists ([`MAX_VERTS`], [`MAX_TRIS`]);
//! * KESTREL's models, drawn by KESTREL's renderer from every side,
//!   never paint outside the view and never overflow the triangle list;
//! * an object half off the view is cut at its edge.
//!
//! Tests may use floating point; the game never does.

use game_kit::{Frame, W};
use kestrel::fixed::{Mat3, ONE14, deg};
use kestrel::models;
use kestrel::poly::{Camera, M8, MAX_TRIS, MAX_VERTS, Object, Rect, Renderer, Shading, View};

struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % (hi - lo + 1) as u64) as i32
    }
}

fn blank() -> Box<Frame> {
    Box::new(Frame::new())
}

/// Flat colours: palette number = material (no light, no fog).
fn flat() -> Shading {
    Shading {
        mats: 8,
        shades: 1,
        light: [0, 0, ONE14],
        glow: 0xFF,
        fog_from: 0,
        fog_to: 0,
        fog_shades: 0,
        clear: 0,
        dark_side: 0,
    }
}

// ---- Drawing whole objects -------------------------------------------------------------

#[test]
fn kestrels_models_never_paint_outside_the_view_or_overflow() {
    let view = View::new(Rect::new(20, 20, 120, 80), 70);
    let sh = flat();
    let mut r = Box::new(Renderer::new());
    // The eye everywhere round and inside the model, looking every way:
    // nothing is painted outside the view, nothing is left out.
    let mut d = Dice(0xA11);
    for n in 0..3000 {
        let cam = Camera {
            pos: [d.range(-600, 600), d.range(-600, 600), d.range(-900, 600)],
            rot: Mat3::rotation(d.next() as u16, d.next() as u16, d.next() as u16).transposed(),
        };
        let o = Object {
            model: models::all()[n % 10].1,
            pos: [0, 0, 0],
            rot: Mat3::rotation(d.next() as u16, d.next() as u16, d.next() as u16),
            scale: d.range(64, 900),
            swap: (0, 0),
        };
        let mut f = blank();
        r.begin();
        r.draw(&mut f, &view, &cam, &sh, &o, 200 * M8);
        assert_eq!(r.stats.dropped, 0);
        let a = view.area;
        for (i, &p) in f.0.iter().enumerate() {
            let (x, y) = ((i % W) as i32, (i / W) as i32);
            if !(x >= a.x0 && x < a.x1 && y >= a.y0 && y < a.y1) {
                assert_eq!(p, 0, "painted outside the view at ({x},{y})");
            }
        }
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
    let mut r = Box::new(Renderer::new());
    for x in [-700, -400, 0, 400, 700] {
        let o = Object {
            model: &models::ROCK_A,
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

// ---- Models -----------------------------------------------------------------------------

#[test]
fn every_model_is_closed_and_every_face_looks_outwards() {
    for (name, m) in models::all() {
        assert!(
            m.verts.len() <= MAX_VERTS,
            "{name}: {} corners",
            m.verts.len()
        );
        assert!(m.faces.len() <= MAX_TRIS, "{name}: {} faces", m.faces.len());
        assert_eq!(m.faces.len(), m.normals.len());
        // Closed and wound the same way: every edge a->b once, and
        // b->a once (from the face next to it).
        let mut edges = std::collections::HashMap::new();
        for f in m.faces {
            for k in 0..3 {
                *edges.entry((f.v[k], f.v[(k + 1) % 3])).or_insert(0) += 1;
            }
        }
        for (&(a, b), &n) in &edges {
            assert_eq!(n, 1, "{name}: edge {a}->{b} used {n} times");
            assert_eq!(
                edges.get(&(b, a)),
                Some(&1),
                "{name}: edge {a}->{b} has no partner"
            );
        }
        // Outwards: the volume the faces enclose is positive.
        let mut vol = 0i64;
        for f in m.faces {
            let [a, b, c] = f.v.map(|i| m.verts[usize::from(i)].map(i64::from));
            vol += a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]);
        }
        assert!(vol > 0, "{name}: faces look inwards");
        // Every normal is one long.
        for n in m.normals {
            let len = (n.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>()).sqrt();
            assert!((len - 16384.0).abs() < 8.0, "{name}: normal {n:?}");
        }
        // The radius holds every corner.
        for v in m.verts {
            let d = v.iter().map(|&c| i64::from(c) * i64::from(c)).sum::<i64>();
            assert!(d <= i64::from(m.radius) * i64::from(m.radius));
        }
    }
}
