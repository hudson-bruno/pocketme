//! The second reader's checks (core-check round): the real console on
//! the desktop board, driven by long seeded random input. After every
//! tick the panel must show exactly the console's picture (nothing left
//! stale), the board must find no problems, and the same seed must give
//! the same frames every time, also when runs go side by side on many
//! threads. No window.

use std::path::{Path, PathBuf};

use board_api::{Files, FsError, Stick, TouchPoint, btn};
use console_core::{Console, Page, theme};
use desktop_board::board::DesktopBoard;
use desktop_board::controls::Controls;
use desktop_board::keymap::{Control, Side};
use desktop_board::run::Runner;
use desktop_board::{SAMPLE_CARD, picture};

/// A small, fixed random number maker (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const BUTTONS: [u32; 10] = [
    btn::SOUTH,
    btn::EAST,
    btn::WEST,
    btn::NORTH,
    btn::LEFT,
    btn::RIGHT,
    btn::UP,
    btn::DOWN,
    btn::HOME,
    btn::PLUS,
];

/// A card with many games and odd cases, made once per name in the test
/// build's temporary folder:
/// - 40 game folders G00..G39, more than the console lists (32);
/// - G05 has an empty GAME.BIN (not a game);
/// - names that are long, have letters the font lacks, or are the same;
/// - BADGAME has a folder named GAME.BIN (not a game);
/// - "notes" and "SPROUT~1" are names the 8.3 rule skips.
fn many_card(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&root);
    let games = root.join("CONSOLE/GAMES");
    for i in 0..40 {
        let dir = games.join(format!("G{i:02}"));
        std::fs::create_dir_all(&dir).unwrap();
        let bin: &[u8] = if i == 5 { b"" } else { b"GAME" };
        std::fs::write(dir.join("GAME.BIN"), bin).unwrap();
        let name = match i % 5 {
            0 => format!("A NAME FAR TOO LONG FOR THE SCREEN {i}"),
            1 => "caf\u{e9} \u{1F600}".to_string(),
            2 => "SAME".to_string(),
            3 => String::new(),
            _ => format!("GAME {i}"),
        };
        std::fs::write(dir.join("INFO.TXT"), format!("NAME={name}\nTAG=TAG {i}\n")).unwrap();
    }
    std::fs::create_dir_all(games.join("BADGAME/GAME.BIN")).unwrap();
    std::fs::create_dir_all(games.join("notes")).unwrap();
    std::fs::create_dir_all(games.join("SPROUT~1")).unwrap();
    std::fs::write(games.join("SPROUT~1/GAME.BIN"), b"GAME").unwrap();
    root
}

/// One random thing a person (or the world) does.
fn random_event(
    rng: &mut Rng,
    controls: &mut Controls,
    board: &mut DesktopBoard,
    console: &mut Console,
) {
    match rng.below(100) {
        0..=29 => controls.press(Control::Button(BUTTONS[rng.below(10) as usize])),
        30..=54 => controls.release(Control::Button(BUTTONS[rng.below(10) as usize])),
        55..=59 => controls.press(Control::Boot),
        60..=64 => controls.release(Control::Boot),
        65..=72 => controls.touch(TouchPoint {
            x: rng.below(320) as u16,
            y: rng.below(240) as u16,
        }),
        73..=78 => controls.lift(),
        79..=86 => {
            // Often small (inside the dead zone: noise), sometimes full.
            let big = rng.below(3) == 0;
            let r = |rng: &mut Rng| {
                if big {
                    (rng.below(65_535) as i32 - 32_767) as i16
                } else {
                    (rng.below(8_000) as i32 - 4_000) as i16
                }
            };
            let s = Stick {
                x: r(rng),
                y: r(rng),
            };
            controls.set_stick(Side::Left, s);
        }
        87..=88 => controls.set_connected(rng.below(2) == 0),
        89..=90 => board.files.set_card(rng.below(3) != 0),
        91..=93 => {
            let texts = [
                "HELLO",
                "A VERY LONG NOTICE THAT DOES NOT FIT AT ALL",
                "",
                "\u{e9}\u{e9}",
            ];
            let now = board.clock.now_us;
            let _ = console.notify(texts[rng.below(4) as usize], now);
        }
        94 => controls.release_all(),
        _ => {}
    }
}

/// Runs `ticks` ticks of random input from `seed` on `card`. With
/// `check`, compares the whole panel to the console's picture after
/// every tick. Returns every tick's fingerprint, the log, and how much
/// of the console the run reached.
fn fuzz(seed: u64, card: Option<&Path>, ticks: u64, check: bool) -> (Vec<u64>, Vec<String>, Stats) {
    let mut console = Box::new(Console::new());
    let mut board = Box::new(DesktopBoard::new(card));
    let mut controls = Controls::new();
    let mut runner = Runner::new(30);
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ seed.wrapping_mul(0x1000_0000_01B3));
    let mut prints = Vec::with_capacity(ticks as usize);
    let mut stats = Stats::default();
    // A person leaving a game the console's way, one input a tick:
    // Home opens the console menu, Down twice reaches BACK TO HOME, the
    // confirm button takes it; then Right one to three times, to the
    // next game along the row. Real games (HAIRPIN, KESTREL, LANDER,
    // LANTERN, ROOFTOP) do not end on one random press, and random input
    // alone seldom finds this, so a seed could otherwise stay in the
    // first game to the end. The
    // extra random numbers are drawn only while a game runs, so runs
    // that never start a game are as before. (`leave` is used from its
    // end.)
    let mut leave: Vec<(u32, bool)> = Vec::new();
    for _ in 0..ticks {
        if leave.is_empty() && console.page() == Page::Game && rng.below(60) == 0 {
            controls.release_all();
            for _ in 0..=rng.below(3) {
                leave.extend([(btn::RIGHT, false), (btn::RIGHT, true)]);
            }
            leave.extend([
                (btn::SOUTH, false),
                (btn::SOUTH, true),
                (btn::DOWN, false),
                (btn::DOWN, true),
                (btn::DOWN, false),
                (btn::DOWN, true),
                (btn::HOME, false),
                (btn::HOME, true),
            ]);
        }
        if let Some((b, down)) = leave.pop() {
            if down {
                controls.press(Control::Button(b));
            } else {
                controls.release(Control::Button(b));
            }
        } else {
            // Quiet stretches too, so the console goes idle and wakes
            // again.
            let n = match rng.below(10) {
                0..=4 => 0,
                5..=8 => 1,
                _ => 3,
            };
            for _ in 0..n {
                random_event(&mut rng, &mut controls, &mut board, &mut console);
            }
        }
        runner.run_tick(&mut board, &mut controls, |b| console.step(b));
        // While a game runs under a notice or the pause menu, the console
        // puts the game's own pixels back into its picture once the panel
        // has them (so the game finds its picture as it left it). The
        // panel then rightly differs from that picture where the overlay
        // is: those pixels are not compared, all others are. The first
        // frame after the overlay leaves is compared whole, so nothing
        // stale can hide. (Added by the games step: before it, no game
        // could start on the sample card.)
        let in_game = console.page() == Page::Game;
        let band = in_game && !console.notices().is_empty();
        let menu = in_game && console.game().paused();
        let covered = |x: usize, y: usize| {
            let band_rows = (theme::NOTE_Y as usize) + theme::NOTE_H as usize;
            let (mx, my) = (theme::MENU_X as usize, theme::MENU_Y as usize);
            (band && y < band_rows)
                || (menu
                    && (mx..mx + theme::MENU_W as usize).contains(&x)
                    && (my..my + theme::MENU_H as usize).contains(&y))
        };
        // While home is read again from the card after a game (or after
        // a game that could not start), the console draws and sends
        // nothing on purpose: the panel keeps the last picture, which
        // differs from the console's half-drawn one. Not compared then;
        // the next frame that sends is compared whole.
        let pending = console.home_pending();
        if pending {
            assert_eq!(
                console.last_report().areas,
                0,
                "seed {seed}: sent while pending"
            );
            stats.pending_frames += 1;
        }
        if check && !pending {
            let video = console.video();
            let (frame, pal) = (video.frame(), video.palette());
            for y in 0..240usize {
                let src = frame.row(y / 2);
                let row = &board.screen.image[y * 320..(y + 1) * 320];
                let same = row
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .zip(src)
                    .enumerate()
                    .all(|(x, (&[a, b], &c))| covered(x, y / 2) || (a == pal.get(c) && b == a));
                assert!(
                    same,
                    "seed {seed} frame {}: panel row {y} is stale",
                    runner.tick - 1
                );
            }
            if in_game {
                stats.game_frames_compared += 1;
            }
        }
        if console.page() == Page::Home {
            stats.home_frames += 1;
            stats.most_selected = stats.most_selected.max(console.home().selected());
        }
        stats.notices = stats.notices.max(console.notices().len());
        prints.push(picture::fingerprint(&board.screen.image));
    }
    let mut problems = runner.problems;
    problems.extend(board.take_problems());
    assert!(problems.is_empty(), "seed {seed}: {problems:?}");
    (prints, board.log.lines.clone(), stats)
}

/// How much of the console a run reached (so a run that never left the
/// boot screen does not pass as a good test).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Stats {
    home_frames: u64,
    most_selected: usize,
    notices: usize,
    /// Frames of a running game whose panel was compared.
    game_frames_compared: u64,
    /// Frames that sent nothing while home was read again after a game.
    pending_frames: u64,
}

#[test]
fn random_input_never_leaves_the_panel_stale_sample_card() {
    // The sample card has linked games (games step): random presses can
    // start them, and a running game is compared too, so a seed may
    // spend most of its frames in a game instead of on the home screen.
    // With 7 tiles a seed often starts the first or second game before
    // it gets further along the row, so the furthest tile is asked of
    // the three seeds together. The real games (CAMBER is first) do not
    // end on one random press: `fuzz` leaves them now and then through
    // the console menu (BACK TO HOME), so every seed gets back home.
    // 1,200 ticks: when LANTERN was the second tile (5 tiles), seed 3
    // had only 59 home frames in 900 (it starts a game again soon after
    // each leave).
    let mut game_frames = 0;
    let mut furthest = 0;
    let mut pending = 0;
    for seed in 1..=3 {
        let (_, _, st) = fuzz(seed, Some(Path::new(SAMPLE_CARD)), 1_200, true);
        eprintln!("seed {seed}: {st:?}");
        assert!(st.home_frames > 60, "{st:?}");
        game_frames += st.game_frames_compared;
        furthest = furthest.max(st.most_selected);
        pending += st.pending_frames;
    }
    assert!(furthest >= 2, "no seed got past the second tile");
    // Every way back home read the list from the card again (the lent
    // RAM went to the game), with nothing sent meanwhile.
    assert!(pending > 0, "no seed came back home from a game");
    assert!(game_frames > 100, "only {game_frames} frames in a game");
}

#[test]
fn random_input_never_leaves_the_panel_stale_many_games() {
    let card = many_card("attack-many-a");
    for seed in 11..=12 {
        let (_, _, st) = fuzz(seed, Some(&card), 900, true);
        assert!(st.home_frames > 600 && st.most_selected >= 3, "{st:?}");
        assert!(st.notices >= 2, "{st:?}");
    }
}

#[test]
fn random_input_with_no_card() {
    let (_, _, st) = fuzz(21, None, 600, true);
    assert!(st.home_frames > 400, "{st:?}");
}

/// The same seed gives the same frames: twice in a row, and four at a
/// time on separate threads.
#[test]
fn the_same_seed_gives_the_same_frames_on_one_thread_or_many() {
    let card = many_card("attack-many-b");
    let one = fuzz(31, Some(&card), 1_000, false);
    let again = fuzz(31, Some(&card), 1_000, false);
    assert_eq!(one, again);
    let side_by_side: Vec<_> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..4)
            .map(|_| s.spawn(|| fuzz(31, Some(&card), 1_000, false)))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for r in side_by_side {
        assert_eq!(r, one);
    }
}

/// The many-games card on the home screen: 32 folders are looked at,
/// the empty game and the folder named GAME.BIN are left out, and the
/// odd names are cut and drawn.
#[test]
fn the_many_games_card_lists_as_expected() {
    let card = many_card("attack-many-c");
    let mut console = Box::new(Console::new());
    // Not about the boot: no boot animation (feature `boot-gather`).
    desktop_board::settings::set_boot(&mut console, desktop_board::settings::Boot::Off);
    let mut board = Box::new(DesktopBoard::new(Some(&card)));
    desktop_board::run::run_frames(&mut console, &mut board, 80);
    assert_eq!(console.page(), Page::Home);
    let list = console.home_games(&mut *board);
    let home = console.home();
    let folders: Vec<&str> = list.iter().map(|g| g.folder.as_str()).collect();
    // The first 32 folders in name order are BADGAME, G00..G30. BADGAME
    // and G05 are no games: 30 left.
    assert_eq!(home.count(), 30, "{folders:?}");
    assert!(!folders.contains(&"BADGAME"));
    assert!(!folders.contains(&"G05"));
    assert!(list.iter().all(|g| g.name.as_str().len() <= 12));
    let mut skipped = board.files.skipped.clone();
    skipped.sort();
    assert_eq!(skipped, ["SPROUT~1", "notes"]);
}

/// Fault (found in this round): the desktop board gave a folder's size
/// (4096 on Linux) for `size`, so a folder named GAME.BIN counted as a
/// game. Now a folder is "not found", as on a real card.
#[test]
fn size_of_a_folder_is_not_found() {
    let card = many_card("attack-many-d");
    let mut board = Box::new(DesktopBoard::new(Some(&card)));
    assert_eq!(
        board.files.size("/CONSOLE/GAMES/BADGAME/GAME.BIN"),
        Err(FsError::NotFound)
    );
    assert_eq!(board.files.size("/CONSOLE/GAMES/G00/GAME.BIN"), Ok(4));
}
