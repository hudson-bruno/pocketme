//! The hand-over between the CYD's two cores (feature `core2`,
//! boards/cyd/src/core2.rs), as plain logic: no pins, no esp crates. The
//! firmware's first core runs these very machines; the PC tests run them
//! against a model of the second core, in every order the two cores can
//! take (tests/interleave.rs).
//!
//! The shared words ([`Words`]):
//!
//! - the job box: [`EMPTY`] (the second core holds nothing), [`PRESENT`]
//!   (send the handed picture), [`HOLD`] (park in RAM for the flash).
//!   Only the first core puts a job in, and only into an empty box; only
//!   the second core empties it, after its last use of what the job gave
//!   it.
//! - `hold`: the first core wants the second parked; `held`: the second
//!   says it is (it waits in RAM with its interrupts off).
//!
//! The first core's side, each a machine stepped in a loop with the time
//! (any counter that wraps: the firmware gives CPU cycles):
//!
//! - [`Join`]: wait until the box is empty. Bounded: after the limit
//!   ([`JOIN_LIMIT_US`], 500 ms; the longest send measured on the board
//!   is about 33 ms) it answers [`Step::TooLong`], and the firmware stops
//!   the console with a message. It never touches the screen again then,
//!   so whatever the second core still does, the two never meet on the
//!   SPI bus.
//! - [`hand`]: put a job into the box, only if it is empty (a broken
//!   rule is reported, not hidden).
//! - [`Park`]: join, ask for the hold, wait until it is held (at most
//!   [`HOLD_WAIT_US`]); if it is not, let go and join again.
//! - [`Unpark`]: let go, join.
//!
//! The joins after letting go (in [`Park`] when it gives up, and in
//! [`Unpark`]) are not needed for the rules (every use of the screen,
//! the slot or the flash begins with a join of its own, and the tests
//! pass without them); they bring the second core back to idle before
//! the first core goes on, so a hang shows up where it began.
//!
//! Each step does at most one access to a shared word, so the tests can
//! put the other core's steps between any two of them. The model is
//! sequentially consistent; the orderings the firmware gives each access
//! (Acquire loads, Release stores) are argued in core2.rs.
//!
//! No std, no heap, no unsafe.

#![no_std]
#![forbid(unsafe_code)]

/// The box is empty: the second core holds nothing.
pub const EMPTY: u32 = 0;
/// Job: send the picture in the slot.
pub const PRESENT: u32 = 1;
/// Job: park in RAM until `hold` is false again.
pub const HOLD: u32 = 2;

/// The longest the first core waits for the second core to finish a
/// job, in microseconds. The longest send measured on the board is about
/// 33 ms (`send_us_max`), the longest wait for one about 20 ms.
pub const JOIN_LIMIT_US: u32 = 500_000;
/// The longest the first core waits for the second core to park, in
/// microseconds (measured on the board: at most 8 us).
pub const HOLD_WAIT_US: u32 = 100_000;

// The firmware counts both in CPU cycles (240 MHz): they must fit a u32
// with room to spare, and parking must give up before a join would.
const _: () = assert!((JOIN_LIMIT_US as u64) * 240 < (u32::MAX as u64) / 2);
const _: () = assert!(HOLD_WAIT_US < JOIN_LIMIT_US);

/// The words the two cores share, as the first core reads and writes
/// them. The firmware's loads are Acquire and its stores Release.
pub trait Words {
    /// The job box.
    fn job(&self) -> u32;
    fn set_job(&self, job: u32);
    /// The first core wants the second parked.
    fn set_hold(&self, on: bool);
    /// The second core is parked.
    fn held(&self) -> bool;
}

/// Whether more than `limit` has passed from `start` to `now`, on a
/// counter that wraps (u32 CPU cycles wrap every 17.9 s at 240 MHz; the
/// limits are far shorter).
pub fn over(start: u32, now: u32, limit: u32) -> bool {
    now.wrapping_sub(start) > limit
}

/// What one step of a wait gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Step {
    /// Not yet: step again.
    Again,
    /// Done: the box is empty.
    Done,
    /// The limit passed and the box is still full: the second core hangs
    /// (or panicked). The firmware stops.
    TooLong,
}

/// Waiting until the box is empty, at most `limit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Join {
    start: u32,
    limit: u32,
}

impl Join {
    pub fn new(now: u32, limit: u32) -> Join {
        Join { start: now, limit }
    }

    /// One look at the box (one shared access). An empty box wins over
    /// the time: a job done right at the limit is done.
    pub fn step<W: Words + ?Sized>(&self, w: &W, now: u32) -> Step {
        if w.job() == EMPTY {
            Step::Done
        } else if over(self.start, now, self.limit) {
            Step::TooLong
        } else {
            Step::Again
        }
    }
}

/// Puts `job` into the box if it is empty. `false` (and nothing changed)
/// if it is not: the first core broke the rule "join first". Then wake
/// the second core.
pub fn hand<W: Words + ?Sized>(w: &W, job: u32) -> bool {
    if w.job() != EMPTY {
        return false;
    }
    w.set_job(job);
    true
}

/// What one step of [`Park`] gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParkStep {
    /// Not yet: step again.
    Again,
    /// The hold job is in the box: wake the second core, then step again.
    Wake,
    /// The second core is parked: the flash may be used until [`Unpark`].
    Held,
    /// It did not park in time and was let go again (the box is empty):
    /// the flash must not be used.
    NotHeld,
    /// A join passed its limit: the firmware stops.
    TooLong,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ParkState {
    Join(Join),
    AskHold,
    AskJob,
    Wait(u32),
    LetGo,
    Rejoin(Join),
    Finished(ParkStep),
}

/// Parking the second core for the flash: join, `hold` on, the hold job
/// into the box, wake, wait for `held` (at most `hold_limit`); if it does
/// not come, `hold` off and join again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Park {
    state: ParkState,
    join_limit: u32,
    hold_limit: u32,
}

impl Park {
    pub fn new(now: u32, join_limit: u32, hold_limit: u32) -> Park {
        Park {
            state: ParkState::Join(Join::new(now, join_limit)),
            join_limit,
            hold_limit,
        }
    }

    /// One step (at most one shared access). After `Held`, `NotHeld` or
    /// `TooLong` the machine is finished and answers the same again
    /// without touching anything.
    pub fn step<W: Words + ?Sized>(&mut self, w: &W, now: u32) -> ParkStep {
        match self.state {
            ParkState::Join(j) => match j.step(w, now) {
                Step::Again => ParkStep::Again,
                Step::Done => {
                    self.state = ParkState::AskHold;
                    ParkStep::Again
                }
                Step::TooLong => self.finish(ParkStep::TooLong),
            },
            ParkState::AskHold => {
                w.set_hold(true);
                self.state = ParkState::AskJob;
                ParkStep::Again
            }
            ParkState::AskJob => {
                // The box is empty (joined above; only this core fills it).
                w.set_job(HOLD);
                self.state = ParkState::Wait(now);
                ParkStep::Wake
            }
            ParkState::Wait(start) => {
                if w.held() {
                    self.finish(ParkStep::Held)
                } else if over(start, now, self.hold_limit) {
                    self.state = ParkState::LetGo;
                    ParkStep::Again
                } else {
                    ParkStep::Again
                }
            }
            ParkState::LetGo => {
                // If it parks late, it sees `hold` off at once and leaves.
                w.set_hold(false);
                self.state = ParkState::Rejoin(Join::new(now, self.join_limit));
                ParkStep::Again
            }
            ParkState::Rejoin(j) => match j.step(w, now) {
                Step::Again => ParkStep::Again,
                Step::Done => self.finish(ParkStep::NotHeld),
                Step::TooLong => self.finish(ParkStep::TooLong),
            },
            ParkState::Finished(out) => out,
        }
    }

    fn finish(&mut self, out: ParkStep) -> ParkStep {
        self.state = ParkState::Finished(out);
        out
    }
}

/// Letting the parked second core go: `hold` off, then join (it empties
/// the box as it leaves).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Unpark {
    join: Option<Join>,
    limit: u32,
}

impl Unpark {
    pub fn new(limit: u32) -> Unpark {
        Unpark { join: None, limit }
    }

    /// One step (at most one shared access).
    pub fn step<W: Words + ?Sized>(&mut self, w: &W, now: u32) -> Step {
        match self.join {
            None => {
                w.set_hold(false);
                self.join = Some(Join::new(now, self.limit));
                Step::Again
            }
            Some(j) => j.step(w, now),
        }
    }
}
