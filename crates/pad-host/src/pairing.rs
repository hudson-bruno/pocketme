//! The pairing window, the console's side of it (crate feature `pair`).
//!
//! pad-proto's host does the radio work: it searches, calls only the pad
//! the user picked, pairs it with "just works" and reports its new key
//! once encryption is on (see pad-proto's `Host::open_pairing`). This part
//! keeps what the board and the console need from that, in the shape they
//! need it:
//!
//! * the gamepads heard, as `board_api::FoundPad` (tail and name), in the
//!   order heard, each once, at most [`FOUND_MAX`];
//! * the pad picked, as a [`PadAddr`], whose `Debug` and `Display` show
//!   only the last two bytes; the six bytes leave only through
//!   [`PadAddr::whole_address`], for the board's gate and store;
//! * the new key, as a [`PendingSave`] the board takes once
//!   ([`PadHost::take_pending_save`]); its bytes are zeroed here when it
//!   is taken or dropped;
//! * the saved pads by place (tail, which one is linked), and forgetting
//!   one.
//!
//! A key change from pad-proto is taken only while the window pairs and
//! only for the pad picked (see `Glue::key_change`); any other is ignored
//! and logged as without this feature. A name heard in the search never
//! reaches a log line.

use core::fmt;

use board_api::{FoundPad, PAD_NAME_MAX, SavedPad};
use pad_proto::hci::{BdAddr, LinkKey};
use pad_proto::{PairEnd, PairError, Window};

use crate::{Glue, PadHost, Radio};

/// Gamepads listed per search (pad-proto lists the same number).
pub const FOUND_MAX: usize = 8;

/// The last two bytes of an address, in the order it is written: an
/// address written `..:AB:CD` gives `[0xAB, 0xCD]` (board-api's "tail").
fn tail_of(a: BdAddr) -> [u8; 2] {
    [a.0[1], a.0[0]]
}

/// A pad's Bluetooth address. `Debug` and `Display` show only its last
/// two bytes (`xx:xx:xx:xx:AB:CD`), so it can go into a log line.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PadAddr(pub(crate) BdAddr);

impl PadAddr {
    /// The last two bytes, as the console shows them (board-api's tail).
    pub fn tail(self) -> [u8; 2] {
        tail_of(self.0)
    }

    /// All six bytes, lowest first (as on the air and in pad-store). Only
    /// for the board's packet gate (`Gate::bind`, `Gate::forget`) and its
    /// store (`save_new`, `forget`). Personal: never log or show it.
    pub fn whole_address(self) -> [u8; 6] {
        self.0.0
    }
}

impl fmt::Display for PadAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [hi, lo] = self.tail();
        write!(f, "xx:xx:xx:xx:{hi:02X}:{lo:02X}")
    }
}

impl fmt::Debug for PadAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PadAddr({self})")
    }
}

/// A new pairing for the board to write to flash: the pad picked in a
/// window, its new link key and the key's type. Made once per window
/// that ends [`PairEnd::Saved`].
///
/// Not `Clone`: there is one copy to write. Its `Debug` shows the tail
/// and the key type, never the key. Dropping it writes zeros over the key
/// and the address.
pub struct PendingSave {
    addr: BdAddr,
    key: LinkKey,
    key_type: u8,
}

impl PendingSave {
    /// The pad (for the gate and the store; see [`PadAddr`]).
    pub fn addr(&self) -> PadAddr {
        PadAddr(self.addr)
    }

    /// The 16 key bytes, for the store's `save_new` only. Secret: never
    /// log, show or copy them anywhere else.
    pub fn key(&self) -> &[u8; 16] {
        &self.key.0
    }

    /// The key's type (0x04 or 0x07: "just works" pairing).
    pub fn key_type(&self) -> u8 {
        self.key_type
    }

    /// Zeros over every byte, written for real.
    fn zero(&mut self) {
        self.key.0 = [0; 16];
        self.addr = BdAddr([0; 6]);
        self.key_type = 0;
        core::hint::black_box(&*self);
    }
}

impl Drop for PendingSave {
    fn drop(&mut self) {
        self.zero();
    }
}

impl fmt::Debug for PendingSave {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingSave")
            .field("addr", &PadAddr(self.addr))
            .field("key_type", &self.key_type)
            .finish_non_exhaustive()
    }
}

/// Where the pairing window is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairPhase {
    /// No window open.
    Off,
    /// Searching: the found list grows ([`PadHost::pair_found`]).
    Searching,
    /// Calling and pairing the pad picked ([`PadHost::pair_choose`]).
    Pairing,
}

/// Why [`PadHost::pair_begin`] or [`PadHost::pair_choose`] said no.
/// Nothing was sent and nothing changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairRefused {
    /// pad-proto's host refused (busy, a window open already, not
    /// searching, not listed, four pads saved and this one new).
    Host(PairError),
    /// No found pad has this number.
    NoSuchPad,
    /// A new pairing waits to be taken ([`PadHost::take_pending_save`]):
    /// no new window until the board has it.
    SaveWaiting,
}

impl PairRefused {
    /// A short word for the log.
    pub fn word(self) -> &'static str {
        match self {
            PairRefused::Host(e) => e.word(),
            PairRefused::NoSuchPad => "no-such-pad",
            PairRefused::SaveWaiting => "save-waiting",
        }
    }
}

/// What `State` keeps for the window (part of the pad host's value, so
/// it is small: about 290 bytes).
pub(crate) struct PairSide {
    /// pad-proto's target, copied after every call into its host, so that
    /// `Glue::key_change` can check a key change against it.
    pub(crate) target: Option<BdAddr>,
    /// The gamepads heard in this search, in order, each once.
    found_addr: [[u8; 6]; FOUND_MAX],
    found_name: [[u8; PAD_NAME_MAX]; FOUND_MAX],
    found_name_len: [u8; FOUND_MAX],
    found_len: u8,
    /// The new pairing, until the board takes it.
    pending: Option<PendingSave>,
    /// The window's link is still up although the window ended without a
    /// save (its hang-up is on its way): not a call to count.
    window_link: bool,
    /// The address of the link that is up (from its Connection Complete).
    pub(crate) link_addr: Option<BdAddr>,
}

impl PairSide {
    pub(crate) const NONE: PairSide = PairSide {
        target: None,
        found_addr: [[0; 6]; FOUND_MAX],
        found_name: [[0; PAD_NAME_MAX]; FOUND_MAX],
        found_name_len: [0; FOUND_MAX],
        found_len: 0,
        pending: None,
        window_link: false,
        link_addr: None,
    };

    /// A gamepad heard: listed once, while there is room. True when the
    /// list grew. The name is kept, cut to [`PAD_NAME_MAX`] bytes, and
    /// never logged.
    pub(crate) fn note_found(&mut self, addr: BdAddr, name: &[u8]) -> bool {
        let n = usize::from(self.found_len);
        if n >= FOUND_MAX || self.found_addr[..n].contains(&addr.0) {
            return false;
        }
        let k = name.len().min(PAD_NAME_MAX);
        self.found_addr[n] = addr.0;
        self.found_name[n] = [0; PAD_NAME_MAX];
        self.found_name[n][..k].copy_from_slice(&name[..k]);
        self.found_name_len[n] = k as u8;
        self.found_len += 1;
        true
    }

    fn clear_found(&mut self) {
        self.found_addr = [[0; 6]; FOUND_MAX];
        self.found_name = [[0; PAD_NAME_MAX]; FOUND_MAX];
        self.found_name_len = [0; FOUND_MAX];
        self.found_len = 0;
    }

    fn found(&self, i: usize) -> Option<BdAddr> {
        (i < usize::from(self.found_len)).then(|| BdAddr(self.found_addr[i]))
    }

    /// Keeps the new pairing for the board (an older one not taken is
    /// zeroed first).
    pub(crate) fn hold(&mut self, addr: BdAddr, key: LinkKey, key_type: u8) {
        self.drop_pending();
        self.pending = Some(PendingSave {
            addr,
            key,
            key_type,
        });
    }

    /// The pending pairing, moved out; zeros are left in its place here.
    fn take_pending(&mut self) -> Option<PendingSave> {
        let p = self.pending.as_mut()?;
        let out = PendingSave {
            addr: p.addr,
            key: LinkKey(p.key.0),
            key_type: p.key_type,
        };
        self.drop_pending();
        Some(out)
    }

    /// Zeros over the pending pairing, then none.
    fn drop_pending(&mut self) {
        if let Some(p) = self.pending.as_mut() {
            p.zero();
        }
        self.pending = None;
    }

    fn pending_addr(&self) -> Option<BdAddr> {
        self.pending.as_ref().map(|p| p.addr)
    }
}

impl PadHost {
    /// After every call into pad-proto's host: copies its target, and says
    /// whether the window (or its link, while that is being hung up) is
    /// what runs now, which is not a call to count.
    pub(crate) fn sync_pair(&mut self) -> bool {
        let open = self.host.pairing_open();
        let linked = self.host.linked();
        let saved = self.host.pairing_outcome() == Some(PairEnd::Saved);
        let p = &mut self.st.pair;
        p.target = self.host.pair_target();
        if !linked {
            p.window_link = false;
            p.link_addr = None;
        } else if open {
            p.window_link = true;
        } else if saved {
            // The pad paired: its link goes on as a saved pad's call.
            p.window_link = false;
        }
        open || p.window_link
    }

    /// True while a window is open, or its link is being hung up.
    pub(crate) fn window_busy(&self) -> bool {
        self.host.pairing_open() || self.st.pair.window_link
    }

    /// Opens a pairing window of `for_ms` ms (pad-proto caps it at
    /// `PAIR_WINDOW_MAX`, 120 s): search and list gamepads, call nobody,
    /// refuse every call. A linked pad is let go first; an injected pad
    /// ends. The found list starts empty. Tick every 10 to 20 ms while it
    /// is open.
    ///
    /// Refused (nothing sent) while a new pairing waits to be taken, and
    /// when pad-proto's host refuses (a window open already, starting up,
    /// accepting a call, a command waiting for its answer).
    pub fn pair_begin(
        &mut self,
        now_ms: u64,
        for_ms: u64,
        radio: &mut impl Radio,
    ) -> Result<(), PairRefused> {
        self.st.set_time(now_ms);
        if self.st.pair.pending.is_some() {
            radio.log(
                "PAD-HOST",
                format_args!("pairing window refused: a new pairing waits to be stored"),
            );
            return Err(PairRefused::SaveWaiting);
        }
        let mut io = Glue {
            radio: &mut *radio,
            st: &mut self.st,
        };
        let r = self.host.open_pairing(now_ms, for_ms, &mut io);
        if r.is_ok() {
            self.st.pair.clear_found();
            if self.st.injecting {
                self.inject_end(now_ms, 0x16, radio);
            }
        }
        if !self.host.linked() {
            self.st.link_handle = None;
        }
        self.follow(radio);
        r.map_err(PairRefused::Host)
    }

    /// Found pad number `i` (0 first, in the order heard), as the console
    /// shows it. The list only grows during one search; it is emptied by
    /// the next [`PadHost::pair_begin`] and by [`PadHost::pair_cancel`].
    pub fn pair_found(&self, i: usize) -> Option<FoundPad> {
        let a = self.st.pair.found(i)?;
        let n = usize::from(self.st.pair.found_name_len[i]);
        Some(FoundPad::new(tail_of(a), &self.st.pair.found_name[i][..n]))
    }

    /// How many pads the search found so far.
    pub fn pair_found_count(&self) -> usize {
        usize::from(self.st.pair.found_len)
    }

    /// The address of found pad `i`, if the window is still searching and
    /// that pad's tail is `tail` (the one the user saw). Sends nothing.
    /// The board binds its gate to it before [`PadHost::pair_choose`].
    pub fn pair_target(&self, i: usize, tail: [u8; 2]) -> Option<PadAddr> {
        if !matches!(self.host.pairing_window(), Some(Window::Scan { .. })) {
            return None;
        }
        let a = self.st.pair.found(i)?;
        (tail_of(a) == tail).then_some(PadAddr(a))
    }

    /// The user picked found pad `i`: stop searching, call it, pair it.
    /// The window then ends `for_ms` from now at the latest (never later
    /// than 120 s after it opened). Refused when there is no pad `i`, and
    /// when pad-proto's host refuses: not searching, a link up, or four
    /// pads saved and this one new ([`PairError::Full`]; a saved pad may
    /// be paired again).
    pub fn pair_choose(
        &mut self,
        i: usize,
        now_ms: u64,
        for_ms: u64,
        radio: &mut impl Radio,
    ) -> Result<(), PairRefused> {
        self.st.set_time(now_ms);
        let Some(addr) = self.st.pair.found(i) else {
            radio.log(
                "PAD-HOST",
                format_args!("pairing: no found pad number {}", i),
            );
            return Err(PairRefused::NoSuchPad);
        };
        let mut io = Glue {
            radio: &mut *radio,
            st: &mut self.st,
        };
        let r = self.host.pair_with(addr, now_ms, for_ms, &mut io);
        self.follow(radio);
        r.map_err(PairRefused::Host)
    }

    /// Stops the window: a search or call under way stops, a pad being
    /// paired is let go and its new key dropped, the found list is
    /// emptied. A new pairing not yet taken is zeroed, and its key leaves
    /// the host's table too (so the table matches the flash, which never
    /// got it): that pad is let go. True when there was something to stop.
    pub fn pair_cancel(&mut self, now_ms: u64, radio: &mut impl Radio) -> bool {
        self.st.set_time(now_ms);
        let mut did = false;
        if let Some(addr) = self.st.pair.pending_addr() {
            self.st.pair.drop_pending();
            radio.log(
                "PAD-HOST",
                format_args!(
                    "pairing cancelled before the new key was stored: dropped addr={}",
                    PadAddr(addr)
                ),
            );
            let mut io = Glue {
                radio: &mut *radio,
                st: &mut self.st,
            };
            self.host.forget_key_of(addr, now_ms, &mut io);
            did = true;
        }
        let mut io = Glue {
            radio: &mut *radio,
            st: &mut self.st,
        };
        did |= self.host.close_pairing(now_ms, &mut io);
        self.st.pair.clear_found();
        if !self.host.linked() {
            self.st.link_handle = None;
        }
        self.follow(radio);
        did
    }

    /// Where the window is.
    pub fn pair_phase(&self) -> PairPhase {
        match self.host.pairing_window() {
            None => PairPhase::Off,
            Some(Window::Scan { .. }) => PairPhase::Searching,
            Some(Window::Pair { .. }) => PairPhase::Pairing,
        }
    }

    /// When the open window ends by itself (the caller's milliseconds).
    pub fn pair_deadline(&self) -> Option<u64> {
        self.host.pairing_window().map(Window::until)
    }

    /// How the last window ended; None while one is open and before the
    /// first. Stays until the next [`PadHost::pair_begin`].
    pub fn pair_outcome(&self) -> Option<PairEnd> {
        self.host.pairing_outcome()
    }

    /// The new pairing, once: after a window ended [`PairEnd::Saved`].
    /// The board writes it to flash (pad-store's `save_new`); this host
    /// already uses the key. Zeros are left in its place here.
    pub fn take_pending_save(&mut self) -> Option<PendingSave> {
        self.st.pair.take_pending()
    }

    /// The tail of the saved pad at `place` (0 to 3, oldest first).
    pub fn saved_tail(&self, place: usize) -> Option<[u8; 2]> {
        self.host.key_addr(place).map(tail_of)
    }

    /// The place of the saved pad whose link is up now (outside a window).
    pub fn connected_place(&self) -> Option<usize> {
        if !self.host.linked() || self.host.pairing_open() {
            return None;
        }
        let a = self.st.pair.link_addr?;
        // The saved pads fill places 0, 1, ... without gaps.
        (0..)
            .map_while(|i| self.host.key_addr(i).map(|k| (i, k)))
            .find(|&(_, k)| k == a)
            .map(|(i, _)| i)
    }

    /// The saved pad at `place`, as the console shows it.
    pub fn saved_pad(&self, place: usize) -> Option<SavedPad> {
        Some(SavedPad {
            tail: self.saved_tail(place)?,
            connected: self.connected_place() == Some(place),
        })
    }

    /// Forgets the saved pad at `place`: its key leaves this host's table
    /// (the others keep their order, so places after it move down), a new
    /// pairing of it not yet taken is zeroed, and if it is linked it is
    /// hung up; a window pairing it ends. Returns its address, for the
    /// board's store and gate. Nothing is written to flash here.
    pub fn forget(&mut self, place: usize, now_ms: u64, radio: &mut impl Radio) -> Option<PadAddr> {
        let addr = self.host.key_addr(place)?;
        self.st.set_time(now_ms);
        if self.st.pair.pending_addr() == Some(addr) {
            self.st.pair.drop_pending();
        }
        let mut io = Glue {
            radio: &mut *radio,
            st: &mut self.st,
        };
        self.host.forget_key_of(addr, now_ms, &mut io);
        if !self.host.linked() {
            self.st.link_handle = None;
        }
        self.follow(radio);
        Some(PadAddr(addr))
    }
}
