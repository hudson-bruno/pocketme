//! The console on the CYD board (ESP32, 320x240 ST7789 panel).
//!
//! Start-up: log and memory, the panel (set-up, drawing clock checked by
//! reading pixels back, cleared, its refresh measured for the tear-free
//! timing), the board's parts, then the console's loop as its own task.
//! See README.md for the features and the numbers.

#![no_std]
#![no_main]
// `isync` after a game's code is written (games.rs).
#![feature(asm_experimental_arch)]

#[cfg(feature = "pad")]
extern crate alloc;

use console_core::Console;
use embassy_executor::Spawner;
use esp_backtrace as _;
use esp_hal::{
    clock::CpuClock,
    dma_tx_buffer,
    gpio::{DriveMode, Flex, InputConfig, Level, Output, OutputConfig, Pull},
    spi::master::{Config as SpiConfig, Spi},
    timer::timg::TimerGroup,
    uart::{Config as UartConfig, Uart},
};
use log::{info, warn};
use static_cell::{ConstStaticCell, StaticCell};

mod app;
mod backlight;
mod beam;
#[cfg(feature = "wide-strips")]
mod bench;
mod board;
#[cfg(feature = "core2")]
mod core2;
mod games;
// With `sd-card` also on, the real card wins and the demo card is left out.
#[cfg(all(feature = "demo-card", not(feature = "sd-card")))]
mod demo_card;
#[cfg(feature = "store")]
mod flash_store;
#[cfg(feature = "pad")]
mod hci;
mod lcd;
#[cfg(feature = "link")]
mod link;
mod logbuf;
mod mem;
#[cfg(feature = "pad")]
mod pad_radio;
#[cfg(any(feature = "store", feature = "pad"))]
mod pairing;
mod screen;
#[cfg(feature = "sd-card")]
mod sd;
mod stats;
#[cfg(feature = "touch")]
mod touch;
#[cfg(feature = "tour")]
mod tour;

use crate::board::{CardFiles, CydBoard, CydClock, CydKeys, CydTouch, TimedFiles};
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

/// The gamepad host (about 19.7 KB) on the heap, built in place, and
/// kept for good (pad-host's notes: never on a small stack, never moved).
/// On the heap, not in a static: the radio's 64 KB reserve leaves data
/// RAM short (a static of this size left the main stack at 18 KB and
/// did not fit at all with every feature on), while the heap, the
/// radio's own memory, had 30 KB free with the radio up.
#[cfg(feature = "pad")]
fn new_pad_host() -> &'static mut pad_host::PadHost {
    /// The new host's bytes, in flash (read only data): copied straight
    /// to the heap. (`Box::write(.., PadHost::new())` built it on the main
    /// stack first: 19.6 KB deep, measured.)
    static FRESH: pad_host::PadHost = pad_host::PadHost::new();
    let free = esp_alloc::HEAP.free();
    let mut b = alloc::boxed::Box::<pad_host::PadHost>::new_uninit();
    // SAFETY: FRESH is a whole, valid PadHost, and `b` has room for one,
    // apart from it. The copy is never dropped nor used as FRESH: FRESH
    // is only ever read here, and a PadHost owns nothing outside its own
    // bytes (no heap, no pointer into itself: pad-host is no_std without
    // alloc, and `new` is const), so the copy is a new host just like
    // `PadHost::new()`.
    let b = unsafe {
        core::ptr::copy_nonoverlapping(&raw const FRESH, b.as_mut_ptr(), 1);
        b.assume_init()
    };
    info!(
        "pad: host on the heap: {} bytes (heap free before {free}, after {})",
        core::mem::size_of::<pad_host::PadHost>(),
        esp_alloc::HEAP.free()
    );
    alloc::boxed::Box::leak(b)
}

/// A refused start-up survey, for the log: the reason, the survey's own
/// lines and line time (0 when it has none; NOT the defaults the
/// `panel:` line shows after a refusal), its first odd step, and what
/// happens next (`then`). Start-up only, not a timed path.
fn log_refused(survey: &beam::Survey, why: beam::Refused, then: &str) {
    let (lines, line_us) = match (survey.period_cycles(), survey.line_cycles()) {
        (Some(period), Some(line)) => (period / line, line / lcd::CPU_MHZ as f32),
        _ => (0.0, 0.0),
    };
    warn!(
        "panel: survey not used: {} (lines={:.2} line_us={:.3} odd_steps={} first_odd(from,to,us)={:?} wraps={} samples={}), {}",
        why.as_str(),
        lines,
        line_us,
        survey.odd_steps,
        survey
            .first_odd
            .map(|(from, to, c)| (from, to, c / lcd::CPU_MHZ)),
        survey.wraps,
        survey.samples,
        then,
    );
}

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    crate::logbuf::init();
    mem::paint_stack();
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: mem::HEAP_SIZE);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    info!("pocketme-cyd {}: {FEATURES}", env!("CARGO_PKG_VERSION"));

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

    // The saved gamepad pairings, read only, before anything else uses
    // the flash: handed to the gamepad host, and the saved pads'
    // addresses to the radio's gate. (The pairing area's CRC-32 is logged
    // by the store's start below, or here without the store.)
    #[cfg(feature = "pad")]
    let mut flash = peripherals.FLASH;
    #[cfg(feature = "pad")]
    let (pads, gate) = {
        let pads = new_pad_host();
        let mut fs = esp_storage::FlashStorage::new(flash.reborrow());
        #[cfg(not(feature = "store"))]
        crate::pairing::log_crc(&mut fs);
        let gate = crate::pairing::load(&mut fs, pads, &mut pad_radio::LogOnly);
        (pads, gate)
    };
    #[cfg(feature = "pair")]
    let (pairing_area, pairing_ok, pair_slots) = {
        let mut fs = esp_storage::FlashStorage::new(flash.reborrow());
        let area = crate::pairing::attach_meta(&mut fs);
        let (ok, slots) = area.status();
        (area, ok, slots)
    };
    #[cfg(not(feature = "pad"))]
    let flash = peripherals.FLASH;

    // The settings, before the radio and before the panel lights: the
    // pairing area's CRC-32 to the log, the brightness read, and the
    // spare sector erased now if it will be needed soon (an erase turns
    // interrupts off for tens of milliseconds).
    #[cfg(feature = "store")]
    let (store, saver) = flash_store::start(flash);
    #[cfg(not(feature = "store"))]
    let store = board_api::Absent;
    #[cfg(not(feature = "store"))]
    let _ = flash;
    crate::logbuf::flush(&mut uart);

    // The gamepad radio, before the panel, so that everything after it
    // (the panel's timing survey too) runs with the radio on. pad-host's
    // set-up commands go out now; the rest runs from the console's loop.
    #[cfg(feature = "pad")]
    let radio = {
        #[allow(unused_mut)]
        let mut radio = pad_radio::PadRadio::start(peripherals.BT, gate, pads);
        #[cfg(feature = "pair")]
        radio.set_pairing_area(pairing_area);
        mem::report("radio");
        crate::logbuf::flush(&mut uart);
        radio
    };

    // ---- the panel ----------------------------------------------------
    // The backlight: low (off) at once, then handed to the LEDC unit at
    // duty 0. It stays off until the console's first picture.
    let mut light_pin = Flex::new(peripherals.GPIO21);
    light_pin.set_low();
    light_pin.set_output_enable(true);
    let (light_in, light_out) = light_pin.split();
    let backlight = backlight::Backlight::new(peripherals.LEDC, light_out, light_in);
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
    // A survey refused for an odd step or uneven wraps (on the PC: a
    // wrong answer of the 45h read; on the board 6 of 225 surveys, never
    // two in a row) is done again, 200 ms each, up to MAX_SURVEYS (3) in
    // all. After the last refusal, or a refusal that a new survey cannot
    // cure (Refused::retry), the defaults stay, as before. One call of
    // each in a loop, so that they are inlined as before
    // (Beam::calibrate matches a hot-block.list glob; hot-check.py must
    // pass). One Survey (160 bytes) is in use at a time.
    let mut surveys: u32 = 0;
    let (survey, calibrated) = loop {
        let survey = beam::survey(&mut screen.lcd, beam::SURVEY_MS);
        let calibrated = screen.beam.calibrate(&survey);
        surveys += 1;
        match calibrated {
            Err(why) if why.retry() && surveys < send_plan::beam::MAX_SURVEYS => {
                log_refused(&survey, why, "surveying again");
            }
            _ => break (survey, calibrated),
        }
    };
    // `none` when no read worked (the fastest read is then unknown).
    let us = |c: u32| beam::LogUs(survey.fastest_read_us().map(|_| c / lcd::CPU_MHZ));
    info!(
        "panel: write_mhz={mhz} strip_bytes={STRIP_BYTES} survey samples={} rejected={} (error={} slow={} bits={} fastest_read_us={} read_limit_us={} spread={:?} odd_steps={}) lines={:.2} line_us={:.3} calibrated={} surveys={surveys}",
        survey.samples,
        survey.rejected,
        survey.rejected_error,
        survey.rejected_slow,
        survey.rejected_bits,
        us(survey.inner_min),
        us(survey.inner_limit()),
        survey.spread,
        survey.odd_steps,
        screen.beam.lines,
        screen.beam.line_cycles / lcd::CPU_MHZ as f32,
        calibrated.is_ok(),
    );
    if let Err(why) = calibrated {
        // The model keeps its defaults (read limit 25 us): with slow
        // reads every anchor then misses and the pictures go out untimed.
        // The lines= and line_us= here are the refused survey's own, not
        // the defaults the panel: line above shows.
        log_refused(
            &survey,
            why,
            if why.retry() {
                "keeping the defaults (last survey)"
            } else {
                "keeping the defaults (a new survey would not cure this)"
            },
        );
    }

    // Feature `core2`: the second core starts now and owns the screen
    // from here on: every timed send (anchor reads too) runs there. The
    // panel's set-up and survey above ran here, and so did the radio's
    // start (its task and interrupts are on this core).
    #[cfg(feature = "core2")]
    let screen = {
        let (screen, us) = core2::start(peripherals.CPU_CTRL, peripherals.FROM_CPU_INTR1, screen);
        info!(
            "core2: second core started in {us} us; it sends the pictures (stack {} bytes)",
            core2::STACK_SIZE
        );
        screen
    };

    // ---- the other parts ------------------------------------------------
    // BOOT: input with pull-up; its output (open drain, so it can only
    // pull low, like the button) stays off except for the measuring
    // letter `k`.
    let mut boot = Flex::new(peripherals.GPIO0);
    boot.apply_input_config(&InputConfig::default().with_pull(Pull::Up));
    boot.set_input_enable(true);
    boot.apply_output_config(&OutputConfig::default().with_drive_mode(DriveMode::OpenDrain));
    boot.set_high();
    boot.set_output_enable(false);
    #[allow(unused_mut)]
    let mut touch = CydTouch::new();
    #[cfg(feature = "touch")]
    {
        let mut chip = touch::Xpt2046::new(
            Output::new(peripherals.GPIO25, Level::Low, OutputConfig::default()),
            Output::new(peripherals.GPIO32, Level::Low, OutputConfig::default()),
            esp_hal::gpio::Input::new(peripherals.GPIO39, InputConfig::default()),
            Output::new(peripherals.GPIO33, Level::High, OutputConfig::default()),
            esp_hal::gpio::Input::new(peripherals.GPIO36, InputConfig::default()),
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
        pad: board::CydPad {
            #[cfg(feature = "pad")]
            host: Some(pads),
            #[cfg(feature = "pair")]
            pairing_ok,
            #[cfg(feature = "pair")]
            pair_slots,
            ..Default::default()
        },
        clock: CydClock,
        files: TimedFiles::new(card),
        link: board_api::Absent,
        log: crate::logbuf::CydLog,
        store,
        games: games::GameSlots::check(),
    };
    let console = CONSOLE.take();
    // The saved brightness, before the first step (and so before the
    // first picture lights the panel).
    #[cfg(feature = "store")]
    console.set_brightness(saver.settings().brightness);
    // The saved boot animation (FULL when none), before the first step:
    // the boot that starts now plays it.
    #[cfg(all(feature = "store", feature = "boot-gather"))]
    {
        let anim = console_core::boot::Anim::from_byte(saver.settings().boot_anim());
        console.set_boot_anim(anim);
        info!("settings: boot anim {}", anim.word());
    }
    #[cfg(feature = "pair")]
    console.set_settings_entry(console_core::settings::Entry::Tiles);
    info!(
        "console: {} bytes in a static at {:#x}",
        core::mem::size_of::<Console>(),
        console as *const Console as usize
    );
    // The backlight stays off: the console lights it after its first
    // picture (board-api `Screen::backlight`).
    let app = APP.init(app::App::new(console, board, uart));
    #[cfg(feature = "store")]
    {
        app.saver = Some(saver);
    }
    #[cfg(feature = "pad")]
    {
        app.radio = Some(radio);
    }
    mem::report("start");
    // Start-up time: from early in main (the clock starts there) to the
    // console's loop; the bootloader before it is not counted.
    info!(
        "start: loop begins {} ms after the clock started (early in main)",
        crate::board::now_us() / 1000
    );
    crate::logbuf::flush(&mut app.uart);

    spawner.spawn(app::run(app).unwrap());
    loop {
        embassy_time::Timer::after(embassy_time::Duration::from_secs(3600)).await;
    }
}

// Measuring only: the flash code probe (README "Flash code probe").
#[cfg(feature = "xip-probe")]
mod xip_probe;
