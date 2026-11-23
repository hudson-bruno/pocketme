//! Shared by the tests: driving the game call by call through its keys,
//! an autopilot that steers the player's ship with the stick and the
//! buttons only, pictures.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use camber::app::{App, Keys, Log, Page, Scene};
use camber::art;
use camber::race::{Carve, Class, Ctl, Mode};
use camber::scene::{Detail, ViewKind};
use game_kit::{Frame, H, W, button};

/// Game time of one call at 30 and at 60 calls a second.
pub const CALL_30: u32 = 33_333;
pub const CALL_60: u32 = 16_667;

/// The game, driven call by call with its own picture.
pub struct Driver {
    pub app: App,
    pub frame: Box<Frame>,
    pub call_us: u32,
    pub held: u32,
    pub logs: Vec<Log>,
    pub draw: bool,
    /// Game time for the picture (ticks).
    pub time: u32,
}

impl Driver {
    pub fn new() -> Driver {
        let mut d = Driver {
            app: App::new(),
            frame: Box::new(Frame::new()),
            call_us: CALL_30,
            held: 0,
            logs: Vec::new(),
            draw: true,
            time: 0,
        };
        d.call(Keys::default());
        d
    }

    /// One call with these keys; draws the picture.
    pub fn call(&mut self, mut k: Keys) -> bool {
        k.delta_us = self.call_us;
        let going = self.app.update(&k);
        while let Some(l) = self.app.take_log() {
            self.logs.push(l);
        }
        if !self.app.still() {
            self.time = self.time.wrapping_add(self.app.ticks);
        }
        if going && self.draw {
            self.app.draw(&mut self.frame, self.time);
        }
        going
    }

    /// One call with buttons held and the stick (pressed: buttons not
    /// held last call).
    pub fn hold(&mut self, held: u32, stick: (i16, i16)) -> bool {
        let k = Keys {
            held,
            pressed: held & !self.held,
            stick,
            ..Keys::default()
        };
        self.held = held;
        self.call(k)
    }

    /// Press and let go of a button (two calls).
    pub fn tap(&mut self, b: u32) {
        self.hold(b, (0, 0));
        self.hold(0, (0, 0));
    }

    /// Sets the options and starts from the title's PLAY line.
    pub fn race(&mut self, track: u8, class: Class, mode: Mode) {
        self.app.opts.track = track;
        self.app.opts.class = class;
        self.app.opts.mode = mode;
        assert_eq!(self.app.scene, Scene::Title);
        self.app.page = Page::Main;
        self.app.sel = 0;
        self.tap(button::A);
        assert_eq!(self.app.scene, Scene::Race);
    }

    pub fn set(&mut self, detail: Detail, view: ViewKind, fast: bool) {
        self.app.opts.detail = detail;
        self.app.opts.view = view;
        self.app.opts.fast = fast;
        self.call_us = if fast { CALL_60 } else { CALL_30 };
    }

    /// One call driven by the computer's pilot, through the keys.
    pub fn auto(&mut self) -> bool {
        let c = self.app.race.drive(0);
        let (held, stick) = keys_of_ctl(c);
        self.hold(held, stick)
    }
}

impl Default for Driver {
    fn default() -> Self {
        Driver::new()
    }
}

/// What a pilot's wish looks like on the pad: the stick for steering,
/// A for thrust, B for both air brakes, L and R for the carve. The
/// player has no one-sided air brake: a pilot's wish for one is dropped.
pub fn keys_of_ctl(c: Ctl) -> (u32, (i16, i16)) {
    let mut held = 0;
    if c.thrust {
        held |= button::A;
    }
    if c.brake_l && c.brake_r {
        held |= button::B;
    }
    held |= match c.carve {
        Carve::Left => button::L,
        Carve::Right => button::R,
        Carve::Both => button::L | button::R,
        Carve::Off | Carve::Keep => 0,
    };
    // The stick's dead zone is taken back out by the game.
    let dz = camber::app::DEAD_ZONE;
    let x = if c.steer == 0 {
        0
    } else {
        c.steer.signum() * dz + c.steer * (32_767 - dz) / 256
    };
    (held, (x.clamp(-32_767, 32_767) as i16, 0))
}

// ---- Pictures --------------------------------------------------------------------------

pub fn rgb(c: u16) -> [u8; 3] {
    let r = (c >> 11) & 31;
    let g = (c >> 5) & 63;
    let b = c & 31;
    [
        (r * 255 / 31) as u8,
        (g * 255 / 63) as u8,
        (b * 255 / 31) as u8,
    ]
}

/// The palette of a track's theme as RGB (numbers the game does not set
/// are magenta, so a stray one shows).
pub fn palette(theme: u8) -> [[u8; 3]; 256] {
    let mut pal = [0u16; art::COLOURS];
    art::palette(&art::THEMES[usize::from(theme)], &mut pal);
    let mut p = [[255u8, 0, 255]; 256];
    for (i, &c) in pal.iter().enumerate() {
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
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(path, out).unwrap();
}

/// Saves a game picture at `scale` times its size.
pub fn save_frame(path: &Path, f: &Frame, theme: u8, scale: usize) {
    let pal = palette(theme);
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

/// Several pictures side by side (a strip of frames).
pub fn save_strip(path: &Path, frames: &[Frame], theme: u8, scale: usize) {
    let pal = palette(theme);
    let (fw, h) = (W * scale + 2, H * scale);
    let w = fw * frames.len();
    let mut buf = vec![0u8; w * h * 3];
    for (k, f) in frames.iter().enumerate() {
        for y in 0..h {
            for x in 0..W * scale {
                let c = pal[f.0[(y / scale) * W + x / scale] as usize];
                buf[(y * w + k * fw + x) * 3..][..3].copy_from_slice(&c);
            }
        }
    }
    write_png(path, w, h, &buf);
}

/// The pictures folder (git-ignored), made if missing.
pub fn shots() -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/camber/shots");
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Checks a picture uses only the game's own palette numbers.
pub fn check_palette(f: &Frame, what: &str) {
    for (i, &p) in f.0.iter().enumerate() {
        assert!(
            usize::from(p) < art::COLOURS,
            "{what}: pixel {i} has palette number {p}"
        );
    }
}

/// Picture height and width, for the tests that look at pixels.
pub const SIZE: (usize, usize) = (W, H);
