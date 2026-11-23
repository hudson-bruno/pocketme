//! The loader experiment's console firmware.
//!
//! It answers one question on a real board: can a program that was built
//! on its own, as a file, be put into RAM on a classic ESP32 and run
//! there, calling the console through a table of functions?
//!
//! What it does after every start, in this order, all reported on the
//! serial port (115200 baud):
//!
//! 1. Says how the start before this one ended, if it ended badly.
//! 2. Loads the game that is built into the firmware, three times, and
//!    plays it for 300 frames each time. The game is a checklist
//!    (`../game`): it reports numbers, and `checks.rs` knows what they
//!    must be.
//! 3. Shows that a damaged file is refused, and times the loading of a
//!    made-up file of the largest size a game can have.
//! 4. `sd` builds: the same from the file /CONSOLE/HELLO.BIN on the card.
//! 5. `probe-byte-read`, `crash-test` and `fault-test` builds: their
//!    experiment.
//! 6. For ever: load and play again every few seconds, one line each.
//!
//! Lines a script can pick out start with a word in capitals: `MEM`,
//! `LOAD`, `PLAY`, `CHECKS`, `REFUSED`, `SD`, `PROBE`, `LAST START`,
//! `SOAK`. `tools/board-run.sh` prints a summary of them.

#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::{clock::CpuClock, time::Instant, timer::timg::TimerGroup};
use game_api::{ConsoleApi, GameCallbacks, GameHeader, layout};
use log::{error, info, warn};

use crate::loader::{LoadError, Loaded, SliceSource, Source, SourceError};

mod api;
#[cfg(feature = "ble")]
mod ble;
mod checks;
mod crash;
mod loader;
#[cfg(feature = "probe-byte-read")]
mod probe;
#[cfg(feature = "sd")]
mod sd;
mod stack;

// This creates a default app-descriptor required by the esp-idf bootloader.
esp_bootloader_esp_idf::esp_app_desc!();

/// The test game, as the file `build-game.sh` made. Being part of the
/// firmware it sits in flash, which the chip shows as ordinary read-only
/// memory.
static HELLO_BIN: &[u8] = include_bytes!("../../game/target/images/HELLO.BIN");

/// The same game with a bug in frame 150: it reads past the end of a
/// table, which is a Rust panic. The game's panic handler calls the
/// console's `abort`.
#[cfg(feature = "crash-test")]
static BAD_GAME: &[u8] = include_bytes!("../../game/target/images/CRASH.BIN");

/// The same game with another bug in frame 150: it reads through a
/// pointer to nowhere. The processor stops; the game says nothing.
#[cfg(feature = "fault-test")]
static BAD_GAME: &[u8] = include_bytes!("../../game/target/images/FAULT.BIN");

#[cfg(all(feature = "crash-test", feature = "fault-test"))]
compile_error!("crash-test and fault-test are two experiments: build one at a time");

/// The console's heap: all of the second block of data RAM up to the
/// game's data slot. Only the radio allocates from it.
const HEAP_SIZE: usize = layout::CONSOLE_HEAP_SIZE as usize + if cfg!(feature = "test-grow-heap") { 4096 } else { 0 };

/// A build test (README, "Checking the guards"): 64 KB of nothing, put
/// where the console's RAM code goes, to push that code into the game's
/// code slot and see the link fail.
#[cfg(feature = "test-grow-code")]
#[used]
#[unsafe(link_section = ".rwtext.grow_test")]
static GROW_TEST: [u8; 64 * 1024] = [0; 64 * 1024];

/// Loads of the built-in game that are reported in full.
const FULL_ROUNDS: u32 = 3;

/// Time between two rounds of the endless part.
const SOAK_EVERY: Duration = Duration::from_secs(4);

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    // As early as possible, so that as much of the stack's history as
    // possible is seen.
    stack::paint();

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: HEAP_SIZE);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    info!(
        "loader-console {}: Bluetooth {}, SD card {}{}{}",
        env!("CARGO_PKG_VERSION"),
        if cfg!(feature = "ble") { "ON" } else { "off" },
        if cfg!(feature = "sd") { "ON" } else { "off" },
        if cfg!(feature = "probe-byte-read") { ", PROBE-BYTE-READ build" } else { "" },
        if cfg!(feature = "crash-test") {
            ", CRASH-TEST build"
        } else if cfg!(feature = "fault-test") {
            ", FAULT-TEST build"
        } else {
            ""
        },
    );
    info!("reset reason: {:?}", esp_hal::system::reset_reason());

    // How the start before this one ended. Read before anything else can
    // go wrong, and remembered: a crash-test build must not load the
    // crashing game a second time.
    let note = crash::take_note();
    match note {
        None => info!("LAST START: nothing to report"),
        Some(crash::LastStart::GameAborted { code, detail, frame }) => warn!(
            "LAST START: a GAME CRASHED. It called abort with code {code} ({}), detail {detail}, in frame {frame}",
            match code {
                game_api::abort_code::PANIC => "a Rust panic; the detail is the line in the game's source",
                game_api::abort_code::BAD_API => "the game cannot work with this console",
                _ => "a code of the game's own",
            }
        ),
        Some(crash::LastStart::StoppedInsideGame { frame }) => {
            warn!("LAST START: the processor stopped INSIDE A GAME function, frame {frame}")
        }
        Some(crash::LastStart::ConsoleStopped) => {
            warn!("LAST START: the console itself stopped (a panic, or a processor fault, in its own code)")
        }
    }

    info!(
        "slots: code {:#010x}..{:#010x}, data {:#010x}..{:#010x}, console heap {:#010x}..{:#010x} ({HEAP_SIZE} bytes)",
        layout::CODE_SLOT_ADDR,
        layout::CODE_SLOT_ADDR + layout::CODE_SLOT_SIZE,
        layout::DATA_SLOT_ADDR,
        layout::DATA_SLOT_ADDR + layout::DATA_SLOT_SIZE,
        layout::DRAM2_ADDR,
        layout::DATA_SLOT_ADDR,
    );
    if !loader::slots_are_where_expected() {
        error!("the linker did not reserve the slots where game-api says they are. Stopping.");
        idle().await
    }
    let (stack_low, stack_top) = stack::ends();
    info!("main stack: {stack_low:#010x}..{stack_top:#010x} ({} bytes)", stack_top - stack_low);
    memory("at start");

    #[cfg(feature = "ble")]
    {
        spawner.spawn(ble::task(peripherals.BT).unwrap());
        // The game is to be loaded and played with the radio at work, so
        // wait until it is.
        let mut waited_ms = 0;
        while !ble::advertising() && waited_ms < 10_000 {
            Timer::after(Duration::from_millis(50)).await;
            waited_ms += 50;
        }
        if ble::advertising() {
            info!("Bluetooth: advertising after {waited_ms} ms");
        } else {
            error!("Bluetooth: NOT advertising after {waited_ms} ms. The numbers below are without it.");
        }
        // The radio's own tasks take a moment to settle their memory.
        Timer::after(Duration::from_millis(500)).await;
        memory("Bluetooth advertising, before any game");
    }
    #[cfg(not(feature = "ble"))]
    let _ = spawner;

    // The probe goes first: every one of its steps may end in a restart,
    // and there is no point in playing the game before each of them.
    #[cfg(feature = "probe-byte-read")]
    probe::run();

    // ------------------------------------------------------------------
    // The game that is built into the firmware.
    // ------------------------------------------------------------------
    let mut all_good = true;
    for round in 1..=FULL_ROUNDS {
        // Only the first round prints every report of the game.
        let quiet = round > 1;
        let label = "built-in";
        match load(label, round, &mut SliceSource(HELLO_BIN), HELLO_BIN.len()) {
            Some(game) => all_good &= play(label, round, &game, quiet).await,
            None => all_good = false,
        }
    }
    memory("after the built-in game");

    all_good &= refusals();
    all_good &= full_size();

    // ------------------------------------------------------------------
    // The game as a file on the SD card.
    // ------------------------------------------------------------------
    #[cfg(feature = "sd")]
    let mut card = sd::Card::open(
        peripherals.SPI3,
        peripherals.GPIO18,
        peripherals.GPIO23,
        peripherals.GPIO19,
        peripherals.GPIO5,
        HELLO_BIN,
    );
    #[cfg(feature = "sd")]
    if let Some(card) = card.as_mut() {
        for round in 1..=FULL_ROUNDS {
            match card.load_game(round) {
                Some(game) => all_good &= play("sd-card", round, &game, true).await,
                None => all_good = false,
            }
        }
        memory("after the game from the SD card");
    } else {
        all_good = false;
    }

    // ------------------------------------------------------------------
    // The game that crashes.
    // ------------------------------------------------------------------
    #[cfg(any(feature = "crash-test", feature = "fault-test"))]
    match note {
        Some(crash::LastStart::GameAborted { .. } | crash::LastStart::StoppedInsideGame { .. }) => {
            warn!("CRASH-TEST: a game crashed in the start before this one, so the crashing game is NOT loaded again");
        }
        _ => {
            warn!("CRASH-TEST: loading the game that crashes in frame 150. The console should restart.");
            if let Some(game) = load("crashing", 1, &mut SliceSource(BAD_GAME), BAD_GAME.len()) {
                play("crashing", 1, &game, false).await;
                error!("CRASH-TEST: the crashing game came back. That was not supposed to happen.");
                all_good = false;
            }
        }
    }

    if all_good {
        info!("SUMMARY: every load and every check passed");
    } else {
        error!("SUMMARY: SOMETHING FAILED, see above");
    }
    memory("before the endless part");

    // ------------------------------------------------------------------
    // For ever: load and play, one line per round.
    // ------------------------------------------------------------------
    let (mut rounds, mut failed) = (0u32, 0u32);
    loop {
        Timer::after(SOAK_EVERY).await;
        rounds += 1;

        // With a card, every second round takes the file from it.
        #[cfg(feature = "sd")]
        let from_card = rounds % 2 == 0 && card.is_some();
        #[cfg(not(feature = "sd"))]
        let from_card = false;

        loader::scrub();
        let loaded = if from_card {
            #[cfg(feature = "sd")]
            {
                card.as_mut().and_then(|card| card.load_quietly())
            }
            #[cfg(not(feature = "sd"))]
            None
        } else {
            loader::load(&mut SliceSource(HELLO_BIN)).ok()
        };
        let line = match &loaded {
            Some(game) => {
                let result = run(game, true).await;
                Some((game.timing, result))
            }
            None => None,
        };
        let heap = esp_alloc::HEAP.stats();
        match line {
            Some((timing, Some(result))) if result.passed == result.total => info!(
                "SOAK round {rounds} ({}): ok. load {} us (read {} us), {} of {} checks, update avg {} us max {} us; failed rounds so far {failed}; heap free {} most-used {}; stack deepest {}",
                if from_card { "sd-card" } else { "built-in" },
                timing.total,
                timing.read,
                result.passed,
                result.total,
                result.update_sum / u64::from(checks::FRAMES),
                result.update_max,
                heap.size - heap.current_usage,
                heap.max_usage,
                Deepest(stack::deepest()),
            ),
            _ => {
                failed += 1;
                error!("SOAK round {rounds}: FAILED ({failed} failed so far)");
            }
        }
    }
}

/// Nothing more to do. The radio and the serial port stay alive.
async fn idle() -> ! {
    loop {
        Timer::after(Duration::from_secs(3600)).await;
    }
}

/// Prints the stack's deepest use, or that it could not be measured.
struct Deepest(Option<usize>);

impl core::fmt::Display for Deepest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Some(bytes) => write!(f, "{bytes}"),
            None => f.write_str("ALL-OF-IT"),
        }
    }
}

/// One line about the heap and the main stack.
fn memory(when: &str) {
    let heap = esp_alloc::HEAP.stats();
    let (low, top) = stack::ends();
    info!(
        "MEM {when}: heap used {} free {} most-ever-used {} (of {}); main stack deepest {} (of {})",
        heap.current_usage,
        heap.size - heap.current_usage,
        heap.max_usage,
        heap.size,
        Deepest(stack::deepest()),
        top - low,
    );
}

/// Wipes the slots, loads a game and reports where the time went.
fn load(label: &str, round: u32, source: &mut dyn Source, file_size: usize) -> Option<Loaded> {
    let (scrub_code, scrub_data) = loader::scrub();
    match loader::load(source) {
        Ok(game) => {
            report_load(label, round, &game, file_size, scrub_code, scrub_data);
            Some(game)
        }
        Err(error) => {
            error!("LOAD {label} #{round}: FAILED: {error:?}");
            None
        }
    }
}

/// The line with a load's times. Also used by `sd.rs`.
pub fn report_load(label: &str, round: u32, game: &Loaded, file_size: usize, scrub_code: u64, scrub_data: u64) {
    let (header, timing) = (&game.header, &game.timing);
    info!(
        "LOAD {label} #{round}: ok in {} us (source read {} us, checksum {} us, code written and read back {} us, zeroing {} us). file {file_size} bytes: code {} at {:#010x}, data {} at {:#010x}, zeroed {}, entry {:#010x}, crc {:#010x}. Wiping the slots before took {scrub_code} + {scrub_data} us",
        timing.total,
        timing.read,
        timing.checksum,
        timing.code_write,
        timing.zero,
        header.code_size,
        header.code_addr,
        header.data_size,
        header.data_addr,
        header.bss_size,
        header.entry,
        header.crc32,
    );
}

/// What one play of the test game came to.
struct Played {
    passed: u32,
    total: u32,
    entry_us: u64,
    init_us: u64,
    exit_us: u64,
    update_min: u64,
    update_max: u64,
    update_sum: u64,
}

/// Plays a loaded game and prints the result. Returns whether every check
/// passed.
async fn play(label: &str, round: u32, game: &Loaded, quiet: bool) -> bool {
    let Some(result) = run(game, quiet).await else {
        error!("CHECKS {label} #{round}: the game could not be started");
        return false;
    };
    info!(
        "PLAY {label} #{round}: entry {} us, init {} us, {} x update min {} avg {} max {} us, exit {} us",
        result.entry_us,
        result.init_us,
        checks::FRAMES,
        result.update_min,
        result.update_sum / u64::from(checks::FRAMES),
        result.update_max,
        result.exit_us,
    );
    if result.passed == result.total {
        info!("CHECKS {label} #{round}: {} of {} passed", result.passed, result.total);
    } else {
        error!("CHECKS {label} #{round}: ONLY {} of {} passed", result.passed, result.total);
    }
    result.passed == result.total
}

/// Looks at the table a game handed back before anything in it is called.
///
/// The table is read as plain numbers first. Rust takes for granted that a
/// function pointer is a real address, so a doubtful one must not be given
/// that type before it has been checked.
fn callbacks_of(game: &Loaded, table: *const GameCallbacks) -> Option<&'static GameCallbacks> {
    let address = table as u32;
    let size = size_of::<GameCallbacks>() as u32;
    let data_end = layout::DATA_SLOT_ADDR + layout::DATA_SLOT_SIZE;
    if table.is_null() {
        error!("game: it refused the console's table (wrong version?)");
        return None;
    }
    if address % 4 != 0 || address < layout::DATA_SLOT_ADDR || address > data_end - size {
        error!("game: its table is at {address:#010x}, which is not in its data slot");
        return None;
    }
    // SAFETY: the address is a multiple of 4 and the whole table lies in
    // the data slot, which is plain RAM. Any four words may be read.
    let words = unsafe { table.cast::<[u32; 4]>().read() };
    let code = game.header.code_addr..game.header.code_addr + game.header.code_size;
    if words[0] < size || words[1..].iter().any(|function| !code.contains(function)) {
        error!("game: its table does not hold three addresses in its own code: {words:#010x?}");
        return None;
    }
    // SAFETY: checked above; the game's memory stays in place until the
    // next load, and callers do not keep the table longer than that.
    Some(unsafe { &*table })
}

/// Starts a loaded game, runs it for [`checks::FRAMES`] frames and stops
/// it.
///
/// Every call into the game is bracketed by `entering_game` and
/// `left_game`, so that a crash in between is known to be the game's.
async fn run(game: &Loaded, quiet: bool) -> Option<Played> {
    api::start_recording(quiet);
    let console: &'static ConsoleApi = &api::API;

    crash::entering_game(0);
    let started = Instant::now();
    let table = (game.entry)(console);
    let entry_us = started.elapsed().as_micros();
    crash::left_game();
    let callbacks = callbacks_of(game, table)?;

    crash::entering_game(0);
    let started = Instant::now();
    let answer = (callbacks.init)();
    let init_us = started.elapsed().as_micros();
    crash::left_game();
    if answer != 0 {
        error!("game: init answered {answer}, which means it does not want to run");
        return None;
    }

    let (mut update_min, mut update_max, mut update_sum) = (u64::MAX, 0, 0);
    let mut last = 0;
    for frame in 0..checks::FRAMES {
        crash::entering_game(frame);
        let started = Instant::now();
        last = (callbacks.update)(frame);
        let took = started.elapsed().as_micros();
        crash::left_game();
        update_min = update_min.min(took);
        update_max = update_max.max(took);
        update_sum += took;
        // A real console draws and waits for the next frame here. This
        // short wait gives the other tasks (Bluetooth) their turn, as that
        // would.
        Timer::after(Duration::from_millis(1)).await;
    }

    crash::entering_game(checks::FRAMES);
    let started = Instant::now();
    (callbacks.exit)();
    let exit_us = started.elapsed().as_micros();
    crash::left_game();

    let (passed, total) = checks::verdict(last, quiet);
    Some(Played { passed, total, entry_us, init_us, exit_us, update_min, update_max, update_sum })
}

/// A game file with one bit flipped, to see the loader refuse it.
struct Damaged<'a> {
    file: SliceSource<'a>,
    /// Bytes handed out so far.
    position: usize,
    /// The byte to damage.
    at: usize,
}

impl Source for Damaged<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, SourceError> {
        let count = self.file.read(buffer)?;
        if (self.position..self.position + count).contains(&self.at) {
            buffer[self.at - self.position] ^= 0x04;
        }
        self.position += count;
        Ok(count)
    }
}

/// Files the loader has to refuse. Returns whether it refused them all,
/// each for the right reason.
fn refusals() -> bool {
    let damaged = |at| Damaged { file: SliceSource(HELLO_BIN), position: 0, at };
    let code_at = GameHeader::SIZE + 40;
    let data_at = HELLO_BIN.len() - 3;
    let short = &HELLO_BIN[..HELLO_BIN.len() - 5];

    let mut good = true;
    let mut refused = |what: &str, result: Result<Loaded, LoadError>, right: fn(&LoadError) -> bool| match result {
        Err(error) if right(&error) => info!("REFUSED as it should be: {what}: {error:?}"),
        Err(error) => {
            good = false;
            error!("REFUSED FOR THE WRONG REASON: {what}: {error:?}");
        }
        Ok(_) => {
            good = false;
            error!("NOT REFUSED: {what}");
        }
    };

    refused("one bit flipped in the code", loader::load(&mut damaged(code_at)), |error| {
        matches!(error, LoadError::Checksum { .. })
    });
    refused("one bit flipped in the data", loader::load(&mut damaged(data_at)), |error| {
        matches!(error, LoadError::Checksum { .. })
    });
    refused("one bit flipped in the API version", loader::load(&mut damaged(4)), |error| {
        matches!(error, LoadError::Header(game_api::HeaderError::WrongApiVersion))
    });
    refused("one bit flipped in the code address", loader::load(&mut damaged(9)), |error| {
        matches!(error, LoadError::Header(game_api::HeaderError::WrongCodeAddress))
    });
    refused("five bytes missing at the end", loader::load(&mut SliceSource(short)), |error| {
        matches!(error, LoadError::TooShort)
    });
    good
}

/// A made-up game file of the largest size the slots can take: 64 KB of
/// "code" and 48 KB of "data", all of it noise. It is loaded and never
/// run. The test game is so small that its load time says little about a
/// real game's; this says what the loader itself costs at full size,
/// without the time a card would take to hand the bytes over.
struct MadeUp {
    header: [u8; GameHeader::SIZE],
    /// Bytes handed out so far.
    position: usize,
    noise: u32,
}

impl MadeUp {
    const CODE: u32 = layout::CODE_SLOT_SIZE;
    const DATA: u32 = layout::DATA_SLOT_SIZE - Self::ZEROED;
    const ZEROED: u32 = 0x4000;
    const SEED: u32 = 0x1234_5678;

    fn new() -> Self {
        let mut header = GameHeader {
            magic: game_api::MAGIC,
            api_version: game_api::API_VERSION,
            code_addr: layout::CODE_SLOT_ADDR,
            code_size: Self::CODE,
            data_addr: layout::DATA_SLOT_ADDR,
            data_size: Self::DATA,
            bss_size: Self::ZEROED,
            entry: layout::CODE_SLOT_ADDR,
            crc32: 0,
        };
        // The checksum has to be right, so the noise is made twice: once
        // here to add it up, once for the loader.
        let mut crc = game_api::Crc32::new();
        crc.update(&header.to_bytes()[..GameHeader::CHECKED]);
        let mut noise = Self::SEED;
        for _ in 0..Self::CODE + Self::DATA {
            crc.update(&[Self::next(&mut noise)]);
        }
        header.crc32 = crc.value();
        Self { header: header.to_bytes(), position: 0, noise: Self::SEED }
    }

    fn next(noise: &mut u32) -> u8 {
        *noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (*noise >> 24) as u8
    }
}

impl Source for MadeUp {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, SourceError> {
        let end = GameHeader::SIZE + (Self::CODE + Self::DATA) as usize;
        let count = buffer.len().min(end - self.position);
        for byte in &mut buffer[..count] {
            *byte = match self.header.get(self.position) {
                Some(&byte) => byte,
                None => Self::next(&mut self.noise),
            };
            self.position += 1;
        }
        Ok(count)
    }
}

/// Loads the made-up file and reports the times. Returns whether it
/// loaded.
fn full_size() -> bool {
    loader::scrub();
    let loaded = loader::load(&mut MadeUp::new());
    // What is in the slots now is noise. Wipe it, so that a stray jump
    // there finds an instruction that stops it.
    loader::scrub();
    match loaded {
        Ok(Loaded { header, timing, .. }) => {
            info!(
                "FULLSIZE: a made-up file of the largest size (code {} bytes, data {}, zeroed {}) loaded: code written and read back {} us, checksum {} us, zeroing {} us; making up the bytes and copying the data took {} us more, which a real load spends on its storage instead",
                header.code_size, header.data_size, header.bss_size, timing.code_write, timing.checksum, timing.zero, timing.read,
            );
            true
        }
        Err(error) => {
            error!("FULLSIZE: FAILED: {error:?}");
            false
        }
    }
}
