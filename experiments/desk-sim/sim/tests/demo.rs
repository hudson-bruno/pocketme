//! Tests with no window: the demo runs on the no-window board, a script
//! gives the input, and the pictures are checked pixel by pixel.

use board_layer::{Board, SCREEN_H, SCREEN_W, Stick, button};
use console_demo::{Demo, Scene, colour, layout, stick_dot, trigger_fill};
use desk_sim::headless::HeadlessBoard;
use desk_sim::{picture, script};

/// Runs the demo for `frames` frames with `script`, keeping every picture.
fn run(frames: u32, script_text: &str) -> (HeadlessBoard, Demo) {
    let mut board = HeadlessBoard::new(frames, 30, script::parse(script_text).unwrap());
    for frame in 0..frames {
        board.keep(frame);
    }
    let mut demo = Demo::new();
    while let Some(tick) = board.wait_frame() {
        demo.frame(&mut board, tick);
    }
    (board, demo)
}

fn pixel(board: &HeadlessBoard, frame: u32, (x, y): (i32, i32)) -> u16 {
    board.pictures[&frame][y as usize * SCREEN_W + x as usize]
}

fn logged(board: &HeadlessBoard, frame: u32, line: &str) -> bool {
    board.log.iter().any(|(f, l)| *f == frame && l == line)
}

/// When a picture is not what was expected, write it (and a picture of
/// the difference) next to the test build, and say where.
fn explain(name: &str, actual: &[u16], expected: Option<&[u16]>) -> String {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let path = dir.join(format!("{name}-actual.png"));
    picture::save_png(&path, actual, SCREEN_W, SCREEN_H).unwrap();
    let mut note = format!("actual picture: {}", path.display());
    if let Some(expected) = expected {
        let (count, diff) = picture::difference(actual, expected);
        let diff_path = dir.join(format!("{name}-difference.png"));
        picture::save_png(&diff_path, &diff, SCREEN_W, SCREEN_H).unwrap();
        note += &format!(
            "; {count} pixels differ, shown in magenta in {}",
            diff_path.display()
        );
    }
    note
}

const TOUR: &str = include_str!("../../scripts/tour.txt");

/// The expected fingerprints of the tour's pictures. When the demo's
/// drawing changes on purpose, run the tour (see README.md), look at the
/// new PNG files, and copy the new fingerprints here.
const TOUR_FINGERPRINTS: [(u32, u64); 3] = [
    (5, 0x1ab5_4c97_9a1f_026b),   // start screen
    (64, 0x405a_5ea6_bae6_9eb9),  // everything held
    (110, 0x79d7_48f8_f6b8_68f1), // back at the start screen
];

#[test]
fn same_pictures_on_every_run() {
    let (a, _) = run(120, TOUR);
    let (b, _) = run(120, TOUR);
    assert_eq!(a.fingerprints.len(), 120);
    assert_eq!(a.fingerprints, b.fingerprints);
    for (frame, expected) in TOUR_FINGERPRINTS {
        let actual = a.fingerprints[&frame];
        assert!(
            actual == expected,
            "frame {frame}: fingerprint {actual:016x}, expected {expected:016x}; {}",
            explain(&format!("tour-{frame}"), &a.pictures[&frame], None)
        );
    }
}

#[test]
fn test_cards_come_out_exactly_as_test_card_draws_them() {
    let (board, demo) = run(120, TOUR);
    assert_eq!(demo.scene, Scene::Start);

    // Frame 75: the panel card, drawn by test-card into a fresh buffer.
    let mut expected = vec![0u16; SCREEN_W * SCREEN_H];
    test_card::draw_panel_card(&mut test_card::PanelBuf::new(&mut expected)).unwrap();
    let actual = &board.pictures[&75];
    assert!(
        actual == &expected,
        "{}",
        explain("panel-card", actual, Some(&expected))
    );
    let corners = [(0, 0), (319, 0), (0, 239), (319, 239)].map(|p| pixel(&board, 75, p));
    assert_eq!(corners, test_card::CORNERS);

    // Frame 85: the 160x120 card, each pixel a 2x2 block of its palette
    // colour.
    let mut fb = vec![0u8; test_card::FB_W * test_card::FB_H];
    test_card::draw_fb_card(&mut test_card::Fb::new(&mut fb)).unwrap();
    let expected: Vec<u16> = (0..SCREEN_W * SCREEN_H)
        .map(|i| {
            let (x, y) = (i % SCREEN_W, i / SCREEN_W);
            test_card::PALETTE[fb[(y / 2) * test_card::FB_W + x / 2] as usize]
        })
        .collect();
    let actual = &board.pictures[&85];
    assert!(
        actual == &expected,
        "{}",
        explain("fb-card", actual, Some(&expected))
    );
    let corners = [(0, 0), (319, 0), (0, 239), (319, 239)].map(|p| pixel(&board, 85, p));
    assert_eq!(
        corners,
        test_card::FB_CORNERS.map(|i| test_card::PALETTE[i as usize])
    );
}

/// START on frame 0 goes to Play; then each button is held on frame 3.
#[test]
fn every_button_reaches_the_demo() {
    for (index, &(name, bit)) in button::NAMES.iter().enumerate() {
        let (board, demo) = run(
            5,
            &format!("0 press start\n1 release start\n3 press {name}"),
        );
        assert!(
            logged(&board, 3, &format!("pad: {name} down")),
            "{name}: no log line"
        );
        let lamp = (layout::lamp_x(index), layout::LAMP_Y);
        match bit {
            // These two leave the Play screen, so look at the scene.
            button::SELECT => assert_eq!(demo.scene, Scene::PanelCard),
            button::HOME => assert_eq!(demo.scene, Scene::Start),
            _ => {
                assert_eq!(demo.scene, Scene::Play, "{name}");
                assert_eq!(
                    pixel(&board, 2, lamp),
                    colour::LAMP_OFF,
                    "{name} lamp before"
                );
                assert_eq!(
                    pixel(&board, 3, lamp),
                    colour::LAMP_ON,
                    "{name} lamp while held"
                );
            }
        }
    }
}

#[test]
fn d_pad_and_left_stick_move_the_character() {
    let (x0, y0) = layout::HERO_START;
    // 10 frames of right on the d-pad: 2 pixels a frame.
    let (_, demo) = run(13, "0 press start\n3 press right");
    assert_eq!(demo.hero, (x0 + 20, y0));
    // Down on the d-pad.
    let (_, demo) = run(13, "0 press start\n3 press down");
    assert_eq!(demo.hero, (x0, y0 + 20));
    // Left stick half left and fully down: -64/32 = -2 and 127/32 = 3.
    let (board, demo) = run(13, "0 press start\n3 stick left -64 127");
    assert_eq!(demo.hero, (x0 - 20, y0 + 30));
    let dot = stick_dot(layout::LEFT_STICK_BOX, Stick { x: -64, y: 127 });
    assert_eq!(pixel(&board, 12, dot), colour::STICK_DOT);
    // A stick barely off the middle does nothing (the dead zone).
    let (_, demo) = run(13, "0 press start\n3 stick left 10 -10");
    assert_eq!(demo.hero, (x0, y0));
    // The character stays inside the play field.
    let (_, demo) = run(200, "0 press start\n3 press left\n3 press up");
    let (fx, fy, _, _) = layout::FIELD;
    assert_eq!(demo.hero, (fx + 2, fy + 2));
}

#[test]
fn right_stick_and_triggers_show() {
    let (board, _) = run(
        5,
        "0 press start\n3 stick right 127 -128\n3 trigger left 128\n3 trigger right 255",
    );
    let dot = stick_dot(layout::RIGHT_STICK_BOX, Stick { x: 127, y: -128 });
    assert_eq!(pixel(&board, 2, dot), colour::BACKGROUND);
    assert_eq!(pixel(&board, 3, dot), colour::STICK_DOT);
    for (bar, pull) in [
        (layout::LEFT_TRIGGER_BAR, 128u8),
        (layout::RIGHT_TRIGGER_BAR, 255),
    ] {
        let bottom = bar.1 + layout::BOX - 2; // the lowest row inside the bar
        let top_filled = bottom - trigger_fill(pull) + 1;
        assert_eq!(
            pixel(&board, 2, (bar.0 + 3, bottom)),
            colour::BACKGROUND,
            "{pull}"
        );
        assert_eq!(
            pixel(&board, 3, (bar.0 + 3, bottom)),
            colour::TRIGGER,
            "{pull}"
        );
        assert_eq!(
            pixel(&board, 3, (bar.0 + 3, top_filled)),
            colour::TRIGGER,
            "{pull}"
        );
        assert_ne!(
            pixel(&board, 3, (bar.0 + 3, top_filled - 1)),
            colour::TRIGGER,
            "{pull}"
        );
    }
}

#[test]
fn touch_reaches_the_demo() {
    // A tap leaves the start screen; the mark shows where the finger is.
    let (board, demo) = run(6, "2 touch 200 100\n4 lift");
    assert_eq!(demo.scene, Scene::Play);
    assert!(logged(&board, 2, "touch: down at 200,100"));
    assert!(logged(&board, 4, "touch: up at 200,100"));
    for frame in [2, 3] {
        assert_eq!(pixel(&board, frame, (200, 100)), colour::TOUCH_MARK);
        assert_eq!(pixel(&board, frame, (208, 100)), colour::TOUCH_MARK);
        assert_eq!(pixel(&board, frame, (200, 92)), colour::TOUCH_MARK);
    }
    assert_ne!(pixel(&board, 4, (208, 100)), colour::TOUCH_MARK);
    // The corners of the screen are reachable.
    let (board, _) = run(4, "0 press start\n3 touch 319 239");
    assert_eq!(pixel(&board, 3, (319, 239)), colour::TOUCH_MARK);
}

#[test]
fn the_clock_moves_one_frame_at_a_time() {
    let mut board = HeadlessBoard::new(61, 60, Vec::new());
    let mut ticks = Vec::new();
    while let Some(tick) = board.wait_frame() {
        assert_eq!(board.now_us(), tick.now_us);
        ticks.push(tick);
    }
    assert_eq!(ticks.len(), 61);
    assert_eq!((ticks[0].frame, ticks[0].now_us), (0, 0));
    assert_eq!((ticks[60].frame, ticks[60].now_us), (60, 1_000_000));
    // The frame number and the time are drawn at the top of Play.
    let (board, _) = run(31, "0 press start");
    let (other, _) = run(31, "0 press start");
    assert_eq!(board.pictures[&30], other.pictures[&30]);
    assert_ne!(board.fingerprints[&29], board.fingerprints[&30]);
}

/// A press and a release on the same frame must not be lost: the demo
/// sees the button for that one frame (as with a quick key in the
/// window). The same for a touch and a lift on the same frame.
#[test]
fn press_and_release_in_one_frame_still_counts() {
    let a = (layout::lamp_x(4), layout::LAMP_Y); // lamp 4 is A
    let (board, demo) = run(
        8,
        "0 press start\n1 release start\n3 press a\n3 release a\n5 touch 10 20\n5 lift",
    );
    assert!(logged(&board, 3, "pad: a down"));
    assert!(logged(&board, 4, "pad: a up"));
    assert_eq!(pixel(&board, 3, a), colour::LAMP_ON);
    assert_eq!(pixel(&board, 4, a), colour::LAMP_OFF);
    assert!(logged(&board, 5, "touch: down at 10,20"));
    assert!(logged(&board, 6, "touch: up at 10,20"));
    assert_eq!(pixel(&board, 5, (10, 20)), colour::TOUCH_MARK);
    assert_ne!(pixel(&board, 6, (10, 20)), colour::TOUCH_MARK);
    assert_eq!(demo.scene, Scene::Play);
    // A tap that lasts less than a frame also leaves the start screen.
    let (_, demo) = run(3, "1 touch 5 5\n1 lift");
    assert_eq!(demo.scene, Scene::Play);
}

/// Two buttons on the same frame both arrive.
#[test]
fn two_buttons_in_one_frame_both_arrive() {
    let (board, _) = run(5, "0 press start\n1 release start\n3 press a\n3 press b");
    for (index, name) in [(4, "a"), (5, "b")] {
        assert!(logged(&board, 3, &format!("pad: {name} down")));
        let lamp = (layout::lamp_x(index), layout::LAMP_Y);
        assert_eq!(pixel(&board, 3, lamp), colour::LAMP_ON, "{name}");
    }
}

/// Script lines after the last frame are reported, not silently dropped.
#[test]
fn unused_script_lines_are_reported() {
    let events = script::parse("0 press start\n500 press a\n600 lift").unwrap();
    let mut board = HeadlessBoard::new(10, 30, events);
    console_demo::run(&mut board);
    assert_eq!(board.unused_events(), Some((2, 500)));
    // A script that ends early is fine: nothing is left over.
    let mut board = HeadlessBoard::new(10, 30, script::parse("2 press a").unwrap());
    console_demo::run(&mut board);
    assert_eq!(board.unused_events(), None);
}

/// The start screen's words blink every half second of time, at 30 and
/// at 60 frames a second alike.
#[test]
fn start_screen_blinks_by_time_not_by_frames() {
    let mut board = HeadlessBoard::new(31, 60, Vec::new());
    console_demo::run(&mut board);
    let f = &board.fingerprints;
    // 0.483 s: still the first half second, so the words still show.
    assert_eq!(f[&0], f[&29]);
    // 0.5 s: the words are gone.
    assert_ne!(f[&29], f[&30]);
}

#[test]
fn shot_lines_in_a_script_keep_the_picture() {
    let mut board = HeadlessBoard::new(10, 30, script::parse("4 shot\n7 shot").unwrap());
    console_demo::run(&mut board);
    assert_eq!(board.pictures.keys().copied().collect::<Vec<_>>(), [4, 7]);
    assert_eq!(board.screen_now().len(), SCREEN_W * SCREEN_H);
    assert_eq!(SCREEN_H, 240);
}

/// The board layer and the demo must not use the PC's standard library,
/// so they can be built for the ESP32. (`check-no-std.sh` builds them
/// for real targets with no standard library at all.)
#[test]
fn board_layer_and_demo_stay_free_of_std() {
    for (name, source) in [
        ("board/src/lib.rs", include_str!("../../board/src/lib.rs")),
        ("demo/src/lib.rs", include_str!("../../demo/src/lib.rs")),
    ] {
        assert!(
            source.lines().any(|l| l.trim() == "#![no_std]"),
            "{name} lost #![no_std]"
        );
        // Only the tests at the end may use std.
        let code = source.split("#[cfg(test)]").next().unwrap();
        for bad in ["extern crate std", "std::", "extern crate alloc", "alloc::"] {
            assert!(!code.contains(bad), "{name} uses `{bad}`");
        }
    }
}
