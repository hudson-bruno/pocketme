//! The title's backdrop: a made-up race of the computer's pilots on the
//! menu's track and class, filmed like television. A few cameras beside
//! the road turn to follow the ships as they come, pass and go;
//! travelling shots (behind, high, low in front looking back) come in
//! between. Each shot follows one ship (the leader, now and then a ship
//! in a close fight) and cuts after a few seconds, or sooner when the
//! ship gets too far, goes into a tunnel the camera is not in, or falls.
//!
//! The race is a normal [`Race`] stepped with no player (`Race::step(None)`
//! flies every ship by the computer, the player's ship too), so it obeys
//! the race's own rules; the app drops its events, so it writes no best
//! lap and no log line, and the recorder never sees it (it only looks at
//! a race on the race screen).

use crate::fixed::{atan2, hash, isqrt, sin, turn};
use crate::race::{COUNT, Class, HOVER, Mode, Race, Ship};
use crate::scene::{Cam, world_at};
use crate::track::{ARCH, FM, GANTRY, GAP, PORTAL, ROOF, SEG_BITS, Track};

/// The kinds of shot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shot {
    /// A camera standing beside the road (also the start's).
    Side,
    /// Behind the ship and above it.
    Far,
    /// Low, in front of the ship, looking back at it (not in tunnels).
    Front,
    /// High above and behind (not near tunnels).
    High,
}

/// The order the shots come in, cut after cut.
const PLAN: [Shot; 8] = [
    Shot::Side,
    Shot::Far,
    Shot::Front,
    Shot::Side,
    Shot::High,
    Shot::Far,
    Shot::Side,
    Shot::Front,
];

/// How far left of the ship the cameras look (angle), so the ship sits
/// right of the menu column, about x 116 of 160 (atan(36 / 96)).
pub const AIM_LEFT: i32 = 3_700;
/// The made-up race starts again after this many ticks at most.
pub const RACE_MAX: u32 = 60 * 60 * 6;
/// A side camera stands this far outside the road's edge (between the
/// lamp posts and arches, 1.2 m out, and the trees, 5 m out or more).
const SIDE_OUT: i32 = FM * 3;

/// The director: which shot, which ship, where the camera stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tv {
    pub shot: Shot,
    /// The ship followed.
    pub ship: u8,
    /// Ticks in this shot, and the most it lasts.
    pub t: u32,
    pub len: u32,
    pub cuts: u32,
    /// A side camera's place (fine units: along, across, up).
    pub s: i32,
    pub x: i32,
    pub h: i32,
    /// Where the camera looks (absolute angles).
    yaw: i32,
    pitch: i32,
    /// Made-up races so far (the next one's seed).
    pub races: u32,
    /// False until the first race is set up.
    pub live: bool,
}

impl Tv {
    pub const fn new() -> Tv {
        Tv {
            shot: Shot::Side,
            ship: 0,
            t: 0,
            len: 0,
            cuts: 0,
            s: 0,
            x: 0,
            h: 0,
            yaw: 0,
            pitch: 0,
            races: 0,
            live: false,
        }
    }

    /// A new made-up race on `track` in `class`, and its first shot: a
    /// camera beside the road just ahead of the grid, looking back at it
    /// (the start gantry behind it).
    pub fn restart(&mut self, race: &mut Race, cam: &mut Cam, track: u8, class: Class) {
        race.setup(track, class, Mode::Race, self.races);
        self.races = self.races.wrapping_add(1);
        self.live = true;
        self.cuts = 0;
        self.ship = race.order[0];
        let front = race.ships[usize::from(self.ship)].s;
        if !self.side(race.tr(), front + FM * 5, FM * 5 / 2, 1, true) {
            self.shot = Shot::High;
        }
        self.len = COUNT + 300;
        self.t = 0;
        self.film(race, cam);
    }

    /// One tick, after the race's: cut when the shot is over, then move
    /// the camera.
    pub fn step(&mut self, race: &Race, cam: &mut Cam) {
        self.t += 1;
        if self.over(race) {
            self.cut(race);
        }
        self.film(race, cam);
    }

    /// Whether the shot must end now.
    fn over(&self, race: &Race) -> bool {
        let tr = race.tr();
        let sh = &race.ships[usize::from(self.ship)];
        let roof = |s: i32| tr.seg(tr.index(s)).kind & ROOF != 0;
        if self.t >= self.len || sh.fall > 30 || (sh.finished() && self.t > 90) {
            return true;
        }
        match self.shot {
            Shot::Side => {
                let d = wrap(sh.s - self.s, tr.len());
                d > FM * 60 || (d < -FM * 150 && self.t > 60) || roof(sh.s)
            }
            Shot::High => roof(sh.s) || roof(sh.s + FM * 24) || roof(sh.s - FM * 26),
            Shot::Front => roof(sh.s) || roof(sh.s + FM * 14),
            Shot::Far => false,
        }
    }

    /// The next shot: which ship, which camera.
    fn cut(&mut self, race: &Race) {
        let tr = race.tr();
        self.cuts = self.cuts.wrapping_add(1);
        self.ship = pick(race, self.cuts);
        let sh = &race.ships[usize::from(self.ship)];
        let mut shot = PLAN[self.cuts as usize % PLAN.len()];
        let near_roof = (0..8).any(|k| tr.seg(tr.index(sh.s) + k).kind & ROOF != 0);
        if matches!(shot, Shot::High | Shot::Front) && near_roof {
            shot = Shot::Far;
        }
        self.t = 0;
        self.len = 240 + hash(self.cuts, race.tick) % 120;
        self.shot = shot;
        if shot == Shot::Side {
            // About 1.6 s ahead of the ship, 25 to 70 m.
            let (v, _) = sh.v_track();
            let side = if self.cuts & 2 == 0 { 1 } else { -1 };
            if self.side(
                tr,
                sh.s + (v * 96).clamp(FM * 25, FM * 70),
                FM * 4,
                side,
                false,
            ) {
                self.len = 420;
            } else {
                self.shot = Shot::Far;
            }
        }
        // The camera jumps to its place (no glide across the track).
        self.yaw = i32::MIN;
    }

    /// Puts a side camera near `s` (at the first place from there with
    /// no tunnel, tunnel mouth or gap within three segments and, unless
    /// `legs`, no arch or gantry leg within a segment), `h` up, on `side`
    /// of the road. False when none is found within 24 segments.
    fn side(&mut self, tr: &Track, s: i32, h: i32, side: i32, legs: bool) -> bool {
        for k in 0..24 {
            let s = s + (k << SEG_BITS);
            let j = tr.index(s) + tr.n();
            let clear = (j - 3..=j + 3).all(|i| {
                let g = tr.seg(i);
                let leg = !legs && i + 1 >= j && i <= j + 2 && g.deco & (ARCH | GANTRY) != 0;
                g.kind & (ROOF | GAP) == 0 && g.deco & PORTAL == 0 && !leg
            });
            if clear {
                self.shot = Shot::Side;
                self.s = s.rem_euclid(tr.len());
                self.x = side * (tr.half(s) * 256 + SIDE_OUT);
                self.h = h;
                self.yaw = i32::MIN;
                return true;
            }
        }
        false
    }

    /// Moves the camera for this tick and turns it to the ship.
    fn film(&mut self, race: &Race, cam: &mut Cam) {
        let tr = race.tr();
        let sh = &race.ships[usize::from(self.ship)];
        let snap = self.yaw == i32::MIN || !cam.set;
        let (s, x, h) = match self.shot {
            Shot::Side => (self.s, self.x, self.h),
            Shot::Far => (sh.s - FM * 12, sh.x * 4 / 5, sh.h.max(-FM * 2) + FM * 7 / 2),
            Shot::Front => (sh.s + FM * 14, sh.x, FM * 17 / 10),
            // Above the start gantry's banner (8 to 9.8 m).
            Shot::High => (sh.s - FM * 26, sh.x / 2, sh.h.max(0) + FM * 12),
        };
        cam.s = s;
        if snap || self.shot == Shot::Side {
            cam.x = x;
            cam.h = h;
        } else {
            cam.x += (x - cam.x) / 5;
            cam.h += (h - cam.h) / 4;
        }
        // Inside a tunnel, below its roof.
        if tr.seg(tr.index(cam.s)).kind & ROOF != 0 {
            cam.h = cam.h.min(FM * 46 / 10);
        }
        cam.h = cam.h.max(FM / 3);
        cam.roll = 0;
        cam.set = true;
        // Aim: a little above the ship's middle.
        let (yaw, pitch) = aim(tr, cam, sh, FM / 2);
        if snap {
            self.yaw = yaw;
            self.pitch = pitch;
        } else {
            // A camera person's lag: a third of the way each tick.
            let k = if self.shot == Shot::Side { 3 } else { 2 };
            self.yaw += turn(self.yaw as u16, yaw as u16) / k;
            self.pitch += (pitch - self.pitch) / k;
        }
        cam.yaw = turn(tr.at(cam.s).yaw, self.yaw as u16);
        cam.pitch = self.pitch;
    }
}

/// Where a camera at `cam` looks (absolute turn and tilt) to see `up`
/// above ship `sh`'s middle [`AIM_LEFT`] right of the picture's middle.
fn aim(tr: &Track, cam: &Cam, sh: &Ship, up: i32) -> (i32, i32) {
    let eye = world_at(tr, cam.s, cam.x, cam.h);
    let at = world_at(tr, sh.s, sh.x, sh.h + up);
    let d = [
        i64::from(at[0] - eye[0]),
        i64::from(at[1] - eye[1]),
        i64::from(at[2] - eye[2]),
    ];
    let yaw = i32::from(atan2(d[0], d[2])) - AIM_LEFT;
    let flat = isqrt((d[0] * d[0] + d[2] * d[2]) as u64) as i64;
    (yaw, i32::from(atan2(d[1], flat) as i16))
}

/// One turn of the SHIP backdrop's ship takes this many ticks (12 s).
pub const SPIN: u32 = 720;

/// The SHIP backdrop at the race's tick (only its clock runs): the
/// player's ship alone on the grid, 12 m before the start gantry,
/// turning slowly on the spot at half its racing height, bobbing and
/// swaying a little (no engine trail: seen this close it hides the ship).
/// A camera standing still 6 m behind it, 1.2 m to its left and 2.4 m up
/// looks at it so that it stands right of the menu, with the road, the
/// gantry and the track's sky and hills behind it.
pub fn stand(race: &mut Race, cam: &mut Cam) {
    let t = race.tick;
    let tr = race.tr();
    let sh = &mut race.ships[0];
    sh.s = tr.len() - FM * 12;
    sh.x = 0;
    sh.psi = (t % SPIN * 65_536 / SPIN) as i32;
    // A bob of an eighth of a metre every 3 s, a sway every 5 s.
    sh.h = HOVER / 2 + (sin(t.wrapping_mul(364) as u16) >> 2);
    sh.rate = sin(t.wrapping_mul(218) as u16) >> 8;
    *cam = Cam {
        s: sh.s - FM * 6,
        x: -FM * 6 / 5,
        h: FM * 12 / 5,
        set: true,
        ..Cam::new()
    };
    let (yaw, pitch) = aim(tr, cam, sh, FM * 3 / 5);
    cam.yaw = turn(tr.at(cam.s).yaw, yaw as u16);
    cam.pitch = pitch;
}

impl Default for Tv {
    fn default() -> Self {
        Tv::new()
    }
}

/// `d` taken round the lap to -len/2..len/2.
fn wrap(d: i32, len: i32) -> i32 {
    (d + len / 2).rem_euclid(len) - len / 2
}

/// The ship to follow: the leader still racing; every third cut, the
/// ship in places 2 to 5 closest behind the one ahead of it (within
/// 20 m), when there is one.
fn pick(race: &Race, cuts: u32) -> u8 {
    let n = usize::from(race.n);
    let len = race.tr().len();
    let racing = |i: u8| !race.ships[usize::from(i)].finished();
    let lead = race.order[..n]
        .iter()
        .copied()
        .find(|&i| racing(i))
        .unwrap_or(race.order[0]);
    if cuts % 3 != 2 {
        return lead;
    }
    let mut best = (FM as i64 * 20, lead);
    for k in 1..n.min(5) {
        let (a, b) = (race.order[k - 1], race.order[k]);
        if !racing(a) || !racing(b) {
            continue;
        }
        let gap = race.ships[usize::from(a)].prog(len) - race.ships[usize::from(b)].prog(len);
        if gap < best.0 {
            best = (gap, b);
        }
    }
    best.1
}

/// Whether every ship of the made-up race is over the line (or it ran
/// too long): time for the next one.
pub fn done(race: &Race) -> bool {
    race.tick > RACE_MAX
        || race.ships[..usize::from(race.n)]
            .iter()
            .all(|sh| sh.finished())
}
