//! Pictures for people to look at, ignored by default.
//!
//! `pictures`: the rebuilt rivals: the grid at the start, a fight in a
//! corner, the results and the CATCH-UP line of the title's OPTIONS
//! page, written to the folder in `CAMBER_PICTURES`:
//! `CAMBER_PICTURES=<dir> cargo test --release --test pictures -- --ignored`.
//! The race is WILD on LAGOON, catch-up LIGHT, the player's ship flown
//! by the computer's pilot through the keys (a test pilot, not a person).
//!
//! `title_pictures`: the title (its menu and the made-up race behind
//! it), written to the folder in `CAMBER_TITLE_PICTURES`, with a list of
//! what each shows (`shots.txt`).
//!
//! `backdrop_pictures`: the SHIP backdrop at eight angles on each track,
//! the OPTIONS page with BACKDROP picked at both values, and four
//! moments of the RACE backdrop, written to the folder in
//! `CAMBER_BACKDROP_PICTURES`.

mod common;

use std::path::{Path, PathBuf};

use std::fmt::Write as _;

use camber::app::{Backdrop, Page, Scene};
use camber::fixed::turn;
use camber::race::{CatchUp, Class, Mode, Phase, SHIPS};
use camber::scene::{Detail, ViewKind};
use camber::track::{FM, TRACKS};
use camber::tv::{self, Shot};
use common::{Driver, check_palette, save_frame, save_strip};
use game_kit::Frame;

fn save(d: &Driver, dir: &Path, name: &str) {
    check_palette(&d.frame, name);
    save_frame(&dir.join(format!("{name}.png")), &d.frame, d.app.theme(), 3);
}

#[test]
#[ignore = "writes files: CAMBER_PICTURES=<dir> cargo test --test pictures -- --ignored"]
fn pictures() {
    let dir = PathBuf::from(std::env::var_os("CAMBER_PICTURES").expect("CAMBER_PICTURES"));
    std::fs::create_dir_all(&dir).unwrap();
    let mut d = Driver::new();
    d.set(Detail::Mid, ViewKind::Chase, false);
    // The title with the CATCH-UP line picked.
    d.app.opts.track = 0;
    d.app.opts.class = Class::Wild;
    d.app.opts.catchup = CatchUp::Light;
    d.app.page = Page::Options;
    d.app.sel = 3;
    d.hold(0, (0, 0));
    save(&d, &dir, "4-title-catch-up");
    d.race(0, Class::Wild, Mode::Race);
    assert_eq!(d.app.race.catchup, CatchUp::Light);
    // The grid, a second before the start.
    while d.app.race.tick < 120 {
        d.hold(0, (0, 0));
    }
    save(&d, &dir, "1-start-grid");
    // A fight in a corner: on lap 2 or later, the road turning, a rival
    // within 8 m ahead and less than 3 m across.
    let mut fight = false;
    let mut calls = 0;
    while !d.app.results() && calls < 30 * 200 {
        d.auto();
        calls += 1;
        let r = &d.app.race;
        let me = &r.ships[0];
        if fight || r.phase != Phase::Run || me.lap < 2 || me.finished() {
            continue;
        }
        let tr = r.tr();
        let k = tr.index(me.s);
        let bend = turn(tr.seg(k).yaw, tr.seg(k + 2).yaw).abs();
        let close = (1..SHIPS).any(|i| {
            let o = &r.ships[i];
            let ds = (o.s - me.s).rem_euclid(tr.len());
            ds > FM * 2 && ds < FM * 8 && (o.x - me.x).abs() < FM * 3 && !o.finished()
        });
        if bend > 900 && close {
            save(&d, &dir, "2-fight-in-a-corner");
            fight = true;
        }
    }
    assert!(fight, "no fight in a corner was found");
    assert!(d.app.results(), "the race did not end");
    for _ in 0..30 {
        d.hold(0, (0, 0));
    }
    assert_eq!(d.app.scene, Scene::Race);
    save(&d, &dir, "3-results");
}

/// What a title picture shows, one line of `shots.txt`.
fn note(d: &Driver, name: &str, out: &mut String) {
    let r = &d.app.race;
    let tv = &d.app.tv;
    let sh = &r.ships[usize::from(tv.ship)];
    writeln!(
        out,
        "{name}: {} {} tick {} shot {:?} ship {} (place {}) cut {} t {} lap {} s {} m cam s {} m x {} m h {} m yaw {}",
        TRACKS[usize::from(r.track)].name,
        r.class.name(),
        r.tick,
        tv.shot,
        tv.ship,
        r.place(tv.ship),
        tv.cuts,
        tv.t,
        sh.lap,
        sh.s / FM,
        d.app.cam.s / FM,
        d.app.cam.x as f32 / FM as f32,
        d.app.cam.h as f32 / FM as f32,
        d.app.cam.yaw,
    )
    .unwrap();
}

#[test]
#[ignore = "writes files: CAMBER_TITLE_PICTURES=<dir> cargo test --test pictures title_pictures -- --ignored"]
fn title_pictures() {
    let dir =
        PathBuf::from(std::env::var_os("CAMBER_TITLE_PICTURES").expect("CAMBER_TITLE_PICTURES"));
    std::fs::create_dir_all(&dir).unwrap();
    let mut out = String::new();
    let shoot = |d: &Driver, name: &str, out: &mut String| {
        save(d, &dir, name);
        note(d, name, out);
    };
    // The main page on each track, at a few moments, each line picked.
    for track in 0..3u8 {
        let mut d = Driver::new();
        d.app.opts.track = track;
        let mut calls = 0;
        for (k, secs) in [(0u8, 2u32), (1, 9), (2, 20), (3, 40), (4, 55), (5, 70)] {
            d.app.sel = k;
            while calls < secs * 30 {
                d.hold(0, (0, 0));
                calls += 1;
            }
            shoot(&d, &format!("main-{track}-{secs:03}s-sel{k}"), &mut out);
        }
    }
    // The OPTIONS page, each line picked, on FURNACE.
    let mut d = Driver::new();
    d.app.opts.track = 1;
    d.tap(game_kit::button::DOWN);
    for _ in 0..3 {
        d.tap(game_kit::button::DOWN);
    }
    d.tap(game_kit::button::A);
    assert_eq!(d.app.page, Page::Options);
    for k in 0..5u8 {
        d.app.sel = k;
        for _ in 0..30 {
            d.hold(0, (0, 0));
        }
        shoot(&d, &format!("options-sel{k}"), &mut out);
    }
    // A whole made-up race on LAGOON: a second into each shot, and every
    // twenty seconds.
    let mut d = Driver::new();
    let mut last_cut = u32::MAX;
    let mut kinds = [0u32; 4];
    let races = d.app.tv.races;
    let mut calls = 0;
    while d.app.tv.races == races && calls < 30 * 60 * 7 {
        d.hold(0, (0, 0));
        calls += 1;
        let tv = d.app.tv;
        let r = &d.app.race;
        if tv.cuts != last_cut && tv.t >= 60 {
            last_cut = tv.cuts;
            kinds[tv.shot as usize] += 1;
            shoot(
                &d,
                &format!("race-{:04}-cut{:02}-{:?}", r.tick / 60, tv.cuts, tv.shot),
                &mut out,
            );
        } else if r.tick % 1200 < 2 && r.tick > 0 {
            shoot(&d, &format!("race-{:04}-every20s", r.tick / 60), &mut out);
        }
        if tv::done(r) {
            shoot(&d, &format!("race-{:04}-end", r.tick / 60), &mut out);
        }
    }
    writeln!(
        out,
        "shots in the race: {kinds:?} (side, far, front, high); calls {calls}"
    )
    .unwrap();
    assert!(
        kinds.iter().all(|&k| k > 0),
        "every kind of shot: {kinds:?}"
    );
    let _ = Shot::Side;
    std::fs::write(dir.join("shots.txt"), out).unwrap();
}

#[test]
#[ignore = "writes files: CAMBER_BACKDROP_PICTURES=<dir> cargo test --test pictures backdrop_pictures -- --ignored"]
fn backdrop_pictures() {
    let dir = PathBuf::from(
        std::env::var_os("CAMBER_BACKDROP_PICTURES").expect("CAMBER_BACKDROP_PICTURES"),
    );
    std::fs::create_dir_all(&dir).unwrap();
    // The SHIP backdrop, an eighth of a turn apart, on each track.
    for track in 0..3u8 {
        let mut d = Driver::new();
        d.app.opts.track = track;
        d.app.opts.backdrop = Backdrop::Ship;
        d.hold(0, (0, 0));
        let mut all = Vec::new();
        for k in 0..8u32 {
            while d.app.race.tick < k * tv::SPIN / 8 + 2 {
                d.hold(0, (0, 0));
            }
            save(&d, &dir, &format!("ship-{track}-{}deg", k * 45));
            all.push(Frame(d.frame.0));
        }
        // All eight in two rows of four, for a look at a glance.
        let path = |r: usize| dir.join(format!("ship-{track}-strip{r}.png"));
        save_strip(&path(0), &all[..4], track, 2);
        save_strip(&path(1), &all[4..], track, 2);
    }
    // The OPTIONS page with BACKDROP picked, by the keys: RACE, then SHIP.
    let mut d = Driver::new();
    d.app.opts.track = 2;
    for _ in 0..4 {
        d.tap(game_kit::button::DOWN);
    }
    d.tap(game_kit::button::A);
    for _ in 0..4 {
        d.tap(game_kit::button::DOWN);
    }
    for _ in 0..60 {
        d.hold(0, (0, 0));
    }
    save(&d, &dir, "options-backdrop-race");
    d.tap(game_kit::button::RIGHT);
    assert_eq!(d.app.opts.backdrop, Backdrop::Ship);
    for _ in 0..60 {
        d.hold(0, (0, 0));
    }
    save(&d, &dir, "options-backdrop-ship");
    // The RACE backdrop on LAGOON, at four moments.
    let mut d = Driver::new();
    let mut calls = 0;
    for secs in [3u32, 15, 30, 45] {
        while calls < secs * 30 {
            d.hold(0, (0, 0));
            calls += 1;
        }
        save(&d, &dir, &format!("race-{secs:02}s"));
    }
}
