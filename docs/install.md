# Put PocketMe on your own CYD

This page is for people who have a "Cheap Yellow Display" board and want
to play PocketMe on it without building anything. You can flash the
console in one click directly from Chrome or Edge, or flash ready-made
files from the command line. No programming and no developer tools are
needed.

Three words used below: the **firmware** is the program the board runs;
writing it to the board is called **flashing**; the **port** is the
name the computer gives the plugged-in board, such as `COM3` on Windows.

[What you get](#what-you-get) ·
[What you need](#what-you-need) ·
[Before you start](#before-you-start) ·
[The 1-click way](#the-1-click-way-in-the-browser) ·
[The manual browser way](#the-manual-browser-way-esptool-js) ·
[Check the download](#check-the-download-optional) ·
[The first start](#the-first-start) ·
[If it does not work](#if-it-does-not-work) ·
[The command-line way](#the-command-line-way)

> **Tested on the project's CYD board (2026-11-24).** The 1-click
> browser installer (`index.html` + `manifest.json`) and three-part
> firmware files (`0x1000`, `0x8000`, `0x10000`) were used to flash the
> project's `ESP32-2432S028` (2 USB, rev v3.1, ST7789) in Chrome/Edge
> (holding **BOOT** while clicking **CONNECT & FLASH** on Windows CH340).
> The manual hosted `esptool-js` page, the single merged `.bin` at `0x0`,
> the command-line snippets, Windows/macOS checksum commands, and other
> CYD variants remain untested. The firmware inside the release bundle
> includes the `pair` feature (see
> [tools/release/README.md](../tools/release/README.md)).

## What you get

- **The console** with its fourteen programs built in: eleven games, two
  unfinished prototypes (DRIVE3D and PROTO3D) and BENCH, a speed test.
  No microSD card is needed.
- **Touch and the BOOT button** as built-in controls. The board has only
  two buttons, BOOT and RST, and only BOOT can be read by the console.
- **On-console Bluetooth gamepad pairing.** The firmware includes
  classic Bluetooth gamepad support and a **GAMEPADS** tile on the Home
  screen to pair or forget a Player 1 controller directly on the board.
  **Without a paired gamepad, only some games can be played** (ROOFTOP,
  DRIVE3D and PROTO3D need one).
- **A prototype.** It runs and is playable on the board it was built
  for, but it is not a product. What is missing is listed in
  [limitations.md](limitations.md).

What touch alone can do, from each game's page in
[games/](../games/README.md):

- **Played by touch:** CAMBER, HAIRPIN and KESTREL can be played whole
  by touch. In STARDUST and LANDER a finger steers (the comet flies
  towards it; the lander leans towards it and fires). LANTERN and
  MERCATOR have touch steering. In SKIMMER touch plays one side, in
  TUNDRA player 2; the computer plays the rest.
- **Menus only:** ROOFTOP and PROTO3D take touch in their menus only.
- **No touch controls:** DRIVE3D needs a gamepad (its page says touch does
  nothing). PADTEST shows where the finger is, as a red cross. BENCH is
  a speed test and reads no touch.

In any game, BOOT opens the console's pause menu, and a tap on
BACK TO HOME leaves the game. That is the way out of a game you cannot
play by touch.

## What you need

**The board.** The firmware was written for, and tested on, one board:
the ESP32-2432S028, the usual "Cheap Yellow Display", with a classic
ESP32 chip of revision 3.1, 4 MB of flash, a 320x240 screen with an
**ST7789** display chip, a resistive touch screen and a microSD slot
([hardware.md](hardware.md#the-board)). "ESP32-2432S028" is the model
name to start from in a shop listing.

Many boards are sold under the same name with small differences, for
example an **ILI9341** display chip instead of the ST7789, or a
different USB socket (both general facts, not checked here). **None of
these other variants was tried.** You cannot reliably tell the display
chip from a listing or by looking at the board: writing the file is
the test, and it is safe. On a different variant the most likely
result is a wrong or blank picture. Writing the file does not damage
the board, and another firmware can be written to it later (see
[below](#put-another-firmware-back)).

**An ESP32 of revision 3 or later.** The firmware refuses to run on an
older ESP32 (its file says so, and the chip library it is built on
stops at start-up). In `esptool-js`, the page prints the chip revision
when it connects so you find out before anything is written.

**A USB data cable** that fits the board. Many cheap cables only charge
and carry no data; with such a cable the computer does not see the
board at all.

**A computer with Google Chrome or Microsoft Edge** (version 89 or
later, as the flashing tool's makers say; the tool's page says Safari
is not supported). The page talks to the board through Web Serial, a
part of the browser that lets a page use a port. This guide covers a
computer (Windows, macOS or Linux), not a phone.

**On Windows, maybe a driver.** The board talks to the computer through
a small USB-to-serial chip. The project's notes do not say which one
the tested board has; on CYD boards it is usually a **CH340** (a
general fact, not checked on the tested board). Windows 10 and 11
often install its driver by themselves (general knowledge, not
checked). To see whether a port appeared: right-click Start, choose
**Device Manager**, and open **Ports (COM & LPT)**; plug the board in
and watch for a new line. If none appears, install the CH340 driver
from its maker, WCH (their site is wch-ic.com, and the Windows driver
is called CH341SER; not checked by the project).

## Before you start

> **Writing PocketMe replaces the program the board came with.** That
> program cannot be brought back unless its maker offers it for
> download.

> **Keeping saved gamepad pairings and settings.** The **1-click web
> installer** writes the three firmware parts (`0x1000`, `0x8000`,
> `0x10000`) and leaves `0x9000..0x10000` (saved Bluetooth gamepad
> pairing) and `0x3FE000..0x400000` (brightness and boot animation)
> untouched as long as **KEEP DATA** (**MANTER DADOS** in Portuguese)
> stays selected. Only select **ERASE ALL** / **APAGAR TUDO** (or write
> the single merged `.bin` at `0x0`) if you want a clean wipe that forgets
> any saved pairing. You can always re-pair a gamepad on the console
> afterwards.

**If you use the 1-click web installer below**, you do not need to
download any `.bin` files manually.

**If you prefer the manual browser tool (`esptool-js`) or the command
line**, open the newest release on the Releases page of this
repository, click **Assets**, and download either:

- the three parts (`*-bootloader-0x1000.bin`, `*-partitions-0x8000.bin`,
  `*-app-0x10000.bin`) to keep a saved gamepad pairing, or
- the one merged file named like `pocketme-cyd-1.0.bin` (without
  `bootloader`, `partitions` or `app` in its name, about 2 MB) for a
  single-file flash at `0x0`.

Do not download "Source code (zip)" or "Source code (tar.gz)": those
are the program's text, not something you can write to the board.

## The 1-click way: in the browser

This way uses PocketMe's built-in **1-click installer** powered by
`esptool-js` over Web Serial directly in desktop Chrome or Edge (the
page automatically opens in Brazilian Portuguese or English matching
your browser language, and can be switched with `EN | PT-BR` in the top
bar). Once a public release is published with GitHub Pages enabled, the
hosted installer lives at <https://hudson-bruno.github.io/pocketme/> (you
can also run a local copy built with `make_release.py --web-installer`
over `http://localhost:8765`, see
[tools/release/README.md](../tools/release/README.md)).

1. **Plug the board into the computer** with the USB data cable.
   *You see:* the board gets power; the screen may show what it ran
   before.
2. **Open the web installer page** in Google Chrome or Microsoft Edge
   (over `https://` or `http://localhost`). *You see:* the live
   Home screen preview, the firmware version badge, and the
   **FLASH POCKETME TO CYD** button.
3. **Click FLASH POCKETME TO CYD** to open the installer modal.
   Leave **BOARD PRESET** on **ESP32-2432S028 (2 USB)** (or switch to
   **CUSTOM** to adjust baud rate, reset mode or debug logging) and
   leave **ERASE FLASH** on **KEEP DATA** (**MANTER DADOS**) to keep
   any saved gamepad pairing and console settings, or choose
   **ERASE ALL** for a full wipe.
4. **Hold BOOT (recommended on CH340 CYDs) and click CONNECT & FLASH.**
   If your board does not enter download mode automatically over DTR/RTS,
   hold the physical **BOOT** button while clicking **CONNECT & FLASH**
   and picking the port, then release **BOOT** once writing begins.
5. **Choose the board's port** in the browser picker and confirm. It is
   the one that appears when you plug the board in; on Windows its name
   often contains "USB" or "CH340", with a COM number. (If the list is
   empty, see [No port in the list](#no-port-in-the-list).)
   *You see:* chip detection (requiring ESP32 rev v3.0+), a live
   progress bar, three part pills (`BOOTLOADER`, `PARTITIONS`, `APP`),
   and the console log. Do not unplug the board.
6. **Watch the board.** When installation finishes, the board hard-resets
   automatically (press **RST** if the screen does not restart on its
   own). *You see:* the PocketMe boot animation, then the home screen.

## The manual browser way: esptool-js

If the 1-click page is not available or you want to flash a downloaded
file manually, you can use **esptool-js**, Espressif's browser flashing
page. Writing the single merged file at `0x0` overwrites `0x9000..0x10000`
and forgets any saved gamepad pairing (you can pair again on the
console afterwards).

1. **Have the file ready**: `pocketme-cyd-1.0.bin` (with the release's
   number, *without* `bootloader`, `partitions` or `app` in its name)
   in your Downloads folder.
2. **Plug the board into the computer** with the USB data cable.
3. **Open the flashing page** <https://espressif.github.io/esptool-js/>
   in Chrome or Edge. *You see:* a page titled "ESP Tool" with a
   **Connect** button.
4. **Click Connect** and pick the board's port. *You see:* "Connected
   to device:" followed by ESP32 and its chip revision in brackets (for
   example `(revision 3)`), and a table with "Flash Address" and "File".
   **If the revision is below 3, stop here**: this firmware cannot run
   on that board.
5. **Set the address to `0x0`.** Replace the default `0x1000` in the
   Flash Address box with `0x0`. This step matters most; with `0x1000`
   the board will not start.
6. **Choose `pocketme-cyd-1.0.bin`** in the same row, leave Flash Mode,
   Flash Frequency, Flash Size and "Erase all" as they are, and click
   **Program**. Do not unplug the board until the progress bar is gone.
7. **Watch the board.** If the screen does not change within a few
   seconds after completion, press **RST**.

What `esptool-js` shows in steps 4, 6 and 7, and that Flash Mode /
Frequency / Size only rewrite headers for a file at `0x1000`, was read
from its repository source, not tried with a board.

## Check the download (optional)

If you downloaded release files manually, `SHA256SUMS.txt` on the same
release page gives each file's SHA-256 checksum. Open a terminal in your
Downloads folder and check your file against `SHA256SUMS.txt`:

- **Windows**: `(Get-FileHash .\pocketme-cyd-1.0.bin).Hash`
- **macOS**: `shasum -a 256 pocketme-cyd-1.0.bin`
- **Linux**: `sha256sum pocketme-cyd-1.0.bin`

Capital and small letters count as the same. If the numbers differ,
download the file again. The Windows and macOS lines were not tried for
this page.

## The first start

A second or so after RST the screen shows the boot animation: points
gather into the name POCKETME, which then shrinks into the title of the
home screen. It takes about 2.7 s; a tap or BOOT skips it.

The **home screen** shows **GAMEPADS** as the first tile, the fourteen
programs next, and **SETTINGS** as the last tile. The chosen tile has a
gold frame, and its name shows below it. The bar at the bottom shows the
time since the start, a gamepad picture (green when a pad is connected),
the battery with a "?" (not measured) and a gear at the right end.

| To | Do this |
|---|---|
| choose a tile | tap it, or drag the row sideways to scroll |
| start a game or open GAMEPADS / SETTINGS | tap the chosen tile again, or press BOOT |
| pair a Bluetooth gamepad | open **GAMEPADS**, put your classic Bluetooth pad into pairing mode, tap **+ PAIR A NEW PAD**, then pick it when found |
| forget a saved gamepad | open **GAMEPADS**, tap **FORGET >** on the saved pad and confirm **FORGET** |
| pause a game | press BOOT: the pause menu opens with RESUME chosen |
| go on playing | press BOOT again, or tap RESUME |
| go back home | in the pause menu, tap BACK TO HOME |
| open the quick menu | tap the gear on the home screen (has BRIGHTNESS, GAMEPADS, SETTINGS, CLOSE) |
| change brightness or boot animation | open **SETTINGS** (or change BRIGHTNESS in the quick menu), adjust BRIGHTNESS or BOOT ANIMATION (FULL, SHORT, OFF), then tap BACK |

A tap counts when the finger lifts, if it moved less than a few pixels
and was held under half a second. Inside a game, the console's pause
menu opens only with BOOT (some games also have a pause box of their
own on the screen). Saved gamepad pairings (`0x9000..0xB000`) and the
brightness and BOOT ANIMATION choices (`0x3FE000..0x400000`) are kept
when the board is switched off. On a clean board, the console starts
with no saved pad, full brightness, and FULL animation.

## If it does not work

### No port in the list

- Try another cable: a charge-only cable shows no port.
- Try another USB socket on the computer, without a hub.
- On Windows, look in Device Manager and install the CH340 driver if
  needed (see [What you need](#what-you-need)), then plug the board in
  again.
- On Linux, the browser may open the port only if your user may use
  it. On many systems that means joining the group that owns
  `/dev/ttyUSB0`, often `dialout` (`sudo usermod -aG dialout $USER`),
  then logging out and in again. On some systems a braille-display
  program, `brltty`, takes CH340 ports for itself; removing it helps.
  (General advice, not tried for this page.)
- Close any other program that may hold the port open (another
  flashing tool, a serial monitor, the Arduino IDE).

### "Failed to connect" or the page waits for ever

The chip must be in its download mode to accept a new firmware. The
page usually puts it there by itself. If not:

1. Hold the **BOOT** button down.
2. While holding it, press and release **RST**.
3. Let go of BOOT, then click **CONNECT & FLASH** (or **Connect** in manual `esptool-js`) again.

If that fails too, keep BOOT held down while starting the connection
and choosing the port, and let go once writing begins (or once
`esptool-js` says "Connected to device"). In `esptool-js`, a slower
speed is also worth a try: reload the page, choose 115200 in the
Baudrate list next to Connect, then click Connect (the list is hidden
while the page is connected).

### If the write stopped or showed an error

The board is not broken: the part of the chip that accepts a new
firmware cannot be overwritten (a general fact about the ESP32, not
tried here). Reload the page and start the browser flash steps again. If
it does not connect, use the BOOT and RST steps above.

### The screen stays dark, or shows a white, blank, mirrored or wrong-coloured picture

Check these first, in this order:

1. **In `esptool-js`, the address and the file.** Did the Flash Address
   box say `0x0`, and was the file the one *without* `bootloader`,
   `partitions` or `app` in its name? If not, flash again with the right
   ones. A wrong address does no harm and is put right by writing
   again.
2. **The chip's revision.** An ESP32 older than revision v3.0 stops at
   start-up and leaves the screen dark (`esptool-js` prints the chip
   revision on connect).
3. **Another variant.** If both were right, the write most likely
   worked and the board is another variant of the CYD (for example
   with the ILI9341 display chip). This firmware drives only the ST7789
   of the tested board, and has no setting to change that. Put another
   firmware back (below).

### Touch hits the wrong place

The touch calibration is a first guess from one board
([limitations.md](limitations.md#hardware)); on another it may be off.
There is no setting to correct it in this version. BOOT still works for
starting and pausing a game.

### Put another firmware back

This firmware locks nothing. Any other firmware for the ESP32 can be
written to the board again the same way, with the address its maker
gives (for a single merged file, usually 0x0). If the board does not
answer, use the BOOT and RST steps above.

## The command-line way

For people who already have a flashing tool installed. Give the
board's port with `--port` (for example `COM3` on Windows,
`/dev/ttyUSB0` on Linux), and the release's number in the file names.

**esptool** (Espressif's own; it needs Python, and is installed with
`pip install esptool`):

```sh
# the one merged file at 0x0 (forgets a saved gamepad pairing)
esptool --port COM3 write-flash 0x0 pocketme-cyd-1.0.bin

# or the three parts: they leave a saved pairing alone
esptool --port COM3 write-flash \
    0x1000  pocketme-cyd-1.0-bootloader-0x1000.bin \
    0x8000  pocketme-cyd-1.0-partitions-0x8000.bin \
    0x10000 pocketme-cyd-1.0-app-0x10000.bin
```

**espflash** (`write-bin` writes one file per call):

```sh
# the one merged file at 0x0 (forgets a saved gamepad pairing)
espflash write-bin --port COM3 0x0 pocketme-cyd-1.0.bin

# or the three parts, one call each: they leave a saved pairing alone
espflash write-bin --port COM3 0x1000  pocketme-cyd-1.0-bootloader-0x1000.bin
espflash write-bin --port COM3 0x8000  pocketme-cyd-1.0-partitions-0x8000.bin
espflash write-bin --port COM3 0x10000 pocketme-cyd-1.0-app-0x10000.bin
```

Each part's address is in its file name. The three parts leave 0x9000
to 0x10000, where a pairing is kept, alone, as long as nothing erases
the whole chip: on a board with a paired pad, never add an erase step.
These commands were checked against the tools' help and documentation,
not run on a board.

## For builders

To build it yourself, with or without the gamepad, see
[Run it on the board](../README.md#run-it-on-the-board).

## Where this comes from

- In this repository: [hardware.md](hardware.md),
  [boards/cyd/README.md](../boards/cyd/README.md),
  [boards/cyd/partitions.csv](../boards/cyd/partitions.csv),
  [crates/console-core/README.md](../crates/console-core/README.md),
  [crates/settings-store/README.md](../crates/settings-store/README.md),
  [games/README.md](../games/README.md) and each game's page (BENCH:
  its source, which reads no touch),
  [tools/release/README.md](../tools/release/README.md) (how the files
  are made and checked, and the lowest chip revision in the firmware's
  file: v3.0).
- esptool-js: <https://espressif.github.io/esptool-js/>, its README and
  the page's source in its repository (the controls, the default
  address, the progress bar, the error line, the chip name with its
  revision); the esptool documentation at docs.espressif.com
  (installation, `write-flash`, flash modes, boot mode selection);
  `espflash write-bin --help` of espflash 4.7.0. All read on
  2026-11-23.
