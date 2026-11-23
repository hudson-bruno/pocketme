//! Bad input must never panic or hang: random descriptors, broken real
//! descriptors, random reports, all the way through to drawing.
//!
//! A tiny xorshift random generator with a fixed seed keeps runs the same.

mod data;

use embedded_graphics::{
    draw_target::DrawTarget, geometry::OriginDimensions, pixelcolor::Rgb565, prelude::*,
};
use pad_view::descriptor::Descriptor;
use pad_view::{Device, View};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn byte(&mut self) -> u8 {
        self.next() as u8
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// A screen that throws pixels away, but checks they are on the screen.
struct Null {
    off_screen: u64,
}
impl OriginDimensions for Null {
    fn size(&self) -> Size {
        Size::new(320, 240)
    }
}
impl DrawTarget for Null {
    type Color = Rgb565;
    type Error = core::convert::Infallible;
    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(
        &mut self,
        pixels: I,
    ) -> Result<(), Self::Error> {
        for Pixel(p, _) in pixels {
            if p.x < 0 || p.y < 0 || p.x >= 320 || p.y >= 240 {
                self.off_screen += 1;
            }
        }
        Ok(())
    }
}

const REAL: [&[u8]; 6] = [
    &data::SWITCH_PRO,
    &data::XBOX_ONE_S_02FD,
    &data::DUALSHOCK4_BT,
    &data::ASUS_ANDROID,
    &data::RAZER_SERVAL,
    &data::GULIKIT_KK2_ANDROID,
];

#[test]
fn random_descriptors() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..20_000 {
        let len = rng.below(300);
        let bytes: Vec<u8> = (0..len).map(|_| rng.byte()).collect();
        let d = Descriptor::parse(&bytes);
        let _ = d.summary();
        for f in d.fields() {
            let _ = f.value(&bytes, 0);
            let _ = f.value(&bytes, f.count);
        }
    }
}

/// Descriptors made of valid-looking items with random values: these get
/// much deeper into the parser than random bytes do.
#[test]
fn random_items() {
    let mut rng = Rng(12345);
    let prefixes: [u8; 22] = [
        0x05, 0x06, 0x09, 0x0A, 0x0B, 0x15, 0x16, 0x25, 0x26, 0x27, 0x75, 0x95, 0x96, 0x85, 0x81,
        0x91, 0xB1, 0xA1, 0xC0, 0x19, 0x29, 0xA4,
    ];
    for _ in 0..20_000 {
        let mut bytes = Vec::new();
        for _ in 0..rng.below(80) {
            let p = if rng.below(40) == 0 {
                0xB4
            } else {
                prefixes[rng.below(prefixes.len())]
            };
            bytes.push(p);
            let size = match p & 3 {
                3 => 4,
                s => s as usize,
            };
            for _ in 0..size {
                bytes.push(rng.byte());
            }
        }
        let d = Descriptor::parse(&bytes);
        let mut v = View::new();
        v.device(&Device {
            name: "",
            ..Default::default()
        });
        v.descriptor(&bytes);
        assert!(d.fields().len() <= pad_view::descriptor::MAX_FIELDS);
        for _ in 0..3 {
            let n = rng.below(70);
            let mut r: Vec<u8> = (0..n).map(|_| rng.byte()).collect();
            if let (Some(first), Some(rep)) = (r.first_mut(), d.reports().first()) {
                *first = rep.id;
            }
            v.report(&r);
        }
    }
}

#[test]
fn broken_real_descriptors() {
    let mut rng = Rng(777);
    for real in REAL {
        // Every cut-off length.
        for n in 0..=real.len() {
            let d = Descriptor::parse(&real[..n]);
            let _ = d.summary();
        }
        // Random bytes changed.
        for _ in 0..2_000 {
            let mut b = real.to_vec();
            for _ in 0..1 + rng.below(6) {
                let i = rng.below(b.len());
                b[i] = rng.byte();
            }
            let mut v = View::new();
            v.descriptor(&b);
            v.report(&[
                0x01, 0x80, 0x80, 0x80, 0x80, 0x08, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ]);
        }
    }
}

#[test]
fn random_reports_with_real_descriptors_and_drawing() {
    let mut rng = Rng(4242);
    let mut screen = Null { off_screen: 0 };
    let names = [
        "Pro Controller",
        "Xbox Wireless Controller",
        "GuliKit Controller XW",
        "",
        "Wireless Controller",
    ];
    for (k, real) in REAL.iter().enumerate() {
        let mut v = View::new();
        v.set_time_ms(0);
        v.status("Connected");
        v.device(&Device {
            name: names[k % names.len()],
            class_of_device: 0x2508,
            ..Default::default()
        });
        v.descriptor(real);
        let d = Descriptor::parse(real);
        let ids: Vec<u8> = d.reports().iter().map(|r| r.id).collect();
        let mut out = [0u8; 64];
        for i in 0..3_000u32 {
            v.set_time_ms(i * 8);
            let n = rng.below(80);
            let mut r: Vec<u8> = (0..n).map(|_| rng.byte()).collect();
            if !r.is_empty() && !ids.is_empty() && rng.below(4) != 0 {
                r[0] = ids[rng.below(ids.len())];
            }
            v.report(&r);
            if let Some(len) = v.next_output(&mut out) {
                assert!(len.len <= out.len());
            }
            if i % 50 == 0 {
                v.draw(&mut screen).unwrap();
            }
            if i % 700 == 0 {
                v.next_page();
            }
            if i == 2000 {
                v.disconnected();
            }
        }
        v.draw(&mut screen).unwrap();
    }
    assert_eq!(screen.off_screen, 0, "nothing drawn outside the screen");
}

#[test]
fn hostile_device_text() {
    let mut v = View::new();
    let long = "é".repeat(100) + "\u{0}\u{7}\n";
    v.status(&long);
    v.device(&Device {
        name: &long,
        class_of_device: 0xFFFF_FFFF,
        vendor_id: Some(0xFFFF),
        version: Some(0),
        ..Default::default()
    });
    let mut s = Null { off_screen: 0 };
    for _ in 0..3 {
        v.draw(&mut s).unwrap();
        v.next_page();
    }
    assert_eq!(s.off_screen, 0);
}

/// Logical ranges at the very ends of 32 bits, on hats, button arrays and
/// triggers: the sums in the decoder must not overflow (the tests run with
/// overflow checks on, so an overflow would panic here).
#[test]
fn extreme_logical_ranges() {
    let big = |tag: u8, v: i32| {
        let b = v.to_le_bytes();
        [tag | 3, b[0], b[1], b[2], b[3]]
    };
    let mut rng = Rng(0xDEAD_BEEF);
    for (min, max) in [
        (i32::MIN, i32::MAX),
        (-1, i32::MAX),
        (i32::MIN, -1),
        (5, 4),
        (1, i32::MAX),
    ] {
        let mut d = vec![0x05, 0x01, 0x09, 0x05, 0xA1, 0x01];
        // Hat, 32 bits.
        d.extend_from_slice(&[0x09, 0x39]);
        d.extend_from_slice(&big(0x14, min));
        d.extend_from_slice(&big(0x24, max));
        d.extend_from_slice(&[0x75, 0x20, 0x95, 0x01, 0x81, 0x42]);
        // Brake and gas, 32 bits.
        d.extend_from_slice(&[
            0x05, 0x02, 0x09, 0xC5, 0x09, 0xC4, 0x75, 0x20, 0x95, 0x02, 0x81, 0x02,
        ]);
        // A button array whose usages start at 0xFFF0, 32-bit slots.
        d.extend_from_slice(&[
            0x05, 0x09, 0x1A, 0xF0, 0xFF, 0x2A, 0xFF, 0xFF, 0x75, 0x20, 0x95, 0x02, 0x81, 0x00,
        ]);
        d.push(0xC0);
        let mut v = View::new();
        v.device(&Device {
            name: "x",
            ..Default::default()
        });
        v.descriptor(&d);
        for _ in 0..500 {
            let r: Vec<u8> = (0..24).map(|_| rng.byte()).collect();
            v.report(&r);
        }
        let mut n = Null { off_screen: 0 };
        for _ in 0..3 {
            v.draw(&mut n).unwrap();
            v.next_page();
        }
    }
    // The summary of a descriptor with a huge button array.
    let d = [
        0x05, 0x09, 0x19, 0x00, 0x2A, 0xFF, 0xFF, 0x75, 0x10, 0x95, 0x01, 0x81, 0x00,
    ];
    let _ = Descriptor::parse(&d).summary();
}
