//! Opening a game's table: older, newer and broken tables.

use core::mem::offset_of;

use game_api::{
    API_VERSION, ConsoleApi, ExitFn, GameCallbacks, GameFrame, Input, LiveFrame, OpenError,
    PICTURE_BYTES, UNWIND_CONSOLE_API, UpdateFn, field_end, open_game, status,
};

extern "C" fn update_done(_f: *mut GameFrame) -> u32 {
    status::DONE
}

extern "C" fn exit_mark(f: *mut GameFrame, reason: u32) {
    // SAFETY: the test passes a pointer to a live table.
    unsafe { (*f).frame = reason }
}

/// A table with room for one more (unknown) function at the end, as a
/// newer game would have.
#[repr(C)]
struct NewerTable {
    now: GameCallbacks,
    later: extern "C" fn(),
}

extern "C" fn later() {}

// SAFETY (for every `new` below): these functions touch only the frame.
const UPDATE_DONE: UpdateFn = unsafe { UpdateFn::new(update_done) };
const EXIT_MARK: ExitFn = unsafe { ExitFn::new(exit_mark) };

static CURRENT: GameCallbacks = GameCallbacks {
    size: size_of::<GameCallbacks>() as u32,
    version: API_VERSION,
    update: UPDATE_DONE,
    exit: EXIT_MARK,
};

/// An older game: its table ends after `update`.
static OLDER: GameCallbacks = GameCallbacks {
    size: offset_of!(GameCallbacks, exit) as u32,
    version: API_VERSION,
    update: UPDATE_DONE,
    exit: EXIT_MARK,
};

static NEWER: NewerTable = NewerTable {
    now: GameCallbacks {
        size: size_of::<NewerTable>() as u32,
        version: API_VERSION + 1,
        update: UPDATE_DONE,
        exit: EXIT_MARK,
    },
    later,
};

static VERSION_1: GameCallbacks = GameCallbacks {
    version: 1,
    ..CURRENT
};

/// A one-player game (version 2): its input is laid out differently.
static VERSION_2: GameCallbacks = GameCallbacks {
    version: 2,
    ..CURRENT
};

static TINY: GameCallbacks = GameCallbacks { size: 8, ..CURRENT };

extern "C" fn entry_current(api: *const ConsoleApi) -> *const GameCallbacks {
    assert!(!api.is_null());
    &CURRENT
}
extern "C" fn entry_older(_: *const ConsoleApi) -> *const GameCallbacks {
    &OLDER
}
extern "C" fn entry_newer(_: *const ConsoleApi) -> *const GameCallbacks {
    &NEWER.now
}
extern "C" fn entry_null(_: *const ConsoleApi) -> *const GameCallbacks {
    core::ptr::null()
}
extern "C" fn entry_v1(_: *const ConsoleApi) -> *const GameCallbacks {
    &VERSION_1
}
extern "C" fn entry_v2(_: *const ConsoleApi) -> *const GameCallbacks {
    &VERSION_2
}
extern "C" fn entry_tiny(_: *const ConsoleApi) -> *const GameCallbacks {
    &TINY
}

/// Real buffers for one call.
#[repr(C, align(4))]
struct Buffers {
    pixels: [u8; PICTURE_BYTES],
    palette: [u16; 256],
    input: Input,
    log: [u8; 16],
}

impl Buffers {
    fn new() -> Box<Buffers> {
        Box::new(Buffers {
            pixels: [0; PICTURE_BYTES],
            palette: [0; 256],
            input: Input::new(),
            log: [0; 16],
        })
    }

    fn frame(&mut self) -> LiveFrame<'_> {
        LiveFrame::new(
            GameFrame::new(),
            &mut self.pixels,
            &mut self.palette,
            &self.input,
            &mut self.log,
        )
        .unwrap()
    }
}

fn call_update(cb: &GameCallbacks) -> u32 {
    let mut b = Buffers::new();
    cb.update.call(&mut b.frame())
}

fn call_exit(cb: &GameCallbacks) -> u32 {
    let mut b = Buffers::new();
    let mut f = b.frame();
    cb.exit.call(&mut f, 7);
    f.get().frame
}

#[test]
fn a_current_table_is_copied_as_it_is() {
    let cb = unsafe { open_game(entry_current, &UNWIND_CONSOLE_API) }.unwrap();
    assert_eq!(cb.size as usize, size_of::<GameCallbacks>());
    assert_eq!(call_update(&cb), status::DONE);
    assert_eq!(call_exit(&cb), 7);
}

#[test]
fn an_older_table_gets_an_exit_that_does_nothing() {
    let cb = unsafe { open_game(entry_older, &UNWIND_CONSOLE_API) }.unwrap();
    assert_eq!(cb.size as usize, field_end!(GameCallbacks, update));
    assert_eq!(call_update(&cb), status::DONE);
    assert_eq!(call_exit(&cb), 0, "the game's own exit must not be read");
}

#[test]
fn a_newer_table_is_read_as_far_as_this_console_knows() {
    let cb = unsafe { open_game(entry_newer, &UNWIND_CONSOLE_API) }.unwrap();
    assert_eq!(cb.version, API_VERSION + 1);
    assert!(cb.size as usize > size_of::<GameCallbacks>());
    assert_eq!(call_exit(&cb), 7);
}

#[test]
fn broken_tables_are_refused() {
    assert_eq!(
        unsafe { open_game(entry_null, &UNWIND_CONSOLE_API) }.unwrap_err(),
        OpenError::NoTable
    );
    assert_eq!(
        unsafe { open_game(entry_v1, &UNWIND_CONSOLE_API) }.unwrap_err(),
        OpenError::TooOld
    );
    assert_eq!(
        unsafe { open_game(entry_v2, &UNWIND_CONSOLE_API) }.unwrap_err(),
        OpenError::TooOld
    );
    assert_eq!(
        unsafe { open_game(entry_tiny, &UNWIND_CONSOLE_API) }.unwrap_err(),
        OpenError::TooSmall
    );
}

#[test]
fn the_pc_abort_panics_with_its_reason() {
    let r = std::panic::catch_unwind(|| (UNWIND_CONSOLE_API.abort)(1, 42));
    let msg = *r.unwrap_err().downcast::<String>().unwrap();
    assert!(msg.contains("panic") && msg.contains("42"), "{msg}");
}
