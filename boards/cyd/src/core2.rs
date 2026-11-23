//! Feature `core2` (on by default since the two-cores round; the one-core
//! build stays supported): the chip's second core sends the console's
//! pictures to the panel.
//!
//! Ported from experiments/cyd-lab (`core2-swap`, its README "The swap
//! on the board"). The console (console-core with `swap`) draws on the
//! first core. Each finished picture is handed over through
//! [`SwapSender`]: the second core sends it with the usual tear-free
//! timing (`CydScreen::present`), while the first core goes on with the
//! next step in the console's other picture. There is no copy of the
//! picture.
//!
//! What runs where:
//!
//! | first core (PRO) | second core (APP) |
//! |---|---|
//! | the console's loop (one task, `app::run`), the radio (the controller's task, pinned to this core, and its interrupts; the PHY code in RTC fast memory, which only this core can run), the debug link, the log, settings saves, the game loader and the games | one esp-rtos thread: an embassy executor with one task, [`worker`], which sends the handed pictures |
//! | panel set-up, clock check, clear, survey and calibration (in `main`, before the second core starts) | every timed send from the first picture on: the anchor reads (Get Scanline) and the windows |
//! | link shots, raw panel commands, the backlight, the letter `l`: only after [`join`] | nothing else: it never logs, never uses flash on purpose, never runs game code |
//!
//! The code the second core runs: its executor, [`worker`]'s task and
//! the screen's `present` from flash (through its own cache), all in the
//! hot block (hot-block.list, see src/screen.rs), the SPI driver, the
//! scanline read, `fill_fast` and [`parked`] from RAM (IRAM, which both
//! cores can run). None of it is in RTC fast memory.
//!
//! # Who may touch which memory, and when
//!
//! The rule is a hand-over flag, [`JOB`]: 0 = the second core holds
//! nothing. The first core writes what the second core will use, then
//! stores the job number with Release; the second core loads it with
//! Acquire, does the job, and stores 0 with Release after its last use;
//! the first core sees 0 with an Acquire load ([`join`]) before it touches
//! any of it again. Only the first core ever sets a job, and only while
//! `JOB` is 0 (`hand_over` checks it and stops the console if not).
//! Nothing in the hardware enforces this: the code does. The first core's
//! side (the bounded join, the hand-over, parking and letting go) is the
//! crate boards/cyd/handover, whose PC tests run it against a model of
//! [`worker`] and [`parked`] in every order the two cores can take.
//!
//! | memory | first core | second core | enforced by | if broken |
//! |---|---|---|---|---|
//! | the handed picture (19,200 B, one of the two in the `Console` static) | only reads it (catch-up copy in `present_handing`) until its next `wait_free`; writes it again only after that | reads it while `JOB` = present, any part of it (a box send also reads pixels outside the changed areas, see below) | console-video's `present_handing` order (find, wait_free, hand_over, catch up, swap); `Video::frame_mut` and `game_buffers` never give the held picture | the panel shows a mix of two pictures for one frame (letter `V` checks for it) |
//! | the drawn picture, the console's palette, the rest of the console | reads and writes | never | the second core gets only a pointer to the handed picture; the colours go over as the screen's wire table | - |
//! | `SLOT` (picture pointer, areas, sum) | writes while `JOB` = 0 | reads while `JOB` = present | `JOB` | wrong areas sent, or a send from a stale pointer |
//! | the screen (`CydScreen`: SPI, DMA buffers, wire table, beam, counters, backlight) | only through [`Core2Screen`] (its `Deref` calls [`join`] first), and in `hand_over` right after `wait_free` | only while `JOB` = present | `JOB`; `Core2Screen`'s `Deref` | two cores on one SPI bus: garbage on the panel, a stuck transfer; a wrong anchor: torn windows |
//! | `SNAP` (a copy of the screen's counters) | reads it in a critical section | writes it in a critical section after each send | the critical section (a spin lock across both cores) | a torn counter in a report |
//! | the counters below (atomics) | reads, resets (`r`) | adds | atomics (Relaxed: counts only) | a count off by one in a report |
//! | `HANDED_US` (the hand-over's time, for the wake lag) | stores it (Relaxed) before the job's Release store | loads it (Relaxed) after the job's Acquire load, so it sees this job's stamp | `JOB` (the stamp is written before the job, in `Real::set_job`) | a wrong `wake_us` (it was so before this round: up to one frame) |
//! | `RESEND` (a send failed: send everything next time) | takes it before each step (Relaxed) | sets it (Relaxed) | a hint only: seen one step later at worst | one more picture with a stale area |
//! | `VERIFY` (letter `V`) | toggles it after [`join`] | reads it during a job | `JOB` (set before the next job's Release store) | one picture checked or not |
//! | `HOLD`, `HELD` | sets and clears `HOLD` ([`Held`]) | sets and clears `HELD` in [`parked`] | Release/Acquire | the flash used while the second core still runs from it |
//! | the flash and its cache (settings saves) | only while a [`Held`] lives: the second core waits in RAM with its interrupts off | runs no flash code while held | [`Held`] in every `EspFlash` call; esp-storage's `multicore_auto_park` stalls it as well | a cache fetch during a write or erase: a crash or a wrong instruction |
//! | the game slots (code in IRAM, data) | the game loader writes, the game runs | never | the second core runs no game code and gets no pointer into a slot | - |
//!
//! The orderings, one by one:
//!
//! - `JOB`: Release stores and Acquire loads on both cores: the hand-over
//!   itself (above).
//! - `HOLD`, `HELD`: Release stores, Acquire loads. `HOLD` is set before
//!   the hold job's Release store, so the second core sees it on when it
//!   sees the job; it sets `HELD` off before it empties the box, so a
//!   later park never sees an old `HELD`.
//! - `SCREEN`: stored (Release) in [`start`] before the second core is
//!   started, loaded with Acquire; starting the core orders it as well.
//! - `STARTED`, `STACK_LOW`: the first core only.
//! - `WAKER` (esp-hal's `AtomicWaker`): `register` and `wake` both take a
//!   lock across the cores (esp-sync). The worker registers, then loads
//!   `JOB`; the first core stores `JOB`, then wakes. Whichever takes the
//!   lock second sees the other's work: the wake finds the waker, or the
//!   load after the register sees the job. No lost wake-up (the PC tests
//!   check the same order).
//! - Everything else is a count, a measuring switch or a hint (table
//!   above): Relaxed.
//!
//! The pictures, by the Rust rules: the second core reads the handed
//! picture through a raw pointer taken from the console's `&Frame`; the
//! first core keeps `&mut` to both pictures (console-video, which has no
//! unsafe code), but neither writes the held one nor hands out a
//! reference to it until the box is empty again. A game gets the drawn
//! picture for one call only (game-kit makes a slice that lives for the
//! call; games have no unsafe code). A game that kept the pointer anyway
//! would write the picture being sent: one frame with a mix of two
//! pictures (letter `V` finds it), nothing worse for the console.
//!
//! The send plan (screen.rs, send-plan's `choose`) may send ONE box around
//! all the changed areas; inside the box it also sends pixels that did
//! not change. Here too that reads only the handed picture: the worker
//! makes its slice from `SLOT.pixels` (the whole 19,200 bytes of the
//! picture `hand_over` got) and `present` reads nothing else for pixels
//! (the fast path, `fill_fast`, inside 160 x 120). The first core does
//! not write any byte of that picture until its next `wait_free` has
//! seen `JOB` = 0 (console-video's `present_handing`: steps 4 and 5 write
//! only the other picture; `frame_mut` and `game_buffers` give only the
//! other one), so no byte of the box changes while it is sent. Outside
//! the changed areas the handed picture equals the one handed before
//! (console-video's catch-up, step 4), which is what the panel shows: the
//! unchanged pixels are sent with the values the panel already has.
//!
//! The beam anchor is a cycle count, and each core has its own cycle
//! counter: an anchor is only good on the core that read it. The second
//! core reads every anchor of a timed send; the survey's anchor (first
//! core) is forgotten in [`start`], and the first core forgets it again
//! after any use of the panel (shots, raw commands, the inline fallback).
//! The line length (a rate) is the same on both cores (both at 240 MHz).
//!
//! # If the second core hangs or panics
//!
//! Every wait of the first core for the second is bounded
//! ([`handover::JOIN_LIMIT_US`], 500 ms; the longest send measured on the
//! board is about 33 ms). Past it the first core panics with a message
//! that names the job (esp-backtrace prints it once on the serial port;
//! the console stops as for a game panic, RST restarts). It touches
//! neither the screen nor the flash on that way, so whatever the second
//! core still does, the two never meet on the SPI bus, and the panel
//! keeps the last picture it got. Sending on the first core instead is not
//! done: a hung send may hold the SPI driver half way (`Lcd.spi` taken,
//! a DMA transfer running), and nothing proves that state could be
//! taken over safely.
//!
//! A panic on the second core is the same case: esp-backtrace prints it
//! and stops only that core (interrupts off, a loop), the job stays in
//! the box, and the first core stops at its next wait with its own
//! message. The second core panics outside any lock: the only critical
//! section it takes (`SNAP`) holds one plain copy.

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{
    AtomicBool, AtomicPtr, AtomicU32, Ordering::Acquire, Ordering::Relaxed, Ordering::Release,
};

use board_api::{PixelSource, Rect, Screen, SendReport};
use console_video::{Handed, Sender};
use core::future::poll_fn;
use core::task::Poll;
use esp_hal::asynch::AtomicWaker;
use esp_hal::peripherals::{CPU_CTRL, FROM_CPU_INTR1};
use esp_hal::system::Stack;
use esp_rtos::embassy::{Callbacks, Executor};
use handover::{Join, Park, ParkStep, Step, Unpark, Words};
use log::info;
use static_cell::{ConstStaticCell, StaticCell};

use crate::lcd::{CPU_MHZ, H, W, cycles};
use crate::screen::{Counters, CydScreen, MAX_JOBS};

/// The second core's stack (esp-rtos's own example uses 8 KB; the lab
/// saw at most 1,792 bytes used). It is in `.bss`, so it comes out of
/// the main stack's room.
pub const STACK_SIZE: usize = 8192;
static STACK: ConstStaticCell<Stack<STACK_SIZE>> = ConstStaticCell::new(Stack::new());
/// Where the stack lies, for measuring how deep it was used.
static STACK_LOW: AtomicU32 = AtomicU32::new(0);
const STACK_PATTERN: u32 = 0x5AC3_A53C;
/// Bytes at the stack's bottom not counted: esp-hal's guard word (60
/// bytes up) and, right above it, the start function esp-hal parks there
/// for the second core.
const GUARD_SKIP: usize = 256;

/// The hand-over flag (the job box): [`handover::EMPTY`] = the second
/// core holds nothing, [`handover::PRESENT`], [`handover::HOLD`].
static JOB: AtomicU32 = AtomicU32::new(handover::EMPTY);
/// [`Held`]: the first core wants the second parked (`HOLD`), and the
/// second says it is (`HELD`).
static HOLD: AtomicBool = AtomicBool::new(false);
static HELD: AtomicBool = AtomicBool::new(false);

/// Whether the second core runs.
static STARTED: AtomicBool = AtomicBool::new(false);
/// The screen, owned here from the start of the second core on.
static SCREEN_CELL: StaticCell<CydScreen> = StaticCell::new();
static SCREEN: AtomicPtr<CydScreen> = AtomicPtr::new(core::ptr::null_mut());

static WAKER: AtomicWaker = AtomicWaker::new();
static EXECUTOR: StaticCell<Executor> = StaticCell::new();

struct Shared<T>(UnsafeCell<T>);
// SAFETY: every use follows the rule in the file notes: `SLOT` only by
// the core `JOB` gives it to, `SNAP` only inside a critical section.
unsafe impl<T> Sync for Shared<T> {}

/// What the second core needs for one send: where the picture is and
/// which areas (16 areas = 128 bytes, plus the pointer).
struct Slot {
    pixels: *const u8,
    areas: [Rect; MAX_JOBS],
    n: usize,
    /// Letter `V`: the picture's sum when handed over.
    sum: u32,
}
static SLOT: Shared<Slot> = Shared(UnsafeCell::new(Slot {
    pixels: core::ptr::null(),
    areas: [Rect::EMPTY; MAX_JOBS],
    n: 0,
    sum: 0,
}));

/// The screen's counters as the second core last left them.
#[derive(Clone, Copy, Default)]
pub struct Snapshot {
    pub counters: Counters,
    pub misses: [u32; 4],
    #[cfg_attr(not(feature = "link"), allow(dead_code))]
    pub calibrated: bool,
    /// The send plan in use (today's only with `send-ab`, letter `o`) and
    /// whether the tear-free timing is on (letter `u`), for the `send`
    /// lines.
    pub old_plan: bool,
    pub timed: bool,
}

impl Snapshot {
    fn of(screen: &CydScreen) -> Snapshot {
        Snapshot {
            counters: screen.counters,
            misses: screen.beam.misses,
            calibrated: screen.beam.calibrated,
            old_plan: screen.old_plan(),
            timed: screen.timed,
        }
    }
}
static SNAP: Shared<Option<Snapshot>> = Shared(UnsafeCell::new(None));

// Counters for the report (letter `m`, every 30 s in the log).
static JOBS_DONE: AtomicU32 = AtomicU32::new(0);
static BUSY_US: AtomicU32 = AtomicU32::new(0);
static HANDS: AtomicU32 = AtomicU32::new(0);
static INLINE: AtomicU32 = AtomicU32::new(0);
static HAND_CYC_MAX: AtomicU32 = AtomicU32::new(0);
static FREE_CALLS: AtomicU32 = AtomicU32::new(0);
static WAITS: AtomicU32 = AtomicU32::new(0);
static WAIT_US_SUM: AtomicU32 = AtomicU32::new(0);
static WAIT_US_MAX: AtomicU32 = AtomicU32::new(0);
static AFTER_N: AtomicU32 = AtomicU32::new(0);
static AFTER_US_SUM: AtomicU32 = AtomicU32::new(0);
static AFTER_US_MAX: AtomicU32 = AtomicU32::new(0);
static POLLS: AtomicU32 = AtomicU32::new(0);
static IDLES: AtomicU32 = AtomicU32::new(0);
static WAKE_US_SUM: AtomicU32 = AtomicU32::new(0);
static WAKE_US_MAX: AtomicU32 = AtomicU32::new(0);
static HANDED_US: AtomicU32 = AtomicU32::new(0);
static HOLDS: AtomicU32 = AtomicU32::new(0);
static HOLD_FAILS: AtomicU32 = AtomicU32::new(0);
static HOLD_US_MAX: AtomicU32 = AtomicU32::new(0);
static VERIFY: AtomicBool = AtomicBool::new(false);
static CHECKED: AtomicU32 = AtomicU32::new(0);
static MISMATCH: AtomicU32 = AtomicU32::new(0);
/// Sends that failed on the second core (each makes the next one full).
static RESENDS: AtomicU32 = AtomicU32::new(0);
/// A send failed: the panel may be out of step with the picture, so the
/// console sends the whole screen at its next present ([`take_resend`]).
static RESEND: AtomicBool = AtomicBool::new(false);

/// The bounded waits ([`handover`]), in CPU cycles.
const JOIN_LIMIT: u32 = handover::JOIN_LIMIT_US * CPU_MHZ;
#[cfg_attr(not(feature = "store"), allow(dead_code))]
const HOLD_LIMIT: u32 = handover::HOLD_WAIT_US * CPU_MHZ;

/// The shared words as the first core uses them: Acquire loads, Release
/// stores. A job's time stamp (`HANDED_US`) is written before the job
/// itself, so the second core, which loads the job with Acquire, reads
/// this job's stamp.
struct Real;

impl Words for Real {
    fn job(&self) -> u32 {
        JOB.load(Acquire)
    }
    fn set_job(&self, job: u32) {
        if job != handover::EMPTY {
            HANDED_US.store(now_us32(), Relaxed);
        }
        JOB.store(job, Release);
    }
    fn set_hold(&self, on: bool) {
        HOLD.store(on, Release);
    }
    fn held(&self) -> bool {
        HELD.load(Acquire)
    }
}

/// A wait for the second core passed its limit: it hangs or panicked.
/// Stops the console with a message (see the file notes).
#[cold]
#[inline(never)]
fn stopped(during: &str) -> ! {
    panic!(
        "core2: the second core still holds job {} after {} ms ({during}); console stopped, press RST",
        JOB.load(Relaxed),
        handover::JOIN_LIMIT_US / 1000,
    )
}

fn now_us32() -> u32 {
    embassy_time::Instant::now().as_micros() as u32
}

/// Starts the second core, which owns the screen from now on. Call once,
/// after the panel's survey and calibration (they run here, on the first
/// core) and after the radio started (so its task and interrupts are set
/// up on the first core). Returns the board's screen part and the start's
/// time in microseconds.
pub fn start(
    cpu: CPU_CTRL<'static>,
    int1: FROM_CPU_INTR1<'static>,
    mut screen: CydScreen,
) -> (Core2Screen, u32) {
    // The survey's anchor (if any) is a count of this core's clock.
    screen.beam.forget();
    let snap = Snapshot::of(&screen);
    // SAFETY: the second core does not run yet.
    unsafe { *SNAP.0.get() = Some(snap) };
    let screen = SCREEN_CELL.init(screen);
    SCREEN.store(screen as *mut CydScreen, Release);

    let stack = STACK.take();
    // Fill the stack with a pattern, to see later how deep it was used.
    let low = stack.bottom() as usize;
    STACK_LOW.store(low as u32, Relaxed);
    let mut at = low;
    while at < low + STACK_SIZE {
        // SAFETY: inside the stack, not in use yet.
        unsafe { (at as *mut u32).write_volatile(STACK_PATTERN) };
        at += 4;
    }
    let t = cycles();
    esp_rtos::start_second_core(cpu, int1, stack, second_main);
    let us = cycles().wrapping_sub(t) / CPU_MHZ;
    STARTED.store(true, Release);
    (Core2Screen(()), us)
}

/// The second core's start: an executor with one task.
fn second_main() {
    let ex = EXECUTOR.init(Executor::new());
    ex.run_with_callbacks(|sp| sp.spawn(worker().unwrap()), Idle)
}

struct Idle;
impl Callbacks for Idle {
    fn before_poll(&mut self) {}
    fn on_idle(&mut self) {
        // Called each time the executor is about to sleep (esp-rtos then
        // runs its idle hook: `waiti`, the core stops until an interrupt).
        IDLES.fetch_add(1, Relaxed);
    }
}

/// The second core's one task: wait for a job, do it, empty the box.
#[embassy_executor::task]
async fn worker() -> ! {
    loop {
        let job = poll_fn(|cx| {
            POLLS.fetch_add(1, Relaxed);
            WAKER.register(cx.waker());
            match JOB.load(Acquire) {
                0 => Poll::Pending,
                j => Poll::Ready(j),
            }
        })
        .await;
        // This job's stamp: written before the job (`Real::set_job`).
        let lag = now_us32().wrapping_sub(HANDED_US.load(Relaxed));
        WAKE_US_SUM.fetch_add(lag, Relaxed);
        WAKE_US_MAX.fetch_max(lag, Relaxed);
        let t = cycles();
        if job == handover::HOLD {
            parked();
        } else if job == handover::PRESENT {
            // SAFETY: the box holds a job: the first core does not touch
            // the slot or the screen, nor write the handed picture, until
            // the box is empty again (file notes).
            unsafe {
                let slot = &*SLOT.0.get();
                let pixels = core::slice::from_raw_parts(slot.pixels, (W / 2) * (H / 2));
                let verify = VERIFY.load(Relaxed);
                let before = if verify { sum(pixels) } else { 0 };
                let screen = &mut *SCREEN.load(Acquire);
                let errors = screen.counters.errors;
                screen.present_held(pixels, &slot.areas[..slot.n]);
                if screen.counters.errors != errors {
                    // A window did not go out whole: the panel may differ
                    // from the picture outside the next changes.
                    RESEND.store(true, Relaxed);
                    RESENDS.fetch_add(1, Relaxed);
                }
                if verify {
                    let after = sum(pixels);
                    CHECKED.fetch_add(1, Relaxed);
                    if before != slot.sum || after != slot.sum {
                        MISMATCH.fetch_add(1, Relaxed);
                    }
                }
                let snap = Snapshot::of(screen);
                critical_section::with(|_| *SNAP.0.get() = Some(snap));
            }
            BUSY_US.fetch_add(cycles().wrapping_sub(t) / CPU_MHZ, Relaxed);
        }
        JOBS_DONE.fetch_add(1, Relaxed);
        // After the last use of the slot, the picture and the screen.
        JOB.store(handover::EMPTY, Release);
    }
}

/// [`Held`] on the second core: waits in RAM, with every interrupt below
/// NMI off, until the first core is done with the flash. Touches nothing
/// but `HOLD` and `HELD` (both in data RAM) and runs from RAM, so the
/// flash and its cache are not used by this core meanwhile.
#[esp_hal::ram]
fn parked() {
    let ps: u32;
    // SAFETY: raises the interrupt level (as esp-sync's lock does) and
    // gives the old one back below.
    unsafe { core::arch::asm!("rsil {0}, 5", out(reg) ps) };
    HELD.store(true, Release);
    while HOLD.load(Acquire) {
        core::hint::spin_loop();
    }
    HELD.store(false, Release);
    // SAFETY: the level saved above.
    unsafe { core::arch::asm!("wsr.ps {0}", "rsync", in(reg) ps) };
}

/// Waits until the second core holds no job. Returns the wait in CPU
/// cycles. After 500 ms the console stops with a message ([`stopped`]).
pub fn join() -> u32 {
    let t = cycles();
    let j = Join::new(t, JOIN_LIMIT);
    loop {
        match j.step(&Real, cycles()) {
            Step::Done => return cycles().wrapping_sub(t),
            Step::Again => core::hint::spin_loop(),
            Step::TooLong => stopped("join"),
        }
    }
}

/// While one of these lives, the second core waits in RAM with its
/// interrupts off ([`parked`]) and touches neither the flash, nor its
/// cache, nor the screen: the flash may be read, written or erased
/// (src/flash_store.rs makes one around each flash call). Dropping it
/// lets the second core go and waits until it is back to idle.
#[cfg_attr(not(feature = "store"), allow(dead_code))]
pub struct Held {
    /// Whether the second core was parked (false before it started).
    parked: bool,
}

#[cfg_attr(not(feature = "store"), allow(dead_code))]
impl Held {
    /// Waits for the send in flight (at most one picture, about 35 ms),
    /// then parks the second core ([`handover::Park`]). `None` if it did
    /// not park within 100 ms: then nothing is held, and the flash must
    /// not be used. Only the first core calls this (the second core never
    /// uses flash). A wait past 500 ms stops the console ([`stopped`]).
    pub fn park() -> Option<Held> {
        if !STARTED.load(Acquire) {
            // Before the second core runs (the store's start): nothing to park.
            return Some(Held { parked: false });
        }
        let mut p = Park::new(cycles(), JOIN_LIMIT, HOLD_LIMIT);
        let mut asked = cycles();
        loop {
            match p.step(&Real, cycles()) {
                ParkStep::Again => core::hint::spin_loop(),
                ParkStep::Wake => {
                    asked = cycles();
                    WAKER.wake();
                }
                ParkStep::Held => {
                    HOLD_US_MAX.fetch_max(cycles().wrapping_sub(asked) / CPU_MHZ, Relaxed);
                    HOLDS.fetch_add(1, Relaxed);
                    return Some(Held { parked: true });
                }
                ParkStep::NotHeld => {
                    HOLD_FAILS.fetch_add(1, Relaxed);
                    return None;
                }
                ParkStep::TooLong => stopped("park"),
            }
        }
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        if self.parked {
            // `hold` off, then wait until it left `parked` and emptied
            // the box ([`handover::Unpark`]).
            let mut u = Unpark::new(JOIN_LIMIT);
            loop {
                match u.step(&Real, cycles()) {
                    Step::Again => core::hint::spin_loop(),
                    Step::Done => return,
                    Step::TooLong => stopped("unpark"),
                }
            }
        }
    }
}

/// The screen's counters as the second core last left them (a picture
/// old at most).
pub fn snapshot() -> Snapshot {
    // SAFETY: the copy is written only inside a critical section.
    critical_section::with(|_| unsafe { *SNAP.0.get() }).unwrap_or_default()
}

/// The screen part of the board with `core2`: the screen itself belongs
/// to the second core. Every use from the first core goes through
/// `Deref`/`DerefMut`, which wait for the second core first ([`join`]).
/// While the first core holds the reference it gets, the second core has
/// nothing to do: only the first core hands it work, through
/// [`SwapSender`], and not while it borrows the board.
pub struct Core2Screen(());

impl Deref for Core2Screen {
    type Target = CydScreen;
    fn deref(&self) -> &CydScreen {
        join();
        // SAFETY: set in `start` (a `Core2Screen` exists only after it),
        // never moved; the second core holds no job (joined above).
        unsafe { &*SCREEN.load(Acquire) }
    }
}

impl DerefMut for Core2Screen {
    fn deref_mut(&mut self) -> &mut CydScreen {
        join();
        // SAFETY: as in `deref`; `&mut self` makes this the only
        // reference from the first core.
        unsafe { &mut *SCREEN.load(Acquire) }
    }
}

impl Screen for Core2Screen {
    const PANEL_W: u16 = W as u16;
    const PANEL_H: u16 = H as u16;

    /// Not used with `core2` (the console hands its pictures to
    /// [`SwapSender`]); if it ever is, the first core sends it itself and
    /// it counts as `inline`.
    fn present<S: PixelSource + ?Sized>(&mut self, src: &S, areas: &[Rect]) -> SendReport {
        INLINE.fetch_add(1, Relaxed);
        let screen: &mut CydScreen = self;
        screen.beam.forget();
        let r = screen.present(src, areas);
        screen.beam.forget();
        r
    }

    fn backlight(&mut self, level: u8) {
        // Only after the second core is idle (the console also waits for
        // its sender first, `wait_free`).
        let screen: &mut CydScreen = self;
        screen.backlight.set(level);
    }
}

fn sum(p: &[u8]) -> u32 {
    // A position-weighted sum, so a moved byte counts too.
    let mut s = 0u32;
    for (i, &b) in p.iter().enumerate() {
        s = s.wrapping_add(u32::from(b).wrapping_mul(i as u32 | 1));
    }
    s
}

/// Letter `V`: the picture check on or off (measuring only: a sum of the
/// handed picture in `hand_over`, and before and after each send).
pub fn toggle_verify() -> bool {
    join();
    let on = !VERIFY.load(Relaxed);
    VERIFY.store(on, Relaxed);
    on
}

/// The console's way out to the second core. Made for each step.
#[derive(Default)]
pub struct SwapSender {
    /// The cycle counter when `hand_over` gave a picture to the second
    /// core (this step), for the catch-up time.
    pub handed_at: Option<u32>,
    /// The last `wait_free`'s wait, in microseconds.
    waited_us: u32,
}

impl Sender for SwapSender {
    fn wait_free(&mut self) {
        FREE_CALLS.fetch_add(1, Relaxed);
        let busy = JOB.load(Acquire) != 0;
        let waited = join() / CPU_MHZ;
        self.waited_us = waited;
        if busy {
            WAITS.fetch_add(1, Relaxed);
            WAIT_US_SUM.fetch_add(waited, Relaxed);
            WAIT_US_MAX.fetch_max(waited, Relaxed);
        }
    }

    /// The report: areas and pixels of this picture; `wait_us` the wait
    /// for the second core before it (`wait_free`), `send_us` the
    /// hand-over itself. The second core's send times are in the screen's
    /// counters ([`snapshot`]).
    fn hand_over(&mut self, h: &Handed<'_>) -> SendReport {
        // Only called right after `wait_free`: the box is empty. Checked,
        // as the rest of this function relies on it.
        if Real.job() != handover::EMPTY {
            broken();
        }
        let t0 = cycles();
        let ptr = SCREEN.load(Acquire);
        // SAFETY: the second core holds no job, and the first core holds
        // no other reference to the screen during the console's step.
        let screen = unsafe { &mut *ptr };
        let plain = h.view.scale == 2 && h.view.left == 0 && h.view.top == 0;
        if !plain {
            // Never with this panel (the picture always fills it at 2x);
            // sent here, on the first core, if it ever happens.
            INLINE.fetch_add(1, Relaxed);
            screen.beam.forget();
            let r = screen.present(&h.source(), h.areas);
            screen.beam.forget();
            return r;
        }
        screen.set_wire(h.palette.colours(), h.palette.version());
        // SAFETY: the box is empty (see above).
        let slot = unsafe { &mut *SLOT.0.get() };
        slot.pixels = h.frame.0.as_ptr();
        let mut report = SendReport::default();
        if h.areas.len() > MAX_JOBS {
            slot.areas[0] = Rect::new(0, 0, W as u16, H as u16);
            slot.n = 1;
        } else {
            slot.areas[..h.areas.len()].copy_from_slice(h.areas);
            slot.n = h.areas.len();
        }
        for r in &slot.areas[..slot.n] {
            report.areas += 1;
            report.pixels += u32::from(r.w) * u32::from(r.h);
        }
        if VERIFY.load(Relaxed) {
            slot.sum = sum(&h.frame.0);
        }
        if !handover::hand(&Real, handover::PRESENT) {
            broken();
        }
        WAKER.wake();
        let t1 = cycles();
        report.wait_us = self.waited_us;
        report.send_us = t1.wrapping_sub(t0) / CPU_MHZ;
        HANDS.fetch_add(1, Relaxed);
        HAND_CYC_MAX.fetch_max(t1.wrapping_sub(t0), Relaxed);
        self.handed_at = Some(t1);
        report
    }
}

/// `hand_over` found the box full: the rule "wait_free first" was
/// broken (never seen; a bug in console-video or here).
#[cold]
#[inline(never)]
fn broken() -> ! {
    panic!(
        "core2: a picture handed over while the second core holds job {}",
        JOB.load(Relaxed)
    )
}

/// Measuring only (`hang-test`, letter `H`): parks the second core for
/// good, as a hang would (`hold` is never let go). The first core's next
/// wait for it passes [`JOIN_LIMIT`] and stops the console with the
/// message of [`stopped`]: the watchdog, seen on the board.
#[cfg(feature = "hang-test")]
pub fn hang_test() {
    join();
    Real.set_hold(true);
    if !handover::hand(&Real, handover::HOLD) {
        broken();
    }
    WAKER.wake();
}

/// Whether a send failed on the second core since the last call: then
/// the console should send the whole screen at its next present
/// (`Video::force_full`). One-core builds have no such step.
pub fn take_resend() -> bool {
    RESEND.swap(false, Relaxed)
}

/// The time from `hand_over`'s return to the end of the console's step
/// (the catch-up copy, the swap and the rest of the step).
pub fn note_after(cycles_after: u32) {
    let us = cycles_after / CPU_MHZ;
    AFTER_N.fetch_add(1, Relaxed);
    AFTER_US_SUM.fetch_add(us, Relaxed);
    AFTER_US_MAX.fetch_max(us, Relaxed);
}

/// Letter `r`: the screen's counters and these to zero.
pub fn reset(screen: &mut Core2Screen) {
    let s: &mut CydScreen = screen;
    s.counters = Counters::default();
    sync(screen);
    for c in [
        &JOBS_DONE,
        &BUSY_US,
        &HANDS,
        &INLINE,
        &HAND_CYC_MAX,
        &FREE_CALLS,
        &WAITS,
        &WAIT_US_SUM,
        &WAIT_US_MAX,
        &AFTER_N,
        &AFTER_US_SUM,
        &AFTER_US_MAX,
        &POLLS,
        &IDLES,
        &WAKE_US_SUM,
        &WAKE_US_MAX,
        &HOLDS,
        &HOLD_FAILS,
        &HOLD_US_MAX,
        &CHECKED,
        &MISMATCH,
        &RESENDS,
    ] {
        c.store(0, Relaxed);
    }
}

/// The snapshot made again from the screen now (after the first core
/// changed it: letter `r`, and `o`, `p`, `u` of `send-ab`), so the
/// reports show it before the next picture is sent.
pub fn sync(screen: &Core2Screen) {
    // `Deref` waits for the second core first ([`join`]).
    let snap = Snapshot::of(screen);
    critical_section::with(|_| {
        // SAFETY: inside the critical section.
        unsafe { *SNAP.0.get() = Some(snap) }
    });
}

/// How deep the second core's stack was ever used, in bytes.
pub fn stack_deepest() -> usize {
    let low = STACK_LOW.load(Relaxed) as usize;
    if low == 0 {
        return 0;
    }
    let mut at = low + GUARD_SKIP;
    while at < low + STACK_SIZE {
        // SAFETY: inside the second core's stack, only read.
        if unsafe { (at as *const u32).read_volatile() } != STACK_PATTERN {
            break;
        }
        at += 4;
    }
    low + STACK_SIZE - at
}

/// The `core2` lines for the log (two: the log cuts lines at 320 bytes).
pub fn report() {
    let s = snapshot();
    let c = &s.counters;
    let waits = WAITS.load(Relaxed);
    let jobs = JOBS_DONE.load(Relaxed);
    info!(
        "core2 hands={} inline={} hand_us_max={} free_calls={} waits={waits} wait_us_avg={} wait_us_max={} after_us_avg={} after_us_max={} send_us_avg={} send_us_max={}",
        HANDS.load(Relaxed),
        INLINE.load(Relaxed),
        HAND_CYC_MAX.load(Relaxed) / CPU_MHZ,
        FREE_CALLS.load(Relaxed),
        WAIT_US_SUM.load(Relaxed) / waits.max(1),
        WAIT_US_MAX.load(Relaxed),
        AFTER_US_SUM.load(Relaxed) / AFTER_N.load(Relaxed).max(1),
        AFTER_US_MAX.load(Relaxed),
        c.total_us / u64::from(c.presents.max(1)),
        c.total_max,
    );
    info!(
        "core2 jobs={jobs} busy_ms={} polls={} idles={} wake_us_avg={} wake_us_max={} holds={} hold_fails={} hold_us_max={} resends={} verify={} checked={} mismatch={} stack_deepest={} stack_size={STACK_SIZE}",
        BUSY_US.load(Relaxed) / 1000,
        POLLS.load(Relaxed),
        IDLES.load(Relaxed),
        WAKE_US_SUM.load(Relaxed) / jobs.max(1),
        WAKE_US_MAX.load(Relaxed),
        HOLDS.load(Relaxed),
        HOLD_FAILS.load(Relaxed),
        HOLD_US_MAX.load(Relaxed),
        RESENDS.load(Relaxed),
        VERIFY.load(Relaxed),
        CHECKED.load(Relaxed),
        MISMATCH.load(Relaxed),
        stack_deepest(),
    );
}

/// Measuring only (`xip-probe`): whether the second core holds a job right
/// now. Only reads the box; the probe never gives the second core work.
#[cfg(feature = "xip-probe")]
pub fn busy() -> bool {
    JOB.load(Acquire) != handover::EMPTY
}
