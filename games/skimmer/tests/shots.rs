//! Pictures of every state, to look at: `.local/airhockey/shots/*.png`
//! (each game pixel 2 x 2, the size of the real screen).

mod common;

use common::{Driver, evidence, save_frame};
use game_kit::button;
use skimmer::app::{LINE, Scene, TITLE_Y, Who};
use skimmer::fixed::PX;
use skimmer::table::{CX, CY, Length, PUCK_MAX, Phase, STALL_TICKS, XL};

fn shot(d: &Driver, name: &str) {
    save_frame(&evidence("shots").join(format!("{name}.png")), &d.frame, 2);
}

/// Plays until `f` holds (at most `max` calls).
fn until(d: &mut Driver, max: u32, f: impl Fn(&Driver) -> bool) {
    for _ in 0..max {
        if f(d) {
            return;
        }
        assert!(d.play_call());
    }
    panic!("never got there: {:?} {:?}", d.app.scene, d.app.m.phase);
}

#[test]
fn pictures_of_every_state() {
    // The title, no pads: CORAL on touch, TEAL the computer.
    let d = Driver::new([false; 4]);
    shot(&d, "01-title-no-pads");
    // Two pads, TEAL's line chosen.
    let mut d = Driver::new([true, true, false, false]);
    assert!(d.press(0, button::DOWN));
    assert!(d.press(0, button::DOWN));
    shot(&d, "02-title-two-pads");

    // A match: the serve.
    let mut d = Driver::new([true, false, false, false]);
    d.start();
    d.idle(10);
    shot(&d, "03-serve");
    // Play with a fast puck (its trail).
    until(&mut d, 3000, |d| {
        let m = &d.app.m;
        let clear =
            (0..2).all(|i| skimmer::fixed::len(m.puck.x - m.m[i].x, m.puck.y - m.m[i].y) > 24 * PX);
        m.phase == Phase::Live && m.puck.speed() > PUCK_MAX * 3 / 4 && clear
    });
    shot(&d, "04-play-fast-puck");
    // A hard hit's sparks.
    until(&mut d, 6000, |d| d.app.m.spark.is_some_and(|s| s.2 >= 7));
    shot(&d, "05-hit-sparks");
    // A goal: the slot flashes, the score too.
    until(
        &mut d,
        6000,
        |d| matches!(d.app.m.phase, Phase::Goal { t, .. } if t < 76),
    );
    shot(&d, "06-goal");
    // The pause menu.
    assert!(d.press(0, button::START));
    shot(&d, "07-pause");
    assert!(d.press(0, button::START));
    // The end of the match.
    until(&mut d, 30 * 60 * 20, |d| d.app.scene == Scene::End);
    shot(&d, "08-winner");

    // TOO SLOW: the puck left alone on one half, the countdown dots first.
    let mut d = Driver::new([false, false, false, false]);
    d.app.opts.side = [Who::Touch, Who::Computer];
    d.start();
    d.until_live();
    d.app.m.puck.x = XL + 30 * PX;
    d.app.m.puck.y = CY + 30 * PX;
    d.app.m.puck.vx = 0;
    d.app.m.puck.vy = 0;
    d.app.m.half = 0;
    d.app.m.half_ticks = STALL_TICKS - 100;
    // The computer stays home (it waits for the puck on its half).
    d.idle(5);
    shot(&d, "09-stall-countdown");
    for _ in 0..200 {
        if matches!(d.app.m.phase, Phase::Stall { .. }) {
            break;
        }
        d.idle(1);
    }
    d.idle(2);
    shot(&d, "10-too-slow");

    // The WILD table, two against two, four pads.
    let mut d = Driver::new([true; 4]);
    d.app.opts.four = true;
    d.app.opts.wild = true;
    d.start();
    until(&mut d, 3000, |d| {
        d.app.m.phase == Phase::Live && d.app.m.rally > 40
    });
    shot(&d, "11-wild-two-against-two");

    // The touch screen: the finger ahead of the mallet.
    let mut d = Driver::new([false; 4]);
    d.start();
    d.until_live();
    let t = d.touch_to(Some((60, 30)));
    assert!(d.call_with([0; 4], t));
    shot(&d, "12-touch-finger");

    // A timed match: the clock, then the golden goal and a draw.
    let mut d = Driver::new([false; 4]);
    d.app.opts.side = [Who::Computer, Who::Computer];
    d.app.opts.length = Length::Timed;
    d.start();
    until(&mut d, 600, |d| d.app.m.phase == Phase::Live);
    d.idle(100);
    shot(&d, "13-timed-clock");
    // Level at the end: golden goal.
    d.app.m.score = [2, 2];
    d.app.m.clock = 2;
    d.app.m.puck.x = CX - 30 * PX;
    d.idle(2);
    assert!(d.app.m.overtime);
    d.app.m.phase = Phase::Serve { side: 0, t: 30 };
    d.app.m.puck.x = CX - 34 * PX;
    d.app.m.puck.y = CY;
    d.idle(1);
    shot(&d, "14-golden-goal");
    // Nobody scores in the golden goal: a draw.
    d.app.m.clock = 2;
    d.app.m.phase = Phase::Live;
    d.app.m.puck.x = CX;
    d.idle(3);
    assert_eq!(d.app.scene, Scene::End);
    shot(&d, "15-draw");

    // The 30-frame title with every choice changed.
    let mut d = Driver::new([true, false, false, false]);
    for k in 3..=7 {
        assert!(d.tap(120, TITLE_Y + k * LINE + 2));
    }
    shot(&d, "16-title-changed");
}
