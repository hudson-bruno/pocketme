//! Shared by the tests: input scripts, playing a room, pictures.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use game_kit::{Frame, H, W, button};
use rooftop::rooms::{ROOMS, Room, VIEW_W};
use rooftop::sim::{Event, Follow, World};
use rooftop::{Keys, Out, Rooftop, draw, pad_from};

/// Game time of one call at 30 calls a second.
pub const CALL_US: u32 = 33_333;

/// One line of a script: these keys held for this many calls.
#[derive(Clone, Copy, Debug)]
pub struct Hold {
    pub calls: u32,
    pub held: u32,
}

/// Script letters: L R U D (the d-pad), J (jump: A), X (dash: X), S
/// (Start), E (Select), B, `-` nothing.
pub fn letter_bits(c: char) -> u32 {
    match c {
        'L' => button::LEFT,
        'R' => button::RIGHT,
        'U' => button::UP,
        'D' => button::DOWN,
        'J' => button::A,
        'X' => button::X,
        'S' => button::START,
        'E' => button::SELECT,
        'B' => button::B,
        '-' => 0,
        _ => panic!("script letter {c}"),
    }
}

/// Parses "N KEYS" lines (`#` comments).
pub fn parse(text: &str) -> Vec<Hold> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let calls: u32 = parts
            .next()
            .unwrap()
            .parse()
            .unwrap_or_else(|_| panic!("line {}: {line}", i + 1));
        let keys = parts.next().unwrap_or("-");
        let held = keys.chars().map(letter_bits).fold(0, |a, b| a | b);
        out.push(Hold { calls, held });
    }
    out
}

/// The solution scripts, one per room, from tests/data/roomN.txt.
pub fn solution(room: usize) -> Vec<Hold> {
    let path = data_dir().join(format!("room{}.txt", room + 1));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    parse(&text)
}

pub fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

/// The calls of a script as (held, pressed) pairs.
pub fn calls(script: &[Hold]) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut was = 0;
    for h in script {
        for _ in 0..h.calls {
            out.push((h.held, h.held & !was));
            was = h.held;
        }
    }
    out
}

pub fn keys(held: u32, pressed: u32) -> Keys {
    Keys {
        held,
        pressed,
        delta_us: CALL_US,
        ..Keys::default()
    }
}

/// What playing a room by script gave.
#[derive(Debug, Default)]
pub struct Played {
    /// The call in which the door was reached, if it was.
    pub cleared_at: Option<usize>,
    /// Every fall or cut: (call, event).
    pub events: Vec<(usize, Event)>,
    /// The runner's box middle after every tick.
    pub path: Vec<(i32, i32)>,
}

/// Plays one room straight on the physics, two ticks per call (30 calls
/// a second), exactly as the game does. Stops at the door.
pub fn play_room(room: usize, script: &[Hold], follow: Follow) -> Played {
    let mut w = World::new(&ROOMS[room], follow);
    let mut p = Played::default();
    for (i, (held, pressed)) in calls(script).into_iter().enumerate() {
        let k = keys(held, pressed);
        for t in 0..2 {
            let pad = pad_from(&k, (false, false), (false, false), t == 0);
            let ev = w.tick(&pad);
            p.path.push(w.runner.centre());
            match ev {
                Some(Event::Cleared) => {
                    p.cleared_at = Some(i);
                    return p;
                }
                Some(e) => p.events.push((i, e)),
                None => {}
            }
        }
    }
    p
}

/// The game, driven call by call, with its own picture.
pub struct Driver {
    pub game: Rooftop,
    pub frame: Box<Frame>,
    pub outs: Vec<Out>,
    pub delta_us: u32,
}

impl Driver {
    pub fn new() -> Driver {
        let mut d = Driver {
            game: Rooftop::new(),
            frame: Box::new(Frame::new()),
            outs: Vec::new(),
            delta_us: CALL_US,
        };
        // The first call of a run comes with no time passed.
        let k = Keys::default();
        let out = d.game.update(&k, &mut d.frame);
        d.outs.push(out);
        d
    }

    pub fn call(&mut self, k: Keys) -> Out {
        let out = self.game.update(&k, &mut self.frame);
        self.outs.push(out);
        out
    }

    pub fn tap(&mut self, b: u32) -> Out {
        let o = self.call(Keys {
            held: b,
            pressed: b,
            delta_us: self.delta_us,
            ..Keys::default()
        });
        self.idle(1);
        o
    }

    pub fn idle(&mut self, n: u32) {
        for _ in 0..n {
            self.call(Keys {
                delta_us: self.delta_us,
                ..Keys::default()
            });
        }
    }

    /// Plays a script; returns the pictures' changed pixels per call.
    pub fn play(&mut self, script: &[Hold]) {
        for (held, pressed) in calls(script) {
            self.call(Keys {
                held,
                pressed,
                delta_us: self.delta_us,
                ..Keys::default()
            });
        }
    }

    pub fn notes(&self) -> Vec<rooftop::Note> {
        self.outs
            .iter()
            .flat_map(|o| o.notes.iter().flatten().copied())
            .collect()
    }
}

// ---- Pictures --------------------------------------------------------

/// RGB565 to 8-bit RGB.
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

/// The game's palette as RGB (numbers it does not set are black).
pub fn palette() -> [[u8; 3]; 256] {
    let mut p = [[0u8; 3]; 256];
    for (i, &c) in draw::COLOURS.iter().enumerate() {
        p[draw::FIRST as usize + i] = rgb(c);
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
    // zlib with stored blocks.
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

/// The whole room (all screens side by side) with the runner's path
/// drawn over it, at 2x, for looking at a solution.
pub fn save_room_path(path: &Path, room: usize, played: &Played) {
    let r = &ROOMS[room];
    let rw = r.w_px() as usize;
    let rh = H - draw::BAR_H as usize;
    let mut idx = vec![0u8; rw * rh];
    let mut cam = 0;
    loop {
        let mut f = Frame::new();
        let mut w = World::new(r, Follow::Smooth);
        w.cam = cam;
        w.runner.y = -100; // out of sight
        draw::paint_play(&mut f, &w);
        for y in 0..rh {
            for x in 0..W {
                let rx = cam as usize + x;
                if rx < rw {
                    idx[y * rw + rx] = f.0[(y + draw::BAR_H as usize) * W + x];
                }
            }
        }
        if cam + VIEW_W >= r.w_px() {
            break;
        }
        cam = (cam + VIEW_W).min(r.w_px() - VIEW_W);
    }
    let pal = palette();
    let s = 2;
    let (w, h) = (rw * s, rh * s);
    let mut buf = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            buf[(y * w + x) * 3..][..3].copy_from_slice(&pal[idx[(y / s) * rw + x / s] as usize]);
        }
    }
    for (i, &(x, y)) in played.path.iter().enumerate() {
        let (x, y) = (x as usize * s, y as usize * s);
        let c = if i % 20 < 10 {
            [255, 60, 200]
        } else {
            [80, 255, 120]
        };
        if x < w && y < h {
            buf[(y * w + x) * 3..][..3].copy_from_slice(&c);
        }
    }
    write_png(path, w, h, &buf);
}

/// The evidence folder (git-ignored), made if missing.
pub fn evidence(sub: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/games/platformer")
        .join(sub);
    std::fs::create_dir_all(&p).unwrap();
    p
}

// ---- Rooms for the tests only -----------------------------------------

/// A flat floor, three screens long: for measuring the handling.
pub static FLAT: Room = Room {
    name: "FLAT",
    idea: "test",
    rows: [
        b"............................................................",
        b"............................................................",
        b"............................................................",
        b"............................................................",
        b"............................................................",
        b"............................................................",
        b"............................................................",
        b"............................................................",
        b"............................................................",
        b"............................................................",
        b"..S.........................................................",
        b"############################################################",
        b"############################################################",
        b"############################################################",
    ],
};

/// Glass on the floor four tiles to the right of the start.
pub static GLASS_FLOOR: Room = Room {
    name: "GLASS FLOOR",
    idea: "test",
    rows: [
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"S...^...............",
        b"####################",
        b"####################",
        b"####################",
    ],
};

/// Glass hanging from a duct over the start: a full jump cuts.
pub static GLASS_CEILING: Room = Room {
    name: "GLASS CEILING",
    idea: "test",
    rows: [
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"MMMM................",
        b"vvvv................",
        b"....................",
        b".S..................",
        b"####################",
        b"####################",
        b"####################",
    ],
};

/// Glass on the left face of a wall, to the right of the start.
pub static GLASS_ON_WALL_RIGHT: Room = Room {
    name: "GLASS ON WALL RIGHT",
    idea: "test",
    rows: [
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b".....#..............",
        b"S...<#..............",
        b"####################",
        b"####################",
        b"####################",
    ],
};

/// Glass on the right face of a wall, to the left of the start.
pub static GLASS_ON_WALL_LEFT: Room = Room {
    name: "GLASS ON WALL LEFT",
    idea: "test",
    rows: [
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b".#..................",
        b".#>..S..............",
        b"####################",
        b"####################",
        b"####################",
    ],
};

/// A short roof that ends in a drop: for coyote time.
pub static LEDGE: Room = Room {
    name: "LEDGE",
    idea: "test",
    rows: [
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"....................",
        b"S...................",
        b"######..............",
        b"######..............",
        b"######..............",
    ],
};
