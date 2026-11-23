//! `boardctl touch`: a scripted finger for the CYD firmware (measuring
//! only). The firmware reads it as plain text letters after `T`, up to
//! `;` (boards/cyd/src/app.rs):
//!
//! | text        | meaning                                           |
//! |-------------|---------------------------------------------------|
//! | `T<x>,<y>;` | finger down at (or moved to) panel pixel x, y     |
//! | `TU;`       | finger lifted                                     |
//! | `TN<n>;`    | add noise of up to n pixels to every reading      |
//!
//! The finger's readings go through the same filter and counters as the
//! chip's (boards/cyd/src/touchfilt.rs), so a script tests that code.
//!
//! A script has one step per line: the time in milliseconds from the
//! start, then what happens. `#` starts a comment.
//!
//! ```text
//! 0    down 280 120
//! 33   move 260 120
//! 66   move 240 120
//! 100  up
//! 0    noise 2
//! ```

use anyhow::{Result, bail};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// Down, or moved, to a panel pixel.
    At(u16, u16),
    Up,
    Noise(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub ms: u64,
    pub act: Act,
}

impl Act {
    /// The text the firmware reads.
    pub fn text(self) -> String {
        match self {
            Act::At(x, y) => format!("T{x},{y};"),
            Act::Up => "TU;".to_string(),
            Act::Noise(n) => format!("TN{n};"),
        }
    }
}

/// The panel's size (landscape), for checking points.
const W: u16 = 320;
const H: u16 = 240;

/// Reads a script; the steps come out sorted by time (a stable sort, so
/// steps at the same time keep their order).
pub fn parse(text: &str) -> Result<Vec<Step>> {
    let mut steps = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        let n = i + 1;
        let num = |s: &str| -> Result<u64> {
            s.parse()
                .map_err(|_| anyhow::anyhow!("line {n}: {s:?} is not a number"))
        };
        let ms = num(words[0])?;
        let act = match &words[1..] {
            ["down" | "move", x, y] => {
                let (x, y) = (num(x)?, num(y)?);
                if x >= u64::from(W) || y >= u64::from(H) {
                    bail!("line {n}: {x},{y} is off the {W}x{H} panel");
                }
                Act::At(x as u16, y as u16)
            }
            ["up"] => Act::Up,
            ["noise", px] => {
                let px = num(px)?;
                if px > 50 {
                    bail!("line {n}: noise is at most 50 pixels");
                }
                Act::Noise(px as u8)
            }
            _ => bail!("line {n}: want `MS down X Y`, `MS move X Y`, `MS up` or `MS noise N`"),
        };
        steps.push(Step { ms, act });
    }
    steps.sort_by_key(|s| s.ms);
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_script() {
        let steps =
            parse("# a drag\n0 noise 2\n0 down 280 120\n 33 move 260 120 # left\n\n100 up\n")
                .unwrap();
        assert_eq!(
            steps,
            vec![
                Step {
                    ms: 0,
                    act: Act::Noise(2)
                },
                Step {
                    ms: 0,
                    act: Act::At(280, 120)
                },
                Step {
                    ms: 33,
                    act: Act::At(260, 120)
                },
                Step {
                    ms: 100,
                    act: Act::Up
                },
            ]
        );
    }

    #[test]
    fn sorts_by_time_and_keeps_order_at_the_same_time() {
        let steps = parse("50 up\n0 down 1 2\n0 move 3 4\n").unwrap();
        let acts: Vec<Act> = steps.iter().map(|s| s.act).collect();
        assert_eq!(acts, vec![Act::At(1, 2), Act::At(3, 4), Act::Up]);
    }

    #[test]
    fn says_what_is_wrong() {
        assert!(
            parse("0 down 320 10")
                .unwrap_err()
                .to_string()
                .contains("off the")
        );
        assert!(
            parse("x up")
                .unwrap_err()
                .to_string()
                .contains("not a number")
        );
        assert!(parse("0 jump").unwrap_err().to_string().contains("want"));
        assert!(parse("0 noise 51").is_err());
    }

    #[test]
    fn text_for_the_firmware() {
        assert_eq!(Act::At(5, 239).text(), "T5,239;");
        assert_eq!(Act::Up.text(), "TU;");
        assert_eq!(Act::Noise(3).text(), "TN3;");
    }
}
