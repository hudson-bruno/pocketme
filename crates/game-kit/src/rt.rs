//! What [`crate::game!`] calls. Not for games to use directly.
//!
//! This is the only place in the kit, with [`crate::Ctx`], that touches
//! the console's raw tables.

use core::sync::atomic::{AtomicPtr, Ordering};

use game_api::{
    API_VERSION, ConsoleApi, ExitFn, GameCallbacks, GameFrame, MIN_GAME_VERSION, UpdateFn,
    abort_code, field_end, status,
};

use crate::{Exit, Flow, Game, ctx::Ctx};

#[cfg(not(target_os = "none"))]
pub use std::thread_local;

/// The console's table, kept from the entry call for the panic handler.
static API: AtomicPtr<ConsoleApi> = AtomicPtr::new(core::ptr::null_mut());

/// Where the game's state is kept between calls.
pub struct Slot<G> {
    #[cfg(target_os = "none")]
    cell: core::cell::UnsafeCell<Option<G>>,
    #[cfg(not(target_os = "none"))]
    cell: core::cell::RefCell<Option<G>>,
}

// SAFETY: on the chip one core runs the console, and the console calls
// one game function at a time and never from an interrupt. So the state
// is never used from two places at once.
#[cfg(target_os = "none")]
unsafe impl<G> Sync for Slot<G> {}

impl<G> Slot<G> {
    pub const fn new() -> Self {
        Slot {
            #[cfg(target_os = "none")]
            cell: core::cell::UnsafeCell::new(None),
            #[cfg(not(target_os = "none"))]
            cell: core::cell::RefCell::new(None),
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut Option<G>) -> R) -> R {
        #[cfg(target_os = "none")]
        {
            // SAFETY: see the `Sync` note; calls are never nested.
            f(unsafe { &mut *self.cell.get() })
        }
        #[cfg(not(target_os = "none"))]
        {
            f(&mut self.cell.borrow_mut())
        }
    }
}

impl<G> Default for Slot<G> {
    fn default() -> Self {
        Self::new()
    }
}

/// The game's table, with this kit's size and version.
///
/// # Safety
/// `update` and `exit` keep the contract (see `game_api::UpdateFn::new`).
pub const unsafe fn callbacks(
    update: unsafe extern "C" fn(*mut GameFrame) -> u32,
    exit: unsafe extern "C" fn(*mut GameFrame, u32),
) -> GameCallbacks {
    GameCallbacks {
        size: size_of::<GameCallbacks>() as u32,
        version: API_VERSION,
        // SAFETY: the caller's promise.
        update: unsafe { UpdateFn::new(update) },
        exit: unsafe { ExitFn::new(exit) },
    }
}

/// The entry function's work: check the console's table and hand back
/// ours. Null when the console's table is missing, too old or too small.
///
/// # Safety
/// `api` is null or points at the console's table, which stays for as
/// long as the game runs.
pub unsafe fn entry(api: *const ConsoleApi, cb: &'static GameCallbacks) -> *const GameCallbacks {
    if api.is_null() || !api.addr().is_multiple_of(align_of::<ConsoleApi>()) {
        return core::ptr::null();
    }
    // SAFETY: the console hands a pointer to its table, which starts with
    // `size` and `version` in every version.
    let (size, version) = unsafe {
        let p = api.cast::<u32>();
        (p.read(), p.add(1).read())
    };
    if version < MIN_GAME_VERSION || (size as usize) < field_end!(ConsoleApi, abort) {
        return core::ptr::null();
    }
    API.store(api.cast_mut(), Ordering::Relaxed);
    cb
}

/// Makes the game's state anew, in a stack frame of its own.
///
/// Never inlined: the fresh state is first built on the stack (once
/// the size of the game's whole state, more if the game's `start`
/// builds parts of it at run time), and inside `update` that room
/// would be part of every call's frame, under everything the game
/// draws with. Here it is given back before the first `Game::update`
/// runs. Found on the CYD on 2026-11-22: a 10.8 KB state made
/// `update`'s frame 25.8 KB of the 29.2 KB stack.
#[inline(never)]
fn restart<G: Game>(s: &mut Option<G>, ctx: &mut Ctx) {
    *s = None;
    *s = Some(G::start(ctx));
}

/// One call of `update`.
///
/// # Safety
/// `raw` is null or the frame the console handed for this call.
pub unsafe fn update<G: Game>(slot: &Slot<G>, raw: *mut GameFrame) -> u32 {
    guarded(slot, raw, || {
        // SAFETY: the console hands its frame for this call.
        let Some(mut ctx) = (unsafe { Ctx::read(raw) }) else {
            slot.with(|s| *s = None);
            return status::ABORTED;
        };
        let st = slot.with(|s| {
            if ctx.started() || s.is_none() {
                restart(s, &mut ctx);
            }
            match s.as_mut().map(|g| g.update(&mut ctx)) {
                Some(Flow::Run) => status::RUN,
                _ => {
                    *s = None;
                    status::DONE
                }
            }
        });
        // SAFETY: the same frame, still in its call.
        unsafe { ctx.write_back(raw) };
        st
    })
}

/// One call of `exit`.
///
/// # Safety
/// As for [`update`].
pub unsafe fn exit<G: Game>(slot: &Slot<G>, raw: *mut GameFrame, reason: u32) {
    guarded(slot, raw, || {
        // SAFETY: the console hands its frame for this call.
        let ctx = unsafe { Ctx::read(raw) };
        let game = slot.with(Option::take);
        if let (Some(mut game), Some(mut ctx)) = (game, ctx) {
            game.exit(&mut ctx, Exit::from_code(reason));
            // SAFETY: the same frame, still in its call.
            unsafe { ctx.write_back(raw) };
        }
        status::RUN
    });
}

/// On the chip there is no unwinding: a panic goes to the handler.
#[cfg(target_os = "none")]
fn guarded<G>(_slot: &Slot<G>, _raw: *mut GameFrame, f: impl FnOnce() -> u32) -> u32 {
    f()
}

/// On the PC a panic is caught here: the state is dropped, the message
/// goes to the log, and the console hears "aborted".
#[cfg(not(target_os = "none"))]
fn guarded<G>(slot: &Slot<G>, raw: *mut GameFrame, f: impl FnOnce() -> u32) -> u32 {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(st) => st,
        Err(payload) => {
            // Dropping the state may panic too; that is caught as well.
            let _ = catch_unwind(AssertUnwindSafe(|| slot.with(|s| *s = None)));
            let msg = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| {
                    payload
                        .downcast_ref::<std::string::String>()
                        .map(|s| s.as_str())
                })
                .unwrap_or("?");
            // SAFETY: the console's frame, still in its call. Whatever the
            // game logged before the panic is lost; this line replaces it.
            if let Some(mut ctx) = unsafe { Ctx::read(raw) } {
                ctx.log_fmt(format_args!("panic: {msg}"));
                unsafe { ctx.write_back(raw) };
            }
            status::ABORTED
        }
    }
}

/// The chip's panic handler: ends the game through the console.
pub fn on_panic(info: &core::panic::PanicInfo<'_>) -> ! {
    let line = info.location().map_or(0, |l| l.line());
    let api = API.load(Ordering::Relaxed);
    if !api.is_null() {
        // SAFETY: `entry` checked this table, and the console keeps it
        // for as long as the game runs.
        let abort = unsafe { (*api).abort };
        abort(abort_code::PANIC, line);
    }
    loop {
        core::hint::spin_loop();
    }
}
