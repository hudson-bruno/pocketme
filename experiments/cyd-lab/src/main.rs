//! The console on the CYD board (ESP32, 320x240 ST7789 panel).
//!
//! Start-up: log and memory, the panel (set-up, drawing clock checked by
//! reading pixels back, cleared, its refresh measured for the tear-free
//! timing), the board's parts, then the console's loop as its own task.
//! See README.md for the features and the numbers.

#![no_std]
#![no_main]

use console_core::Console;
use embassy_executor::Spawner;
use esp_backtrace as _;
use esp_hal::{
    clock::CpuClock,
    dma_tx_buffer,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    spi::master::{Config as SpiConfig, Spi},
    timer::timg::TimerGroup,
    uart::{Config as UartConfig, Uart},
};
use log::{info, warn};
use static_cell::{ConstStaticCell, StaticCell};

mod app;
mod beam;
#[cfg(feature = "wide-strips")]
mod bench;
mod board;
#[cfg(feature = "core2")]
mod core2;
#[cfg(feature = "cpu1-cache-ram")]
mod cpu1_cache;
#[cfg(feature = "cpu1-start-test")]
mod cpu1_start_test;
#[cfg(feature = "fs-bench")]
mod fs_bench;
mod standin;

// The second core's cache RAM holds code (cpu1-cache-ram) only while the
// second core stays off: the two cannot be in one build.
#[cfg(all(feature = "core2", feature = "cpu1-cache-ram"))]
compile_error!(
    "`core2` and `cpu1-cache-ram` exclude each other: starting the second core turns that RAM back into its cache"
);
// With `sd-card` also on, the real card wins and the demo card is left out.
#[cfg(all(feature = "demo-card", not(feature = "sd-card")))]
mod demo_card;
mod lcd;
#[cfg(feature = "link")]
mod link;
mod logbuf;
mod mem;
#[cfg(feature = "radio-fit")]
mod radio_fit;
mod screen;
#[cfg(feature = "sd-card")]
mod sd;
mod stats;
#[cfg(feature = "touch")]
mod touch;
#[cfg(feature = "tour")]
mod tour;

use crate::board::{CablePad, CardFiles, CydBoard, CydClock, CydKeys, CydTouch, TimedFiles};
use crate::lcd::{Lcd, STRIP_BYTES};
use crate::screen::CydScreen;

esp_bootloader_esp_idf::esp_app_desc!();

/// The features this build has, for the log and `boardctl stats`.
/// (Put together by build.rs.)
pub const FEATURES: &str = env!("CYD_FEATURES");

/// The drawing clock tried first, and the one used if it fails the
/// read-back check.
const WRITE_MHZ: u32 = 80;
const FALLBACK_MHZ: u32 = 40;

/// The console: about 40 KB, so in a static, never on the stack.
static CONSOLE: ConstStaticCell<Console> = ConstStaticCell::new(Console::new());
static APP: StaticCell<app::App> = StaticCell::new();

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    crate::logbuf::init();
    // Lab: before anything else, copy the code into the second core's
    // cache RAM (from the game code slot, where it waits at start-up).
    #[cfg(feature = "cpu1-cache-ram")]
    cpu1_cache::start();
    mem::paint_stack();
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: mem::HEAP_SIZE);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    info!("pocketme-cyd {}: {FEATURES}", env!("CARGO_PKG_VERSION"));
    // Lab: the second core, started the esp-rtos way.
    #[cfg(feature = "core2")]
    {
        let us = core2::start(peripherals.CPU_CTRL, peripherals.FROM_CPU_INTR1);
        info!(
            "core2: second core started in {us} us, stack {} bytes",
            core2::STACK_SIZE
        );
    }

    // Log lines wait in the ring until the port is set up; nothing lost.
    let mut uart = Uart::new(
        peripherals.UART0,
        UartConfig::default().with_baudrate(debug_link::BAUD),
    )
    .unwrap()
    .with_rx(peripherals.GPIO3)
    .with_tx(peripherals.GPIO1)
    .into_async();
    crate::logbuf::flush(&mut uart);
    #[cfg(feature = "cpu1-cache-ram")]
    {
        cpu1_cache::probe();
        crate::logbuf::flush(&mut uart);
    }

    // Measuring only: the radio first, so that everything after it (the
    // panel's timing survey too) runs with the radio on.
    #[cfg(feature = "radio-fit")]
    {
        radio_fit::start(peripherals.BT);
        mem::report("radio");
        crate::logbuf::flush(&mut uart);
    }

    // ---- the panel ----------------------------------------------------
    let backlight = Output::new(peripherals.GPIO21, Level::Low, OutputConfig::default());
    // SPI2 on its own pins (needed for 80 MHz), with its DMA channel. CS
    // is a plain pin so it can stay low across several transfers.
    let spi = Spi::new(peripherals.SPI2, SpiConfig::default())
        .unwrap()
        .with_sck(peripherals.GPIO14)
        .with_mosi(peripherals.GPIO13)
        .with_miso(peripherals.GPIO12)
        .with_dma(peripherals.DMA_SPI2);
    let buf_a = dma_tx_buffer!(STRIP_BYTES).unwrap();
    let buf_b = dma_tx_buffer!(STRIP_BYTES).unwrap();
    let cs = Output::new(peripherals.GPIO15, Level::High, OutputConfig::default());
    let dc = Output::new(peripherals.GPIO2, Level::High, OutputConfig::default());
    let mut lcd = Lcd::new(spi, buf_a, buf_b, cs, dc);
    if let Err(e) = lcd.init() {
        warn!("panel set-up failed: {e:?}");
    }
    let mhz = if screen::check_clock(&mut lcd, WRITE_MHZ) {
        WRITE_MHZ
    } else {
        warn!("panel: {WRITE_MHZ} MHz failed the read-back check, using {FALLBACK_MHZ}");
        let _ = lcd.set_write_mhz(FALLBACK_MHZ);
        FALLBACK_MHZ
    };
    let _ = screen::clear(&mut lcd, 0);
    let mut screen = CydScreen::new(lcd, backlight);
    let survey = beam::survey(&mut screen.lcd, beam::SURVEY_MS);
    let calibrated = screen.beam.calibrate(&survey);
    info!(
        "panel: write_mhz={mhz} strip_bytes={STRIP_BYTES} survey samples={} rejected={} (error={} slow={} bits={} fastest_read_us={}) lines={:.2} line_us={:.3} calibrated={calibrated}",
        survey.samples,
        survey.rejected,
        survey.rejected_error,
        survey.rejected_slow,
        survey.rejected_bits,
        survey.inner_min / lcd::CPU_MHZ,
        screen.beam.lines,
        screen.beam.line_cycles / lcd::CPU_MHZ as f32,
    );

    // ---- the other parts ------------------------------------------------
    let boot = Input::new(
        peripherals.GPIO0,
        InputConfig::default().with_pull(Pull::Up),
    );
    #[allow(unused_mut)]
    let mut touch = CydTouch::new();
    #[cfg(feature = "touch")]
    {
        let mut chip = touch::Xpt2046::new(
            Output::new(peripherals.GPIO25, Level::Low, OutputConfig::default()),
            Output::new(peripherals.GPIO32, Level::Low, OutputConfig::default()),
            Input::new(peripherals.GPIO39, InputConfig::default()),
            Output::new(peripherals.GPIO33, Level::High, OutputConfig::default()),
            Input::new(peripherals.GPIO36, InputConfig::default()),
        );
        let [x, y, z1, z2] = chip.raw_all();
        info!(
            "touch: chip check (untouched) raw x={x} y={y} z1={z1} z2={z2} irq_low={}",
            chip.pressed()
        );
        touch.chip = Some(chip);
    }

    #[cfg(feature = "sd-card")]
    let card: CardFiles = {
        let bus = Spi::new(
            peripherals.SPI3,
            SpiConfig::default().with_frequency(esp_hal::time::Rate::from_khz(400)),
        )
        .unwrap()
        .with_sck(peripherals.GPIO18)
        .with_mosi(peripherals.GPIO23)
        .with_miso(peripherals.GPIO19);
        let cs = Output::new(peripherals.GPIO5, Level::High, OutputConfig::default());
        sd::SdFiles::new(bus, cs)
    };
    #[cfg(all(feature = "demo-card", not(feature = "sd-card")))]
    let card: CardFiles = demo_card::DemoCard;
    #[cfg(not(any(feature = "demo-card", feature = "sd-card")))]
    let card: CardFiles = board_api::Absent;

    let board = CydBoard {
        screen,
        touch,
        keys: CydKeys::new(boot),
        pad: CablePad::default(),
        clock: CydClock,
        files: TimedFiles::new(card),
        link: board_api::Absent,
        log: crate::logbuf::CydLog,
        store: board_api::Absent,
    };
    let console = CONSOLE.take();
    info!(
        "console: {} bytes in a static at {:#x}",
        core::mem::size_of::<Console>(),
        console as *const Console as usize
    );
    let app = APP.init(app::App::new(console, board, uart));
    #[cfg(feature = "cpu1-start-test")]
    app.start_test
        .give(peripherals.CPU_CTRL, peripherals.FROM_CPU_INTR1);
    board_api::Screen::backlight(&mut app.board.screen, 255);
    mem::report("start");
    crate::logbuf::flush(&mut app.uart);

    spawner.spawn(app::run(app).unwrap());
    loop {
        embassy_time::Timer::after(embassy_time::Duration::from_secs(3600)).await;
    }
}
