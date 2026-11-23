//! The brightness kept in the store: read at start, saved once 2 s after
//! the player's last change, never written when nothing changed, and
//! back after a restart on the same store.

use console_core::{Console, FRAME_US};
use desktop_board::board::DesktopBoard;
use desktop_board::run::{ScriptRun, run_script};
use desktop_board::script;
use desktop_board::settings::{Kept, SETTINGS_BASE};
use settings_store::{QUIET_US, SettingsStore};

/// Opens the console menu (home) and sets the brightness one step down
/// (left) at frame 63: the backlight goes from 255 to 156.
const ONE_DOWN: &str = "60 press home\n61 release home\n63 press left\n64 release left\n";

/// Runs `text` for `frames` frames on `board` with a fresh console that
/// reads its settings from the board's store. Returns the console, the
/// records written, and the first frame after which the store was no
/// longer as it started (if it changed).
fn run(board: &mut DesktopBoard, text: &str, frames: u64) -> (Box<Console>, u32, Option<u64>) {
    let mut console = Box::new(Console::new());
    // Not about the boot: no boot animation (feature `boot-gather`).
    desktop_board::settings::set_boot(&mut console, desktop_board::settings::Boot::Off);
    let before = board.store.bytes.clone();
    let mut first_write = None;
    let saves = {
        let mut kept = Kept::start(&mut console, board);
        let run = ScriptRun {
            frames,
            fps: 30,
            events: script::parse(text).unwrap(),
        };
        let problems = run_script(&run, board, &mut kept, |info, b, _| {
            if first_write.is_none() && b.store.bytes != before {
                first_write = Some(info.frame);
            }
        });
        assert!(problems.is_empty(), "{problems:?}");
        kept.saves
    };
    (console, saves, first_write)
}

fn stored(board: &mut DesktopBoard) -> (bool, u8) {
    let mut s = SettingsStore::new(SETTINGS_BASE);
    let got = s.load(&mut board.store);
    (s.found(), got.brightness)
}

#[test]
fn nothing_changed_nothing_written() {
    let mut board = Box::new(DesktopBoard::new(None));
    let (console, saves, first_write) = run(&mut board, "", 200);
    assert_eq!((saves, first_write), (0, None));
    assert_eq!(console.brightness(), 4, "the default: full");
    assert_eq!(stored(&mut board), (false, 4));
    assert_eq!(board.log.lines[0], "settings: brightness 4 (default)");
}

#[test]
fn a_change_is_saved_once_after_the_quiet_time() {
    let mut board = Box::new(DesktopBoard::new(None));
    let (console, saves, first_write) = run(&mut board, ONE_DOWN, 200);
    assert_eq!(console.brightness(), 3);
    assert_eq!(saves, 1);
    // The change was at frame 63; the save 2 s (60 frames) later, in
    // the frame that holds that time.
    let quiet = QUIET_US.div_ceil(FRAME_US);
    let at = first_write.expect("saved");
    assert!(
        at >= 63 + quiet - 1 && at <= 63 + quiet,
        "saved at frame {at}"
    );
    assert_eq!(stored(&mut board), (true, 3));
    assert!(
        board
            .log
            .lines
            .iter()
            .any(|l| l == "settings: saved brightness 3 (record 1 of 256)"),
        "{:?}",
        board.log.lines
    );
}

#[test]
fn a_run_that_ends_before_the_quiet_time_keeps_nothing() {
    let mut board = Box::new(DesktopBoard::new(None));
    let (console, saves, first_write) = run(&mut board, ONE_DOWN, 63 + 50);
    assert_eq!(console.brightness(), 3);
    assert_eq!((saves, first_write), (0, None));
    assert_eq!(stored(&mut board), (false, 4));
}

#[test]
fn a_burst_of_changes_is_one_record() {
    // Down, down, up, down within a second: one save of the last step.
    let text = "60 press home\n61 release home\n\
                63 press left\n64 release left\n70 press left\n71 release left\n\
                78 press right\n79 release right\n86 press left\n87 release left\n";
    let mut board = Box::new(DesktopBoard::new(None));
    let (console, saves, first_write) = run(&mut board, text, 250);
    assert_eq!(console.brightness(), 2);
    assert_eq!(saves, 1);
    assert!(first_write.unwrap() >= 86 + 59, "{first_write:?}");
    assert_eq!(stored(&mut board), (true, 2));
}

#[test]
fn the_brightness_is_back_after_a_restart() {
    let mut board = Box::new(DesktopBoard::new(None));
    run(&mut board, ONE_DOWN, 200);
    // A new console and a new board, with the same store.
    let store = std::mem::take(&mut board.store);
    let mut board = Box::new(DesktopBoard::new(None));
    board.store = store;
    let (console, saves, _) = run(&mut board, "", 5);
    assert_eq!(console.brightness(), 3);
    assert_eq!(board.screen.backlight, Some(156), "from the first picture");
    assert_eq!(saves, 0, "loading writes nothing");
    assert_eq!(board.log.lines[0], "settings: brightness 3 (saved)");
}
