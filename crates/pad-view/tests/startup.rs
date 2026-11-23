//! The Switch start-up conversation: the prober that finds the way of
//! talking a pad answers, the bytes sent, the counter, the answers and
//! the retries.
//!
//! Most tests run a fake pad that answers only one way of talking, with
//! the firmware's calling pattern: `next_output` right after every input
//! report and every 20 ms, up to 4 packets in a row.

use pad_view::startup::{
    Channel, NO_REPORT_START_MS, Output, PROBE_TRIES, Phase, SETTLE_MS, STEP_TRIES,
    SWITCH_PACKET_LEN, StepState, TRY_WAIT_MS, VARIANTS, Way, private_bytes,
};
use pad_view::{Device, View};

fn switch_view() -> View {
    let mut v = View::new();
    v.device(&Device {
        name: "Pro Controller",
        class_of_device: 0x2508,
        ..Default::default()
    });
    v
}

/// A 0x21 answer to subcommand `sub`. For device info: firmware 3.72,
/// type 3, and a made-up address in bytes 19-24.
fn reply(sub: u8) -> Vec<u8> {
    let mut r = vec![0u8; 49];
    r[0] = 0x21;
    r[13] = 0x80;
    r[14] = sub;
    if sub == 0x02 {
        r[13] = 0x82;
        r[15..19].copy_from_slice(&[0x03, 0x48, 0x03, 0x02]);
        r[19..25].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    }
    r
}

const SIMPLE: [u8; 12] = [0x3F, 0, 0, 8, 0, 0x80, 0, 0x80, 0, 0x80, 0, 0x80];

fn full_report() -> Vec<u8> {
    let mut r = vec![0u8; 49];
    r[0] = 0x30;
    r
}

/// One packet the view gave, as the fake pad sees it.
#[derive(Clone, Debug)]
struct Sent {
    out: Output,
    bytes: Vec<u8>,
    /// Given in the call right after an input report.
    after_report: bool,
}

impl Sent {
    fn sub(&self) -> u8 {
        self.bytes[10]
    }
    fn neutral(&self) -> bool {
        self.bytes[2..10] == [0x00, 0x01, 0x40, 0x40, 0x00, 0x01, 0x40, 0x40]
    }
    /// Sent the way `w` says.
    fn is_way(&self, w: Way) -> bool {
        self.out.channel == w.channel
            && (self.out.len == SWITCH_PACKET_LEN) == w.full_len
            && self.neutral() == w.neutral
            && (!w.after_report || self.after_report)
    }
}

/// A pad that answers only packets `accepts` likes, `latency` ms later,
/// and nothing before `ready_at`. It sends 0x3F every `period` ms at
/// offset `offset` (0x30 once full report mode is on).
struct Pad {
    accepts: Box<dyn Fn(&Sent) -> bool>,
    latency: u32,
    ready_at: u32,
    period: u32,
    offset: u32,
    answers: Vec<(u32, Vec<u8>)>,
    full_from: Option<u32>,
    full_from_start: bool,
}

impl Pad {
    fn new(accepts: impl Fn(&Sent) -> bool + 'static) -> Self {
        Pad {
            accepts: Box::new(accepts),
            latency: 20,
            ready_at: 0,
            // Reports (at odd times) never fall on a 20 ms tick, and a
            // first report + 300 ms is never a report time either: every
            // packet not paced by reports goes out on a tick, never
            // "right after a report".
            period: 70,
            offset: 13,
            answers: Vec::new(),
            full_from: None,
            full_from_start: false,
        }
    }

    /// Speaks exactly the way of variant `name` (and so of any later
    /// variant with the same way).
    fn speaking(name: &str) -> Self {
        let w = VARIANTS.iter().find(|v| v.name == name).unwrap().way;
        Pad::new(move |s| s.is_way(w))
    }

    fn deaf() -> Self {
        Pad::new(|_| false)
    }
}

struct Sim {
    v: View,
    t: u32,
    pad: Pad,
    sent: Vec<Sent>,
    notes: Vec<String>,
    /// Handshakes the pad gave on the control channel.
    handshakes: u32,
}

impl Sim {
    fn new(pad: Pad) -> Self {
        Sim {
            v: switch_view(),
            t: 0,
            pad,
            sent: Vec::new(),
            notes: Vec::new(),
            handshakes: 0,
        }
    }

    fn drain(&mut self) {
        while let Some(n) = self.v.take_startup_note() {
            self.notes.push(n.to_string());
        }
    }

    /// The firmware's output loop: up to 4 packets in a row.
    fn give(&mut self, after_report: bool) {
        for _ in 0..4 {
            let mut buf = [0xAAu8; 64];
            let Some(out) = self.v.next_output(&mut buf) else {
                break;
            };
            assert!(
                buf[out.len..].iter().all(|b| *b == 0xAA),
                "nothing past the packet"
            );
            let s = Sent {
                out,
                bytes: buf[..out.len].to_vec(),
                after_report,
            };
            if self.t >= self.pad.ready_at && (self.pad.accepts)(&s) {
                let at = self.t + self.pad.latency;
                self.pad.answers.push((at, reply(s.sub())));
                if s.sub() == 0x03 && s.bytes[11] == 0x30 {
                    self.pad.full_from.get_or_insert(at + 5);
                }
                if out.channel == Channel::Control {
                    self.handshakes += 1;
                    self.v.control(&[0x00]);
                }
            } else if out.channel == Channel::Control {
                // A pad that does not take SET_REPORT says so.
                self.handshakes += 1;
                self.v.control(&[0x03]);
            }
            self.sent.push(s);
        }
        self.drain();
    }

    fn report(&mut self, r: &[u8]) {
        self.v.report(r);
        self.drain();
        self.give(true);
    }

    /// Runs the clock to `until` in steps of 1 ms.
    fn run(&mut self, until: u32) {
        while self.t < until {
            self.t += 1;
            let t = self.t;
            self.v.set_time_ms(t);
            let due: Vec<Vec<u8>> = {
                let (now, later): (Vec<_>, Vec<_>) =
                    self.pad.answers.drain(..).partition(|(at, _)| *at <= t);
                self.pad.answers = later;
                now.into_iter().map(|a| a.1).collect()
            };
            for a in due {
                self.report(&a);
            }
            if t % self.pad.period == self.pad.offset {
                let full = self.pad.full_from_start || self.pad.full_from.is_some_and(|f| t >= f);
                if full {
                    self.report(&full_report());
                } else {
                    self.report(&SIMPLE);
                }
            }
            if t.is_multiple_of(20) {
                self.give(false);
            }
        }
    }

    fn outcome(&self) -> String {
        self.v.startup().outcome().to_string()
    }

    /// When the conversation ended (the last note's time is not kept, so
    /// run until finished in 10 ms steps).
    fn run_until_finished(&mut self, limit: u32) -> u32 {
        while !self.v.startup().finished() {
            assert!(
                self.t < limit,
                "not finished after {} ms: {}",
                limit,
                self.outcome()
            );
            let next = self.t + 10;
            self.run(next);
        }
        self.t
    }

    fn has_note(&self, start: &str) -> bool {
        self.notes.iter().any(|n| n.starts_with(start))
    }
}

/// Every variant has a fake pad that answers only that way. The prober
/// finds it, then finishes the conversation that way, in time.
#[test]
fn each_variant_is_found_by_a_pad_that_speaks_only_it() {
    for (i, var) in VARIANTS.iter().enumerate() {
        let pad = if var.sub == 0x03 {
            // Takes 49-byte neutral packets, but not device info.
            let w = var.way;
            Pad::new(move |s| s.is_way(w) && s.sub() != 0x02)
        } else if var.way.channel == Channel::Control {
            Pad::new(|s| s.out.channel == Channel::Control)
        } else if var.way.after_report {
            Pad::new(|s| s.is_way(VARIANTS[4].way) && s.after_report)
        } else {
            // Exactly this length and rumble, on the interrupt channel.
            let w = var.way;
            Pad::new(move |s| {
                s.out.channel == Channel::Interrupt
                    && (s.out.len == SWITCH_PACKET_LEN) == w.full_len
                    && s.neutral() == w.neutral
            })
        };
        let mut s = Sim::new(pad);
        let end = s.run_until_finished(12_000);
        assert_eq!(
            s.outcome(),
            format!("won:{},03:ok,40:ok,30:ok", var.name),
            "variant {} notes:\n{}",
            var.name,
            s.notes.join("\n")
        );
        assert!(end <= 12_000, "{}: finished at {} ms", var.name, end);
        // Each earlier variant was tried 3 times, unanswered.
        let tried: Vec<u8> = s.sent.iter().map(|p| p.sub()).collect();
        let probes = i * 3 + 1;
        assert!(tried.len() >= probes, "{}: {tried:02x?}", var.name);
        // After the win, every packet goes the winner's way, in the
        // order 03 30, 40 01, 30 01.
        let won_at = s
            .notes
            .iter()
            .position(|n| n.starts_with(&format!("variant={} WON", var.name)))
            .expect("a WON line");
        assert!(
            s.notes[won_at..]
                .iter()
                .any(|n| n == "conversation finished")
        );
        let first = s
            .sent
            .iter()
            .position(|p| p.sub() != 0x02)
            .expect("steps sent");
        let steps: Vec<(u8, u8)> = s.sent[first..]
            .iter()
            .map(|p| (p.sub(), p.bytes[11]))
            .collect();
        let want: &[(u8, u8)] = &[(0x03, 0x30), (0x40, 0x01), (0x30, 0x01)];
        assert_eq!(steps, want, "{}", var.name);
        for p in &s.sent[first..] {
            assert!(
                p.is_way(var.way),
                "{}: step sent another way: {p:?}",
                var.name
            );
        }
        // Device info was read, and its address never reached a note.
        if var.sub == 0x02 {
            let d = s.v.startup().device_info.expect("device info");
            assert_eq!((d.firmware, d.kind), ((3, 72), 3));
            assert!(s.has_note("device info: firmware 3.72 type=3 (Pro Controller)"));
        }
        for n in &s.notes {
            assert!(!n.to_lowercase().contains("aabbcc"), "address in {n}");
        }
        // Nothing more after the end.
        let n = s.sent.len();
        s.run(end + 3000);
        assert_eq!(s.sent.len(), n, "{}: sent after the end", var.name);
    }
}

/// A pad that never answers: every variant 3 times, then the end, in
/// time; the 0x3F reports are still decoded.
#[test]
fn a_pad_that_never_answers() {
    let mut s = Sim::new(Pad::deaf());
    let end = s.run_until_finished(12_000);
    let names: Vec<&str> = VARIANTS.iter().map(|v| v.name).collect();
    assert_eq!(
        s.outcome(),
        format!("all-variants-failed({})", names.join(","))
    );
    assert_eq!(s.v.startup().phase(), Phase::AllFailed);
    assert_eq!(s.sent.len(), VARIANTS.len() * PROBE_TRIES as usize);
    assert!(end < 12_000, "ended at {end} ms");
    assert!(s.has_note("all 7 variants failed (info-49,"));
    for v in VARIANTS {
        assert!(s.has_note(&format!("variant={} had no answer after 3 tries", v.name)));
        for k in 1..=3 {
            assert!(
                s.notes
                    .iter()
                    .any(|n| n.starts_with(&format!("variant={} sent ", v.name))
                        && n.ends_with(&format!("try {k} of 3"))),
                "{} try {k}",
                v.name
            );
        }
    }
    // The SET_REPORT variant went on the control channel and its
    // refusals were noted.
    assert_eq!(s.handshakes, 3);
    assert!(s.has_note("HID handshake result=0x3 during variant=info-setreport"));
    // Nothing more is sent, and the reports still reach the screen.
    let n = s.sent.len();
    s.run(end + 5000);
    assert_eq!(s.sent.len(), n);
    assert_eq!(s.v.last_change().id, 0x3F);
}

/// The exact first log lines of a probe.
#[test]
fn log_lines() {
    let mut s = Sim::new(Pad::speaking("info-short-zero"));
    s.run_until_finished(12_000);
    let want = [
        "probe starts, 307 ms after the first input report; 7 ways to try",
        "variant=info-49 sent subcommand 0x02 (device info) len=49 rumble=neutral channel=interrupt counter=0, way 1 of 7, try 1 of 3",
        "variant=info-49 sent subcommand 0x02 (device info) len=49 rumble=neutral channel=interrupt counter=1, way 1 of 7, try 2 of 3",
        "variant=info-49 sent subcommand 0x02 (device info) len=49 rumble=neutral channel=interrupt counter=2, way 1 of 7, try 3 of 3",
        "variant=info-49 had no answer after 3 tries",
        "variant=info-short-zero sent subcommand 0x02 (device info) len=11 rumble=zero channel=interrupt counter=3, way 2 of 7, try 1 of 3",
        "device info: firmware 3.72 type=3 (Pro Controller)",
        "answer to subcommand 0x02 (device info) ack=0x82 20 ms after the last send, credited to variant=info-short-zero",
        "variant=info-short-zero WON after 920 ms of probing; the rest goes the same way (len=short rumble=zero channel=interrupt); variant=info-49 sent the same subcommand 320 ms before the answer, so it may be for it: it takes over if no step is answered",
        "variant=info-short-zero sent subcommand 0x03/0x30 (full report mode) len=12 rumble=zero channel=interrupt counter=4, step 1 of 3, try 1 of 3",
        "answer to subcommand 0x03 (full report mode) ack=0x80 20 ms after the last send, credited to variant=info-short-zero",
        "variant=info-short-zero sent subcommand 0x40/0x01 (motion sensors on) len=12 rumble=zero channel=interrupt counter=5, step 2 of 3, try 1 of 3",
        "answer to subcommand 0x40 (motion sensors on) ack=0x80 20 ms after the last send, credited to variant=info-short-zero",
        "variant=info-short-zero sent subcommand 0x30/0x01 (player 1 light) len=12 rumble=zero channel=interrupt counter=6, step 3 of 3, try 1 of 3",
        "answer to subcommand 0x30 (player 1 light) ack=0x80 20 ms after the last send, credited to variant=info-short-zero",
        "conversation finished",
    ];
    assert_eq!(&s.notes[..want.len()], &want, "\n{}", s.notes.join("\n"));
    assert_eq!(s.notes.last().unwrap(), "conversation finished");
}

#[test]
fn nothing_for_other_pads() {
    let mut v = View::new();
    v.device(&Device {
        name: "Xbox Wireless Controller",
        ..Default::default()
    });
    let mut buf = [0u8; 49];
    v.set_time_ms(0);
    v.report(&[0x01, 0, 0, 0]);
    v.set_time_ms(5000);
    // No start-up packet. The only packet is the battery question
    // (GET_REPORT for report 04, see pad_view::battery), once.
    assert_eq!(
        v.next_output(&mut buf),
        Some(Output {
            len: 1,
            channel: Channel::GetReport
        })
    );
    assert_eq!(buf[0], 0x04);
    assert_eq!(v.next_output(&mut buf), None);
    assert_eq!(v.startup().outcome().to_string(), "not-needed");
    assert_eq!(v.take_startup_note(), None);
}

/// Packets byte by byte: SDL's 49-byte form and Linux's short form.
#[test]
fn packet_bytes() {
    let mut v = switch_view();
    v.set_time_ms(0);
    v.report(&SIMPLE);
    let mut buf = [0xAAu8; 64];
    v.set_time_ms(SETTLE_MS);
    let out = v.next_output(&mut buf).expect("info-49");
    assert_eq!(
        out,
        Output {
            len: 49,
            channel: Channel::Interrupt
        }
    );
    assert_eq!(
        &buf[..11],
        &[
            0x01, 0x00, 0x00, 0x01, 0x40, 0x40, 0x00, 0x01, 0x40, 0x40, 0x02
        ]
    );
    assert!(buf[11..49].iter().all(|b| *b == 0), "rest is zero");
    assert_eq!(buf[49], 0xAA, "nothing written past 49 bytes");
    // Run info-49 out (3 tries); the call after its last wait gives
    // info-short-zero's first packet.
    let mut t = SETTLE_MS;
    let mut last = None;
    for _ in 0..3 {
        t += TRY_WAIT_MS;
        v.set_time_ms(t);
        buf = [0xAA; 64];
        last = v.next_output(&mut buf);
    }
    assert_eq!(
        last,
        Some(Output {
            len: 11,
            channel: Channel::Interrupt
        })
    );
    assert_eq!(
        &buf[..12],
        &[0x01, 0x03, 0, 0, 0, 0, 0, 0, 0, 0, 0x02, 0xAA],
        "11 bytes, zero rumble, counter 3, nothing written past them"
    );
}

#[test]
fn short_buffer_gets_nothing() {
    let mut v = switch_view();
    v.set_time_ms(0);
    v.report(&SIMPLE);
    v.set_time_ms(SETTLE_MS);
    let mut small = [0u8; 48];
    assert_eq!(v.next_output(&mut small), None, "too small: nothing");
    let mut b49 = [0u8; 49];
    assert!(v.next_output(&mut b49).is_some());
}

/// Nothing goes out before the first report plus the settle time; with
/// no report at all, the probe starts a second after the first call.
#[test]
fn waits_for_the_first_report_and_settles() {
    let mut v = switch_view();
    let mut buf = [0u8; 64];
    v.set_time_ms(100);
    assert_eq!(v.next_output(&mut buf), None);
    v.report(&SIMPLE);
    v.set_time_ms(100 + SETTLE_MS - 1);
    assert_eq!(v.next_output(&mut buf), None, "settling");
    assert_eq!(v.startup().outcome().to_string(), "waiting-to-start");
    v.set_time_ms(100 + SETTLE_MS);
    assert!(v.next_output(&mut buf).is_some());
    assert_eq!(
        v.startup().outcome().to_string(),
        "probing:info-49(try 1 of 3)"
    );

    let mut v = switch_view();
    v.set_time_ms(5000);
    assert_eq!(v.next_output(&mut buf), None);
    v.set_time_ms(5000 + NO_REPORT_START_MS - 1);
    assert_eq!(v.next_output(&mut buf), None);
    v.set_time_ms(5000 + NO_REPORT_START_MS);
    assert!(v.next_output(&mut buf).is_some());
    let n = v.take_startup_note().unwrap().to_string();
    assert!(
        n.starts_with("probe starts with no input report seen"),
        "{n}"
    );
}

/// Without the time, each report counts as 10 ms: the probe still runs
/// and ends.
#[test]
fn works_without_a_clock() {
    let mut v = switch_view();
    let mut buf = [0u8; 64];
    let mut sent = 0;
    for _ in 0..3000 {
        v.report(&SIMPLE);
        if v.next_output(&mut buf).is_some() {
            sent += 1;
        }
    }
    assert!(v.startup().finished());
    assert_eq!(sent, VARIANTS.len() * PROBE_TRIES as usize);
}

/// The packet counter goes 0..15 and round again.
#[test]
fn counter_wraps_at_16() {
    let mut s = Sim::new(Pad::deaf());
    s.run_until_finished(12_000);
    let counters: Vec<u8> = s.sent.iter().map(|p| p.bytes[1]).collect();
    let want: Vec<u8> = (0..counters.len()).map(|k| (k % 16) as u8).collect();
    assert_eq!(counters, want);
}

/// Review finding (earlier version): the firmware asked for start-up
/// packets before the HID channel was open, the host could not send
/// them, and the tries were used up into the void. A packet that did not
/// go out must not count as a try, and its "sent" note is taken back.
#[test]
fn a_packet_not_sent_is_not_a_try() {
    let mut v = switch_view();
    let mut buf = [0u8; 64];
    v.set_time_ms(0);
    v.report(&SIMPLE);
    // Four seconds of asking every 20 ms with no channel to send on.
    let mut t = SETTLE_MS;
    let mut notes = Vec::new();
    for _ in 0..200 {
        v.set_time_ms(t);
        if v.next_output(&mut buf).is_some() {
            v.output_not_sent();
        }
        while let Some(n) = v.take_startup_note() {
            notes.push(n.to_string());
        }
        t += 20;
    }
    assert_eq!(v.startup().sent(), 0, "nothing was counted");
    assert_eq!(
        v.startup().outcome().to_string(),
        "waiting-to-start",
        "not even started"
    );
    assert!(notes.is_empty(), "{notes:?}");
    // The channel opens: the first packet is the very first one.
    v.set_time_ms(t);
    let out = v.next_output(&mut buf).expect("device info");
    assert_eq!(out.len, 49);
    assert_eq!((buf[1], buf[10]), (0, 0x02), "counter 0, subcommand 0x02");
    assert_eq!(
        v.startup().outcome().to_string(),
        "probing:info-49(try 1 of 3)"
    );
    assert_eq!(v.startup().sent(), 1);
}

/// A retry that could not go out keeps the earlier try counted and is
/// given again at once, with the same counter.
#[test]
fn a_retry_not_sent_is_given_again() {
    let mut v = switch_view();
    let mut buf = [0u8; 64];
    v.set_time_ms(1000);
    v.report(&SIMPLE);
    v.set_time_ms(1000 + SETTLE_MS);
    v.next_output(&mut buf).unwrap();
    v.set_time_ms(1000 + SETTLE_MS + TRY_WAIT_MS);
    v.next_output(&mut buf).unwrap();
    assert_eq!(buf[1], 1);
    assert_eq!(v.startup().probing().unwrap().1, 2);
    v.output_not_sent();
    assert_eq!(v.startup().probing().unwrap().1, 1);
    v.next_output(&mut buf)
        .expect("given again without waiting");
    assert_eq!((buf[1], buf[10]), (1, 0x02), "same counter, same packet");
    assert_eq!(v.startup().probing().unwrap().1, 2);
    // Calling it with nothing given changes nothing.
    v.output_not_sent();
    v.output_not_sent();
    assert_eq!(v.startup().probing().unwrap().1, 1);
    assert_eq!(v.startup().sent(), 1);
}

/// An answer slower than the wait still counts for the variant that is
/// still running.
#[test]
fn a_slow_answer_still_wins() {
    let mut pad = Pad::speaking("info-49");
    pad.latency = TRY_WAIT_MS + 100;
    let mut s = Sim::new(pad);
    s.run_until_finished(12_000);
    assert_eq!(s.outcome(), "won:info-49,03:ok,40:ok,30:ok");
    // Its first try was answered while the second was out.
    assert!(s.has_note("answer to subcommand 0x02 (device info) ack=0x82 100 ms after the last send, credited to variant=info-49"), "{}", s.notes.join("\n"));
}

/// An answer to the last try of one variant that arrives after the next
/// variant has sent: it is credited to the newer one, the win is marked
/// unclear, the newer way gets no step answered, and the older variant
/// takes over and finishes the conversation.
#[test]
fn an_answer_for_an_older_try() {
    let mut pad = Pad::speaking("info-49");
    // Deaf until the third try of info-49, slow to answer.
    pad.ready_at = 320 + 2 * TRY_WAIT_MS;
    pad.latency = TRY_WAIT_MS + 100;
    let mut s = Sim::new(pad);
    let end = s.run_until_finished(12_000);
    let notes = s.notes.join("\n");
    assert!(s.has_note("variant=info-short-zero WON after"), "{notes}");
    assert!(
        notes.contains("variant=info-49 sent the same subcommand 400 ms before the answer"),
        "{notes}"
    );
    assert!(
        s.has_note(
            "no step answered the way of variant=info-short-zero; variant=info-49 takes over"
        ),
        "{notes}"
    );
    assert_eq!(s.outcome(), "won:info-49,03:ok,40:ok,30:ok", "{notes}");
    assert!(end <= 12_000);
}

/// An answer to a step that was already given up is still taken, and
/// the step after it goes on.
#[test]
fn a_late_step_answer() {
    // Answers everything 49-byte-neutral, but "motion on" only very late.
    let w = VARIANTS[0].way;
    let mut s = Sim::new(Pad::new(move |p| p.is_way(w)));
    // Run until the 40 01 step is out, then hold its answers back.
    while !s.sent.iter().any(|p| p.sub() == 0x40) {
        let n = s.t + 1;
        s.run(n);
    }
    let held = std::mem::take(&mut s.pad.answers);
    assert_eq!(held.len(), 1);
    s.pad.accepts = Box::new(move |p| p.is_way(w) && p.sub() != 0x40);
    // 40 01 is given up after its tries...
    while s.v.startup().step(0x40) != StepState::NoAnswer {
        let n = s.t + 1;
        s.run(n);
    }
    assert!(
        s.has_note("no answer to 0x40/0x01 (motion sensors on) after 3 tries; going on without it")
    );
    // ...then its old answer arrives while 30 01 is out.
    assert!(matches!(s.v.startup().step(0x30), StepState::Sent(_)));
    s.report(&reply(0x40));
    assert!(matches!(
        s.v.startup().step(0x40),
        StepState::Answered(0x80)
    ));
    assert!(s.has_note("late answer to subcommand 0x40 (motion sensors on)"));
    s.run_until_finished(12_000);
    assert_eq!(s.outcome(), "won:info-49,03:ok,40:ok,30:ok");
    let _ = STEP_TRIES;
}

/// A reconnect starts the conversation from the beginning.
#[test]
fn a_reconnect_resets_everything() {
    let mut s = Sim::new(Pad::deaf());
    s.run(2000);
    assert!(s.v.startup().sent() > 0);
    assert!(s.outcome().starts_with("probing:"));
    s.v.disconnected();
    assert_eq!(s.outcome(), "waiting-to-start");
    assert_eq!(s.v.startup().sent(), 0);
    assert_eq!(s.v.take_startup_note(), None, "old notes dropped");
    s.v.device(&Device {
        name: "Pro Controller",
        ..Default::default()
    });
    let mut buf = [0u8; 64];
    s.v.set_time_ms(2100);
    assert_eq!(s.v.next_output(&mut buf), None, "waits for a report again");
    s.v.report(&SIMPLE);
    s.v.set_time_ms(2100 + SETTLE_MS);
    s.v.next_output(&mut buf).unwrap();
    assert_eq!(
        (buf[1], buf[10]),
        (0, 0x02),
        "counter from 0, first variant"
    );
    assert_eq!(
        s.v.startup().outcome().to_string(),
        "probing:info-49(try 1 of 3)"
    );
}

/// Full reports that come unasked (a pad another host set up, or the
/// demo): full mode counts as done, and the probe still looks for a way
/// to send the other steps.
#[test]
fn full_reports_unasked() {
    let mut pad = Pad::deaf();
    pad.full_from_start = true;
    let mut s = Sim::new(pad);
    s.run_until_finished(12_000);
    assert!(s.has_note("full reports (0x30) arrived before we asked for them"));
    assert_eq!(s.v.startup().step(0x03), StepState::ByReports);
    assert!(
        s.outcome().ends_with("mode-49),03:ok-unasked"),
        "{}",
        s.outcome()
    );

    // The same, with a pad that answers info-49: 03 is skipped.
    let mut pad = Pad::speaking("info-49");
    pad.full_from_start = true;
    let mut s = Sim::new(pad);
    s.run_until_finished(12_000);
    assert_eq!(s.outcome(), "won:info-49,03:ok,40:ok,30:ok");
    assert!(!s.sent.iter().any(|p| p.sub() == 0x03), "03 30 not needed");
}

/// The answer to device info carries the pad's address; the firmware
/// must not log it.
#[test]
fn private_bytes_of_device_info() {
    assert_eq!(private_bytes(&reply(0x02)), Some(19..25));
    assert_eq!(private_bytes(&reply(0x03)), None);
    assert_eq!(private_bytes(&SIMPLE), None);
    assert_eq!(private_bytes(&reply(0x02)[..24]), None);
}

/// A HID DATA message on the control channel is looked at like an input
/// report: an answer there also wins.
#[test]
fn an_answer_on_the_control_channel() {
    let mut v = switch_view();
    let mut buf = [0u8; 64];
    v.set_time_ms(0);
    v.report(&SIMPLE);
    v.set_time_ms(SETTLE_MS);
    v.next_output(&mut buf).unwrap();
    let mut msg = vec![0xA1];
    msg.extend(reply(0x02));
    v.control(&msg);
    assert_eq!(v.startup().won().map(|w| w.name), Some("info-49"));
}
