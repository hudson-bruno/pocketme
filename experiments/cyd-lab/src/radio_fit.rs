//! Measuring only (feature `radio-fit`): does the Bluetooth radio fit
//! beside the console and the two game slots?
//!
//! It starts the radio the way experiments/bt-gamepad-test does: the
//! patched driver of experiments/bt-classic-probe, the controller in
//! dual mode (classic Bluetooth and Low Energy). Then it only keeps the
//! radio running. Nothing is sent to it: no pairing, no searching for
//! other devices, no other device's name or address is ever seen. The
//! driver writes nothing to flash.
//!
//! One log line says how long the start took, which mode the controller
//! is in (3 = dual) and how much of the heap the radio took.

use esp_hal::peripherals::BT;
use esp_radio::ble::controller::BleConnector;
use log::info;
use static_cell::StaticCell;

unsafe extern "C" {
    // In the controller library: the mode it runs in (1 Low Energy only,
    // 2 classic only, 3 dual).
    fn btdm_controller_get_mode() -> u32;
}

/// The radio, kept alive for as long as the board runs.
static RADIO: StaticCell<BleConnector<'static>> = StaticCell::new();

/// Starts the radio and logs what it cost. Panics if the driver refuses
/// (then this build has answered the question too).
pub fn start(bt: BT<'static>) {
    let before = esp_alloc::HEAP.stats();
    let t = embassy_time::Instant::now();
    let radio = BleConnector::new(bt, Default::default()).expect("radio: driver refused to start");
    let ms = t.elapsed().as_millis();
    let after = esp_alloc::HEAP.stats();
    // SAFETY: a plain getter in the controller library, no arguments,
    // called after the controller started.
    let mode = unsafe { btdm_controller_get_mode() };
    let free_before = before.size - before.current_usage;
    let free_after = after.size - after.current_usage;
    info!(
        "radio: up in {ms} ms, controller mode {mode} ({}), heap free before {free_before} after {free_after}, radio took {}",
        if mode == 3 { "dual" } else { "NOT dual" },
        free_before - free_after
    );
    RADIO.init(radio);
}
