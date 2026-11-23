//! The Bluetooth HID profile's own small header ("HIDP").
//!
//! Every message on the two HID channels starts with one byte: the
//! message type in the top four bits, a parameter in the low four. The
//! gamepad's input reports arrive on the interrupt channel as
//! DATA + Input (0xA1), then the report itself. Output reports (rumble,
//! lights, start-up commands) go the other way as DATA + Output (0xA2).

pub const HANDSHAKE: u8 = 0x0;
pub const HID_CONTROL: u8 = 0x1;
pub const GET_REPORT: u8 = 0x4;
pub const SET_REPORT: u8 = 0x5;
pub const GET_PROTOCOL: u8 = 0x6;
pub const SET_PROTOCOL: u8 = 0x7;
pub const DATA: u8 = 0xA;

pub const REPORT_INPUT: u8 = 0x1;
pub const REPORT_OUTPUT: u8 = 0x2;
pub const REPORT_FEATURE: u8 = 0x3;

/// DATA + Input: an input report follows.
pub const DATA_INPUT: u8 = 0xA1;
/// DATA + Output: an output report follows.
pub const DATA_OUTPUT: u8 = 0xA2;
/// SET_REPORT + Output, on the control channel: an output report follows
/// (report id first). The device answers with a HANDSHAKE.
pub const SET_REPORT_OUTPUT: u8 = (SET_REPORT << 4) | REPORT_OUTPUT;
/// GET_REPORT + Input, on the control channel: "send me this input report
/// now" (the report id follows). The device answers with DATA + Input
/// (0xA1, the report) or with a HANDSHAKE (an error code).
pub const GET_REPORT_INPUT: u8 = (GET_REPORT << 4) | REPORT_INPUT;

/// HID_CONTROL parameter: the device is being unpaired ("virtual cable
/// unplug"): forget it.
pub const CONTROL_VIRTUAL_CABLE_UNPLUG: u8 = 0x5;

pub fn kind(header: u8) -> u8 {
    header >> 4
}

pub fn param(header: u8) -> u8 {
    header & 0x0F
}

/// The message type in words.
pub fn kind_name(header: u8) -> &'static str {
    match kind(header) {
        HANDSHAKE => "handshake",
        HID_CONTROL => "hid-control",
        GET_REPORT => "get-report",
        SET_REPORT => "set-report",
        GET_PROTOCOL => "get-protocol",
        SET_PROTOCOL => "set-protocol",
        DATA => match param(header) {
            REPORT_INPUT => "data-input",
            REPORT_OUTPUT => "data-output",
            REPORT_FEATURE => "data-feature",
            _ => "data-other",
        },
        _ => "reserved",
    }
}

/// HANDSHAKE results in words.
pub fn handshake_text(param: u8) -> &'static str {
    match param {
        0x0 => "successful",
        0x1 => "not-ready",
        0x2 => "invalid-report-id",
        0x3 => "unsupported-request",
        0x4 => "invalid-parameter",
        0xE => "unknown",
        0xF => "fatal",
        _ => "reserved",
    }
}

/// HID_CONTROL operations in words.
pub fn control_text(param: u8) -> &'static str {
    match param {
        0x0 => "nop",
        0x1 => "hard-reset",
        0x2 => "soft-reset",
        0x3 => "suspend",
        0x4 => "exit-suspend",
        0x5 => "virtual-cable-unplug",
        _ => "reserved",
    }
}

/// An input report from an interrupt-channel message, without the 0xA1
/// header byte. `None` if the message is something else.
pub fn input_report(msg: &[u8]) -> Option<&[u8]> {
    match msg.split_first() {
        Some((&DATA_INPUT, rest)) => Some(rest),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers() {
        assert_eq!(kind_name(0xA1), "data-input");
        assert_eq!(kind_name(0x00), "handshake");
        assert_eq!(kind_name(0x15), "hid-control");
        assert_eq!(control_text(param(0x15)), "virtual-cable-unplug");
        assert_eq!(input_report(&[0xA1, 1, 2]), Some(&[1, 2][..]));
        assert_eq!(input_report(&[0xA2, 1, 2]), None);
        assert_eq!(input_report(&[]), None);
        assert_eq!(SET_REPORT_OUTPUT, 0x52);
        assert_eq!(kind_name(SET_REPORT_OUTPUT), "set-report");
    }
}
