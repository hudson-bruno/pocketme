//! The desktop board's parts, with no window and no console: the screen,
//! the controls (every key and mouse action), the card folder, the loop.

use std::path::Path;

use board_api::{
    ByteOrder, Dir, DirEntry, Files, FsError, Keys, Pad, PadState, PixelSource, Rect, Screen,
    Stick, Touch, Wake, btn, key,
};
use desktop_board::SAMPLE_CARD;
use desktop_board::board::{AREA_COMMAND_BYTES, DesktopBoard, MAX_AREAS, PANEL_H, PANEL_W};
use desktop_board::controls::{Controls, window_to_panel};
use desktop_board::keymap::{self, Control, Side, Toward};
use desktop_board::run::{MAX_STEPS_PER_TICK, Runner, ScriptRun, run_script};
use desktop_board::script;

/// A picture source with one flat colour.
struct Flat(u16);

impl PixelSource for Flat {
    fn run(&self, _x: u16, _y: u16, _dir: Dir, order: ByteOrder, out: &mut [u16]) {
        out.fill(order.apply(self.0));
    }
}

/// A picture source where every pixel is different: x in the red and
/// green bits, y in the blue and green bits.
struct Pattern;

impl Pattern {
    fn at(x: u16, y: u16) -> u16 {
        ((x & 0x1F) << 11) | ((x.wrapping_add(y) & 0x3F) << 5) | (y & 0x1F)
    }
}

impl PixelSource for Pattern {
    fn run(&self, x: u16, y: u16, dir: Dir, order: ByteOrder, out: &mut [u16]) {
        for (i, p) in out.iter_mut().enumerate() {
            let i = i as u16;
            let (px, py) = match dir {
                Dir::Right => (x + i, y),
                Dir::Left => (x - i, y),
                Dir::Down => (x, y + i),
                Dir::Up => (x, y - i),
            };
            *p = order.apply(Pattern::at(px, py));
        }
    }
}

fn board() -> Box<DesktopBoard> {
    Box::new(DesktopBoard::new(None))
}

#[test]
fn the_screen_keeps_stale_pixels_outside_the_presented_areas() {
    let mut b = board();
    let all = Rect::new(0, 0, PANEL_W, PANEL_H);
    b.screen.present(&Flat(0x1234), &[all]);
    let area = Rect::new(10, 20, 30, 40);
    let r = b.screen.present(&Pattern, &[area]);
    for y in 0..PANEL_H {
        for x in 0..PANEL_W {
            let want = if area.contains(x, y) {
                Pattern::at(x, y)
            } else {
                0x1234 // what the panel showed before: not sent again
            };
            assert_eq!(b.screen.pixel(x, y), want, "pixel ({x}, {y})");
        }
    }
    assert_eq!(r.areas, 1);
    assert_eq!(r.pixels, 30 * 40);
    assert_eq!(b.screen.last_bytes, 30 * 40 * 2 + AREA_COMMAND_BYTES);
    assert_eq!(b.screen.last_areas, [area]);
    assert_eq!(b.screen.presents, 2);
    assert!(b.take_problems().is_empty());
}

#[test]
fn the_screen_reports_what_the_board_would_refuse() {
    let mut b = board();
    let too_many = vec![Rect::new(0, 0, 1, 1); MAX_AREAS + 1];
    b.screen.present(&Flat(1), &too_many);
    let r = b.screen.present(&Flat(2), &[Rect::new(300, 230, 40, 40)]);
    assert_eq!(r.pixels, 20 * 10, "only the part inside the panel is sent");
    assert_eq!(b.screen.pixel(319, 239), 2);
    let problems = b.take_problems();
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(problems[0].contains("areas in one present"));
    assert!(problems[1].contains("outside"));
    // A full picture fits in one frame at 40 MHz (30.96 ms < 33.3 ms).
    b.screen
        .present(&Flat(3), &[Rect::new(0, 0, PANEL_W, PANEL_H)]);
    assert_eq!(b.screen.over_budget, 0);
}

#[test]
fn every_player_2_key_reaches_pad_2_only() {
    for &(name, control, _) in keymap::PLAYER2_KEYS {
        let mut c = Controls::new();
        let mut b = board();
        c.set_connected_by(1, true);
        c.press_by(1, control);
        c.apply(&mut b);
        let p2 = b.pad.others[0].state;
        assert!(p2.connected, "key {name}");
        match control {
            Control::Button(bit) => assert_eq!(p2.buttons, bit, "key {name}"),
            Control::Trigger(Side::Left) => assert_eq!((p2.buttons, p2.lt), (btn::ZL, 1023)),
            Control::Trigger(Side::Right) => assert_eq!((p2.buttons, p2.rt), (btn::ZR, 1023)),
            other => panic!("{other:?} on player 2's keys"),
        }
        assert_eq!(b.pad.others[0].pressed, p2.buttons, "key {name}: noted");
        assert_eq!(b.pad.state.buttons, 0, "key {name}: not player 1's");
        assert_eq!(b.pad.pressed, 0, "key {name}: not player 1's");
        for o in &b.pad.others[1..] {
            assert_eq!(o.state, PadState::NONE, "key {name}: pads 3 and 4 stay out");
        }
        assert_eq!(
            b.pad.others[0].motion, None,
            "player 2 starts with motion off"
        );
    }
}

#[test]
fn pads_2_to_4_start_unplugged_and_say_nothing() {
    let mut c = Controls::new();
    let mut b = board();
    // A press on a pad that is not plugged in reaches nothing.
    c.press_by(2, Control::Button(btn::SOUTH));
    c.apply(&mut b);
    assert!(b.pad.state.connected);
    for o in &b.pad.others {
        assert_eq!(o.state, PadState::NONE);
        assert_eq!(o.pressed, 0);
    }
    // Plugged in, it holds the button (pressed again for the note).
    c.set_connected_by(2, true);
    c.press_by(2, Control::Button(btn::SOUTH));
    c.apply(&mut b);
    assert_eq!(b.pad.others[1].state.buttons, btn::SOUTH);
    assert_eq!(b.pad.state.buttons, 0);
}

#[test]
fn every_key_reaches_the_board() {
    for &(name, control, _) in keymap::KEYS {
        let mut c = Controls::new();
        let mut b = board();
        c.press(control);
        c.apply(&mut b);
        let pad = b.pad.state;
        match control {
            Control::Button(bit) => assert_eq!(pad.buttons, bit, "key {name}"),
            Control::Boot => {
                assert_eq!(b.keys.held, key::BOOT, "key {name}");
                assert_eq!(pad.buttons, 0);
            }
            Control::Trigger(side) => {
                let (bit, value) = match side {
                    Side::Left => (btn::ZL, pad.lt),
                    Side::Right => (btn::ZR, pad.rt),
                };
                assert_eq!(pad.buttons, bit, "key {name}");
                assert_eq!(value, 1023, "key {name}");
            }
            Control::Stick(side, toward) => {
                let s = match side {
                    Side::Left => pad.left,
                    Side::Right => pad.right,
                };
                let want = match toward {
                    Toward::Up => Stick { x: 0, y: -32767 },
                    Toward::Down => Stick { x: 0, y: 32767 },
                    Toward::Left => Stick { x: -32767, y: 0 },
                    Toward::Right => Stick { x: 32767, y: 0 },
                };
                assert_eq!(s, want, "key {name}");
            }
            // The motion stand-in: no button, only the motion sensor.
            Control::MotionSwitch => {
                assert_eq!(pad.buttons, 0, "key {name}");
                assert_eq!(b.pad.motion, None, "key {name}: motion off");
            }
            Control::Tilt => {
                assert_eq!(pad.buttons, 0, "key {name}");
                let m = b.pad.motion.expect("motion on");
                assert_eq!(
                    m.accel,
                    [0, 0, 4096],
                    "key {name}: flat until the mouse moves"
                );
            }
        }
        // After a step and the key let go, nothing is left held.
        c.stepped();
        c.release(control);
        c.apply(&mut b);
        assert_eq!(b.pad.state.buttons, 0, "key {name}");
        assert_eq!(b.keys.held, 0, "key {name}");
        assert_eq!(b.pad.state.left, Stick::default(), "key {name}");
        assert_eq!(b.pad.state.right, Stick::default(), "key {name}");
        assert_eq!((b.pad.state.lt, b.pad.state.rt), (0, 0), "key {name}");
    }
}

#[test]
fn two_stick_keys_together_and_opposite() {
    let mut c = Controls::new();
    c.press(Control::Stick(Side::Left, Toward::Up));
    c.press(Control::Stick(Side::Left, Toward::Right));
    assert_eq!(
        c.pad().left,
        Stick {
            x: 32767,
            y: -32767
        }
    );
    c.press(Control::Stick(Side::Left, Toward::Left));
    assert_eq!(c.pad().left.x, 0, "left and right together cancel");
    assert_eq!(c.pad().right, Stick::default());
}

#[test]
fn the_mouse_reaches_the_board() {
    // The window is 3 times the panel: window pixel (300, 150) is panel
    // pixel (100, 50). Outside the window sticks to the edge.
    assert_eq!(window_to_panel(300.0, 150.0, 3), tp(100, 50));
    assert_eq!(window_to_panel(-5.0, 2000.0, 3), tp(0, 239));
    assert_eq!(window_to_panel(959.9, 719.9, 3), tp(319, 239));
    let mut c = Controls::new();
    let mut b = board();
    c.touch(window_to_panel(300.0, 150.0, 3));
    c.apply(&mut b);
    assert_eq!(b.touch.at, Some(tp(100, 50)));
    c.touch(tp(101, 51)); // dragging
    c.stepped();
    c.apply(&mut b);
    assert_eq!(b.touch.at, Some(tp(101, 51)));
    c.lift();
    c.apply(&mut b);
    assert_eq!(b.touch.at, None);
}

fn tp(x: u16, y: u16) -> board_api::TouchPoint {
    board_api::TouchPoint { x, y }
}

#[test]
fn a_press_shorter_than_a_step_is_still_seen_once() {
    let mut c = Controls::new();
    let mut b = board();
    c.press(Control::Button(btn::SOUTH));
    c.release(Control::Button(btn::SOUTH));
    c.press(Control::Boot);
    c.release(Control::Boot);
    c.touch(tp(5, 6));
    c.lift();
    c.apply(&mut b);
    // Now: nothing held, nothing touched. That is the truth.
    assert_eq!(b.pad.state.buttons, 0);
    assert_eq!(b.keys.held, 0);
    assert_eq!(b.touch.at, None);
    // But the board noted the presses and the touch, until taken.
    c.stepped();
    c.apply(&mut b); // a step that read no input: the notes stay
    assert_eq!(Pad::take_pressed(&mut b.pad, 0), btn::SOUTH);
    assert_eq!(Keys::take_pressed(&mut b.keys), key::BOOT);
    assert_eq!(b.touch.take_tap(), Some(tp(5, 6)));
    // Once.
    c.apply(&mut b);
    assert_eq!(Pad::take_pressed(&mut b.pad, 0), 0);
    assert_eq!(Keys::take_pressed(&mut b.keys), 0);
    assert_eq!(b.touch.take_tap(), None);
}

#[test]
fn a_pad_pulled_out_says_nothing() {
    let mut c = Controls::new();
    let mut b = board();
    c.apply(&mut b);
    assert!(b.pad.state.connected, "the keyboard's pad is plugged in");
    c.press(Control::Button(btn::NORTH));
    c.set_connected(false);
    c.apply(&mut b);
    assert_eq!(b.pad.state, PadState::NONE);
}

#[test]
fn the_card_folder_lists_the_sample_games() {
    let mut b = Box::new(DesktopBoard::new(Some(Path::new(SAMPLE_CARD))));
    let mut games = Vec::new();
    b.files
        .list("/CONSOLE/GAMES", |e: &DirEntry<'_>| {
            assert!(e.is_dir, "{} should be a game folder", e.name);
            games.push(e.name.to_string());
            true
        })
        .unwrap();
    assert_eq!(
        games,
        [
            "BENCH", "CAMBER", "DRIVE3D", "HAIRPIN", "KESTREL", "LANDER", "LANTERN", "MERCATOR",
            "PADTEST", "PROTO3D", "ROOFTOP", "SKIMMER", "STARDUST", "TUNDRA",
        ]
    );

    for g in &games {
        // INFO.TXT names the game.
        let info = read_all(&mut b, &format!("/CONSOLE/GAMES/{g}/INFO.TXT"));
        let info = String::from_utf8(info).unwrap();
        assert!(info.lines().any(|l| l == format!("NAME={g}")), "{info}");
        // GAME.BIN is a short note, never a game: the games are linked
        // into the simulator and get their chip file from
        // games/build-chip.sh (see tools/make_sample_card.py). Every
        // folder on the sample card is a linked game.
        let bin = read_all(&mut b, &format!("/CONSOLE/GAMES/{g}/GAME.BIN"));
        assert!(bin.starts_with(b"NOT A GAME FILE."), "{g}");
        let linked = desktop_board::games::GAMES.iter().any(|l| l.folder == g);
        assert!(linked, "{g}: on the sample card but not linked in");
    }

    // Reading part of a file, from an offset.
    let mut buf = [0u8; 4];
    let n = b
        .files
        .read_at("/CONSOLE/GAMES/LANDER/GAME.BIN", 4, &mut buf)
        .unwrap();
    assert_eq!((n, &buf), (4, b"A GA"));
    let size = b.files.size("/CONSOLE/GAMES/LANDER/GAME.BIN").unwrap();
    let n = b
        .files
        .read_at("/CONSOLE/GAMES/LANDER/GAME.BIN", size - 2, &mut buf)
        .unwrap();
    assert_eq!(n, 2, "only what is left of the file");
}

/// A whole file from the card, read in pieces of 512 bytes (a card
/// sector), as the console would.
fn read_all(b: &mut DesktopBoard, path: &str) -> Vec<u8> {
    let size = b.files.size(path).unwrap();
    let mut out = Vec::new();
    let mut buf = [0u8; 512];
    while (out.len() as u32) < size {
        let n = b.files.read_at(path, out.len() as u32, &mut buf).unwrap();
        assert!(n > 0);
        out.extend_from_slice(&buf[..n]);
    }
    out
}

#[test]
fn the_card_keeps_its_rules() {
    let mut b = Box::new(DesktopBoard::new(Some(Path::new(SAMPLE_CARD))));
    let f = &mut b.files;
    let mut buf = [0u8; 4];
    assert_eq!(
        f.read_at("/console/games/comet/game.bin", 0, &mut buf),
        Err(FsError::BadName),
        "small letters"
    );
    assert_eq!(f.size("CONSOLE"), Err(FsError::BadName), "not from /");
    assert_eq!(
        f.size("/CONSOLE/GAMES/LONGERNAME"),
        Err(FsError::BadName),
        "9 letters"
    );
    assert_eq!(f.size("/CONSOLE/NOTHERE.BIN"), Err(FsError::NotFound));
    assert_eq!(
        f.read_at("/CONSOLE/GAMES", 0, &mut buf),
        Err(FsError::NotFound),
        "a folder is not a file"
    );
    let long = format!("/{}", "A/".repeat(40));
    assert_eq!(f.size(&long), Err(FsError::BadName), "path too long");
    // Pull the card out, then put it back.
    f.set_card(false);
    assert_eq!(f.list("/", |_| true), Err(FsError::NoCard));
    f.set_card(true);
    assert_eq!(f.list("/", |_| true), Ok(()));
    // No card at all.
    let mut empty = board();
    assert_eq!(empty.files.size("/A"), Err(FsError::NoCard));
}

#[test]
fn names_the_card_cannot_hold_are_skipped_with_a_note() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("odd-card");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["GOOD.TXT", "readme.md", "A VERY LONG NAME.TXT"] {
        std::fs::write(dir.join(name), "x").unwrap();
    }
    let mut b = Box::new(DesktopBoard::new(Some(&dir)));
    let mut seen = Vec::new();
    b.files
        .list("/", |e| {
            seen.push(e.name.to_string());
            true
        })
        .unwrap();
    assert_eq!(seen, ["GOOD.TXT"]);
    b.files.skipped.sort();
    assert_eq!(b.files.skipped, ["A VERY LONG NAME.TXT", "readme.md"]);
}

#[test]
fn the_loop_calls_the_console_when_it_asked() {
    let mut b = board();
    let mut c = Controls::new();
    let mut r = Runner::new(30);
    let mut times = Vec::new();
    // A console that wants 3 steps a tick, 10 ms apart, then sleeps
    // until input.
    let mut step = |b: &mut DesktopBoard| {
        let now = b.clock.now_us;
        times.push(now);
        if times.len() < 3 {
            Wake::At(now + 10_000)
        } else {
            Wake::InputOr(u64::MAX)
        }
    };
    let info = r.run_tick(&mut b, &mut c, &mut step);
    assert_eq!(info.steps, 3);
    for _ in 0..5 {
        assert_eq!(r.run_tick(&mut b, &mut c, &mut step).steps, 0, "asleep");
    }
    c.press(Control::Button(btn::EAST));
    let info = r.run_tick(&mut b, &mut c, &mut step);
    assert_eq!((info.frame, info.steps), (6, 1), "woken by input");
    assert_eq!(times, [0, 10_000, 20_000, 6 * 33_333]);
    assert!(r.problems.is_empty());
}

#[test]
fn the_loop_reports_a_stuck_console_and_double_sends() {
    let mut b = board();
    let mut c = Controls::new();
    let mut r = Runner::new(30);
    r.run_tick(&mut b, &mut c, |b: &mut DesktopBoard| {
        Wake::At(b.clock.now_us)
    });
    assert!(r.problems[0].contains("at once"), "{:?}", r.problems);
    let mut r = Runner::new(30);
    r.run_tick(&mut b, &mut c, |b: &mut DesktopBoard| {
        let all = [Rect::new(0, 0, 4, 4)];
        b.screen.present(&Flat(1), &all);
        b.screen.present(&Flat(2), &all);
        Wake::At(b.clock.now_us + 50_000)
    });
    assert_eq!(r.problems.len(), 1);
    assert!(r.problems[0].contains("2 presents in one step"));
    assert_eq!(MAX_STEPS_PER_TICK, 8);
}

#[test]
fn a_script_reaches_the_board_on_its_frames() {
    let events = script::parse(
        "2 press west\n2 release west\n3 touch 10 20\n4 stick right 5 -6\n\
         5 trigger left 300\n6 press boot\n7 card out\n8 pad off\n9 shot",
    )
    .unwrap();
    let run = ScriptRun {
        frames: 10,
        fps: 30,
        events,
    };
    let mut b = Box::new(DesktopBoard::new(Some(Path::new(SAMPLE_CARD))));
    let mut seen = Vec::new();
    let problems = run_script(
        &run,
        &mut b,
        |b: &mut DesktopBoard| {
            seen.push((
                b.pad.state,
                b.touch.at,
                b.keys.held,
                b.files.root.is_some(),
                b.clock.now_us,
                // What the board noted (taken, as the console does).
                Pad::take_pressed(&mut b.pad, 0),
            ));
            Wake::At(b.clock.now_us + 33_333)
        },
        |_, _, _| {},
    );
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(seen.len(), 10);
    // A press and release on the same frame: not held when the step
    // looks, but noted, and handed out once.
    assert_eq!(seen[2].0.buttons, 0);
    assert_eq!(seen[2].5, btn::WEST, "a same-frame tap is seen");
    assert_eq!(seen[3].0.buttons, 0);
    assert_eq!(seen[3].5, 0);
    assert_eq!(seen[3].1, Some(tp(10, 20)));
    assert_eq!(seen[4].0.right, Stick { x: 5, y: -6 });
    assert_eq!(seen[5].0.lt, 300);
    assert_eq!(seen[6].2, key::BOOT);
    assert!(seen[6].3 && !seen[7].3, "the card came out on frame 7");
    assert!(seen[7].0.connected && !seen[8].0.connected);
    assert_eq!(seen[9].4, 9 * 33_333, "frame 9 starts at 9 x 33,333 us");
}

#[test]
fn script_lines_after_the_end_are_a_problem() {
    let run = ScriptRun {
        frames: 5,
        fps: 30,
        events: script::parse("1 press south\n7 lift\n9 lift").unwrap(),
    };
    let mut b = board();
    let problems = run_script(
        &run,
        &mut b,
        |_: &mut DesktopBoard| Wake::InputOr(u64::MAX),
        |_, _, _| {},
    );
    assert_eq!(problems.len(), 1);
    assert!(
        problems[0].contains("2 script line(s) never ran"),
        "{problems:?}"
    );
    assert!(problems[0].contains("--frames 8"), "{problems:?}");
}

// ---- Backlight (step 4) ------------------------------------------------------

#[test]
fn the_screen_keeps_the_backlight_and_the_window_dims_by_it() {
    use desktop_board::board::window_dim;
    let mut b = board();
    assert_eq!(b.screen.backlight, None);
    b.screen.backlight(42);
    b.screen.backlight(255);
    assert_eq!(
        (b.screen.backlight, b.screen.backlight_calls),
        (Some(255), 2)
    );
    // Full, and before the first call: the picture as it is.
    assert_eq!(window_dim(None), 255);
    assert_eq!(window_dim(Some(255)), 255);
    assert_eq!(window_dim(Some(0)), 0);
    // The light the window gives (its colour factor to the power 2.2)
    // is the backlight's share of full light, within a few percent.
    let mut last = 0;
    for v in 1..=255u8 {
        let m = window_dim(Some(v));
        assert!(m >= last, "darker at a higher value: {v}");
        last = m;
        let light = (f64::from(m) / 255.0).powf(2.2);
        let want = f64::from(v) / 255.0;
        assert!(
            (light - want).abs() <= 0.02 + want * 0.03,
            "{v}: {light} vs {want}"
        );
    }
}

#[test]
fn the_console_lights_the_screen_once_after_its_first_picture() {
    let mut console = Box::new(console_core::Console::new());
    // Not about the boot: no boot animation (feature `boot-gather`).
    desktop_board::settings::set_boot(&mut console, desktop_board::settings::Boot::Off);
    let mut b = board();
    desktop_board::run::run_frames(&mut console, &mut b, 1);
    assert_eq!(b.screen.presents, 1);
    assert_eq!(
        (b.screen.backlight, b.screen.backlight_calls),
        (Some(255), 1)
    );
    desktop_board::run::run_frames(&mut console, &mut b, 60);
    assert_eq!(b.screen.backlight_calls, 1);
}

#[test]
fn the_backlight_script_word_checks_the_value() {
    let run = |text: &str| {
        let mut console = Box::new(console_core::Console::new());
        // Not about the boot: no boot animation (feature `boot-gather`).
        desktop_board::settings::set_boot(&mut console, desktop_board::settings::Boot::Off);
        let mut b = board();
        let run = ScriptRun {
            frames: 80,
            fps: 30,
            events: script::parse(text).unwrap(),
        };
        run_script(&run, &mut b, &mut *console, |_, _, _| {})
    };
    assert_eq!(
        run("0 backlight 255\n79 backlight 255\n"),
        Vec::<String>::new()
    );
    // Home opens the menu on the home screen; left dims one step.
    let menu = "60 press home\n61 release home\n63 press left\n64 release left\n";
    assert_eq!(
        run(&format!("{menu}64 backlight 156\n")),
        Vec::<String>::new()
    );
    let problems = run(&format!("{menu}64 backlight 255\n"));
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("frame 64: the backlight is 156, the script expected 255"),
        "{problems:?}"
    );
    // A word that is not a value is a mistake in the script.
    assert!(script::parse("3 backlight 256\n").is_err());
    assert!(script::parse("3 backlight\n").is_err());
}
