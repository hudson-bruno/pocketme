//! The measuring recorder (feature `rec`): races driven by the test
//! pilots through the game's keys, their recordings sent as the game
//! sends them, turned into board log text, decoded by the PC tool's own
//! code (examples/rec/capture.rs), replayed and compared with the race
//! the game ran, step by step.
//!
//! The pilots are the computer's, not a person: they say nothing about
//! how a human drives; they only exercise the recording.
//!
//! One test at a time (a lock): the recording's buffer is one static,
//! shared by every thread of this test program.
#![cfg(feature = "rec")]

mod common;

#[path = "../examples/rec/capture.rs"]
mod capture;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Mutex;

use camber::app::{App, Backdrop, Keys, Log, Page, Scene};
use camber::race::{Class, Mode, Phase, Race};
use camber::rec::{self, Before, LINE_BYTES, Rec, Stage, Text};
use camber::track::{FM, GAP, TRACKS};
use common::{CALL_60, Driver, keys_of_ctl};
use game_kit::button;

static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the serial line carries before a game's text.
const BOARD: &str = "INFO - game: ";
/// The firmware's log ring and the port's speed (bytes a second).
const RING: usize = 4096;
const PORT_BPS: u64 = 11_520;

/// One call as `rec::RecRacer::update` makes it: the line due (logged
/// before the game's own), the game's call, then the recorder's look at
/// what the call did.
fn step(d: &mut Driver, rc: &mut Rec, held: u32, stick: (i16, i16)) -> Option<String> {
    let mut t = Text::new();
    let line = rc
        .next_line(d.call_us, &mut t)
        .then(|| t.as_str().to_string());
    let b = Before::of(&d.app);
    let k = Keys {
        held,
        pressed: held & !d.held,
        stick,
        delta_us: d.call_us,
        ..Keys::default()
    };
    d.hold(held, stick);
    rc.after(b, &d.app, &k);
    line
}

/// From the title into a race (A on START), with the recorder watching.
fn start(d: &mut Driver, rc: &mut Rec, track: u8, class: Class, mode: Mode) {
    d.app.opts.track = track;
    d.app.opts.class = class;
    d.app.opts.mode = mode;
    assert_eq!(d.app.scene, Scene::Title);
    d.app.sel = 0;
    assert!(step(d, rc, button::A, (0, 0)).is_none());
    assert_eq!(d.app.scene, Scene::Race);
    assert_eq!(d.app.race.tick, 0);
    assert_eq!(rc.stage, Stage::Taking);
}

/// A pilot: the keys for this call (held, stick).
type Pilot<'a> = &'a mut dyn FnMut(&App, u32) -> (u32, (i16, i16));

/// One race as the game ran it and sent it.
struct Run {
    /// The recording's lines, as the game logged them.
    lines: Vec<String>,
    /// All of it as it reaches the PC: the game's other log lines in
    /// between, the console's and firmware's prefixes.
    board: String,
    /// The race after every call (by tick) up to the finish.
    snaps: BTreeMap<u32, Race>,
    /// The recorder after the finish.
    head: rec::Head,
    finish: u32,
    place: u8,
    falls: u32,
    laps: Vec<u32>,
    /// Calls from the finish to the end of the first send, and their
    /// game time.
    send_calls: u32,
    send_us: u64,
    /// The most bytes waiting in the firmware's ring (model: the port
    /// drains 11,520 bytes a second, the whole time).
    ring_peak: usize,
    /// The longest line of the game's (characters).
    longest: usize,
    sign_seen: bool,
}

fn log_text(l: &Log) -> String {
    format!("camber: {l:?}").to_lowercase()
}

/// Drives one race to the results and to the end of the recording's
/// first send.
fn run(track: u8, class: Class, mode: Mode, fast: bool, pilot: Pilot) -> Run {
    let mut d = Driver::new();
    d.draw = false;
    d.set(d.app.opts.detail, d.app.opts.view, fast);
    // A different start frame each race (the seed; the Driver does not
    // draw, so the frame count stands still otherwise).
    d.app.frames = u32::from(track) * 3 + mode as u32 + u32::from(fast) * 101;
    let mut rc = Rec::new();
    start(&mut d, &mut rc, track, class, mode);
    let mut r = Run {
        lines: Vec::new(),
        board: String::from("INFO - console: camber started\r\n"),
        snaps: BTreeMap::new(),
        head: rec::Head::default(),
        finish: 0,
        place: 0,
        falls: 0,
        laps: Vec::new(),
        send_calls: 0,
        send_us: 0,
        ring_peak: 0,
        longest: 0,
        sign_seen: false,
    };
    r.snaps.insert(d.app.race.tick, d.app.race);
    let mut ring: u64 = 0;
    let mut call = 0u32;
    let mut done_at = None;
    loop {
        call += 1;
        assert!(call < 60 * 60 * 15, "the race did not end");
        let racing = d.app.race.phase != Phase::Done;
        let (held, stick) = if racing {
            pilot(&d.app, call)
        } else {
            (0, (0, 0))
        };
        let mut out = Vec::new();
        if let Some(s) = step(&mut d, &mut rc, held, stick) {
            r.longest = r.longest.max(s.len());
            r.lines.push(s.clone());
            out.push(s);
        }
        for l in d.logs.drain(..) {
            match l {
                Log::Fell => r.falls += 1,
                Log::Lap { time, .. } => r.laps.push(time),
                _ => {}
            }
            out.push(log_text(&l));
        }
        for s in out {
            let wire = format!("{BOARD}{s}\r\n");
            ring += wire.len() as u64;
            r.board.push_str(&wire);
        }
        r.ring_peak = r.ring_peak.max(ring as usize);
        assert!(ring as usize <= RING, "the firmware's ring would overflow");
        ring = ring.saturating_sub(u64::from(d.call_us) * PORT_BPS / 1_000_000);
        if call.is_multiple_of(97) {
            r.board.push_str("INFO - app: loop 4.1 ms, other text\r\n");
        }
        if racing {
            r.snaps.insert(d.app.race.tick, d.app.race);
        }
        if racing && d.app.race.phase == Phase::Done {
            done_at = Some(call);
            r.finish = d.app.race.ships[0].finish;
        }
        if d.app.results() && rc.sign().is_some() {
            r.sign_seen = true;
        }
        if let Some(at) = done_at
            && !rc.sending()
            && r.send_calls == 0
        {
            r.send_calls = call - at;
            r.send_us = u64::from(r.send_calls) * u64::from(d.call_us);
        }
        // A short recording is sent before the results show: go on to
        // them, to see the sign there.
        if r.send_calls > 0 && d.app.results() {
            break;
        }
    }
    assert_eq!(rc.stage, Stage::Done);
    assert_eq!(rc.sends, 1);
    r.head = rc.head;
    r.place = rc.place;
    r
}

/// Decodes the board text, replays it and checks the replay against
/// every state the game had.
fn prove(r: &Run, what: &str) -> (capture::Recording, capture::Replayed) {
    for l in &r.lines {
        // 80 of the game's; with "game: " and "INFO - " at most 93.
        assert!(l.len() <= 80, "{what}: line too long ({}): {l}", l.len());
        assert!(BOARD.len() + l.len() <= 96);
        assert!(l.bytes().all(|b| b.is_ascii_graphic() || b == b' '));
    }
    let found = capture::decode(&r.board).unwrap_or_else(|e| panic!("{what}: {e}"));
    let rc = found.rec;
    assert_eq!(rc.head, r.head, "{what}: header");
    assert_eq!(rc.finish, r.finish, "{what}: finish");
    assert_eq!(rc.place, r.place, "{what}: place");
    let last = rc.head.steps;
    let mut compared = 0;
    let (race, played) = capture::replay(&rc, |race| {
        if let Some(s) = r.snaps.get(&race.tick) {
            assert_eq!(race, s, "{what}: the replay differs at tick {}", race.tick);
            compared += 1;
        }
    });
    let want = r.snaps.keys().filter(|&&k| k >= 1 && k <= last).count();
    assert_eq!(compared, want, "{what}: states compared");
    assert!(compared > 100, "{what}: only {compared} states");
    if !rc.head.cut {
        assert!(played.matches, "{what}: replay {:?}", played);
        assert_eq!(race.phase, Phase::Done);
        assert_eq!(played.finish, r.finish);
        assert_eq!(played.place, r.place);
        assert_eq!(played.falls, r.falls, "{what}: falls");
        assert_eq!(
            &played.laps[..r.laps.len()],
            &r.laps[..],
            "{what}: lap times"
        );
        // The finishing order as the game had it at that tick.
        let fin = r.snaps.get(&race.tick).copied();
        if let Some(f) = fin {
            assert_eq!(&f.order[..usize::from(f.n)], &played.order[..]);
        }
    }
    (rc, played)
}

fn auto(app: &App, _: u32) -> (u32, (i16, i16)) {
    keys_of_ctl(app.race.drive(0))
}

fn clean(app: &App, _: u32) -> (u32, (i16, i16)) {
    keys_of_ctl(app.race.drive_plain(0))
}

/// The autopilot with a shaky thumb: small noise on the stick (more
/// runs, a bigger recording).
fn noisy(seed: u32) -> impl FnMut(&App, u32) -> (u32, (i16, i16)) {
    let mut s = seed | 1;
    move |app, _| {
        let (held, (x, y)) = keys_of_ctl(app.race.drive(0));
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        let n = (s % 4001) as i32 - 2000;
        (held, ((i32::from(x) + n).clamp(-32_767, 32_767) as i16, y))
    }
}

/// Before the gap of a track (lap 2), lets go of the thrust and brakes:
/// the ship falls in and is put back.
fn faller(track: u8) -> impl FnMut(&App, u32) -> (u32, (i16, i16)) {
    let tr = &TRACKS[usize::from(track)];
    let len = tr.len();
    let gap = (0..len / FM)
        .map(|m| m * FM)
        .find(|&s| tr.seg(tr.index(s)).kind & GAP != 0)
        .expect("the track has a gap");
    move |app, _| {
        let sh = &app.race.ships[0];
        let before = (gap - sh.s).rem_euclid(len);
        if sh.lap == 2 && before < FM * 60 && sh.fall == 0 {
            (button::B, (0, 0))
        } else {
            keys_of_ctl(app.race.drive(0))
        }
    }
}

/// The slowest that still gets round: thrust only half the time, noisy
/// stick (the longest recording the pilots make).
fn dawdler(seed: u32) -> impl FnMut(&App, u32) -> (u32, (i16, i16)) {
    let mut n = noisy(seed);
    move |app, call| {
        let (held, stick) = n(app, call);
        let held = if call % 6 < 3 {
            held
        } else {
            held & !button::A
        };
        (held, stick)
    }
}

/// Steering that changes by a lot every call and a carve button
/// flickering: a run every step, so the buffer fills.
fn jitter(app: &App, call: u32) -> (u32, (i16, i16)) {
    let (mut held, (x, y)) = keys_of_ctl(app.race.drive_plain(0));
    let k = if call.is_multiple_of(2) { 6000 } else { -6000 };
    if call.is_multiple_of(3) {
        held |= button::L;
    }
    (held, ((i32::from(x) + k).clamp(-32_767, 32_767) as i16, y))
}

fn out_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("CAMBER_REC_OUT").map(std::path::PathBuf::from)
}

fn row(r: &Run, what: &str, fast: bool) -> String {
    let lines = r.head.lines() + 2;
    format!(
        "{what:<34} {:>3} fps steps {:>6} bytes {:>5} ({:.2} B/step) lines {:>4} send {:>5.2} s ring peak {:>4} B longest line {} cut {}",
        if fast { 60 } else { 30 },
        r.head.steps,
        r.head.len,
        f64::from(r.head.len) / f64::from(r.head.steps.max(1)),
        lines,
        r.send_us as f64 / 1e6,
        r.ring_peak,
        r.longest,
        r.head.cut
    )
}

#[test]
fn every_track_class_and_mode_replays_step_by_step() {
    let _l = lock();
    let mut table = String::new();
    for track in 0..TRACKS.len() as u8 {
        for (ci, &class) in Class::ALL.iter().enumerate() {
            for mode in [Mode::Race, Mode::Trial] {
                // Both frame rates, both pilots over the set.
                let fast = (track as usize + ci).is_multiple_of(2);
                let r = if ci == 1 {
                    run(track, class, mode, fast, &mut clean)
                } else {
                    run(track, class, mode, fast, &mut auto)
                };
                let what = format!(
                    "{} {} {}",
                    TRACKS[usize::from(track)].name,
                    class.name(),
                    mode.name()
                );
                assert!(!r.head.cut, "{what}: cut");
                assert!(r.sign_seen, "{what}: no sign on the results");
                let (rc, p) = prove(&r, &what);
                assert_eq!(rc.head.mode, rec::mode_code(mode));
                assert_eq!(p.laps.len(), 3, "{what}: laps");
                let _ = writeln!(table, "{}", row(&r, &what, fast));
            }
        }
    }
    println!("{table}");
    if let Some(dir) = out_dir() {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("sizes-all.txt"), table).unwrap();
    }
}

#[test]
fn a_fall_and_the_respawn_replay() {
    let _l = lock();
    let track = 2;
    let r = run(track, Class::Brisk, Mode::Race, false, &mut faller(track));
    assert!(r.falls >= 1, "the pilot did not fall");
    let (_, p) = prove(&r, "GLACIER fall");
    assert_eq!(p.falls, r.falls);
    println!("{}", row(&r, "GLACIER BRISK RACE fall", false));
}

#[test]
fn noisy_and_slow_races_replay_and_the_longest_is_measured() {
    let _l = lock();
    let mut table = String::new();
    let mut longest = (0u64, String::new());
    for (k, fast) in [(0u8, false), (1, true), (2, false)] {
        let r = run(
            k,
            Class::Wild,
            Mode::Race,
            fast,
            &mut noisy(7 + u32::from(k)),
        );
        let what = format!("{} WILD RACE noisy", TRACKS[usize::from(k)].name);
        prove(&r, &what);
        let _ = writeln!(table, "{}", row(&r, &what, fast));
        let r = run(
            k,
            Class::Steady,
            Mode::Trial,
            fast,
            &mut dawdler(11 + u32::from(k)),
        );
        let what = format!("{} STEADY TRIAL dawdler", TRACKS[usize::from(k)].name);
        prove(&r, &what);
        let _ = writeln!(table, "{}", row(&r, &what, fast));
        if r.send_us > longest.0 {
            longest = (r.send_us, what);
        }
    }
    println!("{table}");
    println!(
        "longest send: {} ({:.2} s)",
        longest.1,
        longest.0 as f64 / 1e6
    );
    if let Some(dir) = out_dir() {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("sizes-pilots.txt"), table).unwrap();
    }
}

#[test]
fn a_full_buffer_is_cut_marked_and_still_sent() {
    let _l = lock();
    let r = run(2, Class::Steady, Mode::Trial, true, &mut jitter);
    assert!(r.head.cut, "the buffer did not fill: {} bytes", r.head.len);
    assert!(r.head.len as usize > rec::CAP - rec::RUN_MAX);
    assert!(r.sign_seen);
    let (rc, p) = prove(&r, "cut");
    assert!(rc.head.cut);
    assert!(!p.matches);
    // The worst case of a send: the whole buffer.
    println!("{}", row(&r, "GLACIER STEADY TRIAL jitter (full)", true));
    assert!(r.head.lines() * LINE_BYTES as u32 >= rec::CAP as u32 - LINE_BYTES as u32 - 8);
}

#[test]
fn damaged_captures_are_refused() {
    let _l = lock();
    let r = run(1, Class::Brisk, Mode::Trial, false, &mut auto);
    assert!(capture::decode(&r.board).is_ok());
    let lines: Vec<&str> = r.board.lines().collect();
    let data: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].contains("camber: rec D "))
        .collect();
    assert!(data.len() > 4);
    let join = |v: &[&str]| v.join("\n");

    // A lost data line.
    let mut v = lines.clone();
    v.remove(data[3]);
    let e = capture::decode(&join(&v)).unwrap_err();
    assert!(e.contains("REFUSED") && e.contains("data line"), "{e}");

    // One character changed (still base64): the CRC catches it.
    let mut v: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    let l = &mut v[data[2]];
    let at = l.len() - 5;
    let c = l.as_bytes()[at];
    let nc = if c == b'A' { 'B' } else { 'A' };
    l.replace_range(at..at + 1, &nc.to_string());
    let e = capture::decode(&v.join("\n")).unwrap_err();
    assert!(e.contains("CRC MISMATCH"), "{e}");

    // A line the console changed ('?' for a letter it would not print).
    let mut v: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    v[data[1]].replace_range(at - 3..at - 2, "?");
    let e = capture::decode(&v.join("\n")).unwrap_err();
    assert!(e.contains("base64"), "{e}");

    // Two lines swapped: the numbering.
    let mut v = lines.clone();
    v.swap(data[1], data[2]);
    let e = capture::decode(&join(&v)).unwrap_err();
    assert!(e.contains("follows"), "{e}");

    // The end line lost (the send cut short).
    let v: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| !l.contains("camber: rec E "))
        .collect();
    let e = capture::decode(&join(&v)).unwrap_err();
    assert!(e.contains("no end line"), "{e}");

    // A header number changed: the CRC covers it too.
    let mut v: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    let h = v.iter().position(|l| l.contains("camber: rec H ")).unwrap();
    let parts: Vec<&str> = v[h].split(' ').collect();
    let seed = parts.len() - 7;
    let mut p: Vec<String> = parts.iter().map(|s| s.to_string()).collect();
    p[seed] = format!("{:08x}", u32::from_str_radix(&p[seed], 16).unwrap() ^ 1);
    v[h] = p.join(" ");
    let e = capture::decode(&v.join("\n")).unwrap_err();
    assert!(e.contains("CRC MISMATCH"), "{e}");

    // A version 1 header (the format before the catch-up and rules
    // digits): named as such, not as a damaged line.
    let mut v: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    let mut p: Vec<String> = v[h].split(' ').map(str::to_string).collect();
    let ver = p.iter().position(|t| t == "H").unwrap() + 2;
    p[ver] = "1".into();
    p.drain(ver + 4..ver + 6);
    v[h] = p.join(" ");
    let e = capture::decode(&v.join("\n")).unwrap_err();
    assert!(
        e.contains("REFUSED") && e.contains("format version 1"),
        "{e}"
    );

    // Other race rules: refused before any replay.
    let mut v: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    let mut p: Vec<String> = v[h].split(' ').map(str::to_string).collect();
    p[ver + 5] = "1".into();
    v[h] = p.join(" ");
    let e = capture::decode(&v.join("\n")).unwrap_err();
    assert!(e.contains("race rules 1"), "{e}");

    // Nothing at all.
    let e = capture::decode("INFO - game: camber: started\n").unwrap_err();
    assert!(e.contains("no header"), "{e}");

    // A damaged send followed by a good one: the good one is taken and
    // the damaged one named.
    let mut v = lines.clone();
    v.remove(data[3]);
    let twice = format!("{}\n{}", join(&v), r.board);
    let f = capture::decode(&twice).unwrap();
    assert_eq!(f.sends.len(), 2);
    assert!(!f.sends[0].1.is_empty() && f.sends[1].1.is_empty());
}

/// A race left before its first tick (START at once, LEAVE from the pause
/// menu): the next race, also at tick 0, is still a new recording with
/// its own numbers. Before the fix the recorder kept "a race at its first
/// tick was seen" over the title and took the second race as the first.
#[test]
fn a_race_left_at_its_first_tick_does_not_hide_the_next_one() {
    let _l = lock();
    let mut d = Driver::new();
    d.draw = false;
    let mut rc = Rec::new();
    start(&mut d, &mut rc, 0, Class::Brisk, Mode::Race);
    let first = rc.head;
    assert_eq!(first.class, rec::class_code(Class::Brisk));
    step(&mut d, &mut rc, button::START, (0, 0));
    assert_eq!(d.app.race.tick, 0, "paused before the first tick");
    assert!(d.app.pause.is_some());
    // Two up from RESUME: TITLE (one up would be LEAVE, out of the game).
    for _ in 0..2 {
        step(&mut d, &mut rc, 0, (0, 0));
        step(&mut d, &mut rc, button::UP, (0, 0));
    }
    step(&mut d, &mut rc, 0, (0, 0));
    step(&mut d, &mut rc, button::A, (0, 0));
    assert_eq!(d.app.scene, Scene::Title, "LEAVE goes to the title");
    step(&mut d, &mut rc, 0, (0, 0));
    start(&mut d, &mut rc, 1, Class::Wild, Mode::Race);
    assert_eq!(rc.head.id, first.id.wrapping_add(1), "a new recording");
    assert_eq!(rc.head.track, 1);
    assert_eq!(rc.head.class, rec::class_code(Class::Wild));
    assert_eq!(rc.head.catchup, rec::catchup_code(d.app.race.catchup));
}

/// The title's made-up race (its ships fly and its ticks run behind
/// the menu) is never recorded: a minute of the title, across a change
/// of track and class (a new made-up race each) and of the backdrop to
/// SHIP for the last half minute, sends nothing and
/// leaves the recorder empty; the race started afterwards is the first
/// recording.
#[test]
fn a_title_left_running_a_minute_records_nothing() {
    let _l = lock();
    let mut d = Driver::new();
    d.draw = false;
    let mut rc = Rec::new();
    let empty = rc.head;
    for i in 0..30 * 60 {
        let held = match i {
            600 => button::DOWN,
            602 => button::RIGHT,
            604 => button::DOWN,
            606 => button::RIGHT,
            608 => button::UP,
            610 => button::UP,
            // OPTIONS, BACKDROP to SHIP, B back, up to PLAY.
            900 | 902 | 904 | 906 => button::DOWN,
            908 => button::A,
            910 | 912 | 914 | 916 => button::DOWN,
            918 => button::RIGHT,
            920 => button::B,
            922 | 924 | 926 | 928 => button::UP,
            _ => 0,
        };
        assert!(step(&mut d, &mut rc, held, (0, 0)).is_none(), "a line sent");
        assert_eq!(d.app.scene, Scene::Title);
        assert_eq!(rc.stage, Stage::Empty);
        assert_eq!(rc.head, empty);
    }
    assert!(d.app.race.tick > 0, "the made-up race ran");
    assert!(d.app.tv.races >= 2, "a new made-up race after the change");
    assert_eq!(
        d.app.opts.backdrop,
        Backdrop::Ship,
        "the turning ship shown"
    );
    assert_eq!((d.app.page, d.app.sel), (Page::Main, 0));
    assert!(d.logs.is_empty(), "{:?}", d.logs);
    assert_eq!(d.app.best, [[0; 3]; 3]);
    start(&mut d, &mut rc, 1, Class::Wild, Mode::Race);
    assert_eq!(rc.head.id, empty.id.wrapping_add(1), "the first recording");
    assert_eq!(rc.head.track, 1);
}

#[test]
fn bytes_and_runs_round_trip() {
    use camber::race::{Carve, Ctl};
    let _l = lock();
    let mut prev = Ctl::default();
    let mut data = Vec::new();
    let mut want = Vec::new();
    let mut s = 12345u32;
    for k in 0..5000u32 {
        s = s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let c = Ctl {
            steer: match k % 5 {
                0 => (prev.steer + ((s >> 8) % 41) as i32 - 20).clamp(-256, 256),
                1 => ((s >> 4) % 513) as i32 - 256,
                _ => prev.steer,
            },
            thrust: s & 1 != 0,
            brake_l: s & 2 != 0 && k % 3 == 0,
            brake_r: s & 4 != 0 && k % 3 == 0,
            carve: rec::carve_of(((s >> 20) % 5) as u8).unwrap_or(Carve::Off),
        };
        if c == prev && k > 0 {
            continue;
        }
        let n = match k % 7 {
            0 => 1 + (s >> 3) % 300_000,
            1 => 5,
            _ => 1 + (s >> 5) % 4,
        };
        let mut b = [0u8; rec::RUN_MAX];
        let len = rec::encode_run(&prev, &c, n, &mut b);
        data.extend_from_slice(&b[..len]);
        want.push((c, n));
        prev = c;
    }
    let got: Vec<(Ctl, u32)> = rec::Runs::new(&data).map(|r| r.unwrap()).collect();
    // Runs of the same controls written as two (a short byte and a long
    // one) come back as two.
    let mut merged: Vec<(Ctl, u32)> = Vec::new();
    for (c, n) in got {
        match merged.last_mut() {
            Some((pc, pn)) if *pc == c => *pn += n,
            _ => merged.push((c, n)),
        }
    }
    assert_eq!(merged, want);
    // Base64 both ways.
    for len in 0..50usize {
        let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + len) as u8).collect();
        let mut t = String::new();
        for ch in bytes.chunks(3) {
            let v = (u32::from(ch[0]) << 16)
                | (u32::from(*ch.get(1).unwrap_or(&0)) << 8)
                | u32::from(*ch.get(2).unwrap_or(&0));
            for k in 0..=ch.len() {
                t.push(rec::B64[((v >> (18 - 6 * k)) & 63) as usize] as char);
            }
        }
        assert_eq!(capture::b64_decode(&t).unwrap(), bytes);
    }
}

/// The real game through the kit's stand-in console: the race is driven
/// with the keys an autopilot used in a `Driver` race at the same rate,
/// the game's own log lines (as the console hands them over) decode to
/// a recording whose replay is that same race; L and R together on the
/// results send it again.
#[test]
fn the_game_itself_logs_a_recording_that_replays() {
    use game_kit::testing::TestConsole;
    let _l = lock();
    // The keys of a Driver race, call by call, at the rate the game
    // itself starts with (RATE 60 FPS is the default since 2026-11-23):
    // the stand-in console below calls the race that often.
    let mut d = Driver::new();
    d.draw = false;
    assert!(d.app.opts.fast);
    let (detail, view) = (d.app.opts.detail, d.app.opts.view);
    d.set(detail, view, true);
    let mut rc = Rec::new();
    start(&mut d, &mut rc, 0, Class::Brisk, Mode::Trial);
    // The call that sees the A released (`c.hold(0)` below).
    step(&mut d, &mut rc, 0, (0, 0));
    let mut keys = Vec::new();
    while d.app.race.phase != Phase::Done {
        let k = keys_of_ctl(d.app.race.drive(0));
        keys.push(k);
        step(&mut d, &mut rc, k.0, k.1);
        assert!(keys.len() < 60 * 60 * 5);
    }
    let want_check = rc.head.check;
    let want_finish = d.app.race.ships[0].finish;
    assert_eq!(d.call_us, CALL_60);

    // SAFETY: the game's own entry function, made by `game_kit::game!`.
    let mut c = unsafe { TestConsole::new(camber::game_entry) };
    c.run_ms(500);
    // Title: MODE is the fourth line; RIGHT makes it TIME TRIAL.
    c.tap(button::DOWN);
    c.tap(button::DOWN);
    c.tap(button::DOWN);
    c.tap(button::RIGHT);
    c.tap(button::UP);
    c.tap(button::UP);
    c.tap(button::UP);
    c.hold(button::A);
    c.step();
    c.hold(0);
    c.step();
    for (held, (x, y)) in &keys {
        c.hold(*held);
        c.left_stick(0, *x, *y);
        c.step();
    }
    c.hold(0);
    c.left_stick(0, 0, 0);
    c.run_ms(20_000);
    let board = |c: &TestConsole| -> String {
        // The record shows each call's log escaped, in quotes.
        let mut t = String::new();
        for l in c.log_lines() {
            for line in l.trim_matches('"').replace("\\n", "\n").lines() {
                t.push_str(&format!("{BOARD}{line}\r\n"));
            }
        }
        t
    };
    let text = board(&c);
    let f = capture::decode(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    assert_eq!(f.sends.len(), 1);
    let (_, p) = capture::replay(&f.rec, |_| {});
    assert!(p.matches, "{p:?}");
    assert_eq!(f.rec.head.mode, 1);
    // The same keys at the same rate: the same race as the Driver's.
    assert_eq!(f.rec.head.check, want_check);
    assert_eq!(f.rec.finish, want_finish);
    // The results show the sign: REC (dim) over SENT (selected colour).
    let has = |c: &TestConsole, y0: i16, colour: u8| {
        let x0 = camber::app::END_BOX.0 + 3;
        (0..camber::app::LINE).any(|dy| {
            (0..24).any(|dx| c.picture().get((x0 + dx) as u16, (y0 + dy) as u16) == colour)
        })
    };
    assert!(has(&c, camber::app::END_Y, camber::art::TEXT_DIM));
    assert!(has(
        &c,
        camber::app::END_Y + camber::app::LINE,
        camber::art::SEL
    ));
    if let Some(dir) = out_dir() {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("results-sign.png"), c.png()).unwrap();
    }

    // L and R together: a second send, the same recording.
    c.hold(button::L | button::R);
    c.step();
    c.hold(0);
    c.run_ms(20_000);
    let text = board(&c);
    let f2 = capture::decode(&text).unwrap();
    assert_eq!(f2.sends.len(), 2);
    assert!(f2.sends.iter().all(|s| s.1.is_empty()));
    assert_eq!(f2.rec, f.rec);
    // L alone does nothing.
    c.hold(button::L);
    c.step();
    c.hold(0);
    c.run_ms(3_000);
    assert_eq!(capture::decode(&board(&c)).unwrap().sends.len(), 2);
}

/// Writes sample captures, their decoding and the ghost measure for
/// pilot races (not a human) to `CAMBER_REC_OUT`.
#[test]
#[ignore = "writes files: CAMBER_REC_OUT=<dir> cargo test --features rec --test rec -- --ignored"]
fn samples_and_ghost_measure() {
    let _l = lock();
    let dir = out_dir().expect("CAMBER_REC_OUT");
    let mut all = String::from(
        "GHOST MEASURE ON TEST-PILOT RECORDINGS (the computer's pilots, NOT a human)\n\n",
    );
    for (track, class, mode, pilot) in [
        (0u8, Class::Brisk, Mode::Trial, "auto"),
        (1, Class::Wild, Mode::Trial, "auto"),
        (2, Class::Steady, Mode::Trial, "clean"),
        (0, Class::Brisk, Mode::Race, "auto"),
        (2, Class::Brisk, Mode::Race, "fall"),
        (1, Class::Steady, Mode::Trial, "dawdler"),
    ] {
        let r = match pilot {
            "clean" => run(track, class, mode, false, &mut clean),
            "fall" => run(track, class, mode, false, &mut faller(track)),
            "dawdler" => run(track, class, mode, false, &mut dawdler(3)),
            _ => run(track, class, mode, false, &mut auto),
        };
        let name = format!(
            "{}-{}-{}-{pilot}",
            TRACKS[usize::from(track)].name,
            class.name(),
            if mode == Mode::Race { "race" } else { "trial" }
        )
        .to_lowercase();
        let (rc, p) = prove(&r, &name);
        let sub = dir.join(&name);
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("capture.txt"), &r.board).unwrap();
        let g = capture::ghost(&rc);
        if mode == Mode::Trial {
            assert!(
                g.same_as_recorded,
                "{name}: a ghost of a trial is the trial"
            );
        } else {
            // Replayed with its contacts the race is the one recorded.
            assert_eq!(capture::real(&rc).finish, rc.finish, "{name}");
        }
        let text = format!(
            "== {name} ({pilot} pilot)\n{}{}\n",
            capture::summary(&rc, &p),
            capture::measures_text(&rc)
        );
        std::fs::write(sub.join("ghost.txt"), &text).unwrap();
        all.push_str(&text);
    }
    std::fs::write(dir.join("ghost-all.txt"), all).unwrap();
}
