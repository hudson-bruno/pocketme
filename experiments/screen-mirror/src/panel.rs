//! Reading from the display panel: its registers, and the pixels in its
//! memory.
//!
//! The panel keeps every pixel it was sent in a memory of its own, and
//! that memory is what the glass shows. It can be read back over the
//! panel's data-out wire (MISO, GPIO12), which is how this firmware
//! takes a screenshot of "what is really on the screen" instead of "what
//! we meant to draw".
//!
//! Reading uses the same SPI bus as drawing. Three things differ:
//!
//! * **The clock must be much slower.** The panel takes drawing at
//!   40 MHz but answers far slower (the datasheet's read cycle is
//!   150 ns, under 7 MHz). So the bus is slowed down for the read and
//!   put back afterwards. Measured on this board, whole screen compared
//!   with the picture drawn: asking for 1, 2, 4 and 6 MHz gave not one
//!   wrong pixel; 8 MHz gave about 130 wrong ones per read (red only,
//!   other places every time); 10 MHz and up gave thousands.
//! * **The chip-select wire stays low** from the command to the last
//!   byte read. (For drawing, the driver lets it go between a command
//!   and its data; a read would be forgotten at that point.)
//! * **The answer starts late.** After the command the panel first puts
//!   out bits that mean nothing: 1 for the ID and status registers, 8
//!   for pixels. They are clocked and thrown away.

use core::{cell::RefCell, convert::Infallible};

use embedded_hal::digital::{ErrorType, OutputPin};
use esp_hal::{
    Blocking,
    gpio::Output,
    spi::master::{Config as SpiConfig, ConfigError, Spi},
    time::Rate,
};

/// The bus, shared between the drawing driver and the reader. A
/// `RefCell` is enough: both run in the same task, one after the other.
pub type Bus = RefCell<Spi<'static, Blocking>>;
/// A pin both of them drive (chip select, and data/command).
pub type Pin = RefCell<Output<'static>>;

/// Lets the drawing driver use a pin that the reader also needs.
pub struct SharedPin(pub &'static Pin);

impl ErrorType for SharedPin {
    type Error = Infallible;
}

impl OutputPin for SharedPin {
    fn set_low(&mut self) -> Result<(), Infallible> {
        self.0.borrow_mut().set_low();
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Infallible> {
        self.0.borrow_mut().set_high();
        Ok(())
    }
}

/// Panel commands used here (the same on every MIPI-DCS panel).
pub mod cmd {
    /// Read display ID: 1 dummy bit, then 3 bytes.
    pub const RDDID: u8 = 0x04;
    /// Read display status: 1 dummy bit, then 4 bytes.
    pub const RDDST: u8 = 0x09;
    /// Read display power mode: 1 byte.
    pub const RDDPM: u8 = 0x0A;
    /// Read the memory access control setting (which way the picture is
    /// turned): 1 byte.
    pub const RDDMADCTL: u8 = 0x0B;
    /// Read the pixel format setting: 1 byte.
    pub const RDDCOLMOD: u8 = 0x0C;
    /// Column address set: first and last column, 2 bytes each.
    pub const CASET: u8 = 0x2A;
    /// Row address set: first and last row, 2 bytes each.
    pub const RASET: u8 = 0x2B;
    /// Memory read: 8 dummy bits, then 3 bytes per pixel.
    pub const RAMRD: u8 = 0x2E;
}

#[derive(Debug)]
pub enum ReadError {
    /// The clock asked for cannot be made.
    Clock(ConfigError),
    /// The SPI driver's own error. Only ever printed, hence the `allow`.
    Spi(#[allow(dead_code)] esp_hal::spi::Error),
    /// More bytes than the working buffer holds.
    TooLong,
}

/// Most bytes [`PanelReader::raw`] can read in one go.
pub const RAW_MAX: usize = 256;

pub struct PanelReader {
    bus: &'static Bus,
    cs: &'static Pin,
    dc: &'static Pin,
    /// The bus settings for drawing, put back after every read.
    write_config: SpiConfig,
}

impl PanelReader {
    pub fn new(bus: &'static Bus, cs: &'static Pin, dc: &'static Pin, write_config: SpiConfig) -> Self {
        Self { bus, cs, dc, write_config }
    }

    /// Runs `transfer` with the bus at `khz` and chip select low, and
    /// puts both back whatever happened.
    fn slowly<T>(
        &self,
        khz: u32,
        transfer: impl FnOnce(&mut Spi<'static, Blocking>, &Pin) -> Result<T, ReadError>,
    ) -> Result<T, ReadError> {
        let mut bus = self.bus.borrow_mut();
        bus.apply_config(&SpiConfig::default().with_frequency(Rate::from_khz(khz))).map_err(ReadError::Clock)?;
        self.cs.borrow_mut().set_low();
        let result = transfer(&mut bus, self.dc);
        self.cs.borrow_mut().set_high();
        self.dc.borrow_mut().set_high();
        bus.apply_config(&self.write_config).map_err(ReadError::Clock)?;
        result
    }

    /// Sends a command byte (data/command wire low) and its parameters
    /// (wire high). Chip select is the caller's business.
    fn command(bus: &mut Spi<'static, Blocking>, dc: &Pin, cmd: u8, params: &[u8]) -> Result<(), ReadError> {
        dc.borrow_mut().set_low();
        bus.write(&[cmd]).map_err(ReadError::Spi)?;
        dc.borrow_mut().set_high();
        if !params.is_empty() {
            bus.write(params).map_err(ReadError::Spi)?;
        }
        Ok(())
    }

    /// One command to the panel and its answer: sends `cmd` and
    /// `params`, clocks `dummy_bits` bits and drops them, then reads
    /// `out.len()` bytes. Everything at `khz`.
    pub fn raw(&self, khz: u32, cmd: u8, params: &[u8], dummy_bits: u8, out: &mut [u8]) -> Result<(), ReadError> {
        // The SPI hardware reads whole bytes. When the answer starts in
        // the middle of one, read a little more and shift it into place.
        let skip = usize::from(dummy_bits);
        let total = (skip + out.len() * 8).div_ceil(8);
        let mut bytes = [0u8; RAW_MAX + 33];
        if out.len() > RAW_MAX {
            return Err(ReadError::TooLong);
        }
        self.slowly(khz, |bus, dc| {
            Self::command(bus, dc, cmd, params)?;
            if !out.is_empty() {
                bus.read(&mut bytes[..total]).map_err(ReadError::Spi)?;
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

    /// Reads `width` pixels of row `y` of the panel's memory, from column
    /// `x` on, into `out` as RGB565, low byte first (2 bytes a pixel).
    ///
    /// `scratch` is working memory of at least `1 + 3 * width` bytes: the
    /// panel answers with a byte that means nothing, then three bytes
    /// for every pixel (red, green, blue, each in the top 6 bits),
    /// whatever format the pixels were written in.
    pub fn read_row(
        &self,
        khz: u32,
        x: u16,
        y: u16,
        width: u16,
        scratch: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), ReadError> {
        let pixels = usize::from(width);
        let Some(answer) = scratch.get_mut(..1 + 3 * pixels) else {
            return Err(ReadError::TooLong);
        };
        let [x0, x1] = [x, x + width - 1].map(u16::to_be_bytes);
        let [y0, y1] = [y.to_be_bytes(), y.to_be_bytes()];
        // Which pixels: the same "window" commands as for drawing. Each
        // is a transfer of its own.
        self.slowly(khz, |bus, dc| Self::command(bus, dc, cmd::CASET, &[x0[0], x0[1], x1[0], x1[1]]))?;
        self.slowly(khz, |bus, dc| Self::command(bus, dc, cmd::RASET, &[y0[0], y0[1], y1[0], y1[1]]))?;
        self.slowly(khz, |bus, dc| {
            Self::command(bus, dc, cmd::RAMRD, &[])?;
            bus.read(answer).map_err(ReadError::Spi)
        })?;
        for (rgb, pixel) in answer[1..].chunks_exact(3).zip(out.chunks_exact_mut(2)) {
            let (r, g, b) = (u16::from(rgb[0]), u16::from(rgb[1]), u16::from(rgb[2]));
            let rgb565 = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3);
            pixel.copy_from_slice(&rgb565.to_le_bytes());
        }
        Ok(())
    }
}
