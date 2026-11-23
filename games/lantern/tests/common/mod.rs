//! Shared by the tests: driving the game call by call, an autopilot
//! that plays a level from its map, pictures.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use game_kit::{Frame, H, Touch, W, button};
use lantern::app::{App, Keys, Scene};
use lantern::art;
use lantern::fixed::{ONE, SHIFT, TRIG_SHIFT, angle_to, centre, cos, sin};
use lantern::level::MAX_SIDE;
use lantern::world::{FLASH_COST, Kind, Mood, PASS_OPEN, World, cell, is_door};

/// Game time of one call at 30 calls a second.
pub const CALL_US: u64 = 33_333;

/// The game, driven call by call with its own picture.
pub struct Driver {
    pub app: App,
    pub frame: Box<Frame>,
    pub now_us: u64,
    pub call_us: u64,
    pub held: u32,
    /// Every call's held buttons, for writing a script.
    pub record: Vec<u32>,
    /// Lines the game logged.
    pub events: Vec<lantern::app::Event>,
}

impl Driver {
    pub fn new() -> Driver {
        let mut d = Driver {
            app: App::new(),
            frame: Box::new(Frame::new()),
            now_us: 0,
            call_us: CALL_US,
            held: 0,
            record: Vec::new(),
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
        self.record.push(k.held);
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

    /// From the title: PLAY at the options set.
    pub fn play_from_title(&mut self) {
        assert_eq!(self.app.scene, Scene::Title);
        self.app.menu = 0;
        self.tap(button::A);
        assert_eq!(self.app.scene, Scene::Play);
    }
}

// ---- Autopilot ---------------------------------------------------------------------

/// What the autopilot reports after a level.
#[derive(Debug, Default)]
pub struct Trip {
    pub calls: usize,
    pub cleared: bool,
    pub faints: u32,
    pub flashes: u32,
}

/// Plays the current level to the lift: picks up every reachable item
/// (`greedy`) or only the crank, opens doors, works switches, flashes
/// at creatures in front. Gives up after `max_calls`.
pub fn autopilot(d: &mut Driver, greedy: bool, max_calls: usize) -> Trip {
    let mut trip = Trip::default();
    let level = d.app.world.level;
    let mut press_wait = 0u32;
    let mut stuck = 0u32;
    let mut last_pos = (0, 0);
    while trip.calls < max_calls {
        trip.calls += 1;
        match d.app.scene {
            Scene::Clear => {
                trip.cleared = true;
                return trip;
            }
            Scene::Fainted => {
                trip.faints += 1;
                d.idle(45);
                d.tap(button::A);
                continue;
            }
            Scene::Play => {}
            s => panic!("autopilot in scene {s:?}"),
        }
        assert_eq!(d.app.world.level, level);
        let w = &d.app.world;
        let mut held = plan(w, greedy);
        // A button press needs a release between: hold A or B for one
        // call at most, then nothing for one call.
        if held & (button::A | button::B) != 0 {
            if press_wait > 0 {
                held &= !(button::A | button::B);
            } else {
                press_wait = 2;
            }
        }
        press_wait = press_wait.saturating_sub(1);
        if held & button::B != 0 {
            trip.flashes += 1;
        }
        // Stuck on a corner (no progress for 3 s): back off a little.
        let pos = (w.x >> 9, w.y >> 9);
        if pos == last_pos && held & button::UP != 0 {
            stuck += 1;
        } else {
            stuck = 0;
        }
        last_pos = pos;
        if stuck > 90 {
            for _ in 0..6 {
                d.hold(button::DOWN | button::R);
            }
            stuck = 0;
            continue;
        }
        d.hold(held);
    }
    trip
}

/// The buttons for this call.
fn plan(w: &World, greedy: bool) -> u32 {
    // 1. A creature in front and near: flash.
    if w.cool == 0 && w.oil >= FLASH_COST && creature_ahead(w) {
        return button::B;
    }
    let me = (w.x >> SHIFT, w.y >> SHIFT);
    let dist = bfs(w, me);
    // 2. Where to: an item, else the crank, else the lift.
    let goal = choose_goal(w, &dist, greedy);
    let Some(goal) = goal else {
        // Nothing reachable: a switch must open the way.
        return button::RIGHT;
    };
    match goal {
        Goal::Cell(target) => walk_to(w, &dist, target, None),
        Goal::Use(stand, wall) => walk_to(w, &dist, stand, Some(wall)),
    }
}

#[derive(Clone, Copy, Debug)]
enum Goal {
    /// Walk onto this cell.
    Cell((i32, i32)),
    /// Stand on the first cell and press A at the second (a wall cell).
    Use((i32, i32), (i32, i32)),
}

fn passable(w: &World, cx: i32, cy: i32) -> bool {
    match w.at(cx, cy) {
        cell::FLOOR | cell::DOOR => true,
        cell::GATE_A | cell::GATE_B => w.open_at(cx, cy) > 0,
        _ => false,
    }
}

/// Steps from `from` to every cell (u32::MAX: not reachable).
fn bfs(w: &World, from: (i32, i32)) -> Vec<u32> {
    let mut dist = vec![u32::MAX; MAX_SIDE * MAX_SIDE];
    let mut q = VecDeque::new();
    dist[idx(from)] = 0;
    q.push_back(from);
    while let Some((x, y)) = q.pop_front() {
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let n = (x + dx, y + dy);
            if n.0 < 0 || n.1 < 0 || n.0 >= w.w || n.1 >= w.h {
                continue;
            }
            if passable(w, n.0, n.1) && dist[idx(n)] == u32::MAX {
                dist[idx(n)] = dist[idx((x, y))] + 1;
                q.push_back(n);
            }
        }
    }
    dist
}

fn idx(c: (i32, i32)) -> usize {
    c.1 as usize * MAX_SIDE + c.0 as usize
}

fn choose_goal(w: &World, dist: &[u32], greedy: bool) -> Option<Goal> {
    fn consider(best: &mut Option<(u32, Goal)>, d: u32, g: Goal) {
        if d != u32::MAX && best.is_none_or(|(b, _)| d < b) {
            *best = Some((d, g));
        }
    }
    let mut best: Option<(u32, Goal)> = None;
    for t in w.things() {
        if !t.alive || t.kind.creature() {
            continue;
        }
        let c = (t.x >> SHIFT, t.y >> SHIFT);
        let wanted = match t.kind {
            Kind::Crank => true,
            Kind::Flask => greedy && w.life < 3,
            _ => greedy,
        };
        if wanted {
            consider(&mut best, dist[idx(c)], Goal::Cell(c));
        }
    }
    if let Some((_, g)) = best {
        return Some(g);
    }
    // The lift (with the crank), or a switch still off.
    for cy in 0..w.h {
        for cx in 0..w.w {
            let c = w.at(cx, cy);
            let want = (c == cell::LIFT && w.crank) || c == cell::SW_A || c == cell::SW_B;
            if !want {
                continue;
            }
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let s = (cx + dx, cy + dy);
                if s.0 >= 0 && s.1 >= 0 && s.0 < w.w && s.1 < w.h && w.at(s.0, s.1) == cell::FLOOR {
                    // The lift first when both can be reached.
                    let bias = if c == cell::LIFT { 0 } else { 1000 };
                    let d = dist[idx(s)];
                    if d != u32::MAX {
                        consider(&mut best, d + bias, Goal::Use(s, (cx, cy)));
                    }
                }
            }
        }
    }
    best.map(|(_, g)| g)
}

/// Walks along the shortest path to `target`; at the end, faces `wall`
/// and presses A.
fn walk_to(w: &World, dist: &[u32], target: (i32, i32), wall: Option<(i32, i32)>) -> u32 {
    let me = (w.x >> SHIFT, w.y >> SHIFT);
    // The path back from the target, by the distances from me.
    let back = bfs(w, target);
    let _ = dist;
    let mut aim = (centre(target.0), centre(target.1));
    let mut next = target;
    if me != target {
        // The neighbour of me closest to the target.
        let mut best = u32::MAX;
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let n = (me.0 + dx, me.1 + dy);
            if n.0 < 0 || n.1 < 0 || n.0 >= w.w || n.1 >= w.h {
                continue;
            }
            let b = back[idx(n)];
            if b < best && passable(w, n.0, n.1) {
                best = b;
                next = n;
            }
        }
        aim = (centre(next.0), centre(next.1));
        // Off the middle of my own cell sideways: first back to the
        // middle line, so corners are not cut into walls.
        let (mx, my) = (centre(me.0), centre(me.1));
        if next.0 != me.0 && (w.y - my).abs() > ONE / 5 {
            aim = (w.x + (next.0 - me.0) * ONE / 3, my);
        }
        if next.1 != me.1 && (w.x - mx).abs() > ONE / 5 {
            aim = (mx, w.y + (next.1 - me.1) * ONE / 3);
        }
    }
    // A shut door next: face it and press A, then wait for it.
    let door_shut = is_door(w.at(next.0, next.1)) && w.open_at(next.0, next.1) < PASS_OPEN;
    if door_shut && next != me {
        let want = angle_to(centre(next.0) - w.x, centre(next.1) - w.y);
        let t = turn_to(w.angle, want, 900);
        if t != 0 {
            return t;
        }
        return if w.open_at(next.0, next.1) == 0 {
            button::A
        } else {
            0
        };
    }
    if me == target {
        let Some(wall) = wall else {
            // On the item's cell: walk to its middle.
            return go_towards(w, (centre(target.0), centre(target.1)));
        };
        // Face the wall cell and press A.
        let want = angle_to(centre(wall.0) - w.x, centre(wall.1) - w.y);
        let t = turn_to(w.angle, want, 900);
        if t != 0 {
            return t;
        }
        // Close enough to the wall?
        let far = (centre(wall.0) - w.x)
            .abs()
            .max((centre(wall.1) - w.y).abs());
        if far > ONE + ONE / 8 {
            return button::UP;
        }
        return button::A;
    }
    go_towards(w, aim)
}

/// Turn towards a point, walking when roughly facing it.
fn go_towards(w: &World, p: (i32, i32)) -> u32 {
    let (dx, dy) = (p.0 - w.x, p.1 - w.y);
    if dx.abs() < 300 && dy.abs() < 300 {
        return 0;
    }
    let want = angle_to(dx, dy);
    let diff = want.wrapping_sub(w.angle) as i16;
    let mut b = 0;
    if diff.unsigned_abs() > 700 {
        b |= if diff < 0 {
            button::LEFT
        } else {
            button::RIGHT
        };
    }
    if diff.unsigned_abs() < 6000 {
        b |= button::UP;
    }
    b
}

/// LEFT or RIGHT until within `slack` of `want`; 0 when there.
fn turn_to(now: u16, want: u16, slack: u16) -> u32 {
    let diff = want.wrapping_sub(now) as i16;
    if diff.unsigned_abs() <= slack {
        0
    } else if diff < 0 {
        button::LEFT
    } else {
        button::RIGHT
    }
}

/// A creature that would hurt (or drink) within 3 cells, in the
/// flash's cone and in sight.
fn creature_ahead(w: &World) -> bool {
    let (c, s) = (cos(w.angle), sin(w.angle));
    w.things().iter().any(|t| {
        if !t.alive || !t.kind.creature() || t.mood != Mood::Normal {
            return false;
        }
        let (dx, dy) = (t.x - w.x, t.y - w.y);
        if dx.abs() > 3 * ONE || dy.abs() > 3 * ONE {
            return false;
        }
        let depth = (c * dx + s * dy) >> TRIG_SHIFT;
        let side = (c * dy - s * dx) >> TRIG_SHIFT;
        depth > 0 && side.abs() < depth && w.clear_line(w.x, w.y, t.x, t.y)
    })
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
        .join("../../.local/games/3d")
        .join(sub);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// The recorded held buttons as a script for the simulator
/// (`boards/desktop`, `--script`): "frame press|release NAME" lines,
/// one call a frame.
pub fn sim_script(record: &[u32]) -> String {
    const NAMES: [(u32, &str); 8] = [
        (button::UP, "up"),
        (button::DOWN, "down"),
        (button::LEFT, "left"),
        (button::RIGHT, "right"),
        (button::A, "a"),
        (button::B, "b"),
        (button::L, "l"),
        (button::R, "r"),
    ];
    let mut out = String::new();
    let mut was = 0u32;
    for (i, &h) in record.iter().enumerate() {
        for (bit, name) in NAMES {
            if h & bit != 0 && was & bit == 0 {
                out.push_str(&format!("{i} press {name}\n"));
            }
            if h & bit == 0 && was & bit != 0 {
                out.push_str(&format!("{i} release {name}\n"));
            }
        }
        was = h;
    }
    out
}

/// Cell of a position (for messages).
pub fn cell_of(w: &World) -> (i32, i32) {
    (w.x >> SHIFT, w.y >> SHIFT)
}
