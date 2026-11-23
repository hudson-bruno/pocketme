//! Finding changed areas: hand-made cases, and frames from a fixed seed
//! checked against the design page's own code and against the simple
//! rule "every changed pixel is sent, and the panel ends up showing the
//! frame".

use board_api::Rect;
use board_api::fake::{FakeBoard, Walk};
use console_video::{Areas, Frame, H, MAX_AREAS, Palette, Video, W, diff};

/// xorshift32: the same numbers as tests/data/sim_areas.mjs.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
}

fn areas_of(a: &Frame, b: &Frame) -> Areas {
    let mut out = Areas::new();
    diff(a, b, &mut out);
    out
}

/// The rule every result must keep: all changed pixels covered, no two
/// rectangles overlapping, all inside the picture, at most MAX_AREAS.
fn check_rule(a: &Frame, b: &Frame, areas: &Areas) {
    let rects = areas.as_slice();
    assert!(rects.len() <= MAX_AREAS);
    for (i, r) in rects.iter().enumerate() {
        assert!(!r.is_empty());
        assert!(usize::from(r.right()) <= W && usize::from(r.bottom()) <= H);
        for s in &rects[i + 1..] {
            assert!(r.intersect(s).is_empty(), "{r:?} overlaps {s:?}");
        }
    }
    for y in 0..H as u16 {
        for x in 0..W as u16 {
            if a.get(x, y) != b.get(x, y) {
                assert!(
                    rects.iter().any(|r| r.contains(x, y)),
                    "changed pixel ({x}, {y}) not sent; areas {rects:?}"
                );
            }
        }
    }
}

fn boxed_frame() -> Box<Frame> {
    Box::new(Frame::new())
}

#[test]
fn nothing_changed() {
    let (a, b) = (boxed_frame(), boxed_frame());
    assert!(areas_of(&a, &b).is_empty());
}

#[test]
fn one_pixel() {
    let a = boxed_frame();
    for (x, y) in [(0, 0), (3, 7), (4, 7), (159, 119), (80, 60)] {
        let mut b = boxed_frame();
        b.set(x, y, 9);
        assert_eq!(areas_of(&a, &b).as_slice(), &[Rect::new(x, y, 1, 1)]);
    }
}

#[test]
fn two_far_corners() {
    let a = boxed_frame();
    let mut b = boxed_frame();
    b.set(0, 0, 1);
    b.set(159, 119, 1);
    assert_eq!(
        areas_of(&a, &b).as_slice(),
        &[Rect::new(0, 0, 1, 1), Rect::new(159, 119, 1, 1)]
    );
    // Two corners on the same row: one span from one to the other.
    let mut c = boxed_frame();
    c.set(0, 5, 1);
    c.set(159, 5, 1);
    assert_eq!(areas_of(&a, &c).as_slice(), &[Rect::new(0, 5, 160, 1)]);
}

#[test]
fn scroll_and_everything() {
    // Every row different from the one above it: scrolling by one row
    // changes every pixel of every row.
    let mut a = boxed_frame();
    for y in 0..H {
        a.row_mut(y).fill(y as u8);
    }
    let mut b = a.clone();
    b.0.copy_within(W.., 0);
    b.row_mut(H - 1).fill(200);
    assert_eq!(areas_of(&a, &b).as_slice(), &[Rect::new(0, 0, 160, 120)]);

    let mut c = a.clone();
    c.clear(250);
    assert_eq!(areas_of(&a, &c).as_slice(), &[Rect::new(0, 0, 160, 120)]);
}

#[test]
fn small_sprite_moving_one_pixel() {
    let mut a = boxed_frame();
    a.rect(20, 30, 16, 16, 5);
    let mut b = boxed_frame();
    b.rect(21, 30, 16, 16, 5);
    // Only the left column (now empty) and the new right column change.
    let got = areas_of(&a, &b);
    check_rule(&a, &b, &got);
    assert_eq!(got.as_slice(), &[Rect::new(20, 30, 17, 16)]);
}

#[test]
fn more_areas_than_the_maximum() {
    // 30 short marks on rows 4 apart: 30 separate rectangles before
    // joining, MAX_AREAS after.
    let a = boxed_frame();
    let mut b = boxed_frame();
    for i in 0..30 {
        b.set(10 + i * 4, i * 4, 1);
    }
    let got = areas_of(&a, &b);
    assert_eq!(got.len(), MAX_AREAS);
    check_rule(&a, &b, &got);
    // Joining wastes as little as it can: the first joins (all equal
    // waste) happen at the top, as on the design page.
    assert!(got.pixels() < W as u32 * H as u32 / 2);
}

#[test]
fn diagonal_is_cut_into_squares() {
    // The joining rule stops growing a rectangle when it gets too empty:
    // a diagonal becomes a staircase of squares, not one big box.
    let a = boxed_frame();
    let mut b = boxed_frame();
    for i in 0..60 {
        b.set(10 + i, 10 + i, 1);
    }
    let got = areas_of(&a, &b);
    check_rule(&a, &b, &got);
    assert!(got.len() > 1);
    assert!(got.pixels() < 60 * 60 / 2, "{:?}", got.as_slice());
}

/// One step of the seeded walk; the same steps as sim_areas.mjs.
fn step(f: &mut Frame, rng: &mut Rng) {
    let r = |rng: &mut Rng, n| rng.below(n) as i32;
    match rng.below(6) {
        0 => {
            for _ in 0..1 + rng.below(20) {
                let (x, y) = (r(rng, 180) - 10, r(rng, 140) - 10);
                let (w, h) = (1 + r(rng, 40), 1 + r(rng, 30));
                let c = rng.below(256) as u8;
                f.rect(x as i16, y as i16, w as u16, h as u16, c);
            }
        }
        1 => {
            for _ in 0..1 + rng.below(60) {
                let (x, y) = (r(rng, W as u32), r(rng, H as u32));
                f.pixel(x as i16, y as i16, rng.below(256) as u8);
            }
        }
        2 => {
            let (x0, y0) = (r(rng, W as u32), r(rng, H as u32));
            let len = 1 + r(rng, 100);
            let c = rng.below(256) as u8;
            let up = rng.below(2) == 1;
            for i in 0..len {
                let y = if up { y0 - i } else { y0 + i };
                f.pixel((x0 + i) as i16, y as i16, c);
            }
        }
        3 => {
            f.0.copy_within(W.., 0);
            for p in f.row_mut(H - 1) {
                *p = rng.below(256) as u8;
            }
        }
        4 => {
            for _ in 0..1 + rng.below(40) {
                let (x, y) = (r(rng, W as u32), r(rng, H as u32));
                let w = 1 + r(rng, 8);
                f.rect(x as i16, y as i16, w as u16, 1, rng.below(256) as u8);
            }
        }
        _ => {}
    }
}

fn line_of(i: usize, areas: &Areas) -> String {
    let mut s = format!("{i}:");
    for r in areas.as_slice() {
        s += &format!(" {},{},{},{}", r.x, r.y, r.w, r.h);
    }
    s
}

/// A palette where every number has its own colour.
fn spread_palette(p: &mut Palette) {
    for i in 0..=255u8 {
        p.set(i, u16::from(i).wrapping_mul(0x0101) ^ 0x5A5A);
    }
}

#[test]
fn same_areas_as_the_design_page() {
    let golden = include_str!("data/sim_areas.txt");
    let mut video = Box::new(Video::new());
    let mut board = Box::new(FakeBoard::new());
    let mut rng = Rng(0x2545_f491);
    let mut lines = golden.lines();
    for i in 0..400 {
        step(video.frame_mut(), &mut rng);
        video.present(&mut board.screen);
        assert_eq!(
            line_of(i, video.last_areas()),
            lines.next().unwrap(),
            "step {i}"
        );
    }
}

#[test]
fn seeded_frames_keep_the_rule_on_every_walk() {
    for (n, walk) in [
        Walk::Rows,
        Walk::RowsLeft,
        Walk::Columns,
        Walk::ColumnsUp,
        Walk::Indexed,
    ]
    .into_iter()
    .enumerate()
    {
        let mut video = Box::new(Video::new());
        spread_palette(video.palette_mut());
        let mut board = Box::new(FakeBoard::new());
        board.screen.walk = walk;
        let mut rng = Rng(0x1234_5678 + n as u32);
        let mut before = boxed_frame();
        for i in 0..150 {
            step(video.frame_mut(), &mut rng);
            video.present(&mut board.screen);
            if i > 0 {
                check_rule(&before, video.frame(), video.last_areas());
            }
            // What the panel shows equals the frame, pixel for pixel.
            for y in 0..240u16 {
                for x in 0..320u16 {
                    let want = video.palette().get(video.frame().get(x / 2, y / 2));
                    assert_eq!(
                        board.screen.pixel(x, y),
                        want,
                        "{walk:?} step {i} ({x},{y})"
                    );
                }
            }
            before.0.copy_from_slice(&video.frame().0);
        }
    }
}
