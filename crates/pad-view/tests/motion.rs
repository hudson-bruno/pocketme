//! The motion sensor of a Switch pad: reading the three samples of report
//! 0x30, turning them into g and degrees per second, the tilt, the "no
//! motion data" rule, and the limit on how often the screen changes.
//!
//! The reports are built by hand from the layout in dekuNukem's notes:
//! bytes 13 to 48 hold three samples of accelerometer x, y, z and
//! gyroscope x, y, z, 16-bit numbers, low byte first.

use pad_view::decode::Layout;
use pad_view::motion::{ACCEL_COUNTS_PER_G, GYRO_DPS_PER_COUNT, Gate, Motion};
use pad_view::{Device, View};

/// A Switch full report 0x30 with nothing pressed, sticks anywhere, and
/// the three motion samples given.
fn report(samples: [[i16; 6]; 3]) -> Vec<u8> {
    let mut r = vec![0u8; 49];
    r[0] = 0x30;
    r[1] = 0x10;
    r[2] = 0x8E;
    for (k, s) in samples.iter().enumerate() {
        for (i, v) in s.iter().enumerate() {
            let at = 13 + k * 12 + i * 2;
            r[at..at + 2].copy_from_slice(&v.to_le_bytes());
        }
    }
    r
}

fn still(accel: [i16; 3]) -> Motion {
    let s = [accel[0], accel[1], accel[2], 0, 0, 0];
    Motion { samples: [s; 3] }
}

fn close(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

fn switch_view() -> View {
    let mut v = View::new();
    v.device(&Device {
        name: "Pro Controller",
        class_of_device: 0x002508,
        ..Default::default()
    });
    v
}

#[test]
fn three_samples_in_order() {
    let a = [10, -20, 4000, 1, -2, 3];
    let b = [11, -21, 4001, 100, -200, 300];
    let c = [-4096, 2048, 32767, -32768, 1000, -1000];
    let m = Motion::from_switch_report(&report([a, b, c])).expect("motion data");
    assert_eq!(m.samples, [a, b, c]);
    // The third sample is taken as the newest (see `Motion::newest`).
    assert_eq!(m.newest(), c);
}

#[test]
fn units() {
    let m = Motion::from_switch_report(&report([[0, 0, 4096, 1000, -1000, 0]; 3])).unwrap();
    let g = m.accel_g();
    assert!(
        close(g[0], 0.0, 1e-6) && close(g[1], 0.0, 1e-6) && close(g[2], 1.0, 1e-6),
        "{g:?}"
    );
    let d = m.gyro_dps();
    // 1000 counts at 0.070 degrees a second per count.
    assert!(
        close(d[0], 70.0, 1e-3) && close(d[1], -70.0, 1e-3) && close(d[2], 0.0, 1e-6),
        "{d:?}"
    );
    assert_eq!(ACCEL_COUNTS_PER_G, 4096.0);
    assert_eq!(GYRO_DPS_PER_COUNT, 0.070);
    // The extremes stay finite: +-8 g and about +-2294 degrees a second.
    let m = Motion::from_switch_report(&report([[-32768, 32767, 0, -32768, 32767, 0]; 3])).unwrap();
    assert!(close(m.accel_g()[0], -8.0, 1e-6) && close(m.gyro_dps()[1], 2293.69, 0.01));
}

#[test]
fn tilt_flat_sides_and_nose() {
    // (accelerometer, roll, pitch, face down)
    let cases = [
        ([0, 0, 4096], 0.0f32, 0.0f32, false),
        // Right grip down: gravity seen as a push towards +y (left side up).
        ([0, 4096, 0], 90.0, 0.0, false),
        ([0, -4096, 0], -90.0, 0.0, false),
        // Nose (triggers) up.
        ([4096, 0, 0], 0.0, 90.0, false),
        ([-4096, 0, 0], 0.0, -90.0, false),
        // 30 degrees to the right: sin 30 = 0.5, cos 30 = 0.866.
        ([0, 2048, 3547], 30.0, 0.0, false),
        // 20 degrees nose up: sin 20 = 0.342, cos 20 = 0.940.
        ([1401, 0, 3849], 0.0, 20.0, false),
        // Face down.
        ([0, 0, -4096], 180.0, 0.0, true),
    ];
    for (a, roll, pitch, down) in cases {
        let t = still(a).tilt();
        assert!(
            close(t.roll_deg.abs(), roll.abs(), 0.5),
            "{a:?}: roll {} want {roll}",
            t.roll_deg
        );
        if roll != 180.0 {
            assert!(
                close(t.roll_deg, roll, 0.5),
                "{a:?}: roll {} want {roll}",
                t.roll_deg
            );
        }
        assert!(
            close(t.pitch_deg, pitch, 0.5),
            "{a:?}: pitch {} want {pitch}",
            t.pitch_deg
        );
        assert_eq!(t.face_down, down, "{a:?}");
        assert_eq!(t.is_level(3.0), a == [0, 0, 4096], "{a:?}");
    }
    // Where a ball on the pad would roll: right when the right grip is
    // down, towards you (down on the screen) when the nose is up.
    assert_eq!(still([0, 0, 4096]).downhill(13), (0, 0));
    assert_eq!(still([0, 4096, 0]).downhill(13), (13, 0));
    assert_eq!(still([0, -4096, 0]).downhill(13), (-13, 0));
    assert_eq!(still([4096, 0, 0]).downhill(13), (0, 13));
}

#[test]
fn tilt_uses_the_mean_of_three_samples() {
    let m = Motion {
        samples: [
            [0, 300, 4096, 0, 0, 0],
            [0, -300, 4096, 0, 0, 0],
            [0, 0, 4096, 0, 0, 0],
        ],
    };
    assert_eq!(m.accel_mean(), [0, 0, 4096]);
    assert!(m.tilt().is_level(0.1));
}

#[test]
fn all_zero_means_no_motion_data() {
    assert_eq!(Motion::from_switch_report(&report([[0; 6]; 3])), None);
    // Gyroscope numbers alone do not count: the accelerometer always feels
    // gravity when the sensor is on.
    assert_eq!(
        Motion::from_switch_report(&report([[0, 0, 0, 5, 5, 5]; 3])),
        None
    );
    // Too short for one sample.
    assert_eq!(
        Motion::from_switch_report(&report([[0, 0, 4096, 0, 0, 0]; 3])[..24]),
        None
    );
    // One whole sample: it fills the other two places.
    let m = Motion::from_switch_report(&report([[1, 2, 3, 4, 5, 6]; 3])[..25]).unwrap();
    assert_eq!(m.samples, [[1, 2, 3, 4, 5, 6]; 3]);
}

#[test]
fn view_keeps_motion_for_switch_only() {
    let mut v = switch_view();
    v.report(&report([[0; 6]; 3]));
    assert_eq!(v.layout().0, Layout::SwitchPro);
    assert_eq!(v.pad().motion, None);
    assert!(v.motion_shown().is_none());
    assert_eq!(v.motion_changes(), 0);

    v.report(&report([[0, 0, 4096, 0, 0, 0]; 3]));
    assert_eq!(
        v.motion_shown().map(|m| m.newest()),
        Some([0, 0, 4096, 0, 0, 0])
    );
    assert_eq!(v.motion_changes(), 1);

    // A subcommand reply (0x21) carries the reply in those bytes, not
    // motion: the last motion stays.
    let mut reply = report([[0; 6]; 3]);
    reply[0] = 0x21;
    reply[13] = 0x80;
    reply[14] = 0x40;
    v.report(&reply);
    assert_eq!(
        v.pad().motion.map(|m| m.newest()),
        Some([0, 0, 4096, 0, 0, 0])
    );

    // The simple report 0x3F has no motion.
    v.report(&[
        0x3F, 0x00, 0x00, 0x08, 0x00, 0x80, 0x00, 0x80, 0x00, 0x80, 0x00, 0x80,
    ]);
    assert_eq!(v.pad().motion, None);
    assert!(v.motion_shown().is_none());
    assert_eq!(v.motion_changes(), 2);

    // An Xbox pad never shows motion.
    let mut x = View::new();
    x.device(&Device {
        name: "Xbox Wireless Controller",
        class_of_device: 0x002508,
        vendor_id: Some(0x045E),
        product_id: Some(0x02E0),
        ..Default::default()
    });
    x.report(&[0x01; 17]);
    assert_ne!(x.layout().0, Layout::SwitchPro);
    assert!(x.motion_shown().is_none());
}

#[test]
fn gate_ignores_noise_and_limits_the_pace() {
    let mut g = Gate::new();
    assert!(!g.offer(Some(0), None), "no data, nothing to change");
    assert!(
        g.offer(Some(0), Some(&still([0, 0, 4096]))),
        "data appearing shows at once"
    );
    // Noise of a few counts, long after: no change.
    for (t, n) in [(500, 5i16), (1000, -7), (1500, 20), (2000, -40)] {
        let mut m = still([n, -n, 4096 + n]);
        m.samples[2][3] = n / 2;
        assert!(!g.offer(Some(t), Some(&m)), "noise {n} at {t} ms");
    }
    assert_eq!(g.changes, 1);
    // A real move, but too soon after a change: wait.
    let tilted = still([0, 2048, 3547]);
    assert!(g.offer(Some(2100), Some(&tilted)));
    let more = still([0, 4096, 0]);
    assert!(
        !g.offer(Some(2150), Some(&more)),
        "50 ms after the last change"
    );
    assert!(
        g.offer(Some(2200), Some(&more)),
        "100 ms after the last change"
    );
    // A turn shows past about 2 degrees a second.
    let mut turning = more;
    turning.samples[2][5] = 40;
    assert!(g.offer(Some(2400), Some(&turning)));
    // Data going away waits for the gap too (a pad flipping between data
    // and zeros must not redraw on every report).
    assert!(!g.offer(Some(2410), None));
    assert!(g.offer(Some(2500), None));
    assert!(g.shown().is_none());
    assert_eq!(g.changes, 5);
    // Without a clock only the steps count.
    let mut g = Gate::new();
    assert!(g.offer(None, Some(&still([0, 0, 4096]))));
    assert!(g.offer(None, Some(&still([0, 4096, 0]))));
    assert!(!g.offer(None, Some(&still([3, 4090, 2]))));
    // The clock running over (after 49 days) does not stop the screen.
    let mut g = Gate::new();
    assert!(g.offer(Some(u32::MAX - 50), Some(&still([0, 0, 4096]))));
    assert!(g.offer(Some(60), Some(&still([0, 4096, 0]))));
}
