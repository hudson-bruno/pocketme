//! The new choice (2026-11-18): how to send one present's changed areas.
//!
//! Today's rule ([`crate::plan`]) sends every area as its own window and
//! looks for ONE refresh pass in which they all fit, one after the
//! other; if none fits, each window waits for its own pass. A turning
//! 3D view gives about ten wide, low areas; such a window is written much
//! faster than the refresh sweeps, so it may only start in a narrow span,
//! the windows seldom share a pass, and the present waits refresh after
//! refresh (MODEL: most of a 29 ms present is waiting).
//!
//! The new rule looks at a few ways to send the same picture and takes
//! the one the code's own timing model says is done SOONEST:
//!
//! - [`Shape::Windows`]: today's windows (in one pass if one fits, else
//!   each its own pass: today's fallback);
//! - [`Shape::Groups`]: windows whose columns overlap (or nearly touch,
//!   [`GROUP_GAP`]) joined into one taller box each, groups far apart in
//!   columns kept apart; only tried when it gives fewer windows than
//!   today and more than one;
//! - [`Shape::Box`]: one box around all of them.
//!
//! A box also sends pixels that did not change. That costs nothing on
//! the picture: they are sent with the values the panel already shows.
//! A taller window is written more slowly per column, closer to the
//! refresh's own speed, so it may start in a much wider span and it fits
//! one pass nearly always.
//!
//! Everything here runs BEFORE the timed part of a present: the firmware
//! decides, then plans the chosen windows once more from the refresh
//! position at that moment, then sends.

use crate::{Job, MAX_WAIT_REFRESHES, floor, plan};

/// An area in panel pixels: x, y, w, h (inside the panel).
pub type Area = (u16, u16, u16, u16);

/// Windows whose columns are at most this many panel columns apart are
/// joined into one group (0: only overlapping or touching ones).
pub const GROUP_GAP: u16 = 0;

/// How a present is sent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shape {
    /// The areas as they came, one window each (today).
    #[default]
    Windows,
    /// Areas that share columns joined into one box each.
    Groups,
    /// One box around everything.
    Box,
}

impl Shape {
    pub fn name(self) -> &'static str {
        match self {
            Shape::Windows => "windows",
            Shape::Groups => "groups",
            Shape::Box => "box",
        }
    }
}

/// What [`choose`] decided.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Choice {
    pub shape: Shape,
    /// Windows to send: for [`Shape::Windows`] the caller's own, else the
    /// first `n` of the `out` slice given to [`choose`].
    pub n: usize,
    /// The one pass they all fit in, at the moment of the choice (`None`:
    /// each window its own pass, today's fallback).
    pub pass: Option<f32>,
    /// When the last one is done, by the code's own model (virtual line).
    pub end: f32,
}

/// When `jobs`, sent one after the other from `v_now`, each waiting for
/// its start window in pass `k`, are done (the end of the last one's
/// `Job::lines`); `None` if one would start after its window closed.
/// The same arithmetic as [`plan`].
pub fn end_in_pass(jobs: &[Job], k: f32, v_now: f32, lines: f32) -> Option<f32> {
    let mut t = v_now;
    for j in jobs {
        let (lo, hi) = j.window(lines);
        let call = t.max(k * lines + lo - j.lead);
        if call + j.lead > k * lines + hi {
            return None;
        }
        t = call + j.lines;
    }
    Some(t)
}

/// [`plan`] and when that pass is done: the same pass `plan` gives.
pub fn plan_end(jobs: &[Job], v_now: f32, lines: f32) -> Option<(f32, f32)> {
    let k0 = floor(v_now / lines);
    for dk in [-1.0, 0.0, 1.0, 2.0] {
        let k = k0 + dk;
        if let Some(end) = end_in_pass(jobs, k, v_now, lines) {
            return Some((k, end));
        }
    }
    None
}

/// When today's fallback (each window its own earliest pass after the
/// one before, waiting at most [`MAX_WAIT_REFRESHES`]) is done, by the
/// code's own model.
pub fn fallback_end(jobs: &[Job], v_now: f32, lines: f32) -> f32 {
    let mut t = v_now;
    for j in jobs {
        if let Some(k) = plan(core::slice::from_ref(j), t, lines) {
            let (lo, _) = j.window(lines);
            let target = k * lines + lo - j.lead;
            t = t.max(target.min(t + MAX_WAIT_REFRESHES * lines));
        }
        t += j.lines;
    }
    t
}

/// The box around `jobs` (panel pixels; jobs are already widened to
/// whole picture pixels).
pub fn bounding_box(jobs: &[Job]) -> Area {
    let (mut x0, mut y0, mut x1, mut y1) = (u16::MAX, u16::MAX, 0u16, 0u16);
    for j in jobs {
        x0 = x0.min(j.x0);
        y0 = y0.min(j.y0);
        x1 = x1.max(j.x0 + j.w);
        y1 = y1.max(j.y0 + j.h);
    }
    if x1 <= x0 || y1 <= y0 {
        return (0, 0, 0, 0);
    }
    (x0, y0, x1 - x0, y1 - y0)
}

/// Joins `jobs` (sorted by first column) whose columns overlap or are at
/// most `gap` columns apart into one box each, left to right, into
/// `out`. Returns how many; stops at `out.len()`.
pub fn column_groups(jobs: &[Job], gap: u16, out: &mut [Area]) -> usize {
    let mut n = 0;
    let mut cur: Option<(u16, u16, u16, u16)> = None; // x0, y0, x1, y1
    for j in jobs {
        let (x0, y0, x1, y1) = (j.x0, j.y0, j.x0 + j.w, j.y0 + j.h);
        cur = match cur {
            Some((a0, b0, a1, b1)) if x0 <= a1.saturating_add(gap) => {
                Some((a0.min(x0), b0.min(y0), a1.max(x1), b1.max(y1)))
            }
            Some((a0, b0, a1, b1)) => {
                if n == out.len() {
                    return n;
                }
                out[n] = (a0, b0, a1 - a0, b1 - b0);
                n += 1;
                Some((x0, y0, x1, y1))
            }
            None => Some((x0, y0, x1, y1)),
        };
    }
    if let Some((a0, b0, a1, b1)) = cur
        && n < out.len()
    {
        out[n] = (a0, b0, a1 - a0, b1 - b0);
        n += 1;
    }
    n
}

/// Which ways [`choose_with`] may take (for comparing rules in the PC
/// model; the firmware uses [`Rules::FIRMWARE`] through [`choose`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rules {
    /// Try the column groups, joining windows at most `gap` columns apart.
    pub groups: bool,
    pub gap: u16,
    /// Try one box.
    pub boxed: bool,
    /// Take a box or groups only when today's windows find no one pass
    /// (instead of whenever it is done sooner).
    pub only_without_pass: bool,
    /// Line times added per window when comparing (a real window may
    /// cost more than the model says; 0 = trust the model).
    pub window_cost: f32,
}

impl Rules {
    /// Whichever of windows, groups and box is done soonest by the code's
    /// own model.
    pub const NEW: Rules = Rules {
        groups: true,
        gap: GROUP_GAP,
        boxed: true,
        only_without_pass: false,
        window_cost: 0.0,
    };
    /// The firmware's rule: whichever of windows and one box is done
    /// soonest. The column groups are left out: over LANTERN's recorded
    /// area lists every present's areas share columns, so the groups
    /// were always one box (MODEL, examples/score.rs: never chosen), and
    /// a group can only end where the box ends (a pass ends at about its
    /// rightmost column whatever is sent before it).
    pub const FIRMWARE: Rules = Rules {
        groups: false,
        ..Rules::NEW
    };
}

/// The firmware's rule ([`Rules::FIRMWARE`]): of today's windows (`jobs`,
/// sorted by first column) and one box, the way that is done soonest from
/// `v_now` by the code's own model. On a tie the one with fewer pixels
/// wins (in that order). `make` turns an area into a job (the firmware's
/// `Job::new` with the beam's numbers); `out` (at least `jobs.len()`
/// long) receives the windows of a group or box choice.
#[inline]
pub fn choose(
    jobs: &[Job],
    make: impl Fn(Area) -> Job,
    v_now: f32,
    lines: f32,
    out: &mut [Job],
) -> Choice {
    choose_with(jobs, make, v_now, lines, out, Rules::FIRMWARE)
}

/// [`choose`] with other [`Rules`]: of the windows, the column groups
/// (if `rules.groups`) and the box (if `rules.boxed`), the one done
/// soonest; on a tie the one with fewer pixels (in that order).
pub fn choose_with(
    jobs: &[Job],
    make: impl Fn(Area) -> Job,
    v_now: f32,
    lines: f32,
    out: &mut [Job],
    rules: Rules,
) -> Choice {
    let cost = |c: &Choice| c.end + c.n as f32 * rules.window_cost;
    let mut best = match plan_end(jobs, v_now, lines) {
        Some((k, end)) => Choice {
            shape: Shape::Windows,
            n: jobs.len(),
            pass: Some(k),
            end,
        },
        None => Choice {
            shape: Shape::Windows,
            n: jobs.len(),
            pass: None,
            end: fallback_end(jobs, v_now, lines),
        },
    };
    if jobs.len() < 2 || out.is_empty() || (rules.only_without_pass && best.pass.is_some()) {
        return best;
    }
    // Column groups, if they differ from both the windows and the box.
    if rules.groups {
        let mut areas = [(0u16, 0u16, 0u16, 0u16); crate::MAX_JOBS];
        let limit = out.len().min(areas.len());
        let m = column_groups(jobs, rules.gap, &mut areas[..limit]);
        if m > 1 && m < jobs.len() {
            for (o, &a) in out.iter_mut().zip(&areas[..m]) {
                *o = make(a);
            }
            if let Some((k, end)) = plan_end(&out[..m], v_now, lines) {
                let c = Choice {
                    shape: Shape::Groups,
                    n: m,
                    pass: Some(k),
                    end,
                };
                if cost(&c) < cost(&best) {
                    best = c;
                }
            }
        }
    }
    if rules.boxed {
        let bx = make(bounding_box(jobs));
        if let Some((k, end)) = plan_end(core::slice::from_ref(&bx), v_now, lines) {
            let c = Choice {
                shape: Shape::Box,
                n: 1,
                pass: Some(k),
                end,
            };
            if cost(&c) < cost(&best) {
                out[0] = bx;
                best = c;
            }
        }
    }
    best
}
