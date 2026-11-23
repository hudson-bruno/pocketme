//! The serial log.
//!
//! Lines are not printed where they are made. They go into a pipe (a ring
//! of bytes) and a task of their own prints them, 64 bytes at a time,
//! waiting after each piece as long as it takes to leave the serial port
//! (about 87 microseconds a byte at 115200 baud). esp-println writes with
//! interrupts off, so printing a long line in one go would hold up the
//! radio. This way the radio loop never waits for the serial port: when
//! the pipe is full, the line is dropped and counted (`log_dropped` in the
//! MEM line).

use core::cell::Cell;
use core::fmt::{self, Write as _};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use critical_section::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::pipe::Pipe;
use embassy_time::{Duration, Timer};
use heapless::String;
use pad_proto::hci::BdAddr;

/// 16 KB: room for about 1.4 s of output at full serial speed, enough for
/// the bursts (start-up, the SDP answer, the descriptor).
const PIPE_SIZE: usize = 16 * 1024;
/// Longest line; longer ones are cut.
const LINE: usize = 400;

static PIPE: Pipe<CriticalSectionRawMutex, PIPE_SIZE> = Pipe::new();
static DROPPED: AtomicU32 = AtomicU32::new(0);
/// Set while a screenshot owns the serial port (see `pause`).
static PAUSED: AtomicBool = AtomicBool::new(false);

/// Stops (true) or restarts (false) the printing. While stopped, lines
/// wait in the pipe (and are dropped and counted when it is full). Used
/// while a screenshot goes out on the same wire, so that no log text
/// lands in the middle of a picture frame or goes out at the wrong
/// speed. The printer never stops in the middle of a piece: a piece is
/// written without waiting, so when anyone else runs, it is whole.
pub fn pause(on: bool) {
    PAUSED.store(on, Ordering::Relaxed);
}

/// Lines dropped because the pipe was full.
pub fn dropped() -> u32 {
    DROPPED.load(Ordering::Relaxed)
}

/// An address to hide in every line (the SDP test peer): the address
/// itself must never reach a log file outside `.local/`.
static HIDE: Mutex<Cell<Option<BdAddr>>> = Mutex::new(Cell::new(None));

/// Sets the address to hide. Call before anything is logged.
pub fn hide(addr: BdAddr) {
    critical_section::with(|cs| HIDE.borrow(cs).set(Some(addr)));
}

/// Writes one line: the tag, a space, the text, a line end.
pub fn line(tag: &str, args: fmt::Arguments<'_>) {
    let mut l: String<LINE> = String::new();
    let _ = l.push_str(tag);
    let _ = l.push(' ');
    // A line that does not fit is cut short (the writer stops at the end).
    let _ = CutWriter(&mut l).write_fmt(args);
    if let Some(a) = critical_section::with(|cs| HIDE.borrow(cs).get()) {
        hide_addr(&mut l, a);
    }
    let _ = l.push('\n');
    let mut b = l.as_bytes();
    // The whole line goes in, or none of it. The check and the writes run
    // with interrupts off, so no other writer can slip in between.
    critical_section::with(|_| {
        if PIPE.free_capacity() < b.len() {
            DROPPED.fetch_add(1, Ordering::Relaxed);
            return;
        }
        // The pipe is a ring: one write stops at the end of its memory and
        // the rest must go in with a second write. (Writing once lost the
        // end of a line now and then.)
        while !b.is_empty() {
            match PIPE.try_write(b) {
                Ok(n) => b = &b[n..],
                Err(_) => {
                    DROPPED.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            }
        }
    });
}

/// Prints the pipe for ever.
#[embassy_executor::task]
pub async fn printer() -> ! {
    let mut buf = [0_u8; 64];
    loop {
        let n = PIPE.read(&mut buf).await;
        while PAUSED.load(Ordering::Relaxed) {
            Timer::after(Duration::from_millis(5)).await;
        }
        esp_println::Printer::write_bytes(&buf[..n]);
        Timer::after(Duration::from_micros(n as u64 * 90 + 100)).await;
    }
}

/// Writes into a fixed string, keeping what fits.
struct CutWriter<'a>(&'a mut String<LINE>);

impl fmt::Write for CutWriter<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            // Leave one byte for the line end.
            if self.0.len() + c.len_utf8() >= LINE {
                return Ok(());
            }
            let _ = self.0.push(c);
        }
        Ok(())
    }
}

/// Replaces the address, as text ("AA:BB:..") and as the raw bytes in hex
/// (how it appears in command parameters), with "<peer>". Case is ignored.
fn hide_addr(l: &mut String<LINE>, a: BdAddr) {
    let mut text: String<17> = String::new();
    let _ = write!(text, "{a}");
    let mut raw: String<12> = String::new();
    for b in a.0 {
        let _ = write!(raw, "{b:02x}");
    }
    for pat in [text.as_str(), raw.as_str()] {
        while let Some(at) = find_nocase(l.as_str(), pat) {
            let mut n: String<LINE> = String::new();
            let _ = n.push_str(&l[..at]);
            let _ = n.push_str("<peer>");
            let _ = n.push_str(&l[at + pat.len()..]);
            *l = n;
        }
    }
}

fn find_nocase(hay: &str, pat: &str) -> Option<usize> {
    let (h, p) = (hay.as_bytes(), pat.as_bytes());
    if p.is_empty() || h.len() < p.len() {
        return None;
    }
    (0..=h.len() - p.len()).find(|&i| h[i..i + p.len()].eq_ignore_ascii_case(p))
}
