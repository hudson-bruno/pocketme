#!/usr/bin/env python3
"""Read the ble-scan firmware's serial log and say what happened.

The firmware prints one tagged line per fact (DEV, MEM, STAT, PHASE, ...).
This tool turns a log into a short report, so that nobody has to read
thousands of lines, or look at the board through a camera, to learn
whether a run went well.

Two ways to use it:

  1. On a log file captured earlier (for example by `espflash ... --monitor`):

         tools/scanlog.py run.log

  2. Straight from the board. The tool opens the serial port itself, can
     restart the board so the log begins at the start, and reads for a
     set time. The board is shared, so ALWAYS run it under the lock and
     a timeout:

         flock <board.lock> timeout 150 tools/scanlog.py --port /dev/ttyUSB0 --reset --secs 60 --save run.log

What the report holds is safe to paste anywhere: counts and categories,
never an address or a name. The one exception is a device that looks like
a game controller, which is shown in full, because finding it is the
point. `--devices` adds the full list of everything heard; that list
names the neighbours' devices, so keep it to yourself.

Exit code: 0 = no fault found, 1 = the log has errors, a panic or a
refused memory request, 2 = the log has no ble-scan lines at all.

Only the standard library is used (no pyserial needed).
"""

import argparse
import json
import os
import re
import sys
import time

TAGS = (
    "BOOT", "PROBE", "DEV", "CONTROLLER-LIKE", "MEM", "STAT", "PHASE",
    "MINUTE", "SUMMARY", "ROTATE", "PASSBY", "ERROR", "RF",
)

# The colour codes the firmware's logger wraps its own lines in.
ANSI = re.compile(r"\x1b\[[0-9;]*m")
# key=value words. A value in double quotes may hold spaces.
WORD = re.compile(r'(\w[\w.+-]*)=("(?:[^"\\]|\\.)*"|\S+)')

# Company numbers that turn up most, to make the report readable. Anything
# not listed is shown as its number.
COMPANIES = {
    0x0006: "Microsoft",
    0x004C: "Apple",
    0x0075: "Samsung",
    0x00E0: "Google",
    0x0087: "Garmin",
    0x0171: "Amazon",
    0x038F: "Xiaomi",
    0xFFFF: "test number (our own beacon uses it)",
}

SERVICES = {
    "1812": "HID (keyboards, mice, game controllers)",
    "180f": "battery",
    "180a": "device information",
    "fe2c": "Google Fast Pair",
    "fd6f": "exposure notification",
    "fcf1": "Google",
    "fef3": "Google",
    "fd5a": "Samsung",
    "fe9f": "Google",
}


def fields(line):
    """The key=value words of a line, as a dict of strings."""
    return {key: value for key, value in WORD.findall(line)}


def number(text, default=None):
    try:
        return int(text, 0)
    except (TypeError, ValueError):
        return default


class Report:
    """Everything learnt from one log."""

    def __init__(self):
        self.lines = 0
        self.tagged = 0
        self.boots = []            # BOOT lines, as printed
        self.restarts = 0          # how often the firmware started
        self.probes = []           # PROBE lines, as printed
        self.host = []             # the Bluetooth library's own start-up lines
        self.phases = []           # dicts from "PHASE end"
        self.minutes = []          # dicts from "MINUTE"
        self.rotations = []        # (while_scanning, result)
        self.errors = []           # ERROR lines and anything that looks like a crash
        self.devices = {}          # address -> latest DEV fields
        self.device_lines = {}     # address -> latest DEV line, as printed
        self.controllers = {}      # address -> latest CONTROLLER-LIKE line
        self.passby = 0
        self.mem_first = None
        self.mem_last = None
        self.mem_count = 0
        self.heap_free_min = None
        self.heap_min_free = None
        self.heap_largest_min = None
        self.heap_refused = 0
        self.heap_requests = 0
        self.stack_used_max = 0
        self.stack_unknown = False
        self.stack_size = None
        self.heap_size = None
        self.stat_last = None
        self.summary = {}          # the latest SUMMARY pair, merged
        self.rf = []               # dicts from the rf-check program
        self.app_size = None
        self.last_t = 0

    # ------------------------------------------------------------ reading

    def feed(self, raw):
        line = ANSI.sub("", raw).rstrip("\r\n")
        self.lines += 1

        # A serial line can start with rubbish (a GPS module shares the
        # wire, and a reset cuts lines in half). So the tag is looked for
        # anywhere in the line, and what stands before it is dropped.
        tag, rest = None, ""
        for candidate in TAGS:
            at = line.find(candidate + " ")
            if at >= 0 and (at == 0 or not line[at - 1].isalnum()):
                if tag is None or at < line.find(tag + " "):
                    tag, rest = candidate, line[at:]
        if tag is None:
            self.other(line)
            return
        self.tagged += 1
        kv = fields(rest)
        t = number(kv.get("t"))
        if t is not None:
            self.last_t = max(self.last_t, t)
        getattr(self, "on_" + tag.lower().replace("-", "_"))(rest, kv)

    def other(self, line):
        low = line.lower()
        if "[host]" in line:
            # The library prints the board's own address here; that is
            # not needed in a report.
            if "Device Address" not in line:
                self.host.append(line.split("[host]", 1)[1].strip())
        elif "App/part. size" in line:
            self.app_size = line.strip()
        elif any(mark in low for mark in ("panicked", "backtrace", "guru meditation", "exception occurred", "stack overflow")):
            self.errors.append(line.strip())

    def on_boot(self, line, kv):
        if "plan=" in line or "rf-check" in line:
            self.restarts += 1
        self.boots.append(line)

    def on_probe(self, line, kv):
        if line not in self.probes:
            self.probes.append(line)

    def on_dev(self, line, kv):
        addr = kv.get("addr")
        if addr:
            self.devices[addr] = kv
            self.device_lines[addr] = line

    def on_controller_like(self, line, kv):
        self.controllers[kv.get("addr", "?")] = line

    def on_mem(self, line, kv):
        self.mem_count += 1
        if self.mem_first is None:
            self.mem_first = line
        self.mem_last = line
        free = number(kv.get("heap_free"))
        low = number(kv.get("heap_min_free"))
        largest = number(kv.get("heap_largest"))
        if free is not None:
            self.heap_free_min = free if self.heap_free_min is None else min(self.heap_free_min, free)
        if low is not None:
            self.heap_min_free = low if self.heap_min_free is None else min(self.heap_min_free, low)
        # The very first line is printed before the radio has taken its
        # memory; the largest block only means something after that.
        if largest is not None and number(kv.get("heap_requests"), 0) > 0:
            self.heap_largest_min = largest if self.heap_largest_min is None else min(self.heap_largest_min, largest)
        self.heap_refused = max(self.heap_refused, number(kv.get("heap_refused"), 0))
        self.heap_requests = max(self.heap_requests, number(kv.get("heap_requests"), 0))
        used = number(kv.get("stack_used"))
        if used is None:
            self.stack_unknown = True
        else:
            self.stack_used_max = max(self.stack_used_max, used)
        self.stack_size = number(kv.get("stack_size"), self.stack_size)
        self.heap_size = number(kv.get("heap_size"), self.heap_size)

    def on_stat(self, line, kv):
        self.stat_last = kv

    def on_phase(self, line, kv):
        if line.startswith("PHASE end"):
            self.phases.append(kv)

    def on_minute(self, line, kv):
        self.minutes.append(kv)

    def on_summary(self, line, kv):
        self.summary.update(kv)

    def on_rotate(self, line, kv):
        # The result can hold spaces ("Command Disallowed"), so it is taken
        # whole, to the end of the line.
        result = line.split("result=", 1)[1].strip() if "result=" in line else "?"
        self.rotations.append((kv.get("while_scanning"), result))

    def on_passby(self, line, kv):
        self.passby += 1

    def on_error(self, line, kv):
        self.errors.append(line)

    def on_rf(self, line, kv):
        if "access_points" in kv:
            self.rf.append(kv)
        if "ERROR" in line:
            self.errors.append(line)

    # ---------------------------------------------------------- reporting

    def faults(self):
        found = list(self.errors)
        if self.heap_refused:
            found.append(f"the heap refused {self.heap_refused} request(s)")
        if self.restarts > 1:
            found.append(f"the firmware started {self.restarts} times in this log (a restart in the middle of a run is a crash)")
        if self.stat_last and number(self.stat_last.get("errors"), 0):
            found.append(f"the firmware counted {self.stat_last['errors']} error(s)")
        return found

    def as_dict(self):
        """The report as plain data, for `--json`. No address, no name."""
        devices = list(self.devices.values())
        by_type, by_company, by_service = {}, {}, {}
        named = 0
        strongest = None
        for d in devices:
            by_type[d.get("type", "?")] = by_type.get(d.get("type", "?"), 0) + 1
            company = d.get("company", "-")
            if company != "-":
                by_company[company] = by_company.get(company, 0) + 1
            for uuid in filter(None, d.get("uuid16", "-").strip("-").split(",")):
                by_service[uuid] = by_service.get(uuid, 0) + 1
            if d.get("name", '""') not in ('""', "-"):
                named += 1
            rssi = number(d.get("rssi"))
            if rssi is not None:
                strongest = rssi if strongest is None else max(strongest, rssi)
        return {
            "lines": self.lines,
            "tagged_lines": self.tagged,
            "seconds": self.last_t,
            "restarts": self.restarts,
            "faults": self.faults(),
            "probes": self.probes,
            "phases": [
                {key: (number(p.get(key), p.get(key))) for key in ("n", "name", "secs", "adv", "adv_per_min", "new_devices", "errors_total")}
                for p in self.phases
            ],
            "minutes": [
                {key: (number(m.get(key), m.get(key))) for key in ("n", "name", "t", "adv", "errors_total")}
                for m in self.minutes
            ],
            "rotations": [{"while_scanning": w, "result": r} for w, r in self.rotations],
            "devices": {
                "count": len(devices),
                "named": named,
                "by_address_type": by_type,
                "by_company": by_company,
                "by_service_uuid16": by_service,
                "strongest_rssi_dbm": strongest,
                "controller_like": len(self.controllers),
                "passby_beacons": self.passby,
            },
            "controller_like": list(self.controllers.values()),
            "memory": {
                "mem_lines": self.mem_count,
                "heap_size": self.heap_size,
                "heap_free_lowest_sampled": self.heap_free_min,
                "heap_min_free": self.heap_min_free,
                "heap_largest_block_lowest": self.heap_largest_min,
                "heap_requests": self.heap_requests,
                "heap_refused": self.heap_refused,
                "stack_used_max": None if self.stack_unknown else self.stack_used_max,
                "stack_size": self.stack_size,
            },
            "last_stat": self.stat_last,
            "summary": self.summary,
            "rf_check": self.rf,
            "app_size": self.app_size,
        }

    def text(self, show_devices=False):
        out = []
        say = out.append
        data = self.as_dict()

        say(f"log: {self.lines} lines, {self.tagged} from the firmware, {self.last_t} s of running")
        if self.app_size:
            say(f"flash: {self.app_size}")
        for line in self.boots:
            say("  " + line)
        for line in self.host:
            say("  [host] " + line)
        for line in self.probes:
            say("  " + line)

        if self.phases or self.minutes:
            say("")
            say("advertisements heard:")
            for p in self.phases:
                say(f"  phase {p.get('name', '?'):<16} {p.get('secs', '?'):>4} s  heard {p.get('adv', '?'):>5}  = {p.get('adv_per_min', '?'):>5} a minute  new devices {p.get('new_devices', '?')}")
            for m in self.minutes:
                say(f"  minute {m.get('n', '?'):>3} of {m.get('name', '?'):<16} heard {m.get('adv', '?'):>5}")
        if self.stat_last:
            s = self.stat_last
            say(f"  at the end (t={s.get('t')}): {s.get('adv_total')} heard in all, {s.get('devices')} devices, "
                f"{s.get('not_stored')} not stored (table full), {s.get('broken')} broken reports, {s.get('errors')} errors")

        if self.rotations:
            say("")
            say("beacon address changes:")
            tally = {}
            for scanning, result in self.rotations:
                key = f"while_scanning={scanning} -> {result}"
                tally[key] = tally.get(key, 0) + 1
            for key, count in tally.items():
                say(f"  {count} x {key}")

        d = data["devices"]
        if self.devices or self.summary:
            say("")
            say(f"devices in this log: {d['count']} (named: {d['named']}, strongest in any device line {d['strongest_rssi_dbm']} dBm)")
            for kind, count in sorted(d["by_address_type"].items()):
                say(f"  address type {kind}: {count}")
            for company, count in sorted(d["by_company"].items(), key=lambda item: -item[1]):
                name = COMPANIES.get(number(company, -1), "")
                say(f"  company {company}{' ' + name if name else ''}: {count}")
            for uuid, count in sorted(d["by_service_uuid16"].items(), key=lambda item: -item[1]):
                name = SERVICES.get(uuid.lower(), "")
                say(f"  service {uuid}{' ' + name if name else ''}: {count}")
            if self.summary:
                keys = ("devices", "connectable", "named", "with_flags", "with_uuid16", "with_uuid128", "with_svcdata",
                        "with_company", "with_appearance", "controller_like", "hid_service", "pad_appearance",
                        "name_hint_only", "adv_ind", "adv_direct_ind", "adv_scan_ind", "adv_nonconn_ind", "scan_rsp")
                say("  the firmware's own count at t=" + str(self.summary.get("t")) + ": "
                    + " ".join(f"{key}={self.summary[key]}" for key in keys if key in self.summary))
            if self.passby:
                say(f"  beacons of our own format heard: {self.passby}")

        say("")
        if self.controllers:
            say(f"GAME CONTROLLER CANDIDATES: {len(self.controllers)}")
            for line in self.controllers.values():
                say("  " + line)
        elif self.devices or self.summary:
            say("game controller candidates: none")

        if self.mem_count:
            m = data["memory"]
            say("")
            say(f"memory ({self.mem_count} looks, one a second):")
            say(f"  heap: {m['heap_size']} bytes; least free ever {m['heap_min_free']}; "
                f"smallest 'largest block' {m['heap_largest_block_lowest']}; "
                f"{m['heap_requests']} requests, {m['heap_refused']} refused")
            used = "more than was measured" if self.stack_unknown else f"{m['stack_used_max']}"
            say(f"  main stack: {used} of {m['stack_size']} bytes used at most")

        if self.rf:
            say("")
            say("receiver check by WiFi (rf-check program):")
            counts = [number(r.get("access_points"), 0) for r in self.rf]
            strong = [number(r.get("strongest_dbm")) for r in self.rf if "strongest_dbm" in r]
            say(f"  {len(self.rf)} scans, {min(counts)} to {max(counts)} access points each"
                + (f", strongest between {min(strong)} and {max(strong)} dBm" if strong else ""))

        if show_devices and self.device_lines:
            say("")
            say("every device (PRIVATE: names other people's devices, do not paste elsewhere):")
            for line in self.device_lines.values():
                say("  " + line)

        say("")
        faults = self.faults()
        if faults:
            say(f"FAULTS: {len(faults)}")
            for fault in faults[:40]:
                say("  " + fault)
        elif self.tagged:
            say("faults: none (no ERROR line, no panic, no restart, no refused memory request)")
        else:
            say("NOTHING from the ble-scan firmware in this log. Is it the firmware on the board?")
        return "\n".join(out)


# ------------------------------------------------------------- the port

def open_port(path, baud=115200):
    """Opens the serial port raw, with the standard library only."""
    import termios

    fd = os.open(path, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    attrs = termios.tcgetattr(fd)
    speed = getattr(termios, f"B{baud}")
    attrs[0] = 0                                   # no input translation
    attrs[1] = 0                                   # no output translation
    # 8 data bits, receiver on, ignore modem lines. HUPCL is left out, so
    # closing the port does not pull the board's reset line.
    attrs[2] = termios.CS8 | termios.CREAD | termios.CLOCAL
    attrs[3] = 0                                   # no echo, no line editing
    attrs[4] = speed
    attrs[5] = speed
    termios.tcsetattr(fd, termios.TCSANOW, attrs)
    return fd


def set_lines(fd, dtr, rts):
    """Sets the two modem lines. On this board RTS pulls the chip's reset
    pin and DTR its boot-mode pin."""
    import fcntl
    import struct
    import termios

    for bit, on in ((termios.TIOCM_DTR, dtr), (termios.TIOCM_RTS, rts)):
        fcntl.ioctl(fd, termios.TIOCMBIS if on else termios.TIOCMBIC, struct.pack("I", bit))


def read_port(path, secs, reset, save, echo, report):
    import select

    fd = open_port(path)
    try:
        # Both lines off = the chip runs. With `--reset`: reset pin down
        # for a moment, boot-mode pin left alone, so the chip starts the
        # firmware (and not its loader) from the beginning.
        set_lines(fd, dtr=False, rts=False)
        if reset:
            set_lines(fd, dtr=False, rts=True)
            time.sleep(0.12)
            set_lines(fd, dtr=False, rts=False)
        out = open(save, "w", encoding="utf-8", errors="replace") if save else None
        deadline = time.monotonic() + secs
        pending = b""
        while True:
            left = deadline - time.monotonic()
            if left <= 0:
                break
            ready, _, _ = select.select([fd], [], [], min(left, 0.5))
            if not ready:
                continue
            try:
                chunk = os.read(fd, 4096)
            except BlockingIOError:
                continue
            if not chunk:
                continue
            pending += chunk
            *whole, pending = pending.split(b"\n")
            for raw in whole:
                line = raw.decode("utf-8", errors="replace")
                if out:
                    out.write(line.rstrip("\r") + "\n")
                report.feed(line)
                if echo(line):
                    print(ANSI.sub("", line).rstrip("\r"), flush=True)
        if out:
            out.close()
    finally:
        os.close(fd)


def main():
    parser = argparse.ArgumentParser(description="Summarise the ble-scan firmware's serial log.", epilog=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("logs", nargs="*", help="log files captured earlier ('-' for standard input)")
    parser.add_argument("--port", help="read from this serial port instead (run under the board lock and a timeout)")
    parser.add_argument("--secs", type=float, default=60, help="with --port: how long to read (default 60)")
    parser.add_argument("--reset", action="store_true", help="with --port: restart the board first, so the log begins at the start")
    parser.add_argument("--save", help="with --port: also write the raw log to this file")
    parser.add_argument("--watch", choices=("quiet", "events", "all"), default="events",
                        help="with --port: what to show while reading. events = everything but the once-a-second lines "
                             "and the device lines (default); all = every line (shows addresses)")
    parser.add_argument("--devices", action="store_true", help="add the full list of devices (PRIVATE)")
    parser.add_argument("--json", action="store_true", help="print the report as JSON (no addresses, no names)")
    args = parser.parse_args()

    if not args.logs and not args.port:
        parser.error("give a log file, or --port")

    reports = []
    if args.port:
        if args.secs > 290:
            parser.error("--secs above 290: the board is shared, keep a session short")
        report = Report()

        def echo(line):
            if args.watch == "all":
                return True
            if args.watch == "quiet":
                return False
            plain = ANSI.sub("", line)
            return any(tag + " " in plain for tag in TAGS) and not any(
                plain.lstrip().startswith(skip) for skip in ("MEM ", "STAT ", "DEV "))

        read_port(args.port, args.secs, args.reset, args.save, echo, report)
        reports.append((args.port, report))
    for path in args.logs:
        report = Report()
        stream = sys.stdin if path == "-" else open(path, encoding="utf-8", errors="replace")
        with stream:
            for line in stream:
                report.feed(line)
        reports.append((path, report))

    status = 0
    if args.json:
        print(json.dumps({name: report.as_dict() for name, report in reports}, indent=2))
    for name, report in reports:
        if not args.json:
            print(f"==== {name}")
            print(report.text(show_devices=args.devices))
            print()
        if not report.tagged:
            status = max(status, 2)
        elif report.faults():
            status = max(status, 1)
    return status


if __name__ == "__main__":
    sys.exit(main())
