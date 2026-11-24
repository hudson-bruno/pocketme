//! The gate: the only packets the firmware may send to the controller.
//! Every packet passes here before it reaches the radio
//! (boards/cyd/src/hci.rs); anything not on the list is refused and
//! counted, and never sent.
//!
//! The list is what the reconnect-only gamepad host (crates/pad-host,
//! pad-proto's `Config::reconnect_only`) needs, each command with exactly
//! its own parameter length, and nothing more:
//!
//! - set-up: Reset, Read Local Version / Supported Features / Buffer
//!   Size / BD_ADDR, Set Event Mask (its length only), Write Simple
//!   Pairing Mode, Write Default Link Policy, Write Local Name, Write
//!   Class of Device, Write Inquiry Mode (a setting only: it starts no
//!   inquiry), Write Page Timeout;
//! - Write Scan Enable with page scan only or off: never inquiry scan,
//!   so the console is never discoverable;
//! - a call: Accept Connection Request and Link Key Request Reply ONLY
//!   to a saved pad's address ([`Gate::allow`]), and Remote Name Request
//!   only to one too (to any other address it would page a device);
//!   Reject Connection Request to anyone;
//! - every "no" answer of pairing: Link Key, PIN Code, IO Capability,
//!   User Confirmation and User Passkey negative replies;
//! - on a link (by handle): Authentication Requested, Set Connection
//!   Encryption, Read Remote Supported / Extended Features, Read Remote
//!   Version, Read Encryption Key Size, Disconnect;
//! - Write Link Supervision Timeout, only for the handle of a link to a
//!   saved pad (the gate learns it from the controller's Connection
//!   Complete and forgets it at Disconnection Complete:
//!   [`Gate::observe`]) and never shorter than 1 s nor 0 (no timeout);
//! - ACL data whose length matches its header.
//!
//! Never: Inquiry (searching), Create Connection (calling anyone), any
//! "yes" of pairing (PIN Code Reply, IO Capability Reply, User
//! Confirmation Reply), inquiry scan, anything Low Energy, any command
//! not named above.
//!
//! With the crate feature `pair` the gate also has a pairing window
//! (module `pair`): [`Gate::check_at`] and [`Gate::observe_at`] take the
//! time and let a short list of pairing commands out while a window,
//! opened from outside with [`Gate::arm`], is open. [`Gate::check`] and
//! [`Gate::observe`] stay the rules above, window or not.

use crate::{Event, SCAN_OFF, SCAN_PAGE_ONLY, kind, op, packet_len, parse_event};

#[cfg(feature = "pair")]
mod pair;
#[cfg(feature = "pair")]
pub use pair::{FOUND_MAX, INQUIRY_UNITS_MAX, PAIR_WINDOW_MAX_MS, PairPhase, PairRefusal};

/// The shortest supervision timeout the gate lets through: 0x0640 slots
/// of 0.625 ms = 1 s (pad-proto's `LSTO_MIN`).
pub const LSTO_MIN: u16 = 0x0640;

/// Why the gate refused a packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Not a command or ACL data (an event, SCO, an unknown kind).
    NotACommand,
    /// Its length does not match its header.
    BadLength,
    /// A command not on the list (its code).
    NotAllowed(u16),
    /// A command on the list with parameters it may not have (its code).
    BadParams(u16),
    /// A command that names a device which is not a saved pad (its
    /// code): an accept, a key reply, a name request; or a link that is
    /// not a saved pad's (a supervision timeout).
    NotSaved(u16),
    /// A command the pairing window lets out only for the pad it is
    /// bound to, naming another device (its code).
    #[cfg(feature = "pair")]
    NotBound(u16),
}

/// Saved pads the gate knows at most (pad-proto keeps four).
pub const SAVED: usize = 4;

/// What the gate notes of a link that is up: its handle.
#[cfg(not(feature = "pair"))]
type Link = u16;

/// What the gate notes of a link that is up: the device's address (so
/// [`Gate::forget`] can drop it) and its handle.
#[cfg(feature = "pair")]
#[derive(Clone, Copy, PartialEq, Eq)]
struct Link {
    addr: [u8; 6],
    handle: u16,
}

#[cfg(not(feature = "pair"))]
fn handle_of(l: &Link) -> u16 {
    *l
}

#[cfg(feature = "pair")]
fn handle_of(l: &Link) -> u16 {
    l.handle
}

/// The gate, with the saved pads' addresses (given once at start, from
/// the pairings read only).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Gate {
    saved: [Option<[u8; 6]>; SAVED],
    /// The links to saved pads that are up now (with `pair`, also the
    /// link to the pad a pairing window is bound to).
    links: [Option<Link>; SAVED],
    /// The pairing window: closed unless armed.
    #[cfg(feature = "pair")]
    window: pair::Window,
}

/// Addresses are personal: only how many (and, with `pair`, the
/// window's phase as a word while one is open).
impl core::fmt::Debug for Gate {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Gate({} saved, {} linked",
            self.saved.iter().flatten().count(),
            self.links.iter().flatten().count()
        )?;
        #[cfg(feature = "pair")]
        if self.phase() != PairPhase::Off {
            write!(f, ", {}", self.phase())?;
        }
        write!(f, ")")
    }
}

impl Default for Gate {
    fn default() -> Self {
        Self::new()
    }
}

impl Gate {
    /// A gate that knows no saved pad: every accept is refused.
    pub const fn new() -> Self {
        Gate {
            saved: [None; SAVED],
            links: [None; SAVED],
            #[cfg(feature = "pair")]
            window: pair::Window::Off,
        }
    }

    /// Every packet from the controller passes here (before the host
    /// sees it): a link to a saved pad that comes up (Connection
    /// Complete, status 0, ACL) is noted by its handle; a link that ends
    /// (Disconnection Complete, status 0) is forgotten. Anything else
    /// changes nothing.
    pub fn observe(&mut self, packet: &[u8]) {
        match parse_event(packet) {
            Some(Event::ConnectionComplete {
                status: 0,
                handle,
                addr,
                link_type: 1,
            }) if self.is_saved(addr.bytes()) => {
                self.note_link(*addr.bytes(), handle & 0x0FFF);
            }
            Some(Event::DisconnectionComplete {
                status: 0, handle, ..
            }) => {
                let handle = handle & 0x0FFF;
                for s in self.links.iter_mut() {
                    if s.as_ref().map(handle_of) == Some(handle) {
                        *s = None;
                    }
                }
            }
            _ => {}
        }
    }

    /// Notes a link that came up (its handle without the flag bits).
    #[cfg_attr(not(feature = "pair"), allow(unused_variables))]
    fn note_link(&mut self, addr: [u8; 6], handle: u16) {
        #[cfg(not(feature = "pair"))]
        let link: Link = handle;
        #[cfg(feature = "pair")]
        let link = Link { addr, handle };
        if let Some(s) = self
            .links
            .iter_mut()
            .flatten()
            .find(|l| handle_of(l) == handle)
        {
            // Noted already. With `pair` the address is updated, in case
            // the handle came back for another device after an end the
            // gate never saw.
            *s = link;
            return;
        }
        // Full (ends the gate never saw): the oldest noted goes, so the
        // newest link is always known.
        if let Some(slot) = self.links.iter_mut().find(|s| s.is_none()) {
            *slot = Some(link);
        } else {
            self.links.rotate_left(1);
            self.links[SAVED - 1] = Some(link);
        }
    }

    /// How many links to saved pads are up (as the gate saw them).
    pub fn linked(&self) -> usize {
        self.links.iter().flatten().count()
    }

    fn is_linked(&self, handle: u16) -> bool {
        self.links.iter().flatten().any(|l| handle_of(l) == handle)
    }

    /// A saved pad's address (as HCI sends it, low byte first): it may be
    /// accepted and given its key. False when the gate is full. With
    /// `pair`, allowing the address a window has just paired closes that
    /// window (its key is saved: the normal rules now cover the pad).
    pub fn allow(&mut self, addr: [u8; 6]) -> bool {
        if self.is_saved(&addr) {
            #[cfg(feature = "pair")]
            self.paired_saved(addr);
            return true;
        }
        match self.saved.iter_mut().find(|s| s.is_none()) {
            Some(slot) => {
                *slot = Some(addr);
                #[cfg(feature = "pair")]
                self.paired_saved(addr);
                true
            }
            None => false,
        }
    }

    /// How many saved pads it knows.
    pub fn saved(&self) -> usize {
        self.saved.iter().flatten().count()
    }

    fn is_saved(&self, addr: &[u8]) -> bool {
        self.saved.iter().flatten().any(|s| s[..] == *addr)
    }

    /// Whether `packet` (a whole packet, kind byte first) may go to the
    /// controller.
    pub fn check(&self, packet: &[u8]) -> Result<(), Refusal> {
        let k = match packet.first() {
            Some(&k) if k == kind::COMMAND || k == kind::ACL => k,
            _ => return Err(Refusal::NotACommand),
        };
        if packet_len(packet) != Some(packet.len()) {
            return Err(Refusal::BadLength);
        }
        if k == kind::ACL {
            return Ok(());
        }
        let opcode = u16::from_le_bytes([packet[1], packet[2]]);
        let p = &packet[4..];
        let len = |n: usize| p.len() == n;
        let ok = match opcode {
            op::RESET
            | op::READ_LOCAL_VERSION
            | op::READ_LOCAL_FEATURES
            | op::READ_BUFFER_SIZE
            | op::READ_BD_ADDR => p.is_empty(),
            op::SET_EVENT_MASK => len(8),
            op::WRITE_SIMPLE_PAIRING_MODE | op::WRITE_INQUIRY_MODE => len(1),
            op::WRITE_DEFAULT_LINK_POLICY_SETTINGS | op::WRITE_PAGE_TIMEOUT => len(2),
            op::WRITE_LOCAL_NAME => len(248),
            op::WRITE_CLASS_OF_DEVICE => len(3),
            op::WRITE_SCAN_ENABLE => matches!(p, [SCAN_OFF] | [SCAN_PAGE_ONLY]),
            op::ACCEPT_CONNECTION_REQUEST => {
                // Address, then the role: 0 become central, 1 stay.
                if !len(7) || p[6] > 1 {
                    false
                } else if !self.is_saved(&p[..6]) {
                    return Err(Refusal::NotSaved(opcode));
                } else {
                    true
                }
            }
            op::LINK_KEY_REQUEST_REPLY | op::REMOTE_NAME_REQUEST => {
                // Address and key (16), or address, page settings (4).
                let want = if opcode == op::LINK_KEY_REQUEST_REPLY {
                    22
                } else {
                    10
                };
                if !len(want) {
                    false
                } else if !self.is_saved(&p[..6]) {
                    return Err(Refusal::NotSaved(opcode));
                } else {
                    true
                }
            }
            op::REJECT_CONNECTION_REQUEST | op::IO_CAPABILITY_REQUEST_NEGATIVE_REPLY => len(7),
            op::LINK_KEY_REQUEST_NEGATIVE_REPLY
            | op::PIN_CODE_REQUEST_NEGATIVE_REPLY
            | op::USER_CONFIRMATION_REQUEST_NEGATIVE_REPLY
            | op::USER_PASSKEY_REQUEST_NEGATIVE_REPLY => len(6),
            op::AUTHENTICATION_REQUESTED
            | op::READ_REMOTE_SUPPORTED_FEATURES
            | op::READ_REMOTE_VERSION_INFORMATION
            | op::READ_ENCRYPTION_KEY_SIZE => len(2),
            op::SET_CONNECTION_ENCRYPTION | op::READ_REMOTE_EXTENDED_FEATURES | op::DISCONNECT => {
                len(3)
            }
            op::WRITE_LINK_SUPERVISION_TIMEOUT => {
                // Handle, then the timeout (slots): 1 s or more.
                if !len(4) || u16::from_le_bytes([p[2], p[3]]) < LSTO_MIN {
                    false
                } else if !self.is_linked(u16::from_le_bytes([p[0], p[1]])) {
                    return Err(Refusal::NotSaved(opcode));
                } else {
                    true
                }
            }
            other => return Err(Refusal::NotAllowed(other)),
        };
        if ok {
            Ok(())
        } else {
            Err(Refusal::BadParams(opcode))
        }
    }
}

/// The gate with no saved pad: [`Gate::new`]`.check(packet)`.
pub fn gate(packet: &[u8]) -> Result<(), Refusal> {
    Gate::new().check(packet)
}
