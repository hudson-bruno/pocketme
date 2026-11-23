//! The linked games in the simulator, with no window: each game starts
//! from its tile, plays with input and motion, pauses and goes back home.
//! Every run gives the same pictures.

use std::path::{Path, PathBuf};

use board_api::btn;
use console_core::game_api::{self, Crc32, GameHeader, layout, rate};
use console_core::{Console, Page};
use desktop_board::board::DesktopBoard;
use desktop_board::controls::Controls;
use desktop_board::games::GAMES;
use desktop_board::keymap::Control;
use desktop_board::run::{Runner, ScriptRun, run_script};
use desktop_board::{SAMPLE_CARD, picture, script};

const SOUTH: Control = Control::Button(btn::SOUTH);
const RIGHT: Control = Control::Button(btn::RIGHT);
const LEFT: Control = Control::Button(btn::LEFT);
const DOWN: Control = Control::Button(btn::DOWN);
const HOME: Control = Control::Button(btn::HOME);
const PLUS: Control = Control::Button(btn::PLUS);

/// The tiles on the sample card, in the home screen's order.
const TILES: [&str; 14] = [
    "BENCH", "CAMBER", "DRIVE3D", "HAIRPIN", "KESTREL", "LANDER", "LANTERN", "MERCATOR", "PADTEST",
    "PROTO3D", "ROOFTOP", "SKIMMER", "STARDUST", "TUNDRA",
];

/// A copy of the sample card in the test build's temporary folder, with
/// more game folders that are not linked in (their GAME.BIN is a line of
/// text, as on the sample card). Made fresh for each `name`.
fn card_with(name: &str, extra: &[&str]) -> PathBuf {
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
    for folder in extra {
        let dir = root.join("CONSOLE/GAMES").join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("GAME.BIN"), b"NOT A GAME FILE. Not linked in.\n").unwrap();
        std::fs::write(
            dir.join("INFO.TXT"),
            format!("NAME={folder}\nTAG=NOT LINKED\n"),
        )
        .unwrap();
    }
    root
}

/// The console on the desktop board with the sample card, driven tick by
/// tick (the same loop as the program).
struct Sim {
    console: Box<Console>,
    board: Box<DesktopBoard>,
    controls: Controls,
    runner: Runner,
}

impl Sim {
    /// Booted to the home screen, with the sample card.
    fn new() -> Sim {
        Sim::with_card(Path::new(SAMPLE_CARD))
    }

    /// Booted to the home screen, with this card folder.
    fn with_card(card: &Path) -> Sim {
        let mut s = Sim {
            console: Box::new(Console::new()),
            board: Box::new(DesktopBoard::new(Some(card))),
            controls: Controls::new(),
            runner: Runner::new(30),
        };
        while s.console.page() != Page::Home {
            s.tick();
            assert!(s.runner.tick < 120, "no home screen");
        }
        s
    }

    fn tick(&mut self) {
        let console = &mut self.console;
        self.runner
            .run_tick(&mut self.board, &mut self.controls, |b| console.step(b));
        let mut problems = std::mem::take(&mut self.runner.problems);
        problems.extend(self.board.take_problems());
        assert!(problems.is_empty(), "{problems:?}");
    }

    fn ticks(&mut self, n: u32) {
        for _ in 0..n {
            self.tick();
        }
    }

    /// A press of one frame, then one frame let go.
    fn tap(&mut self, c: Control) {
        self.controls.press(c);
        self.tick();
        self.controls.release(c);
        self.tick();
    }

    /// Moves to a tile and starts it.
    fn start(&mut self, folder: &str) {
        let list = self.console.home_games(&mut *self.board);
        let want = (list.iter())
            .position(|g| g.folder.as_str() == folder)
            .unwrap();
        while self.console.home().selected() < want {
            self.tap(RIGHT);
        }
        while self.console.home().selected() > want {
            self.tap(LEFT);
        }
        let list = self.console.home_games(&mut *self.board);
        assert_eq!(list[want].folder.as_str(), folder);
        self.tap(SOUTH);
    }

    /// Home, then BACK TO HOME in the pause menu.
    fn leave(&mut self) {
        self.tap(HOME);
        assert!(self.console.game().paused());
        self.tap(DOWN);
        self.tap(DOWN); // past BRIGHTNESS to BACK TO HOME
        self.tap(SOUTH);
        assert_eq!(self.console.page(), Page::Home);
    }

    fn print(&self) -> u64 {
        picture::fingerprint(&self.board.screen.image)
    }

    fn log_has(&self, line: &str) -> bool {
        self.board.log.lines.iter().any(|l| l == line)
    }
}

/// Where the SKIMMER tests leave their pictures.
fn air_hockey_shots() -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/airhockey/sim");
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Where the TUNDRA tests leave their pictures.
fn hockey_shots() -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/four-players/hockey/shots");
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// The games' slot numbers come from game-api alone: build-chip.sh makes
/// slots.x from `layout::write_linker_symbols`, and nothing in games/chip
/// writes a slot address down (the old copies were slots.x and pack.py).
#[test]
fn the_games_take_their_slots_from_game_api() {
    let chip = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games/chip");
    assert!(
        !chip.join("slots.x").exists(),
        "games/chip/slots.x is back: build-chip.sh makes it from game-api"
    );
    for file in ["game.x", "pack.py", "check_load.py", "../build-chip.sh"] {
        let text = std::fs::read_to_string(chip.join(file)).unwrap();
        let text = text.to_ascii_lowercase().replace('_', "");
        // Stage 1: where code past the slot goes, and the data slot's
        // end (the spill's mirror), are slot numbers too.
        for addr in [
            layout::CODE_SLOT_ADDR,
            layout::DATA_SLOT_ADDR,
            layout::CODE_SPILL_ADDR,
            layout::DRAM2_END,
        ] {
            let hex = format!("{addr:#010x}");
            // Comments may name the slot; code may not. Only the comment
            // lines (# or *) may hold the address.
            for line in text.lines().filter(|l| l.contains(&hex)) {
                let l = line.trim_start();
                assert!(
                    l.starts_with('#') || l.starts_with('*') || l.starts_with("/*"),
                    "{file} writes the slot address {hex} down: `{line}`"
                );
            }
        }
    }
    let pack = std::fs::read_to_string(chip.join("pack.py")).unwrap();
    let line = format!("API_VERSION = {}", game_api::API_VERSION);
    assert!(pack.contains(&line), "pack.py lacks `{line}`");

    // The slots.x build-chip.sh made (check.sh names its folder).
    let Some(dir) = std::env::var_os("GAME_FILES_DIR") else {
        eprintln!("GAME_FILES_DIR not set: run games/build-chip.sh (check.sh does)");
        return;
    };
    let made = std::fs::read_to_string(Path::new(&dir).join("slots.x")).unwrap();
    let mut want = String::new();
    layout::write_linker_symbols(&mut want).unwrap();
    assert!(
        made.ends_with(&want),
        "slots.x is not game-api's layout:\n{made}"
    );
}

/// The real games' INFO.TXT (games/<game>/INFO.TXT, which build-chip.sh
/// puts on the chip's card) is the sample card's.
#[test]
fn the_games_info_files_match_the_sample_card() {
    let games = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games");
    for g in GAMES {
        let own = games.join(g.folder.to_ascii_lowercase()).join("INFO.TXT");
        let card = Path::new(SAMPLE_CARD)
            .join("CONSOLE/GAMES")
            .join(g.folder)
            .join("INFO.TXT");
        assert_eq!(
            std::fs::read(&own).unwrap(),
            std::fs::read(&card).unwrap(),
            "{}",
            g.folder
        );
    }
}

/// The game files games/build-chip.sh made pass the console's own header
/// check. check.sh builds them first and names their folder in
/// GAME_FILES_DIR; a plain `cargo test` has no such folder and skips this.
///
/// A game with a flash part (a `flash.list`, MERCATOR today) is a "GAMF"
/// file: its 32-byte extension follows the header and is covered by the
/// CRC, the payload starts after it, and the extension names exactly the
/// part build-chip.sh wrote to `flash/<NAME>/` (its place in
/// game_api::layout, sizes, CRC-32s). Every other game is a plain "GAME"
/// file, as before.
#[test]
fn chip_game_files_pass_the_header_check() {
    let Some(dir) = std::env::var_os("GAME_FILES_DIR") else {
        eprintln!("GAME_FILES_DIR not set: run games/build-chip.sh (check.sh does)");
        return;
    };
    let dir = Path::new(&dir);
    let crc_of = |bytes: &[u8]| {
        if bytes.is_empty() {
            return 0;
        }
        let mut crc = Crc32::new();
        crc.update(bytes);
        crc.value()
    };
    for game in GAMES {
        let bytes = std::fs::read(dir.join(format!("{}.BIN", game.folder))).unwrap();
        let (head, flash) =
            game_api::read_game_header(&bytes).unwrap_or_else(|e| panic!("{}: {e:?}", game.folder));
        let place = layout::flash_place(game.folder);
        assert_eq!(
            flash.is_some(),
            place.is_some(),
            "{}: a flash part exactly when layout gives the game a place",
            game.folder
        );
        let head_size = match flash {
            Some(_) => game_api::FlashPartHeader::FILE_HEAD,
            None => GameHeader::SIZE,
        };
        assert_eq!(head.payload_size() as usize, bytes.len() - head_size);
        let mut crc = Crc32::new();
        crc.update(&bytes[..GameHeader::CHECKED]);
        crc.update(&bytes[GameHeader::SIZE..]);
        assert_eq!(crc.value(), head.crc32, "{}", game.folder);
        if let (Some(ext), Some(place)) = (flash, place) {
            let part = dir.join("flash").join(game.folder);
            let code = std::fs::read(part.join("CODE.BIN")).unwrap();
            let ro = std::fs::read(part.join("RO.BIN")).unwrap();
            assert_eq!(ext.code_addr, place.code_addr(), "{}", game.folder);
            assert_eq!(ext.code_size as usize, code.len(), "{}", game.folder);
            assert_eq!(ext.code_crc32, crc_of(&code), "{}", game.folder);
            assert!(code.len() <= place.code_max as usize, "{}", game.folder);
            assert_eq!(ext.ro_size as usize, ro.len(), "{}", game.folder);
            assert_eq!(ext.ro_crc32, crc_of(&ro), "{}", game.folder);
            assert!(ro.len() <= place.ro_max as usize, "{}", game.folder);
            if !ro.is_empty() {
                assert_eq!(ext.ro_addr, place.ro_addr(), "{}", game.folder);
            }
        }
    }
}

#[test]
fn every_linked_game_has_a_folder_on_the_sample_card() {
    let games = Path::new(SAMPLE_CARD).join("CONSOLE/GAMES");
    for g in GAMES {
        assert!(
            games.join(g.folder).join("GAME.BIN").is_file(),
            "{}",
            g.folder
        );
        assert!(
            games.join(g.folder).join("INFO.TXT").is_file(),
            "{}",
            g.folder
        );
    }
    let mut sim = Sim::new();
    let list = sim.console.home_games(&mut *sim.board);
    let listed: Vec<&str> = list.iter().map(|g| g.folder.as_str()).collect();
    assert_eq!(listed, TILES);
}

/// No game file on the sample card pretends to be a game: none starts
/// with the game file's magic word, so no header claims a version (the
/// made-up games used to carry version 1 headers). The real chip files
/// come from games/build-chip.sh.
#[test]
fn no_file_on_the_sample_card_pretends_to_be_a_game() {
    let games = Path::new(SAMPLE_CARD).join("CONSOLE/GAMES");
    for tile in TILES {
        let bytes = std::fs::read(games.join(tile).join("GAME.BIN")).unwrap();
        assert!(!bytes.is_empty(), "{tile}: home lists only non-empty files");
        assert!(
            !bytes.starts_with(&game_api::MAGIC.to_le_bytes())
                && !bytes.starts_with(&game_api::MAGIC_FLASH.to_le_bytes()),
            "{tile}: starts with a game header"
        );
        assert!(
            bytes.starts_with(b"NOT A GAME FILE."),
            "{tile}: says what it is"
        );
        if let Some(head) = bytes.first_chunk::<{ GameHeader::SIZE }>() {
            assert!(GameHeader::from_bytes(head).validate().is_err(), "{tile}");
        }
        assert!(game_api::read_game_header(&bytes).is_err(), "{tile}");
    }
}

/// The tile pictures on the sample card are the ones games/build-chip.sh
/// makes for the chip card (both from games/<game>/art.txt), and each
/// passes the console's own reader. A game with an art.txt has an
/// ART.BIN on the sample card; run tools/make_sample_card.py after
/// changing an art.txt.
#[test]
fn the_tile_pictures_on_the_sample_card_are_the_games_own() {
    let games = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games");
    let chip = std::env::var_os("GAME_FILES_DIR");
    if chip.is_none() {
        eprintln!("GAME_FILES_DIR not set: only the console's check (check.sh compares too)");
    }
    for g in GAMES {
        let src = games.join(g.folder.to_ascii_lowercase()).join("art.txt");
        let card = Path::new(SAMPLE_CARD)
            .join("CONSOLE/GAMES")
            .join(g.folder)
            .join("ART.BIN");
        assert_eq!(
            src.is_file(),
            card.is_file(),
            "{}: art.txt and ART.BIN",
            g.folder
        );
        if !card.is_file() {
            continue;
        }
        let bytes = std::fs::read(&card).unwrap();
        let mut pic = console_core::art::Picture::new();
        assert_eq!(
            console_core::art::parse(&bytes, &mut pic),
            Ok(()),
            "{}",
            g.folder
        );
        if let Some(dir) = &chip {
            let made = Path::new(dir)
                .join("card/CONSOLE/GAMES")
                .join(g.folder)
                .join("ART.BIN");
            assert_eq!(
                std::fs::read(&made).unwrap(),
                bytes,
                "{}: the sample card's ART.BIN is old: run tools/make_sample_card.py",
                g.folder
            );
        }
    }
}

/// Every linked game starts from its tile, shows a picture of its own,
/// and goes back home through the pause menu.
#[test]
fn every_linked_game_starts_and_leaves() {
    let mut sim = Sim::new();
    let home = sim.print();
    for (i, g) in GAMES.iter().enumerate() {
        sim.start(g.folder);
        assert_eq!(sim.console.page(), Page::Game, "{}", g.folder);
        assert_eq!(sim.console.game().folder(), g.folder);
        sim.ticks(5);
        assert_ne!(sim.print(), home, "{} draws its own picture", g.folder);
        sim.leave();
        assert_eq!(sim.board.games.started, i as u32 + 1);
        assert_eq!(sim.board.games.ended, i as u32 + 1);
        let lower = g.folder.to_ascii_lowercase();
        assert!(
            sim.log_has(&format!("game: {lower}: started")),
            "{}",
            g.folder
        );
    }
}

/// A folder on the card with no linked game (here a test card, a copy
/// of the sample card with one more folder) shows GAME NOT FOUND.
#[test]
fn a_game_that_is_not_linked_cannot_start() {
    let card = card_with("not-linked", &["NOTLINKD"]);
    let mut sim = Sim::with_card(&card);
    sim.start("NOTLINKD");
    sim.ticks(2);
    assert_eq!(sim.console.page(), Page::Home);
    assert!(sim.log_has("home: game not found"));
    assert_eq!(sim.board.games.started, 0);
}

#[test]
fn with_no_games_linked_nothing_starts() {
    let mut sim = Sim::new();
    sim.board.games.list = &[];
    sim.start("PADTEST");
    assert_eq!(sim.console.page(), Page::Home);
    assert!(sim.log_has("home: games cannot start yet"));
}

#[test]
fn padtest_shows_input_and_motion_and_sleeps_when_still() {
    let mut sim = Sim::new();
    sim.start("PADTEST");
    assert_eq!(sim.console.page(), Page::Game);
    assert!(sim.log_has("console: game PADTEST"));
    assert!(sim.log_has("game: padtest: started"));
    sim.ticks(3);
    let still = sim.print();

    // Each kind of input changes the picture.
    let mut prints = vec![still];
    let mut changed = |sim: &mut Sim, what: &str| {
        sim.ticks(2);
        let p = sim.print();
        assert!(!prints.contains(&p), "{what} changed nothing on the screen");
        prints.push(p);
    };
    sim.controls.press(Control::Button(btn::WEST));
    changed(&mut sim, "a button");
    sim.controls.set_stick(
        desktop_board::keymap::Side::Left,
        board_api::Stick { x: 20000, y: 0 },
    );
    changed(&mut sim, "a stick");
    sim.controls
        .set_trigger(desktop_board::keymap::Side::Right, 1023);
    changed(&mut sim, "a trigger");
    sim.controls.touch(board_api::TouchPoint { x: 100, y: 60 });
    changed(&mut sim, "the touch");
    sim.controls.set_tilt(2000, -1000);
    changed(&mut sim, "a tilt");
    sim.controls.set_spin([0, 320, 0]);
    changed(&mut sim, "a spin");
    sim.controls.set_motion(false);
    changed(&mut sim, "motion off");
    sim.controls.set_connected_by(1, true);
    changed(&mut sim, "player 2's pad plugged in");
    sim.controls.press_by(1, Control::Button(btn::SOUTH));
    changed(&mut sim, "player 2's button");

    // Let go of everything: after a second of nothing, it sleeps.
    sim.controls.release(Control::Button(btn::WEST));
    sim.controls.release_by(1, Control::Button(btn::SOUTH));
    sim.controls.set_stick(
        desktop_board::keymap::Side::Left,
        board_api::Stick { x: 0, y: 0 },
    );
    sim.controls
        .set_trigger(desktop_board::keymap::Side::Right, 0);
    sim.controls.lift();
    sim.ticks(35);
    assert!(sim.log_has("game: padtest: still, sleeping until something changes"));
    assert_eq!(sim.console.game().rate(), rate::ON_DEMAND);
    // Asleep: no call for a few frames, then one at its timer (a quarter
    // of a second, between 7 and 8 frames).
    let calls = sim.console.game().frames();
    sim.ticks(4);
    let quiet = sim.console.game().frames() - calls;
    sim.ticks(16);
    let later = sim.console.game().frames() - calls;
    assert!(quiet <= 1, "called {quiet} times in 4 frames while asleep");
    assert!(
        (2..=4).contains(&later),
        "{later} calls in 20 frames while asleep"
    );
    // Motion alone does not wake it at once; the timer sees it.
    sim.controls.set_motion(true);
    sim.ticks(9);
    assert!(sim.log_has("game: padtest: awake, 30 calls a second"));
    sim.ticks(2);
    assert_eq!(sim.console.game().rate(), rate::FPS30);
}

#[test]
fn stardust_flies_at_30_frames_a_second_and_catches_stars() {
    let mut sim = Sim::new();
    sim.start("STARDUST");
    assert!(sim.log_has("game: stardust: started"));
    assert_eq!(sim.console.game().rate(), rate::FPS30);
    let calls = sim.console.game().frames();
    let mut prints = std::collections::HashSet::new();
    sim.controls.press(RIGHT);
    for _ in 0..30 {
        sim.tick();
        prints.insert(sim.print());
    }
    assert_eq!(sim.console.game().frames() - calls, 30, "one call a frame");
    assert!(prints.len() > 25, "the picture moves every frame");
    sim.controls.release(RIGHT);
    // Steer to a star with the finger: find a star's pixel on the screen
    // (colour 8 in the toy's palette) and hold the finger there. The
    // touch panel has twice the game's pixels.
    let frame = sim.console.video().frame();
    let (x, y) = (0..120u16)
        .flat_map(|y| (0..160u16).map(move |x| (x, y)))
        .find(|&(x, y)| frame.get(x, y) == 8)
        .expect("a star on the screen");
    sim.controls.touch(board_api::TouchPoint {
        x: x * 2 + 1,
        y: y * 2 + 1,
    });
    for _ in 0..90 {
        sim.tick();
        if sim.log_has("game: stardust: star 1") {
            break;
        }
    }
    sim.controls.lift();
    assert!(
        sim.log_has("game: stardust: star 1"),
        "{:?}",
        sim.board.log.lines
    );
    // Start ends the toy: back home with a notice.
    sim.tap(PLUS);
    assert_eq!(sim.console.page(), Page::Home);
    assert!(sim.log_has("console: game ended"));
    assert_eq!((sim.board.games.started, sim.board.games.ended), (1, 1));
}

/// LANTERN, the first-person game: its title waits for input, PLAY asks
/// for 60 calls a second (the game's starting RATE, with detail HEAVY),
/// walking changes the picture every frame, and its own pause menu waits
/// for input again.
#[test]
fn lantern_walks_at_60_frames_a_second_and_waits_on_its_title() {
    let mut sim = Sim::new();
    sim.start("LANTERN");
    assert!(sim.log_has("game: lantern: started"));
    sim.ticks(3);
    assert_eq!(sim.console.game().rate(), rate::ON_DEMAND, "the title");
    // Start (Return) plays from any line of the title.
    sim.tap(PLUS);
    sim.ticks(2);
    assert!(
        sim.log_has("game: lantern: level 1, detail HEAVY"),
        "{:?}",
        sim.board.log.lines
    );
    assert_eq!(sim.console.game().rate(), rate::FPS60);
    let mut prints = std::collections::HashSet::new();
    sim.controls.press(Control::Button(btn::UP));
    for _ in 0..20 {
        sim.tick();
        prints.insert(sim.print());
    }
    sim.controls.release(Control::Button(btn::UP));
    assert!(prints.len() >= 15, "walking moves the view");
    // Its own pause menu (Start): the console may then call it only on
    // input.
    sim.tap(PLUS);
    sim.ticks(2);
    assert_eq!(sim.console.game().rate(), rate::ON_DEMAND, "paused");
    sim.leave();
}

#[test]
fn pause_resume_and_back_to_home() {
    let mut sim = Sim::new();
    sim.start("STARDUST");
    sim.ticks(5);
    sim.tap(HOME);
    assert!(sim.console.game().paused());
    let paused = sim.print();
    let calls = sim.console.game().frames();
    sim.ticks(10);
    assert_eq!(sim.print(), paused, "nothing moves while paused");
    assert_eq!(sim.console.game().frames(), calls, "the game is not called");
    sim.tap(SOUTH); // RESUME
    assert!(!sim.console.game().paused());
    sim.ticks(2);
    assert!(sim.console.game().frames() > calls);
    sim.leave();
    assert!(sim.log_has("console: back to home"));
    assert_eq!(sim.board.games.ended, 1);
}

#[test]
fn a_notification_over_a_running_game_leaves_no_trace() {
    let mut sim = Sim::new();
    sim.start("PADTEST");
    // Still for long enough that it sleeps: the picture stays the same.
    sim.ticks(40);
    let before = sim.print();
    assert!(sim.console.notify("HELLO FROM THE CONSOLE", 0));
    sim.ticks(10);
    assert!(!sim.console.notices().is_empty());
    assert_ne!(sim.print(), before, "the notice shows");
    // The notice leaves after a few seconds; the game's picture is back.
    for _ in 0..300 {
        if sim.console.notices().is_empty() {
            break;
        }
        sim.tick();
    }
    assert!(sim.console.notices().is_empty());
    sim.ticks(10);
    assert_eq!(sim.print(), before);
}

#[test]
fn fifty_starts_and_returns_leave_the_console_as_it_was() {
    let mut sim = Sim::new();
    sim.start("PADTEST");
    sim.leave();
    let selected = sim.console.home().selected();
    // The picture to come back to: the first tile, once all has settled.
    // (On the first tile the row of tiles is not scrolled; elsewhere how
    // far it is scrolled depends on the way the player came.)
    let settled = |sim: &mut Sim| {
        while sim.console.home().selected() > 0 {
            sim.tap(LEFT);
        }
        // The row stops and the notices go (several may wait in turn).
        for _ in 0..1800 {
            if sim.console.notices().is_empty() && !sim.console.home().moving() {
                break;
            }
            sim.tick();
        }
        assert!(sim.console.notices().is_empty() && !sim.console.home().moving());
        sim.ticks(5);
        // The palette, without the tile pictures' part: home reads the
        // pictures from the card again after every game (the game had the
        // RAM they were kept in), nearest the selected tile first, so the
        // same picture may land in another of the six slots (and colour
        // numbers) than at start. The fingerprint shows the screen is
        // the same.
        use console_core::theme::{ART_COLOURS, ART_FIRST, ART_SLOTS};
        let mut colours = *sim.console.video().palette().colours();
        let art = usize::from(ART_FIRST)..usize::from(ART_FIRST) + ART_SLOTS * ART_COLOURS;
        colours[art].fill(0);
        (sim.print(), colours)
    };
    let before = settled(&mut sim);
    for i in 0..50 {
        // Ends with PADTEST, the game played before the first picture
        // (a game's colours stay in its part of the palette, unseen).
        let game = if i % 2 == 0 { "STARDUST" } else { "PADTEST" };
        sim.start(game);
        assert_eq!(sim.console.page(), Page::Game, "round {i}");
        sim.controls.press(RIGHT);
        sim.ticks(3);
        sim.controls.release(RIGHT);
        if game == "STARDUST" && i % 4 == 0 {
            sim.tap(PLUS); // the toy ends itself
            assert_eq!(sim.console.page(), Page::Home);
        } else {
            sim.leave();
        }
        // The selection stays on the game's tile.
        let want = TILES.iter().position(|t| *t == game).unwrap();
        assert_eq!(sim.console.home().selected(), want, "round {i}");
    }
    assert_eq!(sim.console.home().selected(), selected);
    assert_eq!(sim.board.games.started, 51);
    assert_eq!(sim.board.games.ended, 51);
    assert!(!sim.console.game().running());
    assert_eq!(settled(&mut sim), before, "the home screen looks as before");
}

/// A whole scripted run: start each game, play with input and motion,
/// pause, go home. Two runs give the same fingerprint at every frame.
#[test]
fn scripted_play_is_the_same_every_run() {
    let text = "
        46 press right
        47 release right
        48 press right
        49 release right
        50 press right
        51 release right
        52 press right
        53 release right
        54 press right
        55 release right
        56 press right
        57 release right
        58 press right
        59 release right
        60 press right
        61 release right
        62 press south
        63 release south
        64 press west
        64 stick left 20000 -10000
        64 trigger right 600
        64 touch 100 60
        64 tilt 1500 -800
        64 spin 0 160 0
        72 release west
        72 stick left 0 0
        72 trigger right 0
        72 lift
        74 motion off
        82 pad off
        86 pad on
        92 press home
        93 release home
        95 press down
        96 release down
        97 press down
        97 release down
        98 press south
        99 release south
        102 press right
        103 release right
        104 press right
        105 release right
        106 press right
        107 release right
        108 press right
        109 release right
        111 press south
        112 release south
        114 press up
        129 release up
        129 touch 40 200
        154 lift
        156 press plus
        157 release plus
    ";
    let run = |text: &str| {
        let mut console = Box::new(Console::new());
        // Not about the boot: no boot animation (feature `boot-gather`).
        desktop_board::settings::set_boot(&mut console, desktop_board::settings::Boot::Off);
        let mut board = Box::new(DesktopBoard::new(Some(Path::new(SAMPLE_CARD))));
        let run = ScriptRun {
            frames: 170,
            fps: 30,
            events: script::parse(text).unwrap(),
        };
        let mut prints = Vec::new();
        let problems = run_script(&run, &mut board, &mut *console, |_, b, _| {
            prints.push(picture::fingerprint(&b.screen.image))
        });
        (
            prints,
            board.log.lines.clone(),
            problems,
            board.games.started,
            board.games.ended,
        )
    };
    let a = run(text);
    let b = run(text);
    assert!(a.2.is_empty(), "{:?}", a.2);
    assert_eq!((a.3, a.4), (2, 2), "two games started and ended: {:?}", a.1);
    for line in [
        "console: game PADTEST",
        "console: game STARDUST",
        "console: back to home",
        "console: game ended",
    ] {
        assert!(a.1.iter().any(|l| l == line), "no `{line}` in {:?}", a.1);
    }
    assert_eq!(a, b);
}

/// Scripts drive players 2 to 4: `pN pad on` plugs a pad in, `pN press`
/// holds its button. PADTEST shows every player's held buttons, so a
/// press on one player's pad changes the picture, and a press on a pad
/// that is not plugged in changes nothing. Every run is the same.
#[test]
fn a_script_drives_two_and_four_players() {
    // PADTEST is the ninth tile (the eight from BENCH to MERCATOR come first).
    const START: &str = "
        46 press right
        47 release right
        48 press right
        49 release right
        50 press right
        51 release right
        52 press right
        53 release right
        54 press right
        55 release right
        56 press right
        57 release right
        58 press right
        59 release right
        60 press right
        61 release right
        62 press south
        63 release south
    ";
    struct Out {
        prints: Vec<u64>,
        /// Each pad's held buttons and plug at frame 74.
        pads: [(bool, u32); 4],
        log: Vec<String>,
    }
    let run = |text: &str| {
        let mut console = Box::new(Console::new());
        // Not about the boot: no boot animation (feature `boot-gather`).
        desktop_board::settings::set_boot(&mut console, desktop_board::settings::Boot::Off);
        let mut board = Box::new(DesktopBoard::new(Some(Path::new(SAMPLE_CARD))));
        let run = ScriptRun {
            frames: 82,
            fps: 30,
            events: script::parse(&format!("{START}{text}")).unwrap(),
        };
        let mut prints = Vec::new();
        let mut pads = [(false, 0); 4];
        let problems = run_script(&run, &mut board, &mut *console, |t, b, _| {
            prints.push(picture::fingerprint(&b.screen.image));
            if t.frame == 74 {
                pads[0] = (b.pad.state.connected, b.pad.state.buttons);
                for (p, o) in pads[1..].iter_mut().zip(&b.pad.others) {
                    *p = (o.state.connected, o.state.buttons);
                }
            }
        });
        assert!(problems.is_empty(), "{problems:?}");
        Out {
            prints,
            pads,
            log: board.log.lines.clone(),
        }
    };

    // Two players.
    let two = "64 p2 pad on\n";
    let base = run(two);
    assert!(base.log.iter().any(|l| l == "console: game PADTEST"));
    assert_eq!(base.pads, [(true, 0), (true, 0), (false, 0), (false, 0)]);
    let p2 = run(&format!("{two}72 p2 press east\n"));
    assert_eq!(p2.pads[1], (true, btn::EAST));
    assert_eq!(p2.pads[0], (true, 0), "player 1 holds nothing");
    assert_eq!(p2.prints[..72], base.prints[..72]);
    assert_ne!(p2.prints[77], base.prints[77], "player 2's press is drawn");
    // Pad 3 is not plugged in: its press reaches nobody.
    let p3_out = run(&format!("{two}72 p3 press east\n"));
    assert_eq!(p3_out.pads[2], (false, 0));
    assert_eq!(p3_out.prints, base.prints);

    // Four players: each press shows, each in its own place.
    let four = "64 p2 pad on\n65 p3 pad on\n66 p4 pad on\n";
    let base = run(four);
    assert_eq!(base.pads.map(|p| p.0), [true; 4]);
    let mut seen = vec![base.prints[77]];
    for (n, line) in [
        (0, "72 press north"),
        (1, "72 p2 press south"),
        (2, "72 p3 press north"),
        (3, "72 p4 press l"),
    ] {
        let one = run(&format!("{four}{line}\n"));
        for (m, &(_, held)) in one.pads.iter().enumerate() {
            assert_eq!(held != 0, m == n, "`{line}`: pad {m} holds {held:#x}");
        }
        assert!(!seen.contains(&one.prints[77]), "`{line}` drew nothing new");
        seen.push(one.prints[77]);
        // The same every run.
        assert_eq!(one.prints, run(&format!("{four}{line}\n")).prints);
    }
}

/// A `notify` script line shows a notification over a running game, and
/// once it has gone the game's picture is exactly as before.
#[test]
fn a_notify_line_shows_over_a_game_and_the_picture_comes_back() {
    // PADTEST is the ninth tile (the eight from BENCH to MERCATOR come first). Still from
    // frame 72 on: it sleeps.
    let text = "
        46 press right
        47 release right
        48 press right
        49 release right
        50 press right
        51 release right
        52 press right
        53 release right
        54 press right
        55 release right
        56 press right
        57 release right
        58 press right
        59 release right
        60 press right
        61 release right
        62 press south
        63 release south
        112 notify HELLO FROM A SCRIPT
    ";
    let mut console = Box::new(Console::new());
    // Not about the boot: no boot animation (feature `boot-gather`).
    desktop_board::settings::set_boot(&mut console, desktop_board::settings::Boot::Off);
    let mut board = Box::new(DesktopBoard::new(Some(Path::new(SAMPLE_CARD))));
    let run = ScriptRun {
        frames: 322,
        fps: 30,
        events: script::parse(text).unwrap(),
    };
    let mut prints = Vec::new();
    let problems = run_script(&run, &mut board, &mut *console, |_, b, _| {
        prints.push(picture::fingerprint(&b.screen.image));
    });
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(console.page(), Page::Game);
    assert_eq!(console.game().folder(), "PADTEST");
    let before = prints[111];
    assert_eq!(prints[102], before, "the game is still before the notice");
    assert_ne!(prints[117], before, "the notice shows over the game");
    assert_eq!(prints[321], before, "the game's picture is back");
    assert!(
        board.log.lines.iter().all(|l| !l.contains("back to home")),
        "the game ran all the time"
    );
}

/// Home keeps its game list and tile pictures in the RAM the board lends
/// (the chip's game data slot). Every real game start overwrites that RAM
/// (the simulator fills it with rubbish, as the chip loads the game over
/// it), so home must read the card again on the way back: the list comes
/// back whole and clean after every linked game, and while a game runs
/// nothing is lent and home has no list.
#[test]
fn home_reads_its_list_again_after_every_linked_game() {
    let mut sim = Sim::new();
    let lines = |sim: &mut Sim| -> Vec<String> {
        let list = sim.console.home_games(&mut *sim.board);
        list.iter()
            .map(|g| {
                let (f, n, t) = (g.folder.as_str(), g.name.as_str(), g.tag.as_str());
                format!("{f}|{n}|{t}|{:04X}", g.tile)
            })
            .collect()
    };
    let before = lines(&mut sim);
    assert_eq!(before.len(), TILES.len());
    for g in GAMES {
        let fills = sim.board.games.lent.fills;
        sim.start(g.folder);
        assert_eq!(sim.console.page(), Page::Game, "{}", g.folder);
        sim.ticks(3);
        assert!(sim.board.games.lent.loaded(), "{}: nothing lent", g.folder);
        assert!(
            lines(&mut sim).is_empty(),
            "{}: no list in a game",
            g.folder
        );
        sim.leave();
        // Two steps read home again and send nothing; then home is drawn.
        let mut pending = 0;
        while sim.console.home_pending() {
            assert_eq!(sim.console.last_report().areas, 0, "{}", g.folder);
            pending += 1;
            sim.tick();
            assert!(pending < 10, "{}: home never came back", g.folder);
        }
        assert!(pending <= 2, "{}: {pending} steps pending", g.folder);
        assert!(sim.board.games.lent.fills >= fills + 2, "{}", g.folder);
        assert!(!sim.board.games.lent.loaded());
        let after = lines(&mut sim);
        assert_eq!(after, before, "{}: the list after the game", g.folder);
        assert!(after.iter().all(|l| !l.contains('?')), "{after:?}");
    }
}

/// The card goes out on the way back from a real game, in the worst
/// place: after home read its list again and before it read the tile
/// pictures. Home must still be drawn from the right list (never from
/// rubbish), say the card is gone at the next card check, and once the
/// card is back look exactly as a console booted with it.
#[test]
fn a_card_pulled_while_home_is_read_again_heals_when_put_back() {
    fn settle(sim: &mut Sim) {
        for _ in 0..900 {
            let c = &sim.console;
            let busy = c.home_pending()
                || c.home().moving()
                || !c.notices().is_empty()
                || c.home().art_pending();
            if c.page() == Page::Home && !busy {
                break;
            }
            sim.tick();
        }
        assert!(!sim.console.home_pending());
        sim.ticks(3);
    }
    let names = |sim: &mut Sim| -> Vec<String> {
        let list = sim.console.home_games(&mut *sim.board);
        list.iter()
            .map(|g| format!("{}|{}|{:04X}", g.folder.as_str(), g.name.as_str(), g.tile))
            .collect()
    };
    let mut sim = Sim::new();
    // PADTEST has a tile picture on the sample card.
    sim.start("PADTEST");
    sim.leave(); // back on the same tile, settled, as the reference
    settle(&mut sim);
    let before = names(&mut sim);
    let padtest = TILES.iter().position(|&t| t == "PADTEST").unwrap();
    assert!(
        sim.console.home().art().slot(padtest).is_some(),
        "PADTEST's picture"
    );

    sim.tap(SOUTH);
    assert_eq!(sim.console.page(), Page::Game);
    sim.leave();
    while !sim.console.home().has_list() {
        sim.tick();
    }
    assert!(sim.console.home_pending(), "the pictures are still to come");
    sim.board.files.set_card(false);
    sim.tick();
    assert!(!sim.console.home_pending(), "home is drawn");
    assert_eq!(
        names(&mut sim),
        before,
        "the list read before the card went"
    );
    assert!(sim.console.home().art().slot(padtest).is_none());
    sim.ticks(90); // past the next card check
    assert!(names(&mut sim).is_empty(), "no card, no list");
    sim.board.files.set_card(true);
    sim.ticks(90);
    settle(&mut sim);
    assert_eq!(names(&mut sim), before);
    // A card taken out and put back starts at the first tile again (as
    // at home, before lent RAM): the same as a console booted with it.
    let mut fresh = Sim::new();
    settle(&mut fresh);
    assert_eq!(sim.console.home().selected(), 0);
    assert_eq!(sim.print(), fresh.print(), "home looks as booted");
}

/// Fourteen tiles (eleven games, the prototypes DRIVE3D and PROTO3D, and
/// BENCH): the home screen lists all fourteen in order, every tile gets
/// its picture when it is reached, the dots fit, and the lent RAM does
/// not depend on the number of games.
#[test]
fn fourteen_tiles_fit_the_home_screen_its_pictures_and_its_dots() {
    use console_core::{console_video::W, home::MAX_GAMES, lent::LENT_BYTES, theme::ART_SLOTS};
    let mut sim = Sim::new();
    sim.ticks(30);
    let list = sim.console.home_games(&mut *sim.board);
    let names: Vec<&str> = list.iter().map(|g| g.folder.as_str()).collect();
    assert_eq!(names, TILES);
    assert_eq!(sim.console.home().count(), 14);
    // Room for the list, the dots (6 pixels a game), and the RAM home
    // borrows (fixed: room for MAX_GAMES names and ART_SLOTS pictures).
    assert!(TILES.len() <= MAX_GAMES);
    assert!(TILES.len() as i16 * 6 <= W as i16 - 16, "the dots fit");
    assert_eq!(LENT_BYTES, 5_396);
    const { assert!(ART_SLOTS >= 3) };
    // Walk to every tile: each one's picture is kept when it is shown.
    for (i, name) in TILES.iter().enumerate() {
        if i > 0 {
            sim.tap(RIGHT);
        }
        sim.ticks(30);
        assert_eq!(sim.console.home().selected(), i);
        assert!(
            sim.console.home().art().slot(i).is_some(),
            "{name}: no picture"
        );
        assert!(!sim.console.home().art_pending());
    }
    // The last tile is the end of the row.
    sim.tap(RIGHT);
    sim.ticks(30);
    assert_eq!(sim.console.home().selected(), TILES.len() - 1);
    picture::save_png(
        &hockey_shots().join("home-14-tiles.png"),
        &sim.board.screen.image,
        320,
        240,
    )
    .unwrap();
    // And back to the first.
    for _ in 0..TILES.len() {
        sim.tap(LEFT);
    }
    sim.ticks(30);
    assert_eq!(sim.console.home().selected(), 0);
    assert!(sim.console.home().art().slot(0).is_some());
}

/// TUNDRA: its title sleeps, a match runs at 60 frames a second with
/// player 1's pad and player 2's, the pause menu's LEAVE goes home.
#[test]
fn tundra_plays_two_pads_pauses_and_leaves() {
    let mut sim = Sim::new();
    sim.controls.set_connected_by(1, true);
    sim.start("TUNDRA");
    assert_eq!(sim.console.page(), Page::Game);
    assert!(sim.log_has("console: game TUNDRA"));
    assert!(sim.log_has("game: tundra: started"));
    sim.ticks(5);
    assert_eq!(sim.console.game().rate(), rate::ON_DEMAND);
    let title = sim.print();
    picture::save_png(
        &hockey_shots().join("title.png"),
        &sim.board.screen.image,
        320,
        240,
    )
    .unwrap();
    sim.ticks(10);
    assert_eq!(sim.print(), title, "the title waits");
    // PLAY (player 1's A).
    sim.tap(SOUTH);
    sim.ticks(3);
    assert!(
        sim.log_has(
            "game: tundra: match, p1 pad p2 pad p3 cpu p4 cpu, computer EASY, 3 MIN, rate 60"
        ),
        "{:?}",
        sim.board.log.lines
    );
    assert_eq!(sim.console.game().rate(), rate::FPS60);
    // Player 2 skates: the picture differs from a run where it does not.
    let before = sim.console.game().frames();
    sim.ticks(60);
    assert!(
        sim.console.game().frames() - before >= 55,
        "called every tick: the simulator ticks 30 times a second"
    );
    sim.controls.press_by(1, Control::Button(btn::LEFT));
    sim.ticks(20);
    sim.controls.release_by(1, Control::Button(btn::LEFT));
    // Player 2's START pauses; the game sleeps.
    sim.controls.press_by(1, PLUS);
    sim.tick();
    sim.controls.release_by(1, PLUS);
    sim.ticks(3);
    assert!(sim.log_has("game: tundra: pause"));
    assert_eq!(sim.console.game().rate(), rate::ON_DEMAND);
    // LEAVE: the sixth line.
    for _ in 0..5 {
        sim.tap(DOWN);
    }
    sim.tap(SOUTH);
    sim.ticks(5);
    assert!(sim.log_has("game: tundra: leave"));
    assert_eq!(sim.console.page(), Page::Home);
}

/// TUNDRA with no pad 2: a finger on the ice takes player 2.
#[test]
fn tundra_touch_plays_player_two() {
    let mut sim = Sim::new();
    sim.start("TUNDRA");
    sim.ticks(3);
    sim.tap(SOUTH);
    sim.ticks(3);
    assert!(sim.log_has(
        "game: tundra: match, p1 pad p2 cpu p3 cpu p4 cpu, computer EASY, 3 MIN, rate 60"
    ));
    sim.ticks(60);
    // Panel pixels: twice the game's.
    sim.controls.touch(board_api::TouchPoint { x: 220, y: 160 });
    sim.ticks(10);
    sim.controls.touch(board_api::TouchPoint { x: 240, y: 120 });
    sim.ticks(10);
    sim.controls.lift();
    sim.ticks(2);
    assert!(sim.log_has("game: tundra: touch plays player 2"));
    sim.leave();
}

/// SKIMMER: its title sleeps, a match runs at 60 frames a second (two
/// steps a tick here) with player 1's pad on CORAL and player 2's on
/// TEAL, player 2's mallet moves, and the pause menu's LEAVE goes home.
#[test]
fn skimmer_plays_two_pads_pauses_and_leaves() {
    let mut sim = Sim::new();
    sim.controls.set_connected_by(1, true);
    sim.start("SKIMMER");
    assert_eq!(sim.console.page(), Page::Game);
    assert!(sim.log_has("console: game SKIMMER"));
    assert!(sim.log_has("game: skimmer: started"));
    sim.ticks(5);
    assert_eq!(sim.console.game().rate(), rate::ON_DEMAND);
    let title = sim.print();
    picture::save_png(
        &air_hockey_shots().join("title.png"),
        &sim.board.screen.image,
        320,
        240,
    )
    .unwrap();
    sim.ticks(10);
    assert_eq!(sim.print(), title, "the title waits");
    // PLAY (player 1's A on the first line).
    sim.tap(SOUTH);
    sim.ticks(3);
    assert!(
        sim.log_has(
            "game: skimmer: match, coral pad teal pad, computer NORMAL, FIRST TO 7, plain, rate 60"
        ),
        "{:?}",
        sim.board.log.lines
    );
    assert_eq!(sim.console.game().rate(), rate::FPS60);
    let before = sim.console.game().frames();
    sim.ticks(60);
    assert!(
        sim.console.game().frames() - before >= 110,
        "60 calls a second"
    );
    // Player 2 moves TEAL's mallet: the picture differs from the same
    // run where nobody moves. (On its own thread: a linked game keeps its
    // state per thread, so two runs on one thread would share it.)
    let still = std::thread::spawn(|| {
        let mut still = Sim::new();
        still.controls.set_connected_by(1, true);
        still.start("SKIMMER");
        still.ticks(15);
        still.tap(SOUTH);
        still.ticks(3 + 60);
        let at_start = still.print();
        still.ticks(16);
        (at_start, still.print())
    })
    .join()
    .unwrap();
    assert_eq!(still.0, sim.print(), "the same start");
    sim.controls.press_by(1, Control::Button(btn::UP));
    sim.ticks(15);
    sim.controls.release_by(1, Control::Button(btn::UP));
    sim.ticks(1);
    assert_ne!(still.1, sim.print(), "player 2 moved TEAL");
    picture::save_png(
        &air_hockey_shots().join("match.png"),
        &sim.board.screen.image,
        320,
        240,
    )
    .unwrap();
    // Player 2's START pauses; the game sleeps.
    sim.controls.press_by(1, PLUS);
    sim.tick();
    sim.controls.release_by(1, PLUS);
    sim.ticks(3);
    assert!(sim.log_has("game: skimmer: pause"));
    assert_eq!(sim.console.game().rate(), rate::ON_DEMAND);
    // LEAVE: the sixth line (RESUME, COMPUTER, RATE, RESTART, TITLE, LEAVE).
    for _ in 0..5 {
        sim.tap(DOWN);
    }
    sim.tap(SOUTH);
    sim.ticks(5);
    assert!(sim.log_has("game: skimmer: leave"));
    assert_eq!(sim.console.page(), Page::Home);
}

/// SKIMMER with no pad 2: a finger on the right half takes TEAL from
/// the computer.
#[test]
fn skimmer_touch_takes_teal() {
    let mut sim = Sim::new();
    sim.start("SKIMMER");
    sim.ticks(3);
    sim.tap(SOUTH);
    sim.ticks(3);
    assert!(sim.log_has(
        "game: skimmer: match, coral pad teal cpu, computer NORMAL, FIRST TO 7, plain, rate 60"
    ));
    sim.ticks(60);
    assert!(!sim.log_has("game: skimmer: touch plays teal"));
    // Panel pixels: twice the game's (game x 120, y 60: the right half,
    // below the top bar).
    sim.controls.touch(board_api::TouchPoint { x: 240, y: 120 });
    sim.ticks(10);
    sim.controls.touch(board_api::TouchPoint { x: 220, y: 160 });
    sim.ticks(10);
    sim.controls.lift();
    sim.ticks(2);
    assert!(sim.log_has("game: skimmer: touch plays teal"));
    sim.leave();
}
