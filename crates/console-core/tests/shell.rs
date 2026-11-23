//! The console's screens on the test board, driven by the fake clock and
//! short input scripts. No window; the same result on every run.

mod common;

use board_api::Wake;
use common::{EMPTY, Ev, FOUR, HOME, LEFT, NINE, RIGHT, SOUTH, Sim};
use console_core::home::{Card, STRIDE};
use console_core::theme::{
    self, GOLD, NOTE_SHOW_US, NOTE_SLIDE_US, NOTE_Y, OK, REPEAT_DELAY_US, REPEAT_US, TILE,
    TILE_LEFT, TILE_MARGIN, TILE_Y, WARN,
};
use console_core::{FRAME_US, Page, msg};

/// The boot page's frame time (the GATHER animation, feature
/// `boot-gather`, asks for frames twice as often).
#[cfg(not(feature = "boot-gather"))]
const BOOT_FRAME: u64 = FRAME_US;
#[cfg(feature = "boot-gather")]
const BOOT_FRAME: u64 = theme::BOOT_FRAME_US;

/// Ticks needed for `us` microseconds.
fn ticks(us: u64) -> u64 {
    us.div_ceil(FRAME_US)
}

/// Where tile `i` is on screen now (left edge, picture pixels).
fn tile_x(sim: &Sim, i: usize) -> i16 {
    TILE_LEFT + i as i16 * STRIDE - sim.console.home().scroll()
}

// ---- Boot ------------------------------------------------------------------

// The boot screen without the GATHER animation (with it: tests/gather.rs).
#[cfg(not(feature = "boot-gather"))]
#[test]
fn boot_screen_ends_by_itself() {
    let mut sim = Sim::new(Some(FOUR));
    sim.idle(ticks(theme::BOOT_US) - 2);
    assert_eq!(sim.console.page(), Page::Boot);
    // Something is drawn: the name in warm white.
    assert!(sim.console.video().frame().0.contains(&theme::PAPER));
    sim.idle(3);
    assert_eq!(sim.console.page(), Page::Home);
    assert_eq!(sim.board.log.last(), "console: home");
    assert_eq!(sim.console.home().count(), 4);
}

#[test]
fn boot_screen_is_skipped_by_a_button_or_a_tap() {
    let mut sim = Sim::new(Some(FOUR));
    sim.idle(5);
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.page(), Page::Home);
    assert!(sim.tick < 10);
    // The press that skipped the boot screen does not also act on home.
    assert!(sim.console.notices().is_empty());

    let mut sim = Sim::new(Some(FOUR));
    sim.idle(3);
    sim.tap_at(80, 60);
    assert_eq!(sim.console.page(), Page::Home);

    // The board's BOOT button skips too (the CYD has no other).
    let mut sim = Sim::new(Some(FOUR));
    sim.idle(3);
    sim.input(Ev::Boot(true));
    sim.step_tick();
    assert_eq!(sim.console.page(), Page::Home);
}

// The boot screen without the GATHER animation (with it: tests/gather.rs).
#[cfg(not(feature = "boot-gather"))]
#[test]
fn boot_animation_changes_little_per_frame() {
    let mut sim = Sim::new(Some(FOUR));
    sim.idle(ticks(theme::BOOT_US) - 1);
    // The first frame sends everything; after that only small spots.
    let first = sim.ticks[0];
    assert_eq!(first.pixels, 320 * 240);
    for t in &sim.ticks[1..] {
        assert!(t.pixels <= 64 * 64, "a boot frame sent {} pixels", t.pixels);
    }
}

// ---- Home: moving -----------------------------------------------------------

#[test]
fn games_are_listed_sorted_with_their_info() {
    let mut sim = Sim::new(Some(FOUR));
    sim.boot();
    let list = sim.console.home_games(&mut *sim.board);
    let names: Vec<&str> = list.iter().map(|g| g.name.as_str()).collect();
    // The folder with no GAME.BIN is not a game.
    assert_eq!(names, ["BLOCKS", "COMET", "SPROUT", "TRAIL"]);
    let trail = &list[3];
    assert_eq!(trail.tag.as_str(), "USES GPS");
    assert_eq!(trail.tile, theme::rgb(0xC28A3A));
    assert_eq!(sim.console.home().card(), Card::Games);
}

#[test]
fn pad_moves_the_selection() {
    let mut sim = Sim::new(Some(FOUR));
    sim.boot();
    let home = |s: &Sim| s.console.home().selected();
    assert_eq!(home(&sim), 0);
    sim.tap_button(RIGHT);
    assert_eq!(home(&sim), 1);
    sim.tap_button(RIGHT);
    sim.tap_button(RIGHT);
    sim.tap_button(RIGHT); // already the last: stays
    assert_eq!(home(&sim), 3);
    sim.tap_button(LEFT);
    assert_eq!(home(&sim), 2);
    // Home opens the console menu (it used to jump to the first game);
    // the selection stays, and the pad moves the menu, not the row.
    sim.tap_button(HOME);
    assert!(sim.console.menu_open());
    assert_eq!(home(&sim), 2);
    sim.tap_button(LEFT);
    assert_eq!(home(&sim), 2);
    sim.tap_button(HOME);
    assert!(!sim.console.menu_open());
    sim.tap_button(LEFT);
    sim.tap_button(LEFT);
    assert_eq!(home(&sim), 0);
    // The gold frame is around the selected tile.
    sim.idle(10);
    let x = tile_x(&sim, 0);
    assert_eq!(sim.pixel((x - 2) as u16, TILE_Y as u16), GOLD);
}

#[test]
fn stick_moves_with_key_repeat() {
    let mut sim = Sim::new(Some(NINE));
    sim.boot();
    // Inside the dead zone: nothing.
    sim.input(Ev::Stick(8_000, 0));
    sim.idle(20);
    assert_eq!(sim.console.home().selected(), 0);

    // Time at the start of the next tick.
    let time = |s: &Sim| s.tick * FRAME_US;
    let held_at = time(&sim);
    sim.input(Ev::Stick(30_000, 0));
    sim.step_tick();
    assert_eq!(sim.console.home().selected(), 1, "acts once at once");
    // Every tick that ends before the repeat delay: still one move.
    while time(&sim) + FRAME_US <= held_at + REPEAT_DELAY_US {
        sim.step_tick();
    }
    assert_eq!(sim.console.home().selected(), 1);
    // Then one more after the delay, and one every REPEAT_US (each at
    // the first frame at or after its time: at most one frame late).
    while time(&sim) <= held_at + REPEAT_DELAY_US + 3 * REPEAT_US + FRAME_US {
        sim.step_tick();
    }
    assert_eq!(sim.console.home().selected(), 1 + 1 + 3);
    // Let go: no more moves.
    sim.input(Ev::Stick(0, 0));
    sim.idle(30);
    assert_eq!(sim.console.home().selected(), 5);
}

#[test]
fn held_button_repeats_too() {
    let mut sim = Sim::new(Some(NINE));
    sim.boot();
    sim.input(Ev::Press(RIGHT));
    sim.idle(ticks(REPEAT_DELAY_US + 2 * REPEAT_US) + 1);
    sim.input(Ev::Release(RIGHT));
    sim.idle(5);
    assert_eq!(sim.console.home().selected(), 4);
}

#[test]
fn touch_selects_then_starts() {
    let mut sim = Sim::new(Some(FOUR));
    sim.boot();
    let x = tile_x(&sim, 2) + 10;
    sim.tap_at(x as u16, (TILE_Y + 10) as u16);
    assert_eq!(sim.console.home().selected(), 2);
    assert!(sim.console.notices().is_empty());
    // A tap on the selected tile starts it: not possible yet, so a note.
    sim.idle(10);
    let x = tile_x(&sim, 2) + 10;
    sim.tap_at(x as u16, (TILE_Y + 10) as u16);
    assert_eq!(sim.console.notices().current(), Some(msg::NO_START));
    // A tap on empty space does nothing.
    sim.tap_at(80, 100);
    assert_eq!(sim.console.home().selected(), 2);
}

#[test]
fn row_scrolls_smoothly_when_more_games_than_fit() {
    let mut sim = Sim::new(Some(NINE));
    sim.boot();
    assert_eq!(sim.console.home().count(), 9);
    for _ in 0..3 {
        sim.tap_button(RIGHT);
    }
    // The fourth tile did not fit: the row scrolls over several frames.
    let mut steps = Vec::new();
    while sim.console.home().moving() {
        steps.push(sim.console.home().scroll());
        sim.step_tick();
        assert!(steps.len() < 20, "the row never stops");
    }
    assert!(steps.len() >= 2, "scrolled in one jump: {steps:?}");
    // The selected tile is now fully on screen, inside the margins.
    let x = tile_x(&sim, 3);
    assert!(
        x >= TILE_MARGIN && x + TILE as i16 + TILE_MARGIN <= 160,
        "x = {x}"
    );
    // To the end and back to the start.
    for _ in 0..10 {
        sim.tap_button(RIGHT);
    }
    sim.idle(15);
    assert_eq!(sim.console.home().selected(), 8);
    let x = tile_x(&sim, 8);
    assert!(x + TILE as i16 + TILE_MARGIN <= 160);
    for _ in 0..10 {
        sim.tap_button(LEFT);
    }
    sim.idle(15);
    assert_eq!(sim.console.home().scroll(), 0);
}

// ---- Card -------------------------------------------------------------------

#[test]
fn no_card_then_a_card() {
    let mut sim = Sim::new(None);
    sim.boot();
    assert_eq!(sim.console.home().card(), Card::None);
    assert_eq!(sim.console.home().count(), 0);
    // Moving and starting do nothing.
    sim.tap_button(RIGHT);
    sim.tap_button(SOUTH);
    assert!(sim.console.notices().is_empty());
    // A card goes in: found at the next look.
    sim.board.files.files = FOUR;
    sim.input(Ev::Card(true));
    sim.idle(ticks(theme::CARD_CHECK_US) + 1);
    assert_eq!(sim.console.home().card(), Card::Games);
    assert_eq!(sim.console.notices().current(), Some(msg::CARD_IN));
    // And out again.
    sim.idle(ticks(notice_time()) + 2);
    sim.input(Ev::Card(false));
    sim.idle(ticks(theme::CARD_CHECK_US) + 1);
    assert_eq!(sim.console.home().card(), Card::None);
    assert_eq!(sim.console.notices().current(), Some(msg::CARD_OUT));
}

#[test]
fn empty_card() {
    let mut sim = Sim::new(Some(EMPTY));
    sim.boot();
    assert_eq!(sim.console.home().card(), Card::Empty);
    assert_eq!(sim.console.home().count(), 0);
}

// ---- Notifications ----------------------------------------------------------

fn notice_time() -> u64 {
    2 * NOTE_SLIDE_US + NOTE_SHOW_US
}

#[test]
fn notifications_queue_and_leave() {
    let mut sim = Sim::new(Some(FOUR));
    sim.boot();
    // Two in a row: the pad goes, and a game is started.
    sim.input(Ev::Pad(false));
    sim.step_tick();
    sim.input(Ev::Pad(true));
    sim.step_tick();
    sim.tap_button(SOUTH);
    assert_eq!(sim.console.notices().len(), 3);
    assert_eq!(sim.console.notices().current(), Some(msg::PAD_OFF));
    // Resting on screen: an orange panel.
    sim.idle(ticks(NOTE_SLIDE_US) + 1);
    assert_eq!(sim.pixel(80, (NOTE_Y + 1) as u16), WARN);
    // Then the next one, and the next.
    sim.idle(ticks(notice_time()));
    assert_eq!(sim.console.notices().current(), Some(msg::PAD_ON));
    sim.idle(ticks(NOTE_SLIDE_US) + 1);
    assert_eq!(sim.pixel(80, (NOTE_Y + 1) as u16), OK);
    sim.idle(ticks(notice_time()));
    assert_eq!(sim.console.notices().current(), Some(msg::NO_START));
    sim.idle(ticks(notice_time()) + 1);
    assert!(sim.console.notices().is_empty());
    // Gone from the picture too.
    assert_ne!(sim.pixel(80, (NOTE_Y + 1) as u16), WARN);
}

#[test]
fn the_queue_has_a_fixed_size() {
    let mut sim = Sim::new(Some(FOUR));
    for i in 0..console_core::notify::MAX_NOTICES {
        assert!(sim.console.notify("HELLO", 0), "{i}");
    }
    assert!(!sim.console.notify("ONE TOO MANY", 0));
    sim.boot();
    assert_eq!(sim.console.notices().current(), Some("HELLO"));
}

// ---- Idle and timing --------------------------------------------------------

#[test]
fn idle_home_sends_nothing_and_sleeps() {
    let mut sim = Sim::new(Some(FOUR));
    sim.boot();
    sim.idle(10);
    let presents = sim.board.screen.presents;
    let from = sim.ticks.len();
    sim.idle(300); // ten seconds
    assert_eq!(
        sim.board.screen.presents, presents,
        "idle frames sent something"
    );
    let steps: u32 = sim.ticks[from..].iter().map(|t| t.steps).sum();
    // Woken only for the card checks (every 2 s): about 5 steps, not 300.
    assert!(steps <= 6, "{steps} steps in 10 idle seconds");
    assert!(matches!(
        sim.console.step(&mut *sim.board),
        Wake::InputOr(_)
    ));
    // The bar's clock changes after a minute: one small send.
    sim.idle(ticks(60_000_000));
    assert_eq!(sim.board.screen.presents, presents + 1);
    assert!(sim.board.screen.last_report.pixels < 40 * 20);
}

#[test]
fn early_calls_do_nothing() {
    let mut sim = Sim::new(Some(FOUR));
    let wake = sim.console.step(&mut *sim.board);
    assert_eq!(wake, Wake::At(BOOT_FRAME));
    assert_eq!(sim.board.log.last(), "console: started");
    sim.board.clock.advance(1_000);
    assert_eq!(sim.console.step(&mut *sim.board), wake);
    assert_eq!(sim.console.frames(), 1);
}

/// Bytes sent per frame in typical moments (2 bytes per panel pixel plus
/// 11 per area). Printed for the README; the bounds catch big slips.
#[test]
fn bytes_sent_in_typical_frames() {
    let mut sim = Sim::new(Some(FOUR));
    sim.boot();
    sim.idle(10);
    let max_bytes =
        |s: &Sim, from: usize| s.ticks[from..].iter().map(|t| t.bytes()).max().unwrap_or(0);
    let sum_bytes = |s: &Sim, from: usize| s.ticks[from..].iter().map(|t| t.bytes()).sum::<u32>();
    let full = 320 * 240 * 2 + 11;

    let from = sim.ticks.len();
    sim.idle(30);
    let idle = sum_bytes(&sim, from);

    let from = sim.ticks.len();
    sim.tap_button(RIGHT);
    sim.idle(5);
    let moved = sum_bytes(&sim, from);
    let moved_frames = sim.ticks[from..].iter().filter(|t| t.presents > 0).count();

    let from = sim.ticks.len();
    sim.notify(msg::NO_START);
    sim.idle(ticks(NOTE_SLIDE_US) + 1);
    let slide_max = max_bytes(&sim, from);
    let slide_sum = sum_bytes(&sim, from);

    let mut nine = Sim::new(Some(NINE));
    nine.boot();
    nine.idle(10);
    for _ in 0..2 {
        nine.tap_button(RIGHT);
    }
    nine.idle(5);
    let from = nine.ticks.len();
    nine.tap_button(RIGHT);
    nine.idle(15);
    let scroll_max = max_bytes(&nine, from);
    let scroll_sum = sum_bytes(&nine, from);

    println!("full screen        {full} bytes");
    println!("idle home (1 s)    {idle} bytes");
    println!("move selection     {moved} bytes in all, {moved_frames} frame(s)");
    println!("notification slide {slide_max} bytes per frame at most, {slide_sum} in all");
    println!("scroll by one tile {scroll_max} bytes per frame at most, {scroll_sum} in all");
    assert_eq!(idle, 0);
    assert!(moved < full / 4, "{moved}");
    assert!(slide_max < full / 8, "{slide_max}");
    assert!(scroll_max < full / 2, "{scroll_max}");
}

// ---- Short presses (step 3) ---------------------------------------------------
//
// A press (or a tap) that starts and ends between two frames is seen by
// no frame: the button is up again when the console looks. The board
// notes it (`take_pressed`, `take_tap`), and the console counts it once.
// These failed before the console read those notes.

#[test]
fn a_press_shorter_than_a_frame_still_moves_the_selection() {
    let mut sim = Sim::new(Some(FOUR));
    sim.boot();
    sim.idle(2);
    assert_eq!(sim.console.home().selected(), 0);
    sim.input(Ev::Quick(RIGHT));
    sim.step_tick();
    assert_eq!(sim.console.home().selected(), 1);
    // Counted once: no key repeat follows, the button is up.
    sim.idle(ticks(REPEAT_DELAY_US + 4 * REPEAT_US));
    assert_eq!(sim.console.home().selected(), 1);
    // The board's BOOT button, pressed that briefly, confirms.
    sim.input(Ev::QuickBoot);
    sim.step_tick();
    assert_eq!(
        sim.console.notices().current(),
        Some(msg::NO_START),
        "a short BOOT press is OK on home"
    );
}

#[test]
fn a_tap_shorter_than_a_frame_still_chooses_a_tile() {
    let mut sim = Sim::new(Some(FOUR));
    sim.boot();
    sim.idle(2);
    let x = tile_x(&sim, 2) + 10;
    sim.input(Ev::QuickTouch(x as u16, (TILE_Y + 10) as u16));
    sim.step_tick();
    assert_eq!(sim.console.home().selected(), 2);
    // A short press also skips the boot screen.
    let mut sim = Sim::new(Some(FOUR));
    sim.idle(3);
    sim.input(Ev::Quick(SOUTH));
    sim.step_tick();
    assert_eq!(sim.console.page(), Page::Home);
}

#[test]
fn a_board_that_notes_nothing_works_as_before() {
    // The notes stay empty: a press held for one frame works as always.
    let mut sim = Sim::new(Some(FOUR));
    sim.boot();
    sim.idle(2);
    sim.tap_button(RIGHT);
    assert_eq!(sim.console.home().selected(), 1);
    sim.tap_button(LEFT);
    assert_eq!(sim.console.home().selected(), 0);
}
