#!/usr/bin/env bash
# Builds the test game three times (the normal one and the two that crash
# on purpose) and packs each into the one file the console loads.
#
#   ./build-game.sh
#
# Results: game/target/images/HELLO.BIN, CRASH.BIN and FAULT.BIN
#
# It also checks two things the experiment set out to learn:
#   * what the linker put in each slot (the section list is printed), and
#   * whether `objcopy -O binary` (GNU and LLVM) gives the same bytes as
#     our own packer.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
# The Xtensa compiler and linker.
. "$HOME/export-esp.sh"

elf="$here/game/target/xtensa-esp32-none-elf/release/hello-game"
images="$here/game/target/images"
mkdir -p "$images"

# The packer runs on the PC, so it is built with the PC's own toolchain.
(cd "$here/tools/game-pack" && cargo build --release --quiet)
pack="$here/tools/game-pack/target/release/game-pack"

build_one() {  # build_one <NAME> [cargo arguments]
    local name="$1"; shift
    echo "=== $name ==="
    (cd "$here/game" && cargo build --release --quiet "$@")
    # Keep each variant's ELF: the next build overwrites the original.
    cp "$elf" "$images/$name.elf"
    "$pack" pack "$images/$name.elf" "$images/$name.BIN"

    # Second opinion: objcopy lays the loaded sections out by their load
    # address, which the linker script made consecutive. The result must be
    # the packed file without its 36-byte header.
    tail -c +37 "$images/$name.BIN" > "$images/$name.payload"
    xtensa-esp32-elf-objcopy -O binary "$images/$name.elf" "$images/$name.gnu-objcopy"
    if cmp -s "$images/$name.payload" "$images/$name.gnu-objcopy"; then
        echo "GNU objcopy -O binary: same bytes as the packer's payload"
    else
        echo "GNU objcopy -O binary: DIFFERENT from the packer's payload"; exit 1
    fi
    if command -v llvm-objcopy >/dev/null; then
        if llvm-objcopy -O binary "$images/$name.elf" "$images/$name.llvm-objcopy" 2>"$images/$name.llvm-objcopy.err" \
            && cmp -s "$images/$name.payload" "$images/$name.llvm-objcopy"; then
            echo "llvm-objcopy -O binary: same bytes as the packer's payload"
        else
            echo "llvm-objcopy -O binary: failed or different (see $images/$name.llvm-objcopy.err)"
        fi
    fi
    echo
}

build_one HELLO
build_one CRASH --features crash
build_one FAULT --features fault
# Leave the normal game as the ELF in the build folder.
(cd "$here/game" && cargo build --release --quiet)
