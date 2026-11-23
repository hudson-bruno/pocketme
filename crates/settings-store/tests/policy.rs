//! When a board with real flash saves (settings_store::policy).

use settings_store::QUIET_US;
use settings_store::policy::{Moment, SavePolicy, TRIES_AFTER_FAIL};

const MS: u64 = 1000;

fn home() -> Moment {
    Moment::default()
}
fn change() -> Moment {
    Moment {
        changed: true,
        ..Moment::default()
    }
}
fn menu() -> Moment {
    Moment {
        menu: true,
        ..Moment::default()
    }
}
fn game() -> Moment {
    Moment {
        game_running: true,
        ..Moment::default()
    }
}

/// Steps every `every` us from `from` to `to` (inclusive) with moment
/// `m`; the times it said to save.
fn run(p: &mut SavePolicy, from: u64, to: u64, every: u64, m: Moment) -> Vec<u64> {
    let mut saves = vec![];
    let mut t = from;
    while t <= to {
        if p.after_step(t, m) {
            saves.push(t);
        }
        t += every;
    }
    saves
}

#[test]
fn nothing_changed_nothing_saved() {
    let mut p = SavePolicy::new();
    assert!(run(&mut p, 0, 10_000 * MS, 16 * MS, home()).is_empty());
    // Menus opening and closing are not changes.
    for i in 0..10 {
        assert!(!p.after_step(i * 100 * MS, menu()));
        assert!(!p.after_step(i * 100 * MS + 50 * MS, home()));
    }
    assert!(!p.pending());
    assert_eq!(p.deadline(false), None);
}

#[test]
fn saves_once_after_the_quiet_time() {
    let mut p = SavePolicy::new();
    assert!(!p.after_step(1_000 * MS, change()));
    assert_eq!(p.deadline(false), Some(1_000 * MS + QUIET_US));
    let saves = run(&mut p, 1_016 * MS, 20_000 * MS, 16 * MS, home());
    assert_eq!(saves.len(), 1);
    assert!(saves[0] >= 1_000 * MS + QUIET_US && saves[0] < 1_000 * MS + QUIET_US + 16 * MS);
    assert!(!p.pending());
}

#[test]
fn a_burst_of_changes_is_one_save() {
    let mut p = SavePolicy::new();
    let mut saves = 0;
    // A finger along the bars: a change every 300 ms for 3 s.
    for i in 0..10u64 {
        saves += u32::from(p.after_step(i * 300 * MS, change()));
        saves += run(
            &mut p,
            i * 300 * MS + 16 * MS,
            i * 300 * MS + 290 * MS,
            16 * MS,
            home(),
        )
        .len() as u32;
    }
    assert_eq!(saves, 0);
    let after = run(&mut p, 3_000 * MS, 10_000 * MS, 16 * MS, home());
    assert_eq!(after.len(), 1);
    assert!(after[0] >= 2_700 * MS + QUIET_US);
}

#[test]
fn closing_the_menu_saves_at_once() {
    let mut p = SavePolicy::new();
    assert!(!p.after_step(0, menu()));
    assert!(!p.after_step(
        100 * MS,
        Moment {
            changed: true,
            menu: true,
            ..Moment::default()
        }
    ));
    assert!(!p.after_step(200 * MS, menu()));
    // Closed well before the quiet time.
    assert!(p.after_step(300 * MS, home()));
    assert!(!p.pending());
    assert!(run(&mut p, 316 * MS, 10_000 * MS, 16 * MS, home()).is_empty());
}

#[test]
fn closing_the_menu_with_nothing_changed_saves_nothing() {
    let mut p = SavePolicy::new();
    assert!(!p.after_step(0, menu()));
    assert!(!p.after_step(100 * MS, home()));
}

#[test]
fn closing_a_pause_menu_into_the_game_saves_then() {
    let mut p = SavePolicy::new();
    assert!(!p.after_step(0, menu()));
    assert!(!p.after_step(
        50 * MS,
        Moment {
            changed: true,
            menu: true,
            ..Moment::default()
        }
    ));
    // The menu closes and the game runs again in the same step.
    assert!(p.after_step(400 * MS, game()));
    assert!(run(&mut p, 416 * MS, 10_000 * MS, 16 * MS, game()).is_empty());
}

#[test]
fn never_while_a_game_runs() {
    let mut p = SavePolicy::new();
    assert!(!p.after_step(0, change()));
    // The game starts at once and runs for 30 s: no save, no wake for it.
    assert!(run(&mut p, 16 * MS, 30_000 * MS, 16 * MS, game()).is_empty());
    assert!(p.pending());
    assert_eq!(p.deadline(true), None);
    assert_eq!(p.deadline(false), Some(QUIET_US));
    // Paused (its menu opens): saved then.
    assert!(p.after_step(30_016 * MS, menu()));
    assert!(!p.pending());
}

#[test]
fn a_save_left_waiting_by_a_game_comes_when_the_game_ends() {
    let mut p = SavePolicy::new();
    assert!(!p.after_step(0, change()));
    assert!(run(&mut p, 16 * MS, 5_000 * MS, 16 * MS, game()).is_empty());
    assert!(p.after_step(5_016 * MS, home()));
}

/// Runs 20 quiet times after a change, failing every save asked for;
/// how many were asked for.
fn fail_all(p: &mut SavePolicy, t: &mut u64) -> u32 {
    let mut tries = 0;
    for _ in 0..20 {
        let s = run(p, *t + 16 * MS, *t + 2 * QUIET_US, 16 * MS, home());
        *t += 2 * QUIET_US;
        if let Some(&at) = s.first() {
            assert_eq!(s.len(), 1);
            tries += 1;
            p.failed(at);
        }
    }
    tries
}

#[test]
fn a_failed_save_is_tried_again_a_few_times_then_waits_for_a_change() {
    let mut p = SavePolicy::new();
    assert!(!p.after_step(0, change()));
    let mut t = 0;
    assert_eq!(fail_all(&mut p, &mut t), 1 + u32::from(TRIES_AFTER_FAIL));
    assert!(!p.pending());
    // A new change starts again, with all its tries.
    assert!(!p.after_step(t, change()));
    assert_eq!(fail_all(&mut p, &mut t), 1 + u32::from(TRIES_AFTER_FAIL));
    // And a save that works is the end of it.
    assert!(!p.after_step(t, change()));
    assert_eq!(
        run(&mut p, t + 16 * MS, t + 4 * QUIET_US, 16 * MS, home()).len(),
        1
    );
}

#[test]
fn small() {
    assert!(size_of::<SavePolicy>() <= 24);
}
