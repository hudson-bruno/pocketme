//! The 3D view: stars, the sun, the planet, the station, the ships,
//! dust, laser beams and explosions, drawn far to near.
//!
//! **Huge and far things.** The planet is kilometres wide and the sun
//! hundreds of kilometres away; the renderer's sums are made for a few
//! kilometres. So a body farther than [`CAP`] is drawn at `CAP`, made
//! smaller by the same amount: on screen it looks exactly the same
//! (perspective only sees the ratio of size to distance). A body
//! smaller than a pixel is drawn as a dot. Ships are drawn where they
//! are, in true perspective (step M2: no distance floor), and as a 2x2
//! dot of their kind's colour when their ball is under 3 pixels (with
//! the dot under the model up to 6 pixels).
//!
//! **Order.** Everything is sorted by its true distance, far first. A
//! ship or the station hidden behind the planet's ball is drawn before
//! the planet, so the planet covers it.

use game_kit::{Frame, W};

use crate::art::{self, mat};
use crate::fixed::{Mat3, V3, cos, dot, hash, isqrt, length, norm, scale14, sin, sub, wide};
use crate::fx;
use crate::models::{self, SHIP_SCALE};
use crate::poly::{
    Camera, Model, NEAR, Object, Rect, Renderer, SUB, Shading, View, bolt, fill_tri, line,
    rect_fill,
};
use crate::world::{self, BLAST_TICKS, Kind, M, Ship, State, World, add};

/// Bodies farther than this are drawn at this distance, made smaller.
pub const CAP: i32 = 4 * world::KM;
/// Other ships farther than this are only on the radar.
pub const SHIP_SEEN: i32 = 7 * world::KM;
/// Before step M2 other ships were drawn three times their table size;
/// the size steps below ([`marks_of`]) count sixteenths of that.
const OLD_16TH: i32 = SHIP_SCALE * 3 / 16;
/// A fighter's size at 16/16 (step M2: [`world::FIGHTER_X`] times the
/// size before; step F1 drew the courier [`world::ME_X`] times bigger
/// too, about 4/5 of a raider's length); the hit sizes
/// ([`Kind::size`]) match. Debris is drawn at this size.
pub const NPC_SCALE: i32 = SHIP_SCALE * 3 * world::FIGHTER_X;
/// A ship whose ball is smaller than this on the screen (radius, in
/// pixels) is drawn as a 2x2 dot of its kind's colour; under twice
/// this the dot is drawn under the model.
pub const SHIP_DOT: i64 = 3;
/// The flight view: everything above the panel.
pub const VIEW: View = View::new(Rect::new(0, 0, 160, 96), 96);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    Low,
    Mid,
    High,
}

impl Detail {
    pub const ALL: [Detail; 3] = [Detail::Low, Detail::Mid, Detail::High];

    pub fn name(self) -> &'static str {
        match self {
            Detail::Low => "LOW",
            Detail::Mid => "MID",
            Detail::High => "HIGH",
        }
    }

    #[inline(never)]
    fn stars(self) -> usize {
        match self {
            Detail::Low => 24,
            Detail::Mid => 44,
            Detail::High => STARS,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eye {
    /// From the pilot's seat.
    Cockpit,
    /// From behind the ship.
    Chase,
}

impl Eye {
    pub const ALL: [Eye; 2] = [Eye::Cockpit, Eye::Chase];

    pub fn name(self) -> &'static str {
        match self {
            Eye::Cockpit => "COCKPIT",
            Eye::Chase => "CHASE",
        }
    }
}

/// The shading for this system.
#[inline(never)]
pub fn shading(w: &World) -> Shading {
    Shading {
        mats: art::MATS,
        shades: art::SHADES,
        light: w.bodies.light,
        glow: art::GLOW,
        fog_from: 0,
        fog_to: 0,
        fog_shades: 0,
        clear: 0,
        dark_side: 7,
    }
}

const STARS: usize = 64;
/// Star directions, 16000 long, the same in every system.
static STAR_DIRS: [[i16; 3]; STARS] = star_dirs();

const fn star_dirs() -> [[i16; 3]; STARS] {
    let mut s = [[0i16; 3]; STARS];
    let mut k = 0;
    while k < STARS {
        let h = hash(0x51A2, k as u32);
        let g = hash(h, 7);
        let v = [
            (h % 2001) as i64 - 1000,
            ((h >> 11) % 2001) as i64 - 1000,
            (g % 2001) as i64 - 1000,
        ];
        let l = isqrt((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) as u64) as i64;
        let l = if l == 0 { 1 } else { l };
        s[k] = [
            (v[0] * 16000 / l) as i16,
            (v[1] * 16000 / l) as i16,
            (v[2] * 16000 / l) as i16,
        ];
        k += 1;
    }
    s
}

/// The camera for the player's view.
#[inline(never)]
pub fn camera(w: &World, eye: Eye) -> Camera {
    let me = &w.me;
    let mut rot = me.rot;
    // Under fire the view shakes a little (no dice: the tick's hash).
    if fx::SHAKE_ON && w.fx.jolt > 0 && w.state != State::Docked {
        // jolt below 32: a below 2^11.
        let a = u32::from(w.fx.jolt).wrapping_mul(40);
        let h = hash(w.ticks, 0x5A4E);
        let k = |h: u32| (h % a.wrapping_mul(2).wrapping_add(1)) as i32 - a as i32;
        rot = rot.turned(k(h), k(h >> 16), 0);
    }
    let pos = match eye {
        Eye::Cockpit => me.pos,
        // Step F1: back and up with my ship's size (ME_X below 8: below
        // 2^18 a part).
        Eye::Chase => add(
            add(me.pos, scale14(me.rot.col(2), -70 * M * world::ME_X)),
            scale14(me.rot.col(1), 16 * M * world::ME_X),
        ),
    };
    Camera {
        pos,
        rot: rot.transposed(),
    }
}

/// Where a point lands in the view, in whole pixels, if in front.
#[inline(never)]
pub fn on_screen(view: &View, cam: &Camera, p: V3) -> Option<(i32, i32)> {
    let c = cam.rot.apply(sub(p, cam.pos));
    if c[2] < NEAR {
        return None;
    }
    let s = view.project(c);
    Some((s[0].div_euclid(SUB), s[1].div_euclid(SUB)))
}

/// The palette number for a ship kind's dot and radar blip.
#[inline(never)]
pub fn blip(k: Kind) -> u8 {
    match k {
        Kind::Raider => art::BLIP_RAIDER,
        Kind::Hauler => art::BLIP_HAULER,
        Kind::Warden => art::BLIP_WARDEN,
        Kind::Canister => art::BLIP_CAN,
    }
}

/// The palette number for ship `s`'s dot, radar blip and bracket: its
/// kind's ([`blip`]); a raider's TAG blinks mint, the hit flash's colour
/// (step M3: no bolt is mint any more, see [`BEAM_LV`]; its bracket does
/// not blink: that is [`art::TAG_MARK`] at every distance).
#[inline(never)]
pub fn blip_of(s: &Ship) -> u8 {
    if s.load.0 == world::TAG && s.flash > 0 {
        art::BEAM_ME
    } else if s.load.0 == world::ROCK {
        art::TEXT_DIM
    } else {
        blip(s.kind)
    }
}

/// Step M3: a bolt's colour by its gun's level (`Beam::lv`, 0..=2), the
/// same for the player and every other ship: sky blue, white, red.
/// Existing palette entries, none of them the mint of a TAG's blink or
/// a hit flash.
pub const BEAM_LV: [u8; 3] = [art::ECON_LAB, art::STAR_BRIGHT, art::BEAM_FOE];

/// Step L3 switches: a second hull per role (its share per system),
/// and size and markings per ship. Looks only: hit sizes stay.
pub const HULLS_ON: bool = true;
pub const MARKS_ON: bool = true;

/// A ship's hull for this system (`h`, its number): the role's first
/// hull or its second, by the ship's `look` and the system's share
/// (a quarter, half or three quarters of the ships).
#[inline(never)]
pub fn model_of(s: &Ship, h: u32) -> &'static Model {
    let alt = HULLS_ON && u32::from(s.look & 3) <= (h >> 8) % 3;
    match (s.kind, alt) {
        (Kind::Raider, false) => &models::RAIDER,
        (Kind::Raider, true) => &models::HOOK,
        (Kind::Hauler, false) => &models::HAULER,
        (Kind::Hauler, true) => &models::TANKER,
        (Kind::Warden, false) => &models::WARDEN,
        (Kind::Warden, true) => &models::CUTTER,
        (Kind::Canister, _) => &models::CANISTER,
    }
}

/// A ship's drawn size in sixteenths of its size before step M2 (times
/// its growth, [`Kind::grown`]: haulers vary most) and its markings:
/// one material drawn as another, or (0, 0).
#[inline(never)]
fn marks_of(s: &Ship) -> (i32, (u8, u8)) {
    let g = s.kind.grown();
    if !MARKS_ON {
        return (16 * g, (0, 0));
    }
    let step = usize::from((s.look >> 2) & 3);
    let mark = usize::from((s.look >> 4) & 3);
    let (size, mark): (i32, _) = match s.kind {
        Kind::Hauler => (
            [16, 17, 18, 20][step],
            [
                (0, 0),
                (mat::HAULER2, mat::CANISTER),
                (mat::HAULER2, mat::PANEL),
                (mat::HAULER, mat::HULL),
            ][mark],
        ),
        Kind::Raider => (
            [15, 16, 16, 17][step],
            [
                (0, 0),
                (0, 0),
                (mat::RAIDER2, mat::GUNMETAL),
                (mat::RAIDER, mat::RAIDER2),
            ][mark],
        ),
        Kind::Warden => (
            [15, 16, 16, 17][step],
            [(0, 0), (0, 0), (0, 0), (mat::WARDEN2, mat::PANEL)][mark],
        ),
        Kind::Canister => (16, (0, 0)),
    };
    // At most 20 times 6.
    (size.wrapping_mul(g), mark)
}

/// A ship kind's main material (the one a hit flashes mint) and how
/// far behind its middle its engine is (world units, at 16/16 of the
/// size before step M2; the size steps scale it).
#[inline(never)]
fn main_mat(k: Kind) -> (u8, i32) {
    use art::mat;
    const U: i32 = OLD_16TH * 16 / 256;
    match k {
        Kind::Raider => (mat::RAIDER, 160 * U),
        Kind::Hauler => (mat::HAULER, 300 * U),
        Kind::Warden => (mat::WARDEN, 200 * U),
        Kind::Canister => (mat::CANISTER, 0),
    }
}

/// What glows round a body: the sun's corona (its turning by the tick),
/// the planet's air (towards the sun, in the camera's axes).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rim {
    None,
    Sun(u16),
    Air(V3),
}

/// One thing to draw, with its sort distance.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Thing {
    Sun,
    Planet,
    Station,
    Ship(u8),
    Me,
}

/// Whether the planet's ball hides `p` from the camera.
#[inline(never)]
pub(crate) fn behind_planet(w: &World, cam: &Camera, p: V3) -> bool {
    let s = sub(p, cam.pos);
    let c = sub([0; 3], cam.pos);
    let ls = i64::from(length(s)).max(1);
    // How far along the line of sight the planet's middle is.
    let t = dot(s, c) / ls;
    if t <= 0 || t >= ls {
        return false;
    }
    let r = i64::from(w.bodies.planet_r);
    // Squared distance of the middle from the line of sight.
    dot(c, c) - t * t < r * r
}

/// Draws the whole 3D view in `view` (the screen behind it is cleared).
#[inline(never)]
pub fn draw(f: &mut Frame, r: &mut Renderer, w: &World, d: Detail, view: &View, cam: &Camera) {
    rect_fill(f, &view.area, 0);
    let sh = shading(w);
    if fx::NEBULA_ON && d != Detail::Low {
        nebula(f, view, cam, w.sys.h);
    }
    // After a jump, in cruise and in overboost the stars stream out from the middle:
    // each is drawn as a streak away from it, `streak`/64 of its
    // distance long (below 64).
    let ctr = view.project([0, 0, M]);
    let streak = if fx::STREAKS_ON && w.state == State::Flying {
        // Step M4: short streaks while the overboost runs.
        let cruise = if w.me.cruise {
            (w.me.speed / 3000).min(12)
        } else if w.me.boost {
            6
        } else {
            0
        };
        i32::from(w.fx.warp.min(50)).wrapping_add(cruise)
    } else {
        0
    };
    // Stars: directions only, they move only when the ship turns.
    for (k, s) in STAR_DIRS.iter().take(d.stars()).enumerate() {
        let c = cam.rot.apply(s.map(i32::from));
        if c[2] <= 0 {
            continue;
        }
        let p = view.project(c);
        let col = if k % 7 == 0 {
            art::STAR_BRIGHT
        } else if k % 2 == 0 {
            art::STAR
        } else {
            art::STAR_DIM
        };
        if streak > 0 && c[2] > 2000 {
            // Pulled near the picture (in 1/16 pixel, below 2^17):
            // times `streak` stays far inside i32.
            let (x, y) = (near(p[0] / SUB), near(p[1] / SUB));
            let (cx, cy) = (ctr[0] / SUB, ctr[1] / SUB);
            let e = [
                x.wrapping_add(x.wrapping_sub(cx).wrapping_mul(streak) / 64),
                y.wrapping_add(y.wrapping_sub(cy).wrapping_mul(streak) / 64),
            ];
            line(f, &view.area, [x, y], e, col);
        } else {
            dot_px(f, &view.area, p[0] / SUB, p[1] / SUB, col, 1);
        }
    }
    // Sort far to near.
    let mut list = [(0i64, Thing::Me); 4 + world::SHIPS];
    let mut n = 0;
    let dist = |p: V3| {
        let d = sub(p, cam.pos);
        dot(d, d)
    };
    let b = &w.bodies;
    let planet_d = dist([0; 3]);
    // At most 4 + SHIPS things: n stays inside the list.
    let mut put = |key: i64, t: Thing| {
        list[n] = (key, t);
        n = n.wrapping_add(1);
    };
    put(dist(b.sun_pos), Thing::Sun);
    put(planet_d, Thing::Planet);
    let hid = |p: V3, d: i64| {
        if behind_planet(w, cam, p) {
            d.max(planet_d.saturating_add(1))
        } else {
            d
        }
    };
    put(hid(b.station_pos, dist(b.station_pos)), Thing::Station);
    for (i, s) in w.ships.iter().enumerate() {
        if let Some(s) = s
            && world::within(s.pos, cam.pos, SHIP_SEEN)
        {
            put(hid(s.pos, dist(s.pos)), Thing::Ship(i as u8));
        }
    }
    if w.state == State::Flying && cam.pos != w.me.pos {
        put(dist(w.me.pos), Thing::Me);
    }
    // Far to near (insertion sort: a dozen things; the library's sort
    // costs far more code).
    let list = &mut list[..n];
    for i in 1..list.len() {
        let mut j = i;
        while j > 0 && list[j - 1].0 < list[j].0 {
            list.swap(j - 1, j);
            j -= 1;
        }
    }
    let ship_sh = Shading { dark_side: 4, ..sh };
    for &(_, t) in list.iter() {
        let mut swap = (0, 0);
        let mut engine = 0;
        // The engine trail: how far behind the middle it starts, and its
        // length (0: none).
        let mut tail = (0, 0);
        // Step M3: its smoke level and look (the puffs' phase).
        let (mut fume, mut look) = (0u8, 0u8);
        let (model, pos, rot, scale, dot_c) = match t {
            Thing::Sun => (
                &models::SUN_BALL,
                b.sun_pos,
                Mat3::IDENTITY,
                ball_scale(b.sun_r),
                art::FAR_SUN,
            ),
            Thing::Planet => {
                let giant = w.sys.look == 4;
                let m = match (d, giant) {
                    (Detail::Low, false) => &models::PLANET_LO,
                    (Detail::Low, true) => &models::GIANT_LO,
                    (_, false) => &models::PLANET_HI,
                    (_, true) => &models::GIANT_HI,
                };
                (
                    m,
                    [0; 3],
                    b.planet_rot,
                    ball_scale(b.planet_r),
                    art::FAR_PLANET,
                )
            }
            Thing::Station => (
                &models::STATION,
                b.station_pos,
                // Step M3: docked, the station keeps turning (`show`).
                b.station_rot.mul(&Mat3::rotation(
                    0,
                    0,
                    b.spin.wrapping_add((w.show as u16).wrapping_mul(DOCK_SPIN)),
                )),
                SHIP_SCALE * world::STATION_X,
                art::BLIP_STATION,
            ),
            Thing::Ship(i) => match w.ships[usize::from(i)] {
                Some(s) => {
                    let rock = s.kind == Kind::Canister && s.load.0 == world::ROCK;
                    let (m, back) = if rock {
                        (mat::STATION2, 0)
                    } else {
                        main_mat(s.kind)
                    };
                    let (size, mark) = if rock {
                        // 28-42 sixteenths, times SMALL_X (step M2);
                        // ICE rocks show bluish faces.
                        let ice = s.load.1 == 1;
                        (
                            (28 + 2 * i32::from((s.look >> 2) & 7)) * world::SMALL_X,
                            if ice {
                                (mat::STATION2, mat::GLASS)
                            } else {
                                (0, 0)
                            },
                        )
                    } else {
                        marks_of(&s)
                    };
                    swap = mark;
                    if s.flash > 0 {
                        swap = (m, art::mat::FLASH);
                    }
                    // Below 2^14 and 2^7.
                    let back = back * size / 16;
                    engine = back;
                    if s.kind != Kind::Canister {
                        // Ship speeds below 2^11; the longest trail
                        // grows with the ship (step M2).
                        let most = 140 * M * s.kind.grown();
                        tail = (back, s.speed.wrapping_mul(20).min(most));
                        fume = fx::smoke_of(s.hp, s.kind.hull(w.sys.danger));
                        look = s.look;
                    }
                    // A TAG is one colour all round, so its blink (the
                    // flash) shows from any side.
                    let (model, rot) = if s.load.0 == world::TAG {
                        (&models::TAG_BOX, s.rot)
                    } else if rock {
                        // Tumbling slowly, each at its own rate.
                        let spin = (w.ticks as u16).wrapping_mul(24 + u16::from(s.look >> 5));
                        (&models::ROCK, s.rot.mul(&Mat3::rotation(spin, spin / 3, 0)))
                    } else {
                        (model_of(&s, w.sys.h), s.rot)
                    };
                    // Size at most 120: the scale at most 92,160.
                    (model, s.pos, rot, OLD_16TH * size, blip_of(&s))
                }
                None => continue,
            },
            Thing::Me => {
                // Speeds below 2^17, ME_X below 8: below 2^23 (the
                // trail's length below 2^16). Step F1: my ship and its
                // trail at ME_X times their old size; step M3's smoke
                // starts at that tail. Step M5: the ship flown, its tail
                // (at most 300 model units) and its own hull.
                let x = world::ME_X;
                let c = w.car.craft();
                tail = (
                    c.tail * 16 * x,
                    w.me.speed.wrapping_mul(8 * x).min(30 * M * x),
                );
                fume = fx::smoke_of(i32::from(w.car.hull), w.car.hull_max());
                (
                    models::craft_model(w.car.ship),
                    w.me.pos,
                    w.me.rot,
                    SHIP_SCALE * x,
                    0,
                )
            }
        };
        // Ships and the station keep some light on their night side
        // (they must be seen against black space); bodies do not.
        let body = matches!(t, Thing::Sun | Thing::Planet);
        let sh = Shading {
            dark_side: if body { 7 } else { 4 },
            ..sh
        };
        let o = Object {
            model,
            pos,
            rot,
            scale,
            swap,
        };
        // Ships (all within SHIP_SEEN) are drawn where they are.
        let cap = if matches!(t, Thing::Ship(_)) {
            SHIP_SEEN
        } else {
            CAP
        };
        let rim = match t {
            Thing::Sun => Rim::Sun(w.ticks.wrapping_add(w.show) as u16),
            Thing::Planet if fx::AIR_ON => Rim::Air(cam.rot.apply(b.light.map(i32::wrapping_neg))),
            _ => Rim::None,
        };
        // A trail behind a ship coming at the eye is hidden by it: drawn
        // first. Otherwise it is drawn over the ship.
        let tail_on = fx::TRAILS_ON && tail.1 > 4 * M;
        // Only ships come here (within SHIP_SEEN of the eye): the dot is
        // within its bounds.
        let tail_first = tail_on && dot(rot.col(2), sub(pos, cam.pos)) < 0;
        if tail_on && tail_first {
            trail(f, view, cam, pos, rot.col(2), tail);
        }
        let drawn = far_body(f, r, view, cam, &sh, &o, dot_c, rim, cap);
        if tail_on && !tail_first {
            trail(f, view, cam, pos, rot.col(2), tail);
        }
        if engine > 0 {
            glow(f, view, cam, &o, drawn, engine);
        }
        if fume > 0 {
            // Step J1: my ship's smoke grows with it (F1's ME_X).
            let grow = if t == Thing::Me { world::ME_X } else { 1 };
            smoke(
                f,
                view,
                cam,
                pos,
                rot.col(2),
                tail.0,
                grow,
                fume,
                w.ticks,
                look,
            );
        }
        if t == Thing::Station {
            station_lights(f, view, cam, w, &rot);
        }
    }
    if fx::DEBRIS_ON {
        for bit in
            w.fx.bits
                .iter()
                .filter(|b| b.age < fx::BIT_LIFE && world::within(b.at, cam.pos, SHIP_SEEN))
        {
            // Glowing hot at first, charred at the end.
            let m = match bit.age {
                0..12 => mat::ENGINE,
                12..70 => bit.mat,
                _ => mat::GUNMETAL,
            };
            let o = Object {
                model: &models::SHARD,
                pos: add(bit.at, bit.off),
                rot: Mat3::rotation(bit.ang[0], bit.ang[1], 0),
                scale: NPC_SCALE,
                swap: (mat::RAIDER, m),
            };
            r.draw(f, view, cam, &ship_sh, &o, 3 * CAP);
        }
    }
    for s in
        w.fx.sparks
            .iter()
            .filter(|s| s.age < s.life && world::within(s.at, cam.pos, SHIP_SEEN))
    {
        let a = add(s.at, s.off);
        let e = sub(a, s.vel.map(|v| i32::from(v).wrapping_mul(2)));
        let (age, life) = (u32::from(s.age).wrapping_mul(3), u32::from(s.life));
        let col = if age < life {
            art::FIRE1
        } else if age < life.wrapping_mul(2) {
            art::FIRE2
        } else {
            art::FIRE3
        };
        seg(f, view, cam, a, e, col, false);
    }
    if d != Detail::Low && w.state == State::Flying {
        dust(f, view, cam, w, d);
    }
    for bm in w.beams.iter().filter(|b| b.age > 0) {
        beam(f, view, cam, bm, BEAM_LV[usize::from(bm.lv.min(2))]);
    }
    for bl in w.blasts.iter().filter(|b| b.age < BLAST_TICKS) {
        blast(f, view, cam, bl.pos, bl.age, bl.size);
    }
    if fx::FLARE_ON && d != Detail::Low {
        flare(f, view, cam, w);
    }
    if fx::SHAKE_ON && w.fx.jolt > 0 && w.state == State::Flying {
        hit_mark(f, view, cam, w);
    }
    if fx::SMOKE_ON && w.state == State::Flying && cam.pos == w.me.pos {
        let lv = fx::smoke_of(i32::from(w.car.hull), w.car.hull_max());
        if lv > 0 {
            wisps(f, view, lv, w.ticks);
        }
    }
}

/// A line between two points in space, left out when either is closer
/// than 4 m or behind the eye.
#[inline(never)]
fn seg(f: &mut Frame, view: &View, cam: &Camera, a: V3, b: V3, c: u8, thick: bool) {
    let (a, b) = (
        cam.rot.apply(sub(a, cam.pos)),
        cam.rot.apply(sub(b, cam.pos)),
    );
    if a[2] < 4 * M || b[2] < 4 * M {
        return;
    }
    let (pa, pb) = (view.project(a), view.project(b));
    bolt(
        f,
        &view.area,
        [pa[0] / SUB, pa[1] / SUB],
        [pb[0] / SUB, pb[1] / SUB],
        c,
        (0, 8),
        thick,
    );
}

/// An engine trail behind a ship drawn at `at` with its nose along
/// `nose`: `tail.0` behind the middle, `tail.1` long (below 2^16), in
/// three steps fading from the engine's bright orange to its dark red.
#[inline(never)]
fn trail(f: &mut Frame, view: &View, cam: &Camera, at: V3, nose: V3, tail: (i32, i32)) {
    let e = add(at, scale14(nose, -tail.0));
    let v = scale14(nose, -tail.1);
    // Parts below 2^16: times 3 stays inside i32.
    let p = |k: i32| add(e, v.map(|c| c.wrapping_mul(k) / 3));
    for (k, shade) in [1u8, 3, 5].into_iter().enumerate() {
        let k = k as i32;
        let c = shade.wrapping_mul(art::MATS).wrapping_add(mat::ENGINE);
        seg(f, view, cam, p(k), p(k.wrapping_add(1)), c, k == 0);
    }
}

/// The point `len` from (x, y) at angle `a` (0: down the screen).
/// Callers keep |x|, |y| and |len| within 2^13: the sums fit.
#[inline(never)]
fn polar(x: i32, y: i32, a: u16, len: i32) -> [i32; 2] {
    [
        x.wrapping_add(sin(a).wrapping_mul(len) >> 14),
        y.wrapping_add(cos(a).wrapping_mul(len) >> 14),
    ]
}

/// A ring of 16 straight pieces, inside `clip`.
#[inline(never)]
pub fn ring(f: &mut Frame, clip: &Rect, cx: i32, cy: i32, r: i32, c: u8) {
    // |cx|, |cy| <= 4096 and r <= 400: every sum stays inside i32.
    let (cx, cy, r) = (near(cx), near(cy), r.max(0).min(400));
    let mut p = polar(cx, cy, 0, r);
    for k in 1..=16u16 {
        let q = polar(cx, cy, k.wrapping_mul(4096), r);
        line(f, clip, p, q, c);
        p = q;
    }
}

/// The nebula: two soft clouds far behind the stars, each a few
/// overlapping discs in three steps of one tint (the system's, see
/// [`art::NEBULAS`]) inside a speckled (every other pixel) fringe;
/// where they are and how they lie comes from the system's number, so
/// each system has its own sky.
#[inline(never)]
fn nebula(f: &mut Frame, view: &View, cam: &Camera, h: u32) {
    for k in 0..2u32 {
        let g = hash(h, 0x4E42u32.wrapping_add(k));
        let g2 = hash(g, 1);
        let part = |v: u32| (v % 2001) as i32 - 1000;
        let d = norm([part(g), part(g >> 11), part(g2)], 16000);
        let c = cam.rot.apply(d);
        if c[2] <= 3000 {
            continue;
        }
        let p = view.project(c);
        // Pulled near the picture: the sums below stay inside i32.
        let (x, y) = (near(p[0] / SUB), near(p[1] / SUB));
        let r = 20 + ((g2 >> 12) % 24) as i32;
        let a = (g2 >> 16) as u16;
        let [ex, ey] = polar(0, 0, a, r);
        // Along the cloud in quarters of r, and across it (jitter from
        // the hash), each blob's radius in 1/16 of r.
        let at = |t: i32, j: u32| {
            let s = ((hash(g, j) % 9) as i32).wrapping_sub(4);
            (
                x.wrapping_add(ex.wrapping_mul(t) / 4)
                    .wrapping_add(ey.wrapping_mul(s) / 8),
                y.wrapping_add(ey.wrapping_mul(t) / 4)
                    .wrapping_sub(ex.wrapping_mul(s) / 8),
            )
        };
        let blobs = [
            (-4, 9, art::NEB1, 2),
            (-1, 15, art::NEB1, 3),
            (3, 11, art::NEB1, 4),
            (0, 9, art::NEB2, 5),
            (-3, 5, art::NEB2, 6),
            (2, 6, art::NEB2, 7),
            (0, 4, art::NEB3, 8),
        ];
        for &(t, rr, _, j) in blobs.iter().take(3) {
            let (cx, cy) = at(t, j);
            speckle(f, &view.area, cx, cy, r.wrapping_mul(rr) / 12, art::NEB1);
        }
        // The body solid, the brighter steps speckled over it (a blend).
        for (t, rr, col, j) in blobs {
            let (cx, cy) = at(t, j);
            let rad = r.wrapping_mul(rr) / 16;
            if col == art::NEB1 {
                disc(f, &view.area, cx, cy, rad, col);
            } else {
                speckle(f, &view.area, cx, cy, rad, col);
            }
        }
    }
}

/// A disc of every other pixel (a soft edge), inside `clip`.
#[inline(never)]
fn speckle(f: &mut Frame, clip: &Rect, cx: i32, cy: i32, r: i32, c: u8) {
    let a = clip.on_screen();
    // |cx|, |cy| <= 4096 and r <= 400: no sum or product leaves i32.
    let (cx, cy, r) = (near(cx), near(cy), r.max(0).min(400));
    for y in cy.wrapping_sub(r).max(a.y0)..cy.wrapping_add(r).wrapping_add(1).min(a.y1) {
        let dy = y.wrapping_sub(cy);
        let half = isqrt(r.wrapping_mul(r).wrapping_sub(dy.wrapping_mul(dy)).max(0) as u64) as i32;
        let x0 = cx.wrapping_sub(half).max(a.x0);
        let x1 = cx.wrapping_add(half).wrapping_add(1).min(a.x1);
        // Start on the pixel whose x + y is even.
        let mut x = x0.wrapping_add(x0.wrapping_add(y) & 1);
        let at = y as usize * W;
        while x < x1 {
            f.0[at + x as usize] = c;
            x = x.wrapping_add(2);
        }
    }
}

/// The sun's corona: slowly turning rays of two lengths, a dim outer
/// glow, the halo and a bright rim just round the ball.
#[inline(never)]
fn corona(f: &mut Frame, clip: &Rect, x: i32, y: i32, r: i32, t: u16) {
    let (x, y, r) = (near(x), near(y), r.max(0).min(400));
    if !fx::CORONA_ON {
        disc(f, clip, x, y, r.wrapping_mul(5) / 4 + 1, art::HALO);
        return;
    }
    if r < 120 {
        for k in 0..8u16 {
            let a = t.wrapping_mul(4).wrapping_add(k.wrapping_mul(8192));
            // Long and short rays, each breathing slowly.
            let breathe = sin(t.wrapping_mul(9).wrapping_add(k.wrapping_mul(9000)));
            let len = r
                .wrapping_mul(if k % 2 == 0 { 5 } else { 3 })
                .wrapping_add(r.wrapping_mul(breathe) >> 15)
                .wrapping_add(6);
            line(f, clip, [x, y], polar(x, y, a, len), art::HALO);
        }
    }
    disc(f, clip, x, y, r.wrapping_mul(3) / 2 + 2, art::CORONA1);
    disc(f, clip, x, y, r.wrapping_mul(5) / 4 + 1, art::HALO);
    disc(f, clip, x, y, r.wrapping_mul(21) / 20 + 1, art::CORONA2);
}

/// Ghost rings across the view when the sun is in the picture and not
/// behind the planet: on the line from the sun through the middle.
#[inline(never)]
fn flare(f: &mut Frame, view: &View, cam: &Camera, w: &World) {
    let sun = w.bodies.sun_pos;
    let Some((sx, sy)) = on_screen(view, cam, sun) else {
        return;
    };
    let a = view.area;
    if sx < a.x0 || sx >= a.x1 || sy < a.y0 || sy >= a.y1 || behind_planet(w, cam, sun) {
        return;
    }
    let c = view.project([0, 0, M]);
    let (dx, dy) = (c[0] / SUB - sx, c[1] / SUB - sy);
    // (eighths of the way to the middle, radius, ring or disc, colour)
    for (t, r, col) in [
        (5, 3, art::CORONA2),
        (11, 2, art::HALO),
        (15, 7, art::HALO),
        (20, 3, art::CORONA2),
    ] {
        ring(f, &view.area, sx + dx * t / 8, sy + dy * t / 8, r, col);
    }
}

/// The player's ship was hit: a ripple ring spreading round it while
/// the shield holds, a flickering red edge round the view when it is
/// down.
#[inline(never)]
fn hit_mark(f: &mut Frame, view: &View, cam: &Camera, w: &World) {
    let j = i32::from(w.fx.jolt.min(16));
    if w.me.shield > 0 {
        let (x, y) = on_screen(view, cam, w.me.pos).unwrap_or_else(|| {
            let c = view.project([0, 0, M]);
            (c[0] / SUB, c[1] / SUB)
        });
        let r = 48 - j * 2;
        ring(f, &view.area, x, y, r, art::SHIELD);
        if j > 4 {
            ring(f, &view.area, x, y, r - 5, art::SHIELD);
        }
    } else if j % 4 >= 2 {
        let a = view.area.on_screen();
        let (x0, y0, x1, y1) = (a.x0, a.y0, a.x1 - 1, a.y1 - 1);
        for (p, q) in [
            ([x0, y0], [x1, y0]),
            ([x0, y1], [x1, y1]),
            ([x0, y0], [x0, y1]),
            ([x1, y0], [x1, y1]),
        ] {
            bolt(f, &view.area, p, q, art::BEAM_FOE, (0, 8), true);
        }
    }
}

/// A light at `p`: a square dot of side `size`, if in front.
#[inline(never)]
fn light(f: &mut Frame, view: &View, cam: &Camera, p: V3, c: u8, size: i32) {
    if let Some((x, y)) = on_screen(view, cam, p) {
        dot_px(f, &view.area, x, y, c, size);
    }
}

/// The station's lights: red and green on the wing tips blinking once a
/// second, a white strobe on the hub, and (near it, seen from the front)
/// two rows of green guide lights before the mouth, a lit one running
/// in towards it. `rot`: the station's turning with its spin.
#[inline(never)]
fn station_lights(f: &mut Frame, view: &View, cam: &Camera, w: &World, rot: &Mat3) {
    let b = &w.bodies;
    let st = b.station_pos;
    if !world::within(st, cam.pos, 15 * world::KM) || behind_planet(w, cam, st) {
        return;
    }
    let t = w.ticks.wrapping_add(w.show);
    // Model corners within 2^13 times STATION_U (64): inside i32.
    let at = |p: [i32; 3]| add(st, rot.apply(p.map(|c| c.wrapping_mul(world::STATION_U))));
    let mut put = |p: V3, c: u8, size: i32| light(f, view, cam, p, c, size);
    if fx::LIGHTS_ON {
        if (t / 30).is_multiple_of(2) {
            put(at([-4330, 0, 500]), mat::LIGHT_R, 2);
            put(at([4330, 0, 500]), mat::LIGHT_G, 2);
        }
        if t % 80 < 3 {
            put(at([0, 1640, -600]), art::STAR_BRIGHT, 2);
        }
    }
    if fx::GUIDE_ON
        && world::within(st, cam.pos, 6 * world::KM)
        && dot(rot.col(2), sub(cam.pos, st)) > 0
    {
        let lit = 7 - (t / 5) % 8;
        let dim = 4u8.wrapping_mul(art::MATS).wrapping_add(mat::LIGHT_G);
        for k in 0..8u32 {
            let z = i32::from(models::MOUTH_Z) + 300 + 800 * k as i32;
            // Beside the mouth (step M2: it is wider).
            let x = i32::from(models::MOUTH_W) + 100;
            for x in [-x, x] {
                let on = k == lit;
                put(
                    at([x, 0, z]),
                    if on { mat::LIGHT_G } else { dim },
                    if on { 2 } else { 1 },
                );
            }
        }
    }
}

/// The size (256: the model's own) that makes a ball model `r` round.
/// In 64 bits: a big sun's radius times 256 does not fit in i32.
#[inline(never)]
fn ball_scale(r: i32) -> i32 {
    (wide(r, 256) / i64::from(models::BALL)) as i32
}

/// Draws a body (or ship) `o` that may be far or tiny: at `cap` when
/// farther, made smaller by the same amount ([`CAP`]; ships pass
/// [`SHIP_SEEN`], so they are always drawn where they are); as a dot
/// of `dot_c` when smaller than a pixel (a ship: a 2x2 dot under
/// [`SHIP_DOT`] pixels). `rim`: a glow round it (the sun, the air).
/// Gives back where it was drawn.
#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn far_body(
    f: &mut Frame,
    r: &mut Renderer,
    view: &View,
    cam: &Camera,
    sh: &Shading,
    o: &Object,
    dot_c: u8,
    rim: Rim,
    cap: i32,
) -> V3 {
    let (m, pos, scale) = (o.model, o.pos, o.scale);
    let rel = sub(pos, cam.pos);
    let d = i64::from(length(rel)).max(1);
    let (pos, scale) = if d > i64::from(cap) {
        let k = |c: i32| (wide(c, cap) / d) as i32;
        (add(cam.pos, rel.map(k)), k(scale))
    } else {
        (pos, scale)
    };
    let c = cam.rot.apply(sub(pos, cam.pos));
    // Radius on screen in pixels.
    // Radius below 2^12, scale below 2^31, focal below 2^8: the
    // product is below 2^43.
    let world_r = wide(m.radius, scale) / 256;
    let px = if c[2] > 0 {
        world_r.wrapping_mul(i64::from(view.focal)) / i64::from(c[2])
    } else {
        i64::MAX
    };
    // A ship under 3 px is only its dot; under 6 px the dot is drawn
    // under the model too, so a thin hull seen edge-on (thinner than a
    // pixel row) still shows and nothing jumps at 3 px.
    let ship = cap != CAP;
    if c[2] > 0 && px < if ship { 2 * SHIP_DOT } else { 1 } {
        let s = view.project(c);
        let big = ship || matches!(rim, Rim::Sun(_));
        dot_px(
            f,
            &view.area,
            s[0] / SUB,
            s[1] / SUB,
            dot_c,
            1 + i32::from(big),
        );
        if px < SHIP_DOT || !ship {
            return pos;
        }
    }
    if c[2] > 0 {
        let s = view.project(c);
        let (x, y, pr) = (s[0] / SUB, s[1] / SUB, px.min(4096) as i32);
        match rim {
            Rim::Sun(t) => corona(f, &view.area, x, y, pr, t),
            Rim::Air(l) => {
                // Towards the sun on the screen (y down), up to 1/20 of
                // the radius: l's parts are within 2^14, pr below 2^9.
                let pr9 = pr.min(400);
                let o = |v: i32| v.wrapping_mul(pr9) / 16384 / 20;
                let (ox, oy) = (o(l[0]), o(l[1]).wrapping_neg());
                let (x, y) = (near(x), near(y));
                disc(f, &view.area, x + ox, y + oy, pr + pr / 10 + 2, art::ATMOS1);
                disc(
                    f,
                    &view.area,
                    x + ox / 2,
                    y + oy / 2,
                    pr + pr / 30 + 1,
                    art::ATMOS2,
                );
            }
            Rim::None => {}
        }
    }
    // Step M2: up to 2^18 (the biggest hauler is drawn at 92,160).
    let o = Object {
        pos,
        scale: scale.clamp(1, (1 << 18) - 1),
        ..*o
    };
    r.draw(f, view, cam, sh, &o, 3 * CAP);
    pos
}

/// A ship's engine glow, `back` behind its middle (drawn at `drawn`),
/// when its tail is towards the eye.
#[inline(never)]
fn glow(f: &mut Frame, view: &View, cam: &Camera, o: &Object, drawn: V3, back: i32) {
    let nose = o.rot.col(2);
    if dot(nose, sub(o.pos, cam.pos)) > 0
        && let Some((x, y)) = on_screen(view, cam, add(drawn, scale14(nose, -back)))
    {
        dot_px(f, &view.area, x - 1, y - 1, art::mat::ENGINE, 2);
    }
}

/// Step M3: smoke trailing from a damaged ship at `at` (its nose along
/// `nose`, its tail `back` behind the middle): [`fx::PUFFS`] puffs for
/// its `level` ([`fx::smoke_of`]) drifting back from the tail, growing
/// and fading from pale grey to dark; heavy smoke is soot with fire in
/// the youngest puff. Only the trail's two ends are put through the
/// camera; the puffs lie on the screen line between them. Placed by the
/// tick `t` and the ship's `look` alone (no state, no dice); each puff
/// a disc of at most 4 pixels' radius. Step J1: the trail's fixed
/// lengths (30 m past four tails, 2 m of a puff's size) are times
/// `grow` (1, or [`world::ME_X`] for my own ship, which step F1 drew
/// that much bigger, with the chase eye that much farther back): so
/// its smoke looks from the chase eye exactly as before F1, the far
/// end behind the eye and the shorter trail drawn.
#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn smoke(
    f: &mut Frame,
    view: &View,
    cam: &Camera,
    at: V3,
    nose: V3,
    back: i32,
    grow: i32,
    level: u8,
    t: u32,
    look: u8,
) {
    // Tails below 2^16, grow below 8: the trail's end below 2^19.
    let back = back.max(4 * M).min(1 << 16);
    let end = |d: i32| {
        cam.rot
            .apply(sub(add(at, scale14(nose, d.wrapping_neg())), cam.pos))
    };
    // The trail's far end may lie behind the eye (the player's own
    // ship in the chase view): then a shorter trail.
    let far = back.wrapping_mul(4).wrapping_add(grow.wrapping_mul(30 * M));
    let (c, mut e) = (end(back), end(far));
    if e[2] < NEAR {
        e = end(back.wrapping_mul(2));
    }
    if c[2] < NEAR || e[2] < NEAR {
        return;
    }
    let (a, b) = (view.project(c), view.project(e));
    // In whole pixels, pulled near the picture: no sum or product below
    // leaves i32.
    let (ax, ay) = (near(a[0] / SUB), near(a[1] / SUB));
    let (dx, dy) = (
        near(b[0] / SUB).wrapping_sub(ax),
        near(b[1] / SUB).wrapping_sub(ay),
    );
    // A new puff's radius (pixels in 1/128), at the trail's start.
    let size = (wide(
        back / 3 + grow.wrapping_mul(2 * M),
        view.focal.wrapping_mul(128),
    ) / i64::from(c[2]))
    .min(1024) as i32;
    let n = fx::PUFFS[usize::from(level.min(2))];
    for k in 0..n {
        // Its age, 0..256: a new puff every 256 / n steps of the cycle.
        let run = t
            .wrapping_mul(3)
            .wrapping_add(k.wrapping_mul(256) / n)
            .wrapping_add(u32::from(look).wrapping_mul(7));
        let (q, h) = ((run % 256) as i32, hash(u32::from(look), run / 256));
        // Growing to three times its first size, at most 4 pixels.
        let r = size.wrapping_mul(q.wrapping_add(128)) / 16384;
        let r = r.min(4);
        // Scattered by up to its radius.
        let jit = |v: u32| ((v % 256) as i32 - 128).wrapping_mul(r) / 128;
        let x = ax
            .wrapping_add(dx.wrapping_mul(q) / 256)
            .wrapping_add(jit(h));
        let y = ay
            .wrapping_add(dy.wrapping_mul(q) / 256)
            .wrapping_add(jit(h >> 8));
        let col = match (level, q) {
            (2, 0..40) => {
                if t % 4 < 2 {
                    art::FIRE2
                } else {
                    art::FIRE3
                }
            }
            (2, 40..150) => art::DUST,
            (2, _) => art::SMOKE,
            (_, 0..90) => art::STAR,
            (_, 90..180) => art::DUST,
            _ => art::STAR_DIM,
        };
        disc(f, &view.area, x, y, r, col);
    }
}

/// Step M3: the cockpit's sign of a damaged ship: grey wisps rising
/// from the view's lower edge, [`fx::PUFFS`] of them for the smoke
/// `level`, placed by the tick alone. Two-pixel dots: under 20 pixels.
#[inline(never)]
fn wisps(f: &mut Frame, view: &View, level: u8, t: u32) {
    let a = view.area.on_screen();
    let (wd, ht) = (
        a.x1.wrapping_sub(a.x0).max(1),
        (a.y1.wrapping_sub(a.y0) / 3).max(1),
    );
    for k in 0..fx::PUFFS[usize::from(level.min(2))] {
        let rise = (t.wrapping_add(k.wrapping_mul(37)) % ht as u32) as i32;
        let h = hash(k, t.wrapping_add(k.wrapping_mul(37)) / ht as u32);
        let x = a.x0.wrapping_add((h % wd as u32) as i32);
        let col = if rise < ht / 2 {
            art::DUST
        } else {
            art::STAR_DIM
        };
        dot_px(
            f,
            &view.area,
            x,
            a.y1.wrapping_sub(2).wrapping_sub(rise),
            col,
            2,
        );
    }
}

/// A screen coordinate pulled into +-4096, far outside the picture: the
/// pixel helpers below may then use plain wrapping sums (no sum can
/// leave i32), which saves the overflow checks' code on the chip.
#[inline(always)]
fn near(v: i32) -> i32 {
    v.max(-4096).min(4096)
}

/// A square dot of side `size` (0..=8) at (x, y), inside `clip`.
#[inline(never)]
pub fn dot_px(f: &mut Frame, clip: &Rect, x: i32, y: i32, c: u8, size: i32) {
    let a = clip.on_screen();
    let (x, y, size) = (near(x), near(y), size.max(0).min(8));
    let (x0, x1) = (x.max(a.x0), x.wrapping_add(size).min(a.x1));
    for yy in y.max(a.y0)..y.wrapping_add(size).min(a.y1) {
        if x0 < x1 {
            let at = yy as usize * W;
            f.0[at + x0 as usize..at + x1 as usize].fill(c);
        }
    }
}

/// A filled disc, inside `clip`.
#[inline(never)]
pub fn disc(f: &mut Frame, clip: &Rect, cx: i32, cy: i32, r: i32, c: u8) {
    let a = clip.on_screen();
    // |cx|, |cy| <= 4096 and r <= 400: no sum or product below leaves i32.
    let (cx, cy, r) = (near(cx), near(cy), r.max(0).min(400));
    for y in cy.wrapping_sub(r).max(a.y0)..cy.wrapping_add(r).wrapping_add(1).min(a.y1) {
        let dy = y.wrapping_sub(cy);
        let half = isqrt(r.wrapping_mul(r).wrapping_sub(dy.wrapping_mul(dy)).max(0) as u64) as i32;
        let (x0, x1) = (
            cx.wrapping_sub(half).max(a.x0),
            cx.wrapping_add(half).wrapping_add(1).min(a.x1),
        );
        if x0 < x1 {
            let at = y as usize * W;
            f.0[at + x0 as usize..at + x1 as usize].fill(c);
        }
    }
}

/// Space dust: points fixed in space on a 64 m grid round the camera,
/// drawn as short streaks along the way the ship goes.
#[inline(never)]
fn dust(f: &mut Frame, view: &View, cam: &Camera, w: &World, d: Detail) {
    dust_specks(view, cam, w, d, |_, a, b| {
        line(f, &view.area, a, b, art::DUST)
    });
}

/// The specks [`dust`] draws: for each, `each(point, head, tail)` with
/// the speck's point in space and the two ends of its streak on the
/// screen, in whole pixels (`tests/speed.rs` measures the sense of
/// speed with it).
///
/// 2026-11-23: the grid is round the camera, not the ship, in both
/// views, so the chase eye sees the dust as the cockpit does: as many
/// specks, as near, rushing past as fast at the same speed. (Step F1
/// had put it round the ship and made it and the streaks [`world::ME_X`]
/// times bigger for the chase eye, which sits 420 m behind: twice as
/// many specks crossed the picture at a sixth of the cockpit's pace.) The
/// cockpit's dust is unchanged (there the camera is at the ship).
#[inline(always)]
pub fn dust_specks(
    view: &View,
    cam: &Camera,
    w: &World,
    d: Detail,
    mut each: impl FnMut(V3, [i32; 2], [i32; 2]),
) {
    let me = &w.me;
    let cell_m = 64 * M;
    let reach = if d == Detail::High { 1 } else { 0 };
    let base = cam.pos.map(|c| c.div_euclid(cell_m));
    // The streak: how far the ship moved in 3 ticks, at most 30 m
    // (speeds below 2^17).
    let back = scale14(me.rot.col(2), -(me.speed * 3).min(30 * M));
    for dx in -1..=1 {
        for dy in -reach..=reach {
            for dz in -1..=2 {
                let cell = [
                    base[0].wrapping_add(dx),
                    base[1].wrapping_add(dy),
                    base[2].wrapping_add(dz),
                ];
                let h = hash(hash(cell[0] as u32, cell[1] as u32), cell[2] as u32);
                // Points within one cell of the camera, which stays well
                // inside +-2^30: the wrapping sums are exact.
                let at = |c: i32, h: u32| {
                    c.wrapping_mul(cell_m)
                        .wrapping_add((h % cell_m as u32) as i32)
                };
                let p = [at(cell[0], h), at(cell[1], h >> 8), at(cell[2], h >> 16)];
                let a = cam.rot.apply(sub(p, cam.pos));
                let b = cam.rot.apply(sub(add(p, back), cam.pos));
                if a[2] < 4 * M || b[2] < 4 * M {
                    continue;
                }
                let (pa, pb) = (view.project(a), view.project(b));
                each(p, [pa[0] / SUB, pa[1] / SUB], [pb[0] / SUB, pb[1] / SUB]);
            }
        }
    }
}

/// A laser shot as a short, two-pixel bolt flying from the gun to where
/// the beam ends: each of its four ticks shows the next part of the way,
/// taken in space (so a shot coming at the camera grows towards the
/// middle of the view instead of showing as a stub at the edge), then
/// cut at the near plane.
#[inline(never)]
fn beam(f: &mut Frame, view: &View, cam: &Camera, bm: &world::Beam, c: u8) {
    let a0 = cam.rot.apply(sub(bm.from, cam.pos));
    let b0 = cam.rot.apply(sub(bm.to, cam.pos));
    // Age 4 (new) to 1: eighths 1..4, 3..6, 5..8, 5..8. The last part
    // shows twice, so at 30 pictures a second one of them is always
    // seen: a shot at the camera is only long on screen there.
    let head = i32::from(12u8.saturating_sub(bm.age.wrapping_mul(2))).min(8);
    // Beams are at most a few km (2^20): times 8 stays far inside i32.
    let at = |e: i32| -> V3 {
        [0, 1, 2].map(|k| a0[k].wrapping_add(b0[k].wrapping_sub(a0[k]).wrapping_mul(e) / 8))
    };
    let (mut a, mut b) = (at(head.wrapping_sub(3)), at(head));
    let near = 2 * M;
    if a[2] < near && b[2] < near {
        return;
    }
    let cut = |p: V3, q: V3| -> V3 {
        // The point on p..q where z = near (p behind, q in front). Beams
        // are at most a few km long (2^20): the product stays below 2^42.
        let t = near.wrapping_sub(p[2]);
        let den = i64::from(q[2].wrapping_sub(p[2])).max(1);
        [0, 1, 2].map(|k| p[k].wrapping_add((wide(q[k].wrapping_sub(p[k]), t) / den) as i32))
    };
    if a[2] < near {
        a = cut(a, b);
    } else if b[2] < near {
        b = cut(b, a);
    }
    let (pa, pb) = (view.project(a), view.project(b));
    bolt(
        f,
        &view.area,
        [pa[0] / SUB, pa[1] / SUB],
        [pb[0] / SUB, pb[1] / SUB],
        c,
        (0, 8),
        true,
    );
}

/// An explosion in four stages: a white flash, a fireball that shrinks
/// (its heart white at first), sparks running out, and fragments flying
/// apart and fading from white to orange to red to smoke. Like the
/// ships, it is drawn in true perspective (at least a pixel).
#[inline(never)]
fn blast(f: &mut Frame, view: &View, cam: &Camera, pos: V3, age: u8, size: u8) {
    let c = cam.rot.apply(sub(pos, cam.pos));
    if c[2] < 4 * M {
        return;
    }
    let s = view.project(c);
    let depth = c[2];
    // In 1/16 pixel, pulled near the picture; the blast's full radius
    // on screen, at most 160 pixels. With these bounds every product
    // below stays under 2^28, so plain wrapping sums are exact.
    let (cx, cy) = (
        s[0].max(-1 << 16).min(1 << 16),
        s[1].max(-1 << 16).min(1 << 16),
    );
    let full = (i64::from(size).wrapping_mul(wide(2 * M, view.focal.wrapping_mul(SUB)))
        / i64::from(depth))
    .max(16)
    .min(160 * 16) as i32;
    let age = i32::from(age).min(i32::from(BLAST_TICKS));
    let col = match age {
        0..=5 => art::FIRE1,
        6..=15 => art::FIRE2,
        16..=30 => art::FIRE3,
        _ => art::SMOKE,
    };
    let (px, py) = (cx / SUB, cy / SUB);
    if age < 4 {
        disc(f, &view.area, px, py, full / SUB / 2 + 1, art::FIRE1);
    } else if age < 18 {
        // The fireball: shrinking, its heart white while it is young.
        let r = (full / SUB).wrapping_mul(18 - age) / 40;
        disc(f, &view.area, px, py, r.wrapping_add(1), art::FIRE2);
        if age < 10 {
            disc(f, &view.area, px, py, r / 2, art::FIRE1);
        }
    }
    if fx::SHOCK_ON && (2..16).contains(&age) {
        // The shock ring: out to well past the fireball, fading.
        let rr = (full / SUB).wrapping_mul(age.wrapping_add(2)) / 8;
        ring(
            f,
            &view.area,
            px,
            py,
            rr,
            if age < 8 { art::FIRE2 } else { art::FIRE3 },
        );
    }
    let ticks = i32::from(BLAST_TICKS);
    let out = full.wrapping_mul(age.wrapping_add(6)) / (ticks + 6);
    let bit = ((full / 4).max(24).wrapping_mul(ticks.wrapping_sub(age)) / ticks).wrapping_add(12);
    for k in 0..7u32 {
        let h = hash(k, u32::from(size));
        let a = (h as u16).wrapping_add((age as u16).wrapping_mul(90).wrapping_mul(k as u16 % 3));
        let reach = out.wrapping_mul(8 + (h >> 20) as i32 % 8) / 16;
        let (sx, sy) = (crate::fixed::sin(a), crate::fixed::cos(a));
        let m = |a: i32, b: i32| a.wrapping_mul(b) >> 14;
        let px = cx.wrapping_add(m(sx, reach));
        let py = cy.wrapping_add(m(sy, reach));
        let (b1, b2) = (bit, bit.wrapping_mul(2));
        let p = [
            [px.wrapping_add(m(sy, b1)), py.wrapping_sub(m(sx, b1))],
            [px.wrapping_sub(m(sy, b1)), py.wrapping_add(m(sx, b1))],
            [px.wrapping_add(m(sx, b2)), py.wrapping_add(m(sy, b2))],
        ];
        fill_tri(f, &view.area, &p, col);
        // A spark running out ahead of each fragment, twice as far.
        if (3..26).contains(&age) {
            let (x, y) = (px.wrapping_add(m(sx, reach)), py.wrapping_add(m(sy, reach)));
            dot_px(f, &view.area, x / SUB, y / SUB, art::FIRE1, 1);
        }
    }
}

/// Step M3: the station's turn per tick of the docked scene (as in
/// flight), and the docked camera's slow drift: [`DOCK_SWAY`] either
/// side of the old still view's turn (12,000), once in 4096 ticks.
pub const DOCK_SPIN: u16 = 30;
pub const DOCK_TURN: u16 = 12_000;
pub const DOCK_SWAY: i32 = 1400;

/// The docked camera's turn for [`station_camera`] now: drifting with
/// `show`; at 0 (on arriving, on the title) the old still view's.
#[inline(never)]
pub fn dock_turn(w: &World) -> u16 {
    let s = sin((w.show as u16).wrapping_mul(16));
    DOCK_TURN.wrapping_add((s.wrapping_mul(DOCK_SWAY) >> 14) as u16)
}

/// The view's look while docked or on the title: the station from
/// outside, the planet behind it.
#[inline(never)]
pub fn station_camera(w: &World, turn: u16) -> Camera {
    let b = &w.bodies;
    let axis = b.station_rot.col(2);
    let side = b.station_rot.col(0);
    let s = crate::fixed::sin(turn);
    let side = scale14(side, s);
    // Step M2: as before, times the station's growth.
    const S: i32 = M * world::STATION_X;
    let eye = add(
        add(b.station_pos, scale14(axis, 700 * S)),
        add(
            scale14(side, 500 * S),
            scale14(b.station_rot.col(1), 180 * S),
        ),
    );
    let look = sub(b.station_pos, eye);
    Camera {
        pos: eye,
        rot: Mat3::looking(look, b.station_rot.col(1)).transposed(),
    }
}

/// Draws a model alone (title, cards): the model at the middle of
/// `view`, turned by `rot`, seen from `dist`.
#[inline(never)]
pub fn showcase(f: &mut Frame, r: &mut Renderer, view: &View, m: &Model, rot: Mat3, dist: i32) {
    let cam = Camera {
        pos: [0, 0, -dist],
        rot: Mat3::IDENTITY,
    };
    let sh = Shading {
        mats: art::MATS,
        shades: art::SHADES,
        light: [-6000, -12000, 9000],
        glow: art::GLOW,
        fog_from: 0,
        fog_to: 0,
        fog_shades: 0,
        clear: 0,
        dark_side: 6,
    };
    let o = Object {
        model: m,
        pos: [0; 3],
        rot,
        scale: SHIP_SCALE,
        swap: (0, 0),
    };
    r.draw(f, view, &cam, &sh, &o, 4 * dist);
}
