//! L2CAP in basic mode: channels over one ACL link.
//!
//! Each frame starts with a 4-byte header: payload length, channel id
//! (CID). Channel 0x0001 is the "signalling channel": two sides use it to
//! open, set up ("configure") and close the other channels. A channel is
//! opened for a "PSM", a number saying which service is wanted: 0x0001 SDP
//! (the service list), 0x0011 HID control, 0x0013 HID interrupt (where
//! the gamepad's input reports arrive).
//!
//! This module only reads and writes the packets. The rules of when to
//! send what live in `host`.

use crate::buf::u16_at;

pub const CID_SIGNALLING: u16 = 0x0001;
pub const CID_CONNECTIONLESS: u16 = 0x0002;
/// The first channel id we hand out for our own end of a channel.
pub const CID_DYNAMIC_FIRST: u16 = 0x0040;

pub const PSM_SDP: u16 = 0x0001;
pub const PSM_HID_CONTROL: u16 = 0x0011;
pub const PSM_HID_INTERRUPT: u16 = 0x0013;

/// The default and smallest MTU (largest payload) in basic mode for HID.
pub const MTU_DEFAULT: u16 = 672;
pub const MTU_MIN: u16 = 48;

pub mod code {
    pub const COMMAND_REJECT: u8 = 0x01;
    pub const CONNECTION_REQUEST: u8 = 0x02;
    pub const CONNECTION_RESPONSE: u8 = 0x03;
    pub const CONFIGURE_REQUEST: u8 = 0x04;
    pub const CONFIGURE_RESPONSE: u8 = 0x05;
    pub const DISCONNECTION_REQUEST: u8 = 0x06;
    pub const DISCONNECTION_RESPONSE: u8 = 0x07;
    pub const ECHO_REQUEST: u8 = 0x08;
    pub const ECHO_RESPONSE: u8 = 0x09;
    pub const INFORMATION_REQUEST: u8 = 0x0A;
    pub const INFORMATION_RESPONSE: u8 = 0x0B;
}

/// Connection Response results.
pub mod conn_result {
    pub const SUCCESS: u16 = 0x0000;
    pub const PENDING: u16 = 0x0001;
    pub const PSM_NOT_SUPPORTED: u16 = 0x0002;
    pub const SECURITY_BLOCK: u16 = 0x0003;
    pub const NO_RESOURCES: u16 = 0x0004;
}

/// Connection Response status (only meaningful with "pending").
pub mod conn_status {
    pub const NO_INFO: u16 = 0x0000;
    pub const AUTHENTICATION_PENDING: u16 = 0x0001;
    pub const AUTHORIZATION_PENDING: u16 = 0x0002;
}

/// Configuration Response results.
pub mod conf_result {
    pub const SUCCESS: u16 = 0x0000;
    pub const UNACCEPTABLE: u16 = 0x0001;
    pub const REJECTED: u16 = 0x0002;
    pub const UNKNOWN_OPTIONS: u16 = 0x0003;
    pub const PENDING: u16 = 0x0004;
}

/// Configuration option types.
pub mod opt {
    pub const MTU: u8 = 0x01;
    pub const FLUSH_TIMEOUT: u8 = 0x02;
    pub const QOS: u8 = 0x03;
    pub const RETRANSMISSION: u8 = 0x04;
    pub const FCS: u8 = 0x05;
    pub const EXT_FLOW_SPEC: u8 = 0x06;
    pub const EXT_WINDOW: u8 = 0x07;
    /// Top bit set: a "hint", which may be ignored if not understood.
    pub const HINT: u8 = 0x80;
}

/// One signalling command, as read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Signal<'a> {
    CommandReject {
        reason: u16,
        data: &'a [u8],
    },
    ConnectionRequest {
        psm: u16,
        scid: u16,
    },
    ConnectionResponse {
        dcid: u16,
        scid: u16,
        result: u16,
        status: u16,
    },
    ConfigureRequest {
        dcid: u16,
        flags: u16,
        options: &'a [u8],
    },
    ConfigureResponse {
        scid: u16,
        flags: u16,
        result: u16,
        options: &'a [u8],
    },
    DisconnectionRequest {
        dcid: u16,
        scid: u16,
    },
    DisconnectionResponse {
        dcid: u16,
        scid: u16,
    },
    EchoRequest {
        data: &'a [u8],
    },
    EchoResponse {
        data: &'a [u8],
    },
    InformationRequest {
        info_type: u16,
    },
    InformationResponse {
        info_type: u16,
        result: u16,
        data: &'a [u8],
    },
    Unknown {
        code: u8,
        data: &'a [u8],
    },
    /// Too short for its code.
    Malformed {
        code: u8,
        data: &'a [u8],
    },
}

/// One command from the signalling channel: code, identifier (which
/// request an answer belongs to), and the decoded content.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Command<'a> {
    pub code: u8,
    pub id: u8,
    pub signal: Signal<'a>,
}

/// The commands in one signalling frame's payload (there may be several).
/// Stops at the first command whose length runs past the end.
pub fn commands(mut payload: &[u8]) -> impl Iterator<Item = Command<'_>> {
    core::iter::from_fn(move || {
        if payload.len() < 4 {
            return None;
        }
        let code = payload[0];
        let id = payload[1];
        let len = usize::from(u16::from_le_bytes([payload[2], payload[3]]));
        let data = payload.get(4..4 + len)?;
        payload = &payload[4 + len..];
        Some(Command {
            code,
            id,
            signal: decode(code, data),
        })
    })
}

fn decode(code: u8, d: &[u8]) -> Signal<'_> {
    let w = |i: usize| u16_at(d, i);
    let s = (|| {
        Some(match code {
            code::COMMAND_REJECT => Signal::CommandReject {
                reason: w(0)?,
                data: &d[2..],
            },
            code::CONNECTION_REQUEST => Signal::ConnectionRequest {
                psm: w(0)?,
                scid: w(2)?,
            },
            code::CONNECTION_RESPONSE => Signal::ConnectionResponse {
                dcid: w(0)?,
                scid: w(2)?,
                result: w(4)?,
                status: w(6)?,
            },
            code::CONFIGURE_REQUEST => Signal::ConfigureRequest {
                dcid: w(0)?,
                flags: w(2)?,
                options: &d[4..],
            },
            code::CONFIGURE_RESPONSE => Signal::ConfigureResponse {
                scid: w(0)?,
                flags: w(2)?,
                result: w(4)?,
                options: &d[6..],
            },
            code::DISCONNECTION_REQUEST => Signal::DisconnectionRequest {
                dcid: w(0)?,
                scid: w(2)?,
            },
            code::DISCONNECTION_RESPONSE => Signal::DisconnectionResponse {
                dcid: w(0)?,
                scid: w(2)?,
            },
            code::ECHO_REQUEST => Signal::EchoRequest { data: d },
            code::ECHO_RESPONSE => Signal::EchoResponse { data: d },
            code::INFORMATION_REQUEST => Signal::InformationRequest { info_type: w(0)? },
            code::INFORMATION_RESPONSE => Signal::InformationResponse {
                info_type: w(0)?,
                result: w(2)?,
                data: &d[4..],
            },
            _ => Signal::Unknown { code, data: d },
        })
    })();
    s.unwrap_or(Signal::Malformed { code, data: d })
}

/// Writes one signalling command (code, id, length, data) into `out`.
/// Returns the bytes used, or 0 if it does not fit.
pub fn write_command(out: &mut [u8], code: u8, id: u8, data: &[u8]) -> usize {
    let n = 4 + data.len();
    if out.len() < n {
        return 0;
    }
    out[0] = code;
    out[1] = id;
    out[2..4].copy_from_slice(&(data.len() as u16).to_le_bytes());
    out[4..n].copy_from_slice(data);
    n
}

/// One configuration option.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opt<'a> {
    /// Type with the hint bit removed.
    pub kind: u8,
    pub hint: bool,
    pub data: &'a [u8],
}

/// The options of a configuration request or response. `Err(())` at the
/// end if an option's length runs past the end.
pub fn options(mut d: &[u8]) -> impl Iterator<Item = Result<Opt<'_>, ()>> {
    let mut broken = false;
    core::iter::from_fn(move || {
        if broken || d.is_empty() {
            return None;
        }
        if d.len() < 2 || d.len() < 2 + usize::from(d[1]) {
            broken = true;
            return Some(Err(()));
        }
        let len = usize::from(d[1]);
        let o = Opt {
            kind: d[0] & !opt::HINT,
            hint: d[0] & opt::HINT != 0,
            data: &d[2..2 + len],
        };
        d = &d[2 + len..];
        Some(Ok(o))
    })
}

/// Our answer to the other side's Configuration Request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConfigAnswer {
    pub result: u16,
    /// Options to put in the response.
    pub options: [u8; 24],
    pub options_len: usize,
    /// The MTU they asked for, if they named one (what we may send them).
    pub mtu: Option<u16>,
    /// A retransmission mode other than basic was asked for (and refused).
    pub refused_mode: Option<u8>,
}

impl ConfigAnswer {
    pub fn options(&self) -> &[u8] {
        &self.options[..self.options_len]
    }
}

/// Decides the answer to a Configuration Request, following the
/// specification: options we do not know and that are not hints are
/// listed back with "unknown options"; values we cannot live with get
/// "unacceptable parameters" with the values we can; anything else is
/// accepted. We only do basic mode, so any other retransmission mode is
/// unacceptable and "basic" is proposed instead.
pub fn answer_config(options_in: &[u8]) -> ConfigAnswer {
    let mut unknown = [0_u8; 24];
    let mut n_unknown = 0;
    let mut fix = [0_u8; 24];
    let mut n_fix = 0;
    let mut mtu = None;
    let mut refused_mode = None;
    let mut malformed = false;
    for o in options(options_in) {
        let Ok(o) = o else {
            malformed = true;
            break;
        };
        match o.kind {
            opt::MTU if o.data.len() >= 2 => {
                let m = u16::from_le_bytes([o.data[0], o.data[1]]);
                if m < MTU_MIN {
                    if n_fix + 4 <= fix.len() {
                        fix[n_fix..n_fix + 4].copy_from_slice(&[opt::MTU, 2, MTU_MIN as u8, 0]);
                        n_fix += 4;
                    }
                } else {
                    mtu = Some(m);
                }
            }
            opt::RETRANSMISSION if !o.data.is_empty() => {
                if o.data[0] != 0 {
                    refused_mode = Some(o.data[0]);
                    // Basic mode; the other fields are ignored in basic mode.
                    let basic = [opt::RETRANSMISSION, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0];
                    if n_fix + basic.len() <= fix.len() {
                        fix[n_fix..n_fix + basic.len()].copy_from_slice(&basic);
                        n_fix += basic.len();
                    }
                }
            }
            // Known options that change nothing for us in basic mode.
            opt::MTU
            | opt::RETRANSMISSION
            | opt::FLUSH_TIMEOUT
            | opt::QOS
            | opt::FCS
            | opt::EXT_FLOW_SPEC
            | opt::EXT_WINDOW => {}
            _ if o.hint => {}
            other => {
                if n_unknown < unknown.len() {
                    unknown[n_unknown] = other;
                    n_unknown += 1;
                }
            }
        }
    }
    let mut a = ConfigAnswer {
        result: conf_result::SUCCESS,
        options: [0; 24],
        options_len: 0,
        mtu,
        refused_mode,
    };
    if malformed {
        a.result = conf_result::REJECTED;
    } else if n_unknown > 0 {
        a.result = conf_result::UNKNOWN_OPTIONS;
        a.options[..n_unknown].copy_from_slice(&unknown[..n_unknown]);
        a.options_len = n_unknown;
    } else if n_fix > 0 {
        a.result = conf_result::UNACCEPTABLE;
        a.options[..n_fix].copy_from_slice(&fix[..n_fix]);
        a.options_len = n_fix;
    }
    a
}

pub fn code_name(code: u8) -> &'static str {
    match code {
        code::COMMAND_REJECT => "command-reject",
        code::CONNECTION_REQUEST => "connection-request",
        code::CONNECTION_RESPONSE => "connection-response",
        code::CONFIGURE_REQUEST => "configure-request",
        code::CONFIGURE_RESPONSE => "configure-response",
        code::DISCONNECTION_REQUEST => "disconnection-request",
        code::DISCONNECTION_RESPONSE => "disconnection-response",
        code::ECHO_REQUEST => "echo-request",
        code::ECHO_RESPONSE => "echo-response",
        code::INFORMATION_REQUEST => "information-request",
        code::INFORMATION_RESPONSE => "information-response",
        _ => "unknown",
    }
}

pub fn psm_name(psm: u16) -> &'static str {
    match psm {
        PSM_SDP => "sdp",
        PSM_HID_CONTROL => "hid-control",
        PSM_HID_INTERRUPT => "hid-interrupt",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn several_commands_in_one_frame() {
        let mut f = [0_u8; 64];
        let mut n = write_command(
            &mut f,
            code::CONNECTION_REQUEST,
            3,
            &[0x11, 0x00, 0x40, 0x00],
        );
        n += write_command(&mut f[n..], code::ECHO_REQUEST, 4, b"hi");
        let v: Vec<_> = commands(&f[..n]).collect();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, 3);
        assert_eq!(
            v[0].signal,
            Signal::ConnectionRequest {
                psm: 0x11,
                scid: 0x40
            }
        );
        assert_eq!(v[1].signal, Signal::EchoRequest { data: b"hi" });
    }

    #[test]
    fn short_command_is_malformed() {
        let f = [code::CONNECTION_RESPONSE, 1, 2, 0, 0x40, 0x00];
        let v: Vec<_> = commands(&f).collect();
        assert!(matches!(v[0].signal, Signal::Malformed { .. }));
    }

    #[test]
    fn config_plain_mtu_is_accepted() {
        let a = answer_config(&[opt::MTU, 2, 0x80, 0x02]);
        assert_eq!(a.result, conf_result::SUCCESS);
        assert_eq!(a.mtu, Some(640));
        assert!(a.options().is_empty());
    }

    #[test]
    fn config_small_mtu_is_unacceptable() {
        let a = answer_config(&[opt::MTU, 2, 20, 0]);
        assert_eq!(a.result, conf_result::UNACCEPTABLE);
        assert_eq!(a.options(), &[opt::MTU, 2, 48, 0]);
    }

    #[test]
    fn config_ertm_is_refused_basic_proposed() {
        let a = answer_config(&[
            opt::MTU,
            2,
            0xA0,
            0x02,
            opt::RETRANSMISSION,
            9,
            3,
            10,
            3,
            0xD0,
            7,
            0xE0,
            0x2E,
            0xF0,
            0x03,
        ]);
        assert_eq!(a.result, conf_result::UNACCEPTABLE);
        assert_eq!(a.refused_mode, Some(3));
        assert_eq!(a.options()[..3], [opt::RETRANSMISSION, 9, 0]);
    }

    #[test]
    fn config_unknown_option_listed_but_hint_ignored() {
        let a = answer_config(&[
            0x09 | opt::HINT,
            1,
            0,
            0x0A,
            1,
            0,
            opt::FLUSH_TIMEOUT,
            2,
            0xFF,
            0xFF,
        ]);
        assert_eq!(a.result, conf_result::UNKNOWN_OPTIONS);
        assert_eq!(a.options(), &[0x0A]);
    }

    #[test]
    fn config_malformed_is_rejected() {
        let a = answer_config(&[opt::MTU, 4, 0]);
        assert_eq!(a.result, conf_result::REJECTED);
    }
}
