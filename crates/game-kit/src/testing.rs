//! A stand-in console for a game's own tests (feature `testing`, PC only).
//!
//! [`TestConsole`] calls a game through its `game_entry`, the way the
//! console does: the same tables, the same log buffer size, the start
//! colours, video requests taken or refused, pauses and the on-demand
//! rate. It keeps a record of everything the game hands back, call by
//! call:
//!
//! ```text
//! 12 400 pic 1a2b3c4d5e6f7081        whole picture + palette, when it changed
//! 12 400 log "kestrel: started\n"    the log bytes, exactly as handed over
//! 12 400 ask 0 1 0 0                 a video request (mode rate hint colour)
//! 12 400 end DONE                    the run ended (DONE or ABORTED)
//! ```
//!
//! (call number, game time in ms, what.) [`TestConsole::check_golden`]
//! compares the record with a stored file, or writes it when
//! `UPDATE_GOLDEN` is set, as console-core's golden test does.
//!
//! Use it as a dev-dependency only:
//!
//! ```toml
//! [dev-dependencies]
//! game-kit = { path = "../../crates/game-kit", features = ["testing"] }
//! ```

use std::boxed::Box;
use std::format;
use std::string::{String, ToString};
use std::vec::Vec;

use console_video::Frame;
use game_api::{
    GameCallbacks, GameEntry, GameFrame, Input, PLAYERS, PlayerInput, UNWIND_CONSOLE_API, answer,
    event, input_flag, mode, open_game, picture, player_flag, rate, request, status,
};

/// The console's log buffer for one call (console-core's `LOG_CAP`).
pub const LOG_CAP: usize = 256;
/// Time between calls at 30 and at 60 calls a second (console-core's
/// `PERIOD_30_US`, `PERIOD_60_US`).
pub const PERIOD_30_US: u64 = 33_333;
pub const PERIOD_60_US: u64 = 16_667;

/// What the console offers (console-core's `OFFERED_*`).
const OFFERED_MODES: u32 = 1 << mode::PIXELS_160X120;
const OFFERED_RATES: u32 = (1 << rate::FPS30) | (1 << rate::FPS60) | (1 << rate::ON_DEMAND);
const OFFERED_HINTS: u32 = 0b111;
const OFFERED_COLOURS: u32 = 0b11;

/// A short fingerprint (FNV-1a, 64 bits) of bytes.
pub fn fnv1a(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

const FNV_START: u64 = 0xcbf2_9ce4_8422_2325;

/// A console for tests: one game, driven call by call.
pub struct TestConsole {
    cb: GameCallbacks,
    pixels: Box<Frame>,
    palette: [u16; 256],
    log: [u8; LOG_CAP],
    /// The input the script sets; handed to the game at the next call.
    want: Input,
    /// The input of the last call (for pressed / released / touch flags).
    sent: Input,
    finger: Option<(i16, i16)>,
    finger_was: Option<(i16, i16)>,
    touch_at: (i16, i16),
    /// Console state for the run.
    running: bool,
    events: u32,
    rate: u16,
    vmode: u16,
    hint: u16,
    colour: u16,
    answer: u16,
    wake_at_us: u64,
    /// Game time (pauses not counted) and the time of the last call.
    game_us: u64,
    /// Game time over all runs, while a run was going on.
    played_us: u64,
    last_call_us: u64,
    calls: u32,
    /// The record.
    lines: Vec<String>,
    last_pic: Option<u64>,
    pictures: usize,
    wake_logged: u64,
    ended: Option<u32>,
}

impl TestConsole {
    /// Opens the game and starts its first run (no call yet: the first
    /// call comes with the first [`TestConsole::run_ms`] or
    /// [`TestConsole::step`]). Player 1 has a pad.
    ///
    /// # Safety
    /// `entry` is a game's `game_entry` made by [`crate::game!`].
    pub unsafe fn new(entry: GameEntry) -> TestConsole {
        // SAFETY: the caller's promise.
        let cb = unsafe { open_game(entry, &UNWIND_CONSOLE_API) }.expect("the game opens");
        let mut c = TestConsole {
            cb,
            pixels: Box::new(Frame::new()),
            palette: [0; 256],
            log: [0; LOG_CAP],
            want: Input::new(),
            sent: Input::new(),
            finger: None,
            finger_was: None,
            touch_at: (0, 0),
            running: false,
            events: 0,
            rate: rate::FPS30,
            vmode: mode::PIXELS_160X120,
            hint: 0,
            colour: 0,
            answer: answer::NONE,
            wake_at_us: 0,
            game_us: 0,
            played_us: 0,
            last_call_us: 0,
            calls: 0,
            lines: Vec::new(),
            last_pic: None,
            pictures: 0,
            wake_logged: 0,
            ended: None,
        };
        c.set_pad(0, true);
        c.begin();
        c
    }

    /// A new run, as console-core's `begin`: picture cleared to 0, the
    /// start colours, 30 calls a second, START and REDRAW at the next call.
    pub fn begin(&mut self) {
        self.running = true;
        self.ended = None;
        self.events = event::START | event::REDRAW;
        self.rate = rate::FPS30;
        self.vmode = mode::PIXELS_160X120;
        self.hint = 0;
        self.colour = 0;
        self.answer = answer::NONE;
        self.wake_at_us = 0;
        self.game_us = 0;
        self.last_call_us = 0;
        self.calls = 0;
        self.pixels.clear(0);
        for i in 0..=255u8 {
            self.palette[usize::from(i)] = if u16::from(i) < picture::GAME_COLOURS {
                picture::start_colour(i)
            } else {
                // The console's own colours: fixed stand-ins here.
                0xFFFF
            };
        }
        // Buttons held when the run starts are not "pressed".
        self.sent = self.want;
        self.finger_was = self.finger;
        let n = self.calls;
        self.note(n, "run");
    }

    // ---- Input the script sets ------------------------------------------

    /// Connects or takes away player `p`'s pad.
    pub fn set_pad(&mut self, p: usize, on: bool) {
        let pl = &mut self.want.players[p];
        if on {
            pl.flags |= player_flag::PAD;
        } else {
            *pl = PlayerInput::NONE;
        }
    }

    /// Player `p` holds exactly these buttons from now on.
    pub fn hold_player(&mut self, p: usize, buttons: u32) {
        self.want.players[p].held = buttons;
    }

    /// Player 1 holds exactly these buttons from now on.
    pub fn hold(&mut self, buttons: u32) {
        self.hold_player(0, buttons);
    }

    /// Player `p`'s left stick.
    pub fn left_stick(&mut self, p: usize, x: i16, y: i16) {
        self.want.players[p].left_x = x;
        self.want.players[p].left_y = y;
    }

    /// Player `p`'s right stick.
    pub fn right_stick(&mut self, p: usize, x: i16, y: i16) {
        self.want.players[p].right_x = x;
        self.want.players[p].right_y = y;
    }

    /// Player `p`'s triggers.
    pub fn triggers(&mut self, p: usize, l: u16, r: u16) {
        self.want.players[p].left_trigger = l;
        self.want.players[p].right_trigger = r;
    }

    /// Player `p`'s motion sensors (`None`: the pad sends none).
    pub fn motion(&mut self, p: usize, m: Option<([i16; 3], [i16; 3])>) {
        let pl = &mut self.want.players[p];
        match m {
            Some((a, g)) => {
                pl.flags |= player_flag::MOTION;
                pl.accel = a;
                pl.gyro = g;
            }
            None => {
                pl.flags &= !player_flag::MOTION;
                pl.accel = [0; 3];
                pl.gyro = [0; 3];
            }
        }
    }

    /// A finger on the screen at (x, y) in game pixels, or none.
    pub fn finger(&mut self, at: Option<(i16, i16)>) {
        self.finger = at;
    }

    // ---- Time -------------------------------------------------------------

    /// The time between two console frames at the current rate (at the
    /// on-demand rate the console looks 60 times a second).
    fn period(&self) -> u64 {
        if self.rate == rate::FPS30 {
            PERIOD_30_US
        } else {
            PERIOD_60_US
        }
    }

    /// One console frame: calls the game if it is due (always at 30 and
    /// 60 a second; at the on-demand rate on input changes, its timer,
    /// and after start or resume), then moves time on by one period.
    pub fn step(&mut self) {
        if self.running {
            let input = self.input_now();
            let due = match self.rate {
                rate::ON_DEMAND => {
                    let timer = self.wake_at_us != 0 && self.game_us >= self.wake_at_us;
                    self.events != 0 || timer || changed(&input, &self.sent)
                }
                _ => true,
            };
            if due {
                self.call(input);
            }
        }
        if self.running {
            self.played_us += self.period();
        }
        self.game_us += self.period();
    }

    /// Console frames for at least `ms` milliseconds of game time.
    pub fn run_ms(&mut self, ms: u64) {
        let end = self.game_us + ms * 1000;
        while self.game_us < end {
            self.step();
        }
    }

    /// Holds `buttons` (player 1) for `ms`, then lets go for `ms`.
    pub fn tap_ms(&mut self, buttons: u32, ms: u64) {
        self.hold(buttons);
        self.run_ms(ms);
        self.hold(0);
        self.run_ms(ms);
    }

    /// A short press of `buttons` (player 1): down for 100 ms, up for 100 ms.
    pub fn tap(&mut self, buttons: u32) {
        self.tap_ms(buttons, 100);
    }

    /// A finger down at (x, y) for 100 ms, then up for 100 ms.
    pub fn touch(&mut self, x: i16, y: i16) {
        self.finger(Some((x, y)));
        self.run_ms(100);
        self.finger(None);
        self.run_ms(100);
    }

    /// The pause menu: no calls for `ms` (game time stands still), then
    /// the game is called again with RESUME. Buttons held into the menu
    /// read as not pressed afterwards.
    pub fn pause_ms(&mut self, ms: u64) {
        let n = self.calls;
        self.note(n, &format!("pause {ms}"));
        self.events |= event::RESUME;
        self.sent = self.want;
        self.finger_was = self.finger;
    }

    /// "Back to home" from the pause menu: the game's `exit` with the
    /// player's reason. The run is over.
    pub fn quit(&mut self) {
        self.exit(game_api::exit_reason::PLAYER);
    }

    /// Ends the run with `exit` and this reason.
    pub fn exit(&mut self, reason: u32) {
        if !self.running {
            return;
        }
        let input = self.sent;
        let mut f = self.frame_for(&input, self.last_call_us);
        // SAFETY: the frame points at this console's own buffers, which
        // outlive the call.
        unsafe { self.cb.exit.as_fn()(&mut f, reason) };
        self.running = false;
        let n = self.calls;
        self.after(n, &f);
        self.note(n, &format!("exit {reason}"));
    }

    // ---- What the game left ----------------------------------------------

    /// The game's picture now.
    pub fn picture(&self) -> &Frame {
        &self.pixels
    }

    /// The palette now.
    pub fn palette(&self) -> &[u16; 256] {
        &self.palette
    }

    /// The run's end status (`status::DONE` or `ABORTED`), if it ended.
    pub fn ended(&self) -> Option<u32> {
        self.ended
    }

    /// Whether a run is going on.
    pub fn running(&self) -> bool {
        self.running
    }

    /// Calls in this run.
    pub fn calls(&self) -> u32 {
        self.calls
    }

    /// Game time now, in microseconds.
    pub fn game_us(&self) -> u64 {
        self.game_us
    }

    /// Game time over all runs so far, in microseconds.
    pub fn played_us(&self) -> u64 {
        self.played_us
    }

    /// Different pictures recorded so far (each change counts once).
    pub fn pictures(&self) -> usize {
        self.pictures
    }

    /// The record, one line per entry.
    pub fn record(&self) -> &[String] {
        &self.lines
    }

    /// Every log line the game handed over so far, as text.
    pub fn log_lines(&self) -> Vec<&str> {
        self.lines
            .iter()
            .filter_map(|l| l.split_once(" log ").map(|(_, t)| t))
            .collect()
    }

    /// Fails unless the record holds at least `pictures` different
    /// pictures over at least `ms` of game time (all runs together).
    pub fn require(&self, pictures: usize, ms: u64) {
        assert!(
            self.pictures >= pictures,
            "only {} different pictures, want at least {pictures}",
            self.pictures
        );
        assert!(
            self.played_us >= ms * 1000,
            "only {} ms of game time, want at least {ms}",
            self.played_us / 1000
        );
    }

    /// Compares the record with the stored file `path`, or writes it
    /// when the environment has `UPDATE_GOLDEN`. Fails, showing the first
    /// line that differs, when they are not the same.
    pub fn check_golden(&self, path: &str, header: &str) {
        let mut now = String::new();
        for l in header.lines() {
            now.push_str("# ");
            now.push_str(l);
            now.push('\n');
        }
        for l in &self.lines {
            now.push_str(l);
            now.push('\n');
        }
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            if let Some(dir) = std::path::Path::new(path).parent() {
                std::fs::create_dir_all(dir).unwrap();
            }
            std::fs::write(path, &now).unwrap();
            std::eprintln!("wrote {path}");
            return;
        }
        let stored = std::fs::read_to_string(path)
            .unwrap_or_else(|_| panic!("no golden file {path}: run with UPDATE_GOLDEN=1 once"));
        if now == stored {
            return;
        }
        let (a, b): (Vec<&str>, Vec<&str>) = (stored.lines().collect(), now.lines().collect());
        let i = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
        panic!(
            "the record differs from {path} at line {}:\n  stored: {}\n  now:    {}\n\
             (stored {} lines, now {}). If the change is on purpose, look at the \
             pictures first, then run with UPDATE_GOLDEN=1.",
            i + 1,
            a.get(i).copied().unwrap_or("(end)"),
            b.get(i).copied().unwrap_or("(end)"),
            a.len(),
            b.len()
        );
    }

    /// A named place in the script: a `mark` line in the record. With
    /// `GOLDEN_SHOTS=<folder>` in the environment the picture is also
    /// written there as `<prefix>-<n>-<name>.png`, to look at.
    pub fn mark(&mut self, prefix: &str, name: &str) {
        let n = self.calls;
        self.note(n, &format!("mark {name}"));
        if let Some(dir) = std::env::var_os("GOLDEN_SHOTS") {
            let k = self.lines.iter().filter(|l| l.contains(" mark ")).count();
            let path = std::path::Path::new(&dir).join(format!("{prefix}-{k:02}-{name}.png"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(path, self.png()).unwrap();
        }
    }

    /// The picture as a PNG file (8-bit RGB, stored without compression).
    pub fn png(&self) -> Vec<u8> {
        let (w, h) = (console_video::W, console_video::H);
        let mut raw = Vec::with_capacity((w * 3 + 1) * h);
        for y in 0..h {
            raw.push(0);
            for x in 0..w {
                let c = self.palette[usize::from(self.pixels.0[y * w + x])];
                let (r, g, b) = ((c >> 11) & 31, (c >> 5) & 63, c & 31);
                raw.extend([
                    (r * 255 / 31) as u8,
                    (g * 255 / 63) as u8,
                    (b * 255 / 31) as u8,
                ]);
            }
        }
        // zlib with stored blocks.
        let mut z = std::vec![0x78, 0x01];
        let chunks: Vec<&[u8]> = raw.chunks(65_535).collect();
        for (i, ch) in chunks.iter().enumerate() {
            z.push(u8::from(i + 1 == chunks.len()));
            let n = ch.len() as u16;
            z.extend(n.to_le_bytes());
            z.extend((!n).to_le_bytes());
            z.extend_from_slice(ch);
        }
        let (mut a, mut b) = (1u32, 0u32);
        for &x in &raw {
            a = (a + u32::from(x)) % 65_521;
            b = (b + a) % 65_521;
        }
        z.extend(((b << 16) | a).to_be_bytes());
        let mut out = std::vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let mut ihdr = Vec::new();
        ihdr.extend((w as u32).to_be_bytes());
        ihdr.extend((h as u32).to_be_bytes());
        ihdr.extend([8, 2, 0, 0, 0]);
        for (kind, data) in [(b"IHDR", ihdr), (b"IDAT", z), (b"IEND", Vec::new())] {
            out.extend((data.len() as u32).to_be_bytes());
            let mut body = kind.to_vec();
            body.extend(&data);
            out.extend(&body);
            out.extend(crc32(&body).to_be_bytes());
        }
        out
    }

    // ---- Inside -------------------------------------------------------------

    /// The input for a call now: held from the script, pressed and
    /// released against the last call, touch flags against the last call.
    fn input_now(&mut self) -> Input {
        let mut i = self.want;
        for p in 0..PLAYERS {
            let (now, was) = (i.players[p].held, self.sent.players[p].held);
            i.players[p].pressed = now & !was;
            i.players[p].released = was & !now;
        }
        let mut fl = 0;
        match (self.finger, self.finger_was) {
            (Some(at), was) => {
                fl |= input_flag::TOUCH;
                if was.is_none() {
                    fl |= input_flag::TOUCH_START;
                }
                self.touch_at = at;
            }
            (None, Some(_)) => fl |= input_flag::TOUCH_END,
            (None, None) => {}
        }
        i.flags = fl;
        if fl != 0 {
            (i.touch_x, i.touch_y) = self.touch_at;
        }
        i
    }

    fn frame_for(&mut self, input: &Input, now: u64) -> GameFrame {
        GameFrame {
            time_us: now,
            frame: self.calls,
            delta_us: now
                .saturating_sub(self.last_call_us)
                .min(u64::from(u32::MAX)) as u32,
            events: self.events,
            offered_modes: OFFERED_MODES,
            offered_rates: OFFERED_RATES,
            offered_hints: OFFERED_HINTS,
            offered_colours: OFFERED_COLOURS,
            game_colours: picture::GAME_COLOURS,
            mode: self.vmode,
            rate: self.rate,
            hint: self.hint,
            colour: self.colour,
            answer: self.answer,
            pixels: self.pixels.0.as_mut_ptr(),
            palette: self.palette.as_mut_ptr(),
            input,
            log: self.log.as_mut_ptr(),
            log_cap: LOG_CAP as u16,
            log_len: 0,
            width: picture::W,
            height: picture::H,
            stride: picture::W,
            ..GameFrame::new()
        }
    }

    fn call(&mut self, input: Input) {
        let now = self.game_us;
        let mut f = self.frame_for(&input, now);
        // SAFETY: the frame points at this console's own buffers and at
        // `input`, all alive for the call.
        let st = unsafe { self.cb.update.as_fn()(&mut f) };
        let n = self.calls;
        self.events = 0;
        self.calls = self.calls.wrapping_add(1);
        self.last_call_us = now;
        self.wake_at_us = f.wake_at_us;
        self.sent = input;
        self.finger_was = self.finger;
        self.after(n, &f);
        if f.requests & request::VIDEO != 0 {
            self.answer = self.take_video(&f);
            self.note(
                n,
                &format!(
                    "ask {} {} {} {} answer {}",
                    f.ask_mode, f.ask_rate, f.ask_hint, f.ask_colour, self.answer
                ),
            );
        }
        if f.wake_at_us != self.wake_logged {
            self.wake_logged = f.wake_at_us;
            self.note(n, &format!("wake {}", f.wake_at_us));
        }
        if st != status::RUN {
            self.running = false;
            self.ended = Some(st);
            let what = if st == status::DONE {
                "DONE"
            } else {
                "ABORTED"
            };
            self.note(n, &format!("end {what}"));
        }
    }

    /// The log and the picture after a call.
    fn after(&mut self, n: u32, f: &GameFrame) {
        let len = usize::from(f.log_len).min(LOG_CAP);
        if len > 0 {
            let text = self.log[..len].escape_ascii().to_string();
            self.note(n, &format!("log \"{text}\""));
        }
        // The console's colours stay the console's.
        for c in &mut self.palette[usize::from(picture::GAME_COLOURS)..] {
            *c = 0xFFFF;
        }
        let mut h = fnv1a(FNV_START, &self.pixels.0);
        for c in self.palette {
            h = fnv1a(h, &c.to_le_bytes());
        }
        if self.last_pic != Some(h) {
            self.last_pic = Some(h);
            self.pictures += 1;
            self.note(n, &format!("pic {h:016x}"));
        }
    }

    fn take_video(&mut self, f: &GameFrame) -> u16 {
        let has = |bits: u32, v: u16| v < 32 && bits & (1 << v) != 0;
        if !has(OFFERED_MODES, f.ask_mode) {
            return answer::NO_MODE;
        }
        if !has(OFFERED_RATES, f.ask_rate) {
            return answer::NO_RATE;
        }
        if !has(OFFERED_HINTS, f.ask_hint) {
            return answer::NO_HINT;
        }
        if !has(OFFERED_COLOURS, f.ask_colour) {
            return answer::NO_COLOUR;
        }
        self.vmode = f.ask_mode;
        self.rate = f.ask_rate;
        self.hint = f.ask_hint;
        self.colour = f.ask_colour;
        answer::OK
    }

    fn note(&mut self, call: u32, what: &str) {
        self.lines
            .push(format!("{call} {} {what}", self.game_us / 1000));
    }
}

/// Whether an on-demand console would call: buttons, pads, sticks,
/// triggers or the touch changed (motion alone does not wake the game).
fn changed(now: &Input, was: &Input) -> bool {
    if now.flags & (input_flag::TOUCH_START | input_flag::TOUCH_END) != 0
        || (now.touch_x, now.touch_y) != (was.touch_x, was.touch_y)
    {
        return true;
    }
    now.players.iter().zip(&was.players).any(|(a, b)| {
        a.held != b.held
            || a.pressed != 0
            || a.released != 0
            || (a.flags & player_flag::PAD) != (b.flags & player_flag::PAD)
            || (a.left_x, a.left_y, a.right_x, a.right_y)
                != (b.left_x, b.left_y, b.right_x, b.right_y)
            || (a.left_trigger, a.right_trigger) != (b.left_trigger, b.right_trigger)
    })
}

/// CRC-32 (the PNG one).
fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}
