# Credits

PocketMe stands on other people's work. This page lists what the project
uses, what it learned from, and what is its own.

- [Rust on the ESP32](#rust-on-the-esp32)
- [Other crates](#other-crates)
- [Tools](#tools)
- [References](#references)
- [Hardware](#hardware)
- [What is the project's own](#what-is-the-projects-own)
- [How it was written](#how-it-was-written)
- [Licence](#licence)

## Rust on the ESP32

The firmware would not exist without the [esp-rs](https://github.com/esp-rs)
project, which makes bare-metal Rust on Espressif chips possible:

| crate | used for |
|---|---|
| `esp-hal` | the chip's parts: SPI with DMA, GPIO, timers, the second core |
| `esp-rtos` | the scheduler the radio driver needs, with the Embassy integration |
| `esp-radio` | the driver around Espressif's Bluetooth controller |
| `esp-alloc` | the firmware's small heap |
| `esp-storage` | reading and writing the chip's flash |
| `esp-bootloader-esp-idf` | the application image format the bootloader expects |
| `esp-backtrace`, `esp-println` | panic messages and the serial log |
| `xtensa-lx` | low-level access to the Xtensa core |

`esp-radio` is used as a **patched copy** of version 1.0.0-beta.1, kept in
[experiments/bt-classic-probe/vendor/esp-radio](../experiments/bt-classic-probe/vendor/esp-radio).
The patch starts the Bluetooth controller in dual mode (classic and Low
Energy), which the original does not offer, and can put ESP-IDF's fixed
classic functions in place of the ones in ROM. Every changed line is listed
in [PATCH-NOTES.md](../experiments/bt-classic-probe/vendor/PATCH-NOTES.md).
`esp-radio` is licensed MIT or Apache-2.0 by its authors.

[Embassy](https://embassy.dev) provides the async executor and timers
(`embassy-executor`, `embassy-time`, `embassy-futures`).

The Bluetooth controller itself (the radio's lowest layer) is Espressif's
own binary library, reached through `esp-radio`.

## Other crates

| crate | used in | for |
|---|---|---|
| `embedded-hal`, `embedded-hal-bus` | firmware, with the SD card only | the shared SPI bus traits for the SD card |
| `embedded-sdmmc` | firmware, with the SD card only | reading the FAT file system on the microSD card |
| `embedded-graphics` | test-card, pad-view | drawing the debug link's test pictures and the gamepad test firmware's pages |
| `embedded-graphics-core` | console-video, as an option | letting embedded-graphics draw into the console's picture |
| `critical-section`, `static_cell`, `log` | firmware | the usual embedded Rust building blocks |
| `sdl3` | simulator | the window, keyboard and mouse (the simulator also builds and runs without it) |
| `png` | boardctl, pad-view's tests | writing and reading PNG pictures |
| `clap`, `anyhow`, `serialport` | boardctl | the command line and the serial port |

The earlier trials in [experiments/](../experiments) also used
`mipidsi`, `trouble-host`, `bt-hci`, `heapless`, `enumset`,
`embassy-sync` and `portable-atomic`.

Games use no outside crate at all: a game depends only on the project's
own `game-kit` (and, for 3D, `game-3d`).

## Tools

- The Rust compiler for Xtensa and its installer,
  [espup](https://github.com/esp-rs/espup).
- [espflash](https://github.com/esp-rs/espflash), which writes the
  firmware to the board.
- Cargo, rustfmt and Clippy; Python 3 for the build and check scripts.

## References

- **The Bluetooth Core Specification** and the HID profile
  specification, published by the Bluetooth SIG. The project's classic
  Bluetooth host (`pad-proto`) is written from them.
- **ESP-IDF** (Espressif, Apache-2.0) was read to learn how the
  Bluetooth controller must be started and which of its fixed functions
  replace the ones in ROM, how the chip's second RAM block is seen from
  the instruction bus, and the partition table format. No code was
  copied from GPL sources or from BTstack.
- **The ST7789 panel's datasheet**, for the Get Scanline
  command and the refresh timing the tear-free send is built on.
- **The xpadneo project's documentation** of gamepad report descriptors
  and modes, SDL's HID gamepad drivers, the dekuNukem Nintendo Switch
  reverse-engineering notes, and the **GuliKit NS09 manual**, for what
  the test gamepad sends in each of its modes
  ([crates/pad-view/MODES.md](../crates/pad-view/MODES.md) marks which
  facts were measured and which come from these sources).

## Hardware

- The prototype board is the ESP32-2432S028, known as the "Cheap Yellow
  Display" (CYD): a classic ESP32 with a 320x240 ST7789 panel, an
  XPT2046 resistive touch screen and a microSD slot.
- The gamepad the Bluetooth host was built and tested against is a
  GuliKit KingKong 2 Pro.

## What is the project's own

- The console, the simulator, the game interface and every game are
  original work of this project.
- The games are original designs, not recreations of existing games.
- The 5-pixel font, the boot animation and the tile pictures on the home
  screen were made for the project.
- The classic Bluetooth host, the flash settings store, the picture diff
  and the tear-free panel send were written for the project.

## How it was written

The author designed the console, made every design decision and tried
the builds on the real board. Most of the code and of the documentation
was written with [Claude Code](https://claude.com/claude-code),
Anthropic's coding agent, working under the author's direction.

## Licence

The project's own code and documentation are under the MIT licence: see
[LICENSE](../LICENSE). The patched copy of `esp-radio` keeps its authors'
licence (MIT or Apache-2.0); both texts are in its folder.
