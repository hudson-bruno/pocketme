//! The CYD as a `board_api::Board`: one field per part.
//!
//! | part   | here                                                     |
//! |--------|----------------------------------------------------------|
//! | screen | the ST7789, tear-free (screen.rs)                         |
//! | touch  | XPT2046 (feature `touch`), plus taps from the debug link  |
//! | keys   | BOOT (GPIO0)                                             |
//! | pad    | the gamepad (feature `pad`, pad-host), and the "cable    |
//! |        | pad": buttons sent over the debug link, and tour         |
//! | clock  | embassy-time, microseconds since start                   |
//! | files  | demo card (`demo-card`) or the microSD (`sd-card`)       |
//! | link   | absent: the debug link is the board's own (link.rs)      |
//! | log    | the log ring (logbuf.rs)                                 |
//! | store  | the settings partition of the flash (feature `store`,    |
//! |        | flash_store.rs); absent without it                       |
//! | games  | the game loader on the two real slots (games.rs); while   |
//! |        | no game is loaded, the data slot is lent to home          |

use board_api::{
    Absent, Board, Clock, DirEntry, Files, FsError, GameCallbacks, Keys, Pad, PadState, Parts,
    StartError, Touch, TouchPoint, key,
};
use esp_hal::gpio::{Event, Flex};

use crate::logbuf::CydLog;
use touch_filter::{Reading, TouchCounters, TouchFilter};

/// The card the console sees, by feature. With both `sd-card` and
/// `demo-card` on, the real card wins.
#[cfg(feature = "sd-card")]
pub type CardFiles = crate::sd::SdFiles;
#[cfg(all(feature = "demo-card", not(feature = "sd-card")))]
pub type CardFiles = crate::demo_card::DemoCard;
#[cfg(not(any(feature = "demo-card", feature = "sd-card")))]
pub type CardFiles = Absent;

/// The settings' piece of flash, by feature.
#[cfg(feature = "store")]
pub type CydStore = crate::flash_store::CydStore;
#[cfg(not(feature = "store"))]
pub type CydStore = Absent;

/// The screen part, by feature: the screen itself, or (with `core2`)
/// the first core's door to the screen the second core owns.
#[cfg(not(feature = "core2"))]
pub type BoardScreen = crate::screen::CydScreen;
#[cfg(feature = "core2")]
pub type BoardScreen = crate::core2::Core2Screen;

pub struct CydBoard {
    pub screen: BoardScreen,
    pub touch: CydTouch,
    pub keys: CydKeys,
    pub pad: CydPad,
    pub clock: CydClock,
    pub files: TimedFiles<CardFiles>,
    pub link: Absent,
    pub log: CydLog,
    pub store: CydStore,
    pub games: crate::games::GameSlots,
}

impl Board for CydBoard {
    type Screen = BoardScreen;
    type Touch = CydTouch;
    type Keys = CydKeys;
    type Pad = CydPad;
    type Clock = CydClock;
    type Files = TimedFiles<CardFiles>;
    type Link = Absent;
    type Log = CydLog;
    type Store = CydStore;

    fn parts(&mut self) -> Parts<'_, Self> {
        self.parts_and_lent().0
    }

    /// The parts, and the empty game data slot while no game is loaded
    /// (games.rs, `GameSlots::lend`): home keeps its list and tile
    /// pictures there.
    fn parts_and_lent(&mut self) -> (Parts<'_, Self>, Option<&mut [u8]>) {
        (
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
            },
            self.games.lend(),
        )
    }

    fn start_game(&mut self, folder: &str) -> Result<GameCallbacks, StartError> {
        self.games.start(&mut self.files, folder)
    }

    fn end_game(&mut self) {
        self.games.end();
    }
}

// ---- clock ------------------------------------------------------------

pub struct CydClock;

impl Clock for CydClock {
    fn now_us(&self) -> u64 {
        now_us()
    }
}

/// Microseconds since start (embassy-time ticks at 1 MHz here).
pub fn now_us() -> u64 {
    embassy_time::Instant::now().as_micros()
}

// ---- keys -------------------------------------------------------------

/// How long BOOT must be held to start or stop the tour.
pub const LONG_PRESS_US: u64 = 1_000_000;

/// After BOOT was seen let go, falling edges for this long are the
/// contact bouncing, not a new press.
pub const BOUNCE_US: u64 = 30_000;

/// The BOOT button (GPIO0, low while pressed).
///
/// [`CydKeys::poll`] is the one place BOOT is looked at: the loop calls
/// it before every step. It reads the pin's level and its falling-edge
/// latch (the GPIO interrupt status bit, set by the hardware on the
/// edge even while the loop is busy) together, and keeps both for the
/// console: `held()` gives the level as poll saw it, `take_pressed()`
/// the press noted there. So the level and the note always describe the
/// same moment, and a press is never counted twice (board-api, "Rule
/// for a board that notes presses").
///
/// With the `tour` feature, a press is only known to be short when it
/// ends: the console gets it as a press noted at the release (and never
/// as held), and a press of [`LONG_PRESS_US`] or more goes to the tour
/// instead. Without the feature, the press is noted on its falling edge
/// and `held()` follows the (debounced) pin.
pub struct CydKeys {
    /// A `Flex` (input with pull-up) so that the measuring letter `k`
    /// can pull the pin low itself for a moment (open drain: it never
    /// drives the pin high, so it cannot fight the button).
    pub pin: Flex<'static>,
    /// BOOT is down (debounced), as poll last saw it.
    is_down: bool,
    /// When the press started, while BOOT is down.
    down_since: u64,
    /// When BOOT was last seen let go.
    up_since: Option<u64>,
    /// The long press was already used.
    long_done: bool,
    /// The press note for the console (`key` bits), until it takes it.
    note: u16,
    /// The edge the loop's wait saw (the wait clears the latch).
    edge_seen: bool,
    pub counters: KeyCounters,
}

/// BOOT, counted: edges latched, presses noted, and presses the console
/// got by its own rule (a held level that was not held at its last
/// read, or a note).
#[derive(Clone, Copy, Default)]
pub struct KeyCounters {
    pub edges: u32,
    pub bounces: u32,
    pub notes: u32,
    pub console_presses: u32,
    /// What `held()` gave last, for the rule above.
    last_held: u16,
}

/// What [`CydKeys::poll`] saw.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BootEvent {
    None,
    /// Held for [`LONG_PRESS_US`] (once per press).
    Long,
}

impl KeyCounters {
    /// Back to zero, but the console's last read stays known.
    pub fn reset(&mut self) {
        *self = Self {
            last_held: self.last_held,
            ..Self::default()
        };
    }
}

impl CydKeys {
    pub fn new(pin: Flex<'static>) -> Self {
        let mut keys = Self {
            pin,
            is_down: false,
            down_since: 0,
            up_since: None,
            long_done: false,
            note: 0,
            edge_seen: false,
            counters: KeyCounters::default(),
        };
        keys.arm();
        keys
    }

    /// Starts the edge latch (again): after the loop's wait, which turns
    /// it off.
    pub fn arm(&mut self) {
        self.pin.listen(Event::FallingEdge);
    }

    /// The loop's wait ended on a falling edge of BOOT.
    pub fn edge(&mut self) {
        self.edge_seen = true;
    }

    /// BOOT is down (debounced, as the last poll saw it).
    pub fn down(&self) -> bool {
        self.is_down
    }

    /// Looks at BOOT: the level and the edge latch, read together. The
    /// loop calls it on every wake, before the step.
    pub fn poll(&mut self, now: u64) -> BootEvent {
        let latched = self.pin.is_interrupt_set();
        if latched {
            self.pin.clear_interrupt();
        }
        let edge = core::mem::take(&mut self.edge_seen) || latched;
        let level_down = self.pin.is_low();
        if edge {
            self.counters.edges += 1;
        }
        // A press: an edge or a low level while up, unless the contact is
        // still bouncing from the last release.
        let bouncing = self
            .up_since
            .is_some_and(|t| now.saturating_sub(t) < BOUNCE_US);
        if !self.is_down && (edge || level_down) {
            if bouncing {
                self.counters.bounces += 1;
            } else {
                self.is_down = true;
                self.down_since = now;
                self.long_done = false;
                if !cfg!(feature = "tour") {
                    self.note |= key::BOOT;
                    self.counters.notes += 1;
                }
            }
        }
        let mut event = BootEvent::None;
        if self.is_down {
            if level_down {
                if !self.long_done && now - self.down_since >= LONG_PRESS_US {
                    self.long_done = true;
                    event = BootEvent::Long;
                }
            } else {
                // Let go (a press shorter than the time between two polls
                // gets here in the same poll that saw its edge).
                self.is_down = false;
                self.up_since = Some(now);
                // Only the tour build hands BOOT over at the release.
                if !self.long_done && cfg!(feature = "tour") {
                    self.note |= key::BOOT;
                    self.counters.notes += 1;
                }
            }
        }
        if !self.pin.is_listening() {
            self.arm();
        }
        event
    }

    /// Whether a press waits for the console.
    pub fn note_waiting(&self) -> bool {
        self.note != 0
    }

    /// What `held()` gives now.
    fn level_for_console(&self) -> u16 {
        if !cfg!(feature = "tour") && self.is_down {
            key::BOOT
        } else {
            0
        }
    }

    /// Whether the console would see something new: a note, or a level
    /// that differs from what it last read.
    pub fn changed(&self) -> bool {
        self.note != 0 || self.level_for_console() != self.counters.last_held
    }

    /// Measuring only (letter `k`): pulls BOOT low for `us` microseconds
    /// as if pressed, then lets go. Open drain: the pin is only ever
    /// pulled low (as the button does), never driven high. GPIO0 is a
    /// start-up strapping pin, but it is only read at reset.
    pub fn pulse(&mut self, us: u32) {
        self.pin.set_low();
        self.pin.set_output_enable(true);
        esp_hal::delay::Delay::new().delay_micros(us);
        self.pin.set_output_enable(false);
        self.pin.set_high();
    }
}

impl Keys for CydKeys {
    fn held(&mut self) -> u16 {
        let held = self.level_for_console();
        // The console's rule for a press: held now and not at its last
        // read (the note is added in take_pressed).
        if held & !self.counters.last_held != 0 {
            self.counters.console_presses += 1;
        }
        self.counters.last_held = held;
        held
    }

    fn take_pressed(&mut self) -> u16 {
        let note = core::mem::take(&mut self.note);
        // Counted once per press: a note of a button that `held()` just
        // reported as newly down was counted there already (the console
        // ORs the two).
        if note != 0 && note & self.counters.last_held == 0 {
            self.counters.console_presses += 1;
        }
        note
    }
}

// ---- pad ----------------------------------------------------------------

/// Buttons pressed over the debug link (`boardctl key`), or by the tour,
/// shown to the console as pad 0.
///
/// It reports "connected" from the first press on. `read` gives the
/// buttons held now; every press since the console's last look is noted
/// and handed over by `take_pressed`, so a press shorter than one console
/// frame is seen exactly once. Both change only between steps (the link
/// is served by the same task), so they always describe the same moment.
#[derive(Default)]
pub struct CablePad {
    connected: bool,
    held: u32,
    latched: u32,
    /// What the console got from its last `read`.
    given: u32,
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

    /// One short press of `buttons` (as `btn` bits), on top of what is
    /// held: noted for the console's next look.
    #[cfg_attr(not(feature = "link"), allow(dead_code))]
    pub fn press_once(&mut self, buttons: u32) {
        self.connected = true;
        self.latched |= buttons & !self.held;
    }

    /// Whether the console would see something new: a press, or held
    /// buttons that differ from what it last read.
    pub fn changed(&self) -> bool {
        self.latched != 0 || self.held != self.given
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
        self.given = self.held;
        PadState {
            connected: true,
            buttons: self.held,
            ..PadState::NONE
        }
    }

    fn take_pressed(&mut self, index: u8) -> u32 {
        if index != 0 || !self.connected {
            return 0;
        }
        core::mem::take(&mut self.latched)
    }
}

/// Pad 0 as the console sees it: the gamepad (feature `pad`, read from
/// pad-host's state, which the radio's poll keeps up to date) and the
/// cable pad together. Pads 1 to 3 (players 2 to 4) are cable pads only
/// (`others`): no Bluetooth pad reaches them yet.
///
/// - `read`: the gamepad's state while it is connected (with the cable's
///   held buttons added), else the cable pad's; `PadState::NONE` when
///   neither is there. A gamepad that goes gives NONE at the next look:
///   that is the console's "disconnected".
/// - `take_pressed`: both pads' presses since the last look.
/// - `motion`: the gamepad's, when its mode sends motion.
/// - [`CydPad::changed`]: whether the console would see something new;
///   for the gamepad that is pad-host's wake rule (a button, a stick past
///   its dead zone, a connection or a disconnection; never a resting
///   pad's noise, battery news or the report counter), noted by the radio
///   in [`CydPad::wake`] and cleared by [`CydPad::stepped`] after each
///   step.
#[derive(Default)]
pub struct CydPad {
    pub cable: CablePad,
    /// Pads 1 to 3 (the game's players 2 to 4): cable pads only, set over
    /// the debug link (`boardctl key --player N`). The gamepad radio is
    /// pad 0's alone; with no link these never connect.
    pub others: [CablePad; 3],
    #[cfg(feature = "pad")]
    pub host: Option<&'static mut pad_host::PadHost>,
    /// pad-host said the console must see something (cleared after a
    /// step).
    #[cfg(feature = "pad")]
    pub wake: bool,
}

impl CydPad {
    /// Whether the console would see something new.
    pub fn changed(&self) -> bool {
        #[cfg(feature = "pad")]
        if self.wake {
            return true;
        }
        self.cable.changed() || self.others.iter().any(CablePad::changed)
    }

    /// The cable pad of `player` (0 to 3; the caller checks the range).
    #[cfg(feature = "link")]
    pub fn cable_mut(&mut self, player: u8) -> &mut CablePad {
        match player {
            0 => &mut self.cable,
            n => &mut self.others[usize::from(n) - 1],
        }
    }

    /// The console stepped: what woke it was seen.
    pub fn stepped(&mut self) {
        #[cfg(feature = "pad")]
        {
            self.wake = false;
        }
    }

    /// Whether a pad (the gamepad or the cable pad) is connected.
    #[cfg_attr(not(feature = "link"), allow(dead_code))]
    pub fn connected(&self) -> bool {
        #[cfg(feature = "pad")]
        if self.host.as_ref().is_some_and(|h| h.connected()) {
            return true;
        }
        self.cable.connected()
    }

    /// The gamepad host, when it runs.
    #[cfg(feature = "pad")]
    fn host(&self) -> Option<&pad_host::PadHost> {
        self.host.as_deref().filter(|h| h.connected())
    }
}

impl Pad for CydPad {
    fn read(&mut self, index: u8) -> PadState {
        if let Some(other) = self.others.get_mut(usize::from(index).wrapping_sub(1)) {
            return other.read(0);
        }
        let cable = self.cable.read(index);
        #[cfg(feature = "pad")]
        if index == 0
            && let Some(h) = self.host()
        {
            let mut p = h.read();
            p.buttons |= cable.buttons;
            return p;
        }
        cable
    }

    fn motion(&mut self, index: u8) -> Option<board_api::Motion> {
        #[cfg(feature = "pad")]
        if index == 0 {
            return self.host().and_then(|h| h.motion());
        }
        let _ = index;
        None
    }

    fn take_pressed(&mut self, index: u8) -> u32 {
        if let Some(other) = self.others.get_mut(usize::from(index).wrapping_sub(1)) {
            return other.take_pressed(0);
        }
        let cable = self.cable.take_pressed(index);
        #[cfg(feature = "pad")]
        if index == 0
            && let Some(h) = self.host.as_deref_mut()
        {
            // Taken even when it went meanwhile: a press before a
            // disconnection is not left for the next pad.
            let p = h.take_pressed();
            if h.connected() {
                return cable | p;
            }
        }
        cable
    }
}

// ---- touch ----------------------------------------------------------------

/// Gap between two chip reads at least (a burst of link bytes wakes the
/// loop many times; the chip need not be read each time).
const READ_GAP_US: u64 = 4_000;

/// The touch screen, and the debug link's taps and scripted finger.
///
/// [`CydTouch::sample`] is the one place the panel is read: the loop
/// calls it before every step, and every frame while a touch goes on.
/// The reading goes through the filter (crates/touch-filter); `read()` gives
/// the filtered point as sample left it, and `take_tap()` where a touch
/// started since the console's last look (noted by sample, or a link
/// tap). Both are set in the same sample, so they agree.
pub struct CydTouch {
    #[cfg(feature = "touch")]
    pub chip: Option<crate::touch::Xpt2046>,
    pub filter: TouchFilter,
    pub counters: TouchCounters,
    /// The point as the last sample left it (the console's `read`).
    now: Option<TouchPoint>,
    /// Where a touch started since the console's last look.
    tap_note: Option<TouchPoint>,
    /// What the console got from its last `read`.
    given: Option<TouchPoint>,
    /// The scripted finger of the debug link (`T` letters), and the
    /// noise added to its every reading (panel pixels, 0 = none).
    finger: Option<TouchPoint>,
    finger_was: bool,
    noise: u8,
    rng: u32,
    last_read_us: Option<u64>,
    /// The chip's interrupt line fell since the last look (the loop's
    /// wait saw it).
    #[cfg(feature = "touch")]
    edge_seen: bool,
}

impl CydTouch {
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "touch")]
            chip: None,
            filter: TouchFilter::new(crate::lcd::W as u16, crate::lcd::H as u16),
            counters: TouchCounters::default(),
            now: None,
            tap_note: None,
            given: None,
            finger: None,
            finger_was: false,
            noise: 0,
            rng: 0x2545_F491,
            last_read_us: None,
            #[cfg(feature = "touch")]
            edge_seen: false,
        }
    }

    /// A tap from the link: noted as a touch that started and ended
    /// since the console's last look (it never shows in `read`).
    #[cfg_attr(not(feature = "link"), allow(dead_code))]
    pub fn tap(&mut self, at: TouchPoint) {
        self.tap_note.get_or_insert(at);
    }

    /// The link's scripted finger: down (or moved) at `at`, or lifted.
    pub fn finger(&mut self, at: Option<TouchPoint>) {
        self.finger = at;
        // Read it at the next sample, however soon.
        self.last_read_us = None;
    }

    /// Noise for the scripted finger's readings, panel pixels.
    pub fn set_noise(&mut self, px: u8) {
        self.noise = px;
    }

    /// The loop's wait ended on a falling edge of the touch line.
    #[cfg(feature = "touch")]
    pub fn edge(&mut self) {
        self.edge_seen = true;
        self.last_read_us = None;
    }

    /// Whether the console should look again soon: a tap waits, or the
    /// point differs from what it last read.
    pub fn changed(&self) -> bool {
        self.tap_note.is_some() || self.now != self.given
    }

    /// Whether the loop should keep reading every frame.
    pub fn down(&self) -> bool {
        #[cfg(feature = "touch")]
        if let Some(chip) = &self.chip
            && chip.pressed()
        {
            return true;
        }
        self.filter.down() || self.finger.is_some()
    }

    /// Reads the panel (or the scripted finger) once, through the filter,
    /// and logs a touch's end.
    pub fn sample(&mut self, now: u64) {
        if self
            .last_read_us
            .is_some_and(|t| now.saturating_sub(t) < READ_GAP_US)
        {
            return;
        }
        let link = self.finger.is_some() || self.finger_was;
        let reading = if let Some(p) = self.finger {
            Reading::Firm(self.noisy(p), 1000)
        } else if self.finger_was {
            Reading::None
        } else {
            self.read_chip(now)
        };
        self.finger_was = self.finger.is_some();
        self.last_read_us = Some(now);
        let fed = self.filter.feed(now, reading, link, &mut self.counters);
        self.now = self.filter.point();
        if fed.started {
            // The note: where the touch started (the console uses it only
            // if the touch is already over when it looks).
            if let Some(p) = self.now {
                self.tap_note.get_or_insert(p);
            }
            #[cfg(feature = "touch")]
            if !link && let Some(chip) = &self.chip {
                let r = chip.last_raw();
                if let Some(p) = self.now {
                    // "after": since the last lift, ms (a lost contact
                    // comes back within a few ms; a second tap later).
                    log::info!(
                        "touch raw_x={} raw_y={} z={} -> x={} y={} after={} (calibration unproven)",
                        r.x,
                        r.y,
                        r.z,
                        p.x,
                        p.y,
                        fed.after_lift_us
                            .map_or(-1, |us| (us / 1000).min(99_999) as i64),
                    );
                }
            }
        }
        if let Some((life, kind)) = fed.ended {
            log::info!(
                "touch end: {} ms, {} reads, {} drop-outs, max_jump={} rest_jitter_max={} travel={} px, from {},{}, {}{} jump_at={} spikes={}",
                (now - life.start_us) / 1000,
                life.reads,
                life.dropouts,
                life.max_jump,
                life.rest_jitter_max,
                life.travel,
                life.from.x,
                life.from.y,
                kind.name(),
                if life.link { " (link)" } else { "" },
                life.max_jump_at,
                life.spikes,
            );
        }
    }

    #[cfg(feature = "touch")]
    fn read_chip(&mut self, _now: u64) -> Reading {
        let Some(chip) = &mut self.chip else {
            return Reading::None;
        };
        let latched = chip.irq.is_interrupt_set();
        let edge = core::mem::take(&mut self.edge_seen) || latched;
        if !chip.pressed() && !self.filter.down() {
            if edge {
                // The line fell, and the finger was gone before we looked.
                self.counters.edges_unread += 1;
                chip.irq.clear_interrupt();
            }
            chip.irq.listen(Event::FallingEdge);
            return Reading::None;
        }
        let t0 = xtensa_lx::timer::get_cycle_count();
        let r = chip.sample(self.filter.z_limit());
        let us = xtensa_lx::timer::get_cycle_count().wrapping_sub(t0) / crate::lcd::CPU_MHZ;
        if r != Reading::None {
            self.counters.add_read_time(u64::from(us));
        }
        // Our own conversions can make the line fall: forget them.
        chip.irq.clear_interrupt();
        chip.irq.listen(Event::FallingEdge);
        r
    }

    #[cfg(not(feature = "touch"))]
    fn read_chip(&mut self, _now: u64) -> Reading {
        Reading::None
    }

    /// The scripted finger's point with its noise (a small random number
    /// generator: xorshift).
    fn noisy(&mut self, p: TouchPoint) -> TouchPoint {
        if self.noise == 0 {
            return p;
        }
        let mut wobble = || -> i32 {
            self.rng ^= self.rng << 13;
            self.rng ^= self.rng >> 17;
            self.rng ^= self.rng << 5;
            let span = 2 * u32::from(self.noise) + 1;
            (self.rng % span) as i32 - i32::from(self.noise)
        };
        let dx = wobble();
        let dy = wobble();
        TouchPoint {
            x: (i32::from(p.x) + dx).clamp(0, crate::lcd::W as i32 - 1) as u16,
            y: (i32::from(p.y) + dy).clamp(0, crate::lcd::H as i32 - 1) as u16,
        }
    }
}

impl Touch for CydTouch {
    fn read(&mut self) -> Option<TouchPoint> {
        self.given = self.now;
        self.now
    }

    fn take_tap(&mut self) -> Option<TouchPoint> {
        self.tap_note.take()
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
