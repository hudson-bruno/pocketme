//! The settings app (feature `settings`): brightness, the boot animation
//! (with `boot-gather`), the Player 1 gamepad (pair and forget), and a
//! page about the console.
//!
//! Entry ([`Entry::Tiles`], placement E): two system tiles in the home
//! row — GAMEPADS first (before the games; opens directly on GAMEPADS)
//! and SETTINGS last (after the games; opens on SETTINGS). The home
//! screen's quick menu (`HOME` or small gear tap) also gives shortcuts
//! to GAMEPADS and SETTINGS beside BRIGHTNESS and CLOSE.
//!
//! Pairing goes through the board's [`Pad`] pairing methods (see
//! [`board_api::pair`]): the app never waits and never touches the radio
//! or the flash. While a GAMEPADS page is open it reads [`Pad::pair_state`]
//! once per step and leaves at most one request of its own with
//! [`Pad::pair_request`], plus Cancel or Ack when leaving a page:
//!
//! - PAIR PLAYER 1: starts searching at once ([`PairReq::Begin`], 60 s
//!   countdown, no physical button hold). SEARCHING lists the pads found
//!   with name and tail; choosing one asks CALL IT AND PAIR? with NO
//!   first, and YES sends [`PairReq::Choose`] with the tail the player
//!   saw. Then CALLING, then an ending.
//! - Opening GAMEPADS never disconnects a linked pad. For the MVP only
//!   one Player 1 pad is saved at a time; if one is already saved, it
//!   must be forgotten before pairing another.
//! - FORGET: KEEP first; FORGET sends [`PairReq::Forget`] with the tail
//!   the player saw (no physical button hold).
//! - Leaving an open pairing window sends [`PairReq::Cancel`]; leaving an
//!   ending sends [`PairReq::Ack`] (except TRY AGAIN, which sends
//!   [`PairReq::Begin`] straight from the ending).
//!
//! Memory: [`App`] is at most 32 bytes inside the `Console`, all zero
//! when new.

use board_api::Pad;
use board_api::pair::{PAD_NAME_MAX, PairFail, PairReq, PairState, SavedPad};
use console_video::Frame;

use crate::gfx::{self, CX, PAD_ICON, SUN_ICON, Text};
use crate::input::{Input, act};
use crate::theme::{
    BAR_H, BAR_Y, BRIGHT_STEPS, GOLD, INK, LINE, MUTE, NAME_Y, OK, PANEL, PAPER, TAG_Y, TILE, WARN,
};

/// How many pad slots the underlying store can hold (pad-store `MAX_PADS`).
pub const PLACES: u8 = 4;
/// The most found pads the SEARCHING page lists (pad-host keeps eight).
pub const FOUND_MAX: u8 = 8;
/// The search the SEARCHING page counts down: the board's window closes
/// by itself after about this long (its own deadline decides).
pub const SEARCH_US: u64 = 60_000_000;
/// The SEARCHING page's selection is on STOP (not on a found pad). A pad
/// found later does not move it.
pub const STOP_SEL: u8 = u8::MAX;

/// Where the settings app is opened from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Entry {
    /// None: the console as today (the app cannot be opened). So the
    /// feature alone changes nothing until the board picks a way in.
    #[default]
    Off = 0,
    /// E: two system tiles in the home row, GAMEPADS first (before the
    /// games) and SETTINGS last (after them), plus quick-menu shortcuts.
    Tiles,
}

impl Entry {
    /// `tiles`, `on` or `off`.
    pub fn parse(word: &str) -> Option<Entry> {
        match word {
            "tiles" | "on" => Some(Entry::Tiles),
            "off" => Some(Entry::Off),
            _ => None,
        }
    }
}

/// Placement E's system tiles, in their order in the row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SystemTile {
    /// The first tile of the row: opens the app on its GAMEPADS page.
    Gamepads = 0,
    /// The last tile, after the games: opens the app's SETTINGS page.
    Settings = 1,
}

impl SystemTile {
    /// The system tile at place `i` (`None` past them).
    pub const fn at(i: usize) -> Option<SystemTile> {
        match i {
            0 => Some(SystemTile::Gamepads),
            1 => Some(SystemTile::Settings),
            _ => None,
        }
    }

    /// Its name under the row (where a game's name is).
    pub const fn name(self) -> &'static str {
        match self {
            SystemTile::Gamepads => "GAMEPADS",
            SystemTile::Settings => "SETTINGS",
        }
    }

    /// The line under its name (where a game's tag is).
    pub const fn tag(self) -> &'static str {
        match self {
            SystemTile::Gamepads => "PAIR / FORGET",
            SystemTile::Settings => "BRIGHTNESS / BOOT ANIMATION",
        }
    }
}

/// The lines of the SETTINGS page (GAMEPADS has its own tile and quick
/// menu shortcut).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MainLine {
    Bright,
    /// Only with `boot-gather` (without it there is no animation to
    /// choose).
    #[cfg(feature = "boot-gather")]
    Anim,
    About,
}

/// The SETTINGS page's lines, from the top.
#[cfg(feature = "boot-gather")]
pub const MAIN_LINES: &[MainLine] = &[MainLine::Bright, MainLine::Anim, MainLine::About];
#[cfg(not(feature = "boot-gather"))]
pub const MAIN_LINES: &[MainLine] = &[MainLine::Bright, MainLine::About];
/// How many lines the SETTINGS page has.
pub const MAIN_ROWS: u8 = MAIN_LINES.len() as u8;
/// The ABOUT line's place on the SETTINGS page.
pub const ABOUT_LINE: u8 = main_index(MainLine::About);

const fn main_index(line: MainLine) -> u8 {
    let mut i = 0;
    while i < MAIN_LINES.len() {
        if MAIN_LINES[i] as u8 == line as u8 {
            return i as u8;
        }
        i += 1;
    }
    0
}

/// The app's pages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum AppPage {
    /// SETTINGS: brightness, boot animation, about.
    #[default]
    Main = 0,
    /// GAMEPADS: Player 1 status, pair or forget.
    Pads,
    /// ABOUT THIS CONSOLE.
    About,
    /// Pairing: searching, with the 60 s count-down.
    Searching,
    /// A found pad was chosen: CALL IT AND PAIR? NO / YES.
    Found,
    /// The console calls the chosen pad.
    Calling,
    /// Forgetting the saved pad on the board.
    Forgetting,
    /// An ending: saved, forgotten, or why not (the board's
    /// [`PairState`], kept while the page shows).
    End,
    /// FORGET THIS PAD? KEEP / FORGET.
    Forget,
}

impl AppPage {
    /// A page of an open radio pairing window: leaving it for a page
    /// outside the window sends [`PairReq::Cancel`].
    pub const fn window(self) -> bool {
        matches!(self, AppPage::Searching | AppPage::Found | AppPage::Calling)
    }
}

/// What the console must do after a step of the app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Out {
    /// The player set this brightness step.
    pub bright: Option<u8>,
    /// The player set this boot animation (0 FULL, 1 SHORT, 2 OFF; only
    /// with `boot-gather`).
    pub anim: Option<u8>,
    /// The page changed (or the app closed): the finger down now is not
    /// the next page's.
    pub turned: bool,
}

/// What the app shows of the console besides its own state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ctx {
    pub now_us: u64,
    /// Brightness step, 0 (dimmest) to `BRIGHT_STEPS - 1`.
    pub step: u8,
    /// Boot animation: 0 FULL, 1 SHORT, 2 OFF (0 without `boot-gather`).
    pub anim: u8,
}

/// What the board's pad says about saved and found pads, read once per
/// step (and once per drawing).
#[derive(Clone, Copy, Debug, Default)]
struct Seen {
    pairing: bool,
    slots: Option<u8>,
    saved: [Option<SavedPad>; PLACES as usize],
    /// How many pads the search found so far (at most [`FOUND_MAX`]).
    found: u8,
}

impl Seen {
    fn of<P: Pad + ?Sized>(pad: &P) -> Seen {
        let mut saved = [None; PLACES as usize];
        for (place, s) in saved.iter_mut().enumerate() {
            *s = pad.saved_pad(place as u8);
        }
        let found = (0..FOUND_MAX)
            .find(|&i| pad.pair_found(i).is_none())
            .unwrap_or(FOUND_MAX);
        Seen {
            pairing: pad.pairing(),
            slots: pad.pair_slots(),
            saved,
            found,
        }
    }

    fn count(&self) -> u8 {
        self.saved.iter().flatten().count() as u8
    }

    /// When exactly one pad is saved, its place and state.
    fn single(&self) -> Option<(u8, SavedPad)> {
        let mut found = None;
        for (i, s) in self.saved.iter().enumerate() {
            if let Some(pad) = *s {
                if found.is_some() {
                    return None;
                }
                found = Some((i as u8, pad));
            }
        }
        found
    }

    fn connected(&self) -> bool {
        self.saved.iter().flatten().any(|s| s.connected)
    }

    /// The first line the pad's selection lands on in GAMEPADS: the first
    /// saved pad, else PAIR PLAYER 1.
    fn first_line(&self) -> u8 {
        (0..PLACES)
            .find(|&p| self.saved[usize::from(p)].is_some())
            .unwrap_or(PLACES)
    }

    /// Lines of GAMEPADS the pad can choose: PAIR when none is saved, else
    /// only the saved pad(s) so a second Player 1 pad cannot be paired
    /// until the saved one is forgotten.
    fn line_ok(&self, l: u8) -> bool {
        if self.count() == 0 {
            l == PLACES
        } else {
            self.saved.get(usize::from(l)).is_some_and(Option::is_some)
        }
    }
}

/// The settings app's whole state. All zero when new.
#[derive(Clone, Copy, Debug, Default)]
pub struct App {
    open: bool,
    entry: Entry,
    page: AppPage,
    /// The chosen line or button on the page.
    sel: u8,
    /// The request this page sent (Begin, Choose or Forget) waits for
    /// the board's answer.
    sent: bool,
    /// This pairing got as far as YES (a time-out then means the pad did
    /// not answer, not that none was found).
    called: bool,
    /// The pages are about forgetting a pad, not pairing one.
    forgetting: bool,
    /// The app was opened straight on GAMEPADS: BACK there leaves it.
    direct: bool,
    /// The board's pairing state as last read; on the ending page, the
    /// ending shown.
    state: PairState,
    /// The saved place being forgotten.
    place: u8,
    /// The tail the player saw: of the pad being forgotten, or of the
    /// found pad chosen.
    tail: [u8; 2],
    /// The found pad chosen (as [`Pad::pair_found`] numbers them).
    pick: u8,
    /// SEARCHING and the pages after it: when Begin was sent (the
    /// count-down starts there).
    since_us: u64,
}

impl App {
    pub const fn new() -> Self {
        App {
            open: false,
            entry: Entry::Off,
            page: AppPage::Main,
            sel: 0,
            sent: false,
            called: false,
            forgetting: false,
            direct: false,
            state: PairState::Off,
            place: 0,
            tail: [0; 2],
            pick: 0,
            since_us: 0,
        }
    }

    pub fn entry(&self) -> Entry {
        self.entry
    }

    pub fn set_entry(&mut self, entry: Entry) {
        self.entry = entry;
    }

    /// Opens the app directly on its GAMEPADS page (BACK there leaves
    /// the app).
    pub fn open_gamepads<P: Pad + ?Sized>(&mut self, pad: &P) {
        let first = Seen::of(pad).first_line();
        self.open = true;
        self.direct = true;
        self.go(AppPage::Pads, first);
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn page(&self) -> AppPage {
        self.page
    }

    pub fn sel(&self) -> u8 {
        self.sel
    }

    /// The board's pairing state as the app last read it (on the ending
    /// page: the ending it shows).
    pub fn state(&self) -> PairState {
        self.state
    }

    /// The app is open on a page of a pairing or forgetting session: an
    /// open window, a forget in progress, or an ending. A board ends its
    /// session when this turns false.
    pub fn pairing_page(&self) -> bool {
        self.open && (self.page.window() || matches!(self.page, AppPage::Forgetting | AppPage::End))
    }

    /// Opens the app on its SETTINGS page.
    pub fn open(&mut self) {
        self.open = true;
        self.direct = false;
        self.go(AppPage::Main, 0);
    }

    fn close(&mut self) {
        self.open = false;
        self.direct = false;
        self.page = AppPage::Main;
        self.sel = 0;
        self.sent = false;
    }

    fn go(&mut self, page: AppPage, sel: u8) {
        self.page = page;
        self.sel = sel;
        self.sent = false;
    }

    /// The pages that read the board's pairing state every step: every
    /// GAMEPADS page.
    fn polls(&self) -> bool {
        self.open && !matches!(self.page, AppPage::Main | AppPage::About)
    }

    /// Whether the app needs a frame every frame (an open window or
    /// forget request waiting for its answer).
    pub fn moving(&self) -> bool {
        self.open
            && matches!(
                self.page,
                AppPage::Searching | AppPage::Found | AppPage::Calling | AppPage::Forgetting
            )
    }

    /// Seconds left of the search as the page counts it.
    fn left_s(&self, now_us: u64) -> u64 {
        SEARCH_US
            .saturating_sub(now_us.saturating_sub(self.since_us))
            .div_ceil(1_000_000)
    }

    /// One step of the open app. Reads the board's pairing state (on the
    /// GAMEPADS pages) and leaves the requests the page asks for.
    pub fn update<P: Pad + ?Sized>(&mut self, input: &Input, c: &Ctx, pad: &mut P) -> Out {
        let mut out = Out::default();
        let before = (self.open, self.page);
        if self.polls() {
            let st = pad.pair_state();
            // The ending page keeps the ending it shows.
            if self.page != AppPage::End {
                self.state = st;
            }
        }
        let seen = Seen::of(&*pad);
        let mut req = None;
        // When the board's state moved the page, the input of this step
        // is not the new page's.
        if !self.follow(&seen) {
            if input.acts & act::HOME != 0 {
                self.close();
            } else if input.acts & act::BACK != 0 || input.down.is_some_and(|p| in_box(p, BACK_HIT))
            {
                self.back(&seen);
            } else {
                match self.page {
                    AppPage::Main => self.main(input, c, &mut out),
                    AppPage::Pads => self.pads_page(input, c, &seen, &mut req),
                    AppPage::About => {
                        if input.acts & act::OK != 0 {
                            self.go(AppPage::Main, ABOUT_LINE);
                        }
                    }
                    AppPage::Searching => self.searching(input, &seen, &*pad),
                    AppPage::Forgetting => {}
                    _ => self.dialog(input, c, &seen, &mut req),
                }
            }
        }
        let after = (self.open, self.page);
        if let Some(r) = req {
            pad.pair_request(r);
        }
        if let Some(r) = leave_request(before, after) {
            pad.pair_request(r);
        }
        out.turned = after != before;
        out
    }

    /// Follows the board's state on the pages that wait for it. Returns
    /// true when that moved the page.
    fn follow(&mut self, seen: &Seen) -> bool {
        let st = self.state;
        match self.page {
            AppPage::Searching | AppPage::Found | AppPage::Calling => {}
            AppPage::Forgetting => {
                if st.is_end() {
                    self.go(AppPage::End, 0);
                    return true;
                }
                if st != PairState::Off {
                    self.sent = false;
                    return false;
                }
                if self.sent {
                    return false;
                }
                self.go(AppPage::Pads, seen.first_line());
                return true;
            }
            _ => return false,
        }
        if st.is_end() {
            self.go(AppPage::End, 0);
            return true;
        }
        if st.is_open() {
            // The request was taken (Choose: once the call started).
            if self.page != AppPage::Calling || st == PairState::Calling {
                self.sent = false;
            }
            return false;
        }
        // Off: the request is not served yet, or the window closed with
        // no ending (the board stopped it): back to the list.
        if self.sent {
            return false;
        }
        self.go(AppPage::Pads, seen.first_line());
        true
    }

    /// BACK (the button, or B): one page up; from the first page, out.
    fn back(&mut self, seen: &Seen) {
        match self.page {
            AppPage::Main => self.close(),
            // Opened directly on GAMEPADS: back to home.
            AppPage::Pads if self.direct => self.close(),
            AppPage::Pads => self.go(AppPage::Main, 0),
            AppPage::About => self.go(AppPage::Main, ABOUT_LINE),
            // The window stays open: back to the list.
            AppPage::Found => {
                let pick = self.pick;
                self.go(AppPage::Searching, pick);
            }
            AppPage::Forget | AppPage::Forgetting => {
                let p = self.place;
                self.go(AppPage::Pads, p);
            }
            AppPage::End => self.end_done(seen),
            // Leaving any other pairing page stops pairing.
            _ => self.go(AppPage::Pads, seen.first_line()),
        }
    }

    /// The SETTINGS page.
    fn main(&mut self, input: &Input, c: &Ctx, out: &mut Out) {
        let last = MAIN_ROWS - 1;
        if input.acts & act::UP != 0 {
            self.sel = self.sel.saturating_sub(1);
        }
        if input.acts & act::DOWN != 0 {
            self.sel = (self.sel + 1).min(last);
        }
        let mut step = c.step;
        #[allow(unused_mut)]
        let mut anim = c.anim;
        let top = BRIGHT_STEPS - 1;
        let ok = input.acts & act::OK != 0;
        match MAIN_LINES[usize::from(self.sel.min(last))] {
            MainLine::Bright => {
                if input.acts & act::LEFT != 0 {
                    step = step.saturating_sub(1);
                }
                if input.acts & act::RIGHT != 0 {
                    step = (step + 1).min(top);
                }
                if ok {
                    step = if step >= top { 0 } else { step + 1 };
                }
            }
            #[cfg(feature = "boot-gather")]
            MainLine::Anim => {
                if input.acts & act::LEFT != 0 {
                    anim = anim.saturating_sub(1);
                }
                if input.acts & act::RIGHT != 0 {
                    anim = (anim + 1).min(2);
                }
                if ok {
                    anim = (anim + 1) % 3;
                }
            }
            MainLine::About if ok => self.go(AppPage::About, 0),
            _ => {}
        }
        if let Some((x, y)) = input.down
            && let Some(row) = main_row_at(y)
        {
            self.sel = row;
            match MAIN_LINES[usize::from(row)] {
                MainLine::Bright => {
                    if let Some(s) = bright_step_at(x) {
                        step = s;
                    }
                }
                #[cfg(feature = "boot-gather")]
                MainLine::Anim => anim = chip_at(x).unwrap_or((anim + 1) % 3),
                MainLine::About => self.go(AppPage::About, 0),
            }
        }
        // A finger that went down on the bars and slides.
        if let (Some(d), Some((x, _))) = (input.drag, input.touch)
            && main_row_at(d.from.1).map(|r| MAIN_LINES[usize::from(r)]) == Some(MainLine::Bright)
            && let Some(s) = bright_step_at(x)
        {
            step = s;
        }
        if step != c.step {
            out.bright = Some(step);
        }
        if anim != c.anim {
            out.anim = Some(anim);
        }
    }

    /// The GAMEPADS page.
    fn pads_page(&mut self, input: &Input, c: &Ctx, seen: &Seen, req: &mut Option<PairReq>) {
        if !seen.line_ok(self.sel) {
            self.sel = seen.first_line();
        }
        if input.acts & act::UP != 0
            && let Some(l) = (0..self.sel).rev().find(|&l| seen.line_ok(l))
        {
            self.sel = l;
        }
        if input.acts & act::DOWN != 0
            && let Some(l) = (self.sel + 1..=PLACES).find(|&l| seen.line_ok(l))
        {
            self.sel = l;
        }
        let mut take = input.acts & act::OK != 0;
        if let Some((_, y)) = input.down
            && let Some(l) = pads_hit(y, seen)
        {
            self.sel = l;
            take = true;
        }
        // A board that cannot pair: the lines say so and do nothing.
        if !take || !seen.pairing {
            return;
        }
        if self.sel == PLACES {
            if seen.count() > 0 {
                return;
            }
            self.forgetting = false;
            self.called = false;
            if seen.slots == Some(0) {
                // No record left: refused before anything, as the board
                // would refuse Begin.
                self.state = PairState::Failed(PairFail::NoRoom);
                self.go(AppPage::End, 0);
            } else {
                self.since_us = c.now_us;
                self.go(AppPage::Searching, 0);
                self.sent = true;
                *req = Some(PairReq::Begin);
            }
        } else if let Some(s) = seen.saved[usize::from(self.sel)] {
            self.forgetting = true;
            self.place = self.sel;
            self.tail = s.tail;
            // KEEP is chosen first.
            self.go(AppPage::Forget, 0);
        }
    }

    /// The SEARCHING page: the found pads, then STOP.
    fn searching<P: Pad + ?Sized>(&mut self, input: &Input, seen: &Seen, pad: &P) {
        let n = seen.found;
        if input.acts & act::UP != 0 {
            if self.sel >= n {
                if n > 0 {
                    self.sel = n - 1;
                }
            } else {
                self.sel = self.sel.saturating_sub(1);
            }
        } else if input.acts & act::DOWN != 0 && self.sel < n {
            self.sel = if self.sel + 1 < n {
                self.sel + 1
            } else {
                STOP_SEL
            };
        }
        let mut take = input.acts & act::OK != 0;
        if let Some((x, y)) = input.down {
            if in_box((x, y), button_box(1, 0)) {
                self.sel = STOP_SEL;
                take = true;
            } else if let Some(i) = found_row_at(y, first_found(self.sel, n))
                && i < n
            {
                self.sel = i;
                take = true;
            }
        }
        if !take {
            return;
        }
        if self.sel < n {
            if let Some(f) = pad.pair_found(self.sel) {
                self.pick = self.sel;
                self.tail = f.tail;
                // NO is chosen first.
                self.go(AppPage::Found, 0);
            }
        } else {
            self.go(AppPage::Pads, PLACES);
        }
    }

    /// A page with buttons at the bottom.
    fn dialog(&mut self, input: &Input, c: &Ctx, seen: &Seen, req: &mut Option<PairReq>) {
        let n = self.buttons().len() as u8;
        if n == 0 {
            return;
        }
        if input.acts & act::LEFT != 0 {
            self.sel = self.sel.saturating_sub(1);
        }
        if input.acts & act::RIGHT != 0 {
            self.sel = (self.sel + 1).min(n - 1);
        }
        let mut take = input.acts & act::OK != 0;
        if let Some(p) = input.down
            && let Some(b) = (0..n).find(|&b| in_box(p, button_box(n, b)))
        {
            self.sel = b;
            take = true;
        }
        if !take {
            return;
        }
        let b = self.sel.min(n - 1);
        match (self.page, b) {
            (AppPage::Found, 0) => {
                let pick = self.pick;
                self.go(AppPage::Searching, pick);
            }
            (AppPage::Found, _) => {
                *req = Some(PairReq::Choose {
                    i: self.pick,
                    tail: self.tail,
                });
                self.go(AppPage::Calling, 0);
                self.sent = true;
                self.called = true;
            }
            (AppPage::End, 0) if n == 2 => {
                self.called = false;
                self.since_us = c.now_us;
                self.go(AppPage::Searching, 0);
                self.sent = true;
                *req = Some(PairReq::Begin);
            }
            (AppPage::End, _) => self.end_done(seen),
            (AppPage::Forget, 1) => {
                *req = Some(PairReq::Forget {
                    place: self.place,
                    tail: self.tail,
                });
                self.go(AppPage::Forgetting, 0);
                self.sent = true;
            }
            (AppPage::Forget, _) => {
                let p = self.place;
                self.go(AppPage::Pads, p);
            }
            // STOP while searching or calling: pairing ends.
            _ => self.go(AppPage::Pads, seen.first_line()),
        }
    }

    /// OK on an ending: back to the list, on the line it was about.
    fn end_done(&mut self, seen: &Seen) {
        let sel = match self.state {
            PairState::Saved(p) => p,
            PairState::Forgotten(_) => seen.first_line(),
            _ if self.forgetting => {
                if seen.line_ok(self.place) {
                    self.place
                } else {
                    seen.first_line()
                }
            }
            _ => seen.first_line(),
        };
        self.go(AppPage::Pads, sel);
    }

    /// The ending offers TRY AGAIN (a pairing that may work the next
    /// time).
    fn try_again(&self) -> bool {
        !self.forgetting
            && matches!(
                self.state,
                PairState::Failed(PairFail::Timeout | PairFail::NotHeld | PairFail::Lost)
            )
    }

    /// The words of the buttons at the bottom of the page.
    pub fn buttons(&self) -> &'static [&'static str] {
        match self.page {
            AppPage::Searching | AppPage::Calling => &["STOP"],
            AppPage::Found => &["NO", "YES, CALL"],
            AppPage::Forget => &["KEEP", "FORGET"],
            AppPage::End if self.try_again() => &["TRY AGAIN", "BACK"],
            AppPage::End => &["OK"],
            _ => &[],
        }
    }

    /// The chosen found pad's name, or its tail when it gave none (or is
    /// gone from the list).
    fn who<P: Pad + ?Sized>(&self, pad: &P) -> Text<PAD_NAME_MAX> {
        match pad.pair_found(self.pick) {
            Some(f) if f.tail == self.tail && f.name_len > 0 => Text::of_ascii(f.name()),
            _ => Text::of(tail_text(self.tail).as_str()),
        }
    }

    /// Draws the open app over the whole picture.
    pub fn draw<P: Pad + ?Sized>(&self, f: &mut Frame, c: &Ctx, pad: &P) {
        let seen = Seen::of(pad);
        f.clear(INK);
        let title = match self.page {
            AppPage::Main => "SETTINGS",
            AppPage::Pads => "GAMEPADS",
            AppPage::About => "ABOUT",
            AppPage::Forget | AppPage::Forgetting => "FORGET A PAD",
            AppPage::End if self.forgetting => "FORGET A PAD",
            _ => "PAIR A NEW PAD",
        };
        draw_title(f, title);
        match self.page {
            AppPage::Main => self.draw_main(f, c),
            AppPage::Pads => self.draw_pads(f, &seen),
            AppPage::About => draw_about(f, &seen),
            AppPage::Searching => self.draw_searching(f, c, &seen, pad),
            _ => self.draw_dialog(f, c, &seen, pad),
        }
        let n = self.buttons().len();
        let hints = match self.page {
            AppPage::Main => match MAIN_LINES[usize::from(self.sel.min(MAIN_ROWS - 1))] {
                MainLine::About => "A CHOOSE  B BACK",
                _ => "<> CHANGE  B BACK",
            },
            AppPage::Pads => "A CHOOSE  B BACK",
            AppPage::About | AppPage::Forgetting => "B BACK",
            AppPage::Searching if self.sel < seen.found => "A CHOOSE  B BACK",
            AppPage::Searching | AppPage::Calling => "A STOP  B BACK",
            _ if n == 1 => "A OK  B BACK",
            _ => "<> CHOOSE  A OK  B BACK",
        };
        draw_bar(f, hints);
    }

    fn draw_main(&self, f: &mut Frame, c: &Ctx) {
        for (row, line) in MAIN_LINES.iter().enumerate() {
            let row = row as u8;
            let y = main_row_y(row);
            let on = self.sel == row;
            if on {
                f.rect(ROW_X, y, ROW_W, MAIN_ROW_H, GOLD);
            } else {
                f.hline(ROW_X, y + MAIN_ROW_H as i16 - 1, ROW_W, LINE);
            }
            let ink = if on { INK } else { PAPER };
            let quiet = if on { INK } else { MUTE };
            let ty = y + 5;
            match line {
                MainLine::Bright => {
                    f.text(TEXT_X, ty, "BRIGHTNESS", ink);
                    SUN_ICON.draw(f, SUN_X, y + 4, 1, SUN_ICON.h, quiet);
                    for b in 0..BRIGHT_STEPS {
                        let (x, bh) = (bright_bar_x(b), 3 + u16::from(b));
                        let top = y + 11 - bh as i16;
                        if b <= c.step {
                            f.rect(x, top, BRIGHT_BAR_W, bh, ink);
                        } else {
                            f.outline(x, top, BRIGHT_BAR_W, bh, if on { INK } else { LINE });
                        }
                    }
                }
                #[cfg(feature = "boot-gather")]
                MainLine::Anim => {
                    f.text(TEXT_X, ty, "BOOT ANIMATION", ink);
                    for (i, word) in ANIM_WORDS.iter().enumerate() {
                        let (x, w) = chip(i as u8);
                        if i as u8 == c.anim {
                            let (bg, fg) = if on { (INK, GOLD) } else { (GOLD, INK) };
                            f.rect(x, y + 3, w, 9, bg);
                            f.text(x + 3, ty, word, fg);
                        } else {
                            f.text(x + 3, ty, word, quiet);
                        }
                    }
                }
                MainLine::About => {
                    f.text(TEXT_X, ty, "ABOUT", ink);
                    gfx::text_right(f, ROW_X + ROW_W as i16 - 4, ty, ">", ink);
                }
            }
        }
        let help = match MAIN_LINES[usize::from(self.sel.min(MAIN_ROWS - 1))] {
            MainLine::Bright => "CHANGES AT ONCE",
            #[cfg(feature = "boot-gather")]
            MainLine::Anim => "PLAYS AT THE NEXT START",
            MainLine::About => "NAME AND VERSION",
        };
        gfx::text_centred(f, CX, main_row_y(MAIN_ROWS) + 6, help, MUTE, 1);
    }

    fn draw_pads(&self, f: &mut Frame, seen: &Seen) {
        let right = ROW_X + ROW_W as i16 - 4;
        let count = seen.count();
        if count <= 1 {
            // Player 1 MVP: row 0 is Player 1, row 1 is PAIR PLAYER 1.
            let y0 = pads_row_y(0);
            if let Some((place, s)) = seen.single() {
                let on = self.sel == place;
                if on {
                    f.rect(ROW_X, y0, ROW_W, PADS_ROW_H, GOLD);
                } else {
                    f.hline(ROW_X, y0 + PADS_ROW_H as i16 - 1, ROW_W, LINE);
                }
                let ink = if on { INK } else { PAPER };
                let quiet = if on { INK } else { MUTE };
                let ty = y0 + 5;
                f.text(TEXT_X, ty, "PLAYER 1", quiet);
                let end = f.text(P1_NAME_X, ty, tail_text(s.tail).as_str(), ink);
                if s.connected {
                    if on {
                        f.rect(end + 3, y0 + 3, PAD_ICON.w + 4, PAD_ICON.h + 2, INK);
                    }
                    PAD_ICON.draw(f, end + 5, y0 + 4, 1, PAD_ICON.h, OK);
                }
                if seen.pairing {
                    gfx::text_right(f, right, ty, "FORGET >", quiet);
                }
            } else {
                f.hline(ROW_X, y0 + PADS_ROW_H as i16 - 1, ROW_W, LINE);
                let ty = y0 + 5;
                f.text(TEXT_X, ty, "PLAYER 1", MUTE);
                f.text(P1_NAME_X, ty, "- NONE -", MUTE);
            }
            let y1 = pads_row_y(1);
            let on = count == 0 && self.sel == PLACES;
            if on {
                f.rect(ROW_X, y1, ROW_W, PADS_ROW_H, GOLD);
            } else {
                f.hline(ROW_X, y1 + PADS_ROW_H as i16 - 1, ROW_W, LINE);
            }
            let ink = if on { INK } else { PAPER };
            let quiet = if on { INK } else { MUTE };
            let warn = if on { INK } else { WARN };
            let ty = y1 + 5;
            if !seen.pairing {
                f.text(TEXT_X, ty, "NO PAIRING IN THIS BUILD", quiet);
            } else if count > 0 {
                f.text(TEXT_X, ty, "+ PAIR PLAYER 1", quiet);
                if seen.slots == Some(0) {
                    gfx::text_right(f, right, ty, "NO ROOM", warn);
                } else {
                    gfx::text_right(f, right, ty, "1 SAVED", quiet);
                }
            } else if seen.slots == Some(0) {
                f.text(TEXT_X, ty, "+ PAIR PLAYER 1", quiet);
                gfx::text_right(f, right, ty, "NO ROOM", warn);
            } else {
                f.text(TEXT_X, ty, "+ PAIR PLAYER 1", ink);
                gfx::text_right(f, right, ty, ">", ink);
                if let Some(n) = seen.slots {
                    let left = writes_text(n);
                    gfx::text_right(f, right - 8, ty, left.as_str(), quiet);
                }
            }
        } else {
            // Multiple saved entries (e.g. from older test firmware): show
            // each place so the player can forget them individually.
            for l in 0..=PLACES {
                let y = pads_row_y(l);
                let on = self.sel == l;
                if on {
                    f.rect(ROW_X, y, ROW_W, PADS_ROW_H, GOLD);
                } else {
                    f.hline(ROW_X, y + PADS_ROW_H as i16 - 1, ROW_W, LINE);
                }
                let ink = if on { INK } else { PAPER };
                let quiet = if on { INK } else { MUTE };
                let warn = if on { INK } else { WARN };
                let ty = y + 5;
                if l == PLACES {
                    if !seen.pairing {
                        f.text(TEXT_X, ty, "NO PAIRING IN THIS BUILD", quiet);
                    } else {
                        f.text(TEXT_X, ty, "+ PAIR PLAYER 1", quiet);
                        gfx::text_right(f, right, ty, "FORGET ALL", warn);
                    }
                    continue;
                }
                f.text(TEXT_X, ty, PAD_WORDS[usize::from(l)], quiet);
                let Some(s) = seen.saved[usize::from(l)] else {
                    f.text(NAME_X, ty, "- EMPTY -", quiet);
                    continue;
                };
                let end = f.text(NAME_X, ty, tail_text(s.tail).as_str(), ink);
                if s.connected {
                    if on {
                        f.rect(end + 3, y + 3, PAD_ICON.w + 4, PAD_ICON.h + 2, INK);
                    }
                    PAD_ICON.draw(f, end + 5, y + 4, 1, PAD_ICON.h, OK);
                }
                if seen.pairing {
                    gfx::text_right(f, right, ty, "FORGET >", quiet);
                }
            }
        }
        let help = if !seen.pairing {
            "THIS BUILD CANNOT PAIR OR FORGET"
        } else if count == 0 && seen.slots.is_some() {
            "LEFT: WRITES THE PAD MEMORY HOLDS"
        } else if count == 0 {
            "NO PAD SAVED YET"
        } else if seen.connected() {
            "GREEN PAD: CONNECTED NOW"
        } else if count == 1 {
            "FORGET PLAYER 1 TO PAIR ANOTHER"
        } else {
            "FORGET SAVED PADS TO PAIR ONE"
        };
        gfx::text_centred(f, CX, pads_row_y(PLACES + 1) + 5, help, MUTE, 1);
    }

    fn draw_searching<P: Pad + ?Sized>(&self, f: &mut Frame, c: &Ctx, seen: &Seen, pad: &P) {
        let n = seen.found;
        let clock = clock_text(self.left_s(c.now_us));
        gfx::text_centred(f, CX, LINES_Y, "SEARCHING FOR GAMEPADS", PAPER, 1);
        if n == 0 {
            // Nothing found yet: the count-down, big.
            gfx::text_centred(f, CX, 32, clock.as_str(), GOLD, 2);
            let left_us = SEARCH_US.saturating_sub(c.now_us.saturating_sub(self.since_us));
            let w = (left_us * u64::from(METER_W) / SEARCH_US) as u16;
            draw_meter(f, METER_Y - 2, w, MUTE);
            gfx::text_centred(f, CX, line_y(5), "ONLY GAMEPADS ARE SHOWN.", MUTE, 1);
            gfx::text_centred(f, CX, line_y(6), "THE CONSOLE STAYS HIDDEN.", MUTE, 1);
        } else {
            // The pads found, to choose from; the time left, small.
            gfx::text_right(f, ROW_X + ROW_W as i16 - 2, LINES_Y, clock.as_str(), MUTE);
            let first = first_found(self.sel, n);
            for r in 0..FOUND_ROWS {
                let i = first + r;
                let Some(found) = (i < n).then(|| pad.pair_found(i)).flatten() else {
                    break;
                };
                let y = found_row_y(r);
                let on = self.sel == i;
                if on {
                    f.rect(ROW_X, y, ROW_W, FOUND_ROW_H, GOLD);
                } else {
                    f.hline(ROW_X, y + FOUND_ROW_H as i16 - 1, ROW_W, LINE);
                }
                let (ink, quiet) = if on { (INK, INK) } else { (PAPER, MUTE) };
                PAD_ICON.draw(f, ROW_X + 4, y + 2, 1, PAD_ICON.h, ink);
                let tail = tail_text(found.tail);
                let right = ROW_X + ROW_W as i16 - 4;
                let room = right - gfx::width(tail.as_str(), 1) - 4 - FOUND_NAME_X;
                let name = cut(Text::<PAD_NAME_MAX>::of_ascii(found.name()).as_str(), room);
                f.text(FOUND_NAME_X, y + 3, name.as_str(), ink);
                gfx::text_right(f, right, y + 3, tail.as_str(), quiet);
            }
            // More pads above or below the four shown.
            if first > 0 {
                gfx::text_right(f, ROW_X + ROW_W as i16 - 2, found_row_y(0) - 7, "+", MUTE);
            }
            if first + FOUND_ROWS < n {
                let y = found_row_y(FOUND_ROWS) + 3;
                gfx::text_right(f, ROW_X + ROW_W as i16 - 2, y, "+", MUTE);
            }
            let y = found_row_y(FOUND_ROWS) + 3;
            gfx::text_centred(f, CX, y, "ONLY GAMEPADS ARE SHOWN.", MUTE, 1);
        }
        let on = if self.sel < n { STOP_SEL } else { 0 };
        draw_buttons(f, self.buttons(), on);
    }

    fn draw_dialog<P: Pad + ?Sized>(&self, f: &mut Frame, c: &Ctx, seen: &Seen, pad: &P) {
        const E: (&str, u8) = ("", MUTE);
        let now = c.now_us;
        let who = self.who(pad);
        let name = who.as_str();
        let tail = tail_text(self.tail);
        let single_or_none = seen.count() <= 1;
        let pad_word = if single_or_none {
            "PLAYER 1"
        } else {
            PAD_WORDS[usize::from(self.place.min(PLACES - 1))]
        };
        // Joined lines; each page sets the ones it shows.
        let mut t1: Text<40>;
        let t2: Text<40>;
        // (text, colour) lines, from the top, every LINE_STEP pixels.
        let mut lines: [(&str, u8); LINES] = [E; LINES];
        match self.page {
            AppPage::Found => {
                lines = [
                    ("FOUND A GAMEPAD", OK),
                    E,
                    E,
                    E,
                    ("CALL IT AND PAIR?", PAPER),
                    ("NOTHING IS CALLED BEFORE YES.", MUTE),
                    E,
                ];
            }
            AppPage::Calling => {
                let room = beside("CALLING ");
                t1 = Text::joined("CALLING ", cut(name, room).as_str());
                lines = [
                    (t1.as_str(), PAPER),
                    E,
                    ("PAIRING...", MUTE),
                    ("KEEP THE PAD NEAR", MUTE),
                    E,
                    E,
                    E,
                ];
            }
            AppPage::Forget => {
                t1 = Text::joined("FORGET ", pad_word);
                t2 = Text::joined(
                    Text::<40>::joined(t1.as_str(), " (").as_str(),
                    tail.as_str(),
                );
                t1 = Text::joined(t2.as_str(), ")?");
                lines = [
                    (t1.as_str(), GOLD),
                    E,
                    ("IT WILL NOT CONNECT AGAIN", PAPER),
                    ("UNTIL IT IS PAIRED ONCE MORE.", PAPER),
                    E,
                    E,
                    E,
                ];
            }
            AppPage::Forgetting => {
                lines = [E, E, ("FORGETTING...", GOLD), E, E, E, E];
            }
            AppPage::End => {
                lines = match self.state {
                    PairState::Saved(p) => {
                        if p == 0 {
                            let room = beside(" IS NOW PLAYER 1");
                            t2 = Text::joined(cut(name, room).as_str(), " IS NOW PLAYER 1");
                        } else {
                            let n = PLACE_DIGITS[usize::from(p.min(PLACES - 1))];
                            let room = beside(" IS NOW PAD 4");
                            t1 = Text::joined(cut(name, room).as_str(), " IS NOW PAD ");
                            t2 = Text::joined(t1.as_str(), n);
                        }
                        [
                            ("PAIRED AND SAVED", OK),
                            E,
                            (t2.as_str(), PAPER),
                            E,
                            ("FROM NOW ON IT CONNECTS", MUTE),
                            ("BY ITSELF. PAIRING IS OFF.", MUTE),
                            E,
                        ]
                    }
                    PairState::Forgotten(p) => {
                        if seen.count() == 0 {
                            t1 = Text::of("PLAYER 1 IS FORGOTTEN");
                        } else {
                            let n = PLACE_DIGITS[usize::from(p.min(PLACES - 1))];
                            t1 = Text::joined(
                                Text::<40>::joined("PAD ", n).as_str(),
                                " IS FORGOTTEN",
                            );
                        }
                        [
                            (t1.as_str(), OK),
                            E,
                            ("IT WILL NOT CONNECT AGAIN", PAPER),
                            ("UNTIL IT IS PAIRED ONCE MORE.", PAPER),
                            E,
                            ("ITS PLACE IS FREE NOW.", MUTE),
                            E,
                        ]
                    }
                    PairState::Failed(why) => {
                        let (lines, a, b) = fail_lines(why, self.forgetting, self.called, name);
                        t1 = a;
                        t2 = b;
                        let mut out = lines;
                        // Lines 2 and 3 may carry the pad's name.
                        if !t1.is_empty() {
                            out[2].0 = t1.as_str();
                        }
                        if !t2.is_empty() {
                            out[3].0 = t2.as_str();
                        }
                        out
                    }
                    _ => [E; LINES],
                };
            }
            _ => {}
        }
        for (i, (t, colour)) in lines.iter().enumerate() {
            if !t.is_empty() {
                gfx::text_centred(f, CX, line_y(i), t, *colour, 1);
            }
        }
        if self.page == AppPage::Found {
            // The chosen pad, big, its tail under it, and the time
            // left, small.
            let text = clock_text(self.left_s(now));
            gfx::text_right(f, ROW_X + ROW_W as i16 - 2, LINES_Y, text.as_str(), MUTE);
            let name = cut(name, LINE_MAX - BIG_MARK_W);
            let w = BIG_MARK_W + gfx::width(name.as_str(), 1);
            let x = CX - w / 2;
            PAD_ICON.draw(f, x, 32, 2, PAD_ICON.h, PAPER);
            f.text(x + BIG_MARK_W, 37, name.as_str(), PAPER);
            gfx::text_centred(f, CX, 49, tail.as_str(), MUTE, 1);
        }
        draw_buttons(f, self.buttons(), self.sel);
    }
}

/// The request a page change sends: Cancel when a pairing window or a
/// forget-in-progress page is left for a page outside the window (or
/// the app closes), beating any waiting Forget in the 1-slot request
/// box; Ack when an ending is left (unless TRY AGAIN jumped straight into
/// Searching with Begin).
fn leave_request(before: (bool, AppPage), after: (bool, AppPage)) -> Option<PairReq> {
    let (was_open, was) = before;
    let (open, now) = after;
    if !was_open || (open && now == was) {
        return None;
    }
    if (was.window() || was == AppPage::Forgetting)
        && !(open && (now.window() || now == AppPage::End))
    {
        return Some(PairReq::Cancel);
    }
    if was == AppPage::End && !(open && now == AppPage::Searching) {
        return Some(PairReq::Ack);
    }
    None
}

/// The lines of a failed ending, and the joined texts for lines 2 and 3
/// when they name the pad (empty when they do not).
fn fail_lines(
    why: PairFail,
    forgetting: bool,
    called: bool,
    name: &str,
) -> ([(&'static str, u8); LINES], Text<40>, Text<40>) {
    const E: (&str, u8) = ("", MUTE);
    let none = Text::<40>::new();
    let changed = ("NOTHING WAS CHANGED.", MUTE);
    let unsaved = ("NOTHING WAS SAVED.", MUTE);
    let lines = match why {
        PairFail::NotHeld => [
            ("BOOT WAS NOT HELD", WARN),
            E,
            ("HOLD THE CONSOLE'S BOOT", PAPER),
            ("BUTTON FOR THE WHOLE 2 SECONDS,", PAPER),
            ("UNTIL THE BAR IS FULL.", PAPER),
            E,
            changed,
        ],
        PairFail::Linked => [
            ("A PAD IS CONNECTED", WARN),
            E,
            ("TURN THE CONNECTED PAD OFF,", PAPER),
            ("THEN TRY AGAIN.", PAPER),
            E,
            E,
            changed,
        ],
        PairFail::Full => [
            ("PLAYER 1 IS ALREADY SAVED", WARN),
            E,
            ("ONLY ONE PLAYER 1 PAD CAN", PAPER),
            ("BE SAVED RIGHT NOW.", PAPER),
            ("FORGET PLAYER 1 FIRST.", PAPER),
            E,
            changed,
        ],
        PairFail::NoRoom => [
            ("NO ROOM TO SAVE PADS", WARN),
            E,
            ("THE PAD MEMORY IS USED UP.", PAPER),
            ("THE CONSOLE DOES NOT TIDY IT", PAPER),
            ("BY ITSELF, SO PAIRING STAYS OFF.", PAPER),
            E,
            changed,
        ],
        PairFail::Damaged => [
            ("THE PAD MEMORY IS DAMAGED", WARN),
            E,
            ("ITS BYTES CANNOT BE READ AS", PAPER),
            ("A LIST OF PADS, SO THE CONSOLE", PAPER),
            ("NEITHER PAIRS NOR FORGETS.", PAPER),
            E,
            changed,
        ],
        PairFail::Timeout if called => [
            ("THE PAD DID NOT ANSWER", WARN),
            E,
            ("IT DID NOT FINISH PAIRING", PAPER),
            ("IN TIME. KEEP IT NEAR AND", PAPER),
            ("IN PAIRING MODE, THEN TRY AGAIN.", PAPER),
            E,
            unsaved,
        ],
        PairFail::Timeout => [
            ("NO GAMEPAD FOUND", WARN),
            E,
            ("SEARCHED FOR 60 SECONDS.", PAPER),
            ("IS THE PAD IN PAIRING MODE?", PAPER),
            E,
            ("PAIRING IS OFF AGAIN.", MUTE),
            E,
        ],
        PairFail::OldPin => {
            let room = beside(" WANTS A PIN CODE");
            let a = Text::<40>::joined(cut(name, room).as_str(), " WANTS A PIN CODE");
            let lines = [
                ("THIS PAD CANNOT BE PAIRED", WARN),
                E,
                ("", PAPER),
                ("(THE OLD WAY OF PAIRING).", PAPER),
                ("THE CONSOLE PAIRS NO PIN.", PAPER),
                E,
                unsaved,
            ];
            return (lines, a, none);
        }
        PairFail::Refused => [
            ("THE PAD SAID NO", WARN),
            E,
            ("IT REFUSED, OR WANTS A KIND", PAPER),
            ("OF PAIRING THE CONSOLE", PAPER),
            ("DOES NOT DO.", PAPER),
            E,
            unsaved,
        ],
        PairFail::Lost => [
            ("THE PAD WAS LOST", WARN),
            E,
            ("THE LINK BROKE WHILE PAIRING.", PAPER),
            ("KEEP THE PAD NEAR AND IN", PAPER),
            ("PAIRING MODE, THEN TRY AGAIN.", PAPER),
            E,
            unsaved,
        ],
        PairFail::Store if forgetting => [
            ("COULD NOT FORGET", WARN),
            E,
            ("WRITING THE PAD MEMORY FAILED.", PAPER),
            ("TRY AGAIN LATER.", PAPER),
            E,
            E,
            ("THE PAD IS STILL SAVED.", MUTE),
        ],
        PairFail::Store => [
            ("COULD NOT SAVE", WARN),
            E,
            ("WRITING THE PAD MEMORY FAILED.", PAPER),
            ("TRY AGAIN LATER.", PAPER),
            E,
            E,
            unsaved,
        ],
        PairFail::Busy => [
            ("THE CONSOLE WAS BUSY", WARN),
            E,
            ("THE LIST CHANGED, OR PAIRING", PAPER),
            ("WAS NOT READY. TRY AGAIN", PAPER),
            ("IN A MOMENT.", PAPER),
            E,
            changed,
        ],
    };
    (lines, none, none)
}

// ---- Layout (picture pixels) -------------------------------------------------

/// The BACK button at the top left as drawn: x, y, w, h.
pub const BACK_BOX: (i16, i16, u16, u16) = (2, 2, 32, 11);
/// Where a tap counts as BACK: bigger than the drawn button (the whole
/// top left corner down to the title line), 44 x 17 picture pixels.
pub const BACK_HIT: (i16, i16, u16, u16) = (0, 0, 44, 17);
/// The line under the title.
pub const TITLE_LINE_Y: i16 = 15;
/// Rows: left edge, width, and where the words start.
pub const ROW_X: i16 = 4;
pub const ROW_W: u16 = 152;
pub const TEXT_X: i16 = 8;
/// The SETTINGS page: rows of 15 pixels (30 on the CYD panel).
pub const MAIN_Y: i16 = 19;
pub const MAIN_ROW_H: u16 = 15;
/// The GAMEPADS page: 4 places and PAIR, 15 pixels each (as the
/// SETTINGS page).
pub const PADS_Y: i16 = 18;
pub const PADS_ROW_H: u16 = 15;
/// Where a saved pad's tail starts (after "PAD 1" or "PLAYER 1").
pub const NAME_X: i16 = 34;
pub const P1_NAME_X: i16 = 58;
/// The SEARCHING page's list of found pads: its top, row height, rows
/// shown at once, and where a name starts (after the pad mark).
pub const FOUND_Y: i16 = 31;
pub const FOUND_ROW_H: u16 = 11;
pub const FOUND_ROWS: u8 = 4;
pub const FOUND_NAME_X: i16 = ROW_X + 20;
/// Text lines of the other pages: how many, the first, the step.
pub const LINES: usize = 7;
pub const LINES_Y: i16 = 21;
pub const LINE_STEP: i16 = 9;
/// The search countdown bar.
pub const METER_W: u16 = 100;
pub const METER_Y: i16 = 52;
/// Buttons at the bottom of a page: top, height, width, gap.
pub const BUTTON_Y: i16 = 88;
pub const BUTTON_H: u16 = 15;
pub const BUTTON_W: u16 = 60;
pub const BUTTON_GAP: u16 = 8;
/// The brightness bars on the SETTINGS page.
pub const BRIGHT_BAR_W: u16 = 9;
pub const BRIGHT_BAR_GAP: u16 = 3;
pub const BRIGHT_X: i16 = 94;
pub const SUN_X: i16 = 84;

/// The boot animation's three values, all shown.
#[cfg(feature = "boot-gather")]
pub const ANIM_WORDS: [&str; 3] = ["FULL", "SHORT", "OFF"];
/// The saved places as the GAMEPADS page names them.
pub const PAD_WORDS: [&str; 4] = ["PAD 1", "PAD 2", "PAD 3", "PAD 4"];
const PLACE_DIGITS: [&str; 4] = ["1", "2", "3", "4"];

/// The top of text line `i` of a page with lines.
pub const fn line_y(i: usize) -> i16 {
    LINES_Y + i as i16 * LINE_STEP
}

/// The top of row `r` of the found list.
pub const fn found_row_y(r: u8) -> i16 {
    FOUND_Y + r as i16 * FOUND_ROW_H as i16
}

/// The found pad in the list's first row: the list scrolls so that the
/// chosen pad is shown (`sel` past the list is STOP: the last rows).
pub const fn first_found(sel: u8, n: u8) -> u8 {
    if sel < n {
        sel.saturating_sub(FOUND_ROWS - 1)
    } else {
        n.saturating_sub(FOUND_ROWS)
    }
}

/// The found pad under picture row `y`, when the list starts at `first`.
fn found_row_at(y: u16, first: u8) -> Option<u8> {
    let y = y as i16;
    (0..FOUND_ROWS)
        .find(|&r| (found_row_y(r)..found_row_y(r) + FOUND_ROW_H as i16).contains(&y))
        .map(|r| first + r)
}

/// The top of row `row` of the SETTINGS page.
pub const fn main_row_y(row: u8) -> i16 {
    MAIN_Y + row as i16 * MAIN_ROW_H as i16
}

/// The top of line `l` of the GAMEPADS page (`PLACES` is PAIR).
pub const fn pads_row_y(l: u8) -> i16 {
    PADS_Y + l as i16 * PADS_ROW_H as i16
}

fn main_row_at(y: u16) -> Option<u8> {
    let y = y as i16;
    (0..MAIN_ROWS).find(|&r| (main_row_y(r)..main_row_y(r) + MAIN_ROW_H as i16).contains(&y))
}

fn pads_row_at(y: u16) -> Option<u8> {
    let y = y as i16;
    (0..=PLACES).find(|&l| (pads_row_y(l)..pads_row_y(l) + PADS_ROW_H as i16).contains(&y))
}

/// Which logical line on GAMEPADS a tap at `y` hits (`None` on a gap, on
/// `- NONE -`, or on the disabled PAIR row when a pad is saved).
fn pads_hit(y: u16, seen: &Seen) -> Option<u8> {
    let row = pads_row_at(y)?;
    if seen.count() <= 1 {
        match row {
            0 => seen.single().map(|(place, _)| place),
            1 if seen.count() == 0 => Some(PLACES),
            _ => None,
        }
    } else {
        seen.line_ok(row).then_some(row)
    }
}

/// The left edge of brightness bar `b`.
pub const fn bright_bar_x(b: u8) -> i16 {
    BRIGHT_X + b as i16 * (BRIGHT_BAR_W + BRIGHT_BAR_GAP) as i16
}

/// The step under column `x` on the brightness row (`None` left of the
/// sun).
fn bright_step_at(x: u16) -> Option<u8> {
    let x = x as i16;
    if x < SUN_X {
        return None;
    }
    let i =
        (x - BRIGHT_X + BRIGHT_BAR_GAP as i16 / 2).max(0) / (BRIGHT_BAR_W + BRIGHT_BAR_GAP) as i16;
    Some((i as u8).min(BRIGHT_STEPS - 1))
}

/// The value chip `i` on the boot animation row: left edge and width.
/// They end at the row's right edge.
#[cfg(feature = "boot-gather")]
pub fn chip(i: u8) -> (i16, u16) {
    let w = |k: usize| gfx::width(ANIM_WORDS[k], 1) as u16 + 5;
    let mut x = ROW_X + ROW_W as i16 - 2;
    let mut k = ANIM_WORDS.len();
    while k > usize::from(i) {
        k -= 1;
        x -= w(k) as i16 + 2;
    }
    (x, w(usize::from(i)))
}

#[cfg(feature = "boot-gather")]
fn chip_at(x: u16) -> Option<u8> {
    let x = x as i16;
    (0..3u8).find(|&i| {
        let (cx, w) = chip(i);
        (cx - 1..cx + w as i16 + 1).contains(&x)
    })
}

fn in_box((x, y): (u16, u16), (bx, by, bw, bh): (i16, i16, u16, u16)) -> bool {
    let (x, y) = (x as i16, y as i16);
    (bx..bx + bw as i16).contains(&x) && (by..by + bh as i16).contains(&y)
}

/// Button `b` of `n`: x, y, w, h (centred as a group).
pub fn button_box(n: u8, b: u8) -> (i16, i16, u16, u16) {
    let all = n as i16 * BUTTON_W as i16 + (n as i16 - 1) * BUTTON_GAP as i16;
    let x = CX - all / 2 + b as i16 * (BUTTON_W + BUTTON_GAP) as i16;
    (x, BUTTON_Y, BUTTON_W, BUTTON_H)
}

/// The buttons `words` at the bottom of a page, `sel` chosen (none
/// when `sel` is past them).
fn draw_buttons(f: &mut Frame, words: &[&str], sel: u8) {
    let n = words.len() as u8;
    for (b, word) in words.iter().enumerate() {
        let (x, y, w, h) = button_box(n, b as u8);
        let on = sel == b as u8;
        if on {
            f.rect(x, y, w, h, GOLD);
        } else {
            f.rect(x, y, w, h, PANEL);
            f.outline(x, y, w, h, MUTE);
        }
        let c = if on { INK } else { PAPER };
        gfx::text_centred(f, x + w as i16 / 2, y + 5, word, c, 1);
    }
}

fn draw_meter(f: &mut Frame, y: i16, w: u16, c: u8) {
    let x = CX - METER_W as i16 / 2;
    f.rect(x, y, METER_W, 9, PANEL);
    f.rect(x, y, w.min(METER_W), 9, c);
    f.outline(x - 1, y - 1, METER_W + 2, 11, LINE);
}

fn draw_title(f: &mut Frame, title: &str) {
    let (x, y, w, h) = BACK_BOX;
    f.rect(x, y, w, h, PANEL);
    f.outline(x, y, w, h, MUTE);
    f.text(x + 4, y + 3, "< BACK", PAPER);
    gfx::text_centred(f, CX + 8, 5, title, GOLD, 1);
    f.hline(0, TITLE_LINE_Y, 160, LINE);
}

fn draw_bar(f: &mut Frame, hints: &str) {
    f.rect(0, BAR_Y, 160, BAR_H, PANEL);
    f.hline(0, BAR_Y, 160, LINE);
    // No gear here: in the app it would open nothing.
    f.text(4, BAR_Y + 4, hints, MUTE);
}

// ---- Placement E: GAMEPADS first, SETTINGS last -----------------------------

/// Draws the SETTINGS tile at `(x, y)`: dark, a dashed edge, a big gear.
/// Not a game: no colour of its own, no letter, no picture.
pub fn draw_tile(f: &mut Frame, x: i16, y: i16, _selected: bool) {
    draw_tile_edge(f, x, y);
    let t = TILE as i16;
    draw_gear(f, x + (t - 27) / 2, y + (t - 27) / 2, 27, PAPER);
}

/// Dark tile with its dashed edge, no picture yet.
fn draw_tile_edge(f: &mut Frame, x: i16, y: i16) {
    f.rect(x, y, TILE, TILE, PANEL);
    // A dashed edge: 3 on, 2 off.
    let t = TILE as i16;
    for i in (0..t).step_by(5) {
        let n = 3.min(t - i) as u16;
        f.hline(x + i, y, n, MUTE);
        f.hline(x + i, y + t - 1, n, MUTE);
        f.vline(x, y + i, n, MUTE);
        f.vline(x + t - 1, y + i, n, MUTE);
    }
}

/// Draws a system tile at `(x, y)`: dark, a dashed edge, no colour of
/// its own, no letter, with the tile's own picture (a gamepad or a gear).
pub fn draw_system_tile(f: &mut Frame, x: i16, y: i16, which: SystemTile) {
    match which {
        SystemTile::Settings => draw_tile(f, x, y, false),
        SystemTile::Gamepads => {
            draw_tile_edge(f, x, y);
            let t = TILE as i16;
            let (w, h) = (TILE_PAD[0].len() as i16, TILE_PAD.len() as i16);
            draw_rows(f, x + (t - w) / 2, y + (t - h) / 2, &TILE_PAD, PAPER);
        }
    }
}

/// The gamepad on the GAMEPADS tile, 26 x 16: a d-pad, two small middle
/// buttons and four face buttons cut out of the body, and two grips.
pub const TILE_PAD: [&str; 16] = [
    "..######################..",
    ".########################.",
    "###################..#####",
    "#####..############..#####",
    "#####..###..##..##########",
    "###......#######..####..##",
    "###......#######..####..##",
    "#####..###################",
    "#####..############..#####",
    "###################..#####",
    "##########################",
    "#########........#########",
    "########..........########",
    ".#######..........#######.",
    "..#####............#####..",
    "...###..............###...",
];

/// The words under the row when a system tile is chosen: its name and
/// its line, where a game's name and tag go, in a game's colours.
pub fn draw_system_words(f: &mut Frame, which: SystemTile) {
    gfx::text_centred(f, CX, NAME_Y, which.name(), PAPER, 2);
    gfx::text_centred(f, CX, TAG_Y, which.tag(), MUTE, 1);
}

/// A gear `size` pixels square at `(x, y)` (placements A and B; today's
/// 9 x 9 bar gear scaled up reads as a flower): eight teeth round a ring
/// with a hole. Whole numbers only: distances are compared squared, on
/// a grid of half pixels so the middle can fall between two pixels.
pub fn draw_gear(f: &mut Frame, x: i16, y: i16, size: u16, c: u8) {
    let s = size as i32;
    // Radii and the teeth's half width, in half pixels.
    let outer = s - 1;
    let body = s * 7 / 10;
    let hole = s * 3 / 10;
    let half = (s / 5).max(2);
    // A diagonal tooth as wide as a straight one: |u - v| is sqrt(2)
    // times the distance to the diagonal.
    let diag = half * 17 / 12;
    for j in 0..s {
        for i in 0..s {
            let (u, v) = (2 * i - (s - 1), 2 * j - (s - 1));
            let r2 = u * u + v * v;
            if r2 < hole * hole || r2 > outer * outer {
                continue;
            }
            let tooth = u.abs() <= half
                || v.abs() <= half
                || (u - v).abs() <= diag
                || (u + v).abs() <= diag;
            if r2 <= body * body || tooth {
                f.pixel(x + i as i16, y + j as i16, c);
            }
        }
    }
}

// ---- Placement D: the icon row -----------------------------------------------

/// Draws rows of `#` (colour `c`) and `.` (left alone) at `(x, y)`.
fn draw_rows(f: &mut Frame, x: i16, y: i16, rows: &[&str], c: u8) {
    for (j, row) in rows.iter().enumerate() {
        for (i, b) in row.bytes().enumerate() {
            if b == b'#' {
                f.pixel(x + i as i16, y + j as i16, c);
            }
        }
    }
}

// ---- The ABOUT page -----------------------------------------------------------

fn draw_about(f: &mut Frame, seen: &Seen) {
    gfx::text_centred(f, CX, 24, crate::theme::NAME, PAPER, 2);
    gfx::text_centred(f, CX, 38, "HAND-HELD GAME CONSOLE", MUTE, 1);
    let saved = saved_text(seen.count());
    let rows: [(&str, &str); 3] = [
        ("CONSOLE VERSION", env!("CARGO_PKG_VERSION")),
        ("GAME INTERFACE", api_text()),
        ("GAMEPADS", saved.as_str()),
    ];
    for (i, (k, v)) in rows.iter().enumerate() {
        let y = 54 + i as i16 * 12;
        f.text(TEXT_X + 4, y, k, MUTE);
        gfx::text_right(f, ROW_X + ROW_W as i16 - 8, y, v, PAPER);
        f.hline(ROW_X + 4, y + 8, ROW_W - 8, LINE);
    }
}

// ---- Words --------------------------------------------------------------------

/// "NONE" or "1 SAVED" (or "N SAVED" for legacy multi-pad flash state).
fn saved_text(n: u8) -> Text<16> {
    if n == 0 {
        Text::of("NONE")
    } else {
        Text::joined(PLACE_DIGITS_0[usize::from(n.min(PLACES))], " SAVED")
    }
}

const PLACE_DIGITS_0: [&str; 5] = ["0", "1", "2", "3", "4"];

/// "12 LEFT": the board's free record slots (each save and each
/// forget takes one).
fn writes_text(n: u8) -> Text<8> {
    let mut d = [b'0'; 3];
    let mut k = 3;
    let mut v = n;
    loop {
        k -= 1;
        d[k] = b'0' + v % 10;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let digits = Text::<3>::of_ascii(&d[k..]);
    Text::joined(digits.as_str(), " LEFT")
}

/// The game interface's version as words.
fn api_text() -> &'static str {
    const D: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    D[(game_api::API_VERSION as usize).min(9)]
}

/// "0:47".
fn clock_text(s: u64) -> Text<4> {
    let (m, s) = ((s / 60).min(9) as u8, (s % 60) as u8);
    Text::of_ascii(&[b'0' + m, b':', b'0' + s / 10, b'0' + s % 10])
}

/// "AB:CD": a pad's tail as its address ends (see [`board_api::pair`]).
pub fn tail_text(t: [u8; 2]) -> Text<5> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let h = |b: u8| HEX[usize::from(b & 15)];
    Text::of_ascii(&[h(t[0] >> 4), h(t[0]), b':', h(t[1] >> 4), h(t[1])])
}

/// The widest a text line may be, in pixels (the picture less a margin).
const LINE_MAX: i16 = 150;

/// The width of the big pad mark and its gap before the name, on the
/// FOUND page.
const BIG_MARK_W: i16 = PAD_ICON.w as i16 * 2 + 5;

/// `name`, cut short and ended with ".." when it is wider than `room`
/// pixels, so a long name a pad gives never runs off the picture.
fn cut(name: &str, room: i16) -> Text<PAD_NAME_MAX> {
    if gfx::width(name, 1) <= room {
        return Text::of(name);
    }
    let mut n = name.len().min(PAD_NAME_MAX - 2);
    while n > 0 {
        if let Some(head) = name.get(..n) {
            let t = Text::joined(head, "..");
            if gfx::width(t.as_str(), 1) <= room {
                return t;
            }
        }
        n -= 1;
    }
    Text::of("..")
}

/// The room left for a name on a line that also holds `words`: the
/// line's width less the words and the one-pixel gap between letters.
fn beside(words: &str) -> i16 {
    LINE_MAX - gfx::width(words, 1) - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every failure, for the word checks.
    const FAILS: [PairFail; 11] = [
        PairFail::NotHeld,
        PairFail::Linked,
        PairFail::Full,
        PairFail::NoRoom,
        PairFail::Damaged,
        PairFail::Timeout,
        PairFail::OldPin,
        PairFail::Refused,
        PairFail::Lost,
        PairFail::Store,
        PairFail::Busy,
    ];

    /// The widest name a pad may give: as many of the font's widest
    /// letter as a name holds.
    fn widest_name() -> Text<PAD_NAME_MAX> {
        let widest = (b' '..=b'~')
            .max_by_key(|&c| gfx::width(Text::<1>::of_ascii(&[c]).as_str(), 1))
            .unwrap();
        Text::of_ascii(&[widest; PAD_NAME_MAX])
    }

    #[test]
    fn every_word_fits_the_picture() {
        let fits = |t: &str| gfx::width(t, 1) <= 150;
        let name = widest_name();
        let tail = tail_text([0xDD, 0xDD]);
        // Every fixed text line the app draws.
        for t in [
            // The pages' help lines and tile words.
            "GAMEPADS",
            SystemTile::Gamepads.tag(),
            SystemTile::Settings.tag(),
            "CHANGES AT ONCE",
            "PLAYS AT THE NEXT START",
            "NAME AND VERSION",
            "NO PAD SAVED YET",
            "GREEN PAD: CONNECTED NOW",
            "FORGET BEFORE PAIRING ANOTHER",
            "CHOOSE A PAD TO FORGET IT",
            "THIS BUILD CANNOT PAIR OR FORGET",
            "LEFT: WRITES THE PAD MEMORY HOLDS",
            "HAND-HELD GAME CONSOLE",
            // Searching, found, calling, saved, forget, forgotten.
            "PUT THE PAD IN PAIRING MODE",
            "SEARCHING FOR GAMEPADS",
            "ONLY GAMEPADS ARE SHOWN.",
            "THE CONSOLE STAYS HIDDEN.",
            "FOUND A GAMEPAD",
            "CALL IT AND PAIR?",
            "NOTHING IS CALLED BEFORE YES.",
            "PAIRING...",
            "KEEP THE PAD NEAR",
            "PAIRED AND SAVED",
            "FROM NOW ON IT CONNECTS",
            "BY ITSELF. PAIRING IS OFF.",
            "IT WILL NOT CONNECT AGAIN",
            "UNTIL IT IS PAIRED ONCE MORE.",
            "FORGETTING...",
            "PLAYER 1 IS FORGOTTEN",
            "PAD 4 IS FORGOTTEN",
            "ITS PLACE IS FREE NOW.",
            // The bar.
            "<> CHANGE  B BACK",
            "A CHOOSE  B BACK",
            "A STOP  B BACK",
            "A OK  B BACK",
            "<> CHOOSE  A OK  B BACK",
        ] {
            assert!(fits(t), "{t} is {} wide", gfx::width(t, 1));
        }
        // The joined lines, with the widest name and tail they can hold.
        let n = name.as_str();
        let short = |words: &str| cut(n, beside(words));
        for t in [
            Text::<40>::joined("CALLING ", short("CALLING ").as_str()),
            Text::joined(short(" IS NOW PLAYER 1").as_str(), " IS NOW PLAYER 1"),
            Text::joined(short(" IS NOW PAD 4").as_str(), " IS NOW PAD 4"),
            Text::joined(short(" WANTS A PIN CODE").as_str(), " WANTS A PIN CODE"),
            Text::joined(
                Text::<40>::joined("FORGET PLAYER 1 (", tail.as_str()).as_str(),
                ")?",
            ),
            Text::joined(
                Text::<40>::joined("FORGET PAD 4 (", tail.as_str()).as_str(),
                ")?",
            ),
        ] {
            assert!(fits(t.as_str()), "{} is too wide", t.as_str());
        }
        // Every ending's lines, for pairing and forgetting, before and
        // after YES.
        for why in FAILS {
            for (forgetting, called) in [(false, false), (false, true), (true, false)] {
                let (lines, a, b) = fail_lines(why, forgetting, called, n);
                for (t, _) in lines {
                    assert!(fits(t), "{why:?}: {t} is {} wide", gfx::width(t, 1));
                }
                for t in [a, b] {
                    assert!(fits(t.as_str()), "{why:?}: {} is too wide", t.as_str());
                }
            }
        }
        // Found list rows: the pad mark, then room for a cut name of at
        // least 12 letters before the tail.
        assert!(ROW_X + 4 + PAD_ICON.w as i16 + 2 <= FOUND_NAME_X);
        let room = ROW_X + ROW_W as i16 - 4 - gfx::width(tail.as_str(), 1) - 4 - FOUND_NAME_X;
        assert!(room >= gfx::width("MMMMMMMMMMMM", 1), "{room}");
        // The found pad big on its page has room for a 16-letter name.
        assert!(BIG_MARK_W + gfx::width("MMMMMMMMMMMMMMMM", 1) <= LINE_MAX);
        // The list ends above its last line, and that line above the
        // buttons.
        assert!(found_row_y(FOUND_ROWS) + 3 + 5 < BUTTON_Y);
        assert!(found_row_y(0) > LINES_Y + 5);
        // GAMEPADS rows: Player 1 and legacy multi-pad rows fit without touching.
        assert!(TEXT_X + gfx::width("PLAYER 1", 1) + 4 <= P1_NAME_X);
        let p1_end = P1_NAME_X + gfx::width(tail.as_str(), 1) + 5 + PAD_ICON.w as i16 + 2;
        assert!(p1_end < ROW_X + ROW_W as i16 - 4 - gfx::width("FORGET >", 1));
        assert!(TEXT_X + gfx::width("PAD 4", 1) + 4 <= NAME_X);
        let end = NAME_X + gfx::width(tail.as_str(), 1) + 5 + PAD_ICON.w as i16 + 2;
        assert!(end < ROW_X + ROW_W as i16 - 4 - gfx::width("FORGET >", 1));
        // The PAIR row: its words and the widest right-hand words.
        let left = TEXT_X + gfx::width("+ PAIR PLAYER 1", 1) + 4;
        let right = ROW_X + ROW_W as i16 - 4;
        assert!(TEXT_X + gfx::width("NO PAIRING IN THIS BUILD", 1) <= right);
        for w in ["LIST FULL", "NO ROOM", "1 SAVED", "FORGET ALL"] {
            assert!(right - gfx::width(w, 1) >= left, "{w}");
        }
        // The pairing area holds 127 records.
        let most = writes_text(127);
        assert!(right - 8 - gfx::width(most.as_str(), 1) >= left);
        // Every button word fits its button.
        let mut app = App::new();
        for page in [
            AppPage::Searching,
            AppPage::Found,
            AppPage::Forget,
            AppPage::End,
        ] {
            app.page = page;
            for why in FAILS {
                app.state = PairState::Failed(why);
                for w in app.buttons() {
                    assert!(gfx::width(w, 1) <= BUTTON_W as i16 - 4, "{w}");
                }
            }
        }
        // The BACK tap area holds the drawn button and stops above the
        // first row of every page.
        let (bx, by, bw, bh) = BACK_BOX;
        let (hx, hy, hw, hh) = BACK_HIT;
        assert!(hx <= bx && hy <= by && hx + hw as i16 >= bx + bw as i16);
        assert!(hy + hh as i16 >= by + bh as i16);
        assert!(hy + hh as i16 <= PADS_Y.min(MAIN_Y));
        // The GAMEPADS help line ends above the bar.
        assert!(pads_row_y(PLACES + 1) + 5 + 7 <= BAR_Y);
        assert!(gfx::width("SETTINGS", 2) <= 150);
        // The boot chips end inside the row and start right of the label.
        #[cfg(feature = "boot-gather")]
        {
            let (x0, _) = chip(0);
            assert!(x0 > TEXT_X + gfx::width("BOOT ANIMATION", 1) + 2);
            let (x2, w2) = chip(2);
            assert!(x2 + w2 as i16 <= ROW_X + ROW_W as i16);
        }
        assert!(bright_bar_x(4) + BRIGHT_BAR_W as i16 <= ROW_X + ROW_W as i16 - 2);
        assert!(SUN_X > TEXT_X + gfx::width("BRIGHTNESS", 1) + 4);
        // The help line under the SETTINGS rows ends above the bar.
        assert!(main_row_y(MAIN_ROWS) + 6 + 5 <= BAR_Y);
    }

    #[test]
    fn long_names_are_cut_to_their_room() {
        let name = widest_name();
        for room in [0, 10, 40, 77, 100, 150] {
            let t = cut(name.as_str(), room);
            assert!(gfx::width(t.as_str(), 1) <= room.max(gfx::width("..", 1)));
            let whole = gfx::width(name.as_str(), 1) <= room;
            assert_eq!(t.as_str().ends_with(".."), !whole, "{room}");
        }
        assert_eq!(cut("TEST PAD A", 150).as_str(), "TEST PAD A");
        assert_eq!(cut("", 0).as_str(), "");
        let t = cut("ABCDEFGHIJKLMNOPQRSTUVWX", 1000);
        assert_eq!(t.as_str(), "ABCDEFGHIJKLMNOPQRSTUVWX");
    }

    #[test]
    fn the_app_is_small() {
        assert_eq!(core::mem::size_of::<App>(), 24);
    }

    #[test]
    fn words_of_numbers() {
        assert_eq!(tail_text([0xA0, 0x01]).as_str(), "A0:01");
        assert_eq!(tail_text([0x0F, 0xFF]).as_str(), "0F:FF");
        assert_eq!(writes_text(0).as_str(), "0 LEFT");
        assert_eq!(writes_text(7).as_str(), "7 LEFT");
        assert_eq!(writes_text(127).as_str(), "127 LEFT");
        assert_eq!(clock_text(59).as_str(), "0:59");
        assert_eq!(clock_text(60).as_str(), "1:00");
        assert_eq!(saved_text(0).as_str(), "NONE");
        assert_eq!(saved_text(1).as_str(), "1 SAVED");
        assert_eq!(saved_text(3).as_str(), "3 SAVED");
    }

    #[test]
    fn leaving_a_page_cancels_or_acknowledges() {
        use AppPage as P;
        let open = |p| (true, p);
        let shut = (false, P::Main);
        // A pairing page left for a page outside pairing, or out.
        for p in [P::Searching, P::Found, P::Calling, P::Forgetting] {
            assert_eq!(leave_request(open(p), open(P::Pads)), Some(PairReq::Cancel));
            assert_eq!(leave_request(open(p), shut), Some(PairReq::Cancel));
            assert_eq!(leave_request(open(p), open(p)), None);
            assert_eq!(leave_request(open(p), open(P::End)), None);
        }
        // Between pairing pages: nothing.
        assert_eq!(leave_request(open(P::Searching), open(P::Found)), None);
        assert_eq!(leave_request(open(P::Found), open(P::Searching)), None);
        assert_eq!(leave_request(open(P::Found), open(P::Calling)), None);
        // An ending, left in any way except TRY AGAIN directly back to Searching.
        assert_eq!(
            leave_request(open(P::End), open(P::Pads)),
            Some(PairReq::Ack)
        );
        assert_eq!(
            leave_request(open(P::End), open(P::About)),
            Some(PairReq::Ack)
        );
        assert_eq!(leave_request(open(P::End), shut), Some(PairReq::Ack));
        assert_eq!(leave_request(open(P::End), open(P::Searching)), None);
        // Pages with no request.
        for p in [P::Main, P::Pads, P::About, P::Forget] {
            assert_eq!(leave_request(open(p), open(P::Pads)), None);
            assert_eq!(leave_request(open(p), shut), None);
        }
        assert_eq!(leave_request(shut, open(P::Main)), None);
    }

    #[test]
    fn the_settings_lines_are_in_order() {
        assert_eq!(MAIN_LINES[0], MainLine::Bright);
        assert_eq!(MAIN_LINES[usize::from(ABOUT_LINE)], MainLine::About);
        assert_eq!(ABOUT_LINE, MAIN_ROWS - 1);
    }

    #[test]
    fn a_new_app_is_closed_and_on_its_first_page() {
        let a = App::new();
        assert!(!a.is_open());
        assert!(!a.pairing_page());
        assert!(!a.moving());
        assert_eq!(
            (a.page(), a.entry(), a.state()),
            (AppPage::Main, Entry::Off, PairState::Off)
        );
    }
}
