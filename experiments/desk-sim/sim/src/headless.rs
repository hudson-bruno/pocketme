//! The fake board with no window.
//!
//! It runs a fixed number of frames. A script decides the input on each
//! frame, and the clock moves by exactly one frame each time, so every
//! run gives the same pictures. Nothing here reads the PC's real clock,
//! except to measure how long the work took.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use board_layer::{Board, Pad, SCREEN_H, SCREEN_W, Tick, Touch};

use crate::picture;
use crate::script::{self, Action, Event};

pub struct HeadlessBoard {
    events: Vec<Event>,
    next_event: usize,
    frames: u32,
    fps: u32,
    /// The frame handed out by the last `wait_frame`.
    current: u32,
    /// How many frames `wait_frame` has handed out.
    started: u32,
    screen: Vec<u16>,
    /// The input as the script left it.
    script_pad: Pad,
    script_touch: Touch,
    /// The input the console sees this frame. The same as `script_pad` and
    /// `script_touch`, except that a press or touch that the script also undid
    /// in the same frame still shows, for this one frame.
    seen_pad: Pad,
    seen_touch: Touch,
    /// Frames whose picture is kept (and saved, if `out_dir` is set).
    shots: BTreeSet<u32>,
    out_dir: Option<PathBuf>,
    /// Print log lines to stdout as well as keeping them.
    pub echo_log: bool,
    /// Every log line, in order, each with its frame.
    pub log: Vec<(u32, String)>,
    /// The fingerprint of every presented frame, by frame number.
    pub fingerprints: BTreeMap<u32, u64>,
    /// The kept pictures, by frame number.
    pub pictures: BTreeMap<u32, Vec<u16>>,
    /// PNG files written so far.
    pub saved: Vec<PathBuf>,
    /// PNG files that could not be written, each with the reason.
    pub save_errors: Vec<String>,
    /// Time spent by the console between `wait_frame` and `present`.
    pub work: Duration,
    work_started: Option<Instant>,
}

impl HeadlessBoard {
    /// A board that runs `frames` frames at `fps` frames a second of
    /// simulated time, with the input from `events`.
    pub fn new(frames: u32, fps: u32, events: Vec<Event>) -> Self {
        assert!(fps > 0, "fps must be more than 0");
        let pad = Pad {
            connected: true,
            ..Pad::default()
        };
        Self {
            events,
            next_event: 0,
            frames,
            fps,
            current: 0,
            started: 0,
            screen: vec![0; SCREEN_W * SCREEN_H],
            script_pad: pad,
            script_touch: Touch::default(),
            seen_pad: pad,
            seen_touch: Touch::default(),
            shots: BTreeSet::new(),
            out_dir: None,
            echo_log: false,
            log: Vec::new(),
            fingerprints: BTreeMap::new(),
            pictures: BTreeMap::new(),
            saved: Vec::new(),
            save_errors: Vec::new(),
            work: Duration::ZERO,
            work_started: None,
        }
    }

    /// Keep the picture of `frame` (and save it, see [`Self::save_to`]).
    pub fn keep(&mut self, frame: u32) {
        self.shots.insert(frame);
    }

    /// Save every kept picture as a PNG file in `dir`.
    pub fn save_to(&mut self, dir: PathBuf) {
        self.out_dir = Some(dir);
    }

    /// The picture as it is now (after the last `present`).
    pub fn screen_now(&self) -> &[u16] {
        &self.screen
    }

    /// Script lines that were never used because they are for a frame
    /// after the last one. Returns how many, and the first such frame.
    pub fn unused_events(&self) -> Option<(usize, u32)> {
        let left = &self.events[self.next_event..];
        left.first().map(|first| (left.len(), first.at))
    }

    fn time_of(&self, frame: u32) -> u64 {
        frame as u64 * 1_000_000 / self.fps as u64
    }
}

impl Board for HeadlessBoard {
    fn wait_frame(&mut self) -> Option<Tick> {
        if self.started >= self.frames {
            return None;
        }
        let frame = self.started;
        self.current = frame;
        self.started += 1;
        // Apply every script line for this frame (and any earlier ones
        // not applied yet), in order. Remember what went down, so that a
        // press and a release on the same frame are not lost: the console
        // still sees the button held for this one frame. The window does
        // the same with a key tapped faster than one frame.
        let mut went_down = 0u16;
        let mut touched = false;
        while let Some(event) = self.events.get(self.next_event).copied() {
            if event.at > frame {
                break;
            }
            match event.action {
                Action::Press(bit) => went_down |= bit,
                Action::Touch(..) => touched = true,
                _ => {}
            }
            if script::apply(event.action, &mut self.script_pad, &mut self.script_touch) {
                self.shots.insert(frame);
            }
            self.next_event += 1;
        }
        self.seen_pad = self.script_pad;
        self.seen_pad.buttons |= went_down;
        self.seen_touch = self.script_touch;
        self.seen_touch.pressed |= touched;
        self.work_started = Some(Instant::now());
        Some(Tick {
            frame,
            now_us: self.time_of(frame),
        })
    }

    fn now_us(&self) -> u64 {
        self.time_of(self.current)
    }

    fn screen(&mut self) -> &mut [u16] {
        &mut self.screen
    }

    fn present(&mut self) {
        if let Some(start) = self.work_started.take() {
            self.work += start.elapsed();
        }
        let frame = self.current;
        self.fingerprints
            .insert(frame, picture::fingerprint(&self.screen));
        if self.shots.contains(&frame) {
            self.pictures.insert(frame, self.screen.clone());
            if let Some(dir) = &self.out_dir {
                let path = dir.join(format!("frame-{frame:05}.png"));
                match picture::save_png(&path, &self.screen, SCREEN_W, SCREEN_H) {
                    Ok(()) => self.saved.push(path),
                    Err(e) => self
                        .save_errors
                        .push(format!("could not save {}: {e}", path.display())),
                }
            }
        }
    }

    fn touch(&self) -> Touch {
        self.seen_touch
    }

    fn pad(&self) -> Pad {
        self.seen_pad
    }

    fn log(&mut self, line: &str) {
        if self.echo_log {
            println!("[frame {:5}] {line}", self.current);
        }
        self.log.push((self.current, line.to_string()));
    }
}
