//! The gamepad (feature `pad`): the radio, and pad-host run by the
//! console's loop.
//!
//! What it does:
//! - starts the radio (the controller in dual mode) and logs what that
//!   cost: time, mode, heap;
//! - runs pad-host (crates/pad-host), reconnect-only: the controller is
//!   set up by the host's own commands, page scan on (connectable for a
//!   device that already knows the console, never discoverable); a saved
//!   pad that calls is accepted, its saved key given, the HID channels
//!   opened, and in Switch mode the pad's start-up talk held; anybody
//!   else is refused;
//! - gives pad-host's state to the console as pad 0 (src/board.rs
//!   `CydPad`), and wakes the console only when pad-host says a game
//!   would see something new;
//! - counts everything (`boardctl stats` line "pad", and a log line every
//!   5 s when it changed);
//! - plays an injected pad for testing (debug link kind 0x08,
//!   `boardctl pad`): pad-host's own decoder, wake rule and press latch
//!   run as for the real pad, nothing goes over the radio.
//!
//! What it never does: search for devices, become discoverable, start a
//! connection, pair, or write a key. Every packet to the controller
//! passes the gate (`hci_rules::Gate`, src/hci.rs), which knows the saved
//! pads' addresses (read once at start, read only: src/pairing.rs) and
//! lets an accept or a key reply go only to one of them.
//!
//! # Where the host runs
//!
//! The console's loop (src/app.rs) calls [`PadRadio::poll`] once per
//! turn, just before it looks at the input and steps the console, and
//! sleeps on [`PadRadio::wait`] beside its other wake-ups; the host's own
//! times ([`PadRadio::due`]) bring the loop round to `poll` but never
//! make the console step. Packets the controller has for the host are
//! queued on the heap by the driver meanwhile (none lost while a frame
//! is drawn); packets the host sends while the controller cannot take one
//! wait in a small queue here, in order. See README "The gamepad (pad)".

use debug_link::msg::status;
use debug_link::pad::{PadRequest, mode};
use hci_rules::{Gate, MAX_PACKET};
use log::{info, warn};
use pad_host::{Mode, PadEvent, PadHost, Radio, TX_MAX, inject};

use crate::board::CydPad;
use crate::hci::{HciPort, SendError};

unsafe extern "C" {
    // In the controller library: the mode it runs in (1 Low Energy only,
    // 2 classic only, 3 dual).
    fn btdm_controller_get_mode() -> u32;
}

/// Packets that wait for the controller, at most (a Switch start-up
/// sends a few output reports back to back; the controller takes one at
/// a time).
const OUT_QUEUE: usize = 8;
/// pad-host's timers: every 15 ms while it is busy (setting up, a call,
/// a link), once a second while it only waits for a call.
const TICK_BUSY_US: u64 = 15_000;
const TICK_IDLE_US: u64 = 1_000_000;
/// The "pad" log line: at most this often, and only when it changed.
const LINE_US: u64 = 5_000_000;
/// Shortest injected repeat period (200 reports a second).
#[cfg_attr(not(feature = "link"), allow(dead_code))]
const REPEAT_MIN_MS: u16 = 5;
/// Most injected repeat reports handed over in one turn (a frame of
/// 80 ms at 100 a second; the real driver's queue held at most 4).
const REPEAT_BURST: u64 = 8;

/// pad-host's way out before the radio runs (reading the pairings): log
/// lines only; a packet cannot go anywhere (none is sent then).
pub struct LogOnly;

impl Radio for LogOnly {
    fn send(&mut self, _packet: &[u8]) {
        warn!("pad: a packet before the radio started, dropped");
    }

    fn log(&mut self, tag: &'static str, args: core::fmt::Arguments<'_>) {
        info!("{tag} {args}");
    }
}

/// What the board side counted (pad-host counts the rest).
#[derive(Default, Clone, Copy)]
pub struct PadCounters {
    /// Most packets read in one `poll` (how many had piled up).
    pub backlog_max: u32,
    /// Least heap free seen with packets waiting.
    pub heap_min: u32,
    /// Longest time a packet waited before the host got it (us): from
    /// the moment the loop's wait saw it come, or (when it came while
    /// the loop was busy) from the last look before, an upper bound.
    pub wait_max_us: u32,
    /// Packets the host sent that had to wait for the controller, the
    /// most waiting at once, and those dropped because the queue was full.
    pub queued: u32,
    pub queue_max: u32,
    pub out_dropped: u32,
    /// Packets the gate refused (never sent).
    pub refused: u32,
    /// Times pad-host asked for the console to look.
    pub wakes: u32,
    /// Time from the radio's start to waiting for a pad (us).
    pub ready_us: u32,
}

/// Packets waiting for the controller, oldest first.
struct OutQueue {
    buf: [[u8; TX_MAX]; OUT_QUEUE],
    len: [u16; OUT_QUEUE],
    head: usize,
    n: usize,
}

impl OutQueue {
    const fn new() -> Self {
        OutQueue {
            buf: [[0; TX_MAX]; OUT_QUEUE],
            len: [0; OUT_QUEUE],
            head: 0,
            n: 0,
        }
    }

    fn push(&mut self, p: &[u8]) -> bool {
        if self.n == OUT_QUEUE || p.len() > TX_MAX {
            return false;
        }
        let i = (self.head + self.n) % OUT_QUEUE;
        self.buf[i][..p.len()].copy_from_slice(p);
        self.len[i] = p.len() as u16;
        self.n += 1;
        true
    }

    /// Sends what waits, in order, until the controller is busy.
    fn drain(&mut self, port: &mut HciPort, gate: &Gate, c: &mut PadCounters) {
        while self.n > 0 {
            let i = self.head;
            let r = port.send(gate, &self.buf[i][..usize::from(self.len[i])]);
            if r == Err(SendError::Busy) {
                return;
            }
            if let Err(SendError::Refused(why)) = r {
                c.refused += 1;
                warn!("pad: packet refused by the gate ({why:?}), not sent");
            }
            // Packets may hold a key (a key reply): not kept around.
            self.buf[i].fill(0);
            self.head = (self.head + 1) % OUT_QUEUE;
            self.n -= 1;
        }
    }
}

/// pad-host's way out: packets through the gate (queued while the
/// controller is busy), log lines to the log.
struct Out<'a> {
    port: &'a mut HciPort,
    gate: &'a Gate,
    queue: &'a mut OutQueue,
    c: &'a mut PadCounters,
}

impl Radio for Out<'_> {
    fn send(&mut self, packet: &[u8]) {
        if self.queue.n == 0 {
            match self.port.send(self.gate, packet) {
                Ok(()) => return,
                Err(SendError::Refused(why)) => {
                    self.c.refused += 1;
                    warn!("pad: packet refused by the gate ({why:?}), not sent");
                    return;
                }
                Err(SendError::Busy) => {}
            }
        }
        if self.queue.push(packet) {
            self.c.queued += 1;
            self.c.queue_max = self.c.queue_max.max(self.queue.n as u32);
        } else {
            // pad-host's time-outs recover (its notes: drop and count).
            self.c.out_dropped += 1;
            warn!("pad: out queue full, a packet dropped");
        }
    }

    fn log(&mut self, tag: &'static str, args: core::fmt::Arguments<'_>) {
        info!("{tag} {args}");
    }
}

/// The injected pad's repeat (`boardctl pad repeat`).
struct Repeat {
    base: [u8; debug_link::pad::MAX_REPORT],
    len: usize,
    period_us: u64,
    next_us: u64,
    n: u32,
}

pub struct PadRadio {
    port: HciPort,
    gate: Gate,
    queue: OutQueue,
    pub counters: PadCounters,
    started_us: u64,
    last_poll_us: u64,
    next_tick_us: u64,
    /// The last injected report, and the repeat.
    #[cfg_attr(not(feature = "link"), allow(dead_code))]
    last: [u8; debug_link::pad::MAX_REPORT],
    #[cfg_attr(not(feature = "link"), allow(dead_code))]
    last_len: usize,
    repeat: Option<Repeat>,
    /// Reports a second: counted over the last whole second.
    sec_us: u64,
    sec_reports: u32,
    rps: u32,
    /// The "pad" log line: when next, and a sum of the last one logged.
    next_line_us: u64,
    last_line: u32,
}

/// The host's time: ms since start.
fn ms(us: u64) -> u64 {
    us / 1000
}

impl PadRadio {
    /// Starts the radio, logs what that cost, and hands the controller to
    /// pad-host, which sends its set-up commands at once (the rest of the
    /// set-up runs from [`PadRadio::poll`]). `gate` knows the saved pads
    /// (src/pairing.rs). Panics if the driver refuses to start.
    pub fn start(bt: esp_hal::peripherals::BT<'static>, gate: Gate, pads: &mut PadHost) -> Self {
        // The radio driver logs whole packets at the trace level, device
        // addresses and keys included: never more than info here.
        if log::max_level() > log::LevelFilter::Info {
            log::set_max_level(log::LevelFilter::Info);
        }
        let free_before = esp_alloc::HEAP.free();
        let t = crate::board::now_us();
        let port = HciPort::start(bt).expect("pad: radio driver refused to start");
        let us = crate::board::now_us() - t;
        let free_after = esp_alloc::HEAP.free();
        // SAFETY: a plain getter in the controller library, no arguments,
        // called after the controller started.
        let cmode = unsafe { btdm_controller_get_mode() };
        info!(
            "pad: radio up in {} ms, controller mode {cmode} ({}), heap free before {free_before} after {free_after}, radio took {}",
            us / 1000,
            if cmode == 3 { "dual" } else { "NOT dual" },
            free_before - free_after
        );
        let now = crate::board::now_us();
        let mut r = PadRadio {
            port,
            gate,
            queue: OutQueue::new(),
            counters: PadCounters {
                heap_min: free_after as u32,
                ..Default::default()
            },
            started_us: t,
            last_poll_us: now,
            next_tick_us: now + TICK_BUSY_US,
            last: [0; debug_link::pad::MAX_REPORT],
            last_len: 0,
            repeat: None,
            sec_us: now,
            sec_reports: 0,
            rps: 0,
            next_line_us: now + LINE_US,
            last_line: 0,
        };
        info!(
            "pad: host starting: reconnect only, saved pads={} (gate allows {}), page scan only after the set-up",
            pads.saved_pads(),
            r.gate.saved()
        );
        let mut out = Out {
            port: &mut r.port,
            gate: &r.gate,
            queue: &mut r.queue,
            c: &mut r.counters,
        };
        pads.start(ms(now), &mut out);
        r
    }

    /// One turn of the host: what waits for the controller goes out,
    /// every packet the controller queued is handed to pad-host, the
    /// injected repeat, the host's timers, its events to the log. Sets
    /// `pad.wake` when the console must look. Never waits. Call once per
    /// turn of the console's loop, before the input is looked at.
    pub fn poll(&mut self, now: u64, pad: &mut CydPad) {
        let Some(host) = pad.host.as_deref_mut() else {
            return;
        };
        self.queue
            .drain(&mut self.port, &self.gate, &mut self.counters);
        let mut busy = false;
        if self.port.has_data() {
            let free = esp_alloc::HEAP.free() as u32;
            self.counters.heap_min = self.counters.heap_min.min(free);
            let came = self.port.take_seen().unwrap_or(self.last_poll_us);
            self.counters.wait_max_us = self
                .counters
                .wait_max_us
                .max(now.saturating_sub(came) as u32);
            let mut n = 0;
            let mut buf = [0u8; MAX_PACKET];
            while let Some(p) = self.port.receive() {
                // A copy, so that the host may send while it reads it.
                let len = p.len();
                buf[..len].copy_from_slice(p);
                n += 1;
                // The gate notes a saved pad's link that comes up or ends
                // (its handle: the only one a supervision timeout may go
                // to), before the host answers.
                self.gate.observe(&buf[..len]);
                let mut out = Out {
                    port: &mut self.port,
                    gate: &self.gate,
                    queue: &mut self.queue,
                    c: &mut self.counters,
                };
                host.packet(ms(now), &buf[..len], &mut out);
            }
            // ACL data may hold a key exchange's leftovers: not kept.
            buf.fill(0);
            core::hint::black_box(&buf);
            self.counters.backlog_max = self.counters.backlog_max.max(n);
            busy = n > 0;
        }
        if let Some(r) = &mut self.repeat
            && now >= r.next_us
        {
            let mut out_bytes = [0u8; debug_link::pad::MAX_REPORT];
            let late = now - r.next_us;
            self.counters.wait_max_us = self.counters.wait_max_us.max(late as u32);
            // Every report that fell due since the last turn, as a real
            // pad's would wait in the driver's queue during a long frame
            // (at most REPEAT_BURST at once); due times stay on the grid.
            let due = late / r.period_us + 1;
            r.next_us += r.period_us * due;
            let mut out = Out {
                port: &mut self.port,
                gate: &self.gate,
                queue: &mut self.queue,
                c: &mut self.counters,
            };
            for _ in 0..due.min(REPEAT_BURST) {
                r.n += 1;
                let len = inject::resting(&r.base[..r.len], r.n, &mut out_bytes);
                if !host.inject_report(ms(now), &out_bytes[..len], &mut out) {
                    self.repeat = None;
                    break;
                }
            }
            busy = true;
        }
        if busy || now >= self.next_tick_us {
            let mut out = Out {
                port: &mut self.port,
                gate: &self.gate,
                queue: &mut self.queue,
                c: &mut self.counters,
            };
            if host.tick(ms(now), &mut out) {
                pad.wake = true;
                self.counters.wakes += 1;
            }
            self.next_tick_us = now
                + if host.waiting() {
                    TICK_IDLE_US
                } else {
                    TICK_BUSY_US
                };
            if self.counters.ready_us == 0 && host.waiting() {
                self.counters.ready_us = (now - self.started_us) as u32;
                // The set-up's answers waited while the panel was set up
                // (the loop was not running yet): said here, and not
                // counted as the loop's longest wait.
                info!(
                    "pad: waiting for a saved pad to call (page scan on: connectable, not discoverable); {} ms after the radio started; set-up answers waited up to {} us (start-up)",
                    self.counters.ready_us / 1000,
                    self.counters.wait_max_us
                );
                self.counters.wait_max_us = 0;
            }
        }
        while let Some(e) = host.take_event() {
            match e {
                PadEvent::Connected { mode, name } => info!(
                    "pad: connected: {} \"{}\"{}",
                    mode.word(),
                    name,
                    if host.injecting() { " (injected)" } else { "" }
                ),
                PadEvent::Disconnected { reason } => {
                    info!("pad: gone (reason {reason:#04x})");
                    if !host.injecting() {
                        self.repeat = None;
                    }
                }
                PadEvent::Battery(b) => info!(
                    "pad: battery {}%{}",
                    b.percent,
                    if b.charging { " charging" } else { "" }
                ),
            }
        }
        // Reports a second.
        let reports = host.counters().reports;
        if now >= self.sec_us + 1_000_000 {
            self.rps = reports.wrapping_sub(self.sec_reports);
            self.sec_reports = reports;
            self.sec_us = now;
        }
        self.last_poll_us = now;
        if now >= self.next_line_us {
            self.next_line_us = now + LINE_US;
            self.log_line(pad, false);
        }
    }

    /// When [`PadRadio::poll`] must run again although nothing came (us):
    /// the host's timers, the injected repeat, packets waiting for the
    /// controller (soon).
    pub fn due(&self) -> Option<u64> {
        let mut t = self.next_tick_us.min(self.next_line_us);
        if let Some(r) = &self.repeat {
            t = t.min(r.next_us);
        }
        if self.queue.n > 0 {
            t = t.min(self.last_poll_us + 1_000);
        }
        Some(t)
    }

    /// Waits until the controller has something. Safe to drop at any
    /// point (in a `select`).
    pub async fn wait(&mut self) {
        self.port.wait().await
    }

    /// An injected-pad request from the debug link: the link's status
    /// for its answer. The console looks at the next turn.
    #[cfg_attr(not(feature = "link"), allow(dead_code))]
    pub fn inject(&mut self, req: &PadRequest<'_>, pad: &mut CydPad) -> u8 {
        let Some(host) = pad.host.as_deref_mut() else {
            return status::UNSUPPORTED;
        };
        let now = crate::board::now_us();
        let mut out = Out {
            port: &mut self.port,
            gate: &self.gate,
            queue: &mut self.queue,
            c: &mut self.counters,
        };
        let ok = match *req {
            PadRequest::Connect { mode: m } => {
                let m = match m {
                    mode::SWITCH => Mode::Switch,
                    mode::XINPUT => Mode::XInput,
                    mode::ANDROID => Mode::Android,
                    _ => return status::BAD_REQUEST,
                };
                host.inject_begin(ms(now), m, &mut out)
            }
            PadRequest::Report { bytes } => {
                let ok = host.inject_report(ms(now), bytes, &mut out);
                if ok {
                    self.last[..bytes.len()].copy_from_slice(bytes);
                    self.last_len = bytes.len();
                    if let Some(r) = &mut self.repeat {
                        r.base = self.last;
                        r.len = self.last_len;
                    }
                }
                ok
            }
            PadRequest::Disconnect { reason } => {
                let was = host.injecting();
                host.inject_end(ms(now), reason, &mut out);
                self.repeat = None;
                was
            }
            PadRequest::Repeat { period_ms: 0 } => {
                self.repeat = None;
                true
            }
            PadRequest::Repeat { period_ms } => {
                if !host.injecting() || self.last_len == 0 {
                    false
                } else {
                    let period_us = u64::from(period_ms.max(REPEAT_MIN_MS)) * 1000;
                    self.repeat = Some(Repeat {
                        base: self.last,
                        len: self.last_len,
                        period_us,
                        next_us: now + period_us,
                        n: 0,
                    });
                    true
                }
            }
        };
        // The host's view at once (a press shorter than a frame is
        // latched here, as for a real report).
        if host.tick(ms(now), &mut out) {
            pad.wake = true;
            self.counters.wakes += 1;
        }
        if ok { status::OK } else { status::FAILED }
    }

    /// The counters back to zero (letter `r`).
    pub fn reset(&mut self, pad: &mut CydPad) {
        self.counters = PadCounters {
            heap_min: esp_alloc::HEAP.free() as u32,
            ready_us: self.counters.ready_us,
            ..Default::default()
        };
        self.port.counters = Default::default();
        if let Some(h) = pad.host.as_deref_mut() {
            h.reset_counters();
            self.sec_reports = 0;
        }
    }

    /// The "pad" line for `boardctl stats` (short: the stats frame holds
    /// 1 KB): state, mode, reports a second, dropped, the longest gap
    /// between reports (ms), the longest wait for the host (us), calls,
    /// failures by stage (accept/security/open/report), links lost,
    /// battery (%, + charging), injected reports, least heap free. With
    /// `full` (the log), also the reports, the calls that came to a
    /// report, wakes and the byte path's counters. No address, no key.
    #[cfg_attr(not(feature = "link"), allow(dead_code))]
    pub fn write_short(&self, pad: &CydPad, out: &mut impl core::fmt::Write) {
        self.write_line(pad, out, false);
    }

    fn write_line(&self, pad: &CydPad, out: &mut impl core::fmt::Write, full: bool) {
        let Some(h) = pad.host.as_deref() else {
            let _ = writeln!(out, "pad state=none");
            return;
        };
        let c = h.counters();
        let p = &self.counters;
        let b = &self.port.counters;
        let _ = write!(
            out,
            "pad state={} mode={} rps={} drop={} gap_ms={} wait_us={} tries={} fail={}/{}/{}/{} lost={} bat=",
            h.state(),
            if h.connected() { h.mode().word() } else { "-" },
            self.rps,
            p.out_dropped + b.in_bad + c.events_lost,
            c.gap_max_ms,
            p.wait_max_us,
            c.attempts,
            c.failed[0],
            c.failed[1],
            c.failed[2],
            c.failed[3],
            c.lost,
        );
        match h.battery() {
            Some(bat) if h.connected() => {
                let _ = write!(
                    out,
                    "{}{}",
                    bat.percent,
                    if bat.charging { "+" } else { "" }
                );
            }
            _ => {
                let _ = write!(out, "-");
            }
        }
        let _ = write!(out, " inj={} heap_min={}", c.injected, p.heap_min);
        if full {
            let _ = write!(
                out,
                " ok={} silent={} gate_links={} reports={} undecoded={} wakes={} in={}/{} out={} queued={}/{} refused={} ready_ms={}",
                c.ready,
                c.silent,
                self.gate.linked(),
                c.reports,
                c.undecoded,
                p.wakes,
                b.in_events,
                b.in_acl,
                b.out_packets,
                p.queued,
                p.queue_max,
                p.refused + b.out_refused,
                p.ready_us / 1000,
            );
        }
        let _ = writeln!(out);
    }

    /// The "pad" line to the log: `always` (letter `m`, every 30 s), or
    /// when it changed since it was last logged.
    pub fn log_line(&mut self, pad: &CydPad, always: bool) {
        let mut text = [0u8; 320];
        let mut out = debug_link::TextBuf::new(&mut text);
        self.write_line(pad, &mut out, true);
        let line = core::str::from_utf8(out.as_bytes())
            .unwrap_or("")
            .trim_end();
        let sum = settings_store::crc32_more(0, line.as_bytes());
        if always || sum != self.last_line {
            self.last_line = sum;
            info!("{line}");
        }
    }
}
