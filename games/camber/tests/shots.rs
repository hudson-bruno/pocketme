//! Pictures for people to look at (written to `.local/camber/shots`;
//! ignored by default: `cargo test --test shots -- --ignored`). Each
//! picture is also checked to use only the game's palette numbers.

mod common;

use camber::race::{Class, Mode, Phase};
use camber::scene::{Detail, ViewKind};
use camber::track::{GAP, ROOF, SEG_BITS, TRACKS};
use common::{Driver, check_palette, save_frame, save_strip, shots};
use game_kit::button;

const SCALE: usize = 3;

fn save(d: &Driver, name: &str) {
    check_palette(&d.frame, name);
    let theme = d.app.theme();
    save_frame(&shots().join(format!("{name}.png")), &d.frame, theme, SCALE);
    // The raw picture and its colours, for the tile maker (a script
    // kept outside the repository).
    let pal: Vec<u8> = common::palette(theme).iter().flatten().copied().collect();
    std::fs::write(shots().join(format!("{name}.raw")), d.frame.0).unwrap();
    std::fs::write(shots().join(format!("{name}.pal")), pal).unwrap();
}

/// The same moment from each view (CHASE, FAR, NOSE), the camera set
/// straight into place; the race does not move.
fn save_views(d: &mut Driver, name: &str) {
    let (cam, view) = (d.app.cam, d.app.opts.view);
    let mut frames = Vec::new();
    for v in ViewKind::ALL {
        d.app.opts.view = v;
        d.app.cam.set = false;
        d.app.cam.follow(d.app.race.tr(), &d.app.race.ships[0], v);
        d.app.draw(&mut d.frame, d.time);
        save(d, &format!("{name}_{}", v.name().to_lowercase()));
        frames.push((*d.frame).clone());
    }
    // The three side by side, to look at in one go.
    let theme = d.app.theme();
    save_strip(&shots().join(format!("{name}.png")), &frames, theme, 2);
    d.app.cam = cam;
    d.app.opts.view = view;
    d.app.draw(&mut d.frame, d.time);
}

#[test]
#[ignore]
fn take_shots() {
    // The title and the grid.
    let mut d = Driver::new();
    d.call(Default::default());
    save(&d, "title");
    for t in 0..TRACKS.len() as u8 {
        let mut d = Driver::new();
        d.set(Detail::High, ViewKind::Chase, false);
        d.race(t, Class::Brisk, Mode::Race);
        let name = TRACKS[usize::from(t)].name.to_lowercase();
        for _ in 0..10 {
            d.hold(0, (0, 0));
        }
        save_views(&mut d, &format!("{name}_grid"));
        // Racing: a straight, a banked turn, rivals close, a tunnel, a
        // crest, mid-jump; each from every view.
        let tr = d.app.race.tr();
        let mut done = [false; 6];
        let names = ["straight", "turn", "close", "tunnel", "crest", "jump"];
        for _ in 0..30 * 120 {
            d.draw = false;
            d.auto();
            let r = &d.app.race;
            let sh = &r.ships[0];
            let seg = *tr.seg(tr.index(sh.s));
            let next = *tr.seg(tr.index(sh.s + (1 << SEG_BITS) * 2));
            let ahead = *tr.seg(tr.index(sh.s + (1 << SEG_BITS) * 5));
            let near_rivals = r.ships[1..]
                .iter()
                .filter(|o| {
                    let ds = o.s - sh.s;
                    ds > 0 && ds < 65_536 * 25
                })
                .count();
            let want = [
                r.phase == Phase::Run
                    && seg.bank == 0
                    && next.bank == 0
                    && seg.kind & ROOF == 0
                    && sh.lap >= 2,
                r.clock() > 300 && seg.bank.abs() > 1500 && seg.kind & ROOF == 0,
                near_rivals >= 3 && r.phase == Phase::Run,
                seg.kind & ROOF != 0 && next.kind & ROOF != 0,
                seg.pitch > 400 && i32::from(ahead.pitch) < i32::from(seg.pitch) - 1_500,
                sh.h > 65_536 * 3 || (seg.kind & GAP != 0 && sh.fall == 0),
            ];
            for k in 0..6 {
                if want[k] && !done[k] {
                    done[k] = true;
                    d.draw = true;
                    d.auto();
                    save_views(&mut d, &format!("{name}_{}", names[k]));
                }
            }
            if d.app.race.phase == Phase::Done {
                break;
            }
        }
        d.draw = true;
        d.set(Detail::Low, ViewKind::Chase, false);
        d.auto();
        save(&d, &format!("{name}_low"));
    }
    // Pause and results.
    let mut d = Driver::new();
    d.race(0, Class::Wild, Mode::Race);
    for _ in 0..200 {
        d.auto();
    }
    d.tap(button::START);
    save(&d, "pause");
    d.tap(button::START);
    d.draw = false;
    while !d.app.results() {
        d.auto();
    }
    d.draw = true;
    d.auto();
    save(&d, "results");
    // Consecutive frames through a turn (60 a second).
    let mut d = Driver::new();
    d.set(Detail::Mid, ViewKind::Chase, true);
    d.race(1, Class::Brisk, Mode::Race);
    d.draw = false;
    let tr = d.app.race.tr();
    loop {
        d.auto();
        let sh = &d.app.race.ships[0];
        let seg = tr.seg(tr.index(sh.s));
        if d.app.race.phase == Phase::Run && seg.bank.abs() > 1500 {
            break;
        }
    }
    d.draw = true;
    let mut frames = Vec::new();
    for _ in 0..6 {
        d.auto();
        frames.push((*d.frame).clone());
    }
    save_strip(&shots().join("turn_frames.png"), &frames, 1, 2);
}
