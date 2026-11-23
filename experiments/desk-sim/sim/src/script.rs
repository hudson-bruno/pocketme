//! Input scripts: a small text format that says what happens on which
//! frame, so a run with no window gives the same pictures every time.
//!
//! One event per line. A line starts with the frame number (the word
//! `at` before it is allowed), then what happens:
//!
//! ```text
//! # A comment. Empty lines are fine too.
//! at 10 press start          # hold a button down (names: see below)
//! at 12 release start        # let it go
//! at 20 touch 100 50         # finger down at x=100, y=50 (screen pixels)
//! at 25 lift                 # finger up
//! at 30 stick left 127 0     # left stick fully right (x, y: -128..127)
//! at 40 trigger right 255    # right trigger fully pulled (0..255)
//! at 45 shot                 # save a picture of this frame
//! ```
//!
//! Button names: up down left right a b x y l r start select home.
//! Everything stays as the script left it until a later line changes it.

use board_layer::{Pad, Stick, Touch, button};

/// What one line of a script does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Press(u16),
    Release(u16),
    Touch(u16, u16),
    Lift,
    LeftStick(Stick),
    RightStick(Stick),
    LeftTrigger(u8),
    RightTrigger(u8),
    Shot,
}

/// One line of a script: on frame `at`, do `action`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub at: u32,
    pub action: Action,
}

/// Reads a whole script. The events come back sorted by frame; events on
/// the same frame keep their order. An error names the line.
pub fn parse(text: &str) -> Result<Vec<Event>, String> {
    let mut events = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let event = parse_line(line)
            .map_err(|why| format!("script line {}: {why}: `{line}`", number + 1))?;
        events.push(event);
    }
    // A stable sort: same-frame events stay in the order written.
    events.sort_by_key(|e| e.at);
    Ok(events)
}

fn parse_line(line: &str) -> Result<Event, String> {
    let mut words = line.split_whitespace().peekable();
    if words.peek() == Some(&"at") {
        words.next();
    }
    let at = number::<u32>(words.next(), "a frame number")?;
    let verb = words.next().ok_or("nothing after the frame number")?;
    let action = match verb {
        "press" => Action::Press(button_bit(words.next())?),
        "release" => Action::Release(button_bit(words.next())?),
        "touch" => {
            let x = number::<u16>(words.next(), "x (0..319)")?;
            let y = number::<u16>(words.next(), "y (0..239)")?;
            if x >= board_layer::SCREEN_W as u16 || y >= board_layer::SCREEN_H as u16 {
                return Err("the touch is outside the 320x240 screen".into());
            }
            Action::Touch(x, y)
        }
        "lift" => Action::Lift,
        "stick" => {
            let side = words.next();
            let stick = Stick {
                x: number::<i8>(words.next(), "x (-128..127)")?,
                y: number::<i8>(words.next(), "y (-128..127)")?,
            };
            match side {
                Some("left") => Action::LeftStick(stick),
                Some("right") => Action::RightStick(stick),
                _ => return Err("say `stick left` or `stick right`".into()),
            }
        }
        "trigger" => {
            let side = words.next();
            let pull = number::<u8>(words.next(), "how far (0..255)")?;
            match side {
                Some("left") => Action::LeftTrigger(pull),
                Some("right") => Action::RightTrigger(pull),
                _ => return Err("say `trigger left` or `trigger right`".into()),
            }
        }
        "shot" => Action::Shot,
        other => return Err(format!("unknown word `{other}`")),
    };
    if let Some(extra) = words.next() {
        return Err(format!("unexpected `{extra}` at the end"));
    }
    Ok(Event { at, action })
}

fn number<T: std::str::FromStr>(word: Option<&str>, what: &str) -> Result<T, String> {
    word.and_then(|w| w.parse().ok())
        .ok_or_else(|| format!("expected {what}"))
}

fn button_bit(word: Option<&str>) -> Result<u16, String> {
    let name = word.ok_or("expected a button name")?;
    button::from_name(name).ok_or_else(|| format!("unknown button `{name}`"))
}

/// Applies one action to the input state. Returns true for `shot`.
pub fn apply(action: Action, pad: &mut Pad, touch: &mut Touch) -> bool {
    match action {
        Action::Press(bit) => pad.buttons |= bit,
        Action::Release(bit) => pad.buttons &= !bit,
        Action::Touch(x, y) => {
            *touch = Touch {
                pressed: true,
                x,
                y,
            }
        }
        Action::Lift => touch.pressed = false,
        Action::LeftStick(s) => pad.left = s,
        Action::RightStick(s) => pad.right = s,
        Action::LeftTrigger(t) => pad.left_trigger = t,
        Action::RightTrigger(t) => pad.right_trigger = t,
        Action::Shot => return true,
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_kind_of_line() {
        let script = "
            # comment
            at 3 press start
            4 release START   # names in any case
            5 touch 319 239
            6 lift
            7 stick left -128 127
            8 stick right 0 -5
            9 trigger left 255
            10 trigger right 1
            11 shot
        ";
        let actions: Vec<_> = parse(script)
            .unwrap()
            .into_iter()
            .map(|e| (e.at, e.action))
            .collect();
        assert_eq!(
            actions,
            [
                (3, Action::Press(button::START)),
                (4, Action::Release(button::START)),
                (5, Action::Touch(319, 239)),
                (6, Action::Lift),
                (7, Action::LeftStick(Stick { x: -128, y: 127 })),
                (8, Action::RightStick(Stick { x: 0, y: -5 })),
                (9, Action::LeftTrigger(255)),
                (10, Action::RightTrigger(1)),
                (11, Action::Shot),
            ]
        );
    }

    #[test]
    fn sorts_by_frame_and_keeps_same_frame_order() {
        let events = parse("9 press a\n2 press b\n2 release b").unwrap();
        let frames: Vec<_> = events.iter().map(|e| e.at).collect();
        assert_eq!(frames, [2, 2, 9]);
        assert_eq!(events[0].action, Action::Press(button::B));
        assert_eq!(events[1].action, Action::Release(button::B));
    }

    #[test]
    fn mistakes_name_the_line() {
        for (bad, says) in [
            ("1 press turbo", "unknown button"),
            ("x press a", "frame number"),
            ("1 touch 320 0", "outside"),
            ("1 stick middle 0 0", "stick left"),
            ("1 trigger left 256", "how far"),
            ("1 jump", "unknown word"),
            ("1 lift now", "unexpected"),
        ] {
            let err = parse(&format!("\n{bad}")).unwrap_err();
            assert!(err.contains("line 2") && err.contains(says), "{bad}: {err}");
        }
    }
}
