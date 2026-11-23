//! The measurements. Every result is printed as one line of
//! `key=value` pairs starting with `bench=`, so a log can be searched
//! and compared by program. Times are from the CPU's cycle counter
//! (240 per microsecond).

use log::{info, warn};

use crate::{
    dirty,
    lcd::{self, CPU_MHZ, Depth, H, Lcd, Send, Timing, W, cycles},
    pipeline::{self, Frame, LH, LW, Palette, Rect},
};

fn us(c: u64) -> u64 {
    c / u64::from(CPU_MHZ)
}

/// Minimum, sum and maximum of repeated runs, plus the split of the
/// last-but-averaged parts.
#[derive(Default)]
struct Runs {
    n: u64,
    min: u64,
    max: u64,
    total: u64,
    fill: u64,
    wait: u64,
    window: u64,
    bytes: u64,
    transfers: u64,
}

impl Runs {
    fn add(&mut self, t: &Timing) {
        let total = u64::from(t.total);
        self.min = if self.n == 0 {
            total
        } else {
            self.min.min(total)
        };
        self.max = self.max.max(total);
        self.n += 1;
        self.total += total;
        self.fill += u64::from(t.fill);
        self.wait += u64::from(t.wait);
        self.window += u64::from(t.window);
        self.bytes += u64::from(t.bytes);
        self.transfers += u64::from(t.transfers);
    }

    fn avg(&self, v: u64) -> u64 {
        if self.n == 0 { 0 } else { v / self.n }
    }
}

fn send_name(how: Send) -> &'static str {
    match how {
        Send::Overlap => "dma_overlap",
        Send::DmaInTurn => "dma_in_turn",
        Send::Fifo => "fifo_no_dma",
    }
}

/// Wire time of `bytes` at `mhz`, in microseconds.
fn floor_us(bytes: u64, mhz: u32) -> u64 {
    bytes * 8 / u64::from(mhz)
}

/// 1. and 2.: a full frame through the pipeline, in several ways.
pub fn full_frames(lcd: &mut Lcd, fb: &Frame, pal: &Palette, label: &str) {
    const REPS: usize = 10;
    let ways = [
        (Send::Overlap, 4),
        (Send::Overlap, 8),
        (Send::Overlap, 12),
        (Send::DmaInTurn, 8),
        (Send::Fifo, 8),
    ];
    for mhz in [40, 80] {
        let _ = lcd.set_write_mhz(mhz);
        for depth in [Depth::Rgb565, Depth::Rgb444] {
            let _ = lcd.set_depth(depth);
            for (how, strip_rows) in ways {
                let mut runs = Runs::default();
                for _ in 0..REPS {
                    match pipeline::present_full(lcd, fb, pal, strip_rows, how) {
                        Ok(t) => runs.add(&t),
                        Err(e) => warn!("bench: full frame failed: {e:?}"),
                    }
                }
                let total = us(runs.avg(runs.total));
                let (fill, wait, window) = (
                    us(runs.avg(runs.fill)),
                    us(runs.avg(runs.wait)),
                    us(runs.avg(runs.window)),
                );
                let bytes = runs.avg(runs.bytes);
                info!(
                    "bench=full{label} mhz={mhz} depth={} send={} strip_rows={strip_rows} reps={} total_us_avg={total} total_us_min={} total_us_max={} wait_us={wait} fill_us={fill} window_us={window} other_us={} bytes={bytes} transfers={} wire_floor_us={} fps={}",
                    depth.bits(),
                    send_name(how),
                    runs.n,
                    us(runs.min),
                    us(runs.max),
                    total.saturating_sub(fill + wait + window),
                    runs.avg(runs.transfers),
                    floor_us(bytes, mhz),
                    if total > 0 { 1_000_000 / total } else { 0 },
                );
            }
        }
    }
    let _ = lcd.set_depth(Depth::Rgb565);
    let _ = lcd.set_write_mhz(40);
}

/// 3.: small rectangles, and the window commands alone.
pub fn rects(lcd: &mut Lcd, fb: &Frame, pal: &Palette, label: &str) {
    const REPS: usize = 100;
    let one = Rect {
        x: 37,
        y: 53,
        w: 16,
        h: 16,
    };
    for mhz in [40, 80] {
        let _ = lcd.set_write_mhz(mhz);
        for depth in [Depth::Rgb565, Depth::Rgb444] {
            let _ = lcd.set_depth(depth);
            // The window command set alone: CASET, RASET, RAMWR, no pixels.
            let mut win = 0u64;
            for _ in 0..REPS {
                let t = cycles();
                let _ = lcd.begin(74, 106, 74 + 31, 106 + 31);
                lcd.end();
                win += u64::from(cycles().wrapping_sub(t));
            }
            let mut runs = Runs::default();
            for _ in 0..REPS {
                match pipeline::present_rect(lcd, fb, pal, one, 8, Send::Overlap) {
                    Ok(t) => runs.add(&t),
                    Err(e) => warn!("bench: rect failed: {e:?}"),
                }
            }
            let bytes = runs.avg(runs.bytes);
            let total = us(runs.avg(runs.total));
            info!(
                "bench=rect{label} mhz={mhz} depth={} rect=16x16_logical sent_w={} reps={} total_us_avg={total} total_us_min={} total_us_max={} window_cmds_only_us={}.{:02} wait_us={} fill_us={} bytes={bytes} wire_floor_us={} overhead_us={}",
                depth.bits(),
                if depth == Depth::Rgb444 {
                    one.to_quads().w
                } else {
                    one.w
                },
                runs.n,
                us(runs.min),
                us(runs.max),
                us(win / REPS as u64),
                (win / REPS as u64 * 100 / u64::from(CPU_MHZ)) % 100,
                us(runs.avg(runs.wait)),
                us(runs.avg(runs.fill)),
                floor_us(bytes, mhz),
                total.saturating_sub(floor_us(bytes, mhz)),
            );
            // 16 such rectangles in one frame, a 4x4 grid at odd places.
            let mut frames = Runs::default();
            for _ in 0..10 {
                let mut sum = Timing::default();
                let start = cycles();
                for k in 0..16u16 {
                    let r = Rect {
                        x: 3 + (k % 4) * 39,
                        y: 5 + (k / 4) * 28,
                        w: 16,
                        h: 16,
                    };
                    match pipeline::present_rect(lcd, fb, pal, r, 8, Send::Overlap) {
                        Ok(t) => sum.add(&t),
                        Err(e) => warn!("bench: rect failed: {e:?}"),
                    }
                }
                sum.total = cycles().wrapping_sub(start);
                frames.add(&sum);
            }
            let bytes = frames.avg(frames.bytes);
            let total = us(frames.avg(frames.total));
            info!(
                "bench=rects16{label} mhz={mhz} depth={} reps={} total_us_avg={total} total_us_min={} total_us_max={} bytes={bytes} wire_floor_us={} overhead_us={} per_rect_overhead_us={}",
                depth.bits(),
                frames.n,
                us(frames.min),
                us(frames.max),
                floor_us(bytes, mhz),
                total.saturating_sub(floor_us(bytes, mhz)),
                total.saturating_sub(floor_us(bytes, mhz)) / 16,
            );
        }
    }
    let _ = lcd.set_depth(Depth::Rgb565);
    let _ = lcd.set_write_mhz(40);
}

/// 4.: the CPU side alone: comparing frames, and converting a frame.
pub fn cpu(lcd: &mut Lcd, fb: &Frame, other: &mut Frame, pal: &Palette, label: &str) {
    const REPS: u32 = 20;
    let mut rects = [Rect::default(); dirty::MAX_RECTS];

    // Identical frames: every word of every row is compared.
    other.0.copy_from_slice(&fb.0);
    let (mut n, mut best, mut sum) = (0, u32::MAX, 0u64);
    for _ in 0..REPS {
        let t = cycles();
        n = dirty::diff(fb, other, &mut rects);
        let c = cycles().wrapping_sub(t);
        best = best.min(c);
        sum += u64::from(c);
    }
    info!(
        "bench=diff{label} case=identical rects={n} us_min={} us_avg={}",
        us(u64::from(best)),
        us(sum / u64::from(REPS))
    );

    // A 16x16 sprite moved by 3 pixels: two small changed areas.
    for (i, p) in other.0.iter_mut().enumerate() {
        let (x, y) = (i % LW, i / LW);
        if (40..56).contains(&x) && (30..46).contains(&y) {
            *p = p.wrapping_add(1);
        }
        if (43..59).contains(&x) && (90..106).contains(&y) {
            *p = p.wrapping_add(1);
        }
    }
    let (mut best, mut sum) = (u32::MAX, 0u64);
    for _ in 0..REPS {
        let t = cycles();
        n = dirty::diff(fb, other, &mut rects);
        let c = cycles().wrapping_sub(t);
        best = best.min(c);
        sum += u64::from(c);
    }
    info!(
        "bench=diff{label} case=two_sprites rects={n} first={:?} us_min={} us_avg={}",
        rects[0],
        us(u64::from(best)),
        us(sum / u64::from(REPS))
    );

    // Every row changed in its first and last word (worst for the
    // scan from both ends is "identical"; this is the most rows).
    for p in other.0.iter_mut() {
        *p = p.wrapping_add(1);
    }
    let (mut best, mut sum) = (u32::MAX, 0u64);
    for _ in 0..REPS {
        let t = cycles();
        n = dirty::diff(fb, other, &mut rects);
        let c = cycles().wrapping_sub(t);
        best = best.min(c);
        sum += u64::from(c);
    }
    info!(
        "bench=diff{label} case=all_changed rects={n} us_min={} us_avg={}",
        us(u64::from(best)),
        us(sum / u64::from(REPS))
    );

    // Many separate bands (every 4th row changed): merging work.
    other.0.copy_from_slice(&fb.0);
    for y in (0..LH).step_by(4) {
        other.0[y * LW + (y % LW)] ^= 0x55;
    }
    let (mut best, mut sum) = (u32::MAX, 0u64);
    for _ in 0..REPS {
        let t = cycles();
        n = dirty::diff(fb, other, &mut rects);
        let c = cycles().wrapping_sub(t);
        best = best.min(c);
        sum += u64::from(c);
    }
    info!(
        "bench=diff{label} case=30_bands_merged rects={n} us_min={} us_avg={}",
        us(u64::from(best)),
        us(sum / u64::from(REPS))
    );

    // Copying a whole logical frame (what a buffer swap would cost).
    let (mut best, mut sum) = (u32::MAX, 0u64);
    for _ in 0..REPS {
        let t = cycles();
        other.0.copy_from_slice(&fb.0);
        let c = cycles().wrapping_sub(t);
        best = best.min(c);
        sum += u64::from(c);
    }
    info!(
        "bench=frame_copy{label} bytes={} us_min={} us_avg={}",
        LW * LH,
        us(u64::from(best)),
        us(sum / u64::from(REPS))
    );

    // Converting and doubling a whole frame into strips, nothing sent.
    for depth in [Depth::Rgb565, Depth::Rgb444] {
        for strip_rows in [4usize, 8, 12] {
            let (mut best, mut sum) = (u32::MAX, 0u64);
            let mut bytes = 0;
            for _ in 0..REPS {
                let buf = lcd.strip_buffer();
                bytes = 0;
                let t = cycles();
                for k in 0..LH / strip_rows {
                    bytes += pipeline::fill(depth, fb, pal, k * strip_rows, strip_rows, 0, LW, buf);
                }
                let c = cycles().wrapping_sub(t);
                best = best.min(c);
                sum += u64::from(c);
            }
            info!(
                "bench=convert{label} depth={} strip_rows={strip_rows} bytes={bytes} us_min={} us_avg={} cycles_per_logical_pixel={}",
                depth.bits(),
                us(u64::from(best)),
                us(sum / u64::from(REPS)),
                u64::from(best) / (LW * LH) as u64,
            );
        }
    }
}

/// Reads the whole panel back and compares it with `fb` at 2x through
/// the palette, in the given depth's precision. Returns the number of
/// wrong pixels.
pub fn verify_fb(
    lcd: &mut Lcd,
    fb: &Frame,
    pal: &Palette,
    depth: Depth,
    scratch: &mut [u8],
) -> (u32, Option<(u16, u16, u16, u16)>) {
    let mut wrong = 0;
    let mut first = None;
    for y in 0..H as u16 {
        let Ok(row) = lcd.read_row(0, y, W as u16, scratch) else {
            wrong += W as u32;
            continue;
        };
        let logical = fb.row(usize::from(y) / 2);
        for x in 0..W {
            let c = pal.rgb565[usize::from(logical[x / 2])];
            let (expected, got) = match depth {
                Depth::Rgb565 => (c, lcd::read_to_565(&row[3 * x..3 * x + 3])),
                Depth::Rgb444 => (
                    pipeline::rgb565_to_444(c),
                    lcd::read_to_444(&row[3 * x..3 * x + 3]),
                ),
            };
            if expected != got {
                wrong += 1;
                if first.is_none() {
                    first = Some((x as u16, y, expected, got));
                }
            }
        }
    }
    (wrong, first)
}

/// The test card through the pipeline at each clock and depth, read
/// back and compared. Also: which 6-bit values the panel stores for
/// the sixteen 4-bit levels of 12-bit colour, and whether the read
/// format follows COLMOD.
pub fn colours(lcd: &mut Lcd, fb: &mut Frame, pal: &mut Palette, scratch: &mut [u8]) {
    for mhz in [40, 80] {
        let _ = lcd.set_write_mhz(mhz);
        for depth in [Depth::Rgb565, Depth::Rgb444] {
            let _ = lcd.set_depth(depth);
            let sent = pipeline::present_full(lcd, fb, pal, 8, Send::Overlap);
            // Read in 16-bit mode, whatever was sent.
            let _ = lcd.set_depth(Depth::Rgb565);
            let (wrong, first) = verify_fb(lcd, fb, pal, depth, scratch);
            info!(
                "bench=card_readback mhz={mhz} depth={} sent_ok={} pixels=76800 wrong={wrong} first_wrong={:?}",
                depth.bits(),
                sent.is_ok(),
                first
            );
        }
    }
    let _ = lcd.set_write_mhz(40);

    // The 16 grey levels of 12-bit colour, one per 10 logical columns.
    let saved = pal.rgb565;
    let saved_top = fb.0[..LW * 2].to_vec_array();
    for n in 0..16u16 {
        pal.set(240 + usize::from(n), (n << 12) | (n << 7) | (n << 1));
    }
    for y in 0..2 {
        for x in 0..LW {
            fb.0[y * LW + x] = 240 + (x / 10) as u8;
        }
    }
    let _ = lcd.set_depth(Depth::Rgb444);
    let _ = pipeline::present_rect(
        lcd,
        fb,
        pal,
        Rect {
            x: 0,
            y: 0,
            w: LW as u16,
            h: 2,
        },
        8,
        Send::Overlap,
    );
    // Read one row with COLMOD still 12-bit, then with 16-bit.
    let mut r12 = [0u8; 3 * 16];
    let mut r16 = [0u8; 3 * 16];
    for (i, out) in [&mut r12, &mut r16].into_iter().enumerate() {
        if i == 1 {
            let _ = lcd.set_depth(Depth::Rgb565);
        }
        if let Ok(row) = lcd.read_row(0, 0, W as u16, scratch) {
            for n in 0..16 {
                // The middle of each level's 20 panel columns.
                out[3 * n..3 * n + 3]
                    .copy_from_slice(&row[3 * (n * 20 + 10)..3 * (n * 20 + 10) + 3]);
            }
        }
    }
    let six = |v: &[u8; 48], c: usize| {
        let mut s = [0u8; 16];
        for n in 0..16 {
            s[n] = v[3 * n + c] >> 2;
        }
        s
    };
    info!(
        "bench=levels12 read_with=colmod53 r6={:?} g6={:?} b6={:?}",
        six(&r12, 0),
        six(&r12, 1),
        six(&r12, 2)
    );
    info!(
        "bench=levels12 read_with=colmod55 r6={:?} g6={:?} b6={:?}",
        six(&r16, 0),
        six(&r16, 1),
        six(&r16, 2)
    );
    info!("bench=levels12 read_format_follows_colmod={}", r12 != r16);
    pal.set_all(&saved);
    fb.0[..LW * 2].copy_from_slice(&saved_top);
    let _ = pipeline::present_full(lcd, fb, pal, 8, Send::Overlap);
}

/// A fixed-size copy without the heap.
trait ToVecArray {
    fn to_vec_array(&self) -> [u8; LW * 2];
}

impl ToVecArray for [u8] {
    fn to_vec_array(&self) -> [u8; LW * 2] {
        let mut a = [0u8; LW * 2];
        a.copy_from_slice(self);
        a
    }
}
