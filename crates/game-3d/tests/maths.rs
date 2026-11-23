//! Whole-number maths (from KESTREL's tests), the sums, model helpers
//! and the shading rules.
//!
//! Tests may use floating point; the library never does.

mod common;

use common::{CUBES, Dice, cube_raw, cube_verts, lit};
use game_3d::{
    Checked, Flat, Lit, Mat3, ONE14, Shade, Shading, Steps, Sums, Wrapping, cos, cos_fine, isqrt,
    orient_by_middle, sin, sin_fine,
};

#[test]
fn sine_cosine_and_turnings_are_accurate() {
    for a in 0..1024u32 {
        let ang = (a * 64) as u16;
        let want = (f64::from(ang) / 65536.0 * std::f64::consts::TAU).sin() * 16384.0;
        assert!(
            (f64::from(sin(ang)) - want).abs() <= 1.0,
            "sin({a}/1024 turn)"
        );
        let s = i64::from(sin(ang));
        let c = i64::from(cos(ang));
        assert!((s * s + c * c - 16384 * 16384).abs() < 2 * 16384);
    }
    let mut d = Dice(0x7071);
    for _ in 0..2000 {
        let m = Mat3::rotation(d.next() as u16, d.next() as u16, d.next() as u16);
        // Rows one long and at right angles: the turn back is the
        // transpose.
        let p = m.mul(&m.transposed());
        for (r, row) in p.0.iter().enumerate() {
            for (c, &v) in row.iter().enumerate() {
                let want = if r == c { 16384 } else { 0 };
                assert!((v - want).abs() <= 12, "{:?}", p.0);
            }
        }
    }
}

/// KESTREL's square root (Newton's steps), the reference.
fn newton(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    let mut x = 1u64 << (64 - n.leading_zeros()).div_ceil(2);
    loop {
        let y = (x + n / x) / 2;
        if y >= x {
            return x;
        }
        x = y;
    }
}

#[test]
fn the_square_root_is_exact() {
    for n in 0..20_000u64 {
        let r = isqrt(n);
        assert!(r * r <= n && (r + 1) * (r + 1) > n, "isqrt({n}) = {r}");
    }
    let mut d = Dice(0x5157);
    for _ in 0..200_000 {
        let n = d.next() >> (d.next() % 64);
        let r = u128::from(isqrt(n));
        assert!(
            r * r <= u128::from(n) && (r + 1) * (r + 1) > u128::from(n),
            "isqrt({n})"
        );
        assert_eq!(isqrt(n), newton(n), "isqrt({n}) differs from KESTREL's");
    }
    for k in 0..64 {
        for e in [-1i64, 0, 1] {
            let n = (1u64 << k).wrapping_add(e as u64);
            assert_eq!(isqrt(n), newton(n), "isqrt({n})");
        }
    }
    assert_eq!(isqrt(u64::MAX), u64::from(u32::MAX));
}

#[test]
fn wrapping_and_checked_matrices_agree() {
    let mut d = Dice(0x3A7);
    for _ in 0..5000 {
        let a = Mat3::rotation(d.next() as u16, d.next() as u16, d.next() as u16);
        let b = Mat3::rotation(d.next() as u16, d.next() as u16, d.next() as u16);
        let v = [
            d.range(-1 << 20, 1 << 20),
            d.range(-1 << 20, 1 << 20),
            d.range(-1 << 20, 1 << 20),
        ];
        let s = d.range(1, 4000);
        assert_eq!(a.mul_with::<Checked>(&b), a.mul_with::<Wrapping>(&b));
        assert_eq!(a.apply_with::<Checked>(v), a.apply_with::<Wrapping>(v));
        assert_eq!(a.scaled_with::<Checked>(s), a.scaled_with::<Wrapping>(s));
    }
}

#[test]
fn checked_sums_stop_and_wrapping_sums_wrap() {
    // Checked: a sum that runs over stops (overflow checks are on in
    // the tests, as on the chip).
    let stop = std::panic::catch_unwind(|| Checked::add(i32::MAX, std::hint::black_box(1)));
    assert!(stop.is_err());
    // Wrapping: the same sum wraps round; in debug builds (these tests)
    // the debug check still stops it, so a range mistake is found here.
    let wrapped = std::panic::catch_unwind(|| Wrapping::add(i32::MAX, std::hint::black_box(1)));
    if cfg!(debug_assertions) {
        assert!(wrapped.is_err());
    } else {
        assert_eq!(wrapped.ok(), Some(i32::MIN));
    }
    // The two divisions round down alike.
    let mut d = Dice(0xD1F);
    for _ in 0..100_000 {
        let n = d.range(-1 << 28, 1 << 28);
        let q = d.range(1, 1 << 16);
        assert_eq!(Checked::floor_div(n, q), Wrapping::floor_div(n, q));
    }
}

#[test]
fn both_ways_of_turning_faces_outwards_agree_on_cubes() {
    // A cube's corners are far from its middle, so judging from the
    // first corner or from the face's middle gives the same faces.
    let a = game_3d::orient(&cube_verts(), &cube_raw());
    let b = orient_by_middle(&cube_verts(), &cube_raw());
    assert_eq!(a, b);
    assert_eq!(CUBES.faces, &a[..]);
}

#[test]
fn the_shading_rule_lights_fogs_glows_and_clears() {
    let sh = lit();
    let up = [0, ONE14, 0];
    let down = [0, -ONE14, 0];
    // Lit from above: the top is brighter than the bottom.
    let top = sh.shade(1, up, 0);
    let bottom = sh.shade(1, down, 0);
    assert!(top < bottom, "{top} {bottom}");
    assert_eq!(bottom, sh.dark_side);
    // Fog: farther is darker, up to fog_shades steps.
    let near = sh.shade(1, up, sh.fog_from);
    let far = sh.shade(1, up, sh.fog_to + 1000);
    assert_eq!(far, (near + sh.fog_shades).min(sh.shades - 1));
    // Material 3 glows: no light, but fog.
    assert_eq!(sh.shade(3, down, 0), 0);
    assert_eq!(sh.shade(3, down, sh.fog_to), sh.fog_shades);
    // Cleared materials see no fog.
    let clear = Shading {
        clear: 1 << 1,
        ..sh
    };
    assert_eq!(clear.shade(1, up, sh.fog_to), clear.shade(1, up, 0));
    // Without fog (Lit<false>) the same as fog turned off.
    let nofog: Lit<false> = Lit {
        mats: sh.mats,
        shades: sh.shades,
        light: sh.light,
        glow: sh.glow,
        fog_from: sh.fog_from,
        fog_to: sh.fog_to,
        fog_shades: sh.fog_shades,
        clear: sh.clear,
        dark_side: sh.dark_side,
    };
    let off = Shading { fog_to: 0, ..sh };
    let mut d = Dice(0x5AD);
    for _ in 0..2000 {
        let n = [
            d.range(-ONE14, ONE14),
            d.range(-ONE14, ONE14),
            d.range(-ONE14, ONE14),
        ];
        let mat = d.range(0, 3) as u8;
        let depth = d.range(0, 60 * 256);
        assert_eq!(
            nofog.paint::<Checked>(mat, n, depth),
            off.paint::<Checked>(mat, n, depth)
        );
        assert_eq!(
            sh.paint::<Checked>(mat, n, depth),
            sh.paint::<Wrapping>(mat, n, depth)
        );
        // Palette number = shade * mats + material.
        assert_eq!(
            sh.paint::<Checked>(mat, n, depth),
            sh.colour(mat, sh.shade(mat, n, depth))
        );
    }
}

#[test]
fn camber_steps_and_flat_colours() {
    let look = Steps {
        base: [10, 20, 30, 40, 50, 60, 70, 80],
        glow: 1 << 2,
        light: [0, -ONE14, 0],
    };
    // Straight on: step 0; turned away: step 3; glowing: always 0.
    assert_eq!(look.paint::<Checked>(1, [0, ONE14, 0], 0), 20);
    assert_eq!(look.paint::<Checked>(1, [0, -ONE14, 0], 0), 23);
    assert_eq!(look.paint::<Checked>(2, [0, -ONE14, 0], 0), 30);
    // Half lit: step between.
    let s = look.paint::<Wrapping>(4, [0, ONE14 / 2, 0], 0);
    assert!((50..=53).contains(&s));
    assert_eq!(Flat.paint::<Checked>(7, [0; 3], 99), 7);
}

#[test]
fn the_smooth_sine_meets_the_table_and_goes_straight_between() {
    for a in 0..=u16::MAX {
        let want = (f64::from(a) / 65536.0 * std::f64::consts::TAU).sin() * 16384.0;
        let s = sin_fine(a);
        assert!((f64::from(s) - want).abs() <= 2.0, "sin_fine({a}) = {s}");
        // On the table's steps it is the table.
        if a % 64 == 0 {
            assert_eq!(s, sin(a), "{a}");
            assert_eq!(cos_fine(a), cos(a), "{a}");
        }
        assert_eq!(cos_fine(a), sin_fine(a.wrapping_add(16_384)));
    }
    // It never goes backwards within a rising quarter.
    for a in 1..16_384u16 {
        assert!(sin_fine(a) >= sin_fine(a - 1), "{a}");
    }
}

#[test]
fn smooth_turnings_are_turnings_and_columns_are_columns() {
    let mut d = Dice(0x7072);
    for _ in 0..2000 {
        let (y, p, r) = (d.next() as u16, d.next() as u16, d.next() as u16);
        let m = Mat3::rotation_fine(y, p, r);
        let q = m.mul(&m.transposed());
        for (i, row) in q.0.iter().enumerate() {
            for (j, &v) in row.iter().enumerate() {
                let want = if i == j { 16384 } else { 0 };
                assert!((v - want).abs() <= 12, "{:?}", q.0);
            }
        }
        // On the table's steps it is the stepped turning.
        let (y, p, r) = (y & !63, p & !63, r & !63);
        assert_eq!(Mat3::rotation_fine(y, p, r), Mat3::rotation(y, p, r));
        for c in 0..3 {
            assert_eq!(m.col(c), [m.0[0][c], m.0[1][c], m.0[2][c]]);
        }
    }
}
