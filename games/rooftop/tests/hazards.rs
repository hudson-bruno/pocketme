//! Every hazard sends the runner back to the room's start at once:
//! glass pointing each way, and the drop off the bottom. The hit box is
//! smaller than the picture, so a near miss is a miss.

mod common;

use common::*;
use rooftop::rooms::{ROOMS, Room};
use rooftop::sim::*;
use rooftop::{Note, Scene};

fn run_pad(run: i32) -> Pad {
    Pad {
        run,
        dx: run.signum() as i8,
        ..Pad::default()
    }
}

/// Plays `pads` until an event; returns it, the tick and the world.
fn until_event(room: &'static Room, pads: impl Fn(u32) -> Pad) -> (Option<Event>, u32, World) {
    let mut w = World::new(room, Follow::Smooth);
    for t in 0..600 {
        if let Some(e) = w.tick(&pads(t)) {
            return (Some(e), t, w);
        }
    }
    (None, 600, w)
}

/// Back at the start, standing, with the dash, no speed; the puff shows
/// where it happened.
fn assert_back_at_start(w: &World) {
    let fresh = Runner::at_start(w.room);
    assert_eq!(w.runner, fresh);
    assert_eq!(w.runner.dashes, 1);
    assert_eq!(w.puff.2, PUFF_LIFE, "the puff starts");
    assert!(w.ghosts.iter().all(|g| g.life == 0));
}

#[test]
fn glass_on_the_floor_cuts() {
    let (e, t, w) = until_event(&GLASS_FLOOR, |_| run_pad(RUN_MAX));
    assert_eq!(e, Some(Event::Cut), "after {t} ticks");
    assert_back_at_start(&w);
}

#[test]
fn glass_under_a_duct_cuts_a_full_jump_but_not_a_short_one() {
    // A full jump straight up.
    let (e, _, w) = until_event(&GLASS_CEILING, |t| Pad {
        jump: t == 0,
        jump_held: true,
        ..Pad::default()
    });
    assert_eq!(e, Some(Event::Cut));
    assert_back_at_start(&w);
    // A tap: a short hop stays clear.
    let (e, _, _) = until_event(&GLASS_CEILING, |t| Pad {
        jump: t == 0,
        jump_held: t < 4,
        ..Pad::default()
    });
    assert_eq!(e, None);
}

#[test]
fn glass_on_a_wall_cuts_from_either_side() {
    let (e, _, w) = until_event(&GLASS_ON_WALL_RIGHT, |_| run_pad(RUN_MAX));
    assert_eq!(e, Some(Event::Cut));
    assert_back_at_start(&w);
    let (e, _, w) = until_event(&GLASS_ON_WALL_LEFT, |_| run_pad(-RUN_MAX));
    assert_eq!(e, Some(Event::Cut));
    assert_back_at_start(&w);
}

#[test]
fn a_dash_into_glass_cuts_too() {
    let (e, _, w) = until_event(&GLASS_FLOOR, |t| Pad {
        dx: 1,
        dash: t == 0,
        ..Pad::default()
    });
    assert_eq!(e, Some(Event::Cut));
    assert_back_at_start(&w);
}

#[test]
fn falling_off_the_bottom_sends_the_runner_back() {
    let (e, _, w) = until_event(&ROOMS[0], |_| run_pad(RUN_MAX));
    assert_eq!(e, Some(Event::Fell));
    assert_back_at_start(&w);
    // The puff is drawn just inside the picture.
    assert!(w.puff.1 < w.room.h_px());
}

#[test]
fn the_hit_box_is_fair() {
    // GLASS_FLOOR: the glass tile is column 4 (x 32..40), its shards
    // x 33..39, y 84..88. The runner's box is 5 wide, the hurt box the
    // middle 3.
    let room = &GLASS_FLOOR;
    let y = 80;
    // Two pixels of the box over the glass tile: no cut.
    assert!(!touches_glass(room, 29, y));
    // Three: the hurt box reaches the shards.
    assert!(touches_glass(room, 30, y));
    // From the other side the same.
    assert!(!touches_glass(room, 38, y));
    assert!(touches_glass(room, 37, y));
    // Standing right over the shards' tips, one pixel up: no cut either
    // (the hurt box stops 1 pixel above the feet).
    assert!(!touches_glass(room, 34, y - 3));
    assert!(touches_glass(room, 34, y - 2));
}

#[test]
fn falls_and_cuts_count_as_retries_in_the_game() {
    let mut d = Driver::new();
    d.call(keys(rooftop_a(), rooftop_a()));
    assert_eq!(d.game.scene, Scene::Play);
    d.play(&parse("40 R"));
    assert_eq!(d.game.run.retries, 1);
    assert!(d.notes().contains(&Note::Fell { room: 1 }));
    // No waiting: the next call already runs from the start.
    let x0 = Runner::at_start(&ROOMS[0]).x;
    d.play(&parse("1 R"));
    assert!(d.game.run.world.runner.x > x0);
    // Room 2's first glass.
    let mut d = Driver::new();
    d.game.from = 1;
    d.call(keys(rooftop_a(), rooftop_a()));
    d.play(&parse("40 R"));
    assert!(d.notes().contains(&Note::Cut { room: 2 }));
    assert!(d.game.run.retries >= 1);
    assert_eq!(d.game.run.scores[1].retries, d.game.run.retries);
}

fn rooftop_a() -> u32 {
    game_kit::button::A
}
