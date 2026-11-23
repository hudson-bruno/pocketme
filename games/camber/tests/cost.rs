//! What each picture costs, for every detail and view at both rates: the
//! renderer's own counts (points turned, points projected, polygons,
//! triangles, rows, pixels painted), the pixels that change and the
//! pixels the console would send (its own rule, `console_video::diff`,
//! at most 16 areas a picture).
//!
//! The races are the autopilot's, a whole race on every track, so every
//! setting sees the same race (the settings never change the race).
//! Run with `--nocapture` to see the table; it is also written to
//! `.local/camber/cost.txt`.

mod common;

use camber::app::{Backdrop, Scene};
use camber::race::{Class, Mode, Phase};
use camber::scene::{Detail, ViewKind};
use camber::track::TRACKS;
use camber::tv::SPIN;
use common::Driver;
use console_video::{Areas, Frame, H, W, diff};

const FULL: u32 = (W * H) as u32;

#[derive(Default, Clone)]
struct Sum {
    pictures: u64,
    changed: u64,
    sent: u64,
    worst_changed: u32,
    worst_sent: u32,
    verts: u64,
    worst_verts: u32,
    projected: u64,
    polys: u64,
    clipped: u64,
    tris: u64,
    worst_tris: u32,
    rows: u64,
    pixels: u64,
    worst_pixels: u32,
    dropped: u32,
    areas: u64,
    worst_areas: u32,
    all_sent: Vec<u32>,
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
        self.verts += b.verts;
        self.worst_verts = self.worst_verts.max(b.worst_verts);
        self.projected += b.projected;
        self.polys += b.polys;
        self.clipped += b.clipped;
        self.tris += b.tris;
        self.worst_tris = self.worst_tris.max(b.worst_tris);
        self.rows += b.rows;
        self.pixels += b.pixels;
        self.worst_pixels = self.worst_pixels.max(b.worst_pixels);
        self.dropped += b.dropped;
        self.areas += b.areas;
        self.worst_areas = self.worst_areas.max(b.worst_areas);
        self.all_sent.extend_from_slice(&b.all_sent);
    }
}

/// ESTIMATE of the time to send a picture with two cores (the owner's
/// build): the second core sends while the first draws the next one; a
/// whole picture (19,200 game pixels) takes about 14 ms on the board, and
/// a send is taken here as that share of it.
fn send_us(px: u32) -> u64 {
    u64::from(px) * 14_000 / u64::from(FULL)
}

/// ESTIMATE of pictures a second with two cores: each picture takes the
/// longer of its step (`draw_us`) and the previous picture's send, but at
/// least the period of the chosen rate.
fn fps(s: &Sum, fast: bool, draw_us: u64) -> String {
    let period: u64 = if fast { 16_667 } else { 33_333 };
    let total: u64 = s
        .all_sent
        .iter()
        .map(|&px| send_us(px).max(draw_us).max(period))
        .sum();
    let tenths = s.all_sent.len() as u64 * 10_000_000 / total.max(1);
    format!("{}.{}", tenths / 10, tenths % 10)
}

/// A whole race on track `t` with this setting, counting every racing
/// picture.
fn measure(t: u8, detail: Detail, view: ViewKind, fast: bool) -> Sum {
    let mut d = Driver::new();
    d.set(detail, view, fast);
    d.race(t, Class::Brisk, Mode::Race);
    let mut shown = Box::new(Frame(d.frame.0));
    let mut s = Sum::default();
    for _ in 0..60 * 300 {
        if d.app.race.phase == Phase::Done {
            break;
        }
        d.auto();
        count(&mut s, &mut shown, &d);
    }
    assert_eq!(d.app.race.phase, Phase::Done, "the race did not end");
    s
}

/// The title on track `t` at `detail` for one whole made-up race
/// (BRISK), counting every picture (menu column and backdrop).
fn measure_title(t: u8, detail: Detail, backdrop: Backdrop) -> Sum {
    let mut d = Driver::new();
    d.set(detail, ViewKind::Chase, false);
    d.app.opts.track = t;
    d.app.opts.backdrop = backdrop;
    d.hold(0, (0, 0));
    let mut shown = Box::new(Frame(d.frame.0));
    let mut s = Sum::default();
    let races = d.app.tv.races;
    // RACE: a whole made-up race; SHIP: one whole turn of the ship.
    let mut calls = 0;
    while match backdrop {
        Backdrop::Race => d.app.tv.races == races,
        Backdrop::Ship => calls < SPIN / 2,
    } {
        d.hold(0, (0, 0));
        calls += 1;
        assert_eq!(d.app.scene, Scene::Title);
        count(&mut s, &mut shown, &d);
    }
    s
}

/// Adds the picture the driver just drew to `s`; `shown` is the last.
fn count(s: &mut Sum, shown: &mut Frame, d: &Driver) {
    let mut a = Areas::new();
    diff(shown, &d.frame, &mut a);
    assert!(a.len() <= 16);
    let changed = shown
        .0
        .iter()
        .zip(d.frame.0.iter())
        .filter(|(a, b)| a != b)
        .count() as u32;
    shown.0.copy_from_slice(&d.frame.0);
    let r = d.app.stats;
    s.pictures += 1;
    s.changed += u64::from(changed);
    s.sent += u64::from(a.pixels());
    s.worst_changed = s.worst_changed.max(changed);
    s.worst_sent = s.worst_sent.max(a.pixels());
    s.all_sent.push(a.pixels());
    s.verts += u64::from(r.verts);
    s.worst_verts = s.worst_verts.max(r.verts);
    s.projected += u64::from(r.projected);
    s.polys += u64::from(r.polys);
    s.clipped += u64::from(r.clipped);
    s.tris += u64::from(r.tris);
    s.worst_tris = s.worst_tris.max(r.tris);
    s.rows += u64::from(r.rows);
    s.pixels += u64::from(r.pixels);
    s.worst_pixels = s.worst_pixels.max(r.pixels);
    s.dropped += r.dropped;
    s.areas += a.len() as u64;
    s.worst_areas = s.worst_areas.max(a.len() as u32);
}

fn row(name: &str, track: &str, s: &Sum) -> String {
    format!(
        "{name:<18} {track:<8} {:5}  {:5} / {:5}  {:5} {:5} {:4}  {:4} / {:4}  {:5}  {:6} / {:6}   {:6} / {:6}   {:6} / {:6}  {:2}.{} / {:2}\n",
        s.pictures,
        s.mean(s.verts),
        s.worst_verts,
        s.mean(s.projected),
        s.mean(s.polys),
        s.mean(s.clipped),
        s.mean(s.tris),
        s.worst_tris,
        s.mean(s.rows),
        s.mean(s.pixels),
        s.worst_pixels,
        s.mean(s.changed),
        s.worst_changed,
        s.mean(s.sent),
        s.worst_sent,
        s.mean(s.areas * 10) / 10,
        s.mean(s.areas * 10) % 10,
        s.worst_areas,
    )
}

/// The title against a race, detail by detail: the title's made-up race
/// is filmed at the player's DETAIL and must cost no more a picture than
/// a race at that detail (the view CHASE, the default). The column on
/// the left stands still, so fewer pixels are sent.
#[test]
fn the_title_costs_no_more_than_a_race() {
    let mut out = String::from(
        "The title (RACE: a whole made-up race, BRISK; SHIP: one whole turn of the ship) against a race (the autopilot, BRISK, CHASE, 30 a second), MEASURED ON THE PC.\n\
         areas: what console_video::diff sends, mean / worst (at most 16).\n\n\
         setting            track     pics  verts mean/worst  proj  polys clip  tris mean/worst  rows   pixels mean/worst   changed mean/worst  sent mean/worst  areas\n",
    );
    let mut sums = Vec::new();
    for detail in Detail::ALL {
        let (mut titles, mut race) = ([Sum::default(), Sum::default()], Sum::default());
        for t in 0..TRACKS.len() as u8 {
            let name = TRACKS[usize::from(t)].name;
            for (k, backdrop) in Backdrop::ALL.into_iter().enumerate() {
                let a = measure_title(t, detail, backdrop);
                assert_eq!(a.dropped, 0, "model triangles dropped");
                let what = format!("{:<5} T-{}", detail.name(), backdrop.name());
                out.push_str(&row(&what, name, &a));
                titles[k].add(&a);
            }
            let b = measure(t, detail, ViewKind::Chase, false);
            out.push_str(&row(&format!("{:<5} RACE", detail.name()), name, &b));
            race.add(&b);
        }
        for (k, backdrop) in Backdrop::ALL.into_iter().enumerate() {
            let what = format!("{:<5} T-{}", detail.name(), backdrop.name());
            out.push_str(&row(&what, "all", &titles[k]));
        }
        out.push_str(&row(&format!("{:<5} RACE", detail.name()), "all", &race));
        out.push('\n');
        sums.push((detail, titles, race));
    }
    eprintln!("{out}");
    if let Some(path) = std::env::var_os("CAMBER_TITLE_COST_OUT") {
        std::fs::write(path, &out).unwrap();
    }
    for (detail, [film, ship], race) in &sums {
        let name = detail.name();
        // RACE: the renderer's work a picture, on average, is no more than
        // a race's, nor the most pixels painted in one picture; the pixels
        // sent are fewer.
        assert!(
            film.worst_pixels <= race.worst_pixels,
            "{name}: the title's worst picture paints more: {} > {}",
            film.worst_pixels,
            race.worst_pixels
        );
        for (what, t, r) in [
            ("points turned", film.verts, race.verts),
            ("triangles", film.tris, race.tris),
            ("pixels painted", film.pixels, race.pixels),
            ("pixels sent", film.sent, race.sent),
        ] {
            assert!(
                film.mean(t) <= race.mean(r),
                "{name}: the title paints more {what} a picture: {} > {}",
                film.mean(t),
                race.mean(r)
            );
        }
        // SHIP: one still camera on one close ship, nearly the same
        // picture each time. Every one of its pictures is no more work
        // than a race's worst one (its average is a few per cent over a
        // race's average: the ship is big), and it sends far fewer pixels.
        for (what, t, r) in [
            ("points turned", ship.worst_verts, race.worst_verts),
            ("triangles", ship.worst_tris, race.worst_tris),
            ("pixels painted", ship.worst_pixels, race.worst_pixels),
        ] {
            assert!(t <= r, "{name}: a SHIP picture is more {what}: {t} > {r}");
        }
        assert!(ship.mean(ship.sent) * 4 <= race.mean(race.sent));
    }
}

#[test]
fn what_every_setting_costs() {
    let mut out = String::new();
    out.push_str(&format!(
        "One picture is {W} x {H} = {FULL} game pixels. Races: the autopilot, a whole race (BRISK) on each track.\n\
         pixels: painted by the game (the backdrop's {FULL} included; a pixel painted twice counts twice).\n\
         changed / sent: pixels that differ from the last picture / what console_video::diff sends (MEASURED on the PC).\n\n"
    ));
    out.push_str("setting            track    pics  verts mean/worst  proj  polys clip  tris mean/worst  rows   pixels mean/worst   changed mean/worst  sent mean/worst\n");
    let mut low30 = Sum::default();
    for fast in [false, true] {
        for view in ViewKind::ALL {
            for detail in Detail::ALL {
                if fast && view != ViewKind::Chase {
                    continue;
                }
                let mut all = Sum::default();
                for t in 0..TRACKS.len() as u8 {
                    let s = measure(t, detail, view, fast);
                    assert_eq!(s.dropped, 0, "model triangles dropped");
                    let line = format!(
                        "{:<5} {:<5} {:<3}    {:<8} {:5}  {:5} / {:5}  {:5} {:5} {:4}  {:4} / {:4}  {:5}  {:6} / {:6}   {:6} / {:6}   {:6} / {:6}\n",
                        detail.name(),
                        view.name(),
                        if fast { 60 } else { 30 },
                        TRACKS[usize::from(t)].name,
                        s.pictures,
                        s.mean(s.verts),
                        s.worst_verts,
                        s.mean(s.projected),
                        s.mean(s.polys),
                        s.mean(s.clipped),
                        s.mean(s.tris),
                        s.worst_tris,
                        s.mean(s.rows),
                        s.mean(s.pixels),
                        s.worst_pixels,
                        s.mean(s.changed),
                        s.worst_changed,
                        s.mean(s.sent),
                        s.worst_sent,
                    );
                    out.push_str(&line);
                    all.add(&s);
                }
                if detail == Detail::Mid && view == ViewKind::Chase && !fast {
                    low30 = all.clone();
                }
                out.push_str(&format!(
                    "  all tracks: changed {:.1} % of the picture on average; ESTIMATE pictures a second on the board, two cores (step 10 / 20 ms): {} / {}\n",
                    all.changed as f64 * 100.0 / (all.pictures as f64 * f64::from(FULL)),
                    fps(&all, fast, 10_000),
                    fps(&all, fast, 20_000),
                ));
            }
        }
    }
    eprintln!("{out}");
    // CAMBER_COST_OUT: another file for the table (to keep several runs).
    let path = std::env::var_os("CAMBER_COST_OUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/camber/cost.txt")
        });
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, &out).unwrap();
    assert!(low30.pictures > 0);
}
