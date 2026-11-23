//! The second reader's checks (core-check round): long random runs that
//! look for a panel left stale, on every way a board may walk the
//! pixels, in both byte orders, and on a panel with a border. Seeded, so
//! every run is the same.

use board_api::fake::{FakeScreen, Walk};
use board_api::{ByteOrder, Dir, PixelSource, Rect, Screen, SendReport};
use console_video::{Frame, H, MAX_AREAS, Video, View, W, text_width};

/// A small, fixed random number maker (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// One random change to the picture, the palette, or the border.
/// Returns the new border colour when it set one.
fn change(video: &mut Video, rng: &mut Rng) -> Option<u16> {
    match rng.below(100) {
        // Single pixels, often far apart (many small areas).
        0..=39 => {
            for _ in 0..1 + rng.below(40) {
                let (x, y) = (rng.below(W as u64) as u16, rng.below(H as u64) as u16);
                video.frame_mut().set(x, y, rng.below(256) as u8);
            }
        }
        // Rectangles, some sticking out of the picture.
        40..=79 => {
            let r = Rect::new(
                rng.below(W as u64 + 10) as u16,
                rng.below(H as u64 + 10) as u16,
                1 + rng.below(70) as u16,
                1 + rng.below(50) as u16,
            );
            video.frame_mut().fill_rect(r, rng.below(256) as u8);
        }
        // A word-aligned run of 4 pixels that changes only one byte.
        80..=87 => {
            let y = rng.below(H as u64) as u16;
            let x = (rng.below(W as u64 / 4) * 4 + rng.below(4)) as u16;
            let c = video.frame().get(x, y).wrapping_add(1);
            video.frame_mut().set(x, y, c);
        }
        88..=91 => {
            let i = rng.below(256) as u8;
            video.palette_mut().set(i, rng.next() as u16);
        }
        92..=94 => {
            let b = rng.next() as u16;
            video.set_border(b);
            return Some(b);
        }
        95..=96 => video.force_full(),
        97..=98 => video.frame_mut().clear(rng.below(256) as u8),
        // Nothing changes this frame.
        _ => {}
    }
    None
}

/// What the panel must show now: the picture at the view's scale, the
/// border around it.
fn expected(video: &Video, view: &View, x: u16, y: u16, border: u16) -> u16 {
    match view.to_picture(x, y) {
        Some((px, py)) => video.palette().get(video.frame().get(px, py)),
        None => border,
    }
}

#[test]
fn random_runs_never_leave_the_fake_panel_stale() {
    let configs = [
        (Walk::Rows, ByteOrder::Native, 1u64),
        (Walk::RowsLeft, ByteOrder::Wire, 2),
        (Walk::Columns, ByteOrder::Wire, 3),
        (Walk::ColumnsUp, ByteOrder::Native, 4),
        (Walk::Indexed, ByteOrder::Wire, 5),
    ];
    let view = View::fit(320, 240);
    for (walk, order, seed) in configs {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ seed);
        let mut video = Box::new(Video::new());
        let mut screen = Box::new(FakeScreen::new());
        screen.walk = walk;
        screen.order = order;
        for round in 0..400 {
            for _ in 0..1 + rng.below(3) {
                let _ = change(&mut video, &mut rng);
            }
            let r = video.present(&mut *screen);
            assert!(usize::from(r.areas) <= MAX_AREAS);
            for y in 0..240 {
                for x in 0..320 {
                    let want = expected(&video, &view, x, y, 0);
                    assert_eq!(
                        screen.pixel(x, y),
                        want,
                        "{walk:?} {order:?} round {round}: stale panel pixel ({x},{y})"
                    );
                }
            }
        }
    }
}

/// A panel of any size that keeps what it was sent (stale outside the
/// areas), walks columns bottom to top in wire order, and checks every
/// area is inside it. Used for a panel with a border around the picture.
struct OddScreen<const PW: u16, const PH: u16> {
    image: Vec<u16>,
    border_seen: bool,
}

impl<const PW: u16, const PH: u16> Screen for OddScreen<PW, PH> {
    const PANEL_W: u16 = PW;
    const PANEL_H: u16 = PH;

    fn present<S: PixelSource + ?Sized>(&mut self, src: &S, areas: &[Rect]) -> SendReport {
        assert!(areas.len() <= MAX_AREAS, "{} areas", areas.len());
        let panel = Rect::new(0, 0, PW, PH);
        let mut pixels = 0;
        let mut buf = vec![0u16; usize::from(PH)];
        for &a in areas {
            assert_eq!(a.intersect(&panel), a, "area {a:?} outside the panel");
            for x in a.x..a.right() {
                let run = &mut buf[..usize::from(a.h)];
                src.run(x, a.bottom() - 1, Dir::Up, ByteOrder::Wire, run);
                for (i, &c) in run.iter().enumerate() {
                    let y = a.bottom() - 1 - i as u16;
                    self.image[usize::from(y) * usize::from(PW) + usize::from(x)] = u16::from_be(c);
                    if x == 0 || y == 0 {
                        self.border_seen = true;
                    }
                }
            }
            pixels += a.area();
        }
        SendReport {
            areas: areas.len() as u16,
            pixels,
            send_us: 0,
            wait_us: 0,
        }
    }
}

#[test]
fn random_runs_never_leave_a_bordered_panel_stale() {
    // 336x250: scale 2, a border of 8 left and right, 5 top and bottom.
    const PW: u16 = 336;
    const PH: u16 = 250;
    let view = View::fit(PW, PH);
    assert_eq!((view.scale, view.left, view.top), (2, 8, 5));
    let mut screen = OddScreen::<PW, PH> {
        image: vec![0xDEAD; usize::from(PW) * usize::from(PH)],
        border_seen: false,
    };
    let mut video = Box::new(Video::new());
    let mut rng = Rng(42);
    let mut border = 0u16;
    for round in 0..300 {
        for _ in 0..1 + rng.below(3) {
            if let Some(b) = change(&mut video, &mut rng) {
                border = b;
            }
        }
        video.present(&mut screen);
        for y in 0..PH {
            for x in 0..PW {
                let want = expected(&video, &view, x, y, border);
                let got = screen.image[usize::from(y) * usize::from(PW) + usize::from(x)];
                assert_eq!(got, want, "round {round}: stale panel pixel ({x},{y})");
            }
        }
    }
    assert!(screen.border_seen);
}

/// Fault (found in this round): the width of a very long text was summed
/// in a u16 that overflowed: a panic in debug builds, a wrong width in
/// release builds. Now it stops at the biggest value.
#[test]
fn a_very_long_text_has_a_width_and_draws() {
    let long = "W".repeat(20_000);
    assert_eq!(text_width(&long), u16::MAX - 1);
    let mut f = Box::new(Frame::new());
    f.text(-30_000, 10, &long, 1);
    f.text(150, 10, &long, 1);
}
