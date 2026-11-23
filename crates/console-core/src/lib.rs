//! The console itself, the same on every board.
//!
//! [`Console::step`] does ONE frame of work and returns at once; it never
//! waits. The board runs the loop around it (see `board_api::time`):
//!
//! ```text
//! let mut console = Console::new();
//! loop {
//!     let wake = console.step(&mut board);
//!     // sleep until wake.deadline(), or until input if wake.on_input()
//! }
//! ```
//!
//! Inside one step, in this order: read the clock once; if nothing is
//! due, return; read the input; update the page on show (boot, home, or
//! a running game); draw the page, the bottom bar and the notifications
//! into the 160x120 picture; hand the changes to the board; say when to
//! come back.
//!
//! The pages are a simple state machine: [`Page::Boot`], then
//! [`Page::Home`], and [`Page::Game`] while a game runs. Each page is its
//! own module ([`game`] for a running game, with [`gamein`] for its input
//! and [`pause`] for the menu Home opens). The look (colours, sizes,
//! timings) is in [`theme`]; which button does what is in [`input`].
//!
//! The console menu ([`pause`]) opens over a game (Home or BOOT) and on
//! the home screen (Home, or a tap on the gear in the bar). Both have a
//! BRIGHTNESS line with five steps ([`theme::BACKLIGHT`]). The console
//! lights the screen ([`board_api::Screen::backlight`]) once, just after
//! its first picture was sent, and again only when the step changes.
//! The step is not saved here: a board sets it at start
//! ([`Console::set_brightness`]) and saves it when
//! [`Console::take_settings_changed`] says so.
//!
//! With the feature `boot-gather` the boot page plays the GATHER
//! animation instead (module `gather`; `boot` is then `boot_gather.rs`) and
//! the home menu gets a BOOT ANIM line (FULL, SHORT, OFF), kept by the
//! board like the brightness (`Console::set_boot_anim`).
//!
//! When something moves (the boot animation, the row scrolling, a
//! notification sliding, a game at 30 or 60 frames a second), the
//! console asks for the next frame. When nothing moves, it asks to be
//! called on input, or when the next change is due (the bar's minute, a
//! notification leaving, a key repeat, a card check, an on-demand game's
//! timer). So an idle home screen lets the chip sleep, and sends nothing.
//!
//! Memory (no heap, fixed sizes). PC numbers from `tests/memory.rs`;
//! the ESP32's whole `Console` from the firmware's `CONSOLE` symbol.
//! Home's game list and tile pictures are not in the `Console`: they
//! are in the RAM the board lends while no game is loaded (the empty
//! game data slot on the chip; see [`lent`] and `board_api::lent`).
//!
//! | what                                    | PC     |
//! |-----------------------------------------|--------|
//! | renderer (`Video`: 2 pictures, palette) | 39,064 |
//! | home (selection, scroll, card state, which picture in which slot) | 72 |
//! | notification queue                      |    160 |
//! | running game (`game::Run`; 5,440 keep the picture under the overlays) | 5,744 |
//! | input (touch: 72), boot, bar, clock times, brightness, the rest | 152 |
//! | **`Console`** (ESP32: 45,168; 8 more on both with `boot-gather`) | 45,192 |
//! | in the lent RAM: stamp, game list 32 x 41, pictures 6 x 680 | 5,396 |

#![no_std]
#![forbid(unsafe_code)]

pub mod art;
pub mod bar;
// The boot page: today's boot screen, or with `boot-gather` the GATHER
// animation (`gather`) with its setting. Without the feature boot.rs is
// used, unchanged.
#[cfg(not(feature = "boot-gather"))]
pub mod boot;
#[cfg(feature = "boot-gather")]
#[path = "boot_gather.rs"]
pub mod boot;
pub mod game;
pub mod gamein;
#[cfg(feature = "boot-gather")]
pub mod gather;
pub mod gfx;
pub mod home;
pub mod input;
pub mod lent;
pub mod notify;
mod out;
pub mod pause;
pub mod theme;

pub use board_api;
pub use console_video;
pub use game_api;

use board_api::{
    Board, Clock, Keys, LogOut, Pad, Screen, SendReport, StartError, Touch, Wake, key,
};
use console_video::{Video, View};
use game_api::{OpenError, PLAYERS};

use crate::game::Ended;

use crate::home::{CardChange, HomeAction};
use crate::input::{Input, InputState, Noted, act};
use crate::notify::Tone;
use crate::theme::BRIGHT_STEPS;

/// Time between frames at the normal rate: 30 frames a second.
pub const FRAME_US: u64 = 33_333;

/// The messages the console shows (each fits [`notify::NOTICE_LEN`]).
pub mod msg {
    pub const PAD_ON: &str = "PAD CONNECTED";
    pub const PAD_OFF: &str = "PAD DISCONNECTED";
    /// The same for each pad: pad 1 (the one that drives the menus) says
    /// no number, the others say theirs.
    pub const PADS_ON: [&str; 4] = [
        PAD_ON,
        "PAD 2 CONNECTED",
        "PAD 3 CONNECTED",
        "PAD 4 CONNECTED",
    ];
    pub const PADS_OFF: [&str; 4] = [
        PAD_OFF,
        "PAD 2 DISCONNECTED",
        "PAD 3 DISCONNECTED",
        "PAD 4 DISCONNECTED",
    ];
    pub const NO_START: &str = "GAMES CANNOT START YET";
    pub const CARD_OUT: &str = "CARD REMOVED";
    pub const CARD_IN: &str = "CARD READY";
    pub const GAME_NOT_FOUND: &str = "GAME NOT FOUND";
    pub const GAME_DAMAGED: &str = "GAME FILE IS DAMAGED";
    pub const GAME_TOO_OLD: &str = "GAME IS TOO OLD";
    pub const GAME_REFUSED: &str = "GAME NEEDS A NEWER CONSOLE";
    pub const GAME_ENDED: &str = "GAME ENDED";
    pub const GAME_ABORTED: &str = "GAME STOPPED: ERROR";
}

/// The page on show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Boot,
    Home,
    /// A game runs (its pause menu too).
    Game,
}

/// The whole console. Make it once and keep it (on the chip, in a
/// `static`): `new` is `const`.
pub struct Console {
    video: Video,
    input: InputState,
    boot: boot::Boot,
    home: home::Home,
    notices: notify::Notices,
    run: game::Run,
    page: Page,
    /// The clock at the first step.
    start_us: u64,
    /// Which pads were connected at the last step (bit `n`: pad `n`).
    pads: u8,
    /// Frames done so far.
    frames: u32,
    /// When the next frame is due while something moves.
    next_us: u64,
    /// What the last step answered.
    wake: Wake,
    last_report: SendReport,
    /// Brightness: how many steps below full (0 = full, so a new console,
    /// all zero bytes, is at full).
    dim: u8,
    /// The backlight value last sent to the board (0 = none sent yet).
    light: u8,
    /// The player changed a setting since the board last asked.
    settings_changed: bool,
    /// The console menu on the home screen is open, and its chosen item.
    menu: bool,
    menu_sel: u8,
    /// The last step drew and sent nothing: home is being read again
    /// after a game (see [`Console::home_pending`]).
    home_pending: bool,
}

impl Console {
    pub const fn new() -> Self {
        Console {
            video: Video::new(),
            input: InputState::new(),
            boot: boot::Boot::new(),
            home: home::Home::new(),
            notices: notify::Notices::new(),
            run: game::Run::new(),
            page: Page::Boot,
            start_us: 0,
            pads: 0,
            frames: 0,
            next_us: 0,
            wake: Wake::At(0),
            last_report: SendReport {
                areas: 0,
                pixels: 0,
                send_us: 0,
                wait_us: 0,
            },
            dim: 0,
            light: 0,
            settings_changed: false,
            menu: false,
            menu_sel: 0,
            home_pending: false,
        }
    }

    /// One frame of work. Never waits. Returns when to be called again.
    /// Calling early or late is safe (see `board_api::Wake`).
    pub fn step<B: Board>(&mut self, board: &mut B) -> Wake {
        self.step_via(board, out::Direct)
    }

    /// The same as [`Console::step`], but the picture goes to `sender`
    /// (for example the board's second core) instead of being sent by the
    /// screen right away. Feature `swap`; the rule is in
    /// `crates/console-video/README.md`, "Swap: two cores, two pictures".
    ///
    /// Before the console lights or dims the backlight, it waits for the
    /// sender ([`console_video::Sender::wait_free`]), so the screen is
    /// never lit before the first picture is really on it, and the
    /// backlight is never touched while the sender uses the screen.
    ///
    /// The board must not move the console while a picture is handed
    /// over (keep it in a `static`), and must call `wait_free` itself
    /// before it uses the screen or the console's pictures in any other
    /// way that the sender could disturb (see the README).
    #[cfg(feature = "swap")]
    pub fn step_with_sender<B: Board, O: console_video::Sender + ?Sized>(
        &mut self,
        board: &mut B,
        sender: &mut O,
    ) -> Wake {
        self.step_via(board, out::Handing(sender))
    }

    /// The body of `step` and `step_with_sender` (one copy per way out).
    /// `out` is taken by value: `Direct` has no size, so without `swap`
    /// the call is the same as before this split.
    fn step_via<B: Board, O: out::Out<B::Screen>>(&mut self, board: &mut B, mut out: O) -> Wake {
        // The parts, and the RAM the board lends while no game is loaded
        // (home's game list and tile pictures are kept there, see `home`).
        let (mut p, mut lent) = board.parts_and_lent();
        let now = p.clock.now_us();
        // Early, and not woken by input: nothing is due. The input is not
        // read, so no press is lost; the due step will see it.
        if now < self.next_us && !self.wake.on_input() {
            return self.wake;
        }
        let view = View::fit(B::Screen::PANEL_W, B::Screen::PANEL_H);
        // Every pad, one per player. Pad 0 also drives the menus.
        let mut pads = [board_api::PadState::NONE; PLAYERS];
        let mut pads_noted = [0u32; PLAYERS];
        let mut connected = 0u8;
        for (n, (pad, noted)) in pads.iter_mut().zip(&mut pads_noted).enumerate() {
            *pad = p.pad.read(n as u8);
            // Presses the board noted since the last frame: they count
            // even if already over (see `board_api::input`). Taken from
            // every pad in every step, so none piles up.
            let taken = p.pad.take_pressed(n as u8);
            if pad.connected {
                *noted = taken;
                connected |= 1 << n;
            }
        }
        let pad = pads[0];
        let keys = p.keys.held();
        let touch = p.touch.read();
        let noted = Noted {
            buttons: pads_noted[0],
            keys: p.keys.take_pressed(),
            tap: p.touch.take_tap(),
        };
        if self.frames == 0 {
            p.log.line("console: started");
            gfx::set_console_colours(self.video.palette_mut());
            self.video.set_border(theme::rgb(theme::UI_COLOURS[0]));
            self.start_us = now;
            // A pad already there at start is no news.
            self.pads = connected;
        }
        let input: Input = self.input.update(now, touch, keys, &pad, &noted, &view);
        let boot_held = keys & key::BOOT != 0;
        let boot_noted = noted.keys & key::BOOT != 0;

        for n in 0..PLAYERS {
            let now_on = connected & (1 << n) != 0;
            if now_on != (self.pads & (1 << n) != 0) {
                let (text, tone) = if now_on {
                    (msg::PADS_ON[n], Tone::Good)
                } else {
                    (msg::PADS_OFF[n], Tone::Warn)
                };
                self.notices.push(text, tone);
            }
        }
        self.pads = connected;

        // Update the page on show.
        let page_was = self.page;
        let mut launch = None;
        match self.page {
            Page::Boot => {
                if self.boot.update(now, &input) {
                    #[cfg(not(feature = "boot-gather"))]
                    {
                        let pal = self.video.palette_mut();
                        self.home.scan(p.files, lent.as_deref_mut(), now);
                        self.home.set_colours(pal, lent.as_deref());
                        self.home.load_art(p.files, pal, p.log, lent.as_deref_mut());
                    }
                    // The animation's colours go (home's palette is then
                    // exactly as without the feature), and home's come.
                    // None of the changed colours is on the animation's
                    // last picture (home's background, stars, bar and
                    // title), so when the boot ran to its end the panel
                    // gets only what home adds, not the whole screen.
                    #[cfg(feature = "boot-gather")]
                    {
                        let home = &mut self.home;
                        self.video.recolour_unseen(|pal| {
                            pal.set_all(&gather::PLAIN);
                            home.scan(p.files, lent.as_deref_mut(), now);
                            home.set_colours(pal, lent.as_deref());
                            home.load_art(p.files, pal, p.log, lent.as_deref_mut());
                        });
                    }
                    self.page = Page::Home;
                    p.log.line("console: home");
                }
            }
            Page::Home => {
                // Back from a game (or from a game that could not start):
                // the list went with the lent RAM, so it is read from the
                // card again, in this step; the pictures in the next one,
                // and home is drawn only then (see "Back home" below).
                let reread = !self.home.valid(lent.as_deref());
                let change = if reread {
                    let change = self.home.rescan(p.files, lent.as_deref_mut(), now);
                    self.home
                        .set_colours(self.video.palette_mut(), lent.as_deref());
                    change
                } else {
                    let change = self.home.check(p.files, lent.as_deref_mut(), now);
                    if let Some(CardChange::Ready(_)) = change {
                        self.home
                            .set_colours(self.video.palette_mut(), lent.as_deref());
                    }
                    change
                };
                match change {
                    Some(CardChange::Removed) => self.notices.push(msg::CARD_OUT, Tone::Warn),
                    Some(CardChange::Ready(_)) => self.notices.push(msg::CARD_IN, Tone::Good),
                    None => true,
                };
                let home_input = self.home_menu(&input, p.log);
                if let HomeAction::Launch(i) = self.home.update(&home_input) {
                    launch = self.home.game(lent.as_deref(), i).map(|g| g.folder);
                }
                // The tile pictures, only once the row is still (and not
                // in the step that read the list again).
                if launch.is_none() && !reread {
                    let pal = self.video.palette_mut();
                    self.home.load_art(p.files, pal, p.log, lent.as_deref_mut());
                    self.home.settle_fresh();
                }
            }
            Page::Game => {}
        }

        // Starting a game: the board finds it and hands back its table.
        // The lent RAM goes to the game (whether it starts or not), so
        // home lets go of its list first. The parts are lent again
        // afterwards.
        let mut ended = false;
        let mut launched = false;
        if let Some(folder) = launch {
            self.home.lose_list();
            let started = board.start_game(folder.as_str());
            (p, lent) = board.parts_and_lent();
            match started {
                Ok(cb) => {
                    let line = gfx::Text::<32>::joined("console: game ", folder.as_str());
                    p.log.line(line.as_str());
                    let raw = Self::game_raw(p.pad, &pads, &pads_noted, &noted, touch, &view);
                    self.run
                        .begin(cb, folder.as_str(), now, &raw, boot_held, &mut self.video);
                    self.page = Page::Game;
                    launched = true;
                }
                Err(e) => {
                    let (text, line) = start_error_text(e);
                    self.notices.push(text, Tone::Warn);
                    p.log.line(line);
                }
            }
        }

        // Notifications wait during the boot screen.
        if page_was != Page::Boot {
            self.notices.update(now);
        }

        // The game's frame.
        if self.page == Page::Game {
            let mut raw = Self::game_raw(p.pad, &pads, &pads_noted, &noted, touch, &view);
            // The presses noted in the frame that started the game
            // started it: they are not the game's.
            if launched {
                raw = raw.without_noted();
            }
            let boot_noted = boot_noted && !launched;
            let mut step = self.brightness();
            let was_paused = self.run.paused();
            let how = self.run.step(
                now,
                &input,
                boot_held,
                boot_noted,
                &raw,
                &mut step,
                &mut self.video,
                p.log,
            );
            self.player_set_brightness(step, p.log);
            // A finger already down on the game when its menu opens is
            // the game's: it does not slide the bars or tap the menu.
            if !was_paused && self.run.paused() {
                self.input.cancel_touch();
            }
            if let Some(how) = how {
                self.game_ended(how, p.log);
                ended = true;
            }
        }

        // A finger put down on one page does not tap or drag on the next
        // (for example the touch that skipped the boot screen).
        if self.page != page_was {
            self.input.cancel_touch();
        }

        // Back home: while home has no list (the step a game ended or
        // could not start) or its pictures are not read yet (the step that
        // read the list again), nothing is drawn or sent; the screen keeps
        // the last picture for those two frames. So the screen never shows
        // home with no games, or with letters where pictures will be.
        let back_home =
            self.page == Page::Home && (!self.home.valid(lent.as_deref()) || self.home.fresh());

        // The boot animation's colours, set while it shows (they change
        // the palette only once: `set_all` keeps the version when no
        // colour changes).
        #[cfg(feature = "boot-gather")]
        if self.page == Page::Boot {
            self.video.palette_mut().set_all(&gather::PALETTE);
        }

        // Draw the whole picture (a game draws its own). The renderer
        // finds what changed, so this costs only CPU work, not the wire.
        let frame = self.video.frame_mut();
        match self.page {
            _ if back_home => {}
            Page::Boot => self.boot.draw(now, frame),
            Page::Home => {
                self.home.draw(frame, lent.as_deref());
                let status = bar::Status {
                    uptime_us: now - self.start_us,
                    pad_connected: self.pads & 1 != 0,
                    battery: None,
                    hints: if self.home.count() > 0 {
                        bar::Hints::Games
                    } else {
                        bar::Hints::None
                    },
                    menu: self.menu,
                };
                bar::draw(&status, frame);
                if self.menu {
                    let step = BRIGHT_STEPS - 1 - self.dim;
                    #[cfg(not(feature = "boot-gather"))]
                    pause::draw(frame, pause::Kind::Home, self.menu_sel, step);
                    #[cfg(feature = "boot-gather")]
                    pause::draw_home(frame, self.menu_sel, step, self.boot.anim());
                }
                self.notices.draw(now, frame);
            }
            Page::Game => {
                // Over the game's picture, and taken off again once sent.
                let step = BRIGHT_STEPS - 1 - self.dim;
                self.run.cover(frame, !self.notices.is_empty(), step);
                self.notices.draw(now, frame);
            }
        }

        self.home_pending = back_home;
        self.last_report = if back_home {
            SendReport::default()
        } else {
            out.present(&mut self.video, p.screen)
        };
        // The backlight: first after the first picture was sent (never
        // lit before there is one), then once per change.
        let want = theme::BACKLIGHT[usize::from(self.brightness())];
        if want != self.light {
            // With `swap`, the picture handed over must be on the panel
            // first (nothing to wait for without it).
            out.settle();
            p.screen.backlight(want);
            self.light = want;
        }
        if self.page == Page::Game {
            self.run.put_back(self.video.frame_mut());
        }
        if ended {
            board.end_game();
        }
        self.frames += 1;
        self.wake = self.next_wake(now, back_home);
        self.wake
    }

    /// The pads and touch as a game sees them.
    fn game_raw<P: Pad>(
        board_pads: &mut P,
        pads: &[board_api::PadState; PLAYERS],
        pads_noted: &[u32; PLAYERS],
        noted: &Noted,
        touch: Option<board_api::TouchPoint>,
        view: &View,
    ) -> gamein::Raw {
        let mut motion = [None; PLAYERS];
        for (n, (m, pad)) in motion.iter_mut().zip(pads).enumerate() {
            if pad.connected {
                *m = board_pads.motion(n as u8);
            }
        }
        let at = touch.and_then(|t| view.to_picture(t.x, t.y));
        let tap = noted.tap.and_then(|t| view.to_picture(t.x, t.y));
        gamein::read(pads, pads_noted, &motion, at, tap)
    }

    /// Back home after a game, with a word on how it ended.
    fn game_ended<L: LogOut>(&mut self, how: Ended, log: &mut L) {
        match how {
            Ended::Done => {
                self.notices.push(msg::GAME_ENDED, Tone::Info);
                log.line("console: game ended");
            }
            Ended::Aborted(_) => {
                self.notices.push(msg::GAME_ABORTED, Tone::Warn);
                log.line("console: game stopped with an error");
            }
            Ended::Quit => {}
        }
        self.run.clear();
        // The game's colours go. The tile colours come back with the
        // list, which home reads again at the next step (the game had
        // the lent RAM).
        self.home.reset_colours(self.video.palette_mut());
        self.page = Page::Home;
        log.line("console: home");
    }

    /// When to be called next: at the next frame while something moves,
    /// else on input or when the next change is due.
    /// `back_home`: home is read again after a game (two steps, at the
    /// frame rate).
    fn next_wake(&mut self, now: u64, back_home: bool) -> Wake {
        // A running game at a fixed rate sets the period.
        let period = match self.page {
            Page::Game if !self.run.paused() => self.run.period_us().unwrap_or(FRAME_US),
            #[cfg(feature = "boot-gather")]
            Page::Boot => theme::BOOT_FRAME_US,
            _ => FRAME_US,
        };
        // A fixed rate: the next frame one period after this one was due.
        // After a pause (or at start), start again from now. A step woken
        // early by input (before `next_us`) keeps the frame already due:
        // adding a period there would push the next frame further away
        // with every input wake (a noisy stick could push it seconds
        // ahead, and the screen would freeze).
        if now >= self.next_us {
            self.next_us += period;
            if self.next_us <= now {
                self.next_us = now + period;
            }
        }
        let moving = match self.page {
            Page::Boot => self.boot.moving(now),
            // A finger on the screen that moves (or just went down) is
            // looked at every frame, so a drag is drawn smoothly and a
            // lift is seen at once. A resting finger lets the console
            // sleep again (see `input::Gesture::live`).
            Page::Home => {
                back_home
                    || self.home.moving()
                    || self.home.art_pending()
                    || self.home.check_waiting()
                    || self.notices.moving(now)
                    || self.input.live(now)
            }
            // The menu over a game follows a finger on its bars.
            Page::Game => {
                let paused = self.run.paused();
                let fixed = !paused && self.run.period_us().is_some();
                fixed || self.notices.moving(now) || (paused && self.input.live(now))
            }
        };
        if moving {
            return Wake::At(self.next_us);
        }
        let mut due = match self.page {
            Page::Boot => self.boot.end_us(),
            Page::Home => bar::next_change(self.start_us, now).min(self.home.next_check_us()),
            // An on-demand game: its timer, if it set one. A paused game:
            // only input.
            Page::Game => self.run.timer_due().unwrap_or(u64::MAX),
        };
        if let Some(t) = self.notices.next_change(now) {
            due = due.min(t);
        }
        // Key repeat moves the menus; a running game does not use it.
        if (self.page != Page::Game || self.run.paused())
            && let Some(t) = self.input.repeat_due()
        {
            due = due.min(t);
        }
        // Never sooner than the next frame.
        Wake::InputOr(due.max(self.next_us))
    }

    /// Frames done so far.
    pub fn frames(&self) -> u32 {
        self.frames
    }

    /// The page on show.
    pub fn page(&self) -> Page {
        self.page
    }

    /// The renderer: the picture and the palette (for screenshots).
    pub fn video(&self) -> &Video {
        &self.video
    }

    /// The renderer, to draw into and present by hand (feature `swap`).
    /// For a board's own measurements only: the CYD lab's full-screen
    /// bench (`experiments/cyd-lab`, letter `X`) scrolls a picture in it
    /// and hands it over with [`Video::present_handing`], so the bench
    /// uses the same two pictures and the same rule as the console. The
    /// console draws its whole page again at its next step; call
    /// [`Video::force_full`] when done so the panel gets all of it.
    #[cfg(feature = "swap")]
    pub fn video_mut(&mut self) -> &mut Video {
        &mut self.video
    }

    /// The home screen (its selection, count, scroll), for tests and
    /// tools. The list itself is in the board's lent RAM: see
    /// [`Console::home_games`].
    pub fn home(&self) -> &home::Home {
        &self.home
    }

    /// A copy of home's game list, read from `board`'s lent RAM (empty
    /// while home has no list: while a game runs, and in the step after
    /// one ended). For tests and tools.
    pub fn home_games<B: Board>(&self, board: &mut B) -> home::GameList {
        let (_, lent) = board.parts_and_lent();
        self.home.list(lent.as_deref())
    }

    /// The running game (or none): its folder, rate, pause menu.
    pub fn game(&self) -> &game::Run {
        &self.run
    }

    /// The notification queue.
    pub fn notices(&self) -> &notify::Notices {
        &self.notices
    }

    /// The brightness step: 0 (dimmest) to `theme::BRIGHT_STEPS - 1`
    /// (full, the default).
    pub fn brightness(&self) -> u8 {
        (BRIGHT_STEPS - 1).saturating_sub(self.dim)
    }

    /// Sets the brightness step (a bigger one is taken as full). For a
    /// board at start, with the step it saved; this is not a change by
    /// the player, so [`Console::take_settings_changed`] does not report
    /// it. The board's backlight follows at the next step (as with
    /// [`Console::notify`], call [`Console::step`] soon).
    pub fn set_brightness(&mut self, step: u8) {
        self.dim = BRIGHT_STEPS - 1 - step.min(BRIGHT_STEPS - 1);
    }

    /// The backlight value last sent to the board (0 before the first,
    /// which comes after the first picture).
    pub fn backlight(&self) -> u8 {
        self.light
    }

    /// True once after the player changed a setting (the brightness, and
    /// with `boot-gather` the boot animation), then false until the next
    /// change. A board that keeps
    /// settings saves them then. The console itself writes nothing.
    pub fn take_settings_changed(&mut self) -> bool {
        core::mem::take(&mut self.settings_changed)
    }

    /// Whether the console menu is open on the home screen (over a game,
    /// see [`game::Run::paused`]).
    pub fn menu_open(&self) -> bool {
        self.menu
    }

    /// The chosen item of the home screen's menu
    /// ([`pause::HOME_ITEMS`]).
    pub fn menu_item(&self) -> u8 {
        self.menu_sel
    }

    /// A change of step by the player (from a menu).
    fn player_set_brightness<L: LogOut>(&mut self, step: u8, log: &mut L) {
        if step != self.brightness() {
            self.set_brightness(step);
            self.settings_changed = true;
            let line = gfx::Text::<24>::joined("console: brightness ", digit(self.brightness()));
            log.line(line.as_str());
        }
    }

    /// How the boot animation plays (feature `boot-gather`): FULL, SHORT
    /// or OFF (see [`boot::Anim`]).
    #[cfg(feature = "boot-gather")]
    pub fn boot_anim(&self) -> boot::Anim {
        self.boot.anim()
    }

    /// Sets how the boot animation plays (feature `boot-gather`). For a
    /// board at start, with the value it saved, before the first step
    /// (later it counts for the next start only). Like
    /// [`Console::set_brightness`], this is not a change by the player.
    #[cfg(feature = "boot-gather")]
    pub fn set_boot_anim(&mut self, anim: boot::Anim) {
        self.boot.set_anim(anim);
    }

    /// A change of the boot animation by the player (from the menu). It
    /// counts from the next start.
    #[cfg(feature = "boot-gather")]
    fn player_set_boot_anim<L: LogOut>(&mut self, anim: boot::Anim, log: &mut L) {
        if anim != self.boot.anim() {
            self.boot.set_anim(anim);
            self.settings_changed = true;
            let line = gfx::Text::<32>::joined("console: boot anim ", anim.word());
            log.line(line.as_str());
        }
    }

    /// The home screen's menu: opens it (Home, or a tap on the gear),
    /// runs it while open, closes it. Returns the input left for the home
    /// screen: none while the menu is open, so the row keeps still and
    /// nothing starts.
    fn home_menu<L: LogOut>(&mut self, input: &Input, log: &mut L) -> Input {
        let quiet = Input {
            pad: input.pad,
            ..Input::default()
        };
        if self.menu {
            let mut step = self.brightness();
            let choice = pause::update(pause::Kind::Home, &mut self.menu_sel, &mut step, input);
            self.player_set_brightness(step, log);
            #[cfg(feature = "boot-gather")]
            {
                let anim = self.boot.anim();
                let new = pause::update_boot(pause::Kind::Home, self.menu_sel, anim, input);
                self.player_set_boot_anim(new, log);
            }
            let gear = input.down.is_some_and(|(x, y)| on_gear(x, y));
            if choice != pause::Choice::Stay || gear {
                self.menu = false;
                // The finger that closed it does not tap the row when it
                // lifts.
                self.input.cancel_touch();
                log.line("console: menu closed");
            }
            return quiet;
        }
        let gear = input.tap.is_some_and(|(x, y)| on_gear(x, y));
        if input.acts & act::HOME != 0 || gear {
            self.menu = true;
            self.menu_sel = 0;
            self.input.cancel_touch();
            log.line("console: menu");
            return quiet;
        }
        *input
    }

    /// What the board reported for the last frame (all zeros when nothing
    /// changed, so nothing was sent).
    pub fn last_report(&self) -> SendReport {
        self.last_report
    }

    /// True when the last step was on the home screen but drew and sent
    /// nothing, because home was being read again from the card after a
    /// game (two steps: the one the game ended or could not start in,
    /// and the one that read the list). The panel still shows the
    /// picture sent before.
    pub fn home_pending(&self) -> bool {
        self.home_pending
    }

    /// Shows a short message for a few seconds (see [`notify`]). It waits
    /// in a queue behind earlier ones; `now_us` is not needed for that,
    /// and kept for the board's convenience. Returns false when the queue
    /// is full.
    ///
    /// The [`Wake`] the last step gave does not know about the message:
    /// call [`Console::step`] again soon (for example at the next frame),
    /// or it shows only at the next input or timed change.
    pub fn notify(&mut self, text: &str, now_us: u64) -> bool {
        let _ = now_us;
        self.notices.push(text, Tone::Info)
    }
}

/// Whether a picture point is on the home screen's gear (its tap area).
pub fn on_gear(x: u16, y: u16) -> bool {
    x as i16 >= theme::GEAR_HIT_X && y as i16 >= theme::GEAR_HIT_Y
}

/// One digit as text (for the log).
fn digit(n: u8) -> &'static str {
    const D: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    D[usize::from(n.min(9))]
}

/// The notice and the log line for a game that could not start.
fn start_error_text(e: StartError) -> (&'static str, &'static str) {
    match e {
        StartError::NotSupported => (msg::NO_START, "home: games cannot start yet"),
        StartError::NotFound => (msg::GAME_NOT_FOUND, "home: game not found"),
        StartError::BadFile | StartError::BadTable(OpenError::TooSmall) => {
            (msg::GAME_DAMAGED, "home: game file is damaged")
        }
        StartError::BadTable(OpenError::TooOld) => (msg::GAME_TOO_OLD, "home: game is too old"),
        StartError::BadTable(OpenError::NoTable) => {
            (msg::GAME_REFUSED, "home: game refused this console")
        }
        StartError::Refused(notice) => (notice, "home: the board refused the game file"),
    }
}

impl Default for Console {
    fn default() -> Self {
        Self::new()
    }
}
