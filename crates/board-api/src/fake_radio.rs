//! The fake pad's pairing radio (feature `testing`): a script the test
//! chooses plays the board's side of [`crate::Pad`]'s pairing methods, so
//! the console's pages can be tested and pictured without a radio.
//!
//! It has no clock. It moves one "turn" at a time: by default every
//! [`crate::Pad::pair_state`] call is one turn (the console reads the
//! state once per step while a pairing page is open, so a turn is a
//! step); with [`FakeRadio::manual`] set, only [`FakeRadio::turn`] moves
//! it. A turn first serves a waiting request, as the real board does
//! after the step; a turn with no request moves the script on by one.
//! The same calls always give the same states.
//!
//! It follows the rules of [`crate::pair`] the real board will follow:
//! one request box where a waiting Cancel beats the rest, endings that
//! stay until Ack or the next Begin, a found list that only grows during
//! a search, Choose and Forget refused when their tail does not match,
//! a linked pad hung up when a search starts, and a full list refused
//! at Choose (a saved pad may always be paired again).
//!
//! Every tail and name here is made up.

use crate::pair::{FoundPad, PairFail, PairReq, PairState, SavedPad};

/// Places for saved pads on the fake (four, as on the console firmware).
pub const FAKE_PLACES: usize = 4;

/// The most found pads one script holds (eight, as pad-host keeps).
pub const FAKE_FOUND_MAX: usize = 8;

/// Turns in [`PairState::Starting`] in [`PairScript::new`].
pub const FAKE_START_TURNS: u16 = 1;
/// Turns of searching in [`PairScript::new`] (60 s at 60 steps a second).
pub const FAKE_SEARCH_TURNS: u16 = 3600;
/// Turns of calling in [`PairScript::new`].
pub const FAKE_CALL_TURNS: u16 = 90;

/// Made-up tails and names for found pads.
pub const FAKE_TAIL_A: [u8; 2] = [0xA0, 0x01];
pub const FAKE_NAME_A: &[u8] = b"TEST PAD A";
pub const FAKE_TAIL_B: [u8; 2] = [0xB0, 0x02];
pub const FAKE_NAME_B: &[u8] = b"TEST PAD B";

/// Made-up tails of four saved pads (see [`FakeRadio::full`]). None of
/// them is [`FAKE_TAIL_A`] or [`FAKE_TAIL_B`].
pub const FAKE_SAVED_TAILS: [[u8; 2]; FAKE_PLACES] =
    [[0x51, 0x01], [0x52, 0x02], [0x53, 0x03], [0x54, 0x04]];

/// What the fake radio does, chosen by the test. Build it with
/// [`PairScript::new`] and the `const` methods below, or take one of the
/// ready cases ([`PairScript::found_and_saved`] and the rest).
///
/// Turns count from the moment a phase starts: `start` turns after a
/// Begin was taken the search starts; a pad found `at` turns into the
/// search is in the list from that turn on; after `search` turns with no
/// Choose the window ends with [`PairFail::Timeout`]; `call` turns after
/// a Choose was taken the call ends. A count of 0 acts as 1, except
/// `start`: 0 starts the search at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairScript {
    /// Begin is refused with this (for example [`PairFail::NotHeld`],
    /// [`PairFail::Linked`], [`PairFail::Damaged`], [`PairFail::Busy`]).
    /// `None`: Begin is taken (unless [`FakeRadio::slots`] is `Some(0)`,
    /// which gives [`PairFail::NoRoom`]).
    pub begin: Option<PairFail>,
    /// Turns in [`PairState::Starting`].
    pub start: u16,
    /// Turns the search lasts.
    pub search: u16,
    /// Turns the call lasts.
    pub call: u16,
    /// How the call ends: `None` saves the pad, `Some(f)` fails with `f`
    /// (for example [`PairFail::OldPin`], [`PairFail::Lost`],
    /// [`PairFail::Refused`], [`PairFail::Store`], [`PairFail::Timeout`]).
    pub call_end: Option<PairFail>,
    /// Forget is refused with this. [`PairFail::NotHeld`] is given before
    /// the tail is looked at (the board checks the pin first); any other
    /// only for a Forget whose tail matches (a failed flash write).
    pub forget: Option<PairFail>,
    /// The pads the search finds, in the order found, with the turn each
    /// is found at. Only the first `found_len` count.
    found: [(u16, FoundPad); FAKE_FOUND_MAX],
    found_len: u8,
}

impl PairScript {
    /// Begin is taken, starts in [`FAKE_START_TURNS`], searches for
    /// [`FAKE_SEARCH_TURNS`] and finds nothing; a call would take
    /// [`FAKE_CALL_TURNS`] and save the pad; Forget works.
    pub const fn new() -> PairScript {
        PairScript {
            begin: None,
            start: FAKE_START_TURNS,
            search: FAKE_SEARCH_TURNS,
            call: FAKE_CALL_TURNS,
            call_end: None,
            forget: None,
            found: [(0, FoundPad::new([0, 0], b"")); FAKE_FOUND_MAX],
            found_len: 0,
        }
    }

    /// Adds a pad the search finds `at` turns into the search. Pads must
    /// be added in the order they are found (`at` never falls); at most
    /// [`FAKE_FOUND_MAX`].
    pub const fn find(mut self, at: u16, tail: [u8; 2], name: &[u8]) -> PairScript {
        let n = self.found_len as usize;
        assert!(n < FAKE_FOUND_MAX, "a script finds at most eight pads");
        assert!(
            n == 0 || self.found[n - 1].0 <= at,
            "add found pads in time order"
        );
        self.found[n] = (at, FoundPad::new(tail, name));
        self.found_len += 1;
        self
    }

    /// Begin is refused with `fail`.
    pub const fn begin_fails(mut self, fail: PairFail) -> PairScript {
        self.begin = Some(fail);
        self
    }

    /// The board takes `turns` turns to start the search.
    pub const fn starting(mut self, turns: u16) -> PairScript {
        self.start = turns;
        self
    }

    /// The search lasts `turns` turns.
    pub const fn searching(mut self, turns: u16) -> PairScript {
        self.search = turns;
        self
    }

    /// The call lasts `turns` turns.
    pub const fn calling(mut self, turns: u16) -> PairScript {
        self.call = turns;
        self
    }

    /// The call ends with `fail` instead of saving the pad.
    pub const fn call_fails(mut self, fail: PairFail) -> PairScript {
        self.call_end = Some(fail);
        self
    }

    /// Forget is refused with `fail`.
    pub const fn forget_fails(mut self, fail: PairFail) -> PairScript {
        self.forget = Some(fail);
        self
    }

    /// How many pads the search finds.
    pub const fn found_len(&self) -> u8 {
        self.found_len
    }

    // Ready cases. Each names its pads with the made-up tails and names
    // above.

    /// Pad A is found `n` turns into the search and saved `m` turns after
    /// it is chosen.
    pub const fn found_and_saved(n: u16, m: u16) -> PairScript {
        PairScript::new()
            .find(n, FAKE_TAIL_A, FAKE_NAME_A)
            .calling(m)
    }

    /// Pad A is found `a` turns into the search, pad B `b` turns in
    /// (`a <= b`); either is saved when chosen.
    pub const fn two_found(a: u16, b: u16) -> PairScript {
        PairScript::new()
            .find(a, FAKE_TAIL_A, FAKE_NAME_A)
            .find(b, FAKE_TAIL_B, FAKE_NAME_B)
    }

    /// Nothing answers; the window ends after `turns` turns of search
    /// with [`PairFail::Timeout`].
    pub const fn nothing_found(turns: u16) -> PairScript {
        PairScript::new().searching(turns)
    }

    /// Pad A is found after `n` turns and, `m` turns after it is chosen,
    /// asks for an old PIN pairing: [`PairFail::OldPin`].
    pub const fn old_pin(n: u16, m: u16) -> PairScript {
        PairScript::found_and_saved(n, m).call_fails(PairFail::OldPin)
    }

    /// Pad A is found after `n` turns and the link is lost `m` turns
    /// after it is chosen: [`PairFail::Lost`].
    pub const fn link_lost(n: u16, m: u16) -> PairScript {
        PairScript::found_and_saved(n, m).call_fails(PairFail::Lost)
    }

    /// Pad A is found after `n` turns and pairs, but the record cannot be
    /// written: [`PairFail::Store`] `m` turns after it is chosen.
    pub const fn store_fails(n: u16, m: u16) -> PairScript {
        PairScript::found_and_saved(n, m).call_fails(PairFail::Store)
    }

    /// BOOT was not held when the board served Begin or Forget:
    /// [`PairFail::NotHeld`] for both.
    pub const fn not_held() -> PairScript {
        PairScript::new()
            .begin_fails(PairFail::NotHeld)
            .forget_fails(PairFail::NotHeld)
    }
}

impl Default for PairScript {
    fn default() -> Self {
        Self::new()
    }
}

/// The fake pad's pairing radio: plays a [`PairScript`] behind
/// [`crate::Pad`]'s pairing methods ([`crate::fake::FakePad::radio`]).
///
/// No clock: it moves one turn at a time. By default every
/// [`crate::Pad::pair_state`] call is one turn; with
/// [`FakeRadio::manual`] only [`FakeRadio::turn`] moves it. A turn
/// serves the waiting request (as the real board does after the step),
/// or, with none waiting, moves the script on by one. It keeps the rules
/// of [`crate::pair`]: a waiting Cancel beats the rest, endings stay
/// until Ack or the next Begin, the found list only grows during a
/// search, Choose and Forget with a wrong tail end in
/// [`PairFail::Busy`], a linked pad is hung up at Begin, and a full list
/// is refused at Choose (a saved pad may always be paired again).
///
/// [`FakeRadio::default`] is a board that cannot pair: the same answers
/// as [`crate::Pad`]'s default bodies (no saved pads, requests dropped,
/// always `Off`), so a test that does not set it up sees today's
/// console. [`FakeRadio::new`] is a board that can.
///
/// The saved list here is the radio's own: it does not change
/// [`crate::fake::FakePad::state`] (what the console reads as pad 0's
/// buttons). A test that wants both sets both.
#[derive(Clone, Copy, Debug, Default)]
pub struct FakeRadio {
    /// What [`crate::Pad::pairing`] says. When false, requests are
    /// counted and dropped.
    pub pairing: bool,
    /// What [`crate::Pad::pair_slots`] says. `Some(0)` makes Begin fail
    /// with [`PairFail::NoRoom`]; each save and each forget uses one.
    pub slots: Option<u8>,
    /// The saved pads by place. Forgetting empties a place; a new pad
    /// takes the first empty one.
    pub saved: [Option<SavedPad>; FAKE_PLACES],
    pub script: PairScript,
    /// Only [`FakeRadio::turn`] moves the radio; `pair_state` just reads.
    pub manual: bool,
    /// Requests left so far (dropped ones too).
    pub requests: u32,
    /// The last request left (perhaps dropped or beaten by a Cancel).
    pub last_request: Option<PairReq>,
    /// Turns so far.
    pub turns: u32,
    waiting: Option<PairReq>,
    state: PairState,
    tick: u16,
    found: [FoundPad; FAKE_FOUND_MAX],
    found_len: u8,
    /// The pad being called.
    target: [u8; 2],
}

impl FakeRadio {
    /// A board that can pair, playing `script`, with no saved pads and
    /// no slot count.
    pub fn new(script: PairScript) -> FakeRadio {
        FakeRadio {
            pairing: true,
            script,
            ..FakeRadio::default()
        }
    }

    /// A board that can pair, with all four places taken
    /// ([`FAKE_SAVED_TAILS`], none linked).
    pub fn full(script: PairScript) -> FakeRadio {
        let mut r = FakeRadio::new(script);
        for (place, &tail) in FAKE_SAVED_TAILS.iter().enumerate() {
            r.saved[place] = Some(SavedPad {
                tail,
                connected: false,
            });
        }
        r
    }

    /// Puts a saved pad at `place` (0 to 3).
    pub fn with_saved(mut self, place: u8, tail: [u8; 2], connected: bool) -> FakeRadio {
        self.saved[usize::from(place)] = Some(SavedPad { tail, connected });
        self
    }

    /// Sets the free record slots.
    pub fn with_slots(mut self, slots: u8) -> FakeRadio {
        self.slots = Some(slots);
        self
    }

    /// The state now, without moving a turn.
    pub fn state(&self) -> PairState {
        self.state
    }

    /// The request waiting to be served, if any.
    pub fn waiting(&self) -> Option<PairReq> {
        self.waiting
    }

    /// How many pads are in the found list now.
    pub fn found_count(&self) -> u8 {
        self.found_len
    }

    /// What [`crate::Pad::saved_pad`] gives.
    pub fn saved_pad(&self, place: u8) -> Option<SavedPad> {
        self.saved.get(usize::from(place)).copied().flatten()
    }

    /// What [`crate::Pad::pair_found`] gives.
    pub fn found(&self, i: u8) -> Option<FoundPad> {
        if i < self.found_len {
            Some(self.found[usize::from(i)])
        } else {
            None
        }
    }

    /// What [`crate::Pad::pair_request`] does: one box, the newest
    /// wins, a waiting Cancel beats the rest. Dropped when the board
    /// cannot pair.
    pub fn request(&mut self, req: PairReq) {
        self.requests += 1;
        self.last_request = Some(req);
        if !self.pairing || self.waiting == Some(PairReq::Cancel) {
            return;
        }
        self.waiting = Some(req);
    }

    /// What [`crate::Pad::pair_state`] does: one turn (unless
    /// [`FakeRadio::manual`]), then the state.
    pub fn read_state(&mut self) -> PairState {
        if !self.manual {
            self.turn();
        }
        self.state
    }

    /// One turn of the board: serve the waiting request, or, if there is
    /// none, move the script on by one.
    pub fn turn(&mut self) {
        self.turns += 1;
        match self.waiting.take() {
            Some(req) => self.serve(req),
            None => self.advance(),
        }
    }

    fn serve(&mut self, req: PairReq) {
        match req {
            PairReq::Cancel => self.state = PairState::Off,
            PairReq::Ack => {
                if self.state.is_end() {
                    self.state = PairState::Off;
                }
            }
            PairReq::Begin => {
                if !self.state.is_open() {
                    self.begin();
                }
            }
            PairReq::Choose { i, tail } => {
                if self.state == PairState::Searching {
                    self.choose(i, tail);
                }
            }
            PairReq::Forget { place, tail } => {
                if !self.state.is_open() {
                    self.forget(place, tail);
                }
            }
        }
    }

    fn begin(&mut self) {
        self.found_len = 0;
        self.tick = 0;
        if let Some(fail) = self.script.begin {
            self.state = PairState::Failed(fail);
            return;
        }
        if self.slots == Some(0) {
            self.state = PairState::Failed(PairFail::NoRoom);
            return;
        }
        // The host has one link: a linked pad is hung up first.
        self.hang_up();
        self.state = if self.script.start == 0 {
            PairState::Searching
        } else {
            PairState::Starting
        };
    }

    fn choose(&mut self, i: u8, tail: [u8; 2]) {
        if self.found(i).is_none_or(|p| p.tail != tail) {
            self.state = PairState::Failed(PairFail::Busy);
            return;
        }
        let known = self.saved.iter().flatten().any(|s| s.tail == tail);
        if !known && self.saved.iter().all(Option::is_some) {
            self.state = PairState::Failed(PairFail::Full);
            return;
        }
        self.target = tail;
        self.tick = 0;
        self.state = PairState::Calling;
    }

    fn forget(&mut self, place: u8, tail: [u8; 2]) {
        if self.script.forget == Some(PairFail::NotHeld) {
            self.state = PairState::Failed(PairFail::NotHeld);
            return;
        }
        if self.saved_pad(place).is_none_or(|s| s.tail != tail) {
            self.state = PairState::Failed(PairFail::Busy);
            return;
        }
        if let Some(fail) = self.script.forget {
            self.state = PairState::Failed(fail);
            return;
        }
        self.saved[usize::from(place)] = None;
        self.use_slot();
        self.state = PairState::Forgotten(place);
    }

    fn advance(&mut self) {
        match self.state {
            PairState::Starting => {
                self.tick = self.tick.saturating_add(1);
                if self.tick >= self.script.start {
                    self.tick = 0;
                    self.state = PairState::Searching;
                }
            }
            PairState::Searching => {
                self.tick = self.tick.saturating_add(1);
                while let Some(&(at, pad)) = self.script.found[..usize::from(self.script.found_len)]
                    .get(usize::from(self.found_len))
                {
                    if at > self.tick {
                        break;
                    }
                    self.found[usize::from(self.found_len)] = pad;
                    self.found_len += 1;
                }
                if self.tick >= self.script.search {
                    self.state = PairState::Failed(PairFail::Timeout);
                }
            }
            PairState::Calling => {
                self.tick = self.tick.saturating_add(1);
                if self.tick >= self.script.call {
                    self.finish();
                }
            }
            _ => {}
        }
    }

    fn finish(&mut self) {
        if let Some(fail) = self.script.call_end {
            self.state = PairState::Failed(fail);
            return;
        }
        let place = self
            .saved
            .iter()
            .position(|s| s.is_some_and(|s| s.tail == self.target))
            .or_else(|| self.saved.iter().position(Option::is_none));
        let Some(place) = place else {
            self.state = PairState::Failed(PairFail::Full);
            return;
        };
        self.hang_up();
        self.saved[place] = Some(SavedPad {
            tail: self.target,
            connected: true,
        });
        self.use_slot();
        self.state = PairState::Saved(place as u8);
    }

    fn hang_up(&mut self) {
        for s in self.saved.iter_mut().flatten() {
            s.connected = false;
        }
    }

    fn use_slot(&mut self) {
        self.slots = self.slots.map(|n| n.saturating_sub(1));
    }
}
