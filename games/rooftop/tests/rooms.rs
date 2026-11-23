//! Every room is well formed and can be finished: each has a recorded
//! input script (tests/data/roomN.txt) that reaches its door with no
//! fall and no cut. Pictures of each path go to
//! .local/games/platformer/paths/.

mod common;

use common::*;
use rooftop::rooms::{self, LETTERS, ROOMS, ROWS};
use rooftop::sim::{Event, Follow};

#[test]
fn every_room_is_well_formed() {
    let mut wide = 0;
    for (i, r) in ROOMS.iter().enumerate() {
        let w = r.rows[0].len();
        assert!(w >= 20 && w % 20 == 0, "room {}: width {w}", i + 1);
        for (j, row) in r.rows.iter().enumerate() {
            assert_eq!(row.len(), w, "room {} row {j}", i + 1);
            for &t in row.iter() {
                assert!(
                    LETTERS.contains(&t),
                    "room {} row {j}: {}",
                    i + 1,
                    t as char
                );
            }
        }
        let starts = r
            .rows
            .iter()
            .flat_map(|r| r.iter())
            .filter(|&&t| t == rooms::START);
        assert_eq!(starts.count(), 1, "room {}: one start", i + 1);
        let doors = r
            .rows
            .iter()
            .flat_map(|r| r.iter())
            .filter(|&&t| t == rooms::DOOR);
        assert!(doors.count() >= 1, "room {}: a door", i + 1);
        // The start stands on something solid.
        let (c, row) = r.start();
        assert!((row as usize) < ROWS - 1);
        assert!(
            rooms::is_solid(r.at(c, row + 1)),
            "room {}: start floats",
            i + 1
        );
        assert!(
            r.name.len() <= 14,
            "room {}: name too long for the bar",
            i + 1
        );
        if r.scrolls() {
            wide += 1;
        }
    }
    assert!(wide >= 1, "at least one wide room");
}

#[test]
fn every_room_can_be_finished_by_its_script() {
    let dir = evidence("paths");
    for room in 0..ROOMS.len() {
        let script = solution(room);
        let p = play_room(room, &script, Follow::Smooth);
        save_room_path(&dir.join(format!("room{}.png", room + 1)), room, &p);
        assert!(
            p.cleared_at.is_some(),
            "room {} not finished; last place {:?}; events {:?}",
            room + 1,
            p.path.last(),
            p.events
        );
        assert!(p.events.is_empty(), "room {}: {:?}", room + 1, p.events);
        // The script ends where the door is reached (no idle tail), so the
        // full run can be made by joining them.
        let total: u32 = script.iter().map(|h| h.calls).sum();
        assert_eq!(p.cleared_at, Some(total as usize - 1), "room {}", room + 1);
        // The camera mode does not change the physics.
        let q = play_room(room, &script, Follow::Step);
        assert_eq!(q.path, p.path);
    }
}

/// A design aid, not a check: plays tests/data/try.txt in the room named
/// by TRY_ROOM (1-based) and prints where the runner went.
/// `TRY_ROOM=3 cargo test --test rooms try_a_script -- --ignored --nocapture`
#[test]
#[ignore]
fn try_a_script() {
    let room: usize = std::env::var("TRY_ROOM").unwrap().parse::<usize>().unwrap() - 1;
    let text = std::fs::read_to_string(data_dir().join("try.txt")).unwrap();
    let script = parse(&text);
    let p = play_room(room, &script, Follow::Smooth);
    for (i, pos) in p.path.iter().enumerate() {
        if i % 2 == 1 {
            println!("call {:4}: {:?}", i / 2, pos);
        }
    }
    println!("cleared at {:?}; events {:?}", p.cleared_at, p.events);
    save_room_path(&evidence("paths").join("try.png"), room, &p);
}

#[test]
fn a_script_that_walks_into_the_first_gap_falls() {
    let p = play_room(0, &parse("120 R"), Follow::Smooth);
    assert_eq!(p.events.first().map(|e| e.1), Some(Event::Fell));
    assert_eq!(p.cleared_at, None);
}
