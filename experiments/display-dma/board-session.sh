#!/bin/sh
# One locked board session: flash this firmware (the board is shared, so
# someone else's firmware may be on it), let it run its start-up
# measurements, then optionally press a button and keep listening.
#
#   ./board-session.sh <log> [button] [listen-seconds]
#
# Examples:
#   ./board-session.sh logs/boot.log                 # boot measurements only
#   ./board-session.sh logs/soak-80-16.log b 100     # B: 90 s check at 80 MHz, 16-bit
#   ./board-session.sh logs/soak-80-12.log x 100     # X: same in 12-bit
#   ./board-session.sh logs/soak-40-16.log y 100     # Y: control at 40 MHz
#   ./board-session.sh logs/tear-up.log up 15        # UP: next way of sending
#   ./board-session.sh logs/tear-cols.log 'up*3' 30  # UP three times (columns)
#   ./board-session.sh logs/demo.log - 30            # no button, just listen
#
# A button may end in *N (N from 1 to 9): pressed and let go N-1 times,
# then pressed a last time and held while listening.
#
# Set ELF to flash another build (for example the Bluetooth one).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
lock="$root/.local/board.lock"
boardctl="$root/tools/boardctl/target/release/boardctl"
elf=${ELF:-$here/target/xtensa-esp32-none-elf/release/display-dma}
log=$1
button=${2:-}
listen=${3:-0}
presses=1
case $button in
  *'*'[1-9]) presses=${button##*'*'}; button=${button%'*'*} ;;
  *'*'*) echo "button repeat must be *1 to *9: $button" >&2; exit 2 ;;
esac

export PATH="$HOME/.cargo/bin:$PATH"
. "$HOME/export-esp.sh"

# Everything that opens the port runs inside one lock, under a timeout.
flock "$lock" timeout 154 sh -c "
  espflash flash --chip esp32 --port /dev/ttyUSB0 --non-interactive '$elf' >/dev/null 2>&1 || { echo 'flash failed'; exit 2; }
  echo '--- flashed $elf'
  # espflash resets the chip after flashing; the start-up measurements take about
  # 10 s, then the tearing characterisation about 9 s (its read test about 5 s):
  # ready at about 19 s, the first demo report about 5 s later.
  # Flashing + 26 + the listen time below must stay under the 154 s timeout.
  '$boardctl' log --seconds 26
  if [ '$button' = '-' ]; then
    '$boardctl' log --seconds $listen
  elif [ -n '$button' ]; then
    i=1
    while [ \$i -lt $presses ]; do
      '$boardctl' key $button
      sleep 0.3
      i=\$((i + 1))
    done
    '$boardctl' key $button --keep
    '$boardctl' log --seconds $listen
    '$boardctl' release
  fi
" >"$log" 2>&1 || echo "session ended with status $? (124 = timeout)" >>"$log"
sed 's/\x1b\[[0-9;]*m//g' "$log" | grep -a -E 'bench=|check:|scan:|panel:|tear:|demo|ready|pressed|released|flash|session|boardctl' || true
