//! What each picture costs, per detail: the pixels that change and the
//! pixels the console would send (its own rule, `console_video::diff`,
//! at most 16 areas a picture), and the renderer's own counted work
//! (rays, ray steps, multiplications, divisions, pixels by kind).
//!
//! From these counts it prints an ESTIMATE of the time on the board:
//!
//! * drawing: counted work x cycles per item. The cycles per pixel and
//!   per ray step are instruction counts read from the chip build's
//!   disassembly (the counts are kept outside the repository), times 1.3 for
//!   memory waits and taken branches (a guess); divisions and the rest
//!   are guesses, marked so below.
//! * sending: sent pixels x 15.5 ms / 19 200, from the board measurement
//!   of a whole-screen send (about 15.5 ms).
//!
//! Nothing here was timed on the board. Run with `--nocapture` to see
//! the table; it is also written to `.local/games/3d/cost.txt`.

mod common;

use common::{Driver, autopilot, evidence};
use console_video::{Areas, Frame, H, W, WINDOW_COST_PX, diff};
use game_kit::button;
use lantern::app::Scene;
use lantern::render::{Cost, Detail};

const FULL: u32 = (W * H) as u32;

/// Cycles per item. COUNTED: instructions in the chip build's inner
/// loops. ASSUMED: the rest.
mod cycles {
    /// Counted: ~17 instructions a wall pixel (1 px columns), ~29 for a
    /// 2 px wall row (LIGHT): ~15 a pixel.
    pub const WALL_1: u64 = 17;
    pub const WALL_2: u64 = 15;
    /// Counted: ~10 a plain floor or ceiling pixel.
    pub const FLAT: u64 = 10;
    /// Counted: ~24 a textured floor or ceiling pixel (HEAVY).
    pub const FLOOR: u64 = 24;
    /// Counted: ~22 a sprite pixel (with its see-through test).
    pub const SPRITE: u64 = 22;
    /// Counted: ~85 a ray step (53 + the map look-up 22 + 10).
    pub const STEP: u64 = 85;
    /// Assumed: a 32-bit divide.
    pub const DIV: u64 = 40;
    /// Assumed: the rest of a ray (set-up, depth, texture column).
    pub const RAY: u64 = 150;
    /// Assumed: a sprite's set-up (sort, size).
    pub const SPRITE_SETUP: u64 = 500;
    /// Assumed: the status line, the frame around LIGHT, text.
    pub const OTHER: u64 = 30_000;
    /// Assumed: memory waits and taken branches, x 13/10.
    pub const STALL_NUM: u64 = 13;
    pub const STALL_DEN: u64 = 10;
    /// 240 MHz: cycles per microsecond.
    pub const PER_US: u64 = 240;
}

/// Estimated drawing time on the board, in microseconds.
fn draw_us(c: &Cost, detail: Detail) -> u64 {
    use cycles::*;
    let wall = if detail == Detail::Light {
        WALL_2
    } else {
        WALL_1
    };
    let n = u64::from(c.wall_px) * wall
        + u64::from(c.flat_px) * FLAT
        + u64::from(c.floor_px) * FLOOR
        + u64::from(c.sprite_px) * SPRITE
        + u64::from(c.steps) * STEP
        + u64::from(c.divs) * DIV
        + u64::from(c.rays) * RAY
        + u64::from(c.sprites) * SPRITE_SETUP
        + OTHER;
    n * STALL_NUM / STALL_DEN / PER_US
}

/// Estimated sending time on the board, in microseconds (15.5 ms for
/// the whole screen, measured; each area costs some set-up, which the
/// console's rule counts as WINDOW_COST_PX pixels).
fn send_us(sent: u32, areas: usize) -> u64 {
    if sent == 0 {
        return 0;
    }
    (u64::from(sent) + areas as u64 * u64::from(WINDOW_COST_PX)) * 15_500 / u64::from(FULL)
}

#[derive(Default)]
struct Sum {
    pictures: u64,
    changed: u64,
    sent: u64,
    worst_changed: u32,
    worst_sent: u32,
    worst_areas: usize,
    rays: u64,
    steps: u64,
    worst_steps: u32,
    muls: u64,
    divs: u64,
    wall: u64,
    flat: u64,
    floor: u64,
    sprite: u64,
    draw_us: u64,
    worst_draw_us: u64,
    send_us: u64,
    worst_send_us: u64,
}

impl Sum {
    fn mean(&self, v: u64) -> u64 {
        v / self.pictures.max(1)
    }

    fn line(&self, what: &str) -> String {
        format!(
            "{what:<26} pictures {:5} | changed px mean {:5} worst {:5} | sent px mean {:5} worst {:5} areas worst {:2} \
             | rays {:3} steps mean {:4} worst {:4} mul {:4} div {:4} \
             | px wall {:5} flat {:5} floor {:5} sprite {:4} \
             | ESTIMATE draw ms mean {:2}.{} worst {:2}.{}, send ms mean {:2}.{} worst {:2}.{}",
            self.pictures,
            self.mean(self.changed),
            self.worst_changed,
            self.mean(self.sent),
            self.worst_sent,
            self.worst_areas,
            self.mean(self.rays),
            self.mean(self.steps),
            self.worst_steps,
            self.mean(self.muls),
            self.mean(self.divs),
            self.mean(self.wall),
            self.mean(self.flat),
            self.mean(self.floor),
            self.mean(self.sprite),
            self.mean(self.draw_us) / 1000,
            self.mean(self.draw_us) % 1000 / 100,
            self.worst_draw_us / 1000,
            self.worst_draw_us % 1000 / 100,
            self.mean(self.send_us) / 1000,
            self.mean(self.send_us) % 1000 / 100,
            self.worst_send_us / 1000,
            self.worst_send_us % 1000 / 100,
        )
    }
}

/// The game with the console's picture rule beside it.
struct Meter {
    d: Driver,
    shown: Box<Frame>,
    sum: Sum,
    on: bool,
}

impl Meter {
    fn new(detail: Detail, level: u8) -> Meter {
        let mut d = Driver::new();
        d.app.opts.detail = detail;
        d.app.opts.level = level;
        // Counted at 30 pictures a second (the driver's call rate).
        d.app.opts.fast = false;
        d.play_from_title();
        let shown = Box::new(Frame(d.frame.0));
        Meter {
            d,
            shown,
            sum: Sum::default(),
            on: true,
        }
    }

    /// Counts the picture of the call just made.
    fn count(&mut self) {
        let mut a = Areas::new();
        diff(&self.shown, &self.d.frame, &mut a);
        let changed = (self.shown.0.iter().zip(self.d.frame.0.iter()))
            .filter(|(a, b)| a != b)
            .count() as u32;
        self.shown.0.copy_from_slice(&self.d.frame.0);
        if !self.on || self.d.app.scene != Scene::Play {
            return;
        }
        assert!(a.len() <= 16);
        let c = self.d.app.render.cost;
        let s = &mut self.sum;
        s.pictures += 1;
        s.changed += u64::from(changed);
        s.sent += u64::from(a.pixels());
        s.worst_changed = s.worst_changed.max(changed);
        s.worst_sent = s.worst_sent.max(a.pixels());
        s.worst_areas = s.worst_areas.max(a.len());
        s.rays += u64::from(c.rays);
        s.steps += u64::from(c.steps);
        s.worst_steps = s.worst_steps.max(c.steps);
        s.muls += u64::from(c.muls);
        s.divs += u64::from(c.divs);
        s.wall += u64::from(c.wall_px);
        s.flat += u64::from(c.flat_px);
        s.floor += u64::from(c.floor_px);
        s.sprite += u64::from(c.sprite_px);
        let dr = draw_us(&c, self.d.app.opts.detail);
        let se = send_us(a.pixels(), a.len());
        s.draw_us += dr;
        s.worst_draw_us = s.worst_draw_us.max(dr);
        s.send_us += se;
        s.worst_send_us = s.worst_send_us.max(se);
    }

    fn hold(&mut self, b: u32) {
        self.d.hold(b);
        self.count();
    }
}

#[test]
fn every_detail_costs_what_it_says() {
    let mut out = String::new();
    out.push_str(&format!(
        "One picture is {W} x {H} = {FULL} pixels. 30 pictures a second (RATE 30; the game starts at 60).\n\
         changed/sent/areas/rays/steps/mul/div/px: COUNTED in the simulator's game code.\n\
         draw ms / send ms: ESTIMATE for the board (see tests/cost.rs), not measured.\n\n"
    ));
    let mut walk_sent = Vec::new();
    let mut turn_sent = Vec::new();
    for detail in Detail::ALL {
        // Typical play: the autopilot through levels 1 and 2.
        let mut walk = Sum::default();
        for level in [0, 1] {
            let mut m = Meter::new(detail, level);
            // The autopilot drives the Driver itself; count after each of
            // its calls by running it one call at a time.
            for _ in 0..30 * 120 {
                let trip = autopilot(&mut m.d, true, 1);
                m.count();
                if trip.cleared || m.d.app.scene != Scene::Play {
                    break;
                }
            }
            merge(&mut walk, &m.sum);
        }
        let line = walk.line(&format!("{} walk (levels 1-2)", detail.name()));
        out.push_str(&line);
        out.push('\n');
        walk_sent.push(walk.mean(walk.sent));

        // The worst: turning on the spot at full speed (every wall
        // column moves), then walking straight.
        let mut m = Meter::new(detail, 2);
        m.on = false;
        m.hold(0);
        m.on = true;
        for _ in 0..60 {
            m.hold(button::RIGHT);
        }
        let line = m.sum.line(&format!("{} turning", detail.name()));
        out.push_str(&line);
        out.push('\n');
        turn_sent.push(m.sum.mean(m.sum.sent));
        assert!(m.sum.worst_steps < 3000);

        // Standing still: only the lamp, the creatures, the status line.
        let mut m = Meter::new(detail, 0);
        m.on = false;
        m.hold(0);
        m.on = true;
        for _ in 0..60 {
            m.hold(0);
        }
        let line = m.sum.line(&format!("{} standing", detail.name()));
        out.push_str(&line);
        out.push('\n');
        let view_px = (detail.view().w * detail.view().h) as u64;
        assert!(
            m.sum.mean(m.sum.sent) * 4 < view_px,
            "{}: standing sends {} a picture",
            detail.name(),
            m.sum.mean(m.sum.sent)
        );
    }

    // The other screens.
    let mut d = Driver::new();
    let mut shown = Box::new(Frame(d.frame.0));
    let mut step = |d: &mut Driver, what: &str, out: &mut String| {
        let mut a = Areas::new();
        diff(&shown, &d.frame, &mut a);
        shown.0.copy_from_slice(&d.frame.0);
        out.push_str(&format!(
            "{what:<26} sent px {:5} areas {:2}\n",
            a.pixels(),
            a.len()
        ));
        a.pixels()
    };
    d.hold(0);
    step(&mut d, "title, no input", &mut out);
    d.hold(button::DOWN);
    step(&mut d, "title menu step", &mut out);
    d.hold(0);
    d.app.menu = 0;
    d.hold(button::A);
    step(&mut d, "title -> play", &mut out);
    d.hold(0);
    step(&mut d, "play", &mut out);
    d.hold(button::SELECT);
    step(&mut d, "play -> map", &mut out);
    d.hold(0);
    let map_still = step(&mut d, "map, no input", &mut out);
    assert_eq!(map_still, 0);
    d.hold(button::SELECT);
    step(&mut d, "map -> play", &mut out);
    d.hold(0);
    d.hold(button::START);
    step(&mut d, "pause opens", &mut out);
    d.hold(0);
    step(&mut d, "pause, no input", &mut out);

    eprintln!("{out}");
    std::fs::write(evidence("").join("cost.txt"), &out).unwrap();

    // LIGHT sends less than NORMAL, which sends less than HEAVY, while
    // walking and while turning.
    assert!(
        walk_sent[0] < walk_sent[1] && walk_sent[1] < walk_sent[2],
        "{walk_sent:?}"
    );
    assert!(
        turn_sent[0] < turn_sent[1] && turn_sent[1] <= turn_sent[2],
        "{turn_sent:?}"
    );
    // LIGHT's view is 120 x 80: it never sends the whole screen when turning.
    assert!(turn_sent[0] < u64::from(FULL) * 2 / 3);
}

fn merge(a: &mut Sum, b: &Sum) {
    a.pictures += b.pictures;
    a.changed += b.changed;
    a.sent += b.sent;
    a.worst_changed = a.worst_changed.max(b.worst_changed);
    a.worst_sent = a.worst_sent.max(b.worst_sent);
    a.worst_areas = a.worst_areas.max(b.worst_areas);
    a.rays += b.rays;
    a.steps += b.steps;
    a.worst_steps = a.worst_steps.max(b.worst_steps);
    a.muls += b.muls;
    a.divs += b.divs;
    a.wall += b.wall;
    a.flat += b.flat;
    a.floor += b.floor;
    a.sprite += b.sprite;
    a.draw_us += b.draw_us;
    a.worst_draw_us = a.worst_draw_us.max(b.worst_draw_us);
    a.send_us += b.send_us;
    a.worst_send_us = a.worst_send_us.max(b.worst_send_us);
}
