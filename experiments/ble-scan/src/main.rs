//! Experiment: the scanning ("central") side of Bluetooth Low Energy on the
//! CYD board, with memory figures, and a first try of the pass-by beacon.
//!
//! Nothing here connects to anything. The firmware listens to the
//! advertisements around it, and in some builds sends one of its own.
//!
//! What it prints, one kind of line per fact, so that `grep` and
//! `tools/scanlog.py` can pick them out:
//!
//! * `DEV`             a new advertiser, or something new about a known one
//! * `CONTROLLER-LIKE` an advertiser that looks like a game controller
//! * `MEM`             once a second: heap and main stack
//! * `STAT`            once a second: advertisements heard, devices known
//! * `PHASE`           start and end of each phase, with the rate heard
//! * `SUMMARY`         counts by category, nothing that names a device
//! * `PROBE`           answers of the radio to single commands at start-up
//! * `ROTATE`          a change of the beacon's address
//! * `PASSBY`          a beacon of our own format, from another console
//! * `ERROR`           anything that went wrong
//!
//! Builds (cargo features, see Cargo.toml): none = listen all the time;
//! `duty15` = listen 15%; `beacon` = beacon + listen 15%; `sequence` = the
//! four combinations in turn; `active` = also ask for scan responses.
//!
//! The serial port is only written to. Its receive pin may carry a GPS
//! module's chatter (connector P1 shares it), so nothing is read from it.

#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

extern crate alloc;

use core::{cell::RefCell, fmt::Write as _};

use bt_hci::{
    cmd::{
        info::ReadLocalVersionInformation,
        le::{LeRand, LeReadLocalSupportedFeatures, LeSetExtScanEnable, LeSetRandomAddr, LeSetResolvablePrivateAddrTimeout},
    },
    controller::ExternalController,
    param::{AddrKind, BdAddr, FilterDuplicates, LeAdvEventKind, LeAdvReportsIter},
};
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_time::{Duration, Ticker, Timer};
use esp_backtrace as _;
use esp_hal::{clock::CpuClock, rng::Rng, timer::timg::TimerGroup};
use esp_radio::ble::controller::{BleConnector, BleConnectorError};
use heapless::String;
use static_cell::ConstStaticCell;
use trouble_host::prelude::*;

mod beacon;
mod mem;
mod seen;

use beacon::Beacon;
use seen::{Addr, Device, Kinds, Name, Seen, Why};

// This creates a default app-descriptor required by the esp-idf bootloader.
esp_bootloader_esp_idf::esp_app_desc!();

/// The radio as the Bluetooth library sees it. The `1`: one command to the
/// radio at a time, as in the owner's display firmware.
type Radio = ExternalController<BleConnector<'static>, 1>;
type Ble<'a> = Stack<'a, Radio, DefaultPacketPool>;
/// What a refused or failed command comes back as.
type Failure = BleHostError<BleConnectorError>;

/// Room for one connection and its two standard channels. Nothing connects
/// in this experiment, but the console will want one gamepad, so the
/// memory figures include the room for it.
const CONNECTIONS_MAX: usize = 1;
const L2CAP_CHANNELS_MAX: usize = 2;

/// The same heap the display firmware gives its radio, for a fair
/// comparison. It sits in memory the bootloader used and no longer needs.
const HEAP_SIZE: usize = 98768;

/// How often the beacon is sent.
const BEACON_EVERY: Duration = Duration::from_millis(200);

/// In a `beacon` build: how often the beacon changes its address and its
/// rotating id. A real console would wait 10 to 20 minutes; a minute lets
/// a short run show several changes.
const ROTATE_SECS: u32 = 60;

/// Length of each phase of the `sequence` build.
#[cfg(feature = "sequence")]
const PHASE_SECS: u32 = 25;

/// One stretch of time with one radio set-up.
struct Phase {
    name: &'static str,
    /// The radio listens `window_ms` out of every `interval_ms`.
    window_ms: u64,
    interval_ms: u64,
    beacon: bool,
    /// 0: for ever.
    secs: u32,
}

const fn phase(name: &'static str, window_ms: u64, interval_ms: u64, beacon: bool, secs: u32) -> Phase {
    Phase { name, window_ms, interval_ms, beacon, secs }
}

// Listening "100%": the window is as long as the interval. The radio still
// hops to the next of the three advertising channels every interval.
#[cfg(feature = "sequence")]
const PLAN: &[Phase] = &[
    phase("scan15", 150, 1000, false, PHASE_SECS),
    phase("scan15+beacon", 150, 1000, true, PHASE_SECS),
    phase("scan100", 100, 100, false, PHASE_SECS),
    phase("scan100+beacon", 100, 100, true, PHASE_SECS),
];
#[cfg(feature = "phonecheck")]
const PLAN: &[Phase] = &[phase("scan100+beacon", 100, 100, true, 0)];
#[cfg(all(feature = "beacon", not(feature = "sequence"), not(feature = "phonecheck")))]
const PLAN: &[Phase] = &[phase("scan15+beacon", 150, 1000, true, 0)];
#[cfg(all(feature = "duty15", not(feature = "beacon"), not(feature = "sequence"), not(feature = "phonecheck")))]
const PLAN: &[Phase] = &[phase("scan15", 150, 1000, false, 0)];
#[cfg(not(any(feature = "duty15", feature = "beacon", feature = "sequence", feature = "phonecheck")))]
const PLAN: &[Phase] = &[phase("scan100", 100, 100, false, 0)];

/// Prints one line, 64 bytes at a time, and after each piece waits as long
/// as it takes to leave the serial port (87 microseconds a byte at 115200
/// baud).
///
/// Why in pieces: the chip's serial port holds 128 bytes, and esp-println
/// writes with all interrupts switched off. A line longer than the room
/// left would keep them off until enough of it has gone out (about 9 ms for
/// a 230-byte line), and the radio's own interrupts with them. A 64-byte
/// piece always fits an empty port, so interrupts are off only for the
/// microseconds it takes to copy it in; the waits in between let the
/// Bluetooth library run.
///
/// (The library's own `[host]` log lines, rare after start-up, could land
/// between two pieces of a line.)
macro_rules! say {
    ($($arg:tt)*) => {{
        let mut line: String<400> = String::new();
        // A line that does not fit is cut short, which is fine for a log.
        let _ = write!(line, $($arg)*);
        let _ = line.push('\n');
        for piece in line.as_bytes().chunks(64) {
            esp_println::Printer::write_bytes(piece);
            Timer::after(Duration::from_micros(piece.len() as u64 * 90 + 100)).await;
        }
    }};
}

/// Takes the advertisements from the Bluetooth library. It is called from
/// inside the library, between two awaits, so it only counts and stores;
/// the printing is done by `control`.
struct Ears<'a> {
    seen: &'a RefCell<Seen>,
}

impl EventHandler for Ears<'_> {
    fn on_adv_reports(&self, reports: LeAdvReportsIter<'_>) {
        let mut seen = self.seen.borrow_mut();
        for report in reports {
            match report {
                Ok(report) => {
                    let kind = match report.event_kind {
                        LeAdvEventKind::AdvInd => 0,
                        LeAdvEventKind::AdvDirectInd => 1,
                        LeAdvEventKind::AdvScanInd => 2,
                        LeAdvEventKind::AdvNonconnInd => 3,
                        LeAdvEventKind::ScanRsp => 4,
                    };
                    seen.heard(kind, report.addr_kind.as_raw(), &report.addr.into_inner(), report.data, report.rssi);
                }
                Err(_) => seen.count.broken = seen.count.broken.wrapping_add(1),
            }
        }
    }
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
    esp_println::println!(
        "BOOT ble-scan {} plan={} phases active_scan={} security={}",
        env!("CARGO_PKG_VERSION"),
        PLAN.len(),
        cfg!(feature = "active"),
        cfg!(feature = "security"),
    );
    for phase in PLAN {
        esp_println::println!(
            "BOOT phase {} listen={}ms/{}ms beacon={} secs={}",
            phase.name,
            phase.window_ms,
            phase.interval_ms,
            phase.beacon,
            phase.secs
        );
    }
    report_mem(0).await;

    // This is where the radio is started. A radio that does not come up
    // ends in a panic inside this call.
    let transport = BleConnector::new(peripherals.BT, Default::default()).expect("radio settings refused");
    let controller: Radio = ExternalController::new(transport);
    let mut resources: HostResources<Radio, DefaultPacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX> = HostResources::new();
    // No address is set here, so scanning (and later a gamepad link) uses
    // the chip's own fixed address. The beacon gets a random one of its
    // own; see `rotate`.
    let stack = trouble_host::new(controller, &mut resources).build();
    let mut runner = stack.runner();

    static SEEN: ConstStaticCell<RefCell<Seen>> = ConstStaticCell::new(RefCell::new(Seen::new()));
    let seen: &RefCell<Seen> = SEEN.take();
    let ears = Ears { seen };

    let host = async {
        // The library's own loop. Without it nothing else here moves.
        let stopped = runner.run_with_handler(&ears).await;
        panic!("the Bluetooth library stopped: {stopped:?}");
    };

    join(host, control(&stack, seen, Rng::new())).await.1
}

/// Runs the phases and does all the printing.
async fn control(stack: &Ble<'_>, seen: &RefCell<Seen>, rng: Rng) -> ! {
    probes(stack).await;

    let mut peripheral = stack.peripheral();
    // Made-up contents, the same every time, except for the rotating id.
    let mut me = Beacon { id: [0; 6], look: 0x1234, nickname: [7, 42], game: 0x0001, card: *b"CARDTEST", echo: [0; 4] };
    let mut ticker = Ticker::every(Duration::from_secs(1));
    // Seconds since the phases began, counted by the ticker.
    let mut t = 0_u32;
    let mut errors = 0_u32;
    let mut rotations = 0_u32;

    for (n, phase) in PLAN.iter().cycle().enumerate() {
        {
            let mut seen = seen.borrow_mut();
            seen.count.adv_phase = 0;
            seen.count.new_phase = 0;
        }

        // ---- start listening
        let config = ScanConfig {
            active: cfg!(feature = "active"),
            filter_accept_list: &[],
            phys: PhySet::M1,
            interval: Duration::from_millis(phase.interval_ms),
            window: Duration::from_millis(phase.window_ms),
            timeout: Duration::from_secs(0),
            // Off on purpose: every advertisement is to be counted. The
            // table in `seen` is what makes each device print only once.
            filter_duplicates: FilterDuplicates::Disabled,
        };
        let mut scanner = Scanner::new(stack.central());
        // The old ("legacy") scan call. The newer `scan_ext` is for
        // Bluetooth 5 radios; see the PROBE lines for what this one says
        // to it.
        let session = scanner.scan(&config).await;

        // ---- start the beacon, with a new address and a new id
        let mut advertiser = None;
        let session = if phase.beacon {
            // First try with the scan running. The Bluetooth rules say the
            // radio may refuse a new random address while it scans.
            let scanning = session.is_ok();
            let changed = rotate(stack, &rng, &mut me).await;
            say!("ROTATE n={} while_scanning={} result={:?}", rotations, scanning, changed);
            rotations += 1;
            let session = if changed.is_err() && scanning {
                // Refused: stop listening, change, listen again.
                drop(session);
                Timer::after(Duration::from_millis(150)).await;
                let changed = rotate(stack, &rng, &mut me).await;
                say!("ROTATE n={} while_scanning=false result={:?}", rotations, changed);
                rotations += 1;
                errors += u32::from(changed.is_err());
                scanner.scan(&config).await
            } else {
                errors += u32::from(changed.is_err());
                session
            };
            match advertise(&mut peripheral, &me).await {
                Ok(started) => advertiser = Some(started),
                Err(error) => {
                    errors += 1;
                    say!("ERROR beacon did not start: {:?}", error);
                }
            }
            session
        } else {
            session
        };
        if let Err(error) = &session {
            errors += 1;
            say!("ERROR scan did not start: {:?}", error);
        }

        say!(
            "PHASE start n={} name={} t={} listen={}ms/{}ms scan={} beacon={}",
            n,
            phase.name,
            t,
            phase.window_ms,
            phase.interval_ms,
            if session.is_ok() { "on" } else { "FAILED" },
            if advertiser.is_some() { "on" } else { "off" },
        );

        // ---- the phase itself, a second at a time
        let mut secs = 0_u32;
        let mut minute_adv = 0_u32;
        loop {
            ticker.next().await;
            t += 1;
            secs += 1;

            report_devices(seen, t).await;

            // Another console heard? Put the start of its id in our beacon,
            // so that it can tell we heard it.
            let passby = {
                let mut seen = seen.borrow_mut();
                core::mem::take(&mut seen.passby_news).then_some(seen.last_passby).flatten()
            };
            if let Some(other) = passby {
                say!(
                    "PASSBY id={:02x?} look={:#06x} nickname={:?} game={:#06x} echo={:02x?} rssi={}",
                    other.id,
                    other.look,
                    other.nickname,
                    other.game,
                    other.echo,
                    other.rssi
                );
                me.echo = [other.id[0], other.id[1], other.id[2], other.id[3]];
                if advertiser.is_some() {
                    let adv = me.advertisement();
                    let data = Advertisement::NonconnectableNonscannableUndirected { adv_data: &adv };
                    if let Err(error) = peripheral.update_adv_data(data).await {
                        errors += 1;
                        say!("ERROR beacon data not updated: {:?}", error);
                    }
                }
            }

            report_mem(t).await;

            let count = {
                let mut seen = seen.borrow_mut();
                let count = seen.count;
                seen.count.adv_second = 0;
                count
            };
            minute_adv += count.adv_second;
            say!(
                "STAT t={} phase={} scan={} beacon={} adv_sec={} adv_phase={} adv_total={} devices={} not_stored={} broken={} passby={} errors={}",
                t,
                phase.name,
                if session.is_ok() { "on" } else { "off" },
                if advertiser.is_some() { "on" } else { "off" },
                count.adv_second,
                count.adv_phase,
                count.adv_total,
                seen.borrow().devices().len(),
                count.not_stored,
                count.broken,
                count.passby,
                errors,
            );

            let over = phase.secs != 0 && secs >= phase.secs;

            // A phase without end reports every minute instead.
            if !over && secs.is_multiple_of(60) {
                say!("MINUTE n={} name={} t={} adv={} adv_per_min={} errors_total={}", n, phase.name, t, minute_adv, minute_adv, errors);
                minute_adv = 0;
                report_summary(seen, t).await;
            }

            // In a `beacon` build the beacon changes its address now and
            // then while the scan stays on. If the radio refuses that, the
            // phase is left, and its next start does it the slow way.
            let mut refused = false;
            if !over && phase.beacon && phase.secs == 0 && secs.is_multiple_of(ROTATE_SECS) {
                advertiser = None;
                Timer::after(Duration::from_millis(150)).await;
                let changed = rotate(stack, &rng, &mut me).await;
                say!("ROTATE n={} while_scanning={} result={:?}", rotations, session.is_ok(), changed);
                rotations += 1;
                match changed {
                    Ok(()) => match advertise(&mut peripheral, &me).await {
                        Ok(started) => advertiser = Some(started),
                        Err(error) => {
                            errors += 1;
                            say!("ERROR beacon did not start again: {:?}", error);
                        }
                    },
                    Err(_) => refused = true,
                }
            }

            if over || refused {
                let count = seen.borrow().count;
                say!(
                    "PHASE end n={} name={} t={} secs={} adv={} adv_per_min={} new_devices={} errors_total={}",
                    n,
                    phase.name,
                    t,
                    secs,
                    count.adv_phase,
                    u64::from(count.adv_phase) * 60 / u64::from(secs.max(1)),
                    count.new_phase,
                    errors,
                );
                report_summary(seen, t).await;
                break;
            }
        }

        // Dropping these asks the library to switch the beacon and the
        // scan off. It does so a moment later, in its own loop, so it is
        // given that moment before the next phase asks for something else.
        drop(advertiser);
        drop(session);
        Timer::after(Duration::from_millis(150)).await;
    }
    // `cycle()` over a plan that is not empty never ends.
    unreachable!()
}

/// Gives the beacon a new random address and a new rotating id, together,
/// so that the one cannot be used to follow the other across the change.
///
/// The address is of the "non-resolvable" kind (top two bits 00): random,
/// and meaning nothing to anyone. It is used for the beacon only; the scan
/// keeps the chip's fixed address (and so would a gamepad link).
async fn rotate(stack: &Ble<'_>, rng: &Rng, me: &mut Beacon) -> Result<(), Failure> {
    let mut addr = [0; 6];
    rng.read(&mut addr);
    addr[5] &= 0x3F;
    // The rules forbid the random part being all zeros or all ones.
    addr[0] |= 0x01;
    addr[1] &= 0xFE;
    stack.command(LeSetRandomAddr::new(BdAddr::new(addr))).await?;
    // Only once the radio took the address: the id changes with it.
    rng.read(&mut me.id);
    Ok(())
}

/// Starts the beacon: not connectable, not scannable, every 200 ms, from
/// the random address set by `rotate`.
async fn advertise<'a>(peripheral: &mut Peripheral<'a, Radio, DefaultPacketPool>, me: &Beacon) -> Result<Advertiser<'a, Radio, DefaultPacketPool>, Failure> {
    let adv = me.advertisement();
    let params = AdvertisementParameters { interval_min: BEACON_EVERY, interval_max: BEACON_EVERY, own_addr_kind: Some(AddrKind::RANDOM), ..Default::default() };
    peripheral.advertise(&params, Advertisement::NonconnectableNonscannableUndirected { adv_data: &adv }).await
}

/// Asks the radio a few things once, at start-up, and prints the answers.
/// Each goes straight to the radio as a single command, around the
/// library's own scan and pairing code, so that a "no" is just a line in
/// the log and breaks nothing.
async fn probes(stack: &Ble<'_>) {
    // Which Bluetooth version the radio says it is (8 = 4.2, 9 = 5.0).
    match stack.command(ReadLocalVersionInformation::new()).await {
        Ok(v) => {
            // Copied out first: the answer is a packed record, and its
            // fields cannot be borrowed where they lie.
            let (hci, hci_sub, lmp, company, lmp_sub) = (v.hci_version, v.hci_subversion, v.lmp_version, v.company_identifier, v.lmp_subversion);
            say!(
                "PROBE local-version: hci_version={} hci_subversion={:#06x} lmp_version={} company={:#06x} lmp_subversion={:#06x}",
                hci.into_inner(),
                hci_sub,
                lmp.into_inner(),
                company,
                lmp_sub
            )
        }
        Err(error) => say!("PROBE local-version: refused: {:?}", error),
    }
    match stack.command(LeReadLocalSupportedFeatures::new()).await {
        Ok(features) => say!("PROBE le-features: {:02x?}", features),
        Err(error) => say!("PROBE le-features: refused: {:?}", error),
    }
    // The Bluetooth 5 way of scanning. "Switch it off" is the harmless one
    // of its commands: a radio that knows it does nothing, a radio that
    // does not know it says so.
    let off = LeSetExtScanEnable::new(false, FilterDuplicates::Disabled, bt_hci::param::Duration::from_secs(0), bt_hci::param::Duration::from_secs(0));
    match stack.command(off).await {
        Ok(()) => say!("PROBE extended-scan (LeSetExtScanEnable off): accepted"),
        Err(error) => say!("PROBE extended-scan (LeSetExtScanEnable off): refused: {:?}", error),
    }
    // Two commands the library sends at start-up once pairing support is
    // built in, and gives up if they are refused: random numbers from the
    // radio, and the life span of private addresses (set here to its
    // standard value, 900 s, so nothing changes).
    match stack.command(LeRand::new()).await {
        Ok(_) => say!("PROBE le-rand (needed for pairing): accepted"),
        Err(error) => say!("PROBE le-rand (needed for pairing): refused: {:?}", error),
    }
    match stack.command(LeSetResolvablePrivateAddrTimeout::new(bt_hci::param::Duration::from_secs(900))).await {
        Ok(()) => say!("PROBE rpa-timeout (needed for pairing): accepted"),
        Err(error) => say!("PROBE rpa-timeout (needed for pairing): refused: {:?}", error),
    }
}

/// Prints what is new in the table of devices: at most a dozen lines a
/// second, so that a crowded start cannot hold everything else up.
async fn report_devices(seen: &RefCell<Seen>, t: u32) {
    for _ in 0..12 {
        // The table is borrowed only for this one statement, never across
        // an await: the library fills it from its own loop meanwhile.
        let Some((n, first, device)) = seen.borrow_mut().take_news() else {
            break;
        };
        say!("DEV {} {} t={} {}", n, if first { "new" } else { "more" }, t, Described(&device));
        if device.controller != 0 {
            say!("CONTROLLER-LIKE why={} {}", Why(device.controller), Described(&device));
        }
    }
    // Controllers found are named again every ten seconds, with the signal
    // strength of the moment, so that a log watched live keeps showing them.
    if t.is_multiple_of(10) {
        let mut at = 0;
        loop {
            let found = {
                let seen = seen.borrow();
                seen.devices().iter().enumerate().skip(at).find(|(_, d)| d.controller != 0).map(|(i, d)| (i, d.clone()))
            };
            let Some((i, device)) = found else { break };
            say!("CONTROLLER-LIKE still-known why={} reports={} {}", Why(device.controller), device.reports, Described(&device));
            at = i + 1;
        }
    }
}

/// Everything known about one device, as `key=value` words.
struct Described<'a>(&'a Device);

impl core::fmt::Display for Described<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let d = self.0;
        write!(f, "addr={} type={} kinds={} rssi={}", Addr(&d.addr), d.addr_type().as_str(), Kinds(d.kinds), d.rssi)?;
        match d.flags {
            Some(flags) => write!(f, " flags={flags:#04x}")?,
            None => f.write_str(" flags=-")?,
        }
        match d.appearance {
            Some(appearance) => write!(f, " appearance={appearance:#06x}")?,
            None => f.write_str(" appearance=-")?,
        }
        match d.company {
            Some(company) => write!(f, " company={company:#06x}")?,
            None => f.write_str(" company=-")?,
        }
        f.write_str(" uuid16=")?;
        if d.uuids16.is_empty() {
            f.write_str("-")?;
        }
        for (i, uuid) in d.uuids16.iter().enumerate() {
            write!(f, "{}{uuid:04x}", if i == 0 { "" } else { "," })?;
        }
        match d.service_data {
            Some(uuid) => write!(f, " svcdata={uuid:04x}")?,
            None => f.write_str(" svcdata=-")?,
        }
        write!(f, " uuid128={} name={} controller={}", if d.has_uuid128 { "yes" } else { "no" }, Name(&d.name), Why(d.controller))
    }
}

/// The counts by category. Safe to copy anywhere: no address, no name.
async fn report_summary(seen: &RefCell<Seen>, t: u32) {
    let s = seen.borrow().summary();
    let kinds = seen.borrow().count.by_kind;
    say!(
        "SUMMARY t={} devices={} public={} random_static={} random_resolvable={} random_nonresolvable={} connectable={} named={} with_flags={} with_uuid16={} with_uuid128={} with_svcdata={} with_company={} with_appearance={}",
        t,
        s.devices,
        s.public,
        s.random_static,
        s.resolvable,
        s.non_resolvable,
        s.connectable,
        s.named,
        s.with_flags,
        s.with_uuid16,
        s.with_uuid128,
        s.with_service_data,
        s.with_company,
        s.with_appearance,
    );
    say!(
        "SUMMARY t={} controller_like={} hid_service={} pad_appearance={} name_hint_only={} packets: adv_ind={} adv_direct_ind={} adv_scan_ind={} adv_nonconn_ind={} scan_rsp={}",
        t,
        s.controller_like,
        s.hid,
        s.pad_appearance,
        s.name_hint_only,
        kinds[0],
        kinds[1],
        kinds[2],
        kinds[3],
        kinds[4],
    );
}

/// The memory line, printed through `say!` like every other line.
async fn report_mem(t: u32) {
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
        t,
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
