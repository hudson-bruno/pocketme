//! What each picture costs, for every setting (detail x view x picture
//! rate): the pixels that change, the pixels the console would send
//! (its own rule, `console_video::diff`, at most 16 areas a picture),
//! and the renderer's own counts (objects, faces, triangles, pixels
//! filled).
//!
//! The flights are the autopilot's, through all three missions, so
//! every setting sees exactly the same flight (the settings never change
//! the world).
//!
//! The rule of thumb for 30 pictures a second on the board (from the
//! LANTERN measurements): about 6,000 sent game pixels a picture or
//! fewer. LOW, CHASE, 30 (the default until 2026-11-23, now HIGH,
//! CHASE, 60) must stay at 6,000 on average and 8,000 at worst. Nothing here was timed on the board. Run with
//! `--nocapture` to see the table; it is also written to
//! `.local/next/ship/cost.txt`.

mod common;

use common::{Driver, buttons_of, evidence, plan, stick_of};
use console_video::{Areas, Frame, H, W, diff};
use game_kit::button;
use kestrel::app::Scene;
use kestrel::course::MISSIONS;
use kestrel::scene::{Detail, Eye};

const FULL: u32 = (W * H) as u32;

#[derive(Default, Clone)]
struct Sum {
    pictures: u64,
    changed: u64,
    sent: u64,
    worst_changed: u32,
    worst_sent: u32,
    worst_areas: usize,
    /// Pictures sending more than 6,000 and more than 11,000 pixels.
    over_6k: u64,
    over_11k: u64,
    tris: u64,
    worst_tris: u32,
    objects: u64,
    verts: u64,
    worst_verts: u32,
    filled: u64,
    worst_filled: u32,
    dropped: u32,
}

impl Sum {
    fn mean(&self, v: u64) -> u64 {
        v / self.pictures.max(1)
    }

    fn add(&mut self, b: &Sum) {
        self.pictures += b.pictures;
        self.changed += b.changed;
        self.sent += b.sent;
        self.worst_changed = self.worst_changed.max(b.worst_changed);
        self.worst_sent = self.worst_sent.max(b.worst_sent);
        self.worst_areas = self.worst_areas.max(b.worst_areas);
        self.over_6k += b.over_6k;
        self.over_11k += b.over_11k;
        self.tris += b.tris;
        self.worst_tris = self.worst_tris.max(b.worst_tris);
        self.objects += b.objects;
        self.verts += b.verts;
        self.worst_verts = self.worst_verts.max(b.worst_verts);
        self.filled += b.filled;
        self.worst_filled = self.worst_filled.max(b.worst_filled);
        self.dropped += b.dropped;
    }

    fn line(&self, what: &str) -> String {
        format!(
            "{what:<18} pictures {:5} | sent px mean {:5} worst {:5} areas worst {:2} | >6000 {:4.1}% >11000 {:4.1}% \
             | changed px mean {:5} worst {:5} | objects {:3} verts mean {:3} worst {:3} | tris mean {:3} worst {:3} | filled px mean {:5} worst {:5} | dropped {}",
            self.pictures,
            self.mean(self.sent),
            self.worst_sent,
            self.worst_areas,
            pct(self.over_6k, self.pictures),
            pct(self.over_11k, self.pictures),
            self.mean(self.changed),
            self.worst_changed,
            self.mean(self.objects),
            self.mean(self.verts),
            self.worst_verts,
            self.mean(self.tris),
            self.worst_tris,
            self.mean(self.filled),
            self.worst_filled,
            self.dropped,
        )
    }
}

fn pct(a: u64, b: u64) -> f64 {
    // Only for the printed table (the game has no floating point).
    a as f64 * 100.0 / b.max(1) as f64
}

/// Flies one mission with the autopilot at this setting, counting every
/// picture while flying.
fn measure(mission: u8, detail: Detail, eye: Eye, fast: bool) -> Sum {
    let mut d = Driver::new();
    d.app.opts.mission = mission;
    d.app.opts.detail = detail;
    d.app.opts.eye = eye;
    d.app.opts.fast = fast;
    if fast {
        d.call_us = 16_667;
    }
    d.play_from_title();
    let mut shown = Box::new(Frame(d.frame.0));
    let mut s = Sum::default();
    for _ in 0..60 * 180 {
        if d.app.scene != Scene::Play {
            break;
        }
        let p = plan(&d.app.world);
        d.hold_stick(buttons_of(&p), stick_of(p.steer));
        let mut a = Areas::new();
        diff(&shown, &d.frame, &mut a);
        let changed = (shown.0.iter().zip(d.frame.0.iter()))
            .filter(|(a, b)| a != b)
            .count() as u32;
        shown.0.copy_from_slice(&d.frame.0);
        if d.app.scene != Scene::Play {
            break;
        }
        assert!(a.len() <= 16);
        let r = d.app.render.stats;
        if std::env::var("KESTREL_COST_DEBUG").is_ok()
            && a.pixels() > 6_000
            && !fast
            && detail == Detail::Low
        {
            let w = &d.app.world;
            let g = w.m().gate(w.next_gate, w.ticks);
            let prev = w.m().gate(w.next_gate.saturating_sub(1), w.ticks);
            eprintln!(
                "m{} {:?} {:?} t {} sent {} areas {} next gate in {} m, last gate {} m behind, hurt {}, tris {}, note {:?}",
                mission + 1,
                detail,
                eye,
                w.ticks,
                a.pixels(),
                a.len(),
                (g.z - (w.ship.z >> 4)) / 256,
                ((w.ship.z >> 4) - prev.z) / 256,
                w.ship.hurt,
                r.tris,
                d.app.note.map(|n| n.0)
            );
            common::save_frame(
                &evidence("debug").join(format!("m{}-{:?}-{}.png", mission + 1, eye, w.ticks)),
                &d.frame,
                3,
            );
        }
        s.pictures += 1;
        s.changed += u64::from(changed);
        s.sent += u64::from(a.pixels());
        s.worst_changed = s.worst_changed.max(changed);
        s.worst_sent = s.worst_sent.max(a.pixels());
        s.worst_areas = s.worst_areas.max(a.len());
        s.over_6k += u64::from(a.pixels() > 6_000);
        s.over_11k += u64::from(a.pixels() > 11_000);
        s.tris += u64::from(r.tris);
        s.worst_tris = s.worst_tris.max(r.tris);
        s.objects += u64::from(r.objects);
        s.verts += u64::from(r.verts);
        s.worst_verts = s.worst_verts.max(r.verts);
        s.filled += u64::from(r.pixels);
        s.worst_filled = s.worst_filled.max(r.pixels);
        s.dropped += r.dropped;
    }
    assert_eq!(
        d.app.world.state,
        kestrel::world::State::Delivered,
        "{detail:?} {eye:?}"
    );
    s
}

#[test]
fn every_setting_costs_what_it_says() {
    let mut out = String::new();
    out.push_str(&format!(
        "One picture is {W} x {H} = {FULL} pixels. Flights: the autopilot through missions 1, 2 and 3.\n\
         sent px: what console_video::diff would send for each picture (MEASURED on the PC, game pixels).\n\
         Rule of thumb from the board (LANTERN): ~6,000 sent px a picture holds 30 a second.\n\n"
    ));
    let mut low_chase = Sum::default();
    let mut means = Vec::new();
    for fast in [false, true] {
        for detail in Detail::ALL {
            for eye in Eye::ALL {
                let mut all = Sum::default();
                for m in 0..MISSIONS.len() as u8 {
                    all.add(&measure(m, detail, eye, fast));
                }
                let what = format!(
                    "{} {} {}",
                    detail.name(),
                    eye.name(),
                    if fast { 60 } else { 30 }
                );
                let line = all.line(&what);
                println!("{line}");
                out.push_str(&line);
                out.push('\n');
                assert_eq!(all.dropped, 0, "{what}: triangles left out");
                assert!(
                    all.worst_tris <= 300,
                    "{what}: {} triangles",
                    all.worst_tris
                );
                if !fast && eye == Eye::Chase && detail == Detail::Low {
                    low_chase = all.clone();
                }
                means.push((what, all.mean(all.sent)));
            }
        }
        out.push('\n');
    }

    // The other screens.
    let mut d = Driver::new();
    let mut shown = Box::new(Frame(d.frame.0));
    let mut step = |d: &mut Driver, what: &str, out: &mut String| {
        let mut a = Areas::new();
        diff(&shown, &d.frame, &mut a);
        shown.0.copy_from_slice(&d.frame.0);
        let line = format!("{what:<26} sent px {:5} areas {:2}", a.pixels(), a.len());
        println!("{line}");
        out.push_str(&line);
        out.push('\n');
        a.pixels()
    };
    d.hold(0);
    step(&mut d, "title, no input", &mut out);
    d.hold(button::DOWN);
    step(&mut d, "title menu step", &mut out);
    d.hold(0);
    d.app.menu = 0;
    d.hold(button::A);
    step(&mut d, "title -> briefing", &mut out);
    d.idle(30);
    step(&mut d, "briefing", &mut out);
    let still = step(&mut d, "briefing, no input", &mut out);
    assert_eq!(still, 0);
    d.tap(button::A);
    step(&mut d, "briefing -> flying", &mut out);
    d.hold(button::START);
    step(&mut d, "pause opens", &mut out);
    d.hold(0);
    let still = step(&mut d, "pause, no input", &mut out);
    assert_eq!(still, 0);

    std::fs::write(evidence("").join("cost.txt"), &out).unwrap();

    // LOW, CHASE, 30 holds the rule of thumb.
    let mean = low_chase.mean(low_chase.sent);
    assert!(
        mean <= 6_000,
        "LOW CHASE 30 sends {mean} a picture on average"
    );
    assert!(
        low_chase.worst_sent <= 8_000,
        "LOW CHASE 30 sends {} at worst",
        low_chase.worst_sent
    );
}

/// A rough pilot: the autopilot's plan, but with the stick thrown hard
/// to one side now and then, barrel rolls and boost bursts. The camera
/// then slides more and the craft leans more, which changes more of
/// the picture than the smooth autopilot does. The world may be lost on
/// the way; only the pictures while flying count.
fn rough(mission: u8, detail: Detail, eye: Eye) -> Sum {
    let mut d = Driver::new();
    d.app.opts.mission = mission;
    d.app.opts.detail = detail;
    d.app.opts.eye = eye;
    d.app.opts.fast = false;
    d.play_from_title();
    let mut shown = Box::new(Frame(d.frame.0));
    let mut s = Sum::default();
    let mut seed = 0x9E37_79B9_u32 ^ u32::from(mission);
    let mut roll = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let (mut throw, mut left) = ((0i32, 0i32), 0u32);
    for call in 0..30 * 60 {
        if d.app.scene != Scene::Play {
            break;
        }
        let p = plan(&d.app.world);
        if left == 0 && roll() % 20 == 0 {
            // Throw the stick for a quarter to two thirds of a second.
            throw = (
                [-256, 256, 0][(roll() % 3) as usize],
                [-256, 256, 0][(roll() % 3) as usize],
            );
            left = 8 + roll() % 12;
        }
        let steer = if left > 0 {
            left -= 1;
            throw
        } else {
            p.steer
        };
        let mut held = buttons_of(&p);
        if call % 45 == 20 {
            held |= if roll() % 2 == 0 {
                button::L
            } else {
                button::R
            };
        }
        if (call / 30) % 4 == 1 {
            held |= button::A;
        }
        d.hold_stick(held, stick_of(steer));
        let mut a = Areas::new();
        diff(&shown, &d.frame, &mut a);
        let changed = (shown.0.iter().zip(d.frame.0.iter()))
            .filter(|(a, b)| a != b)
            .count() as u32;
        shown.0.copy_from_slice(&d.frame.0);
        if d.app.scene != Scene::Play {
            break;
        }
        let r = d.app.render.stats;
        if std::env::var("KESTREL_COST_DEBUG").is_ok()
            && a.pixels() > 6_000
            && detail == Detail::Low
        {
            let w = &d.app.world;
            eprintln!(
                "rough m{} {:?} t {} sent {} areas {} hurt {} roll {} note {:?}",
                mission + 1,
                eye,
                w.ticks,
                a.pixels(),
                a.len(),
                w.ship.hurt,
                w.ship.roll_left,
                d.app.note.map(|n| n.0)
            );
            common::save_frame(
                &evidence("debug").join(format!(
                    "rough-m{}-{:?}-{}.png",
                    mission + 1,
                    eye,
                    w.ticks
                )),
                &d.frame,
                3,
            );
        }
        s.pictures += 1;
        s.changed += u64::from(changed);
        s.sent += u64::from(a.pixels());
        s.worst_changed = s.worst_changed.max(changed);
        s.worst_sent = s.worst_sent.max(a.pixels());
        s.worst_areas = s.worst_areas.max(a.len());
        s.over_6k += u64::from(a.pixels() > 6_000);
        s.over_11k += u64::from(a.pixels() > 11_000);
        s.tris += u64::from(r.tris);
        s.worst_tris = s.worst_tris.max(r.tris);
        s.objects += u64::from(r.objects);
        s.verts += u64::from(r.verts);
        s.worst_verts = s.worst_verts.max(r.verts);
        s.filled += u64::from(r.pixels);
        s.worst_filled = s.worst_filled.max(r.pixels);
        s.dropped += r.dropped;
    }
    s
}

/// The same table for the rough pilot, at 30 pictures a second, written
/// to `.local/next/ship/cost-rough.txt`. Nothing is asserted but the
/// triangle limit: this shows how far a restless player pushes the cost.
#[test]
fn a_rough_pilot_costs_more_but_low_stays_near_the_rule() {
    let mut out = String::from(
        "Rough pilot (stick thrown hard now and then, rolls, boost bursts), 30 a second,\n\
         the first minute of each mission or until it ends. MEASURED on the PC with console_video::diff.\n\n",
    );
    for detail in Detail::ALL {
        for eye in Eye::ALL {
            let mut all = Sum::default();
            for m in 0..MISSIONS.len() as u8 {
                all.add(&rough(m, detail, eye));
            }
            let what = format!("{} {} 30", detail.name(), eye.name());
            let line = all.line(&what);
            println!("{line}");
            out.push_str(&line);
            out.push('\n');
            assert_eq!(all.dropped, 0, "{what}");
            assert!(all.worst_tris <= 300, "{what}: {}", all.worst_tris);
        }
    }
    std::fs::write(evidence("").join("cost-rough.txt"), &out).unwrap();
}
