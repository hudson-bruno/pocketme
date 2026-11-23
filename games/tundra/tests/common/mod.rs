//! Shared by the tests: the game driven call by call, with its own
//! picture, and the console's own rule for what a changed picture sends.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use console_video::{Areas, diff};
use game_kit::{Frame, Touch, button};
use tundra::app::{App, Keys, Log, PadIn, Scene, TITLE_Y};
use tundra::draw;
use tundra::fixed::PX;
use tundra::play::Phase;

/// Game time of one call at 30 calls a second.
pub const CALL_US: u32 = 33_333;

/// The game, driven call by call.
pub struct Driver {
    pub app: App,
    pub frame: Box<Frame>,
    pub shown: Box<Frame>,
    /// Which pads are connected.
    pub pads: [bool; 4],
    held: [u32; 4],
    pub calls: u32,
    pub logs: Vec<Log>,
    /// What the console would send after each call: (pixels, areas).
    pub sent: Vec<(u32, Areas)>,
    pub delta_us: u32,
}

impl Driver {
    pub fn new(pads: [bool; 4]) -> Driver {
        let mut d = Driver {
            app: App::new(),
            frame: Box::new(Frame::new()),
            shown: Box::new(Frame::new()),
            pads,
            held: [0; 4],
            calls: 0,
            logs: Vec::new(),
            sent: Vec::new(),
            delta_us: CALL_US,
        };
        // The first call of a run comes with no time passed.
        let k = d.keys([0; 4], None, 0);
        assert!(d.app.update(&k));
        d.paint();
        d
    }

    pub fn keys(&self, held: [u32; 4], touch: Option<Touch>, delta_us: u32) -> Keys {
        let mut pads = [PadIn::default(); 4];
        for n in 0..4 {
            if self.pads[n] {
                pads[n] = PadIn {
                    connected: true,
                    held: held[n],
                    pressed: held[n] & !self.held[n],
                    stick: (0, 0),
                };
            }
        }
        Keys {
            pads,
            touch,
            delta_us,
            frame: self.calls,
        }
    }

    fn paint(&mut self) {
        while let Some(l) = self.app.take_log() {
            self.logs.push(l);
        }
        draw::draw(&self.app, &mut self.frame);
        let mut areas = Areas::new();
        diff(&self.shown, &self.frame, &mut areas);
        self.sent.push((areas.pixels(), areas));
        *self.shown = (*self.frame).clone();
    }

    /// One call with these buttons held per player and this touch.
    /// False: the game ended.
    pub fn call_with(&mut self, held: [u32; 4], touch: Option<Touch>) -> bool {
        let k = self.keys(held, touch, self.delta_us);
        self.held = held;
        self.calls += 1;
        let going = self.app.update(&k);
        if going {
            self.paint();
        } else {
            while let Some(l) = self.app.take_log() {
                self.logs.push(l);
            }
        }
        going
    }

    pub fn call(&mut self, held: [u32; 4]) -> bool {
        self.call_with(held, None)
    }

    pub fn idle(&mut self, calls: u32) {
        for _ in 0..calls {
            assert!(self.call([0; 4]));
        }
    }

    /// Player `n` presses and lets go of `bits`.
    pub fn press(&mut self, n: usize, bits: u32) -> bool {
        let mut held = [0; 4];
        held[n] = bits;
        let going = self.call(held);
        going && self.call([0; 4])
    }

    /// A short tap at game pixel `(x, y)`: down in one call, up in the next.
    pub fn tap(&mut self, x: i16, y: i16) -> bool {
        let down = Touch {
            x,
            y,
            down: true,
            started: true,
            ended: false,
        };
        let up = Touch {
            down: false,
            started: false,
            ended: true,
            ..down
        };
        self.call_with([0; 4], Some(down)) && self.call_with([0; 4], Some(up))
    }

    /// A finger held at `(x, y)` (it came down earlier, or comes down now
    /// when `start`).
    pub fn finger(&mut self, x: i16, y: i16, start: bool) -> bool {
        self.call_with(
            [0; 4],
            Some(Touch {
                x,
                y,
                down: true,
                started: start,
                ended: false,
            }),
        )
    }

    /// Starts a match from the title with player `n`'s A (or a tap on
    /// PLAY with no pads).
    pub fn start(&mut self) {
        assert_eq!(self.app.scene, Scene::Title);
        self.app.sel = 0;
        if let Some(n) = self.pads.iter().position(|&p| p) {
            assert!(self.press(n, button::A));
        } else {
            assert!(self.tap(120, TITLE_Y + 2));
        }
        assert_eq!(self.app.scene, Scene::Play);
    }

    /// Idles until the puck has dropped.
    pub fn until_live(&mut self) {
        for _ in 0..200 {
            if self.app.m.phase == Phase::Live {
                return;
            }
            self.idle(1);
        }
        panic!("the puck never dropped");
    }

    pub fn has_log(&self, f: impl Fn(&Log) -> bool) -> bool {
        self.logs.iter().any(f)
    }

    /// Player 1, played by the test: skate to the puck, carry it towards
    /// the right goal, hold A to shoot from close, B to check.
    pub fn bot_held(&self) -> u32 {
        let m = &self.app.m;
        let i = self.app.at[0];
        let s = m.sk[i];
        let carrying = m.carrier() == Some(i);
        let (tx, ty) = if carrying {
            (140 * PX, 66 * PX)
        } else {
            (m.puck.x, m.puck.y)
        };
        let mut held = 0;
        if tx > s.x + 2 * PX {
            held |= button::RIGHT;
        }
        if tx < s.x - 2 * PX {
            held |= button::LEFT;
        }
        if ty > s.y + 2 * PX {
            held |= button::DOWN;
        }
        if ty < s.y - 2 * PX {
            held |= button::UP;
        }
        if carrying && s.x > 100 * PX && s.charge < 20 {
            held |= button::A;
        }
        if !carrying && self.calls.is_multiple_of(20) && !m.team_has_puck(0) {
            held |= button::B;
        }
        held
    }
}

// ---- Pictures ----------------------------------------------------------------------

/// Saves a game picture as PNG, every pixel `scale` x `scale` (stored,
/// no compression: no outside crates needed).
pub fn save_frame(path: &Path, f: &Frame, scale: usize) {
    use game_kit::{H, W};
    let rgb = |c: u16| {
        let (r, g, b) = (
            u32::from(c >> 11) & 31,
            u32::from(c >> 5) & 63,
            u32::from(c) & 31,
        );
        [
            (r * 255 / 31) as u8,
            (g * 255 / 63) as u8,
            (b * 255 / 31) as u8,
        ]
    };
    let (w, h) = (W * scale, H * scale);
    let mut raw = Vec::with_capacity(h * (w * 3 + 1));
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            let i = f.0[(y / scale) * W + x / scale] as usize;
            raw.extend_from_slice(&rgb(*draw::COLOURS.get(i).unwrap_or(&0xF81F)));
        }
    }
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

/// Where the tests leave pictures, numbers and traces.
pub fn evidence(sub: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/four-players/hockey")
        .join(sub);
    std::fs::create_dir_all(&p).unwrap();
    p
}
