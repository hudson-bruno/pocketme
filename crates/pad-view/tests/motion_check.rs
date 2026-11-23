//! A second, independent check of the motion sensor work (written by the
//! reviewer, not by the author of `src/motion.rs`).
//!
//! The reports here are typed out byte by byte from the published layout
//! (dekuNukem's bluetooth_hid_notes.md, "Standard input report"; SDL's
//! SwitchStatePacket_t), with no helper from pad-view or the other tests:
//!
//! ```text
//! byte 0       0x30, the report id
//! byte 1       timer
//! byte 2       battery (top 4 bits) and connection
//! bytes 3-5    buttons (right, shared, left)
//! bytes 6-11   the two sticks, 3 bytes each
//! byte 12      vibration report
//! bytes 13-48  three samples of 12 bytes: accelerometer x, y, z, then
//!              gyroscope x, y, z, each a signed 16-bit number, low byte
//!              first
//! ```
//!
//! It also checks the start-up packets byte by byte, that the motion
//! parts draw only inside their own boxes, that broken and extreme
//! reports never panic, and the limits on how often the motion parts are
//! drawn.

use embedded_graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};
use pad_view::decode::Layout;
use pad_view::{Device, View};

/// Sample 1: accel x = 4096 (00 10), y = -2 (FE FF), z = 32767 (FF 7F),
/// gyro x = -32768 (00 80), y = 0x0102 (02 01), z = 0x0304 (04 03).
/// Sample 2: accel 1, 2, 3; gyro -1, -2, -3.
/// Sample 3: accel x = -4096 (00 F0), y = 0x1234 (34 12), z = 0x0FA0
/// (A0 0F, 4000); gyro x = 1000 (E8 03), y = -1000 (18 FC), z = 0 (00 00).
const REPORT: [u8; 49] = [
    0x30, 0x42, 0x8E, 0x00, 0x00, 0x00, 0x00, 0x08, 0x80, 0x00, 0x08, 0x80, 0x0B, //
    0x00, 0x10, 0xFE, 0xFF, 0xFF, 0x7F, 0x00, 0x80, 0x02, 0x01, 0x04, 0x03, //
    0x01, 0x00, 0x02, 0x00, 0x03, 0x00, 0xFF, 0xFF, 0xFE, 0xFF, 0xFD, 0xFF, //
    0x00, 0xF0, 0x34, 0x12, 0xA0, 0x0F, 0xE8, 0x03, 0x18, 0xFC, 0x00, 0x00, //
];

fn switch_view() -> View {
    let mut v = View::new();
    v.device(&Device {
        name: "Pro Controller",
        class_of_device: 0x002508,
        vendor_id: Some(0x057E),
        product_id: Some(0x2009),
        version: Some(1),
        vendor_id_source: Some(2),
    });
    v
}

#[test]
fn hand_typed_report_decodes_to_the_published_layout() {
    let mut v = switch_view();
    v.report(&REPORT);
    assert_eq!(v.layout().0, Layout::SwitchPro);
    let m = v.pad().motion.expect("motion data");
    assert_eq!(m.samples[0], [4096, -2, 32767, -32768, 0x0102, 0x0304]);
    assert_eq!(m.samples[1], [1, 2, 3, -1, -2, -3]);
    assert_eq!(m.samples[2], [-4096, 0x1234, 4000, 1000, -1000, 0]);
    // The screen shows the third sample as the newest.
    assert_eq!(m.newest(), [-4096, 0x1234, 4000, 1000, -1000, 0]);
    // Units: 4096 counts per g; 0.070 degrees a second per count.
    let g = m.accel_g();
    assert!(
        (g[0] + 1.0).abs() < 1e-6 && (g[2] - 4000.0 / 4096.0).abs() < 1e-6,
        "{g:?}"
    );
    let d = m.gyro_dps();
    assert!(
        (d[0] - 70.0).abs() < 1e-3 && (d[1] + 70.0).abs() < 1e-3 && d[2] == 0.0,
        "{d:?}"
    );
    // The sticks and the rest of the report are still read as before.
    assert_eq!(v.pad().battery, Some(8));
    assert_eq!(v.pad().lx.raw, 2048);
    assert_eq!(v.pad().ly.raw, 2048);
    // The same bytes in a 0x31 report (which is longer) give the same motion.
    let mut long = [0u8; 362];
    long[..49].copy_from_slice(&REPORT);
    long[0] = 0x31;
    v.report(&long);
    assert_eq!(v.pad().motion, Some(m));
}

/// The start-up packets once the first way (info-49) is answered, byte by
/// byte (dekuNukem's "Output report 0x01": id, packet counter 0-15, 8
/// bytes of rumble, subcommand id, arguments; SDL sends 49 bytes over
/// Bluetooth and uses the neutral rumble 00 01 40 40 for the KingKong 2
/// Pro). Changed on purpose with the prober: device info (0x02) goes
/// first, after the first report and a 300 ms settle, and the steps
/// follow in SDL's order 03 30, 40 01, 30 01.
#[test]
fn start_up_packets_byte_by_byte() {
    let mut v = switch_view();
    v.set_time_ms(0);
    v.report(&[0x3F, 0, 0, 8, 0, 0x80, 0, 0x80, 0, 0x80, 0, 0x80]);
    v.set_time_ms(300);
    let mut want = [[0u8; 49]; 4];
    for (k, (sub, arg)) in [(0x02, 0x00), (0x03, 0x30), (0x40, 0x01), (0x30, 0x01)]
        .into_iter()
        .enumerate()
    {
        want[k][..12].copy_from_slice(&[
            0x01, k as u8, 0x00, 0x01, 0x40, 0x40, 0x00, 0x01, 0x40, 0x40, sub, arg,
        ]);
    }
    for w in &want {
        let mut buf = [0xAAu8; 64];
        let out = v.next_output(&mut buf).expect("a packet");
        assert_eq!(out.len, 49);
        assert_eq!(out.channel, pad_view::startup::Channel::Interrupt);
        assert_eq!(&buf[..49], &w[..], "packet");
        // Nothing more until the pad answers.
        assert_eq!(v.next_output(&mut [0u8; 64]), None);
        // The answer: report 0x21, byte 13 ack (top bit set), byte 14 the
        // subcommand id.
        let mut reply = [0u8; 49];
        reply[0] = 0x21;
        reply[2] = 0x8E;
        reply[13] = 0x80;
        reply[14] = w[10];
        v.report(&reply);
    }
    assert!(v.startup().finished());
    assert_eq!(
        v.startup().outcome().to_string(),
        "won:info-49,03:ok,40:ok,30:ok"
    );
    assert_eq!(v.next_output(&mut [0u8; 64]), None);
}

/// A screen that keeps every pixel and the box around what was drawn.
struct Shot {
    px: Vec<Rgb565>,
    touched: Vec<Point>,
    off_screen: u64,
}

impl Shot {
    fn new() -> Self {
        Shot {
            px: vec![Rgb565::BLACK; 320 * 240],
            touched: Vec::new(),
            off_screen: 0,
        }
    }
}

impl OriginDimensions for Shot {
    fn size(&self) -> Size {
        Size::new(320, 240)
    }
}

impl DrawTarget for Shot {
    type Color = Rgb565;
    type Error = core::convert::Infallible;
    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(
        &mut self,
        pixels: I,
    ) -> Result<(), Self::Error> {
        for Pixel(p, c) in pixels {
            if p.x < 0 || p.y < 0 || p.x >= 320 || p.y >= 240 {
                self.off_screen += 1;
                continue;
            }
            self.px[p.y as usize * 320 + p.x as usize] = c;
            self.touched.push(p);
        }
        Ok(())
    }
}

fn with_motion(samples: [[i16; 6]; 3]) -> [u8; 49] {
    let mut r = REPORT;
    for (k, s) in samples.iter().enumerate() {
        for (i, v) in s.iter().enumerate() {
            let [lo, hi] = v.to_le_bytes();
            r[13 + 12 * k + 2 * i] = lo;
            r[14 + 12 * k + 2 * i] = hi;
        }
    }
    r
}

/// Extreme and odd motion values, used by several tests below.
fn odd_samples() -> Vec<[[i16; 6]; 3]> {
    let (lo, hi) = (i16::MIN, i16::MAX);
    let mut out = vec![
        [[hi; 6]; 3],
        [[lo; 6]; 3],
        [
            [lo, hi, lo, hi, lo, hi],
            [hi, lo, hi, lo, hi, lo],
            [lo, lo, hi, hi, lo, hi],
        ],
        [[0, 0, lo, 0, 0, 0]; 3],
        [[0, 0, 1, 0, 0, 0]; 3],
        [[1, 0, 0, hi, lo, hi]; 3],
        [[0, 0, 0, hi, hi, hi]; 3],
        [[1, -1, 0, 0, 0, 0], [-1, 1, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0]],
        [
            [lo, lo, lo, 0, 0, 0],
            [hi, hi, hi, 0, 0, 0],
            [0, 0, 1, 0, 0, 0],
        ],
        [[0, 0, 4096, 0, 0, 0]; 3],
    ];
    let mut x: u32 = 12345;
    for _ in 0..200 {
        let mut s = [[0i16; 6]; 3];
        for v in s.iter_mut().flatten() {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            *v = (x >> 8) as i16;
        }
        out.push(s);
    }
    out
}

/// Reports cut anywhere (also in the middle of a motion number) and
/// extreme values: never a panic (the tests run with overflow checks),
/// never a pixel off the screen, and the motion parts of the Pad page
/// stay in their boxes.
#[test]
fn extreme_and_cut_reports_never_panic_and_stay_in_their_boxes() {
    // The boxes of the Pad page's motion parts (the round level in the
    // lower left grip, the gyroscope lines in the lower right one).
    let ball = Rectangle::new(Point::new(40, 107), Size::new(43, 43));
    let gyro = Rectangle::new(Point::new(229, 112), Size::new(52, 37));
    for page in 0..3 {
        let mut v = switch_view();
        v.set_time_ms(0);
        v.report(&with_motion([[0, 0, 4096, 0, 0, 0]; 3]));
        for _ in 0..page {
            v.next_page();
        }
        let mut s = Shot::new();
        v.draw(&mut s).unwrap();
        let mut t = 0u32;
        for samples in odd_samples() {
            let r = with_motion(samples);
            for len in [49, 48, 47, 37, 36, 30, 25, 24, 14, 13, 12, 1] {
                t += 120;
                v.set_time_ms(t);
                v.report(&r[..len]);
                s.touched.clear();
                v.draw(&mut s).unwrap();
                if page == 0 {
                    // Only the motion parts change in the picture of the
                    // pad (y 46 to 155): buttons and sticks stay the same.
                    // A cut report (shorter than 12 bytes) is not read.
                    for p in s.touched.iter().filter(|p| p.y >= 46 && p.y < 156) {
                        assert!(
                            ball.contains(*p) || gyro.contains(*p),
                            "page {page}, len {len}: pixel {p:?} outside the motion boxes"
                        );
                    }
                }
                if page == 1 {
                    // The Data page's motion block: right half, rows 24 to 167.
                    for p in s
                        .touched
                        .iter()
                        .filter(|p| p.y >= 24 && p.y < 168 && p.x >= 160)
                    {
                        assert!(p.x < 320, "{p:?}");
                    }
                }
            }
            // Through Motion's own functions too.
            if let Some(m) = pad_view::motion::Motion::from_switch_report(&r) {
                let tilt = m.tilt();
                assert!(tilt.roll_deg.is_finite() && tilt.pitch_deg.is_finite());
                assert!(
                    tilt.roll_deg.abs() <= 180.5 && tilt.pitch_deg.abs() <= 90.5,
                    "{tilt:?}"
                );
                let (dx, dy) = m.downhill(13);
                assert!(
                    dx * dx + dy * dy <= 13 * 13 + 1,
                    "ball offset {dx}, {dy} for {samples:?}"
                );
            }
        }
        assert_eq!(s.off_screen, 0, "page {page}: pixels off the screen");
    }
}

/// When the motion stops changing, the ball and numbers are drawn once and
/// then left alone; when the motion data stops, the ball goes and nothing
/// of it is left behind.
#[test]
fn no_leftovers_when_the_ball_moves_or_the_data_stops() {
    let mut v = switch_view();
    v.set_time_ms(0);
    v.report(&with_motion([[0, 0, 4096, 0, 0, 0]; 3]));
    let mut s = Shot::new();
    v.draw(&mut s).unwrap();
    let flat = s.px.clone();
    // Tilt right and nose up, then back to flat: the picture must be the
    // same as before, pixel for pixel, apart from the raw hex lines and
    // the report counters (below y 156).
    v.set_time_ms(1000);
    v.report(&with_motion([[2000, 3000, 2000, 500, 0, 0]; 3]));
    v.draw(&mut s).unwrap();
    v.set_time_ms(2000);
    v.report(&with_motion([[0, 0, 4096, 0, 0, 0]; 3]));
    v.draw(&mut s).unwrap();
    let diff = (0..320 * 156).filter(|&i| s.px[i] != flat[i]).count();
    assert_eq!(diff, 0, "ball or numbers left behind");
    // The sensor stops (all zeros): no ball, and the same picture as a
    // pad that never sent motion.
    v.set_time_ms(3000);
    v.report(&with_motion([[0; 6]; 3]));
    v.draw(&mut s).unwrap();
    let mut w = switch_view();
    w.set_time_ms(3000);
    w.report(&with_motion([[0; 6]; 3]));
    let mut never = Shot::new();
    w.draw(&mut never).unwrap();
    let diff = (320 * 46..320 * 156)
        .filter(|&i| s.px[i] != never.px[i])
        .count();
    assert_eq!(
        diff, 0,
        "after the data stopped, the picture differs from a pad without motion"
    );
}

/// A pad whose reports flip between motion data and zeros (the sensor
/// switching off and on) must not redraw the motion parts on every report.
#[test]
fn flipping_data_and_zeros_is_limited_too() {
    let mut v = switch_view();
    let data = with_motion([[0, 0, 4096, 0, 0, 0]; 3]);
    let zeros = with_motion([[0; 6]; 3]);
    let start = v.motion_changes();
    let mut t = 0;
    // 2 s at 125 reports a second.
    while t < 2000 {
        v.set_time_ms(t);
        v.report(if (t / 8) % 2 == 0 { &data } else { &zeros });
        t += 8;
    }
    let n = v.motion_changes() - start;
    assert!(n <= 21, "{n} changes in 2 s");
}

/// The last small move must show even when it came less than 100 ms after
/// the change before and then the reports stopped: the next draw shows it.
#[test]
fn last_move_shows_after_the_reports_stop() {
    let mut v = switch_view();
    v.set_time_ms(0);
    v.report(&with_motion([[0, 0, 4096, 0, 0, 0]; 3]));
    v.set_time_ms(200);
    v.report(&with_motion([[0, 2048, 3547, 0, 0, 0]; 3]));
    assert_eq!(v.motion_shown().map(|m| m.newest()[1]), Some(2048));
    // 30 ms later the pad turns further and then nothing more comes.
    v.set_time_ms(230);
    v.report(&with_motion([[0, 4096, 0, 0, 0, 0]; 3]));
    assert_eq!(
        v.motion_shown().map(|m| m.newest()[1]),
        Some(2048),
        "too soon to show"
    );
    // The draw timer, 100 ms later.
    v.set_time_ms(330);
    v.draw(&mut Shot::new()).unwrap();
    assert_eq!(
        v.motion_shown().map(|m| m.newest()[1]),
        Some(4096),
        "the last move must show"
    );
    // The Data page's slower values catch up too.
    v.set_time_ms(600);
    v.draw(&mut Shot::new()).unwrap();
    assert_eq!(v.motion_shown_data().0.map(|m| m.newest()[1]), Some(4096));
}

/// The Data page's motion block changes at most 4 times a second (one
/// change there redraws about 100 letters).
#[test]
fn data_page_motion_is_slower() {
    let mut v = switch_view();
    let (_, start) = v.motion_shown_data();
    let mut t = 0u32;
    let mut k = 0i32;
    while t < 2000 {
        v.set_time_ms(t);
        v.report(&with_motion(
            [[0, ((k * 300) % 4096) as i16, 4000, 0, 0, 0]; 3],
        ));
        t += 8;
        k += 1;
    }
    let (_, end) = v.motion_shown_data();
    assert!(
        (7..=9).contains(&(end - start)),
        "{} Data page changes in 2 s",
        end - start
    );
    assert!(
        (18..=21).contains(&v.motion_changes()),
        "{} Pad page changes in 2 s",
        v.motion_changes()
    );
}
