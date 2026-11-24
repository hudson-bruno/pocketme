//! The pairing window (crate feature `pair`).
//!
//! The console runs the host in reconnect-only mode for ever
//! ([`Config::reconnect_only`](super::Config::reconnect_only)). A window
//! is the one way to add a pad, and it is narrow:
//!
//! - The caller opens it ([`Host::open_pairing`]) with a deadline. The
//!   host lets a linked pad go, refuses every call (0x0D, saved pads
//!   included) and searches in rounds, none of which ends after the
//!   deadline. It calls nobody on its own: each gamepad heard is listed
//!   once ([`UiEvent::Found`]) and kept in a table of eight with what is
//!   needed to call it (page scan mode, clock offset). Its name is never
//!   logged.
//! - The user picks one listed pad ([`Host::pair_with`]). Only that
//!   address may now be called and answered "yes" to pairing (Secure
//!   Simple Pairing "just works"); everyone else stays refused. It is
//!   offered no saved key (a pad in pairing mode has dropped its own), a
//!   PIN or passkey request ends the window, and only key types 0x04 and
//!   0x07 are taken (0x03 is the public debug key).
//! - The new key goes into the table at once (replacing the same
//!   address in place, never pushing another pad out) but is reported
//!   ([`Io::key_change`]) only when encryption is on. If the window ends
//!   any other way, the table is put back as it was.
//! - Each window ends exactly once, with one [`PairEnd`]
//!   ([`Host::pairing_outcome`]); the held key is zeroed then.
//!
//! The central check is in `Host::cmd` (and again in `Host::pump`): a
//! command reconnect-only forbids leaves only when the window allows it
//! (`PairState::allows`).

use super::{
    Candidate, Host, INQUIRY_GUARD_EXTRA, Io, KEYS, KeyChange, Phase, Purpose, REASON_AUTH_FAILURE,
    REASON_LOCAL_HOST, REASON_USER_ENDED, Sec, Tag, Text, UiEvent, allowed,
    forbidden_when_reconnect_only, key_type_text,
};
use crate::hci::{self, BdAddr, Found, LinkKey, op, params};

/// The longest a window stays open, from [`Host::open_pairing`], whatever
/// the deadlines the caller passes (milliseconds). The board's own guard
/// (hci-rules) caps its window at the same 120 s and should be given a
/// deadline a few seconds later than this host's, so the host's clean-up
/// commands still pass.
pub const PAIR_WINDOW_MAX: u64 = 120_000;

/// Gamepads kept per window (the first eight heard).
const FOUND_MAX: usize = 8;

/// One inquiry unit, in milliseconds (1.28 s).
const INQUIRY_UNIT_MS: u64 = 1_280;

/// A pairing window: searching, or pairing with the one pad picked.
/// Times are the caller's milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window {
    /// Searching; gamepads heard are listed. Ends at `until`.
    Scan { opened: u64, until: u64 },
    /// Calling and pairing `addr` only. Ends at `until`, which is also
    /// at most `SECURITY_TIMEOUT` after its link came up.
    Pair {
        opened: u64,
        addr: BdAddr,
        until: u64,
    },
}

impl Window {
    /// When the window ends by itself.
    pub fn until(self) -> u64 {
        match self {
            Window::Scan { until, .. } | Window::Pair { until, .. } => until,
        }
    }

    /// The pad picked, in [`Window::Pair`].
    pub fn target(self) -> Option<BdAddr> {
        match self {
            Window::Pair { addr, .. } => Some(addr),
            Window::Scan { .. } => None,
        }
    }
}

/// How a pairing window ended. One per window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairEnd {
    /// The pad paired and encryption is on: its key was reported
    /// ([`KeyChange::Save`]) and the link goes on as a saved pad's.
    Saved,
    /// The deadline passed.
    Timeout,
    /// The pad asked for old-style PIN pairing or for a passkey: refused.
    OldPin,
    /// The pairing failed or was refused (authentication failed, a key
    /// type other than 0x04 or 0x07, no room for a new pad, encryption
    /// without a new key).
    Refused,
    /// The call to the pad failed, or its link ended before encryption.
    Lost,
    /// The caller closed the window ([`Host::close_pairing`]), or forgot
    /// the pad being paired ([`Host::forget_key_of`]).
    Cancelled,
}

impl PairEnd {
    /// A short word for the log.
    pub fn word(self) -> &'static str {
        match self {
            PairEnd::Saved => "saved",
            PairEnd::Timeout => "timeout",
            PairEnd::OldPin => "old-pin",
            PairEnd::Refused => "refused",
            PairEnd::Lost => "lost",
            PairEnd::Cancelled => "cancelled",
        }
    }
}

/// Why [`Host::open_pairing`] or [`Host::pair_with`] said no. Nothing
/// changed in the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairError {
    /// The host is not in reconnect-only mode: windows are only for it.
    NotReconnectOnly,
    /// A window is open already.
    AlreadyOpen,
    /// The host is busy: starting up, accepting a call, a command
    /// waiting for its answer, or (for `pair_with`) a link up.
    Busy,
    /// `pair_with` outside a searching window.
    NotScanning,
    /// `pair_with` for an address this window has not listed.
    NotFound,
    /// Four pads are saved and the one picked is not one of them.
    Full,
}

impl PairError {
    /// A short word for the log.
    pub fn word(self) -> &'static str {
        match self {
            PairError::NotReconnectOnly => "not-reconnect-only",
            PairError::AlreadyOpen => "already-open",
            PairError::Busy => "busy",
            PairError::NotScanning => "not-scanning",
            PairError::NotFound => "not-found",
            PairError::Full => "full",
        }
    }
}

/// A gamepad heard in this window, with what calling it needs.
#[derive(Clone, Copy)]
pub(super) struct Seen {
    addr: BdAddr,
    psrm: u8,
    clock: u16,
    cod: u32,
    found_at: u64,
}

/// The new key of the pad being paired, from its Link Key Notification
/// until encryption is on (then reported) or the window ends. No `Debug`:
/// it holds keys.
#[derive(Clone, Copy)]
pub(super) struct Held {
    pub(super) addr: BdAddr,
    key: LinkKey,
    key_type: u8,
    /// The key this address had before, to put back if the pairing fails.
    old: Option<(LinkKey, u8)>,
}

/// Everything the window keeps. All of it is reset when a window opens.
pub(super) struct PairState {
    pub(super) window: Option<Window>,
    found: [Option<Seen>; FOUND_MAX],
    pub(super) held: Option<Held>,
    outcome: Option<PairEnd>,
}

/// What `PairState::note` did with a gamepad heard.
enum Noted {
    New,
    Again,
    NoRoom,
}

impl PairState {
    pub(super) const NONE: PairState = PairState {
        window: None,
        found: [None; FOUND_MAX],
        held: None,
        outcome: None,
    };

    pub(super) fn target(&self) -> Option<BdAddr> {
        self.window.and_then(Window::target)
    }

    /// The window lets this reconnect-only-forbidden command out (`p`:
    /// its parameters). Inquiry while searching; Create Connection and
    /// the IO capability and user confirmation replies for the target
    /// only. Never a PIN reply.
    pub(super) fn allows(&self, opcode: u16, p: &[u8]) -> bool {
        match (self.window, opcode) {
            (Some(Window::Scan { .. }), op::INQUIRY) => true,
            (
                Some(Window::Pair { addr, .. }),
                op::CREATE_CONNECTION
                | op::IO_CAPABILITY_REQUEST_REPLY
                | op::USER_CONFIRMATION_REQUEST_REPLY,
            ) => p.get(..6) == Some(&addr.0[..]),
            _ => false,
        }
    }

    /// The target's link came up: the window ends `security` ms later at
    /// most.
    pub(super) fn link_up(&mut self, now: u64, addr: BdAddr, security: u64) {
        if let Some(Window::Pair { addr: a, until, .. }) = self.window.as_mut()
            && *a == addr
        {
            *until = (*until).min(now + security);
        }
    }

    fn seen(&self, addr: BdAddr) -> Option<Seen> {
        self.found
            .iter()
            .flatten()
            .find(|s| s.addr == addr)
            .copied()
    }

    /// Keeps a gamepad heard: the first eight, each once (a second answer
    /// refreshes what calling it needs).
    fn note(&mut self, s: Seen) -> Noted {
        if let Some(old) = self.found.iter_mut().flatten().find(|o| o.addr == s.addr) {
            old.psrm = s.psrm;
            old.clock = s.clock;
            return Noted::Again;
        }
        match self.found.iter_mut().find(|o| o.is_none()) {
            Some(slot) => {
                *slot = Some(s);
                Noted::New
            }
            None => Noted::NoRoom,
        }
    }

    /// Writes zeros over the held keys, then drops them.
    fn zero_held(&mut self) {
        if let Some(h) = self.held.as_mut() {
            h.key.0 = [0; 16];
            if let Some((k, _)) = h.old.as_mut() {
                k.0 = [0; 16];
            }
            // The zeros must really be written, not left out as unused.
            core::hint::black_box(&*h);
        }
        self.held = None;
    }
}

impl Host {
    // ------------------------------------------------------------ public

    /// Opens a pairing window of `for_ms` milliseconds (at most
    /// [`PAIR_WINDOW_MAX`]): search and list gamepads, call nobody. A
    /// linked pad is let go first (the host has room for one link). The
    /// first search round is sent at once when no command is on its way,
    /// else at the next [`Host::tick`]; the caller should tick soon (every
    /// 20 to 100 ms while a window is open).
    ///
    /// Refused (nothing changes) outside reconnect-only mode, while a
    /// window is open, while starting up or accepting a call, and while a
    /// command waits for its answer.
    pub fn open_pairing(
        &mut self,
        now: u64,
        for_ms: u64,
        io: &mut impl Io,
    ) -> Result<(), PairError> {
        let refused = if !self.cfg.reconnect_only {
            Some(PairError::NotReconnectOnly)
        } else if self.pair.window.is_some() {
            Some(PairError::AlreadyOpen)
        } else if self.cmd_wait.is_some()
            || matches!(
                self.phase,
                Phase::Init
                    | Phase::Accepting { .. }
                    | Phase::Cancelling { .. }
                    | Phase::Paging { .. }
            )
        {
            Some(PairError::Busy)
        } else {
            None
        };
        if let Some(e) = refused {
            log!(
                io,
                State,
                "pairing window refused: {} (phase={}, command waiting={})",
                e.word(),
                self.phase_name(),
                u8::from(self.cmd_wait.is_some())
            );
            return Err(e);
        }
        if self.link.is_some() {
            log!(io, State, "pairing window: the linked pad is let go first");
            self.drop_link(now, REASON_LOCAL_HOST, io);
        }
        self.pair.zero_held();
        self.pair.found = [None; FOUND_MAX];
        self.pair.outcome = None;
        let len = for_ms.min(PAIR_WINDOW_MAX);
        self.pair.window = Some(Window::Scan {
            opened: now,
            until: now + len,
        });
        self.cand = None;
        self.early_role = None;
        self.phase = Phase::Pause { until: now };
        log!(
            io,
            State,
            "pairing window open for {} ms: searching, nobody is called until the user picks a pad; every call is refused meanwhile (saved pads={})",
            len,
            self.key_count()
        );
        self.status(io, format_args!("Searching for a new pad"));
        self.window_tick(now, io);
        self.pump(now, io);
        Ok(())
    }

    /// The user picked `addr` from this window's list: stop searching,
    /// call it and pair with it. The window now ends `for_ms` from now
    /// (never later than [`PAIR_WINDOW_MAX`] after it opened), and at
    /// most `SECURITY_TIMEOUT` (30 s) after the link comes up.
    ///
    /// Refused (nothing changes) outside a searching window, for an
    /// address not listed in it, while a link is up, and when four pads
    /// are saved and `addr` is not one of them ([`PairError::Full`]: a
    /// saved pad may be paired again, its new key replaces the old one).
    pub fn pair_with(
        &mut self,
        addr: BdAddr,
        now: u64,
        for_ms: u64,
        io: &mut impl Io,
    ) -> Result<(), PairError> {
        let refused = match (self.pair.window, self.pair.seen(addr)) {
            (Some(Window::Scan { .. }), None) => Err(PairError::NotFound),
            (Some(Window::Scan { opened, .. }), Some(seen)) => {
                if self.key(addr).is_none() && self.key_count() >= KEYS {
                    Err(PairError::Full)
                } else if self.link.is_some() {
                    Err(PairError::Busy)
                } else {
                    Ok((opened, seen))
                }
            }
            _ => Err(PairError::NotScanning),
        };
        let (opened, seen) = match refused {
            Ok(v) => v,
            Err(e) => {
                log!(io, State, "pairing: pad not taken: {}", e.word());
                return Err(e);
            }
        };
        self.pair.window = Some(Window::Pair {
            opened,
            addr,
            until: (now + for_ms).min(opened + PAIR_WINDOW_MAX),
        });
        // A search round still queued never leaves now.
        let (inquiry_taken_back, _) = self.purge_window_cmds(io);
        // No name: the link asks for it, as for a pad that calls us, so
        // the name read in the search never reaches a log or a status.
        self.cand = Some(Candidate {
            addr,
            psrm: seen.psrm,
            clock: Some(seen.clock),
            cod: seen.cod,
            name: Text::new(),
            purpose: Purpose::Gamepad,
            found_at: Some(seen.found_at),
        });
        log!(
            io,
            State,
            "pairing: the user picked addr={} (saved before: {}); calling it",
            addr,
            u8::from(self.key(addr).is_some())
        );
        match self.phase {
            Phase::Inquiry { .. } if !inquiry_taken_back => {
                self.cmd(io, op::INQUIRY_CANCEL, &[]);
                self.phase = Phase::Cancelling { since: now };
            }
            _ => self.start_page(now, io),
        }
        self.pump(now, io);
        Ok(())
    }

    /// Closes the window now ([`PairEnd::Cancelled`]): a search or call
    /// under way is stopped, a pad being paired is let go and its new key
    /// dropped. False when no window was open.
    pub fn close_pairing(&mut self, now: u64, io: &mut impl Io) -> bool {
        if self.pair.window.is_none() {
            return false;
        }
        self.end_window(now, PairEnd::Cancelled, io);
        self.pump(now, io);
        true
    }

    /// True while a pairing window is open.
    pub fn pairing_open(&self) -> bool {
        self.pair.window.is_some()
    }

    /// The pad the open window pairs with (after [`Host::pair_with`]).
    pub fn pair_target(&self) -> Option<BdAddr> {
        self.pair.target()
    }

    /// The open window, with its deadline.
    pub fn pairing_window(&self) -> Option<Window> {
        self.pair.window
    }

    /// How the last window ended; None while one is open or before the
    /// first. It stays until the next [`Host::open_pairing`].
    pub fn pairing_outcome(&self) -> Option<PairEnd> {
        self.pair.outcome
    }

    /// The user forgets one pad: its key leaves the table (the others
    /// keep their order). The caller asked for it, so nothing is reported
    /// through [`Io::key_change`]; the caller erases its own copy. If that
    /// pad is linked it is let go; if a window is pairing with it, the
    /// window ends ([`PairEnd::Cancelled`]). True when a key was there.
    pub fn forget_key_of(&mut self, addr: BdAddr, now: u64, io: &mut impl Io) -> bool {
        // First, so the window does not put an old key back afterwards.
        if self.target() == Some(addr) {
            self.end_window(now, PairEnd::Cancelled, io);
        }
        if let Some(slot) = self
            .keys
            .iter_mut()
            .find(|k| matches!(k, Some((a, _, _)) if *a == addr))
        {
            // Zeros over the key before its place is given up.
            *slot = Some((addr, LinkKey([0; 16]), 0));
        }
        let had = self.remove_key(addr);
        for k in self.known.iter_mut() {
            if *k == Some(addr) {
                *k = None;
            }
        }
        if self.called_back == Some(addr) {
            self.called_back = None;
        }
        if self.forced.is_some_and(|f| f.0 == addr) {
            self.forced = None;
        }
        log!(
            io,
            State,
            "forgot one pairing addr={} (asked by the user): {}; saved pads={}",
            addr,
            if had { "removed" } else { "there was none" },
            self.key_count()
        );
        if self
            .link
            .is_some_and(|l| l.addr == addr && l.purpose == Purpose::Gamepad)
        {
            self.status(io, format_args!("Pairing forgotten"));
            self.disconnect(io, REASON_USER_ENDED);
        }
        self.pump(now, io);
        had
    }

    // ------------------------------------------------------------ hooks

    /// From [`Host::tick`]: the deadline, then the next search round.
    pub(super) fn window_tick(&mut self, now: u64, io: &mut impl Io) {
        let Some(w) = self.pair.window else { return };
        if now >= w.until() {
            self.end_window(now, PairEnd::Timeout, io);
            return;
        }
        let Window::Scan { until, .. } = w else {
            return;
        };
        if self.link.is_some() || self.cmd_wait.is_some() {
            return;
        }
        let due = match self.phase {
            Phase::Pause { until: p } => now >= p,
            Phase::Waiting => true,
            _ => false,
        };
        if !due {
            return;
        }
        // No round that would end after the deadline.
        let units = u64::from(self.cfg.inquiry_units).min((until - now) / INQUIRY_UNIT_MS);
        if units == 0 {
            if self.phase != Phase::Waiting {
                self.phase = Phase::Waiting;
                log!(
                    io,
                    State,
                    "pairing window: too little time left for another search round"
                );
            }
            return;
        }
        self.stats.rounds += 1;
        let round = self.stats.rounds;
        self.status(
            io,
            format_args!("Searching for a new pad (round {})", round),
        );
        // `units` is at most `inquiry_units`, a u8.
        let units = units as u8;
        self.cmd(io, op::INQUIRY, &params::inquiry(units));
        self.phase = Phase::Inquiry {
            until: now + u64::from(units) * INQUIRY_UNIT_MS + INQUIRY_GUARD_EXTRA,
        };
    }

    /// From `on_found` while a window is open: list gamepads (searching
    /// only), call nobody, never log a name.
    pub(super) fn window_found(&mut self, now: u64, f: Found<'_>, io: &mut impl Io) {
        let eir = hci::parse_eir(f.eir);
        let name = match eir.name.iter().position(|&b| b == 0) {
            Some(i) => &eir.name[..i],
            None => eir.name,
        };
        let rssi = f.rssi.map_or(0, i32::from);
        if !hci::is_gamepad(f.cod, name) {
            self.stats.found_other += 1;
            let major = hci::cod_major(f.cod);
            log!(
                io,
                Hci,
                "found other cod={:#08x} major={:#04x}({}) rssi={} (pairing window; not a gamepad, address not logged)",
                f.cod,
                major,
                hci::major_text(major),
                rssi
            );
            return;
        }
        if !allowed(self.cfg.only, f.addr, name) {
            self.stats.found_skipped += 1;
            log!(
                io,
                Hci,
                "found gamepad not on the allow-list cod={:#08x} rssi={} (pairing window; not listed; address and name not logged)",
                f.cod,
                rssi
            );
            return;
        }
        self.stats.found_gamepads += 1;
        let noted = match self.pair.window {
            Some(Window::Scan { .. }) => Some(self.pair.note(Seen {
                addr: f.addr,
                psrm: f.psrm,
                clock: f.clock_offset,
                cod: f.cod,
                found_at: now,
            })),
            _ => None,
        };
        log!(
            io,
            Hci,
            "found gamepad addr={} cod={:#08x} minor={:#04x} rssi={} psrm={} name_len={} eir_hid_uuid={} listed={} (pairing window; name not logged)",
            f.addr,
            f.cod,
            hci::cod_minor(f.cod),
            rssi,
            f.psrm,
            name.len(),
            u8::from(eir.hid),
            match noted {
                Some(Noted::New) => "new",
                Some(Noted::Again) => "again",
                Some(Noted::NoRoom) => "no-room",
                None => "no",
            }
        );
        if let Some(Noted::New) = noted {
            io.ui(UiEvent::Found {
                addr: f.addr,
                class_of_device: f.cod,
                name,
            });
        }
    }

    /// From the PIN and passkey requests: the target asks for a kind of
    /// pairing we never do. Let it go, end with [`PairEnd::OldPin`]. (The
    /// negative answer is already queued.)
    pub(super) fn window_old_pin(&mut self, now: u64, addr: BdAddr, what: &str, io: &mut impl Io) {
        if self.target() != Some(addr) {
            return;
        }
        log!(
            io,
            State,
            "pairing: the picked pad asks for {} pairing; refused, the window ends",
            what
        );
        self.status(io, format_args!("Pairing refused: old PIN pairing"));
        if self.link.is_some_and(|l| l.addr == addr) {
            self.disconnect(io, REASON_AUTH_FAILURE);
        }
        self.end_window(now, PairEnd::OldPin, io);
    }

    /// From the Link Key Notification of the target on its link: check
    /// the key type, then put the key into the table (in place of the
    /// address's old one, never pushing another pad out) and hold it until
    /// encryption is on.
    pub(super) fn window_new_key(
        &mut self,
        now: u64,
        addr: BdAddr,
        key: LinkKey,
        key_type: u8,
        io: &mut impl Io,
    ) {
        // Just works gives 0x04 (P-192) or 0x07 (P-256); 0x03 is the
        // public debug key, the others are not Secure Simple Pairing.
        let good = matches!(key_type, 0x04 | 0x07);
        log!(
            io,
            Hci,
            "link-key-notification addr={} key_type={:#04x}({}) stored={} (pairing window; key never printed; reported for saving once encryption is on)",
            addr,
            key_type,
            key_type_text(key_type),
            u8::from(good)
        );
        self.sec_event(now, Sec::Pairing);
        if !good {
            self.window_refuse(
                now,
                io,
                format_args!(
                    "key type {:#04x} refused (only 0x04 and 0x07 are taken)",
                    key_type
                ),
            );
            return;
        }
        let place = self
            .keys
            .iter()
            .position(|k| matches!(k, Some((a, _, _)) if *a == addr));
        if let Some(h) = self.pair.held.as_mut() {
            // A second key in the same pairing: it replaces the first;
            // the key from before the window stays aside.
            h.key = key;
            h.key_type = key_type;
            if let Some(i) = place {
                self.keys[i] = Some((addr, key, key_type));
            }
        } else if let Some(i) = place {
            let old = self.keys[i].map(|(_, k, t)| (k, t));
            self.keys[i] = Some((addr, key, key_type));
            self.pair.held = Some(Held {
                addr,
                key,
                key_type,
                old,
            });
        } else if let Some(slot) = self.keys.iter_mut().find(|k| k.is_none()) {
            *slot = Some((addr, key, key_type));
            self.pair.held = Some(Held {
                addr,
                key,
                key_type,
                old: None,
            });
        } else {
            // pair_with refuses this; never push a saved pad out.
            self.window_refuse(
                now,
                io,
                format_args!("four pads saved and this one is new; no pad is pushed out"),
            );
            return;
        }
        if let Some(l) = self.link.as_mut() {
            l.new_key = true;
        }
    }

    /// From `on_encryption` (encryption on, link to `addr`): for the
    /// target, report the held key and end with [`PairEnd::Saved`]; with no
    /// new key, refuse. False when the link is ending.
    pub(super) fn window_encrypted(&mut self, now: u64, addr: BdAddr, io: &mut impl Io) -> bool {
        if self.target() != Some(addr) {
            return true;
        }
        let Some(h) = self.pair.held.filter(|h| h.addr == addr) else {
            self.window_refuse(
                now,
                io,
                format_args!("encryption came on without a new key"),
            );
            return false;
        };
        io.key_change(KeyChange::Save {
            addr,
            key: h.key,
            key_type: h.key_type,
        });
        log!(
            io,
            State,
            "pairing: the new key goes to the store addr={} (it replaced an older one: {}; saved pads={})",
            addr,
            u8::from(h.old.is_some()),
            self.key_count()
        );
        self.status(io, format_args!("Pad paired"));
        self.end_window(now, PairEnd::Saved, io);
        true
    }

    /// The pairing with the target failed here: log why, let the pad go,
    /// end with [`PairEnd::Refused`].
    fn window_refuse(&mut self, now: u64, io: &mut impl Io, why: core::fmt::Arguments<'_>) {
        log!(io, State, "pairing refused: {}; the window ends", why);
        self.status(io, format_args!("Pairing refused"));
        self.disconnect(io, REASON_AUTH_FAILURE);
        self.end_window(now, PairEnd::Refused, io);
    }

    /// Ends the window once, with `why`: stops what it started, takes
    /// back what it queued, puts the key table back unless the pad was
    /// saved, zeroes the held key, forgets the list.
    pub(super) fn end_window(&mut self, now: u64, why: PairEnd, io: &mut impl Io) {
        let Some(w) = self.pair.window.take() else {
            return;
        };
        // Nothing the window let in may still leave.
        let (inquiry_taken_back, call_taken_back) = self.purge_window_cmds(io);
        match self.phase {
            Phase::Inquiry { .. } => {
                if !inquiry_taken_back {
                    self.cmd(io, op::INQUIRY_CANCEL, &[]);
                }
                self.phase = Phase::Pause { until: now };
            }
            Phase::Cancelling { .. } => {
                // Its Inquiry Cancel is on its way; nobody is called after.
                self.cand = None;
                self.phase = Phase::Pause { until: now };
            }
            Phase::Paging { .. } => {
                if let Some(c) = self.cand
                    && !call_taken_back
                {
                    self.cmd(io, op::CREATE_CONNECTION_CANCEL, &params::addr(c.addr));
                }
                self.cand = None;
                self.phase = Phase::Pause { until: now };
            }
            Phase::Pause { .. } | Phase::Waiting if self.link.is_none() => {
                self.phase = Phase::Pause { until: now };
            }
            _ => {}
        }
        if why != PairEnd::Saved
            && let Some(t) = w.target()
            && self.link.is_some_and(|l| l.addr == t)
        {
            self.disconnect(io, REASON_USER_ENDED);
        }
        if why != PairEnd::Saved
            && let Some(h) = self.pair.held
        {
            // The table as it was before the window.
            let place = self
                .keys
                .iter()
                .position(|k| matches!(k, Some((a, _, _)) if *a == h.addr));
            if let Some(i) = place {
                match h.old {
                    Some((k, t)) => self.keys[i] = Some((h.addr, k, t)),
                    None => {
                        self.keys[i] = Some((h.addr, LinkKey([0; 16]), 0));
                        self.remove_key(h.addr);
                    }
                }
            }
            log!(
                io,
                State,
                "pairing: the new key is dropped; the saved pads are as before"
            );
        }
        self.pair.zero_held();
        self.pair.found = [None; FOUND_MAX];
        self.pair.outcome = Some(why);
        log!(
            io,
            State,
            "pairing window closed: {} (saved pads={})",
            why.word(),
            self.key_count()
        );
        self.status(io, format_args!("Pairing closed: {}", why.word()));
    }

    /// Takes queued commands the window no longer allows out of the
    /// command queue, keeping the order of the rest. Says whether an
    /// Inquiry and whether a Create Connection were among them (then
    /// they never left, and need no cancel).
    fn purge_window_cmds(&mut self, io: &mut impl Io) -> (bool, bool) {
        let mut inquiry = false;
        let mut call = false;
        for _ in 0..self.cmdq.len() {
            let Some(p) = self.cmdq.pop() else { break };
            let b = p.as_bytes();
            let opcode = u16::from_le_bytes([b[1], b[2]]);
            if forbidden_when_reconnect_only(opcode) && !self.pair.allows(opcode, &b[4..]) {
                inquiry |= opcode == op::INQUIRY;
                call |= opcode == op::CREATE_CONNECTION;
                log!(
                    io,
                    State,
                    "queued command {} taken back (pairing window)",
                    op::name(opcode)
                );
                continue;
            }
            // There is room: one was just taken out.
            let _ = self.cmdq.push(p);
        }
        (inquiry, call)
    }
}
