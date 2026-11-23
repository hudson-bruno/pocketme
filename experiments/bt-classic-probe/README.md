# bt-classic-probe

A one-day test: can the ESP32 on the CYD board run **classic Bluetooth**
next to Low Energy from our bare-metal Rust stack, and what does it cost?

Why: the owner's gamepad (a GuliKit pad) talks classic Bluetooth only, and
the Rust stack (esp-radio + trouble-host) is Low Energy only. This is
"option 3" of the gamepad plan: change esp-radio so the controller starts in
dual mode (BTDM), then talk to it with raw HCI commands.

Short answer:

- **Dual mode starts.** The controller reports mode 3 (dual), every HCI
  command is accepted, and Low Energy keeps working.
- **The pad is found.** A 10 s inquiry finds it, class of device
  0x000508 (gamepad), RSSI about -45, with its name in the inquiry
  answer. It was heard in every round while it was in pairing mode.
- **Surprise: the unchanged Low Energy build finds it too.** The ESP32's
  controller runs inquiry, page and remote name request even when it was
  started in Low Energy mode. In that mode it even made a real call-up
  (page) to the pad and read its name (3.3 s). So dual mode is not needed
  to *find* the pad.
- **Second surprise (second check): the LE build can also open a classic
  link.** A real connection (Create Connection) to a phone came
  up in the LE build *and* in the dual build: Connection Complete status
  0, a first data packet from the phone, held 3 s, closed cleanly. So the
  first report's "dual mode is needed to keep a link (LE mode has room for
  0)" is **not** true on this board. What dual mode really changes is the
  mode the controller reports (3), the link limit the ESP-IDF classic
  functions enforce, the extra heap, and ESP-IDF's fixed classic code.
  Whether a link in LE mode stays sound for long, with pairing and
  encryption, is not known; ESP-IDF never runs classic in LE mode, so the
  safe base for a real host is still the `dual` build.
- **Nothing tells the modes apart from the outside.** Feature bits, an
  accepted inquiry with Inquiry Complete, a page timeout, a name request
  and a short link all look the same in both builds. The proof that dual
  mode is on is `controller_mode=3` (read from the library) and the 3,840 B
  more heap the controller takes.
- **Cost of dual mode** (the `dual` build, ESP-IDF's way): 10.5 KB more
  IRAM, 46 KB more flash code, 0.5 KB more static RAM and 3.8 KB more heap.
- Nothing was paired, and no L2CAP or HID was tried. That is most of the
  work (see "What a real classic HID host still needs").

## What is in here

| Path | What |
|---|---|
| `vendor/esp-radio/` | A copy of esp-radio 1.0.0-beta.1 with a small patch, used through `[patch.crates-io]` in `Cargo.toml`. `~/.cargo` is not touched. |
| `vendor/PATCH-NOTES.md` | Every changed line, with file and line number. |
| `esp32-rom-bluetooth.x` | The ROM addresses from `experiments/ble-scan`, plus one more (`lm_get_total_pkt_type`) that the `dual` build needs. |
| `src/main.rs` | Start-up, the list of HCI commands, the LE check, the loop. |
| `src/hci.rs` | Raw HCI: builds each command by hand, reads each event, inquiry, names, page test, the `FOUND`/`NAME`/`SUMMARY` lines. |
| `src/mem.rs` | Heap watch (allocator hooks) and stack paint, same as `ble-scan`. |
| `tools/classiclog.py` | Reads the serial log (a file or the port) and prints a short report. |
| `logs/` | Raw logs of the runs (git-ignored; they hold device addresses). `logs/elf/` keeps the built firmware of each run (git-ignored by this folder's `.gitignore`). |

Three builds, picked by Cargo feature:

| Build | Feature | Controller | Notes |
|---|---|---|---|
| LE | none | Low Energy only, as the unchanged esp-radio | baseline |
| dual-rom | `dual-rom` | dual, classic code from the chip's ROM as it is | patch steps 1-3 only |
| **dual** | `dual` | dual, plus ESP-IDF's fixed classic functions (`config_bt_funcs_reset`) | the way ESP-IDF does it; the one left on the board |

## The patch

All of it is behind two new features of the copy, `bt-dual` and
`bt-dual-idf-patches`. Without them the copy builds the same code as the
original. In short (details in `vendor/PATCH-NOTES.md`):

1. `create_ble_config`: `mode` BLE -> BTDM, `bt_max_acl_conn` 0 -> 2,
   `bt_max_sync_conn` 1 -> 0.
2. `btdm_controller_enable`: called with BTDM, and its return value (an
   `int`) is now read and checked.
3. `patch_apply`: also call `config_bt_funcs_reset()` (only with
   `bt-dual-idf-patches`), between the two calls already there, as ESP-IDF
   does.
4. Linker: `PROVIDE ( lm_get_total_pkt_type = 0x4004f928 );` (address
   checked against ESP-IDF's `esp32.rom.ld`, Apache-2.0).

Steps were flashed and read one at a time:

| Step | Result |
|---|---|
| 0: copy only | Same as crates.io: only 840 B of path strings differ. Board: mode 1 (LE), radio heap 15,740 B. |
| 1-3: BTDM config + enable | Boots, mode 3 (dual), radio heap 18,300 B with 1 classic link. All commands accepted. |
| 4: `config_bt_funcs_reset` | Did not link until the ROM symbol was added (as expected). Then boots the same. |
| acl 1 -> 2 | With 1 link and ESP-IDF's functions, every page or name request was refused at once with 0x09 "connection limit exceeded". With 2 (ESP-IDF's default) it works. +1,280 B heap per link. |

## What the firmware does

At start (log tag `BOOT`, `MEM`): start the radio, print the controller mode
and how much heap the radio took.

Then raw HCI (tag `HCI`, one line per command with status and time):
reset, read version, supported commands, features, extended features,
buffer sizes, address; set the event mask, LE host support, inquiry mode 2
(RSSI + extended answers), local name "PocketMe probe", the extended
inquiry answer, class of device 0x000814 (toy), and scan enable 3 (visible
and connectable).

Then three checks once:

- `LE`: Low Energy scan alone for 10 s, LE scan during a classic inquiry,
  LE advertising "PocketMe LE" for 3 s.
- `PAGETEST`: a name request to a made-up address (must fail with 0x04
  page timeout after about 5.1 s; any other answer means paging is broken).
- `CONNTEST nobody`: a connection attempt (Create Connection) to the same
  made-up address. Same expected result: taken, then 0x04 after 5.1 s.
- `PEERTEST` and `CONNTEST peer`: only in a build made with the
  environment variable `PROBE_PEER=AA:BB:CC:DD:EE:FF` (the address of a
  classic device of your own that is switched on; a phone with
  Bluetooth on is enough, it need not be visible). A name request, then a
  real link: held 3 s, then closed. The address stays out of the source;
  do not leave such a build on the board.

Then for ever:

- `INQ`: a 10.24 s inquiry (general access code).
- `FOUND`: one line per answer (class of device, major and minor class,
  RSSI, HID service in the answer, name in the answer). Devices whose class
  says gamepad or joystick get `GAMEPAD-LIKE`.
- `NAME`: a remote name request for each new device without a name, and
  once for each gamepad-like device even when its name came in the answer
  (that is a real page to it).
- `SUMMARY`: counts only.
- a 2 s pause, and again. `MEM` is printed once a second the whole time.
  Connection requests from other devices are refused (0x0D).

## Results

### Controller (same in all three builds)

| | |
|---|---|
| Version | HCI 8 (Bluetooth 4.2), HCI rev 0x030e, LMP 8, manufacturer 0x0060 (Espressif) |
| Features | bf ee cd fe db ff 7b 87: BR/EDR, LE, LE + BR/EDR together, inquiry RSSI, extended inquiry answer, secure simple pairing, EDR 2 and 3 Mb/s |
| ACL buffers | 1021 bytes x 9 (SCO 255 x 4), LE 251 x 10, 5 command credits |
| Read back | inquiry mode 2, scan enable 3, name "PocketMe probe", class 0x000814 |

Note: in the LE build every classic command is accepted too, and the
features say BR/EDR = 1. So "accepted" proves nothing; only results do.

### Inquiry

With the pad in pairing mode (logs `gamepad-le.log`, `gamepad-dual-rom.log`,
`final-dual.log`, `padname-*.log`):

| Build | Pad found | How | Name |
|---|---|---|---|
| LE | yes, every round | extended inquiry result, RSSI about -48 | from the answer; and by a real name request (page) in 3.3 s |
| dual-rom | yes | extended inquiry result | from the answer; the name request timed out (the pad was leaving pairing mode, inconclusive) |
| dual | yes, 19 of 19 rounds over 245 s | extended inquiry result, RSSI -44 to -50 | from the answer (23 bytes of extended answer). The new "page the gamepad" check came after the pad left pairing mode, so not tested in this build. |

The pad (in its XInput mode), class 0x000508:
peripheral, gamepad. Its inquiry answer has **no** HID service UUID and no
Device ID, so the class of device is the only hint that it is a gamepad.

Other devices nearby: over the 245 s `dual` run, 2 classic devices in all:
the pad and 1 phone. Every answer came as an extended inquiry result
(event 0x2F, 9-35 per round); none as the old kinds (0x02, 0x22). All 18
inquiries ended with status 0. Two name requests (not to the pad) ended in
page timeout.

Before the pad was put in pairing mode, every build heard 0 devices. A pad
that is not in pairing mode does not answer an inquiry; to reconnect, the
host has to page a known address (or the pad pages the host).

### Low Energy while classic is on

| Check | LE build | dual build |
|---|---|---|
| LE scan alone | 2-3 reports/s | about 3 reports/s (30 in 10 s) |
| LE scan during an inquiry | 0.2-0.5 reports/s | 0.3 reports/s (3 in 10 s) |
| LE advertising on/off | accepted | accepted |
| LE scan enable while scan enable is 3 | accepted | accepted |

So LE works next to classic, but an inquiry takes most of the air time:
LE scan drops to about a tenth while one runs. The drop is the same in the
LE build, so it comes from the inquiry, not from dual mode.

### Memory (measured on the board)

Heap: 98,768 B in total (the 96 KB region esp-alloc gets).

| | LE | dual (acl 2) |
|---|---|---|
| Heap free before the radio | 98,768 | 98,768 |
| Heap taken by the radio | 15,740 | 19,580 (+3,840) |
| Heap free after start | 82,996 | 79,188 |
| Least free (whole run) | 82,736 | 78,896 |
| Largest free block at the end | about the same as free | 79,147 |
| Refused requests | 0 | 0 |
| Stack used (of about 111 KB) | 4.4-4.7 KB | 4.8 KB |

With 1 classic link the radio took 18,300 B, with 2 links 19,580 B, so
each link after the first costs 1,280 B. The +3,840 B is not "2 links x
1,280": going from LE (0 links, 1 audio link) to dual with 1 link already
costs 2,560 B. Over 245 s the heap stayed flat (no leak). The "least free"
figures come from the same firmware in both builds (an earlier LE log,
before the page test was added, showed 82,924; the page test itself takes
about 190 B for a moment). A real 3 s link to a phone did not move the heap
in either build.

Static sizes (`xtensa-esp32-elf-size -A`, same source, three builds):

| Section | LE | dual-rom | dual | dual - LE |
|---|---|---|---|---|
| `.rwtext.wifi` (IRAM, radio code) | 37,344 | 37,344 | 47,852 | **+10,508** |
| `.data.wifi` | 776 | 776 | 1,012 | +236 |
| `.bss` | 12,176 | 12,176 | 12,408 | +232 |
| `.data` | 6,692 | 6,700 | 6,692 | 0 |
| `.text` (flash) | 169,561 | 169,645 | 215,349 | **+45,788** |
| `.rodata` (flash) | 34,008 | 34,056 | 36,584 | +2,576 |
| stack left over | 111,424 | 111,416 | 110,960 | -464 |
| `size -B` text / data / bss | 253,365 / 7,468 / 222,368 | 253,497 / 7,476 / 222,360 | 312,237 / 7,704 / 222,136 | +58,872 / +236 / -232 |

So steps 1-3 alone (dual-rom) cost almost nothing (+132 B). All of the
static cost is ESP-IDF's fixed classic functions (`config_bt_funcs_reset`).
dual-rom finds the pad as well; whether a real link works without the fixed
functions is not known. ESP-IDF always uses them, so the safe choice for a
real host is the `dual` build.

App size on flash (dual): 320,048 B of the 4 MB app partition.

### Second check (an independent rebuild, same day)

Rebuilt from clean (`cargo clean`, then the three builds). The section
sizes match the table above to the byte, and the code and data sections
are byte-identical to the first build's firmware. Then, on the board:

| | LE | dual-rom | dual |
|---|---|---|---|
| controller_mode | 1 | 3 | 3 |
| Heap taken by the radio | 15,740 | 19,580 | 19,580 |
| Least free / free at the end | 82,736 / 82,996 | 78,896 / 79,156 | 78,896 / 79,156 |
| Inquiry: taken, Inquiry Complete status 0 after 10.24 s | yes | yes | yes |
| Name request to a made-up address: 0x04 after | 5,129 ms | 5,129 ms | 5,129 ms |
| `CONNTEST nobody`: 0x04 after | 5,134 ms | 5,134 ms | 5,134 ms |
| Name request to a phone | ok, 748 ms | not run | ok, 699 ms |
| Link to a phone (`CONNTEST peer`) | up in 1,570 ms, 1 data packet in, held 3 s, closed (0x16) | not run | up in 1,757 ms, same |

No classic device was in pairing mode during this check, so the pad was
not seen again (0 devices in every inquiry). The phone was reached by its
address (it was not visible), which is why it answered pages but not
inquiries.

## How to run it

Tested as written on 2026-11-16 (second check), from the repository root:

```sh
export PATH="$HOME/.cargo/bin:$PATH"; . ~/export-esp.sh
cd experiments/bt-classic-probe

# build: pick one
cargo build --release --features dual       # the one to use
cargo build --release --features dual-rom   # dual, ROM classic code
cargo build --release                       # LE only (baseline)

# flash (the board is shared: always under the lock and a timeout)
LOCK=../../.local/board.lock
flock $LOCK timeout 60 espflash flash --chip esp32 --port /dev/ttyUSB0 \
    --non-interactive target/xtensa-esp32-none-elf/release/bt-classic-probe
```

### How to listen

`tools/classiclog.py` reads the port for a set time (at most 290 s) and
prints a report with counts and classes only, never an address. A
gamepad-like device is listed with its class, signal and rounds; its name
only if it is one of the names the owner's pad advertises in its modes
(listed in the script), since a gamepad nearby may be someone else's.

```sh
# from experiments/bt-classic-probe, with LOCK set as above
LOCK=../../.local/board.lock
flock $LOCK timeout 280 python3 tools/classiclog.py --port /dev/ttyUSB0 \
    --reset --secs 245 --save logs/run.log

# a report from a log saved earlier
python3 tools/classiclog.py logs/run.log
```

Opening the port restarts the board (checked: even without `--reset` the
log starts with the ROM's boot lines). So every session starts from boot:
about 45 s of start-up checks (LE scan 10 s, an inquiry with LE scan
10 s, LE advertising 3 s, two 5 s page tests), then the loop.

While it reads, the default (`--watch events`) shows BOOT, INQ done, LE,
PAGETEST, CONNTEST, SUMMARY, ERROR and GAMEPAD-LIKE lines, with addresses
hidden. `--watch all` shows every line as it is (with addresses).
`--devices` lists every device heard, with addresses and names: keep that
output private. `--json` gives the report as JSON. Exit code 0 = fine,
1 = errors or a panic, 2 = no probe lines.

To find the pad: start the session, put the pad in pairing mode about
30 s later (so it is still in pairing mode when the loop starts; in one
run it stopped answering after about 30-40 s), and look for `GAMEPAD-LIKE`
lines. A good result looks like:

```
FOUND round=3 new=1 addr=<hidden> cod=0x000508 major=0x05(peripheral) ... eir_name="<the pad's name>" GAMEPAD-LIKE
```

and, at the end, a `GAMEPAD-LIKE devices:` list with `cod 0x000508` (or
0x002508). Each round the loop also pages each gamepad-like device once
for its name (a `NAME ... GAMEPAD-LIKE` line): that is the check the first
run could not finish in the `dual` build.

## What a real classic HID host still needs

This test only found the pad. To use it as a controller, the console still
needs, roughly in this order:

1. **One HCI path for two hosts.** trouble-host owns the radio's HCI today.
   Classic needs its own commands and events (inquiry, connect, ACL data).
   Something must share the one transport: pass LE events and LE
   commands to trouble-host, classic ones to our code, and split the
   command credits.
2. **ACL link**: Create Connection to the pad (or accept its connection),
   handle Connection Complete and Disconnection Complete. The probe has
   done this once with a phone (3 s, then closed), not with the
   pad, and never for longer than 3 s. A pad that pages the console to
   reconnect sends a Connection Request, which the probe now refuses.
   Credits for outgoing data (Number of Completed Packets) are not handled.
3. **Pairing and security**: Secure Simple Pairing (IO capability "no
   input, no output", "just works"), the link key events, and
   authentication and encryption on the link. HID devices expect it.
4. **Store link keys** in flash (one per paired pad), and on start page the
   known pad, or accept its page, so it reconnects without pairing again.
   A pad that is not in pairing mode does not answer an inquiry.
5. **L2CAP** (basic mode is enough): signalling channel, connect and
   configure two channels, PSM 0x11 (HID control) and PSM 0x13 (HID
   interrupt). Our outgoing ACL packets must fit the pad's buffers; keep
   them small (a few hundred bytes at most).
6. **HIDP**: read the input reports on the interrupt channel. Many pads
   also need an output report to start sending (for example the Switch
   "Pro Controller" mode). The report layout depends on the pad's mode;
   the SDP record (PSM 0x01) holds the HID descriptor if we want to read
   it, or we hard-code the layout for known pads.
7. **Air time**: no inquiry while playing (it cuts LE to a tenth). Use
   sniff mode or a short poll interval as the pad asks.
8. **Memory**: the measured cost (+10.5 KB IRAM, +3.8 KB heap, +46 KB
   flash) plus our own L2CAP/HID code and buffers.

No GPL or BTstack code may be copied for this; the Bluetooth spec and
ESP-IDF (Apache-2.0) can be read.

## Not done here

- Seeing "PocketMe probe" from a phone: the firmware sets it up and the
  controller reads it back, but no phone looked for it.
- Pairing, encryption, L2CAP or HID. (A short ACL link to a
  phone was opened and closed in both the LE and the dual build, see
  `CONNTEST peer` above; a link to the pad was not tried.)
- A page to the pad in the `dual` build (the pad had left pairing mode).
- Dual mode with 0 classic links (ESP-IDF refuses 0; not tried).
- `sdk_config_set_bt_pll_track_enable` (ESP-IDF sets it; the flag is on by
  default, so it was left out).
