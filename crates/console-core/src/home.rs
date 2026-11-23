//! The home screen: a row of game tiles, like the Switch, one per game
//! found on the card. One tile is selected (a gold frame around it); its
//! name and tag show under the row. Left and right (pad, stick, or a tap
//! on a tile) move the selection. When the selected tile would leave the
//! screen, the row scrolls smoothly to keep it in view.
//!
//! Touch (see `input` for tap and drag):
//!
//! - A tap on a tile selects it; a tap on the selected tile starts it.
//!   The tap acts when the finger lifts, and only if the touch never
//!   became a drag. It acts on the tile that was under the finger when
//!   it went down (the row may have moved under a still finger since).
//!   It starts the game only if that tile was already selected when the
//!   finger went down.
//! - A drag that starts on the row (the tiles' band) moves the row with
//!   the finger, pixel for pixel, between its two ends
//!   ([`Home::max_scroll`]). A drag that starts elsewhere does nothing.
//! - Let go, the row settles on a resting place: a tile at the left
//!   margin, or the far end of the row. (The buttons can leave the
//!   row elsewhere: scrolling right puts the selected tile at the right
//!   margin.) A flick ([`FLICK_MIN`] or faster) goes on in its
//!   direction, further for a faster one ([`FLICK_US`], at most
//!   [`FLICK_MAX`]). The settle eases in as button scrolling does: half
//!   the way each frame.
//! - The selection rule: while the row moves under a finger or settles
//!   after one, the selected tile stays fully on screen (with its gold
//!   frame). When the row pushes it out, the nearest tile that is fully
//!   on screen becomes selected. At a resting place those are exactly
//!   the tiles inside the margins, so after a drag the selected tile is
//!   never off screen and the buttons find the row as they would have
//!   left it (no extra scroll).
//! - A finger that goes down on the row while it settles (a glide)
//!   stops it where it is. That touch is never a tap: lifted without
//!   dragging, it selects and starts nothing, and the row settles on the
//!   resting place nearest to where it stopped (with the selection rule
//!   below). If it drags, the drag moves the row from where it stopped.
//!   (A row scrolling for a button is not a glide: a tap then acts as
//!   above.)
//! - A press held longer than [`theme::TAP_MAX_US`] is not a tap (see
//!   `input`): lifted, it does nothing.
//! - Buttons keep working. A button press during a settle ends it: the
//!   row goes to the resting place nearest to where it is, moved as
//!   little as needed to show the newly selected tile. A button press
//!   during a drag, or while a finger keeps a glide stopped, does the
//!   same and takes the row from the finger until it lifts. A tap during
//!   a settle (one that went down before it began) does the same for
//!   the tapped tile.
//! - A finger taken from the row with no lift (the console menu opened
//!   over it) lets the row settle, as if it had lifted.
//!
//! The card layout (the same on the PC folder and the SD card):
//!
//! ```text
//! /CONSOLE/GAMES/<FOLDER>/GAME.BIN   the game (needed: no file, no tile)
//! /CONSOLE/GAMES/<FOLDER>/INFO.TXT   optional, lines of KEY=VALUE:
//!     NAME=LANDER        the name on screen (default: the folder's name)
//!     TAG=MOON LANDER    a short line under the name
//!     TILE=3C8F4A        the tile's colour as RRGGBB
//! /CONSOLE/GAMES/<FOLDER>/ART.BIN    optional: the tile's picture (`art`)
//! ```
//!
//! Tile pictures. A game with a good ART.BIN shows it on its tile; any
//! other game (no file, a bad one, or not read yet) shows the drawn
//! placeholder: the tile colour with the game's first letter. Home keeps
//! [`theme::ART_SLOTS`] pictures (see [`Home::art_wanted`] for which).
//! The card is read for pictures only when the row is still: no finger
//! on the row, not dragged, not gliding, not scrolling for a button
//! ([`Home::still`]). Then, in the same step, every wanted picture not
//! yet tried is read (at most one file per slot, 688 bytes each). A
//! picture that was tried and is missing or bad is not tried again until
//! the list is read again. The quick card check every 2 s also waits for
//! the row to be still.
//!
//! Memory. The game list and the tile pictures are NOT in the console's
//! own RAM: they are in the RAM the board lends while no game is loaded
//! (on the chip, the empty game data slot; layout in `lent`,
//! [`LENT_BYTES`] = 5,396 bytes). Starting a game ends the loan: the
//! list and pictures are gone ([`Home::lose_list`]), and home reads them
//! from the card again before it is shown again ([`Home::valid`]). The
//! console keeps only the count, the selection, the scroll, and which
//! picture is in which slot (about 60 bytes). Reading INFO.TXT uses a
//! 192-byte buffer on the stack, reading a picture 40 bytes.

use board_api::files::PATH_MAX;
use board_api::{Files, FsError, LogOut};
use console_video::{Frame, Palette, W};

use crate::art::{self, Art};
use crate::gfx::{self, CARD_ICON, CX, Text};
use crate::input::{Input, act};
use crate::lent;
pub use crate::lent::LENT_BYTES;
use crate::theme::{
    self, ART_SLOTS, DOTS_Y, FLICK_MAX, FLICK_MIN, FLICK_US, FRAME_GAP, FRAME_W, GOLD, INK, LINE,
    MUTE, NAME, NAME_Y, PAPER, TAG_Y, TILE, TILE_FIRST, TILE_GAP, TILE_LEFT, TILE_MARGIN, TILE_Y,
    TITLE_X, TOP_Y,
};

/// The folder the games are in, on the card.
pub const GAMES_DIR: &str = "/CONSOLE/GAMES";
/// Most games listed (fixed size: no heap). More are not shown.
pub const MAX_GAMES: usize = 32;
/// Longest name shown (12 letters fit twice the font size).
pub const NAME_LEN: usize = 12;
/// Longest tag shown.
pub const TAG_LEN: usize = 16;
/// Most bytes read from an INFO.TXT.
pub const INFO_MAX: usize = 192;
/// Distance from one tile to the next.
pub const STRIDE: i16 = (TILE + TILE_GAP) as i16;

/// One game on the card. 42 bytes as a value; 41 in the lent RAM
/// (`lent::GAME_BYTES`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Game {
    /// Its folder (an 8.3 name, without dot: at most 8 letters).
    pub folder: Text<8>,
    pub name: Text<NAME_LEN>,
    pub tag: Text<TAG_LEN>,
    /// The tile's colour (RGB565).
    pub tile: u16,
}

/// What the card holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Card {
    /// Not looked at yet.
    #[default]
    Unknown,
    /// No card, or it cannot be read.
    None,
    /// A card, but no games on it.
    Empty,
    /// A card with games.
    Games,
}

/// What the home screen wants after a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomeAction {
    None,
    /// Start game number `n` of the list.
    Launch(usize),
}

/// What changed on the card at a check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardChange {
    /// The card was taken out.
    Removed,
    /// A card with this many games was found.
    Ready(usize),
}

/// A copy of the game list (for tests and tools): see
/// [`Home::list`]. Reads as a slice of [`Game`].
pub struct GameList {
    games: [Game; MAX_GAMES],
    count: usize,
}

impl core::ops::Deref for GameList {
    type Target = [Game];

    fn deref(&self) -> &[Game] {
        &self.games[..self.count]
    }
}

pub struct Home {
    /// Games in the list (the list itself is in the lent RAM).
    count: usize,
    card: Card,
    selected: usize,
    /// How far the row is scrolled, in pixels (moves toward the target a
    /// little each frame).
    scroll: i16,
    /// While `settling`: where the row goes after a touch let it go (or a
    /// button cut that short). Otherwise not used.
    settle_to: i16,
    settling: bool,
    /// While settling after a drag: the selection follows the row (see
    /// the module notes). A button or a tap ends that.
    follow: bool,
    /// A button took the row from the finger: the rest of this touch
    /// does not move it.
    drag_off: bool,
    /// A finger held the row at the last frame (it may rest there now).
    held: bool,
    /// The touch going on went down on the row while it glided and
    /// stopped it: it is not a tap, and (unless `drag_off`) the row stays
    /// where it stopped until the finger drags or lifts.
    stopped: bool,
    /// The selection when the last touch went down: a tap starts a game
    /// only if its tile was selected then.
    down_sel: usize,
    /// The scroll when the last touch went down. A tap acts on the tile
    /// that was under the finger then, even if the row moved on since.
    down_scroll: i16,
    /// When to look at the card again.
    next_check_us: u64,
    /// The card check came due while the row was not still: it is done
    /// as soon as the row is still.
    check_late: bool,
    /// The touch going on went down on the row.
    down_on_row: bool,
    /// A finger that went down on the row is on the screen now.
    finger: bool,
    /// Which tile pictures are kept (in the lent RAM).
    art: Art,
    /// The list in the lent RAM is home's: read from the card since the
    /// last game start. False in a new console (nothing read yet).
    list_ok: bool,
    /// The stamp written with the list (see `lent`); 0: none yet.
    stamp: u32,
    /// The list was read again in this step after a game: the pictures
    /// are read in the next step, and home is drawn only then (see
    /// `Console::step`).
    fresh: bool,
}

impl Default for Home {
    fn default() -> Self {
        Self::new()
    }
}

impl Home {
    pub const fn new() -> Self {
        Home {
            count: 0,
            card: Card::Unknown,
            selected: 0,
            scroll: 0,
            settle_to: 0,
            settling: false,
            follow: false,
            drag_off: false,
            held: false,
            stopped: false,
            down_sel: 0,
            down_scroll: 0,
            next_check_us: 0,
            check_late: false,
            down_on_row: false,
            finger: false,
            art: Art::new(),
            list_ok: false,
            stamp: 0,
            fresh: false,
        }
    }

    /// Games found by the last [`Home::scan`].
    pub fn count(&self) -> usize {
        self.count
    }

    pub fn card(&self) -> Card {
        self.card
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Whether home's data in the lent RAM `ram` can be used: the list
    /// was read since the last game start, and the RAM carries home's
    /// stamp. With no games nothing is read from the RAM, so no RAM is
    /// needed then.
    pub fn valid(&self, ram: Option<&[u8]>) -> bool {
        self.list_ok
            && (self.count == 0
                || ram.is_some_and(|r| r.len() >= LENT_BYTES && lent::stamp(r) == self.stamp))
    }

    /// Whether home has a list (false from a game start until the list
    /// was read again).
    pub fn has_list(&self) -> bool {
        self.list_ok
    }

    /// A game is about to start: the lent RAM goes to it, and the list
    /// and pictures with it. Nothing is read from the RAM until
    /// [`Home::scan`] read the card again.
    pub fn lose_list(&mut self) {
        self.list_ok = false;
        self.fresh = false;
        self.art.forget();
    }

    /// The list was read again after a game, in this step: the pictures
    /// come next step, and home waits for them before it is drawn.
    pub fn fresh(&self) -> bool {
        self.fresh
    }

    /// Ends [`Home::fresh`].
    pub fn settle_fresh(&mut self) {
        self.fresh = false;
    }

    /// Game `i` of the list (sorted by name), read from the lent RAM;
    /// `None` past the list or while home has no valid list.
    pub fn game(&self, ram: Option<&[u8]>, i: usize) -> Option<Game> {
        let ram = ram.filter(|_| i < self.count && self.valid(ram))?;
        Some(lent::game(ram, i))
    }

    /// A copy of the whole list (empty while home has no valid list).
    pub fn list(&self, ram: Option<&[u8]>) -> GameList {
        let mut out = GameList {
            games: [Game::default(); MAX_GAMES],
            count: 0,
        };
        if let Some(ram) = ram.filter(|_| self.valid(ram)) {
            for (i, g) in out.games[..self.count].iter_mut().enumerate() {
                *g = lent::game(ram, i);
            }
            out.count = self.count;
        }
        out
    }

    /// How far the row is scrolled now, in pixels.
    pub fn scroll(&self) -> i16 {
        self.scroll
    }

    /// Reads the list of games from the card into the lent RAM `ram`.
    /// Keeps the selection when it can. The tile colours are then put in
    /// the palette with [`Home::set_colours`]. With no RAM (or too
    /// little) the card is not read and home shows no games.
    pub fn scan<F: Files>(&mut self, files: &mut F, ram: Option<&mut [u8]>, now_us: u64) {
        self.next_check_us = now_us + theme::CARD_CHECK_US;
        self.check_late = false;
        // The list's order may change: the pictures are read again.
        self.art.forget();
        self.list_ok = true;
        self.fresh = false;
        let kept = match ram.filter(|r| r.len() >= LENT_BYTES) {
            Some(ram) => self.read_list(files, ram),
            None => {
                self.card = Card::None;
                0
            }
        };
        self.count = kept;
        if kept > 0 {
            self.card = Card::Games;
        }
        self.selected = self.selected.min(kept.saturating_sub(1));
        self.settling = false;
        self.follow = false;
        self.stopped = false;
        self.scroll = self.target_scroll();
    }

    /// The card part of [`Home::scan`]: writes the list into `ram` and
    /// sets `card`. Returns how many games it kept.
    fn read_list<F: Files>(&mut self, files: &mut F, ram: &mut [u8]) -> usize {
        // A new stamp, never 0 (see `lent`): whatever the RAM held
        // before is not home's list any more.
        self.stamp = self.stamp.wrapping_add(1).max(1);
        lent::set_stamp(ram, self.stamp);
        let mut found = 0;
        let listed = files.list(GAMES_DIR, |e| {
            let ok_name = !e.name.starts_with('.') && e.name.len() <= 8;
            // `found < MAX_GAMES` again here: a board may keep listing
            // after we said stop (see the return value below).
            if e.is_dir && ok_name && found < MAX_GAMES {
                let g = Game {
                    folder: Text::of(e.name),
                    ..Game::default()
                };
                lent::put_game(ram, found, &g);
                found += 1;
            }
            found < MAX_GAMES
        });
        self.card = match listed {
            Ok(()) => Card::Empty,
            // No GAMES folder: a card with no games.
            Err(FsError::NotFound) => Card::Empty,
            Err(_) => Card::None,
        };
        // Keep only folders with a game in them, and read their INFO.TXT.
        // `own_tile`: bit i is set when game i gave its own TILE colour.
        let mut kept = 0;
        let mut own_tile: u32 = 0;
        for i in 0..found {
            let mut g = lent::game(ram, i);
            let mut path = Path::new();
            path.push(GAMES_DIR).push("/").push(g.folder.as_str());
            let mut bin = path;
            bin.push("/GAME.BIN");
            if !matches!(files.size(bin.as_str()), Ok(n) if n > 0) {
                continue;
            }
            path.push("/INFO.TXT");
            let mut buf = [0u8; INFO_MAX];
            let n = files.read_at(path.as_str(), 0, &mut buf).unwrap_or(0);
            if read_info(&buf[..n], &mut g) {
                own_tile |= 1 << kept;
            }
            lent::put_game(ram, kept, &g);
            kept += 1;
        }
        // Sorted by name, then by folder for equal names, so every board
        // shows the same order (a card lists files in the order they
        // were written).
        let before = |ram: &[u8], a: usize, b: usize| {
            let (a, b) = (lent::game(ram, a), lent::game(ram, b));
            (a.name.as_str(), a.folder.as_str()) > (b.name.as_str(), b.folder.as_str())
        };
        for i in 1..kept {
            let mut j = i;
            while j > 0 && before(ram, j - 1, j) {
                lent::swap_games(ram, j - 1, j);
                let (a, b) = ((own_tile >> (j - 1)) & 1, (own_tile >> j) & 1);
                if a != b {
                    own_tile ^= (1 << (j - 1)) | (1 << j);
                }
                j -= 1;
            }
        }
        // Games with no TILE get the default colours in list order, also
        // the same on every board.
        let defaults = &theme::TILE_DEFAULTS;
        for i in 0..kept {
            if own_tile & (1 << i) == 0 {
                let mut g = lent::game(ram, i);
                g.tile = theme::rgb(defaults[i % defaults.len()]);
                lent::put_game(ram, i, &g);
            }
        }
        kept
    }

    /// Reads the list again after a game (or after a game that could not
    /// start): [`Home::scan`], and the pictures wait for the next step
    /// ([`Home::fresh`]). Returns what changed on the card meanwhile.
    pub fn rescan<F: Files>(
        &mut self,
        files: &mut F,
        ram: Option<&mut [u8]>,
        now_us: u64,
    ) -> Option<CardChange> {
        let had = self.card;
        self.scan(files, ram, now_us);
        self.fresh = true;
        match (had == Card::Games, self.card == Card::Games) {
            (true, false) if self.card == Card::None => Some(CardChange::Removed),
            (false, true) => Some(CardChange::Ready(self.count)),
            _ => None,
        }
    }

    /// Looks at the card when it is time (every [`theme::CARD_CHECK_US`]).
    /// With no games: a full scan. With games: a quick look that the card
    /// is still there. Returns what changed.
    ///
    /// Never while the row is not [still](Home::still): a check that comes
    /// due then is done at the first still step.
    pub fn check<F: Files>(
        &mut self,
        files: &mut F,
        ram: Option<&mut [u8]>,
        now_us: u64,
    ) -> Option<CardChange> {
        if now_us < self.next_check_us && !self.check_late {
            return None;
        }
        self.next_check_us = now_us + theme::CARD_CHECK_US;
        if !self.still() {
            self.check_late = true;
            return None;
        }
        self.check_late = false;
        if self.card == Card::Games {
            if let Err(FsError::NoCard | FsError::Io) = files.list("/", |_| false) {
                self.scan(files, ram, now_us);
                return Some(CardChange::Removed);
            }
            return None;
        }
        self.scan(files, ram, now_us);
        (self.card == Card::Games).then_some(CardChange::Ready(self.count))
    }

    /// When the card should be looked at again.
    pub fn next_check_us(&self) -> u64 {
        self.next_check_us
    }

    /// A card check waited for the row, and the row is still now: it is
    /// due at the next step.
    pub fn check_waiting(&self) -> bool {
        self.check_late && self.still()
    }

    /// Puts the tile colours into the palette (numbers from
    /// [`TILE_FIRST`]). Setting a colour it already has changes nothing,
    /// so the screen is only sent again when a colour really changed.
    pub fn set_colours(&self, p: &mut Palette, ram: Option<&[u8]>) {
        let Some(ram) = ram.filter(|_| self.valid(ram)) else {
            return;
        };
        for i in 0..self.count {
            p.set(TILE_FIRST + i as u8, lent::tile(ram, i));
        }
        self.art.set_colours(p, ram);
    }

    /// Whether the row is still: no finger on it, not dragged, not
    /// gliding, not scrolling. Only then is the card read.
    pub fn still(&self) -> bool {
        !self.held
            && !self.finger
            && !self.frozen()
            && !self.settling
            && self.scroll == self.target_scroll()
    }

    /// The games whose pictures home wants kept, most wanted first: the
    /// tiles on screen (nearest the selected one first), then the next
    /// tile off each end. At most [`ART_SLOTS`]. Returns them and how
    /// many there are.
    pub fn art_wanted(&self) -> ([usize; ART_SLOTS], usize) {
        let mut out = [0; ART_SLOTS];
        let mut n = 0;
        let on_screen = |i: usize, margin: i16| {
            let x = Self::tile_x(i, self.scroll);
            x + TILE as i16 > -margin && x < W as i16 + margin
        };
        for pass in 0..2 {
            let margin = if pass == 0 { 0 } else { STRIDE };
            for d in 0..self.count {
                for i in [self.selected + d, self.selected.wrapping_sub(d)] {
                    let new = i < self.count
                        && on_screen(i, margin)
                        && (pass == 0 || !on_screen(i, 0))
                        && !out[..n].contains(&i);
                    if new && n < ART_SLOTS {
                        out[n] = i;
                        n += 1;
                    }
                }
            }
        }
        (out, n)
    }

    /// Whether a wanted picture is still to be read now (the row is still
    /// and some wanted game was not tried yet).
    pub fn art_pending(&self) -> bool {
        if !self.list_ok || self.card != Card::Games || !self.still() {
            return false;
        }
        let (want, n) = self.art_wanted();
        want[..n].iter().any(|&g| !self.art.known(g))
    }

    /// Reads the wanted pictures not tried yet, when the row is still
    /// (else nothing). Each kept picture's colours go in the palette.
    /// Returns how many files it tried.
    pub fn load_art<F: Files, L: LogOut>(
        &mut self,
        files: &mut F,
        p: &mut Palette,
        log: &mut L,
        ram: Option<&mut [u8]>,
    ) -> usize {
        if !self.art_pending() {
            return 0;
        }
        let Some(ram) = ram.filter(|r| self.valid(Some(r))) else {
            return 0;
        };
        let (want, n) = self.art_wanted();
        let mut tried = 0;
        for &g in &want[..n] {
            if self.art.known(g) {
                continue;
            }
            let folder = lent::game(ram, g).folder;
            let mut path = Path::new();
            path.push(GAMES_DIR)
                .push("/")
                .push(folder.as_str())
                .push("/")
                .push(art::FILE);
            match self.art.load(files, path.as_str(), g, &want[..n], p, ram) {
                None => {}
                // No ART.BIN is normal: the letter tile, no log line.
                Some(Ok(())) | Some(Err(art::ArtError::Missing)) => tried += 1,
                Some(Err(e)) => {
                    tried += 1;
                    // For game makers: why their picture is not shown.
                    let mut line = Path::new();
                    line.push("home: picture refused: ")
                        .push(folder.as_str())
                        .push(" (")
                        .push(e.word())
                        .push(")");
                    log.line(line.as_str());
                }
            }
        }
        tried
    }

    /// The tile pictures kept.
    pub fn art(&self) -> &Art {
        &self.art
    }

    /// The palette home had before any game: the game's part (0 to 239)
    /// black, as at start. Called when a game ends, so none of its
    /// colours stay behind. The tile colours come back with the list
    /// ([`Home::set_colours`] after the list was read again).
    pub fn reset_colours(&self, p: &mut Palette) {
        for i in 0..game_api::picture::GAME_COLOURS {
            p.set(i as u8, 0);
        }
    }

    /// Left edge of tile `i` on screen, with the row scrolled by `scroll`.
    fn tile_x(i: usize, scroll: i16) -> i16 {
        TILE_LEFT + i as i16 * STRIDE - scroll
    }

    /// The scroll nearest to `s` that keeps the selected tile at least
    /// [`TILE_MARGIN`] from both edges (never before the first tile).
    fn keep_in_view(&self, s: i16) -> i16 {
        let x = Self::tile_x(self.selected, 0);
        let min = x + TILE as i16 + TILE_MARGIN - W as i16; // right edge
        let max = x - TILE_MARGIN; // left edge
        s.clamp(min.max(0), max.max(0))
    }

    /// Where the row is going: the settle's end, or else the scroll that
    /// keeps the selected tile in view.
    fn target_scroll(&self) -> i16 {
        if self.settling {
            self.settle_to
        } else {
            self.keep_in_view(self.scroll)
        }
    }

    /// True while the row still scrolls by itself (not while a finger
    /// holds it, or keeps a glide stopped).
    pub fn moving(&self) -> bool {
        !self.held && !self.frozen() && self.scroll != self.target_scroll()
    }

    /// A finger stopped a glide and keeps the row still.
    fn frozen(&self) -> bool {
        self.stopped && !self.drag_off
    }

    /// Whether the row is gliding: settling, and not there yet.
    fn gliding(&self) -> bool {
        self.settling && self.scroll != self.settle_to
    }

    /// The far end of the row: the scroll that shows the last tile at the
    /// right margin (0 when every tile fits).
    pub fn max_scroll(&self) -> i16 {
        if self.count == 0 {
            return 0;
        }
        let x = Self::tile_x(self.count - 1, 0);
        (x + TILE as i16 + TILE_MARGIN - W as i16).max(0)
    }

    /// Whether a picture row `y` is on the tiles' band (with the frame).
    fn on_row(y: u16) -> bool {
        let (y, pad) = (y as i16, (FRAME_GAP + FRAME_W) as i16);
        y >= TILE_Y - pad && y < TILE_Y + TILE as i16 + pad
    }

    /// The tile under picture column `x` (with the frame round it), with
    /// the row at `scroll`.
    fn tile_at(&self, x: u16, scroll: i16) -> Option<usize> {
        let (x, pad) = (x as i16, (FRAME_GAP + FRAME_W) as i16);
        (0..self.count).find(|&i| {
            let left = Self::tile_x(i, scroll);
            x >= left - pad && x < left + TILE as i16 + pad
        })
    }

    /// Whether tile `i`, with its selection frame, is fully on screen
    /// with the row at `scroll`.
    fn shown(i: usize, scroll: i16) -> bool {
        let x = Self::tile_x(i, scroll);
        let edge = (FRAME_GAP + FRAME_W) as i16;
        x - edge >= 0 && x + TILE as i16 + edge <= W as i16
    }

    /// The selection rule: when the selected tile is not fully on screen,
    /// the nearest tile that is becomes selected. At a resting place the
    /// tiles fully on screen are exactly those inside the margins, so
    /// the buttons then find the row as they would have left it.
    fn pull_selection_into_view(&mut self) {
        if self.count == 0 || Self::shown(self.selected, self.scroll) {
            return;
        }
        let sel = self.selected;
        if let Some(i) = (0..self.count)
            .filter(|&i| Self::shown(i, self.scroll))
            .min_by_key(|&i| i.abs_diff(sel))
        {
            self.selected = i;
        }
    }

    /// The resting place for the row let go at scroll `s`, with the row's
    /// speed `speed` (picture pixels a second; positive: toward the end).
    /// Resting places: a tile at the left margin, or the far end.
    fn rest_near(&self, s: i16, speed: i32) -> i16 {
        let max = i32::from(self.max_scroll());
        let flick = speed.abs() >= FLICK_MIN;
        let carry = if flick {
            (speed * FLICK_US / 1_000_000).clamp(-FLICK_MAX, FLICK_MAX)
        } else {
            0
        };
        let aim = (i32::from(s) + carry).clamp(0, max);
        // Scroll of "tile k at the left margin" is off + k * STRIDE.
        let (off, step) = (i32::from(TILE_LEFT - TILE_MARGIN), i32::from(STRIDE));
        let lo = off + (aim - off).div_euclid(step) * step;
        let hi = (lo + step).min(max);
        let pick = if lo == aim {
            lo
        } else if flick {
            if speed > 0 { hi } else { lo }
        } else if aim - lo <= hi - aim {
            lo
        } else {
            hi
        };
        pick.clamp(0, max) as i16
    }

    /// Ends a settle early (a button or a tap chose a tile): the row goes
    /// to the resting place nearest to where it is, moved as little as
    /// needed to show the selected tile.
    fn cut_settle(&mut self) {
        let base = if self.follow {
            self.rest_near(self.scroll, 0)
        } else {
            self.settle_to
        };
        self.settle_to = self.keep_in_view(base);
        self.settling = true;
        self.follow = false;
    }

    /// One frame of the home screen.
    pub fn update(&mut self, input: &Input) -> HomeAction {
        let mut action = HomeAction::None;
        if let Some((_, y)) = input.down {
            self.down_on_row = self.count > 0 && Self::on_row(y);
            self.down_sel = self.selected;
            self.down_scroll = self.scroll;
            self.drag_off = false;
            // A finger on the row stops a glide where it is.
            self.stopped = self.count > 0 && Self::on_row(y) && self.gliding();
            if self.stopped {
                self.settling = false;
                self.follow = false;
            }
        }
        // This touch stopped a glide: it is no tap.
        let stopped = self.stopped;
        // A drag moves the row only if it started on the row.
        let mut drag = input
            .drag
            .filter(|d| Self::on_row(d.from.1) && !self.drag_off);
        if self.count > 0 {
            let last = self.count - 1;
            // The finger that held the row was taken away with no lift (the
            // console menu opened over it): settle from here, as after a
            // drag let go at rest. (In a grace frame `drag` is still there.)
            if self.held && input.drag.is_none() && input.release.is_none() {
                self.settle_to = self.rest_near(self.scroll, 0);
                self.settling = true;
                self.follow = true;
            }
            // The tile chosen when the frame began: a tap starts only that
            // one, even if a button moved the choice in this same frame.
            let was = self.selected;
            if input.acts & act::LEFT != 0 {
                self.selected = self.selected.saturating_sub(1);
            }
            if input.acts & act::RIGHT != 0 {
                self.selected = (self.selected + 1).min(last);
            }
            // (Home opens the console menu; the console takes it before
            // this, see `Console::step`.)
            if input.acts & act::OK != 0 {
                action = HomeAction::Launch(self.selected);
            }
            // A button ends a settle, and takes the row from a finger.
            if input.acts != 0 && (self.settling || drag.is_some() || self.frozen()) {
                if drag.is_some() || self.frozen() {
                    self.drag_off = true;
                    self.follow = true; // settle from where the finger left it
                    drag = None;
                }
                self.cut_settle();
            }
            // The finger that stopped a glide lifted (or was taken away)
            // without dragging: the row settles from where it stopped.
            if stopped && !input.touching && drag.is_none() {
                if !self.drag_off {
                    self.settle_to = self.rest_near(self.scroll, 0);
                    self.settling = true;
                    self.follow = true;
                }
                self.stopped = false;
            }
            // The tile under the finger when it went down (the row may have
            // moved under a still finger since: a button scroll).
            if let Some((tx, ty)) = input.tap
                && !stopped
                && Self::on_row(ty)
                && let Some(i) = self.tile_at(tx, self.down_scroll)
            {
                if i == was && i == self.down_sel {
                    action = HomeAction::Launch(i);
                }
                self.selected = i;
                if self.settling {
                    self.cut_settle();
                }
            }
            if let Some(d) = drag {
                // The row follows the finger, between its two ends (from
                // where a stopped glide left it).
                self.stopped = false;
                self.settling = false;
                self.follow = false;
                self.scroll = (self.scroll - d.dx).clamp(0, self.max_scroll());
                self.pull_selection_into_view();
            }
            if let Some(r) = input.release
                && Self::on_row(r.from.1)
                && !self.drag_off
            {
                // The finger moving right pulls the row back (scroll
                // goes down).
                self.settle_to = self.rest_near(self.scroll, -i32::from(r.vx));
                self.settling = true;
                self.follow = true;
            }
        }
        // Scroll: half the way each frame, at least one pixel. Not while a
        // finger holds the row, or keeps a glide stopped.
        if drag.is_none() && !self.frozen() {
            let d = self.target_scroll() - self.scroll;
            if d != 0 {
                self.scroll += (d + d.signum()) / 2;
            }
        }
        self.held = drag.is_some();
        self.finger = input.touching && self.down_on_row;
        if self.settling {
            if self.follow {
                self.pull_selection_into_view();
            }
            if self.scroll == self.settle_to {
                self.settling = false;
                self.follow = false;
            }
        }
        action
    }

    /// Whether the row is settling after a touch let it go (or a button
    /// cut that short).
    pub fn settling(&self) -> bool {
        self.settling
    }

    /// Draws the home screen, with the list from the lent RAM `ram`. The
    /// console draws only when [`Home::valid`]; if it is not, the row is
    /// left out (never rubbish from the RAM).
    pub fn draw(&self, f: &mut Frame, ram: Option<&[u8]>) {
        f.clear(INK);
        gfx::stars(f);
        f.text(TITLE_X, TOP_Y, NAME, MUTE);
        match self.card {
            Card::Games => {
                if let Some(ram) = ram.filter(|_| self.valid(ram)) {
                    self.draw_games(f, ram);
                }
            }
            Card::None | Card::Unknown => {
                CARD_ICON.draw(f, CX - 10, 24, 3, CARD_ICON.h, MUTE);
                f.rect(CX - 7, 36, 15, 1, INK); // the card's label line
                gfx::text_centred(f, CX, NAME_Y, "NO CARD", PAPER, 2);
                gfx::text_centred(f, CX, TAG_Y, "INSERT A CARD TO SEE GAMES", MUTE, 1);
            }
            Card::Empty => {
                let x = CX - TILE as i16 / 2;
                f.outline(x, TILE_Y, TILE, TILE, LINE);
                gfx::text_centred(f, CX, TILE_Y + 11, "?", LINE, 3);
                gfx::text_centred(f, CX, NAME_Y, "NO GAMES", PAPER, 2);
                gfx::text_centred(f, CX, TAG_Y, "PUT GAMES IN /CONSOLE/GAMES", MUTE, 1);
            }
        }
    }

    fn draw_games(&self, f: &mut Frame, ram: &[u8]) {
        // How many games, top right.
        let mut buf = [0u8; 12];
        let label = count_text(self.count, &mut buf);
        gfx::text_right(f, 154, TOP_Y, label, MUTE);

        for i in 0..self.count {
            let x = Self::tile_x(i, self.scroll);
            if x + (TILE as i16) + 4 <= 0 || x - 4 >= W as i16 {
                continue;
            }
            let kept = (self.art.slot(i)).and_then(|k| Some((k, lent::picture(ram, k)?)));
            match kept {
                Some((k, stored)) => art::draw(f, k, stored, x, TILE_Y),
                None => {
                    let g = lent::game(ram, i);
                    draw_tile(f, x, TILE_Y, &g, TILE_FIRST + i as u8);
                }
            }
            if i == self.selected {
                let gap = FRAME_GAP as i16;
                gfx::ring(
                    f,
                    x - gap,
                    TILE_Y - gap,
                    TILE + 2 * FRAME_GAP,
                    TILE + 2 * FRAME_GAP,
                    FRAME_W,
                    GOLD,
                );
            }
        }

        let g = lent::game(ram, self.selected);
        let name = g.name.as_str();
        if gfx::width(name, 2) <= W as i16 - 8 {
            gfx::text_centred(f, CX, NAME_Y, name, PAPER, 2);
        } else {
            gfx::text_centred(f, CX, NAME_Y + 3, name, PAPER, 1);
        }
        gfx::text_centred(f, CX, TAG_Y, g.tag.as_str(), MUTE, 1);

        // One small dot per game, the selected one gold (when they fit).
        let n = self.count as i16;
        if n > 1 && n * 6 <= W as i16 - 16 {
            let x0 = CX - n * 3;
            for i in 0..self.count {
                let c = if i == self.selected { GOLD } else { LINE };
                f.rect(x0 + i as i16 * 6 + 1, DOTS_Y, 3, 3, c);
            }
        }
    }
}

/// A placeholder tile: the tile colour, a thin dark edge, and the game's
/// first letter, big, with a shadow.
fn draw_tile(f: &mut Frame, x: i16, y: i16, g: &Game, colour: u8) {
    f.rect(x, y, TILE, TILE, colour);
    f.outline(x, y, TILE, TILE, LINE);
    let mut buf = [0u8; 4];
    let letter = g.name.as_str().chars().next().unwrap_or('?');
    let s: &str = letter.encode_utf8(&mut buf);
    let (cx, ty) = (x + TILE as i16 / 2, y + (TILE as i16 - 15) / 2);
    gfx::text_centred(f, cx + 1, ty + 1, s, INK, 3);
    gfx::text_centred(f, cx, ty, s, PAPER, 3);
}

/// "1 GAME", "12 GAMES".
fn count_text(n: usize, buf: &mut [u8; 12]) -> &str {
    let mut k = 0;
    if n >= 10 {
        buf[k] = b'0' + (n / 10 % 10) as u8;
        k += 1;
    }
    buf[k] = b'0' + (n % 10) as u8;
    k += 1;
    let word: &[u8] = if n == 1 { b" GAME" } else { b" GAMES" };
    buf[k..k + word.len()].copy_from_slice(word);
    k += word.len();
    core::str::from_utf8(&buf[..k]).unwrap_or("")
}

/// Reads the KEY=VALUE lines of an INFO.TXT into `g`. Unknown keys and
/// broken lines are skipped. Returns true when it gave its own TILE
/// colour (the others get a default colour after sorting).
fn read_info(bytes: &[u8], g: &mut Game) -> bool {
    g.name = Text::of(g.folder.as_str());
    g.tag = Text::new();
    let mut own_tile = false;
    // Only whole lines of valid text; a cut last line is still used.
    let text = match core::str::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) => core::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap_or(""),
    };
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "NAME" if !value.is_empty() => g.name = Text::of(value),
            "TAG" => g.tag = Text::of(value),
            "TILE" => {
                if let Ok(hex) = u32::from_str_radix(value, 16)
                    && value.len() == 6
                {
                    g.tile = theme::rgb(hex);
                    own_tile = true;
                }
            }
            _ => {}
        }
    }
    own_tile
}

/// A path built in a fixed buffer ([`PATH_MAX`] bytes, no heap). Parts
/// that would not fit are left out (the card then says "not found").
#[derive(Clone, Copy)]
struct Path {
    bytes: [u8; PATH_MAX],
    len: usize,
}

impl Path {
    const fn new() -> Self {
        Path {
            bytes: [0; PATH_MAX],
            len: 0,
        }
    }

    fn push(&mut self, s: &str) -> &mut Self {
        if self.len + s.len() <= PATH_MAX {
            self.bytes[self.len..self.len + s.len()].copy_from_slice(s.as_bytes());
            self.len += s.len();
        }
        self
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_lines_are_read() {
        let mut g = Game {
            folder: Text::of("SPROUT"),
            ..Game::default()
        };
        let own = read_info(
            b"NAME=Sprout\r\nTAG=IDLE GAME\nTILE=3C8F4A\nNEEDS=GPS\nbroken\n",
            &mut g,
        );
        assert!(own);
        assert_eq!(g.name.as_str(), "Sprout");
        assert_eq!(g.tag.as_str(), "IDLE GAME");
        assert_eq!(g.tile, theme::rgb(0x3C8F4A));
    }

    #[test]
    fn missing_info_uses_the_folder() {
        let mut g = Game {
            folder: Text::of("COMET"),
            ..Game::default()
        };
        // A broken TILE is not the game's own colour: `scan` then gives
        // it a default one.
        assert!(!read_info(b"TILE=nothex", &mut g));
        assert_eq!(g.name.as_str(), "COMET");
    }

    #[test]
    fn count_text_reads_well() {
        let mut b = [0; 12];
        assert_eq!(count_text(1, &mut b), "1 GAME");
        assert_eq!(count_text(4, &mut b), "4 GAMES");
        assert_eq!(count_text(32, &mut b), "32 GAMES");
    }

    #[test]
    fn game_is_small() {
        assert_eq!(size_of::<Game>(), 42);
    }
}
