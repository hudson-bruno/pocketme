//! The GATHER boot animation (feature `boot-gather`) in the whole
//! console, on the test board: the animation as a pure function of time,
//! its last frame being home, skipping it at any moment, the frame rate
//! the board is asked for, SHORT and OFF, and the BOOT ANIM setting in
//! the home menu. Without the feature this file is empty (the old boot
//! screen keeps its own tests in `tests/shell.rs`).
//!
//! Run with `cargo test --features boot-gather --test gather -- --nocapture`
//! to see the measured numbers.

#![cfg(feature = "boot-gather")]

mod common;

use board_api::Wake;
use common::{DOWN, EAST, Ev, FOUR, HOME, LEFT, RIGHT, SOUTH, Sim, fingerprint};
use console_core::boot::{Anim, BOOT_FRAME_US};
use console_core::console_video::FONT_H;
use console_core::console_video::{H, W};
use console_core::gather::{self, CHIME_MS, END_MS, FINAL_MS, SHORT_AT_MS, SHRINK_AT_MS};
use console_core::pause::{self, Kind, item_y};
use console_core::theme::{INK, TITLE_X, TOP_Y};
use console_core::{FRAME_US, Page};

/// Panel pixels in a whole-screen send.
const FULL_SCREEN: u32 = 320 * 240;

/// What one step did.
#[derive(Clone, Copy, Debug)]
struct Step {
    at: u64,
    page: Page,
    /// Panel pixels sent (0: nothing sent).
    pixels: u32,
    areas: usize,
    wake: Wake,
}

/// One console step with the clock at `at`.
fn step_at(sim: &mut Sim, at: u64) -> Step {
    sim.board.clock.now_us = at;
    let before = sim.board.screen.presents;
    let wake = sim.console.step(&mut *sim.board);
    let sent = sim.board.screen.presents != before;
    let s = &sim.board.screen;
    Step {
        at,
        page: sim.console.page(),
        pixels: if sent { s.last_report.pixels } else { 0 },
        areas: if sent { s.last_area_count } else { 0 },
        wake,
    }
}

/// A console set to `anim`, powered on at time 0.
fn sim(anim: Anim) -> Sim {
    let mut sim = Sim::new(Some(FOUR));
    sim.console.set_boot_anim(anim);
    sim
}

/// Steps exactly when the console asks (as the board does) until home
/// shows, or until `until` (µs). Returns every step.
fn follow(sim: &mut Sim, until: u64) -> Vec<Step> {
    let mut steps: Vec<Step> = Vec::new();
    let mut at = 0;
    loop {
        let s = step_at(sim, at);
        assert!(
            s.areas <= 16,
            "{} areas in one send at {} µs",
            s.areas,
            s.at
        );
        steps.push(s);
        if s.page == Page::Home || s.wake.deadline() > until {
            return steps;
        }
        at = s.wake.deadline();
        assert!(steps.len() < 400, "the boot never ends");
    }
}

/// What the home screen looks like a while after `sim` reached it: the
/// picture, the palette and the panel (what was really sent).
fn settled_home(sim: &mut Sim, from: u64) -> (Vec<u8>, Vec<u16>, u64) {
    for i in 1..=30 {
        step_at(sim, from + i * FRAME_US);
    }
    assert_eq!(sim.console.page(), Page::Home);
    let v = sim.console.video();
    (
        v.frame().0.to_vec(),
        v.palette().colours().to_vec(),
        fingerprint(&sim.board.screen.image),
    )
}

/// Home as an OFF boot shows it (the same as without the feature; the
/// golden test checks that).
fn home_off() -> (Vec<u8>, Vec<u16>, u64) {
    let mut s = sim(Anim::Off);
    let steps = follow(&mut s, u64::MAX);
    assert_eq!(steps.len(), 1, "OFF goes home at the first step");
    settled_home(&mut s, 0)
}

// ---- The drawing alone ---------------------------------------------------------

#[test]
fn the_animation_is_a_pure_function_of_time() {
    let mut a = [0x55u8; W * H];
    let mut b = [0xAAu8; W * H];
    for short in [false, true] {
        for t in (0..=END_MS + 200).step_by(13) {
            // Whatever was in the picture before, and in any order.
            let over_a = gather::draw(t, short, &mut a);
            gather::draw(END_MS - t.min(END_MS), short, &mut b);
            let over_b = gather::draw(t, short, &mut b);
            assert_eq!(a, b, "t={t} short={short}");
            assert_eq!(over_a, over_b);
            assert_eq!(over_a, t >= END_MS);
        }
    }
}

#[test]
fn short_draws_the_same_as_full_from_the_shrink_on() {
    let mut a = [0u8; W * H];
    let mut b = [0u8; W * H];
    for t in SHRINK_AT_MS..=END_MS {
        gather::draw(t, false, &mut a);
        gather::draw(t, true, &mut b);
        assert_eq!(a, b, "t={t}");
    }
    // The chime moment comes after SHORT joins and before the shrink.
    const { assert!(SHORT_AT_MS < CHIME_MS && CHIME_MS < SHRINK_AT_MS) };
}

#[test]
fn the_last_frame_is_still_from_final_to_end() {
    let mut last = [0u8; W * H];
    gather::draw_final(&mut last);
    let mut f = [0u8; W * H];
    for t in FINAL_MS..=END_MS {
        gather::draw(t, false, &mut f);
        assert_eq!(f, last, "t={t}");
    }
}

// ---- In the console ------------------------------------------------------------

/// The board is asked for a frame every `BOOT_FRAME_US` while something
/// moves, then once more when the boot ends. The first picture is a
/// whole-screen send; after that only the changed spots, and the
/// switch to home sends only what home adds.
#[test]
fn full_asks_for_frames_at_its_rate_and_ends_on_home() {
    let mut s = sim(Anim::Full);
    let steps = follow(&mut s, u64::MAX);
    let home = steps.last().unwrap();
    assert_eq!(home.page, Page::Home);
    assert_eq!(home.at, u64::from(END_MS) * 1000, "home at the end");
    assert_eq!(steps[0].pixels, FULL_SCREEN, "the first picture is whole");
    let boot = &steps[..steps.len() - 1];
    for w in boot.windows(2) {
        if w[0].at + BOOT_FRAME_US < u64::from(FINAL_MS) * 1000 {
            assert_eq!(w[1].at - w[0].at, BOOT_FRAME_US, "at {} µs", w[0].at);
        }
    }
    let worst = boot[1..].iter().map(|s| s.pixels).max().unwrap();
    let sent: u64 = boot.iter().map(|s| u64::from(s.pixels)).sum();
    assert!(worst < FULL_SCREEN / 2, "a boot frame sent {worst} pixels");
    // Home's own things (tiles, gear, pad sign) are new on the panel,
    // the background, stars, bar and title are not.
    assert!(
        home.pixels < FULL_SCREEN / 2,
        "the switch to home sent {} pixels",
        home.pixels
    );
    println!(
        "FULL at {} µs a frame: {} steps, {} pictures sent, {} panel pixels in all, \
         worst frame after the first {} px, switch to home {} px in {} areas",
        BOOT_FRAME_US,
        boot.len(),
        boot.iter().filter(|s| s.pixels > 0).count(),
        sent,
        worst,
        home.pixels,
        home.areas
    );
    assert_eq!(settled_home(&mut s, home.at), home_off());
}

/// The boot's last picture is home without home's own things: home
/// only draws over it (tiles, gear, pad mark, hints), never takes
/// anything away, and the title is the same pixel for pixel. Every
/// colour the last picture uses is the same in home's palette, so the
/// panel keeps what it shows.
#[test]
fn the_boot_s_last_picture_is_part_of_home() {
    let mut s = sim(Anim::Full);
    follow(&mut s, u64::from(END_MS) * 1000 - 1);
    assert_eq!(s.console.page(), Page::Boot);
    let last = s.console.video().frame().0;
    let mut fin = [0u8; W * H];
    gather::draw_final(&mut fin);
    assert!(last == fin, "the last boot picture is the final frame");
    let boot_pal = *s.console.video().palette().colours();
    step_at(&mut s, u64::from(END_MS) * 1000);
    assert_eq!(s.console.page(), Page::Home);
    let home = s.console.video().frame().0;
    let home_pal = s.console.video().palette().colours();
    let mut kept = 0;
    for (i, (&b, &h)) in last.iter().zip(home.iter()).enumerate() {
        assert_eq!(
            boot_pal[usize::from(b)],
            home_pal[usize::from(b)],
            "colour {b}"
        );
        if b == h {
            kept += 1;
        } else {
            assert_ne!(h, INK, "home takes away pixel {i}");
        }
    }
    let (x0, y0) = (TITLE_X as usize, TOP_Y as usize);
    for y in y0..y0 + FONT_H as usize {
        for x in x0..x0 + gather::NAME_W {
            assert_eq!(home[y * W + x], last[y * W + x], "title at {x},{y}");
        }
    }
    println!(
        "home keeps {kept} of the {} pixels of the boot's last picture",
        W * H
    );
}

#[test]
fn short_is_under_a_second_and_ends_on_the_same_home() {
    let mut s = sim(Anim::Short);
    let steps = follow(&mut s, u64::MAX);
    let home = steps.last().unwrap();
    assert_eq!(home.page, Page::Home);
    assert_eq!(home.at, u64::from(END_MS - SHORT_AT_MS) * 1000);
    assert!(home.at <= 1_000_000, "SHORT takes {} µs", home.at);
    assert!(home.pixels < FULL_SCREEN / 2);
    println!(
        "SHORT: home at {} µs, {} pictures sent, switch to home {} px",
        home.at,
        steps.iter().filter(|s| s.pixels > 0).count() - 1,
        home.pixels
    );
    assert_eq!(settled_home(&mut s, home.at), home_off());
}

#[test]
fn off_goes_home_at_once() {
    let mut s = sim(Anim::Off);
    let steps = follow(&mut s, u64::MAX);
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].page, Page::Home);
    assert_eq!(steps[0].pixels, FULL_SCREEN);
    assert_eq!(s.board.log.last(), "console: home");
}

/// A button or a tap at any moment ends the boot in that step, and the
/// home that follows (picture, palette and the panel itself) is the one
/// an OFF boot shows.
#[test]
fn a_button_or_a_tap_skips_at_any_moment() {
    let want = home_off();
    let mut tried = 0;
    for (anim, every) in [(Anim::Full, 173), (Anim::Short, 61)] {
        let long = u64::from(END_MS - if anim == Anim::Short { SHORT_AT_MS } else { 0 });
        for ms in (0..long).step_by(every) {
            for tap in [false, true] {
                let mut s = sim(anim);
                let at = ms * 1000;
                let mut wake = None;
                if at > 0 {
                    let steps = follow(&mut s, at);
                    let last = steps.last().unwrap();
                    assert_eq!(last.page, Page::Boot, "{anim:?} {ms} ms");
                    wake = Some(last.wake);
                }
                s.input(if tap {
                    Ev::Touch(80, 60)
                } else {
                    Ev::Press(SOUTH)
                });
                // The board looks at the input at the next frame (or at
                // once, when the console waits for input).
                let at = match wake {
                    Some(w) if !w.on_input() => w.deadline(),
                    _ => at + 1,
                };
                let skip = step_at(&mut s, at);
                assert_eq!(skip.page, Page::Home, "{anim:?} {ms} ms tap={tap}");
                s.input(if tap { Ev::Lift } else { Ev::Release(SOUTH) });
                step_at(&mut s, at + FRAME_US);
                let now = settled_home(&mut s, at + FRAME_US);
                assert!(now == want, "{anim:?} {ms} ms tap={tap}: home differs");
                // The press or tap that skipped did not act on home.
                assert_eq!(s.board.games.started, 0);
                assert!(!s.console.menu_open());
                tried += 1;
            }
        }
    }
    assert!(tried > 40);
}

/// A board that steps only 30 times a second (or late) sees the same
/// pictures at the same times, and home within one of its frames.
#[test]
fn a_slower_board_sees_the_same_pictures_at_the_same_times() {
    let mut fast = sim(Anim::Full);
    let mut pictures = Vec::new();
    let steps = follow_keeping(&mut fast, &mut pictures);
    let end = steps.last().unwrap().at;
    let mut slow = sim(Anim::Full);
    let mut at = 0;
    loop {
        let st = step_at(&mut slow, at);
        if st.page == Page::Home {
            assert!(at >= end && at < end + 2 * BOOT_FRAME_US);
            break;
        }
        // While it moves the fast board stepped at the same time; once it
        // stands still the fast board's last picture holds.
        let (t, p) = pictures.iter().rfind(|(t, _)| *t <= at).unwrap();
        assert!(*t == at || *t >= u64::from(FINAL_MS) * 1000, "at {at} µs");
        assert!(*p == slow.console.video().frame().0, "at {at} µs");
        at += 2 * BOOT_FRAME_US;
    }
}

/// [`follow`], keeping each boot picture with its time.
fn follow_keeping(sim: &mut Sim, pictures: &mut Vec<(u64, Vec<u8>)>) -> Vec<Step> {
    let mut steps: Vec<Step> = Vec::new();
    let mut at = 0;
    loop {
        let s = step_at(sim, at);
        steps.push(s);
        if s.page == Page::Home {
            return steps;
        }
        pictures.push((at, sim.console.video().frame().0.to_vec()));
        at = s.wake.deadline();
    }
}

// ---- The setting ---------------------------------------------------------------

#[test]
fn a_new_console_plays_full() {
    assert_eq!(console_core::Console::new().boot_anim(), Anim::Full);
    assert_eq!(Anim::default(), Anim::Full);
    for a in Anim::ALL {
        assert_eq!(Anim::from_byte(a.byte()), a);
    }
    assert_eq!(Anim::from_byte(3), Anim::Full);
    assert_eq!(Anim::from_byte(0xFF), Anim::Full, "a blank byte");
}

#[test]
fn the_home_menu_changes_the_boot_animation() {
    let mut s = Sim::new(Some(FOUR));
    s.boot();
    assert!(!s.console.take_settings_changed());
    s.tap_button(HOME);
    assert!(s.console.menu_open());
    let row = Kind::Home.index_of(pause::Item::Boot).unwrap();
    for _ in 0..row {
        s.tap_button(DOWN);
    }
    assert_eq!(usize::from(s.console.menu_item()), row);
    // OK goes round FULL, SHORT, OFF; LEFT and RIGHT stop at the ends.
    let mut seen = Vec::new();
    for b in [SOUTH, RIGHT, RIGHT, LEFT, LEFT, LEFT, SOUTH, SOUTH, SOUTH] {
        s.tap_button(b);
        seen.push(s.console.boot_anim());
    }
    use Anim::*;
    assert_eq!(seen, [Short, Off, Off, Short, Full, Full, Short, Off, Full]);
    assert!(s.console.take_settings_changed(), "the board saves it");
    assert!(s.console.menu_open(), "the menu stays open");
    let lines = s.board.log.lines();
    assert!(lines.iter().any(|l| l == "console: boot anim SHORT"));
    assert!(lines.iter().any(|l| l == "console: boot anim OFF"));
    // A tap on the line changes it too.
    let (x, y) = (80, (item_y(Kind::Home, row) + 4) as u16);
    s.tap_at(x, y);
    assert_eq!(s.console.boot_anim(), Short);
    // Back closes the menu and keeps the value; home did not move.
    s.tap_button(EAST);
    assert!(!s.console.menu_open());
    assert_eq!(s.console.boot_anim(), Short);
    assert_eq!(s.board.games.started, 0);
    assert_eq!(s.console.page(), Page::Home);
}

/// The value a board sets at start is what the boot plays, and setting
/// it is not a change by the player (nothing to save).
#[test]
fn the_board_s_value_is_played_and_not_saved_again() {
    let mut s = sim(Anim::Short);
    assert_eq!(s.console.boot_anim(), Anim::Short);
    let steps = follow(&mut s, u64::MAX);
    assert!(steps.last().unwrap().at <= 1_000_000);
    assert!(!s.console.take_settings_changed());
}
