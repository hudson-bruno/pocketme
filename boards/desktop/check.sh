#!/usr/bin/env bash
# Checks the console's PC crates, all in one go. No window opens, no
# board is needed, nothing is flashed.
#
#   0. Every game in games/ built for the chip, linked for its two slots
#      and checked to fit: code at most 56 KB, data at most 48 KB
#      (games/build-chip.sh; it prints the sizes). Overlapped in the
#      background while non-chip PC crates are checked.
#   1. For each selected crate: format check, clippy (warnings count as
#      errors), and tests (via cargo-nextest when available, plus
#      `cargo test --doc`, falling back to `cargo test`). Crates without
#      `[features]` are tested once instead of twice. Games with identical
#      feature graphs are batched together for full multi-core parallelism.
#      `boards/desktop` runs with default, `--no-default-features`, and
#      `--all-features` (window tests use SDL's "offscreen" driver).
#   2. The shared crates built for two targets with no standard library
#      at all, to prove they can run on the chip:
#        - the ESP32 itself (xtensa-esp32-none-elf), "esp" toolchain;
#        - a bare PC (x86_64-unknown-none), "nightly" toolchain.
#      Both run in parallel and build `core` from source (-Zbuild-std=core).
#
# The root Cargo.toml is an explicit virtual workspace covering these 27
# crates (excluding boards/cyd, tools/, experiments/, and crates not listed
# here), with `[profile.dev] opt-level = 2` (keeping overflow checks and
# debug assertions enabled).
#
# Usage: boards/desktop/check.sh [OPTIONS] [CRATE...]   (from anywhere)
#   Options:
#     --no-chip      Skip games/build-chip.sh
#     --no-nostd     Skip xtensa-esp32-none-elf and x86_64-unknown-none builds
#     --cargo-test   Use plain `cargo test` instead of `cargo nextest`
#     -h, --help     Show this help
#   CRATE can be a path (e.g. games/mercator, boards/desktop) or a package
#   name (e.g. mercator, desktop-board). With none, checks all 27 crates.
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
TARGET="$HERE/target/check"
TC="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$HERE/rust-toolchain.toml")"
if [ -z "$TC" ]; then
    echo "check.sh: could not read toolchain channel from $HERE/rust-toolchain.toml" >&2
    exit 1
fi

CRATES=(
    crates/game-api
    crates/game-kit
    crates/game-3d
    crates/board-api
    crates/console-video
    crates/console-core
    boards/desktop
    games/padtest
    games/stardust
    games/lander
    games/rooftop
    games/lantern
    games/hairpin
    games/kestrel
    games/tundra
    games/skimmer
    games/camber
    games/mercator
    games/bench
    games/drive3d
    games/proto3d
    crates/game-loader
    crates/settings-store
    crates/touch-filter
    crates/pad-store
    crates/pad-proto
    crates/pad-host
)

SHARED=(
    crates/game-api
    crates/game-kit
    crates/game-3d
    crates/board-api
    crates/console-video
    crates/console-core
    crates/game-loader
    crates/settings-store
    crates/touch-filter
    crates/pad-store
    crates/pad-proto
    crates/pad-host
)

# 13 games sharing an identical default feature graph (all pull in
# game-kit/testing via dev-dependencies; bench is separate because it does not).
BATCH_GAMES=(
    games/padtest
    games/stardust
    games/lander
    games/rooftop
    games/lantern
    games/hairpin
    games/kestrel
    games/tundra
    games/skimmer
    games/camber
    games/mercator
    games/drive3d
    games/proto3d
)

LIB_CRATES=(
    crates/game-api
    crates/game-kit
    crates/game-3d
    crates/board-api
    crates/console-video
    crates/console-core
    crates/settings-store
    crates/touch-filter
    crates/pad-store
    crates/pad-proto
    crates/pad-host
)

PARTS="bt-classic,passby,gps,sound"

usage() {
    sed -n '2,/^[^#]/{/^#/s/^# \{0,1\}//p;}' "$0"
}

pkg_of() {
    case "$1" in
        boards/desktop) echo "desktop-board" ;;
        *) basename "$1" ;;
    esac
}

resolve_crate() {
    local arg="$1"
    arg="${arg#./}"
    while [ "${arg%/}" != "$arg" ]; do
        arg="${arg%/}"
    done
    local c
    for c in "${CRATES[@]}"; do
        if [ "$arg" = "$c" ] || [ "$arg" = "$(pkg_of "$c")" ]; then
            echo "$c"
            return 0
        fi
    done
    return 1
}

NO_CHIP=0
NO_NOSTD=0
FORCE_CARGO_TEST=0
REQ_ARGS=()

while [ "$#" -gt 0 ]; do
    case "$1" in
        --no-chip) NO_CHIP=1; shift ;;
        --no-nostd) NO_NOSTD=1; shift ;;
        --cargo-test) FORCE_CARGO_TEST=1; shift ;;
        -h|--help) usage; exit 0 ;;
        --)
            shift
            while [ "$#" -gt 0 ]; do
                REQ_ARGS+=("$1")
                shift
            done
            ;;
        -*)
            echo "check.sh: unknown option: $1" >&2
            usage >&2
            exit 1
            ;;
        *)
            REQ_ARGS+=("$1")
            shift
            ;;
    esac
done

SELECTED=()
add_selected() {
    local cand="$1" existing
    for existing in "${SELECTED[@]:-}"; do
        if [ "$existing" = "$cand" ]; then
            return 0
        fi
    done
    SELECTED+=("$cand")
}

if [ "${#REQ_ARGS[@]}" -eq 0 ]; then
    SELECTED=("${CRATES[@]}")
else
    for arg in "${REQ_ARGS[@]}"; do
        if ! resolved="$(resolve_crate "$arg")"; then
            echo "check.sh: unknown crate: $arg" >&2
            exit 1
        fi
        add_selected "$resolved"
    done
fi

is_selected() {
    local target="$1" c
    for c in "${SELECTED[@]}"; do
        if [ "$c" = "$target" ]; then
            return 0
        fi
    done
    return 1
}

has_features() {
    grep -q '^\[features\]' "$ROOT/$1/Cargo.toml"
}

USE_NEXTEST=1
if [ "$FORCE_CARGO_TEST" -eq 1 ]; then
    USE_NEXTEST=0
elif ! cargo "+$TC" nextest --version >/dev/null 2>&1; then
    echo "check: cargo-nextest not found; falling back to cargo test" >&2
    USE_NEXTEST=0
fi

TMP="$(mktemp -d)"
CHIP_PID=""
ESP_PID=""
NIGHTLY_PID=""

cleanup() {
    local pid
    for pid in "$CHIP_PID" "$ESP_PID" "$NIGHTLY_PID"; do
        if [ -n "$pid" ]; then
            kill "$pid" 2>/dev/null || true
        fi
    done
    rm -rf "$TMP"
}
trap cleanup EXIT

run_tests() {
    if [ "$USE_NEXTEST" -eq 1 ]; then
        cargo "+$TC" nextest run --offline --locked --target-dir "$TARGET" \
            --cargo-quiet --status-level=fail --show-progress=none --no-tests=pass "$@"
        cargo "+$TC" test --offline --locked --target-dir "$TARGET" \
            -q --doc "$@"
    else
        cargo "+$TC" test --offline --locked --target-dir "$TARGET" \
            -q "$@"
    fi
}

# Phase 0: chip games build (needed only by crates/game-loader and boards/desktop).
NEED_CHIP=0
if [ "$NO_CHIP" -eq 0 ] && { is_selected "boards/desktop" || is_selected "crates/game-loader"; }; then
    NEED_CHIP=1
fi

HAS_NON_CHIP=0
for c in "${SELECTED[@]}"; do
    if [ "$c" != "boards/desktop" ] && [ "$c" != "crates/game-loader" ]; then
        HAS_NON_CHIP=1
        break
    fi
done

if [ "$NEED_CHIP" -eq 1 ]; then
    export GAME_FILES_DIR="$TARGET/chip-games/files"
    if [ "$HAS_NON_CHIP" -eq 1 ]; then
        echo "== games for the chip (running in background)"
        GAMES_TARGET="$TARGET" "$ROOT/games/build-chip.sh" >"$TMP/build-chip.log" 2>&1 &
        CHIP_PID=$!
    else
        echo "== games for the chip"
        GAMES_TARGET="$TARGET" "$ROOT/games/build-chip.sh"
    fi
fi

cd "$ROOT"

# Phase 1: formatting check for selected crates only.
echo "== format (${#SELECTED[@]} crates)"
fmt_args=()
for c in "${SELECTED[@]}"; do
    fmt_args+=(-p "$(pkg_of "$c")")
done
cargo "+$TC" fmt --check "${fmt_args[@]}"

# Phase 2A: non-chip-dependent PC crates (clippy + tests).
game_p_args=()
game_names=()
for g in "${BATCH_GAMES[@]}"; do
    if is_selected "$g"; then
        pkg="$(pkg_of "$g")"
        game_p_args+=(-p "$pkg")
        game_names+=("$pkg")
    fi
done

if [ "${#game_p_args[@]}" -gt 0 ]; then
    echo "== games (${game_names[*]})"
    cargo "+$TC" clippy --offline --locked --target-dir "$TARGET" \
        --all-targets "${game_p_args[@]}" -- -D warnings
    run_tests "${game_p_args[@]}"
    if is_selected "games/camber"; then
        echo "== games/camber (--all-features)"
        cargo "+$TC" clippy --offline --locked --target-dir "$TARGET" \
            --all-targets -p camber --all-features -- -D warnings
        run_tests -p camber --all-features
    fi
fi

if is_selected "games/bench"; then
    echo "== games/bench"
    cargo "+$TC" clippy --offline --locked --target-dir "$TARGET" \
        --all-targets -p bench -- -D warnings
    run_tests -p bench
    echo "== games/bench (--all-features)"
    cargo "+$TC" clippy --offline --locked --target-dir "$TARGET" \
        --all-targets -p bench --all-features -- -D warnings
    run_tests -p bench --all-features
fi

for c in "${LIB_CRATES[@]}"; do
    if ! is_selected "$c"; then
        continue
    fi
    pkg="$(pkg_of "$c")"
    echo "== $c"
    cargo "+$TC" clippy --offline --locked --target-dir "$TARGET" \
        --all-targets -p "$pkg" -- -D warnings
    run_tests -p "$pkg"
    if has_features "$c"; then
        echo "== $c (--all-features)"
        cargo "+$TC" clippy --offline --locked --target-dir "$TARGET" \
            --all-targets -p "$pkg" --all-features -- -D warnings
        run_tests -p "$pkg" --all-features
    fi
done

# Wait for background chip games build before running crates that read GAME_FILES_DIR.
if [ -n "$CHIP_PID" ]; then
    rc=0
    wait "$CHIP_PID" || rc=$?
    CHIP_PID=""
    echo "== games for the chip (output)"
    cat "$TMP/build-chip.log"
    if [ "$rc" -ne 0 ]; then
        exit "$rc"
    fi
fi

# Phase 2B: chip-dependent PC crates.
if is_selected "crates/game-loader"; then
    echo "== crates/game-loader"
    cargo "+$TC" clippy --offline --locked --target-dir "$TARGET" \
        --all-targets -p game-loader -- -D warnings
    run_tests -p game-loader
    echo "== crates/game-loader (--all-features)"
    cargo "+$TC" clippy --offline --locked --target-dir "$TARGET" \
        --all-targets -p game-loader --all-features -- -D warnings
    run_tests -p game-loader --all-features
fi

if is_selected "boards/desktop"; then
    for mode in default no-default all; do
        case "$mode" in
            default) f_args=(); label="default features" ;;
            no-default) f_args=(--no-default-features); label="--no-default-features" ;;
            all) f_args=(--all-features); label="--all-features" ;;
        esac
        echo "== boards/desktop ($label)"
        cargo "+$TC" clippy --offline --locked --target-dir "$TARGET" \
            --all-targets -p desktop-board "${f_args[@]}" -- -D warnings
        run_tests -p desktop-board "${f_args[@]}"
    done
fi

# Phase 3: no-std cross-compilation builds (esp + nightly in parallel).
SHARED_SELECTED=()
for c in "${SHARED[@]}"; do
    if is_selected "$c"; then
        SHARED_SELECTED+=("$c")
    fi
done

run_nostd_target() {
    local tc="$1" triple="$2" tdir="$3"
    local c all mode f_args
    for c in "${SHARED_SELECTED[@]}"; do
        case "$c" in
            *game-api|*game-kit|*game-3d|*touch-filter|*pad-store) all="" ;;
            *console-video) all="swap" ;;
            *console-core) all="$PARTS,swap,boot-gather,settings" ;;
            *pad-proto) all="pair" ;;
            *pad-host) all="$PARTS,pair" ;;
            *) all="$PARTS" ;;
        esac
        for mode in default ${all:+features}; do
            if [ "$mode" = "default" ]; then
                f_args=()
                echo "== $c for $triple (toolchain $tc)"
            else
                f_args=(--features "$all")
                echo "== $c for $triple (toolchain $tc) --features $all"
            fi
            (
                cd "$ROOT/$c"
                cargo "+$tc" build --offline -q -Zbuild-std=core --target "$triple" \
                    --target-dir "$tdir" --config 'profile.dev.opt-level=0' "${f_args[@]}"
            )
        done
    done
}

if [ "$NO_NOSTD" -eq 0 ] && [ "${#SHARED_SELECTED[@]}" -gt 0 ]; then
    # The esp toolchain needs its environment for some builds; harmless here.
    if [ -f "$HOME/export-esp.sh" ]; then
        # shellcheck disable=SC1091
        . "$HOME/export-esp.sh" >/dev/null 2>&1 || true
    fi
    run_nostd_target esp xtensa-esp32-none-elf "$TARGET/no-std/esp" \
        >"$TMP/nostd-esp.log" 2>&1 &
    ESP_PID=$!
    run_nostd_target nightly x86_64-unknown-none "$TARGET/no-std/x86_64" \
        >"$TMP/nostd-nightly.log" 2>&1 &
    NIGHTLY_PID=$!

    esp_rc=0
    wait "$ESP_PID" || esp_rc=$?
    ESP_PID=""
    nightly_rc=0
    wait "$NIGHTLY_PID" || nightly_rc=$?
    NIGHTLY_PID=""

    cat "$TMP/nostd-esp.log"
    cat "$TMP/nostd-nightly.log"
    if [ "$esp_rc" -ne 0 ] || [ "$nightly_rc" -ne 0 ]; then
        exit 1
    fi
fi

# Phase 4: simulator CLI smoke test and no-SDL3 dependency check.
if is_selected "boards/desktop"; then
    echo "== boards/desktop (smoke & no-sdl3 tree)"
    cargo "+$TC" run --offline --locked -p desktop-board --bin pocketme-sim \
        --target-dir "$TARGET" -q -- --quiet --frames 3 --png "$TARGET/check-shot.png"
    if cargo "+$TC" tree --offline --locked -p desktop-board --no-default-features -e normal | grep -q sdl3; then
        echo "check: the no-window build pulls in sdl3" >&2
        exit 1
    fi
fi

echo "check: ok"
