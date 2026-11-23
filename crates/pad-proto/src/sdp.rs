//! SDP, the Service Discovery Protocol: asking a device what it offers.
//!
//! A device keeps a list of "service records". Each record is a list of
//! numbered attributes. The HID record (service class 0x1124) holds the
//! gamepad's HID report descriptor (the description of its input
//! reports) and a few HID settings; the Device ID record (0x1200) holds
//! the vendor and product numbers.
//!
//! SDP runs on an L2CAP channel with PSM 0x0001. Unlike the rest of
//! Bluetooth, every number in SDP is big-endian (most significant byte
//! first). Values are "data elements": one header byte (type in the top 5
//! bits, size code in the low 3), maybe a length, then the value.

use core::fmt;

use crate::buf::Printable;

pub mod pdu {
    pub const ERROR_RESPONSE: u8 = 0x01;
    pub const SERVICE_SEARCH_REQUEST: u8 = 0x02;
    pub const SERVICE_SEARCH_RESPONSE: u8 = 0x03;
    pub const SERVICE_ATTRIBUTE_REQUEST: u8 = 0x04;
    pub const SERVICE_ATTRIBUTE_RESPONSE: u8 = 0x05;
    pub const SERVICE_SEARCH_ATTRIBUTE_REQUEST: u8 = 0x06;
    pub const SERVICE_SEARCH_ATTRIBUTE_RESPONSE: u8 = 0x07;
}

pub const UUID_L2CAP: u16 = 0x0100;
pub const UUID_HID: u16 = 0x1124;
pub const UUID_PNP: u16 = 0x1200;

/// The attribute numbers we read.
pub mod attr {
    pub const SERVICE_RECORD_HANDLE: u16 = 0x0000;
    pub const SERVICE_CLASS_ID_LIST: u16 = 0x0001;
    pub const SERVICE_NAME: u16 = 0x0100;
    pub const SERVICE_DESCRIPTION: u16 = 0x0101;
    pub const PROVIDER_NAME: u16 = 0x0102;
    // HID record.
    pub const HID_PARSER_VERSION: u16 = 0x0201;
    pub const HID_DEVICE_SUBCLASS: u16 = 0x0202;
    pub const HID_COUNTRY_CODE: u16 = 0x0203;
    pub const HID_VIRTUAL_CABLE: u16 = 0x0204;
    pub const HID_RECONNECT_INITIATE: u16 = 0x0205;
    pub const HID_DESCRIPTOR_LIST: u16 = 0x0206;
    pub const HID_LANGID_BASE_LIST: u16 = 0x0207;
    pub const HID_SDP_DISABLE: u16 = 0x0208;
    pub const HID_BATTERY_POWER: u16 = 0x0209;
    pub const HID_REMOTE_WAKE: u16 = 0x020A;
    pub const HID_PROFILE_VERSION: u16 = 0x020B;
    pub const HID_SUPERVISION_TIMEOUT: u16 = 0x020C;
    pub const HID_NORMALLY_CONNECTABLE: u16 = 0x020D;
    pub const HID_BOOT_DEVICE: u16 = 0x020E;
    pub const HID_SSR_HOST_MAX_LATENCY: u16 = 0x020F;
    pub const HID_SSR_HOST_MIN_TIMEOUT: u16 = 0x0210;
    // Device ID record (same numbers, other meaning).
    pub const PNP_SPECIFICATION_ID: u16 = 0x0200;
    pub const PNP_VENDOR_ID: u16 = 0x0201;
    pub const PNP_PRODUCT_ID: u16 = 0x0202;
    pub const PNP_VERSION: u16 = 0x0203;
    pub const PNP_PRIMARY_RECORD: u16 = 0x0204;
    pub const PNP_VENDOR_ID_SOURCE: u16 = 0x0205;
}

/// The SDP error codes, in words.
pub fn error_text(code: u16) -> &'static str {
    match code {
        0x0001 => "unsupported-version",
        0x0002 => "invalid-record-handle",
        0x0003 => "invalid-syntax",
        0x0004 => "invalid-pdu-size",
        0x0005 => "invalid-continuation-state",
        0x0006 => "insufficient-resources",
        _ => "other",
    }
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

/// Writes a Service Search Attribute Request for one 16-bit service
/// class UUID, asking for the attribute ranges given. Returns the PDU
/// length in `out` (0 if it does not fit).
pub fn search_attr_request(
    out: &mut [u8],
    tid: u16,
    uuid: u16,
    max_bytes: u16,
    ranges: &[(u16, u16)],
    cont: &[u8],
) -> usize {
    let mut p = [0_u8; 96];
    let mut n = 0;
    let mut put = |b: &[u8]| {
        if n + b.len() <= p.len() {
            p[n..n + b.len()].copy_from_slice(b);
        }
        n += b.len();
    };
    // Header: PDU id, transaction id, parameter length (filled in below).
    put(&[pdu::SERVICE_SEARCH_ATTRIBUTE_REQUEST]);
    put(&tid.to_be_bytes());
    put(&[0, 0]);
    // ServiceSearchPattern: a sequence (0x35, 1-byte length) of one UUID16 (0x19).
    put(&[0x35, 3, 0x19]);
    put(&uuid.to_be_bytes());
    put(&max_bytes.to_be_bytes());
    // AttributeIDList: a sequence of 32-bit ranges (0x0A, start, end).
    put(&[0x35, (ranges.len() * 5) as u8]);
    for &(a, b) in ranges {
        put(&[0x0A]);
        put(&a.to_be_bytes());
        put(&b.to_be_bytes());
    }
    put(&[cont.len() as u8]);
    put(cont);
    if n > p.len() || n > out.len() {
        return 0;
    }
    let plen = (n - 5) as u16;
    p[3..5].copy_from_slice(&plen.to_be_bytes());
    out[..n].copy_from_slice(&p[..n]);
    n
}

/// An SDP PDU as read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pdu<'a> {
    Error {
        tid: u16,
        code: u16,
    },
    SearchAttrResponse {
        tid: u16,
        lists: &'a [u8],
        cont: &'a [u8],
    },
    /// A request from the other side (they ask us).
    Request {
        id: u8,
        tid: u16,
        params: &'a [u8],
    },
    Other {
        id: u8,
        tid: u16,
    },
}

pub fn parse_pdu(b: &[u8]) -> Option<Pdu<'_>> {
    let id = *b.first()?;
    let tid = be16(b, 1)?;
    let plen = usize::from(be16(b, 3)?);
    let params = b.get(5..5 + plen)?;
    Some(match id {
        pdu::ERROR_RESPONSE => Pdu::Error {
            tid,
            code: be16(params, 0)?,
        },
        pdu::SERVICE_SEARCH_ATTRIBUTE_RESPONSE => {
            let count = usize::from(be16(params, 0)?);
            let lists = params.get(2..2 + count)?;
            let clen = usize::from(*params.get(2 + count)?);
            let cont = params.get(3 + count..3 + count + clen)?;
            Pdu::SearchAttrResponse { tid, lists, cont }
        }
        pdu::SERVICE_SEARCH_REQUEST
        | pdu::SERVICE_ATTRIBUTE_REQUEST
        | pdu::SERVICE_SEARCH_ATTRIBUTE_REQUEST => Pdu::Request { id, tid, params },
        _ => Pdu::Other { id, tid },
    })
}

/// Our answer as a tiny SDP server: we offer no services. A gamepad may
/// ask the console what it offers; an honest empty answer keeps it from
/// waiting. Returns the PDU length written to `out`.
pub fn server_answer(out: &mut [u8], id: u8, tid: u16) -> usize {
    let (rid, params): (u8, &[u8]) = match id {
        // No records: total 0, current 0, no continuation.
        pdu::SERVICE_SEARCH_REQUEST => (pdu::SERVICE_SEARCH_RESPONSE, &[0, 0, 0, 0, 0]),
        // We have no record handles at all.
        pdu::SERVICE_ATTRIBUTE_REQUEST => (pdu::ERROR_RESPONSE, &[0x00, 0x02]),
        // An empty list of records (an empty sequence), no continuation.
        pdu::SERVICE_SEARCH_ATTRIBUTE_REQUEST => (
            pdu::SERVICE_SEARCH_ATTRIBUTE_RESPONSE,
            &[0x00, 0x02, 0x35, 0x00, 0x00],
        ),
        _ => (pdu::ERROR_RESPONSE, &[0x00, 0x03]),
    };
    let n = 5 + params.len();
    if out.len() < n {
        return 0;
    }
    out[0] = rid;
    out[1..3].copy_from_slice(&tid.to_be_bytes());
    out[3..5].copy_from_slice(&(params.len() as u16).to_be_bytes());
    out[5..n].copy_from_slice(params);
    n
}

/// One data element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Element<'a> {
    Nil,
    /// Unsigned integer, big-endian bytes (1, 2, 4, 8 or 16).
    Uint(&'a [u8]),
    Sint(&'a [u8]),
    /// UUID: 2, 4 or 16 bytes.
    Uuid(&'a [u8]),
    Text(&'a [u8]),
    Bool(bool),
    /// Sequence: the body, itself a row of elements.
    Seq(&'a [u8]),
    /// Alternative: also a row of elements.
    Alt(&'a [u8]),
    Url(&'a [u8]),
    Unknown(u8, &'a [u8]),
}

/// Reads one data element from the start of `b`. Returns it and the bytes
/// it took.
pub fn element(b: &[u8]) -> Option<(Element<'_>, usize)> {
    let h = *b.first()?;
    let kind = h >> 3;
    let size = h & 7;
    let (len, at): (usize, usize) = match size {
        0 if kind == 0 => (0, 1),
        0 => (1, 1),
        1 => (2, 1),
        2 => (4, 1),
        3 => (8, 1),
        4 => (16, 1),
        5 => (usize::from(*b.get(1)?), 2),
        6 => (usize::from(be16(b, 1)?), 3),
        _ => (
            u32::from_be_bytes([*b.get(1)?, *b.get(2)?, *b.get(3)?, *b.get(4)?]) as usize,
            5,
        ),
    };
    let body = b.get(at..at.checked_add(len)?)?;
    let e = match kind {
        0 => Element::Nil,
        1 => Element::Uint(body),
        2 => Element::Sint(body),
        3 => Element::Uuid(body),
        4 => Element::Text(body),
        5 => Element::Bool(body.first().is_some_and(|&v| v != 0)),
        6 => Element::Seq(body),
        7 => Element::Alt(body),
        8 => Element::Url(body),
        _ => Element::Unknown(kind, body),
    };
    Some((e, at + len))
}

/// The elements in a sequence's body, one after the other. Stops at the
/// first one that cannot be read.
pub fn items(mut body: &[u8]) -> impl Iterator<Item = Element<'_>> {
    core::iter::from_fn(move || {
        let (e, n) = element(body)?;
        body = &body[n..];
        Some(e)
    })
}

impl<'a> Element<'a> {
    /// An unsigned value of up to 4 bytes.
    pub fn uint(&self) -> Option<u32> {
        match self {
            Element::Uint(b) if b.len() <= 4 => {
                Some(b.iter().fold(0_u32, |a, &x| a << 8 | u32::from(x)))
            }
            _ => None,
        }
    }

    /// A UUID that fits in 16 bits (also from the 32 and 128-bit forms
    /// built on the Bluetooth base UUID).
    pub fn uuid16(&self) -> Option<u16> {
        const BASE_TAIL: [u8; 12] = [
            0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0x80, 0x5F, 0x9B, 0x34, 0xFB,
        ];
        match self {
            Element::Uuid(b) if b.len() == 2 => Some(u16::from_be_bytes([b[0], b[1]])),
            Element::Uuid(b) if b.len() == 4 && b[0] == 0 && b[1] == 0 => {
                Some(u16::from_be_bytes([b[2], b[3]]))
            }
            Element::Uuid(b) if b.len() == 16 && b[0] == 0 && b[1] == 0 && b[4..] == BASE_TAIL => {
                Some(u16::from_be_bytes([b[2], b[3]]))
            }
            _ => None,
        }
    }

    pub fn boolean(&self) -> Option<bool> {
        match self {
            Element::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn seq(&self) -> Option<&'a [u8]> {
        match self {
            Element::Seq(b) | Element::Alt(b) => Some(b),
            _ => None,
        }
    }

    pub fn bytes(&self) -> Option<&'a [u8]> {
        match self {
            Element::Text(b) | Element::Url(b) => Some(b),
            _ => None,
        }
    }
}

/// A short description of an element for the log.
impl fmt::Display for Element<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Element::Nil => write!(f, "nil"),
            Element::Uint(b) | Element::Sint(b) | Element::Uuid(b) => {
                let tag = match self {
                    Element::Uint(_) => "uint",
                    Element::Sint(_) => "sint",
                    _ => "uuid",
                };
                write!(f, "{tag}:0x")?;
                for x in *b {
                    write!(f, "{x:02x}")?;
                }
                Ok(())
            }
            Element::Text(b) | Element::Url(b) => {
                let shown = &b[..b.len().min(48)];
                write!(
                    f,
                    "text:\"{}\"{}",
                    Printable(shown),
                    if b.len() > 48 { "..." } else { "" }
                )
            }
            Element::Bool(v) => write!(f, "bool:{}", u8::from(*v)),
            Element::Seq(b) | Element::Alt(b) => {
                write!(f, "seq[")?;
                let mut first = true;
                for (i, e) in items(b).enumerate() {
                    if i == 6 {
                        write!(f, " ...")?;
                        break;
                    }
                    if !first {
                        write!(f, " ")?;
                    }
                    first = false;
                    match e {
                        Element::Seq(_) | Element::Alt(_) => {
                            write!(f, "seq({}B)", e.seq().map_or(0, <[u8]>::len))?
                        }
                        Element::Text(t) if t.len() > 16 => write!(f, "text({}B)", t.len())?,
                        other => write!(f, "{other}")?,
                    }
                }
                write!(f, "]")
            }
            Element::Unknown(k, b) => write!(f, "type{}({}B)", k, b.len()),
        }
    }
}

/// The records in a Service Search Attribute answer (after all its
/// pieces were joined): one outer sequence, holding one sequence per
/// record. Gives each record's body.
pub fn records(lists: &[u8]) -> impl Iterator<Item = &[u8]> {
    let outer = element(lists).and_then(|(e, _)| e.seq()).unwrap_or(&[]);
    items(outer).filter_map(|e| e.seq())
}

/// The (attribute id, value) pairs of one record body.
pub fn attributes(record: &[u8]) -> impl Iterator<Item = (u16, Element<'_>)> {
    let mut it = items(record);
    core::iter::from_fn(move || {
        let id = it.next()?.uint()? as u16;
        let v = it.next()?;
        Some((id, v))
    })
}

/// The service classes a record says it is (attribute 0x0001).
pub fn has_class(record: &[u8], uuid: u16) -> bool {
    attributes(record)
        .find(|(id, _)| *id == attr::SERVICE_CLASS_ID_LIST)
        .and_then(|(_, v)| v.seq())
        .is_some_and(|list| items(list).any(|u| u.uuid16() == Some(uuid)))
}

/// The HID attributes worth showing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HidInfo {
    pub parser_version: Option<u16>,
    pub device_subclass: Option<u8>,
    pub country_code: Option<u8>,
    pub virtual_cable: Option<bool>,
    pub reconnect_initiate: Option<bool>,
    pub sdp_disable: Option<bool>,
    pub battery_power: Option<bool>,
    pub remote_wake: Option<bool>,
    pub profile_version: Option<u16>,
    pub supervision_timeout: Option<u16>,
    pub normally_connectable: Option<bool>,
    pub boot_device: Option<bool>,
    pub ssr_host_max_latency: Option<u16>,
    pub ssr_host_min_timeout: Option<u16>,
    /// Descriptor type of the first descriptor (0x22 = report descriptor).
    pub descriptor_type: Option<u8>,
}

/// Reads a HID record: its settings and its report descriptor (the
/// first descriptor of type 0x22 in the descriptor list).
pub fn read_hid(record: &[u8]) -> (HidInfo, Option<&[u8]>) {
    let mut h = HidInfo::default();
    let mut desc = None;
    for (id, v) in attributes(record) {
        let u16v = v.uint().map(|x| x as u16);
        match id {
            attr::HID_PARSER_VERSION => h.parser_version = u16v,
            attr::HID_DEVICE_SUBCLASS => h.device_subclass = v.uint().map(|x| x as u8),
            attr::HID_COUNTRY_CODE => h.country_code = v.uint().map(|x| x as u8),
            attr::HID_VIRTUAL_CABLE => h.virtual_cable = v.boolean(),
            attr::HID_RECONNECT_INITIATE => h.reconnect_initiate = v.boolean(),
            attr::HID_DESCRIPTOR_LIST => {
                // A sequence of (type, bytes) sequences.
                for d in items(v.seq().unwrap_or(&[])) {
                    let mut parts = items(d.seq().unwrap_or(&[]));
                    let kind = parts.next().and_then(|e| e.uint());
                    let bytes = parts.next().and_then(|e| e.bytes());
                    if h.descriptor_type.is_none() {
                        h.descriptor_type = kind.map(|k| k as u8);
                    }
                    if kind == Some(0x22) && desc.is_none() {
                        desc = bytes;
                    }
                }
            }
            attr::HID_SDP_DISABLE => h.sdp_disable = v.boolean(),
            attr::HID_BATTERY_POWER => h.battery_power = v.boolean(),
            attr::HID_REMOTE_WAKE => h.remote_wake = v.boolean(),
            attr::HID_PROFILE_VERSION => h.profile_version = u16v,
            attr::HID_SUPERVISION_TIMEOUT => h.supervision_timeout = u16v,
            attr::HID_NORMALLY_CONNECTABLE => h.normally_connectable = v.boolean(),
            attr::HID_BOOT_DEVICE => h.boot_device = v.boolean(),
            attr::HID_SSR_HOST_MAX_LATENCY => h.ssr_host_max_latency = u16v,
            attr::HID_SSR_HOST_MIN_TIMEOUT => h.ssr_host_min_timeout = u16v,
            _ => {}
        }
    }
    (h, desc)
}

/// The Device ID record.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PnpInfo {
    pub specification_id: Option<u16>,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub version: Option<u16>,
    pub primary_record: Option<bool>,
    /// 1 = Bluetooth SIG number, 2 = USB number.
    pub vendor_id_source: Option<u16>,
}

pub fn read_pnp(record: &[u8]) -> PnpInfo {
    let mut p = PnpInfo::default();
    for (id, v) in attributes(record) {
        let u = v.uint().map(|x| x as u16);
        match id {
            attr::PNP_SPECIFICATION_ID => p.specification_id = u,
            attr::PNP_VENDOR_ID => p.vendor_id = u,
            attr::PNP_PRODUCT_ID => p.product_id = u,
            attr::PNP_VERSION => p.version = u,
            attr::PNP_PRIMARY_RECORD => p.primary_record = v.boolean(),
            attr::PNP_VENDOR_ID_SOURCE => p.vendor_id_source = u,
            _ => {}
        }
    }
    p
}

/// The record's service name (attribute 0x0100), if it has one.
pub fn service_name(record: &[u8]) -> Option<&[u8]> {
    attributes(record)
        .find(|(id, _)| *id == attr::SERVICE_NAME)
        .and_then(|(_, v)| v.bytes())
}

/// Builds data elements (big-endian) into a fixed buffer. For tests, and
/// for anyone who wants to fake a device's answer.
pub struct Builder<const N: usize> {
    pub buf: [u8; N],
    pub len: usize,
}

impl<const N: usize> Default for Builder<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Builder<N> {
    pub const fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    pub fn raw(&mut self, b: &[u8]) -> &mut Self {
        self.buf[self.len..self.len + b.len()].copy_from_slice(b);
        self.len += b.len();
        self
    }

    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.raw(&[0x08, v])
    }

    pub fn u16(&mut self, v: u16) -> &mut Self {
        let b = v.to_be_bytes();
        self.raw(&[0x09, b[0], b[1]])
    }

    pub fn uuid16(&mut self, v: u16) -> &mut Self {
        let b = v.to_be_bytes();
        self.raw(&[0x19, b[0], b[1]])
    }

    pub fn boolean(&mut self, v: bool) -> &mut Self {
        self.raw(&[0x28, u8::from(v)])
    }

    pub fn text(&mut self, t: &[u8]) -> &mut Self {
        if t.len() < 256 {
            self.raw(&[0x25, t.len() as u8]).raw(t)
        } else {
            let b = (t.len() as u16).to_be_bytes();
            self.raw(&[0x26, b[0], b[1]]).raw(t)
        }
    }

    /// Opens a sequence with a 2-byte length; close it with `end`.
    pub fn seq(&mut self) -> usize {
        let at = self.len;
        self.raw(&[0x36, 0, 0]);
        at
    }

    pub fn end(&mut self, at: usize) -> &mut Self {
        let body = (self.len - at - 3) as u16;
        self.buf[at + 1..at + 3].copy_from_slice(&body.to_be_bytes());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A HID record like a gamepad's, then a Device ID record.
    fn sample() -> Builder<512> {
        let mut b = Builder::<512>::new();
        let outer = b.seq();
        // HID record.
        let r = b.seq();
        b.u16(attr::SERVICE_RECORD_HANDLE).raw(&[0x0A, 0, 1, 0, 0]);
        b.u16(attr::SERVICE_CLASS_ID_LIST);
        let c = b.seq();
        b.uuid16(UUID_HID);
        b.end(c);
        b.u16(attr::SERVICE_NAME).text(b"Wireless Controller");
        b.u16(attr::HID_PARSER_VERSION).u16(0x0111);
        b.u16(attr::HID_DEVICE_SUBCLASS).u8(0x08);
        b.u16(attr::HID_COUNTRY_CODE).u8(0x21);
        b.u16(attr::HID_VIRTUAL_CABLE).boolean(true);
        b.u16(attr::HID_RECONNECT_INITIATE).boolean(true);
        b.u16(attr::HID_DESCRIPTOR_LIST);
        let l = b.seq();
        let d = b.seq();
        b.u8(0x22).text(&[0x05, 0x01, 0x09, 0x05, 0xA1, 0x01, 0xC0]);
        b.end(d);
        b.end(l);
        b.u16(attr::HID_NORMALLY_CONNECTABLE).boolean(false);
        b.u16(attr::HID_BOOT_DEVICE).boolean(false);
        b.end(r);
        // Device ID record.
        let r = b.seq();
        b.u16(attr::SERVICE_CLASS_ID_LIST);
        let c = b.seq();
        b.uuid16(UUID_PNP);
        b.end(c);
        b.u16(attr::PNP_SPECIFICATION_ID).u16(0x0103);
        b.u16(attr::PNP_VENDOR_ID).u16(0x045E);
        b.u16(attr::PNP_PRODUCT_ID).u16(0x02E0);
        b.u16(attr::PNP_VERSION).u16(0x0903);
        b.u16(attr::PNP_PRIMARY_RECORD).boolean(true);
        b.u16(attr::PNP_VENDOR_ID_SOURCE).u16(2);
        b.end(r);
        b.end(outer);
        b
    }

    #[test]
    fn request_layout() {
        let mut out = [0_u8; 64];
        let n = search_attr_request(&mut out, 1, UUID_HID, 600, &[(0x0000, 0xFFFF)], &[]);
        assert_eq!(
            &out[..n],
            &[
                0x06, 0x00, 0x01, 0x00, 0x0F, 0x35, 0x03, 0x19, 0x11, 0x24, 0x02, 0x58, 0x35, 0x05,
                0x0A, 0x00, 0x00, 0xFF, 0xFF, 0x00
            ]
        );
        let n = search_attr_request(&mut out, 2, UUID_PNP, 100, &[(0, 0xFFFF)], &[1, 2, 3]);
        assert_eq!(&out[n - 4..n], &[3, 1, 2, 3]);
        assert_eq!(u16::from_be_bytes([out[3], out[4]]) as usize, n - 5);
    }

    #[test]
    fn element_sizes() {
        assert_eq!(element(&[0x08, 0x05]), Some((Element::Uint(&[5]), 2)));
        assert_eq!(element(&[0x0A, 0, 0, 1, 0]).unwrap().0.uint(), Some(256));
        assert_eq!(
            element(&[0x25, 2, b'h', b'i']),
            Some((Element::Text(b"hi"), 4))
        );
        assert_eq!(element(&[0x35, 0]), Some((Element::Seq(&[]), 2)));
        assert_eq!(element(&[0x00]), Some((Element::Nil, 1)));
        assert_eq!(element(&[0x25, 5, b'h']), None);
        let u128 = [
            0x1C, 0, 0, 0x11, 0x24, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0x80, 0x5F, 0x9B,
            0x34, 0xFB,
        ];
        assert_eq!(element(&u128).unwrap().0.uuid16(), Some(0x1124));
    }

    #[test]
    fn reads_hid_and_pnp_records() {
        let b = sample();
        let recs: Vec<&[u8]> = records(b.bytes()).collect();
        assert_eq!(recs.len(), 2);
        assert!(has_class(recs[0], UUID_HID));
        assert!(!has_class(recs[0], UUID_PNP));
        let (h, desc) = read_hid(recs[0]);
        assert_eq!(h.parser_version, Some(0x0111));
        assert_eq!(h.device_subclass, Some(0x08));
        assert_eq!(h.virtual_cable, Some(true));
        assert_eq!(h.normally_connectable, Some(false));
        assert_eq!(h.descriptor_type, Some(0x22));
        assert_eq!(desc, Some(&[0x05, 0x01, 0x09, 0x05, 0xA1, 0x01, 0xC0][..]));
        assert_eq!(service_name(recs[0]), Some(&b"Wireless Controller"[..]));
        assert!(has_class(recs[1], UUID_PNP));
        let p = read_pnp(recs[1]);
        assert_eq!(p.vendor_id, Some(0x045E));
        assert_eq!(p.product_id, Some(0x02E0));
        assert_eq!(p.version, Some(0x0903));
        assert_eq!(p.vendor_id_source, Some(2));
        assert_eq!(p.primary_record, Some(true));
    }

    #[test]
    fn response_parse_and_server_answers() {
        let mut out = [0_u8; 32];
        let n = server_answer(&mut out, pdu::SERVICE_SEARCH_ATTRIBUTE_REQUEST, 7);
        assert_eq!(
            parse_pdu(&out[..n]),
            Some(Pdu::SearchAttrResponse {
                tid: 7,
                lists: &[0x35, 0x00],
                cont: &[]
            })
        );
        assert_eq!(records(&[0x35, 0x00]).count(), 0);
        let n = server_answer(&mut out, pdu::SERVICE_ATTRIBUTE_REQUEST, 8);
        assert_eq!(parse_pdu(&out[..n]), Some(Pdu::Error { tid: 8, code: 2 }));
        let n = server_answer(&mut out, pdu::SERVICE_SEARCH_REQUEST, 9);
        assert_eq!(&out[..n], &[0x03, 0, 9, 0, 5, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn display_is_short() {
        let b = sample();
        let rec = records(b.bytes()).next().unwrap();
        let shown: Vec<String> = attributes(rec)
            .map(|(id, v)| format!("{id:#06x}={v}"))
            .collect();
        assert_eq!(shown[1], "0x0001=seq[uuid:0x1124]");
        assert_eq!(shown[2], "0x0100=text:\"Wireless Controller\"");
        assert_eq!(shown[8], "0x0206=seq[seq(11B)]");
    }
}
