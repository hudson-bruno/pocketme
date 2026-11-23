//! Experiment: the console's own display driver, measured.
//!
//! An ST7789 driver written straight on esp-hal's SPI with DMA (see
//! `lcd.rs`), the console's pipeline from a 160x120 one-byte screen to the
//! 320x240 panel (`pipeline.rs`), and finding what changed between two
//! screens (`dirty.rs`).
//!
//! At start it measures (`bench.rs`, `scan.rs`) and prints every result
//! as a `bench=...` line of `key=value` pairs. Then it runs a demo: a
//! bouncing sprite sent as small changed rectangles, switching every 5
//! seconds to a full-screen scroll, with the frame rate printed. All the
//! while it answers the debug link (`tools/boardctl`): screenshots of the
//! panel and of the logical screen, buttons, stats.
//!
//! Buttons (from `boardctl key ...`):
//!
//! * SELECT: demo / test card (the card through the pipeline, still, so a
//!   panel screenshot can be compared with the framebuffer one);
//! * START: demo in 16-bit / 12-bit colour;
//! * A: run the measurements again;
//! * B: 90 s check of the 80 MHz clock, 16-bit colour;
//! * X: the same in 12-bit colour;
//! * Y: the same at 40 MHz (the control: known good);
//! * L: scanline and refresh rate test, then the tearing characterisation;
//! * R: demo frame cap: 60 fps -> 30 fps -> none -> 60 fps ...;
//! * UP: way of sending, in the order slices locked to the refresh ->
//!   rows (today's) -> slices free -> columns (MADCTL 40h, sent behind
//!   the sweep, see `cols.rs`) -> slices locked ... (see `tear.rs`);
//! * DOWN: scene: sprite and scroll (switching every 5 s) / tear test
//!   bars / flash (the whole screen changes colour every frame);
//! * LEFT: assumed sweep direction A (left to right) / B (right to left)
//!   (columns: B flips MY and sends the columns right to left);
//! * RIGHT: measured tear counter (two scanline reads per piece) on / off.
//!
//! Everything runs in one task, one thing after the other: a log line
//! never lands inside a screenshot frame, a drawing never inside a
//! screenshot. A long check blocks the debug link until it ends.

#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use core::fmt::Write as _;

use debug_link::{
    TextBuf,
    frame::{COMMAND_DECODER_BUF, Decoder, Event, MAX_FRAME},
    image::ImageSender,
    msg::{PixelFormat, Reply, Request, Source, button, kind, status},
};
use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Timer};
use esp_backtrace as _;
use esp_hal::{
    Async,
    clock::CpuClock,
    delay::Delay,
    dma_tx_buffer,
    gpio::{Level, Output, OutputConfig},
    spi::master::{Config as SpiConfig, Spi},
    timer::timg::TimerGroup,
    uart::{Config as UartConfig, Uart},
};
use log::{info, warn};
use static_cell::{ConstStaticCell, StaticCell};

mod beam;
mod bench;
#[cfg(feature = "ble")]
mod ble;
mod check;
mod cols;
mod dirty;
mod lcd;
mod pipeline;
mod scan;
mod stack;
mod tear;

use lcd::{CPU_MHZ, Depth, H, Lcd, READ_KHZ, STRIP_BYTES, Send, W, cycles};
use pipeline::{Frame, LH, LW, Palette, Rect};
use tear::Way;

// This creates a default app-descriptor required by the esp-idf bootloader.
esp_bootloader_esp_idf::esp_app_desc!();

/// How long the board waits for the PC's "go" after changing speed.
const GO_WAIT: Duration = Duration::from_millis(1500);
/// Silence after which a half-received frame is given up.
const QUIET: Duration = Duration::from_millis(100);
/// Each demo phase lasts this long.
const PHASE: Duration = Duration::from_secs(5);
/// The demo's frame period at the 60 fps cap: about the panel's own
/// refresh (measured 58.8 Hz at its default setting). Uncapped the
/// sprite would move 1000+ times a second, too fast to judge by eye.
const FRAME_PERIOD_60: Duration = Duration::from_micros(16_667);
/// The same at the 30 fps cap: about every second refresh, which gives
/// a full-screen frame time to wait for its window.
const FRAME_PERIOD_30: Duration = Duration::from_micros(33_333);
/// Strip size used outside the benches (logical rows).
const STRIP_ROWS: usize = 8;
/// Length of the checks run from a button.
/// 90 s: with flashing first, one locked board session stays under 150 s.
const SOAK_SECONDS: u32 = 90;
/// Tear test bars: how far they move each frame (logical pixels), and
/// how wide (vertical and slanted bars) and tall (lying bars) they are.
/// A tear shows as a step in the bars; moving 3 pixels a frame makes the
/// step 6 panel pixels, easy to see.
const BAR_STEP: u32 = 3;
const BAR_W: u32 = 8;
const BAR_H: u32 = 4;
/// Flash scene: the two palette entries it switches between (black and
/// grey). A tear shows as one edge between them.
const FLASH_COLOURS: [u8; 2] = [0, 15];
/// Way of sending at start: the new one.
const WAY_AT_START: Way = Way::SlicesLocked;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Demo,
    Card,
}

/// The demo's frame cap (R button cycles it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cap {
    None,
    Fps60,
    Fps30,
}

impl Cap {
    fn next(self) -> Cap {
        match self {
            Cap::None => Cap::Fps60,
            Cap::Fps60 => Cap::Fps30,
            Cap::Fps30 => Cap::None,
        }
    }

    fn period(self) -> Option<Duration> {
        match self {
            Cap::None => None,
            Cap::Fps60 => Some(FRAME_PERIOD_60),
            Cap::Fps30 => Some(FRAME_PERIOD_30),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Cap::None => "none",
            Cap::Fps60 => "60",
            Cap::Fps30 => "30",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Sprite,
    Scroll,
    Bars,
    Flash,
}

/// What the demo shows (DOWN button).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scene {
    /// Sprite and scroll, switching every 5 s, as before.
    Auto,
    /// Black and white bars moving: vertical ones sideways (top third),
    /// lying ones up (middle), slanted ones sideways (bottom).
    Bars,
    /// The whole screen one colour, the other colour next frame.
    Flash,
}

impl Scene {
    fn next(self) -> Scene {
        match self {
            Scene::Auto => Scene::Bars,
            Scene::Bars => Scene::Flash,
            Scene::Flash => Scene::Auto,
        }
    }

    fn first_phase(self) -> Phase {
        match self {
            Scene::Auto => Phase::Sprite,
            Scene::Bars => Phase::Bars,
            Scene::Flash => Phase::Flash,
        }
    }
}

/// The demo's moving parts and its counters for one phase.
struct Demo {
    phase: Phase,
    started: Instant,
    x: i32,
    y: i32,
    dx: i32,
    dy: i32,
    scroll: u32,
    /// When the next frame is due (capped mode).
    due: Instant,
    /// The frame cap (R button cycles it).
    cap: Cap,
    frames: u32,
    present_cycles: u64,
    present_max: u32,
    cpu_cycles: u64,
    rects: u32,
    bytes: u64,
}

impl Demo {
    const SIZE: i32 = 16;

    fn new() -> Self {
        Self {
            phase: Phase::Sprite,
            started: Instant::now(),
            x: 20,
            y: 30,
            dx: 1,
            dy: 1,
            scroll: 0,
            due: Instant::now(),
            cap: Cap::Fps60,
            frames: 0,
            present_cycles: 0,
            present_max: 0,
            cpu_cycles: 0,
            rects: 0,
            bytes: 0,
        }
    }

    fn restart_counters(&mut self) {
        self.started = Instant::now();
        self.frames = 0;
        self.present_cycles = 0;
        self.present_max = 0;
        self.cpu_cycles = 0;
        self.rects = 0;
        self.bytes = 0;
    }
}

/// The background: a pattern that shows movement when scrolled.
#[inline(always)]
fn background(x: u32, y: u32) -> u8 {
    let checker = ((x >> 4) ^ (y >> 4)) & 1;
    // Palette entries 16..255 spread the bits over red, green, blue.
    let shade = 16 + ((y * 2 + (x >> 3)) % 200) as u8;
    if checker == 1 { shade } else { 1 }
}

#[derive(Default)]
struct Counters {
    rx_bytes: u32,
    rx_junk: u32,
    rx_frames: u32,
    rx_corrupt: u32,
    rx_errors: u32,
    shots: u32,
    taps: u32,
    buttons: u16,
    shot_total_ms: u32,
    panel_id: [u8; 3],
    last_frame_us: u32,
}

struct App {
    uart: Uart<'static, Async>,
    lcd: Lcd,
    fb: &'static mut Frame,
    /// What the panel holds now, as a logical screen: compared with `fb`
    /// to find what to send.
    shown: &'static mut Frame,
    pal: &'static mut Palette,
    frame: &'static mut [u8; MAX_FRAME],
    row: &'static mut [u8; W * 2],
    scratch: &'static mut [u8; 1 + W * 3],
    mode: Mode,
    demo: Demo,
    /// The drawing clock used outside the benches: 80 MHz if the card
    /// read back right at it at the start, else 40.
    mhz: u32,
    depth: Depth,
    counters: Counters,
    /// The model of the panel's refresh (`beam.rs`).
    beam: beam::Beam,
    /// How demo frames are sent (UP button).
    way: Way,
    scene: Scene,
    /// Measured tear counter on (RIGHT button).
    measure: bool,
    tear: tear::Stats,
}

struct Pending {
    kind: u8,
    seq: u8,
    payload: [u8; COMMAND_DECODER_BUF],
    len: usize,
}

/// Sends bytes and returns when all are in the port's buffer, without
/// `await`: nothing else gets to run, so no log line lands in a frame.
fn send(uart: &mut Uart<'static, Async>, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        match uart.write(bytes) {
            Ok(sent) => bytes = &bytes[sent..],
            Err(_) => return,
        }
    }
}

/// Heap and main stack, one line.
fn memory(when: &str) {
    let heap = esp_alloc::HEAP.stats();
    let (used, size) = stack::deepest();
    info!(
        "bench=memory when={when} heap_size={} heap_used={} heap_free={} heap_max_used={} stack_deepest={} stack_size={size}",
        heap.size,
        heap.current_usage,
        heap.size - heap.current_usage,
        heap.max_usage,
        used.map_or(-1, |u| u as i64),
    );
}

impl App {
    fn reply(&mut self, seq: u8, reply: Reply<'_>) {
        if let Ok(len) = reply.encode(seq, &mut self.frame[..]) {
            send(&mut self.uart, &self.frame[..len]);
        }
    }

    fn ack(&mut self, seq: u8, request: u8, status: u8) {
        self.reply(seq, Reply::Ack { request, status });
    }

    /// Puts the test card in the framebuffer and the console palette in
    /// place.
    fn reset_fb(&mut self) {
        self.pal.set_all(&test_card::PALETTE);
        // Drawing into memory cannot fail.
        let _ = test_card::draw_fb_card(&mut test_card::Fb::new(&mut self.fb.0[..]));
    }

    /// Sends the whole logical screen at the working clock and depth.
    fn present_all(&mut self) {
        let _ = self.lcd.set_write_mhz(self.mhz);
        let _ = self.lcd.set_depth(self.depth);
        match pipeline::present_full(&mut self.lcd, self.fb, self.pal, STRIP_ROWS, Send::Overlap) {
            Ok(t) => self.counters.last_frame_us = t.total / CPU_MHZ,
            Err(e) => warn!("present failed: {e:?}"),
        }
        self.shown.0.copy_from_slice(&self.fb.0);
    }

    fn show_card(&mut self) {
        self.reset_fb();
        self.present_all();
        info!(
            "card: shown through the pipeline in {} us at {} MHz, {}-bit",
            self.counters.last_frame_us,
            self.mhz,
            self.depth.bits()
        );
    }

    fn start_demo(&mut self) {
        self.pal.set_all(&test_card::PALETTE);
        // A new demo, but the cap setting (R button) is kept.
        let cap = self.demo.cap;
        self.demo = Demo::new();
        self.demo.cap = cap;
        // Something else ran before this (start-up, a check, a button):
        // the last anchor may be older than the cycle counter's wrap.
        self.beam.forget();
        self.demo.phase = self.scene.first_phase();
        self.draw_background();
        self.draw_sprite();
        self.present_all();
        self.demo.restart_counters();
        self.tear.restart();
        info!(
            "demo: start phase={:?} scene={:?} way={} dir={} measure={} mhz={} depth={}",
            self.demo.phase,
            self.scene,
            self.way.name(),
            if self.beam.reversed { "B" } else { "A" },
            self.measure,
            self.mhz,
            self.depth.bits()
        );
    }

    /// Tear test bars, moved by the frame count.
    fn draw_bars(&mut self) {
        let t = self.demo.scroll * BAR_STEP;
        for y in 0..LH as u32 {
            let row = &mut self.fb.0[y as usize * LW..][..LW];
            for (x, p) in row.iter_mut().enumerate() {
                let x = x as u32;
                let on = match y * 3 / LH as u32 {
                    0 => (x + t) / BAR_W,
                    1 => (y + t) / BAR_H,
                    _ => (x + y + t) / BAR_W,
                } & 1;
                *p = if on == 1 { 7 } else { 0 };
            }
        }
    }

    /// Two small squares in the top left corner: the way of sending
    /// (red rows, yellow slices free, green slices locked, magenta
    /// columns) and the
    /// assumed sweep direction (white A, blue B), so a photo or a look
    /// tells which mode is on.
    fn draw_marker(&mut self) {
        const S: usize = 6;
        let way = match self.way {
            Way::Rows => 8,
            Way::SlicesFree => 10,
            Way::SlicesLocked => 11,
            Way::Columns => 14,
        };
        let dir = if self.beam.reversed { 12 } else { 7 };
        for y in 0..S {
            self.fb.0[y * LW..][..S].fill(way);
            self.fb.0[y * LW + S..][..S].fill(dir);
        }
    }

    fn scene_name(&self) -> &'static str {
        match self.demo.phase {
            Phase::Sprite => "sprite",
            Phase::Scroll => "scroll",
            Phase::Bars => "bars",
            Phase::Flash => "flash",
        }
    }

    fn draw_background(&mut self) {
        let s = self.demo.scroll;
        for y in 0..LH {
            let row = &mut self.fb.0[y * LW..][..LW];
            for (x, p) in row.iter_mut().enumerate() {
                *p = background(x as u32 + s, y as u32);
            }
        }
    }

    /// Puts the background back where the sprite is.
    fn erase_sprite(&mut self) {
        let s = self.demo.scroll;
        for y in self.demo.y..self.demo.y + Demo::SIZE {
            for x in self.demo.x..self.demo.x + Demo::SIZE {
                self.fb.0[y as usize * LW + x as usize] = background(x as u32 + s, y as u32);
            }
        }
    }

    /// A 16x16 square: yellow with a red ring and a dark middle.
    fn draw_sprite(&mut self) {
        for j in 0..Demo::SIZE {
            for i in 0..Demo::SIZE {
                let edge = i.min(j).min(Demo::SIZE - 1 - i).min(Demo::SIZE - 1 - j);
                let colour = match edge {
                    0 | 1 => 8,
                    2..=4 => 10,
                    _ => 0,
                };
                self.fb.0[(self.demo.y + j) as usize * LW + (self.demo.x + i) as usize] = colour;
            }
        }
    }

    fn move_sprite(&mut self) {
        let d = &mut self.demo;
        d.x += d.dx;
        d.y += d.dy;
        if d.x <= 0 || d.x + Demo::SIZE >= LW as i32 {
            d.dx = -d.dx;
        }
        if d.y <= 0 || d.y + Demo::SIZE >= LH as i32 {
            d.dy = -d.dy;
        }
        d.x = d.x.clamp(0, LW as i32 - Demo::SIZE);
        d.y = d.y.clamp(0, LH as i32 - Demo::SIZE);
    }

    /// One demo frame: change the logical screen, send what changed.
    fn demo_frame(&mut self) {
        let start = cycles();
        match self.demo.phase {
            Phase::Sprite => {
                self.erase_sprite();
                self.move_sprite();
                self.draw_sprite();
            }
            Phase::Scroll => {
                self.demo.scroll = self.demo.scroll.wrapping_add(1);
                self.move_sprite();
                self.draw_background();
                self.draw_sprite();
            }
            Phase::Bars => {
                self.demo.scroll = self.demo.scroll.wrapping_add(1);
                self.draw_bars();
            }
            Phase::Flash => {
                self.demo.scroll = self.demo.scroll.wrapping_add(1);
                self.fb
                    .0
                    .fill(FLASH_COLOURS[(self.demo.scroll & 1) as usize]);
            }
        }
        self.draw_marker();
        let mut sent = 0u32;
        let mut rects = [Rect::default(); dirty::MAX_RECTS];
        let present_start = cycles();
        // The sprite sends what changed; the other scenes change the
        // whole screen. Either way through the chosen way of sending,
        // which also counts tears.
        let n = if self.demo.phase == Phase::Sprite {
            dirty::diff(self.shown, self.fb, &mut rects)
        } else {
            rects[0] = Rect::FULL;
            1
        };
        match tear::present(
            &mut self.lcd,
            self.fb,
            self.pal,
            &rects[..n],
            self.way,
            &mut self.beam,
            self.measure,
            &mut self.tear,
            STRIP_ROWS,
        ) {
            Ok(bytes) => sent += bytes,
            Err(e) => warn!("demo: present failed: {e:?}"),
        }
        self.demo.rects += n as u32;
        self.shown.0.copy_from_slice(&self.fb.0);
        let end = cycles();
        let present = end.wrapping_sub(present_start);
        let d = &mut self.demo;
        d.frames += 1;
        d.present_cycles += u64::from(present);
        d.present_max = d.present_max.max(present);
        d.cpu_cycles += u64::from(present_start.wrapping_sub(start));
        d.bytes += u64::from(sent);

        if d.started.elapsed() >= PHASE {
            let ms = d.started.elapsed().as_millis().max(1);
            let frames = u64::from(d.frames.max(1));
            info!(
                "bench=demo phase={:?} capped={} mhz={} depth={} frames={} seconds={}.{:03} fps={}.{} present_avg_us={} present_max_us={} draw_avg_us={} rects_per_frame={}.{:02} bytes_per_frame={} cap_fps={}",
                d.phase,
                d.cap != Cap::None,
                self.mhz,
                self.depth.bits(),
                d.frames,
                ms / 1000,
                ms % 1000,
                u64::from(d.frames) * 1000 / ms,
                u64::from(d.frames) * 10_000 / ms % 10,
                d.present_cycles / frames / u64::from(CPU_MHZ),
                d.present_max / CPU_MHZ,
                d.cpu_cycles / frames / u64::from(CPU_MHZ),
                u64::from(d.rects) / frames,
                u64::from(d.rects) * 100 / frames % 100,
                d.bytes / frames,
                d.cap.name(),
            );
            let scene = self.scene_name();
            self.tear.report(
                self.way,
                scene,
                &self.beam,
                self.measure,
                self.mhz,
                self.depth,
                ms,
            );
            let d = &mut self.demo;
            d.phase = match (self.scene, d.phase) {
                (Scene::Auto, Phase::Sprite) => Phase::Scroll,
                (Scene::Auto, _) => Phase::Sprite,
                (_, phase) => phase,
            };
            d.restart_counters();
        }
    }

    // ---- measurements ---------------------------------------------------

    fn run_benches(&mut self, label: &str) {
        info!("bench: start{label}");
        let start = Instant::now();
        self.reset_fb();
        bench::full_frames(&mut self.lcd, self.fb, self.pal, label);
        bench::rects(&mut self.lcd, self.fb, self.pal, label);
        bench::cpu(&mut self.lcd, self.fb, self.shown, self.pal, label);
        bench::colours(&mut self.lcd, self.fb, self.pal, &mut self.scratch[..]);
        // Can the working clock be 80 MHz? One card at 80 MHz, read back.
        let _ = self.lcd.set_write_mhz(80);
        let _ = self.lcd.set_depth(Depth::Rgb565);
        let _ = pipeline::present_full(&mut self.lcd, self.fb, self.pal, STRIP_ROWS, Send::Overlap);
        let (wrong, _) = bench::verify_fb(
            &mut self.lcd,
            self.fb,
            self.pal,
            Depth::Rgb565,
            &mut self.scratch[..],
        );
        self.mhz = if wrong == 0 { 80 } else { 40 };
        let _ = self.lcd.set_write_mhz(self.mhz);
        info!(
            "bench: done{label} in {} ms; working clock {} MHz",
            start.elapsed().as_millis(),
            self.mhz
        );
        memory("after_bench");
    }

    fn soak(&mut self, mhz: u32, depth: Depth) {
        let _ = check::soak(
            &mut self.lcd,
            self.fb,
            self.pal,
            &mut self.scratch[..],
            mhz,
            depth,
            SOAK_SECONDS,
        );
        memory("after_check");
        self.after_interruption();
    }

    /// Puts back the current picture after something else drew.
    fn after_interruption(&mut self) {
        self.pal.set_all(&test_card::PALETTE);
        match self.mode {
            Mode::Card => self.show_card(),
            Mode::Demo => self.start_demo(),
        }
    }

    // ---- the debug link -------------------------------------------------

    fn on_buttons(&mut self, mask: u16) {
        let pressed = mask & !self.counters.buttons;
        self.counters.buttons = mask;
        info!("input: buttons mask=0x{mask:04x}");
        if pressed & button::SELECT != 0 {
            self.mode = if self.mode == Mode::Demo {
                Mode::Card
            } else {
                Mode::Demo
            };
            info!("mode: {:?}", self.mode);
            self.after_interruption();
        }
        if pressed & button::START != 0 {
            self.depth = if self.depth == Depth::Rgb565 {
                Depth::Rgb444
            } else {
                Depth::Rgb565
            };
            info!("depth: {}-bit", self.depth.bits());
            self.after_interruption();
        }
        if pressed & button::A != 0 {
            self.run_benches("");
            self.after_interruption();
        }
        if pressed & button::B != 0 {
            self.soak(80, Depth::Rgb565);
        }
        if pressed & button::X != 0 {
            self.soak(80, Depth::Rgb444);
        }
        if pressed & button::Y != 0 {
            self.soak(40, Depth::Rgb565);
        }
        if pressed & button::R != 0 {
            self.demo.cap = self.demo.cap.next();
            info!("demo: cap={}", self.demo.cap.name());
            self.restart_counting();
        }
        if pressed & button::L != 0 {
            scan::run(&mut self.lcd);
            self.characterise();
            self.after_interruption();
        }
        if pressed & button::UP != 0 {
            self.way = self.way.next();
            info!("tear: way={} cycle={}", self.way.name(), Way::CYCLE);
            self.restart_counting();
        }
        if pressed & button::DOWN != 0 {
            self.scene = self.scene.next();
            info!("tear: scene={:?}", self.scene);
            if self.mode == Mode::Demo {
                self.start_demo();
            }
        }
        if pressed & button::LEFT != 0 {
            self.beam.reversed = !self.beam.reversed;
            info!(
                "tear: dir={} ({})",
                if self.beam.reversed { "B" } else { "A" },
                if self.beam.reversed {
                    "right to left"
                } else {
                    "left to right"
                }
            );
            self.restart_counting();
        }
        if pressed & button::RIGHT != 0 {
            self.measure = !self.measure;
            info!("tear: measure={}", self.measure);
            self.restart_counting();
        }
    }

    /// The scanline survey and the sender's checks (`beam.rs`), on the
    /// framebuffer as it is.
    fn characterise(&mut self) {
        beam::characterise(
            &mut self.lcd,
            self.fb,
            self.pal,
            &mut self.scratch[..],
            &mut self.beam,
        );
    }

    fn restart_counting(&mut self) {
        self.demo.restart_counters();
        self.tear.restart();
    }

    fn send_stats(&mut self, seq: u8) {
        let mut text = [0u8; 640];
        let mut out = TextBuf::new(&mut text);
        let c = &self.counters;
        let (stack_used, stack_size) = stack::deepest();
        let heap = esp_alloc::HEAP.stats();
        let _ = writeln!(out, "firmware=display-dma {}", env!("CARGO_PKG_VERSION"));
        let _ = writeln!(out, "ble={}", cfg!(feature = "ble"));
        let _ = writeln!(out, "uptime_ms={}", Instant::now().as_millis());
        let _ = writeln!(out, "mode={:?}", self.mode);
        let _ = writeln!(out, "demo_phase={:?}", self.demo.phase);
        let _ = writeln!(out, "demo_cap={}", self.demo.cap.name());
        let _ = writeln!(out, "write_mhz={}", self.mhz);
        let _ = writeln!(out, "depth={}", self.depth.bits());
        let _ = writeln!(out, "tear_way={}", self.way.name());
        let _ = writeln!(out, "tear_scene={:?}", self.scene);
        let _ = writeln!(
            out,
            "tear_dir={}",
            if self.beam.reversed { "B" } else { "A" }
        );
        let _ = writeln!(
            out,
            "tear_cols_ok={}",
            self.beam
                .orient
                .confirms(cols::Config::for_direction(self.beam.reversed))
        );
        let _ = writeln!(out, "tear_measure={}", self.measure);
        let _ = writeln!(out, "heap_free={}", heap.size - heap.current_usage);
        let _ = writeln!(out, "heap_used={}", heap.current_usage);
        match stack_used {
            Some(used) => {
                let _ = writeln!(out, "stack_used={used}");
            }
            None => {
                let _ = writeln!(out, "stack_used=more_than_measured");
            }
        }
        let _ = writeln!(out, "stack_size={stack_size}");
        let _ = writeln!(out, "frame_us={}", c.last_frame_us);
        let _ = writeln!(out, "shots={}", c.shots);
        let _ = writeln!(out, "shot_total_ms={}", c.shot_total_ms);
        let _ = writeln!(out, "buttons=0x{:04x}", c.buttons);
        let _ = writeln!(out, "taps={}", c.taps);
        let _ = writeln!(
            out,
            "panel_id={:02x}{:02x}{:02x}",
            c.panel_id[0], c.panel_id[1], c.panel_id[2]
        );
        let _ = writeln!(out, "read_khz={READ_KHZ}");
        let _ = writeln!(out, "rx_bytes={}", c.rx_bytes);
        let _ = writeln!(out, "rx_junk={}", c.rx_junk);
        let _ = writeln!(out, "rx_frames={}", c.rx_frames);
        let _ = writeln!(out, "rx_corrupt={}", c.rx_corrupt);
        let _ = writeln!(out, "rx_errors={}", c.rx_errors);
        if let Ok(len) = (Reply::StatsText {
            text: out.as_bytes(),
        })
        .encode(seq, &mut self.frame[..])
        {
            send(&mut self.uart, &self.frame[..len]);
        }
    }

    fn panel_raw(
        &mut self,
        seq: u8,
        cmd: u8,
        dummy_bits: u8,
        read_len: u16,
        clock_khz: u16,
        params: &[u8],
    ) {
        const RAW_MAX: usize = 256;
        let khz = if clock_khz == 0 {
            READ_KHZ
        } else {
            u32::from(clock_khz)
        };
        let mut answer = [0u8; RAW_MAX];
        let answer = &mut answer[..usize::from(read_len).min(RAW_MAX)];
        match self.lcd.raw(khz, cmd, params, dummy_bits, answer) {
            Ok(()) => self.reply(seq, Reply::PanelRawReply { bytes: answer }),
            Err(error) => {
                warn!("panel command {cmd:02x}h failed: {error:?}");
                self.ack(seq, kind::PANEL_RAW, status::FAILED);
            }
        }
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
            if Instant::now() >= deadline {
                return false;
            }
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

    async fn shot(&mut self, seq: u8, source: Source, baud: u32, read_khz: u16) {
        // Only the default read clock: the driver's read path is set up
        // for it, and it is the one proven clean on this board.
        if (baud != 0 && !(9600..=3_000_000).contains(&baud))
            || (read_khz != 0 && u32::from(read_khz) != READ_KHZ)
        {
            self.ack(seq, kind::SHOT, status::BAD_REQUEST);
            return;
        }
        let started = Instant::now();
        self.ack(seq, kind::SHOT, status::OK);
        let mut went = true;
        if baud != 0 {
            went = self.set_baud(baud) && self.wait_for_go(seq).await;
        }
        // The read format is 3 bytes a pixel either way, but in 12-bit
        // mode the panel is put in 16-bit for reading, as the checks do.
        let twelve = self.lcd.depth() == Depth::Rgb444;
        if twelve {
            let _ = self
                .lcd
                .command(lcd::cmd::COLMOD, &[Depth::Rgb565.colmod()]);
        }
        let mut failed = false;
        if went {
            let (format, width, height) = match source {
                Source::Panel => (PixelFormat::Rgb565, W as u16, H as u16),
                Source::Framebuffer => (PixelFormat::Indexed8, LW as u16, LH as u16),
            };
            let mut sender = ImageSender::new(seq, source, format, width, height);
            let App {
                uart,
                lcd,
                fb,
                pal,
                frame,
                row,
                scratch,
                ..
            } = self;
            loop {
                let fetch = |y: u16, out: &mut [u8]| match source {
                    Source::Panel => {
                        let rgb = lcd.read_row(0, y, W as u16, &mut scratch[..])?;
                        for (px, out) in rgb.chunks_exact(3).zip(out.chunks_exact_mut(2)) {
                            out.copy_from_slice(&lcd::read_to_565(px).to_le_bytes());
                        }
                        Ok(())
                    }
                    Source::Framebuffer => {
                        out.copy_from_slice(fb.row(usize::from(y)));
                        Ok::<(), lcd::LcdError>(())
                    }
                };
                match sender.next_frame(&mut row[..], Some(&pal.rgb565), fetch, &mut frame[..]) {
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
        if twelve {
            let _ = self
                .lcd
                .command(lcd::cmd::COLMOD, &[Depth::Rgb444.colmod()]);
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
        // A shot can last longer than the cycle counter's wrap (17.9 s):
        // then the last anchor could look recent.
        self.beam.forget();
        self.counters.shots += 1;
        self.counters.shot_total_ms = started.elapsed().as_millis() as u32;
        if !went {
            warn!("shot: no go from the PC at {baud} baud, nothing sent");
        } else if failed {
            warn!("shot: reading the panel failed");
        } else {
            info!(
                "shot: {:?} sent in {} ms",
                source, self.counters.shot_total_ms
            );
        }
    }

    async fn handle(&mut self, pending: &Pending) {
        let frame = debug_link::frame::Frame {
            kind: pending.kind,
            seq: pending.seq,
            payload: &pending.payload[..pending.len],
        };
        let seq = frame.seq;
        match Request::parse(&frame) {
            Ok(Request::Ping) => self.ack(seq, kind::PING, status::OK),
            Ok(Request::Stats) => self.send_stats(seq),
            Ok(Request::Buttons { mask }) => {
                // Answer first: a check takes long, and the PC should not
                // think the request was lost.
                self.ack(seq, kind::BUTTONS, status::OK);
                self.on_buttons(mask);
            }
            Ok(Request::Tap { x, y }) => {
                self.counters.taps += 1;
                let ok = usize::from(x) < W && usize::from(y) < H;
                self.ack(
                    seq,
                    kind::TAP,
                    if ok { status::OK } else { status::BAD_REQUEST },
                );
            }
            Ok(Request::Shot {
                source,
                baud,
                read_khz,
            }) => self.shot(seq, source, baud, read_khz).await,
            Ok(Request::PanelRaw {
                cmd,
                dummy_bits,
                read_len,
                clock_khz,
                params,
            }) => {
                self.panel_raw(seq, cmd, dummy_bits, read_len, clock_khz, params);
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

    async fn run(&mut self) -> ! {
        let mut decoder = Decoder::<COMMAND_DECODER_BUF>::new();
        let mut bytes = [0u8; 64];
        let mut pending = Pending {
            kind: 0,
            seq: 0,
            payload: [0; COMMAND_DECODER_BUF],
            len: 0,
        };
        let mut last_byte = Instant::now();
        loop {
            // In the demo the timer wakes the loop when the next frame is
            // due (at once when uncapped); meanwhile the serial port is
            // served.
            let wake = if self.mode == Mode::Demo {
                if self.demo.cap.period().is_some() {
                    self.demo.due
                } else {
                    Instant::now()
                }
            } else {
                Instant::now() + QUIET
            };
            let got = match select(self.uart.read_async(&mut bytes), Timer::at(wake)).await {
                Either::First(Ok(got)) => got,
                Either::First(Err(_)) => {
                    self.counters.rx_errors += 1;
                    continue;
                }
                Either::Second(()) => 0,
            };
            self.counters.rx_bytes += got as u32;
            if got > 0 {
                last_byte = Instant::now();
            }
            // A pause on the line gives up a half-received frame.
            let quiet = got == 0 && last_byte.elapsed() >= QUIET;
            let mut at = 0;
            while at < got || (quiet && decoder.mid_frame()) {
                let mut complete = false;
                let counters = &mut self.counters;
                let mut sink = |event: Event<'_>| match event {
                    // Possibly a GPS receiver's position: only counted,
                    // never shown.
                    Event::Text(_) => counters.rx_junk += 1,
                    Event::Corrupt => counters.rx_corrupt += 1,
                    Event::Frame(frame) => {
                        counters.rx_frames += 1;
                        pending.kind = frame.kind;
                        pending.seq = frame.seq;
                        pending.len = frame.payload.len();
                        pending.payload[..pending.len].copy_from_slice(frame.payload);
                        complete = true;
                    }
                };
                if quiet {
                    decoder.idle(&mut sink);
                } else {
                    decoder.push(bytes[at], &mut sink);
                    at += 1;
                }
                if complete {
                    self.handle(&pending).await;
                }
            }
            // Not only when the line was quiet: a GPS module on the same
            // RX pin may send text all the time, and must not stop the
            // frames.
            let period = self.demo.cap.period();
            if self.mode == Mode::Demo && (period.is_none() || Instant::now() >= self.demo.due) {
                // Next frame one period after this one was due; if this
                // one ran late, one period from now (no catching up).
                let now = Instant::now();
                self.demo.due =
                    (self.demo.due + period.unwrap_or(Duration::from_ticks(0))).max(now);
                self.demo_frame();
            }
        }
    }
}

/// The CPU clock, checked: cycles counted against the system timer.
fn check_cpu_clock() {
    let start = esp_hal::time::Instant::now();
    let c0 = cycles();
    while start.elapsed().as_millis() < 100 {}
    let c = cycles().wrapping_sub(c0);
    let us = start.elapsed().as_micros();
    info!(
        "bench=cpu_clock cycles_per_us={} expected={CPU_MHZ}",
        u64::from(c) / us.max(1)
    );
}

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    esp_println::logger::init_logger_from_env();
    stack::paint();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    // The same heap as the reference firmware: only the radio uses it.
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 98768);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    info!(
        "display-dma {}: own ST7789 driver with DMA, Bluetooth {}",
        env!("CARGO_PKG_VERSION"),
        if cfg!(feature = "ble") { "ON" } else { "off" }
    );
    check_cpu_clock();

    // Setting the port up throws away what is still waiting to go out,
    // so the lines above are given time to leave first.
    Delay::new().delay_millis(20);
    let uart = Uart::new(
        peripherals.UART0,
        UartConfig::default().with_baudrate(debug_link::BAUD),
    )
    .unwrap()
    .with_rx(peripherals.GPIO3)
    .with_tx(peripherals.GPIO1)
    .into_async();

    let mut backlight = Output::new(peripherals.GPIO21, Level::Low, OutputConfig::default());

    // SPI2 on its own pins (needed for 80 MHz), with its DMA channel. CS
    // is a plain pin so it can stay low across several transfers.
    let spi = Spi::new(peripherals.SPI2, SpiConfig::default())
        .unwrap()
        .with_sck(peripherals.GPIO14)
        .with_mosi(peripherals.GPIO13)
        .with_miso(peripherals.GPIO12)
        .with_dma(peripherals.DMA_SPI2);
    // Each macro call makes its own static buffer.
    let buf_a = dma_tx_buffer!(STRIP_BYTES).unwrap();
    let buf_b = dma_tx_buffer!(STRIP_BYTES).unwrap();
    let cs = Output::new(peripherals.GPIO15, Level::High, OutputConfig::default());
    let dc = Output::new(peripherals.GPIO2, Level::High, OutputConfig::default());
    let mut lcd = Lcd::new(spi, buf_a, buf_b, cs, dc);
    if let Err(e) = lcd.init() {
        warn!("panel set-up failed: {e:?}");
    }

    static FB: ConstStaticCell<Frame> = ConstStaticCell::new(Frame::new());
    static SHOWN: ConstStaticCell<Frame> = ConstStaticCell::new(Frame::new());
    static PAL: ConstStaticCell<Palette> = ConstStaticCell::new(Palette::new());
    static FRAME: ConstStaticCell<[u8; MAX_FRAME]> = ConstStaticCell::new([0; MAX_FRAME]);
    static ROW: ConstStaticCell<[u8; W * 2]> = ConstStaticCell::new([0; W * 2]);
    static SCRATCH: ConstStaticCell<[u8; 1 + W * 3]> = ConstStaticCell::new([0; 1 + W * 3]);
    static APP: StaticCell<App> = StaticCell::new();
    let app = APP.init(App {
        uart,
        lcd,
        fb: FB.take(),
        shown: SHOWN.take(),
        pal: PAL.take(),
        frame: FRAME.take(),
        row: ROW.take(),
        scratch: SCRATCH.take(),
        mode: Mode::Demo,
        demo: Demo::new(),
        mhz: 40,
        depth: Depth::Rgb565,
        counters: Counters::default(),
        beam: beam::Beam::new(),
        way: WAY_AT_START,
        scene: Scene::Auto,
        measure: false,
        tear: tear::Stats::new(),
    });
    app.counters.panel_id = scan::report(&mut app.lcd);
    app.reset_fb();
    app.present_all();
    backlight.set_high();
    memory("start");

    #[cfg(feature = "ble")]
    {
        spawner.spawn(ble::task(peripherals.BT).unwrap());
        let mut waited_ms = 0;
        while !ble::advertising() && waited_ms < 10_000 {
            Timer::after(Duration::from_millis(50)).await;
            waited_ms += 50;
        }
        info!(
            "bench=ble advertising={} waited_ms={waited_ms}",
            ble::advertising()
        );
        Timer::after(Duration::from_millis(500)).await;
        memory("ble_advertising");
    }
    #[cfg(not(feature = "ble"))]
    let _ = spawner;

    let label = if cfg!(feature = "ble") { "_ble" } else { "" };
    app.run_benches(label);
    scan::run(&mut app.lcd);
    // The tearing characterisation runs before the demo: the new sender
    // takes its model of the refresh from it.
    app.characterise();
    app.start_demo();
    info!(
        "ready: demo running; debug link on UART0 at {} baud",
        debug_link::BAUD
    );
    app.run().await
}
