# pad-view

Understands what a Bluetooth gamepad sends, and draws it on the 320x240
screen. It is the screen half of the gamepad test firmware
(`experiments/bt-gamepad-test`). The Bluetooth half is `crates/pad-proto`.

It is a `no_std` library: no heap, fixed-size tables, nothing that can
panic on bad input. It draws on anything `embedded-graphics` can draw on
(`DrawTarget<Color = Rgb565>`), so the same code runs on the board and in
the tests on the PC.

## What is inside

| File | What it does |
|---|---|
| `src/descriptor.rs` | Reads a HID report descriptor (the gamepad's own description of its reports) into a table of fields: report id, bit position, size, range, usage. |
| `src/decode.rs` | Turns one report into a "standard gamepad" (face buttons, bumpers, triggers, d-pad, two sticks). Known pads (Switch Pro, Xbox on Bluetooth) are decoded by hand; others from the descriptor. |
| `src/motion.rs` | The motion sensor of a Switch-mode pad: the three samples in report 0x30, in g and degrees per second, the tilt, and the limit on how often the screen shows a change. |
| `src/startup.rs` | The packets a Switch-mode pad needs before it sends full reports: a prober that finds the way of talking the pad answers, then the steps. |
| `src/draw.rs` | The three pages. Draws only what changed. |
| `src/text.rs` | Fixed-size text lines, hex dumps, names of usages and device classes. |
| `MODES.md` | What the GuliKit KingKong 2 Pro sends in each of its four modes, with sources. |

## The screen

* Before a gamepad is found: the status in big letters and how to put
  the pad in pairing mode.
* Page 1, "Pad": a picture of a gamepad. Pressed buttons turn yellow, the
  stick dots move (and turn red when the stick is clicked), the trigger
  bars fill. Under it: both sticks (normalised and raw), triggers,
  battery, reports per second, total reports, the buttons held by name,
  and the last report in hex.
  Switch mode only: a small level in the lower left grip (a ball rolls to
  the lower side of the pad; it turns green when the pad lies flat, red
  when it lies face down, grey with no live data) and the gyroscope's
  x, y, z in degrees per second in the lower right grip. With no motion
  data, the right grip says "No motion data yet". Other modes have no
  motion here, so their picture is unchanged.
* Page 2, "Data": every field of the report, by name, with its value;
  and the last bytes of each report id. Switch mode: a motion block on
  the right with the six numbers of the newest sample (accelerometer in
  g, gyroscope in degrees per second, each with its raw count) and the
  tilt (roll and pitch, from gravity alone).
* Page 3, "About": name, class of device, vendor and product, which
  decoding is used and why, what the descriptor declares, the start-up
  conversation and whether the pad answered, and a "Motion sensor:" line:
  "yes (accelerometer + gyroscope, Switch report 0x30)" with whether the
  pad answered `40 01`, "asked, no data yet", "not in this mode" (Xbox
  layout), "none found" (a pad decoded from its descriptor), or, for
  a Sony pad or a descriptor with HID sensor usages, that it has one but
  pad-view does not read it.

The BOOT button (in the firmware) moves to the next page. When the link
drops, the values stay on screen in grey.

## How the firmware should use it

```rust
use static_cell::ConstStaticCell;
static VIEW: ConstStaticCell<pad_view::View> = ConstStaticCell::new(pad_view::View::new());

let view: &'static mut pad_view::View = VIEW.take();
let mut out = [0u8; 64];          // 49 bytes are enough today

view.status("Searching");
// ... a gamepad answered:
view.device(&pad_view::Device { name, class_of_device, ..Default::default() });
// ... SDP gave the Device ID record and the HID descriptor:
view.device(&pad_view::Device { name, class_of_device, vendor_id, product_id, version, vendor_id_source });
view.descriptor(&hid_descriptor_bytes);
// ... for every input report on the interrupt channel (0xA1 removed):
view.set_time_ms(now_ms);
view.report(bytes);
while let Some(o) = view.next_output(&mut out) {
    match o.channel {
        // send 0xA2 followed by out[..o.len] on the interrupt channel
        pad_view::startup::Channel::Interrupt => {}
        // send 0x52 (SET_REPORT Output) followed by out[..o.len] on the
        // control channel
        pad_view::startup::Channel::Control => {}
    }
    // if it could not be sent after all: view.output_not_sent();
}
// ... for every message on the HID control channel (header byte kept):
view.control(&msg);
// ... the start-up conversation's notes, for the log:
while let Some(note) = view.take_startup_note() {
    // log "switch start-up: {note}"
}
// ... about 30 times a second (and right after a BOOT press):
view.set_time_ms(now_ms);
view.draw(&mut display)?;
```

* `View` is about 7 KB (7120 bytes on the PC). Keep it in a `static` (a `ConstStaticCell`
  builds it in place, with no copy on the stack). `View::new()` is a
  `const fn`.
* `draw()` itself uses about 5 KB of stack while it runs (the slot memory
  and the text of the page being drawn; the motion block adds a little,
  not measured again). Give the task that calls it at least 8 KB.
* Call `draw()` on a timer, 20 to 30 times a second, not after every
  report: it draws only what changed, so a call with no change sends
  nothing to the screen, and a stick move costs about 1100 pixels.
  Motion is redrawn at most 10 times a second on the Pad page and 4
  times on the Data page, and only when a number moved past a small step
  (0.02 g, or about 2 degrees a second), so a pad lying still sends
  nothing for it. One motion change costs about 700 pixels in 18 windows
  on the Pad page (about 1.2 ms on the board), 4000 pixels in 100
  windows on the Data page (about 7 ms). The raw bytes in hex (Pad and
  Data pages) are drawn again at most twice a second: with the motion
  sensor on, almost every byte from 13 on changes in every report.
  The first call, a page change, a new layout, the first report and a
  disconnection redraw the whole screen (about 140,000 pixels).
* Call `next_output()` after `device()`, after `descriptor()`, after every
  `report()` and also on the draw timer: a Switch-mode pad may send
  nothing until asked, and a lost packet is sent again only when
  `next_output()` is called. Give it a buffer of 49 bytes or more (a
  shorter one gets nothing). Each packet names its channel (interrupt
  or control). A packet that could not be sent is handed back with
  `output_not_sent()`: it is not counted as a try, and its "sent" note is
  taken back if not yet read.
* `view.startup().outcome()` is the one-line result for a log, for
  example `won:info-49,03:ok,40:ok,30:ok` or
  `all-variants-failed(info-49,...)`; `take_startup_note()` gives one
  note per send, answer, win or failure. `startup::private_bytes()` names
  the bytes of a report that hold the pad's address (the answer to device
  info): a firmware must not log them.
* `set_time_ms()` is optional but makes the report rate show, lets
  start-up packets be retried after 300 ms (without it each report
  counts as 10 ms), and sets the pace of
  motion changes and of the hex lines (without it only the small steps
  apply, and the hex lines are drawn on every call).
* `disconnected()` when the link goes. If a different gamepad may come
  next, call `reset()` too.

## Tests

```sh
cd crates/pad-view && cargo test
```

They run on the PC:

* `tests/descriptors.rs`: the parser against real published descriptors
  (Switch Pro, Xbox One S, DualShock 4, two Android pads, and the GuliKit
  in Android mode), stored in `tests/data/mod.rs` with their sources.
* `tests/decode.rs`: reports with known answers for each layout.
* `tests/motion.rs`: hand-made reports 0x30 with three different samples;
  units (4096 = 1 g, 1000 counts = 70 degrees a second); the tilt flat,
  on each side, nose up and down, face down; all zeros meaning "no motion
  data"; motion kept for the Switch layout only; the redraw limit.
* `tests/motion_check.rs`: a second, independent check: a report typed
  out byte by byte from the published layout, the start-up packets byte
  by byte, extreme and cut reports through drawing, the motion parts
  drawing only inside their boxes, no leftovers, and the redraw limits.
* `tests/startup.rs`: the Switch start-up prober against fake pads: one
  per variant that answers only that way (each is found and the
  conversation finishes in under 12 s), one that never answers, slow and
  late answers, an answer for an older try, a reconnect, packets handed
  back, the counter and the exact log lines.
* `tests/fuzz.rs`: random and broken descriptors and reports, all the way
  through drawing; must never panic or hang.
* `tests/render.rs`: draws every page into a framebuffer, saves PNG files
  in `.local/pad-view-shots/` (twice the real size), checks a pressed
  button is yellow, that a second `draw()` sends nothing, that a stick
  move redraws under 3% of the screen, and that drawing only the changes
  gives exactly the same picture as drawing everything. The motion
  pictures are the files with "motion" in their name (11 to 18). It also
  measures what one motion change costs on screen, checks that sensor
  noise of a still pad redraws nothing, that a moving pad is redrawn 15
  to 21 times in 2 s, that the other modes keep a plain grip, and what a
  whole page costs per second with the motion sensor on (under 5% of
  the board's time).

Build for the board (checks it compiles for the ESP32):

```sh
. ~/export-esp.sh
cargo +esp build --release --target xtensa-esp32-none-elf -Zbuild-std=core
```
