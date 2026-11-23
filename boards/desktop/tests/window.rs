//! The window, tested with SDL's "offscreen" video driver: the window
//! exists only in memory, so nothing appears on the desktop. Key presses
//! and clicks are put into SDL's queue by the test.
//!
//! SDL can be started once per program, so this is one long test.

#![cfg(feature = "window")]

use std::time::Duration;

use board_api::{ByteOrder, Dir, PixelSource, Rect, Screen, Wake, btn, key};
use console_core::Console;
use desktop_board::board::{DesktopBoard, PANEL_H, PANEL_W, window_dim};
use desktop_board::keymap::{self, Control};
use desktop_board::picture;
use desktop_board::run::{ScriptRun, run_script};
use desktop_board::window::{SimWindow, WindowOptions};
use sdl3::event::Event;
use sdl3::keyboard::{Keycode, Mod, Scancode};
use sdl3::mouse::{MouseButton, MouseState};

fn key_event(scancode: Scancode, down: bool) -> Event {
    // SDL wants a key name too; the board only looks at the scancode.
    let keycode = Keycode::from_scancode(scancode, sdl3::sys::keycode::SDL_Keymod(0), true);
    if down {
        Event::KeyDown {
            timestamp: 0,
            window_id: 0,
            keycode,
            scancode: Some(scancode),
            keymod: Mod::empty(),
            repeat: false,
            which: 0,
            raw: 0,
        }
    } else {
        Event::KeyUp {
            timestamp: 0,
            window_id: 0,
            keycode,
            scancode: Some(scancode),
            keymod: Mod::empty(),
            repeat: false,
            which: 0,
            raw: 0,
        }
    }
}

fn click(down: bool, x: f32, y: f32) -> Event {
    if down {
        Event::MouseButtonDown {
            timestamp: 0,
            window_id: 0,
            which: 0,
            mouse_btn: MouseButton::Left,
            clicks: 1,
            x,
            y,
        }
    } else {
        Event::MouseButtonUp {
            timestamp: 0,
            window_id: 0,
            which: 0,
            mouse_btn: MouseButton::Left,
            clicks: 1,
            x,
            y,
        }
    }
}

/// A picture where every pixel differs, with pure red at the top left.
struct Pattern;

fn pattern(x: u16, y: u16) -> u16 {
    if x == 0 && y == 0 {
        return 0xF800;
    }
    ((x & 0x1F) << 11) | (((x / 2) ^ y) & 0x3F) << 5 | (y & 0x1F)
}

impl PixelSource for Pattern {
    fn run(&self, x: u16, y: u16, _dir: Dir, order: ByteOrder, out: &mut [u16]) {
        for (i, p) in out.iter_mut().enumerate() {
            *p = order.apply(pattern(x + i as u16, y));
        }
    }
}

/// A console stand-in that sends the pattern once, then sleeps until
/// input. It also notes what input it saw.
fn pattern_step(
    seen: &mut Vec<(u32, u16, Option<board_api::TouchPoint>)>,
) -> impl FnMut(&mut DesktopBoard) -> Wake + '_ {
    move |b: &mut DesktopBoard| {
        if b.screen.presents == 0 {
            b.screen
                .present(&Pattern, &[Rect::new(0, 0, PANEL_W, PANEL_H)]);
        }
        // What is held now, and what the board noted since the last step
        // (taken, as the console does).
        let pressed = board_api::Pad::take_pressed(&mut b.pad, 0);
        let keys = board_api::Keys::take_pressed(&mut b.keys);
        let tap = board_api::Touch::take_tap(&mut b.touch);
        seen.push((
            b.pad.state.buttons | pressed,
            b.keys.held | keys,
            b.touch.at.or(tap),
        ));
        Wake::InputOr(u64::MAX)
    }
}

#[test]
fn the_window_shows_the_panel_and_takes_keys_and_mouse() {
    // Must be set before SDL starts.
    sdl3::hint::set("SDL_VIDEO_DRIVER", "offscreen");
    sdl3::hint::set("SDL_RENDER_DRIVER", "software");
    let shots = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("window-shots");
    let _ = std::fs::remove_dir_all(&shots);
    let mut win = SimWindow::open(WindowOptions {
        scale: 2,
        fps: 30,
        close_after: Some(Duration::from_secs(5)), // a safety net only
        shots_dir: shots.clone(),
        overlay: false,
    })
    .unwrap();
    let queue = win.event_queue().unwrap();
    let mut board = Box::new(DesktopBoard::new(None));
    let mut seen = Vec::new();

    // 1. Every key of the table reaches the board, for at least one step,
    //    even when it goes down and up before the step.
    {
        let mut step = pattern_step(&mut seen);
        win.advance(&mut board, 1, &mut step); // the first step: no input
        for &(name, _, _) in keymap::KEYS {
            let sc = Scancode::from_name(name).unwrap_or_else(|| panic!("no SDL key `{name}`"));
            queue.push_event(key_event(sc, true)).unwrap();
            queue.push_event(key_event(sc, false)).unwrap();
            win.handle_events(&mut board);
            win.advance(&mut board, 1, &mut step);
        }
    }
    for (i, &(name, control, _)) in keymap::KEYS.iter().enumerate() {
        // Step 0 is the first one (no input yet); key i is step i + 1.
        let (buttons, keys, _) = seen[i + 1];
        match control {
            Control::Button(bit) => assert_eq!(buttons, bit, "key {name}"),
            Control::Boot => assert_eq!(keys, key::BOOT, "key {name}"),
            Control::Trigger(_) => assert_ne!(buttons & (btn::ZL | btn::ZR), 0, "{name}"),
            // A stick key held for less than a step moves nothing: sticks
            // are not latched. Checked held, below.
            Control::Stick(..) => {}
            // The motion stand-in presses no button.
            Control::MotionSwitch | Control::Tilt => assert_eq!(buttons, 0, "key {name}"),
        }
    }
    assert_eq!(seen.len(), keymap::KEYS.len() + 1);

    // A held stick key moves the stick.
    queue.push_event(key_event(Scancode::L, true)).unwrap();
    win.handle_events(&mut board);
    assert_eq!(win.controls.pad().left.x, 32767);
    queue.push_event(key_event(Scancode::L, false)).unwrap();
    win.handle_events(&mut board);
    assert_eq!(win.controls.pad().left.x, 0);

    // The number pad is player 2's: its first key plugs pad 2 in, and its
    // buttons reach pad 2 only. Every name is one SDL knows.
    assert!(!win.controls.players[1].pad.connected);
    for &(name, control, _) in keymap::PLAYER2_KEYS {
        let sc = Scancode::from_name(name).unwrap_or_else(|| panic!("no SDL key `{name}`"));
        queue.push_event(key_event(sc, true)).unwrap();
        win.handle_events(&mut board);
        let p2 = win.controls.players[1].pad;
        assert!(p2.connected, "key {name}");
        match control {
            Control::Button(bit) => assert_eq!(p2.buttons, bit, "key {name}"),
            Control::Trigger(_) => assert_ne!(p2.buttons & (btn::ZL | btn::ZR), 0, "{name}"),
            other => panic!("{other:?} on the number pad"),
        }
        assert_eq!(win.controls.pad().buttons, 0, "key {name}: not player 1's");
        queue.push_event(key_event(sc, false)).unwrap();
        win.handle_events(&mut board);
        assert_eq!(win.controls.players[1].pad.buttons, 0, "key {name}");
    }

    // 2. The mouse: window pixel (400, 200) is panel pixel (200, 100) at
    //    2x. A drag moves the touch; letting go lifts it.
    queue.push_event(click(true, 400.0, 200.0)).unwrap();
    win.handle_events(&mut board);
    assert_eq!(board_touch(&win), Some((200, 100)));
    let drag = Event::MouseMotion {
        timestamp: 0,
        window_id: 0,
        which: 0,
        mousestate: MouseState::from_sdl_state(1), // left button held
        x: 402.0,
        y: 204.0,
        xrel: 2.0,
        yrel: 4.0,
    };
    queue.push_event(drag).unwrap();
    win.handle_events(&mut board);
    assert_eq!(board_touch(&win), Some((201, 102)));
    queue.push_event(click(false, 402.0, 204.0)).unwrap();
    win.handle_events(&mut board);
    assert_eq!(board_touch(&win), None);

    // 3. What the window shows, read back from the window itself, is the
    //    panel picture: each pixel a sharp 2x2 square, colours right.
    win.draw(&board);
    let (w, h, shown) = win.read_back(&board).unwrap();
    assert_eq!((w, h), (640, 480));
    assert_eq!(&shown[..3], &[255, 0, 0], "top left must be red");
    assert_eq!(wrong_pixels(&shown, w, &board.screen.image), 0);

    // 4. The overlay is drawn on the window only, not into the picture.
    queue.push_event(key_event(Scancode::F1, true)).unwrap();
    win.handle_events(&mut board);
    assert!(win.overlay);
    win.draw(&board);
    assert_eq!(board.screen.image[0], 0xF800);

    // 5. F12 saves a screenshot of the panel.
    queue.push_event(key_event(Scancode::F12, true)).unwrap();
    win.handle_events(&mut board);
    assert_eq!(win.shots.len(), 1);
    assert!(win.shots[0].exists());

    // 6. P pauses (the run loop then runs ticks only on N); P again goes on.
    queue.push_event(key_event(Scancode::P, true)).unwrap();
    win.handle_events(&mut board);
    assert!(win.paused);
    queue.push_event(key_event(Scancode::P, true)).unwrap();
    win.handle_events(&mut board);
    assert!(!win.paused);

    // 7. The real console in the window gives the same picture as with
    //    no window, read back from the window.
    let mut console = Box::new(Console::new());
    let mut board = Box::new(DesktopBoard::new(None));
    win.runner = desktop_board::run::Runner::new(30);
    // console-core edit (shell round): forget the presses latched by the
    // key tests above. The console now reacts to them (a press skips the
    // boot screen), so the plain run below must start from the same input.
    win.controls = desktop_board::controls::Controls::new();
    win.advance(&mut board, 6, |b| console.step(b));
    let (w, _, shown) = win.read_back(&board).unwrap();
    let mut console2 = Box::new(Console::new());
    let mut plain = Box::new(DesktopBoard::new(None));
    let run = ScriptRun {
        frames: 6,
        fps: 30,
        events: Vec::new(),
    };
    run_script(&run, &mut plain, &mut *console2, |_, _, _| {});
    assert_eq!(
        picture::fingerprint(&board.screen.image),
        picture::fingerprint(&plain.screen.image)
    );
    assert_eq!(wrong_pixels(&shown, w, &plain.screen.image), 0);

    // 7b. The backlight dims the window, and only the window: the picture
    //     (screenshots, fingerprints) stays the console's own.
    assert_eq!(board.screen.backlight, Some(255));
    let print = picture::fingerprint(&board.screen.image);
    board.screen.backlight(16);
    let (_, _, dim) = win.read_back(&board).unwrap();
    assert_eq!(picture::fingerprint(&board.screen.image), print);
    let m = i32::from(window_dim(Some(16)));
    let mut darker = 0;
    for (d, f) in dim.iter().zip(&shown) {
        let want = i32::from(*f) * m / 255;
        assert!((i32::from(*d) - want).abs() <= 2, "{d} vs {f} x {m}/255");
        darker += usize::from(d < f);
    }
    assert!(darker > 1000, "the window is darker");
    board.screen.backlight(255);
    let (_, _, full) = win.read_back(&board).unwrap();
    assert_eq!(full, shown);

    // 8. Esc closes: `run` returns at once.
    queue.push_event(key_event(Scancode::Escape, true)).unwrap();
    let report = win.run(&mut board, |b| console.step(b));
    assert!(report.starts_with("window:"), "{report}");
}

fn board_touch(win: &SimWindow) -> Option<(u16, u16)> {
    win.controls.touch.map(|t| (t.x, t.y))
}

/// Window pixels (8-bit RGB, 2x) that differ from the panel picture.
fn wrong_pixels(shown: &[u8], w: usize, panel: &[u16]) -> usize {
    let mut wrong = 0;
    for (i, rgb) in shown.as_chunks::<3>().0.iter().enumerate() {
        let (x, y) = (i % w, i / w);
        let want = picture::to_rgb888(panel[(y / 2) * usize::from(PANEL_W) + x / 2]);
        if *rgb != want {
            wrong += 1;
        }
    }
    wrong
}
