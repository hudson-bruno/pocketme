//! Pairing a gamepad: the small types the [`crate::Pad`] methods pass.
//!
//! The console's step may not touch the radio, the BOOT pin or the
//! flash, and may not wait. So pairing goes by requests, not answers:
//! the console leaves one [`PairReq`] with [`crate::Pad::pair_request`];
//! the board serves it after the step (opens the radio's window, writes
//! the flash) and reports how far it got through [`crate::Pad::pair_state`],
//! which the console reads in a later step. Refusals come back the same
//! way, as [`PairState::Failed`].
//!
//! The request box holds one request. A newer request replaces an older
//! one that was not served yet, except that a waiting [`PairReq::Cancel`]
//! is never replaced: leaving a page must always stop the radio.
//!
//! A pad is named to the console only by its "tail": the last two bytes
//! of its Bluetooth address, in the order an address is written, so a
//! pad whose address is written `..:AB:CD` has the tail `[0xAB, 0xCD]`.
//! The whole address never crosses this line. [`PairReq::Choose`] and
//! [`PairReq::Forget`] carry the tail the player saw, so a list that
//! changed between the drawn frame and the press can never hit another
//! pad: the board refuses a request whose tail does not match.
//!
//! These are plain data: no logic beyond small helpers.

/// How long BOOT must be held down, in milliseconds, on boards that
/// require a physical hold before pairing or forgetting (`PairFail::NotHeld`).
/// Boards such as the CYD trigger pairing and forgetting directly from
/// the console UI instead.
pub const PAIR_HOLD_MS: u32 = 2000;

/// The longest pad name a board hands the console, in bytes.
pub const PAD_NAME_MAX: usize = 24;

/// One saved pad, as the GAMEPADS page shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SavedPad {
    /// The last two bytes of its address (see [`crate::pair`]). The page
    /// shows them to tell saved pads apart; no names are saved.
    pub tail: [u8; 2],
    /// It is linked now (the page marks it, and may show its name).
    pub connected: bool,
}

/// One gamepad the search found, as the FOUND page shows it.
///
/// Its `Debug` output shows the tail and the name's length, never the
/// name: names of other people's pads must not end up in a log.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct FoundPad {
    /// The last two bytes of its address (see [`crate::pair`]).
    pub tail: [u8; 2],
    /// The name the pad gave, as it sent it (UTF-8 bytes, cut to
    /// [`PAD_NAME_MAX`] bytes, perhaps inside a letter). Only the first
    /// `name_len` bytes count. The console decides how to draw letters
    /// its font lacks.
    pub name: [u8; PAD_NAME_MAX],
    /// How many bytes of `name` count; 0 when the pad gave no name (the
    /// page then shows the tail alone).
    pub name_len: u8,
}

impl FoundPad {
    /// A found pad with this tail and name (cut to [`PAD_NAME_MAX`]
    /// bytes).
    pub const fn new(tail: [u8; 2], name: &[u8]) -> FoundPad {
        let mut pad = FoundPad {
            tail,
            name: [0; PAD_NAME_MAX],
            name_len: 0,
        };
        let mut i = 0;
        while i < name.len() && i < PAD_NAME_MAX {
            pad.name[i] = name[i];
            i += 1;
        }
        pad.name_len = i as u8;
        pad
    }

    /// The bytes of the name that count (empty when the pad gave none).
    pub fn name(&self) -> &[u8] {
        let n = usize::from(self.name_len).min(PAD_NAME_MAX);
        &self.name[..n]
    }
}

impl core::fmt::Debug for FoundPad {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FoundPad")
            .field("tail", &self.tail)
            .field("name_len", &self.name_len)
            .finish_non_exhaustive()
    }
}

/// What the console asks of the board's pairing. Left with
/// [`crate::Pad::pair_request`]; served after the step; the outcome comes
/// back through [`crate::Pad::pair_state`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairReq {
    /// Start a search for gamepads. The board opens the pairing window
    /// (hanging up a linked pad first if it allows doing so while linked,
    /// else returning `PairFail::Linked`): [`PairState::Starting`], then
    /// [`PairState::Searching`]. Taken when no window is open
    /// ([`PairState::Off`] or an ending); ignored while one is.
    Begin,
    /// Call and pair found pad number `i` (as [`crate::Pad::pair_found`]
    /// numbers them), whose tail the player saw as `tail`. Sent after the
    /// player said YES. Taken only while [`PairState::Searching`]; a pad
    /// that is not there or has another tail ends the window with
    /// [`PairFail::Busy`] and nothing is called.
    Choose { i: u8, tail: [u8; 2] },
    /// Stop whatever runs (search, call), drop what was not saved yet,
    /// clear an ending, and go to [`PairState::Off`]. Sent when the
    /// player leaves a pairing page. Beats every other request in the
    /// box.
    Cancel,
    /// Forget the saved pad at `place` (as [`crate::Pad::saved_pad`]
    /// numbers them), whose tail the player saw as `tail`. Sent after
    /// the player confirmed forgetting in the UI (and held BOOT on boards
    /// that require a physical hold). The board removes the pad from flash
    /// first and only then forgets/hangs it up: [`PairState::Forgotten`]
    /// or [`PairState::Failed`]. Taken when no window is open; ignored
    /// while one is.
    Forget { place: u8, tail: [u8; 2] },
    /// The page has shown an ending ([`PairState::Saved`],
    /// [`PairState::Forgotten`], [`PairState::Failed`]): go back to
    /// [`PairState::Off`]. Ignored in any other state.
    Ack,
}

/// How far the board's pairing is, as the pages show it.
///
/// `Off` is the zero value (`repr(u8)`, first variant), so a console that
/// keeps the last state in a struct that starts as all zero bytes starts
/// at `Off`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum PairState {
    /// Nothing runs and nothing waits to be shown. The page shows the
    /// saved list and PAIR NEW.
    #[default]
    Off,
    /// [`PairReq::Begin`] was taken; the board is opening the window
    /// (hanging up a linked pad, arming the radio). The page may already
    /// show SEARCHING.
    Starting,
    /// The window is open and the radio asks for gamepads. The console
    /// stays hidden. [`crate::Pad::pair_found`] grows as pads answer.
    Searching,
    /// The chosen pad is being called and paired.
    Calling,
    /// Paired: the pad is saved at this place (as
    /// [`crate::Pad::saved_pad`] numbers them) and stays linked. Stays
    /// until [`PairReq::Ack`] or the next [`PairReq::Begin`].
    Saved(u8),
    /// The pad that was at this place is forgotten: erased from flash and
    /// hung up. Stays until [`PairReq::Ack`] or the next
    /// [`PairReq::Begin`].
    Forgotten(u8),
    /// The request or the window ended without a result, for this reason.
    /// Nothing was saved or erased. Stays until [`PairReq::Ack`] or the
    /// next [`PairReq::Begin`].
    Failed(PairFail),
}

impl PairState {
    /// A window is open (starting, searching or calling): the page asks
    /// for every frame and offers STOP.
    pub const fn is_open(self) -> bool {
        matches!(
            self,
            PairState::Starting | PairState::Searching | PairState::Calling
        )
    }

    /// An ending the page shows until it sends [`PairReq::Ack`].
    pub const fn is_end(self) -> bool {
        matches!(
            self,
            PairState::Saved(_) | PairState::Forgotten(_) | PairState::Failed(_)
        )
    }
}

/// Why a pairing or a forget ended without a result. Each has its own
/// page or notice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PairFail {
    /// Optional for boards that require BOOT to be held physically on the
    /// console when serving Begin or Forget (unused on boards such as the
    /// CYD where pairing and forgetting are triggered directly from UI).
    NotHeld,
    /// A pad is linked and this board does not hang it up to pair another.
    /// (Boards that hang up the linked pad first never give it.)
    Linked,
    /// Four pads are saved and the chosen pad is not one of them. The page
    /// asks the player to forget one first. A saved pad may always be
    /// paired again: its new key replaces the old one.
    Full,
    /// The pairing area in flash has no free record left. The console
    /// never tidies it by itself, so pairing stays refused.
    NoRoom,
    /// The pairing area in flash cannot be read as a list of pads (bytes
    /// of another kind, or more pads than the console keeps). The console
    /// neither pairs nor forgets there.
    Damaged,
    /// The window ended: nothing was found or chosen in time, or the pad
    /// did not finish pairing in time.
    Timeout,
    /// The pad asked for an old PIN pairing. The console pairs no PIN.
    OldPin,
    /// The pad refused, or asked for a kind of pairing the console does
    /// not do.
    Refused,
    /// The link to the pad was lost while calling or pairing.
    Lost,
    /// Writing to flash or reading it back failed. Nothing was saved (or,
    /// for a forget, the pad is still saved).
    Store,
    /// The board cannot do this now (a game runs, the pairing part is not
    /// ready), or the request no longer matches what is there (the tail of
    /// the found or saved pad differs). Nothing was changed.
    Busy,
}
