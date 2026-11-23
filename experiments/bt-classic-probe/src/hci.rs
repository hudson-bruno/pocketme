//! Raw HCI: talking to the radio's controller directly, with no Bluetooth
//! library in between.
//!
//! "HCI" is the standard set of commands and events between a Bluetooth
//! host (our program) and a Bluetooth controller (the radio's own firmware,
//! in the chip's ROM and in Espressif's library). Every command here is
//! built by hand, byte by byte, from the Bluetooth Core specification, and
//! every answer is read the same way. That keeps the probe small and shows
//! exactly what the controller says.
//!
//! Rules kept here:
//! * One command at a time. The next is sent only once the controller has
//!   answered the last one (with "Command Complete" or "Command Status").
//! * Everything that arrives meanwhile (inquiry results, names, Low Energy
//!   reports) is handled by `handle`, which only counts, stores and prints.

use core::fmt::Write as _;

use bt_hci::transport::Transport as _;
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Timer};
use esp_radio::ble::controller::BleConnector;
use heapless::{String, Vec};

use crate::say;

/// Room for the largest packet the controller can hand us. An event is at
/// most 258 bytes; a classic data (ACL) packet can be longer. esp-radio
/// copies each packet in whole and does not check the room, so this must
/// be large enough for anything.
pub const RX_LEN: usize = 1100;

/// How many different classic devices the table keeps.
const DEVICES_MAX: usize = 32;

/// How long a command may take before we call it lost.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);

/// The longest wait for a name. The controller itself gives up after the
/// "page timeout", 5.12 s by default.
const NAME_TIMEOUT: Duration = Duration::from_secs(12);

/// The opcodes used, as named constants. An opcode is a 16-bit number:
/// group (OGF) in the top 6 bits, command (OCF) in the low 10 bits.
pub mod op {
    // Link control (OGF 0x01): talking to other devices.
    pub const INQUIRY: u16 = 0x0401;
    pub const INQUIRY_CANCEL: u16 = 0x0402;
    pub const CREATE_CONNECTION: u16 = 0x0405;
    pub const DISCONNECT: u16 = 0x0406;
    pub const REJECT_CONNECTION: u16 = 0x040A;
    pub const REMOTE_NAME: u16 = 0x0419;
    pub const REMOTE_NAME_CANCEL: u16 = 0x041A;
    // Controller and baseband (OGF 0x03): settings of our own radio.
    pub const SET_EVENT_MASK: u16 = 0x0C01;
    pub const RESET: u16 = 0x0C03;
    pub const WRITE_LOCAL_NAME: u16 = 0x0C13;
    pub const READ_SCAN_ENABLE: u16 = 0x0C19;
    pub const WRITE_SCAN_ENABLE: u16 = 0x0C1A;
    pub const WRITE_CLASS_OF_DEVICE: u16 = 0x0C24;
    pub const READ_INQUIRY_MODE: u16 = 0x0C44;
    pub const WRITE_INQUIRY_MODE: u16 = 0x0C45;
    pub const WRITE_EIR: u16 = 0x0C52;
    pub const WRITE_LE_HOST_SUPPORT: u16 = 0x0C6D;
    // Information (OGF 0x04): what the controller is and can do.
    pub const READ_LOCAL_VERSION: u16 = 0x1001;
    pub const READ_LOCAL_COMMANDS: u16 = 0x1002;
    pub const READ_LOCAL_FEATURES: u16 = 0x1003;
    pub const READ_LOCAL_EXT_FEATURES: u16 = 0x1004;
    pub const READ_BUFFER_SIZE: u16 = 0x1005;
    pub const READ_BD_ADDR: u16 = 0x1009;
    // Low Energy (OGF 0x08).
    pub const LE_SET_EVENT_MASK: u16 = 0x2001;
    pub const LE_READ_BUFFER_SIZE: u16 = 0x2002;
    pub const LE_SET_ADV_PARAMS: u16 = 0x2006;
    pub const LE_SET_ADV_DATA: u16 = 0x2008;
    pub const LE_SET_ADV_ENABLE: u16 = 0x200A;
    pub const LE_SET_SCAN_PARAMS: u16 = 0x200B;
    pub const LE_SET_SCAN_ENABLE: u16 = 0x200C;
}

/// The controller's answer to one command.
pub struct Answer {
    /// 0x00 = done. Anything else is an error code from the specification
    /// (0x01 = unknown command, 0x0C = not allowed now, 0x11 = not
    /// supported, 0x12 = bad parameters).
    pub status: u8,
    /// The "return parameters" of a Command Complete, after the status
    /// byte. Empty for a Command Status.
    pub ret: Vec<u8, 255>,
}

/// One classic device heard during an inquiry.
#[derive(Clone)]
pub struct Dev {
    pub addr: [u8; 6],
    /// Class of device: what kind of thing it says it is (24 bits).
    pub cod: u32,
    pub rssi: i8,
    pub best_rssi: i8,
    /// Two values the name request needs to reach the device quickly.
    pub psrm: u8,
    pub clock_offset: u16,
    /// Answers heard, over all rounds.
    pub hits: u32,
    /// The last round it answered in.
    pub round: u32,
    pub name: String<48>,
    /// 0 = no name yet, 1 = from its inquiry answer (EIR), 2 = asked for.
    pub name_from: u8,
    pub name_tries: u8,
    /// Its inquiry answer lists the classic HID service (UUID 0x1124).
    pub hid: bool,
    /// Its Device ID record: vendor and product number.
    pub device_id: Option<(u16, u16)>,
}

impl Dev {
    /// Major class 0x05 is "peripheral" (keyboards, mice, game controllers).
    /// Within it, the low four bits of the minor class say which: 0x01
    /// joystick, 0x02 gamepad. The GuliKit pad says 0x002508 or 0x000508.
    pub fn gamepad_like(&self) -> bool {
        let major = (self.cod >> 8) & 0x1F;
        let minor = (self.cod >> 2) & 0x3F;
        major == 0x05 && matches!(minor & 0x0F, 0x01 | 0x02)
    }
}

/// Counts of everything that arrived.
#[derive(Default, Clone, Copy)]
pub struct Counts {
    pub inquiry_result: u32,
    pub inquiry_result_rssi: u32,
    pub inquiry_result_ext: u32,
    pub connection_requests: u32,
    pub hardware_errors: u32,
    pub vendor_events: u32,
    pub other_events: u32,
    pub acl_packets: u32,
    pub le_adv_reports: u32,
    pub broken: u32,
}

/// Everything `handle` fills in. Kept apart from the receive buffer, so
/// that both can be used at once.
pub struct State {
    /// The opcode of the command we wait for an answer to.
    waiting: Option<u16>,
    answer: Option<Answer>,
    pub inquiry_done: Option<u8>,
    pub name_done: Option<([u8; 6], u8)>,
    /// Status and link number ("handle") of the last Connection Complete
    /// event.
    pub conn_done: Option<(u8, u16)>,
    /// Set when a Disconnection Complete event arrives.
    pub disconnected: bool,
    pub pending_reject: Option<[u8; 6]>,
    pub devices: Vec<Dev, DEVICES_MAX>,
    pub not_stored: u32,
    /// Number of the inquiry round now running (1, 2, ...).
    pub round: u32,
    /// Devices that answered in this round.
    pub round_unique: u32,
    pub round_gamepads: u32,
    pub counts: Counts,
    /// "Num_HCI_Command_Packets" from the last answer: how many commands
    /// the controller would take at once.
    pub cmd_credits: u8,
    /// Low Energy advertisers with a public (fixed, factory) address. Many
    /// devices that speak both kinds use the same address for classic, so
    /// these are something to page for a classic test (see `page_test`).
    pub le_public: Vec<[u8; 6], 4>,
}

pub struct Hci {
    conn: BleConnector<'static>,
    rx: &'static mut [u8; RX_LEN],
    pub st: State,
    next_mem: Instant,
}

impl Hci {
    pub fn new(conn: BleConnector<'static>, rx: &'static mut [u8; RX_LEN]) -> Self {
        Self {
            conn,
            rx,
            st: State {
                waiting: None,
                answer: None,
                inquiry_done: None,
                name_done: None,
                conn_done: None,
                disconnected: false,
                pending_reject: None,
                devices: Vec::new(),
                not_stored: 0,
                round: 0,
                round_unique: 0,
                round_gamepads: 0,
                counts: Counts::default(),
                cmd_credits: 0,
                le_public: Vec::new(),
            },
            next_mem: Instant::now(),
        }
    }

    /// Waits for one packet from the controller, until `until` at most,
    /// and handles it. Prints the memory line once a second meanwhile.
    /// Returns false if the time ran out first.
    pub async fn pump(&mut self, until: Instant) -> bool {
        loop {
            let wake = if self.next_mem < until { self.next_mem } else { until };
            // The packet is copied into `rx`; only "did a packet come" is
            // kept from the read itself, so `rx` is free again below.
            let got = match select(self.conn.read(&mut self.rx[..]), Timer::at(wake)).await {
                Either::First(parsed) => Some(parsed.is_ok()),
                Either::Second(()) => None,
            };
            match got {
                Some(parsed) => {
                    if !parsed {
                        self.st.counts.broken += 1;
                    }
                    let len = packet_len(&self.rx[..]);
                    handle(&mut self.st, &self.rx[..len]).await;
                    return true;
                }
                None => {
                    let now = Instant::now();
                    if now >= self.next_mem {
                        crate::report_mem().await;
                        self.next_mem += Duration::from_secs(1);
                        if self.next_mem < now {
                            self.next_mem = now + Duration::from_secs(1);
                        }
                    }
                    if now >= until {
                        return false;
                    }
                }
            }
        }
    }

    /// Handles whatever arrives for `time`.
    pub async fn idle(&mut self, time: Duration) {
        let until = Instant::now() + time;
        while Instant::now() < until {
            self.pump(until).await;
            self.service().await;
        }
    }

    /// Sends one command and waits for its answer. Prints a line `HCI ...`
    /// with the answer, or `ERROR ...` if none came.
    pub async fn cmd(&mut self, name: &str, opcode: u16, params: &[u8]) -> Option<Answer> {
        let mut buf = [0_u8; 259];
        let total = 4 + params.len();
        buf[0] = 0x01; // packet type: command
        buf[1..3].copy_from_slice(&opcode.to_le_bytes());
        buf[3] = params.len() as u8;
        buf[4..total].copy_from_slice(params);

        self.st.waiting = Some(opcode);
        self.st.answer = None;
        let started = Instant::now();
        let mut sent = 0;
        while sent < total {
            match self.conn.write_async(&buf[sent..total]).await {
                Ok(0) | Err(_) => {
                    say!("ERROR cmd={} op={:#06x} write_failed_at={}", name, opcode, sent);
                    self.st.waiting = None;
                    return None;
                }
                Ok(n) => sent += n,
            }
        }

        let until = started + COMMAND_TIMEOUT;
        while self.st.answer.is_none() {
            if !self.pump(until).await {
                break;
            }
        }
        self.st.waiting = None;
        let ms = started.elapsed().as_millis();
        match self.st.answer.take() {
            Some(answer) => {
                let mut ret: String<300> = String::new();
                for b in &answer.ret {
                    let _ = write!(ret, "{b:02x}");
                }
                say!(
                    "HCI cmd={} op={:#06x} status={:#04x} ({}) ms={} ret={}",
                    name,
                    opcode,
                    answer.status,
                    status_text(answer.status),
                    ms,
                    if ret.is_empty() { "-" } else { ret.as_str() }
                );
                Some(answer)
            }
            None => {
                say!("ERROR cmd={} op={:#06x} no_answer_ms={}", name, opcode, ms);
                None
            }
        }
    }

    /// Like `cmd`, but only says whether the controller said "done".
    pub async fn ok(&mut self, name: &str, opcode: u16, params: &[u8]) -> bool {
        matches!(self.cmd(name, opcode, params).await, Some(Answer { status: 0, .. }))
    }

    /// Answers a phone that tried to connect: "no" (reason 0x0D, "limited
    /// resources"). Without this, the controller says no by itself after
    /// about 5 s.
    pub async fn service(&mut self) {
        if let Some(addr) = self.st.pending_reject.take() {
            let mut p = [0_u8; 7];
            p[..6].copy_from_slice(&addr);
            p[6] = 0x0D;
            self.cmd("reject_connection", op::REJECT_CONNECTION, &p).await;
        }
    }

    /// One general inquiry: about `units` x 1.28 s of asking "who is there?"
    /// on the classic channels. Returns the inquiry's end status, or None.
    pub async fn inquiry(&mut self, units: u8) -> Option<u8> {
        self.st.round += 1;
        self.st.round_unique = 0;
        self.st.round_gamepads = 0;
        self.st.inquiry_done = None;
        let before = self.st.counts;
        let started = Instant::now();
        say!("INQ start round={} length_units={} secs_max={}", self.st.round, units, u32::from(units) * 128 / 100);
        // GIAC, the "general" access code every discoverable device answers
        // to: 0x9E8B33, sent low byte first. 0 = no limit on answers.
        let ok = self.ok("inquiry", op::INQUIRY, &[0x33, 0x8B, 0x9E, units, 0x00]).await;
        if !ok {
            say!("ERROR inquiry refused round={}", self.st.round);
            return None;
        }
        let until = started + Duration::from_millis(u64::from(units) * 1280 + 5000);
        while self.st.inquiry_done.is_none() {
            if !self.pump(until).await {
                break;
            }
            self.service().await;
        }
        let status = self.st.inquiry_done;
        if status.is_none() {
            say!("ERROR inquiry no_complete_event round={} ms={}", self.st.round, started.elapsed().as_millis());
            self.cmd("inquiry_cancel", op::INQUIRY_CANCEL, &[]).await;
        }
        let c = self.st.counts;
        say!(
            "INQ done round={} status={} ms={} ev02={} ev22={} ev2f={} unique={} gamepads={} known={} not_stored={}",
            self.st.round,
            match status {
                Some(s) => s as i32,
                None => -1,
            },
            started.elapsed().as_millis(),
            c.inquiry_result - before.inquiry_result,
            c.inquiry_result_rssi - before.inquiry_result_rssi,
            c.inquiry_result_ext - before.inquiry_result_ext,
            self.st.round_unique,
            self.st.round_gamepads,
            self.st.devices.len(),
            self.st.not_stored,
        );
        status
    }

    /// Asks for the name of each device of this round that has none yet,
    /// at most `max` of them and at most twice per device over all rounds.
    /// A name request calls the device up briefly (it "pages" it), so a
    /// device that put its name in its inquiry answer is not asked again.
    pub async fn names(&mut self, max: usize) {
        let mut asked = 0;
        let mut i = 0;
        while i < self.st.devices.len() && asked < max {
            let d = self.st.devices[i].clone();
            i += 1;
            // A gamepad-like device is asked once even when its inquiry
            // answer held its name: a name request is a real call-up
            // (page) and a short exchange with it, which proves more than
            // an inquiry does.
            let confirm = d.gamepad_like() && d.name_from == 1 && d.name_tries == 0;
            if d.round != self.st.round || (d.name_from != 0 && !confirm) || d.name_tries >= 2 {
                continue;
            }
            asked += 1;
            self.st.devices[i - 1].name_tries += 1;
            let mut p = [0_u8; 10];
            p[..6].copy_from_slice(&d.addr);
            p[6] = d.psrm;
            p[7] = 0;
            // Bit 15 says "this clock offset is valid".
            p[8..10].copy_from_slice(&(d.clock_offset | 0x8000).to_le_bytes());
            self.st.name_done = None;
            let started = Instant::now();
            if !self.ok("remote_name", op::REMOTE_NAME, &p).await {
                continue;
            }
            let until = started + NAME_TIMEOUT;
            loop {
                if matches!(self.st.name_done, Some((a, _)) if a == d.addr) {
                    break;
                }
                if !self.pump(until).await {
                    break;
                }
            }
            if matches!(self.st.name_done, Some((a, _)) if a == d.addr) {
                say!("NAME addr={} request_ms={}", Addr(&d.addr), started.elapsed().as_millis());
            } else {
                say!("NAME addr={} result=timeout ms={}", Addr(&d.addr), started.elapsed().as_millis());
                self.cmd("remote_name_cancel", op::REMOTE_NAME_CANCEL, &d.addr).await;
                self.idle(Duration::from_millis(500)).await;
            }
        }
    }
}

impl Hci {
    /// A classic test that needs no device in pairing mode: ask, over
    /// classic Bluetooth, for the name of each Low Energy device seen with
    /// a public address. A reply proves the classic radio both sends (it
    /// "pages" the device) and receives. No reply proves nothing: the device
    /// may simply not speak classic.
    pub async fn page_test(&mut self) {
        // First a made-up address that no device has (01:23:45:67:89:AB).
        // A controller that can page sends "taken" (status 0), then gives
        // up after the page timeout (5.12 s) with status 0x04. That shows
        // the classic paging machinery runs, without any other device.
        let nobody = [0xAB, 0x89, 0x67, 0x45, 0x23, 0x01];
        let started = Instant::now();
        let mut p = [0_u8; 10];
        p[..6].copy_from_slice(&nobody);
        p[6] = 0x01;
        self.st.name_done = None;
        let taken = self.cmd("remote_name_nobody", op::REMOTE_NAME, &p).await.map(|a| a.status);
        let until = started + NAME_TIMEOUT;
        if taken == Some(0) {
            while self.st.name_done.is_none() {
                if !self.pump(until).await {
                    break;
                }
            }
        }
        say!(
            "PAGETEST nobody taken={} end_status={} ms={} (expected: taken=0x00, end 0x04 page-timeout after ~5120 ms)",
            taken.map_or(-1, i32::from),
            self.st.name_done.map_or(-1, |(_, s)| i32::from(s)),
            started.elapsed().as_millis()
        );

        // Added by the second check: a real connection attempt (Create
        // Connection) to the same made-up address. Measured: the LE build
        // takes it and pages for 5.12 s exactly like the dual build, so
        // this alone does not tell the two apart either.
        self.connect_test("nobody", nobody).await;

        // Optional, only when the firmware was built with PROBE_PEER set
        // to the address of a classic device of the owner's that is
        // switched on (a phone with Bluetooth on is enough; it need not be
        // visible). A real link (Connection Complete status 0) is the one
        // result that shows the controller can hold a classic link. The
        // address is never written in the source.
        if let Some(text) = option_env!("PROBE_PEER")
            && let Some(peer) = parse_addr(text)
        {
            let mut p = [0_u8; 10];
            p[..6].copy_from_slice(&peer);
            p[6] = 0x01;
            self.st.name_done = None;
            let started = Instant::now();
            if self.ok("remote_name_peer", op::REMOTE_NAME, &p).await {
                let until = started + NAME_TIMEOUT;
                while self.st.name_done.is_none() {
                    if !self.pump(until).await {
                        break;
                    }
                }
            }
            say!(
                "PEERTEST name end_status={} ms={}",
                self.st.name_done.map_or(-1, |(_, s)| i32::from(s)),
                started.elapsed().as_millis()
            );
            self.connect_test("peer", peer).await;
        }

        let targets = self.st.le_public.clone();
        let mut ok = 0;
        for addr in &targets {
            let mut p = [0_u8; 10];
            p[..6].copy_from_slice(addr);
            p[6] = 0x01; // page scan mode R1, the common one
            self.st.name_done = None;
            let started = Instant::now();
            if !self.ok("remote_name_le_public", op::REMOTE_NAME, &p).await {
                continue;
            }
            let until = started + NAME_TIMEOUT;
            while !matches!(self.st.name_done, Some((a, _)) if a == *addr) {
                if !self.pump(until).await {
                    break;
                }
            }
            match self.st.name_done {
                Some((a, 0)) if a == *addr => ok += 1,
                Some((a, _)) if a == *addr => {}
                _ => {
                    say!("NAME addr={} result=timeout ms={}", Addr(addr), started.elapsed().as_millis());
                    self.cmd("remote_name_cancel", op::REMOTE_NAME_CANCEL, addr).await;
                    self.idle(Duration::from_millis(500)).await;
                }
            }
        }
        say!("PAGETEST le_public_devices={} classic_name_ok={}", targets.len(), ok);
    }

    /// Create Connection to `addr`, wait for Connection Complete, and if a
    /// link came up, keep it 3 s and close it again. Prints one `CONNTEST`
    /// line (no address).
    async fn connect_test(&mut self, who: &str, addr: [u8; 6]) {
        let mut p = [0_u8; 13];
        p[..6].copy_from_slice(&addr);
        // Packet types DM1, DH1, DM3, DH3, DM5, DH5 (0xCC18).
        p[6..8].copy_from_slice(&0xCC18_u16.to_le_bytes());
        p[8] = 0x01; // page scan mode R1
        p[9] = 0x00; // reserved
        // p[10..12]: clock offset 0, not valid.
        p[12] = 0x01; // the other side may become the central
        self.st.conn_done = None;
        self.st.disconnected = false;
        let started = Instant::now();
        let taken = self.cmd("create_connection", op::CREATE_CONNECTION, &p).await.map(|a| a.status);
        let until = started + NAME_TIMEOUT;
        if taken == Some(0) {
            while self.st.conn_done.is_none() {
                if !self.pump(until).await {
                    break;
                }
            }
        }
        let ms = started.elapsed().as_millis();
        let mut held = false;
        if let Some((0, handle)) = self.st.conn_done {
            // A link is up. Keep it a moment (the other side may talk),
            // then hang up: reason 0x13, "the user ended it".
            self.idle(Duration::from_secs(3)).await;
            held = !self.st.disconnected;
            let mut d = [0_u8; 3];
            d[..2].copy_from_slice(&handle.to_le_bytes());
            d[2] = 0x13;
            if !self.st.disconnected && self.ok("disconnect", op::DISCONNECT, &d).await {
                let until = Instant::now() + COMMAND_TIMEOUT;
                while !self.st.disconnected {
                    if !self.pump(until).await {
                        break;
                    }
                }
            }
        }
        say!(
            "CONNTEST {} taken={} end_status={} ms={} link_held_3s={} disconnected={} (made-up address: expect taken=0, end 4 = page timeout after ~5120 ms)",
            who,
            taken.map_or(-1, i32::from),
            self.st.conn_done.map_or(-1, |(s, _)| i32::from(s)),
            ms,
            u8::from(held),
            u8::from(self.st.disconnected)
        );
    }
}

/// "AA:BB:CC:DD:EE:FF" to the order the controller wants (low byte first).
fn parse_addr(text: &str) -> Option<[u8; 6]> {
    let mut out = [0_u8; 6];
    let mut parts = text.split(':');
    for i in (0..6).rev() {
        out[i] = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    parts.next().is_none().then_some(out)
}

/// The length of the packet at the start of `p` (type byte included).
fn packet_len(p: &[u8]) -> usize {
    let len = match p[0] {
        0x04 => 3 + usize::from(p[2]),
        0x02 => 5 + usize::from(u16::from_le_bytes([p[3], p[4]])),
        _ => 1,
    };
    len.min(p.len())
}

/// Handles one packet from the controller.
async fn handle(st: &mut State, p: &[u8]) {
    match p.first() {
        Some(0x04) if p.len() >= 3 => event(st, p[1], &p[3..]).await,
        Some(0x02) => {
            st.counts.acl_packets += 1;
            say!("ACL len={}", p.len());
        }
        _ => {
            st.counts.broken += 1;
            say!("ERROR packet kind={:02x?} len={}", p.first(), p.len());
        }
    }
}

/// Handles one event. `e` is the event's parameters.
async fn event(st: &mut State, code: u8, e: &[u8]) {
    match code {
        // Command Complete: credits, opcode, then the return parameters,
        // which begin with the status (for nearly every command).
        0x0E if e.len() >= 3 => {
            st.cmd_credits = e[0];
            let opcode = u16::from_le_bytes([e[1], e[2]]);
            if st.waiting == Some(opcode) {
                let status = e.get(3).copied().unwrap_or(0);
                let ret = Vec::from_slice(e.get(4..).unwrap_or(&[])).unwrap_or_default();
                st.answer = Some(Answer { status, ret });
            } else if opcode != 0 {
                say!("HCI unexpected_complete op={:#06x}", opcode);
            }
        }
        // Command Status: the command was taken (status 0) and will finish
        // later with an event of its own, or it was refused.
        0x0F if e.len() >= 4 => {
            st.cmd_credits = e[1];
            let opcode = u16::from_le_bytes([e[2], e[3]]);
            if st.waiting == Some(opcode) {
                st.answer = Some(Answer { status: e[0], ret: Vec::new() });
            } else if opcode != 0 {
                say!("HCI unexpected_status op={:#06x} status={:#04x}", opcode, e[0]);
            }
        }
        // Inquiry Complete.
        0x01 => st.inquiry_done = Some(e.first().copied().unwrap_or(0xFF)),
        // Inquiry Result (old form, no signal strength). The specification
        // puts several answers as one list per field.
        0x02 if !e.is_empty() => {
            st.counts.inquiry_result += 1;
            let n = usize::from(e[0]);
            if e.len() < 1 + n * 14 {
                st.counts.broken += 1;
                return;
            }
            for i in 0..n {
                let addr = take6(&e[1 + 6 * i..]);
                let psrm = e[1 + 6 * n + i];
                let cod = cod_at(&e[1 + 6 * n + 3 * n + 3 * i..]);
                let clk = u16::from_le_bytes([e[1 + 12 * n + 2 * i], e[2 + 12 * n + 2 * i]]);
                found(st, "02", addr, psrm, cod, clk, 0, &[]).await;
            }
        }
        // Inquiry Result with RSSI: as above, plus signal strength.
        0x22 if !e.is_empty() => {
            st.counts.inquiry_result_rssi += 1;
            let n = usize::from(e[0]);
            if e.len() < 1 + n * 14 {
                st.counts.broken += 1;
                return;
            }
            for i in 0..n {
                let addr = take6(&e[1 + 6 * i..]);
                let psrm = e[1 + 6 * n + i];
                let cod = cod_at(&e[1 + 8 * n + 3 * i..]);
                let clk = u16::from_le_bytes([e[1 + 11 * n + 2 * i], e[2 + 11 * n + 2 * i]]);
                let rssi = e[1 + 13 * n + i] as i8;
                found(st, "22", addr, psrm, cod, clk, rssi, &[]).await;
            }
        }
        // Extended Inquiry Result: one answer, with up to 240 bytes of
        // "extended inquiry response" data (name, services, ...).
        0x2F if e.len() >= 15 => {
            st.counts.inquiry_result_ext += 1;
            let addr = take6(&e[1..]);
            let psrm = e[7];
            let cod = cod_at(&e[9..]);
            let clk = u16::from_le_bytes([e[12], e[13]]);
            let rssi = e[14] as i8;
            found(st, "2f", addr, psrm, cod, clk, rssi, &e[15..]).await;
        }
        // Remote Name Request Complete: status, address, name (up to 248
        // bytes, ends at the first zero byte).
        0x07 if e.len() >= 7 => {
            let addr = take6(&e[1..]);
            let status = e[0];
            st.name_done = Some((addr, status));
            let name = e.get(7..).unwrap_or(&[]);
            let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
            let name = &name[..end];
            let mut gamepad = false;
            if let Some(d) = st.devices.iter_mut().find(|d| d.addr == addr) {
                if status == 0 {
                    d.name = clean(name);
                    d.name_from = 2;
                }
                gamepad = d.gamepad_like();
            }
            if status == 0 {
                say!("NAME addr={} result=ok from=request name=\"{}\"{}", Addr(&addr), clean::<64>(name), if gamepad { " GAMEPAD-LIKE" } else { "" });
            } else {
                say!("NAME addr={} result=failed status={:#04x} ({})", Addr(&addr), status, status_text(status));
            }
        }
        // Connection Request: someone (a phone?) wants to connect to us.
        0x04 if e.len() >= 10 => {
            st.counts.connection_requests += 1;
            let addr = take6(e);
            say!("CONN request addr={} cod={:#08x} link_type={}", Addr(&addr), cod_at(&e[6..]), e[9]);
            st.pending_reject = Some(addr);
        }
        0x03 if e.len() >= 11 => {
            st.conn_done = Some((e[0], u16::from_le_bytes([e[1], e[2]]) & 0x0FFF));
            say!("CONN complete status={:#04x} ({}) addr={} link_type={}", e[0], status_text(e[0]), Addr(&take6(&e[3..])), e[9]);
        }
        0x05 if e.len() >= 4 => {
            st.disconnected = true;
            say!("CONN disconnect status={:#04x} reason={:#04x}", e[0], e[3]);
        }
        // LE Meta event; sub-event 0x02 = advertising reports. Counted only.
        0x3E if !e.is_empty() => {
            if e[0] == 0x02 {
                let n = e.get(1).copied().unwrap_or(0);
                st.counts.le_adv_reports += u32::from(n);
                // One report: kind, address type (0 = public), address.
                if n == 1 && e.len() >= 9 && e[3] == 0x00 {
                    let addr = take6(&e[4..]);
                    if !st.le_public.contains(&addr) {
                        let _ = st.le_public.push(addr);
                    }
                }
            } else {
                st.counts.other_events += 1;
                say!("EVT le_meta sub={:#04x} len={}", e[0], e.len());
            }
        }
        // Number of Completed Packets: only matters once data flows.
        0x13 => {}
        0x10 => {
            st.counts.hardware_errors += 1;
            say!("ERROR hardware_error code={:02x?}", e.first());
        }
        0xFF => {
            st.counts.vendor_events += 1;
            say!("EVT vendor len={} data={:02x?}", e.len(), &e[..e.len().min(16)]);
        }
        _ => {
            st.counts.other_events += 1;
            say!("EVT code={:#04x} len={} data={:02x?}", code, e.len(), &e[..e.len().min(16)]);
        }
    }
}

/// Puts one inquiry answer in the table, and prints `FOUND` the first time
/// a device answers in a round.
#[allow(clippy::too_many_arguments)]
async fn found(st: &mut State, ev: &str, addr: [u8; 6], psrm: u8, cod: u32, clk: u16, rssi: i8, eir: &[u8]) {
    let round = st.round;
    let info = Eir::parse(eir);
    let idx = match st.devices.iter().position(|d| d.addr == addr) {
        Some(i) => Some(i),
        None => {
            let d = Dev {
                addr,
                cod,
                rssi,
                best_rssi: rssi,
                psrm,
                clock_offset: clk,
                hits: 0,
                round: 0,
                name: String::new(),
                name_from: 0,
                name_tries: 0,
                hid: false,
                device_id: None,
            };
            match st.devices.push(d) {
                Ok(()) => Some(st.devices.len() - 1),
                Err(_) => {
                    st.not_stored += 1;
                    None
                }
            }
        }
    };
    let Some(i) = idx else {
        say!("FOUND round={} new=? addr={} cod={:#08x} rssi={} ev={} stored=no", round, Addr(&addr), cod, rssi, ev);
        return;
    };
    let d = &mut st.devices[i];
    let first_ever = d.hits == 0;
    d.hits += 1;
    d.cod = cod;
    d.rssi = rssi;
    if first_ever || rssi > d.best_rssi {
        d.best_rssi = rssi;
    }
    d.psrm = psrm;
    d.clock_offset = clk;
    d.hid |= info.hid;
    if info.device_id.is_some() {
        d.device_id = info.device_id;
    }
    if !info.name.is_empty() {
        d.name = info.name.clone();
        d.name_from = 1;
    }
    if d.round == round {
        return;
    }
    d.round = round;
    let d = d.clone();
    st.round_unique += 1;
    if d.gamepad_like() {
        st.round_gamepads += 1;
    }
    let major = (cod >> 8) & 0x1F;
    let minor = (cod >> 2) & 0x3F;
    let mut extra: String<120> = String::new();
    if let Some((vendor, product)) = d.device_id {
        let _ = write!(extra, " device_id={vendor:04x}:{product:04x}");
    }
    if d.name_from == 1 {
        let _ = write!(extra, " eir_name=\"{}\"", d.name);
    }
    say!(
        "FOUND round={} new={} addr={} cod={:#08x} major={:#04x}({}) minor={:#04x} rssi={} ev={} hid_uuid={} eir_len={}{}{}",
        round,
        u8::from(first_ever),
        Addr(&addr),
        cod,
        major,
        major_text(major),
        minor,
        rssi,
        ev,
        u8::from(d.hid),
        eir.iter().rposition(|&b| b != 0).map_or(0, |n| n + 1),
        extra,
        if d.gamepad_like() { " GAMEPAD-LIKE" } else { "" },
    );
}

/// What an extended inquiry answer says, of the parts we look at.
struct Eir {
    name: String<48>,
    hid: bool,
    device_id: Option<(u16, u16)>,
}

impl Eir {
    /// The data is a row of pieces: length, type, then (length - 1) bytes.
    /// A length of zero ends it.
    fn parse(mut d: &[u8]) -> Self {
        let mut out = Eir { name: String::new(), hid: false, device_id: None };
        while d.len() >= 2 {
            let len = usize::from(d[0]);
            if len == 0 || len + 1 > d.len() {
                break;
            }
            let (kind, body) = (d[1], &d[2..=len]);
            match kind {
                // Short or complete name.
                0x08 | 0x09 => out.name = clean(body),
                // Lists of 16-bit service numbers; 0x1124 = classic HID.
                0x02 | 0x03 => out.hid |= body.chunks_exact(2).any(|u| u16::from_le_bytes([u[0], u[1]]) == 0x1124),
                // Device ID: source, vendor, product, version.
                0x10 if body.len() >= 6 => {
                    out.device_id = Some((u16::from_le_bytes([body[2], body[3]]), u16::from_le_bytes([body[4], body[5]])));
                }
                _ => {}
            }
            d = &d[len + 1..];
        }
        out
    }
}

/// A name made safe for a log line: printable ASCII only, quotes and
/// backslashes replaced, cut to the room given.
fn clean<const N: usize>(raw: &[u8]) -> String<N> {
    let mut out = String::new();
    for &b in raw {
        let c = if (0x20..0x7F).contains(&b) && b != b'"' && b != b'\\' { b as char } else { '?' };
        if out.push(c).is_err() {
            break;
        }
    }
    out
}

fn take6(p: &[u8]) -> [u8; 6] {
    let mut a = [0; 6];
    a.copy_from_slice(&p[..6]);
    a
}

fn cod_at(p: &[u8]) -> u32 {
    u32::from(p[0]) | u32::from(p[1]) << 8 | u32::from(p[2]) << 16
}

/// A Bluetooth address, printed the usual way (most significant byte
/// first; it travels least significant first).
pub struct Addr<'a>(pub &'a [u8; 6]);

impl core::fmt::Display for Addr<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let a = self.0;
        write!(f, "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", a[5], a[4], a[3], a[2], a[1], a[0])
    }
}

pub fn status_text(status: u8) -> &'static str {
    match status {
        0x00 => "ok",
        0x01 => "unknown-command",
        0x02 => "unknown-connection",
        0x04 => "page-timeout",
        0x07 => "memory-full",
        0x08 => "connection-timeout",
        0x0C => "command-disallowed",
        0x0D => "rejected-limited-resources",
        0x10 => "accept-timeout",
        0x11 => "unsupported",
        0x12 => "invalid-parameters",
        0x16 => "terminated-by-host",
        0x1F => "unspecified",
        _ => "other",
    }
}

fn major_text(major: u32) -> &'static str {
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
