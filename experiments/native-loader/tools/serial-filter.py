#!/usr/bin/env python3
"""Cleans up what comes from the board's serial port, line by line.

Reads the raw output of `espflash ... --monitor` on standard input and
writes to standard output:

  * every line with the time since this filter started in front of it, so
    that a log says when things happened even where the firmware does not;
  * without the colour codes the firmware's logger adds;
  * WITHOUT ANY GPS SENTENCE. A GPS module may share the serial pins, and
    its lines carry positions. Such a line is dropped here, before it can
    reach a file or a screen, and only counted;
  * with hardware addresses (the "MAC address" espflash prints) blanked.

It must sit directly after espflash in the pipe, before `tee`.
"""

import re
import sys
import time

ANSI = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")
# "$GPGGA,", "$GNRMC,", "$BDGSV," ... anywhere in the line: the start of the
# line may have been lost.
NMEA = re.compile(r"\$?(GP|GN|GL|GA|GB|BD|QZ)(GGA|GLL|GSA|GSV|RMC|VTG|ZDA|TXT|GNS|GST)\b")
MAC = re.compile(r"\b([0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}\b")


def main() -> None:
    started = time.monotonic()
    dropped = 0
    stream = sys.stdin.buffer
    for raw in stream:
        # espflash redraws its progress line with carriage returns.
        for piece in raw.decode("utf-8", "replace").replace("\r", "\n").split("\n"):
            line = ANSI.sub("", piece).rstrip()
            if not line:
                continue
            if NMEA.search(line):
                dropped += 1
                continue
            line = MAC.sub("xx:xx:xx:xx:xx:xx", line)
            sys.stdout.write(f"[{time.monotonic() - started:7.2f}] {line}\n")
            sys.stdout.flush()
    if dropped:
        sys.stdout.write(f"[{time.monotonic() - started:7.2f}] (serial-filter: {dropped} GPS lines dropped unread)\n")


if __name__ == "__main__":
    main()
