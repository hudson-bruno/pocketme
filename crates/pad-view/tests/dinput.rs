//! The GuliKit KingKong 2 Pro in DInput mode, with its real descriptor and
//! real reports from the owner's 2026-11-16 live test; the unmapped-input
//! tracker; the serial trace decisions.

mod data;

use pad_view::decode::{Btn, Layout};
use pad_view::trace::{Change, FULL_MS, Kind, MAX_LINES_PER_S, Tracer};
use pad_view::{Device, View};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn dinput() -> View {
    let mut v = View::new();
    v.set_time_ms(0);
    v.device(&Device {
        name: "GuliKit Controller DW",
        class_of_device: 0x002508,
        vendor_id: Some(0x1949),
        product_id: Some(0x0402),
        version: Some(0x011B),
        vendor_id_source: Some(2),
    });
    v.descriptor(&data::GULIKIT_KK2_DINPUT);
    v
}

fn held(v: &View) -> Vec<Btn> {
    Btn::ALL
        .into_iter()
        .filter(|b| v.pad().pressed(*b))
        .collect()
}

fn held_text(v: &View) -> String {
    let mut s = String::new();
    v.write_held(&mut s);
    s
}

fn unmapped_text(v: &View) -> String {
    let mut s = String::new();
    v.write_unmapped(&mut s, "|");
    s
}

const IDLE: &str = "07807f807f000f0000";
const LT: &str = "07807f807f000f0400";
const RT: &str = "07807f807f000f0800";

#[test]
fn dinput_layout_and_on_off_triggers() {
    let mut v = dinput();
    assert_eq!(v.layout().0, Layout::Generic);
    // Known from the descriptor alone, before any press.
    assert!(v.triggers_on_off());
    v.report(&hex(IDLE));
    assert_eq!(held(&v), vec![]);
    assert_eq!(held_text(&v), "-");
    assert_eq!(v.pad().lt.value, 0);
    v.report(&hex(LT));
    assert_eq!(held(&v), vec![Btn::ZL]);
    assert_eq!(v.pad().lt.value, 1023, "LT bar full while held");
    assert_eq!(v.pad().rt.value, 0);
    assert!(v.pad().lt.present && !v.pad().lt.analog);
    v.report(&hex(RT));
    assert_eq!(held(&v), vec![Btn::ZR]);
    assert_eq!(v.pad().rt.value, 1023);
    assert_eq!(v.pad().lt.value, 0);
    v.report(&hex("07807f807f00070000"));
    assert_eq!(v.pad().hat, Some(7));
    v.report(&hex("07807fa46a000f0000"));
    assert!(v.pad().lx.norm > 0.2, "{:?}", v.pad().lx);
    assert_eq!(unmapped_text(&v), "", "nothing unmapped in real reports");
}

#[test]
fn other_layouts_are_not_on_off() {
    // An analog-trigger pad: the Xbox One S descriptor.
    let mut v = View::new();
    v.device(&Device {
        name: "Xbox Wireless Controller",
        vendor_id: Some(0x045E),
        product_id: Some(0x02FD),
        ..Default::default()
    });
    v.descriptor(&data::XBOX_ONE_S_02FD);
    assert!(!v.triggers_on_off());
    // The DualShock 4 has trigger axes too.
    let mut v = View::new();
    v.device(&Device {
        name: "Wireless Controller",
        ..Default::default()
    });
    v.descriptor(&data::DUALSHOCK4_BT);
    assert!(!v.triggers_on_off());
}

#[test]
fn dinput_vendor_bit_is_unmapped() {
    let mut v = dinput();
    v.report(&hex(IDLE));
    v.report(&hex("07807f807f000f0001"));
    assert_eq!(held(&v), vec![]);
    assert_eq!(unmapped_text(&v), "07 b8.0 vendor FF00:1");
    assert!(v.last_change().digital);
    v.report(&hex(IDLE));
    assert_eq!(unmapped_text(&v), "");
    assert!(v.last_change().digital);
}

#[test]
fn dinput_consumer_report() {
    let mut v = dinput();
    v.report(&hex(IDLE));
    // AC Home (bit 7) is the Home button.
    v.report(&[0x02, 0x80]);
    assert_eq!(held(&v), vec![Btn::Home]);
    assert_eq!(unmapped_text(&v), "");
    v.report(&[0x02, 0x00]);
    assert_eq!(held(&v), vec![]);
    // AC Back (bit 6) is not guessed: this pad has a button for "-".
    v.report(&[0x02, 0x40]);
    assert_eq!(held(&v), vec![]);
    assert_eq!(unmapped_text(&v), "02 b1.6 AC Back");
    v.report(&[0x02, 0x00]);
    v.report(&[0x02, 0x01]);
    assert_eq!(unmapped_text(&v), "02 b1.0 Play/Pause");
    v.report(&[0x02, 0x00]);
    assert_eq!(unmapped_text(&v), "");

    // The very first consumer report already has its key on (the pad
    // sends report 02 only on a press): still shown as on.
    let mut v = dinput();
    v.report(&hex(IDLE));
    v.report(&[0x02, 0x40]);
    assert!(v.last_change().first && v.last_change().digital);
    assert_eq!(unmapped_text(&v), "02 b1.6 AC Back");
    v.report(&[0x02, 0x00]);
    assert_eq!(unmapped_text(&v), "");
}

#[test]
fn usage_zero_byte_value() {
    let mut v = dinput();
    v.report(&hex(IDLE));
    v.report(&hex("07807f807f120f0000"));
    assert_eq!(unmapped_text(&v), "07 b5 usage 0=18");
}

#[test]
fn hand_decoded_layouts_use_known_bits() {
    // Xbox (GuliKit XInput mode), 16-byte report 01: byte 15 bit 2 is not
    // read by the decoder.
    let mut v = View::new();
    v.device(&Device {
        name: "GuliKit Controller XW",
        ..Default::default()
    });
    v.descriptor(&data::GULIKIT_KK2_ANDROID);
    assert_eq!(v.layout().0, Layout::Xbox);
    let mut r = [
        0x01, 0x00, 0xC0, 0x00, 0x40, 0x00, 0x80, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ];
    v.report(&r);
    r[15] = 0x04;
    v.report(&r);
    let s = unmapped_text(&v);
    assert!(s.starts_with("01 b15.2 "), "{s}");
    println!("xbox: {s}");
    r[15] = 0x00;
    // A standard button (A) is not unmapped.
    r[14] = 0x01;
    v.report(&r);
    assert_eq!(unmapped_text(&v), "");
    assert_eq!(held(&v), vec![Btn::South]);

    // Switch simple report 3F: byte 2 bit 6 is not read.
    let mut v = View::new();
    v.device(&Device {
        name: "Pro Controller",
        vendor_id: Some(0x057E),
        product_id: Some(0x2009),
        ..Default::default()
    });
    v.descriptor(&data::SWITCH_PRO);
    assert_eq!(v.layout().0, Layout::SwitchPro);
    let mut r = [
        0x3F, 0x00, 0x00, 0x08, 0x00, 0x80, 0x00, 0x80, 0x00, 0x80, 0x00, 0x80,
    ];
    v.report(&r);
    r[2] = 0x40;
    v.report(&r);
    let s = unmapped_text(&v);
    assert!(s.starts_with("3F b2.6 "), "{s}");
    println!("switch: {s}");
}

/// A short press between two screen snapshots still shows once.
#[test]
fn short_press_is_latched() {
    let mut v = dinput();
    let mut f = NullTarget;
    v.report(&hex(IDLE));
    v.draw(&mut f).unwrap();
    v.set_time_ms(100);
    v.report(&hex("07807f807f000f0001"));
    v.set_time_ms(120);
    v.report(&hex(IDLE));
    v.set_time_ms(300);
    v.draw(&mut f).unwrap();
    let (items, total) = v.unmapped_shown();
    assert_eq!(total, 1);
    let mut s = String::new();
    items[0].label(&mut s);
    assert_eq!(s, "07 b8.0 vendor FF00:1");
    // Gone at the next snapshot.
    v.set_time_ms(600);
    v.draw(&mut f).unwrap();
    assert_eq!(v.unmapped_shown().1, 0);
}

/// A byte that changes all the time changes the screen at most about four
/// times a second.
#[test]
fn noisy_byte_is_rate_limited() {
    let mut v = dinput();
    let mut f = NullTarget;
    let before = v.unmapped_changes().0;
    for i in 0..1000u32 {
        v.set_time_ms(i * 5);
        let mut r = hex(IDLE);
        r[5] = (i % 251) as u8;
        v.report(&r);
        v.draw(&mut f).unwrap();
    }
    let (screen, reports) = v.unmapped_changes();
    // 5 seconds.
    assert!(screen - before <= 21, "{}", screen - before);
    // A value counts as on or off, not as a new thing at each step: it
    // turns on and off only when it leaves or reaches its base (0 here,
    // every 251 reports), so it does not make every report a trace line.
    assert!(reports <= 10, "{reports}");
}

// ---------------------------------------------------------------- trace

fn ch(digital: bool, analog: bool, same: bool) -> Change {
    Change {
        id: 7,
        len: 9,
        same,
        first: false,
        digital,
        analog,
    }
}

#[test]
fn trace_change_same_analog() {
    let mut t = Tracer::new();
    let first = Change {
        first: true,
        ..ch(false, false, false)
    };
    assert_eq!(t.report(0, &first).kind, Some(Kind::Change));
    // Stick wobble every 8 ms: no line.
    for i in 1..100 {
        assert_eq!(t.report(i * 8, &ch(false, true, false)).kind, None);
    }
    // Identical report after a 200 ms silence.
    let d = t.report(792 + 200, &ch(false, false, true));
    assert_eq!(d.kind, Some(Kind::Same));
    assert_eq!(d.gap_ms, Some(200));
    // Identical report quickly after: nothing.
    assert_eq!(t.report(1000, &ch(false, false, true)).kind, None);
    // Analog move after a silence.
    assert_eq!(
        t.report(1400, &ch(false, true, false)).kind,
        Some(Kind::Analog)
    );
    assert_eq!(t.report(1408, &ch(false, true, false)).kind, None);
    // A digital change always.
    assert_eq!(
        t.report(1416, &ch(true, false, false)).kind,
        Some(Kind::Change)
    );
}

#[test]
fn trace_cap_and_skipped_count() {
    let mut t = Tracer::new();
    let mut lines = 0;
    // 100 button changes in one second.
    for i in 0..100 {
        if t.report(i * 10, &ch(true, false, false)).kind.is_some() {
            lines += 1;
        }
    }
    assert_eq!(lines, MAX_LINES_PER_S);
    let d = t.report(1000, &ch(true, false, false));
    assert_eq!(d.skipped, 100 - MAX_LINES_PER_S);
    assert_eq!(d.kind, Some(Kind::Change));
    assert_eq!(t.total_skipped, 100 - MAX_LINES_PER_S);
}

#[test]
fn trace_full_lasts_ten_seconds() {
    let mut t = Tracer::new();
    t.report(
        0,
        &Change {
            first: true,
            ..ch(false, false, false)
        },
    );
    t.start_full(10);
    assert!(t.full_active(10));
    let mut lines = 0;
    let mut ended = false;
    let mut ms = 18;
    while ms < 12_000 {
        let d = t.report(ms, &ch(false, true, false));
        if d.kind.is_some() {
            lines += 1;
        }
        ended |= d.full_ended;
        ms += 8;
    }
    assert!(ended);
    assert!(!t.full_active(12_000));
    // 10 s at the cap, nothing after.
    assert_eq!(lines, (FULL_MS / 1000) * MAX_LINES_PER_S);
    assert!(t.total_skipped > 0);
}

#[test]
fn trace_from_view() {
    let mut v = dinput();
    v.report(&hex(IDLE));
    assert!(v.last_change().first);
    v.report(&hex(IDLE));
    let c = v.last_change();
    assert!(c.same && !c.digital && !c.analog && !c.first);
    v.report(&hex("07807fa46a000f0000"));
    let c = v.last_change();
    assert!(c.analog && !c.digital);
    v.report(&hex("07807fa46a000f0400"));
    assert!(v.last_change().digital);
    assert_eq!(held_text(&v), "LT");
}

/// A draw target that throws everything away.
struct NullTarget;

impl embedded_graphics::geometry::OriginDimensions for NullTarget {
    fn size(&self) -> embedded_graphics::geometry::Size {
        embedded_graphics::geometry::Size::new(320, 240)
    }
}

impl embedded_graphics::draw_target::DrawTarget for NullTarget {
    type Color = embedded_graphics::pixelcolor::Rgb565;
    type Error = core::convert::Infallible;
    fn draw_iter<I: IntoIterator<Item = embedded_graphics::Pixel<Self::Color>>>(
        &mut self,
        _p: I,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}
