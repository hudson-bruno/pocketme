//! The pixel source against a plain reference scaler: every run, in rows
//! and in columns, both ways, both byte orders, several scales.

use board_api::{ByteOrder, Dir, PixelSource};
use console_video::{Frame, H, Palette, Source, View, W};

struct Rng(u32);

impl Rng {
    fn below(&mut self, n: u32) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 % n
    }
}

/// The reference: one panel pixel, worked out the plainest way.
fn reference(f: &Frame, p: &Palette, v: View, border: u16, x: i32, y: i32) -> u16 {
    let k = i32::from(v.scale);
    let (px, py) = (x - i32::from(v.left), y - i32::from(v.top));
    if px < 0 || py < 0 || px >= W as i32 * k || py >= H as i32 * k {
        return border;
    }
    p.get(f.0[(py / k) as usize * W + (px / k) as usize])
}

fn picture() -> (Box<Frame>, Box<Palette>) {
    let mut f = Box::new(Frame::new());
    let mut rng = Rng(77);
    for p in f.0.iter_mut() {
        *p = rng.below(256) as u8;
    }
    let mut pal = Box::new(Palette::new());
    for i in 0..=255u8 {
        // Different high and low bytes, so a missing swap shows.
        pal.set(i, u16::from(i) << 8 | u16::from(255 - i));
    }
    (f, pal)
}

/// Checks one run against the reference.
fn check(src: &Source, panel: (u16, u16), x: u16, y: u16, dir: Dir, order: ByteOrder, n: usize) {
    let mut out = vec![0xDEADu16; n];
    src.run(x, y, dir, order, &mut out);
    let (dx, dy) = match dir {
        Dir::Right => (1, 0),
        Dir::Left => (-1, 0),
        Dir::Down => (0, 1),
        Dir::Up => (0, -1),
    };
    for (i, &got) in out.iter().enumerate() {
        let (px, py) = (i32::from(x) + dx * i as i32, i32::from(y) + dy * i as i32);
        assert!(px >= 0 && py >= 0 && px < i32::from(panel.0) && py < i32::from(panel.1));
        let want = order.apply(reference(
            src.frame,
            src.palette,
            src.view,
            src.border,
            px,
            py,
        ));
        assert_eq!(
            got, want,
            "run ({x},{y}) {dir:?} {order:?} n={n}, pixel {i} at ({px},{py}), view {:?}",
            src.view
        );
    }
}

/// Panels to try: the CYD (scale 2, no border), a bigger one (scale 3)
/// and odd ones that leave a border on some sides.
const PANELS: [(u16, u16); 5] = [(320, 240), (480, 360), (200, 150), (330, 250), (321, 480)];

#[test]
fn whole_rows_and_columns_every_way() {
    let (f, pal) = picture();
    for panel in PANELS {
        let view = View::fit(panel.0, panel.1);
        let src = Source::new(&f, &pal, view, 0x1234);
        let (pw, ph) = panel;
        for order in [ByteOrder::Native, ByteOrder::Wire] {
            for y in 0..ph {
                check(&src, panel, 0, y, Dir::Right, order, pw.into());
                check(&src, panel, pw - 1, y, Dir::Left, order, pw.into());
            }
            for x in 0..pw {
                check(&src, panel, x, 0, Dir::Down, order, ph.into());
                check(&src, panel, x, ph - 1, Dir::Up, order, ph.into());
            }
        }
    }
}

#[test]
fn random_short_runs() {
    let (f, pal) = picture();
    let mut rng = Rng(4242);
    for panel in PANELS {
        let view = View::fit(panel.0, panel.1);
        let src = Source::new(&f, &pal, view, 0xF00F);
        let (pw, ph) = (u32::from(panel.0), u32::from(panel.1));
        for _ in 0..4000 {
            let dir = [Dir::Right, Dir::Left, Dir::Down, Dir::Up][rng.below(4) as usize];
            let order = [ByteOrder::Native, ByteOrder::Wire][rng.below(2) as usize];
            let (x, y) = (rng.below(pw), rng.below(ph));
            // Room left in that direction, so the run stays on the panel.
            let room = match dir {
                Dir::Right => pw - x,
                Dir::Left => x + 1,
                Dir::Down => ph - y,
                Dir::Up => y + 1,
            };
            let n = 1 + rng.below(room) as usize;
            check(&src, panel, x as u16, y as u16, dir, order, n);
        }
    }
}

#[test]
fn view_fits_and_maps_touches() {
    assert_eq!(
        View::fit(320, 240),
        View {
            scale: 2,
            left: 0,
            top: 0
        }
    );
    let v = View::fit(330, 250);
    assert_eq!((v.scale, v.left, v.top), (2, 5, 5));
    assert_eq!(v.to_picture(4, 100), None);
    assert_eq!(v.to_picture(5, 5), Some((0, 0)));
    assert_eq!(v.to_picture(324, 244), Some((159, 119)));
    assert_eq!(v.to_picture(325, 100), None);
}
