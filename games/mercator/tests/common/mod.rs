//! Shared by the tests: driving the game call by call, a pilot that
//! flies, fights and trades through the world's own controls, pictures.
#![allow(dead_code)]

pub mod below;

use std::path::{Path, PathBuf};

use game_kit::{Frame, H, Touch, W};
use mercator::app::{App, CARD_WAIT, Keys, Scene};
use mercator::galaxy::{self, SYSTEMS};
use mercator::market::{self, GOODS};
use mercator::world::{self, Kind, MAX_SPEED, Mode, PLAYER, Pad, State, TICK_US, World};

/// Game time of one call at 60 calls a second.
pub const CALL_US: u64 = TICK_US as u64;

/// The game, driven call by call with its own picture.
pub struct Driver {
    pub app: Box<App>,
    pub frame: Box<Frame>,
    pub now_us: u64,
    pub call_us: u64,
    pub held: u32,
    pub events: Vec<mercator::app::Event>,
}

impl Driver {
    pub fn new() -> Driver {
        let mut d = Driver {
            app: Box::new(App::new()),
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

    /// Idle until the card on screen takes a press.
    pub fn wait_card(&mut self) {
        for _ in 0..400 {
            if self.app.scene_t >= CARD_WAIT {
                return;
            }
            self.hold(0);
        }
        panic!("the card never took a press");
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

    /// Title, PLAY: a new career, docked.
    pub fn start(&mut self) {
        assert_eq!(self.app.scene, Scene::Title);
        self.app.sel = 0;
        self.tap(game_kit::button::A);
        assert_eq!(self.app.scene, Scene::Docked);
    }

    /// A hash of the picture (for determinism checks).
    pub fn picture_hash(&self) -> u64 {
        let mut h = 0xCBF2_9CE4_8422_2325u64;
        for &b in self.frame.0.iter() {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3);
        }
        h
    }
}

// ---- The pilot --------------------------------------------------------------------

/// The stick that turns the ship towards `to` (what the auto-dock does).
pub fn steer_to(w: &World, to: [i32; 3]) -> (i32, i32) {
    let (p, y) = world::aim_at(&w.me.rot, w.me.pos, to, 240);
    (
        (y * 256 / 190).clamp(-256, 256),
        (p * 256 / 230).clamp(-256, 256),
    )
}

/// The nearest ship hunting the player within `r`.
pub fn hunter(w: &World, r: i32) -> Option<usize> {
    let mut best = None;
    let mut d = i64::MAX;
    for (i, s) in w.ships.iter().enumerate() {
        let Some(s) = s else { continue };
        let hunts = (s.target == PLAYER && s.mode != Mode::Travel) || s.angry;
        if !hunts || !world::within(s.pos, w.me.pos, r) {
            continue;
        }
        let e = mercator::fixed::sub(s.pos, w.me.pos);
        let dd = mercator::fixed::dot(e, e);
        if dd < d {
            d = dd;
            best = Some(i);
        }
    }
    best
}

/// One tick of fighting the nearest hunter: turn to it, fire when it is
/// in the sights and the laser is not hot, keep a fighting speed. The
/// hunter itself counts as in the sights even when it is a warden after
/// a wanted pilot (step M4: before, the pilot never fired at a warden,
/// and a warden hunting it made a stalemate no trip ever left).
pub fn fight_pad(w: &World, i: usize) -> Pad {
    let s = w.ships[i].unwrap();
    let steer = steer_to(w, s.pos);
    let sights = w.foe_in_sights() || w.first_hit().is_some_and(|(j, _)| j == i);
    Pad {
        steer,
        roll: 0,
        throttle: if w.me.speed > MAX_SPEED / 2 { -256 } else { 64 },
        fire: sights && clear_shot(w, i) && w.me.heat < world::HEAT_MAX * 8 / 10,
        cruise: w.me.cruise,
        auto: false,
        turret: false,
        boost: false,
    }
}

/// No warden or calm hauler within twice the laser's hit reach of the
/// line ahead (step F1: a hauler now outlives a stray shot, and the
/// later launch point once put a warden into a shot at a raider; a
/// careful pilot holds fire while one is that near the line). The
/// ship fought, `target`, does not count (step J1: so step M4's pilot
/// still fires at a warden hunting it).
pub fn clear_shot(w: &World, target: usize) -> bool {
    w.ships.iter().enumerate().all(|(k, s)| {
        let Some(s) = s.filter(|_| k != target) else {
            return true;
        };
        let calm = matches!(s.kind, Kind::Warden | Kind::Hauler) && !s.angry;
        let l = w.me.rot.apply_t(mercator::fixed::sub(s.pos, w.me.pos));
        if !calm || l[2] <= 0 || !w.in_reach(s.pos) {
            return true;
        }
        // Within the laser's reach: the products fit in i64.
        let reach = i64::from(s.kind.size() + l[2] / 40) * 2;
        let side = i64::from(l[0]) * i64::from(l[0]) + i64::from(l[1]) * i64::from(l[1]);
        side > reach * reach
    })
}

/// The nearest raider's TAG within `r` (a canister whose load is
/// `world::TAG`).
pub fn tag_near(w: &World, r: i32) -> Option<usize> {
    let mut best = None;
    let mut d = i64::MAX;
    for (i, s) in w.ships.iter().enumerate() {
        let Some(s) = s else { continue };
        if s.kind != Kind::Canister || s.load.0 != world::TAG || !world::within(s.pos, w.me.pos, r)
        {
            continue;
        }
        let e = mercator::fixed::sub(s.pos, w.me.pos);
        let dd = mercator::fixed::dot(e, e);
        if dd < d {
            d = dd;
            best = Some(i);
        }
    }
    best
}

/// One tick of flying to TAG `i` to scoop it: auto-dock and cruise
/// off, turn to it (slowly while it is not ahead, so the turn is
/// tight), slow down near it.
pub fn fetch_pad(w: &World, i: usize) -> Pad {
    let s = w.ships[i].unwrap();
    let ahead = w.me.rot.apply_t(mercator::fixed::norm(
        mercator::fixed::sub(s.pos, w.me.pos),
        1 << 14,
    ))[2]
        > (1 << 14) * 19 / 20;
    let near = world::within(s.pos, w.me.pos, 600 * world::M);
    // Step M5: the ship's own top speed (MAX_SPEED for the LARK).
    let want = if !ahead {
        100
    } else if near {
        400
    } else {
        w.top()
    };
    Pad {
        steer: steer_to(w, s.pos),
        throttle: if w.me.speed > want { -256 } else { 256 },
        cruise: w.me.cruise,
        auto: w.me.auto,
        ..Pad::default()
    }
}

/// What the pilot's trips saw of the bounty rules (world events).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    /// Raiders the player downed: each dropped a TAG.
    pub dropped: u32,
    /// TAGs scooped.
    pub scooped: u32,
    /// TAGs paid for at a dock, and the credits paid.
    pub paid: u32,
    pub paid_cr: u32,
    /// Docks with TAGs where none were paid (LAWLESS or OUTLAW).
    pub unpaid_docks: u32,
    /// TAGs carried when the ship was lost.
    pub lost: u32,
}

/// What a trip ended in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trip {
    Docked { ticks: u32, kills: u32 },
    Died { ticks: u32 },
    TimedOut,
}

/// Flies from the dock to system `to` and docks there: out at full
/// speed and cruise until a jump is allowed, jump, then the auto-dock;
/// any hunter is fought first, then any raider's TAG within 3 km is
/// fetched (for at most two minutes a trip). World level (no tunnel, no menus).
pub fn trip(w: &mut World, to: u8, max_ticks: u32) -> Trip {
    trip_tally(w, to, max_ticks, &mut Tally::default())
}

/// [`trip`], counting what happened to the TAGs into `tally`.
pub fn trip_tally(w: &mut World, to: u8, max_ticks: u32, tally: &mut Tally) -> Trip {
    trip_hunt(w, to, max_ticks, tally, 0)
}

/// Step M5: [`trip_tally`] for a bounty hunter: after the jump it
/// first waits up to `linger` ticks out at the arrival point (drifting
/// at a quarter of its top speed) for raiders to come, fighting them
/// and fetching their TAGs as on any trip, then auto-docks. `linger`
/// 0 is exactly [`trip_tally`].
pub fn trip_hunt(w: &mut World, to: u8, max_ticks: u32, tally: &mut Tally, linger: u32) -> Trip {
    assert_eq!(w.state, State::Docked);
    let mut linger = linger;
    w.launch();
    let kills0 = u32::from(w.car.kills);
    let mut jumped = to == w.car.system;
    // At most two minutes of fetching a trip: in a raider system new
    // raiders keep coming, and a pilot who fetches every TAG never
    // leaves.
    let mut fetch_left = 60 * 120;
    for t in 0..max_ticks {
        if w.state == State::Dead {
            tally.lost += u32::from(w.car.tags);
            return Trip::Died { ticks: t };
        }
        if w.state == State::Docked {
            return Trip::Docked {
                ticks: t,
                kills: u32::from(w.car.kills) - kills0,
            };
        }
        let tag =
            tag_near(w, 3 * world::KM).filter(|_| w.car.tags < world::TAGS_MAX && fetch_left > 0);
        let pad = if let Some(i) = hunter(w, 6 * world::KM) {
            fight_pad(w, i)
        } else if let Some(i) = tag {
            fetch_left -= 1;
            fetch_pad(w, i)
        } else if !jumped {
            if w.can_jump(to).is_ok() {
                w.jump(to);
                jumped = true;
                Pad::default()
            } else {
                // Straight out of the mouth; cruise once allowed.
                Pad {
                    throttle: 256,
                    // Step M5: at the ship's own top speed.
                    cruise: !w.me.cruise && w.me.throttle >= w.top() && w.room() > 0,
                    ..Pad::default()
                }
            }
        } else if linger > 0 {
            linger -= 1;
            Pad {
                throttle: if w.me.speed > w.top() / 4 { -256 } else { 0 },
                cruise: w.me.cruise,
                ..Pad::default()
            }
        } else {
            Pad {
                auto: !w.me.auto && t % 30 == 0,
                ..Pad::default()
            }
        };
        let tags = u32::from(w.car.tags);
        w.tick(&pad);
        while let Some(e) = w.take_event() {
            match e {
                world::Event::Killed { kind: Kind::Raider } => tally.dropped += 1,
                world::Event::Scooped {
                    good: world::TAG, ..
                } => tally.scooped += 1,
                world::Event::Bounty { sum } => {
                    tally.paid += tags;
                    tally.paid_cr += sum;
                }
                world::Event::Unpaid { .. } => tally.unpaid_docks += 1,
                _ => {}
            }
        }
    }
    Trip::TimedOut
}

/// The best single good to carry from here to `to`: (good, profit a
/// tonne at tomorrow's prices there).
pub fn best_good(w: &World, to: u8) -> Option<(usize, i64)> {
    let there = galaxy::system(w.car.seed, to);
    let day = w.car.day.saturating_add(1);
    (0..GOODS)
        .filter(|&g| w.stock[g] > 0)
        .map(|g| {
            let p = i64::from(market::sell_price(&there, g, day)) - i64::from(w.price(g));
            (g, p)
        })
        .max_by_key(|&(_, p)| p)
}

/// The trader's choice: the reachable system (with its danger at most
/// `max_danger`) with the best profit a tonne, and that good.
pub fn plan(w: &World, max_danger: u8) -> Option<(u8, usize, i64)> {
    let here = w.sys;
    let mut best: Option<(u8, usize, i64)> = None;
    for i in 0..SYSTEMS {
        let s = galaxy::system(w.car.seed, i as u8);
        if s.id == here.id || here.dist(&s) > u32::from(w.car.fuel) || s.danger > max_danger {
            continue;
        }
        if let Some((g, p)) = best_good(w, s.id)
            && best.is_none_or(|b| p > b.2)
        {
            best = Some((s.id, g, p));
        }
    }
    best
}

/// [`plan`] among safe systems, or any system when none is in reach.
pub fn plan_any(w: &World, max_danger: u8) -> (u8, usize, i64) {
    plan(w, max_danger)
        .or_else(|| plan(w, 3))
        .expect("no system in reach")
}

/// Sells everything, repairs, fills the tank.
pub fn sell_all(w: &mut World) {
    for g in 0..GOODS {
        while w.sell(g) {}
    }
    w.repair();
    w.buy_fuel();
}

/// Buys as much of `g` as room, stock and money allow (keeping
/// `keep` credits).
pub fn buy_max(w: &mut World, g: usize, keep: u32) -> u32 {
    let mut n = 0;
    while w.car.credits >= keep + w.price(g) && w.buy(g) {
        n += 1;
    }
    n
}

/// Kinds of ship near the player (for checks).
pub fn count(w: &World, k: Kind) -> usize {
    w.ships.iter().flatten().filter(|s| s.kind == k).count()
}

// ---- Pictures ----------------------------------------------------------------------

/// Saves the picture as PNG, every pixel `scale` x `scale`, with the
/// game's palette (stored, no compression: no outside crates needed).
pub fn save_frame(path: &Path, f: &Frame, colours: &[u16], scale: usize) {
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
            raw.extend_from_slice(&rgb(*colours.get(i).unwrap_or(&0xF81F)));
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

/// Where the tests leave pictures (git-ignored, outside the crate:
/// `.local/mercator/shots/` at the repository root), made if missing.
pub fn shots() -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/mercator/shots");
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Saves the driver's picture under `name`.png in the shots folder.
pub fn shot(d: &mut Driver, name: &str) {
    let c = d.app.colours().to_vec();
    save_frame(&shots().join(format!("{name}.png")), &d.frame, &c, 3);
}
