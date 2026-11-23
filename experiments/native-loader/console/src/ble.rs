//! Bluetooth, only so that it is there.
//!
//! The finished console will talk to a gamepad over Bluetooth while a game
//! runs. The radio is the biggest user of RAM in the whole firmware, and
//! part of its code has to live in the executable RAM that the game's code
//! slot is cut from. So the loader has to be shown to work with the radio
//! on, and the memory has to be measured with it on.
//!
//! This sets the radio up the way an earlier firmware of the author's
//! does, which is known to work on this board, and then advertises: it
//! announces itself, so that a phone's scanner would list it, and accepts
//! one connection at a time. Nothing is done with a connection.

use core::future::pending;

use bt_hci::controller::ExternalController;
use embassy_futures::join::join;
use embassy_time::{Duration, Timer, with_timeout};
use esp_hal::peripherals::BT;
use esp_radio::ble::controller::BleConnector;
use log::{error, info, warn};
use portable_atomic::{AtomicBool, AtomicU32, Ordering};
use trouble_host::prelude::*;

/// One connection at a time.
const CONNECTIONS_MAX: usize = 1;

/// The two channels every connection has: signalling and attributes.
const L2CAP_CHANNELS_MAX: usize = 2;

/// The name a scanner shows. Nothing of the chip's address is in it.
const NAME: &str = "LOADER-TEST";

/// Set once the radio has accepted the order to advertise.
static ADVERTISING: AtomicBool = AtomicBool::new(false);
/// Connections accepted since the start.
static CONNECTIONS: AtomicU32 = AtomicU32::new(0);

/// Whether the radio has been told to advertise, and agreed.
pub fn advertising() -> bool {
    ADVERTISING.load(Ordering::Relaxed)
}

#[gatt_server]
struct Server {
    test: TestService,
}

/// A service with one number in it, so that a phone that connects finds
/// something. The identifiers are made up.
#[gatt_service(uuid = "6c6f6164-6572-4000-8000-746573740001")]
struct TestService {
    #[characteristic(uuid = "6c6f6164-6572-4000-8000-746573740002", read, write)]
    number: u8,
}

/// Runs Bluetooth for as long as the console is on.
#[embassy_executor::task]
pub async fn task(bt: BT<'static>) -> ! {
    // This is where the radio is started. A radio that does not come up
    // ends in a panic inside this call, and so in a restart.
    let transport = match BleConnector::new(bt, Default::default()) {
        Ok(transport) => transport,
        Err(error) => give_up(error).await,
    };
    let controller = ExternalController::<_, 1>::new(transport);
    let mut resources: HostResources<_, DefaultPacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX> = HostResources::new();
    let stack = trouble_host::new(controller, &mut resources).build();
    let runner = stack.runner();
    let mut peripheral = stack.peripheral();

    let server = match Server::new_with_config(GapConfig::default(NAME)) {
        Ok(server) => server,
        Err(error) => give_up(error).await,
    };
    // The service is never used from here: the library serves it to
    // whoever connects. This line only tells the compiler it is wanted.
    let _ = &server.test;

    // What is broadcast: that this is a Bluetooth LE device, and its name.
    let mut advert = [0; 31];
    let parts =
        [AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED), AdStructure::CompleteLocalName(NAME.as_bytes())];
    let Ok(advert_len) = AdStructure::encode_slice(&parts, &mut advert) else {
        give_up("the name does not fit in an advertisement").await
    };
    let advert = &advert[..advert_len];

    let serve = async {
        loop {
            let connection = match connect(&mut peripheral, &server, advert).await {
                Ok(connection) => connection,
                Err(error) => {
                    ADVERTISING.store(false, Ordering::Relaxed);
                    warn!("Bluetooth: could not wait for a connection: {error:?}");
                    Timer::after(Duration::from_secs(2)).await;
                    continue;
                }
            };
            let count = CONNECTIONS.fetch_add(1, Ordering::Relaxed) + 1;
            info!("Bluetooth: something connected (connection number {count} since the start)");
            talk(&stack, &connection).await;
        }
    };
    join(run_host(runner), serve).await.0
}

/// Bluetooth cannot be set up. The rest goes on without it.
async fn give_up(why: impl core::fmt::Debug) -> ! {
    error!("Bluetooth: cannot be set up: {why:?}");
    pending().await
}

/// Keeps the Bluetooth library going. Without this nothing else here
/// moves.
async fn run_host<C: Controller, P: PacketPool>(mut runner: Runner<'_, C, P>) -> ! {
    let stopped = runner.run().await;
    panic!("Bluetooth stopped: {stopped:?}");
}

/// Advertises until something connects.
async fn connect<'stack, 'server, C: Controller>(
    peripheral: &mut Peripheral<'stack, C, DefaultPacketPool>,
    server: &'server Server<'_>,
    advert: &[u8],
) -> Result<GattConnection<'stack, 'server, DefaultPacketPool>, BleHostError<C::Error>> {
    let advertisement = Advertisement::ConnectableScannableUndirected { adv_data: advert, scan_data: &[] };
    let advertiser = peripheral.advertise(&AdvertisementParameters::default(), advertisement).await?;
    // `advertise` returns once the radio's controller has answered "done"
    // to the commands that set the advertisement up and switch it on.
    if !ADVERTISING.swap(true, Ordering::Relaxed) {
        info!("Bluetooth: the radio accepted the order to advertise as \"{NAME}\"");
    }
    Ok(advertiser.accept().await?.with_attribute_server(server)?)
}

/// Serves one connection until it ends.
async fn talk<C: Controller>(stack: &Stack<'_, C, DefaultPacketPool>, connection: &GattConnection<'_, '_, DefaultPacketPool>) {
    loop {
        // The library can lose the notice that the other side has gone, so
        // a connection that says nothing is asked now and then.
        let Ok(event) = with_timeout(Duration::from_secs(1), connection.next()).await else {
            if connection.raw().is_connected() {
                continue;
            }
            info!("Bluetooth: disconnected (found by asking)");
            return;
        };
        match event {
            GattConnectionEvent::Disconnected { reason } => {
                info!("Bluetooth: disconnected ({reason:?})");
                return;
            }
            GattConnectionEvent::Gatt { event } => match event.accept() {
                Ok(reply) => reply.send().await,
                Err(error) => warn!("Bluetooth: could not answer: {error:?}"),
            },
            GattConnectionEvent::RequestConnectionParams(request) => {
                if let Err(error) = request.accept(None, stack).await {
                    warn!("Bluetooth: could not change the connection's timing: {error:?}");
                }
            }
            _ => {}
        }
    }
}
