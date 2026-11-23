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
//! | drag   | a finger on the panel, on the home screen (or the link's  |
//! |        | scripted finger)                                         |
//! | select | the selection changed (and nothing slides)               |
//! | idle   | anything else on the home screen (the bar's clock, ...)  |
//! | game   | a game's frame (its pause menu not open)                 |
//! | menu   | the pause menu over a game                               |
//!
//! Frames a second: from the time between two frames in a row of the
//! same scene. `fps` takes every such gap, so it also counts the times
//! the console slept in the middle of a scene (a finger resting on the
//! panel, a menu waiting): on the owner's test "drag" gave 22 fps while
//! the frames themselves came at 30 a second. `run_fps` takes only the
//! gaps of at most [`PAUSE_US`] (frames back to back); `pauses` counts
//! the others.

use core::fmt::Write;

/// A gap between two frames of a scene longer than this is a pause
/// (the console slept), not a frame rate: three frames at 30 a second.
pub const PAUSE_US: u64 = 100_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scene {
    Boot,
    Drag,
    Scroll,
    Notice,
    Select,
    Idle,
    Game,
    Menu,
}

pub const SCENES: [Scene; 8] = [
    Scene::Boot,
    Scene::Drag,
    Scene::Scroll,
    Scene::Notice,
    Scene::Select,
    Scene::Idle,
    Scene::Game,
    Scene::Menu,
];

impl Scene {
    pub fn name(self) -> &'static str {
        match self {
            Scene::Boot => "boot",
            Scene::Drag => "drag",
            Scene::Scroll => "scroll",
            Scene::Notice => "notice",
            Scene::Select => "select",
            Scene::Idle => "idle",
            Scene::Game => "game",
            Scene::Menu => "menu",
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
    /// The same, for gaps of at most [`PAUSE_US`] only; and how many
    /// were longer.
    pub run_gap: Sum,
    pub pauses: u32,
    /// Panel pixels sent.
    pub pixels: u64,
}

#[derive(Default)]
pub struct Stats {
    pub scenes: [SceneStats; 8],
    last: Option<(Scene, u64)>,
    /// The home screen's first card scan (at the end of the boot screen).
    pub scan: Sum,
    /// The 2-second card checks, with games on the card and with none.
    pub check_games: Sum,
    pub check_empty: Sum,
    /// Home's way back after a game: from the step a game ended (or could
    /// not start) to the end of the step that drew home again, with the
    /// list and the pictures read from the card again.
    pub reload: Sum,
    /// Steps that returned at once (nothing due yet).
    pub early: u32,
    /// Every call of `Console::step` (frames and early ones): how often
    /// the loop woke up.
    pub wakes: u32,
    /// From the press that started a game to the end of its first
    /// presented frame (the load is inside).
    pub game_first_frame: Sum,
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
            let gap = start - t;
            s.gap.add(gap);
            if gap <= PAUSE_US {
                s.run_gap.add(gap);
            } else {
                s.pauses += 1;
            }
        }
        self.last = Some((scene, start));
    }

    /// Frames a second, in hundredths, for a scene.
    pub fn fps_x100(&self, scene: Scene) -> u64 {
        per_second_x100(&self.scenes[scene as usize].gap)
    }

    /// The same, from frames back to back only (no pauses).
    pub fn run_fps_x100(&self, scene: Scene) -> u64 {
        per_second_x100(&self.scenes[scene as usize].run_gap)
    }

    /// The short form for `boardctl stats` (one link frame holds at
    /// most 1 KB): per scene, frames, step and present average/largest
    /// (us), and frames a second. The log's report has the long form.
    #[cfg_attr(not(feature = "link"), allow(dead_code))]
    pub fn write_short(&self, out: &mut impl Write) {
        for scene in SCENES {
            let s = &self.scenes[scene as usize];
            if s.step.n == 0 {
                continue;
            }
            let fps = self.fps_x100(scene);
            let _ = write!(
                out,
                "perf {} n={} step={}/{} present={}/{} fps={}.{:02}",
                scene.name(),
                s.step.n,
                s.step.avg(),
                s.step.max,
                s.present.avg(),
                s.present.max,
                fps / 100,
                fps % 100,
            );
            // Only where it differs from fps: the scene paused, and also
            // had frames back to back (the frame holds 1 KB).
            if s.pauses > 0 && s.run_gap.n > 0 {
                let run = self.run_fps_x100(scene);
                let _ = write!(out, " run={}.{:02}", run / 100, run % 100);
            }
            let _ = writeln!(out);
        }
        let _ = writeln!(out, "loop wakes={} early={}", self.wakes, self.early);
    }

    /// One line per scene with frames, into `out`.
    pub fn write(&self, out: &mut impl Write) {
        for scene in SCENES {
            let s = &self.scenes[scene as usize];
            if s.step.n == 0 {
                continue;
            }
            let fps = self.fps_x100(scene);
            let run = self.run_fps_x100(scene);
            let _ = writeln!(
                out,
                "perf scene={} frames={} step_avg_us={} step_max_us={} presents={} present_avg_us={} present_max_us={} fps={}.{:02} run_fps={}.{:02} pauses={} px_per_frame={}",
                scene.name(),
                s.step.n,
                s.step.avg(),
                s.step.max,
                s.present.n,
                s.present.avg(),
                s.present.max,
                fps / 100,
                fps % 100,
                run / 100,
                run % 100,
                s.pauses,
                s.pixels / u64::from(s.step.n.max(1)),
            );
        }
        let _ = writeln!(out, "loop wakes={} early={}", self.wakes, self.early);
        for (name, s) in [
            ("scan", &self.scan),
            ("check_games", &self.check_games),
            ("check_empty", &self.check_empty),
            ("home_reload", &self.reload),
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
        let g = &self.game_first_frame;
        if g.n > 0 {
            let _ = writeln!(
                out,
                "game first_frame n={} avg_us={} max_us={}",
                g.n,
                g.avg(),
                g.max
            );
        }
    }
}

/// How many a second, in hundredths, from the gaps between them.
fn per_second_x100(g: &Sum) -> u64 {
    (u64::from(g.n) * 100_000_000)
        .checked_div(g.sum)
        .unwrap_or(0)
}
