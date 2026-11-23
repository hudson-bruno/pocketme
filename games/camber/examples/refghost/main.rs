//! `refghost`: CAMBER's rivals measured against a person's own recorded
//! races (feature `rec`; a PC tool, never part of the game).
//!
//! ```text
//! cargo build --offline --release --features rec --example refghost
//! target/release/examples/refghost path <capture.txt> <OUT.path>
//! target/release/examples/refghost ghost <A.path> [<B.path> ...] [--contacts none|oneway] [--tsv FILE]
//! target/release/examples/refghost tracks <A.path> [<B.path> ...]
//! target/release/examples/refghost pilot <A.path> [<B.path> ...]
//! ```
//!
//! * `path` decodes a captured recording (`examples/rec`'s decoder),
//!   replays it with the code as it is, refuses it unless the replay
//!   gives the console's own race check, and writes the player's ship
//!   step by step: the human reference. Every field of the ship is kept,
//!   so the path can be put back exactly whatever the rivals do later.
//! * `ghost` races today's rivals against a path: before each step the
//!   player's ship is put where the path had it a step earlier and given
//!   the recorded buttons for this step (so the rivals see it where they
//!   saw it in the real race), and after the step it is put exactly on
//!   the path again. `--contacts none` (the default): the path's ship is
//!   a ghost (the game's ghost flag), nothing touches it and it touches
//!   nothing, the rivals still see it and steer round it. `--contacts
//!   oneway`: bumps happen as in the game, but only the rival keeps its
//!   side of them (the path cannot be pushed). With today's code
//!   `oneway` is the real race itself (the tool checks the race check).
//! * `tracks` races the rivals alone (the player's ship held out of the
//!   race: kept falling, which no rival sees or touches) on every track,
//!   with each path's class and seed.
//! * `pilot` (`pilot.rs`) flies the player's ship with a copy of the
//!   rivals' own driving whose numbers can be changed (checked tick for
//!   tick against `Race::drive` with today's numbers): what an equal
//!   ship reaches with that driving, and what each change would be worth.
//!
//! An example and not a binary on purpose, as `examples/rec`: the chip
//! build makes the crate a static library, which leaves examples out.

#[path = "../rec/capture.rs"]
mod capture;
mod pilot;

use std::fmt::Write as _;
use std::process::ExitCode;

use camber::art::ENTRANTS;
use camber::fixed::turn;
use camber::race::{COUNT, Carve, CatchUp, Class, Ctl, Ev, LAPS, Mode, Phase, Race, SHIPS, Ship};
use camber::rec;
use camber::track::{FM, SEG_BITS, TRACKS, Track};

// ---- Measuring settings (`--set`, `--pace`, `--catchup`) -------------------------------

/// What the measuring runs change after [`Race::setup`]: nothing by
/// default (the game as built, catch-up OFF).
#[derive(Clone, Debug, Default)]
struct Cfg {
    /// `--set CLASS.field=value`: a field of the class's skill row.
    set: Vec<(Class, String, i32)>,
    /// `--pace a,b,c,d,e,f,g`: the rivals' ships instead of `race::PACE`.
    pace: Option<Vec<i16>>,
    catchup: Option<CatchUp>,
}

static CFG: std::sync::OnceLock<Cfg> = std::sync::OnceLock::new();

fn cfg() -> &'static Cfg {
    CFG.get_or_init(Cfg::default)
}

/// Sets the race up as the game would (catch-up OFF unless asked), with
/// the measuring changes.
fn setup(race: &mut Race, track: u8, class: Class, seed: u32) {
    race.setup(track, class, Mode::Race, seed);
    let c = cfg();
    race.catchup = c.catchup.unwrap_or(CatchUp::Off);
    for (cl, f, v) in &c.set {
        if *cl != class {
            continue;
        }
        let sk = &mut race.skill;
        match f.as_str() {
            "share" => sk.share = *v as u32,
            "step" => sk.step = *v as u8,
            "room" => sk.room = *v * FM / 10,
            "rest" => sk.rest = *v as u8,
            "turn" => sk.turn = *v,
            "dare" => sk.dare = *v,
            "brake" => sk.brake = *v,
            "sling_brake" => sk.sling_brake = *v,
            "carve_rate" => sk.carve_rate = *v,
            "need" => sk.need = *v != 0,
            _ => panic!("refghost: no skill field {f}"),
        }
    }
    if let Some(p) = &c.pace {
        let shift = (seed % 7) as usize;
        for i in 1..SHIPS {
            race.ships[i].pace = p[(i - 1 + shift) % (SHIPS - 1)];
        }
    }
}

fn parse_set(v: &str) -> Result<(Class, String, i32), String> {
    let (k, val) = v.split_once('=').ok_or("--set CLASS.field=value")?;
    let (c, f) = k.split_once('.').ok_or("--set CLASS.field=value")?;
    let class = Class::ALL
        .into_iter()
        .find(|x| x.name().eq_ignore_ascii_case(c))
        .ok_or(format!("no class {c}"))?;
    let val: i32 = val.parse().map_err(|_| format!("not a number: {val}"))?;
    Ok((class, f.to_string(), val))
}

// ---- The path file -------------------------------------------------------------------

/// The path file's format version.
const PATH_VERSION: u32 = 1;

/// The raw ship fields, in the file's column order (upper case: exact
/// integers of `race::Ship`).
const RAW: [&str; 37] = [
    "S",
    "X",
    "H",
    "LAP",
    "COUNTED",
    "VF",
    "VL",
    "VH",
    "PSI",
    "RATE",
    "SHIELD",
    "LIMP",
    "BOOST",
    "FALL",
    "GHOST",
    "HIT",
    "PACE",
    "LANE",
    "C_STEER",
    "C_THRUST",
    "C_BRAKE_L",
    "C_BRAKE_R",
    "C_CARVE",
    "PITCH",
    "LAP_START",
    "BEST",
    "FINISH",
    "SAFE",
    "CARVE",
    "HEAT",
    "SLING",
    "SLING_STEP",
    "AIR",
    "HELD",
    "SETTLE",
    "LEAN",
    "REST",
];

/// The readable columns before the raw ones; the buttons (`steer` to
/// `carve`) are what the player pressed for this step, exactly.
const HUMAN: [&str; 12] = [
    "t", "clock_s", "lap", "s_m", "x_m", "v_mps", "head_deg", "steer", "thrust", "brake_l",
    "brake_r", "carve",
];

fn raw_of(sh: &Ship) -> [i64; 37] {
    [
        sh.s.into(),
        sh.x.into(),
        sh.h.into(),
        sh.lap.into(),
        sh.counted.into(),
        sh.vf.into(),
        sh.vl.into(),
        sh.vh.into(),
        sh.psi.into(),
        sh.rate.into(),
        sh.shield.into(),
        i64::from(sh.limp),
        sh.boost.into(),
        sh.fall.into(),
        sh.ghost.into(),
        sh.hit.into(),
        sh.pace.into(),
        sh.lane.into(),
        sh.ctl.steer.into(),
        i64::from(sh.ctl.thrust),
        i64::from(sh.ctl.brake_l),
        i64::from(sh.ctl.brake_r),
        rec::carve_code(sh.ctl.carve).into(),
        sh.pitch.into(),
        sh.lap_start.into(),
        sh.best.into(),
        sh.finish.into(),
        sh.safe.into(),
        sh.carve.into(),
        sh.heat.into(),
        sh.sling.into(),
        sh.sling_step.into(),
        sh.air.into(),
        sh.held.into(),
        sh.settle.into(),
        sh.lean.into(),
        sh.rest.into(),
    ]
}

fn ship_of(v: &[i64]) -> Result<Ship, String> {
    fn n<T: TryFrom<i64>>(v: i64, what: &str) -> Result<T, String> {
        T::try_from(v).map_err(|_| format!("{what} {v} out of range"))
    }
    let carve =
        |c: i64| rec::carve_of(n::<u8>(c, "carve")?).ok_or_else(|| format!("carve code {c}"));
    Ok(Ship {
        s: n(v[0], "S")?,
        x: n(v[1], "X")?,
        h: n(v[2], "H")?,
        lap: n(v[3], "LAP")?,
        counted: n(v[4], "COUNTED")?,
        vf: n(v[5], "VF")?,
        vl: n(v[6], "VL")?,
        vh: n(v[7], "VH")?,
        psi: n(v[8], "PSI")?,
        rate: n(v[9], "RATE")?,
        shield: n(v[10], "SHIELD")?,
        limp: v[11] != 0,
        boost: n(v[12], "BOOST")?,
        fall: n(v[13], "FALL")?,
        ghost: n(v[14], "GHOST")?,
        hit: n(v[15], "HIT")?,
        pace: n(v[16], "PACE")?,
        lane: n(v[17], "LANE")?,
        ctl: Ctl {
            steer: n(v[18], "C_STEER")?,
            thrust: v[19] != 0,
            brake_l: v[20] != 0,
            brake_r: v[21] != 0,
            carve: carve(v[22])?,
        },
        pitch: n(v[23], "PITCH")?,
        lap_start: n(v[24], "LAP_START")?,
        best: n(v[25], "BEST")?,
        finish: n(v[26], "FINISH")?,
        safe: n(v[27], "SAFE")?,
        carve: n(v[28], "CARVE")?,
        heat: n(v[29], "HEAT")?,
        sling: n(v[30], "SLING")?,
        sling_step: n(v[31], "SLING_STEP")?,
        air: n(v[32], "AIR")?,
        held: n(v[33], "HELD")?,
        settle: n(v[34], "SETTLE")?,
        lean: n(v[35], "LEAN")?,
        rest: n(v[36], "REST")?,
    })
}

fn carve_word(c: Carve) -> &'static str {
    match c {
        Carve::Off => "off",
        Carve::Left => "L",
        Carve::Right => "R",
        Carve::Both => "LR",
        Carve::Keep => "keep",
    }
}

fn carve_from_word(w: &str) -> Result<Carve, String> {
    Ok(match w {
        "off" => Carve::Off,
        "L" => Carve::Left,
        "R" => Carve::Right,
        "LR" => Carve::Both,
        "keep" => Carve::Keep,
        _ => return Err(format!("carve {w:?}")),
    })
}

/// A ship's speed (the length of its velocity), m/s.
fn mps(sh: &Ship) -> f64 {
    let (f, l) = (f64::from(sh.vf), f64::from(sh.vl));
    (f * f + l * l).sqrt() * 60.0 / f64::from(FM)
}

fn m(v: i32) -> f64 {
    f64::from(v) / f64::from(FM)
}

/// A recorded race as a path: row 0 is the grid (before the first step),
/// row k the player's ship after step k and the buttons of step k.
#[derive(Clone, Debug)]
struct PathFile {
    track: u8,
    class: Class,
    seed: u32,
    /// The console's numbers: finish (ticks of race clock), place, race
    /// check after the last step.
    finish: u32,
    place: u8,
    check: u32,
    laps: Vec<u32>,
    source: String,
    rows: Vec<(Ctl, Ship)>,
}

fn class_code(c: Class) -> u8 {
    rec::class_code(c)
}

fn class_of(code: u8) -> Option<Class> {
    Class::ALL.into_iter().find(|&c| rec::class_code(c) == code)
}

fn write_path(p: &PathFile) -> String {
    let tr = &TRACKS[usize::from(p.track)];
    let mut s = String::new();
    let _ = writeln!(s, "# camber reference path {PATH_VERSION}");
    let _ = writeln!(
        s,
        "# made by examples/refghost from a recording of a real race (feature rec); MEASURED: the console's own race, replayed exactly on the PC"
    );
    let _ = writeln!(s, "# source {}", p.source);
    let _ = writeln!(s, "# track {} {}", p.track, tr.name);
    let _ = writeln!(s, "# class {} {}", class_code(p.class), p.class.name());
    let _ = writeln!(s, "# seed {:08x}", p.seed);
    let _ = writeln!(s, "# steps {}", p.rows.len() - 1);
    let _ = writeln!(s, "# finish {} {}", p.finish, capture::time(p.finish));
    let _ = writeln!(s, "# place {}", p.place);
    let _ = writeln!(s, "# check {:08x}", p.check);
    let laps: Vec<String> = p.laps.iter().map(|t| t.to_string()).collect();
    let _ = writeln!(s, "# laps {}", laps.join(" "));
    let _ = writeln!(
        s,
        "# units: t = race tick (1/60 s, the count is ticks 0..{COUNT}); clock_s = race clock; lap = times over the line (1 on lap 1, {} finished); s_m along the lap from the line, x_m right of the road's middle, v_mps the speed, head_deg the nose from the road's direction (right +); steer -256..256, thrust/brake_l/brake_r 0/1, carve off/L/R/LR/keep: the buttons of this step. Upper case: every field of race::Ship, exact integers (fine units 1/65536 m, a tick; angles 65536 a turn). Row t=0 is the grid before the first step (its buttons are none).",
        LAPS + 1
    );
    let mut cols: Vec<&str> = HUMAN.to_vec();
    cols.extend(RAW);
    let _ = writeln!(s, "# cols {}", cols.join(" "));
    for (k, (c, sh)) in p.rows.iter().enumerate() {
        let clock = (k as f64 - f64::from(COUNT)).max(0.0) / 60.0;
        let _ = write!(
            s,
            "{k} {clock:.3} {} {:.3} {:.3} {:.2} {:.2} {} {} {} {} {}",
            sh.lap,
            m(sh.s),
            m(sh.x),
            mps(sh),
            f64::from(sh.psi) * 360.0 / 65_536.0,
            c.steer,
            u8::from(c.thrust),
            u8::from(c.brake_l),
            u8::from(c.brake_r),
            carve_word(c.carve)
        );
        for v in raw_of(sh) {
            let _ = write!(s, " {v}");
        }
        s.push('\n');
    }
    s
}

fn read_path(text: &str, name: &str) -> Result<PathFile, String> {
    let mut head = std::collections::BTreeMap::new();
    let mut cols: Vec<String> = Vec::new();
    let mut rows = Vec::new();
    for (no, line) in text.lines().enumerate() {
        if let Some(h) = line.strip_prefix("# ") {
            let (k, v) = h.split_once(' ').unwrap_or((h, ""));
            if k == "cols" {
                cols = v.split(' ').map(str::to_string).collect();
            } else {
                head.insert(k.to_string(), v.to_string());
            }
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        if cols.is_empty() {
            return Err(format!("{name}: data before the cols line"));
        }
        let t: Vec<&str> = line.split(' ').collect();
        if t.len() != cols.len() {
            return Err(format!(
                "{name} line {}: {} fields, {} columns",
                no + 1,
                t.len(),
                cols.len()
            ));
        }
        let get = |c: &str| -> Result<&str, String> {
            cols.iter()
                .position(|x| x == c)
                .map(|i| t[i])
                .ok_or_else(|| format!("{name}: no column {c}"))
        };
        let int = |c: &str| -> Result<i64, String> {
            get(c)?
                .parse::<i64>()
                .map_err(|e| format!("{name} line {}: {c}: {e}", no + 1))
        };
        let ctl = Ctl {
            steer: int("steer")? as i32,
            thrust: int("thrust")? != 0,
            brake_l: int("brake_l")? != 0,
            brake_r: int("brake_r")? != 0,
            carve: carve_from_word(get("carve")?)?,
        };
        let mut raw = [0i64; 37];
        for (i, c) in RAW.iter().enumerate() {
            raw[i] = int(c)?;
        }
        if int("t")? != rows.len() as i64 {
            return Err(format!("{name} line {}: rows out of order", no + 1));
        }
        rows.push((ctl, ship_of(&raw)?));
    }
    let h = |k: &str| {
        head.get(k)
            .cloned()
            .ok_or_else(|| format!("{name}: no `# {k}` line"))
    };
    let first = |k: &str| -> Result<String, String> {
        Ok(h(k)?.split(' ').next().unwrap_or("").to_string())
    };
    let version = h("camber")?;
    if version != format!("reference path {PATH_VERSION}") {
        return Err(format!("{name}: format {version:?}"));
    }
    let track: u8 = first("track")?.parse().map_err(|e| format!("{e}"))?;
    if usize::from(track) >= TRACKS.len() {
        return Err(format!("{name}: track {track}"));
    }
    let class = class_of(first("class")?.parse().map_err(|e| format!("{e}"))?)
        .ok_or(format!("{name}: class"))?;
    let seed = u32::from_str_radix(&first("seed")?, 16).map_err(|e| format!("{e}"))?;
    let steps: usize = first("steps")?.parse().map_err(|e| format!("{e}"))?;
    if rows.len() != steps + 1 {
        return Err(format!(
            "{name}: {} rows for {steps} steps (and the grid)",
            rows.len()
        ));
    }
    Ok(PathFile {
        track,
        class,
        seed,
        finish: first("finish")?.parse().map_err(|e| format!("{e}"))?,
        place: first("place")?.parse().map_err(|e| format!("{e}"))?,
        check: u32::from_str_radix(&first("check")?, 16).map_err(|e| format!("{e}"))?,
        laps: h("laps")?
            .split(' ')
            .filter(|x| !x.is_empty())
            .map(|x| x.parse().map_err(|e| format!("{e}")))
            .collect::<Result<_, String>>()?,
        source: h("source")?,
        rows,
    })
}

/// `path`: capture -> checked replay -> path file.
fn cmd_path(capture_file: &str, out: &str) -> ExitCode {
    let bytes = match std::fs::read(capture_file) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("refghost: cannot read {capture_file}: {e}");
            return ExitCode::from(1);
        }
    };
    let found = match capture::decode(&String::from_utf8_lossy(&bytes)) {
        Ok(f) => f,
        Err(e) => {
            println!("{e}");
            println!("REFUSED: no good recording in {capture_file}");
            return ExitCode::from(2);
        }
    };
    let r = &found.rec;
    if r.head.mode() != Some(Mode::Race) {
        println!("REFUSED: not a RACE recording");
        return ExitCode::from(2);
    }
    let grid = capture::start_of(&r.head).ships[0];
    let mut ships = Vec::new();
    let (_, played) = capture::replay(r, |race| ships.push(race.ships[0]));
    print!("{}", capture::summary(r, &played));
    if !played.matches {
        println!(
            "REFUSED: the replay does not give the console's race (check {:08x}, replay {:08x}); the code is not the code that raced it",
            r.head.check, played.check
        );
        return ExitCode::from(3);
    }
    let mut rows = vec![(Ctl::default(), grid)];
    rows.extend(capture::steps(r).zip(ships));
    let p = PathFile {
        track: r.head.track,
        class: r.head.class().expect("checked"),
        seed: r.head.seed,
        finish: r.finish,
        place: r.place,
        check: r.head.check,
        laps: played.laps.clone(),
        source: capture_file.to_string(),
        rows,
    };
    let text = write_path(&p);
    // The file must give back exactly the same ships.
    match read_path(&text, out) {
        Ok(back) if back.rows == p.rows => {}
        Ok(_) => {
            println!("REFUSED: the path file does not read back the same ships");
            return ExitCode::from(3);
        }
        Err(e) => {
            println!("REFUSED: the path file does not read back: {e}");
            return ExitCode::from(3);
        }
    }
    if let Some(dir) = std::path::Path::new(out).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(out, text) {
        eprintln!("refghost: cannot write {out}: {e}");
        return ExitCode::from(1);
    }
    println!(
        "PATH WRITTEN: {out} ({} steps, {} {}, finish {}, read back identical)",
        p.rows.len() - 1,
        TRACKS[usize::from(p.track)].name,
        p.class.name(),
        capture::time(p.finish)
    );
    ExitCode::SUCCESS
}

// ---- The corners of a track -----------------------------------------------------------

/// A corner: segments that turn the same way, at least `BEND` angle
/// units each (1.4 degrees in 8 m).
#[derive(Clone, Debug)]
struct Corner {
    name: String,
    /// Start (fine units along the lap) and length.
    a: i64,
    len: i64,
    /// Turn in degrees (right +) and the lowest planned rival speed in it.
    deg: f64,
    sign: i32,
    fast: u8,
}

const BEND: i32 = 250;

fn corners(tr: &Track) -> Vec<Corner> {
    let n = tr.n();
    let bend: Vec<i32> = (0..n)
        .map(|j| turn(tr.seg(j).yaw, tr.seg(j + 1).yaw))
        .collect();
    let sg = |b: i32| {
        if b >= BEND {
            1
        } else if b <= -BEND {
            -1
        } else {
            0
        }
    };
    let start = (0..n).find(|&j| sg(bend[j]) == 0).unwrap_or(0);
    let mut out = Vec::new();
    let mut cur: Option<(usize, i32, usize)> = None;
    let close = |a: usize, k: usize, sign: i32, out: &mut Vec<Corner>| {
        let segs: Vec<usize> = (a..a + k).map(|j| j % n).collect();
        let sum: i32 = segs.iter().map(|&j| bend[j]).sum();
        let deg = f64::from(sum) * 360.0 / 65_536.0;
        if deg.abs() >= 12.0 {
            out.push(Corner {
                name: String::new(),
                a: ((a % n) as i64) << SEG_BITS,
                len: (k as i64) << SEG_BITS,
                deg,
                sign,
                fast: segs.iter().map(|&j| tr.seg(j).fast).min().unwrap_or(255),
            });
        }
    };
    for k in 0..=n {
        let j = start + k;
        let g = if k == n { 0 } else { sg(bend[j % n]) };
        cur = match cur {
            Some((a, s, len)) if g == s => Some((a, s, len + 1)),
            Some((a, s, len)) => {
                close(a, len, s, &mut out);
                if g != 0 { Some((j, g, 1)) } else { None }
            }
            None if g != 0 => Some((j, g, 1)),
            None => None,
        };
    }
    out.sort_by_key(|c| c.a);
    for (i, c) in out.iter_mut().enumerate() {
        c.name = format!("T{}", i + 1);
    }
    out
}

/// The parts of a lap: (start, end, label, corner index or none) in
/// fine units from the line, in order, covering the lap.
fn zones(tr: &Track, cs: &[Corner]) -> Vec<(i64, i64, String, Option<usize>)> {
    let len = i64::from(tr.len());
    let mut marks: Vec<i64> = vec![0, len];
    for c in cs {
        marks.push(c.a);
        marks.push((c.a + c.len).min(len));
        if c.a + c.len > len {
            marks.push(c.a + c.len - len);
        }
    }
    marks.sort_unstable();
    marks.dedup();
    let mut out = Vec::new();
    for w in marks.windows(2) {
        let (a, b) = (w[0], w[1]);
        let inside = cs.iter().position(|c| {
            let r = (a - c.a).rem_euclid(len);
            r < c.len
        });
        let label = match inside {
            Some(i) => cs[i].name.clone(),
            None => {
                let next = cs
                    .iter()
                    .min_by_key(|c| (c.a - a).rem_euclid(len))
                    .map_or("line".to_string(), |c| c.name.clone());
                format!("to {next}")
            }
        };
        out.push((a, b, label, inside));
    }
    out
}

// ---- One race, sampled ---------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Sample {
    tick: u32,
    prog: i64,
    x: i32,
    v: f64,
    ctl: Ctl,
    carve: i8,
    sling: u8,
    sling_step: u8,
    boost: u8,
    rate: i32,
    fall: u8,
    hit: u8,
    heat: i32,
}

fn sample(tick: u32, sh: &Ship, len: i32) -> Sample {
    Sample {
        tick,
        prog: sh.prog(len),
        x: sh.x,
        v: mps(sh),
        ctl: sh.ctl,
        carve: sh.carve,
        sling: sh.sling,
        sling_step: sh.sling_step,
        boost: sh.boost,
        rate: sh.rate,
        fall: sh.fall,
        hit: sh.hit,
        heat: sh.heat,
    }
}

/// A sampled race: every ship's state after every step, until it
/// finished (the finishing step included).
struct Run {
    track: u8,
    samples: Vec<Vec<Sample>>,
    laps: Vec<Vec<u32>>,
    finish: Vec<u32>,
    best: Vec<u32>,
    pace: Vec<i16>,
    walls: Vec<u32>,
    /// Ticks a rival was inside the bump box of ship 0 (would have
    /// touched it), and the separate times that began.
    overlap: Vec<(u32, u32)>,
    /// Steps of the path where the physics moved ship 0 elsewhere than the
    /// path (the real race's contacts on the player), of all path steps.
    moved_off: (u32, u32),
    /// The race check right after the path's last step.
    check_at_end: u32,
    /// Rival against rival: separate times two rivals came inside each
    /// other's bump box (both racing, neither falling nor a ghost).
    rr_contacts: u32,
    /// Ticks each ship limped.
    limp: Vec<u32>,
    /// The highest speed each rival reached, over the top its ship
    /// (with catch-up and boost pads) allowed, fine units a tick.
    over_top: Vec<i32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Contacts {
    None,
    OneWay,
    /// Ship 0 is held out of the race (no path): the rivals alone.
    Alone,
}

/// The bump box of `race.rs` (`BUMP_S`, `BUMP_X`; 2 m of height).
const BUMP_S: i32 = FM * 42 / 10;
const BUMP_X: i32 = FM * 24 / 10;

fn run(track: u8, class: Class, seed: u32, path: Option<&PathFile>, contacts: Contacts) -> Run {
    let mut race = Race::new();
    setup(&mut race, track, class, seed);
    let len = race.tr().len();
    if let Some(p) = path {
        assert_eq!(
            race.ships[0], p.rows[0].1,
            "the path's grid is not this race's grid"
        );
    }
    let last = path.map_or(0, |p| p.rows.len() - 1);
    let limit = last as u32 + 60 * 600;
    let mut samples = vec![Vec::new(); SHIPS];
    let mut laps = vec![Vec::new(); SHIPS];
    let mut lap_start = [0u32; SHIPS];
    let mut walls = vec![0u32; SHIPS];
    let mut overlap = vec![(0u32, 0u32); SHIPS];
    let mut was_over = [false; SHIPS];
    let mut stopped = [false; SHIPS];
    let mut moved_off = (0u32, 0u32);
    let mut check_at_end = 0;
    let mut rr_contacts = 0u32;
    let mut rr_was = [[false; SHIPS]; SHIPS];
    let mut limp = vec![0u32; SHIPS];
    let mut over_top = vec![i32::MIN; SHIPS];
    let mut k = 0usize;
    loop {
        k += 1;
        let ctl = match path {
            Some(p) if k <= last => {
                race.ships[0] = p.rows[k - 1].1;
                p.rows[k].0
            }
            _ => Ctl::default(),
        };
        match contacts {
            Contacts::None => race.ships[0].ghost = u8::MAX,
            Contacts::Alone => {
                let sh = &mut race.ships[0];
                sh.fall = 1;
                sh.vf = 0;
                sh.vl = 0;
                sh.vh = 0;
            }
            Contacts::OneWay => {}
        }
        let paces: Vec<i32> = (0..SHIPS).map(|i| race.pace_of(i)).collect();
        race.step(Some(ctl));
        while let Some(e) = race.take_ev() {
            if let Ev::Wall { ship } = e {
                walls[usize::from(ship)] += 1;
                if ship > 0 && std::env::var_os("REFGHOST_WALLS").is_some() {
                    let sh = race.ships[usize::from(ship)];
                    let tr = race.tr();
                    let k = tr.index(sh.s);
                    let bend = camber::fixed::turn(tr.seg(k).yaw, tr.seg(k + 3).yaw);
                    eprintln!(
                        "WALL {} {} lap {} s {:.0} x {:+.1} half {:.1} v {:.1} vl {:+.1} fast {} carve {} heat {} sling {}/{} brk {} bend3 {} psi {} shield {}",
                        TRACKS[usize::from(track)].name,
                        name(usize::from(ship)),
                        sh.lap,
                        f64::from(sh.s) / f64::from(FM),
                        f64::from(sh.x) / f64::from(FM),
                        f64::from(tr.at(sh.s).half) / 256.0,
                        f64::from(sh.vf) * 60.0 / f64::from(FM),
                        f64::from(sh.vl) * 60.0 / f64::from(FM),
                        tr.seg(k).fast,
                        sh.carve,
                        sh.heat,
                        sh.sling_step,
                        sh.sling,
                        sh.ctl.brake_l,
                        bend,
                        sh.psi,
                        sh.shield
                    );
                }
            }
        }
        // REFGHOST_ONLY=<track>,<seed> keeps the trace to one rivals-alone race.
        let only = std::env::var("REFGHOST_ONLY")
            .ok()
            .is_none_or(|v| path.is_none() && v == format!("{track},{seed}"));
        if let Some(t) = std::env::var("REFGHOST_TRACE")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|_| only)
        {
            let sh = race.ships[t];
            let tr = race.tr();
            let k = tr.index(sh.s);
            eprintln!(
                "T {} lap {} s {:.1} x {:+.2} line {:+.2} half {:.1} v {:.1} vl {:+.1} psi {} rate {} carve {} heat {} sling {}/{} rest {} st {} th {} br {} cv {:?} fast {} bend3 {}",
                race.tick,
                sh.lap,
                f64::from(sh.s) / f64::from(FM),
                f64::from(sh.x) / f64::from(FM),
                f64::from(tr.line(sh.s)) / f64::from(FM),
                f64::from(tr.at(sh.s).half) / 256.0,
                f64::from(sh.vf) * 60.0 / f64::from(FM),
                f64::from(sh.vl) * 60.0 / f64::from(FM),
                sh.psi,
                sh.rate,
                sh.carve,
                sh.heat,
                sh.sling_step,
                sh.sling,
                sh.rest,
                sh.ctl.steer,
                u8::from(sh.ctl.thrust),
                u8::from(sh.ctl.brake_l),
                sh.ctl.carve,
                tr.seg(k).fast,
                turn(tr.seg(k).yaw, tr.seg(k + 3).yaw)
            );
        }
        for a in 1..SHIPS {
            let sa = race.ships[a];
            if sa.limp {
                limp[a] += 1;
            }
            // No faster than the ship can: the top of its pace (boost pad
            // 1.3, sling 1.5 of it), or what a bump or a slope gave it.
            let top = race.class.top() * paces[a] / 1024;
            let allowed = if sa.boost > 0 { top * 13 / 10 } else { top };
            let allowed = allowed.max(top * camber::race::REWARD.top[2] / 1024);
            over_top[a] = over_top[a].max(sa.vf - allowed);
            for (b, was) in rr_was[a].iter_mut().enumerate().skip(a + 1) {
                let sb = race.ships[b];
                let on = !sa.finished()
                    && !sb.finished()
                    && sa.fall == 0
                    && sb.fall == 0
                    && sa.ghost == 0
                    && sb.ghost == 0
                    && gap(sa.s, sb.s, len).abs() < BUMP_S
                    && (sa.x - sb.x).abs() < BUMP_X
                    && (sa.h - sb.h).abs() <= FM * 2;
                if on && !*was {
                    rr_contacts += 1;
                }
                *was = on;
            }
        }
        if let Some(p) = path.filter(|_| k <= last) {
            let mut moved = race.ships[0];
            let want = p.rows[k].1;
            moved.ghost = want.ghost;
            moved_off.1 += 1;
            if moved != want {
                moved_off.0 += 1;
            }
            race.ships[0] = want;
            if contacts == Contacts::None {
                race.ships[0].ghost = u8::MAX;
            }
            if want.finished() && race.phase == Phase::Run {
                race.phase = Phase::Done;
            }
            if k == last {
                check_at_end = rec::race_check(&race);
            }
        }
        let tick = race.tick;
        let o = race.ships[0];
        for i in 0..SHIPS {
            let sh = race.ships[i];
            if !stopped[i] && (i > 0 || contacts != Contacts::Alone) {
                samples[i].push(sample(tick, &sh, len));
                stopped[i] = sh.finished();
            }
            if sh.lap_start != lap_start[i] {
                if lap_start[i] != 0 {
                    laps[i].push(sh.lap_start - lap_start[i]);
                }
                lap_start[i] = sh.lap_start;
            }
            if i > 0 && path.is_some() && !sh.finished() {
                let ds = gap(o.s, sh.s, len);
                let over = ds.abs() < BUMP_S
                    && (sh.x - o.x).abs() < BUMP_X
                    && (sh.h - o.h).abs() <= FM * 2
                    && sh.fall == 0
                    && o.fall == 0
                    && (k <= last);
                if over {
                    overlap[i].0 += 1;
                    if !was_over[i] {
                        overlap[i].1 += 1;
                    }
                }
                was_over[i] = over;
            }
        }
        let all = race.ships[1..].iter().all(Ship::finished)
            && (contacts == Contacts::Alone || race.ships[0].finished());
        if (all && k >= last) || race.tick >= limit {
            break;
        }
    }
    Run {
        track,
        samples,
        laps,
        finish: race.ships.iter().map(|s| s.finish).collect(),
        best: race.ships.iter().map(|s| s.best).collect(),
        pace: race.ships.iter().map(|s| s.pace).collect(),
        walls,
        overlap,
        moved_off,
        check_at_end,
        rr_contacts,
        limp,
        over_top,
    }
}

fn gap(a: i32, b: i32, len: i32) -> i32 {
    let ds = b - a;
    if ds > len / 2 {
        ds - len
    } else if ds < -len / 2 {
        ds + len
    } else {
        ds
    }
}

// ---- Measures on samples ---------------------------------------------------------------

/// The tick (with a fraction) a ship first reached `p` along the race.
fn cross(sm: &[Sample], p: i64) -> Option<f64> {
    let k = sm.iter().position(|q| q.prog >= p)?;
    if k == 0 {
        return Some(f64::from(sm[0].tick));
    }
    let (a, b) = (&sm[k - 1], &sm[k]);
    let d = (b.prog - a.prog) as f64;
    if d <= 0.0 {
        return Some(f64::from(b.tick));
    }
    Some(f64::from(a.tick) + (p - a.prog) as f64 / d * f64::from(b.tick - a.tick))
}

/// The samples between two places along the race (first reach of each).
fn between(sm: &[Sample], a: i64, b: i64) -> &[Sample] {
    let i = sm.iter().position(|q| q.prog >= a).unwrap_or(sm.len());
    let j = sm.iter().position(|q| q.prog >= b).unwrap_or(sm.len());
    if j <= i { &sm[i..i] } else { &sm[i..j] }
}

/// One pass through a corner.
#[derive(Clone, Copy, Debug, Default)]
struct Pass {
    entry: f64,
    min: f64,
    exit: f64,
    /// Ticks from the corner's start to its end.
    time: f64,
    /// Where the speed was lowest, metres from the corner's start, and
    /// how far to the inside the ship was there (m).
    min_at: f64,
    inside: f64,
    /// The first both-brakes and the first thrust-off tick on the
    /// approach (from the last corner's end) or in the corner, metres
    /// from the corner's start; ticks of brakes.
    brake_at: Option<f64>,
    lift_at: Option<f64>,
    brake_ticks: u32,
    carved: bool,
    sling: u8,
}

fn pass(sm: &[Sample], from: i64, a: i64, b: i64, sign: i32) -> Option<Pass> {
    let ta = cross(sm, a)?;
    let tb = cross(sm, b)?;
    let inner = between(sm, a, b);
    let appr = between(sm, from, b);
    let first = inner.first()?;
    let lo = inner
        .iter()
        .min_by(|x, y| x.v.total_cmp(&y.v))
        .copied()
        .unwrap_or(*first);
    let end = sm.iter().find(|q| q.prog >= b)?;
    let rel = |q: &Sample| (q.prog - a) as f64 / f64::from(FM);
    Some(Pass {
        entry: first.v,
        min: lo.v,
        exit: end.v,
        time: tb - ta,
        min_at: rel(&lo),
        inside: m(lo.x) * f64::from(sign),
        brake_at: appr
            .iter()
            .find(|q| q.ctl.brake_l && q.ctl.brake_r)
            .map(rel),
        lift_at: appr.iter().find(|q| !q.ctl.thrust).map(rel),
        brake_ticks: appr
            .iter()
            .filter(|q| q.ctl.brake_l && q.ctl.brake_r)
            .count() as u32,
        carved: inner.iter().any(|q| q.carve != 0),
        sling: appr.iter().map(|q| q.sling_step).max().unwrap_or(0),
    })
}

/// Counts of what a ship did with its buttons in a race.
#[derive(Clone, Copy, Debug, Default)]
struct Use {
    ticks: u32,
    carve_presses: u32,
    carves: u32,
    slings: [u32; 3],
    sling_push: u32,
    pads: u32,
    brake_ticks: u32,
    one_brake: u32,
    lift_ticks: u32,
    full_lock: u32,
    knocks: u32,
    /// Steering on ticks not carving: stick flips (beyond 32 of 256) and
    /// roll flips (the nose's turn rate changes side beyond 100 units a
    /// tick), as `tests/steer.rs rivals_steer` counts them.
    plain_ticks: u32,
    flips: u32,
    rolls: u32,
    max_heat: i32,
}

fn usage(sm: &[Sample]) -> Use {
    let mut u = Use::default();
    let mut prev: Option<Sample> = None;
    let (mut last_s, mut last_r) = (0, 0);
    for q in sm.iter().filter(|q| q.tick > COUNT) {
        u.ticks += 1;
        let p = prev.unwrap_or(*q);
        if q.ctl.carve.pressed() && !p.ctl.carve.pressed() {
            u.carve_presses += 1;
        }
        if q.carve != 0 && p.carve == 0 {
            u.carves += 1;
        }
        if q.sling_step > 0 && q.sling > p.sling {
            u.slings[usize::from(q.sling_step - 1)] += 1;
        }
        if q.sling > 0 && !q.ctl.brake_l && !q.ctl.brake_r {
            u.sling_push += 1;
        }
        if q.boost > p.boost && p.boost < 65 && prev.is_some() {
            u.pads += 1;
        }
        if q.ctl.brake_l && q.ctl.brake_r {
            u.brake_ticks += 1;
        }
        if q.ctl.brake_l != q.ctl.brake_r {
            u.one_brake += 1;
        }
        if !q.ctl.thrust {
            u.lift_ticks += 1;
        }
        if q.ctl.steer.abs() >= 256 {
            u.full_lock += 1;
        }
        if q.hit == 8 && p.hit != 8 && prev.is_some() {
            u.knocks += 1;
        }
        u.max_heat = u.max_heat.max(q.heat);
        if q.carve == 0 && q.fall == 0 {
            u.plain_ticks += 1;
            let s = q.ctl.steer;
            if s.abs() > 32 {
                if last_s != 0 && s.signum() != last_s {
                    u.flips += 1;
                }
                last_s = s.signum();
            }
            if q.rate.abs() > 100 {
                if last_r != 0 && q.rate.signum() != last_r {
                    u.rolls += 1;
                }
                last_r = q.rate.signum();
            }
        }
        prev = Some(*q);
    }
    u
}

fn secs(t: f64) -> String {
    let cs = (t * 100.0 / 60.0).round() as i64;
    format!("{}:{:02}.{:02}", cs / 6000, cs / 100 % 60, cs % 100)
}

fn opt_mean(v: &[Option<f64>]) -> (Option<f64>, usize) {
    let got: Vec<f64> = v.iter().flatten().copied().collect();
    if got.is_empty() {
        (None, 0)
    } else {
        (Some(got.iter().sum::<f64>() / got.len() as f64), got.len())
    }
}

fn fmt_at(x: (Option<f64>, usize), laps: usize) -> String {
    match x.0 {
        Some(v) if x.1 == laps => format!("{v:>+6.0}  "),
        Some(v) => format!("{v:>+6.0}/{}", x.1),
        None => "     -  ".into(),
    }
}

// ---- The ghost report ------------------------------------------------------------------

/// Per ship, per corner, one pass a lap.
fn passes(r: &Run, i: usize, cs: &[Corner]) -> Vec<Vec<Option<Pass>>> {
    let len = i64::from(TRACKS[usize::from(r.track)].len());
    let sm = &r.samples[i];
    cs.iter()
        .enumerate()
        .map(|(ci, c)| {
            let prev_end = if ci == 0 {
                cs[cs.len() - 1].a + cs[cs.len() - 1].len - len
            } else {
                cs[ci - 1].a + cs[ci - 1].len
            };
            (0..i64::from(LAPS))
                .map(|lap| {
                    let a = lap * len + c.a;
                    pass(sm, (lap * len + prev_end).max(0), a, a + c.len, c.sign)
                })
                .collect()
        })
        .collect()
}

struct Report {
    text: String,
    tsv: String,
}

fn name(i: usize) -> &'static str {
    ENTRANTS[i].name
}

fn ghost_report(p: &PathFile, contacts: Contacts) -> Report {
    let tr = &TRACKS[usize::from(p.track)];
    let len = i64::from(tr.len());
    let cs = corners(tr);
    let zs = zones(tr, &cs);
    let r = run(p.track, p.class, p.seed, Some(p), contacts);
    let mut s = String::new();
    let mut tsv = String::new();
    let _ = writeln!(
        s,
        "REFERENCE GHOST: {} {} seed {:08x}, path {} ({} steps), contacts {}",
        tr.name,
        p.class.name(),
        p.seed,
        p.source,
        p.rows.len() - 1,
        match contacts {
            Contacts::None =>
                "NONE (the path is a ghost: nothing touches it, the rivals see it and steer round it)",
            Contacts::OneWay =>
                "ONE-WAY (bumps as in the game, only the rival keeps its side; the path is not pushed)",
            Contacts::Alone => "ALONE",
        }
    );
    // Self-test: with one-way contacts and today's code, the run is the
    // console's race.
    let selftest = run(p.track, p.class, p.seed, Some(p), Contacts::OneWay);
    let _ = writeln!(
        s,
        "self-test (contacts one-way): race check after the path's last step {:08x}, the console's {:08x}: {}",
        selftest.check_at_end,
        p.check,
        if selftest.check_at_end == p.check {
            "SAME: the placement gives back the real race exactly (today's rivals)"
        } else {
            "DIFFERENT: the rivals or the physics are not the code that raced the path (expected after a rival change)"
        }
    );
    let _ = writeln!(
        s,
        "path placement in this run: the physics moved the player's ship somewhere else than the path on {} of {} steps (the real race's contacts on the player; the path wins)",
        r.moved_off.0, r.moved_off.1
    );
    let laps: Vec<String> = p.laps.iter().map(|&t| capture::time(t)).collect();
    let _ = writeln!(
        s,
        "OWNER (the path): finish {} place {} laps [{}]; lap {} m",
        capture::time(p.finish),
        p.place,
        laps.join(", "),
        len / i64::from(FM)
    );
    let own = &r.samples[0];
    let own_finish = f64::from(p.finish);
    // Rival table.
    let _ = writeln!(
        s,
        "\nship       pace  finish    gap s   gap %   best lap  laps                      top m/s  first line  touch ticks/times  {}",
        (1..=8).map(|k| format!("  8th{k}")).collect::<String>()
    );
    let _ = writeln!(
        tsv,
        "class\tship\tname\tpace\tfinish_ticks\tgap_s\tgap_pct\tbest_ticks\tlap1\tlap2\tlap3\ttop_mps\tfirst_line_s\toverlap_ticks"
    );
    let first_line = |sm: &[Sample]| cross(sm, 0);
    let own_line = first_line(own).unwrap_or(0.0);
    let eighth = |sm: &[Sample], b: usize| cross(sm, b as i64 * len / 8);
    for i in 0..SHIPS {
        let sm = &r.samples[i];
        let fin = if i == 0 {
            own_finish
        } else {
            f64::from(r.finish[i])
        };
        let top = sm
            .iter()
            .filter(|q| q.tick > COUNT)
            .map(|q| q.v)
            .fold(0.0, f64::max);
        let fl = first_line(sm).map_or(f64::NAN, |t| (t - own_line) / 60.0);
        let lapl: Vec<String> = r.laps[i].iter().map(|&t| capture::time(t)).collect();
        let mut sect = String::new();
        let mut sect_v = [0.0f64; 8];
        for b in 1..=(LAPS as usize) * 8 {
            let d = |q: usize| -> Option<f64> { Some(eighth(sm, q)? - eighth(own, q)?) };
            if let (Some(x), Some(y)) = (d(b), d(b - 1)) {
                sect_v[(b - 1) % 8] += x - y;
            }
        }
        for v in sect_v {
            let _ = write!(sect, "{:>+7.2}", v / 60.0);
        }
        let gap = (fin - own_finish) / 60.0;
        let (ov, ovn) = r.overlap[i];
        let _ = writeln!(
            s,
            "{:<9} {:>5}  {:<8} {:>+7.2} {:>+7.1}%  {:<8}  {:<24}  {:>6.1}  {:>+9.2}  {:>6}/{:<3}         {}",
            if i == 0 { "OWNER" } else { name(i) },
            r.pace[i],
            if fin > 0.0 { secs(fin) } else { "-".into() },
            gap,
            gap * 6000.0 / own_finish,
            capture::time(if i == 0 {
                *p.laps.iter().min().unwrap_or(&0)
            } else {
                r.best[i]
            }),
            lapl.join(" "),
            top,
            fl,
            ov,
            ovn,
            if i == 0 { String::new() } else { sect }
        );
        let lap = |k: usize| r.laps[i].get(k).copied().unwrap_or(0);
        let _ = writeln!(
            tsv,
            "{}\t{i}\t{}\t{}\t{}\t{:.3}\t{:.2}\t{}\t{}\t{}\t{}\t{:.2}\t{:.3}\t{}",
            p.class.name(),
            if i == 0 { "OWNER" } else { name(i) },
            r.pace[i],
            fin,
            gap,
            gap * 6000.0 / own_finish,
            if i == 0 {
                *p.laps.iter().min().unwrap_or(&0)
            } else {
                r.best[i]
            },
            lap(0),
            lap(1),
            lap(2),
            top,
            fl,
            ov
        );
    }
    let _ = writeln!(
        s,
        "pace: the ship's own top and corner pace, 1/1024 (race::PACE, the class's skill row sets the rest). gap: finish behind the owner (+). first line: seconds behind (+) the owner at the first crossing of the line (the owner starts last on the grid). touch: ticks a rival was inside the bump box of the owner's ship / separate times (it passed through: no contacts). 8thN: seconds lost (+) or gained (-) on the owner in that eighth of the lap, three laps added."
    );

    // Corners.
    let _ = writeln!(
        s,
        "\nCORNERS of {} (segments turning the same way, at least {:.1} degrees in 8 m; fast = the bake's lowest planned rival speed in it):",
        tr.name,
        f64::from(BEND) * 360.0 / 65_536.0
    );
    for c in &cs {
        let _ = writeln!(
            s,
            "  {:<4} {:>5.0}-{:<5.0} m  turn {:>+5.0} deg  {:>4} m  fast {:>3} m/s",
            c.name,
            c.a as f64 / f64::from(FM),
            (c.a + c.len) as f64 / f64::from(FM),
            c.deg,
            c.len / i64::from(FM),
            c.fast
        );
    }
    let all: Vec<Vec<Vec<Option<Pass>>>> = (0..SHIPS).map(|i| passes(&r, i, &cs)).collect();
    let _ = writeln!(
        s,
        "\nPER CORNER, mean of the three laps (speeds m/s; time = seconds in the corner, three laps added, and lost (+) on the owner; brake / lift = metres from the corner's start (- before it) of the first both-brakes / thrust-off tick from the last corner's end on, /N after it = only on N of the 3 laps; brk = brake ticks a lap; in = metres to the inside at the slowest point; carve = laps carved in it; sling = highest sling step running on the approach):"
    );
    for (ci, c) in cs.iter().enumerate() {
        let _ = writeln!(
            s,
            "{} ({:+.0} deg, {:.0} m from {:.0} m, fast {} m/s)\n  ship      entry   min  exit  min@m    in   brake     lift    brk   time s  lost s  carve sling",
            c.name,
            c.deg,
            c.len as f64 / f64::from(FM),
            c.a as f64 / f64::from(FM),
            c.fast
        );
        let own_time: f64 = all[0][ci].iter().flatten().map(|x| x.time).sum();
        for (i, per) in all.iter().enumerate() {
            let ps: Vec<Pass> = per[ci].iter().flatten().copied().collect();
            if ps.is_empty() {
                continue;
            }
            let n = ps.len() as f64;
            let mean = |f: fn(&Pass) -> f64| ps.iter().map(f).sum::<f64>() / n;
            let t: f64 = ps.iter().map(|x| x.time).sum();
            let br: Vec<Option<f64>> = ps.iter().map(|x| x.brake_at).collect();
            let li: Vec<Option<f64>> = ps.iter().map(|x| x.lift_at).collect();
            let _ = writeln!(
                s,
                "  {:<9} {:>5.1} {:>5.1} {:>5.1} {:>6.0} {:>5.1} {} {} {:>4.0} {:>8.2} {:>+7.2}  {:>3}   {:>3}",
                if i == 0 { "OWNER" } else { name(i) },
                mean(|x| x.entry),
                mean(|x| x.min),
                mean(|x| x.exit),
                mean(|x| x.min_at),
                mean(|x| x.inside),
                fmt_at(opt_mean(&br), ps.len()),
                fmt_at(opt_mean(&li), ps.len()),
                ps.iter().map(|x| f64::from(x.brake_ticks)).sum::<f64>() / n,
                t / 60.0,
                (t - own_time) / 60.0,
                ps.iter().filter(|x| x.carved).count(),
                ps.iter().map(|x| x.sling).max().unwrap_or(0),
            );
        }
    }

    // Zones: where the time goes.
    let _ = writeln!(
        s,
        "\nTIME LOST (+) ON THE OWNER PER PART OF THE LAP, seconds, three laps added (the parts add up to the gap at the finish, less the start: the grid to the first line):"
    );
    let _ = write!(s, "  part           m       owner s  top m/s ");
    for i in 1..SHIPS {
        let _ = write!(s, " {:>8}", name(i));
    }
    let _ = writeln!(s, "   rivals' top m/s (mean)");
    let zone_time = |sm: &[Sample], z: &(i64, i64, String, Option<usize>)| -> Option<f64> {
        let mut t = 0.0;
        for lap in 0..i64::from(LAPS) {
            t += cross(sm, lap * len + z.1)? - cross(sm, lap * len + z.0)?;
        }
        Some(t)
    };
    let zone_top = |sm: &[Sample], z: &(i64, i64, String, Option<usize>)| -> f64 {
        let mut top: f64 = 0.0;
        for lap in 0..i64::from(LAPS) {
            for q in between(sm, lap * len + z.0, lap * len + z.1) {
                top = top.max(q.v);
            }
        }
        top
    };
    let mut start_line = String::from("  start (grid to line)                ");
    for i in 1..SHIPS {
        let d = first_line(&r.samples[i]).map(|t| (t - own_line) / 60.0);
        let _ = write!(start_line, " {:>+8.2}", d.unwrap_or(f64::NAN));
    }
    let _ = writeln!(s, "{start_line}");
    for z in &zs {
        let ot = zone_time(own, z);
        let _ = write!(
            s,
            "  {:<9} {:>5.0}-{:<5.0} {:>7.2} {:>7.1} ",
            z.2,
            z.0 as f64 / f64::from(FM),
            z.1 as f64 / f64::from(FM),
            ot.unwrap_or(f64::NAN) / 60.0,
            zone_top(own, z)
        );
        let mut tops = 0.0;
        for i in 1..SHIPS {
            let rt = zone_time(&r.samples[i], z);
            let d = match (rt, ot) {
                (Some(a), Some(b)) => (a - b) / 60.0,
                _ => f64::NAN,
            };
            tops += zone_top(&r.samples[i], z);
            let _ = write!(s, " {d:>+8.2}");
        }
        let _ = writeln!(s, "   {:>6.1}", tops / (SHIPS - 1) as f64);
    }

    // The start.
    let _ = writeln!(
        s,
        "\nTHE START (from GO): speed m/s 1 s and 2 s after GO, metres covered in the first 3 s, and seconds to cover the first 100 m from the ship's own grid place:"
    );
    for i in 0..SHIPS {
        let sm = &r.samples[i];
        let at = |t: u32| sm.iter().find(|q| q.tick == COUNT + t);
        let p0 = sm.iter().find(|q| q.tick == COUNT).map_or(0, |q| q.prog);
        let t100 = cross(sm, p0 + 100 * i64::from(FM)).map(|t| (t - f64::from(COUNT)) / 60.0);
        let _ = writeln!(
            s,
            "  {:<9} {:>5.1} {:>5.1}  {:>5.0} m  {:>5.2} s",
            if i == 0 { "OWNER" } else { name(i) },
            at(60).map_or(f64::NAN, |q| q.v),
            at(120).map_or(f64::NAN, |q| q.v),
            at(180).map_or(f64::NAN, |q| (q.prog - p0) as f64 / f64::from(FM)),
            t100.unwrap_or(f64::NAN)
        );
    }

    // Buttons.
    let _ = writeln!(
        s,
        "\nWHAT EACH SHIP DID WITH ITS BUTTONS (GO to its finish; the owner's from the recorded buttons):\n  ship      carve presses carves  slings 1/2/3  sling push s  pads  brake s  one-side brake ticks  lift s  full-lock s  knocks  max heat deg | stick flips/s  roll flips/s (not carving)"
    );
    for i in 0..SHIPS {
        let u = usage(&r.samples[i]);
        let ps = f64::from(u.plain_ticks.max(1)) / 60.0;
        let _ = writeln!(
            s,
            "  {:<9} {:>6} {:>12}  {:>3}/{:>2}/{:>2}  {:>11.1}  {:>5}  {:>7.1}  {:>19}  {:>6.1}  {:>11.1}  {:>6}  {:>12.1} | {:>13.2}  {:>12.2}",
            if i == 0 { "OWNER" } else { name(i) },
            u.carve_presses,
            u.carves,
            u.slings[0],
            u.slings[1],
            u.slings[2],
            f64::from(u.sling_push) / 60.0,
            u.pads,
            f64::from(u.brake_ticks) / 60.0,
            u.one_brake,
            f64::from(u.lift_ticks) / 60.0,
            f64::from(u.full_lock) / 60.0,
            u.knocks,
            f64::from(u.max_heat) * 360.0 / 65_536.0,
            f64::from(u.flips) / ps,
            f64::from(u.rolls) / ps,
        );
    }
    let _ = writeln!(
        s,
        "walls (Ev::Wall, hard hits; may miss one when another event of the same ship wins the tick): {}",
        (1..SHIPS)
            .map(|i| format!("{} {}", name(i), r.walls[i]))
            .collect::<Vec<_>>()
            .join(", ")
    );
    Report { text: s, tsv }
}

fn cmd_ghost(paths: &[PathFile], contacts: Contacts, tsv_out: Option<&str>) -> ExitCode {
    let mut tsv_all = String::new();
    for (k, p) in paths.iter().enumerate() {
        let rep = ghost_report(p, contacts);
        print!("{}", rep.text);
        println!();
        if k == 0 {
            tsv_all.push_str(&rep.tsv);
        } else {
            tsv_all.push_str(rep.tsv.split_once('\n').map_or("", |x| x.1));
        }
    }
    if let Some(f) = tsv_out
        && let Err(e) = std::fs::write(f, tsv_all)
    {
        eprintln!("refghost: cannot write {f}: {e}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

/// One line of a run: the leader and the front and the pack behind it.
fn brief_line(r: &Run, own: Option<u32>) -> String {
    let mut by_pace: Vec<usize> = (1..SHIPS).collect();
    by_pace.sort_by_key(|&i| (-i32::from(r.pace[i]), i));
    let front = &by_pace[..3];
    let pack = &by_pace[3..];
    let fin = |i: usize| f64::from(r.finish[i]);
    let lead = (1..SHIPS)
        .filter(|&i| r.finish[i] > 0)
        .min_by_key(|&i| r.finish[i])
        .unwrap_or(1);
    let mut fr: Vec<f64> = front.iter().map(|&i| fin(i)).collect();
    fr.sort_by(f64::total_cmp);
    let fmean = fr.iter().sum::<f64>() / 3.0;
    let mut out = String::new();
    let _ = write!(out, "lead {} {}", name(lead), capture::time(r.finish[lead]));
    if let Some(o) = own {
        let _ = write!(
            out,
            " ({:+.2} s {:+.1}%)",
            (fin(lead) - f64::from(o)) / 60.0,
            (fin(lead) - f64::from(o)) * 100.0 / f64::from(o)
        );
    }
    let _ = write!(
        out,
        " | fronts +{:.2} +{:.2} s",
        (fr[1] - fr[0]) / 60.0,
        (fr[2] - fr[0]) / 60.0
    );
    let mut pk: Vec<f64> = pack
        .iter()
        .map(|&i| (fin(i) - fmean) * 100.0 / fmean)
        .collect();
    pk.sort_by(f64::total_cmp);
    let _ = write!(
        out,
        " | pack {}% behind front mean",
        pk.iter()
            .map(|v| format!("{v:+.1}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let walls: u32 = r.walls[1..].iter().sum();
    let limp: u32 = r.limp[1..].iter().sum();
    let over = r.over_top[1..].iter().copied().max().unwrap_or(0);
    let _ = write!(
        out,
        " | walls {walls} rr {} limp {limp} over {:+.2} m/s",
        r.rr_contacts,
        f64::from(over) * 60.0 / f64::from(FM)
    );
    out
}

/// `brief`: one line per path (against the owner, contacts none) and one
/// per track (the rivals alone, the path's class and seed).
fn cmd_brief(paths: &[PathFile]) -> ExitCode {
    let c = cfg();
    println!("catch-up {}", c.catchup.unwrap_or(CatchUp::Off).name());
    for p in paths {
        let r = run(p.track, p.class, p.seed, Some(p), Contacts::None);
        println!(
            "{:<6} OWNER {}  {}",
            p.class.name(),
            capture::time(p.finish),
            brief_line(&r, Some(p.finish))
        );
    }
    // Every grid order (seeds 0..6 give the seven shifts), the rivals
    // alone, means: the leader's finish, the fronts behind it, the pack
    // behind the fronts' mean, walls and rival contacts a race.
    for p in paths {
        for t in 0..TRACKS.len() as u8 {
            let (mut lead, mut f2, mut f3, mut pk, mut pkmin, mut w, mut rr) =
                (0.0, 0.0, 0.0, 0.0, f64::MAX, 0u32, 0u32);
            for seed in 0..7u32 {
                let r = run(t, p.class, seed, None, Contacts::Alone);
                let mut by_pace: Vec<usize> = (1..SHIPS).collect();
                by_pace.sort_by_key(|&i| (-i32::from(r.pace[i]), i));
                let mut fr: Vec<f64> = by_pace[..3]
                    .iter()
                    .map(|&i| f64::from(r.finish[i]))
                    .collect();
                fr.sort_by(f64::total_cmp);
                let fm = fr.iter().sum::<f64>() / 3.0;
                let best = (1..SHIPS).map(|i| r.finish[i]).min().unwrap_or(0);
                lead += f64::from(best) / 60.0 / 7.0;
                f2 += (fr[1] - fr[0]) / 60.0 / 7.0;
                f3 += (fr[2] - fr[0]) / 60.0 / 7.0;
                for &i in &by_pace[3..] {
                    let g = (f64::from(r.finish[i]) - fm) * 100.0 / fm;
                    pk += g / 28.0;
                    pkmin = pkmin.min(g);
                }
                w += r.walls[1..].iter().sum::<u32>();
                rr += r.rr_contacts;
            }
            println!(
                "{:<6} {:<8} seeds0-6 lead {} | fronts +{:.2} +{:.2} s | pack mean {:+.1}% min {:+.1}% | walls {:.1} rr {:.1} a race",
                p.class.name(),
                TRACKS[usize::from(t)].name,
                secs(lead * 60.0),
                f2,
                f3,
                pk,
                pkmin,
                f64::from(w) / 7.0,
                f64::from(rr) / 7.0
            );
        }
    }
    for p in paths {
        for t in 0..TRACKS.len() as u8 {
            let r = run(t, p.class, p.seed, None, Contacts::Alone);
            println!(
                "{:<6} {:<8} alone  {}",
                p.class.name(),
                TRACKS[usize::from(t)].name,
                brief_line(&r, None)
            );
        }
    }
    ExitCode::SUCCESS
}

/// `tracks`: the rivals alone on every track, each path's class and seed.
fn cmd_tracks(paths: &[PathFile]) -> ExitCode {
    println!(
        "THE RIVALS ALONE on every track (no player: its ship is held out of the race, kept falling, which no rival sees or touches), RACE, each path's class and seed; ticks of 1/60 s"
    );
    for p in paths {
        let runs: Vec<Run> = (0..TRACKS.len() as u8)
            .map(|t| run(t, p.class, p.seed, None, Contacts::Alone))
            .collect();
        println!("\n{} seed {:08x}", p.class.name(), p.seed);
        println!(
            "  ship       pace  track     lap m  best lap  mean lap  finish    best m/s  best lap / LAGOON's   finish / LAGOON's"
        );
        for i in 1..SHIPS {
            for (t, r) in runs.iter().enumerate() {
                let tr = &TRACKS[t];
                let lm = f64::from(tr.len()) / f64::from(FM);
                let mean = r.laps[i].iter().sum::<u32>() as f64 / r.laps[i].len().max(1) as f64;
                let best = f64::from(r.best[i]);
                let lb = f64::from(runs[0].best[i]);
                let lf = f64::from(runs[0].finish[i]);
                println!(
                    "  {:<9} {:>5}  {:<8} {:>6.0}  {:<8}  {:<8}  {:<8} {:>8.1}  {:>8.3}             {:>8.3}",
                    name(i),
                    r.pace[i],
                    tr.name,
                    lm,
                    capture::time(r.best[i]),
                    secs(mean),
                    capture::time(r.finish[i]),
                    lm / (best / 60.0),
                    best / lb,
                    f64::from(r.finish[i]) / lf
                );
            }
        }
    }
    println!(
        "\nbest m/s: the lap's length over the best lap. / LAGOON's: the same rival (same class, seed and so pace) on LAGOON in the same rivals-alone race. No person has raced FURNACE or GLACIER in a recording: there is no human reference for them."
    );
    ExitCode::SUCCESS
}

fn load(files: &[String]) -> Result<Vec<PathFile>, String> {
    files
        .iter()
        .map(|f| {
            let t = std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?;
            read_path(&t, f)
        })
        .collect()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: refghost path <capture.txt> <OUT.path>\n       refghost ghost <A.path>... [--contacts none|oneway] [--tsv FILE]\n       refghost tracks <A.path>...\n       refghost brief <A.path>... [--catchup off|light] [--set CLASS.field=N]... [--pace a,b,c,d,e,f,g]\n       refghost pilot <A.path>...";
    let Some(cmd) = args.first() else {
        eprintln!("{usage}");
        return ExitCode::from(1);
    };
    let mut files = Vec::new();
    let mut contacts = Contacts::None;
    let mut tsv = None;
    let mut cfg = Cfg::default();
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--contacts" => {
                contacts = match it.next().map(String::as_str) {
                    Some("none") => Contacts::None,
                    Some("oneway") => Contacts::OneWay,
                    x => {
                        eprintln!("refghost: --contacts none|oneway, not {x:?}");
                        return ExitCode::from(1);
                    }
                }
            }
            "--tsv" => tsv = it.next().cloned(),
            "--set" => match it.next().map(|v| parse_set(v)) {
                Some(Ok(x)) => cfg.set.push(x),
                x => {
                    eprintln!("refghost: --set CLASS.field=value: {x:?}");
                    return ExitCode::from(1);
                }
            },
            "--pace" => {
                let v: Option<Vec<i16>> = it
                    .next()
                    .and_then(|v| v.split(',').map(|x| x.parse().ok()).collect());
                match v {
                    Some(v) if v.len() == SHIPS - 1 => cfg.pace = Some(v),
                    _ => {
                        eprintln!("refghost: --pace needs {} numbers", SHIPS - 1);
                        return ExitCode::from(1);
                    }
                }
            }
            "--catchup" => {
                cfg.catchup = match it.next().map(String::as_str) {
                    Some("off") => Some(CatchUp::Off),
                    Some("light") => Some(CatchUp::Light),
                    x => {
                        eprintln!("refghost: --catchup off|light, not {x:?}");
                        return ExitCode::from(1);
                    }
                }
            }
            "-h" | "--help" => {
                println!("{usage}");
                return ExitCode::SUCCESS;
            }
            _ => files.push(a.clone()),
        }
    }
    let _ = CFG.set(cfg);
    match cmd.as_str() {
        "path" if files.len() == 2 => cmd_path(&files[0], &files[1]),
        "ghost" | "tracks" | "pilot" | "brief" if !files.is_empty() => match load(&files) {
            Ok(ps) if cmd == "ghost" => cmd_ghost(&ps, contacts, tsv.as_deref()),
            Ok(ps) if cmd == "brief" => cmd_brief(&ps),
            Ok(ps) if cmd == "pilot" => pilot::cmd_pilot(&ps),
            Ok(ps) => cmd_tracks(&ps),
            Err(e) => {
                eprintln!("refghost: {e}");
                ExitCode::from(2)
            }
        },
        _ => {
            eprintln!("{usage}");
            ExitCode::from(1)
        }
    }
}
