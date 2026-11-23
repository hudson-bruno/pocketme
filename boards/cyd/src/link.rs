//! The debug link (feature `link`): the PC's `boardctl` talks to the
//! board over the same serial port as the log (crates/debug-link).
//!
//! - `shot`: the console's own picture (`--source fb`, 160x120 with its
//!   palette), or the panel read back pixel by pixel (`--source panel`).
//! - `key`: buttons, given to the console as pad 0 (the "cable pad").
//! - `tap`: one touch at a panel pixel.
//! - `stats`: the measurements as text.
//! - letters outside frames (`boardctl text`): t = tour on/off,
//!   r = reset measurements, m = measurements to the log now,
//!   b = strip bench (`wide-strips` builds only).
//!
//! Frames are sent whole, without `await` in between, so no log line
//! can land inside one. The pattern is experiments/display-dma's.

use core::fmt::Write as _;

use board_api::TouchPoint;
use debug_link::{
    TextBuf,
    frame::{COMMAND_DECODER_BUF, Decoder, Event, MAX_FRAME},
    image::ImageSender,
    msg::{PixelFormat, Reply, Request, Source, kind, status},
};
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Timer};
use esp_hal::{
    Async,
    uart::{Config as UartConfig, Uart},
};
use log::{info, warn};

use crate::app::App;
use crate::board::{link_to_pad, now_us};
use crate::lcd::{H, READ_KHZ, W};

/// How long the PC has to say "go" at a faster speed.
const GO_WAIT: Duration = Duration::from_millis(1500);
/// Silence after which a half-received frame is given up.
const QUIET_US: u64 = 100_000;

pub type LinkDecoder = Decoder<COMMAND_DECODER_BUF>;

pub fn new_decoder() -> LinkDecoder {
    Decoder::new()
}

/// A request copied out of the decoder.
struct Pending {
    kind: u8,
    seq: u8,
    payload: [u8; COMMAND_DECODER_BUF],
    len: usize,
}

#[derive(Default, Clone, Copy)]
pub struct LinkCounters {
    pub rx_bytes: u32,
    pub rx_text: u32,
    pub rx_frames: u32,
    pub rx_corrupt: u32,
    pub shots: u32,
    pub shot_ms: u32,
    pub keys: u32,
    pub taps: u32,
}

pub struct LinkState {
    frame: [u8; MAX_FRAME],
    row: [u8; W * 2],
    scratch: [u8; 1 + W * 3],
    last_byte_us: u64,
    pub counters: LinkCounters,
}

impl LinkState {
    pub const fn new() -> Self {
        Self {
            frame: [0; MAX_FRAME],
            row: [0; W * 2],
            scratch: [0; 1 + W * 3],
            last_byte_us: 0,
            counters: LinkCounters {
                rx_bytes: 0,
                rx_text: 0,
                rx_frames: 0,
                rx_corrupt: 0,
                shots: 0,
                shot_ms: 0,
                keys: 0,
                taps: 0,
            },
        }
    }
}

/// Sends bytes and returns when all are in the port's queue, without
/// `await`.
fn send(uart: &mut Uart<'static, Async>, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        match uart.write(bytes) {
            Ok(sent) => bytes = &bytes[sent..],
            Err(_) => return,
        }
    }
}

/// The beam calibration flag for `stats`: from the screen, or (with
/// `core2`) from the second core's last snapshot.
#[cfg(not(feature = "core2"))]
macro_rules! beam_calibrated {
    ($app:expr, $snap:ident) => {
        $app.board.screen.beam.calibrated
    };
}
#[cfg(feature = "core2")]
macro_rules! beam_calibrated {
    ($app:expr, $snap:ident) => {
        $snap.calibrated
    };
}

impl App {
    fn reply(&mut self, seq: u8, reply: Reply<'_>) {
        // Log text already waiting goes first, whole lines.
        crate::logbuf::flush(&mut self.uart);
        if let Ok(len) = reply.encode(seq, &mut self.link.frame[..]) {
            send(&mut self.uart, &self.link.frame[..len]);
        }
    }

    fn ack(&mut self, seq: u8, request: u8, status: u8) {
        self.reply(seq, Reply::Ack { request, status });
    }

    /// No bytes came: maybe give up a half frame.
    pub async fn link_idle(&mut self, decoder: &mut LinkDecoder) {
        if decoder.mid_frame() && now_us() - self.link.last_byte_us >= QUIET_US {
            let counters = &mut self.link.counters;
            decoder.idle(&mut |event| {
                if let Event::Corrupt = event {
                    counters.rx_corrupt += 1;
                }
            });
        }
    }

    pub async fn link_bytes(&mut self, decoder: &mut LinkDecoder, bytes: &[u8]) {
        self.link.counters.rx_bytes += bytes.len() as u32;
        self.link.last_byte_us = now_us();
        let mut pending = Pending {
            kind: 0,
            seq: 0,
            payload: [0; COMMAND_DECODER_BUF],
            len: 0,
        };
        for &b in bytes {
            let mut complete = false;
            let mut letter = None;
            let counters = &mut self.link.counters;
            decoder.push(b, &mut |event| match event {
                Event::Text(t) => {
                    counters.rx_text += 1;
                    letter = Some(t);
                }
                Event::Corrupt => counters.rx_corrupt += 1,
                Event::Frame(frame) => {
                    counters.rx_frames += 1;
                    pending.kind = frame.kind;
                    pending.seq = frame.seq;
                    pending.len = frame.payload.len();
                    pending.payload[..pending.len].copy_from_slice(frame.payload);
                    complete = true;
                }
            });
            if let Some(t) = letter {
                self.on_letter(t);
            }
            if complete {
                self.handle(&pending).await;
            }
        }
    }

    async fn handle(&mut self, pending: &Pending) {
        let frame = debug_link::frame::Frame {
            kind: pending.kind,
            seq: pending.seq,
            payload: &pending.payload[..pending.len],
        };
        let seq = frame.seq;
        match Request::parse(&frame) {
            Ok(Request::Ping) => self.ack(seq, kind::PING, status::OK),
            Ok(Request::Stats) => self.send_stats(seq),
            Ok(Request::Buttons { mask }) => {
                self.ack(seq, kind::BUTTONS, status::OK);
                self.link.counters.keys += 1;
                if mask != 0 {
                    self.last_press_us = crate::board::now_us();
                }
                self.board.pad.cable.set(link_to_pad(mask));
            }
            Ok(Request::Tap { x, y }) => {
                let ok = usize::from(x) < W && usize::from(y) < H;
                self.ack(
                    seq,
                    kind::TAP,
                    if ok { status::OK } else { status::BAD_REQUEST },
                );
                if ok {
                    self.link.counters.taps += 1;
                    self.board.touch.tap(TouchPoint { x, y });
                }
            }
            Ok(Request::Shot {
                source,
                baud,
                read_khz,
            }) => self.shot(seq, source, baud, read_khz).await,
            Ok(Request::PanelRaw {
                cmd,
                dummy_bits,
                read_len,
                clock_khz,
                params,
            }) => {
                let khz = if clock_khz == 0 {
                    READ_KHZ
                } else {
                    u32::from(clock_khz)
                };
                let mut answer = [0u8; 256];
                let answer = &mut answer[..usize::from(read_len).min(256)];
                match self
                    .board
                    .screen
                    .lcd
                    .raw(khz, cmd, params, dummy_bits, answer)
                {
                    Ok(()) => self.reply(seq, Reply::PanelRawReply { bytes: answer }),
                    Err(e) => {
                        warn!("panel command {cmd:02x}h failed: {e:?}");
                        self.ack(seq, kind::PANEL_RAW, status::FAILED);
                    }
                }
                self.board.screen.beam.forget();
            }
            Ok(Request::Go) => {}
            Err(debug_link::msg::ParseError::UnknownKind) => {
                let st = if frame.kind == kind::PLAYER_BUTTONS {
                    self.player_buttons(&frame)
                } else {
                    self.pad_request(&frame)
                };
                self.ack(seq, frame.kind, st)
            }
            Err(debug_link::msg::ParseError::BadPayload) => {
                self.ack(seq, frame.kind, status::BAD_REQUEST)
            }
        }
    }

    /// Buttons for one player's cable pad (kind 0x09, `boardctl key
    /// --player N`): the status for its answer.
    fn player_buttons(&mut self, frame: &debug_link::frame::Frame<'_>) -> u8 {
        match debug_link::msg::PlayerButtons::parse(frame) {
            Ok(pb) => {
                self.link.counters.keys += 1;
                if pb.mask != 0 {
                    self.last_press_us = crate::board::now_us();
                }
                self.board
                    .pad
                    .cable_mut(pb.player)
                    .set(link_to_pad(pb.mask));
                status::OK
            }
            Err(_) => status::BAD_REQUEST,
        }
    }

    /// An injected-pad request (kind 0x08): the status for its answer.
    /// UNSUPPORTED without the gamepad host, as for any kind not known.
    fn pad_request(&mut self, frame: &debug_link::frame::Frame<'_>) -> u8 {
        #[cfg(feature = "pad")]
        if frame.kind == debug_link::pad::KIND
            && let Some(r) = &mut self.radio
        {
            use debug_link::pad::PadRequest;
            return match PadRequest::parse(frame) {
                Ok(req) => {
                    if matches!(req, PadRequest::Report { .. }) {
                        self.last_press_us = crate::board::now_us();
                    }
                    r.inject(&req, &mut self.board.pad)
                }
                Err(_) => status::BAD_REQUEST,
            };
        }
        let _ = frame;
        status::UNSUPPORTED
    }

    fn send_stats(&mut self, seq: u8) {
        let mut text = [0u8; debug_link::frame::MAX_PAYLOAD];
        let mut out = TextBuf::new(&mut text);
        let (stack_used, stack_size) = crate::mem::stack_deepest();
        let heap = esp_alloc::HEAP.stats();
        #[cfg(not(feature = "core2"))]
        let c = &self.board.screen.counters;
        #[cfg(feature = "core2")]
        let snap = crate::core2::snapshot();
        #[cfg(feature = "core2")]
        let c = &snap.counters;
        let home = self.console.home();
        let _ = writeln!(out, "firmware=pocketme-cyd {}", env!("CARGO_PKG_VERSION"));
        let _ = writeln!(out, "features={}", crate::FEATURES);
        let _ = writeln!(out, "uptime_ms={}", Instant::now().as_millis());
        let _ = writeln!(
            out,
            "page={:?} frames={}",
            self.console.page(),
            self.console.frames()
        );
        let _ = writeln!(
            out,
            "games={} selected={} scroll={} pad={}",
            home.count(),
            home.selected(),
            home.scroll(),
            self.board.pad.connected()
        );
        #[cfg(feature = "tour")]
        let _ = writeln!(out, "tour={}", self.tour.on());
        // (The write clock and strip size are fixed by the build: the
        // boot log's `panel:` line has them.)
        let _ = writeln!(
            out,
            "presents={} windows={} untimed={} no_anchor={} no_plan={} late={} crossed={} errors={} beam_calibrated={}",
            c.presents,
            c.windows,
            c.untimed,
            c.no_anchor,
            c.no_plan,
            c.late,
            c.crossed,
            c.errors,
            beam_calibrated!(self, snap)
        );
        let _ = writeln!(
            out,
            "heap_free={} stack_used={} stack_size={stack_size}",
            heap.size - heap.current_usage,
            stack_used.map_or(-1, |u| u as i64)
        );
        let l = &self.link.counters;
        let _ = writeln!(
            out,
            "rx_bytes={} rx_frames={} rx_corrupt={} keys={} taps={} shots={} log_dropped={}",
            l.rx_bytes,
            l.rx_frames,
            l.rx_corrupt,
            l.keys,
            l.taps,
            l.shots,
            crate::logbuf::dropped()
        );
        let g = &self.board.games;
        let _ = writeln!(
            out,
            "game running={} starts={} opened={} wipes={} load_us={} wipe_us={}",
            g.running, g.starts, g.opened, g.wipes, g.last.load_us, g.wipe_us
        );
        self.write_input_short(&mut out);
        self.write_light(&mut out);
        #[cfg(feature = "store")]
        if let Some(s) = &self.saver {
            s.write_short(&self.board.store, &mut out);
        }
        #[cfg(feature = "pad")]
        if let Some(r) = &self.radio {
            r.write_short(&self.board.pad, &mut out);
        }
        self.stats.write_short(&mut out);
        let len = out.as_bytes().len();
        // A full frame was cut short: say so on its last line instead of
        // ending in the middle of a number.
        if len == text.len() {
            const CUT: &[u8] = b"\n(cut: frame full)\n";
            text[len - CUT.len()..].copy_from_slice(CUT);
        }
        let mut frame = [0u8; MAX_FRAME];
        if let Ok(len) = (Reply::StatsText { text: &text[..len] }).encode(seq, &mut frame) {
            crate::logbuf::flush(&mut self.uart);
            send(&mut self.uart, &frame[..len]);
        }
    }

    fn set_baud(&mut self, baud: u32) -> bool {
        let _ = self.uart.flush();
        self.uart
            .apply_config(&UartConfig::default().with_baudrate(baud))
            .is_ok()
    }

    async fn wait_for_go(&mut self, seq: u8) -> bool {
        let deadline = Instant::now() + GO_WAIT;
        let mut decoder = Decoder::<COMMAND_DECODER_BUF>::new();
        let mut bytes = [0u8; 32];
        loop {
            if Instant::now() >= deadline {
                return false;
            }
            let got = match select(self.uart.read_async(&mut bytes), Timer::at(deadline)).await {
                Either::First(Ok(got)) => got,
                Either::First(Err(_)) => continue,
                Either::Second(()) => return false,
            };
            let mut go = false;
            decoder.feed(&bytes[..got], &mut |event| {
                if let Event::Frame(frame) = event {
                    go |= frame.kind == kind::GO && frame.seq == seq;
                }
            });
            if go {
                return true;
            }
        }
    }

    async fn shot(&mut self, seq: u8, source: Source, baud: u32, read_khz: u16) {
        if (baud != 0 && !(9600..=3_000_000).contains(&baud))
            || (read_khz != 0 && u32::from(read_khz) != READ_KHZ)
        {
            self.ack(seq, kind::SHOT, status::BAD_REQUEST);
            return;
        }
        let started = Instant::now();
        self.ack(seq, kind::SHOT, status::OK);
        let mut went = true;
        if baud != 0 {
            went = self.set_baud(baud) && self.wait_for_go(seq).await;
        }
        let mut failed = false;
        if went {
            let (format, width, height) = match source {
                Source::Panel => (PixelFormat::Rgb565, W as u16, H as u16),
                Source::Framebuffer => (
                    PixelFormat::Indexed8,
                    console_video::W as u16,
                    console_video::H as u16,
                ),
            };
            let mut sender = ImageSender::new(seq, source, format, width, height);
            let video = self.console.video();
            let palette = video.palette().colours();
            let lcd = &mut self.board.screen.lcd;
            let LinkState {
                frame,
                row,
                scratch,
                ..
            } = &mut self.link;
            let uart = &mut self.uart;
            loop {
                let fetch = |y: u16, out: &mut [u8]| match source {
                    Source::Panel => {
                        let rgb = lcd.read_row(0, y, W as u16, &mut scratch[..])?;
                        for (px, out) in rgb.chunks_exact(3).zip(out.chunks_exact_mut(2)) {
                            out.copy_from_slice(&crate::lcd::read_to_565(px).to_le_bytes());
                        }
                        Ok(())
                    }
                    Source::Framebuffer => {
                        out.copy_from_slice(video.frame().row(usize::from(y)));
                        Ok::<(), crate::lcd::LcdError>(())
                    }
                };
                match sender.next_frame(&mut row[..], Some(palette), fetch, &mut frame[..]) {
                    Ok(Some(len)) => send(uart, &frame[..len]),
                    Ok(None) => break,
                    Err(_) => {
                        failed = true;
                        break;
                    }
                }
            }
        }
        if failed {
            self.ack(seq, kind::SHOT, status::FAILED);
        }
        if baud != 0 {
            self.set_baud(debug_link::BAUD);
            // The PC goes back to the resting speed a little after us; a
            // log line sent before that would reach it as garbage.
            Timer::after(Duration::from_millis(60)).await;
        }
        self.board.screen.beam.forget();
        self.link.counters.shots += 1;
        self.link.counters.shot_ms = started.elapsed().as_millis() as u32;
        if !went {
            warn!("shot: no go from the PC at {baud} baud, nothing sent");
        } else if failed {
            warn!("shot: reading the panel failed");
        } else {
            info!(
                "shot: {:?} sent in {} ms",
                source, self.link.counters.shot_ms
            );
        }
    }
}
