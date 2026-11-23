//! Which PC key stands for which button. ONE table, here, for the whole
//! simulator: change a key in [`KEYS`] and the window, `--help` and the
//! README table (made from it by a test) all follow.
//!
//! The keys in [`KEYS`] fill the first gamepad (player 1), plus the
//! board's own BOOT button, plus a stand-in for the pad's motion sensor
//! (a keyboard cannot be tilted): `M` switches motion on and off, and
//! holding `V` while moving the mouse tilts the pad. The keyboard gives
//! every button the board layer knows; the console decides what a game
//! sees of them.
//!
//! The number pad is a second, smaller gamepad: player 2 ([`PLAYER2_KEYS`]:
//! d-pad, face buttons, shoulders, triggers, start and select; no sticks,
//! home, capture or motion). Pad 2 counts as plugged in from its first
//! key press. Players 3 and 4 have no keys; scripts drive them.

use board_api::btn;

/// One thing a key (or a script line) can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// A gamepad button (`board_api::btn` bits).
    Button(u32),
    /// The board's own BOOT button (`board_api::key::BOOT`).
    Boot,
    /// Push a stick fully one way while held.
    Stick(Side, Toward),
    /// Pull a trigger fully while held (its button bit too).
    Trigger(Side),
    /// Each press switches the pad's motion sensor on or off (some pads
    /// send motion only in one of their modes).
    MotionSwitch,
    /// While held, moving the mouse tilts the pad; let go and it lies
    /// flat again.
    Tilt,
}

/// Left or right stick or trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// A stick direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toward {
    Up,
    Down,
    Left,
    Right,
}

/// The keyboard table: SDL key name, what it does, and the label shown
/// in `--help`. The key names are SDL's own names for keys by their
/// place on the keyboard ("scancodes"), so the layout of the letters
/// (QWERTY or not) does not matter.
pub const KEYS: &[(&str, Control, &str)] = &[
    ("Up", Control::Button(btn::UP), "d-pad up"),
    ("Down", Control::Button(btn::DOWN), "d-pad down"),
    ("Left", Control::Button(btn::LEFT), "d-pad left"),
    ("Right", Control::Button(btn::RIGHT), "d-pad right"),
    ("Z", Control::Button(btn::SOUTH), "bottom face button"),
    ("X", Control::Button(btn::EAST), "right face button"),
    ("A", Control::Button(btn::WEST), "left face button"),
    ("S", Control::Button(btn::NORTH), "top face button"),
    ("Q", Control::Button(btn::L), "left shoulder"),
    ("W", Control::Button(btn::R), "right shoulder"),
    ("1", Control::Trigger(Side::Left), "left trigger"),
    ("2", Control::Trigger(Side::Right), "right trigger"),
    ("Return", Control::Button(btn::PLUS), "start (+)"),
    ("Backspace", Control::Button(btn::MINUS), "select (-)"),
    ("Tab", Control::Button(btn::HOME), "home"),
    ("C", Control::Button(btn::CAPTURE), "capture"),
    ("I", Control::Stick(Side::Left, Toward::Up), "left stick up"),
    (
        "K",
        Control::Stick(Side::Left, Toward::Down),
        "left stick down",
    ),
    (
        "J",
        Control::Stick(Side::Left, Toward::Left),
        "left stick left",
    ),
    (
        "L",
        Control::Stick(Side::Left, Toward::Right),
        "left stick right",
    ),
    ("U", Control::Button(btn::LSTICK), "left stick pressed in"),
    (
        "T",
        Control::Stick(Side::Right, Toward::Up),
        "right stick up",
    ),
    (
        "G",
        Control::Stick(Side::Right, Toward::Down),
        "right stick down",
    ),
    (
        "F",
        Control::Stick(Side::Right, Toward::Left),
        "right stick left",
    ),
    (
        "H",
        Control::Stick(Side::Right, Toward::Right),
        "right stick right",
    ),
    ("Y", Control::Button(btn::RSTICK), "right stick pressed in"),
    ("B", Control::Boot, "the board's BOOT button"),
    (
        "M",
        Control::MotionSwitch,
        "motion sensor on or off (starts on)",
    ),
    (
        "V",
        Control::Tilt,
        "hold, and move the mouse to tilt the pad",
    ),
];

/// The second key set, on the number pad: player 2's gamepad. Same form
/// as [`KEYS`]. SDL names the number pad's keys "Keypad ..." whether Num
/// Lock is on or not.
pub const PLAYER2_KEYS: &[(&str, Control, &str)] = &[
    ("Keypad 8", Control::Button(btn::UP), "player 2 d-pad up"),
    (
        "Keypad 2",
        Control::Button(btn::DOWN),
        "player 2 d-pad down",
    ),
    (
        "Keypad 4",
        Control::Button(btn::LEFT),
        "player 2 d-pad left",
    ),
    (
        "Keypad 6",
        Control::Button(btn::RIGHT),
        "player 2 d-pad right",
    ),
    (
        "Keypad 0",
        Control::Button(btn::SOUTH),
        "player 2 bottom face button",
    ),
    (
        "Keypad .",
        Control::Button(btn::EAST),
        "player 2 right face button",
    ),
    (
        "Keypad 1",
        Control::Button(btn::WEST),
        "player 2 left face button",
    ),
    (
        "Keypad 3",
        Control::Button(btn::NORTH),
        "player 2 top face button",
    ),
    (
        "Keypad 7",
        Control::Button(btn::L),
        "player 2 left shoulder",
    ),
    (
        "Keypad 9",
        Control::Button(btn::R),
        "player 2 right shoulder",
    ),
    (
        "Keypad /",
        Control::Trigger(Side::Left),
        "player 2 left trigger",
    ),
    (
        "Keypad *",
        Control::Trigger(Side::Right),
        "player 2 right trigger",
    ),
    ("Keypad +", Control::Button(btn::PLUS), "player 2 start (+)"),
    (
        "Keypad -",
        Control::Button(btn::MINUS),
        "player 2 select (-)",
    ),
];

/// Keys for the simulator itself (window only). Never seen by the console.
pub const SIM_KEYS: &[(&str, &str)] = &[
    ("P", "pause / go on"),
    ("N", "one frame on (while paused)"),
    ("F1", "overlay: changed areas and bytes sent"),
    ("F12", "screenshot (PNG of the panel)"),
    ("Escape", "quit"),
];

/// Button names for scripts. The face buttons are named by position
/// (south = bottom), as in board-api, because "A" sits at the bottom on
/// one brand of pad and on the right on another.
pub const BUTTONS: &[(&str, Control)] = &[
    ("south", Control::Button(btn::SOUTH)),
    ("east", Control::Button(btn::EAST)),
    ("west", Control::Button(btn::WEST)),
    ("north", Control::Button(btn::NORTH)),
    ("l", Control::Button(btn::L)),
    ("r", Control::Button(btn::R)),
    ("zl", Control::Button(btn::ZL)),
    ("zr", Control::Button(btn::ZR)),
    ("minus", Control::Button(btn::MINUS)),
    ("plus", Control::Button(btn::PLUS)),
    ("home", Control::Button(btn::HOME)),
    ("capture", Control::Button(btn::CAPTURE)),
    ("lstick", Control::Button(btn::LSTICK)),
    ("rstick", Control::Button(btn::RSTICK)),
    ("up", Control::Button(btn::UP)),
    ("down", Control::Button(btn::DOWN)),
    ("left", Control::Button(btn::LEFT)),
    ("right", Control::Button(btn::RIGHT)),
    ("boot", Control::Boot),
];

/// What a key of player 1's set does, by its SDL name. `None`: not one
/// of player 1's keys.
pub fn for_key(name: &str) -> Option<Control> {
    KEYS.iter().find(|(k, _, _)| *k == name).map(|&(_, c, _)| c)
}

/// Whose key it is (0 = player 1, 1 = player 2) and what it does, by its
/// SDL name. `None`: the console ignores it.
pub fn for_any_key(name: &str) -> Option<(usize, Control)> {
    if let Some(c) = for_key(name) {
        return Some((0, c));
    }
    PLAYER2_KEYS
        .iter()
        .find(|(k, _, _)| *k == name)
        .map(|&(_, c, _)| (1, c))
}

/// A script's button name (any case), as a control.
pub fn for_button(name: &str) -> Option<Control> {
    let name = name.to_ascii_lowercase();
    BUTTONS.iter().find(|(n, _)| *n == name).map(|&(_, c)| c)
}

/// The key table as text, for `--help`.
pub fn help_table() -> String {
    let mut out = String::new();
    for (key, _, label) in KEYS {
        out.push_str(&format!("  {key:<10} {label}\n"));
    }
    out.push_str("  number pad: player 2 (plugged in at its first key)\n");
    for (key, _, label) in PLAYER2_KEYS {
        out.push_str(&format!("  {key:<10} {label}\n"));
    }
    for (key, label) in SIM_KEYS {
        out.push_str(&format!("  {key:<10} {label}\n"));
    }
    out.push_str("  mouse      touch (hold the left button)\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pad_button_has_a_key_and_a_name() {
        let mut by_key = 0;
        for (_, c, _) in KEYS {
            match c {
                Control::Button(b) => by_key |= b,
                Control::Trigger(Side::Left) => by_key |= btn::ZL,
                Control::Trigger(Side::Right) => by_key |= btn::ZR,
                _ => {}
            }
        }
        assert_eq!(by_key, btn::ALL, "a pad button no key reaches");
        let mut by_name = 0;
        for (_, c) in BUTTONS {
            if let Control::Button(b) = c {
                by_name |= b;
            }
        }
        assert_eq!(by_name, btn::ALL, "a pad button no script name reaches");
        assert!(KEYS.iter().any(|(_, c, _)| *c == Control::Boot));
        assert!(KEYS.iter().any(|(_, c, _)| *c == Control::MotionSwitch));
        assert!(KEYS.iter().any(|(_, c, _)| *c == Control::Tilt));
    }

    #[test]
    fn no_key_is_used_twice() {
        let mut names: Vec<&str> = KEYS.iter().map(|k| k.0).collect();
        names.extend(PLAYER2_KEYS.iter().map(|k| k.0));
        names.extend(SIM_KEYS.iter().map(|k| k.0));
        let count = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count);
    }

    #[test]
    fn names_in_any_case() {
        assert_eq!(for_button("SOUTH"), Some(Control::Button(btn::SOUTH)));
        assert_eq!(for_button("Boot"), Some(Control::Boot));
        assert_eq!(for_button("turbo"), None);
        assert_eq!(for_key("Z"), Some(Control::Button(btn::SOUTH)));
        assert_eq!(for_key("P"), None, "a simulator key, not the console's");
        assert_eq!(for_any_key("Z"), Some((0, Control::Button(btn::SOUTH))));
        assert_eq!(
            for_any_key("Keypad 0"),
            Some((1, Control::Button(btn::SOUTH)))
        );
        assert_eq!(for_key("Keypad 0"), None, "not player 1's");
        assert_eq!(for_any_key("P"), None);
    }

    #[test]
    fn player_2_has_the_buttons_a_game_reads() {
        let mut by_key = 0;
        for (_, c, _) in PLAYER2_KEYS {
            match c {
                Control::Button(b) => by_key |= b,
                Control::Trigger(Side::Left) => by_key |= btn::ZL,
                Control::Trigger(Side::Right) => by_key |= btn::ZR,
                Control::Stick(..) => {}
                other => panic!("{other:?} is player 1's only"),
            }
        }
        let wanted = btn::UP
            | btn::DOWN
            | btn::LEFT
            | btn::RIGHT
            | btn::SOUTH
            | btn::EAST
            | btn::WEST
            | btn::NORTH
            | btn::L
            | btn::R
            | btn::ZL
            | btn::ZR
            | btn::PLUS
            | btn::MINUS;
        assert_eq!(by_key, wanted);
    }
}
