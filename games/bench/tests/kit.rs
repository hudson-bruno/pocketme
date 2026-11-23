//! BENCH through its `game_entry`, as a console calls it: a tiny
//! stand-in console (the game kit's own test harness, cut down).

use bench::art::PALETTE;
use game_kit::api::{
    self, GameCallbacks, GameFrame, Input, UNWIND_CONSOLE_API, event, request, status,
};
use game_kit::{Frame, button, rate};

struct Console {
    cb: GameCallbacks,
    frame: GameFrame,
    pixels: Box<Frame>,
    palette: [u16; 256],
    input: Input,
    log: [u8; 256],
}

impl Console {
    fn start() -> Console {
        // SAFETY: `game_entry` is made by `game!`.
        let cb =
            unsafe { api::open_game(bench::game_entry, &UNWIND_CONSOLE_API) }.expect("game opens");
        let mut c = Console {
            cb,
            frame: GameFrame::new(),
            pixels: Box::new(Frame::new()),
            palette: [0; 256],
            input: Input::new(),
            log: [0; 256],
        };
        c.frame.rate = rate::FPS30;
        c
    }

    fn update(&mut self, events: u32, pressed: u32) -> u32 {
        self.input.players[0].pressed = pressed;
        let f = &mut self.frame;
        f.pixels = self.pixels.0.as_mut_ptr();
        f.palette = self.palette.as_mut_ptr();
        f.input = &self.input;
        f.log = self.log.as_mut_ptr();
        f.log_cap = self.log.len() as u16;
        f.log_len = 0;
        f.requests = 0;
        f.events = events;
        f.width = 160;
        f.height = 120;
        f.stride = 160;
        f.game_colours = 240;
        f.offered_modes = 1;
        f.offered_rates = 0b111;
        f.offered_hints = 0b111;
        f.offered_colours = 0b11;
        f.delta_us = if f.rate == rate::FPS60 {
            16_667
        } else {
            33_333
        };
        // SAFETY: the frame points at this console's own buffers.
        let st = unsafe { self.cb.update.as_fn()(&mut self.frame) };
        self.frame.frame += 1;
        // The console takes a rate it was asked for.
        if self.frame.requests & request::VIDEO != 0 {
            self.frame.rate = self.frame.ask_rate;
        }
        st
    }

    fn log_text(&self) -> String {
        String::from_utf8_lossy(&self.log[..usize::from(self.frame.log_len)]).into_owned()
    }
}

#[test]
fn the_game_starts_sets_its_colours_and_draws() {
    let mut c = Console::start();
    assert_eq!(c.cb.version, api::API_VERSION);
    assert_eq!(c.update(event::START | event::REDRAW, 0), status::RUN);
    assert!(
        c.log_text().starts_with("bench: started\n"),
        "{}",
        c.log_text()
    );
    assert_eq!(&c.palette[..PALETTE.len()], &PALETTE[..]);
    assert_ne!(c.frame.requests & request::PALETTE, 0);
    assert_eq!(c.frame.requests & request::VIDEO, 0, "30 is the start rate");
    assert_eq!(c.pixels.get(0, 0), bench::art::BAND);
}

#[test]
fn x_asks_for_60_and_a_starts_the_ramp_at_60() {
    let mut c = Console::start();
    c.update(event::START | event::REDRAW, 0);
    c.update(0, button::X);
    assert_ne!(c.frame.requests & request::VIDEO, 0);
    assert_eq!(c.frame.ask_rate, rate::FPS60);
    assert_eq!(c.frame.rate, rate::FPS60);
    c.update(0, button::X);
    assert_eq!(c.frame.rate, rate::FPS30);
    c.update(0, button::A);
    assert!(c.log_text().contains("bench: ramp FILL starts"));
    assert_eq!(c.frame.rate, rate::FPS60, "the ramp starts at 60");
    for _ in 0..10 {
        assert_eq!(c.update(0, 0), status::RUN);
        assert!(usize::from(c.frame.log_len) < c.log.len());
    }
    c.update(0, button::B);
    assert!(c.log_text().contains("bench: ramp stopped"));
    c.update(0, 0);
    assert_eq!(c.frame.rate, rate::FPS30);
}

#[test]
fn start_leaves() {
    let mut c = Console::start();
    c.update(event::START | event::REDRAW, 0);
    assert_eq!(c.update(0, button::START), status::DONE);
    assert_eq!(c.log_text(), "bench: leave\n");
}

#[test]
fn a_new_run_starts_fresh() {
    let mut c = Console::start();
    c.update(event::START | event::REDRAW, 0);
    c.update(0, button::RIGHT);
    c.update(0, button::START);
    c.pixels.clear(0);
    c.update(event::START | event::REDRAW, 0);
    assert!(c.log_text().contains("bench: started"));
    // FILL again: its band and its fills.
    assert_eq!(c.pixels.get(0, 0), bench::art::BAND);
    assert_eq!(c.pixels.get(80, 60), bench::art::FILL0 + 3);
}

#[test]
fn the_game_state_is_small() {
    // Without the PC-only `check` feature (its recorder is about 180 KB).
    if cfg!(feature = "check") {
        return;
    }
    let n = core::mem::size_of::<bench::Bench>();
    println!("Bench: {n} bytes");
    assert!(n <= 10_000, "{n} bytes on the console's stack in start()");
}
