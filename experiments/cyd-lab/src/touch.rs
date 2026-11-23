//! The touch screen: an XPT2046 chip, driven by toggling pins by hand
//! (both SPI units are taken by the panel and the card; touch needs only
//! a few thousand bits a second).
//!
//! Copied from an earlier project of the author's on the same board,
//! where pins, control bytes, pressure limit and calibration were
//! measured on this board, in portrait. Pins: CLK 25, MOSI 32, MISO 39,
//! CS 33, and IRQ 36, which the chip pulls low while the panel is
//! pressed.
//!
//! UNPROVEN in landscape: the turn from that project's portrait numbers
//! to this console's landscape is worked out on paper (see `to_panel`),
//! not yet checked by a finger. Each touch writes one log line with the
//! raw values and the pixel it became, so the owner can check.

use board_api::TouchPoint;
use esp_hal::{
    delay::Delay,
    gpio::{Input, Level, Output},
};

const HALF_PERIOD_US: u32 = 2;

/// Pressure above which the panel counts as pressed (untouched reads
/// close to 0, a firm press in the thousands).
const PRESSURE_THRESHOLD: u16 = 300;

// Control bytes: start bit, channel, 12-bit, differential reference,
// power down between conversions (keeps the pen interrupt on).
const READ_X: u8 = 0xD0;
const READ_Y: u8 = 0x90;
const READ_Z1: u8 = 0xB0;
const READ_Z2: u8 = 0xC0;

/// One raw reading, in ADC counts (0..=4095).
#[derive(Clone, Copy, Debug, Default)]
pub struct RawTouch {
    pub x: u16,
    pub y: u16,
    pub z: u16,
}

/// The raw readings at the panel's edges, measured in that project on
/// 2026-11-10 in portrait (240 wide, 320 tall): raw x 3823 at the left
/// edge to 365 at the right, raw y 166 at the top to 3856 at the bottom.
const X_AT_PORTRAIT_LEFT: i32 = 3823;
const X_AT_PORTRAIT_RIGHT: i32 = 365;
const Y_AT_PORTRAIT_TOP: i32 = 166;
const Y_AT_PORTRAIT_BOTTOM: i32 = 3856;

fn along(raw: u16, start: i32, end: i32, size: i32) -> u16 {
    ((i32::from(raw) - start) * (size - 1) / (end - start)).clamp(0, size - 1) as u16
}

/// A raw reading as a landscape panel pixel (320x240), the console's
/// way round (MADCTL 40h).
///
/// Under MADCTL 40h, the panel's landscape x is the portrait y, and the
/// landscape y is the portrait x counted from the right (239 - x). So:
/// landscape x follows raw y (166 at x 0, 3856 at x 319), and landscape
/// y follows raw x (365 at y 0, 3823 at y 239).
pub fn to_panel(raw: RawTouch) -> TouchPoint {
    TouchPoint {
        x: along(raw.y, Y_AT_PORTRAIT_TOP, Y_AT_PORTRAIT_BOTTOM, 320),
        y: along(raw.x, X_AT_PORTRAIT_RIGHT, X_AT_PORTRAIT_LEFT, 240),
    }
}

pub struct Xpt2046 {
    clk: Output<'static>,
    mosi: Output<'static>,
    miso: Input<'static>,
    cs: Output<'static>,
    pub irq: Input<'static>,
    delay: Delay,
}

impl Xpt2046 {
    pub fn new(
        clk: Output<'static>,
        mosi: Output<'static>,
        miso: Input<'static>,
        cs: Output<'static>,
        irq: Input<'static>,
    ) -> Self {
        Self {
            clk,
            mosi,
            miso,
            cs,
            irq,
            delay: Delay::new(),
        }
    }

    /// Whether the chip says the panel is pressed (IRQ low).
    pub fn pressed(&self) -> bool {
        self.irq.is_low()
    }

    /// The four raw channels, whatever the pressure (for the start-up
    /// check): x, y, z1, z2.
    pub fn raw_all(&mut self) -> [u16; 4] {
        self.cs.set_low();
        let r = [
            self.convert(READ_X),
            self.convert(READ_Y),
            self.convert(READ_Z1),
            self.convert(READ_Z2),
        ];
        self.cs.set_high();
        r
    }

    /// A reading if the panel is pressed. About 1 ms when pressed, nothing
    /// when IRQ says it is not.
    pub fn read(&mut self) -> Option<RawTouch> {
        if !self.pressed() {
            return None;
        }
        self.cs.set_low();
        let reading = match self.pressure() {
            z if z >= PRESSURE_THRESHOLD => {
                // The first conversion after the panel drivers switch on
                // is taken before the voltage settles: thrown away.
                let _ = self.convert(READ_X);
                let x = (self.convert(READ_X) + self.convert(READ_X)) / 2;
                let _ = self.convert(READ_Y);
                let y = (self.convert(READ_Y) + self.convert(READ_Y)) / 2;
                // Only trusted if the finger is still down.
                (self.pressure() >= PRESSURE_THRESHOLD).then_some(RawTouch { x, y, z })
            }
            _ => None,
        };
        self.cs.set_high();
        reading
    }

    fn pressure(&mut self) -> u16 {
        let z1 = self.convert(READ_Z1);
        let z2 = self.convert(READ_Z2);
        (z1 + 4095).saturating_sub(z2)
    }

    /// Sends one control byte and clocks in the 12-bit result.
    fn convert(&mut self, control: u8) -> u16 {
        for bit in (0..8).rev() {
            let level = if (control >> bit) & 1 == 1 {
                Level::High
            } else {
                Level::Low
            };
            self.mosi.set_level(level);
            self.pulse();
        }
        self.mosi.set_low();
        // 16 clocks: one busy bit, 12 data bits (first the highest), 3
        // padding bits.
        let mut word = 0u16;
        for _ in 0..16 {
            self.delay.delay_micros(HALF_PERIOD_US);
            self.clk.set_high();
            word = (word << 1) | u16::from(self.miso.is_high());
            self.delay.delay_micros(HALF_PERIOD_US);
            self.clk.set_low();
        }
        (word >> 3) & 0x0FFF
    }

    fn pulse(&mut self) {
        self.delay.delay_micros(HALF_PERIOD_US);
        self.clk.set_high();
        self.delay.delay_micros(HALF_PERIOD_US);
        self.clk.set_low();
    }
}
