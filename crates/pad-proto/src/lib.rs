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
//!
//! Crate feature `pair` (off by default): a run-time pairing window in
//! `Host` for the console's everyday reconnect-only mode. The caller opens
//! it (`Host::open_pairing`), the host searches and reports the gamepads it
//! hears (`UiEvent::Found`, the name never logged), the user picks one
//! (`Host::pair_with`), and the host calls and pairs that one address only
//! (Secure Simple Pairing "just works", key types 0x04 and 0x07), then
//! closes the window with exactly one outcome (`PairEnd`, read with
//! `Host::pairing_outcome`). The new key is reported for saving only once
//! encryption is on; it replaces the same address's key in place and never
//! pushes another pad out. The feature also adds `Host::forget_key_of`
//! (forget one pad). Every call is refused while a window is open. Without
//! the feature none of this exists and the host behaves exactly as before.
//! (Plain names, not links: they exist only with the feature.)

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
#[cfg(feature = "pair")]
pub use host::{PAIR_WINDOW_MAX, PairEnd, PairError, Window};
