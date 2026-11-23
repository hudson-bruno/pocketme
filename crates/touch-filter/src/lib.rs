//! Making a resistive touch panel's readings steady enough for taps and
//! dragging, and counting what the panel does (for tuning). Plain logic,
//! no pins: the CYD's chip readings and the debug link's scripted finger
//! both go through it, so a scripted drag tests the same code a finger
//! does, and the PC tests (tests/filter.rs) test it too.
//!
//! What it does to the readings, in panel pixels (320x240 on the CYD):
//!
//! - **Pressure with two limits** (hysteresis): a touch starts at
//!   pressure [`Z_ON`] or more, and goes on while it stays at [`Z_OFF`]
//!   or more (the chip read takes the limit from [`TouchFilter::z_limit`]).
//!   A finger resting near the limit does not flicker on and off.
//! - **Drop-outs held**: a reading with the panel's interrupt line still
//!   low but the pressure too light (a "weak" reading) inside a touch
//!   keeps the last point, up to [`HOLD_READS`] in a row. One more ends
//!   the touch. The line going high (nothing on the panel) ends it at
//!   once, so a lift or a flick is not delayed.
//! - **A dead band at rest**: while the finger rests, the point the
//!   console sees stays where it is until readings land more than
//!   [`DEAD_PX`] from the resting finger's average reading (the
//!   "anchor": the average of the readings since it came to rest, so a
//!   first reading that happened to be off does not leave the band
//!   lopsided). Then the point follows every reading ("moving") until
//!   [`REST_READS`] readings in a row stay within [`DEAD_PX`] of it; then
//!   it rests again. A resting finger shows no movement, so the console
//!   is not woken by its jitter, and a moving one has no lag.
//! - **A lone wild reading left out**: a resting finger starts moving
//!   only when [`CONFIRM_READS`] firm readings in a row are outside the
//!   band (weak readings in between do not count either way). One wild
//!   reading followed by one back in the band, or by the lift, is left
//!   out (counted as a "spike"): it never reaches the console, so a tap
//!   with one bad reading stays a tap. Once moving, every reading is
//!   followed at once, as before.
//!
//! Why these numbers: README.md, "Tuning" (the owner's finger test).

#![no_std]
#![forbid(unsafe_code)]

use board_api::TouchPoint;
use console_core::input::Gesture;
use console_video::View;

/// Pressure a touch must reach to start.
pub const Z_ON: u16 = 300;
/// Pressure a touch must keep to go on. Between [`Z_OFF`] and [`Z_ON`],
/// a touch going on stays and a new one does not start.
pub const Z_OFF: u16 = 180;
/// Weak readings in a row that keep the touch (holding its last point).
pub const HOLD_READS: u8 = 2;
/// The dead band at rest, panel pixels (Chebyshev distance: the larger
/// of the x and y gaps). One console picture pixel is 2 panel pixels;
/// the console's drag starts at 4 picture pixels (8 panel pixels).
pub const DEAD_PX: u16 = 3;
/// Readings in a row within [`DEAD_PX`] of the point that make a moving
/// finger rest again.
pub const REST_READS: u8 = 3;
/// Firm readings in a row outside the dead band that make a resting
/// finger move (1: the first one does, as before round 5). The cost: a
/// drag from rest starts one reading later (about 16 to 24 ms on the
/// CYD); after that there is no lag.
pub const CONFIRM_READS: u8 = 2;
/// A touch starting this soon after a lift, near where it lifted, is
/// counted as a "rebound" (contact lost and found again, or the same
/// spot touched again quickly).
pub const REBOUND_US: u64 = 100_000;
pub const REBOUND_PX: u16 = 16;

/// What one look at the panel found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    /// Nothing on the panel (the chip's interrupt line is high, or the
    /// scripted finger is up).
    None,
    /// The line is low but the pressure is under the limit, or fell
    /// during the read: a drop-out inside a touch.
    Weak,
    /// A point, with its pressure.
    Firm(TouchPoint, u16),
}

/// Sums and largest values since the start (or the letter `r`).
#[derive(Clone, Copy, Default, Debug)]
pub struct TouchCounters {
    /// Touches started (all sources), and those of the scripted finger.
    pub touches: u32,
    pub link_touches: u32,
    /// Readings taken while a touch went on.
    pub reads: u32,
    /// Weak readings inside a touch (held), and touches that ended
    /// because there were more than [`HOLD_READS`] of them in a row.
    pub dropouts: u32,
    pub dropout_ends: u32,
    /// Touches that started within [`REBOUND_US`] and [`REBOUND_PX`] of
    /// the last lift.
    pub rebounds: u32,
    /// Largest jump between two readings in a row of one touch, px.
    pub max_jump: u16,
    /// Resting: readings, the sum and the largest of their distance to
    /// the anchor (only those inside the dead band), px.
    pub rest_reads: u32,
    pub rest_jitter_sum: u32,
    pub rest_jitter_max: u16,
    /// A resting finger left the dead band; and of those, how many came
    /// back to rest within a few readings, close to where they left
    /// (jitter that got through the dead band, not a real move).
    pub breakouts: u32,
    pub false_moves: u32,
    /// Lone readings outside the band at rest, left out (see
    /// [`CONFIRM_READS`]).
    pub spikes: u32,
    /// As console-core's own `Gesture` sees the filtered points: taps,
    /// drags, and drags that ended with a flick (speed of `FLICK_MIN`
    /// or more).
    pub taps: u32,
    pub drags: u32,
    pub flicks: u32,
    /// The chip's interrupt line fell, but by the time the board looked
    /// the finger was gone (a touch with no reading: lost). Counted by
    /// the board.
    pub edges_unread: u32,
    /// Time of one chip read, microseconds: count, sum, largest.
    pub read_n: u32,
    pub read_us_sum: u64,
    pub read_us_max: u32,
}

impl TouchCounters {
    pub fn add_read_time(&mut self, us: u64) {
        self.read_n += 1;
        self.read_us_sum += us;
        self.read_us_max = self.read_us_max.max(us.min(u64::from(u32::MAX)) as u32);
    }
}

/// One touch, for its log line at the lift.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct TouchLife {
    pub start_us: u64,
    pub from: TouchPoint,
    pub reads: u32,
    pub dropouts: u32,
    pub max_jump: u16,
    /// Which reading of the touch (1 = the first) made `max_jump`: near
    /// `reads` means at the lift, 2 means at the landing.
    pub max_jump_at: u32,
    pub rest_jitter_max: u16,
    /// Lone wild readings left out (see [`CONFIRM_READS`]).
    pub spikes: u32,
    /// Farthest the reported point got from where the touch started, px.
    pub travel: u16,
    pub link: bool,
}

/// What a touch did, as the filter's own `Gesture` saw it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Tap,
    Drag,
    Flick,
    /// Neither: for example a press held too long for a tap.
    Other,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Tap => "tap",
            Kind::Drag => "drag",
            Kind::Flick => "flick",
            Kind::Other => "other",
        }
    }
}

/// What one reading changed.
#[derive(Clone, Copy, Default, Debug)]
pub struct Fed {
    /// A touch started at this reading (at the returned point).
    pub started: bool,
    /// With `started`: time since the last lift, if there was one (for
    /// telling a lost contact from a second touch).
    pub after_lift_us: Option<u64>,
    /// A touch ended at this reading: its life and kind.
    pub ended: Option<(TouchLife, Kind)>,
}

pub struct TouchFilter {
    /// A touch goes on.
    down: bool,
    /// The point the console sees.
    point: TouchPoint,
    /// The last firm reading.
    last_raw: TouchPoint,
    /// Moving: the point follows each reading. Else resting.
    moving: bool,
    /// Readings in a row near the point (while moving), and weak
    /// readings in a row (while down).
    still: u8,
    weak: u8,
    /// Firm readings in a row outside the band while resting.
    out: u8,
    /// The resting readings: their sums and count (the anchor is their
    /// average).
    rest_sum: (u32, u32),
    rest_n: u32,
    /// Where the finger rested before it last broke out, and readings
    /// since then.
    left_from: TouchPoint,
    since_break: u8,
    /// The last lift: when and where (for rebounds).
    lift_us: Option<u64>,
    lift_at: TouchPoint,
    life: TouchLife,
    /// console-core's own recogniser, fed with the filtered points (in
    /// picture pixels): taps, drags and flicks as the console would see
    /// them at this reading rate.
    gesture: Gesture,
    kind: Kind,
    view: View,
}

fn gap(a: u16, b: u16) -> u16 {
    a.abs_diff(b)
}

/// The larger of the x and y distances.
pub fn cheb(a: TouchPoint, b: TouchPoint) -> u16 {
    gap(a.x, b.x).max(gap(a.y, b.y))
}

const ORIGIN: TouchPoint = TouchPoint { x: 0, y: 0 };

impl TouchFilter {
    /// A filter for a panel of `panel_w` x `panel_h` pixels (for the
    /// console's picture pixels).
    pub const fn new(panel_w: u16, panel_h: u16) -> Self {
        Self {
            down: false,
            point: ORIGIN,
            last_raw: ORIGIN,
            moving: false,
            still: 0,
            weak: 0,
            out: 0,
            rest_sum: (0, 0),
            rest_n: 0,
            left_from: ORIGIN,
            since_break: 0,
            lift_us: None,
            lift_at: ORIGIN,
            life: TouchLife {
                start_us: 0,
                from: ORIGIN,
                reads: 0,
                dropouts: 0,
                max_jump: 0,
                max_jump_at: 0,
                rest_jitter_max: 0,
                spikes: 0,
                travel: 0,
                link: false,
            },
            gesture: Gesture::new(),
            kind: Kind::Other,
            view: View::fit(panel_w, panel_h),
        }
    }

    /// A touch goes on.
    pub fn down(&self) -> bool {
        self.down
    }

    /// The point the console sees now, `None` when no touch goes on.
    pub fn point(&self) -> Option<TouchPoint> {
        self.down.then_some(self.point)
    }

    /// The pressure limit for the next chip read (hysteresis).
    pub fn z_limit(&self) -> u16 {
        if self.down { Z_OFF } else { Z_ON }
    }

    /// One reading at clock time `now` (us). `link`: it came from the
    /// debug link's scripted finger.
    pub fn feed(&mut self, now: u64, r: Reading, link: bool, c: &mut TouchCounters) -> Fed {
        let mut fed = Fed::default();
        match (self.down, r) {
            (false, Reading::Firm(p, _)) => {
                fed.after_lift_us = self.lift_us.map(|t| now.saturating_sub(t));
                self.start(now, p, link, c);
                fed.started = true;
            }
            (false, Reading::None | Reading::Weak) => {}
            (true, Reading::Firm(p, _)) => {
                self.weak = 0;
                self.follow(p, c);
            }
            (true, Reading::Weak) => {
                self.weak += 1;
                if self.weak > HOLD_READS {
                    c.dropout_ends += 1;
                    fed.ended = Some(self.end(now, c));
                } else {
                    c.dropouts += 1;
                    self.life.dropouts += 1;
                    c.reads += 1;
                    self.life.reads += 1;
                }
            }
            (true, Reading::None) => fed.ended = Some(self.end(now, c)),
        }
        if self.down {
            self.see(now, c);
        }
        fed
    }

    fn start(&mut self, now: u64, p: TouchPoint, link: bool, c: &mut TouchCounters) {
        c.touches += 1;
        if link {
            c.link_touches += 1;
        }
        if let Some(t) = self.lift_us
            && now.saturating_sub(t) < REBOUND_US
            && cheb(p, self.lift_at) <= REBOUND_PX
        {
            c.rebounds += 1;
        }
        self.down = true;
        self.point = p;
        self.last_raw = p;
        self.moving = false;
        self.still = 0;
        self.weak = 0;
        self.out = 0;
        self.since_break = u8::MAX;
        self.rest_from(p);
        self.kind = Kind::Other;
        self.life = TouchLife {
            start_us: now,
            from: p,
            reads: 1,
            link,
            ..TouchLife::default()
        };
        c.reads += 1;
    }

    /// Starts resting at `p`: the anchor's average starts there.
    fn rest_from(&mut self, p: TouchPoint) {
        self.rest_sum = (u32::from(p.x), u32::from(p.y));
        self.rest_n = 1;
    }

    /// The resting finger's average reading.
    fn anchor(&self) -> TouchPoint {
        let n = self.rest_n.max(1);
        TouchPoint {
            x: ((self.rest_sum.0 + n / 2) / n) as u16,
            y: ((self.rest_sum.1 + n / 2) / n) as u16,
        }
    }

    /// Readings outside the band that never got a second one: left out.
    fn drop_spike(&mut self, c: &mut TouchCounters) {
        if self.out > 0 {
            self.out = 0;
            c.spikes += 1;
            self.life.spikes += 1;
        }
    }

    /// A firm reading during a touch.
    fn follow(&mut self, p: TouchPoint, c: &mut TouchCounters) {
        c.reads += 1;
        self.life.reads += 1;
        let jump = cheb(p, self.last_raw);
        c.max_jump = c.max_jump.max(jump);
        if jump > self.life.max_jump {
            self.life.max_jump = jump;
            self.life.max_jump_at = self.life.reads;
        }
        self.last_raw = p;
        self.since_break = self.since_break.saturating_add(1);
        if self.moving {
            let off = cheb(p, self.point);
            self.still = if off <= DEAD_PX { self.still + 1 } else { 0 };
            self.point = p;
            if self.still >= REST_READS {
                self.moving = false;
                self.rest_from(p);
                // Came back to rest soon, close to where it left: the
                // break-out was jitter.
                if self.since_break <= REST_READS + 2 && cheb(p, self.left_from) <= 2 * DEAD_PX {
                    c.false_moves += 1;
                }
            }
            self.life.travel = self.life.travel.max(cheb(self.point, self.life.from));
            return;
        }
        // Off the anchor's band, or (a very slow drift, which the
        // average follows) twice as far from the point the console has.
        let off = cheb(p, self.anchor());
        if off > DEAD_PX || cheb(p, self.point) > 2 * DEAD_PX {
            self.out = self.out.saturating_add(1);
            if self.out < CONFIRM_READS {
                // Wait for the next reading: one alone may be wild.
                return;
            }
            self.out = 0;
            c.breakouts += 1;
            self.left_from = self.point;
            self.since_break = 0;
            self.moving = true;
            self.still = 0;
            self.point = p;
        } else {
            // Back in the band: the reading before, if outside, was wild.
            self.drop_spike(c);
            // Long rests: keep the average of the last readings only
            // (halved now and then, so the sums cannot overflow).
            if self.rest_n >= 64 {
                self.rest_sum = (self.rest_sum.0 / 2, self.rest_sum.1 / 2);
                self.rest_n /= 2;
            }
            self.rest_sum.0 += u32::from(p.x);
            self.rest_sum.1 += u32::from(p.y);
            self.rest_n += 1;
            c.rest_reads += 1;
            c.rest_jitter_sum += u32::from(off);
            c.rest_jitter_max = c.rest_jitter_max.max(off);
            self.life.rest_jitter_max = self.life.rest_jitter_max.max(off);
        }
        self.life.travel = self.life.travel.max(cheb(self.point, self.life.from));
    }

    fn end(&mut self, now: u64, c: &mut TouchCounters) -> (TouchLife, Kind) {
        // A reading outside the band just before the lift: left out.
        self.drop_spike(c);
        self.down = false;
        self.lift_us = Some(now);
        self.lift_at = self.point;
        // The gesture ends a drag only after its grace (readings with no
        // finger): look that many times more.
        for _ in 0..=console_core::theme::TOUCH_GRACE {
            self.see(now, c);
        }
        (self.life, self.kind)
    }

    /// Feeds the filter's gesture with the point now.
    fn see(&mut self, now: u64, c: &mut TouchCounters) {
        let pic = self.point().and_then(|p| self.view.to_picture(p.x, p.y));
        let was_drag = self.gesture.dragging();
        let t = self.gesture.update(now, pic, None);
        if !was_drag && self.gesture.dragging() {
            c.drags += 1;
            self.kind = Kind::Drag;
        }
        if t.tap.is_some() {
            c.taps += 1;
            self.kind = Kind::Tap;
        }
        if let Some(r) = t.release {
            let speed = i32::from(r.vx).abs().max(i32::from(r.vy).abs());
            if speed >= console_core::theme::FLICK_MIN {
                c.flicks += 1;
                self.kind = Kind::Flick;
            }
        }
    }
}

impl Default for TouchFilter {
    /// The CYD's panel: 320 x 240.
    fn default() -> Self {
        Self::new(320, 240)
    }
}
