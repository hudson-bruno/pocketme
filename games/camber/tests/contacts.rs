//! The rivals' contacts: hard wall hits ([`Ev::Wall`]) and the times two
//! rivals ran into each other's bump box, in races of the rivals alone
//! (the player's ship held out at the grid, the catch-up OFF) on every
//! track and class, seeds 0 to 6 (every grid order).
//!
//! `contacts_table` prints the table. The same file, run on the rivals
//! before the rebuild (2026-11-22), gave the `BEFORE` numbers below; it
//! uses only what both versions of the game have.

use camber::race::{Class, Ev, Mode, Race, SHIPS};
use camber::track::{FM, TRACKS};

/// The bump box (`race.rs`): 4.2 m along the road, 2.4 m across, 2 m high.
const BOX_S: i32 = FM * 42 / 10;
const BOX_X: i32 = FM * 24 / 10;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Count {
    /// Hard wall hits of the rivals.
    walls: u32,
    /// Entries of two rivals into each other's bump box.
    rr: u32,
    /// The slowest rival's finish (ticks).
    last: u32,
}

/// The distance from `a` to `b` along a lap of `len`, the short way.
fn gap(a: i32, b: i32, len: i32) -> i32 {
    let d = (b - a).rem_euclid(len);
    if d > len / 2 { d - len } else { d }
}

fn count(track: u8, class: Class, seed: u32) -> Count {
    let mut r = Race::new();
    r.setup(track, class, Mode::Race, seed);
    let len = TRACKS[usize::from(track)].len();
    let mut c = Count::default();
    let mut was = [[false; SHIPS]; SHIPS];
    for _ in 0..60 * 400 {
        {
            let p = &mut r.ships[0];
            p.fall = 1;
            p.vf = 0;
            p.vl = 0;
            p.vh = 0;
        }
        r.step(None);
        while let Some(e) = r.take_ev() {
            if let Ev::Wall { ship } = e
                && ship > 0
            {
                c.walls += 1;
            }
        }
        for a in 1..SHIPS {
            for (b, was) in was[a].iter_mut().enumerate().skip(a + 1) {
                let (sa, sb) = (&r.ships[a], &r.ships[b]);
                let on = !sa.finished()
                    && !sb.finished()
                    && sa.fall == 0
                    && sb.fall == 0
                    && gap(sa.s, sb.s, len).abs() < BOX_S
                    && (sa.x - sb.x).abs() < BOX_X
                    && (sa.h - sb.h).abs() <= FM * 2;
                if on && !*was {
                    c.rr += 1;
                }
                *was = on;
            }
        }
        if (1..SHIPS).all(|i| r.ships[i].finished()) {
            c.last = (1..SHIPS).map(|i| r.ships[i].finish).max().unwrap_or(0);
            return c;
        }
    }
    panic!("track {track} {class:?} seed {seed}: the rivals did not all finish");
}

/// Sums over seeds 0 to 6, per class (rows) and track (columns).
fn table() -> [[Count; 3]; 3] {
    let mut t = [[Count::default(); 3]; 3];
    for (ci, class) in Class::ALL.into_iter().enumerate() {
        for track in 0..TRACKS.len() as u8 {
            let cell = &mut t[ci][usize::from(track)];
            for seed in 0..7 {
                let c = count(track, class, seed);
                cell.walls += c.walls;
                cell.rr += c.rr;
                cell.last = cell.last.max(c.last);
            }
        }
    }
    t
}

/// The rivals before the rebuild, measured with this file on the PC
/// (2026-11-22): walls and rival contacts, seven races a cell, tracks
/// LAGOON, FURNACE, GLACIER.
const BEFORE: [[(u32, u32); 3]; 3] = [
    [(82, 14), (21, 5), (13, 39)],
    [(386, 10), (278, 4), (345, 10)],
    [(629, 9), (565, 10), (540, 11)],
];

#[test]
fn contacts_table() {
    let t = table();
    eprintln!("class  track    walls  rr  (seven races, rivals alone, catch-up OFF)");
    for (ci, class) in Class::ALL.into_iter().enumerate() {
        for (k, tr) in TRACKS.iter().enumerate() {
            let c = t[ci][k];
            eprintln!(
                "{:<6} {:<8} {:>5} {:>3}",
                class.name(),
                tr.name,
                c.walls,
                c.rr
            );
        }
    }
}

#[test]
fn the_rivals_hit_walls_and_each_other_less_than_before() {
    let t = table();
    let mut msg = String::new();
    let (mut w_now, mut w_before, mut rr_now, mut rr_before) = (0, 0, 0, 0);
    for (ci, class) in Class::ALL.into_iter().enumerate() {
        let walls = |row: &[u32; 3]| row.iter().sum::<u32>();
        let now = walls(&t[ci].map(|c| c.walls));
        let before = walls(&BEFORE[ci].map(|c| c.0));
        assert!(
            now < before,
            "{}: walls {now}, before {before}",
            class.name()
        );
        for (k, tr) in TRACKS.iter().enumerate() {
            let c = t[ci][k];
            let (bw, brr) = BEFORE[ci][k];
            msg += &format!(
                "{} {}: walls {} (before {bw}), rr {} (before {brr})\n",
                class.name(),
                tr.name,
                c.walls,
                c.rr
            );
            w_now += c.walls;
            w_before += bw;
            rr_now += c.rr;
            rr_before += brr;
        }
    }
    eprint!("{msg}");
    assert!(w_now < w_before, "walls {w_now}, before {w_before}\n{msg}");
    assert!(
        rr_now <= rr_before,
        "rival contacts {rr_now}, before {rr_before}\n{msg}"
    );
}
