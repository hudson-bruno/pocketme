#!/usr/bin/env python3
"""Says in a few lines what a log of the loader console shows.

    tools/log-summary.py logs/plain.log

The firmware starts every line that carries a result with a word in
capitals. This picks those lines out, counts the starts, the panics and
the processor faults, and boils the endless SOAK lines down to their
smallest, middle and largest numbers. It ends with one line, PASS or FAIL,
and the exit status says the same (0 or 1).
"""

import re
import statistics
import sys

KEEP = ("MEM ", "LOAD ", "PLAY ", "CHECKS ", "REFUSED", "NOT REFUSED", "FULLSIZE", "SD", "PROBE", "LAST START", "SUMMARY", "CRASH-TEST", "Bluetooth:", "reset reason", "slots:", "main stack:", "loader-console ")
TROUBLE = ("panicked", "Exception occurred", "Guru", "abort", "FAILED", "FAIL ", "ONLY ", "Brownout", "Backtrace", "stack overflow")
SOAK = re.compile(r"SOAK round (\d+) \(([a-z-]+)\): ok\. load (\d+) us \(read (\d+) us\), (\d+) of (\d+) checks, update avg (\d+) us max (\d+) us; failed rounds so far (\d+); heap free (\d+) most-used (\d+); stack deepest (\S+)")


def spread(values):
    return f"min {min(values)}, median {int(statistics.median(values))}, max {max(values)}"


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    lines = open(sys.argv[1], encoding="utf-8", errors="replace").read().splitlines()

    starts = sum("loader-console " in line and "Bluetooth" in line for line in lines)
    soak = {}
    soak_failed = 0
    trouble = []
    print(f"== {sys.argv[1]}: {len(lines)} lines, the firmware started {starts} time(s)")
    for line in lines:
        text = line.split("] ", 1)[-1] if line.startswith("[") else line
        body = re.sub(r"^(INFO|WARN|ERROR) - ", "", text)
        found = SOAK.search(body)
        if found:
            soak.setdefault(found.group(2), []).append(found)
            continue
        if "SOAK round" in body and "FAILED" in body:
            soak_failed += 1
        if any(word in body for word in TROUBLE):
            trouble.append(line)
        if body.startswith(KEEP):
            print("  " + text)

    for source, rounds in soak.items():
        last = rounds[-1]
        print(f"  SOAK {source}: {len(rounds)} good rounds")
        print(f"       load time us: {spread([int(r.group(3)) for r in rounds])}")
        print(f"       of which source read us: {spread([int(r.group(4)) for r in rounds])}")
        print(f"       update avg us: {spread([int(r.group(7)) for r in rounds])};  update max us: {spread([int(r.group(8)) for r in rounds])}")
        print(f"       at the last round: heap free {last.group(10)}, heap most-used {last.group(11)}, main stack deepest {last.group(12)}")
    if soak_failed:
        print(f"  SOAK: {soak_failed} FAILED rounds")

    if trouble:
        print(f"== {len(trouble)} line(s) that speak of trouble:")
        for line in trouble[:40]:
            print("  " + line)

    checks = [line for line in lines if "CHECKS " in line]
    bad_checks = [line for line in checks if "ONLY" in line or "could not" in line]
    summary_ok = any("SUMMARY: every load and every check passed" in line for line in lines)
    ok = bool(checks) and not bad_checks and not soak_failed and summary_ok
    print("== PASS" if ok else "== FAIL (or a build that is meant to stop: probe, crash-test; read the lines above)")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
