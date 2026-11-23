//! The motion sensor of a Switch-style pad: an accelerometer (it feels
//! gravity and shaking) and a gyroscope (it feels turning).
//!
//! Only the Switch layout carries motion here. In report 0x30 (and 0x31)
//! bytes 13 to 48 hold three samples of 12 bytes each. One sample is six
//! signed 16-bit numbers, low byte first: accelerometer x, y, z, then
//! gyroscope x, y, z. The pad measures every 5 ms and sends the three
//! newest measurements in each report, so one report covers 15 ms.
//! Sources (read to learn the facts; no code copied): dekuNukem's
//! Nintendo_Switch_Reverse_Engineering notes (bluetooth_hid_notes.md,
//! imu_sensor_notes.md) and SDL's SDL_hidapi_switch.c. See MODES.md for
//! every number and where it comes from.
//!
//! The axes, for a Pro Controller seen from above, buttons up
//! (from the Linux driver's notes and SDL's axis swap; not yet checked on
//! the GuliKit):
//!
//! * x points away from you, towards the triggers;
//! * y points to the left;
//! * z points up, out of the buttons.
//!
//! Lying still and flat, the accelerometer reads about +4096 on z (one g,
//! the pull of the Earth, seen as a push upwards) and about 0 on x and y.
//!
//! Nothing here uses the heap or panics, and there is no `libm`: the two
//! bits of trigonometry the tilt needs are done with a short formula.

/// Accelerometer counts per g at the default range of +-8 g: 4096.
/// SDL uses 4096 (SWITCH_ACCEL_SCALE) when the pad's own calibration is
/// not read; dekuNukem gives 0.000244 g per count (the same, 1/4096).
pub const ACCEL_COUNTS_PER_G: f32 = 4096.0;
/// Gyroscope degrees per second per count at the default range of +-2000
/// degrees per second: about 0.070. dekuNukem recommends 0.070 (the
/// sensor maker's figure); SDL uses 1/14.2842 = 0.0700; the Linux driver
/// 1/14.247 = 0.0702.
pub const GYRO_DPS_PER_COUNT: f32 = 0.070;
/// Samples in one report.
pub const SAMPLES: usize = 3;
/// Time between two samples, in milliseconds.
pub const SAMPLE_MS: u32 = 5;
/// The first byte of the first sample in report 0x30 (the report id is
/// byte 0).
pub const FIRST_BYTE: usize = 13;
/// Bytes per sample: six 16-bit numbers.
pub const SAMPLE_BYTES: usize = 12;

/// The three motion samples of one report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Motion {
    /// In the order they came (bytes 13-24, 25-36, 37-48). Each one:
    /// accelerometer x, y, z, then gyroscope x, y, z, raw counts.
    pub samples: [[i16; 6]; SAMPLES],
}

/// The tilt of the pad, from the accelerometer alone (where gravity
/// points). It is right while the pad is held still; a quick shake
/// disturbs it. Turning flat on a table (yaw) cannot be seen this way.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tilt {
    /// Turning around the forward axis: 0 flat, +90 lying on its right
    /// side (right grip down), -90 on its left side. Up to +-180 when it
    /// lies face down.
    pub roll_deg: f32,
    /// Nose up or down: 0 flat, +90 standing with the triggers up, -90
    /// with the triggers down.
    pub pitch_deg: f32,
    /// True when the buttons face the floor.
    pub face_down: bool,
}

impl Tilt {
    /// True when the pad lies flat, face up, within `deg` degrees.
    pub fn is_level(&self, deg: f32) -> bool {
        !self.face_down && abs(self.roll_deg) <= deg && abs(self.pitch_deg) <= deg
    }
}

impl Motion {
    /// Read the motion samples of a Switch full report (report id at
    /// index 0). None when the report is too short to hold one sample,
    /// or when every accelerometer number is zero: a real accelerometer
    /// always feels gravity, so all zeros means the sensor is off (or the
    /// pad does not have one), not a pad lying level. SDL decides the same
    /// way. A report cut short keeps its last whole sample in the missing
    /// places.
    pub fn from_switch_report(r: &[u8]) -> Option<Motion> {
        let mut m = Motion::default();
        let mut got = 0;
        for (k, sample) in m.samples.iter_mut().enumerate() {
            let at = FIRST_BYTE + k * SAMPLE_BYTES;
            let Some(b) = r.get(at..at + SAMPLE_BYTES) else {
                break;
            };
            for (i, v) in sample.iter_mut().enumerate() {
                *v = i16::from_le_bytes([b[2 * i], b[2 * i + 1]]);
            }
            got += 1;
        }
        if got == 0 {
            return None;
        }
        for k in got..SAMPLES {
            m.samples[k] = m.samples[got - 1];
        }
        let any_accel = m
            .samples
            .iter()
            .any(|s| s[0] != 0 || s[1] != 0 || s[2] != 0);
        any_accel.then_some(m)
    }

    /// The newest sample, raw. Which of the three is newest is not certain:
    /// dekuNukem's timing example and the Linux driver take the third
    /// sample as the latest, SDL the first. This takes the third. It
    /// matters little on screen: they are 5 ms apart.
    pub fn newest(&self) -> [i16; 6] {
        self.samples[SAMPLES - 1]
    }

    /// The newest accelerometer sample in g (1 g = the pull of gravity).
    pub fn accel_g(&self) -> [f32; 3] {
        let s = self.newest();
        [
            s[0] as f32 / ACCEL_COUNTS_PER_G,
            s[1] as f32 / ACCEL_COUNTS_PER_G,
            s[2] as f32 / ACCEL_COUNTS_PER_G,
        ]
    }

    /// The newest gyroscope sample in degrees per second. The pad's own
    /// zero offset (stored in its memory) is not read, so a pad lying still
    /// may show a few degrees per second.
    pub fn gyro_dps(&self) -> [f32; 3] {
        let s = self.newest();
        [
            s[3] as f32 * GYRO_DPS_PER_COUNT,
            s[4] as f32 * GYRO_DPS_PER_COUNT,
            s[5] as f32 * GYRO_DPS_PER_COUNT,
        ]
    }

    /// The accelerometer averaged over the three samples (raw counts): a
    /// little less noise for the tilt.
    pub fn accel_mean(&self) -> [i32; 3] {
        let mut sum = [0i32; 3];
        for s in &self.samples {
            for (a, v) in sum.iter_mut().zip(&s[..3]) {
                *a += *v as i32;
            }
        }
        [
            sum[0] / SAMPLES as i32,
            sum[1] / SAMPLES as i32,
            sum[2] / SAMPLES as i32,
        ]
    }

    /// Roll and pitch from gravity (the averaged accelerometer). No sensor
    /// fusion: the gyroscope is not used.
    pub fn tilt(&self) -> Tilt {
        let [x, y, z] = self.accel_mean();
        let yz = isqrt((y as i64 * y as i64 + z as i64 * z as i64) as u64) as f32;
        Tilt {
            roll_deg: atan2_deg(y as f32, z as f32),
            pitch_deg: atan2_deg(x as f32, yz),
            face_down: z < 0,
        }
    }

    /// Where gravity pulls, as seen from above, scaled so that a pad on
    /// its side or on its nose gives `reach`: (right, towards you). A ball
    /// on the pad would roll that way. (0, 0) when flat.
    pub fn downhill(&self, reach: i32) -> (i32, i32) {
        let [x, y, z] = self.accel_mean();
        let (x, y, z) = (x as i64, y as i64, z as i64);
        let len = isqrt((x * x + y * y + z * z) as u64).max(1) as i64;
        // y points left, so a lower right side reads +y (gravity seen as a
        // push upwards): the ball goes right. A raised nose reads +x: the
        // ball comes towards you, which is down on the screen.
        let r = reach as i64;
        ((y * r / len) as i32, (x * r / len) as i32)
    }
}

/// How the screen limits redraws of the motion values: at most one change
/// every [`Gate::MIN_GAP_MS`] (or the gap given to [`Gate::with_gap`]),
/// and only when a value moved by more than a small step. A pad lying
/// still (only sensor noise) is then not redrawn at all, and a pad in the
/// hand at most 10 times a second.
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    shown: Option<Motion>,
    at_ms: Option<u32>,
    gap_ms: u32,
    /// How many times the shown values changed.
    pub changes: u32,
}

impl Default for Gate {
    fn default() -> Self {
        Self::new()
    }
}

impl Gate {
    /// Shortest time between two changes on screen.
    pub const MIN_GAP_MS: u32 = 100;
    /// Smallest accelerometer move that counts: 82 counts, 0.02 g (about
    /// one degree of tilt). Noise of a still pad is a few counts.
    pub const ACCEL_STEP: i32 = 82;
    /// Smallest gyroscope move that counts: 29 counts, about 2 degrees a
    /// second.
    pub const GYRO_STEP: i32 = 29;

    /// A gate with the shortest gap, [`Gate::MIN_GAP_MS`].
    pub const fn new() -> Self {
        Self::with_gap(Self::MIN_GAP_MS)
    }

    /// A gate that lets a change through at most every `gap_ms`.
    pub const fn with_gap(gap_ms: u32) -> Self {
        Gate {
            shown: None,
            at_ms: None,
            gap_ms,
            changes: 0,
        }
    }

    /// What the screen shows now.
    pub fn shown(&self) -> Option<&Motion> {
        self.shown.as_ref()
    }

    /// Offer the newest motion (`None`: no motion data). Returns true when
    /// the values to show changed. A move counts only past the steps
    /// above. Every change, also data appearing or going away, waits for
    /// the gap after the change before; only the very first one shows at
    /// once. So even a pad whose reports flip between motion data and
    /// zeros is redrawn at most 10 times a second. Without a time
    /// (`now_ms` None) only the steps apply; the firmware's draw timer then
    /// sets the pace.
    pub fn offer(&mut self, now_ms: Option<u32>, latest: Option<&Motion>) -> bool {
        match (self.shown.as_ref(), latest) {
            (None, None) => return false,
            (Some(old), Some(new)) if !moved(old, new) => return false,
            _ => {}
        }
        if let (Some(now), Some(at)) = (now_ms, self.at_ms)
            && now.wrapping_sub(at) < self.gap_ms
        {
            return false;
        }
        self.shown = latest.copied();
        self.at_ms = now_ms;
        self.changes = self.changes.wrapping_add(1);
        true
    }
}

fn moved(a: &Motion, b: &Motion) -> bool {
    let (x, y) = (a.newest(), b.newest());
    let step = |i: usize| {
        if i < 3 {
            Gate::ACCEL_STEP
        } else {
            Gate::GYRO_STEP
        }
    };
    (0..6).any(|i| (x[i] as i32 - y[i] as i32).abs() > step(i))
        || a.accel_mean()
            .iter()
            .zip(b.accel_mean())
            .any(|(p, q)| (p - q).abs() > Gate::ACCEL_STEP)
}

fn abs(v: f32) -> f32 {
    if v < 0.0 { -v } else { v }
}

/// Whole-number square root.
pub(crate) fn isqrt(v: u64) -> u64 {
    if v < 2 {
        return v;
    }
    let mut x = v;
    let mut y = x / 2 + x % 2;
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}

/// atan of z (from -1 to 1) in degrees, by a short formula that is within
/// about 0.25 degrees.
fn atan_deg(z: f32) -> f32 {
    45.0 * z + 15.64 * z * (1.0 - abs(z))
}

/// The angle of the point (x, y) seen from (0, 0), in degrees from -180
/// to 180 (0 along x, +90 along y).
pub fn atan2_deg(y: f32, x: f32) -> f32 {
    let (ax, ay) = (abs(x), abs(y));
    if ax == 0.0 && ay == 0.0 {
        return 0.0;
    }
    if ax >= ay {
        let a = atan_deg(y / x);
        if x > 0.0 {
            a
        } else if y >= 0.0 {
            a + 180.0
        } else {
            a - 180.0
        }
    } else {
        let a = atan_deg(x / y);
        if y > 0.0 { 90.0 - a } else { -90.0 - a }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atan2_close_enough() {
        for (y, x, want) in [
            (0.0, 1.0, 0.0),
            (1.0, 1.0, 45.0),
            (1.0, 0.0, 90.0),
            (1.0, -1.0, 135.0),
            (0.0, -1.0, 180.0),
            (-1.0, -1.0, -135.0),
            (-1.0, 0.0, -90.0),
            (-1.0, 1.0, -45.0),
            (0.5, 1.0, 26.565),
            (1.0, 0.2, 78.69),
        ] {
            let got = atan2_deg(y, x);
            assert!(
                (got - want).abs() < 0.3,
                "atan2({y}, {x}) = {got}, want {want}"
            );
        }
        assert_eq!(atan2_deg(0.0, 0.0), 0.0);
    }

    #[test]
    fn square_roots() {
        for v in [0u64, 1, 2, 3, 4, 15, 16, 17, 4096 * 4096, 3 * 32768 * 32768] {
            let r = isqrt(v);
            assert!(r * r <= v && (r + 1) * (r + 1) > v, "{v}");
        }
    }
}
