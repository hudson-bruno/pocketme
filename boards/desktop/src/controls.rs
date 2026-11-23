//! What the person is doing with the controls: the keyboard and mouse in
//! the window, or the lines of a script with no window. Both go through
//! here, so they reach the board in exactly the same way.
//!
//! A short press is never lost. If a button goes down and up again
//! before the console's next step, the board notes the press, and the
//! console takes the note at its next step (`take_pressed` in
//! `board_api::input`). The same for a touch that starts and ends
//! between two steps (`take_tap`). The levels ("held now") always say
//! what is true now. (Before step 3 the board showed such a press as
//! held for one step instead; the board layer had no other way to say
//! it.)
//!
//! Four pads, one per player (see [`Controls`]): the main keys are
//! player 1's, a second key set (the number pad) player 2's, and scripts
//! can drive all four.
//!
//! The motion sensor has a stand-in, as a keyboard cannot be tilted.
//! Player 1's is on at the start, lying flat and still (the accelerometer then reads
//! one g straight up: x 0, y 0, z 4096). `M` (or a `motion off` script
//! line) switches it off, as a pad in a mode without motion. Holding `V`
//! and moving the mouse tilts the pad (or a `tilt X Y` line); the gyro
//! shows how fast the mouse moves (or a `spin X Y Z` line).

use board_api::input::TRIGGER_MAX;
use board_api::{Motion, PadState, Stick, TouchPoint, btn, key};

use crate::board::{DesktopBoard, PANEL_H, PANEL_W};
use crate::keymap::{Control, Side, Toward};

/// Fully pushed stick.
const STICK_MAX: i16 = 32767;

/// One g on the accelerometer (game_api::motion: 4096 per g).
pub const ONE_G: i16 = 4096;

/// Tilt per window pixel the mouse moves while `V` is held: 128 pixels
/// tip the pad all the way over (one g sideways).
const TILT_PER_PIXEL: i32 = 32;

/// Gyro per window pixel the mouse moves in one step while `V` is held.
const SPIN_PER_PIXEL: i32 = 64;

/// How many gamepads the simulator fills: one per player of the game
/// interface (`game_api::PLAYERS`).
pub const PADS: usize = 4;

/// One gamepad's controls (one player).
#[derive(Clone, Debug)]
pub struct PadControls {
    /// The gamepad, as it is now.
    pub pad: PadState,
    /// Whether the pad sends motion now.
    pub motion_on: bool,
    /// The accelerometer's x and y (z follows: the pad's tilt), in
    /// 4096 per g.
    pub tilt: (i16, i16),
    /// The gyroscope, 16 per degree per second.
    pub spin: [i16; 3],
    /// The gyro was set by the mouse: it goes back to 0 once a step saw it.
    spin_from_mouse: bool,
    /// Stick keys held down: bit (side * 4 + direction).
    stick_keys: u8,
    /// Presses not yet handed to the board.
    latched_buttons: u32,
}

impl PadControls {
    /// Nothing held; plugged in or not; motion on or off, lying flat.
    fn new(connected: bool, motion_on: bool) -> Self {
        PadControls {
            pad: PadState {
                connected,
                ..PadState::NONE
            },
            motion_on,
            tilt: (0, 0),
            spin: [0; 3],
            spin_from_mouse: false,
            stick_keys: 0,
            latched_buttons: 0,
        }
    }

    /// The motion the pad sends now, if it sends any.
    pub fn motion(&self) -> Option<Motion> {
        if !self.motion_on {
            return None;
        }
        let (x, y) = (i32::from(self.tilt.0), i32::from(self.tilt.1));
        let g = i32::from(ONE_G);
        let z = (g * g - x * x - y * y).max(0).isqrt();
        Some(Motion {
            accel: [self.tilt.0, self.tilt.1, z as i16],
            gyro: self.spin,
        })
    }

    fn set_trigger(&mut self, side: Side, value: u16) {
        match side {
            Side::Left => self.pad.lt = value,
            Side::Right => self.pad.rt = value,
        }
    }

    fn set_tilt(&mut self, x: i16, y: i16) {
        let (mut x, mut y) = (
            i32::from(x).clamp(-4096, 4096),
            i32::from(y).clamp(-4096, 4096),
        );
        let g = i32::from(ONE_G);
        let len = (x * x + y * y).isqrt();
        if len > g {
            x = x * g / len;
            y = y * g / len;
        }
        self.tilt = (x as i16, y as i16);
    }

    fn update_sticks(&mut self) {
        let axis = |minus: bool, plus: bool| match (minus, plus) {
            (true, false) => -STICK_MAX,
            (false, true) => STICK_MAX,
            _ => 0,
        };
        let held = |side, toward| self.stick_keys & stick_bit(side, toward) != 0;
        for side in [Side::Left, Side::Right] {
            let s = Stick {
                x: axis(held(side, Toward::Left), held(side, Toward::Right)),
                y: axis(held(side, Toward::Up), held(side, Toward::Down)),
            };
            match side {
                Side::Left => self.pad.left = s,
                Side::Right => self.pad.right = s,
            }
        }
    }

    /// Puts this pad into the board's pad `n`.
    fn apply(&mut self, board: &mut DesktopBoard, n: usize) {
        let motion = self.motion();
        let slot = board.pad.slot(n);
        if self.pad.connected {
            *slot.pressed |= self.latched_buttons;
            *slot.state = self.pad;
            *slot.motion = motion;
        } else {
            // A pad that is not there says nothing.
            *slot.state = PadState::NONE;
            *slot.motion = None;
        }
        self.latched_buttons = 0;
    }
}

/// The controls now, plus the short presses noted for the next step.
///
/// Four pads, one per player. Player 1 (`players[0]`) is the main
/// keyboard set and the mouse's tilt; it starts plugged in, with motion
/// on. Players 2 to 4 start unplugged, with motion off: the window's
/// player-2 keys plug pad 2 in at their first press, and scripts plug any
/// pad in with `pN pad on`. The methods without a player number act on
/// player 1, as before there were four.
#[derive(Clone, Debug)]
pub struct Controls {
    /// The four pads.
    pub players: [PadControls; PADS],
    /// The board's own buttons now (`board_api::key` bits).
    pub keys: u16,
    /// The finger now.
    pub touch: Option<TouchPoint>,
    /// `V` is held: mouse moves tilt player 1's pad.
    pub tilting: bool,
    /// Presses and a touch start not yet handed to the board.
    latched_keys: u16,
    latched_touch: Option<TouchPoint>,
    /// Something changed since the last step.
    changed: bool,
}

impl Default for Controls {
    fn default() -> Self {
        Self::new()
    }
}

impl Controls {
    /// Nothing held, nothing touched, the keyboard's first pad connected
    /// (the other three not).
    pub fn new() -> Self {
        Controls {
            players: [
                PadControls::new(true, true),
                PadControls::new(false, false),
                PadControls::new(false, false),
                PadControls::new(false, false),
            ],
            keys: 0,
            touch: None,
            tilting: false,
            latched_keys: 0,
            latched_touch: None,
            changed: false,
        }
    }

    /// Player 1's pad now (a short way for `players[0].pad`).
    pub fn pad(&self) -> &PadState {
        &self.players[0].pad
    }

    /// Whether player 1's pad sends motion now.
    pub fn motion_on(&self) -> bool {
        self.players[0].motion_on
    }

    /// A key or button of player 1 goes down.
    pub fn press(&mut self, c: Control) {
        self.press_by(0, c);
    }

    /// A key or button goes down, on player `n`'s pad (0 to 3). The
    /// board's BOOT button is nobody's pad: it is the same for every `n`.
    /// The mouse tilt is player 1's only.
    pub fn press_by(&mut self, n: usize, c: Control) {
        let p = &mut self.players[n];
        match c {
            Control::Button(b) => {
                p.pad.buttons |= b;
                p.latched_buttons |= b;
            }
            Control::Boot => {
                self.keys |= key::BOOT;
                self.latched_keys |= key::BOOT;
            }
            Control::Stick(side, toward) => {
                p.stick_keys |= stick_bit(side, toward);
                p.update_sticks();
            }
            Control::Trigger(side) => {
                let bit = trigger_bit(side);
                p.pad.buttons |= bit;
                p.latched_buttons |= bit;
                p.set_trigger(side, TRIGGER_MAX);
            }
            Control::MotionSwitch => p.motion_on = !p.motion_on,
            Control::Tilt => self.tilting |= n == 0,
        }
        self.changed = true;
    }

    /// A key or button of player 1 goes up.
    pub fn release(&mut self, c: Control) {
        self.release_by(0, c);
    }

    /// A key or button goes up, on player `n`'s pad.
    pub fn release_by(&mut self, n: usize, c: Control) {
        let p = &mut self.players[n];
        match c {
            Control::Button(b) => p.pad.buttons &= !b,
            Control::Boot => self.keys &= !key::BOOT,
            Control::Stick(side, toward) => {
                p.stick_keys &= !stick_bit(side, toward);
                p.update_sticks();
            }
            Control::Trigger(side) => {
                p.pad.buttons &= !trigger_bit(side);
                p.set_trigger(side, 0);
            }
            Control::MotionSwitch => {}
            Control::Tilt => {
                if n == 0 {
                    // Let go: the pad lies flat and still again.
                    self.tilting = false;
                    p.tilt = (0, 0);
                    p.spin = [0; 3];
                }
            }
        }
        self.changed = true;
    }

    /// Lets go of everything (the window lost the keyboard). Which pads
    /// are plugged in, whether they send motion, and the finger stay.
    pub fn release_all(&mut self) {
        let touch = self.touch;
        let plugged = self
            .players
            .each_ref()
            .map(|p| (p.pad.connected, p.motion_on));
        let latched = self.players.each_ref().map(|p| p.latched_buttons);
        let (lk, lt) = (self.latched_keys, self.latched_touch);
        *self = Controls::new();
        for (n, p) in self.players.iter_mut().enumerate() {
            (p.pad.connected, p.motion_on) = plugged[n];
            p.latched_buttons = latched[n];
        }
        self.touch = touch;
        (self.latched_keys, self.latched_touch) = (lk, lt);
        self.changed = true;
    }

    /// Sets player 1's stick to an exact position (scripts).
    pub fn set_stick(&mut self, side: Side, s: Stick) {
        self.set_stick_by(0, side, s);
    }

    /// Sets player `n`'s stick to an exact position (scripts).
    pub fn set_stick_by(&mut self, n: usize, side: Side, s: Stick) {
        let p = &mut self.players[n];
        match side {
            Side::Left => p.pad.left = s,
            Side::Right => p.pad.right = s,
        }
        self.changed = true;
    }

    /// Sets how far player 1's trigger is pulled, 0 to `TRIGGER_MAX`
    /// (scripts).
    pub fn set_trigger(&mut self, side: Side, value: u16) {
        self.set_trigger_by(0, side, value);
    }

    /// The same for player `n`.
    pub fn set_trigger_by(&mut self, n: usize, side: Side, value: u16) {
        self.players[n].set_trigger(side, value);
        self.changed = true;
    }

    /// Player 1's pad starts or stops sending motion (scripts; `M` in the
    /// window).
    pub fn set_motion(&mut self, on: bool) {
        self.set_motion_by(0, on);
    }

    /// The same for player `n`.
    pub fn set_motion_by(&mut self, n: usize, on: bool) {
        self.players[n].motion_on = on;
        self.changed = true;
    }

    /// Tilts player 1's pad: the accelerometer's x and y, each -4096 to
    /// 4096 (one g). Together they are kept within one g; z follows, so
    /// the three always add up to one g, as for a pad held still.
    pub fn set_tilt(&mut self, x: i16, y: i16) {
        self.set_tilt_by(0, x, y);
    }

    /// The same for player `n`.
    pub fn set_tilt_by(&mut self, n: usize, x: i16, y: i16) {
        self.players[n].set_tilt(x, y);
        self.changed = true;
    }

    /// Sets player 1's gyroscope, 16 per degree per second (scripts). It
    /// stays until changed.
    pub fn set_spin(&mut self, spin: [i16; 3]) {
        self.set_spin_by(0, spin);
    }

    /// The same for player `n`.
    pub fn set_spin_by(&mut self, n: usize, spin: [i16; 3]) {
        let p = &mut self.players[n];
        p.spin = spin;
        p.spin_from_mouse = false;
        self.changed = true;
    }

    /// The mouse moved by `dx`, `dy` window pixels while `V` is held: tip
    /// player 1's pad that way, as if your hand tipped it. Right tips the
    /// right side down (accelerometer x goes down); up tips the far side
    /// down (y goes down). The gyro shows the speed for one step.
    pub fn tilt_by(&mut self, dx: f32, dy: f32) {
        if !self.tilting {
            return;
        }
        let p = &mut self.players[0];
        let (dx, dy) = (dx.round() as i32, dy.round() as i32);
        let x = i32::from(p.tilt.0) - dx * TILT_PER_PIXEL;
        let y = i32::from(p.tilt.1) + dy * TILT_PER_PIXEL;
        p.set_tilt(x.clamp(-4096, 4096) as i16, y.clamp(-4096, 4096) as i16);
        let spin = |v: i32| (v * SPIN_PER_PIXEL).clamp(-32767, 32767) as i16;
        // Tipping right turns the pad round its y axis, tipping away round
        // its x axis.
        p.spin = [spin(-dy), spin(dx), 0];
        p.spin_from_mouse = true;
        self.changed = true;
    }

    /// The motion player 1's pad sends now, if it sends any.
    pub fn motion(&self) -> Option<Motion> {
        self.players[0].motion()
    }

    /// Plugs player 1's pad in or pulls it out.
    pub fn set_connected(&mut self, on: bool) {
        self.set_connected_by(0, on);
    }

    /// Plugs player `n`'s pad in or pulls it out.
    pub fn set_connected_by(&mut self, n: usize, on: bool) {
        self.players[n].pad.connected = on;
        self.changed = true;
    }

    /// The finger touches (or moves to) a panel pixel.
    pub fn touch(&mut self, at: TouchPoint) {
        let at = TouchPoint {
            x: at.x.min(PANEL_W - 1),
            y: at.y.min(PANEL_H - 1),
        };
        if self.touch.is_none() {
            self.latched_touch = Some(at);
        }
        self.touch = Some(at);
        self.changed = true;
    }

    /// The finger lifts.
    pub fn lift(&mut self) {
        self.touch = None;
        self.changed = true;
    }

    /// Whether anything changed since the last call. Clears the flag.
    pub fn take_changed(&mut self) -> bool {
        core::mem::take(&mut self.changed)
    }

    /// Puts what the console should see into the board: the controls now,
    /// and the presses and touch start noted since the last call. The
    /// notes move to the board, which keeps them until the console takes
    /// them; so a step that reads no input loses none.
    pub fn apply(&mut self, board: &mut DesktopBoard) {
        for (n, p) in self.players.iter_mut().enumerate() {
            p.apply(board, n);
        }
        board.keys.held = self.keys;
        board.keys.pressed |= self.latched_keys;
        board.touch.at = self.touch;
        if board.touch.tap.is_none() {
            board.touch.tap = self.latched_touch;
        }
        self.latched_keys = 0;
        self.latched_touch = None;
    }

    /// A step has run: the gyro moved by the mouse goes back to 0.
    pub fn stepped(&mut self) {
        for p in &mut self.players {
            if p.spin_from_mouse {
                p.spin = [0; 3];
                p.spin_from_mouse = false;
            }
        }
    }
}

/// A mouse position in the window (window pixels) as a panel pixel. The
/// window shows the panel `scale` times bigger. A position outside the
/// window (the mouse dragged out) sticks to the nearest edge.
pub fn window_to_panel(x: f32, y: f32, scale: u32) -> TouchPoint {
    let s = scale.max(1) as f32;
    let clamp = |v: f32, max: u16| (v / s).clamp(0.0, f32::from(max - 1)) as u16;
    TouchPoint {
        x: clamp(x, PANEL_W),
        y: clamp(y, PANEL_H),
    }
}

fn stick_bit(side: Side, toward: Toward) -> u8 {
    let s = match side {
        Side::Left => 0,
        Side::Right => 4,
    };
    let t = match toward {
        Toward::Up => 0,
        Toward::Down => 1,
        Toward::Left => 2,
        Toward::Right => 3,
    };
    1 << (s + t)
}

fn trigger_bit(side: Side) -> u32 {
    match side {
        Side::Left => btn::ZL,
        Side::Right => btn::ZR,
    }
}
