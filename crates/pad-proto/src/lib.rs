//! A small classic Bluetooth host for one gamepad, written for the ESP32
//! but testable on a PC.
//!
//! No heap and no operating system: every buffer has a fixed size. The
//! radio's controller speaks HCI (commands, events, data packets); this
//! crate speaks the layers above it:
//!
//! - [`hci`]: build commands, read events.
//! - [`acl`]: cut data into packets, join it again, count the controller's
//!   free buffers.
//! - [`l2cap`]: channels inside one link, and their set-up messages.
//! - [`sdp`]: ask the gamepad what it is (the HID description, the vendor
//!   and product numbers).
//! - [`hid`]: the one-byte header in front of each HID message.
//! - [`host`]: the state machine tying it all together. It takes
//!   (packet or tick, time) and gives (packets to send, events for the
//!   screen, log lines), so tests can replay whole conversations.

#![cfg_attr(not(test), no_std)]

pub mod acl;
pub mod buf;
pub mod hci;
pub mod hid;
pub mod host;
pub mod l2cap;
pub mod sdp;

pub use host::{
    Config, DeviceInfo, ForgetReason, Host, Io, KeyChange, Purpose, Stats, Tag, UiEvent,
};
