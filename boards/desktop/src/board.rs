//! The desktop board: every part the console needs, made of PC things.
//!
//! It is strict about the board's exact limits (owner decision D5, taken:
//! follow the recommendation). What the real board would refuse or cut
//! is written down as a "problem": the run with no window then ends with
//! an error. Numbers the PC cannot know (the time to send the picture)
//! come from a model and are only shown, never refused.

use std::path::{Path, PathBuf};

#[cfg(any(
    feature = "bt-classic",
    feature = "passby",
    feature = "gps",
    feature = "sound"
))]
use board_api::Absent;
use board_api::game::{GameCallbacks, StartError};
use board_api::link::{LOG_LINE_MAX, SECTOR};
use board_api::{
    Board, ByteOrder, Clock, Dir, DirEntry, Files, FsError, Keys, LogOut, Motion, Pad, PadState,
    Parts, PixelSource, Rect, Screen, SendReport, Store, StoreError, Touch, TouchPoint,
    files::PATH_MAX,
};
use game_api::LinkedGame;

use crate::link::SimLink;

/// The panel: the CYD's 320 x 240.
pub const PANEL_W: u16 = 320;
pub const PANEL_H: u16 = 240;
const PIXELS: usize = PANEL_W as usize * PANEL_H as usize;

/// Most areas the board takes in one `present` (console-video's one
/// setting; the real board will settle 8 or 16).
pub const MAX_AREAS: usize = console_video::MAX_AREAS;

/// Command bytes the panel needs to start one area: set the columns
/// (1 + 4 bytes), set the rows (1 + 4), start writing (1). From the
/// design page's model.
pub const AREA_COMMAND_BYTES: u32 = 11;

/// The send-time model: the board's numbers at 40 MHz, a full frame in
/// 30.96 ms, plus 0.1 ms for each area. A model, not a measurement.
pub const FULL_FRAME_US: u64 = 30_960;
pub const PER_AREA_US: u32 = 100;

/// The screen: a 320x240 RGB565 picture in the PC's memory, holding what
/// the panel would show. Only the presented areas change, so a wrong
/// list of changed areas shows up as a stale picture, as on the panel.
pub struct DesktopScreen {
    /// RGB565 pixels, row after row.
    pub image: Vec<u16>,
    /// How many times `present` was called, in all.
    pub presents: u32,
    /// The areas of the last `present` (for the overlay).
    pub last_areas: Vec<Rect>,
    pub last_report: SendReport,
    /// Bytes a real panel would have been sent by the last `present`:
    /// 2 per pixel, plus the command bytes of each area.
    pub last_bytes: u32,
    /// Presents whose modelled send time did not fit in one frame.
    pub over_budget: u32,
    /// What the real board would refuse (see the file's top).
    pub problems: Vec<String>,
    /// The backlight value last asked for (`None` before the first
    /// call). The window dims the picture by it (see [`window_dim`]); the
    /// picture itself (screenshots, fingerprints) is not changed.
    pub backlight: Option<u8>,
    /// How many times the backlight was set, in all.
    pub backlight_calls: u32,
}

impl DesktopScreen {
    fn new() -> Self {
        DesktopScreen {
            image: vec![0; PIXELS],
            presents: 0,
            last_areas: Vec::new(),
            last_report: SendReport::default(),
            last_bytes: 0,
            over_budget: 0,
            problems: Vec::new(),
            backlight: None,
            backlight_calls: 0,
        }
    }

    /// One pixel of the picture.
    pub fn pixel(&self, x: u16, y: u16) -> u16 {
        self.image[usize::from(y) * usize::from(PANEL_W) + usize::from(x)]
    }
}

/// How much the window darkens the picture for a backlight value: the
/// factor (0 to 255) its colours are multiplied by.
///
/// The backlight value is light (twice the value, twice the light). A
/// PC screen's colour numbers are not: the light grows about as the
/// number to the power 2.2. So the factor is 255 x (value / 255)^(1 /
/// 2.2), and the window gives off about the share of light the panel
/// would. Before the first backlight call the picture shows at full:
/// the console sets it right after its first picture, in the same step,
/// so the window never shows a frame before it.
pub fn window_dim(backlight: Option<u8>) -> u8 {
    let Some(v) = backlight else {
        return 255;
    };
    let share = f64::from(v) / 255.0;
    (255.0 * share.powf(1.0 / 2.2)).round() as u8
}

/// The modelled time to send `pixels` in `areas` windows.
pub fn send_time_us(pixels: u32, areas: u16) -> u32 {
    (u64::from(pixels) * FULL_FRAME_US / PIXELS as u64) as u32 + PER_AREA_US * u32::from(areas)
}

impl Screen for DesktopScreen {
    const PANEL_W: u16 = PANEL_W;
    const PANEL_H: u16 = PANEL_H;

    fn present<S: PixelSource + ?Sized>(&mut self, src: &S, areas: &[Rect]) -> SendReport {
        if areas.len() > MAX_AREAS {
            self.problems.push(format!(
                "screen: {} areas in one present; the board takes at most {MAX_AREAS}",
                areas.len()
            ));
        }
        let panel = Rect::new(0, 0, PANEL_W, PANEL_H);
        let mut pixels = 0u32;
        let mut sent = 0u16;
        self.last_areas.clear();
        for a in areas.iter().filter(|a| !a.is_empty()) {
            let inside = a.intersect(&panel);
            if inside != *a {
                self.problems.push(format!(
                    "screen: area {a:?} goes outside the {PANEL_W}x{PANEL_H} panel"
                ));
            }
            if inside.is_empty() {
                continue;
            }
            let w = usize::from(PANEL_W);
            for y in inside.y..inside.bottom() {
                let start = usize::from(y) * w + usize::from(inside.x);
                let row = &mut self.image[start..start + usize::from(inside.w)];
                src.run(inside.x, y, Dir::Right, ByteOrder::Native, row);
            }
            pixels += inside.area();
            sent += 1;
            self.last_areas.push(inside);
        }
        self.presents += 1;
        let send_us = send_time_us(pixels, sent);
        if u64::from(send_us) > console_core::FRAME_US {
            self.over_budget += 1;
        }
        self.last_bytes = pixels * 2 + AREA_COMMAND_BYTES * u32::from(sent);
        self.last_report = SendReport {
            areas: sent,
            pixels,
            send_us,
            wait_us: 0,
        };
        self.last_report
    }

    fn backlight(&mut self, level: u8) {
        self.backlight = Some(level);
        self.backlight_calls += 1;
    }
}

/// The clock: moved by the board's loop, never read from the PC.
#[derive(Default)]
pub struct SimClock {
    pub now_us: u64,
}

impl Clock for SimClock {
    fn now_us(&self) -> u64 {
        self.now_us
    }
}

/// Touch: the mouse, or a script.
#[derive(Default)]
pub struct SimTouch {
    /// The finger now.
    pub at: Option<TouchPoint>,
    /// Where a touch started that the console has not taken yet (it may
    /// be over already). See `board_api::input`.
    pub tap: Option<TouchPoint>,
}

impl Touch for SimTouch {
    fn read(&mut self) -> Option<TouchPoint> {
        self.at
    }

    fn take_tap(&mut self) -> Option<TouchPoint> {
        self.tap.take()
    }
}

/// The BOOT button: a key of the keyboard, or a script.
#[derive(Default)]
pub struct SimKeys {
    /// Held now.
    pub held: u16,
    /// Pressed since the console last took them (maybe up again).
    pub pressed: u16,
}

impl Keys for SimKeys {
    fn held(&mut self) -> u16 {
        self.held
    }

    fn take_pressed(&mut self) -> u16 {
        core::mem::take(&mut self.pressed)
    }
}

/// One gamepad slot past the first (pads 2 to 4): what it holds now.
#[derive(Clone, Copy, Debug, Default)]
pub struct OtherPad {
    pub state: PadState,
    /// Its motion sensor, while it sends motion.
    pub motion: Option<Motion>,
    /// Buttons pressed since the console last took them.
    pub pressed: u32,
}

/// One pad's parts, to change them in place (see [`SimPad::slot`]).
pub struct PadSlot<'a> {
    pub state: &'a mut PadState,
    pub motion: &'a mut Option<Motion>,
    pub pressed: &'a mut u32,
}

/// The four gamepads: the keyboard's (and the number pad's), or a
/// script's. Pad 0 (player 1) is the fields here, pads 1 to 3 are
/// `others`.
#[derive(Default)]
pub struct SimPad {
    pub state: PadState,
    /// Its motion sensor, while it sends motion (see `controls.rs`: the
    /// keyboard's stand-in for tilting a pad).
    pub motion: Option<Motion>,
    /// Buttons pressed since the console last took them (maybe up
    /// again).
    pub pressed: u32,
    /// Pads 1 to 3 (players 2 to 4).
    pub others: [OtherPad; 3],
}

impl SimPad {
    /// Pad `n` (0 to 3) to change in place. Panics past 3.
    pub fn slot(&mut self, n: usize) -> PadSlot<'_> {
        match n {
            0 => PadSlot {
                state: &mut self.state,
                motion: &mut self.motion,
                pressed: &mut self.pressed,
            },
            _ => {
                let o = &mut self.others[n - 1];
                PadSlot {
                    state: &mut o.state,
                    motion: &mut o.motion,
                    pressed: &mut o.pressed,
                }
            }
        }
    }
}

impl Pad for SimPad {
    fn read(&mut self, index: u8) -> PadState {
        if usize::from(index) < 4 {
            *self.slot(usize::from(index)).state
        } else {
            PadState::NONE
        }
    }

    fn motion(&mut self, index: u8) -> Option<Motion> {
        if usize::from(index) < 4 {
            *self.slot(usize::from(index)).motion
        } else {
            None
        }
    }

    fn take_pressed(&mut self, index: u8) -> u32 {
        if usize::from(index) >= 4 {
            return 0;
        }
        let slot = self.slot(usize::from(index));
        let p = core::mem::take(slot.pressed);
        if slot.state.connected { p } else { 0 }
    }
}

/// The games this board can start: linked into the program (see
/// `games.rs`), found by their card folder.
pub struct LinkedGames {
    /// The list (empty: this board cannot start games, as before games
    /// existed).
    pub list: &'static [LinkedGame],
    /// Games started so far.
    pub started: u32,
    /// Calls of `end_game` so far.
    pub ended: u32,
    /// The RAM lent to the console while no game is loaded (the chip's
    /// game data slot).
    pub lent: LentRam,
}

impl Default for LinkedGames {
    fn default() -> Self {
        LinkedGames {
            list: crate::games::GAMES,
            started: 0,
            ended: 0,
            lent: LentRam::new(),
        }
    }
}

/// Bytes the board lends: as many as the chip's game data slot.
pub const LENT_SIZE: usize = game_api::layout::DATA_SLOT_SIZE as usize;

/// The RAM the board lends the console while no game is loaded (the
/// rules: `board_api::lent`). On the chip it is the game data slot: the
/// console's home screen keeps its game list and tile pictures there,
/// and a game start loads the game over them.
///
/// The simulator is stricter than the chip, on purpose: the RAM starts as
/// rubbish, and every `start_game` call (good or not) and every
/// `end_game` fills it with other rubbish. A console that read its list
/// from here after a game, without reading the card again, would show
/// wrong names in a test instead of the right ones by luck. The rubbish
/// is the same in every run (a fixed xorshift), so pictures stay
/// repeatable.
pub struct LentRam {
    bytes: Vec<u8>,
    /// A game is loaded: nothing is lent.
    loaded: bool,
    /// How many times the bytes were filled with rubbish.
    pub fills: u32,
}

impl Default for LentRam {
    fn default() -> Self {
        Self::new()
    }
}

impl LentRam {
    pub fn new() -> Self {
        let mut me = LentRam {
            bytes: vec![0; LENT_SIZE],
            loaded: false,
            fills: 0,
        };
        me.fill();
        me
    }

    /// The bytes, while no game is loaded.
    pub fn lend(&mut self) -> Option<&mut [u8]> {
        if self.loaded {
            return None;
        }
        Some(&mut self.bytes)
    }

    /// Whether a game is loaded (nothing lent).
    pub fn loaded(&self) -> bool {
        self.loaded
    }

    /// A start was asked for: the loan ends, and the bytes are rubbish
    /// (the chip loads the game over them, or wipes them after a load
    /// that failed). `loaded`: the game started.
    fn start(&mut self, loaded: bool) {
        self.fill();
        self.loaded = loaded;
    }

    /// The game ended: lent again, holding the game's leftovers.
    fn end(&mut self) {
        self.fill();
        self.loaded = false;
    }

    fn fill(&mut self) {
        self.fills += 1;
        let mut x = 0x2545_F491u32 ^ self.fills.wrapping_mul(0x9E37_79B9);
        for b in self.bytes.iter_mut() {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *b = x as u8;
        }
    }
}

/// The SD card: a folder on the PC with the card's layout. With no
/// folder, it behaves like an empty slot. Read only, as the console
/// does not write to the card yet.
#[derive(Default)]
pub struct FolderFiles {
    /// The folder standing in for the card, while the card is in.
    pub root: Option<PathBuf>,
    /// The folder while the card is out (see [`FolderFiles::set_card`]).
    pulled: Option<PathBuf>,
    /// Names in the folder the card could not hold (not 8.3), skipped.
    pub skipped: Vec<String>,
}

/// Whether `name` is an 8.3 name in capitals, as the card needs: up to
/// 8 letters, digits, `_` or `-`, then an optional dot and up to 3 more.
pub fn is_8_3(name: &str) -> bool {
    let (base, ext) = name.split_once('.').unwrap_or((name, ""));
    let ok = |s: &str, max: usize| {
        s.len() <= max
            && s.bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    };
    !base.is_empty() && ok(base, 8) && ok(ext, 3) && (ext.is_empty() == !name.contains('.'))
}

impl FolderFiles {
    /// Puts the card in (true) or pulls it out (false).
    pub fn set_card(&mut self, inserted: bool) {
        if inserted {
            if self.root.is_none() {
                self.root = self.pulled.take();
            }
        } else if let Some(r) = self.root.take() {
            self.pulled = Some(r);
        }
    }

    /// The PC path of a card path, after checking the card's rules.
    fn pc_path(&self, path: &str) -> Result<PathBuf, FsError> {
        let root = self.root.as_deref().ok_or(FsError::NoCard)?;
        if path.len() > PATH_MAX || !path.starts_with('/') {
            return Err(FsError::BadName);
        }
        let mut out = root.to_path_buf();
        for part in path.split('/').filter(|p| !p.is_empty()) {
            if !is_8_3(part) {
                return Err(FsError::BadName);
            }
            out.push(part);
        }
        Ok(out)
    }
}

fn io_error(e: std::io::Error) -> FsError {
    match e.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory => FsError::NotFound,
        _ => FsError::Io,
    }
}

impl Files for FolderFiles {
    fn list<F: FnMut(&DirEntry<'_>) -> bool>(
        &mut self,
        dir: &str,
        mut each: F,
    ) -> Result<(), FsError> {
        let pc = self.pc_path(dir)?;
        // Sorted by name, so every run lists in the same order. (The real
        // card lists in the order the files were written.)
        let mut entries: Vec<(String, u32, bool)> = Vec::new();
        for e in std::fs::read_dir(&pc).map_err(io_error)? {
            let e = e.map_err(io_error)?;
            let name = e.file_name().to_string_lossy().into_owned();
            if !is_8_3(&name) {
                if !self.skipped.contains(&name) {
                    self.skipped.push(name);
                }
                continue; // the card could not hold this name
            }
            let meta = e.metadata().map_err(io_error)?;
            let size = if meta.is_dir() {
                0
            } else {
                meta.len().min(u64::from(u32::MAX)) as u32
            };
            entries.push((name, size, meta.is_dir()));
        }
        entries.sort();
        for (name, size, is_dir) in &entries {
            if !each(&DirEntry {
                name,
                size: *size,
                is_dir: *is_dir,
            }) {
                break;
            }
        }
        Ok(())
    }

    fn size(&mut self, path: &str) -> Result<u32, FsError> {
        let meta = std::fs::metadata(self.pc_path(path)?).map_err(io_error)?;
        // A folder is not a file. (Linux gives folders a size of 4096,
        // so a folder named GAME.BIN looked like a game; a real card
        // driver says "not found" for a file that is a folder.)
        if meta.is_dir() {
            return Err(FsError::NotFound);
        }
        Ok(meta.len().min(u64::from(u32::MAX)) as u32)
    }

    fn read_at(&mut self, path: &str, offset: u32, buf: &mut [u8]) -> Result<usize, FsError> {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = std::fs::File::open(self.pc_path(path)?).map_err(io_error)?;
        if f.metadata().map_err(io_error)?.is_dir() {
            return Err(FsError::NotFound);
        }
        f.seek(SeekFrom::Start(u64::from(offset)))
            .map_err(io_error)?;
        let mut n = 0;
        while n < buf.len() {
            match f.read(&mut buf[n..]).map_err(io_error)? {
                0 => break,
                k => n += k,
            }
        }
        Ok(n)
    }
}

/// The log: printed to the terminal with the frame number (when `echo`
/// is on), and kept in memory.
#[derive(Default)]
pub struct SimLog {
    pub echo: bool,
    /// Every line, as the console wrote it (cut to the board's limit).
    pub lines: Vec<String>,
    /// The frame now (set by the loop), printed in front of each line.
    pub frame: u64,
    /// What the real board would refuse (see the file's top).
    pub problems: Vec<String>,
}

impl LogOut for SimLog {
    fn line(&mut self, text: &str) {
        let mut text = text;
        if text.len() > LOG_LINE_MAX {
            self.problems.push(format!(
                "log: a line of {} bytes; the board cuts lines at {LOG_LINE_MAX}",
                text.len()
            ));
            let mut end = LOG_LINE_MAX;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text = &text[..end];
        }
        if self.echo {
            println!("[frame {:5}] {text}", self.frame);
        }
        self.lines.push(text.to_string());
    }
}

/// Saved settings: a few sectors of fake flash, kept in a file (or only
/// in memory, for tests and repeatable runs).
pub struct FileStore {
    pub bytes: Vec<u8>,
    /// The file it is saved to after every change, if any.
    pub path: Option<PathBuf>,
}

/// Size of the store: 4 sectors, 16 KB. (The board's partition size is
/// for boards/cyd to settle.)
pub const STORE_SIZE: usize = 4 * SECTOR as usize;

impl Default for FileStore {
    fn default() -> Self {
        FileStore {
            bytes: vec![0xFF; STORE_SIZE],
            path: None,
        }
    }
}

impl FileStore {
    /// The store kept in `path`. A missing file starts erased (all 0xFF).
    /// A file of another size is refused.
    pub fn open(path: &Path) -> Result<FileStore, String> {
        let bytes = match std::fs::read(path) {
            Ok(b) if b.len() == STORE_SIZE => b,
            Ok(b) => {
                return Err(format!(
                    "{} has {} bytes; a store file has {STORE_SIZE}",
                    path.display(),
                    b.len()
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![0xFF; STORE_SIZE],
            Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
        };
        Ok(FileStore {
            bytes,
            path: Some(path.to_path_buf()),
        })
    }

    fn range(&self, offset: u32, len: usize) -> Result<std::ops::Range<usize>, StoreError> {
        let start = offset as usize;
        match start.checked_add(len) {
            Some(end) if end <= self.bytes.len() => Ok(start..end),
            _ => Err(StoreError::OutOfRange),
        }
    }

    fn save(&self) -> Result<(), StoreError> {
        match &self.path {
            Some(p) => std::fs::write(p, &self.bytes).map_err(|_| StoreError::Io),
            None => Ok(()),
        }
    }
}

impl Store for FileStore {
    fn size(&self) -> u32 {
        self.bytes.len() as u32
    }
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), StoreError> {
        let r = self.range(offset, buf.len())?;
        buf.copy_from_slice(&self.bytes[r]);
        Ok(())
    }
    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), StoreError> {
        if !offset.is_multiple_of(4) || !data.len().is_multiple_of(4) {
            return Err(StoreError::OutOfRange);
        }
        let r = self.range(offset, data.len())?;
        for (b, &d) in self.bytes[r].iter_mut().zip(data) {
            *b &= d; // like flash: bits only go from 1 to 0
        }
        self.save()
    }
    fn erase_sector(&mut self, offset: u32) -> Result<(), StoreError> {
        if !offset.is_multiple_of(SECTOR) {
            return Err(StoreError::OutOfRange);
        }
        let r = self.range(offset, SECTOR as usize)?;
        self.bytes[r].fill(0xFF);
        self.save()
    }
}

/// The desktop board.
pub struct DesktopBoard {
    pub screen: DesktopScreen,
    pub touch: SimTouch,
    pub keys: SimKeys,
    pub pad: SimPad,
    pub clock: SimClock,
    pub files: FolderFiles,
    /// The debug link: a TCP port, or unplugged (see `link.rs`).
    pub link: SimLink,
    pub log: SimLog,
    pub store: FileStore,
    /// The games linked into the program.
    pub games: LinkedGames,
    // No radio, GPS or sound on the PC: each optional part is absent.
    #[cfg(feature = "bt-classic")]
    pub hci: Absent,
    #[cfg(feature = "passby")]
    pub beacon: Absent,
    #[cfg(feature = "gps")]
    pub gps: Absent,
    #[cfg(feature = "sound")]
    pub audio: Absent,
}

impl DesktopBoard {
    /// A board with a black screen, no input, the clock at 0, the link
    /// unplugged, the store in memory, and the card folder `card`
    /// (`None`: no card).
    pub fn new(card: Option<&Path>) -> Self {
        DesktopBoard {
            screen: DesktopScreen::new(),
            touch: SimTouch::default(),
            keys: SimKeys::default(),
            pad: SimPad::default(),
            clock: SimClock::default(),
            files: FolderFiles {
                root: card.map(Path::to_path_buf),
                ..FolderFiles::default()
            },
            link: SimLink::default(),
            log: SimLog::default(),
            store: FileStore::default(),
            games: LinkedGames::default(),
            #[cfg(feature = "bt-classic")]
            hci: Absent,
            #[cfg(feature = "passby")]
            beacon: Absent,
            #[cfg(feature = "gps")]
            gps: Absent,
            #[cfg(feature = "sound")]
            audio: Absent,
        }
    }

    /// Takes every problem found so far (see the file's top).
    pub fn take_problems(&mut self) -> Vec<String> {
        let mut all = std::mem::take(&mut self.screen.problems);
        all.append(&mut self.log.problems);
        all
    }
}

impl Board for DesktopBoard {
    type Screen = DesktopScreen;
    type Touch = SimTouch;
    type Keys = SimKeys;
    type Pad = SimPad;
    type Clock = SimClock;
    type Files = FolderFiles;
    type Link = SimLink;
    type Log = SimLog;
    type Store = FileStore;
    #[cfg(feature = "bt-classic")]
    type Hci = Absent;
    #[cfg(feature = "passby")]
    type Beacon = Absent;
    #[cfg(feature = "gps")]
    type Gps = Absent;
    #[cfg(feature = "sound")]
    type Audio = Absent;

    fn parts(&mut self) -> Parts<'_, Self> {
        self.parts_and_lent().0
    }

    /// The parts, and the lent RAM while no game is loaded (see
    /// [`LentRam`]).
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

    /// Finds the game by its folder in the linked list. As on the chip,
    /// with the card out there is no game to start. Every call ends the
    /// loan of the lent RAM and fills it with rubbish, as the chip's
    /// loader writes over the slot.
    fn start_game(&mut self, folder: &str) -> Result<GameCallbacks, StartError> {
        let got = self.open_game(folder);
        self.games.lent.start(got.is_ok());
        if got.is_ok() {
            self.games.started += 1;
        }
        got
    }

    fn end_game(&mut self) {
        self.games.ended += 1;
        self.games.lent.end();
    }
}

impl DesktopBoard {
    /// The game's table, from the linked list (see `start_game`).
    fn open_game(&self, folder: &str) -> Result<GameCallbacks, StartError> {
        if self.games.list.is_empty() {
            return Err(StartError::NotSupported);
        }
        if self.files.root.is_none() {
            return Err(StartError::NotFound);
        }
        let game = self
            .games
            .list
            .iter()
            .find(|g| g.folder == folder)
            .ok_or(StartError::NotFound)?;
        Ok(game.open(&game_api::UNWIND_CONSOLE_API)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_card_can_hold() {
        assert!(is_8_3("SNAKE.BIN"));
        assert!(is_8_3("GAMES"));
        assert!(is_8_3("A-B_1.X"));
        assert!(!is_8_3("snake.bin"), "small letters");
        assert!(!is_8_3("LONGNAME1.BIN"), "9 letters");
        assert!(!is_8_3("A.BINX"));
        assert!(!is_8_3(".BIN"));
        assert!(!is_8_3("A."), "a dot with nothing after it");
        assert!(!is_8_3("A.B.C"));
        assert!(!is_8_3("MY GAME"));
    }

    #[test]
    fn the_model_send_time() {
        assert_eq!(send_time_us(PIXELS as u32, 1), 30_960 + 100);
        assert_eq!(send_time_us(0, 0), 0);
    }

    #[test]
    fn the_store_behaves_like_flash_and_keeps_a_file() {
        let dir = std::env::temp_dir().join(format!("pocketme-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("store.bin");
        let _ = std::fs::remove_file(&path);
        let mut s = FileStore::open(&path).unwrap();
        assert_eq!(s.size() as usize, STORE_SIZE);
        s.write(8, &[0x0F, 0xF0, 0x12, 0x34]).unwrap();
        s.write(8, &[0xFF, 0x0F, 0xFF, 0xFF]).unwrap();
        assert_eq!(s.write(2, &[0; 4]), Err(StoreError::OutOfRange));
        assert_eq!(s.erase_sector(100), Err(StoreError::OutOfRange));
        assert_eq!(
            s.read(STORE_SIZE as u32 - 2, &mut [0; 4]),
            Err(StoreError::OutOfRange)
        );
        // A second open sees what the first one wrote.
        let mut again = FileStore::open(&path).unwrap();
        let mut b = [0; 4];
        again.read(8, &mut b).unwrap();
        assert_eq!(b, [0x0F, 0x00, 0x12, 0x34], "bits only go from 1 to 0");
        again.erase_sector(0).unwrap();
        again.read(8, &mut b).unwrap();
        assert_eq!(b, [0xFF; 4]);
        std::fs::write(&path, [0u8; 10]).unwrap();
        assert!(FileStore::open(&path).err().unwrap().contains("10 bytes"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn long_log_lines_are_cut_and_reported() {
        let mut log = SimLog::default();
        log.line(&"é".repeat(150)); // 300 bytes, 2 per letter
        assert_eq!(log.lines[0].len(), LOG_LINE_MAX);
        assert_eq!(log.problems.len(), 1);
        log.line("short");
        assert_eq!(log.problems.len(), 1);
    }
}
