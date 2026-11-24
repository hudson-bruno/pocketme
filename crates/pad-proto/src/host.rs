//! The state machine that takes one gamepad from "heard in a search" (or
//! "it is calling us") to "input reports are flowing".
//!
//! It is a plain value with no clock and no radio of its own: the caller
//! hands it each packet from the controller ([`Host::on_packet`]) and calls
//! [`Host::tick`] every few tens of milliseconds, always with the current
//! time in milliseconds. Everything it does comes out through the [`Io`]
//! trait: packets to send to the controller, events for the screen, and
//! log lines. So a test can replay a whole conversation on the PC.
//!
//! The path, in short:
//!
//! 1. Start: reset the controller, read what it is, set it up (name,
//!    class, Secure Simple Pairing on, connectable but not visible).
//! 2. Search: an inquiry of a few seconds, a pause (in which a paired
//!    gamepad can call us), again. When a device whose class says
//!    "peripheral, gamepad or joystick" answers, stop the inquiry and call
//!    it (Create Connection) at once: a gamepad stays in pairing mode
//!    only about 30 to 40 seconds.
//! 3. Or: a peripheral (or a device that paired earlier in this run)
//!    calls us; we accept.
//! 4. Security: authentication (Secure Simple Pairing "just works", or
//!    the link key from earlier in this run), then encryption. The HID
//!    channels need it.
//! 5. SDP: read the HID record (with the report descriptor) and the
//!    Device ID record. A failure here is logged, not fatal.
//! 6. HID: open the control channel (PSM 0x11), then the interrupt
//!    channel (0x13), or accept them when the gamepad opens them. Both
//!    sides opening at once is fine: the extra channel is closed.
//! 7. Reports flow on the interrupt channel to the screen.
//! 8. Link lost: back to 2.
//!
//! In reconnect-only mode ([`Config::reconnect_only`]) steps 2 and 4's
//! pairing are off. With the crate feature `pair`, a pairing window
//! (`Host::open_pairing`) turns a narrow part of them back on for a
//! while: the search of step 2 without calling anyone, then pairing with
//! the one address the user picked (`Host::pair_with`). Every other
//! rule of reconnect-only mode stays; see the `pair` module.

use core::fmt::{self, Write as _};

use crate::acl::{self, Credits, Reassembler};
use crate::buf::{Hex, Packet, Printable, Ring, Text, u16_at};
use crate::hci::{self, BdAddr, Event, Found, LinkKey, op, params};
use crate::hid;
use crate::l2cap::{self, Command, Signal, code, conf_result, conn_result, conn_status};
use crate::sdp;

#[cfg(feature = "pair")]
pub use pair::{PAIR_WINDOW_MAX, PairEnd, PairError, Window};

/// The largest L2CAP payload we accept (our "MTU"). The default value.
pub const OUR_MTU: u16 = l2cap::MTU_DEFAULT;

/// Room for one incoming L2CAP frame (our MTU plus headers, with margin).
const FRAME_IN: usize = 1100;
/// Room for one outgoing L2CAP frame we build.
const FRAME_OUT: usize = 700;
/// Room for a whole SDP answer, all continuation pieces joined.
pub const SDP_MAX: usize = 2048;
/// Room for the HID report descriptor.
pub const DESCRIPTOR_MAX: usize = 1024;
const CHANNELS: usize = 6;
const KEYS: usize = 4;

/// The most bytes we ask for per SDP answer piece; it must fit in our MTU
/// with the SDP headers (5 + 2 + 1 + 16 continuation bytes).
const SDP_MAX_PER_ANSWER: u16 = 600;

// Timing, in milliseconds.
const COMMAND_TIMEOUT: u64 = 3_000;
const SIGNAL_TIMEOUT: u64 = 5_000;
const PENDING_TIMEOUT: u64 = 30_000;
const SECURITY_TIMEOUT: u64 = 30_000;
/// When the gamepad calls us, give it this long to start security itself.
const INCOMING_AUTH_DELAY: u64 = 600;
/// If the other side started pairing but nothing moved for this long, ask
/// for authentication ourselves (it leads to encryption).
const SECURITY_NUDGE: u64 = 2_000;
/// When both sides start the same security step at the same moment, the
/// controllers report a "collision" and one of them gives up. Try once
/// more after this pause.
const COLLISION_RETRY: u64 = 500;
const SDP_TIMEOUT: u64 = 8_000;
/// After SDP, on a link the gamepad started, wait this long for it to
/// open the HID channels itself before we do.
const PAD_OPENS_GRACE: u64 = 1_000;
/// After a control channel the gamepad opened, wait this long for its
/// interrupt channel before we open one.
const INTERRUPT_GRACE: u64 = 1_000;
const HID_OPEN_TIMEOUT: u64 = 20_000;
const PAGE_GUARD: u64 = 15_000;
const CANCEL_GUARD: u64 = 3_000;
const INQUIRY_GUARD_EXTRA: u64 = 5_000;
const AFTER_LOSS_PAUSE: u64 = 1_000;
const HID_CLOSED_GRACE: u64 = 3_000;
/// A Disconnect sent and no Disconnection Complete for this long: the
/// link is dropped here anyway, so the host never stays stuck on a link
/// the controller has forgotten.
const DISCONNECT_GUARD: u64 = 5_000;
/// The reason logged for a link dropped by [`DISCONNECT_GUARD`]: "the
/// local host ended it".
const REASON_LOCAL_HOST: u8 = 0x16;
const PROBE_HOLD: u64 = 500;
/// The PAD-SUMMARY "flowing" line comes this long after the first input
/// report, so that its report rate means something.
const FLOWING_AFTER: u64 = 2_000;
/// HID open and no report for this long: say so in the log (once).
const NO_REPORT_WAIT: u64 = 3_000;
/// Reports were coming and then none for this long: say so (once).
const REPORTS_STOPPED: u64 = 2_000;

/// How many input reports are logged in full before switching to one
/// line a second.
const REPORTS_LOGGED: u32 = 20;
/// Output reports and SET_REPORTs logged in full on one link. A Switch-mode
/// start-up probe sends up to about 35.
const OUTPUTS_LOGGED: u32 = 60;
/// Output reports whose "the controller sent it" line is still awaited.
const OUTS_WATCHED: usize = 8;

/// IO capability "no input, no output": no screen to show a number on
/// that the gamepad could check, no keys. Pairing is "just works".
const IO_CAP_NO_INPUT_NO_OUTPUT: u8 = 0x03;
/// Authentication requirements: no protection against a listener in the
/// middle (impossible without input/output), general bonding (both sides
/// keep the key so the gamepad can reconnect).
const AUTH_REQ_GENERAL_BONDING: u8 = 0x04;

/// Link supervision timeout, in slots of 0.625 ms: how long a link may
/// stay silent before the controller ends it (Connection Timeout, 0x08).
/// The controller's own default is 0x7D00 = 20 s: a pad switched off
/// (it sends no "detach") stayed "connected" that long. Once we are the
/// central of a gamepad's link we write the pad's own wish (its SDP
/// attribute 0x020C, HIDSupervisionTimeout; the GuliKit asks for
/// 0x0C80 = 2 s), kept between [`LSTO_MIN`] and [`LSTO_MAX`], else
/// [`LSTO_DEFAULT`].
pub const LSTO_DEFAULT: u16 = 0x0C80;
/// 1 s: shorter would end a healthy link on a short fade (the longest
/// gap between two reports seen with the real pad was 366 ms).
pub const LSTO_MIN: u16 = 0x0640;
/// 5 s: longer would leave a switched-off pad "connected" for too long.
pub const LSTO_MAX: u16 = 0x1F40;
/// In sniff mode the link is only heard every sniff interval: the
/// timeout is kept at least this many sniff intervals long.
const LSTO_SNIFF_FACTOR: u32 = 3;
/// A link we dropped ourselves ([`Host::drop_link`]) whose Disconnection
/// Complete has not come: forgotten after this long.
const STALE_FORGET: u64 = 10_000;

/// The supervision timeout for a link: the pad's wish when sane (not 0,
/// clamped to 1 to 5 s), else 2 s; and never shorter than
/// [`LSTO_SNIFF_FACTOR`] sniff intervals (`sniff_slots`, 0 when the link
/// is active), even if that is over [`LSTO_MAX`]: a timeout shorter than
/// the sniff interval would end a healthy link.
pub fn supervision_slots(wish: Option<u16>, sniff_slots: u16) -> u16 {
    let base = match wish {
        Some(w) if w != 0 => w.clamp(LSTO_MIN, LSTO_MAX),
        _ => LSTO_DEFAULT,
    };
    let sniff = (u32::from(sniff_slots) * LSTO_SNIFF_FACTOR).min(0xFFFF) as u16;
    base.max(sniff)
}

/// Disconnect reasons we use.
const REASON_USER_ENDED: u8 = 0x13;
const REASON_AUTH_FAILURE: u8 = 0x05;

/// Settings of the console's side.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// What phones and gamepads see as our name.
    pub local_name: &'static str,
    /// Our class of device (24 bits).
    pub class_of_device: u32,
    /// Inquiry length in units of 1.28 s.
    pub inquiry_units: u8,
    /// Pause between inquiries, in which a paired gamepad can call us.
    pub search_pause_ms: u64,
    /// A PIN for old-style (legacy) pairing, or None to refuse it.
    pub legacy_pin: Option<&'static [u8]>,
    /// Role when we accept a call: 0x00 = ask to become central,
    /// 0x01 = stay peripheral (no role switch).
    pub accept_role: u8,
    /// Only these gamepads (an "allow-list"), or None for any gamepad. A
    /// comma-separated list of addresses ("AA:BB:CC:DD:EE:FF") and pieces
    /// of names ("GuliKit"; upper or lower case does not matter). A
    /// gamepad found in a search must match one item; the others are
    /// logged with their class and signal strength only, never their
    /// address or name, and never called. A gamepad that calls us is
    /// checked against the addresses only (a call carries no name); with
    /// no address in the list, any peripheral may call.
    pub only: Option<&'static str>,
    /// Reconnect only: the console's everyday mode. The host never
    /// searches (no inquiry), never calls anyone (no Create Connection,
    /// [`Host::request_page`] is ignored, no call-back), is never visible
    /// (page scan only), accepts a call only from an address it has a key
    /// for ([`Host::load_key`]), refuses every pairing (IO capability, user
    /// confirmation, PIN, passkey) and never changes its table of keys:
    /// [`Io::key_change`] is never called. A failure of any kind ends the
    /// link and goes back to "waiting".
    pub reconnect_only: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Config {
    /// The defaults, as a constant (for `const` construction).
    pub const DEFAULT: Config = Config {
        local_name: "PocketMe",
        // Major class "computer" (0x01), minor "laptop" (0x03): the
        // most common kind of host a gamepad is paired with.
        class_of_device: 0x00010C,
        // 3 x 1.28 s = 3.84 s.
        inquiry_units: 3,
        search_pause_ms: 1_500,
        legacy_pin: None,
        accept_role: 0x01,
        only: None,
        reconnect_only: false,
    };
}

/// Where a log line belongs. The firmware prints the name in front.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
    State,
    Hci,
    L2cap,
    Sdp,
    Hid,
    Err,
    /// One line that sums up a connection (see [`Host`]'s PAD-SUMMARY).
    Summary,
}

impl Tag {
    pub fn name(self) -> &'static str {
        match self {
            Tag::State => "PAD-STATE",
            Tag::Hci => "PAD-HCI",
            Tag::L2cap => "PAD-L2CAP",
            Tag::Sdp => "PAD-SDP",
            Tag::Hid => "PAD-HID",
            Tag::Err => "PAD-ERR",
            Tag::Summary => "PAD-SUMMARY",
        }
    }
}

/// What the gamepad says about itself. Same fields as pad-view's `Device`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceInfo<'a> {
    pub name: &'a str,
    pub class_of_device: u32,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub version: Option<u16>,
    pub vendor_id_source: Option<u16>,
}

/// Everything the screen needs to know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiEvent<'a> {
    /// One short line about what the console is doing.
    Status(&'a str),
    /// Who the gamepad is (sent again when more is known).
    Device(DeviceInfo<'a>),
    /// The HID report descriptor read over SDP.
    Descriptor(&'a [u8]),
    /// One input report, without the 0xA1 byte in front.
    Report(&'a [u8]),
    /// One message on the HID control channel, its header byte first: a
    /// HANDSHAKE (0x00 to 0x0F, the answer to a SET_REPORT), a DATA
    /// message, or anything else the gamepad sends there.
    Control(&'a [u8]),
    /// The link is gone.
    Disconnected,
    /// A gamepad heard in a pairing window's search (feature `pair`):
    /// once per address and window, gamepads only. `name` is the name
    /// from its inquiry answer as sent (not checked as text, cut at the
    /// first zero byte; empty when it sent none). The host never logs the
    /// name, only its length; the receiver should not either.
    #[cfg(feature = "pair")]
    Found {
        addr: BdAddr,
        class_of_device: u32,
        name: &'a [u8],
    },
}

/// Why the host dropped a pairing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForgetReason {
    /// Authentication with the saved key failed (0x05 or 0x06): the
    /// gamepad no longer has this key.
    AuthFailed,
    /// The gamepad said "virtual cable unplug": it unpaired itself.
    Unplugged,
    /// A newer gamepad took the place of the oldest one (the table holds
    /// four).
    Evicted,
}

impl ForgetReason {
    /// A short word for the log.
    pub fn word(self) -> &'static str {
        match self {
            ForgetReason::AuthFailed => "auth-failed",
            ForgetReason::Unplugged => "unplugged",
            ForgetReason::Evicted => "evicted",
        }
    }
}

/// A change to the host's table of pairings, for whoever keeps them
/// across restarts (the firmware writes them to flash). The host itself
/// only keeps them in RAM.
///
/// A pairing is the gamepad's address plus the secret link key both
/// sides agreed on. The key's `Debug` output hides its bytes; never print
/// them another way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyChange {
    /// A new pairing, or a new key for an address we knew: keep it. It
    /// replaces any older key of that address.
    Save {
        addr: BdAddr,
        key: LinkKey,
        key_type: u8,
    },
    /// This pairing is no longer good: erase it.
    Forget { addr: BdAddr, why: ForgetReason },
}

/// The machine's way out.
pub trait Io {
    /// One whole HCI packet for the controller, packet type byte first.
    fn send(&mut self, packet: &[u8]);
    fn ui(&mut self, event: UiEvent<'_>);
    /// One log line (without the tag; the receiver puts it in front).
    fn log(&mut self, tag: Tag, args: fmt::Arguments<'_>);
    /// The table of pairings changed (see [`KeyChange`]). Called right
    /// after the change; the default does nothing (keys in RAM only).
    fn key_change(&mut self, change: KeyChange) {
        let _ = change;
    }
}

macro_rules! log {
    ($io:expr, $tag:ident, $($a:tt)*) => {
        $io.log(Tag::$tag, format_args!($($a)*))
    };
}

// After `log!`, so the window's code can use it.
#[cfg(feature = "pair")]
mod pair;

/// Why a link is being made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// The real thing: pair, read the description, open HID.
    Gamepad,
    /// A test with a device that is not a gamepad (a phone):
    /// no pairing at all, only an SDP query for `uuid` in pieces of at
    /// most `max_bytes`, then close.
    SdpProbe { uuid: u16, max_bytes: u16 },
}

/// Counters for the log.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub events: u32,
    pub acl_in: u32,
    pub acl_out: u32,
    pub reports: u32,
    pub links: u32,
    pub rounds: u32,
    pub found_gamepads: u32,
    pub found_other: u32,
    /// Gamepads heard that are not on the allow-list ([`Config::only`]).
    pub found_skipped: u32,
    pub dropped_out: u32,
    pub errors: u32,
}

impl Stats {
    /// All counters at zero (the same as `Stats::default()`).
    pub const ZERO: Stats = Stats {
        events: 0,
        acl_in: 0,
        acl_out: 0,
        reports: 0,
        links: 0,
        rounds: 0,
        found_gamepads: 0,
        found_other: 0,
        found_skipped: 0,
        dropped_out: 0,
        errors: 0,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Start-up commands running.
    Init,
    /// Between inquiries; connectable.
    Pause {
        until: u64,
    },
    Inquiry {
        until: u64,
    },
    /// Reconnect-only mode, nothing under way: connectable (page scan)
    /// for a saved gamepad's call, nothing else.
    Waiting,
    /// A gamepad answered; waiting for the inquiry to stop.
    Cancelling {
        since: u64,
    },
    /// Create Connection sent.
    Paging {
        since: u64,
    },
    /// A call accepted; waiting for Connection Complete.
    Accepting {
        since: u64,
    },
    Linked,
}

#[derive(Clone, Copy)]
struct Candidate {
    addr: BdAddr,
    psrm: u8,
    clock: Option<u16>,
    cod: u32,
    name: Text<48>,
    purpose: Purpose,
    /// When a search heard it (None: it called us, or a board test).
    found_at: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sec {
    None,
    /// Pairing or key exchange under way.
    Pairing,
    Encrypting,
    Encrypted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SdpState {
    Idle,
    Opening,
    Waiting { tid: u16 },
    Done,
}

#[derive(Clone, Copy)]
struct Link {
    handle: u16,
    addr: BdAddr,
    cod: u32,
    name: Text<48>,
    incoming: bool,
    purpose: Purpose,
    sec: Sec,
    we_asked_auth: bool,
    auth_retried: bool,
    auth_at: Option<u64>,
    enc_retried: bool,
    /// When to ask for encryption again after a collision.
    enc_at: Option<u64>,
    last_sec_event: u64,
    sec_deadline: u64,
    sdp: SdpState,
    sdp_query: u8,
    sdp_tid: u16,
    sdp_deadline: u64,
    sdp_cont: [u8; 17],
    sdp_cont_len: usize,
    sdp_pieces: u32,
    pnp: sdp::PnpInfo,
    open_ctrl_at: Option<u64>,
    open_intr_at: Option<u64>,
    hid_deadline: Option<u64>,
    hid_up: bool,
    info_req: Option<(u8, u64)>,
    reports: u32,
    reports_second: u32,
    report_log_at: u64,
    /// Output reports (interrupt channel) and SET_REPORTs (control
    /// channel) handed over, counted together.
    outputs: u32,
    set_reports: u32,
    handshakes: u32,
    /// The SET_REPORT now waiting for its HANDSHAKE: (sent at, output n).
    set_report_wait: Option<(u64, u32)>,
    /// The GET_REPORT now waiting for its answer: (sent at, report id).
    get_report_wait: Option<(u64, u8)>,
    /// ACL packets queued for this link, and how many of them the
    /// controller said it sent (Number Of Completed Packets).
    acl_queued: u32,
    acl_done: u32,
    /// Outputs waiting for their "sent" line: (last ACL packet number,
    /// output n, SET_REPORT?, queued at).
    outs: [(u32, u32, bool, u64); OUTS_WATCHED],
    outs_n: usize,
    disconnect_at: Option<u64>,
    disconnect_sent: bool,
    /// When the first tick after the Disconnect saw it (for
    /// [`DISCONNECT_GUARD`]).
    disconnect_seen_at: Option<u64>,
    // For the PAD-SUMMARY lines and the "still waiting" lines.
    found_at: Option<u64>,
    up_at: u64,
    hid_up_at: Option<u64>,
    first_report_at: Option<u64>,
    last_report_at: u64,
    /// We answered a Link Key Request with a key kept from earlier.
    key_offered: bool,
    /// A new key came (Link Key Notification): this was a new pairing.
    new_key: bool,
    auth_ok: bool,
    desc_bytes: usize,
    ids: [IdCount; IDS_COUNTED],
    ids_n: usize,
    flowing_logged: bool,
    no_report_logged: bool,
    stopped_logged: bool,
    /// We are the link's central (after a role change, or because we
    /// called): only then may we write its supervision timeout.
    central: bool,
    /// The pad's HIDSupervisionTimeout from SDP (slots), when read.
    lsto_wish: Option<u16>,
    /// The sniff interval now (slots), 0 while the link is active.
    sniff_slots: u16,
    /// The supervision timeout we last asked the controller for.
    lsto_sent: Option<u16>,
}

/// A link we dropped ourselves ([`Host::drop_link`]) and whose
/// Disconnection Complete has not come yet: its handle, since when, and
/// the data packets that still came for it (dropped quietly).
#[derive(Clone, Copy, Debug)]
struct Stale {
    handle: u16,
    since: u64,
    acl: u32,
}

/// Input reports counted by their first byte (the report id, for every
/// gamepad that uses report ids).
#[derive(Clone, Copy, Debug, Default)]
struct IdCount {
    id: u8,
    min_len: u16,
    max_len: u16,
    count: u32,
}

const IDS_COUNTED: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChSt {
    Free,
    /// We sent a Connection Request.
    Connecting,
    /// They asked; we answered "pending" until security is done.
    Pending,
    Config,
    Open,
    /// We sent a Disconnection Request.
    Closing,
}

#[derive(Clone, Copy, Debug)]
struct Chan {
    st: ChSt,
    psm: u16,
    local: u16,
    remote: u16,
    /// We opened it (rather than the other side).
    ours: bool,
    our_conf_ok: bool,
    their_conf_ok: bool,
    remote_mtu: u16,
    /// An MTU from a part of their Configuration Request that said "more
    /// parts follow" (continuation flag): used when the last part comes.
    pending_mtu: Option<u16>,
    /// The MTU we put in our Configuration Request (None = leave it out).
    ask_mtu: Option<u16>,
    conf_tries: u8,
    /// Identifier of our request now waiting for an answer.
    req_id: u8,
    /// Identifier of their Connection Request we answered "pending".
    their_req_id: u8,
    deadline: u64,
}

const FREE_CHAN: Chan = Chan {
    st: ChSt::Free,
    psm: 0,
    local: 0,
    remote: 0,
    ours: false,
    our_conf_ok: false,
    their_conf_ok: false,
    remote_mtu: l2cap::MTU_DEFAULT,
    pending_mtu: None,
    ask_mtu: Some(OUR_MTU),
    conf_tries: 0,
    req_id: 0,
    their_req_id: 0,
    deadline: 0,
};

/// Data packets that wait for the controller to take them.
const ACLQ: usize = 16;

/// The host: one gamepad at a time.
pub struct Host {
    cfg: Config,
    phase: Phase,
    cand: Option<Candidate>,
    /// A call to make instead of the next inquiry: address, purpose and
    /// the name shown while calling.
    forced: Option<(BdAddr, Purpose, &'static str)>,
    /// The gamepad we already called back once after it hung up on our
    /// saved key (see `on_link_down`), so we do not do it again and again.
    called_back: Option<BdAddr>,
    link: Option<Link>,
    chans: [Chan; CHANNELS],
    next_cid: u16,
    sig_id: u8,
    cmdq: Ring<Packet, 16>,
    cmd_wait: Option<(u16, u64)>,
    cmd_credits: u8,
    aclq: Ring<Packet, ACLQ>,
    credits: Credits,
    acl_max: usize,
    reasm: Reassembler<FRAME_IN>,
    sdp_buf: [u8; SDP_MAX],
    sdp_len: usize,
    sdp_truncated: bool,
    desc: [u8; DESCRIPTOR_MAX],
    desc_len: usize,
    /// Pairings: address, link key, key type. Oldest first, no gaps.
    keys: [Option<(BdAddr, LinkKey, u8)>; KEYS],
    known: [Option<BdAddr>; KEYS],
    stats: Stats,
    /// The old link after [`Host::drop_link`], until the controller says
    /// it is closed.
    stale: Option<Stale>,
    /// A role change for the call being accepted that came before its
    /// Connection Complete: (address, we are central).
    early_role: Option<(BdAddr, bool)>,
    /// The pairing window, its found table, the key it holds and its
    /// outcome (see the `pair` module).
    #[cfg(feature = "pair")]
    pair: pair::PairState,
}

impl Host {
    /// A host that has not started. `const`, so a board can keep it in a
    /// static without building it on the stack first (it is about 15 KB).
    pub const fn new(cfg: Config) -> Self {
        Self {
            cfg,
            phase: Phase::Init,
            cand: None,
            forced: None,
            called_back: None,
            link: None,
            chans: [FREE_CHAN; CHANNELS],
            next_cid: l2cap::CID_DYNAMIC_FIRST,
            sig_id: 0,
            cmdq: Ring::new(Packet::new()),
            cmd_wait: None,
            cmd_credits: 1,
            aclq: Ring::new(Packet::new()),
            credits: Credits { total: 0, free: 0 },
            acl_max: acl::MAX_FRAGMENT,
            reasm: Reassembler::new(),
            sdp_buf: [0; SDP_MAX],
            sdp_len: 0,
            sdp_truncated: false,
            desc: [0; DESCRIPTOR_MAX],
            desc_len: 0,
            keys: [None; KEYS],
            known: [None; KEYS],
            stats: Stats::ZERO,
            stale: None,
            early_role: None,
            #[cfg(feature = "pair")]
            pair: pair::PairState::NONE,
        }
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// A word for the current step, for a status line or the log.
    pub fn phase_name(&self) -> &'static str {
        match self.phase {
            Phase::Init => "init",
            Phase::Pause { .. } => "pause",
            Phase::Inquiry { .. } => "inquiry",
            Phase::Waiting => "waiting",
            Phase::Cancelling { .. } => "cancelling",
            Phase::Paging { .. } => "paging",
            Phase::Accepting { .. } => "accepting",
            Phase::Linked => match self.link.map(|l| (l.sec, l.hid_up, l.purpose)) {
                Some((_, _, Purpose::SdpProbe { .. })) => "probe",
                Some((_, true, _)) => "connected",
                Some((Sec::Encrypted, false, _)) => "opening",
                _ => "pairing",
            },
        }
    }

    /// True while a link exists.
    pub fn linked(&self) -> bool {
        self.link.is_some()
    }

    /// True when output reports can go to the gamepad now: both HID
    /// channels are open on a gamepad link. Before that,
    /// [`Host::send_output_report`] can only say no, so a caller with a
    /// start-up conversation to hold (Switch mode) should wait for this.
    pub fn hid_ready(&self) -> bool {
        self.link
            .is_some_and(|l| l.hid_up && l.purpose == Purpose::Gamepad)
            && self.hid_chan(l2cap::PSM_HID_INTERRUPT).is_some()
    }

    /// Instead of the next inquiry, call this address. For tests on the
    /// board: a made-up address (must fail with a page timeout), or the
    /// a phone of your own with [`Purpose::SdpProbe`].
    /// Ignored in reconnect-only mode ([`Config::reconnect_only`]).
    pub fn request_page(&mut self, addr: BdAddr, purpose: Purpose) {
        if self.cfg.reconnect_only {
            return;
        }
        let name = if purpose == Purpose::Gamepad {
            "test address"
        } else {
            "probe peer"
        };
        self.forced = Some((addr, purpose, name));
    }

    /// Gives the host a pairing saved earlier (for example in flash), so
    /// the gamepad can connect without pairing mode. Call it before
    /// [`Host::start`], oldest pairing first. Nothing is reported back
    /// through [`Io::key_change`]. Beyond four, the oldest is dropped.
    pub fn load_key(&mut self, addr: BdAddr, key: LinkKey, key_type: u8) {
        self.remove_key(addr);
        if self.keys[KEYS - 1].is_some() {
            self.keys.rotate_left(1);
            self.keys[KEYS - 1] = None;
        }
        if let Some(slot) = self.keys.iter_mut().find(|k| k.is_none()) {
            *slot = Some((addr, key, key_type));
        }
    }

    /// The address of saved pairing number `i` (0 to 3, oldest first),
    /// for a board that checks the packets it sends against the saved
    /// pads. Never the key. An address is personal: never print it whole
    /// (its `Display` shows the last two bytes only).
    pub fn key_addr(&self, i: usize) -> Option<BdAddr> {
        self.keys.get(i).copied().flatten().map(|(a, _, _)| a)
    }

    /// How many pairings the host knows now.
    pub fn key_count(&self) -> usize {
        self.keys.iter().flatten().count()
    }

    /// Forgets every pairing (the caller asked for it, so nothing is
    /// reported through [`Io::key_change`]; the caller erases its own
    /// copy). A connected gamepad is let go, so it has to pair again.
    /// Returns how many pairings there were.
    pub fn forget_all_keys(&mut self, io: &mut impl Io) -> usize {
        let n = self.key_count();
        self.keys = [None; KEYS];
        self.known = [None; KEYS];
        self.called_back = None;
        log!(io, State, "forgot all pairings: {} (asked by the user)", n);
        if self.link.is_some_and(|l| l.purpose == Purpose::Gamepad) {
            self.status(io, format_args!("Pairings forgotten"));
            self.disconnect(io, REASON_USER_ENDED);
        }
        n
    }

    /// True while a [`Host::request_page`] has not been started yet.
    pub fn page_requested(&self) -> bool {
        self.forced.is_some()
    }

    /// True when nothing is under way: no link, no call, between or
    /// during inquiries (or waiting, in reconnect-only mode). A good
    /// moment for the next board test.
    pub fn searching(&self) -> bool {
        self.link.is_none()
            && matches!(
                self.phase,
                Phase::Pause { .. } | Phase::Inquiry { .. } | Phase::Waiting
            )
    }

    /// True in reconnect-only mode while nothing is under way: no link,
    /// no call being accepted. The state every failure comes back to.
    pub fn waiting(&self) -> bool {
        self.link.is_none() && self.phase == Phase::Waiting
    }

    /// Sends the start-up commands. Call once.
    pub fn start(&mut self, now: u64, io: &mut impl Io) {
        self.status(io, format_args!("Starting radio"));
        if self.cfg.reconnect_only {
            log!(
                io,
                State,
                "mode: reconnect-only; saved pads={}; no search, not visible, no calls of our own, no pairing; only a saved pad may call",
                self.key_count()
            );
        }
        match self.cfg.only {
            // Reconnect-only calls nobody: the saved keys are the list.
            None if self.cfg.reconnect_only => {
                log!(io, State, "allow-list: off (the saved pads are the list)")
            }
            None => log!(
                io,
                State,
                "allow-list: off (any gamepad that answers is called)"
            ),
            Some(list) => {
                let addrs = only_items(list)
                    .filter(|t| BdAddr::parse(t).is_some())
                    .count();
                let names = only_items(list)
                    .filter(|t| BdAddr::parse(t).is_none())
                    .count();
                log!(
                    io,
                    State,
                    "allow-list: {} address(es) and {} name piece(s); other gamepads are not called and not logged",
                    addrs,
                    names
                );
            }
        }
        self.cmd(io, op::RESET, &[]);
        self.cmd(io, op::READ_LOCAL_VERSION, &[]);
        self.cmd(io, op::READ_LOCAL_FEATURES, &[]);
        self.cmd(io, op::READ_BUFFER_SIZE, &[]);
        self.cmd(io, op::READ_BD_ADDR, &[]);
        self.cmd(io, op::SET_EVENT_MASK, &hci::EVENT_MASK);
        self.cmd(io, op::WRITE_SIMPLE_PAIRING_MODE, &[0x01]);
        // Allow role switch and sniff mode (a low-power mode gamepads like).
        self.cmd(
            io,
            op::WRITE_DEFAULT_LINK_POLICY_SETTINGS,
            &0x0005_u16.to_le_bytes(),
        );
        self.cmd(
            io,
            op::WRITE_LOCAL_NAME,
            &params::local_name(self.cfg.local_name),
        );
        self.cmd(
            io,
            op::WRITE_CLASS_OF_DEVICE,
            &params::class_of_device(self.cfg.class_of_device),
        );
        // Inquiry answers with signal strength, or the extended kind.
        self.cmd(io, op::WRITE_INQUIRY_MODE, &[0x02]);
        // 0x2000 x 0.625 ms = 5.12 s, the usual page timeout.
        self.cmd(io, op::WRITE_PAGE_TIMEOUT, &0x2000_u16.to_le_bytes());
        // 0x02: answer calls (page scan) but not searches (no inquiry scan).
        self.cmd(io, op::WRITE_SCAN_ENABLE, &[0x02]);
        self.pump(now, io);
    }

    /// One packet from the controller, packet type byte first.
    pub fn on_packet(&mut self, now: u64, p: &[u8], io: &mut impl Io) {
        match p.first() {
            Some(0x04) => match hci::parse_event(&p[1..]) {
                Some(ev) => {
                    self.stats.events += 1;
                    self.on_event(now, ev, io);
                }
                None => self.err(
                    io,
                    format_args!("event shorter than its header len={}", p.len()),
                ),
            },
            Some(0x02) => self.on_acl(now, &p[1..], io),
            _ => self.err(
                io,
                format_args!("packet of unknown kind {:02x?} len={}", p.first(), p.len()),
            ),
        }
        self.pump(now, io);
    }

    /// Timers. Call often (every 20 to 100 ms).
    pub fn tick(&mut self, now: u64, io: &mut impl Io) {
        if let Some(st) = self.stale
            && now >= st.since + STALE_FORGET
        {
            self.err(io, format_args!("timeout: no disconnection-complete for the old link handle={:#05x} {} ms after we dropped it; forgetting it", st.handle, STALE_FORGET));
            self.stale = None;
        }
        if let Some((opcode, at)) = self.cmd_wait
            && now >= at + COMMAND_TIMEOUT
        {
            self.err(io, format_args!("timeout: no answer to command {} op={:#06x} after {} ms; awaited command-status or command-complete from the controller", op::name(opcode), opcode, COMMAND_TIMEOUT));
            self.cmd_wait = None;
            self.cmd_credits = 1;
            self.on_command_failed(now, opcode, 0xFF, io);
        }
        // The pairing window's deadline and its search rounds.
        #[cfg(feature = "pair")]
        self.window_tick(now, io);
        match self.phase {
            Phase::Pause { until }
                if now >= until
                    && self.link.is_none()
                    && self.cfg.reconnect_only
                    && !self.window_open() =>
            {
                self.phase = Phase::Waiting;
                self.status(io, format_args!("Waiting for a saved pad"));
            }
            Phase::Pause { until }
                if now >= until && self.link.is_none() && self.cmd_wait.is_none() =>
            {
                if let Some((addr, purpose, label)) = self.forced.take() {
                    let mut name = Text::new();
                    let _ = write!(name, "{}", label);
                    self.cand = Some(Candidate {
                        addr,
                        psrm: 0x01,
                        clock: None,
                        cod: 0,
                        name,
                        purpose,
                        found_at: None,
                    });
                    self.start_page(now, io);
                } else {
                    self.start_inquiry(now, io);
                }
            }
            Phase::Inquiry { until } if now >= until => {
                self.err(io, format_args!("inquiry did not end in time; cancelling"));
                self.cmd(io, op::INQUIRY_CANCEL, &[]);
                self.phase = Phase::Pause {
                    until: now + self.cfg.search_pause_ms,
                };
            }
            Phase::Cancelling { since } if now >= since + CANCEL_GUARD => {
                self.err(io, format_args!("timeout: no answer to inquiry-cancel after {} ms; calling the gamepad anyway", CANCEL_GUARD));
                self.start_page(now, io);
            }
            Phase::Paging { since } if now >= since + PAGE_GUARD => {
                self.err(io, format_args!("timeout: no connection-complete {} ms after create-connection; cancelling the call", PAGE_GUARD));
                if let Some(c) = self.cand {
                    self.cmd(io, op::CREATE_CONNECTION_CANCEL, &params::addr(c.addr));
                }
                self.back_to_search(now, io);
            }
            Phase::Accepting { since } if now >= since + PAGE_GUARD => {
                self.err(
                    io,
                    format_args!(
                        "timeout: no connection-complete {} ms after accepting the call",
                        PAGE_GUARD
                    ),
                );
                self.back_to_search(now, io);
            }
            Phase::Linked => self.link_timers(now, io),
            _ => {}
        }
        self.pump(now, io);
    }

    /// Sends an output report (without the 0xA2 byte) on the HID
    /// interrupt channel, as DATA + Output (0xA2). False if there is no
    /// open channel or it does not fit.
    pub fn send_output_report(&mut self, now: u64, report: &[u8], io: &mut impl Io) -> bool {
        self.send_hid(now, l2cap::PSM_HID_INTERRUPT, hid::DATA_OUTPUT, report, io)
    }

    /// Sends an output report (report id first) on the HID control
    /// channel as SET_REPORT, type Output (header 0x52). The gamepad must
    /// answer with a HANDSHAKE on the control channel; it is logged as
    /// `PAD-HID handshake result=...` and handed on as
    /// [`UiEvent::Control`]. False if there is no open channel or it does
    /// not fit.
    pub fn send_set_report(&mut self, now: u64, report: &[u8], io: &mut impl Io) -> bool {
        self.send_hid(
            now,
            l2cap::PSM_HID_CONTROL,
            hid::SET_REPORT_OUTPUT,
            report,
            io,
        )
    }

    /// Asks the gamepad for input report `id` now: GET_REPORT, type Input
    /// (header 0x41), on the HID control channel. The answer is a DATA
    /// message (0xA1, the report) or a HANDSHAKE (an error code) on the
    /// control channel. Both are logged with the time it took and handed
    /// on as [`UiEvent::Control`]. False if there is no open channel.
    pub fn send_get_report(&mut self, now: u64, id: u8, io: &mut impl Io) -> bool {
        let sent = self.send_hid(
            now,
            l2cap::PSM_HID_CONTROL,
            hid::GET_REPORT_INPUT,
            &[id],
            io,
        );
        if sent && let Some(l) = self.link.as_mut() {
            l.get_report_wait = Some((now, id));
        }
        sent
    }

    /// One HID message: `header`, then `report`, on the open HID channel
    /// of `psm` of a gamepad link.
    fn send_hid(
        &mut self,
        now: u64,
        psm: u16,
        header: u8,
        report: &[u8],
        io: &mut impl Io,
    ) -> bool {
        let Some(link) = self.link else { return false };
        if link.purpose != Purpose::Gamepad {
            return false;
        }
        let Some(i) = self.hid_chan(psm) else {
            return false;
        };
        let ch = self.chans[i];
        if report.len() + 1 > usize::from(ch.remote_mtu) || report.len() + 1 > FRAME_OUT - 4 {
            self.err(
                io,
                format_args!(
                    "output report of {} bytes is over the gamepad's MTU {}",
                    report.len(),
                    ch.remote_mtu
                ),
            );
            return false;
        }
        // No room in the send queue (the controller took no packets for a
        // while): say "not sent", so the caller keeps the report and asks
        // again, instead of "sent" for a packet the queue would drop.
        let pieces = (report.len() + 1 + 4).div_ceil(self.acl_max.clamp(1, acl::MAX_FRAGMENT));
        if self.aclq.len() + pieces > ACLQ {
            return false;
        }
        let set_report = header == hid::SET_REPORT_OUTPUT;
        let get_report = header == hid::GET_REPORT_INPUT;
        let mut msg = [0_u8; FRAME_OUT];
        msg[0] = header;
        msg[1..=report.len()].copy_from_slice(report);
        let in_flight = self.credits.in_flight();
        let n = match self.link.as_mut() {
            Some(l) => {
                l.outputs += 1;
                if set_report {
                    l.set_reports += 1;
                    if l.set_report_wait.is_some() {
                        log!(
                            io,
                            Hid,
                            "set-report n={}: the one before had no handshake yet",
                            l.outputs
                        );
                    }
                    l.set_report_wait = Some((now, l.outputs));
                }
                l.outputs
            }
            None => 0,
        };
        if n <= OUTPUTS_LOGGED {
            log!(
                io,
                Hid,
                "{} n={} len={} cid={:#06x} in_flight={} hex={}",
                if set_report {
                    "set-report"
                } else if get_report {
                    "get-report"
                } else {
                    "output"
                },
                n,
                report.len(),
                ch.remote,
                in_flight,
                Hex(report)
            );
        }
        self.send_frame(io, ch.remote, &msg[..=report.len()]);
        if let Some(l) = self.link.as_mut() {
            if l.outs_n == OUTS_WATCHED {
                // The oldest one is given up on (its line never comes).
                l.outs.copy_within(1.., 0);
                l.outs_n -= 1;
            }
            l.outs[l.outs_n] = (l.acl_queued, n, set_report, now);
            l.outs_n += 1;
        }
        self.pump(now, io);
        true
    }

    // ------------------------------------------------------------ output

    fn status(&mut self, io: &mut impl Io, args: fmt::Arguments<'_>) {
        let mut t: Text<64> = Text::new();
        let _ = t.write_fmt(args);
        log!(io, State, "{} (phase={})", t.as_str(), self.phase_name());
        io.ui(UiEvent::Status(t.as_str()));
    }

    fn err(&mut self, io: &mut impl Io, args: fmt::Arguments<'_>) {
        self.stats.errors += 1;
        io.log(Tag::Err, args);
    }

    fn ui_device(&self, io: &mut impl Io) {
        let (name, cod, pnp) = match (self.link, self.cand) {
            (Some(l), _) => (l.name, l.cod, l.pnp),
            (None, Some(c)) => (c.name, c.cod, sdp::PnpInfo::default()),
            _ => return,
        };
        io.ui(UiEvent::Device(DeviceInfo {
            name: name.as_str(),
            class_of_device: cod,
            vendor_id: pnp.vendor_id,
            product_id: pnp.product_id,
            version: pnp.version,
            vendor_id_source: pnp.vendor_id_source,
        }));
    }

    /// The address an open pairing window pairs with (after
    /// [`Host::pair_with`]). Always None without the feature `pair`.
    fn target(&self) -> Option<BdAddr> {
        #[cfg(feature = "pair")]
        {
            self.pair.target()
        }
        #[cfg(not(feature = "pair"))]
        {
            None
        }
    }

    /// A pairing window pairs with `addr` and its new key has not come
    /// yet: the pairing "yes" answers may go to `addr`. Always false
    /// without the feature `pair`.
    fn window_pairs(&self, addr: BdAddr) -> bool {
        self.target() == Some(addr) && !self.window_has_new_key(addr)
    }

    /// The window holds a new key for `addr` (it came, encryption has not
    /// yet). Always false without the feature `pair`.
    fn window_has_new_key(&self, addr: BdAddr) -> bool {
        #[cfg(feature = "pair")]
        {
            self.pair.held.is_some_and(|h| h.addr == addr)
        }
        #[cfg(not(feature = "pair"))]
        {
            let _ = addr;
            false
        }
    }

    /// A pairing window is open. Always false without the feature `pair`.
    fn window_open(&self) -> bool {
        #[cfg(feature = "pair")]
        {
            self.pair.window.is_some()
        }
        #[cfg(not(feature = "pair"))]
        {
            false
        }
    }

    /// An open pairing window lets this reconnect-only-forbidden command
    /// out: Inquiry while it searches; Create Connection and the two
    /// pairing "yes" answers for its one address. Never a PIN. Always
    /// false without the feature `pair`.
    fn window_allows(&self, opcode: u16, p: &[u8]) -> bool {
        #[cfg(feature = "pair")]
        {
            self.pair.allows(opcode, p)
        }
        #[cfg(not(feature = "pair"))]
        {
            let _ = (opcode, p);
            false
        }
    }

    fn cmd(&mut self, io: &mut impl Io, opcode: u16, p: &[u8]) {
        // The last gate of reconnect-only mode: whatever path led here, a
        // search, a call of our own or a "yes" to pairing never leaves,
        // except the few an open pairing window lets out (for its one
        // address only; never a PIN).
        if self.cfg.reconnect_only
            && forbidden_when_reconnect_only(opcode)
            && !self.window_allows(opcode, p)
        {
            self.err(
                io,
                format_args!(
                    "command {} blocked: not allowed in reconnect-only mode",
                    op::name(opcode)
                ),
            );
            return;
        }
        if self.cmdq.push(hci::command(opcode, p)).is_err() {
            self.err(
                io,
                format_args!("command queue full, dropped {}", op::name(opcode)),
            );
        }
    }

    /// Sends what may be sent now: the next command if none is waiting,
    /// and data packets while the controller has buffers.
    fn pump(&mut self, now: u64, io: &mut impl Io) {
        while self.cmd_wait.is_none()
            && self.cmd_credits > 0
            && let Some(p) = self.cmdq.pop()
        {
            let opcode = u16::from_le_bytes([p.as_bytes()[1], p.as_bytes()[2]]);
            if opcode == op::WRITE_LINK_SUPERVISION_TIMEOUT
                && !self.supervision_fits(&p.as_bytes()[4..])
            {
                // It waited behind other commands while its link ended:
                // the board's gate would refuse a handle that is gone,
                // and nothing would answer it for COMMAND_TIMEOUT.
                log!(
                    io,
                    State,
                    "supervision timeout not sent: its link ended while it waited"
                );
                continue;
            }
            // The window's rule again: a command it let in may have waited
            // here while the window closed or moved on to one address.
            #[cfg(feature = "pair")]
            if self.cfg.reconnect_only
                && forbidden_when_reconnect_only(opcode)
                && !self.window_allows(opcode, &p.as_bytes()[4..])
            {
                self.err(
                    io,
                    format_args!(
                        "queued command {} dropped: the pairing window no longer allows it",
                        op::name(opcode)
                    ),
                );
                continue;
            }
            // The parameters are logged, except the link key's 16 bytes,
            // the PIN, and all but the last two bytes of an address.
            let shown = &p.as_bytes()[4..];
            let secret = matches!(
                opcode,
                op::LINK_KEY_REQUEST_REPLY | op::PIN_CODE_REQUEST_REPLY
            );
            let shown = if secret {
                &shown[..shown.len().min(6)]
            } else {
                &shown[..shown.len().min(24)]
            };
            let addr = if starts_with_address(opcode) {
                2..6
            } else {
                0..0
            };
            log!(
                io,
                Hci,
                "cmd {} op={:#06x} params={}{}",
                op::name(opcode),
                opcode,
                HexMask(shown, addr),
                if secret { "+secret(hidden)" } else { "" }
            );
            self.cmd_credits -= 1;
            self.cmd_wait = Some((opcode, now));
            io.send(p.as_bytes());
        }
        while self.credits.free > 0 {
            let Some(p) = self.aclq.pop() else { break };
            self.credits.take();
            self.stats.acl_out += 1;
            io.send(p.as_bytes());
        }
    }

    fn new_id(&mut self) -> u8 {
        self.sig_id = self.sig_id.wrapping_add(1);
        if self.sig_id == 0 {
            self.sig_id = 1;
        }
        self.sig_id
    }

    /// Queues one L2CAP frame (cut into ACL packets).
    fn send_frame(&mut self, io: &mut impl Io, cid: u16, payload: &[u8]) {
        let Some(link) = self.link else { return };
        if payload.len() + 4 > FRAME_OUT {
            self.err(
                io,
                format_args!("frame of {} bytes too long to send", payload.len()),
            );
            return;
        }
        let mut f = [0_u8; FRAME_OUT];
        f[0..2].copy_from_slice(&(payload.len() as u16).to_le_bytes());
        f[2..4].copy_from_slice(&cid.to_le_bytes());
        f[4..4 + payload.len()].copy_from_slice(payload);
        let mut dropped = 0;
        let mut queued = 0;
        for p in acl::fragments(link.handle, &f[..4 + payload.len()], self.acl_max) {
            if self.aclq.push(p).is_err() {
                dropped += 1;
            } else {
                queued += 1;
            }
        }
        if let Some(l) = self.link.as_mut() {
            l.acl_queued += queued;
        }
        if dropped > 0 {
            self.stats.dropped_out += dropped;
            self.err(
                io,
                format_args!("send queue full, {} data packets dropped", dropped),
            );
        }
    }

    fn send_signal(&mut self, io: &mut impl Io, code: u8, id: u8, data: &[u8]) {
        let mut f = [0_u8; 80];
        let n = l2cap::write_command(&mut f, code, id, &data[..data.len().min(76)]);
        log!(
            io,
            L2cap,
            "tx {} id={} data={}",
            l2cap::code_name(code),
            id,
            Hex(&data[..data.len().min(76)])
        );
        self.send_frame(io, l2cap::CID_SIGNALLING, &f[..n]);
    }

    // ------------------------------------------------------------ search

    fn start_inquiry(&mut self, now: u64, io: &mut impl Io) {
        if self.cfg.reconnect_only {
            self.phase = Phase::Waiting;
            return;
        }
        self.stats.rounds += 1;
        let round = self.stats.rounds;
        self.status(io, format_args!("Searching (round {})", round));
        self.cmd(io, op::INQUIRY, &params::inquiry(self.cfg.inquiry_units));
        let len = u64::from(self.cfg.inquiry_units) * 1280;
        self.phase = Phase::Inquiry {
            until: now + len + INQUIRY_GUARD_EXTRA,
        };
    }

    fn back_to_search(&mut self, now: u64, io: &mut impl Io) {
        let _ = io;
        self.cand = None;
        self.early_role = None;
        self.phase = Phase::Pause {
            until: now + self.cfg.search_pause_ms,
        };
        // The call to the chosen pad failed: the window ends.
        #[cfg(feature = "pair")]
        if self.target().is_some() {
            self.end_window(now, PairEnd::Lost, io);
        }
    }

    fn on_found(&mut self, now: u64, f: Found<'_>, io: &mut impl Io) {
        // In a pairing window nobody is called on being heard; the
        // window code lists gamepads for the user instead.
        #[cfg(feature = "pair")]
        if self.window_open() {
            self.window_found(now, f, io);
            return;
        }
        let eir = hci::parse_eir(f.eir);
        let gamepad = hci::is_gamepad(f.cod, eir.name);
        let major = hci::cod_major(f.cod);
        if gamepad && !allowed(self.cfg.only, f.addr, eir.name) {
            // Someone else's gamepad, maybe: no address, no name.
            self.stats.found_skipped += 1;
            log!(
                io,
                Hci,
                "found gamepad not on the allow-list cod={:#08x} rssi={} (not called; address and name not logged)",
                f.cod,
                f.rssi.map_or(0, i32::from)
            );
            return;
        }
        if gamepad {
            self.stats.found_gamepads += 1;
            log!(
                io,
                Hci,
                "found gamepad addr={} cod={:#08x} minor={:#04x} rssi={} psrm={} name=\"{}\" eir_hid_uuid={} device_id={:04x?}",
                f.addr,
                f.cod,
                hci::cod_minor(f.cod),
                f.rssi.map_or(0, i32::from),
                f.psrm,
                Printable(eir.name),
                u8::from(eir.hid),
                eir.device_id
            );
        } else {
            // Not ours to care about: no address, no name in the log.
            self.stats.found_other += 1;
            log!(
                io,
                Hci,
                "found other cod={:#08x} major={:#04x}({}) rssi={} (not a gamepad, address not logged)",
                f.cod,
                major,
                hci::major_text(major),
                f.rssi.map_or(0, i32::from)
            );
        }
        if gamepad
            && matches!(self.phase, Phase::Inquiry { .. })
            && self.link.is_none()
            && self.cand.is_none()
        {
            let name = Text::from_device_bytes(eir.name);
            self.cand = Some(Candidate {
                addr: f.addr,
                psrm: f.psrm,
                clock: Some(f.clock_offset),
                cod: f.cod,
                name,
                purpose: Purpose::Gamepad,
                found_at: Some(now),
            });
            self.status(
                io,
                format_args!(
                    "Found: {}",
                    if name.is_empty() {
                        "gamepad"
                    } else {
                        name.as_str()
                    }
                ),
            );
            self.ui_device(io);
            self.cmd(io, op::INQUIRY_CANCEL, &[]);
            self.phase = Phase::Cancelling { since: now };
        }
    }

    fn start_page(&mut self, now: u64, io: &mut impl Io) {
        // Reconnect-only calls nobody, except a pairing window's target.
        if self.cfg.reconnect_only
            && !self
                .cand
                .is_some_and(|c| Some(c.addr) == self.target() && c.purpose == Purpose::Gamepad)
        {
            self.back_to_search(now, io);
            return;
        }
        let Some(c) = self.cand else {
            self.back_to_search(now, io);
            return;
        };
        self.status(
            io,
            format_args!(
                "Connecting to {}",
                if c.name.is_empty() {
                    "gamepad"
                } else {
                    c.name.as_str()
                }
            ),
        );
        self.cmd(
            io,
            op::CREATE_CONNECTION,
            &params::create_connection(c.addr, c.psrm, c.clock),
        );
        self.phase = Phase::Paging { since: now };
    }

    fn known(&self, addr: BdAddr) -> bool {
        self.known.iter().flatten().any(|a| *a == addr) || self.key(addr).is_some()
    }

    fn remember(&mut self, addr: BdAddr) {
        if self.known(addr) {
            return;
        }
        if let Some(slot) = self.known.iter_mut().find(|k| k.is_none()) {
            *slot = Some(addr);
        } else {
            self.known.rotate_left(1);
            self.known[KEYS - 1] = Some(addr);
        }
    }

    fn key(&self, addr: BdAddr) -> Option<LinkKey> {
        self.keys
            .iter()
            .flatten()
            .find(|(a, _, _)| *a == addr)
            .map(|(_, k, _)| *k)
    }

    /// Keeps a new key (the newest goes last; a fifth pushes the oldest
    /// out) and reports both changes to the caller.
    fn store_key(&mut self, addr: BdAddr, key: LinkKey, key_type: u8, io: &mut impl Io) {
        if self.cfg.reconnect_only {
            // The table is read-only in this mode (never reached: a new
            // key means pairing, which is refused before).
            return;
        }
        self.remove_key(addr);
        // No gaps: the table is full when its last place is taken.
        if let (Some(_), Some((old, _, _))) = (self.keys[KEYS - 1], self.keys[0]) {
            self.remove_key(old);
            log!(
                io,
                State,
                "pairing table full: the oldest pairing makes room"
            );
            io.key_change(KeyChange::Forget {
                addr: old,
                why: ForgetReason::Evicted,
            });
        }
        if let Some(slot) = self.keys.iter_mut().find(|k| k.is_none()) {
            *slot = Some((addr, key, key_type));
        }
        io.key_change(KeyChange::Save {
            addr,
            key,
            key_type,
        });
    }

    /// Drops the key of `addr` and reports it, if there was one.
    fn forget_key(&mut self, addr: BdAddr, why: ForgetReason, io: &mut impl Io) {
        if self.cfg.reconnect_only {
            if self.key(addr).is_some() {
                log!(
                    io,
                    State,
                    "saved key kept (reconnect-only: the table is read-only; reason would be {})",
                    why.word()
                );
            }
            return;
        }
        if self.remove_key(addr) {
            io.key_change(KeyChange::Forget { addr, why });
        }
    }

    /// Takes the key of `addr` out of the table, keeping the order.
    fn remove_key(&mut self, addr: BdAddr) -> bool {
        let Some(i) = self
            .keys
            .iter()
            .position(|k| matches!(k, Some((a, _, _)) if *a == addr))
        else {
            return false;
        };
        self.keys[i] = None;
        self.keys[i..].rotate_left(1);
        true
    }

    // ------------------------------------------------------------ events

    fn on_event(&mut self, now: u64, ev: Event<'_>, io: &mut impl Io) {
        match ev {
            Event::CommandComplete {
                credits,
                opcode,
                ret,
            } => {
                self.cmd_credits = credits;
                if opcode == 0 {
                    return;
                }
                if matches!(self.cmd_wait, Some((o, _)) if o == opcode) {
                    self.cmd_wait = None;
                }
                let status = ret.first().copied().unwrap_or(0);
                // Answers that carry an address (after the status byte):
                // only its last two bytes are shown.
                let addr = if starts_with_address(opcode) || opcode == op::READ_BD_ADDR {
                    3..7
                } else {
                    0..0
                };
                log!(
                    io,
                    Hci,
                    "done {} op={:#06x} status={:#04x}({}) ret={}",
                    op::name(opcode),
                    opcode,
                    status,
                    hci::status_text(status),
                    HexMask(&ret[..ret.len().min(24)], addr)
                );
                self.on_command_complete(now, opcode, status, ret, io);
            }
            Event::CommandStatus {
                status,
                credits,
                opcode,
            } => {
                self.cmd_credits = credits;
                if opcode == 0 {
                    return;
                }
                if matches!(self.cmd_wait, Some((o, _)) if o == opcode) {
                    self.cmd_wait = None;
                }
                log!(
                    io,
                    Hci,
                    "status {} op={:#06x} status={:#04x}({})",
                    op::name(opcode),
                    opcode,
                    status,
                    hci::status_text(status)
                );
                if status != 0 {
                    self.on_command_failed(now, opcode, status, io);
                }
            }
            Event::InquiryComplete { status } => {
                log!(
                    io,
                    Hci,
                    "inquiry-complete status={:#04x}({}) round={}",
                    status,
                    hci::status_text(status),
                    self.stats.rounds
                );
                match self.phase {
                    Phase::Inquiry { .. } => {
                        self.phase = Phase::Pause {
                            until: now + self.cfg.search_pause_ms,
                        }
                    }
                    Phase::Cancelling { .. } => self.start_page(now, io),
                    _ => {}
                }
            }
            Event::InquiryResult(r) => {
                for i in 0..r.n {
                    if let Some(f) = r.get(i) {
                        self.on_found(now, f, io);
                    }
                }
            }
            Event::ConnectionRequest {
                addr,
                cod,
                link_type,
            } => self.on_connection_request(now, addr, cod, link_type, io),
            Event::ConnectionComplete {
                status,
                handle,
                addr,
                link_type,
                encryption,
            } => {
                let ours = self.cand.is_some_and(|c| c.addr == addr);
                if ours || status == 0 {
                    log!(
                        io,
                        Hci,
                        "connection-complete status={:#04x}({}) handle={:#05x} addr={} link_type={} encryption={}",
                        status,
                        hci::status_text(status),
                        handle,
                        addr,
                        link_type,
                        encryption
                    );
                } else {
                    log!(
                        io,
                        Hci,
                        "connection-complete status={:#04x}({}) (not our call, address not logged)",
                        status,
                        hci::status_text(status)
                    );
                }
                if link_type != 1 {
                    return;
                }
                if ours && matches!(self.phase, Phase::Paging { .. } | Phase::Accepting { .. }) {
                    if status == 0 {
                        self.on_link_up(now, handle, io);
                    } else {
                        self.status(
                            io,
                            format_args!("Connect failed: {}", hci::status_text(status)),
                        );
                        self.back_to_search(now, io);
                    }
                } else if status == 0 {
                    self.err(
                        io,
                        format_args!(
                            "a link we did not expect (handle {:#05x}); closing it",
                            handle
                        ),
                    );
                    self.cmd(
                        io,
                        op::DISCONNECT,
                        &params::handle_u8(handle, REASON_USER_ENDED),
                    );
                }
            }
            Event::DisconnectionComplete {
                status,
                handle,
                reason,
            } => {
                log!(
                    io,
                    Hci,
                    "disconnection-complete status={:#04x} handle={:#05x} reason={:#04x}({})",
                    status,
                    handle,
                    reason,
                    hci::status_text(reason)
                );
                if status == 0
                    && let Some(st) = self.stale.filter(|st| st.handle == handle)
                {
                    log!(
                        io,
                        State,
                        "old link closed by the controller {} ms after we dropped it (data that still came for it: {})",
                        now.saturating_sub(st.since),
                        st.acl
                    );
                    self.stale = None;
                } else if status == 0 && self.link.is_some_and(|l| l.handle == handle) {
                    self.on_link_down(now, reason, io);
                }
            }
            Event::AuthenticationComplete { status, handle } => {
                log!(
                    io,
                    Hci,
                    "authentication-complete status={:#04x}({}) handle={:#05x}",
                    status,
                    hci::status_text(status),
                    handle
                );
                self.on_auth_complete(now, status, handle, io);
            }
            Event::EncryptionChange {
                status,
                handle,
                enabled,
            } => {
                log!(
                    io,
                    Hci,
                    "encryption-change status={:#04x}({}) handle={:#05x} enabled={}",
                    status,
                    hci::status_text(status),
                    handle,
                    enabled
                );
                self.on_encryption(now, status, handle, enabled, io);
            }
            Event::RemoteNameComplete { status, addr, name } => {
                if self.link.is_some_and(|l| l.addr == addr) {
                    log!(
                        io,
                        Hci,
                        "remote-name status={:#04x}({}) name=\"{}\"",
                        status,
                        hci::status_text(status),
                        Printable(name)
                    );
                    if status == 0 && !name.is_empty() {
                        if let Some(l) = self.link.as_mut() {
                            l.name = Text::from_device_bytes(name);
                        }
                        self.ui_device(io);
                    }
                } else {
                    log!(
                        io,
                        Hci,
                        "remote-name status={:#04x} (another device, not logged)",
                        status
                    );
                }
            }
            Event::RemoteFeatures {
                status,
                handle,
                features: f,
            } => {
                log!(
                    io,
                    Hci,
                    "remote-features status={:#04x} handle={:#05x} bytes={} encryption={} role_switch={} sniff={} edr2={} edr3={} eir={} ssp={} extended={}",
                    status,
                    handle,
                    Hex(&f),
                    u8::from(f[0] & 0x04 != 0),
                    u8::from(f[0] & 0x20 != 0),
                    u8::from(f[0] & 0x80 != 0),
                    u8::from(f[3] & 0x02 != 0),
                    u8::from(f[3] & 0x04 != 0),
                    u8::from(f[6] & 0x01 != 0),
                    u8::from(f[6] & 0x08 != 0),
                    u8::from(f[7] & 0x80 != 0)
                );
                if status == 0 && f[7] & 0x80 != 0 {
                    self.cmd(
                        io,
                        op::READ_REMOTE_EXTENDED_FEATURES,
                        &params::handle_u8(handle, 1),
                    );
                }
            }
            Event::RemoteExtFeatures {
                status,
                handle,
                page,
                max_page,
                features: f,
            } => {
                log!(
                    io,
                    Hci,
                    "remote-ext-features status={:#04x} handle={:#05x} page={} max_page={} bytes={} ssp_host={} le_host={} sc_host={}",
                    status,
                    handle,
                    page,
                    max_page,
                    Hex(&f),
                    u8::from(page == 1 && f[0] & 0x01 != 0),
                    u8::from(page == 1 && f[0] & 0x02 != 0),
                    u8::from(page == 1 && f[0] & 0x08 != 0)
                );
            }
            Event::RemoteVersion {
                status,
                handle,
                version,
                manufacturer,
                subversion,
            } => {
                log!(
                    io,
                    Hci,
                    "remote-version status={:#04x} handle={:#05x} lmp_version={} manufacturer={:#06x} subversion={:#06x}",
                    status,
                    handle,
                    version,
                    manufacturer,
                    subversion
                );
            }
            Event::HardwareError { code } => self.err(
                io,
                format_args!("controller hardware error code={:#04x}", code),
            ),
            Event::RoleChange { status, addr, role } => {
                log!(
                    io,
                    Hci,
                    "role-change status={:#04x} addr={} role={}({})",
                    status,
                    addr,
                    role,
                    if role == 0 {
                        "we-are-central"
                    } else {
                        "we-are-peripheral"
                    }
                );
                if status == 0 {
                    self.on_role(addr, role == 0, io);
                }
            }
            Event::NumberOfCompletedPackets { n, data } => {
                for (handle, count) in Event::completed(n, data) {
                    self.credits.give_back(count);
                    self.on_completed(now, handle, count, io);
                }
            }
            Event::ModeChange {
                status,
                handle,
                mode,
                interval,
            } => {
                let m = match mode {
                    0 => "active",
                    1 => "hold",
                    2 => "sniff",
                    _ => "?",
                };
                log!(
                    io,
                    Hci,
                    "mode-change status={:#04x} handle={:#05x} mode={}({}) interval={} slots ({} us)",
                    status,
                    handle,
                    mode,
                    m,
                    interval,
                    u32::from(interval) * 625
                );
                if status == 0
                    && let Some(l) = self.link.as_mut()
                    && l.handle == handle
                {
                    l.sniff_slots = if mode == 2 { interval } else { 0 };
                    self.supervision(io);
                }
            }
            Event::PinCodeRequest { addr } => {
                self.err(
                    io,
                    format_args!("gamepad asks for old-style PIN pairing addr={}", addr),
                );
                let pin = self.cfg.legacy_pin.filter(|_| !self.cfg.reconnect_only);
                match (pin, self.link.map(|l| l.purpose)) {
                    (Some(pin), Some(Purpose::Gamepad)) => self.cmd(
                        io,
                        op::PIN_CODE_REQUEST_REPLY,
                        &params::pin_code_reply(addr, pin),
                    ),
                    _ => self.cmd(io, op::PIN_CODE_REQUEST_NEGATIVE_REPLY, &params::addr(addr)),
                }
                // The chosen pad wants old-style PIN pairing: never.
                #[cfg(feature = "pair")]
                self.window_old_pin(now, addr, "PIN", io);
                self.refuse_pairing(io, "PIN", addr);
            }
            Event::LinkKeyRequest { addr } => {
                let purpose = self.link.filter(|l| l.addr == addr).map(|l| l.purpose);
                // The pad a pairing window pairs with gets no saved key,
                // even when one is saved: in pairing mode it has dropped
                // its own, and the negative answer goes straight on to
                // Secure Simple Pairing. Once the new key has come, a
                // second request on that link gets the new key.
                let key = if self.target() == Some(addr) && !self.window_has_new_key(addr) {
                    None
                } else {
                    self.key(addr)
                };
                log!(
                    io,
                    Hci,
                    "link-key-request addr={} have_key={}",
                    addr,
                    u8::from(key.is_some() && purpose == Some(Purpose::Gamepad))
                );
                self.sec_event(now, Sec::Pairing);
                match (purpose, key) {
                    (Some(Purpose::Gamepad), Some(k)) => {
                        if let Some(l) = self.link.as_mut() {
                            l.key_offered = true;
                        }
                        self.cmd(
                            io,
                            op::LINK_KEY_REQUEST_REPLY,
                            &params::link_key_reply(addr, &k.0),
                        );
                    }
                    _ => self.cmd(io, op::LINK_KEY_REQUEST_NEGATIVE_REPLY, &params::addr(addr)),
                }
            }
            Event::LinkKeyNotification {
                addr,
                key,
                key_type,
            } => {
                // The key itself is never printed.
                #[cfg(feature = "pair")]
                if self.target() == Some(addr)
                    && self
                        .link
                        .is_some_and(|l| l.addr == addr && l.purpose == Purpose::Gamepad)
                {
                    self.window_new_key(now, addr, key, key_type, io);
                    return;
                }
                let gamepad = self
                    .link
                    .is_some_and(|l| l.addr == addr && l.purpose == Purpose::Gamepad)
                    && !self.cfg.reconnect_only;
                log!(
                    io,
                    Hci,
                    "link-key-notification addr={} key_type={:#04x}({}) stored={} (key never printed; a stored key also goes to the firmware's flash store)",
                    addr,
                    key_type,
                    key_type_text(key_type),
                    u8::from(gamepad)
                );
                if gamepad {
                    self.store_key(addr, key, key_type, io);
                    if let Some(l) = self.link.as_mut() {
                        l.new_key = true;
                    }
                }
                self.sec_event(now, Sec::Pairing);
                // A new key means a new pairing: not in this mode.
                self.refuse_pairing(io, "new link key", addr);
            }
            Event::IoCapabilityRequest { addr } => {
                let purpose = self.link.filter(|l| l.addr == addr).map(|l| l.purpose);
                log!(io, Hci, "io-capability-request addr={}", addr);
                self.sec_event(now, Sec::Pairing);
                if purpose == Some(Purpose::Gamepad)
                    && (!self.cfg.reconnect_only || self.window_pairs(addr))
                {
                    self.cmd(
                        io,
                        op::IO_CAPABILITY_REQUEST_REPLY,
                        &params::io_capability(
                            addr,
                            IO_CAP_NO_INPUT_NO_OUTPUT,
                            AUTH_REQ_GENERAL_BONDING,
                        ),
                    );
                } else {
                    // 0x18: pairing not allowed.
                    self.cmd(
                        io,
                        op::IO_CAPABILITY_REQUEST_NEGATIVE_REPLY,
                        &params::addr_u8(addr, 0x18),
                    );
                }
                self.refuse_pairing(io, "secure simple pairing", addr);
            }
            Event::IoCapabilityResponse {
                addr,
                io_cap,
                oob,
                auth_req,
            } => {
                log!(
                    io,
                    Hci,
                    "io-capability-response addr={} io_cap={}({}) oob={} auth_req={:#04x}({})",
                    addr,
                    io_cap,
                    io_cap_text(io_cap),
                    oob,
                    auth_req,
                    auth_req_text(auth_req)
                );
                self.sec_event(now, Sec::Pairing);
            }
            Event::UserConfirmationRequest { addr, value } => {
                let gamepad = self
                    .link
                    .is_some_and(|l| l.addr == addr && l.purpose == Purpose::Gamepad)
                    && (!self.cfg.reconnect_only || self.window_pairs(addr));
                log!(
                    io,
                    Hci,
                    "user-confirmation-request addr={} value={:06} answer={}",
                    addr,
                    value,
                    if gamepad { "yes (just works)" } else { "no" }
                );
                self.sec_event(now, Sec::Pairing);
                if gamepad {
                    self.cmd(io, op::USER_CONFIRMATION_REQUEST_REPLY, &params::addr(addr));
                } else {
                    self.cmd(
                        io,
                        op::USER_CONFIRMATION_REQUEST_NEGATIVE_REPLY,
                        &params::addr(addr),
                    );
                }
                self.refuse_pairing(io, "user confirmation", addr);
            }
            Event::UserPasskeyRequest { addr } => {
                self.err(
                    io,
                    format_args!("asked to type a passkey (we have no keys) addr={}", addr),
                );
                self.cmd(
                    io,
                    op::USER_PASSKEY_REQUEST_NEGATIVE_REPLY,
                    &params::addr(addr),
                );
                #[cfg(feature = "pair")]
                self.window_old_pin(now, addr, "passkey", io);
                self.refuse_pairing(io, "passkey", addr);
            }
            Event::SimplePairingComplete { status, addr } => {
                log!(
                    io,
                    Hci,
                    "simple-pairing-complete status={:#04x}({}) addr={}",
                    status,
                    hci::status_text(status),
                    addr
                );
                if status != 0 {
                    self.err(
                        io,
                        format_args!(
                            "pairing failed status={:#04x}({})",
                            status,
                            hci::status_text(status)
                        ),
                    );
                }
                self.sec_event(now, Sec::Pairing);
            }
            Event::EncryptionKeyRefreshComplete { status, handle } => {
                log!(
                    io,
                    Hci,
                    "encryption-key-refresh status={:#04x} handle={:#05x}",
                    status,
                    handle
                );
            }
            Event::MaxSlotsChange { handle, max_slots } => log!(
                io,
                Hci,
                "max-slots-change handle={:#05x} max_slots={}",
                handle,
                max_slots
            ),
            Event::LinkSupervisionTimeoutChanged { handle, timeout } => {
                log!(
                    io,
                    Hci,
                    "link-supervision-timeout handle={:#05x} timeout={} slots ({} ms)",
                    handle,
                    timeout,
                    u32::from(timeout) * 625 / 1000
                );
            }
            Event::PageScanRepetitionModeChange { addr, psrm } => {
                log!(io, Hci, "page-scan-mode-change addr={} psrm={}", addr, psrm)
            }
            Event::RemoteHostFeatures { addr, features } => log!(
                io,
                Hci,
                "remote-host-features addr={} bytes={}",
                addr,
                Hex(&features)
            ),
            Event::DataBufferOverflow { link_type } => self.err(
                io,
                format_args!("controller data buffer overflow link_type={}", link_type),
            ),
            Event::LeMeta { .. } => {}
            // Two kinds of event carry a link key: "return link keys"
            // (0x15) and "link key notification" (0x18, here only when
            // it was too short to read). Their bytes are never printed.
            Event::Unknown { code, params } if matches!(code, 0x15 | 0x18) => log!(
                io,
                Hci,
                "event code={:#04x} len={} data=(hidden: it may hold a link key)",
                code,
                params.len()
            ),
            // Other events may carry an address: their bytes are not shown.
            Event::Unknown { code, params } => log!(
                io,
                Hci,
                "event code={:#04x} len={} data=(not shown: it may hold an address)",
                code,
                params.len()
            ),
        }
    }

    /// The controller sent `count` more of our ACL packets on `handle`.
    /// Each output report whose last packet is now sent gets a line: proof
    /// that it left our side (not that the gamepad took it).
    fn on_completed(&mut self, now: u64, handle: u16, count: u16, io: &mut impl Io) {
        let Some(l) = self.link.as_mut() else { return };
        if handle != l.handle {
            return;
        }
        l.acl_done += u32::from(count);
        let done = l.acl_done;
        let mut k = 0;
        while k < l.outs_n {
            let (seq, n, set_report, at) = l.outs[k];
            if seq <= done {
                if n <= OUTPUTS_LOGGED {
                    log!(
                        io,
                        Hci,
                        "completed {} n={} after {} ms: our controller sent it (handle={:#05x} acl_done={})",
                        if set_report { "set-report" } else { "output" },
                        n,
                        now.saturating_sub(at),
                        handle,
                        done
                    );
                }
                l.outs.copy_within(k + 1.., k);
                l.outs_n -= 1;
            } else {
                k += 1;
            }
        }
    }

    fn on_command_complete(
        &mut self,
        now: u64,
        opcode: u16,
        status: u8,
        ret: &[u8],
        io: &mut impl Io,
    ) {
        if status != 0 {
            self.on_command_failed(now, opcode, status, io);
            return;
        }
        match opcode {
            op::READ_BUFFER_SIZE if ret.len() >= 8 => {
                let acl_len = u16_at(ret, 1).unwrap_or(0);
                let acl_num = u16_at(ret, 4).unwrap_or(0);
                self.acl_max = usize::from(acl_len).clamp(1, acl::MAX_FRAGMENT);
                self.credits.set_total(acl_num);
                log!(
                    io,
                    Hci,
                    "buffers acl_len={} acl_num={} sco_len={} sco_num={} we_send_at_most={}",
                    acl_len,
                    acl_num,
                    ret[3],
                    u16_at(ret, 6).unwrap_or(0),
                    self.acl_max
                );
            }
            op::READ_LOCAL_VERSION if ret.len() >= 9 => {
                log!(
                    io,
                    Hci,
                    "local-version hci={} hci_rev={:#06x} lmp={} manufacturer={:#06x} lmp_sub={:#06x}",
                    ret[1],
                    u16_at(ret, 2).unwrap_or(0),
                    ret[4],
                    u16_at(ret, 5).unwrap_or(0),
                    u16_at(ret, 7).unwrap_or(0)
                );
            }
            op::READ_BD_ADDR if ret.len() >= 7 => {
                let mut a = [0; 6];
                a.copy_from_slice(&ret[1..7]);
                log!(io, Hci, "own-address {}", BdAddr(a));
            }
            op::WRITE_SCAN_ENABLE if self.phase == Phase::Init => {
                self.status(io, format_args!("Radio ready"));
                self.phase = Phase::Pause { until: now };
            }
            op::INQUIRY_CANCEL if matches!(self.phase, Phase::Cancelling { .. }) => {
                self.start_page(now, io)
            }
            _ => {}
        }
    }

    /// A command refused (status) or not answered (0xFF).
    fn on_command_failed(&mut self, now: u64, opcode: u16, status: u8, io: &mut impl Io) {
        self.err(
            io,
            format_args!(
                "command {} op={:#06x} failed status={:#04x}({})",
                op::name(opcode),
                opcode,
                status,
                hci::status_text(status)
            ),
        );
        match opcode {
            op::INQUIRY if matches!(self.phase, Phase::Inquiry { .. }) => {
                self.phase = Phase::Pause { until: now + 2_000 }
            }
            // The inquiry may have ended by itself just before.
            op::INQUIRY_CANCEL if matches!(self.phase, Phase::Cancelling { .. }) => {
                self.start_page(now, io)
            }
            op::CREATE_CONNECTION | op::ACCEPT_CONNECTION_REQUEST
                if matches!(self.phase, Phase::Paging { .. } | Phase::Accepting { .. }) =>
            {
                self.status(
                    io,
                    format_args!("Connect failed: {}", hci::status_text(status)),
                );
                self.back_to_search(now, io);
            }
            op::WRITE_SCAN_ENABLE if self.phase == Phase::Init => {
                self.status(io, format_args!("Radio set-up incomplete"));
                self.phase = Phase::Pause { until: now };
            }
            // Not taken (we are not the central after all, or the link is
            // gone): the controller's own timeout stays; the pad host's
            // silence guard still notices a pad that went quiet. No retry.
            op::WRITE_LINK_SUPERVISION_TIMEOUT => {
                log!(
                    io,
                    State,
                    "supervision timeout not taken (status {:#04x}); the controller keeps its own; no retry",
                    status
                );
            }
            // Refused at once (often because the gamepad's own pairing is
            // under way): the gamepad's events, or one later try, go on.
            op::AUTHENTICATION_REQUESTED => {
                if let Some(m) = self.link.as_mut()
                    && m.purpose == Purpose::Gamepad
                    && !m.auth_retried
                {
                    m.auth_retried = true;
                    m.we_asked_auth = false;
                    if m.sec == Sec::Pairing {
                        m.sec = Sec::None;
                    }
                    m.auth_at = Some(now + COLLISION_RETRY);
                }
            }
            _ => {}
        }
    }

    fn on_connection_request(
        &mut self,
        now: u64,
        addr: BdAddr,
        cod: u32,
        link_type: u8,
        io: &mut impl Io,
    ) {
        // A pairing window is open: nobody may call in, saved pads
        // included (the host has room for one link, and the chosen pad is
        // called by us). 0x0D, "no room now": a saved pad calls again
        // after the window.
        if self.window_open() {
            log!(
                io,
                Hci,
                "connection-request cod={:#08x} link_type={} refused: a pairing window is open (address not logged)",
                cod,
                link_type
            );
            self.cmd(
                io,
                op::REJECT_CONNECTION_REQUEST,
                &params::addr_u8(addr, 0x0D),
            );
            return;
        }
        // The pad we are linked to calls again: for it, the old link is
        // gone (a device has one link to us at most; it was switched off
        // and on before either side noticed). Drop the old link and take
        // the new call, so the console is never stuck on a dead link.
        if link_type == 1
            && self.cfg.reconnect_only
            && self.key(addr).is_some()
            && self
                .link
                .is_some_and(|l| l.addr == addr && l.purpose == Purpose::Gamepad)
            && self.phase == Phase::Linked
        {
            log!(
                io,
                State,
                "the linked pad calls again: its old link is dead; dropping it and taking the new call"
            );
            self.drop_link(now, REASON_LOCAL_HOST, io);
        }
        let busy = self.link.is_some()
            || matches!(
                self.phase,
                Phase::Paging { .. }
                    | Phase::Accepting { .. }
                    | Phase::Cancelling { .. }
                    | Phase::Init
            );
        // Reconnect-only: a saved key is the only way in.
        let welcome = if self.cfg.reconnect_only {
            self.key(addr).is_some()
        } else {
            (hci::is_peripheral(cod) && allowed_call(self.cfg.only, addr)) || self.known(addr)
        };
        if self.cfg.reconnect_only && !welcome {
            log!(
                io,
                Hci,
                "connection-request cod={:#08x} link_type={} refused: no saved key for this address (reconnect-only; address not logged)",
                cod,
                link_type
            );
        } else if welcome {
            log!(
                io,
                Hci,
                "connection-request addr={} cod={:#08x} link_type={} known={} busy={}",
                addr,
                cod,
                link_type,
                u8::from(self.known(addr)),
                u8::from(busy)
            );
        } else {
            log!(
                io,
                Hci,
                "connection-request cod={:#08x} major={:#04x}({}) link_type={} refused (not a peripheral or not on the allow-list; address not logged)",
                cod,
                hci::cod_major(cod),
                hci::major_text(hci::cod_major(cod)),
                link_type
            );
        }
        if link_type != 1 || !welcome || busy {
            // 0x0F: unacceptable address; 0x0D: no room now.
            let reason = if welcome { 0x0D } else { 0x0F };
            self.cmd(
                io,
                op::REJECT_CONNECTION_REQUEST,
                &params::addr_u8(addr, reason),
            );
            return;
        }
        self.cmd(
            io,
            op::ACCEPT_CONNECTION_REQUEST,
            &params::addr_u8(addr, self.cfg.accept_role),
        );
        if matches!(self.phase, Phase::Inquiry { .. }) {
            self.cmd(io, op::INQUIRY_CANCEL, &[]);
        }
        self.cand = Some(Candidate {
            addr,
            psrm: 0x01,
            clock: None,
            cod,
            name: Text::new(),
            purpose: Purpose::Gamepad,
            found_at: None,
        });
        self.phase = Phase::Accepting { since: now };
        self.status(io, format_args!("Gamepad is calling"));
        self.ui_device(io);
    }

    fn on_link_up(&mut self, now: u64, handle: u16, io: &mut impl Io) {
        let Some(c) = self.cand else { return };
        let incoming = matches!(self.phase, Phase::Accepting { .. });
        let early_central = matches!(self.early_role.take(), Some((a, true)) if a == c.addr);
        if self.stale.is_some_and(|s| s.handle == handle) {
            // The controller gave the old handle out again: the old link
            // is surely closed.
            self.stale = None;
        }
        self.stats.links += 1;
        self.phase = Phase::Linked;
        self.reasm.reset();
        self.chans = [FREE_CHAN; CHANNELS];
        self.sdp_len = 0;
        self.link = Some(Link {
            handle,
            addr: c.addr,
            cod: c.cod,
            name: c.name,
            incoming,
            purpose: c.purpose,
            sec: Sec::None,
            we_asked_auth: false,
            auth_retried: false,
            auth_at: None,
            enc_retried: false,
            enc_at: None,
            last_sec_event: now,
            sec_deadline: now + SECURITY_TIMEOUT,
            sdp: SdpState::Idle,
            sdp_query: 0,
            sdp_tid: 0,
            sdp_deadline: 0,
            sdp_cont: [0; 17],
            sdp_cont_len: 0,
            sdp_pieces: 0,
            pnp: sdp::PnpInfo::default(),
            open_ctrl_at: None,
            open_intr_at: None,
            hid_deadline: None,
            hid_up: false,
            info_req: None,
            reports: 0,
            reports_second: 0,
            report_log_at: now + 1_000,
            outputs: 0,
            set_reports: 0,
            handshakes: 0,
            set_report_wait: None,
            get_report_wait: None,
            acl_queued: 0,
            acl_done: 0,
            outs: [(0, 0, false, 0); OUTS_WATCHED],
            outs_n: 0,
            disconnect_at: None,
            disconnect_sent: false,
            disconnect_seen_at: None,
            found_at: c.found_at,
            up_at: now,
            hid_up_at: None,
            first_report_at: None,
            last_report_at: now,
            key_offered: false,
            new_key: false,
            auth_ok: false,
            desc_bytes: 0,
            ids: [IdCount::default(); IDS_COUNTED],
            ids_n: 0,
            flowing_logged: false,
            no_report_logged: false,
            stopped_logged: false,
            // We called: we are central. It called: we accepted with
            // `accept_role` (0x00 asks to become central, which the role
            // change confirms) and stay peripheral until a role change.
            central: !incoming || early_central,
            lsto_wish: None,
            sniff_slots: 0,
            lsto_sent: None,
        });
        if c.purpose == Purpose::Gamepad {
            self.remember(c.addr);
        }
        // A pairing window's link has SECURITY_TIMEOUT at most.
        #[cfg(feature = "pair")]
        self.pair.link_up(now, c.addr, SECURITY_TIMEOUT);
        self.status(
            io,
            format_args!(
                "Linked ({}), {}",
                if incoming { "it called" } else { "we called" },
                if c.purpose == Purpose::Gamepad {
                    "pairing"
                } else {
                    "probe, no pairing"
                }
            ),
        );
        self.ui_device(io);
        self.supervision(io);
        self.cmd(
            io,
            op::READ_REMOTE_SUPPORTED_FEATURES,
            &params::handle(handle),
        );
        self.cmd(
            io,
            op::READ_REMOTE_VERSION_INFORMATION,
            &params::handle(handle),
        );
        if c.name.is_empty() && c.purpose == Purpose::Gamepad {
            self.cmd(
                io,
                op::REMOTE_NAME_REQUEST,
                &params::remote_name(c.addr, c.psrm, c.clock),
            );
        }
        // What L2CAP features does it have? (Only for the log.)
        let id = self.new_id();
        if let Some(l) = self.link.as_mut() {
            l.info_req = Some((id, now + SIGNAL_TIMEOUT));
        }
        self.send_signal(io, code::INFORMATION_REQUEST, id, &0x0002_u16.to_le_bytes());
        match c.purpose {
            Purpose::Gamepad if incoming => {
                if let Some(l) = self.link.as_mut() {
                    l.auth_at = Some(now + INCOMING_AUTH_DELAY);
                }
            }
            Purpose::Gamepad => self.request_auth(io),
            Purpose::SdpProbe { .. } => self.start_sdp(now, io),
        }
    }

    fn on_link_down(&mut self, now: u64, reason: u8, io: &mut impl Io) {
        let Some(link) = self.link.take() else { return };
        if link.purpose == Purpose::Gamepad {
            self.summary(now, &link, Some(reason), io);
        }
        // Packets for a link that is gone are thrown away by the
        // controller and their buffers count as free again.
        self.credits.give_back(self.credits.in_flight());
        self.aclq.clear();
        self.chans = [FREE_CHAN; CHANNELS];
        self.reasm.reset();
        if reason == REASON_AUTH_FAILURE
            && self.key(link.addr).is_some()
            && !self.cfg.reconnect_only
        {
            log!(
                io,
                State,
                "forgetting the link key after an authentication failure"
            );
            self.forget_key(link.addr, ForgetReason::AuthFailed, io);
        }
        // The gamepad called us, we offered its saved key, and it hung up
        // before encryption came on. Maybe it no longer has that key,
        // maybe it was just switched off. Keep the key (never erase it on
        // a plain hang-up) and call the gamepad back once: on our own call
        // the key is either accepted, or refused with a clear answer
        // (then it is forgotten and pairing starts again), or the chip
        // pairs anew at once.
        let call_back = link.purpose == Purpose::Gamepad
            && link.incoming
            && link.key_offered
            && !link.new_key
            && link.sec != Sec::Encrypted
            && reason == REASON_USER_ENDED
            && self.key(link.addr).is_some()
            && self.called_back != Some(link.addr)
            && self.forced.is_none()
            && !self.cfg.reconnect_only;
        if call_back {
            log!(
                io,
                State,
                "the gamepad hung up after we offered its saved key; keeping the key and calling it back once"
            );
            self.called_back = Some(link.addr);
            self.forced = Some((link.addr, Purpose::Gamepad, "saved gamepad"));
        }
        log!(
            io,
            State,
            "link down reason={:#04x}({}) reports={} pairing_state={:?} hid_up={}",
            reason,
            hci::status_text(reason),
            link.reports,
            link.sec,
            u8::from(link.hid_up)
        );
        io.ui(UiEvent::Disconnected);
        self.cand = None;
        self.phase = Phase::Pause {
            until: now + AFTER_LOSS_PAUSE,
        };
        self.status(io, format_args!("Lost: {}", hci::status_text(reason)));
        // The chosen pad's link ended before its new key was saved.
        #[cfg(feature = "pair")]
        if self.target() == Some(link.addr) {
            self.end_window(now, PairEnd::Lost, io);
        }
    }

    /// The console decided the pad is gone (for example a pad that streams
    /// reports went silent): the link ends here at once. A Disconnect goes
    /// to the controller for the old handle, the console sees the pad
    /// gone (`reason` in the log), and the host goes back to waiting: a
    /// new call from the pad is taken at once, whenever the controller
    /// hands it over. The old link's Disconnection Complete, whenever it
    /// comes (or never), changes nothing; data that still comes for it is
    /// dropped quietly. False when there was no link.
    pub fn drop_link(&mut self, now: u64, reason: u8, io: &mut impl Io) -> bool {
        let Some(l) = self.link else { return false };
        log!(
            io,
            State,
            "dropping the link here now (handle={:#05x}); the controller closes it in the background",
            l.handle
        );
        if !l.disconnect_sent {
            self.cmd(
                io,
                op::DISCONNECT,
                &params::handle_u8(l.handle, REASON_USER_ENDED),
            );
        }
        self.stale = Some(Stale {
            handle: l.handle,
            since: now,
            acl: 0,
        });
        self.on_link_down(now, reason, io);
        // Ready for the next call at once (no pause after a loss).
        if self.cfg.reconnect_only {
            self.phase = Phase::Waiting;
            self.status(io, format_args!("Waiting for a saved pad"));
        }
        self.pump(now, io);
        true
    }

    /// A role change on our link (or on the call being accepted, before
    /// its Connection Complete).
    fn on_role(&mut self, addr: BdAddr, central: bool, io: &mut impl Io) {
        match self.link.as_mut() {
            Some(l) if l.addr == addr => {
                if l.central != central {
                    // A new role: the timeout is ours to set again.
                    l.lsto_sent = None;
                }
                l.central = central;
                self.supervision(io);
            }
            Some(_) => {}
            None => {
                if matches!(self.phase, Phase::Accepting { .. })
                    && self.cand.is_some_and(|c| c.addr == addr)
                {
                    self.early_role = Some((addr, central));
                }
            }
        }
    }

    /// When we are the central of a gamepad's link: ask the controller for
    /// the supervision timeout [`supervision_slots`] gives, if it is not
    /// the one we asked for last. Once per change, never repeated.
    /// A queued supervision timeout still fits the link: the same
    /// handle, we are still central to a gamepad, and it is not ending.
    fn supervision_fits(&self, params: &[u8]) -> bool {
        let Some(l) = self.link else { return false };
        params.len() >= 2
            && u16::from_le_bytes([params[0], params[1]]) & 0x0FFF == l.handle
            && l.central
            && l.purpose == Purpose::Gamepad
            && !l.disconnect_sent
    }

    fn supervision(&mut self, io: &mut impl Io) {
        let Some(l) = self.link else { return };
        if !l.central || l.purpose != Purpose::Gamepad || l.disconnect_sent {
            return;
        }
        let want = supervision_slots(l.lsto_wish, l.sniff_slots);
        if l.lsto_sent == Some(want) {
            return;
        }
        if let Some(m) = self.link.as_mut() {
            m.lsto_sent = Some(want);
        }
        log!(
            io,
            State,
            "supervision timeout: {} slots ({} ms) for handle={:#05x} (the pad asked {}; sniff interval {} slots)",
            want,
            u32::from(want) * 625 / 1000,
            l.handle,
            Wish(l.lsto_wish),
            l.sniff_slots
        );
        self.cmd(
            io,
            op::WRITE_LINK_SUPERVISION_TIMEOUT,
            &params::handle_u16(l.handle, want),
        );
    }

    /// Reconnect-only mode: a pairing step came for `addr` (its answer is
    /// already queued). Say so and end the link. Nothing in other modes,
    /// and nothing when a pairing window pairs with `addr` or is linked
    /// to its target (a stray event for another address does not end the
    /// pairing under way; that address was already answered "no").
    fn refuse_pairing(&mut self, io: &mut impl Io, what: &str, addr: BdAddr) {
        if !self.cfg.reconnect_only {
            return;
        }
        if let Some(t) = self.target()
            && (t == addr || self.link.is_some_and(|l| l.addr == t))
        {
            return;
        }
        log!(
            io,
            State,
            "pairing refused ({}): reconnect-only mode takes saved pads only; ending the link",
            what
        );
        if self.link.is_some() {
            self.status(io, format_args!("Pairing refused"));
            self.disconnect(io, REASON_AUTH_FAILURE);
        }
    }

    fn disconnect(&mut self, io: &mut impl Io, reason: u8) {
        let Some(l) = self.link.as_mut() else { return };
        if l.disconnect_sent {
            return;
        }
        l.disconnect_sent = true;
        let h = l.handle;
        self.cmd(io, op::DISCONNECT, &params::handle_u8(h, reason));
    }

    // ---------------------------------------------------------- security

    fn sec_event(&mut self, now: u64, to: Sec) {
        if let Some(l) = self.link.as_mut() {
            l.last_sec_event = now;
            if l.sec == Sec::None {
                l.sec = to;
            }
        }
    }

    fn request_auth(&mut self, io: &mut impl Io) {
        let Some(l) = self.link.as_mut() else { return };
        if l.we_asked_auth || l.sec == Sec::Encrypted || l.sec == Sec::Encrypting {
            return;
        }
        l.we_asked_auth = true;
        l.auth_at = None;
        if l.sec == Sec::None {
            l.sec = Sec::Pairing;
        }
        let h = l.handle;
        self.status(io, format_args!("Pairing"));
        self.cmd(io, op::AUTHENTICATION_REQUESTED, &params::handle(h));
    }

    fn on_auth_complete(&mut self, now: u64, status: u8, handle: u16, io: &mut impl Io) {
        let Some(l) = self.link.filter(|l| l.handle == handle) else {
            return;
        };
        if l.purpose != Purpose::Gamepad {
            return;
        }
        if status == 0 {
            if let Some(m) = self.link.as_mut() {
                m.auth_ok = true;
            }
            if l.sec != Sec::Encrypted {
                if let Some(m) = self.link.as_mut() {
                    m.sec = Sec::Encrypting;
                    m.last_sec_event = now;
                }
                self.status(io, format_args!("Paired, encrypting"));
                self.cmd(
                    io,
                    op::SET_CONNECTION_ENCRYPTION,
                    &params::handle_u8(handle, 1),
                );
            }
            return;
        }
        // The pad a pairing window pairs with takes the normal path below:
        // one retry after a collision, else the pairing failed.
        let target = self.target() == Some(l.addr);
        // Reconnect-only: no second try, no new pairing, the key stays.
        if self.cfg.reconnect_only && !target {
            log!(
                io,
                State,
                "authentication failed ({}); reconnect-only: keeping the saved key, ending the link",
                hci::status_text(status)
            );
            self.status(
                io,
                format_args!("Saved key not accepted: {}", hci::status_text(status)),
            );
            self.disconnect(io, REASON_AUTH_FAILURE);
            return;
        }
        // A stored key the gamepad no longer has: forget it and pair again.
        // (Never for a window's target: it was offered no key.)
        if matches!(status, 0x05 | 0x06) && !l.auth_retried && !target {
            log!(
                io,
                State,
                "authentication failed ({}): dropping any stored key and pairing again",
                hci::status_text(status)
            );
            self.forget_key(l.addr, ForgetReason::AuthFailed, io);
            if let Some(m) = self.link.as_mut() {
                m.auth_retried = true;
                m.we_asked_auth = false;
                m.sec = Sec::None;
            }
            self.request_auth(io);
            return;
        }
        // Both sides asked at once: the gamepad's own attempt may still
        // finish (then its events cancel our retry); if not, try again.
        if is_collision(status) && !l.auth_retried {
            log!(
                io,
                State,
                "authentication collided with the gamepad's ({}); trying again in {} ms",
                hci::status_text(status),
                COLLISION_RETRY
            );
            if let Some(m) = self.link.as_mut() {
                m.auth_retried = true;
                m.we_asked_auth = false;
                m.sec = Sec::None;
                m.auth_at = Some(now + COLLISION_RETRY);
            }
            return;
        }
        self.status(
            io,
            format_args!("Pairing failed: {}", hci::status_text(status)),
        );
        self.disconnect(io, REASON_AUTH_FAILURE);
        #[cfg(feature = "pair")]
        if target {
            self.end_window(now, PairEnd::Refused, io);
        }
    }

    fn on_encryption(&mut self, now: u64, status: u8, handle: u16, enabled: u8, io: &mut impl Io) {
        let Some(l) = self.link.filter(|l| l.handle == handle) else {
            return;
        };
        if status == 0 && enabled != 0 {
            if l.sec != Sec::Encrypted {
                if let Some(m) = self.link.as_mut() {
                    m.sec = Sec::Encrypted;
                    m.auth_at = None;
                    m.enc_at = None;
                    // Encryption needs an accepted key. On a gamepad's
                    // call the gamepad checks the key itself and we get no
                    // authentication-complete, so this is where we learn
                    // that the saved key worked.
                    m.auth_ok = true;
                }
                if self.called_back == Some(l.addr) {
                    self.called_back = None;
                }
                // A pairing window's target: the new key is saved now (or
                // the link ends when no new key came).
                #[cfg(feature = "pair")]
                if !self.window_encrypted(now, l.addr, io) {
                    return;
                }
                self.on_secure(now, io);
            }
        } else if l.purpose == Purpose::Gamepad && l.sec == Sec::Encrypted && status != 0 {
            // A failed second attempt (ours, after the gamepad's own
            // succeeded): the link is still encrypted.
            log!(
                io,
                State,
                "an extra encryption request failed ({}); the link stays encrypted",
                hci::status_text(status)
            );
        } else if l.purpose == Purpose::Gamepad && is_collision(status) && !l.enc_retried {
            log!(
                io,
                State,
                "encryption collided with the gamepad's ({}); trying again in {} ms",
                hci::status_text(status),
                COLLISION_RETRY
            );
            if let Some(m) = self.link.as_mut() {
                m.enc_retried = true;
                m.enc_at = Some(now + COLLISION_RETRY);
                m.last_sec_event = now;
            }
        } else if l.purpose == Purpose::Gamepad {
            self.status(
                io,
                format_args!("Encryption failed: {}", hci::status_text(status)),
            );
            self.disconnect(io, REASON_AUTH_FAILURE);
            #[cfg(feature = "pair")]
            if self.target() == Some(l.addr) {
                self.end_window(now, PairEnd::Refused, io);
            }
        }
    }

    /// Encryption is on: answer the HID channel requests we held back,
    /// and read the description.
    fn on_secure(&mut self, now: u64, io: &mut impl Io) {
        self.status(io, format_args!("Encrypted, reading description"));
        for i in 0..CHANNELS {
            if self.chans[i].st == ChSt::Pending {
                let c = self.chans[i];
                self.conn_response(
                    io,
                    c.their_req_id,
                    c.local,
                    c.remote,
                    conn_result::SUCCESS,
                    conn_status::NO_INFO,
                );
                self.chans[i].st = ChSt::Config;
                self.send_config_request(now, io, i);
            }
        }
        if let Some(l) = self.link.as_mut() {
            l.hid_deadline = Some(now + SDP_TIMEOUT + HID_OPEN_TIMEOUT);
        }
        self.start_sdp(now, io);
    }

    // ------------------------------------------------------------ timers

    fn link_timers(&mut self, now: u64, io: &mut impl Io) {
        let Some(l) = self.link else { return };
        if l.disconnect_sent {
            match l.disconnect_seen_at {
                None => {
                    if let Some(m) = self.link.as_mut() {
                        m.disconnect_seen_at = Some(now);
                    }
                }
                Some(t) if now >= t + DISCONNECT_GUARD => {
                    self.err(io, format_args!("timeout: no disconnection-complete {} ms after disconnect; dropping the link here", DISCONNECT_GUARD));
                    self.on_link_down(now, REASON_LOCAL_HOST, io);
                    return;
                }
                Some(_) => {}
            }
        }
        if l.purpose == Purpose::Gamepad {
            if l.auth_at.is_some_and(|t| now >= t) {
                if let Some(m) = self.link.as_mut() {
                    m.auth_at = None;
                }
                if l.sec == Sec::None {
                    log!(
                        io,
                        State,
                        "security has not started {} ms after the link came up; asking for authentication ourselves",
                        now.saturating_sub(l.up_at)
                    );
                    self.request_auth(io);
                }
            }
            if l.enc_at.is_some_and(|t| now >= t) {
                if let Some(m) = self.link.as_mut() {
                    m.enc_at = None;
                }
                if l.sec != Sec::Encrypted {
                    log!(io, State, "asking for encryption again (after a collision)");
                    self.cmd(
                        io,
                        op::SET_CONNECTION_ENCRYPTION,
                        &params::handle_u8(l.handle, 1),
                    );
                }
            }
            // The gamepad started pairing, then nothing happened: ask for
            // authentication ourselves, which leads on to encryption.
            if l.sec == Sec::Pairing && !l.we_asked_auth && now >= l.last_sec_event + SECURITY_NUDGE
            {
                log!(
                    io,
                    State,
                    "security stalled for {} ms; asking for authentication ourselves",
                    SECURITY_NUDGE
                );
                self.request_auth(io);
            }
            if l.sec != Sec::Encrypted && now >= l.sec_deadline && !l.disconnect_sent {
                let awaited = match l.sec {
                    Sec::None => "the pairing to start (io-capability-request or link-key-request)",
                    Sec::Pairing => "authentication-complete",
                    Sec::Encrypting => "encryption-change",
                    Sec::Encrypted => "-",
                };
                self.err(io, format_args!("timeout: security not finished {} ms after the link came up (state {:?}, we asked for authentication: {}, last security event {} ms ago); awaited {}", SECURITY_TIMEOUT, l.sec, u8::from(l.we_asked_auth), now.saturating_sub(l.last_sec_event), awaited));
                self.status(io, format_args!("Pairing timed out"));
                self.disconnect(io, REASON_USER_ENDED);
            }
        }
        for i in 0..CHANNELS {
            let c = self.chans[i];
            if c.st == ChSt::Free || c.st == ChSt::Open || now < c.deadline {
                continue;
            }
            self.err(
                io,
                format_args!(
                    "timeout: channel {} local={:#06x} in state {:?}; awaited {}",
                    l2cap::psm_name(c.psm),
                    c.local,
                    c.st,
                    ChanWait(c)
                ),
            );
            match c.st {
                ChSt::Connecting | ChSt::Pending => {
                    self.chans[i] = FREE_CHAN;
                    self.on_channel_failed(now, io, c.psm, c.ours);
                }
                ChSt::Config => self.close_channel(now, io, i),
                _ => {
                    self.chans[i] = FREE_CHAN;
                    self.on_channel_gone(now, io, c.psm, c.ours);
                }
            }
        }
        let Some(l) = self.link else { return };
        if l.info_req.is_some_and(|(_, t)| now >= t) {
            log!(io, L2cap, "no answer to the information request (fine)");
            if let Some(m) = self.link.as_mut() {
                m.info_req = None;
            }
        }
        if matches!(l.sdp, SdpState::Opening | SdpState::Waiting { .. }) && now >= l.sdp_deadline {
            match l.sdp {
                SdpState::Waiting { tid } => self.err(io, format_args!("timeout: SDP not finished after {} ms; awaited the answer to SDP request tid={} (query {} of 2, {} pieces so far); going on without it", SDP_TIMEOUT, tid, l.sdp_query + 1, l.sdp_pieces)),
                _ => self.err(io, format_args!("timeout: SDP not finished after {} ms; awaited the SDP channel to open; going on without it", SDP_TIMEOUT)),
            }
            self.sdp_finish(now, io);
        }
        let Some(l) = self.link else { return };
        if l.open_ctrl_at.is_some_and(|t| now >= t) {
            if let Some(m) = self.link.as_mut() {
                m.open_ctrl_at = None;
            }
            if self.any_chan(l2cap::PSM_HID_CONTROL).is_none() {
                log!(
                    io,
                    State,
                    "the gamepad did not open HID control itself; opening it ourselves"
                );
                self.status(io, format_args!("Opening HID channels"));
                self.open_channel(now, io, l2cap::PSM_HID_CONTROL);
            }
        }
        if l.open_intr_at.is_some_and(|t| now >= t) {
            if let Some(m) = self.link.as_mut() {
                m.open_intr_at = None;
            }
            if self.any_chan(l2cap::PSM_HID_INTERRUPT).is_none()
                && self.hid_chan(l2cap::PSM_HID_CONTROL).is_some()
            {
                log!(
                    io,
                    State,
                    "the gamepad did not open HID interrupt itself; opening it ourselves"
                );
                self.open_channel(now, io, l2cap::PSM_HID_INTERRUPT);
            }
        }
        if l.hid_deadline.is_some_and(|t| now >= t) && !l.hid_up {
            if let Some(m) = self.link.as_mut() {
                m.hid_deadline = None;
            }
            self.err(io, format_args!("timeout: HID channels not both open after {} ms (control: {}, interrupt: {}); awaited both open", SDP_TIMEOUT + HID_OPEN_TIMEOUT, self.chan_state(l2cap::PSM_HID_CONTROL), self.chan_state(l2cap::PSM_HID_INTERRUPT)));
            self.status(io, format_args!("HID channels did not open"));
            self.disconnect(io, REASON_USER_ENDED);
        }
        if now >= l.report_log_at {
            if l.reports_second > 0 && l.reports > REPORTS_LOGGED {
                log!(
                    io,
                    Hid,
                    "reports total={} last_second={}",
                    l.reports,
                    l.reports_second
                );
            }
            if let Some(m) = self.link.as_mut() {
                m.reports_second = 0;
                m.report_log_at = now + 1_000;
            }
        }
        if l.disconnect_at.is_some_and(|t| now >= t) {
            if let Some(m) = self.link.as_mut() {
                m.disconnect_at = None;
            }
            log!(
                io,
                State,
                "closing the link now (planned: a HID channel stayed closed, or the probe is done)"
            );
            self.disconnect(io, REASON_USER_ENDED);
        }
        self.report_timers(now, io);
    }

    /// The "still waiting" lines and the PAD-SUMMARY "flowing" line.
    fn report_timers(&mut self, now: u64, io: &mut impl Io) {
        let Some(l) = self.link else { return };
        if l.purpose != Purpose::Gamepad {
            return;
        }
        if let Some(t) = l.first_report_at
            && !l.flowing_logged
            && now >= t + FLOWING_AFTER
        {
            if let Some(m) = self.link.as_mut() {
                m.flowing_logged = true;
            }
            self.summary(now, &l, None, io);
        }
        if let Some(t) = l.hid_up_at
            && l.reports == 0
            && !l.no_report_logged
            && now >= t + NO_REPORT_WAIT
        {
            if let Some(m) = self.link.as_mut() {
                m.no_report_logged = true;
            }
            log!(
                io,
                State,
                "waiting: the HID channels have been open {} ms and no input report came; awaited a report on the interrupt channel (in Switch mode the pad may send only when a button changes: press one)",
                now - t
            );
        }
        if l.reports > 0 && !l.stopped_logged && now >= l.last_report_at + REPORTS_STOPPED {
            if let Some(m) = self.link.as_mut() {
                m.stopped_logged = true;
            }
            log!(
                io,
                State,
                "no input report for {} ms (last was n={}); some pads send only when something changes",
                now - l.last_report_at,
                l.reports
            );
        }
    }

    /// One line about a gamepad connection: when reports flow (`end` is
    /// None) and when the link ends (`end` is the reason). No address.
    fn summary(&self, now: u64, l: &Link, end: Option<u8>, io: &mut impl Io) {
        let reached = if l.reports > 0 {
            "reports"
        } else if l.hid_up {
            "hid-open"
        } else if l.sdp == SdpState::Done {
            "sdp-done"
        } else if l.sec == Sec::Encrypted {
            "encrypted"
        } else if l.auth_ok {
            "paired"
        } else {
            "linked"
        };
        let pairing = if l.new_key {
            "new"
        } else if l.key_offered && l.auth_ok {
            "stored-key"
        } else if l.key_offered {
            "stored-key-not-accepted"
        } else {
            "none"
        };
        // Reports a second, times ten, over the time since the first one.
        let rate = l.first_report_at.and_then(|t| {
            let span = now.saturating_sub(t);
            (span >= 1_000).then(|| (u64::from(l.reports.saturating_sub(1)) * 10_000 / span) as u32)
        });
        log!(
            io,
            Summary,
            "{} name=\"{}\" class={:#08x} vendor={} product={} version={} vendor_source={} descriptor_bytes={} pairing={} link={} reached={} reports={} rate={}/s ids={} outputs={} set_reports={} handshakes={} acl_sent={} acl_completed={} found_to_link_ms={} link_to_hid_ms={} hid_to_first_report_ms={} link_ms={}{}",
            if end.is_some() { "end" } else { "flowing" },
            Printable(l.name.as_str().as_bytes()),
            l.cod,
            Opt16(l.pnp.vendor_id),
            Opt16(l.pnp.product_id),
            Opt16(l.pnp.version),
            Opt16(l.pnp.vendor_id_source),
            l.desc_bytes,
            pairing,
            if l.incoming { "it-called" } else { "we-called" },
            reached,
            l.reports,
            Tenths(rate),
            Ids(&l.ids[..l.ids_n]),
            l.outputs,
            l.set_reports,
            l.handshakes,
            l.acl_queued,
            l.acl_done,
            Ms(l.found_at.map(|f| l.up_at.saturating_sub(f))),
            Ms(l.hid_up_at.map(|h| h.saturating_sub(l.up_at))),
            Ms(l.hid_up_at
                .zip(l.first_report_at)
                .map(|(h, f)| f.saturating_sub(h))),
            now.saturating_sub(l.up_at),
            EndReason(end)
        );
    }

    /// A HID channel's state, for the log.
    fn chan_state(&self, psm: u16) -> &'static str {
        if self.hid_chan(psm).is_some() {
            return "open";
        }
        match self.any_chan(psm).map(|i| self.chans[i].st) {
            None => "none",
            Some(ChSt::Connecting) => "connecting",
            Some(ChSt::Pending) => "pending",
            Some(ChSt::Config) => "configuring",
            Some(ChSt::Closing) => "closing",
            Some(_) => "?",
        }
    }

    // -------------------------------------------------------------- data

    fn on_acl(&mut self, now: u64, p: &[u8], io: &mut impl Io) {
        self.stats.acl_in += 1;
        let Some((h, data)) = acl::parse(p) else {
            self.err(
                io,
                format_args!("data packet shorter than its header, len={}", p.len()),
            );
            return;
        };
        if self.link.is_none_or(|l| l.handle != h.handle)
            && let Some(st) = self.stale.as_mut()
            && st.handle == h.handle
        {
            // The old link we dropped: its last packets, quietly.
            st.acl += 1;
            return;
        }
        let Some(link) = self.link else {
            self.err(
                io,
                format_args!("data for handle {:#05x} with no link", h.handle),
            );
            return;
        };
        if h.handle != link.handle {
            self.err(
                io,
                format_args!(
                    "data for handle {:#05x}, ours is {:#05x}",
                    h.handle, link.handle
                ),
            );
            return;
        }
        let mut frame = [0_u8; FRAME_IN];
        let n = match self.reasm.push(h.pb, data) {
            Ok(Some(f)) => {
                frame[..f.len()].copy_from_slice(f);
                f.len()
            }
            Ok(None) => return,
            Err(e) => {
                self.err(io, format_args!("reassembly: {:?}", e));
                return;
            }
        };
        let frame = &frame[..n];
        let len = usize::from(u16_at(frame, 0).unwrap_or(0));
        let cid = u16_at(frame, 2).unwrap_or(0);
        let payload = &frame[4..4 + len];
        match cid {
            l2cap::CID_SIGNALLING => {
                for c in l2cap::commands(payload) {
                    self.on_signal(now, c, io);
                }
            }
            l2cap::CID_CONNECTIONLESS => log!(
                io,
                L2cap,
                "connectionless data len={} (ignored)",
                payload.len()
            ),
            _ => {
                let Some(i) = self
                    .chans
                    .iter()
                    .position(|c| c.st != ChSt::Free && c.local == cid)
                else {
                    self.err(
                        io,
                        format_args!("data on unknown channel {:#06x} len={}", cid, payload.len()),
                    );
                    return;
                };
                let c = self.chans[i];
                if c.st != ChSt::Open {
                    log!(
                        io,
                        L2cap,
                        "data on channel {:#06x} before it is open (state {:?}) len={}",
                        cid,
                        c.st,
                        payload.len()
                    );
                }
                match c.psm {
                    l2cap::PSM_SDP if c.ours => self.on_sdp_answer(now, payload, io),
                    l2cap::PSM_SDP => self.on_sdp_request(c.remote, payload, io),
                    l2cap::PSM_HID_INTERRUPT => self.on_hid_interrupt(now, payload, io),
                    l2cap::PSM_HID_CONTROL => self.on_hid_control(now, payload, io),
                    _ => log!(
                        io,
                        L2cap,
                        "data on psm {:#06x} len={}",
                        c.psm,
                        payload.len()
                    ),
                }
            }
        }
    }

    // ------------------------------------------------------------ L2CAP

    fn alloc_cid(&mut self) -> u16 {
        loop {
            let cid = self.next_cid;
            self.next_cid = if self.next_cid == 0xFFFF {
                l2cap::CID_DYNAMIC_FIRST
            } else {
                self.next_cid + 1
            };
            if !self
                .chans
                .iter()
                .any(|c| c.st != ChSt::Free && c.local == cid)
            {
                return cid;
            }
        }
    }

    fn free_slot(&self) -> Option<usize> {
        self.chans.iter().position(|c| c.st == ChSt::Free)
    }

    /// Any channel for this PSM, in any state but free.
    fn any_chan(&self, psm: u16) -> Option<usize> {
        self.chans
            .iter()
            .position(|c| c.st != ChSt::Free && c.psm == psm)
    }

    /// The open channel to use for this HID PSM: the one the gamepad
    /// opened, if there are two.
    fn hid_chan(&self, psm: u16) -> Option<usize> {
        let open = |ours: bool| {
            self.chans
                .iter()
                .position(|c| c.st == ChSt::Open && c.psm == psm && c.ours == ours)
        };
        open(false).or_else(|| open(true))
    }

    fn open_channel(&mut self, now: u64, io: &mut impl Io, psm: u16) -> Option<usize> {
        let Some(i) = self.free_slot() else {
            self.err(
                io,
                format_args!("no free channel slot for {}", l2cap::psm_name(psm)),
            );
            return None;
        };
        let local = self.alloc_cid();
        let id = self.new_id();
        self.chans[i] = Chan {
            st: ChSt::Connecting,
            psm,
            local,
            ours: true,
            req_id: id,
            deadline: now + SIGNAL_TIMEOUT,
            ..FREE_CHAN
        };
        log!(
            io,
            L2cap,
            "opening {} psm={:#06x} local={:#06x}",
            l2cap::psm_name(psm),
            psm,
            local
        );
        let mut d = [0_u8; 4];
        d[..2].copy_from_slice(&psm.to_le_bytes());
        d[2..].copy_from_slice(&local.to_le_bytes());
        self.send_signal(io, code::CONNECTION_REQUEST, id, &d);
        Some(i)
    }

    fn close_channel(&mut self, now: u64, io: &mut impl Io, i: usize) {
        let c = self.chans[i];
        if c.st == ChSt::Free || c.st == ChSt::Closing {
            return;
        }
        if c.remote == 0 {
            // Nothing to tell the other side yet.
            self.chans[i] = FREE_CHAN;
            self.on_channel_gone(now, io, c.psm, c.ours);
            return;
        }
        let id = self.new_id();
        self.chans[i].st = ChSt::Closing;
        self.chans[i].req_id = id;
        self.chans[i].deadline = now + SIGNAL_TIMEOUT;
        let mut d = [0_u8; 4];
        d[..2].copy_from_slice(&c.remote.to_le_bytes());
        d[2..].copy_from_slice(&c.local.to_le_bytes());
        self.send_signal(io, code::DISCONNECTION_REQUEST, id, &d);
    }

    fn conn_response(
        &mut self,
        io: &mut impl Io,
        id: u8,
        dcid: u16,
        scid: u16,
        result: u16,
        status: u16,
    ) {
        let mut d = [0_u8; 8];
        d[0..2].copy_from_slice(&dcid.to_le_bytes());
        d[2..4].copy_from_slice(&scid.to_le_bytes());
        d[4..6].copy_from_slice(&result.to_le_bytes());
        d[6..8].copy_from_slice(&status.to_le_bytes());
        self.send_signal(io, code::CONNECTION_RESPONSE, id, &d);
    }

    fn send_config_request(&mut self, now: u64, io: &mut impl Io, i: usize) {
        let id = self.new_id();
        let c = &mut self.chans[i];
        c.req_id = id;
        c.deadline = now + SIGNAL_TIMEOUT;
        c.our_conf_ok = false;
        let mut d = [0_u8; 8];
        d[0..2].copy_from_slice(&c.remote.to_le_bytes());
        // d[2..4]: flags 0, no continuation.
        let n = match c.ask_mtu {
            Some(m) => {
                d[4] = l2cap::opt::MTU;
                d[5] = 2;
                d[6..8].copy_from_slice(&m.to_le_bytes());
                8
            }
            None => 4,
        };
        self.send_signal(io, code::CONFIGURE_REQUEST, id, &d[..n]);
    }

    fn on_signal(&mut self, now: u64, c: Command<'_>, io: &mut impl Io) {
        // Their bytes are shown with the pad's address hidden.
        let addr = self.link.map(|l| l.addr);
        match c.signal {
            Signal::ConfigureRequest {
                dcid,
                flags,
                options,
            } => {
                log!(
                    io,
                    L2cap,
                    "rx configure-request id={} dcid={:#06x} flags={:#06x} options={}",
                    c.id,
                    dcid,
                    flags,
                    HexHide(options, addr)
                );
            }
            Signal::ConfigureResponse {
                scid,
                flags,
                result,
                options,
            } => {
                log!(
                    io,
                    L2cap,
                    "rx configure-response id={} scid={:#06x} flags={:#06x} result={}({}) options={}",
                    c.id,
                    scid,
                    flags,
                    result,
                    conf_result_text(result),
                    HexHide(options, addr)
                );
            }
            Signal::InformationResponse {
                info_type,
                result,
                data,
            } => {
                log!(
                    io,
                    L2cap,
                    "rx information-response id={} type={} result={} data={}{}",
                    c.id,
                    info_type,
                    result,
                    HexHide(data, addr),
                    info_text(info_type, result, data)
                );
            }
            Signal::ConnectionResponse {
                dcid,
                scid,
                result,
                status,
            } => {
                log!(
                    io,
                    L2cap,
                    "rx connection-response id={} dcid={:#06x} scid={:#06x} result={}({}) status={}",
                    c.id,
                    dcid,
                    scid,
                    result,
                    conn_result_text(result),
                    status
                );
            }
            other => log!(io, L2cap, "rx id={} {:?}", c.id, other),
        }
        match c.signal {
            Signal::ConnectionRequest { psm, scid } => {
                self.on_conn_request(now, io, c.id, psm, scid)
            }
            Signal::ConnectionResponse {
                dcid,
                scid,
                result,
                status: _,
            } => self.on_conn_response(now, io, c.id, dcid, scid, result),
            Signal::ConfigureRequest {
                dcid,
                flags,
                options,
            } => self.on_conf_request(now, io, c.id, dcid, flags, options),
            Signal::ConfigureResponse {
                scid,
                flags,
                result,
                options,
            } => self.on_conf_response(now, io, c.id, scid, flags, result, options),
            Signal::DisconnectionRequest { dcid, scid } => {
                let mut d = [0_u8; 4];
                d[..2].copy_from_slice(&dcid.to_le_bytes());
                d[2..].copy_from_slice(&scid.to_le_bytes());
                match self
                    .chans
                    .iter()
                    .position(|ch| ch.st != ChSt::Free && ch.local == dcid && ch.remote == scid)
                {
                    Some(i) => {
                        let ch = self.chans[i];
                        self.send_signal(io, code::DISCONNECTION_RESPONSE, c.id, &d);
                        self.chans[i] = FREE_CHAN;
                        log!(
                            io,
                            L2cap,
                            "closed by the other side: {} local={:#06x}",
                            l2cap::psm_name(ch.psm),
                            ch.local
                        );
                        self.on_channel_gone(now, io, ch.psm, ch.ours);
                    }
                    None => {
                        // Reason 0x0002: invalid channel id.
                        let mut r = [0_u8; 6];
                        r[0..2].copy_from_slice(&0x0002_u16.to_le_bytes());
                        r[2..4].copy_from_slice(&dcid.to_le_bytes());
                        r[4..6].copy_from_slice(&scid.to_le_bytes());
                        self.send_signal(io, code::COMMAND_REJECT, c.id, &r);
                    }
                }
            }
            Signal::DisconnectionResponse { dcid: _, scid } => {
                if let Some(i) = self
                    .chans
                    .iter()
                    .position(|ch| ch.st == ChSt::Closing && ch.local == scid)
                {
                    let ch = self.chans[i];
                    self.chans[i] = FREE_CHAN;
                    self.on_channel_gone(now, io, ch.psm, ch.ours);
                }
            }
            Signal::EchoRequest { data } => self.send_signal(io, code::ECHO_RESPONSE, c.id, data),
            Signal::InformationRequest { info_type } => {
                let mut d = [0_u8; 12];
                d[0..2].copy_from_slice(&info_type.to_le_bytes());
                let n = match info_type {
                    // Connectionless MTU.
                    0x0001 => {
                        d[4..6].copy_from_slice(&OUR_MTU.to_le_bytes());
                        6
                    }
                    // Extended features: none (basic mode only).
                    0x0002 => 8,
                    // Anything else: "not supported".
                    _ => {
                        d[2] = 0x01;
                        4
                    }
                };
                self.send_signal(io, code::INFORMATION_RESPONSE, c.id, &d[..n]);
            }
            Signal::InformationResponse { .. } => {
                if let Some(l) = self.link.as_mut()
                    && l.info_req.is_some_and(|(id, _)| id == c.id)
                {
                    l.info_req = None;
                }
            }
            Signal::CommandReject { reason, .. } => {
                if let Some(l) = self.link.as_mut()
                    && l.info_req.is_some_and(|(id, _)| id == c.id)
                {
                    l.info_req = None;
                    return;
                }
                if let Some(i) = self
                    .chans
                    .iter()
                    .position(|ch| ch.st != ChSt::Free && ch.st != ChSt::Open && ch.req_id == c.id)
                {
                    let ch = self.chans[i];
                    self.err(
                        io,
                        format_args!(
                            "our {} request was refused (reason {})",
                            l2cap::psm_name(ch.psm),
                            reason
                        ),
                    );
                    self.chans[i] = FREE_CHAN;
                    if ch.st == ChSt::Connecting {
                        self.on_channel_failed(now, io, ch.psm, ch.ours);
                    } else {
                        self.on_channel_gone(now, io, ch.psm, ch.ours);
                    }
                }
            }
            Signal::EchoResponse { .. } => {}
            Signal::Unknown { .. } | Signal::Malformed { .. } => {
                // Reason 0x0000: command not understood.
                self.send_signal(io, code::COMMAND_REJECT, c.id, &[0, 0]);
            }
        }
    }

    fn on_conn_request(&mut self, now: u64, io: &mut impl Io, id: u8, psm: u16, scid: u16) {
        let Some(link) = self.link else { return };
        let hid = psm == l2cap::PSM_HID_CONTROL || psm == l2cap::PSM_HID_INTERRUPT;
        let refuse = if psm != l2cap::PSM_SDP && !hid {
            Some(conn_result::PSM_NOT_SUPPORTED)
        } else if hid && link.purpose != Purpose::Gamepad {
            Some(conn_result::SECURITY_BLOCK)
        } else if self.free_slot().is_none() {
            Some(conn_result::NO_RESOURCES)
        } else {
            None
        };
        if let Some(result) = refuse {
            self.conn_response(io, id, 0, scid, result, conn_status::NO_INFO);
            return;
        }
        let Some(i) = self.free_slot() else { return };
        let local = self.alloc_cid();
        self.chans[i] = Chan {
            st: ChSt::Config,
            psm,
            local,
            remote: scid,
            ours: false,
            their_req_id: id,
            deadline: now + SIGNAL_TIMEOUT,
            ..FREE_CHAN
        };
        log!(
            io,
            L2cap,
            "the other side opens {} psm={:#06x} their_cid={:#06x} local={:#06x}",
            l2cap::psm_name(psm),
            psm,
            scid,
            local
        );
        if hid {
            // The gamepad opens HID itself: do not open another one.
            if let Some(l) = self.link.as_mut() {
                if psm == l2cap::PSM_HID_CONTROL {
                    l.open_ctrl_at = None;
                    l.open_intr_at = Some(now + PAD_OPENS_GRACE + SIGNAL_TIMEOUT);
                } else {
                    l.open_intr_at = None;
                }
            }
            if link.sec != Sec::Encrypted {
                // HID needs an encrypted link: answer "pending" and get
                // security going if nobody has started it yet.
                self.chans[i].st = ChSt::Pending;
                self.chans[i].deadline = now + PENDING_TIMEOUT;
                self.conn_response(
                    io,
                    id,
                    local,
                    scid,
                    conn_result::PENDING,
                    conn_status::AUTHENTICATION_PENDING,
                );
                if link.sec == Sec::None {
                    self.request_auth(io);
                }
                return;
            }
        }
        self.conn_response(
            io,
            id,
            local,
            scid,
            conn_result::SUCCESS,
            conn_status::NO_INFO,
        );
        self.send_config_request(now, io, i);
    }

    fn on_conn_response(
        &mut self,
        now: u64,
        io: &mut impl Io,
        id: u8,
        dcid: u16,
        scid: u16,
        result: u16,
    ) {
        let Some(i) = self
            .chans
            .iter()
            .position(|c| c.st == ChSt::Connecting && c.local == scid)
        else {
            if result == conn_result::SUCCESS && dcid != 0 {
                // An answer to a request we gave up on: close that channel.
                log!(
                    io,
                    L2cap,
                    "late connection response for local={:#06x}; closing it",
                    scid
                );
                let id2 = self.new_id();
                let mut d = [0_u8; 4];
                d[..2].copy_from_slice(&dcid.to_le_bytes());
                d[2..].copy_from_slice(&scid.to_le_bytes());
                self.send_signal(io, code::DISCONNECTION_REQUEST, id2, &d);
            }
            return;
        };
        if self.chans[i].req_id != id {
            log!(
                io,
                L2cap,
                "connection response id={} does not match our request id={} (used anyway)",
                id,
                self.chans[i].req_id
            );
        }
        match result {
            conn_result::SUCCESS => {
                self.chans[i].remote = dcid;
                self.chans[i].st = ChSt::Config;
                self.send_config_request(now, io, i);
            }
            conn_result::PENDING => {
                self.chans[i].remote = dcid;
                self.chans[i].deadline = now + PENDING_TIMEOUT;
            }
            _ => {
                let c = self.chans[i];
                self.chans[i] = FREE_CHAN;
                self.err(
                    io,
                    format_args!(
                        "{} channel refused: {}",
                        l2cap::psm_name(c.psm),
                        conn_result_text(result)
                    ),
                );
                self.on_channel_failed(now, io, c.psm, true);
            }
        }
    }

    fn on_conf_request(
        &mut self,
        now: u64,
        io: &mut impl Io,
        id: u8,
        dcid: u16,
        flags: u16,
        options: &[u8],
    ) {
        let Some(i) = self
            .chans
            .iter()
            .position(|c| matches!(c.st, ChSt::Config | ChSt::Open) && c.local == dcid)
        else {
            let mut r = [0_u8; 6];
            r[0..2].copy_from_slice(&0x0002_u16.to_le_bytes());
            r[2..4].copy_from_slice(&dcid.to_le_bytes());
            self.send_signal(io, code::COMMAND_REJECT, id, &r);
            return;
        };
        let a = l2cap::answer_config(options);
        if let Some(mode) = a.refused_mode {
            log!(
                io,
                L2cap,
                "the other side asked for retransmission mode {} on {}; we only do basic mode (0)",
                mode,
                l2cap::psm_name(self.chans[i].psm)
            );
        }
        let mut d = [0_u8; 6 + 24];
        d[0..2].copy_from_slice(&self.chans[i].remote.to_le_bytes());
        d[2..4].copy_from_slice(&(flags & 1).to_le_bytes());
        d[4..6].copy_from_slice(&a.result.to_le_bytes());
        d[6..6 + a.options_len].copy_from_slice(a.options());
        let c = &mut self.chans[i];
        if a.result != conf_result::SUCCESS {
            // They will send the whole request again.
            c.pending_mtu = None;
        } else if flags & 1 != 0 {
            // More parts follow (continuation flag): keep what this part
            // says until the last part comes (Core spec, Vol 3, Part A,
            // 4.4: the request without the flag completes the request).
            if a.mtu.is_some() {
                c.pending_mtu = a.mtu;
            }
            log!(
                io,
                L2cap,
                "configure request continues in another part (mtu so far {:?})",
                c.pending_mtu
            );
        } else {
            c.their_conf_ok = true;
            c.remote_mtu = a.mtu.or(c.pending_mtu).unwrap_or(l2cap::MTU_DEFAULT);
            c.pending_mtu = None;
        }
        self.send_signal(io, code::CONFIGURE_RESPONSE, id, &d[..6 + a.options_len]);
        self.check_open(now, io, i);
    }

    #[allow(clippy::too_many_arguments)]
    fn on_conf_response(
        &mut self,
        now: u64,
        io: &mut impl Io,
        id: u8,
        scid: u16,
        flags: u16,
        result: u16,
        options: &[u8],
    ) {
        let Some(i) = self
            .chans
            .iter()
            .position(|c| matches!(c.st, ChSt::Config | ChSt::Open) && c.local == scid)
        else {
            log!(
                io,
                L2cap,
                "configure response for unknown channel {:#06x}",
                scid
            );
            return;
        };
        if self.chans[i].req_id != id {
            log!(
                io,
                L2cap,
                "configure response id={} does not match our request id={}",
                id,
                self.chans[i].req_id
            );
        }
        match result {
            conf_result::SUCCESS if flags & 1 == 0 => {
                self.chans[i].our_conf_ok = true;
                self.check_open(now, io, i);
            }
            conf_result::SUCCESS | conf_result::PENDING => {
                self.chans[i].deadline = now + PENDING_TIMEOUT
            }
            conf_result::UNACCEPTABLE | conf_result::UNKNOWN_OPTIONS => {
                let c = &mut self.chans[i];
                c.conf_tries += 1;
                if result == conf_result::UNACCEPTABLE {
                    for o in l2cap::options(options).flatten() {
                        if o.kind == l2cap::opt::MTU && o.data.len() >= 2 {
                            let m = u16::from_le_bytes([o.data[0], o.data[1]]);
                            c.ask_mtu = Some(m.clamp(l2cap::MTU_MIN, OUR_MTU));
                        }
                    }
                } else if options.contains(&l2cap::opt::MTU) {
                    c.ask_mtu = None;
                }
                let c = *c;
                if c.conf_tries > 3 {
                    self.err(
                        io,
                        format_args!(
                            "configuration of {} failed after 3 tries",
                            l2cap::psm_name(c.psm)
                        ),
                    );
                    self.close_channel(now, io, i);
                } else {
                    log!(
                        io,
                        L2cap,
                        "configuring again with mtu={:?} (try {})",
                        c.ask_mtu,
                        c.conf_tries + 1
                    );
                    self.send_config_request(now, io, i);
                }
            }
            _ => {
                self.err(
                    io,
                    format_args!(
                        "configuration of {} rejected",
                        l2cap::psm_name(self.chans[i].psm)
                    ),
                );
                self.close_channel(now, io, i);
            }
        }
    }

    fn check_open(&mut self, now: u64, io: &mut impl Io, i: usize) {
        let c = self.chans[i];
        if c.st != ChSt::Config || !c.our_conf_ok || !c.their_conf_ok {
            return;
        }
        self.chans[i].st = ChSt::Open;
        log!(
            io,
            L2cap,
            "open {} local={:#06x} remote={:#06x} remote_mtu={} opened_by={}",
            l2cap::psm_name(c.psm),
            c.local,
            c.remote,
            c.remote_mtu,
            if c.ours { "us" } else { "them" }
        );
        let hid = c.psm == l2cap::PSM_HID_CONTROL || c.psm == l2cap::PSM_HID_INTERRUPT;
        if hid
            && let Some(j) = (0..CHANNELS)
                .find(|&j| j != i && self.chans[j].st == ChSt::Open && self.chans[j].psm == c.psm)
        {
            // Both sides opened the same HID channel: keep the gamepad's.
            let extra = if c.ours { i } else { j };
            log!(
                io,
                L2cap,
                "two {} channels: closing the one we opened",
                l2cap::psm_name(c.psm)
            );
            self.close_channel(now, io, extra);
            if extra == i {
                return;
            }
        }
        match c.psm {
            l2cap::PSM_SDP if c.ours => self.sdp_send_query(now, io),
            l2cap::PSM_HID_CONTROL => {
                // Interrupt follows on the next tick (20 ms) when we opened
                // control: if the gamepad's own control channel crosses
                // ours meanwhile, it opens interrupt itself.
                if let Some(l) = self.link.as_mut()
                    && l.open_intr_at.is_none()
                {
                    l.open_intr_at = Some(if c.ours { now } else { now + INTERRUPT_GRACE });
                }
                self.check_hid_up(now, io);
            }
            l2cap::PSM_HID_INTERRUPT => self.check_hid_up(now, io),
            _ => {}
        }
    }

    fn check_hid_up(&mut self, now: u64, io: &mut impl Io) {
        if self.hid_chan(l2cap::PSM_HID_CONTROL).is_none()
            || self.hid_chan(l2cap::PSM_HID_INTERRUPT).is_none()
        {
            return;
        }
        let Some(l) = self.link.as_mut() else { return };
        if l.hid_up {
            return;
        }
        l.hid_up = true;
        l.hid_up_at = Some(now);
        l.hid_deadline = None;
        l.open_intr_at = None;
        l.open_ctrl_at = None;
        self.status(io, format_args!("Connected"));
    }

    /// Our request to open a channel failed.
    fn on_channel_failed(&mut self, now: u64, io: &mut impl Io, psm: u16, ours: bool) {
        match psm {
            l2cap::PSM_SDP if ours => {
                if self
                    .link
                    .is_some_and(|l| matches!(l.sdp, SdpState::Opening | SdpState::Waiting { .. }))
                {
                    self.sdp_finish(now, io);
                }
            }
            l2cap::PSM_HID_CONTROL | l2cap::PSM_HID_INTERRUPT
                if self.any_chan(psm).is_none()
                    && self
                        .link
                        .is_some_and(|l| l.purpose == Purpose::Gamepad && !l.hid_up) =>
            {
                self.status(io, format_args!("HID channel refused"));
                self.disconnect(io, REASON_USER_ENDED);
            }
            _ => {}
        }
    }

    /// A channel closed (by either side).
    fn on_channel_gone(&mut self, now: u64, io: &mut impl Io, psm: u16, ours: bool) {
        match psm {
            l2cap::PSM_SDP if ours => {
                if self
                    .link
                    .is_some_and(|l| matches!(l.sdp, SdpState::Opening | SdpState::Waiting { .. }))
                {
                    self.sdp_finish(now, io);
                }
            }
            l2cap::PSM_HID_CONTROL | l2cap::PSM_HID_INTERRUPT => {
                let up = self.link.is_some_and(|l| l.hid_up);
                if up && self.hid_chan(psm).is_none() {
                    log!(
                        io,
                        State,
                        "a HID channel closed; closing the link in {} ms unless it does",
                        HID_CLOSED_GRACE
                    );
                    if let Some(l) = self.link.as_mut() {
                        l.disconnect_at = Some(now + HID_CLOSED_GRACE);
                    }
                }
            }
            _ => {}
        }
    }

    // -------------------------------------------------------------- SDP

    fn start_sdp(&mut self, now: u64, io: &mut impl Io) {
        let Some(l) = self.link else { return };
        if l.sdp != SdpState::Idle {
            return;
        }
        if let Some(m) = self.link.as_mut() {
            m.sdp = SdpState::Opening;
            m.sdp_deadline = now + SDP_TIMEOUT;
            m.sdp_query = 0;
        }
        self.sdp_len = 0;
        self.sdp_truncated = false;
        if self.open_channel(now, io, l2cap::PSM_SDP).is_none() {
            self.sdp_finish(now, io);
        }
    }

    /// The service classes asked for, in turn.
    fn sdp_uuid(purpose: Purpose, query: u8) -> Option<u16> {
        match (purpose, query) {
            (Purpose::Gamepad, 0) => Some(sdp::UUID_HID),
            (Purpose::Gamepad, 1) => Some(sdp::UUID_PNP),
            (Purpose::SdpProbe { uuid, .. }, 0) => Some(uuid),
            _ => None,
        }
    }

    fn sdp_send_query(&mut self, now: u64, io: &mut impl Io) {
        let Some(l) = self.link else { return };
        let Some(uuid) = Self::sdp_uuid(l.purpose, l.sdp_query) else {
            self.sdp_finish(now, io);
            return;
        };
        let Some(i) = self
            .chans
            .iter()
            .position(|c| c.st == ChSt::Open && c.psm == l2cap::PSM_SDP && c.ours)
        else {
            self.sdp_finish(now, io);
            return;
        };
        let (max, ranges): (u16, &[(u16, u16)]) = match l.purpose {
            Purpose::Gamepad => (SDP_MAX_PER_ANSWER, &[(0x0000, 0xFFFF)]),
            // The probe asks for a few attributes in small pieces, so the
            // answer needs several rounds of "continuation".
            Purpose::SdpProbe { max_bytes, .. } => (
                max_bytes,
                &[(0x0000, 0x0001), (0x0004, 0x0004), (0x0100, 0x0100)],
            ),
        };
        let tid = l.sdp_tid.wrapping_add(1);
        let mut pdu = [0_u8; 96];
        let n = sdp::search_attr_request(
            &mut pdu,
            tid,
            uuid,
            max,
            ranges,
            &l.sdp_cont[..l.sdp_cont_len],
        );
        if let Some(m) = self.link.as_mut() {
            m.sdp_tid = tid;
            m.sdp = SdpState::Waiting { tid };
        }
        log!(
            io,
            Sdp,
            "request tid={} uuid={:#06x} max_bytes={} continuation={} pdu={}",
            tid,
            uuid,
            max,
            Hex(&l.sdp_cont[..l.sdp_cont_len]),
            Hex(&pdu[..n])
        );
        let remote = self.chans[i].remote;
        self.send_frame(io, remote, &pdu[..n]);
    }

    fn on_sdp_answer(&mut self, now: u64, payload: &[u8], io: &mut impl Io) {
        for (k, piece) in payload.chunks(64).enumerate() {
            log!(
                io,
                Sdp,
                "answer bytes={} part={} hex={}",
                payload.len(),
                k,
                Hex(piece)
            );
        }
        let Some(l) = self.link else { return };
        let SdpState::Waiting { tid: want } = l.sdp else {
            log!(io, Sdp, "answer while not waiting for one (ignored)");
            return;
        };
        match sdp::parse_pdu(payload) {
            Some(sdp::Pdu::SearchAttrResponse { tid, lists, cont }) => {
                if tid != want {
                    log!(
                        io,
                        Sdp,
                        "answer tid={} but we wait for tid={} (used anyway)",
                        tid,
                        want
                    );
                }
                let room = SDP_MAX - self.sdp_len;
                let take = lists.len().min(room);
                self.sdp_buf[self.sdp_len..self.sdp_len + take].copy_from_slice(&lists[..take]);
                self.sdp_len += take;
                if take < lists.len() {
                    self.sdp_truncated = true;
                }
                if let Some(m) = self.link.as_mut() {
                    m.sdp_pieces += 1;
                }
                log!(
                    io,
                    Sdp,
                    "piece tid={} list_bytes={} joined_so_far={} continuation_len={}",
                    tid,
                    lists.len(),
                    self.sdp_len,
                    cont.len()
                );
                if !cont.is_empty() && cont.len() <= 16 {
                    if let Some(m) = self.link.as_mut() {
                        m.sdp_cont[..cont.len()].copy_from_slice(cont);
                        m.sdp_cont_len = cont.len();
                    }
                    self.sdp_send_query(now, io);
                } else {
                    if cont.len() > 16 {
                        self.err(
                            io,
                            format_args!(
                                "SDP continuation state of {} bytes is too long",
                                cont.len()
                            ),
                        );
                    }
                    self.sdp_query_done(now, io);
                }
            }
            Some(sdp::Pdu::Error { tid, code }) => {
                self.err(
                    io,
                    format_args!(
                        "SDP error tid={} code={:#06x}({})",
                        tid,
                        code,
                        sdp::error_text(code)
                    ),
                );
                self.sdp_len = 0;
                self.sdp_query_done(now, io);
            }
            other => {
                self.err(
                    io,
                    format_args!("SDP answer not understood: {:?}", other.map(|_| ())),
                );
                self.sdp_len = 0;
                self.sdp_query_done(now, io);
            }
        }
    }

    /// One query's answer is complete: read it, then the next query.
    fn sdp_query_done(&mut self, now: u64, io: &mut impl Io) {
        let Some(l) = self.link else { return };
        let uuid = Self::sdp_uuid(l.purpose, l.sdp_query).unwrap_or(0);
        let lists = &self.sdp_buf[..self.sdp_len];
        let mut n_records = 0;
        let mut pnp = None;
        let mut desc_len = None;
        let mut hid_info = None;
        for (r, rec) in sdp::records(lists).enumerate() {
            n_records += 1;
            for (id, v) in sdp::attributes(rec) {
                log!(io, Sdp, "record={} attr={:#06x} {}", r, id, v);
            }
            if sdp::has_class(rec, sdp::UUID_HID) && hid_info.is_none() {
                let (h, d) = sdp::read_hid(rec);
                hid_info = Some(h);
                if let Some(d) = d {
                    let n = d.len().min(DESCRIPTOR_MAX);
                    self.desc[..n].copy_from_slice(&d[..n]);
                    desc_len = Some((n, d.len()));
                }
            }
            if sdp::has_class(rec, sdp::UUID_PNP) && pnp.is_none() {
                pnp = Some(sdp::read_pnp(rec));
            }
        }
        log!(
            io,
            Sdp,
            "query uuid={:#06x} done records={} bytes={} truncated={} pieces={}",
            uuid,
            n_records,
            self.sdp_len,
            u8::from(self.sdp_truncated),
            l.sdp_pieces
        );
        if let Some(h) = hid_info {
            log!(io, Sdp, "hid {:?}", h);
            if let Some(m) = self.link.as_mut() {
                m.lsto_wish = h.supervision_timeout;
            }
            self.supervision(io);
        }
        if let Some(p) = pnp {
            log!(
                io,
                Sdp,
                "device-id vendor={:04x?} product={:04x?} version={:04x?} source={:?}({}) spec={:04x?} primary={:?}",
                p.vendor_id,
                p.product_id,
                p.version,
                p.vendor_id_source,
                match p.vendor_id_source {
                    Some(1) => "bluetooth-sig",
                    Some(2) => "usb",
                    _ => "?",
                },
                p.specification_id,
                p.primary_record
            );
            if let Some(m) = self.link.as_mut() {
                m.pnp = p;
            }
            self.ui_device(io);
        }
        if let Some((n, full)) = desc_len {
            self.desc_len = n;
            if let Some(m) = self.link.as_mut() {
                m.desc_bytes = full;
            }
            log!(
                io,
                Sdp,
                "descriptor bytes={}{}",
                full,
                if full > n { " (cut to fit)" } else { "" }
            );
            for (k, piece) in self.desc[..n].chunks(64).enumerate() {
                log!(io, Sdp, "descriptor off={} hex={}", k * 64, Hex(piece));
            }
            io.ui(UiEvent::Descriptor(&self.desc[..n]));
        }
        self.sdp_len = 0;
        self.sdp_truncated = false;
        if let Some(m) = self.link.as_mut() {
            m.sdp_query += 1;
            m.sdp_cont_len = 0;
            m.sdp_pieces = 0;
        }
        if self
            .link
            .is_some_and(|l| Self::sdp_uuid(l.purpose, l.sdp_query).is_some())
        {
            self.sdp_send_query(now, io);
        } else {
            self.sdp_finish(now, io);
        }
    }

    /// SDP is over (done, failed or timed out): close its channel and go on.
    fn sdp_finish(&mut self, now: u64, io: &mut impl Io) {
        let Some(l) = self.link else { return };
        if l.sdp == SdpState::Done {
            return;
        }
        if let Some(m) = self.link.as_mut() {
            m.sdp = SdpState::Done;
        }
        if let Some(i) = self.chans.iter().position(|c| {
            c.psm == l2cap::PSM_SDP
                && c.ours
                && matches!(c.st, ChSt::Open | ChSt::Config | ChSt::Connecting)
        }) {
            self.close_channel(now, io, i);
        }
        match l.purpose {
            Purpose::Gamepad => {
                if self.any_chan(l2cap::PSM_HID_CONTROL).is_none() {
                    if l.incoming {
                        // A gamepad that calls usually opens HID itself.
                        log!(
                            io,
                            State,
                            "SDP over; giving the gamepad {} ms to open the HID channels itself",
                            PAD_OPENS_GRACE
                        );
                        if let Some(m) = self.link.as_mut() {
                            m.open_ctrl_at = Some(now + PAD_OPENS_GRACE);
                        }
                    } else {
                        // We called: open at once, no need to wait.
                        self.status(io, format_args!("Opening HID channels"));
                        self.open_channel(now, io, l2cap::PSM_HID_CONTROL);
                    }
                }
            }
            Purpose::SdpProbe { .. } => {
                self.status(io, format_args!("Probe done, closing"));
                if let Some(m) = self.link.as_mut() {
                    m.disconnect_at = Some(now + PROBE_HOLD);
                }
            }
        }
    }

    /// The other side asks us what services we offer: we offer none.
    fn on_sdp_request(&mut self, remote: u16, payload: &[u8], io: &mut impl Io) {
        // Their bytes are shown with the pad's address hidden (a stray
        // Switch device-info reply here carries it).
        let addr = self.link.map(|l| l.addr);
        match sdp::parse_pdu(payload) {
            Some(sdp::Pdu::Request { id, tid, params }) => {
                log!(
                    io,
                    Sdp,
                    "the other side asks us: pdu={:#04x} tid={} params={} (we answer: no services)",
                    id,
                    tid,
                    HexHide(&params[..params.len().min(32)], addr)
                );
                let mut out = [0_u8; 16];
                let n = sdp::server_answer(&mut out, id, tid);
                self.send_frame(io, remote, &out[..n]);
            }
            _ => log!(
                io,
                Sdp,
                "unexpected pdu from the other side: {}",
                HexHide(&payload[..payload.len().min(32)], addr)
            ),
        }
    }

    // -------------------------------------------------------------- HID

    fn on_hid_interrupt(&mut self, now: u64, payload: &[u8], io: &mut impl Io) {
        let addr = self.link.map(|l| l.addr);
        let Some(report) = hid::input_report(payload) else {
            log!(
                io,
                Hid,
                "interrupt {} len={} hex={}",
                hid::kind_name(payload.first().copied().unwrap_or(0)),
                payload.len(),
                HexHide(&payload[..payload.len().min(64)], addr)
            );
            return;
        };
        self.stats.reports += 1;
        let Some(l) = self.link.as_mut() else { return };
        l.reports += 1;
        l.reports_second += 1;
        let n = l.reports;
        if l.first_report_at.is_none() {
            l.first_report_at = Some(now);
            log!(
                io,
                State,
                "first input report {} ms after the HID channels opened ({} ms after the link came up)",
                now.saturating_sub(l.hid_up_at.unwrap_or(now)),
                now.saturating_sub(l.up_at)
            );
        }
        if l.stopped_logged {
            l.stopped_logged = false;
            log!(
                io,
                State,
                "input reports again after {} ms of silence",
                now.saturating_sub(l.last_report_at)
            );
        }
        l.last_report_at = now;
        count_id(l, report);
        // The first ones in full; after that, the first of each second.
        if n <= REPORTS_LOGGED || l.reports_second == 1 {
            log!(
                io,
                Hid,
                "report n={} len={} hex={}",
                n,
                report.len(),
                HexHide(report, addr)
            );
        }
        io.ui(UiEvent::Report(report));
    }

    fn on_hid_control(&mut self, now: u64, payload: &[u8], io: &mut impl Io) {
        let h = payload.first().copied().unwrap_or(0);
        let addr = self.link.map(|l| l.addr);
        if hid::kind(h) == hid::HANDSHAKE {
            // A SET_REPORT of ours is answered first; otherwise it may be
            // the error answer to a GET_REPORT.
            let (wait, get_wait) = match self.link.as_mut() {
                Some(l) => {
                    l.handshakes += 1;
                    match l.set_report_wait.take() {
                        Some(w) => (Some(w), None),
                        None => (None, l.get_report_wait.take()),
                    }
                }
                None => (None, None),
            };
            if let Some((at, id)) = get_wait {
                log!(
                    io,
                    Hid,
                    "handshake result={}({:#03x}) after get-report id={:#04x} in {} ms (the pad did not send the report)",
                    hid::handshake_text(hid::param(h)),
                    hid::param(h),
                    id,
                    now.saturating_sub(at)
                );
            }
            match wait {
                _ if get_wait.is_some() => {}
                Some((at, n)) => log!(
                    io,
                    Hid,
                    "handshake result={}({:#03x}) after set-report n={} in {} ms",
                    hid::handshake_text(hid::param(h)),
                    hid::param(h),
                    n,
                    now.saturating_sub(at)
                ),
                None => log!(
                    io,
                    Hid,
                    "handshake result={}({:#03x}) with no set-report of ours waiting",
                    hid::handshake_text(hid::param(h)),
                    hid::param(h)
                ),
            }
        } else {
            if h == hid::DATA_INPUT {
                let id = payload.get(1).copied();
                let wait = self.link.as_mut().and_then(|l| match l.get_report_wait {
                    Some((at, want)) if Some(want) == id => {
                        l.get_report_wait = None;
                        Some((at, want))
                    }
                    _ => None,
                });
                if let Some((at, want)) = wait {
                    log!(
                        io,
                        Hid,
                        "get-report answer id={:#04x} len={} in {} ms",
                        want,
                        payload.len() - 1,
                        now.saturating_sub(at)
                    );
                }
            }
            let (sep, what) = match hid::kind(h) {
                hid::HID_CONTROL => (" ", hid::control_text(hid::param(h))),
                _ => ("", ""),
            };
            log!(
                io,
                Hid,
                "control {}{}{} len={} hex={}",
                hid::kind_name(h),
                sep,
                what,
                payload.len(),
                HexHide(&payload[..payload.len().min(64)], addr)
            );
        }
        if !payload.is_empty() {
            io.ui(UiEvent::Control(payload));
        }
        if hid::kind(h) == hid::HID_CONTROL && hid::param(h) == hid::CONTROL_VIRTUAL_CABLE_UNPLUG {
            if let Some(l) = self.link {
                self.forget_key(l.addr, ForgetReason::Unplugged, io);
            }
            self.status(io, format_args!("Gamepad unpaired itself"));
            self.disconnect(io, REASON_USER_ENDED);
        }
    }
}

/// The items of an allow-list ([`Config::only`]).
fn only_items(list: &str) -> impl Iterator<Item = &str> {
    list.split(',').map(str::trim).filter(|t| !t.is_empty())
}

/// May a gamepad heard in a search be called? With no list (or an empty
/// one), yes; otherwise its address or a piece of its name must match.
pub fn allowed(only: Option<&str>, addr: BdAddr, name: &[u8]) -> bool {
    let Some(list) = only else { return true };
    let mut any = false;
    for item in only_items(list) {
        any = true;
        let hit = match BdAddr::parse(item) {
            Some(a) => a == addr,
            None => contains_nocase(name, item.as_bytes()),
        };
        if hit {
            return true;
        }
    }
    !any
}

/// May a device that calls us be accepted? Only addresses can be checked
/// (a call carries no name): with no address in the list, yes.
pub fn allowed_call(only: Option<&str>, addr: BdAddr) -> bool {
    let Some(list) = only else { return true };
    let mut addrs = false;
    for a in only_items(list).filter_map(BdAddr::parse) {
        addrs = true;
        if a == addr {
            return true;
        }
    }
    !addrs
}

fn contains_nocase(hay: &[u8], pat: &[u8]) -> bool {
    !pat.is_empty()
        && hay.len() >= pat.len()
        && hay.windows(pat.len()).any(|w| w.eq_ignore_ascii_case(pat))
}

/// Commands whose parameters start with a device address (and whose
/// Command Complete, if any, gives it back after the status byte).
fn starts_with_address(opcode: u16) -> bool {
    matches!(
        opcode,
        op::CREATE_CONNECTION
            | op::CREATE_CONNECTION_CANCEL
            | op::ACCEPT_CONNECTION_REQUEST
            | op::REJECT_CONNECTION_REQUEST
            | op::LINK_KEY_REQUEST_REPLY
            | op::LINK_KEY_REQUEST_NEGATIVE_REPLY
            | op::PIN_CODE_REQUEST_REPLY
            | op::PIN_CODE_REQUEST_NEGATIVE_REPLY
            | op::REMOTE_NAME_REQUEST
            | op::IO_CAPABILITY_REQUEST_REPLY
            | op::IO_CAPABILITY_REQUEST_NEGATIVE_REPLY
            | op::USER_CONFIRMATION_REQUEST_REPLY
            | op::USER_CONFIRMATION_REQUEST_NEGATIVE_REPLY
            | op::USER_PASSKEY_REQUEST_NEGATIVE_REPLY
    )
}

/// Commands that never leave in reconnect-only mode: a search, a call of
/// our own, and every "yes" to a pairing.
fn forbidden_when_reconnect_only(opcode: u16) -> bool {
    matches!(
        opcode,
        op::INQUIRY
            | op::CREATE_CONNECTION
            | op::IO_CAPABILITY_REQUEST_REPLY
            | op::USER_CONFIRMATION_REQUEST_REPLY
            | op::PIN_CODE_REQUEST_REPLY
    )
}

/// Hex like [`Hex`], with the bytes at `hide` written "xx".
struct HexMask<'a>(&'a [u8], core::ops::Range<usize>);

impl fmt::Display for HexMask<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, b) in self.0.iter().enumerate() {
            if self.1.contains(&i) {
                f.write_str("xx")?;
            } else {
                write!(f, "{b:02x}")?;
            }
        }
        Ok(())
    }
}

/// "LMP transaction collision" and "different transaction collision": both
/// sides started a security step at the same moment.
fn is_collision(status: u8) -> bool {
    matches!(status, 0x23 | 0x2A)
}

fn key_type_text(t: u8) -> &'static str {
    match t {
        0x00 => "combination",
        0x03 => "debug",
        0x04 => "unauthenticated-p192",
        0x05 => "authenticated-p192",
        0x06 => "changed-combination",
        0x07 => "unauthenticated-p256",
        0x08 => "authenticated-p256",
        _ => "other",
    }
}

fn io_cap_text(c: u8) -> &'static str {
    match c {
        0 => "display-only",
        1 => "display-yes-no",
        2 => "keyboard-only",
        3 => "no-input-no-output",
        _ => "?",
    }
}

fn auth_req_text(a: u8) -> &'static str {
    match a {
        0x00 => "no-bonding",
        0x01 => "no-bonding-mitm",
        0x02 => "dedicated-bonding",
        0x03 => "dedicated-bonding-mitm",
        0x04 => "general-bonding",
        0x05 => "general-bonding-mitm",
        _ => "?",
    }
}

fn conn_result_text(r: u16) -> &'static str {
    match r {
        0 => "success",
        1 => "pending",
        2 => "psm-not-supported",
        3 => "security-block",
        4 => "no-resources",
        6 => "invalid-source-cid",
        7 => "source-cid-in-use",
        _ => "other",
    }
}

fn conf_result_text(r: u16) -> &'static str {
    match r {
        0 => "success",
        1 => "unacceptable-parameters",
        2 => "rejected",
        3 => "unknown-options",
        4 => "pending",
        5 => "flow-spec-rejected",
        _ => "other",
    }
}

/// A few words on an Information Response, for the log.
struct InfoText {
    ertm: bool,
    streaming: bool,
    fcs: bool,
    fixed: bool,
    known: bool,
}

impl fmt::Display for InfoText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.known {
            return Ok(());
        }
        write!(
            f,
            " (ertm={} streaming={} fcs={} fixed_channels={})",
            u8::from(self.ertm),
            u8::from(self.streaming),
            u8::from(self.fcs),
            u8::from(self.fixed)
        )
    }
}

fn info_text(info_type: u16, result: u16, data: &[u8]) -> InfoText {
    let mask = if info_type == 2 && result == 0 {
        crate::buf::u32_at(data, 0)
    } else {
        None
    };
    let m = mask.unwrap_or(0);
    InfoText {
        ertm: m & 0x08 != 0,
        streaming: m & 0x10 != 0,
        fcs: m & 0x20 != 0,
        fixed: m & 0x80 != 0,
        known: mask.is_some(),
    }
}

/// Counts one input report by its first byte.
fn count_id(l: &mut Link, report: &[u8]) {
    let Some(&id) = report.first() else { return };
    let len = report.len().min(usize::from(u16::MAX)) as u16;
    let i = match l.ids[..l.ids_n].iter().position(|c| c.id == id) {
        Some(i) => i,
        None if l.ids_n < IDS_COUNTED => {
            l.ids[l.ids_n] = IdCount {
                id,
                min_len: len,
                max_len: len,
                count: 0,
            };
            l.ids_n += 1;
            l.ids_n - 1
        }
        // More different first bytes than we count: the total still counts.
        None => return,
    };
    let c = &mut l.ids[i];
    c.min_len = c.min_len.min(len);
    c.max_len = c.max_len.max(len);
    c.count = c.count.saturating_add(1);
}

/// "30:49x812,21:49x3" (id:length x count), "3f:12-13x5" when the length
/// changed, "-" when none.
struct Ids<'a>(&'a [IdCount]);

impl fmt::Display for Ids<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return f.write_str("-");
        }
        for (k, c) in self.0.iter().enumerate() {
            if k > 0 {
                f.write_str(",")?;
            }
            if c.min_len == c.max_len {
                write!(f, "{:02x}:{}x{}", c.id, c.min_len, c.count)?;
            } else {
                write!(f, "{:02x}:{}-{}x{}", c.id, c.min_len, c.max_len, c.count)?;
            }
        }
        Ok(())
    }
}

/// Hex like [`Hex`], but six bytes that spell the gamepad's own address
/// (in either byte order: HCI sends it backwards, a Switch pad's device
/// info answer forwards) come out as "xxxxxxxxxxxx".
struct HexHide<'a>(&'a [u8], Option<BdAddr>);

impl fmt::Display for HexHide<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.0;
        let private = info_address_at(b);
        let mut i = 0;
        while i < b.len() {
            if private.as_ref().is_some_and(|r| r.contains(&i)) {
                f.write_str("xx")?;
                i += 1;
                continue;
            }
            if let Some(a) = self.1
                && let Some(w) = b.get(i..i + 6)
            {
                let back = w.iter().rev().eq(a.0.iter());
                if w == a.0 || back {
                    f.write_str("xxxxxxxxxxxx")?;
                    i += 6;
                    continue;
                }
            }
            write!(f, "{:02x}", b[i])?;
            i += 1;
        }
        Ok(())
    }
}

/// Where a Switch device-info answer (report 0x21 to subcommand 0x02)
/// carries the pad's own address, which need not be the link address the
/// rest of [`HexHide`] looks for: bytes 19..25 of the report, one later
/// when a HID header byte comes first.
fn info_address_at(b: &[u8]) -> Option<core::ops::Range<usize>> {
    let at = match b {
        [0x21, ..] => 0,
        [h, 0x21, ..] if h >> 4 == hid::DATA => 1,
        _ => return None,
    };
    if b.get(at + 14) == Some(&0x02) && b.len() > at + 19 {
        Some(at + 19..at + 25)
    } else {
        None
    }
}

/// Four hex digits, or "-".
struct Opt16(Option<u16>);

impl fmt::Display for Opt16 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(v) => write!(f, "{v:04x}"),
            None => f.write_str("-"),
        }
    }
}

/// A number in tenths ("12.5"), or "-".
struct Tenths(Option<u32>);

impl fmt::Display for Tenths {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(v) => write!(f, "{}.{}", v / 10, v % 10),
            None => f.write_str("-"),
        }
    }
}

/// Milliseconds, or "-".
struct Ms(Option<u64>);

impl fmt::Display for Ms {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(v) => write!(f, "{v}"),
            None => f.write_str("-"),
        }
    }
}

/// " reason=0x13(remote-user-ended)" for an end line, nothing otherwise.
struct EndReason(Option<u8>);

impl fmt::Display for EndReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(r) => write!(f, " reason={:#04x}({})", r, hci::status_text(r)),
            None => Ok(()),
        }
    }
}

/// What a channel that timed out was waiting for.
struct ChanWait(Chan);

impl fmt::Display for ChanWait {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = self.0;
        match c.st {
            ChSt::Connecting => write!(
                f,
                "their connection-response to our request id={}",
                c.req_id
            ),
            ChSt::Pending => write!(
                f,
                "encryption, to answer their connection-request id={} (we said pending)",
                c.their_req_id
            ),
            ChSt::Config => match (c.our_conf_ok, c.their_conf_ok) {
                (false, false) => write!(
                    f,
                    "their configure-response to our request id={} and their own configure-request",
                    c.req_id
                ),
                (false, true) => {
                    write!(f, "their configure-response to our request id={}", c.req_id)
                }
                (true, false) => f.write_str("their configure-request"),
                (true, true) => f.write_str("nothing (it should be open)"),
            },
            ChSt::Closing => write!(
                f,
                "their disconnection-response to our request id={}",
                c.req_id
            ),
            ChSt::Free | ChSt::Open => f.write_str("-"),
        }
    }
}

/// The pad's supervision timeout wish, for the log.
struct Wish(Option<u16>);

impl fmt::Display for Wish {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(w) => write!(f, "{} slots ({} ms)", w, u32::from(w) * 625 / 1000),
            None => f.write_str("nothing (yet)"),
        }
    }
}
