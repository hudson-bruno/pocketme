//! The speed work's guard: every picture of a set of drives, hashed and
//! compared with the pinned list in `tests/speed_baseline.txt`.
//!
//! The speed work must leave every pixel as it is. This test draws the
//! test driver's whole scripted drive (20 m/s, 14 m/s in the corners,
//! to standing past the finish) at LOW, MID and HIGH and hashes every
//! picture; then the same with the line of numbers on, the START menu
//! on each of its lines, the car pushing into the hut until the
//! "blocked" hint shows, reversing from the start, a Y reset and a
//! START restart, a second drive with the handbrake over the grass and
//! through the ford, and a run of the whole game through game-kit's
//! stand-in console (its palette too).
//!
//! The file has one line per 10 pictures (each picture's 64-bit FNV-1a
//! hash folded to 32 bits), and a total per section (a 64-bit hash of
//! all the section's picture hashes, and the count). On a difference the
//! test names the section, the first picture that differs and how many
//! lines differ.
//!
//! The `line-*` sections show the line of numbers (T, O, F, P, C, S):
//! a change that leaves the pixels alone but changes a count (say,
//! skipping a fill that draws no pixel) shows only there.
//!
//! `UPDATE_SPEED_BASELINE=1 cargo test --offline --test speed_baseline`
//! writes the file anew (only after a deliberate change of the picture).

mod common;

use common::autopilot;
use drive3d::camera::Chase;
use drive3d::car::{Car, Controls};
use drive3d::detail::Detail;
use drive3d::draw::{self, Opts, PALETTE};
use drive3d::land::Stretch;
use drive3d::look::Look;
use drive3d::maths::{dir, yaw_of};
use drive3d::menu::Menu;
use drive3d::play::{Input, Play};
use drive3d::road::{POINTS, ROAD};
use drive3d::scene::Painter;
use drive3d::things::{THINGS, kind};
use drive3d::tune::{FORD, GRASS, M, RESET_STEPS};
use game_kit::button;
use game_kit::testing::TestConsole;
use game_kit::{Frame, H, W};

const LEVELS: [Detail; 3] = [Detail::Low, Detail::Mid, Detail::High];
const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/speed_baseline.txt");

const OFF: Opts = Opts {
    line: false,
    plain: false,
};
const ON: Opts = Opts {
    line: true,
    plain: false,
};

fn fnv(h: u64, bytes: &[u8]) -> u64 {
    let mut h = h;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

const FNV_START: u64 = 0xcbf2_9ce4_8422_2325;

fn hash(f: &Frame) -> u64 {
    fnv(FNV_START, &f.0)
}

/// One section of the file: its lines of 10 folded hashes and a total.
fn section(name: &str, hashes: &[u64]) -> Vec<String> {
    let mut out = Vec::new();
    for (k, chunk) in hashes.chunks(10).enumerate() {
        let mut l = format!("{name} {:04}", k * 10);
        for h in chunk {
            l += &format!(" {:08x}", (h ^ (h >> 32)) as u32);
        }
        out.push(l);
    }
    let mut t = FNV_START;
    for h in hashes {
        t = fnv(t, &h.to_le_bytes());
    }
    out.push(format!("{name} total {t:016x} pictures {}", hashes.len()));
    out
}

/// Draws pictures the way the game does: one painter kept for a run.
struct Drawer {
    paint: Box<Painter>,
    f: Box<Frame>,
}

impl Drawer {
    fn new() -> Drawer {
        Drawer {
            paint: Box::new(Painter::new()),
            f: Box::new(Frame([0; W * H])),
        }
    }
    fn draw(&mut self, p: &Play, d: Detail, menu: Option<&Menu>, opts: Opts) -> u64 {
        let mut q = *p;
        q.detail = d;
        let look = Look::new(&q, 1);
        draw::picture(&mut self.f, &mut self.paint, &look, menu, opts, &mut ());
        hash(&self.f)
    }
}

// ---- Drives ----------------------------------------------------------

/// The scripted drive of `tests/drive.rs`: every state from the start
/// to standing past the finish.
pub fn scripted() -> Vec<Play> {
    let mut p = Play::new();
    let mut out = vec![p];
    for _ in 0..1_500 {
        let input = autopilot(&p, 20 * M / 30, 14 * M / 30);
        p.step(&input);
        out.push(p);
        if p.finished.is_some() && p.car.speed() == 0 {
            break;
        }
    }
    out
}

fn hold(c: Controls) -> Input {
    Input {
        controls: c,
        ..Input::default()
    }
}

/// From the start, full brake (reverse) for 90 steps.
fn reversing() -> Vec<Play> {
    let mut p = Play::new();
    let mut out = vec![p];
    for _ in 0..90 {
        p.step(&hold(Controls {
            brake: 256,
            ..Controls::default()
        }));
        out.push(p);
    }
    out
}

/// The car set 12 m before the hut, pushing into it for 150 steps
/// (`tests/final_look.rs`): the hint shows after a second blocked.
fn into_the_hut() -> Vec<Play> {
    let hut = THINGS
        .iter()
        .find(|t| t.kind == kind::HUT)
        .expect("the hut");
    let (hx, hz) = hut.at();
    let (sx, sz) = (hx - 12 * M, hz);
    let yaw = yaw_of(hx - sx, hz - sz).expect("a way");
    let mut p = Play::new();
    p.car = Car::standing(&Stretch, sx, sz, yaw);
    p.cam = Chase::behind(&p.car, &Stretch);
    let mut out = vec![p];
    for _ in 0..150 {
        p.step(&hold(Controls {
            throttle: 256,
            ..Controls::default()
        }));
        out.push(p);
    }
    out
}

/// The test driver aiming `side` subs to the right of the road (and
/// pulling the handbrake when `hb`).
fn off_line(p: &Play, side: i32, hb: bool) -> Input {
    let mut i = autopilot(p, 20 * M / 30, 15 * M / 30);
    let k = (usize::from(p.last_road) + 5).min(POINTS - 1);
    let (s, c) = dir(ROAD.heading[k]);
    // Right of the heading: (cos, -sin).
    let tx = ROAD.x[k] + (i64::from(c) * i64::from(side) / 16_384) as i32;
    let tz = ROAD.z[k] - (i64::from(s) * i64::from(side) / 16_384) as i32;
    let car = &p.car;
    if let Some(want) = yaw_of(tx - car.pos[0], tz - car.pos[2]) {
        let off = drive3d::maths::angle_diff(car.yaw, want);
        i.controls.aim = (off * 256 / 2_912).clamp(-256, 256);
    }
    i.controls.handbrake = hb;
    i
}

/// The second drive: off the road onto the grass with handbrake slides,
/// back over the road, and through the ford, to 12 m past the ford's
/// middle (road point 98; at most 900 steps).
pub fn slides() -> Vec<Play> {
    let mut p = Play::new();
    let mut out = vec![p];
    for n in 0..900u32 {
        let side = match p.last_road {
            0..10 => 0,
            10..30 => 10 * M,
            30..50 => -9 * M,
            50..70 => 6 * M,
            _ => -5 * M,
        };
        let hb = n >= 50 && n % 16 < 4;
        let i = off_line(&p, side, hb);
        p.step(&i);
        out.push(p);
        if p.last_road >= 98 {
            break;
        }
    }
    out
}

/// Y and START: the test driver for 150 steps, then full right lock
/// off the road for 60, Y, 60 more steps (the car waits `RESET_STEPS`
/// and is put back), then START and 30 steps from the start.
fn reset_and_restart() -> Vec<Play> {
    let mut p = Play::new();
    let mut out = vec![p];
    let go = |p: &mut Play, i: Input, out: &mut Vec<Play>| {
        p.step(&i);
        out.push(*p);
    };
    for _ in 0..150 {
        let i = autopilot(&p, 20 * M / 30, 14 * M / 30);
        go(&mut p, i, &mut out);
    }
    for _ in 0..60 {
        let i = hold(Controls {
            aim: 256,
            throttle: 256,
            ..Controls::default()
        });
        go(&mut p, i, &mut out);
    }
    let y = Input {
        reset: true,
        ..Input::default()
    };
    go(&mut p, y, &mut out);
    for _ in 0..(usize::from(RESET_STEPS) + 15) {
        let i = autopilot(&p, 20 * M / 30, 14 * M / 30);
        go(&mut p, i, &mut out);
    }
    let start = Input {
        restart: true,
        ..Input::default()
    };
    go(&mut p, start, &mut out);
    for _ in 0..30 {
        let i = autopilot(&p, 20 * M / 30, 14 * M / 30);
        go(&mut p, i, &mut out);
    }
    out
}

// ---- Sections --------------------------------------------------------

fn every(name: &str, states: &[Play], d: Detail, opts: Opts) -> Vec<String> {
    let mut dr = Drawer::new();
    let h: Vec<u64> = states.iter().map(|p| dr.draw(p, d, None, opts)).collect();
    section(&format!("{name}-{}", d.name()), &h)
}

fn palette() -> Vec<String> {
    let mut h = FNV_START;
    for c in PALETTE.iter() {
        h = fnv(h, &c.to_le_bytes());
    }
    vec![format!("palette {h:016x} colours {}", PALETTE.len())]
}

fn drive(d: Detail) -> Vec<String> {
    every("drive", &scripted(), d, OFF)
}

fn line(d: Detail) -> Vec<String> {
    every("line", &scripted(), d, ON)
}

/// The START menu on each of its four lines, line off and on, at the
/// start, in the middle of the drive (step 300) and standing past the
/// finish, at each level.
fn menu() -> Vec<String> {
    let s = scripted();
    let moments = [s[0], s[300], *s.last().expect("a state")];
    let mut dr = Drawer::new();
    let mut h = Vec::new();
    for d in LEVELS {
        for p in &moments {
            for at in 0..4 {
                let m = Menu { at };
                h.push(dr.draw(p, d, Some(&m), OFF));
                h.push(dr.draw(p, d, Some(&m), ON));
            }
        }
    }
    section("menu", &h)
}

/// The hut push, every step; the last picture with the line on too.
fn hut(d: Detail) -> Vec<String> {
    let s = into_the_hut();
    let mut dr = Drawer::new();
    let mut h: Vec<u64> = s.iter().map(|p| dr.draw(p, d, None, OFF)).collect();
    h.push(dr.draw(s.last().expect("a state"), d, None, ON));
    section(&format!("hut-{}", d.name()), &h)
}

fn reverse(d: Detail) -> Vec<String> {
    every("reverse", &reversing(), d, OFF)
}

fn reset(d: Detail) -> Vec<String> {
    every("reset", &reset_and_restart(), d, OFF)
}

fn slide(d: Detail) -> Vec<String> {
    every("slide", &slides(), d, OFF)
}

/// The whole game through the stand-in console: start, A held for 6 s
/// (the ford), SELECT (the line), START and the menu (DOWN, A: the next
/// level), B, Y, X, then 2 s more with A. Every console frame's picture
/// and the palette the game set.
fn game() -> Vec<String> {
    // SAFETY: `game_entry` is made by `game_kit::game!`.
    let mut c = unsafe { TestConsole::new(drive3d::game_entry) };
    let mut h = Vec::new();
    let frames = |c: &mut TestConsole, n: u32, h: &mut Vec<u64>| {
        for _ in 0..n {
            c.step();
            h.push(hash(c.picture()));
        }
    };
    frames(&mut c, 15, &mut h);
    c.hold(button::A);
    frames(&mut c, 180, &mut h);
    for b in [
        button::A | button::SELECT,
        button::A,
        button::START,
        0,
        button::DOWN,
        0,
        button::A,
        0,
        button::A,
        button::A | button::Y,
        button::A,
        button::A | button::X,
        button::A,
        button::START,
        0,
        button::B,
        0,
    ] {
        c.hold(b);
        frames(&mut c, 3, &mut h);
    }
    c.hold(button::A);
    frames(&mut c, 60, &mut h);
    let mut p = FNV_START;
    for v in c.palette() {
        p = fnv(p, &v.to_le_bytes());
    }
    let mut out = section("game", &h);
    out.push(format!("game palette {p:016x}"));
    out
}

fn all() -> Vec<String> {
    let mut out =
        vec!["# DRIVE3D speed baseline: picture hashes (see tests/speed_baseline.rs)".to_string()];
    out.extend(palette());
    for d in LEVELS {
        out.extend(drive(d));
    }
    for d in LEVELS {
        out.extend(line(d));
    }
    out.extend(menu());
    for d in LEVELS {
        out.extend(hut(d));
        out.extend(reverse(d));
        out.extend(reset(d));
        out.extend(slide(d));
    }
    out.extend(game());
    out
}

fn updating() -> bool {
    std::env::var_os("UPDATE_SPEED_BASELINE").is_some()
}

/// Compares a section's lines with the pinned file's lines of the same
/// name.
fn check(name: &str, lines: Vec<String>) {
    if updating() {
        return;
    }
    let pinned = std::fs::read_to_string(PATH).expect("tests/speed_baseline.txt");
    let want: Vec<&str> = pinned
        .lines()
        .filter(|l| l.split(' ').next() == Some(name))
        .collect();
    assert!(!want.is_empty(), "no section {name} in the pinned file");
    let mut differ = 0;
    let mut first = None;
    for k in 0..want.len().max(lines.len()) {
        let (a, b) = (want.get(k).copied(), lines.get(k).map(String::as_str));
        if a != b {
            differ += 1;
            if first.is_none() {
                first = Some((a.unwrap_or("-").to_string(), b.unwrap_or("-").to_string()));
            }
        }
    }
    if let Some((a, b)) = first {
        panic!("{name}: {differ} lines differ; first:\n pinned {a}\n now    {b}");
    }
}

#[test]
fn update_the_pinned_file_when_asked() {
    if !updating() {
        return;
    }
    let mut text = all().join("\n");
    text.push('\n');
    std::fs::write(PATH, text).expect("write tests/speed_baseline.txt");
}

#[test]
fn the_palette_is_unchanged() {
    check("palette", palette());
}

#[test]
fn the_scripted_drive_low() {
    check("drive-LOW", drive(Detail::Low));
}

#[test]
fn the_scripted_drive_mid() {
    check("drive-MID", drive(Detail::Mid));
}

#[test]
fn the_scripted_drive_high() {
    check("drive-HIGH", drive(Detail::High));
}

#[test]
fn the_scripted_drive_with_the_line_low() {
    check("line-LOW", line(Detail::Low));
}

#[test]
fn the_scripted_drive_with_the_line_mid() {
    check("line-MID", line(Detail::Mid));
}

#[test]
fn the_scripted_drive_with_the_line_high() {
    check("line-HIGH", line(Detail::High));
}

#[test]
fn the_start_menu() {
    check("menu", menu());
}

#[test]
fn the_hut_reverse_reset_and_slides() {
    for d in LEVELS {
        let n = d.name();
        check(&format!("hut-{n}"), hut(d));
        check(&format!("reverse-{n}"), reverse(d));
        check(&format!("reset-{n}"), reset(d));
        check(&format!("slide-{n}"), slide(d));
    }
}

#[test]
fn the_whole_game() {
    check("game", game());
}

/// The drives really go where the sections say: the hint shows in the
/// hut push, the reset puts the car back, the restart starts again, the
/// slides reach the grass and the ford with the handbrake, and the
/// scripted drive finishes.
#[test]
fn the_drives_cover_what_they_say() {
    let s = scripted();
    let last = s.last().expect("a state");
    assert!(last.finished.is_some() && last.car.speed() == 0);
    assert!(into_the_hut().last().expect("a state").blocked > 30);
    let r = reset_and_restart();
    assert!(r.iter().any(|p| p.car.surface == GRASS));
    assert!(r.iter().any(|p| p.resetting_for_tests() > 0));
    assert!(r.iter().any(|p| p.events.put_back));
    assert!(r.iter().any(|p| p.events.restarted));
    let v = reversing();
    assert!(v.last().expect("a state").car.fwd_side().0 < -400);
    let sl = slides();
    assert!(sl.len() > 300);
    assert!(sl.iter().any(|p| p.car.surface == GRASS));
    assert!(sl.iter().any(|p| p.car.surface == FORD));
    assert!(
        sl.iter()
            .any(|p| p.car.surface == GRASS && p.car.fwd_side().1.abs() > M / 8)
    );
}

trait Resetting {
    fn resetting_for_tests(&self) -> u8;
}

impl Resetting for Play {
    fn resetting_for_tests(&self) -> u8 {
        self.reset_in
    }
}
