//! Tile pictures (ART.BIN): the reader against damaged and hostile
//! files, the fallback to the letter tile, which pictures home keeps, and
//! that the card is never read while the row moves.

mod common;

use board_api::{DirEntry, Files, FsError};
use common::{Ev, LEFT, RIGHT, Sim, art_card, art_file, leak_card};
use console_core::art::{self, ArtError, FILE_MAX, HEADER, Picture};
use console_core::theme::{ART_FIRST, ART_SLOTS, TILE_LEFT, TILE_Y};

const PATH: &str = "/CONSOLE/GAMES/G00/ART.BIN";

/// A card of one file, with ways to misbehave.
struct OneFile {
    bytes: Vec<u8>,
    /// `size` says this instead of the real length.
    claim: Option<u32>,
    /// Each read gives at most this many bytes (a board may, though the
    /// trait says it reads all it can).
    drip: usize,
    /// Reads fail from this call on.
    fail_from: Option<u32>,
    calls: u32,
    bytes_read: usize,
}

impl OneFile {
    fn new(bytes: Vec<u8>) -> Self {
        OneFile {
            bytes,
            claim: None,
            drip: usize::MAX,
            fail_from: None,
            calls: 0,
            bytes_read: 0,
        }
    }
}

impl Files for OneFile {
    fn list<F: FnMut(&DirEntry<'_>) -> bool>(&mut self, _: &str, _: F) -> Result<(), FsError> {
        Ok(())
    }

    fn size(&mut self, path: &str) -> Result<u32, FsError> {
        self.calls += 1;
        if path != PATH {
            return Err(FsError::NotFound);
        }
        Ok(self.claim.unwrap_or(self.bytes.len() as u32))
    }

    fn read_at(&mut self, _: &str, offset: u32, buf: &mut [u8]) -> Result<usize, FsError> {
        self.calls += 1;
        if self.fail_from.is_some_and(|n| self.calls >= n) {
            return Err(FsError::Io);
        }
        let start = (offset as usize).min(self.bytes.len());
        let n = buf.len().min(self.bytes.len() - start).min(self.drip);
        buf[..n].copy_from_slice(&self.bytes[start..start + n]);
        self.bytes_read += n;
        Ok(n)
    }
}

fn read(f: &mut OneFile) -> Result<(), ArtError> {
    let mut pic = Picture::new();
    art::read(f, PATH, &mut pic)
}

/// A small random number maker (xorshift): the same numbers every run.
fn next(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

#[test]
fn a_good_file_reads_and_every_cut_one_is_refused() {
    let good = art_file(0x1000, 0);
    assert_eq!(good.len(), FILE_MAX);
    let mut f = OneFile::new(good.clone());
    assert_eq!(read(&mut f), Ok(()));
    assert!(f.calls <= 4, "size and three reads, not {}", f.calls);
    for len in 0..good.len() {
        let mut f = OneFile::new(good[..len].to_vec());
        let got = read(&mut f);
        let want = if len < HEADER {
            ArtError::Short
        } else {
            ArtError::Length
        };
        assert_eq!(got, Err(want), "cut to {len} bytes");
        assert!(f.bytes_read <= HEADER, "{len}: read past the header");
    }
    // One byte too many is refused too.
    let mut long = good.clone();
    long.push(0);
    assert_eq!(read(&mut OneFile::new(long)), Err(ArtError::Length));
    assert_eq!(read(&mut OneFile::new(Vec::new())), Err(ArtError::Short));
}

#[test]
fn wrong_headers_are_refused() {
    let good = art_file(0x1000, 0);
    let cases: [(usize, u8, ArtError); 9] = [
        (0, b'a', ArtError::NotArt),
        (2, b'X', ArtError::NotArt),
        (3, 0, ArtError::Version),
        (3, 2, ArtError::Version),
        (4, 48, ArtError::Size),
        (5, 18, ArtError::Size),
        (6, 0, ArtError::Colours),
        (6, 200, ArtError::Colours),
        (7, 8, ArtError::Depth),
    ];
    for (at, v, want) in cases {
        let mut b = good.clone();
        b[at] = v;
        let mut f = OneFile::new(b);
        assert_eq!(read(&mut f), Err(want), "byte {at} = {v}");
        assert!(f.bytes_read <= HEADER, "byte {at}: read past the header");
    }
    // Fewer colours than the file holds: the length no longer matches.
    let mut b = good.clone();
    b[6] = 8;
    assert_eq!(read(&mut OneFile::new(b)), Err(ArtError::Length));
    // A pixel past the colours (8 colours, 8 + 16 + 648 bytes).
    let mut b = good[..HEADER].to_vec();
    b[6] = 8;
    b.extend_from_slice(&good[HEADER..HEADER + 16]);
    b.extend(std::iter::repeat_n(0x77, 648));
    assert_eq!(read(&mut OneFile::new(b.clone())), Ok(()));
    let last = b.len() - 1;
    b[last] = 0x78;
    assert_eq!(read(&mut OneFile::new(b)), Err(ArtError::Pixel));
}

#[test]
fn a_lying_card_cannot_make_it_read_more_or_hang() {
    let good = art_file(0x1000, 0);
    // The size says 4 GB: refused after the header.
    let mut f = OneFile::new(good.clone());
    f.claim = Some(u32::MAX);
    assert_eq!(read(&mut f), Err(ArtError::Length));
    assert!(f.bytes_read <= HEADER);
    // The size says 688, the file is shorter: the read stops at its end.
    for len in [0, 7, 8, 40, 300, 687] {
        let mut f = OneFile::new(good[..len].to_vec());
        f.claim = Some(688);
        assert!(read(&mut f).is_err(), "{len}");
        assert!(f.calls <= 5, "{len}: {} calls", f.calls);
    }
    // One byte per read: still read, in at most one call per byte.
    let mut f = OneFile::new(good.clone());
    f.drip = 1;
    assert_eq!(read(&mut f), Ok(()));
    assert!(f.calls as usize <= FILE_MAX + 1);
    // A read error at any point.
    for from in 1..=4 {
        let mut f = OneFile::new(good.clone());
        f.fail_from = Some(from);
        let got = read(&mut f);
        assert!(got.is_err(), "fail from call {from}");
    }
}

#[test]
fn random_bytes_never_crash_and_are_refused() {
    let good = art_file(0x1000, 0);
    let mut rng = 0x1234_5678;
    for i in 0..3000 {
        let len = (next(&mut rng) % 1000) as usize;
        let mut b: Vec<u8> = (0..len).map(|_| next(&mut rng) as u8).collect();
        // Every third one with a good header, to get past the first checks.
        if i % 3 == 0 && b.len() >= HEADER {
            b[..HEADER].copy_from_slice(&good[..HEADER]);
        }
        let mut f = OneFile::new(b.clone());
        let got = read(&mut f);
        if b.len() != FILE_MAX || b[..HEADER] != good[..HEADER] {
            assert!(got.is_err(), "run {i}: {} bytes", b.len());
        }
        assert!(f.bytes_read <= FILE_MAX);
    }
}

// ---- On the home screen ---------------------------------------------------

/// The home screen after boot with this card, settled.
fn home(card: &'static [(&'static str, &'static [u8])]) -> Sim {
    let mut sim = Sim::new(Some(card));
    sim.boot();
    sim.idle(10);
    sim
}

fn print(sim: &Sim) -> u64 {
    sim.ticks.last().unwrap().print
}

/// One game, G00, with `art` as its ART.BIN (or none).
fn one_game(art: Option<Vec<u8>>) -> &'static [(&'static str, &'static [u8])] {
    let mut files = vec![("/CONSOLE/GAMES/G00/GAME.BIN".to_string(), b"GAME".to_vec())];
    if let Some(a) = art {
        files.push((PATH.to_string(), a));
    }
    leak_card(files)
}

#[test]
fn a_good_picture_is_drawn_on_its_tile() {
    let art = art_file(0x2000, 0);
    let sim = home(one_game(Some(art.clone())));
    let mut pic = Picture::new();
    art::parse(&art, &mut pic).unwrap();
    let k = sim.console.home().art().slot(0).expect("kept");
    // Every pixel of the tile, as the panel shows it (2 x 2 per pixel).
    for y in 0..36u16 {
        for x in 0..36u16 {
            let want = pic.colours[usize::from(pic.at(x.into(), y.into()))];
            let (px, py) = ((TILE_LEFT as u16 + x) * 2, (TILE_Y as u16 + y) * 2);
            assert_eq!(sim.board.screen.pixel(px, py), want, "({x}, {y})");
        }
    }
    let first = usize::from(ART_FIRST) + k * 16;
    assert_eq!(
        sim.console.video().palette().colours()[first..first + 16],
        pic.colours
    );
    let none = home(one_game(None));
    assert_ne!(print(&sim), print(&none), "not the letter tile");
}

#[test]
fn a_bad_picture_shows_the_letter_tile() {
    let letter = print(&home(one_game(None)));
    let good = art_file(0x2000, 0);
    let mut bad: Vec<Vec<u8>> = Vec::new();
    // Cuts (every 23rd length), a byte too many, header faults.
    bad.extend((0..good.len()).step_by(23).map(|n| good[..n].to_vec()));
    bad.push([good.clone(), vec![0]].concat());
    for (at, v) in [(0, b'x'), (3, 2), (4, 37), (5, 35), (6, 0), (6, 17), (7, 2)] {
        let mut b = good.clone();
        b[at] = v;
        bad.push(b);
    }
    // Random bytes, some exactly the right length.
    let mut rng = 99;
    for i in 0..20 {
        let len = if i % 2 == 0 {
            688
        } else {
            (next(&mut rng) % 900) as usize
        };
        bad.push((0..len).map(|_| next(&mut rng) as u8).collect());
    }
    for (i, b) in bad.into_iter().enumerate() {
        let sim = home(one_game(Some(b)));
        assert_eq!(print(&sim), letter, "bad file {i}");
        assert!(sim.console.home().art().has_none(0), "bad file {i}");
    }
}

#[test]
fn a_bad_picture_is_tried_once_until_the_list_is_read_again() {
    let mut sim = home(one_game(Some(vec![b'A'; 688])));
    let calls = sim.board.files.calls;
    for _ in 0..3 {
        sim.tap_button(RIGHT);
        sim.tap_button(LEFT);
    }
    sim.idle(30);
    assert_eq!(sim.board.files.calls, calls, "not tried again");
    // One log line says why (for the game's maker); only once.
    let refused: Vec<&String> = (sim.board.log.all.iter())
        .filter(|l| l.starts_with("home: picture refused"))
        .collect();
    assert_eq!(refused, ["home: picture refused: G00 (not ART)"]);
    // No ART.BIN is normal: no log line.
    let none = home(one_game(None));
    assert!(none.board.log.all.iter().all(|l| !l.contains("picture")));
    // A cut file says so.
    let cut = home(one_game(Some(art_file(0x2000, 0)[..100].to_vec())));
    assert!(
        cut.board
            .log
            .all
            .iter()
            .any(|l| l == "home: picture refused: G00 (length)"),
        "{:?}",
        cut.board.log.all
    );
}

/// Picture pixel x of a finger on the row, for a drag.
const ROW: u16 = 40;

#[test]
fn scrolling_does_not_read_the_card() {
    let mut sim = home(art_card(14, &[]));
    let home_of = |sim: &Sim| sim.console.home().still();
    assert!(home_of(&sim));
    let at_rest = sim.board.files.calls;
    assert!(at_rest > 0);
    // A drag that goes on for 3 s (past a 2 s card check): the finger
    // goes left and right on the row.
    let mut x = 150;
    let mut left = true;
    sim.input(Ev::Touch(x, ROW));
    sim.step_tick();
    for _ in 0..90 {
        if x <= 20 {
            left = false;
        }
        if x >= 150 {
            left = true;
        }
        x = if left { x - 5 } else { x + 5 };
        sim.input(Ev::Touch(x, ROW));
        sim.step_tick();
        assert_eq!(sim.board.files.calls, at_rest, "read while dragging");
    }
    // A flick: the row glides. Nothing is read until it is still.
    for x in [140, 120, 100, 80] {
        sim.input(Ev::Touch(x, ROW));
        sim.step_tick();
    }
    sim.input(Ev::Lift);
    let mut glide = 0;
    loop {
        sim.step_tick();
        if home_of(&sim) {
            break;
        }
        assert_eq!(sim.board.files.calls, at_rest, "read while gliding");
        glide += 1;
        assert!(glide < 60);
    }
    assert!(glide > 2, "it glided");
    // Still: the pictures now on screen were read in that step.
    assert!(sim.board.files.calls > at_rest);
    let (want, n) = sim.console.home().art_wanted();
    for &g in &want[..n] {
        assert!(sim.console.home().art().slot(g).is_some(), "game {g}");
    }
    // Buttons: a step that reads the card is one where the row was still
    // before it, or is still after it (never in the middle of a scroll).
    let mut was_still = true;
    for i in 0..40 {
        let before = sim.board.files.calls;
        let b = if i < 25 { RIGHT } else { LEFT };
        sim.input(Ev::Press(b));
        sim.step_tick();
        sim.input(Ev::Release(b));
        let still = home_of(&sim);
        if sim.board.files.calls != before {
            assert!(was_still || still, "press {i}: read while scrolling");
        }
        was_still = still;
        sim.step_tick();
        if sim.board.files.calls != before {
            assert!(was_still || home_of(&sim));
        }
        was_still = home_of(&sim);
    }
}

#[test]
fn a_long_list_keeps_the_pictures_on_screen_and_the_next_one() {
    let mut sim = home(art_card(14, &[3]));
    let h = sim.console.home();
    // At the start: tiles 0 to 3 show (3 in part), 4 is the next one.
    let (want, n) = h.art_wanted();
    assert_eq!(&want[..n], &[0, 1, 2, 3, 4]);
    for g in [0, 1, 2, 4] {
        assert!(h.art().slot(g).is_some(), "{g}");
    }
    assert!(h.art().has_none(3), "G03 has no ART.BIN");
    assert!(h.art().slot(5).is_none());
    // To the far end with the buttons; the pictures follow.
    for _ in 0..13 {
        sim.tap_button(RIGHT);
    }
    sim.idle(20);
    let h = sim.console.home();
    assert_eq!(h.selected(), 13);
    let (want, n) = h.art_wanted();
    assert!(n <= ART_SLOTS);
    assert!(want[..n].contains(&13) && want[..n].contains(&10));
    for &g in &want[..n] {
        assert!(h.art().slot(g).is_some(), "{g}");
    }
    assert!(h.art().slot(0).is_none(), "an old one was let go");
    // And back: read again.
    for _ in 0..13 {
        sim.tap_button(LEFT);
    }
    sim.idle(20);
    assert!(sim.console.home().art().slot(0).is_some());
}

#[test]
fn an_idle_home_with_pictures_sleeps_and_sends_nothing() {
    let mut sim = home(art_card(5, &[1]));
    let calls = sim.board.files.calls;
    sim.idle(30);
    let steps: u32 = sim.ticks[sim.ticks.len() - 30..]
        .iter()
        .map(|t| t.steps)
        .sum();
    let pixels: u32 = sim.ticks[sim.ticks.len() - 30..]
        .iter()
        .map(|t| t.pixels)
        .sum();
    assert!(steps <= 1, "{steps} steps in 1 s");
    assert_eq!(pixels, 0);
    assert_eq!(sim.board.files.calls, calls);
}
