//! The CYD as a `board_api::Board`: one field per part.
//!
//! | part   | here                                                     |
//! |--------|----------------------------------------------------------|
//! | screen | the ST7789, tear-free (screen.rs)                         |
//! | touch  | XPT2046 (feature `touch`), plus taps from the debug link  |
//! | keys   | BOOT (GPIO0)                                             |
//! | pad    | "cable pad": buttons sent over the debug link, and tour  |
//! | clock  | embassy-time, microseconds since start                   |
//! | files  | demo card (`demo-card`) or the microSD (`sd-card`)       |
//! | link   | absent: the debug link is the board's own (link.rs)      |
//! | log    | the log ring (logbuf.rs)                                 |
//! | store  | absent: nothing writes flash this round                  |

use board_api::{
    Absent, Board, Clock, DirEntry, Files, FsError, Keys, Pad, PadState, Parts, Touch, TouchPoint,
    key,
};
use esp_hal::gpio::Input;

use crate::logbuf::CydLog;
use crate::screen::CydScreen;

/// The card the console sees, by feature. With both `sd-card` and
/// `demo-card` on, the real card wins.
#[cfg(feature = "sd-card")]
pub type CardFiles = crate::sd::SdFiles;
#[cfg(all(feature = "demo-card", not(feature = "sd-card")))]
pub type CardFiles = crate::demo_card::DemoCard;
#[cfg(not(any(feature = "demo-card", feature = "sd-card")))]
pub type CardFiles = Absent;

pub struct CydBoard {
    pub screen: CydScreen,
    pub touch: CydTouch,
    pub keys: CydKeys,
    pub pad: CablePad,
    pub clock: CydClock,
    pub files: TimedFiles<CardFiles>,
    pub link: Absent,
    pub log: CydLog,
    pub store: Absent,
}

impl Board for CydBoard {
    type Screen = CydScreen;
    type Touch = CydTouch;
    type Keys = CydKeys;
    type Pad = CablePad;
    type Clock = CydClock;
    type Files = TimedFiles<CardFiles>;
    type Link = Absent;
    type Log = CydLog;
    type Store = Absent;

    fn parts(&mut self) -> Parts<'_, Self> {
        Parts {
            screen: &mut self.screen,
            touch: &mut self.touch,
            keys: &mut self.keys,
            pad: &mut self.pad,
            clock: &mut self.clock,
            files: &mut self.files,
            link: &mut self.link,
            log: &mut self.log,
            store: &mut self.store,
        }
    }
}

// ---- clock ------------------------------------------------------------

pub struct CydClock;

impl Clock for CydClock {
    fn now_us(&self) -> u64 {
        console_now()
    }
}

/// Lab: the console's clock can run faster than the real one (letter
/// `D`). At 2x the console asks for its 30 frames a second of ITS time,
/// which is 60 a second of real time, with the same work in each frame.
/// (real time at the last change, console time then, speed)
static CLOCK: critical_section::Mutex<core::cell::Cell<(u64, u64, u64)>> =
    critical_section::Mutex::new(core::cell::Cell::new((0, 0, 1)));

/// The console's time now, in its microseconds.
pub fn console_now() -> u64 {
    let (real0, console0, k) = critical_section::with(|cs| CLOCK.borrow(cs).get());
    console0 + (now_us() - real0) * k
}

/// A console time as real time (for sleeping until it).
pub fn to_real(t: u64) -> u64 {
    let (real0, console0, k) = critical_section::with(|cs| CLOCK.borrow(cs).get());
    if t == u64::MAX {
        u64::MAX
    } else if t <= console0 {
        real0
    } else {
        real0 + (t - console0) / k
    }
}

/// Sets how many times faster than real time the console's clock runs.
/// The console's time goes on smoothly from where it is.
pub fn set_clock_speed(k: u64) {
    let console = console_now();
    let real = now_us();
    critical_section::with(|cs| CLOCK.borrow(cs).set((real, console, k.max(1))));
}

pub fn clock_speed() -> u64 {
    critical_section::with(|cs| CLOCK.borrow(cs).get()).2
}

/// Microseconds since start (embassy-time ticks at 1 MHz here).
pub fn now_us() -> u64 {
    embassy_time::Instant::now().as_micros()
}

// ---- keys -------------------------------------------------------------

/// How long BOOT must be held to start or stop the tour.
pub const LONG_PRESS_US: u64 = 1_000_000;

/// The BOOT button (GPIO0, low while pressed).
///
/// With the `tour` feature, a press is only known to be short when it
/// ends, so the console gets it as one "click" after the release (one
/// input read with BOOT held, the next with it free), and a press of
/// [`LONG_PRESS_US`] or more goes to the tour instead. Without the
/// feature, the console sees the pin as it is.
pub struct CydKeys {
    pub pin: Input<'static>,
    /// When the press started, while BOOT is down.
    down_since: Option<u64>,
    /// The long press was already used.
    long_done: bool,
    /// A click waits for the console's next input read.
    click: bool,
}

/// What [`CydKeys::poll`] saw.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BootEvent {
    None,
    /// Held for [`LONG_PRESS_US`] (once per press).
    Long,
}

impl CydKeys {
    pub fn new(pin: Input<'static>) -> Self {
        Self {
            pin,
            down_since: None,
            long_done: false,
            click: false,
        }
    }

    pub fn down(&self) -> bool {
        self.pin.is_low()
    }

    /// Follows the pin; the loop calls it on every wake.
    pub fn poll(&mut self, now: u64) -> BootEvent {
        let down = self.down();
        match (self.down_since, down) {
            (None, true) => {
                self.down_since = Some(now);
                self.long_done = false;
            }
            (Some(t), true) => {
                if !self.long_done && now - t >= LONG_PRESS_US {
                    self.long_done = true;
                    return BootEvent::Long;
                }
            }
            (Some(_), false) => {
                self.down_since = None;
                // Only the tour build hands BOOT over as a click. Without
                // it nothing would ever take the click, and the loop
                // would see "input waiting" for ever and never sleep.
                if !self.long_done && cfg!(feature = "tour") {
                    self.click = true;
                }
            }
            (None, false) => {}
        }
        BootEvent::None
    }

    /// Whether a click waits for the console.
    pub fn click_waiting(&self) -> bool {
        self.click
    }
}

impl Keys for CydKeys {
    fn held(&mut self) -> u16 {
        if cfg!(feature = "tour") {
            if core::mem::take(&mut self.click) {
                key::BOOT
            } else {
                0
            }
        } else if self.down() {
            key::BOOT
        } else {
            0
        }
    }
}

// ---- pad ----------------------------------------------------------------

/// Buttons pressed over the debug link (`boardctl key`), or by the tour,
/// shown to the console as pad 0.
///
/// It reports "connected" from the first press on. A press shorter than
/// one console frame is still seen: buttons pressed since the last read
/// count as held for that read.
#[derive(Default)]
pub struct CablePad {
    connected: bool,
    held: u32,
    latched: u32,
}

impl CablePad {
    /// New held buttons, as `btn` bits.
    #[cfg_attr(not(any(feature = "link", feature = "tour")), allow(dead_code))]
    pub fn set(&mut self, buttons: u32) {
        self.connected = true;
        self.latched |= buttons & !self.held;
        self.held = buttons;
    }

    #[cfg_attr(not(feature = "link"), allow(dead_code))]
    pub fn connected(&self) -> bool {
        self.connected
    }

    /// Whether a press waits for the console's next read.
    pub fn waiting(&self) -> bool {
        self.latched != 0
    }
}

/// debug-link button bits to pad buttons. The link names its buttons the
/// Xbox way (A bottom, B right, X left, Y top); positions are what count.
/// Whether A or the right button confirms is the owner's decision
/// (undecided); the console's BUTTON_MAP decides, not this table.
#[cfg(feature = "link")]
pub fn link_to_pad(mask: u16) -> u32 {
    use board_api::btn;
    use debug_link::msg::button as b;
    const MAP: [(u16, u32); 12] = [
        (b::UP, btn::UP),
        (b::DOWN, btn::DOWN),
        (b::LEFT, btn::LEFT),
        (b::RIGHT, btn::RIGHT),
        (b::A, btn::SOUTH),
        (b::B, btn::EAST),
        (b::X, btn::WEST),
        (b::Y, btn::NORTH),
        (b::L, btn::L),
        (b::R, btn::R),
        (b::START, btn::PLUS),
        (b::SELECT, btn::MINUS),
    ];
    MAP.iter()
        .filter(|(l, _)| mask & l != 0)
        .fold(0, |acc, (_, p)| acc | p)
}

impl Pad for CablePad {
    fn read(&mut self, index: u8) -> PadState {
        if index != 0 || !self.connected {
            return PadState::NONE;
        }
        let buttons = self.held | core::mem::take(&mut self.latched);
        PadState {
            connected: true,
            buttons,
            ..PadState::NONE
        }
    }
}

// ---- touch ----------------------------------------------------------------

/// The touch screen, and taps sent over the debug link (`boardctl tap`).
pub struct CydTouch {
    #[cfg(feature = "touch")]
    pub chip: Option<crate::touch::Xpt2046>,
    /// A tap from the link: shown for one read, then lifted.
    tap: Option<TouchPoint>,
    /// The panel was pressed at the last read (to log once per touch).
    #[cfg(feature = "touch")]
    was_down: bool,
}

impl CydTouch {
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "touch")]
            chip: None,
            tap: None,
            #[cfg(feature = "touch")]
            was_down: false,
        }
    }

    #[cfg_attr(not(feature = "link"), allow(dead_code))]
    pub fn tap(&mut self, at: TouchPoint) {
        self.tap = Some(at);
    }

    pub fn tap_waiting(&self) -> bool {
        self.tap.is_some()
    }

    /// Whether a finger is on the panel (IRQ low).
    pub fn down(&self) -> bool {
        #[cfg(feature = "touch")]
        if let Some(chip) = &self.chip {
            return chip.pressed();
        }
        false
    }
}

impl Touch for CydTouch {
    fn read(&mut self) -> Option<TouchPoint> {
        if let Some(t) = self.tap.take() {
            return Some(t);
        }
        #[cfg(feature = "touch")]
        if let Some(chip) = &mut self.chip {
            let raw = chip.read();
            let point = raw.map(crate::touch::to_panel);
            if let (Some(r), Some(p), false) = (raw, point, self.was_down) {
                log::info!(
                    "touch raw_x={} raw_y={} z={} -> x={} y={} (calibration unproven)",
                    r.x,
                    r.y,
                    r.z,
                    p.x,
                    p.y
                );
            }
            self.was_down = point.is_some();
            return point;
        }
        None
    }
}

// ---- files ------------------------------------------------------------------

/// Any `Files`, with the time spent in it added up (for the scan and
/// card-check measurements).
pub struct TimedFiles<F> {
    pub inner: F,
    pub calls: u32,
    pub us: u64,
}

impl<F> TimedFiles<F> {
    pub fn new(inner: F) -> Self {
        Self {
            inner,
            calls: 0,
            us: 0,
        }
    }

    fn timed<T>(&mut self, f: impl FnOnce(&mut F) -> T) -> T {
        let t = now_us();
        let r = f(&mut self.inner);
        self.us += now_us() - t;
        self.calls += 1;
        r
    }
}

impl<F: Files> Files for TimedFiles<F> {
    fn list<E: FnMut(&DirEntry<'_>) -> bool>(&mut self, dir: &str, each: E) -> Result<(), FsError> {
        self.timed(|f| f.list(dir, each))
    }

    fn size(&mut self, path: &str) -> Result<u32, FsError> {
        self.timed(|f| f.size(path))
    }

    fn read_at(&mut self, path: &str, offset: u32, buf: &mut [u8]) -> Result<usize, FsError> {
        self.timed(|f| f.read_at(path, offset, buf))
    }
}
