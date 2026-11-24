#!/usr/bin/env bash
# Checks the CYD firmware without the board: the real game files first
# (games/build-chip.sh), the radio's HCI rules (hci-rules: format, clippy,
# PC tests), the screen's timing rules (send-plan: the same), the two
# cores' hand-over (handover: the same), formatting, clippy on both
# worlds (two cores, the default, and one core), and a build of every
# combination of the 7 other features without core2 (128 one-core
# builds) plus nine with the gamepad (pad), the measuring switch send-ab
# or the boot animation boot-gather, and ten with the second core
# sending the pictures (core2, two of them with send-ab, one with
# hang-test, one with xip-probe, one with boot-gather; one is the default
# set): 147 builds; then the owner's builds again, two cores and one,
# each with and without boot-gather, each checked by hot-check.py (the
# timed screen paths' hot block, see hot-block.list, and for the game
# slots' start-up check in the firmware) and by xip-check.py --absent (no
# trace of the flash code probe), the owner's builds with the gamepad and
# the game flash parts (game-flash, two cores and one, with and without
# `pair`), checked the same way and by game-flash-check.py (the windows
# and the parts in them; the other sets by game-flash-check.py --absent),
# and the two-core one once more with the measuring switch xip-probe,
# checked by hot-check.py and xip-check.py (the probe block):
# 156 builds, about 22 minutes.
# Never opens the serial port, never flashes. It refuses to run with the
# measuring switches CYD_LAYOUT_PAD or CYD_DEMO_EXTRA set (build.rs reads
# them; they would change every build and move the hot block).
#
# Run from anywhere:  boards/cyd/check.sh
# Builds go to target/check, so the firmware in target/ (the one you
# flash) is left as it is.
set -euo pipefail

for v in CYD_LAYOUT_PAD CYD_DEMO_EXTRA; do
    if [ -n "${!v+x}" ]; then
        echo "check.sh: $v is set (a measuring switch of build.rs); unset it first"
        exit 1
    fi
done

here="$(cd "$(dirname "$0")" && pwd)"
cd "$here"
export PATH="$HOME/.cargo/bin:$PATH"
# The esp toolchain's settings (the Xtensa linker).
# shellcheck disable=SC1091
. "$HOME/export-esp.sh" >/dev/null 2>&1
export CARGO_TARGET_DIR="$here/target/check"

echo "== the real games (demo-card builds them in)"
# build.rs takes them from target/chip-games/files (CYD_GAME_FILES).
GAMES_TARGET="$here/target" "$here/../../games/build-chip.sh"

echo "== hci-rules (the radio's HCI rules, on the PC)"
# From the repository's root, so that neither this crate's chip settings
# (.cargo/config.toml) nor its toolchain apply: hci-rules has its own
# (rust-toolchain.toml there names it).
(
    cd "$here/../.."
    m="$here/hci-rules/Cargo.toml"
    tc="+$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$here/hci-rules/rust-toolchain.toml")"
    export CARGO_TARGET_DIR="$here/target/hci-rules"
    cargo "$tc" fmt --manifest-path "$m" -- --check || {
        echo "formatting differs: run 'cargo fmt' in boards/cyd/hci-rules"
        exit 1
    }
    cargo "$tc" clippy --offline --quiet --manifest-path "$m" --all-targets -- -D warnings
    cargo "$tc" clippy --offline --quiet --manifest-path "$m" --all-targets --features pair -- -D warnings
    cargo "$tc" test --offline --quiet --manifest-path "$m"
    cargo "$tc" test --offline --quiet --manifest-path "$m" --features pair
)

echo "== send-plan (the screen's timing rules and their model, on the PC)"
# Like hci-rules: from the repository's root, with its own toolchain.
# Its feature `model` (PC only) holds the replay model the tests use.
(
    cd "$here/../.."
    m="$here/send-plan/Cargo.toml"
    tc="+$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$here/send-plan/rust-toolchain.toml")"
    export CARGO_TARGET_DIR="$here/target/send-plan"
    cargo "$tc" fmt --manifest-path "$m" -- --check || {
        echo "formatting differs: run 'cargo fmt' in boards/cyd/send-plan"
        exit 1
    }
    cargo "$tc" clippy --offline --quiet --manifest-path "$m" --all-targets --all-features -- -D warnings
    cargo "$tc" clippy --offline --quiet --manifest-path "$m" -- -D warnings
    cargo "$tc" test --offline --quiet --manifest-path "$m" --all-features
)

echo "== handover (the two cores' hand-over and its model, on the PC)"
# Like hci-rules: from the repository's root, with its own toolchain.
(
    cd "$here/../.."
    m="$here/handover/Cargo.toml"
    tc="+$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$here/handover/rust-toolchain.toml")"
    export CARGO_TARGET_DIR="$here/target/handover"
    cargo "$tc" fmt --manifest-path "$m" -- --check || {
        echo "formatting differs: run 'cargo fmt' in boards/cyd/handover"
        exit 1
    }
    cargo "$tc" clippy --offline --quiet --manifest-path "$m" --all-targets -- -D warnings
    cargo "$tc" test --offline --quiet --manifest-path "$m"
)

echo "== format (this crate only)"
# Plain `cargo fmt` inside this crate; -v lists the files it looks at,
# which must all be this crate's own.
out="$(cargo fmt -v -- --check 2>&1)" || {
    echo "$out"
    echo "formatting differs: run 'cargo fmt' in boards/cyd"
    exit 1
}
if echo "$out" | grep '^rustfmt' | tr ' ' '\n' | grep '\.rs$' | grep -v "^$here/"; then
    echo "cargo fmt would look at files outside boards/cyd"
    exit 1
fi

echo "== pairing flash safety guard"
if grep -rnE 'Store::open|forget_all' "$here/src/"; then
    echo "forbidden pad-store write path in boards/cyd/src/"
    exit 1
fi
if grep -rnE '\.save\(' "$here/src/pairing.rs" "$here/src/pad_radio.rs" "$here/src/pad_radio/"; then
    echo "forbidden pad-store .save() call in boards/cyd pairing modules"
    exit 1
fi

echo "== clippy"
# The default features have core2 (two cores); the one-core world gets
# its own runs: the old default set (tour), and the owner's build with
# the gamepad and send-ab.
for set in "" "--no-default-features" "--all-features" \
    "--no-default-features --features demo-card,touch,link,tour,store" \
    "--no-default-features --features demo-card,touch,link,store,pad,send-ab"; do
    echo "-- clippy ${set:-(default features)}"
    # shellcheck disable=SC2086
    cargo clippy --release --offline --quiet $set -- -D warnings
done
# core2 without the store (no flash guard), with it but no radio, the
# owner's build with the gamepad and send-ab, and the measuring switch
# xip-probe without the link and the store; --all-features above has
# everything.
# Then the game flash parts (game-flash) on the owner's build with the
# gamepad, and alone on one core.
for joined in "core2" "demo-card,touch,link,store,core2" "demo-card,touch,link,store,pad,core2,send-ab" \
    "core2,xip-probe" "demo-card,touch,link,store,pad,core2,boot-gather,game-flash" \
    "demo-card,touch,link,store,pad,core2,boot-gather,game-flash,pair" \
    "demo-card,touch,link,store,pad,boot-gather,game-flash,pair" "game-flash"; do
    echo "-- clippy $joined"
    cargo clippy --release --offline --quiet --no-default-features --features "$joined" -- -D warnings
done

echo "== build every feature combination"
# One build; on failure the linker's own HOT BLOCK or GAME SLOT message
# (build.rs's guards), if that is why.
build() {
    local err
    if err="$(cargo build --release --offline --quiet --no-default-features --features "$1" 2>&1)"; then
        echo ok
    else
        echo FAILED
        echo "$err" | grep -o '\(HOT BLOCK\|GAME SLOT\):.*' || true
        failed=$((failed + 1))
    fi
}
features=(demo-card sd-card touch link tour wide-strips store)
n=${#features[@]}
failed=0
for ((mask = 0; mask < (1 << n); mask++)); do
    list=()
    for ((i = 0; i < n; i++)); do
        if (((mask >> i) & 1)); then list+=("${features[$i]}"); fi
    done
    joined="$(IFS=,; echo "${list[*]-}")"
    printf '%-55s ' "[${joined:-none}]"
    build "$joined"
done

# The gamepad (pad) on top of six sets, not on every combination (that
# would double the time), all on one core: the old default set (with
# tour), the owner's build (demo-card touch link store), the gamepad alone (no store, no touch,
# no link), without the store and touch but with the link (injection),
# the real card without the link, and everything. Then the measuring
# switch send-ab (old/new screen plan, timing off: letters o, p, u):
# on the owner's build with the gamepad, and alone (letters without the
# link). Last, boot-gather (the GATHER boot animation) on the owner's
# build with the gamepad.
radio_sets=("demo-card,touch,link,tour,store,pad" "demo-card,touch,link,store,pad" "pad"
    "demo-card,link,pad" "sd-card,touch,store,pad"
    "demo-card,sd-card,touch,link,tour,wide-strips,store,pad,send-ab"
    "demo-card,touch,link,store,pad,send-ab" "send-ab"
    "demo-card,touch,link,store,pad,boot-gather")
for joined in "${radio_sets[@]}"; do
    printf '%-55s ' "[$joined]"
    build "$joined"
done

# The second core (core2, on by default) on ten sets: the owner's build
# with and without pad, the default features (tour), core2 alone (no
# store, no link), and everything but wide-strips; then the measuring
# switch send-ab with core2: on the owner's build with the gamepad, and
# alone (letters without the link); and the measuring switch hang-test
# (letter H: the watchdog); and the measuring switch xip-probe (the flash
# code probe, letters a d j 0-5) alone, without the link and the store (it
# is built with the owner's set in the hot block loop below); and
# boot-gather (the GATHER boot animation) on the owner's build with the
# gamepad. The feature list above leaves
# core2 out, so these are the only builds with it. core2 with pad and
# wide-strips does not link: the main stack would be under 8 KB.
core2_sets=("demo-card,touch,link,store,pad,core2" "demo-card,touch,link,store,core2"
    "demo-card,touch,link,tour,store,core2" "core2"
    "demo-card,sd-card,touch,link,tour,store,pad,core2"
    "demo-card,touch,link,store,pad,core2,send-ab" "core2,send-ab"
    "demo-card,touch,link,store,core2,hang-test" "core2,xip-probe"
    "demo-card,touch,link,store,pad,core2,boot-gather")
for joined in "${core2_sets[@]}"; do
    printf '%-55s ' "[$joined]"
    build "$joined"
done
total=$(((1 << n) + ${#radio_sets[@]} + ${#core2_sets[@]}))

# The timed screen paths' hot block (hot-block.list, hot-check.py) on the
# owner's builds, two cores and one, with and without the boot animation
# (boot-gather, in the owner's builds since 2026-11-19): every listed
# function found and in its part of the block, nothing in the timed
# paths calling flash code outside it, its literals inside it, the parts
# above their floors and small enough for the flash cache. A broken link (build.rs's own guards)
# shows as "FAILED (build)", the guard's faults as "FAILED (hot-check)";
# the guard's last line (sizes, cache lines left) is shown either way.
# Each of these builds must also hold the game slots' start-up log lines
# (slots ok, spill mirror ok and WRONG), else "FAILED (game slots)".
# The last set is the measuring switch xip-probe on the owner's build
# with the gamepad: xip-check.py checks its probe block (128 KB of flash
# code before the hot block, nothing in it but the probe lines); on the
# other sets xip-check.py --absent checks that no trace of it is there.
# Its faults show as "FAILED (xip-check)". The sets with game-flash are
# checked by game-flash-check.py (the two windows where game_api::layout
# says, running straight into esp-hal's own flash code and data, the end
# marks, each part byte for byte as games/build-chip.sh wrote it, zeros
# elsewhere) and must hold the flash part check's log lines; the others
# by game-flash-check.py --absent. Its faults show as "FAILED (game
# flash)". Its last line ends with the bytes the image loads (about the
# size espflash writes to the app partition, 4,063,232 bytes).
# Rebuilt here because the loops above leave only the last set in
# target/check.
echo "== the hot block (hot-check.py, xip-check.py)"
for joined in "demo-card,touch,link,store,pad,core2,boot-gather" "demo-card,touch,link,store,pad,boot-gather" \
    "demo-card,touch,link,store,pad,core2" "demo-card,touch,link,store,pad" \
    "demo-card,touch,link,store,pad,core2,boot-gather,game-flash" "demo-card,touch,link,store,pad,boot-gather,game-flash" \
    "demo-card,touch,link,store,pad,core2,boot-gather,game-flash,pair" "demo-card,touch,link,store,pad,boot-gather,game-flash,pair" \
    "demo-card,touch,link,store,pad,core2,boot-gather,xip-probe"; do
    case ",$joined," in *,core2,*) cores=two ;; *) cores=one ;; esac
    case ",$joined," in *,xip-probe,*) xip=() ;; *) xip=(--absent) ;; esac
    case ",$joined," in *,game-flash,*) gflash=("${CYD_GAME_FILES:-$here/target/chip-games/files}") ;; *) gflash=(--absent) ;; esac
    printf '%-55s ' "[$joined]"
    total=$((total + 1))
    if ! err="$(cargo build --release --offline --quiet --no-default-features --features "$joined" 2>&1)"; then
        echo "FAILED (build)"
        echo "$err" | grep -o '\(HOT BLOCK\|GAME SLOT\):.*' || echo "$err" | tail -n 20
        failed=$((failed + 1))
        continue
    fi
    elf="$CARGO_TARGET_DIR/xtensa-esp32-none-elf/release/pocketme-cyd"
    # The game slots' start-up path must be in the firmware: once the
    # compiler took the slot check for always false (two different
    # statics' addresses) and dropped it, so every game was refused.
    missing=""
    for text in "game slots ok: code " "game slots: spill mirror ok, " "spill mirror WRONG: writing I "; do
        grep -qaF "$text" "$elf" || missing="$missing \"$text\""
    done
    if [ -n "$missing" ]; then
        echo "FAILED (game slots)"
        echo "the firmware has no log line$missing: the game slots' start-up check was compiled out"
        failed=$((failed + 1))
        continue
    fi
    if [ "${gflash[0]}" != --absent ]; then
        for text in "game flash: code " "flash part ok, code " "flash part DIFFERS: "; do
            grep -qaF "$text" "$elf" || missing="$missing \"$text\""
        done
    fi
    if [ -n "$missing" ]; then
        echo "FAILED (game flash)"
        echo "the firmware has no log line$missing: the flash part check was compiled out"
        failed=$((failed + 1))
        continue
    fi
    if ! out="$(python3 "$here/hot-check.py" "$elf" --cores "$cores" 2>&1)"; then
        echo "FAILED (hot-check)"
        echo "$out"
        failed=$((failed + 1))
    elif ! xout="$(python3 "$here/xip-check.py" "$elf" "${xip[@]}" 2>&1)"; then
        echo "FAILED (xip-check)"
        echo "$xout"
        failed=$((failed + 1))
    elif ! gout="$(python3 "$here/game-flash-check.py" "$elf" "${gflash[@]}" 2>&1)"; then
        echo "FAILED (game flash)"
        echo "$gout"
        failed=$((failed + 1))
    else
        echo ok
        echo "$out" | tail -n 1
        echo "$xout" | tail -n 1
        echo "$gout" | tail -n 1
    fi
done

if ((failed > 0)); then
    echo "$failed of $total builds failed"
    exit 1
fi
echo "all $total builds ok"
