//! The cable to the PC, the log, and saved settings.

/// The debug link's wire: UART0 (the USB cable) on the CYD, a network
/// socket or a fake serial port on the PC. Bytes only; the framing is in
/// crates/debug-link. Neither call waits.
pub trait Link {
    /// Copies bytes that have arrived into `buf`. Returns how many (0 when
    /// none are waiting).
    fn read(&mut self, buf: &mut [u8]) -> usize;
    /// Queues bytes to send. Returns how many were taken: fewer than
    /// `data.len()` when the board's send buffer is full.
    fn write(&mut self, data: &[u8]) -> usize;
}

/// Longest log line in bytes. Longer lines are cut by the board. (The
/// game interface uses the same limit.)
pub const LOG_LINE_MAX: usize = 200;

/// Text lines for a person: the serial monitor on the CYD, the terminal
/// on the PC.
pub trait LogOut {
    /// One line, without the line end. Must not wait long: a board that
    /// sends slowly keeps a queue and drops lines when it is full.
    fn line(&mut self, text: &str);
}

/// Size of one flash sector: the smallest piece that can be erased.
pub const SECTOR: u32 = 4096;

/// What went wrong with saved settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// The offset or length is outside the store, or not a multiple of 4.
    OutOfRange,
    /// The flash (or the PC file) gave an error.
    Io,
}

/// A small piece of flash kept for settings (a file on the PC).
///
/// It behaves like flash: an erase sets a whole sector to `0xFF`, and a
/// write can only turn bits from 1 to 0. Offsets count from the start of
/// the store, not of the chip. (crates/pad-store has a trait of the same
/// shape with offsets from the start of the chip; a 15-line adapter that
/// adds the store's start joins the two.)
pub trait Store {
    /// Size of the store in bytes: a whole number of [`SECTOR`]s.
    fn size(&self) -> u32;
    /// Reads `buf.len()` bytes at `offset`.
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), StoreError>;
    /// Writes `data` at `offset`. Offset and length are multiples of 4.
    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), StoreError>;
    /// Erases the sector that starts at `offset` (a multiple of [`SECTOR`]).
    fn erase_sector(&mut self, offset: u32) -> Result<(), StoreError>;
}
