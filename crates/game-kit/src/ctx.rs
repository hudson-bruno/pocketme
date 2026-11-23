//! [`Ctx`]: one call's view of the console, with no `unsafe` for the game.

use core::fmt;

use console_video::Frame;
use game_api::{
    GameFrame, Input, PLAYERS, PlayerInput, answer, event, field_end, input_flag, mode, picture,
    player_flag, request,
};

/// The smallest console frame this kit works with: all of version 2 (the
/// frame did not change in version 3).
const MIN_FRAME: usize = field_end!(GameFrame, spare);

/// Everything the game sees and asks for in one call.
///
/// The kit copies the console's tables into this at the start of the call
/// (as far as the console's `size` says; anything the console does not
/// have reads as 0) and writes the game's wishes back at the end.
pub struct Ctx {
    f: GameFrame,
    input: Input,
    /// How many bytes of `input` the console really gave.
    input_size: usize,
}

/// One player's pad in this call: [`Ctx::player`].
///
/// A player with no pad reads as nothing held, sticks in the middle and
/// no motion, so a game can read every player the same way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Player {
    p: PlayerInput,
}

impl Player {
    /// Whether any of these [`crate::button`] bits is held.
    pub fn held(&self, buttons: u32) -> bool {
        self.p.held & buttons != 0
    }

    /// Whether any of these buttons went down since the last call.
    pub fn pressed(&self, buttons: u32) -> bool {
        self.p.pressed & buttons != 0
    }

    /// Whether any of these buttons came up since the last call.
    pub fn released(&self, buttons: u32) -> bool {
        self.p.released & buttons != 0
    }

    /// All [`crate::button`] bits held now.
    pub fn held_bits(&self) -> u32 {
        self.p.held
    }

    /// All buttons that went down since the last call.
    pub fn pressed_bits(&self) -> u32 {
        self.p.pressed
    }

    /// All buttons that came up since the last call.
    pub fn released_bits(&self) -> u32 {
        self.p.released
    }

    /// Whether this player has a gamepad connected.
    pub fn connected(&self) -> bool {
        self.p.flags & player_flag::PAD != 0
    }

    /// Left stick x, y: -32767 to +32767, y grows downwards.
    pub fn left_stick(&self) -> (i16, i16) {
        (self.p.left_x, self.p.left_y)
    }

    /// Right stick x, y, the same way.
    pub fn right_stick(&self) -> (i16, i16) {
        (self.p.right_x, self.p.right_y)
    }

    /// Left and right trigger: 0 to 32767.
    pub fn triggers(&self) -> (u16, u16) {
        (self.p.left_trigger, self.p.right_trigger)
    }

    /// The pad's motion sensors, when it sends them now. Check every
    /// frame: some pads send motion only in one of their modes.
    pub fn motion(&self) -> Option<Motion> {
        if self.p.flags & player_flag::MOTION == 0 {
            return None;
        }
        Some(Motion {
            accel: self.p.accel,
            gyro: self.p.gyro,
        })
    }

    /// The raw table entry.
    pub fn raw(&self) -> &PlayerInput {
        &self.p
    }
}

/// The motion sensors of the pad, in [`crate::motion_units`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Motion {
    /// Accelerometer x, y, z (4096 per g; flat and still reads z = +4096).
    pub accel: [i16; 3],
    /// Gyroscope x, y, z (16 per degree per second).
    pub gyro: [i16; 3],
}

/// The touch, in game pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Touch {
    pub x: i16,
    pub y: i16,
    /// The finger is on the screen now (false: it just came up here).
    pub down: bool,
    /// It came down since the last call.
    pub started: bool,
    /// It came up since the last call.
    pub ended: bool,
}

/// Video settings: [`crate::mode`], [`crate::rate`], [`crate::hint`],
/// [`crate::colour`] values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VideoSettings {
    pub mode: u16,
    pub rate: u16,
    pub hint: u16,
    pub colour: u16,
}

/// What this console offers: bit `n` set means value `n` is offered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VideoOffers {
    pub modes: u32,
    pub rates: u32,
    pub hints: u32,
    pub colours: u32,
}

impl VideoOffers {
    /// Whether all four settings are offered; if not, the first that is
    /// not (in the order mode, rate, hint, colour, as the console checks).
    pub fn check(&self, want: VideoSettings) -> Result<(), Refused> {
        let has = |bits: u32, v: u16| v < 32 && bits & (1 << v) != 0;
        if !has(self.modes, want.mode) {
            Err(Refused::Mode)
        } else if !has(self.rates, want.rate) {
            Err(Refused::Rate)
        } else if !has(self.hints, want.hint) {
            Err(Refused::Hint)
        } else if !has(self.colours, want.colour) {
            Err(Refused::Colour)
        } else {
            Ok(())
        }
    }
}

/// Which video setting is not offered. Nothing was changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    Mode,
    Rate,
    Hint,
    Colour,
}

impl Ctx {
    /// Copies the console's frame and input. `None` when the frame is
    /// missing or smaller than version 2.
    ///
    /// # Safety
    /// `raw` is null or the frame the console handed for this call.
    pub(crate) unsafe fn read(raw: *const GameFrame) -> Option<Ctx> {
        if raw.is_null() || !raw.addr().is_multiple_of(align_of::<GameFrame>()) {
            return None;
        }
        // SAFETY: every version starts with `size`.
        let size = unsafe { raw.cast::<u32>().read() } as usize;
        if size < MIN_FRAME {
            return None;
        }
        let mut f = GameFrame::new();
        // SAFETY: the console says `size` bytes are its frame; we copy at
        // most our own size. The frame has no gaps (checked in game-api).
        unsafe {
            core::ptr::copy_nonoverlapping(
                raw.cast::<u8>(),
                (&raw mut f).cast::<u8>(),
                size.min(size_of::<GameFrame>()),
            );
        }
        if !f.pixels.addr().is_multiple_of(4) {
            f.pixels = core::ptr::null_mut();
        }
        if !f.palette.addr().is_multiple_of(2) {
            f.palette = core::ptr::null_mut();
        }
        // Fields the console's input does not have read 0.
        let mut input = Input::default();
        let mut input_size = 0;
        if !f.input.is_null() && f.input.addr().is_multiple_of(align_of::<Input>()) {
            // SAFETY: as for the frame, with the input's own `size`.
            let n = unsafe { f.input.cast::<u32>().read() } as usize;
            input_size = n.min(size_of::<Input>());
            unsafe {
                core::ptr::copy_nonoverlapping(
                    f.input.cast::<u8>(),
                    (&raw mut input).cast::<u8>(),
                    input_size,
                );
            }
        }
        Some(Ctx {
            f,
            input,
            input_size,
        })
    }

    /// Writes the game's wishes back into the console's frame.
    ///
    /// # Safety
    /// `raw` is the frame [`Ctx::read`] accepted, still in its call.
    pub(crate) unsafe fn write_back(&self, raw: *mut GameFrame) {
        // SAFETY: `read` checked that the console's frame holds all of
        // version 2, so each of these fields is there. Only the fields a
        // game writes are touched.
        unsafe {
            (*raw).wake_at_us = self.f.wake_at_us;
            (*raw).requests = self.f.requests;
            (*raw).log_len = self.f.log_len;
            (*raw).ask_mode = self.f.ask_mode;
            (*raw).ask_rate = self.f.ask_rate;
            (*raw).ask_hint = self.f.ask_hint;
            (*raw).ask_colour = self.f.ask_colour;
        }
    }

    // ---- Picture and palette ---------------------------------------------

    /// Whether the picture is the default 160 x 120 one this kit can draw.
    /// Always true in version 2.
    pub fn has_screen(&self) -> bool {
        !self.f.pixels.is_null()
            && self.f.mode == mode::PIXELS_160X120
            && usize::from(self.f.width) == console_video::W
            && usize::from(self.f.height) == console_video::H
            && usize::from(self.f.stride) == console_video::W
    }

    /// The picture to draw into, with all of the console's drawing (see
    /// [`Frame`]). It keeps what was drawn in the last call.
    ///
    /// Panics when [`Ctx::has_screen`] is false (never in version 2).
    pub fn screen(&mut self) -> &mut Frame {
        assert!(self.has_screen(), "no 160x120 picture");
        // SAFETY: the console's picture is 160 x 120 bytes, aligned to 4
        // (checked), valid and ours alone for this call. `Frame` is
        // `repr(C, align(4))` around exactly those bytes. The borrow ends
        // with `self`, which ends with the call.
        unsafe { &mut *self.f.pixels.cast::<Frame>() }
    }

    /// How many palette numbers the game may set, from 0 (240).
    pub fn game_colours(&self) -> u16 {
        if self.f.palette.is_null() {
            0
        } else {
            self.f.game_colours
        }
    }

    /// Sets palette number `i` to an RGB565 colour (see
    /// [`crate::rgb565`]). False, and nothing changed, for a number the
    /// console keeps (240 and up).
    pub fn set_colour(&mut self, i: u8, rgb565: u16) -> bool {
        if u16::from(i) >= self.game_colours() {
            return false;
        }
        // SAFETY: the palette has 256 entries; `i` is below 240.
        unsafe { self.f.palette.add(usize::from(i)).write(rgb565) };
        self.f.requests |= request::PALETTE;
        true
    }

    /// Sets palette numbers from `first` on. Returns how many were set
    /// (it stops at the console's part).
    pub fn set_colours(&mut self, first: u8, colours: &[u16]) -> usize {
        let room = usize::from(self.game_colours()).saturating_sub(usize::from(first));
        let n = colours.len().min(room);
        for (k, c) in colours[..n].iter().enumerate() {
            self.set_colour(first + k as u8, *c);
        }
        n
    }

    /// The colour of palette number `i` now (any of the 256).
    pub fn colour(&self, i: u8) -> u16 {
        if self.f.palette.is_null() {
            return 0;
        }
        // SAFETY: the palette has 256 entries.
        unsafe { self.f.palette.add(usize::from(i)).read() }
    }

    // ---- Input -------------------------------------------------------

    /// This call's input as the console gave it (fields the console did
    /// not have read 0).
    pub fn input(&self) -> &Input {
        &self.input
    }

    fn has(&self, end: usize) -> bool {
        self.input_size >= end
    }

    /// Player `n`'s pad (0 is player 1 on screen, up to
    /// [`crate::PLAYERS`]` - 1`). A number past the last player, or a
    /// player the console did not send, reads as no pad.
    pub fn player(&self, n: usize) -> Player {
        // Checked first: for a huge `n` the sum below would overflow (and
        // panic on the chip, where overflow checks are on).
        let Some(p) = self.input.players.get(n) else {
            return Player::default();
        };
        let end = core::mem::offset_of!(Input, players) + (n + 1) * size_of::<PlayerInput>();
        if self.has(end) {
            Player { p: *p }
        } else {
            Player::default()
        }
    }

    /// Whether player `n` has a gamepad connected.
    pub fn has_pad(&self, n: usize) -> bool {
        self.player(n).connected()
    }

    /// How many players have a gamepad connected (0 to
    /// [`crate::PLAYERS`]). They need not be the first ones: player 1
    /// may have none while player 3 has one.
    pub fn pad_count(&self) -> usize {
        (0..PLAYERS).filter(|&n| self.has_pad(n)).count()
    }

    /// Whether any of these [`crate::button`] bits is held by player 1
    /// (`player(0)`). The same for the calls below up to `motion`:
    /// one-player games read player 1 this way.
    pub fn held(&self, buttons: u32) -> bool {
        self.player(0).held(buttons)
    }

    /// Whether any of these buttons went down since the last call
    /// (player 1).
    pub fn pressed(&self, buttons: u32) -> bool {
        self.player(0).pressed(buttons)
    }

    /// Whether any of these buttons came up since the last call
    /// (player 1).
    pub fn released(&self, buttons: u32) -> bool {
        self.player(0).released(buttons)
    }

    /// Whether player 1 has a gamepad connected.
    pub fn pad_connected(&self) -> bool {
        self.has_pad(0)
    }

    /// Player 1's left stick x, y: -32767 to +32767, y grows downwards.
    pub fn left_stick(&self) -> (i16, i16) {
        self.player(0).left_stick()
    }

    /// Player 1's right stick x, y, the same way.
    pub fn right_stick(&self) -> (i16, i16) {
        self.player(0).right_stick()
    }

    /// Player 1's left and right trigger: 0 to 32767.
    pub fn triggers(&self) -> (u16, u16) {
        self.player(0).triggers()
    }

    /// The touch, if a finger is down or just came up.
    pub fn touch(&self) -> Option<Touch> {
        let fl = self.input.flags;
        if !self.has(field_end!(Input, touch_y))
            || fl & (input_flag::TOUCH | input_flag::TOUCH_END) == 0
        {
            return None;
        }
        Some(Touch {
            x: self.input.touch_x,
            y: self.input.touch_y,
            down: fl & input_flag::TOUCH != 0,
            started: fl & input_flag::TOUCH_START != 0,
            ended: fl & input_flag::TOUCH_END != 0,
        })
    }

    /// Player 1's motion sensors, when the pad sends them now. Check
    /// every frame: some pads send motion only in one of their modes.
    pub fn motion(&self) -> Option<Motion> {
        self.player(0).motion()
    }

    // ---- Time and events ----------------------------------------------

    /// Microseconds the game has run, pauses not counted.
    pub fn time_us(&self) -> u64 {
        self.f.time_us
    }

    /// Calls before this one in this run.
    pub fn frame(&self) -> u32 {
        self.f.frame
    }

    /// Game time since the last call, in microseconds.
    pub fn delta_us(&self) -> u32 {
        self.f.delta_us
    }

    /// The first call of this run.
    pub fn started(&self) -> bool {
        self.f.events & event::START != 0
    }

    /// The first call after the pause menu.
    pub fn resumed(&self) -> bool {
        self.f.events & event::RESUME != 0
    }

    /// The picture was lost: draw all of it.
    pub fn must_redraw(&self) -> bool {
        self.f.events & event::REDRAW != 0
    }

    /// The contract version the console was built with.
    pub fn console_version(&self) -> u32 {
        self.f.version
    }

    // ---- Video settings and waking -------------------------------------

    /// The settings in use now.
    pub fn video(&self) -> VideoSettings {
        VideoSettings {
            mode: self.f.mode,
            rate: self.f.rate,
            hint: self.f.hint,
            colour: self.f.colour,
        }
    }

    /// What this console offers.
    pub fn offers(&self) -> VideoOffers {
        VideoOffers {
            modes: self.f.offered_modes,
            rates: self.f.offered_rates,
            hints: self.f.offered_hints,
            colours: self.f.offered_colours,
        }
    }

    /// Asks for other video settings. Checked here against what the
    /// console offers: `Err` means nothing changes. `Ok` means the
    /// console takes them after this call; [`Ctx::video`] shows them
    /// from the next call on.
    pub fn ask_video(&mut self, want: VideoSettings) -> Result<(), Refused> {
        self.offers().check(want)?;
        self.f.ask_mode = want.mode;
        self.f.ask_rate = want.rate;
        self.f.ask_hint = want.hint;
        self.f.ask_colour = want.colour;
        self.f.requests |= request::VIDEO;
        Ok(())
    }

    /// Asks for another frame rate ([`crate::rate`]), the rest as now.
    pub fn set_rate(&mut self, rate: u16) -> Result<(), Refused> {
        self.ask_video(VideoSettings {
            rate,
            ..self.video()
        })
    }

    /// The console's answer to the last request ([`crate::answer`]).
    pub fn last_answer(&self) -> u16 {
        self.f.answer
    }

    /// Whether the last request was taken.
    pub fn last_request_ok(&self) -> bool {
        self.f.answer == answer::OK
    }

    /// With the on-demand rate: call again when game time reaches this
    /// (even with no input). Set it again in every call that wants it.
    pub fn wake_at(&mut self, time_us: u64) {
        self.f.wake_at_us = time_us;
    }

    /// The same, `us` microseconds from now.
    pub fn wake_after(&mut self, us: u64) {
        self.wake_at(self.f.time_us.saturating_add(us).max(1));
    }

    // ---- Log ---------------------------------------------------------

    /// Writes one line to the console's log (on the PC: the terminal; on
    /// the chip: the debug link). ASCII only; other signs show as `?`.
    /// What does not fit in this call's buffer is cut off.
    ///
    /// The text goes into the buffer as it is, without `core::fmt`, so a
    /// game that never calls [`Ctx::log_fmt`] does not carry the
    /// formatting code for it. The bytes are the same as from
    /// `log_fmt(format_args!("{line}"))`.
    pub fn log(&mut self, line: &str) {
        if let Some(mut w) = self.log_writer() {
            // The writer's own method, called directly: no `fmt::write`.
            let _ = fmt::Write::write_str(&mut w, line);
            // The newline always fits: the writer keeps one byte for it.
            w.push(b'\n');
        }
    }

    /// One log line built from pieces, without `core::fmt`:
    ///
    /// ```
    /// # fn f(ctx: &mut game_kit::Ctx, lap: u8, ticks: u32) {
    /// ctx.log_line().text("lap ").num(lap).text(" in ").num(ticks).end();
    /// # }
    /// ```
    ///
    /// Texts and whole numbers (every integer type up to 64 bits, signed
    /// ones with a `-` when below zero). The bytes are exactly those of
    /// `log_fmt(format_args!("lap {lap} in {ticks}"))`, with the same
    /// rules as [`Ctx::log`]: a newline in a text becomes a space, the
    /// line is cut off where the buffer is full (in the middle of a
    /// number too), and `end` adds the newline.
    ///
    /// A game that logs numbers this way and never calls [`Ctx::log_fmt`]
    /// does not carry the formatting code (about 0.4 to 2 KB of chip
    /// code). Only `end` finishes the line: a line that is never ended
    /// has no newline, so the compiler warns when the line is dropped.
    pub fn log_line(&mut self) -> LogLine<'_> {
        LogLine {
            w: self.log_writer(),
        }
    }

    /// [`Ctx::log`] with formatting: `ctx.log_fmt(format_args!("x {x}"))`.
    pub fn log_fmt(&mut self, args: fmt::Arguments<'_>) {
        if let Some(mut w) = self.log_writer() {
            let _ = fmt::write(&mut w, args);
            // The newline always fits: the writer keeps one byte for it.
            w.push(b'\n');
        }
    }

    /// A writer for one log line; none when there is no buffer or it is
    /// full.
    fn log_writer(&mut self) -> Option<LogWriter<'_>> {
        if self.f.log.is_null() || self.f.log_len >= self.f.log_cap {
            return None;
        }
        Some(LogWriter { ctx: self })
    }
}

/// Writes into the log buffer, always keeping one byte for the newline.
struct LogWriter<'a> {
    ctx: &'a mut Ctx,
}

impl LogWriter<'_> {
    fn push(&mut self, b: u8) {
        let f = &mut self.ctx.f;
        if f.log_len < f.log_cap {
            // SAFETY: `log` has `log_cap` bytes; `log_len` is below it.
            unsafe { f.log.add(usize::from(f.log_len)).write(b) };
            f.log_len += 1;
        }
    }
}

impl fmt::Write for LogWriter<'_> {
    /// Text into the line, a newline in it as a space; stops one byte
    /// before the end of the buffer.
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            let f = &self.ctx.f;
            if f.log_len + 1 >= f.log_cap {
                break;
            }
            self.push(if b == b'\n' { b' ' } else { b });
        }
        Ok(())
    }
}

/// One log line being built: [`Ctx::log_line`].
#[must_use = "a log line is finished (and gets its newline) only by `end()`"]
pub struct LogLine<'a> {
    /// None when there is no buffer or it was full before the line.
    w: Option<LogWriter<'a>>,
}

impl LogLine<'_> {
    /// Adds a text, as it is (a newline in it as a space).
    pub fn text(mut self, s: &str) -> Self {
        if let Some(w) = &mut self.w {
            // The writer's own method, called directly: no `fmt::write`.
            let _ = fmt::Write::write_str(w, s);
        }
        self
    }

    /// Adds a whole number in decimal, as `{}` writes it.
    pub fn num(self, n: impl LogNum) -> Self {
        // Generic only down to here: each type picks one of two shared
        // functions, so a game carries the digit code once.
        match n.parts() {
            Parts::Small(minus, n) => self.small(minus, n),
            Parts::Wide(minus, n) => self.wide(minus, n),
        }
    }

    #[inline(never)]
    fn small(self, minus: bool, n: u32) -> Self {
        self.text(Digits::small(minus, n).as_str())
    }

    #[inline(never)]
    fn wide(self, minus: bool, n: u64) -> Self {
        self.text(Digits::wide(minus, n).as_str())
    }

    /// Ends the line with its newline.
    pub fn end(self) {
        if let Some(mut w) = self.w {
            // The newline always fits: the writer keeps one byte for it.
            w.push(b'\n');
        }
    }
}

mod sealed {
    pub trait Sealed {}
}

/// A whole number [`LogLine::num`] can write: `u8` to `u64`, `i8` to
/// `i64`, `usize` and `isize`.
///
/// Numbers that fit in 32 bits are turned into digits with 32-bit sums;
/// only a 64-bit type that really holds a bigger number uses 64-bit
/// division (a library call on the chip), so a game that logs no 64-bit
/// numbers does not carry it.
pub trait LogNum: Copy + sealed::Sealed {
    #[doc(hidden)]
    fn parts(self) -> Parts;
}

/// A number as its sign and its size, 32 or 64 bits wide.
#[doc(hidden)]
pub enum Parts {
    Small(bool, u32),
    Wide(bool, u64),
}

/// The text of one number, kept at the end of a small buffer.
struct Digits {
    /// Room for the longest: `-9223372036854775808` and
    /// `18446744073709551615` are 20 signs each.
    b: [u8; 20],
    /// Where the text starts in `b`.
    at: usize,
}

impl Digits {
    /// `n` (with a `-` before it when `minus`), 32-bit sums only.
    fn small(minus: bool, mut n: u32) -> Digits {
        let mut d = Digits { b: [0; 20], at: 20 };
        // At most ten digits and a sign: `at` never reaches 0.
        loop {
            d.put(b'0' + (n % 10) as u8);
            n /= 10;
            if n == 0 {
                break;
            }
        }
        if minus {
            d.put(b'-');
        }
        d
    }

    /// The same for 64 bits; numbers that fit in 32 bits go to `small`.
    fn wide(minus: bool, mut n: u64) -> Digits {
        if let Ok(n) = u32::try_from(n) {
            return Digits::small(minus, n);
        }
        let mut d = Digits { b: [0; 20], at: 20 };
        // At most twenty digits, and a sign only below -2^32 (19 digits).
        loop {
            d.put(b'0' + (n % 10) as u8);
            n /= 10;
            if n == 0 {
                break;
            }
        }
        if minus {
            d.put(b'-');
        }
        d
    }

    /// One sign before the ones already there (nothing when full).
    fn put(&mut self, c: u8) {
        if let Some(at) = self.at.checked_sub(1)
            && let Some(slot) = self.b.get_mut(at)
        {
            *slot = c;
            self.at = at;
        }
    }

    fn as_str(&self) -> &str {
        // Only ASCII digits and '-' are ever put in.
        core::str::from_utf8(self.b.get(self.at..).unwrap_or(&[])).unwrap_or("")
    }
}

/// Unsigned types up to 32 bits.
macro_rules! log_num_small {
    ($($t:ty),*) => {$(
        impl sealed::Sealed for $t {}
        impl LogNum for $t {
            fn parts(self) -> Parts {
                Parts::Small(false, u32::from(self))
            }
        }
    )*};
}

/// Signed types up to 32 bits (`unsigned_abs` keeps `i32::MIN` whole).
macro_rules! log_num_small_signed {
    ($($t:ty),*) => {$(
        impl sealed::Sealed for $t {}
        impl LogNum for $t {
            fn parts(self) -> Parts {
                Parts::Small(self < 0, u32::from(self.unsigned_abs()))
            }
        }
    )*};
}

/// Types that may be 64 bits wide (`usize` is 32 bits on the chip and 64
/// on the PC): a number that fits in 32 bits goes to `small`, only a
/// bigger one to `wide`. On the chip `usize` and `isize` always fit, so
/// the 64-bit path is not built for them.
macro_rules! log_num_wide {
    ($($t:ty),*) => {$(
        impl sealed::Sealed for $t {}
        impl LogNum for $t {
            fn parts(self) -> Parts {
                let (minus, m) = Split::split(self);
                match u32::try_from(m) {
                    Ok(m) => Parts::Small(minus, m),
                    Err(_) => Parts::Wide(minus, m as u64),
                }
            }
        }
    )*};
}

/// A number as its sign and its size (`unsigned_abs` keeps `i64::MIN`
/// whole), so one macro serves signed and unsigned types.
trait Split {
    type U;
    fn split(self) -> (bool, Self::U);
}

macro_rules! split {
    (signed $($t:ty => $u:ty),*) => {$(
        impl Split for $t {
            type U = $u;
            fn split(self) -> (bool, $u) {
                (self < 0, self.unsigned_abs())
            }
        }
    )*};
    (unsigned $($t:ty),*) => {$(
        impl Split for $t {
            type U = $t;
            fn split(self) -> (bool, $t) {
                (false, self)
            }
        }
    )*};
}

log_num_small!(u8, u16, u32);
log_num_small_signed!(i8, i16, i32);
split!(signed i64 => u64, isize => usize);
split!(unsigned u64, usize);
log_num_wide!(u64, usize, i64, isize);

// The picture this kit draws is the contract's default picture.
const _: () = {
    assert!(console_video::W == picture::W as usize);
    assert!(console_video::H == picture::H as usize);
};
