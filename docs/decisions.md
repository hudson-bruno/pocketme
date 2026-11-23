# Design decisions

The choices that shaped PocketMe, each with the reason behind it and,
where one exists, the measurement that settled it. Dates are the days
the choice was made or confirmed on the real board.

The project had one rule about decisions: anything a player or a game
author would notice (video modes, the input set, slot sizes, one core or
two, how menus look) was decided by the project's author on measured
options, never settled silently in code. That is why many entries below
end with a number.

- [The shape of the code](#the-shape-of-the-code)
- [The picture](#the-picture)
- [Games](#games)
- [Input](#input)
- [The chip's two cores](#the-chips-two-cores)
- [Memory and flash](#memory-and-flash)
- [Things left out on purpose](#things-left-out-on-purpose)
- [What was shelved](#what-was-shelved)

## The shape of the code

### Bare-metal Rust, no operating system

The console is one `no_std` Rust program on the chip. There is no heap
in the console's core, no floating point, and no `unsafe` in the
console's core or in any game. On a chip with 520 KB of RAM and no
memory protection the compiler is the only guard there is, so the
project leans on it: a game that compiles cannot touch hardware, cannot
allocate, and cannot corrupt the console by a type error.

### The simulator came first

The same console code runs on the PC as a simulator
([boards/desktop](../boards/desktop/README.md)). Everything that does not need the real chip (menus,
games, the picture diff, input rules) is written and tested there, with
simulated time, so a run gives the same pictures every time.

The simulator is deliberately not an emulator of the ESP32. It cannot
judge memory, speed, tearing, the radio or real touch. Those were always
measured on the board; a PC result was never reported as a board result.

### Small board traits, one step function

A board is a handful of small traits (`Screen`, `Touch`, `Keys`, `Pad`,
`Clock`, `Files`, ...) in [crates/board-api](../crates/board-api/README.md).
The console is one plain struct, and the board calls
`Console::step(&mut board)` and is told when to call again. The console
never waits and reads the time once per step.

Why: a console that owns its own loop and its own delays cannot be
tested deterministically and cannot share a core with a radio. A step
function can do both, and it ports to a new board by writing the traits
again and nothing else.

### One game source, two worlds

A game is one `no_std` library crate. The simulator links it in
directly; for the chip the same source is built as a separate file and
loaded into fixed memory slots. There is no second "PC version" of any
game to drift away from the real one.

### Every big part can be left out

Bluetooth, the saved settings, the debug link, the second core, the boot
animation and the game flash parts are each behind a build feature
([the feature table](../boards/cyd/README.md#features)). With so little
RAM, a part that cannot be switched off cannot be measured either: the
cost of each one is the difference between two builds.

### No root Cargo workspace

Every crate stands alone with its own `Cargo.lock` and is built and
tested from its own folder. A workspace would pull in the old crates
and the experiments, and the firmware needs a different toolchain and
target.

## The picture

### 160x120, one byte per pixel, on a 320x240 panel

The console keeps a 160x120 picture with a 256-colour palette and shows
each picture pixel as 2x2 panel pixels. A full 320x240 picture at 16
bits per pixel would be 153,600 bytes; the chip has 520 KB of RAM in
total and no external RAM. The small picture is 19,200 bytes, which
leaves room for a second copy to compare against.

### Send only what changed, in at most 16 rectangles

The console compares the new picture with the one on the panel and sends
the difference as at most 16 rectangles
([crates/console-video](../crates/console-video/README.md)). The limit
was chosen from board timings: one rectangle costs about 48 to 50
microseconds of set-up on the wire, so 16 cost at most about 0.8 ms of a
33 ms frame, and more rectangles mean fewer unchanged pixels sent.

### Games never bring their own screen code

A game draws into the picture the console hands it and picks from what
the console offers. It never talks to the panel. That keeps every game
working on another board with another screen, and keeps the timed,
tear-free send in one place.

### No tearing: follow the panel, do not fight it

The panel has no tearing-effect pin on this board, so the firmware reads
the panel's current line over SPI (the Get Scanline command) and sends
each changed rectangle behind or ahead of the line being drawn. The
panel refreshes about 58.7 times a second (measured on the board), not
60, so a game at 60 that changes the whole width every frame is held to
the panel's pace.

### 30 pictures a second is the target

The console's default is 30 pictures a second, and games offer 60
where they have the room. Since 2026-11-23 every game with a 30 or 60
setting starts at 60 (the author's decision); STARDUST, DRIVE3D and
PROTO3D have no 60 setting and stay at 30. The highest detail level (MAX) of PROTO3D and
DRIVE3D is sized for an average of 24 pictures a second (41.67 ms), the
author's decision of 2026-11-22; that is an average, not a ceiling.

### Player-facing options show every choice

When a setting is switchable while playing (detail level, frame rate,
camera), all the options stay in the menu. The measured best one is only
the default.

## Games

### Fixed memory slots

A game is loaded into two fixed places in RAM: a code slot of 56 KB in
instruction RAM and a data slot of 48 KB
([crates/game-api](../crates/game-api/README.md), the only place these
numbers are written). Fixed slots mean no relocation at load time: a
game file is checked (header, CRC-32) and copied, and it runs. Moving a
slot means rebuilding every game. The code slot shrank from 64 to 56 KB
on 2026-11-17 to make room for the radio's RAM code, and the data slot
was fixed at 48 KB on 2026-11-19.

### Code may run on into the free data slot

Approved 2026-11-19, passed on the board 2026-11-22. The two slots sit
next to each other in memory, and most games use far less data than
code. A game's code may therefore continue past the end of the code slot
into the unused part of the data slot. Three games use this today.

### A big game may keep part of itself in the firmware's flash

Built 2026-11-22, on the board since 2026-11-23. When a game outgrows
RAM altogether (MERCATOR, with its planet landing), the code and
constants named in its `flash.list` are placed in two reserved windows
of the firmware's own flash and run from there. The game file carries
the checksums of its flash part, and the firmware starts the game only
when it holds that part byte for byte.

This is a stop on the way, not the end of the road: installing games
from the SD card into flash is not built (see below).

### A shared library compiled into each game

Four games (KESTREL, CAMBER, PROTO3D and DRIVE3D) share
[crates/game-3d](../crates/game-3d/README.md) (fixed-point maths, the
triangle filler, sorting, the camera). It is compiled into each game
file, not kept once in the console. A game that does not use it pays
nothing, and one that does gets only the parts it calls.

### Overflow checks stay on

Game builds for the chip keep Rust's integer overflow checks on by
default; a game may turn them off for itself (today only CAMBER, by the
author's decision). With the checks on, a sum that runs over stops the
game and names the source line; with them off it wraps round and shows
as a wrong picture.

### A game panic stops the console

There is no watchdog yet: a game that panics halts the chip until the
reset button. This is a known gap, listed in
[limitations](limitations.md), and it is why game code is checked so
hard on the PC first.

## Input

### A gamepad over classic Bluetooth, inside the console

In the firmware with the real pad since 2026-11-17. The board has one
usable button, so a real pad was needed. The pad the project was built
around speaks classic Bluetooth (not Low Energy), and the Rust Bluetooth
stack available for this chip was Low Energy only, so a classic host was
written from the Bluetooth specification
([crates/pad-proto](../crates/pad-proto), `pad-view`, `pad-store`,
`pad-host`).

### Reconnect only

The console firmware never searches for devices, is never discoverable,
never starts a connection and never pairs. It only answers a pad that
was paired before. Two independent layers enforce it: an allow-list on
every packet sent to the Bluetooth controller
([boards/cyd/hci-rules](../boards/cyd/hci-rules)) and the protocol
state machine's own configuration one layer up. A console in a pocket
should not be something a stranger's device can talk to.

Pairing a pad is done by a separate test firmware. Pairing from the
console's own menu was planned and not built.

### Four players in the interface

Decided 2026-11-18. The game interface carries four players from the
start, because adding a player later would change every game's contract.
On this board the touch screen can stand in as a second player.

### Touch rules

Decided 2026-11-17, after trying them on the resistive panel: a finger
missing for one frame is still the same touch (a "grace" frame, so a
lost contact is not a second tap), a touch stops a list that is still
gliding, and a press held longer than 500 ms gives no tap, so a resting
finger is not a tap.

## The chip's two cores

### Two cores by default

Chosen 2026-11-18. The first core runs the console, the game and the
radio; the second core does nothing but send each finished picture to
the panel. Measured on the board with a gamepad connected, one core
against two: HAIRPIN at its heavy setting went from 45.7 to 58.7
pictures a second, and windows that crossed the panel's drawing line (a
possible tear) went to 0 in every run.

The hand-over between the cores is its own crate
([boards/cyd/handover](../boards/cyd/handover)), tested on the PC
against a model of the second core in every order of events, and every
wait in it is bounded: after 500 ms the firmware panics with a message
instead of hanging.

### One core still builds, and gets no more work

The one-core build is kept compiling and checked by the scripts, and its
behaviour must not change when the two-core path does. Since 2026-11-19
it gets no tuning and no board time.

## Memory and flash

### Flash at 80 MHz

Adopted 2026-11-22. The chip reads its program flash at 80 MHz instead
of the default 40 MHz. Code that runs from flash and misses the cache
waits for that read, so the faster clock is a direct gain for everything
not held in RAM.

### The timed screen code sits in one block of flash

The functions on the timed path to the panel are linked as one block
placed after all other flash code, so that no three of its cache lines
can land in the same flash cache set, whatever the linker does with the
rest of the program. The firmware check runs a script on seven of its
builds (the ones meant for use, with the gamepad, and one measuring
build) to check it. The
story and the numbers are in
[the firmware README](../boards/cyd/README.md#the-timed-paths-and-the-flash-cache-2026-11-18).

### Settings live away from the pairing

The saved settings (brightness, the boot animation choice) are written
to their own two flash sectors, through a store that survives a power
cut at any moment
([crates/settings-store](../crates/settings-store/README.md)). The
firmware checks every write and erase against that range. The saved
gamepad pairing is in another partition that the console never writes.

### Big values never go on the stack

The console struct (about 45 KB) is a static that must be all zero
bytes, so it costs no flash; the gamepad host (about 19.7 KB) is placed
on the heap once and never moved. (45,176 and 19,704 bytes, read from
the firmware built on 2026-11-23.)

## Things left out on purpose

- **No operating system and no standard library** on the chip.
- **No heap, no floating point, no `unsafe` in games.** A game depends
  on `game-kit` and, if it draws in 3D, on `game-3d`; on nothing else.
- **No searching, pairing or advertising** in the console firmware.
- **No writes to the SD card.** The card is read only.
- **No copied code** from GPL sources or from BTstack. The Bluetooth
  host is written from the specification.

## What was shelved

These were studied or planned and are not in the build:

- installing games from the SD card into flash (game code that comes
  with the firmware does run from flash: MERCATOR's flash part, on the
  board since 2026-11-23);
- saved games;
- pairing a gamepad from the console.

See also [limitations](limitations.md) for what the build that exists
cannot do.
