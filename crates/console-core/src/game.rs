//! A game running: calling it, its input, its time, its video settings,
//! the pause menu, and keeping its picture under the console's overlays.
//!
//! The board starts the game ([`board_api::Board::start_game`]) and hands
//! back its two functions. From then on this module calls `update` once
//! per frame (or, at the on-demand rate, only when something happened),
//! with a `GameFrame` made on the stack for that one call (a `LiveFrame`:
//! it can only point at the real picture, palette, input and log, so the
//! console needs no `unsafe` to call the game). The game draws
//! straight into the console's picture and palette.
//!
//! Notifications and the pause menu are drawn over the game's picture
//! just before it is sent, and the pixels under them are put back right
//! after. So the game always finds its own picture, as it left it.
//!
//! Memory, kept for as long as the console runs (in [`Run`]):
//!
//! | what                                        | bytes |
//! |---------------------------------------------|-------|
//! | picture under the notifications (160 x 16)  | 2,560 |
//! | picture under the pause menu (80 x 36)      | 2,880 |
//! | last input handed to the game (4 players)   |   176 |
//! | the game's table, times, settings, the rest |   128 |
//! | **`Run`** (PC; 5,736 on the ESP32)          | 5,744 |
//!
//! Only during a call, on the stack: the `GameFrame` (104 bytes on the
//! chip), the input (176) and the log buffer ([`LOG_CAP`]).

use board_api::{GameCallbacks, LogOut};
use console_video::{Frame, Video, W};
use game_api::{
    GameFrame, Input as GameInput, LiveFrame, PLAYERS, PlayerInput, answer, event, exit_reason,
    input_flag, mode, picture, player_flag, rate, request, status,
};

use crate::gamein::{Raw, RawPad};
use crate::gfx::{self, Text};
use crate::input::{Input, act};
use crate::pause::{self, Choice};
use crate::theme::{MENU_H, MENU_W, MENU_X, MENU_Y, NOTE_H, NOTE_Y};

/// Bytes of log text a game may write in one call.
pub const LOG_CAP: usize = 256;
/// Longest log line printed (longer ones are cut).
pub const LOG_LINE: usize = 96;

/// Time between calls at 30 and at 60 frames a second.
pub const PERIOD_30_US: u64 = 33_333;
pub const PERIOD_60_US: u64 = 16_667;
/// At the on-demand rate, the shortest time between two calls woken by
/// input: the fastest rate offered (60 a second).
pub const MIN_CALL_US: u64 = PERIOD_60_US;

/// What this console offers (bit `n`: value `n`). Hints and colour
/// settings are taken and kept, but change nothing yet.
pub const OFFERED_MODES: u32 = 1 << mode::PIXELS_160X120;
pub const OFFERED_RATES: u32 = (1 << rate::FPS30) | (1 << rate::FPS60) | (1 << rate::ON_DEMAND);
pub const OFFERED_HINTS: u32 = 0b111;
pub const OFFERED_COLOURS: u32 = 0b11;

/// Rows at the top kept under a notification (it rests at NOTE_Y and
/// slides from above).
const BAND_ROWS: usize = (NOTE_Y as usize) + (NOTE_H as usize);
const MENU_BYTES: usize = MENU_W as usize * MENU_H as usize;

/// How a game ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ended {
    /// It said it is over.
    Done,
    /// It stopped with an error (its status: `ABORTED`, or a value the
    /// console does not know).
    Aborted(u32),
    /// The player chose "Back to home".
    Quit,
}

/// The picture under the overlays, kept while they are on show.
struct Under {
    band: [u8; BAND_ROWS * W],
    menu: [u8; MENU_BYTES],
    band_kept: bool,
    menu_kept: bool,
}

impl Under {
    const fn new() -> Self {
        Under {
            band: [0; BAND_ROWS * W],
            menu: [0; MENU_BYTES],
            band_kept: false,
            menu_kept: false,
        }
    }

    fn keep(&mut self, f: &Frame, band: bool, menu: bool) {
        if band {
            self.band.copy_from_slice(&f.0[..BAND_ROWS * W]);
        }
        if menu {
            for (r, row) in self
                .menu
                .as_chunks_mut::<{ MENU_W as usize }>()
                .0
                .iter_mut()
                .enumerate()
            {
                let at = (MENU_Y as usize + r) * W + MENU_X as usize;
                row.copy_from_slice(&f.0[at..at + MENU_W as usize]);
            }
        }
        self.band_kept = band;
        self.menu_kept = menu;
    }

    fn put_back(&mut self, f: &mut Frame) {
        if self.band_kept {
            f.0[..BAND_ROWS * W].copy_from_slice(&self.band);
        }
        if self.menu_kept {
            for (r, row) in self
                .menu
                .as_chunks::<{ MENU_W as usize }>()
                .0
                .iter()
                .enumerate()
            {
                let at = (MENU_Y as usize + r) * W + MENU_X as usize;
                f.0[at..at + MENU_W as usize].copy_from_slice(row);
            }
        }
        self.band_kept = false;
        self.menu_kept = false;
    }
}

/// The running game, or none.
pub struct Run {
    cb: Option<GameCallbacks>,
    folder: Text<8>,
    mode: u16,
    rate: u16,
    hint: u16,
    colour: u16,
    answer: u16,
    /// Calls of `update` so far in this run.
    frames: u32,
    /// Clock time at the start, and all time spent paused since.
    start_us: u64,
    paused_us: u64,
    /// Clock time the pause menu opened, while it shows.
    paused_at: Option<u64>,
    sel: u8,
    /// Game time of the last call, and the game's timer (0: none).
    last_call_us: u64,
    wake_at_us: u64,
    /// Events for the next call.
    events: u32,
    /// The input handed to the last call.
    sent: GameInput,
    /// Buttons held when the game started or resumed, per player: hidden
    /// from the game until let go. The same for a touch.
    hide: [u32; PLAYERS],
    hide_touch: bool,
    boot_was: bool,
    /// On demand: an input change came too soon after the last call and
    /// waits for [`Run::timer_due`].
    held_back: bool,
    under: Under,
}

impl Run {
    pub const fn new() -> Self {
        Run {
            cb: None,
            folder: Text::new(),
            mode: mode::PIXELS_160X120,
            rate: rate::FPS30,
            hint: 0,
            colour: 0,
            answer: answer::NONE,
            frames: 0,
            start_us: 0,
            paused_us: 0,
            paused_at: None,
            sel: 0,
            last_call_us: 0,
            wake_at_us: 0,
            events: 0,
            // All zeros, even its size and version: no game was called
            // yet, and `begin` puts in a fresh one before the first call.
            // Zeros keep the console's `static` out of the chip's flash
            // (see the README, "Starting as zeros").
            sent: GameInput {
                size: 0,
                version: 0,
                ..GameInput::new()
            },
            hide: [0; PLAYERS],
            hide_touch: false,
            boot_was: false,
            held_back: false,
            under: Under::new(),
        }
    }

    // ---- What tests and boards may look at -------------------------------

    /// Whether a game runs (paused or not).
    pub fn running(&self) -> bool {
        self.cb.is_some()
    }

    /// The running game's folder ("" when none).
    pub fn folder(&self) -> &str {
        self.folder.as_str()
    }

    /// Whether the pause menu shows.
    pub fn paused(&self) -> bool {
        self.paused_at.is_some()
    }

    /// The pause menu's chosen item.
    pub fn menu_item(&self) -> u8 {
        self.sel
    }

    /// The frame rate in use ([`rate`]).
    pub fn rate(&self) -> u16 {
        self.rate
    }

    /// The answer to the game's last video request ([`answer`]).
    pub fn answer(&self) -> u16 {
        self.answer
    }

    /// Calls of `update` so far.
    pub fn frames(&self) -> u32 {
        self.frames
    }

    /// The input handed to the last call. Before the first game starts,
    /// all zeros (even `size` and `version`).
    pub fn last_input(&self) -> &GameInput {
        &self.sent
    }

    /// Game time at clock time `now_us`: pauses not counted.
    pub fn game_time(&self, now_us: u64) -> u64 {
        let until = self.paused_at.unwrap_or(now_us);
        until.saturating_sub(self.start_us + self.paused_us)
    }

    /// Time between calls at the rate in use; `None` at on-demand.
    pub fn period_us(&self) -> Option<u64> {
        match self.rate {
            rate::FPS60 => Some(PERIOD_60_US),
            rate::ON_DEMAND => None,
            _ => Some(PERIOD_30_US),
        }
    }

    /// With the on-demand rate, when the game's timer falls due (clock
    /// time).
    /// Also when an input change is held back (see [`MIN_CALL_US`]).
    pub fn timer_due(&self) -> Option<u64> {
        if self.paused() {
            return None;
        }
        // Saturating: a game may ask for the end of time (u64::MAX).
        let base = self.start_us.saturating_add(self.paused_us);
        let timer = (self.wake_at_us != 0).then(|| base.saturating_add(self.wake_at_us));
        let held = self
            .held_back
            .then(|| base.saturating_add(self.last_call_us.saturating_add(MIN_CALL_US)));
        match (timer, held) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    // ---- Start, run, end ---------------------------------------------------

    /// A new game starts: clears the picture to number 0 and puts the
    /// start colours in the game's part of the palette. Its first call
    /// comes with [`Run::step`] in the same frame.
    pub fn begin(
        &mut self,
        cb: GameCallbacks,
        folder: &str,
        now_us: u64,
        raw: &Raw,
        boot_held: bool,
        video: &mut Video,
    ) {
        // Field by field: a whole new `Run` would be a 5 KB temporary
        // on the stack.
        self.cb = Some(cb);
        self.folder = Text::of(folder);
        self.mode = mode::PIXELS_160X120;
        self.rate = rate::FPS30;
        self.hint = 0;
        self.colour = 0;
        self.answer = answer::NONE;
        self.frames = 0;
        self.start_us = now_us;
        self.paused_us = 0;
        self.paused_at = None;
        self.sel = 0;
        self.last_call_us = 0;
        self.wake_at_us = 0;
        self.events = event::START | event::REDRAW;
        self.sent = GameInput::new();
        self.hide_held(raw);
        self.hide_touch = raw.touch.is_some();
        self.boot_was = boot_held;
        self.held_back = false;
        self.under.band_kept = false;
        self.under.menu_kept = false;
        video.frame_mut().clear(0);
        let p = video.palette_mut();
        for i in 0..picture::GAME_COLOURS as u8 {
            p.set(i, picture::start_colour(i));
        }
        guard_console_colours(p);
    }

    /// One frame while a game runs. `input` is the console's own input
    /// (for Home and the pause menu); `raw` the pad as the game would see
    /// it. `boot_noted`: the board noted a press of its BOOT button since
    /// the last frame (it may be up again). `bright` is the brightness
    /// step, which the pause menu may change. Returns how the game ended,
    /// if it did.
    #[allow(clippy::too_many_arguments)]
    pub fn step<L: LogOut>(
        &mut self,
        now_us: u64,
        input: &Input,
        boot_held: bool,
        boot_noted: bool,
        raw: &Raw,
        bright: &mut u8,
        video: &mut Video,
        log: &mut L,
    ) -> Option<Ended> {
        let boot_pressed = (boot_held && !self.boot_was) || boot_noted;
        self.boot_was = boot_held;
        // Presses noted by the board in the frame the menu closed belong
        // to the menu, not to the game.
        let mut raw = *raw;
        if let Some(at) = self.paused_at {
            match pause::update(pause::Kind::Game, &mut self.sel, bright, input) {
                Choice::Stay => return None,
                // Not in the menu over a game.
                #[cfg(feature = "settings")]
                Choice::Gamepads | Choice::Settings => return None,
                Choice::Home => {
                    log.line("console: back to home");
                    self.call_exit(exit_reason::PLAYER, video, log);
                    return Some(Ended::Quit);
                }
                Choice::Resume => {
                    self.paused_us += now_us.saturating_sub(at);
                    self.paused_at = None;
                    self.events |= event::RESUME;
                    self.hide_held(&raw);
                    self.hide_touch = raw.touch.is_some();
                    raw = raw.without_noted();
                    log.line("console: game resumed");
                }
            }
        } else if input.acts & act::HOME != 0 || boot_pressed {
            self.paused_at = Some(now_us);
            self.sel = 0;
            log.line("console: game paused");
            return None;
        }

        let gi = self.game_input(&raw);
        let now_game = self.game_time(now_us);
        let due = match self.rate {
            rate::ON_DEMAND => {
                let timer = self.wake_at_us != 0 && now_game >= self.wake_at_us;
                // A short press or tap (over before this frame) shows only
                // in `pressed`, `released` and the touch start/end flags.
                let now = wakes_now(&gi) != wakes_now(&self.sent) || short_input(&gi);
                // Sticks, triggers and a moving finger are taken at most
                // every MIN_CALL_US. A board wakes the console at every
                // pad report; with a stick that shakes a little, that
                // would call the game up to a thousand times a second. A
                // change that comes too soon waits (the console comes
                // back for it).
                let later = wakes_later(&gi) != wakes_later(&self.sent);
                let soon =
                    self.frames > 0 && now_game < self.last_call_us.saturating_add(MIN_CALL_US);
                let call = self.events != 0 || timer || now || (later && !soon);
                self.held_back = later && !call;
                call
            }
            _ => true,
        };
        if !due {
            return None;
        }
        self.sent = gi;
        let st = self.call_update(now_game, video, log);
        match st {
            status::RUN => None,
            status::DONE => {
                self.cb = None;
                Some(Ended::Done)
            }
            other => {
                self.cb = None;
                Some(Ended::Aborted(other))
            }
        }
    }

    /// Clears what is left of a game that ended (its table is dropped).
    pub fn clear(&mut self) {
        self.cb = None;
        self.paused_at = None;
        self.held_back = false;
        self.under.band_kept = false;
        self.under.menu_kept = false;
    }

    /// Hides every button held now, for each player, until let go.
    fn hide_held(&mut self, raw: &Raw) {
        for (h, p) in self.hide.iter_mut().zip(&raw.players) {
            *h = p.buttons;
        }
    }

    /// The input the game sees now: buttons held since the start or the
    /// resume stay hidden until let go; presses and releases count from
    /// the last call. Each player on its own (see [`player_input`]).
    ///
    /// A noted tap counts even when the finger is up again: touch start
    /// and end in one call, at the place it started.
    fn game_input(&mut self, raw: &Raw) -> GameInput {
        if raw.touch.is_none() || raw.tap.is_some() {
            self.hide_touch = false;
        }
        let touch = if self.hide_touch { None } else { raw.touch };
        let tap = if self.hide_touch { None } else { raw.tap };
        let was_touching = self.sent.flags & input_flag::TOUCH != 0;
        let mut flags = 0;
        let (touch_x, touch_y) = match (touch, tap) {
            (Some(at), _) => {
                flags |= input_flag::TOUCH;
                if !was_touching {
                    flags |= input_flag::TOUCH_START;
                } else if tap.is_some() {
                    // Lifted and touched again since the last call.
                    flags |= input_flag::TOUCH_START | input_flag::TOUCH_END;
                }
                at
            }
            (None, Some(at)) => {
                // A short tap: started and ended since the last call.
                flags |= input_flag::TOUCH_START | input_flag::TOUCH_END;
                at
            }
            (None, None) => {
                if was_touching {
                    flags |= input_flag::TOUCH_END;
                }
                (self.sent.touch_x, self.sent.touch_y)
            }
        };
        let mut gi = GameInput {
            flags,
            touch_x,
            touch_y,
            ..GameInput::new()
        };
        for n in 0..PLAYERS {
            gi.players[n] = player_input(&mut self.hide[n], &raw.players[n], &self.sent.players[n]);
        }
        gi
    }

    /// The frame for one call. It borrows the picture, the palette, the
    /// input and the log buffer until it is dropped; `LiveFrame` fills in
    /// the pointers and the sizes from them. `None` only if the picture
    /// were not on a 4-byte boundary (it always is: `Frame` is aligned).
    fn frame<'a>(
        &self,
        now_game: u64,
        video: &'a mut Video,
        input: &'a GameInput,
        log: &'a mut [u8; LOG_CAP],
    ) -> Option<LiveFrame<'a>> {
        let (pixels, palette) = video.game_buffers();
        let numbers = GameFrame {
            time_us: now_game,
            frame: self.frames,
            delta_us: now_game
                .saturating_sub(self.last_call_us)
                .min(u32::MAX as u64) as u32,
            events: self.events,
            offered_modes: OFFERED_MODES,
            offered_rates: OFFERED_RATES,
            offered_hints: OFFERED_HINTS,
            offered_colours: OFFERED_COLOURS,
            game_colours: picture::GAME_COLOURS,
            mode: self.mode,
            rate: self.rate,
            hint: self.hint,
            colour: self.colour,
            answer: self.answer,
            ..GameFrame::new()
        };
        LiveFrame::new(numbers, pixels, palette, input, log)
    }

    fn call_update<L: LogOut>(&mut self, now_game: u64, video: &mut Video, log: &mut L) -> u32 {
        let Some(cb) = self.cb else {
            return status::ABORTED;
        };
        let input = self.sent;
        let mut text = [0u8; LOG_CAP];
        let Some(mut live) = self.frame(now_game, video, &input, &mut text) else {
            return status::ABORTED;
        };
        let st = cb.update.call(&mut live);
        // Only the numbers the game wrote back are kept. The frame's
        // borrow of the buffers ends here, after its last use.
        let f = *live.get();
        self.events = 0;
        self.frames = self.frames.wrapping_add(1);
        self.last_call_us = now_game;
        self.wake_at_us = f.wake_at_us;
        self.after_call(&f, &text, video, log);
        if f.requests & request::VIDEO != 0 {
            self.answer = self.take_video(&f);
        }
        st
    }

    fn call_exit<L: LogOut>(&mut self, reason: u32, video: &mut Video, log: &mut L) {
        let Some(cb) = self.cb.take() else {
            return;
        };
        let input = self.sent;
        let mut text = [0u8; LOG_CAP];
        let now_game = self.last_call_us;
        let Some(mut live) = self.frame(now_game, video, &input, &mut text) else {
            return;
        };
        cb.exit.call(&mut live, reason);
        let f = *live.get();
        self.after_call(&f, &text, video, log);
    }

    /// What every call leaves behind: the log, and palette changes.
    fn after_call<L: LogOut>(
        &self,
        f: &GameFrame,
        text: &[u8; LOG_CAP],
        video: &mut Video,
        log: &mut L,
    ) {
        print_log(&text[..usize::from(f.log_len).min(LOG_CAP)], log);
        let p = video.palette_mut();
        if f.requests & request::PALETTE != 0 {
            p.touch();
        }
        guard_console_colours(p);
    }

    /// Checks a video request against what is offered; takes it when all
    /// of it is. Returns the answer.
    fn take_video(&mut self, f: &GameFrame) -> u16 {
        let has = |bits: u32, v: u16| v < 32 && bits & (1 << v) != 0;
        if !has(OFFERED_MODES, f.ask_mode) {
            return answer::NO_MODE;
        }
        if !has(OFFERED_RATES, f.ask_rate) {
            return answer::NO_RATE;
        }
        if !has(OFFERED_HINTS, f.ask_hint) {
            return answer::NO_HINT;
        }
        if !has(OFFERED_COLOURS, f.ask_colour) {
            return answer::NO_COLOUR;
        }
        self.mode = f.ask_mode;
        self.rate = f.ask_rate;
        self.hint = f.ask_hint;
        self.colour = f.ask_colour;
        answer::OK
    }

    // ---- Overlays ------------------------------------------------------

    /// Draws the pause menu (when it shows) over the game's picture,
    /// after keeping what is under it and under the notification band.
    /// Call [`Run::put_back`] once the picture was sent. `bright` is the
    /// brightness step the menu shows.
    pub fn cover(&mut self, f: &mut Frame, notices: bool, bright: u8) {
        let menu = self.paused();
        self.under.keep(f, notices, menu);
        if menu {
            pause::draw(f, pause::Kind::Game, self.sel, bright);
        }
    }

    /// Puts the game's own pixels back where the overlays were.
    pub fn put_back(&mut self, f: &mut Frame) {
        self.under.put_back(f);
    }
}

impl Default for Run {
    fn default() -> Self {
        Self::new()
    }
}

/// One player's input: what `pad` holds now, minus `hide` (buttons held
/// since the start or the resume, until let go), with presses and
/// releases counted from `was` (this player's input at the last call).
///
/// A press the board noted (`pad.pressed`) counts even when the button is
/// up again: the game then sees it pressed AND released in the same
/// call. A pad that went away holds nothing, so all it held is released.
fn player_input(hide: &mut u32, pad: &RawPad, was: &PlayerInput) -> PlayerInput {
    // A noted press of a hidden button: it was let go and pressed again,
    // so it is a new press and no longer hidden.
    *hide &= pad.buttons & !pad.pressed;
    let held = pad.buttons & !*hide;
    let noted = pad.pressed & !*hide;
    let was_held = was.held;
    let mut flags = 0;
    if pad.pad {
        flags |= player_flag::PAD;
    }
    let (accel, gyro) = match pad.motion {
        Some(m) => {
            flags |= player_flag::MOTION;
            (m.accel, m.gyro)
        }
        None => ([0; 3], [0; 3]),
    };
    PlayerInput {
        held,
        pressed: (held & !was_held) | noted,
        // Up now after a noted press, or noted while it was already down
        // at the last call (let go, then pressed again).
        released: (was_held & !held) | (noted & (was_held | !held)),
        flags,
        left_x: pad.left.0,
        left_y: pad.left.1,
        right_x: pad.right.0,
        right_y: pad.right.1,
        left_trigger: pad.triggers.0,
        right_trigger: pad.triggers.1,
        accel,
        gyro,
    }
}

/// What makes an on-demand game wake at once: a button of any player, a
/// pad coming or going, a finger down or up. Never held back, so no
/// press is lost.
fn wakes_now(i: &GameInput) -> ([(u32, u32); PLAYERS], u32) {
    let mut pads = [(0, 0); PLAYERS];
    for (w, p) in pads.iter_mut().zip(&i.players) {
        *w = (p.held, p.flags & player_flag::PAD);
    }
    (pads, i.flags & input_flag::TOUCH)
}

/// A press, a release or a touch start or end that the levels alone do
/// not show (a short press or tap the board noted). Wakes at once.
fn short_input(i: &GameInput) -> bool {
    i.players.iter().any(|p| p.pressed != 0 || p.released != 0)
        || i.flags & (input_flag::TOUCH_START | input_flag::TOUCH_END) != 0
}

/// What wakes it too, but at most every [`MIN_CALL_US`]: the sticks and
/// triggers of every player, and the finger moving. Not motion. These are
/// levels, so a change held back and then undone loses nothing.
fn wakes_later(i: &GameInput) -> ([[i16; 6]; PLAYERS], (i16, i16)) {
    let mut pads = [[0; 6]; PLAYERS];
    for (w, p) in pads.iter_mut().zip(&i.players) {
        *w = [
            p.left_x,
            p.left_y,
            p.right_x,
            p.right_y,
            // Only compared: `as` keeps every u16 apart.
            p.left_trigger as i16,
            p.right_trigger as i16,
        ];
    }
    let touching = i.flags & input_flag::TOUCH != 0;
    (
        pads,
        if touching {
            (i.touch_x, i.touch_y)
        } else {
            (0, 0)
        },
    )
}

/// The console's 16 colours stay the console's, whatever a game wrote:
/// 240 to 247 as the theme says, 248 to 255 black.
fn guard_console_colours(p: &mut console_video::Palette) {
    gfx::set_console_colours(p);
    for i in 248..=255 {
        p.set(i, 0);
    }
}

/// Prints the game's log text, one line per `\n`, each starting with
/// "game: ". Signs other than printable ASCII show as `?`.
fn print_log<L: LogOut>(text: &[u8], log: &mut L) {
    const PREFIX: &[u8] = b"game: ";
    for line in text.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let mut buf = [0u8; PREFIX.len() + LOG_LINE];
        buf[..PREFIX.len()].copy_from_slice(PREFIX);
        let n = line.len().min(LOG_LINE);
        for (d, &s) in buf[PREFIX.len()..].iter_mut().zip(&line[..n]) {
            *d = if s.is_ascii_graphic() || s == b' ' {
                s
            } else {
                b'?'
            };
        }
        if let Ok(s) = core::str::from_utf8(&buf[..PREFIX.len() + n]) {
            log.line(s);
        }
    }
}

// The contract's picture is the renderer's picture.
const _: () = {
    assert!(picture::W as usize == console_video::W);
    assert!(picture::H as usize == console_video::H);
};
