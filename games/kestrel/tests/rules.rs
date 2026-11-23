//! The rules of a mission (`src/world.rs`, `src/course.rs`), one at a
//! time: the craft is put where a rule applies and moved one tick.

use kestrel::course::{CORRIDOR_X, CORRIDOR_Y, Kind, MISSIONS, Mission, Thing, Things};
use kestrel::poly::M8;
use kestrel::world::{
    CELL_POINTS, CELL_S10, Event, GATE_POINTS, HULL_MAX, HULL_POINTS, HURT_TICKS, HZ, Pad,
    ROLL_COOL, ROLL_TICKS, SECOND_POINTS, SHIP_R, State, World, cruise_speed,
};

/// Puts the craft at world position (x, y, z) (1/256 m), standing still
/// sideways.
fn put(w: &mut World, x: i32, y: i32, z: i32) {
    w.ship.x = x << 4;
    w.ship.y = y << 4;
    w.ship.z = z << 4;
    w.ship.vx = 0;
    w.ship.vy = 0;
}

fn events(w: &mut World) -> Vec<Event> {
    let mut v = Vec::new();
    while let Some(e) = w.take_event() {
        v.push(e);
    }
    v
}

/// One tick with the craft placed so it crosses gate `n` this tick at
/// (dx, dy) from the gate's middle.
fn cross_gate(w: &mut World, n: u8, dx: i32, dy: i32) -> Vec<Event> {
    events(w);
    w.next_gate = n;
    let g = w.m().gate(n, w.ticks + 1);
    put(w, g.x + dx, g.y + dy, g.z - 4);
    w.tick(&Pad::default());
    events(w)
}

/// The first thing of this kind on mission `m`'s course at tick `t`.
fn find(m: usize, t: u32, want: impl Fn(&Thing) -> bool) -> Thing {
    let mission = &MISSIONS[m];
    let last = Mission::cell_of(mission.dock_z());
    for c in 0..=last {
        let mut list = Things::new();
        mission.things(c, t, &mut list);
        if let Some(t) = list.as_slice().iter().find(|t| want(t)) {
            return *t;
        }
    }
    panic!("nothing found on mission {m}");
}

#[test]
fn a_mission_starts_with_a_full_hull_and_the_missions_clock() {
    for (n, m) in MISSIONS.iter().enumerate() {
        let w = World::new(n);
        assert_eq!(w.state, State::Flying);
        assert_eq!(w.hull, HULL_MAX);
        assert_eq!(w.clock_s10(), m.start_s10);
        assert_eq!(w.score, 0);
        assert_eq!(w.ship.speed, cruise_speed(m));
    }
}

#[test]
fn through_a_gate_gives_points_and_time() {
    let mut w = World::new(0);
    let clock = w.clock;
    let ev = cross_gate(&mut w, 0, 0, 0);
    assert!(ev.contains(&Event::Gate(0)), "{ev:?}");
    assert_eq!(w.score, GATE_POINTS);
    assert_eq!(w.clock, clock - 1 + MISSIONS[0].bonus_s10 * HZ / 10);
    assert_eq!(w.next_gate, 1);
    assert_eq!(w.gate_ok, 1);
    assert_eq!(w.tally.passed, 1);
    assert_eq!(w.hull, HULL_MAX);
    // Near the hole's edge still counts.
    let g = w.m().gate(1, w.ticks + 1);
    let edge = g.inner() - SHIP_R / 2 - 8;
    let ev = cross_gate(&mut w, 1, 0, edge);
    assert!(ev.contains(&Event::Gate(1)), "{ev:?}");
}

#[test]
fn past_a_gate_counts_as_missed_and_into_its_ring_hurts() {
    let mut w = World::new(0);
    let g = w.m().gate(0, 1);
    // Wide of the ring, on the side with more room.
    let side = if g.x > 0 { -1 } else { 1 };
    let ev = cross_gate(&mut w, 0, side * (g.outer() + SHIP_R + M8), 0);
    assert!(ev.contains(&Event::Missed(0)), "{ev:?}");
    assert_eq!(
        (w.score, w.hull, w.tally.missed, w.next_gate),
        (0, HULL_MAX, 1, 1)
    );
    // Into gate 1's ring.
    let g = w.m().gate(1, w.ticks + 1);
    let speed = w.ship.speed;
    let ev = cross_gate(&mut w, 1, (g.inner() + g.outer()) / 2, 0);
    assert!(ev.contains(&Event::Ring(1)), "{ev:?}");
    assert_eq!(w.hull, HULL_MAX - 1);
    assert_eq!(w.tally.missed, 2);
    assert_eq!(w.ship.hurt, HURT_TICKS);
    assert!(w.ship.speed < speed * 2 / 3, "a hit slows the craft");
    assert_eq!(w.gate_ok, 0);
}

#[test]
fn a_rock_hurts_once_then_the_hull_is_safe_for_a_while_and_three_hits_lose() {
    let rock = find(0, 1, |t| matches!(t.kind, Kind::Rock(_)));
    let mut w = World::new(0);
    let mut hits = Vec::new();
    for tick in 0..400u32 {
        put(&mut w, rock.x, rock.y, rock.z);
        w.ship.speed = 0;
        w.next_gate = w.m().gates; // no gate crossing here
        w.tick(&Pad::default());
        for e in events(&mut w) {
            if e == Event::Hit {
                hits.push(tick);
            }
        }
        if w.state != State::Flying {
            break;
        }
    }
    assert_eq!(
        hits,
        vec![0, u32::from(HURT_TICKS), 2 * u32::from(HURT_TICKS)]
    );
    assert_eq!(w.state, State::Lost);
    assert_eq!(w.hull, 0);
    // Lost is the end: more ticks change nothing.
    let t = w.ticks;
    w.tick(&Pad::default());
    assert_eq!(w.ticks, t);
}

#[test]
fn wrecks_and_pods_hurt_too() {
    for want in [Kind::Wreck, Kind::Pod] {
        let t = 50u32;
        let thing = find(1, t, |th| th.kind == want);
        let mut w = World::new(1);
        w.ticks = t - 1;
        w.next_gate = w.m().gates;
        put(&mut w, thing.x, thing.y, thing.z);
        w.ship.speed = 0;
        w.tick(&Pad::default());
        let ev = events(&mut w);
        assert!(ev.contains(&Event::Hit), "{want:?}: {ev:?}");
        assert_eq!(w.hull, HULL_MAX - 1);
    }
}

#[test]
fn a_time_cell_is_taken_once() {
    let cell = find(0, 1, |t| t.kind == Kind::Cell);
    let mut w = World::new(0);
    w.next_gate = w.m().gates;
    let clock = w.clock;
    for _ in 0..5 {
        put(&mut w, cell.x, cell.y, cell.z);
        w.ship.speed = 0;
        w.tick(&Pad::default());
    }
    assert_eq!(w.tally.cells, 1);
    assert_eq!(w.score, CELL_POINTS);
    assert_eq!(w.clock, clock - 5 + CELL_S10 * HZ / 10);
    assert!(w.is_taken(cell.id));
    assert_eq!(w.hull, HULL_MAX, "a time cell never hurts");
}

#[test]
fn the_clock_running_out_ends_the_mission() {
    let mut w = World::new(2);
    w.clock = 2;
    w.tick(&Pad::default());
    assert_eq!(w.state, State::Flying);
    w.tick(&Pad::default());
    assert_eq!(w.state, State::OutOfTime);
    assert!(events(&mut w).contains(&Event::OutOfTime));
    assert_eq!(w.clock_s10(), 0);
}

#[test]
fn through_the_dock_delivers_with_a_bonus_for_time_and_hull() {
    let mut w = World::new(0);
    w.score = 700;
    w.hull = 2;
    w.clock = 9 * HZ + 30; // 9.5 s
    let n = w.m().gates;
    let ev = cross_gate(&mut w, n, M8, -M8);
    assert!(ev.contains(&Event::Delivered), "{ev:?}");
    assert_eq!(w.state, State::Delivered);
    // 9.5 s left after the tick rounds up to 9.5; whole seconds count.
    assert_eq!(w.score, 700 + 9 * SECOND_POINTS + 2 * HULL_POINTS);
}

#[test]
fn past_the_dock_is_a_missed_delivery() {
    let mut w = World::new(1);
    let n = w.m().gates;
    let ev = cross_gate(&mut w, n, 8 * M8, 0);
    assert!(ev.contains(&Event::MissedDock), "{ev:?}");
    assert_eq!(w.state, State::MissedDock);
    assert_eq!(w.score, 0);
}

#[test]
fn the_craft_stays_in_the_corridor() {
    for steer in [(256, 256), (-256, -256), (256, -256)] {
        let mut w = World::new(0);
        for _ in 0..600 {
            w.ship.z = 0; // stay at the start: nothing there
            w.tick(&Pad {
                steer,
                ..Pad::default()
            });
            assert!(w.ship.x.abs() <= CORRIDOR_X * 16);
            assert!(w.ship.y.abs() <= CORRIDOR_Y * 16);
        }
        assert_eq!(w.ship.x.abs(), CORRIDOR_X * 16, "{steer:?}");
        assert_eq!(w.ship.y.abs(), CORRIDOR_Y * 16, "{steer:?}");
        // Right is +x, up is +y.
        assert_eq!(w.ship.x.signum(), steer.0.signum());
        assert_eq!(w.ship.y.signum(), steer.1.signum());
    }
}

#[test]
fn a_barrel_roll_throws_the_craft_aside_then_must_wait() {
    let mut w = World::new(0);
    let roll = Pad {
        roll: 1,
        ..Pad::default()
    };
    let mut starts = Vec::new();
    for t in 0..200u32 {
        w.ship.z = 0;
        let before = w.tally.rolls;
        w.tick(&roll);
        if w.tally.rolls != before {
            starts.push(t);
        }
        if t == 15 {
            assert!(w.ship.vx > 0, "a right roll moves right");
        }
    }
    let gap = u32::from(ROLL_TICKS) + u32::from(ROLL_COOL);
    assert_eq!(
        starts[..3],
        [0, gap, 2 * gap],
        "a roll held down repeats after the wait"
    );
    // A left roll moves left; a roll asked for during a roll is ignored.
    let mut w = World::new(0);
    w.tick(&Pad {
        roll: -1,
        ..Pad::default()
    });
    for _ in 0..10 {
        w.ship.z = 0;
        w.tick(&Pad {
            roll: 1,
            ..Pad::default()
        });
    }
    assert!(w.ship.vx < 0);
    assert_eq!(w.tally.rolls, 1);
}

#[test]
fn boost_and_brake_set_the_speed_within_limits() {
    let m = &MISSIONS[0];
    let base = cruise_speed(m);
    for (throttle, lo, hi) in [(256, 147, 150), (0, 99, 101), (-256, 53, 57)] {
        let mut w = World::new(0);
        for _ in 0..600 {
            w.ship.z = 0;
            w.tick(&Pad {
                throttle,
                ..Pad::default()
            });
        }
        let pct = w.ship.speed * 100 / base;
        assert!(
            (lo..=hi).contains(&pct),
            "throttle {throttle}: {pct}% of cruise"
        );
    }
    // Far outside the range is the same as the end of the range.
    let mut a = World::new(0);
    let mut b = World::new(0);
    for _ in 0..100 {
        a.ship.z = 0;
        b.ship.z = 0;
        a.tick(&Pad {
            throttle: 256,
            steer: (256, 256),
            ..Pad::default()
        });
        b.tick(&Pad {
            throttle: 99_999,
            steer: (99_999, 99_999),
            ..Pad::default()
        });
    }
    assert_eq!(a.ship, b.ship);
}

#[test]
fn the_same_inputs_give_the_same_flight() {
    let run = |m: usize| {
        let mut w = World::new(m);
        let mut log = Vec::new();
        for t in 0..1500i32 {
            let pad = Pad {
                steer: ((t * 37 % 512) - 256, (t * 91 % 512) - 256),
                throttle: (t * 13 % 512) - 256,
                roll: if t % 97 == 0 { 1 } else { 0 },
            };
            w.tick(&pad);
            log.extend(events(&mut w));
        }
        (w.ship, w.score, w.clock, w.hull, w.state, log)
    };
    for m in 0..3 {
        assert_eq!(run(m), run(m));
    }
    // And the course itself is the same every time.
    let (mut a, mut b) = (Things::new(), Things::new());
    for c in 0..150 {
        MISSIONS[2].things(c, 777, &mut a);
        MISSIONS[2].things(c, 777, &mut b);
        assert_eq!(a.as_slice(), b.as_slice());
    }
}

#[test]
fn nothing_blocks_a_gates_hole() {
    for (n, m) in MISSIONS.iter().enumerate() {
        for t in (0..3000u32).step_by(37) {
            for i in 0..=m.gates {
                let g = m.gate(i, t);
                let free = g.inner() - SHIP_R;
                let c = Mission::cell_of(g.z);
                for slice in c - 2..=c + 2 {
                    let mut list = Things::new();
                    m.things(slice, t, &mut list);
                    for th in list.as_slice().iter().filter(|th| th.hurts()) {
                        for dz in [-3 * M8, -M8, 0, M8, 3 * M8] {
                            for (ax, ay) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                                let (x, y) = (g.x + ax * free / 2, g.y + ay * free / 2);
                                assert!(
                                    !th.touches(x, y, g.z + dz, SHIP_R),
                                    "mission {n} gate {i} at tick {t}: {th:?} in the hole"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn gates_stay_inside_the_corridor() {
    for m in &MISSIONS {
        for t in (0..5000u32).step_by(13) {
            for i in 0..=m.gates {
                let g = m.gate(i, t);
                assert!(
                    g.x.abs() + g.inner() <= CORRIDOR_X + 3 * M8,
                    "{} gate {i}",
                    m.name
                );
                assert!(
                    g.y.abs() + g.inner() <= CORRIDOR_Y + 3 * M8,
                    "{} gate {i}",
                    m.name
                );
            }
        }
    }
}

#[test]
fn the_missions_get_harder() {
    for w in MISSIONS.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        assert!(b.gates > a.gates);
        assert!(b.cruise > a.cruise);
        assert!(b.rocks > a.rocks);
        assert!(b.start_s10 < a.start_s10);
        assert!(b.bonus_s10 <= a.bonus_s10);
        assert!(b.gate_scale < a.gate_scale);
        // Spare time at cruise speed: clock plus all gate bonuses, less
        // the time the course takes. It shrinks.
        let spare = |m: &Mission| {
            let course_s10 = m.dock_z() / M8 * 10 / m.cruise;
            m.start_s10 as i32 + i32::from(m.gates) * m.bonus_s10 as i32 - course_s10
        };
        assert!(
            spare(b) < spare(a),
            "{} {} vs {} {}",
            a.name,
            spare(a),
            b.name,
            spare(b)
        );
        assert!(spare(b) > 0, "{} cannot be made at cruise speed", b.name);
    }
    // Only the later missions have drift, wrecks, pods, sliding gates.
    assert_eq!(
        (
            MISSIONS[0].drift,
            MISSIONS[0].wreck_every,
            MISSIONS[0].gate_slide
        ),
        (0, 0, 0)
    );
    assert!(MISSIONS[1].wreck_every > 0 && MISSIONS[1].pod_every > 0);
    assert!(MISSIONS[2].gate_slide > 0);
}

#[test]
fn a_mission_end_is_logged_by_the_name_debug_gives() {
    // The log line used `{state:?}` before it was written without
    // `core::fmt`; the golden record holds those words.
    for s in [
        State::Flying,
        State::Delivered,
        State::OutOfTime,
        State::Lost,
        State::MissedDock,
    ] {
        assert_eq!(s.name(), format!("{s:?}"));
    }
}
