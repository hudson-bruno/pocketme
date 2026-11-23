//! Known answers: a report goes in, the right buttons and sticks come out.
//!
//! The Switch Pro, Xbox One S (02FD) and Asus reports are from the Android
//! CTS test files (Apache-2.0), where the expected Android key or axis is
//! written next to each report. The others are built by hand from the
//! layouts in SDL's and dekuNukem's notes.

mod data;

use pad_view::decode::{Btn, Layout};
use pad_view::{Device, View};

fn view(name: &str, vid: Option<u16>, pid: Option<u16>, desc: &[u8]) -> View {
    let mut v = View::new();
    v.device(&Device {
        name,
        class_of_device: 0x002508,
        vendor_id: vid,
        product_id: pid,
        ..Default::default()
    });
    if !desc.is_empty() {
        v.descriptor(desc);
    }
    v
}

fn held(v: &View) -> Vec<Btn> {
    Btn::ALL
        .into_iter()
        .filter(|b| v.pad().pressed(*b))
        .collect()
}

// ---------------------------------------------------------------- Switch

fn switch30(r3: u8, r4: u8, r5: u8, left: [u8; 3], right: [u8; 3]) -> Vec<u8> {
    let mut r = vec![0u8; 49];
    r[0] = 0x30;
    r[1] = 0xE2;
    r[2] = 0x40;
    r[3] = r3;
    r[4] = r4;
    r[5] = r5;
    r[6..9].copy_from_slice(&left);
    r[9..12].copy_from_slice(&right);
    r
}
const CENTRE: [u8; 3] = [0xCF, 0xF7, 0x7C];

#[test]
fn switch_full_report_buttons() {
    let mut v = view(
        "Pro Controller",
        Some(0x057E),
        Some(0x2009),
        &data::SWITCH_PRO,
    );
    assert_eq!(v.layout().0, Layout::SwitchPro);
    // From CTS: "Press BUTTON_A" sets r3 = 0x04. Android's BUTTON_A is the
    // bottom button, which Nintendo labels B.
    v.report(&switch30(0x04, 0, 0, CENTRE, CENTRE));
    assert_eq!(held(&v), [Btn::South]);
    v.report(&switch30(0x08, 0, 0, CENTRE, CENTRE));
    assert_eq!(held(&v), [Btn::East]);
    v.report(&switch30(0x01, 0, 0, CENTRE, CENTRE));
    assert_eq!(held(&v), [Btn::West]);
    v.report(&switch30(0x02, 0, 0, CENTRE, CENTRE));
    assert_eq!(held(&v), [Btn::North]);
    v.report(&switch30(0xC0, 0x3F, 0xCF, CENTRE, CENTRE));
    let mut all = held(&v);
    all.sort_by_key(|b| *b as u8);
    let mut want = vec![
        Btn::R,
        Btn::ZR,
        Btn::Minus,
        Btn::Plus,
        Btn::RStick,
        Btn::LStick,
        Btn::Home,
        Btn::Capture,
        Btn::Down,
        Btn::Up,
        Btn::Right,
        Btn::Left,
        Btn::L,
        Btn::ZL,
    ];
    want.sort_by_key(|b| *b as u8);
    assert_eq!(all, want);
    v.report(&switch30(0, 0, 0, CENTRE, CENTRE));
    assert!(held(&v).is_empty(), "release shows");
    // Battery is the top half of byte 2: 0x40 -> 4.
    assert_eq!(v.pad().battery, Some(4));
}

#[test]
fn switch_full_report_sticks() {
    let mut v = view(
        "Pro Controller",
        Some(0x057E),
        Some(0x2009),
        &data::SWITCH_PRO,
    );
    v.report(&switch30(0, 0, 0, CENTRE, CENTRE));
    let p = v.pad();
    assert_eq!((p.lx.raw, p.ly.raw), (1999, 1999));
    assert!(p.lx.norm.abs() < 0.05 && p.ly.norm.abs() < 0.05);
    // CTS "Left stick - press left": cf f1 7c.
    v.report(&switch30(0, 0, 0, [0xCF, 0xF1, 0x7C], CENTRE));
    assert!(v.pad().lx.norm < -0.9, "{}", v.pad().lx.norm);
    // "press right": cf fd 7c.
    v.report(&switch30(0, 0, 0, [0xCF, 0xFD, 0x7C], CENTRE));
    assert!(v.pad().lx.norm > 0.9);
    // "press up": cf f7 da. Up is a big number on the wire; on screen y
    // grows downwards, so up must be negative.
    v.report(&switch30(0, 0, 0, [0xCF, 0xF7, 0xDA], CENTRE));
    assert!(v.pad().ly.norm < -0.8, "{}", v.pad().ly.norm);
    // "press down": cf f7 17.
    v.report(&switch30(0, 0, 0, [0xCF, 0xF7, 0x17], CENTRE));
    assert!(v.pad().ly.norm > 0.8);
    // Right stick, "press left": cf f1 7c in bytes 9-11.
    v.report(&switch30(0, 0, 0, CENTRE, [0xCF, 0xF1, 0x7C]));
    assert!(v.pad().rx.norm < -0.9);
    assert!(v.pad().lx.norm.abs() < 0.05);
}

#[test]
fn switch_motion_in_0x30() {
    let mut v = view("Pro Controller", None, None, &[]);
    let mut r = switch30(0, 0, 0, CENTRE, CENTRE);
    r[13..15].copy_from_slice(&(-1000i16).to_le_bytes());
    r[23..25].copy_from_slice(&(1234i16).to_le_bytes());
    v.report(&r);
    // The first of the three samples (bytes 13-24).
    assert_eq!(
        v.pad().motion.map(|m| (m.samples[0][0], m.samples[0][5])),
        Some((-1000, 1234))
    );
}

#[test]
fn switch_simple_report_0x3f() {
    // No descriptor, only the name: the GuliKit in Switch mode before the
    // start-up conversation.
    let mut v = view("Pro Controller", None, None, &[]);
    // A (right button, 0x02) + Home (0x10), hat 8 = centred, sticks centred.
    v.report(&[
        0x3F, 0x02, 0x10, 0x08, 0x00, 0x80, 0x00, 0x80, 0x00, 0x80, 0x00, 0x80,
    ]);
    assert_eq!(held(&v), [Btn::East, Btn::Home]);
    assert!(v.pad().lx.norm.abs() < 0.01);
    // Hat 2 = right; left stick full left and up.
    v.report(&[
        0x3F, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x80,
    ]);
    assert_eq!(held(&v), [Btn::Right]);
    assert_eq!(v.pad().hat, Some(2));
    assert!(v.pad().lx.norm < -0.99 && v.pad().ly.norm < -0.99);
}

#[test]
fn switch_chosen_from_first_report_alone() {
    let mut v = view("", None, None, &[]);
    assert_eq!(v.layout().0, Layout::Unknown);
    v.report(&[
        0x3F, 0x01, 0x00, 0x08, 0x00, 0x80, 0x00, 0x80, 0x00, 0x80, 0x00, 0x80,
    ]);
    assert_eq!(v.layout().0, Layout::SwitchPro);
    assert_eq!(held(&v), [Btn::South]);
}

// ------------------------------------------------------------------ Xbox

fn xbox17(b14: u8, b15: u8) -> Vec<u8> {
    // From CTS: sticks at ff 7f (centre), triggers 0, hat 0.
    let mut r = vec![
        0x01, 0xFF, 0x7F, 0xFF, 0x7F, 0xFF, 0x7F, 0xFF, 0x7F, 0, 0, 0, 0, 0, b14, b15, 0,
    ];
    r.truncate(17);
    r
}

#[test]
fn xbox_newer_firmware_17_bytes() {
    let mut v = view(
        "Xbox Wireless Controller",
        Some(0x045E),
        Some(0x02FD),
        &data::XBOX_ONE_S_02FD,
    );
    assert_eq!(v.layout().0, Layout::Xbox);
    for (b14, b15, want) in [
        (0x01, 0, Btn::South),
        (0x02, 0, Btn::East),
        (0x08, 0, Btn::West),
        (0x10, 0, Btn::North),
        (0x40, 0, Btn::L),
        (0x80, 0, Btn::R),
        (0, 0x20, Btn::LStick),
        (0, 0x40, Btn::RStick),
        (0, 0x08, Btn::Plus),
        (0, 0x10, Btn::Home),
    ] {
        v.report(&xbox17(b14, b15));
        assert_eq!(held(&v), [want], "b14 {b14:02X} b15 {b15:02X}");
    }
    v.report(&xbox17(0, 0));
    assert!(held(&v).is_empty());
    assert!(v.pad().lx.norm.abs() < 0.01);
}

#[test]
fn xbox_gulikit_16_bytes() {
    // GuliKit XInput / Android mode: the 307-byte descriptor, 16-byte report.
    let mut v = view(
        "GuliKit Controller XW",
        None,
        None,
        &data::GULIKIT_KK2_ANDROID,
    );
    assert_eq!(v.layout().0, Layout::Xbox);
    assert_eq!(v.mode_guess(), "XInput (Windows) mode");
    // Left stick full right (ffff), right stick full up (0000), left
    // trigger full (0x3ff), right trigger 0, hat 3 = right, buttons:
    // byte 14 = 0x41 (A + View), byte 15 = 0x02 (right stick click).
    let r = [
        0x01, 0xFF, 0xFF, 0x00, 0x80, 0x00, 0x80, 0x00, 0x00, 0xFF, 0x03, 0x00, 0x00, 0x03, 0x41,
        0x02,
    ];
    v.report(&r);
    let p = v.pad();
    assert!(p.lx.norm > 0.99);
    assert!(p.ry.norm < -0.99);
    assert_eq!(p.lt.value, 1023);
    assert_eq!(p.rt.value, 0);
    assert_eq!(p.hat, Some(2));
    let mut h = held(&v);
    h.sort_by_key(|b| *b as u8);
    let mut want = vec![Btn::South, Btn::Minus, Btn::RStick, Btn::Right, Btn::ZL];
    want.sort_by_key(|b| *b as u8);
    assert_eq!(h, want);
    // The Guide button arrives alone in report 2.
    v.report(&[0x02, 0x01]);
    assert!(v.pad().pressed(Btn::Home));
    v.report(&[0x02, 0x00]);
    assert!(!v.pad().pressed(Btn::Home));
    // Battery, report 4.
    v.report(&[0x04, 0x55]);
    assert_eq!(v.pad().battery, Some(0x55));
}

#[test]
fn android_mode_name() {
    let v = view(
        "Xbox Wireless Controller",
        None,
        None,
        &data::GULIKIT_KK2_ANDROID,
    );
    assert_eq!(v.layout().0, Layout::Xbox);
    assert_eq!(v.mode_guess(), "Android/iOS mode (or a real Xbox)");
}

// ------------------------------------------------- Android (generic) pad

fn asus(b1: u8, b2: u8, axes: [u8; 4], brake: u8, gas: u8) -> [u8; 9] {
    [0x01, b1, b2, axes[0], axes[1], axes[2], axes[3], brake, gas]
}

#[test]
fn asus_android_layout() {
    let mut v = view(
        "Asus Gamepad",
        Some(0x0B05),
        Some(0x4500),
        &data::ASUS_ANDROID,
    );
    assert_eq!(v.layout().0, Layout::Android);
    assert!(
        !v.layout().0.is_guess(),
        "Android's button order is published, not guessed"
    );
    let c = [0x80; 4];
    // Hat in the top half of byte 2; 8 = centred.
    v.report(&asus(0x01, 0x80, c, 0, 0));
    assert_eq!(held(&v), [Btn::South]);
    v.report(&asus(0x02, 0x80, c, 0, 0));
    assert_eq!(held(&v), [Btn::East]);
    v.report(&asus(0x04, 0x80, c, 0, 0));
    assert_eq!(held(&v), [Btn::West]);
    v.report(&asus(0x08, 0x80, c, 0, 0));
    assert_eq!(held(&v), [Btn::North]);
    // From CTS: 01 00 81 is BUTTON_MODE (Home), 01 00 82 is BACK.
    v.report(&asus(0x00, 0x81, c, 0, 0));
    assert_eq!(held(&v), [Btn::Home]);
    v.report(&asus(0x00, 0x82, c, 0, 0));
    assert_eq!(held(&v), [Btn::Minus]);
    // From CTS: hat 0x60 is left, 0x20 is right.
    v.report(&asus(0, 0x60, c, 0, 0));
    assert_eq!(held(&v), [Btn::Left]);
    v.report(&asus(0, 0x20, c, 0, 0));
    assert_eq!(held(&v), [Btn::Right]);
    // Brake (left trigger) full, gas half.
    v.report(&asus(0, 0x80, [0xFF, 0x00, 0x80, 0x80], 0xFF, 0x80));
    let p = v.pad();
    assert_eq!(p.lt.value, 1023);
    assert!((500..530).contains(&p.rt.value), "{}", p.rt.value);
    assert!(p.lx.norm > 0.99 && p.ly.norm < -0.99);
    assert!(p.rx.norm.abs() < 0.01);
    assert!(v.pad().pressed(Btn::ZL));
}

// --------------------------------------------- DualShock 4 (generic path)

#[test]
fn dualshock4_simple_report() {
    let mut v = view(
        "Wireless Controller",
        Some(0x054C),
        Some(0x05C4),
        &data::DUALSHOCK4_BT,
    );
    assert_eq!(v.layout().0, Layout::PlayStation);
    // Report 1: X Y Z Rz, then hat (low 4 bits) and buttons 1-4 (top 4
    // bits), buttons 5-12, 13-14, then L2 (Rx) and R2 (Ry).
    // Cross is button 2 -> bit 5 of byte 5.
    v.report(&[0x01, 0x80, 0x80, 0x80, 0x80, 0x28, 0x00, 0x00, 0x00, 0x00]);
    assert_eq!(held(&v), [Btn::South]);
    assert_eq!(v.pad().hat, None);
    // Hat 4 (down), right stick (Z, Rz) full left, L2 analog full.
    v.report(&[0x01, 0x80, 0x80, 0x00, 0x80, 0x04, 0x00, 0x00, 0xFF, 0x00]);
    assert_eq!(v.pad().hat, Some(4));
    assert!(v.pad().rx.norm < -0.99);
    assert_eq!(v.pad().lt.value, 1023);
}

// ---------------------------------------------------- nothing is known

#[test]
fn unknown_pad_reports_are_kept_raw() {
    let mut v = view("", None, None, &[]);
    v.report(&[0x07, 1, 2, 3]);
    assert_eq!(v.layout().0, Layout::Unknown);
    assert!(held(&v).is_empty());
}

// ------------------------------------------- names that do not fit

#[test]
fn a_borrowed_name_does_not_beat_the_descriptor() {
    // Named like the GuliKit in XInput mode, but describing itself as a
    // DualShock 4: the descriptor wins and the screen says it is a guess.
    let v = view("GuliKit Controller XW", None, None, &data::DUALSHOCK4_BT);
    assert_eq!(v.layout().0, Layout::Generic);
    // Named "Pro Controller" with an Android descriptor: not a Switch pad.
    let v = view("Pro Controller", None, None, &data::ASUS_ANDROID);
    assert_eq!(v.layout().0, Layout::Android);
    // With no descriptor at all, the name is all there is.
    let v = view("GuliKit Controller XW", None, None, &[]);
    assert_eq!(v.layout().0, Layout::Xbox);
}

#[test]
fn xbox_guide_report_and_report_one_do_not_fight() {
    // 17-byte report 1 (Home at byte 15 bit 0x10) on a pad that also sends
    // the Xbox button alone in report 2: report 1's empty bit must not
    // clear Home while report 2 says it is held.
    let mut v = view(
        "Xbox Wireless Controller",
        Some(0x045E),
        Some(0x02FD),
        &data::XBOX_ONE_S_02FD,
    );
    v.report(&xbox17(0, 0x10));
    assert!(
        v.pad().pressed(Btn::Home),
        "before any report 2, report 1 carries Home"
    );
    v.report(&[0x02, 0x01]);
    v.report(&xbox17(0, 0));
    assert!(v.pad().pressed(Btn::Home), "report 2 owns Home now");
    v.report(&[0x02, 0x00]);
    assert!(!v.pad().pressed(Btn::Home));
}

#[test]
fn switch_simple_report_sticks_and_zr_in_byte_two() {
    let mut v = view(
        "Pro Controller",
        Some(0x057E),
        Some(0x2009),
        &data::SWITCH_PRO,
    );
    // 0x3F: sticks are 16-bit, centre 0x8000, bigger y = down (as SDL
    // reads it); hat 8 = centred. ZR given in byte 2 bit 0x80.
    v.report(&[
        0x3F, 0x00, 0x80, 0x08, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x80, 0x00, 0x80,
    ]);
    let p = v.pad();
    assert!(p.lx.norm > 0.99 && p.ly.norm < -0.99);
    assert_eq!(p.hat, None);
    assert_eq!(held(&v), [Btn::ZR]);
}

#[test]
fn gulikit_wired_dinput_ids_use_android_order_with_home_16() {
    // A made-up descriptor: 16 buttons, X Y Z Rz, hat. With the KK2 Pro's
    // wired DInput ids the Android order is used, Home on button 16.
    let desc = [
        0x05, 0x01, 0x09, 0x05, 0xA1, 0x01, // Generic Desktop, Game Pad, Collection
        0x05, 0x09, 0x19, 0x01, 0x29, 0x10, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x10, 0x81,
        0x02, // 16 buttons
        0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x09, 0x32, 0x09, 0x35, 0x15, 0x00, 0x26, 0xFF, 0x00,
        0x75, 0x08, 0x95, 0x04, 0x81, 0x02, // 4 axes
        0x09, 0x39, 0x15, 0x00, 0x25, 0x07, 0x75, 0x08, 0x95, 0x01, 0x81,
        0x42, // hat with null state
        0xC0,
    ];
    let mut v = view("GuliKit Controller", Some(0x0079), Some(0x181C), &desc);
    assert_eq!(v.layout().0, Layout::Android);
    v.report(&[0x00, 0x80, 0x80, 0x80, 0x80, 0x80, 0x08]);
    assert_eq!(held(&v), [Btn::Home]);
    v.report(&[0x08, 0x00, 0x80, 0x80, 0x80, 0x80, 0x08]);
    assert_eq!(
        held(&v),
        [Btn::West],
        "button 4 is X (west) in Android order"
    );
}
