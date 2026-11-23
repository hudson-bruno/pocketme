//! The world's rules, tick by tick: walking and walls, doors, gates and
//! switches, the lift, things to pick up, creatures and the flash.

use lantern::fixed::{ONE, angle_to, centre};
use lantern::level::LEVELS;
use lantern::world::{
    FLASH_COST, Kind, LIFE_MAX, Mood, Msg, OIL_MAX, PASS_OPEN, PLAYER_R, Pad, World, cell,
};

fn walk(w: &mut World, ticks: u32, pad: Pad) {
    for _ in 0..ticks {
        w.tick(&pad);
    }
}

const FORWARD: Pad = Pad {
    forward: 256,
    turn: 0,
    strafe: 0,
    act: false,
    flash: false,
};

/// Puts the player in the middle of a cell, looking at an angle, with
/// every creature taken out (so nothing interferes).
fn place(w: &mut World, cx: i32, cy: i32, angle: u16) {
    w.x = centre(cx);
    w.y = centre(cy);
    w.angle = angle;
    for t in w.things.iter_mut() {
        if t.kind.creature() {
            t.alive = false;
        }
    }
}

const EAST: u16 = 0;
const SOUTH: u16 = 16_384;
const WEST: u16 = 32_768;
const NORTH: u16 = 49_152;

#[test]
fn every_level_loads_with_a_start_a_crank_and_a_lift() {
    for n in 0..LEVELS.len() {
        let w = World::new(n);
        assert!(!w.solid(w.x >> 12, w.y >> 12), "level {n}: start in a wall");
        let cranks = w.things().iter().filter(|t| t.kind == Kind::Crank).count();
        assert_eq!(cranks, 1, "level {n}");
        let lifts = w.map.iter().filter(|&&c| c == cell::LIFT).count();
        assert_eq!(lifts, 1, "level {n}");
        assert!(w.tally.crystals_total > 0);
        // Levels grow.
        if n > 0 {
            let before = World::new(n - 1);
            assert!(w.w * w.h > before.w * before.h);
        }
    }
}

#[test]
fn walking_into_a_wall_stops_flush_and_never_enters_it() {
    let mut w = World::new(0);
    // Level 1 row 1: "#>..*#*...*#": from (1,1) east, a wall at x = 5.
    place(&mut w, 1, 1, EAST);
    walk(&mut w, 200, FORWARD);
    assert_eq!(
        w.x,
        5 * ONE - PLAYER_R - 1,
        "stopped flush against the wall"
    );
    assert!(w.box_free(w.x, w.y, PLAYER_R));
    // North: the outer wall.
    place(&mut w, 1, 1, NORTH);
    walk(&mut w, 100, FORWARD);
    assert_eq!(w.y, ONE + PLAYER_R);
}

#[test]
fn walking_into_a_wall_at_a_slant_slides_along_it() {
    let mut w = World::new(0);
    place(&mut w, 1, 1, EAST.wrapping_sub(2_000));
    // Facing a little north of east: the wall to the north stops y,
    // x goes on.
    let x0 = w.x;
    walk(&mut w, 30, FORWARD);
    assert!(w.x > x0 + ONE, "slid east along the wall");
    assert_eq!(w.y, ONE + PLAYER_R);
}

#[test]
fn turning_and_side_steps_move_as_set() {
    let mut w = World::new(0);
    place(&mut w, 1, 1, EAST);
    let turn = Pad {
        turn: 256,
        ..Pad::default()
    };
    walk(&mut w, 10, turn);
    assert_eq!(w.angle, 6_000);
    // A side step to the right while looking south goes west.
    let mut w = World::new(0);
    place(&mut w, 1, 4, SOUTH);
    w.x = centre(2);
    let x0 = w.x;
    walk(
        &mut w,
        10,
        Pad {
            strafe: 256,
            ..Pad::default()
        },
    );
    assert!(w.x < x0 - 1000, "{} {}", w.x, x0);
}

#[test]
fn a_door_opens_with_the_use_button_and_then_lets_the_player_through() {
    let mut w = World::new(0);
    // Level 1 row 3: "#.##.D.#m..#": a door at (5, 3); stand at (4, 3).
    place(&mut w, 4, 3, EAST);
    assert_eq!(w.at(5, 3), cell::DOOR);
    walk(&mut w, 60, FORWARD);
    assert!(w.x < 5 * ONE, "a shut door stops");
    w.tick(&Pad {
        act: true,
        ..Pad::default()
    });
    assert!(w.open_at(5, 3) > 0);
    walk(&mut w, 30, Pad::default());
    assert!(w.open_at(5, 3) >= PASS_OPEN);
    walk(&mut w, 40, FORWARD);
    assert!(w.x > 5 * ONE + ONE / 2, "through the door");
}

#[test]
fn a_gate_stays_shut_until_its_switch_is_worked() {
    let mut w = World::new(1);
    // Level 2: the switch S at (4, 8), the gate G at (12, 10).
    assert_eq!(w.at(4, 8), cell::SW_A);
    assert_eq!(w.at(12, 10), cell::GATE_A);
    place(&mut w, 11, 10, EAST);
    w.tick(&Pad {
        act: true,
        ..Pad::default()
    });
    assert_eq!(w.open_at(12, 10), 0, "a gate does not open by hand");
    assert_eq!(w.msg, Msg::Locked);
    // Now the switch, from (3, 8) looking east.
    place(&mut w, 3, 8, EAST);
    w.tick(&Pad {
        act: true,
        ..Pad::default()
    });
    assert_eq!(w.at(4, 8), cell::SW_A_ON);
    assert_eq!(w.msg, Msg::GatesOpen);
    walk(&mut w, 30, Pad::default());
    assert!(w.open_at(12, 10) >= PASS_OPEN);
}

#[test]
fn the_lift_needs_the_crank_and_then_ends_the_level() {
    let mut w = World::new(0);
    // Level 1 row 9: "#.*........L": the lift at (11, 9).
    place(&mut w, 10, 9, EAST);
    let used = Pad {
        act: true,
        ..Pad::default()
    };
    assert_eq!(w.tick(&used), None);
    assert_eq!(w.msg, Msg::NeedCrank);
    w.crank = true;
    walk(&mut w, 2, Pad::default());
    assert_eq!(w.tick(&used), Some(lantern::world::Event::Clear));
}

#[test]
fn items_are_picked_up_by_walking_over_them() {
    let mut w = World::new(0);
    // A crystal at (4, 1); walk east from (2, 1).
    place(&mut w, 2, 1, EAST);
    walk(&mut w, 40, FORWARD);
    assert_eq!(w.tally.crystals, 1);
    // Oil: refills, up to full.
    let mut w = World::new(0);
    // Level 1 row 6: "#o..#..#B#.#": oil at (1, 6).
    place(&mut w, 3, 6, WEST);
    w.oil = 1000;
    walk(&mut w, 40, FORWARD);
    assert!(w.oil > 3000, "oil {}", w.oil);
    // The crank.
    let mut w = World::new(0);
    // Level 1 row 7: "#.#.#*.#k..#": the crank at (8, 7).
    place(&mut w, 10, 7, WEST);
    walk(&mut w, 40, FORWARD);
    assert!(w.crank);
}

#[test]
fn a_flask_is_left_where_it_is_while_every_heart_is_full() {
    let mut w = World::new(1);
    // Level 2 row 14: "#*......+.#*...L": a flask at (8, 14).
    let flask = |w: &World| w.things().iter().any(|t| t.kind == Kind::Flask && t.alive);
    place(&mut w, 7, 14, EAST);
    walk(&mut w, 25, FORWARD);
    assert!(flask(&w), "full hearts: the flask stays");
    w.life = 1;
    place(&mut w, 7, 14, EAST);
    walk(&mut w, 25, FORWARD);
    assert!(!flask(&w));
    assert_eq!(w.life, 2);
}

#[test]
fn the_lamp_burns_down_and_its_light_shrinks() {
    let mut w = World::new(0);
    place(&mut w, 1, 1, EAST);
    let full = w.light();
    walk(&mut w, 60 * 60, Pad::default());
    assert_eq!(w.oil, OIL_MAX - 30 * 60, "one unit every second tick");
    assert!(w.light() < full);
    w.oil = 0;
    assert_eq!(w.light(), ONE * 5 / 2, "never below 2.5 cells");
}

#[test]
fn a_crawler_that_sees_the_player_comes_and_bites() {
    let mut w = World::new(1);
    // Level 2: a crawler at (9, 2). Stand two cells west, back turned.
    place(&mut w, 7, 2, WEST);
    let i = w
        .things()
        .iter()
        .position(|t| t.kind == Kind::Crawler)
        .unwrap();
    w.things[i].alive = true;
    w.things[i].x = centre(9);
    w.things[i].y = centre(2);
    let mut bitten = false;
    for _ in 0..400 {
        w.tick(&Pad::default());
        if w.life < LIFE_MAX {
            bitten = true;
            break;
        }
    }
    assert!(bitten, "the crawler never reached the player");
    assert_eq!(w.msg, Msg::Hurt);
    assert_eq!(w.tally.hurts, 1);
}

#[test]
fn the_flash_stuns_a_crawler_in_front_and_costs_oil() {
    let mut w = World::new(1);
    place(&mut w, 7, 2, EAST);
    let i = w
        .things()
        .iter()
        .position(|t| t.kind == Kind::Crawler)
        .unwrap();
    w.things[i].alive = true;
    w.things[i].x = centre(9);
    w.things[i].y = centre(2);
    let oil = w.oil;
    w.tick(&Pad {
        flash: true,
        ..Pad::default()
    });
    assert_eq!(w.things[i].mood, Mood::Stunned);
    assert_eq!(w.oil, oil - FLASH_COST);
    assert!(!w.things[i].harmful());
    // A second flash at once is not allowed (the lamp cools down).
    let oil = w.oil;
    w.tick(&Pad {
        flash: true,
        ..Pad::default()
    });
    assert!(w.oil >= oil - 1, "no second flash yet");
    // Stunned: it does not bite, even close up.
    w.things[i].x = w.x + 1000;
    walk(&mut w, 60, Pad::default());
    assert_eq!(w.life, LIFE_MAX);
}

/// The owner's rule: the use button with nothing to use ahead does
/// nothing at all. No flash, no oil, no cool-down, even with a creature
/// in front.
#[test]
fn the_use_button_with_nothing_to_use_ahead_does_nothing() {
    let used = Pad {
        act: true,
        ..Pad::default()
    };
    // Open floor ahead, and a crawler 2 cells away (in flash range).
    let mut w = World::new(1);
    place(&mut w, 7, 2, EAST);
    assert_eq!(w.at(8, 2), cell::FLOOR);
    let i = w
        .things()
        .iter()
        .position(|t| t.kind == Kind::Crawler)
        .unwrap();
    w.things[i].alive = true;
    w.things[i].x = centre(9);
    w.things[i].y = centre(2);
    let oil = w.oil;
    w.tick(&used);
    assert_ne!(w.things[i].mood, Mood::Stunned, "no flash");
    assert!(
        w.oil >= oil - 1,
        "only the lamp's burn: {} -> {}",
        oil,
        w.oil
    );
    assert_eq!((w.glow, w.cool, w.tally.flashes), (0, 0, 0));

    // A plain wall ahead (level 1 row 1: a wall at x = 5).
    let mut w = World::new(0);
    place(&mut w, 4, 1, EAST);
    assert_eq!(w.at(5, 1), cell::ROCK);
    // The crystal on this cell is picked up first (its own message).
    walk(&mut w, 1, Pad::default());
    let (oil, msg, map) = (w.oil, w.msg, w.map);
    for _ in 0..3 {
        w.tick(&used);
        w.tick(&Pad::default());
    }
    assert!(
        w.oil >= oil - 6,
        "only the lamp's burn: {} -> {}",
        oil,
        w.oil
    );
    assert_eq!((w.glow, w.cool, w.tally.flashes), (0, 0, 0));
    assert_eq!(w.msg, msg, "no message either");
    assert!(w.map == map, "nothing on the map changed");

    // A switch already on: also nothing.
    let mut w = World::new(1);
    place(&mut w, 3, 8, EAST);
    w.tick(&used);
    assert_eq!(w.at(4, 8), cell::SW_A_ON);
    walk(&mut w, 60, Pad::default());
    let (oil, flashes) = (w.oil, w.tally.flashes);
    w.tick(&used);
    assert!(w.oil >= oil - 1);
    assert_eq!(w.tally.flashes, flashes);
    assert_eq!(w.cool, 0);
}

#[test]
fn the_flash_misses_a_creature_behind_the_player_or_behind_a_wall() {
    let mut w = World::new(1);
    place(&mut w, 7, 2, WEST);
    let i = w
        .things()
        .iter()
        .position(|t| t.kind == Kind::Crawler)
        .unwrap();
    w.things[i].alive = true;
    w.things[i].x = centre(9);
    w.things[i].y = centre(2);
    w.tick(&Pad {
        flash: true,
        ..Pad::default()
    });
    assert_eq!(w.things[i].mood, Mood::Normal, "behind the player");
}

#[test]
fn a_moth_drinks_the_lamp_oil_and_a_flash_sends_it_away_for_good() {
    let mut w = World::new(0);
    // Level 1: a moth at (8, 3). Stand at (8, 4)... a wall; use (9, 3).
    place(&mut w, 10, 3, WEST);
    let i = w
        .things()
        .iter()
        .position(|t| t.kind == Kind::Moth)
        .unwrap();
    w.things[i].alive = true;
    w.things[i].x = centre(9);
    w.things[i].y = centre(3);
    let oil = w.oil;
    walk(&mut w, 60, Pad::default());
    assert!(
        oil - w.oil > 60 / 2 + 50,
        "the moth drank: {} -> {}",
        oil,
        w.oil
    );
    // Face it and flash.
    w.angle = angle_to(w.things[i].x - w.x, w.things[i].y - w.y);
    w.tick(&Pad {
        flash: true,
        ..Pad::default()
    });
    assert_eq!(w.things[i].mood, Mood::Fleeing);
    walk(&mut w, 60, Pad::default());
    assert!(!w.things[i].alive);
    assert_eq!(w.tally.moths_sent, 1);
}

#[test]
fn a_digger_runs_its_track_back_and_forth_and_never_through_walls() {
    let mut w = World::new(2);
    place(&mut w, 1, 1, EAST);
    // Bring the diggers back (place() took them out).
    let mut ids = Vec::new();
    for (i, t) in w.things.iter_mut().enumerate() {
        if t.kind == Kind::Digger {
            t.alive = true;
            ids.push(i);
        }
    }
    assert!(!ids.is_empty());
    let start: Vec<(i32, i32)> = ids
        .iter()
        .map(|&i| (w.things[i].x, w.things[i].y))
        .collect();
    let mut turned = vec![false; ids.len()];
    let first: Vec<u16> = ids.iter().map(|&i| w.things[i].dir).collect();
    for _ in 0..1200 {
        w.tick(&Pad::default());
        for (k, &i) in ids.iter().enumerate() {
            let t = w.things[i];
            assert!(w.box_free(t.x, t.y, t.radius()), "digger in a wall");
            if t.dir != first[k] {
                turned[k] = true;
            }
            // On its track: one of x or y never changes.
            assert!(t.x == start[k].0 || t.y == start[k].1);
        }
    }
    assert!(
        turned.iter().all(|&t| t),
        "every digger turned at a track end"
    );
}

#[test]
fn losing_every_heart_ends_the_level_as_fainted() {
    let mut w = World::new(1);
    place(&mut w, 7, 2, WEST);
    w.life = 1;
    let i = w
        .things()
        .iter()
        .position(|t| t.kind == Kind::Crawler)
        .unwrap();
    w.things[i].alive = true;
    w.things[i].x = w.x + 1500;
    w.things[i].y = w.y;
    let mut ended = None;
    for _ in 0..200 {
        if let Some(e) = w.tick(&Pad::default()) {
            ended = Some(e);
            break;
        }
    }
    assert_eq!(ended, Some(lantern::world::Event::Fainted));
}

#[test]
fn the_same_input_gives_the_same_world() {
    let run = || {
        let mut w = World::new(3);
        let mut rng = lantern::fixed::Rng(7);
        for _ in 0..3000 {
            let pad = Pad {
                forward: rng.below(513) as i32 - 256,
                turn: rng.below(513) as i32 - 256,
                strafe: rng.below(513) as i32 - 256,
                act: rng.below(30) == 0,
                flash: rng.below(60) == 0,
            };
            w.tick(&pad);
        }
        (w.x, w.y, w.angle, w.oil, w.life, w.things, w.open, w.seen)
    };
    assert!(run() == run());
}
