//! The fake board with a window, through SDL3.
//!
//! The 320x240 picture is shown at a whole-number size (2x, 3x, ...) with
//! sharp pixels. The keyboard stands in for the gamepad and the mouse for
//! the finger. The clock moves by exactly one frame each frame, as in the
//! no-window mode, so a pause does not move the time on. The PC's real
//! clock only decides when the next frame is due.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use board_layer::{Board, Pad, SCREEN_H, SCREEN_W, Stick, Tick, Touch, button};
use sdl3::event::Event;
use sdl3::keyboard::Scancode;
use sdl3::mouse::MouseButton;
use sdl3::pixels::PixelFormat;
use sdl3::render::{ScaleMode, Texture, WindowCanvas};

use crate::picture;

/// Keyboard keys that stand in for the pad's buttons.
pub const KEYS: [(Scancode, u16, &str); 13] = [
    (Scancode::Up, button::UP, "arrow up"),
    (Scancode::Down, button::DOWN, "arrow down"),
    (Scancode::Left, button::LEFT, "arrow left"),
    (Scancode::Right, button::RIGHT, "arrow right"),
    (Scancode::X, button::A, "X"),
    (Scancode::Z, button::B, "Z"),
    (Scancode::S, button::X, "S"),
    (Scancode::A, button::Y, "A"),
    (Scancode::Q, button::L, "Q"),
    (Scancode::W, button::R, "W"),
    (Scancode::Return, button::START, "Enter"),
    (Scancode::Backspace, button::SELECT, "Backspace"),
    (Scancode::Tab, button::HOME, "Tab"),
];

/// How the window behaves. Filled in from the command line.
pub struct WindowOptions {
    /// 2 makes a 640x480 window, 3 makes 960x720, and so on.
    pub scale: u32,
    /// Frames a second: 30 or 60.
    pub fps: u32,
    /// Close by itself after this long (for quick checks).
    pub close_after: Option<Duration>,
    /// Where F12 saves screenshots.
    pub shots_dir: PathBuf,
}

pub struct WindowBoard {
    // The SDL context must stay alive while the window is open.
    _sdl: sdl3::Sdl,
    canvas: WindowCanvas,
    texture: Texture<'static>,
    events: sdl3::EventPump,
    options: WindowOptions,
    screen: Vec<u16>,
    pad: Pad,
    touch: Touch,
    /// How many frames `wait_frame` has handed out.
    frame: u32,
    /// The frame handed out by the last `wait_frame`.
    current: u32,
    paused: bool,
    opened: Instant,
    next_due: Instant,
    /// Frames shown, and the console's own work time, for the report.
    shown: u32,
    work: Duration,
    work_started: Option<Instant>,
}

impl WindowBoard {
    pub fn open(options: WindowOptions) -> Result<Self, String> {
        // A quick check (--close-after) should not take the keyboard away
        // from whoever is typing in another window. Must be set before SDL
        // starts. (Untested: whether WSLg honours it.)
        if options.close_after.is_some() {
            sdl3::hint::set("SDL_WINDOW_ACTIVATE_WHEN_SHOWN", "0");
        }
        let sdl = sdl3::init().map_err(|e| e.to_string())?;
        let video = sdl.video().map_err(|e| e.to_string())?;
        let (w, h) = (
            SCREEN_W as u32 * options.scale,
            SCREEN_H as u32 * options.scale,
        );
        let window = video
            .window("desk sim", w, h)
            .position_centered()
            .build()
            .map_err(|e| e.to_string())?;
        let canvas = window.into_canvas();
        // The texture borrows the "texture creator" for as long as it
        // lives. Both live until the program ends, so the creator is
        // simply kept forever ("leaked"); this happens once.
        let creator = Box::leak(Box::new(canvas.texture_creator()));
        // The texture takes the console's own colour format, RGB565, so
        // no colour conversion is needed.
        let mut texture = creator
            .create_texture_streaming(PixelFormat::RGB565, SCREEN_W as u32, SCREEN_H as u32)
            .map_err(|e| e.to_string())?;
        // Sharp pixels: each console pixel becomes a solid square.
        texture.set_scale_mode(ScaleMode::Nearest);
        let events = sdl.event_pump().map_err(|e| e.to_string())?;
        let now = Instant::now();
        let mut board = Self {
            _sdl: sdl,
            canvas,
            texture,
            events,
            options,
            screen: vec![0; SCREEN_W * SCREEN_H],
            pad: Pad {
                connected: true,
                ..Pad::default()
            },
            touch: Touch::default(),
            frame: 0,
            current: 0,
            paused: false,
            opened: now,
            next_due: now,
            shown: 0,
            work: Duration::ZERO,
            work_started: None,
        };
        board.update_title();
        Ok(board)
    }

    /// True while the P key has paused the console.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Lets a test put key presses and clicks into the window's queue,
    /// as if they came from the keyboard and mouse.
    pub fn event_queue(&self) -> Result<sdl3::EventSubsystem, String> {
        self._sdl.event().map_err(|e| e.to_string())
    }

    /// A short line on how the run went.
    pub fn report(&self) -> String {
        let secs = self.opened.elapsed().as_secs_f64();
        let work_ms = self.work.as_secs_f64() * 1000.0 / self.shown.max(1) as f64;
        format!(
            "window: {} frames in {secs:.1} s ({:.1} a second); console work {work_ms:.3} ms a frame on average",
            self.shown,
            self.shown as f64 / secs
        )
    }

    fn update_title(&mut self) {
        let state = if self.paused {
            "  PAUSED (N: next frame, P: go on)"
        } else {
            ""
        };
        let title = format!(
            "desk sim  {} fps  x{}{state}",
            self.options.fps, self.options.scale
        );
        let _ = self.canvas.window_mut().set_title(&title);
    }

    /// Shows the texture (the last presented picture) again.
    fn show(&mut self) {
        self.canvas.clear();
        let _ = self.canvas.copy(&self.texture, None, None);
        self.canvas.present();
    }

    /// Reads back what the window shows (the last presented picture,
    /// drawn again but not shown again). Gives the window's width and
    /// height in pixels and 8-bit red, green, blue bytes, row by row.
    /// This checks the window's own path: colour order and scaling.
    pub fn read_back(&mut self) -> Result<(usize, usize, Vec<u8>), String> {
        self.canvas.clear();
        self.canvas
            .copy(&self.texture, None, None)
            .map_err(|e| e.to_string())?;
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

    /// Saves [`Self::read_back`] as a PNG file, at the window's size.
    pub fn save_read_back(&mut self, name: &str) -> Result<PathBuf, String> {
        let (w, h, rgb) = self.read_back()?;
        let path = self.options.shots_dir.join(name);
        picture::save_png_rgb888(&path, &rgb, w, h)
            .map_err(|e| format!("could not save {}: {e}", path.display()))?;
        Ok(path)
    }

    fn time_is_up(&self) -> bool {
        self.options
            .close_after
            .is_some_and(|limit| self.opened.elapsed() >= limit)
    }

    fn screenshot(&mut self) {
        let path = self
            .options
            .shots_dir
            .join(format!("shot-{:05}.png", self.current));
        match picture::save_png(&path, &self.screen, SCREEN_W, SCREEN_H) {
            Ok(()) => println!("screenshot saved: {}", path.display()),
            Err(e) => eprintln!("could not save {}: {e}", path.display()),
        }
    }

    /// Room for a real gamepad. The Linux in WSL2 cannot see a pad that
    /// is connected to Windows, so this is not done yet. SDL3 can read
    /// pads (its "gamepad" part); a Windows build of the simulator, or a
    /// pad passed into WSL over USB, would fill this in. Until then the
    /// keyboard is the pad.
    fn real_pad(&mut self) -> Option<Pad> {
        None
    }

    /// Reads the keyboard and mouse into the pad and touch state.
    /// `tapped` holds keys and clicks seen during this frame, so a press
    /// shorter than one frame is not lost.
    fn read_input(&mut self, quick_keys: u16, quick_click: Option<(f32, f32)>) {
        let keys = self.events.keyboard_state();
        let held = |sc| keys.is_scancode_pressed(sc);
        let mut buttons = quick_keys;
        for (key, bit, _) in KEYS {
            if held(key) {
                buttons |= bit;
            }
        }
        let axis = |minus, plus| match (held(minus), held(plus)) {
            (true, false) => -128,
            (false, true) => 127,
            _ => 0,
        };
        let left = Stick {
            x: axis(Scancode::J, Scancode::L),
            y: axis(Scancode::I, Scancode::K),
        };
        let right = Stick {
            x: axis(Scancode::F, Scancode::H),
            y: axis(Scancode::T, Scancode::G),
        };
        let trigger = |key| if held(key) { 255 } else { 0 };
        let keyboard_pad = Pad {
            connected: true,
            buttons,
            left,
            right,
            left_trigger: trigger(Scancode::_1),
            right_trigger: trigger(Scancode::_2),
        };
        self.pad = self.real_pad().unwrap_or(keyboard_pad);

        let mouse = self.events.mouse_state();
        let (mx, my) = quick_click.unwrap_or((mouse.x(), mouse.y()));
        let scale = self.options.scale as f32;
        self.touch = Touch {
            pressed: mouse.left() || quick_click.is_some(),
            x: ((mx / scale) as u16).min(SCREEN_W as u16 - 1),
            y: ((my / scale) as u16).min(SCREEN_H as u16 - 1),
        };
    }
}

impl Board for WindowBoard {
    fn wait_frame(&mut self) -> Option<Tick> {
        let mut quick_keys = 0u16;
        let mut quick_click = None;
        loop {
            let mut step = false;
            let mut toggled = false;
            let mut shot = false;
            for event in self.events.poll_iter() {
                match event {
                    Event::Quit { .. } => return None,
                    Event::KeyDown {
                        scancode: Some(key),
                        repeat: false,
                        ..
                    } => match key {
                        Scancode::Escape => return None,
                        Scancode::P => toggled = true,
                        Scancode::N => step = true,
                        Scancode::F12 => shot = true,
                        _ => {
                            if let Some((_, bit, _)) = KEYS.iter().find(|(k, _, _)| *k == key) {
                                quick_keys |= bit;
                            }
                        }
                    },
                    Event::MouseButtonDown {
                        mouse_btn: MouseButton::Left,
                        x,
                        y,
                        ..
                    } => quick_click = Some((x, y)),
                    _ => {}
                }
            }
            if shot {
                self.screenshot();
            }
            if toggled {
                self.paused = !self.paused;
                self.update_title();
            }
            if self.time_is_up() {
                return None;
            }
            if !self.paused || step {
                break;
            }
            // Paused: keep the window drawn, wait a little, look again.
            self.show();
            std::thread::sleep(Duration::from_millis(15));
            self.next_due = Instant::now();
        }

        // Wait until the next frame is due. If the PC fell far behind,
        // start counting again from now instead of rushing to catch up.
        let frame_len = Duration::from_nanos(1_000_000_000 / self.options.fps as u64);
        let now = Instant::now();
        if self.next_due > now {
            std::thread::sleep(self.next_due - now);
        } else if now - self.next_due > frame_len {
            self.next_due = now;
        }
        self.next_due += frame_len;

        self.read_input(quick_keys, quick_click);
        self.current = self.frame;
        self.frame += 1;
        let tick = Tick {
            frame: self.current,
            now_us: self.now_us(),
        };
        self.work_started = Some(Instant::now());
        Some(tick)
    }

    fn now_us(&self) -> u64 {
        self.current as u64 * 1_000_000 / self.options.fps as u64
    }

    fn screen(&mut self) -> &mut [u16] {
        &mut self.screen
    }

    fn present(&mut self) {
        if let Some(start) = self.work_started.take() {
            self.work += start.elapsed();
        }
        // RGB565 in the PC's own byte order, as the texture expects.
        let bytes: Vec<u8> = self.screen.iter().flat_map(|p| p.to_ne_bytes()).collect();
        let _ = self.texture.update(None, &bytes, SCREEN_W * 2);
        self.show();
        self.shown += 1;
    }

    fn touch(&self) -> Touch {
        self.touch
    }

    fn pad(&self) -> Pad {
        self.pad
    }

    fn log(&mut self, line: &str) {
        println!("[frame {:5}] {line}", self.current);
    }
}
