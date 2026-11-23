//! The area menu, the pause menu's RESTART and AREAS lines, the done
//! panel and the four areas starting (AREAS-SPEC.md, sections 1 and 2).
//!
//! With `PROTO3D_PICTURES=<folder>` the menus, the done panel and each
//! area's start are written there as PNG files (4x).

mod common;

use common::Lcg;
use game_kit::testing::TestConsole;
use game_kit::{Frame, H, W, button};
use proto3d::area::{Area, Hud};
use proto3d::draw::{PALETTE, Painter, Shown};
use proto3d::island::flag;
use proto3d::pause::{AREA_LINE0, AreaMenu, Menu, Tap};
use proto3d::play::{Detail, Play};
use proto3d::step::Pad;
use proto3d::{Input, Note, Proto};

/// The budgets (triangles a picture), LOW, MID, HIGH and MAX (MAX: the most
/// triangles of the swept ISLAND views at MAX plus 5 %, as in
/// tests/max_level.rs).
const BUDGET: [u32; 4] = [160, 430, 940, 1324];
/// ISLAND's budgets: the spec's, with LOW and MID raised by the
/// creatures' share (MEASURED, PC: the busiest LOW view went from 160 to
/// 243 triangles and MID from 372 to 474 when the burrs and tuftlings
/// came in; the estimated drawing time stays under 20 ms at both).
const ISLAND_BUDGET: [u32; 4] = [256, 500, 940, 1324];

/// The budget of `a` at level `lvl`.
fn budget(a: Area, lvl: u8) -> u32 {
    if a == Area::Island {
        ISLAND_BUDGET[usize::from(lvl)]
    } else {
        BUDGET[usize::from(lvl)]
    }
}

fn proto() -> Box<Proto> {
    Box::new(Proto::new())
}

/// One call with `b` pressed (and held), half a step of time.
fn press(p: &mut Proto, b: u32) -> Note {
    p.control(
        &Input {
            held: b,
            pressed: b,
            delta_us: 16_667,
            ..Input::default()
        },
        |_| {},
    )
}

/// `n` calls of one step each with these buttons held, nothing new.
fn hold(p: &mut Proto, held: u32, n: usize) {
    for _ in 0..n {
        p.control(
            &Input {
                held,
                delta_us: 33_333,
                ..Input::default()
            },
            |_| {},
        );
    }
}

/// A finger down at (x, y), then up there.
fn tap(p: &mut Proto, x: i16, y: i16) -> Note {
    let t = |ended| Input {
        touch: Some(Tap { x, y, ended }),
        delta_us: 16_667,
        ..Input::default()
    };
    p.control(&t(false), |_| {});
    p.control(&t(true), |_| {})
}

#[test]
fn the_area_menu_opens_at_start_and_picks() {
    let mut p = proto();
    assert!(p.areas.open);
    assert!(!p.areas.back_ok);
    assert_eq!(p.areas.sel, 0);
    // Behind it, ISLAND waits: no steps while it is open, and B does
    // nothing (there is no play to go back to).
    hold(&mut p, 0, 10);
    assert_eq!(press(&mut p, button::B), Note::None);
    assert!(p.areas.open);
    assert_eq!(p.play.steps, 0);
    // DOWN and A: MACHINES. The start menu's first choice is not logged.
    press(&mut p, button::DOWN);
    assert_eq!(press(&mut p, button::A), Note::None);
    assert!(!p.areas.open);
    assert_eq!(p.play.area, Area::Machines);
    hold(&mut p, 0, 10);
    assert_eq!(p.play.steps, 10);
    // The stick moves it too (one move per push), START takes it.
    let mut p = proto();
    for _ in 0..2 {
        p.control(
            &Input {
                left: (0, 32_767),
                ..Input::default()
            },
            |_| {},
        );
        p.control(&Input::default(), |_| {});
    }
    press(&mut p, button::START);
    assert_eq!(p.play.area, Area::Sail);
    // At start, A on ISLAND gives the first prototype's play.
    let mut p = proto();
    press(&mut p, button::A);
    assert_eq!(p.play.area, Area::Island);
    assert_eq!(p.play.fingerprint(), Play::new().fingerprint());
}

#[test]
fn a_tap_on_a_line_chooses_it() {
    let mut p = proto();
    // Outside the box at start: nothing.
    assert_eq!(tap(&mut p, 5, 5), Note::None);
    assert!(p.areas.open);
    // On STONE's line (rows 44 + 10n to 53 + 10n).
    tap(&mut p, 70, AREA_LINE0 + 30 + 4);
    assert!(!p.areas.open);
    assert_eq!(p.play.area, Area::Stone);
    // Opened from AREAS, a tap outside goes back to play.
    press(&mut p, button::START);
    for _ in 0..4 {
        press(&mut p, button::DOWN);
    }
    press(&mut p, button::A);
    assert!(p.areas.open && p.areas.back_ok);
    assert_eq!(p.areas.sel, Area::Stone as u8);
    tap(&mut p, 150, 110);
    assert!(!p.areas.open);
    assert_eq!(p.play.area, Area::Stone);
    // The line test itself.
    assert_eq!(AreaMenu::line_at(70, 44), Some(0));
    assert_eq!(AreaMenu::line_at(70, 53), Some(0));
    assert_eq!(AreaMenu::line_at(70, 54), Some(1));
    assert_eq!(AreaMenu::line_at(70, 83), Some(3));
    assert_eq!(AreaMenu::line_at(70, 84), None);
    assert_eq!(AreaMenu::line_at(30, 50), None);
}

#[test]
fn restart_and_areas_lines_work() {
    let mut p = proto();
    press(&mut p, button::A);
    let start = p.play.hero.pos;
    hold(&mut p, button::UP, 20);
    assert_ne!(p.play.hero.pos, start);
    // START opens on HIGH (line 2, the start); two downs: RESTART.
    press(&mut p, button::START);
    assert!(p.menu.open);
    assert_eq!(p.menu.sel, 2);
    press(&mut p, button::DOWN);
    press(&mut p, button::DOWN);
    assert_eq!(press(&mut p, button::A), Note::Restart(Area::Island));
    assert_eq!(p.play.hero.pos, start);
    assert_eq!(p.play.steps, 0);
    assert!(!p.menu.open);
    // AREAS (line 5, the stick can't go past it) opens the area menu on
    // the area in use, with B going back to play unchanged.
    hold(&mut p, button::UP, 5);
    let before = p.play.fingerprint();
    press(&mut p, button::START);
    for _ in 0..6 {
        press(&mut p, button::DOWN);
    }
    assert_eq!(p.menu.sel, 5);
    assert_eq!(press(&mut p, button::START), Note::None);
    assert!(p.areas.open && p.areas.back_ok);
    assert_eq!(p.areas.sel, 0);
    assert_eq!(press(&mut p, button::B), Note::None);
    assert!(!p.areas.open);
    assert_eq!(p.play.fingerprint(), before);
    // AREAS again, two downs and A: SAIL, logged.
    press(&mut p, button::START);
    for _ in 0..4 {
        press(&mut p, button::DOWN);
    }
    press(&mut p, button::A);
    press(&mut p, button::DOWN);
    press(&mut p, button::DOWN);
    assert_eq!(press(&mut p, button::A), Note::Area(Area::Sail));
    assert_eq!(p.play.area, Area::Sail);
    assert_eq!(p.play.steps, 0);
}

#[test]
fn the_detail_setting_is_kept_over_areas_and_restart() {
    let mut p = proto();
    press(&mut p, button::A);
    // MID (one up from HIGH, the start).
    press(&mut p, button::START);
    press(&mut p, button::UP);
    assert_eq!(press(&mut p, button::A), Note::Detail(Detail::Mid));
    // AREAS: STONE.
    press(&mut p, button::START);
    for _ in 0..4 {
        press(&mut p, button::DOWN);
    }
    press(&mut p, button::A);
    for _ in 0..3 {
        press(&mut p, button::DOWN);
    }
    assert_eq!(press(&mut p, button::A), Note::Area(Area::Stone));
    assert_eq!(p.play.detail, Detail::Mid);
    // RESTART.
    press(&mut p, button::START);
    assert_eq!(p.menu.sel, 1);
    press(&mut p, button::DOWN);
    press(&mut p, button::DOWN);
    press(&mut p, button::DOWN);
    assert_eq!(press(&mut p, button::A), Note::Restart(Area::Stone));
    assert_eq!(p.play.detail, Detail::Mid);
}

#[test]
fn every_area_starts() {
    let mut rng = Lcg(23);
    for a in Area::ALL {
        for d in Detail::ALL {
            let mut p = Play::start(a, d);
            assert_eq!(p.area, a);
            assert_eq!(p.detail, d);
            assert_eq!(p.hero.frozen, 0);
            assert!(p.ground().is_some(), "{a:?} starts on a top");
            assert!(p.blocks().len() <= proto3d::play::MAX_LIVE);
            assert!(
                p.blocks()
                    .iter()
                    .filter(|b| b.flags & flag::FADE != 0)
                    .count()
                    <= 4
            );
            let (f, tris) = picture(&p);
            assert!(tris <= budget(a, d.level()), "{a:?} {d:?}: {tris}");
            assert!(f.0.iter().any(|&c| c < 96), "{a:?}: something is drawn");
            let mut last = Pad::NONE;
            for _ in 0..300 {
                last = rng.pad(&last);
                p.step(&last);
            }
            let (_, tris) = picture(&p);
            assert!(tris <= budget(a, d.level()), "{a:?} {d:?}: {tris}");
        }
        // The area's HUD names it (ISLAND keeps its counter).
        let p = Play::start(a, Detail::Mid);
        let mut h = Hud::default();
        p.kit.hud(&mut h);
        if a == Area::Island {
            assert_eq!(h.text[0].as_str(), "");
        } else {
            assert_eq!(h.text[0].as_str(), a.name());
            assert!(h.label.is_some());
        }
    }
}

#[test]
fn the_done_panel_stops_play_and_keeps_the_best() {
    let mut p = proto();
    press(&mut p, button::DOWN);
    press(&mut p, button::A);
    assert_eq!(p.play.area, Area::Machines);
    hold(&mut p, 0, 30);
    // The stubs never finish: done is set here by hand.
    p.play.done = true;
    let t = p.play.kit.time();
    assert_eq!(t, Some(30));
    assert_eq!(press(&mut p, 0), Note::Done(Area::Machines, Some(30)));
    assert_eq!(p.best[1], 30);
    assert!(p.new_best);
    // While done, nothing moves; START still opens the menu.
    let fp = p.play.fingerprint();
    hold(&mut p, button::UP, 10);
    assert_eq!(p.play.fingerprint(), fp);
    // A: again, the best kept, once the panel has been up half a second
    // (rules review F5: A at once is not taken).
    hold(&mut p, 0, 5);
    assert_eq!(press(&mut p, button::A), Note::Restart(Area::Machines));
    assert!(!p.play.done);
    hold(&mut p, 0, 20);
    p.play.done = true;
    assert_eq!(press(&mut p, 0), Note::Done(Area::Machines, Some(20)));
    assert_eq!(p.best[1], 20);
    // Slower: the best stays (RESTART: two downs from HIGH, the start).
    press(&mut p, button::START);
    press(&mut p, button::DOWN);
    press(&mut p, button::DOWN);
    press(&mut p, button::A);
    hold(&mut p, 0, 25);
    p.play.done = true;
    press(&mut p, 0);
    assert_eq!(p.best[1], 20);
    assert!(!p.new_best);
}

#[test]
fn the_game_logs_area_changes_but_not_the_first_choice() {
    // SAFETY: `game_entry` is made by `game_kit::game!`.
    let mut c = unsafe { TestConsole::new(proto3d::game_entry) };
    c.run_ms(200);
    c.tap(button::DOWN);
    c.tap(button::A);
    c.run_ms(500);
    assert_eq!(c.log_lines().len(), 1);
    // The pause menu opens on HIGH (the start): three downs to AREAS.
    c.tap(button::START);
    for _ in 0..3 {
        c.tap(button::DOWN);
    }
    c.tap(button::A);
    c.tap(button::DOWN);
    c.tap(button::A);
    let logs = c.log_lines();
    assert_eq!(logs.len(), 2, "{logs:?}");
    assert!(logs[1].contains("proto3d: area SAIL"));
    // Two downs from HIGH to RESTART.
    c.tap(button::START);
    c.tap(button::DOWN);
    c.tap(button::DOWN);
    c.tap(button::A);
    assert!(c.log_lines()[2].contains("proto3d: restart SAIL"));
}

/// A play's picture with no menu, and its triangle count.
fn picture(p: &Play) -> (Box<Frame>, u32) {
    let mut paint = Box::new(Painter::new());
    let mut f = Box::new(Frame::new());
    paint.picture(&mut f, p, &Menu::new(), false, 1);
    (f, paint.r.stats.tris)
}

#[test]
fn pictures_of_the_menus_and_areas() {
    let Some(dir) = std::env::var_os("PROTO3D_PICTURES") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut paint = Box::new(Painter::new());
    let mut f = Box::new(Frame::new());
    let mut save = |name: &str, play: &Play, s: &Shown| {
        paint.show(&mut f, play, s);
        std::fs::write(dir.join(format!("{name}.png")), png(&f, 4)).unwrap();
    };
    let menu = Menu::new();
    let closed = AreaMenu::CLOSED;
    let plain = |menu, areas| Shown {
        menu,
        areas,
        line: false,
        steps: 1,
        best: 0,
        new_best: false,
    };
    // The start: ISLAND behind the area menu.
    let p = Play::new();
    let start = AreaMenu::new();
    save("menu-areas-start", &p, &plain(&menu, &start));
    let mut m = Menu::new();
    m.open(Detail::Mid, 0);
    m.sel = 3;
    save("menu-pause-restart", &p, &plain(&m, &closed));
    for a in Area::ALL {
        let p = Play::start(a, Detail::Mid);
        save(
            &format!("area-{}", a.name().to_lowercase()),
            &p,
            &plain(&menu, &closed),
        );
    }
    // The done panel (set by hand: the stubs never finish).
    let mut p = Play::start(Area::Machines, Detail::Mid);
    for _ in 0..95 {
        p.step(&Pad::NONE);
    }
    p.done = true;
    let s = Shown {
        best: 80,
        ..plain(&menu, &closed)
    };
    save("done-panel", &p, &s);
}

/// A PNG (8-bit RGB, stored zlib), each pixel `k` times `k` (as in
/// `tests/draw.rs`).
fn png(f: &Frame, k: usize) -> Vec<u8> {
    let (w, h) = (W * k, H * k);
    let mut raw = Vec::with_capacity((w * 3 + 1) * h);
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            let c = PALETTE
                .get(usize::from(f.0[(y / k) * W + x / k]))
                .copied()
                .unwrap_or(0);
            let (r, g, b) = ((c >> 11) & 31, (c >> 5) & 63, c & 31);
            raw.extend([
                (r * 255 / 31) as u8,
                (g * 255 / 63) as u8,
                (b * 255 / 31) as u8,
            ]);
        }
    }
    let mut z = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, ch) in chunks.iter().enumerate() {
        z.push(u8::from(i + 1 == chunks.len()));
        let n = ch.len() as u16;
        z.extend(n.to_le_bytes());
        z.extend((!n).to_le_bytes());
        z.extend_from_slice(ch);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend(((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend((w as u32).to_be_bytes());
    ihdr.extend((h as u32).to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    for (kind, data) in [(b"IHDR", ihdr), (b"IDAT", z), (b"IEND", Vec::new())] {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(&data);
        out.extend(&body);
        let mut c = 0xFFFF_FFFFu32;
        for &x in &body {
            c ^= u32::from(x);
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    (c >> 1) ^ 0xEDB8_8320
                } else {
                    c >> 1
                };
            }
        }
        out.extend((!c).to_be_bytes());
    }
    out
}
