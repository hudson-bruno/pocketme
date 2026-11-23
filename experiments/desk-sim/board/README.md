# board-layer (provisional)

**The owner has not decided the shape of this layer yet.** It is a first
try, made for the desktop simulator. Expect it to change.

## What it is

The board layer is the list of things the console may ask of the
hardware. The console's code talks only to this list (the `Board` trait
in `src/lib.rs`). Underneath, either a fake board on the PC or the real
board answers. So the console's code does not change between the two.

## What it covers today

Only what the first simulator slice needs:

- **Screen**: a 320x240 picture in RGB565 (16-bit colour: 5 bits red,
  6 green, 5 blue). The console draws into it, then calls `present()`
  to show it.
- **Touch**: pressed or not, and where (in screen pixels).
- **Gamepad**: 13 buttons as bits, two sticks, two triggers.
- **Time**: `wait_frame()` waits for the next frame and gives its number
  and the time in microseconds. `now_us()` gives the time too.
- **Log**: one line of text at a time.

## Open points

- One 320x240 RGB565 copy of the screen takes 150 KB. The ESP32 has
  520 KB of RAM in all. The plan on the design page is a 160x120 screen
  with one byte per pixel (19 KB) and a palette. This layer may well
  change to that.
- The button list: the first 12 bits are the debug link's button mask
  (`crates/debug-link`), plus HOME. pad-view has 18 buttons. Not decided.
- Touch is in screen pixels (320x240), like the debug link's Tap. The
  design page's model uses 160x120. Not decided.
- Not here yet: the SD card, Bluetooth, saved settings, sound, GPS,
  battery, backlight.

## Rules it keeps

- No `std` (the PC's standard library) and no dependencies. It builds
  for the ESP32: see `../check-no-std.sh`.
