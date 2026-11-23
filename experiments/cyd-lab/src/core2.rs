//! Lab (feature `core2`): the chip's second core, put to work.
//!
//! The ESP32 has two cores. The console normally uses only the first.
//! This file starts the second one (the way esp-rtos does it: its own
//! stack, its own scheduler, its main job pinned to it) and gives it a
//! tiny "job box": the first core puts one job in the box, the second
//! core does it and empties the box. Two kinds of job:
//!
//! - **burn**: a stand-in for a game's update. It works on 4 KB of its
//!   own memory for a set time (so it also uses the memory bus, like a
//!   real game would).
//! - **present**: sends one picture to the panel, with the same tear-free
//!   timing as always. The first core first copies the picture (19,200
//!   bytes) and its colours into a box of their own, so it can draw the
//!   next picture while the second core sends this one.
//!
//! Only one job at a time. "join" means: wait until the box is empty.
//!
//! Costs kept here so the report can say them: the second core's stack
//! (how deep it was ever used), how long the start took.

use core::cell::UnsafeCell;
use core::sync::atomic::{
    AtomicBool, AtomicPtr, AtomicU32, Ordering::Acquire, Ordering::Relaxed, Ordering::Release,
};

use board_api::Rect;
#[cfg(not(feature = "core2-swap"))]
use board_api::{ByteOrder, Dir, Indexed, PixelSource};
use esp_hal::peripherals::{CPU_CTRL, FROM_CPU_INTR1};
use esp_hal::system::Stack;
use log::info;
use static_cell::ConstStaticCell;

use crate::lcd::{CPU_MHZ, H, W, cycles};
use crate::screen::{CydScreen, MAX_JOBS};

/// The second core's stack. esp-rtos's own example uses 8 KB; build.rs
/// takes another size from CYD_CORE2_STACK (measuring only).
pub const STACK_SIZE: usize = crate::mem::parse(env!("CYD_CORE2_STACK"));
static STACK: ConstStaticCell<Stack<STACK_SIZE>> = ConstStaticCell::new(Stack::new());
/// Where the stack lies, for measuring how deep it was used.
static STACK_LOW: AtomicU32 = AtomicU32::new(0);
const STACK_PATTERN: u32 = 0x5AC3_A53C;
/// Bytes at the stack's bottom not counted: esp-hal's guard word (60
/// bytes up) and, right above it, the start function esp-hal parks there
/// for the second core (system/multi_core.rs, setup_second_core_stack).
const GUARD_SKIP: usize = 256;

/// The job box. 0 = empty.
static JOB: AtomicU32 = AtomicU32::new(0);
const JOB_BURN: u32 = 1;
const JOB_PRESENT: u32 = 2;
/// The burn's length in microseconds.
static ARG: AtomicU32 = AtomicU32::new(0);

/// Counters of the second core.
static JOBS_DONE: AtomicU32 = AtomicU32::new(0);
static BUSY_US: AtomicU32 = AtomicU32::new(0);
static ALIVE: AtomicU32 = AtomicU32::new(0);
/// Whether the second core runs, and whether pictures go to it.
static STARTED: AtomicBool = AtomicBool::new(false);
static SEND_HERE: AtomicBool = AtomicBool::new(false);
/// The screen, while the second core sends a picture with it.
static SCREEN: AtomicPtr<CydScreen> = AtomicPtr::new(core::ptr::null_mut());

/// The screen's counters after the last picture the second core sent.
static COUNTERS: Shared<crate::screen::Counters> =
    Shared(UnsafeCell::new(crate::screen::Counters::ZERO));

/// The copy of one picture for the second core (copy version only).
#[cfg(not(feature = "core2-swap"))]
struct FrameBox {
    pixels: [u8; PW * PH],
    palette: [u16; 256],
    version: u32,
    /// Whether `palette` holds a copy yet (all zero at start, so the box
    /// costs RAM only, no space in the flash image).
    have_palette: bool,
    border: u16,
    areas: [Rect; MAX_JOBS],
    n: usize,
}
const PW: usize = W / 2;
const PH: usize = H / 2;

struct Shared<T>(UnsafeCell<T>);
// SAFETY: the job box protocol: the first core touches the inside only
// while the box is empty, the second only while it holds a job.
unsafe impl<T> Sync for Shared<T> {}

#[cfg(not(feature = "core2-swap"))]
static FRAME: Shared<FrameBox> = Shared(UnsafeCell::new(FrameBox {
    pixels: [0; PW * PH],
    palette: [0; 256],
    version: 0,
    have_palette: false,
    border: 0,
    areas: [Rect::EMPTY; MAX_JOBS],
    n: 0,
}));

/// Starts the second core. Returns the time it took, in microseconds.
pub fn start(cpu: CPU_CTRL<'static>, int1: FROM_CPU_INTR1<'static>) -> u32 {
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
    #[cfg(not(feature = "core2-swap"))]
    esp_rtos::start_second_core(cpu, int1, stack, || worker());
    #[cfg(feature = "core2-swap")]
    esp_rtos::start_second_core(cpu, int1, stack, || swap::second_main());
    let us = cycles().wrapping_sub(t) / CPU_MHZ;
    STARTED.store(true, Release);
    us
}

/// The second core's main job: wait for a job, do it, empty the box.
#[cfg(not(feature = "core2-swap"))]
fn worker() -> ! {
    loop {
        ALIVE.fetch_add(1, Relaxed);
        let job = JOB.load(Acquire);
        if job == 0 {
            // A short pause on the core's own clock (no memory traffic).
            let t = cycles();
            while cycles().wrapping_sub(t) < CPU_MHZ {}
            continue;
        }
        let t = cycles();
        if job == JOB_BURN {
            core::hint::black_box(crate::standin::burn(ARG.load(Relaxed)));
        } else if job == JOB_PRESENT {
            // SAFETY: the box holds a job, so the first core does not
            // touch the frame box or the screen until it is empty again.
            unsafe {
                let frame = &mut *FRAME.0.get();
                let screen = &mut *SCREEN.load(Acquire);
                let src = BoxSource(frame);
                let areas = frame.areas;
                screen.present_here(&src, &areas[..frame.n]);
                let c = screen.counters;
                critical_section::with(|_| *COUNTERS.0.get() = c);
            }
        }
        BUSY_US.fetch_add(cycles().wrapping_sub(t) / CPU_MHZ, Relaxed);
        JOBS_DONE.fetch_add(1, Relaxed);
        JOB.store(0, Release);
    }
}

/// Waits until the second core has finished its job. Returns the wait in
/// CPU cycles.
// The spin is on purpose and measured as it is (the copy version's
// numbers were taken with exactly this loop).
#[allow(clippy::missing_spin_loop)]
pub fn join() -> u32 {
    let t = cycles();
    while JOB.load(Acquire) != 0 {}
    cycles().wrapping_sub(t)
}

/// Gives the second core a burn of `us` microseconds (after the job
/// before it is done).
pub fn kick_burn(us: u32) {
    join();
    ARG.store(us, Relaxed);
    JOB.store(JOB_BURN, Release);
    #[cfg(feature = "core2-swap")]
    swap::wake();
}

pub fn started() -> bool {
    STARTED.load(Acquire)
}

pub fn send_here() -> bool {
    SEND_HERE.load(Acquire)
}

/// Pictures to the second core (true) or back to the first.
pub fn set_send_here(on: bool, screen: &mut CydScreen) {
    join();
    if on && !started() {
        return;
    }
    SEND_HERE.store(on, Release);
    // SAFETY: the box is empty, so the second core does not write it.
    critical_section::with(|_| unsafe { *COUNTERS.0.get() = screen.counters });
}

/// The screen's counters as the second core last left them.
pub fn counters() -> crate::screen::Counters {
    // A copy taken while the second core may write it: only for the
    // measurements, a value can be one picture old.
    critical_section::with(|_| unsafe { *COUNTERS.0.get() })
}

/// Keeps the counters copy right after the first core changed them.
pub fn counters_set(c: crate::screen::Counters) {
    join();
    // SAFETY: the box is empty.
    critical_section::with(|_| unsafe { *COUNTERS.0.get() = c });
}

/// The picture to the second core: copies it and posts the job. `None`
/// when this picture cannot go (not the plain 160x120 picture at 2x):
/// then the caller sends it itself.
#[cfg(not(feature = "core2-swap"))]
pub fn post_present(
    screen: &mut CydScreen,
    src: &(impl PixelSource + ?Sized),
    areas: &[Rect],
) -> Option<board_api::SendReport> {
    let ix = src.indexed().filter(|ix| {
        ix.scale == 2
            && ix.left == 0
            && ix.top == 0
            && usize::from(ix.width) == PW
            && usize::from(ix.height) == PH
    });
    let t0 = cycles();
    let waited = join();
    let ix = ix?;
    // SAFETY: the box is empty (joined above).
    let frame = unsafe { &mut *FRAME.0.get() };
    frame.pixels.copy_from_slice(&ix.pixels[..PW * PH]);
    if !frame.have_palette || frame.version != ix.palette_version {
        frame.palette.copy_from_slice(ix.palette);
        frame.version = ix.palette_version;
        frame.have_palette = true;
    }
    frame.border = ix.border;
    let full = Rect::new(0, 0, W as u16, H as u16);
    let mut report = board_api::SendReport::default();
    if areas.len() > MAX_JOBS {
        frame.areas[0] = full;
        frame.n = 1;
    } else {
        frame.areas[..areas.len()].copy_from_slice(areas);
        frame.n = areas.len();
    }
    for r in &frame.areas[..frame.n] {
        report.areas += 1;
        report.pixels += u32::from(r.w) * u32::from(r.h);
    }
    SCREEN.store(screen as *mut CydScreen, Release);
    JOB.store(JOB_PRESENT, Release);
    let t1 = cycles();
    report.wait_us = waited / CPU_MHZ;
    report.send_us = t1.wrapping_sub(t0).wrapping_sub(waited) / CPU_MHZ;
    POSTS.fetch_add(1, Relaxed);
    POST_CYCLES_MAX.fetch_max(t1.wrapping_sub(t0).wrapping_sub(waited), Relaxed);
    COPY_CYCLES_LAST.store(t1.wrapping_sub(t0).wrapping_sub(waited), Relaxed);
    Some(report)
}

static POSTS: AtomicU32 = AtomicU32::new(0);
static POST_CYCLES_MAX: AtomicU32 = AtomicU32::new(0);
static COPY_CYCLES_LAST: AtomicU32 = AtomicU32::new(0);

/// The copied picture as a pixel source for the screen code.
#[cfg(not(feature = "core2-swap"))]
struct BoxSource<'a>(&'a FrameBox);

#[cfg(not(feature = "core2-swap"))]
impl PixelSource for BoxSource<'_> {
    fn run(&self, x: u16, y: u16, dir: Dir, order: ByteOrder, out: &mut [u16]) {
        // Not used by the screen code for this picture (it takes the fast
        // path), but correct anyway.
        let (mut x, mut y) = (i32::from(x), i32::from(y));
        let (dx, dy) = match dir {
            Dir::Right => (1, 0),
            Dir::Left => (-1, 0),
            Dir::Down => (0, 1),
            Dir::Up => (0, -1),
        };
        for o in out.iter_mut() {
            let c =
                self.0.palette[usize::from(self.0.pixels[(y as usize / 2) * PW + x as usize / 2])];
            *o = match order {
                ByteOrder::Native => c,
                ByteOrder::Wire => c.to_be(),
            };
            x += dx;
            y += dy;
        }
    }

    fn indexed(&self) -> Option<Indexed<'_>> {
        Some(Indexed {
            pixels: &self.0.pixels,
            width: PW as u16,
            height: PH as u16,
            scale: 2,
            left: 0,
            top: 0,
            palette: &self.0.palette,
            palette_version: self.0.version,
            border: self.0.border,
        })
    }
}

/// How deep the second core's stack was ever used, in bytes.
pub fn stack_deepest() -> usize {
    let low = STACK_LOW.load(Relaxed) as usize;
    if low == 0 {
        return 0;
    }
    // esp-hal writes its stack guard word 60 bytes above the bottom
    // (ESP_HAL_CONFIG_STACK_GUARD_OFFSET, default 60) and watches it, and
    // parks the start function just above; neither is real use.
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

/// One `core2` line for the report.
pub fn report(burn_ms: u32, burn_on_core2: bool, clock_x: u64) {
    info!(
        "core2 started={} send_here={} burn_ms={burn_ms} burn_on={}_core console_clock_x={clock_x} jobs={} busy_ms={} alive={} posts={} post_us_max={} copy_us_last={} stack_deepest={} stack_size={STACK_SIZE}",
        started(),
        send_here(),
        if burn_on_core2 { "second" } else { "first" },
        JOBS_DONE.load(Relaxed),
        BUSY_US.load(Relaxed) / 1000,
        ALIVE.load(Relaxed),
        POSTS.load(Relaxed),
        POST_CYCLES_MAX.load(Relaxed) / CPU_MHZ,
        COPY_CYCLES_LAST.load(Relaxed) / CPU_MHZ,
        stack_deepest(),
    );
}

/// Lab (`core2-swap`): the second core sends the console's own picture,
/// with no copy of it.
///
/// The rule (from console-video's `present_handing`): after `hand_over`
/// the console leaves the handed picture alone until the next
/// `wait_free` returns, and draws in its other picture. So the second
/// core may read the handed picture until it empties the job box, and
/// nothing else of the console. The colours are not shared: the first
/// core turns them into the screen's wire table in `hand_over`, while the
/// second core is idle.
///
/// Who touches what, and when:
///
/// | memory                    | first core                                  | second core                    |
/// |---------------------------|---------------------------------------------|--------------------------------|
/// | handed picture (19,200 B) | reads only (catch-up copy, shots); writes it again only after `wait_free` | reads, while `JOB` = present |
/// | other picture, palette    | reads and writes                            | never                          |
/// | `SLOT` (areas, pointer)   | writes while `JOB` = 0                      | reads while `JOB` != 0         |
/// | the screen (`CydScreen`)  | uses it only while `JOB` = 0                | uses it while `JOB` = present  |
/// | `COUNTERS`                | reads a copy (critical section)             | writes a copy after each send  |
///
/// `JOB` is the hand-over flag: the first core stores it with Release
/// after its last write to `SLOT`/screen; the second core loads it with
/// Acquire, and stores 0 with Release after its last read; `join` loads
/// it with Acquire. Nothing else enforces the rule: if the console wrote
/// the handed picture early, the panel would show a mix of two pictures
/// (letter `V` checks for exactly that, with a sum before and after
/// each send).
///
/// Waiting without spinning: the second core runs an esp-rtos
/// thread-mode embassy executor. With no job, its task waits on an
/// `AtomicWaker`; the executor then puts its thread to sleep in the
/// esp-rtos scheduler and the core sits in the idle hook (`waiti`). The
/// first core's `wake()` resumes that thread through esp-rtos (it raises
/// the second core's software interrupt).
#[cfg(feature = "core2-swap")]
pub mod swap {
    use super::*;
    use board_api::SendReport;
    use console_video::{Handed, Sender};
    use core::future::poll_fn;
    use core::task::Poll;
    use esp_hal::asynch::AtomicWaker;
    use esp_rtos::embassy::{Callbacks, Executor};
    use static_cell::StaticCell;

    static WAKER: AtomicWaker = AtomicWaker::new();
    static EXECUTOR: StaticCell<Executor> = StaticCell::new();

    /// What the second core needs for one send: where the picture is and
    /// which areas. 16 areas = 128 bytes, plus the pointer.
    struct Slot {
        pixels: *const u8,
        areas: [Rect; MAX_JOBS],
        n: usize,
        /// Letter `V`: the sum of the picture when handed over.
        sum: u32,
    }
    static SLOT: Shared<Slot> = Shared(UnsafeCell::new(Slot {
        pixels: core::ptr::null(),
        areas: [Rect::EMPTY; MAX_JOBS],
        n: 0,
        sum: 0,
    }));

    /// Counters for the report.
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
    static VERIFY: AtomicBool = AtomicBool::new(false);
    static CHECKED: AtomicU32 = AtomicU32::new(0);
    static MISMATCH: AtomicU32 = AtomicU32::new(0);

    fn now_us32() -> u32 {
        embassy_time::Instant::now().as_micros() as u32
    }

    /// The second core's start: an executor with one task.
    pub(super) fn second_main() -> ! {
        let ex = EXECUTOR.init(Executor::new());
        ex.run_with_callbacks(|sp| sp.spawn(worker().unwrap()), Idle)
    }

    struct Idle;
    impl Callbacks for Idle {
        fn before_poll(&mut self) {}
        fn on_idle(&mut self) {
            // Called each time the executor is about to sleep.
            IDLES.fetch_add(1, Relaxed);
        }
    }

    /// Tells the second core a job is in the box.
    pub(super) fn wake() {
        HANDED_US.store(now_us32(), Relaxed);
        WAKER.wake();
    }

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
            let lag = now_us32().wrapping_sub(HANDED_US.load(Relaxed));
            WAKE_US_SUM.fetch_add(lag, Relaxed);
            WAKE_US_MAX.fetch_max(lag, Relaxed);
            ALIVE.fetch_add(1, Relaxed);
            let t = cycles();
            if job == JOB_BURN {
                core::hint::black_box(crate::standin::burn(ARG.load(Relaxed)));
            } else if job == JOB_PRESENT {
                // SAFETY: the box holds a job: the first core does not
                // touch the slot or the screen, nor write the handed
                // picture, until the box is empty again.
                unsafe {
                    let slot = &*SLOT.0.get();
                    let pixels = core::slice::from_raw_parts(slot.pixels, PW * PH);
                    let verify = VERIFY.load(Relaxed);
                    let before = if verify { sum(pixels) } else { 0 };
                    let screen = &mut *SCREEN.load(Acquire);
                    screen.present_held(pixels, &slot.areas[..slot.n]);
                    if verify {
                        let after = sum(pixels);
                        CHECKED.fetch_add(1, Relaxed);
                        if before != slot.sum || after != slot.sum {
                            MISMATCH.fetch_add(1, Relaxed);
                        }
                    }
                    let c = screen.counters;
                    critical_section::with(|_| *COUNTERS.0.get() = c);
                }
            }
            BUSY_US.fetch_add(cycles().wrapping_sub(t) / CPU_MHZ, Relaxed);
            JOBS_DONE.fetch_add(1, Relaxed);
            JOB.store(0, Release);
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

    /// Letter `V`: the picture check on or off.
    pub fn toggle_verify() -> bool {
        join();
        let on = !VERIFY.load(Relaxed);
        VERIFY.store(on, Relaxed);
        on
    }

    /// The console's way out to the second core.
    pub struct SwapSender {
        screen: *mut CydScreen,
        /// The cycle counter when `hand_over` gave a picture to the
        /// second core (this step), for the catch-up time.
        pub handed_at: Option<u32>,
    }

    impl SwapSender {
        pub fn new(screen: *mut CydScreen) -> Self {
            Self {
                screen,
                handed_at: None,
            }
        }
    }

    impl Sender for SwapSender {
        fn wait_free(&mut self) {
            FREE_CALLS.fetch_add(1, Relaxed);
            let busy = JOB.load(Acquire) != 0;
            let waited = join() / CPU_MHZ;
            if busy {
                WAITS.fetch_add(1, Relaxed);
                WAIT_US_SUM.fetch_add(waited, Relaxed);
                WAIT_US_MAX.fetch_max(waited, Relaxed);
            }
        }

        fn hand_over(&mut self, h: &Handed<'_>) -> SendReport {
            // Only called right after `wait_free`: the box is empty.
            let t0 = cycles();
            let plain = h.view.scale == 2 && h.view.left == 0 && h.view.top == 0;
            // SAFETY: the second core holds no job, so nobody else uses
            // the screen now.
            let screen = unsafe { &mut *self.screen };
            if !plain || !send_here() {
                INLINE.fetch_add(1, Relaxed);
                return screen.present_here(&h.source(), h.areas);
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
            SCREEN.store(self.screen, Release);
            JOB.store(JOB_PRESENT, Release);
            wake();
            let t1 = cycles();
            report.send_us = t1.wrapping_sub(t0) / CPU_MHZ;
            HANDS.fetch_add(1, Relaxed);
            HAND_CYC_MAX.fetch_max(t1.wrapping_sub(t0), Relaxed);
            self.handed_at = Some(t1);
            report
        }
    }

    /// The time from `hand_over`'s return to the end of the console's
    /// step: the catch-up copy, the swap and the rest of the step (an
    /// upper bound for the catch-up).
    pub fn note_after(cycles_after: u32) {
        let us = cycles_after / CPU_MHZ;
        AFTER_N.fetch_add(1, Relaxed);
        AFTER_US_SUM.fetch_add(us, Relaxed);
        AFTER_US_MAX.fetch_max(us, Relaxed);
    }

    /// Sets the swap counters to zero (letter `r`).
    pub fn reset() {
        for c in [
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
            &CHECKED,
            &MISMATCH,
            &JOBS_DONE,
            &BUSY_US,
            &ALIVE,
        ] {
            c.store(0, Relaxed);
        }
    }

    /// The wait counters, for the bench: (waits, wait_us_sum, wait_us_max).
    #[cfg(feature = "fs-bench")]
    pub fn waits() -> (u32, u32, u32) {
        (
            WAITS.load(Relaxed),
            WAIT_US_SUM.load(Relaxed),
            WAIT_US_MAX.load(Relaxed),
        )
    }

    /// One `core2swap` line for the report.
    pub fn report() {
        let hands = HANDS.load(Relaxed);
        let after_n = AFTER_N.load(Relaxed).max(1);
        let waits = WAITS.load(Relaxed);
        let jobs = JOBS_DONE.load(Relaxed).max(1);
        // Two lines: the log cuts lines at 320 bytes.
        info!(
            "core2swap hands={hands} inline={} hand_us_max={} free_calls={} waits={waits} wait_us_avg={} wait_us_max={} after_hand_n={} after_hand_us_avg={} after_hand_us_max={}",
            INLINE.load(Relaxed),
            HAND_CYC_MAX.load(Relaxed) / CPU_MHZ,
            FREE_CALLS.load(Relaxed),
            WAIT_US_SUM.load(Relaxed) / waits.max(1),
            WAIT_US_MAX.load(Relaxed),
            AFTER_N.load(Relaxed),
            AFTER_US_SUM.load(Relaxed) / after_n,
            AFTER_US_MAX.load(Relaxed),
        );
        info!(
            "core2swap jobs={} polls={} idles={} wake_us_avg={} wake_us_max={} verify={} checked={} mismatch={}",
            JOBS_DONE.load(Relaxed),
            POLLS.load(Relaxed),
            IDLES.load(Relaxed),
            WAKE_US_SUM.load(Relaxed) / jobs,
            WAKE_US_MAX.load(Relaxed),
            VERIFY.load(Relaxed),
            CHECKED.load(Relaxed),
            MISMATCH.load(Relaxed),
        );
    }
}
