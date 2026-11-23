//! A check of the board's receiver that does not depend on Bluetooth.
//!
//! Why it exists: in the Bluetooth scans every advertiser came in between
//! -100 and -105 dBm. That is either true (they are all far away, behind
//! walls) or the board hears badly. WiFi uses the same radio and the same
//! antenna, and a home has an access point near by, so one WiFi scan tells
//! the two apart: a strong access point means the receiver is fine.
//!
//! The scan is passive: the board only listens for the beacons access
//! points send by themselves, and transmits nothing.
//!
//! Only numbers are printed: how many access points and how strong. No
//! network names and no addresses.
//!
//! Build: `cargo build --release --features rfcheck --bin rf-check`

#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::{clock::CpuClock, timer::timg::TimerGroup};
use esp_println::println;
use esp_radio::wifi::{
    WifiController,
    scan::{ScanConfig, ScanTypeConfig},
};

// The allocator is built with "alloc-hooks" and wants the two functions
// in there, in this program too.
#[path = "../mem.rs"]
#[allow(dead_code)]
mod mem;

esp_bootloader_esp_idf::esp_app_desc!();

/// How long the radio listens on each of the 13 or so WiFi channels.
/// Access points send a beacon about every 102 ms, so 120 ms catches one.
const LISTEN_PER_CHANNEL_MS: u64 = 120;

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 98768);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    println!();
    println!("BOOT rf-check {} passive wifi scan, {} ms a channel", env!("CARGO_PKG_VERSION"), LISTEN_PER_CHANNEL_MS);

    let mut wifi = WifiController::new(peripherals.WIFI, Default::default()).expect("wifi settings refused");
    let scan = ScanConfig::default()
        .with_show_hidden(true)
        .with_scan_type(ScanTypeConfig::Passive(esp_hal::time::Duration::from_millis(LISTEN_PER_CHANNEL_MS)));

    let mut round = 0_u32;
    loop {
        match wifi.scan_async(&scan).await {
            Ok(found) => {
                let mut strongest = i8::MIN;
                let mut weakest = i8::MAX;
                // How many in each band of strength, strongest band first.
                let mut bands = [0_u32; 5];
                for ap in &found {
                    let rssi = ap.signal_strength;
                    strongest = strongest.max(rssi);
                    weakest = weakest.min(rssi);
                    let band = match rssi {
                        -50..=0 => 0,
                        -65..=-51 => 1,
                        -80..=-66 => 2,
                        -90..=-81 => 3,
                        _ => 4,
                    };
                    bands[band] += 1;
                }
                if found.is_empty() {
                    println!("RF round={} access_points=0", round);
                } else {
                    println!(
                        "RF round={} access_points={} strongest_dbm={} weakest_dbm={} at_or_above_-50={} -51..-65={} -66..-80={} -81..-90={} below_-90={}",
                        round,
                        found.len(),
                        strongest,
                        weakest,
                        bands[0],
                        bands[1],
                        bands[2],
                        bands[3],
                        bands[4],
                    );
                }
            }
            Err(error) => println!("RF round={} ERROR scan failed: {:?}", round, error),
        }
        let heap = mem::heap();
        println!("RF round={} heap_free={} heap_min_free={}", round, heap.free, heap.min_free);
        round += 1;
        Timer::after(Duration::from_secs(2)).await;
    }
}
