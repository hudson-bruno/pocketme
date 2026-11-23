//! The board's loop: runs `Console::step` per the board-api contract,
//! serves the debug link, drains the log, measures.
//!
//! The contract: call `step`; it returns a `Wake`. Sleep until its
//! deadline, or (for `Wake::InputOr`) until input comes. Here "sleep" is
//! an `await` on the timer, the serial port, and the BOOT and touch pins
//! (their falling edges), whichever comes first. While BOOT or the panel
//! is held, the loop also wakes once a frame to follow it.
//!
//! The whole loop is ONE embassy task ([`run`]). It is written so that
//! later the radio (classic Bluetooth for the gamepad) can be a task of
//! its own beside it. One thing to know for that: a `present` does not
//! yield. It waits for the panel's refresh by spinning and sends by DMA
//! while the CPU fills the next strip, for up to about 35 ms (one full
//! frame: see the README). A radio task on the same executor would wait
//! that long, so the radio will need its own esp-rtos thread, or an
//! executor at a higher interrupt priority.

use console_core::{Console, FRAME_US, Page};
#[cfg(feature = "touch")]
use embassy_futures::select::select;
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

pub struct App {
    pub console: &'static mut Console,
    pub board: CydBoard,
    pub uart: Uart<'static, Async>,
    pub stats: Stats,
    #[cfg(feature = "link")]
    pub link: crate::link::LinkState,
    #[cfg(feature = "tour")]
    pub tour: crate::tour::Tour,
    #[cfg(feature = "cpu1-cache-ram")]
    pub cpu1: crate::cpu1_cache::Check,
    #[cfg(feature = "cpu1-start-test")]
    pub start_test: crate::cpu1_start_test::StartTest,
    /// Lab: a stand-in for a game's update, run once per due frame
    /// (letter `G`: 0, 5, 10, 20 ms), on the first core or (letter `H`,
    /// feature `core2`) on the second, side by side with the console's
    /// step.
    pub burn_ms: u32,
    pub burn_second: bool,
    /// When the console's next frame is due (console time).
    due_at: u64,
    /// Lab (`fs-bench`): the letter `X` asked for the full-screen bench.
    #[cfg(feature = "fs-bench")]
    pub bench_pending: bool,
    next_report: u64,
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
            #[cfg(feature = "cpu1-cache-ram")]
            cpu1: crate::cpu1_cache::Check::new(),
            #[cfg(feature = "cpu1-start-test")]
            start_test: crate::cpu1_start_test::StartTest::new(),
            burn_ms: 0,
            burn_second: false,
            due_at: 0,
            #[cfg(feature = "fs-bench")]
            bench_pending: false,
            next_report: REPORT_US,
        }
    }

    /// One console step, measured.
    fn step(&mut self) -> console_core::board_api::Wake {
        let frames = self.console.frames();
        let selected = self.console.home().selected();
        let page = self.console.page();
        let c = self.board.screen.counters_now();
        let presents = c.presents;
        let present_us = c.total_us;
        let (calls, files_us) = (self.board.files.calls, self.board.files.us);

        self.stats.wakes += 1;
        let t0 = now_us();
        // Lab: the game stand-in, once per due frame.
        let due = crate::board::console_now() >= self.due_at;
        #[allow(unused_mut)]
        let mut kicked = false;
        if due && self.burn_ms > 0 {
            #[cfg(feature = "core2")]
            if self.burn_second && crate::core2::started() {
                crate::core2::kick_burn(self.burn_ms * 1000);
                kicked = true;
            }
            if !kicked {
                core::hint::black_box(crate::standin::burn(self.burn_ms * 1000));
            }
        }
        #[cfg(feature = "core2-swap")]
        let wake = if crate::core2::send_here() {
            // The console hands its picture to the second core (no copy).
            let screen: *mut crate::screen::CydScreen = &mut self.board.screen;
            let mut out = crate::core2::swap::SwapSender::new(screen);
            let wake = self.console.step_with_sender(&mut self.board, &mut out);
            if let Some(t) = out.handed_at {
                crate::core2::swap::note_after(crate::lcd::cycles().wrapping_sub(t));
            }
            wake
        } else {
            self.console.step(&mut self.board)
        };
        #[cfg(not(feature = "core2-swap"))]
        let wake = self.console.step(&mut self.board);
        // The stand-in on the second core must be done before the next
        // frame: wait for it here.
        #[cfg(feature = "core2")]
        if kicked {
            crate::core2::join();
        }
        let _ = kicked;
        self.due_at = wake.deadline();
        let t1 = now_us();

        if self.console.frames() == frames {
            self.stats.early += 1;
            return wake;
        }
        let home = self.console.home();
        let scene = if self.console.page() == Page::Boot {
            Scene::Boot
        } else if home.moving() {
            Scene::Scroll
        } else if self.console.notices().moving(crate::board::console_now()) {
            Scene::Notice
        } else if home.selected() != selected {
            Scene::Select
        } else {
            Scene::Idle
        };
        let c = self.board.screen.counters_now();
        let present = (c.presents != presents).then(|| c.total_us - present_us);
        let pixels = self.console.last_report().pixels;
        self.stats.frame(scene, t0, t1 - t0, present, pixels);

        if self.board.files.calls != calls {
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

    /// Writes the measurements to the log.
    pub fn report(&mut self) {
        let mut text = [0u8; 1024];
        let mut out = debug_link::TextBuf::new(&mut text);
        self.stats.write(&mut out);
        let text = core::str::from_utf8(out.as_bytes()).unwrap_or("");
        for line in text.lines() {
            info!("{line}");
        }
        let c = self.board.screen.counters_now();
        info!(
            "screen presents={} windows={} untimed={} no_anchor={} no_plan={} late={} crossed={} errors={} bytes={} log_dropped={}",
            c.presents,
            c.windows,
            c.untimed,
            c.no_anchor,
            c.no_plan,
            c.late,
            c.crossed,
            c.errors,
            c.bytes,
            crate::logbuf::dropped()
        );
        crate::mem::report("report");
        #[cfg(feature = "cpu1-cache-ram")]
        crate::cpu1_cache::report(&self.cpu1);
        #[cfg(feature = "core2")]
        crate::core2::report(self.burn_ms, self.burn_second, crate::board::clock_speed());
        #[cfg(feature = "core2-swap")]
        crate::core2::swap::report();
        #[cfg(not(feature = "core2"))]
        info!(
            "core2 started=false burn_ms={} console_clock_x={}",
            self.burn_ms,
            crate::board::clock_speed()
        );
        // Second reader: the stand-in's real work rate on each core.
        let [r0, r1] = crate::standin::take_rate();
        info!("standin rounds_per_ms first_core={r0} second_core={r1}");
    }

    /// Text letters from the serial port (`boardctl text`), outside link
    /// frames: one letter, one command.
    pub fn on_letter(&mut self, letter: u8) {
        // Lab: the second core may still be sending a picture.
        self.board.screen.idle();
        match letter {
            #[cfg(feature = "tour")]
            b't' => self.toggle_tour(),
            b'r' => {
                self.stats = Stats::default();
                self.board.screen.reset_counters();
                #[cfg(feature = "core2-swap")]
                crate::core2::swap::reset();
                info!("measurements reset");
            }
            // Lab: the game stand-in's length per frame: 0, 5, 10, 20 ms.
            b'G' => {
                self.burn_ms = match self.burn_ms {
                    0 => 5,
                    5 => 10,
                    10 => 20,
                    _ => 0,
                };
                info!("lab: game stand-in {} ms per frame", self.burn_ms);
            }
            // Lab: the stand-in on the first or the second core.
            b'H' => {
                self.burn_second = !self.burn_second;
                info!(
                    "lab: game stand-in on the {} core",
                    if self.burn_second { "second" } else { "first" }
                );
            }
            // Lab: the console's clock at 1x or 2x (30 or 60 frames a
            // second asked, in real time).
            b'D' => {
                let k = if crate::board::clock_speed() == 1 {
                    2
                } else {
                    1
                };
                crate::board::set_clock_speed(k);
                info!("lab: console clock {k}x ({} frames a second asked)", 30 * k);
            }
            // Lab: pictures sent by the second core (on/off).
            #[cfg(feature = "core2")]
            b'P' => {
                let on = !crate::core2::send_here();
                crate::core2::set_send_here(on, &mut self.board.screen);
                info!(
                    "lab: pictures sent by the {} core",
                    if crate::core2::send_here() {
                        "second"
                    } else {
                        "first"
                    }
                );
            }
            #[cfg(feature = "fs-bench")]
            b'X' => self.bench_pending = true,
            // Lab (core2-swap): the picture check on the second core (a
            // sum of the handed picture before and after each send).
            #[cfg(feature = "core2-swap")]
            b'V' => {
                let on = crate::core2::swap::toggle_verify();
                info!("lab: picture check {}", if on { "on" } else { "off" });
            }
            // Lab: the whole panel read back and compared with the
            // console's picture, pixel by pixel (about half a second).
            #[cfg(feature = "link")]
            b'K' => self.panel_check(),
            #[cfg(feature = "cpu1-start-test")]
            b'S' => self.start_test.ram_only(),
            #[cfg(feature = "cpu1-start-test")]
            b'E' => self.start_test.standard(),
            b'm' => self.report(),
            // Lab: one single-byte read or write in the second core's
            // cache RAM (expected to stop the board with an error).
            #[cfg(feature = "cpu1-cache-ram")]
            b'F' => {
                self.cpu1.on = !self.cpu1.on;
                info!(
                    "cpu1: flash check {}",
                    if self.cpu1.on { "on" } else { "off" }
                );
            }
            // Lab (second reader): wipe the radio's PHY code in the area
            // while the radio runs (see cpu1_cache::wipe_phy_now).
            // Lab (second reader): flash read through the driver, then
            // the area check.
            #[cfg(feature = "cpu1-cache-ram")]
            b'R' => {
                let (a, b, e) = crate::cpu1_cache::driver_read_test();
                info!(
                    "cpu1 driver read: 2 x 64 KB, sums {a:#010x} {b:#010x} same={} errors={e}",
                    a == b
                );
                crate::cpu1_cache::report(&self.cpu1);
            }
            #[cfg(feature = "cpu1-cache-ram")]
            b'Z' => {
                let (from, to) = crate::cpu1_cache::wipe_phy_now();
                log::warn!(
                    "cpu1 RUNTIME WIPE: {from:#x}..{to:#x} set to zeros while the radio runs"
                );
                crate::logbuf::flush(&mut self.uart);
            }
            #[cfg(feature = "cpu1-cache-ram")]
            b'B' => {
                info!("cpu1: byte read test now");
                crate::logbuf::flush(&mut self.uart);
                let v = crate::cpu1_cache::byte_read();
                info!("cpu1: byte read gave {v:#x} (no error)");
            }
            #[cfg(feature = "cpu1-cache-ram")]
            b'W' => {
                info!("cpu1: byte write test now");
                crate::logbuf::flush(&mut self.uart);
                crate::cpu1_cache::byte_write();
                info!("cpu1: byte write done (no error)");
            }
            #[cfg(feature = "wide-strips")]
            b'b' => crate::bench::strips(self),
            // Measuring only: act as if the card were out (`n`) or back (`c`).
            #[cfg(feature = "sd-card")]
            b'n' => self.board.files.inner.pretend_absent(true),
            #[cfg(feature = "sd-card")]
            b'c' => self.board.files.inner.pretend_absent(false),
            _ => {}
        }
    }

    #[cfg(feature = "tour")]
    pub fn toggle_tour(&mut self) {
        let on = self.tour.toggle(now_us());
        info!("tour: {}", if on { "on" } else { "off" });
    }

    pub async fn run(&mut self) -> ! {
        #[cfg(feature = "link")]
        let mut decoder = crate::link::new_decoder();
        let mut bytes = [0u8; 64];
        loop {
            let now = now_us();
            if self.board.keys.poll(now) == BootEvent::Long {
                #[cfg(feature = "tour")]
                self.toggle_tour();
            }
            #[cfg(feature = "tour")]
            self.tour.tick(now, &mut self.board.pad, self.console);

            let wake = self.step();
            // Lab: 2 KB more of the flash check per wake.
            #[cfg(feature = "cpu1-cache-ram")]
            self.cpu1.tick();
            crate::logbuf::drain(&mut self.uart);

            let now = now_us();
            if now >= self.next_report {
                self.next_report = now + REPORT_US;
                self.report();
            }

            #[cfg(feature = "fs-bench")]
            if self.bench_pending {
                self.bench_pending = false;
                crate::fs_bench::matrix(self).await;
                continue;
            }

            // When to come back (the console's time made real).
            let mut deadline = crate::board::to_real(wake.deadline());
            let input_waits = self.board.keys.click_waiting()
                || self.board.pad.waiting()
                || self.board.touch.tap_waiting();
            if input_waits && wake.on_input() {
                deadline = now;
            }
            if self.board.keys.down() || self.board.touch.down() {
                deadline = deadline.min(now + FRAME_US);
            }
            if crate::logbuf::pending() {
                deadline = deadline.min(now + LOG_POLL_US);
            }
            #[cfg(feature = "tour")]
            if let Some(t) = self.tour.due() {
                deadline = deadline.min(t);
            }
            if deadline <= now {
                embassy_futures::yield_now().await;
                continue;
            }

            let timer = Timer::at(Instant::from_micros(deadline));
            let keys = &mut self.board.keys;
            #[cfg(feature = "touch")]
            let touch_irq = self.board.touch.chip.as_mut().map(|c| &mut c.irq);
            let edges = async {
                #[cfg(feature = "touch")]
                if let Some(irq) = touch_irq {
                    select(
                        keys.pin.wait_for_falling_edge(),
                        irq.wait_for_falling_edge(),
                    )
                    .await;
                    return;
                }
                keys.pin.wait_for_falling_edge().await;
            };
            let got = match select3(self.uart.read_async(&mut bytes), timer, edges).await {
                Either3::First(Ok(n)) => n,
                Either3::First(Err(_)) => 0,
                Either3::Second(()) | Either3::Third(()) => 0,
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

/// The console's task. The console and the board live in statics; this
/// task owns them from here on.
#[embassy_executor::task]
pub async fn run(app: &'static mut App) -> ! {
    app.run().await
}
