//! The screen loop: draws the view, reads the BOOT button, and answers
//! the debug link on the serial cable.
//!
//! It runs in the same task as the radio loop (both are joined in
//! `main`), so the two take turns: while this loop draws, the radio loop
//! waits. That is why drawing is measured (the PAD-UI line):
//!
//! - `draw()` is called every 33 ms (30 times a second). pad-view draws
//!   only what changed, so most calls send nothing or a few hundred
//!   pixels. A whole new page (the first picture, a page change, the first
//!   report, a lost link) is about 140,000 pixels.
//! - The radio loop records the longest gap between two of its turns.
//!
//! The debug link (crates/debug-link, tools/boardctl) shares UART0 with
//! the log:
//!
//! - `boardctl shot` reads the picture back from the panel's own memory.
//!   The log printer is paused meanwhile, and the radio loop gets a turn
//!   after every frame of the picture.
//! - `boardctl key <any button>` acts like a BOOT press (next page). So
//!   does the single letter `n` sent as plain text.
//! - The letter `f` (`boardctl key f`) forgets every saved pairing. So
//!   does holding BOOT for 5 seconds (the first press still turns the
//!   page; from the second second on the screen counts down).
//! - `boardctl stats` gives the PAD-UI figures as key=value lines.
//! - `boardctl ping` and `boardctl raw` work as in screen-mirror.

use core::fmt::Write as _;

use debug_link::{
    TextBuf,
    frame::{COMMAND_DECODER_BUF, Decoder, Event, MAX_FRAME},
    image::{ImageSender, MIN_SENDER_FRAME},
    msg::{PixelFormat, Reply, Request, Source, kind, status},
};
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Timer};
use esp_hal::{
    Async,
    gpio::Input,
    uart::{Config as UartConfig, Uart},
};

use crate::display::Display;
use crate::lcd::{H, READ_KHZ, W, read_to_565};
use crate::ui::Shared;
use crate::{mem, out};

/// How often the view is drawn (it draws only what changed).
const DRAW_EVERY: Duration = Duration::from_millis(33);
/// How often the BOOT button is looked at.
const BUTTON_EVERY: Duration = Duration::from_millis(10);
/// The button must read the same this many times in a row (30 ms) before
/// a change counts: a press makes the contact bounce for a few ms.
const BUTTON_STABLE: u8 = 3;
/// Holding BOOT this long forgets every pairing.
const FORGET_HOLD_MS: u64 = 5_000;
/// The countdown starts after this much holding.
const COUNTDOWN_FROM_MS: u64 = 1_000;
/// The PAD-UI line.
const LOG_EVERY: Duration = Duration::from_secs(5);
/// A draw that sends more pixels than this counts as a full redraw.
const FULL_PIXELS: u32 = 20_000;
/// How long the board waits for the PC's "go" after changing speed.
const GO_WAIT: Duration = Duration::from_millis(1500);
/// Silence after which a half-received frame is given up.
const QUIET: Duration = Duration::from_millis(100);

/// Drawing figures. "win" values are since the last PAD-UI line.
#[derive(Default, Clone, Copy)]
pub struct DrawStats {
    pub draws: u32,
    /// Draws that sent something.
    pub drawn: u32,
    /// Draws that sent more than `FULL_PIXELS`.
    pub full: u32,
    pub full_us_max: u32,
    /// The longest draw that was not a full redraw, and its pixels.
    pub small_us_max: u32,
    pub small_px_at_max: u32,
    /// The longest draw that sent nothing (the cost of asking).
    pub idle_us_max: u32,
    pub win_us_max: u32,
    pub win_px_max: u32,
    pub pixels: u64,
    pub windows: u64,
    pub errors: u32,
    pub pages: u32,
    pub boot_presses: u32,
    pub serial_keys: u32,
    pub shots: u32,
    pub shot_ms_last: u32,
    /// Bus time of the longest draw since the last line.
    pub win_bus_us: u32,
}

/// A fixed piece of CPU work, timed: shows how much of the CPU this task
/// gets (it is shared with the radio's own threads).
#[inline(never)]
pub fn cpu_bench_us() -> u32 {
    let t0 = esp_hal::time::Instant::now();
    let mut h: u32 = 0x811C_9DC5;
    for i in 0..20_000u32 {
        h = core::hint::black_box((h ^ i).wrapping_mul(0x0100_0193));
    }
    core::hint::black_box(h);
    t0.elapsed().as_micros() as u32
}

struct Pending {
    kind: u8,
    seq: u8,
    payload: [u8; COMMAND_DECODER_BUF],
    len: usize,
}

pub struct Screen {
    pub display: Display,
    uart: Uart<'static, Async>,
    boot: Input<'static>,
    frame: &'static mut [u8; MAX_FRAME],
    row: &'static mut [u8; W * 2],
    scratch: &'static mut [u8; 1 + W * 3],
    pub stats: DrawStats,
    /// Debounce: the last raw reading, how many times in a row, and the
    /// state accepted.
    raw_low: bool,
    same: u8,
    pressed: bool,
    /// When BOOT went down, whether the 5 s hold already fired, and the
    /// seconds left last shown.
    held_since: Option<Instant>,
    hold_fired: bool,
    hold_left: u64,
    /// Buttons held according to the debug link.
    serial_mask: u16,
}

fn now_us() -> u64 {
    esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_micros()
}

/// Sends bytes and returns when all are in the port's buffer, without
/// waiting in between: nothing else runs, so no log text lands inside.
fn send(uart: &mut Uart<'static, Async>, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        match uart.write(bytes) {
            Ok(sent) => bytes = &bytes[sent..],
            Err(_) => return,
        }
    }
}

impl Screen {
    pub fn new(
        display: Display,
        uart: Uart<'static, Async>,
        boot: Input<'static>,
        frame: &'static mut [u8; MAX_FRAME],
        row: &'static mut [u8; W * 2],
        scratch: &'static mut [u8; 1 + W * 3],
    ) -> Self {
        Self {
            display,
            uart,
            boot,
            frame,
            row,
            scratch,
            stats: DrawStats::default(),
            raw_low: false,
            same: 0,
            pressed: false,
            held_since: None,
            hold_fired: false,
            hold_left: 0,
            serial_mask: 0,
        }
    }

    /// One draw of the view, measured.
    pub fn draw(&mut self, shared: &Shared) {
        let started = now_us();
        let mut v = shared.view.borrow_mut();
        v.set_time_ms((started / 1000) as u32);
        let result = v.draw(&mut self.display);
        drop(v);
        let us = (now_us() - started) as u32;
        let (px, win) = self.display.take_counts();
        let bus = core::mem::take(&mut self.display.bus_us) as u32;
        let s = &mut self.stats;
        s.draws += 1;
        s.pixels += u64::from(px);
        s.windows += u64::from(win);
        if result.is_err() {
            s.errors += 1;
        }
        if px == 0 {
            s.idle_us_max = s.idle_us_max.max(us);
        } else {
            s.drawn += 1;
            if px > FULL_PIXELS {
                s.full += 1;
                s.full_us_max = s.full_us_max.max(us);
            } else if us > s.small_us_max {
                s.small_us_max = us;
                s.small_px_at_max = px;
            }
        }
        if us > s.win_us_max {
            s.win_us_max = us;
            s.win_px_max = px;
            s.win_bus_us = bus;
        }
    }

    fn next_page(&mut self, shared: &Shared, why: &str) {
        shared.view.borrow_mut().next_page();
        self.stats.pages += 1;
        let page = shared.view.borrow().page();
        out::line("PAD-UI", format_args!("next page ({}) -> {:?}", why, page));
    }

    /// Reads the BOOT button (GPIO0, low while pressed) and debounces it.
    fn poll_button(&mut self, shared: &Shared) {
        let low = self.boot.is_low();
        if low == self.raw_low {
            self.same = self.same.saturating_add(1);
        } else {
            self.raw_low = low;
            self.same = 1;
        }
        if self.same >= BUTTON_STABLE && low != self.pressed {
            self.pressed = low;
            if low {
                self.stats.boot_presses += 1;
                self.held_since = Some(Instant::now());
                self.hold_fired = false;
                self.hold_left = 0;
                self.next_page(shared, "BOOT button");
            } else {
                self.held_since = None;
                if !self.hold_fired && self.hold_left > 0 {
                    shared
                        .view
                        .borrow_mut()
                        .notice("BOOT let go: pads kept", 2_000);
                }
                self.hold_left = 0;
            }
        }
        // Held down: count down, then ask the radio loop to forget all.
        if let (true, Some(since), false) = (self.pressed, self.held_since, self.hold_fired) {
            let held = since.elapsed().as_millis();
            if held >= FORGET_HOLD_MS {
                self.hold_fired = true;
                shared.forget_all_asked.set(true);
                out::line("PAD-UI", format_args!("BOOT held 5 s: forget all pairings"));
            } else if held >= COUNTDOWN_FROM_MS {
                let left = (FORGET_HOLD_MS - held).div_ceil(1000);
                if left != self.hold_left {
                    self.hold_left = left;
                    let mut l: heapless::String<40> = heapless::String::new();
                    let _ = write!(l, "Hold BOOT {} s: forget all pads", left);
                    shared.view.borrow_mut().notice(&l, 1_500);
                }
            }
        }
    }

    fn log(&mut self, shared: &Shared) {
        let s = &self.stats;
        out::line(
            "PAD-UI",
            format_args!(
                "draws={} drawn={} full={} full_us_max={} small_us_max={} small_px={} idle_us_max={} win_draw_us_max={} win_draw_px={} win_draw_bus_us={} cpu_bench_us={} pixels={} windows={} draw_errors={} reports={} event_us_max={} gap_us_max={} gap_us_max_all={} gaps_over_50ms={} tick_late_us_max={} gap_shot_us_max={} pages={} boot={} keys={} shots={} shot_ms={}",
                s.draws,
                s.drawn,
                s.full,
                s.full_us_max,
                s.small_us_max,
                s.small_px_at_max,
                s.idle_us_max,
                s.win_us_max,
                s.win_px_max,
                s.win_bus_us,
                cpu_bench_us(),
                s.pixels,
                s.windows,
                s.errors + self.display.errors,
                shared.reports.get(),
                shared.event_us_max.get(),
                shared.gap_us_max.get(),
                shared.gap_us_max_all.get(),
                shared.gaps_over_50ms.get(),
                shared.tick_late_us_max.get(),
                shared.gap_shot_us_max.get(),
                s.pages,
                s.boot_presses,
                s.serial_keys,
                s.shots,
                s.shot_ms_last
            ),
        );
        self.stats.win_us_max = 0;
        self.stats.win_px_max = 0;
        shared.gap_us_max.set(0);
        shared.tick_late_us_max.set(0);
    }

    /// The loop. Never returns.
    pub async fn run(&mut self, shared: &Shared) -> ! {
        let mut decoder = Decoder::<COMMAND_DECODER_BUF>::new();
        let mut bytes = [0u8; 64];
        let mut pending = Pending {
            kind: 0,
            seq: 0,
            payload: [0; COMMAND_DECODER_BUF],
            len: 0,
        };
        let mut last_byte = Instant::now();
        let mut next_draw = Instant::now();
        let mut next_button = Instant::now();
        let mut next_log = Instant::now() + LOG_EVERY;
        loop {
            let mut wake = next_draw.min(next_button).min(next_log);
            if decoder.mid_frame() {
                wake = wake.min(last_byte + QUIET);
            }
            let got = match select(self.uart.read_async(&mut bytes), Timer::at(wake)).await {
                Either::First(Ok(n)) => n,
                Either::First(Err(_)) => 0,
                Either::Second(()) => 0,
            };
            if got > 0 {
                last_byte = Instant::now();
            }
            let quiet = got == 0 && last_byte.elapsed() >= QUIET;
            let mut at = 0;
            while at < got || (quiet && decoder.mid_frame()) {
                let mut complete = false;
                let mut key = false;
                let mut trace_key = false;
                let mut forget_key = false;
                let mut sink = |event: Event<'_>| match event {
                    Event::Text(b) => {
                        key |= b == b'n';
                        trace_key |= b == b't';
                        forget_key |= b == b'f';
                    }
                    Event::Corrupt => {}
                    Event::Frame(frame) => {
                        pending.kind = frame.kind;
                        pending.seq = frame.seq;
                        pending.len = frame.payload.len();
                        pending.payload[..pending.len].copy_from_slice(frame.payload);
                        complete = true;
                    }
                };
                if quiet && at >= got {
                    decoder.idle(&mut sink);
                } else {
                    decoder.push(bytes[at], &mut sink);
                    at += 1;
                }
                if key {
                    self.stats.serial_keys += 1;
                    self.next_page(shared, "serial key n");
                }
                if trace_key {
                    self.stats.serial_keys += 1;
                    crate::ui::start_full_trace(shared);
                }
                if forget_key {
                    self.stats.serial_keys += 1;
                    out::line("PAD-UI", format_args!("serial key f: forget all pairings"));
                    shared.forget_all_asked.set(true);
                }
                if complete {
                    self.handle(shared, &pending).await;
                }
            }

            let now = Instant::now();
            if now >= next_button {
                self.poll_button(shared);
                next_button = now + BUTTON_EVERY;
            }
            if now >= next_draw {
                self.draw(shared);
                // One period after the start, not after the end: a long
                // draw is not followed by another at once.
                next_draw = (next_draw + DRAW_EVERY).max(Instant::now() + Duration::from_millis(5));
            }
            if now >= next_log {
                self.log(shared);
                next_log = now + LOG_EVERY;
            }
        }
    }

    fn reply(&mut self, seq: u8, reply: Reply<'_>) {
        if let Ok(len) = reply.encode(seq, &mut self.frame[..]) {
            send(&mut self.uart, &self.frame[..len]);
        }
    }

    fn ack(&mut self, seq: u8, request: u8, st: u8) {
        self.reply(
            seq,
            Reply::Ack {
                request,
                status: st,
            },
        );
    }

    async fn handle(&mut self, shared: &Shared, p: &Pending) {
        let frame = debug_link::frame::Frame {
            kind: p.kind,
            seq: p.seq,
            payload: &p.payload[..p.len],
        };
        let seq = frame.seq;
        match Request::parse(&frame) {
            Ok(Request::Ping) => self.ack(seq, kind::PING, status::OK),
            Ok(Request::Stats) => self.send_stats(shared, seq),
            Ok(Request::Buttons { mask }) => {
                self.ack(seq, kind::BUTTONS, status::OK);
                // Any newly held button: next page.
                let new = mask & !self.serial_mask;
                self.serial_mask = mask;
                if new != 0 {
                    self.stats.serial_keys += 1;
                    self.next_page(shared, "debug-link key");
                }
            }
            Ok(Request::Tap { .. }) => self.ack(seq, kind::TAP, status::UNSUPPORTED),
            Ok(Request::Shot {
                source,
                baud,
                read_khz,
            }) => self.shot(shared, seq, source, baud, read_khz).await,
            Ok(Request::PanelRaw {
                cmd,
                dummy_bits,
                read_len,
                clock_khz,
                params,
            }) => {
                if clock_khz != 0 && u32::from(clock_khz) != READ_KHZ {
                    self.ack(seq, kind::PANEL_RAW, status::BAD_REQUEST);
                    return;
                }
                let mut answer = [0u8; 256];
                let n = usize::from(read_len).min(answer.len());
                match self
                    .display
                    .lcd
                    .raw(cmd, params, dummy_bits, &mut answer[..n])
                {
                    Ok(()) => self.reply(
                        seq,
                        Reply::PanelRawReply {
                            bytes: &answer[..n],
                        },
                    ),
                    Err(_) => self.ack(seq, kind::PANEL_RAW, status::FAILED),
                }
            }
            Ok(Request::Go) => {}
            Err(debug_link::msg::ParseError::UnknownKind) => {
                self.ack(seq, frame.kind, status::UNSUPPORTED)
            }
            Err(debug_link::msg::ParseError::BadPayload) => {
                self.ack(seq, frame.kind, status::BAD_REQUEST)
            }
        }
    }

    fn send_stats(&mut self, shared: &Shared, seq: u8) {
        let mut text = [0u8; 900];
        let mut t = TextBuf::new(&mut text);
        let s = self.stats;
        let heap = mem::heap();
        let (stack_used, stack_size) = mem::stack();
        let v = shared.view.borrow();
        let _ = writeln!(t, "uptime_ms={}", Instant::now().as_millis());
        let _ = writeln!(t, "phase={}", shared.phase.get());
        let _ = writeln!(t, "linked={}", u8::from(shared.linked.get()));
        let _ = writeln!(t, "page={:?}", v.page());
        let _ = writeln!(t, "layout={:?}", v.layout().0);
        let _ = writeln!(t, "reports={}", shared.reports.get());
        let _ = writeln!(t, "draws={}", s.draws);
        let _ = writeln!(t, "full_draws={}", s.full);
        let _ = writeln!(t, "full_us_max={}", s.full_us_max);
        let _ = writeln!(t, "small_us_max={}", s.small_us_max);
        let _ = writeln!(t, "idle_us_max={}", s.idle_us_max);
        let _ = writeln!(t, "event_us_max={}", shared.event_us_max.get());
        let _ = writeln!(t, "gap_us_max_all={}", shared.gap_us_max_all.get());
        let _ = writeln!(t, "gaps_over_50ms={}", shared.gaps_over_50ms.get());
        let _ = writeln!(t, "pages={}", s.pages);
        let _ = writeln!(t, "boot_presses={}", s.boot_presses);
        let _ = writeln!(t, "serial_keys={}", s.serial_keys);
        let _ = writeln!(t, "shots={}", s.shots);
        let _ = writeln!(t, "heap_free={}", heap.free);
        let _ = writeln!(t, "heap_min_free={}", heap.min_free);
        match stack_used {
            Some(u) => {
                let _ = writeln!(t, "stack_used={u}");
            }
            None => {
                let _ = writeln!(t, "stack_used=OVER");
            }
        }
        let _ = writeln!(t, "stack_size={stack_size}");
        let _ = writeln!(t, "log_dropped={}", out::dropped());
        drop(v);
        let len = t.as_bytes().len();
        self.reply(seq, Reply::StatsText { text: &text[..len] });
    }

    fn set_baud(&mut self, baud: u32) -> bool {
        let _ = self.uart.flush();
        self.uart
            .apply_config(&UartConfig::default().with_baudrate(baud))
            .is_ok()
    }

    async fn wait_for_go(&mut self, seq: u8) -> bool {
        let deadline = Instant::now() + GO_WAIT;
        let mut decoder = Decoder::<COMMAND_DECODER_BUF>::new();
        let mut bytes = [0u8; 32];
        loop {
            let got = match select(self.uart.read_async(&mut bytes), Timer::at(deadline)).await {
                Either::First(Ok(got)) => got,
                Either::First(Err(_)) => continue,
                Either::Second(()) => return false,
            };
            let mut go = false;
            decoder.feed(&bytes[..got], &mut |event| {
                if let Event::Frame(frame) = event {
                    go |= frame.kind == kind::GO && frame.seq == seq;
                }
            });
            if go {
                return true;
            }
        }
    }

    /// A screenshot of the panel's memory (what the glass shows). Only
    /// the panel source exists here: this firmware keeps no picture in
    /// memory.
    async fn shot(&mut self, shared: &Shared, seq: u8, source: Source, baud: u32, read_khz: u16) {
        if source != Source::Panel {
            self.ack(seq, kind::SHOT, status::UNSUPPORTED);
            return;
        }
        if (baud != 0 && !(9600..=3_000_000).contains(&baud))
            || (read_khz != 0 && u32::from(read_khz) != READ_KHZ)
        {
            self.ack(seq, kind::SHOT, status::BAD_REQUEST);
            return;
        }
        let started = Instant::now();
        shared.shot_busy.set(true);
        shared.shots.set(shared.shots.get().wrapping_add(1));
        // From here on only picture frames go out: the log waits.
        out::pause(true);
        self.ack(seq, kind::SHOT, status::OK);
        let mut went = true;
        if baud != 0 {
            went = self.set_baud(baud) && self.wait_for_go(seq).await;
        }
        let mut failed = false;
        if went {
            let mut sender =
                ImageSender::new(seq, Source::Panel, PixelFormat::Rgb565, W as u16, H as u16);
            loop {
                let Self {
                    uart,
                    display,
                    frame,
                    row,
                    scratch,
                    ..
                } = self;
                let lcd = &mut display.lcd;
                let fetch = |y: u16, dst: &mut [u8]| {
                    let rgb = lcd.read_row(0, y, W as u16, &mut scratch[..])?;
                    for (px, o) in rgb.chunks_exact(3).zip(dst.chunks_exact_mut(2)) {
                        o.copy_from_slice(&read_to_565(px).to_le_bytes());
                    }
                    Ok::<(), crate::lcd::LcdError>(())
                };
                // Small frames: fewer rows read per frame, so the radio
                // loop waits less between two of its turns.
                match sender.next_frame(&mut row[..], None, fetch, &mut frame[..MIN_SENDER_FRAME]) {
                    Ok(Some(len)) => send(uart, &frame[..len]),
                    Ok(None) => break,
                    Err(_) => {
                        failed = true;
                        break;
                    }
                }
                embassy_futures::yield_now().await;
            }
        }
        if failed {
            self.ack(seq, kind::SHOT, status::FAILED);
        }
        if baud != 0 {
            self.set_baud(debug_link::BAUD);
            // The PC goes back to the resting speed a little after us; a
            // log line sent before that would reach it as garbage.
            Timer::after(Duration::from_millis(60)).await;
        }
        let _ = self.uart.flush();
        out::pause(false);
        shared.shot_busy.set(false);
        self.stats.shots += 1;
        self.stats.shot_ms_last = started.elapsed().as_millis() as u32;
        out::line(
            "PAD-UI",
            format_args!(
                "shot {} in {} ms",
                if !went {
                    "not sent (no go from the PC)"
                } else if failed {
                    "failed (panel read)"
                } else {
                    "sent"
                },
                self.stats.shot_ms_last
            ),
        );
    }
}
