//! The log: lines go into a ring buffer in memory and leave through the
//! serial port (UART0) a little at a time, from the board's loop.
//!
//! Why not write straight to the port: at 115,200 baud one 100-letter
//! line takes 9 ms, and the console's `LogOut::line` must not wait that
//! long. So a line is only copied here (a few microseconds). The loop
//! then puts as much into the port's 128-byte hardware queue as fits,
//! without waiting, and comes back about every 5 ms while text waits.
//!
//! When the ring is full, a new line is dropped whole, and counted
//! (`dropped` in `boardctl stats`). A crash message (panic) does not use
//! the ring: esp-backtrace writes it to the port directly.
//!
//! Debug-link frames are sent by the same loop, between two drains, so a
//! log line never lands inside a frame.

use core::cell::RefCell;
use core::fmt::Write as _;

use critical_section::Mutex;
use esp_hal::{Async, uart::Uart};

/// Size of the ring, in bytes (static memory).
pub const RING_BYTES: usize = 4096;
/// Longest line; longer lines are cut (the `mem` line is about 260).
const LINE_MAX: usize = 320;

struct Ring {
    bytes: [u8; RING_BYTES],
    /// Next byte to send, and how many wait.
    head: usize,
    len: usize,
    dropped: u32,
}

impl Ring {
    const fn new() -> Self {
        Self {
            bytes: [0; RING_BYTES],
            head: 0,
            len: 0,
            dropped: 0,
        }
    }

    /// Adds one whole line, or drops it when it does not fit.
    fn push(&mut self, line: &[u8]) {
        if line.len() > RING_BYTES - self.len {
            self.dropped += 1;
            return;
        }
        let mut at = (self.head + self.len) % RING_BYTES;
        for &b in line {
            self.bytes[at] = b;
            at = (at + 1) % RING_BYTES;
        }
        self.len += line.len();
    }

    /// The waiting bytes up to the end of the buffer (the rest follows
    /// from the start).
    fn front(&self) -> &[u8] {
        let end = (self.head + self.len).min(RING_BYTES);
        &self.bytes[self.head..end]
    }

    fn consume(&mut self, n: usize) {
        self.head = (self.head + n) % RING_BYTES;
        self.len -= n;
    }
}

static RING: Mutex<RefCell<Ring>> = Mutex::new(RefCell::new(Ring::new()));

/// A line being put together on the stack before it goes in the ring.
struct Line {
    buf: [u8; LINE_MAX],
    len: usize,
}

impl core::fmt::Write for Line {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        // Room for the line end is kept.
        let room = LINE_MAX - 2 - self.len;
        let n = s.len().min(room);
        self.buf[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
        Ok(())
    }
}

fn push_line(write: impl FnOnce(&mut Line)) {
    let mut line = Line {
        buf: [0; LINE_MAX],
        len: 0,
    };
    write(&mut line);
    line.buf[line.len] = b'\r';
    line.buf[line.len + 1] = b'\n';
    line.len += 2;
    critical_section::with(|cs| RING.borrow_ref_mut(cs).push(&line.buf[..line.len]));
}

/// The `log` crate's output (`info!` and the others).
struct Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        push_line(|l| {
            let _ = write!(l, "{} - {}", record.level(), record.args());
        });
    }

    fn flush(&self) {}
}

static LOGGER: Logger = Logger;

/// Makes `info!` and the others go to the ring. Call once, first.
pub fn init() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}

/// The console's log (`board_api::LogOut`): same ring, same format.
pub struct CydLog;

impl board_api::LogOut for CydLog {
    fn line(&mut self, text: &str) {
        push_line(|l| {
            let _ = write!(l, "INFO - {text}");
        });
    }
}

/// Whether text waits to be sent.
pub fn pending() -> bool {
    critical_section::with(|cs| RING.borrow_ref(cs).len > 0)
}

/// Lines dropped because the ring was full, since start.
pub fn dropped() -> u32 {
    critical_section::with(|cs| RING.borrow_ref(cs).dropped)
}

/// Moves as much waiting text into the port's hardware queue as fits,
/// without waiting.
pub fn drain(uart: &mut Uart<'static, Async>) {
    critical_section::with(|cs| {
        let mut ring = RING.borrow_ref_mut(cs);
        while ring.len > 0 && uart.write_ready() {
            match uart.write(ring.front()) {
                Ok(n) if n > 0 => ring.consume(n),
                _ => break,
            }
        }
    });
}

/// Sends everything waiting, waiting as long as needed (before a shot
/// changes the port's speed).
pub fn flush(uart: &mut Uart<'static, Async>) {
    while pending() {
        drain(uart);
    }
    let _ = uart.flush();
}
