//! Experiment: can the ESP32's Bluetooth controller run classic Bluetooth
//! ("BR/EDR", what the GuliKit gamepad speaks) from bare-metal Rust, and
//! what does it cost?
//!
//! The radio driver, esp-radio, starts the controller in Low Energy mode
//! only. This experiment uses a copy of esp-radio (vendor/esp-radio) with
//! a few lines changed so that the `dual` build starts it in dual mode
//! (classic and Low Energy together). See vendor/PATCH-NOTES.md.
//!
//! There is no Bluetooth library here. The firmware talks to the
//! controller in raw HCI (src/hci.rs) and prints, on the serial port, one
//! line per fact, each starting with a tag:
//!
//! * `BOOT`  start-up: build, controller mode
//! * `MEM`   once a second: heap and main stack
//! * `HCI`   every command sent and the controller's answer
//! * `INFO`  what the answers mean (version, features, buffers, ...)
//! * `LE`    the Low Energy checks
//! * `INQ`   start and end of each inquiry ("who is there?")
//! * `FOUND` a classic device that answered, first time in each round
//! * `NAME`  a device's name, or why it could not be had
//! * `CONN`  someone tried to connect to us
//! * `PAGETEST` the classic name test on Low Energy public addresses
//! * `CONNTEST` a classic connection attempt (made-up address, and the
//!   owner's device when built with `PROBE_PEER`); no address in the line
//! * `PEERTEST` the name request to the `PROBE_PEER` device
//! * `SUMMARY` counts only, no address, no name
//! * `ERROR` anything that went wrong
//!
//! Builds: no feature = Low Energy only (the unchanged driver; a control
//! run). `dual` = dual mode as ESP-IDF starts it. `dual-rom` = dual mode
//! without ESP-IDF's fixed classic functions (smaller).
//!
//! After the checks it repeats for ever: inquiry for about 10 s, names,
//! a 2 s pause. Meanwhile it stays visible to phones as a classic device
//! called "PocketMe probe".

#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

extern crate alloc;

use core::fmt::Write as _;

use embassy_executor::Spawner;
use embassy_time::{Duration, Instant};
use esp_backtrace as _;
use esp_hal::{clock::CpuClock, timer::timg::TimerGroup};
use esp_radio::ble::controller::BleConnector;
use heapless::String;
use static_cell::ConstStaticCell;

mod hci;
mod mem;

use hci::{Addr, Hci, RX_LEN, op};

// This creates a default app-descriptor required by the esp-idf bootloader.
esp_bootloader_esp_idf::esp_app_desc!();

/// The same heap ble-scan and the display firmware give the radio, so the
/// figures can be compared.
const HEAP_SIZE: usize = 98768;

/// The name phones see.
const NAME: &[u8] = b"PocketMe probe";

/// Class of device we show: major class "toy" (0x08), minor "game"
/// (0x05 << 2). Deliberately not the gamepad's class, so the two cannot be
/// mixed up in a list.
const OUR_CLASS: [u8; 3] = [0x14, 0x08, 0x00];

/// One inquiry: 8 x 1.28 s = 10.24 s.
const INQUIRY_UNITS: u8 = 8;

/// Most names asked for per round (each can take up to about 5 s).
const NAMES_PER_ROUND: usize = 6;

/// "dual", "dual-rom" or "le", for the log.
const MODE: &str = if cfg!(feature = "dual") {
    "dual"
} else if cfg!(feature = "dual-rom") {
    "dual-rom"
} else {
    "le"
};

/// Prints one line, 64 bytes at a time, and after each piece waits as long
/// as it takes to leave the serial port (87 microseconds a byte at 115200
/// baud). Same as in ble-scan: esp-println writes with interrupts off, and
/// a long line would keep them off, the radio's with them, for
/// milliseconds.
#[macro_export]
macro_rules! say {
    ($($arg:tt)*) => {{
        let mut line: heapless::String<400> = heapless::String::new();
        // A line that does not fit is cut short, which is fine for a log.
        let _ = core::fmt::Write::write_fmt(&mut line, format_args!($($arg)*));
        let _ = line.push('\n');
        for piece in line.as_bytes().chunks(64) {
            esp_println::Printer::write_bytes(piece);
            embassy_time::Timer::after(embassy_time::Duration::from_micros(piece.len() as u64 * 90 + 100)).await;
        }
    }};
}

unsafe extern "C" {
    // In Espressif's controller library: the mode the controller was
    // started in. 1 = Low Energy only, 2 = classic only, 3 = dual.
    fn btdm_controller_get_mode() -> u32;
}

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    // First of all, so that the start-up itself is measured too.
    mem::paint_stack();

    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    // Memory for the radio driver. Our own code allocates nothing.
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: HEAP_SIZE);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    esp_println::println!();
    say!("BOOT bt-classic-probe {} mode={} inquiry_units={} names_per_round={}", env!("CARGO_PKG_VERSION"), MODE, INQUIRY_UNITS, NAMES_PER_ROUND);
    report_mem().await;

    // This is where the radio is started. A radio that does not come up
    // ends in a panic (or an assert of the controller) inside this call.
    let before = mem::heap();
    let started = Instant::now();
    let conn = BleConnector::new(peripherals.BT, Default::default()).expect("radio settings refused");
    let after = mem::heap();
    // SAFETY: a plain getter in the controller library, no arguments.
    let controller_mode = unsafe { btdm_controller_get_mode() };
    say!(
        "BOOT radio-up ms={} controller_mode={} ({}) heap_free_before={} heap_free_after={} radio_heap={} heap_min_free={} heap_largest={}",
        started.elapsed().as_millis(),
        controller_mode,
        match controller_mode {
            1 => "le-only",
            2 => "classic-only",
            3 => "dual",
            _ => "?",
        },
        before.free,
        after.free,
        before.free - after.free,
        after.min_free,
        after.largest
    );
    report_mem().await;

    static RX: ConstStaticCell<[u8; RX_LEN]> = ConstStaticCell::new([0; RX_LEN]);
    let mut hci = Hci::new(conn, RX.take());

    bring_up(&mut hci).await;
    le_check(&mut hci).await;
    hci.page_test().await;

    // ---- for ever: inquiry, names, pause; visible all the time.
    loop {
        let status = hci.inquiry(INQUIRY_UNITS).await;
        if status == Some(0) {
            hci.names(NAMES_PER_ROUND).await;
        }
        report_round(&hci).await;
        hci.idle(Duration::from_secs(2)).await;
    }
}

/// The start: reset, what the controller is and can do, then the settings
/// for inquiry and for being visible.
async fn bring_up(hci: &mut Hci) {
    hci.cmd("reset", op::RESET, &[]).await;

    if let Some(a) = hci.cmd("read_local_version", op::READ_LOCAL_VERSION, &[]).await
        && a.status == 0
        && a.ret.len() >= 8
    {
        let r = &a.ret;
        say!(
            "INFO version hci={} hci_rev={:#06x} lmp={} manufacturer={:#06x} lmp_sub={:#06x}",
            r[0],
            u16::from_le_bytes([r[1], r[2]]),
            r[3],
            u16::from_le_bytes([r[4], r[5]]),
            u16::from_le_bytes([r[6], r[7]])
        );
    }

    // Which commands the controller says it knows: 64 bytes, one bit each.
    if let Some(a) = hci.cmd("read_local_commands", op::READ_LOCAL_COMMANDS, &[]).await
        && a.status == 0
        && a.ret.len() >= 64
    {
        let r = &a.ret;
        let bit = |octet: usize, b: u8| u8::from(r[octet] & (1 << b) != 0);
        say!(
            "INFO commands inquiry={} inquiry_cancel={} reject_conn={} remote_name={} write_local_name={} write_scan_enable={} write_class={} write_inquiry_mode={} read_buffer_size={} read_bd_addr={} write_eir={} write_ssp_mode={} write_le_host={} le_adv_enable={} le_scan_enable={}",
            bit(0, 0),
            bit(0, 1),
            bit(1, 1),
            bit(2, 3),
            bit(7, 0),
            bit(7, 7),
            bit(9, 1),
            bit(12, 7),
            bit(14, 7),
            bit(15, 1),
            bit(17, 1),
            bit(17, 6),
            bit(24, 6),
            bit(26, 1),
            bit(26, 3),
        );
    }

    // The controller's features: 8 bytes, one bit each.
    if let Some(a) = hci.cmd("read_local_features", op::READ_LOCAL_FEATURES, &[]).await
        && a.status == 0
        && a.ret.len() >= 8
    {
        let f = &a.ret;
        let on = |byte: usize, mask: u8| u8::from(f[byte] & mask != 0);
        say!(
            "INFO features bytes={:02x?} bredr={} le={} simultaneous_le_bredr={} inquiry_rssi={} eir={} ssp={} edr2m={} edr3m={} extended={}",
            &f[..8],
            // Bit 37 says "BR/EDR NOT supported", so it must be 0.
            1 - on(4, 0x20),
            on(4, 0x40),
            on(6, 0x02),
            on(3, 0x40),
            on(6, 0x01),
            on(6, 0x08),
            on(3, 0x02),
            on(3, 0x04),
            on(7, 0x80),
        );
    }
    hci.cmd("read_local_ext_features_p1", op::READ_LOCAL_EXT_FEATURES, &[0x01]).await;

    // How big and how many classic data packets the controller can hold.
    if let Some(a) = hci.cmd("read_buffer_size", op::READ_BUFFER_SIZE, &[]).await
        && a.status == 0
        && a.ret.len() >= 7
    {
        let r = &a.ret;
        say!(
            "INFO buffers acl_len={} sco_len={} acl_num={} sco_num={} cmd_credits={}",
            u16::from_le_bytes([r[0], r[1]]),
            r[2],
            u16::from_le_bytes([r[3], r[4]]),
            u16::from_le_bytes([r[5], r[6]]),
            hci.st.cmd_credits
        );
    }
    if let Some(a) = hci.cmd("le_read_buffer_size", op::LE_READ_BUFFER_SIZE, &[]).await
        && a.status == 0
        && a.ret.len() >= 3
    {
        say!("INFO le_buffers len={} num={}", u16::from_le_bytes([a.ret[0], a.ret[1]]), a.ret[2]);
    }

    // Our own address (the board's, fine to print).
    if let Some(a) = hci.cmd("read_bd_addr", op::READ_BD_ADDR, &[]).await
        && a.status == 0
        && a.ret.len() >= 6
    {
        let mut addr = [0; 6];
        addr.copy_from_slice(&a.ret[..6]);
        say!("INFO own_address={}", Addr(&addr));
    }

    // Which events we want. Reset puts the standard set back, which lacks
    // the extended inquiry answer (bit 46) and Low Energy (bit 61).
    // Bits: 0 inquiry complete, 1 inquiry result, 2 connection complete,
    // 3 connection request, 4 disconnection, 6 name complete, 15 hardware
    // error, 25 buffer overflow, 33 inquiry result with RSSI, 46 extended
    // inquiry result, 61 Low Energy.
    hci.cmd("set_event_mask", op::SET_EVENT_MASK, &[0x5F, 0x80, 0x00, 0x02, 0x02, 0x40, 0x00, 0x20]).await;
    // "This host speaks Low Energy too."
    hci.cmd("write_le_host_support", op::WRITE_LE_HOST_SUPPORT, &[0x01, 0x00]).await;
    // The standard Low Energy events (advertising reports among them).
    hci.cmd("le_set_event_mask", op::LE_SET_EVENT_MASK, &[0x1F, 0, 0, 0, 0, 0, 0, 0]).await;

    // Inquiry answers with signal strength, or the extended kind.
    hci.cmd("write_inquiry_mode", op::WRITE_INQUIRY_MODE, &[0x02]).await;
    hci.cmd("read_inquiry_mode", op::READ_INQUIRY_MODE, &[]).await;

    // ---- visible to phones as "PocketMe probe"
    let mut name = [0_u8; 248];
    name[..NAME.len()].copy_from_slice(NAME);
    hci.cmd("write_local_name", op::WRITE_LOCAL_NAME, &name).await;
    // The extended inquiry answer: our name, so a phone needs no name
    // request. First byte: no error correction needed. Then one piece:
    // length, type 0x09 (complete name), the name.
    let mut eir = [0_u8; 241];
    eir[1] = NAME.len() as u8 + 1;
    eir[2] = 0x09;
    eir[3..3 + NAME.len()].copy_from_slice(NAME);
    hci.cmd("write_eir", op::WRITE_EIR, &eir).await;
    hci.cmd("write_class_of_device", op::WRITE_CLASS_OF_DEVICE, &OUR_CLASS).await;
    // 0x03 = answer inquiries (be found) and pages (be connectable).
    hci.cmd("write_scan_enable", op::WRITE_SCAN_ENABLE, &[0x03]).await;
    if let Some(a) = hci.cmd("read_scan_enable", op::READ_SCAN_ENABLE, &[]).await
        && a.status == 0
    {
        say!("INFO visible scan_enable={:#04x} name=\"PocketMe probe\" class=0x000814", a.ret.first().copied().unwrap_or(0xFF));
    }
}

/// Does Low Energy still work with classic on? A passive LE scan alone,
/// then the same scan during an inquiry, then an LE advertisement.
async fn le_check(hci: &mut Hci) {
    // Passive scan, every 60 ms for 30 ms, our public address, no filter.
    hci.cmd("le_set_scan_params", op::LE_SET_SCAN_PARAMS, &[0x00, 0x60, 0x00, 0x30, 0x00, 0x00, 0x00]).await;
    // On, with duplicates kept (so the count is a rate).
    let scan_on = hci.ok("le_scan_on", op::LE_SET_SCAN_ENABLE, &[0x01, 0x00]).await;
    let start = hci.st.counts.le_adv_reports;
    let t = Instant::now();
    hci.idle(Duration::from_secs(10)).await;
    let alone = hci.st.counts.le_adv_reports - start;
    let alone_ms = t.elapsed().as_millis();
    say!("LE phase=scan-alone scan_on={} reports={} ms={} per_s={}", u8::from(scan_on), alone, alone_ms, u64::from(alone) * 1000 / alone_ms.max(1));

    // The same scan, with an inquiry running at the same time.
    let start = hci.st.counts.le_adv_reports;
    let t = Instant::now();
    let status = hci.inquiry(INQUIRY_UNITS).await;
    let during = hci.st.counts.le_adv_reports - start;
    let during_ms = t.elapsed().as_millis();
    say!(
        "LE phase=scan-during-inquiry inquiry_status={} reports={} ms={} per_s={} classic_unique={}",
        status.map_or(-1, i32::from),
        during,
        during_ms,
        u64::from(during) * 1000 / during_ms.max(1),
        hci.st.round_unique
    );
    hci.cmd("le_scan_off", op::LE_SET_SCAN_ENABLE, &[0x00, 0x00]).await;

    // An LE advertisement while the classic side is visible: every 100 ms,
    // not connectable, all three channels.
    let params = [0xA0, 0x00, 0xA0, 0x00, 0x03, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0x07, 0x00];
    hci.cmd("le_set_adv_params", op::LE_SET_ADV_PARAMS, &params).await;
    let mut data = [0_u8; 32];
    data[0] = 16; // bytes used of the 31
    data[1..4].copy_from_slice(&[0x02, 0x01, 0x06]); // flags: general discoverable, no classic
    data[4] = 12;
    data[5] = 0x09; // complete name
    data[6..17].copy_from_slice(b"PocketMe LE");
    hci.cmd("le_set_adv_data", op::LE_SET_ADV_DATA, &data).await;
    let adv_on = hci.ok("le_adv_on", op::LE_SET_ADV_ENABLE, &[0x01]).await;
    hci.idle(Duration::from_secs(3)).await;
    let adv_off = hci.ok("le_adv_off", op::LE_SET_ADV_ENABLE, &[0x00]).await;
    say!("LE phase=advertise adv_on={} adv_off={} secs=3", u8::from(adv_on), u8::from(adv_off));
    report_round(hci).await;
}

/// Counts so far: safe to share, no address, no name.
async fn report_round(hci: &Hci) {
    let st = &hci.st;
    let c = st.counts;
    let named = st.devices.iter().filter(|d| d.name_from != 0).count();
    let gamepads = st.devices.iter().filter(|d| d.gamepad_like()).count();
    let hid = st.devices.iter().filter(|d| d.hid).count();
    let mut majors: String<120> = String::new();
    for major in 0..=0x1F_u32 {
        let n = st.devices.iter().filter(|d| (d.cod >> 8) & 0x1F == major).count();
        if n > 0 {
            let _ = write!(majors, "{}{:#04x}:{}", if majors.is_empty() { "" } else { "," }, major, n);
        }
    }
    say!(
        "SUMMARY round={} known={} named={} gamepad_like={} hid_uuid={} by_major={} ev02={} ev22={} ev2f={} conn_req={} hw_err={} vendor_ev={} other_ev={} broken={} le_reports={} not_stored={}",
        st.round,
        st.devices.len(),
        named,
        gamepads,
        hid,
        if majors.is_empty() { "-" } else { majors.as_str() },
        c.inquiry_result,
        c.inquiry_result_rssi,
        c.inquiry_result_ext,
        c.connection_requests,
        c.hardware_errors,
        c.vendor_events,
        c.other_events,
        c.broken,
        c.le_adv_reports,
        st.not_stored,
    );
}

/// The memory line, same fields as ble-scan's.
pub async fn report_mem() {
    let heap = mem::heap();
    let (stack_used, stack_size) = mem::stack();
    let mut stack: String<16> = String::new();
    match stack_used {
        Some(used) => {
            let _ = write!(stack, "{used}");
        }
        // The pattern is gone all the way down: more than could be measured.
        None => {
            let _ = stack.push_str("OVER");
        }
    }
    say!(
        "MEM t={} heap_size={} heap_free={} heap_min_free={} heap_largest={} heap_requests={} heap_refused={} stack_used={} stack_size={}",
        Instant::now().as_secs(),
        heap.size,
        heap.free,
        heap.min_free,
        heap.largest,
        heap.requests,
        heap.refused,
        stack,
        stack_size
    );
}
