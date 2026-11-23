//! The title: its made-up race and cameras. The followed ship stays in
//! the picture right of the menu nearly all the time, no camera stands
//! on the road or in a tunnel's wall or roof, and the title writes no
//! best lap and no log line however long it runs.

mod common;

use camber::app::{COL_W, Keys, Scene};
use camber::race::Class;
use camber::scene::FOCAL;
use camber::track::{FM, ROOF, TRACKS};
use camber::tv::Shot;
use common::Driver;

/// Where the followed ship's middle is in the picture (None: behind the
/// camera).
fn on_screen(d: &Driver) -> Option<(i32, i32)> {
    let r = &d.app.race;
    let tr = r.tr();
    let sh = &r.ships[usize::from(d.app.tv.ship)];
    let (cam, _) = d.app.cam.camera(tr);
    let p = camber::scene::world_at(tr, sh.s, sh.x, sh.h + FM / 2);
    let v = cam
        .rot
        .apply([p[0] - cam.pos[0], p[1] - cam.pos[1], p[2] - cam.pos[2]]);
    (v[2] > 64).then(|| (80 + v[0] * FOCAL / v[2], 60 - v[1] * FOCAL / v[2]))
}

#[test]
fn the_cameras_keep_the_followed_ship_in_sight() {
    for track in 0..TRACKS.len() as u8 {
        for class in Class::ALL {
            let mut d = Driver::new();
            d.draw = false;
            d.app.opts.track = track;
            d.app.opts.class = class;
            d.hold(0, (0, 0));
            let races = d.app.tv.races;
            let (mut ticks, mut seen, mut right) = (0u32, 0u32, 0u32);
            let mut shots = [0u32; 4];
            let mut last = u32::MAX;
            while d.app.tv.races == races {
                d.hold(0, (0, 0));
                assert_eq!(d.app.scene, Scene::Title);
                let tv = d.app.tv;
                if tv.cuts != last {
                    last = tv.cuts;
                    shots[tv.shot as usize] += 1;
                }
                let r = &d.app.race;
                let tr = r.tr();
                let cam = &d.app.cam;
                let half = tr.half(cam.s) * 256;
                let seg = tr.seg(tr.index(cam.s));
                let name = TRACKS[usize::from(track)].name;
                // A camera beside the road stands off it; in a tunnel
                // a camera is inside it, under the roof.
                if tv.shot == Shot::Side {
                    assert!(cam.x.abs() > half + FM, "{name}: a side camera on the road");
                    assert_eq!(seg.kind & ROOF, 0, "{name}: a side camera in a tunnel");
                }
                if seg.kind & ROOF != 0 {
                    assert!(
                        cam.x.abs() < half && cam.h < FM * 5,
                        "{name}: in a tunnel's wall"
                    );
                }
                ticks += 1;
                if let Some((x, y)) = on_screen(&d)
                    && (0..160).contains(&x)
                    && (0..120).contains(&y)
                {
                    seen += 1;
                    if x >= i32::from(COL_W) {
                        right += 1;
                    }
                }
            }
            println!(
                "{} {}: {ticks} calls, ship in the picture {} %, right of the menu {} %, shots {shots:?}",
                TRACKS[usize::from(track)].name,
                class.name(),
                seen * 100 / ticks,
                right * 100 / ticks
            );
            assert!(right * 100 >= ticks * 85, "the ship out of sight too long");
            // HIGH may be left out on a track with many tunnels.
            assert!(
                shots[..3].iter().all(|&k| k > 0),
                "every kind of shot: {shots:?}"
            );
        }
    }
}

#[test]
fn the_title_writes_no_best_and_no_log() {
    let mut d = Driver::new();
    d.draw = false;
    for _ in 0..30 * 60 * 3 {
        d.call(Keys::default());
    }
    assert!(d.app.tv.races > 2, "the made-up races came round");
    assert!(d.logs.is_empty(), "{:?}", d.logs);
    assert_eq!(d.app.best, [[0; 3]; 3]);
    assert_eq!(d.app.scene, Scene::Title);
    // Game time (the race's bob and lights) does not run on the title.
    assert_eq!(d.time, 0);
}
