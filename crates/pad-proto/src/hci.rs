//! HCI: the commands our program (the "host") sends to the radio's own
//! firmware (the "controller"), and the events the controller sends back.
//!
//! Everything is built and read by hand from the Bluetooth Core
//! Specification (volume 4, part E). An HCI command packet is:
//! type 0x01, opcode (2 bytes, low first), parameter length (1), parameters.
//! An event is: type 0x04, event code (1), parameter length (1), parameters.

use core::fmt;

use crate::buf::{Packet, u16_at, u24_at, u32_at};

/// A Bluetooth device address. Kept in the order it travels on the wire
/// (least significant byte first); printed the usual way round.
///
/// An address is personal (it names one person's device), so `Display`
/// and `Debug` show only its last two bytes: "xx:xx:xx:xx:EE:FF". Enough
/// to tell two devices apart in a log, not enough to find one.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct BdAddr(pub [u8; 6]);

impl BdAddr {
    /// "AA:BB:CC:DD:EE:FF" (the usual way of writing it) to an address.
    pub fn parse(text: &str) -> Option<Self> {
        let mut out = [0_u8; 6];
        let mut parts = text.trim().split(':');
        for i in (0..6).rev() {
            out[i] = u8::from_str_radix(parts.next()?, 16).ok()?;
        }
        parts.next().is_none().then_some(Self(out))
    }

    fn at(b: &[u8], at: usize) -> Option<Self> {
        let mut a = [0; 6];
        a.copy_from_slice(b.get(at..at + 6)?);
        Some(Self(a))
    }
}

impl fmt::Display for BdAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let a = self.0;
        write!(f, "xx:xx:xx:xx:{:02X}:{:02X}", a[1], a[0])
    }
}

impl fmt::Debug for BdAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BdAddr({self})")
    }
}

/// A link key: the secret two paired devices share. Its `Debug` output
/// never shows the bytes, so it cannot end up in a log by mistake.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LinkKey(pub [u8; 16]);

impl fmt::Debug for LinkKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LinkKey(hidden)")
    }
}

/// Command opcodes: group (OGF) in the top 6 bits, command (OCF) below.
pub mod op {
    // Link control (OGF 0x01): talking to other devices.
    pub const INQUIRY: u16 = 0x0401;
    pub const INQUIRY_CANCEL: u16 = 0x0402;
    pub const CREATE_CONNECTION: u16 = 0x0405;
    pub const DISCONNECT: u16 = 0x0406;
    pub const CREATE_CONNECTION_CANCEL: u16 = 0x0408;
    pub const ACCEPT_CONNECTION_REQUEST: u16 = 0x0409;
    pub const REJECT_CONNECTION_REQUEST: u16 = 0x040A;
    pub const LINK_KEY_REQUEST_REPLY: u16 = 0x040B;
    pub const LINK_KEY_REQUEST_NEGATIVE_REPLY: u16 = 0x040C;
    pub const PIN_CODE_REQUEST_REPLY: u16 = 0x040D;
    pub const PIN_CODE_REQUEST_NEGATIVE_REPLY: u16 = 0x040E;
    pub const AUTHENTICATION_REQUESTED: u16 = 0x0411;
    pub const SET_CONNECTION_ENCRYPTION: u16 = 0x0413;
    pub const REMOTE_NAME_REQUEST: u16 = 0x0419;
    pub const READ_REMOTE_SUPPORTED_FEATURES: u16 = 0x041B;
    pub const READ_REMOTE_EXTENDED_FEATURES: u16 = 0x041C;
    pub const READ_REMOTE_VERSION_INFORMATION: u16 = 0x041D;
    pub const IO_CAPABILITY_REQUEST_REPLY: u16 = 0x042B;
    pub const USER_CONFIRMATION_REQUEST_REPLY: u16 = 0x042C;
    pub const USER_CONFIRMATION_REQUEST_NEGATIVE_REPLY: u16 = 0x042D;
    pub const USER_PASSKEY_REQUEST_NEGATIVE_REPLY: u16 = 0x042F;
    pub const IO_CAPABILITY_REQUEST_NEGATIVE_REPLY: u16 = 0x0434;
    // Link policy (OGF 0x02).
    pub const WRITE_DEFAULT_LINK_POLICY_SETTINGS: u16 = 0x080F;
    // Controller and baseband (OGF 0x03): our own radio's settings.
    pub const SET_EVENT_MASK: u16 = 0x0C01;
    pub const RESET: u16 = 0x0C03;
    pub const WRITE_LOCAL_NAME: u16 = 0x0C13;
    pub const WRITE_PAGE_TIMEOUT: u16 = 0x0C18;
    pub const WRITE_SCAN_ENABLE: u16 = 0x0C1A;
    pub const WRITE_CLASS_OF_DEVICE: u16 = 0x0C24;
    pub const WRITE_INQUIRY_MODE: u16 = 0x0C45;
    pub const WRITE_SIMPLE_PAIRING_MODE: u16 = 0x0C56;
    /// Write Link Supervision Timeout (controller and baseband 0x0037):
    /// how long a link may stay silent before the controller ends it.
    /// Only the central of a link may write it.
    pub const WRITE_LINK_SUPERVISION_TIMEOUT: u16 = 0x0C37;
    // Information (OGF 0x04).
    pub const READ_LOCAL_VERSION: u16 = 0x1001;
    pub const READ_LOCAL_FEATURES: u16 = 0x1003;
    pub const READ_BUFFER_SIZE: u16 = 0x1005;
    pub const READ_BD_ADDR: u16 = 0x1009;
    // Status (OGF 0x05).
    pub const READ_ENCRYPTION_KEY_SIZE: u16 = 0x1408;

    /// A short name for the log.
    pub fn name(opcode: u16) -> &'static str {
        match opcode {
            INQUIRY => "inquiry",
            INQUIRY_CANCEL => "inquiry_cancel",
            CREATE_CONNECTION => "create_connection",
            DISCONNECT => "disconnect",
            CREATE_CONNECTION_CANCEL => "create_connection_cancel",
            ACCEPT_CONNECTION_REQUEST => "accept_connection",
            REJECT_CONNECTION_REQUEST => "reject_connection",
            LINK_KEY_REQUEST_REPLY => "link_key_reply",
            LINK_KEY_REQUEST_NEGATIVE_REPLY => "link_key_negative_reply",
            PIN_CODE_REQUEST_REPLY => "pin_code_reply",
            PIN_CODE_REQUEST_NEGATIVE_REPLY => "pin_code_negative_reply",
            AUTHENTICATION_REQUESTED => "authentication_requested",
            SET_CONNECTION_ENCRYPTION => "set_connection_encryption",
            REMOTE_NAME_REQUEST => "remote_name_request",
            READ_REMOTE_SUPPORTED_FEATURES => "read_remote_features",
            READ_REMOTE_EXTENDED_FEATURES => "read_remote_ext_features",
            READ_REMOTE_VERSION_INFORMATION => "read_remote_version",
            IO_CAPABILITY_REQUEST_REPLY => "io_capability_reply",
            USER_CONFIRMATION_REQUEST_REPLY => "user_confirmation_reply",
            USER_CONFIRMATION_REQUEST_NEGATIVE_REPLY => "user_confirmation_negative_reply",
            USER_PASSKEY_REQUEST_NEGATIVE_REPLY => "user_passkey_negative_reply",
            IO_CAPABILITY_REQUEST_NEGATIVE_REPLY => "io_capability_negative_reply",
            WRITE_DEFAULT_LINK_POLICY_SETTINGS => "write_default_link_policy",
            SET_EVENT_MASK => "set_event_mask",
            RESET => "reset",
            WRITE_LOCAL_NAME => "write_local_name",
            WRITE_PAGE_TIMEOUT => "write_page_timeout",
            WRITE_SCAN_ENABLE => "write_scan_enable",
            WRITE_CLASS_OF_DEVICE => "write_class_of_device",
            WRITE_INQUIRY_MODE => "write_inquiry_mode",
            WRITE_SIMPLE_PAIRING_MODE => "write_simple_pairing_mode",
            WRITE_LINK_SUPERVISION_TIMEOUT => "write_link_supervision_timeout",
            READ_LOCAL_VERSION => "read_local_version",
            READ_LOCAL_FEATURES => "read_local_features",
            READ_BUFFER_SIZE => "read_buffer_size",
            READ_BD_ADDR => "read_bd_addr",
            READ_ENCRYPTION_KEY_SIZE => "read_encryption_key_size",
            _ => "?",
        }
    }
}

/// Builds one command packet (type byte included).
pub fn command(opcode: u16, params: &[u8]) -> Packet {
    let mut p = Packet::new();
    p.put_u8(0x01);
    p.put_u16(opcode);
    p.put_u8(params.len().min(255) as u8);
    p.put(&params[..params.len().min(255)]);
    p
}

/// Builders for the parameters of the commands we send.
pub mod params {
    use super::BdAddr;

    /// Inquiry with the general access code (GIAC, 0x9E8B33): every
    /// discoverable device answers. `units` x 1.28 s; 0 answers = no limit.
    pub fn inquiry(units: u8) -> [u8; 5] {
        [0x33, 0x8B, 0x9E, units, 0x00]
    }

    /// Create Connection. `clock_offset` from the inquiry answer, if any.
    pub fn create_connection(addr: BdAddr, psrm: u8, clock_offset: Option<u16>) -> [u8; 13] {
        let mut p = [0_u8; 13];
        p[..6].copy_from_slice(&addr.0);
        // Packet types DM1, DH1, DM3, DH3, DM5, DH5.
        p[6..8].copy_from_slice(&0xCC18_u16.to_le_bytes());
        p[8] = psrm;
        p[9] = 0x00;
        // Bit 15 says "this clock offset is valid".
        let clk = clock_offset.map_or(0, |c| (c & 0x7FFF) | 0x8000);
        p[10..12].copy_from_slice(&clk.to_le_bytes());
        // The other side may ask to swap roles.
        p[12] = 0x01;
        p
    }

    pub fn handle(handle: u16) -> [u8; 2] {
        handle.to_le_bytes()
    }

    /// A handle and a 16-bit value (Write Link Supervision Timeout).
    pub fn handle_u16(handle: u16, v: u16) -> [u8; 4] {
        let h = handle.to_le_bytes();
        let v = v.to_le_bytes();
        [h[0], h[1], v[0], v[1]]
    }

    pub fn handle_u8(handle: u16, v: u8) -> [u8; 3] {
        let h = handle.to_le_bytes();
        [h[0], h[1], v]
    }

    pub fn addr(addr: BdAddr) -> [u8; 6] {
        addr.0
    }

    pub fn addr_u8(addr: BdAddr, v: u8) -> [u8; 7] {
        let mut p = [0; 7];
        p[..6].copy_from_slice(&addr.0);
        p[6] = v;
        p
    }

    pub fn remote_name(addr: BdAddr, psrm: u8, clock_offset: Option<u16>) -> [u8; 10] {
        let mut p = [0_u8; 10];
        p[..6].copy_from_slice(&addr.0);
        p[6] = psrm;
        let clk = clock_offset.map_or(0, |c| (c & 0x7FFF) | 0x8000);
        p[8..10].copy_from_slice(&clk.to_le_bytes());
        p
    }

    /// IO Capability Request Reply: address, IO capability, OOB data
    /// present, authentication requirements.
    pub fn io_capability(addr: BdAddr, io_cap: u8, auth_req: u8) -> [u8; 9] {
        let mut p = [0_u8; 9];
        p[..6].copy_from_slice(&addr.0);
        p[6] = io_cap;
        p[7] = 0x00;
        p[8] = auth_req;
        p
    }

    pub fn link_key_reply(addr: BdAddr, key: &[u8; 16]) -> [u8; 22] {
        let mut p = [0_u8; 22];
        p[..6].copy_from_slice(&addr.0);
        p[6..].copy_from_slice(key);
        p
    }

    pub fn pin_code_reply(addr: BdAddr, pin: &[u8]) -> [u8; 23] {
        let mut p = [0_u8; 23];
        p[..6].copy_from_slice(&addr.0);
        let n = pin.len().min(16);
        p[6] = n as u8;
        p[7..7 + n].copy_from_slice(&pin[..n]);
        p
    }

    /// The 248-byte name field, zero-padded.
    pub fn local_name(name: &str) -> [u8; 248] {
        let mut p = [0_u8; 248];
        let n = name.len().min(247);
        p[..n].copy_from_slice(&name.as_bytes()[..n]);
        p
    }

    pub fn class_of_device(cod: u32) -> [u8; 3] {
        let b = cod.to_le_bytes();
        [b[0], b[1], b[2]]
    }
}

/// Which events we want: Set Event Mask bits. All the classic events
/// (bits 0-34), the Secure Simple Pairing ones and the newer link events
/// (43-60), and LE meta (61).
pub const EVENT_MASK: [u8; 8] = 0x3DBF_F807_FFFF_FFFF_u64.to_le_bytes();

/// One device answer from an inquiry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Found<'a> {
    pub addr: BdAddr,
    /// Page scan repetition mode: needed to call it up quickly.
    pub psrm: u8,
    /// Class of device, 24 bits.
    pub cod: u32,
    pub clock_offset: u16,
    pub rssi: Option<i8>,
    /// Extended inquiry response data (name, services); empty if none.
    pub eir: &'a [u8],
}

/// The answers of one inquiry result event. The older event kinds put
/// several answers in one event, field by field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InquiryResults<'a> {
    /// 0x02, 0x22 or 0x2F.
    pub code: u8,
    pub n: usize,
    data: &'a [u8],
}

impl<'a> InquiryResults<'a> {
    pub fn get(&self, i: usize) -> Option<Found<'a>> {
        if i >= self.n {
            return None;
        }
        let e = self.data;
        // Several answers come one after the other, each whole (the way
        // BlueZ and BTstack read these events; with one answer, which is
        // what controllers send in practice, every reading agrees).
        let base = 1 + 14 * i;
        match self.code {
            // Inquiry Result: addr, psrm, 2 reserved, cod, clock offset.
            0x02 => Some(Found {
                addr: BdAddr::at(e, base)?,
                psrm: *e.get(base + 6)?,
                cod: u24_at(e, base + 9)?,
                clock_offset: u16_at(e, base + 12)?,
                rssi: None,
                eir: &[],
            }),
            // With RSSI: addr, psrm, 1 reserved, cod, clock offset, rssi.
            0x22 => Some(Found {
                addr: BdAddr::at(e, base)?,
                psrm: *e.get(base + 6)?,
                cod: u24_at(e, base + 8)?,
                clock_offset: u16_at(e, base + 11)?,
                rssi: Some(*e.get(base + 13)? as i8),
                eir: &[],
            }),
            // Extended: always one answer, then 240 bytes of EIR data.
            0x2F => Some(Found {
                addr: BdAddr::at(e, 1)?,
                psrm: *e.get(7)?,
                cod: u24_at(e, 9)?,
                clock_offset: u16_at(e, 12)?,
                rssi: Some(*e.get(14)? as i8),
                eir: e.get(15..).unwrap_or(&[]),
            }),
            _ => None,
        }
    }
}

/// The events we read. Unknown ones are kept raw.
#[derive(Debug, PartialEq)]
pub enum Event<'a> {
    InquiryComplete {
        status: u8,
    },
    InquiryResult(InquiryResults<'a>),
    ConnectionComplete {
        status: u8,
        handle: u16,
        addr: BdAddr,
        link_type: u8,
        encryption: u8,
    },
    ConnectionRequest {
        addr: BdAddr,
        cod: u32,
        link_type: u8,
    },
    DisconnectionComplete {
        status: u8,
        handle: u16,
        reason: u8,
    },
    AuthenticationComplete {
        status: u8,
        handle: u16,
    },
    RemoteNameComplete {
        status: u8,
        addr: BdAddr,
        name: &'a [u8],
    },
    EncryptionChange {
        status: u8,
        handle: u16,
        enabled: u8,
    },
    RemoteFeatures {
        status: u8,
        handle: u16,
        features: [u8; 8],
    },
    RemoteVersion {
        status: u8,
        handle: u16,
        version: u8,
        manufacturer: u16,
        subversion: u16,
    },
    /// `ret` = the return parameters (usually starting with a status).
    CommandComplete {
        credits: u8,
        opcode: u16,
        ret: &'a [u8],
    },
    CommandStatus {
        status: u8,
        credits: u8,
        opcode: u16,
    },
    HardwareError {
        code: u8,
    },
    RoleChange {
        status: u8,
        addr: BdAddr,
        role: u8,
    },
    /// Pairs of (handle, packets done), one after the other; see
    /// [`Event::completed`].
    NumberOfCompletedPackets {
        n: usize,
        data: &'a [u8],
    },
    ModeChange {
        status: u8,
        handle: u16,
        mode: u8,
        interval: u16,
    },
    PinCodeRequest {
        addr: BdAddr,
    },
    LinkKeyRequest {
        addr: BdAddr,
    },
    LinkKeyNotification {
        addr: BdAddr,
        key: LinkKey,
        key_type: u8,
    },
    DataBufferOverflow {
        link_type: u8,
    },
    MaxSlotsChange {
        handle: u16,
        max_slots: u8,
    },
    PageScanRepetitionModeChange {
        addr: BdAddr,
        psrm: u8,
    },
    RemoteExtFeatures {
        status: u8,
        handle: u16,
        page: u8,
        max_page: u8,
        features: [u8; 8],
    },
    EncryptionKeyRefreshComplete {
        status: u8,
        handle: u16,
    },
    IoCapabilityRequest {
        addr: BdAddr,
    },
    IoCapabilityResponse {
        addr: BdAddr,
        io_cap: u8,
        oob: u8,
        auth_req: u8,
    },
    UserConfirmationRequest {
        addr: BdAddr,
        value: u32,
    },
    UserPasskeyRequest {
        addr: BdAddr,
    },
    SimplePairingComplete {
        status: u8,
        addr: BdAddr,
    },
    LinkSupervisionTimeoutChanged {
        handle: u16,
        timeout: u16,
    },
    RemoteHostFeatures {
        addr: BdAddr,
        features: [u8; 8],
    },
    LeMeta {
        subevent: u8,
    },
    Unknown {
        code: u8,
        params: &'a [u8],
    },
}

impl<'a> Event<'a> {
    /// The (handle, count) pairs of a Number Of Completed Packets event.
    pub fn completed(n: usize, data: &'a [u8]) -> impl Iterator<Item = (u16, u16)> + 'a {
        (0..n).filter_map(move |i| {
            Some((u16_at(data, 1 + 4 * i)? & 0x0FFF, u16_at(data, 3 + 4 * i)?))
        })
    }
}

/// Reads one event: `p` starts with the event code (no type byte).
/// Returns `None` only when the packet is shorter than its own header
/// says; a known event that is too short becomes `Unknown`.
pub fn parse_event(p: &[u8]) -> Option<Event<'_>> {
    let code = *p.first()?;
    let len = usize::from(*p.get(1)?);
    let e = p.get(2..2 + len)?;
    Some(parse_params(code, e).unwrap_or(Event::Unknown { code, params: e }))
}

fn features(e: &[u8], at: usize) -> Option<[u8; 8]> {
    let mut f = [0; 8];
    f.copy_from_slice(e.get(at..at + 8)?);
    Some(f)
}

fn parse_params(code: u8, e: &[u8]) -> Option<Event<'_>> {
    let b = |i: usize| e.get(i).copied();
    let h = |i: usize| u16_at(e, i).map(|v| v & 0x0FFF);
    Some(match code {
        0x01 => Event::InquiryComplete { status: b(0)? },
        0x02 | 0x22 | 0x2F => {
            let n = usize::from(b(0)?);
            let per = match code {
                0x02 | 0x22 => 14,
                _ => 254,
            };
            if code == 0x2F && e.len() < 15 {
                return None;
            }
            if code != 0x2F && e.len() < 1 + per * n {
                return None;
            }
            Event::InquiryResult(InquiryResults {
                code,
                n: if code == 0x2F { 1 } else { n },
                data: e,
            })
        }
        0x03 => Event::ConnectionComplete {
            status: b(0)?,
            handle: h(1)?,
            addr: BdAddr::at(e, 3)?,
            link_type: b(9)?,
            encryption: b(10)?,
        },
        0x04 => Event::ConnectionRequest {
            addr: BdAddr::at(e, 0)?,
            cod: u24_at(e, 6)?,
            link_type: b(9)?,
        },
        0x05 => Event::DisconnectionComplete {
            status: b(0)?,
            handle: h(1)?,
            reason: b(3)?,
        },
        0x06 => Event::AuthenticationComplete {
            status: b(0)?,
            handle: h(1)?,
        },
        0x07 => {
            let name = e.get(7..).unwrap_or(&[]);
            let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            Event::RemoteNameComplete {
                status: b(0)?,
                addr: BdAddr::at(e, 1)?,
                name: &name[..end],
            }
        }
        0x08 => Event::EncryptionChange {
            status: b(0)?,
            handle: h(1)?,
            enabled: b(3)?,
        },
        0x0B => Event::RemoteFeatures {
            status: b(0)?,
            handle: h(1)?,
            features: features(e, 3)?,
        },
        0x0C => Event::RemoteVersion {
            status: b(0)?,
            handle: h(1)?,
            version: b(3)?,
            manufacturer: u16_at(e, 4)?,
            subversion: u16_at(e, 6)?,
        },
        0x0E => Event::CommandComplete {
            credits: b(0)?,
            opcode: u16_at(e, 1)?,
            ret: &e[3..],
        },
        0x0F => Event::CommandStatus {
            status: b(0)?,
            credits: b(1)?,
            opcode: u16_at(e, 2)?,
        },
        0x10 => Event::HardwareError { code: b(0)? },
        0x12 => Event::RoleChange {
            status: b(0)?,
            addr: BdAddr::at(e, 1)?,
            role: b(7)?,
        },
        0x13 => {
            let n = usize::from(b(0)?);
            if e.len() < 1 + 4 * n {
                return None;
            }
            Event::NumberOfCompletedPackets { n, data: e }
        }
        0x14 => Event::ModeChange {
            status: b(0)?,
            handle: h(1)?,
            mode: b(3)?,
            interval: u16_at(e, 4)?,
        },
        0x16 => Event::PinCodeRequest {
            addr: BdAddr::at(e, 0)?,
        },
        0x17 => Event::LinkKeyRequest {
            addr: BdAddr::at(e, 0)?,
        },
        0x18 => {
            let mut key = [0; 16];
            key.copy_from_slice(e.get(6..22)?);
            Event::LinkKeyNotification {
                addr: BdAddr::at(e, 0)?,
                key: LinkKey(key),
                key_type: b(22)?,
            }
        }
        0x1A => Event::DataBufferOverflow { link_type: b(0)? },
        0x1B => Event::MaxSlotsChange {
            handle: h(0)?,
            max_slots: b(2)?,
        },
        0x20 => Event::PageScanRepetitionModeChange {
            addr: BdAddr::at(e, 0)?,
            psrm: b(6)?,
        },
        0x23 => Event::RemoteExtFeatures {
            status: b(0)?,
            handle: h(1)?,
            page: b(3)?,
            max_page: b(4)?,
            features: features(e, 5)?,
        },
        0x30 => Event::EncryptionKeyRefreshComplete {
            status: b(0)?,
            handle: h(1)?,
        },
        0x31 => Event::IoCapabilityRequest {
            addr: BdAddr::at(e, 0)?,
        },
        0x32 => Event::IoCapabilityResponse {
            addr: BdAddr::at(e, 0)?,
            io_cap: b(6)?,
            oob: b(7)?,
            auth_req: b(8)?,
        },
        0x33 => Event::UserConfirmationRequest {
            addr: BdAddr::at(e, 0)?,
            value: u32_at(e, 6)?,
        },
        0x34 => Event::UserPasskeyRequest {
            addr: BdAddr::at(e, 0)?,
        },
        0x36 => Event::SimplePairingComplete {
            status: b(0)?,
            addr: BdAddr::at(e, 1)?,
        },
        0x38 => Event::LinkSupervisionTimeoutChanged {
            handle: h(0)?,
            timeout: u16_at(e, 2)?,
        },
        0x3D => Event::RemoteHostFeatures {
            addr: BdAddr::at(e, 0)?,
            features: features(e, 6)?,
        },
        0x3E => Event::LeMeta { subevent: b(0)? },
        _ => return None,
    })
}

/// Class of device: major class (bits 8-12).
pub fn cod_major(cod: u32) -> u8 {
    ((cod >> 8) & 0x1F) as u8
}

/// Class of device: minor class (bits 2-7).
pub fn cod_minor(cod: u32) -> u8 {
    ((cod >> 2) & 0x3F) as u8
}

/// Major class 0x05 is "peripheral" (keyboards, mice, game controllers).
pub fn is_peripheral(cod: u32) -> bool {
    cod_major(cod) == 0x05
}

/// A peripheral whose minor class says joystick (0x01) or gamepad (0x02)
/// in its low four bits. The GuliKit pad says 0x000508 or 0x002508. A
/// peripheral named "Pro Controller" (the Switch mode) counts as well.
pub fn is_gamepad(cod: u32, name: &[u8]) -> bool {
    is_peripheral(cod)
        && (matches!(cod_minor(cod) & 0x0F, 0x01 | 0x02) || name == b"Pro Controller")
}

pub fn major_text(major: u8) -> &'static str {
    match major {
        0x00 => "misc",
        0x01 => "computer",
        0x02 => "phone",
        0x03 => "network",
        0x04 => "audio-video",
        0x05 => "peripheral",
        0x06 => "imaging",
        0x07 => "wearable",
        0x08 => "toy",
        0x09 => "health",
        0x1F => "uncategorized",
        _ => "reserved",
    }
}

/// What an extended inquiry answer says, of the parts we look at.
#[derive(Default, Debug, PartialEq)]
pub struct EirInfo<'a> {
    pub name: &'a [u8],
    /// It lists the classic HID service (UUID 0x1124).
    pub hid: bool,
    /// Device ID: (source, vendor, product, version).
    pub device_id: Option<(u16, u16, u16, u16)>,
}

/// The data is a row of pieces: length, type, then (length - 1) bytes.
/// A length of zero ends it.
pub fn parse_eir(mut d: &[u8]) -> EirInfo<'_> {
    let mut out = EirInfo::default();
    while d.len() >= 2 {
        let len = usize::from(d[0]);
        if len == 0 || len + 1 > d.len() {
            break;
        }
        let (kind, body) = (d[1], &d[2..=len]);
        match kind {
            // Short or complete name; a complete name wins.
            0x08 if out.name.is_empty() => out.name = body,
            0x09 => out.name = body,
            0x02 | 0x03 => {
                out.hid |= body
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .any(|u| u16::from_le_bytes(*u) == 0x1124)
            }
            0x10 if body.len() >= 8 => {
                out.device_id = Some((
                    u16::from_le_bytes([body[0], body[1]]),
                    u16::from_le_bytes([body[2], body[3]]),
                    u16::from_le_bytes([body[4], body[5]]),
                    u16::from_le_bytes([body[6], body[7]]),
                ));
            }
            _ => {}
        }
        d = &d[len + 1..];
    }
    out
}

/// HCI error codes, in words, for the log.
pub fn status_text(status: u8) -> &'static str {
    match status {
        0x00 => "ok",
        0x01 => "unknown-command",
        0x02 => "unknown-connection",
        0x03 => "hardware-failure",
        0x04 => "page-timeout",
        0x05 => "authentication-failure",
        0x06 => "pin-or-key-missing",
        0x07 => "memory-full",
        0x08 => "connection-timeout",
        0x09 => "connection-limit",
        0x0B => "connection-exists",
        0x0C => "command-disallowed",
        0x0D => "rejected-limited-resources",
        0x0E => "rejected-security",
        0x0F => "rejected-unacceptable-address",
        0x10 => "accept-timeout",
        0x11 => "unsupported",
        0x12 => "invalid-parameters",
        0x13 => "remote-user-ended",
        0x14 => "remote-low-resources",
        0x15 => "remote-power-off",
        0x16 => "local-host-ended",
        0x17 => "repeated-attempts",
        0x18 => "pairing-not-allowed",
        0x1A => "unsupported-remote-feature",
        0x1F => "unspecified",
        0x22 => "lmp-response-timeout",
        0x23 => "lmp-transaction-collision",
        0x25 => "encryption-mode-not-acceptable",
        0x28 => "instant-passed",
        0x29 => "pairing-with-unit-key-unsupported",
        0x2A => "different-transaction-collision",
        0x2F => "insufficient-security",
        0x3D => "connection-terminated-mic-failure",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_parse_and_print() {
        let a = BdAddr::parse("01:23:45:67:89:AB").unwrap();
        assert_eq!(a.0, [0xAB, 0x89, 0x67, 0x45, 0x23, 0x01]);
        // Printed, only the last two bytes show (privacy).
        assert_eq!(format!("{a}"), "xx:xx:xx:xx:89:AB");
        assert_eq!(format!("{a:?}"), "BdAddr(xx:xx:xx:xx:89:AB)");
        let some = Some(a);
        assert!(!format!("{some:?}").contains("45"));
        assert!(BdAddr::parse("01:23:45").is_none());
        assert!(BdAddr::parse("01:23:45:67:89:AB:CD").is_none());
    }

    #[test]
    fn command_layout() {
        let p = command(op::WRITE_SCAN_ENABLE, &[0x02]);
        assert_eq!(p.as_bytes(), &[0x01, 0x1A, 0x0C, 0x01, 0x02]);
    }

    #[test]
    fn link_key_debug_is_hidden() {
        let k = LinkKey([0x42; 16]);
        assert_eq!(format!("{k:?}"), "LinkKey(hidden)");
    }

    #[test]
    fn extended_inquiry_result() {
        // The GuliKit pad's answer as measured: class 0x000508, name in EIR.
        let mut e = vec![0x2F, 255, 0x01];
        e.extend_from_slice(&[1, 2, 3, 4, 5, 6]); // addr
        e.extend_from_slice(&[0x01, 0x00]); // psrm, reserved
        e.extend_from_slice(&[0x08, 0x05, 0x00]); // cod
        e.extend_from_slice(&[0x34, 0x12]); // clock offset
        e.push((-48_i8) as u8);
        let name = b"GuliKit Controller XW";
        let mut eir = vec![name.len() as u8 + 1, 0x09];
        eir.extend_from_slice(name);
        eir.resize(240, 0);
        e.extend_from_slice(&eir);
        let ev = parse_event(&e).unwrap();
        let Event::InquiryResult(r) = ev else {
            panic!("{ev:?}")
        };
        let f = r.get(0).unwrap();
        assert_eq!(f.addr.0, [1, 2, 3, 4, 5, 6]);
        assert_eq!(f.cod, 0x000508);
        assert_eq!(f.clock_offset, 0x1234);
        assert_eq!(f.rssi, Some(-48));
        let info = parse_eir(f.eir);
        assert_eq!(info.name, name);
        assert!(!info.hid);
        assert!(is_gamepad(f.cod, info.name));
        assert!(r.get(1).is_none());
    }

    #[test]
    fn inquiry_result_with_rssi_two_answers() {
        let mut e = vec![0x22, 0, 2];
        // addr, psrm, reserved, cod, clock, rssi: a phone, then a gamepad.
        e.extend_from_slice(&[1, 1, 1, 1, 1, 1, 1, 0, 0x0C, 0x02, 0x5A, 0x11, 0x00, 0xC0]);
        e.extend_from_slice(&[2, 2, 2, 2, 2, 2, 2, 0, 0x08, 0x25, 0x00, 0x22, 0x00, 0xD0]);
        e[1] = (e.len() - 2) as u8;
        let Some(Event::InquiryResult(r)) = parse_event(&e) else {
            panic!()
        };
        let a = r.get(0).unwrap();
        let b = r.get(1).unwrap();
        assert_eq!(a.cod, 0x5A020C);
        assert!(!is_gamepad(a.cod, b""));
        assert_eq!(b.addr.0, [2; 6]);
        assert_eq!(b.cod, 0x002508);
        assert!(is_gamepad(b.cod, b""));
        assert_eq!(b.psrm, 2);
        assert_eq!(b.clock_offset, 0x22);
        assert_eq!(b.rssi, Some(0xD0_u8 as i8));
    }

    #[test]
    fn gamepad_class_rules() {
        assert!(is_gamepad(0x000508, b""));
        assert!(is_gamepad(0x000504, b"")); // joystick
        assert!(!is_gamepad(0x000540, b"")); // keyboard
        assert!(is_gamepad(0x000500, b"Pro Controller"));
        assert!(!is_gamepad(0x5A020C, b"Pro Controller")); // a phone with that name
    }

    #[test]
    fn command_complete_and_status() {
        let e = [0x0E, 0x04, 0x05, 0x03, 0x0C, 0x00];
        assert_eq!(
            parse_event(&e),
            Some(Event::CommandComplete {
                credits: 5,
                opcode: op::RESET,
                ret: &[0]
            })
        );
        let e = [0x0F, 0x04, 0x00, 0x01, 0x05, 0x04];
        assert_eq!(
            parse_event(&e),
            Some(Event::CommandStatus {
                status: 0,
                credits: 1,
                opcode: op::CREATE_CONNECTION
            })
        );
    }

    #[test]
    fn completed_packets_pairs() {
        let e = [
            0x13, 0x09, 0x02, 0x0B, 0x00, 0x02, 0x00, 0x0C, 0x00, 0x01, 0x00,
        ];
        let Some(Event::NumberOfCompletedPackets { n, data }) = parse_event(&e) else {
            panic!()
        };
        let v: Vec<_> = Event::completed(n, data).collect();
        assert_eq!(v, vec![(0x0B, 2), (0x0C, 1)]);
    }

    #[test]
    fn short_event_is_unknown_not_a_panic() {
        let e = [0x03, 0x02, 0x00, 0x01];
        assert!(matches!(
            parse_event(&e),
            Some(Event::Unknown { code: 0x03, .. })
        ));
        assert_eq!(parse_event(&[0x03, 0x05, 0x00]), None);
    }

    #[test]
    fn link_key_notification() {
        let mut e = vec![0x18, 23];
        e.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        e.extend_from_slice(&[0x77; 16]);
        e.push(0x04);
        let Some(Event::LinkKeyNotification {
            addr,
            key,
            key_type,
        }) = parse_event(&e)
        else {
            panic!()
        };
        assert_eq!(addr.0, [1, 2, 3, 4, 5, 6]);
        assert_eq!(key.0, [0x77; 16]);
        assert_eq!(key_type, 4);
    }

    #[test]
    fn event_mask_bits() {
        let m = u64::from_le_bytes(EVENT_MASK);
        for bit in [0, 1, 2, 3, 4, 5, 6, 7, 21, 22, 23, 33, 46, 48, 49, 50, 53] {
            assert!(m & (1 << bit) != 0, "bit {bit}");
        }
    }
}
