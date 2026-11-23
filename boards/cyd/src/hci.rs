//! The HCI byte path to the radio's controller (feature `pad`), both
//! ways, as whole packets with their kind byte first (0x01 command, 0x02
//! ACL data, 0x04 event).
//!
//! The controller runs in its own esp-rtos task. Each packet it has for
//! the host is copied onto the heap and queued by the driver (the patched
//! esp-radio of experiments/bt-classic-probe) at once, so the controller
//! never waits for the host and no packet is lost while the console draws
//! a frame (only the heap could run out: a few hundred bytes per frame at
//! 100 packets a second, see README). [`HciPort::receive`] takes them
//! from that queue one at a time; [`HciPort::wait`] sleeps until there is
//! one.
//!
//! Every packet to the controller passes the gate of hci-rules
//! (`hci_rules::Gate`) first: only what the reconnect-only gamepad host
//! needs, and an accept or a key only for a saved pad's address (the
//! gate's list is filled once at start from the pairings read). No
//! search, no inquiry scan, no call of our own, no "yes" to a pairing
//! can reach the radio. [`HciPort::send`] never waits: when the
//! controller cannot take a packet now it says [`SendError::Busy`] and
//! nothing was sent.

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use esp_hal::peripherals::BT;
use esp_radio::ble::controller::BleConnector;
use hci_rules::{Gate, MAX_PACKET, Refusal, kind, packet_len};

/// Why a packet did not go to the controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendError {
    /// The gate refused it: it never reached the radio.
    Refused(Refusal),
    /// The controller cannot take a packet now (the one before is still
    /// being taken). Nothing was sent; try again later.
    Busy,
}

/// What went through the byte path.
#[derive(Debug, Default, Clone, Copy)]
pub struct HciCounters {
    /// Packets from the controller: all, events, ACL data.
    pub in_packets: u32,
    pub in_events: u32,
    pub in_acl: u32,
    pub in_bytes: u32,
    /// Packets from the controller whose length did not match their
    /// header, or of an unknown kind: dropped.
    pub in_bad: u32,
    /// Packets to the controller: sent, refused by the gate, not taken
    /// (busy).
    pub out_packets: u32,
    pub out_bytes: u32,
    pub out_refused: u32,
    pub out_busy: u32,
}

/// The radio's controller, as a byte path.
pub struct HciPort {
    conn: BleConnector<'static>,
    /// The packet handed out by [`HciPort::receive`].
    rx: [u8; MAX_PACKET],
    /// The first byte of the next packet, already taken by
    /// [`HciPort::wait`].
    first: Option<u8>,
    /// When [`HciPort::wait`] saw data come (us), until the next
    /// [`HciPort::take_seen`].
    seen_us: Option<u64>,
    pub counters: HciCounters,
}

impl HciPort {
    /// Starts the radio: the controller in dual mode (classic and Low
    /// Energy), nothing sent to it yet. About 400 ms, about 19.6 KB of
    /// the heap.
    pub fn start(bt: BT<'static>) -> Result<Self, esp_radio::ble::controller::BleInitError> {
        Ok(HciPort {
            conn: BleConnector::new(bt, Default::default())?,
            rx: [0; MAX_PACKET],
            first: None,
            seen_us: None,
            counters: HciCounters::default(),
        })
    }

    /// Whether the controller has something waiting.
    pub fn has_data(&self) -> bool {
        self.first.is_some() || esp_radio::ble::have_hci_read_data()
    }

    /// The next whole packet from the controller (kind byte first), or
    /// `None` when nothing waits. A packet that does not match its own
    /// header is counted (`in_bad`) and skipped. Never waits.
    pub fn receive(&mut self) -> Option<&[u8]> {
        let n = loop {
            let start = match self.first.take() {
                Some(b) => {
                    self.rx[0] = b;
                    1
                }
                None => 0,
            };
            let n = start + esp_radio::ble::read_hci(&mut self.rx[start..]);
            if n == 0 {
                return None;
            }
            let want = packet_len(&self.rx[..n]);
            if n == MAX_PACKET {
                // Longer than any packet this controller makes: read the
                // rest away (exactly, when the header says how much).
                self.skip_rest(want.map(|w| w.saturating_sub(n)));
                self.counters.in_bad += 1;
                continue;
            }
            if want != Some(n) {
                self.counters.in_bad += 1;
                continue;
            }
            break n;
        };
        let c = &mut self.counters;
        c.in_packets += 1;
        c.in_bytes += n as u32;
        match self.rx[0] {
            kind::EVENT => c.in_events += 1,
            kind::ACL => c.in_acl += 1,
            _ => {}
        }
        Some(&self.rx[..n])
    }

    /// Reads away the rest of a packet too long for the buffer: `left`
    /// bytes, or (unknown) until a read comes back short.
    fn skip_rest(&mut self, left: Option<usize>) {
        let mut scratch = [0u8; 64];
        match left {
            Some(mut left) => {
                while left > 0 {
                    let take = left.min(scratch.len());
                    let got = esp_radio::ble::read_hci(&mut scratch[..take]);
                    if got == 0 {
                        break;
                    }
                    left -= got;
                }
            }
            None => while esp_radio::ble::read_hci(&mut scratch) == scratch.len() {},
        }
    }

    /// Hands one whole packet (kind byte first) to the controller, if the
    /// gate allows it and the controller can take it now. Never waits.
    pub fn send(&mut self, gate: &Gate, packet: &[u8]) -> Result<(), SendError> {
        if let Err(why) = gate.check(packet) {
            self.counters.out_refused += 1;
            return Err(SendError::Refused(why));
        }
        // One look at the driver's send: it either takes the whole packet
        // now (it was checked above, so its length is right) or is not
        // ready and takes nothing.
        let mut fut = pin!(self.conn.write_async(packet));
        let mut cx = Context::from_waker(Waker::noop());
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(n)) if n == packet.len() => {
                self.counters.out_packets += 1;
                self.counters.out_bytes += n as u32;
                Ok(())
            }
            // Taken only in part: cannot happen for a checked packet.
            Poll::Ready(_) => {
                self.counters.out_refused += 1;
                Err(SendError::Refused(Refusal::BadLength))
            }
            Poll::Pending => {
                self.counters.out_busy += 1;
                Err(SendError::Busy)
            }
        }
    }

    /// Waits until the controller has something for [`HciPort::receive`].
    /// Safe to drop at any point (in a `select`): nothing is lost.
    pub async fn wait(&mut self) {
        if self.has_data() {
            return;
        }
        let mut b = [0u8; 1];
        // Returns as soon as data waits, with its first byte. Dropped
        // before that, it has taken nothing.
        if let Ok(1) = self.conn.read_async(&mut b).await {
            self.first = Some(b[0]);
            self.seen_us = Some(crate::board::now_us());
        }
    }

    /// When [`HciPort::wait`] last saw data come, once: the moment a
    /// packet arrived while the loop slept on the radio. None when it
    /// came while the loop was busy (then only the last look is known).
    pub fn take_seen(&mut self) -> Option<u64> {
        self.seen_us.take()
    }
}
