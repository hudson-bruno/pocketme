//! Rules review (2026-11-22): the menus and the done panel, through
//! [`Proto::control`] as the game calls it (one call = 33,333 us = one
//! step).
//!
//! Some tests here show a fault and FAIL on purpose (the review's
//! REVIEW.md lists which): each asserts what a player would expect.
//!
//! Stick y grows downwards (game-kit): +32767 is the stick pulled back
//! (towards the player), -32767 pushed forward.

use game_kit::button;
use proto3d::area::sail as sl;
use proto3d::area::{Area, Kit};
use proto3d::hero::Hero;
use proto3d::maths::cm;
use proto3d::pause::{self, Tap};
use proto3d::play::Detail;
use proto3d::{Input, Note, Proto};

const BACK: (i16, i16) = (0, 32_767);
const FWD: (i16, i16) = (0, -32_767);

fn call(p: &mut Proto, held: u32, pressed: u32, left: (i16, i16)) -> Note {
    call_t(p, held, pressed, left, None)
}

fn call_t(p: &mut Proto, held: u32, pressed: u32, left: (i16, i16), touch: Option<Tap>) -> Note {
    p.control(
        &Input {
            held: held | pressed,
            pressed,
            left,
            touch,
            delta_us: 33_333,
            ..Input::default()
        },
        |_| {},
    )
}

/// A fresh game in `area` (from the start's area menu, d-pad DOWN and
/// A), at `detail` (pause menu, d-pad, A), then 20 calls of play.
fn game_in(area: Area, detail: Detail) -> Box<Proto> {
    let mut p = Box::new(Proto::new());
    for _ in 0..area.index() {
        call(&mut p, 0, button::DOWN, (0, 0));
        call(&mut p, 0, 0, (0, 0));
    }
    call(&mut p, 0, button::A, (0, 0));
    call(&mut p, 0, 0, (0, 0));
    assert_eq!(p.play.area, area);
    if detail != p.play.detail {
        call(&mut p, 0, button::START, (0, 0));
        let mut sel = p.menu.sel;
        while sel != detail.level() {
            let b = if sel < detail.level() {
                button::DOWN
            } else {
                button::UP
            };
            call(&mut p, 0, b, (0, 0));
            sel = p.menu.sel;
        }
        assert_eq!(call(&mut p, 0, button::A, (0, 0)), Note::Detail(detail));
    }
    for _ in 0..20 {
        call(&mut p, 0, 0, (0, 0));
    }
    assert!(!p.menu.open && !p.areas.open);
    p
}

/// Running back (stick pulled), START to pause, START again to go on:
/// the player expects to be where he was, at HIGH. The pause menu
/// starts its stick edge from "not pushed", so the held stick counts as
/// a push at the first call: the highlight moves from HIGH to RESTART,
/// and the second START restarts the area.
#[test]
fn rv_rules_pause_and_resume_with_the_stick_held_back_keeps_the_run() {
    let mut p = game_in(Area::Machines, Detail::High);
    for _ in 0..30 {
        call(&mut p, 0, 0, BACK);
    }
    let steps = p.play.steps;
    assert_eq!(call(&mut p, 0, button::START, BACK), Note::None);
    assert!(p.menu.open);
    assert_eq!(p.menu.sel, Detail::High.level(), "the menu opens on HIGH");
    let n = call(&mut p, 0, button::START, BACK);
    println!(
        "START, START with the stick held back: {n:?}; steps of play {steps} -> {}",
        p.play.steps
    );
    assert_ne!(
        n,
        Note::Restart(Area::Machines),
        "pause and resume restarted the area"
    );
    assert_eq!(n, Note::Detail(Detail::High));
}

/// The same at MID with the stick pushed forward: the detail drops to
/// LOW with no move of the player's own on the menu.
#[test]
fn rv_rules_pause_and_resume_with_the_stick_held_forward_keeps_the_detail() {
    let mut p = game_in(Area::Sail, Detail::Mid);
    for _ in 0..10 {
        call(&mut p, 0, 0, FWD);
    }
    call(&mut p, 0, button::START, FWD);
    let n = call(&mut p, 0, button::START, FWD);
    println!("MID, START, START with the stick forward: {n:?}");
    assert_eq!(n, Note::Detail(Detail::Mid));
}

/// To the pause menu's last line (AREAS) with the stick, A: the area
/// menu opens on the area in use, but the stick is still held down, so
/// it moves one line at once (MACHINES -> SAIL). One more A starts SAIL.
#[test]
fn rv_rules_areas_reached_with_the_stick_opens_on_the_area_in_use() {
    let mut p = game_in(Area::Machines, Detail::Mid);
    call(&mut p, 0, button::START, (0, 0));
    // MID -> HIGH -> RESTART -> AREAS, one push each.
    for _ in 0..3 {
        call(&mut p, 0, 0, BACK);
        call(&mut p, 0, 0, (0, 0));
    }
    call(&mut p, 0, 0, BACK);
    assert_eq!(p.menu.sel, pause::AREAS);
    // A while the stick is still held down.
    call(&mut p, 0, button::A, BACK);
    assert!(p.areas.open);
    assert_eq!(p.areas.sel, Area::Machines as u8, "opens on MACHINES");
    call(&mut p, 0, 0, BACK);
    println!(
        "area menu highlight after one call with the stick still held: {}",
        p.areas.sel
    );
    assert_eq!(
        p.areas.sel,
        Area::Machines as u8,
        "the held stick moved the area menu"
    );
}

/// SAIL's goal: the hero drops onto P4 (placed just above it, the clock
/// running). The DONE panel shows the time. Before the fix the player's
/// next fresh A (a jump pressed a moment too late) restarted at once.
/// This test presses A on every call from the very next one and counts
/// the calls the panel stays: at least 15 (half a second), and A does
/// restart after that. (The review's version counted at most one call
/// and so could never pass: it was rewritten to count every call.)
#[test]
fn rv_rules_done_panel_is_not_skipped_by_an_a_press_on_the_next_call() {
    let mut p = game_in(Area::Sail, Detail::Mid);
    let b = sl::WORLD.blocks[usize::from(sl::P4)];
    let mid = [
        (i32::from(b.lo[0]) + i32::from(b.hi[0])) / 2,
        (i32::from(b.lo[2]) + i32::from(b.hi[2])) / 2,
    ];
    let mut h = Hero::new(
        [cm(mid[0] as i16), cm(b.hi[1] + 20), cm(mid[1] as i16)],
        sl::P4,
    );
    h.ground = None;
    p.play.hero = h;
    if let Kit::Sail(k) = &mut p.play.kit {
        k.started = true;
    }
    let mut done = None;
    for i in 0..30 {
        if let Note::Done(a, t) = call(&mut p, 0, 0, (0, 0)) {
            done = Some((i, a, t));
            break;
        }
    }
    let (i, a, t) = done.expect("lands on P4");
    println!("done after {i} calls: {a:?} {t:?}");
    assert!(p.play.done);
    let mut shown = 0;
    let mut restarted = None;
    for _ in 0..40 {
        let n = call(&mut p, button::A, button::A, (0, 0));
        if !p.play.done {
            restarted = Some(n);
            break;
        }
        shown += 1;
    }
    println!("A on every call: the done panel stayed {shown} calls, then {restarted:?}");
    assert!(shown >= 15, "the DONE panel went after {shown} calls");
    assert!(shown <= 16, "A must restart after half a second: {shown}");
    assert_eq!(restarted, Some(Note::Restart(Area::Sail)));
}

/// The pause menu takes no touch (the crate's doc says "The menus also
/// take a tap on a line"): a tap ended on the line RESTART does nothing.
/// This one only reports what happens; it passes either way.
#[test]
fn rv_rules_pause_menu_and_a_tap_is_reported() {
    let mut p = game_in(Area::Machines, Detail::Mid);
    call(&mut p, 0, button::START, (0, 0));
    let mut hits = 0;
    for y in 0..120 {
        for x in (0..160).step_by(8) {
            let before = p.menu;
            let n = call_t(&mut p, 0, 0, (0, 0), Some(Tap { x, y, ended: true }));
            if n != Note::None || p.menu != before {
                hits += 1;
                call(&mut p, 0, button::START, (0, 0));
            }
        }
    }
    println!("taps over the whole screen that changed the open pause menu: {hits}");
    assert!(p.menu.open);
}

/// Best times are kept per area across switches, and a switch in the
/// middle of anything (a glide here) starts the new area fresh.
#[test]
fn rv_rules_best_times_stay_per_area_after_switches() {
    let mut p = game_in(Area::Sail, Detail::Mid);
    p.best = [0, 400, 500, 600];
    // Into a glide: 2 m over P0 with B held.
    let mut h = p.play.hero;
    h.ground = None;
    h.pos[1] += cm(200);
    p.play.hero = h;
    for _ in 0..3 {
        call(&mut p, button::B, 0, (0, 0));
    }
    assert!(p.play.hero.gliding, "gliding before the switch");
    // AREAS from the pause menu with the d-pad, then STONE.
    call(&mut p, 0, button::START, (0, 0));
    while p.menu.sel != pause::AREAS {
        call(&mut p, 0, button::DOWN, (0, 0));
    }
    call(&mut p, 0, button::A, (0, 0));
    call(&mut p, 0, button::DOWN, (0, 0));
    assert_eq!(call(&mut p, 0, button::A, (0, 0)), Note::Area(Area::Stone));
    assert_eq!(p.best, [0, 400, 500, 600]);
    assert!(!p.new_best);
    assert!(!p.play.hero.gliding && p.play.hero.ground.is_some());
    assert_eq!(p.play.steps, 0);
}
