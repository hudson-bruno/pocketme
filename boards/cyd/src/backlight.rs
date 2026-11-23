//! The backlight (GPIO21), dimmed by the chip's LED PWM unit (LEDC).
//!
//! - Low-speed timer 0, from the 80 MHz APB clock, 12-bit duty: the
//!   divider is exactly 1.0, so the light switches at 19,531 Hz (80 MHz /
//!   4096). That is far above flicker for the eye, and not a whole
//!   multiple or divisor of the panel's refresh (about 58.8 Hz) or its
//!   line rate (about 20.2 kHz): no steady pattern against the panel.
//! - 4096 duty steps: the console's dimmest value, 16, is 257 steps
//!   (6.3 %), so even the dimmest light is many steps wide.
//! - Value 0 is duty 0 (the pin stays low: off), 255 is duty 4096 (the
//!   pin stays high: on).
//! - Until the console's first call, the duty is 0: the light stays off
//!   until there is a picture (board-api `Screen::backlight`).
//! - Nothing is stored here: after every restart the console asks
//!   again (for the saved brightness with `store`, else for full).
//!
//! Clashes: nothing else on this board uses LEDC (the radio, esp-rtos
//! and the embassy timer use the timer groups, not LEDC).
//!
//! [`Backlight::check`] (the letter `l`) checks it from the chip's side:
//! for each of the console's five values (and 0), the duty the LEDC
//! unit reports back, and the pin itself sampled (share of the time high,
//! and switching rate).

use esp_hal::{
    gpio::{
        DriveMode,
        interconnect::{InputSignal, OutputSignal},
    },
    ledc::{
        LSGlobalClkSource, Ledc, LowSpeed,
        channel::{self, ChannelHW, ChannelIFace},
        timer::{self, TimerIFace},
    },
    peripherals::LEDC,
    time::Rate,
};
use log::info;
use static_cell::StaticCell;

/// The switching rate asked of the timer (80 MHz / 4096, which makes the
/// divider exactly 1.0; one hertz more would need a divider under 1).
pub const PWM_HZ: u32 = 19_531;
/// Duty steps (12 bits).
pub const STEPS: u32 = 4096;

static TIMER: StaticCell<timer::Timer<'static, LowSpeed>> = StaticCell::new();

pub struct Backlight {
    _ledc: Ledc<'static>,
    channel: channel::Channel<'static, LowSpeed>,
    /// The pin, read back while the LEDC drives it.
    pin_in: InputSignal<'static>,
    /// The last value set (0..=255).
    level: u8,
    /// Values set since start.
    pub calls: u32,
}

/// The LEDC duty for a value (0 off .. 255 on).
pub fn duty_for(level: u8) -> u32 {
    u32::from(level) * STEPS / 255
}

impl Backlight {
    /// Takes the pin (as its two halves: the output for the LEDC, the
    /// input to read it back). The light is off when this returns.
    pub fn new(
        ledc: LEDC<'static>,
        pin_out: OutputSignal<'static>,
        pin_in: InputSignal<'static>,
    ) -> Self {
        let mut ledc = Ledc::new(ledc);
        ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);
        let timer = TIMER.init(ledc.timer::<LowSpeed>(timer::Number::Timer0));
        if let Err(e) = timer.configure(timer::config::Config {
            duty: timer::config::Duty::Duty12Bit,
            clock_source: timer::LSClockSource::APBClk,
            frequency: Rate::from_hz(PWM_HZ),
        }) {
            log::warn!("backlight: timer set-up failed: {e:?}");
        }
        let mut channel = ledc.channel(channel::Number::Channel0, pin_out);
        if let Err(e) = channel.configure(channel::config::Config {
            timer: timer as &'static dyn TimerIFace<LowSpeed>,
            duty_pct: 0,
            drive_mode: DriveMode::PushPull,
        }) {
            log::warn!("backlight: channel set-up failed: {e:?}");
        }
        channel.set_duty_hw(0);
        Self {
            _ledc: ledc,
            channel,
            pin_in,
            level: 0,
            calls: 0,
        }
    }

    /// 0 (off) to 255 (full).
    pub fn set(&mut self, level: u8) {
        self.channel.set_duty_hw(duty_for(level));
        self.level = level;
        self.calls += 1;
    }

    #[cfg_attr(not(feature = "link"), allow(dead_code))]
    pub fn level(&self) -> u8 {
        self.level
    }

    /// Measuring only (letter `l`): for 0 and each of the console's
    /// values, sets it, then reads the duty back from the LEDC unit and
    /// samples the pin. Puts the last value back at the end. Takes about
    /// 0.2 s (the panel goes through the five levels).
    pub fn check(&mut self) {
        let keep = self.level;
        let (div, bits) = timer_conf();
        info!(
            "light check: timer0 div={}.{:03} bits={} -> {} Hz; ledc_conf={:#x}",
            div >> 8,
            (div & 0xFF) * 1000 / 256,
            bits,
            ((80_000_000u64 * 256 / u64::from(div.max(1))) >> bits),
            ledc_reg(0x190)
        );
        let mut values = [0u8; 6];
        values[1..].copy_from_slice(&console_core::theme::BACKLIGHT);
        for v in values {
            self.set(v);
            // Let the new duty start (it applies from the next period).
            esp_hal::delay::Delay::new().delay_micros(200);
            let duty_r = ledc_reg(0xB0) >> 4;
            let s = self.sample(200_000);
            info!(
                "light {v}: duty set={} read_back={} of {STEPS} ({}.{}%), pin high {}.{}% of {} samples, {} rises in {} us = {} Hz",
                duty_for(v),
                duty_r,
                duty_r * 100 / STEPS,
                duty_r * 1000 / STEPS % 10,
                s.high_permille / 10,
                s.high_permille % 10,
                s.samples,
                s.rises,
                s.us,
                u64::from(s.rises) * 1_000_000 / u64::from(s.us.max(1)),
            );
        }
        self.set(keep);
        self.calls -= values.len() as u32 + 1;
    }

    fn sample(&self, n: u32) -> Sampled {
        let t0 = xtensa_lx::timer::get_cycle_count();
        let mut high = 0u32;
        let mut rises = 0u32;
        let mut was = self.pin_in.is_input_high();
        for _ in 0..n {
            let now = self.pin_in.is_input_high();
            high += u32::from(now);
            rises += u32::from(now && !was);
            was = now;
        }
        let us = xtensa_lx::timer::get_cycle_count().wrapping_sub(t0) / crate::lcd::CPU_MHZ;
        Sampled {
            samples: n,
            high_permille: (u64::from(high) * 1000 / u64::from(n)) as u32,
            rises,
            us,
        }
    }
}

struct Sampled {
    samples: u32,
    high_permille: u32,
    rises: u32,
    us: u32,
}

/// The LEDC unit's registers (ESP32 TRM, LED PWM): low-speed channel 0
/// at 0xA0 (its DUTY_R at 0xB0, the duty in 1/16 steps), low-speed timer
/// 0's CONF at 0x160, the unit's CONF at 0x190. Read only.
const LEDC_BASE: usize = 0x3FF5_9000;

fn ledc_reg(offset: usize) -> u32 {
    // SAFETY: a read of a register of the LEDC unit, which this module
    // owns; reading has no side effects.
    unsafe { core::ptr::read_volatile((LEDC_BASE + offset) as *const u32) }
}

/// Low-speed timer 0: its divider (fixed point, 8 fraction bits) and
/// duty bits, as the unit has them.
fn timer_conf() -> (u32, u32) {
    let conf = ledc_reg(0x160);
    ((conf >> 5) & 0x3_FFFF, conf & 0x1F)
}
