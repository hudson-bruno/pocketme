//! 6.: does the panel say which line it is refreshing (Get Scanline,
//! 45h), and does the frame rate setting (FRCTRL2, C6h) change it?
//!
//! Why it matters: a game that draws while the panel is reading the
//! same place in its memory shows a tear. Knowing the line being shown
//! lets the console send in step with it.
//!
//! The refresh period is found by reading the scanline over and over
//! and timing the moments it starts again from the top.

use log::{info, warn};

use crate::lcd::{CPU_MHZ, Lcd, READ_KHZ, cmd, cycles};

/// The scanline, as the panel answers 45h: two bytes after `dummy`
/// bits, ten bits used.
fn scanline(lcd: &mut Lcd, dummy: u8) -> Option<u16> {
    let mut out = [0u8; 2];
    lcd.raw(READ_KHZ, cmd::GSCAN, &[], dummy, &mut out).ok()?;
    Some(u16::from_be_bytes(out) & 0x3FF)
}

/// What sampling the scanline for a while showed.
struct Period {
    samples: u32,
    min_line: u16,
    max_line: u16,
    wraps: u32,
    /// Average time between two returns to the top, in microseconds.
    period_us: u32,
    /// Time of one scanline read, in microseconds.
    read_us: u32,
}

fn sample(lcd: &mut Lcd, dummy: u8, ms: u32) -> Period {
    let span = ms * 1000 * CPU_MHZ;
    let start = cycles();
    let (mut samples, mut min_line, mut max_line) = (0, u16::MAX, 0);
    let mut last = None;
    let (mut first_wrap, mut last_wrap, mut wraps) = (0u32, 0u32, 0u32);
    while cycles().wrapping_sub(start) < span {
        let Some(line) = scanline(lcd, dummy) else {
            continue;
        };
        let now = cycles().wrapping_sub(start);
        samples += 1;
        min_line = min_line.min(line);
        max_line = max_line.max(line);
        // Back at the top: the line number fell by a lot.
        if let Some(previous) = last
            && line + 100 < previous
        {
            if wraps == 0 {
                first_wrap = now;
            }
            last_wrap = now;
            wraps += 1;
        }
        last = Some(line);
    }
    let elapsed = cycles().wrapping_sub(start);
    let period_us = if wraps > 1 {
        (last_wrap - first_wrap) / (wraps - 1) / CPU_MHZ
    } else {
        0
    };
    let read_us = if samples > 0 {
        elapsed / samples / CPU_MHZ
    } else {
        0
    };
    Period {
        samples,
        min_line,
        max_line,
        wraps,
        period_us,
        read_us,
    }
}

pub fn run(lcd: &mut Lcd) {
    // Which number of dummy bits gives a sensible answer: print the raw
    // bytes for each.
    for dummy in [0u8, 1, 8] {
        let mut raw = [0u8; 3];
        let mut lines = [0u16; 4];
        for line in &mut lines {
            *line = scanline(lcd, dummy).unwrap_or(0xFFFF);
        }
        let ok = lcd.raw(READ_KHZ, cmd::GSCAN, &[], dummy, &mut raw).is_ok();
        info!("scan: GSCAN dummy_bits={dummy} ok={ok} raw={raw:02x?} four_reads={lines:?}");
    }

    // Which dummy setting is right: with the wrong one every number is
    // doubled or halved, so the range of lines seen over a few refreshes
    // tells (the panel has 320 lines plus porches).
    for dummy in [0u8, 1] {
        let p = sample(lcd, dummy, 100);
        info!(
            "scan: range dummy_bits={dummy} samples={} line_min={} line_max={} wraps={} period_us={}",
            p.samples, p.min_line, p.max_line, p.wraps, p.period_us
        );
    }

    // The frame rate settings to try: 0Fh is the power-on default
    // (60 Hz in the datasheet's table), 00h the fastest (119 Hz), 1Eh/1Fh
    // the slowest (40/39 Hz).
    for (setting, datasheet_hz) in [
        (0x0Fu8, 60u32),
        (0x00, 119),
        (0x1E, 40),
        (0x1F, 39),
        (0x0F, 60),
    ] {
        if lcd.command(cmd::FRCTRL2, &[setting]).is_err() {
            warn!("scan: FRCTRL2 {setting:02x}h could not be sent");
            continue;
        }
        let p = sample(lcd, 1, 250);
        info!(
            "scan: frctrl2=0x{setting:02x} datasheet_hz={datasheet_hz} samples={} read_us={} line_min={} line_max={} wraps={} period_us={} measured_hz={}",
            p.samples,
            p.read_us,
            p.min_line,
            p.max_line,
            p.wraps,
            p.period_us,
            if p.period_us > 0 {
                1_000_000 / p.period_us
            } else {
                0
            },
        );
    }
}

/// The panel's identity and settings, read back: proof that reading
/// works at all, and what COLMOD and MADCTL hold now.
pub fn report(lcd: &mut Lcd) -> [u8; 3] {
    let mut id = [0u8; 3];
    let mut status = [0u8; 4];
    let mut one = [0u8; 1];
    match lcd.raw(READ_KHZ, cmd::RDDID, &[], 1, &mut id) {
        Ok(()) => info!("panel: RDDID (04h) -> {id:02x?}"),
        Err(error) => warn!("panel: RDDID failed: {error:?}"),
    }
    match lcd.raw(READ_KHZ, cmd::RDDST, &[], 1, &mut status) {
        Ok(()) => info!("panel: RDDST (09h) -> {status:02x?}"),
        Err(error) => warn!("panel: RDDST failed: {error:?}"),
    }
    for (name, command) in [
        ("RDDPM (0Ah)", cmd::RDDPM),
        ("RDDMADCTL (0Bh)", cmd::RDDMADCTL),
        ("RDDCOLMOD (0Ch)", cmd::RDDCOLMOD),
    ] {
        match lcd.raw(READ_KHZ, command, &[], 0, &mut one) {
            Ok(()) => info!("panel: {name} -> {one:02x?}"),
            Err(error) => warn!("panel: {name} failed: {error:?}"),
        }
    }
    id
}
