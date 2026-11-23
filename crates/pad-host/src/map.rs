//! From pad-view's decoded pad to what a game sees: board-api's
//! `PadState` and `Motion`, a stick dead zone, the battery, and the rule
//! for "something changed, the console should wake".

use board_api::input::TRIGGER_MAX;
use board_api::{Motion, PadState, Stick, btn};
use pad_view::decode::{Axis, Layout, Pad};

/// Stick values at or below this distance from the middle read 0 (about
/// 10 % of the travel). Above it the value grows again from 0, so the
/// full range stays usable. A resting stick of the GuliKit sits within
/// about 3 % of the middle in the logs.
pub const DEAD_ZONE: i32 = 3277;
/// The largest stick value (board-api: -32767 to +32767).
pub const STICK_MAX: i32 = 32767;
/// A stick counts as moved (a change worth waking for) when an axis moved
/// at least this far since the last change: about 6 % of the travel.
pub const STICK_STEP: i32 = 2048;
/// A trigger counts as moved when it moved at least this far (of 1023).
pub const TRIGGER_STEP: i32 = 32;

/// The integer square root (floor).
fn isqrt(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    // Newton's method from a guess above the root; at most ~35 steps.
    let mut x = n;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// An axis as -32767..=32767 (0 when the pad never sent it).
fn axis_value(a: &Axis) -> i32 {
    if !a.present {
        return 0;
    }
    let n = a.norm;
    // NaN never comes from the decoder; treat it as the middle anyway.
    if n.is_nan() {
        return 0;
    }
    let v = n.clamp(-1.0, 1.0) * STICK_MAX as f32;
    v as i32
}

/// A stick with the round dead zone: inside it reads (0, 0); outside, the
/// distance from the zone's edge is stretched to the full range, in the
/// same direction. Each axis is clamped to -32767..=32767.
pub fn stick(x: i32, y: i32) -> Stick {
    let (x, y) = (
        x.clamp(-STICK_MAX, STICK_MAX) as i64,
        y.clamp(-STICK_MAX, STICK_MAX) as i64,
    );
    let r = isqrt((x * x + y * y) as u64) as i64;
    if r <= DEAD_ZONE as i64 {
        return Stick { x: 0, y: 0 };
    }
    let over = r - DEAD_ZONE as i64;
    let span = (STICK_MAX - DEAD_ZONE) as i64;
    let m = STICK_MAX as i64;
    let scale = |v: i64| -> i16 { (v * over * m / (span * r)).clamp(-m, m) as i16 };
    Stick {
        x: scale(x),
        y: scale(y),
    }
}

/// The decoded pad as board-api's `PadState` (connected).
pub fn pad_state(p: &Pad) -> PadState {
    let trig = |t: &pad_view::decode::Trigger| -> u16 {
        if t.present {
            t.value.min(TRIGGER_MAX)
        } else {
            0
        }
    };
    PadState {
        connected: true,
        buttons: p.buttons & btn::ALL,
        left: stick(axis_value(&p.lx), axis_value(&p.ly)),
        right: stick(axis_value(&p.rx), axis_value(&p.ry)),
        lt: trig(&p.lt),
        rt: trig(&p.rt),
    }
}

/// Gyroscope: the pad gives 0.070 degrees per second per count, the
/// console wants 16 per degree per second: times 1.12.
fn gyro(v: i16) -> i16 {
    (v as i32 * 112 / 100).clamp(i16::MIN as i32 + 1, i16::MAX as i32) as i16
}

fn neg(v: i16) -> i16 {
    // -(-32768) does not fit: it becomes +32767.
    v.checked_neg().unwrap_or(i16::MAX)
}

/// The newest motion sample of a Switch-mode pad in the console's units
/// and axes. The pad's axes (pad-view's motion notes): x away from the
/// player, y to the left, z up. The console's: x right, y away, z up. So
/// console x = -pad y, console y = pad x, z stays; the gyroscope turns the
/// same way (a turn about z by 90 degrees keeps the right-hand rule).
/// The accelerometer's units are the same (4096 per g).
pub fn motion(m: &pad_view::motion::Motion) -> Motion {
    let s = m.newest();
    let (ax, ay, az, gx, gy, gz) = (s[0], s[1], s[2], s[3], s[4], s[5]);
    Motion {
        accel: [neg(ay), ax, az],
        gyro: [neg(gyro(gy)), gyro(gx), gyro(gz)],
    }
}

/// The battery, when the pad's mode reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    /// 0, 25, 50, 75 or 100 in Switch mode; 0, 33, 66 or 100 in the Xbox
    /// modes. Rough: the pad gives only a few levels.
    pub percent: u8,
    pub charging: bool,
    /// The level as the pad sent it (Switch: 0-4, Xbox: 0-3).
    pub level: u8,
}

/// The battery from the decoded byte. Switch mode: pad-view keeps the high
/// nibble of report byte 2: level = n >> 1 (0 empty to 4 full), bit 0 =
/// charging. Xbox modes (report 04): bits 0-1 the level (0 to 3), bit 4
/// charging; not yet checked on the GuliKit.
pub fn battery(layout: Layout, raw: Option<u8>) -> Option<Battery> {
    let b = raw?;
    match layout {
        Layout::SwitchPro => {
            let level = (b >> 1).min(4);
            Some(Battery {
                percent: level * 25,
                charging: b & 1 != 0,
                level,
            })
        }
        Layout::Xbox => {
            let level = b & 0x03;
            Some(Battery {
                percent: (level as u16 * 100 / 3) as u8,
                charging: b & 0x10 != 0,
                level,
            })
        }
        _ => None,
    }
}

/// True when `now` differs from `seen` (the state at the last change)
/// enough to wake the console:
///
/// * any button went down or up;
/// * a stick axis left the dead zone or came back to it, or moved at
///   least [`STICK_STEP`];
/// * a trigger left 0 or came back to it, reached the end or left it, or
///   moved at least [`TRIGGER_STEP`];
/// * the pad came or went.
///
/// Not a change: motion, the report counter, a stick wobbling inside the
/// dead zone or by less than a step. A pad lying still sends 70 to 130
/// reports a second (logs); none of them is a change.
pub fn changed(seen: &PadState, now: &PadState) -> bool {
    if seen.connected != now.connected || seen.buttons != now.buttons {
        return true;
    }
    let axis = |a: i16, b: i16| -> bool {
        (a == 0) != (b == 0) || (a as i32 - b as i32).abs() >= STICK_STEP
    };
    let trig = |a: u16, b: u16| -> bool {
        (a == 0) != (b == 0)
            || (a == TRIGGER_MAX) != (b == TRIGGER_MAX)
            || (a as i32 - b as i32).abs() >= TRIGGER_STEP
    };
    axis(seen.left.x, now.left.x)
        || axis(seen.left.y, now.left.y)
        || axis(seen.right.x, now.right.x)
        || axis(seen.right.y, now.right.y)
        || trig(seen.lt, now.lt)
        || trig(seen.rt, now.rt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isqrt_is_exact() {
        for n in (0..200_000_u64).chain([u32::MAX as u64, 2 * 32767 * 32767]) {
            let r = isqrt(n);
            assert!(r * r <= n && (r + 1) * (r + 1) > n, "{n}");
        }
    }

    #[test]
    fn dead_zone_is_round_and_the_range_stays_full() {
        assert_eq!(stick(0, 0), Stick { x: 0, y: 0 });
        assert_eq!(stick(3000, 1000), Stick { x: 0, y: 0 });
        assert_eq!(
            stick(-2300, 2300),
            Stick { x: 0, y: 0 },
            "inside the circle"
        );
        assert_eq!(stick(32767, 0), Stick { x: 32767, y: 0 });
        assert_eq!(stick(0, -32767), Stick { x: 0, y: -32767 });
        let s = stick(3400, 0);
        assert!(
            s.x > 0 && s.x < 300,
            "{s:?}: just past the edge reads small"
        );
        // A corner (both axes full) stays in range and keeps its direction.
        let c = stick(32767, 32767);
        assert_eq!(c.x, c.y);
        assert!(c.x > 20000);
        for (x, y) in [
            (i32::MIN, i32::MAX),
            (40000, -40000),
            (-32768, -32768),
            (12345, -30000),
        ] {
            let s = stick(x, y);
            assert!(s.x > -32768 && s.y > -32768, "{s:?}");
        }
    }

    #[test]
    fn btn_bits_are_board_api_bits() {
        use pad_view::decode::Btn;
        let want = [
            (Btn::South, btn::SOUTH),
            (Btn::East, btn::EAST),
            (Btn::West, btn::WEST),
            (Btn::North, btn::NORTH),
            (Btn::L, btn::L),
            (Btn::R, btn::R),
            (Btn::ZL, btn::ZL),
            (Btn::ZR, btn::ZR),
            (Btn::Minus, btn::MINUS),
            (Btn::Plus, btn::PLUS),
            (Btn::Home, btn::HOME),
            (Btn::Capture, btn::CAPTURE),
            (Btn::LStick, btn::LSTICK),
            (Btn::RStick, btn::RSTICK),
            (Btn::Up, btn::UP),
            (Btn::Down, btn::DOWN),
            (Btn::Left, btn::LEFT),
            (Btn::Right, btn::RIGHT),
        ];
        assert_eq!(want.len(), pad_view::decode::BTN_COUNT);
        for (b, bit) in want {
            assert_eq!(b.bit(), bit, "{b:?}");
        }
    }

    #[test]
    fn motion_axes_and_units() {
        // Flat and still: gravity on z only.
        let mut m = pad_view::motion::Motion::default();
        m.samples[2] = [0, 0, 4096, 0, 0, 0];
        assert_eq!(motion(&m).accel, [0, 0, 4096]);
        // Pad x (away) -> console y; pad y (left) -> console -x.
        m.samples[2] = [100, 200, 4096, 1000, -2000, 50];
        let c = motion(&m);
        assert_eq!(c.accel, [-200, 100, 4096]);
        // 1000 counts x 0.07 = 70 dps = 1120 console units.
        assert_eq!(c.gyro, [2240, 1120, 56]);
        // The ends do not wrap round.
        m.samples[2] = [i16::MIN, i16::MIN, i16::MIN, i16::MIN, i16::MAX, i16::MIN];
        let c = motion(&m);
        assert_eq!(c.accel[0], i16::MAX);
        assert_eq!(c.gyro[0], -32767);
        assert_eq!(c.gyro[1], -32767);
        assert_eq!(c.gyro[2], -32767);
        // The newest of the three samples is used.
        m.samples = [[1, 1, 1, 1, 1, 1], [2, 2, 2, 2, 2, 2], [0, 0, 7, 0, 0, 0]];
        assert_eq!(motion(&m).accel, [0, 0, 7]);
    }

    #[test]
    fn battery_levels() {
        // Live log (Switch mode): report byte 2 = 0x80, nibble 8 = full.
        assert_eq!(
            battery(Layout::SwitchPro, Some(0x8)),
            Some(Battery {
                percent: 100,
                charging: false,
                level: 4
            })
        );
        assert_eq!(battery(Layout::SwitchPro, Some(0x3)).unwrap().percent, 25);
        assert!(battery(Layout::SwitchPro, Some(0x3)).unwrap().charging);
        assert_eq!(battery(Layout::SwitchPro, Some(0xF)).unwrap().level, 4);
        assert_eq!(battery(Layout::Xbox, Some(0x13)).unwrap().percent, 100);
        assert!(battery(Layout::Xbox, Some(0x13)).unwrap().charging);
        assert_eq!(battery(Layout::Xbox, Some(0x01)).unwrap().percent, 33);
        assert_eq!(battery(Layout::Generic, Some(0x01)), None);
        assert_eq!(battery(Layout::SwitchPro, None), None);
    }

    fn st(lx: i16, lt: u16, buttons: u32) -> PadState {
        PadState {
            connected: true,
            buttons,
            left: Stick { x: lx, y: 0 },
            lt,
            ..PadState::NONE
        }
    }

    #[test]
    fn what_counts_as_change() {
        let a = st(0, 0, 0);
        assert!(!changed(&a, &a));
        assert!(changed(&a, &st(0, 0, btn::SOUTH)), "a button");
        assert!(changed(&a, &st(5, 0, 0)), "a stick leaves the dead zone");
        assert!(
            changed(&st(5, 0, 0), &a),
            "a stick comes back to the middle"
        );
        assert!(
            !changed(&st(5000, 0, 0), &st(6000, 0, 0)),
            "less than a step"
        );
        assert!(changed(&st(5000, 0, 0), &st(7048, 0, 0)), "a step");
        assert!(changed(&a, &st(0, 1, 0)), "a trigger leaves 0");
        assert!(!changed(&st(0, 500, 0), &st(0, 520, 0)));
        assert!(changed(&st(0, 500, 0), &st(0, 532, 0)));
        assert!(
            changed(&st(0, 1000, 0), &st(0, 1023, 0)),
            "a trigger reaches the end"
        );
        assert!(changed(&a, &PadState::NONE), "the pad goes");
    }
}
