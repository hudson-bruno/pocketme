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

use board_api::{ByteOrder, Dir, PixelSource, Rect, Screen, SendReport};
use esp_hal::gpio::Output;

use crate::beam::{Beam, floor, spin_us};
use crate::lcd::{CPU_MHZ, H, Lcd, LcdError, MADCTL_COLS, STRIP_BYTES, W, cycles};

/// Most windows sent apart in one present; more are sent as one full
/// frame. console-video hands at most 16.
pub const MAX_JOBS: usize = 16;

/// Lines kept between the refresh and the writer (display-dma's
/// `COLS_GUARD_LINES`, reasoned there line by line: about 7 needed).
const GUARD_LINES: f32 = 8.0;
/// Time from the start of a window to its first pixel byte (window
/// commands, filling the first strip, starting the DMA), and after the
/// last byte. display-dma's guesses, checked on the board there.
const LEAD_US: f32 = 35.0;
const TAIL_US: f32 = 10.0;
/// The wire is this much slower than its clock says (gaps between
/// bytes and between strips), measured in display-dma.
const WIRE_SLOWDOWN: f32 = 1.015;
/// Longest wait for the right moment, in refreshes.
const MAX_WAIT_REFRESHES: f32 = 2.0;

/// First strip of a window at most this many bytes, the second at most
/// [`SECOND_STRIP_BYTES`], then whole strip buffers.
const FIRST_STRIP_BYTES: usize = 1024;
const SECOND_STRIP_BYTES: usize = 4096;

/// One window to send: panel columns `x0..x0 + w` (even), rows
/// `y0..y0 + h` (even), and its timing.
#[derive(Clone, Copy, Default)]
struct Job {
    x0: u16,
    w: u16,
    y0: u16,
    h: u16,
    /// First and last gate line.
    g0: f32,
    g1: f32,
    /// Line times the wire needs per gate line, and from the start of the
    /// window to its first pixel byte.
    per_gate: f32,
    lead: f32,
    /// Line times for the whole window.
    lines: f32,
}

impl Job {
    fn new(r: Rect, beam: &Beam, mhz: u32) -> Job {
        // Widen to whole picture pixels (2x2 panel pixels).
        let x0 = r.x & !1;
        let y0 = r.y & !1;
        let x1 = (r.x + r.w + 1).min(W as u16) & !1;
        let y1 = (r.y + r.h + 1).min(H as u16) & !1;
        let (w, h) = (x1 - x0, y1 - y0);
        let to_lines = CPU_MHZ as f32 / beam.line_cycles;
        let gate_us = f32::from(h) * 2.0 * 8.0 / mhz as f32 * WIRE_SLOWDOWN;
        let per_gate = gate_us * to_lines;
        let lead = LEAD_US * to_lines;
        Job {
            x0,
            w,
            y0,
            h,
            g0: beam.gate(x0),
            g1: beam.gate(x1.saturating_sub(1)),
            per_gate,
            lead,
            lines: lead + f32::from(w) * per_gate + TAIL_US * to_lines,
        }
    }

    /// The start window of this job in virtual lines of pass 0: the first
    /// pixel byte may go out at any v with lo < v < hi (add k * lines for
    /// pass k). Derived in display-dma/src/tear.rs (`cols_window`): the
    /// refresh must have left each column before the writer starts it,
    /// and the writer must be done with it before the next refresh comes.
    fn window(&self, lines: f32) -> (f32, f32) {
        let slope = (self.g1 - self.g0) * (1.0 - self.per_gate);
        let lo = self.g0 + 1.0 + GUARD_LINES + slope.max(0.0);
        let hi = lines + self.g0 - self.per_gate - GUARD_LINES + slope.min(0.0);
        (lo, hi)
    }
}

/// The refresh pass k such that every window, sent one after the other
/// from `v_now` (each waiting if needed), starts inside its start window
/// of pass k. Earliest first; `None` if none works.
fn plan(jobs: &[Job], v_now: f32, lines: f32) -> Option<f32> {
    let k0 = floor(v_now / lines);
    for dk in [-1.0, 0.0, 1.0, 2.0] {
        let k = k0 + dk;
        let mut t = v_now;
        let mut ok = true;
        for j in jobs {
            let (lo, hi) = j.window(lines);
            let call = t.max(k * lines + lo - j.lead);
            if call + j.lead > k * lines + hi {
                ok = false;
                break;
            }
            t = call + j.lines;
        }
        if ok {
            return Some(k);
        }
    }
    None
}

/// Did the refresh and the writer meet in a window, by the model? The
/// writer moves evenly from virtual line `vs` (first pixel byte) to `ve`
/// (last) over gate lines g0..=g1 (display-dma `cols_crossing`).
fn crossed(vs: f32, ve: f32, g0: f32, g1: f32, lines: f32) -> bool {
    let n = g1 - g0;
    let w = (ve - vs) / (n + 1.0);
    let k = floor(vs / lines);
    let mut j = k - 1.0;
    while j <= k + 4.0 {
        let d0 = vs - (j * lines + g0);
        let d1 = d0 + n * (w - 1.0);
        let (lo, hi) = (d0.min(d1), d0.max(d1));
        if lo < 1.0 && hi > -w {
            return true;
        }
        j += 1.0;
    }
    false
}

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
    /// Sum and worst of: time per present (all of it), the waiting part
    /// (anchor reads and waiting for the refresh), the sending part.
    pub total_us: u64,
    pub total_max: u32,
    pub wait_us: u64,
    pub send_us: u64,
    /// Bytes sent, and the time from first to last pixel byte (the wire's
    /// real speed).
    pub bytes: u64,
    pub wire_cycles: u64,
    /// CPU time filling strips.
    pub fill_cycles: u64,
    pub errors: u32,
}

impl Counters {
    /// All zero (for statics; same as `Default`).
    #[cfg(feature = "core2")]
    pub const ZERO: Counters = Counters {
        presents: 0,
        windows: 0,
        untimed: 0,
        no_anchor: 0,
        no_plan: 0,
        late: 0,
        crossed: 0,
        total_us: 0,
        total_max: 0,
        wait_us: 0,
        send_us: 0,
        bytes: 0,
        wire_cycles: 0,
        fill_cycles: 0,
        errors: 0,
    };
}

/// The board's screen.
pub struct CydScreen {
    pub lcd: Lcd,
    pub beam: Beam,
    backlight: Output<'static>,
    /// The palette, each colour twice in one word, high byte first on the
    /// wire (a doubled pixel), and the palette version it was made from.
    wire: [u32; 256],
    wire_version: Option<u32>,
    /// Strip size in use, at most [`STRIP_BYTES`] (the bench lowers it).
    pub strip_limit: usize,
    /// The tear-free timing; off only for the bench.
    pub timed: bool,
    pub counters: Counters,
    /// Cycle counter at the last present, to forget an old anchor.
    last_present: Option<u64>,
    /// Lab: the core that sent the last picture. Each core has its own
    /// cycle counter, so an anchor taken on one is no good on the other.
    last_core: u8,
}

impl CydScreen {
    pub fn new(lcd: Lcd, backlight: Output<'static>) -> Self {
        Self {
            lcd,
            beam: Beam::new(),
            backlight,
            wire: [0; 256],
            wire_version: None,
            strip_limit: STRIP_BYTES,
            timed: true,
            counters: Counters::default(),
            last_present: None,
            last_core: 0,
        }
    }

    /// Lab (`core2`): waits until the second core is done with the
    /// screen. Call before touching the screen outside `present`.
    pub fn idle(&mut self) {
        #[cfg(feature = "core2")]
        crate::core2::join();
    }

    /// The counters, also while the second core sends the pictures.
    pub fn counters_now(&self) -> Counters {
        #[cfg(feature = "core2")]
        if crate::core2::send_here() {
            return crate::core2::counters();
        }
        self.counters
    }

    /// Sets the counters to zero (after `idle`).
    pub fn reset_counters(&mut self) {
        self.idle();
        self.counters = Counters::default();
        #[cfg(feature = "core2")]
        crate::core2::counters_set(self.counters);
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

/// Lab (`core2-swap`): the pixel source for [`CydScreen::present_held`].
/// That path always has the plain picture and the wire palette, so
/// `run` is never asked; if it were, it gives black.
#[cfg(feature = "core2-swap")]
struct WireOnly;

#[cfg(feature = "core2-swap")]
impl PixelSource for WireOnly {
    fn run(&self, _x: u16, _y: u16, _dir: Dir, _order: ByteOrder, out: &mut [u16]) {
        out.fill(0);
    }
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

    fn present<S: PixelSource + ?Sized>(&mut self, src: &S, areas: &[Rect]) -> SendReport {
        if areas.is_empty() {
            return SendReport::default();
        }
        // Lab: the second core sends a copy of it, if asked to (see
        // core2.rs). With `core2-swap` there is no copy: the console hands
        // its picture over through `core2::SwapSender`, never through here.
        #[cfg(all(feature = "core2", not(feature = "core2-swap")))]
        if crate::core2::send_here()
            && let Some(report) = crate::core2::post_present(self, src, areas)
        {
            return report;
        }
        self.present_here(src, areas)
    }

    fn backlight(&mut self, level: u8) {
        // Lab (core2): the console calls this right after `present`. With
        // the pictures on the second core, that core may still be using
        // this screen, so wait for it first (found by the second reader:
        // without this, both cores held the screen at the same time).
        self.idle();
        // On or off only: the pin is not dimmed (yet).
        if level > 0 {
            self.backlight.set_high();
        } else {
            self.backlight.set_low();
        }
    }
}

impl CydScreen {
    /// Sends the changed areas, on the core this runs on.
    pub fn present_here<S: PixelSource + ?Sized>(&mut self, src: &S, areas: &[Rect]) -> SendReport {
        if areas.is_empty() {
            return SendReport::default();
        }
        let t_start = self.begin();

        // The fast path when the source is the plain 2x picture.
        let fast = src.indexed().filter(|ix| {
            ix.scale == 2
                && ix.left == 0
                && ix.top == 0
                && usize::from(ix.width) * 2 == W
                && usize::from(ix.height) * 2 == H
        });
        if let Some(ix) = fast {
            self.set_wire(ix.palette, ix.palette_version);
        }
        let pixels = fast.map(|ix| ix.pixels);
        self.present_from(src, pixels, areas, t_start)
    }

    /// Lab (`core2-swap`): sends the plain 160x120 picture `pixels` at 2x
    /// with the wire palette already set by [`CydScreen::set_wire`]. The
    /// second core sends the held picture this way: it never sees the
    /// console's palette (see core2.rs).
    #[cfg(feature = "core2-swap")]
    pub fn present_held(&mut self, pixels: &[u8], areas: &[Rect]) -> SendReport {
        if areas.is_empty() {
            return SendReport::default();
        }
        let t_start = self.begin();
        self.present_from(&WireOnly, Some(pixels), areas, t_start)
    }

    /// Makes the wire palette (each colour doubled, wire byte order) from
    /// `palette`, unless it was made from this version already.
    pub fn set_wire(&mut self, palette: &[u16; 256], version: u32) {
        if self.wire_version != Some(version) {
            for (out, &c) in self.wire.iter_mut().zip(palette.iter()) {
                let w = u32::from(c.to_be());
                *out = w | (w << 16);
            }
            self.wire_version = Some(version);
        }
    }

    /// The start of every present: forgets an anchor of the other core or
    /// an old one. Returns the cycle counter at the start.
    fn begin(&mut self) -> u32 {
        let core = esp_hal::system::Cpu::current() as u8;
        if core != self.last_core {
            self.beam.forget();
            self.last_core = core;
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
        t_start
    }

    /// The rest of a present: plan, wait for the refresh, send. `pixels`
    /// is the plain 2x picture (sent with the wire palette), else every
    /// pixel comes from `src.run`.
    fn present_from<S: PixelSource + ?Sized>(
        &mut self,
        src: &S,
        pixels: Option<&[u8]>,
        areas: &[Rect],
        t_start: u32,
    ) -> SendReport {
        // The windows, sorted by first column.
        let mhz = self.lcd.write_mhz();
        let mut jobs = [Job::default(); MAX_JOBS];
        let mut n = 0;
        let full = Rect::new(0, 0, W as u16, H as u16);
        if areas.len() > MAX_JOBS {
            jobs[0] = Job::new(full, &self.beam, mhz);
            n = 1;
        } else {
            for r in areas {
                let r = r.intersect(&full);
                if r.is_empty() {
                    continue;
                }
                jobs[n] = Job::new(r, &self.beam, mhz);
                n += 1;
            }
        }
        let jobs = &mut jobs[..n];
        jobs.sort_unstable_by_key(|j| j.x0);

        // When to start.
        // One refresh pass for all windows when they fit in one;
        // otherwise each window gets the earliest pass it fits in, after
        // the one before it (some windows then wait one more refresh).
        let lines = self.beam.lines;
        let mut anchored = false;
        let mut pass = None;
        if self.timed {
            anchored = self.beam.anchor(&mut self.lcd);
            if anchored {
                pass = plan(jobs, self.beam.v_at(cycles()), lines);
                if pass.is_none() {
                    self.counters.no_plan += 1;
                }
            } else {
                self.counters.no_anchor += 1;
            }
        }
        let max_wait = (MAX_WAIT_REFRESHES * lines * self.beam.line_cycles) as u32;

        let c = &mut self.counters;
        c.presents += 1;
        let mut report = SendReport::default();
        let mut wait = cycles().wrapping_sub(t_start);
        let mut send = 0u32;
        for job in jobs.iter() {
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
        let total = cycles().wrapping_sub(t_start);
        report.send_us = send / CPU_MHZ;
        report.wait_us = wait / CPU_MHZ;
        let c = &mut self.counters;
        c.total_us += u64::from(total / CPU_MHZ);
        c.total_max = c.total_max.max(total / CPU_MHZ);
        c.wait_us += u64::from(report.wait_us);
        c.send_us += u64::from(report.send_us);
        report
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
