//! Attacks on home's data in the lent RAM (an independent check of
//! `tests/lent.rs`). Every test here tries to make the console show or
//! use an old or rubbish game list or picture, lose games, or read the
//! card for longer in one step than the boot step does:
//!
//! - the card pulled in the step a game ends, and between the step that
//!   reads the list and the step that reads the pictures; put back;
//! - the console menu opened, a message pushed, and buttons pressed while
//!   home is read again;
//! - many starts in a row, good and refused;
//! - a running game and its pause menu never touch the card;
//! - more games than picture slots; the same folders with new pictures;
//! - card calls per step on the way back, against the boot step.

mod common;

use std::cell::Cell;

use common::{Ev, FOUR, HOME, NINE, RIGHT, SOUTH, Sim, art_card, art_file, leak_card};
use console_core::game_api::LinkedGame;
use console_core::home::{Card, MAX_GAMES};
use console_core::{Page, msg};
use game_kit::{Ctx, Flow, Game};

// ---- A small game that does what it is told ----------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Order {
    Run,
    Done,
}

thread_local! {
    static ORDER: Cell<Order> = const { Cell::new(Order::Run) };
}

mod toy {
    use super::*;

    pub struct Toy;

    impl Game for Toy {
        fn start(ctx: &mut Ctx) -> Self {
            ctx.screen().clear(9);
            Toy
        }

        fn update(&mut self, _ctx: &mut Ctx) -> Flow {
            match ORDER.with(Cell::get) {
                Order::Run => Flow::Run,
                Order::Done => Flow::Done,
            }
        }
    }

    game_kit::game!(Toy);
}

/// The toy under each folder name the test cards use.
macro_rules! toys {
    ($($name:literal),*) => {
        // SAFETY: made by `game_kit::game!`.
        &[$(unsafe { LinkedGame::new($name, toy::game_entry) }),*]
    };
}

static ON_FOUR: &[LinkedGame] = toys!("BLOCKS", "COMET", "SPROUT", "TRAIL");
static ON_NINE: &[LinkedGame] = toys!("ALPHA", "BRAVO", "INDIA");
static ON_ART: &[LinkedGame] = toys!(
    "G00", "G01", "G02", "G03", "G04", "G05", "G06", "G07", "G08", "G09", "G10", "G11"
);

// ---- Helpers -------------------------------------------------------------------

fn home(card: &'static [(&'static str, &'static [u8])], games: &'static [LinkedGame]) -> Sim {
    ORDER.with(|o| o.set(Order::Run));
    let mut sim = Sim::with_games(Some(card), games);
    sim.boot();
    settle(&mut sim);
    sim
}

/// Runs until home is drawn, still, without notices or pictures to read.
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

#[derive(PartialEq)]
struct Shown {
    panel: Vec<u16>,
    picture: Vec<u8>,
    list: Vec<String>,
    selected: usize,
}

/// Short: the list, the selection, and a fingerprint of each picture
/// (a failed comparison stays readable).
impl std::fmt::Debug for Shown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let picture: Vec<u16> = self.picture.iter().map(|&p| u16::from(p)).collect();
        write!(
            f,
            "Shown {{ list: {:?}, selected: {}, picture: {:016x}, panel: {:016x} }}",
            self.list,
            self.selected,
            common::fingerprint(&picture),
            common::fingerprint(&self.panel)
        )
    }
}

fn shown(sim: &mut Sim) -> Shown {
    settle(sim);
    Shown {
        panel: sim.board.screen.image.to_vec(),
        picture: sim.console.video().frame().0.to_vec(),
        list: list(sim),
        selected: sim.console.home().selected(),
    }
}

/// Opens the running game's pause menu and presses BACK TO HOME, up to
/// the step the game ends in (home has no list yet).
fn quit_game(sim: &mut Sim) {
    assert_eq!(sim.console.page(), Page::Game);
    sim.tap_button(HOME);
    assert!(sim.console.game().paused());
    sim.tap_button(common::DOWN);
    sim.tap_button(common::DOWN);
    // A short press (down and up between two steps), so no button is
    // left held for the steps after.
    sim.input(Ev::Quick(SOUTH));
    sim.step_tick();
    assert_eq!(sim.console.page(), Page::Home);
    assert!(
        !sim.console.home().has_list(),
        "the game ended in this step"
    );
    assert!(sim.console.home_pending());
}

/// The same on the panel (colours), the same list and selection.
fn same_look(a: Shown, b: Shown) {
    assert_eq!(a.list, b.list);
    assert_eq!(a.selected, b.selected);
    assert!(a.panel == b.panel, "the panels differ: {a:?} / {b:?}");
}

fn names_are_clean(l: &[String]) {
    for g in l {
        assert!(!g.contains('?'), "rubbish in the list: {g}");
    }
}

/// Every step of `sim` from now on that sends while home is pending
/// fails the test; runs until home is drawn.
fn finish_back(sim: &mut Sim) -> usize {
    let mut steps = 0;
    while sim.console.home_pending() {
        let t = sim.step_tick();
        if sim.console.home_pending() {
            assert_eq!(t.areas, 0, "sent while home was read again");
        }
        steps += 1;
        assert!(steps < 10, "home never came back");
    }
    steps
}

// ---- The card -------------------------------------------------------------------

#[test]
fn a_card_pulled_in_the_step_a_game_ends_shows_no_card_and_nothing_old() {
    let mut sim = home(FOUR, ON_FOUR);
    let before = shown(&mut sim);
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Game);
    // The game ends and the card goes out in the same step.
    ORDER.with(|o| o.set(Order::Done));
    sim.input(Ev::Card(false));
    sim.step_tick();
    ORDER.with(|o| o.set(Order::Run));
    assert_eq!(sim.console.page(), Page::Home);
    finish_back(&mut sim);
    assert_eq!(sim.console.home().card(), Card::None);
    assert_eq!(sim.console.home().count(), 0);
    assert!(list(&mut sim).is_empty());
    // "CARD REMOVED" is said (after "GAME ENDED").
    let mut said = Vec::new();
    for _ in 0..600 {
        if let Some(n) = sim.console.notices().current()
            && said.last().is_none_or(|l: &String| l != n)
        {
            said.push(n.to_string());
        }
        sim.step_tick();
    }
    assert_eq!(said, [msg::GAME_ENDED, msg::CARD_OUT]);
    assert!(list(&mut sim).is_empty());
    // Back in: the very same home as before.
    sim.input(Ev::Card(true));
    for _ in 0..200 {
        if sim.console.home().card() == Card::Games {
            break;
        }
        sim.step_tick();
    }
    assert_eq!(shown(&mut sim), before);
}

#[test]
fn a_card_pulled_between_the_list_and_the_pictures_is_safe_and_heals() {
    let mut sim = home(art_card(6, &[]), ON_ART);
    let before = shown(&mut sim);
    sim.tap_button(SOUTH);
    quit_game(&mut sim);
    // The step that reads the list again.
    sim.step_tick();
    assert!(sim.console.home().has_list());
    assert!(sim.console.home_pending(), "the pictures come next step");
    // The card goes out before the pictures are read.
    sim.input(Ev::Card(false));
    let t = sim.step_tick();
    assert!(!sim.console.home_pending());
    assert!(t.areas > 0, "home is drawn (with letter tiles)");
    // The list is still the right one (it is in the RAM, card or not).
    let l = list(&mut sim);
    assert_eq!(l, before.list);
    names_are_clean(&l);
    for k in 0..6 {
        assert!(sim.console.home().art().slot(k).is_none(), "no picture {k}");
    }
    // The quick card check sees the card gone, then finds it again: the
    // pictures come back with the list.
    sim.idle(80);
    assert_eq!(sim.console.home().card(), Card::None);
    sim.input(Ev::Card(true));
    for _ in 0..200 {
        if sim.console.home().card() == Card::Games {
            break;
        }
        sim.step_tick();
    }
    assert_eq!(shown(&mut sim), before);
}

#[test]
fn the_same_folders_with_new_pictures_show_the_new_pictures() {
    // A card swapped during a game for one with the same folders but
    // other pictures and colours: nothing of the old card may stay.
    let new_card = || {
        let mut files = Vec::new();
        for i in 0..6usize {
            let dir = format!("/CONSOLE/GAMES/G{i:02}");
            files.push((format!("{dir}/GAME.BIN"), b"GAME".to_vec()));
            files.push((
                format!("{dir}/INFO.TXT"),
                format!(
                    "NAME=NEW {i}\nTAG=OTHER CARD\nTILE={:06X}\n",
                    0x102030 * (i + 1)
                )
                .into_bytes(),
            ));
            files.push((
                format!("{dir}/ART.BIN"),
                art_file(0x8000 + i as u16 * 0x0321, i + 7),
            ));
        }
        leak_card(files)
    };
    let b = new_card();
    let mut sim = home(art_card(6, &[]), ON_ART);
    sim.tap_button(SOUTH);
    sim.board.files.files = b;
    quit_game(&mut sim);
    let after = shown(&mut sim);
    let mut fresh = home(b, ON_ART);
    let want = shown(&mut fresh);
    assert_eq!(after.list, want.list);
    assert!(after.list[0].contains("NEW 0"), "{:?}", after.list);
    assert!(after.picture == want.picture, "the new card's pictures");
    assert!(
        after.panel == want.panel,
        "the new card's colours on the panel"
    );
}

#[test]
fn more_games_than_picture_slots_come_back_where_the_player_was() {
    let mut sim = home(art_card(12, &[3, 8]), ON_ART);
    for _ in 0..9 {
        sim.tap_button(RIGHT);
    }
    let before = shown(&mut sim);
    assert_eq!(before.selected, 9);
    assert!(sim.console.home().scroll() > 0, "the row is scrolled");
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.game().folder(), "G09");
    quit_game(&mut sim);
    // The pictures may sit in other slots (other palette numbers) than
    // before: the panel, the list and the selection must be the same.
    same_look(shown(&mut sim), before);
    // The kept pictures are those home wants, and only those.
    let (want, n) = sim.console.home().art_wanted();
    for &g in &want[..n] {
        let art = sim.console.home().art();
        assert!(art.slot(g).is_some() || g == 8, "picture of game {g}");
    }
    // Back to the start of the row: the evicted pictures are read again.
    for _ in 0..9 {
        sim.tap_button(common::LEFT);
    }
    let left = shown(&mut sim);
    let mut fresh = home(art_card(12, &[3, 8]), ON_ART);
    same_look(left, shown(&mut fresh));
}

// ---- Input, menus and messages during the reload ---------------------------------

#[test]
fn the_console_menu_opened_while_home_is_read_again_looks_as_usual() {
    let mut sim = home(FOUR, ON_FOUR);
    // The menu opened on a settled home: the picture to expect.
    sim.tap_button(HOME);
    assert!(sim.console.menu_open());
    let with_menu = shown(&mut sim);
    sim.tap_button(HOME);
    assert!(!sim.console.menu_open());
    settle(&mut sim);
    sim.tap_button(SOUTH);
    quit_game(&mut sim);
    // HOME pressed in the step that reads the list again.
    sim.input(Ev::Press(HOME));
    sim.step_tick();
    sim.input(Ev::Release(HOME));
    finish_back(&mut sim);
    assert!(sim.console.menu_open());
    assert_eq!(shown(&mut sim), with_menu);
}

#[test]
fn a_message_during_the_reload_waits_and_nothing_is_sent_early() {
    let mut sim = home(FOUR, ON_FOUR);
    let before = shown(&mut sim);
    sim.tap_button(SOUTH);
    quit_game(&mut sim);
    assert!(sim.notify("HELLO"));
    let steps = finish_back(&mut sim);
    assert!(steps <= 2, "{steps}");
    assert_eq!(sim.console.notices().current(), Some("HELLO"));
    assert_eq!(shown(&mut sim), before);
}

#[test]
fn buttons_during_the_reload_act_on_the_new_list() {
    // RIGHT in the step that reads the list, A in the step that reads
    // the pictures: the second game of the list starts.
    let mut sim = home(NINE, ON_NINE);
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.game().folder(), "ALPHA");
    quit_game(&mut sim);
    sim.input(Ev::Quick(RIGHT));
    sim.step_tick();
    assert!(sim.console.home().has_list());
    assert_eq!(sim.console.home().selected(), 1);
    sim.input(Ev::Quick(SOUTH));
    sim.step_tick();
    sim.idle(1);
    assert_eq!(sim.console.page(), Page::Game);
    assert_eq!(sim.console.game().folder(), "BRAVO");
    quit_game(&mut sim);
    finish_back(&mut sim);
    let l = list(&mut sim);
    assert_eq!(l.len(), 9);
    names_are_clean(&l);
}

#[test]
fn a_press_in_every_step_with_refused_starts_never_shows_rubbish() {
    let mut sim = home(FOUR, ON_FOUR);
    let before = shown(&mut sim);
    sim.board.games.refuse = Some("NO");
    let starts = sim.board.games.lent.scrambles;
    for _ in 0..12 {
        sim.input(Ev::Quick(SOUTH));
        let t = sim.step_tick();
        if sim.console.home_pending() {
            assert_eq!(t.areas, 0, "sent while home was read again");
        }
        assert_eq!(sim.console.page(), Page::Home);
    }
    assert!(sim.board.games.lent.scrambles >= starts + 6);
    sim.board.games.refuse = None;
    assert_eq!(shown(&mut sim), before);
    // Two good starts right after each other: A in the first step that
    // draws home again.
    for round in 0..2 {
        sim.tap_button(SOUTH);
        assert_eq!(sim.console.page(), Page::Game, "round {round}");
        quit_game(&mut sim);
        sim.step_tick(); // reads the list
        sim.input(Ev::Quick(SOUTH));
        sim.step_tick(); // the pictures, home drawn, and the start
        sim.idle(1);
        assert_eq!(sim.console.page(), Page::Game, "round {round}");
        assert_eq!(sim.console.game().folder(), "BLOCKS");
        quit_game(&mut sim);
    }
    assert_eq!(shown(&mut sim), before);
}

// ---- While a game is loaded ------------------------------------------------------

#[test]
fn a_game_and_its_pause_menu_read_neither_the_card_nor_the_lent_ram() {
    let mut sim = home(art_card(6, &[]), ON_ART);
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Game);
    let calls = sim.board.files.calls;
    sim.idle(30);
    sim.tap_button(HOME);
    assert!(sim.console.game().paused());
    for _ in 0..5 {
        sim.tap_button(common::DOWN);
        sim.tap_button(common::UP);
    }
    sim.idle(90); // past a card check's time
    sim.input(Ev::Card(false));
    sim.idle(90);
    sim.input(Ev::Card(true));
    assert_eq!(sim.board.files.calls, calls, "the card was read in a game");
    assert!(sim.board.games.lent.loaded);
    assert!(list(&mut sim).is_empty());
    assert!(
        sim.console.home().art().slot(0).is_none(),
        "pictures forgotten"
    );
}

// ---- What one step costs -----------------------------------------------------------

#[test]
fn no_step_of_the_way_back_reads_more_than_the_boot_step() {
    for (name, n) in [("32 games", MAX_GAMES), ("40 games", 40), ("6 games", 6)] {
        let card = art_card(n, &[]);
        ORDER.with(|o| o.set(Order::Run));
        let mut sim = Sim::with_games(Some(card), ON_ART);
        let mut boot_step = 0;
        while sim.console.page() == Page::Boot {
            let c = sim.board.files.calls;
            sim.step_tick();
            boot_step = boot_step.max(sim.board.files.calls - c);
        }
        settle(&mut sim);
        sim.tap_button(SOUTH);
        assert_eq!(sim.console.page(), Page::Game, "{name}");
        quit_game(&mut sim);
        let mut worst = 0;
        let mut steps = 0;
        while sim.console.home_pending() {
            let c = sim.board.files.calls;
            sim.step_tick();
            worst = worst.max(sim.board.files.calls - c);
            steps += 1;
        }
        println!(
            "{name}: boot step {boot_step} card calls; way back: worst step {worst}, {steps} steps"
        );
        assert!(worst <= boot_step, "{name}: {worst} > {boot_step}");
        assert_eq!(sim.console.home().count(), n.min(MAX_GAMES), "{name}");
        names_are_clean(&list(&mut sim));
    }
}
