//! Eight fixed ISLAND views for looking at the picture by eye, at every
//! detail level (`PROTO3D_VIEWS=<folder> cargo test --test views --
//! --ignored`): the start, the start tilted fully down, next to a burr,
//! next to a tuftling, a burr squashed, an island edge looking across,
//! the far view from the floating block, and near the trees. Only the
//! play's read interface is used, so the same file also gives the views
//! of builds without creatures (the places are the same).

use game_kit::{Frame, H, W};
use proto3d::area::Area;
use proto3d::draw::{PALETTE, Painter};
use proto3d::hero::Hero;
use proto3d::island::top_under;
use proto3d::maths::cm;
use proto3d::pause::Menu;
use proto3d::play::{Detail, Play};
use proto3d::step::Pad;
use proto3d::tune::{PITCH, PITCH_MIN};

/// ISLAND at `d` after `steps` steps with no input.
fn after(d: Detail, steps: u32, pad: &Pad) -> Play {
    let mut p = Play::start(Area::Island, d);
    for _ in 0..steps {
        p.step(pad);
    }
    p
}

/// `p` after `steps` more steps with no input.
fn run(mut p: Play, steps: u32) -> Play {
    for _ in 0..steps {
        p.step(&Pad::NONE);
    }
    p
}

/// The hero standing at (x, z) cm (on the highest top at or under `y`
/// cm), facing and seen from `yaw`, the camera at `pitch`.
fn at(mut p: Play, x: i16, y: i16, z: i16, yaw: u16, pitch: u16) -> Play {
    let blocks = p.blocks().to_vec();
    let (x, z) = (cm(x), cm(z));
    let (top, fy) = top_under(&blocks, x, z, cm(y)).unwrap_or((0, 0));
    p.hero = Hero::new([x, fy, z], top);
    p.hero.yaw = yaw;
    p.shadow = p.hero.shadow(&blocks);
    p.ring = None;
    p.cam.yaw = yaw;
    p.cam.pitch = pitch;
    let props = p.world.props;
    let lvl = p.detail.level();
    p.cam.snap(&p.hero, &blocks, props, lvl);
    p
}

/// The eight views at `d`.
fn views(d: Detail) -> Vec<(&'static str, Play)> {
    let none = Pad::NONE;
    let down = Pad {
        right: (0, 32_767),
        ..Pad::NONE
    };
    let start = after(d, 30, &none);
    let mut tilted = Play::start(Area::Island, d);
    for _ in 0..90 {
        tilted.step(&down);
    }
    for _ in 0..30 {
        tilted.step(&none);
    }
    // The squash: the hero dropped from 60 cm onto the first burr where
    // it walks four steps later, the camera from the side; the picture
    // three steps after he touches it.
    let mut squash = at(Play::start(Area::Island, d), -52, 0, -140, 49_152, PITCH);
    squash.hero.pos[1] = cm(60);
    squash.hero.ground = None;
    squash.hero.air_steps = 10;
    for _ in 0..7 {
        squash.step(&none);
    }
    vec![
        ("1-start", start),
        ("2-down", tilted),
        (
            "3-burr",
            run(at(Play::start(Area::Island, d), -70, 0, -270, 0, PITCH), 40),
        ),
        (
            "4-tuftling",
            run(at(Play::start(Area::Island, d), -80, 0, -430, 0, PITCH), 20),
        ),
        ("5-squash", squash),
        (
            "6-edge",
            at(Play::start(Area::Island, d), 0, 0, 640, 0, PITCH),
        ),
        (
            "7-far",
            at(
                Play::start(Area::Island, d),
                280,
                150,
                380,
                32_768,
                PITCH_MIN,
            ),
        ),
        (
            "8-trees",
            at(Play::start(Area::Island, d), 380, 0, -300, 40_000, PITCH),
        ),
    ]
}

#[test]
#[ignore]
fn eight_views() {
    let Some(dir) = std::env::var_os("PROTO3D_VIEWS") else {
        println!("PROTO3D_VIEWS not set: nothing written");
        return;
    };
    let mut paint = Box::new(Painter::new());
    let mut f = Box::new(Frame::new());
    let mut text = String::from(
        "# Eight ISLAND views. T triangles, P pixels: MEASURED (PC, the game's own counts).\n",
    );
    for d in Detail::ALL {
        for (name, p) in views(d) {
            paint.picture(&mut f, &p, &Menu::new(), false, 1);
            let st = paint.r.stats;
            text.push_str(&format!(
                "{:4} {name}: T {} P {}\n",
                d.name(),
                st.tris,
                st.pixels
            ));
            let d2 = std::path::Path::new(&dir);
            std::fs::create_dir_all(d2).unwrap();
            let file = format!("{}-{name}.png", d.name().to_lowercase());
            std::fs::write(d2.join(file), png(&f, 2)).unwrap();
        }
    }
    std::fs::write(std::path::Path::new(&dir).join("views.txt"), &text).unwrap();
    print!("{text}");
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
