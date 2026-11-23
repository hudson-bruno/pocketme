//! The fake board itself: every way of walking the panel gives the same
//! picture, and the small parts behave like the real ones.

use board_api::fake::{FAKE_H, FAKE_W, FakeBoard, Walk};
use board_api::{
    Board, ByteOrder, Dir, Files, Indexed, Link, LogOut, PixelSource, Rect, Screen, Store,
};

/// A test source: a 4x3 indexed picture shown 8 times bigger at (16, 24),
/// with a grey border. Each pixel's colour is its own number.
struct Tiny {
    pixels: [u8; 12],
    palette: [u16; 256],
}

impl Tiny {
    fn new() -> Self {
        let mut palette = [0u16; 256];
        for (i, c) in palette.iter_mut().enumerate() {
            *c = (i as u16).wrapping_mul(0x0841) ^ 0x1234;
        }
        Tiny {
            pixels: core::array::from_fn(|i| i as u8 + 1),
            palette,
        }
    }

    fn at(&self, x: u16, y: u16) -> u16 {
        let ix = self.indexed().unwrap();
        let (k, l, t) = (8u16, ix.left, ix.top);
        if x < l || y < t || x >= l + 4 * k || y >= t + 3 * k {
            return ix.border;
        }
        let i = usize::from((y - t) / k) * 4 + usize::from((x - l) / k);
        self.palette[usize::from(self.pixels[i])]
    }
}

impl PixelSource for Tiny {
    fn run(&self, x: u16, y: u16, dir: Dir, order: ByteOrder, out: &mut [u16]) {
        for (i, o) in out.iter_mut().enumerate() {
            let i = i as u16;
            let (px, py) = match dir {
                Dir::Right => (x + i, y),
                Dir::Left => (x - i, y),
                Dir::Down => (x, y + i),
                Dir::Up => (x, y - i),
            };
            *o = order.apply(self.at(px, py));
        }
    }

    fn indexed(&self) -> Option<Indexed<'_>> {
        Some(Indexed {
            pixels: &self.pixels,
            width: 4,
            height: 3,
            scale: 8,
            left: 16,
            top: 24,
            palette: &self.palette,
            palette_version: 1,
            border: 0x8410,
        })
    }
}

#[test]
fn every_walk_gives_the_same_picture() {
    let src = Tiny::new();
    let areas = [Rect::new(0, 0, FAKE_W, FAKE_H)];
    let mut first: Option<Box<FakeBoard>> = None;
    for walk in [
        Walk::Rows,
        Walk::RowsLeft,
        Walk::Columns,
        Walk::ColumnsUp,
        Walk::Indexed,
    ] {
        for order in [ByteOrder::Native, ByteOrder::Wire] {
            let mut b = Box::new(FakeBoard::new());
            b.screen.walk = walk;
            b.screen.order = order;
            let report = b.parts().screen.present(&src, &areas);
            assert_eq!(report.pixels, 320 * 240);
            assert_eq!(b.screen.last_was_indexed, walk == Walk::Indexed);
            assert_eq!(b.screen.pixel(0, 0), 0x8410, "border");
            assert_eq!(b.screen.pixel(16, 24), src.palette[1]);
            assert_eq!(b.screen.pixel(16 + 31, 24 + 23), src.palette[12]);
            match &first {
                None => first = Some(b),
                Some(f) => assert!(f.screen.image == b.screen.image, "{walk:?} {order:?}"),
            }
        }
    }
}

#[test]
fn only_the_areas_are_sent() {
    let src = Tiny::new();
    let mut b = Box::new(FakeBoard::new());
    let r = b
        .screen
        .present(&src, &[Rect::new(16, 24, 8, 8), Rect::EMPTY]);
    assert_eq!((r.areas, r.pixels), (1, 64));
    assert_eq!(b.screen.last_area_count, 2);
    assert_eq!(b.screen.pixel(16, 24), src.palette[1]);
    assert_eq!(b.screen.pixel(24, 24), 0, "outside the area: untouched");
}

#[test]
fn small_parts() {
    let mut b = Box::new(FakeBoard::new());
    b.files.files = &[
        ("/CONSOLE/GAMES/SNAKE.BIN", b"hello"),
        ("/CONSOLE/X.TXT", b"x"),
    ];
    let mut names = Vec::new();
    b.files
        .list("/CONSOLE/GAMES", |e| {
            names.push((e.name.to_string(), e.size));
            true
        })
        .unwrap();
    assert_eq!(names, [("SNAKE.BIN".to_string(), 5)]);
    let mut buf = [0u8; 8];
    assert_eq!(
        b.files.read_at("/CONSOLE/GAMES/SNAKE.BIN", 2, &mut buf),
        Ok(3)
    );
    assert_eq!(&buf[..3], b"llo");

    b.link.feed(b"ping");
    let mut buf = [0u8; 2];
    assert_eq!(b.link.read(&mut buf), 2);
    assert_eq!(b.link.read(&mut buf), 2);
    assert_eq!(&buf, b"ng");
    b.link.write(b"ack");
    assert_eq!(b.link.sent(), b"ack");

    b.log.line("hello");
    assert_eq!((b.log.lines, b.log.last()), (1, "hello"));

    b.store.write(8, &[0x0F; 4]).unwrap();
    b.store.write(8, &[0xF3; 4]).unwrap();
    let mut got = [0u8; 4];
    b.store.read(8, &mut got).unwrap();
    assert_eq!(got, [0x03; 4], "a write only clears bits");
    b.store.erase_sector(0).unwrap();
    b.store.read(8, &mut got).unwrap();
    assert_eq!(got, [0xFF; 4]);
}

/// Short presses (step 3): a board that notes nothing (the defaults, as
/// `Absent` has them) says "nothing noted"; the fake board hands each
/// note out once.
#[test]
fn noted_presses_are_handed_out_once() {
    use board_api::fake::{FakeKeys, FakePad, FakeTouch};
    use board_api::{Absent, Keys, Pad, PadState, Touch, TouchPoint, btn, key};

    let mut a = Absent;
    assert_eq!(Keys::take_pressed(&mut a), 0);
    assert_eq!(Pad::take_pressed(&mut a, 0), 0);
    assert_eq!(a.take_tap(), None);

    let mut keys = FakeKeys {
        held: 0,
        pressed: key::BOOT,
    };
    assert_eq!(keys.take_pressed(), key::BOOT);
    assert_eq!(keys.take_pressed(), 0);

    let mut pad = FakePad {
        state: PadState {
            connected: true,
            ..PadState::NONE
        },
        motion: None,
        pressed: btn::SOUTH,
        ..FakePad::default()
    };
    assert_eq!(pad.take_pressed(1), 0, "pad 1 is not there yet");
    assert_eq!(pad.take_pressed(0), btn::SOUTH);
    assert_eq!(pad.take_pressed(0), 0);
    pad.pressed = btn::EAST;
    pad.state.connected = false;
    assert_eq!(pad.take_pressed(0), 0, "a pad that is not there");

    let at = TouchPoint { x: 3, y: 4 };
    let mut touch = FakeTouch {
        at: None,
        tap: Some(at),
    };
    assert_eq!(touch.take_tap(), Some(at));
    assert_eq!(touch.take_tap(), None);
}

#[test]
fn the_fake_pad_holds_four_pads_apart() {
    use board_api::fake::{FakePad, OnePad};
    use board_api::{Motion, Pad, PadState, btn};

    let mut pad = FakePad::default();
    let m = Motion {
        accel: [1, 2, 3],
        gyro: [4, 5, 6],
    };
    *pad.other(2) = OnePad {
        state: PadState {
            connected: true,
            buttons: btn::NORTH,
            ..PadState::NONE
        },
        motion: Some(m),
        pressed: btn::WEST,
    };
    assert_eq!(pad.read(0), PadState::NONE);
    assert_eq!(pad.read(1), PadState::NONE);
    assert_eq!(pad.read(2).buttons, btn::NORTH);
    assert_eq!(pad.read(3), PadState::NONE);
    assert_eq!(pad.read(4), PadState::NONE, "no pad 4");
    assert_eq!(pad.motion(2), Some(m));
    assert_eq!(pad.motion(0), None);
    assert_eq!(pad.take_pressed(0), 0);
    assert_eq!(pad.take_pressed(1), 0);
    assert_eq!(pad.take_pressed(2), btn::WEST);
    assert_eq!(pad.take_pressed(2), 0, "handed out once");
    // A pad that goes away: no motion, no notes.
    pad.other(2).state.connected = false;
    pad.other(2).pressed = btn::SOUTH;
    assert_eq!(pad.motion(2), None);
    assert_eq!(pad.take_pressed(2), 0);
}

/// Lent RAM: the fake board lends rubbish while no game is loaded, the
/// same bytes from one loan to the next, nothing while a game is loaded,
/// and every start (good or not) and every end makes it other rubbish.
#[test]
fn lent_ram_follows_the_loan_rules() {
    use board_api::StartError;
    use board_api::fake::FAKE_LENT;

    let mut b = FakeBoard::new();
    assert_eq!(FAKE_LENT, game_api::layout::DATA_SLOT_SIZE as usize);
    let first: Vec<u8> = b.parts_and_lent().1.expect("lent at start").to_vec();
    assert_eq!(first.len(), FAKE_LENT);
    assert!(first.iter().any(|&x| x != 0), "rubbish, not zeros");
    // Written, and the same bytes come back at the next loan.
    b.parts_and_lent().1.unwrap()[..4].copy_from_slice(b"HOME");
    assert_eq!(&b.parts_and_lent().1.unwrap()[..4], b"HOME");
    // A start that fails still ends the loan: the bytes are gone.
    assert!(matches!(
        b.start_game("NONE"),
        Err(StartError::NotSupported)
    ));
    let after = b.parts_and_lent().1.expect("lent again: no game loaded");
    assert_ne!(&after[..4], b"HOME");
    assert_ne!(after, &first[..]);
    // A board that lends nothing: the default.
    let mut a = board_api::fake::FakeBoard::new();
    a.games.lent.loaded = true;
    assert!(a.parts_and_lent().1.is_none());
}
