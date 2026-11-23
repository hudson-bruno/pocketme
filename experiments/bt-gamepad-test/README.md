# bt-gamepad-test

A test firmware for the CYD board (classic ESP32). It finds a classic
Bluetooth gamepad, pairs with it, reads what the gamepad says about
itself, logs every input report on the serial port, and draws the
gamepad on the screen: a picture of the pad with every button and stick,
a page of raw numbers, and a page about the device.

The Bluetooth "host" (the part that speaks the protocols above the radio)
is our own code, in `crates/pad-proto`. The pictures and the report
decoding are in `crates/pad-view`. This folder joins the two: it moves
bytes between the host, the radio and the serial port, and sends what
pad-view draws to the screen.

## What it does

- Starts the radio in dual mode (classic Bluetooth and Low Energy). This
  needs the patched radio driver of `experiments/bt-classic-probe`, used
  from there (see `[patch.crates-io]` in `Cargo.toml`).
- Searches in rounds: an inquiry (a classic Bluetooth search) of 3.84 s,
  then a pause of 1.5 s, again and again.
- Calls a device only when its class says "peripheral, gamepad or
  joystick". Everything else heard in a search is only counted.
- Optional allow-list, set when building (off by default): with
  `PAD_ONLY="GuliKit,Pro Controller"` it calls only gamepads whose name
  holds one of those pieces (or whose address is in the list). Other
  gamepads are logged by class only (`found_skipped=` in the stats
  line). It is off by default because in Switch mode the pad has a
  different name and a different address.
- Accepts a call only from a peripheral, or from a device that paired
  with us before (a saved pairing). Other calls are refused.
- Pairs with Secure Simple Pairing, "just works" (no PIN, no number to
  compare). The link key (the shared secret made by pairing) is saved
  in flash, so after a restart or a new flash the pad connects again
  without pairing mode. The key is never printed. See "Saved pairings"
  below.
- Reads two SDP records (SDP is the service list a device offers): the
  HID record, which holds the report descriptor (the map of what each
  byte of a report means), and the Device ID record (vendor, product,
  version).
- Opens the two HID channels (control, PSM 0x11, and interrupt, PSM
  0x13) and logs each input report.
- When the link is lost, goes back to searching.
- Prints a MEM line every 5 s (heap and stack use) and a stats line
  every 10 s.
- Draws on the screen 30 times a second, but only what changed (see
  "The screen" below). The BOOT button turns the page.
- Answers the debug link on the same serial cable, so `boardctl` can
  take a screenshot, turn the page and read figures.

## Files

| Path | What it is |
| --- | --- |
| `src/main.rs` | Starts the board, the screen and the radio; runs the radio loop and the screen loop side by side |
| `src/ui.rs` | The screen hook: `UiSink`, `ViewSink` (logs each event and gives it to pad-view's `View`), and `Shared`, what the two loops share |
| `src/screen.rs` | The screen loop: draws every 33 ms, reads the BOOT button, answers the debug link, prints the PAD-UI line |
| `src/display.rs` | The screen as an embedded-graphics drawing target: one window, then the pixels through a small buffer |
| `src/lcd.rs` | The ST7789 panel on SPI2 with DMA (a trimmed copy of `experiments/display-dma/src/lcd.rs`), and reading it back for screenshots |
| `src/demo.rs` | Only in the `demo` build: made-up reports, to check the picture without a gamepad |
| `src/out.rs` | The serial log: a 16 KB pipe and a task that prints it slowly (paused during a screenshot) |
| `src/mem.rs` | The MEM line (same code as bt-classic-probe) |
| `src/pairings.rs` | Saved pairings: finds the `nvs` partition, opens the store, loads the keys into the host, saves and forgets |
| `.cargo/esp-config.toml` | Puts the SPI driver in fast memory (see "Drawing speed") |
| `tools/padlog.py` | Reads the serial port or a saved log and prints a summary |
| `../../crates/pad-proto` | The Bluetooth host itself (tested on the PC) |
| `../../crates/pad-view` | Report decoding and the three pages (tested on the PC) |
| `../../crates/pad-store` | Keeps pairings in flash, safe against a power cut (tested on the PC) |
| `../../crates/debug-link` | The debug link's frames, shared with `tools/boardctl` |

## How to build

From the repository root:

```sh
export PATH="$HOME/.cargo/bin:$PATH"; . ~/export-esp.sh
cd experiments/bt-gamepad-test
cargo build --release
```

The `dual` feature is on by default. The firmware lands in
`target/xtensa-esp32-none-elf/release/bt-gamepad-test`.

The two libraries' own tests run on the PC, with no board:

```sh
cd crates/pad-proto && cargo test
cd crates/pad-view && cargo test
```

### The demo build

```sh
cargo build --release --features demo --target-dir target/demo
```

It lands in `target/demo/xtensa-esp32-none-elf/release/bt-gamepad-test`
(its own folder, so it never mixes with the real build). It feeds
made-up events through the same path as real ones, 1.5 s after start: a
device "DEMO canned pad" with the GuliKit's real descriptor, then a
report every 8 ms (125 a second) where the sticks turn in circles, the
triggers ramp, the d-pad turns and the buttons light one after another.
At 30 s in each 40 s cycle the link is "lost" for 4 s (the picture
greys out). When it comes back, the other kind of pad comes: "DEMO
Switch pad" (Nintendo ids, no descriptor) sends full reports 0x30 with
the same moving sticks and buttons, and a motion sensor that rocks the
pad side to side (40 degrees, every 4 s) and nose up and down (25
degrees, every 6 s): the ball in the left grip rolls, the gyroscope
numbers in the right grip move. After the next loss a DInput pad
comes (ids 1949:0402, the 148-byte descriptor the GuliKit really sent
in its DInput mode): report 0x07 every 8 ms with the sticks
and hat moving, and one thing pressed at a time, 400 ms each: buttons 1
to 12 (7 and 8 are LT and RT: the bars fill fully, "triggers: on/off"),
then a vendor bit (the "?" lamp lights: "07 b8.0 vendor FF00:1"), then AC
Back in the consumer report 0x02 ("02 b1.6 AC Back"), then AC Home (the
H button). After the next loss the Xbox pad comes back, and so on. The demo sends nothing to the pad (the HID channels
are never open, so no output is asked for); its Switch reports are 0x30
already, so the About page shows "Start-up: 0x30 came unasked; not
started" and "Steps: 03 30 ok, 40 01 -, 30 01 -". The radio still searches
meanwhile. The boot line says
`demo=1`. **Never leave the demo build on the board**: flash the real
one afterwards.

## How to flash and listen

The board is shared. Every command that opens the serial port runs under
the lock and with a time limit of at most 150 s.

```sh
# from experiments/bt-gamepad-test
LOCK=../../.local/board.lock

# flash
flock $LOCK timeout 90 espflash flash --chip esp32 --port /dev/ttyUSB0 \
    --non-interactive target/xtensa-esp32-none-elf/release/bt-gamepad-test

# listen for 130 s, keep the raw log, print a summary
flock $LOCK timeout 145 python3 tools/padlog.py --port /dev/ttyUSB0 \
    --reset --secs 130 --save logs/run.log

# a summary of a log saved earlier
python3 tools/padlog.py logs/run.log
```

Opening the serial port with `padlog.py` restarts the board, so every
listen starts from boot. `boardctl` does not restart it (except
`boardctl reset` and `boardctl log --reset`).

```sh
# from the repository root
LOCK=.local/board.lock
B=tools/boardctl/target/release/boardctl

flock $LOCK timeout 60 $B shot -o shot.png     # screenshot, about 1 s
flock $LOCK timeout 30 $B key a                # next page (any button name)
flock $LOCK timeout 30 $B stats                # figures, key=value lines
flock $LOCK timeout 30 $B ping
flock $LOCK timeout 100 $B log --seconds 80    # the log, without a restart
```

`padlog.py` echoes the PAD-STATE, PAD-ERR and MEM lines while it reads,
with every Bluetooth address replaced by `<addr>`. The summary at the end
has counts only. The saved raw log does hold the gamepad's address, so
keep it in `logs/` (ignored by git). Exit code: 0 fine, 1 errors or a
panic, 2 no firmware lines.

To pair: start a listen, then hold the gamepad's pairing button. A
gamepad stays in pairing mode only about 30 to 40 s, and the firmware
calls it as soon as a search round hears it.

## Trying it with the gamepad

This is a walk-through for a live try with the GuliKit KingKong 2 Pro,
in all four of its modes. Check first that the real build is on the
board: the boot line says `demo=0`, and the screen says "Searching".

### 1. Record the log

One command, from the repository root. It does **not** restart the board,
so pairings made earlier in this run are kept:

```sh
flock .local/board.lock tools/boardctl/target/release/boardctl log --seconds 900 | tee -i experiments/bt-gamepad-test/logs/live.log
```

- It shows the log on the screen and writes it to `logs/live.log` at the
  same time. It stops after 15 minutes (`--seconds 900`); Ctrl+C stops
  it earlier, and the file keeps everything up to that point.
- It holds the board lock the whole time, so nothing else uses the
  serial cable meanwhile.
- The file holds the gamepad's address. `logs/` is ignored by git; do
  not copy the file anywhere public.
- Afterwards, a short summary (addresses and names hidden):

  ```sh
  python3 experiments/bt-gamepad-test/tools/padlog.py experiments/bt-gamepad-test/logs/live.log
  ```

Use a new file name for each try (`live2.log`, ...), or the old one is
overwritten.

### 2. What the screen shows, and when to press

The top line of the screen is the status. In a good pairing it goes
through these, in order:

| Screen says | What is happening | What you do |
| --- | --- | --- |
| `Searching (round N)` | The console searches for 3.84 s, rests 1.5 s, and again | **Now**: hold the pad's Pairing button 2 s, until the player lights scroll |
| `Found: <name>` | A search heard a gamepad; its name shows | Nothing. Check that the name is your pad's |
| `Connecting to <name>` | The console calls the pad | Nothing |
| `Gamepad is calling` | Instead, the pad called the console (it knew it already) | Nothing |
| `Linked (we called), pairing`, then `Pairing` | The link is up; pairing ("just works", no PIN) | Nothing |
| `Paired, encrypting` | Pairing done, the link is being encrypted | Nothing |
| `Encrypted, reading description` | The console reads the pad's service list (its name, ids, report map) | Nothing |
| `Opening HID channels` | The two data channels open | Nothing |
| `Connected` | The picture of the pad appears | Press every button and move both sticks and triggers |
| `Lost: <reason>` | The link ended | After 1 s the search starts again |

The whole run from "Found" to "Connected" should take a few seconds.
The pad stays in pairing mode only 30 to 40 s; one search round is about
5.3 s, so it should be found within a round or two. If the lights stop
scrolling and the screen still says "Searching", hold Pairing again.

If something fails, the screen says so instead (`Connect failed: ...`,
`Pairing failed: ...`, `Pairing timed out`, `Encryption failed: ...`,
`HID channels did not open`, `HID channel refused`, `Gamepad unpaired
itself`), a PAD-ERR line says why, and the search starts again.

The BOOT button on the board turns the page: Pad (the picture), Data
(every value in numbers), About (name, class, ids, report map).

In Switch mode the motion sensor shows too (once the pad answered
`40 01`, "motion sensor on"):

- Pad page: a small level in the lower left grip. A ball rolls to the
  side of the pad that is lower; it is green when the pad lies flat
  (within 3 degrees), red when it lies face down, grey when the link is
  lost. The lower right grip shows the gyroscope (how fast the pad
  turns) in degrees a second, x, y and z. Before any motion data it says
  "No motion data yet".
- Data page: on the right, the six numbers of the newest sample
  (accelerometer in g, gyroscope in degrees a second, each with its raw
  number), and the tilt (roll and pitch).
- About page: "Motion sensor: yes (accelerometer + gyroscope, Switch
  report 0x30)" and whether the pad answered `40 01`; "asked, no data
  yet" while the sensor sends zeros; "not in this mode" in the XInput
  and Android/iOS modes; "none found" for a pad decoded from its own
  report map (DInput mode).

To check it: lay the pad flat (the ball sits in the middle and turns
green, Acc Z near +1.00 g on the Data page), then tilt it right side
down (the ball rolls right, Acc Y goes towards +1 g) and nose up (the
ball comes down, Acc X towards +1 g). If one goes the wrong way, note
which. The motion numbers change on the screen at most 10 times a
second on the Pad page and 4 times on the Data page, and not at all
while the pad lies still.

### 3. The four modes of the KingKong 2 Pro

From the maker's manual (NS09 English manual, archived copy on
manuals.plus):

- Turn the pad on: click Mode once. Turn it off: double-click Mode.
- Change mode: hold Mode 2 s or more.
- Pairing: hold Pairing 2 s, until the player lights scroll.
- The manual names the modes "PC XINPUT", "Android iOS", Switch, and
  "DINPUT" (for old games and emulators on Windows).

**Unchecked** (not in the manual's text, not tried by us yet):

- The order. An unofficial guide says each long press of Mode moves
  XInput → Switch → Android/iOS → DInput → XInput.
- Which light colour means which mode. It is only in the manual's
  pictures. So tell the mode by the **name the console shows** at
  `Found:`, and by the `name=` in the PAD-SUMMARY line.
- Whether the pad must be on or off when Mode is held.

| Mode | How to pick it | Name you should see | What to expect |
| --- | --- | --- | --- |
| XInput | Hold Mode 2 s until this name shows (unchecked) | The pad's XInput-mode name (seen on our board) | Vendor 045e, product 02e0. Reports with id 01 (16 bytes). Already paired with our board once |
| Android/iOS | Hold Mode 2 s until this name shows (unchecked) | `Xbox Wireless Controller` (manual) | The same reports as XInput. Not tried on our board. One Linux driver author said this mode did not pair with his PC |
| Switch | Hold Mode 2 s until this name shows (unchecked) | `Pro Controller` | Vendor 057e, product 2009, and a **different address**. Reports id 3f first, then 30 once the console asks. Player light 1 should turn on: that shows the console's start-up packets reached the pad |
| DInput | Hold Mode 2 s until this name shows (unchecked) | Unknown: please write it down | Unknown reports. The screen falls back to the pad's own report map, or raw bytes |

In Switch mode the pad may send a report only when something changes.
If the log says `waiting: the HID channels have been open ... no input
report came`, press a button.

### 4. Between two modes

- **The console saves pairings in flash.** A restart (reset button,
  `padlog.py --port`, `boardctl reset`, `boardctl log --reset`) or a new
  flash keeps them. Only "forget all" removes them (see "Saved
  pairings").
- A pad paired with an older firmware (one that kept keys in RAM only)
  calls with a key the console never saved. The console says it has no
  key, so the pad drops the link (the screen shows `Gamepad is
  calling`, then `Lost: ...`). That is harmless: hold Pairing 2 s and
  it pairs anew, this time for good.
- The pad remembers the last device it paired with, probably one for
  each mode (unchecked). If a phone or PC paired with it in the same
  mode is nearby, the pad may go there instead: switch Bluetooth off on
  those for the try.
- How to make the pad forget all its pairings: the manual's text does
  not say (unchecked). Holding Pairing 2 s starts a fresh pairing, and
  that is enough here.

A good order for each mode:

1. With the pad on, hold Mode 2 s to move to the next mode.
2. Wait until the screen says `Searching`, then hold Pairing 2 s.
3. Wait for `Connected` and the `PAD-SUMMARY flowing` line, then press
   every button and move every stick and trigger for about 20 s.
4. Turn the pad off (double-click Mode). The screen says `Lost: ...`
   and the log prints `PAD-SUMMARY end`.
5. Next mode.

### 5. What the log shows in a good pairing

In order (`<addr>` is where the pad's address appears in the file):

1. `PAD-STATE Searching (round N)` and `PAD-HCI inquiry-complete ...` once a round.
2. `PAD-HCI found gamepad addr=<addr> cod=0x...508 ... name="..."`, then
   `PAD-STATE Found: ...` and `Connecting to ...`.
3. `PAD-HCI connection-complete status=0x00(ok) ...`, then
   `PAD-STATE Linked (we called), pairing`.
4. Pairing: `link-key-request ... have_key=0`, `io-capability-request`,
   `user-confirmation-request ... answer=yes (just works)`,
   `simple-pairing-complete status=0x00(ok)`, `link-key-notification ...
   stored=1 (key never printed; ...)`, then `PAD-STORE saved new
   pairing ...`, and `authentication-complete status=0x00(ok)`. With a
   saved key: `link-key-request ... have_key=1`, then `link_key_reply
   ... +key(hidden)` (the key bytes are left out). When the pad calls
   us, it checks the key by itself, so there may be no
   `authentication-complete`; `encryption-change ... enabled=1` shows
   that the key was right.
5. `PAD-HCI encryption-change status=0x00(ok) ... enabled=1`.
6. SDP: `PAD-SDP query uuid=0x1124 done records=1 ...`,
   `PAD-SDP descriptor bytes=N`, `PAD-SDP device-id vendor=... product=...`.
7. `PAD-L2CAP open hid-control ...` and `PAD-L2CAP open hid-interrupt ...`,
   then `PAD-STATE Connected`.
8. `PAD-STATE first input report N ms after the HID channels opened ...`
   and `PAD-HID report n=1 ...`.
9. Switch mode only, the start-up prober (see pad-view's MODES.md), one
   `PAD-STATE ui switch start-up: ...` line per event:
   `probe starts, N ms after the first input report; 7 ways to try`,
   then per packet
   `variant=info-49 sent subcommand 0x02 (device info) len=49 rumble=neutral channel=interrupt counter=0, way 1 of 7, try 1 of 3`,
   per answer `answer to subcommand 0x02 (device info) ack=0x82 20 ms after the last send, credited to variant=info-49`,
   `device info: firmware 3.72 type=3 (Pro Controller)` (never the
   address), then `variant=info-49 WON after ... ms of probing; ...`,
   the steps 03 30, 40 01, 30 01 sent the same way, and
   `conversation finished`. If no way works:
   `variant=<name> had no answer after 3 tries` for each, then
   `all 7 variants failed (...); carrying on with the simple 0x3F reports`.
   The host adds, per output, `PAD-HID output n=.. len=.. cid=..` (or
   `PAD-HID set-report ...` on the control channel), `PAD-HCI completed
   output n=.. after .. ms: our controller sent it`, and for a
   SET_REPORT `PAD-HID handshake result=...`. `tools/padlog.py` prints
   all of this in its "switch start-up" section.
10. 2 s after the first report: two summary lines, for example

    ```
    PAD-SUMMARY flowing name="Pro Controller" class=0x002508 vendor=057e product=2009 version=0001 vendor_source=0002 descriptor_bytes=170 pairing=new link=we-called reached=reports reports=84 rate=42.0/s ids=30:49x80,21:49x4 outputs=4 set_reports=0 handshakes=0 acl_sent=30 acl_completed=30 found_to_link_ms=310 link_to_hid_ms=900 hid_to_first_report_ms=40 link_ms=3200
    PAD-SUMMARY view layout=SwitchPro because ... mode_guess="..." switch_startup=won:info-49,03:ok,40:ok,30:ok switch_info=fw3.72/type3 switch_handshake=none outputs_sent=4 reports_seen=84
    ```

    `switch_startup=` is `not-needed`, `waiting-to-start`,
    `probing:<variant>(try k of 3)`, `won:<variant>,03:..,40:..,30:..`
    (each step `ok`, `waiting`, `no-answer` or `not-sent`) or
    `all-variants-failed(<list>)`. The About page shows the same in
    three lines (Start-up, Steps, Pad info).
    `pairing=` is `new` (paired now), `stored-key` (used a saved key)
    or `stored-key-not-accepted`. `ids=` lists each
    report id seen, as id:length x count. `rate=` is reports a second.
11. When the link ends: `PAD-SUMMARY end ... reason=0x13(remote-user-ended)`
    (the pad closed the link, for example when it was turned off),
    then `PAD-STATE Lost: ...` and the search again.

A line that starts `PAD-ERR timeout:` says which step ran out of time
and what the console was waiting for. That line is the first thing to
look at when a try fails.

## Saved pairings

Pair the pad once. After that it connects again on its own, also after
a restart or after flashing a new firmware.

How it works:

- At start-up, before the radio starts, the firmware reads the
  partition table (the list of areas on the flash chip) and finds the
  `nvs` data partition. The store uses its first 8 KB (0x9000 to
  0xB000 on this board). The bootloader, the partition table and the
  program sit outside that range, and the code checks it (see
  `crates/pad-store`).
- The saved pairings are loaded into the host. The About page shows
  `Known pads: N (saved)`.
- When the pad pairs, the host gets the new key
  (`link-key-notification`). The radio loop saves it right away. A save
  writes one small record of 32 bytes and takes well under 1 ms.
- Erasing a flash sector takes about 45 ms, and the radio cannot run
  during that time. So the store erases only at start-up, never while
  the radio runs.
- At most 4 pads are kept. A 5th pushes the oldest out.
- A pad whose key is refused (the pad says the key is wrong) is
  forgotten. A pad that only hangs up is not: hanging up says nothing
  about the key.

If the store cannot be used (no `nvs` partition, flash encryption on,
or the partition holds data the store did not write), the firmware
keeps keys in RAM only, as before. The log says why
(`PAD-STORE store not used (<why>): ...`) and the About page shows
`Known pads: N (RAM only: <why>)`.

To forget every pad:

- Type `f` in the serial terminal, or run `boardctl text f`.
- Or hold the BOOT button for 5 s. After 1 s the screen counts down
  (`Hold BOOT 4 s: forget all pads`). Letting go early keeps them.

The screen then shows `All pads forgotten: pair again` for 8 s. The pad
must go into pairing mode once more.

Limits (this is an experiment): the keys sit in flash as plain bytes,
not encrypted. Anyone with the board and a USB cable can read them.

The `store-selftest` build saves a made-up pairing (made-up address,
made-up key) once, while the radio searches. It checks that a flash
write during a search causes no radio errors. The normal build has no
made-up pairing in it. Flash a normal build afterwards, then forget all
(`f`) to remove the made-up pairing.

```sh
cargo build --release --features store-selftest
```

## Battery (Xbox modes)

In its Xbox modes the pad lists input report 04, the battery report,
but may not send it by itself. So once per link, after the first
report, the console asks for it: a GET_REPORT ("send me input report 04
now", header 0x41) on the HID control channel. The About page then
shows one of:

- `Battery: level N of 3 (byte 0x..)`, with `, charging` when the pad
  says so. Bits 0 and 1 are the level and bit 4 is "charging", as Xbox
  pads use them. This is not yet checked on this pad, so the raw byte
  is shown too.
- `Battery: not reported (pad said no, code N)`: the pad answered with
  an error code (2: no such report, 3: request not supported).
- `Battery: not reported (no answer in 1 s)`.

In the log: `PAD-HID get-report n=...`, then `PAD-HID get-report answer
id=0x04 ...` or `PAD-HID handshake ... after get-report id=0x04 ...`.
The `PAD-SUMMARY view` line ends with `battery=... battery_byte=...`.
Not tried with the pad yet; the PC tests cover all three outcomes.

## Optional board tests

Two tests are switched on at build time with an environment variable.
The address is given on the command line only: never keep it in a file
in the repository, and flash a normal build afterwards so the board is
not left with it.

- `PAD_TEST_PAGE=AA:BB:CC:DD:EE:FF`: after two search rounds, call this
  made-up address once. It must fail with a page timeout (status 0x04),
  and the search must go on.
- `PAD_PEER=AA:BB:CC:DD:EE:FF`: after that, open one SDP-only link to
  this device (meant for a phone of your own). No pairing, no
  authentication: one SDP search for UUID 0x0100 in small pieces of at
  most 64 bytes, so the answer comes in many pieces joined with
  "continuation state". Then the link is closed. The address is replaced
  by `<peer>` in every log line.

```sh
PAD_TEST_PAGE=00:11:22:33:44:55 cargo build --release
```

## Log tags

Every line starts with a tag.

| Tag | What it says |
| --- | --- |
| `PAD-STATE` | Status changes, screen events (`ui ...`), stats every 10 s |
| `PAD-HCI` | Commands to the radio controller and its events (HCI is the language between host and controller) |
| `PAD-L2CAP` | Channels: open, configure, close (L2CAP carries all data over one link) |
| `PAD-SDP` | SDP requests, answer pieces, the records read, the descriptor in hex |
| `PAD-HID` | Input reports in hex: the first ones in full, then the first of each second and a count per second. Also the button trace (below) |
| `PAD-ERR` | Something went wrong; the line says what and what happens next. A time limit that ran out starts with `timeout:` and says what was awaited |
| `PAD-SUMMARY` | One line when reports have flowed for 2 s (`flowing`) and one when the link ends (`end`), then a `view` line from the screen side. See "Trying it with the gamepad" |
| `PAD-UI` | The screen: every 5 s the drawing and radio-turn figures (see below); also page changes and screenshots |
| `PAD-STORE` | Saved pairings: the partition found, the store opened, pairings loaded, saved and forgotten. Never a key, never an address |
| `MEM` | Heap and stack use, lines lost from the log pipe, packets not sent |

In a healthy run `heap_free` in the MEM line stays the same for minutes,
and `log_dropped`, `tx_dropped`, `read_errors` and `heap_refused` stay at
0.

### The button trace

The `PAD-HID report` lines show only about one report a second, so a
short press can fall between them. The screen side (`src/ui.rs`, with
pad-view's `trace` module deciding) writes one more line for each report
worth seeing:

```
PAD-HID change n=812 id=07 len=9 gap=8 hex=07807f807f000f0400 held=LT unmapped=""
PAD-HID change n=902 id=07 len=9 gap=8 hex=07807f807f000f0001 held=- unmapped="07 b8.0 vendor FF00:1"
PAD-HID same n=1040 id=01 len=16 gap=412 hex=0100c0... held=- unmapped=""
```

| Word | When |
| --- | --- |
| `change` | A standard button, the hat, a button with no standard name, or an unmapped bit or value turned on or off; also the first report of each id. An unmapped value that only moves while it is on (a noisy byte) is not a change |
| `same` | Byte for byte the same as the last report of its id, after at least 150 ms with no report. In the Xbox modes the GuliKit sends only on a change, so this means a press the report has no place for |
| `analog` | Only sticks or analog triggers changed, after such a silence (the first move, not every step) |
| `trace` | Any other report during a full trace (key `t`, below) |

`n=` is the report count (as in `PAD-HID report`), `gap=` the ms since
the report before (`-` for the first), `hex=` the report (the first 64
bytes, `..` when cut), `held=` the standard buttons held by their names
on screen (`B13` for a button with no standard name; `-` for none),
`unmapped=` the unmapped input on now, as on the Data page, joined by
`|`. Stick wobble alone makes no line. All four kinds together: at most
40 lines a second. What the cap skipped is logged when its second is
over: `PAD-HID trace skipped=N (more than 40 lines in one second)`.

The letter `t` on the serial line starts a full trace: every report gets
a line for 10 s (still at most 40 a second), between
`PAD-HID trace start: ...` and `PAD-HID trace end: ...`. The `end` line
comes with the first report after the 10 s.

The `view` line of PAD-SUMMARY adds `triggers=` (`on/off` or
`analog-or-unknown`), `unmapped_screen_changes=`,
`unmapped_report_changes=`, `trace_lines=` and `trace_skipped=`.

`tools/padlog.py` sums it up in a "button trace" part: per connection
the distinct (held, unmapped) states with a line count, every unmapped
item seen, and the number of `same` lines per report id.

## The state machine, in words

The host is in one of these phases (the `phase=` word in the log):

1. **init**: reset the controller, read its address and buffer sizes,
   set the name ("PocketMe"), the class ("computer, laptop"), Secure
   Simple Pairing on, connectable but not visible. Then go to *inquiry*.
2. **inquiry**: search for 3.84 s.
   - A device answers with a gamepad or joystick class: cancel the
     search (*cancelling*), then call it (*paging*).
   - The search ends with nothing: go to *pause*.
3. **pause**: wait 1.5 s. A gamepad that paired earlier in this run can
   call us now. Then back to *inquiry*.
4. **paging**: we called. If the call fails (for example status 0x04,
   page timeout: nobody answered), log it and go back to *pause*.
5. **accepting**: a device called us and we said yes (only a peripheral,
   or a device paired earlier in this run).
6. **pairing**: the link is up. We ask for authentication.
   - With a key from earlier in this run: the key is used, no pairing.
   - Without one: Secure Simple Pairing, "just works".
   - Then encryption is switched on. If both sides start the same
     security step at once (a "collision", status 0x23 or 0x2A), we try
     once more after 500 ms. A second failure drops the link.
7. **opening**: the link is encrypted. Read the HID and Device ID
   records over SDP (a failure here is logged, not fatal). Then open the
   HID control and interrupt channels, or accept them if the gamepad
   opens them first. If both sides open the same channel at once, the
   gamepad's is kept and ours is closed.
8. **connected**: both HID channels are open. Input reports flow to the
   screen hook.
9. When the link is lost from any of 6, 7 or 8, the screen hook gets
   `Disconnected` and the host goes back to *pause* (1 s), then
   *inquiry*.

Every step that waits has a time limit: 3 s for a controller command,
5 s for a channel answer, 30 s for pairing and encryption, 8 s for SDP,
and 28 s in all for the HID channels to open. Each one, when it runs
out, prints a PAD-ERR line that starts with `timeout:` and says what the
host was waiting for. When SDP runs out of time, the host goes on to the
HID channels without it. When any other limit passes, the link is
dropped and the search starts again (after a 1 s pause).

A few short waits are on purpose. When the gamepad called us, the host
waits 0.6 s before asking for pairing and 1 s before opening the HID
channels, because the gamepad usually does both itself. When we called
it, the host opens the HID channels right after SDP.

## The screen hook

`src/ui.rs` holds the trait `UiSink`. The firmware passes every screen
event to it: `Status`, `Device`, `Descriptor`, `Report` and
`Disconnected` (the type is `pad_proto::UiEvent`). The sink in use is
`ViewSink`. It logs the event (PAD-STATE `ui ...` lines, as before, but
not each report) and gives it to pad-view's `View` at once.

Rules: `event` runs on the radio loop and must return quickly. Giving a
report to the `View` only decodes it into a few numbers; the longest
event so far took about 0.9 ms (`event_us_max`). Borrowed data is valid
only during the call. `next_output` asks the `View` for output reports
(the Switch start-up packets); each names its channel, and the host
sends it on the HID interrupt channel (0xA2) or as a SET_REPORT on the
control channel (0x52). It is asked after every device, descriptor,
report and control event, and every 20 ms, but only once both HID
channels are open. After a send the sink is told (`output_sent`) and
logs the start-up notes. If the host still cannot send a report, the
sink is told (`output_not_sent`) and gives it back: it is asked for
again later, does not count as a try of the Switch start-up, and leaves
no "sent" line. Control-channel messages reach the `View` too
(`UiEvent::Control`), for the handshake and any answer sent there. The
trace lines write the pad's address inside its device-info answer as
`xx`.

There is no queue of reports. Each report changes the `View` at once,
so the screen always draws the newest state, every report is counted
(`reports=`), and nothing piles up when the screen is slower than the
gamepad.

## The screen

- The panel is the CYD's ST7789: SPI2 at 40 MHz, landscape (MADCTL
  60h), 320 x 240, backlight on GPIO21.
- There is no picture of the whole screen in memory (that would be
  150 KB). Each drawing call opens one window on the panel and sends the
  pixels through two 4 KB buffers by DMA, or through the SPI's small
  FIFO when it is under 512 bytes. A window with the same columns or
  rows as the last one skips that command.
- pad-view draws only what changed. A call that changed nothing sends
  nothing (it costs at most about 3.5 ms, usually under 0.3 ms).
- The radio loop and the screen loop run in the same task and take
  turns. A draw cannot be cut in half, so a long draw makes the radio
  wait. The radio driver keeps incoming packets in a queue meanwhile, so
  nothing is lost; it is only late.
- BOOT button (GPIO0, pressed = low): read every 10 ms, it must read
  the same 3 times in a row (30 ms) to count. A press turns the page.
  `boardctl key <name>` and the letter `n` on the serial line do the
  same. The letter `t` on the serial line starts a 10 s full button
  trace (see "The button trace").
- The three pages: Pad (the picture; in Switch mode also the tilt ball
  and gyroscope numbers), Data (each value in numbers, the motion sensor
  in Switch mode, the last report of up to 4 ids in hex, and the
  unmapped input), About (name, class, ids, layout, descriptor summary,
  motion sensor).
- Triggers that are only buttons (Switch, and a descriptor with trigger
  buttons and no trigger axis, as the GuliKit's DInput mode): the bar
  fills fully while held, and "triggers: on/off" shows between the bars.
- Unmapped input: any bit of a report that no standard control takes (a
  declared field such as a vendor bit or a consumer key, padding that
  changes, bytes the descriptor does not declare). The Pad page has a
  "?" lamp under the d-pad, yellow while something is on, with the first
  item ("07 b8.0 vendor FF00:1": report id, byte.bit, name) and "+N" for
  more. The Data page lists them all under "Unmapped input on". The
  list is taken at most 4 times a second; a press shorter than that
  still shows once. The Xbox and Switch layouts count the bits their
  decoder reads by hand as used; the others use the descriptor.
- Motion is redrawn only past a small step (0.02 g, about 2 degrees a
  second), so the ball and the motion numbers of a still pad cost
  nothing. A moving pad: at most 10 changes a second on the Pad page and
  4 on the Data page. One change measured on the PC: about 700 pixels in
  18 windows on the Pad page (about 1.2 ms on the panel, so at most
  12 ms a second), 4000 pixels in 100 windows on the Data page (about
  7 ms, so at most 28 ms a second).
- The raw bytes in hex (the last report on the Pad page, the last
  report of each id on the Data page) are drawn again at most twice a
  second. With the motion sensor on, nearly every byte from 13 on
  changes in every report (sensor noise); drawing them on every call
  took about a quarter of the board's time. They may lag up to half a
  second behind the pad.
- Measured on the PC for a whole page (tests/render.rs,
  `motion_share_of_screen_time`): a Switch pad moving all the time costs
  about 25 ms a second more than one with the sensor off on the Pad
  page, about 29 ms more on the Data page (under 3% of the time).

### The PAD-UI line

Every 5 s, for example:

```
PAD-UI draws=8422 drawn=1092 full=9 full_us_max=125258 small_us_max=23982 ...
```

| Field | Meaning |
| --- | --- |
| `draws`, `drawn` | Calls to `draw()`, and those that sent something |
| `full`, `full_us_max` | Draws of more than 20,000 pixels (a new page), and the longest one |
| `small_us_max`, `small_px` | The longest other draw, and its pixels |
| `idle_us_max` | The longest draw that sent nothing |
| `win_draw_us_max`, `win_draw_px`, `win_draw_bus_us` | The longest draw in the last 5 s, its pixels, and its time inside windows |
| `cpu_bench_us` | A fixed piece of work, timed: if it grows, something else takes the CPU |
| `event_us_max` | The longest screen event inside the sink |
| `gap_us_max`, `gap_us_max_all` | The longest wait between two turns of the radio loop: in the last 5 s, and since start |
| `gaps_over_50ms` | How many such waits were longer than 50 ms |
| `tick_late_us_max` | How late the radio loop's 20 ms timer fired, at most |
| `gap_shot_us_max` | The same during screenshots (kept apart) |
| `pages`, `boot`, `keys`, `shots` | Page changes, BOOT presses, debug-link keys, screenshots |

### Drawing speed

The first measurements changed about five times between two builds that
differed only a little (a full page 540 ms or 139 ms). The ESP32 runs
code from flash through a small cache, and the drawing code was missing
it. Now the SPI driver (esp-hal's `place-spi-master-driver-in-ram`, set
in `.cargo/esp-config.toml`) and our window code (`#[esp_hal::ram]`)
run from fast internal memory.

### Screenshots

`boardctl shot` reads the picture back from the panel's own memory at
4 MHz, so it shows what the glass shows. It takes about 1 s. The log
printer is paused meanwhile and resumes afterwards (no line is lost:
`log_dropped=0`). The radio loop gets a turn after every frame of the
picture; the longest wait during a shot was about 165 ms.

## Results so far (2026-11-16)

Saved pairings, checked on the board:

- The store finds the `nvs` partition and uses 0x9000 to 0xB000.
  Opening it takes about 5 ms (8.5 ms the first time, when it sets up
  the empty area).
- Self-test build: the made-up pairing was saved during a search in
  0.55 ms (a new pad) and 0.78 ms (a new key for a known pad). The
  radio showed 0 errors and there was no `PAD-ERR`.
- The pairings survived a restart and a new flash (`loaded 2 saved
  pairing(s)`: the made-up one and the owner's pad).
- The owner's pad paired while the self-test build ran. After a restart
  it called on its own, the console answered with the saved key,
  encryption came on and the HID channels opened (`pairing=stored-key`).
  The pad then hung up before the Switch start-up, with 0 reports. It
  is not known yet whether that was the pad or the console.
- `f` (by `boardctl text f`): `forgot all pairings: 2`, the screen
  showed `All pads forgotten: pair again`, then `Known pads: 0
  (saved)`; after a restart `loaded 0`.
- Size: the program grew from 510,656 to 536,240 bytes with the store,
  and to 538,912 bytes with the battery question too. The store's
  state in RAM is 84 bytes; the heap is unchanged (79,156 bytes free).
  Stack: 27,040 bytes before the store opens, 27,632 after, 35,696
  while searching, 37,168 at most with a pad linked (of 49,900).
- Not tried: holding the real BOOT button 5 s.

Second check of the store (no gamepad):

- Stack fix: the flash calls hold a 4 KB buffer in their stack frame.
  When the compiler folded them into `main`, main's frame grew by about
  9.6 KB for the whole run. They are now kept apart (`#[inline(never)]`).
  Stack: 17,360 bytes at start, 18,384 after the store opens, 26,000
  while searching (of 49,844). Before the store it was about 25,700.
- Size: 536,304 bytes (normal build). Heap free still 79,156.
- A short link-key event that cannot be read is now logged without its
  bytes.
- Board: the self-test pairing survived a restart and a new flash, `f`
  wiped it (`1 wiped`), and the normal build then said `loaded 0`.

After the review fixes (Switch start-up waits for HID, MTU kept across
a two-part configuration, optional allow-list, timeout lines,
PAD-SUMMARY), checked on the board without a gamepad:

- Real build: radio mode 3, `allow_list=0`, 47 search rounds in about
  4 minutes, 0 errors, the "Searching" screen with the pairing hint
  (screenshot checked).
- Memory: `heap_free` 79156 from t=5 s to t=210 s, `heap_refused` 0,
  stack at most 22,544 of 54,188 bytes, `log_dropped` 0.
- Demo build still draws the controller (screenshot checked); the real
  build was flashed back afterwards.
- Not yet tried with the pad: the new summary lines and the Switch
  start-up timing. The PC replay tests cover both.

With the screen (all on the board, figures from the PAD-UI and MEM
lines):

- Real build: boots in radio mode 3, search rounds run, the screen shows
  "Searching (round N)" with the pairing hint (screenshot checked).
- Memory with the screen: `heap_free` 79156 for 205 s without a restart
  (then 79060 after a gamepad link; it stays there), `heap_refused` 0,
  stack at most 22,288 of 55,084 bytes, `log_dropped` 0.
- Drawing while searching: first page 50 ms; the status line change once
  a round about 8.5 ms; a draw with nothing to do well under 1 ms.
  Longest radio wait about 27 ms.
- A full page (page change, a gamepad's name arriving) takes 95 to
  125 ms. The radio waits that long once.
- The owner's GuliKit linked on its own during the test and sent
  reports (about 18 to 53 a second, as it was handled). Draws while
  linked: usually 5 to 16 ms, at most 23 ms; longest radio wait 38 ms;
  every report counted (the host's count and the screen's count match).
- After a restart the GuliKit first called us twice and dropped the
  link itself (it asked for the old link key, which is lost at a
  restart). Then, in pairing mode, the search found it and it paired
  again; the picture went from "Pairing" to "Connected".
- Demo build (125 reports a second, everything moving at once, about
  15,000 pixels a draw): draws about 24 ms, longest radio wait about
  30 ms (110 ms at a full page). This is the worst case; one stick
  moving changes about 1,100 pixels.
- Page change by `boardctl key`: Pad, Data, About, back to Pad
  (screenshots checked, in the demo and in the real build).
- The demo's "link lost" phase greys the picture out (screenshot
  checked).

Not tried: pressing the real BOOT button (only the serial key, which
calls the same function).

Earlier results, before the screen:

- Boot in radio mode 3 (dual), search rounds start and end, MEM flat
  for about 5 minutes (`heap_free` 79156 all the time).
- A call to a made-up address fails with page timeout (0x04) and the
  search goes on.
- An SDP-only link to a phone: 14 pieces with continuation,
  874 bytes, 14 records, closed cleanly, no pairing.
- The owner's GuliKit pad (in its Xbox mode): found, paired ("just
  works"), encrypted, both SDP records read (573 and 136 bytes, the
  descriptor is 306 bytes), both HID channels open, input reports of 16
  bytes received, 0 errors. The pad says it is a Microsoft "Xbox
  Bluetooth Gamepad" (vendor 0x045E, product 0x02E0). It sends a report
  only when something changes, so with nobody touching it only the
  first few reports arrive.

Not yet tried on the board: a reconnect from the pad after a link loss,
and reports while buttons are pressed. Both paths are covered by the
replay tests in `crates/pad-proto/tests/replay.rs`.
