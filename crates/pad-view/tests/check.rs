//! Second-reader checks (round 2): the three real descriptors and the
//! real reports of every mode from the owner's live test, through the
//! decoder, the unmapped-input tracker and the trace decisions.

mod live1;

use embedded_graphics::{
    draw_target::DrawTarget, geometry::OriginDimensions, pixelcolor::Rgb565, prelude::*,
};
use pad_view::decode::{Btn, Layout};
use pad_view::trace::{Kind, MAX_LINES_PER_S, Tracer};
use pad_view::{Device, View};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

struct Null;
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
        p: I,
    ) -> Result<(), Self::Error> {
        for _ in p {}
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Xw,
    Android,
    Dinput,
    Switch,
}

/// A view fed as the firmware feeds it in the live log: the name first,
/// then the descriptor, then the SDP device ids.
fn view(m: Mode) -> View {
    let mut v = View::new();
    setup(&mut v, m);
    v
}

/// What the firmware hands a view when a pad of this mode connects.
fn setup(v: &mut View, m: Mode) {
    let (name, desc, ids) = match m {
        Mode::Xw => (
            "GuliKit Controller XW",
            live1::XBOX_306,
            (0x045E, 0x02E0, 0x0903),
        ),
        Mode::Android => (
            "Xbox Wireless Controller",
            live1::XBOX_306,
            (0x045E, 0x02E0, 0x0903),
        ),
        Mode::Dinput => (
            "GuliKit Controller DW",
            live1::DINPUT_148,
            (0x1949, 0x0402, 0x011B),
        ),
        Mode::Switch => (
            "Pro Controller",
            live1::SWITCH_170,
            (0x057E, 0x2009, 0x0001),
        ),
    };
    v.set_time_ms(1000);
    v.device(&Device {
        name,
        class_of_device: 0x2508,
        ..Device::default()
    });
    v.descriptor(&hex(desc));
    v.device(&Device {
        name,
        class_of_device: 0x2508,
        vendor_id: Some(ids.0),
        product_id: Some(ids.1),
        version: Some(ids.2),
        vendor_id_source: Some(2),
    });
}

fn unmapped(v: &View) -> String {
    let mut s = String::new();
    v.write_unmapped(&mut s, "|");
    s
}

fn held(v: &View) -> String {
    let mut s = String::new();
    v.write_held(&mut s);
    s
}

/// Every real report of every mode: nothing shows as unmapped (the lamp
/// must not light for input the decoder already shows), and drawing all
/// three pages after each report does not panic.
#[test]
fn real_reports_leave_nothing_unmapped() {
    for (m, reports) in [
        (Mode::Xw, live1::XW_REPORTS),
        (Mode::Android, live1::ANDROID_REPORTS),
        (Mode::Dinput, live1::DW_REPORTS),
        (Mode::Switch, live1::SWITCH_REPORTS),
    ] {
        let mut v = view(m);
        let mut t = 1000;
        for r in reports {
            t += 10;
            v.set_time_ms(t);
            v.report(&hex(r));
            assert_eq!(unmapped(&v), "", "{r}");
            v.draw(&mut Null).unwrap();
        }
        for _ in 0..3 {
            v.next_page();
            v.draw(&mut Null).unwrap();
        }
        assert_eq!(v.unmapped_changes().1, 0);
    }
}

/// A handful of real reports per mode, every field checked by hand.
#[test]
fn real_reports_decoded_by_hand() {
    // XInput: 01 | LX 0x0c00 | LY 0xb800 | RX 0x8000 | RY 0x7fff | LT 0 |
    // RT 0x3ff | hat 0 | 0x30 (LB, RB) 0x00.
    let mut v = view(Mode::Xw);
    assert_eq!(v.layout().0, Layout::Xbox);
    v.report(&hex("01000c00b80080ff7f0000ff03003000"));
    let p = v.pad();
    assert!(p.lx.norm < -0.85 && p.lx.norm > -0.95, "{}", p.lx.norm);
    assert!(p.ly.norm > 0.4 && p.ly.norm < 0.45, "{}", p.ly.norm);
    assert!(p.rx.norm.abs() < 0.01 && p.ry.norm.abs() < 0.01);
    assert_eq!((p.lt.value, p.rt.value), (0, 1023));
    assert!(p.lt.analog && p.rt.analog);
    assert_eq!(held(&v), "LB,RB,RT");
    // LT 0x3c, RT 0x1b4, nothing pressed (both under half way).
    v.report(&hex("010080ff7f0080ff7f3c00b401000000"));
    assert_eq!((v.pad().lt.value, v.pad().rt.value), (60, 436));
    assert_eq!(held(&v), "-");
    // Hat 7 = left, then byte 15 bit 1 = right stick click.
    v.report(&hex("010080ff7f0080ff7f00000000070000"));
    assert_eq!(held(&v), "Left");
    v.report(&hex("010080ff7f0080ff7f00000000000002"));
    assert_eq!(held(&v), "RS");
    // Report 2: the Xbox button.
    v.report(&hex("0201"));
    assert_eq!(held(&v), "Xbox,RS");
    v.report(&hex("0200"));
    assert!(!v.pad().pressed(Btn::Home));

    // DInput: 07 | Z 80 Rz 7f (right stick) | X 80 Y 7f (left) | usage-0 00
    // | hat (low 4 bits) + buttons 1-4 | buttons 5-12 | vendor byte.
    let mut v = view(Mode::Dinput);
    assert_eq!(v.layout().0, Layout::Generic);
    assert!(v.triggers_on_off());
    v.report(&hex("07807fa64c009f0a00"));
    // hat 0xF = centred; 0x9 = buttons 1 and 4; 0x0a = buttons 6 and 8.
    assert_eq!(held(&v), "A,Y,RB,RT");
    assert_eq!((v.pad().lt.value, v.pad().rt.value), (0, 1023));
    let p = v.pad();
    assert!(p.rx.norm.abs() < 0.01 && p.ry.norm.abs() < 0.01);
    assert!(p.lx.norm > 0.25 && p.ly.norm < -0.35);
    v.report(&hex("075880807f00008000"));
    // hat 0 = up, button 12 = right stick click; right stick moved.
    assert_eq!(held(&v), "RS,Up");
    assert!(v.pad().rx.norm < -0.25);
    v.report(&hex("07807f807f000fc000"));
    assert_eq!(held(&v), "LS,RS");
    v.report(&hex("07807f807f000f0c00"));
    assert_eq!(held(&v), "LT,RT");
    assert_eq!((v.pad().lt.value, v.pad().rt.value), (1023, 1023));
    v.report(&hex("07807f807f000f0000"));
    assert_eq!((v.pad().lt.value, v.pad().rt.value), (0, 0));

    // Switch 0x3F: buttons, hat 0x0F centred, four 16-bit axes.
    let mut v = view(Mode::Switch);
    assert_eq!(v.layout().0, Layout::SwitchPro);
    v.report(&hex("3f00000f007a007f007e0083"));
    assert_eq!(held(&v), "-");
    v.report(&hex("3f000006007a007f007e0083"));
    // Hat 6 = left (0 up, clockwise).
    assert_eq!(held(&v), "Left");
    assert!(v.triggers_on_off());
}

/// A bit no drawn control uses, in each mode, lights the lamp by name.
#[test]
fn unexpected_bits_are_visible() {
    // XInput: byte 15, bit 2 (after the ten buttons: padding).
    let mut v = view(Mode::Xw);
    v.report(&hex("010080ff7f0080ff7f00000000000000"));
    v.report(&hex("010080ff7f0080ff7f00000000000004"));
    assert_eq!(unmapped(&v), "01 b15.2 padding");
    // Report 2, bit 1 (padding after the Xbox button).
    v.report(&hex("0200"));
    v.report(&hex("0202"));
    assert!(unmapped(&v).contains("02 b1.1 padding"), "{}", unmapped(&v));
    // Switch 0x3F: byte 2 bit 6 (button 15 of the descriptor).
    let mut v = view(Mode::Switch);
    v.report(&hex("3f00000f007a007f007e0083"));
    v.report(&hex("3f00400f007a007f007e0083"));
    assert_eq!(unmapped(&v), "3F b2.6 button 15");
}

/// DInput: every vendor bit, and every consumer key of report 0x02
/// except AC Home (shown as Home), lights the lamp with its name.
#[test]
fn dinput_vendor_and_consumer_bits() {
    let names = [
        "Play/Pause",
        "Scan Prev",
        "Scan Next",
        "Vol Down",
        "Vol Up",
        "Eject",
        "AC Back",
    ];
    for (bit, name) in names.iter().enumerate() {
        let mut v = view(Mode::Dinput);
        v.report(&hex("07807f807f000f0000"));
        v.report(&[0x02, 1 << bit]);
        assert_eq!(unmapped(&v), format!("02 b1.{bit} {name}"));
        v.report(&[0x02, 0]);
        assert_eq!(unmapped(&v), "");
    }
    let mut v = view(Mode::Dinput);
    v.report(&[0x02, 0x80]);
    assert_eq!(unmapped(&v), "");
    assert_eq!(held(&v), "Xbox");
    for bit in 0..8 {
        let mut v = view(Mode::Dinput);
        v.report(&hex("07807f807f000f0000"));
        let mut r = hex("07807f807f000f0000");
        r[8] = 1 << bit;
        v.report(&r);
        assert_eq!(unmapped(&v), format!("07 b8.{bit} vendor FF00:1"));
    }
}

/// A report longer than the descriptor says, and a report id the
/// descriptor never declares: the extra bytes show when they are not 0.
#[test]
fn longer_report_and_undeclared_id_are_visible() {
    let mut v = view(Mode::Xw);
    v.report(&hex("010080ff7f0080ff7f00000000000000"));
    // 16 bytes declared; a 17th byte with a bit set.
    v.report(&hex("010080ff7f0080ff7f0000000000000002"));
    assert!(
        unmapped(&v).contains("01 b16.1 undeclared"),
        "{}",
        unmapped(&v)
    );
    // An id the descriptor does not have.
    let mut v = view(Mode::Dinput);
    v.report(&hex("07807f807f000f0000"));
    v.report(&hex("0500200000"));
    assert_eq!(unmapped(&v), "05 b2.5 undeclared");
    // Its first report all zero: nothing on.
    let mut v = view(Mode::Dinput);
    v.report(&hex("0500000000"));
    assert_eq!(unmapped(&v), "");
}

fn lines_per_second(
    v: &mut View,
    t: &mut Tracer,
    from: u32,
    step: u32,
    n: u32,
    f: impl Fn(u32) -> Vec<u8>,
) -> (u32, u32) {
    let mut lines = 0;
    let draws_before = v.unmapped_changes().0;
    for i in 0..n {
        let now = from + i * step;
        v.set_time_ms(now);
        v.report(&f(i));
        if t.report(now, &v.last_change()).kind.is_some() {
            lines += 1;
        }
        v.draw(&mut Null).unwrap();
    }
    let changes = v.unmapped_changes().0 - draws_before;
    (lines, changes)
}

/// A noisy unmapped value (a byte that changes with every report) must
/// not flood the log: it is a value, not a button.
#[test]
fn noisy_unmapped_value_does_not_flood_the_log() {
    let mut v = view(Mode::Dinput);
    let mut t = Tracer::new();
    // 10 s at 100 reports a second, byte 5 (usage 0) noisy, never 0.
    let (lines, screen) = lines_per_second(&mut v, &mut t, 0, 10, 1000, |i| {
        let mut r = hex("07807f807f000f0000");
        r[5] = if i == 0 { 1 } else { 2 + (i % 200) as u8 };
        r
    });
    // The first report, and the value leaving its base once.
    assert!(lines <= 2, "{lines} lines in 10 s");
    assert!(screen <= 41, "{screen} screen changes in 10 s");
    // A value that keeps coming back to its base is on and off each time:
    // two lines per return, never more than the cap.
    let mut v = view(Mode::Dinput);
    let mut t = Tracer::new();
    let (lines, _) = lines_per_second(&mut v, &mut t, 0, 10, 1000, |i| {
        let mut r = hex("07807f807f000f0000");
        r[5] = 1 + (i % 2) as u8;
        r
    });
    assert!(lines <= MAX_LINES_PER_S * 10 + 1, "{lines}");
}

/// Stick and trigger noise: no line after the first, few screen changes.
#[test]
fn stick_noise_is_quiet() {
    for (m, base, at) in [
        (
            Mode::Xw,
            "010080ff7f0080ff7f00000000000000",
            [1usize, 5, 9, 11],
        ),
        (Mode::Dinput, "07807f807f000f0000", [1, 2, 3, 4]),
    ] {
        let mut v = view(m);
        let mut t = Tracer::new();
        let (lines, screen) = lines_per_second(&mut v, &mut t, 0, 8, 1250, |i| {
            let mut r = hex(base);
            for &k in &at {
                r[k] = r[k].wrapping_add((i % 7) as u8);
            }
            r
        });
        assert!(lines <= 2, "{lines}");
        // The first draw takes the empty list once.
        assert!(screen <= 1, "{screen}");
    }
}

/// One clean press and release of a button: exactly two lines, both
/// "change", in every mode (the pads send continuously or only on change).
#[test]
fn one_press_two_lines() {
    for (m, idle, down, every) in [
        (
            Mode::Xw,
            "010080ff7f0080ff7f00000000000000",
            "010080ff7f0080ff7f00000000000100",
            None,
        ),
        (
            Mode::Dinput,
            "07807f807f000f0000",
            "07807f807f000f0400",
            Some(20),
        ),
        (
            Mode::Switch,
            "3f00000f007a007f007e0083",
            "3f00200f007a007f007e0083",
            Some(40),
        ),
    ] {
        let mut v = view(m);
        let mut t = Tracer::new();
        let mut kinds: Vec<Kind> = Vec::new();
        let mut now = 0;
        let feed = |v: &mut View, t: &mut Tracer, r: &str, now: u32, kinds: &mut Vec<Kind>| {
            v.set_time_ms(now);
            v.report(&hex(r));
            if let Some(k) = t.report(now, &v.last_change()).kind {
                kinds.push(k);
            }
        };
        // The first report gets its own "first" line; then idle.
        feed(&mut v, &mut t, idle, now, &mut kinds);
        let gap = every.unwrap_or(500);
        for _ in 0..5 {
            now += gap;
            if every.is_some() {
                feed(&mut v, &mut t, idle, now, &mut kinds);
            }
        }
        for r in [down, down, down, idle, idle, idle] {
            now += every.unwrap_or(30);
            if every.is_none() && r == down && kinds.len() > 1 {
                continue;
            }
            feed(&mut v, &mut t, r, now, &mut kinds);
        }
        assert_eq!(kinds.len(), 3, "{kinds:?}");
        assert!(kinds.iter().all(|k| *k == Kind::Change));
    }
}

/// The cap holds even with a digital change in every report.
#[test]
fn cap_holds_for_view_changes() {
    let mut v = view(Mode::Dinput);
    let mut t = Tracer::new();
    let (lines, _) = lines_per_second(&mut v, &mut t, 0, 5, 2000, |i| {
        hex(if i % 2 == 0 {
            "07807f807f000f0400"
        } else {
            "07807f807f000f0000"
        })
    });
    assert!(lines <= MAX_LINES_PER_S * 10 + 1, "{lines}");
}

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
}

/// Random, short, empty, long and undeclared reports through the three
/// real descriptors, the tracker, the trace and all pages: no panic.
#[test]
fn fuzz_real_descriptors_with_tracker() {
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    for m in [Mode::Xw, Mode::Android, Mode::Dinput, Mode::Switch] {
        let mut v = view(m);
        let mut t = Tracer::new();
        for i in 0..20_000u32 {
            let len = match rng.next() % 6 {
                0 => 0,
                1 => 1,
                2 => 2,
                3 => (rng.next() % 20) as usize,
                4 => 49,
                _ => 60 + (rng.next() % 80) as usize,
            };
            let mut r: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
            if !r.is_empty() && rng.next().is_multiple_of(2) {
                r[0] = [0x01, 0x02, 0x04, 0x07, 0x21, 0x30, 0x3F, 0x05][(rng.next() % 8) as usize];
            }
            let now = i * 3;
            v.set_time_ms(now);
            v.report(&r);
            let _ = t.report(now, &v.last_change());
            let mut s = String::new();
            v.write_unmapped(&mut s, "|");
            v.write_held(&mut s);
            let mut buf = [0u8; 64];
            let _ = v.next_output(&mut buf);
            if i % 50 == 0 {
                v.draw(&mut Null).unwrap();
            }
            if i % 3000 == 0 {
                v.next_page();
            }
        }
    }
}

/// A small framebuffer: the colour of each pixel.
struct Fb(Vec<Rgb565>);
impl OriginDimensions for Fb {
    fn size(&self) -> Size {
        Size::new(320, 240)
    }
}
impl DrawTarget for Fb {
    type Color = Rgb565;
    type Error = core::convert::Infallible;
    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(
        &mut self,
        p: I,
    ) -> Result<(), Self::Error> {
        for Pixel(at, c) in p {
            if at.x >= 0 && at.y >= 0 && at.x < 320 && at.y < 240 {
                self.0[at.y as usize * 320 + at.x as usize] = c;
            }
        }
        Ok(())
    }
}

/// The owner saw "LT,RT held" in the text and empty bars in DInput mode.
/// Replay the live order (an Xbox session first, then a reset for the
/// other gamepad, the name, the descriptor, the ids, and real DInput
/// reports) and read the bar pixels: both bars fill when held, empty when
/// let go.
#[test]
fn dinput_trigger_bars_fill() {
    let cyan = Rgb565::new(0, 52, 31);
    // Middle of the bar insides (LT 44..114, RT 206..276, y 23..32).
    let lt = 27 * 320 + 79;
    let rt = 27 * 320 + 241;
    let mut v = view(Mode::Xw);
    let mut fb = Fb(vec![Rgb565::BLACK; 320 * 240]);
    for r in live1::XW_REPORTS.iter().take(10) {
        v.report(&hex(r));
        v.draw(&mut fb).unwrap();
    }
    // As ui.rs does for another gamepad after a loss: the same view.
    v.disconnected();
    v.draw(&mut fb).unwrap();
    v.reset();
    setup(&mut v, Mode::Dinput);
    let mut t = 2000;
    for (r, want) in [
        ("07807f807f000f0000", (false, false)),
        ("07807f807f000f0400", (true, false)),
        ("07807f807f000f0c00", (true, true)),
        ("07807f807f000f0800", (false, true)),
        ("07807f807f000f0000", (false, false)),
    ] {
        t += 300;
        v.set_time_ms(t);
        v.report(&hex(r));
        v.draw(&mut fb).unwrap();
        assert_eq!((fb.0[lt] == cyan, fb.0[rt] == cyan), want, "{r}");
    }
    // The real DInput reports of the session in order: whenever the text
    // says LT or RT is held, the bar is full.
    for r in live1::DW_REPORTS {
        t += 20;
        v.set_time_ms(t);
        v.report(&hex(r));
        v.draw(&mut fb).unwrap();
        let h = held(&v);
        let has = |n: &str| h.split(',').any(|x| x == n);
        assert_eq!(fb.0[lt] == cyan, has("LT"), "{r} {h}");
        assert_eq!(fb.0[rt] == cyan, has("RT"), "{r} {h}");
    }
}

/// A key that was on for a moment leaves the screen one gate (250 ms)
/// after it is let go, not two: the demo's AC Back, drawn every 24 ms.
#[test]
fn short_press_leaves_the_lamp_soon() {
    let mut v = view(Mode::Dinput);
    let mut off_at = None;
    let mut gone_at = None;
    let mut consumer = 0u8;
    for t in (2000..5000u32).step_by(8) {
        v.set_time_ms(t);
        // As the demo: the vendor bit, then AC Back, then AC Home.
        let mut r = hex("07807f807f000f0000");
        r[8] = (2600..3000).contains(&t) as u8;
        v.report(&r);
        let c = if (3000..3400).contains(&t) {
            0x40
        } else if (3400..3800).contains(&t) {
            0x80
        } else {
            0
        };
        if c != consumer {
            consumer = c;
            v.report(&[0x02, c]);
            if c == 0x80 {
                off_at = Some(t);
            }
        }
        if t % 24 == 0 {
            v.draw(&mut Null).unwrap();
        }
        if off_at.is_some() && gone_at.is_none() && v.unmapped_shown().1 == 0 {
            gone_at = Some(t);
        }
    }
    let lag = gone_at.unwrap() - off_at.unwrap();
    assert!(lag <= 300, "{lag} ms");
}

/// pad3: the timed notice covers the status, then goes away; the
/// pairings line survives a reset.
#[test]
fn notice_covers_the_status_for_a_while() {
    let mut v = pad_view::View::new();
    v.status("Searching");
    v.notice("All pads forgotten", 1000);
    assert_eq!(v.shown_status(), "Searching", "no time yet: no notice");
    v.set_time_ms(5_000);
    v.notice("All pads forgotten", 1000);
    v.status("Searching (round 2)");
    assert_eq!(v.shown_status(), "All pads forgotten");
    v.set_time_ms(5_999);
    assert_eq!(v.shown_status(), "All pads forgotten");
    v.set_time_ms(6_000);
    assert_eq!(v.shown_status(), "Searching (round 2)");
    v.pairings("Known pads: 2 (saved)");
    v.reset();
    v.set_time_ms(6_100);
    v.notice("x", u32::MAX / 2);
    v.set_time_ms(6_200);
    assert_eq!(v.shown_status(), "x");
}

/// What next_output gives now, as (channel, bytes).
fn take_output(v: &mut View) -> Option<(pad_view::startup::Channel, Vec<u8>)> {
    let mut buf = [0_u8; 64];
    v.next_output(&mut buf)
        .map(|o| (o.channel, buf[..o.len].to_vec()))
}

fn battery_text(v: &View) -> String {
    let mut s = String::new();
    pad_view::battery::line(&mut s, v.pad().battery, v.battery_ask());
    s
}

/// Part B: an Xbox-mode pad is asked once per link for input report 04
/// (GET_REPORT on the control channel), only after its first report.
#[test]
fn xbox_pad_is_asked_for_its_battery_once() {
    use pad_view::battery::Ask;
    use pad_view::startup::Channel;
    let mut v = view(Mode::Xw);
    assert_eq!(take_output(&mut v), None, "no report yet: no question");
    v.report(&hex(live1::XW_REPORTS[0]));
    assert_eq!(take_output(&mut v), Some((Channel::GetReport, vec![0x04])));
    assert_eq!(v.battery_ask(), Ask::Waiting { since: Some(1000) });
    assert_eq!(battery_text(&v), "Battery: asking the pad");
    assert_eq!(take_output(&mut v), None, "asked only once");
    // The answer: DATA, Input (0xA1), report 04, byte 0x13 (level 3,
    // charging).
    v.control(&[0xA1, 0x04, 0x13]);
    assert_eq!(v.battery_ask(), Ask::Answered);
    assert_eq!(v.pad().battery, Some(0x13));
    assert_eq!(
        battery_text(&v),
        "Battery: level 3 of 3, charging (byte 0x13)"
    );
    v.next_page();
    v.next_page();
    v.draw(&mut Null).unwrap();
    // A new link asks again.
    v.disconnected();
    v.report(&hex(live1::XW_REPORTS[0]));
    assert_eq!(take_output(&mut v), Some((Channel::GetReport, vec![0x04])));
}

/// The question goes back if the host could not send it.
#[test]
fn battery_question_not_sent_is_asked_again() {
    use pad_view::startup::Channel;
    let mut v = view(Mode::Android);
    v.report(&hex(live1::ANDROID_REPORTS[0]));
    assert_eq!(take_output(&mut v), Some((Channel::GetReport, vec![0x04])));
    v.output_not_sent();
    assert_eq!(take_output(&mut v), Some((Channel::GetReport, vec![0x04])));
}

/// "battery: not reported": the pad refuses, or says nothing for 1 s.
#[test]
fn battery_not_reported() {
    use pad_view::battery::Ask;
    // Refused: HANDSHAKE 0x03 (this request is not supported).
    let mut v = view(Mode::Xw);
    v.report(&hex(live1::XW_REPORTS[0]));
    take_output(&mut v).unwrap();
    v.control(&[0x03]);
    assert_eq!(v.battery_ask(), Ask::Refused(3));
    assert_eq!(
        battery_text(&v),
        "Battery: not reported (pad said no, code 3)"
    );
    // No answer.
    let mut v = view(Mode::Xw);
    v.report(&hex(live1::XW_REPORTS[0]));
    take_output(&mut v).unwrap();
    v.set_time_ms(2000);
    assert!(
        matches!(v.battery_ask(), Ask::Waiting { .. }),
        "1 s is still in time"
    );
    v.set_time_ms(2001);
    assert_eq!(v.battery_ask(), Ask::NoAnswer);
    assert_eq!(battery_text(&v), "Battery: not reported (no answer in 1 s)");
    v.next_page();
    v.next_page();
    v.draw(&mut Null).unwrap();
    // A late answer is not taken as the battery any more.
    v.control(&[0xA1, 0x04, 0x03]);
    assert_eq!(v.pad().battery, None);
}

/// Other modes are never asked, and every line fits the 54-letter row.
#[test]
fn battery_only_in_xbox_modes() {
    for (m, r) in [
        (Mode::Dinput, live1::DW_REPORTS[0]),
        (Mode::Switch, live1::SWITCH_REPORTS[0]),
    ] {
        let mut v = view(m);
        v.report(&hex(r));
        while let Some((ch, _)) = take_output(&mut v) {
            assert_ne!(ch, pad_view::startup::Channel::GetReport);
        }
    }
    use pad_view::battery::{Ask, line};
    for (value, ask) in [
        (Some(0xFF), Ask::Answered),
        (None, Ask::NotAsked),
        (None, Ask::Waiting { since: None }),
        (None, Ask::Answered),
        (None, Ask::Refused(15)),
        (None, Ask::NoAnswer),
    ] {
        let mut s = String::new();
        line(&mut s, value, ask);
        assert!(s.len() <= 54, "{s}");
    }
}
