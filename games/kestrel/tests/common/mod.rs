//! Shared by the tests: driving the game call by call, an autopilot
//! that flies a mission with the left stick and A/B only, pictures.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use game_kit::{Frame, H, Touch, W, button};
use kestrel::app::{App, CARD_WAIT, DEAD_ZONE, Keys, Scene};
use kestrel::art;
use kestrel::course::{CORRIDOR_X, CORRIDOR_Y, Kind, Mission, Thing, Things};
use kestrel::models::WRECK_BOX;
use kestrel::poly::M8;
use kestrel::world::{SHIP_R, SIDE_MPS, UPDOWN_MPS, World};

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
    pub events: Vec<kestrel::app::Event>,
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

    /// One call with buttons and the left stick.
    pub fn hold_stick(&mut self, held: u32, left: (i16, i16)) -> bool {
        let k = Keys {
            held,
            pressed: held & !self.held,
            left,
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

    /// Idle until the card on screen takes a press (at any call rate).
    pub fn wait_card(&mut self) {
        for _ in 0..200 {
            if self.app.scene_t >= CARD_WAIT {
                return;
            }
            self.hold(0);
        }
        panic!("the card never took a press");
    }

    pub fn idle(&mut self, calls: u32) {
        for _ in 0..calls {
            self.hold(0);
        }
    }

    /// A finger: down at (x, y) in game pixels for one call, then up.
    pub fn touch(&mut self, x: i16, y: i16) {
        let down = Touch {
            x,
            y,
            down: true,
            started: true,
            ended: false,
        };
        self.call_keys(Keys {
            touch: Some(down),
            ..Keys::default()
        });
        let up = Touch {
            down: false,
            started: false,
            ended: true,
            ..down
        };
        self.call_keys(Keys {
            touch: Some(up),
            ..Keys::default()
        });
    }

    /// From the title: PLAY at the options set, through the briefing.
    pub fn play_from_title(&mut self) {
        assert_eq!(self.app.scene, Scene::Title);
        self.app.menu = 0;
        self.tap(button::A);
        assert_eq!(self.app.scene, Scene::Brief);
        self.launch();
    }

    /// From the briefing card: wait until it takes a press, press A.
    pub fn launch(&mut self) {
        assert_eq!(self.app.scene, Scene::Brief);
        self.wait_card();
        self.tap(button::A);
        assert_eq!(self.app.scene, Scene::Play);
    }
}

// ---- Autopilot ---------------------------------------------------------------------

/// What the autopilot reports after a mission.
#[derive(Debug, Default)]
pub struct Trip {
    pub calls: usize,
    pub delivered: bool,
}

/// What the autopilot wants for one call.
#[derive(Clone, Copy, Debug, Default)]
pub struct Plan {
    /// Steering, -256..=256 (right, up).
    pub steer: (i32, i32),
    /// -1 brake, 0 cruise, 1 boost.
    pub speed: i8,
    /// A rock (or pod, or wreck) is being dodged.
    pub dodging: bool,
}

/// Flies the current mission until it is over (or `max_calls`), with
/// the left stick and A (boost) / B (brake) only.
pub fn autopilot(d: &mut Driver, max_calls: usize) -> Trip {
    let mut trip = Trip::default();
    while trip.calls < max_calls {
        if d.app.scene != kestrel::app::Scene::Play {
            break;
        }
        trip.calls += 1;
        let p = plan(&d.app.world);
        d.hold_stick(buttons_of(&p), stick_of(p.steer));
    }
    trip.delivered = d.app.world.state == kestrel::world::State::Delivered;
    trip
}

pub fn buttons_of(p: &Plan) -> u32 {
    match p.speed {
        1 => button::A,
        -1 => button::B,
        _ => 0,
    }
}

/// The stick value that reads as `steer` (stick up reads minus).
pub fn stick_of(steer: (i32, i32)) -> (i16, i16) {
    let v = |s: i32| -> i16 {
        if s == 0 {
            0
        } else {
            (s * (32_767 - DEAD_ZONE) / 256 + s.signum() * DEAD_ZONE).clamp(-32_767, 32_767) as i16
        }
    };
    (v(steer.0), -v(steer.1))
}

/// Room kept between the craft and anything hard (1/256 m).
const MARGIN: i32 = M8 * 7 / 10;

/// How far a thing reaches out from its middle, sideways and up-down
/// (half sizes), including the craft and a margin.
fn reach(t: &Thing) -> (i32, i32, i32, i32) {
    let r = SHIP_R + MARGIN;
    match t.kind {
        Kind::Wreck => {
            let across = t.angles.0 != 0;
            let hx = if across { WRECK_BOX[3] } else { WRECK_BOX[0] };
            // x left, x right, y down, y up.
            (hx + r, hx + r, WRECK_BOX[1] + r, WRECK_BOX[2] + r)
        }
        Kind::Rock(_) => {
            let k = t.size * 7 / 8 + r;
            (k, k, k, k)
        }
        Kind::Pod => (230 + r, 230 + r, 230 + r, 230 + r),
        Kind::Cell => (0, 0, 0, 0),
    }
}

/// The plan for this moment: aim for the next gate's middle (where it
/// will be on arrival); if something hard is in the way on that line,
/// aim past its nearer edge instead. Boost when nothing is near.
pub fn plan(w: &World) -> Plan {
    let m = w.m();
    let p = w.ship.pos();
    let per_tick = (w.ship.speed >> 4).max(1);
    let eta = |z: i32| ((z - p[2]).max(0) / per_tick) as u32;
    let g = m.gate(w.next_gate, w.ticks);
    let g = m.gate(w.next_gate, w.ticks + eta(g.z));
    let mut aim = (g.x, g.y, g.z);
    let mut dodging = false;

    // The first hard thing in the way, nearest first.
    let look = 34 * M8;
    let c0 = Mission::cell_of(p[2]);
    let c1 = Mission::cell_of((p[2] + look).min(g.z + 12 * M8));
    let mut found: Option<(Thing, (i32, i32))> = None;
    let mut best_z = i32::MAX;
    let mut list = Things::new();
    for c in c0..=c1 {
        m.things(c, w.ticks, &mut list);
        let n = list.n;
        for i in 0..n {
            let t0 = list.list[i];
            if t0.kind == Kind::Cell || t0.z < p[2] - M8 {
                continue;
            }
            // Where it will be when the craft gets there.
            let mut later = Things::new();
            m.things(c, w.ticks + eta(t0.z), &mut later);
            let t = later
                .as_slice()
                .iter()
                .find(|x| x.id == t0.id)
                .copied()
                .unwrap_or(t0);
            // Where the craft would be at its z on the line to the aim.
            let span = (aim.2 - p[2]).max(1);
            let f = ((t.z - p[2]).clamp(0, span) as i64 * 1024 / span as i64) as i32;
            let at = (
                p[0] + ((aim.0 - p[0]) as i64 * f as i64 / 1024) as i32,
                p[1] + ((aim.1 - p[1]) as i64 * f as i64 / 1024) as i32,
            );
            let (xl, xr, yd, yu) = reach(&t);
            let inside = at.0 > t.x - xl && at.0 < t.x + xr && at.1 > t.y - yd && at.1 < t.y + yu;
            if inside && t.z < best_z {
                best_z = t.z;
                // Out past the nearer side, kept in the corridor; for a
                // wide thing go over or under when that is shorter.
                let lim_x = CORRIDOR_X - M8 / 2;
                let lim_y = CORRIDOR_Y - M8 / 2;
                let left = t.x - xl - M8 / 4;
                let right = t.x + xr + M8 / 4;
                let down = t.y - yd - M8 / 4;
                let up = t.y + yu + M8 / 4;
                let mut opts: Vec<(i32, (i32, i32))> = Vec::new();
                if left > -lim_x {
                    opts.push(((at.0 - left).abs(), (left, at.1)));
                }
                if right < lim_x {
                    opts.push(((right - at.0).abs(), (right, at.1)));
                }
                if down > -lim_y {
                    opts.push(((at.1 - down).abs() * 3 / 2, (at.0, down)));
                }
                if up < lim_y {
                    opts.push(((up - at.1).abs() * 3 / 2, (at.0, up)));
                }
                // Prefer the side towards the gate.
                for o in opts.iter_mut() {
                    let towards = ((o.1.0 - g.x).abs() + (o.1.1 - g.y).abs()) / 4;
                    o.0 += towards;
                }
                if let Some(best) = opts.iter().min_by_key(|o| o.0) {
                    found = Some((t, best.1));
                }
            }
        }
    }
    if let Some((t, at)) = found {
        aim = (at.0, at.1, t.z);
        dodging = true;
    }

    // Steer to be at the aim's sideways place when reaching its z.
    let ticks_left = eta(aim.2).max(8) as i32;
    let side_max = SIDE_MPS * 4096 / 60;
    let up_max = UPDOWN_MPS * 4096 / 60;
    // Wanted speed in 1/4096 m a tick, then as stick travel.
    let want_vx = ((aim.0 - p[0]) * 16 / ticks_left).clamp(-side_max, side_max);
    let want_vy = ((aim.1 - p[1]) * 16 / ticks_left).clamp(-up_max, up_max);
    let gain = |want: i32, now: i32, max: i32| -> i32 {
        // A little ahead of the smoothing: push harder than wanted while
        // the speed is still far off.
        let v = want + (want - now) / 2;
        (v * 256 / max).clamp(-256, 256)
    };
    let steer = (
        gain(want_vx, w.ship.vx, side_max),
        gain(want_vy, w.ship.vy, up_max),
    );
    // Speed: brake when a big move is needed soon, boost when clear.
    let need = (aim.0 - p[0]).abs() * 16 / side_max.max(1);
    let speed = if dodging && need > ticks_left * 3 / 4 {
        -1
    } else if dodging && best_z - p[2] < 14 * M8 {
        0
    } else if (aim.0 - p[0]).abs() < 3 * M8 && (aim.1 - p[1]).abs() < 3 * M8 {
        1
    } else if need > ticks_left {
        -1
    } else {
        0
    };
    Plan {
        steer,
        speed,
        dodging,
    }
}

// ---- Pictures ------------------------------------------------------------------------

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

/// The game's palette as RGB (numbers it does not set are magenta, so
/// a stray one shows).
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

/// The evidence folder (git-ignored), made if missing.
pub fn evidence(sub: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/next/ship")
        .join(sub);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Checks a picture uses only the game's own palette numbers.
pub fn check_palette(f: &Frame, what: &str) {
    let max = art::COLOURS.len() as u8;
    for (i, &p) in f.0.iter().enumerate() {
        assert!(p < max, "{what}: pixel {i} has palette number {p}");
    }
}
