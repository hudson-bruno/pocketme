//! The screen part of the board: `board_api::Screen` on the ST7789.
//!
//! How a picture is sent ("the columns way", proven tear-free on this
//! board in experiments/display-dma: 0% torn frames at 55 frames a
//! second): the panel refreshes one panel column after the other, left
//! to right. With MADCTL 40h a memory write also runs column by column,
//! left to right. So each changed area is sent as ONE window, column by
//! column, started at a moment when the refresh has just passed its
//! first column and will not catch up with the writer before it is done.
//! The writer (80 MHz, 16-bit) needs 48 us a column, the refresh 49.5:
//! the writer follows just behind the refresh and never meets it.
//!
//! The strip buffers hold whole picture columns (each sent as two panel
//! columns). The first strip of a window is small (one picture column)
//! so the wire starts at once; each next strip is filled while the one
//! before is on the wire.
//!
//! Which windows (2026-11-18, send-plan's `choose`): a turning 3D view
//! changes about ten wide, low bands. Each such window may only start in
//! a narrow span of the refresh, so they seldom fit one refresh pass and
//! the present waited refresh after refresh. Now the present also looks
//! at ONE box around all the areas (a taller window may start in a much
//! wider span) and takes whichever of the two the timing model says is
//! done sooner. Pixels inside the box that did not change are sent with
//! the values the panel already shows (with `core2` they come from the
//! picture handed to the second core, which nobody writes while it is
//! sent: src/core2.rs, "The send plan"). The choice and every start time
//! are worked out BEFORE the first window; between the first and the
//! last window only the wait, the send path and a few stores run.
//! Today's plan stays reachable for comparing (feature `send-ab`).
//!
//! Where the code runs from: `fill_fast` from RAM (with the scanline read
//! and esp-hal's low-level SPI driver); everything else of a present
//! (`present` with its loop over the windows, `send`, the window
//! set-up, esp-hal's `SpiDma` write and DMA set-up, the SPI clock
//! switches, the plan) from flash, as ONE block placed after all other
//! flash code: boards/cyd/hot-block.list names the functions, build.rs
//! turns it into the linker script hot-block.x, and hot-check.py checks
//! a built firmware against it. The part run for every window is under
//! 16 KB and the whole block under 32 KB, so no three of its lines share
//! a set of the two-way flash cache, wherever the linker puts other
//! code. Before, that depended on the layout: the lead (call to first
//! pixel byte) was 30 us in one build and over 100 us in the next (README
//! "The timed paths and the flash cache").

use board_api::{ByteOrder, Dir, PixelSource, Rect, Screen, SendReport};

use send_plan::choose::{Shape, choose};
use send_plan::{Job, MAX_WAIT_REFRESHES, Panel, crossed, plan};

/// Most windows sent apart in one present (send-plan's; src/core2.rs
/// sizes its hand-over box with it).
pub use send_plan::MAX_JOBS;

use crate::beam::{Beam, floor, spin_us};
use crate::lcd::{CPU_MHZ, H, Lcd, LcdError, MADCTL_COLS, STRIP_BYTES, W, cycles};

/// The panel, for send-plan.
const PANEL: Panel = Panel {
    w: W as u16,
    h: H as u16,
};

/// The window for area `r` (inside the panel), by the beam's numbers.
fn job(r: Rect, beam: &Beam, mhz: u32) -> Job {
    Job::new(
        (r.x, r.y, r.w, r.h),
        PANEL,
        |x| beam.gate(x),
        CPU_MHZ as f32 / beam.line_cycles,
        mhz,
    )
}

// MAX_JOBS (send-plan): most windows sent apart in one present; more
// are sent as one full frame. console-video hands at most 16.

// The timing rules (Job, its start window, plan, crossed, GUARD_LINES,
// LEAD_US, TAIL_US, WIRE_SLOWDOWN, MAX_WAIT_REFRESHES) live in the
// send-plan crate (boards/cyd/send-plan), plain logic tested on the PC.

/// First strip of a window at most this many bytes, the second at most
/// [`SECOND_STRIP_BYTES`], then whole strip buffers.
const FIRST_STRIP_BYTES: usize = 1024;
const SECOND_STRIP_BYTES: usize = 4096;

/// Counters, since start (or since the last [`Counters::take`]).
#[derive(Clone, Copy, Default)]
pub struct Counters {
    pub presents: u32,
    pub windows: u32,
    /// Windows sent with no timing: no usable anchor, or no pass fitted.
    pub untimed: u32,
    /// Presents where no anchor read worked (and none recent to reuse),
    /// and where the windows did not all fit in one refresh pass (each
    /// then got its own pass).
    pub no_anchor: u32,
    pub no_plan: u32,
    /// Windows that started after their start window had closed.
    pub late: u32,
    /// Windows the model says were crossed by the refresh (torn).
    pub crossed: u32,
    /// How the new plan sent its timed presents: the areas as their own
    /// windows in one pass, or one box around them in one pass. (The old
    /// plan counts neither; `no_plan` counts, for both, presents with no
    /// one-pass plan.)
    pub as_windows: u32,
    pub as_box: u32,
    /// Refresh passes begun from the plan to the last byte, over the
    /// timed presents (0 for a present done in the pass it planned in).
    pub refreshes: u32,
    /// Sum and worst of: time per present (all of it), the waiting part
    /// (anchor reads and waiting for the refresh), the sending part.
    pub total_us: u64,
    pub total_max: u32,
    pub wait_us: u64,
    pub wait_max: u32,
    pub send_us: u64,
    pub send_max: u32,
    /// Panel pixels sent (a box also sends pixels that did not change).
    pub pixels: u64,
    /// From the start of a window's send call to its first pixel byte,
    /// in cycles: sum, worst, and over how many windows.
    pub lead_cycles: u64,
    pub lead_max: u32,
    pub leads: u32,
    /// Bytes sent, and the time from first to last pixel byte (the wire's
    /// real speed).
    pub bytes: u64,
    pub wire_cycles: u64,
    /// CPU time filling strips.
    pub fill_cycles: u64,
    pub errors: u32,
}

/// The board's screen.
pub struct CydScreen {
    pub lcd: Lcd,
    pub beam: Beam,
    pub backlight: crate::backlight::Backlight,
    /// The palette, each colour twice in one word, high byte first on the
    /// wire (a doubled pixel), and the palette version it was made from.
    wire: [u32; 256],
    wire_version: Option<u32>,
    /// Strip size in use, at most [`STRIP_BYTES`] (the bench lowers it).
    pub strip_limit: usize,
    /// The tear-free timing; off only for the bench and (feature
    /// `send-ab`, letter `u`) for comparing: "fast, may tear".
    pub timed: bool,
    /// Measuring only (feature `send-ab`, letters `o` and `p`): today's
    /// plan (every area its own window; each window its own pass when
    /// they do not fit in one) instead of the new choice.
    #[cfg(feature = "send-ab")]
    pub old_plan: bool,
    pub counters: Counters,
    /// Cycle counter at the last present, to forget an old anchor.
    last_present: Option<u64>,
    /// Per window of a present planned in one pass: when to start and
    /// from when it is late (worked out before the first window), and
    /// what its send measured (looked at after the last window).
    marks: [Mark; MAX_JOBS],
}

/// One window of a present planned in one pass (cycle counts): when to
/// start, from when it is late, when its send call started, its first
/// and last pixel byte (the call's start and end when the send did not
/// say), and [`Mark::OK`] / [`Mark::FIRST`] (the send said when its first
/// byte went).
#[derive(Clone, Copy, Default)]
struct Mark {
    target: u32,
    latest: u32,
    start: u32,
    first: u32,
    last: u32,
    flags: u8,
}

impl Mark {
    const OK: u8 = 1;
    const FIRST: u8 = 2;
}

impl CydScreen {
    pub fn new(lcd: Lcd, backlight: crate::backlight::Backlight) -> Self {
        Self {
            lcd,
            beam: Beam::new(),
            backlight,
            wire: [0; 256],
            wire_version: None,
            strip_limit: STRIP_BYTES,
            timed: true,
            #[cfg(feature = "send-ab")]
            old_plan: false,
            counters: Counters::default(),
            last_present: None,
            marks: [Mark::default(); MAX_JOBS],
        }
    }

    /// Whether today's plan is in use (only ever with `send-ab`).
    #[inline(always)]
    pub fn old_plan(&self) -> bool {
        #[cfg(feature = "send-ab")]
        return self.old_plan;
        #[cfg(not(feature = "send-ab"))]
        false
    }

    /// Sends `jobs`, all planned in refresh pass `k`. Every start and
    /// "late" moment is worked out first; between the first and the last
    /// window only the wait, the send path (the same code for every
    /// window), a few stores and two sums run: no planning, no float.
    /// Returns the waiting and sending cycles.
    fn send_in_pass(
        &mut self,
        jobs: &[Job],
        k: f32,
        src: &(impl PixelSource + ?Sized),
        pixels: Option<&[u8]>,
        max_wait: u32,
    ) -> (u32, u32) {
        let lines = self.beam.lines;
        for (m, j) in self.marks.iter_mut().zip(jobs) {
            let (lo, hi) = j.window(lines);
            m.target = self.beam.cycles_at(k * lines + lo - j.lead);
            m.latest = self.beam.cycles_at(k * lines + hi - j.lead);
        }
        // The timed part.
        let (mut wait, mut send) = (0u32, 0u32);
        for (i, job) in jobs.iter().enumerate() {
            let target = self.marks[i].target;
            // Spin on the cycle counter (no bus traffic) until the first
            // byte would go out at the start of the window.
            let w0 = cycles();
            while (cycles().wrapping_sub(target) as i32) < 0 && cycles().wrapping_sub(w0) < max_wait
            {
            }
            let s = cycles();
            wait = wait.wrapping_add(s.wrapping_sub(w0));
            let result = self.send(job, src, pixels);
            let e = cycles();
            send = send.wrapping_add(e.wrapping_sub(s));
            let m = &mut self.marks[i];
            m.start = s;
            (m.first, m.last, m.flags) = match result {
                Ok(t) => {
                    self.counters.bytes += u64::from(t.bytes);
                    self.counters.fill_cycles += u64::from(t.fill);
                    let first = if t.first_byte.is_some() {
                        Mark::FIRST
                    } else {
                        0
                    };
                    (
                        t.first_byte.unwrap_or(s),
                        t.last_byte.unwrap_or(e),
                        Mark::OK | first,
                    )
                }
                Err(_) => (s, e, 0),
            };
        }
        // After the last window: the counters and the model's look at
        // whether the refresh met a writer.
        for (m, job) in self.marks.iter().zip(jobs) {
            let c = &mut self.counters;
            c.windows += 1;
            if (m.start.wrapping_sub(m.latest) as i32) > 0 {
                c.late += 1;
            }
            if m.flags & Mark::OK == 0 {
                c.errors += 1;
                continue;
            }
            c.wire_cycles += u64::from(m.last.wrapping_sub(m.first));
            if m.flags & Mark::FIRST != 0 {
                count_lead(c, m.first.wrapping_sub(m.start));
            }
            if crossed(
                self.beam.v_at(m.first),
                self.beam.v_at(m.last),
                job.g0,
                job.g1,
                lines,
            ) {
                c.crossed += 1;
            }
        }
        (wait, send)
    }

    /// Sends one window. `fast` is the indexed picture when it is the
    /// plain 160x120 at 2x; else every pixel comes from `src.run`.
    fn send(
        &mut self,
        job: &Job,
        src: &(impl PixelSource + ?Sized),
        fast: Option<&[u8]>,
    ) -> Result<crate::lcd::Timing, LcdError> {
        // One unit = one picture column = two panel columns.
        let units = usize::from(job.w / 2);
        let unit_bytes = 4 * usize::from(job.h);
        let full = (self.strip_limit / unit_bytes).max(1);
        let first = (FIRST_STRIP_BYTES / unit_bytes).clamp(1, full);
        let second = (SECOND_STRIP_BYTES / unit_bytes).clamp(1, full);
        let start = |k: usize| -> usize {
            let s = match k {
                0 => 0,
                1 => first,
                _ => first + second + (k - 2) * full,
            };
            s.min(units)
        };
        let strips = if units <= first {
            1
        } else if units <= first + second {
            2
        } else {
            2 + (units - first - second).div_ceil(full)
        };
        let window = (job.y0, job.x0, job.y0 + job.h - 1, job.x0 + job.w - 1);
        let wire = &self.wire;
        let (x0, y0, h) = (usize::from(job.x0), usize::from(job.y0), usize::from(job.h));
        self.lcd.draw_in(MADCTL_COLS, window, strips, |k, buf| {
            let (a, b) = (start(k), start(k + 1));
            match fast {
                Some(pixels) => fill_fast(pixels, wire, x0 / 2 + a, b - a, y0 / 2, h / 2, buf),
                None => fill_run(src, x0 + 2 * a, 2 * (b - a), y0, h, buf),
            }
        })
    }
}

/// Feature `core2`: the two calls the second core's sender needs
/// (src/core2.rs). Without the feature this block is not there, and
/// `present` above is the same code either way.
#[cfg(feature = "core2")]
impl CydScreen {
    /// Makes the wire palette (each colour doubled, wire byte order) from
    /// `palette`, unless it was made from this version already: the
    /// same as `present` does for the fast path. The first core calls it
    /// in `hand_over`, while the second core holds no job, so the second
    /// core never reads the console's palette.
    pub fn set_wire(&mut self, palette: &[u16; 256], version: u32) {
        if self.wire_version != Some(version) {
            for (out, &c) in self.wire.iter_mut().zip(palette.iter()) {
                let w = u32::from(c.to_be());
                *out = w | (w << 16);
            }
            self.wire_version = Some(version);
        }
    }

    /// Sends the plain 160x120 picture `pixels` at 2x, through `present`
    /// (the same timing, the same counters), with the wire palette set
    /// before by [`CydScreen::set_wire`]. Sends nothing if no wire palette
    /// was ever made (the sender always makes one first).
    pub fn present_held(&mut self, pixels: &[u8], areas: &[Rect]) -> SendReport {
        let Some(version) = self.wire_version else {
            return SendReport::default();
        };
        self.present(&Held { pixels, version }, areas)
    }
}

/// Feature `core2`: the held picture as a pixel source. It says it is the
/// plain 2x picture, with the palette version the wire palette was made
/// from, so `present` takes the fast path and never looks at the
/// colours (an all-black table stands in for them).
#[cfg(feature = "core2")]
struct Held<'a> {
    pixels: &'a [u8],
    version: u32,
}

#[cfg(feature = "core2")]
static NO_COLOURS: [u16; 256] = [0; 256];

#[cfg(feature = "core2")]
impl PixelSource for Held<'_> {
    fn run(&self, _x: u16, _y: u16, _dir: Dir, _order: ByteOrder, out: &mut [u16]) {
        // Never asked (the fast path is always taken); black if it were.
        out.fill(0);
    }

    fn indexed(&self) -> Option<board_api::Indexed<'_>> {
        Some(board_api::Indexed {
            pixels: self.pixels,
            width: (W / 2) as u16,
            height: (H / 2) as u16,
            scale: 2,
            left: 0,
            top: 0,
            palette: &NO_COLOURS,
            palette_version: self.version,
            border: 0,
        })
    }
}

/// One window's lead: cycles from the start of its send call to its first
/// pixel byte.
#[inline(always)]
fn count_lead(c: &mut Counters, cycles: u32) {
    c.lead_cycles += u64::from(cycles);
    c.lead_max = c.lead_max.max(cycles);
    c.leads += 1;
}

/// Views a strip buffer as words. DMA buffers from esp-hal are 4-aligned.
fn words_mut(buf: &mut [u8]) -> &mut [u32] {
    // SAFETY: any bit pattern is a valid u32; `align_to_mut` only hands
    // out the aligned middle part.
    let (head, words, _) = unsafe { buf.align_to_mut::<u32>() };
    assert!(head.is_empty(), "strip buffer not 4-aligned");
    words
}

/// The fast fill: picture columns `px..px + cols`, picture rows
/// `py..py + rows`, from the 160x120 indexed picture. Each picture pixel
/// is looked up once and written as two panel pixels (one word); each
/// picture column becomes two equal panel columns. Returns bytes.
#[esp_hal::ram] // in RAM: no flash cache misses while the wire waits
fn fill_fast(
    pixels: &[u8],
    wire: &[u32; 256],
    px: usize,
    cols: usize,
    py: usize,
    rows: usize,
    dst: &mut [u8],
) -> usize {
    const PW: usize = W / 2;
    let words = words_mut(dst);
    let mut at = 0;
    for c in px..px + cols {
        let (first, rest) = words[at..at + 2 * rows].split_at_mut(rows);
        let mut i = py * PW + c;
        for out in first.iter_mut() {
            *out = wire[usize::from(pixels[i])];
            i += PW;
        }
        rest.copy_from_slice(first);
        at += 2 * rows;
    }
    at * 4
}

/// The general fill: panel columns `x..x + cols`, rows `y..y + h`, each
/// column one `run` of the source, top to bottom, in wire byte order.
fn fill_run(
    src: &(impl PixelSource + ?Sized),
    x: usize,
    cols: usize,
    y: usize,
    h: usize,
    dst: &mut [u8],
) -> usize {
    let words = words_mut(dst);
    // SAFETY: a [u32] seen as twice as many u16, same memory and
    // alignment (u16 needs less).
    let (_, halves, _) = unsafe { words.align_to_mut::<u16>() };
    for (i, col) in (x..x + cols).enumerate() {
        src.run(
            col as u16,
            y as u16,
            Dir::Down,
            ByteOrder::Wire,
            &mut halves[i * h..(i + 1) * h],
        );
    }
    cols * h * 2
}

impl Screen for CydScreen {
    const PANEL_W: u16 = W as u16;
    const PANEL_H: u16 = H as u16;

    /// Never inlined: the per-window loop must stay a function of its
    /// own, so that hot-block.x can place it in the hot block (see the
    /// module's comment).
    #[inline(never)]
    fn present<S: PixelSource + ?Sized>(&mut self, src: &S, areas: &[Rect]) -> SendReport {
        if areas.is_empty() {
            return SendReport::default();
        }
        let t_start = cycles();
        let now = embassy_time::Instant::now().as_micros();
        // An anchor from more than a second ago is not trusted (the cycle
        // counter wraps every 17.9 s).
        if self
            .last_present
            .is_none_or(|t| now.saturating_sub(t) > 1_000_000)
        {
            self.beam.forget();
        }
        self.last_present = Some(now);

        // The fast path when the source is the plain 2x picture.
        let fast = src.indexed().filter(|ix| {
            ix.scale == 2
                && ix.left == 0
                && ix.top == 0
                && usize::from(ix.width) * 2 == W
                && usize::from(ix.height) * 2 == H
        });
        if let Some(ix) = fast
            && self.wire_version != Some(ix.palette_version)
        {
            for (out, &c) in self.wire.iter_mut().zip(ix.palette.iter()) {
                let w = u32::from(c.to_be());
                *out = w | (w << 16);
            }
            self.wire_version = Some(ix.palette_version);
        }
        let pixels = fast.map(|ix| ix.pixels);

        // The windows, sorted by first column.
        let mhz = self.lcd.write_mhz();
        let mut all = [Job::default(); MAX_JOBS];
        let mut n = 0;
        let full = Rect::new(0, 0, W as u16, H as u16);
        if areas.len() > MAX_JOBS {
            all[0] = job(full, &self.beam, mhz);
            n = 1;
        } else {
            for r in areas {
                let r = r.intersect(&full);
                if r.is_empty() {
                    continue;
                }
                all[n] = job(r, &self.beam, mhz);
                n += 1;
            }
        }
        all[..n].sort_unstable_by_key(|j| j.x0);

        // When to start.
        // The new plan: the windows or one box around them, whichever
        // the model says is done sooner (see the top of this file).
        // Today's plan: the windows as they are.
        // Then one refresh pass for all windows when they fit in one;
        // otherwise each window gets the earliest pass it fits in, after
        // the one before it (some windows then wait one more refresh).
        let lines = self.beam.lines;
        let mut anchored = false;
        let mut pass = None;
        let mut planned = 0u32;
        if self.timed {
            anchored = self.beam.anchor(&mut self.lcd);
            if anchored {
                let mut shape = Shape::Windows;
                if !self.old_plan() {
                    let beam = &self.beam;
                    let mut boxed = [Job::default(); 1];
                    let make =
                        |a: (u16, u16, u16, u16)| job(Rect::new(a.0, a.1, a.2, a.3), beam, mhz);
                    let choice = choose(&all[..n], make, beam.v_at(cycles()), lines, &mut boxed);
                    if choice.shape == Shape::Box {
                        all[0] = boxed[0];
                        n = 1;
                    }
                    shape = choice.shape;
                }
                planned = cycles();
                pass = plan(&all[..n], self.beam.v_at(planned), lines);
                match pass {
                    None => self.counters.no_plan += 1,
                    Some(_) if self.old_plan() => {}
                    Some(_) if shape == Shape::Box => self.counters.as_box += 1,
                    Some(_) => self.counters.as_windows += 1,
                }
            } else {
                self.counters.no_anchor += 1;
            }
        }
        let jobs = &all[..n];
        let max_wait = (MAX_WAIT_REFRESHES * lines * self.beam.line_cycles) as u32;

        let c = &mut self.counters;
        c.presents += 1;
        let mut report = SendReport::default();
        let mut wait = cycles().wrapping_sub(t_start);
        let mut send = 0u32;
        // The new plan in one pass: everything worked out before the
        // first window.
        let in_pass = match pass {
            Some(k) if !self.old_plan() => {
                let (w, s) = self.send_in_pass(jobs, k, src, pixels, max_wait);
                wait = wait.wrapping_add(w);
                send = s;
                for job in jobs {
                    report.areas += 1;
                    report.pixels += u32::from(job.w) * u32::from(job.h);
                }
                true
            }
            _ => false,
        };
        // Today's loop (today's plan, the new plan with no one-pass plan,
        // and untimed sends).
        let rest = if in_pass { &jobs[..0] } else { jobs };
        for job in rest.iter() {
            if job.w == 0 || job.h == 0 {
                continue;
            }
            let (lo, hi) = job.window(lines);
            let job_pass = match pass {
                Some(k) => Some(k),
                None if anchored => {
                    plan(core::slice::from_ref(job), self.beam.v_at(cycles()), lines)
                }
                None => None,
            };
            if let Some(k) = job_pass {
                // Spin on the cycle counter (no bus traffic) until the
                // first byte would go out at the start of the window.
                let target = self.beam.cycles_at(k * lines + lo - job.lead);
                let w0 = cycles();
                while (cycles().wrapping_sub(target) as i32) < 0
                    && cycles().wrapping_sub(w0) < max_wait
                {}
                wait = wait.wrapping_add(cycles().wrapping_sub(w0));
                if self.beam.v_at(cycles()) + job.lead > k * lines + hi {
                    self.counters.late += 1;
                }
            } else {
                self.counters.untimed += 1;
            }
            let s = cycles();
            let result = self.send(job, src, pixels);
            let e = cycles();
            send = send.wrapping_add(e.wrapping_sub(s));
            let c = &mut self.counters;
            c.windows += 1;
            match result {
                Ok(t) => {
                    let (fs, fe) = (t.first_byte.unwrap_or(s), t.last_byte.unwrap_or(e));
                    c.bytes += u64::from(t.bytes);
                    c.wire_cycles += u64::from(fe.wrapping_sub(fs));
                    c.fill_cycles += u64::from(t.fill);
                    if let Some(f) = t.first_byte {
                        count_lead(c, f.wrapping_sub(s));
                    }
                    if job_pass.is_some()
                        && crossed(
                            self.beam.v_at(fs),
                            self.beam.v_at(fe),
                            job.g0,
                            job.g1,
                            lines,
                        )
                    {
                        c.crossed += 1;
                    }
                }
                Err(_) => c.errors += 1,
            }
            report.areas += 1;
            report.pixels += u32::from(job.w) * u32::from(job.h);
        }
        let t_end = cycles();
        let total = t_end.wrapping_sub(t_start);
        report.send_us = send / CPU_MHZ;
        report.wait_us = wait / CPU_MHZ;
        let c = &mut self.counters;
        c.total_us += u64::from(total / CPU_MHZ);
        c.total_max = c.total_max.max(total / CPU_MHZ);
        c.wait_us += u64::from(report.wait_us);
        c.wait_max = c.wait_max.max(report.wait_us);
        c.send_us += u64::from(report.send_us);
        c.send_max = c.send_max.max(report.send_us);
        c.pixels += u64::from(report.pixels);
        if anchored {
            let pass_of = |t: u32| floor(self.beam.v_at(t) / lines);
            c.refreshes += (pass_of(t_end) - pass_of(planned)).max(0.0) as u32;
        }
        report
    }

    fn backlight(&mut self, level: u8) {
        // Dimmed by the LEDC unit (backlight.rs): 0 off, 255 full.
        self.backlight.set(level);
    }
}

/// Fills the whole panel with one colour (start-up, before the console's
/// first picture), untimed.
pub fn clear(lcd: &mut Lcd, rgb565: u16) -> Result<(), LcdError> {
    let bytes = W * H * 2;
    let per = STRIP_BYTES - STRIP_BYTES % 4;
    let w = u32::from(rgb565.to_be());
    let word = w | (w << 16);
    lcd.draw_in(
        MADCTL_COLS,
        (0, 0, H as u16 - 1, W as u16 - 1),
        bytes.div_ceil(per),
        |k, buf| {
            let len = per.min(bytes - k * per);
            for out in words_mut(&mut buf[..len]) {
                *out = word;
            }
            len
        },
    )
    .map(|_| ())
}

/// Checks the drawing clock: a test pattern sent at `mhz`, read back.
/// Returns whether every pixel read back right.
pub fn check_clock(lcd: &mut Lcd, mhz: u32) -> bool {
    if lcd.set_write_mhz(mhz).is_err() {
        return false;
    }
    // Eight rows across the top, colours that read back exactly.
    const COLOURS: [u16; 4] = [0xF800, 0x07E0, 0x001F, 0xFFFF];
    let rows = 8u16;
    let colour = |x: usize, y: usize| COLOURS[(x / 3 + y) % 4];
    let ok_send = lcd
        .draw_in(MADCTL_COLS, (0, 0, rows - 1, W as u16 - 1), 1, |_, buf| {
            let mut at = 0;
            for x in 0..W {
                for y in 0..usize::from(rows) {
                    let [a, b] = colour(x, y).to_be_bytes();
                    buf[at] = a;
                    buf[at + 1] = b;
                    at += 2;
                }
            }
            at
        })
        .is_ok();
    let mut scratch = [0u8; 1 + 3 * W];
    let mut good = ok_send;
    for y in 0..rows {
        match lcd.read_row(0, y, W as u16, &mut scratch) {
            Ok(rgb) => {
                for (x, px) in rgb.chunks_exact(3).enumerate() {
                    good &= crate::lcd::read_to_565(px) == colour(x, usize::from(y));
                }
            }
            Err(_) => good = false,
        }
    }
    // Leave time for nothing: the caller sends a full picture next.
    spin_us(10);
    good
}
