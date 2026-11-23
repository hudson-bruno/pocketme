//! The whole chain with no window: console-core, console-video, the
//! desktop board, the loop, the PNG writer. The console is still young,
//! so these tests check the chain, not what the console draws.

use std::path::Path;

use console_core::Console;
use desktop_board::board::DesktopBoard;
use desktop_board::run::{ScriptRun, run_script};
use desktop_board::{SAMPLE_CARD, picture, run, script};

/// The boot page's frame time: the GATHER animation (feature
/// `boot-gather`) asks for frames twice as often as the plain boot screen.
#[cfg(feature = "boot-gather")]
const BOOT_FRAME_US: u64 = console_core::theme::BOOT_FRAME_US;
#[cfg(not(feature = "boot-gather"))]
const BOOT_FRAME_US: u64 = console_core::FRAME_US;

/// Console frames until the boot has ended by itself, with a margin:
/// the plain boot screen ends well under 2 s (60 frames at 30 a second),
/// the GATHER animation after `gather::END_MS` at 60 a second.
#[cfg(feature = "boot-gather")]
const BOOT_DONE_FRAMES: u32 =
    (console_core::gather::END_MS as u64 * 1000 / BOOT_FRAME_US) as u32 + 5;
#[cfg(not(feature = "boot-gather"))]
const BOOT_DONE_FRAMES: u32 = 60;

/// At least `frames` console frames from power-on, all on the boot page.
/// Returns the clock, the frames drawn (`run_frames` looks once a tick,
/// and the GATHER animation draws two frames a tick), the fingerprint
/// and the PNG.
fn run_once(frames: u32) -> (u64, u32, u64, Vec<u8>) {
    let mut console = Box::new(Console::new());
    let mut board = Box::new(DesktopBoard::new(None));
    let end = run::run_frames(&mut console, &mut board, frames);
    let img = &board.screen.image;
    (
        end,
        console.frames(),
        picture::fingerprint(img),
        picture::png_bytes(img, 320, 240),
    )
}

#[test]
fn same_result_every_run() {
    let a = run_once(5);
    let b = run_once(5);
    assert_eq!(a, b);
    // Frames 0 to n - 1: the last step ran at the start of frame n - 1.
    let n = u64::from(a.1);
    assert!(n >= 5);
    assert_eq!(
        a.0,
        (n - 1) * BOOT_FRAME_US,
        "the clock moved one frame per step"
    );
    assert!(a.3.starts_with(b"\x89PNG\r\n\x1a\n"));
}

// console-core edit (shell round): the console now draws a boot screen
// and then the home screen, so this checks those instead of the old
// one-colour picture.
#[test]
fn the_boot_screen_draws_then_home_comes() {
    let mut console = Box::new(Console::new());
    let mut board = Box::new(DesktopBoard::new(None));
    run::run_frames(&mut console, &mut board, 2);
    let img = &board.screen.image;
    assert_ne!(img[0], 0, "the border is drawn");
    assert!(img.iter().any(|&p| p != img[0]), "the boot screen draws");
    assert_eq!(board.log.lines, ["console: started"]);
    // The boot ends by itself.
    run::run_frames(&mut console, &mut board, BOOT_DONE_FRAMES);
    assert_eq!(console.page(), console_core::Page::Home);
    assert_eq!(board.log.lines, ["console: started", "console: home"]);
}

/// Runs a script with the real console and the sample card. Returns the
/// fingerprint of every frame, the log, and the problems.
fn scripted(text: &str, frames: u64) -> (Vec<u64>, Vec<String>, Vec<String>) {
    let mut console = Box::new(Console::new());
    // Not about the boot: no boot animation (feature `boot-gather`).
    desktop_board::settings::set_boot(&mut console, desktop_board::settings::Boot::Off);
    let mut board = Box::new(DesktopBoard::new(Some(Path::new(SAMPLE_CARD))));
    let run = ScriptRun {
        frames,
        fps: 30,
        events: script::parse(text).unwrap(),
    };
    let mut prints = Vec::new();
    let problems = run_script(&run, &mut board, &mut *console, |_, b, _| {
        prints.push(picture::fingerprint(&b.screen.image))
    });
    (prints, board.log.lines.clone(), problems)
}

#[test]
fn a_script_gives_the_same_frames_every_time() {
    let text = "3 press south\n4 release south\n10 touch 160 120\n12 lift\n\
                20 stick left 32767 0\n30 card out\n40 card in\n50 pad off";
    let a = scripted(text, 90);
    let b = scripted(text, 90);
    assert!(a.2.is_empty(), "{:?}", a.2);
    assert_eq!(a.0.len(), 90);
    assert_eq!(a, b);
}
