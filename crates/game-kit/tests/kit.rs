//! The kit seen from the console's side: a tiny stand-in console calls
//! games made with `game!` through their raw tables.

use game_kit::api::{
    self, ConsoleApi, GameCallbacks, GameFrame, Input, PlayerInput, UNWIND_CONSOLE_API, event,
    input_flag, player_flag, request, status,
};
use game_kit::{Ctx, Exit, Flow, Frame, Game, Refused, VideoSettings, button, rate, rgb565};

/// What a console owns for one call.
struct Console {
    cb: GameCallbacks,
    frame: GameFrame,
    pixels: Box<Frame>,
    palette: [u16; 256],
    input: Input,
    log: [u8; 64],
}

impl Console {
    fn start(entry: api::GameEntry) -> Console {
        // SAFETY: every caller passes a `game_entry` made by `game!`.
        let cb = unsafe { api::open_game(entry, &UNWIND_CONSOLE_API) }.expect("game opens");
        Console {
            cb,
            frame: GameFrame::new(),
            pixels: Box::new(Frame::new()),
            palette: [0; 256],
            input: Input::new(),
            log: [0; 64],
        }
    }

    fn prepare(&mut self, events: u32) {
        let f = &mut self.frame;
        f.pixels = self.pixels.0.as_mut_ptr();
        f.palette = self.palette.as_mut_ptr();
        f.input = &self.input;
        f.log = self.log.as_mut_ptr();
        f.log_cap = self.log.len() as u16;
        f.log_len = 0;
        f.requests = 0;
        f.wake_at_us = 0;
        f.events = events;
        f.width = 160;
        f.height = 120;
        f.stride = 160;
        f.game_colours = 240;
        f.offered_modes = 1;
        f.offered_rates = 0b111;
        f.offered_hints = 0b111;
        f.offered_colours = 0b11;
    }

    fn update(&mut self, events: u32) -> u32 {
        self.prepare(events);
        // SAFETY: the frame points at this console's own buffers.
        let st = unsafe { self.cb.update.as_fn()(&mut self.frame) };
        self.frame.frame += 1;
        st
    }

    fn exit(&mut self, reason: u32) {
        self.prepare(0);
        // SAFETY: as in `update`.
        unsafe { self.cb.exit.as_fn()(&mut self.frame, reason) };
    }

    fn log_text(&self) -> String {
        String::from_utf8_lossy(&self.log[..usize::from(self.frame.log_len)]).into_owned()
    }
}

// ---- A game that uses most of the kit ------------------------------------

mod painter {
    use super::*;

    pub struct Painter {
        pub x: i16,
    }

    impl Game for Painter {
        fn start(ctx: &mut Ctx) -> Self {
            ctx.log("hello");
            Painter { x: 10 }
        }

        fn update(&mut self, ctx: &mut Ctx) -> Flow {
            if ctx.held(button::RIGHT) {
                self.x += 1;
            }
            if ctx.pressed(button::A) {
                ctx.set_colour(7, rgb565(255, 0, 0));
            }
            if ctx.pressed(button::Y) {
                // The console's colours cannot be set.
                assert!(!ctx.set_colour(250, 1));
                assert_eq!(ctx.set_colours(238, &[1, 2, 3]), 2);
            }
            if ctx.pressed(button::X) {
                let _ = ctx.set_rate(rate::ON_DEMAND);
                ctx.wake_after(1_000);
            }
            if ctx.pressed(button::L) {
                panic!("on purpose");
            }
            let x = self.x;
            let s = ctx.screen();
            s.clear(1);
            s.rect(x, 20, 4, 4, 7);
            if ctx.pressed(button::START) {
                Flow::Done
            } else {
                Flow::Run
            }
        }

        fn exit(&mut self, ctx: &mut Ctx, why: Exit) {
            ctx.log_fmt(format_args!("bye {why:?} at {}", self.x));
        }
    }

    game_kit::game!(Painter);
}

#[test]
fn a_game_starts_draws_and_logs() {
    let mut c = Console::start(painter::game_entry);
    assert_eq!(c.cb.version, api::API_VERSION);
    assert_eq!(c.update(event::START | event::REDRAW), status::RUN);
    assert_eq!(c.log_text(), "hello\n");
    assert_eq!(c.pixels.get(10, 20), 7);
    assert_eq!(c.pixels.get(0, 0), 1);
    assert_eq!(c.frame.requests, 0);

    c.input.players[0].held = button::RIGHT | button::A;
    c.input.players[0].pressed = button::A;
    assert_eq!(c.update(0), status::RUN);
    assert_eq!(c.pixels.get(11, 20), 7);
    assert_eq!(c.pixels.get(10, 20), 1);
    assert_eq!(c.palette[7], rgb565(255, 0, 0));
    assert_eq!(c.frame.requests, request::PALETTE);
    assert_eq!(c.frame.log_len, 0, "the console clears the log each call");
}

#[test]
fn only_the_games_colours_can_be_set() {
    let mut c = Console::start(painter::game_entry);
    c.update(event::START);
    c.input.players[0].pressed = button::Y;
    assert_eq!(c.update(0), status::RUN);
    assert_eq!(&c.palette[238..241], &[1, 2, 0]);
}

#[test]
fn a_rate_request_and_a_timer_go_back_to_the_console() {
    let mut c = Console::start(painter::game_entry);
    c.update(event::START);
    c.frame.time_us = 5_000;
    c.input.players[0].pressed = button::X;
    c.update(0);
    assert_eq!(c.frame.requests, request::VIDEO);
    assert_eq!(c.frame.ask_rate, rate::ON_DEMAND);
    assert_eq!(c.frame.wake_at_us, 6_000);
}

#[test]
fn done_ends_the_run_and_the_next_start_is_fresh() {
    let mut c = Console::start(painter::game_entry);
    c.update(event::START);
    c.input.players[0].held = button::RIGHT;
    c.update(0);
    c.input = Input::new();
    c.input.players[0].pressed = button::START;
    assert_eq!(c.update(0), status::DONE);
    c.input = Input::new();
    c.update(event::START);
    assert_eq!(c.pixels.get(10, 20), 7, "x starts at 10 again");
}

#[test]
fn exit_reaches_the_game_with_its_reason() {
    let mut c = Console::start(painter::game_entry);
    c.update(event::START);
    c.exit(api::exit_reason::PLAYER);
    assert_eq!(c.log_text(), "bye Player at 10\n");
    // The state is gone: a second exit says nothing.
    c.exit(api::exit_reason::CONSOLE);
    assert_eq!(c.log_text(), "");
}

#[test]
fn a_panic_is_caught_and_reported_as_aborted() {
    let mut c = Console::start(painter::game_entry);
    c.update(event::START);
    c.input.players[0].pressed = button::L;
    assert_eq!(c.update(0), status::ABORTED);
    assert_eq!(c.log_text(), "panic: on purpose\n");
    // A new run works.
    c.input = Input::new();
    assert_eq!(c.update(event::START), status::RUN);
}

mod talker {
    use super::*;
    pub struct Talker;
    impl Game for Talker {
        fn start(_: &mut Ctx) -> Self {
            Talker
        }
        fn update(&mut self, ctx: &mut Ctx) -> Flow {
            ctx.log("first\nline");
            for _ in 0..20 {
                ctx.log("0123456789");
            }
            Flow::Run
        }
    }
    game_kit::game!(Talker);
}

#[test]
fn a_long_log_line_is_cut_and_still_ends_the_line() {
    let mut c = Console::start(talker::game_entry);
    c.update(event::START);
    let t = c.log_text();
    assert_eq!(t.len(), 64);
    assert!(t.starts_with("first line\n0123456789\n"), "{t:?}");
    assert!(t.ends_with('\n'));
}

/// Lines for the two log paths: empty, newlines, a long one, not ASCII.
const LINES: [&str; 7] = [
    "",
    "a",
    "first\nline",
    "\n\n",
    "tab\tand ~ {} {{",
    "0123456789012345678901234567890123456789012345678901234567890123456789",
    "caf\u{e9} \u{2192}",
];

mod plain_log {
    use super::*;
    pub struct Plain;
    impl Game for Plain {
        fn start(_: &mut Ctx) -> Self {
            Plain
        }
        fn update(&mut self, ctx: &mut Ctx) -> Flow {
            for line in LINES {
                ctx.log(line);
            }
            Flow::Run
        }
    }
    game_kit::game!(Plain);
}

mod formatted_log {
    use super::*;
    pub struct Formatted;
    impl Game for Formatted {
        fn start(_: &mut Ctx) -> Self {
            Formatted
        }
        fn update(&mut self, ctx: &mut Ctx) -> Flow {
            for line in LINES {
                ctx.log_fmt(format_args!("{line}"));
            }
            Flow::Run
        }
    }
    game_kit::game!(Formatted);
}

/// The whole log buffer and its length after one call, with the buffer
/// `cap` bytes long and `used` bytes already taken (`None`: no buffer).
fn log_after(entry: api::GameEntry, cap: Option<u16>, used: u16) -> (Vec<u8>, u16) {
    let mut c = Console::start(entry);
    c.prepare(event::START);
    match cap {
        Some(cap) => c.frame.log_cap = cap,
        None => {
            c.frame.log = core::ptr::null_mut();
            c.frame.log_cap = 0;
        }
    }
    c.frame.log_len = used;
    // SAFETY: the frame points at this console's own buffers.
    let st = unsafe { c.cb.update.as_fn()(&mut c.frame) };
    assert_eq!(st, status::RUN);
    (c.log.to_vec(), c.frame.log_len)
}

#[test]
fn log_writes_the_same_bytes_as_log_fmt_with_the_line_alone() {
    let mut cases = vec![(None, 0)];
    for cap in 0..=64u16 {
        for used in [
            0,
            1,
            cap / 2,
            cap.saturating_sub(2),
            cap.saturating_sub(1),
            cap,
        ] {
            cases.push((Some(cap), used.min(cap)));
        }
    }
    for (cap, used) in cases {
        let plain = log_after(plain_log::game_entry, cap, used);
        let formatted = log_after(formatted_log::game_entry, cap, used);
        assert_eq!(plain, formatted, "buffer {cap:?}, {used} bytes used");
    }
    // The full buffer, seen whole once.
    let (bytes, len) = log_after(plain_log::game_entry, Some(64), 0);
    assert_eq!(len, 64);
    assert_eq!(&bytes[..24], b"\na\nfirst line\n  \ntab\tand");
}

// ---- Numbers in the log (LogLine) ---------------------------------------

mod numbers {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        /// Which line the next call writes (the game state is per thread
        /// on the PC, so this is too).
        pub static CASE: Cell<usize> = const { Cell::new(0) };
        /// `log_fmt` instead of `log_line`.
        pub static FMT: Cell<bool> = const { Cell::new(false) };
    }

    /// One line: `text` then the number, through `log_line` or `log_fmt`.
    macro_rules! line {
        ($ctx:expr, $fmt:expr, $text:expr, $n:expr) => {
            if $fmt {
                $ctx.log_fmt(format_args!("{}{}", $text, $n))
            } else {
                $ctx.log_line().text($text).num($n).end()
            }
        };
    }

    /// The edge values of every type `num` takes, and a few in between.
    pub const CASES: usize = 33;

    pub fn write(ctx: &mut Ctx, k: usize, fmt: bool) {
        match k {
            0 => line!(ctx, fmt, "n ", 0u32),
            1 => line!(ctx, fmt, "n ", -1i32),
            2 => line!(ctx, fmt, "n ", i32::MIN),
            3 => line!(ctx, fmt, "n ", i32::MAX),
            4 => line!(ctx, fmt, "n ", u32::MAX),
            5 => line!(ctx, fmt, "n ", u64::MAX),
            6 => line!(ctx, fmt, "n ", i64::MIN),
            7 => line!(ctx, fmt, "n ", i64::MAX),
            8 => line!(ctx, fmt, "n ", u64::from(u32::MAX) + 1),
            9 => line!(ctx, fmt, "n ", -i64::from(u32::MAX)),
            10 => line!(ctx, fmt, "n ", -i64::from(u32::MAX) - 1),
            11 => line!(ctx, fmt, "n ", 0u64),
            12 => line!(ctx, fmt, "n ", -1i64),
            13 => line!(ctx, fmt, "n ", u8::MAX),
            14 => line!(ctx, fmt, "n ", i8::MIN),
            15 => line!(ctx, fmt, "n ", i8::MAX),
            16 => line!(ctx, fmt, "n ", u16::MAX),
            17 => line!(ctx, fmt, "n ", i16::MIN),
            18 => line!(ctx, fmt, "n ", usize::MAX),
            19 => line!(ctx, fmt, "n ", isize::MIN),
            20 => line!(ctx, fmt, "n ", isize::MAX),
            21 => line!(ctx, fmt, "n ", 0usize),
            22 => line!(ctx, fmt, "n ", 9u32),
            23 => line!(ctx, fmt, "n ", 10u32),
            24 => line!(ctx, fmt, "n ", 999_999_999u32),
            25 => line!(ctx, fmt, "n ", 1_000_000_000u32),
            26 => line!(ctx, fmt, "n ", -10i16),
            27 => line!(ctx, fmt, "", 7u8),
            28 => line!(ctx, fmt, "two\nlines ", -42i32),
            29 => {
                // Several pieces, and the number first.
                let (a, b, c) = (123_456u32, -7_890i16, 255u8);
                if fmt {
                    ctx.log_fmt(format_args!("{a}: b {b}, c {c}/{a}"))
                } else {
                    ctx.log_line()
                        .num(a)
                        .text(": b ")
                        .num(b)
                        .text(", c ")
                        .num(c)
                        .text("/")
                        .num(a)
                        .end()
                }
            }
            30 => {
                // Texts only, and an empty line.
                if fmt {
                    ctx.log_fmt(format_args!("{}{}", "", ""))
                } else {
                    ctx.log_line().text("").end()
                }
            }
            31 => {
                // Two lines in one call.
                line!(ctx, fmt, "first ", u32::MAX);
                line!(ctx, fmt, "second ", i64::MIN);
            }
            _ => line!(ctx, fmt, "x ", 1_234_567_890_123_456_789u64),
        }
    }

    pub struct Numbers;
    impl Game for Numbers {
        fn start(_: &mut Ctx) -> Self {
            Numbers
        }
        fn update(&mut self, ctx: &mut Ctx) -> Flow {
            write(ctx, CASE.get(), FMT.get());
            Flow::Run
        }
    }
    game_kit::game!(Numbers);
}

/// The log after line `k`, through `log_line` or `log_fmt`.
fn number_log(k: usize, fmt: bool, cap: Option<u16>, used: u16) -> (Vec<u8>, u16) {
    numbers::CASE.set(k);
    numbers::FMT.set(fmt);
    log_after(numbers::game_entry, cap, used)
}

#[test]
fn log_line_writes_the_same_bytes_as_log_fmt_for_every_edge_number() {
    for k in 0..numbers::CASES {
        let mut cases = vec![(None, 0)];
        for cap in 0..=64u16 {
            // Every start: so each line is cut at every byte, in the middle
            // of each number too.
            for used in 0..=cap {
                cases.push((Some(cap), used));
            }
        }
        for (cap, used) in cases {
            let helper = number_log(k, false, cap, used);
            let formatted = number_log(k, true, cap, used);
            assert_eq!(
                helper, formatted,
                "line {k}, buffer {cap:?}, {used} bytes used"
            );
        }
    }
}

#[test]
fn log_line_writes_the_numbers_whole_and_cuts_them_when_full() {
    let text = |k: usize, cap: u16| {
        let (bytes, len) = number_log(k, false, Some(cap), 0);
        String::from_utf8(bytes[..usize::from(len)].to_vec()).unwrap()
    };
    assert_eq!(text(0, 64), "n 0\n");
    assert_eq!(text(1, 64), "n -1\n");
    assert_eq!(text(2, 64), "n -2147483648\n");
    assert_eq!(text(3, 64), "n 2147483647\n");
    assert_eq!(text(4, 64), "n 4294967295\n");
    assert_eq!(text(5, 64), "n 18446744073709551615\n");
    assert_eq!(text(6, 64), "n -9223372036854775808\n");
    assert_eq!(text(7, 64), "n 9223372036854775807\n");
    assert_eq!(text(28, 64), "two lines -42\n");
    assert_eq!(text(29, 64), "123456: b -7890, c 255/123456\n");
    assert_eq!(text(30, 64), "\n");
    // Cut in the middle of a number: seven signs fit, then the newline.
    assert_eq!(text(4, 8), "n 42949\n");
    assert_eq!(text(6, 5), "n -9\n");
    // Only the newline fits.
    assert_eq!(text(6, 1), "\n");
    // A full buffer takes nothing.
    assert_eq!(number_log(6, false, Some(16), 16).1, 16);
}

// ---- Input -------------------------------------------------------------

#[derive(Default)]
struct Seen {
    input: Input,
    motion: Option<game_kit::Motion>,
    touch: Option<game_kit::Touch>,
    sticks: ((i16, i16), (i16, i16)),
    triggers: (u16, u16),
    pad: bool,
    players: [game_kit::Player; 4],
    has: [bool; 4],
    count: usize,
    past_the_last: game_kit::Player,
    /// Players 7, 255 and `usize::MAX`: none of them exists.
    far: [game_kit::Player; 3],
    far_pads: [bool; 3],
}

std::thread_local! {
    static SEEN: std::cell::RefCell<Seen> = std::cell::RefCell::default();
}

struct Watcher;
impl Game for Watcher {
    fn start(_: &mut Ctx) -> Self {
        Watcher
    }
    fn update(&mut self, ctx: &mut Ctx) -> Flow {
        SEEN.with(|s| {
            *s.borrow_mut() = Seen {
                input: *ctx.input(),
                motion: ctx.motion(),
                touch: ctx.touch(),
                sticks: (ctx.left_stick(), ctx.right_stick()),
                triggers: ctx.triggers(),
                pad: ctx.pad_connected(),
                players: [ctx.player(0), ctx.player(1), ctx.player(2), ctx.player(3)],
                has: [
                    ctx.has_pad(0),
                    ctx.has_pad(1),
                    ctx.has_pad(2),
                    ctx.has_pad(3),
                ],
                count: ctx.pad_count(),
                past_the_last: ctx.player(game_kit::PLAYERS),
                far: [ctx.player(7), ctx.player(255), ctx.player(usize::MAX)],
                far_pads: [ctx.has_pad(7), ctx.has_pad(255), ctx.has_pad(usize::MAX)],
            }
        });
        Flow::Run
    }
}
mod watcher {
    game_kit::game!(super::Watcher);
}

#[test]
fn the_kit_hands_on_every_input() {
    let mut c = Console::start(watcher::game_entry);
    c.input.flags = input_flag::TOUCH | input_flag::TOUCH_START;
    c.input.touch_x = 159;
    c.input.touch_y = 119;
    c.input.players[0] = PlayerInput {
        held: button::ALL,
        pressed: button::A,
        released: button::B,
        flags: player_flag::PAD | player_flag::MOTION,
        left_x: -32767,
        left_y: 1,
        right_x: 2,
        right_y: 32767,
        left_trigger: 100,
        right_trigger: 32767,
        accel: [0, 0, 4096],
        gyro: [-16, 0, 16],
    };
    c.update(event::START);
    SEEN.with(|s| {
        let s = s.borrow();
        assert_eq!(s.input, c.input);
        assert!(s.pad);
        assert_eq!(s.sticks, ((-32767, 1), (2, 32767)));
        assert_eq!(s.triggers, (100, 32767));
        let t = s.touch.unwrap();
        assert_eq!(
            (t.x, t.y, t.down, t.started, t.ended),
            (159, 119, true, true, false)
        );
        let m = s.motion.unwrap();
        assert_eq!(m.accel, [0, 0, 4096]);
        assert_eq!(m.gyro, [-16, 0, 16]);
        // The one-player calls are player 1's.
        assert_eq!(*s.players[0].raw(), c.input.players[0]);
        assert_eq!(s.players[0].held_bits(), button::ALL);
        assert_eq!(s.has, [true, false, false, false]);
        assert_eq!(s.count, 1);
    });

    // Motion and touch off.
    c.input.flags = 0;
    c.input.players[0].flags = player_flag::PAD;
    c.update(0);
    SEEN.with(|s| {
        let s = s.borrow();
        assert_eq!(s.motion, None);
        assert_eq!(s.touch, None);
    });
}

#[test]
fn each_player_reads_only_its_own_pad() {
    let mut c = Console::start(watcher::game_entry);
    // Player 1 has no pad; players 2 and 4 have one; player 3's entry
    // holds stray numbers but no pad flag.
    c.input.players[1] = PlayerInput {
        held: button::A | button::UP,
        pressed: button::A,
        flags: player_flag::PAD | player_flag::MOTION,
        left_x: 300,
        accel: [1, 2, 3],
        ..PlayerInput::NONE
    };
    c.input.players[3] = PlayerInput {
        released: button::B,
        flags: player_flag::PAD,
        right_trigger: 7,
        ..PlayerInput::NONE
    };
    c.update(event::START);
    SEEN.with(|s| {
        let s = s.borrow();
        assert_eq!(s.has, [false, true, false, true]);
        assert_eq!(s.count, 2);
        assert!(!s.pad, "player 1 has no pad");
        assert_eq!(s.sticks, ((0, 0), (0, 0)), "player 1's sticks");
        assert_eq!(s.motion, None, "player 1 has no motion");
        let p2 = s.players[1];
        assert!(p2.held(button::A) && p2.pressed(button::A) && !p2.released(button::A));
        assert_eq!(
            (p2.held_bits(), p2.pressed_bits()),
            (button::A | button::UP, button::A)
        );
        assert_eq!(p2.left_stick(), (300, 0));
        assert_eq!(p2.motion().unwrap().accel, [1, 2, 3]);
        let p4 = s.players[3];
        assert!(p4.released(button::B) && !p4.held(button::B));
        assert_eq!(p4.triggers(), (0, 7));
        assert_eq!(p4.motion(), None);
        assert_eq!(s.players[2], game_kit::Player::default());
        assert_eq!(s.past_the_last, game_kit::Player::default());
        // Far past the last player: nothing, and no overflow panic.
        assert_eq!(s.far, [game_kit::Player::default(); 3]);
        assert_eq!(s.far_pads, [false; 3]);
    });
}

#[test]
fn an_older_consoles_input_reads_as_zero_past_its_size() {
    let mut c = Console::start(watcher::game_entry);
    // A console whose input ends after player 1: player 2's entry says
    // "pad, motion", but the kit must not believe numbers it was not
    // given.
    c.input.size = (core::mem::offset_of!(Input, players) + size_of::<PlayerInput>()) as u32;
    c.input.flags = input_flag::TOUCH;
    c.input.touch_x = 5;
    c.input.players[0].flags = player_flag::PAD;
    c.input.players[1] = PlayerInput {
        held: button::A,
        flags: player_flag::PAD | player_flag::MOTION,
        accel: [1, 2, 3],
        ..PlayerInput::NONE
    };
    c.update(event::START);
    SEEN.with(|s| {
        let s = s.borrow();
        assert_eq!(s.has, [true, false, false, false]);
        assert_eq!(s.count, 1);
        assert_eq!(s.players[1], game_kit::Player::default());
        assert_eq!(s.input.players[1], PlayerInput::NONE);
        assert_eq!(s.touch.unwrap().x, 5);
    });
}

// ---- Table sizes ---------------------------------------------------------

#[test]
fn an_older_or_broken_console_table_is_refused_at_entry() {
    let old = ConsoleApi {
        version: 1,
        ..UNWIND_CONSOLE_API
    };
    // A one-player console (version 2) hands the old input: refused.
    let v2 = ConsoleApi {
        version: 2,
        ..UNWIND_CONSOLE_API
    };
    let small = ConsoleApi {
        size: 8,
        ..UNWIND_CONSOLE_API
    };
    // SAFETY: null or real tables, alive for the whole test.
    unsafe {
        assert!(painter::game_entry(core::ptr::null()).is_null());
        assert!(painter::game_entry(&old).is_null());
        assert!(painter::game_entry(&v2).is_null());
        assert!(painter::game_entry(&small).is_null());
        assert!(!painter::game_entry(&UNWIND_CONSOLE_API).is_null());
    }
}

#[test]
fn a_frame_smaller_than_version_2_aborts_without_calling_the_game() {
    let mut c = Console::start(painter::game_entry);
    c.prepare(event::START);
    c.frame.size = core::mem::offset_of!(GameFrame, answer) as u32;
    // SAFETY: a short frame on purpose; the kit must read only its size.
    let st = unsafe { c.cb.update.as_fn()(&mut c.frame) };
    assert_eq!(st, status::ABORTED);
    assert_eq!(c.frame.log_len, 0);
    assert_eq!(c.pixels.get(0, 0), 0, "nothing was drawn");
}

/// A newer console's frame: version 2 plus a field this kit does not know.
#[repr(C)]
struct NewerFrame {
    now: GameFrame,
    later: u64,
}

#[test]
fn a_newer_consoles_frame_works_and_its_new_fields_are_left_alone() {
    let mut c = Console::start(painter::game_entry);
    c.prepare(event::START);
    let mut big = NewerFrame {
        now: GameFrame {
            size: size_of::<NewerFrame>() as u32,
            version: api::API_VERSION + 1,
            ..c.frame
        },
        later: 0x1234_5678,
    };
    // SAFETY: a bigger frame on purpose, pointing at the real buffers.
    let st = unsafe { c.cb.update.as_fn()(&mut big.now) };
    assert_eq!(st, status::RUN);
    assert_eq!(big.later, 0x1234_5678);
    assert_eq!(c.pixels.get(10, 20), 7);
    assert_eq!(big.now.log_len, 6, "hello and a newline");
}

mod asker {
    use super::*;
    pub struct Asker;
    impl Game for Asker {
        fn start(_: &mut Ctx) -> Self {
            Asker
        }
        fn update(&mut self, ctx: &mut Ctx) -> Flow {
            let now = ctx.video();
            assert_eq!(
                ctx.ask_video(VideoSettings { mode: 3, ..now }),
                Err(Refused::Mode)
            );
            assert_eq!(ctx.set_rate(9), Err(Refused::Rate));
            assert_eq!(
                ctx.ask_video(VideoSettings { hint: 40, ..now }),
                Err(Refused::Hint)
            );
            assert_eq!(
                ctx.ask_video(VideoSettings { colour: 2, ..now }),
                Err(Refused::Colour)
            );
            assert_eq!(ctx.set_rate(rate::FPS60), Ok(()));
            Flow::Run
        }
    }
    game_kit::game!(Asker);
}

#[test]
fn video_requests_are_checked_against_the_offer() {
    let mut c = Console::start(asker::game_entry);
    assert_eq!(c.update(event::START), status::RUN);
    assert_eq!(c.frame.requests, request::VIDEO);
    assert_eq!(c.frame.ask_rate, rate::FPS60);
}
