//! The strip bench (feature `wide-strips`, letter `b`): how fast the
//! panel is fed with small strip buffers against large ones.
//!
//! The strip buffers are where the CPU puts pixels while the DMA sends
//! the other one. `wide-strips` makes them 15,360 bytes each (as
//! display-dma had); the bench then also limits them to 5,120 (the
//! normal build's size) and sends the same areas both ways, from the
//! console's own picture of the moment:
//!
//! - full: the whole panel (320x240);
//! - small: one tile (72x72 panel pixels);
//! - row: the band of tiles a row scroll redraws (320x88).
//!
//! Each case is sent [`RUNS`] times untimed (no waiting for the
//! refresh: the pure cost of sending) and [`RUNS`] times with the
//! tear-free timing (what a frame really costs).

use board_api::{Rect, Screen};
use console_video::{Source, View};
use log::info;

use crate::app::App;
use crate::lcd::{CPU_MHZ, H, W, cycles};

const RUNS: u32 = 20;

pub fn strips(app: &mut App) {
    let screen = &mut app.board.screen;
    let video = app.console.video();
    let view = View::fit(W as u16, H as u16);
    let src = Source::new(video.frame(), video.palette(), view, 0);
    let cases = [
        ("full", Rect::new(0, 0, W as u16, H as u16)),
        ("small", Rect::new(28, 44, 72, 72)),
        ("row", Rect::new(0, 38, W as u16, 88)),
    ];
    let saved = (screen.strip_limit, screen.timed);
    for limit in [5_120usize, 15_360] {
        screen.strip_limit = limit.min(crate::lcd::STRIP_BYTES);
        for (name, rect) in cases {
            for timed in [false, true] {
                screen.timed = timed;
                let (mut sum, mut max, mut send_sum, mut fill_sum) = (0u64, 0u32, 0u64, 0u64);
                let before = screen.counters;
                for _ in 0..RUNS {
                    let t = cycles();
                    let r = screen.present(&src, &[rect]);
                    let us = cycles().wrapping_sub(t) / CPU_MHZ;
                    sum += u64::from(us);
                    max = max.max(us);
                    send_sum += u64::from(r.send_us);
                }
                let after = screen.counters;
                fill_sum += after.fill_cycles - before.fill_cycles;
                info!(
                    "bench=strips strip_bytes={} case={name} timed={timed} runs={RUNS} total_avg_us={} total_max_us={max} send_avg_us={} fill_avg_us={} late={} crossed={}",
                    screen.strip_limit,
                    sum / u64::from(RUNS),
                    send_sum / u64::from(RUNS),
                    fill_sum / u64::from(RUNS) / u64::from(CPU_MHZ),
                    after.late - before.late,
                    after.crossed - before.crossed,
                );
                crate::logbuf::flush(&mut app.uart);
            }
        }
    }
    (screen.strip_limit, screen.timed) = saved;
}
