//! The second reader's checks (core-check round): cases that broke, or
//! could break, the console. Each test names the fault it guards.
//! No window, the same result on every run.

mod common;

use board_api::fake::FakeBoard;
use board_api::{DirEntry, Files, FsError, Stick, Wake, btn};
use common::*;
use console_core::home::{Home, LENT_BYTES, MAX_GAMES};
use console_core::{Console, FRAME_US, Page};

// ---- A card made on the fly -------------------------------------------------

/// A card of `n` game folders named G00, G01, ... Each has a GAME.BIN and
/// an INFO.TXT with the name `names(i)`. `ignore_stop`: the card keeps
/// listing after `each` said stop (a careless board, or a card driver that
/// cannot stop a listing early, as embedded-sdmmc's `iterate_dir`).
struct ManyCard {
    n: usize,
    ignore_stop: bool,
    reverse: bool,
    names: fn(usize) -> String,
}

impl ManyCard {
    fn folder(&self, i: usize) -> String {
        format!("G{i:02}")
    }
}

impl Files for ManyCard {
    fn list<F: FnMut(&DirEntry<'_>) -> bool>(
        &mut self,
        dir: &str,
        mut each: F,
    ) -> Result<(), FsError> {
        if dir != "/CONSOLE/GAMES" {
            return Ok(());
        }
        let order: Vec<usize> = if self.reverse {
            (0..self.n).rev().collect()
        } else {
            (0..self.n).collect()
        };
        for i in order {
            let name = self.folder(i);
            let go = each(&DirEntry {
                name: &name,
                size: 0,
                is_dir: true,
            });
            if !go && !self.ignore_stop {
                break;
            }
        }
        Ok(())
    }

    fn size(&mut self, path: &str) -> Result<u32, FsError> {
        if path.ends_with("/GAME.BIN") {
            Ok(4)
        } else {
            Err(FsError::NotFound)
        }
    }

    fn read_at(&mut self, path: &str, offset: u32, buf: &mut [u8]) -> Result<usize, FsError> {
        let folder = path
            .strip_prefix("/CONSOLE/GAMES/G")
            .and_then(|r| r.strip_suffix("/INFO.TXT"))
            .ok_or(FsError::NotFound)?;
        let i: usize = folder.parse().map_err(|_| FsError::NotFound)?;
        let text = format!("NAME={}\nTAG=T{i}\n", (self.names)(i));
        let b = text.as_bytes();
        let start = (offset as usize).min(b.len());
        let n = buf.len().min(b.len() - start);
        buf[..n].copy_from_slice(&b[start..start + n]);
        Ok(n)
    }
}

/// Fault 1 (found in this round): a board that keeps listing after the
/// console said "stop" made `Home::scan` write past its 32-game list and
/// panic (on the chip: a restart). The console must protect itself.
#[test]
fn a_card_that_lists_more_than_asked_does_not_crash() {
    let mut card = ManyCard {
        n: 40,
        ignore_stop: true,
        reverse: false,
        names: |i| format!("GAME {i:02}"),
    };
    let mut home = Box::new(Home::new());
    let mut ram = vec![0u8; LENT_BYTES];
    home.scan(&mut card, Some(&mut ram), 0);
    assert_eq!(home.count(), MAX_GAMES);
}

/// Forty games on a card that stops when asked: 32 are listed, the rest
/// are left out, and the last one can be reached and is drawn in view.
#[test]
fn forty_games_show_thirty_two_and_the_last_is_reachable() {
    let mut card = ManyCard {
        n: 40,
        ignore_stop: false,
        reverse: false,
        names: |i| format!("GAME {i:02}"),
    };
    let mut home = Box::new(Home::new());
    let mut ram = vec![0u8; LENT_BYTES];
    home.scan(&mut card, Some(&mut ram), 0);
    assert_eq!(home.count(), MAX_GAMES);
    assert_eq!(home.list(Some(&ram))[31].name.as_str(), "GAME 31");
}

/// Fault 2 (found in this round): two games with the same NAME were
/// shown in the order the card lists them, and games with no TILE got
/// their default colour by card order. So the PC folder (sorted) and a
/// real card (order of writing) could show different rows and colours.
/// Now the folder name breaks the tie, and default colours follow the
/// sorted list.
#[test]
fn equal_names_keep_one_order_whatever_the_card_order() {
    let names: fn(usize) -> String = |_| "SAME".into();
    let mut a = ManyCard {
        n: 5,
        ignore_stop: false,
        reverse: false,
        names,
    };
    let mut b = ManyCard {
        n: 5,
        ignore_stop: false,
        reverse: true,
        names,
    };
    let (mut ha, mut hb) = (Box::new(Home::new()), Box::new(Home::new()));
    let (mut ra, mut rb) = (vec![0u8; LENT_BYTES], vec![0xFFu8; LENT_BYTES]);
    ha.scan(&mut a, Some(&mut ra), 0);
    hb.scan(&mut b, Some(&mut rb), 0);
    // The order, and the default tile colours (no TILE in these
    // INFO.TXT files), are the same whatever order the card lists in.
    let rows = |h: &Home, ram: &[u8]| -> Vec<(String, u16)> {
        h.list(Some(ram))
            .iter()
            .map(|g| (g.folder.as_str().to_string(), g.tile))
            .collect()
    };
    assert_eq!(rows(&ha, &ra), rows(&hb, &rb));
    assert_eq!(rows(&ha, &ra)[0].0, "G00");
}

/// Long and odd names: cut to their fixed size, letters the font lacks
/// drawn as '?', and nothing panics while drawing them.
#[test]
fn long_and_odd_names_are_cut_and_drawn() {
    let mut card = ManyCard {
        n: 3,
        ignore_stop: false,
        reverse: false,
        names: |i| match i {
            0 => "A NAME FAR TOO LONG FOR ANY SCREEN AT ALL".into(),
            1 => "caf\u{e9} \u{1F600} tab\there".into(),
            _ => "=".repeat(300),
        },
    };
    let mut home = Box::new(Home::new());
    let mut ram = vec![0u8; LENT_BYTES];
    home.scan(&mut card, Some(&mut ram), 0);
    assert_eq!(home.count(), 3);
    for g in home.list(Some(&ram)).iter() {
        assert!(g.name.as_str().len() <= console_core::home::NAME_LEN);
        assert!(g.name.as_str().is_ascii());
    }
    let mut f = Box::new(console_core::console_video::Frame::new());
    home.draw(&mut f, Some(&ram));
}

// ---- Timing -----------------------------------------------------------------

/// Fault 3 (found in this round): every step woken early by input pushed
/// the next frame time one more frame into the future. A board that wakes
/// the console on every small input change (a noisy stick, a resting
/// finger on the touch screen) could push it seconds ahead; the next
/// animation then froze, and input was ignored until that time.
#[test]
fn many_early_input_wakes_do_not_push_the_next_frame_away() {
    let mut sim = Sim::new(Some(NINE));
    sim.boot();
    sim.idle(10);
    // Select tile 2: the next RIGHT then has to scroll the row.
    sim.tap_button(RIGHT);
    sim.tap_button(RIGHT);
    sim.idle(20);
    assert_eq!(sim.console.home().selected(), 2);
    assert_eq!(sim.console.home().scroll(), 0);
    // Idle home: the console waits for input.
    let mut now = sim.board.clock.now_us;
    // 100 input wakes, 1 ms apart: a stick wobbling inside its dead zone.
    for i in 0..100 {
        now += 1_000;
        sim.board.clock.now_us = now;
        sim.board.pad.state.left = Stick {
            x: (i % 7) as i16 * 100,
            y: 0,
        };
        let w = sim.console.step(&mut *sim.board);
        assert!(w.on_input(), "idle home waits for input");
    }
    // Now a real press: the row starts to scroll. The next frame must
    // come within one frame time, not seconds later.
    now += 1_000;
    sim.board.clock.now_us = now;
    sim.board.pad.state.buttons = btn::RIGHT;
    let w = sim.console.step(&mut *sim.board);
    assert!(matches!(w, Wake::At(_)), "the row scrolls: {w:?}");
    assert!(
        w.deadline() <= now + FRAME_US,
        "next frame at {} us, now is {now} us: {} us late",
        w.deadline(),
        w.deadline() - now
    );
}

/// Two inputs in one frame: a button press and a tap in the same step
/// are both used (the press first, then the tap). LEFT and RIGHT together
/// cancel out. Nothing panics.
#[test]
fn two_inputs_in_one_frame() {
    let mut sim = Sim::new(Some(NINE));
    sim.boot();
    sim.idle(10);
    assert_eq!(sim.console.home().selected(), 0);
    // RIGHT and a tap on tile 2 (picture x 102..138, y 22..58) in the
    // same tick: RIGHT moves to 1, then the tap selects 2.
    sim.input(Ev::Press(RIGHT));
    sim.input(Ev::Touch(120, 40));
    sim.step_tick();
    sim.input(Ev::Release(RIGHT));
    sim.input(Ev::Lift);
    sim.idle(20);
    assert_eq!(sim.console.home().selected(), 2);
    // LEFT and RIGHT at once: one step back, one step on.
    sim.input(Ev::Press(LEFT | RIGHT));
    sim.step_tick();
    sim.input(Ev::Release(LEFT | RIGHT));
    sim.idle(20);
    assert_eq!(sim.console.home().selected(), 2);
    assert_eq!(sim.console.page(), Page::Home);
}

/// Fault (found in this round): a button that moved the choice and a tap
/// on the newly chosen tile, in the same frame, started that game. The
/// person tapped a tile that was not chosen when they touched it, so it
/// must only be chosen.
#[test]
fn a_tap_on_a_tile_chosen_in_the_same_frame_does_not_start_it() {
    let mut sim = Sim::new(Some(NINE));
    sim.boot();
    sim.idle(10);
    // RIGHT moves to tile 1 (picture x 58..94); the tap lands on tile 1.
    sim.input(Ev::Press(RIGHT));
    sim.input(Ev::Touch(76, 40));
    sim.step_tick();
    sim.input(Ev::Release(RIGHT));
    sim.input(Ev::Lift);
    sim.idle(5);
    assert_eq!(sim.console.home().selected(), 1);
    assert_eq!(sim.console.notices().len(), 0, "no game was started");
    // A second tap on it now starts it.
    sim.tap_at(76, 40);
    sim.idle(2);
    assert_eq!(sim.console.notices().len(), 1, "the game was started");
}

/// A key held for a long time: the selection walks to the end and stops
/// there, at the repeat rate, with no burst after a late frame.
#[test]
fn a_held_key_walks_to_the_end_and_stops() {
    let mut sim = Sim::new(Some(NINE));
    sim.boot();
    sim.idle(10);
    sim.input(Ev::Press(RIGHT));
    sim.idle(ticks_of(400_000 + 120_000 * 3) + 1);
    // 1 at the press, then one per repeat: 1 + 3 = 4 (maybe 5 with the
    // frame grid).
    let s = sim.console.home().selected();
    assert!((3..=5).contains(&s), "selected {s}");
    sim.idle(ticks_of(2_000_000));
    assert_eq!(sim.console.home().selected(), 8);
    sim.input(Ev::Release(RIGHT));
    sim.idle(10);
    assert_eq!(sim.console.home().selected(), 8);
}

fn ticks_of(us: u64) -> u64 {
    us.div_ceil(FRAME_US)
}

/// Any amount of time between two calls (an hour) is fine: the console
/// does one frame, never a burst, and its clock text still fits.
#[test]
fn a_very_late_call_is_one_frame() {
    let mut board = Box::new(FakeBoard::new());
    let mut console = Box::new(Console::new());
    console.step(&mut *board);
    board.clock.now_us = 3_600 * 1_000_000 * 1_000; // a thousand hours
    let before = console.frames();
    let w = console.step(&mut *board);
    assert_eq!(console.frames(), before + 1);
    assert!(w.deadline() > board.clock.now_us);
    if let Wake::At(t) = w {
        assert!(t <= board.clock.now_us + FRAME_US);
    }
}
