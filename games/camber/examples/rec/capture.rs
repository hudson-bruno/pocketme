//! The PC side of the `rec` feature: finds a recording in captured log
//! text, checks it, replays it, and races the rivals against it.
//!
//! Shared by the tool (`examples/rec/main.rs`) and the tests
//! (`tests/rec.rs` includes this file by path), so the tests prove the
//! very code the tool runs.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fmt::Write as _;

use camber::race::{COUNT, Ctl, Ev, LAPS, Mode, Phase, Race};
use camber::rec::{self, Bad, Crc, Head, LINE_BYTES, RULES, Runs, TAG, VERSION};
use camber::track::{FM, TRACKS};

/// One checked recording.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recording {
    pub head: Head,
    pub data: Vec<u8>,
    /// The player's finish (ticks of race clock) and place, from the end
    /// line.
    pub finish: u32,
    pub place: u8,
    /// The runs, decoded.
    pub runs: Vec<(Ctl, u32)>,
}

/// What a capture held.
#[derive(Debug)]
pub struct Found {
    /// The last send that passed every check.
    pub rec: Recording,
    /// Every send seen, in order, and what was wrong with it (empty:
    /// good).
    pub sends: Vec<(u16, Vec<String>)>,
}

#[derive(Default)]
struct Send {
    id: u16,
    head: Option<Head>,
    lines: u32,
    data: BTreeMap<u32, Vec<u8>>,
    end: Option<(u32, u32, u32, u8)>,
    problems: Vec<String>,
}

fn hex(t: &str, digits: usize) -> Result<u32, String> {
    if t.len() != digits
        || !t
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(format!("{t:?} is not {digits} hex digits"));
    }
    u32::from_str_radix(t, 16).map_err(|e| e.to_string())
}

fn digit(t: &str) -> Result<u8, String> {
    if t.len() != 1 || !t.as_bytes()[0].is_ascii_digit() {
        return Err(format!("{t:?} is not one digit"));
    }
    Ok(t.as_bytes()[0] - b'0')
}

/// base64url without padding, strictly (unused bits must be 0).
pub fn b64_decode(t: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let b = t.as_bytes();
    if b.len() % 4 == 1 {
        return Err(format!("{} base64 characters cannot be bytes", b.len()));
    }
    for chunk in b.chunks(4) {
        let mut v: u32 = 0;
        for (k, &c) in chunk.iter().enumerate() {
            let x = rec::b64_value(c).ok_or_else(|| format!("{:?} is not base64url", c as char))?;
            v |= u32::from(x) << (18 - 6 * k);
        }
        let n = chunk.len() - 1;
        let bytes = [(v >> 16) as u8, (v >> 8) as u8, v as u8];
        if bytes[n..].iter().any(|&x| x != 0) && n < 3 {
            return Err("base64 with stray bits at the end".into());
        }
        out.extend_from_slice(&bytes[..n]);
    }
    Ok(out)
}

/// The part of a line after `camber: rec `, when it has one.
fn rec_part(line: &str) -> Option<&str> {
    line.find(TAG).map(|at| line[at + TAG.len()..].trim_end())
}

/// Finds every send of a recording in `text` (any lines around them are
/// skipped) and checks each; the last good one wins.
pub fn decode(text: &str) -> Result<Found, String> {
    let mut sends: Vec<Send> = Vec::new();
    let mut stray: Vec<String> = Vec::new();
    for (no, line) in text.lines().enumerate() {
        let Some(part) = rec_part(line) else { continue };
        let no = no + 1;
        let t: Vec<&str> = part.split(' ').collect();
        let id = match t.get(1).map(|x| hex(x, 4)) {
            Some(Ok(id)) => id as u16,
            _ => {
                stray.push(format!("line {no}: no recording number: {part:?}"));
                continue;
            }
        };
        let open = sends.iter().rposition(|s| s.id == id);
        match t[0] {
            "H" => {
                let mut s = Send {
                    id,
                    ..Send::default()
                };
                match parse_head(&t, u32::from(id)) {
                    Ok((h, lines)) => {
                        s.head = Some(h);
                        s.lines = lines;
                    }
                    Err(e) => s.problems.push(format!("line {no}: bad header: {e}")),
                }
                sends.push(s);
            }
            "D" | "E" => {
                let Some(i) = open else {
                    stray.push(format!(
                        "line {no}: recording {id:04x} line without its header"
                    ));
                    continue;
                };
                let s = &mut sends[i];
                if s.end.is_some() {
                    s.problems.push(format!("line {no}: after the end line"));
                    continue;
                }
                if t[0] == "D" {
                    match parse_data(&t) {
                        Ok((seq, bytes)) => {
                            if let Some(old) = s.data.get(&seq) {
                                if *old != bytes {
                                    s.problems.push(format!(
                                        "line {no}: data line {seq} twice, different"
                                    ));
                                }
                            } else {
                                if let Some((&last, _)) = s.data.iter().next_back() {
                                    if seq != last + 1 {
                                        s.problems.push(format!(
                                            "line {no}: data line {seq} follows {last}"
                                        ));
                                    }
                                } else if seq != 0 {
                                    s.problems
                                        .push(format!("line {no}: first data line is {seq}"));
                                }
                                s.data.insert(seq, bytes);
                            }
                        }
                        Err(e) => s.problems.push(format!("line {no}: bad data line: {e}")),
                    }
                } else {
                    match parse_end(&t) {
                        Ok(e) => s.end = Some(e),
                        Err(e) => s.problems.push(format!("line {no}: bad end line: {e}")),
                    }
                }
            }
            other => stray.push(format!("line {no}: unknown kind {other:?}")),
        }
    }
    let mut report = Vec::new();
    let mut best = None;
    for s in &sends {
        let r = check(s);
        let mut problems = s.problems.clone();
        match r {
            Ok(rec) if problems.is_empty() => best = Some(rec),
            Ok(_) => {}
            Err(e) => problems.push(e),
        }
        report.push((s.id, problems));
    }
    match best {
        Some(rec) => Ok(Found { rec, sends: report }),
        None => {
            let mut msg = String::from("REFUSED: no complete, correct recording in the capture");
            if sends.is_empty() {
                msg.push_str("\n  no header line (`camber: rec H ...`) found");
            }
            for (id, p) in &report {
                for x in p {
                    let _ = write!(msg, "\n  recording {id:04x}: {x}");
                }
            }
            for x in &stray {
                let _ = write!(msg, "\n  {x}");
            }
            Err(msg)
        }
    }
}

fn parse_head(t: &[&str], id: u32) -> Result<(Head, u32), String> {
    // The version first: a version 1 header (13 fields, no catch-up or
    // rules digit) is told apart from a damaged one.
    if t.len() >= 3 && t[2] != VERSION.to_string() {
        return Err(format!(
            "format version {}, this tool reads {VERSION} (version 1: the rivals before \
             2026-11-22, read it with the decoder of that time)",
            t[2]
        ));
    }
    if t.len() != 15 {
        return Err(format!("{} fields, not 15", t.len()));
    }
    let h = Head {
        version: digit(t[2])?,
        id: id as u16,
        track: digit(t[3])?,
        class: digit(t[4])?,
        mode: digit(t[5])?,
        catchup: digit(t[6])?,
        rules: digit(t[7])?,
        seed: hex(t[8], 8)?,
        steps: hex(t[9], 8)?,
        len: hex(t[10], 4)?,
        cut: match digit(t[11])? {
            0 => false,
            1 => true,
            x => return Err(format!("cut is {x}")),
        },
        check: hex(t[12], 8)?,
        crc: hex(t[13], 8)?,
    };
    let lines = hex(t[14], 4)?;
    if h.rules != RULES {
        return Err(format!(
            "race rules {}, this tool replays rules {RULES} only",
            h.rules
        ));
    }
    if usize::from(h.track) >= TRACKS.len()
        || h.class().is_none()
        || h.mode().is_none()
        || h.catchup().is_none()
    {
        return Err(format!(
            "track {} class {} mode {} catch-up {} out of range",
            h.track, h.class, h.mode, h.catchup
        ));
    }
    if lines != h.lines() {
        return Err(format!("{lines} data lines announced for {} bytes", h.len));
    }
    Ok((h, lines))
}

fn parse_data(t: &[&str]) -> Result<(u32, Vec<u8>), String> {
    if t.len() != 4 {
        return Err(format!("{} fields, not 4", t.len()));
    }
    let seq = hex(t[2], 4)?;
    let bytes = b64_decode(t[3])?;
    if bytes.is_empty() || bytes.len() > LINE_BYTES {
        return Err(format!("{} bytes on a line", bytes.len()));
    }
    Ok((seq, bytes))
}

fn parse_end(t: &[&str]) -> Result<(u32, u32, u32, u8), String> {
    if t.len() != 6 {
        return Err(format!("{} fields, not 6", t.len()));
    }
    Ok((hex(t[2], 4)?, hex(t[3], 8)?, hex(t[4], 8)?, digit(t[5])?))
}

fn check(s: &Send) -> Result<Recording, String> {
    let h = s.head.ok_or("no good header")?;
    let (lines, crc, finish, place) = s
        .end
        .ok_or("no end line (lost, or the send was cut short)")?;
    if lines != s.lines || crc != h.crc {
        return Err(format!(
            "end line says {lines} lines crc {crc:08x}, header {} lines crc {:08x}",
            s.lines, h.crc
        ));
    }
    let missing: Vec<u32> = (0..s.lines).filter(|k| !s.data.contains_key(k)).collect();
    if !missing.is_empty() {
        return Err(format!(
            "{} of {} data lines missing (first: {})",
            missing.len(),
            s.lines,
            missing[0]
        ));
    }
    if let Some((&k, _)) = s.data.iter().find(|(k, _)| **k >= s.lines) {
        return Err(format!("data line {k} beyond the {} announced", s.lines));
    }
    let mut data = Vec::new();
    for (k, b) in &s.data {
        let want = if *k + 1 < s.lines {
            LINE_BYTES
        } else {
            h.len as usize - LINE_BYTES * (s.lines as usize - 1)
        };
        if b.len() != want {
            return Err(format!("data line {k} has {} bytes, not {want}", b.len()));
        }
        data.extend_from_slice(b);
    }
    if data.len() != h.len as usize {
        return Err(format!("{} bytes, header says {}", data.len(), h.len));
    }
    let mut c = Crc::new();
    c.bytes(&data);
    c.bytes(&h.meta());
    if c.done() != h.crc {
        return Err(format!(
            "CRC MISMATCH: computed {:08x}, recorded {:08x}",
            c.done(),
            h.crc
        ));
    }
    let mut runs = Vec::new();
    let mut steps: u64 = 0;
    for r in Runs::new(&data) {
        let (ctl, n) = r.map_err(|b: Bad| format!("bad runs: {b:?}"))?;
        steps += u64::from(n);
        runs.push((ctl, n));
    }
    if steps != u64::from(h.steps) {
        return Err(format!("runs hold {steps} steps, header says {}", h.steps));
    }
    Ok(Recording {
        head: h,
        data,
        finish,
        place,
        runs,
    })
}

// ---- Replay --------------------------------------------------------------------------

/// The race a recording starts from.
pub fn start_of(h: &Head) -> Race {
    let mut race = Race::new();
    race.setup(
        h.track,
        h.class().expect("checked class"),
        h.mode().expect("checked mode"),
        h.seed,
    );
    race.catchup = h.catchup().expect("checked catch-up");
    race
}

/// Every step's controls, one by one.
pub fn steps(r: &Recording) -> impl Iterator<Item = Ctl> + '_ {
    r.runs
        .iter()
        .flat_map(|&(c, n)| std::iter::repeat_n(c, n as usize))
}

/// What a replay gave.
#[derive(Clone, Debug, Default)]
pub struct Replayed {
    /// Ship 0's lap times (ticks), fall count.
    pub laps: Vec<u32>,
    pub falls: u32,
    pub finish: u32,
    /// Finish place of ship 0 (0: not finished).
    pub place: u8,
    /// Every ship's finish (ticks, 0: not yet) when ship 0 finished.
    pub finishes: Vec<u32>,
    /// The order right after ship 0's finishing step.
    pub order: Vec<u8>,
    /// `race_check` after the last step, and whether it is the console's.
    pub check: u32,
    pub matches: bool,
}

/// Replays the recording step by step as the game does (events taken
/// after every step); `each` sees the race after every step.
pub fn replay(r: &Recording, mut each: impl FnMut(&Race)) -> (Race, Replayed) {
    let mut race = start_of(&r.head);
    let mut out = Replayed::default();
    for c in steps(r) {
        race.step(Some(c));
        while let Some(e) = race.take_ev() {
            match e {
                Ev::Lap { ship: 0, time, .. } => out.laps.push(time),
                Ev::Finish {
                    ship: 0,
                    place,
                    time,
                } => {
                    out.place = place;
                    out.laps.push(time - out.laps.iter().sum::<u32>());
                }
                Ev::Fell { ship: 0 } => out.falls += 1,
                _ => {}
            }
        }
        each(&race);
    }
    let n = usize::from(race.n);
    out.finish = race.ships[0].finish;
    out.finishes = race.ships[..n].iter().map(|s| s.finish).collect();
    out.order = race.order[..n].to_vec();
    out.check = rec::race_check(&race);
    out.matches = !r.head.cut
        && race.phase == Phase::Done
        && out.check == r.head.check
        && out.finish == r.finish
        && out.place == r.place;
    (race, out)
}

/// Ticks as `m:ss.cc`.
pub fn time(t: u32) -> String {
    let cs = t * 100 / 60;
    format!("{}:{:02}.{:02}", cs / 6000, cs / 100 % 60, cs % 100)
}

pub fn class_name(h: &Head) -> &'static str {
    h.class().map_or("?", |c| c.name())
}

pub fn mode_name(h: &Head) -> &'static str {
    h.mode().map_or("?", |m| m.name())
}

/// A summary of a decoded and replayed recording.
pub fn summary(r: &Recording, p: &Replayed) -> String {
    let h = &r.head;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "recording {:04x}: format {}, {} {} {}, seed {:08x}, {} steps ({:.1} s of race incl. the {} s count), {} bytes{}",
        h.id,
        h.version,
        TRACKS[usize::from(h.track)].name,
        class_name(h),
        mode_name(h),
        h.seed,
        h.steps,
        f64::from(h.steps) / 60.0,
        COUNT / 60,
        h.len,
        if h.cut {
            ", CUT (the buffer filled)"
        } else {
            ""
        }
    );
    let _ = writeln!(s, "crc {:08x}: matched", h.crc);
    let laps: Vec<String> = p.laps.iter().map(|&t| time(t)).collect();
    let _ = writeln!(
        s,
        "replay: finish {} place {}, laps [{}], falls {}",
        time(p.finish),
        p.place,
        laps.join(", "),
        p.falls
    );
    let _ = writeln!(
        s,
        "console said: finish {} place {}; race check {:08x} vs replay {:08x}: {}",
        time(r.finish),
        r.place,
        h.check,
        p.check,
        if p.matches {
            "REPLAY REPRODUCES THE RACE"
        } else if h.cut {
            "not comparable (cut recording)"
        } else {
            "REPLAY DIFFERS"
        }
    );
    s
}

/// The runs as text (one per line: ticks, steering, thrust, brakes,
/// carve).
pub fn runs_text(r: &Recording) -> String {
    let mut s = String::from("# ticks steer thrust brake_l brake_r carve\n");
    for (c, n) in &r.runs {
        let _ = writeln!(
            s,
            "{n} {} {} {} {} {:?}",
            c.steer,
            u8::from(c.thrust),
            u8::from(c.brake_l),
            u8::from(c.brake_r),
            c.carve
        );
    }
    s
}

// ---- Ghost ---------------------------------------------------------------------------

/// Parts of a lap the gains and losses are counted in.
pub const SECTORS: usize = 8;

/// One rival against the ghost.
#[derive(Clone, Debug)]
pub struct Rival {
    pub ship: u8,
    pub name: &'static str,
    /// Finish (ticks of race clock, 0: not finished).
    pub finish: u32,
    pub best: u32,
    /// Ticks the rival lost (+) or gained (-) on the ghost in each
    /// sector, all laps added up; `None` when a boundary was not reached
    /// by both.
    pub sector: [Option<i64>; SECTORS],
    /// Behind (+) or ahead (-) of the ghost when both crossed the start
    /// line the first time (ticks).
    pub at_start: Option<i64>,
}

/// The ghost race.
#[derive(Clone, Debug)]
pub struct Ghost {
    /// True: the recorded ship was a ghost; false: the real race,
    /// replayed with its contacts and run on until the rivals finish.
    pub ghost: bool,
    pub track: u8,
    /// The ghost's finish and best lap in the ghost race (ticks).
    pub finish: u32,
    pub best: u32,
    /// Whether the ghost's finish equals the recorded one (it may not:
    /// without bumps the ghost's path can differ from the real race's).
    pub same_as_recorded: bool,
    pub recorded_finish: u32,
    pub rivals: Vec<Rival>,
    pub steps: u32,
}

/// Races the seven rivals against the recording: the recorded ship is a
/// ghost (no bumps, it passes through and nothing hits it; the rivals
/// still see it and steer round it), the rivals start from the grid of
/// a race with the recording's track, class and seed.
pub fn ghost(r: &Recording) -> Ghost {
    measure(r, true)
}

/// A recording of a RACE replayed as it was (contacts and all) and run
/// on after the player's finish until every rival is home: the rivals'
/// real finishes and sectors against the player's.
pub fn real(r: &Recording) -> Ghost {
    measure(r, false)
}

fn measure(r: &Recording, as_ghost: bool) -> Ghost {
    let h = &r.head;
    let class = h.class().expect("checked class");
    let mut race = Race::new();
    if as_ghost {
        race.setup(h.track, class, Mode::Race, h.seed);
        race.catchup = h.catchup().expect("checked catch-up");
        let solo = start_of(h);
        race.ships[0] = solo.ships[0];
    } else {
        race = start_of(h);
    }
    let len = race.tr().len();
    let n = usize::from(race.n);
    let bounds = (LAPS as usize) * SECTORS + 1;
    // First tick each ship reached each boundary (0 is the start line).
    let mut cross = vec![vec![None::<u32>; bounds]; n];
    let ctls: Vec<Ctl> = steps(r).collect();
    let limit = ctls.len() as u32 + 60 * 600;
    let mut k = 0usize;
    loop {
        let c = ctls.get(k).copied().unwrap_or_default();
        if as_ghost {
            race.ships[0].ghost = u8::MAX;
        }
        race.step(Some(c));
        while race.take_ev().is_some() {}
        k += 1;
        for (i, row) in cross.iter_mut().enumerate() {
            let p = race.ships[i].prog(len);
            for (b, slot) in row.iter_mut().enumerate() {
                let at = (b as i64) * i64::from(len) / SECTORS as i64;
                if slot.is_none() && p >= at {
                    *slot = Some(race.tick);
                }
            }
        }
        let all = race.ships[..n].iter().all(|s| s.finished());
        if all || race.tick >= limit {
            break;
        }
    }
    let g = &race.ships[0];
    let mut rivals = Vec::new();
    for i in 1..n {
        let sh = &race.ships[i];
        let d =
            |b: usize| -> Option<i64> { Some(i64::from(cross[i][b]?) - i64::from(cross[0][b]?)) };
        let mut sector = [Some(0i64); SECTORS];
        for b in 1..bounds {
            let s = (b - 1) % SECTORS;
            sector[s] = match (sector[s], d(b), d(b - 1)) {
                (Some(acc), Some(x), Some(y)) => Some(acc + x - y),
                _ => None,
            };
        }
        rivals.push(Rival {
            ship: i as u8,
            name: camber::art::ENTRANTS[i].name,
            finish: sh.finish,
            best: sh.best,
            sector,
            at_start: d(0),
        });
    }
    Ghost {
        ghost: as_ghost,
        track: h.track,
        finish: g.finish,
        best: g.best,
        same_as_recorded: g.finish == r.finish && !h.cut,
        recorded_finish: r.finish,
        rivals,
        steps: race.tick,
    }
}

/// Every measure of a recording as text: for a RACE recording the real
/// race first, then the ghost race.
pub fn measures_text(r: &Recording) -> String {
    let mut s = String::new();
    if r.head.mode() == Some(Mode::Race) {
        s.push_str(&ghost_text(r, &real(r)));
        s.push('\n');
    }
    s.push_str(&ghost_text(r, &ghost(r)));
    s
}

/// The ghost race as a table.
pub fn ghost_text(r: &Recording, g: &Ghost) -> String {
    let h = &r.head;
    let tr = &TRACKS[usize::from(h.track)];
    let len_m = tr.len() / FM;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "{}: {} {} (recorded as {}), seed {:08x}; lap {} m, sectors of {} m",
        if g.ghost { "GHOST" } else { "THE REAL RACE" },
        tr.name,
        class_name(h),
        mode_name(h),
        h.seed,
        len_m,
        len_m / SECTORS as i32
    );
    if g.ghost {
        let _ = writeln!(
            s,
            "the recorded ship is a ghost: no bumps; rivals still steer round it. Rivals start on the race grid of this seed."
        );
        if h.mode() == Some(Mode::Race) {
            let _ = writeln!(
                s,
                "a RACE recording as a ghost: its controls were made for a race with contacts, so without them the ship soon leaves the recorded path; THE REAL RACE above is the measure for it"
            );
        }
    } else {
        let _ = writeln!(
            s,
            "the race replayed as it was (contacts and all), run on after the player's finish until the rivals are home; the player's ship is the reference"
        );
    }
    if h.cut {
        let _ = writeln!(
            s,
            "the recording is CUT: the ghost stops after {} steps and does not finish",
            h.steps
        );
    }
    let _ = writeln!(
        s,
        "ghost finish {} (recorded {}: {}), best lap {}",
        if g.finish > 0 {
            time(g.finish)
        } else {
            "-".into()
        },
        time(g.recorded_finish),
        if g.same_as_recorded {
            "same path"
        } else {
            "DIFFERENT: the real race had contacts or the grid differs"
        },
        time(g.best)
    );
    let _ = writeln!(
        s,
        "rival     finish     vs ghost        best lap   start  {}   most lost / most gained",
        (1..=SECTORS)
            .map(|k| format!("{k:>5}"))
            .collect::<Vec<_>>()
            .join("")
    );
    for rv in &g.rivals {
        let (fin, delta) = if rv.finish > 0 && g.finish > 0 {
            let d = i64::from(rv.finish) - i64::from(g.finish);
            (
                time(rv.finish),
                format!(
                    "{:+.2} s {:+.1}%",
                    d as f64 / 60.0,
                    d as f64 * 100.0 / f64::from(g.finish)
                ),
            )
        } else if rv.finish > 0 {
            (time(rv.finish), "ghost did not finish".into())
        } else {
            ("-".into(), "rival did not finish".into())
        };
        let secs: String = rv
            .sector
            .iter()
            .map(|x| match x {
                Some(t) => format!("{:>+5.1}", *t as f64 / 60.0),
                None => "    -".into(),
            })
            .collect();
        let known: Vec<(usize, i64)> = rv
            .sector
            .iter()
            .enumerate()
            .filter_map(|(k, x)| x.map(|v| (k, v)))
            .collect();
        let worst = known.iter().max_by_key(|x| x.1);
        let best = known.iter().min_by_key(|x| x.1);
        let part = |x: Option<&(usize, i64)>| match x {
            Some(&(k, v)) => format!(
                "{}-{} m {:+.2} s",
                k as i32 * len_m / SECTORS as i32,
                (k as i32 + 1) * len_m / SECTORS as i32,
                v as f64 / 60.0
            ),
            None => "-".into(),
        };
        let _ = writeln!(
            s,
            "{:<9} {:<10} {:<15} {:<10} {:>+5.1} {}   {} / {}",
            rv.name,
            fin,
            delta,
            if rv.best > 0 {
                time(rv.best)
            } else {
                "-".into()
            },
            rv.at_start.map_or(f64::NAN, |x| x as f64 / 60.0),
            secs,
            part(worst),
            part(best)
        );
    }
    let _ = writeln!(
        s,
        "vs ghost: + the rival finished behind. Sector columns: seconds the rival lost (+) or gained (-) on the ghost in that eighth of the lap, three laps added. start: behind (+) at the first crossing of the line."
    );
    s
}
