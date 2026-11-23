//! The menu fix's history check. The fix changed eight picture lines of
//! the `game` section of `tests/speed_baseline.txt` on purpose (and its
//! total). That section was re-pinned in speed round 2b; the section as
//! it was pinned BEFORE the fix is kept in
//! `tests/speed_baseline_fixes.txt` as history and is never rewritten.
//!
//! Only the `game` section changed: its script takes a START menu line
//! with A and keeps A held for two more console frames (and later takes
//! a line again the same way). Since the fix, an A or B held on from a
//! menu choice counts for nothing until it is let go, so those frames
//! show the car standing instead of setting off. Every other section
//! draws its pictures without the game's update (no menu choice by
//! button), so it cannot change.
//!
//! This file draws the `game` section exactly as `tests/speed_baseline.rs`
//! does and checks:
//! * the pictures before the menu opens are the pre-fix pinned ones,
//!   picture for picture, and the first picture after the first menu
//!   choice is not (the menu's own pictures, from its opening to that
//!   choice, were the pre-fix ones until MAX: its fifth line made the
//!   box 8 pixels taller, so they changed on purpose then);
//! * the whole section is the one in the re-pinned
//!   `tests/speed_baseline.txt` (that file's own test checks it too).
//!
//! The pre-fix pictures were drawn at MID, the game's start until
//! 2026-11-23; since then it starts at HIGH (the owner's decision), and
//! the stand-in console cannot start it anywhere else. So the first
//! check replays the game's update without the console ([`replay`]) at
//! MID; the replay is held to the console's own pictures at the start's
//! level first, picture for picture.

use drive3d::clock::Clock;
use drive3d::detail::Detail;
use drive3d::draw::{self, Opts};
use drive3d::look::Look;
use drive3d::menu::{Choice, Menu};
use drive3d::play::{Input, Play};
use drive3d::scene::Painter;
use game_kit::button;
use game_kit::testing::{PERIOD_30_US, TestConsole};
use game_kit::{Frame, H, W};

/// The `game` section as pinned before the menu fix (history).
const OLD: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/speed_baseline_fixes.txt"
);
/// The pinned file of `tests/speed_baseline.rs` (re-pinned after the fix).
const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/speed_baseline.txt");

/// Pictures before the first menu choice: 15 + 180, then six held
/// patterns of 3 frames (A|SELECT, A, START, 0, DOWN, 0), and the choice
/// frame itself.
const BEFORE_CHOICE: usize = 15 + 180 + 6 * 3 + 1;
/// Pictures before the menu opens (START is the third held pattern).
const BEFORE_MENU: usize = 15 + 180 + 2 * 3;

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

fn fold(h: u64) -> String {
    format!("{:08x}", (h ^ (h >> 32)) as u32)
}

/// One section: its lines of 10 folded hashes and a total (the same
/// format as `tests/speed_baseline.rs`).
fn section(name: &str, hashes: &[u64]) -> Vec<String> {
    let mut out = Vec::new();
    for (k, chunk) in hashes.chunks(10).enumerate() {
        let mut l = format!("{name} {:04}", k * 10);
        for h in chunk {
            l += " ";
            l += &fold(*h);
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

/// The `game` section's pictures (the same script as
/// `tests/speed_baseline.rs`) and the palette's hash.
fn game() -> (Vec<u64>, u64) {
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
    (h, p)
}

fn lines() -> Vec<String> {
    let (h, p) = game();
    let mut out = section("game", &h);
    out.push(format!("game palette {p:016x}"));
    out
}

/// The `game` section's pictures before the first menu choice (the
/// first `BEFORE_CHOICE - 1`), drawn by replaying the game's update
/// (`src/lib.rs`) without the console, at `level` from the start: the
/// console's first call brings no time, each later one a frame of 30 a
/// second; SELECT, START, DOWN and the menu's pictures as the update
/// handles them. Only what the script uses before its first choice
/// (no Y, X, B or menu choice) is replayed.
fn replay(level: Detail) -> Vec<u64> {
    let mut play = Play::new();
    play.detail = level;
    let mut clock = Clock::new();
    let mut paint = Box::new(Painter::new());
    let mut f = Box::new(Frame([0; W * H]));
    let mut menu: Option<Menu> = None;
    let mut line = false;
    let mut was = 0u32;
    let mut held_for = vec![0u32; 15];
    held_for.extend([button::A; 180]);
    for b in [
        button::A | button::SELECT,
        button::A,
        button::START,
        0,
        button::DOWN,
        0,
    ] {
        held_for.extend([b; 3]);
    }
    let mut out = Vec::new();
    for (k, &held) in held_for.iter().enumerate() {
        let pressed = held & !was;
        was = held;
        if pressed & button::SELECT != 0 {
            line = !line;
        }
        let steps_last;
        if let Some(m) = &mut menu {
            assert_eq!(m.press(pressed), Choice::Stay, "frame {k}");
            steps_last = 0;
        } else if pressed & button::START != 0 {
            menu = Some(Menu::open(play.detail));
            steps_last = 0;
        } else {
            let delta = if k == 0 { 0 } else { PERIOD_30_US as u32 };
            let n = clock.add(delta);
            let input = Input {
                controls: drive3d::controls(held, 0, (0, 0)),
                ..Input::default()
            };
            for _ in 0..n {
                play.step(&input);
            }
            steps_last = n;
        }
        let look = Look::new(&play, steps_last);
        draw::picture(
            &mut f,
            &mut paint,
            &look,
            menu.as_ref(),
            Opts { line, plain: false },
            &mut (),
        );
        out.push(hash(&f));
    }
    assert_eq!(out.len(), BEFORE_CHOICE - 1);
    out
}

#[test]
fn the_pictures_before_the_first_menu_choice_are_the_old_ones() {
    let (h, p) = game();
    let old = std::fs::read_to_string(OLD).expect("tests/speed_baseline_fixes.txt");
    let old: Vec<&str> = old
        .lines()
        .filter(|l| l.starts_with("game 0"))
        .flat_map(|l| l.split(' ').skip(2))
        .collect();
    assert!(old.len() > BEFORE_CHOICE);
    // The replay at the start's level gives the console's pictures.
    let start = replay(Detail::START);
    for (k, (a, b)) in start.iter().zip(&h).enumerate() {
        assert_eq!(fold(*a), fold(*b), "replay picture {k}");
    }
    // At MID, the old start, it gives the pre-fix pictures.
    let mid = replay(Detail::Mid);
    for (k, (now, was)) in mid.iter().zip(&old).take(BEFORE_MENU).enumerate() {
        assert_eq!(&fold(*now), was, "picture {k}");
    }
    // The open menu: its box is taller since MAX, so these differ.
    for k in BEFORE_MENU..BEFORE_CHOICE - 1 {
        assert_ne!(fold(mid[k]), old[k], "menu picture {k}");
    }
    // The first picture after the choice, with A still held, is new.
    assert_ne!(fold(h[BEFORE_CHOICE]), old[BEFORE_CHOICE]);
    // The palette is the old one.
    let pinned = std::fs::read_to_string(OLD).expect("tests/speed_baseline_fixes.txt");
    assert!(pinned.contains(&format!("game palette {p:016x}")));
}

#[test]
fn the_changed_section_is_the_pinned_one() {
    let now = lines();
    let pinned = std::fs::read_to_string(PATH).expect("tests/speed_baseline.txt");
    let want: Vec<&str> = pinned
        .lines()
        .filter(|l| l.split(' ').next() == Some("game"))
        .collect();
    assert_eq!(want.len(), now.len(), "line count");
    for (a, b) in want.iter().zip(&now) {
        assert_eq!(a, b);
    }
}
