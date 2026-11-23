//! `boardctl pad`: plays a gamepad for the CYD firmware's gamepad host
//! (feature `pad` with `link`), through the injected-pad frames of
//! `debug_link::pad`. The reports are Switch-mode full reports built by
//! `pad_host::inject`, the builder the host's own tests check, so the
//! board's real decoder, press latch, wake rule and motion mapping all
//! run as for the real pad. Nothing goes over the radio.
//!
//! A script has one step per line: the time in ms from the start, then
//!
//! ```text
//! 0     connect switch      # a pad connects (switch, xinput, android)
//! 100   press east          # hold buttons (names below), one report
//! 200   release east        # let go, one report
//! 300   tap home            # down and up in two reports back to back:
//!                           # a press shorter than any frame
//! 400   stick left 32767 0  # x, y: -32767..32767, y down (as the simulator)
//! 500   tilt 2048 -1024     # accelerometer x, y (4096 = one g); z follows
//! 600   spin 0 160 0        # gyroscope x, y, z (16 = one degree a second)
//! 700   battery 2           # 0..4
//! 800   repeat 10           # the last report again every 10 ms, with a
//!                           # resting pad's small changes (0: stop)
//! 900   off                 # the pad goes (switched off)
//! ```
//!
//! Names: south east west north l r zl zr minus plus home capture lstick
//! rstick up down left right (any case), as in the simulator's scripts.
//! Everything stays as the script left it until a later line changes it.
//! Before the first `tilt` the pad lies flat and still (z = one g), as the
//! simulator's.

use anyhow::{Context, Result, bail};
use board_api::btn;
use debug_link::pad::{PadRequest, mode};
use pad_host::inject::{self, SwitchInput};

/// Button names, by position (south = bottom), as boards/desktop's
/// scripts name them.
pub const BUTTONS: [(&str, u32); 18] = [
    ("south", btn::SOUTH),
    ("east", btn::EAST),
    ("west", btn::WEST),
    ("north", btn::NORTH),
    ("l", btn::L),
    ("r", btn::R),
    ("zl", btn::ZL),
    ("zr", btn::ZR),
    ("minus", btn::MINUS),
    ("plus", btn::PLUS),
    ("home", btn::HOME),
    ("capture", btn::CAPTURE),
    ("lstick", btn::LSTICK),
    ("rstick", btn::RSTICK),
    ("up", btn::UP),
    ("down", btn::DOWN),
    ("left", btn::LEFT),
    ("right", btn::RIGHT),
];

pub fn button(name: &str) -> Result<u32> {
    BUTTONS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|&(_, b)| b)
        .with_context(|| format!("{name:?} is not a button (names: south east west north l r zl zr minus plus home capture lstick rstick up down left right)"))
}

pub fn buttons(names: &[String]) -> Result<u32> {
    names.iter().try_fold(0, |acc, n| Ok(acc | button(n)?))
}

/// One step of a script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Connect(u8),
    Press(u32),
    Release(u32),
    Tap(u32),
    Stick { right: bool, x: i16, y: i16 },
    Tilt(i16, i16),
    Spin([i16; 3]),
    Battery(u8),
    Repeat(u16),
    Off,
}

/// What the pad holds now (the console's view of it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Held {
    pub buttons: u32,
    pub left: (i16, i16),
    pub right: (i16, i16),
    pub tilt: (i16, i16),
    pub spin: [i16; 3],
    pub battery: u8,
    pub timer: u8,
}

impl Default for Held {
    fn default() -> Self {
        Held { buttons: 0, left: (0, 0), right: (0, 0), tilt: (0, 0), spin: [0; 3], battery: 4, timer: 0 }
    }
}

impl Held {
    /// The Switch report for this state (the report counter goes up by 3
    /// each time, as a Switch pad's does).
    pub fn report(&mut self) -> [u8; inject::SWITCH_REPORT_LEN] {
        self.timer = self.timer.wrapping_add(3);
        let (x, y) = (i32::from(self.tilt.0), i32::from(self.tilt.1));
        let g = 4096_i32;
        let z = (g * g - x * x - y * y).max(0).isqrt() as i16;
        let (accel, gyro) = inject::console_motion_to_pad([self.tilt.0, self.tilt.1, z], self.spin);
        inject::switch_report(&SwitchInput {
            buttons: self.buttons,
            left: self.left,
            right: self.right,
            accel,
            gyro,
            battery: self.battery,
            charging: false,
            timer: self.timer,
        })
    }
}

fn num<T: std::str::FromStr>(word: Option<&str>, what: &str) -> Result<T> {
    let w = word.with_context(|| format!("{what} missing"))?;
    w.parse().map_err(|_| anyhow::anyhow!("{w:?} is not a number ({what})"))
}

/// Reads a script: (ms, step) in time order.
pub fn parse(text: &str) -> Result<Vec<(u64, Step)>> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let step = (|| -> Result<(u64, Step)> {
            let mut w = line.split_whitespace();
            let ms: u64 = num(w.next(), "time in ms")?;
            let verb = w.next().context("nothing after the time")?.to_ascii_lowercase();
            let rest: Vec<String> = w.map(str::to_owned).collect();
            let mut r = rest.iter().map(String::as_str);
            let step = match verb.as_str() {
                "connect" => {
                    let m = r.next().unwrap_or("switch");
                    Step::Connect(mode::from_name(m).with_context(|| format!("{m:?} is not a mode (switch, xinput, android)"))?)
                }
                "press" => Step::Press(buttons(&rest)?),
                "release" => Step::Release(buttons(&rest)?),
                "tap" => Step::Tap(buttons(&rest)?),
                "stick" => {
                    let side = r.next().context("stick: left or right")?;
                    let right = match side {
                        "left" => false,
                        "right" => true,
                        _ => bail!("stick: {side:?} is not left or right"),
                    };
                    Step::Stick { right, x: num(r.next(), "stick x")?, y: num(r.next(), "stick y")? }
                }
                "tilt" => Step::Tilt(num(r.next(), "tilt x")?, num(r.next(), "tilt y")?),
                "spin" => Step::Spin([num(r.next(), "spin x")?, num(r.next(), "spin y")?, num(r.next(), "spin z")?]),
                "battery" => {
                    let b: u8 = num(r.next(), "battery level")?;
                    if b > 4 {
                        bail!("battery: 0..4");
                    }
                    Step::Battery(b)
                }
                "repeat" => Step::Repeat(num(r.next(), "period in ms")?),
                "off" => Step::Off,
                _ => bail!("{verb:?}: not a step (connect press release tap stick tilt spin battery repeat off)"),
            };
            Ok((ms, step))
        })()
        .with_context(|| format!("line {}: {line:?}", n + 1))?;
        out.push(step);
    }
    if out.windows(2).any(|p| p[1].0 < p[0].0) {
        bail!("the times must not go back");
    }
    Ok(out)
}

/// The requests for one step, in order, from the state `held`.
pub fn requests(held: &mut Held, step: &Step) -> Vec<Request> {
    let report = |h: &mut Held| Request::Report(h.report().to_vec());
    match *step {
        Step::Connect(m) => vec![Request::Connect(m), report(held)],
        Step::Press(b) => {
            held.buttons |= b;
            vec![report(held)]
        }
        Step::Release(b) => {
            held.buttons &= !b;
            vec![report(held)]
        }
        Step::Tap(b) => {
            let was = held.buttons;
            held.buttons |= b;
            let down = report(held);
            held.buttons = was;
            vec![down, report(held)]
        }
        Step::Stick { right, x, y } => {
            if right {
                held.right = (x, y);
            } else {
                held.left = (x, y);
            }
            vec![report(held)]
        }
        Step::Tilt(x, y) => {
            held.tilt = (x, y);
            vec![report(held)]
        }
        Step::Spin(s) => {
            held.spin = s;
            vec![report(held)]
        }
        Step::Battery(b) => {
            held.battery = b;
            vec![report(held)]
        }
        Step::Repeat(ms) => vec![Request::Repeat(ms)],
        Step::Off => {
            *held = Held::default();
            vec![Request::Disconnect(0x13)]
        }
    }
}

/// An owned request (the wire type borrows its report).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Connect(u8),
    Report(Vec<u8>),
    Disconnect(u8),
    Repeat(u16),
}

impl Request {
    pub fn wire(&self) -> PadRequest<'_> {
        match self {
            Request::Connect(mode) => PadRequest::Connect { mode: *mode },
            Request::Report(bytes) => PadRequest::Report { bytes },
            Request::Disconnect(reason) => PadRequest::Disconnect { reason: *reason },
            Request::Repeat(period_ms) => PadRequest::Repeat { period_ms: *period_ms },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_reads() {
        let s = parse(
            "# a comment\n0 connect switch\n10 press east home\n20 release EAST\n30 tap home\n40 stick left 32767 -5\n50 tilt 2048 -1024 # x y\n60 spin 0 160 0\n70 battery 2\n80 repeat 10\n90 off\n",
        )
        .unwrap();
        assert_eq!(s.len(), 10);
        assert_eq!(s[0], (0, Step::Connect(mode::SWITCH)));
        assert_eq!(s[1], (10, Step::Press(btn::EAST | btn::HOME)));
        assert_eq!(s[4], (40, Step::Stick { right: false, x: 32767, y: -5 }));
        assert!(parse("10 press a\n").is_err(), "positions, not letters");
        assert!(parse("20 off\n10 off\n").is_err(), "time goes back");
        assert!(parse("0 connect dinput\n").is_err());
    }

    #[test]
    fn a_tap_is_two_reports_and_leaves_the_button_up() {
        let mut h = Held::default();
        let r = requests(&mut h, &Step::Tap(btn::HOME));
        assert_eq!(r.len(), 2);
        let (Request::Report(down), Request::Report(up)) = (&r[0], &r[1]) else { panic!("{r:?}") };
        assert_eq!(down[4] & 0x10, 0x10, "home is in the middle byte");
        assert_eq!(up[4] & 0x10, 0);
        assert_eq!(h.buttons, 0);
        assert_ne!(down[1], up[1], "the report counter moves");
    }

    #[test]
    fn lying_flat_has_one_g_down() {
        let mut h = Held::default();
        let r = h.report();
        let z = i16::from_le_bytes([r[17], r[18]]);
        assert_eq!(z, 4096);
        assert_eq!(i16::from_le_bytes([r[13], r[14]]), 0);
    }
}
