//! Experiment: find a classic Bluetooth gamepad, pair with it, read what
//! it says about itself, and log every input report, from bare-metal Rust
//! on the ESP32 (the CYD board).
//!
//! The Bluetooth host is the `pad-proto` crate (crates/pad-proto), a state
//! machine with no clock and no radio of its own. This file only moves
//! bytes: packets from the radio's controller into the host, packets from
//! the host to the controller, a tick every 20 ms, log lines to the serial
//! port, and screen events to the screen hook (src/ui.rs), which hands
//! them to pad-view's `View` (crates/pad-view).
//!
//! Two loops share the one task and take turns: the radio loop (here) and
//! the screen loop (src/screen.rs: drawing, the BOOT button, the debug
//! link). Neither waits for the other while holding anything.
//!
//! The radio driver is the patched esp-radio of bt-classic-probe, started
//! in dual mode (classic and Low Energy).
//!
//! Optional board tests, set at build time (never kept in a file):
//! - `PAD_TEST_PAGE=AA:BB:CC:DD:EE:FF`: after two search rounds, call this
//!   made-up address once. It must fail with a page timeout and the
//!   search must go on.
//! - `PAD_PEER=AA:BB:CC:DD:EE:FF`: after that, open an SDP-only link to
//!   this device (a phone of your own): no pairing, one Service Search
//!   Attribute Request for UUID 0x0100 in small pieces, then close. The
//!   address is replaced by `<peer>` in every log line.
//!
//! Cargo feature `demo`: made-up gamepad data for the screen (src/demo.rs).
//! Cargo feature `store-selftest`: once per start, while the radio
//! searches, save a made-up pairing for a made-up address (src/pairings.rs).
//!
//! Pairings are saved in flash (src/pairings.rs, crates/pad-store), so a
//! gamepad paired once connects again after a restart.

#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

extern crate alloc;

use core::fmt;

use bt_hci::transport::Transport as _;
use debug_link::frame::MAX_FRAME;
use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Timer};
use esp_backtrace as _;
use esp_hal::{
    clock::CpuClock,
    delay::Delay,
    dma_tx_buffer,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    spi::master::{Config as SpiConfig, Spi},
    timer::timg::TimerGroup,
    uart::{Config as UartConfig, Uart},
};
use esp_radio::ble::controller::BleConnector;
use heapless::{Deque, Vec};
use pad_proto::hci::BdAddr;
use pad_proto::{Config, Host, Io, KeyChange, Purpose, Tag, UiEvent};
use pad_view::startup::Channel;
use static_cell::{ConstStaticCell, StaticCell};

#[cfg(feature = "demo")]
mod demo;
mod display;
mod lcd;
mod mem;
mod out;
mod pairings;
mod screen;
mod ui;

use lcd::{STRIP_BYTES, W};
use screen::Screen;
use ui::{Shared, UiSink, ViewSink};

esp_bootloader_esp_idf::esp_app_desc!();

/// The same heap the probe gives the radio, so the figures compare.
const HEAP_SIZE: usize = 98768;
/// Room for the largest packet from the controller (1021 data bytes plus
/// the header), with margin.
const RX_LEN: usize = 1100;
const TICK: Duration = Duration::from_millis(20);
const MEM_EVERY: Duration = Duration::from_secs(5);
const STATS_EVERY: Duration = Duration::from_secs(10);
/// Most output reports taken from the screen hook in a row.
const OUTPUTS_IN_A_ROW: usize = 4;
/// Search rounds before the first board test, and between tests.
const ROUNDS_BEFORE_TEST: u32 = 2;

const PEER_UUID: u16 = 0x0100;
/// Small pieces, so the peer's answer needs several continuation rounds.
const PEER_MAX_BYTES: u16 = 64;

unsafe extern "C" {
    // In Espressif's controller library: 1 = Low Energy only,
    // 2 = classic only, 3 = dual.
    fn btdm_controller_get_mode() -> u32;
}

macro_rules! say {
    ($tag:literal, $($a:tt)*) => {
        out::line($tag, format_args!($($a)*))
    };
}

/// The host's way out, on the board.
struct FwIo<S: UiSink> {
    /// Packets for the controller, sent by the main loop.
    tx: Deque<Vec<u8, 259>, 24>,
    tx_dropped: u32,
    sink: S,
    /// A Device, Descriptor, Report or Control event happened: ask the
    /// screen hook for output reports.
    want_output: bool,
    /// Pairing changes from the host, applied to the flash store by the
    /// radio loop right after the host call that made them.
    changes: Deque<KeyChange, 8>,
}

impl<S: UiSink> FwIo<S> {
    fn new(sink: S) -> Self {
        Self {
            tx: Deque::new(),
            tx_dropped: 0,
            sink,
            want_output: false,
            changes: Deque::new(),
        }
    }
}

impl<S: UiSink> Io for FwIo<S> {
    fn send(&mut self, packet: &[u8]) {
        let Ok(v) = Vec::from_slice(packet) else {
            self.tx_dropped += 1;
            return;
        };
        if self.tx.push_back(v).is_err() {
            self.tx_dropped += 1;
        }
    }

    fn ui(&mut self, event: UiEvent<'_>) {
        if matches!(
            event,
            UiEvent::Device(_) | UiEvent::Descriptor(_) | UiEvent::Report(_) | UiEvent::Control(_)
        ) {
            self.want_output = true;
        }
        self.sink.event(&event);
    }

    fn log(&mut self, tag: Tag, args: fmt::Arguments<'_>) {
        out::line(tag.name(), args);
        // The host's summary knows the radio side; the screen side (layout,
        // mode guess, Switch start-up) follows on a line of its own.
        if tag == Tag::Summary {
            self.sink.summary();
        }
    }

    fn key_change(&mut self, change: KeyChange) {
        if self.changes.push_back(change).is_err() {
            // Eight changes from one packet cannot happen; say so if it does.
            say!("PAD-ERR", "pairing change queue full: one change not saved");
        }
    }
}

fn ms() -> u64 {
    Instant::now().as_millis()
}

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    mem::paint_stack();
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    // Memory for the radio driver. The host itself allocates nothing.
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: HEAP_SIZE);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    // UART0 for the debug link (requests from the PC, screenshots). The
    // log goes out on the same wire through esp-println. Setting the port
    // up throws away what is still waiting to go out, so it comes first.
    Delay::new().delay_millis(20);
    let uart = Uart::new(
        peripherals.UART0,
        UartConfig::default().with_baudrate(debug_link::BAUD),
    )
    .expect("UART0 settings")
    .with_rx(peripherals.GPIO3)
    .with_tx(peripherals.GPIO1)
    .into_async();

    let test_page = option_env!("PAD_TEST_PAGE").and_then(BdAddr::parse);
    let peer = option_env!("PAD_PEER").and_then(BdAddr::parse);
    // Optional allow-list, set when building: PAD_ONLY="name piece,AA:BB:..."
    // Only gamepads that match are called. Empty or unset: any gamepad.
    let only = option_env!("PAD_ONLY").filter(|s| !s.trim().is_empty());
    if let Some(p) = peer {
        out::hide(p);
    }
    spawner.spawn(out::printer().expect("printer task"));

    esp_println::println!();
    say!(
        "PAD-STATE",
        "boot bt-gamepad-test {} page_test={} peer_sdp_test={} demo={} allow_list={}",
        env!("CARGO_PKG_VERSION"),
        u8::from(test_page.is_some()),
        u8::from(peer.is_some()),
        u8::from(cfg!(feature = "demo")),
        u8::from(only.is_some())
    );

    // The screen: SPI2 with its DMA channel, CS as a plain pin.
    let mut backlight = Output::new(peripherals.GPIO21, Level::Low, OutputConfig::default());
    let spi = Spi::new(peripherals.SPI2, SpiConfig::default())
        .expect("SPI2 settings")
        .with_sck(peripherals.GPIO14)
        .with_mosi(peripherals.GPIO13)
        .with_miso(peripherals.GPIO12)
        .with_dma(peripherals.DMA_SPI2);
    let buf_a = dma_tx_buffer!(STRIP_BYTES).expect("strip buffer A");
    let buf_b = dma_tx_buffer!(STRIP_BYTES).expect("strip buffer B");
    let cs = Output::new(peripherals.GPIO15, Level::High, OutputConfig::default());
    let dc = Output::new(peripherals.GPIO2, Level::High, OutputConfig::default());
    let mut lcd = lcd::Lcd::new(spi, buf_a, buf_b, cs, dc);
    if let Err(e) = lcd.init() {
        say!("PAD-ERR", "screen set-up failed: {:?}", e);
    }
    // BOOT button: GPIO0, low while pressed (the board has a pull-up; the
    // chip's own is added).
    let boot = Input::new(
        peripherals.GPIO0,
        InputConfig::default().with_pull(Pull::Up),
    );

    static SHARED: ConstStaticCell<Shared> = ConstStaticCell::new(Shared::new());
    static FRAME: ConstStaticCell<[u8; MAX_FRAME]> = ConstStaticCell::new([0; MAX_FRAME]);
    static ROW: ConstStaticCell<[u8; W * 2]> = ConstStaticCell::new([0; W * 2]);
    static SCRATCH: ConstStaticCell<[u8; 1 + W * 3]> = ConstStaticCell::new([0; 1 + W * 3]);
    let shared: &'static Shared = SHARED.take();
    let mut screen = Screen::new(
        display::Display::new(lcd),
        uart,
        boot,
        FRAME.take(),
        ROW.take(),
        SCRATCH.take(),
    );

    // The first picture, before the radio starts: a whole screen.
    shared.view.borrow_mut().status("Starting radio");
    screen.draw(shared);
    backlight.set_high();
    say!(
        "PAD-UI",
        "screen up: first draw {} us, {} draw errors, cpu_bench_us={}",
        screen.stats.full_us_max.max(screen.stats.small_us_max),
        screen.stats.errors + screen.display.errors,
        screen::cpu_bench_us()
    );
    report_mem(0, 0);

    // Saved pairings: found and opened before the radio starts, because
    // opening may erase a flash sector (about 45 ms with every interrupt
    // off, the radio's included).
    let mut pairings = pairings::Pairings::open(peripherals.FLASH);
    // stack_peak: the most stack used so far (opening the store included),
    // measured before the radio starts. Compare it with the MEM line at t=0.
    let (stack_peak, _) = mem::stack();
    say!(
        "PAD-STORE",
        "ram_bytes={} (the store's state in RAM) stack_peak_after_open={:?}",
        core::mem::size_of::<pairings::Pairings>(),
        stack_peak
    );

    let before = mem::heap();
    let started = Instant::now();
    let conn =
        BleConnector::new(peripherals.BT, Default::default()).expect("radio settings refused");
    let after = mem::heap();
    // SAFETY: a plain getter in the controller library, no arguments.
    let mode = unsafe { btdm_controller_get_mode() };
    say!(
        "PAD-STATE",
        "radio-up ms={} controller_mode={} ({}) heap_free_before={} heap_free_after={} radio_heap={}",
        started.elapsed().as_millis(),
        mode,
        match mode {
            1 => "le-only",
            2 => "classic-only",
            3 => "dual",
            _ => "?",
        },
        before.free,
        after.free,
        before.free - after.free
    );
    if mode != 3 {
        say!(
            "PAD-ERR",
            "the controller is not in dual mode; classic Bluetooth will not work"
        );
    }

    static RX: ConstStaticCell<[u8; RX_LEN]> = ConstStaticCell::new([0; RX_LEN]);
    static HOST: StaticCell<Host> = StaticCell::new();
    static IO: StaticCell<FwIo<ViewSink>> = StaticCell::new();
    let rx = RX.take();
    let host = HOST.init_with(|| {
        Host::new(Config {
            only,
            ..Config::default()
        })
    });
    let io = IO.init_with(|| FwIo::new(ViewSink::new(shared)));
    pairings.load_into(host);
    show_pairings(shared, &pairings, host);

    let mut tests: Deque<(BdAddr, Purpose), 2> = Deque::new();
    if let Some(a) = test_page {
        let _ = tests.push_back((a, Purpose::Gamepad));
    }
    if let Some(a) = peer {
        let _ = tests.push_back((
            a,
            Purpose::SdpProbe {
                uuid: PEER_UUID,
                max_bytes: PEER_MAX_BYTES,
            },
        ));
    }

    let radio = radio_loop(conn, host, io, rx, shared, tests, pairings);
    match select(radio, screen.run(shared)).await {
        Either::First(never) | Either::Second(never) => never,
    }
}

fn us() -> u64 {
    Instant::now().as_micros()
}

/// The radio loop: feeds the host with packets and ticks, sends what it
/// queues, and notes how long it had to wait for its turn.
async fn radio_loop(
    mut conn: BleConnector<'static>,
    host: &mut Host,
    io: &mut FwIo<ViewSink>,
    rx: &mut [u8; RX_LEN],
    shared: &Shared,
    mut tests: Deque<(BdAddr, Purpose), 2>,
    mut pairings: pairings::Pairings,
) -> ! {
    let mut next_test_round = ROUNDS_BEFORE_TEST;
    #[cfg(feature = "store-selftest")]
    let mut selftest_done = false;
    #[cfg(feature = "demo")]
    let mut demo = demo::Demo::new();

    host.start(ms(), io);
    let mut read_errors = 0_u32;
    let mut next_tick = Instant::now() + TICK;
    let mut next_mem = Instant::now() + MEM_EVERY;
    let mut next_stats = Instant::now() + STATS_EVERY;
    let mut last_turn = us();
    let mut shots_seen = shared.shots.get();
    loop {
        // Output reports from the screen hook, then everything queued.
        // Only once both HID channels are open: before that the host
        // cannot send them, and a Switch-mode start-up packet asked for
        // too early would be counted as a try that never reached the pad.
        if io.want_output && host.hid_ready() {
            io.want_output = false;
            let mut buf = [0_u8; 64];
            for _ in 0..OUTPUTS_IN_A_ROW {
                let Some(out) = io.sink.next_output(&mut buf) else {
                    break;
                };
                let n = out.len.min(buf.len());
                // The view says the channel: a DATA Output (0xA2) on the
                // interrupt channel, a SET_REPORT Output (0x52) on the
                // control channel, or a GET_REPORT Input (0x41, the
                // battery question to an Xbox-mode pad; buf[0] is the
                // report id).
                let sent = match out.channel {
                    Channel::Interrupt => host.send_output_report(ms(), &buf[..n], io),
                    Channel::Control => host.send_set_report(ms(), &buf[..n], io),
                    Channel::GetReport => host.send_get_report(ms(), buf[0], io),
                };
                if sent {
                    io.sink.output_sent();
                } else {
                    // It did not go out: give it back, so it is asked
                    // for again later and not counted as a try.
                    io.sink.output_not_sent();
                    break;
                }
            }
        }
        apply_changes(io, &mut pairings, host, shared);
        flush(&mut conn, io).await;

        // "Forget all pairings": serial key "f" or BOOT held 5 s.
        if shared.forget_all_asked.take() {
            let n = host.forget_all_keys(io);
            pairings.forget_all();
            say!(
                "PAD-STORE",
                "forget all done: the host had {} pairing(s); pair the gamepad again (pairing mode)",
                n
            );
            shared
                .view
                .borrow_mut()
                .notice("All pads forgotten: pair again", 8_000);
            show_pairings(shared, &pairings, host);
            flush(&mut conn, io).await;
        }

        // Board self-test of the store: one save while the radio is in
        // the middle of a search, to see that it does not upset it.
        #[cfg(feature = "store-selftest")]
        if !selftest_done && host.stats().rounds >= 1 && host.phase_name() == "inquiry" {
            selftest_done = true;
            pairings.selftest_save(host);
            show_pairings(shared, &pairings, host);
        }

        // Board tests, one at a time, only while plainly searching.
        if host.stats().rounds >= next_test_round
            && host.searching()
            && !host.page_requested()
            && let Some((addr, purpose)) = tests.pop_front()
        {
            say!(
                "PAD-STATE",
                "board test: {}",
                if purpose == Purpose::Gamepad {
                    "call a made-up address"
                } else {
                    "SDP-only link to the peer"
                }
            );
            host.request_page(addr, purpose);
            next_test_round = host.stats().rounds + ROUNDS_BEFORE_TEST;
        }

        #[allow(unused_mut)]
        let mut wake = next_tick.min(next_mem).min(next_stats);
        #[cfg(feature = "demo")]
        {
            wake = wake.min(Instant::from_millis(demo.due()));
        }
        // The packet is copied into `rx`; only "did it parse" is kept from
        // the read, so `rx` is free again below. A packet the parser does
        // not like is still handed to the host as raw bytes.
        let got = match select(conn.read(&mut rx[..]), Timer::at(wake)).await {
            Either::First(parsed) => Some(parsed.is_ok()),
            Either::Second(()) => None,
        };

        // How long since the last turn: drawing and screenshots run in
        // between, so this is how long the radio may have waited.
        let turn = us();
        let gap = (turn - last_turn) as u32;
        last_turn = turn;
        let shots = shared.shots.get();
        if shared.shot_busy.get() || shots != shots_seen {
            shots_seen = shots;
            shared
                .gap_shot_us_max
                .set(shared.gap_shot_us_max.get().max(gap));
        } else {
            shared.gap_us_max.set(shared.gap_us_max.get().max(gap));
            shared
                .gap_us_max_all
                .set(shared.gap_us_max_all.get().max(gap));
            if gap > 50_000 {
                shared.gaps_over_50ms.set(shared.gaps_over_50ms.get() + 1);
            }
            if got.is_none() {
                let late = turn.saturating_sub(wake.as_micros()) as u32;
                shared
                    .tick_late_us_max
                    .set(shared.tick_late_us_max.get().max(late));
            }
        }

        if let Some(parsed) = got {
            if !parsed {
                read_errors += 1;
            }
            let len = packet_len(&rx[..]);
            host.on_packet(ms(), &rx[..len], io);
            // A new key is saved here, right after the event that brought
            // it (one 32-byte record, about 1 ms).
            apply_changes(io, &mut pairings, host, shared);
        }
        #[cfg(feature = "demo")]
        demo.pump(ms(), io);
        let now = Instant::now();
        if now >= next_tick {
            host.tick(ms(), io);
            apply_changes(io, &mut pairings, host, shared);
            next_tick = now + TICK;
            // A gamepad that needs a start-up conversation (Switch mode)
            // gets lost packets sent again even when it says nothing.
            if host.hid_ready() {
                io.want_output = true;
            }
            shared.phase.set(host.phase_name());
            shared.linked.set(host.linked());
        }
        if now >= next_mem {
            report_mem(io.tx_dropped, read_errors);
            next_mem += MEM_EVERY;
            if next_mem < now {
                next_mem = now + MEM_EVERY;
            }
        }
        if now >= next_stats {
            let s = host.stats();
            say!(
                "PAD-STATE",
                "stats phase={} rounds={} found_gamepads={} found_other={} found_skipped={} links={} reports={} events={} acl_in={} acl_out={} errors={} out_dropped={}",
                host.phase_name(),
                s.rounds,
                s.found_gamepads,
                s.found_other,
                s.found_skipped,
                s.links,
                s.reports,
                s.events,
                s.acl_in,
                s.acl_out,
                s.errors,
                s.dropped_out
            );
            next_stats = now + STATS_EVERY;
        }
    }
}

/// Applies the host's pairing changes to the store and updates the
/// About page line.
fn apply_changes<S: UiSink>(
    io: &mut FwIo<S>,
    pairings: &mut pairings::Pairings,
    host: &Host,
    shared: &Shared,
) {
    if io.changes.is_empty() {
        return;
    }
    while let Some(c) = io.changes.pop_front() {
        pairings.apply(c);
    }
    show_pairings(shared, pairings, host);
}

fn show_pairings(shared: &Shared, pairings: &pairings::Pairings, host: &Host) {
    let line = pairings.about_line(host.key_count());
    shared.view.borrow_mut().pairings(&line);
}

/// Sends every queued packet to the controller.
async fn flush<S: UiSink>(conn: &mut BleConnector<'static>, io: &mut FwIo<S>) {
    while let Some(p) = io.tx.pop_front() {
        let mut sent = 0;
        while sent < p.len() {
            match conn.write_async(&p[sent..]).await {
                Ok(0) | Err(_) => {
                    say!(
                        "PAD-ERR",
                        "write to the controller failed at byte {} of {} (kind {:#04x})",
                        sent,
                        p.len(),
                        p[0]
                    );
                    break;
                }
                Ok(n) => sent += n,
            }
        }
    }
}

/// The length of the packet at the start of `p`, from its own header.
fn packet_len(p: &[u8]) -> usize {
    let len = match p.first() {
        Some(0x04) if p.len() >= 3 => 3 + usize::from(p[2]),
        Some(0x02) if p.len() >= 5 => 5 + usize::from(u16::from_le_bytes([p[3], p[4]])),
        _ => 1,
    };
    len.min(p.len())
}

/// The memory line, same fields as the probe's, plus the drop counters.
fn report_mem(tx_dropped: u32, read_errors: u32) {
    let heap = mem::heap();
    let (stack_used, stack_size) = mem::stack();
    let mut stack: heapless::String<16> = heapless::String::new();
    match stack_used {
        Some(used) => {
            let _ = fmt::Write::write_fmt(&mut stack, format_args!("{used}"));
        }
        None => {
            let _ = stack.push_str("OVER");
        }
    }
    say!(
        "MEM",
        "t={} heap_size={} heap_free={} heap_min_free={} heap_largest={} heap_requests={} heap_refused={} stack_used={} stack_size={} log_dropped={} tx_dropped={} read_errors={}",
        Instant::now().as_secs(),
        heap.size,
        heap.free,
        heap.min_free,
        heap.largest,
        heap.requests,
        heap.refused,
        stack,
        stack_size,
        out::dropped(),
        tx_dropped,
        read_errors
    );
}
