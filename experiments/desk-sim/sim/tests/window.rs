//! The window board, tested with SDL's "offscreen" video driver: the
//! window exists only in memory, so nothing appears on the desktop.
//! Key presses and clicks are put into SDL's queue by the test.

#![cfg(feature = "window")]

use std::time::Duration;

use board_layer::{Board, button};
use console_demo::{Demo, Scene};
use desk_sim::window::{WindowBoard, WindowOptions};
use sdl3::event::Event;
use sdl3::keyboard::{Keycode, Mod, Scancode};
use sdl3::mouse::MouseButton;

fn key(scancode: Scancode) -> Event {
    Event::KeyDown {
        timestamp: 0,
        window_id: 0,
        // SDL wants a key name too; the board only looks at the scancode.
        keycode: Keycode::from_scancode(scancode, sdl3::sys::keycode::SDL_Keymod(0), true),
        scancode: Some(scancode),
        keymod: Mod::empty(),
        repeat: false,
        which: 0,
        raw: 0,
    }
}

#[test]
fn keys_and_mouse_reach_the_demo_through_the_window() {
    // Must be set before SDL starts.
    sdl3::hint::set("SDL_VIDEO_DRIVER", "offscreen");
    sdl3::hint::set("SDL_RENDER_DRIVER", "software");
    let shots = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("window-shots");
    let _ = std::fs::remove_dir_all(&shots);
    let mut board = WindowBoard::open(WindowOptions {
        scale: 2,
        fps: 60,
        close_after: Some(Duration::from_secs(20)), // a safety net only
        shots_dir: shots.clone(),
    })
    .unwrap();
    let queue = board.event_queue().unwrap();
    let mut demo = Demo::new();
    let step = |board: &mut WindowBoard, demo: &mut Demo| {
        let tick = board.wait_frame().expect("the window closed too early");
        demo.frame(board, tick);
        tick
    };

    let first = step(&mut board, &mut demo);
    assert_eq!(demo.scene, Scene::Start);

    // Enter is START: it leaves the start screen.
    queue.push_event(key(Scancode::Return)).unwrap();
    step(&mut board, &mut demo);
    assert_eq!(demo.scene, Scene::Play);

    // Each key gives its button, for one frame at least.
    for (scancode, bit, name) in desk_sim::window::KEYS {
        if bit == button::SELECT || bit == button::HOME {
            continue; // these change the scene; checked below
        }
        queue.push_event(key(scancode)).unwrap();
        step(&mut board, &mut demo);
        assert!(board.pad().held(bit), "key {name}");
    }

    // A click at window pixel (400, 200) is a touch at (200, 100), as
    // the window is twice the screen's size.
    queue
        .push_event(Event::MouseButtonDown {
            timestamp: 0,
            window_id: 0,
            which: 0,
            mouse_btn: MouseButton::Left,
            clicks: 1,
            x: 400.0,
            y: 200.0,
        })
        .unwrap();
    step(&mut board, &mut demo);
    let touch = board.touch();
    assert!(touch.pressed);
    assert_eq!((touch.x, touch.y), (200, 100));

    // Backspace is SELECT: the test card.
    queue.push_event(key(Scancode::Backspace)).unwrap();
    step(&mut board, &mut demo);
    assert_eq!(demo.scene, Scene::PanelCard);

    // What the window shows is the test card, each pixel a sharp 2x2
    // square, with red, green and blue in the right places (read back
    // from the window itself, not from the console's copy).
    let (w, h, shown) = board.read_back().unwrap();
    assert_eq!((w, h), (640, 480));
    let screen = board.screen().to_vec();
    let mut wrong = 0;
    for y in 0..h {
        for x in 0..w {
            let want = desk_sim::picture::to_rgb888(screen[(y / 2) * 320 + x / 2]);
            if shown[(y * w + x) * 3..][..3] != want {
                wrong += 1;
            }
        }
    }
    assert_eq!(
        wrong, 0,
        "window pixels that differ from the console's picture"
    );
    assert_eq!(&shown[..3], &[255, 0, 0], "top left must be red");

    // F12 saves a screenshot.
    queue.push_event(key(Scancode::F12)).unwrap();
    step(&mut board, &mut demo);
    let saved: Vec<_> = std::fs::read_dir(&shots).unwrap().collect();
    assert_eq!(saved.len(), 1);

    // P pauses; N then runs exactly one frame; P again goes on.
    queue.push_event(key(Scancode::P)).unwrap();
    queue.push_event(key(Scancode::N)).unwrap();
    let before = board.now_us();
    let stepped = step(&mut board, &mut demo);
    assert!(board.is_paused());
    assert!(stepped.now_us > before);
    queue.push_event(key(Scancode::P)).unwrap();
    step(&mut board, &mut demo);
    assert!(!board.is_paused());

    // Tab is HOME: back to the start screen.
    queue.push_event(key(Scancode::Tab)).unwrap();
    step(&mut board, &mut demo);
    assert_eq!(demo.scene, Scene::Start);

    // The clock moved by exactly one frame each frame (60 a second).
    let last = step(&mut board, &mut demo);
    assert_eq!(last.now_us, last.frame as u64 * 1_000_000 / 60);
    assert_eq!(first.frame, 0);

    // Esc closes.
    queue.push_event(key(Scancode::Escape)).unwrap();
    assert!(board.wait_frame().is_none());
}
