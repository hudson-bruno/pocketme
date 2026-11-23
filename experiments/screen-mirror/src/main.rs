//! Experiment: can the board report its own screen?
//!
//! Proving firmware for the debug link (`crates/debug-link`), on the CYD
//! prototype board (ESP32-2432S028). It draws test pictures with known
//! pixel values and answers the PC tool (`tools/boardctl`) over the USB
//! serial cable, the same wire the log goes out on:
//!
//! * a screenshot of the **panel's own memory**, read back from the
//!   display controller: what the glass shows;
//! * a screenshot of the **framebuffer**: what the firmware believes it
//!   drew;
//! * buttons and taps sent from the PC, as if a gamepad were connected;
//! * a few numbers about itself (stats).
//!
//! Two pictures, switched with the SELECT button:
//!
//! * **card**: `test_card::draw_panel_card`, 320x240, drawn straight to
//!   the panel through the display driver. A tap draws a small cross on
//!   it, which is then a real difference from the card.
//! * **fb**: a 160x120 framebuffer of one byte per pixel
//!   (`test_card::draw_fb_card`), shown at twice the size through a
//!   palette, as the console will do. A tap draws a cross into the
//!   framebuffer, and the bottom line shows the buttons held.
//!
//! START draws the current picture afresh.
//!
//! Everything runs in one task, one thing after the other. That is what
//! keeps a log line from landing in the middle of a frame, and a drawing
//! from landing in the middle of a screenshot.

#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use core::{cell::RefCell, fmt::Write as _};

use debug_link::{
    TextBuf,
    frame::{COMMAND_DECODER_BUF, Decoder, Event, MAX_FRAME},
    image::ImageSender,
    msg::{PixelFormat, Reply, Request, Source, button, kind, status},
};
use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Timer};
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::{Gray8, Rgb565, raw::RawU16},
    prelude::*,
    primitives::Rectangle,
    text::{Baseline, Text},
};
use embedded_hal_bus::spi::{NoDelay, RefCellDevice};
use esp_backtrace as _;
use esp_hal::{
    Async,
    clock::CpuClock,
    delay::Delay,
    gpio::{Level, Output, OutputConfig},
    spi::master::{Config as SpiConfig, Spi},
    time::Rate,
    timer::timg::TimerGroup,
    uart::{Config as UartConfig, Uart},
};
use log::{info, warn};
use mipidsi::{
    Builder, Display, NoResetPin,
    interface::SpiInterface,
    models::ST7789,
    options::{ColorInversion, ColorOrder, Orientation, Rotation},
};
use static_cell::{ConstStaticCell, StaticCell};
use test_card::{FB_H, FB_STATUS_Y, FB_W, Fb, PALETTE, PANEL_H, PANEL_W};

mod panel;
mod stack;

use panel::{Bus, PanelReader, Pin, RAW_MAX, SharedPin, cmd};

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

/// The panel as it is built: taller than wide. Turned a quarter to make
/// it 320 wide, as in the reference firmware.
const PANEL: (u16, u16) = (240, 320);
const TURNED: Rotation = Rotation::Deg90;

/// Clock for drawing, as in the reference firmware.
const WRITE_MHZ: u32 = 40;

/// Clock for reading the panel back when the PC does not name one.
/// Chosen from what was measured on the board: see the README.
const READ_KHZ: u32 = 4000;

/// How long the board waits for the PC's "go" after changing speed.
const GO_WAIT: Duration = Duration::from_millis(1500);

/// A stretch of silence after which bytes of a half-received frame are
/// given up.
const QUIET: Duration = Duration::from_millis(100);

type PanelDevice = RefCellDevice<'static, Spi<'static, esp_hal::Blocking>, SharedPin, NoDelay>;
type Panel = Display<SpiInterface<'static, PanelDevice, SharedPin>, ST7789, NoResetPin>;

/// Which picture is on the screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Card,
    Fb,
}

/// Counters for the stats report.
#[derive(Default)]
struct Counters {
    /// Bytes received on the serial port, and how many of them belonged
    /// to no frame (a GPS receiver on the same wire, noise).
    rx_bytes: u32,
    rx_junk: u32,
    rx_frames: u32,
    rx_corrupt: u32,
    /// Errors the serial port itself reported (wrong speed, overflow).
    rx_errors: u32,
    shots: u32,
    taps: u32,
    last_tap: (u16, u16),
    buttons: u16,
    /// Time the last full drawing of each kind took.
    card_us: u32,
    blit_us: u32,
    /// The last screenshot: time spent fetching pixels, and in all.
    shot_capture_ms: u32,
    shot_total_ms: u32,
    panel_id: [u8; 3],
}

struct App {
    uart: Uart<'static, Async>,
    panel: Panel,
    reader: PanelReader,
    /// The logical screen: 160x120 palette numbers.
    fb: &'static mut [u8; FB_W * FB_H],
    /// One frame being put together for sending.
    frame: &'static mut [u8; MAX_FRAME],
    /// One row of a screenshot, as it goes on the wire.
    row: &'static mut [u8; PANEL_W * 2],
    /// One row as the panel answers it: a dummy byte, 3 bytes a pixel.
    raw_row: &'static mut [u8; 1 + PANEL_W * 3],
    mode: Mode,
    counters: Counters,
}

/// A request copied out of the decoder's buffer, to be dealt with after
/// the decoder is done with the byte that completed it.
struct Pending {
    kind: u8,
    seq: u8,
    payload: [u8; COMMAND_DECODER_BUF],
    len: usize,
}

fn rgb(raw: u16) -> Rgb565 {
    Rgb565::from(RawU16::new(raw))
}

/// Sends bytes out of the serial port and returns when all are in its
/// buffer. It does not `await`: nothing else gets to run, and so no log
/// line can land inside a frame.
fn send(uart: &mut Uart<'static, Async>, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        match uart.write(bytes) {
            Ok(sent) => bytes = &bytes[sent..],
            Err(_) => return,
        }
    }
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

    /// Draws the 320x240 test card straight to the panel.
    fn draw_card(&mut self) {
        let start = Instant::now();
        if let Err(error) = test_card::draw_panel_card(&mut self.panel) {
            warn!("drawing the card failed: {error:?}");
        }
        self.counters.card_us = start.elapsed().as_micros() as u32;
        info!("drew the test card in {} us", self.counters.card_us);
    }

    /// Sends the whole framebuffer to the panel at twice its size: each
    /// pixel becomes two across, and each row is sent twice.
    fn blit(&mut self) {
        let start = Instant::now();
        let fb = &*self.fb;
        let pixels = (0..PANEL_H).flat_map(|y| {
            let row = &fb[(y / 2) * FB_W..][..FB_W];
            row.iter().flat_map(|&index| {
                let colour = rgb(PALETTE[usize::from(index)]);
                [colour, colour]
            })
        });
        if let Err(error) = self.panel.set_pixels(0, 0, PANEL_W as u16 - 1, PANEL_H as u16 - 1, pixels) {
            warn!("sending the framebuffer failed: {error:?}");
        }
        self.counters.blit_us = start.elapsed().as_micros() as u32;
    }

    /// Puts the pure test card into the framebuffer, without status line
    /// or crosses.
    fn reset_fb(&mut self) {
        // Drawing into memory cannot fail.
        let _ = test_card::draw_fb_card(&mut Fb::new(&mut self.fb[..]));
    }

    /// Shows the picture of the current mode from scratch.
    fn redraw(&mut self) {
        match self.mode {
            Mode::Card => self.draw_card(),
            Mode::Fb => {
                self.reset_fb();
                self.blit();
                info!("showed the framebuffer at 2x in {} us", self.counters.blit_us);
            }
        }
    }

    /// Writes the buttons held and the last tap into the framebuffer's
    /// bottom line.
    fn draw_status(&mut self) {
        let mut text = [0u8; 24];
        let mut line = TextBuf::new(&mut text);
        let (x, y) = self.counters.last_tap;
        let _ = write!(line, "B:{:04X} T:{},{}", self.counters.buttons, x, y);
        let mut fb = Fb::new(&mut self.fb[..]);
        let _ = fb.fill_solid(&Rectangle::new(Point::new(1, FB_STATUS_Y), Size::new(FB_W as u32 - 2, 11)), Gray8::new(0));
        let style = MonoTextStyle::new(&FONT_6X10, Gray8::new(7));
        let words = core::str::from_utf8(line.as_bytes()).unwrap_or("");
        let _ = Text::with_baseline(words, Point::new(4, FB_STATUS_Y), style, Baseline::Top).draw(&mut fb);
    }

    fn on_buttons(&mut self, mask: u16) {
        let pressed = mask & !self.counters.buttons;
        self.counters.buttons = mask;

        let mut text = [0u8; 80];
        let mut names = TextBuf::new(&mut text);
        for (name, bit) in button::NAMES {
            if mask & bit != 0 {
                let _ = write!(names, " {name}");
            }
        }
        info!("input: buttons mask=0x{:04x} held:{}", mask, core::str::from_utf8(names.as_bytes()).unwrap_or("?"));

        if pressed & button::SELECT != 0 {
            self.mode = if self.mode == Mode::Card { Mode::Fb } else { Mode::Card };
            info!("mode: {:?}", self.mode);
            self.redraw();
        } else if pressed & button::START != 0 {
            self.redraw();
        }
        if self.mode == Mode::Fb {
            self.draw_status();
            self.blit();
        }
    }

    fn on_tap(&mut self, x: u16, y: u16) {
        self.counters.taps += 1;
        self.counters.last_tap = (x, y);
        info!("input: tap x={x} y={y}");
        let (px, py) = (i32::from(x), i32::from(y));
        match self.mode {
            Mode::Card => {
                // A cross straight on the panel: from now on the screen
                // differs from the card, on purpose.
                let magenta = rgb(0xF81F);
                let _ = self.panel.fill_solid(&Rectangle::new(Point::new(px - 3, py), Size::new(7, 1)), magenta);
                let _ = self.panel.fill_solid(&Rectangle::new(Point::new(px, py - 3), Size::new(1, 7)), magenta);
            }
            Mode::Fb => {
                let orange = Gray8::new(9);
                let mut fb = Fb::new(&mut self.fb[..]);
                let _ = fb.fill_solid(&Rectangle::new(Point::new(px / 2 - 2, py / 2), Size::new(5, 1)), orange);
                let _ = fb.fill_solid(&Rectangle::new(Point::new(px / 2, py / 2 - 2), Size::new(1, 5)), orange);
                self.draw_status();
                self.blit();
            }
        }
    }

    fn send_stats(&mut self, seq: u8) {
        let mut text = [0u8; 640];
        let mut out = TextBuf::new(&mut text);
        let c = &self.counters;
        let (stack_used, stack_size) = stack::deepest();
        let _ = writeln!(out, "firmware=screen-mirror {}", env!("CARGO_PKG_VERSION"));
        let _ = writeln!(out, "uptime_ms={}", Instant::now().as_millis());
        let _ = writeln!(out, "mode={:?}", self.mode);
        let _ = writeln!(out, "heap_free={}", esp_alloc::HEAP.free());
        let _ = writeln!(out, "heap_used={}", esp_alloc::HEAP.used());
        match stack_used {
            Some(used) => {
                let _ = writeln!(out, "stack_used={used}");
            }
            None => {
                let _ = writeln!(out, "stack_used=more_than_measured");
            }
        }
        let _ = writeln!(out, "stack_size={stack_size}");
        // How long the picture now on screen took to draw.
        let _ = writeln!(out, "frame_us={}", if self.mode == Mode::Card { c.card_us } else { c.blit_us });
        let _ = writeln!(out, "card_draw_us={}", c.card_us);
        let _ = writeln!(out, "fb_blit_us={}", c.blit_us);
        let _ = writeln!(out, "shots={}", c.shots);
        let _ = writeln!(out, "shot_capture_ms={}", c.shot_capture_ms);
        let _ = writeln!(out, "shot_total_ms={}", c.shot_total_ms);
        let _ = writeln!(out, "buttons=0x{:04x}", c.buttons);
        let _ = writeln!(out, "taps={}", c.taps);
        let _ = writeln!(out, "last_tap={},{}", c.last_tap.0, c.last_tap.1);
        let _ = writeln!(out, "panel_id={:02x}{:02x}{:02x}", c.panel_id[0], c.panel_id[1], c.panel_id[2]);
        let _ = writeln!(out, "read_khz={READ_KHZ}");
        let _ = writeln!(out, "rx_bytes={}", c.rx_bytes);
        let _ = writeln!(out, "rx_junk={}", c.rx_junk);
        let _ = writeln!(out, "rx_frames={}", c.rx_frames);
        let _ = writeln!(out, "rx_corrupt={}", c.rx_corrupt);
        let _ = writeln!(out, "rx_errors={}", c.rx_errors);
        if let Ok(len) = (Reply::StatsText { text: out.as_bytes() }).encode(seq, &mut self.frame[..]) {
            send(&mut self.uart, &self.frame[..len]);
        }
    }

    fn panel_raw(&mut self, seq: u8, cmd: u8, dummy_bits: u8, read_len: u16, clock_khz: u16, params: &[u8]) {
        let khz = if clock_khz == 0 { READ_KHZ } else { u32::from(clock_khz) };
        let mut answer = [0u8; RAW_MAX];
        let answer = &mut answer[..usize::from(read_len).min(RAW_MAX)];
        match self.reader.raw(khz, cmd, params, dummy_bits, answer) {
            Ok(()) => self.reply(seq, Reply::PanelRawReply { bytes: answer }),
            Err(error) => {
                warn!("panel command {cmd:02x}h failed: {error:?}");
                self.ack(seq, kind::PANEL_RAW, status::FAILED);
            }
        }
    }

    /// Sets the serial port's speed. What is still waiting to go out is
    /// sent at the old speed first.
    fn set_baud(&mut self, baud: u32) -> bool {
        let _ = self.uart.flush();
        self.uart.apply_config(&UartConfig::default().with_baudrate(baud)).is_ok()
    }

    /// Waits for the PC's "go" with this sequence number.
    async fn wait_for_go(&mut self, seq: u8) -> bool {
        let deadline = Instant::now() + GO_WAIT;
        let mut decoder = Decoder::<COMMAND_DECODER_BUF>::new();
        let mut bytes = [0u8; 32];
        loop {
            let now = Instant::now();
            if now >= deadline {
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
        // 70 kHz is the slowest clock the SPI driver can make.
        let khz = if read_khz == 0 { READ_KHZ } else { u32::from(read_khz) };
        if (baud != 0 && !(9600..=3_000_000).contains(&baud)) || khz < 70 {
            self.ack(seq, kind::SHOT, status::BAD_REQUEST);
            return;
        }
        let started = Instant::now();
        self.ack(seq, kind::SHOT, status::OK);

        // From here to the end nothing is logged: at another speed the
        // PC would still read it, but it is simpler to reason about a
        // stretch of line that carries frames only.
        let mut went = true;
        if baud != 0 {
            went = self.set_baud(baud) && self.wait_for_go(seq).await;
        }

        let mut capture = Duration::from_ticks(0);
        let mut failed = false;
        let mut bytes_sent = 0usize;
        if went {
            let (format, width, height) = match source {
                Source::Panel => (PixelFormat::Rgb565, PANEL_W as u16, PANEL_H as u16),
                Source::Framebuffer => (PixelFormat::Indexed8, FB_W as u16, FB_H as u16),
            };
            let mut sender = ImageSender::new(seq, source, format, width, height);
            let App { uart, reader, fb, frame, row, raw_row, .. } = self;
            loop {
                let fetch = |y: u16, out: &mut [u8]| {
                    let begun = Instant::now();
                    let result = match source {
                        Source::Panel => reader.read_row(khz, 0, y, width, &mut raw_row[..], out),
                        Source::Framebuffer => {
                            out.copy_from_slice(&fb[usize::from(y) * FB_W..][..FB_W]);
                            Ok(())
                        }
                    };
                    capture += begun.elapsed();
                    result
                };
                match sender.next_frame(&mut row[..], Some(&PALETTE), fetch, &mut frame[..]) {
                    Ok(Some(len)) => {
                        send(uart, &frame[..len]);
                        bytes_sent += len;
                    }
                    Ok(None) => break,
                    Err(_) => {
                        failed = true;
                        break;
                    }
                }
                sender.capture_ms = capture.as_millis() as u32;
                // Let other tasks have a turn between frames.
                embassy_futures::yield_now().await;
            }
        }
        if failed {
            self.ack(seq, kind::SHOT, status::FAILED);
        }
        if baud != 0 {
            self.set_baud(debug_link::BAUD);
            // The PC goes back to the resting speed only once it has the
            // last frame, a little after us. A log line sent before that
            // would reach it as garbage (seen: half of the line below).
            Timer::after(Duration::from_millis(60)).await;
        }

        self.counters.shots += 1;
        self.counters.shot_capture_ms = capture.as_millis() as u32;
        self.counters.shot_total_ms = started.elapsed().as_millis() as u32;
        if !went {
            warn!("shot: no go from the PC at {baud} baud, nothing sent");
        } else if failed {
            warn!("shot: reading the panel failed");
        } else {
            info!(
                "shot: {:?} sent, {} bytes at {} baud, read clock {} kHz, fetching pixels {} ms, {} ms in all",
                source,
                bytes_sent,
                if baud == 0 { debug_link::BAUD } else { baud },
                khz,
                self.counters.shot_capture_ms,
                self.counters.shot_total_ms
            );
        }
    }

    async fn handle(&mut self, pending: &Pending) {
        let frame = debug_link::frame::Frame { kind: pending.kind, seq: pending.seq, payload: &pending.payload[..pending.len] };
        let seq = frame.seq;
        match Request::parse(&frame) {
            Ok(Request::Ping) => self.ack(seq, kind::PING, status::OK),
            Ok(Request::Stats) => self.send_stats(seq),
            Ok(Request::Buttons { mask }) => {
                // Answer first: a redraw takes a while, and the PC should
                // not think the request was lost.
                self.ack(seq, kind::BUTTONS, status::OK);
                self.on_buttons(mask);
            }
            Ok(Request::Tap { x, y }) => {
                if usize::from(x) < PANEL_W && usize::from(y) < PANEL_H {
                    self.ack(seq, kind::TAP, status::OK);
                    self.on_tap(x, y);
                } else {
                    self.ack(seq, kind::TAP, status::BAD_REQUEST);
                }
            }
            Ok(Request::Shot { source, baud, read_khz }) => self.shot(seq, source, baud, read_khz).await,
            Ok(Request::PanelRaw { cmd, dummy_bits, read_len, clock_khz, params }) => {
                self.panel_raw(seq, cmd, dummy_bits, read_len, clock_khz, params);
            }
            // A "go" outside a screenshot: late, nothing to do.
            Ok(Request::Go) => {}
            Err(debug_link::msg::ParseError::UnknownKind) => self.ack(seq, frame.kind, status::UNSUPPORTED),
            Err(debug_link::msg::ParseError::BadPayload) => self.ack(seq, frame.kind, status::BAD_REQUEST),
        }
    }

    /// Asks the panel who it is and how it is set up. If these answers
    /// make sense, reading works at all.
    fn panel_report(&mut self) {
        let mut id = [0u8; 3];
        let mut status = [0u8; 4];
        let mut one = [0u8; 1];
        match self.reader.raw(READ_KHZ, cmd::RDDID, &[], 1, &mut id) {
            Ok(()) => info!("panel: RDDID (04h) -> {id:02x?}"),
            Err(error) => warn!("panel: RDDID failed: {error:?}"),
        }
        self.counters.panel_id = id;
        match self.reader.raw(READ_KHZ, cmd::RDDST, &[], 1, &mut status) {
            Ok(()) => info!("panel: RDDST (09h) -> {status:02x?}"),
            Err(error) => warn!("panel: RDDST failed: {error:?}"),
        }
        for (name, command) in [("RDDPM (0Ah)", cmd::RDDPM), ("RDDMADCTL (0Bh)", cmd::RDDMADCTL), ("RDDCOLMOD (0Ch)", cmd::RDDCOLMOD)] {
            match self.reader.raw(READ_KHZ, command, &[], 0, &mut one) {
                Ok(()) => info!("panel: {name} -> {one:02x?}"),
                Err(error) => warn!("panel: {name} failed: {error:?}"),
            }
        }
    }

    async fn run(&mut self) -> ! {
        let mut decoder = Decoder::<COMMAND_DECODER_BUF>::new();
        let mut bytes = [0u8; 64];
        let mut pending = Pending { kind: 0, seq: 0, payload: [0; COMMAND_DECODER_BUF], len: 0 };
        loop {
            let got = match select(self.uart.read_async(&mut bytes), Timer::after(QUIET)).await {
                Either::First(Ok(got)) => got,
                Either::First(Err(_)) => {
                    // Wrong speed on the wire, or more than the buffer
                    // holds: count it and carry on.
                    self.counters.rx_errors += 1;
                    continue;
                }
                Either::Second(()) => 0,
            };
            self.counters.rx_bytes += got as u32;

            // A pause on the line gives up a half-received frame.
            let quiet = got == 0;
            let mut at = 0;
            while at < got || (quiet && decoder.mid_frame()) {
                let mut complete = false;
                let counters = &mut self.counters;
                let mut sink = |event: Event<'_>| match event {
                    // What it says is nobody's business here: it may be a
                    // GPS receiver's position. Only counted.
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
        }
    }
}

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    esp_println::logger::init_logger_from_env();
    stack::paint();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(size: 32 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    info!("screen-mirror {}: debug link experiment", env!("CARGO_PKG_VERSION"));

    // The log goes out of UART0 by itself. To also listen on it, and to
    // change its speed for screenshots, this takes the port over. Setting
    // a port up throws away what it still has waiting to go out, so the
    // line above is given time to leave first (seen on this board).
    Delay::new().delay_millis(20);
    let uart = Uart::new(peripherals.UART0, UartConfig::default().with_baudrate(debug_link::BAUD))
        .unwrap()
        .with_rx(peripherals.GPIO3)
        .with_tx(peripherals.GPIO1)
        .into_async();

    // The backlight stays off until there is a picture.
    let mut backlight = Output::new(peripherals.GPIO21, Level::Low, OutputConfig::default());

    // The panel, set up as in the reference firmware: ST7789, red and
    // blue in RGB order, no inversion, 40 MHz, software reset. The one
    // addition is MISO (GPIO12), the wire the panel answers on.
    //
    // Bus and pins sit in `RefCell`s because two things use them: the
    // display driver for drawing and `PanelReader` for reading back.
    static BUS: StaticCell<Bus> = StaticCell::new();
    static CS: StaticCell<Pin> = StaticCell::new();
    static DC: StaticCell<Pin> = StaticCell::new();
    static SPI_BUFFER: ConstStaticCell<[u8; 4096]> = ConstStaticCell::new([0; 4096]);
    let write_config = SpiConfig::default().with_frequency(Rate::from_mhz(WRITE_MHZ));
    let spi = Spi::new(peripherals.SPI2, write_config)
        .unwrap()
        .with_sck(peripherals.GPIO14)
        .with_mosi(peripherals.GPIO13)
        .with_miso(peripherals.GPIO12);
    let bus: &'static Bus = BUS.init(RefCell::new(spi));
    let cs: &'static Pin = CS.init(RefCell::new(Output::new(peripherals.GPIO15, Level::High, OutputConfig::default())));
    let dc: &'static Pin = DC.init(RefCell::new(Output::new(peripherals.GPIO2, Level::Low, OutputConfig::default())));
    let spi_device = RefCellDevice::new_no_delay(bus, SharedPin(cs)).unwrap();
    let interface = SpiInterface::new(spi_device, SharedPin(dc), SPI_BUFFER.take());
    let panel = Builder::new(ST7789, interface)
        .display_size(PANEL.0, PANEL.1)
        .color_order(ColorOrder::Rgb)
        .invert_colors(ColorInversion::Normal)
        .orientation(Orientation::new().rotate(TURNED))
        .init(&mut Delay::new())
        .unwrap();

    static FB: ConstStaticCell<[u8; FB_W * FB_H]> = ConstStaticCell::new([0; FB_W * FB_H]);
    static FRAME: ConstStaticCell<[u8; MAX_FRAME]> = ConstStaticCell::new([0; MAX_FRAME]);
    static ROW: ConstStaticCell<[u8; PANEL_W * 2]> = ConstStaticCell::new([0; PANEL_W * 2]);
    static RAW_ROW: ConstStaticCell<[u8; 1 + PANEL_W * 3]> = ConstStaticCell::new([0; 1 + PANEL_W * 3]);
    static APP: StaticCell<App> = StaticCell::new();
    let app = APP.init(App {
        uart,
        panel,
        reader: PanelReader::new(bus, cs, dc, write_config),
        fb: FB.take(),
        frame: FRAME.take(),
        row: ROW.take(),
        raw_row: RAW_ROW.take(),
        mode: Mode::Card,
        counters: Counters::default(),
    });

    app.panel_report();
    app.reset_fb();
    app.redraw();
    backlight.set_high();
    info!("ready: listening for debug-link requests on UART0 at {} baud", debug_link::BAUD);

    app.run().await
}
