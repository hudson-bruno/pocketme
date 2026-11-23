# boardctl

The PC end of the console's debug link: screenshots to PNG, injected
buttons and taps, stats, and the plain log, over the USB serial cable.
Made to be run by scripts and agents: no terminal needed, one job per
call, ends by itself.

```sh
cd tools/boardctl
cargo build --release          # target/release/boardctl
cargo test --release           # 8 tests, no board
```

The board must run a firmware that speaks the debug link, for example
`experiments/screen-mirror`. All commands, the speeds that were measured
and the protocol are in `crates/debug-link/README.md`. `boardctl --help`
and `boardctl <command> --help` list every option.

`key` and `release` take `--player N` (1 to 4, default 1): the buttons
go to that player's pad. Player 1 uses the old `Buttons` request, so it
works with every firmware; players 2 to 4 use `PlayerButtons`, which a
firmware from before four players refuses as unsupported.

Three things to know:

* **The port lock is not taken by boardctl.** Where the board is shared,
  wrap every call: `flock <lockfile> timeout 150 boardctl ...`.
* **Opening the port does not reset the board.** boardctl leaves the DTR
  and RTS lines as the system sets them (both on when the port opens,
  both off when it closes, and the board only resets when they differ).
  Checked on the board: `uptime_ms` in `stats` kept counting across 167
  calls in a row. `boardctl reset` and `boardctl log --reset` are the
  only commands that restart it (DTR off with RTS on for 100 ms, then
  RTS off).
* **GPS sentences are never printed.** A GPS receiver may share the
  serial wires; `log` drops every line that looks like `$GPxxx,...` and
  only says how many it hid.

The port is `/dev/ttyUSB0` unless `--port` says otherwise.

## The CYD's scripted finger and measuring letters

`boardctl touch SCRIPT` plays a finger on the CYD firmware
(boards/cyd), for measuring: one line per step, the time in ms from the
start, then `down X Y`, `move X Y`, `up` or `noise N` (panel pixels;
noise of up to N pixels on every reading). It ends with the finger up.
`--log` prints the board's log meanwhile, `--then LETTERS` sends letters
at the end (`i` logs the touch counters, `m` the measurements), and
`--after-ms` says how long to keep printing.

```text
# a slow drag left along the home row, then still, then up
0    down 280 80
33   move 260 80
66   move 240 80
500  up
```

The firmware reads these as plain text letters (`T280,80;`, `TU;`,
`TN2;`), not as link frames, so the link's protocol is unchanged. The
finger's readings go through the same filter as the touch chip's.

`boardctl log --send LETTERS --seconds S` sends letters and prints what
the board logs for them (a measuring letter such as `l` or `x` logs its
result at once, which `text` alone would not show).
