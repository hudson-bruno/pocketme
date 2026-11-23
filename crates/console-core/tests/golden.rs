//! Golden fingerprints: scripted tours of the console, with the
//! picture's fingerprint saved at fixed ticks. The stored lists are in
//! `tests/data/golden.txt` (the FOUR card: letter tiles only) and
//! `tests/data/golden_art.txt` (a card with tile pictures: good ones,
//! a broken one and a game with none, more games than pictures kept).
//!
//! When the look changes on purpose, write the new list with:
//!
//! ```text
//! UPDATE_GOLDEN=1 cargo test --test golden
//! ```
//!
//! and look at the new pictures (the simulator's `--shot`) before keeping it.
//!
//! With the feature `boot-gather`, the two stored lists are only read:
//! the tours run with the boot animation OFF and must match them from
//! home on. The animation's own pictures are in
//! `tests/data/golden_gather.txt` (`UPDATE_GOLDEN=1 cargo test --features
//! boot-gather --test golden` writes only that one).

mod common;

use common::{Ev, FOUR, LEFT, RIGHT, SOUTH, Sim, art_file, leak_card};

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/golden.txt");
const GOLDEN_ART: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/golden_art.txt");

/// The tour: events at tick numbers, and the ticks whose fingerprint is
/// kept (with a name for each).
const SCRIPT: &[(u64, Ev)] = &[
    (60, Ev::Press(RIGHT)),
    (61, Ev::Release(RIGHT)),
    (75, Ev::Press(RIGHT)),
    (76, Ev::Release(RIGHT)),
    (90, Ev::Press(SOUTH)),
    (91, Ev::Release(SOUTH)),
    (200, Ev::Pad(false)),
    (300, Ev::Pad(true)),
    (400, Ev::Press(LEFT)),
    (401, Ev::Release(LEFT)),
    (420, Ev::Touch(80, 40)),
    (421, Ev::Lift),
];

const SHOTS: &[(u64, &str)] = &[
    (1, "boot-start"),
    (12, "boot-middle"),
    (40, "boot-end"),
    (55, "home"),
    (63, "moving"),
    (88, "moved-twice"),
    (96, "notice-sliding"),
    (110, "notice-shown"),
    (205, "pad-off"),
    (305, "pad-on"),
    (410, "moved-back"),
    (430, "tapped"),
    (600, "quiet"),
];

/// The tile picture tour: 8 games, more than the 6 pictures home keeps.
/// G00 to G07 have a picture each, except G02 (no ART.BIN: the letter
/// tile) and G04 (an ART.BIN cut short: refused, the letter tile).
/// Home sorts by name: BROKEN PICTURE (G04), NO PICTURE (G02), then
/// PICTURE 0, 1, 3, 5, 6, 7. Right to the end of the row, a drag back,
/// a tap. The simulator gives the same pictures for the same card and
/// script (its last fingerprint is the same as `tapped`).
const ART_SCRIPT: &[(u64, Ev)] = &[
    (60, Ev::Press(RIGHT)),
    (61, Ev::Release(RIGHT)),
    (80, Ev::Press(RIGHT)),
    (81, Ev::Release(RIGHT)),
    (100, Ev::Press(RIGHT)),
    (101, Ev::Release(RIGHT)),
    (120, Ev::Press(RIGHT)),
    (121, Ev::Release(RIGHT)),
    (140, Ev::Press(RIGHT)),
    (141, Ev::Release(RIGHT)),
    (160, Ev::Press(RIGHT)),
    (161, Ev::Release(RIGHT)),
    (180, Ev::Press(RIGHT)),
    (181, Ev::Release(RIGHT)),
    (230, Ev::Touch(30, 40)),
    (231, Ev::Touch(50, 40)),
    (232, Ev::Touch(70, 40)),
    (233, Ev::Touch(90, 40)),
    (240, Ev::Lift),
    (300, Ev::Touch(36, 40)),
    (301, Ev::Lift),
];

const ART_SHOTS: &[(u64, &str)] = &[
    (55, "home-broken-and-none-first"),
    (78, "on-no-picture"),
    (98, "on-picture-0"),
    (138, "on-picture-3"),
    (198, "row-end-picture-7"),
    (232, "dragging"),
    (290, "dragged-and-settled"),
    (330, "tapped"),
];

/// The tile picture tour's card.
fn art_card() -> &'static [(&'static str, &'static [u8])] {
    let mut files = Vec::new();
    for i in 0..8usize {
        let dir = format!("/CONSOLE/GAMES/G{i:02}");
        files.push((format!("{dir}/GAME.BIN"), b"GAME".to_vec()));
        let name = match i {
            2 => "NO PICTURE".to_string(),
            4 => "BROKEN PICTURE".to_string(),
            _ => format!("PICTURE {i}"),
        };
        files.push((
            format!("{dir}/INFO.TXT"),
            format!("NAME={name}\nTAG=G{i:02}\n").into_bytes(),
        ));
        match i {
            2 => {}
            4 => files.push((
                format!("{dir}/ART.BIN"),
                art_file(0x2104, 4)[..300].to_vec(),
            )),
            _ => files.push((
                format!("{dir}/ART.BIN"),
                art_file(0x0841 * i as u16 + 0x1000, i),
            )),
        }
    }
    leak_card(files)
}

/// Runs a tour and returns "name fingerprint" lines.
fn run_tour(
    card: &'static [(&'static str, &'static [u8])],
    script: &[(u64, Ev)],
    shots: &[(u64, &str)],
) -> Vec<String> {
    let mut sim = Sim::new(Some(card));
    // With the boot animation built in, the tours run with it OFF: then
    // every picture from home on must be the stored one (see
    // `fingerprints_match_the_stored_ones`).
    #[cfg(feature = "boot-gather")]
    sim.console.set_boot_anim(console_core::boot::Anim::Off);
    let mut out = Vec::new();
    for &(at, name) in shots {
        sim.run_until(at, script);
        out.push(format!("{name} {:016x}", sim.ticks.last().unwrap().print));
    }
    out
}

fn tour() -> Vec<String> {
    run_tour(FOUR, SCRIPT, SHOTS)
}

fn art_tour() -> Vec<String> {
    run_tour(art_card(), ART_SCRIPT, ART_SHOTS)
}

#[test]
fn the_same_fingerprints_every_run() {
    assert_eq!(tour(), tour());
    assert_eq!(art_tour(), art_tour());
}

/// Compares with the stored list, or writes it with UPDATE_GOLDEN.
fn matches(now: Vec<String>, file: &str) {
    let now = now.join("\n") + "\n";
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(file, &now).unwrap();
        eprintln!("wrote {file}");
        return;
    }
    let stored =
        std::fs::read_to_string(file).expect("no golden file: run with UPDATE_GOLDEN=1 once");
    assert_eq!(
        now, stored,
        "the pictures changed; if on purpose, run with UPDATE_GOLDEN=1"
    );
}

#[cfg(not(feature = "boot-gather"))]
#[test]
fn fingerprints_match_the_stored_ones() {
    matches(tour(), GOLDEN);
}

#[cfg(not(feature = "boot-gather"))]
#[test]
fn tile_picture_fingerprints_match_the_stored_ones() {
    matches(art_tour(), GOLDEN_ART);
}

/// With the boot animation built in and set to OFF, home and everything
/// after it look exactly as without the feature: every stored picture
/// from "home" on is the same. (The boot pictures differ: OFF has none.)
/// The stored lists are never written from here.
#[cfg(feature = "boot-gather")]
#[test]
fn fingerprints_match_the_stored_ones() {
    for (now, file, first) in [(tour(), GOLDEN, "home "), (art_tour(), GOLDEN_ART, "")] {
        let stored = std::fs::read_to_string(file).expect("no golden file");
        let from = |v: Vec<&str>| -> Vec<String> {
            let i = v.iter().position(|l| l.starts_with(first)).unwrap();
            v[i..].iter().map(|l| l.to_string()).collect()
        };
        assert_eq!(
            from(now.iter().map(String::as_str).collect()),
            from(stored.lines().collect()),
            "{file}: home differs with the boot animation built in"
        );
    }
}

/// The boot animation's own tours (feature `boot-gather`): power on to
/// home with FULL and with SHORT, in `tests/data/golden_gather.txt`.
#[cfg(feature = "boot-gather")]
mod gather {
    use super::*;
    use console_core::boot::Anim;

    const GOLDEN_GATHER: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/golden_gather.txt");

    /// Ticks (of 1/30 s) to keep, by name; the boot runs to its end.
    const FULL_SHOTS: &[(u64, &str)] = &[
        (1, "full-start"),
        (10, "full-sphere"),
        (25, "full-writing"),
        (45, "full-name"),
        (57, "full-gleam"),
        (68, "full-shrink"),
        (80, "full-last"),
        (90, "full-home"),
    ];
    const SHORT_SHOTS: &[(u64, &str)] = &[
        (1, "short-start"),
        (6, "short-gleam"),
        (16, "short-shrink"),
        (26, "short-last"),
        (35, "short-home"),
    ];

    fn tours() -> Vec<String> {
        let mut out = Vec::new();
        for (anim, shots) in [(Anim::Full, FULL_SHOTS), (Anim::Short, SHORT_SHOTS)] {
            let mut sim = Sim::new(Some(FOUR));
            sim.console.set_boot_anim(anim);
            for &(at, name) in shots {
                sim.run_until(at, &[]);
                out.push(format!("{name} {:016x}", sim.ticks.last().unwrap().print));
            }
        }
        out
    }

    #[test]
    fn the_same_boot_pictures_every_run() {
        assert_eq!(tours(), tours());
    }

    /// FULL and SHORT end on the same picture, and the home they hand
    /// over to is exactly the stored home without the feature.
    #[test]
    fn both_boots_end_on_the_home_stored_without_the_feature() {
        let t = tours();
        let print = |name: &str| -> String {
            let l = t.iter().find(|l| l.starts_with(name)).unwrap();
            l.split(' ').nth(1).unwrap().to_string()
        };
        assert_eq!(print("full-last "), print("short-last "));
        let stored = std::fs::read_to_string(GOLDEN).expect("no golden file");
        let home = stored.lines().find(|l| l.starts_with("home ")).unwrap();
        assert_eq!(print("full-home "), home[5..]);
        assert_eq!(print("short-home "), home[5..]);
    }

    #[test]
    fn boot_fingerprints_match_the_stored_ones() {
        matches(tours(), GOLDEN_GATHER);
    }
}
