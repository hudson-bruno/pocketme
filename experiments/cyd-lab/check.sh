#!/usr/bin/env bash
# Checks the CYD firmware without the board: formatting, clippy, and a
# build of every feature combination (6 features: 64 builds, plus one
# with radio-fit; about 10 minutes). Never opens the serial port, never
# flashes.
#
# Run from anywhere:  boards/cyd/check.sh
# Builds go to target/check, so the firmware in target/ (the one you
# flash) is left as it is.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
cd "$here"
export PATH="$HOME/.cargo/bin:$PATH"
# The esp toolchain's settings (the Xtensa linker).
# shellcheck disable=SC1091
. "$HOME/export-esp.sh" >/dev/null 2>&1
export CARGO_TARGET_DIR="$here/target/check"

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

echo "== clippy"
for set in "" "--no-default-features" "--all-features"; do
    echo "-- clippy ${set:-(default features)}"
    cargo clippy --release --offline --quiet $set -- -D warnings
done

echo "== build every feature combination"
features=(demo-card sd-card touch link tour wide-strips)
n=${#features[@]}
failed=0
for ((mask = 0; mask < (1 << n); mask++)); do
    list=()
    for ((i = 0; i < n; i++)); do
        if (((mask >> i) & 1)); then list+=("${features[$i]}"); fi
    done
    joined="$(IFS=,; echo "${list[*]-}")"
    printf '%-55s ' "[${joined:-none}]"
    if cargo build --release --offline --quiet --no-default-features --features "$joined" 2>/dev/null; then
        echo ok
    else
        echo FAILED
        failed=$((failed + 1))
    fi
done

# The radio measurement (radio-fit) on top of the default features. Not
# in the combinations above: it would double them, and it is measuring
# only.
printf '%-55s ' "[default + radio-fit]"
if cargo build --release --offline --quiet --features radio-fit 2>/dev/null; then
    echo ok
else
    echo FAILED
    failed=$((failed + 1))
fi

if ((failed > 0)); then
    echo "$failed of $(((1 << n) + 1)) builds failed"
    exit 1
fi
echo "all $(((1 << n) + 1)) builds ok"
