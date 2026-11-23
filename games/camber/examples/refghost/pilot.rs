//! `pilot`: the rivals' own driving (`Race::drive` and `Race::drive_plain`
//! in `race.rs`) copied here with knobs, flying the player's ship (ship 0)
//! through `Race::step(Some(ctl))`. With the knobs at today's values the
//! copy gives exactly the controls `Race::drive` gives (the tool checks that
//! every tick and says SAME), so each knob shows what one change to the
//! rivals' driving would be worth, with no game rule changed. A PC
//! measure of a scripted pilot: never how hard the game is for a person.

use camber::fixed::{hash, isqrt, turn};
use camber::race::{
    CARVE_BEND, CARVE_HOLD_BEND, CARVE_START_SPEED, Carve, Class, Ctl, Ev, Mode, Phase, Race,
    SHIPS, SKILLS, TURN_MAX, carve_stick, heat_step, mps,
};
use camber::track::{BOOST, CHARGE, FM, SEG_BITS, TRACKS, Track};

/// Copies of private numbers of `race.rs` (kept in step by hand; the
/// SAME check fails when they drift): `Class::pace`, `Class::rival_top`,
/// `PACE`, `LANES`.
const CLASS_PACE: [i32; 3] = [940, 1120, 1320];
const RIVAL_TOP: [i32; 3] = [950, 1000, 920];
const PACE: [i32; SHIPS - 1] = [1004, 992, 980, 968, 956, 944, 930];
const LANES: [i8; SHIPS - 1] = [-4, 5, -6, 3, 6, -3, 0];

fn row(c: Class) -> usize {
    match c {
        Class::Steady => 0,
        Class::Brisk => 1,
        Class::Wild => 2,
    }
}

/// What may differ from today's driving.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knobs {
    /// The planned sideways pull, m/s^2 (`GRIP_A` of the bake, 45).
    pub grip: i32,
    /// The planned turn-rate term of the bake (125_163 = 1.5 rad/s); 0: none.
    pub rate: i32,
    /// The planned braking: m/s a segment (8 m) of the bake's back pass (2).
    pub ramp: i32,
    /// The class pace (`Class::pace`), per class.
    pub cpace: [i32; 3],
    /// The third look point (`sh.s + look * 2`) counts.
    pub look2: bool,
    /// Both brakes when over the plan by more than cap / `bdiv` (12), and
    /// by cap / `bdiv_sling` while a sling runs (12).
    pub bdiv: i32,
    pub bdiv_sling: i32,
    /// The carve skill row (None: the class's own).
    pub skill: Option<usize>,
    /// The speed plan at all (false: always thrust, never brake).
    pub cap: bool,
}

pub const TODAY: Knobs = Knobs {
    grip: 45,
    rate: 125_163,
    ramp: 2,
    cpace: CLASS_PACE,
    look2: true,
    bdiv: 12,
    bdiv_sling: 12,
    skill: None,
    cap: true,
};

/// The planned speed a segment (m/s), as bake step 5 with the knobs.
pub fn fast_table(tr: &Track, k: &Knobs) -> Vec<i32> {
    let n = tr.n();
    let bend: Vec<i32> = (0..n)
        .map(|j| turn(tr.seg(j).yaw, tr.seg(j + 1).yaw))
        .collect();
    let mut f: Vec<i32> = (0..n)
        .map(|j| {
            let kk = bend[j]
                .abs()
                .max(bend[(j + 1) % n].abs())
                .max(bend[(j + 2) % n].abs());
            if kk == 0 {
                255
            } else {
                let g = isqrt((i64::from(k.grip) * 8 * 10_430 / i64::from(kk)) as u64) as i32;
                let r = if k.rate > 0 { k.rate / kk } else { i32::MAX };
                r.min(g).min(255)
            }
        })
        .collect();
    for _ in 0..2 {
        for j in (0..n).rev() {
            let next = f[(j + 1) % n] + k.ramp;
            if f[j] > next {
                f[j] = next;
            }
        }
    }
    f
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

/// `Race::drive_plain` with the knobs.
fn drive_plain(r: &Race, i: usize, k: &Knobs, fast: &[i32]) -> Ctl {
    let tr = r.tr();
    let len = tr.len();
    let sh = &r.ships[i];
    let (vs, _) = sh.v_track();
    let v = vs.max(mps(10));
    let look = FM * 10 + v * 21;
    let st = sh.s + look;
    let at = tr.at(st);
    let half = at.half * 256;
    let room = (half - FM * 18 / 10).max(0);
    let mut tx = tr.line(st) + i32::from(sh.lane) * FM / 4;
    let k0 = tr.index(sh.s);
    let n = ((st >> SEG_BITS) - (sh.s >> SEG_BITS)).max(0) as usize;
    let mut q = 0;
    while q <= n {
        let p = tr.seg(k0 + q);
        if p.kind & BOOST != 0 {
            tx = i32::from(p.pad) * FM / 2;
            break;
        }
        q += 1;
    }
    let ahead = tr.seg(at.seg);
    if sh.shield < 400 && ahead.kind & CHARGE != 0 {
        tx = -half + FM * 2;
    }
    for (j, o) in r.ships.iter().enumerate().take(usize::from(r.n)) {
        if j == i || o.fall > 0 {
            continue;
        }
        let ds = gap(sh.s, o.s, len);
        if ds > 0 && ds < FM * 16 && (o.x - tx).abs() < FM * 26 / 10 {
            tx = if o.x > sh.x {
                o.x - FM * 3
            } else {
                o.x + FM * 3
            };
            if tx.abs() > room {
                tx = if o.x > 0 { o.x - FM * 3 } else { o.x + FM * 3 };
            }
        }
    }
    let tx = tx.clamp(-room, room);
    let psi_want = (i64::from(tx - sh.x) * 10_430 / i64::from(look)) as i32;
    let psi_want = psi_want.clamp(-8_000, 8_000);
    let here = tr.seg(tr.index(sh.s));
    let dyaw = turn(here.yaw, tr.seg(tr.index(sh.s) + 1).yaw);
    let follow = ((i64::from(dyaw) * i64::from(vs)) >> SEG_BITS) as i32;
    let travel = sh.psi + (i64::from(sh.vl) * 10_430 / i64::from(sh.vf.max(mps(30)))) as i32;
    let want_rate = follow + (psi_want - travel) / 6;
    let want_rate = if sh.carve == 0 {
        let d = want_rate - sh.rate;
        sh.rate + d * 4 + d.signum()
    } else {
        want_rate
    };
    let steer = (want_rate * 256 / TURN_MAX).clamp(-256, 256);
    if !k.cap {
        return Ctl {
            steer,
            thrust: true,
            brake_l: false,
            brake_r: false,
            carve: Carve::Off,
        };
    }
    let pace = i32::from(sh.pace);
    let cap_at = |s: i32| fast[tr.index(s)];
    let mut cap = cap_at(sh.s).min(cap_at(sh.s + look));
    if k.look2 {
        cap = cap.min(cap_at(sh.s + look * 2));
    }
    let cap = mps(cap) * pace / 1024 * k.cpace[row(r.class)] / 1024;
    let over = sh.vf - cap;
    let div = if sh.sling > 0 { k.bdiv_sling } else { k.bdiv };
    let both = over > cap / div.max(1);
    let one = steer.abs() >= 256 && over > -cap / 20;
    Ctl {
        steer,
        thrust: over < 0,
        brake_l: both || (one && steer < 0),
        brake_r: both || (one && steer > 0),
        carve: Carve::Off,
    }
}

/// `Race::drive` with the knobs.
pub fn drive(r: &Race, i: usize, k: &Knobs, fast: &[i32]) -> Ctl {
    let mut c = drive_plain(r, i, k, fast);
    let both = c.brake_l && c.brake_r;
    c.brake_l = both;
    c.brake_r = both;
    let sh = &r.ships[i];
    let tr = r.tr();
    let sk = SKILLS[k.skill.unwrap_or(row(r.class))];
    let bend = |n: usize| {
        let q = tr.index(sh.s);
        turn(tr.seg(q).yaw, tr.seg(q + n).yaw)
    };
    let half = tr.at(sh.s).half * 256;
    let room = |side: i32| half - sh.x * side;
    let side = |dir: i32| if dir < 0 { Carve::Left } else { Carve::Right };
    let dir = i32::from(sh.carve);
    if dir != 0 {
        if bend(2) * dir > CARVE_HOLD_BEND
            && room(dir) > sk.room
            && room(-dir) > sk.room
            && heat_step(sh.heat) < sk.step
            && c.steer * dir > -200
        {
            c.carve = side(dir);
            c.steer = carve_stick(dir, c.steer * TURN_MAX / 256);
        }
        return c;
    }
    let b = bend(3);
    if sh.rest > sk.rest
        && b.abs() > CARVE_BEND
        && bend(8) * b.signum() >= sk.turn
        && sh.vf > CARVE_START_SPEED + FM / 6
        && sh.fall == 0
        && !sh.ctl.carve.pressed()
        && room(-1) > sk.room
        && room(1) > sk.room
    {
        let place = (tr.index(sh.s) >> 3) as u32;
        let lap = sh.lap.max(0) as u32;
        if hash(i as u32 * 1_009 + lap * 7, place) % 256 < sk.share {
            c.carve = side(b.signum());
            c.steer = 0;
        }
    }
    c
}

/// One flight of ship 0 with the knobs.
pub struct Flight {
    pub finish: u32,
    pub laps: Vec<u32>,
    pub best: u32,
    pub top: f64,
    pub walls: u32,
    pub falls: u32,
    pub brake: u32,
    pub lift: u32,
    pub carves: u32,
    pub slings: [u32; 3],
    pub push: u32,
    /// Ticks limping (shield ran out), and the lowest shield.
    pub limp: u32,
    pub shield: i16,
    pub place: u8,
    /// Every tick's controls equal `Race::drive(0)`'s (checked only when asked).
    pub same: Option<bool>,
    /// Per corner (`super::corners`), the lowest speed of each lap, m/s.
    pub corner_min: Vec<Vec<f64>>,
    /// Ticks of the pilot's own `drive` calls (counted work: one a tick).
    pub ticks: u32,
}

/// Flies ship 0 with the pilot: `Mode::Trial` alone (`seed` unused), or
/// `Mode::Race` from the back of the grid among today's rivals.
#[allow(clippy::too_many_arguments)]
pub fn fly(
    track: u8,
    class: Class,
    mode: Mode,
    seed: u32,
    pace: i16,
    lane: i8,
    k: &Knobs,
    check: bool,
) -> Flight {
    let mut race = Race::new();
    race.setup(track, class, mode, seed);
    race.ships[0].pace = pace;
    race.ships[0].lane = lane;
    let tr = race.tr();
    let len = tr.len();
    let fast = fast_table(tr, k);
    let cs = super::corners(tr);
    let mut out = Flight {
        finish: 0,
        laps: Vec::new(),
        best: 0,
        top: 0.0,
        walls: 0,
        falls: 0,
        brake: 0,
        lift: 0,
        carves: 0,
        slings: [0; 3],
        push: 0,
        limp: 0,
        shield: camber::race::SHIELD_MAX,
        place: 0,
        same: if check { Some(true) } else { None },
        corner_min: vec![vec![f64::MAX; 3]; cs.len()],
        ticks: 0,
    };
    let mut lap_start = 0u32;
    let mut was_carve = 0i8;
    let mut was_sling = 0u8;
    while race.tick < 60 * 600 {
        let running = race.phase != Phase::Count || race.tick + 1 >= camber::race::COUNT;
        let ctl = drive(&race, 0, k, &fast);
        if running {
            out.ticks += 1;
        }
        if let Some(s) = out.same.as_mut()
            && ctl != race.drive(0)
        {
            *s = false;
        }
        race.step(Some(ctl));
        while let Some(e) = race.take_ev() {
            match e {
                Ev::Wall { ship: 0 } => out.walls += 1,
                Ev::Fell { ship: 0 } => out.falls += 1,
                _ => {}
            }
        }
        let sh = race.ships[0];
        if race.phase == Phase::Count {
            continue;
        }
        let v = f64::from(sh.vf) * 60.0 / f64::from(FM);
        out.top = out.top.max(v);
        if sh.ctl.brake_l && sh.ctl.brake_r {
            out.brake += 1;
        }
        if !sh.ctl.thrust {
            out.lift += 1;
        }
        if sh.carve != 0 && was_carve == 0 {
            out.carves += 1;
        }
        was_carve = sh.carve;
        if sh.sling > was_sling && sh.sling_step > 0 {
            out.slings[usize::from(sh.sling_step - 1)] += 1;
        }
        was_sling = sh.sling;
        if sh.sling > 0 && sh.ctl.brake_l == sh.ctl.brake_r && !sh.ctl.brake_l {
            out.push += 1;
        }
        if sh.limp {
            out.limp += 1;
        }
        out.shield = out.shield.min(sh.shield);
        if (1..=3).contains(&sh.lap) && sh.fall == 0 {
            let s = i64::from(sh.s);
            for (c, m) in cs.iter().zip(out.corner_min.iter_mut()) {
                let d = (s - c.a).rem_euclid(i64::from(len));
                // The part of a corner over the line belongs to the lap
                // after: on lap 1 it is the start, not the corner.
                if d < c.len && !(sh.lap == 1 && s < c.a) {
                    let l = usize::from(sh.lap.clamp(1, 3) as u8) - 1;
                    m[l] = m[l].min(v);
                }
            }
        }
        if sh.lap_start != lap_start {
            if lap_start != 0 {
                out.laps.push(sh.lap_start - lap_start);
            }
            lap_start = sh.lap_start;
        }
        if sh.finished() {
            out.finish = sh.finish;
            out.best = sh.best;
            out.place = race.place(0);
            break;
        }
    }
    out
}

/// The variants measured, by name.
pub fn variants() -> Vec<(&'static str, &'static str, Knobs)> {
    let t = TODAY;
    let mut v = vec![
        ("today", "the rivals' driving as it is", t),
        (
            "nolook2",
            "the plan read at s and s+look only (not s+2*look)",
            Knobs { look2: false, ..t },
        ),
        (
            "ramp4",
            "planned braking 4 m/s a segment (was 2)",
            Knobs { ramp: 4, ..t },
        ),
        (
            "late",
            "nolook2 + ramp4 (brake later)",
            Knobs {
                look2: false,
                ramp: 4,
                ..t
            },
        ),
        (
            "slingok",
            "while a sling runs, brake only at cap/3 over (was cap/12)",
            Knobs { bdiv_sling: 3, ..t },
        ),
        (
            "skill3",
            "WILD's carve row in every class (carve most corners, hold to step 3)",
            Knobs {
                skill: Some(2),
                ..t
            },
        ),
    ];
    for g in [70, 100, 140, 200] {
        let name: &'static str = Box::leak(format!("grip{g}").into_boxed_str());
        let what: &'static str = Box::leak(
            format!("planned sideways pull {g} m/s^2 (was 45), turn-rate term kept")
                .into_boxed_str(),
        );
        v.push((name, what, Knobs { grip: g, ..t }));
    }
    v.push((
        "grip140late",
        "grip140 + nolook2 + ramp4",
        Knobs {
            grip: 140,
            look2: false,
            ramp: 4,
            ..t
        },
    ));
    v.push((
        "grip140all",
        "grip140 + nolook2 + ramp4 + slingok + skill3",
        Knobs {
            grip: 140,
            look2: false,
            ramp: 4,
            bdiv_sling: 3,
            skill: Some(2),
            ..t
        },
    ));
    for (name, what, g, late) in [
        ("g55sling", "grip55 + slingok", 55, false),
        ("g70sling", "grip70 + slingok", 70, false),
        ("g70late", "grip70 + nolook2 + ramp4", 70, true),
        ("g70all", "grip70 + nolook2 + ramp4 + slingok", 70, true),
    ] {
        v.push((
            name,
            what,
            Knobs {
                grip: g,
                look2: !late,
                ramp: if late { 4 } else { 2 },
                bdiv_sling: if name == "g70late" { 12 } else { 3 },
                ..t
            },
        ));
    }
    v.push((
        "nocap",
        "no speed plan at all: always thrust, never brake (steering and carve as today)",
        Knobs { cap: false, ..t },
    ));
    v.push((
        "nocap3",
        "nocap + skill3",
        Knobs {
            cap: false,
            skill: Some(2),
            ..t
        },
    ));
    v
}

fn clock(t: u32) -> String {
    format!("{}:{:05.2}", t / 3600, f64::from(t % 3600) / 60.0)
}

/// `pilot`: every variant, every class, an equal ship (pace 1024) and
/// the class's fastest and slowest rival pace, alone on every track; then
/// the equal ship in each path's real race (class and seed) from the
/// owner's grid place among today's rivals.
pub fn cmd_pilot(paths: &[super::PathFile]) -> std::process::ExitCode {
    println!(
        "PILOT: the rivals' own driving copied with knobs, flying ship 0 (a PC test pilot, NOT a person). TIME TRIAL alone, 3 laps, ticks of 1/60 s. MEASURED ON THE PC."
    );
    // The copy against the game: the baked plan and every tick's controls.
    let mut ok = true;
    for (t, tr) in TRACKS.iter().enumerate() {
        let f = fast_table(tr, &TODAY);
        let same = (0..tr.n()).all(|j| f[j] == i32::from(tr.seg(j).fast));
        println!(
            "self-test: the plan rebuilt from the knobs = the baked `fast` on {}: {}",
            tr.name,
            if same { "SAME" } else { "DIFFERENT" }
        );
        ok &= same;
        for c in Class::ALL {
            for (pace, lane) in [(1024i16, 0i8), (900, 5)] {
                let fl = fly(t as u8, c, Mode::Trial, 0, pace, lane, &TODAY, true);
                let s = fl.same == Some(true);
                ok &= s;
                if !s {
                    println!(
                        "self-test: {} {} pace {pace}: the copy's controls DIFFER from Race::drive",
                        tr.name,
                        c.name()
                    );
                }
            }
        }
    }
    for p in paths {
        let mut race = Race::new();
        race.setup(p.track, p.class, Mode::Race, p.seed);
        let fast = fast_table(race.tr(), &TODAY);
        let mut same = true;
        for _ in 0..60 * 120 {
            for i in 1..SHIPS {
                same &= drive(&race, i, &TODAY, &fast) == race.drive(i);
            }
            let c = drive(&race, 0, &TODAY, &fast);
            race.step(Some(c));
        }
        println!(
            "self-test: the copy against Race::drive for all eight ships in the race {} seed {:08x}, 120 s: {}",
            p.class.name(),
            p.seed,
            if same { "SAME" } else { "DIFFERENT" }
        );
        ok &= same;
    }
    println!(
        "self-test overall: {}\n",
        if ok {
            "SAME (the copy with today's knobs is today's driving, tick for tick)"
        } else {
            "DIFFERENT"
        }
    );
    let vs = variants();
    println!("VARIANTS:");
    for (n, w, _) in &vs {
        println!("  {n:<12} {w}");
    }
    let lagoon_cs = super::corners(&TRACKS[0]);
    for c in Class::ALL {
        let r = row(c);
        let paces: Vec<i32> = PACE.iter().map(|&p| p * RIVAL_TOP[r] / 1024).collect();
        let (pmax, pmin) = (paces[0], paces[SHIPS - 2]);
        println!(
            "\n== {} (class top {} m/s; rival paces {}..{} of 1024) ==",
            c.name(),
            c.top() * 60 / FM,
            pmin,
            pmax
        );
        println!(
            "  variant      pace  LAGOON finish  best    laps                    top m/s  walls limp s shield brake s lift s carves slings1/2/3 push s | FURNACE finish walls limp s falls | GLACIER finish walls limp s falls"
        );
        for (n, _, k) in &vs {
            for pace in [1024, pmax, pmin] {
                let f: Vec<Flight> = (0..3u8)
                    .map(|t| fly(t, c, Mode::Trial, 0, pace as i16, LANES[0], k, false))
                    .collect();
                let l = &f[0];
                let laps: Vec<String> = l.laps.iter().map(|&x| clock(x)).collect();
                println!(
                    "  {:<12} {:>4}  {:<12}  {:<7} {:<22}  {:>6.1}  {:>5} {:>6.1} {:>6} {:>7.1} {:>6.1} {:>6} {:>4}/{}/{} {:>6.1} | {:<14} {:>5} {:>6.1} {:>5} | {:<14} {:>5} {:>6.1} {:>5}",
                    n,
                    pace,
                    clock(l.finish),
                    clock(l.best),
                    laps.join(" "),
                    l.top,
                    l.walls,
                    f64::from(l.limp) / 60.0,
                    l.shield,
                    f64::from(l.brake) / 60.0,
                    f64::from(l.lift) / 60.0,
                    l.carves,
                    l.slings[0],
                    l.slings[1],
                    l.slings[2],
                    f64::from(l.push) / 60.0,
                    clock(f[1].finish),
                    f[1].walls,
                    f64::from(f[1].limp) / 60.0,
                    f[1].falls,
                    clock(f[2].finish),
                    f[2].walls,
                    f64::from(f[2].limp) / 60.0,
                    f[2].falls,
                );
            }
        }
        // The equal ship's lowest speed per corner of LAGOON, mean of 3 laps.
        println!(
            "  equal ship (pace 1024) on LAGOON, lowest speed per corner, m/s, mean of the 3 laps:"
        );
        let head: Vec<String> = lagoon_cs.iter().map(|q| format!("{:>6}", q.name)).collect();
        println!("  variant      {}", head.join(""));
        for (n, _, k) in &vs {
            let fl = fly(0, c, Mode::Trial, 0, 1024, 0, k, false);
            let cells: Vec<String> = fl
                .corner_min
                .iter()
                .map(|m| format!("{:>6.1}", m.iter().sum::<f64>() / 3.0))
                .collect();
            println!("  {:<12} {}", n, cells.join(""));
        }
    }
    println!(
        "\nTHE EQUAL SHIP IN THE OWNER'S RACE: ship 0 (pace 1024, the player's ship) flown by the pilot from the owner's grid place (last) among today's rivals, each path's track, class and seed. The owner's finish is the recorded one."
    );
    println!(
        "  class   owner     variant      finish    gap s   gap %  place  walls limp s shield  top m/s"
    );
    for p in paths {
        for (n, _, k) in &vs {
            let fl = fly(p.track, p.class, Mode::Race, p.seed, 1024, 0, k, false);
            let g = (f64::from(fl.finish) - f64::from(p.finish)) / 60.0;
            println!(
                "  {:<7} {}  {:<12} {}  {:>+7.2} {:>+6.1}%  {:>5}  {:>5} {:>6.1} {:>6}  {:>6.1}",
                p.class.name(),
                clock(p.finish),
                n,
                clock(fl.finish),
                g,
                100.0 * g * 60.0 / f64::from(p.finish),
                fl.place,
                fl.walls,
                f64::from(fl.limp) / 60.0,
                fl.shield,
                fl.top
            );
        }
    }
    if ok {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::from(3)
    }
}
