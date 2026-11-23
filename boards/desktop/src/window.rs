//! The window (feature `window`), through SDL3.
//!
//! The 320x240 panel picture is shown at a whole-number size (2x, 3x,
//! ...) with sharp pixels. The keyboard stands in for the gamepad (the
//! table is in `keymap.rs`) and the mouse for the finger. The loop is
//! the one in `run.rs`: the PC's real clock only decides how many ticks
//! to run, so pausing does not move the console's time on.
//!
//! The overlay (F1) draws the areas of the last send as outlines, and a
//! line with the bytes a real panel would have been sent and the
//! modelled send time, and the backlight value. It is drawn on the
//! window only, never into the panel picture.
//!
//! The window also shows the backlight: a lower value draws the picture
//! darker (see `board::window_dim`). Only the window: screenshots (F12)
//! and fingerprints stay the console's own picture.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use board_api::Wake;
use sdl3::event::{Event, WindowEvent};
use sdl3::keyboard::Scancode;
use sdl3::mouse::MouseButton;
use sdl3::pixels::{Color, PixelFormat};
use sdl3::render::{BlendMode, FRect, ScaleMode, Texture, WindowCanvas};

use crate::board::{DesktopBoard, PANEL_H, PANEL_W, window_dim};
use crate::controls::{Controls, window_to_panel};
use crate::keymap;
use crate::picture;
use crate::run::Runner;

/// Most ticks run in one go when the PC fell behind. Beyond this the
/// simulator drops time instead of rushing to catch up.
const MAX_CATCH_UP: u32 = 4;

/// How the window behaves. Filled in from the command line.
pub struct WindowOptions {
    /// 2 makes a 640x480 window, 3 makes 960x720, and so on.
    pub scale: u32,
    /// Ticks a second: 30 or 60.
    pub fps: u32,
    /// Close by itself after this long, and never take the keyboard
    /// (for quick checks while someone is typing elsewhere).
    pub close_after: Option<Duration>,
    /// Where F12 saves screenshots.
    pub shots_dir: PathBuf,
    /// Start with the overlay on.
    pub overlay: bool,
}

/// The open window and the loop's state.
pub struct SimWindow {
    // The SDL context must stay alive while the window is open.
    sdl: sdl3::Sdl,
    canvas: WindowCanvas,
    texture: Texture<'static>,
    events: sdl3::EventPump,
    options: WindowOptions,
    /// What the keyboard and mouse are doing.
    pub controls: Controls,
    /// The board's loop (ticks and the console's wake time).
    pub runner: Runner,
    pub paused: bool,
    pub overlay: bool,
    /// N was pressed while paused: run one tick.
    step_once: bool,
    /// The window should close.
    quit: bool,
    /// `board.screen.presents` when the texture was last filled.
    copied_presents: u32,
    /// A tick sent something since the texture was last filled.
    texture_stale: bool,
    /// The tick of the last present (for the overlay).
    last_present_tick: u64,
    opened: Instant,
    /// Screenshots saved, in order.
    pub shots: Vec<PathBuf>,
}

impl SimWindow {
    pub fn open(options: WindowOptions) -> Result<Self, String> {
        let quick = options.close_after.is_some();
        // A quick check must not take the keyboard from whoever is
        // typing in another window. The hint must be set before SDL
        // starts.
        if quick {
            sdl3::hint::set("SDL_WINDOW_ACTIVATE_WHEN_SHOWN", "0");
        }
        let sdl = sdl3::init().map_err(|e| e.to_string())?;
        let video = sdl.video().map_err(|e| e.to_string())?;
        let scale = options.scale.max(1);
        let mut builder = video.window(
            "PocketMe simulator",
            u32::from(PANEL_W) * scale,
            u32::from(PANEL_H) * scale,
        );
        builder.position_centered();
        if quick {
            let flags = builder.flags() | sdl3::video::WindowFlags::NOT_FOCUSABLE;
            builder.set_flags(flags);
        }
        let window = builder.build().map_err(|e| e.to_string())?;
        let mut canvas = window.into_canvas();
        canvas.set_blend_mode(BlendMode::Blend);
        // The texture borrows the "texture creator" for as long as it
        // lives. Both live until the program ends, so the creator is
        // simply kept for ever ("leaked"); this happens once.
        let creator = Box::leak(Box::new(canvas.texture_creator()));
        // The texture takes the panel's own colour format, RGB565, so no
        // colour conversion is needed.
        let mut texture = creator
            .create_texture_streaming(PixelFormat::RGB565, u32::from(PANEL_W), u32::from(PANEL_H))
            .map_err(|e| e.to_string())?;
        // Sharp pixels: each panel pixel becomes a solid square.
        texture.set_scale_mode(ScaleMode::Nearest);
        let events = sdl.event_pump().map_err(|e| e.to_string())?;
        let mut w = SimWindow {
            sdl,
            canvas,
            texture,
            events,
            runner: Runner::new(options.fps),
            overlay: options.overlay,
            options: WindowOptions { scale, ..options },
            controls: Controls::new(),
            paused: false,
            step_once: false,
            quit: false,
            copied_presents: 0,
            texture_stale: true,
            last_present_tick: 0,
            opened: Instant::now(),
            shots: Vec::new(),
        };
        w.update_title();
        Ok(w)
    }

    /// Lets a test put key presses and clicks into the window's queue,
    /// as if they came from the keyboard and mouse.
    pub fn event_queue(&self) -> Result<sdl3::EventSubsystem, String> {
        self.sdl.event().map_err(|e| e.to_string())
    }

    /// Runs until Esc, the close button, or `close_after`. Returns a
    /// short line on how the run went.
    pub fn run(
        &mut self,
        board: &mut DesktopBoard,
        mut step: impl FnMut(&mut DesktopBoard) -> Wake,
    ) -> String {
        let tick = Duration::from_micros(self.runner.tick_us);
        let mut owed = Duration::ZERO;
        let mut last = Instant::now();
        loop {
            self.handle_events(board);
            if self.quit || self.time_is_up() {
                break;
            }
            let now = Instant::now();
            owed += now - last;
            last = now;
            let mut n = 0;
            if self.paused {
                owed = Duration::ZERO;
                if std::mem::take(&mut self.step_once) {
                    n = 1;
                }
            } else {
                while owed >= tick && n < MAX_CATCH_UP {
                    owed -= tick;
                    n += 1;
                }
                if owed >= tick {
                    owed = Duration::ZERO; // too far behind: drop it
                }
            }
            self.advance(board, n, &mut step);
            self.draw(board);
            let wait = if self.paused {
                Duration::from_millis(15)
            } else {
                tick.saturating_sub(owed)
            };
            std::thread::sleep(wait.max(Duration::from_millis(1)));
        }
        let secs = self.opened.elapsed().as_secs_f64();
        format!(
            "window: {} ticks and {} steps in {secs:.1} s",
            self.runner.tick, self.runner.steps
        )
    }

    /// Reads every waiting key and mouse event.
    pub fn handle_events(&mut self, board: &mut DesktopBoard) {
        let scale = self.options.scale;
        let events: Vec<Event> = self.events.poll_iter().collect();
        for event in events {
            match event {
                Event::Quit { .. } => self.quit = true,
                Event::KeyDown {
                    scancode: Some(key),
                    repeat,
                    ..
                } => match key {
                    Scancode::Escape => self.quit = true,
                    Scancode::P if !repeat => {
                        self.paused = !self.paused;
                        self.update_title();
                    }
                    Scancode::N => self.step_once = true,
                    Scancode::F1 if !repeat => self.overlay = !self.overlay,
                    Scancode::F12 if !repeat => self.screenshot(board),
                    _ => {
                        if let Some((n, c)) = keymap::for_any_key(key.name())
                            && !repeat
                        {
                            // Player 2's pad plugs itself in at its first
                            // key.
                            let plug = n > 0 && !self.controls.players[n].pad.connected;
                            if plug {
                                self.controls.set_connected_by(n, true);
                            }
                            self.controls.press_by(n, c);
                            if plug || c == keymap::Control::MotionSwitch {
                                self.update_title();
                            }
                        }
                    }
                },
                Event::KeyUp {
                    scancode: Some(key),
                    ..
                } => {
                    if let Some((n, c)) = keymap::for_any_key(key.name()) {
                        self.controls.release_by(n, c);
                    }
                }
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } => self.controls.touch(window_to_panel(x, y, scale)),
                Event::MouseMotion {
                    mousestate,
                    x,
                    y,
                    xrel,
                    yrel,
                    ..
                } => {
                    if mousestate.left() && self.controls.touch.is_some() {
                        self.controls.touch(window_to_panel(x, y, scale));
                    }
                    // With `V` held, the mouse tilts the pad.
                    self.controls.tilt_by(xrel, yrel);
                }
                Event::MouseButtonUp {
                    mouse_btn: MouseButton::Left,
                    ..
                } => self.controls.lift(),
                Event::Window {
                    win_event: WindowEvent::FocusLost,
                    ..
                } => {
                    // Keys let go in another window never come here.
                    self.controls.release_all();
                    self.controls.lift();
                }
                _ => {}
            }
        }
    }

    /// Runs `ticks` ticks of the console's loop.
    pub fn advance(
        &mut self,
        board: &mut DesktopBoard,
        ticks: u32,
        mut step: impl FnMut(&mut DesktopBoard) -> Wake,
    ) {
        for _ in 0..ticks {
            let info = self.runner.run_tick(board, &mut self.controls, &mut step);
            if info.presents > 0 {
                self.last_present_tick = info.frame;
                self.texture_stale = true;
            }
        }
        for p in self.runner.problems.drain(..).chain(board.take_problems()) {
            eprintln!("problem: {p}");
        }
    }

    /// Draws the window: the panel picture, then the overlay if on.
    pub fn draw(&mut self, board: &DesktopBoard) {
        self.show_picture(board);
        if self.overlay {
            self.draw_overlay(board);
        }
        self.canvas.present();
    }

    /// Draws the panel picture into the window's back buffer (not shown
    /// yet). Fills the texture only when the picture changed.
    fn show_picture(&mut self, board: &DesktopBoard) {
        if self.texture_stale || self.copied_presents != board.screen.presents {
            // RGB565 in the PC's own byte order, as the texture expects.
            let bytes: Vec<u8> = board
                .screen
                .image
                .iter()
                .flat_map(|p| p.to_ne_bytes())
                .collect();
            let _ = self.texture.update(None, &bytes, usize::from(PANEL_W) * 2);
            self.copied_presents = board.screen.presents;
            self.texture_stale = false;
        }
        // The backlight: the window shows the picture darker.
        let m = window_dim(board.screen.backlight);
        self.texture.set_color_mod(m, m, m);
        self.canvas.set_draw_color(Color::RGB(0, 0, 0));
        self.canvas.clear();
        let _ = self.canvas.copy(&self.texture, None, None);
    }

    fn draw_overlay(&mut self, board: &DesktopBoard) {
        let s = self.options.scale as f32;
        let screen = &board.screen;
        let rects: Vec<FRect> = screen
            .last_areas
            .iter()
            .map(|a| {
                FRect::new(
                    f32::from(a.x) * s,
                    f32::from(a.y) * s,
                    f32::from(a.w) * s,
                    f32::from(a.h) * s,
                )
            })
            .collect();
        self.canvas.set_draw_color(Color::RGB(255, 230, 0));
        let _ = self.canvas.draw_rects(&rects);
        let r = screen.last_report;
        let budget = console_core::FRAME_US;
        let line1 = format!(
            "frame {}  last send {} ago: {} areas  {} px  {} bytes",
            self.runner.tick.saturating_sub(1),
            self.runner
                .tick
                .saturating_sub(1)
                .saturating_sub(self.last_present_tick),
            r.areas,
            r.pixels,
            screen.last_bytes
        );
        let light = match screen.backlight {
            Some(v) => v.to_string(),
            None => "-".to_string(),
        };
        let line2 = format!(
            "send {}.{} ms of {}.{} ms (model)  over: {}  light {light}",
            r.send_us / 1000,
            r.send_us % 1000 / 100,
            budget / 1000,
            budget % 1000 / 100,
            screen.over_budget
        );
        let h = 22.0;
        let w = self.canvas.window().size().0 as f32;
        let top = u32::from(PANEL_H) as f32 * s - h;
        self.canvas.set_draw_color(Color::RGBA(0, 0, 0, 170));
        let _ = self.canvas.fill_rect(FRect::new(0.0, top, w, h));
        self.canvas.set_draw_color(Color::RGB(255, 255, 255));
        let _ = self.canvas.draw_debug_text(&line1, (3.0, top + 2.0));
        if u64::from(r.send_us) > budget {
            self.canvas.set_draw_color(Color::RGB(255, 80, 80));
        }
        let _ = self.canvas.draw_debug_text(&line2, (3.0, top + 12.0));
    }

    /// Reads back what the window shows: the window's width and height in
    /// pixels and 8-bit red, green, blue bytes, row by row. This checks
    /// the window's own path (colour order and scaling).
    /// The picture is drawn again for this, without the overlay.
    pub fn read_back(&mut self, board: &DesktopBoard) -> Result<(usize, usize, Vec<u8>), String> {
        self.show_picture(board);
        let surface = self
            .canvas
            .read_pixels(None)
            .and_then(|s| s.convert_format(PixelFormat::RGB24))
            .map_err(|e| e.to_string())?;
        let (w, h, pitch) = (
            surface.width() as usize,
            surface.height() as usize,
            surface.pitch() as usize,
        );
        let rgb = surface.with_lock(|bytes| {
            bytes
                .chunks(pitch)
                .take(h)
                .flat_map(|row| row[..w * 3].iter().copied())
                .collect()
        });
        Ok((w, h, rgb))
    }

    fn update_title(&mut self) {
        let state = if self.paused {
            "  PAUSED (N: one frame, P: go on)"
        } else {
            ""
        };
        let motion = if self.controls.motion_on() {
            ""
        } else {
            "  motion off (M)"
        };
        let p2 = if self.controls.players[1].pad.connected {
            "  P2 pad (number pad)"
        } else {
            ""
        };
        let title = format!(
            "PocketMe simulator  {} ticks/s  x{}{motion}{p2}{state}",
            self.options.fps, self.options.scale
        );
        let _ = self.canvas.window_mut().set_title(&title);
    }

    fn time_is_up(&self) -> bool {
        self.options
            .close_after
            .is_some_and(|limit| self.opened.elapsed() >= limit)
    }

    fn screenshot(&mut self, board: &DesktopBoard) {
        let path = self.options.shots_dir.join(format!(
            "shot-{:05}.png",
            self.runner.tick.saturating_sub(1)
        ));
        let (w, h) = (usize::from(PANEL_W), usize::from(PANEL_H));
        match picture::save_png(&path, &board.screen.image, w, h) {
            Ok(()) => {
                println!("screenshot saved: {}", path.display());
                self.shots.push(path);
            }
            Err(e) => eprintln!("could not save {}: {e}", path.display()),
        }
    }
}
