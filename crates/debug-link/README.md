# debug-link

The console's debug link: a way to find out what the board's screen
shows, and to press its buttons, over the USB serial cable alone.

Three parts belong together:

| where | what |
|---|---|
| `crates/debug-link` (this crate) | The wire protocol. `no_std`, no heap, no I/O; also builds and tests on the PC. |
| `tools/boardctl` | The PC program: `shot`, `key`, `tap`, `stats`, `log`, `raw`, `compare`, ... |
| `experiments/screen-mirror` | A firmware that proves it on the CYD board, and the working example of everything below. |

`crates/test-card` holds the test pictures, so the firmware and the PC
draw the same pixels and a screenshot can be checked by program.

```sh
cd crates/debug-link && cargo test     # 27 tests, on the PC, no board
```

## The wire

Requests and answers travel as frames on UART0, the same wire as the log
text, at 115200 baud (`debug_link::BAUD`):

```text
FE DB | kind | seq | len lo, len hi | hcrc | payload (0..=1024) | crc lo, crc hi
```

`hcrc` is a CRC-8 of the four bytes before it; `crc` is CRC-16/CCITT-FALSE
of everything from `kind` to the end of the payload. Byte `FE` appears in
neither log text (UTF-8) nor GPS sentences (ASCII), and when any check
fails the decoder gives up one byte and looks again. So log lines, GPS
sentences and noise between frames do no harm, in either direction.

| request (PC to board) | answer |
|---|---|
| `Ping` | `Ack` |
| `Stats` | `StatsText`: lines of `key=value` |
| `Buttons { mask }` | `Ack`. The mask is every button held from now on; to press and release, send the mask and then 0. Bits are in `msg::button`. |
| `Tap { x, y }` | `Ack`. Panel pixels, 0,0 top left. |
| `Shot { source, baud, read_khz }` | `Ack`, then `ShotBegin`, `ShotPalette` (indexed pictures only), `ShotData`..., `ShotEnd` |
| `PanelRaw { cmd, dummy_bits, read_len, clock_khz, params }` | `PanelRawReply`: the bytes the panel answered |
| `PlayerButtons { player, mask }` (kind `0x09`; its own type, not a `Request`) | `Ack`. Like `Buttons`, for one player's pad: `player` 0 to 3 (0 is the pad `Buttons` sets; 1 to 3 are the game's players 2 to 4). A player past 3 gets `BAD_REQUEST`; a firmware from before four players answers `UNSUPPORTED`. |

An answer repeats the request's `seq`. A request the firmware does not
know gets `Ack` with status `UNSUPPORTED`.

**A screenshot at a faster speed.** With `baud` other than 0 the board
acknowledges at 115200, sends out what is still waiting, changes its
UART to `baud`, and waits up to 1.5 s for `Go` (same `seq`) at that
speed. Then it sends the picture, changes back to 115200, and only then
logs again. If no `Go` comes it changes back and sends nothing.

**The picture** is sent top to bottom as run-length packed pixels
(`rle`), each `ShotData` frame saying which pixel it starts at, so a lost
frame is noticed. `ShotEnd` carries a CRC-32 of all the unpacked pixels.
There is no re-sending of single frames: when a picture arrives damaged,
`boardctl` asks for the whole picture again.

## What a firmware does

It owns UART0 (RX GPIO3, TX GPIO1) and gives the crate bytes; the crate
never touches hardware. The whole pattern is in
`experiments/screen-mirror/src/main.rs` (`App::run`, `App::handle`,
`App::shot`). In short:

```rust
use debug_link::{
    frame::{COMMAND_DECODER_BUF, Decoder, Event, MAX_FRAME},
    image::ImageSender,
    msg::{PixelFormat, Reply, Request, Source, kind, status},
};

// 1. Feed every received byte to a decoder. 96 bytes hold any request.
let mut decoder = Decoder::<COMMAND_DECODER_BUF>::new();
decoder.push(byte, &mut |event| match event {
    Event::Frame(frame) => { /* copy kind, seq, payload out; handle after */ }
    Event::Text(_) => {}      // not ours: log text, GPS. Do not print it.
    Event::Corrupt => {}      // a frame that failed its check
});
// After 100 ms without a byte, call decoder.idle(&mut sink): it gives up
// a frame that was cut short.

// 2. Handle a frame.
match Request::parse(&frame) {
    Ok(Request::Ping) => send(Reply::Ack { request: kind::PING, status: status::OK }),
    Ok(Request::Buttons { mask }) => { /* ack, then hand `mask` to the input code */ }
    Ok(Request::Tap { x, y }) => { /* ack, then hand it to the input code */ }
    Ok(Request::Stats) => { /* write key=value lines with debug_link::TextBuf, send Reply::StatsText */ }
    Ok(Request::Shot { source, baud, read_khz }) => { /* below */ }
    Ok(_) | Err(_) => send(Reply::Ack { request: frame.kind, status: status::UNSUPPORTED }),
}

// 3. To send any reply: encode into a buffer, write the bytes to the UART.
let len = reply.encode(seq, &mut frame_buf)?;   // frame_buf: [u8; MAX_FRAME]
uart_write_all(&frame_buf[..len]);
```

For a screenshot, after the `Ack` (and the change of speed, if asked):

```rust
let mut sender = ImageSender::new(seq, Source::Panel, PixelFormat::Rgb565, 320, 240);
// row_buf: sender.row_bytes() long (640 for the panel, 160 for the framebuffer)
while let Some(len) = sender.next_frame(&mut row_buf, Some(&palette), fetch_row, &mut frame_buf)? {
    uart_write_all(&frame_buf[..len]);
}
```

### What the firmware must provide

1. **A blocking "write all these bytes to UART0"**, used for every frame.
   Nothing else may write to UART0 while a frame is going out, or a log
   line lands in the middle of it. (In `screen-mirror` everything runs in
   one task, and a frame is written without an `await`.) While the line
   is at another speed for a screenshot, do not log at all.
2. **`fetch_row(y: u16, row: &mut [u8]) -> Result<(), E>`**: fill `row`
   with row `y` of the picture, left to right. Called once per row, top
   to bottom.
   * Framebuffer: copy 160 bytes (one palette index per pixel), and pass
     the 256 RGB565 colours as `palette`.
   * Panel: read the row back from the display controller and write
     RGB565, **low byte first** (2 bytes per pixel). See the next
     section.
3. **A way to change the UART's speed** (`uart.apply_config(...)` in
   esp-hal) if faster screenshots are wanted. Without it, answer only
   `baud == 0`.
4. Somewhere to keep `frame_buf` (1033 bytes) and `row_buf` (640 bytes).
   Statics, not the stack.
5. For `Stats`: whatever numbers are worth having, as `key=value` lines.
   `screen-mirror` reports `heap_free`, `stack_used`, `frame_us`,
   `uptime_ms` and counters of what it received.

## Reading the panel back (what worked on the CYD board)

The display controller keeps every pixel it was sent, and that memory is
what the glass shows. Reading it is the proof that a picture arrived, and
it is the function a new display driver must provide to be checked by
this link. Code: `experiments/screen-mirror/src/panel.rs`
(`PanelReader::read_row`, `PanelReader::raw`).

Board facts: panel on SPI2, SCK GPIO14, MOSI GPIO13, **MISO GPIO12**,
CS GPIO15, DC GPIO2. The panel answers ID `81 81 B3`.

For one row `y`, columns `x0..=x1`, with the SPI clock **at 4 MHz**
(SPI mode 0, as for drawing):

| step | CS | DC | bytes |
|---|---|---|---|
| 1 | low | low | write `2A` (CASET) |
|   |     | high | write `x0 hi, x0 lo, x1 hi, x1 lo` |
|   | high | | |
| 2 | low | low | write `2B` (RASET) |
|   |     | high | write `y hi, y lo, y hi, y lo` |
|   | high | | |
| 3 | low | low | write `2E` (RAMRD) |
|   |     | high | read `1 + 3 * width` bytes, **CS stays low the whole time** |
|   | high | | |

* The first byte read means nothing (8 dummy clock pulses). Drop it.
* Then come **three bytes per pixel: red, green, blue**, each in the top
  6 bits of its byte, although the pixels were written as 16-bit
  (COLMOD stays `05`; it does not have to be changed for reading).
* RGB565 is `((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)`. Read back
  this way, every pixel of the test card came out exactly as written.
* x and y are the same turned coordinates used for drawing (MADCTL
  `60`, 320 wide, 240 high).
* A read longer than the window wraps round to the window's first pixel.
* Registers read the same way, but with **1 dummy bit**, not 8:
  `04` (RDDID) gives `81 81 B3`, `09` (RDDST) gives `B0 53 04 00`. The
  one-byte registers `0A`, `0B`, `0C` need no dummy bit.

**The clock.** With esp-hal the SPI clock is changed between drawing and
reading with `Spi::apply_config(&Config::default().with_frequency(...))`,
and changed back afterwards. Measured over the whole screen:

| read clock asked for | wrong pixels of 76,800 | reading the screen |
|---|---|---|
| 1 MHz | 0 | 1992 ms |
| 2 MHz | 0 | 1043 ms |
| **4 MHz** (the default) | 0 | 563 ms |
| 6 MHz | 0 | 426 ms |
| 8 MHz | 119 to 136, different pixels on every read, red only | 325 ms |
| 10 MHz and more | thousands | |

So read at 4 MHz. Drawing may be as fast as the driver likes: the read
is a separate, slow transfer afterwards. A driver that sends pixels by
DMA must be idle, with CS high, before the read starts.

**Telling a read error from a wrong pixel:** take the screenshot twice.
A wrong pixel in the panel's memory is in the same place with the same
value both times (`boardctl compare a.png b.png` says 0 differences
between the two shots, and both differ from the expected picture). A
read error moves.

## boardctl

```sh
cd tools/boardctl && cargo build --release      # target/release/boardctl
```

`boardctl` does not take the board lock by itself. Where the board is
shared, wrap every call: `flock <lockfile> timeout 150 boardctl ...`.
It does not reset the board when it opens the port (it leaves the DTR
and RTS lines alone); only `reset` and `log --reset` do.

```sh
boardctl ping                                   # is a debug-link firmware there?
boardctl shot --source panel -o panel.png       # the panel's memory, 320x240
boardctl shot --source fb -o fb.png             # the framebuffer, 160x120, palette kept
boardctl shot --source panel -o p.png --baud 115200 --read-khz 2000 --tries 1
boardctl key a up                               # hold A and UP for 100 ms, then release
boardctl key select --hold-ms 500
boardctl key a --keep ; boardctl release        # leave down; let go of everything
boardctl key a --player 2                       # the same on player 2's pad (PlayerButtons)
boardctl release --player 2
boardctl tap 100 200
boardctl text f                                 # plain letters, as typed in a terminal (f: forget all pads)
boardctl stats                                  # key=value lines
boardctl log --seconds 5                        # the plain log; frames and GPS sentences left out
boardctl log --reset --seconds 3                # restart the board and show its start-up
boardctl reset
boardctl raw 04 --dummy-bits 1 --read 3         # one panel command, answer in hex
boardctl raw 2a --params 00 00 00 07 --read 0
boardctl testcard --kind panel -o expected.png  # the test picture as it should be (no board)
boardctl compare expected.png panel.png --diff diff.png
boardctl compare fb.png panel.png --scale-expected 2
```

`--show-log` (before the subcommand) also prints the board's log lines
seen during the command and for 300 ms after, marked `[board]`, on
stderr.

Exit codes: 0 done (for `compare`: the pictures are the same), 1
`compare` found differences, 2 anything that went wrong.

`compare` works on the 16-bit colours the panel holds and prints the
number of differing pixels, the rectangle they lie in, how often each
colour bit was wrong, and the first ones with both values.

**Speed.** `shot` asks for 460800 baud by default. It does not fall
back to a slower speed by itself: when a picture arrives damaged it asks
for it again at the same speed (`--tries`, default 3). Measured on this board through its CH340 and WSL2's USB forwarding:

| baud | panel screenshot | framebuffer screenshot | single attempts that arrived whole |
|---|---|---|---|
| 115200 | 2.7 s | | 6 of 6 |
| 230400 | 1.7 s | 0.31 s | 38 of 42 |
| 460800 | 1.2 s | 0.19 s | panel 79 of 80, framebuffer 40 of 40 |
| 500000 | 1.15 s | | 38 of 40 |
| 576000 and more | | | none: bytes go missing in every frame |

A damaged picture was always caught by the checksums; no screenshot that
was accepted ever had a wrong pixel.
