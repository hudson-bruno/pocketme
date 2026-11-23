//! One race: five karts, three laps, run in fixed ticks of 1/60 s.
//!
//! Units (all whole numbers):
//!
//! * Along the road: `prog`, how far a kart has come since the start
//!   line, in 1/[`SUB`] world units (minus: still on the grid behind the
//!   line). One lap is the course's length.
//! * Across the road: `x`, in 1/[`X_ONE`] of half the road's width
//!   (0 is the middle, [`X_ONE`] the right edge).
//! * Speed: `v`, in 1/[`SUB`] world units a tick. [`V_MAX`] is one
//!   segment a tick (shown as 120 km/h).
//!
//! The rules in short: the road bends, and a bend pushes the kart to its
//! outside more the faster it goes (with the square of the speed), while
//! steering moves it only with the speed. So a sharp bend has a top
//! speed you must brake to. Braking and rolling charge the battery; the
//! boost spends it for a higher top speed. Off the road the kart is
//! slow, and the trees, rocks and posts stop it short.

use crate::track::{COURSES, Course, ROAD_HALF, SEG, Shoulder};

pub const SUB: i32 = 256;
/// Top speed: one segment a tick.
pub const V_MAX: i32 = SEG * SUB;
/// Top speed while boosting.
pub const V_BOOST: i32 = V_MAX * 5 / 4;
/// Half the road's width, across.
pub const X_ONE: i32 = 16384;
/// No kart goes further from the middle than this.
pub const X_LIMIT: i32 = 3 * X_ONE;
/// Beyond this the kart is off the road (the kerb still counts as road).
pub const OFF_ROAD: i32 = X_ONE + X_ONE / 8;
pub const LAPS: u32 = 3;
pub const KARTS: usize = 5;
/// Ticks of "3, 2, 1" before the start.
pub const COUNTDOWN: u32 = 180;
/// A full battery.
pub const CHARGE_MAX: i32 = 360;
/// What the battery holds at the start.
pub const CHARGE_START: i32 = 120;
/// Boosting spends this much a tick (a full battery: 3 s).
pub const BOOST_COST: i32 = 2;
/// Sideways movement a tick at full steering and top speed.
pub const STEER: i32 = 640;
/// The bend's push: `curve16 * r * r / PUSH_DIV` a tick (r: speed in
/// 1/1024 of top speed).
pub const PUSH_DIV: i64 = 293_650;
/// The fastest a bend can be taken with full steering, as
/// `GRIP / |curve16|` in 1/1024 of top speed (from STEER and PUSH_DIV).
pub const GRIP: i32 = (STEER as i64 * PUSH_DIV / 1024) as i32;
/// A kart's length and width, for bumping.
pub const KART_LEN: i32 = 280;
pub const KART_W: i32 = X_ONE * 6 / 10;
/// How far left the kart's middle must be to stand in the lake: the water
/// begins 1.3 road halves out, plus half a kart.
pub const IN_WATER: i32 = X_ONE * 13 / 10 + KART_W / 2;
/// How much a climb slows a kart: speed lost a tick per world unit of
/// climb per segment.
pub const HILL_G: i32 = 4;
/// Grid spacing, in world units.
pub const GRID: i32 = 320;

/// The player's wishes for one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pad {
    /// 0 to 256.
    pub throttle: i32,
    /// 0 to 256.
    pub brake: i32,
    /// -256 (full left) to 256 (full right).
    pub steer: i32,
    pub boost: bool,
}

/// One kart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kart {
    pub prog: i32,
    pub x: i32,
    pub v: i32,
    /// The rivals' own lane.
    pub lane: i32,
    /// The rivals' top speed, in 1/1024 of [`V_MAX`].
    pub skill: i32,
    /// Its brake lights are on.
    pub braking: bool,
    /// The race tick it crossed the line for the last time, plus one
    /// (0: not yet).
    pub finish: u32,
}

impl Kart {
    const fn new() -> Kart {
        Kart {
            prog: 0,
            x: 0,
            v: 0,
            lane: 0,
            skill: 0,
            braking: false,
            finish: 0,
        }
    }

    /// Speed in 1/1024 of top speed.
    pub fn ratio(&self) -> i32 {
        self.v * 1024 / V_MAX
    }
}

/// Something worth a sound, a note or a log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaceEvent {
    Go,
    /// Lap `done` (1-based) finished in `ticks`.
    Lap {
        done: u8,
        ticks: u32,
    },
    Finish {
        place: u8,
        ticks: u32,
    },
    /// Ran into something beside the road.
    Crash,
    /// Bumped another kart.
    Bump,
}

pub struct Race {
    pub course: u8,
    /// Kart 0 is the player's.
    pub karts: [Kart; KARTS],
    /// Ticks since the race was set up (the countdown included).
    pub tick: u32,
    pub charge: i32,
    pub boosting: bool,
    /// The race tick the current lap began.
    pub lap_start: u32,
    /// Each lap's time in ticks (0: not yet).
    pub laps: [u32; LAPS as usize],
    pub best_lap: u32,
    /// The player's place now (1 to 5).
    pub place: u8,
    pub finished: bool,
    pub finish_place: u8,
    pub finish_ticks: u32,
    /// The backdrop's turn, in 1/256 pixel (it moves in bends).
    pub bg: i32,
    /// Ticks of shaking left (after a crash, or off the road).
    pub shake: u8,
    /// Which way the driver leans: -1, 0, 1.
    pub lean: i8,
    pub off_road: bool,
    /// Ticks before another bump counts.
    bump_cool: u8,
    /// The last tick's controls (tests read them).
    pub last_pad: Pad,
    events: [Option<RaceEvent>; 4],
}

impl Race {
    /// A race not yet set up (call [`Race::reset`]).
    pub const fn empty() -> Race {
        Race {
            course: 0,
            karts: [Kart::new(); KARTS],
            tick: 0,
            charge: 0,
            boosting: false,
            lap_start: 0,
            laps: [0; LAPS as usize],
            best_lap: 0,
            place: KARTS as u8,
            finished: false,
            finish_place: 0,
            finish_ticks: 0,
            bg: 0,
            shake: 0,
            lean: 0,
            off_road: false,
            bump_cool: 0,
            last_pad: Pad {
                throttle: 0,
                brake: 0,
                steer: 0,
                boost: false,
            },
            events: [None; 4],
        }
    }

    /// Sets up course `c` (0-based) on the grid, in place.
    pub fn reset(&mut self, c: u8) {
        let c = c.min(COURSES.len() as u8 - 1);
        *self = Race::empty();
        self.course = c;
        self.charge = CHARGE_START;
        let skills = COURSES[usize::from(c)].skills;
        for (k, kart) in self.karts.iter_mut().enumerate() {
            // Rivals ahead in pairs, left and right; the player last.
            let lane = if k % 2 == 1 {
                X_ONE * 45 / 100
            } else {
                -X_ONE * 45 / 100
            };
            let row = if k == 0 { KARTS as i32 } else { k as i32 };
            kart.prog = -row * GRID * SUB;
            kart.x = lane;
            kart.lane = if k == 0 { 0 } else { lane };
            kart.skill = if k == 0 { 1024 } else { skills[k - 1] };
        }
        self.karts[0].x = -X_ONE * 45 / 100;
    }

    pub fn course(&self) -> &'static Course {
        &COURSES[usize::from(self.course)]
    }

    /// One lap, in `prog` units.
    pub fn lap_prog(&self) -> i32 {
        self.course().lap_z() * SUB
    }

    /// Ticks since the start light went green (0 before).
    pub fn race_ticks(&self) -> u32 {
        self.tick.saturating_sub(COUNTDOWN)
    }

    /// Still counting down.
    pub fn counting(&self) -> bool {
        self.tick < COUNTDOWN
    }

    /// The lap the player is on (1 to 3; 3 after the finish).
    pub fn lap(&self) -> u32 {
        let p = self.karts[0].prog;
        if p < 0 {
            1
        } else {
            (p / self.lap_prog() + 1).min(LAPS as i32) as u32
        }
    }

    /// Where kart `k` is on the loop, in world units (0 to a lap).
    pub fn z_of(&self, k: usize) -> i32 {
        (self.karts[k].prog / SUB).rem_euclid(self.course().lap_z())
    }

    /// The segment kart `k` is on.
    pub fn seg_of(&self, k: usize) -> u32 {
        (self.z_of(k) / SEG) as u32
    }

    pub fn take_event(&mut self) -> Option<RaceEvent> {
        let e = self.events[0].take();
        self.events.rotate_left(1);
        e
    }

    fn event(&mut self, e: RaceEvent) {
        if let Some(slot) = self.events.iter_mut().find(|s| s.is_none()) {
            *slot = Some(e);
        }
    }

    /// One tick. After the finish the player's kart drives itself and
    /// `pad` is not used.
    pub fn tick(&mut self, pad: &Pad) {
        self.tick = self.tick.saturating_add(1);
        self.shake = self.shake.saturating_sub(1);
        self.bump_cool = self.bump_cool.saturating_sub(1);
        if self.tick < COUNTDOWN {
            return;
        }
        if self.tick == COUNTDOWN {
            self.event(RaceEvent::Go);
            return;
        }
        let pad = Pad {
            throttle: pad.throttle.clamp(0, 256),
            brake: pad.brake.clamp(0, 256),
            steer: pad.steer.clamp(-256, 256),
            boost: pad.boost,
        };
        self.last_pad = pad;
        let snap = self.karts;
        if self.finished {
            self.karts[0].skill = 800;
            self.drive_ai(0, &snap);
            self.boosting = false;
            self.lean = 0;
        } else {
            self.drive_player(&pad);
        }
        for k in 1..KARTS {
            self.drive_ai(k, &snap);
        }
        self.bumps();
        let end = LAPS as i32 * self.lap_prog();
        let now = self.race_ticks() + 1;
        let lap = self.lap_prog();
        for kart in self.karts[1..].iter_mut() {
            if kart.finish == 0 && kart.prog >= end {
                kart.finish = now;
            }
            // A rival that is done keeps lapping, but its count stays
            // in range however long the player takes (else it would
            // overflow after about ten minutes).
            if kart.finish != 0 && kart.prog >= end + lap {
                kart.prog -= lap;
            }
        }
        // The backdrop turns with the bend under the player.
        let c = self.course().seg(self.seg_of(0)).curve16;
        self.bg = (self.bg + c * self.karts[0].ratio() / 1400).rem_euclid(256 * 256);
        if !self.finished {
            self.place = self.place_now();
        }
    }

    /// The player's place from the karts' progress.
    pub fn place_now(&self) -> u8 {
        let me = self.karts[0];
        let ahead = self.karts[1..]
            .iter()
            .filter(|k| {
                if k.finish != 0 {
                    me.finish == 0 || k.finish < me.finish
                } else {
                    me.finish == 0 && k.prog > me.prog
                }
            })
            .count();
        1 + ahead as u8
    }

    fn drive_player(&mut self, pad: &Pad) {
        let course = self.course();
        let lap = self.lap_prog();
        let k = self.karts[0];
        let seg = self.seg_of(0);
        let here = course.seg(seg);

        // Speed.
        let mut v = k.v;
        self.boosting = pad.boost && self.charge > 0 && pad.brake == 0;
        let mut vt = if self.boosting { V_BOOST } else { V_MAX };
        self.off_road = k.x.abs() > OFF_ROAD;
        if self.off_road {
            vt = vt * 45 / 100;
        }
        let throttle = if self.boosting { 256 } else { pad.throttle };
        let mut braking = false;
        if pad.brake > 0 {
            v -= 1000 * pad.brake / 256;
            braking = true;
            if v > 0 {
                self.charge += 2;
            }
        } else if throttle > 0 && v < vt {
            let mut a = 140 + 320 * (vt - v) / vt;
            if self.boosting {
                a = a * 3 / 2;
            }
            v = (v + a * throttle / 256).min(vt);
        } else if throttle == 0 {
            v -= 60 + v / 512;
            if v > V_MAX / 3 {
                self.charge += 1;
            }
        }
        if v > vt {
            let drag = if self.off_road { 450 } else { 80 };
            v -= (v - vt).min(drag);
        }
        v -= course.slope(seg) * HILL_G;
        v = v.clamp(0, V_BOOST);
        if self.boosting {
            self.charge -= BOOST_COST;
        }
        self.charge = self.charge.clamp(0, CHARGE_MAX);

        // Across: steering against the bend's push.
        let r = i64::from(v * 1024 / V_MAX);
        let mut x = k.x;
        x += (i64::from(pad.steer) * i64::from(STEER) * r / (256 * 1024)) as i32;
        x -= (i64::from(here.curve16) * r * r / PUSH_DIV) as i32;
        x = x.clamp(-X_LIMIT, X_LIMIT);
        if self.off_road && v > V_MAX / 4 && self.tick.is_multiple_of(4) {
            self.shake = self.shake.max(2);
        }
        self.lean = if pad.steer > 96 {
            1
        } else if pad.steer < -96 {
            -1
        } else {
            0
        };

        // Along, and what is passed.
        let old = k.prog;
        let new = old + v;
        let kart = &mut self.karts[0];
        kart.v = v;
        kart.x = x;
        kart.prog = new;
        kart.braking = braking;
        self.crashes(old, new);
        for done in 1..=LAPS {
            let line = done as i32 * lap;
            if old < line && new >= line {
                let rt = self.race_ticks();
                let t = rt - self.lap_start;
                self.laps[done as usize - 1] = t;
                if self.best_lap == 0 || t < self.best_lap {
                    self.best_lap = t;
                }
                self.lap_start = rt;
                self.event(RaceEvent::Lap {
                    done: done as u8,
                    ticks: t,
                });
                if done == LAPS {
                    self.finished = true;
                    self.karts[0].finish = rt + 1;
                    self.finish_place = self.place_now();
                    self.place = self.finish_place;
                    self.finish_ticks = rt;
                    self.event(RaceEvent::Finish {
                        place: self.finish_place,
                        ticks: rt,
                    });
                }
            }
        }
    }

    /// Runs into the roadside things of the segments passed this tick.
    fn crashes(&mut self, old: i32, new: i32) {
        let course = self.course();
        let step = SEG * SUB;
        let (a, b) = (old.div_euclid(step), new.div_euclid(step));
        let x = self.karts[0].x;
        if x.abs() < X_ONE {
            return;
        }
        for s in (a + 1)..=b {
            let i = s.rem_euclid(course.segs as i32) as u32;
            // Past the railing, in the lake (only reached by coming in
            // from far off the road before the lake begins): the kart is
            // stopped short and put back on the road's side of the rail.
            if course.shoulder(i) == Shoulder::WaterLeft && x < -IN_WATER {
                let kart = &mut self.karts[0];
                kart.v /= 4;
                kart.x = -X_ONE * 8 / 10;
                self.shake = 24;
                self.event(RaceEvent::Crash);
                return;
            }
            for t in course.things(i).into_iter().flatten() {
                if !t.kind.solid() {
                    continue;
                }
                let tx = i32::from(t.x) * (X_ONE / 1024);
                let reach = t.kind.width() * X_ONE / ROAD_HALF / 2 + KART_W / 2;
                // The arch: only its two feet are in the way.
                let hit = if t.kind == crate::track::Kind::Gantry {
                    (x.abs() - X_ONE * 13 / 10).abs() < reach
                } else {
                    (x - tx).abs() < reach
                };
                if hit {
                    let kart = &mut self.karts[0];
                    kart.v /= 4;
                    kart.x -= x.signum() * X_ONE / 3;
                    self.shake = 24;
                    self.event(RaceEvent::Crash);
                    return;
                }
            }
        }
    }

    /// The rivals (and the player's kart after the finish): keep to a
    /// lane, take the inside of bends, pass slower karts, and slow down
    /// for what is coming.
    fn drive_ai(&mut self, me: usize, snap: &[Kart; KARTS]) {
        let course = self.course();
        let lap = self.lap_prog();
        let seg = self.seg_of(me);
        let k = snap[me];
        let r_now = k.ratio().max(0) as u32;
        let ahead = 4 + r_now * 14 / 1024;
        let bend = course.sharpest(seg, ahead);
        let cap = if bend == 0 {
            4096
        } else {
            GRIP * 85 / 100 / bend.abs()
        };
        let vt = k.skill.min(cap) * V_MAX / 1024;
        let mut v = k.v;
        let mut braking = false;
        if v < vt {
            v = (v + 140 + 320 * (vt - v) / vt.max(1)).min(vt);
        } else if v > vt {
            braking = v > vt + V_MAX / 32;
            v -= (v - vt).min(600);
        }
        v -= course.slope(seg) * HILL_G;
        v = v.clamp(0, V_BOOST);

        // The lane: the inside of a bend, and round slower karts ahead.
        let mut target = k.lane + bend.signum() * (X_ONE / 4) * bend.abs().min(128) / 128;
        for (j, o) in snap.iter().enumerate() {
            if j == me {
                continue;
            }
            let dz = wrap(o.prog - k.prog, lap);
            if dz > 0 && dz < 6 * SEG * SUB && (o.x - k.x).abs() < KART_W * 3 / 2 && o.v < v {
                target = if o.x > 0 {
                    o.x - KART_W * 3 / 2
                } else {
                    o.x + KART_W * 3 / 2
                };
                if dz < 2 * SEG * SUB && (o.x - k.x).abs() < KART_W {
                    // Boxed in: wait behind it.
                    v = v.min(o.v);
                }
            }
        }
        let target = target.clamp(-X_ONE * 8 / 10, X_ONE * 8 / 10);
        let x = k.x + (target - k.x).clamp(-X_ONE / 80, X_ONE / 80);

        let kart = &mut self.karts[me];
        kart.v = v;
        kart.x = x;
        kart.prog += v;
        kart.braking = braking;
    }

    /// Karts that touch: the one behind is held back, both are pushed
    /// apart.
    fn bumps(&mut self) {
        let lap = self.lap_prog();
        for i in 0..KARTS {
            for j in (i + 1)..KARTS {
                let (a, b) = (self.karts[i], self.karts[j]);
                let dz = wrap(a.prog - b.prog, lap);
                if dz.abs() >= KART_LEN * SUB || (a.x - b.x).abs() >= KART_W {
                    continue;
                }
                let (front, back) = if dz >= 0 { (i, j) } else { (j, i) };
                let fv = self.karts[front].v;
                let overlap = KART_LEN * SUB - dz.abs();
                let kb = &mut self.karts[back];
                kb.v = kb.v.min(fv * 7 / 8);
                kb.prog -= overlap;
                let side = if self.karts[i].x >= self.karts[j].x {
                    1
                } else {
                    -1
                };
                self.karts[i].x += side * X_ONE / 16;
                self.karts[j].x -= side * X_ONE / 16;
                if (i == 0 || j == 0) && self.bump_cool == 0 {
                    self.bump_cool = 30;
                    self.event(RaceEvent::Bump);
                }
            }
        }
    }
}

/// `d` taken round a loop of `lap`: between -lap/2 and lap/2.
pub fn wrap(d: i32, lap: i32) -> i32 {
    let d = d.rem_euclid(lap);
    if d > lap / 2 { d - lap } else { d }
}
