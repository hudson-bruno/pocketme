//! What each picture costs the screen: how many pixels change, and how
//! many the console would send (its areas, by `console_video::diff`, at
//! most 16 a picture). Run with `--nocapture` to see the table.
//!
//! The calm default (Still view) changes a few hundred pixels a picture;
//! the heavy mode (Follow view) moves the whole picture with the craft.
//!
//! Counted at 30 pictures a second (RATE 30, set by `Run::new`), though
//! the game starts at 60: the README's table is at 30.

mod common;

use common::{Cost, Run, pilot};
use console_video::{H, W};
use game_kit::button;
use lander::app::{Scene, View};
use lander::craft::MAX_SPEED;
use lander::fixed::PX;

const FULL: u32 = (W * H) as u32;

#[derive(Default)]
struct Sum {
    pictures: u32,
    changed: u64,
    sent: u64,
    worst_changed: u32,
    worst_sent: u32,
    worst_areas: usize,
    full: u32,
}

impl Sum {
    fn add(&mut self, c: Cost) {
        self.pictures += 1;
        self.changed += u64::from(c.changed);
        self.sent += u64::from(c.sent);
        self.worst_changed = self.worst_changed.max(c.changed);
        self.worst_sent = self.worst_sent.max(c.sent);
        self.worst_areas = self.worst_areas.max(c.areas);
        if c.sent == FULL {
            self.full += 1;
        }
        assert!(c.areas <= 16);
    }

    fn mean_sent(&self) -> u64 {
        self.sent / u64::from(self.pictures.max(1))
    }

    fn show(&self, what: &str) {
        eprintln!(
            "{what:<34} pictures {:5}  changed: mean {:5} worst {:5}  sent: mean {:5} worst {:5}  areas worst {:2}  whole screen {:4}",
            self.pictures,
            self.changed / u64::from(self.pictures.max(1)),
            self.worst_changed,
            self.mean_sent(),
            self.worst_sent,
            self.worst_areas,
            self.full,
        );
    }
}

/// One autopilot flight on `level` with `view`: the cost of each picture
/// from the first flight picture to the landing.
fn flight(level: u16, view: View) -> (Sum, Scene) {
    let mut r = Run::new();
    r.app.opts.level = level;
    r.app.opts.view = view;
    r.play();
    r.call(&pilot(&r.app).input()); // the first flight picture replaces the card
    let mut s = Sum::default();
    while r.app.scene == Scene::Fly && s.pictures < 4000 {
        let c = r.call(&pilot(&r.app).input());
        s.add(c);
    }
    (s, r.app.scene)
}

/// Typical play: the autopilot's flights (the last picture of each is
/// the "LANDED" card coming up).
#[test]
fn typical_flights_cost_little_in_either_view() {
    eprintln!("(one picture is {W} x {H} = {FULL} pixels)");
    let mut means = Vec::new();
    for view in [View::Still, View::Follow] {
        for level in [1, 3, 9] {
            let (s, end) = flight(level, view);
            assert_eq!(end, Scene::Landed);
            s.show(&format!("level {level}, {view:?} view, flight"));
            means.push((view, s.mean_sent()));
            assert!(s.worst_sent < FULL * 2 / 3);
        }
    }
    // Following costs more on every level, but a typical flight is slow,
    // and most of the picture is plain sky.
    for k in 0..3 {
        assert!(means[k].1 < means[k + 3].1, "{means:?}");
    }
    for (_, m) in means {
        assert!(m * 8 < u64::from(FULL), "{m} a picture");
    }
}

/// Worst play: fast and low across a two-screen level, so the ground
/// moves under the camera every picture. Each call puts the craft back
/// `h` pixels over the ground, flying sideways at top speed.
fn skim(view: View, h: i32) -> Sum {
    let mut r = Run::new();
    r.app.opts.level = 9;
    r.app.opts.view = view;
    r.play();
    let mut s = Sum::default();
    // Towards the far side of the world.
    let dir = if r.app.craft.x / PX < 160 { 1 } else { -1 };
    for k in 0..75 {
        let c = &mut r.app.craft;
        let x = c.x / PX;
        let ground = (x - 6..=x + 6)
            .map(|x| r.app.world.ground_at(x))
            .min()
            .unwrap_or(100);
        c.y = (ground - 1 - 4 - h) * PX;
        c.vx = MAX_SPEED * dir;
        c.vy = 0;
        c.angle = 0;
        c.fuel = r.app.world.params.fuel;
        let cost = r.call(&common::hold(button::A));
        assert_eq!(r.app.scene, Scene::Fly, "call {k}");
        if k > 0 {
            s.add(cost);
        }
        if !(20..=300).contains(&(r.app.craft.x / PX)) {
            break;
        }
    }
    s
}

#[test]
fn skimming_the_ground_at_top_speed_is_the_worst_case() {
    let still = skim(View::Still, 10);
    still.show("skim, still view");
    // The still view: only the page jumps resend (most of) the screen.
    assert!(still.mean_sent() * 4 < u64::from(FULL));
    let follow = skim(View::Follow, 40);
    follow.show("skim, follow view, 40 px up");
    let zoomed = skim(View::Follow, 10);
    zoomed.show("skim, follow view zoomed, 10 px up");
    // Following low and fast: a big part of every picture changes. The
    // sky is one plain colour, so it is never the whole screen.
    assert!(follow.mean_sent() > still.mean_sent() * 3);
    assert!(
        zoomed.mean_sent() * 3 > u64::from(FULL),
        "{}",
        zoomed.mean_sent()
    );
    assert!(zoomed.worst_sent < FULL);
}

#[test]
fn the_other_screens_cost_little() {
    // The title: only the flame flickers.
    let mut r = Run::new();
    r.idle(2);
    let mut s = Sum::default();
    for _ in 0..60 {
        s.add(r.call(&common::hold(0)));
    }
    s.show("title, 2 s");
    assert!(s.worst_sent < FULL / 8);

    // A menu step on the title.
    let c = r.call(&common::press(button::DOWN));
    eprintln!(
        "title menu step: changed {} sent {} areas {}",
        c.changed, c.sent, c.areas
    );

    // The "LEVEL" card: the first picture is the whole screen.
    r.app.menu = 0;
    let c = r.call(&common::press(button::A));
    eprintln!("title -> level card: changed {} sent {}", c.changed, c.sent);

    // The crash: pieces and sparks fly.
    let mut r = Run::new();
    r.play();
    let mut s = Sum::default();
    while r.app.scene == Scene::Fly {
        r.call(&common::hold(0));
    }
    for _ in 0..90 {
        s.add(r.call(&common::hold(0)));
    }
    s.show("crash, 3 s after");
    assert!(s.worst_sent < FULL / 2);

    // The game's own pause: one picture to open it, then nothing.
    let mut r = Run::new();
    r.play();
    let open = r.call(&common::press(button::START));
    let after = r.call(&common::hold(0));
    eprintln!(
        "pause opens: changed {} sent {}; then {}",
        open.changed, open.sent, after.sent
    );
    assert_eq!(after.sent, 0);
}
