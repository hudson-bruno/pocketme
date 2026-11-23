//! The fourth detail level, MAX (above HIGH): its triangle ceiling, its
//! cost on the CYD as the owner's model ESTIMATES it from the picture's
//! counts, and the views the reports list.
//!
//! The board decides the time, not these tests: MAX is sized for an
//! average of 24 pictures a second during play (41.67 ms a picture, an
//! average, not a ceiling; the owner's decision of 2026-11-22). The est
//! below is only printed.
//!
//! est ms = 0.48 + 0.01473 T + 0.000291 P, with T the triangles filled
//! and P the pixels painted (ESTIMATED: the owner's fit of board timings;
//! it is not a measurement of this game on the board).
//!
//! * `max_stays_in_its_budget`: every swept ISLAND view at MAX (hero on a
//!   grid of places, 32 camera turns) has at most the ceiling's triangles
//!   (the busiest swept view of this step, at the usual pitch and tilted
//!   fully down, plus 5 %) and no triangle is dropped.
//! * `max_is_richer_than_high_everywhere`: in each area the busiest MAX
//!   view has clearly more triangles than the busiest HIGH view.
//! * The ignored tests write the report's numbers:
//!   `PROTO3D_REPORT=<folder> cargo test --release --test max_level -- --ignored`.

use game_3d::Stats;
use game_kit::{Frame, H, W, button};
use proto3d::area::Area;
use proto3d::draw::{PALETTE, Painter};
use proto3d::hero::Hero;
use proto3d::island::{self, Block, flag, top_under};
use proto3d::maths::cm;
use proto3d::pause::Menu;
use proto3d::play::{Detail, Play};
use proto3d::step::Pad;
use proto3d::tune::{HERO_H, PITCH, PITCH_MAX, SHOULDER_STEPS};

/// The swept ISLAND view at MAX with the most triangles had this many
/// (MEASURED, PC, this test's 100 cm grid and 32 turns, the camera at its
/// usual pitch and tilted fully down, 2026-11-22, with the creatures and
/// the ground look `draw::LOOK`); the ceiling is 5 % over it, and the
/// other tests' MAX budget is the same. Set it again with each change of
/// MAX.
const MAX_BUSIEST_T: u32 = 1261;
const MAX_CEIL_T: u32 = MAX_BUSIEST_T * 105 / 100;

/// est in microseconds, from the counts (whole numbers: 480 + 14.73 T +
/// 0.291 P).
pub fn est_us(st: &Stats) -> u32 {
    480 + st.tris * 1_473 / 100 + st.pixels * 291 / 1_000
}

fn est_text(us: u32) -> String {
    format!("{}.{:02}", us / 1000, (us % 1000) / 10)
}

/// Whether the hero's body fits at feet `pos` (subs).
fn fits(blocks: &[Block], pos: [i32; 3]) -> bool {
    let r = cm(22);
    for dy in [cm(5), HERO_H / 2, HERO_H - cm(5)] {
        for (dx, dz) in [(0, 0), (r, r), (-r, r), (r, -r), (-r, -r)] {
            if island::inside_any(blocks, [pos[0] + dx, pos[1] + dy, pos[2] + dz]).is_some() {
                return false;
            }
        }
    }
    true
}

/// Feet places (subs, and the top) on the walkable tops, on a grid of
/// `step` cm, 30 cm in from the edges.
fn spots(blocks: &[Block], step: i16) -> Vec<([i32; 3], u8)> {
    let mut out: Vec<([i32; 3], u8)> = Vec::new();
    for b in blocks
        .iter()
        .filter(|b| b.flags & flag::WALK != 0 && b.solid())
    {
        let mut x = b.lo[0] + 30;
        while x <= b.hi[0] - 30 {
            let mut z = b.lo[2] + 30;
            while z <= b.hi[2] - 30 {
                let (px, pz) = (cm(x), cm(z));
                if let Some((t, y)) = top_under(blocks, px, pz, i32::MAX) {
                    let pos = [px, y, pz];
                    if fits(blocks, pos) && !out.iter().any(|(p, _)| *p == pos) {
                        out.push((pos, t));
                    }
                }
                z += step;
            }
            x += step;
        }
    }
    out
}

/// The hero standing at `pos` on `top`, facing `yaw`, the camera behind
/// him at `yaw` and `pitch`, snapped.
fn stand(p: &mut Play, pos: [i32; 3], top: u8, yaw: u16, pitch: u16) {
    p.hero = Hero::new(pos, top);
    p.hero.yaw = yaw;
    let blocks = p.blocks().to_vec();
    p.shadow = p.hero.shadow(&blocks);
    p.ring = None;
    p.cam.yaw = yaw;
    p.cam.pitch = pitch;
    let props = p.world.props;
    let lvl = p.detail.level();
    p.cam.snap(&p.hero, &blocks, props, lvl);
}

/// The scene's counts (the HUD is not counted).
fn counts(paint: &mut Painter, f: &mut Frame, p: &Play) -> Stats {
    paint.picture(f, p, &Menu::new(), false, 1);
    paint.r.stats
}

/// The busiest view of a sweep.
#[derive(Clone, Debug, Default)]
pub struct Busiest {
    pub t: u32,
    pub p: u32,
    pub est: u32,
    pub at: String,
    pub views: u32,
    /// The view with the most triangles (may differ from the dearest).
    pub most_t: u32,
    pub dropped: u32,
    /// The dearest view itself.
    pub play: Option<Play>,
}

/// Every view of `area` at `d`: the hero on a `step` cm grid, `turns`
/// camera turns, the camera's usual pitch. Returns the dearest view (by
/// est).
pub fn sweep(area: Area, d: Detail, step: i16, turns: u32) -> Busiest {
    sweep_at(area, d, step, turns, PITCH)
}

/// [`sweep`] with the camera at `pitch`.
pub fn sweep_at(area: Area, d: Detail, step: i16, turns: u32, pitch: u16) -> Busiest {
    let mut paint = Box::new(Painter::new());
    let mut f = Box::new(Frame::new());
    let base = Play::start(area, d);
    let blocks = base.blocks().to_vec();
    let mut b = Busiest::default();
    for (pos, top) in spots(&blocks, step) {
        for k in 0..turns {
            let yaw = (k * 65_536 / turns) as u16;
            let mut p = base;
            stand(&mut p, pos, top, yaw, pitch);
            let st = counts(&mut paint, &mut f, &p);
            let e = est_us(&st);
            b.views += 1;
            b.dropped += st.dropped;
            b.most_t = b.most_t.max(st.tris);
            if e > b.est {
                let c = |s: i32| s * 25 / 1024;
                b.t = st.tris;
                b.p = st.pixels;
                b.est = e;
                b.play = Some(p);
                b.at = format!(
                    "x{} y{} z{} cm, turn {} deg",
                    c(pos[0]),
                    c(pos[1]),
                    c(pos[2]),
                    k * 360 / turns
                );
            }
        }
    }
    b
}

/// The grid of the tests (cm); the report uses a finer one.
fn test_step() -> i16 {
    std::env::var("MAX_STEP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100)
}

#[test]
fn max_stays_in_its_budget() {
    let mut b = sweep(Area::Island, Detail::Max, test_step(), 32);
    let down = sweep_at(Area::Island, Detail::Max, test_step(), 32, PITCH_MAX);
    println!("tilted fully down: most T {}", down.most_t);
    b.most_t = b.most_t.max(down.most_t);
    b.dropped += down.dropped;
    println!(
        "ISLAND MAX: {} views, dearest T {} P {} est {} ms (ESTIMATED) at {}; most T {}",
        b.views,
        b.t,
        b.p,
        est_text(b.est),
        b.at,
        b.most_t
    );
    assert_eq!(b.dropped, 0);
    assert!(b.most_t <= MAX_CEIL_T, "{} triangles", b.most_t);
}

/// In each area, MAX's view with the most triangles has at least 15 %
/// more than HIGH's, and its dearest view costs more (16 turns, the
/// test's grid).
#[test]
fn max_is_richer_than_high_everywhere() {
    for area in Area::ALL {
        let high = sweep(area, Detail::High, test_step(), 16);
        let max = sweep(area, Detail::Max, test_step(), 16);
        println!(
            "{}: most T HIGH {} MAX {}; dearest est HIGH {} MAX {} ms",
            area.name(),
            high.most_t,
            max.most_t,
            est_text(high.est),
            est_text(max.est)
        );
        assert!(max.most_t * 100 >= high.most_t * 115, "{area:?}");
        assert!(max.est > high.est, "{area:?}");
        assert_eq!(max.dropped, 0, "{area:?}");
    }
}

/// The report's table: the dearest view of each area at each level
/// (`MAX_STEP` cm grid, default 50; 32 turns).
#[test]
#[ignore]
fn max_level_report() {
    let step: i16 = std::env::var("MAX_STEP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);
    let mut text = format!(
        "# The dearest view per area and level: hero on a {step} cm grid, 32 camera turns, pitch 24 deg.\n# est ms = 0.48 + 0.01473 T + 0.000291 P (ESTIMATED from MEASURED PC counts T, P).\n"
    );
    // `MAX_TRIM=1`: ISLAND at MAX only (for the trim table).
    let trim = std::env::var_os("MAX_TRIM").is_some();
    for area in Area::ALL {
        for d in Detail::ALL {
            if trim && (area != Area::Island || d != Detail::Max) {
                continue;
            }
            let b = sweep(area, d, step, 32);
            let line = format!(
                "{:8} {:4}: {:5} views; dearest T {:4} P {:5} est {:>5} ms at {}; most T {}; dropped {}\n",
                area.name(),
                d.name(),
                b.views,
                b.t,
                b.p,
                est_text(b.est),
                b.at,
                b.most_t,
                b.dropped
            );
            print!("{line}");
            text.push_str(&line);
            if let (Some(dir), Some(p)) = (std::env::var_os("PROTO3D_REPORT"), &b.play) {
                let dir = std::path::Path::new(&dir).join("busiest");
                let name = format!("{}-{}", area.name(), d.name()).to_lowercase();
                save(dir.as_os_str(), &name, p);
            }
        }
    }
    if let Some(dir) = std::env::var_os("PROTO3D_REPORT") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::Path::new(&dir).join("busiest-views.txt"), text).unwrap();
    }
}

/// The start of `area` at `d` after R was tapped `taps` times (each tap:
/// one step with R held, then the turn's steps and a settling time).
fn after_taps(area: Area, d: Detail, taps: u32) -> Play {
    let mut p = Play::start(area, d);
    let none = Pad::NONE;
    let r = Pad {
        held: button::R,
        ..Pad::NONE
    };
    for _ in 0..taps {
        p.step(&r);
        for _ in 0..u32::from(SHOULDER_STEPS) + 2 {
            p.step(&none);
        }
    }
    for _ in 0..30 {
        p.step(&none);
    }
    p
}

/// The start views: each area at each level, R tapped 0 to 7 times (45
/// degrees a tap), with T, P and est; the busiest turn of each area (at
/// MAX) and the busiest view of each area and level.
#[test]
#[ignore]
fn start_views_report() {
    let mut paint = Box::new(Painter::new());
    let mut f = Box::new(Frame::new());
    let mut text = String::from(
        "# PROTO3D start views: each area at its start, R tapped 0..7 times (45 deg a tap, camera settled 30 steps).\n# T triangles, P pixels painted: MEASURED (PC, the game's own counts, tests/max_level.rs start_views_report).\n# est ms = 0.48 + 0.01473 T + 0.000291 P: ESTIMATED (the owner's model of the CYD, not measured on the board).\n\n",
    );
    let mut busiest = String::from("\n# The busiest start view per area and level\n");
    for area in Area::ALL {
        let mut turn_est = [0u32; 8];
        for d in Detail::ALL {
            let mut best = (0u32, 0u32, 0u32, 0u32);
            for taps in 0..8u32 {
                let p = after_taps(area, d, taps);
                let st = counts(&mut paint, &mut f, &p);
                let e = est_us(&st);
                let line = format!(
                    "{:8} {:4} R x{}: T {:4} P {:5} est {:>5} ms\n",
                    area.name(),
                    d.name(),
                    taps,
                    st.tris,
                    st.pixels,
                    est_text(e)
                );
                text.push_str(&line);
                if d == Detail::Max {
                    turn_est[taps as usize] = e;
                }
                if e > best.3 {
                    best = (taps, st.tris, st.pixels, e);
                }
                if let Some(dir) = std::env::var_os("PROTO3D_REPORT")
                    && (taps == 0 || d == Detail::Max)
                {
                    let dir = std::path::Path::new(&dir).join("start");
                    let name = format!("{}-{}-r{}", area.name(), d.name(), taps).to_lowercase();
                    save(dir.as_os_str(), &name, &p);
                }
            }
            busiest.push_str(&format!(
                "{:8} {:4}: R x{} (T {} P {} est {} ms)\n",
                area.name(),
                d.name(),
                best.0,
                best.1,
                best.2,
                est_text(best.3)
            ));
        }
        let (k, e) = turn_est
            .iter()
            .enumerate()
            .max_by_key(|(_, e)| **e)
            .map(|(k, e)| (k, *e))
            .unwrap_or((0, 0));
        text.push_str(&format!(
            "{:8} busiest turn at MAX: R x{} ({} ms)\n\n",
            area.name(),
            k,
            est_text(e)
        ));
    }
    text.push_str(&busiest);
    print!("{text}");
    if let Some(dir) = std::env::var_os("PROTO3D_REPORT") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::Path::new(&dir).join("start-views.txt"), text).unwrap();
    }
}

/// The start of `area` at `d` with the camera tilted fully down (the right
/// stick held down until the pitch stops at its limit, 40 deg), then R
/// tapped `taps` times.
fn tilted_down(area: Area, d: Detail, taps: u32) -> Play {
    let mut p = after_taps(area, d, taps);
    let down = Pad {
        right: (0, 32_767),
        ..Pad::NONE
    };
    for _ in 0..90 {
        p.step(&down);
    }
    for _ in 0..30 {
        p.step(&Pad::NONE);
    }
    p
}

/// The hero standing at (x, y, z) cm on ISLAND at `d`, facing and seen
/// from `yaw`, the camera at `pitch` (snapped; the sweep's way).
fn island_view(d: Detail, at: [i16; 3], yaw: u16, pitch: u16) -> Play {
    let mut p = Play::start(Area::Island, d);
    let blocks = p.blocks().to_vec();
    let (x, z) = (cm(at[0]), cm(at[2]));
    let (top, y) = top_under(&blocks, x, z, cm(at[1])).unwrap_or((0, 0));
    stand(&mut p, [x, y, z], top, yaw, pitch);
    p.cam.pitch = pitch;
    let props = p.world.props;
    let lvl = p.detail.level();
    p.cam.snap(&p.hero, &blocks, props, lvl);
    p
}

/// The ladder's fixed views (name, play): ISLAND's start, the start
/// tilted fully down, near the trees, at the main island's edge looking
/// across the gap, a far view from the floating block, and the other
/// areas' starts.
pub fn ladder_views(d: Detail) -> Vec<(&'static str, Play)> {
    vec![
        ("1-start", after_taps(Area::Island, d, 0)),
        ("2-down", tilted_down(Area::Island, d, 0)),
        ("3-trees", island_view(d, [380, 0, -300], 40_000, PITCH)),
        ("4-edge", island_view(d, [0, 0, 640], 0, PITCH)),
        (
            "5-far",
            island_view(d, [280, 150, 380], 32_768, proto3d::tune::PITCH_MIN),
        ),
        ("6-machines", after_taps(Area::Machines, d, 0)),
        ("7-sail", after_taps(Area::Sail, d, 0)),
        ("8-stone", after_taps(Area::Stone, d, 0)),
    ]
}

/// The ladder's numbers (`PROTO3D_LADDER=<folder>` also writes
/// `ladder.txt` and the fixed views' pictures there): ISLAND at MAX, the
/// mean and the most over the swept views (`MAX_STEP` cm grid, default
/// 100; 32 turns), the eight start turns, the start tilted fully down,
/// and the fixed views. T and P MEASURED (PC, the game's own counts); est
/// ESTIMATED with the PC model (too low at MAX by up to 3 ms on the board
/// last time).
#[test]
#[ignore]
fn ladder_report() {
    let step = test_step();
    let mut paint = Box::new(Painter::new());
    let mut f = Box::new(Frame::new());
    let base = Play::start(Area::Island, Detail::Max);
    let blocks = base.blocks().to_vec();
    let (mut n, mut st_sum, mut sp_sum, mut se_sum) = (0u64, 0u64, 0u64, 0u64);
    let (mut most_t, mut most_p, mut most_e, mut dropped) = (0u32, 0u32, 0u32, 0u32);
    let (mut at_t, mut at_e) = (String::new(), String::new());
    let mut hist = [0u32; 8];
    for (pos, top) in spots(&blocks, step) {
        for k in 0..32u32 {
            let yaw = (k * 65_536 / 32) as u16;
            let mut p = base;
            stand(&mut p, pos, top, yaw, PITCH);
            let st = counts(&mut paint, &mut f, &p);
            let e = est_us(&st);
            n += 1;
            st_sum += u64::from(st.tris);
            sp_sum += u64::from(st.pixels);
            se_sum += u64::from(e);
            dropped += st.dropped;
            hist[(e / 10_000).min(7) as usize] += 1;
            let c = |s: i32| s * 25 / 1024;
            let here = format!(
                "x{} y{} z{} cm, turn {} deg",
                c(pos[0]),
                c(pos[1]),
                c(pos[2]),
                k * 360 / 32
            );
            if st.tris > most_t {
                most_t = st.tris;
                at_t = format!("{here} (P {})", st.pixels);
            }
            most_p = most_p.max(st.pixels);
            if e > most_e {
                most_e = e;
                at_e = format!("{here} (T {} P {})", st.tris, st.pixels);
            }
        }
    }
    let mut text = format!(
        "# PROTO3D ISLAND at MAX. T triangles, P pixels: MEASURED (PC, the game's own counts).\n# est = 0.48 + 0.01473 T + 0.000291 P ms: ESTIMATED (PC model; too low at MAX by up to 3 ms on the board last time).\nsweep: {n} views ({step} cm grid, 32 turns); mean T {} P {} est {}; most T {most_t} at {at_t}; most P {most_p}; dearest est {} at {at_e}; dropped {dropped}\nsweep est histogram (0-10, 10-20, .. 70+ ms): {hist:?}\n",
        st_sum / n,
        sp_sum / n,
        est_text((se_sum / n) as u32),
        est_text(most_e),
    );
    let down = sweep_at(Area::Island, Detail::Max, step, 32, PITCH_MAX);
    text.push_str(&format!(
        "sweep tilted fully down (40 deg): most T {}; dearest est {} at {} (T {} P {}); dropped {}\n",
        down.most_t,
        est_text(down.est),
        down.at,
        down.t,
        down.p,
        down.dropped
    ));
    let (mut s_t, mut s_p, mut s_max_t) = (0u32, 0u32, 0u32);
    let mut line = String::from("start turns T:");
    for taps in 0..8 {
        let p = after_taps(Area::Island, Detail::Max, taps);
        let st = counts(&mut paint, &mut f, &p);
        s_t += st.tris;
        s_p += st.pixels;
        s_max_t = s_max_t.max(st.tris);
        line.push_str(&format!(" {}/{}", st.tris, st.pixels));
    }
    text.push_str(&format!(
        "{line}\nstart 8 turns: mean T {} P {} est {}; most T {s_max_t}\n",
        s_t / 8,
        s_p / 8,
        est_text(480 + (s_t / 8) * 1_473 / 100 + (s_p / 8) * 291 / 1_000)
    ));
    for taps in [0u32, 1, 7] {
        let p = tilted_down(Area::Island, Detail::Max, taps);
        let st = counts(&mut paint, &mut f, &p);
        text.push_str(&format!(
            "start tilted down (40 deg) R x{taps}: T {} P {} est {} (pitch {} tilt {})\n",
            st.tris,
            st.pixels,
            est_text(est_us(&st)),
            p.cam.pitch,
            p.cam.tilt
        ));
    }
    let dir = std::env::var_os("PROTO3D_LADDER");
    for (name, p) in ladder_views(Detail::Max) {
        let st = counts(&mut paint, &mut f, &p);
        text.push_str(&format!(
            "view {name}: T {} P {} est {}\n",
            st.tris,
            st.pixels,
            est_text(est_us(&st))
        ));
        if let Some(d) = &dir {
            save(d.as_os_str(), name, &p);
        }
    }
    print!("{text}");
    if let Some(d) = &dir {
        std::fs::create_dir_all(d).unwrap();
        std::fs::write(std::path::Path::new(d).join("ladder.txt"), text).unwrap();
    }
}

/// A PNG (8-bit RGB, stored zlib), each pixel `k` times `k`.
pub fn png(f: &Frame, k: usize) -> Vec<u8> {
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
        out.extend(crc32(&body).to_be_bytes());
    }
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// Writes a view's picture (4x) into `dir`.
fn save(dir: &std::ffi::OsStr, name: &str, p: &Play) {
    let mut paint = Box::new(Painter::new());
    let mut f = Box::new(Frame::new());
    paint.picture(&mut f, p, &Menu::new(), false, 1);
    let d = std::path::Path::new(dir);
    std::fs::create_dir_all(d).unwrap();
    std::fs::write(d.join(format!("{name}.png")), png(&f, 4)).unwrap();
}

/// Each level's ISLAND sweep (`MAX_STEP` cm grid, default 100; 32 turns;
/// the usual pitch): views, mean T and P, the most T, and the dearest
/// view. T and P MEASURED (PC, the game's own counts).
#[test]
#[ignore]
fn level_means_report() {
    let step = test_step();
    let mut paint = Box::new(Painter::new());
    let mut f = Box::new(Frame::new());
    let mut text = format!(
        "# ISLAND sweep per level: hero on a {step} cm grid, 32 turns, usual pitch. T, P MEASURED (PC).\n"
    );
    for d in Detail::ALL {
        let base = Play::start(Area::Island, d);
        let blocks = base.blocks().to_vec();
        let (mut n, mut st, mut sp) = (0u64, 0u64, 0u64);
        let (mut most_t, mut dear) = (0u32, (0u32, 0u32, 0u32));
        for (pos, top) in spots(&blocks, step) {
            for k in 0..32u32 {
                let mut p = base;
                stand(&mut p, pos, top, (k * 65_536 / 32) as u16, PITCH);
                let s = counts(&mut paint, &mut f, &p);
                n += 1;
                st += u64::from(s.tris);
                sp += u64::from(s.pixels);
                most_t = most_t.max(s.tris);
                let e = est_us(&s);
                if e > dear.2 {
                    dear = (s.tris, s.pixels, e);
                }
            }
        }
        text.push_str(&format!(
            "{:4}: {n} views; mean T {} P {}; most T {most_t}; dearest T {} P {} est {} ms\n",
            d.name(),
            st / n,
            sp / n,
            dear.0,
            dear.1,
            est_text(dear.2)
        ));
    }
    print!("{text}");
    if let Some(dir) = std::env::var_os("PROTO3D_REPORT") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::Path::new(&dir).join("level-means.txt"), text).unwrap();
    }
}
