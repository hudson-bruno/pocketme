//! The game as a file on the microSD card: /CONSOLE/HELLO.BIN.
//!
//! THE CARD IS THE OWNER'S. This module writes to it exactly once in the
//! card's life: when the folder CONSOLE or the file HELLO.BIN in it is not
//! there, it makes them. It never changes or removes anything that exists,
//! never looks at any other file's contents, and never prints another
//! file's name. A file that is there but wrong is reported and left alone.
//!
//! The card is set up the way an earlier firmware of the author's does
//! it, which was tried on this board with this card.

use core::convert::Infallible;

use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::{Mode, SdCard, SdCardError, TimeSource, Timestamp, VolumeIdx, VolumeManager};
use esp_hal::{
    Blocking,
    delay::Delay,
    gpio::{
        Level, Output, OutputConfig, OutputPin,
        interconnect::{PeripheralInput, PeripheralOutput},
    },
    spi::master::{Config, Instance, Spi},
    time::{Instant, Rate},
};
use game_api::{Crc32, GameHeader};
use log::{error, info, warn};

use crate::loader::{self, Loaded, Source, SourceError};

/// The folder and the file. Short names (8 characters, a dot, 3), all
/// capitals: the only kind the card library can create.
const FOLDER: &str = "CONSOLE";
const FILE: &str = "HELLO.BIN";

/// Cards must be woken up at 400 kHz or slower.
const START_UP_KHZ: u32 = 400;
/// The clock for everything after that. The probe firmware read this card
/// without errors at 20 MHz.
const WORK_MHZ: u32 = 20;

type Bus = Spi<'static, Blocking>;
type Device = ExclusiveDevice<Bus, ChipSelectHeldLow, Delay>;
type Volumes = VolumeManager<SdCard<Device, Delay>, FixedClock>;
type CardError = embedded_sdmmc::Error<SdCardError>;

/// A chip-select pin that does nothing.
///
/// The card library sends each step of an SD command as a separate SPI
/// operation, and a normal SPI device wrapper lets go of chip-select
/// between operations. The SD standard wants it held for the whole
/// command. The card is alone on its bus, so the real pin is simply held
/// low for good, and the wrapper gets this stand-in.
pub struct ChipSelectHeldLow;

impl embedded_hal::digital::ErrorType for ChipSelectHeldLow {
    type Error = Infallible;
}

impl embedded_hal::digital::OutputPin for ChipSelectHeldLow {
    fn set_low(&mut self) -> Result<(), Infallible> {
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Infallible> {
        Ok(())
    }
}

/// The date a new file gets. The board has no clock, so it is always the
/// day this was written: 16 November 2026, noon.
pub struct FixedClock;

impl TimeSource for FixedClock {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp { year_since_1970: 56, zero_indexed_month: 10, zero_indexed_day: 15, hours: 12, minutes: 0, seconds: 0 }
    }
}

/// The card, ready to have the game file read from it.
pub struct Card {
    volumes: Volumes,
    /// The real chip-select pin. Kept so that it stays low.
    _chip_select: Output<'static>,
}

/// A file on the card as the loader wants it.
struct FileSource<'a>(embedded_sdmmc::File<'a, SdCard<Device, Delay>, FixedClock, 4, 4, 1>);

impl Source for FileSource<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, SourceError> {
        self.0.read(buffer).map_err(|_| SourceError)
    }
}

impl Card {
    /// Wakes the card up and makes sure the game file is on it. `None`
    /// when there is no usable card; the reason has been printed.
    pub fn open(
        spi: impl Instance + 'static,
        sck: impl PeripheralOutput<'static>,
        mosi: impl PeripheralOutput<'static>,
        miso: impl PeripheralInput<'static>,
        cs: impl OutputPin + 'static,
        game: &[u8],
    ) -> Option<Self> {
        let started = Instant::now();
        let start_up = Config::default().with_frequency(Rate::from_khz(START_UP_KHZ));
        let mut bus = match Spi::new(spi, start_up) {
            Ok(bus) => bus.with_sck(sck).with_mosi(mosi).with_miso(miso),
            Err(error) => {
                error!("SD: could not set up the SPI bus ({error:?})");
                return None;
            }
        };
        let mut chip_select = Output::new(cs, Level::High, OutputConfig::default());

        // The SD standard asks for at least 74 clock pulses with the card
        // not selected before the first command.
        let _ = bus.write(&[0xFF; 10]);
        chip_select.set_low();

        let Ok(device) = ExclusiveDevice::new(bus, ChipSelectHeldLow, Delay::new());
        let card = SdCard::new(device, Delay::new());
        // Asking for the size is what makes the library wake the card up.
        let size = match card.num_bytes() {
            Ok(size) => size,
            Err(error) => {
                error!("SD: NO CARD answered ({error:?}). Everything about the SD card is skipped.");
                return None;
            }
        };
        let work = Config::default().with_frequency(Rate::from_mhz(WORK_MHZ));
        if let Err(error) = card.spi(|device| device.bus_mut().apply_config(&work)) {
            warn!("SD: could not raise the SPI clock ({error:?}); staying at {START_UP_KHZ} kHz");
        }
        info!(
            "SD: card awake after {} us: {:?}, {} MiB. Clock now {WORK_MHZ} MHz",
            started.elapsed().as_micros(),
            card.get_card_type(),
            size / (1024 * 1024)
        );

        let mut card = Self { volumes: VolumeManager::new(card, FixedClock), _chip_select: chip_select };
        match card.make_sure_file_is_there(game) {
            Ok(()) => Some(card),
            Err(error) => {
                error!("SD: could not make sure /{FOLDER}/{FILE} is there: {error:?}. Nothing more is done with the card.");
                None
            }
        }
    }

    /// Creates the folder and the file if, and only if, they do not exist.
    fn make_sure_file_is_there(&mut self, game: &[u8]) -> Result<(), CardError> {
        let volume = self.volumes.open_volume(VolumeIdx(0))?;
        let root = volume.open_root_dir()?;

        let folder = match root.open_dir(FOLDER) {
            Ok(folder) => {
                info!("SD: the folder /{FOLDER} is there already");
                folder
            }
            Err(CardError::NotFound) => {
                warn!("SD: WRITING TO THE CARD: making the new folder /{FOLDER}");
                root.make_dir_in_dir(FOLDER)?;
                root.open_dir(FOLDER)?
            }
            // Anything else (for one: the name belongs to a file) is not
            // for this code to sort out.
            Err(error) => return Err(error),
        };

        match folder.open_file_in_dir(FILE, Mode::ReadOnly) {
            Ok(file) => {
                info!("SD: the file /{FOLDER}/{FILE} is there already ({} bytes). Nothing is written.", file.length());
                file.close()?;
            }
            Err(CardError::NotFound) => {
                warn!("SD: WRITING TO THE CARD: making the new file /{FOLDER}/{FILE}, {} bytes", game.len());
                let started = Instant::now();
                // This mode fails if the file exists: nothing can be
                // overwritten here even by mistake.
                let file = folder.open_file_in_dir(FILE, Mode::ReadWriteCreate)?;
                file.write(game)?;
                // Closing is what puts the file's length on the card.
                file.close()?;
                info!("SD: file written in {} us. This build will not write to the card again.", started.elapsed().as_micros());
            }
            Err(error) => return Err(error),
        }
        folder.close()?;
        root.close()?;
        volume.close()?;
        Ok(())
    }

    /// Opens the file and hands it to `use_it`. Returns what `use_it`
    /// returned, the file's length, and how long finding the file took:
    /// reading the card's layout, then the two folder lookups.
    fn with_file<R>(&mut self, use_it: impl FnOnce(&mut dyn Source) -> R) -> Result<(R, u32, u64, u64), CardError> {
        let started = Instant::now();
        let volume = self.volumes.open_volume(VolumeIdx(0))?;
        let mount_us = started.elapsed().as_micros();

        let started = Instant::now();
        let root = volume.open_root_dir()?;
        let folder = root.open_dir(FOLDER)?;
        let file = folder.open_file_in_dir(FILE, Mode::ReadOnly)?;
        let open_us = started.elapsed().as_micros();

        let length = file.length();
        let mut source = FileSource(file);
        let result = use_it(&mut source);
        // Dropping the handles closes them. Nothing was written, so there
        // is nothing a close could fail to save.
        Ok((result, length, mount_us, open_us))
    }

    /// Wipes the slots, loads the game from the card and reports the
    /// times. `None` when it failed; the reason has been printed.
    pub fn load_game(&mut self, round: u32) -> Option<Loaded> {
        let (scrub_code, scrub_data) = loader::scrub();
        let started = Instant::now();
        match self.with_file(|source| loader::load(source)) {
            Ok((Ok(game), length, mount_us, open_us)) => {
                let all = started.elapsed().as_micros();
                crate::report_load("sd-card", round, &game, length as usize, scrub_code, scrub_data);
                info!(
                    "SD load #{round}: {all} us from \"card awake\" to \"ready to call\": reading the card's layout {mount_us} us, finding the file {open_us} us, loading {} us (of which waiting for the card {} us)",
                    game.timing.total, game.timing.read,
                );
                Some(game)
            }
            Ok((Err(error), length, ..)) => {
                error!("LOAD sd-card #{round}: FAILED: {error:?} (file of {length} bytes)");
                None
            }
            Err(error) => {
                error!("LOAD sd-card #{round}: FAILED: the card says {error:?}");
                None
            }
        }
    }

    /// The same without a word, for the endless part. The slots must have
    /// been wiped by the caller. The time reported as the load's total is
    /// then the whole of it, finding the file included.
    pub fn load_quietly(&mut self) -> Option<Loaded> {
        let started = Instant::now();
        let (game, ..) = self.with_file(|source| loader::load(source)).ok()?;
        let mut game = game.ok()?;
        game.timing.total = started.elapsed().as_micros();
        Some(game)
    }

    /// Reads the whole file once more and compares its checksum with the
    /// one in its header, without touching the slots. Returns the time.
    #[allow(dead_code)]
    pub fn verify(&mut self) -> Option<u64> {
        let started = Instant::now();
        let (good, ..) = self
            .with_file(|source| {
                let mut first = [0u8; GameHeader::SIZE];
                let mut filled = 0;
                while filled < first.len() {
                    match source.read(&mut first[filled..]) {
                        Ok(0) | Err(_) => return false,
                        Ok(count) => filled += count,
                    }
                }
                let header = GameHeader::from_bytes(&first);
                let mut crc = Crc32::new();
                crc.update(&first[..GameHeader::CHECKED]);
                let mut block = [0u8; 512];
                loop {
                    match source.read(&mut block) {
                        Ok(0) => break,
                        Ok(count) => crc.update(&block[..count]),
                        Err(_) => return false,
                    }
                }
                crc.value() == header.crc32
            })
            .ok()?;
        good.then(|| started.elapsed().as_micros())
    }
}
