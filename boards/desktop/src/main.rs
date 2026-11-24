//! The simulator program. See `pocketme-sim --help`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use console_core::Console;
use desktop_board::board::{DesktopBoard, FileStore, PANEL_H, PANEL_W};
use desktop_board::link::SimLink;
use desktop_board::run::{self, ScriptRun};
use desktop_board::settings::{Boot, Kept};
use desktop_board::{SAMPLE_CARD, keymap, picture, script};

const HELP: &str = "\
pocketme-sim: the console's own code on the PC, on a desktop board.
It is not an emulator of the ESP32 chip: it cannot tell you about memory,
speed, tearing or the radio. The real board stays the judge of those.

USAGE
  pocketme-sim [options]             no window: run frames, save pictures
  pocketme-sim --window [options]    open a window (keyboard = pad, mouse = touch)

NO-WINDOW OPTIONS
  --frames N       how many frames to run (default 60); frame 0 is the first
  --script FILE    the input script (see README.md); none means no input
  --shot LIST      frames to save as PNG, for example 0,10,59 or last
  --out DIR        where pictures go (default: shots)
  --png FILE       also save the last picture as FILE
  --prints         print a fingerprint of the picture after every frame
  --trace          print what was sent to the panel, frame by frame

WINDOW OPTIONS
  --scale N        window size: 320x240 times N, 1 to 6 (default 3)
  --close-after S  close by itself after S seconds (0.5 to 5), and do not
                   take the keyboard; saves what the window showed last as
                   window-last.png in the --out folder
  --overlay        start with the overlay on (F1 switches it)

BOTH
  --fps 30|60      ticks a second of simulated time (default 30)
  --card DIR       a folder that stands in for the SD card
                   (default: the sample card that comes with the simulator)
  --no-card        no SD card in the slot
  --store FILE     keep the saved settings in FILE (default: memory only);
                   the brightness is saved 2 s after the last change
  --boot full|short|off
                   how the GATHER boot animation plays in this run (default:
                   the saved setting, FULL when none; the BOOT ANIM line in
                   the console menu changes it too). Builds without the
                   `boot-gather` feature have only the plain boot screen
  --link ADDR      the debug link on a local TCP address, e.g. 127.0.0.1:7000
  --settings-entry tiles|on|off
                   builds with the `settings` feature only: where the
                   settings app is opened from (default off: the console
                   as without the app). tiles or on: GAMEPADS and
                   SETTINGS tiles in the home row (before and after the
                   games) plus quick-menu shortcuts
  --pair WHAT      settings app: what the stand-in pairing radio does
                   (there is no real radio on the PC). found (default):
                   finds pad A after 3 s, pairs it 2 s after YES; two:
                   finds pads A and B; none: finds nothing in 60 s;
                   old-pin, lost, refused, store: finds pad A, then the
                   call ends with that failure; linked, damaged, not-held,
                   busy: the board refuses at once; off: a board that
                   cannot pair
  --pair-saved N   settings app: pads already saved, 0 to 4 (default 1;
                   the first one shows as connected)
  --pair-slots N   settings app: free records in the pad memory, 0 to 255
                   (default: not known)
  --quiet          do not print the console's log lines
  --help           this text

GAMES AND MOTION
  Fourteen are linked in and are on the sample card: BENCH (a speed
  test, a measuring tool and not a game), CAMBER (3D hover-ship
  racing), DRIVE3D (a 3D rally prototype, not a finished game),
  HAIRPIN (a kart race), KESTREL (a 3D courier craft made of
  polygons), LANDER (a moon lander), LANTERN (a first-person walk
  through a mine), MERCATOR (3D space trading), PADTEST (shows every
  input), PROTO3D (a 3D platformer prototype with four test areas,
  not a finished game), ROOFTOP (a platformer), SKIMMER (air hockey
  for 1 to 4 players), STARDUST (a comet that catches stars) and
  TUNDRA (ice hockey for 1 to 4 players). Each game's README lists its
  controls (games/bench, games/camber, games/drive3d, games/hairpin,
  games/kestrel, games/lander, games/lantern, games/mercator,
  games/proto3d, games/rooftop, games/skimmer, games/tundra). Pick a
  tile and press Z or Return to start one; Tab opens the pause menu.
  On the home screen Tab (or a click on the gear) opens the console
  menu, with BRIGHTNESS. The motion sensor is a stand-in: hold V and
  move the mouse to tilt the pad; M turns the sensor off or on. In a
  script: motion on|off, tilt X Y, spin X Y Z (see README.md).

A run with no window ends with an error (exit code 1) on any problem:
a script mistake, a picture that cannot be saved, script lines after the
last frame, or anything the real board would refuse (too many areas, a
log line too long, two sends in one step).

KEYS IN THE WINDOW
";

fn main() -> ExitCode {
    match run_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("pocketme-sim: {why}");
            ExitCode::FAILURE
        }
    }
}

/// Every option, read from the command line.
struct Options {
    window: bool,
    frames: u64,
    fps: u32,
    script: Option<PathBuf>,
    shots: Vec<String>,
    out: PathBuf,
    png: Option<PathBuf>,
    prints: bool,
    trace: bool,
    scale: u32,
    close_after: Option<f64>,
    overlay: bool,
    card: Option<PathBuf>,
    store: Option<PathBuf>,
    boot: Option<Boot>,
    link: Option<String>,
    quiet: bool,
    /// The settings app (feature `settings`): the way in and the
    /// stand-in pairing radio. Checked in `settings_app`.
    settings_entry: Option<String>,
    pair: Option<String>,
    pair_saved: Option<u8>,
    pair_slots: Option<u8>,
}

fn parse_args(args: &[String]) -> Result<Option<Options>, String> {
    let mut o = Options {
        window: false,
        frames: 60,
        fps: 30,
        script: None,
        shots: Vec::new(),
        out: PathBuf::from("shots"),
        png: None,
        prints: false,
        trace: false,
        scale: 3,
        close_after: None,
        overlay: false,
        card: Some(PathBuf::from(SAMPLE_CARD)),
        store: None,
        boot: None,
        link: None,
        quiet: false,
        settings_entry: None,
        pair: None,
        pair_saved: None,
        pair_slots: None,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || {
            it.next()
                .map(String::as_str)
                .ok_or(format!("{arg} needs a value"))
        };
        let whole = |v: &str| {
            v.parse::<u64>()
                .map_err(|_| format!("{arg} wants a whole number, not `{v}`"))
        };
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--window" => o.window = true,
            "--frames" => o.frames = whole(value()?)?,
            "--fps" => {
                o.fps = match whole(value()?)? {
                    30 => 30,
                    60 => 60,
                    n => return Err(format!("--fps must be 30 or 60, not {n}")),
                }
            }
            "--script" => o.script = Some(PathBuf::from(value()?)),
            "--shot" => o.shots.extend(value()?.split(',').map(|s| s.trim().into())),
            "--out" => o.out = PathBuf::from(value()?),
            "--png" => o.png = Some(PathBuf::from(value()?)),
            "--prints" => o.prints = true,
            "--trace" => o.trace = true,
            "--scale" => {
                o.scale = match whole(value()?)? {
                    n @ 1..=6 => n as u32,
                    n => return Err(format!("--scale must be 1 to 6, not {n}")),
                }
            }
            "--close-after" => {
                let v = value()?;
                let s = v
                    .parse::<f64>()
                    .ok()
                    .filter(|s| (0.5..=5.0).contains(s))
                    .ok_or(format!("--close-after wants 0.5 to 5 seconds, not `{v}`"))?;
                o.close_after = Some(s);
            }
            "--overlay" => o.overlay = true,
            "--card" => o.card = Some(PathBuf::from(value()?)),
            "--no-card" => o.card = None,
            "--store" => o.store = Some(PathBuf::from(value()?)),
            "--boot" => {
                let v = value()?;
                if !cfg!(feature = "boot-gather") {
                    return Err(
                        "--boot: this build has no boot animation (built without `boot-gather`)"
                            .into(),
                    );
                }
                o.boot = Some(
                    Boot::parse(v)
                        .ok_or(format!("--boot must be full, short or off, not `{v}`"))?,
                );
            }
            "--link" => o.link = Some(value()?.to_string()),
            "--quiet" => o.quiet = true,
            "--settings-entry" => o.settings_entry = Some(value()?.to_string()),
            "--pair" => o.pair = Some(value()?.to_string()),
            "--pair-saved" => {
                o.pair_saved = match whole(value()?)? {
                    n @ 0..=4 => Some(n as u8),
                    n => return Err(format!("--pair-saved must be 0 to 4, not {n}")),
                }
            }
            "--pair-slots" => {
                o.pair_slots = match whole(value()?)? {
                    n @ 0..=255 => Some(n as u8),
                    n => return Err(format!("--pair-slots must be 0 to 255, not {n}")),
                }
            }
            other => return Err(format!("unknown option `{other}` (try --help)")),
        }
    }
    Ok(Some(o))
}

fn run_main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(o) = parse_args(&args)? else {
        print!("{HELP}{}", keymap::help_table());
        return Ok(());
    };
    if let Some(card) = &o.card
        && !card.is_dir()
    {
        return Err(format!("--card {}: no such folder", card.display()));
    }

    // About 200 KB together: on the heap, not the stack.
    let mut console = Box::new(Console::new());
    let mut board = Box::new(DesktopBoard::new(o.card.as_deref()));
    settings_app(&o, &mut console, &mut board)?;
    board.log.echo = !o.quiet;
    if let Some(path) = &o.store {
        board.store = FileStore::open(path)?;
    }
    if let Some(addr) = &o.link {
        board.link = SimLink::listen(addr).map_err(|e| format!("--link {addr}: {e}"))?;
        println!("debug link: listening on {addr}");
    }

    if o.window {
        return run_window(&o, &mut console, &mut board);
    }
    run_headless(&o, &mut console, &mut board)
}

/// The settings app's options (feature `settings`).
#[cfg(feature = "settings")]
fn settings_app(
    o: &Options,
    console: &mut Console,
    board: &mut DesktopBoard,
) -> Result<(), String> {
    use console_core::settings::Entry;
    let entry = match &o.settings_entry {
        None => Entry::Off,
        Some(v) => Entry::parse(v).ok_or(format!(
            "--settings-entry must be tiles, on or off, not `{v}`"
        ))?,
    };
    let what = o.pair.as_deref().unwrap_or("found");
    let radio = pair_radio(what, o.pair_saved.unwrap_or(1), o.pair_slots).ok_or(format!(
        "--pair must be found, two, none, old-pin, lost, refused, store, linked, \
         damaged, not-held, busy or off, not `{what}`"
    ))?;
    if what == "off" && (o.pair_saved.is_some() || o.pair_slots.is_some()) {
        return Err(
            "--pair-saved and --pair-slots need a radio that can pair (not --pair off)".into(),
        );
    }
    console.set_settings_entry(entry);
    board.pad.radio = radio;
    Ok(())
}

/// The stand-in pairing radio for `--pair`: `saved` pads already saved
/// (the first connected), `slots` free records if known. `None` for a
/// name it does not know.
#[cfg(feature = "settings")]
fn pair_radio(
    what: &str,
    saved: u8,
    slots: Option<u8>,
) -> Option<desktop_board::fake_radio::FakeRadio> {
    use board_api::PairFail;
    use desktop_board::fake_radio::{FAKE_SAVED_TAILS, FakeRadio, PairScript};
    // In console steps (30 a second while a pairing page is open): pad A
    // is found 3 s into the search, pad B 5 s in, a call takes 2 s, a
    // search 60 s.
    const FIND: u16 = 90;
    const FIND_B: u16 = 150;
    const CALL: u16 = 60;
    const SEARCH: u16 = 1800;
    let found = PairScript::found_and_saved(FIND, CALL).searching(SEARCH);
    let script = match what {
        "off" => return Some(FakeRadio::default()),
        "found" => found,
        "two" => PairScript::two_found(FIND, FIND_B)
            .calling(CALL)
            .searching(SEARCH),
        "none" => PairScript::nothing_found(SEARCH),
        "old-pin" => found.call_fails(PairFail::OldPin),
        "lost" => found.call_fails(PairFail::Lost),
        "refused" => found.call_fails(PairFail::Refused),
        "store" => found
            .call_fails(PairFail::Store)
            .forget_fails(PairFail::Store),
        "linked" => found.begin_fails(PairFail::Linked),
        "damaged" => found
            .begin_fails(PairFail::Damaged)
            .forget_fails(PairFail::Damaged),
        "not-held" => PairScript::not_held(),
        "busy" => found
            .begin_fails(PairFail::Busy)
            .forget_fails(PairFail::Busy),
        _ => return None,
    };
    let mut radio = FakeRadio::new(script);
    for place in 0..saved.min(4) {
        radio = radio.with_saved(place, FAKE_SAVED_TAILS[usize::from(place)], place == 0);
    }
    if let Some(n) = slots {
        radio = radio.with_slots(n);
    }
    Some(radio)
}

#[cfg(not(feature = "settings"))]
fn settings_app(o: &Options, _: &mut Console, _: &mut DesktopBoard) -> Result<(), String> {
    if o.settings_entry.is_some()
        || o.pair.is_some()
        || o.pair_saved.is_some()
        || o.pair_slots.is_some()
    {
        return Err(
            "--settings-entry, --pair, --pair-saved and --pair-slots need a build \
             with the `settings` feature"
                .into(),
        );
    }
    Ok(())
}

fn run_headless(
    o: &Options,
    console: &mut Console,
    board: &mut DesktopBoard,
) -> Result<(), String> {
    let events = match &o.script {
        None => Vec::new(),
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            script::parse(&text)?
        }
    };
    let last = o.frames.saturating_sub(1);
    let mut shots = BTreeSet::new();
    for item in &o.shots {
        let frame = match item.as_str() {
            "last" => last,
            n => n
                .parse()
                .map_err(|_| format!("--shot wants frame numbers or `last`, not `{n}`"))?,
        };
        // A frame that never runs would silently give no picture.
        if frame >= o.frames {
            return Err(format!(
                "--shot {frame}: that frame never runs (--frames {} runs frames 0 to {last})",
                o.frames
            ));
        }
        shots.insert(frame);
    }

    let mut save_errors = Vec::new();
    let mut sends = 0u32;
    let started = Instant::now();
    let run = ScriptRun {
        frames: o.frames,
        fps: o.fps,
        events,
    };
    let mut light = None;
    // The console with its settings read from the store (and saved back).
    let mut kept = Kept::start(console, board);
    if let Some(b) = o.boot {
        kept.boot_option(board, b);
    }
    let mut problems = run::run_script(&run, board, &mut kept, |info, b, shot| {
        sends += info.presents;
        if o.trace && b.screen.backlight != light {
            light = b.screen.backlight;
            if let Some(v) = light {
                println!("frame {:5} backlight {v}", info.frame);
            }
        }
        if o.trace && info.presents > 0 {
            let r = b.screen.last_report;
            println!(
                "frame {:5} sent {} areas, {} pixels, {} bytes, {} us (model){}",
                info.frame,
                r.areas,
                r.pixels,
                b.screen.last_bytes,
                r.send_us,
                if u64::from(r.send_us) > console_core::FRAME_US {
                    "  OVER THE FRAME"
                } else {
                    ""
                }
            );
        }
        if o.prints {
            println!(
                "frame {:5} fingerprint {:016x}",
                info.frame,
                picture::fingerprint(&b.screen.image)
            );
        }
        if shot || shots.contains(&info.frame) {
            let path = o.out.join(format!("frame-{:05}.png", info.frame));
            save(&path, &b.screen.image, &mut save_errors);
        }
    });
    let wall = started.elapsed();
    if let Some(path) = &o.png {
        save(path, &board.screen.image, &mut save_errors);
    }
    for name in &board.files.skipped {
        eprintln!(
            "warning: card folder: `{name}` is not an 8.3 name, so the card could not hold it"
        );
    }
    println!(
        "ran {} frames ({} console frames, {} sends, {} over the frame time) in {:.1} ms; \
         clock {} us; backlight {}; fingerprint {:016x}",
        o.frames,
        console.frames(),
        sends,
        board.screen.over_budget,
        wall.as_secs_f64() * 1000.0,
        board.clock.now_us,
        board
            .screen
            .backlight
            .map_or("-".to_string(), |v| v.to_string()),
        picture::fingerprint(&board.screen.image)
    );
    problems.extend(save_errors);
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} problem(s):\n  {}",
            problems.len(),
            problems.join("\n  ")
        ))
    }
}

fn save(path: &Path, image: &[u16], errors: &mut Vec<String>) {
    let (w, h) = (usize::from(PANEL_W), usize::from(PANEL_H));
    match picture::save_png(path, image, w, h) {
        Ok(()) => println!("saved {}", path.display()),
        Err(e) => errors.push(format!("could not save {}: {e}", path.display())),
    }
}

#[cfg(feature = "window")]
fn run_window(o: &Options, console: &mut Console, board: &mut DesktopBoard) -> Result<(), String> {
    use desktop_board::window::{SimWindow, WindowOptions};
    if o.script.is_some() || !o.shots.is_empty() || o.prints || o.trace || o.png.is_some() {
        return Err(
            "--script, --shot, --png, --prints and --trace are for runs with no window".into(),
        );
    }
    let mut win = SimWindow::open(WindowOptions {
        scale: o.scale,
        fps: o.fps,
        close_after: o.close_after.map(std::time::Duration::from_secs_f64),
        shots_dir: o.out.clone(),
        overlay: o.overlay,
    })?;
    let mut kept = Kept::start(console, board);
    if let Some(b) = o.boot {
        kept.boot_option(board, b);
    }
    println!("{}", win.run(board, |b| kept.step(b)));
    // A quick check also keeps what the window showed last, read back
    // from the window itself, so nobody has to look at the screen.
    if o.close_after.is_some() {
        let (w, h, rgb) = win.read_back(board)?;
        let path = o.out.join("window-last.png");
        picture::save_png_rgb888(&path, &rgb, w, h)
            .map_err(|e| format!("could not save {}: {e}", path.display()))?;
        println!("what the window showed last: {}", path.display());
    }
    Ok(())
}

#[cfg(not(feature = "window"))]
fn run_window(_: &Options, _: &mut Console, _: &mut DesktopBoard) -> Result<(), String> {
    Err("this build has no window (it was built without the `window` feature)".into())
}
