//! The serial port, and talking to the board over it.

use std::{
    cell::Cell,
    collections::VecDeque,
    io::{Read, Write},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use debug_link::{
    frame::{Decoder, EncodeError, Event, Frame, MAX_FRAME},
    image::{Image, ImageReceiver},
    msg::{PlayerButtons, Reply, Request, Source, kind, status},
    pad::PadRequest,
};
use serialport::{ClearBuffer, SerialPort};

/// How often a request is sent again when no answer comes.
const TRIES: u32 = 3;
/// How long to wait for the answer to one try.
const ANSWER_WAIT: Duration = Duration::from_millis(700);

/// Turns the bytes that belong to no frame into lines of log text.
///
/// Two things are taken out. Colour codes, which only clutter a file.
/// And NMEA sentences: a GPS receiver may share this wire, and its
/// sentences hold the exact position of whoever owns the board, which
/// must not end up in a log someone pastes somewhere.
pub struct LogText {
    line: Vec<u8>,
    pub hidden_nmea: usize,
}

impl LogText {
    pub fn new() -> Self {
        Self { line: Vec::new(), hidden_nmea: 0 }
    }

    /// Takes one byte; returns a finished line when this byte ended one.
    pub fn push(&mut self, byte: u8) -> Option<String> {
        if byte != b'\n' {
            self.line.push(byte);
            // A line this long is not text. Cut it rather than grow.
            if self.line.len() < 2000 {
                return None;
            }
        }
        self.finish()
    }

    /// What is left of a line that never ended.
    pub fn finish(&mut self) -> Option<String> {
        let raw = String::from_utf8_lossy(&self.line).into_owned();
        self.line.clear();
        let mut text = String::new();
        let mut chars = raw.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // A colour code: ESC [ digits and ; then a letter.
                for code in chars.by_ref() {
                    if code.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else if c != '\r' {
                text.push(c);
            }
        }
        if looks_like_nmea(&text) {
            self.hidden_nmea += 1;
            return None;
        }
        (!text.is_empty()).then_some(text)
    }
}

/// "$GPGGA,...", "$GNRMC,..." and the like, also when the line starts
/// with a few damaged bytes.
fn looks_like_nmea(line: &str) -> bool {
    line.find('$').is_some_and(|at| {
        let rest = &line.as_bytes()[at + 1..];
        rest.len() > 5 && rest[..5].iter().all(u8::is_ascii_uppercase) && rest[5] == b','
    })
}

/// An answer from the board, copied out of the receive buffer.
#[derive(Debug)]
pub enum Answer {
    Ack { request: u8, status: u8 },
    Stats(String),
    PanelRaw(Vec<u8>),
}

/// What happened on the wire during one screenshot.
pub struct ShotReport {
    pub image: Image,
    pub baud: u32,
    /// Bytes of frames received for this picture.
    pub wire_bytes: usize,
    /// From sending the request to the last frame.
    pub took: Duration,
    /// How many times the whole screenshot had to be asked for.
    pub attempts: u32,
}

pub struct Link {
    port: Box<dyn SerialPort>,
    decoder: Box<Decoder<MAX_FRAME>>,
    seq: u8,
    log: LogText,
    /// Print the board's log lines seen while waiting for answers.
    pub show_log: bool,
    /// With `show_log`: how long to go on listening when done. The
    /// firmware logs what it did right after answering, and that line
    /// is the proof that a key or a tap arrived.
    pub linger: Duration,
    /// Frames that failed their check since the port was opened.
    pub corrupt: usize,
    /// Bytes received since the port was opened.
    pub rx_bytes: usize,
    /// Received bytes not yet looked at.
    pending: VecDeque<u8>,
}

impl Link {
    /// Opens the port without resetting the board.
    ///
    /// On an ESP32 board the port's two control lines are wired to the
    /// chip's reset and boot pins: reset happens when RTS is on while
    /// DTR is off. Linux turns both on when a port is opened and both
    /// off when it is closed, each time together, and with both in the
    /// same state the board's circuit does nothing. So the trick is to
    /// touch neither line, which is what this does. `reset` is the one
    /// place that moves them.
    pub fn open(path: &str) -> Result<Self> {
        let port = serialport::new(path, debug_link::BAUD)
            .timeout(Duration::from_millis(20))
            .open()
            .with_context(|| format!("cannot open {path}"))?;
        // Whatever arrived before we were listening is not for us.
        port.clear(ClearBuffer::Input).ok();
        // A different starting number on every run: an answer still on
        // its way to an earlier run cannot be taken for ours.
        let seq = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| (since.subsec_nanos() >> 8) as u8);
        Ok(Self { port, decoder: Box::new(Decoder::new()), seq, log: LogText::new(), show_log: false, linger: Duration::ZERO, corrupt: 0, rx_bytes: 0, pending: VecDeque::new() })
    }

    /// Restarts the board, as pressing its reset button would.
    pub fn reset(&mut self) -> Result<()> {
        // DTR off while RTS is on pulls the chip's enable pin low.
        self.port.write_data_terminal_ready(false)?;
        self.port.write_request_to_send(true)?;
        std::thread::sleep(Duration::from_millis(100));
        // RTS off lets it go again. With DTR off too the boot pin stays
        // high, so the chip starts the firmware and not the boot loader.
        self.port.write_request_to_send(false)?;
        Ok(())
    }

    /// Drops what was received but not yet looked at, and any frame
    /// half taken in. For when the line's speed has just changed.
    fn forget(&mut self) {
        self.pending.clear();
        *self.decoder = Decoder::new();
    }

    fn next_seq(&mut self) -> u8 {
        self.seq = self.seq.wrapping_add(1);
        self.seq
    }

    fn send(&mut self, encode: &impl Fn(u8, &mut [u8]) -> Result<usize, EncodeError>, seq: u8) -> Result<()> {
        let mut bytes = [0u8; 128];
        let len = encode(seq, &mut bytes).map_err(|error| anyhow::anyhow!("cannot encode the request: {error:?}"))?;
        self.port.write_all(&bytes[..len])?;
        self.port.flush()?;
        Ok(())
    }

    /// Sends bytes as they are, for testing what the firmware makes of
    /// things that are not frames.
    pub fn send_raw(&mut self, bytes: &[u8]) -> Result<()> {
        self.port.write_all(bytes)?;
        self.port.flush()?;
        Ok(())
    }

    fn print_log(&self, line: &str) {
        if self.show_log {
            eprintln!("[board] {line}");
        }
    }

    /// Reads from the port until `deadline`, or until `on_frame` returns
    /// something. Log text goes to `on_text` line by line.
    ///
    /// Bytes that came in the same read as the wanted frame, but after
    /// it, stay in `pending` for the next call. Without that, the first
    /// frames of a picture were lost whenever they arrived together with
    /// the acknowledgement before them.
    fn pump<T>(
        &mut self,
        deadline: Instant,
        mut on_frame: impl FnMut(&Frame<'_>) -> Result<Option<T>>,
        mut on_text: impl FnMut(&str),
    ) -> Result<Option<T>> {
        let mut quiet_since = Instant::now();
        loop {
            if self.pending.is_empty() {
                let mut bytes = [0u8; 4096];
                match self.port.read(&mut bytes) {
                    Ok(got) => {
                        self.rx_bytes += got;
                        self.pending.extend(&bytes[..got]);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(error) => return Err(error).context("reading the serial port"),
                }
            }
            let mut result = Ok(None);
            let mut corrupt = 0;
            let done = Cell::new(false);
            let log = &mut self.log;
            let mut sink = |event: Event<'_>| match event {
                Event::Text(byte) => {
                    if let Some(line) = log.push(byte) {
                        on_text(&line);
                    }
                }
                Event::Corrupt => corrupt += 1,
                Event::Frame(frame) => {
                    result = on_frame(&frame);
                    done.set(!matches!(result, Ok(None)));
                }
            };
            if self.pending.is_empty() {
                // A frame cut short would hold up the text after it.
                if quiet_since.elapsed() > Duration::from_millis(100) {
                    self.decoder.idle(&mut sink);
                }
            } else {
                quiet_since = Instant::now();
                while let Some(byte) = self.pending.pop_front() {
                    self.decoder.push(byte, &mut sink);
                    if done.get() {
                        break;
                    }
                }
            }
            self.corrupt += corrupt;
            match result {
                Ok(None) => {}
                done => return done,
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
        }
    }

    /// Sends a request and waits for the board's answer to it, sending
    /// it again a few times if none comes.
    pub fn ask(&mut self, request: &Request<'_>) -> Result<Answer> {
        self.ask_with(|seq, out| request.encode(seq, out))
    }

    /// Like [`Self::ask`], for an injected-pad request (`debug_link::pad`).
    pub fn ask_pad(&mut self, request: &PadRequest<'_>) -> Result<()> {
        match self.ask_with(|seq, out| request.encode(seq, out))? {
            Answer::Ack { status: status::OK, .. } => Ok(()),
            Answer::Ack { status, .. } => bail!("the board refused: {}", status_name(status)),
            other => bail!("unexpected answer: {other:?}"),
        }
    }

    /// The buttons held from now on on pad `player` (0 to 3). Pad 0 goes
    /// as the old `Buttons` request, which every firmware knows; pads 1
    /// to 3 as `PlayerButtons`, which an older firmware refuses as
    /// unsupported.
    pub fn set_buttons(&mut self, player: u8, mask: u16) -> Result<()> {
        if player == 0 {
            return self.ask_ok(&Request::Buttons { mask });
        }
        let request = PlayerButtons { player, mask };
        match self.ask_with(|seq, out| request.encode(seq, out))? {
            Answer::Ack { status: status::OK, .. } => Ok(()),
            Answer::Ack { status, .. } => bail!("the board refused: {}", status_name(status)),
            other => bail!("unexpected answer: {other:?}"),
        }
    }

    fn ask_with(&mut self, encode: impl Fn(u8, &mut [u8]) -> Result<usize, EncodeError>) -> Result<Answer> {
        let seq = self.next_seq();
        let mut lines = Vec::new();
        for _ in 0..TRIES {
            self.send(&encode, seq)?;
            let answer = self.pump(
                Instant::now() + ANSWER_WAIT,
                |frame| {
                    if frame.seq != seq {
                        return Ok(None);
                    }
                    Ok(match Reply::parse(frame) {
                        Ok(Reply::Ack { request, status }) => Some(Answer::Ack { request, status }),
                        Ok(Reply::StatsText { text }) => Some(Answer::Stats(String::from_utf8_lossy(text).into_owned())),
                        Ok(Reply::PanelRawReply { bytes }) => Some(Answer::PanelRaw(bytes.to_vec())),
                        _ => None,
                    })
                },
                |line| lines.push(line.to_owned()),
            )?;
            for line in lines.drain(..) {
                self.print_log(&line);
            }
            if let Some(answer) = answer {
                return Ok(answer);
            }
        }
        bail!(
            "no answer from the board after {TRIES} tries. Is a firmware with the debug link running, and is the line at {} baud?",
            debug_link::BAUD
        )
    }

    /// Like [`Self::ask`], for requests whose whole answer is "done".
    pub fn ask_ok(&mut self, request: &Request<'_>) -> Result<()> {
        match self.ask(request)? {
            Answer::Ack { status: status::OK, .. } => Ok(()),
            Answer::Ack { status, .. } => bail!("the board refused: {}", status_name(status)),
            other => bail!("unexpected answer: {other:?}"),
        }
    }

    /// Reads until the line has been silent for `quiet`, or `limit` has
    /// passed. Used after a failed screenshot: the board goes on sending
    /// the rest of it.
    fn wait_for_silence(&mut self, quiet: Duration, limit: Duration) {
        let end = Instant::now() + limit;
        let mut last = Instant::now();
        let mut bytes = [0u8; 4096];
        while Instant::now() < end && last.elapsed() < quiet {
            if matches!(self.port.read(&mut bytes), Ok(got) if got > 0) {
                last = Instant::now();
            }
        }
        self.forget();
    }

    /// Takes a screenshot. `baud` is the speed for the picture's frames;
    /// the request itself always goes at the resting speed.
    pub fn shot(&mut self, source: Source, baud: u32, read_khz: u16, tries: u32, limit: Duration) -> Result<ShotReport> {
        let mut last_error = None;
        for attempt in 1..=tries {
            match self.shot_once(source, baud, read_khz, limit) {
                Ok(mut report) => {
                    report.attempts = attempt;
                    return Ok(report);
                }
                Err(error) => {
                    eprintln!("screenshot attempt {attempt} failed: {error:#}");
                    last_error = Some(error);
                    // Let the board finish and return to the resting
                    // speed before asking again.
                    self.wait_for_silence(Duration::from_millis(400), Duration::from_secs(30));
                    self.port.set_baud_rate(debug_link::BAUD)?;
                    std::thread::sleep(Duration::from_millis(50));
                    self.port.clear(ClearBuffer::Input).ok();
                    self.forget();
                }
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("no attempt was made")))
    }

    fn shot_once(&mut self, source: Source, baud: u32, read_khz: u16, limit: Duration) -> Result<ShotReport> {
        let change_speed = baud != debug_link::BAUD;
        let request = Request::Shot { source, baud: if change_speed { baud } else { 0 }, read_khz };
        let started = Instant::now();
        let rx_start = self.rx_bytes;
        // `ask` hands back the acknowledgement; the picture's frames
        // carry the same sequence number.
        match self.ask(&request)? {
            Answer::Ack { request: kind::SHOT, status: status::OK } => {}
            Answer::Ack { status, .. } => bail!("the board refused the screenshot: {}", status_name(status)),
            other => bail!("unexpected answer: {other:?}"),
        }
        let seq = self.seq;

        let result = (|| -> Result<(Image, usize)> {
            if change_speed {
                // The board changed speed right after its acknowledgement
                // and now waits for "go" at the new speed.
                self.port.set_baud_rate(baud)?;
                std::thread::sleep(Duration::from_millis(30));
                self.port.clear(ClearBuffer::Input).ok();
                self.forget();
            }
            let mut receiver = ImageReceiver::new(seq);
            let mut wire_bytes = 0;
            let mut begun = false;
            let mut lines = Vec::new();
            let deadline = Instant::now() + limit;
            // Until the first frame arrives, "go" is repeated: the first
            // one may have left before the board was listening.
            let mut go_sent = 0;
            loop {
                if change_speed && !begun && go_sent < 4 {
                    self.send(&|seq, out: &mut [u8]| Request::Go.encode(seq, out), seq)?;
                    go_sent += 1;
                }
                let slice = if begun { deadline } else { (Instant::now() + Duration::from_millis(300)).min(deadline) };
                let image = self.pump(
                    slice,
                    |frame| {
                        if frame.seq == seq {
                            begun = true;
                            wire_bytes += frame.payload.len() + debug_link::frame::OVERHEAD;
                        }
                        Ok(receiver.on_frame(frame)?)
                    },
                    |line| lines.push(line.to_owned()),
                )?;
                for line in lines.drain(..) {
                    self.print_log(&line);
                }
                if let Some(image) = image {
                    return Ok((image, wire_bytes));
                }
                if Instant::now() >= deadline {
                    bail!("the picture did not arrive within {} s", limit.as_secs());
                }
                if !begun && (!change_speed || go_sent >= 4) && started.elapsed() > Duration::from_secs(3) {
                    bail!("the board acknowledged but sent no picture");
                }
            }
        })();
        let took = started.elapsed();

        if change_speed && result.is_ok() {
            // The board goes back by itself after the last frame.
            self.port.set_baud_rate(debug_link::BAUD)?;
        }
        let rx_before = rx_start;
        let (image, wire_bytes) = result.with_context(|| {
            format!("{} bytes received, {} frames failed their check", self.rx_bytes - rx_before, self.corrupt)
        })?;
        Ok(ShotReport { image, baud, wire_bytes, took, attempts: 1 })
    }

    /// Prints the board's log for a while. Frames are left out.
    pub fn log(&mut self, time: Duration, mut print: impl FnMut(&str)) -> Result<()> {
        self.pump(Instant::now() + time, |_| Ok(None::<()>), &mut print)?;
        if let Some(rest) = self.log.finish() {
            print(&rest);
        }
        Ok(())
    }

    /// NMEA sentences kept out of the log so far.
    pub fn hidden_nmea(&self) -> usize {
        self.log.hidden_nmea
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        if self.show_log && !self.linger.is_zero() {
            let _ = self.log(self.linger, |line| eprintln!("[board] {line}"));
        }
    }
}

pub fn status_name(code: u8) -> String {
    match code {
        status::OK => "ok".into(),
        status::UNSUPPORTED => "this firmware does not know the request".into(),
        status::BAD_REQUEST => "bad request".into(),
        status::FAILED => "it tried and failed".into(),
        other => format!("status {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(bytes: &[u8]) -> (Vec<String>, usize) {
        let mut log = LogText::new();
        let mut out: Vec<String> = bytes.iter().filter_map(|&byte| log.push(byte)).collect();
        out.extend(log.finish());
        (out, log.hidden_nmea)
    }

    #[test]
    fn colour_codes_are_taken_out() {
        let (out, _) = lines(b"\x1b[32mINFO - ready\x1b[0m\r\nplain\r\n");
        assert_eq!(out, ["INFO - ready", "plain"]);
    }

    #[test]
    fn nmea_sentences_are_hidden() {
        let (out, hidden) = lines(b"INFO - a\r\n$GPGGA,123519,4807.038,N,01131.000,E,1,08,0.9,545.4,M,46.9,M,,*47\r\n\xff$GNRMC,1,2\r\nINFO - b $5\r\n");
        assert_eq!(out, ["INFO - a", "INFO - b $5"]);
        assert_eq!(hidden, 2);
    }
}
