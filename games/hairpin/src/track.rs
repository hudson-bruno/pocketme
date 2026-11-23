//! The three courses: each a loop of road cut into short *segments*
//! (200 world units long). A course is written as a list of *sections*
//! (a length, a bend, a climb, what stands beside the road); the
//! segments are worked out from that list when they are needed, so no
//! big table sits in memory.
//!
//! Units: the road is 2 x [`ROAD_HALF`] = 2000 world units wide, a
//! segment is [`SEG`] = 200 long, heights are world units too.

use crate::art::{
    BIRCH_PIC, BUSH_PIC, CHEVRON_PIC, LAMP_PIC, MAST_PIC, PINE_PIC, POLE_PIC, POST_PIC, Pic,
    RAIL_PIC, ROCK_PIC,
};

/// Length of one segment, in world units.
pub const SEG: i32 = 200;
/// From the middle of the road to its edge, in world units.
pub const ROAD_HALF: i32 = 1000;
/// Segments per kerb stripe (red, then white).
pub const RUMBLE_SEGS: u32 = 3;
/// One step of a section's climb, in world units.
pub const HILL_UNIT: i32 = 100;
/// Most sections a course may have.
pub const MAX_SECTIONS: usize = 24;
/// A bend this sharp or sharper gets warning signs.
pub const SHARP: i8 = 9;

/// What stands beside the road in a section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deco {
    Open,
    Posts,
    Birch,
    Pines,
    /// Water on the left behind a railing.
    Lake,
    Lamps,
    /// Rocks and gravel shoulders.
    Rocks,
    Snow,
    /// Snow, and a radio mast.
    Mast,
}

/// One piece of a course.
#[derive(Clone, Copy, Debug)]
pub struct Section {
    /// In segments.
    pub len: u16,
    /// The bend: minus is left, plus is right. 16 is a hairpin.
    pub curve: i8,
    /// The climb over the whole section, in [`HILL_UNIT`]s.
    pub hill: i8,
    pub deco: Deco,
}

const fn s(len: u16, curve: i8, hill: i8, deco: Deco) -> Section {
    Section {
        len,
        curve,
        hill,
        deco,
    }
}

/// A course: its sections and what is worked out from them.
pub struct Course {
    pub name: &'static str,
    pub sections: &'static [Section],
    /// The first segment of each section; the last entry is the length.
    start: [u32; MAX_SECTIONS + 1],
    /// The height at the start of each section.
    y0: [i32; MAX_SECTIONS + 1],
    /// Segments in one lap.
    pub segs: u32,
    /// The rivals' top speed, in 1/1024 of the top speed of your kart.
    pub skills: [i32; 4],
}

const fn course(name: &'static str, sections: &'static [Section], skills: [i32; 4]) -> Course {
    assert!(sections.len() <= MAX_SECTIONS, "too many sections");
    let mut start = [0u32; MAX_SECTIONS + 1];
    let mut y0 = [0i32; MAX_SECTIONS + 1];
    let mut i = 0;
    while i < sections.len() {
        assert!(sections[i].len >= 8, "a section is too short");
        start[i + 1] = start[i] + sections[i].len as u32;
        y0[i + 1] = y0[i] + sections[i].hill as i32 * HILL_UNIT;
        i += 1;
    }
    // A loop: the road ends where it began, at the same height.
    assert!(
        y0[sections.len()] == 0,
        "a course must climb as much as it falls"
    );
    // The start straight: no bend or climb at the line.
    assert!(sections[0].curve == 0 && sections[0].hill == 0);
    // Fill the rest, so a search past the end stays in the last section.
    let mut k = sections.len() + 1;
    while k <= MAX_SECTIONS {
        start[k] = u32::MAX;
        k += 1;
    }
    Course {
        name,
        sections,
        start,
        y0,
        segs: start[sections.len()],
        skills,
    }
}

use Deco::*;

/// Course 1: round a lake below the dam. Gentle bends, small hills.
const RESERVOIR: [Section; 14] = [
    s(50, 0, 0, Lamps),
    s(110, 4, 0, Posts),
    s(70, 0, 12, Birch),
    s(130, -6, 0, Lake),
    s(90, 0, -12, Lake),
    s(70, 8, 0, Posts),
    s(70, -8, 8, Posts),
    s(100, 0, 0, Birch),
    s(160, 4, 16, Birch),
    s(80, 0, -16, Posts),
    s(110, -10, -8, Birch),
    s(80, 0, 0, Lamps),
    s(70, 5, 0, Posts),
    s(40, 0, 0, Lamps),
];

/// Course 2: through the pine forest above the lake. Rolling hills,
/// sharper bends.
const PINEWOOD: [Section; 16] = [
    s(50, 0, 0, Lamps),
    s(90, -6, 20, Pines),
    s(60, 0, -10, Pines),
    s(80, 11, 0, Pines),
    s(60, 0, 15, Birch),
    s(70, -12, -15, Pines),
    s(70, 12, 0, Pines),
    s(110, 0, 25, Posts),
    s(90, 0, -25, Pines),
    s(120, -7, 10, Pines),
    s(80, 9, -20, Birch),
    s(80, 0, 0, Pines),
    s(100, -12, 10, Pines),
    s(60, 0, -10, Posts),
    s(90, 6, 0, Pines),
    s(60, 0, 0, Lamps),
];

/// Course 3: the hairpins up to the radio mast and the long way down.
/// Steep climbs, bends you must brake for.
const SUMMIT: [Section; 16] = [
    s(50, 0, 0, Lamps),
    s(80, 0, 30, Snow),
    s(60, 15, 0, Rocks),
    s(50, 0, 20, Snow),
    s(60, -15, 0, Rocks),
    s(60, 0, 20, Snow),
    s(60, 16, 0, Rocks),
    s(90, 0, 10, Mast),
    s(70, -9, -20, Snow),
    s(100, 0, -40, Snow),
    s(60, 14, 0, Rocks),
    s(70, -8, -10, Snow),
    s(90, 10, -10, Rocks),
    s(70, 0, 0, Snow),
    s(90, -16, 0, Rocks),
    s(60, 0, 0, Lamps),
];

pub static COURSES: [Course; 3] = [
    course("RESERVOIR", &RESERVOIR, [880, 860, 840, 820]),
    course("PINEWOOD", &PINEWOOD, [935, 910, 885, 860]),
    course("SUMMIT", &SUMMIT, [965, 945, 920, 895]),
];

/// One segment, worked out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seg {
    /// The bend, in 1/16 units (a hairpin is 16 x 16 = 256): how much
    /// the road's direction changes from this segment to the next.
    pub curve16: i32,
    /// The road's height at the start of the segment.
    pub y: i32,
    pub deco: Deco,
    pub section: u8,
}

/// Smooth step from 0 to 1024 as `t` goes from 0 to 1024.
const fn smooth(t: i32) -> i32 {
    (t * t / 1024) * (3 * 1024 - 2 * t) / 1024
}

/// What stands beside the road.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Pine,
    Birch,
    Rock,
    Post,
    Pole,
    Lamp,
    Chevron,
    Rail,
    Bush,
    Mast,
    /// The start and finish arch (drawn in its own way).
    Gantry,
}

impl Kind {
    /// The picture.
    pub fn pic(self) -> &'static Pic {
        match self {
            Kind::Pine => &PINE_PIC,
            Kind::Birch => &BIRCH_PIC,
            Kind::Rock => &ROCK_PIC,
            Kind::Post | Kind::Gantry => &POST_PIC,
            Kind::Pole => &POLE_PIC,
            Kind::Lamp => &LAMP_PIC,
            Kind::Chevron => &CHEVRON_PIC,
            Kind::Rail => &RAIL_PIC,
            Kind::Bush => &BUSH_PIC,
            Kind::Mast => &MAST_PIC,
        }
    }

    /// Width in world units (the height follows from the picture).
    pub fn width(self) -> i32 {
        match self {
            Kind::Pine => 1150,
            Kind::Birch => 700,
            Kind::Rock => 900,
            Kind::Post => 110,
            Kind::Pole => 64,
            Kind::Lamp => 400,
            Kind::Chevron => 620,
            Kind::Rail => 120,
            Kind::Bush => 620,
            Kind::Mast => 600,
            Kind::Gantry => 120,
        }
    }

    /// A kart that runs into it stops short. Bushes are only looks.
    pub fn solid(self) -> bool {
        !matches!(self, Kind::Bush)
    }
}

/// One thing beside the road.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Thing {
    pub kind: Kind,
    /// Across the road, in 1/1024 of [`ROAD_HALF`] (minus: left).
    pub x: i16,
    /// Drawn mirrored.
    pub flip: bool,
}

/// The ground beside the road, besides grass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shoulder {
    None,
    /// Gravel (or dirt) on both sides, out to 1.5 road halves.
    Gravel,
    /// Water on the left, from 1.3 road halves out.
    WaterLeft,
}

/// Mixes a segment number into 32 bits that look random.
fn hash(i: u32) -> u32 {
    let mut h = i.wrapping_mul(0x9E37_79B1);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85EB_CA77);
    h ^= h >> 13;
    h
}

impl Course {
    /// The length of one lap, in world units.
    pub fn lap_z(&self) -> i32 {
        self.segs as i32 * SEG
    }

    /// The section holding segment `i` (taken round the loop).
    pub fn section_of(&self, i: u32) -> usize {
        let i = i % self.segs;
        // The first start above `i`, minus one.
        let (mut lo, mut hi) = (0usize, self.sections.len());
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if self.start[mid] <= i {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// Segment `i` (any number: taken round the loop).
    pub fn seg(&self, i: u32) -> Seg {
        let i = i % self.segs;
        let n = self.section_of(i);
        let sec = &self.sections[n];
        let len = i32::from(sec.len);
        let k = (i - self.start[n]) as i32;
        // The bend eases in over the first quarter and out over the last.
        let q = (len / 4).clamp(1, 24);
        let ramp = (k + 1).min(len - k).min(q);
        let curve16 = i32::from(sec.curve) * 16 * ramp / q;
        let climb = i32::from(sec.hill) * HILL_UNIT;
        let y = self.y0[n] + climb * smooth(k * 1024 / len) / 1024;
        Seg {
            curve16,
            y,
            deco: sec.deco,
            section: n as u8,
        }
    }

    /// The road's height at `z` (world units, any value: taken round
    /// the loop), between the segments' heights.
    pub fn y_at(&self, z: i32) -> i32 {
        let z = z.rem_euclid(self.lap_z());
        let i = (z / SEG) as u32;
        let t = z % SEG;
        let a = self.seg(i).y;
        let b = self.seg(i + 1).y;
        a + (b - a) * t / SEG
    }

    /// How steep the road is at segment `i`: the climb to the next
    /// segment, in world units.
    pub fn slope(&self, i: u32) -> i32 {
        self.seg(i + 1).y - self.seg(i).y
    }

    /// The sharpest bend (|curve16|) from segment `i` over the next
    /// `n` segments, with the sign of that bend.
    pub fn sharpest(&self, i: u32, n: u32) -> i32 {
        let mut best = 0i32;
        for k in 0..n {
            let c = self.seg(i + k).curve16;
            if c.abs() > best.abs() {
                best = c;
            }
        }
        best
    }

    /// The ground beside segment `i`.
    pub fn shoulder(&self, i: u32) -> Shoulder {
        match self.seg(i).deco {
            Deco::Lake => Shoulder::WaterLeft,
            Deco::Rocks => Shoulder::Gravel,
            _ => Shoulder::None,
        }
    }

    /// What stands at segment `i`: one thing on the left, one on the
    /// right, or none. The same every time (worked out from `i`).
    pub fn things(&self, i: u32) -> [Option<Thing>; 2] {
        let i = i % self.segs;
        if i == 0 {
            return [
                Some(Thing {
                    kind: Kind::Gantry,
                    x: 0,
                    flip: false,
                }),
                None,
            ];
        }
        let n = self.section_of(i);
        let sec = &self.sections[n];
        let k = i - self.start[n];
        let h = hash(i);
        let t = |kind: Kind, x: i32, flip: bool| {
            Some(Thing {
                kind,
                x: x as i16,
                flip,
            })
        };
        // Bend signs: before a sharp bend and along its outside.
        let next = &self.sections[(n + 1) % self.sections.len()];
        let to_next = self.start[n + 1] - i;
        let sign = |c: i8| -> Option<(i32, bool)> {
            // A right bend (plus) has its outside on the left; the sign
            // points right. Mirrored for a left bend.
            if c >= SHARP {
                Some((-1, false))
            } else if c <= -SHARP {
                Some((1, true))
            } else {
                None
            }
        };
        let mut chevron = None;
        if sec.curve.abs() < SHARP
            && let Some((side, flip)) = sign(next.curve)
            && to_next <= 30
            && to_next.is_multiple_of(6)
        {
            chevron = Some((side, flip));
        }
        if let Some((side, flip)) = sign(sec.curve)
            && k % 8 == 4
            && k + 6 < u32::from(sec.len)
        {
            chevron = Some((side, flip));
        }
        let mut out = [None, None];
        if let Some((side, flip)) = chevron {
            let slot = if side < 0 { 0 } else { 1 };
            out[slot] = t(Kind::Chevron, side * 1450, flip);
        }

        let far = |hh: u32, from: i32, span: i32| from + (hh % span as u32) as i32;
        let (l, r) = match sec.deco {
            Deco::Open => {
                let p = (k % 20 == 10).then_some(Kind::Post);
                (p.and_then(|p| t(p, -1250, false)), None)
            }
            Deco::Posts => (
                k.is_multiple_of(6)
                    .then(|| t(Kind::Post, -1250, false))
                    .flatten(),
                (k % 6 == 3).then(|| t(Kind::Post, 1250, false)).flatten(),
            ),
            Deco::Birch => (
                h.is_multiple_of(5)
                    .then(|| t(Kind::Birch, -far(h >> 8, 1500, 1400), false))
                    .flatten(),
                (h >> 4)
                    .is_multiple_of(5)
                    .then(|| t(Kind::Birch, far(h >> 12, 1500, 1400), true))
                    .flatten(),
            ),
            Deco::Pines => (
                h.is_multiple_of(3)
                    .then(|| t(Kind::Pine, -far(h >> 8, 1500, 1800), false))
                    .flatten(),
                (h >> 4)
                    .is_multiple_of(3)
                    .then(|| t(Kind::Pine, far(h >> 12, 1500, 1800), false))
                    .flatten(),
            ),
            Deco::Lake => (
                k.is_multiple_of(2)
                    .then(|| t(Kind::Rail, -1250, false))
                    .flatten(),
                h.is_multiple_of(4)
                    .then(|| t(Kind::Birch, far(h >> 8, 1500, 1000), true))
                    .flatten(),
            ),
            Deco::Lamps => (
                (k % 10 == 5).then(|| t(Kind::Post, -1250, false)).flatten(),
                // The lamp's arm reaches over the road: mirrored on the right.
                k.is_multiple_of(10)
                    .then(|| t(Kind::Lamp, 1300, true))
                    .flatten(),
            ),
            Deco::Rocks => (
                if h.is_multiple_of(4) {
                    t(Kind::Rock, -far(h >> 8, 1650, 1200), false)
                } else {
                    k.is_multiple_of(8)
                        .then(|| t(Kind::Pole, -1250, false))
                        .flatten()
                },
                if (h >> 4).is_multiple_of(4) {
                    t(Kind::Rock, far(h >> 12, 1650, 1200), true)
                } else {
                    (k % 8 == 4).then(|| t(Kind::Pole, 1250, false)).flatten()
                },
            ),
            Deco::Snow | Deco::Mast => (
                if h.is_multiple_of(4) {
                    t(Kind::Pine, -far(h >> 8, 1600, 1600), false)
                } else {
                    k.is_multiple_of(6)
                        .then(|| t(Kind::Pole, -1250, false))
                        .flatten()
                },
                if sec.deco == Deco::Mast && k == 20 {
                    t(Kind::Mast, 2600, false)
                } else if (h >> 4).is_multiple_of(4) {
                    t(Kind::Pine, far(h >> 12, 1600, 1600), false)
                } else {
                    (k % 6 == 3).then(|| t(Kind::Pole, 1250, false)).flatten()
                },
            ),
        };
        if out[0].is_none() {
            out[0] = l;
        }
        if out[1].is_none() {
            out[1] = r;
        }
        // Bushes where nothing else stands (looks only).
        if !matches!(sec.deco, Deco::Snow | Deco::Mast | Deco::Lake) {
            if out[0].is_none() && (h >> 20) % 7 == 1 {
                out[0] = t(Kind::Bush, -far(h >> 3, 1400, 1100), false);
            }
            if out[1].is_none() && (h >> 24) % 7 == 1 {
                out[1] = t(Kind::Bush, far(h >> 5, 1400, 1100), true);
            }
        }
        out
    }
}
