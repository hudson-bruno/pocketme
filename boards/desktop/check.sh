#!/bin/sh
# Checks the console's PC crates, all in one go. No window opens, no
# board is needed, nothing is flashed.
#
#   0. Every game in games/ built for the chip, linked for its two slots
#      and checked to fit: code at most 56 KB, data at most 48 KB
#      (games/build-chip.sh; it prints the sizes).
#   1. For each crate (and each game): format check, clippy (warnings count as errors),
#      tests. Each with the default parts and with all of them; the
#      simulator also with no window at all (--no-default-features).
#      The window tests use SDL's "offscreen" driver: nothing appears.
#      The GATHER boot animation (console-core's feature `boot-gather`)
#      is so checked both ways: console-core's default is without it and
#      --all-features with it; the simulator's default is with it and
#      --no-default-features without it.
#   2. The shared crates and the game side built for two targets with
#      no standard library at all, to prove they can run on the chip:
#        - the ESP32 itself (xtensa-esp32-none-elf), "esp" toolchain;
#        - a bare PC (x86_64-unknown-none), "nightly" toolchain.
#      Both build the core library from source (-Zbuild-std=core). This
#      only compiles; nothing is linked or flashed.
#
# There is no shared Cargo workspace on purpose: one above crates/ would
# take in the older crates and experiments too. So each crate is checked
# in its own folder, with one shared build folder to save time.
#
# Usage: boards/desktop/check.sh        (from anywhere)
set -eu
export PATH="$HOME/.cargo/bin:$PATH"
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
TARGET="$HERE/target/check"
# game-api and game-kit added by the game interface step (PC track);
# the games and their chip build by the games step.
CRATES="crates/game-api crates/game-kit crates/game-3d crates/board-api crates/console-video crates/console-core boards/desktop games/padtest games/stardust games/lander games/rooftop games/lantern games/hairpin games/kestrel games/tundra games/skimmer games/camber games/mercator games/bench games/drive3d games/proto3d"
SHARED="crates/game-api crates/game-kit crates/game-3d crates/board-api crates/console-video crates/console-core"
PARTS="bt-classic,passby,gps,sound"
CRATES="$CRATES crates/game-loader"; SHARED="$SHARED crates/game-loader" # game loader (step 3); its tests load the chip game files below
CRATES="$CRATES crates/settings-store"; SHARED="$SHARED crates/settings-store" # settings in flash (round 5); its tests cut the power at every byte
CRATES="$CRATES crates/touch-filter"; SHARED="$SHARED crates/touch-filter" # the CYD's touch filter (round 5), tested on the PC
CRATES="$CRATES crates/pad-host"; SHARED="$SHARED crates/pad-host" # the gamepad host (round 6): scripted-controller and fuzz tests on the PC

echo "== games for the chip"
GAMES_TARGET="$TARGET" "$ROOT/games/build-chip.sh"
# The simulator's tests check these files with the console's own header
# check (tests/games.rs).
export GAME_FILES_DIR="$TARGET/chip-games/files"

for c in $CRATES; do
    echo "== $c"
    cd "$ROOT/$c"
    # Plain `cargo fmt` touches only this crate, never its path
    # dependencies (unlike `cargo fmt --all`).
    cargo fmt --check
    # The simulator also once without its window (no outside library).
    case $c in
        boards/desktop) sets="--no-default-features --all-features" ;;
        *) sets="--all-features" ;;
    esac
    for f in "" $sets; do
        cargo clippy --offline --locked --target-dir "$TARGET" --all-targets $f -- -D warnings
        cargo test --offline --locked --target-dir "$TARGET" -q $f
    done
done

# The esp toolchain needs its environment for some builds; harmless here.
if [ -f "$HOME/export-esp.sh" ]; then . "$HOME/export-esp.sh" >/dev/null 2>&1 || true; fi
for pair in "esp xtensa-esp32-none-elf" "nightly x86_64-unknown-none"; do
    set -- $pair
    for c in $SHARED; do
        # game-api, game-kit, game-3d and touch-filter have no optional
        # parts.
        # console-video and console-core also with `swap` (the two
        # pictures swap roles; see crates/console-video/README.md), and
        # console-core with `boot-gather` (the GATHER boot animation).
        case $c in
            *game-api|*game-kit|*game-3d|*touch-filter) all="" ;;
            *console-video) all="--features swap" ;;
            *console-core) all="--features $PARTS,swap,boot-gather" ;;
            *) all="--features $PARTS" ;;
        esac
        for f in "" ${all:+"$all"}; do
            echo "== $c for $2 (toolchain $1) $f"
            cd "$ROOT/$c"
            cargo "+$1" build --offline -q -Zbuild-std=core --target "$2" \
                --target-dir "$TARGET/no-std" $f
        done
    done
done

# The whole chain once, from the command line: a few frames, one PNG.
cd "$ROOT/boards/desktop"
cargo run --offline --locked --target-dir "$TARGET" -q -- --quiet --frames 3 --png "$TARGET/check-shot.png"
# The build with no window must use nothing from outside Rust.
if cargo tree --offline --locked --no-default-features -e normal | grep -q sdl3; then
    echo "check: the no-window build pulls in sdl3" >&2; exit 1
fi
echo "check: ok"
