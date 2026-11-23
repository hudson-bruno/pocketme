//! A test board and a small frame-numbered runner, no window.
//!
//! The board is board-api's fake board, except for the card: board-api's
//! `FakeFiles` does not list folders, and the console's card layout is
//! one folder per game. So `TestCard` lists folders too.
//!
//! The runner works like the simulator's loop (boards/desktop/src/run.rs):
//! time goes in ticks of `FRAME_US`; in each tick the console runs when
//! its wake time falls inside the tick, or when it asked to be woken by
//! input and the input changed. The clock is set to the exact time it
//! asked for. So every run gives the same result.

#![allow(dead_code)]

use board_api::fake::{
    FakeClock, FakeGames, FakeKeys, FakeLink, FakePad, FakeScreen, FakeStore, FakeTouch,
};
use board_api::{
    Board, DirEntry, Files, FsError, GameCallbacks, PadState, Parts, StartError, Stick, TouchPoint,
    Wake, btn,
};
use console_core::game_api::LinkedGame;
use console_core::{Console, FRAME_US};

/// A card made of a fixed list of files: `(path, bytes)`. Folders are
/// found from the paths.
pub struct TestCard {
    pub inserted: bool,
    pub files: &'static [(&'static str, &'static [u8])],
    /// Every call to the card (list, size, read_at), counted: a test can
    /// see when the console reads the card.
    pub calls: u32,
}

impl TestCard {
    fn find(&self, path: &str) -> Result<&'static [u8], FsError> {
        if !self.inserted {
            return Err(FsError::NoCard);
        }
        self.files
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, b)| *b)
            .ok_or(FsError::NotFound)
    }

    /// The entry of `dir` that `path` is in: (name, is a folder).
    fn entry<'a>(dir: &str, path: &'a str) -> Option<(&'a str, bool)> {
        let rest = path.strip_prefix(dir)?.strip_prefix('/')?;
        Some(match rest.split_once('/') {
            Some((name, _)) => (name, true),
            None => (rest, false),
        })
    }
}

impl Files for TestCard {
    fn list<F: FnMut(&DirEntry<'_>) -> bool>(
        &mut self,
        dir: &str,
        mut each: F,
    ) -> Result<(), FsError> {
        self.calls += 1;
        if !self.inserted {
            return Err(FsError::NoCard);
        }
        let dir = dir.trim_end_matches('/');
        let mut found = false;
        for (i, (path, bytes)) in self.files.iter().enumerate() {
            let Some((name, is_dir)) = Self::entry(dir, path) else {
                continue;
            };
            found = true;
            // A folder is listed once, at its first file.
            let seen = self.files[..i]
                .iter()
                .any(|(p, _)| Self::entry(dir, p).map(|e| e.0) == Some(name));
            if seen {
                continue;
            }
            let e = DirEntry {
                name,
                size: if is_dir { 0 } else { bytes.len() as u32 },
                is_dir,
            };
            if !each(&e) {
                break;
            }
        }
        if found || dir.is_empty() {
            Ok(())
        } else {
            Err(FsError::NotFound)
        }
    }

    fn size(&mut self, path: &str) -> Result<u32, FsError> {
        self.calls += 1;
        Ok(self.find(path)?.len() as u32)
    }

    fn read_at(&mut self, path: &str, offset: u32, buf: &mut [u8]) -> Result<usize, FsError> {
        self.calls += 1;
        let b = self.find(path)?;
        let start = (offset as usize).min(b.len());
        let n = buf.len().min(b.len() - start);
        buf[..n].copy_from_slice(&b[start..start + n]);
        Ok(n)
    }
}

/// The log, every line kept (the game tests look for lines).
#[derive(Default)]
pub struct TestLog {
    pub all: Vec<String>,
}

impl TestLog {
    pub fn last(&self) -> &str {
        self.all.last().map_or("", |s| s.as_str())
    }

    pub fn lines(&self) -> &[String] {
        &self.all
    }
}

impl board_api::LogOut for TestLog {
    fn line(&mut self, text: &str) {
        self.all.push(text.to_string());
    }
}

/// The fake board with [`TestCard`] as its card.
pub struct TestBoard {
    pub screen: FakeScreen,
    pub touch: FakeTouch,
    pub keys: FakeKeys,
    pub pad: FakePad,
    pub clock: FakeClock,
    pub files: TestCard,
    pub link: FakeLink,
    pub log: TestLog,
    pub store: FakeStore,
    /// The games "linked in" (empty: this board cannot start games),
    /// and the RAM the board lends while none is loaded.
    pub games: FakeGames,
    #[cfg(feature = "bt-classic")]
    pub hci: board_api::Absent,
    #[cfg(feature = "passby")]
    pub beacon: board_api::Absent,
    #[cfg(feature = "gps")]
    pub gps: board_api::Absent,
    #[cfg(feature = "sound")]
    pub audio: board_api::Absent,
}

impl Board for TestBoard {
    type Screen = FakeScreen;
    type Touch = FakeTouch;
    type Keys = FakeKeys;
    type Pad = FakePad;
    type Clock = FakeClock;
    type Files = TestCard;
    type Link = FakeLink;
    type Log = TestLog;
    type Store = FakeStore;
    #[cfg(feature = "bt-classic")]
    type Hci = board_api::Absent;
    #[cfg(feature = "passby")]
    type Beacon = board_api::Absent;
    #[cfg(feature = "gps")]
    type Gps = board_api::Absent;
    #[cfg(feature = "sound")]
    type Audio = board_api::Absent;

    fn parts(&mut self) -> Parts<'_, Self> {
        self.parts_and_lent().0
    }

    fn parts_and_lent(&mut self) -> (Parts<'_, Self>, Option<&mut [u8]>) {
        let parts = Parts {
            screen: &mut self.screen,
            touch: &mut self.touch,
            keys: &mut self.keys,
            pad: &mut self.pad,
            clock: &mut self.clock,
            files: &mut self.files,
            link: &mut self.link,
            log: &mut self.log,
            store: &mut self.store,
            #[cfg(feature = "bt-classic")]
            hci: &mut self.hci,
            #[cfg(feature = "passby")]
            beacon: &mut self.beacon,
            #[cfg(feature = "gps")]
            gps: &mut self.gps,
            #[cfg(feature = "sound")]
            audio: &mut self.audio,
        };
        (parts, self.games.lent.lend())
    }

    fn start_game(&mut self, folder: &str) -> Result<GameCallbacks, StartError> {
        self.games.start(folder)
    }

    fn end_game(&mut self) {
        self.games.end();
    }
}

// ---- Cards ----------------------------------------------------------------

const BIN: &[u8] = b"GAME";

/// Four made-up games, listed out of order. (Until 2026-11-17 the
/// simulator's sample card had these four too; now it has only games that
/// start, and they stay here as a test card.)
pub const FOUR: &[(&str, &[u8])] = &[
    ("/CONSOLE/GAMES/TRAIL/GAME.BIN", BIN),
    (
        "/CONSOLE/GAMES/TRAIL/INFO.TXT",
        b"NAME=TRAIL\nTAG=USES GPS\nTILE=C28A3A\nNEEDS=GPS\n",
    ),
    ("/CONSOLE/GAMES/SPROUT/GAME.BIN", BIN),
    (
        "/CONSOLE/GAMES/SPROUT/INFO.TXT",
        b"NAME=SPROUT\nTAG=IDLE GAME\nTILE=3C8F4A\n",
    ),
    ("/CONSOLE/GAMES/BLOCKS/GAME.BIN", BIN),
    (
        "/CONSOLE/GAMES/BLOCKS/INFO.TXT",
        b"NAME=BLOCKS\nTAG=PUZZLE\nTILE=3A6BC2\n",
    ),
    ("/CONSOLE/GAMES/COMET/GAME.BIN", BIN),
    (
        "/CONSOLE/GAMES/COMET/INFO.TXT",
        b"NAME=COMET\nTAG=ARCADE\nTILE=20204A\n",
    ),
    // A folder with no game in it: not a tile.
    ("/CONSOLE/GAMES/NOTES/INFO.TXT", b"NAME=NOTES\n"),
];

/// Nine games (more than fit on the screen); most have no INFO.TXT.
pub const NINE: &[(&str, &[u8])] = &[
    ("/CONSOLE/GAMES/ALPHA/GAME.BIN", BIN),
    ("/CONSOLE/GAMES/BRAVO/GAME.BIN", BIN),
    ("/CONSOLE/GAMES/CHARLIE/GAME.BIN", BIN),
    ("/CONSOLE/GAMES/DELTA/GAME.BIN", BIN),
    ("/CONSOLE/GAMES/ECHO/GAME.BIN", BIN),
    ("/CONSOLE/GAMES/FOXTROT/GAME.BIN", BIN),
    ("/CONSOLE/GAMES/GOLF/GAME.BIN", BIN),
    ("/CONSOLE/GAMES/HOTEL/GAME.BIN", BIN),
    ("/CONSOLE/GAMES/INDIA/GAME.BIN", BIN),
    (
        "/CONSOLE/GAMES/INDIA/INFO.TXT",
        b"NAME=INDIA\nTAG=LAST ONE\nTILE=E86AA6\n",
    ),
];

/// A good tile picture (ART.BIN, 36 x 36, 16 colours): colour 0 is
/// `base`, the pixel at (x, y) is colour (x / 3 + y / 3 + seed) % 16, and
/// colour c is `base + c * 0x0841` (RGB565).
pub fn art_file(base: u16, seed: usize) -> Vec<u8> {
    let mut b = b"ART".to_vec();
    b.extend_from_slice(&[1, 36, 36, 16, 4]);
    for c in 0..16u16 {
        b.extend_from_slice(&base.wrapping_add(c * 0x0841).to_le_bytes());
    }
    for y in 0..36 {
        for x in (0..36).step_by(2) {
            let v = |x: usize| ((x / 3 + y / 3 + seed) % 16) as u8;
            b.push((v(x) << 4) | v(x + 1));
        }
    }
    assert_eq!(b.len(), 688);
    b
}

/// A card that lives as long as the test program (made at run time).
pub fn leak_card(files: Vec<(String, Vec<u8>)>) -> &'static [(&'static str, &'static [u8])] {
    let v: Vec<(&'static str, &'static [u8])> = files
        .into_iter()
        .map(|(p, b)| {
            (
                &*Box::leak(p.into_boxed_str()),
                &*Box::leak(b.into_boxed_slice()),
            )
        })
        .collect();
    Box::leak(v.into_boxed_slice())
}

/// `n` games G00, G01, ... each with a GAME.BIN and a good ART.BIN (a
/// different picture each), except the folders in `no_art`.
pub fn art_card(n: usize, no_art: &[usize]) -> &'static [(&'static str, &'static [u8])] {
    let mut files = Vec::new();
    for i in 0..n {
        let dir = format!("/CONSOLE/GAMES/G{i:02}");
        files.push((format!("{dir}/GAME.BIN"), b"GAME".to_vec()));
        if !no_art.contains(&i) {
            files.push((
                format!("{dir}/ART.BIN"),
                art_file(0x1000 + i as u16 * 0x0100, i),
            ));
        }
    }
    leak_card(files)
}

/// A card with no games folder at all.
pub const EMPTY: &[(&str, &[u8])] = &[("/README.TXT", b"hello")];

// ---- Input script ---------------------------------------------------------

/// One thing that happens at the start of a tick.
#[derive(Clone, Copy, Debug)]
pub enum Ev {
    Press(u32),
    Release(u32),
    /// The board's BOOT button down (true) or up.
    Boot(bool),
    /// Finger down at a PICTURE pixel (turned into panel pixels, 2x).
    Touch(u16, u16),
    Lift,
    Stick(i16, i16),
    Pad(bool),
    Card(bool),
    /// A pad button pressed AND let go before the next step (a short
    /// press): the board only noted it (`take_pressed`).
    Quick(u32),
    /// The same for the board's BOOT button.
    QuickBoot,
    /// A touch at a PICTURE pixel that started and ended before the next
    /// step: the board only noted it (`take_tap`).
    QuickTouch(u16, u16),
    /// Board pad 1, 2 or 3 (players 2 to 4) plugged in (true, nothing
    /// held) or out. `Press`, `Release`, `Quick` and `Stick` are pad 0's.
    PadN(u8, bool),
    PressN(u8, u32),
    ReleaseN(u8, u32),
    QuickN(u8, u32),
    StickN(u8, i16, i16),
}

/// What one tick did.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tick {
    pub steps: u32,
    pub presents: u32,
    pub areas: u32,
    pub pixels: u32,
    pub print: u64,
}

impl Tick {
    /// Bytes on the wire for this tick, with the simulator's model:
    /// 2 bytes per panel pixel plus 11 command bytes per area.
    pub fn bytes(&self) -> u32 {
        self.pixels * 2 + self.areas * 11
    }
}

/// The console on the test board, with the loop around it.
pub struct Sim {
    pub console: Box<Console>,
    pub board: Box<TestBoard>,
    pub tick: u64,
    wake: Option<Wake>,
    input_changed: bool,
    /// One entry per tick run.
    pub ticks: Vec<Tick>,
}

impl Sim {
    /// A console with this card (`None`: no card in the slot), a pad
    /// connected, nothing held.
    pub fn new(card: Option<&'static [(&'static str, &'static [u8])]>) -> Sim {
        let board = Box::new(TestBoard {
            screen: FakeScreen::new(),
            touch: FakeTouch::default(),
            keys: FakeKeys::default(),
            pad: FakePad {
                state: PadState {
                    connected: true,
                    ..PadState::NONE
                },
                motion: None,
                pressed: 0,
                ..FakePad::default()
            },
            clock: FakeClock::default(),
            files: TestCard {
                inserted: card.is_some(),
                files: card.unwrap_or(&[]),
                calls: 0,
            },
            link: FakeLink::new(),
            log: TestLog::default(),
            store: FakeStore::new(),
            games: FakeGames::default(),
            #[cfg(feature = "bt-classic")]
            hci: board_api::Absent,
            #[cfg(feature = "passby")]
            beacon: board_api::Absent,
            #[cfg(feature = "gps")]
            gps: board_api::Absent,
            #[cfg(feature = "sound")]
            audio: board_api::Absent,
        });
        Sim {
            console: Box::new(Console::new()),
            board,
            tick: 0,
            wake: None,
            input_changed: false,
            ticks: Vec::new(),
        }
    }

    /// The same, with these games linked into the board.
    pub fn with_games(
        card: Option<&'static [(&'static str, &'static [u8])]>,
        games: &'static [LinkedGame],
    ) -> Sim {
        let mut sim = Sim::new(card);
        sim.board.games.list = games;
        sim
    }

    /// Applies one input event now (before the next tick).
    pub fn input(&mut self, ev: Ev) {
        let b = &mut self.board;
        match ev {
            Ev::Press(m) => b.pad.state.buttons |= m,
            Ev::Release(m) => b.pad.state.buttons &= !m,
            Ev::Boot(down) => b.keys.held = u16::from(down),
            Ev::Touch(x, y) => {
                b.touch.at = Some(TouchPoint {
                    x: x * 2 + 1,
                    y: y * 2 + 1,
                })
            }
            Ev::Lift => b.touch.at = None,
            Ev::Stick(x, y) => b.pad.state.left = Stick { x, y },
            Ev::Pad(on) => {
                b.pad.state = PadState {
                    connected: on,
                    ..PadState::NONE
                }
            }
            Ev::Card(on) => b.files.inserted = on,
            Ev::Quick(m) => b.pad.pressed |= m,
            Ev::QuickBoot => b.keys.pressed |= board_api::key::BOOT,
            Ev::QuickTouch(x, y) => {
                b.touch.tap.get_or_insert(TouchPoint {
                    x: x * 2 + 1,
                    y: y * 2 + 1,
                });
            }
            Ev::PadN(n, on) => {
                b.pad.other(n).state = PadState {
                    connected: on,
                    ..PadState::NONE
                }
            }
            Ev::PressN(n, m) => b.pad.other(n).state.buttons |= m,
            Ev::ReleaseN(n, m) => b.pad.other(n).state.buttons &= !m,
            Ev::QuickN(n, m) => b.pad.other(n).pressed |= m,
            Ev::StickN(n, x, y) => b.pad.other(n).state.left = Stick { x, y },
        }
        self.input_changed = true;
    }

    /// Runs one tick.
    pub fn step_tick(&mut self) -> Tick {
        self.step_tick_by(|console, board| console.step(board))
    }

    /// Runs one tick, stepping the console with `step` (for example
    /// `Console::step_with_sender`, feature `swap`).
    pub fn step_tick_by(
        &mut self,
        mut step: impl FnMut(&mut Console, &mut TestBoard) -> Wake,
    ) -> Tick {
        let start = self.tick * FRAME_US;
        let end = start + FRAME_US;
        let mut t = Tick::default();
        loop {
            let at = match self.wake {
                None => start,
                Some(w) if w.on_input() && self.input_changed => start,
                Some(w) if w.deadline() < end => w.deadline().max(start),
                Some(_) => break,
            };
            assert!(t.steps < 8, "tick {}: the console is stuck", self.tick);
            self.board.clock.now_us = self.board.clock.now_us.max(at);
            let before = self.board.screen.presents;
            self.wake = Some(step(&mut self.console, &mut self.board));
            let n = self.board.screen.presents - before;
            assert!(n <= 1, "two presents in one step");
            if n == 1 {
                let r = self.board.screen.last_report;
                t.areas += u32::from(r.areas);
                t.pixels += r.pixels;
            }
            t.presents += n;
            t.steps += 1;
            self.input_changed = false;
        }
        t.print = fingerprint(&self.board.screen.image);
        self.tick += 1;
        self.ticks.push(t);
        t
    }

    /// A message from outside the console ([`Console::notify`]). The loop
    /// then steps at the next tick, as the board should.
    pub fn notify(&mut self, text: &str) -> bool {
        let now = self.board.clock.now_us;
        let ok = self.console.notify(text, now);
        self.wake = None;
        ok
    }

    /// Runs ticks until tick number `until` (not included), applying the
    /// script's events (absolute tick numbers) as their ticks come.
    pub fn run_until(&mut self, until: u64, script: &[(u64, Ev)]) {
        while self.tick < until {
            for &(at, ev) in script {
                if at == self.tick {
                    self.input(ev);
                }
            }
            self.step_tick();
        }
    }

    /// Runs `n` more ticks with no input.
    pub fn idle(&mut self, n: u64) {
        let until = self.tick + n;
        self.run_until(until, &[]);
    }

    /// A short press of a pad button: down for one tick, then up.
    pub fn tap_button(&mut self, b: u32) {
        self.input(Ev::Press(b));
        self.step_tick();
        self.input(Ev::Release(b));
        self.step_tick();
    }

    /// A short touch at a picture pixel: down for one tick, then up.
    /// Runs the grace ticks too, so the tap (believed
    /// `TOUCH_GRACE` frames after the lift) has come.
    pub fn tap_at(&mut self, x: u16, y: u16) {
        self.input(Ev::Touch(x, y));
        self.step_tick();
        self.input(Ev::Lift);
        self.step_tick();
        self.idle(u64::from(console_core::theme::TOUCH_GRACE));
    }

    /// Runs past the boot screen (it ends by itself).
    pub fn boot(&mut self) {
        while self.console.page() == console_core::Page::Boot {
            self.step_tick();
            assert!(self.tick < 100, "the boot screen never ended");
        }
    }

    /// The picture's pixel (palette number) at a picture position.
    pub fn pixel(&self, x: u16, y: u16) -> u8 {
        self.console.video().frame().get(x, y)
    }
}

/// The simulator's fingerprint (boards/desktop picture::fingerprint):
/// FNV-1a over the panel's RGB565 pixels, low byte first.
pub fn fingerprint(pixels: &[u16]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &p in pixels {
        for byte in p.to_le_bytes() {
            h ^= byte as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    }
    h
}

/// The right buttons, for short names in tests.
pub const RIGHT: u32 = btn::RIGHT;
pub const LEFT: u32 = btn::LEFT;
pub const SOUTH: u32 = btn::SOUTH;
pub const HOME: u32 = btn::HOME;
pub const EAST: u32 = btn::EAST;
pub const DOWN: u32 = btn::DOWN;
pub const UP: u32 = btn::UP;
