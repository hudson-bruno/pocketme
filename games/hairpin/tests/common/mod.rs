//! Shared by the tests: driving the game call by call, an autopilot
//! that races a course, pictures.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use game_kit::{Frame, H, Touch, W, button};
use hairpin::app::{App, DEAD_ZONE, Event, Keys, Scene};
use hairpin::art;
use hairpin::race::{GRIP, KART_W, PUSH_DIV, STEER, SUB, X_ONE, wrap};
use hairpin::track::SEG;

/// Game time of one call at 30 calls a second.
pub const CALL_US: u64 = 33_333;

/// The game, driven call by call with its own picture.
pub struct Driver {
    pub app: App,
    pub frame: Box<Frame>,
    pub now_us: u64,
    pub call_us: u64,
    pub held: u32,
    /// Lines the game logged.
    pub events: Vec<Event>,
}

impl Driver {
    pub fn new() -> Driver {
        let mut d = Driver {
            app: App::new(),
            frame: Box::new(Frame::new()),
            now_us: 0,
            call_us: CALL_US,
            held: 0,
            events: Vec::new(),
        };
        d.call_keys(Keys::default());
        d
    }

    /// One call with these keys; draws the picture.
    pub fn call_keys(&mut self, k: Keys) -> bool {
        let going = self.app.update(&k, self.now_us);
        while let Some(e) = self.app.take_event() {
            self.events.push(e);
        }
        if going {
            self.app.draw(&mut self.frame);
        }
        self.now_us += self.call_us;
        going
    }

    /// One call with buttons held (pressed: those not held last call).
    pub fn hold(&mut self, held: u32) -> bool {
        let k = Keys {
            held,
            pressed: held & !self.held,
            ..Keys::default()
        };
        self.held = held;
        self.call_keys(k)
    }

    /// Press and let go of a button (two calls).
    pub fn tap(&mut self, b: u32) {
        self.hold(b);
        self.hold(0);
    }

    pub fn idle(&mut self, calls: u32) {
        for _ in 0..calls {
            self.hold(0);
        }
    }

    /// A finger held at (x, y) for one call (it started this call when
    /// `started`).
    pub fn finger(&mut self, x: i16, y: i16, started: bool) -> bool {
        self.held = 0;
        self.call_keys(Keys {
            touch: Some(Touch {
                x,
                y,
                down: true,
                started,
                ended: false,
            }),
            ..Keys::default()
        })
    }

    /// The finger lifted at (x, y).
    pub fn lift(&mut self, x: i16, y: i16) -> bool {
        self.call_keys(Keys {
            touch: Some(Touch {
                x,
                y,
                down: false,
                started: false,
                ended: true,
            }),
            ..Keys::default()
        })
    }

    /// A tap: down at (x, y) for one call, then up.
    pub fn touch(&mut self, x: i16, y: i16) {
        self.finger(x, y, true);
        self.lift(x, y);
    }

    /// From the title: PLAY at the course set in the options.
    pub fn play_from_title(&mut self) {
        assert_eq!(self.app.scene, Scene::Title);
        self.app.menu = 0;
        self.tap(button::A);
        assert_eq!(self.app.scene, Scene::Race);
    }

    /// From the title: course `c` (0-based).
    pub fn race_course(&mut self, c: u8) {
        self.app.opts.course = c;
        self.play_from_title();
        assert_eq!(self.app.race.course, c);
    }
}

// ---- Autopilot ---------------------------------------------------------------------

/// A stick position for a steering wish (-256..256).
pub fn stick_for(steer: i32) -> i16 {
    if steer == 0 {
        0
    } else {
        let a = DEAD_ZONE + steer.abs().min(256) * (32_767 - DEAD_ZONE) / 256;
        (steer.signum() * a) as i16
    }
}

/// What the autopilot does in one call: a racing driver's keys.
pub fn plan(app: &App) -> Keys {
    let r = &app.race;
    let course = r.course();
    let me = r.karts[0];
    let seg = r.seg_of(0);
    let ratio = me.ratio().max(1);
    let look = 4 + (ratio as u32) * 22 / 1024;
    let bend = course.sharpest(seg, look);
    let cap = if bend == 0 {
        4096
    } else {
        GRIP * 90 / 100 / bend.abs()
    };
    // Aim for the inside of the bend, round slower karts ahead.
    let mut target = bend.signum() * X_ONE * 3 / 10;
    let lap = r.lap_prog();
    for o in &r.karts[1..] {
        let dz = wrap(o.prog - me.prog, lap);
        if dz > 0 && dz < 5 * SEG * SUB && (o.x - me.x).abs() < KART_W * 3 / 2 {
            target = if o.x > 0 {
                o.x - KART_W * 3 / 2
            } else {
                o.x + KART_W * 3 / 2
            };
        }
    }
    let target = target.clamp(-X_ONE * 7 / 10, X_ONE * 7 / 10);
    // Steering: hold against the bend's push, and towards the aim.
    let here = course.seg(seg).curve16;
    let rr = i64::from(ratio);
    let push = i64::from(here) * rr * rr / PUSH_DIV;
    let ff = (push * 256 * 1024 / (i64::from(STEER) * rr)) as i32;
    let steer = (ff + (target - me.x) * 256 / (X_ONE / 3)).clamp(-256, 256);

    let mut held = 0;
    if ratio > cap + 30 {
        held |= button::B;
    } else if ratio <= cap {
        held |= button::A;
    }
    let calm = course.sharpest(seg, 30) == 0;
    if calm && r.charge > 90 && ratio > 500 && held & button::A != 0 {
        held |= button::X;
    }
    Keys {
        held,
        pressed: 0,
        left: (stick_for(steer), 0),
        ..Keys::default()
    }
}

/// What the autopilot reports after a race.
#[derive(Debug, Default)]
pub struct Trip {
    pub calls: usize,
    pub finished: bool,
    pub place: u8,
    pub ticks: u32,
    pub crashes: u32,
}

/// Races the current course until the finish card. Gives up after
/// `max_calls`.
pub fn autopilot(d: &mut Driver, max_calls: usize) -> Trip {
    let mut trip = Trip::default();
    let from = d.events.len();
    while trip.calls < max_calls {
        trip.calls += 1;
        match d.app.scene {
            Scene::Finish => {
                trip.finished = true;
                trip.place = d.app.race.finish_place;
                trip.ticks = d.app.race.finish_ticks;
                break;
            }
            Scene::Race => {}
            s => panic!("autopilot in scene {s:?}"),
        }
        let mut k = plan(&d.app);
        k.pressed = k.held & !d.held;
        d.held = k.held;
        d.call_keys(k);
    }
    trip.crashes = d.events[from..]
        .iter()
        .filter(|e| matches!(e, Event::Crash))
        .count() as u32;
    trip
}

// ---- Pictures ----------------------------------------------------------------------

fn rgb(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 31) as u32;
    let g = ((c >> 5) & 63) as u32;
    let b = (c & 31) as u32;
    [
        (r * 255 / 31) as u8,
        (g * 255 / 63) as u8,
        (b * 255 / 31) as u8,
    ]
}

pub fn palette() -> [[u8; 3]; 256] {
    let mut p = [[255u8, 0, 255]; 256];
    for (i, &c) in art::COLOURS.iter().enumerate() {
        p[i] = rgb(c);
    }
    p
}

/// Writes an RGB picture as PNG (stored, no compression: no outside
/// crates needed).
pub fn write_png(path: &Path, w: usize, h: usize, rgb: &[u8]) {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc(&body).to_be_bytes());
    }
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0);
        raw.extend_from_slice(&rgb[y * w * 3..(y + 1) * w * 3]);
    }
    let mut z = vec![0x78, 0x01];
    for (i, block) in raw.chunks(65_535).enumerate() {
        let last = (i + 1) * 65_535 >= raw.len();
        z.push(u8::from(last));
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    std::fs::write(path, out).unwrap();
}

/// Saves a game picture, every pixel `scale` x `scale`.
pub fn save_frame(path: &Path, f: &Frame, scale: usize) {
    let pal = palette();
    let (w, h) = (W * scale, H * scale);
    let mut buf = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let c = pal[f.0[(y / scale) * W + x / scale] as usize];
            buf[(y * w + x) * 3..][..3].copy_from_slice(&c);
        }
    }
    write_png(path, w, h, &buf);
}

/// The evidence folder (git-ignored), made if missing.
pub fn evidence(sub: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/next/racing")
        .join(sub);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// A small random number maker (xorshift), the same every run for a
/// seed.
pub struct Dice(pub u64);

impl Dice {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}
