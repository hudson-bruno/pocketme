//! Lab (feature `fs-bench`, letter `X`): the full-screen scroll bench.
//!
//! A stand-in for a game that scrolls the WHOLE screen: every frame the
//! 160x120 picture moves one pixel to the left, so all 76,800 panel
//! pixels change and are sent, with the normal tear-free timing. Before
//! each frame a "game update" stand-in works for 0, 5, 10 or 20 ms
//! (see standin.rs). Asked rates: 30 and 60 frames a second.
//!
//! Three ways to share the work between the cores ("modes"):
//! - `a`: everything on the first core, one thing after the other.
//! - `b`: the update on the SECOND core: the update for the next frame
//!   runs while the first core sends this one; the first core waits for
//!   it before it draws the next picture.
//! - `c`: the sending on the second core: the first core copies the
//!   picture into a box for the second core and goes on with the next
//!   frame's update while the second core sends.
//!   (b and c need the feature `core2`; without it only `a` runs.)
//! - `s` (feature `core2-swap`, in place of `c`): the sending on the
//!   second core with NO copy: the console's own two pictures and
//!   console-video's `present_handing` (find, wait for the second core,
//!   hand over, catch up, swap). The first core scrolls the console's
//!   drawn picture in place. The bench line adds the catch-up time (from
//!   the hand-over to the end of `present_handing`) and how often and
//!   how long the first core waited for the second.
//!
//! Each case runs 10 seconds and writes one `bench` line to the log.
//! While the bench runs, the console is not drawn; afterwards the panel
//! still shows the bench picture until the console redraws that part
//! (restart the board for a clean screen).

use core::cell::UnsafeCell;

use board_api::{ByteOrder, Dir, Indexed, PixelSource, Rect, Screen};
use embassy_time::{Instant, Timer};
use log::info;

use crate::app::App;
use crate::board::now_us;
use crate::lcd::{H, W};

const PW: usize = W / 2;
const PH: usize = H / 2;
/// Length of one case.
const CASE_US: u64 = 10_000_000;

struct Pic(UnsafeCell<[u8; PW * PH]>);
// SAFETY: only the bench (first core) writes it, and only while no copy
// of it is being sent (mode c sends a copy).
unsafe impl Sync for Pic {}
static PIC: Pic = Pic(UnsafeCell::new([0; PW * PH]));

struct Source<'a> {
    pixels: &'a [u8],
    palette: &'a [u16; 256],
}

impl PixelSource for Source<'_> {
    fn run(&self, x: u16, y: u16, dir: Dir, order: ByteOrder, out: &mut [u16]) {
        let (mut x, mut y) = (i32::from(x), i32::from(y));
        let (dx, dy) = match dir {
            Dir::Right => (1, 0),
            Dir::Left => (-1, 0),
            Dir::Down => (0, 1),
            Dir::Up => (0, -1),
        };
        for o in out.iter_mut() {
            let c = self.palette[usize::from(self.pixels[(y as usize / 2) * PW + x as usize / 2])];
            *o = match order {
                ByteOrder::Native => c,
                ByteOrder::Wire => c.to_be(),
            };
            x += dx;
            y += dy;
        }
    }

    fn indexed(&self) -> Option<Indexed<'_>> {
        Some(Indexed {
            pixels: self.pixels,
            width: PW as u16,
            height: PH as u16,
            scale: 2,
            left: 0,
            top: 0,
            palette: self.palette,
            palette_version: 0xBE4C,
            border: 0,
        })
    }
}

/// The colours: a simple rainbow-ish ramp (RGB565).
fn palette() -> [u16; 256] {
    let mut p = [0u16; 256];
    for (i, c) in p.iter_mut().enumerate() {
        let i = i as u16;
        let r = (i >> 3) & 0x1F;
        let g = (i.wrapping_mul(3) >> 2) & 0x3F;
        let b = (31 - (i >> 3)) & 0x1F;
        *c = (r << 11) | (g << 5) | b;
    }
    p
}

/// The new column at the right edge for frame `f`.
fn colour(f: u32, y: usize) -> u8 {
    ((f / 4 + y as u32 / 6) * 37 % 256) as u8
}

/// "Draws" the next picture: everything one pixel to the left, a new
/// column at the right.
fn draw(f: u32) {
    // SAFETY: see `Pic`; the caller made sure no send reads it now.
    let pic = unsafe { &mut *PIC.0.get() };
    for (y, row) in pic.chunks_exact_mut(PW).enumerate() {
        row.copy_within(1.., 0);
        row[PW - 1] = colour(f, y);
    }
}

/// Runs every case and writes the results to the log.
pub async fn matrix(app: &mut App) {
    let pal = palette();
    // A first picture: diagonal stripes.
    {
        // SAFETY: nothing sends it yet.
        let pic = unsafe { &mut *PIC.0.get() };
        for (i, px) in pic.iter_mut().enumerate() {
            *px = ((i % PW + i / PW) * 5 % 256) as u8;
        }
    }
    #[cfg(all(feature = "core2", not(feature = "core2-swap")))]
    let modes: &[u8] = if crate::core2::started() {
        b"abc"
    } else {
        b"a"
    };
    #[cfg(feature = "core2-swap")]
    let modes: &[u8] = if crate::core2::started() {
        b"abs"
    } else {
        b"a"
    };
    #[cfg(not(feature = "core2"))]
    let modes: &[u8] = b"a";
    // Mode s draws into the console's own picture, with this palette;
    // the console's colours are put back afterwards.
    #[cfg(feature = "core2-swap")]
    let console_pal = *app.console.video().palette().colours();
    #[cfg(feature = "core2")]
    let was_here = crate::core2::send_here();
    info!(
        "bench: full-screen scroll, {} cases of {} s, features {}",
        modes.len() * 8,
        CASE_US / 1_000_000,
        crate::FEATURES
    );
    crate::logbuf::drain(&mut app.uart);
    for rate in [30u32, 60] {
        for &mode in modes {
            for burn_ms in [0u32, 5, 10, 20] {
                case(app, mode, burn_ms, rate, &pal).await;
                crate::logbuf::flush(&mut app.uart);
            }
        }
    }
    #[cfg(feature = "core2")]
    crate::core2::set_send_here(was_here, &mut app.board.screen);
    #[cfg(feature = "core2-swap")]
    {
        let video = app.console.video_mut();
        video.palette_mut().set_all(&console_pal);
        video.force_full();
    }
    app.board.screen.reset_counters();
    info!("bench: done (the panel shows the bench picture until the console redraws)");
}

async fn case(app: &mut App, mode: u8, burn_ms: u32, rate: u32, pal: &[u16; 256]) {
    let screen = &mut app.board.screen;
    screen.idle();
    #[cfg(feature = "core2")]
    crate::core2::set_send_here(mode == b'c' || mode == b's', screen);
    #[cfg(feature = "core2-swap")]
    let console = &mut *app.console;
    #[cfg(feature = "core2-swap")]
    if mode == b's' {
        // The console's picture becomes the bench picture: the stripes,
        // then sent whole once (not counted).
        let video = console.video_mut();
        video.palette_mut().set_all(pal);
        // SAFETY: nothing sends PIC now.
        video
            .frame_mut()
            .0
            .copy_from_slice(unsafe { &*PIC.0.get() });
        video.force_full();
        let mut out = crate::core2::swap::SwapSender::new(screen as *mut _);
        let _ = video.present_handing::<crate::screen::CydScreen, _>(&mut out);
        crate::core2::join();
    }
    screen.reset_counters();
    let period = 1_000_000 / u64::from(rate);
    let full = Rect::new(0, 0, W as u16, H as u16);
    let burn_us = burn_ms * 1000;

    let first = now_us();
    let end = first + CASE_US;
    let mut next = first;
    let mut frames = 0u32;
    let mut last_start = 0u64;
    let mut gap_max = 0u64;
    let mut slow_gaps = 0u32;
    let mut work_sum = 0u64;
    let mut work_max = 0u64;
    #[allow(unused_mut)]
    let mut join_sum = 0u64;
    #[allow(unused_mut, unused_variables)]
    let mut kicked = false;
    #[allow(unused_mut)]
    let (mut catch_sum, mut catch_max) = (0u64, 0u64);
    screen.reset_counters();
    #[cfg(feature = "core2-swap")]
    crate::core2::swap::reset();
    loop {
        if now_us() < next {
            Timer::at(Instant::from_micros(next)).await;
        }
        let start = now_us();
        if start >= end {
            break;
        }
        if frames > 0 {
            let gap = start - last_start;
            gap_max = gap_max.max(gap);
            if gap * 2 > period * 3 {
                slow_gaps += 1;
            }
        }
        last_start = start;

        // The game's update for this frame.
        if mode == b'b' {
            #[cfg(feature = "core2")]
            if kicked {
                // Started last frame on the second core: wait for it.
                join_sum += u64::from(crate::core2::join() / crate::lcd::CPU_MHZ);
            }
        } else if burn_us > 0 {
            core::hint::black_box(crate::standin::burn(burn_us));
        }
        // Drawing needs the picture: in mode c the second core sends a
        // copy, so the original is free.
        #[cfg(feature = "core2-swap")]
        if mode == b's' {
            // Scroll the console's drawn picture in place (after the
            // catch-up it equals the one handed over last).
            let video = console.video_mut();
            for (y, row) in video.frame_mut().0.chunks_exact_mut(PW).enumerate() {
                row.copy_within(1.., 0);
                row[PW - 1] = colour(frames, y);
            }
            let mut out = crate::core2::swap::SwapSender::new(screen as *mut _);
            let _ = video.present_handing::<crate::screen::CydScreen, _>(&mut out);
            if let Some(t) = out.handed_at {
                let us = u64::from(crate::lcd::cycles().wrapping_sub(t) / crate::lcd::CPU_MHZ);
                catch_sum += us;
                catch_max = catch_max.max(us);
            }
        }
        if mode != b's' {
            draw(frames);
        }
        #[cfg(feature = "core2")]
        if mode == b'b' && burn_us > 0 {
            // The update for the NEXT frame, side by side with sending this one.
            crate::core2::kick_burn(burn_us);
            kicked = true;
        }
        if mode != b's' {
            // SAFETY: read only while sent (mode a/b) or copied (mode c).
            let pixels = unsafe { &*PIC.0.get() };
            let src = Source {
                pixels,
                palette: pal,
            };
            let _ = screen.present(&src, &[full]);
        }
        let done = now_us();
        work_sum += done - start;
        work_max = work_max.max(done - start);
        frames += 1;

        next += period;
        if next <= done {
            next = done;
        }
    }
    #[cfg(feature = "core2")]
    crate::core2::join();
    screen.idle();
    let c = screen.counters_now();
    let span = last_start.saturating_sub(first).max(1);
    let fps_x100 = u64::from(frames.saturating_sub(1)) * 100_000_000 / span;
    #[allow(unused_mut)]
    let (mut waits, mut wait_sum, mut wait_max) = (0u32, 0u32, 0u32);
    #[cfg(feature = "core2-swap")]
    if mode == b's' {
        let w = crate::core2::swap::waits();
        (waits, wait_sum, wait_max) = w;
        join_sum = u64::from(wait_sum);
    }
    info!(
        "bench mode={} burn_ms={burn_ms} asked={rate} frames={frames} fps={}.{:02} gap_max_us={gap_max} slow_gaps={slow_gaps} first_core_frame_avg_us={} first_core_frame_max_us={work_max} join_avg_us={} presents={} present_avg_us={} present_max_us={} windows={} untimed={} no_plan={} late={} crossed={} errors={}",
        mode as char,
        fps_x100 / 100,
        fps_x100 % 100,
        work_sum / u64::from(frames.max(1)),
        join_sum / u64::from(frames.max(1)),
        c.presents,
        c.total_us / u64::from(c.presents.max(1)),
        c.total_max,
        c.windows,
        c.untimed,
        c.no_plan,
        c.late,
        c.crossed,
        c.errors,
    );
    // A second line (the log cuts long lines): the swap's own costs.
    if mode == b's' {
        info!(
            "bench_swap burn_ms={burn_ms} asked={rate} catch_up_avg_us={} catch_up_max_us={catch_max} waits={waits} of {frames} wait_avg_us={} wait_max_us={wait_max}",
            catch_sum / u64::from(frames.max(1)),
            wait_sum / waits.max(1),
        );
    }
}
