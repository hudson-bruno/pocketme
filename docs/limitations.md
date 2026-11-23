# Limitations

This is what the current PocketMe build cannot do, or has not been shown
to do. Each item says why and, where the sources name one, what would
lift it. Items that were planned and shelved are listed in
[decisions.md](decisions.md#what-was-shelved).

- [Hardware](#hardware)
- [Console](#console)
- [Games](#games)
- [Gamepad](#gamepad)
- [Tools and tests](#tools-and-tests)
- [Where this comes from](#where-this-comes-from)

## Hardware

- **One usable button.** The board has BOOT and RESET, and only BOOT
  can be read. ROOFTOP and the two prototypes need the Bluetooth
  gamepad; the other games have touch controls, some of them simpler
  than the pad's (see [hardware.md](hardware.md#touch-and-the-boot-button)).
- **520 KB of RAM and no external RAM.** Every slot, buffer and stack
  is carved out of that. With the gamepad, 8,260 bytes of instruction
  RAM are left below the game code slot (8,204 with two cores); new
  code that must run from RAM has almost nowhere to go.
- **Resistive touch with a guessed calibration.** The mapping from raw
  readings to screen pixels is still the first guess, and the source
  marks it unproven in landscape. Whether short taps stay taps and
  whether a drag feels slow on the real panel is left for a finger
  test.
- **No tearing-effect pin.** The firmware has to read the panel's
  current line over SPI and time each send around it. Windows that
  cross the drawing line ("crossed", a possible tear) showed up now and
  then around game start and leave on one core: 12 in 204
  start-and-leave cycles in one board run. The cause is not found. With
  two cores (the default) the board runs showed none.
- **The panel is not 60 Hz.** It refreshes about 58.7 times a second
  (measured on the board), so a game at "60" that changes the whole
  width every frame is held to the panel's pace (BENCH showed about
  58.8).
- **No sound.** The board traits have an optional `Audio` part behind a
  `sound` feature, but only the empty `Absent` stand-in implements it
  and the firmware has no sound feature. Several games list sound as
  future work.
- **Power is not measured.** There was no meter, so battery life and
  the cost of the second core are unknown. The longest continuous run
  with two cores was 10 minutes of soak plus about 20 minutes of other
  tests.
- **Light and button feel are unchecked.** The BOOT button's bounce and
  how the five brightness steps look (spacing, flicker, whine) are for
  a person to check; only the chip side was measured.

## Console

- **A game panic stops the chip.** The panic prints "game aborted:
  panic, source line N" and the chip halts with interrupts off until
  the RST button. esp-hal turns every watchdog off at start, so nothing
  restarts it. The console-core README lists five steps that would
  bring a panicking game back to the home screen (a recovery point
  saved around each game call); they are not built.
- **No memory guard.** The ESP32 has none between a game and the
  console. A game that writes over the console's memory, never returns
  from its update, or uses up the stack can break or hang the console.
  The guard today is the compiler: games have no `unsafe`.
- **Little stack for a game.** The console's stack is 29,200 bytes and
  the console used 16,752 of it at its deepest, which leaves about
  12,400 bytes for a game (measured on the board; the game limits drawn
  from it are estimates). The game build refuses functions with large
  frames, but frames add up along a call chain and the check reads each
  one alone.
- **One video mode.** Only mode 0 (160x120, one byte per pixel) is
  offered. Video hints and colour settings are kept but change nothing
  yet.
- **Fixed sizes in the menus.** At most 32 games, names of 12 letters,
  tags of 16, an `INFO.TXT` read up to 192 bytes, paths of 64 bytes.
- **Few saved settings.** The store keeps the brightness and the boot
  animation choice. There are no saved games.
- **A settings record number near 4,294,967,295 stops saves.** It cannot
  come from saving (that is about 4 billion saves), only from rubbish
  in the area with a valid CRC by chance. The fix needs a format
  change.
- **An empty `GAME.BIN` is not listed.** A game file of size 0 does not
  show at all, with no notice.
- **The microSD card is read only.** The console never writes the card.
  Games have not been loaded from a real card on the board; the loader
  path was tested with the built-in demo card.
- **One core is frozen.** A build without the second core still
  compiles and is checked, but since 2026-11-19 it gets no tuning and
  no board time.

## Games

- **Fixed slots.** Code gets 56 KB of instruction RAM and data 48 KB.
  Code may run on into the data slot, but code past the slot,
  constants, starting values and zeroed variables together must fit in
  48 KB. MERCATOR had 2,816 bytes of the slots left on 2026-11-23.
  Moving a slot means rebuilding every game.
- **Code slot access is by whole words only.** A byte read from
  instruction RAM stops the chip, so constants and strings must live in
  the data slot.
- **Flash parts are tied to one firmware.** A game that keeps part of
  itself in the firmware's flash (only MERCATOR) starts only when the
  firmware holds that part byte for byte; otherwise it shows FLASH PART
  MISSING or FLASH PART DIFFERS. A new MERCATOR build needs a new
  firmware. Code run from flash is slower: from flash the planet's
  inner loops took twice the time, so they went back to RAM. Installing
  games from the card into flash is not built.
- **No heap, no floating point, no `unsafe` in games.** This is by
  design: a game depends only on `game-kit` and the shared game
  libraries.
- **Two titles are prototypes.** DRIVE3D and PROTO3D carry working
  names and are not finished games. BENCH is a speed test, not a game.
- **Overflow checks are off for CAMBER.** There a sum that overflows
  wraps round on the chip and shows as a wrong picture instead of a
  stop. The PC tests keep the checks on.

## Gamepad

- **One radio pad.** The Bluetooth gamepad is always player 1. Players
  2 to 4 exist in the game interface but, on the board, only as cable
  pads driven over the debug link.
- **Pairing needs another firmware.** The console firmware never
  pairs. A pad is paired by flashing a separate test firmware, then a
  normal build again. Pairing from the console was planned and not
  built.
- **Link keys are not encrypted.** The saved pairing sits in plain
  bytes in the first 8 KB of the `nvs` partition. The console only
  reads it and logs a checksum, never the bytes.
- **The switch-off fix is untested with the real pad.** A pad switched
  off once stayed "connected" for about 20 s. The fix (a 2 s link
  supervision timeout and a 1 s silence guard) was tested on the PC and
  with an injected pad only. What the controller does with the pad's
  call while it still holds the old link is not known.
- **The silence guard is shorter than the supervision timeout.** At the
  edge of range a pad may go quiet for more than 1 s while the radio
  still holds it; the console then drops it and it has to call again.
  The longest real gap seen nearby was 366 ms. Not seen yet.
- **An unbounded radio queue.** The radio driver keeps each received
  packet in its own heap box until the loop reads it, with no limit.
  With about 10 KB of heap free and 100 reports a second, a loop that
  does not look for about 1 s or more could run the heap out and stop
  the chip. Nothing in the normal build holds the loop that long; a
  screenshot over the cable holds it about 0.8 s.
- **A copied address can drop the link.** Someone who copies the saved
  pad's address and has a radio nearby can make the console drop the
  real pad's link. The caller fails the key check and no key leaks; the
  pad calls again by itself or after a key press.
- **Two cores and a real pad in play: not measured.** The second
  core's long runs used the injected pad. The frame-rate runs of
  2026-11-18 had a gamepad connected, but their input came from scripted
  keys, and the runs of 2026-11-23 had none connected. Whether the
  radio's traffic makes some screen windows late is not checked.
- **One button is missing in three modes.** In a live test on
  2026-11-16 the pad's C (screenshot) button showed nothing in XInput,
  Android and DInput modes.

## Tools and tests

- **The simulator is not the chip.** It cannot judge memory, speed,
  tearing, the radio, real touch, the card or colours. Those are only
  measured on the board. It also has no touch filter, so a quick flick
  ends one tile further on the board than in the simulator.
- **Some tests pass without checking.** The simulator's tests on the
  built chip game files print a note and pass when the game files were
  not built first. The full PC check script builds them.
- **The PC check does not cover every crate.** `crates/debug-link`,
  `test-card`, `pad-proto`, `pad-view` and `pad-store` are tested by
  hand in their own folders. They also use the default toolchain,
  not a pinned one.
- **The firmware check is long.** `boards/cyd/check.sh` builds 154
  feature combinations and takes about 22 minutes. It never touches the
  board.
- **A second-core hang is checked on the PC only.** After 500 ms the
  console stops with a message; a test build exists for the board, but
  the case has not been seen there.
- **The first-frame time in the log is only right for button starts.**
  A game started by a tap shows the time since the last button press.
- **One board.** Every board measurement comes from a single
  prototype, often from one run, and runs of the same build differ:
  five-minute tours of one build showed 33 to 35 late windows and 6 to
  9 crossed.

## Where this comes from

- [boards/cyd/README.md](../boards/cyd/README.md), sections "Not done
  yet", "Still unknown", "A pad switched off and on", "With the host",
  "Settings in flash" and "Game flash parts"
- [boards/cyd/src/touch.rs](../boards/cyd/src/touch.rs)
- [crates/console-core/README.md](../crates/console-core/README.md)
- [crates/board-api/README.md](../crates/board-api/README.md)
- [crates/game-api/README.md](../crates/game-api/README.md)
- [crates/pad-store/README.md](../crates/pad-store/README.md)
- [crates/pad-view/README.md](../crates/pad-view/README.md)
- [crates/touch-filter/README.md](../crates/touch-filter/README.md)
- [games/README.md](../games/README.md)
- [games/build-chip.sh](../games/build-chip.sh)
- [boards/desktop/README.md](../boards/desktop/README.md)
- [boards/cyd/check.sh](../boards/cyd/check.sh)
- [hardware.md](hardware.md)
- [decisions.md](decisions.md)
