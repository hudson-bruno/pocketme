//! A minimal ST7789 driver written straight on esp-hal's SPI with DMA.
//!
//! Why not mipidsi: it sends pixels with blocking CPU writes, 64 bytes at
//! a time, and the CPU can do nothing else meanwhile. Here the pixels go
//! out by DMA (the SPI hardware fetches them from RAM by itself) while
//! the CPU prepares the next piece. mipidsi also has no 12-bit colour.
//!
//! Two ways of sending are used, on purpose:
//!
//! * **Command bytes** (a command and its 4 parameters) go through the
//!   SPI's 64-byte FIFO, driven by the CPU. For 1 to 5 bytes that is
//!   cheaper than setting up a DMA transfer. esp-hal does this by itself
//!   for any slice shorter than `min_async_transfer_size`.
//! * **Pixels** go by DMA from one of two strip buffers ([`Lcd::stream`]):
//!   while strip N is on the wire, the CPU fills strip N+1 in the other.
//!
//! Chip select (CS) is an ordinary pin driven by this code, so that it
//! stays low from the window commands to the last pixel byte, across
//! several DMA transfers.
//!
//! Reading the panel back (for checking pictures) follows what
//! `experiments/screen-mirror/src/panel.rs` proved on this board: slow
//! clock (4 MHz), CS low from the command to the last byte, 8 dummy bits
//! before pixels, 3 bytes per pixel.

use esp_hal::{
    Blocking,
    delay::Delay,
    dma::DmaTxBuf,
    gpio::Output,
    spi::master::{Config, SpiDma},
    time::Rate,
};

/// The panel as the console uses it: turned to landscape.
pub const W: usize = 320;
pub const H: usize = 240;

/// Most logical rows (160 pixels, shown as two panel rows of 320) one
/// strip buffer holds. 12 rows in 16-bit colour are 15,360 bytes; two
/// such buffers are the driver's whole pixel memory.
pub const STRIP_ROWS_MAX: usize = 12;
/// Size of one strip buffer: 12 logical rows, doubled, 2 bytes a pixel.
pub const STRIP_BYTES: usize = STRIP_ROWS_MAX * 2 * W * 2;

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
    pub const RDDID: u8 = 0x04;
    pub const RDDST: u8 = 0x09;
    pub const RDDPM: u8 = 0x0A;
    pub const RDDMADCTL: u8 = 0x0B;
    pub const RDDCOLMOD: u8 = 0x0C;
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
    /// Write Memory Continue: more pixels, going on from where the last
    /// memory write stopped, without new window commands.
    pub const RAMWRC: u8 = 0x3C;
    pub const GSCAN: u8 = 0x45;
    /// Porch setting: back porch and front porch lines (BPA, FPA), the
    /// quiet lines between two refreshes.
    pub const PORCTRL: u8 = 0xB2;
    pub const FRCTRL2: u8 = 0xC6;
}

/// PORCTRL (B2h) after reset, from the data sheet: BPA 0Ch, FPA 0Ch,
/// separate porch off, idle and partial porches 33h.
pub const PORCH_DEFAULT: [u8; 5] = [0x0C, 0x0C, 0x00, 0x33, 0x33];

/// What MADCTL (36h) holds after mipidsi's set-up in the reference
/// firmware (ST7789, RGB order, turned 90 degrees): row/column exchange
/// (MV, 0x20) and column order reversed (MX, 0x40). Read back from the
/// panel by screen-mirror as 60h.
pub const MADCTL_LANDSCAPE: u8 = 0x60;

/// The MADCTL bit that mirrors the page (row) address (MY, D7).
pub const MADCTL_MY: u8 = 0x80;

/// Colour depth on the wire.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Depth {
    /// 16 bits a pixel (COLMOD 55h): 153,600 bytes a frame.
    Rgb565,
    /// 12 bits a pixel (COLMOD 53h): 3 bytes for 2 pixels, 115,200 a frame.
    Rgb444,
}

impl Depth {
    pub fn colmod(self) -> u8 {
        match self {
            Depth::Rgb565 => 0x55,
            Depth::Rgb444 => 0x53,
        }
    }

    pub fn bits(self) -> u32 {
        match self {
            Depth::Rgb565 => 16,
            Depth::Rgb444 => 12,
        }
    }

    /// Bytes for `n` panel pixels.
    pub fn bytes(self, pixels: usize) -> usize {
        match self {
            Depth::Rgb565 => pixels * 2,
            Depth::Rgb444 => pixels * 3 / 2,
        }
    }
}

/// Where the time of one send went, in CPU cycles.
#[derive(Clone, Copy, Default, Debug)]
pub struct Timing {
    /// From the first command byte to CS going high after the last pixel.
    pub total: u32,
    /// The CPU filling strip buffers (palette lookups, doubling).
    pub fill: u32,
    /// The CPU standing still, waiting for a DMA transfer to end.
    pub wait: u32,
    /// The window commands (CASET, RASET, RAMWR) through the FIFO.
    pub window: u32,
    /// Pixel bytes sent.
    pub bytes: u32,
    /// DMA transfers (or FIFO writes) used for the pixels.
    pub transfers: u32,
    /// Cycle counter (a time stamp, not a duration) just before the first
    /// pixel transfer started: the first pixel byte goes out then.
    pub first_byte: Option<u32>,
    /// Cycle counter just after the last pixel transfer ended.
    pub last_byte: Option<u32>,
}

impl Timing {
    pub fn add(&mut self, other: &Timing) {
        self.total = self.total.wrapping_add(other.total);
        self.fill = self.fill.wrapping_add(other.fill);
        self.wait = self.wait.wrapping_add(other.wait);
        self.window = self.window.wrapping_add(other.window);
        self.bytes += other.bytes;
        self.transfers += other.transfers;
        // Stamps: the earlier send's first byte, the later one's last.
        self.first_byte = self.first_byte.or(other.first_byte);
        self.last_byte = other.last_byte.or(self.last_byte);
    }
}

/// How the strips are sent: the real pipeline and two slower ones to
/// compare with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Send {
    /// DMA, filling strip N+1 while strip N is on the wire.
    Overlap,
    /// DMA, but each strip is filled only after the previous one has
    /// left: no overlap.
    DmaInTurn,
    /// No DMA at all: the CPU pushes every byte through the 64-byte FIFO
    /// and waits for it (what mipidsi on esp-hal's blocking SPI does).
    Fifo,
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
    depth: Depth,
    /// What MADCTL (36h) holds in the panel; `None` when not known (before
    /// set-up, after a failed send, a reset or a raw 36h).
    madctl: Option<u8>,
    /// MADCTL commands sent since start, to see how often the ways switch.
    pub madctl_sends: u32,
}

impl Lcd {
    pub fn new(
        spi: SpiDma<'static, Blocking>,
        buf_a: DmaTxBuf,
        buf_b: DmaTxBuf,
        cs: Output<'static>,
        dc: Output<'static>,
    ) -> Self {
        let write_config = Self::write_config_for(40);
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
            write_mhz: 40,
            write_config,
            read_config,
            depth: Depth::Rgb565,
            madctl: None,
            madctl_sends: 0,
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

    /// Sets the drawing clock. The ESP32 can make 80 MHz (no divider) or
    /// 80/n: 40, 26.7, 20 ... MHz. 80 MHz needs the SPI's own pins (IO_MUX),
    /// which GPIO13/14 are.
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

    pub fn depth(&self) -> Depth {
        self.depth
    }

    /// One command and its parameters, through the FIFO. CS is left as
    /// it was: the caller decides.
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
        self.forget_madctl_on(command);
        self.cs.set_low();
        let result = self.command_bytes(command, params);
        self.cs.set_high();
        result
    }

    /// The set-up mipidsi does in the reference firmware, byte for byte:
    /// software reset, sleep out, MADCTL 60h (landscape), no inversion,
    /// COLMOD 55h (16-bit), normal mode, display on, with its waits.
    pub fn init(&mut self) -> Result<(), LcdError> {
        let delay = Delay::new();
        self.command(cmd::SWRESET, &[])?;
        delay.delay_millis(150);
        self.command(cmd::SLPOUT, &[])?;
        delay.delay_millis(10);
        self.set_madctl(MADCTL_LANDSCAPE)?;
        self.command(cmd::INVOFF, &[])?;
        self.command(cmd::COLMOD, &[Depth::Rgb565.colmod()])?;
        delay.delay_millis(10);
        self.command(cmd::NORON, &[])?;
        delay.delay_millis(10);
        self.command(cmd::DISPON, &[])?;
        // The datasheet asks for a pause after DISPON before data.
        delay.delay_millis(120);
        self.depth = Depth::Rgb565;
        Ok(())
    }

    /// A command that changes MADCTL behind [`Lcd::set_madctl`]'s back (a
    /// reset, or 36h itself) makes its value unknown, so the next window
    /// sends it again.
    fn forget_madctl_on(&mut self, command: u8) {
        if command == cmd::MADCTL || command == cmd::SWRESET {
            self.madctl = None;
        }
    }

    /// Puts `value` in MADCTL (36h), only if the panel does not hold it
    /// already. If sending fails the value is unknown, so it is sent
    /// again next time: no window opens under a wrong value.
    pub fn set_madctl(&mut self, value: u8) -> Result<(), LcdError> {
        if self.madctl == Some(value) {
            return Ok(());
        }
        // command() forgets the value first; it is known again only once
        // the send worked.
        self.command(cmd::MADCTL, &[value])?;
        self.madctl = Some(value);
        self.madctl_sends += 1;
        Ok(())
    }

    /// What MADCTL holds, if known.
    pub fn madctl(&self) -> Option<u8> {
        self.madctl
    }

    pub fn set_depth(&mut self, depth: Depth) -> Result<(), LcdError> {
        self.command(cmd::COLMOD, &[depth.colmod()])?;
        self.depth = depth;
        Ok(())
    }

    /// Opens a window (panel pixels, inclusive corners, landscape: MADCTL
    /// 60h) and starts a memory write: CASET, RASET, RAMWR, all through
    /// the FIFO, CS low from here until [`Lcd::end`].
    pub fn begin(&mut self, x0: u16, y0: u16, x1: u16, y1: u16) -> Result<(), LcdError> {
        self.begin_in(MADCTL_LANDSCAPE, x0, y0, x1, y1)
    }

    /// The same under MADCTL `madctl` (sent first if the panel holds
    /// another value). The corners are interface addresses: column
    /// (CASET) `x`, row (RASET) `y`.
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

    /// Goes on with a memory write that [`Lcd::end`] paused and another
    /// command interrupted: CS low, Write Memory Continue (3Ch), then
    /// pixels by [`Lcd::stream`] as after [`Lcd::begin`]. The data sheet
    /// says the panel then writes from the pixel after the last one.
    pub fn continue_write(&mut self) -> Result<(), LcdError> {
        self.cs.set_low();
        self.command_bytes(cmd::RAMWRC, &[])
    }

    /// Sets the porches (PORCTRL, B2h): `bpa` lines before the picture,
    /// `fpa` after it, each 1 to 127. The other three bytes stay at
    /// their reset values.
    pub fn set_porch(&mut self, bpa: u8, fpa: u8) -> Result<(), LcdError> {
        let mut p = PORCH_DEFAULT;
        p[0] = bpa;
        p[1] = fpa;
        self.command(cmd::PORCTRL, &p)
    }

    /// Get Scanline (45h), timed: the line the panel is refreshing, and
    /// the cycle counter just before the command byte and just after the
    /// last answer byte. Those two stamps are taken inside the read, so
    /// switching the clock before and after (most of the 24 to 83 us a
    /// read costs) does not blur them.
    pub fn scanline_stamped(&mut self) -> Result<(u16, u32, u32), LcdError> {
        let (raw, before, after) = self.scanline_stamped_raw()?;
        Ok((raw & SCANLINE_MASK, before, after))
    }

    /// The same with the whole 16-bit answer. Only the low 10 bits carry
    /// the line (a refresh has at most 575 lines); a set bit above them
    /// is a misread.
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

    /// Sends `strips` strips of pixel bytes after [`Lcd::begin`].
    /// `fill(k, buf)` writes strip `k` into `buf` and returns its length
    /// (a multiple of 4: the ESP32's DMA moves whole 32-bit words).
    ///
    /// With [`Send::Overlap`] strip k+1 is filled while strip k is on
    /// the wire; the time in `wait` is then what the wire costs beyond
    /// the CPU's own work.
    pub fn stream(
        &mut self,
        strips: usize,
        how: Send,
        timing: &mut Timing,
        mut fill: impl FnMut(usize, &mut [u8]) -> usize,
    ) -> Result<(), LcdError> {
        if strips == 0 {
            return Ok(());
        }
        if how == Send::Fifo {
            return self.stream_fifo(strips, timing, fill);
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
            // Start strip k.
            next.set_length(len);
            timing.bytes += len as u32;
            timing.transfers += 1;
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
            // Fill strip k+1 meanwhile (or, without overlap, afterwards).
            let mut next_len = 0;
            if how == Send::Overlap && k < strips {
                let t = cycles();
                next_len = fill(k, spare.as_mut_slice());
                timing.fill = timing.fill.wrapping_add(cycles().wrapping_sub(t));
            }
            let t = cycles();
            let (back, sent) = transfer.wait();
            let done = cycles();
            timing.wait = timing.wait.wrapping_add(done.wrapping_sub(t));
            timing.last_byte = Some(done);
            spi = back;
            next = spare;
            spare = sent;
            if k >= strips {
                break;
            }
            if how == Send::DmaInTurn {
                let t = cycles();
                next_len = fill(k, next.as_mut_slice());
                timing.fill = timing.fill.wrapping_add(cycles().wrapping_sub(t));
            }
            len = next_len;
        }
        self.spi = Some(spi);
        self.buf_a = Some(next);
        self.buf_b = Some(spare);
        result
    }

    /// The same strips with no DMA: the CPU writes them through the FIFO.
    fn stream_fifo(
        &mut self,
        strips: usize,
        timing: &mut Timing,
        mut fill: impl FnMut(usize, &mut [u8]) -> usize,
    ) -> Result<(), LcdError> {
        // A threshold above any length makes esp-hal use the FIFO.
        let fifo = self.write_config.with_min_async_transfer_size(usize::MAX);
        let mut buf = self.buf_a.take().expect("buffer A present");
        let mut spi = self.spi.take().expect("SPI present");
        let mut result = spi.apply_config(&fifo).map_err(LcdError::Clock);
        for k in 0..strips {
            if result.is_err() {
                break;
            }
            let t = cycles();
            let len = fill(k, buf.as_mut_slice());
            timing.fill = timing.fill.wrapping_add(cycles().wrapping_sub(t));
            let t = cycles();
            if k == 0 {
                timing.first_byte = Some(t);
            }
            result = spi.write(&buf.as_slice()[..len]).map_err(LcdError::Spi);
            let done = cycles();
            timing.wait = timing.wait.wrapping_add(done.wrapping_sub(t));
            timing.last_byte = Some(done);
            timing.bytes += len as u32;
            timing.transfers += 1;
        }
        let back = spi
            .apply_config(&self.write_config)
            .map_err(LcdError::Clock);
        self.spi = Some(spi);
        self.buf_a = Some(buf);
        result.and(back)
    }

    /// A whole window: begin, stream, end, timed (landscape, 60h).
    pub fn draw(
        &mut self,
        window: (u16, u16, u16, u16),
        strips: usize,
        how: Send,
        fill: impl FnMut(usize, &mut [u8]) -> usize,
    ) -> Result<Timing, LcdError> {
        self.draw_in(MADCTL_LANDSCAPE, window, strips, how, fill)
    }

    /// The same under MADCTL `madctl` (see [`Lcd::begin_in`]).
    pub fn draw_in(
        &mut self,
        madctl: u8,
        window: (u16, u16, u16, u16),
        strips: usize,
        how: Send,
        fill: impl FnMut(usize, &mut [u8]) -> usize,
    ) -> Result<Timing, LcdError> {
        let mut timing = Timing::default();
        let start = cycles();
        let result = self.begin_in(madctl, window.0, window.1, window.2, window.3);
        timing.window = cycles().wrapping_sub(start);
        let result = result.and_then(|()| self.stream(strips, how, &mut timing, fill));
        self.end();
        timing.total = cycles().wrapping_sub(start);
        result.map(|()| timing)
    }

    /// One of the strip buffers, for measuring the CPU side alone.
    pub fn strip_buffer(&mut self) -> &mut [u8] {
        self.buf_a
            .as_mut()
            .expect("buffer A present")
            .as_mut_slice()
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
    /// `params`, skips `dummy_bits`, reads `out.len()` bytes.
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
        // The SPI reads whole bytes. When the answer starts in the middle
        // of one, read a little more and shift it into place.
        let skip = usize::from(dummy_bits);
        let total = (skip + out.len() * 8).div_ceil(8);
        let mut bytes = [0u8; MAX + 33];
        // A raw 36h or reset from the PC: the value is not known any more.
        self.forget_madctl_on(command);
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

    /// Reads `width` panel pixels of row `y` from column `x` into
    /// `scratch` (at least `1 + 3 * width` bytes): a dummy byte, then
    /// red, green, blue for each pixel, each in the top 6 bits of its
    /// byte. Returns the pixel bytes (without the dummy byte).
    pub fn read_row<'s>(
        &mut self,
        x: u16,
        y: u16,
        width: u16,
        scratch: &'s mut [u8],
    ) -> Result<&'s [u8], LcdError> {
        self.read_row_in(MADCTL_LANDSCAPE, x, y, width, scratch)
    }

    /// The same under MADCTL `madctl`: `x` is the interface column
    /// (CASET), `y` the interface row (RASET).
    pub fn read_row_in<'s>(
        &mut self,
        madctl: u8,
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
        self.set_madctl(madctl)?;
        self.begin_read_window(x, y, x + width - 1, y)?;
        self.slowly(READ_KHZ, |lcd| {
            lcd.command_bytes(cmd::RAMRD, &[])?;
            lcd.spi().read(answer).map_err(LcdError::Spi)
        })?;
        Ok(&answer[1..])
    }

    fn begin_read_window(&mut self, x0: u16, y0: u16, x1: u16, y1: u16) -> Result<(), LcdError> {
        self.cs.set_low();
        let [a, b] = x0.to_be_bytes();
        let [c, d] = x1.to_be_bytes();
        let result = self
            .command_bytes(cmd::CASET, &[a, b, c, d])
            .and_then(|()| {
                let [a, b] = y0.to_be_bytes();
                let [c, d] = y1.to_be_bytes();
                self.command_bytes(cmd::RASET, &[a, b, c, d])
            });
        self.cs.set_high();
        result
    }
}

/// A panel pixel as read back (6 bits a channel) turned into RGB565, the
/// way screen-mirror does it.
#[inline]
pub fn read_to_565(rgb: &[u8]) -> u16 {
    let (r, g, b) = (u16::from(rgb[0]), u16::from(rgb[1]), u16::from(rgb[2]));
    ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)
}

/// A panel pixel as read back turned into RGB444 (top 4 of each 6 bits).
#[inline]
pub fn read_to_444(rgb: &[u8]) -> u16 {
    let (r, g, b) = (u16::from(rgb[0]), u16::from(rgb[1]), u16::from(rgb[2]));
    ((r >> 4) << 8) | ((g >> 4) << 4) | (b >> 4)
}
