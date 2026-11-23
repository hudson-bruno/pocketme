//! The triangle filler against the slow, obvious one: the same pixels
//! for random triangles (small, large, thin, off the picture, in a
//! clip window), and two triangles sharing an edge paint every pixel
//! of their square exactly once.

use camber::poly::{Checked, Filled, GUARD, Rect, SUB, fill_tri_counted, fill_tri_slow};
use game_kit::{Frame, H, W};

/// The game's filler: game-3d's, counting rows, with checked sums.
fn fill_tri(f: &mut Frame, clip: &Rect, p: &[[i32; 2]; 3], c: u8) -> Filled {
    fill_tri_counted::<Checked>(f, clip, p, c)
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % (hi - lo + 1) as u32) as i32
    }
}

fn corner(rng: &mut Rng, k: u32) -> [i32; 2] {
    match k % 4 {
        // Inside the picture, any 1/16 pixel.
        0 => [rng.range(0, W as i32 * SUB), rng.range(0, H as i32 * SUB)],
        // On whole pixels and half pixels (edges through pixel middles).
        1 => [
            rng.range(0, W as i32) * SUB + if rng.next() & 1 == 0 { 0 } else { SUB / 2 },
            rng.range(0, H as i32) * SUB + SUB / 2,
        ],
        // Far out in the guard band.
        2 => [rng.range(-GUARD, GUARD), rng.range(-GUARD, GUARD)],
        // Near the edges.
        _ => [
            rng.range(-40 * SUB, (W as i32 + 40) * SUB),
            rng.range(-40 * SUB, (H as i32 + 40) * SUB),
        ],
    }
}

#[test]
fn the_fast_filler_paints_what_the_slow_one_paints() {
    let mut rng = Rng(0x0F11_7E57);
    let clips = [
        Rect::new(0, 0, W as i32, H as i32),
        Rect::new(10, 7, 100, 60),
        Rect::new(-20, -20, W as i32 + 40, 40),
    ];
    for n in 0..4_000u32 {
        let mut p = [
            corner(&mut rng, n),
            corner(&mut rng, n),
            corner(&mut rng, n / 4),
        ];
        // Now and then a thin one or a flat top.
        if n % 7 == 0 {
            p[2] = [p[0][0] + 1, p[1][1]];
        }
        if n % 11 == 0 {
            p[1][1] = p[0][1];
        }
        let clip = clips[(n % 3) as usize];
        let mut a = Box::new(Frame::new());
        let mut b = Box::new(Frame::new());
        let got = fill_tri(&mut a, &clip, &p, 7);
        let want = fill_tri_slow(&mut b, &clip, &p, 7);
        assert_eq!(got.pixels, want, "triangle {n}: {p:?} in {clip:?}");
        assert!(a.0 == b.0, "triangle {n}: {p:?} in {clip:?}: other pixels");
    }
}

#[test]
fn two_triangles_sharing_an_edge_paint_each_pixel_once() {
    let mut rng = Rng(0x5EA_0001);
    let full = Rect::new(0, 0, W as i32, H as i32);
    for _ in 0..500 {
        let q = [
            [rng.range(0, 60 * SUB), rng.range(0, 40 * SUB)],
            [rng.range(90 * SUB, 159 * SUB), rng.range(0, 40 * SUB)],
            [
                rng.range(90 * SUB, 159 * SUB),
                rng.range(70 * SUB, 119 * SUB),
            ],
            [rng.range(0, 60 * SUB), rng.range(70 * SUB, 119 * SUB)],
        ];
        // Each triangle paints its own number in a frame of its own, so
        // a pixel painted by both shows.
        let mut a = Box::new(Frame::new());
        let mut b = Box::new(Frame::new());
        let mut whole = Box::new(Frame::new());
        let na = fill_tri(&mut a, &full, &[q[0], q[1], q[2]], 1).pixels;
        let nb = fill_tri(&mut b, &full, &[q[0], q[2], q[3]], 1).pixels;
        let both =
            a.0.iter()
                .zip(b.0.iter())
                .filter(|(x, y)| **x == 1 && **y == 1)
                .count();
        assert_eq!(both, 0, "{q:?}: pixels painted twice");
        // Together they paint what the quad's two halves paint, with no
        // hole: compare with the slow filler's count of the same halves.
        let sa = fill_tri_slow(&mut whole, &full, &[q[0], q[1], q[2]], 1);
        let sb = fill_tri_slow(&mut whole, &full, &[q[0], q[2], q[3]], 1);
        assert_eq!(na + nb, sa + sb);
    }
}
