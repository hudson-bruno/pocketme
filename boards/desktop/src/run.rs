//! The board's loop: the same in the window and with no window.
//!
//! Simulated time goes in ticks of a fixed length (33,333 microseconds
//! at 30 ticks a second). A tick is what scripts call a "frame". In each
//! tick the board:
//!
//! 1. puts the controls into the board (keyboard, mouse or script);
//! 2. calls `Console::step` when the console asked for it: its deadline
//!    falls inside this tick, or it asked to be woken by input and the
//!    input changed. The clock is set to that exact deadline first, so a
//!    step never sees a time that depends on the PC;
//! 3. checks the board's rules (at most one `present` per step).
//!
//! With no window, ticks follow each other at once: a minute of play
//! takes milliseconds. In the window, the PC's real clock only decides
//! how many ticks to run; it never changes what a tick computes. So the
//! same script gives the same pictures in both.

use board_api::Wake;
use console_core::{Console, FRAME_US};

use crate::board::DesktopBoard;
use crate::controls::Controls;
use crate::script::{Action, Event};

/// What a script run drives: the console itself (`&mut *console`), or
/// (in tests) any step function. Write the closure's argument type:
/// `|b: &mut DesktopBoard| ...`, so Rust sees it works for any borrow.
pub trait Steps {
    /// One `Console::step`.
    fn step(&mut self, board: &mut DesktopBoard) -> Wake;

    /// A notification from outside (a `notify` script line), as
    /// `Console::notify`. False when it was not taken (the queue is full,
    /// or this is not a console).
    fn notify(&mut self, text: &str, now_us: u64) -> bool {
        let _ = (text, now_us);
        false
    }
}

impl<F: FnMut(&mut DesktopBoard) -> Wake> Steps for F {
    fn step(&mut self, board: &mut DesktopBoard) -> Wake {
        self(board)
    }
}

impl Steps for &mut Console {
    fn step(&mut self, board: &mut DesktopBoard) -> Wake {
        Console::step(self, board)
    }

    fn notify(&mut self, text: &str, now_us: u64) -> bool {
        Console::notify(self, text, now_us)
    }
}

/// Most steps in one tick. A console that asks to run again at once
/// more often than this is stuck: that is reported as a problem.
pub const MAX_STEPS_PER_TICK: u32 = 8;

/// The loop's own state.
pub struct Runner {
    /// Length of one tick in microseconds.
    pub tick_us: u64,
    /// The tick about to run (the number of ticks run so far).
    pub tick: u64,
    /// Steps run so far.
    pub steps: u64,
    /// The console's last answer (`None` before the first step).
    wake: Option<Wake>,
    /// What the board's rules found wrong.
    pub problems: Vec<String>,
}

/// What happened in one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TickInfo {
    /// The tick's number (a "frame" in scripts).
    pub frame: u64,
    /// Steps run in this tick (0 when the console was not due).
    pub steps: u32,
    /// Presents in this tick.
    pub presents: u32,
}

impl Runner {
    /// A loop with `fps` ticks a second of simulated time. At 30 a tick
    /// is exactly the console's frame (`FRAME_US`).
    pub fn new(fps: u32) -> Runner {
        let tick_us = if fps == 30 {
            FRAME_US
        } else {
            1_000_000 / u64::from(fps.max(1))
        };
        Runner {
            tick_us,
            tick: 0,
            steps: 0,
            wake: None,
            problems: Vec::new(),
        }
    }

    /// Runs one tick (see the file's top). `step` is the console's step,
    /// usually `|b| console.step(b)`.
    pub fn run_tick(
        &mut self,
        board: &mut DesktopBoard,
        controls: &mut Controls,
        mut step: impl FnMut(&mut DesktopBoard) -> Wake,
    ) -> TickInfo {
        let start = self.tick * self.tick_us;
        let end = start + self.tick_us;
        let mut info = TickInfo {
            frame: self.tick,
            ..TickInfo::default()
        };
        let mut input_changed = controls.take_changed();
        controls.apply(board);
        board.log.frame = self.tick;
        loop {
            let at = match self.wake {
                None => start,
                Some(w) if w.on_input() && input_changed => start,
                Some(w) if w.deadline() < end => w.deadline().max(start),
                Some(_) => break,
            };
            if info.steps == MAX_STEPS_PER_TICK {
                self.problems.push(format!(
                    "frame {}: the console asked to run again at once {MAX_STEPS_PER_TICK} times",
                    self.tick
                ));
                break;
            }
            // The clock never goes backwards.
            board.clock.now_us = board.clock.now_us.max(at);
            let before = board.screen.presents;
            self.wake = Some(step(board));
            let presents = board.screen.presents - before;
            if presents > 1 {
                self.problems.push(format!(
                    "frame {}: {presents} presents in one step; the console may send at most one",
                    self.tick
                ));
            }
            info.presents += presents;
            info.steps += 1;
            self.steps += 1;
            // The step saw every latched press.
            controls.stepped();
            controls.apply(board);
            input_changed = false;
        }
        self.tick += 1;
        info
    }
}

/// Runs the console with no input until it has done `frames` frames.
/// Returns the clock at the end, in microseconds. (A short way for
/// tests; [`run_script`] does the full job.)
pub fn run_frames(console: &mut Console, board: &mut DesktopBoard, frames: u32) -> u64 {
    let mut runner = Runner::new(30);
    let mut controls = Controls::new();
    // A console that stopped making frames would loop for ever: stop.
    let limit = u64::from(frames) * 100 + 100;
    while console.frames() < frames && runner.tick < limit {
        runner.run_tick(board, &mut controls, |b| console.step(b));
    }
    board.clock.now_us
}

/// A run with no window: how long, how fast, and what happens when.
pub struct ScriptRun {
    /// Ticks to run (frames 0 to `frames - 1`).
    pub frames: u64,
    /// Ticks a second (30 or 60).
    pub fps: u32,
    /// The script, sorted by frame (see `script::parse`).
    pub events: Vec<Event>,
}

/// Runs a script with no window. `step` is the console (`&mut *console`,
/// which also takes `notify` lines) or a step function. Calls `after`
/// once after every tick,
/// with the tick's info, the board, and whether the script asked for a
/// picture of this frame. Returns every problem found, also script lines
/// that never ran; an empty list means all went well.
pub fn run_script(
    run: &ScriptRun,
    board: &mut DesktopBoard,
    mut step: impl Steps,
    mut after: impl FnMut(&TickInfo, &DesktopBoard, bool),
) -> Vec<String> {
    let mut runner = Runner::new(run.fps);
    let mut controls = Controls::new();
    let mut next = 0;
    while runner.tick < run.frames {
        let mut shot = false;
        let mut light = None;
        while let Some(e) = run.events.get(next).filter(|e| e.at <= runner.tick) {
            let n = usize::from(e.player);
            match e.action {
                Action::Press(c) => controls.press_by(n, c),
                Action::Release(c) => controls.release_by(n, c),
                Action::Touch(p) => controls.touch(p),
                Action::Lift => controls.lift(),
                Action::Stick(side, s) => controls.set_stick_by(n, side, s),
                Action::Trigger(side, v) => controls.set_trigger_by(n, side, v),
                Action::Pad(on) => controls.set_connected_by(n, on),
                Action::Motion(on) => controls.set_motion_by(n, on),
                Action::Tilt(x, y) => controls.set_tilt_by(n, x, y),
                Action::Spin(g) => controls.set_spin_by(n, g),
                Action::Card(inserted) => board.files.set_card(inserted),
                Action::Shot => shot = true,
                Action::Backlight(v) => light = Some(v),
                Action::Notify(text) => {
                    if step.notify(text.as_str(), board.clock.now_us) {
                        // The console's last wake did not know of it:
                        // step at this tick (as `Console::notify` asks).
                        runner.wake = None;
                    } else {
                        runner.problems.push(format!(
                            "frame {}: the notification `{}` was not taken",
                            runner.tick,
                            text.as_str()
                        ));
                    }
                }
            }
            next += 1;
        }
        let info = runner.run_tick(board, &mut controls, |b| step.step(b));
        if let Some(want) = light
            && board.screen.backlight != Some(want)
        {
            let is = board
                .screen
                .backlight
                .map_or("not set".into(), |v| v.to_string());
            runner.problems.push(format!(
                "frame {}: the backlight is {is}, the script expected {want}",
                info.frame
            ));
        }
        after(&info, board, shot);
    }
    let mut problems = runner.problems;
    problems.extend(board.take_problems());
    if let Some(first) = run.events.get(next) {
        problems.push(format!(
            "{} script line(s) never ran: the first is for frame {}, but the run stopped \
             after frame {}. Use --frames {} or more.",
            run.events.len() - next,
            first.at,
            run.frames.saturating_sub(1),
            first.at + 1
        ));
    }
    problems
}
