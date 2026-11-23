//! A small speed check on the PC. Not a test: it only catches a path
//! that is far slower than it should be. The PC is about 50 times faster
//! than the chip and works very differently, so only the board can give
//! real numbers.
//!
//!     cargo run --release --example bench

use std::hint::black_box;
use std::time::Instant;

use board_api::fake::{FakeBoard, Walk};
use board_api::{ByteOrder, Dir, PixelSource};
use console_video::{Areas, CLEAR, Frame, Palette, Source, Sprite, Video, View, diff};

/// Runs `f` many times and prints the time of one run.
fn time(name: &str, reps: u32, mut f: impl FnMut()) {
    f();
    let t = Instant::now();
    for _ in 0..reps {
        f();
    }
    let us = t.elapsed().as_secs_f64() * 1e6 / f64::from(reps);
    println!("{name:<44} {us:>9.2} us");
}

fn main() {
    let mut a = Box::new(Frame::new());
    for (i, p) in a.0.iter_mut().enumerate() {
        *p = (i * 7 % 251) as u8;
    }
    let mut pal = Box::new(Palette::new());
    for i in 0..=255u8 {
        pal.set(i, u16::from(i) * 257);
    }
    let mut areas = Areas::new();

    println!("-- finding changed areas (diff), one frame");
    let same = a.clone();
    time("nothing changed", 20_000, || {
        diff(black_box(&a), black_box(&same), &mut areas)
    });
    let mut sprite = a.clone();
    sprite.rect(40, 40, 16, 16, 3);
    time("one 16x16 sprite", 20_000, || {
        diff(black_box(&a), black_box(&sprite), &mut areas)
    });
    let mut scroll = a.clone();
    scroll.0.copy_within(160.., 0);
    time("scroll (every pixel)", 20_000, || {
        diff(black_box(&a), black_box(&scroll), &mut areas)
    });
    let mut marks = a.clone();
    for i in 0..60u16 {
        marks.set(i * 2 + 10, i * 2, !marks.get(i * 2 + 10, i * 2));
    }
    time("60 marks on separate rows (many joins)", 20_000, || {
        diff(black_box(&a), black_box(&marks), &mut areas)
    });

    println!("-- making panel pixels (Source::run), whole 320x240 panel");
    let src = Source::new(&a, &pal, View::fit(320, 240), 0);
    let mut row = [0u16; 320];
    let mut col = [0u16; 240];
    time("rows, native order", 2_000, || {
        for y in 0..240 {
            src.run(0, y, Dir::Right, ByteOrder::Native, &mut row);
        }
        black_box(&row);
    });
    time("columns, wire order (the CYD's way)", 2_000, || {
        for x in 0..320 {
            src.run(x, 239, Dir::Up, ByteOrder::Wire, &mut col);
        }
        black_box(&col);
    });

    println!("-- drawing, one frame");
    let mut f = Box::new(Frame::new());
    time("clear", 20_000, || f.clear(black_box(4)));
    let pixels: Vec<u8> = (0..256)
        .map(|i| if i % 5 == 0 { CLEAR } else { i as u8 })
        .collect();
    let spr = Sprite::new(16, 16, &pixels);
    time("20 sprites of 16x16 with see-through", 20_000, || {
        for i in 0..20 {
            f.sprite(i * 7, i * 5, black_box(&spr), CLEAR);
        }
    });
    time("4 lines of 30 letters", 20_000, || {
        for i in 0..4 {
            f.text(2, 2 + i * 6, black_box("THE QUICK BROWN FOX 0123456789"), 7);
        }
    });

    println!("-- the whole present, fake board (includes its 150 KB copy)");
    let mut video = Box::new(Video::new());
    video.palette_mut().set_all(pal.colours());
    let mut board = Box::new(FakeBoard::new());
    for walk in [Walk::Rows, Walk::Columns, Walk::Indexed] {
        board.screen.walk = walk;
        let name = format!("full frame, walk {walk:?}");
        time(&name, 500, || {
            video.force_full();
            black_box(video.present(&mut board.screen));
        });
    }
    let mut x = 0;
    time("one moving 16x16 sprite, walk Indexed", 20_000, || {
        video.frame_mut().rect(x % 140, 50, 16, 16, 9);
        black_box(video.present(&mut board.screen));
        video.frame_mut().rect(x % 140, 50, 16, 16, 0);
        x += 1;
    });
}
