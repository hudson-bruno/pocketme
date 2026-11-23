//! Measurements, from the board's own clock: time per console step and
//! per present, frames a second, and the card's cost, per "scene".
//!
//! A scene is what the screen was doing in a step that drew a frame:
//!
//! | scene  | the step's frame                                        |
//! |--------|---------------------------------------------------------|
//! | boot   | the boot screen                                         |
//! | scroll | the row of tiles still sliding after the step            |
//! | notice | a notification sliding in or out                         |
//! | select | the selection changed (and nothing slides)               |
//! | idle   | anything else on the home screen (the bar's clock, ...)  |
//!
//! Frames a second: from the time between two frames in a row of the
//! same scene.

use core::fmt::Write;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scene {
    Boot,
    Scroll,
    Notice,
    Select,
    Idle,
}

pub const SCENES: [Scene; 5] = [
    Scene::Boot,
    Scene::Scroll,
    Scene::Notice,
    Scene::Select,
    Scene::Idle,
];

impl Scene {
    pub fn name(self) -> &'static str {
        match self {
            Scene::Boot => "boot",
            Scene::Scroll => "scroll",
            Scene::Notice => "notice",
            Scene::Select => "select",
            Scene::Idle => "idle",
        }
    }
}

/// Sums and worst cases of one kind of thing, in microseconds.
#[derive(Clone, Copy, Default)]
pub struct Sum {
    pub n: u32,
    pub sum: u64,
    pub max: u32,
}

impl Sum {
    pub fn add(&mut self, us: u64) {
        self.n += 1;
        self.sum += us;
        self.max = self.max.max(us.min(u64::from(u32::MAX)) as u32);
    }

    pub fn avg(&self) -> u64 {
        self.sum / u64::from(self.n.max(1))
    }
}

#[derive(Clone, Copy, Default)]
pub struct SceneStats {
    /// Whole step (input, drawing, finding changes, sending).
    pub step: Sum,
    /// The board's `present` alone (waiting for the refresh + sending);
    /// only steps where something changed call it.
    pub present: Sum,
    /// Time between two frames in a row of this scene.
    pub gap: Sum,
    /// Panel pixels sent.
    pub pixels: u64,
}

#[derive(Default)]
pub struct Stats {
    pub scenes: [SceneStats; 5],
    last: Option<(Scene, u64)>,
    /// The home screen's first card scan (at the end of the boot screen).
    pub scan: Sum,
    /// The 2-second card checks, with games on the card and with none.
    pub check_games: Sum,
    pub check_empty: Sum,
    /// Steps that returned at once (nothing due yet).
    pub early: u32,
    /// Every call of `Console::step` (frames and early ones): how often
    /// the loop woke up.
    pub wakes: u32,
}

impl Stats {
    pub fn frame(
        &mut self,
        scene: Scene,
        start: u64,
        step_us: u64,
        present_us: Option<u64>,
        pixels: u32,
    ) {
        let s = &mut self.scenes[scene as usize];
        s.step.add(step_us);
        if let Some(p) = present_us {
            s.present.add(p);
        }
        s.pixels += u64::from(pixels);
        if let Some((last, t)) = self.last
            && last == scene
        {
            s.gap.add(start - t);
        }
        self.last = Some((scene, start));
    }

    /// Frames a second, in hundredths, for a scene.
    pub fn fps_x100(&self, scene: Scene) -> u64 {
        let g = &self.scenes[scene as usize].gap;
        (u64::from(g.n) * 100_000_000)
            .checked_div(g.sum)
            .unwrap_or(0)
    }

    /// One line per scene with frames, into `out`.
    pub fn write(&self, out: &mut impl Write) {
        for scene in SCENES {
            let s = &self.scenes[scene as usize];
            if s.step.n == 0 {
                continue;
            }
            let fps = self.fps_x100(scene);
            let _ = writeln!(
                out,
                "perf scene={} frames={} step_avg_us={} step_max_us={} presents={} present_avg_us={} present_max_us={} fps={}.{:02} px_per_frame={}",
                scene.name(),
                s.step.n,
                s.step.avg(),
                s.step.max,
                s.present.n,
                s.present.avg(),
                s.present.max,
                fps / 100,
                fps % 100,
                s.pixels / u64::from(s.step.n.max(1)),
            );
        }
        let _ = writeln!(out, "loop wakes={} early={}", self.wakes, self.early);
        for (name, s) in [
            ("scan", &self.scan),
            ("check_games", &self.check_games),
            ("check_empty", &self.check_empty),
        ] {
            if s.n > 0 {
                let _ = writeln!(
                    out,
                    "card {name} n={} avg_us={} max_us={}",
                    s.n,
                    s.avg(),
                    s.max
                );
            }
        }
    }
}
