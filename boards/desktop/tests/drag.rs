//! A drag on the home row, said in the script language: `touch X Y`
//! puts a finger down, more `touch` lines move it, `lift` lets go.
//! The real console and a test card of exactly 6 games (the sample
//! card's first six, or all of them and made-up folders up to six), so
//! the row scrolls up to 124 picture pixels. Panel pixels in the script,
//! 2 per picture pixel.

use std::path::{Path, PathBuf};

use console_core::{Console, Page};
use desktop_board::board::DesktopBoard;
use desktop_board::run::{ScriptRun, run_script};
use desktop_board::{SAMPLE_CARD, picture, script};

/// Home is up well before this frame (the boot screen ends at 1.5 s).
const START: u64 = 60;
/// The tiles' middle line, in panel pixels (picture y 40).
const ROW_Y: u16 = 80;
/// Picture pixels between two tiles' left edges (a tile and a gap).
const STRIDE: i16 = 44;

/// Finger down at panel x 260, then 20 panel pixels (10 picture pixels)
/// to the left every frame for 6 frames. Then it rests for `rest` frames
/// and is lifted (no `lift` line when `rest` is `None`).
fn drag_script(rest: Option<u64>) -> String {
    let mut s = String::new();
    for i in 0..=6u16 {
        s += &format!("{} touch {} {ROW_Y}\n", START + i as u64, 260 - 20 * i);
    }
    if let Some(rest) = rest {
        s += &format!("{} lift\n", START + 7 + rest);
    }
    s
}

/// The sample card copied to the test build's temporary folder, cut or
/// filled (game folders not linked in, no tile picture) to six tiles.
/// Made once per test, under its own name.
fn six_card(name: &str) -> PathBuf {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                copy(&e.path(), &to.join(e.file_name()));
            } else {
                std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
            }
        }
    }
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&root);
    copy(Path::new(SAMPLE_CARD), &root);
    let games = root.join("CONSOLE/GAMES");
    // The sample card may have more than six games (it has fourteen): the
    // last ones in name order are left off, so the row is always six
    // tiles and scrolls the same.
    let mut folders: Vec<PathBuf> = std::fs::read_dir(&games)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    folders.sort();
    for extra in folders.iter().skip(6) {
        std::fs::remove_dir_all(extra).unwrap();
    }
    let have = folders.len().min(6);
    for folder in ["XTRA1", "XTRA2", "XTRA3", "XTRA4", "XTRA5", "XTRA6"]
        .into_iter()
        .take(6 - have)
    {
        let dir = root.join("CONSOLE/GAMES").join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("GAME.BIN"), b"NOT A GAME FILE.\n").unwrap();
        std::fs::write(dir.join("INFO.TXT"), format!("NAME={folder}\n")).unwrap();
    }
    root
}

struct Run {
    console: Box<Console>,
    /// Home's game list at the end (read from the board's lent RAM).
    games: usize,
    prints: Vec<u64>,
    steps: Vec<u32>,
    problems: Vec<String>,
}

fn run(card: &Path, text: &str, frames: u64) -> Run {
    let mut console = Box::new(Console::new());
    // Not about the boot: no boot animation (feature `boot-gather`).
    desktop_board::settings::set_boot(&mut console, desktop_board::settings::Boot::Off);
    let mut board = Box::new(DesktopBoard::new(Some(card)));
    let run = ScriptRun {
        frames,
        fps: 30,
        events: script::parse(text).unwrap(),
    };
    let mut prints = Vec::new();
    let mut steps = Vec::new();
    let problems = run_script(&run, &mut board, &mut *console, |info, b, _| {
        prints.push(picture::fingerprint(&b.screen.image));
        steps.push(info.steps);
    });
    let games = console.home_games(&mut *board).len();
    Run {
        console,
        games,
        prints,
        steps,
        problems,
    }
}

#[test]
fn a_scripted_drag_moves_the_row_with_the_finger() {
    let text = drag_script(None);
    // Stop while the finger is still down, after its last move.
    let card = six_card("drag-moves");
    let held = run(&card, &text, START + 7);
    assert_eq!(held.games, 6);
    assert!(held.problems.is_empty(), "{:?}", held.problems);
    assert_eq!(held.console.page(), Page::Home);
    assert_eq!(held.console.home().scroll(), 60, "one to one: 6 x 10");
    // Each frame of the drag drew a new picture.
    let during = &held.prints[START as usize..];
    for w in during.windows(2) {
        assert_ne!(w[0], w[1], "the row moved every frame");
    }
}

#[test]
fn a_scripted_flick_glides_to_a_tidy_rest_then_sleeps() {
    let text = drag_script(Some(0));
    let end = START + 60;
    let card = six_card("drag-flick");
    let r = run(&card, &text, end);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.console.page(), Page::Home, "a drag starts no game");
    let home = r.console.home();
    let s = home.scroll();
    assert!(
        s == home.max_scroll() || s % STRIDE == 0,
        "rests on a tile edge or the row's end, not at {s}"
    );
    // Let go while still moving (300 picture pixels a second): a flick.
    // It carries 30 pixels (60 to 90), and the row goes on to the next
    // rest past that: 132, cut to the row's end.
    assert_eq!(s, home.max_scroll());
    assert!(!home.settling());
    // The last second: the picture is still and the console is asleep
    // (at most one step for the card check every 2 s).
    let last = &r.prints[(end - 30) as usize..];
    assert!(last.iter().all(|&p| p == last[0]), "the row is at rest");
    let woke: u32 = r.steps[(end - 30) as usize..].iter().sum();
    assert!(woke <= 1, "{woke} steps while at rest");
    // The same script gives the same frames every time.
    assert_eq!(run(&card, &text, end).prints, r.prints);
}

#[test]
fn a_scripted_drag_let_go_after_resting_goes_to_the_nearest_rest() {
    // 5 still frames (167 ms) before the lift: no speed, no flick. From
    // 60 the nearest rest is 44 (one tile).
    let text = drag_script(Some(5));
    let card = six_card("drag-rest");
    let r = run(&card, &text, START + 40);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.console.page(), Page::Home);
    assert_eq!(r.console.home().scroll(), STRIDE);
}
