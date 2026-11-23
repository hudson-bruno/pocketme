//! A tiny fake board for tests on the PC (feature `testing`).
//!
//! Every part is a plain struct with public fields, so a test sets the
//! input, moves the clock, calls the console's `step`, and then looks at
//! the picture, the log or the link. Nothing reads the PC's real clock,
//! so every run gives the same result.
//!
//! Memory: the picture alone is 320 x 240 x 2 = 153,600 bytes. Fine on
//! the PC (a test thread has 2 MB of stack); never meant for the chip.

use crate::{
    Board, Parts,
    files::{DirEntry, Files, FsError, NAME_MAX},
    game::{GameCallbacks, StartError},
    geom::Rect,
    input::{Keys, Motion, Pad, PadState, Touch, TouchPoint},
    link::{LOG_LINE_MAX, Link, LogOut, SECTOR, Store, StoreError},
    screen::{ByteOrder, Dir, PixelSource, Screen, SendReport},
    time::Clock,
};

/// The fake panel's size: the CYD's.
pub const FAKE_W: u16 = 320;
pub const FAKE_H: u16 = 240;
const PIXELS: usize = FAKE_W as usize * FAKE_H as usize;

/// How the fake screen asks the source for pixels. All must give the
/// same picture; tests use this to check that a source is right in
/// every direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Walk {
    /// Rows, left to right (like the PC board).
    Rows,
    /// Rows, right to left.
    RowsLeft,
    /// Columns, top to bottom (like the CYD's tear-free way).
    Columns,
    /// Columns, bottom to top.
    ColumnsUp,
    /// The indexed fast path, if the source offers it; rows if not.
    Indexed,
}

/// A 320x240 picture in RAM that records what it was asked to send.
pub struct FakeScreen {
    /// The panel's pixels, RGB565, row after row (always native order).
    pub image: [u16; PIXELS],
    pub walk: Walk,
    /// The byte order asked of the source (turned back to native when
    /// stored, so `image` is the same either way).
    pub order: ByteOrder,
    /// How many times `present` was called.
    pub presents: u32,
    /// The areas of the last `present` (the first 32 of them).
    pub last_areas: [Rect; 32],
    /// How many areas the last `present` had (may be more than 32).
    pub last_area_count: usize,
    pub last_report: SendReport,
    /// Whether the last present was given the indexed fast path.
    pub last_was_indexed: bool,
    /// The backlight calls (the first 32): the level, and how many times
    /// `present` had been called before it.
    pub backlight_log: [(u8, u32); 32],
    /// How many times `backlight` was called (may be more than 32).
    pub backlight_calls: usize,
    /// The level of the last backlight call (0 before the first).
    pub backlight: u8,
}

impl FakeScreen {
    pub const fn new() -> Self {
        FakeScreen {
            image: [0; PIXELS],
            walk: Walk::Rows,
            order: ByteOrder::Native,
            presents: 0,
            last_areas: [Rect::EMPTY; 32],
            last_area_count: 0,
            last_report: SendReport {
                areas: 0,
                pixels: 0,
                send_us: 0,
                wait_us: 0,
            },
            last_was_indexed: false,
            backlight_log: [(0, 0); 32],
            backlight_calls: 0,
            backlight: 0,
        }
    }

    pub fn pixel(&self, x: u16, y: u16) -> u16 {
        self.image[usize::from(y) * usize::from(FAKE_W) + usize::from(x)]
    }

    fn put(&mut self, x: u16, y: u16, c: u16) {
        let c = match self.order {
            ByteOrder::Native => c,
            ByteOrder::Wire => u16::from_be(c),
        };
        self.image[usize::from(y) * usize::from(FAKE_W) + usize::from(x)] = c;
    }

    fn send_area<S: PixelSource + ?Sized>(&mut self, src: &S, a: Rect) {
        let mut buf = [0u16; FAKE_W as usize];
        match self.walk {
            Walk::Rows | Walk::RowsLeft | Walk::Indexed => {
                let left = self.walk == Walk::RowsLeft;
                for y in a.y..a.bottom() {
                    let run = &mut buf[..usize::from(a.w)];
                    if left {
                        src.run(a.right() - 1, y, Dir::Left, self.order, run);
                    } else {
                        src.run(a.x, y, Dir::Right, self.order, run);
                    }
                    for (i, &c) in run.iter().enumerate() {
                        let i = i as u16;
                        let x = if left { a.right() - 1 - i } else { a.x + i };
                        self.put(x, y, c);
                    }
                }
            }
            Walk::Columns | Walk::ColumnsUp => {
                let up = self.walk == Walk::ColumnsUp;
                for x in a.x..a.right() {
                    let run = &mut buf[..usize::from(a.h)];
                    if up {
                        src.run(x, a.bottom() - 1, Dir::Up, self.order, run);
                    } else {
                        src.run(x, a.y, Dir::Down, self.order, run);
                    }
                    for (i, &c) in run.iter().enumerate() {
                        let i = i as u16;
                        let y = if up { a.bottom() - 1 - i } else { a.y + i };
                        self.put(x, y, c);
                    }
                }
            }
        }
    }

    /// Fills an area from the indexed fast path. Returns false if the
    /// source has none.
    fn send_indexed<S: PixelSource + ?Sized>(&mut self, src: &S, a: Rect) -> bool {
        let Some(ix) = src.indexed() else {
            return false;
        };
        let k = u16::from(ix.scale.max(1));
        let pic = Rect::new(ix.left, ix.top, ix.width * k, ix.height * k);
        for y in a.y..a.bottom() {
            for x in a.x..a.right() {
                let c = if pic.contains(x, y) {
                    let px = (x - ix.left) / k;
                    let py = (y - ix.top) / k;
                    let i = usize::from(py) * usize::from(ix.width) + usize::from(px);
                    ix.palette[usize::from(ix.pixels[i])]
                } else {
                    ix.border
                };
                // `put` expects the source's order.
                self.put(x, y, self.order.apply(c));
            }
        }
        true
    }
}

impl Default for FakeScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for FakeScreen {
    const PANEL_W: u16 = FAKE_W;
    const PANEL_H: u16 = FAKE_H;

    fn present<S: PixelSource + ?Sized>(&mut self, src: &S, areas: &[Rect]) -> SendReport {
        let panel = Rect::new(0, 0, FAKE_W, FAKE_H);
        let mut pixels = 0u32;
        let mut sent = 0u16;
        self.last_was_indexed = false;
        for (i, &a) in areas.iter().enumerate() {
            assert_eq!(a.intersect(&panel), a, "area {a:?} is outside the panel");
            if i < self.last_areas.len() {
                self.last_areas[i] = a;
            }
            if a.is_empty() {
                continue;
            }
            if !(self.walk == Walk::Indexed && self.send_indexed(src, a)) {
                self.send_area(src, a);
            } else {
                self.last_was_indexed = true;
            }
            pixels += a.area();
            sent += 1;
        }
        self.presents += 1;
        self.last_area_count = areas.len();
        // The board's measured numbers at 40 MHz: a full frame in 30.96 ms,
        // plus 0.1 ms per area.
        let send_us = (u64::from(pixels) * 30_960 / PIXELS as u64) as u32 + 100 * u32::from(sent);
        self.last_report = SendReport {
            areas: sent,
            pixels,
            send_us,
            wait_us: 0,
        };
        self.last_report
    }

    fn backlight(&mut self, level: u8) {
        if let Some(slot) = self.backlight_log.get_mut(self.backlight_calls) {
            *slot = (level, self.presents);
        }
        self.backlight_calls += 1;
        self.backlight = level;
    }
}

/// Touch: whatever the test puts in `at` (now) and `tap` (a touch that
/// started since the console last looked; taken by `take_tap`).
#[derive(Default)]
pub struct FakeTouch {
    pub at: Option<TouchPoint>,
    pub tap: Option<TouchPoint>,
}

impl Touch for FakeTouch {
    fn read(&mut self) -> Option<TouchPoint> {
        self.at
    }

    fn take_tap(&mut self) -> Option<TouchPoint> {
        self.tap.take()
    }
}

/// The board's own buttons: whatever the test puts in `held` (now) and
/// `pressed` (went down since the console last looked; taken by
/// `take_pressed`).
#[derive(Default)]
pub struct FakeKeys {
    pub held: u16,
    pub pressed: u16,
}

impl Keys for FakeKeys {
    fn held(&mut self) -> u16 {
        self.held
    }

    fn take_pressed(&mut self) -> u16 {
        core::mem::take(&mut self.pressed)
    }
}

/// Gamepads 0 to 3. Pad 0 is whatever the test puts in `state`, `motion`
/// and `pressed` (buttons that went down since the console last looked;
/// taken by `take_pressed`). Pads 1 to 3 are the same, in `others[0..3]`
/// (or [`FakePad::other`]); they start absent. Any higher index has no
/// pad.
#[derive(Default)]
pub struct FakePad {
    pub state: PadState,
    pub motion: Option<Motion>,
    pub pressed: u32,
    /// Pads 1, 2 and 3.
    pub others: [OnePad; 3],
}

/// One more fake gamepad: see [`FakePad`].
#[derive(Clone, Copy, Debug, Default)]
pub struct OnePad {
    pub state: PadState,
    pub motion: Option<Motion>,
    pub pressed: u32,
}

impl FakePad {
    /// Pad `index`, 1 to 3. Panics on any other index (pad 0 is the
    /// fields of `FakePad` itself).
    pub fn other(&mut self, index: u8) -> &mut OnePad {
        assert!((1..=3).contains(&index), "FakePad::other takes 1 to 3");
        &mut self.others[usize::from(index) - 1]
    }

    /// Pad `index`'s parts, or `None` past the last pad.
    fn parts(&mut self, index: u8) -> Option<(PadState, Option<Motion>, &mut u32)> {
        match index {
            0 => Some((self.state, self.motion, &mut self.pressed)),
            1..=3 => {
                let p = &mut self.others[usize::from(index) - 1];
                Some((p.state, p.motion, &mut p.pressed))
            }
            _ => None,
        }
    }
}

impl Pad for FakePad {
    fn take_pressed(&mut self, index: u8) -> u32 {
        match self.parts(index) {
            Some((state, _, pressed)) => {
                let p = core::mem::take(pressed);
                if state.connected { p } else { 0 }
            }
            None => 0,
        }
    }

    fn read(&mut self, index: u8) -> PadState {
        match self.parts(index) {
            Some((state, _, _)) => state,
            None => PadState::NONE,
        }
    }

    fn motion(&mut self, index: u8) -> Option<Motion> {
        match self.parts(index) {
            Some((state, motion, _)) if state.connected => motion,
            _ => None,
        }
    }
}

/// Games "linked into" the fake board, as the simulator does it. With an
/// empty list the board cannot start games at all (like a board that
/// never heard of them).
#[derive(Default)]
pub struct FakeGames {
    pub list: &'static [game_api::LinkedGame],
    /// Games started so far.
    pub started: u32,
    /// Calls of `end_game` so far.
    pub ended: u32,
    /// When set, every start is refused with this notice, as the chip's
    /// loader refuses a bad file ([`StartError::Refused`]).
    pub refuse: Option<&'static str>,
    /// The RAM lent to the console while no game is loaded (the chip's
    /// game data slot, see [`crate::lent`]).
    pub lent: FakeLent,
}

impl FakeGames {
    /// What [`Board::start_game`] does on the fake board. Every call
    /// ends the loan and fills the lent RAM with rubbish, as the chip's
    /// loader overwrites the slot (even when the start then fails).
    pub fn start(&mut self, folder: &str) -> Result<GameCallbacks, StartError> {
        self.lent.scramble();
        let got = self.open(folder);
        self.lent.loaded = got.is_ok();
        got
    }

    fn open(&mut self, folder: &str) -> Result<GameCallbacks, StartError> {
        if self.list.is_empty() {
            return Err(StartError::NotSupported);
        }
        if let Some(notice) = self.refuse {
            return Err(StartError::Refused(notice));
        }
        let game = self
            .list
            .iter()
            .find(|g| g.folder == folder)
            .ok_or(StartError::NotFound)?;
        let cb = game.open(&game_api::UNWIND_CONSOLE_API)?;
        self.started += 1;
        Ok(cb)
    }

    /// What [`Board::end_game`] does on the fake board: counts, and lends
    /// the RAM again (still holding the game's rubbish).
    pub fn end(&mut self) {
        self.ended += 1;
        self.lent.loaded = false;
        self.lent.scramble();
    }
}

/// Bytes the fake board lends: as many as the chip's game data slot.
pub const FAKE_LENT: usize = game_api::layout::DATA_SLOT_SIZE as usize;

/// The fake board's lent RAM (see [`crate::lent`]). It starts as rubbish
/// and becomes other rubbish at every game start and end, so a console
/// that reads what it did not write (or wrote before a game) shows
/// wrong text in a test instead of hiding the fault.
pub struct FakeLent {
    pub bytes: [u8; FAKE_LENT],
    /// A game is loaded: nothing is lent.
    pub loaded: bool,
    /// How many times the bytes were filled with rubbish.
    pub scrambles: u32,
}

impl FakeLent {
    pub fn new() -> Self {
        let mut me = FakeLent {
            bytes: [0; FAKE_LENT],
            loaded: false,
            scrambles: 0,
        };
        me.scramble();
        me
    }

    /// The RAM, while no game is loaded.
    pub fn lend(&mut self) -> Option<&mut [u8]> {
        if self.loaded {
            return None;
        }
        Some(&mut self.bytes)
    }

    /// Fills the bytes with rubbish: printable letters and stray bytes,
    /// different each time (a small xorshift, so every run is the same).
    pub fn scramble(&mut self) {
        self.scrambles += 1;
        let mut x = 0x9E37_79B9u32 ^ self.scrambles.wrapping_mul(0x85EB_CA6B);
        for b in self.bytes.iter_mut() {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *b = x as u8;
        }
    }
}

impl Default for FakeLent {
    fn default() -> Self {
        Self::new()
    }
}

/// A clock the test moves by hand.
#[derive(Default)]
pub struct FakeClock {
    pub now_us: u64,
}

impl FakeClock {
    pub fn advance(&mut self, us: u64) {
        self.now_us += us;
    }
}

impl Clock for FakeClock {
    fn now_us(&self) -> u64 {
        self.now_us
    }
}

/// A card made of a fixed table of files: `(path, bytes)`. Folders are
/// not listed as entries; a file `/A/B.BIN` is listed in `/A`.
#[derive(Default)]
pub struct FakeFiles {
    pub files: &'static [(&'static str, &'static [u8])],
}

impl FakeFiles {
    fn find(&self, path: &str) -> Result<&'static [u8], FsError> {
        self.files
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, b)| *b)
            .ok_or(FsError::NotFound)
    }
}

impl Files for FakeFiles {
    fn list<F: FnMut(&DirEntry<'_>) -> bool>(
        &mut self,
        dir: &str,
        mut each: F,
    ) -> Result<(), FsError> {
        let dir = dir.trim_end_matches('/');
        let mut found = false;
        for (path, bytes) in self.files {
            let Some(name) = path.strip_prefix(dir).and_then(|r| r.strip_prefix('/')) else {
                continue;
            };
            if name.contains('/') || name.len() > NAME_MAX {
                continue;
            }
            found = true;
            let entry = DirEntry {
                name,
                size: bytes.len() as u32,
                is_dir: false,
            };
            if !each(&entry) {
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
        Ok(self.find(path)?.len() as u32)
    }

    fn read_at(&mut self, path: &str, offset: u32, buf: &mut [u8]) -> Result<usize, FsError> {
        let bytes = self.find(path)?;
        let start = (offset as usize).min(bytes.len());
        let n = buf.len().min(bytes.len() - start);
        buf[..n].copy_from_slice(&bytes[start..start + n]);
        Ok(n)
    }
}

/// Link bytes: the test puts bytes in `inbox`, and reads what was sent
/// from `outbox`.
pub struct FakeLink {
    pub inbox: [u8; 256],
    pub in_len: usize,
    pub outbox: [u8; 1024],
    pub out_len: usize,
}

impl FakeLink {
    pub const fn new() -> Self {
        FakeLink {
            inbox: [0; 256],
            in_len: 0,
            outbox: [0; 1024],
            out_len: 0,
        }
    }

    /// Adds bytes as if they came from the PC. Returns how many fitted.
    pub fn feed(&mut self, data: &[u8]) -> usize {
        let n = data.len().min(self.inbox.len() - self.in_len);
        self.inbox[self.in_len..self.in_len + n].copy_from_slice(&data[..n]);
        self.in_len += n;
        n
    }

    pub fn sent(&self) -> &[u8] {
        &self.outbox[..self.out_len]
    }
}

impl Default for FakeLink {
    fn default() -> Self {
        Self::new()
    }
}

impl Link for FakeLink {
    fn read(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.in_len);
        buf[..n].copy_from_slice(&self.inbox[..n]);
        self.inbox.copy_within(n..self.in_len, 0);
        self.in_len -= n;
        n
    }

    fn write(&mut self, data: &[u8]) -> usize {
        let n = data.len().min(self.outbox.len() - self.out_len);
        self.outbox[self.out_len..self.out_len + n].copy_from_slice(&data[..n]);
        self.out_len += n;
        n
    }
}

/// The log: counts lines and keeps the last one.
pub struct FakeLog {
    pub lines: u32,
    last: [u8; LOG_LINE_MAX],
    last_len: usize,
}

impl FakeLog {
    pub const fn new() -> Self {
        FakeLog {
            lines: 0,
            last: [0; LOG_LINE_MAX],
            last_len: 0,
        }
    }

    /// The last line logged (cut to [`LOG_LINE_MAX`] bytes).
    pub fn last(&self) -> &str {
        core::str::from_utf8(&self.last[..self.last_len]).unwrap_or("")
    }
}

impl Default for FakeLog {
    fn default() -> Self {
        Self::new()
    }
}

impl LogOut for FakeLog {
    fn line(&mut self, text: &str) {
        // Cut on a character boundary so `last` stays valid text.
        let mut n = text.len().min(LOG_LINE_MAX);
        while !text.is_char_boundary(n) {
            n -= 1;
        }
        self.last[..n].copy_from_slice(&text.as_bytes()[..n]);
        self.last_len = n;
        self.lines += 1;
    }
}

/// Saved settings: 4 sectors (16 KB) of fake flash, starting erased.
pub struct FakeStore {
    pub bytes: [u8; 4 * SECTOR as usize],
}

impl FakeStore {
    pub const fn new() -> Self {
        FakeStore {
            bytes: [0xFF; 4 * SECTOR as usize],
        }
    }

    fn range(&self, offset: u32, len: usize) -> Result<core::ops::Range<usize>, StoreError> {
        let start = offset as usize;
        let end = start.checked_add(len).ok_or(StoreError::OutOfRange)?;
        if end > self.bytes.len() {
            return Err(StoreError::OutOfRange);
        }
        Ok(start..end)
    }
}

impl Default for FakeStore {
    fn default() -> Self {
        Self::new()
    }
}

impl Store for FakeStore {
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
        // Like flash: a write can only turn bits from 1 to 0.
        for (b, &d) in self.bytes[r].iter_mut().zip(data) {
            *b &= d;
        }
        Ok(())
    }

    fn erase_sector(&mut self, offset: u32) -> Result<(), StoreError> {
        if !offset.is_multiple_of(SECTOR) {
            return Err(StoreError::OutOfRange);
        }
        let r = self.range(offset, SECTOR as usize)?;
        self.bytes[r].fill(0xFF);
        Ok(())
    }
}

/// The whole fake board. Optional parts are [`crate::Absent`].
#[derive(Default)]
pub struct FakeBoard {
    pub screen: FakeScreen,
    pub touch: FakeTouch,
    pub keys: FakeKeys,
    pub pad: FakePad,
    pub clock: FakeClock,
    pub files: FakeFiles,
    pub link: FakeLink,
    pub log: FakeLog,
    pub store: FakeStore,
    pub games: FakeGames,
    // One stand-in per optional part, so `parts` can lend them separately.
    #[cfg(feature = "bt-classic")]
    pub hci: crate::Absent,
    #[cfg(feature = "passby")]
    pub beacon: crate::Absent,
    #[cfg(feature = "gps")]
    pub gps: crate::Absent,
    #[cfg(feature = "sound")]
    pub audio: crate::Absent,
}

impl FakeBoard {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Board for FakeBoard {
    type Screen = FakeScreen;
    type Touch = FakeTouch;
    type Keys = FakeKeys;
    type Pad = FakePad;
    type Clock = FakeClock;
    type Files = FakeFiles;
    type Link = FakeLink;
    type Log = FakeLog;
    type Store = FakeStore;
    #[cfg(feature = "bt-classic")]
    type Hci = crate::Absent;
    #[cfg(feature = "passby")]
    type Beacon = crate::Absent;
    #[cfg(feature = "gps")]
    type Gps = crate::Absent;
    #[cfg(feature = "sound")]
    type Audio = crate::Absent;

    fn parts(&mut self) -> Parts<'_, Self> {
        self.parts_and_lent().0
    }

    fn start_game(&mut self, folder: &str) -> Result<GameCallbacks, StartError> {
        self.games.start(folder)
    }

    fn end_game(&mut self) {
        self.games.end();
    }

    fn parts_and_lent(&mut self) -> (Parts<'_, Self>, Option<&mut [u8]>) {
        let lent = self.games.lent.lend();
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
        (parts, lent)
    }
}
