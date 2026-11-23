//! Every order of the two cores' steps, checked.
//!
//! The first core runs the crate's own machines ([`Join`], [`hand`],
//! [`Park`], [`Unpark`]) through a short script of what the console
//! does: hand a picture over (join, touch the screen and the slot, put
//! the job in, wake), use the screen itself (join, touch), or use the
//! flash (park; only if held: use the flash, unpark). The second core is
//! a model of boards/cyd/src/core2.rs, step for step:
//!
//! - `worker`: register the waker, look into the box; empty: sleep until
//!   woken; a picture: send it (it reads the slot and uses the screen),
//!   then empty the box; the hold job: `parked`.
//! - `parked`: `held` on, spin while `hold` is on, `held` off, then the
//!   worker empties the box.
//! - the waker (esp-hal's `AtomicWaker`, behind a lock): `wake` takes the
//!   registered waker, if any, and makes the task ready.
//!
//! On top of that the second core may hang (or panic) at any point and
//! never step again, and the time may jump past every limit at any point
//! (a few times). Every reachable state is checked:
//!
//! - never the first core on the screen or the slot while the second
//!   sends (even a hung one);
//! - never the flash used unless the second core spins in `parked`;
//! - never a job put into a full box;
//! - never a job in the box with the second core asleep and not woken
//!   (a lost wake-up);
//! - without a hang, every order ends with the script done, and without
//!   a time jump the first core never stops;
//! - with a hang, the first core always ends: it finishes (it did not
//!   need the second core again) or stops at a limit, and it never
//!   waits for good.
//!
//! The model is sequentially consistent: the orderings the firmware gives
//! each access are argued in core2.rs.

use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};

use handover::{
    EMPTY, HOLD, HOLD_WAIT_US, JOIN_LIMIT_US, Join, PRESENT, Park, ParkStep, Step, Unpark, Words,
    hand, over,
};

/// Small limits for the model; a jump passes both.
const JL: u32 = 10;
const HL: u32 = 5;
const JUMP: u32 = 11;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
struct Shared {
    job: u32,
    hold: bool,
    held: bool,
    /// The worker's waker is registered (`AtomicWaker` holds it).
    registered: bool,
    /// The worker is ready to be polled (woken).
    ready: bool,
}

struct View<'a>(&'a Cell<Shared>);

impl View<'_> {
    fn edit(&self, f: impl FnOnce(&mut Shared)) {
        let mut s = self.0.get();
        f(&mut s);
        self.0.set(s);
    }
}

impl Words for View<'_> {
    fn job(&self) -> u32 {
        self.0.get().job
    }
    fn set_job(&self, job: u32) {
        self.edit(|s| s.job = job);
    }
    fn set_hold(&self, on: bool) {
        self.edit(|s| s.hold = on);
    }
    fn held(&self) -> bool {
        self.0.get().held
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Op {
    Hand,
    Screen,
    Flash,
    /// Measuring only (`hang-test`, letter `H`): join, `hold` on, the hold
    /// job in, wake, and never let go (core2.rs `hang_test`).
    HangTest,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum First {
    Next,
    HandJoin(Join),
    /// On the screen (wire table) and the slot.
    HandTouch,
    HandPut,
    HandWake,
    ScreenJoin(Join),
    /// On the screen (`Core2Screen`'s `Deref`).
    ScreenTouch,
    Parking(Park),
    ParkWake(Park),
    /// Using the flash.
    Flash,
    Unparking(Unpark),
    HangJoin(Join),
    HangHold,
    HangPut,
    HangWake,
    Finished,
    /// Stopped at a limit (the firmware panics with a message).
    Stopped,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Second {
    Poll,
    Check,
    Sleep,
    /// Sending: reads the slot and the picture, uses the screen.
    Send,
    ParkSet,
    /// In `parked`: RAM only, interrupts off.
    ParkSpin,
    ParkClear,
    Done,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct State {
    shared: Shared,
    first: First,
    pc: u8,
    second: Second,
    hung: bool,
    t: u32,
    jumps: u8,
    held_once: bool,
}

impl State {
    fn new() -> State {
        State {
            shared: Shared::default(),
            first: First::Next,
            pc: 0,
            second: Second::Poll,
            hung: false,
            t: 0,
            jumps: 0,
            held_once: false,
        }
    }

    fn terminal(&self) -> bool {
        matches!(self.first, First::Finished | First::Stopped)
    }
}

/// Breaks of the rules found while stepping.
#[derive(Default)]
struct Faults(Vec<String>);

fn wake(v: &View<'_>) {
    v.edit(|s| {
        if s.registered {
            s.registered = false;
            s.ready = true;
        }
    });
}

/// One step of the first core.
fn step_first(st: &State, script: &[Op], faults: &mut Faults) -> State {
    let cell = Cell::new(st.shared);
    let v = View(&cell);
    let mut n = *st;
    let now = st.t;
    n.first = match st.first {
        First::Next => match script.get(st.pc as usize) {
            None => First::Finished,
            Some(Op::Hand) => First::HandJoin(Join::new(now, JL)),
            Some(Op::Screen) => First::ScreenJoin(Join::new(now, JL)),
            Some(Op::Flash) => First::Parking(Park::new(now, JL, HL)),
            Some(Op::HangTest) => First::HangJoin(Join::new(now, JL)),
        },
        First::HandJoin(j) => match j.step(&v, now) {
            Step::Again => First::HandJoin(j),
            Step::Done => First::HandTouch,
            Step::TooLong => First::Stopped,
        },
        First::HandTouch => First::HandPut,
        First::HandPut => {
            if !hand(&v, PRESENT) {
                faults.0.push(format!("job put into a full box: {st:?}"));
            }
            First::HandWake
        }
        First::HandWake => {
            wake(&v);
            n.pc += 1;
            First::Next
        }
        First::ScreenJoin(j) => match j.step(&v, now) {
            Step::Again => First::ScreenJoin(j),
            Step::Done => First::ScreenTouch,
            Step::TooLong => First::Stopped,
        },
        First::ScreenTouch => {
            n.pc += 1;
            First::Next
        }
        First::Parking(mut p) => match p.step(&v, now) {
            ParkStep::Again => First::Parking(p),
            ParkStep::Wake => First::ParkWake(p),
            ParkStep::Held => {
                n.held_once = true;
                First::Flash
            }
            ParkStep::NotHeld => {
                n.pc += 1;
                First::Next
            }
            ParkStep::TooLong => First::Stopped,
        },
        First::ParkWake(p) => {
            wake(&v);
            First::Parking(p)
        }
        First::Flash => First::Unparking(Unpark::new(JL)),
        First::Unparking(mut u) => match u.step(&v, now) {
            Step::Again => First::Unparking(u),
            Step::Done => {
                n.pc += 1;
                First::Next
            }
            Step::TooLong => First::Stopped,
        },
        First::HangJoin(j) => match j.step(&v, now) {
            Step::Again => First::HangJoin(j),
            Step::Done => First::HangHold,
            Step::TooLong => First::Stopped,
        },
        First::HangHold => {
            v.set_hold(true);
            First::HangPut
        }
        First::HangPut => {
            if !hand(&v, HOLD) {
                faults
                    .0
                    .push(format!("hang test put into a full box: {st:?}"));
            }
            First::HangWake
        }
        First::HangWake => {
            wake(&v);
            n.pc += 1;
            First::Next
        }
        First::Finished | First::Stopped => st.first,
    };
    n.shared = cell.get();
    n
}

/// One step of the second core, if it can step (`Sleep` waits for a
/// wake).
fn step_second(st: &State) -> Option<State> {
    let mut n = *st;
    let s = &mut n.shared;
    n.second = match st.second {
        Second::Poll => {
            s.registered = true;
            Second::Check
        }
        Second::Check => match s.job {
            EMPTY => Second::Sleep,
            PRESENT => Second::Send,
            HOLD => Second::ParkSet,
            other => panic!("unknown job {other}"),
        },
        Second::Sleep => {
            if !s.ready {
                return None;
            }
            s.ready = false;
            Second::Poll
        }
        Second::Send => Second::Done,
        Second::ParkSet => {
            s.held = true;
            Second::ParkSpin
        }
        Second::ParkSpin => {
            if s.hold {
                Second::ParkSpin
            } else {
                Second::ParkClear
            }
        }
        Second::ParkClear => {
            s.held = false;
            Second::Done
        }
        Second::Done => {
            s.job = EMPTY;
            Second::Poll
        }
    };
    Some(n)
}

/// Every state one step on, with what moved.
fn next(st: &State, script: &[Op], hangs: bool, jumps: u8, faults: &mut Faults) -> Vec<State> {
    let mut out = Vec::new();
    if !st.terminal() {
        out.push(step_first(st, script, faults));
    }
    if !st.hung {
        if let Some(n) = step_second(st) {
            out.push(n);
        }
        if hangs {
            out.push(State { hung: true, ..*st });
        }
    }
    if st.jumps < jumps && !st.terminal() {
        out.push(State {
            t: st.t + JUMP,
            jumps: st.jumps + 1,
            ..*st
        });
    }
    out
}

fn check_rules(st: &State, faults: &mut Faults) {
    let on_screen = matches!(st.first, First::HandTouch | First::ScreenTouch);
    if on_screen && st.second == Second::Send {
        faults.0.push(format!("both cores on the screen: {st:?}"));
    }
    if st.first == First::Flash && st.second != Second::ParkSpin {
        faults
            .0
            .push(format!("flash used while the second core runs: {st:?}"));
    }
    // A lost wake-up: a job in the box, the worker asleep and not woken,
    // and the first core past its wake.
    let waking = matches!(
        st.first,
        First::HandWake | First::ParkWake(_) | First::HangWake
    );
    if st.second == Second::Sleep && !st.shared.ready && st.shared.job != EMPTY && !waking {
        faults.0.push(format!(
            "a job in the box and nobody woke the second core: {st:?}"
        ));
    }
}

/// Every reachable state and its successors.
fn explore(
    script: &[Op],
    hangs: bool,
    jumps: u8,
    faults: &mut Faults,
) -> HashMap<State, Vec<State>> {
    let mut graph = HashMap::new();
    let mut todo = VecDeque::from([State::new()]);
    while let Some(st) = todo.pop_front() {
        if graph.contains_key(&st) {
            continue;
        }
        check_rules(&st, faults);
        let succ = next(&st, script, hangs, jumps, faults);
        for n in &succ {
            if !graph.contains_key(n) {
                todo.push_back(*n);
            }
        }
        graph.insert(st, succ);
    }
    graph
}

/// The states from which some order reaches the end.
fn can_end(graph: &HashMap<State, Vec<State>>) -> HashSet<State> {
    let mut back: HashMap<State, Vec<State>> = HashMap::new();
    for (s, succ) in graph {
        for n in succ {
            back.entry(*n).or_default().push(*s);
        }
    }
    let mut seen: HashSet<State> = graph.keys().filter(|s| s.terminal()).copied().collect();
    let mut todo: VecDeque<State> = seen.iter().copied().collect();
    while let Some(s) = todo.pop_front() {
        for p in back.get(&s).into_iter().flatten() {
            if seen.insert(*p) {
                todo.push_back(*p);
            }
        }
    }
    seen
}

fn scripts(len: usize) -> Vec<Vec<Op>> {
    let ops = [Op::Hand, Op::Screen, Op::Flash];
    let mut out = vec![Vec::new()];
    for _ in 0..len {
        out = out
            .into_iter()
            .flat_map(|s| {
                ops.iter().map(move |op| {
                    let mut t = s.clone();
                    t.push(*op);
                    t
                })
            })
            .collect();
    }
    out
}

#[test]
fn every_order_keeps_the_rules_even_with_a_hang() {
    let mut total = 0;
    for script in scripts(3) {
        let mut faults = Faults::default();
        let graph = explore(&script, true, 3, &mut faults);
        assert!(
            faults.0.is_empty(),
            "{script:?}: {:#?}",
            &faults.0[..faults.0.len().min(3)]
        );
        total += graph.len();
    }
    println!("states checked: {total}");
    assert!(total > 10_000, "the model is too small: {total}");
}

#[test]
fn without_a_hang_every_order_can_finish_and_never_stops_on_its_own() {
    for script in scripts(3) {
        // Time jumps allowed: from every state the script can still end.
        let mut faults = Faults::default();
        let graph = explore(&script, false, 2, &mut faults);
        assert!(faults.0.is_empty(), "{script:?}: {:?}", faults.0.first());
        let ends = can_end(&graph);
        for s in graph.keys() {
            assert!(ends.contains(s), "{script:?}: stuck for good in {s:?}");
        }
        // No time jump (every wait well inside its limit): the first core
        // never stops, every end is the script done, and each flash use
        // was held.
        let graph = explore(&script, false, 0, &mut faults);
        assert!(faults.0.is_empty());
        let ends = can_end(&graph);
        for s in graph.keys() {
            assert_ne!(
                s.first,
                First::Stopped,
                "{script:?}: stopped without a hang"
            );
            assert!(ends.contains(s), "{script:?}: stuck for good in {s:?}");
            if s.terminal() {
                assert_eq!(s.first, First::Finished);
                if script.contains(&Op::Flash) {
                    assert!(s.held_once, "{script:?}: the flash was never held");
                }
                assert!(!s.shared.hold);
                // A last picture may still be on its way, but the second
                // core does not sleep through it.
                if s.second == Second::Sleep && !s.shared.ready {
                    assert_eq!(s.shared.job, EMPTY);
                }
            }
        }
    }
}

#[test]
fn a_hung_second_core_always_ends_in_a_stop_or_the_script_done() {
    for script in scripts(3) {
        let mut faults = Faults::default();
        let graph = explore(&script, true, 0, &mut faults);
        for s in graph.keys().filter(|s| s.hung && !s.terminal()) {
            // The first core alone, the time jumping past the limit each
            // time it would wait again.
            let mut st = *s;
            let mut began_after_hang = false;
            for _ in 0..200 {
                if st.terminal() {
                    break;
                }
                if st.first == First::Next && (st.pc as usize) < script.len() {
                    began_after_hang = true;
                }
                let n = step_first(&st, &script, &mut faults);
                check_rules(&n, &mut faults);
                st = if n == st {
                    State {
                        t: st.t + JUMP,
                        ..n
                    }
                } else {
                    n
                };
            }
            assert!(st.terminal(), "{script:?}: waits for good from {s:?}");
            // Every op begins with a join: one begun after the hang with
            // the box still full can only end in a stop.
            if began_after_hang && s.shared.job != EMPTY {
                assert_eq!(
                    st.first,
                    First::Stopped,
                    "{script:?}: went on past a hung job from {s:?}"
                );
            }
        }
        assert!(faults.0.is_empty(), "{script:?}: {:?}", faults.0.first());
    }
}

#[test]
fn a_park_that_comes_too_late_is_let_go_and_never_used() {
    // The second core is slow to look at the box: the first core gives up
    // the hold, then the second core parks late, sees `hold` off and
    // leaves. The flash is not used, the box ends empty.
    let script = [Op::Flash, Op::Hand];
    let mut faults = Faults::default();
    let graph = explore(&script, false, 1, &mut faults);
    assert!(faults.0.is_empty(), "{:?}", faults.0.first());
    let late = graph.keys().any(|s| {
        matches!(s.first, First::Parking(_)) && s.second == Second::ParkSpin && !s.shared.hold
    });
    assert!(late, "the model never reached a late park");
}

#[test]
fn the_waits_wrap_with_the_counter() {
    assert!(!over(u32::MAX - 5, 4, 10));
    assert!(over(u32::MAX - 5, 5, 10));
    assert!(!over(7, 7, 0));
    // The firmware's limits in CPU cycles (240 MHz) fit a u32 with room
    // (also checked when the crate compiles).
    assert!(over(0, JOIN_LIMIT_US * 240 + 1, JOIN_LIMIT_US * 240));
    assert!(!over(0, HOLD_WAIT_US * 240, JOIN_LIMIT_US * 240));
}

#[test]
fn an_empty_box_wins_over_the_time() {
    let cell = Cell::new(Shared::default());
    let v = View(&cell);
    let j = Join::new(0, 10);
    assert_eq!(j.step(&v, 1_000), Step::Done);
    cell.set(Shared {
        job: PRESENT,
        ..Shared::default()
    });
    assert_eq!(j.step(&v, 10), Step::Again);
    assert_eq!(j.step(&v, 11), Step::TooLong);
}

#[test]
fn a_job_is_never_put_into_a_full_box() {
    let cell = Cell::new(Shared {
        job: HOLD,
        ..Shared::default()
    });
    let v = View(&cell);
    assert!(!hand(&v, PRESENT));
    assert_eq!(cell.get().job, HOLD);
    cell.set(Shared::default());
    assert!(hand(&v, PRESENT));
    assert_eq!(cell.get().job, PRESENT);
}

#[test]
fn a_finished_park_answers_the_same_and_touches_nothing() {
    let cell = Cell::new(Shared::default());
    let v = View(&cell);
    let mut p = Park::new(0, JL, HL);
    assert_eq!(p.step(&v, 0), ParkStep::Again); // joined
    assert_eq!(p.step(&v, 0), ParkStep::Again); // hold on
    assert_eq!(p.step(&v, 0), ParkStep::Wake); // job in
    assert_eq!(cell.get().job, HOLD);
    assert!(cell.get().hold);
    cell.set(Shared {
        held: true,
        ..cell.get()
    });
    assert_eq!(p.step(&v, 1), ParkStep::Held);
    let before = cell.get();
    assert_eq!(p.step(&v, 99), ParkStep::Held);
    assert_eq!(cell.get(), before);
}

#[test]
fn after_the_hang_test_every_later_use_stops_and_touches_nothing() {
    // The measuring letter `H` parks the second core for good. Whatever
    // the first core does next (a picture, the screen, the flash), in any
    // order of the two cores and with late timers, it must never touch
    // the screen or the flash again, and it must end in a stop at a limit
    // (never wait for good, never go on as if nothing happened).
    let mut total = 0;
    for later in scripts(2) {
        let mut script = vec![Op::Hand, Op::HangTest];
        script.extend(later);
        let mut faults = Faults::default();
        let graph = explore(&script, false, 2, &mut faults);
        assert!(faults.0.is_empty(), "{script:?}: {:?}", faults.0.first());
        let touches = |s: &State| {
            s.pc >= 2
                && matches!(
                    s.first,
                    First::HandTouch | First::ScreenTouch | First::Flash
                )
        };
        for s in graph.keys() {
            assert!(
                !touches(s),
                "{script:?}: used the screen or the flash after the hang test: {s:?}"
            );
            if s.pc < 2 || s.terminal() {
                assert!(
                    !s.terminal() || s.first == First::Stopped,
                    "{script:?}: went on: {s:?}"
                );
                continue;
            }
            // The first core alone from here (the second core is parked
            // for good), the time jumping past the limit whenever it waits.
            let mut st = *s;
            for _ in 0..200 {
                if st.terminal() {
                    break;
                }
                let mut n = step_first(&st, &script, &mut faults);
                if let Some(m) = step_second(&n) {
                    // The parked core spins; nothing else may change.
                    assert!(
                        matches!(
                            m.second,
                            Second::ParkSpin
                                | Second::ParkSet
                                | Second::Check
                                | Second::Poll
                                | Second::Sleep
                        ),
                        "{m:?}"
                    );
                    n = m;
                }
                check_rules(&n, &mut faults);
                assert!(
                    !touches(&n),
                    "{script:?}: used the screen or the flash after the hang test: {n:?}"
                );
                st = if n.first == st.first && n.pc == st.pc {
                    State {
                        t: st.t + JUMP,
                        ..n
                    }
                } else {
                    n
                };
            }
            assert_eq!(
                st.first,
                First::Stopped,
                "{script:?}: did not stop from {s:?}"
            );
        }
        assert!(faults.0.is_empty(), "{script:?}: {:?}", faults.0.first());
        total += graph.len();
    }
    assert!(total > 1_000, "the model is too small: {total}");
}
