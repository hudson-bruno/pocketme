//! Input scripts: a small text format that says what happens on which
//! frame, so a run with no window gives the same pictures every time.
//!
//! One event per line. A line starts with the frame number (the word
//! `at` before it is allowed), then what happens:
//!
//! ```text
//! # A comment. Empty lines are fine too.
//! at 10 press south          # hold a button down (names: see below)
//! at 12 release south        # let it go
//! at 20 touch 100 50         # finger down at x=100, y=50 (panel pixels, 320x240)
//! at 22 touch 80 50          # a finger already down moves (a drag)
//! at 25 lift                 # finger up
//! at 30 stick left 32767 0   # left stick fully right (x, y: -32767..32767, y down)
//! at 40 trigger right 1023   # right trigger fully pulled (0..1023)
//! at 50 pad off              # pull the pad out (`pad on` plugs it back)
//! at 52 motion off           # the pad stops sending motion (`motion on`)
//! at 54 tilt 2048 -1024      # accelerometer x and y (4096 = one g); z follows
//! at 56 spin 0 160 0         # gyroscope x, y, z (16 = one degree a second)
//! at 60 card out             # pull the SD card out (`card in` puts it back)
//! at 45 shot                 # save a picture of this frame
//! at 70 notify HELLO THERE    # a console notification (32 letters at most)
//! at 80 backlight 255        # check: the backlight is 255 after this frame
//! at 90 p2 pad on            # plug in player 2's pad (p1 to p4)
//! at 91 p2 press south       # player 2 holds a button down
//! ```
//!
//! Players: a pad line (`press`, `release`, `stick`, `trigger`, `pad`,
//! `motion`, `tilt`, `spin`) may name the player right after the frame
//! number, `p1` to `p4`. Without it the line is player 1's, as in
//! scripts written before there were four. Player 1's pad starts plugged
//! in with motion on; players 2 to 4 start unplugged with motion off, so
//! a script plugs them in with `pN pad on` first. `boot` is the board's
//! own button, nobody's pad: `p2 press boot` is a mistake, as is a
//! player on any line that is not a pad line.
//!
//! Button names: south east west north l r zl zr minus plus home capture
//! lstick rstick up down left right, and boot (the board's own button),
//! in any case. Everything stays as the script left it until a later
//! line changes it.
//!
//! A `press` and a `release` of the same button on the same frame is a
//! press shorter than a frame: when the console looks, the button is up
//! again, but the board noted the press, so the console (and a game)
//! sees it once, as pressed and released. The same for `touch` then
//! `lift` on one frame: a short tap.
//!
//! `notify` shows a notification, as the console's own news would (the
//! words after it, joined by one space; printable ASCII, 32 at most; a
//! `#` starts a comment, so it cannot be in the text). It waits behind
//! notifications already queued; a full queue is reported as a problem.
//!
//! `backlight N` (0 to 255) changes nothing: it checks that after this
//! frame the console's last backlight call asked for N. Anything else
//! (also no call yet) is reported as a problem, and the run fails.
//!
//! A frame here is one tick of the simulated clock: frame N starts at
//! N x 33,333 microseconds (at 30 ticks a second).

use board_api::input::TRIGGER_MAX;
use board_api::{Stick, TouchPoint};
use console_core::notify::NOTICE_LEN;

use crate::board::{PANEL_H, PANEL_W};
use crate::keymap::{self, Control, Side};

/// What one line of a script does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Press(Control),
    Release(Control),
    Touch(TouchPoint),
    Lift,
    Stick(Side, Stick),
    Trigger(Side, u16),
    /// Pad plugged in (true) or pulled out.
    Pad(bool),
    /// The pad sends motion (true) or not.
    Motion(bool),
    /// The pad's tilt: accelerometer x and y, -4096 to 4096.
    Tilt(i16, i16),
    /// The gyroscope x, y, z.
    Spin([i16; 3]),
    /// SD card in (true) or out.
    Card(bool),
    /// Save the picture of this frame.
    Shot,
    /// Show a console notification with this text.
    Notify(NoticeText),
    /// Check that the backlight is at this value after the frame.
    Backlight(u8),
}

/// The text of a `notify` line: printable ASCII, 1 to 32 letters. Kept
/// in place (no heap) so an [`Action`] stays a small plain value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoticeText {
    bytes: [u8; NOTICE_LEN],
    len: u8,
}

impl NoticeText {
    /// `None` when the text is empty, too long, or not printable ASCII.
    pub fn new(text: &str) -> Option<NoticeText> {
        let ok = |b: &u8| (b' '..=b'~').contains(b);
        if text.is_empty() || text.len() > NOTICE_LEN || !text.bytes().all(|b| ok(&b)) {
            return None;
        }
        let mut bytes = [0; NOTICE_LEN];
        bytes[..text.len()].copy_from_slice(text.as_bytes());
        Some(NoticeText {
            bytes,
            len: text.len() as u8,
        })
    }

    pub fn as_str(&self) -> &str {
        // Only printable ASCII goes in (see `new`).
        std::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("")
    }
}

/// One line of a script: on frame `at`, do `action`, on player
/// `player`'s pad (0 to 3; 0 also for lines that are no pad's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub at: u64,
    pub player: u8,
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
    let at = number::<u64>(words.next(), "a frame number")?;
    let player = match words.peek().and_then(|w| player(w)) {
        Some(p) => {
            words.next();
            Some(p?)
        }
        None => None,
    };
    let verb = words.next().ok_or("nothing after the frame number")?;
    let action = match verb {
        "press" => Action::Press(button(words.next())?),
        "release" => Action::Release(button(words.next())?),
        "touch" => {
            let x = number::<u16>(words.next(), "x (0..319)")?;
            let y = number::<u16>(words.next(), "y (0..239)")?;
            if x >= PANEL_W || y >= PANEL_H {
                return Err("the touch is outside the 320x240 panel".into());
            }
            Action::Touch(TouchPoint { x, y })
        }
        "lift" => Action::Lift,
        "stick" => {
            let side = side(words.next(), "stick")?;
            let mut axis = |what| {
                let v = number::<i16>(words.next(), what)?;
                if v == i16::MIN {
                    return Err(format!("expected {what}"));
                }
                Ok::<i16, String>(v)
            };
            let x = axis("x (-32767..32767)")?;
            let y = axis("y (-32767..32767)")?;
            Action::Stick(side, Stick { x, y })
        }
        "trigger" => {
            let side = side(words.next(), "trigger")?;
            let pull = number::<u16>(words.next(), "how far (0..1023)")?;
            if pull > TRIGGER_MAX {
                return Err("expected how far (0..1023)".into());
            }
            Action::Trigger(side, pull)
        }
        "pad" => Action::Pad(on_off(words.next(), "on", "off", "pad")?),
        "motion" => Action::Motion(on_off(words.next(), "on", "off", "motion")?),
        "tilt" => {
            let mut axis = |what| {
                let v = number::<i16>(words.next(), what)?;
                if !(-4096..=4096).contains(&v) {
                    return Err(format!("expected {what}"));
                }
                Ok::<i16, String>(v)
            };
            let x = axis("x (-4096..4096)")?;
            let y = axis("y (-4096..4096)")?;
            Action::Tilt(x, y)
        }
        "spin" => {
            let mut axis = |what| {
                let v = number::<i16>(words.next(), what)?;
                if v == i16::MIN {
                    return Err(format!("expected {what}"));
                }
                Ok::<i16, String>(v)
            };
            let x = axis("x (-32767..32767)")?;
            let y = axis("y (-32767..32767)")?;
            let z = axis("z (-32767..32767)")?;
            Action::Spin([x, y, z])
        }
        "card" => Action::Card(on_off(words.next(), "in", "out", "card")?),
        "shot" => Action::Shot,
        "backlight" => Action::Backlight(number::<u8>(words.next(), "a backlight value (0..255)")?),
        "notify" => {
            let text = words.by_ref().collect::<Vec<_>>().join(" ");
            let text = NoticeText::new(&text).ok_or_else(|| {
                format!("expected a text after `notify` (printable ASCII, 1 to {NOTICE_LEN})")
            })?;
            Action::Notify(text)
        }
        other => return Err(format!("unknown word `{other}`")),
    };
    if let Some(extra) = words.next() {
        return Err(format!("unexpected `{extra}` at the end"));
    }
    if player.is_some() {
        let pad_line = match action {
            Action::Press(c) | Action::Release(c) => c != Control::Boot,
            Action::Stick(..)
            | Action::Trigger(..)
            | Action::Pad(_)
            | Action::Motion(_)
            | Action::Tilt(..)
            | Action::Spin(_) => true,
            _ => false,
        };
        if !pad_line {
            return Err(format!("`{verb}` is no pad's, so it takes no player"));
        }
    }
    Ok(Event {
        at,
        player: player.unwrap_or(0),
        action,
    })
}

/// A player word: `None` when the word is not one (`p` and a digit);
/// an error for a player that does not exist.
fn player(word: &str) -> Option<Result<u8, String>> {
    let rest = word.strip_prefix('p').or_else(|| word.strip_prefix('P'))?;
    let n: u8 = rest.parse().ok()?;
    Some(match n {
        1..=4 => Ok(n - 1),
        _ => Err(format!("no player {n}: say p1, p2, p3 or p4")),
    })
}

fn number<T: std::str::FromStr>(word: Option<&str>, what: &str) -> Result<T, String> {
    word.and_then(|w| w.parse().ok())
        .ok_or_else(|| format!("expected {what}"))
}

fn button(word: Option<&str>) -> Result<Control, String> {
    let name = word.ok_or("expected a button name")?;
    keymap::for_button(name).ok_or_else(|| format!("unknown button `{name}`"))
}

fn side(word: Option<&str>, what: &str) -> Result<Side, String> {
    match word {
        Some("left") => Ok(Side::Left),
        Some("right") => Ok(Side::Right),
        _ => Err(format!("say `{what} left` or `{what} right`")),
    }
}

fn on_off(word: Option<&str>, yes: &str, no: &str, what: &str) -> Result<bool, String> {
    match word {
        Some(w) if w == yes => Ok(true),
        Some(w) if w == no => Ok(false),
        _ => Err(format!("say `{what} {yes}` or `{what} {no}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_api::btn;

    #[test]
    fn reads_every_kind_of_line() {
        let script = "
            # comment
            at 3 press plus
            4 release PLUS   # names in any case
            5 touch 319 239
            6 lift
            7 stick left -32767 32767
            8 stick right 0 -5
            9 trigger left 1023
            10 trigger right 1
            11 shot
            12 press boot
            13 pad off
            14 card out
            15 card in
            16 motion off
            17 tilt -4096 2048
            18 spin 1 -2 32767
            19 notify HELLO   THERE  # spaces between words become one
            20 p1 press south
            21 p4 pad on
            22 P2 stick right 5 6
            23 p3 trigger left 7
            24 p2 motion on
            25 p2 tilt 1 2
            26 p2 spin 3 4 5
            27 p2 release west
        ";
        let actions: Vec<_> = parse(script)
            .unwrap()
            .into_iter()
            .map(|e| (e.at, e.action))
            .collect();
        let plus = Control::Button(btn::PLUS);
        assert_eq!(
            actions,
            [
                (3, Action::Press(plus)),
                (4, Action::Release(plus)),
                (5, Action::Touch(TouchPoint { x: 319, y: 239 })),
                (6, Action::Lift),
                (
                    7,
                    Action::Stick(
                        Side::Left,
                        Stick {
                            x: -32767,
                            y: 32767
                        }
                    )
                ),
                (8, Action::Stick(Side::Right, Stick { x: 0, y: -5 })),
                (9, Action::Trigger(Side::Left, 1023)),
                (10, Action::Trigger(Side::Right, 1)),
                (11, Action::Shot),
                (12, Action::Press(Control::Boot)),
                (13, Action::Pad(false)),
                (14, Action::Card(false)),
                (15, Action::Card(true)),
                (16, Action::Motion(false)),
                (17, Action::Tilt(-4096, 2048)),
                (18, Action::Spin([1, -2, 32767])),
                (19, Action::Notify(NoticeText::new("HELLO THERE").unwrap())),
                (20, Action::Press(Control::Button(btn::SOUTH))),
                (21, Action::Pad(true)),
                (22, Action::Stick(Side::Right, Stick { x: 5, y: 6 })),
                (23, Action::Trigger(Side::Left, 7)),
                (24, Action::Motion(true)),
                (25, Action::Tilt(1, 2)),
                (26, Action::Spin([3, 4, 5])),
                (27, Action::Release(Control::Button(btn::WEST))),
            ]
        );
    }

    #[test]
    fn a_line_without_a_player_is_player_1s() {
        let events =
            parse("1 press south\n2 p1 press south\n3 p4 press south\n4 p2 pad off").unwrap();
        let players: Vec<_> = events.iter().map(|e| e.player).collect();
        assert_eq!(players, [0, 0, 3, 1]);
        // Lines that are no pad's stay player 1's (0).
        assert_eq!(parse("5 shot").unwrap()[0].player, 0);
    }

    #[test]
    fn sorts_by_frame_and_keeps_same_frame_order() {
        let events = parse("9 press south\n2 press east\n2 release east").unwrap();
        let frames: Vec<_> = events.iter().map(|e| e.at).collect();
        assert_eq!(frames, [2, 2, 9]);
        assert_eq!(events[0].action, Action::Press(Control::Button(btn::EAST)));
        assert_eq!(
            events[1].action,
            Action::Release(Control::Button(btn::EAST))
        );
    }

    #[test]
    fn mistakes_name_the_line() {
        for (bad, says) in [
            ("1 press turbo", "unknown button"),
            ("1 press", "expected a button name"),
            ("x press south", "frame number"),
            ("-1 press south", "frame number"),
            ("1", "nothing after"),
            ("1 touch 320 0", "outside"),
            ("1 touch 0 240", "outside"),
            ("1 touch 5", "expected y"),
            ("1 stick middle 0 0", "stick left"),
            ("1 stick left 32768 0", "expected x"),
            ("1 stick left -32768 0", "expected x"),
            ("1 trigger left 1024", "how far"),
            ("1 pad maybe", "pad on"),
            ("1 card", "card in"),
            ("1 motion", "motion on"),
            ("1 tilt 4097 0", "expected x"),
            ("1 tilt 0", "expected y"),
            ("1 spin 0 0", "expected z"),
            ("1 spin 0 -32768 0", "expected y"),
            ("1 jump", "unknown word"),
            ("1 lift now", "unexpected"),
            ("1 notify", "text after `notify`"),
            (
                "1 notify ABCDEFGHIJKLMNOPQRSTUVWXYZ 1234567",
                "text after `notify`",
            ),
            ("1 notify CAFÉ", "text after `notify`"),
            ("1 p5 press south", "no player 5"),
            ("1 p0 press south", "no player 0"),
            ("1 p2 press boot", "takes no player"),
            ("1 p2 touch 5 5", "takes no player"),
            ("1 p2 shot", "takes no player"),
            ("1 p2", "nothing after"),
        ] {
            let err = parse(&format!("\n{bad}")).unwrap_err();
            assert!(err.contains("line 2") && err.contains(says), "{bad}: {err}");
        }
    }
}
