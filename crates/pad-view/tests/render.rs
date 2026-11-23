//! Drawing: every page is drawn into a framebuffer on the PC and saved as
//! a PNG in .local/pad-view-shots/ (at the top of the repository) so a
//! person can look at it. The tests also count how many pixels each draw()
//! sends, which is what costs time on the real screen.

mod data;

use embedded_graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};
use pad_view::{Device, HEIGHT, View, WIDTH};

const W: usize = WIDTH as usize;
const H: usize = HEIGHT as usize;

/// A framebuffer that counts what is sent to it.
struct Frame {
    px: Vec<Rgb565>,
    /// Pixels sent (each would cross the SPI bus).
    sent: u64,
    /// Number of drawing calls.
    calls: u64,
    /// Windows the real screen would open: one per filled rectangle, and
    /// one per run of pixels side by side on a row for single pixels
    /// (at most 64 long), as the firmware's display.rs does. Each costs
    /// about 50 microseconds on the board, plus 0.4 per pixel.
    windows: u64,
    /// The smallest box around everything sent since `reset`.
    touched: Option<(i32, i32, i32, i32)>,
    /// Pixels sent inside `watch` since `reset`.
    watch: Option<Rectangle>,
    watched: u64,
    /// Windows that start inside `watch` since `reset`.
    watched_windows: u64,
}

impl Frame {
    fn new() -> Self {
        Frame {
            px: vec![Rgb565::new(31, 0, 31); W * H],
            sent: 0,
            calls: 0,
            windows: 0,
            touched: None,
            watch: None,
            watched: 0,
            watched_windows: 0,
        }
    }
    fn reset(&mut self) {
        self.sent = 0;
        self.calls = 0;
        self.windows = 0;
        self.touched = None;
        self.watched = 0;
        self.watched_windows = 0;
    }
    fn put(&mut self, p: Point, c: Rgb565) {
        self.sent += 1;
        if self.watch.is_some_and(|r| r.contains(p)) {
            self.watched += 1;
        }
        if p.x >= 0 && p.y >= 0 && (p.x as usize) < W && (p.y as usize) < H {
            self.px[p.y as usize * W + p.x as usize] = c;
            let t = self.touched.get_or_insert((p.x, p.y, p.x, p.y));
            t.0 = t.0.min(p.x);
            t.1 = t.1.min(p.y);
            t.2 = t.2.max(p.x);
            t.3 = t.3.max(p.y);
        }
    }
    fn at(&self, x: usize, y: usize) -> Rgb565 {
        self.px[y * W + x]
    }
    fn save(&self, name: &str) {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/pad-view-shots");
        std::fs::create_dir_all(&dir).unwrap();
        // Saved at twice the size so small text is easy to read.
        const S: usize = 2;
        let mut rgb = vec![0u8; W * H * 3 * S * S];
        for y in 0..H * S {
            for x in 0..W * S {
                let c = self.px[(y / S) * W + x / S];
                let i = (y * W * S + x) * 3;
                rgb[i] = (c.r() << 3) | (c.r() >> 2);
                rgb[i + 1] = (c.g() << 2) | (c.g() >> 4);
                rgb[i + 2] = (c.b() << 3) | (c.b() >> 2);
            }
        }
        let f = std::fs::File::create(dir.join(name)).unwrap();
        let mut e = png::Encoder::new(std::io::BufWriter::new(f), (W * S) as u32, (H * S) as u32);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(&rgb).unwrap();
    }
}

impl OriginDimensions for Frame {
    fn size(&self) -> Size {
        Size::new(WIDTH, HEIGHT)
    }
}

impl DrawTarget for Frame {
    type Color = Rgb565;
    type Error = core::convert::Infallible;
    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(
        &mut self,
        pixels: I,
    ) -> Result<(), Self::Error> {
        self.calls += 1;
        let mut run: Option<(Point, i32)> = None;
        for Pixel(p, c) in pixels {
            let joins = run.is_some_and(|(s, n)| n < 64 && p.y == s.y && p.x == s.x + n);
            if joins {
                run = run.map(|(s, n)| (s, n + 1));
            } else {
                self.windows += 1;
                self.watched_windows += self.watch.is_some_and(|r| r.contains(p)) as u64;
                run = Some((p, 1));
            }
            self.put(p, c);
        }
        Ok(())
    }
    fn fill_contiguous<I: IntoIterator<Item = Rgb565>>(
        &mut self,
        area: &Rectangle,
        colors: I,
    ) -> Result<(), Self::Error> {
        self.calls += 1;
        self.windows += 1;
        self.watched_windows += self.watch.is_some_and(|r| r.contains(area.top_left)) as u64;
        for (p, c) in area.points().zip(colors) {
            self.put(p, c);
        }
        Ok(())
    }
    fn fill_solid(&mut self, area: &Rectangle, color: Rgb565) -> Result<(), Self::Error> {
        self.calls += 1;
        self.windows += 1;
        self.watched_windows += self.watch.is_some_and(|r| r.contains(area.top_left)) as u64;
        for p in area.points() {
            self.put(p, color);
        }
        Ok(())
    }
}

fn switch_report(r3: u8, r4: u8, r5: u8, lx: u16, ly: u16, rx: u16, ry: u16) -> Vec<u8> {
    let mut r = vec![0u8; 49];
    r[0] = 0x30;
    r[1] = 0x5A;
    r[2] = 0x8E;
    r[3] = r3;
    r[4] = r4;
    r[5] = r5;
    let pack = |x: u16, y: u16| {
        [
            (x & 0xFF) as u8,
            ((x >> 8) & 0x0F) as u8 | ((y & 0x0F) << 4) as u8,
            (y >> 4) as u8,
        ]
    };
    r[6..9].copy_from_slice(&pack(lx, ly));
    r[9..12].copy_from_slice(&pack(rx, ry));
    r[12] = 0x0B;
    // Lying almost flat: the same motion sample three times.
    set_motion(&mut r, [[-120, 40, 4096, 3, -7, 12]; 3]);
    r
}

/// Write the three motion samples into a Switch full report.
fn set_motion(r: &mut [u8], samples: [[i16; 6]; 3]) {
    for (k, s) in samples.iter().enumerate() {
        for (i, v) in s.iter().enumerate() {
            let at = 13 + 12 * k + 2 * i;
            r[at..at + 2].copy_from_slice(&v.to_le_bytes());
        }
    }
}

fn switch_view() -> View {
    let mut v = View::new();
    v.set_time_ms(0);
    v.status("Connected");
    v.device(&Device {
        name: "Pro Controller",
        class_of_device: 0x002508,
        vendor_id: Some(0x057E),
        product_id: Some(0x2009),
        version: Some(0x0001),
        vendor_id_source: Some(2),
    });
    v.descriptor(&data::SWITCH_PRO);
    v
}

/// Feed a Switch pad 60 reports a second for a while (so the rate shows),
/// answering the start-up conversation.
fn run_switch(v: &mut View, ms: u32, report: &[u8]) {
    let mut buf = [0u8; 49];
    let mut t = 0;
    while t < ms {
        v.set_time_ms(t);
        if let Some(_n) = v.next_output(&mut buf) {
            let mut a = report.to_vec();
            a[0] = 0x21;
            a[13] = 0x80;
            a[14] = buf[10];
            if buf[10] == 0x02 {
                // Device info: firmware 3.72, Pro Controller.
                a[13] = 0x82;
                a[15..18].copy_from_slice(&[0x03, 0x48, 0x03]);
            }
            v.report(&a);
        }
        v.report(report);
        t += 16;
    }
}

#[test]
fn waiting_screen() {
    let mut f = Frame::new();
    let mut v = View::new();
    v.status("Searching for a gamepad");
    v.draw(&mut f).unwrap();
    f.save("0-waiting.png");
    // Not drawn again when nothing changed.
    f.reset();
    v.draw(&mut f).unwrap();
    assert_eq!(f.sent, 0);
    // A new status redraws only the status line.
    v.status("Pairing");
    f.reset();
    v.draw(&mut f).unwrap();
    assert!(f.sent > 0 && f.sent < (W * 26) as u64, "{}", f.sent);
    f.save("0-waiting-pairing.png");
}

#[test]
fn switch_pages() {
    let mut f = Frame::new();
    let mut v = switch_view();
    // Pressing A (Nintendo right button = East), L, and Up; left stick
    // pushed right and a bit up; right stick down-left; ZR held.
    let r = switch_report(0x08 | 0x80, 0x00, 0x02 | 0x40, 3400, 2600, 900, 900);
    run_switch(&mut v, 2000, &r);
    v.draw(&mut f).unwrap();
    f.save("1-switch-controller.png");
    // The A button (East) is drawn yellow: check its centre pixel.
    let yellow = Rgb565::new(31, 58, 0);
    let mut found = false;
    for y in 70..95 {
        for x in 250..270 {
            found |= f.at(x, y) == yellow;
        }
    }
    assert!(found, "pressed A should be yellow");
    v.next_page();
    v.draw(&mut f).unwrap();
    f.save("2-switch-data.png");
    v.next_page();
    v.draw(&mut f).unwrap();
    f.save("3-switch-about.png");
    v.next_page();
    v.draw(&mut f).unwrap();
    v.disconnected();
    v.status("Gamepad gone");
    v.draw(&mut f).unwrap();
    f.save("4-switch-disconnected.png");
}

#[test]
fn second_draw_sends_nothing_and_stick_move_is_small() {
    let mut f = Frame::new();
    let mut v = switch_view();
    let centre = switch_report(0, 0, 0, 2048, 2048, 2048, 2048);
    run_switch(&mut v, 1500, &centre);
    v.draw(&mut f).unwrap();
    let full = f.sent;
    println!("first draw: {} pixels in {} calls", full, f.calls);
    for page in 0..3 {
        f.reset();
        v.draw(&mut f).unwrap();
        assert_eq!(
            (f.sent, f.calls),
            (0, 0),
            "page {page}: second draw must send nothing"
        );
        v.next_page();
        v.draw(&mut f).unwrap();
    }
    // Back on page 1. The same report again: still nothing on screen
    // changes except the counters (total) - measure that too.
    v.set_time_ms(5000);
    v.report(&centre);
    f.reset();
    v.draw(&mut f).unwrap();
    println!("same report again: {} pixels in {} calls", f.sent, f.calls);
    // Move the left stick only.
    let moved = switch_report(0, 0, 0, 3000, 2048, 2048, 2048);
    v.report(&moved);
    f.reset();
    v.draw(&mut f).unwrap();
    let share = f.sent as f64 / (W * H) as f64 * 100.0;
    println!(
        "left stick move: {} pixels in {} calls ({:.1}% of the screen), box {:?}",
        f.sent, f.calls, share, f.touched
    );
    assert!(
        f.sent < (W * H) as u64 * 3 / 100,
        "a stick move should redraw less than 3% of the screen"
    );
    // One button press.
    let pressed = switch_report(0x08, 0, 0, 3000, 2048, 2048, 2048);
    v.report(&pressed);
    f.reset();
    v.draw(&mut f).unwrap();
    println!("A pressed: {} pixels in {} calls", f.sent, f.calls);
    println!("size of View: {} bytes", core::mem::size_of::<View>());
}

#[test]
fn xbox_and_generic_pages() {
    // GuliKit in XInput mode (by its name), 16-byte reports.
    let mut f = Frame::new();
    let mut v = View::new();
    v.set_time_ms(0);
    v.status("Connected");
    v.device(&Device {
        name: "GuliKit Controller XW",
        class_of_device: 0x000508,
        ..Default::default()
    });
    v.descriptor(&data::GULIKIT_KK2_ANDROID);
    let r = [
        0x01, 0x00, 0xC0, 0x00, 0x40, 0x00, 0x80, 0x00, 0x80, 0x00, 0x02, 0xFF, 0x03, 0x05, 0x11,
        0x01,
    ];
    for i in 0..200 {
        v.set_time_ms(i * 27);
        v.report(&r);
    }
    v.report(&[0x04, 0x64]);
    v.report(&r);
    v.draw(&mut f).unwrap();
    f.save("5-xinput-controller.png");
    v.next_page();
    v.draw(&mut f).unwrap();
    f.save("6-xinput-data.png");
    v.next_page();
    v.draw(&mut f).unwrap();
    f.save("7-xinput-about.png");

    // A DInput-style pad (guessed layout): the Asus Android descriptor
    // under a GuliKit name, no vendor id.
    let mut f = Frame::new();
    let mut v = View::new();
    v.set_time_ms(0);
    v.status("Connected");
    v.device(&Device {
        name: "GuliKit Controller",
        class_of_device: 0x000508,
        ..Default::default()
    });
    v.descriptor(&data::DUALSHOCK4_BT);
    let r = [0x01, 0x20, 0x80, 0xF0, 0x80, 0x86, 0x02, 0x00, 0x40, 0xC0];
    for i in 0..100 {
        v.set_time_ms(i * 28);
        v.report(&r);
    }
    v.draw(&mut f).unwrap();
    f.save("8-generic-controller.png");
    v.next_page();
    v.draw(&mut f).unwrap();
    f.save("9-generic-data.png");
    v.next_page();
    v.draw(&mut f).unwrap();
    f.save("10-generic-about.png");
}

/// Drawing only the changes must give the same picture as drawing it all.
#[test]
fn partial_updates_match_a_full_redraw() {
    let mut rng: u64 = 0x1234_5678;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    let mut v = switch_view();
    run_switch(&mut v, 500, &switch_report(0, 0, 0, 2048, 2048, 2048, 2048));
    let mut part = Frame::new();
    for page in 0..3 {
        for step in 0..300u32 {
            let r = next();
            let mut report = switch_report(
                (r & 0xCF) as u8 & if r & 0x100 != 0 { 0xFF } else { 0 },
                ((r >> 8) & 0x3F) as u8 & if r & 0x200 != 0 { 0xFF } else { 0 },
                ((r >> 16) & 0xCF) as u8 & if r & 0x400 != 0 { 0xFF } else { 0 },
                ((r >> 20) & 0xFFF) as u16,
                ((r >> 32) & 0xFFF) as u16,
                ((r >> 44) & 0xFFF) as u16,
                ((r >> 52) & 0xFFF) as u16,
            );
            // Motion too: sometimes none (all zeros), often a new random
            // tilt and turn, else the same as before.
            let m = next();
            match m % 4 {
                0 => set_motion(&mut report, [[0; 6]; 3]),
                1 | 2 => {
                    let a = |shift: u32| ((m >> shift) & 0x1FFF) as i16 - 0x1000;
                    let one = [a(2), a(15), a(28), a(41), a(50) / 4, a(51) * 3];
                    set_motion(&mut report, [one; 3]);
                }
                _ => {}
            }
            v.set_time_ms(1000 + step * 16);
            v.report(&report);
            if step % 50 == 7 {
                v.status(["Connected", "Connected, link encrypted", "OK"][(r % 3) as usize]);
            }
            if step == 250 {
                v.disconnected();
            }
            if step == 270 {
                v.device(&Device {
                    name: "Pro Controller",
                    class_of_device: 0x2508,
                    ..Default::default()
                });
            }
            v.draw(&mut part).unwrap();
        }
        // The raw hex lines and the motion values catch up within half a
        // second of the last report (they are drawn less often).
        v.set_time_ms(1000 + 300 * 16 + 600);
        v.draw(&mut part).unwrap();
        // Go round the pages back to this one: a full redraw.
        let mut full = Frame::new();
        for _ in 0..3 {
            v.next_page();
        }
        v.draw(&mut full).unwrap();
        let diff = part.px.iter().zip(&full.px).filter(|(a, b)| a != b).count();
        if diff != 0 {
            part.save(&format!("bad-{page}-partial.png"));
            full.save(&format!("bad-{page}-full.png"));
        }
        assert_eq!(
            diff, 0,
            "page {page}: partial drawing differs from a full redraw"
        );
        v.next_page();
        v.draw(&mut part).unwrap();
    }
}

// ---------------------------------------------------------------- motion

const BODY: Rgb565 = Rgb565::new(5, 11, 7);
/// The ball level (lower left grip) and the gyroscope lines (lower right
/// grip) of the Pad page.
const BALL_AREA: Rectangle = Rectangle::new(Point::new(40, 107), Size::new(43, 43));
const GYRO_AREA: Rectangle = Rectangle::new(Point::new(229, 112), Size::new(48, 37));

/// Gravity for a pad tilted by `roll` and `pitch` degrees, in raw counts
/// (4096 per g), and a turn in counts.
fn tilted(roll: f64, pitch: f64, gyro: [i16; 3]) -> [i16; 6] {
    let (r, p) = (roll.to_radians(), pitch.to_radians());
    let g = 4096.0;
    [
        (p.sin() * g).round() as i16,
        (r.sin() * p.cos() * g).round() as i16,
        (r.cos() * p.cos() * g).round() as i16,
        gyro[0],
        gyro[1],
        gyro[2],
    ]
}

fn motion_report(samples: [[i16; 6]; 3]) -> Vec<u8> {
    let mut r = switch_report(0, 0, 0, 2048, 2048, 2048, 2048);
    set_motion(&mut r, samples);
    r
}

/// True when no pixel in `area` differs from the body colour. Points on
/// or outside the body's rounded lower corners (radius 44, centres at
/// (68, 112) and (252, 112)) are left out.
fn plain_body(f: &Frame, area: Rectangle) -> bool {
    let inside = |p: &Point| {
        let cx = p.x.clamp(68, 252);
        let cy = if p.y > 112 { 112 } else { p.y };
        let (dx, dy) = (p.x - cx, p.y - cy);
        dx * dx + dy * dy < 41 * 41
    };
    area.points()
        .filter(inside)
        .all(|p| f.at(p.x as usize, p.y as usize) == BODY)
}

#[test]
fn motion_pages() {
    // Right grip down 20 degrees, nose down 10, turning.
    let mut f = Frame::new();
    let mut v = switch_view();
    let s = tilted(20.0, -10.0, [150, -40, 700]);
    run_switch(&mut v, 2000, &motion_report([s; 3]));
    v.draw(&mut f).unwrap();
    f.save("11-motion-controller.png");
    assert!(!plain_body(&f, BALL_AREA), "the ball level is drawn");
    // The ball is right of and below the centre (right side down, nose
    // down means... the ball rolls to the right and away from you: up).
    let yellow = Rgb565::new(31, 58, 0);
    let mut ball = (0, 0, 0);
    for p in BALL_AREA.points() {
        if f.at(p.x as usize, p.y as usize) == yellow {
            ball = (ball.0 + p.x, ball.1 + p.y, ball.2 + 1);
        }
    }
    assert!(ball.2 > 40, "a yellow ball: {ball:?}");
    let (bx, by) = (ball.0 / ball.2, ball.1 / ball.2);
    assert!(bx > 61 && by < 128, "ball at ({bx}, {by})");
    v.next_page();
    v.draw(&mut f).unwrap();
    f.save("12-motion-data.png");
    v.next_page();
    v.draw(&mut f).unwrap();
    f.save("13-motion-about.png");

    // Lying flat: the ball sits in the middle and is green.
    let mut f = Frame::new();
    let mut v = switch_view();
    run_switch(
        &mut v,
        1000,
        &motion_report([tilted(0.0, 0.0, [2, -3, 1]); 3]),
    );
    v.draw(&mut f).unwrap();
    f.save("14-motion-level-controller.png");
    assert_eq!(
        f.at(61, 128),
        Rgb565::new(6, 60, 8),
        "green ball in the middle"
    );

    // Face down: red ball.
    let mut f = Frame::new();
    let mut v = switch_view();
    run_switch(
        &mut v,
        1000,
        &motion_report([tilted(170.0, 5.0, [0, 0, 0]); 3]),
    );
    v.draw(&mut f).unwrap();
    f.save("15-motion-face-down-controller.png");
    assert!(
        BALL_AREA
            .points()
            .any(|p| f.at(p.x as usize, p.y as usize) == Rgb565::new(31, 8, 6)),
        "red ball"
    );
}

#[test]
fn no_motion_data() {
    // A Switch pad that sends zeros: grey gate, no ball, "No motion".
    let mut f = Frame::new();
    let mut v = switch_view();
    run_switch(&mut v, 1500, &motion_report([[0; 6]; 3]));
    assert!(v.pad().motion.is_none() && v.motion_shown().is_none());
    v.draw(&mut f).unwrap();
    f.save("16-nomotion-controller.png");
    let yellow = Rgb565::new(31, 58, 0);
    let green = Rgb565::new(6, 60, 8);
    assert!(
        BALL_AREA.points().all(|p| {
            let c = f.at(p.x as usize, p.y as usize);
            c != yellow && c != green
        }),
        "no ball"
    );
    v.next_page();
    v.draw(&mut f).unwrap();
    f.save("17-nomotion-data.png");
    v.next_page();
    v.draw(&mut f).unwrap();
    f.save("18-nomotion-about.png");

    // Other layouts draw nothing there: the page looks as before.
    let mut f = Frame::new();
    let mut v = View::new();
    v.device(&Device {
        name: "GuliKit Controller XW",
        class_of_device: 0x000508,
        ..Default::default()
    });
    v.descriptor(&data::GULIKIT_KK2_ANDROID);
    v.report(&[
        0x01, 0x00, 0xC0, 0x00, 0x40, 0x00, 0x80, 0x00, 0x80, 0x00, 0x02, 0xFF, 0x03, 0x05, 0x11,
        0x01,
    ]);
    v.draw(&mut f).unwrap();
    assert!(
        plain_body(&f, BALL_AREA) && plain_body(&f, GYRO_AREA),
        "XInput: no motion parts"
    );
    assert!(v.pad().motion.is_none());
}

/// What one motion change costs on the screen: the pixels and windows
/// sent inside the motion parts (the Pad page's ball and gyroscope lines,
/// the Data page's motion block) when the pad tilts and turns.
#[test]
fn motion_update_cost() {
    let areas = [
        ("Pad", vec![("ball", BALL_AREA), ("gyro", GYRO_AREA)]),
        (
            "Data",
            vec![(
                "block",
                Rectangle::new(Point::new(162, 24), Size::new(158, 144)),
            )],
        ),
    ];
    for (page, (name, parts)) in areas.iter().enumerate() {
        let mut f = Frame::new();
        let mut v = switch_view();
        let a = motion_report([tilted(0.0, 0.0, [0, 0, 0]); 3]);
        let b = motion_report([tilted(25.0, 15.0, [300, -200, 900]); 3]);
        run_switch(&mut v, 1000, &a);
        for _ in 0..page {
            v.next_page();
        }
        v.draw(&mut f).unwrap();
        v.set_time_ms(3000);
        v.report(&b);
        let (mut px, mut win) = (0, 0);
        for (part, area) in parts {
            // Draw on a copy of the view each time, so each part is
            // measured on the same change.
            let mut g = Frame::new();
            g.px = f.px.clone();
            g.watch = Some(*area);
            let mut w = switch_view();
            run_switch(&mut w, 1000, &a);
            for _ in 0..page {
                w.next_page();
            }
            w.draw(&mut g).unwrap();
            w.set_time_ms(3000);
            w.report(&b);
            g.reset();
            w.draw(&mut g).unwrap();
            println!(
                "{name} page, {part}: {} pixels in {} windows",
                g.watched, g.watched_windows
            );
            px += g.watched;
            win += g.watched_windows;
        }
        let us = win * 50 + px / 2;
        println!(
            "{name} page: one motion change costs {px} pixels in {win} windows, about {us} us on the board"
        );
        match page {
            0 => assert!(
                px < 1_500 && win < 40,
                "Pad page motion change too big: {px} px, {win} windows"
            ),
            _ => assert!(
                px < 5_000 && win < 120,
                "Data page motion change too big: {px} px, {win} windows"
            ),
        }
        // Drawing only the change leaves nothing behind: compare with a
        // full redraw, after going back to level again.
        v.draw(&mut f).unwrap();
        v.set_time_ms(3400);
        v.report(&a);
        v.draw(&mut f).unwrap();
        // The raw hex lines catch up within half a second.
        v.set_time_ms(4000);
        v.draw(&mut f).unwrap();
        let mut full = Frame::new();
        for _ in 0..3 {
            v.next_page();
        }
        v.draw(&mut full).unwrap();
        assert!(
            f.px == full.px,
            "{name} page: partial motion drawing differs from a full redraw"
        );
    }
}

/// A pad lying still sends sensor noise in every report: the motion parts
/// must not be redrawn. A pad that moves all the time: at most about 10
/// redraws a second.
#[test]
fn motion_rate_limit() {
    let mut f = Frame::new();
    let mut v = switch_view();
    let flat = tilted(3.0, -2.0, [10, -14, 5]);
    run_switch(&mut v, 500, &motion_report([flat; 3]));
    v.draw(&mut f).unwrap();
    let changes0 = v.motion_changes();
    // 3 s of noise: a few counts on every number, 66 reports a second,
    // a draw every 33 ms.
    let mut seed: u32 = 7;
    let mut noise = |amp: i32| {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        ((seed >> 16) as i32 % (2 * amp + 1)) - amp
    };
    f.watch = Some(BALL_AREA);
    let mut ball_px = 0;
    let mut gyro_px = 0;
    let mut t = 1000;
    while t < 4000 {
        let mut s = flat;
        for (i, x) in s.iter_mut().enumerate() {
            *x += noise(if i < 3 { 12 } else { 4 }) as i16;
        }
        v.set_time_ms(t);
        v.report(&motion_report([s; 3]));
        if t % 33 < 15 {
            f.reset();
            v.draw(&mut f).unwrap();
            ball_px += f.watched;
            f.watch = Some(GYRO_AREA);
            f.reset();
            v.draw(&mut f).unwrap();
            gyro_px += f.watched;
            f.watch = Some(BALL_AREA);
        }
        t += 15;
    }
    println!(
        "still pad, 3 s of noise: {} motion changes, {} ball pixels, {} gyro pixels",
        v.motion_changes() - changes0,
        ball_px,
        gyro_px
    );
    assert_eq!(
        v.motion_changes(),
        changes0,
        "noise must not change the shown values"
    );
    assert_eq!(
        (ball_px, gyro_px),
        (0, 0),
        "noise must not redraw the motion parts"
    );

    // Turning all the time, a report every 8 ms for 2 s.
    let start = v.motion_changes();
    let mut t = 5000u32;
    let mut k = 0;
    let mut draws_with_ball = 0;
    f.watch = Some(BALL_AREA);
    while t < 7000 {
        let s = tilted(
            (k as f64 * 3.0) % 60.0 - 30.0,
            0.0,
            [(k * 37 % 2000) as i16, 0, 0],
        );
        v.set_time_ms(t);
        v.report(&motion_report([s; 3]));
        if t.is_multiple_of(32) {
            f.reset();
            v.draw(&mut f).unwrap();
            draws_with_ball += (f.watched > 0) as u32;
        }
        t += 8;
        k += 1;
    }
    let n = v.motion_changes() - start;
    println!(
        "moving pad, 2 s at 125 reports a second: {n} motion changes, {draws_with_ball} draws touched the ball"
    );
    assert!(
        (15..=21).contains(&n),
        "about 10 changes a second, got {n} in 2 s"
    );
    assert!(draws_with_ball <= n);
}

/// Screen time a whole page costs per second on the board, from what one
/// test run sent: about 50 microseconds per window plus 0.5 per pixel
/// (16-bit pixels at 40 MHz SPI, rounded up).
fn board_ms(windows: u64, pixels: u64) -> f64 {
    (windows as f64 * 50.0 + pixels as f64 * 0.5) / 1000.0
}

/// Run a Switch pad for `secs` seconds on one page: a report every 15 ms
/// (66 a second), a draw every 33 ms, motion `kind`: 0 none (zeros),
/// 1 lying still with sensor noise, 2 rocking all the time. Returns the
/// windows and pixels sent by all the draws after the first one.
fn page_cost(page: usize, kind: u32, secs: u32) -> (u64, u64) {
    let mut f = Frame::new();
    let mut v = switch_view();
    let flat = tilted(3.0, -2.0, [10, -14, 5]);
    run_switch(
        &mut v,
        500,
        &motion_report(if kind == 0 { [[0; 6]; 3] } else { [flat; 3] }),
    );
    for _ in 0..page {
        v.next_page();
    }
    v.draw(&mut f).unwrap();
    f.reset();
    let mut seed: u32 = 99;
    let mut noise = |amp: i32| {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        ((seed >> 16) as i32 % (2 * amp + 1)) - amp
    };
    let (mut t, mut k, mut next_draw) = (1000u32, 0u32, 1000u32);
    let end = 1000 + secs * 1000;
    let mut report = motion_report([flat; 3]);
    while t < end {
        let mut s = match kind {
            2 => tilted(
                40.0 * ((t as f64) / 4000.0 * std::f64::consts::TAU).sin(),
                25.0 * ((t as f64) / 6000.0 * std::f64::consts::TAU).sin(),
                [(k * 37 % 900) as i16, -200, 300],
            ),
            _ => flat,
        };
        for (i, x) in s.iter_mut().enumerate() {
            *x += noise(if i < 3 { 12 } else { 4 }) as i16;
        }
        // The timer byte counts up, as on a real pad.
        report[1] = k as u8;
        set_motion(&mut report, if kind == 0 { [[0; 6]; 3] } else { [s; 3] });
        v.set_time_ms(t);
        v.report(&report);
        while next_draw <= t {
            v.set_time_ms(next_draw);
            v.draw(&mut f).unwrap();
            next_draw += 33;
        }
        t += 15;
        k += 1;
    }
    (f.windows, f.sent)
}

/// What the whole page costs per second with motion on, against the same
/// pad with the sensor off. The motion display must not take more than 5%
/// of the board's time (50 ms in each second). A still pad's sensor noise
/// changes only the raw hex lines (redrawn twice a second at most): under
/// 2.5%. Before the hex lines were limited, a still pad cost 276 ms a
/// second on the Pad page (28% of the time).
#[test]
fn motion_share_of_screen_time() {
    for (page, name) in [(0, "Pad"), (1, "Data")] {
        let secs = 4;
        let base = page_cost(page, 0, secs);
        let still = page_cost(page, 1, secs);
        let moving = page_cost(page, 2, secs);
        let per_s = |c: (u64, u64)| board_ms(c.0, c.1) / secs as f64;
        println!(
            "{name} page per second: sensor off {:.1} ms ({} windows), still pad {:.1} ms ({} windows), moving pad {:.1} ms ({} windows)",
            per_s(base),
            base.0 / secs as u64,
            per_s(still),
            still.0 / secs as u64,
            per_s(moving),
            moving.0 / secs as u64
        );
        assert!(
            per_s(still) - per_s(base) < 25.0,
            "{name} page: a still pad's sensor noise costs {:.1} ms a second",
            per_s(still) - per_s(base)
        );
        assert!(
            per_s(moving) - per_s(base) < 50.0,
            "{name} page: motion costs {:.1} ms a second (over 5%)",
            per_s(moving) - per_s(base)
        );
    }
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// The GuliKit in DInput mode with its real descriptor and reports: LT
/// held (the bar must be full), then a vendor bit and a consumer bit that
/// no control takes.
#[test]
fn dinput_pages() {
    let mut f = Frame::new();
    let mut v = View::new();
    v.set_time_ms(0);
    v.status("Connected");
    v.device(&Device {
        name: "GuliKit Controller DW",
        class_of_device: 0x002508,
        vendor_id: Some(0x1949),
        product_id: Some(0x0402),
        version: Some(0x011B),
        vendor_id_source: Some(2),
    });
    v.descriptor(&data::GULIKIT_KK2_DINPUT);
    let idle = hex("07807f807f000f0000");
    let lt = hex("07807f807f000f0400");
    let mut t = 0;
    while t < 3000 {
        v.set_time_ms(t);
        v.report(if t > 2000 { &lt } else { &idle });
        t += 10;
    }
    v.draw(&mut f).unwrap();
    f.save("19-dinput-lt-controller.png");
    assert!(v.triggers_on_off());
    assert_eq!(v.pad().lt.value, 1023);

    // A vendor bit and AC Back held.
    v.report(&[0x02, 0x40]);
    let vendor = hex("07807f807f000f0001");
    while t < 4000 {
        v.set_time_ms(t);
        v.report(&vendor);
        t += 10;
    }
    v.draw(&mut f).unwrap();
    f.save("20-dinput-unmapped-controller.png");
    let (_, total) = v.unmapped_shown();
    assert_eq!(total, 2);
    v.next_page();
    v.draw(&mut f).unwrap();
    f.save("21-dinput-unmapped-data.png");
}
