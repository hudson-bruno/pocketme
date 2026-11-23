//! What the 3D view shows: the settings (detail and camera), where the
//! eye is, which things are near enough to draw, in which order, with
//! which model. The pixels themselves are [`crate::poly`]'s work.

use game_kit::Frame;

use crate::art::{self, GLOW, MATS, SHADES, mat};
use crate::course::{Kind, Mission, Thing, Things};
use crate::fixed::Mat3;
use crate::models::{CELL, GATE, POD, ROCK_A, ROCK_B, ROCK_HI, ROCK_LO, SHIP, STATION, WRECK};
use crate::poly::{Camera, M8, Model, NEAR, Object, Rect, Renderer, Shading, View};
use crate::world::{State, World};

/// The top bar's height and the bottom bar's first row.
pub const TOP_H: i32 = 10;
pub const BOTTOM_Y: i32 = 106;

/// How much is drawn. HIGH is the default (with 60 pictures a second,
/// since 2026-11-23); LOW is the one that holds 30 pictures a second on
/// one core by the send rule of thumb.
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

    /// The pixels the view fills.
    pub fn area(self) -> Rect {
        match self {
            Detail::Low => Rect::new(16, 18, 128, 80),
            _ => Rect::new(0, TOP_H, 160, BOTTOM_Y - TOP_H),
        }
    }

    pub fn view(self) -> View {
        let a = self.area();
        View::new(a, a.w() * 6 / 10)
    }

    /// How far things are drawn (1/256 m).
    pub fn far(self) -> i32 {
        match self {
            Detail::Low => 65 * M8,
            Detail::Mid => 90 * M8,
            Detail::High => 120 * M8,
        }
    }

    /// Rocks farther than this use the eight-sided model.
    pub fn simple_from(self) -> i32 {
        match self {
            Detail::Low => 25 * M8,
            Detail::Mid => 45 * M8,
            Detail::High => 70 * M8,
        }
    }

    /// Rocks nearer than this use the 80-sided model (at most
    /// [`MAX_FINE`] of them, the nearest).
    pub fn fine_to(self) -> i32 {
        match self {
            Detail::High => 20 * M8,
            _ => 0,
        }
    }

    /// A gate nearer the eye than this many times its outer radius (in
    /// tenths) is no longer drawn; 0: always drawn. A ring that fills
    /// the view changes the ends of nearly every row of the view, so
    /// the console would send nearly the whole view for each of the
    /// last few pictures before the eye passes it. LOW lets it go
    /// while it still fits in the view: the one picture it vanishes in
    /// costs about as much as its rows.
    pub fn gate_cut(self) -> i32 {
        match self {
            Detail::Low => 22,
            Detail::Mid => 0,
            Detail::High => 0,
        }
    }

    /// Most triangles one picture should fill. The renderer cannot know
    /// a picture's count before it is drawn, so [`draw`] works it out
    /// beforehand from each model's faces (about half of a closed
    /// model's faces look at the eye) and leaves out the farthest rocks,
    /// wrecks, pods and cells while the guess is over this. The
    /// craft, the gates and the station are never left out.
    pub fn tri_budget(self) -> usize {
        match self {
            Detail::Low => 160,
            Detail::Mid => 240,
            Detail::High => 320,
        }
    }

    pub fn next(self) -> Detail {
        Detail::ALL[(self as usize + 1) % 3]
    }
}

/// Where the eye is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eye {
    /// Behind and a little above the craft.
    Chase,
    /// Farther back and higher: more of the way ahead, a smaller craft.
    Wide,
    /// From the craft's nose; the craft itself is not drawn.
    Nose,
}

impl Eye {
    pub const ALL: [Eye; 3] = [Eye::Chase, Eye::Wide, Eye::Nose];

    pub fn name(self) -> &'static str {
        match self {
            Eye::Chase => "CHASE",
            Eye::Wide => "WIDE",
            Eye::Nose => "NOSE",
        }
    }

    pub fn next(self) -> Eye {
        Eye::ALL[(self as usize + 1) % 3]
    }
}

/// The camera for the world now. It slides with the craft but never
/// turns, so the stars and the planet stay on the same pixels.
pub fn camera(w: &World, eye: Eye) -> Camera {
    let s = &w.ship;
    let pos = match eye {
        Eye::Chase => [
            w.cam.0 >> 4,
            (w.cam.1 >> 4) + M8 * 3 / 2,
            (s.z >> 4) - 6 * M8,
        ],
        Eye::Wide => [
            (w.cam.0 >> 4) * 3 / 4,
            (w.cam.1 >> 4) * 3 / 4 + 3 * M8,
            (s.z >> 4) - 11 * M8,
        ],
        Eye::Nose => [s.x >> 4, (s.y >> 4) + M8 / 4, (s.z >> 4) + M8 * 2],
    };
    Camera {
        pos,
        rot: Mat3::IDENTITY,
    }
}

/// The light and the fog for a detail.
pub fn shading(d: Detail) -> Shading {
    let far = d.far();
    Shading {
        mats: MATS,
        shades: SHADES,
        // From the upper right, a little from behind the eye: (0.45,
        // -0.55, 0.70) times 16384.
        light: [7373, -9011, 11469],
        glow: GLOW,
        fog_from: far / 2,
        fog_to: far,
        fog_shades: 6,
        clear: (1 << mat::GATE_NEXT) | (1 << mat::DOCK),
        dark_side: 5,
    }
}

/// Where the craft is on the picture, in pixels (for steering by
/// finger); the view's middle in the nose view.
pub fn ship_on_screen(w: &World, d: Detail, eye: Eye) -> (i32, i32) {
    let v = d.view();
    let (cx, cy) = v.centre();
    if eye == Eye::Nose {
        return (cx / 16, cy / 16);
    }
    let c = camera(w, eye);
    let p = w.ship.pos();
    let rel = [
        p[0] - c.pos[0],
        p[1] - c.pos[1],
        (p[2] - c.pos[2]).max(NEAR),
    ];
    let s = v.project(rel);
    (s[0] / 16, s[1] / 16)
}

/// One thing to draw this picture. A course thing is kept as its slice
/// and its number in the slice (and its kind, to choose the model), not
/// as a whole [`Thing`]: the list stays small on the stack, and the
/// thing is worked out again from its slice when it is drawn.
#[derive(Clone, Copy)]
enum Item {
    Ship,
    Gate(u8),
    Station,
    Thing { slice: i16, k: u8, kind: Kind },
}

/// Most things drawn in one picture.
pub const MAX_ITEMS: usize = 48;
/// Most rocks drawn with the 80-sided model in one picture: each puts
/// about 40 triangles on screen, and HIGH must stay near 300.
pub const MAX_FINE: usize = 2;

/// The things in view, sorted far to near: kept on the stack while one
/// picture is drawn (about 0.5 KB).
struct DrawList {
    items: [Item; MAX_ITEMS],
    keys: [u32; MAX_ITEMS],
    n: usize,
}

impl DrawList {
    fn push(&mut self, depth: i32, it: Item) {
        if self.n < MAX_ITEMS {
            self.items[self.n] = it;
            self.keys[self.n] = ((depth.clamp(0, 0xF_FFFF) as u32) << 8) | self.n as u32;
            self.n += 1;
        }
    }

    /// Far to near.
    fn sort(&mut self) {
        let keys = &mut self.keys[..self.n];
        for i in 1..keys.len() {
            let k = keys[i];
            let mut j = i;
            while j > 0 && keys[j - 1] < k {
                keys[j] = keys[j - 1];
                j -= 1;
            }
            keys[j] = k;
        }
    }
}

/// Draws the 3D view: backdrop, then every object far to near.
// Not inlined: its working lists then get a stack frame of their
// own instead of making the caller's bigger (see README, "Stack").
#[inline(never)]
pub fn draw(f: &mut Frame, r: &mut Renderer, w: &World, d: Detail, eye: Eye) {
    let view = d.view();
    let cam = camera(w, eye);
    let sh = shading(d);
    let far = d.far();
    let m = w.m();
    art::backdrop(f, &view.area);
    r.begin();

    let mut list = DrawList {
        items: [Item::Ship; MAX_ITEMS],
        keys: [0; MAX_ITEMS],
        n: 0,
    };
    let eye_z = cam.pos[2];
    // The craft first, so it is never the one left out.
    if eye != Eye::Nose {
        list.push(w.ship.pos()[2] - eye_z, Item::Ship);
    }
    // Gates: the one just passed (still in front of the eye) and those
    // ahead, as far as the eye reaches; the dock and the station.
    let first = w.next_gate.saturating_sub(1);
    for i in first..=m.gates {
        let g = m.gate(i, w.ticks);
        let dz = g.z - eye_z;
        if dz > far + g.outer() {
            break;
        }
        if dz > -g.outer() && dz * 10 >= g.outer() * d.gate_cut() {
            list.push(dz, Item::Gate(i));
        }
    }
    let st = m.station_z() - eye_z;
    if st < far + 30 * M8 {
        list.push(st.max(0), Item::Station);
    }
    // Things, slice by slice, near to far (so the far ones are left out
    // if the list is full).
    let c0 = Mission::cell_of(eye_z);
    let c1 = Mission::cell_of(eye_z + far);
    let mut things = Things::new();
    for c in c0..=c1 {
        m.things(c, w.ticks, &mut things);
        for (k, t) in things.as_slice().iter().enumerate() {
            if t.kind == Kind::Cell && w.is_taken(t.id) {
                continue;
            }
            let dz = t.z - eye_z;
            if dz < -4 * M8 || dz > far + 4 * M8 {
                continue;
            }
            let it = Item::Thing {
                slice: c as i16,
                k: k as u8,
                kind: t.kind,
            };
            list.push(dz, it);
        }
    }
    list.sort();

    // Rocks near enough for the fine model; only the nearest MAX_FINE
    // get it (the list is far to near, so the first ones counted are
    // the farther ones).
    let is_fine = |k: usize| {
        let near = ((list.keys[k] >> 8) as i32) < d.fine_to();
        near && matches!(
            list.items[(list.keys[k] & 0xFF) as usize],
            Item::Thing {
                kind: Kind::Rock(_),
                ..
            }
        )
    };
    let all_fine = (0..list.n).filter(|&k| is_fine(k)).count();
    // The model each item gets (true: the 80-sided rock), and the guess
    // of the triangles they fill together.
    let mut fine = [false; MAX_ITEMS];
    let mut guess = 0usize;
    let mut fine_left = all_fine;
    for (k, f) in fine.iter_mut().enumerate().take(list.n) {
        if is_fine(k) {
            fine_left -= 1;
            *f = fine_left < MAX_FINE;
        }
        guess += tris_guess(&list, k, d, *f);
    }
    // Over the budget: leave out course things, the farthest first.
    let mut skip = [false; MAX_ITEMS];
    for (k, s) in skip.iter_mut().enumerate().take(list.n) {
        if guess <= d.tri_budget() {
            break;
        }
        if matches!(
            list.items[(list.keys[k] & 0xFF) as usize],
            Item::Thing { .. }
        ) {
            *s = true;
            guess -= tris_guess(&list, k, d, fine[k]);
        }
    }

    let hurt_blink = w.ship.hurt > 0 && (w.ship.hurt / 6) % 2 == 1;
    // The slice whose things are in `things` now (the list is sorted by
    // depth, so things of one slice mostly come one after the other).
    let mut have = None;
    for k in 0..list.n {
        if skip[k] {
            continue;
        }
        let it = list.items[(list.keys[k] & 0xFF) as usize];
        let depth = (list.keys[k] >> 8) as i32;
        let fine = fine[k];
        let o = match it {
            Item::Ship => {
                let (yaw, pitch, roll) = w.ship.angles();
                let swap = if hurt_blink {
                    (mat::HULL, mat::WARN)
                } else if w.state == State::Flying || w.state == State::Delivered {
                    (mat::ENGINE, mat::ENGINE)
                } else {
                    // A dead engine after a failed mission.
                    (mat::ENGINE, mat::HULL2)
                };
                Object {
                    model: &SHIP,
                    pos: w.ship.pos(),
                    rot: Mat3::rotation(yaw, pitch, roll),
                    scale: 256,
                    swap,
                }
            }
            Item::Gate(i) => {
                let g = m.gate(i, w.ticks);
                let look = if g.dock {
                    mat::DOCK
                } else if i < w.next_gate {
                    if w.gate_ok & (1 << i) != 0 {
                        mat::GATE_DONE
                    } else {
                        mat::GATE_MISSED
                    }
                } else if i == w.next_gate {
                    mat::GATE_NEXT
                } else {
                    mat::GATE_LATER
                };
                Object {
                    model: &GATE,
                    pos: [g.x, g.y, g.z],
                    rot: Mat3::IDENTITY,
                    scale: g.scale,
                    swap: (mat::GATE_LIGHT, look),
                }
            }
            Item::Station => Object {
                model: &STATION,
                pos: [0, 0, m.station_z()],
                rot: Mat3::rotation(0, 0, (w.ticks.wrapping_mul(40)) as u16),
                scale: 640,
                swap: (0, 0),
            },
            Item::Thing { slice, k, .. } => {
                let c = i32::from(slice);
                if have != Some(c) {
                    m.things(c, w.ticks, &mut things);
                    have = Some(c);
                }
                let Some(t) = things.as_slice().get(usize::from(k)) else {
                    continue;
                };
                thing_object(t, d, depth, fine)
            }
        };
        r.draw(f, &view, &cam, &sh, &o, far + 30 * M8);
    }
}

/// The triangles item `k` of the sorted list is likely to fill: half
/// its model's faces, rounded up, and two for faces cut at the view's
/// edges.
fn tris_guess(list: &DrawList, k: usize, d: Detail, fine: bool) -> usize {
    let model: &Model = match list.items[(list.keys[k] & 0xFF) as usize] {
        Item::Ship => &SHIP,
        Item::Gate(_) => &GATE,
        Item::Station => &STATION,
        Item::Thing { kind, .. } => thing_model(kind, d, (list.keys[k] >> 8) as i32, fine),
    };
    model.faces.len().div_ceil(2) + 2
}

/// The model of a course thing; rocks change model with distance
/// (`fine`: this rock gets the 80-sided model).
fn thing_model(kind: Kind, d: Detail, depth: i32, fine: bool) -> &'static Model {
    match kind {
        Kind::Rock(_) if fine => &ROCK_HI,
        Kind::Rock(_) if depth > d.simple_from() => &ROCK_LO,
        Kind::Rock(0) => &ROCK_A,
        Kind::Rock(_) => &ROCK_B,
        Kind::Wreck => &WRECK,
        Kind::Pod => &POD,
        Kind::Cell => &CELL,
    }
}

/// The model and place of a course thing.
fn thing_object(t: &Thing, d: Detail, depth: i32, fine: bool) -> Object<'static> {
    let scale = match t.kind {
        Kind::Rock(_) => t.size,
        _ => 256,
    };
    Object {
        model: thing_model(t.kind, d, depth, fine),
        pos: [t.x, t.y, t.z],
        rot: Mat3::rotation(t.angles.0, t.angles.1, t.angles.2),
        scale,
        swap: (0, 0),
    }
}
