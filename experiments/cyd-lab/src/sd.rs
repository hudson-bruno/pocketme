//! The microSD card (feature `sd-card`), READ ONLY.
//!
//! Set up as experiments/native-loader/console/src/sd.rs does it, which
//! was tried on this board with this card: SPI3 (SCK 18, MOSI 23,
//! MISO 19, CS 5), woken at 400 kHz, then 20 MHz.
//!
//! Safety for the owner's card: the card library sits on [`ReadOnly`],
//! a block device whose `write` refuses every time and never reaches
//! the card. Nothing here formats, erases or writes. No name read from
//! the card is ever logged.
//!
//! Names follow the FAT "8.3" rules: up to 8 letters, a dot, up to 3;
//! no long names. A path is walked folder by folder on every call, and
//! every handle is closed before the call returns.
//!
//! No card, or a card that stopped answering: the call answers `NoCard`,
//! and the next call tries to wake a card again (that is what the
//! console's 2-second check costs when no card is in; see README).
//!
//! Before the card library wakes a card, [`probe`] asks once (CMD0, "go
//! idle") and gives up after 16 bytes without an answer. Without it, the
//! library's wake-up with no card in would retry 50 times, each time
//! for up to 10,000 bytes: seconds with the console stopped, every 2 s.

use core::convert::Infallible;
use core::ops::ControlFlow;

use board_api::files::NAME_MAX;
use board_api::{DirEntry, Files, FsError};
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::{
    Block, BlockCount, BlockDevice, BlockIdx, Mode, RawDirectory, RawVolume, SdCard, SdCardError,
    TimeSource, Timestamp, VolumeIdx, VolumeManager,
};
use esp_hal::{
    Blocking,
    delay::Delay,
    gpio::Output,
    spi::master::{Config, Spi},
    time::Rate,
};
use log::{info, warn};

/// Cards must be woken at 400 kHz or slower.
const START_UP_KHZ: u32 = 400;
/// The clock after that (read without errors on this board).
const WORK_MHZ: u32 = 20;
/// A card answers a command within 8 bytes (SD standard, "NCR"); 16 to
/// be safe.
const PROBE_BYTES: usize = 16;
/// CMD0 (go idle, which also selects SPI mode) with its checksum.
const CMD0: [u8; 6] = [0x40, 0, 0, 0, 0, 0x95];

type Bus = Spi<'static, Blocking>;
type Device = ExclusiveDevice<Bus, ChipSelectHeldLow, Delay>;
type Card = SdCard<Device, Delay>;
type Volumes = VolumeManager<ReadOnly<Card>, NoClock>;
type CardError = embedded_sdmmc::Error<RoError>;

/// A chip-select that does nothing: the card is alone on its bus, so
/// the real pin is held low by hand (the SD standard wants it held for a
/// whole command, which the SPI wrapper would not do).
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

/// Dates are only needed to write files, which never happens.
pub struct NoClock;

impl TimeSource for NoClock {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 0,
            zero_indexed_month: 0,
            zero_indexed_day: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

#[derive(Debug)]
pub enum RoError {
    Card(SdCardError),
    /// Something tried to write; it was refused.
    WriteRefused,
}

impl core::fmt::Display for RoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RoError::Card(e) => write!(f, "{e}"),
            RoError::WriteRefused => f.write_str("write refused (read-only card)"),
        }
    }
}

impl core::error::Error for RoError {}

/// A block device that can only read.
pub struct ReadOnly<D>(D);

impl BlockDevice for ReadOnly<Card> {
    type Error = RoError;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), RoError> {
        self.0.read(blocks, start).map_err(RoError::Card)
    }

    fn write(&self, _blocks: &[Block], _start: BlockIdx) -> Result<(), RoError> {
        warn!("sd: a write was asked for and refused");
        Err(RoError::WriteRefused)
    }

    fn num_blocks(&self) -> Result<BlockCount, RoError> {
        self.0.num_blocks().map_err(RoError::Card)
    }
}

/// The card's state between calls.
///
/// The awake state is much bigger (the library's block cache); it is kept
/// inline on purpose: the whole thing lives once, in a static, and a box
/// would need the heap, which is kept for the radio.
#[allow(clippy::large_enum_variant)]
enum State {
    /// Not awake (or not there).
    Asleep(Card),
    /// Awake, its first volume open.
    Ready(Volumes, RawVolume),
    /// Only while changing state.
    Gone,
}

pub struct SdFiles {
    state: State,
    /// The real chip-select pin, held low.
    cs: Output<'static>,
    /// Tries to wake a card, and how many worked.
    pub wakes: u32,
    pub woke: u32,
    /// Measuring only (serial letter `n`): act as if no card were in, by
    /// keeping the card's chip-select high. A card not selected ignores
    /// the bus, exactly like an empty slot.
    pretend_absent: bool,
}

/// Asks "is a card there?" with one CMD0 at the slow clock. Sends only
/// the command (no data, nothing that changes the card's contents).
fn probe(bus: &mut Bus, cs: &mut Output<'static>, pretend_absent: bool) -> bool {
    cs.set_high();
    let _ = bus.write(&[0xFF; 10]);
    if pretend_absent {
        // The command still goes out, to nobody, so it costs the same.
        let _ = bus.write(&CMD0);
    } else {
        cs.set_low();
        let _ = bus.write(&CMD0);
    }
    let mut answer = [0xFFu8; 1];
    for _ in 0..PROBE_BYTES {
        answer[0] = 0xFF;
        if bus.transfer(&mut answer).is_err() {
            break;
        }
        // An answer ("R1") has its top bit clear; an empty slot reads
        // 0xFF (or a floating line: then "no card" is decided later).
        if answer[0] & 0x80 == 0 {
            return !pretend_absent;
        }
    }
    false
}

fn fs_error(e: CardError) -> FsError {
    match e {
        embedded_sdmmc::Error::NotFound => FsError::NotFound,
        embedded_sdmmc::Error::FilenameError(_) => FsError::BadName,
        embedded_sdmmc::Error::DeviceError(RoError::Card(_)) => FsError::NoCard,
        embedded_sdmmc::Error::Unsupported => FsError::Unsupported,
        _ => FsError::Io,
    }
}

/// The FAT short name as "NAME.EXT" in `out`; its length.
fn short_name(e: &embedded_sdmmc::DirEntry, out: &mut [u8; NAME_MAX]) -> usize {
    let (base, ext) = (e.name.base_name(), e.name.extension());
    let mut n = 0;
    for &b in base {
        out[n] = b;
        n += 1;
    }
    if !ext.is_empty() {
        out[n] = b'.';
        n += 1;
        for &b in ext {
            out[n] = b;
            n += 1;
        }
    }
    n
}

impl SdFiles {
    /// Takes the bus (made at 400 kHz) and the chip-select pin. Does not
    /// talk to the card yet.
    pub fn new(mut bus: Bus, mut cs: Output<'static>) -> Self {
        cs.set_high();
        // At least 74 clock pulses with the card not selected before the
        // first command.
        let _ = bus.write(&[0xFF; 10]);
        cs.set_low();
        let Ok(device) = ExclusiveDevice::new(bus, ChipSelectHeldLow, Delay::new());
        Self {
            state: State::Asleep(SdCard::new(device, Delay::new())),
            cs,
            wakes: 0,
            woke: 0,
            pretend_absent: false,
        }
    }

    /// Measuring only: act as if the card were taken out (or put back).
    pub fn pretend_absent(&mut self, on: bool) {
        self.pretend_absent = on;
        if on {
            self.drop_card();
            self.cs.set_high();
        }
        info!("sd: pretend no card = {on}");
    }

    /// Makes sure a card is awake and its volume open.
    fn ready(&mut self) -> Result<(), FsError> {
        if matches!(self.state, State::Ready(..)) {
            return Ok(());
        }
        let State::Asleep(card) = core::mem::replace(&mut self.state, State::Gone) else {
            return Err(FsError::Io);
        };
        self.wakes += 1;
        let t = crate::board::now_us();
        card.mark_card_uninit();
        let slow = Config::default().with_frequency(Rate::from_khz(START_UP_KHZ));
        let (cs, pretend) = (&mut self.cs, self.pretend_absent);
        let there = card.spi(|d| {
            let _ = d.bus_mut().apply_config(&slow);
            probe(d.bus_mut(), cs, pretend)
        });
        if !there {
            self.cs.set_high();
            self.state = State::Asleep(card);
            return Err(FsError::NoCard);
        }
        // Wake from the start again, as the library expects.
        self.cs.set_high();
        card.spi(|d| {
            let _ = d.bus_mut().write(&[0xFF; 10]);
        });
        self.cs.set_low();
        let size = match card.num_bytes() {
            Ok(size) => size,
            Err(_) => {
                self.state = State::Asleep(card);
                return Err(FsError::NoCard);
            }
        };
        let fast = Config::default().with_frequency(Rate::from_mhz(WORK_MHZ));
        card.spi(|d| {
            let _ = d.bus_mut().apply_config(&fast);
        });
        let volumes = VolumeManager::new(ReadOnly(card), NoClock);
        match volumes.open_raw_volume(VolumeIdx(0)) {
            Ok(volume) => {
                self.woke += 1;
                info!(
                    "sd: card awake in {} us, {} MiB, read only",
                    crate::board::now_us() - t,
                    size / (1024 * 1024)
                );
                self.state = State::Ready(volumes, volume);
                Ok(())
            }
            Err(e) => {
                warn!("sd: card answers but its first volume does not open ({e:?})");
                let (ReadOnly(card), _) = volumes.free();
                self.state = State::Asleep(card);
                Err(FsError::Io)
            }
        }
    }

    /// After a card error: back to asleep, so the next call wakes again
    /// (with an empty block cache: the card may be another one).
    fn drop_card(&mut self) {
        if let State::Ready(volumes, volume) = core::mem::replace(&mut self.state, State::Gone) {
            let _ = volumes.close_volume(volume);
            let (ReadOnly(card), _) = volumes.free();
            card.mark_card_uninit();
            self.state = State::Asleep(card);
        }
    }

    /// Runs `f` with the volume and the folder `dir` open, then closes
    /// the folders.
    fn in_dir<T>(
        &mut self,
        dir: &str,
        f: impl FnOnce(&Volumes, RawDirectory) -> Result<T, CardError>,
    ) -> Result<T, FsError> {
        if !dir.starts_with('/') {
            return Err(FsError::BadName);
        }
        self.ready()?;
        let State::Ready(volumes, volume) = &self.state else {
            return Err(FsError::Io);
        };
        let result = (|| {
            let mut at = volumes.open_root_dir(*volume)?;
            for part in dir.split('/').filter(|p| !p.is_empty()) {
                let next = volumes.open_dir(at, part);
                let _ = volumes.close_dir(at);
                at = next?;
            }
            let r = f(volumes, at);
            let _ = volumes.close_dir(at);
            r
        })();
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                let e = fs_error(e);
                if e == FsError::NoCard {
                    self.drop_card();
                }
                Err(e)
            }
        }
    }

    /// Splits "/A/B/FILE.EXT" into "/A/B" and "FILE.EXT".
    fn split(path: &str) -> Result<(&str, &str), FsError> {
        let i = path.rfind('/').ok_or(FsError::BadName)?;
        let (dir, name) = (&path[..i], &path[i + 1..]);
        if name.is_empty() {
            return Err(FsError::BadName);
        }
        Ok((if dir.is_empty() { "/" } else { dir }, name))
    }
}

impl Files for SdFiles {
    fn list<F: FnMut(&DirEntry<'_>) -> bool>(
        &mut self,
        dir: &str,
        mut each: F,
    ) -> Result<(), FsError> {
        self.in_dir(dir, |v, d| {
            v.iterate_dir(d, |e| {
                let a = e.attributes;
                if a.is_volume() || a.is_lfn() || e.name.base_name().first() == Some(&b'.') {
                    return ControlFlow::Continue(());
                }
                let mut buf = [0u8; NAME_MAX];
                let n = short_name(e, &mut buf);
                // Short names are plain ASCII on any card made by a PC
                // or a phone; anything else is skipped.
                let Ok(name) = core::str::from_utf8(&buf[..n]) else {
                    return ControlFlow::Continue(());
                };
                let entry = DirEntry {
                    name,
                    size: e.size,
                    is_dir: a.is_directory(),
                };
                if each(&entry) {
                    ControlFlow::Continue(())
                } else {
                    ControlFlow::Break(())
                }
            })
        })
    }

    fn size(&mut self, path: &str) -> Result<u32, FsError> {
        let (dir, name) = Self::split(path)?;
        self.in_dir(dir, |v, d| {
            let f = v.open_file_in_dir(d, name, Mode::ReadOnly)?;
            let len = v.file_length(f);
            let _ = v.close_file(f);
            len
        })
    }

    fn read_at(&mut self, path: &str, offset: u32, buf: &mut [u8]) -> Result<usize, FsError> {
        let (dir, name) = Self::split(path)?;
        self.in_dir(dir, |v, d| {
            let f = v.open_file_in_dir(d, name, Mode::ReadOnly)?;
            let r = (|| {
                let len = v.file_length(f)?;
                if offset >= len {
                    return Ok(0);
                }
                v.file_seek_from_start(f, offset)?;
                let mut got = 0;
                while got < buf.len() {
                    match v.read(f, &mut buf[got..]) {
                        Ok(0) => break,
                        Ok(n) => got += n,
                        Err(embedded_sdmmc::Error::EndOfFile) => break,
                        Err(e) => return Err(e),
                    }
                }
                Ok(got)
            })();
            let _ = v.close_file(f);
            r
        })
    }
}
