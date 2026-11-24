//! The CYD's rules for the gamepad radio's HCI bytes (feature `pad` of
//! boards/cyd), as plain logic: no radio, no pins. The firmware's byte
//! path (boards/cyd/src/hci.rs, boards/cyd/src/pad_radio.rs) puts every
//! packet for the controller through [`Gate`]; the PC tests (tests/)
//! test it, also against the real gamepad host (crates/pad-host).
//!
//! HCI packets here are whole packets with their one-byte kind first
//! (the "H4" form: 0x01 command, 0x02 ACL data, 0x03 SCO data, 0x04
//! event), exactly the bytes that go to or come from the controller.
//!
//! The promise: the console is reconnect-only. It is never discoverable,
//! never searches, never calls a device, and never says yes to pairing;
//! only a saved pad may connect, and only a saved pad is given its key.
//! The gate holds that promise on its own, whatever the host above it
//! asks for (pad-proto's `Config::reconnect_only` holds it one layer up).
//!
//! The one exception is the pairing window of the crate feature `pair`
//! (off by default; without it none of it is compiled in): opened from
//! outside ([`Gate::arm`], which the firmware calls only after the BOOT
//! button was held), it lets the console search for pads, call the one
//! pad the user chose ([`Gate::bind`]) and pair it with "just works"
//! Secure Simple Pairing, and it closes by itself at its deadline (at
//! most two minutes), on any failure, and once the pad's key is saved.
//! Even then the console is never discoverable and never answers a PIN.
//! The gate never reads or keeps a link key.
//!
//! - [`packet_len`]: how long a packet is, from its header.
//! - [`Gate`]: the only packets the firmware may send to the controller
//!   (the reconnect-only gamepad host's commands, an accept or a key only
//!   to a saved pad, ACL data). Everything else is refused before it
//!   reaches the radio.
//! - [`Event`] and [`parse_event`]: the events the gate reads.
//! - [`BdAddr`]: a device address that only ever prints its last two
//!   bytes (device addresses are personal).
//! - [`Cmd`] and its builders, [`answer_connection_request`], [`Setup`]
//!   and [`EVENT_MASK`]: a small start-up sequence and its commands, kept
//!   and tested here but not used by the firmware, whose commands all
//!   come from pad-proto (crates/pad-proto). In particular the firmware
//!   sends pad-proto's event mask, not [`EVENT_MASK`], and the gate
//!   checks only that a Set Event Mask is 8 bytes long.
//!
//! No std, no heap, no unsafe.

#![no_std]
#![forbid(unsafe_code)]

mod event;
mod gate;
mod setup;

pub use event::{BdAddr, Event, parse_event};
#[cfg(feature = "pair")]
pub use gate::{FOUND_MAX, INQUIRY_UNITS_MAX, PAIR_WINDOW_MAX_MS, PairPhase, PairRefusal};
pub use gate::{Gate, LSTO_MIN, Refusal, SAVED, gate};
pub use setup::{Setup, Step};

/// The kinds of packet (the first byte of each).
pub mod kind {
    pub const COMMAND: u8 = 0x01;
    pub const ACL: u8 = 0x02;
    pub const SCO: u8 = 0x03;
    pub const EVENT: u8 = 0x04;
}

/// The command codes ("opcodes") the firmware uses (the same numbers as
/// pad-proto's `hci::op`), and the ones the gate refuses outside a
/// pairing window (feature `pair`) or always.
pub mod op {
    // Link control (OGF 0x01).
    /// Inquiry: a search for devices. Only in a pairing window's search.
    pub const INQUIRY: u16 = 0x0401;
    /// Inquiry Cancel: stops a search ([`crate::Gate::check_at`] lets it
    /// out at any time; [`crate::Gate::check`] never).
    pub const INQUIRY_CANCEL: u16 = 0x0402;
    /// Create Connection: calling a device. Only to the pad a pairing
    /// window is bound to.
    pub const CREATE_CONNECTION: u16 = 0x0405;
    pub const DISCONNECT: u16 = 0x0406;
    /// Create Connection Cancel: stops a call ([`crate::Gate::check_at`]
    /// lets it out at any time; [`crate::Gate::check`] never).
    pub const CREATE_CONNECTION_CANCEL: u16 = 0x0408;
    pub const ACCEPT_CONNECTION_REQUEST: u16 = 0x0409;
    /// Reject Connection Request (link control 0x000A): the answer to a
    /// device that asks to connect.
    pub const REJECT_CONNECTION_REQUEST: u16 = 0x040A;
    pub const LINK_KEY_REQUEST_REPLY: u16 = 0x040B;
    pub const LINK_KEY_REQUEST_NEGATIVE_REPLY: u16 = 0x040C;
    /// PIN Code Request Reply: old PIN pairing. Never allowed.
    pub const PIN_CODE_REQUEST_REPLY: u16 = 0x040D;
    pub const PIN_CODE_REQUEST_NEGATIVE_REPLY: u16 = 0x040E;
    pub const AUTHENTICATION_REQUESTED: u16 = 0x0411;
    pub const SET_CONNECTION_ENCRYPTION: u16 = 0x0413;
    pub const REMOTE_NAME_REQUEST: u16 = 0x0419;
    pub const READ_REMOTE_SUPPORTED_FEATURES: u16 = 0x041B;
    pub const READ_REMOTE_EXTENDED_FEATURES: u16 = 0x041C;
    pub const READ_REMOTE_VERSION_INFORMATION: u16 = 0x041D;
    /// IO Capability Request Reply: pairing. Only to the pad a pairing
    /// window is bound to.
    pub const IO_CAPABILITY_REQUEST_REPLY: u16 = 0x042B;
    /// User Confirmation Request Reply: pairing. Only to the pad a
    /// pairing window is bound to.
    pub const USER_CONFIRMATION_REQUEST_REPLY: u16 = 0x042C;
    pub const USER_CONFIRMATION_REQUEST_NEGATIVE_REPLY: u16 = 0x042D;
    pub const USER_PASSKEY_REQUEST_NEGATIVE_REPLY: u16 = 0x042F;
    pub const IO_CAPABILITY_REQUEST_NEGATIVE_REPLY: u16 = 0x0434;
    // Link policy (OGF 0x02).
    pub const WRITE_DEFAULT_LINK_POLICY_SETTINGS: u16 = 0x080F;
    // Controller and baseband (OGF 0x03).
    /// Set Event Mask (controller and baseband 0x0001).
    pub const SET_EVENT_MASK: u16 = 0x0C01;
    /// Reset (controller and baseband 0x0003).
    pub const RESET: u16 = 0x0C03;
    pub const WRITE_LOCAL_NAME: u16 = 0x0C13;
    pub const WRITE_PAGE_TIMEOUT: u16 = 0x0C18;
    /// Write Scan Enable (controller and baseband 0x001A).
    pub const WRITE_SCAN_ENABLE: u16 = 0x0C1A;
    pub const WRITE_CLASS_OF_DEVICE: u16 = 0x0C24;
    pub const WRITE_INQUIRY_MODE: u16 = 0x0C45;
    pub const WRITE_SIMPLE_PAIRING_MODE: u16 = 0x0C56;
    /// Write Link Supervision Timeout (controller and baseband 0x0037):
    /// allowed only for the handle of a saved pad's link (see
    /// [`crate::Gate::observe`]).
    pub const WRITE_LINK_SUPERVISION_TIMEOUT: u16 = 0x0C37;
    // Informational (OGF 0x04).
    /// Read Local Version Information (informational 0x0001).
    pub const READ_LOCAL_VERSION: u16 = 0x1001;
    pub const READ_LOCAL_FEATURES: u16 = 0x1003;
    /// Read Buffer Size (informational 0x0005).
    pub const READ_BUFFER_SIZE: u16 = 0x1005;
    pub const READ_BD_ADDR: u16 = 0x1009;
    // Status (OGF 0x05).
    pub const READ_ENCRYPTION_KEY_SIZE: u16 = 0x1408;
}

/// The event codes the firmware reads.
pub mod code {
    pub const CONNECTION_COMPLETE: u8 = 0x03;
    pub const CONNECTION_REQUEST: u8 = 0x04;
    pub const DISCONNECTION_COMPLETE: u8 = 0x05;
    pub const COMMAND_COMPLETE: u8 = 0x0E;
    pub const COMMAND_STATUS: u8 = 0x0F;
    pub const HARDWARE_ERROR: u8 = 0x10;
}

/// Write Scan Enable's values: page scan only (connectable, not
/// discoverable), or nothing. Inquiry scan (bit 0, discoverable) is never
/// allowed.
pub const SCAN_OFF: u8 = 0x00;
pub const SCAN_PAGE_ONLY: u8 = 0x02;

/// The reason given when a device asks to connect and is refused:
/// "Connection Rejected due to Unacceptable BD_ADDR".
pub const REJECT_UNACCEPTABLE_ADDR: u8 = 0x0F;

/// An event mask for [`Setup`]: the standard default set (bits 0 to 44)
/// without the three that only come after a search for devices (Inquiry
/// Complete, Inquiry Result, Inquiry Result with RSSI). The firmware does
/// not send it: it sends pad-proto's mask, which has the search and
/// pairing events on, and the gate checks only a Set Event Mask's length.
pub const EVENT_MASK: u64 = 0x0000_1FFF_FFFF_FFFF & !(1 | (1 << 1) | (1 << 33));

/// The longest packet the controller can hand over: an ACL packet of
/// 1,021 data bytes (the ESP32 controller's classic buffer size, from
/// Read Buffer Size) plus its 5 header bytes, rounded up. A longer one is
/// cut and counted as bad (boards/cyd/src/hci.rs).
pub const MAX_PACKET: usize = 1028;

/// How long the whole packet that starts with `b` is, from its header,
/// counting the kind byte. `None` while the header is not all there or
/// for an unknown kind.
pub fn packet_len(b: &[u8]) -> Option<usize> {
    let (&k, rest) = b.split_first()?;
    match k {
        kind::COMMAND | kind::SCO => rest.get(2).map(|&n| 4 + usize::from(n)),
        kind::ACL => {
            let n = u16::from_le_bytes([*rest.get(2)?, *rest.get(3)?]);
            Some(5 + usize::from(n))
        }
        kind::EVENT => rest.get(1).map(|&n| 3 + usize::from(n)),
        _ => None,
    }
}

/// A command packet, built here, at most 12 bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Cmd {
    bytes: [u8; 12],
    len: u8,
}

/// Only the command code: a Reject Connection Request carries a device
/// address, which never prints.
impl core::fmt::Debug for Cmd {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Cmd({:#06x})", self.opcode())
    }
}

impl Cmd {
    fn new(opcode: u16, params: &[u8]) -> Self {
        let mut bytes = [0u8; 12];
        let [lo, hi] = opcode.to_le_bytes();
        bytes[0] = kind::COMMAND;
        bytes[1] = lo;
        bytes[2] = hi;
        bytes[3] = params.len() as u8;
        bytes[4..4 + params.len()].copy_from_slice(params);
        Cmd {
            bytes,
            len: 4 + params.len() as u8,
        }
    }

    /// The whole packet, kind byte first.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }

    /// Its command code.
    pub fn opcode(&self) -> u16 {
        u16::from_le_bytes([self.bytes[1], self.bytes[2]])
    }

    pub fn reset() -> Self {
        Self::new(op::RESET, &[])
    }

    pub fn read_local_version() -> Self {
        Self::new(op::READ_LOCAL_VERSION, &[])
    }

    pub fn read_buffer_size() -> Self {
        Self::new(op::READ_BUFFER_SIZE, &[])
    }

    pub fn set_event_mask(mask: u64) -> Self {
        Self::new(op::SET_EVENT_MASK, &mask.to_le_bytes())
    }

    /// Page scan on (connectable, not discoverable) or all scans off.
    pub fn write_scan_enable(page_scan: bool) -> Self {
        let v = if page_scan { SCAN_PAGE_ONLY } else { SCAN_OFF };
        Self::new(op::WRITE_SCAN_ENABLE, &[v])
    }

    /// The answer to a device asking to connect: no.
    pub fn reject_connection(addr: &BdAddr) -> Self {
        let mut p = [0u8; 7];
        p[..6].copy_from_slice(addr.bytes());
        p[6] = REJECT_UNACCEPTABLE_ADDR;
        Self::new(op::REJECT_CONNECTION_REQUEST, &p)
    }
}

/// This round's answer to a device that asks to connect: always no (the
/// firmware has no host yet that could take a connection; whatever the
/// device is, it is refused).
pub fn answer_connection_request(addr: &BdAddr) -> Cmd {
    Cmd::reject_connection(addr)
}
