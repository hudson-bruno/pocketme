//! The ST7789 panel driver, straight on esp-hal's SPI with DMA.
//!
//! A trimmed copy of experiments/display-dma/src/lcd.rs, where every part
//! was measured on this board. Kept: 16-bit colour, sending by DMA from
//! two strip buffers (one fills while the other is on the wire), the
//! scanline read (45h) for the tear-free timing, and reading pixels back
//! (for `boardctl shot --source panel`).
//!
//! Chip select (CS) is an ordinary pin driven by this code, so that it
//! stays low from the window commands to the last pixel byte, across
//! several DMA transfers.

use esp_hal::{
    Blocking,
    delay::Delay,
    dma::DmaTxBuf,
    gpio::Output,
    spi::master::{Config, SpiDma},
    time::Rate,
};

/// The panel as the console uses it: landscape.
pub const W: usize = 320;
pub const H: usize = 240;

/// Size of ONE strip buffer, in bytes. There are two.
///
/// One picture column at full height is two panel columns of 240 pixels,
/// 960 bytes. 5,120 bytes hold 5 such columns ("about 4 picture rows"
/// worth of bytes); display-dma used 15,360 (16 columns). The bench at
/// start (`wide-strips` builds) measured both; see README.
#[cfg(not(feature = "wide-strips"))]
pub const STRIP_BYTES: usize = 5_120;
#[cfg(feature = "wide-strips")]
pub const STRIP_BYTES: usize = 15_360;

/// The clock used to read the panel back. 1 to 6 MHz read clean on this
/// board; 8 MHz gave wrong red bits (screen-mirror's measurements).
pub const READ_KHZ: u32 = 4000;

/// The bits of a Get Scanline (45h) answer that carry the line.
pub const SCANLINE_MASK: u16 = 0x3FF;

/// Below this many bytes esp-hal sends through the FIFO instead of DMA.
/// Command bytes are 1 to 4 long; this keeps them off the DMA.
const FIFO_BELOW: usize = 64;

/// The CPU clock, for turning cycle counts into microseconds.
pub const CPU_MHZ: u32 = 240;

/// The CPU's cycle counter. Wraps every 17.9 s, so it is only used for
/// durations shorter than that (`wrapping_sub` handles one wrap).
#[inline(always)]
pub fn cycles() -> u32 {
    xtensa_lx::timer::get_cycle_count()
}

/// Panel commands used here (MIPI DCS and ST7789 numbers).
pub mod cmd {
    pub const SWRESET: u8 = 0x01;
    pub const SLPOUT: u8 = 0x11;
    pub const NORON: u8 = 0x13;
    pub const INVOFF: u8 = 0x20;
    pub const DISPON: u8 = 0x29;
    pub const CASET: u8 = 0x2A;
    pub const RASET: u8 = 0x2B;
    pub const RAMWR: u8 = 0x2C;
    pub const RAMRD: u8 = 0x2E;
    pub const MADCTL: u8 = 0x36;
    pub const COLMOD: u8 = 0x3A;
    pub const GSCAN: u8 = 0x45;
}

/// MADCTL (36h) for landscape: row/column exchange (MV) and column order
/// reversed (MX). What the reference firmware set up.
pub const MADCTL_LANDSCAPE: u8 = 0x60;

/// MADCTL for the columns way (MV clear): a memory write then runs along
/// one panel column (a "gate line", 240 pixels) and steps to the next
/// column, in the order the panel refreshes them. Interface column c is
/// panel y, interface row r is panel x, no mirror. Confirmed on this
/// board by reading pixels back (display-dma `bench=tear_orient
/// use=0x40 confirmed_a=true`).
pub const MADCTL_COLS: u8 = 0x40;

/// 16-bit colour (RGB565) on the wire.
const COLMOD_565: u8 = 0x55;

/// Where the time of one send went, in CPU cycles.
#[derive(Clone, Copy, Default, Debug)]
pub struct Timing {
    /// From the first command byte to CS going high after the last pixel.
    pub total: u32,
    /// The CPU filling strip buffers.
    pub fill: u32,
    /// Pixel bytes sent.
    pub bytes: u32,
    /// Cycle counter just before the first pixel transfer started.
    pub first_byte: Option<u32>,
    /// Cycle counter just after the last pixel transfer ended.
    pub last_byte: Option<u32>,
}

#[derive(Debug)]
pub enum LcdError {
    Spi(#[allow(dead_code)] esp_hal::spi::Error),
    Clock(#[allow(dead_code)] esp_hal::spi::master::ConfigError),
    TooLong,
}

pub struct Lcd {
    /// `None` only while a DMA transfer owns it, inside [`Lcd::stream`].
    spi: Option<SpiDma<'static, Blocking>>,
    /// The two strip buffers; `None` only while in use inside `stream`.
    buf_a: Option<DmaTxBuf>,
    buf_b: Option<DmaTxBuf>,
    cs: Output<'static>,
    dc: Output<'static>,
    write_mhz: u32,
    write_config: Config,
    read_config: Config,
    /// What MADCTL holds in the panel; `None` when not known.
    madctl: Option<u8>,
}

impl Lcd {
    pub fn new(
        spi: SpiDma<'static, Blocking>,
        buf_a: DmaTxBuf,
        buf_b: DmaTxBuf,
        cs: Output<'static>,
        dc: Output<'static>,
    ) -> Self {
        // Reads go through the FIFO (whatever their length): no DMA
        // receive buffer and no 4-byte alignment needed.
        let read_config = Config::default()
            .with_frequency(Rate::from_khz(READ_KHZ))
            .with_min_async_transfer_size(usize::MAX);
        let mut lcd = Self {
            spi: Some(spi),
            buf_a: Some(buf_a),
            buf_b: Some(buf_b),
            cs,
            dc,
            write_mhz: 40,
            write_config: Self::write_config_for(40),
            read_config,
            madctl: None,
        };
        let _ = lcd.set_write_mhz(40);
        lcd
    }

    fn write_config_for(mhz: u32) -> Config {
        Config::default()
            .with_frequency(Rate::from_mhz(mhz))
            .with_min_async_transfer_size(FIFO_BELOW)
    }

    fn spi(&mut self) -> &mut SpiDma<'static, Blocking> {
        self.spi
            .as_mut()
            .expect("the SPI is back after every transfer")
    }

    /// Sets the drawing clock: 80 MHz (no divider) or 80/n. 80 MHz needs
    /// the SPI's own pins (IO_MUX), which GPIO13/14 are.
    pub fn set_write_mhz(&mut self, mhz: u32) -> Result<(), LcdError> {
        let config = Self::write_config_for(mhz);
        self.spi().apply_config(&config).map_err(LcdError::Clock)?;
        self.write_config = config;
        self.write_mhz = mhz;
        Ok(())
    }

    pub fn write_mhz(&self) -> u32 {
        self.write_mhz
    }

    /// One command and its parameters, through the FIFO. CS is left as
    /// it was.
    fn command_bytes(&mut self, command: u8, params: &[u8]) -> Result<(), LcdError> {
        self.dc.set_low();
        self.spi().write(&[command]).map_err(LcdError::Spi)?;
        self.dc.set_high();
        if !params.is_empty() {
            self.spi().write(params).map_err(LcdError::Spi)?;
        }
        Ok(())
    }

    /// One whole command, CS low around it.
    pub fn command(&mut self, command: u8, params: &[u8]) -> Result<(), LcdError> {
        if command == cmd::MADCTL || command == cmd::SWRESET {
            self.madctl = None;
        }
        self.cs.set_low();
        let result = self.command_bytes(command, params);
        self.cs.set_high();
        result
    }

    /// The panel's set-up: software reset, sleep out, landscape, no
    /// inversion, 16-bit colour, normal mode, display on, with the waits
    /// the data sheet asks for.
    pub fn init(&mut self) -> Result<(), LcdError> {
        let delay = Delay::new();
        self.command(cmd::SWRESET, &[])?;
        delay.delay_millis(150);
        self.command(cmd::SLPOUT, &[])?;
        delay.delay_millis(10);
        self.set_madctl(MADCTL_LANDSCAPE)?;
        self.command(cmd::INVOFF, &[])?;
        self.command(cmd::COLMOD, &[COLMOD_565])?;
        delay.delay_millis(10);
        self.command(cmd::NORON, &[])?;
        delay.delay_millis(10);
        self.command(cmd::DISPON, &[])?;
        delay.delay_millis(120);
        Ok(())
    }

    /// Puts `value` in MADCTL, only if the panel does not hold it already.
    pub fn set_madctl(&mut self, value: u8) -> Result<(), LcdError> {
        if self.madctl == Some(value) {
            return Ok(());
        }
        self.command(cmd::MADCTL, &[value])?;
        self.madctl = Some(value);
        Ok(())
    }

    /// Opens a window under MADCTL `madctl` and starts a memory write:
    /// CASET x0..x1, RASET y0..y1 (interface addresses, inclusive), RAMWR.
    /// CS stays low until [`Lcd::end`].
    pub fn begin_in(
        &mut self,
        madctl: u8,
        x0: u16,
        y0: u16,
        x1: u16,
        y1: u16,
    ) -> Result<(), LcdError> {
        self.set_madctl(madctl)?;
        self.cs.set_low();
        let [a, b] = x0.to_be_bytes();
        let [c, d] = x1.to_be_bytes();
        self.command_bytes(cmd::CASET, &[a, b, c, d])?;
        let [a, b] = y0.to_be_bytes();
        let [c, d] = y1.to_be_bytes();
        self.command_bytes(cmd::RASET, &[a, b, c, d])?;
        self.command_bytes(cmd::RAMWR, &[])
    }

    pub fn end(&mut self) {
        self.cs.set_high();
    }

    /// Get Scanline (45h), timed: the 16-bit answer, and the cycle counter
    /// just before the command byte and just after the last answer byte.
    ///
    /// In RAM (about 400 bytes with its constants): from flash, where the
    /// code lands decides whether the read is fast. In the first `pad`
    /// build this function's code and its constants (the "literal pool",
    /// also in flash) fell on the same lines of the flash cache, so each
    /// read missed the cache several times: every read took 29 us or more
    /// (17 us in the other builds), the survey kept none, and the console
    /// lost its tear-free timing (round 6). What it calls from flash
    /// (esp-hal's `SpiDma` read and write, the clock switches,
    /// `apply_config`) is in the hot block (hot-block.list).
    #[esp_hal::ram]
    pub fn scanline_stamped_raw(&mut self) -> Result<(u16, u32, u32), LcdError> {
        // 1 dummy bit, then 16 bits of answer (10 used): 3 bytes read.
        let mut bytes = [0u8; 3];
        let (before, after) = self.slowly(READ_KHZ, |lcd| {
            let before = cycles();
            lcd.command_bytes(cmd::GSCAN, &[])?;
            lcd.spi().read(&mut bytes).map_err(LcdError::Spi)?;
            Ok((before, cycles()))
        })?;
        let hi = (u16::from_be_bytes([bytes[0], bytes[1]]) >> 7) as u8;
        let lo = (u16::from_be_bytes([bytes[1], bytes[2]]) >> 7) as u8;
        Ok((u16::from_be_bytes([hi, lo]), before, after))
    }

    /// Sends `strips` strips of pixel bytes after [`Lcd::begin_in`].
    /// `fill(k, buf)` writes strip `k` into `buf` (at most `limit` bytes
    /// used) and returns its length, a multiple of 4 (the ESP32's DMA
    /// moves whole 32-bit words). Strip k+1 is filled while strip k is on
    /// the wire.
    pub fn stream(
        &mut self,
        strips: usize,
        timing: &mut Timing,
        mut fill: impl FnMut(usize, &mut [u8]) -> usize,
    ) -> Result<(), LcdError> {
        if strips == 0 {
            return Ok(());
        }
        let mut spi = self.spi.take().expect("SPI present");
        let mut next = self.buf_a.take().expect("buffer A present");
        let mut spare = self.buf_b.take().expect("buffer B present");

        let t = cycles();
        let mut len = fill(0, next.as_mut_slice());
        timing.fill = timing.fill.wrapping_add(cycles().wrapping_sub(t));
        let mut result = Ok(());
        let mut k = 0;
        loop {
            next.set_length(len);
            timing.bytes += len as u32;
            if k == 0 {
                timing.first_byte = Some(cycles());
            }
            let transfer = match spi.write_buffer(len, next) {
                Ok(transfer) => transfer,
                Err((error, back, buf)) => {
                    spi = back;
                    next = buf;
                    result = Err(LcdError::Spi(error));
                    break;
                }
            };
            k += 1;
            let mut next_len = 0;
            if k < strips {
                let t = cycles();
                next_len = fill(k, spare.as_mut_slice());
                timing.fill = timing.fill.wrapping_add(cycles().wrapping_sub(t));
            }
            let (back, sent) = transfer.wait();
            timing.last_byte = Some(cycles());
            spi = back;
            next = spare;
            spare = sent;
            if k >= strips {
                break;
            }
            len = next_len;
        }
        self.spi = Some(spi);
        self.buf_a = Some(next);
        self.buf_b = Some(spare);
        result
    }

    /// A whole window: begin, stream, end, timed.
    pub fn draw_in(
        &mut self,
        madctl: u8,
        window: (u16, u16, u16, u16),
        strips: usize,
        fill: impl FnMut(usize, &mut [u8]) -> usize,
    ) -> Result<Timing, LcdError> {
        let mut timing = Timing::default();
        let start = cycles();
        let result = self.begin_in(madctl, window.0, window.1, window.2, window.3);
        let result = result.and_then(|()| self.stream(strips, &mut timing, fill));
        self.end();
        timing.total = cycles().wrapping_sub(start);
        result.map(|()| timing)
    }

    // ---- reading back -------------------------------------------------

    /// Runs `transfer` with the bus at the read clock and CS low, and
    /// puts both back whatever happened.
    fn slowly<T>(
        &mut self,
        khz: u32,
        transfer: impl FnOnce(&mut Self) -> Result<T, LcdError>,
    ) -> Result<T, LcdError> {
        let config = if khz == READ_KHZ {
            self.read_config
        } else {
            self.read_config.with_frequency(Rate::from_khz(khz))
        };
        self.spi().apply_config(&config).map_err(LcdError::Clock)?;
        self.cs.set_low();
        let result = transfer(self);
        self.cs.set_high();
        self.dc.set_high();
        let config = self.write_config;
        self.spi().apply_config(&config).map_err(LcdError::Clock)?;
        result
    }

    /// One command and its answer, at `khz`: sends `command` and
    /// `params`, skips `dummy_bits`, reads `out.len()` bytes (at most 256).
    #[cfg_attr(not(feature = "link"), allow(dead_code))]
    pub fn raw(
        &mut self,
        khz: u32,
        command: u8,
        params: &[u8],
        dummy_bits: u8,
        out: &mut [u8],
    ) -> Result<(), LcdError> {
        const MAX: usize = 256;
        if out.len() > MAX {
            return Err(LcdError::TooLong);
        }
        let skip = usize::from(dummy_bits);
        let total = (skip + out.len() * 8).div_ceil(8);
        let mut bytes = [0u8; MAX + 33];
        if command == cmd::MADCTL || command == cmd::SWRESET {
            self.madctl = None;
        }
        self.slowly(khz, |lcd| {
            lcd.command_bytes(command, params)?;
            if !out.is_empty() {
                lcd.spi().read(&mut bytes[..total]).map_err(LcdError::Spi)?;
            }
            Ok(())
        })?;
        let (whole, bits) = (skip / 8, skip % 8);
        for (i, byte) in out.iter_mut().enumerate() {
            let pair = u16::from_be_bytes([bytes[whole + i], bytes[whole + i + 1]]);
            *byte = (pair >> (8 - bits)) as u8;
        }
        Ok(())
    }

    /// Reads `width` panel pixels of landscape row `y` from column `x`
    /// into `scratch` (at least `1 + 3 * width` bytes): a dummy byte, then
    /// red, green, blue for each pixel, each in the top 6 bits of its
    /// byte. Returns the pixel bytes (without the dummy byte).
    pub fn read_row<'s>(
        &mut self,
        x: u16,
        y: u16,
        width: u16,
        scratch: &'s mut [u8],
    ) -> Result<&'s [u8], LcdError> {
        let n = 1 + 3 * usize::from(width);
        let Some(answer) = scratch.get_mut(..n) else {
            return Err(LcdError::TooLong);
        };
        self.set_madctl(MADCTL_LANDSCAPE)?;
        self.cs.set_low();
        let [a, b] = x.to_be_bytes();
        let [c, d] = (x + width - 1).to_be_bytes();
        let window = self
            .command_bytes(cmd::CASET, &[a, b, c, d])
            .and_then(|()| {
                let [a, b] = y.to_be_bytes();
                self.command_bytes(cmd::RASET, &[a, b, a, b])
            });
        self.cs.set_high();
        window?;
        self.slowly(READ_KHZ, |lcd| {
            lcd.command_bytes(cmd::RAMRD, &[])?;
            lcd.spi().read(answer).map_err(LcdError::Spi)
        })?;
        Ok(&answer[1..])
    }
}

/// A panel pixel as read back (6 bits a channel) turned into RGB565.
#[inline]
pub fn read_to_565(rgb: &[u8]) -> u16 {
    let (r, g, b) = (u16::from(rgb[0]), u16::from(rgb[1]), u16::from(rgb[2]));
    ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)
}
