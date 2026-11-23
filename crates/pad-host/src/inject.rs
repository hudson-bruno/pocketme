//! Reports made up for testing without a pad: a Switch-mode full report
//! (0x30) built from what a game should see, and the small changes a pad
//! lying still makes from one report to the next.
//!
//! Used by the PC tool (`boardctl pad`) to build what it injects, and by
//! the board to repeat an injected report at a pad's rate. The layout is
//! the one pad-view decodes (dekuNukem's notes, SDL's reading of it):
//!
//! ```text
//! 0      0x30 (report id)
//! 1      timer (counts up)
//! 2      battery (high nibble) and connection (low nibble)
//! 3-5    buttons: right byte, middle byte, left byte
//! 6-8    left stick: two 12-bit numbers
//! 9-11   right stick
//! 12     vibration answer
//! 13-48  motion: three samples of accel x y z, gyro x y z (i16 LE)
//! ```

use board_api::btn;

/// Length of a Switch full report without the 0xA1 byte.
pub const SWITCH_REPORT_LEN: usize = 49;
/// The stick's middle and the distance to its end (as pad-view reads it).
const MID: i32 = 2048;
const HALF: i32 = 1600;
/// Motion bytes start here; three samples of 12 bytes.
const MOTION_AT: usize = 13;

/// Which bit of which byte each button is in a Switch full report.
const BUTTONS: [(u32, usize, u8); 18] = [
    (btn::WEST, 3, 0x01),  // Y
    (btn::NORTH, 3, 0x02), // X
    (btn::SOUTH, 3, 0x04), // B
    (btn::EAST, 3, 0x08),  // A
    (btn::R, 3, 0x40),
    (btn::ZR, 3, 0x80),
    (btn::MINUS, 4, 0x01),
    (btn::PLUS, 4, 0x02),
    (btn::RSTICK, 4, 0x04),
    (btn::LSTICK, 4, 0x08),
    (btn::HOME, 4, 0x10),
    (btn::CAPTURE, 4, 0x20),
    (btn::DOWN, 5, 0x01),
    (btn::UP, 5, 0x02),
    (btn::RIGHT, 5, 0x04),
    (btn::LEFT, 5, 0x08),
    (btn::L, 5, 0x40),
    (btn::ZL, 5, 0x80),
];

/// What a made-up Switch report holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SwitchInput {
    /// board-api `btn` bits.
    pub buttons: u32,
    /// Sticks as a game reads them before the dead zone: -32767 (left,
    /// up) to +32767 (right, down), x then y.
    pub left: (i16, i16),
    pub right: (i16, i16),
    /// The motion sensor in the PAD's own axes and units (pad-view's
    /// notes: x away from the player, y to the left, z up; 4096 per g;
    /// 0.070 degrees a second per count). All zero accel: no motion (the
    /// sensor is off). [`console_motion_to_pad`] turns a game's numbers
    /// into these.
    pub accel: [i16; 3],
    pub gyro: [i16; 3],
    /// Battery level 0 (empty) to 4 (full); charging.
    pub battery: u8,
    pub charging: bool,
    /// The report counter (byte 1).
    pub timer: u8,
}

/// One stick axis (game value) as the pad's 12-bit number. `flip`: the
/// pad's y grows upwards.
fn twelve_bit(v: i16, flip: bool) -> u16 {
    let v = i32::from(v).clamp(-32767, 32767);
    let d = (v * HALF + if v >= 0 { 16383 } else { -16383 }) / 32767;
    let raw = if flip { MID - d } else { MID + d };
    raw.clamp(0, 4095) as u16
}

fn put_stick(out: &mut [u8], (x, y): (i16, i16)) {
    let x = twelve_bit(x, false);
    let y = twelve_bit(y, true);
    out[0] = (x & 0xFF) as u8;
    out[1] = ((x >> 8) as u8 & 0x0F) | (((y & 0x0F) as u8) << 4);
    out[2] = (y >> 4) as u8;
}

/// A Switch full report (0x30) for `s`.
pub fn switch_report(s: &SwitchInput) -> [u8; SWITCH_REPORT_LEN] {
    let mut r = [0_u8; SWITCH_REPORT_LEN];
    r[0] = 0x30;
    r[1] = s.timer;
    // Battery nibble: level * 2, bit 0 charging; low nibble 0x0E (as the
    // logged pad sends: pro controller, powered by its battery).
    r[2] = ((s.battery.min(4) << 1 | u8::from(s.charging)) << 4) | 0x0E;
    for (bit, at, mask) in BUTTONS {
        if s.buttons & bit != 0 {
            r[at] |= mask;
        }
    }
    put_stick(&mut r[6..9], s.left);
    put_stick(&mut r[9..12], s.right);
    r[12] = 0x70;
    let sample = [
        s.accel[0], s.accel[1], s.accel[2], s.gyro[0], s.gyro[1], s.gyro[2],
    ];
    for k in 0..3 {
        for (i, v) in sample.iter().enumerate() {
            let at = MOTION_AT + k * 12 + 2 * i;
            r[at..at + 2].copy_from_slice(&v.to_le_bytes());
        }
    }
    r
}

/// A game's motion numbers (board-api `Motion`: x right, y away, z up;
/// 4096 per g; 16 per degree a second) in the pad's axes and units, as
/// [`SwitchInput`] wants them: the inverse of [`crate::map::motion`].
/// The gyroscope comes back exactly for every number a real pad can
/// give (the pad's count times 1.12, cut toward zero: 9, 18, 27 ... are
/// never seen, and come back one nearer zero).
pub fn console_motion_to_pad(accel: [i16; 3], gyro: [i16; 3]) -> ([i16; 3], [i16; 3]) {
    let neg = |v: i16| v.checked_neg().unwrap_or(i16::MAX);
    // The smallest count (away from zero) whose console value reaches v:
    // count * 112 / 100 cut toward zero gives v back whenever any count
    // can (one count is 1.12, so at most one count lands on v).
    let unscale = |v: i16| {
        let m = i32::from(v).abs() * 100;
        let c = (m + 111) / 112;
        let c = if (c * 112 / 100) == i32::from(v).abs() {
            c
        } else {
            m / 112
        };
        (c * i32::from(v).signum()).clamp(i32::from(i16::MIN) + 1, i32::from(i16::MAX)) as i16
    };
    // console x = -pad y, console y = pad x, z the same.
    (
        [accel[1], neg(accel[0]), accel[2]],
        [unscale(gyro[1]), neg(unscale(gyro[0])), unscale(gyro[2])],
    )
}

/// The `n`-th report of a pad lying still that last sent `base`: the
/// counter goes up by 3 (as a Switch pad's does at its 15 ms rate), the
/// sticks wobble by up to 3 counts (well inside the dead zone) and the
/// motion numbers by up to 7, as in the owner's logs. Buttons, and so
/// what a game sees, stay as they were. Reports that are not Switch full
/// reports come back unchanged.
pub fn resting(base: &[u8], n: u32, out: &mut [u8]) -> usize {
    let len = base.len().min(out.len());
    out[..len].copy_from_slice(&base[..len]);
    if len < 12 || base[0] != 0x30 {
        return len;
    }
    out[1] = base[1].wrapping_add((n.wrapping_mul(3)) as u8);
    // A small wobble that changes every report: -3..=3, -7..=7.
    let wob = |k: u32, span: i32| -> i32 {
        let h = n.wrapping_mul(2_654_435_761).rotate_left(k * 7) ^ k.wrapping_mul(40_503);
        (h % (2 * span as u32 + 1)) as i32 - span
    };
    for (s, at) in [(0_u32, 6_usize), (1, 9)] {
        let x = i32::from(base[at]) | (i32::from(base[at + 1] & 0x0F) << 8);
        let y = i32::from(base[at + 1] >> 4) | (i32::from(base[at + 2]) << 4);
        let x = (x + wob(2 * s, 3)).clamp(0, 4095);
        let y = (y + wob(2 * s + 1, 3)).clamp(0, 4095);
        out[at] = (x & 0xFF) as u8;
        out[at + 1] = ((x >> 8) as u8 & 0x0F) | (((y & 0x0F) as u8) << 4);
        out[at + 2] = (y >> 4) as u8;
    }
    let mut at = MOTION_AT;
    let mut k = 4;
    while at + 2 <= len {
        let v = i16::from_le_bytes([base[at], base[at + 1]]);
        // A sensor that is off (all zero) stays off.
        if v != 0 {
            let w = (i32::from(v) + wob(k, 7)).clamp(-32767, 32767) as i16;
            out[at..at + 2].copy_from_slice(&w.to_le_bytes());
        }
        at += 2;
        k += 1;
    }
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sticks_round_trip_through_twelve_bits() {
        assert_eq!(twelve_bit(0, false), 2048);
        assert_eq!(twelve_bit(32767, false), 2048 + 1600);
        assert_eq!(twelve_bit(-32767, false), 2048 - 1600);
        assert_eq!(twelve_bit(32767, true), 2048 - 1600);
        assert_eq!(twelve_bit(i16::MIN, false), 2048 - 1600);
        let r = switch_report(&SwitchInput {
            left: (32767, -32767),
            ..Default::default()
        });
        let (x, y) = pad_view::decode::twelve(&r[6..9]);
        assert_eq!((x, y), (2048 + 1600, 2048 + 1600));
    }

    #[test]
    fn console_motion_comes_back() {
        let accel = [-200, 100, 4096];
        let gyro = [2240, 1120, 112];
        let (pa, pg) = console_motion_to_pad(accel, gyro);
        let mut m = pad_view::motion::Motion::default();
        m.samples[2] = [pa[0], pa[1], pa[2], pg[0], pg[1], pg[2]];
        let c = crate::map::motion(&m);
        assert_eq!(c.accel, accel);
        assert_eq!(c.gyro, gyro);
    }

    /// Every gyroscope number a real pad can give (count x 1.12, cut
    /// toward zero) comes back exactly; the board's PADTEST picture showed
    /// 31 -63 15 for 32 -64 16 before (the inverse cut toward zero too).
    #[test]
    fn every_reachable_gyro_number_comes_back() {
        let back = |v: i16| {
            let (_, pg) = console_motion_to_pad([0, 0, 4096], [0, 0, v]);
            let mut m = pad_view::motion::Motion::default();
            m.samples[2] = [0, 0, 4096, pg[0], pg[1], pg[2]];
            crate::map::motion(&m).gyro[2]
        };
        assert_eq!(back(16), 16);
        assert_eq!(back(-64), -64);
        assert_eq!(back(32), 32);
        let mut reachable = [false; 4001];
        for count in -3600_i32..=3600 {
            let v = count * 112 / 100;
            if (-2000..=2000).contains(&v) {
                reachable[(v + 2000) as usize] = true;
            }
        }
        for v in -2000_i16..=2000 {
            let b = back(v);
            if reachable[(i32::from(v) + 2000) as usize] {
                assert_eq!(b, v, "{v}");
            } else {
                assert!((i32::from(b) - i32::from(v)).abs() <= 1, "{v} -> {b}");
            }
        }
    }

    #[test]
    fn a_resting_report_changes_only_noise() {
        let base = switch_report(&SwitchInput {
            buttons: btn::SOUTH,
            accel: [10, -20, 4096],
            gyro: [3, -4, 5],
            timer: 7,
            ..Default::default()
        });
        let mut out = [0_u8; 64];
        let mut changed = 0;
        for n in 1..500 {
            let len = resting(&base, n, &mut out);
            assert_eq!(len, SWITCH_REPORT_LEN);
            assert_eq!(out[0], 0x30);
            assert_eq!(out[1], 7_u8.wrapping_add((n * 3) as u8));
            assert_eq!(out[2..6], base[2..6], "battery and buttons stay");
            let (x, y) = pad_view::decode::twelve(&out[6..9]);
            assert!((x - 2048).abs() <= 3 && (y - 2048).abs() <= 3);
            if out[13..] != base[13..] {
                changed += 1;
            }
        }
        assert!(changed > 400, "the motion moves: {changed}");
        // Not a Switch full report: as it was.
        let other = [0x01, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];
        assert_eq!(resting(&other, 9, &mut out), other.len());
        assert_eq!(out[..other.len()], other);
    }
}
