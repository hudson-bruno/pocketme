//! What a game sees of the pads and the touch screen, in ONE place.
//!
//! Four players: board pad `n` (0..[`PLAYERS`]) is game player `n`, each
//! read on its own, so a press on one pad never shows on another player.
//!
//! The pad's buttons become the 12 game buttons of the contract
//! (`game_api::button`) through [`GAME_BUTTONS`]. A and B are named by
//! their job: A is the button the console's own menus confirm with (see
//! [`crate::input::BUTTON_MAP`]), B the one they go back with. Change
//! both tables together.
//!
//! Not passed to games: Home (the console's pause menu), Capture, the
//! stick clicks, and the board's BOOT button (on the CYD it opens the
//! pause menu, because it is the only button the board has).

use board_api::{Motion, PadState, btn};
use game_api::{PLAYERS, STICK_MAX, TRIGGER_MAX, button};

/// Which pad buttons give which game button.
pub const GAME_BUTTONS: [(u32, u32); 12] = [
    (btn::UP, button::UP),
    (btn::DOWN, button::DOWN),
    (btn::LEFT, button::LEFT),
    (btn::RIGHT, button::RIGHT),
    // The bottom face button confirms, as in the console's menus.
    (btn::SOUTH, button::A),
    (btn::EAST, button::B),
    (btn::WEST, button::X),
    (btn::NORTH, button::Y),
    (btn::L, button::L),
    (btn::R, button::R),
    (btn::PLUS, button::START),
    (btn::MINUS, button::SELECT),
];

/// One pad, turned into the game's numbers, before the console decides
/// what the game may see (see `game::Run`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RawPad {
    /// Whether this pad is connected.
    pub pad: bool,
    /// Game [`button`] bits held now.
    pub buttons: u32,
    /// Game [`button`] bits the board noted going down since the last
    /// frame, even if already let go (a short press).
    pub pressed: u32,
    pub left: (i16, i16),
    pub right: (i16, i16),
    /// Left and right trigger, 0 to [`TRIGGER_MAX`].
    pub triggers: (u16, u16),
    /// The pad's motion, when it sends it now.
    pub motion: Option<Motion>,
}

/// All pads and the touch, turned into the game's numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Raw {
    /// Player `n` is board pad `n`.
    pub players: [RawPad; PLAYERS],
    /// The touch, in picture pixels.
    pub touch: Option<(i16, i16)>,
    /// Where a touch the board noted started since the last frame, in
    /// picture pixels, even if already over (a short tap).
    pub tap: Option<(i16, i16)>,
}

impl Raw {
    /// The same, without the presses and the tap the board noted (they
    /// belong to the console: the press that started the game or closed
    /// its pause menu).
    pub fn without_noted(self) -> Raw {
        let mut r = self;
        for p in &mut r.players {
            p.pressed = 0;
        }
        r.tap = None;
        r
    }
}

/// Reads one pad: its buttons now, the presses the board noted, its
/// motion. A pad that is not there reads as nothing at all.
pub fn read_pad(pad: &PadState, noted: u32, motion: Option<Motion>) -> RawPad {
    if !pad.connected {
        return RawPad::default();
    }
    RawPad {
        pad: true,
        buttons: game_buttons(pad.buttons),
        pressed: game_buttons(noted),
        left: (stick(pad.left.x), stick(pad.left.y)),
        right: (stick(pad.right.x), stick(pad.right.y)),
        triggers: (
            trigger(pad.lt, pad.buttons & btn::ZL != 0),
            trigger(pad.rt, pad.buttons & btn::ZR != 0),
        ),
        motion,
    }
}

/// Reads every pad (see [`read_pad`]) and the touch (now, and a noted
/// start; already in picture pixels).
pub fn read(
    pads: &[PadState; PLAYERS],
    noted: &[u32; PLAYERS],
    motion: &[Option<Motion>; PLAYERS],
    touch: Option<(u16, u16)>,
    tap: Option<(u16, u16)>,
) -> Raw {
    let mut r = Raw {
        touch: touch.map(|(x, y)| (x as i16, y as i16)),
        tap: tap.map(|(x, y)| (x as i16, y as i16)),
        ..Raw::default()
    };
    for (n, p) in r.players.iter_mut().enumerate() {
        *p = read_pad(&pads[n], noted[n], motion[n]);
    }
    r
}

/// Pad [`btn`] bits as game [`button`] bits, through [`GAME_BUTTONS`].
fn game_buttons(pad: u32) -> u32 {
    let mut buttons = 0;
    for &(bits, b) in &GAME_BUTTONS {
        if pad & bits != 0 {
            buttons |= b;
        }
    }
    buttons
}

/// -32768 would have no partner on the other side: the contract runs
/// from -32767 to +32767.
fn stick(v: i16) -> i16 {
    v.max(-STICK_MAX)
}

/// The board's 0..1023 scaled to the contract's 0..32767. A pad whose
/// trigger is only a button (ZL, ZR) reads fully pressed while it is held.
fn trigger(v: u16, button: bool) -> u16 {
    let scaled = u32::from(v.min(board_api::input::TRIGGER_MAX)) * u32::from(TRIGGER_MAX)
        / u32::from(board_api::input::TRIGGER_MAX);
    if button { TRIGGER_MAX } else { scaled as u16 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_api::Stick;

    fn pad(buttons: u32) -> PadState {
        PadState {
            connected: true,
            buttons,
            ..PadState::NONE
        }
    }

    #[test]
    fn every_game_button_has_one_pad_button() {
        let mut all = 0;
        for &(p, g) in &GAME_BUTTONS {
            assert_eq!(read_pad(&pad(p), 0, None).buttons, g);
            all |= g;
        }
        assert_eq!(all, button::ALL);
        // Home, Capture, the stick clicks: never.
        let kept = btn::HOME | btn::CAPTURE | btn::LSTICK | btn::RSTICK;
        assert_eq!(read_pad(&pad(kept), 0, None).buttons, 0);
    }

    #[test]
    fn sticks_and_triggers_use_the_contracts_range() {
        let p = PadState {
            left: Stick {
                x: -32768,
                y: 32767,
            },
            right: Stick { x: 5, y: -5 },
            lt: 1023,
            rt: 0,
            ..pad(btn::ZR)
        };
        let r = read_pad(&p, 0, None);
        assert_eq!(r.left, (-32767, 32767));
        assert_eq!(r.right, (5, -5));
        assert_eq!(r.triggers, (TRIGGER_MAX, TRIGGER_MAX));
        let half = read_pad(&PadState { lt: 512, ..pad(0) }, 0, None);
        assert_eq!(half.triggers, (16399, 0));
    }

    #[test]
    fn noted_presses_use_the_same_table() {
        let pads = [pad(0), PadState::NONE, pad(0), PadState::NONE];
        let noted = [btn::SOUTH | btn::HOME | btn::PLUS, 0, btn::EAST, 0];
        let r = read(&pads, &noted, &[None; 4], None, Some((7, 8)));
        assert_eq!(r.players[0].pressed, button::A | button::START);
        assert_eq!(r.players[0].buttons, 0);
        assert_eq!(r.players[2].pressed, button::B);
        assert_eq!(r.tap, Some((7, 8)));
        let quiet = r.without_noted();
        assert!(quiet.players.iter().all(|p| p.pressed == 0));
        assert_eq!(quiet.tap, None);
    }

    #[test]
    fn no_pad_means_no_buttons_but_touch_still_counts() {
        let p = PadState {
            buttons: btn::SOUTH,
            ..PadState::NONE
        };
        let r = read(
            &[p; 4],
            &[btn::SOUTH; 4],
            &[Some(Motion::default()); 4],
            Some((3, 4)),
            None,
        );
        assert_eq!(r.players, [RawPad::default(); 4]);
        assert_eq!(r.touch, Some((3, 4)));
    }

    #[test]
    fn each_pad_is_its_own_player() {
        let m = Motion {
            accel: [1, 2, 3],
            gyro: [0; 3],
        };
        let pads = [
            PadState::NONE,
            pad(btn::UP),
            PadState::NONE,
            PadState {
                left: Stick { x: 9, y: 0 },
                ..pad(btn::NORTH)
            },
        ];
        let r = read(
            &pads,
            &[0, 0, 0, btn::WEST],
            &[None, None, None, Some(m)],
            None,
            None,
        );
        assert!(!r.players[0].pad && r.players[1].pad && !r.players[2].pad && r.players[3].pad);
        assert_eq!(r.players[1].buttons, button::UP);
        assert_eq!(r.players[3].buttons, button::Y);
        assert_eq!(r.players[3].pressed, button::X);
        assert_eq!(r.players[3].left, (9, 0));
        assert_eq!(r.players[3].motion, Some(m));
        assert_eq!(r.players[1].motion, None);
    }
}
