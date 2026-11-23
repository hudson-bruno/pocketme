//! Input mapping, in ONE place: pad buttons, the left stick, the board's
//! BOOT button and the touch screen all end up as the same few menu
//! actions ([`act`]).
//!
//! - A direction acts once when pressed. Held, it acts again after
//!   [`REPEAT_DELAY_US`], then every [`REPEAT_US`] ("key repeat").
//! - The stick counts as a direction once pushed past [`STICK_ON`], and
//!   stops once back under [`STICK_OFF`] (the "dead zone" in the middle
//!   is everything under that).
//! - Touches come in picture pixels (160x120), not panel pixels.
//! - A press or a tap so short that it was over before this frame looked
//!   still counts, once, when the board noted it ([`Noted`], from
//!   `take_pressed` and `take_tap` in `board_api::input`).
//! - HOME is kept for the console itself: it opens the console menu
//!   (see `pause`), also over a game.
//!
//! The touch screen gives, per frame (see [`Gesture`]):
//!
//! - `down`: a finger went down (where).
//! - `tap`: a finger went down and up again without moving
//!   [`DRAG_START`] pixels (where it went down), and was not held longer
//!   than [`TAP_MAX_US`]. It comes when the finger LIFTS, and a lift is
//!   believed [`TOUCH_GRACE`] frames late (one missing read is not a
//!   lift). A touch the board only noted (down and up inside one frame)
//!   gives `down` and `tap` at once, in the same frame.
//! - `drag`: the finger moved [`DRAG_START`] pixels or more; from then
//!   on, each frame says how far it moved since the last one. The
//!   largest distance decides: a drag that comes back to where it began
//!   is still a drag, never a tap.
//! - `release`: a drag ended, with the finger's speed.
//! - `touching`: a touch of the page's is going on (down, not taken).
//!
//! A page that uses `tap` or `drag` adds [`InputState::live`] to its
//! "moving" check, so the console draws every frame while a finger
//! drags, and comes back to see the finger lift.
//!
//! Which buttons a GAME will see is not decided yet (owner decision D4).
//! This is only the console's own menu mapping. To change which button
//! does what, edit [`BUTTON_MAP`].

use board_api::{PadState, TouchPoint, btn, key};
use console_video::View;

use crate::theme::{
    DRAG_START, REPEAT_DELAY_US, REPEAT_US, SPEED_MAX, SPEED_MIN_DT_US, STICK_OFF, STICK_ON,
    TAP_MAX_US, TOUCH_GRACE, TOUCH_STILL_US,
};

/// The menu actions, as bits of a `u8`.
pub mod act {
    pub const LEFT: u8 = 1 << 0;
    pub const RIGHT: u8 = 1 << 1;
    pub const UP: u8 = 1 << 2;
    pub const DOWN: u8 = 1 << 3;
    /// Choose / start ("A").
    pub const OK: u8 = 1 << 4;
    /// Go back ("B").
    pub const BACK: u8 = 1 << 5;
    /// The console's own Home button.
    pub const HOME: u8 = 1 << 6;
    /// The four directions together.
    pub const DIRS: u8 = LEFT | RIGHT | UP | DOWN;
}

/// Which pad buttons give which action. The confirm button is the
/// BOTTOM face button (A on an Xbox pad, B on a Nintendo pad), as the
/// positions are named in board-api; Start (+) confirms too.
pub const BUTTON_MAP: [(u32, u8); 7] = [
    (btn::LEFT, act::LEFT),
    (btn::RIGHT, act::RIGHT),
    (btn::UP, act::UP),
    (btn::DOWN, act::DOWN),
    (btn::SOUTH | btn::PLUS, act::OK),
    (btn::EAST, act::BACK),
    (btn::HOME, act::HOME),
];

/// The board's own BOOT button confirms (the CYD has no other button).
pub const BOOT_ACT: u8 = act::OK;

/// What the board noted since the last frame: presses and a touch that
/// started, even if they are already over (see `board_api::input`). All
/// zero on a board that notes nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Noted {
    /// Pad buttons that went down (`btn` bits). 0 when no pad is there.
    pub buttons: u32,
    /// The board's own buttons that went down (`key` bits).
    pub keys: u16,
    /// Where a touch started, in panel pixels.
    pub tap: Option<TouchPoint>,
}

impl Noted {
    /// Nothing noted.
    pub const NONE: Noted = Noted {
        buttons: 0,
        keys: 0,
        tap: None,
    };
}

/// The input of one frame, as the screens use it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    /// Menu actions this frame: presses, and repeats of a held direction.
    pub acts: u8,
    /// Menu actions held now.
    pub held: u8,
    /// Pad buttons that went down this frame (raw `btn` bits; the BOOT
    /// button counts as `btn::SOUTH`). "Any button" uses this.
    pub pressed: u32,
    /// Where the screen is touched now, in picture pixels.
    pub touch: Option<(u16, u16)>,
    /// A touch that started this frame, in picture pixels (also one the
    /// board only noted). Before step 4 this was called `tap`.
    pub down: Option<(u16, u16)>,
    /// A tap that ended this frame: the finger went down and up without
    /// becoming a drag. Where it went down, in picture pixels.
    pub tap: Option<(u16, u16)>,
    /// A drag going on this frame.
    pub drag: Option<Drag>,
    /// A drag that ended this frame.
    pub release: Option<Release>,
    /// A touch of the page's is going on (see [`Gesture::touching`]):
    /// false from the frame it lifted, or once the console took it.
    pub touching: bool,
    /// Whether a pad is connected.
    pub pad: bool,
}

impl Input {
    /// Whether anything was pressed or touched this frame.
    pub fn any(&self) -> bool {
        self.pressed != 0 || self.down.is_some() || self.tap.is_some()
    }
}

/// A drag going on. Picture pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drag {
    /// Where the finger went down.
    pub from: (u16, u16),
    /// How far it moved since the last frame. In the frame the drag
    /// starts: since it went down, so what is under the finger can
    /// follow it exactly. 0 in a frame the finger was missing (see
    /// [`TOUCH_GRACE`]).
    pub dx: i16,
    pub dy: i16,
}

/// A drag that ended: the finger lifted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Release {
    /// Where the finger went down.
    pub from: (u16, u16),
    /// The finger's speed when it lifted, picture pixels a second
    /// (positive: right, down). 0 when it had rested
    /// [`TOUCH_STILL_US`] before lifting.
    pub vx: i16,
    pub vy: i16,
}

/// Where a touch is in its life.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
enum Phase {
    /// No finger.
    #[default]
    Idle = 0,
    /// A finger is down and has not moved [`DRAG_START`] yet: lifted
    /// now, it is a tap.
    Press,
    /// It moved [`DRAG_START`] or more: a drag until it lifts.
    Drag,
    /// The console took it away (the page changed under it): ignored
    /// until the finger lifts.
    Spent,
}

/// The touch, turned into down, tap, drag and release (see the module
/// notes). Plain integer maths. 56 bytes.
///
/// Rules:
/// - A finger that is still down at the next frame is the same touch,
///   even when the board noted a new start in between (a short loss of
///   contact on a noisy panel).
/// - A finger down may be missing for [`TOUCH_GRACE`] frames in a row
///   and the touch goes on: a drag with no movement in those frames, a
///   press with nothing yet. One more missing frame is the lift: the
///   tap (or the release) comes then. A missing finger the board noted
///   touching again since the last frame is not missing.
/// - The same holds for a touch the console took away ([`Gesture::cancel`]):
///   it stays taken through a missing read.
/// - A press held longer than [`TAP_MAX_US`] gives no tap. It is timed
///   from the read that saw it go down to the first read that missed it
///   (the console may sleep under a resting finger, and wakes for the
///   lift).
/// - The speed is measured between frames at least
///   [`SPEED_MIN_DT_US`] apart, and averaged with the one before.
#[derive(Clone, Copy, Debug, Default)]
pub struct Gesture {
    phase: Phase,
    /// Frames in a row the finger was missing.
    lost: u8,
    /// Where the finger went down.
    from: (u16, u16),
    /// Where it was at the last frame.
    at: (u16, u16),
    /// Its speed, picture pixels a second.
    speed: (i16, i16),
    /// The last place and time used for the speed.
    sample: (u16, u16),
    sample_us: u64,
    /// When it last moved (or went down), and when it was last seen.
    moved_us: u64,
    seen_us: u64,
    /// When it went down, and whether it was held longer than
    /// [`TAP_MAX_US`] (then it is no tap).
    down_us: u64,
    long: bool,
}

/// What the touch did in one frame (the touch part of [`Input`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Touches {
    pub down: Option<(u16, u16)>,
    pub tap: Option<(u16, u16)>,
    pub drag: Option<Drag>,
    pub release: Option<Release>,
    pub touching: bool,
}

impl Gesture {
    pub const fn new() -> Self {
        Gesture {
            phase: Phase::Idle,
            lost: 0,
            from: (0, 0),
            at: (0, 0),
            speed: (0, 0),
            sample: (0, 0),
            sample_us: 0,
            moved_us: 0,
            seen_us: 0,
            down_us: 0,
            long: false,
        }
    }

    /// One frame. `touch`: where the finger is now; `noted`: where a
    /// touch started since the last frame, as the board noted it (it may
    /// be over). Both in picture pixels.
    pub fn update(
        &mut self,
        now_us: u64,
        touch: Option<(u16, u16)>,
        noted: Option<(u16, u16)>,
    ) -> Touches {
        let mut out = Touches::default();
        // A missing finger that the board saw touch again since the last
        // frame (a bounce on a noisy panel) is still the same touch.
        let lost_now = self.phase != Phase::Idle && touch.is_none();
        if lost_now && noted.is_some() {
            self.lost = 0;
            self.seen_us = now_us;
        } else if lost_now {
            self.lost = self.lost.saturating_add(1);
        }
        let grace = self.lost <= TOUCH_GRACE;
        // How long a press was held: up to this read while the finger is
        // there, and up to the first read that missed it (the console may
        // have slept under a resting finger, so the last read that saw it
        // can be long before the lift).
        if self.phase == Phase::Press
            && self.lost <= 1
            && now_us.saturating_sub(self.down_us) > TAP_MAX_US
        {
            self.long = true;
        }
        match (self.phase, touch) {
            (Phase::Idle, Some(p)) => {
                *self = Gesture {
                    phase: Phase::Press,
                    from: p,
                    at: p,
                    sample: p,
                    sample_us: now_us,
                    moved_us: now_us,
                    seen_us: now_us,
                    down_us: now_us,
                    ..Gesture::new()
                };
                out.down = Some(p);
            }
            (Phase::Idle, None) => {
                // Down and up again since the last frame: a tap (it was
                // over at once, so never held too long).
                out.down = noted;
                out.tap = noted;
            }
            (Phase::Press | Phase::Drag, Some(p)) => {
                self.lost = 0;
                self.track(now_us, p, &mut out);
            }
            (Phase::Press, None) if grace => {}
            (Phase::Press, None) => {
                if !self.long {
                    out.tap = Some(self.from);
                }
                self.phase = Phase::Idle;
            }
            (Phase::Drag, None) if grace => {
                out.drag = Some(Drag {
                    from: self.from,
                    dx: 0,
                    dy: 0,
                });
            }
            (Phase::Drag, None) => {
                let rested = self.seen_us - self.moved_us >= TOUCH_STILL_US;
                let (vx, vy) = if rested { (0, 0) } else { self.speed };
                out.release = Some(Release {
                    from: self.from,
                    vx,
                    vy,
                });
                self.phase = Phase::Idle;
            }
            (Phase::Spent, Some(_)) => self.lost = 0,
            (Phase::Spent, None) if grace => {}
            (Phase::Spent, None) => self.phase = Phase::Idle,
        }
        out.touching = self.touching();
        out
    }

    /// Whether a touch of the page's is going on: a finger is down (or
    /// missing for at most [`TOUCH_GRACE`] reads) and the console did not
    /// take it away. False in the frame it lifts.
    pub fn touching(&self) -> bool {
        matches!(self.phase, Phase::Press | Phase::Drag)
    }

    /// The finger is down at `p`: speed, and drag once far enough.
    fn track(&mut self, now_us: u64, p: (u16, u16), out: &mut Touches) {
        if p != self.at {
            self.moved_us = now_us;
        }
        let dt = now_us.saturating_sub(self.sample_us);
        if dt >= SPEED_MIN_DT_US {
            let dt = dt.min(i32::MAX as u64) as i64;
            let speed = |a: u16, b: u16| -> i32 {
                let v = (i64::from(a) - i64::from(b)) * 1_000_000 / dt;
                v.clamp(-i64::from(SPEED_MAX), i64::from(SPEED_MAX)) as i32
            };
            let now = (speed(p.0, self.sample.0), speed(p.1, self.sample.1));
            let (sx, sy) = if dt as u64 > TOUCH_STILL_US {
                // After a rest, the old speed is stale.
                now
            } else {
                (
                    (i32::from(self.speed.0) + now.0) / 2,
                    (i32::from(self.speed.1) + now.1) / 2,
                )
            };
            self.speed = (sx as i16, sy as i16);
            self.sample = p;
            self.sample_us = now_us;
        }
        let gap = |a: u16, b: u16| a as i16 - b as i16;
        match self.phase {
            Phase::Press => {
                let far = gap(p.0, self.from.0)
                    .unsigned_abs()
                    .max(gap(p.1, self.from.1).unsigned_abs());
                if far >= DRAG_START {
                    self.phase = Phase::Drag;
                    out.drag = Some(Drag {
                        from: self.from,
                        dx: gap(p.0, self.from.0),
                        dy: gap(p.1, self.from.1),
                    });
                }
            }
            _ => {
                out.drag = Some(Drag {
                    from: self.from,
                    dx: gap(p.0, self.at.0),
                    dy: gap(p.1, self.at.1),
                });
            }
        }
        self.at = p;
        self.seen_us = now_us;
    }

    /// True while the console should come back every frame for the
    /// touch: a finger down that moved in the last [`TOUCH_STILL_US`]
    /// (or just went down), or a finger missing in its grace (to see
    /// whether it comes back, and to give the tap or the release one
    /// frame later when it does not).
    pub fn live(&self, now_us: u64) -> bool {
        match self.phase {
            Phase::Press | Phase::Drag => {
                self.lost > 0 || now_us < self.moved_us.saturating_add(TOUCH_STILL_US)
            }
            Phase::Spent => self.lost > 0,
            Phase::Idle => false,
        }
    }

    /// Forgets the touch going on: no tap, drag or release comes from it.
    /// The next touch counts again once this finger is lifted.
    pub fn cancel(&mut self) {
        if self.phase != Phase::Idle {
            self.phase = Phase::Spent;
        }
    }

    /// Whether a drag is going on.
    pub fn dragging(&self) -> bool {
        self.phase == Phase::Drag
    }
}

/// What the last frames saw: to find presses, and for key repeat.
/// 72 bytes.
#[derive(Clone, Copy, Debug, Default)]
pub struct InputState {
    buttons: u32,
    held: u8,
    gesture: Gesture,
    /// The stick's direction on each axis: -1, 0 or +1.
    stick_x: i8,
    stick_y: i8,
    /// The direction being repeated (one `act` bit, or 0) and when it
    /// acts next.
    repeat: u8,
    repeat_at: u64,
}

impl InputState {
    pub const fn new() -> Self {
        InputState {
            buttons: 0,
            held: 0,
            gesture: Gesture::new(),
            stick_x: 0,
            stick_y: 0,
            repeat: 0,
            repeat_at: 0,
        }
    }

    /// When a held direction acts next, if one is held. The console asks
    /// to be called by then.
    pub fn repeat_due(&self) -> Option<u64> {
        (self.repeat != 0).then_some(self.repeat_at)
    }

    /// True while the touch needs a frame every frame (see
    /// [`Gesture::live`]). A page that uses taps or drags adds this to
    /// its "moving" check.
    pub fn live(&self, now_us: u64) -> bool {
        self.gesture.live(now_us)
    }

    /// Forgets the touch going on (see [`Gesture::cancel`]). The console
    /// calls it when the page changes, so a finger put down on one page
    /// does not tap or drag on the next.
    pub fn cancel_touch(&mut self) {
        self.gesture.cancel();
    }

    /// The touch, as tap and drag (read only).
    pub fn gesture(&self) -> &Gesture {
        &self.gesture
    }

    /// Reads one frame of raw input, at clock time `now_us`. `noted`:
    /// what the board noted since the last frame (short presses).
    pub fn update(
        &mut self,
        now_us: u64,
        touch: Option<TouchPoint>,
        keys: u16,
        pad: &PadState,
        noted: &Noted,
        view: &View,
    ) -> Input {
        let mut buttons = if pad.connected { pad.buttons } else { 0 };
        let mut held = map_buttons(buttons);
        if keys & key::BOOT != 0 {
            held |= BOOT_ACT;
            buttons |= btn::SOUTH;
        }
        // Presses the board noted: they count even when already let go.
        let mut noted_buttons = if pad.connected { noted.buttons } else { 0 };
        let mut noted_acts = map_buttons(noted_buttons);
        if noted.keys & key::BOOT != 0 {
            noted_acts |= BOOT_ACT;
            noted_buttons |= btn::SOUTH;
        }
        // The left stick, with a dead zone and a little "stickiness".
        let (sx, sy) = if pad.connected {
            (pad.left.x, pad.left.y)
        } else {
            (0, 0)
        };
        self.stick_x = stick_axis(self.stick_x, sx);
        self.stick_y = stick_axis(self.stick_y, sy);
        held |= match self.stick_x {
            -1 => act::LEFT,
            1 => act::RIGHT,
            _ => 0,
        };
        held |= match self.stick_y {
            -1 => act::UP,
            1 => act::DOWN,
            _ => 0,
        };

        let mut acts = (held & !self.held) | noted_acts;
        // Key repeat for the newest direction pressed (and still held).
        let new_dirs = acts & held & act::DIRS;
        if new_dirs != 0 {
            // Lowest bit only: one direction repeats at a time.
            self.repeat = 1 << new_dirs.trailing_zeros();
            self.repeat_at = now_us + REPEAT_DELAY_US;
        } else if self.repeat != 0 && held & self.repeat == 0 {
            self.repeat = 0;
        } else if self.repeat != 0 && now_us >= self.repeat_at {
            acts |= self.repeat;
            // Late frames give one repeat each, never a burst.
            self.repeat_at = (self.repeat_at + REPEAT_US).max(now_us + 1);
        }

        let touch_pic = touch.and_then(|t| view.to_picture(t.x, t.y));
        let noted_pic = noted.tap.and_then(|t| view.to_picture(t.x, t.y));
        let t = self.gesture.update(now_us, touch_pic, noted_pic);
        let input = Input {
            acts,
            held,
            pressed: (buttons & !self.buttons) | noted_buttons,
            touch: touch_pic,
            down: t.down,
            tap: t.tap,
            drag: t.drag,
            release: t.release,
            touching: t.touching,
            pad: pad.connected,
        };
        self.buttons = buttons;
        self.held = held;
        input
    }
}

/// The menu actions of some pad buttons, through [`BUTTON_MAP`].
fn map_buttons(buttons: u32) -> u8 {
    let mut acts = 0;
    for &(bits, a) in &BUTTON_MAP {
        if buttons & bits != 0 {
            acts |= a;
        }
    }
    acts
}

/// One stick axis: -1, 0 or +1, with the dead zone and stickiness.
fn stick_axis(was: i8, v: i16) -> i8 {
    let far = v.unsigned_abs();
    if far > STICK_ON as u16 {
        v.signum() as i8
    } else if far < STICK_OFF as u16 {
        0
    } else {
        was
    }
}
