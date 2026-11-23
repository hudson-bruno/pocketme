//! The measuring recorder (feature `rec`, OFF in the normal build).
//!
//! Built so the normal game is not touched at all: with the feature the
//! console runs [`RecRacer`], which wraps the game ([`Racer`]) and
//! watches it from outside after every call; `app.rs`, `hud.rs` and the
//! rest are the same files either way.
//!
//! What the race reads: each tick (1/60 s) [`Race::step`] takes one
//! [`Ctl`], made by [`crate::app::control`] from player 1's keys of that
//! call (stick x, d-pad left/right, A, B, L, R, the triggers past their
//! thresholds, a finger); every tick of one call gets the same one.
//! Players 2 to 4 are never read. A race is [`Race::setup`] (track,
//! class, mode, seed) followed by those `Ctl`s, so this records exactly
//! them, tick by tick, from the race's first tick to the end of the call
//! in which the player's ship crosses the line for the last time (a call
//! may run a tick or more past that tick; the race no longer reads the
//! player's `Ctl` then, but the ticks are recorded so the replay ends
//! where the console's race was when it was checked). Nothing is
//! simplified: all 513 steering values, each air brake on its own, all
//! five carve states.
//!
//! Storage: runs of ticks with the same `Ctl` in a fixed buffer
//! ([`CAP`] bytes, a static outside the game's state so the state built
//! by `game_kit::rt::restart` stays small). A run is
//!
//! * `0dddddd c` (one byte): steering changed by `d` (-32..=31), nothing
//!   else changed, `c + 1` ticks (1 or 2);
//! * `1s ffffff` [steering change, zigzag LEB128, when `s`] [ticks,
//!   LEB128]: `ffffff` is thrust (bit 0), left brake (1), right brake
//!   (2), carve (3..5: 0 off, 1 left, 2 right, 3 both, 4 keep); without
//!   `s` the steering is the last run's.
//!
//! A short run longer than 2 ticks is a short byte followed by a long
//! run without `s` (the same controls). The first run is measured
//! against the default `Ctl` (no buttons, steering 0). LEB128: 7 bits a
//! byte, lowest first, the top bit set on every byte but the last;
//! zigzag: 0, -1, 1, -2, 2 ... as 0, 1, 2, 3, 4 ...
//!
//! When the race ends, and again on the results screen when L and R are
//! held together, the recording is sent as log lines (at most one line
//! per call and one per [`LINE_US`] of game time):
//!
//! ```text
//! camber: rec H <id> <ver> <track> <class> <mode> <catchup> <rules> <seed> <steps> <len> <cut> <check> <crc> <lines>
//! camber: rec D <id> <seq> <base64url of up to 42 bytes, no padding>
//! camber: rec E <id> <lines> <crc> <finish> <place>
//! ```
//!
//! All numbers are lower-case hex (fixed widths), except `ver`, `track`,
//! `class`, `mode`, `catchup`, `rules`, `cut` and `place` (one decimal
//! digit). `catchup` is the race's [`crate::race::CatchUp`] (0 OFF,
//! 1 LIGHT) and `rules` the [`RULES`] the game was built with. `crc` is the
//! CRC-32 (IEEE) of the data bytes followed by [`Head::meta`]; `check`
//! is [`race_check`] of the race after the last recorded tick, so the
//! PC can tell that its replay gives the race the console had. Every
//! line is at most 80 characters (93 with the console's `game: ` and the
//! firmware's `INFO - ` before it: under the 96 a line may have).
//!
//! The send goes on only while the game is called: on the results
//! screen that is every picture, on the title and in the pause menu
//! the console may call the game only on input, so the send should be
//! left to end on the results (the sign there says SENT). A new race
//! (also RESTART and AGAIN) begins a new recording and stops a send that
//! is going on; a race left for the title before the finish is never
//! sent. Nothing is kept anywhere but in RAM.

use core::sync::atomic::{AtomicU8, Ordering};

use game_kit::{Ctx, Flow, Frame, Game, button};

use crate::app::{self, App, END_BOX, END_Y, Keys, LINE, Scene};
use crate::art::{SEL, TEXT_DIM};
use crate::hud;
use crate::race::{Carve, CatchUp, Class, Ctl, Mode, Phase, Race};
use crate::{Racer, keys_of};

/// The format's version (in every header). 2: the header got the
/// catch-up and the rules digits (2026-11-22); version 1 had neither.
pub const VERSION: u8 = 2;
/// The race's rules: what the rivals and the catch-up do. A replay is the
/// console's race only with the same rules. 1: the rivals up to
/// 2026-11-22 (no catch-up); 2: the rebuilt rivals with the catch-up.
pub const RULES: u8 = 2;
/// The buffer: at most this many bytes of runs.
pub const CAP: usize = 16_384;
/// Bytes on one data line (56 characters of base64).
pub const LINE_BYTES: usize = 42;
/// At most one line per this much game time (and one per call).
pub const LINE_US: u32 = 25_000;
/// What every line of the recording starts with.
pub const TAG: &str = "camber: rec ";
/// The most bytes one run takes (long form, 2 bytes of steering, 5 of
/// ticks).
pub const RUN_MAX: usize = 8;

/// The recording's bytes. Atomics only to have a mutable static without
/// `unsafe`; only one game runs at a time on the chip. On the PC every
/// test thread in one test program shares it (tests/rec.rs takes a
/// lock).
static BUF: [AtomicU8; CAP] = [const { AtomicU8::new(0) }; CAP];

fn put(i: usize, b: u8) {
    BUF[i].store(b, Ordering::Relaxed);
}

/// Byte `i` of the buffer.
pub fn byte(i: usize) -> u8 {
    BUF[i].load(Ordering::Relaxed)
}

// ---- CRC-32 (IEEE, the zip one) ---------------------------------------------------

/// A running CRC-32: `Crc::new()`, `add`, `done`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crc(u32);

impl Crc {
    pub const fn new() -> Crc {
        Crc(0xFFFF_FFFF)
    }

    pub fn add(&mut self, b: u8) {
        let mut c = self.0 ^ u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
        self.0 = c;
    }

    pub fn bytes(&mut self, b: &[u8]) {
        for &x in b {
            self.add(x);
        }
    }

    pub fn done(&self) -> u32 {
        !self.0
    }
}

impl Default for Crc {
    fn default() -> Self {
        Crc::new()
    }
}

// ---- Controls as bytes -------------------------------------------------------------

pub fn carve_code(c: Carve) -> u8 {
    match c {
        Carve::Off => 0,
        Carve::Left => 1,
        Carve::Right => 2,
        Carve::Both => 3,
        Carve::Keep => 4,
    }
}

pub fn carve_of(code: u8) -> Option<Carve> {
    Some(match code {
        0 => Carve::Off,
        1 => Carve::Left,
        2 => Carve::Right,
        3 => Carve::Both,
        4 => Carve::Keep,
        _ => return None,
    })
}

/// Everything of a `Ctl` but the steering, in six bits.
pub fn flags(c: &Ctl) -> u8 {
    u8::from(c.thrust)
        | (u8::from(c.brake_l) << 1)
        | (u8::from(c.brake_r) << 2)
        | (carve_code(c.carve) << 3)
}

/// Writes `v` as LEB128 (7 bits a byte, low first) at `out[k..]`; the
/// index after it.
fn leb(mut v: u32, out: &mut [u8; RUN_MAX], mut k: usize) -> usize {
    loop {
        let b = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out[k] = b;
            return k + 1;
        }
        out[k] = b | 0x80;
        k += 1;
    }
}

/// One run (`n` ticks of `c`, after a run of `prev`) as bytes; the
/// count of bytes written.
pub fn encode_run(prev: &Ctl, c: &Ctl, n: u32, out: &mut [u8; RUN_MAX]) -> usize {
    // Steering is -256..=256 (the clamp only guards the format).
    let d = c.steer.clamp(-256, 256) - prev.steer.clamp(-256, 256);
    if flags(prev) == flags(c) && (-32..=31).contains(&d) {
        let short = n.clamp(1, 2);
        out[0] = (((d as u8) & 0x3F) << 1) | (short - 1) as u8;
        if n == short {
            return 1;
        }
        // More of the same: a long run without steering.
        out[1] = 0x80 | flags(c);
        return leb(n - short, out, 2);
    }
    let s = if d != 0 { 0x40 } else { 0 };
    out[0] = 0x80 | s | flags(c);
    let mut k = 1;
    if d != 0 {
        // Zigzag: 0, -1, 1, -2, ... as 0, 1, 2, 3, ...
        let z = ((d << 1) ^ (d >> 31)) as u32;
        k = leb(z, out, k);
    }
    leb(n, out, k)
}

/// What is wrong with a recording's bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bad {
    /// A carve code above 4, at this byte.
    Carve(usize),
    /// The bytes end inside a run, at this byte.
    Short(usize),
    /// A run of 0 ticks or a number longer than 5 bytes, at this byte.
    Count(usize),
    /// A steering change of 0 or steering past -256..=256, at this byte.
    Steer(usize),
}

/// The runs in `data`: `(ctl, ticks)`, or what is wrong.
pub struct Runs<'a> {
    data: &'a [u8],
    at: usize,
    prev: Ctl,
    failed: bool,
}

impl<'a> Runs<'a> {
    pub fn new(data: &'a [u8]) -> Runs<'a> {
        Runs {
            data,
            at: 0,
            prev: Ctl::default(),
            failed: false,
        }
    }

    fn one(&mut self) -> Result<(Ctl, u32), Bad> {
        let start = self.at;
        let b = self.data[start];
        self.at += 1;
        if b & 0x80 == 0 {
            // Six bits of signed steering change.
            let d = i32::from(((b >> 1) as i8) << 2 >> 2);
            let mut c = self.prev;
            c.steer += d;
            return Ok((c, u32::from(b & 1) + 1));
        }
        let f = b & 0x3F;
        let carve = carve_of(f >> 3).ok_or(Bad::Carve(start))?;
        let mut c = Ctl {
            steer: self.prev.steer,
            thrust: f & 1 != 0,
            brake_l: f & 2 != 0,
            brake_r: f & 4 != 0,
            carve,
        };
        if b & 0x40 != 0 {
            let z = self.leb(start)?;
            let d = (z >> 1) as i32 ^ -((z & 1) as i32);
            c.steer += d;
            if z == 0 || !(-256..=256).contains(&c.steer) {
                return Err(Bad::Steer(start));
            }
        }
        let n = self.leb(start)?;
        if n == 0 {
            return Err(Bad::Count(start));
        }
        Ok((c, n))
    }

    /// One LEB128 number (at most 5 bytes, fitting 32 bits).
    fn leb(&mut self, start: usize) -> Result<u32, Bad> {
        let mut n: u64 = 0;
        let mut shift = 0;
        loop {
            let Some(&x) = self.data.get(self.at) else {
                return Err(Bad::Short(start));
            };
            self.at += 1;
            n |= u64::from(x & 0x7F) << shift;
            if x & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 28 {
                return Err(Bad::Count(start));
            }
        }
        u32::try_from(n).map_err(|_| Bad::Count(start))
    }
}

impl Iterator for Runs<'_> {
    type Item = Result<(Ctl, u32), Bad>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.at >= self.data.len() {
            return None;
        }
        let r = self.one();
        match r {
            Ok((c, _)) => self.prev = c,
            Err(_) => self.failed = true,
        }
        Some(r)
    }
}

// ---- The header ----------------------------------------------------------------------

/// What a recording is (the header line's numbers).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Head {
    pub version: u8,
    /// Recordings started since the game started (tells sends apart).
    pub id: u16,
    pub track: u8,
    /// 0 STEADY, 1 BRISK, 2 WILD.
    pub class: u8,
    /// 0 RACE, 1 TIME TRIAL.
    pub mode: u8,
    /// 0 OFF, 1 LIGHT ([`crate::race::CatchUp`]).
    pub catchup: u8,
    /// The [`RULES`] the race was run by.
    pub rules: u8,
    /// The seed of [`Race::setup`].
    pub seed: u32,
    /// Ticks recorded (from the race's first tick).
    pub steps: u32,
    /// Bytes of runs.
    pub len: u32,
    /// The buffer filled: `steps` stop before the finish.
    pub cut: bool,
    /// [`race_check`] right after the finishing tick.
    pub check: u32,
    /// CRC-32 of the bytes and [`Head::meta`].
    pub crc: u32,
}

impl Head {
    /// The numbers the CRC covers after the bytes (23 bytes, little
    /// endian): version, track, class, mode, catchup, rules, seed, steps,
    /// len, cut, check.
    pub fn meta(&self) -> [u8; 23] {
        let mut m = [0u8; 23];
        m[0] = self.version;
        m[1] = self.track;
        m[2] = self.class;
        m[3] = self.mode;
        m[4] = self.catchup;
        m[5] = self.rules;
        m[6..10].copy_from_slice(&self.seed.to_le_bytes());
        m[10..14].copy_from_slice(&self.steps.to_le_bytes());
        m[14..18].copy_from_slice(&self.len.to_le_bytes());
        m[18] = u8::from(self.cut);
        m[19..23].copy_from_slice(&self.check.to_le_bytes());
        m
    }

    /// Data lines of the recording.
    pub fn lines(&self) -> u32 {
        self.len.div_ceil(LINE_BYTES as u32)
    }

    pub fn class(&self) -> Option<Class> {
        Class::ALL.get(usize::from(self.class)).copied()
    }

    pub fn mode(&self) -> Option<Mode> {
        match self.mode {
            0 => Some(Mode::Race),
            1 => Some(Mode::Trial),
            _ => None,
        }
    }

    pub fn catchup(&self) -> Option<CatchUp> {
        match self.catchup {
            0 => Some(CatchUp::Off),
            1 => Some(CatchUp::Light),
            _ => None,
        }
    }
}

pub fn class_code(c: Class) -> u8 {
    match c {
        Class::Steady => 0,
        Class::Brisk => 1,
        Class::Wild => 2,
    }
}

pub fn mode_code(m: Mode) -> u8 {
    match m {
        Mode::Race => 0,
        Mode::Trial => 1,
    }
}

pub fn catchup_code(c: CatchUp) -> u8 {
    match c {
        CatchUp::Off => 0,
        CatchUp::Light => 1,
    }
}

/// A fingerprint of a race (CRC-32 of every ship's state, the clock,
/// the order and the places; not the events waiting to be taken).
pub fn race_check(r: &Race) -> u32 {
    let mut c = Crc::new();
    let n = usize::from(r.n).min(r.ships.len());
    c.bytes(&r.tick.to_le_bytes());
    c.add(match r.phase {
        Phase::Count => 0,
        Phase::Run => 1,
        Phase::Done => 2,
    });
    c.add(r.n);
    c.add(r.placed);
    c.bytes(&r.order[..n]);
    for sh in &r.ships[..n] {
        for v in [
            sh.s,
            sh.x,
            sh.h,
            sh.vf,
            sh.vl,
            sh.vh,
            sh.psi,
            sh.rate,
            sh.pitch,
            sh.safe,
            sh.heat,
            sh.lean,
            sh.ctl.steer,
            i32::from(sh.lap),
            i32::from(sh.counted),
            i32::from(sh.shield),
            i32::from(sh.pace),
            i32::from(sh.lane),
            i32::from(sh.carve),
        ] {
            c.bytes(&v.to_le_bytes());
        }
        for v in [sh.lap_start, sh.best, sh.finish] {
            c.bytes(&v.to_le_bytes());
        }
        c.bytes(&[
            u8::from(sh.limp),
            sh.boost,
            sh.fall,
            sh.ghost,
            sh.hit,
            sh.sling,
            sh.sling_step,
            sh.air,
            sh.held,
            sh.settle,
            sh.rest,
            flags(&sh.ctl),
        ]);
    }
    c.done()
}

// ---- Text --------------------------------------------------------------------------

/// One log line built in place (at most 88 bytes).
pub struct Text {
    buf: [u8; 88],
    len: usize,
}

impl Text {
    pub const fn new() -> Text {
        Text {
            buf: [0; 88],
            len: 0,
        }
    }

    fn byte(&mut self, b: u8) {
        if self.len < self.buf.len() {
            self.buf[self.len] = b;
            self.len += 1;
        }
    }

    pub fn push(&mut self, s: &str) -> &mut Text {
        for &b in s.as_bytes() {
            self.byte(b);
        }
        self
    }

    /// `v` in lower-case hex, `digits` wide, and a space before it.
    pub fn hex(&mut self, v: u32, digits: u32) -> &mut Text {
        self.byte(b' ');
        for k in (0..digits).rev() {
            let d = ((u64::from(v) >> (4 * k)) & 15) as u8;
            self.byte(if d < 10 { b'0' + d } else { b'a' + d - 10 });
        }
        self
    }

    /// One decimal digit (0..=9) and a space before it.
    pub fn digit(&mut self, v: u8) -> &mut Text {
        self.byte(b' ');
        self.byte(b'0' + v.min(9));
        self
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for Text {
    fn default() -> Self {
        Text::new()
    }
}

/// The base64url alphabet (letters, digits, `-` and `_`: nothing the
/// console's log or a terminal changes).
pub const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// The value of a base64url character.
pub fn b64_value(c: u8) -> Option<u8> {
    B64.iter().position(|&x| x == c).map(|p| p as u8)
}

// ---- The recorder ------------------------------------------------------------------

/// Where a recording is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// No race recorded yet.
    Empty,
    /// A race is going on.
    Taking,
    /// The player finished: it can be sent.
    Done,
}

/// What the results screen shows about the recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sign {
    pub cut: bool,
    /// Percent sent while a send goes on.
    pub sending: Option<u8>,
    /// Sends finished.
    pub sends: u16,
}

pub struct Rec {
    pub stage: Stage,
    pub head: Head,
    crc: Crc,
    /// The last run written, and the run still being counted.
    prev: Ctl,
    cur: Ctl,
    n: u32,
    /// The player's finish (ticks) and place.
    pub finish: u32,
    pub place: u8,
    /// The next line to send: 0 the header, 1..=lines the data, then the
    /// end line.
    line: u32,
    sending: bool,
    pub sends: u16,
    /// Game time saved up towards the next line (at most [`LINE_US`]).
    wait: u32,
    /// A race at its first tick was seen (it is begun once).
    fresh: bool,
}

/// What the game was before a call (for [`Rec::after`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Before {
    pub tick: u32,
    pub frames: u32,
}

impl Before {
    pub fn of(app: &App) -> Before {
        Before {
            tick: app.race.tick,
            frames: app.frames,
        }
    }
}

impl Rec {
    pub const fn new() -> Rec {
        Rec {
            stage: Stage::Empty,
            head: Head {
                version: VERSION,
                id: 0,
                track: 0,
                class: 0,
                mode: 0,
                catchup: 0,
                rules: RULES,
                seed: 0,
                steps: 0,
                len: 0,
                cut: false,
                check: 0,
                crc: 0,
            },
            crc: Crc::new(),
            prev: Ctl {
                steer: 0,
                thrust: false,
                brake_l: false,
                brake_r: false,
                carve: Carve::Off,
            },
            cur: Ctl {
                steer: 0,
                thrust: false,
                brake_l: false,
                brake_r: false,
                carve: Carve::Off,
            },
            n: 0,
            finish: 0,
            place: 0,
            line: 0,
            sending: false,
            sends: 0,
            wait: 0,
            fresh: false,
        }
    }

    /// After one call of the game (`b` taken before it, `k` its keys):
    /// begins a recording when a race was just set up, records the ticks
    /// the call ran, ends the recording when the player has finished,
    /// and on the results sends it again for L and R held together
    /// (nothing else on that screen reads L or R).
    pub fn after(&mut self, b: Before, app: &App, k: &Keys) {
        let race = &app.race;
        if app.scene != Scene::Race {
            // Off the race (the title): the next race at tick 0 is new,
            // even one whose start was seen at tick 0 and left at once.
            self.fresh = false;
            return;
        }
        // `App::start` leaves a new race at tick 0, and only it does.
        if race.tick == 0 {
            if !self.fresh {
                self.begin(race.track, race.class, race.mode, b.frames);
                self.head.catchup = catchup_code(race.catchup);
                self.fresh = true;
            }
            return;
        }
        self.fresh = false;
        if self.stage == Stage::Done {
            let lr = button::L | button::R;
            if app.results() && k.held & lr == lr && k.pressed & lr != 0 {
                self.resend();
            }
            return;
        }
        if self.stage != Stage::Taking || race.tick <= b.tick {
            return;
        }
        // Before the finish every tick of a call takes the call's
        // controls (`App::racing`).
        let ctl = app::control(k);
        for _ in b.tick..race.tick {
            self.push(ctl);
        }
        if race.phase == Phase::Done {
            self.finish(race);
        }
    }

    /// A race starts (just after [`Race::setup`] with these numbers): a
    /// new recording; the last one is gone (a send going on stops).
    pub fn begin(&mut self, track: u8, class: Class, mode: Mode, seed: u32) {
        let id = self.head.id.wrapping_add(1);
        *self = Rec {
            wait: self.wait,
            fresh: self.fresh,
            ..Rec::new()
        };
        self.stage = Stage::Taking;
        self.head.id = id;
        self.head.track = track;
        self.head.class = class_code(class);
        self.head.mode = mode_code(mode);
        self.head.seed = seed;
    }

    /// The controls of one tick, before the race takes it.
    pub fn push(&mut self, c: Ctl) {
        if self.stage != Stage::Taking || self.head.cut {
            return;
        }
        if self.n > 0 && c == self.cur && self.n < u32::MAX {
            self.n += 1;
            return;
        }
        self.flush();
        if !self.head.cut {
            self.cur = c;
            self.n = 1;
        }
    }

    /// Writes the run being counted; marks the recording cut when it
    /// does not fit.
    fn flush(&mut self) {
        if self.n == 0 {
            return;
        }
        let len = self.head.len as usize;
        if len + RUN_MAX > CAP {
            self.head.cut = true;
            self.n = 0;
            return;
        }
        let mut b = [0u8; RUN_MAX];
        let k = encode_run(&self.prev, &self.cur, self.n, &mut b);
        for (i, &x) in b[..k].iter().enumerate() {
            put(len + i, x);
            self.crc.add(x);
        }
        self.head.len += k as u32;
        self.head.steps += self.n;
        self.prev = self.cur;
        self.n = 0;
    }

    /// The player's ship has just finished (`race` right after that
    /// tick): the recording is complete and its first send starts.
    pub fn finish(&mut self, race: &Race) {
        if self.stage != Stage::Taking {
            return;
        }
        self.flush();
        self.head.check = race_check(race);
        let mut crc = self.crc;
        crc.bytes(&self.head.meta());
        self.head.crc = crc.done();
        let f = race.ships[0].finish;
        self.finish = f;
        // The place the race gave: ships that finished on an earlier
        // tick are ahead; on the same tick ship 0 is counted first.
        let n = usize::from(race.n).min(race.ships.len());
        let ahead = race.ships[1..n]
            .iter()
            .filter(|s| s.finished() && s.finish < f)
            .count();
        self.place = ahead as u8 + 1;
        self.stage = Stage::Done;
        self.line = 0;
        self.sending = true;
    }

    /// Sends the finished recording again (from its header).
    pub fn resend(&mut self) {
        if self.stage == Stage::Done {
            self.line = 0;
            self.sending = true;
        }
    }

    /// Whether a send goes on.
    pub fn sending(&self) -> bool {
        self.sending
    }

    pub fn sign(&self) -> Option<Sign> {
        (self.stage == Stage::Done).then(|| Sign {
            cut: self.head.cut,
            sending: self
                .sending
                .then(|| (self.line * 100 / (self.head.lines() + 2)).min(99) as u8),
            sends: self.sends,
        })
    }

    /// One call of the game with `delta_us` of game time: the next line
    /// to log, when one is due (at most one a call, one per
    /// [`LINE_US`]).
    pub fn next_line(&mut self, delta_us: u32, out: &mut Text) -> bool {
        self.wait = self.wait.saturating_add(delta_us).min(LINE_US);
        if !self.sending || self.wait < LINE_US {
            return false;
        }
        self.wait = 0;
        let h = self.head;
        let lines = h.lines();
        out.push(TAG);
        if self.line == 0 {
            out.push("H").hex(u32::from(h.id), 4);
            out.digit(h.version)
                .digit(h.track)
                .digit(h.class)
                .digit(h.mode)
                .digit(h.catchup)
                .digit(h.rules);
            out.hex(h.seed, 8).hex(h.steps, 8).hex(h.len, 4);
            out.digit(u8::from(h.cut));
            out.hex(h.check, 8).hex(h.crc, 8).hex(lines, 4);
        } else if self.line <= lines {
            let seq = self.line - 1;
            out.push("D").hex(u32::from(h.id), 4).hex(seq, 4);
            out.push(" ");
            let from = seq as usize * LINE_BYTES;
            let to = (from + LINE_BYTES).min(h.len as usize);
            let mut i = from;
            while i < to {
                let n = (to - i).min(3);
                let b = [
                    byte(i),
                    if n > 1 { byte(i + 1) } else { 0 },
                    if n > 2 { byte(i + 2) } else { 0 },
                ];
                let v = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
                for k in 0..=n {
                    let x = (v >> (18 - 6 * k)) & 63;
                    out.byte(B64[x as usize]);
                }
                i += n;
            }
        } else {
            out.push("E").hex(u32::from(h.id), 4).hex(lines, 4);
            out.hex(h.crc, 8).hex(self.finish, 8).digit(self.place);
        }
        self.line += 1;
        if self.line > lines + 1 {
            self.sending = false;
            self.sends = self.sends.saturating_add(1);
        }
        true
    }
}

impl Default for Rec {
    fn default() -> Self {
        Rec::new()
    }
}

/// The game with the recorder: what the console runs when the feature
/// is on. The recording's line for this call goes first (so it always
/// fits the call's 256-byte log), then the game's own call, then the
/// recorder looks at what the call did and draws its sign on the
/// results.
pub struct RecRacer {
    pub game: Racer,
    pub rec: Rec,
}

impl Game for RecRacer {
    #[inline(never)]
    fn start(ctx: &mut Ctx) -> Self {
        RecRacer {
            game: Racer::start(ctx),
            rec: Rec::new(),
        }
    }

    fn update(&mut self, ctx: &mut Ctx) -> Flow {
        let keys = keys_of(ctx);
        send(ctx, &mut self.rec, keys.delta_us);
        let before = Before::of(&self.game.app);
        let flow = self.game.update(ctx);
        self.rec.after(before, &self.game.app, &keys);
        if flow == Flow::Run && self.game.app.results() {
            sign(&self.rec, ctx.screen());
        }
        flow
    }
}

/// The next line of a recording being sent, when one is due.
#[inline(never)]
fn send(ctx: &mut Ctx, rec: &mut Rec, delta_us: u32) {
    let mut t = Text::new();
    if rec.next_line(delta_us, &mut t) {
        ctx.log(t.as_str());
    }
}

/// Bottom left of the results panel: a recording exists (REC, or CUT
/// when the buffer filled), and its send (a percentage while it goes,
/// then SENT).
#[inline(never)]
pub fn sign(rec: &Rec, f: &mut Frame) {
    let Some(s) = rec.sign() else {
        return;
    };
    let x = END_BOX.0 + 3;
    f.text(x, END_Y, if s.cut { "CUT" } else { "REC" }, TEXT_DIM);
    let mut t = hud::Line::new();
    let c = match s.sending {
        Some(p) => {
            t.num(u32::from(p), 1).push("%");
            TEXT_DIM
        }
        None => {
            t.push("SENT");
            SEL
        }
    };
    f.text(x, END_Y + LINE, t.as_str(), c);
}
