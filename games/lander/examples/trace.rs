//! Prints the test autopilot's flight on one level, twice a second: a
//! tool for tuning levels and the autopilot.
//!
//! `cargo run --offline --example trace 7` (level 7; 1 when left out).

#[path = "../tests/common/mod.rs"]
mod common;

use lander::app::Scene;
use lander::fixed::{PX, STEP};

fn main() {
    let level: u16 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(1);
    let mut r = common::Run::new();
    r.app.opts.level = level;
    r.play();
    let w = r.app.world;
    println!(
        "level {level}: pads {:?}, width {}, wind {}, gust {}, gravity {}",
        w.pads(),
        w.params.width,
        w.params.wind,
        w.params.gust,
        w.params.gravity
    );
    println!("(x, y in pixels; speeds in pixels a second; lean in steps of 1/256 turn)");
    for n in 0..4000 {
        if r.app.scene != Scene::Fly {
            println!("end: {:?} {:?} after {n} calls", r.app.scene, r.app.ending);
            return;
        }
        let p = common::pilot(&r.app);
        if n % 15 == 0 {
            let c = r.app.craft;
            println!(
                "{n:5}  x {:4} y {:4}  vx {:4} vy {:4}  lean {:4}  fuel {:6}  height {:4}  {p:?}",
                c.x / PX,
                c.y / PX,
                c.vx * 60 / PX,
                c.vy * 60 / PX,
                c.angle / STEP,
                c.fuel,
                c.height(&w)
            );
        }
        r.call(&p.input());
    }
}
