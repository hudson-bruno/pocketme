//! The simulator's command line program. See `desk-sim --help`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use desk_sim::headless::HeadlessBoard;
use desk_sim::script;

const HELP: &str = "\
desk-sim: the console's own code on the PC, with a fake board underneath.
It is not an emulator of the ESP32 chip: it cannot tell you about memory
limits, speed, tearing or the radio. The board stays the judge of those.

USAGE
  desk-sim window   [options]   open a window (keyboard = pad, mouse = touch)
  desk-sim headless [options]   no window: run a number of frames with a script
  desk-sim --help               this text

WINDOW OPTIONS
  --scale N          window size: 320x240 times N (default 2)
  --fps 30|60        frames a second (default 30)
  --close-after S    close by itself after S seconds (for quick checks);
                     also saves what the window showed last as
                     window-last.png in the screenshot folder
  --shots DIR        where F12 saves screenshots (default: shots)

HEADLESS OPTIONS
  --frames N         how many frames to run (default 60)
  --fps 30|60        frames a second of simulated time (default 30)
  --script FILE      the input script (see README.md); none means no input
  --shot LIST        frames to save as PNG, for example 0,10,59 or last
  --out DIR          where the PNG files go (default: shots)
  --quiet            do not print the console's log lines

KEYS IN THE WINDOW
  arrows          d-pad              X / Z         A / B
  S / A           X / Y              Q / W         L / R (bumpers)
  1 / 2           left / right trigger (fully pulled while held)
  I J K L         left stick         T F G H       right stick
  Enter           START              Backspace     SELECT
  Tab             HOME               mouse button  touch
  P               pause / go on      N             next frame (while paused)
  F12             screenshot (PNG)   Esc           quit
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None | Some("-h" | "--help" | "help") => {
            print!("{HELP}");
            Ok(())
        }
        Some("window") => run_window(&args[1..]),
        Some("headless") => run_headless(&args[1..]),
        Some(other) => Err(format!("unknown command `{other}` (try --help)")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("desk-sim: {why}");
            ExitCode::FAILURE
        }
    }
}

/// Options as `--name value` pairs, or `--name` alone for a switch.
struct Options<'a> {
    args: &'a [String],
}

impl<'a> Options<'a> {
    /// Fails on any option not in `known`.
    fn new(args: &'a [String], known: &[&str], switches: &[&str]) -> Result<Self, String> {
        let mut i = 0;
        while i < args.len() {
            let name = args[i].as_str();
            if switches.contains(&name) {
                i += 1;
            } else if known.contains(&name) {
                if i + 1 >= args.len() {
                    return Err(format!("{name} needs a value"));
                }
                i += 2;
            } else {
                return Err(format!("unknown option `{name}` (try --help)"));
            }
        }
        Ok(Self { args })
    }

    fn value(&self, name: &str) -> Option<&'a str> {
        self.args
            .iter()
            .position(|a| a == name)
            .map(|i| self.args[i + 1].as_str())
    }

    fn number(&self, name: &str, default: u32) -> Result<u32, String> {
        match self.value(name) {
            None => Ok(default),
            Some(v) => v
                .parse()
                .map_err(|_| format!("{name} wants a whole number, not `{v}`")),
        }
    }

    fn switch(&self, name: &str) -> bool {
        self.args.iter().any(|a| a == name)
    }

    fn fps(&self) -> Result<u32, String> {
        match self.number("--fps", 30)? {
            fps @ (30 | 60) => Ok(fps),
            other => Err(format!("--fps must be 30 or 60, not {other}")),
        }
    }
}

fn run_headless(args: &[String]) -> Result<(), String> {
    let opts = Options::new(
        args,
        &["--frames", "--fps", "--script", "--shot", "--out"],
        &["--quiet"],
    )?;
    let frames = opts.number("--frames", 60)?;
    let fps = opts.fps()?;
    let events = match opts.value("--script") {
        None => Vec::new(),
        Some(path) => {
            let text =
                std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
            script::parse(&text)?
        }
    };

    let mut board = HeadlessBoard::new(frames, fps, events);
    board.echo_log = !opts.switch("--quiet");
    board.save_to(PathBuf::from(opts.value("--out").unwrap_or("shots")));
    if let Some(list) = opts.value("--shot") {
        for item in list.split(',') {
            let frame = match item.trim() {
                "last" => frames.saturating_sub(1),
                n => n
                    .parse()
                    .map_err(|_| format!("--shot wants frame numbers or `last`, not `{n}`"))?,
            };
            // A frame that never runs would silently give no picture.
            if frame >= frames {
                return Err(format!(
                    "--shot {frame}: that frame never runs (--frames {frames} runs frames 0 to {})",
                    frames.saturating_sub(1)
                ));
            }
            board.keep(frame);
        }
    }

    let started = Instant::now();
    console_demo::run(&mut board);
    let wall = started.elapsed();

    for path in &board.saved {
        println!("saved {}", path.display());
    }
    for frame in board.pictures.keys() {
        println!(
            "frame {frame:5} fingerprint {:016x}",
            board.fingerprints[frame]
        );
    }
    if let Some((frame, print)) = board.fingerprints.last_key_value() {
        println!("last frame {frame} fingerprint {print:016x}");
    }
    let n = board.fingerprints.len().max(1) as f64;
    println!(
        "ran {} frames in {:.1} ms: console work {:.3} ms a frame, whole frame {:.3} ms (PNG saving included)",
        board.fingerprints.len(),
        wall.as_secs_f64() * 1000.0,
        board.work.as_secs_f64() * 1000.0 / n,
        wall.as_secs_f64() * 1000.0 / n,
    );
    if let Some((count, first)) = board.unused_events() {
        eprintln!(
            "warning: {count} script line(s) were not used: they are for frame {first} or later, \
             but the run stopped after frame {}. Use --frames {} or more to reach them.",
            frames.saturating_sub(1),
            first.saturating_add(1)
        );
    }
    if !board.save_errors.is_empty() {
        return Err(board.save_errors.join("\n"));
    }
    Ok(())
}

#[cfg(feature = "window")]
fn run_window(args: &[String]) -> Result<(), String> {
    use desk_sim::window::{WindowBoard, WindowOptions};
    let opts = Options::new(args, &["--scale", "--fps", "--close-after", "--shots"], &[])?;
    let scale = opts.number("--scale", 2)?;
    if !(1..=6).contains(&scale) {
        return Err(format!("--scale must be 1 to 6, not {scale}"));
    }
    let close_after = match opts.value("--close-after") {
        None => None,
        Some(v) => Some(std::time::Duration::from_secs_f64(
            v.parse::<f64>()
                .ok()
                .filter(|s| *s > 0.0)
                .ok_or(format!("--close-after wants seconds, not `{v}`"))?,
        )),
    };
    let options = WindowOptions {
        scale,
        fps: opts.fps()?,
        close_after,
        shots_dir: PathBuf::from(opts.value("--shots").unwrap_or("shots")),
    };
    let quick_check = options.close_after.is_some();
    let mut board = WindowBoard::open(options)?;
    console_demo::run(&mut board);
    println!("{}", board.report());
    // A quick check also keeps what the window showed last, read back
    // from the window itself, so nobody has to look at the screen.
    if quick_check {
        let path = board.save_read_back("window-last.png")?;
        println!("what the window showed last: {}", path.display());
    }
    Ok(())
}

#[cfg(not(feature = "window"))]
fn run_window(_args: &[String]) -> Result<(), String> {
    Err("this build has no window (it was built without the `window` feature)".into())
}
