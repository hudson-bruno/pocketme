//! The board's loop: runs `Console::step` per the board-api contract,
//! serves the debug link, drains the log, measures.
//!
//! The contract: call `step`; it returns a `Wake`. Sleep until its
//! deadline, or (for `Wake::InputOr`) until input comes. Here "sleep" is
//! an `await` on the timer, the serial port, and the BOOT and touch pins
//! (their falling edges), whichever comes first. When the loop goes round
//! without that sleep (the console is already behind, for example), it
//! reads the bytes the port already holds without waiting
//! ([`App::serial_without_wait`]), so the debug link still answers.
//!
//! Input is looked at in one place, before each step: BOOT's level and
//! edge latch ([`crate::board::CydKeys::poll`]) and the touch panel
//! ([`crate::board::CydTouch::sample`]). While BOOT or the panel is held,
//! the loop also wakes every [`TOUCH_SAMPLE_US`] to look again; such a
//! wake runs the console's step only if the console would see something
//! new (or its own deadline came). So a finger resting on the panel
//! costs a chip read now and then, not a console step at frame rate.
//!
//! The whole loop is ONE embassy task ([`run`]). A `present` does not
//! yield: it waits for the panel's refresh by spinning and sends by DMA
//! while the CPU fills the next strip, for up to about 35 ms (one full
//! frame: see the README).
//!
//! The gamepad radio (feature `pad`) is served by this loop too: once per
//! turn, just before the input is looked at, [`crate::pad_radio::PadRadio::poll`]
//! reads every packet the controller queued meanwhile (the driver queues
//! them on the heap from the controller's own task, so none is lost while
//! a frame is drawn) and sends what is due; and the loop sleeps on the
//! radio beside its other wake-ups. So a pad report is read before the
//! very next step, at most about one frame after it came, without a task
//! or an interrupt that could cut into the panel's timed windows. The
//! measurements: README, "The gamepad radio (pad)".

use console_core::{Console, Page};
#[cfg(feature = "touch")]
use embassy_futures::select::{Either, select};
use embassy_futures::select::{Either3, select3};
use embassy_time::{Instant, Timer};
use esp_hal::{Async, uart::Uart};
use log::info;

use crate::board::{BootEvent, CydBoard, now_us};
use crate::stats::{Scene, Stats};

/// How often the log is pushed out while text waits (the port's 128-byte
/// queue lasts 11 ms at 115,200 baud).
const LOG_POLL_US: u64 = 5_000;
/// How often a measurement summary goes to the log.
const REPORT_US: u64 = 30_000_000;
/// How often BOOT and the panel are looked at while held (about two
/// looks per console frame).
pub const TOUCH_SAMPLE_US: u64 = 16_000;
/// While a touch goes on: how often its counters go to the log.
const TOUCH_LOG_US: u64 = 5_000_000;
/// The measuring letter `k`: how long BOOT is pulled low (well under
/// one console frame).
const PULSE_US: u32 = 5_000;

/// One way back home after a game, while it is measured.
#[derive(Clone, Copy)]
struct Reload {
    calls: u32,
    card_us: u64,
    steps: u32,
    began_us: u64,
}

pub struct App {
    pub console: &'static mut Console,
    pub board: CydBoard,
    pub uart: Uart<'static, Async>,
    pub stats: Stats,
    #[cfg(feature = "link")]
    pub link: crate::link::LinkState,
    #[cfg(feature = "tour")]
    pub tour: crate::tour::Tour,
    next_report: u64,
    /// When the last button press came in (link, BOOT): the start of a
    /// game's "press to first frame" time.
    pub last_press_us: u64,
    /// Set when a game starts: its press time, until its first frame is
    /// on the panel.
    game_wait: Option<u64>,
    /// While home reads its list and pictures again after a game (they
    /// were in the game data slot, which the game overwrote): card calls,
    /// card microseconds, steps, and when it began.
    reload: Option<Reload>,
    /// The console's own next deadline, and whether it wants to be woken
    /// by input before it (from its last `Wake`).
    console_due: u64,
    console_on_input: bool,
    /// Steps left out: the loop woke only to look at a held BOOT or
    /// panel, and nothing the console would see had changed.
    pub skipped: u32,
    /// A text command being typed (`T...;`), and its length.
    cmd: [u8; 16],
    cmd_len: Option<usize>,
    next_touch_log: u64,
    /// The settings kept in flash (set by main after the load).
    #[cfg(feature = "store")]
    pub saver: Option<crate::flash_store::Saver>,
    /// The gamepad radio (set by main after it started).
    #[cfg(feature = "pad")]
    pub radio: Option<crate::pad_radio::PadRadio>,
}

impl App {
    pub fn new(console: &'static mut Console, board: CydBoard, uart: Uart<'static, Async>) -> Self {
        Self {
            console,
            board,
            uart,
            stats: Stats::default(),
            #[cfg(feature = "link")]
            link: crate::link::LinkState::new(),
            #[cfg(feature = "tour")]
            tour: crate::tour::Tour::new(),
            next_report: REPORT_US,
            last_press_us: 0,
            game_wait: None,
            reload: None,
            console_due: 0,
            console_on_input: true,
            skipped: 0,
            cmd: [0; 16],
            cmd_len: None,
            next_touch_log: 0,
            #[cfg(feature = "store")]
            saver: None,
            #[cfg(feature = "pad")]
            radio: None,
        }
    }

    /// Whether a game runs and is not paused (its frames at full rate).
    #[cfg_attr(not(feature = "store"), allow(dead_code))]
    fn game_running(&self) -> bool {
        self.console.page() == Page::Game && !self.console.game().paused()
    }

    /// After the loop looked (and maybe stepped): the settings saved if
    /// they changed and the rules say now (settings_store::policy).
    #[cfg(feature = "store")]
    fn keep_settings(&mut self) {
        let Some(saver) = &mut self.saver else {
            return;
        };
        let m = settings_store::policy::Moment {
            changed: self.console.take_settings_changed(),
            menu: self.console.menu_open() || self.console.game().paused(),
            game_running: self.console.page() == Page::Game && !self.console.game().paused(),
        };
        let new = saver
            .store
            .current()
            .with_brightness(self.console.brightness());
        // The boot animation beside it (feature `boot-gather`; without it
        // the stored value is left as it is).
        #[cfg(feature = "boot-gather")]
        let new = new.with_boot_anim(self.console.boot_anim().byte());
        saver.after_step(&mut self.board.store, now_us(), m, new);
    }

    /// Measuring only (letter `w`): saves through the settings store,
    /// brightness going 0, 1, 0, 1, ..., until a sector had to be erased
    /// (at most 600 saves), to time a record write and an erase; then the
    /// console's own brightness is saved back. Wears the flash a little
    /// (one erase in 100,000).
    #[cfg(feature = "store")]
    fn wear_test(&mut self) {
        let Some(saver) = &mut self.saver else {
            return;
        };
        let store = &mut self.board.store;
        let t0 = now_us();
        let (mut n, mut erased_at, mut fails) = (0u32, None, 0u32);
        let mut save = crate::stats::Sum::default();
        while n < 600 && erased_at.is_none() {
            let new = saver.store.current().with_brightness((n % 2) as u8);
            let t = now_us();
            match saver.store.save(store, &new) {
                Ok(settings_store::Saved::Written { erased }) => {
                    save.add(now_us() - t);
                    if erased {
                        erased_at = Some(n);
                    }
                }
                Ok(settings_store::Saved::Unchanged) => {}
                Err(_) => fails += 1,
            }
            n += 1;
        }
        let back = saver
            .store
            .current()
            .with_brightness(self.console.brightness());
        let _ = saver.store.save(store, &back);
        let (w, e) = store
            .flash()
            .map_or(Default::default(), |f| (f.writes, f.erases));
        info!(
            "settings wear test: {n} saves in {} ms, fails={fails}, erase at save {:?}; save_us avg={} max={}; flash write_us avg={} max={} (n={}); erase_us avg={} max={} (n={}); record now {}",
            (now_us() - t0) / 1000,
            erased_at,
            save.avg(),
            save.max,
            w.avg(),
            w.max,
            w.n,
            e.avg(),
            e.max,
            e.n,
            saver.store.used(),
        );
    }

    /// One console step, measured.
    fn step(&mut self) -> console_core::board_api::Wake {
        let frames = self.console.frames();
        let selected = self.console.home().selected();
        let page = self.console.page();
        #[cfg(not(feature = "core2"))]
        let presents = self.board.screen.counters.presents;
        #[cfg(not(feature = "core2"))]
        let present_us = self.board.screen.counters.total_us;
        let (calls, files_us) = (self.board.files.calls, self.board.files.us);
        let pending = self.console.home_pending();

        self.stats.wakes += 1;
        let t0 = now_us();
        #[cfg(not(feature = "core2"))]
        let wake = self.console.step(&mut self.board);
        // With `core2` the picture goes to the second core (no copy); the
        // step returns while it is sent.
        // A send that failed on the second core may have left the panel
        // out of step with the picture: the whole screen goes next time.
        #[cfg(feature = "core2")]
        if crate::core2::take_resend() {
            self.console.video_mut().force_full();
        }
        #[cfg(feature = "core2")]
        let mut out = crate::core2::SwapSender::default();
        #[cfg(feature = "core2")]
        let wake = self.console.step_with_sender(&mut self.board, &mut out);
        let t1 = now_us();
        #[cfg(feature = "core2")]
        if let Some(t) = out.handed_at {
            crate::core2::note_after(crate::lcd::cycles().wrapping_sub(t));
        }

        if page != Page::Game && self.console.page() == Page::Game {
            self.game_wait = Some(self.last_press_us.min(t0));
        }
        self.note_reload(pending, t0, t1, calls, files_us);
        if self.console.page() != Page::Game {
            self.game_wait = None;
        }

        if self.console.frames() == frames {
            self.stats.early += 1;
            return wake;
        }
        let home = self.console.home();
        let scene = if self.console.page() == Page::Boot {
            Scene::Boot
        } else if self.console.page() == Page::Game {
            if self.console.menu_open() {
                Scene::Menu
            } else {
                Scene::Game
            }
        } else if self.board.touch.down() {
            Scene::Drag
        } else if home.moving() {
            Scene::Scroll
        } else if self.console.notices().moving(t1) {
            Scene::Notice
        } else if home.selected() != selected {
            Scene::Select
        } else {
            Scene::Idle
        };
        #[cfg(not(feature = "core2"))]
        let c = &self.board.screen.counters;
        #[cfg(not(feature = "core2"))]
        let present = (c.presents != presents).then(|| c.total_us - present_us);
        #[cfg(feature = "xip-probe")] // measuring only: probe code after the hand-over
        crate::xip_probe::frame(out.handed_at.is_some());
        // With `core2`: the first core's part of the present (wait, hand-over); sends: `core2` line.
        #[cfg(feature = "core2")]
        let present = out.handed_at.map(|_| {
            let r = self.console.last_report();
            u64::from(r.wait_us) + u64::from(r.send_us)
        });
        let pixels = self.console.last_report().pixels;
        self.stats.frame(scene, t0, t1 - t0, present, pixels);
        if present.is_some()
            && let Some(pressed) = self.game_wait.take()
        {
            let us = t1 - pressed;
            self.stats.game_first_frame.add(us);
            let g = &self.board.games.last;
            info!(
                "game first frame: {us} us after the press (load {} us, open {} us, step {} us)",
                g.load_us,
                g.open_us,
                t1 - t0
            );
        }

        if self.board.files.calls != calls && !pending {
            let us = self.board.files.us - files_us;
            if page == Page::Boot && self.console.page() == Page::Home {
                self.stats.scan.add(us);
                info!(
                    "card scan_us={us} calls={} games={}",
                    self.board.files.calls - calls,
                    home.count()
                );
            } else if home.count() > 0 {
                self.stats.check_games.add(us);
            } else {
                self.stats.check_empty.add(us);
            }
        }
        wake
    }

    /// Home's way back after a game: the steps in which home is pending
    /// (nothing drawn or sent, the panel keeps the last picture) and the
    /// one that draws home again. One log line when home is shown.
    fn note_reload(&mut self, pending: bool, t0: u64, t1: u64, calls: u32, files_us: u64) {
        let now_pending = self.console.home_pending();
        if !pending && !now_pending {
            return;
        }
        let r = self.reload.get_or_insert(Reload {
            calls: 0,
            card_us: 0,
            steps: 0,
            began_us: t0,
        });
        r.calls += self.board.files.calls - calls;
        r.card_us += self.board.files.us - files_us;
        r.steps += 1;
        if !now_pending {
            let r = *r;
            self.reload = None;
            let us = t1 - r.began_us;
            self.stats.reload.add(us);
            info!(
                "home reload: {} steps, {} card calls in {} us, home drawn {} us after the game ended (list and pictures read again from the card: they were in the game data slot)",
                r.steps, r.calls, r.card_us, us
            );
        }
    }

    /// Writes the measurements to the log.
    pub fn report(&mut self) {
        let mut text = [0u8; 1024];
        let mut out = debug_link::TextBuf::new(&mut text);
        self.stats.write(&mut out);
        let lines = core::str::from_utf8(out.as_bytes()).unwrap_or("");
        for line in lines.lines() {
            info!("{line}");
        }
        #[cfg(not(feature = "core2"))]
        let c = &self.board.screen.counters;
        #[cfg(not(feature = "core2"))]
        let m = self.board.screen.beam.misses;
        #[cfg(feature = "core2")]
        let snap = crate::core2::snapshot();
        #[cfg(feature = "core2")]
        let (c, m) = (&snap.counters, snap.misses);
        info!(
            "screen presents={} windows={} untimed={} no_anchor={} anchor_miss={}/{}/{}/{} no_plan={} late={} crossed={} errors={} bytes={} log_dropped={}",
            c.presents,
            c.windows,
            c.untimed,
            c.no_anchor,
            m[0],
            m[1],
            m[2],
            m[3],
            c.no_plan,
            c.late,
            c.crossed,
            c.errors,
            c.bytes,
            crate::logbuf::dropped()
        );
        // The same buffer again (no second one on the stack).
        let mut out = debug_link::TextBuf::new(&mut text);
        self.write_send(&mut out, false);
        info!(
            "{}",
            core::str::from_utf8(out.as_bytes())
                .unwrap_or("")
                .trim_end()
        );
        self.log_input();
        #[cfg(feature = "store")]
        if let Some(s) = &self.saver {
            let mut text = [0u8; 160];
            let mut out = debug_link::TextBuf::new(&mut text);
            s.write_short(&self.board.store, &mut out);
            info!(
                "{}",
                core::str::from_utf8(out.as_bytes())
                    .unwrap_or("")
                    .trim_end()
            );
        }
        #[cfg(feature = "pad")]
        if let Some(r) = &mut self.radio {
            r.log_line(&self.board.pad, true);
        }
        #[cfg(feature = "core2")]
        crate::core2::report();
        crate::mem::report("report");
    }

    /// The input counters, to the log.
    pub fn log_input(&self) {
        let mut text = [0u8; 512];
        let mut out = debug_link::TextBuf::new(&mut text);
        self.write_input(&mut out);
        let text = core::str::from_utf8(out.as_bytes()).unwrap_or("");
        for line in text.lines() {
            info!("{line}");
        }
    }

    /// How pictures were sent, as one line (the log's report): the plan
    /// in use (`new`, or `old` = today's, by the letters `o`/`p` of
    /// feature `send-ab`), the timing (`off` = fast, may tear: letter
    /// `u`), presents, windows per present (x100), presents sent as their
    /// own windows in one pass / as one box in one pass (new plan only) /
    /// with no one-pass plan, late and crossed windows, the waiting and
    /// the sending part of a present and the lead from a send call to its
    /// first pixel byte (average/largest, us), pixels per present, refresh
    /// passes begun per present (x100).
    ///
    /// `short` (for `boardctl stats`, whose one frame holds at most 1 KB
    /// and already has presents, late and crossed): `send <plan> <timing>
    /// as=<windows>/<box>/<no plan> wait=<avg>/<max> send=<avg>/<max>
    /// lead=<avg>/<max>`, the same numbers. Only in builds with `send-ab`:
    /// with `pad` and `store` that frame can already be full (then its
    /// end, the `perf` lines, is cut), and this line would push more out.
    ///
    /// With `core2` the numbers are the second core's, as it last left
    /// them ([`crate::core2::snapshot`]: no wait for the second core).
    pub fn write_send(&self, out: &mut impl core::fmt::Write, short: bool) {
        #[cfg(not(feature = "core2"))]
        let (c, old_plan, timed) = {
            let s = &self.board.screen;
            (&s.counters, s.old_plan(), s.timed)
        };
        #[cfg(feature = "core2")]
        let snap = crate::core2::snapshot();
        #[cfg(feature = "core2")]
        let (c, old_plan, timed) = (&snap.counters, snap.old_plan, snap.timed);
        let n = u64::from(c.presents.max(1));
        let mhz = crate::lcd::CPU_MHZ;
        let lead_avg = c.lead_cycles / u64::from(c.leads.max(1)) / u64::from(mhz);
        if short {
            let _ = writeln!(
                out,
                "send {} {} as={}/{}/{} wait={}/{} send={}/{} lead={}/{}",
                if old_plan { "old" } else { "new" },
                if timed { "timed" } else { "fast" },
                c.as_windows,
                c.as_box,
                c.no_plan,
                c.wait_us / n,
                c.wait_max,
                c.send_us / n,
                c.send_max,
                lead_avg,
                c.lead_max / mhz,
            );
            return;
        }
        let _ = writeln!(
            out,
            "send plan={} timing={} presents={} win_x100={} as_windows={} as_box={} no_plan={} late={} crossed={} wait_us={}/{} send_us={}/{} lead_us={}/{} px={} refreshes_x100={}",
            if old_plan { "old" } else { "new" },
            if timed { "on" } else { "off" },
            c.presents,
            u64::from(c.windows) * 100 / n,
            c.as_windows,
            c.as_box,
            c.no_plan,
            c.late,
            c.crossed,
            c.wait_us / n,
            c.wait_max,
            c.send_us / n,
            c.send_max,
            lead_avg,
            c.lead_max / mhz,
            c.pixels / n,
            u64::from(c.refreshes) * 100 / n,
        );
    }

    /// The input counters in short (for `boardctl stats`, whose one
    /// frame holds at most 1 KB; the letter `i` logs the long form),
    /// then (feature `send-ab`) the screen's send line ([`App::write_send`]):
    /// touches (all, of the link), drop-outs (held / that ended a touch),
    /// largest jump between readings, resting jitter (average in tenths,
    /// largest), break-outs of the dead band (all / jitter), lone wild
    /// readings left out (spk), taps, drags,
    /// flicks, chip read time (average / largest, us); BOOT edges, notes
    /// and presses the console got; steps left out.
    #[cfg_attr(not(feature = "link"), allow(dead_code))]
    pub fn write_input_short(&self, out: &mut impl core::fmt::Write) {
        let t = &self.board.touch.counters;
        let _ = writeln!(
            out,
            "touch n={}/{} drop={}/{} jump={} jit={}/{} brk={}/{} spk={} tap={} drag={} flick={} read_us={}/{} lost={}",
            t.touches,
            t.link_touches,
            t.dropouts,
            t.dropout_ends,
            t.max_jump,
            u64::from(t.rest_jitter_sum) * 10 / u64::from(t.rest_reads.max(1)),
            t.rest_jitter_max,
            t.breakouts,
            t.false_moves,
            t.spikes,
            t.taps,
            t.drags,
            t.flicks,
            t.read_us_sum / u64::from(t.read_n.max(1)),
            t.read_us_max,
            t.edges_unread,
        );
        let k = &self.board.keys.counters;
        let _ = writeln!(
            out,
            "boot edges={} notes={} got={} menu={} paused={} skipped={}",
            k.edges,
            k.notes,
            k.console_presses,
            self.console.menu_open(),
            self.console.game().paused(),
            self.skipped,
        );
        #[cfg(feature = "send-ab")]
        self.write_send(out, true);
    }

    /// The backlight, as one text line.
    #[cfg_attr(not(feature = "link"), allow(dead_code))]
    pub fn write_light(&self, out: &mut impl core::fmt::Write) {
        let b = &self.board.screen.backlight;
        let _ = writeln!(
            out,
            "light level={} duty={}/{} calls={} pwm_hz={}",
            b.level(),
            crate::backlight::duty_for(b.level()),
            crate::backlight::STEPS,
            b.calls,
            crate::backlight::PWM_HZ,
        );
    }

    /// The input counters (touch, BOOT, skipped steps), as text lines.
    pub fn write_input(&self, out: &mut impl core::fmt::Write) {
        let t = &self.board.touch.counters;
        let _ = writeln!(
            out,
            "touch touches={} link={} reads={} dropouts={} dropout_ends={} rebounds={} edges_unread={} max_jump={} rest_reads={} rest_jitter_avg_x10={} rest_jitter_max={} breakouts={} false_moves={} spikes={}",
            t.touches,
            t.link_touches,
            t.reads,
            t.dropouts,
            t.dropout_ends,
            t.rebounds,
            t.edges_unread,
            t.max_jump,
            t.rest_reads,
            u64::from(t.rest_jitter_sum) * 10 / u64::from(t.rest_reads.max(1)),
            t.rest_jitter_max,
            t.breakouts,
            t.false_moves,
            t.spikes,
        );
        let _ = writeln!(
            out,
            "touch taps={} drags={} flicks={} read_n={} read_avg_us={} read_max_us={}",
            t.taps,
            t.drags,
            t.flicks,
            t.read_n,
            t.read_us_sum / u64::from(t.read_n.max(1)),
            t.read_us_max,
        );
        let k = &self.board.keys.counters;
        let _ = writeln!(
            out,
            "boot edges={} bounces={} notes={} console_presses={} home_menu={} game_paused={} steps_skipped={}",
            k.edges,
            k.bounces,
            k.notes,
            k.console_presses,
            self.console.menu_open(),
            self.console.game().paused(),
            self.skipped,
        );
    }

    /// Text letters from the serial port (`boardctl text`), outside link
    /// frames: one letter, one command.
    pub fn on_letter(&mut self, letter: u8) {
        // A text command (`T...;`) being typed.
        if let Some(len) = self.cmd_len {
            if letter == b';' || letter == b'\n' || len == self.cmd.len() {
                self.cmd_len = None;
                let cmd = self.cmd;
                self.on_command(&cmd[..len]);
            } else {
                self.cmd[len] = letter;
                self.cmd_len = Some(len + 1);
            }
            return;
        }
        match letter {
            b'T' => self.cmd_len = Some(0),
            #[cfg(feature = "xip-probe")] // measuring only: a d j 0-5, and r m go on below
            l if crate::xip_probe::letter(l) => {}
            b'k' => {
                self.board.keys.pulse(PULSE_US); // Measuring only: BOOT press under one frame.
                info!("boot: pulled low for {PULSE_US} us");
            }
            #[cfg(feature = "tour")]
            b't' => self.toggle_tour(),
            b'r' => {
                self.stats = Stats::default();
                #[cfg(not(feature = "core2"))]
                {
                    self.board.screen.counters = Default::default();
                }
                #[cfg(feature = "core2")]
                crate::core2::reset(&mut self.board.screen);
                self.board.touch.counters = Default::default();
                self.board.keys.counters.reset();
                self.skipped = 0;
                #[cfg(feature = "pad")]
                if let Some(r) = &mut self.radio {
                    r.reset(&mut self.board.pad);
                }
                info!("measurements reset");
            }
            b'm' => self.report(),
            #[cfg(feature = "wide-strips")]
            b'b' => crate::bench::strips(self),
            // Measuring only: act as if the card were out (`n`) or back (`c`).
            #[cfg(feature = "sd-card")]
            b'n' => self.board.files.inner.pretend_absent(true),
            #[cfg(feature = "sd-card")]
            b'c' => self.board.files.inner.pretend_absent(false),
            // One press of Home (the pause menu), as the link has no Home.
            b'h' => {
                self.last_press_us = now_us();
                self.board.pad.cable.press_once(board_api::btn::HOME);
            }
            // Measuring only: how many words of the slots are not zero.
            b'z' => self.board.games.log_slots(),
            b'i' => self.log_input(),
            #[cfg(feature = "store")]
            b'w' => self.wear_test(),
            // Measuring only: how long one full touch reading takes.
            #[cfg(feature = "touch")]
            b'x' => {
                if let Some(chip) = &mut self.board.touch.chip {
                    let (mut sum, mut max) = (0u32, 0u32);
                    for _ in 0..20 {
                        let us = chip.time_full_read();
                        sum += us;
                        max = max.max(us);
                    }
                    info!(
                        "touch: one full reading takes {} us on average, {max} at most (20 tries, no finger needed)",
                        sum / 20
                    );
                }
            }
            // Measuring only: the backlight checked from the chip's side.
            b'l' => self.board.screen.backlight.check(),
            // Measuring only (`core2`): the picture check on the second
            // core (a sum of the handed picture before and after each send).
            #[cfg(feature = "core2")]
            b'V' => {
                let on = crate::core2::toggle_verify();
                info!("core2: picture check {}", if on { "on" } else { "off" });
            }
            // Measuring only (`hang-test`): the second core parks for
            // good; the console stops 500 ms later with a message (RST).
            #[cfg(feature = "hang-test")]
            b'H' => {
                info!("core2: hang test: the second core parks for good");
                crate::core2::hang_test();
            }
            // Measuring only (`send-ab`): today's plan (`o`), the new one
            // (`p`), the timing off and on again (`u`: off is fast but may
            // tear). The counters are not reset: send `r` after a switch.
            #[cfg(feature = "send-ab")]
            b'o' | b'p' => {
                self.board.screen.old_plan = letter == b'o';
                // With `core2`: the reports read the second core's copy.
                #[cfg(feature = "core2")]
                crate::core2::sync(&self.board.screen);
                info!(
                    "send: {} plan",
                    if letter == b'o' {
                        "old (today's)"
                    } else {
                        "new"
                    }
                );
            }
            #[cfg(feature = "send-ab")]
            b'u' => {
                let s = &mut self.board.screen;
                s.timed = !s.timed;
                let timed = s.timed;
                #[cfg(feature = "core2")]
                crate::core2::sync(&self.board.screen);
                info!(
                    "send: timing {}",
                    if timed { "on" } else { "off (fast, may tear)" }
                );
            }
            _ => {}
        }
    }

    /// The scripted finger (measuring only), as text after `T`, up to `;`:
    /// `<x>,<y>` down or moved to that panel pixel, `U` lifted, `N<n>`
    /// noise of up to n pixels on each reading (0 = none).
    fn on_command(&mut self, cmd: &[u8]) {
        fn num(b: &[u8]) -> Option<u16> {
            core::str::from_utf8(b).ok()?.trim().parse().ok()
        }
        match cmd {
            [b'U'] => self.board.touch.finger(None),
            [b'N', rest @ ..] => {
                if let Some(n) = num(rest) {
                    self.board.touch.set_noise(n.min(50) as u8);
                }
            }
            _ => {
                let mut parts = cmd.split(|&b| b == b',');
                if let (Some(x), Some(y), None) = (
                    parts.next().and_then(num),
                    parts.next().and_then(num),
                    parts.next(),
                ) && x < crate::lcd::W as u16
                    && y < crate::lcd::H as u16
                {
                    self.board
                        .touch
                        .finger(Some(board_api::TouchPoint { x, y }));
                }
            }
        }
    }

    #[cfg(feature = "tour")]
    pub fn toggle_tour(&mut self) {
        let on = self.tour.toggle(now_us());
        info!("tour: {}", if on { "on" } else { "off" });
    }

    /// Looks at BOOT and the panel, then runs the console's step if it
    /// is due: `force` (something other than the look-again timer woke the
    /// loop), its own deadline came, or it waits for input and the input
    /// it would see changed.
    fn look_and_step(&mut self, force: bool) -> bool {
        let now = now_us();
        if self.board.keys.poll(now) == BootEvent::Long {
            #[cfg(feature = "tour")]
            self.toggle_tour();
        }
        self.board.touch.sample(now);
        if self.board.keys.note_waiting() {
            self.last_press_us = now;
        }
        #[cfg(feature = "tour")]
        self.tour.tick(now, &mut self.board.pad.cable, self.console);

        let input_changed =
            self.board.keys.changed() || self.board.pad.changed() || self.board.touch.changed();
        let due = force || now >= self.console_due || (self.console_on_input && input_changed);
        if !due {
            self.skipped += 1;
            return false;
        }
        let wake = self.step();
        self.board.pad.stepped();
        self.console_due = wake.deadline();
        self.console_on_input = wake.on_input();
        true
    }

    /// The serial bytes that already came, read WITHOUT waiting, and
    /// served (link frames, or plain letters). Called where the loop goes
    /// round again without its wait (the console is behind, the radio is
    /// due, an edge came during the step), because only the wait reads
    /// the port: a game that is behind on every frame would otherwise
    /// leave `boardctl` unanswered until a restart (seen on the board with
    /// LANTERN in HEAVY detail at the 60 setting). It runs between steps,
    /// never inside a screen send, and it does not make the console step:
    /// what the bytes change (a key, a tap) is seen by the next step that
    /// was going to run anyway.
    async fn serial_without_wait(
        &mut self,
        #[cfg(feature = "link")] decoder: &mut crate::link::LinkDecoder,
        bytes: &mut [u8; 64],
    ) {
        // The port's queue holds 128 bytes: a few rounds empty it, and
        // the bound keeps a flood from holding the loop here.
        for _ in 0..4 {
            let got = match self.uart.read_buffered(bytes) {
                Ok(n) => n,
                // A receive error (the queue is reset or the fault
                // cleared by the read): try again.
                Err(_) => continue,
            };
            if got == 0 {
                break;
            }
            #[cfg(feature = "link")]
            self.link_bytes(decoder, &bytes[..got]).await;
            #[cfg(not(feature = "link"))]
            for &b in &bytes[..got] {
                self.on_letter(b);
            }
        }
        // A half frame left with the port quiet for long is given up,
        // as after a wait.
        #[cfg(feature = "link")]
        self.link_idle(decoder).await;
    }

    pub async fn run(&mut self) -> ! {
        #[cfg(feature = "link")]
        let mut decoder = crate::link::new_decoder();
        let mut bytes = [0u8; 64];
        // Whether something other than the look-again timer woke us.
        let mut force = true;
        loop {
            // The radio first: a pad report that came during the last
            // frame is read before this step looks at the input.
            #[cfg(feature = "pad")]
            if let Some(r) = &mut self.radio {
                r.poll(now_us(), &mut self.board.pad);
            }
            self.look_and_step(force);
            #[cfg(feature = "store")]
            self.keep_settings();
            crate::logbuf::drain(&mut self.uart);

            let now = now_us();
            if now >= self.next_report {
                self.next_report = now + REPORT_US;
                self.report();
            }
            let held = self.board.keys.down() || self.board.touch.down();
            if held && now >= self.next_touch_log {
                if self.next_touch_log != 0 {
                    self.log_input();
                }
                self.next_touch_log = now + TOUCH_LOG_US;
            } else if !held {
                self.next_touch_log = 0;
            }

            // When to come back.
            let mut deadline = self.console_due;
            let input_changed =
                self.board.keys.changed() || self.board.pad.changed() || self.board.touch.changed();
            if input_changed && self.console_on_input {
                deadline = now;
            }
            if held {
                deadline = deadline.min(now + TOUCH_SAMPLE_US);
            }
            if crate::logbuf::pending() {
                deadline = deadline.min(now + LOG_POLL_US);
            }
            #[cfg(feature = "tour")]
            if let Some(t) = self.tour.due() {
                deadline = deadline.min(t);
            }
            #[cfg(feature = "store")]
            if let Some(t) = self
                .saver
                .as_ref()
                .and_then(|s| s.policy.deadline(self.game_running()))
            {
                deadline = deadline.min(t);
            }
            if deadline <= now {
                // Behind: no wait, so read the port here (see
                // `serial_without_wait`).
                self.serial_without_wait(
                    #[cfg(feature = "link")]
                    &mut decoder,
                    &mut bytes,
                )
                .await;
                embassy_futures::yield_now().await;
                force = true;
                continue;
            }
            // The radio's own times (pad-host's timers, the injected repeat) only
            // bring the loop round to `poll`: they never make the console
            // step (round 6: with the load test on, they did, about 1,800
            // extra steps in 5 minutes).
            #[cfg(feature = "pad")]
            if let Some(t) = self.radio.as_ref().and_then(|r| r.due()) {
                if t <= now {
                    self.serial_without_wait(
                        #[cfg(feature = "link")]
                        &mut decoder,
                        &mut bytes,
                    )
                    .await;
                    embassy_futures::yield_now().await;
                    force = false;
                    continue;
                }
                deadline = deadline.min(t);
            }
            // An edge that came during the step: the wait below would
            // clear its latch, so look at it first.
            if self.board.keys.pin.is_interrupt_set() {
                self.serial_without_wait(
                    #[cfg(feature = "link")]
                    &mut decoder,
                    &mut bytes,
                )
                .await;
                force = true;
                continue;
            }
            #[cfg(feature = "touch")]
            if let Some(chip) = &self.board.touch.chip
                && chip.irq.is_interrupt_set()
                && !self.board.touch.filter.down()
            {
                // Read at once (the sample clears the latch).
                self.board.touch.edge();
                self.serial_without_wait(
                    #[cfg(feature = "link")]
                    &mut decoder,
                    &mut bytes,
                )
                .await;
                force = true;
                continue;
            }

            let timer = Timer::at(Instant::from_micros(deadline));
            // The radio wakes the loop like the timer does: a look again,
            // and `poll` at the top reads what came.
            #[cfg(feature = "pad")]
            let radio = self.radio.as_mut();
            #[cfg(feature = "pad")]
            let timer = async move {
                match radio {
                    Some(r) => {
                        embassy_futures::select::select(timer, r.wait()).await;
                    }
                    None => timer.await,
                }
            };
            let keys = &mut self.board.keys.pin;
            #[cfg(feature = "touch")]
            let touch_irq = self.board.touch.chip.as_mut().map(|c| &mut c.irq);
            let edges = async {
                #[cfg(feature = "touch")]
                if let Some(irq) = touch_irq {
                    return match select(keys.wait_for_falling_edge(), irq.wait_for_falling_edge())
                        .await
                    {
                        Either::First(()) => Edge::Boot,
                        Either::Second(()) => Edge::Touch,
                    };
                }
                keys.wait_for_falling_edge().await;
                Edge::Boot
            };
            let woke = select3(self.uart.read_async(&mut bytes), timer, edges).await;
            // The waits turned the edge latches off: on again, so edges
            // during the next step are kept.
            self.board.keys.arm();
            #[cfg(feature = "touch")]
            if let Some(chip) = &mut self.board.touch.chip {
                chip.irq.listen(esp_hal::gpio::Event::FallingEdge);
            }
            // The timer alone: a look-again wake (or the console's own
            // deadline, which look_and_step sees by itself).
            force = !matches!(woke, Either3::Second(()));
            let got = match woke {
                Either3::First(Ok(n)) => n,
                Either3::First(Err(_)) => 0,
                Either3::Second(()) => 0,
                Either3::Third(edge) => {
                    match edge {
                        Edge::Boot => self.board.keys.edge(),
                        #[cfg(feature = "touch")]
                        Edge::Touch => self.board.touch.edge(),
                    }
                    0
                }
            };
            if got == 0 {
                #[cfg(feature = "link")]
                self.link_idle(&mut decoder).await;
                continue;
            }
            #[cfg(feature = "link")]
            self.link_bytes(&mut decoder, &bytes[..got]).await;
            #[cfg(not(feature = "link"))]
            for &b in &bytes[..got] {
                self.on_letter(b);
            }
        }
    }
}

/// Which pin's edge ended the wait.
enum Edge {
    Boot,
    #[cfg(feature = "touch")]
    Touch,
}

/// The console's task. The console and the board live in statics; this
/// task owns them from here on.
#[embassy_executor::task]
pub async fn run(app: &'static mut App) -> ! {
    app.run().await
}
