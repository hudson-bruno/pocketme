#!/bin/sh
# Proves that the board layer and the demo need no standard library:
# builds them for two targets that have none at all.
#   1. the ESP32 itself (xtensa-esp32-none-elf), with the ESP toolchain;
#   2. a bare PC (x86_64-unknown-none), with the nightly toolchain.
# Both build the core library from source (-Zbuild-std=core). Nothing is
# flashed and no board is needed: this only compiles.
set -e
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")"
for pair in "esp xtensa-esp32-none-elf" "nightly x86_64-unknown-none"; do
    set -- $pair
    echo "== $2 (toolchain $1)"
    cargo "+$1" build --offline -Zbuild-std=core --target "$2" \
        -p board-layer -p console-demo --target-dir target/no-std
done
echo "no_std check: ok"
