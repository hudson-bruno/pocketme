#!/usr/bin/env bash
# Builds the console, flashes it, listens to it for a while, and says what
# it saw. One command, no terminal needed, safe to run from a script.
#
#   tools/board-run.sh <name> <seconds> [cargo features, comma separated]
#
#   tools/board-run.sh plain 100
#   tools/board-run.sh ble   120 ble
#   tools/board-run.sh sd    120 ble,sd      # WRITES TO THE SD CARD ONCE, see console/src/sd.rs
#   tools/board-run.sh probe 100 probe-byte-read
#   tools/board-run.sh crash 100 crash-test
#   tools/board-run.sh fault 100 fault-test
#
# The log goes to logs/<name>.log (cleaned by tools/serial-filter.py: time
# stamps added, colours and any GPS sentence removed) and a summary of it
# is printed (tools/log-summary.py).
#
# The serial port is shared. Everything that opens it runs under a lock
# file and a time limit, so that two users never talk to the board at once
# and nobody keeps it for good. Set BOARD_LOCK to the lock file the others
# use; the default is good for working alone.
#
# Without a second word, "listen only": tools/board-run.sh listen 60
# resets the board and listens, without building or flashing anything.
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
name="${1:?usage: board-run.sh <name> <seconds> [features]}"
seconds="${2:-100}"
features="${3:-}"
port="${BOARD_PORT:-/dev/ttyUSB0}"
lock="${BOARD_LOCK:-/tmp/native-loader-board.lock}"

export PATH="$HOME/.cargo/bin:$PATH"
# The Xtensa compiler and linker.
. "$HOME/export-esp.sh"

mkdir -p "$here/logs" "$here/console/target/builds"
log="$here/logs/$name.log"
elf="$here/console/target/builds/$name.elf"

if [ "$name" != listen ]; then
    # The game files are part of the firmware, so they must exist first.
    if [ ! -f "$here/game/target/images/HELLO.BIN" ] || [ ! -f "$here/game/target/images/CRASH.BIN" ] \
        || [ ! -f "$here/game/target/images/FAULT.BIN" ]; then
        "$here/build-game.sh"
    fi
    echo "== building the console (features: ${features:-none})"
    (cd "$here/console" && cargo build --release ${features:+--features "$features"})
    # Each build gets a name of its own: the next build with other
    # features would replace the file cargo made.
    cp "$here/console/target/xtensa-esp32-none-elf/release/loader-console" "$elf"
fi

if [ ! -e "$port" ]; then
    # The board is a USB device handed to Linux by Windows. Ask once. The
    # bus id can change between PC starts: `usbipd.exe list` shows it on
    # the line with 1a86:7523 (the CH340); pass it as BOARD_BUSID.
    echo "== $port is missing, asking Windows to attach it"
    usbipd.exe attach --wsl --busid "${BOARD_BUSID:-4-17}" || true
    sleep 3
fi
if [ ! -e "$port" ]; then
    echo "== $port is still missing. Nothing was flashed." >&2
    exit 3
fi

if [ "$name" = listen ]; then
    command=(espflash monitor --chip esp32 --port "$port" --non-interactive)
else
    command=(espflash flash --chip esp32 --port "$port" --monitor --non-interactive "$elf")
fi

echo "== ${command[*]}  (for $seconds s, log: $log)"
# `timeout` ends the listening; that is the normal way out, so its exit
# status is not an error. The filter comes before `tee`: what it drops
# never reaches the disk.
set +e
flock "$lock" timeout "$seconds" "${command[@]}" 2>&1 | python3 "$here/tools/serial-filter.py" | tee "$log" | grep --line-buffered -E "LAST START|SUMMARY|CHECKS|panicked|Exception|Error|error:" 
set -e

echo
python3 "$here/tools/log-summary.py" "$log" || true
