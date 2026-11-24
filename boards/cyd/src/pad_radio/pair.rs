//! UI-driven pairing and forgetting requests (feature `pair`).
//!
//! Runs after `Console::step` in `App::run`: serves any pending request
//! from `CydPad::pair_req`, enforces deadlines (host 60 s search / 30 s
//! call; gate +6 s capped at 120 s), binds the gate to the chosen pad,
//! writes `0x9000..0xB000` only on pair/forget (never erases), and never
//! disconnects a linked pad on opening GAMEPADS or on a refused `Begin`.

use board_api::{PairFail, PairReq, PairState};
use hci_rules::PairRefusal;
use log::{info, warn};
use pad_host::{PairEnd, PairError, PairPhase, PairRefused};

use super::{Out, PadRadio, TICK_BUSY_US, ms};
use crate::board::CydPad;
use crate::flash_store::CydStore;
use crate::pairing::{
    PairWriteError, PairingArea, forget as flash_forget, save_new as flash_save_new,
};

const SEARCH_HOST_MS: u64 = 60_000;
const SEARCH_GATE_MS: u64 = 66_000;
const CALL_HOST_MS: u64 = 30_000;
const CALL_GATE_MS: u64 = 36_000;

impl PadRadio {
    /// Stores the flash metadata found at startup.
    pub fn set_pairing_area(&mut self, area: PairingArea) {
        self.pairing_area = area;
    }

    fn cancel_window(&mut self, now_ms: u64, host: &mut pad_host::PadHost) {
        let mut out = Out {
            port: &mut self.port,
            gate: &self.gate,
            queue: &mut self.queue,
            c: &mut self.counters,
        };
        host.pair_cancel(now_ms, &mut out);
        self.gate.disarm();
        self.pair_window_open = false;
    }

    /// Serves pairing and forgetting requests between console steps.
    pub fn serve_pairing(
        &mut self,
        now: u64,
        on_page: bool,
        pad: &mut CydPad,
        store: &mut CydStore,
    ) {
        let Some(host) = pad.host.as_deref_mut() else {
            return;
        };
        let now_ms = ms(now);
        let prev_state = pad.pair_state;

        if let Some(req) = pad.pair_req.take() {
            match req {
                PairReq::Cancel => {
                    self.cancel_window(now_ms, host);
                    pad.pair_state = PairState::Off;
                }
                PairReq::Ack => {
                    if pad.pair_state.is_end() {
                        pad.pair_state = PairState::Off;
                    }
                }
                PairReq::Begin => {
                    if !pad.pair_state.is_open() {
                        pad.pair_state = self.handle_begin(now, now_ms, host);
                    }
                }
                PairReq::Choose { i, tail } => {
                    if self.pair_window_open && pad.pair_state == PairState::Searching {
                        pad.pair_state = self.handle_choose(now, now_ms, i, tail, host);
                    }
                }
                PairReq::Forget { place, tail } => {
                    if !pad.pair_state.is_open() {
                        let (st, slots) = self.handle_forget(now_ms, place, tail, host, store);
                        pad.pair_state = st;
                        if let Some(s) = slots {
                            pad.pair_slots = s;
                        }
                    }
                }
            }
        }

        if !on_page {
            if self.pair_window_open {
                self.cancel_window(now_ms, host);
                pad.pair_state = PairState::Off;
            } else if pad.pair_state.is_end() {
                pad.pair_state = PairState::Off;
            }
        }

        if self.pair_window_open && host.pair_phase() == PairPhase::Off {
            self.pair_window_open = false;
            pad.pair_state = self.finish_window(now_ms, host, store, &mut pad.pair_slots);
        }

        self.queue
            .drain(&mut self.port, &self.gate, &mut self.counters);

        if pad.pair_state != prev_state {
            pad.wake = true;
        }
    }

    fn handle_begin(&mut self, now: u64, now_ms: u64, host: &mut pad_host::PadHost) -> PairState {
        match self.pairing_area {
            PairingArea::Damaged => return PairState::Failed(PairFail::Damaged),
            PairingArea::Unavailable => return PairState::Failed(PairFail::Store),
            PairingArea::Ready { free_slots: 0, .. } => {
                return PairState::Failed(PairFail::NoRoom);
            }
            PairingArea::Ready { .. } => {}
        }
        // Never disconnect an already-connected gamepad or overwrite Player 1.
        if host.connected() {
            return PairState::Failed(PairFail::Linked);
        }
        if host.saved_pads() > 0 {
            return PairState::Failed(PairFail::Full);
        }
        self.gate.disarm();
        if let Err(e) = self.gate.arm(now_ms, SEARCH_GATE_MS) {
            warn!("pair: gate arm refused: {e:?}");
            return PairState::Failed(PairFail::Busy);
        }
        let mut out = Out {
            port: &mut self.port,
            gate: &self.gate,
            queue: &mut self.queue,
            c: &mut self.counters,
        };
        match host.pair_begin(now_ms, SEARCH_HOST_MS, &mut out) {
            Ok(()) => {
                info!("pair: search started");
                self.pair_window_open = true;
                self.next_tick_us = now + TICK_BUSY_US;
                PairState::Searching
            }
            Err(e) => {
                warn!("pair: host begin refused: {}", e.word());
                self.gate.disarm();
                PairState::Failed(PairFail::Busy)
            }
        }
    }

    fn handle_choose(
        &mut self,
        now: u64,
        now_ms: u64,
        i: u8,
        tail: [u8; 2],
        host: &mut pad_host::PadHost,
    ) -> PairState {
        let Some(addr) = host.pair_target(usize::from(i), tail) else {
            self.cancel_window(now_ms, host);
            return PairState::Failed(PairFail::Busy);
        };
        if let Err(e) = self.gate.bind(addr.whole_address(), now_ms, CALL_GATE_MS) {
            warn!("pair: gate bind refused: {e:?}");
            self.cancel_window(now_ms, host);
            return PairState::Failed(match e {
                PairRefusal::Full => PairFail::Full,
                _ => PairFail::Busy,
            });
        }
        let mut out = Out {
            port: &mut self.port,
            gate: &self.gate,
            queue: &mut self.queue,
            c: &mut self.counters,
        };
        match host.pair_choose(usize::from(i), now_ms, CALL_HOST_MS, &mut out) {
            Ok(()) => {
                info!("pair: calling {addr}");
                self.next_tick_us = now + TICK_BUSY_US;
                PairState::Calling
            }
            Err(PairRefused::Host(PairError::Full)) => {
                self.cancel_window(now_ms, host);
                PairState::Failed(PairFail::Full)
            }
            Err(e) => {
                warn!("pair: host choose refused: {}", e.word());
                self.cancel_window(now_ms, host);
                PairState::Failed(PairFail::Busy)
            }
        }
    }

    fn handle_forget(
        &mut self,
        now_ms: u64,
        place: u8,
        tail: [u8; 2],
        host: &mut pad_host::PadHost,
        store: &mut CydStore,
    ) -> (PairState, Option<Option<u8>>) {
        match self.pairing_area {
            PairingArea::Damaged => return (PairState::Failed(PairFail::Damaged), None),
            PairingArea::Unavailable => return (PairState::Failed(PairFail::Store), None),
            PairingArea::Ready { .. } => {}
        }
        if !host
            .saved_pad(usize::from(place))
            .is_some_and(|s| s.tail == tail)
        {
            return (PairState::Failed(PairFail::Busy), None);
        }
        let Some(addr) = host.saved_address(usize::from(place)) else {
            return (PairState::Failed(PairFail::Busy), None);
        };
        // Write flash before updating host & gate so an I/O failure never
        // desynchronizes the live state from flash.
        let r = store.with_flash(|fs| flash_forget(fs, &mut self.pairing_area, addr));
        let slots = Some(self.pairing_area.status().1);
        match r {
            Ok(_) => {
                let mut out = Out {
                    port: &mut self.port,
                    gate: &self.gate,
                    queue: &mut self.queue,
                    c: &mut self.counters,
                };
                let forgotten = host.forget(usize::from(place), now_ms, &mut out);
                self.gate.forget(addr);
                if let Some(p) = forgotten {
                    info!("pair: forgot {p} at place {place}");
                }
                (PairState::Forgotten(place), slots)
            }
            Err(PairWriteError::Damaged) => (PairState::Failed(PairFail::Damaged), slots),
            Err(_) => (PairState::Failed(PairFail::Store), slots),
        }
    }

    fn finish_window(
        &mut self,
        now_ms: u64,
        host: &mut pad_host::PadHost,
        store: &mut CydStore,
        pair_slots: &mut Option<u8>,
    ) -> PairState {
        let outcome = host.pair_outcome();
        if outcome != Some(PairEnd::Saved) {
            let _ = host.take_pending_save();
            self.gate.disarm();
            return match outcome {
                Some(PairEnd::Timeout) => PairState::Failed(PairFail::Timeout),
                Some(PairEnd::OldPin) => PairState::Failed(PairFail::OldPin),
                Some(PairEnd::Refused) => PairState::Failed(PairFail::Refused),
                Some(PairEnd::Lost) => PairState::Failed(PairFail::Lost),
                Some(PairEnd::Cancelled) | None => PairState::Off,
                Some(PairEnd::Saved) => unreachable!(),
            };
        }
        let Some(pending) = host.take_pending_save() else {
            self.gate.disarm();
            return PairState::Failed(PairFail::Store);
        };
        let addr = pending.addr().whole_address();
        let tail_addr = pending.addr();
        let r = store.with_flash(|fs| {
            flash_save_new(
                fs,
                &mut self.pairing_area,
                addr,
                pending.key(),
                pending.key_type(),
            )
        });
        drop(pending);
        *pair_slots = self.pairing_area.status().1;
        match r {
            Ok(_) => {
                let allowed = self.gate.allow(addr);
                let place = (0..hci_rules::SAVED)
                    .find(|&i| host.saved_address(i) == Some(addr))
                    .unwrap_or(0) as u8;
                info!(
                    "pair: saved {tail_addr} at place {place} (gate_allow={allowed}, slots={:?})",
                    *pair_slots
                );
                PairState::Saved(place)
            }
            Err(e) => {
                warn!("pair: flash save failed for {tail_addr}: {e:?}");
                if let Some(place) =
                    (0..hci_rules::SAVED).find(|&i| host.saved_address(i) == Some(addr))
                {
                    let mut out = Out {
                        port: &mut self.port,
                        gate: &self.gate,
                        queue: &mut self.queue,
                        c: &mut self.counters,
                    };
                    host.forget(place, now_ms, &mut out);
                }
                self.gate.disarm();
                PairState::Failed(match e {
                    PairWriteError::Damaged => PairFail::Damaged,
                    PairWriteError::NoRoom => PairFail::NoRoom,
                    PairWriteError::Full => PairFail::Full,
                    PairWriteError::Store => PairFail::Store,
                })
            }
        }
    }
}
