//! The ST7789 screen of the CYD board, on SPI2 with DMA.
//!
//! A trimmed copy of `experiments/display-dma/src/lcd.rs` (proven on this
//! board). Kept: the set-up, opening a window, sending pixels by DMA from
//! two strip buffers, and reading a row back for screenshots. Left out:
//! 12-bit colour, 80 MHz, scanline reads, porches and timing.
//!
//! - The clock is 40 MHz, inside the panel's data sheet (80 MHz is not).
//! - Landscape: MADCTL 60h, 320 wide and 240 high.
//! - Small writes (a command, a few pixels) go through the SPI's 64-byte
//!   FIFO, pushed by the CPU. Large ones go by DMA: the SPI hardware
//!   fetches the bytes from RAM by itself while the CPU fills the other
//!   strip buffer.
//! - Chip select (CS) is a plain pin driven here, so it stays low from
//!   the window commands to the last pixel byte.

use esp_hal::{
    Blocking,
    delay::Delay,
    dma::DmaTxBuf,
    gpio::Output,
    spi::master::{Config, SpiDma},
    time::Rate,
};

pub const W: usize = 320;
pub const H: usize = 240;

/// Size of one strip buffer. Two of them: 8 KB, the driver's whole pixel
/// memory (a whole screen would be 153 KB and does not fit next to
/// Bluetooth).
pub const STRIP_BYTES: usize = 4096;

/// Writes shorter than this go through the FIFO, without DMA. A letter
/// of the small fonts is 80 to 120 bytes: cheaper by FIFO than setting
/// up a DMA transfer.
pub const FIFO_BELOW: usize = 512;

/// The drawing clock.
pub const WRITE_MHZ: u32 = 40;

/// The clock used to read the panel back. 1 to 6 MHz read clean on this
/// board; 8 MHz gave wrong red bits (screen-mirror's measurements).
pub const READ_KHZ: u32 = 4000;

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
}

/// Row/column exchange (MV, 0x20) and column order reversed (MX, 0x40):
/// landscape, as the reference firmware sets it.
pub const MADCTL_LANDSCAPE: u8 = 0x60;
/// 16 bits a pixel.
const COLMOD_565: u8 = 0x55;

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
    write_config: Config,
    read_config: Config,
    /// The column and row range the panel holds now, if known. A window
    /// with the same range skips that command (the panel keeps it): for
    /// letters on one line, the rows stay the same.
    cols: Option<(u16, u16)>,
    rows: Option<(u16, u16)>,
}

impl Lcd {
    pub fn new(
        spi: SpiDma<'static, Blocking>,
        buf_a: DmaTxBuf,
        buf_b: DmaTxBuf,
        cs: Output<'static>,
        dc: Output<'static>,
    ) -> Self {
        let write_config = Config::default()
            .with_frequency(Rate::from_mhz(WRITE_MHZ))
            .with_min_async_transfer_size(FIFO_BELOW);
        // Reads go through the FIFO too (whatever their length), so that
        // they need no DMA receive buffer and no 4-byte alignment.
        let read_config = Config::default()
            .with_frequency(Rate::from_khz(READ_KHZ))
            .with_min_async_transfer_size(usize::MAX);
        let mut lcd = Self {
            spi: Some(spi),
            buf_a: Some(buf_a),
            buf_b: Some(buf_b),
            cs,
            dc,
            write_config,
            read_config,
            cols: None,
            rows: None,
        };
        let _ = lcd.spi().apply_config(&write_config);
        lcd
    }

    fn spi(&mut self) -> &mut SpiDma<'static, Blocking> {
        self.spi
            .as_mut()
            .expect("the SPI is back after every transfer")
    }

    /// One command and its parameters, through the FIFO. CS is left as
    /// it was: the caller decides. In fast memory (IRAM), as are the other
    /// drawing functions: run from flash, their speed changed five times
    /// between two builds (cache misses).
    #[esp_hal::ram]
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
        self.forget_window();
        self.cs.set_low();
        let result = self.command_bytes(command, params);
        self.cs.set_high();
        result
    }

    /// The set-up the reference firmware does: software reset (the board
    /// has no reset wire), sleep out, landscape, no inversion, 16-bit
    /// colour, normal mode, display on, with the data sheet's waits.
    /// Takes about 300 ms.
    pub fn init(&mut self) -> Result<(), LcdError> {
        let delay = Delay::new();
        self.command(cmd::SWRESET, &[])?;
        delay.delay_millis(150);
        self.command(cmd::SLPOUT, &[])?;
        delay.delay_millis(10);
        self.command(cmd::MADCTL, &[MADCTL_LANDSCAPE])?;
        self.command(cmd::INVOFF, &[])?;
        self.command(cmd::COLMOD, &[COLMOD_565])?;
        delay.delay_millis(10);
        self.command(cmd::NORON, &[])?;
        delay.delay_millis(10);
        self.command(cmd::DISPON, &[])?;
        delay.delay_millis(120);
        Ok(())
    }

    /// The panel's window is no longer known (after a read, a raw
    /// command or an error): the next `begin` sends it in full.
    fn forget_window(&mut self) {
        self.cols = None;
        self.rows = None;
    }

    /// Opens a window (inclusive corners) and starts a memory write:
    /// CASET, RASET, RAMWR. CS stays low until [`Lcd::end`]. CASET or
    /// RASET is left out when the panel already holds that range.
    #[esp_hal::ram]
    pub fn begin(&mut self, x0: u16, y0: u16, x1: u16, y1: u16) -> Result<(), LcdError> {
        self.cs.set_low();
        let result = self.begin_inner(x0, y0, x1, y1);
        if result.is_err() {
            self.forget_window();
        }
        result
    }

    #[esp_hal::ram]
    fn begin_inner(&mut self, x0: u16, y0: u16, x1: u16, y1: u16) -> Result<(), LcdError> {
        if self.cols != Some((x0, x1)) {
            self.cols = None;
            let [a, b] = x0.to_be_bytes();
            let [c, d] = x1.to_be_bytes();
            self.command_bytes(cmd::CASET, &[a, b, c, d])?;
            self.cols = Some((x0, x1));
        }
        if self.rows != Some((y0, y1)) {
            self.rows = None;
            let [a, b] = y0.to_be_bytes();
            let [c, d] = y1.to_be_bytes();
            self.command_bytes(cmd::RASET, &[a, b, c, d])?;
            self.rows = Some((y0, y1));
        }
        self.command_bytes(cmd::RAMWR, &[])
    }

    pub fn end(&mut self) {
        self.cs.set_high();
    }

    /// Pixel bytes after [`Lcd::begin`], through the FIFO (short pieces).
    #[esp_hal::ram]
    pub fn write_small(&mut self, bytes: &[u8]) -> Result<(), LcdError> {
        self.spi().write(bytes).map_err(LcdError::Spi)
    }

    /// Sends `strips` strips of pixel bytes after [`Lcd::begin`].
    /// `fill(k, buf)` writes strip `k` into `buf` (up to [`STRIP_BYTES`])
    /// and returns its length, a multiple of 4: the ESP32's DMA moves
    /// whole 32-bit words. Strip k+1 is filled while strip k is on the
    /// wire.
    pub fn stream(
        &mut self,
        strips: usize,
        mut fill: impl FnMut(usize, &mut [u8]) -> usize,
    ) -> Result<(), LcdError> {
        if strips == 0 {
            return Ok(());
        }
        let mut spi = self.spi.take().expect("SPI present");
        let mut next = self.buf_a.take().expect("buffer A present");
        let mut spare = self.buf_b.take().expect("buffer B present");
        let mut len = fill(0, next.as_mut_slice());
        let mut result = Ok(());
        let mut k = 0;
        loop {
            next.set_length(len);
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
                next_len = fill(k, spare.as_mut_slice());
            }
            let (back, sent) = transfer.wait();
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

    // ---- reading back (screenshots) -----------------------------------

    /// Runs `transfer` with the bus at the read clock and CS low, and
    /// puts both back whatever happened.
    fn slowly<T>(
        &mut self,
        transfer: impl FnOnce(&mut Self) -> Result<T, LcdError>,
    ) -> Result<T, LcdError> {
        self.forget_window();
        let config = self.read_config;
        self.spi().apply_config(&config).map_err(LcdError::Clock)?;
        self.cs.set_low();
        let result = transfer(self);
        self.cs.set_high();
        self.dc.set_high();
        let config = self.write_config;
        self.spi().apply_config(&config).map_err(LcdError::Clock)?;
        result
    }

    /// One command and its answer, at the read clock: sends `command`
    /// and `params`, skips `dummy_bits`, reads `out.len()` bytes (at most
    /// 256). For `boardctl raw`.
    pub fn raw(
        &mut self,
        command: u8,
        params: &[u8],
        dummy_bits: u8,
        out: &mut [u8],
    ) -> Result<(), LcdError> {
        const MAX: usize = 256;
        if out.len() > MAX || dummy_bits > 32 {
            return Err(LcdError::TooLong);
        }
        let skip = usize::from(dummy_bits);
        let total = (skip + out.len() * 8).div_ceil(8);
        let mut bytes = [0u8; MAX + 6];
        self.slowly(|lcd| {
            lcd.command_bytes(command, params)?;
            if !out.is_empty() {
                lcd.spi().read(&mut bytes[..total]).map_err(LcdError::Spi)?;
            }
            Ok(())
        })?;
        // The answer may start in the middle of a byte: shift it in place.
        let (whole, bits) = (skip / 8, skip % 8);
        for (i, byte) in out.iter_mut().enumerate() {
            let pair = u16::from_be_bytes([bytes[whole + i], bytes[whole + i + 1]]);
            *byte = (pair >> (8 - bits)) as u8;
        }
        // A reset or a new MADCTL from the PC would change the picture's
        // turn; put landscape back.
        if command == cmd::SWRESET || command == cmd::MADCTL {
            let _ = self.command(cmd::MADCTL, &[MADCTL_LANDSCAPE]);
        }
        Ok(())
    }

    /// Reads `width` pixels of row `y` from column `x` into `scratch` (at
    /// least `1 + 3 * width` bytes): a dummy byte, then red, green, blue
    /// for each pixel, each in the top 6 bits of its byte. Returns the
    /// pixel bytes (without the dummy byte).
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
        // The window commands are writes and go at the drawing clock;
        // only the read itself is slow.
        self.forget_window();
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
        self.slowly(|lcd| {
            lcd.command_bytes(cmd::RAMRD, &[])?;
            lcd.spi().read(answer).map_err(LcdError::Spi)
        })?;
        Ok(&answer[1..])
    }
}

/// A pixel as read back (6 bits a channel) turned into RGB565.
#[inline]
pub fn read_to_565(rgb: &[u8]) -> u16 {
    let (r, g, b) = (u16::from(rgb[0]), u16::from(rgb[1]), u16::from(rgb[2]));
    ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)
}
