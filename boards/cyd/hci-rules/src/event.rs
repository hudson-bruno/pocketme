//! The events the firmware reads, and device addresses that never print
//! in full.

use core::fmt;

use crate::{code, kind};

/// A device address, as it comes in an HCI packet (lowest byte first).
/// Addresses are personal: `Debug` and `Display` both show only the last
/// two bytes of the written form ("..cd:ef" for AB:..:CD:EF), and nothing
/// here prints the rest.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BdAddr([u8; 6]);

impl BdAddr {
    pub fn new(bytes: [u8; 6]) -> Self {
        BdAddr(bytes)
    }

    /// The six bytes as HCI has them (for an answer to the controller,
    /// never for a log).
    pub fn bytes(&self) -> &[u8; 6] {
        &self.0
    }

    /// The last two bytes of the written address (the first two in HCI
    /// order), in written order.
    pub fn tail(&self) -> [u8; 2] {
        [self.0[1], self.0[0]]
    }
}

impl fmt::Display for BdAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [a, b] = self.tail();
        write!(f, "..{a:02x}:{b:02x}")
    }
}

impl fmt::Debug for BdAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// An event from the controller, read from its packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event<'a> {
    /// A command finished. `ret` is what it returns (its first byte is
    /// the status for every command used here). `credits`: how many
    /// commands the controller can take now.
    CommandComplete {
        credits: u8,
        opcode: u16,
        ret: &'a [u8],
    },
    /// A command was taken (status 0) or refused.
    CommandStatus {
        status: u8,
        credits: u8,
        opcode: u16,
    },
    /// A device asks to connect.
    ConnectionRequest {
        addr: BdAddr,
        class: u32,
        link_type: u8,
    },
    /// A connection is up (status 0) or did not come up (its reason).
    ConnectionComplete {
        status: u8,
        handle: u16,
        addr: BdAddr,
        link_type: u8,
    },
    /// A connection ended.
    DisconnectionComplete { status: u8, handle: u16, reason: u8 },
    /// The controller found a fault in itself.
    HardwareError { code: u8 },
    /// Any other event: only its code.
    Other { code: u8 },
}

/// Reads a whole event packet (kind byte first). `None` if it is not an
/// event, its length does not match its header, or a known event is too
/// short.
pub fn parse_event(p: &[u8]) -> Option<Event<'_>> {
    let (&k, rest) = p.split_first()?;
    if k != kind::EVENT {
        return None;
    }
    let (&c, rest) = rest.split_first()?;
    let (&n, params) = rest.split_first()?;
    if params.len() != usize::from(n) {
        return None;
    }
    let u16_at = |i: usize| Some(u16::from_le_bytes([*params.get(i)?, *params.get(i + 1)?]));
    let addr_at = |i: usize| -> Option<BdAddr> {
        let b: [u8; 6] = params.get(i..i + 6)?.try_into().ok()?;
        Some(BdAddr(b))
    };
    let ev = match c {
        code::COMMAND_COMPLETE => Event::CommandComplete {
            credits: *params.first()?,
            opcode: u16_at(1)?,
            ret: &params[3..],
        },
        code::COMMAND_STATUS if params.len() == 4 => Event::CommandStatus {
            status: params[0],
            credits: params[1],
            opcode: u16_at(2)?,
        },
        code::CONNECTION_REQUEST if params.len() == 10 => Event::ConnectionRequest {
            addr: addr_at(0)?,
            class: u32::from_le_bytes([params[6], params[7], params[8], 0]),
            link_type: params[9],
        },
        code::CONNECTION_COMPLETE if params.len() == 11 => Event::ConnectionComplete {
            status: params[0],
            handle: u16_at(1)?,
            addr: addr_at(3)?,
            link_type: params[9],
        },
        code::DISCONNECTION_COMPLETE if params.len() == 4 => Event::DisconnectionComplete {
            status: params[0],
            handle: u16_at(1)?,
            reason: params[3],
        },
        code::HARDWARE_ERROR if params.len() == 1 => Event::HardwareError { code: params[0] },
        code::COMMAND_STATUS
        | code::CONNECTION_REQUEST
        | code::CONNECTION_COMPLETE
        | code::DISCONNECTION_COMPLETE
        | code::HARDWARE_ERROR => return None,
        other => Event::Other { code: other },
    };
    Some(ev)
}
