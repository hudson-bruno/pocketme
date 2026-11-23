//! Home's data in the lent RAM (on the chip, the empty game data slot).
//!
//! The test board lends RAM that starts as rubbish and becomes other
//! rubbish at every game start and end (board-api's `FakeLent`), as the
//! chip's loader writes the game over the slot. So a console that showed
//! its list from the RAM after a game, without reading the card again,
//! would show wrong names and colours here.
//!
//! Every way back home is tried: the pause menu, a game that ends, a game
//! that panics, a start the board refuses (a bad file), a game that is
//! not there, a board that starts no games, a card pulled during a game,
//! a card changed during a game, and a board that broke the loan's rules.
//! After each one, home must show exactly what it showed before (or what
//! the new card holds), the list must be complete, and nothing may be
//! sent while it is read again.

mod common;

use std::cell::Cell;

use common::{DOWN, Ev, FOUR, HOME, NINE, SOUTH, Sim, art_card};
use console_core::game_api::LinkedGame;
use console_core::home::{Card, LENT_BYTES};
use console_core::{Page, msg};
use game_kit::{Ctx, Flow, Game};

// ---- A small game that does what it is told ----------------------------------

/// What the toy does in its next call.
#[derive(Clone, Copy, PartialEq)]
enum Order {
    Run,
    Done,
    Panic,
}

thread_local! {
    static ORDER: Cell<Order> = const { Cell::new(Order::Run) };
}

mod toy {
    use super::*;

    pub struct Toy;

    impl Game for Toy {
        fn start(ctx: &mut Ctx) -> Self {
            // A picture unlike home's, so a home that is not drawn again
            // shows up.
            ctx.screen().clear(7);
            Toy
        }

        fn update(&mut self, _ctx: &mut Ctx) -> Flow {
            match ORDER.with(Cell::get) {
                Order::Run => Flow::Run,
                Order::Done => Flow::Done,
                Order::Panic => panic!("the toy was told to"),
            }
        }
    }

    game_kit::game!(Toy);
}

// SAFETY: made by `game_kit::game!`.
const TOY: LinkedGame = unsafe { LinkedGame::new("BLOCKS", toy::game_entry) };
/// The toy as BLOCKS (the first tile of FOUR).
static ON_FOUR: &[LinkedGame] = &[TOY];
/// The toy as G00 (the first tile of the picture card).
// SAFETY: as above.
static ON_ART: &[LinkedGame] = &[unsafe { LinkedGame::new("G00", toy::game_entry) }];
/// The toy as ALPHA (the first tile of NINE).
// SAFETY: as above.
static ON_NINE: &[LinkedGame] = &[unsafe { LinkedGame::new("ALPHA", toy::game_entry) }];

// ---- Helpers -------------------------------------------------------------------

/// A booted console on `card` with `games` linked in, home settled.
fn home(card: &'static [(&'static str, &'static [u8])], games: &'static [LinkedGame]) -> Sim {
    ORDER.with(|o| o.set(Order::Run));
    let mut sim = Sim::with_games(Some(card), games);
    sim.boot();
    settle(&mut sim);
    sim
}

/// Runs until home is drawn, still, and has no notice; then a few more.
fn settle(sim: &mut Sim) {
    for _ in 0..900 {
        let c = &sim.console;
        let busy = c.home_pending()
            || c.home().moving()
            || !c.notices().is_empty()
            || c.home().art_pending();
        if c.page() == Page::Home && !busy {
            break;
        }
        sim.step_tick();
    }
    assert_eq!(sim.console.page(), Page::Home);
    assert!(!sim.console.home_pending());
    sim.idle(3);
}

/// Home's list, one line per game: folder, name, tag, tile colour.
fn list(sim: &mut Sim) -> Vec<String> {
    let l = sim.console.home_games(&mut *sim.board);
    l.iter()
        .map(|g| {
            format!(
                "{}|{}|{}|{:04X}",
                g.folder.as_str(),
                g.name.as_str(),
                g.tag.as_str(),
                g.tile
            )
        })
        .collect()
}

/// What home shows, settled: the panel, the picture, the list.
#[derive(Debug, PartialEq)]
struct Shown {
    panel: Vec<u16>,
    picture: Vec<u8>,
    list: Vec<String>,
}

fn shown(sim: &mut Sim) -> Shown {
    settle(sim);
    Shown {
        panel: sim.board.screen.image.to_vec(),
        picture: sim.console.video().frame().0.to_vec(),
        list: list(sim),
    }
}

/// Starts the selected game with a short press, as a player would.
fn start_selected(sim: &mut Sim) {
    sim.tap_button(SOUTH);
}

/// One tick, written down: (home pending after it, areas sent, card
/// calls so far).
fn tick(sim: &mut Sim, log: &mut Vec<(bool, u32, u32)>) {
    let t = sim.step_tick();
    log.push((sim.console.home_pending(), t.areas, sim.board.files.calls));
}

/// What the way back home cost, from the tick that ended the game (or
/// failed to start it) to the first tick that drew home again.
#[derive(Debug)]
struct Back {
    /// Ticks that sent nothing because home was read again.
    pending: usize,
    /// Card calls (list, size, read) in those ticks and the one after.
    reads: u32,
}

/// Runs the ticks after `log`'s last one until home is drawn again, and
/// checks that nothing was sent while home was pending.
fn back(sim: &mut Sim, mut log: Vec<(bool, u32, u32)>, calls_before: u32) -> Back {
    while log.last().is_some_and(|l| l.0) {
        tick(sim, &mut log);
        assert!(log.len() < 20, "home never came back");
    }
    let pending = log.iter().filter(|l| l.0).count();
    for l in &log {
        if l.0 {
            assert_eq!(l.1, 0, "sent while home was read again: {log:?}");
        }
    }
    Back {
        pending,
        reads: log.last().map_or(0, |l| l.2) - calls_before,
    }
}

/// Leaves the running game through its pause menu (BACK TO HOME, the
/// third item) and watches the way back.
fn leave_by_menu(sim: &mut Sim) -> Back {
    assert_eq!(sim.console.page(), Page::Game);
    sim.tap_button(HOME);
    sim.tap_button(DOWN);
    sim.tap_button(DOWN);
    let calls = sim.board.files.calls;
    let mut log = Vec::new();
    sim.input(Ev::Press(SOUTH));
    tick(sim, &mut log);
    sim.input(Ev::Release(SOUTH));
    tick(sim, &mut log);
    assert_eq!(sim.console.page(), Page::Home);
    back(sim, log, calls)
}

/// Tells the toy to end (or panic) and watches the way back.
fn end_by(sim: &mut Sim, order: Order) -> Back {
    assert_eq!(sim.console.page(), Page::Game);
    ORDER.with(|o| o.set(order));
    let calls = sim.board.files.calls;
    let mut log = Vec::new();
    tick(sim, &mut log);
    ORDER.with(|o| o.set(Order::Run));
    assert_eq!(sim.console.page(), Page::Home);
    back(sim, log, calls)
}

/// A start that fails: watched from the press.
fn failed_start(sim: &mut Sim) -> Back {
    let calls = sim.board.files.calls;
    let mut log = Vec::new();
    sim.input(Ev::Press(SOUTH));
    tick(sim, &mut log);
    sim.input(Ev::Release(SOUTH));
    tick(sim, &mut log);
    assert_eq!(sim.console.page(), Page::Home);
    back(sim, log, calls)
}

fn names_are_clean(l: &[String]) {
    for g in l {
        assert!(!g.contains('?'), "rubbish in the list: {g}");
    }
}

// ---- Every way back home ---------------------------------------------------------

#[test]
fn the_lent_ram_is_rubbish_at_start_and_home_needs_little_of_it() {
    // The console never reads before it writes: the first loan is
    // rubbish (the chip: zeros), and the list still comes out right.
    let mut sim = home(FOUR, ON_FOUR);
    let l = list(&mut sim);
    assert_eq!(l.len(), 4);
    names_are_clean(&l);
    assert!(l[0].starts_with("BLOCKS|BLOCKS|PUZZLE|"), "{l:?}");
    // All of home's data fits easily in the 48 KB slot.
    assert_eq!(LENT_BYTES, 5_396);
    assert!(LENT_BYTES <= console_core::game_api::layout::DATA_SLOT_SIZE as usize);
}

#[test]
fn back_by_the_pause_menu_reads_home_again() {
    let mut sim = home(FOUR, ON_FOUR);
    let before = shown(&mut sim);
    let scrambles = sim.board.games.lent.scrambles;
    start_selected(&mut sim);
    assert_eq!(sim.console.page(), Page::Game);
    // While the game runs, nothing is lent, and home has no list.
    assert!(sim.board.games.lent.loaded);
    assert!(list(&mut sim).is_empty());
    let b = leave_by_menu(&mut sim);
    // The game had the RAM (rubbish now), and home read it all again.
    assert!(sim.board.games.lent.scrambles >= scrambles + 2);
    assert_eq!(b.pending, 2, "{b:?}");
    assert!(b.reads > 0);
    assert_eq!(shown(&mut sim), before);
}

#[test]
fn back_from_a_game_that_ends_reads_home_again() {
    let mut sim = home(FOUR, ON_FOUR);
    let before = shown(&mut sim);
    start_selected(&mut sim);
    let b = end_by(&mut sim, Order::Done);
    assert_eq!(b.pending, 2, "{b:?}");
    assert_eq!(sim.console.notices().current(), Some(msg::GAME_ENDED));
    assert_eq!(shown(&mut sim), before);
}

#[test]
fn back_from_a_game_that_panics_reads_home_again() {
    let mut sim = home(FOUR, ON_FOUR);
    let before = shown(&mut sim);
    start_selected(&mut sim);
    let b = end_by(&mut sim, Order::Panic);
    assert_eq!(b.pending, 2, "{b:?}");
    assert_eq!(sim.console.notices().current(), Some(msg::GAME_ABORTED));
    assert_eq!(shown(&mut sim), before);
}

#[test]
fn a_start_the_board_refuses_reads_home_again() {
    // The chip's loader may have written half a file over the slot
    // before it found the fault: the RAM is gone all the same.
    let mut sim = home(FOUR, ON_FOUR);
    let before = shown(&mut sim);
    sim.board.games.refuse = Some("GAME FILE IS DAMAGED");
    let scrambles = sim.board.games.lent.scrambles;
    let b = failed_start(&mut sim);
    assert!(sim.board.games.lent.scrambles > scrambles);
    assert!(!sim.board.games.lent.loaded);
    assert_eq!(b.pending, 2, "{b:?}");
    assert_eq!(
        sim.console.notices().current(),
        Some("GAME FILE IS DAMAGED")
    );
    assert_eq!(shown(&mut sim), before);
    // And the next start works.
    sim.board.games.refuse = None;
    start_selected(&mut sim);
    assert_eq!(sim.console.page(), Page::Game);
}

#[test]
fn a_game_not_found_and_a_board_without_games_read_home_again() {
    // SAFETY: made by `game_kit::game!`.
    static OTHER: &[LinkedGame] = &[unsafe { LinkedGame::new("COMET", toy::game_entry) }];
    for (games, notice) in [(OTHER, msg::GAME_NOT_FOUND), (&[][..], msg::NO_START)] {
        let mut sim = home(FOUR, games);
        let before = shown(&mut sim);
        let b = failed_start(&mut sim);
        assert_eq!(b.pending, 2, "{b:?}");
        assert_eq!(sim.console.notices().current(), Some(notice));
        assert_eq!(shown(&mut sim), before);
    }
}

#[test]
fn a_card_pulled_during_a_game_shows_no_card_then_comes_back_whole() {
    let mut sim = home(FOUR, ON_FOUR);
    let before = shown(&mut sim);
    start_selected(&mut sim);
    sim.input(Ev::Card(false));
    sim.idle(5);
    let b = leave_by_menu(&mut sim);
    assert_eq!(b.pending, 2, "{b:?}");
    assert_eq!(sim.console.notices().current(), Some(msg::CARD_OUT));
    assert_eq!(sim.console.home().card(), Card::None);
    assert!(list(&mut sim).is_empty());
    assert_eq!(sim.console.home().count(), 0);
    // The card goes back in: the next card check finds every game.
    sim.input(Ev::Card(true));
    let mut ticks = 0;
    while sim.console.home().card() != Card::Games {
        sim.step_tick();
        ticks += 1;
        assert!(ticks < 200, "the card was never found again");
    }
    // "CARD READY" follows "CARD REMOVED" in the queue.
    while sim.console.notices().current() != Some(msg::CARD_IN) {
        sim.step_tick();
        ticks += 1;
        assert!(ticks < 400, "no CARD READY");
    }
    assert_eq!(shown(&mut sim), before);
}

#[test]
fn a_card_changed_during_a_game_shows_the_new_cards_games() {
    // Taken out and another put in while the game ran: home shows the
    // new card exactly as a console booted with it, never the old list.
    let mut sim = home(FOUR, ON_FOUR);
    start_selected(&mut sim);
    sim.board.files.files = NINE;
    leave_by_menu(&mut sim);
    let after = shown(&mut sim);
    let mut fresh = home(NINE, ON_NINE);
    let want = shown(&mut fresh);
    assert_eq!(after.list, want.list);
    assert_eq!(after.list.len(), 9);
    names_are_clean(&after.list);
    assert!(
        after.picture == want.picture,
        "home looks as booted with it"
    );
}

#[test]
fn tile_pictures_come_back_after_a_game() {
    let mut sim = home(art_card(6, &[]), ON_ART);
    let before = shown(&mut sim);
    let tried = sim.console.home().art_wanted().1;
    assert_eq!(tried, 5, "the tiles on screen, then one off the end");
    for k in 0..tried {
        assert!(sim.console.home().art().slot(k).is_some(), "picture {k}");
    }
    start_selected(&mut sim);
    let b = leave_by_menu(&mut sim);
    assert_eq!(b.pending, 2, "{b:?}");
    for k in 0..tried {
        assert!(sim.console.home().art().slot(k).is_some(), "picture {k}");
    }
    assert_eq!(shown(&mut sim), before);
}

#[test]
fn a_board_that_changes_the_lent_ram_is_caught_by_the_stamp() {
    // Against the loan's rules, the board writes over the RAM while home
    // shows (no game start). The stamp no longer matches: home reads the
    // card again instead of drawing rubbish.
    let mut sim = home(FOUR, ON_FOUR);
    let before = shown(&mut sim);
    sim.board.games.lent.scramble();
    let calls = sim.board.files.calls;
    // Something that wakes the sleeping console but changes nothing.
    sim.input(Ev::Pad(true));
    sim.idle(1);
    assert!(sim.console.home_pending());
    let log = vec![(true, 0, sim.board.files.calls)];
    let b = back(&mut sim, log, calls);
    assert!(b.reads > 0);
    assert_eq!(shown(&mut sim), before);
}

#[test]
fn a_game_starts_right_after_boot() {
    // The first press after the boot screen: the list was just read into
    // RAM that was rubbish before.
    ORDER.with(|o| o.set(Order::Run));
    let mut sim = Sim::with_games(Some(FOUR), ON_FOUR);
    sim.boot();
    start_selected(&mut sim);
    assert_eq!(sim.console.page(), Page::Game);
    assert_eq!(sim.console.game().folder(), "BLOCKS");
    leave_by_menu(&mut sim);
    names_are_clean(&list(&mut sim));
    assert_eq!(list(&mut sim).len(), 4);
}

#[test]
fn a_press_while_home_is_read_again_starts_the_right_game() {
    // The confirm press comes in the very step that reads the list
    // again: the folder is taken from the new list.
    let mut sim = home(FOUR, ON_FOUR);
    start_selected(&mut sim);
    ORDER.with(|o| o.set(Order::Done));
    sim.input(Ev::Press(SOUTH));
    sim.step_tick(); // the game ends; the press is the game's
    assert_eq!(sim.console.page(), Page::Home);
    ORDER.with(|o| o.set(Order::Run));
    sim.input(Ev::Release(SOUTH));
    sim.step_tick();
    sim.input(Ev::Quick(SOUTH));
    sim.step_tick();
    sim.idle(2);
    assert_eq!(sim.console.page(), Page::Game);
    assert_eq!(sim.console.game().folder(), "BLOCKS");
    assert_eq!(sim.board.games.started, 2);
}

#[test]
fn many_rounds_of_every_kind_leave_home_as_it_was() {
    let mut sim = home(FOUR, ON_FOUR);
    let before = shown(&mut sim);
    for round in 0..12 {
        match round % 4 {
            0 => {
                start_selected(&mut sim);
                leave_by_menu(&mut sim);
            }
            1 => {
                start_selected(&mut sim);
                end_by(&mut sim, Order::Done);
            }
            2 => {
                start_selected(&mut sim);
                end_by(&mut sim, Order::Panic);
            }
            _ => {
                sim.board.games.refuse = Some("NO");
                failed_start(&mut sim);
                sim.board.games.refuse = None;
            }
        }
        assert_eq!(shown(&mut sim), before, "round {round}");
    }
}

#[test]
fn a_board_that_lends_nothing_shows_no_games_and_does_not_crash() {
    // A board written before lent RAM (the default `parts_and_lent`): the
    // console has nowhere to keep a list.
    ORDER.with(|o| o.set(Order::Run));
    let mut sim = Sim::with_games(Some(FOUR), ON_FOUR);
    sim.board.games.lent.loaded = true; // lends nothing
    sim.boot();
    sim.idle(60);
    assert_eq!(sim.console.page(), Page::Home);
    assert_eq!(sim.console.home().count(), 0);
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Home);
}

// ---- Measured: what the way back costs ------------------------------------------

/// The card calls and steps of the way back, against the boot's scan, for
/// the report (`cargo test --test lent -- --nocapture`). The board
/// decides the time; these are counts.
#[test]
fn the_way_back_costs_what_the_boot_scan_costs() {
    for (name, card, games) in [
        ("FOUR (4 games, no pictures)", FOUR, ON_FOUR),
        ("6 games with pictures", art_card(6, &[]), ON_ART),
    ] {
        ORDER.with(|o| o.set(Order::Run));
        let mut sim = Sim::with_games(Some(card), games);
        let before_boot = sim.board.files.calls;
        sim.boot();
        let boot_reads = sim.board.files.calls - before_boot;
        settle(&mut sim);
        start_selected(&mut sim);
        let b = leave_by_menu(&mut sim);
        println!(
            "{name}: boot scan {boot_reads} card calls in 1 step; way back {} card calls, \
             {} steps sending nothing, home drawn in the step after",
            b.reads, b.pending
        );
        assert_eq!(b.reads, boot_reads, "{name}");
        assert_eq!(b.pending, 2, "{name}");
    }
}
