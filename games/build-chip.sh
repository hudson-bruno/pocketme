#!/bin/sh
# Builds every game in games/ for the chip and checks that it fits.
#
# The two slots (where a game's code and data live on the chip) come from
# one place, crates/game-api (`layout`): this script asks that crate for
# them (its example `slots_x`) and writes them to <files>/slots.x, which
# chip/game.x includes. No slot number is written in this script or in
# chip/.
#
# For each folder here with a Cargo.toml (a game crate):
#   1. compile it for the ESP32 (xtensa-esp32-none-elf, "esp" toolchain)
#      as one library file, with the game kit's entry function in it;
#   2. link it for the two game slots with chip/game.x (code 56 KB at
#      0x4009_2000, data 48 KB at 0x3FFF_4000 on 2026-11-17), as the
#      loader experiment did (experiments/native-loader/build-game.sh);
#   3. print the sizes, and FAIL when the code is bigger than the code
#      slot and the spill together, or when the code past the code slot
#      (the spill, stage 1: it takes the TOP of the data slot) and the
#      data (constants + starting values + zeroed variables) do not both
#      fit in the data slot (game_api::layout::fits; the linker's ASSERT
#      in chip/game.x stops that first, this prints the numbers);
#   3b. read every function's stack frame from the linked game and FAIL
#      when one is over its limit (chip/check_frames.py; the table goes
#      to <files>/<NAME>.frames.txt, the three biggest are printed). The
#      limits are estimates and a guard against one known mistake (the
#      game's state built inside the update call's frame), not a proof
#      that a game fits the console's stack: see that file;
#   4. pack it into the file the console loads: <NAME>.BIN, where NAME is
#      the folder name in capitals (the folder it gets on the card);
#   5. load that file as the console does and check every byte against
#      the linked game, spill and data sharing one memory
#      (chip/check_load.py, with this run's slots.x);
#   6. put it, with the game's INFO.TXT, into a card folder:
#      <files>/card/CONSOLE/GAMES/<NAME>/{GAME.BIN,INFO.TXT} (the CYD's
#      demo card is built from there);
#   7. when the game has a tile picture (games/<game>/art.txt), turn it
#      into <NAME>/ART.BIN in that folder too (art/make_art.py; the
#      format is described there). No art.txt: no ART.BIN, and the home
#      screen shows the letter tile.
#
# A game with a flash part (a games/<game>/flash.list; see games/README.md,
# "Flash parts") links twice instead of once in step 2:
#   2a. pass 1, the plain link above, only to read the call graph (it may
#       be too big for the slots, so it never stops here and is never
#       packed): <files>/flash/<NAME>/pass1.elf and .map;
#   2b. chip/flash_split.py split: which functions and constants go to
#       flash (flash-code.x, flash-rodata.x, split.txt in the same folder);
#   2c. the game's place in the firmware's flash windows, from game-api
#       (slots_x --flash NAME > flash-place.x);
#   2d. pass 2 with chip/game-flash.x before chip/game.x: the real ELF;
#   then step 3 (sizes, plus the flash halves), 3b, and
#   3c. chip/flash_split.py check (the rules; a failure packs nothing) and
#       selftest (two wrong lists must fail);
#   4. a "GAMF" file (pack.py --flash) without the flash halves, which go
#      to <files>/flash/<NAME>/CODE.BIN and RO.BIN for the firmware
#      (boards/cyd, feature game-flash); never onto a card.
# Every game without flash.list builds exactly as before, byte for byte.
#
# Nothing is flashed and no board is needed. Before anything is built,
# chip/test_check_frames.py tests the frame reader on a made-up
# disassembly (no chip tools needed).
#
# Usage: games/build-chip.sh [GAME_FOLDER...]   (from anywhere)
#   With no folders: every game in games/. With folders: only those (a
#   test game kept elsewhere, for example).
# Results: boards/desktop/target/check/chip-games/files/ (set GAMES_TARGET
# to use another build folder than boards/desktop/target/check).
# Tests only: GAMES_SLOTS_X=<file> links for the slots in that file
# instead of game-api's (to make a game for another layout, which the
# console must refuse). Never put such a game on a card.
set -eu
# chip/test_check_frames.py imports chip/check_frames.py: without this,
# Python would leave a chip/__pycache__/ folder in the source tree.
export PYTHONDONTWRITEBYTECODE=1

# Games whose chip build has overflow checks OFF (folder names in games/,
# separated by spaces). Every other game, a new one too, has them ON
# without doing anything. With them off, a sum that runs over (200 + 100
# in a u8) quietly wraps round on the chip: a range mistake shows as a
# wrong picture or a wrong number instead of a stop. The PC tests of the
# same game (debug builds) keep the checks on, so the tests still find
# such a mistake. A name here that is not a game folder in games/ stops
# the script with an error. Owner's decision, 2026-11-19: CAMBER only, for speed
# (games/camber/README.md, "Overflow checks"). Kept here and not in the
# game's Cargo.toml, like every chip-only setting, so the simulator's
# build of the game is not changed. Only folders inside games/ are
# matched; a game folder kept elsewhere always has the checks on.
OVERFLOW_CHECKS_OFF="camber"
export PATH="$HOME/.cargo/bin:$PATH"
# The Xtensa linker and tools (xtensa-esp32-elf-ld, -size, -nm, -objcopy).
if [ -f "$HOME/export-esp.sh" ]; then . "$HOME/export-esp.sh" >/dev/null 2>&1 || true; fi
HERE="$(cd "$(dirname "$0")" && pwd)"
# A name in OVERFLOW_CHECKS_OFF that is not a game folder (a typing
# mistake such as "cambr" or "Camber") would leave that game's checks on
# without a word, so the build stops here instead. Only a bare folder
# name counts (a path such as ../games/camber would never match).
bad_names=0
set -f
for off in $OVERFLOW_CHECKS_OFF; do
    case "$off" in
        *[!A-Za-z0-9_-]*) ok=false ;;
        *) if [ -f "$HERE/$off/Cargo.toml" ]; then ok=true; else ok=false; fi ;;
    esac
    if [ "$ok" = false ]; then
        echo "build-chip: OVERFLOW_CHECKS_OFF names \"$off\", which is not a game folder in games/" >&2
        bad_names=1
    fi
done
set +f
if [ "$bad_names" -ne 0 ]; then exit 1; fi
# The stack frame reader's and the flash splitter's own tests (made-up
# text, no chip tools).
for selftest in test_check_frames.py test_flash_split.py; do
    if ! python3 "$HERE/chip/$selftest" >/dev/null 2>&1; then
        python3 "$HERE/chip/$selftest" >&2 || true
        echo "build-chip: chip/$selftest failed (see above)" >&2
        exit 1
    fi
done
TARGET="${GAMES_TARGET:-$HERE/../boards/desktop/target/check}/chip-games"
OUT="$TARGET/files"
mkdir -p "$OUT"

# The slots, from game-api (or, for a test, from GAMES_SLOTS_X).
SLOTS="$OUT/slots.x"
if [ -n "${GAMES_SLOTS_X:-}" ]; then
    echo "build-chip: TEST ONLY: slots from $GAMES_SLOTS_X, not from game-api"
    cp "$GAMES_SLOTS_X" "$SLOTS"
else
    (cd "$HERE/../crates/game-api" && cargo run --offline --locked -q \
        --target-dir "$TARGET/host" --example slots_x) > "$SLOTS.new"
    mv "$SLOTS.new" "$SLOTS"
fi
slot_value() {
    v=$(awk -v k="$1" '$1 == k && $2 == "=" { sub(";", "", $3); print $3 }' "$SLOTS")
    case "$v" in
        0x[0-9a-fA-F]*) echo "$v" ;;
        *) echo "build-chip: $SLOTS has no $1" >&2; exit 1 ;;
    esac
}
CODE_ADDR=$(slot_value GAME_CODE_ORIGIN)
CODE_MAX=$(($(slot_value GAME_CODE_LENGTH)))
DATA_ADDR=$(slot_value GAME_DATA_ORIGIN)
DATA_MAX=$(($(slot_value GAME_DATA_LENGTH)))
SPILL_ADDR=$(slot_value GAME_SPILL_ORIGIN)
SPILL_MAX=$(($(slot_value GAME_SPILL_LENGTH)))
echo "build-chip: slots: code $CODE_ADDR, $CODE_MAX bytes; data $DATA_ADDR, $DATA_MAX bytes; code past the slot at $SPILL_ADDR, up to $SPILL_MAX bytes, shared with the data"

for tool in xtensa-esp32-elf-ld xtensa-esp32-elf-size xtensa-esp32-elf-nm xtensa-esp32-elf-objcopy xtensa-esp32-elf-objdump python3; do
    command -v "$tool" >/dev/null || { echo "build-chip: $tool is missing" >&2; exit 1; }
done

# Section sizes of a linked game, by name.
size_of() {
    xtensa-esp32-elf-size -A "$1" | awk -v s="$2" '$1 == s { print $2; f = 1 } END { if (!f) print 0 }'
}
# A section's address, in hex (0x...); $3 when it is not there (an empty
# flash data half: its place's address).
section_addr() {
    xtensa-esp32-elf-size -A -x "$1" | awk -v s="$2" -v d="${3:-none}" '$1 == s { print $3; f = 1 } END { if (!f) print d }'
}
# Prints a game's size line from its ELF and sets code, rodata, data,
# bss. Fails when the game does not fit its slots.
measure() {
    text=$(size_of "$1" .text)
    rodata=$(size_of "$1" .rodata)
    data=$(size_of "$1" .data)
    bss=$(size_of "$1" .bss)
    code=$text
    data_all=$((rodata + data + bss))
    # Code past the code slot (the spill) shares the data slot with the
    # data, so the room left is one number for both.
    spill=0
    if [ "$code" -gt "$CODE_MAX" ]; then spill=$((code - CODE_MAX)); fi
    printf '%-8s code %6d of %d (+%d past the slot)   data %6d (constants %d, starting values %d, zeroed %d)   room left %d of %d\n' \
        "$name" "$code" "$CODE_MAX" "$spill" "$data_all" "$rodata" "$data" "$bss" \
        $((CODE_MAX + DATA_MAX - code - data_all)) $((CODE_MAX + DATA_MAX))
    fit=0
    if [ "$code" -gt $((CODE_MAX + SPILL_MAX)) ]; then
        echo "build-chip: $name: the code is over the code slot and the spill ($((CODE_MAX + SPILL_MAX)) bytes)" >&2
        fit=1
    fi
    if [ $((data_all + spill)) -gt "$DATA_MAX" ]; then
        echo "build-chip: $name: GAME TOO BIG: code past the slot ($spill) and data ($data_all) meet in the data slot ($DATA_MAX bytes)" >&2
        fit=1
    fi
    return "$fit"
}

if [ "$#" -eq 0 ]; then set -- "$HERE"/*/; fi
failed=0
count=0
# (The list is read once, so the loop body may set "$@" for its own use.)
for dir in "$@"; do
    dir="$(cd "$dir" && pwd)"
    [ -f "$dir/Cargo.toml" ] || continue
    crate="$(basename "$dir")"
    name="$(echo "$crate" | tr 'a-z-' 'A-Z_')"
    if [ "${#name}" -gt 8 ]; then
        echo "build-chip: $crate: the card folder $name is longer than 8 letters" >&2
        exit 1
    fi
    # Release build, small code (opt-level s), one piece (fat LTO), and a
    # panic stops the game (no unwinding on the chip). Given here and not
    # in the game's Cargo.toml, so the simulator's build of the same game
    # is not changed (and Cargo does not warn about it).
    # Overflow checks stay on, as in the simulator: a number that runs
    # over (200 + 100 in a u8) stops the game on both, instead of
    # stopping it on the PC and quietly wrapping round on the chip. Only
    # the games in OVERFLOW_CHECKS_OFF (above) have them off.
    # Paths are trimmed: the file names kept for panic messages say
    # "game-kit-0.1.0/src/ctx.rs", not the builder's home folder, so a
    # shared game file does not carry its maker's user name.
    # (Both added by the second reader, step 2.)
    checks=true
    case " $OVERFLOW_CHECKS_OFF " in
        *" $crate "*)
            if [ "$dir" = "$HERE/$crate" ]; then
                checks=false
                echo "build-chip: $name: overflow checks OFF (OVERFLOW_CHECKS_OFF in this script)"
            fi
            ;;
    esac
    (cd "$dir" && cargo +esp rustc --offline --locked -q --release -Ztrim-paths \
        --target xtensa-esp32-none-elf -Zbuild-std=core \
        --target-dir "$TARGET" --crate-type staticlib \
        --config 'profile.release.panic="abort"' \
        --config 'profile.release.opt-level="s"' \
        --config 'profile.release.lto="fat"' \
        --config 'profile.release.codegen-units=1' \
        --config "profile.release.overflow-checks=$checks" \
        --config 'profile.release.trim-paths="all"')
    lib="$TARGET/xtensa-esp32-none-elf/release/lib$(echo "$crate" | tr - _).a"
    elf="$OUT/$name.elf"
    # A game that fails below must not leave the file of an earlier good
    # build behind, where a card or a demo-card build would take it.
    rm -f "$OUT/$name.BIN" "$OUT/$name.payload" "$OUT/$name.frames.txt" "$OUT/card/CONSOLE/GAMES/$name/GAME.BIN"
    fdir="$OUT/flash/$name"
    flash=false
    if [ -f "$dir/flash.list" ]; then flash=true; fi
    if [ "$flash" = true ]; then
        # 2a. Pass 1: the plain link, only for the call graph. It may not
        # fit without its flash part, so --noinhibit-exec (the file is
        # written in spite of errors) and no stop here; never packed.
        rm -rf "$fdir"
        mkdir -p "$fdir"
        xtensa-esp32-elf-ld -L "$OUT" -L "$HERE/chip" -T "$HERE/chip/game.x" --gc-sections \
            --noinhibit-exec -Map "$fdir/pass1.map" -o "$fdir/pass1.elf" "$lib" >/dev/null 2>&1 || true
        if [ ! -f "$fdir/pass1.elf" ]; then
            echo "build-chip: $name: pass 1 of the flash part did not link" >&2
            failed=1
            continue
        fi
        # 2b. The split.
        if ! python3 "$HERE/chip/flash_split.py" split "$fdir/pass1.elf" "$fdir/pass1.map" \
            "$dir/flash.list" "$fdir"; then
            echo "build-chip: $name: the flash split failed (see above)" >&2
            failed=1
            continue
        fi
        # 2c. The place, from game-api.
        if ! (cd "$HERE/../crates/game-api" && cargo run --offline --locked -q \
            --target-dir "$TARGET/host" --example slots_x -- --flash "$name") > "$fdir/flash-place.x"; then
            echo "build-chip: $name: has flash.list but no place in game_api::layout::FLASH_PLACES" >&2
            failed=1
            continue
        fi
    fi
    # The linker itself refuses a game that does not fit its slots, or that
    # calls something it does not have.
    # slots.x is found in $OUT (the first -L), next to the results. With a
    # flash part (2d), game-flash.x comes first and finds flash-place.x,
    # flash-code.x and flash-rodata.x in $fdir.
    if [ "$flash" = true ]; then
        set -- -L "$fdir" -L "$OUT" -L "$HERE/chip" -T "$HERE/chip/game-flash.x"
    else
        set -- -L "$OUT" -L "$HERE/chip"
    fi
    if ! xtensa-esp32-elf-ld "$@" -T "$HERE/chip/game.x" --gc-sections \
        -Map "$OUT/$name.map" -o "$elf" "$lib"; then
        echo "build-chip: $name: the link failed (see above)" >&2
        # Link once more, only to print the sizes: --noinhibit-exec writes
        # the output in spite of the errors (and then ends with 0, so this
        # file is measured and deleted, never packed).
        big="$OUT/$name.toobig.elf"
        if xtensa-esp32-elf-ld "$@" -T "$HERE/chip/game.x" --gc-sections \
            --noinhibit-exec -o "$big" "$lib" >/dev/null 2>&1 && [ -f "$big" ]; then
            measure "$big" || true
        fi
        rm -f "$big"
        failed=1
        continue
    fi
    measure "$elf" || failed=1
    if [ "$flash" = true ]; then
        place_len() { echo $(($(awk -v k="$1" '$1 == k { sub(";", "", $3); print $3 }' "$fdir/flash-place.x"))); }
        printf '%-8s flash part: code %6d of %d   constants %6d of %d\n' "$name" \
            "$(size_of "$elf" .flash_text)" "$(place_len GAME_FLASH_CODE_LENGTH)" \
            "$(size_of "$elf" .flash_rodata)" "$(place_len GAME_FLASH_RO_LENGTH)"
    fi
    # Stack frames: the table, the three biggest, and a stop when one is
    # over its limit (no file is packed for that game).
    if ! python3 "$HERE/chip/check_frames.py" "$elf" "$name" "$OUT/$name.frames.txt"; then
        failed=1
        continue
    fi
    entry=$(xtensa-esp32-elf-nm "$elf" | awk '$3 == "game_entry" { print $1 }')
    if [ -z "$entry" ]; then
        echo "build-chip: $name: no game_entry (is game_kit::game! missing?)" >&2
        failed=1
        continue
    fi
    if [ "$flash" = true ]; then
        # 3c. The rules, then the proof that they bite on this game.
        if ! python3 "$HERE/chip/flash_split.py" check "$elf" "$dir/flash.list" \
            "$fdir/flash-place.x" > "$fdir/check.txt"; then
            cat "$fdir/check.txt" >&2
            echo "build-chip: $name: the flash part breaks a rule (see above); nothing packed" >&2
            failed=1
            continue
        fi
        head -n 3 "$fdir/check.txt"
        if ! python3 "$HERE/chip/flash_split.py" selftest "$elf" "$dir/flash.list" \
            "$fdir/flash-place.x"; then
            echo "build-chip: $name: the flash checker did not refuse a wrong list" >&2
            failed=1
            continue
        fi
        # 4. The halves for the firmware, and a file without them.
        xtensa-esp32-elf-objcopy -O binary -j .flash_text "$elf" "$fdir/CODE.BIN"
        xtensa-esp32-elf-objcopy -O binary -j .flash_rodata "$elf" "$fdir/RO.BIN"
        xtensa-esp32-elf-objcopy -O binary -R .flash_text -R .flash_rodata "$elf" "$OUT/$name.payload"
        python3 "$HERE/chip/pack.py" "$OUT/$name.payload" "$code" $((rodata + data)) "$bss" \
            "$entry" "$OUT/$name.BIN" "$CODE_ADDR" "$DATA_ADDR" \
            --flash "$(section_addr "$elf" .flash_text)" "$fdir/CODE.BIN" \
            "$(section_addr "$elf" .flash_rodata "$(awk '$1 == "GAME_FLASH_RO_ORIGIN" { sub(";", "", $3); print $3 }' "$fdir/flash-place.x")")" "$fdir/RO.BIN"
        set -- --flash "$fdir/CODE.BIN" "$fdir/RO.BIN"
    else
        xtensa-esp32-elf-objcopy -O binary "$elf" "$OUT/$name.payload"
        python3 "$HERE/chip/pack.py" "$OUT/$name.payload" "$code" $((rodata + data)) "$bss" \
            "$entry" "$OUT/$name.BIN" "$CODE_ADDR" "$DATA_ADDR"
        set --
    fi
    # Load the file as the console does and compare it byte for byte with
    # what the linker placed (added by the second reader, step 2). The
    # slot numbers come from slots.x (stage 1: the spill too). With a
    # flash part: the extension against the ELF and the two halves.
    if ! python3 "$HERE/chip/check_load.py" "$elf" "$OUT/$name.BIN" "$SLOTS" "$@"; then
        rm -f "$OUT/$name.BIN"
        failed=1
        continue
    fi
    # The card folder: GAME.BIN and the game's INFO.TXT (or a bare NAME).
    card="$OUT/card/CONSOLE/GAMES/$name"
    mkdir -p "$card"
    cp "$OUT/$name.BIN" "$card/GAME.BIN"
    if [ -f "$dir/INFO.TXT" ]; then
        cp "$dir/INFO.TXT" "$card/INFO.TXT"
    else
        printf 'NAME=%s\n' "$name" > "$card/INFO.TXT"
    fi
    rm -f "$card/ART.BIN"
    if [ -f "$dir/art.txt" ]; then
        if ! python3 "$HERE/art/make_art.py" "$dir/art.txt" "$card/ART.BIN"; then
            echo "build-chip: $name: art.txt is not a picture the console takes (see above)" >&2
            failed=1
            continue
        fi
    fi
    count=$((count + 1))
done

if [ "$failed" -ne 0 ]; then
    echo "build-chip: FAILED" >&2
    exit 1
fi
echo "build-chip: $count games fit their slots; files in $OUT"
