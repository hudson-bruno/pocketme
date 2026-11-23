//! Shared by the tests: the app driven call by call as the console would
//! (the rate it asks for is taken after the call), pictures, hashes.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use bench::app::{App, Input, Mode, Out};
use bench::art::{COLOURS, PALETTE};
use bench::load::{TESTS, Work};
use bench::ramp::period;
use game_kit::{Frame, H, W, button};

/// The app with its own picture and a console's rate.
pub struct Driver {
    pub app: Box<App>,
    pub work: Box<Work>,
    pub frame: Box<Frame>,
    /// The rate the console runs at now.
    pub rate60: bool,
    pub logs: Vec<String>,
    pub out: Out,
}

impl Driver {
    pub fn new() -> Driver {
        let mut d = Driver {
            app: Box::new(App::new()),
            work: Box::new(Work::new()),
            frame: Box::new(Frame::new()),
            rate60: false,
            logs: Vec::new(),
            out: Out::default(),
        };
        d.call_full(0, period(false), true);
        d
    }

    /// One call: keys pressed, time since the last call.
    pub fn call_full(&mut self, pressed: u32, delta_us: u32, fresh: bool) -> Out {
        let inp = Input {
            pressed,
            delta_us,
            fresh,
            rate60_now: self.rate60,
        };
        let out = self.app.update(&inp, &mut self.frame, &mut self.work);
        if !self.app.log.is_empty() {
            for line in self.app.log.as_str().split('\n') {
                self.logs.push(line.to_string());
            }
        }
        // The console takes the rate after the call.
        self.rate60 = out.rate60;
        self.out = out;
        out
    }

    /// One call on time (the time between calls is the frame).
    pub fn call(&mut self, pressed: u32) -> Out {
        let p = period(self.rate60);
        self.call_full(pressed, p, false)
    }

    pub fn idle(&mut self, calls: u32) {
        for _ in 0..calls {
            self.call(0);
        }
    }

    /// Goes to test `i` (RIGHT presses from the current one) at level
    /// `level` (DOWN to the bottom, then UP).
    pub fn pick(&mut self, i: usize, level: usize) {
        while self.app.test != i {
            self.call(button::RIGHT);
        }
        for _ in 0..8 {
            self.call(button::DOWN);
        }
        for _ in 0..level {
            self.call(button::UP);
        }
        assert_eq!(self.app.mode, Mode::Fixed);
        assert_eq!(self.app.load(), TESTS[i].loads[level]);
    }
}

/// FNV-1a over the picture.
pub fn hash(f: &Frame) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for &b in f.0.iter() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01B3);
    }
    h
}

/// Checks a picture uses only the game's own palette numbers.
pub fn check_palette(f: &Frame, what: &str) {
    for (i, &p) in f.0.iter().enumerate() {
        assert!(
            usize::from(p) < COLOURS,
            "{what}: pixel {i} has palette number {p}"
        );
    }
}

/// The colour of a palette number (added for `tests/city.rs`).
pub fn rgb_of(i: u8) -> [u8; 3] {
    rgb(i)
}

fn rgb(i: u8) -> [u8; 3] {
    let c = PALETTE.get(usize::from(i)).copied().unwrap_or(0xF81F);
    let r = (c >> 11) & 31;
    let g = (c >> 5) & 63;
    let b = c & 31;
    [
        ((r << 3) | (r >> 2)) as u8,
        ((g << 2) | (g >> 4)) as u8,
        ((b << 3) | (b >> 2)) as u8,
    ]
}

/// Saves a picture at `scale` times its size.
pub fn save_frame(path: &Path, f: &Frame, scale: usize) {
    let (w, h) = (W * scale, H * scale);
    let mut buf = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let c = rgb(f.0[(y / scale) * W + x / scale]);
            buf[(y * w + x) * 3..][..3].copy_from_slice(&c);
        }
    }
    write_png(path, w, h, &buf);
}

/// The pictures folder (git-ignored, outside the crate:
/// `.local/bench/shots/` at the repository root), made if missing.
pub fn shots() -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/bench/shots");
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// A plain PNG (stored, not packed), with no crates.
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
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(path, out).unwrap();
}

/// A made-up console: the time one loop takes for a load is `fixed_us +
/// load * unit_ns / 1000`; a call comes on time when that fits the frame,
/// else when the loop is done.
#[derive(Clone, Copy, Debug)]
pub struct CostModel {
    pub fixed_us: u32,
    pub unit_ns: u32,
}

impl CostModel {
    pub fn work(&self, load: u32) -> u32 {
        let w = u64::from(self.fixed_us) + u64::from(load) * u64::from(self.unit_ns) / 1000;
        w.min(u64::from(u32::MAX)) as u32
    }

    pub fn delta(&self, load: u32, period: u32) -> u32 {
        self.work(load).max(period)
    }
}
