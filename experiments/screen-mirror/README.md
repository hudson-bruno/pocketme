# screen-mirror

Proving firmware for the debug link on the CYD board (ESP32-2432S028).
It draws test pictures whose every pixel is known, and answers
`tools/boardctl` over the USB serial cable: screenshots of the panel's
own memory and of the framebuffer, injected buttons and taps, stats.

The protocol, what a firmware has to provide, and the panel read-back
sequence are described in `crates/debug-link/README.md`.

## Build and flash

```sh
export PATH="$HOME/.cargo/bin:$PATH"; . ~/export-esp.sh
cd experiments/screen-mirror
cargo build --release
espflash flash --chip esp32 --port /dev/ttyUSB0 --non-interactive \
    target/xtensa-esp32-none-elf/release/screen-mirror
```

Where the board is shared, put `flock <lockfile> timeout 150` in front
of the `espflash` line and of every `boardctl` call.

## Try it

```sh
B=../../tools/boardctl/target/release/boardctl
$B ping
$B testcard --kind panel -o expected.png
$B shot --source panel -o panel.png
$B compare expected.png panel.png          # mismatches=0
$B --show-log key select                   # switch to the framebuffer picture
$B shot --source fb -o fb.png
$B shot --source panel -o panel-fb.png
$B compare fb.png panel-fb.png --scale-expected 2
$B --show-log tap 200 100
$B stats
```

## The two pictures

* **card** (after start-up): `test_card::draw_panel_card`, 320x240, drawn
  straight to the panel through `mipidsi`, as the reference firmware
  does. Colour bars with known values, one stripe per colour bit, ramps,
  one-pixel patterns, noise, text, a one-pixel border with a different
  colour in each corner, a letter F and a triangle that show mirroring.
  A tap draws a 7-pixel magenta cross: a real difference from the card,
  to prove that `compare` finds one.
* **fb** (SELECT switches): a 160x120 framebuffer of one byte per pixel,
  shown at twice the size through a 256-colour palette. A tap draws an
  orange cross into it; the bottom line shows the buttons held and the
  last tap.

START draws the current picture afresh.

## Files

| file | what |
|---|---|
| `src/main.rs` | Set-up, the pictures, and the debug-link side: receive loop, requests, sending a screenshot. |
| `src/panel.rs` | Reading the panel back over MISO, and sharing the SPI bus with the drawing driver. |
| `src/stack.rs` | How deep the stack has been (for `stats`). |
