//! The renderer against the fake board.

use board_api::Rect;
use board_api::fake::{FakeBoard, Walk};
use console_video::{Video, rgb565};

#[test]
fn flat_colour_fills_the_panel() {
    let mut video = Box::new(Video::new());
    let mut board = Box::new(FakeBoard::new());
    let blue = rgb565(0, 0, 255);
    video.palette_mut().set(7, blue);
    video.frame_mut().clear(7);
    let r = video.present(&mut board.screen);
    assert_eq!(r.pixels, 320 * 240);
    assert!(board.screen.image.iter().all(|&c| c == blue));

    // Nothing changed: the board is not even called.
    let r = video.present(&mut board.screen);
    assert_eq!(r.pixels, 0);
    assert_eq!(board.screen.presents, 1);
}

#[test]
fn one_pixel_sends_a_small_area() {
    let mut video = Box::new(Video::new());
    let mut board = Box::new(FakeBoard::new());
    video.palette_mut().set(1, 0xFFFF);
    video.present(&mut board.screen);
    video.frame_mut().set(10, 20, 1);
    let r = video.present(&mut board.screen);
    assert_eq!(video.last_areas().as_slice(), &[Rect::new(10, 20, 1, 1)]);
    assert!(r.pixels >= 4, "at least the 2x2 panel pixels");
    assert_eq!(board.screen.pixel(20, 40), 0xFFFF);
    assert_eq!(board.screen.pixel(21, 41), 0xFFFF);
    assert_eq!(board.screen.pixel(22, 40), 0);
}

#[test]
fn every_walk_gives_the_same_picture() {
    let mut first: Option<Box<FakeBoard>> = None;
    for walk in [
        Walk::Rows,
        Walk::RowsLeft,
        Walk::Columns,
        Walk::ColumnsUp,
        Walk::Indexed,
    ] {
        let mut video = Box::new(Video::new());
        for i in 0..=255u8 {
            video
                .palette_mut()
                .set(i, u16::from(i).wrapping_mul(0x0101) ^ 0x5A5A);
        }
        for (i, p) in video.frame_mut().0.iter_mut().enumerate() {
            *p = (i % 251) as u8;
        }
        let mut board = Box::new(FakeBoard::new());
        board.screen.walk = walk;
        video.present(&mut board.screen);
        match &first {
            None => first = Some(board),
            Some(f) => assert!(f.screen.image == board.screen.image, "{walk:?}"),
        }
    }
}

#[test]
fn test_card_through_the_whole_path() {
    let mut video = Box::new(Video::new());
    test_card::draw_fb_card(&mut test_card::Fb::new(&mut video.frame_mut().0)).unwrap();
    video.palette_mut().set_all(&test_card::PALETTE);
    for walk in [Walk::Rows, Walk::Columns, Walk::ColumnsUp, Walk::Indexed] {
        let mut board = Box::new(FakeBoard::new());
        board.screen.walk = walk;
        video.force_full();
        let r = video.present(&mut board.screen);
        assert_eq!(r.pixels, 320 * 240);
        for y in 0..240u16 {
            for x in 0..320u16 {
                let want = test_card::PALETTE[usize::from(video.frame().get(x / 2, y / 2))];
                assert_eq!(board.screen.pixel(x, y), want, "{walk:?} ({x},{y})");
            }
        }
        // The card's corner pixels, as the card itself names them.
        let corners = [(0, 0), (319, 0), (0, 239), (319, 239)];
        for ((x, y), c) in corners.into_iter().zip(test_card::FB_CORNERS) {
            assert_eq!(board.screen.pixel(x, y), test_card::PALETTE[usize::from(c)]);
        }
    }
}

#[test]
fn palette_change_sends_everything() {
    let mut video = Box::new(Video::new());
    let mut board = Box::new(FakeBoard::new());
    video.frame_mut().set(3, 3, 1);
    video.present(&mut board.screen);
    // Same pixels, one new colour: every pixel may look different.
    video.palette_mut().set(0, rgb565(255, 0, 0));
    let r = video.present(&mut board.screen);
    assert_eq!(video.last_areas().as_slice(), &[Rect::new(0, 0, 160, 120)]);
    assert_eq!(r.pixels, 320 * 240);
    assert_eq!(board.screen.pixel(100, 100), rgb565(255, 0, 0));
    // Setting the same colour again is no change.
    video.palette_mut().set(0, rgb565(255, 0, 0));
    assert_eq!(video.present(&mut board.screen).pixels, 0);
}

#[test]
fn memory_is_as_written_down() {
    use console_video::{Areas, Frame, Palette};
    use core::mem::size_of;
    assert_eq!(size_of::<Frame>(), 19_200);
    assert_eq!(size_of::<Palette>(), 516);
    // 16 rectangles of 8 bytes, plus the count (8 bytes on the PC, 4 on
    // the chip).
    assert_eq!(size_of::<Areas>(), 16 * 8 + size_of::<usize>());
    assert!(size_of::<Video>() <= 2 * 19_200 + 516 + 136 + 16);
}

#[cfg(feature = "embedded-graphics")]
#[test]
fn embedded_graphics_draws_the_same_card() {
    let mut a = Box::new(console_video::Frame::new());
    let mut b = Box::new(console_video::Frame::new());
    test_card::draw_fb_card(&mut *a).unwrap();
    test_card::draw_fb_card(&mut test_card::Fb::new(&mut b.0)).unwrap();
    assert!(a.0 == b.0);
}

/// A game writes into the picture and the palette; the console then
/// tells the palette, and the next present sends everything again.
#[test]
fn game_buffers_point_at_the_picture_and_the_palette() {
    let mut v = Video::new();
    let (px, pal) = v.game_buffers();
    let (px, pal) = (px.as_ptr(), pal.as_ptr());
    assert_eq!(px.addr() % 4, 0);
    assert_eq!(pal.addr() % 2, 0);
    assert_eq!(px, v.frame().0.as_ptr());
    assert_eq!(pal, v.palette().colours().as_ptr());
    let before = v.palette().version();
    v.palette_mut().touch();
    assert_ne!(v.palette().version(), before);
}

/// Colours that no pixel on the panel uses change without a whole-screen
/// send; the next present sends only what was drawn differently, in the
/// new colours. A changed colour that IS on the panel still sends all.
#[test]
fn colours_not_on_the_panel_change_without_a_full_send() {
    let mut video = Box::new(Video::new());
    let mut board = Box::new(FakeBoard::new());
    video.palette_mut().set(1, 0x1111);
    video.palette_mut().set(2, 0x2222);
    video.frame_mut().clear(1);
    video.present(&mut board.screen);
    let presents = board.screen.presents;

    // Colour 3 and 200 are on no pixel: no send at all for them.
    let version = video.palette().version();
    video.recolour_unseen(|p| {
        p.set(3, 0x3333);
        p.set(200, 0x4444);
    });
    assert_ne!(video.palette().version(), version, "the version changes");
    assert_eq!(video.present(&mut board.screen).pixels, 0);
    assert_eq!(board.screen.presents, presents, "nothing to send");

    // A pixel drawn in a new colour: only its area goes.
    video.frame_mut().set(10, 20, 3);
    video.present(&mut board.screen);
    assert_eq!(video.last_areas().as_slice(), &[Rect::new(10, 20, 1, 1)]);
    assert_eq!(board.screen.pixel(20, 40), 0x3333);

    // Colour 1 is on the panel: the whole screen goes, as for any change.
    video.recolour_unseen(|p| p.set(1, 0x5555));
    video.present(&mut board.screen);
    assert_eq!(video.last_areas().as_slice(), &[Rect::new(0, 0, 160, 120)]);
    assert_eq!(board.screen.pixel(0, 0), 0x5555);

    // Nothing changed: nothing at all happens.
    video.recolour_unseen(|p| p.set(2, 0x2222));
    assert_eq!(video.present(&mut board.screen).pixels, 0);
}

/// Before the first present the panel is not in step: the first present
/// sends everything whatever the colours did.
#[test]
fn recolouring_before_the_first_present_still_sends_all() {
    let mut video = Box::new(Video::new());
    let mut board = Box::new(FakeBoard::new());
    video.recolour_unseen(|p| p.set(9, 0x9999));
    let r = video.present(&mut board.screen);
    assert_eq!(r.pixels, 320 * 240);
}
