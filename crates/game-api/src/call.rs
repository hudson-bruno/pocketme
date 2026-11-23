//! Calling a game's functions safely.
//!
//! A game's `update` and `exit` take a raw pointer to a [`GameFrame`].
//! The game then reads and writes wherever that frame points: the
//! picture, the palette, the input, the log. A made-up frame would make
//! the game write over random memory.
//!
//! So the two functions are kept in wrapper types, [`UpdateFn`] and
//! [`ExitFn`]. Their only call takes a [`LiveFrame`], and a `LiveFrame`
//! can only be built from real buffers of the right sizes. The pointers
//! and sizes in it are filled in here, never by the caller. So the
//! console calls games without `unsafe`, and safe code cannot point a
//! game at memory that is not there.
//!
//! The wrappers are `repr(transparent)`: in memory they are exactly the
//! function pointer, so the game's table has the same bytes as before.
//!
//! Making a wrapper is `unsafe` ([`UpdateFn::new`]): whoever makes one
//! promises the function keeps the contract. The game kit does that for
//! every game, and [`crate::open_game`] for a table read from a game.

use core::marker::PhantomData;

use crate::{API_VERSION, GameFrame, Input, picture};

/// Bytes in the picture: 160 x 120, one palette number per pixel.
pub const PICTURE_BYTES: usize = picture::W as usize * picture::H as usize;

/// A game's `update`: one frame of the game, returns a [`crate::status`].
#[repr(transparent)]
#[derive(Clone, Copy, Debug)]
pub struct UpdateFn(unsafe extern "C" fn(frame: *mut GameFrame) -> u32);

/// A game's `exit`: the console ends the game (`reason` is an
/// [`crate::exit_reason`]).
#[repr(transparent)]
#[derive(Clone, Copy, Debug)]
pub struct ExitFn(unsafe extern "C" fn(frame: *mut GameFrame, reason: u32));

impl UpdateFn {
    /// Wraps a game's `update`.
    ///
    /// Only with `unsafe`:
    ///
    /// ```compile_fail,E0133
    /// unsafe extern "C" fn wild(_: *mut game_api::GameFrame) -> u32 { 0 }
    /// let _ = game_api::UpdateFn::new(wild);
    /// ```
    ///
    /// # Safety
    ///
    /// Called with a frame made by [`LiveFrame`], `f` must not do
    /// anything wrong: it may read and write the frame and what its
    /// pointers point at (within the sizes the frame gives), and nothing
    /// else of the console's. It must not keep the pointers after it
    /// returns.
    pub const unsafe fn new(f: unsafe extern "C" fn(frame: *mut GameFrame) -> u32) -> UpdateFn {
        UpdateFn(f)
    }

    /// The bare function, for a test that hands the game an odd frame
    /// on purpose. Calling it needs `unsafe`.
    pub const fn as_fn(self) -> unsafe extern "C" fn(frame: *mut GameFrame) -> u32 {
        self.0
    }

    /// Calls the game's `update` with a frame the console built.
    pub fn call(self, frame: &mut LiveFrame<'_>) -> u32 {
        // SAFETY: `new` was promised that `self.0` keeps the contract,
        // and a `LiveFrame` always points at real buffers of the sizes it
        // says, borrowed for as long as the frame lives.
        unsafe { (self.0)(&mut frame.frame) }
    }
}

impl ExitFn {
    /// An `exit` that does nothing: stands in for a game whose table has
    /// none (an older game).
    pub const NONE: ExitFn = ExitFn(crate::no_exit);

    /// Wraps a game's `exit`.
    ///
    /// # Safety
    ///
    /// As for [`UpdateFn::new`].
    pub const unsafe fn new(f: unsafe extern "C" fn(frame: *mut GameFrame, reason: u32)) -> ExitFn {
        ExitFn(f)
    }

    /// The bare function (see [`UpdateFn::as_fn`]).
    pub const fn as_fn(self) -> unsafe extern "C" fn(frame: *mut GameFrame, reason: u32) {
        self.0
    }

    /// Calls the game's `exit` with a frame the console built.
    pub fn call(self, frame: &mut LiveFrame<'_>, reason: u32) {
        // SAFETY: as in `UpdateFn::call`.
        unsafe { (self.0)(&mut frame.frame, reason) }
    }
}

/// A [`GameFrame`] for one call, pointing at real buffers.
///
/// It borrows the picture, the palette, the input and the log buffer for
/// as long as it lives, so nothing else can use or free them during the
/// call. The pointers, `size`, `version`, `width`, `height`, `stride`,
/// `log_cap` and `game_colours` are set here from the buffers; the caller
/// gives only the other numbers.
///
/// ```
/// use game_api::{GameFrame, Input, LiveFrame, PICTURE_BYTES};
///
/// #[repr(C, align(4))]
/// struct Picture([u8; PICTURE_BYTES]);
///
/// let mut picture = Picture([0; PICTURE_BYTES]);
/// let mut palette = [0u16; 256];
/// let input = Input::new();
/// let mut log = [0u8; 64];
/// let numbers = GameFrame { frame: 7, width: 9999, ..GameFrame::new() };
/// let live = LiveFrame::new(numbers, &mut picture.0, &mut palette, &input, &mut log).unwrap();
/// assert_eq!(live.get().frame, 7);
/// assert_eq!(live.get().width, 160, "the size comes from the buffer");
/// assert_eq!(live.get().log_cap, 64);
/// ```
pub struct LiveFrame<'a> {
    frame: GameFrame,
    _buffers: PhantomData<(&'a mut [u8], &'a mut [u16], &'a Input)>,
}

impl<'a> LiveFrame<'a> {
    /// A frame for one call. `numbers` gives the time, the events, the
    /// settings and the rest; its pointers and sizes are replaced.
    ///
    /// `None` when the picture does not start on a 4-byte boundary (the
    /// contract promises games that), and when `input.size` says the
    /// input is bigger than an [`Input`] is. `game_colours` is cut to 256,
    /// the palette's size, and `log_cap` to 65,535.
    pub fn new(
        numbers: GameFrame,
        pixels: &'a mut [u8; PICTURE_BYTES],
        palette: &'a mut [u16; 256],
        input: &'a Input,
        log: &'a mut [u8],
    ) -> Option<LiveFrame<'a>> {
        if !pixels.as_ptr().addr().is_multiple_of(4) {
            return None;
        }
        // A game reads `input.size` bytes of the input (up to the size of
        // its own `Input`). A size bigger than the real input would make a
        // game built for a later version read past its end.
        if input.size as usize > size_of::<Input>() {
            return None;
        }
        let frame = GameFrame {
            size: size_of::<GameFrame>() as u32,
            version: API_VERSION,
            pixels: pixels.as_mut_ptr(),
            palette: palette.as_mut_ptr(),
            input,
            log: log.as_mut_ptr(),
            width: picture::W,
            height: picture::H,
            stride: picture::W,
            game_colours: numbers.game_colours.min(256),
            log_cap: log.len().min(usize::from(u16::MAX)) as u16,
            ..numbers
        };
        Some(LiveFrame {
            frame,
            _buffers: PhantomData,
        })
    }

    /// The frame as it is now: before a call, what the game will get;
    /// after it, what the game wrote back. Copy what you need, then drop
    /// the frame to use the buffers again.
    pub fn get(&self) -> &GameFrame {
        &self.frame
    }
}
