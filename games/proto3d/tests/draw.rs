//! The picture (PROTOTYPE-SPEC.md, sections 3, 5 and 6, tests): the four
//! moments of the comparison at each detail, budgets, cracks, the line
//! of numbers, the counter and golden pictures.
//!
//! With `PROTO3D_PICTURES=<folder>` the twelve pictures are written there
//! as PNG files, at 1x and 4x. With `UPDATE_GOLDEN=1` the golden file is
//! written again.

use std::path::Path;

use game_3d::{Stats, deg};
use game_kit::{FONT_H, Frame, H, W, text_width};
use proto3d::draw::{self, LINE_AT, PALETTE, Painter, col, counter_text, counter_x, line_text};
use proto3d::hero::{Hero, Pose};
use proto3d::island::{ISLAND, MAIN, TERRACE, World};
use proto3d::maths::cm;
use proto3d::models::{self, mat};
use proto3d::pause::Menu;
use proto3d::play::{Detail, Play};
use proto3d::step::Pad;
use proto3d::tune::{JUMP, RUN};

/// ISLAND's budgets (triangles a picture), LOW, MID, HIGH and MAX (MAX:
/// the most triangles of the swept ISLAND views at MAX plus 5 %, as in
/// tests/max_level.rs). The spec's are 160, 430 and 940; LOW and MID are
/// raised by the creatures' share (MEASURED, PC: the busiest LOW view
/// went from 160 to 243 triangles and MID from 372 to 474 with them).
const BUDGET: [u32; 4] = [256, 500, 940, 1324];

/// The four moments of the comparison (`compare/mock`).
const MOMENTS: [&str; 4] = ["a-stand", "b-jump", "c-gap", "d-terrace"];

/// A play state for a moment: the hero placed on its ground with the
/// camera behind it, then (for the jumps) lifted into the air, so the
/// camera keeps the ground height as in a real jump.
fn moment(name: &str, d: Detail) -> Play {
    turned(name, d, 0)
}

/// [`moment`] with the camera turned `turn` more round the hero.
fn turned(name: &str, d: Detail, turn: u16) -> Play {
    moment_in(&ISLAND, name, d, turn)
}

/// [`turned`] in the world `w` (ISLAND, or ISLAND without creatures).
fn moment_in(w: &'static World, name: &str, d: Detail, turn: u16) -> Play {
    let mut p = Play::on(w, d);
    let (feet, top, yaw, air) = match name {
        "a-stand" => ([50, 0, -320], MAIN, 0, None),
        "b-jump" => ([380, 0, 220], MAIN, 20, Some((120, JUMP / 2, 0))),
        "c-gap" => ([0, 0, 640], MAIN, 0, Some((70, JUMP / 4, RUN))),
        _ => ([-350, 100, 300], TERRACE, 40, None),
    };
    p.hero = Hero::new(feet.map(cm), top);
    p.hero.yaw = deg(yaw);
    p.cam.yaw = deg(yaw).wrapping_add(turn);
    p.cam.snap(&p.hero, w.blocks, w.props, d.level());
    let forward = Pad {
        left: (0, -32_767),
        ..Pad::NONE
    };
    if let Some((h, up, run)) = air {
        p.hero.ground = None;
        p.hero.pos[1] = cm(h);
        if name == "c-gap" {
            p.hero.pos[2] = cm(820);
        }
        p.hero.vel = [0, up, run];
    }
    p.shadow = p.hero.shadow(w.blocks);
    p.ring = p.hero.ring(&forward, p.cam.yaw, w.blocks);
    p
}

fn picture(p: &Play, line: bool) -> (Box<Frame>, Stats) {
    let mut paint = Box::new(Painter::new());
    let mut f = Box::new(Frame::new());
    paint.picture(&mut f, p, &Menu::new(), line, 1);
    (f, paint.r.stats)
}

/// A PNG (8-bit RGB, stored zlib), each pixel `k` times `k`.
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
        out.extend(crc32(&body).to_be_bytes());
    }
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
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

fn is_sky(c: u8) -> bool {
    (col::SKY0..=col::HAZE).contains(&c)
}

/// The surface a palette entry belongs to: a lit material with its
/// checker, rim and dark-patch twins folded in; `None` for sky, text,
/// the ring and leaves (a HIGH tree's crown is two cones and a bush sits
/// before trees: their jagged outlines leave one-pixel notches of sky
/// between leaf pixels that are the shape, not a seam).
fn surface(c: u8) -> Option<u8> {
    let m = match c {
        col::DARK_GRASS => mat::GRASS,
        col::DARK_STONE => mat::STONE,
        col::DARK_SAND => mat::SAND,
        col::DARK_BLOCK => mat::BLOCK_TOP,
        c if c < 96 => c % models::MATS,
        _ => return None,
    };
    if m == mat::LEAF || m == mat::LEAF2 {
        return None;
    }
    Some(match m {
        mat::GRASS2 | mat::RIM_GRASS => mat::GRASS,
        mat::RIM_STONE => mat::STONE,
        mat::RIM_SAND => mat::SAND,
        mat::RIM_BLOCK => mat::BLOCK_TOP,
        m => m,
    })
}

/// Crack pixels: sky showing in a single pixel between two pixels of the
/// same surface (left and right, or above and below), a seam that should
/// not be there. A one-pixel strip of sky between two different things
/// (a nut over a block, the islet over the gap, a leaf over the grass
/// behind, the counter's letters) is a real gap and is not counted.
fn cracks(f: &Frame) -> u32 {
    let (w, h) = (W, H);
    let at = |x: usize, y: usize| f.0[y * w + x];
    let same = |a: u8, b: u8| surface(a).is_some() && surface(a) == surface(b);
    let mut n = 0;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            if !is_sky(at(x, y)) {
                continue;
            }
            let lr = same(at(x - 1, y), at(x + 1, y));
            let ud = same(at(x, y - 1), at(x, y + 1));
            if lr || ud {
                n += 1;
            }
        }
    }
    n
}

fn hash(f: &Frame) -> u64 {
    f.0.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}

fn detail_name(d: Detail) -> String {
    d.name().to_lowercase()
}

#[test]
fn the_four_moments_stay_in_budget_and_are_written() {
    let dir = std::env::var_os("PROTO3D_PICTURES");
    for d in Detail::ALL {
        for m in MOMENTS {
            let p = moment(m, d);
            let (f, st) = picture(&p, false);
            println!(
                "{} {m}: tris {} objects {} front {} pixels {} cracks {}",
                detail_name(d),
                st.tris,
                st.objects,
                st.front,
                st.pixels,
                cracks(&f)
            );
            if let Some(dir) = &dir {
                std::fs::create_dir_all(dir).unwrap();
                let base = Path::new(dir).join(format!("{}-{m}", detail_name(d)));
                std::fs::write(base.with_extension("png"), png(&f, 1)).unwrap();
                let x4 = Path::new(dir).join(format!("{}-{m}-x4.png", detail_name(d)));
                std::fs::write(x4, png(&f, 4)).unwrap();
            }
            assert_eq!(st.dropped, 0, "{d:?} {m}");
            assert!(
                st.tris <= BUDGET[usize::from(d.level())],
                "{d:?} {m}: {st:?}"
            );
        }
    }
}

#[test]
fn the_moments_show_what_they_should() {
    // The jumps' moments are in the air, the others on a top.
    assert_eq!(moment("a-stand", Detail::Mid).pose(), Pose::Stand);
    assert_eq!(moment("b-jump", Detail::Mid).pose(), Pose::Jump);
    assert_eq!(moment("d-terrace", Detail::Mid).pose(), Pose::Stand);
    // Over the gap: no shadow, a landing ring on the islet.
    let gap = moment("c-gap", Detail::Mid);
    assert!(gap.shadow.is_none());
    assert_eq!(gap.ring.map(|r| r.top), Some(proto3d::island::ISLET));
    // The hero shows its crimson top and white cap; a nut shows gold.
    let (f, _) = picture(&moment("a-stand", Detail::Mid), false);
    let has = |m: u8| f.0.iter().any(|&c| c % models::MATS == m && c < 96);
    assert!(has(mat::HERO), "no crimson");
    assert!(has(mat::CAP), "no cap");
    let (f, _) = picture(&moment("b-jump", Detail::Mid), false);
    assert!(
        f.0.iter().any(|&c| c < 96 && c % models::MATS == mat::GOLD),
        "no gold"
    );
    // The ring is drawn over the gap.
    let (f, _) = picture(&gap, false);
    assert!(f.0.contains(&col::RING), "no ring");
}

/// A sweep: the camera turned all the way round at each moment, at each
/// detail. Every picture within budget, nothing dropped, few cracks.
#[test]
fn a_camera_sweep_stays_in_budget_with_few_cracks() {
    for d in Detail::ALL {
        let (mut most, mut worst_cracks, mut sum_cracks, mut pictures) = (0, 0, 0, 0);
        let (mut worst_at, mut crack_at) = (("", 0), ("", 0));
        for m in MOMENTS {
            for k in 0..16u16 {
                let p = turned(m, d, k * 4096);
                let (f, st) = picture(&p, false);
                assert_eq!(st.dropped, 0, "{d:?} {m} {k}");
                if st.tris > most {
                    worst_at = (m, k);
                }
                most = most.max(st.tris);
                let c = cracks(&f);
                if c > worst_cracks {
                    crack_at = (m, k);
                }
                worst_cracks = worst_cracks.max(c);
                sum_cracks += c;
                pictures += 1;
            }
        }
        println!(
            "{} sweep: {pictures} pictures, most tris {most} at {worst_at:?} (budget {}), cracks worst {worst_cracks} at {crack_at:?}, total {sum_cracks}",
            detail_name(d),
            BUDGET[usize::from(d.level())]
        );
        assert!(most <= BUDGET[usize::from(d.level())], "{d:?}: {most}");
        assert!(worst_cracks <= 9, "{d:?}: {worst_cracks} crack pixels");
    }
}

#[test]
fn the_line_of_numbers_matches_its_stored_text() {
    let p = moment("a-stand", Detail::Mid);
    let (_, st) = picture(&p, true);
    assert_eq!(line_text(&st, 1).as_str(), STORED_LINE);
}

/// The line for `a-stand` at MID, one step.
const STORED_LINE: &str = "T451 O35 F363 P30422 S1";

#[test]
fn hiding_the_line_changes_no_other_pixel() {
    for m in MOMENTS {
        let p = moment(m, Detail::High);
        let (shown, st) = picture(&p, true);
        let (hidden, st2) = picture(&p, false);
        assert_eq!(st, st2, "the line is not counted");
        let w = text_width(line_text(&st, 1).as_str()) as usize;
        let (x0, y0) = (LINE_AT.0 as usize, LINE_AT.1 as usize);
        // The text and its shadow one pixel right and down.
        let (x1, y1) = (x0 + w + 1, y0 + usize::from(FONT_H) + 1);
        let mut differ = 0;
        for y in 0..H {
            for x in 0..W {
                let i = y * W + x;
                if shown.0[i] != hidden.0[i] {
                    differ += 1;
                    assert!(x >= x0 && x < x1 && y >= y0 && y < y1, "{m}: pixel {x},{y}");
                }
            }
        }
        assert!(differ > 0, "{m}: the line is not drawn");
    }
}

#[test]
fn the_counter_keeps_off_the_right_edge() {
    let mut p = moment("a-stand", Detail::Mid);
    for taken in [0u8, 0b11_1111] {
        p.coins.taken = taken;
        let t = counter_text(&p);
        let right = counter_x(&t) + text_width(t.as_str()) as i16;
        // Text ends 3 pixels in, its shadow 2.
        assert_eq!(right, W as i16 - 3, "{}", t.as_str());
        let (f, _) = picture(&p, false);
        for y in 0..12 {
            for x in W - 2..W {
                let c = f.0[y * W + x];
                assert!(c != col::TEXT && c != col::TEXT_SHADOW, "{x},{y}");
            }
        }
    }
}

#[test]
fn every_model_fits_the_renderer() {
    let all = [
        &models::NUT,
        &models::TREE_LOW,
        &models::TREE_MID,
        &models::TREE_HIGH,
        &models::TREE_MAX,
        &models::HERO_HEAD,
        &models::BUSH,
        &models::TUFTS,
        &models::CONE_MAIN_LOW,
        &models::CONE_MAIN_MID,
        &models::CONE_MAIN_HIGH,
        &models::CONE_ISLET,
        &models::FAR_ISLET,
    ];
    for m in all.into_iter().chain(models::HERO.iter()) {
        assert!(m.verts.len() <= 64 && m.faces.len() <= 128);
        assert_eq!(m.faces.len(), m.normals.len());
    }
    // MAX's hero parts: every face of HERO in exactly one part, the
    // head and cap ([`models::HERO_HEAD`] replaces them) left out.
    let parts = [
        models::HERO_BODY,
        models::HERO_ARM_PX,
        models::HERO_ARM_NX,
        models::HERO_BRIM,
    ];
    for k in 0..models::POSES {
        let mut n = 0;
        for r in parts.clone() {
            let m = models::hero_part(k, r.clone());
            assert_eq!(m.faces.len(), r.len());
            assert_eq!(m.faces.len(), m.normals.len());
            assert!(
                m.faces
                    .iter()
                    .all(|f| f.v.iter().all(|&v| usize::from(v) < m.verts.len()))
            );
            n += r.len();
        }
        assert_eq!(
            n + 18,
            models::HERO[k].faces.len(),
            "8 head and 10 cap faces left out"
        );
    }
    assert_eq!(models::HERO_BRIM.end, models::HERO[0].faces.len());
    const { assert!(draw::COLOURS <= 240) };
}

/// ISLAND's LOW, MID and HIGH golden pictures from before the creatures
/// came (tests/golden/pictures.txt of R6, the build they were added to),
/// as the repair round after V1 left them: the rock under the islands
/// starts 2 cm inside them, the main island's rock at MID and up meets
/// its square edge, and LOW's nuts have no sides (a few pixels each).
const BEFORE_CREATURES: [(&str, &str, &str); 12] = [
    ("low", "a-stand", "661957edc343c76b"),
    ("low", "b-jump", "f95d71692ff7f0a6"),
    ("low", "c-gap", "8ce80f971830c2ca"),
    ("low", "d-terrace", "44e09df266ac31cf"),
    ("mid", "a-stand", "c267ebb96056629c"),
    ("mid", "b-jump", "907252a50d6f6f4a"),
    ("mid", "c-gap", "0ce06e3e016d784f"),
    ("mid", "d-terrace", "70ae8820d6ba669b"),
    ("high", "a-stand", "00ce874eb8ee48e8"),
    ("high", "b-jump", "05babec297f4bf9b"),
    ("high", "c-gap", "b8d8971816252eea"),
    ("high", "d-terrace", "b083b9f617aa3c78"),
];

/// The creatures are the only change to ISLAND's LOW, MID and HIGH
/// pictures: with them taken out, the four moments are the pictures from
/// before they came, pixel for pixel.
#[test]
fn without_creatures_the_moments_are_the_old_pictures() {
    let bare: &'static World = Box::leak(Box::new(World {
        critters: &[],
        ..ISLAND
    }));
    for (d, (lvl, m, want)) in [Detail::Low, Detail::Mid, Detail::High]
        .iter()
        .flat_map(|&d| std::iter::repeat_n(d, 4))
        .zip(BEFORE_CREATURES)
    {
        assert_eq!(detail_name(d), lvl);
        let (f, _) = picture(&moment_in(bare, m, d, 0), true);
        assert_eq!(format!("{:016x}", hash(&f)), want, "{lvl} {m}");
    }
}

/// Golden pictures: a hash of each of the twelve pictures, kept in
/// `tests/golden/pictures.txt`.
#[test]
fn golden_pictures_of_the_four_moments() {
    let mut text = String::from("# PROTO3D: FNV-1a hashes of the four moments at each detail\n");
    for d in Detail::ALL {
        for m in MOMENTS {
            let (f, _) = picture(&moment(m, d), true);
            text += &format!("{} {m} {:016x}\n", detail_name(d), hash(&f));
        }
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/pictures.txt");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    let want = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        text, want,
        "pictures changed: look at them, then UPDATE_GOLDEN=1"
    );
}
