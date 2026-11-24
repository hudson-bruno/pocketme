//! The pairing window (crate feature `pair`): the only time the gate lets
//! the console search for a pad, call it and say yes to pairing.
//!
//! The window is opened from outside ([`Gate::arm`], by the firmware once
//! it saw the BOOT button held) and closes by itself: every judgement
//! takes the time, and past the window's deadline the gate is closed
//! again before anything is judged, whether or not anybody calls
//! [`Gate::disarm`]. No window, however it is armed or bound, lasts past
//! [`PAIR_WINDOW_MAX_MS`] from its arming.
//!
//! The phases:
//!
//! - **Off**: the reconnect-only rules of [`Gate::check`], plus Inquiry
//!   Cancel and Create Connection Cancel (they only stop something, and
//!   the host's clean-up may come after the window's deadline).
//! - **Search** ([`Gate::arm`]): plus Inquiry with the general access
//!   code, `33 8B 9E`, 1 to [`INQUIRY_UNITS_MAX`] units of 1.28 s and no
//!   limit on answers (`00`), exactly what pad-proto sends. The gate notes
//!   the address of every inquiry answer whose class of device says
//!   "peripheral" (major class 0x05), each once, the newest
//!   [`FOUND_MAX`]. Accept Connection Request is refused to everybody: a
//!   saved pad calling in would take the host's one link.
//! - **Bound** ([`Gate::bind`], to one address noted in this window's
//!   search): plus, for that address only, Create Connection (packet
//!   types `18 CC`, byte 9 zero, role switch 0 or 1), Remote Name
//!   Request, IO Capability Request Reply with `03 00 04` (no input, no
//!   output; no OOB data; general bonding), User Confirmation Request
//!   Reply, and the supervision timeout on its link once it is up.
//!   Accept is still refused to everybody (the bound pad is called, never
//!   accepted), and so is Link Key Request Reply to the bound address, so
//!   a stale saved key is never offered to a pad being paired again.
//! - **Paired** (from the Link Key Notification for the bound address):
//!   plus Link Key Request Reply, Remote Name Request and the supervision
//!   timeout for that pad, until the firmware saved its key and called
//!   [`Gate::allow`] (the normal rules cover it from then on), its link
//!   ends, or the deadline.
//!
//! Never, in any phase: PIN Code Request Reply, Write Scan Enable other
//! than off or page scan only, the stored-key commands, anything Low
//! Energy, any command not named here or in [`Gate::check`].
//!
//! The window falls back to Off by itself ([`Gate::observe_at`]) on a
//! Hardware Error, and, for the bound address, on a PIN Code Request, a
//! failed Simple Pairing Complete, a failed Connection Complete, or the
//! Disconnection Complete of its link.
//!
//! The gate never reads or keeps a link key: of a Link Key Notification
//! it reads only the address.

use core::fmt;

use super::{Gate, Refusal, SAVED};
use crate::{Event, kind, op, packet_len, parse_event};

/// The longest a window can be open, from [`Gate::arm`] to its end,
/// whatever it is told: 120 s.
pub const PAIR_WINDOW_MAX_MS: u64 = 120_000;

/// How many found addresses a search keeps (the newest).
pub const FOUND_MAX: usize = 8;

/// The longest inquiry the gate lets out, in units of 1.28 s (pad-proto's
/// `inquiry_units`).
pub const INQUIRY_UNITS_MAX: u8 = 3;

/// The general inquiry access code (GIAC, 0x9E8B33), low byte first.
const GIAC: [u8; 3] = [0x33, 0x8B, 0x9E];
/// IO Capability Request Reply after the address: NoInputNoOutput, no OOB
/// data, general bonding (pad-proto's constants).
const IO_REPLY: [u8; 3] = [0x03, 0x00, 0x04];
/// Create Connection's packet types: DM1, DH1, DM3, DH3, DM5, DH5.
const PACKET_TYPES: [u8; 2] = [0x18, 0xCC];
/// Class of device's major class "peripheral" (keyboards, mice, game
/// controllers).
const MAJOR_PERIPHERAL: u32 = 0x05;

// The events the window reads beside those of `parse_event`.
const INQUIRY_RESULT: u8 = 0x02;
const PIN_CODE_REQUEST: u8 = 0x16;
const LINK_KEY_NOTIFICATION: u8 = 0x18;
const INQUIRY_RESULT_WITH_RSSI: u8 = 0x22;
const EXTENDED_INQUIRY_RESULT: u8 = 0x2F;
const SIMPLE_PAIRING_COMPLETE: u8 = 0x36;

/// The window's state. Deadlines are in the caller's milliseconds; open
/// while `now < until`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Window {
    Off,
    Search {
        armed_at: u64,
        until: u64,
        found: Found,
    },
    Bound {
        armed_at: u64,
        until: u64,
        addr: [u8; 6],
        /// Its link's handle, once its Connection Complete came.
        handle: Option<u16>,
    },
    Paired {
        until: u64,
        addr: [u8; 6],
        handle: Option<u16>,
    },
}

impl Window {
    /// The window as it is at `now`: Off once its deadline passed.
    fn at(self, now: u64) -> Window {
        match self {
            Window::Search { until, .. }
            | Window::Bound { until, .. }
            | Window::Paired { until, .. }
                if now >= until =>
            {
                Window::Off
            }
            w => w,
        }
    }

    /// The address it is bound to (Bound or Paired).
    fn addr(&self) -> Option<[u8; 6]> {
        match self {
            Window::Bound { addr, .. } | Window::Paired { addr, .. } => Some(*addr),
            _ => None,
        }
    }

    /// Its link's handle (Bound or Paired, once the link is up).
    fn handle(&self) -> Option<u16> {
        match self {
            Window::Bound { handle, .. } | Window::Paired { handle, .. } => *handle,
            _ => None,
        }
    }
}

/// The addresses a search found, oldest first, each once.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Found {
    addrs: [[u8; 6]; FOUND_MAX],
    len: u8,
}

impl Found {
    const fn new() -> Self {
        Found {
            addrs: [[0; 6]; FOUND_MAX],
            len: 0,
        }
    }

    fn all(&self) -> &[[u8; 6]] {
        &self.addrs[..usize::from(self.len)]
    }

    fn contains(&self, addr: &[u8; 6]) -> bool {
        self.all().contains(addr)
    }

    /// Notes an address. One seen again moves to the newest place (so a
    /// pad that keeps answering is never pushed out by older ones); when
    /// all places are taken, the oldest goes.
    fn note(&mut self, addr: [u8; 6]) {
        let n = usize::from(self.len);
        if let Some(i) = self.all().iter().position(|a| *a == addr) {
            self.addrs[i..n].rotate_left(1);
            self.addrs[n - 1] = addr;
        } else if n < FOUND_MAX {
            self.addrs[n] = addr;
            self.len += 1;
        } else {
            self.addrs.rotate_left(1);
            self.addrs[FOUND_MAX - 1] = addr;
        }
    }
}

/// The window's phase, for logs: only a word, never an address.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PairPhase {
    /// No window: the reconnect-only rules.
    Off,
    /// Armed: searching.
    Searching,
    /// Bound to one found pad: calling and pairing it.
    Bound,
    /// The bound pad's key came; waiting for it to be saved.
    Paired,
}

impl PairPhase {
    /// "off", "searching", "bound" or "paired".
    pub fn word(self) -> &'static str {
        match self {
            PairPhase::Off => "off",
            PairPhase::Searching => "searching",
            PairPhase::Bound => "bound",
            PairPhase::Paired => "paired",
        }
    }
}

impl fmt::Display for PairPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

impl fmt::Debug for PairPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// Why [`Gate::arm`] or [`Gate::bind`] refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairRefusal {
    /// `arm`: a window is open already (it cannot be stretched by arming
    /// again).
    Open,
    /// `bind`: no search is open.
    NotSearching,
    /// `bind`: this address answered no inquiry of this window as a
    /// peripheral (or was pushed out by newer ones).
    NotFound,
    /// `bind`: four pads are saved and this address is not one of them.
    Full,
    /// `arm` or `bind` asked for no time at all.
    NoTime,
}

/// A command packet of the right length: its code and its parameters.
fn command(packet: &[u8]) -> Option<(u16, &[u8])> {
    if packet.first() != Some(&kind::COMMAND) || packet_len(packet) != Some(packet.len()) {
        return None;
    }
    Some((u16::from_le_bytes([packet[1], packet[2]]), &packet[4..]))
}

/// An event packet of the right length: its code and its parameters.
fn event(packet: &[u8]) -> Option<(u8, &[u8])> {
    let (&k, rest) = packet.split_first()?;
    let (&c, rest) = rest.split_first()?;
    let (&n, params) = rest.split_first()?;
    (k == kind::EVENT && params.len() == usize::from(n)).then_some((c, params))
}

/// The address in `p` from byte `at`, if it is all there.
fn addr_at(p: &[u8], at: usize) -> Option<[u8; 6]> {
    p.get(at..at + 6)?.try_into().ok()
}

/// Notes the peripherals among an inquiry result's answers. 0x02 and 0x22
/// carry several answers, each whole (14 bytes after the count byte);
/// 0x2F carries one. Read the way pad-proto reads them (its `hci.rs`).
fn note_answers(found: &mut Found, code: u8, e: &[u8]) {
    let mut one = |addr: usize, cod: usize| {
        if let (Some(a), Some(c)) = (addr_at(e, addr), e.get(cod..cod + 3)) {
            let cod = u32::from_le_bytes([c[0], c[1], c[2], 0]);
            if (cod >> 8) & 0x1F == MAJOR_PERIPHERAL {
                found.note(a);
            }
        }
    };
    match code {
        EXTENDED_INQUIRY_RESULT if e.len() >= 15 => one(1, 9),
        INQUIRY_RESULT | INQUIRY_RESULT_WITH_RSSI => {
            let Some(&n) = e.first() else { return };
            let n = usize::from(n);
            if e.len() < 1 + 14 * n {
                return;
            }
            // Address, page scan mode, then 2 reserved bytes (0x02) or 1
            // (0x22) before the class of device.
            let cod = if code == INQUIRY_RESULT { 9 } else { 8 };
            for i in 0..n {
                let base = 1 + 14 * i;
                one(base, base + cod);
            }
        }
        _ => {}
    }
}

impl Gate {
    /// Opens a window at `now_ms` for `for_ms` (at most
    /// [`PAIR_WINDOW_MAX_MS`]): the gate searches. Refused while a window
    /// is open. Not refused when four pads are saved: one of them may be
    /// paired again ([`Gate::bind`] decides).
    pub fn arm(&mut self, now_ms: u64, for_ms: u64) -> Result<(), PairRefusal> {
        self.expire(now_ms);
        if self.window != Window::Off {
            return Err(PairRefusal::Open);
        }
        if for_ms == 0 {
            return Err(PairRefusal::NoTime);
        }
        self.window = Window::Search {
            armed_at: now_ms,
            until: now_ms.saturating_add(for_ms.min(PAIR_WINDOW_MAX_MS)),
            found: Found::new(),
        };
        Ok(())
    }

    /// Binds the search to one pad, the user's choice: from `now_ms` for
    /// `for_ms`, but never past the window's cap. Only while searching,
    /// only for an address noted from an inquiry answer of this window,
    /// and, when four pads are saved, only for one of them.
    pub fn bind(&mut self, addr: [u8; 6], now_ms: u64, for_ms: u64) -> Result<(), PairRefusal> {
        self.expire(now_ms);
        let Window::Search {
            armed_at, found, ..
        } = self.window
        else {
            return Err(PairRefusal::NotSearching);
        };
        if for_ms == 0 {
            return Err(PairRefusal::NoTime);
        }
        if !found.contains(&addr) {
            return Err(PairRefusal::NotFound);
        }
        if self.saved() == SAVED && !self.is_saved(&addr) {
            return Err(PairRefusal::Full);
        }
        let cap = armed_at.saturating_add(PAIR_WINDOW_MAX_MS);
        self.window = Window::Bound {
            armed_at,
            until: now_ms.saturating_add(for_ms).min(cap),
            addr,
            handle: None,
        };
        Ok(())
    }

    /// Closes the window, whatever its phase.
    pub fn disarm(&mut self) {
        self.window = Window::Off;
    }

    /// Closes the window if its deadline passed at `now_ms`.
    pub fn expire(&mut self, now_ms: u64) {
        self.window = self.window.at(now_ms);
    }

    /// Forgets a saved pad: its address and its link's handle go, and a
    /// window bound to it closes. True when it was saved.
    pub fn forget(&mut self, addr: [u8; 6]) -> bool {
        let mut was = false;
        for s in self.saved.iter_mut() {
            if *s == Some(addr) {
                *s = None;
                was = true;
            }
        }
        for l in self.links.iter_mut() {
            if l.is_some_and(|l| l.addr == addr) {
                *l = None;
            }
        }
        if self.window.addr() == Some(addr) {
            self.window = Window::Off;
        }
        was
    }

    /// The window's phase as of the last call that took the time (for
    /// logs).
    pub fn phase(&self) -> PairPhase {
        match self.window {
            Window::Off => PairPhase::Off,
            Window::Search { .. } => PairPhase::Searching,
            Window::Bound { .. } => PairPhase::Bound,
            Window::Paired { .. } => PairPhase::Paired,
        }
    }

    /// [`Gate::allow`] for the address a window paired: the window ends.
    pub(super) fn paired_saved(&mut self, addr: [u8; 6]) {
        if matches!(self.window, Window::Paired { addr: a, .. } if a == addr) {
            self.window = Window::Off;
        }
    }

    /// Whether `packet` may go to the controller at `now_ms`: the rules
    /// of [`Gate::check`] with the window's (see the module's text). A
    /// gate never armed differs from [`Gate::check`] only in letting
    /// Inquiry Cancel and Create Connection Cancel out.
    pub fn check_at(&self, packet: &[u8], now_ms: u64) -> Result<(), Refusal> {
        let today = self.check(packet);
        let Some((opcode, p)) = command(packet) else {
            return today;
        };
        let bad = Err(Refusal::BadParams(opcode));
        match opcode {
            op::INQUIRY_CANCEL => return if p.is_empty() { Ok(()) } else { bad },
            // Any address: it only stops a call the controller makes.
            op::CREATE_CONNECTION_CANCEL => return if p.len() == 6 { Ok(()) } else { bad },
            _ => {}
        }
        // Only the bound address; a command of the wrong shape is
        // refused first.
        let to_bound = |shape_ok: bool, addr: [u8; 6]| {
            if !shape_ok {
                bad
            } else if p[..6] != addr {
                Err(Refusal::NotBound(opcode))
            } else {
                Ok(())
            }
        };
        // A rule of `check` for saved pads, widened to the bound address.
        let widened = |addr: [u8; 6]| match today {
            Err(Refusal::NotSaved(_)) if p[..6] == addr => Ok(()),
            Err(Refusal::NotSaved(_)) => Err(Refusal::NotBound(opcode)),
            other => other,
        };
        // The supervision timeout, also on the bound pad's link.
        let on_link = |handle: Option<u16>| match today {
            Err(Refusal::NotSaved(_)) if handle == Some(u16::from_le_bytes([p[0], p[1]])) => Ok(()),
            other => other,
        };
        match self.window.at(now_ms) {
            Window::Off => today,
            Window::Search { .. } => match opcode {
                op::INQUIRY => {
                    let ok = matches!(p, [a, b, c, units, 0]
                        if [*a, *b, *c] == GIAC && (1..=INQUIRY_UNITS_MAX).contains(units));
                    if ok { Ok(()) } else { bad }
                }
                op::ACCEPT_CONNECTION_REQUEST => Err(Refusal::NotAllowed(opcode)),
                _ => today,
            },
            Window::Bound { addr, handle, .. } => match opcode {
                op::ACCEPT_CONNECTION_REQUEST => Err(Refusal::NotAllowed(opcode)),
                op::LINK_KEY_REQUEST_REPLY if p.len() == 22 && p[..6] == addr => {
                    Err(Refusal::NotAllowed(opcode))
                }
                op::CREATE_CONNECTION => to_bound(
                    p.len() == 13 && p[6..8] == PACKET_TYPES && p[9] == 0 && p[12] <= 1,
                    addr,
                ),
                op::IO_CAPABILITY_REQUEST_REPLY => {
                    to_bound(p.len() == 9 && p[6..9] == IO_REPLY, addr)
                }
                op::USER_CONFIRMATION_REQUEST_REPLY => to_bound(p.len() == 6, addr),
                op::REMOTE_NAME_REQUEST => widened(addr),
                op::WRITE_LINK_SUPERVISION_TIMEOUT => on_link(handle),
                _ => today,
            },
            Window::Paired { addr, handle, .. } => match opcode {
                op::LINK_KEY_REQUEST_REPLY | op::REMOTE_NAME_REQUEST => widened(addr),
                op::WRITE_LINK_SUPERVISION_TIMEOUT => on_link(handle),
                _ => today,
            },
        }
    }

    /// Every packet from the controller, at `now_ms`: what
    /// [`Gate::observe`] does, and the window's part. A window past its
    /// deadline closes first. While searching, peripherals' inquiry
    /// answers are noted. While bound, the bound pad's good Connection
    /// Complete notes its link (so its supervision timeout passes, also
    /// after the window), and its Link Key Notification turns the window
    /// to Paired. A Hardware Error closes any window; a PIN Code Request,
    /// a failed Simple Pairing Complete or a failed Connection Complete
    /// for the bound address, or its link's Disconnection Complete, close
    /// a bound or paired one.
    pub fn observe_at(&mut self, packet: &[u8], now_ms: u64) {
        self.expire(now_ms);
        self.observe_window(packet);
        self.observe(packet);
    }

    fn observe_window(&mut self, packet: &[u8]) {
        match parse_event(packet) {
            Some(Event::HardwareError { .. }) => {
                self.window = Window::Off;
                return;
            }
            Some(Event::ConnectionComplete {
                status,
                handle,
                addr,
                link_type,
            }) if self.window.addr() == Some(*addr.bytes()) => {
                let handle = handle & 0x0FFF;
                if status != 0 {
                    self.window = Window::Off;
                } else if link_type == 1
                    && let Window::Bound { handle: h, .. } = &mut self.window
                {
                    *h = Some(handle);
                    self.note_link(*addr.bytes(), handle);
                }
                return;
            }
            Some(Event::DisconnectionComplete {
                status: 0, handle, ..
            }) => {
                if self.window.handle() == Some(handle & 0x0FFF) {
                    self.window = Window::Off;
                }
                return;
            }
            _ => {}
        }
        let Some((code, e)) = event(packet) else {
            return;
        };
        let bound = self.window.addr();
        match code {
            INQUIRY_RESULT | INQUIRY_RESULT_WITH_RSSI | EXTENDED_INQUIRY_RESULT => {
                if let Window::Search { found, .. } = &mut self.window {
                    note_answers(found, code, e);
                }
            }
            PIN_CODE_REQUEST if bound.is_some() && addr_at(e, 0) == bound => {
                self.window = Window::Off;
            }
            // Address (6), key (16), key type (1): only the address is
            // read.
            LINK_KEY_NOTIFICATION if e.len() >= 23 => {
                if let Window::Bound {
                    until,
                    addr,
                    handle,
                    ..
                } = self.window
                    && addr_at(e, 0) == Some(addr)
                {
                    self.window = Window::Paired {
                        until,
                        addr,
                        handle,
                    };
                }
            }
            SIMPLE_PAIRING_COMPLETE
                if bound.is_some()
                    && e.first().is_some_and(|s| *s != 0)
                    && addr_at(e, 1) == bound =>
            {
                self.window = Window::Off;
            }
            _ => {}
        }
    }
}
