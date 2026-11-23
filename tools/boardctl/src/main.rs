//! boardctl: the PC end of the console's debug link.
//!
//! It talks to the firmware over the USB serial cable: asks for a
//! screenshot and writes it as a PNG, presses buttons and taps the
//! screen without anyone touching the board, reads the firmware's
//! statistics, and prints the plain log.
//!
//! Made to be run by a script or an agent: every command does one thing,
//! needs no terminal, and ends by itself with exit code 0 when it worked.
//!
//! It does NOT take the port lock by itself. Where several programs
//! share the board, wrap each call:
//!
//! ```text
//! flock /path/to/board.lock timeout 150 boardctl stats
//! ```

mod link;
mod pad;
mod picture;
mod touch;

use std::{path::PathBuf, process::ExitCode, time::Duration};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use debug_link::msg::{Request, Source, button};

use crate::{
    link::{Answer, Link, status_name},
    picture::Picture,
};

#[derive(Parser)]
#[command(version, about = "Screenshots, injected input, stats and the log of the console, over the serial cable")]
struct Args {
    /// The board's serial port.
    #[arg(long, global = true, default_value = "/dev/ttyUSB0")]
    port: String,
    /// Also print (to stderr, marked "[board]") the board's log lines
    /// seen while waiting for answers, and for a moment after.
    #[arg(long, global = true)]
    show_log: bool,
    /// With --show-log: how long to go on listening after the command.
    #[arg(long, global = true, default_value_t = 300)]
    linger_ms: u64,
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum SourceArg {
    /// Read back from the display panel's own memory: what the glass shows.
    Panel,
    /// The firmware's framebuffer: what it meant to draw.
    Fb,
}

#[derive(Clone, Copy, ValueEnum)]
enum CardArg {
    Panel,
    Fb,
}

#[derive(Subcommand)]
enum Command {
    /// Checks that a firmware with the debug link answers.
    Ping {
        /// First send what does not belong on the line: log text, a
        /// made-up GPS sentence, and a frame that stops half way. The
        /// firmware must skip all of it and still answer.
        #[arg(long)]
        junk: bool,
    },
    /// Takes a screenshot and writes it as a PNG.
    Shot {
        #[arg(long, value_enum, default_value = "panel")]
        source: SourceArg,
        #[arg(short, long)]
        output: PathBuf,
        /// Serial speed for the picture. The line goes back to 115200
        /// afterwards.
        #[arg(long, default_value_t = 460_800)]
        baud: u32,
        /// Clock for reading the panel, in kHz. 0: the firmware's own.
        #[arg(long, default_value_t = 0)]
        read_khz: u16,
        /// How often to ask again when a picture arrives damaged.
        #[arg(long, default_value_t = 3)]
        tries: u32,
        /// Give up on one attempt after this many seconds.
        #[arg(long, default_value_t = 60)]
        limit: u64,
    },
    /// Presses buttons: holds them for a moment, then lets go.
    ///
    /// Names: up down left right a b x y l r start select.
    Key {
        #[arg(required = true)]
        names: Vec<String>,
        /// Whose pad: 1 to 4, as the game numbers its players (pads 2 to
        /// 4 need a firmware that knows four players).
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=4))]
        player: u8,
        /// How long they stay down, in milliseconds.
        #[arg(long, default_value_t = 100)]
        hold_ms: u64,
        /// Leave them down (send no release).
        #[arg(long)]
        keep: bool,
    },
    /// Sends letters as plain text, the way a person types them into a
    /// serial terminal. The firmware reads some single letters: n (next
    /// page), t (full button trace), f (forget every saved pairing).
    Text { letters: String },
    /// Lets go of every button (of player 1's pad, or `--player`'s).
    Release {
        /// Whose pad: 1 to 4.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=4))]
        player: u8,
    },
    /// Taps the screen at x, y (panel pixels, 0,0 is the top left).
    Tap { x: u16, y: u16 },
    /// A scripted finger (CYD firmware, measuring only): plays a script
    /// of `MS down X Y`, `MS move X Y`, `MS up`, `MS noise N` lines
    /// (milliseconds from the start, panel pixels) at its times. Ends
    /// with the finger lifted.
    Touch {
        /// The script file.
        script: PathBuf,
        /// Print the board's log while the script plays (and for
        /// `--after-ms` after it).
        #[arg(long)]
        log: bool,
        /// Letters to send when the script is over (as `text`), before
        /// the last log time: `i` prints the touch counters.
        #[arg(long)]
        then: Option<String>,
        /// How long to keep printing the log after the script, ms.
        #[arg(long, default_value_t = 500)]
        after_ms: u64,
    },
    /// Prints the firmware's statistics, one key=value per line.
    Stats,
    /// Plays a gamepad for the CYD firmware's gamepad host (features
    /// `pad` and `link`): injected reports, no radio. See src/pad.rs for
    /// the script format.
    Pad {
        #[command(subcommand)]
        what: PadCommand,
    },
    /// Prints the board's plain log for a while. Frames of the debug
    /// link are left out, and so are GPS sentences.
    Log {
        #[arg(long, default_value_t = 5.0)]
        seconds: f32,
        /// Restart the board first, to see its start-up lines.
        #[arg(long)]
        reset: bool,
        /// Letters to send first (as `text`), to see what they log.
        #[arg(long)]
        send: Option<String>,
    },
    /// Restarts the board (the only command that does, besides `log --reset`).
    Reset,
    /// Sends one command to the display panel and prints the bytes it
    /// answers. For finding out how a panel wants to be read.
    Raw {
        /// The command byte, in hex (2e, 04, ...).
        cmd: String,
        /// Parameter bytes, in hex.
        #[arg(long, num_args = 0..)]
        params: Vec<String>,
        /// Bits to clock and throw away before the answer.
        #[arg(long, default_value_t = 0)]
        dummy_bits: u8,
        /// How many bytes to read.
        #[arg(long, default_value_t = 4)]
        read: u16,
        /// Clock in kHz. 0: the firmware's own.
        #[arg(long, default_value_t = 0)]
        khz: u16,
    },
    /// Writes the test picture as it should look, without a board.
    Testcard {
        #[arg(long, value_enum, default_value = "panel")]
        kind: CardArg,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Compares two PNGs pixel by pixel, as the 16-bit colours the panel
    /// holds. Exit code 0: the same. 1: they differ.
    Compare {
        /// The picture that is right.
        expected: PathBuf,
        /// The picture to check.
        actual: PathBuf,
        /// Enlarge the first picture this many times before comparing.
        #[arg(long, default_value_t = 1)]
        scale_expected: usize,
        /// Enlarge the second picture this many times before comparing.
        #[arg(long, default_value_t = 1)]
        scale_actual: usize,
        /// Write a picture that marks the wrong pixels in red.
        #[arg(long)]
        diff: Option<PathBuf>,
        /// How many wrong pixels to list.
        #[arg(long, default_value_t = 20)]
        list: usize,
    },
}

#[derive(Subcommand)]
enum PadCommand {
    /// A pad connects in this mode: switch (default), xinput, android.
    Connect {
        #[arg(default_value = "switch")]
        mode: String,
    },
    /// The pad goes (reason 0x13: switched off).
    Off {
        #[arg(long, default_value = "13")]
        reason: String,
    },
    /// Holds buttons for a moment (two reports), names by position:
    /// south east west north l r zl zr minus plus home capture lstick
    /// rstick up down left right.
    Press {
        #[arg(required = true)]
        names: Vec<String>,
        #[arg(long, default_value_t = 100)]
        hold_ms: u64,
    },
    /// Down and up in two reports back to back: a press shorter than a
    /// frame.
    Tap {
        #[arg(required = true)]
        names: Vec<String>,
    },
    /// One report as raw hex bytes (without the 0xA1 byte).
    Report { hex: String },
    /// The board repeats the last report every MS ms, with a resting
    /// pad's small changes (0: stop).
    Repeat { ms: u16 },
    /// Plays a script (src/pad.rs).
    Script {
        script: PathBuf,
        /// Print the board's log while it plays (and `--after-ms` after).
        #[arg(long)]
        log: bool,
        /// Letters to send when the script is over (as `text`).
        #[arg(long)]
        then: Option<String>,
        #[arg(long, default_value_t = 300)]
        after_ms: u64,
    },
}

fn hex_bytes(text: &str) -> Result<Vec<u8>> {
    let digits: String = text.chars().filter(|c| !c.is_whitespace() && *c != ':').collect();
    if !digits.len().is_multiple_of(2) {
        bail!("{text:?}: an odd number of hex digits");
    }
    (0..digits.len()).step_by(2).map(|i| hex_byte(&digits[i..i + 2])).collect()
}

fn run_pad(link: &mut Link, what: PadCommand) -> Result<()> {
    use pad::{Held, Request as P, Step};
    let mut held = Held::default();
    let send = |link: &mut Link, step: &Step, held: &mut Held| -> Result<()> {
        for r in pad::requests(held, step) {
            link.ask_pad(&r.wire())?;
        }
        Ok(())
    };
    match what {
        PadCommand::Connect { mode } => {
            let m = debug_link::pad::mode::from_name(&mode).with_context(|| format!("{mode:?}: switch, xinput or android"))?;
            send(link, &Step::Connect(m), &mut held)?;
            println!("connected mode={mode}");
        }
        PadCommand::Off { reason } => {
            link.ask_pad(&P::Disconnect(hex_byte(&reason)?).wire())?;
            println!("off");
        }
        PadCommand::Press { names, hold_ms } => {
            let b = pad::buttons(&names)?;
            send(link, &Step::Press(b), &mut held)?;
            std::thread::sleep(Duration::from_millis(hold_ms));
            send(link, &Step::Release(b), &mut held)?;
            println!("pressed {names:?} for {hold_ms} ms");
        }
        PadCommand::Tap { names } => {
            send(link, &Step::Tap(pad::buttons(&names)?), &mut held)?;
            println!("tapped {names:?}");
        }
        PadCommand::Report { hex } => {
            let bytes = hex_bytes(&hex)?;
            link.ask_pad(&P::Report(bytes.clone()).wire())?;
            println!("sent {} bytes", bytes.len());
        }
        PadCommand::Repeat { ms } => {
            link.ask_pad(&P::Repeat(ms).wire())?;
            println!("repeat every {ms} ms");
        }
        PadCommand::Script { script, log, then, after_ms } => {
            let text = std::fs::read_to_string(&script).with_context(|| format!("reading {}", script.display()))?;
            let steps = pad::parse(&text)?;
            let wait = |link: &mut Link, time: Duration| -> Result<()> {
                if log && !time.is_zero() {
                    link.log(time, |line| println!("{line}"))
                } else {
                    std::thread::sleep(time);
                    Ok(())
                }
            };
            let start = std::time::Instant::now();
            let mut late_ms = 0u64;
            for (ms, step) in &steps {
                let at = start + Duration::from_millis(*ms);
                let now = std::time::Instant::now();
                if at > now {
                    wait(link, at - now)?;
                } else {
                    late_ms = late_ms.max((now - at).as_millis() as u64);
                }
                send(link, step, &mut held)?;
            }
            if let Some(letters) = then {
                wait(link, Duration::from_millis(100))?;
                link.send_raw(letters.as_bytes())?;
            }
            wait(link, Duration::from_millis(after_ms))?;
            println!("played {} steps over {} ms (latest step {late_ms} ms late)", steps.len(), steps.last().map_or(0, |s| s.0));
        }
    }
    Ok(())
}

fn hex_byte(text: &str) -> Result<u8> {
    let digits = text.trim_start_matches("0x").trim_end_matches('h');
    u8::from_str_radix(digits, 16).with_context(|| format!("{text:?} is not a hex byte"))
}

fn button_mask(names: &[String]) -> Result<u16> {
    let mut mask = 0;
    for name in names.iter().flat_map(|names| names.split(['+', ','])) {
        let Some(bit) = button::from_name(&name.to_ascii_lowercase()) else {
            let known: Vec<&str> = button::NAMES.iter().map(|(name, _)| *name).collect();
            bail!("no button is called {name:?}. Known: {}", known.join(" "));
        };
        mask |= bit;
    }
    Ok(mask)
}

fn run(args: Args) -> Result<ExitCode> {
    let open = || -> Result<Link> {
        let mut link = Link::open(&args.port)?;
        link.show_log = args.show_log;
        link.linger = Duration::from_millis(args.linger_ms);
        Ok(link)
    };

    match args.command {
        Command::Ping { junk } => {
            let mut link = open()?;
            if junk {
                // The sentence has the shape of a real one and zeros for
                // a position. The last four bytes start a frame that
                // never continues.
                link.send_raw(b"INFO - not for you\r\n$GPGGA,000000.00,0000.000,N,00000.000,E,0,00,99.9,0.0,M,0.0,M,,*00\r\n\xFE\xDB\x01\x07")?;
            }
            let started = std::time::Instant::now();
            link.ask_ok(&Request::Ping)?;
            println!("the board answered in {} ms", started.elapsed().as_millis());
        }

        Command::Shot { source, output, baud, read_khz, tries, limit } => {
            let source = match source {
                SourceArg::Panel => Source::Panel,
                SourceArg::Fb => Source::Framebuffer,
            };
            let mut link = open()?;
            let report = link.shot(source, baud, read_khz, tries, Duration::from_secs(limit))?;
            picture::save_image(&report.image, &output)?;
            let image = &report.image;
            let raw = image.pixels.len() + image.palette.as_ref().map_or(0, |palette| palette.len() * 2);
            println!("file={}", output.display());
            println!("source={source:?}");
            println!("size={}x{}", image.width, image.height);
            println!("format={:?}", image.format);
            println!("baud={}", report.baud);
            println!("total_ms={}", report.took.as_millis());
            println!("board_capture_ms={}", image.capture_ms);
            println!("wire_bytes={}", report.wire_bytes);
            println!("raw_bytes={raw}");
            println!("attempts={}", report.attempts);
            println!("corrupt_frames={}", link.corrupt);
        }

        Command::Key { names, player, hold_ms, keep } => {
            let mask = button_mask(&names)?;
            let mut link = open()?;
            link.set_buttons(player - 1, mask)?;
            println!("pressed mask=0x{mask:04x} player={player}");
            if !keep {
                std::thread::sleep(Duration::from_millis(hold_ms));
                link.set_buttons(player - 1, 0)?;
                println!("released after {hold_ms} ms");
            }
        }

        Command::Text { letters } => {
            let mut link = open()?;
            link.send_raw(letters.as_bytes())?;
            // Give the port time to send before it is closed.
            std::thread::sleep(Duration::from_millis(200));
            println!("sent {:?}", letters);
        }

        Command::Release { player } => {
            open()?.set_buttons(player - 1, 0)?;
            println!("released player={player}");
        }

        Command::Touch { script, log, then, after_ms } => {
            let text = std::fs::read_to_string(&script)
                .with_context(|| format!("reading {}", script.display()))?;
            let steps = touch::parse(&text)?;
            let mut link = open()?;
            link.show_log = false;
            // Waits until `at`, printing the log meanwhile if asked.
            let wait = |link: &mut Link, time: Duration| -> Result<()> {
                if log && !time.is_zero() {
                    link.log(time, |line| println!("{line}"))
                } else {
                    std::thread::sleep(time);
                    Ok(())
                }
            };
            let start = std::time::Instant::now();
            let mut late_ms = 0u64;
            for step in &steps {
                let at = start + Duration::from_millis(step.ms);
                let now = std::time::Instant::now();
                if at > now {
                    wait(&mut link, at - now)?;
                } else {
                    late_ms = late_ms.max((now - at).as_millis() as u64);
                }
                link.send_raw(step.act.text().as_bytes())?;
            }
            if steps.last().map(|s| s.act) != Some(touch::Act::Up) {
                link.send_raw(touch::Act::Up.text().as_bytes())?;
            }
            if let Some(letters) = then {
                wait(&mut link, Duration::from_millis(100))?;
                link.send_raw(letters.as_bytes())?;
            }
            wait(&mut link, Duration::from_millis(after_ms.max(200)))?;
            println!(
                "played {} steps over {} ms (latest step {} ms late)",
                steps.len(),
                steps.last().map_or(0, |s| s.ms),
                late_ms
            );
        }

        Command::Tap { x, y } => {
            open()?.ask_ok(&Request::Tap { x, y })?;
            println!("tapped x={x} y={y}");
        }

        Command::Pad { what } => {
            let mut link = open()?;
            run_pad(&mut link, what)?;
        }

        Command::Stats => match open()?.ask(&Request::Stats)? {
            Answer::Stats(text) => print!("{text}"),
            Answer::Ack { status, .. } => bail!("the board refused: {}", status_name(status)),
            other => bail!("unexpected answer: {other:?}"),
        },

        Command::Log { seconds, reset, send } => {
            let mut link = open()?;
            link.show_log = false;
            if reset {
                link.reset()?;
            }
            if let Some(letters) = send {
                link.send_raw(letters.as_bytes())?;
            }
            link.log(Duration::from_secs_f32(seconds), |line| println!("{line}"))?;
            if link.hidden_nmea() > 0 {
                eprintln!("({} GPS sentences hidden)", link.hidden_nmea());
            }
        }

        Command::Reset => {
            open()?.reset()?;
            println!("board restarted");
        }

        Command::Raw { cmd, params, dummy_bits, read, khz } => {
            let cmd = hex_byte(&cmd)?;
            let params = params.iter().map(|text| hex_byte(text)).collect::<Result<Vec<u8>>>()?;
            let request = Request::PanelRaw { cmd, dummy_bits, read_len: read, clock_khz: khz, params: &params };
            match open()?.ask(&request)? {
                Answer::PanelRaw(bytes) if bytes.is_empty() => println!("(sent, nothing read)"),
                Answer::PanelRaw(bytes) => {
                    let hex: Vec<String> = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
                    println!("{}", hex.join(" "));
                }
                Answer::Ack { status: 0, .. } => println!("(no bytes)"),
                Answer::Ack { status, .. } => bail!("the board refused: {}", status_name(status)),
                other => bail!("unexpected answer: {other:?}"),
            }
        }

        Command::Testcard { kind, output } => {
            match kind {
                CardArg::Panel => Picture::panel_card().save(&output)?,
                CardArg::Fb => {
                    picture::save_indexed(test_card::FB_W, test_card::FB_H, &picture::fb_card(), &test_card::PALETTE, &output)?
                }
            }
            println!("file={}", output.display());
        }

        Command::Compare { expected, actual, scale_expected, scale_actual, diff, list } => {
            let a = Picture::load(&expected)?.scaled(scale_expected.max(1));
            let b = Picture::load(&actual)?.scaled(scale_actual.max(1));
            let difference = picture::compare(&a, &b)?;
            println!("pixels={}", difference.total);
            println!("mismatches={}", difference.wrong.len());
            if let Some((x0, y0, x1, y1)) = difference.bounds() {
                println!("bounds=x{x0}..{x1} y{y0}..{y1}");
                let bits: Vec<String> = difference.bits.iter().map(usize::to_string).collect();
                println!("wrong_bits_r4..r0_g5..g0_b4..b0={}", bits.join(","));
                for (x, y, want, got) in difference.wrong.iter().take(list) {
                    println!("at x={x} y={y} expected=0x{want:04x} actual=0x{got:04x}");
                }
                if difference.wrong.len() > list {
                    println!("... and {} more", difference.wrong.len() - list);
                }
            }
            if let Some(path) = diff {
                picture::difference_picture(&b, &difference).save(&path)?;
                println!("diff={}", path.display());
            }
            if !difference.wrong.is_empty() {
                return Ok(ExitCode::from(1));
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("boardctl: {error:#}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_and_release_name_a_player_1_to_4() {
        let parse = |line: &[&str]| Args::try_parse_from(line).map(|a| a.command);
        assert!(matches!(parse(&["boardctl", "key", "a"]), Ok(Command::Key { player: 1, .. })));
        assert!(matches!(parse(&["boardctl", "key", "a", "--player", "4"]), Ok(Command::Key { player: 4, .. })));
        assert!(matches!(parse(&["boardctl", "release"]), Ok(Command::Release { player: 1 })));
        assert!(matches!(parse(&["boardctl", "release", "--player", "2"]), Ok(Command::Release { player: 2 })));
        for bad in ["0", "5"] {
            assert!(parse(&["boardctl", "key", "a", "--player", bad]).is_err(), "{bad}");
        }
    }
}
