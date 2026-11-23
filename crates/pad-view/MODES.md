# GuliKit KingKong 2 Pro: what it sends in each mode

The GuliKit KingKong 2 Pro (also sold as "KK2 Pro", model NS09) has four
modes: XInput (Windows), DInput (Windows), Android/iOS (also for macOS)
and Switch. This file collects what is known about each mode over
Bluetooth, so that `pad-view` can decode it.

Every fact is marked:

* **[source: ...]** - read in a named source;
* **[measured]** - seen on our own board with `experiments/bt-classic-probe`;
* **[guess]** - an unchecked guess. Do not build on it without checking.

The real pad may differ from all of this. `pad-view` never depends on it
being right: when the pad sends something else, the view falls back to
the pad's own HID report descriptor, and when even that is missing, it
shows the raw bytes.

## What is the same in every mode

* The pad speaks classic Bluetooth (BR/EDR) only. None of the four modes
  showed up in a Bluetooth Low Energy scanner **[measured, by the owner]**.
  The only FCC filing under NS09 (the first KingKong Pro, 2019) lists
  classic Bluetooth only **[source: FCC ID 2AQNP-NS09 test report]**.
* It is a classic HID device: two L2CAP channels, control (PSM 0x11) and
  interrupt (PSM 0x13). Input reports arrive on the interrupt channel
  with a 0xA1 byte in front; output reports go out with 0xA2 in front
  **[source: Bluetooth HID Profile 1.1]**.
* Change mode: hold the Mode button 2 seconds or more. Pairing: hold the
  pairing button 2 seconds, until the player lights scroll **[source:
  GuliKit NS09 English manual, archived copy on manuals.plus]**. The order
  of the modes is said to be XInput, Switch, Android/iOS, DInput
  **[source: retrocompass.web.app guide, unofficial]**.
* It stays in pairing mode only about 30 to 40 seconds **[measured]**.
* Report rates measured over Bluetooth on Windows: about 42 per second in
  Switch mode, 37 in XInput, 36 in DInput **[source: gamepadla.com,
  GuliKit NS09 test]**.

## Mode 1: XInput (Windows)

| What | Value |
|---|---|
| Bluetooth name | "GuliKit Controller XW" **[measured]** **[source: xpadneo issue #391]**. The manual calls it "GuliKit Controller" **[source: manual]**. |
| Class of device | 0x000508: peripheral, gamepad **[measured]** |
| Vendor / product | 045E:02E0 (Microsoft, Xbox One S model 1708 on classic Bluetooth) **[source: xpadneo issue #391, kernel log]**. That this comes from an SDP Device ID record with vendor id source 2 (USB) is **[guess]**. |
| Report descriptor | 307 bytes, the Xbox One S 1708 classic descriptor **[source: xpadneo issue #391; xpadneo docs/descriptors]**. It is in `tests/data/mod.rs` as `GULIKIT_KK2_ANDROID`. |
| Start-up needed? | No. Reports should flow as soon as the interrupt channel is open: SDL's Bluetooth Xbox code only reads report 0x01, 0x02 and 0x04 and writes nothing but rumble **[source: SDL_hidapi_xboxone.c, my reading of it]**. |
| Motion sensors | None in this mode: the descriptor declares none **[source: the descriptor]**. |
| Rumble | Output report 0x03, 9 bytes with the id: which motors (0x0F = all four), left trigger, right trigger, left and right motor strengths, then time on, time off, repeat count (SDL sends `03 0F lt rt lo hi FF 00 EB`) **[source: the descriptor; SDL_hidapi_xboxone.c, rumble packet]**. pad-view does not send it. |

Input reports (the first byte is the report id):

| Report | Bytes | Layout |
|---|---|---|
| 0x01 | 16 | 1-2 left stick X, 3-4 left stick Y, 5-6 right stick X, 7-8 right stick Y (each 0..65535, centre 32768, Y grows downwards); 9-10 left trigger, 11-12 right trigger (0..1023); 13 hat (1 up, 2 up-right, ... 8 up-left, 0 centred); 14 buttons A 0x01, B 0x02, X 0x04, Y 0x08, LB 0x10, RB 0x20, View 0x40, Menu 0x80; 15 left stick click 0x01, right stick click 0x02 **[source: SDL_hidapi_xboxone.c, HandleButtons16 and HandleStatePacket; the descriptor]** |
| 0x02 | 2 | The Xbox (Home) button alone, bit 0x01 **[source: the descriptor; SDL]** |
| 0x04 | 2 | Battery, one byte **[source: the descriptor; SDL]** |

Newer real Xbox pads send a 17-byte report 0x01 with a different button
order (A 0x01, B 0x02, X 0x08, Y 0x10, LB 0x40, RB 0x80; byte 15: View
0x04, Menu 0x08, Xbox 0x10, stick clicks 0x20 and 0x40; byte 16 bit 0x01
Back or Share) **[source: SDL_hidapi_xboxone.c, HandleButtons]**. pad-view
tells the two apart by the report length, as SDL does.

## Mode 2: Android/iOS (also macOS)

| What | Value |
|---|---|
| Bluetooth name | "Xbox Wireless Controller" **[source: manual, "choose Xbox Wireless Controller"]** |
| Class of device | 0x000508 **[guess]**, as in XInput mode |
| Vendor / product | 045E:02E0 **[guess]**: the descriptor is byte for byte the same as the GuliKit ES Pro's, which reported 045E:02E0 **[source: xpadneo docs/descriptors, gulikit_kingkong2_android.md and gulikit_esports_pro.md]** |
| Report descriptor | The same 307 bytes as XInput mode **[source: xpadneo docs/descriptors/gulikit_kingkong2_android.md]** |
| Reports | The same as XInput mode **[source: same descriptor]** |
| Start-up needed? | No **[source: SDL, as above]** |
| Motion sensors, rumble | As XInput mode **[source: same descriptor]** |
| Warning | xpadneo's author wrote "Android mode didn't pair for me" (with a Linux PC) **[source: xpadneo docs/README.md]** |

So XInput and Android/iOS modes look the same on the wire. Only the name
tells them apart. pad-view's "mode guess" uses the name for that.

## Mode 3: Switch

| What | Value |
|---|---|
| Bluetooth name | "Pro Controller" **[source: xpadneo issue #391, bluetoothctl output]** |
| Class of device | 0x002508: peripheral, gamepad, with the "limited discoverable" bit **[source: xpadneo issue #391]** |
| Vendor / product | 057E:2009 (Nintendo Switch Pro Controller) **[source: xpadneo issue #391, modalias]** |
| Bluetooth address | Starts with a Nintendo prefix (98:B6:E9) in this mode **[source: xpadneo issue #391]** |
| Report descriptor | That of a real Pro Controller (170 bytes, in `tests/data/mod.rs` as `SWITCH_PRO`, from Android's test files) **[guess]** that the GuliKit copies it exactly |
| Start-up needed? | Yes, for full reports. See below. |
| Motion sensors | Yes: a 6-axis sensor (accelerometer and gyroscope) in report 0x30, three samples per report, after subcommand 0x40 with argument 0x01 **[source: dekuNukem Nintendo_Switch_Reverse_Engineering, bluetooth_hid_notes.md and imu_sensor_notes.md]**. That the KK2 Pro has a gyroscope: its firmware V5.1 notes talk about "gyro stability" **[source: gulikit.com firmware page]**. |
| Rumble | In bytes 2-9 of every output report 0x01 or 0x10 **[source: dekuNukem]**. The neutral value (motors still) is `00 01 40 40 00 01 40 40`. SDL notes: "The KingKong2 PRO Controller doesn't initialize correctly with a 0 amplitude value over Bluetooth", so the neutral value must be used, not zeros **[source: SDL_hidapi_switch.c, SetNeutralRumble]**. |

Input reports:

| Report | Bytes | Layout |
|---|---|---|
| 0x3F | 12 | The "simple" report, sent before the host asks for more. 1 buttons B 0x01, A 0x02, Y 0x04, X 0x08, L 0x10, R 0x20, ZL 0x40, ZR 0x80; 2 Minus 0x01, Plus 0x02, left stick click 0x04, right stick click 0x08, Home 0x10, Capture 0x20 (some pads put ZR at 0x80 here too); 3 hat 0 up ... 7 up-left, 8 centred; 4-11 four 16-bit stick values, centre 0x8000, Y grows downwards **[source: SDL_hidapi_switch.c, HandleSimpleControllerState; dekuNukem]**. That it is sent only when a button changes is **[guess]** (dekuNukem says so for the Joy-Con). |
| 0x30 | 49 | The full report. 1 timer; 2 battery (top 4 bits, 8 = full) and connection info; 3 right buttons Y 0x01, X 0x02, B 0x04, A 0x08, R 0x40, ZR 0x80; 4 Minus 0x01, Plus 0x02, right stick click 0x04, left stick click 0x08, Home 0x10, Capture 0x20; 5 left buttons Down 0x01, Up 0x02, Right 0x04, Left 0x08, L 0x40, ZL 0x80; 6-8 left stick and 9-11 right stick, each two 12-bit numbers packed in 3 bytes, Y grows UPWARDS; 12 vibration report; 13-48 three motion samples of 12 bytes (accel x y z, gyro x y z, 16-bit signed) **[source: dekuNukem bluetooth_hid_notes.md; SDL inverts Y]** |
| 0x21 | 49 | Answer to a subcommand: bytes 1-12 as in 0x30, 13 acknowledgement (top bit set = done), 14 the subcommand id it answers, 15.. data **[source: dekuNukem]** |

The start-up conversation (output report 0x01, "rumble and subcommand"):

```text
byte 0     0x01
byte 1     packet counter, 0 to 15, one more for every packet sent
bytes 2-9  rumble: 00 01 40 40 00 01 40 40 (motors still; SDL), or all
           zero (Linux, Bluepad32)
byte 10    subcommand id
byte 11..  arguments; SDL pads with zeros to 49 bytes, Linux and
           Bluepad32 send only the bytes used (11 for device info, 12
           for a subcommand with one argument)
```

**[source: dekuNukem bluetooth_hid_notes.md; SDL_hidapi_switch.c;
Bluepad32 and the Linux hid-nintendo driver, read to learn]**

**Measured 2026-11-16 (Switch mode, first live test):** the GuliKit never
answered our first way (03 30 sent at once, 49 bytes, neutral rumble, on
the interrupt channel), three tries of each step. A PC replay test checks
every header of those packets (ACL handle and flags, lengths, the pad's
interrupt channel 0x0043, the 0xA2 byte): they are well formed, with the
same flags as the signalling and SDP packets the pad did answer. Counting
the HCI events in the log, our controller completed every one of them
(Number Of Completed Packets), so they went on the air. Which detail the
pad wants is not known; the hosts that work with it differ in these:

* SDL, Bluepad32, BlueRetro and Linux all send device info (0x02) first.
* BlueRetro waits about 300 ms after the interrupt channel opens; Linux
  sends about 4 ms after an input report.
* SDL: 49 bytes, neutral rumble, waits 100 ms for an answer, 5 tries.
  Linux and Bluepad32: short packets, zero rumble.

So pad-view now probes. It waits for the first input report plus 300 ms
(or 1 s after the HID channels open, if no report comes), then tries
these ways of talking, in order, three times each, 300 ms apart:

| Variant | Subcommand | Length | Rumble | Channel / timing |
|---|---|---|---|---|
| `info-49` | 02 | 49 | neutral | interrupt |
| `info-short-zero` | 02 | 11 | zero | interrupt |
| `info-short` | 02 | 11 | neutral | interrupt |
| `info-49-zero` | 02 | 49 | zero | interrupt |
| `info-after-report` | 02 | 49 | neutral | interrupt, right after an input report |
| `info-setreport` | 02 | 49 | neutral | control channel, HID SET_REPORT (0x52) |
| `mode-49` | 03 30 | 49 | neutral | interrupt (our first way, after the settle) |

The first variant the pad answers wins: a 0x21 naming its subcommand, or
reports 0x30 starting after `mode-49`. An answer is credited to the newest
variant that sent that subcommand. If an older variant sent the same
subcommand less than 600 ms before the answer, the win is unclear: when
no step is then answered the winner's way, the older variant takes over
once and the steps start again.

After a win, the steps go out the winner's way, one at a time, in SDL's
and Bluepad32's order, each up to three times:

1. `03 30`: "send report 0x30 from now on" (full reports, 12-bit sticks).
   Reports 0x30 arriving count as its answer.
2. `40 01`: motion sensor on, so its numbers show on the Pad and Data
   pages (see "The motion sensor" below).
3. `30 01`: player light 1 on, so the owner can see the pad is talking.

If no variant is answered, the conversation ends and the 0x3F reports are
shown as before. They are shown the whole time anyway. Worst case, from
the first report to the end: 0.3 s + 7 x 3 x 0.3 s + 3 x 3 x 0.3 s =
9.3 s (an unclear win adds at most 0.9 s). The answer to device info
carries the pad's own address (bytes 19-24); it is never logged.

A real Switch also reads the stick calibration from the pad's memory
(subcommand 0x10). pad-view does not: for a test picture the centre
(about 2048) and a round guess of the travel (about 1600 either way) are
close enough **[guess]**. SDL itself keeps Bluetooth Pro Controllers on
0x3F reports by default and asks for 0x30 only when the program wants
sensors **[source: SDL_hidapi_switch.c, GetDefaultInputMode]**.

## Mode 4: DInput (Windows)

Little was published about this mode over Bluetooth; the board's live
test of 2026-11-16 filled in most of it.

| What | Value |
|---|---|
| Bluetooth name | "GuliKit Controller DW" **[measured]** |
| Class of device | 0x002504 (peripheral, joystick) on one connection, 0x002508 (gamepad) on a later one **[measured]**. Why it changed is not known. |
| Vendor / product | 1949:0402, version 011B, from the SDP Device ID record with vendor id source 2 (USB) **[measured]**. Not the USB ids SDL knows for the KK2 Pro (0079:181C, 0079:0122) **[source: SDL_gamepad_db.h]**. |
| Report descriptor | 148 bytes **[measured]**. It is in `tests/data/mod.rs` as `GULIKIT_KK2_DINPUT`. |
| Report 0x07 | 9 bytes: 1 Z and 2 Rz (right stick), 3 X and 4 Y (left stick), each 0..255 with centre 0x80 or 0x7F; 5 a byte with usage 0 (always 0 so far); 6 the hat in the low 4 bits (0 up ... 7 up-left, 0x0F centred) and buttons 1-4 in the high 4 bits; 7 buttons 5-12; 8 eight vendor bits (usage page FF00, usage 1) **[measured: the descriptor, and real reports such as `07 80 7F 80 7F 00 0F 00 00` (idle), `... 0F 04 00` (LT), `... 0F 08 00` (RT), `... 07 00 00` (hat up-left)]** |
| Report 0x02 | 2 bytes: eight consumer keys, one bit each: Play/Pause (bit 0), Scan Previous, Scan Next, Volume Down, Volume Up, Eject, AC Back (bit 6), AC Home (bit 7) **[measured: the descriptor]**. Which buttons send it is not known yet; no report 0x02 was captured in this mode. |
| Button order | Plain: A 1, B 2, X 3, Y 4, LB 5, RB 6, LT 7, RT 8 **[measured: LT and RT]**; the rest (9 to 12) **[guess]**: View 9, Menu 10, stick clicks 11 and 12, as SDL's USB entry 0079:0122 **[source: SDL_gamepad_db.h]** |
| Triggers | Buttons only (7 and 8). The descriptor has no trigger axis **[measured]**. |
| Start-up needed? | No **[measured]**: reports flowed with nothing sent. |
| Motion sensors, rumble | No sensor in the descriptor **[measured]**. Report 0x07 also has an output part of 8 bytes and a feature part of 4 bytes, both vendor usage FF00:2; what they do is not known (rumble is a **[guess]**). |

pad-view decodes this mode from the pad's own report descriptor. If the
Device ID says 0079:181C it uses the Android order above; if the button
numbers look like Android's (no buttons 3 and 6), it uses that order too;
otherwise it uses the plain order and says on screen that the mapping is
a guess. The DW descriptor gets the plain order ("generic HID, guessed").
Because it has buttons for LT and RT and no trigger axis, the Pad page
says "triggers: on/off" and fills a trigger bar fully while the button is
held. AC Home in report 0x02 is the Home button. AC Back is not taken as
"-" here, because button 9 already is "-": it shows as unmapped input
("02 b1.6 AC Back"), as does every vendor bit ("07 b8.0 vendor FF00:1").

## The motion sensor

Only the Switch mode gives motion that pad-view reads. A motion sensor
here is two parts: an accelerometer (it feels gravity and shaking, in g,
where 1 g is the pull of the Earth) and a gyroscope (it feels turning, in
degrees per second).

| What | Value |
|---|---|
| Where | Report 0x30 (also 0x31), bytes 13 to 48: three samples of 12 bytes. Each sample is six signed 16-bit numbers, low byte first: accelerometer x, y, z, then gyroscope x, y, z **[source: dekuNukem bluetooth_hid_notes.md, "3 frames of 2 groups of 3 Int16LE each"]**. In report 0x21 the same bytes hold the subcommand answer, not motion. |
| Turning it on | Subcommand 0x40 with argument 0x01. It also puts the ranges back to the defaults below **[source: dekuNukem bluetooth_hid_notes.md, subcommand 0x40; subcommand 0x41 sets the ranges]**. |
| Accelerometer scale | 4096 counts per g at the default range of +-8 g (0.000244 g per count) **[source: dekuNukem imu_sensor_notes.md; SDL_hidapi_switch.c uses 4096 when it does not read the calibration; the Linux hid-nintendo driver uses 4096 too]**. |
| Gyroscope scale | About 0.070 degrees a second per count at the default range of +-2000 degrees a second **[source: dekuNukem imu_sensor_notes.md recommends 0.070 (the sensor maker's figure) over the plain 0.061; SDL uses 14.2842 counts per degree a second, 0.0700; Linux uses 14.247, 0.0702, with the note ".0610 * 1.15"]**. |
| Time between samples | 5 ms: one report covers 15 ms, so the pad measures 200 times a second while it sends 60 to 120 reports a second **[source: dekuNukem bluetooth_hid_notes.md, "1st sample 0ms, 2nd 5ms, 3rd 10ms"; SDL sends the three 5 ms apart]**. |
| Which sample is newest | Not agreed. dekuNukem's timing and the Linux driver (it gives the first sample the earliest time) make the third the newest. SDL hands the third sample on first, as the oldest. pad-view takes the third as the newest; 5 ms makes no visible difference. |
| Axes | Pad flat, buttons up: x towards the triggers, y to the left, z up out of the buttons. Lying flat it reads about +4096 on z **[source: the Linux hid-nintendo driver's comment on the axes; SDL swaps the axes to the same effect]**. **[guess]** that the GuliKit copies this; the live test should check. |
| No data | All accelerometer numbers zero: the sensor is off or missing (a real accelerometer always feels gravity). SDL decides the same way. The Data page then says "No motion data yet". |
| Calibration | The pad keeps its own zero points and scales in its memory (SPI flash 0x6020 and 0x8026). pad-view does not read them, so a pad lying still may show a few degrees a second on the gyroscope **[source: dekuNukem spi_flash_notes.md; SDL reads them]**. |

What pad-view does with it (`src/motion.rs`): it keeps all three samples
of the last report (36 bytes, no heap), shows the newest one in g and
degrees a second with its raw count, and works out the tilt from the
average of the three accelerometer samples: roll (left or right side
down) and pitch (nose up or down). The tilt comes from gravity alone, so
it is right while the pad is held still and cannot see turning flat on a
table. The screen shows a change at most 10 times a second (4 on the
Data page), and only when a number moved past 0.02 g or about 2 degrees
a second.

Other modes and pads:

* XInput and Android/iOS modes: no motion sensor in the descriptor, and
  Xbox pads have none over Bluetooth **[source: the descriptor; SDL's
  Xbox driver reads no sensors]**. The About page says "not in this
  mode". A pad decoded from its own descriptor (DInput mode) gets "none
  found": nothing is known for sure there.
* A DualShock 4 does have one: in its long report (0x11 over Bluetooth)
  the gyroscope and accelerometer sit in bytes its descriptor marks only
  as vendor data **[source: SDL_hidapi_ps4.c, PS4StatePacket_t]**.
  pad-view does not decode it; for a Sony vendor id (054C) the About page
  says "Sony pads have one; not read here".
* A generic descriptor may declare sensors with the HID "Sensors" usage
  page (0x20). pad-view does not read those either; the About page says
  "listed in descriptor, not read".

## How pad-view picks a decoding

In this order (see `decode::choose`):

1. Vendor id 057E (Nintendo): Switch. Vendor id 045E (Microsoft): Xbox.
2. The name: "Pro Controller" or "Joy-Con": Switch. "Xbox" or
   "Controller XW": Xbox.
3. The shape of the descriptor: reports 0x3F, 0x21 and 0x30 mean Switch;
   report 1 starting with 16-bit X and Y plus a hat numbered 1-8 means
   Xbox; vendor 054C means PlayStation numbering; 0079:181C or
   Android-like button numbers mean Android numbering; anything else is
   decoded from its usages, marked "guessed".
4. With no descriptor at all: the first report (0x3F of 12 bytes, or
   0x30/0x21 of 13 bytes or more: Switch; 0x01 of 16 to 20 bytes: Xbox).

Steps 1 and 2 count only when the descriptor, if there is one, agrees
(it has a report 0x30 or 0x3F for Switch; a report 1 of 16 bytes or more
for Xbox). A pad that borrows a name but describes something else is
decoded by its descriptor.

## Live test, 2026-11-16 (the owner, all four modes)

| Mode | Name, class, ids | What worked | What did not |
|---|---|---|---|
| Switch | "Pro Controller", 0x002508 | Everything. The triggers are on/off only, as on a real Pro Controller. | - |
| XInput | "GuliKit Controller XW", 0x000508, 045E:02E0 version 0903 | Everything but one button | The screenshot button (drawn "C" on the Pad page; on the pad it sits under "-", above the d-pad, right of the left stick) shows nothing. |
| Android/iOS | "Xbox Wireless Controller", 0x000508, 045E:02E0 version 0903 | As XInput | As XInput. |
| DInput | "GuliKit Controller DW", 0x002504 or 0x002508, 1949:0402 version 011B | The rest, as far as the owner said; LT and RT were named as held in the text | The C button showed nothing, but the "total" report counter went up when it was pressed: the pad sends a report for it. The trigger bars stayed empty. |

**[measured, by the owner]**

What was done about it (pad-view, after the test):

* Trigger bars in DInput mode: the pad-view code at the time of writing
  fills them for the real descriptor and reports (render test
  `dinput_pages`, picture `19-dinput-lt-controller.png`). The firmware on
  the board was most likely built before that code **[guess: from the
  build times]**. The decoder now also marks the triggers as present and
  on/off from the descriptor alone, and starts from an empty pad on each
  new descriptor.
* The C button in XInput, Android/iOS and DInput mode: not known which bit
  it is. Two tools now show it on the next test:
  * the unmapped-input tracker (`src/unmapped.rs`): any bit no standard
    control takes (a declared field, padding, or bytes the descriptor
    does not declare) lights the "?" lamp on the Pad page and is listed
    on the Data page as id, byte.bit and name. Bytes past the declared
    size of a report, and report ids the descriptor does not declare,
    count from 0, so they show even when already set in the first
    report;
  * the serial trace (`src/trace.rs`, firmware lines `PAD-HID change`
    and `PAD-HID same`): in the Xbox modes the pad sends only on a
    change, so a report byte for byte the same as the last one, after a
    silence, means a press that has no place in the report. If the C
    button gives only such "same" reports, the pad does not send it in
    that mode at all, and nothing can show it.
  In DInput mode the likely places are a vendor bit (report 0x07 byte 8)
  or a key of report 0x02 **[guess]**.

## What the live test should tell us

* The real name, class of device and Device ID record in each mode
  (DInput above all).
* Whether the Switch mode answers subcommands, which way of talking it
  answers (the `switch_startup=won:<variant>` field of the
  `PAD-SUMMARY view` line), and how fast it then sends report 0x30.
* Whether the XInput and Android/iOS modes really send 16-byte reports.
* Whether the Switch mode answers `40 01` and then fills bytes 13-48 of
  report 0x30. Lying flat, the Data page should show Acc Z near +1.00 g
  and the others near 0; the right grip down should give Acc Y near +1 g
  and the ball rolling right on the Pad page. If a sign or an axis is
  the wrong way round on the GuliKit, `src/motion.rs` is the one place to
  change.
* The DInput mode's report descriptor (the About page shows its length
  and first bytes; the serial log should hold all of it). Done: see
  Mode 4.
* Next test: which bit the C button sets in each mode (press it a few
  times, alone, then read the "button trace" part of `tools/padlog.py`).
